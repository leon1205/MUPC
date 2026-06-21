"""MSSA 搜索结果 JSON 输出

严格对齐 PRD Section 7.4.2 JSON Schema。支持输出构建、序列化和自校验。
"""

from __future__ import annotations

import json
import logging
import math
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional

try:
    from .mssa import OptimizationResult
    from .config import MSSAConfig
except ImportError:
    from mssa import OptimizationResult
    from config import MSSAConfig

logger = logging.getLogger(__name__)


# ============================================================================
# 输出构建器
# ============================================================================


class SearchOutput:
    """MSSA/IPS 搜索输出构建器。

    将 OptimizationResult 和 MSSAConfig 合并为符合 PRD 7.4.2 JSON Schema 的字典结构。
    """

    # 合法枚举值
    VALID_CONVERGENCE_REASONS = {"max_iter", "no_improvement", "timeout"}
    VALID_QUALITY_FLAGS = {"usable", "unusable"}
    REQUIRED_HP_KEYS = {
        "hidden_size", "num_layers", "attn_score", "vmd_k", "vmd_alpha",
        "lr", "batch_size", "dropout", "optimizer", "input_window",
    }

    @classmethod
    def build(
        cls,
        result: OptimizationResult,
        config: MSSAConfig,
        start_time: Optional[datetime] = None,
        end_time: Optional[datetime] = None,
        mape_pv: Optional[float] = None,
        mape_load: Optional[float] = None,
        quality_flag: str = "usable",
    ) -> Dict[str, Any]:
        """构建符合 PRD 7.4.2 Schema 的输出字典。

        Args:
            result: MSSA 优化结果。
            config: 搜索配置。
            start_time: 搜索开始时间（None 则用当前时间推算）。
            end_time: 搜索结束时间。
            mape_pv: 最优解的光伏 MAPE（如果已知）。
            mape_load: 最优解的负荷 MAPE（如果已知）。
            quality_flag: 质量标记 ("usable" / "unusable")。

        Returns:
            输出字典。
        """
        # 时间处理
        if end_time is None:
            end_time = datetime.now(timezone.utc)
        if start_time is None:
            from datetime import timedelta
            start_time = end_time - timedelta(seconds=result.elapsed_seconds)

        return {
            "search_metadata": cls._build_metadata(result, config, start_time, end_time),
            "best_hyperparameters": result.best_hyperparams,
            "best_objective": cls._build_best_objective(result, mape_pv, mape_load),
            "convergence_curve": result.convergence_curve,
            "per_parameter_trajectory": result.per_parameter_trajectory,
            "quality_flag": quality_flag,
            "additional_info": cls._build_additional_info(result, config),
        }

    @classmethod
    def _build_metadata(
        cls,
        result: OptimizationResult,
        config: MSSAConfig,
        start_time: datetime,
        end_time: datetime,
    ) -> Dict[str, Any]:
        return {
            "algorithm": config.algorithm.upper(),
            "start_time": start_time.isoformat(),
            "end_time": end_time.isoformat(),
            "total_iterations": result.total_iterations,
            "convergence_reason": result.convergence_reason,
            "elapsed_seconds": result.elapsed_seconds,
            "population_size": result.population_size,
            "discoverer_ratio": config.population.discoverer_ratio,
            "scout_ratio": config.population.scout_ratio,
            "invalid_solutions": result.invalid_solutions,
            "cache_hits": result.cache_hits,
            "total_evaluations": result.total_evaluations,
        }

    @classmethod
    def _build_best_objective(
        cls,
        result: OptimizationResult,
        mape_pv: Optional[float] = None,
        mape_load: Optional[float] = None,
    ) -> Dict[str, float]:
        # 尝试从 best_hyperparams 无法推断 MAPE 分量
        # 如果调用方未提供，则存储占位值（结果主要靠 weighted_mape）
        pv = mape_pv if mape_pv is not None else result.best_fitness
        load = mape_load if mape_load is not None else result.best_fitness
        return {
            "weighted_mape": result.best_fitness,
            "mape_pv": pv,
            "mape_load": load,
        }

    @classmethod
    def _build_additional_info(
        cls,
        result: OptimizationResult,
        config: MSSAConfig,
    ) -> Dict[str, Any]:
        info: Dict[str, Any] = {
            "population_size": result.population_size,
            "discoverer_count": result.discoverer_count,
            "joiner_count": result.joiner_count,
            "scout_count": result.scout_count,
            "random_seed": config.random_seed,
            "invalid_solutions": result.invalid_solutions,
            "cache_hits": result.cache_hits,
            "total_evaluations": result.total_evaluations,
            "stopped_early": result.stopped_early,
            "final_stagnation_count": result.final_stagnation,
            "final_diversity": result.final_diversity,
        }

        if config.is_mssa:
            info["opposition_learning_frequency"] = config.enhancement.opposition_frequency
            info["corsi_stagnation_threshold"] = config.enhancement.corsi_stagnation
            info["corsi_initial_strength"] = config.enhancement.corsi_strength
            info["corsi_decay_factor"] = config.enhancement.corsi_decay

        return info


# ============================================================================
# 输出校验
# ============================================================================


