"""MSSA 目标函数：加权 MAPE 最小化

提供可注入的 objective callable，支持 SHA256 指纹缓存和训练子进程调用。
训练管线部分为占位接口，可注入自定义 evaluate 函数以适配实际训练环境。
"""

from __future__ import annotations

import hashlib
import json
import logging
import math
import os
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Callable, Dict, Optional, Tuple

import numpy as np

try:
    from .config import MSSAConfig, ObjectiveConfig
    from .search_space import SearchSpace, get_default_space
except ImportError:
    from config import MSSAConfig, ObjectiveConfig
    from search_space import SearchSpace, get_default_space

logger = logging.getLogger(__name__)


# ============================================================================
# 评估缓存管理
# ============================================================================


class CacheManager:
    """目标函数评估缓存管理器。

    缓存键 = 超参组合的 SHA256 前 16 位十六进制。
    跨运行持久化，训练数据变更时自动失效（通过 training_data_fingerprint）。
    """

    def __init__(
        self,
        cache_path: str = "mssa_cache.json",
        training_data_fingerprint: Optional[str] = None,
    ):
        """
        Args:
            cache_path: 缓存文件路径。
            training_data_fingerprint: 训练数据指纹（SHA256），变更则缓存失效。
        """
        self.cache_path = Path(cache_path)
        self.training_data_fingerprint = training_data_fingerprint or "unknown"
        self._cache: Dict[str, Dict[str, Any]] = {}
        self._hits: int = 0
        self._loaded: bool = False

    @property
    def hits(self) -> int:
        return self._hits

    def load(self) -> None:
        """从磁盘加载缓存文件。"""

        if not self.cache_path.exists():
            logger.debug(f"缓存文件不存在: {self.cache_path}，将创建新缓存")
            self._loaded = True
            return

        try:
            with open(self.cache_path, "r", encoding="utf-8") as f:
                data = json.load(f)

            # 检查训练数据指纹
            stored_fingerprint = data.get("training_data_fingerprint", "")
            if stored_fingerprint != self.training_data_fingerprint:
                logger.warning(
                    f"训练数据指纹已变更 ({stored_fingerprint[:8]}... -> "
                    f"{self.training_data_fingerprint[:8]}...)，缓存全部失效"
                )
                self._cache = {}
            else:
                self._cache = data.get("entries", {})
                logger.info(f"已加载 {len(self._cache)} 条评估缓存记录")

            self._loaded = True
        except (json.JSONDecodeError, KeyError, OSError) as exc:
            logger.warning(f"缓存文件损坏或不可读: {exc}，将创建新缓存")
            self._cache = {}
            self._loaded = True

    def save(self) -> None:
        """持久化缓存到磁盘。"""
        data = {
            "cache_version": "1.0",
            "created": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "training_data_fingerprint": self.training_data_fingerprint,
            "entries": self._cache,
        }
        try:
            self.cache_path.parent.mkdir(parents=True, exist_ok=True)
            with open(self.cache_path, "w", encoding="utf-8") as f:
                json.dump(data, f, indent=2, ensure_ascii=False)
            logger.debug(f"缓存已保存: {len(self._cache)} 条记录 -> {self.cache_path}")
        except OSError as exc:
            logger.warning(f"缓存保存失败: {exc}")

    def lookup(self, hyperparams: Dict[str, Any]) -> Optional[float]:
        """查询缓存。

        Args:
            hyperparams: 超参字典。

        Returns:
            缓存的加权 MAPE 值，如果未命中返回 None。
        """
        if not self._loaded:
            self.load()

        fingerprint = self._compute_fingerprint(hyperparams)
        entry = self._cache.get(fingerprint)
        if entry is not None:
            self._hits += 1
            logger.debug(f"缓存命中: {fingerprint} -> weighted_mape={entry['weighted_mape']}")
            return float(entry["weighted_mape"])
        return None

    def store(
        self,
        hyperparams: Dict[str, Any],
        mape_pv: float,
        mape_load: float,
        weighted_mape: float,
    ) -> None:
        """存储评估结果到缓存。"""
        if not self._loaded:
            self.load()

        fingerprint = self._compute_fingerprint(hyperparams)
        self._cache[fingerprint] = {
            "hyperparams": hyperparams,
            "mape_pv": mape_pv,
            "mape_load": mape_load,
            "weighted_mape": weighted_mape,
            "evaluated_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
        }

    @staticmethod
    def _compute_fingerprint(hyperparams: Dict[str, Any]) -> str:
        """计算超参组合的 SHA256 指纹（取前 16 位十六进制）。"""
        serialized = json.dumps(hyperparams, sort_keys=True, ensure_ascii=False)
        full_hash = hashlib.sha256(serialized.encode("utf-8")).hexdigest()
        return full_hash[:16]


