# MSSA 超参自动优化工具

MSSA (Multi-Strategy Sparrow Search Algorithm) 多策略麻雀搜索算法，用于 MUPC AI 引擎
LSTM/Attention/BiLSTM/VMD 超参数自动优化。

## 快速开始

```bash
cd tools/mssa_optimizer
python -m mssa_optimizer --config mssa_search_config.yaml
```

## 编程接口

```python
from mssa_optimizer import MSSA, MSSAConfig, SearchSpace, create_objective, to_json
from mssa_optimizer.config import load_config

# 加载配置
config = load_config("mssa_search_config.yaml")

# 创建搜索空间
space = SearchSpace()

# 自定义目标函数（最小化）
def my_objective(hyperparams: dict) -> tuple:
    mape_pv = train_pv_model(hyperparams)
    mape_load = train_load_model(hyperparams)
    return mape_pv, mape_load

obj = create_objective(config, space, custom_runner=my_objective)

# 执行优化
optimizer = MSSA(config, space)
result = optimizer.optimize(obj)

# 输出结果
json_str = to_json(result, config, output_path="mssa_result.json")
print(f"Best MAPE: {result.best_fitness:.4f}")
print(f"Best hyperparams: {result.best_hyperparams}")
```

## 运行测试

```bash
python -m pytest test_mssa.py -v
```

## 配置文件

编辑 `mssa_search_config.yaml` 调整种群参数、终止条件和搜索空间。

## 依赖

- Python >= 3.9
- numpy >= 1.24
- PyYAML >= 6.0
- pytest (仅测试)
