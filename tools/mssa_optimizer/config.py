"""MSSA 搜索配置加载与校验

支持从 YAML 文件加载 MSSA/IPS 超参搜索配置，并提供参数合法性校验。
"""

from __future__ import annotations

import math
import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple, Union

import yaml


# ============================================================================
# 配置数据结构
# ============================================================================

@dataclass
class PopulationConfig:
    """种群参数配置。"""

    size: int = 30
    discoverer_ratio: float = 0.20
    scout_ratio: float = 0.10

    @property
    def joiner_ratio(self) -> float:
        return 1.0 - self.discoverer_ratio - self.scout_ratio


@dataclass
class EnhancementConfig:
    """增强策略配置（仅 MSSA 生效，IPSO 忽略）。"""

    good_point_set: bool = True
    opposition_learning: bool = True
    opposition_frequency: int = 5
    corsi_mutation: bool = True
    corsi_stagnation: int = 10
    corsi_strength: float = 0.1
    corsi_decay: float = 2.0


@dataclass
class TerminationConfig:
    """终止条件配置。"""

    max_iterations: int = 50
    no_improvement_rounds: int = 10
    epsilon: float = 1.0e-4
    max_wall_time_seconds: float = 7200.0
    restart_on_stagnation: bool = False


@dataclass
class ObjectiveConfig:
    """目标函数配置。"""

    pv_weight: float = 0.5
    load_weight: float = 0.5
    penalty_score: float = 1_000_000.0
    training_timeout_seconds: int = 600
    cache_enabled: bool = True
    cache_path: str = "mssa_cache.json"


@dataclass
class TrainingConfig:
    """训练脚本调用配置。"""

    script_path: str = "../../train.py"
    python_executable: Optional[str] = None
    extra_args: List[str] = field(default_factory=list)


@dataclass
class OutputConfig:
    """输出配置。"""

    result_path: str = "mssa_search_result.json"
    verbose: bool = True
    log_level: str = "INFO"


@dataclass
class MSSAConfig:
    """MSSA/IPS 超参搜索主配置结构体。

    Attributes:
        algorithm: 算法选择 ("MSSA" 或 "IPSO")
        random_seed: 随机种子（确定性复现）
        population: 种群参数
        enhancement: 增强策略（MSSA 专属）
        termination: 终止条件
        objective: 目标函数配置
        training: 训练脚本配置
        search_space_overrides: 搜索空间覆盖（可选）
        output: 输出配置
    """

    algorithm: str = "MSSA"
    random_seed: int = 42
    population: PopulationConfig = field(default_factory=PopulationConfig)
    enhancement: EnhancementConfig = field(default_factory=EnhancementConfig)
    termination: TerminationConfig = field(default_factory=TerminationConfig)
    objective: ObjectiveConfig = field(default_factory=ObjectiveConfig)
    training: TrainingConfig = field(default_factory=TrainingConfig)
    output: OutputConfig = field(default_factory=OutputConfig)
    search_space_overrides: Dict[str, Any] = field(default_factory=dict)

    @property
    def is_mssa(self) -> bool:
        """是否为 MSSA 模式（而非 IPSO）。"""
        return self.algorithm.upper() == "MSSA"


# ============================================================================
# YAML 加载
# ============================================================================


def _parse_population(data: Dict[str, Any]) -> PopulationConfig:
    return PopulationConfig(
        size=data.get("size", 30),
        discoverer_ratio=data.get("discoverer_ratio", 0.20),
        scout_ratio=data.get("scout_ratio", 0.10),
    )


def _parse_enhancement(data: Dict[str, Any]) -> EnhancementConfig:
    return EnhancementConfig(
        good_point_set=data.get("good_point_set", True),
        opposition_learning=data.get("opposition_learning", True),
        opposition_frequency=data.get("opposition_frequency", 5),
        corsi_mutation=data.get("corsi_mutation", True),
        corsi_stagnation=data.get("corsi_stagnation", 10),
        corsi_strength=data.get("corsi_strength", 0.1),
        corsi_decay=data.get("corsi_decay", 2.0),
    )