# ============================================================================
# 训练结果解析
# ============================================================================


class ResultParser:
    """训练脚本 stdout 输出解析器。

    从训练输出中提取 MAPE_pv 和 MAPE_load 值。
    """

    # 正则模式（支持多种常见输出格式）
    PATTERNS = {
        "mape_pv": [
            re.compile(r"MAPE_pv[:\s=]+([\d.]+(?:[eE][+-]?\d+)?)", re.IGNORECASE),
            re.compile(r"pv.*?MAPE[:\s=]+([\d.]+(?:[eE][+-]?\d+)?)", re.IGNORECASE),
        ],
        "mape_load": [
            re.compile(r"MAPE_load[:\s=]+([\d.]+(?:[eE][+-]?\d+)?)", re.IGNORECASE),
            re.compile(r"load.*?MAPE[:\s=]+([\d.]+(?:[eE][+-]?\d+)?)", re.IGNORECASE),
        ],
    }

    @classmethod
    def parse(cls, stdout: str) -> Tuple[Optional[float], Optional[float]]:
        """从训练脚本 stdout 提取 MAPE 值。

        Args:
            stdout: 训练脚本的标准输出文本。

        Returns:
            (mape_pv, mape_load) 元组，解析失败返回 (None, None)。
        """
        mape_pv = cls._extract(stdout, cls.PATTERNS["mape_pv"])
        mape_load = cls._extract(stdout, cls.PATTERNS["mape_load"])
        return mape_pv, mape_load

    @classmethod
    def _extract(cls, text: str, patterns: list) -> Optional[float]:
        for pattern in patterns:
            match = pattern.search(text)
            if match:
                try:
                    return float(match.group(1))
                except (ValueError, IndexError):
                    continue
        return None


# ============================================================================
# 训练运行器
# ============================================================================


class TrainingRunner:
    """训练子进程调用封装。

    支持将实际的训练调用替换为自定义 callable，以适配不同训练环境。
    """

    def __init__(
        self,
        config: ObjectiveConfig,
        training_config,
        search_space: SearchSpace,
    ):
        self.config = config
        self.training_config = training_config
        self.space = search_space

    def run(
        self,
        hyperparams: Dict[str, Any],
        temp_config_path: str,
    ) -> Tuple[Optional[float], Optional[float]]:
        """执行训练脚本并解析 MAPE 结果。

        Args:
            hyperparams: 超参字典（用于日志/缓存）。
            temp_config_path: 临时训练配置文件路径。

        Returns:
            (mape_pv, mape_load)。
        """
        python_exe = self.training_config.python_executable or sys.executable
        script_path = self.training_config.script_path
        extra_args = self.training_config.extra_args or []

        cmd = [
            python_exe,
            script_path,
            "--config",
            temp_config_path,
        ] + extra_args

        logger.debug(f"执行训练: {' '.join(cmd)}")

        try:
            proc = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=self.config.training_timeout_seconds,
            )
        except subprocess.TimeoutExpired:
            logger.warning(
                f"训练超时 ({self.config.training_timeout_seconds}s): "
                f"{hyperparams}"
            )
            return None, None
        except FileNotFoundError:
            logger.error(f"训练脚本或 Python 解释器不存在: {cmd[0]} {cmd[1]}")
            return None, None
        except Exception as exc:
            logger.error(f"子进程执行异常: {exc}")
            return None, None

        if proc.returncode != 0:
            logger.warning(
                f"训练脚本返回非零退出码 ({proc.returncode}): "
                f"stderr={proc.stderr[:200]}"
            )
            return None, None

        mape_pv, mape_load = ResultParser.parse(proc.stdout)
        if mape_pv is None or mape_load is None:
            logger.warning(
                f"无法从训练输出中解析 MAPE 值: stdout={proc.stdout[:300]}"
            )
            return None, None

        return mape_pv, mape_load


