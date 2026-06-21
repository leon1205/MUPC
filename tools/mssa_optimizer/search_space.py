"""MSSA 搜索空间定义与混合编码

10 维逻辑超参 → 14 维实际搜索向量，支持离散、连续、log-连续和枚举类型的
混合编码。提供编码/解码往返一致性保证。
"""

from __future__ import annotations

import math
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional, Sequence, Tuple, Union

import numpy as np


# ============================================================================
# 超参类型定义
# ============================================================================


@dataclass
class HyperParam:
    """单个超参的搜索空间定义。

    Attributes:
        name: 超参名称。
        kind: 类型：'discrete' | 'continuous' | 'integer' | 'enum' | 'log_continuous'。
        bounds: 编码后的搜索边界 (low, high)。
        discrete_values: 离散/枚举类型的合法值列表。
        decode_fn: 从编码值解码为实际超参值的函数。
        encode_fn: 从实际超参值编码为搜索向量元素的函数。
    """

    name: str
    kind: str  # discrete | continuous | integer | enum | log_continuous
    bounds: Tuple[float, float]
    dim_start: int
    dim_count: int
    discrete_values: Optional[List[Any]] = None
    decode_fn: Optional[callable] = None
    encode_fn: Optional[callable] = None

    def decode(self, vector_slice: np.ndarray) -> Any:
        """将搜索向量切片解码为实际超参值。"""
        if self.decode_fn is not None:
            return self.decode_fn(vector_slice, self)
        # 默认：浮点直接解码
        return float(np.clip(vector_slice[0], self.bounds[0], self.bounds[1]))

    def encode(self, value: Any) -> np.ndarray:
        """将实际超参值编码为搜索向量切片。"""
        if self.encode_fn is not None:
            return self.encode_fn(value, self)
        # 默认：浮点直接编码
        return np.array([float(value)], dtype=np.float64)

    def random(self, rng: np.random.Generator) -> np.ndarray:
        """在搜索空间内均匀随机采样一个向量切片。"""
        if self.kind == "discrete":
            idx = rng.integers(0, len(self.discrete_values))
            return np.array([float(idx)], dtype=np.float64)
        elif self.kind == "enum":
            n = len(self.discrete_values)
            one_hot = np.zeros(n, dtype=np.float64)
            one_hot[rng.integers(0, n)] = 1.0
            return one_hot
        else:
            low, high = self.bounds
            return np.array([rng.uniform(low, high)], dtype=np.float64)


# ============================================================================
# 解码/编码辅助函数
# ============================================================================


def _decode_discrete(slice_: np.ndarray, hp: HyperParam) -> Any:
    """离散类型解码：取最近的合法值索引。

    使用 floor(x + 0.5) 而非 round()，避免银行家舍入在 0.5 边界引入非确定性。
    设计评审 M-08 建议。
    """
    idx = int(np.clip(slice_[0], 0, len(hp.discrete_values) - 1) + 0.5)
    idx = max(0, min(idx, len(hp.discrete_values) - 1))
    return hp.discrete_values[idx]


def _encode_discrete(value: Any, hp: HyperParam) -> np.ndarray:
    """离散类型编码：将实际值映射为索引。"""
    try:
        idx = hp.discrete_values.index(value)
    except ValueError:
        raise ValueError(
            f"超参 '{hp.name}' 的值 {value} 不在离散选项 {hp.discrete_values} 中"
        )
    return np.array([float(idx)], dtype=np.float64)


def _decode_enum(slice_: np.ndarray, hp: HyperParam) -> Any:
    """枚举类型解码：取 argmax。"""
    idx = int(np.argmax(slice_))
    return hp.discrete_values[idx]


def _encode_enum(value: Any, hp: HyperParam) -> np.ndarray:
    """枚举类型编码：生成 one-hot 向量。"""
    try:
        idx = hp.discrete_values.index(value)
    except ValueError:
        raise ValueError(
            f"超参 '{hp.name}' 的值 '{value}' 不在枚举选项 {hp.discrete_values} 中"
        )
    one_hot = np.zeros(len(hp.discrete_values), dtype=np.float64)
    one_hot[idx] = 1.0
    return one_hot


def _decode_integer(slice_: np.ndarray, hp: HyperParam) -> Any:
    """整数类型解码：截断 + 取整。"""
    val = np.clip(slice_[0], hp.bounds[0], hp.bounds[1])
    return int(val + 0.5)


def _decode_log_continuous(slice_: np.ndarray, hp: HyperParam) -> Any:
    """log-连续类型解码：10^x 变换。"""
    val = np.clip(slice_[0], hp.bounds[0], hp.bounds[1])
    return float(10.0 ** val)


def _encode_log_continuous(value: Any, hp: HyperParam) -> np.ndarray:
    """log-连续类型编码：log10 变换。"""
    return np.array([math.log10(float(value))], dtype=np.float64)