def _parse_termination(data: Dict[str, Any]) -> TerminationConfig:
    return TerminationConfig(
        max_iterations=data.get("max_iterations", 50),
        no_improvement_rounds=data.get("no_improvement_rounds", 10),
        epsilon=data.get("epsilon", 1.0e-4),
        max_wall_time_seconds=data.get("max_wall_time_seconds", 7200.0),
        restart_on_stagnation=data.get("restart_on_stagnation", False),
    )


def _parse_objective(data: Dict[str, Any]) -> ObjectiveConfig:
    return ObjectiveConfig(
        pv_weight=data.get("pv_weight", 0.5),
        load_weight=data.get("load_weight", 0.5),
        penalty_score=data.get("penalty_score", 1_000_000.0),
        training_timeout_seconds=data.get("training_timeout_seconds", 600),
        cache_enabled=data.get("cache_enabled", True),
        cache_path=data.get("cache_path", "mssa_cache.json"),
    )


def _parse_training(data: Dict[str, Any]) -> TrainingConfig:
    return TrainingConfig(
        script_path=data.get("script_path", "../../train.py"),
        python_executable=data.get("python_executable", None),
        extra_args=data.get("extra_args", []),
    )


def _parse_output(data: Dict[str, Any]) -> OutputConfig:
    return OutputConfig(
        result_path=data.get("result_path", "mssa_search_result.json"),
        verbose=data.get("verbose", True),
        log_level=data.get("log_level", "INFO"),
    )


def load_config(yaml_path: Union[str, Path]) -> MSSAConfig:
    """从 YAML 文件加载搜索配置。

    Args:
        yaml_path: YAML 配置文件路径。

    Returns:
        MSSAConfig: 解析后的配置对象。

    Raises:
        FileNotFoundError: 配置文件不存在。
        yaml.YAMLError: YAML 解析错误。
    """
    yaml_path = Path(yaml_path)
    if not yaml_path.exists():
        raise FileNotFoundError(f"配置文件不存在: {yaml_path}")

    with open(yaml_path, "r", encoding="utf-8") as f:
        raw = yaml.safe_load(f)

    if raw is None:
        raw = {}

    config = MSSAConfig(
        algorithm=raw.get("algorithm", "MSSA"),
        random_seed=raw.get("random_seed", 42),
        population=_parse_population(raw.get("population", {})),
        enhancement=_parse_enhancement(raw.get("enhancement", {})),
        termination=_parse_termination(raw.get("termination", {})),
        objective=_parse_objective(raw.get("objective", {})),
        training=_parse_training(raw.get("training", {})),
        output=_parse_output(raw.get("output", {})),
        search_space_overrides=raw.get("search_space_overrides", {}),
    )

    return config


# ============================================================================
# 配置校验
# ============================================================================


@dataclass
class ValidationResult:
    """配置校验结果。"""

    valid: bool
    errors: List[str] = field(default_factory=list)
    warnings: List[str] = field(default_factory=list)


