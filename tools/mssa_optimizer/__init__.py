"""MSSA 超参自动优化工具

MSSA (Multi-Strategy Sparrow Search Algorithm) 多策略麻雀搜索算法，
用于 MUPC AI 引擎 LSTM/Attention/BiLSTM/VMD 超参数自动优化。

主要 API:
    from mssa_optimizer import MSSA, MSSAConfig, SearchSpace
    optimizer = MSSA(config)
    result = optimizer.optimize(objective_fn)
"""

try:
    from .config import MSSAConfig, load_config, validate_config
    from .search_space import SearchSpace, HyperParam, encode, decode, random_sample
    from .mssa import MSSA, OptimizationResult
    from .objective import ObjectiveFunc, create_objective
    from .output import SearchOutput, to_json, validate_output
except ImportError:
    from config import MSSAConfig, load_config, validate_config
    from search_space import SearchSpace, HyperParam, encode, decode, random_sample
    from mssa import MSSA, OptimizationResult
    from objective import ObjectiveFunc, create_objective
    from output import SearchOutput, to_json, validate_output

__version__ = "1.0.0"
__all__ = [
    "MSSA",
    "MSSAConfig",
    "SearchSpace",
    "HyperParam",
    "OptimizationResult",
    "ObjectiveFunc",
    "SearchOutput",
    "load_config",
    "validate_config",
    "encode",
    "decode",
    "random_sample",
    "create_objective",
    "to_json",
    "validate_output",
]
