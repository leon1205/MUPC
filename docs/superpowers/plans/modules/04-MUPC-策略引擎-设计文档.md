# MUPC 策略引擎模块设计文档

> **版本：** v2.16（2026-08-31）

> **文档定位：** 本文档记录实现级设计决策（架构、Rust 结构体/trait、状态机、配置结构、测试策略、文件组织）。需求级内容（功能描述、验收标准、性能指标）请参考 [04-MUPC-策略引擎-PRD](../specs/modules/04-MUPC-策略引擎-PRD.md)。

> **v2.16 变更（2026-08-31）：** 新增「第 4 策略：台区储能治理策略」（AI 失效时的台区治理兜底），整合自 [2026-08-25 台区储能控制策略设计](2026-08-25-台区储能控制策略-design.md)。含：扩展 `DataPackage` 分相字段、扩展 `ControlCommand` 分相设定、扩展核间协议 V3 帧（分相 P/Q 下发）、带状态控制器设计、离线回放验证。

---

## 目录

1. [模块架构](#1-模块架构)
2. [削峰填谷策略](#2-削峰填谷策略)
3. [需量控制策略](#3-需量控制策略)
4. [防逆流保护策略](#4-防逆流保护策略)
5. [电压越限与三相不平衡无功补偿（接口预留）](#5-电压越限与三相不平衡无功补偿接口预留)
6. [AI 指令安全校验](#6-ai-指令安全校验)
7. [AI 引擎集成](#7-ai-引擎集成)
8. [策略模式切换](#8-策略模式切换)
9. [接口定义](#9-接口定义)
10. [文件结构](#10-文件结构)
11. [错误处理](#11-错误处理)
12. [配置管理](#12-配置管理)
13. [测试体系](#13-测试体系)
14. [演进路线](#14-演进路线)
15. [台区储能治理策略（第 4 策略）](#15-台区储能治理策略第-4-策略)

---

## 1. 模块架构

### 1.1 定位与职责

策略引擎（Strategy Engine）是 MUPC 通信管理模块的**本地决策核心**，对应 workspace crate `mupc-strategy-engine`。

**核心职责：**
- 提供三种兜底策略：削峰填谷、需量控制、防逆流保护
- 对 AI 引擎输出的指令进行安全校验（`AiCommandValidator`）
- 管理策略模式切换（AI 模式 / 本地兜底模式 / 基础模式）
- 通过消息总线接收遥测数据，输出控制指令
- 集成 AI 优化引擎（Phase 3C）

### 1.2 "AI 优先、本地兜底"机制

```
AI 引擎正常运行:
  AI 决策 → AiCommandValidator 安全校验 → 通过 → 指令下发
                                        → 不通过 → 降级至本地策略

AI 引擎失效:
  检测异常（心跳/状态码）→ 自动切换至本地策略引擎 → 发出告警
  → 本地策略接管控制 → AI 引擎恢复后自动切回 AI 模式
```

| 模式 | 决策源 | 校验方式 | 适用场景 |
|------|--------|----------|----------|
| AI 智能模式 | LSTM/TCN + MADDPG/PPO | AiValidator 安全校验 | 默认运行模式 |
| 本地兜底模式 | 削峰填谷/需量控制/防逆流 | 策略内置边界检查 | AI 失效/指令校验不通过 |
| 基础模式 | 无自动控制 | 手动操作 | 调试/维护 |

### 1.3 模块依赖关系

```
mupc-strategy-engine
  ├── mupc-common              (MupcError, ErrorCode)
  ├── mupc-data-processing     (DataPackage, telemetry 类型)
  └── mupc-ai-engine           (ModelManager, FusedSystemState, ActionOutput, ModelStatus)
```

### 1.4 整体数据流

```
intercore (TCP/RJ45)
    │
    ▼
DataCollector → HighFrequencyTelemetry (1Hz)
    │
    ▼
Message Bus (tokio::sync::mpsc)
    │
    ▼
strategy-engine
    ├── 削峰填谷 (PeakShavingStrategy)       ← cmd_id: 1
    ├── 需量控制 (DemandControlStrategy)     ← cmd_id: 2
    ├── 防逆流 (AntiReverseStrategy)         ← cmd_id: 3
    │
    ▼
AiCommandValidator (可插拔 AI 模型)
    │
    ▼
┌──────────────────────────────────────────────────────────────┐
│  AI→  p_ref + k_droop → IntercoreClient → 实时控制模块     │
│  本地策略→ pv_limit / load_shedding → SouthCommandDispatcher → 南向设备 │
└──────────────────────────────────────────────────────────────┘
```

> **分发路径更新：** p_ref + k_droop 由 AI 引擎输出并通过核间通信下发至实时控制模块。pv_limit 和 load_shedding 不再作为 AI 动作维度，仅由本地兜底策略（防逆流/需量控制）设置并通过南向通信分发至设备。

### 1.5 策略 ID 分配

| 策略 | cmd_id | 说明 |
|------|--------|------|
| 削峰填谷 | 1 | 固定 ID，用于日志追踪和消息路由 |
| 需量控制 | 2 | 固定 ID |
| 防逆流 | 3 | 固定 ID |
| 保留 | 4-10 | 供 Phase 2+ 扩展策略使用 |

### 1.6 性能与可靠性

> 非功能性需求详见 [PRD §10](../specs/modules/04-MUPC-策略引擎-PRD.md)。本条记录设计层面的关键实现约束：策略决策延迟 < 100ms、单实例内存 < 10MB、三策略并发互不阻塞、模式切换 < 50ms。

---

## 2. 削峰填谷策略

> 功能需求详见 [PRD §2](../specs/modules/04-MUPC-策略引擎-PRD.md)。本节记录实现级设计。

### 2.1 架构

- **结构体**: `PeakShavingStrategy`
- **配置**: `PeakShavingConfig`（位于 `config.rs`）
- **接口**: 实现 `FallbackStrategy` trait
- **文件**: `peak_shaving.rs`

### 2.2 决策逻辑

```
充电优先级：
  1. 谷时 + PV 低 → 电网充电（15kW）
  2. 谷时 + PV 高 → PV 充电（min(PV, 30kW)）

放电优先级：
  1. 峰时 → 放电（25kW）

边界保护：
  - SOC < soc_charge_min（20%）→ 强制充电（20kW）
  - SOC > soc_charge_max（80%）→ 强制放电（-20kW）
  - 非峰非谷的平时段 → 待机（0kW，PowerRegulation 模式）
```

代码实现路径：

```rust
fn decide(&self, battery_soc: f64, pv_power: f64, _load_power: f64,
          is_peak: bool, is_valley: bool) -> (f64, CommandType) {
    if battery_soc < self.config.soc_charge_min {
        // 强制充电 20kW
        (20.0, CommandType::ChargeDischarge)
    } else if battery_soc > self.config.soc_charge_max {
        // 强制放电 -20kW
        (-20.0, CommandType::ChargeDischarge)
    } else if is_valley {
        // 谷时：PV 充足则用 PV 充电，不足则电网充电
        let p_batt = if pv_power > 10.0 { pv_power.min(30.0) } else { 15.0 };
        (p_batt, CommandType::ChargeDischarge)
    } else if is_peak {
        // 峰时：放电 25kW
        (-25.0, CommandType::ChargeDischarge)
    } else {
        // 平时：待机
        (0.0, CommandType::PowerRegulation)
    }
}
```

### 2.3 时段检测规则

峰时段检测和谷时段检测使用同一套跨天兼容的规则：

```rust
fn is_peak_hour(&self, hour: u8) -> bool {
    self.config.peak_hours.iter().any(|(start, end)| {
        if *start <= *end {
            hour >= *start && hour < *end    // 同天
        } else {
            hour >= *start || hour < *end    // 跨天（如 23:00-07:00）
        }
    })
}
```

从 Unix 时间戳提取小时（u64 截断到当日秒）：

```rust
let hour = (data.timestamp % 86400) / 3600;
```

### 2.4 配置参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `peak_hours` | `Vec<(u8, u8)>` | `[(8, 11), (18, 21)]` | 峰时段列表，(起始小时, 结束小时) |
| `valley_hours` | `Vec<(u8, u8)>` | `[(23, 7)]` | 谷时段列表，(起始小时, 结束小时) |
| `soc_charge_max` | `f64` | `80.0` | SOC 充电上限（%） |
| `soc_charge_min` | `f64` | `20.0` | SOC 充电下限（%） |
| `battery_capacity` | `f64` | `100.0` | 电池容量（kWh） |

### 2.5 输出字段

| 字段 | 值 | 说明 |
|------|-----|------|
| `cmd_id` | 1 | 削峰填谷策略固定 ID |
| `cmd_type` | `ChargeDischarge` / `PowerRegulation` | 充放电控制或待机 |
| `p_ref` | ±15~30 kW | 有功基准点（双参数模式） |
| `priority` | 1 | 默认优先级 |

### 2.6 测试覆盖

| 测试用例 | 文件 | 验证点 |
|----------|------|--------|
| `test_peak_hours_detection` | `peak_shaving_test.rs` | 峰时段检测边界 |
| `test_valley_hours_detection` | `peak_shaving_test.rs` | 谷时段检测边界（含跨天） |
| `test_discharge_at_peak_when_soc_high` | `peak_shaving_test.rs` | 峰时 + SOC > 80% 放电 -20kW |
| `test_charge_at_valley_when_soc_low` | `peak_shaving_test.rs` | 谷时 + SOC < 20% 充电 20kW |
| `test_charge_at_valley_with_pv` | `peak_shaving_test.rs` | 谷时 + PV 充足按 PV 充电 |
| `test_discharge_at_peak` | `peak_shaving_test.rs` | 峰时 + SOC 正常放电 -25kW |
| `test_idle_at_normal_hours` | `peak_shaving_test.rs` | 平时段待机 |
| `test_soc_too_low_force_charge` | `peak_shaving_test.rs` | SOC 低于下限强制充电 |
| `test_soc_too_high_force_discharge` | `peak_shaving_test.rs` | SOC 高于上限强制放电 |

---

## 3. 需量控制策略

> 功能需求详见 [PRD §3](../specs/modules/04-MUPC-策略引擎-PRD.md)。

### 3.1 架构

- **结构体**: `DemandControlStrategy`
- **配置**: `DemandControlConfig`（位于 `config.rs`）
- **接口**: 实现 `FallbackStrategy` trait
- **文件**: `demand_control.rs`

### 3.2 决策逻辑

```
负载率计算：transformer_load = (load_power + ev_charger_power) / transformer_capacity

Level 0 (负载率 ≤ warning_threshold = 80%):
  无动作。电池待机，不放电不充电。
  优先级：0

Level 1 (80% < 负载率 ≤ action_threshold = 90%):
  电池放电补偿（-10kW）
  不切除负荷
  优先级：1

Level 2 (90% < 负载率 ≤ emergency_threshold = 95%):
  电池放电（-20kW）
  切除次要负荷（10kW）
  cmd_type 切换为 SwitchControl
  优先级：2

Level 3 (负载率 > 95%):
  紧急放电（-30kW）
  强制切除非重要负荷（20kW）
  cmd_type 切换为 SwitchControl
  优先级：3

低 SOC 保护：
  当 SOC < 20% 且需要放电时，放电功率限制为 max(-10kW)
```

### 3.3 配置参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `transformer_capacity` | `f64` | `200.0` | 变压器容量（kVA） |
| `demand_factor` | `f64` | `0.85` | 需量因子 |
| `warning_threshold` | `f64` | `0.80` | 预警阈值（Level 1 触发） |
| `action_threshold` | `f64` | `0.90` | 行动阈值（Level 2 触发） |
| `emergency_threshold` | `f64` | `0.95` | 紧急阈值（Level 3 触发） |

### 3.4 输出字段

| 字段 | 值 | 说明 |
|------|-----|------|
| `cmd_id` | 2 | 需量控制策略固定 ID |
| `cmd_type` | `PowerRegulation` / `SwitchControl` | Level 1 为功率调节，Level >= 2 为开关控制 |
| `p_ref` | -10/-20/-30 kW | 有功基准点，按等级决定放电功率 |
| `load_shedding` | `Some(10/20 kW)` | Level >= 2 时执行负荷切除 |
| `priority` | 0~3 | 对应策略等级 |

### 3.5 测试覆盖

| 测试用例 | 文件 | 验证点 |
|----------|------|--------|
| `test_transformer_load_calculation` | `demand_control_test.rs` | 负载率计算正确性 |
| `test_level_0_normal` | `demand_control_test.rs` | 负载率 <= 80% 无动作 |
| `test_level_1_warning` | `demand_control_test.rs` | 80%-90% 放电 -10kW |
| `test_level_2_action` | `demand_control_test.rs` | 90%-95% 放电 -20kW + 切负荷 10kW |
| `test_level_3_emergency` | `demand_control_test.rs` | > 95% 紧急放电 -30kW + 切负荷 20kW |
| `test_low_soc_protection` | `demand_control_test.rs` | SOC < 20% 放电受限 |
| `test_custom_thresholds` | `demand_control_test.rs` | 支持自定义阈值配置 |

---

## 4. 防逆流保护策略

> 功能需求详见 [PRD §4](../specs/modules/04-MUPC-策略引擎-PRD.md)。

### 4.1 架构

- **结构体**: `AntiReverseStrategy`
- **配置**: `AntiReverseConfig`（位于 `config.rs`）
- **接口**: 实现 `FallbackStrategy` trait
- **文件**: `anti_reverse.rs`
- **注意**: 该策略的 `evaluate_sync` 需要 `&mut self`，因其内部维护 `pv_limit_count` 状态

### 4.2 决策逻辑

```
触发条件：grid_power < reverse_power_threshold（默认 -0.1kW，允许微小逆流）

Step 1 - 电池有余量（SOC < soc_charge_max = 80%）：
  电池充电功率 = min(PV出力 × 0.8, max_charge_power = 50kW)
  不限制 PV 出力

Step 2 - 电池已满（SOC >= soc_charge_max）：
  限制 PV 出力 = PV出力 × (pv_limit_count × 0.1)，上限 50%
  电池不充电
  pv_limit_count 递增

Step 3 - PV 已限制且仍逆流：
  触发告警（通过消息总线发送，Phase 2+ 实现）

恢复正常：
  当 grid_power >= reverse_power_threshold 时，恢复正常运行
  pv_limit_count 清零
```

### 4.3 状态管理

| 状态字段 | 初始值 | 说明 |
|----------|--------|------|
| `pv_limit_count` | 0 | 连续 PV 限制次数，每次逆流且电池满时递增，恢复正常时清零 |

### 4.4 配置参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `reverse_power_threshold` | `f64` | `-0.1` | 逆功率阈值（kW），允许微小逆流 |
| `pv_limit_step` | `f64` | `0.10` | 每次 PV 限制幅度（比例） |
| `max_charge_power` | `f64` | `50.0` | 最大充电功率（kW） |
| `soc_charge_max` | `f64` | `80.0` | SOC 充电上限（%） |

### 4.5 输出字段

| 字段 | 值 | 说明 |
|------|-----|------|
| `cmd_id` | 3 | 防逆流策略固定 ID |
| `cmd_type` | `PowerRegulation` | 功率调节 |
| `p_ref` | 正数（充电）/ 0 | 有功基准点，消纳逆流功率 |
| `pv_limit` | `Some(0.0~0.5)` / `None` | PV 限功率比例，无限制时为 None |
| `priority` | 2 | 默认优先级 |

### 4.6 测试覆盖

| 测试用例 | 文件 | 验证点 |
|----------|------|--------|
| `test_anti_reverse_charge_when_grid_reverse_and_battery_not_full` | `anti_reverse_test.rs` | 逆流时电池充电 |
| `test_anti_reverse_limit_pv_when_battery_full` | `anti_reverse_test.rs` | 逆流 + 电池满，限制 PV |
| `test_anti_reverse_no_action_when_grid_normal` | `anti_reverse_test.rs` | 正常功率时无动作 |
| `test_strategy_type` | `anti_reverse_test.rs` | 策略类型标记为 Fallback |
| `test_strategy_name` | `anti_reverse_test.rs` | 策略名称返回 "AntiReverseStrategy" |

---

## 5. 电压越限与三相不平衡无功补偿（接口预留）

### 5.1 概述

通过电池逆变器提供无功功率支撑，改善台区电压质量和三相不平衡度。当前为**接口预留阶段**，完整的决策逻辑和实现延后至后续 Phase。

### 5.2 已预留接口

`ControlCommand` 中已包含以下字段，供无功补偿策略使用：

| 字段 | 类型 | 用途 | 范围 |
|------|------|------|------|
| `q_batt_set` | `Option<f64>` | 无功由实时控制模块闭环调节 | - |
| `phase_compensation` | `Option<[f64; 3]>` | A/B/C 三相分相补偿系数 | 各相独立设置 |

### 5.3 计划策略（Phase 2+）

| 策略 | 触发条件 | 动作 |
|------|----------|------|
| 电压越限补偿 | 电压超出额定范围 ±7%（或 ±10%，按国标要求） | 电池吸收/发出无功 |
| 三相不平衡补偿 | 三相电流不平衡度 > 15% | 分相无功补偿 |

---

## 6. AI 指令安全校验

### 6.1 概述

`AiCommandValidatorImpl` 作为 AI 引擎与执行层之间的**安全闸门**，对所有 AI 决策指令进行校验。校验不通过时自动降级至本地兜底模式。

### 6.2 架构

- **trait**: `AiCommandValidator`（定义于 `strategies.rs`）
- **实现**: `AiCommandValidatorImpl`（定义于 `ai_validator.rs`）
- **可插拔 AI 模型**: `AiModel` trait（定义于 `ai_validator.rs`）
- **默认模型**: `MockAiModel`（模拟预测逻辑）

### 6.3 接口定义

```rust
/// AI 指令校验器 Trait（可插拔）
#[async_trait]
pub trait AiCommandValidator: Send + Sync {
    async fn validate(&self, cmd: &ControlCommand) -> ValidationResult;
    fn name(&self) -> &str;
}

/// AI 模型 Trait（可插拔，Phase 3C 替换为 LSTM/TCN）
pub trait AiModel: Send + Sync {
    fn predict(&self, input: &ModelInput) -> ModelOutput;
}

/// 模型输入
pub struct ModelInput {
    pub battery_soc: f64,     // 电池 SOC（0.0-1.0）
    pub pv_power: f64,        // 光伏功率（kW）
    pub load_power: f64,      // 负荷功率（kW）
    pub grid_power: f64,      // 电网功率（kW）
}

/// 模型输出
pub struct ModelOutput {
    pub recommended_p_batt: f64,  // 推荐电池功率（kW）
    pub confidence: f64,          // 置信度（0.0-1.0）
}
```

### 6.4 校验规则

```rust
pub fn validate_sync(&self, cmd: &ControlCommand) -> ValidationResult {
    // 1. 无模型时默认通过
    if self.model.is_none() {
        return ValidationResult::valid();
    }

    // 2. 只校验功率调节命令，开关命令直接通过
    if cmd.cmd_type != CommandType::PowerRegulation {
        return ValidationResult::valid();
    }

    // 3. 无 p_ref 时默认通过（双参数模式）
    let p_ref = match cmd.p_ref {
        Some(p) => p,
        None => return ValidationResult::valid(),
    };

    // 4. 调用 AI 模型预测比较
    let model_output = model.predict(&model_input);
    let diff = (p_batt - model_output.recommended_p_batt).abs();
    if diff > 10.0 && model_output.confidence < 0.7 {
        return ValidationResult::invalid("...");
    }

    ValidationResult::valid()
}
```

### 6.5 MockAiModel 模拟逻辑

```rust
impl AiModel for MockAiModel {
    fn predict(&self, input: &ModelInput) -> ModelOutput {
        let recommended_p_batt = if input.battery_soc > 0.8 {
            (input.pv_power - input.load_power).max(0.0)   // SOC 高，优先放电
        } else if input.battery_soc < 0.2 {
            (input.pv_power - input.load_power).min(0.0)   // SOC 低，优先充电
        } else {
            0.0                                              // SOC 中等，待机
        };
        ModelOutput { recommended_p_batt, confidence: 0.5 }
    }
}
```

### 6.6 降级流程

```
AiCommandValidator.validate(cmd)
  ├── 校验通过 → 指令继续下发
  └── 校验不通过 →
        ├── 记录告警日志
        ├── 丢弃 AI 指令
        ├── 切换至本地兜底模式
        └── FallbackStrategy.evaluate(data) 生成兜底指令
```

### 6.7 测试覆盖

| 测试用例 | 文件 | 验证点 |
|----------|------|--------|
| `test_mock_ai_model_predict_high_soc` | `ai_validator_test.rs` | SOC 高时推荐放电 |
| `test_mock_ai_model_predict_low_soc` | `ai_validator_test.rs` | SOC 低时推荐充电 |
| `test_mock_ai_model_predict_mid_soc` | `ai_validator_test.rs` | SOC 中等时推荐待机 |
| `test_validator_without_model` | `ai_validator_test.rs` | 无模型时校验默认通过 |
| `test_validator_with_model` | `ai_validator_test.rs` | 有模型时调用 predict 校验 |
| `test_validator_switch_command_passthrough` | `ai_validator_test.rs` | 开关命令直接通过 |
| `test_validator_name` | `ai_validator_test.rs` | 校验器名称返回正确 |
| `test_validator_async_validate` | `ai_validator_test.rs` | 异步 validate 接口正常 |

---

## 7. AI 引擎集成

### 7.1 概述

`AiIntegrator` 负责管理 AI 模型生命周期，提供 AI 决策接口。位于 `ai_integration.rs`，仅在 Phase 3C 启用。

### 7.2 结构体定义

```rust
pub struct AiIntegrator {
    model_manager: Arc<RwLock<Option<ModelManager>>>,
    status: Arc<RwLock<ModelStatus>>,
}
```

### 7.3 关键方法

| 方法 | 说明 | 异步 |
|------|------|------|
| `new()` | 创建 AI 集成器，初始状态为 Unloaded | 否 |
| `initialize(config)` | 加载 AI 模型 | 是 |
| `get_decision(state)` | 获取 AI 决策 | 是 |
| `is_ready()` | 检查 AI 是否就绪 | 是 |
| `status()` | 获取当前状态 | 是 |

### 7.4 状态管理

| AiIntegrator 状态 | 策略模式 | 说明 |
|--------------------|----------|------|
| `Unloaded` | Fallback / Basic | 模型未加载，使用兜底策略 |
| `Loading` | Fallback | 模型加载中，暂用兜底策略 |
| `Ready` | Intelligent | 模型就绪，AI 决策 + Validator 校验 |
| `Error` | Fallback | 模型异常，自动降级 |

### 7.5 数据集成

```rust
// strategy-engine 通过 AiIntegrator 集成 AI 引擎
strategy-engine ←→ AiIntegrator ←→ ai-engine::ModelManager
                                  ├── 决策接口 → ActionOutput (p_ref, k_droop)
                                  └── 状态管理 → ModelStatus

数据流：
1. LSTM/TCN 时序预测（光伏出力/负荷）
2. MADDPG/PPO 基于预测结果决策，输出 2 维动作（p_ref, k_droop）
3. AiCommandValidator 校验 AI 指令安全性
4. AI 指令分发：
   - p_ref + k_droop → IntercoreClient → 实时控制模块（闭环下垂控制）
5. 本地策略独立执行（不经过 AI）：
   - pv_limit → 防逆流策略(AntiReverseStrategy) → SouthCommandDispatcher → 光伏逆变器
   - load_shedding → 需量控制策略(DemandControlStrategy) → SouthCommandDispatcher → 负荷控制装置
```

---

## 8. 策略模式切换

### 8.1 模式定义

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrategyType {
    Basic,         // 基础模式 - 无自动控制
    Intelligent,   // 智能模式 - AI 引擎决策
    Fallback,      // 兜底模式 - 本地策略引擎
}
```

### 8.2 切换触发器

| 当前模式 | 切换条件 | 目标模式 |
|----------|----------|----------|
| Intelligent | AI 引擎心跳超时 / 状态异常 | Fallback |
| Intelligent | AiValidator 校验不通过 | Fallback |
| Fallback | AI 引擎恢复（status == Ready） | Intelligent |
| Any | 运维人员手动切换 | Basic / Intelligent / Fallback |
| Basic | 运维人员手动切换 | Intelligent / Fallback |

### 8.3 核间通信信号

策略模式通过 TCP 帧中的 `strategy_mode` 字段同步给实时控制模块：

| 值 | 模式 | 说明 |
|----|------|------|
| 0 | 基础模式 | Basic |
| 1 | 智能模式 | Intelligent |
| 2 | 兜底模式 | Fallback |

同时，`ai_ready` 字段（u8, 0/1）指示 AI 引擎可用状态。

---

## 9. 接口定义

### 9.1 FallbackStrategy Trait（strategies.rs）

```rust
#[async_trait]
pub trait FallbackStrategy: Send + Sync {
    /// 评估数据并生成控制命令
    async fn evaluate(&self, data: &DataPackage) -> Result<ControlCommand, MupcError>;

    /// 获取策略类型
    fn strategy_type(&self) -> StrategyType;

    /// 获取策略名称
    fn name(&self) -> &str;
}
```

所有策略实现此 trait 的三个方法：
- `evaluate()` — 根据遥测数据生成控制命令
- `strategy_type()` — 均返回 `StrategyType::Fallback`
- `name()` — 返回策略名称字符串

### 9.2 ControlCommand 结构体

```rust
#[derive(Debug, Clone)]
pub struct ControlCommand {
    pub cmd_id: u16,                          // 命令 ID（1-削峰填谷, 2-需量控制, 3-防逆流）
    pub cmd_type: CommandType,                // 命令类型
    pub p_ref: Option<f64>,                  // 有功基准点 (kW)，AI输出或本地策略设置
    pub k_droop: Option<f64>,                // 电压-有功下垂系数 (kW/V)，AI输出或本地策略设置
    pub q_batt_set: Option<f64>,             // 无功由实时控制模块闭环调节
    pub phase_compensation: Option<[f64; 3]>, // 分相补偿系数 [预留]
    pub start_stop: Option<bool>,            // 启停命令
    pub priority: u8,                        // 优先级（0-3）
    pub pv_limit: Option<f64>,               // PV 限功率比例 (0.0-1.0)，仅由本地防逆流策略设置
    pub load_shedding: Option<f64>,          // 负荷切除功率 (kW)，仅由本地需量控制策略设置
    // v2.16 新增：台区储能分相设定（仅由台区储能治理策略设置）
    pub phase_p_set: Option<[f64; 3]>,       // 台区储能分相有功设定 (kW) [A/B/C]，正=放电/注入
    pub phase_q_set: Option<[f64; 3]>,       // 台区储能分相无功设定 (kVAr) [A/B/C]
}
```

> **字段语义说明：** `pv_limit` 和 `load_shedding` 保留在 `ControlCommand` 中，但仅由本地兜底策略引擎（`AntiReverseStrategy` / `DemandControlStrategy`）设置，不再作为 AI 引擎（`ActionOutput`）的动作维度。
>
> **v2.16 新增：** `phase_p_set` / `phase_q_set` 为台区储能分相有功/无功设定，仅由台区储能治理策略（`TaiStorageStrategy`，见 §15）设置。设定值经核间 V3 帧下发到实时控制模块，由其转发至台区储能 PCS（三相四桥臂分相 PQ 独立可控）。

### 9.3 CommandType 枚举

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CommandType {
    SwitchControl,      // 开关控制（需量控制 Level 2/3）
    PowerRegulation,    // 功率调节（防逆流、削峰填谷平时段）
    ChargeDischarge,    // 充放电控制（削峰填谷峰谷时段）
}
```

### 9.4 AiCommandValidator Trait

```rust
#[async_trait]
pub trait AiCommandValidator: Send + Sync {
    /// 校验 AI 命令
    async fn validate(&self, cmd: &ControlCommand) -> ValidationResult;
    /// 获取校验器名称
    fn name(&self) -> &str;
}
```

### 9.5 ValidationResult 结构体

```rust
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,                              // 是否通过
    pub message: String,                          // 错误消息
    pub suggested_command: Option<ControlCommand>,  // 建议命令
}

impl ValidationResult {
    pub fn valid() -> Self;                       // 创建通过结果
    pub fn invalid(message: impl Into<String>) -> Self;  // 创建失败结果
}
```

### 9.6 AiCommand 结构体

```rust
#[derive(Debug, Clone)]
pub struct AiCommand {
    pub cmd_id: u16,           // 命令 ID
    pub p_set: f64,            // 有功设定值 (kW)
    pub q_set: f64,            // 无功设定值 (kVar)
    pub priority: u8,          // 优先级
    pub raw_command: String,   // 原始命令 JSON
}
```

---

## 10. 文件结构

```
mupc/crates/strategy-engine/
├── Cargo.toml
├── src/
│   ├── lib.rs                    # 模块导出，AI Engine re-export
│   │                             # Phase 1: 仅导出 strategies
│   │                             # Phase 3A: 完整导出 peak_shaving, demand_control, ...
│   │                             # Phase 3C: 增加 ai_integration 模块
│   │
│   ├── strategies.rs             # FallbackStrategy trait, ControlCommand,
│   │                             # CommandType, AiCommandValidator trait,
│   │                             # ValidationResult, StrategyType, AiCommand
│   │
│   ├── peak_shaving.rs           # 削峰填谷策略实现
│   ├── demand_control.rs         # 需量控制策略实现
│   ├── anti_reverse.rs           # 防逆流保护策略实现
│   ├── tai_storage.rs            # 台区储能治理策略实现（v2.16 第 4 策略）
│   │
│   ├── ai_validator.rs           # AiCommandValidatorImpl 可插拔校验器
│   │                             # AiModel trait, MockAiModel, ModelInput/Output
│   │
│   ├── ai_integration.rs         # AiIntegrator（Phase 3C: AI 引擎集成）
│   │
│   ├── config.rs                 # PeakShavingConfig, DemandControlConfig, AntiReverseConfig
│   ├── errors.rs                 # StrategyError 枚举
│   │
│   ├── peak_shaving_test.rs      # 削峰填谷策略单元测试（9 tests）
│   ├── demand_control_test.rs    # 需量控制策略单元测试（7 tests）
│   ├── anti_reverse_test.rs      # 防逆流策略单元测试（5 tests）
│   ├── ai_validator_test.rs      # AI 校验器单元测试（8 tests）
│   └── tai_storage_test.rs       # 台区储能治理策略单元测试（~15 tests，v2.16）
```

### lib.rs 模块导出

```rust
pub mod strategies;
pub mod peak_shaving;
pub mod demand_control;
pub mod anti_reverse;
pub mod tai_storage;          // v2.16 第 4 策略
pub mod ai_validator;
pub mod config;
pub mod errors;
pub mod ai_integration;       // Phase 3C

pub use peak_shaving::PeakShavingStrategy;
pub use demand_control::DemandControlStrategy;
pub use anti_reverse::AntiReverseStrategy;
pub use tai_storage::{TaiControllerState, TaiStorageConfig, TaiStorageStrategy, TaiState}; // v2.16
pub use ai_validator::{AiCommandValidatorImpl, AiModel, ModelInput, ModelOutput, MockAiModel};
pub use config::{PeakShavingConfig, DemandControlConfig, AntiReverseConfig};
pub use errors::StrategyError;
pub use strategies::{FallbackStrategy, AiCommandValidator, StrategyType, ControlCommand, CommandType, ValidationResult};
pub use mupc_ai_engine::{ModelManager, FusedSystemState, ActionOutput, ModelStatus, RobustnessManager, AnomalyType};
```

---

## 11. 错误处理

### 11.1 StrategyError 枚举

```rust
#[derive(Error, Debug)]
pub enum StrategyError {
    #[error("策略执行失败: {0}")]
    ExecutionFailed(String),

    #[error("AI 模型错误: {0}")]
    ModelError(String),

    #[error("配置错误: {0}")]
    ConfigError(String),
}
```

### 11.2 错误使用场景

| 错误类型 | 触发场景 | 处理方式 |
|----------|----------|----------|
| `ExecutionFailed` | 策略 evaluate() 内部计算异常 | 返回默认安全指令（p_batt=0） |
| `ModelError` | AI 模型加载失败、预测异常 | 自动降级至兜底模式 |
| `ConfigError` | 配置参数无效（如空时段列表） | 使用默认配置替代 |

---

## 12. 配置管理

### 12.1 削峰填谷配置（PeakShavingConfig）

```rust
#[derive(Debug, Clone)]
pub struct PeakShavingConfig {
    pub peak_hours: Vec<(u8, u8)>,       // 默认: [(8, 11), (18, 21)]
    pub valley_hours: Vec<(u8, u8)>,     // 默认: [(23, 7)]
    pub soc_charge_max: f64,             // 默认: 80.0
    pub soc_charge_min: f64,             // 默认: 20.0
    pub battery_capacity: f64,           // 默认: 100.0
}
```

### 12.2 需量控制配置（DemandControlConfig）

```rust
#[derive(Debug, Clone)]
pub struct DemandControlConfig {
    pub transformer_capacity: f64,    // 默认: 200.0
    pub demand_factor: f64,           // 默认: 0.85
    pub warning_threshold: f64,       // 默认: 0.80
    pub action_threshold: f64,        // 默认: 0.90
    pub emergency_threshold: f64,     // 默认: 0.95
}
```

### 12.3 防逆流配置（AntiReverseConfig）

```rust
#[derive(Debug, Clone)]
pub struct AntiReverseConfig {
    pub reverse_power_threshold: f64,   // 默认: -0.1
    pub pv_limit_step: f64,             // 默认: 0.10
    pub max_charge_power: f64,          // 默认: 50.0
    pub soc_charge_max: f64,            // 默认: 80.0
}
```

### 12.4 运行时配置热加载（Phase 2+）

- 当前实现：所有配置通过 `Default` trait 提供默认值，构造时传入
- Phase 2+ 规划：支持配置文件热加载（修改无需重启）、运行时动态调整

---

## 13. 测试体系

### 13.1 测试覆盖统计

| 测试文件 | 测试用例数 | 测试内容 |
|----------|-----------|----------|
| `peak_shaving_test.rs` | 9 | 峰谷时段检测、SOC 边界、PV 充电优先级 |
| `demand_control_test.rs` | 7 | 四级负载率、低 SOC 保护、自定义阈值 |
| `anti_reverse_test.rs` | 5 | 逆流充电、PV 限功率、正常无动作、类型/名称 |
| `ai_validator_test.rs` | 8 | 模型预测（3）、无模型/有模型校验、开关命令、异步接口 |

**总计：29 个单元测试**

### 13.2 测试数据构造模式

所有策略测试共用 `mupc_data_processing::telemetry::DataPackage` 作为输入，通过辅助函数构造：

```rust
fn create_test_data(timestamp: u64, battery_soc: f64, pv_power: f64, load_power: f64) -> DataPackage {
    DataPackage {
        electrical: ElectricalData { ... },
        battery: BatteryData { soc: Some(battery_soc), ... },
        device_status: DeviceStatus { pv_power: Some(pv_power), load_power: Some(load_power), ... },
        timestamp,
    }
}
```

### 13.3 验证要求

每次代码变更必须通过：

- [ ] `cargo build --release` 编译成功
- [ ] `cargo clippy` 无警告
- [ ] `cargo test -p mupc-strategy-engine` 全部通过
- [ ] `cargo fmt` 格式化通过

---

## 14. 演进路线

| Phase | 内容 | 说明 | 状态 |
|-------|------|------|------|
| Phase 1 | 接口定义：`FallbackStrategy` trait 和 `AiCommandValidator` trait | 仅接口预留 | 已完成 |
| Phase 3A | 完整实现三种兜底策略 + `AiCommandValidatorImpl` + `MockAiModel` | 削峰填谷、需量控制、防逆流实现 | 已完成 |
| Phase 3C | AI 引擎集成：`AiIntegrator` 集成 LSTM/TCN + MADDPG/PPO | 替换 MockAiModel，真实 AI 决策 + 校验 | 已完成 |
| Phase 2+ | 电压越限无功补偿 | 完整策略实现 | 规划中 |
| Phase 2+ | 三相不平衡补偿 | 分相无功补偿 | 规划中 |
| Phase 2+ | 运行时配置热加载 | 配置修改无需重启 | 规划中 |
| Phase 2+ | 消息总线扩展（AMQP/MQTT） | 支持更多消费者 | 规划中 |
| — | Q 控制 | 无功由实时控制模块闭环调节，ControlCommand 中 q_batt_set 已废弃 | 已关闭 |

---

## 15. 台区储能治理策略（第 4 策略）

> **v2.16 新增（2026-08-31）**。整合自 `2026-08-25-台区储能控制策略-design.md`（方案A：分时状态机 + 共模/差模分解），作为策略引擎**第 4 种兜底策略**，在 AI 引擎不生效时实现台区储能的台区治理目标。

### 15.1 定位与目标

台区配光伏 + 储能 + 三相四桥臂 PCS，当 AI 引擎失效（兜底模式）时，由本策略接管储能控制，实现三个治理目标（按优先级）：

1. **降低光伏返送**：缩短返送时长、压缩返送幅值（软目标，偶尔返送可接受）；
2. **降低三相电流不平衡度**：目标 <20%（电网公司口径 `(1 − MIN(Ia,Ib,Ic)/MAX(Ia,Ib,Ic)) × 100%`，幅值式）；
3. **提高功率因数**：大部分时间接近 1。

**目标优先级（2026-08-26 评审）**：日终 SOC 清空（S4 硬约束）> 不平衡度 <20%（物理极限内尽力）> 降低返送（软目标）> PF（软目标）——受电池容量限制，"零返送"与"晚峰全削峰"不可同时完美达成，冲突时按此顺序妥协。

### 15.2 硬件与约束

| 项 | 取值 |
|---|---|
| PCS | 125kW，三相四桥臂，分相 PQ 独立可控 |
| 电池 | 60kW / 120kWh，SOC 运行带 10%~90%，日终回到 10% |
| 测量 | 台区总表（20s 延时），无本地实时测点；PCS/EMS 均无交采模块 |
| 控制 | 分钟级（T=60s）下发分相 P/Q 设定值 |
| 预测 | 纯实时，无光伏/负荷预测 |
| PCS 容量边界 | 每相/中线额定电流 190A，过载 1.1×长期（209A）/1.2×1min（228A）；总视在 125kVA |
| 分时 SOC 上限 | 18:00 前 SOC ≤70%（可标定），之后释放至 90% |

### 15.3 架构（融入策略引擎）

```
台区总表(分相 P/Q/PF/U/I，20s) → DataPackage.ElectricalData.phase(扩展)
        ↓ (南向采集循环写入 set_latest_data)
AiIntegrator.run_fallback_strategies()          ← AI 失效时调用
        ↓
TaiStorageStrategy (第 4 策略，持 Arc<Mutex<TaiControllerState>>)
        · 4 状态机 S1/S2/S3/S4 + 积分器(共模P/差模P/分相Q)
        · 每 60s 一个控制周期 → ControlCommand(phase_p_set/phase_q_set)
        ↓
IntercoreClient.send_tai_command()              ← 新增核间 V3 帧(分相 P/Q)
        ↓
实时控制模块 → 台区储能 PCS(分相 P/Q 设定)
```

**与现有三策略的关系**：削峰填谷/需量控制/防逆流继续负责各自的南向动作（`pv_limit`/`load_shedding`）；台区储能治理策略负责台区储能分相 P/Q 下发。二者并行运行、互不阻塞（`dispatch_ai_decision` 失败降级时同时执行）。

### 15.4 状态机（4 状态）

| 状态 | 时段/触发 | 主目标 | 共模 P 方向 |
|---|---|---|---|
| S1 光伏吸收 | 白天，`P_表 < −P_abs_trig`（−10kW）且 SOC < 分时上限−滞回 | 吸收返送 | 充电 |
| S2 平段 | 其他时间 | 三相平衡 + PF | 0 |
| S3 高峰放电 | 任一时刻 `P_表 > P_dis_trig`（+30kW） | 放电供负荷 + 平衡 | 放电 |
| S4 日终清空 | 临近日终且 SOC>10% | 强制放电到 10% | 强制放电（允许晚反送） |

**切换规则**：
- 触发带滞回（进入阈值 ≠ 退出阈值）；S1 退出在目标另一侧（进口 ≥+4kW 或 SOC 顶格），防"达目标即退→返送复现→重进"抖振；
- S3 退出 = 共模积分回零且进口 ≤目标 +5kW（削峰完成）；退出不可设在进口数值上（积分把进口恒压到 +5，固定阈值不可达）；
- S3 全天负荷触发（无时段门控）；优先级 S4 > S1 > S3 > S2；
- **failsafe**：总表数据超时（>150s）或坏数 → 冻结积分并斜坡回归 0，保持最后有效 Q，恢复后从 0 重新积分。

### 15.5 控制律（三通道）

```
Q_i = clamp(Q_i[k−1] + s·K_q × Q_meter_i, −Q_i_max, +Q_i_max)   # 分相 Q（PF，积分式，常开）
P_st = clamp(P_st[k−1] + Kp×(P_表[k]−P_目标进口), 状态限幅, 斜坡限速)  # 共模 P（能量，增量式）
ΔP_i = clamp(ΔP_i[k−1] + K_diff×U_i×(I_i[k]−I_均值), −ΔP_max, +ΔP_max)  # 差模 P（三相平衡，积分式）
P_i = P_st/3 + ΔP_i    # 每相合成
Q_i = Q_i_补偿
```

**要点**：
- **必须积分式**：表计无功/电流包含 PCS 自身注入（作动器在测量回路内），比例式闭环特征根 +1，稳态只收敛一半；积分式稳态收敛到目标（`Q_表=0`、各相电流=均值）；
- **差模 P 零净能量**：三相增量 Σ=0（Σ(I_i−均值)=0），ΣΔP 恒为 0，不耗电池；电压不平衡时轻微残差（数据核实中位 <1%，可忽略）；
- **I_i 必须为带符号电流**（由分相有功 Pa 的符号或相角导出），不能用幅值——单相返送时幅值法方向相反、反向加重不平衡；
- **Q 通道与 ΔP 通道协作**：Q 先把各相 PF 校正到 1（使 S→P），ΔP 再平衡各相净有功/电流，不重复作用；
- **单相/两相返送（光伏随机接相）**：某相返送而总表仍净受电 → 不触发 S1，改由差模 P 拉向均值（零净能量、不耗电池）；总表净返送超阈值才叠加 S1 充电；
- **s 符号**（Q 积分方向）：s=±1 以表计/PCS 约定为准，发散则翻转；投运前用小幅 Q 阶跃 + 分相注流核相（强制）。

### 15.6 容量仲裁（每相、每周期）

- 约束：每相/中线电流 ≤190A、总视在 ≤125kVA、总有功 ≤60kW（电池）；
- 裁剪顺序（按优先级）：先减 **Q**（PF，软目标）→ 再减 **差模 P**（不平衡）→ 最后减 **共模 P**（返送/能量，S4 不可剪）；仅 SOC 保护可剪共模 P；
- ΔP 裁剪后重归一化 ΣΔP=0（等比缩差模后均匀回补残差）；
- SOC 保护：充电 ≥90% 共模 P 剪 0、放电 ≤10% 共模 P=0；88%/12% 线性降额；
- 斜坡限速：P_cm 与 ΔP_i 每周期变化 ≤5kW。

### 15.7 数据接入（DataPackage 扩展）

`ElectricalData` 新增分相字段（`mupc-data-processing/src/telemetry.rs`）：

```rust
/// 分相电气数据（台区总表，v2.16 新增）
#[derive(Debug, Clone, Default)]
pub struct PhaseElectricalData {
    pub voltage: [Option<f64>; 3],        // Ua/Ub/Uc (V)
    pub current: [Option<f64>; 3],        // Ia/Ib/Ic (A，带符号)
    pub active_power: [Option<f64>; 3],   // Pa/Pb/Pc (kW，含符号：>0 受电 / <0 返送)
    pub reactive_power: [Option<f64>; 3], // Qa/Qb/Qc (kVAr，含符号)
    pub cos_phi: [Option<f64>; 3],        // PFa/PFb/PFc
}

pub struct ElectricalData {
    // ... 现有三相总字段
    /// 分相数据（台区总表），None = 不可用（v2.16 新增）
    pub phase: Option<PhaseElectricalData>,
}
```

- 南向采集循环（或台区总表数据源）填充 `phase` 字段；
- 分相数据缺失时：策略按 failsafe 处理（积分冻结、斜坡回归 0）；
- `DataPackage` 构造处（`dataframe_to_datapackage` 等）同步更新，未填分相字段时 `phase=None`，不破坏现有调用方。

### 15.8 执行路径（核间协议 V3）

核间协议新增分相下发通道（`mupc-intercore`）：

```rust
// tcp_server.rs
/// 控制指令 JSON Payload v3.0（分相模式，v2.16 新增）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlCmdPayloadV3 {
    #[serde(rename = "frame_version")]
    pub frame_version: Option<u8>,          // = 3
    #[serde(rename = "p_ref")]
    pub p_ref: Option<f64>,                 // 兼容 v2 双参数（分相模式可为 None）
    #[serde(rename = "k_droop")]
    pub k_droop: Option<f64>,
    #[serde(rename = "phase_p_set")]
    pub phase_p_set: Option<[f64; 3]>,      // 分相有功 (kW)
    #[serde(rename = "phase_q_set")]
    pub phase_q_set: Option<[f64; 3]>,      // 分相无功 (kVAr)
    #[serde(rename = "ai_ready")]
    pub ai_ready: Option<bool>,
    #[serde(rename = "strategy_mode")]
    pub strategy_mode: Option<String>,
    #[serde(rename = "timestamp_ms")]
    pub timestamp_ms: Option<u64>,
}
```

- **复用 `IntercoreFrameType::ControlCmd` 帧类型**（帧定长 64 字节，JSON 数据区可承载分相六维），仅新增 payload 版本（`detect_version` 按 `frame_version` 区分 v1/v2/v3）；向后兼容，不新增帧类型；
- `IntercoreClient` 新增 `send_tai_command(&self, p: [f64;3], q: [f64;3], strategy_mode: &str)`，封装 V3 帧发送，复用现有持久连接。

### 15.9 带状态控制器设计

现有 `FallbackStrategy` 为无状态纯函数（`evaluate(&self, &DataPackage)`）。台区治理策略需跨周期积分状态（`st/P_st/Q_pcs/dP/...`），采用**策略实例内部持状态**方式，不改动 trait 签名：

```rust
// strategy-engine/src/tai_storage.rs
/// 台区储能控制器状态（4 状态机）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaiState {
    S1PvAbsorb,   // 光伏吸收
    S2Flat,       // 平段
    S3Peak,       // 高峰放电
    S4Clear,      // 日终清空
}

/// 台区总表单周期测量（控制律输入，含符号约定）
#[derive(Debug, Clone)]
pub struct MeterData {
    pub p: f64,               // 三相总有功 (kW，>0 受电 / <0 返送)
    pub q: f64,               // 三相总无功 (kVAr)
    pub pf: [f64; 3],         // 分相功率因数
    pub u: [f64; 3],          // 分相电压 (V)
    pub i: [f64; 3],          // 分相电流 (A，带符号)
    pub p_i: [f64; 3],        // 分相有功 (kW，含符号)
    pub q_i: [f64; 3],        // 分相无功 (kVAr，含符号)
}

/// 台区储能控制器跨周期状态（由策略实例持 Arc<Mutex> 保存）
pub struct TaiControllerState {
    pub st: TaiState,                 // S1/S2/S3/S4
    pub p_st: f64,                    // 共模出力 (kW)
    pub q_pcs: [f64; 3],              // 无功积分状态
    pub d_p: [f64; 3],                // 差模积分状态
    pub q_active: [bool; 3],          // 死区滞回锁存
    pub d_p_active: bool,             // 差模死区锁存
    pub q_last: [f64; 3],             // 最近有效 Q（failsafe 用）
    pub meter_buf: VecDeque<MeterData>, // 滑动滤波窗口（3~5 点）
}

pub struct TaiStorageStrategy {
    config: TaiStorageConfig,
    state: Arc<Mutex<TaiControllerState>>,   // 跨周期状态（纯函数 control() 的显式状态存储）
}
```

- `evaluate(&self, data)` 内：取锁 → `control()` 纯函数计算（跨周期状态读-算-写）→ 组装 `ControlCommand{ phase_p_set, phase_q_set }` → 释放锁；
- **控制周期节流**：`evaluate` 每周期被 `run_fallback_strategies` 调用；内部按 `timestamp` 判断距上次控制 ≥60s 才执行 `control()`，未到期则返回上次指令（避免 1s 决策循环与 60s 控制周期不匹配）；
- 首次周期初值：`st=S2, p_st=0, q_pcs=d_p=0, q_active=d_p_active=false, q_last=0, meter_buf=空`。

### 15.10 配置（TaiStorageConfig）

| 参数 | 初始值 | 作用 |
|---|---|---|
| `control_period_s` | 60 | 控制周期 |
| `p_abs_trig` / `p_dis_trig` | 10 / 30 (kW) | S1 返送 / S3 高峰触发阈值 |
| `s1_exit` / `p_tgt_s1` / `p_tgt_s3` | 4 / 2 / 5 (kW) | S1 退出阈值 / 目标进口 |
| `p_cap` / `slope` | 60 / 5 | 电池功率上限 / 斜坡限速 (kW/周期) |
| `kp` / `k_diff` / `k_q` | 0.4 / 0.4 / 0.4 | 共模/差模/无功积分增益 |
| `dp_max` / `q_i_max` | 40 / 30 | 差模上限 (kW/相) / 无功上限 (kVAr/相) |
| `i_rated` / `s_rated` | 190 / 125 | 每相·中线电流限 (A) / 总视在限 (kVA) |
| `soc_cap_day` / `soc_hys` | 0.70 / 0.03 | 分时 SOC 上限 / 滞回 |
| `t_release` / `t_clear_start` / `t_clear_end` | 18:00 / 21:00 / 23:30 | 分时上限释放 / S4 清空时段 |
| `stale_t` | 150 (s) | failsafe 数据超时 |
| `battery_capacity_kwh` | 120 | 电池容量 |

初始值为占位，最终值在离线回放（§15.12）中标定（分时 SOC 上限扫 60/70/80%、P_abs_trig 扫 5/10/15/20、Kp 灵敏度）。

### 15.11 集成点（AiIntegrator）

- `AiIntegrator` 新增字段 `tai_storage: Arc<Mutex<TaiStorageStrategy>>`；
- `set_tai_storage_strategy()` 注入（startup 装配时创建并注入）；
- `run_fallback_strategies()` 中追加：调用 `tai_storage.evaluate(&data)`，产出分相指令 → 经 `intercore_client.send_tai_command()` 下发（若未注入核间客户端则跳过并记录警告）；
- 与现有防逆流/需量控制并行执行（各自独立输出，互不阻塞）。

### 15.12 离线回放验证

**工具形态**：独立二进制 `mupc-tai-replay`（workspace 下新增 bin crate 或 `tests/` 集成测试），读取历史 data_rule 数据逐周期回放，输出 KPI 报告。

**数据源**：`E:/MUPC2/数据/2026_06_27_data_rule.xlsx`（低负荷日）、`2026_07_04_data_rule.xlsx`（高负荷日）。xlsx 解析用 `calamine` crate（新增 dev-dependency），列含 A/B/C 相有功/无功（含符号）、功率因数、电压、电流、调控值。

**回放流程**：
1. 读 xlsx → 按 60s 步进对齐时间戳，缺段跳过；
2. 逐周期调用 `TaiStorageStrategy` 的纯函数 `control()`（跨周期状态由回放循环保存回传，与运行时 Mutex 等价）；
3. 累计 SOC（±120kWh）；初值 SOC=0.50；
4. 统计 KPI：返送时长/幅值（vs 无储能基线）、不平衡度 <20% 达标时长占比（目标 ≥80%）、PF 接近 1 占比、SOC 日终回到 10% 且始终在 10~90% 带内、晚反送量。

**边界用例**：中午大返送+SOC 快满、S1 到分时上限后返送仍持续（无 S1↔S2 振荡）、晚峰负荷小（S4 晚反送）、返送+不平衡同时（仲裁顺序）、单相返送、21:00 S3→S4 交接、通信故障注入、数据缺口、SOC 误差 ±3%。

**回放报告**：打印每日 KPI 表 + 参数灵敏度（扫分时 SOC 上限、P_abs_trig、Kp），供标定初始值。

### 15.13 测试体系

| 测试文件 | 用例数 | 测试内容 |
|----------|--------|----------|
| `tai_storage_test.rs` | ~15 | 状态机切换（S1~S4 进入/退出/滞回）、积分收敛（共模/差模/Q）、零净能量 ΣΔP=0、容量仲裁裁剪顺序、failsafe 数据超时、控制周期节流 |
| `mupc-tai-replay` | 集成 | 6-27/7-04 回放 KPI 断言（不平衡 <20% 达标时长 ≥80% 等） |
| 核间 V3 帧 | ~4 | `ControlCmdPayloadV3` 序列化/反序列化、版本检测（v1/v2/v3）、`send_tai_command` 帧组装 |

### 15.14 依赖清单（实现前确认）

1. 台区总表实时接口提供分相 Q（含符号）与分相 PF（data_rule 字段已确认）；
2. PCS 通信接受分相 P/Q 设定值（已确认）；实时控制模块能转发分相 P/Q 到 PCS（**需与实时控制模块协议确认 V3 帧对接**）；
3. PCS 每相/中线电流限值、总视在额定（已确认：190A/125kVA）；
4. 状态机时段参数初值（已用 6-27/7-04 data_rule 负荷曲线标定，P_dis_trig=30kW、T_清空 21:00/23:30）；
5. 电池充/放电功率限值 60kW（已确认）；
6. 通信协议细节：设定值下发瞬时生效或斜坡生效、超时/失败响应、时钟同步；现场核相流程（强制）。

---

## 附录 A：Cargo.toml 依赖

```toml
[package]
name = "mupc-strategy-engine"
edition.workspace = true

[dependencies]
tokio.workspace = true
tracing.workspace = true
thiserror.workspace = true
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
async-trait = "0.1"
mupc-common = { path = "../common" }
mupc-data-processing = { path = "../data-processing" }
mupc-ai-engine = { path = "../ai-engine" }

[dev-dependencies]
tokio-test = "0.4"
```

## 附录 B：术语表

| 术语 | 说明 |
|------|------|
| AiIntegrator | AI 集成器，管理 AI 模型生命周期 |
| AiCommandValidator | AI 指令校验器，安全闸门 |
| FallbackStrategy | 兜底策略 trait，所有策略实现此接口 |
| SOC | 电池荷电状态（%） |
| PV | 光伏（Photovoltaic） |
| 削峰填谷 | 基于电价时段的充放电策略 |
| 需量控制 | 基于变压器负载率的阶梯控制 |
| 防逆流 | 防止向电网逆送电的保护策略 |
| DataPackage | 遥测数据包结构体（定义于 mupc-data-processing） |

---

## 附录：版本演进

> 正文已整合全部历史补丁，本表仅作演进追溯。

| 版本 | 主要变更 |
|------|----------|
| v1.0 | 初版：定义 `FallbackStrategy`/`AiCommandValidator` 接口与三种兜底策略 |
| v1.1 | 农网台区参数更新（变压器容量 500kVA→200kVA） |
| v2.15 | 动作空间精简：AI 2 维动作（p_ref + k_droop），load_shedding/pv_limit 下沉至本地策略 |
| v2.16 | 新增第 4 策略「台区储能治理」：扩展 DataPackage 分相字段、ControlCommand 分相设定、核间 V3 帧、带状态控制器、离线回放验证 |