def validate_config(config: MSSAConfig) -> ValidationResult:
    """校验 MSSAConfig 的合法性和一致性。

    校验规则（按设计 Section 12.8）：
    - algorithm 必须为 "MSSA" 或 "IPSO"
    - population.size 必须在 [10, 100]
    - discoverer_ratio + scout_ratio < 1.0
    - max_iterations >= 1
    - max_wall_time_seconds >= 300
    - pv_weight + load_weight ≈ 1.0（否则自动归一化 + WARN）
    - penalty_score > 1.0
    - training_timeout_seconds >= 60
    - training.script_path 必须存在

    Args:
        config: 待校验的配置对象。

    Returns:
        ValidationResult: 校验结果，包含错误和警告列表。
    """
    errors: List[str] = []
    warnings: List[str] = []

    # 算法名称
    if config.algorithm.upper() not in ("MSSA", "IPSO"):
        errors.append(
            f"algorithm 必须为 'MSSA' 或 'IPSO'，当前值: '{config.algorithm}'"
        )

    # 种群大小
    if not (10 <= config.population.size <= 100):
        errors.append(
            f"population.size 必须在 [10, 100] 范围，当前值: {config.population.size}"
        )

    # 角色比例
    total_ratio = config.population.discoverer_ratio + config.population.scout_ratio
    if total_ratio >= 1.0:
        errors.append(
            f"discoverer_ratio ({config.population.discoverer_ratio}) + "
            f"scout_ratio ({config.population.scout_ratio}) = {total_ratio} >= 1.0，"
            f"加入者数量将为 0，不可接受"
        )

    # 最大迭代次数
    if config.termination.max_iterations < 1:
        errors.append(
            f"max_iterations 必须 >= 1，当前值: {config.termination.max_iterations}"
        )

    # 总时间上限
    if config.termination.max_wall_time_seconds < 300:
        errors.append(
            f"max_wall_time_seconds 必须 >= 300 (至少 5 分钟)，"
            f"当前值: {config.termination.max_wall_time_seconds}"
        )

    # 权重归一化检查
    weight_sum = config.objective.pv_weight + config.objective.load_weight
    if abs(weight_sum - 1.0) > 1e-6:
        warnings.append(
            f"pv_weight ({config.objective.pv_weight}) + load_weight "
            f"({config.objective.load_weight}) = {weight_sum} ≠ 1.0，将自动归一化"
        )
        # 自动归一化
        config.objective.pv_weight /= weight_sum
        config.objective.load_weight /= weight_sum

    # 惩罚分数
    if config.objective.penalty_score <= 1.0:
        warnings.append(
            f"penalty_score ({config.objective.penalty_score}) <= 1.0，"
            f"将使用默认值 1e6"
        )
        config.objective.penalty_score = 1_000_000.0

    # 训练超时
    if config.objective.training_timeout_seconds < 60:
        warnings.append(
            f"training_timeout_seconds ({config.objective.training_timeout_seconds}) < 60，"
            f"将使用默认值 600"
        )
        config.objective.training_timeout_seconds = 600

    # 训练脚本路径
    script_path = Path(config.training.script_path)
    if not script_path.is_absolute():
        # 相对路径：相对于配置文件所在目录
        # 因为无法在此处获知配置文件目录，仅检查绝对路径情况
        # 运行时由 objective.py 做最终检查
        pass
    else:
        if not script_path.exists():
            errors.append(f"训练脚本不存在: {script_path}")

    # 搜索空间覆盖校验
    if config.search_space_overrides:
        _validate_search_space_overrides(config.search_space_overrides, errors)

    return ValidationResult(
        valid=len(errors) == 0,
        errors=errors,
        warnings=warnings,
    )


def _validate_search_space_overrides(
    overrides: Dict[str, Any], errors: List[str]
) -> None:
    """校验搜索空间覆盖参数的合法性。"""
    valid_keys = {
        "hidden_size", "num_layers", "attn_score", "vmd_k", "vmd_alpha",
        "lr", "batch_size", "dropout", "optimizer", "input_window",
    }

    for key, value in overrides.items():
        if key not in valid_keys:
            errors.append(f"未知的搜索空间覆盖键: '{key}'，有效键: {sorted(valid_keys)}")
            continue

        if isinstance(value, list):
            # 离散/枚举值列表
            if len(value) < 2:
                errors.append(
                    f"搜索空间覆盖 '{key}' 的值列表至少需要 2 个选项，当前: {value}"
                )
        elif isinstance(value, dict):
            # 范围定义 {min, max}
            if "min" not in value or "max" not in value:
                errors.append(
                    f"搜索空间覆盖 '{key}' 的范围定义缺少 min/max 字段: {value}"
                )
            elif value["min"] >= value["max"]:
                errors.append(
                    f"搜索空间覆盖 '{key}' 的 min ({value['min']}) >= max ({value['max']})"
                )
        else:
            errors.append(
                f"搜索空间覆盖 '{key}' 的值类型不支持: {type(value).__name__}，"
                f"应为 list 或 dict"
            )
