# MUPC ↔ MUPC-AI2 跨项目一致性审计报告

> **审计日期**：2026-06-21 | **审计范围**：PRD v3.0 + 设计文档 v3.0 + Rust 推理代码 + Python 训练代码
> **审计方法**：逐模块交叉比对（文档 vs 文档 vs 代码），覆盖 ONNX 导出、LSTM 训练、MSSA 接口、配置对齐、动作/观测空间 5 个维度

---

## 一、阻塞级差异（C — 影响模型部署或训练正确性）

### C-1. `error_correction.py` 缺失

| 项目 | 状态 |
|------|------|
| MUPC 推理端 | `residual_buffer.rs` + `prediction_pipeline.rs` 已实现误差修正管线 |
| 上游改造要求 §1.1 | 明确列出 `error_correction.rknn`（可选但接口已定义） |
| MUPC-AI2 设计文档 §7 | `ErrorCorrectionBiLSTM` 类设计完成 |
| MUPC-AI2 代码 | **文件不存在** |

**影响**：误差修正 BiLSTM 无法训练和导出，MUPC 推理端 `execute_error_correction()` 无模型可用。

**修复建议**：创建 `error_correction.py`，实现：
1. `ErrorCorrectionBiLSTM(nn.Module)` — 独立轻量 BiLSTM（hidden=32, num_layers=1）
2. `ErrorCorrectionTrainer` — 主模型预测→残差序列→训练修正模型
3. `export_onnx.export_error_correction()` — ONNX 导出 + metadata_props

---

### C-2. BiLSTM 训练代码空缺

| 项目 | 状态 |
|------|------|
| MUPC 推理端 | `model_manager.rs` 管理双 `RknnRuntime`（单向 + 双向），`BiLstmConfig.gate_passed` 控制启用 |
| MUPC-AI2 `export_onnx.py` | 接受 `--bidirectional` 参数，写入 metadata `direction="bidirectional"` |
| MUPC-AI2 `lstm_model.py` | `nn.LSTM` **不含 `bidirectional=True`**，无双向训练逻辑 |

**影响**：`bilstm_attn.rknn` 无法产出；BiLSTM 硬件延迟摸底（上游改造要求 §5.3）无模型可测。

**修复建议**：
1. `LSTMForecast.__init__` 新增 `bidirectional: bool = False` 参数
2. `nn.LSTM(..., bidirectional=bidirectional)` 
3. 若 bidirectional=True 且 with_attention=True，LSTM hidden 维度折半以控制参数量 ≤ 单向的 2.2x

---

### C-3. `input_seq_len` vs `input_window` 双窗口不一致

`lstm_model.py` 存在两个窗口参数，MSSA 搜索对训练无效：

```python
# lstm_model.py:400-401
LSTM_TRAIN_CONFIG = {
    "input_seq_len": 8,     # prepare_data() 实际使用的窗口
    "input_window": 12,     # MSSA 搜索的值，但训练不使用
}

# lstm_model.py:436 — 始终使用 input_seq_len
seq_len = self.config["input_seq_len"]  # = 8，恒为 8
```

**影响**：MSSA 搜索 `input_window ∈ {12, 24, 36}` 不改变训练窗口，导出 ONNX 输入 shape 恒为 `[batch, 8, 7]`，与 MUPC 推理端预期的 MSSA 最优窗口不一致。

**修复建议**：`prepare_data()` 改用 `cfg.get("input_window", cfg["input_seq_len"])`，删除 `input_seq_len` 遗留字段。

---

## 二、高优先级差异（H — 数据/配置不一致，可能导致静默错误）

### H-1. 合同需量三重冲突

| 位置 | 值 | 优先级 |
|------|-----|--------|
| `config/mupc_env_config.yaml:41` | `contract_demand_kw: 300.0` | YAML 覆盖 |
| `config/config_manager.py:59` | `contract_demand_kw: float = 300.0` | 代码默认值 |
| `mupc_env/constants.py:32` | `CONTRACT_DEMAND_KW = 200.0` | 常量（v2.17 已修） |
| MUPC 下游 `data_fusion.rs:105` | `contract_demand: 200.0` | Rust 默认值 |

F8 修复（`待处理一致性任务清单.md`）仅改了 `constants.py`，但 YAML 和 `config_manager.py` 仍是 300。**使用 `--config` 时 300 覆盖 200**，需量控制奖励计算使用错误的分母归一化值。

**修复建议**：同步修改 3 处：`mupc_env_config.yaml`、`config_manager.py` 默认值、`constants.py`（已完成），统一为 200。

---

### H-2. 已导出 ONNX 模型缺少 metadata_props

| 文件 | metadata |
|------|----------|
| `exported_models/lstm_forecast_20260608_102120.onnx` | **0 个 key** |
| `export_onnx.py` 最新代码 | 10 个 key（正确） |
| MUPC 推理端 `model_validator.rs` | 启动时校验 10 个 key |

**影响**：旧 ONNX 模型部署到 MUPC 推理端时 `validate_rknn_model()` 校验失败，模型拒绝加载。

**修复建议**：用最新 `export_onnx.py` 重新导出 LSTM ONNX 模型（含完整 metadata_props）。

---

### H-3. Config YAML 动作空间声明过时

`config/mupc_env_config.yaml:85-98` 仍定义 5 维动作空间（`p_ref, k_droop, load_shedding, pv_limit, confidence`），注释"对齐下游 v2.13"。实际代码自 v2.15 起已精简为 2 维。

**修复建议**：更新 YAML 中 `action_space` 节为 2 维定义，对齐 `action_validator.py` 实际实现。

---

### H-4. 配置版本指纹缺失