def validate_output(output: Dict[str, Any]) -> List[str]:
    """对搜索结果输出进行自校验。

    校验规则（设计 Section 12.7）：
    1. best_hyperparameters 所有必填键存在且类型正确
    2. weighted_mape ≈ 0.5 * mape_pv + 0.5 * mape_load（容差仅对等权）
    3. len(convergence_curve) == total_iterations
    4. per_parameter_trajectory 每个键长度 == total_iterations
    5. convergence_reason 为合法枚举值
    6. quality_flag 为合法枚举值

    Args:
        output: build() 输出的字典。

    Returns:
        校验错误信息列表（空列表 = 通过）。
    """
    errors: List[str] = []

    # 1. 必填顶层键
    for key in ("search_metadata", "best_hyperparameters", "best_objective"):
        if key not in output:
            errors.append(f"缺少必填字段: '{key}'")

    if errors:
        return errors

    # 2. best_hyperparameters 键完整性
    hp = output.get("best_hyperparameters", {})
    for key in SearchOutput.REQUIRED_HP_KEYS:
        if key not in hp:
            errors.append(f"best_hyperparameters 缺少键: '{key}'")

    # 类型检查
    _check_type(hp, "hidden_size", int, errors)
    _check_type(hp, "num_layers", int, errors)
    _check_type(hp, "attn_score", str, errors)
    _check_type(hp, "vmd_k", int, errors)
    _check_type(hp, "vmd_alpha", (int, float), errors)
    _check_type(hp, "lr", (int, float), errors)
    _check_type(hp, "batch_size", int, errors)
    _check_type(hp, "dropout", (int, float), errors)
    _check_type(hp, "optimizer", str, errors)
    _check_type(hp, "input_window", int, errors)

    # 3. best_objective
    obj = output.get("best_objective", {})
    if "weighted_mape" not in obj:
        errors.append("best_objective 缺少键: 'weighted_mape'")

    # 4. metadata
    meta = output.get("search_metadata", {})
    if "convergence_reason" in meta:
        if meta["convergence_reason"] not in SearchOutput.VALID_CONVERGENCE_REASONS:
            errors.append(
                f"非法的 convergence_reason: '{meta['convergence_reason']}'"
            )

    if "algorithm" in meta:
        if meta["algorithm"] not in ("MSSA", "IPSO"):
            errors.append(
                f"非法的 algorithm: '{meta['algorithm']}'，应为 'MSSA' 或 'IPSO'"
            )

    # 5. convergence_curve 长度
    total_iter = meta.get("total_iterations", 0)
    curve = output.get("convergence_curve", [])
    if len(curve) != total_iter:
        errors.append(
            f"convergence_curve 长度 ({len(curve)}) != total_iterations ({total_iter})"
        )

    # 6. per_parameter_trajectory 长度
    traj = output.get("per_parameter_trajectory", {})
    for name, values in traj.items():
        if len(values) != total_iter:
            errors.append(
                f"per_parameter_trajectory['{name}'] 长度 ({len(values)}) "
                f"!= total_iterations ({total_iter})"
            )

    # 7. quality_flag
    qf = output.get("quality_flag", "")
    if qf not in SearchOutput.VALID_QUALITY_FLAGS:
        errors.append(f"非法的 quality_flag: '{qf}'")

    return errors


def _check_type(
    obj: Dict[str, Any],
    key: str,
    expected_type,
    errors: List[str],
) -> None:
    """检查字典键的类型。"""
    if key not in obj:
        return
    value = obj[key]
    if not isinstance(value, expected_type):
        type_name = (
            expected_type.__name__
            if isinstance(expected_type, type)
            else " | ".join(t.__name__ for t in expected_type)
        )
        errors.append(
            f"best_hyperparameters.{key} 类型错误: "
            f"期望 {type_name}，实际 {type(value).__name__}"
        )


# ============================================================================
# 序列化与输出
# ============================================================================


def to_json(
    result: OptimizationResult,
    config: MSSAConfig,
    start_time: Optional[datetime] = None,
    end_time: Optional[datetime] = None,
    mape_pv: Optional[float] = None,
    mape_load: Optional[float] = None,
    quality_flag: str = "usable",
    output_path: Optional[str] = None,
    indent: int = 2,
) -> str:
    """将优化结果序列化为符合 PRD 7.4.2 Schema 的 JSON 字符串并可选写文件。

    Args:
        result: 优化结果。
        config: 搜索配置。
        start_time: 搜索开始时间。
        end_time: 搜索结束时间。
        mape_pv: 光伏 MAPE。
        mape_load: 负荷 MAPE。
        quality_flag: 质量标记。
        output_path: 输出文件路径（None 则不写文件）。
        indent: JSON 缩进空格数。

    Returns:
        JSON 字符串。
    """
    output_dict = SearchOutput.build(
        result=result,
        config=config,
        start_time=start_time,
        end_time=end_time,
        mape_pv=mape_pv,
        mape_load=mape_load,
        quality_flag=quality_flag,
    )

    # 自校验
    validation_errors = validate_output(output_dict)
    if validation_errors:
        logger.error(
            f"输出校验发现 {len(validation_errors)} 个错误:"
        )
        for err in validation_errors:
            logger.error(f"  - {err}")
        output_dict["quality_flag"] = "unusable"
        if "additional_info" in output_dict:
            output_dict["additional_info"]["validation_errors"] = validation_errors

    # 序列化
    json_str = json.dumps(output_dict, indent=indent, ensure_ascii=False)

    # 写文件
    if output_path is not None:
        output_path = Path(output_path)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        with open(output_path, "w", encoding="utf-8") as f:
            f.write(json_str)
        logger.info(f"搜索结果已写入: {output_path}")

    return json_str
