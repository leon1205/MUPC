"""MSSA (Multi-Strategy Sparrow Search Algorithm) 多策略麻雀搜索算法核心

实现"发现者-加入者-侦察者"三群体协同机制，集成佳点集初始化、反向学习增强、
Corsi 变异扰动等策略。支持 IPSO 降级模式。

算法流程见设计文档 Section 12.3.1。
"""

from __future__ import annotations

import logging
import math
import signal
import time
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional, Tuple

import numpy as np

try:
    from .config import MSSAConfig, PopulationConfig, EnhancementConfig, TerminationConfig
    from .search_space import SearchSpace, get_default_space
except ImportError:
    from config import MSSAConfig, PopulationConfig, EnhancementConfig, TerminationConfig
    from search_space import SearchSpace, get_default_space

logger = logging.getLogger(__name__)


# ============================================================================
# 优化结果数据结构
# ============================================================================


@dataclass
class OptimizationResult:
    """MSSA/IPS 单次优化运行结果。

    Attributes:
        best_position: 最优个体的编码向量。
        best_fitness: 最优目标函数值。
        best_hyperparams: 最优超参字典。
        convergence_curve: 每次迭代的全局最优值列表。
        per_parameter_trajectory: 每个超参的迭代轨迹。
        total_iterations: 实际完成的迭代次数。
        convergence_reason: 终止原因。
        elapsed_seconds: 总耗时。
        invalid_solutions: 无效解（训练失败）总数。
        cache_hits: 缓存命中次数。
        total_evaluations: 目标函数调用总次数。
        population_size: 种群大小。
        discoverer_count: 发现者数量。
        joiner_count: 加入者数量。
        scout_count: 侦察者数量。
        final_stagnation: 最终停滞计数。
        final_diversity: 最终种群多样性。
        stopped_early: 是否因 Ctrl+C 提前终止。
    """

    best_position: np.ndarray
    best_fitness: float
    best_hyperparams: Dict[str, Any]
    convergence_curve: List[float]
    per_parameter_trajectory: Dict[str, List[float]]
    total_iterations: int
    convergence_reason: str  # "max_iter" | "no_improvement" | "timeout"
    elapsed_seconds: float
    invalid_solutions: int = 0
    cache_hits: int = 0
    total_evaluations: int = 0
    population_size: int = 0
    discoverer_count: int = 0
    joiner_count: int = 0
    scout_count: int = 0
    final_stagnation: int = 0
    final_diversity: float = 0.0
    stopped_early: bool = False


# ============================================================================
# MSSA 优化器
# ============================================================================


# 黄金分割比 φ = (sqrt(5) - 1) / 2
GOLDEN_RATIO = (math.sqrt(5.0) - 1.0) / 2.0  # ≈ 0.6180339887