# ============================================================================
# 目标函数工厂
# ============================================================================


class ObjectiveFunc:
    """MSSA 目标函数包装器。

    将超参编码向量映射为加权 MAPE 标量值，供 MSSA 优化器最小化。

    Usage:
        space = SearchSpace()
        obj_func = ObjectiveFunc(config, space)
        # or with custom runner:
        obj_func = ObjectiveFunc(config, space, custom_runner_fn)
        score = obj_func(encoded_vector)
    """

    def __init__(
        self,
        config: MSSAConfig,
        search_space: Optional[SearchSpace] = None,
        custom_runner: Optional[Callable[[Dict[str, Any]], Tuple[float, float]]] = None,
        training_data_fingerprint: Optional[str] = None,
    ):
        """
        Args:
            config: MSSA 搜索配置。
            search_space: 搜索空间定义。
            custom_runner: 自定义训练执行函数，接收超参 dict，返回 (mape_pv, mape_load)。
                           若为 None，使用默认 TrainingRunner（子进程调用 train.py）。
            training_data_fingerprint: 训练数据指纹（用于缓存失效）。
        """
        self.config = config
        self.space = search_space or get_default_space()
        self.custom_runner = custom_runner

        # 缓存
        cache_path = config.objective.cache_path
        if not Path(cache_path).is_absolute():
            cache_path = str(Path(__file__).parent / cache_path)
        self.cache = CacheManager(cache_path, training_data_fingerprint)
        if config.objective.cache_enabled:
            self.cache.load()

        # 权重
        self.pv_weight = config.objective.pv_weight
        self.load_weight = config.objective.load_weight
        self.penalty_score = config.objective.penalty_score

        # 训练运行器
        if custom_runner is None:
            self.runner = TrainingRunner(
                config.objective, config.training, self.space
            )
        else:
            self.runner = None  # 使用自定义 runner，不使用默认子进程

        # 统计
        self._eval_count: int = 0
        self._invalid_count: int = 0

    @property
    def eval_count(self) -> int:
        return self._eval_count

    @property
    def invalid_count(self) -> int:
        return self._invalid_count

    @property
    def cache_hits(self) -> int:
        return self.cache.hits

    def __call__(self, vector: np.ndarray) -> float:
        """评估目标函数 f(x) -> float。

        Args:
            vector: 14 维编码向量。

        Returns:
            加权 MAPE 值（越小越好），训练失败返回 penalty_score。
        """
        self._eval_count += 1

        # Step 1: 解码
        try:
            hyperparams = self.space.decode(vector)
        except Exception as exc:
            logger.error(f"解码失败: {exc}")
            self._invalid_count += 1
            return self.penalty_score

        # Step 2: 缓存查找
        if self.config.objective.cache_enabled:
            cached = self.cache.lookup(hyperparams)
            if cached is not None:
                return cached

        # Step 3 & 4: 执行训练/评估
        mape_pv, mape_load = self._evaluate(hyperparams)

        # Step 5: 失败处理
        if mape_pv is None or mape_load is None:
            self._invalid_count += 1
            return self.penalty_score

        # 有效性检查
        if (
            math.isnan(mape_pv) or math.isinf(mape_pv)
            or math.isnan(mape_load) or math.isinf(mape_load)
            or mape_pv > 1.0 or mape_load > 1.0
        ):
            self._invalid_count += 1
            return self.penalty_score

        # Step 6: 计算加权分数
        score = self.pv_weight * mape_pv + self.load_weight * mape_load

        # 写入缓存
        if self.config.objective.cache_enabled:
            self.cache.store(hyperparams, mape_pv, mape_load, score)

        return score

    def _evaluate(
        self, hyperparams: Dict[str, Any]
    ) -> Tuple[Optional[float], Optional[float]]:
        """评估超参组合的 MAPE。

        设计评审 M-06：使用 try/finally 确保临时配置文件清理。
        """
        # 使用自定义 runner（通常用于单元测试）
        if self.custom_runner is not None:
            try:
                return self.custom_runner(hyperparams)
            except Exception as exc:
                logger.error(f"自定义 runner 执行异常: {exc}")
                return None, None

        # 使用默认子进程 runner
        temp_config = None
        try:
            # Step 3: 写临时训练配置
            temp_config = tempfile.NamedTemporaryFile(
                suffix=".yaml", mode="w", delete=False, encoding="utf-8"
            )
            self._write_temp_config(temp_config, hyperparams)
            temp_config_path = temp_config.name
            temp_config.close()

            # Step 4: 调用训练脚本
            mape_pv, mape_load = self.runner.run(hyperparams, temp_config_path)
            return mape_pv, mape_load

        except Exception as exc:
            logger.error(f"训练评估异常: {exc}")
            return None, None

        finally:
            # Step 7 (设计评审 M-06): 清理临时文件
            if temp_config is not None:
                try:
                    os.unlink(temp_config.name)
                except OSError:
                    pass

    def _write_temp_config(
        self, file_handle, hyperparams: Dict[str, Any]
    ) -> None:
        """将超参字典写入临时训练配置文件。

        Args:
            file_handle: 已打开的文件句柄。
            hyperparams: 超参字典。
        """
        # 生成 YAML 格式的训练配置
        lines = [
            "# MSSA 自动生成的训练配置",
            f"# 生成时间: {time.strftime('%Y-%m-%dT%H:%M:%S')}",
            "",
        ]
        for name, value in hyperparams.items():
            if isinstance(value, str):
                lines.append(f"{name}: \"{value}\"")
            else:
                lines.append(f"{name}: {value}")
        lines.append("")

        file_handle.write("\n".join(lines))
        file_handle.flush()

    def save_cache(self) -> None:
        """持久化缓存（优化完成后调用）。"""
        if self.config.objective.cache_enabled:
            self.cache.save()


# ============================================================================
# 便捷工厂函数
# ============================================================================


def create_objective(
    config: MSSAConfig,
    search_space: Optional[SearchSpace] = None,
    custom_runner: Optional[Callable[[Dict[str, Any]], Tuple[float, float]]] = None,
    training_data_fingerprint: Optional[str] = None,
) -> ObjectiveFunc:
    """创建目标函数实例（工厂函数）。

    Args:
        config: MSSA 搜索配置。
        search_space: 搜索空间定义。
        custom_runner: 自定义训练执行函数，用于注入模拟目标函数以支持单元测试。
                       签名: (hyperparams: dict) -> (mape_pv: float, mape_load: float)
        training_data_fingerprint: 训练数据指纹。

    Returns:
        ObjectiveFunc 可调用对象。

    Example:
        config = load_config("mssa_search_config.yaml")
        obj = create_objective(config)
        score = obj(encoded_vector)  # 最小化
    """
    return ObjectiveFunc(
        config=config,
        search_space=search_space,
        custom_runner=custom_runner,
        training_data_fingerprint=training_data_fingerprint,
    )