| 项目 | 配置文件 | 版本指纹 |
|------|----------|----------|
| MUPC 推理端 | `mupc/config/mupc_env_config.yaml` | `v2.6-20260611` |
| MUPC-AI2 训练端 | `config/mupc_env_config.yaml` | **无 version 字段** |

MUPC 推理端 `dynamic_config_loader.rs` 启动时校验版本指纹，训练侧无指纹意味着版本对齐只能人工保证。v3.0 预测增强架构引入后指纹应更新为 `v3.0-20260621`。

**修复建议**：
1. MUPC-AI2 `config/mupc_env_config.yaml` 新增 `version` 节
2. MUPC `mupc/config/mupc_env_config.yaml` 指纹更新为 `v3.0-20260621`

---

### H-5. MUPC 推理端配置缺少训练侧物理字段

MUPC `mupc/config/mupc_env_config.yaml` 缺少以下训练侧已使用字段：

| 缺失字段 | 训练侧值 | 用途 |
|----------|----------|------|
| `q_batt_max_kvar` | 300.0 | 最大无功输出 |
| `pv_array_kw` | 150.0 | 光伏容量 |
| `load_peak_kw` | 60.0 | 负荷峰值 |
| `battery_charge_efficiency` | 0.90 | 充电效率 |
| `battery_discharge_efficiency` | 0.90 | 放电效率 |

**修复建议**：在 MUPC `mupc_env_config.yaml` 中补全这些字段，保持与训练侧物理参数一致。

---

## 三、中优先级差异（M — 代码/文档漂移，不影响当前运行但有隐患）

### M-1. CLAUDE.md 动作空间声明过期

`MUPC-AI2/CLAUDE.md:102-107`：
```
动作空间（3维）：[p_batt, load_shedding, pv_limit]
```
实际代码（v2.15+）为 2 维 `[p_ref, k_droop]`。

**修复建议**：更新 CLAUDE.md 动作空间描述，补充 v2.15 精简说明。

---

### M-2. MSSA `KEY_MAP` 映射不完整

`train.py:194`：
```python
KEY_MAP = {"hidden_size": "hidden_dim", "lr": "learning_rate"}
```

MSSA 传入 10 个超参，仅 2 个被显式映射。`num_layers`, `batch_size`, `dropout` 因命名一致恰好可用；`attn_score`, `vmd_k`, `vmd_alpha`, `optimizer`, `input_window` 的语义和取值范围未经训练脚本校验。

**修复建议**：
1. 补全 KEY_MAP 映射表（含类型转换/范围校验）
2. `parse_mssa_config()` 增加对未知 key 的 WARN 日志

---

### M-3. `load_mic_features` 返回值简化

| 位置 | 签名 |
|------|------|
| `train.py:138` | `-> list[str]` |
| MUPC-AI2 设计文档 §5 | `-> tuple[list[str], int]`（含 top_k） |
| MUPC PRD §14.3.3.1 | MIC JSON 同时包含 `top_k` 和 `features` |

代码丢弃了 `top_k`，若实际选中特征数 ≠ `top_k` 时无法感知。

**修复建议**：`load_mic_features()` 返回 `tuple[list[str], int]`，当 `len(selected) != top_k` 时输出 WARN。

---

### M-4. `prepare_data()` 样本构造硬编码 8 步

`lstm_model.py:436`：`seq_len = self.config["input_seq_len"]` 恒为 8。与 C-3 同一根因。MSSA 搜索的 `input_window` 值不参与样本构造。

**修复建议**：同 C-3。

---

## 四、低优先级差异（D — 已知记录，有意差异或待后续处理）

| 编号 | 差异点 | 记录文档 | 状态 |
|------|--------|----------|------|
| D-1 | SCENE-B1 过载惩罚差异 | 待处理一致性任务清单 | 本地多 w3 过载惩罚，下游无 |
| D-2 | SCENE-B3 VPP 经济模型差异 | 待处理一致性任务清单 | 下游为占位实现 |
| D-3 | SCENE-B5 绿色消耗定义不同 | 待处理一致性任务清单 | 设计理念不同 |
| D-4 | SCENE-01 光伏消纳公式差异 | 待处理一致性任务清单 | 衡量维度不同 |
| D-5 | D10 语义差异（17 vs 45 维） | 待处理一致性任务清单 D-7 | 未来扩展 |
| D-6 | 输入窗口长度差异（8 vs 24 步） | 待处理一致性任务清单 D-6 | 本报告 C-3 跟踪 |

---

## 五、汇总统计

| 严重度 | 数量 | 涉及模块 |
|--------|------|----------|
| C（阻塞） | 3 | `error_correction.py`(缺失)、`lstm_model.py`(BiLSTM+窗口) |
| H（高） | 5 | `config/`(YAML+默认值)、`exported_models/`(旧ONNX) |
| M（中） | 4 | `train.py`(KEY_MAP)、`CLAUDE.md`、`lstm_model.py` |
| D（低） | 6 | 已知差异，已记录于待处理清单 |

**核心结论**：v3.0 预测增强架构在推理端（MUPC）已完成全部三轮代码实现（VMD + Attention + BiLSTM + 误差修正 + MSSA），但训练侧（MUPC-AI2）改造完成度约 **60%**：

- **已完成**：R1 LSTM + AdditiveAttention + metadata_props + `--config`/`--mic` CLI + stdout MAPE + `compute_data_fingerprint()`
- **空缺**：R2 BiLSTM 训练 + 误差修正模型训练 + `input_window` 参数贯通 + 配置版本同步
- **待修复**：contract_demand 冲突 + ONNX 重新导出 + Config YAML 清理

---

**文档状态**：待评审