class MSSA:
    """MSSA 多策略麻雀搜索算法优化器。

    模拟麻雀群体觅食行为，在 D 维搜索空间中寻找全局最优解。
    支持 IPSO 降级模式。

    Usage:
        config = load_config("mssa_search_config.yaml")
        space = SearchSpace()
        optimizer = MSSA(config, space)
        result = optimizer.optimize(my_objective_fn)
    """

    def __init__(
        self,
        config: MSSAConfig,
        search_space: Optional[SearchSpace] = None,
    ):
        """
        Args:
            config: 搜索配置（MSSAConfig）。
            search_space: 搜索空间定义（None 则使用默认）。
        """
        self.config = config
        self.space = search_space or get_default_space()

        # 应用搜索空间覆盖
        if config.search_space_overrides:
            self.space.apply_overrides(config.search_space_overrides)

        # 种群参数
        self.pop_size: int = config.population.size
        self.p_discoverer: float = config.population.discoverer_ratio
        self.p_scout: float = config.population.scout_ratio

        # 角色数量
        self.n_discoverers: int = max(1, int(math.ceil(self.p_discoverer * self.pop_size)))
        self.n_scouts: int = max(1, int(math.ceil(self.p_scout * self.pop_size)))
        self.n_joiners: int = self.pop_size - self.n_discoverers - self.n_scouts
        if self.n_joiners < 0:
            self.n_joiners = 0
            self.n_discoverers = self.pop_size - self.n_scouts

        # 终止条件
        self.max_iter: int = config.termination.max_iterations
        self.epsilon: float = config.termination.epsilon
        self.no_improvement_rounds: int = config.termination.no_improvement_rounds
        self.max_wall_time: float = config.termination.max_wall_time_seconds

        # 增强策略（MSSA 专属）
        self.use_gps: bool = (
            config.enhancement.good_point_set if config.is_mssa else False
        )
        self.use_obl: bool = (
            config.enhancement.opposition_learning if config.is_mssa else False
        )
        self.obl_freq: int = config.enhancement.opposition_frequency
        self.use_corsi: bool = (
            config.enhancement.corsi_mutation if config.is_mssa else False
        )
        self.corsi_stag_threshold: int = config.enhancement.corsi_stagnation
        self.corsi_c0: float = config.enhancement.corsi_strength
        self.corsi_beta: float = config.enhancement.corsi_decay

        # 安全阈值 ST (discoverer update)
        self.ST: float = 0.8

        # 随机状态
        self._rng: np.random.Generator = np.random.default_rng(config.random_seed)

        # 内部状态
        self._interrupted: bool = False

    def _setup_signal_handler(self) -> None:
        """注册 SIGINT 优雅退出处理器。"""
        def _handler(signum, frame):
            self._interrupted = True
            logger.warning("收到中断信号 (Ctrl+C)，将在当前迭代完成后优雅退出...")

        try:
            signal.signal(signal.SIGINT, _handler)
        except (ValueError, OSError):
            # 非主线程无法注册信号处理器
            pass

    # ------------------------------------------------------------------
    # 初始化
    # ------------------------------------------------------------------

    def _good_point_set_init(
        self, pop_size: int, dim: int, bounds: np.ndarray
    ) -> np.ndarray:
        """佳点集初始化：生成均匀分布的初始种群。

        使用黄金分割比 φ 作为生成元，在 [0,1] 上生成低偏差序列，
        然后线性映射到各维搜索范围。

        Args:
            pop_size: 种群大小 N。
            dim: 搜索空间维度 D。
            bounds: (D, 2) 边界数组，bounds[d] = (lb_d, ub_d)。

        Returns:
            种群位置矩阵 (N, D)。
        """
        population = np.zeros((pop_size, dim), dtype=np.float64)
        lb = bounds[:, 0]
        ub = bounds[:, 1]
        ranges = ub - lb

        for i in range(pop_size):
            for j in range(dim):
                # 佳点集公式: (i * φ^{j+1}) mod 1
                # 使用 (j+1) 次方以区分各维
                gps_val = ((i + 1) * (GOLDEN_RATIO ** (j + 1))) % 1.0
                population[i, j] = lb[j] + gps_val * ranges[j]

        return population

    def _random_init(
        self, pop_size: int, dim: int, bounds: np.ndarray
    ) -> np.ndarray:
        """随机初始化（IPSO 模式或非佳点集模式使用）。"""
        population = np.zeros((pop_size, dim), dtype=np.float64)
        lb = bounds[:, 0]
        ub = bounds[:, 1]
        for j in range(dim):
            population[:, j] = self._rng.uniform(lb[j], ub[j], size=pop_size)
        return population

    # ------------------------------------------------------------------
    # 增强策略
    # ------------------------------------------------------------------

    def _opposition_learning(
        self, population: np.ndarray, bounds: np.ndarray
    ) -> np.ndarray:
        """反向学习增强：生成每个个体的反向解。

        x_opposition = lb + ub - x

        Args:
            population: 当前种群 (N, D)。
            bounds: 边界数组 (D, 2)。

        Returns:
            反向种群 (N, D)。
        """
        lb = bounds[:, 0]
        ub = bounds[:, 1]
        opposition = lb + ub - population
        # 裁剪到边界内
        return np.clip(opposition, lb, ub)

    def _corsi_mutation(
        self,
        population: np.ndarray,
        bounds: np.ndarray,
        stagnation_count: int,
        diversity: float,
        rng: np.random.Generator,
    ) -> np.ndarray:
        """Corsi 自适应变异扰动。

        当连续 stagnation 次迭代全局最优无改善时，对后 50% 个体施加变异。

        Corsi = C_0 * exp(-beta * stagnation / S_max) * diversity
        x_new = x + Corsi * (ub - lb) * randn()

        Args:
            population: 当前种群。
            bounds: 边界数组。
            stagnation_count: 当前停滞次数。
            diversity: 种群多样性分数 [0, 1]。
            rng: 随机数生成器。

        Returns:
            变异后的种群。
        """
        mutated = population.copy()
        lb = bounds[:, 0]
        ub = bounds[:, 1]
        ranges = ub - lb

        # Corsi 系数
        corsi = (
            self.corsi_c0
            * math.exp(-self.corsi_beta * stagnation_count / self.corsi_stag_threshold)
            * diversity
        )

        # 仅变异后 50% 个体
        n_mutate = max(1, population.shape[0] // 2)
        start_idx = population.shape[0] - n_mutate

        noise = rng.standard_normal((n_mutate, population.shape[1]))
        mutated[start_idx:] += corsi * ranges * noise
        mutated = np.clip(mutated, lb, ub)

        return mutated

    def _compute_diversity(self, population: np.ndarray) -> float:
        """计算种群多样性分数。

        diversity = mean(pairwise_euclidean(all)) / max_pairwise_euclidean
        归一化到 [0, 1]，0 表示所有个体完全相同，1 表示最大分散。

        Args:
            population: 种群 (N, D)。

        Returns:
            多样性分数。
        """
        n = population.shape[0]
        if n <= 1:
            return 0.0

        # 成对欧氏距离
        diff = population[:, np.newaxis, :] - population[np.newaxis, :, :]
        distances = np.sqrt(np.sum(diff**2, axis=2))

        # 上三角均值（排除对角线）
        triu_idx = np.triu_indices(n, k=1)
        mean_dist = np.mean(distances[triu_idx])
        max_dist = np.max(distances[triu_idx])

        if max_dist < 1e-12:
            return 0.0

        return float(mean_dist / max_dist)

    # ------------------------------------------------------------------
    # 位置更新
    # ------------------------------------------------------------------

    def _update_discoverers(
        self,
        population: np.ndarray,
        fitness: np.ndarray,
        iteration: int,
        rng: np.random.Generator,
    ) -> np.ndarray:
        """发现者位置更新。

        安全 (R2 < ST): x_new = x * exp(-i / (alpha * T_max))
        危险 (R2 >= ST): x_new = x + Q * L

        Args:
            population: 当前种群。
            fitness: 适应度值。
            iteration: 当前迭代编号 (1-based)。
            rng: 随机数生成器。

        Returns:
            更新后的种群。
        """
        updated = population.copy()
        n_disc = self.n_discoverers
        dim = population.shape[1]

        # 发现者是按适应度排序后的前 n_disc 个
        discoverer_indices = np.argsort(fitness)[:n_disc]

        # 预警值 R2 ∈ [0, 1]
        R2 = rng.uniform(0, 1)

        for idx_rank, pop_idx in enumerate(discoverer_indices):
            i_rank = idx_rank + 1  # 1-based rank
            alpha = rng.uniform(1e-6, 1.0)  # (0, 1]

            if R2 < self.ST:
                # 安全：在当前最优附近按指数衰减步长搜索
                factor = math.exp(-i_rank / (alpha * self.max_iter))
                updated[pop_idx] = population[pop_idx] * factor
            else:
                # 危险：随机飞离
                Q = rng.standard_normal(dim)
                updated[pop_idx] = population[pop_idx] + Q

        return updated

    def _update_joiners(
        self,
        population: np.ndarray,
        fitness: np.ndarray,
        best_idx: int,
        worst_idx: int,
        rng: np.random.Generator,
    ) -> np.ndarray:
        """加入者位置更新。

        低排名 (i > N/2): x_new = Q * exp((x_worst - x_i) / i^2)
        高排名 (i <= N/2): x_new = x_best + |x_i - x_best| * A_plus

        Args:
            population: 当前种群。
            fitness: 适应度值。
            best_idx: 全局最优个体索引。
            worst_idx: 全局最差个体索引。
            rng: 随机数生成器。

        Returns:
            更新后的种群。
        """
        updated = population.copy()
        n_disc = self.n_discoverers
        n_join = self.n_joiners

        # 加入者是从 n_disc 开始的后 n_join 个体
        sorted_indices = np.argsort(fitness)
        joiner_indices = sorted_indices[n_disc : n_disc + n_join]

        if len(joiner_indices) == 0:
            return updated

        x_best = population[best_idx]
        x_worst = population[worst_idx]
        dim = population.shape[1]
        half_n = population.shape[0] / 2.0

        # 加入者内部的排序位置
        for local_rank, pop_idx in enumerate(joiner_indices):
            global_rank = n_disc + local_rank + 1  # 1-based

            if global_rank > half_n:
                # 低排名加入者：饥饿驱动，朝最优方向移动
                Q = rng.standard_normal()
                factor = math.exp(
                    (x_worst - population[pop_idx]).sum() / (global_rank**2 + 1e-12)
                )
                updated[pop_idx] = population[pop_idx] + Q * factor
            else:
                # 高排名加入者：在最优附近局部搜索
                # A_plus: D 维向量，每个元素 ±1/D
                A = rng.choice([-1.0, 1.0], size=dim)
                A_plus = A / dim
                diff = np.abs(population[pop_idx] - x_best)
                updated[pop_idx] = x_best + diff * A_plus

        return updated

    def _update_scouts(
        self,
        population: np.ndarray,
        fitness: np.ndarray,
        bounds: np.ndarray,
        best_idx: int,
        worst_idx: int,
        rng: np.random.Generator,
    ) -> np.ndarray:
        """侦察者位置更新。

        当前个体非最优: x_new = x_best + beta * |x_i - x_best|
        当前个体已最优: x_new = x_i + K * (|x_i - x_worst| / (|f_i - f_worst| + 1e-8))

        Args:
            population: 当前种群。
            fitness: 适应度值。
            bounds: 边界数组。
            best_idx: 全局最优个体索引。
            worst_idx: 全局最差个体索引。
            rng: 随机数生成器。

        Returns:
            更新后的种群。
        """
        updated = population.copy()
        n_scouts = self.n_scouts

        # 侦察者是适应度最差的后 n_scouts 个
        sorted_indices = np.argsort(fitness)
        scout_indices = sorted_indices[-n_scouts:] if n_scouts > 0 else np.array([])

        if len(scout_indices) == 0:
            return updated

        x_best = population[best_idx]
        x_worst = population[worst_idx]
        f_best = fitness[best_idx]
        f_worst = fitness[worst_idx]

        for pop_idx in scout_indices:
            if fitness[pop_idx] > f_best:
                # 当前个体比最优差：向最优靠拢
                beta = rng.standard_normal()
                updated[pop_idx] = x_best + beta * np.abs(population[pop_idx] - x_best)
            else:
                # 当前个体在最优点：随机逃离
                K = rng.uniform(-1, 1)
                denom = abs(fitness[pop_idx] - f_worst) + 1e-8
                updated[pop_idx] = (
                    population[pop_idx]
                    + K
                    * np.abs(population[pop_idx] - x_worst)
                    / denom
                )

        # 裁剪到边界
        lb = bounds[:, 0]
        ub = bounds[:, 1]
        updated[scout_indices] = np.clip(updated[scout_indices], lb, ub)

        return updated

    # ------------------------------------------------------------------
    # 主优化循环
    # ------------------------------------------------------------------

    def optimize(
        self,
        objective_fn: Callable[[np.ndarray], float],
        callbacks: Optional[List[Callable]] = None,
    ) -> OptimizationResult:
        """执行 MSSA/IPS 主优化循环。

        Args:
            objective_fn: 目标函数 f(x) -> float (最小化)。
            callbacks: 可选回调列表，每个回调签名 cb(iteration, best_fitness, population)。

        Returns:
            OptimizationResult: 优化结果。
        """
        dim = self.space.dim
        bounds = self.space.bounds
        pop_size = self.pop_size
        max_iter = self.max_iter

        # ---------------------------------------------------------------
        # Step 1: 初始化
        # ---------------------------------------------------------------
        start_time = time.monotonic()

        if self.use_gps:
            population = self._good_point_set_init(pop_size, dim, bounds)
        else:
            population = self._random_init(pop_size, dim, bounds)

        # 评估初始种群
        fitness = np.full(pop_size, np.inf, dtype=np.float64)
        for i in range(pop_size):
            fitness[i] = objective_fn(population[i])

        # 排序
        sorted_order = np.argsort(fitness)
        population = population[sorted_order].copy()
        fitness = fitness[sorted_order].copy()

        # 全局最优
        best_idx_at = 0
        f_best = fitness[0]
        x_best = population[0].copy()

        # 追踪
        prev_best = f_best
        stagnation = 0
        invalid_count = sum(1 for f in fitness if f >= 1e5)
        eval_count = pop_size
        cache_hits = 0
        # cache hits are tracked inside objective_fn, pass through by convention

        convergence_curve: List[float] = [float(f_best)]
        trajectories: Dict[str, List[float]] = {
            name: [] for name in self.space.param_names
        }
        self._append_trajectory(trajectories, x_best)

        # 信号处理
        self._interrupted = False
        self._setup_signal_handler()

        # 初始种群多样性
        initial_max_pairwise = self._compute_max_pairwise(population)

        # ---------------------------------------------------------------
        # Step 2: 主循环
        # ---------------------------------------------------------------
        iteration = 0
        convergence_reason = "max_iter"

        for iteration in range(1, max_iter + 1):
            # 时间检查
            elapsed = time.monotonic() - start_time
            if elapsed >= self.max_wall_time:
                convergence_reason = "timeout"
                logger.info(
                    f"迭代 {iteration}: 超时 ({elapsed:.1f}s >= {self.max_wall_time}s)，终止搜索"
                )
                break

            # Ctrl+C 检查
            if self._interrupted:
                convergence_reason = "timeout"
                logger.info(f"迭代 {iteration}: 收到中断信号，输出当前最优解后退出")
                break

            # ---- 角色分配 ----
            sorted_order = np.argsort(fitness)
            population = population[sorted_order].copy()
            fitness = fitness[sorted_order].copy()

            # 更新最优/最差索引（排序后最优在 index 0）
            best_idx_at = 0
            worst_idx_at = pop_size - 1
            f_best = fitness[0]
            x_best = population[0].copy()

            # ---- 发现者更新 ----
            population = self._update_discoverers(
                population, fitness, iteration, self._rng
            )
            # 边界裁剪
            population = np.clip(population, bounds[:, 0], bounds[:, 1])

            # ---- 加入者更新 ----
            population = self._update_joiners(
                population, fitness, best_idx_at, worst_idx_at, self._rng
            )
            population = np.clip(population, bounds[:, 0], bounds[:, 1])

            # ---- 侦察者更新 ----
            population = self._update_scouts(
                population, fitness, bounds, best_idx_at, worst_idx_at, self._rng
            )
            population = np.clip(population, bounds[:, 0], bounds[:, 1])

            # ---- 重新评估 ----
            for i in range(pop_size):
                fitness[i] = objective_fn(population[i])
            eval_count += pop_size

            # ---- 精英保留 ----
            sorted_order = np.argsort(fitness)
            if fitness[sorted_order[0]] > f_best:
                # 新种群最优不如上一代最优，保留精英
                worst_idx = sorted_order[-1]
                population[worst_idx] = x_best
                fitness[worst_idx] = f_best

            # 重新排序和更新最优
            sorted_order = np.argsort(fitness)
            population = population[sorted_order].copy()
            fitness = fitness[sorted_order].copy()
            f_best = fitness[0]
            x_best = population[0].copy()

            # ---- 反向学习增强 （每 obl_freq 次迭代） ----
            if self.use_obl and iteration % self.obl_freq == 0:
                opposition = self._opposition_learning(population, bounds)
                for i in range(pop_size):
                    f_opp = objective_fn(opposition[i])
                    eval_count += 1
                    if f_opp < fitness[i]:
                        population[i] = opposition[i]
                        fitness[i] = f_opp

                # 重排序
                sorted_order = np.argsort(fitness)
                population = population[sorted_order].copy()
                fitness = fitness[sorted_order].copy()
                f_best = fitness[0]
                x_best = population[0].copy()

            # ---- Corsi 变异 ----
            if self.use_corsi:
                # 收敛判定
                if abs(f_best - prev_best) < self.epsilon:
                    stagnation += 1
                else:
                    stagnation = 0
                prev_best = f_best

                if stagnation >= self.corsi_stag_threshold:
                    diversity = self._compute_diversity(population)
                    population = self._corsi_mutation(
                        population, bounds, stagnation, diversity, self._rng
                    )
                    # 重新评估变异个体（仅评估后 50%，保守处理：全部重评估）
                    for i in range(pop_size):
                        fitness[i] = objective_fn(population[i])
                    eval_count += pop_size

                    # 重排序
                    sorted_order = np.argsort(fitness)
                    population = population[sorted_order].copy()
                    fitness = fitness[sorted_order].copy()
                    f_best = fitness[0]
                    x_best = population[0].copy()

                    # 若在 3 次迭代内改善不明显，让停滞继续
                    logger.info(
                        f"迭代 {iteration}: 触发 Corsi 变异 "
                        f"(stagnation={stagnation}, diversity={diversity:.4f})"
                    )
            else:
                # IPSO 模式或禁用 Corsi：简单收敛判定
                if abs(f_best - prev_best) < self.epsilon:
                    stagnation += 1
                else:
                    stagnation = 0
                prev_best = f_best

            # ---- 收敛判定 ----
            if stagnation >= self.no_improvement_rounds:
                convergence_reason = "no_improvement"
                logger.info(
                    f"迭代 {iteration}: 收敛 (连续 {stagnation} 次无改善, "
                    f"|delta| < {self.epsilon})"
                )
                break

            # ---- 保存轨迹 ----
            convergence_curve.append(float(f_best))
            self._append_trajectory(trajectories, x_best)

            # ---- 日志 ----
            if self.config.output.verbose and iteration % 5 == 0:
                diversity = self._compute_diversity(population)
                logger.info(
                    f"[Iter {iteration:3d}] best_MAPE={f_best:.6f}  "
                    f"stagnation={stagnation}  diversity={diversity:.4f}"
                )

            # ---- 回调 ----
            if callbacks:
                for cb in callbacks:
                    try:
                        cb(iteration, f_best, population.copy())
                    except Exception as exc:
                        logger.warning(f"回调执行异常: {exc}")

        # ---------------------------------------------------------------
        # 最终评估
        # ---------------------------------------------------------------
        elapsed = time.monotonic() - start_time

        # 确保最终最优个体在记录中
        if len(convergence_curve) == 0 or abs(convergence_curve[-1] - f_best) > 1e-12:
            convergence_curve.append(float(f_best))
            self._append_trajectory(trajectories, x_best)

        # 解码最优超参
        best_hyperparams = self.space.decode(x_best)

        # 多样性
        final_diversity = self._compute_diversity(population)
        final_stagnation = stagnation

        # 无效解统计
        invalid_count = sum(1 for f in fitness if f >= 1e5)

        logger.info(
            f"优化完成: reason={convergence_reason}, "
            f"iterations={iteration}, "
            f"best_MAPE={f_best:.6f}, "
            f"elapsed={elapsed:.1f}s"
        )

        return OptimizationResult(
            best_position=x_best,
            best_fitness=float(f_best),
            best_hyperparams=best_hyperparams,
            convergence_curve=convergence_curve,
            per_parameter_trajectory=trajectories,
            total_iterations=iteration,
            convergence_reason=convergence_reason,
            elapsed_seconds=elapsed,
            invalid_solutions=invalid_count,
            cache_hits=cache_hits,
            total_evaluations=eval_count,
            population_size=pop_size,
            discoverer_count=self.n_discoverers,
            joiner_count=self.n_joiners,
            scout_count=self.n_scouts,
            final_stagnation=final_stagnation,
            final_diversity=final_diversity,
            stopped_early=self._interrupted,
        )

    def _append_trajectory(
        self,
        trajectories: Dict[str, List[float]],
        x_best: np.ndarray,
    ) -> None:
        """记录当前最优个体在各超参维度的解码值到轨迹中。

        根据设计评审 M-09 建议，存储解码后的可读值，使轨迹可直接消费。
        """
        decoded = self.space.decode(x_best)
        for name, value in decoded.items():
            if name not in trajectories:
                trajectories[name] = []
            # 存储为 float（枚举/字符串类型存其索引）
            if isinstance(value, str):
                # 查找枚举索引
                for hp in self.space.params:
                    if hp.name == name and hp.discrete_values:
                        try:
                            idx = hp.discrete_values.index(value)
                            trajectories[name].append(float(idx))
                        except ValueError:
                            trajectories[name].append(float("nan"))
                        break
            else:
                trajectories[name].append(float(value))

    @staticmethod
    def _compute_max_pairwise(population: np.ndarray) -> float:
        """计算初始种群最大成对欧氏距离。"""
        n = population.shape[0]
        if n <= 1:
            return 1.0
        diff = population[:, np.newaxis, :] - population[np.newaxis, :, :]
        distances = np.sqrt(np.sum(diff**2, axis=2))
        triu_idx = np.triu_indices(n, k=1)
        return float(np.max(distances[triu_idx]))