# ============================================================================
# 默认搜索空间构建
# ============================================================================


def _build_default_params() -> List[HyperParam]:
    """构建 10 维超参的默认搜索空间定义。

    编码维度 = 14（设计 Section 12.5 显式索引映射）。
    逻辑超参数 = 10。

    Returns:
        超参定义列表（按编码向量顺序排列）。
    """
    params: List[HyperParam] = []

    # 1. hidden_size (离散, x[0], 1 dim)
    params.append(HyperParam(
        name="hidden_size",
        kind="discrete",
        bounds=(0, 3),
        dim_start=0,
        dim_count=1,
        discrete_values=[32, 64, 96, 128],
        decode_fn=_decode_discrete,
        encode_fn=_encode_discrete,
    ))

    # 2. num_layers (离散, x[1], 1 dim)
    params.append(HyperParam(
        name="num_layers",
        kind="discrete",
        bounds=(0, 2),
        dim_start=1,
        dim_count=1,
        discrete_values=[1, 2, 3],
        decode_fn=_decode_discrete,
        encode_fn=_encode_discrete,
    ))

    # 3. attn_score (枚举 one-hot 3 维, x[2:5])
    params.append(HyperParam(
        name="attn_score",
        kind="enum",
        bounds=(0, 1),
        dim_start=2,
        dim_count=3,
        discrete_values=["additive", "dot", "general"],
        decode_fn=_decode_enum,
        encode_fn=_encode_enum,
    ))

    # 4. vmd_k (整数, x[5], 1 dim)
    params.append(HyperParam(
        name="vmd_k",
        kind="integer",
        bounds=(2, 10),
        dim_start=5,
        dim_count=1,
        decode_fn=_decode_integer,
    ))

    # 5. vmd_alpha (连续, x[6], 1 dim)
    params.append(HyperParam(
        name="vmd_alpha",
        kind="continuous",
        bounds=(100, 5000),
        dim_start=6,
        dim_count=1,
    ))

    # 6. lr (log-连续, x[7], 1 dim, 编码空间 [-4, -2])
    params.append(HyperParam(
        name="lr",
        kind="log_continuous",
        bounds=(-4, -2),
        dim_start=7,
        dim_count=1,
        decode_fn=_decode_log_continuous,
        encode_fn=_encode_log_continuous,
    ))

    # 7. batch_size (离散, x[8], 1 dim)
    params.append(HyperParam(
        name="batch_size",
        kind="discrete",
        bounds=(0, 3),
        dim_start=8,
        dim_count=1,
        discrete_values=[16, 32, 64, 128],
        decode_fn=_decode_discrete,
        encode_fn=_encode_discrete,
    ))

    # 8. dropout (连续, x[9], 1 dim)
    params.append(HyperParam(
        name="dropout",
        kind="continuous",
        bounds=(0.0, 0.5),
        dim_start=9,
        dim_count=1,
    ))

    # 9. optimizer (枚举 one-hot 3 维, x[10:13])
    params.append(HyperParam(
        name="optimizer",
        kind="enum",
        bounds=(0, 1),
        dim_start=10,
        dim_count=3,
        discrete_values=["Adam", "AdamW", "RMSprop"],
        decode_fn=_decode_enum,
        encode_fn=_encode_enum,
    ))

    # 10. input_window (离散, x[13], 1 dim)
    params.append(HyperParam(
        name="input_window",
        kind="discrete",
        bounds=(0, 2),
        dim_start=13,
        dim_count=1,
        discrete_values=[12, 24, 36],
        decode_fn=_decode_discrete,
        encode_fn=_encode_discrete,
    ))

    return params


# ============================================================================
# SearchSpace 主类
# ============================================================================


@dataclass
class SearchSpace:
    """10 维逻辑超参 → 14 维搜索向量的搜索空间定义。

    Attributes:
        params: 超参定义列表。
        dim: 编码向量总维度（14）。
        bounds: 所有维度的 (low, high) 边界数组，形状 (dim, 2)。
        param_names: 超参名列表（10 个）。
    """

    params: List[HyperParam] = field(default_factory=_build_default_params)
    dim: int = field(init=False)
    bounds: np.ndarray = field(init=False)
    param_names: List[str] = field(init=False)

    def __post_init__(self):
        self.dim = self.params[-1].dim_start + self.params[-1].dim_count
        self.bounds = np.zeros((self.dim, 2), dtype=np.float64)
        for hp in self.params:
            for d in range(hp.dim_start, hp.dim_start + hp.dim_count):
                self.bounds[d] = hp.bounds
        self.param_names = [hp.name for hp in self.params]

    def apply_overrides(self, overrides: Dict[str, Any]) -> None:
        """应用搜索空间覆盖配置。

        支持范围覆盖（dict: {min, max}）和离散值列表覆盖（list）。

        Args:
            overrides: 来自配置文件的 search_space_overrides 字典。
        """
        for name, override in overrides.items():
            hp = self._find_param(name)
            if hp is None:
                continue

            if isinstance(override, list):
                # 离散/枚举值覆盖
                hp.discrete_values = override
                if hp.kind == "discrete":
                    hp.bounds = (0, len(override) - 1)
            elif isinstance(override, dict):
                # 范围覆盖
                new_low = override.get("min", hp.bounds[0])
                new_high = override.get("max", hp.bounds[1])
                if hp.kind == "log_continuous":
                    hp.bounds = (math.log10(new_low), math.log10(new_high))
                else:
                    hp.bounds = (float(new_low), float(new_high))
                # 更新全局 bounds 数组
                for d in range(hp.dim_start, hp.dim_start + hp.dim_count):
                    self.bounds[d] = hp.bounds

    def _find_param(self, name: str) -> Optional[HyperParam]:
        for hp in self.params:
            if hp.name == name:
                return hp
        return None

    def decode(self, vector: np.ndarray) -> Dict[str, Any]:
        """将 14 维搜索向量解码为 10 键超参字典。

        Args:
            vector: 编码向量，形状 (14,)。

        Returns:
            超参名字典，如 {'hidden_size': 64, 'lr': 0.001, ...}。

        Raises:
            ValueError: 向量维度不匹配。
        """
        vector = np.asarray(vector, dtype=np.float64)
        if vector.shape[-1] != self.dim:
            raise ValueError(
                f"向量维度 {vector.shape[-1]} 不匹配搜索空间维度 {self.dim}"
            )

        result: Dict[str, Any] = {}
        for hp in self.params:
            slice_ = vector[hp.dim_start : hp.dim_start + hp.dim_count]
            result[hp.name] = hp.decode(slice_)
        return result

    def encode(self, params: Dict[str, Any]) -> np.ndarray:
        """将超参字典编码为 14 维搜索向量。

        Args:
            params: 超参名字典。

        Returns:
            编码向量，形状 (14,)。

        Raises:
            ValueError: 超参名未知或值不合法。
        """
        vector = np.zeros(self.dim, dtype=np.float64)
        for name, value in params.items():
            hp = self._find_param(name)
            if hp is None:
                raise ValueError(f"未知超参: '{name}'")
            encoded = hp.encode(value)
            vector[hp.dim_start : hp.dim_start + hp.dim_count] = encoded
        return vector

    def random_sample(self, rng: Optional[np.random.Generator] = None) -> np.ndarray:
        """在搜索空间内均匀随机采样一个编码向量。

        Args:
            rng: numpy 随机数生成器。

        Returns:
            编码向量，形状 (14,)。
        """
        if rng is None:
            rng = np.random.default_rng()
        vector = np.zeros(self.dim, dtype=np.float64)
        for hp in self.params:
            vec = hp.random(rng)
            vector[hp.dim_start : hp.dim_start + hp.dim_count] = vec
        return vector

    def project(self, vector: np.ndarray) -> np.ndarray:
        """将向量投影到搜索空间合法范围内（裁剪）。

        Args:
            vector: 输入向量。

        Returns:
            裁剪后的向量。
        """
        vector = np.asarray(vector, dtype=np.float64).copy()
        for d in range(self.dim):
            vector[d] = np.clip(vector[d], self.bounds[d, 0], self.bounds[d, 1])
        return vector


# ============================================================================
# 模块级便捷函数
# ============================================================================

# 默认全局搜索空间实例
_default_space: Optional[SearchSpace] = None


def get_default_space() -> SearchSpace:
    """获取默认搜索空间实例（单例）。"""
    global _default_space
    if _default_space is None:
        _default_space = SearchSpace()
    return _default_space


def encode(params: Dict[str, Any], space: Optional[SearchSpace] = None) -> np.ndarray:
    """将超参字典编码为搜索向量。

    Args:
        params: 超参名字典。
        space: 搜索空间定义（None 则使用默认）。

    Returns:
        编码向量。
    """
    if space is None:
        space = get_default_space()
    return space.encode(params)


def decode(
    vector: np.ndarray, space: Optional[SearchSpace] = None
) -> Dict[str, Any]:
    """将搜索向量解码为超参字典。

    Args:
        vector: 编码向量。
        space: 搜索空间定义（None 则使用默认）。

    Returns:
        超参名字典。
    """
    if space is None:
        space = get_default_space()
    return space.decode(vector)


def random_sample(space: Optional[SearchSpace] = None) -> np.ndarray:
    """在搜索空间内均匀随机采样。

    Args:
        space: 搜索空间定义（None 则使用默认）。

    Returns:
        编码向量。
    """
    if space is None:
        space = get_default_space()
    return space.random_sample()
