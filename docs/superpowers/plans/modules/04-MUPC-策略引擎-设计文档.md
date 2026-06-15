# MUPC 策略引擎模块设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.1 | 2026-06-10 | 架构师 | 当前版本 |
| v1.0 | 2026-05-29 | 架构师 | 初版 |

> **文档定位：** 本文档记录实现级设计决策（架构、Rust 结构体/trait、状态机、配置结构、测试策略、文件组织）。需求级内容（功能描述、验收标准、性能指标）请参考 [04-MUPC-策略引擎-PRD](../specs/modules/04-MUPC-策略引擎-PRD.md)。

**合并来源：** PRD v1.1 + Phase3A 实施计划 + 代码库 `mupc/crates/strategy-engine/src/`

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
│  p_ref + k_droop → IntercoreClient → 实时控制模块          │
│  pv_limit / load_shedding → SouthCommandDispatcher → 南向设备 │
└──────────────────────────────────────────────────────────────┘
```

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

### 2.3 决策逻辑

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

### 2.4 时段检测规则

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

### 2.5 配置参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `peak_hours` | `Vec<(u8, u8)>` | `[(8, 11), (18, 21)]` | 峰时段列表，(起始小时, 结束小时) |
| `valley_hours` | `Vec<(u8, u8)>` | `[(23, 7)]` | 谷时段列表，(起始小时, 结束小时) |
| `soc_charge_max` | `f64` | `80.0` | SOC 充电上限（%） |
| `soc_charge_min` | `f64` | `20.0` | SOC 充电下限（%） |
| `battery_capacity` | `f64` | `100.0` | 电池容量（kWh） |

### 2.6 输出字段

| 字段 | 值 | 说明 |
|------|-----|------|
| `cmd_id` | 1 | 削峰填谷策略固定 ID |
| `cmd_type` | `ChargeDischarge` / `PowerRegulation` | 充放电控制或待机 |
| `p_ref` | ±15~30 kW | 有功基准点（v2.7+ 双参数模式） |
| `priority` | 1 | 默认优先级 |

### 2.7 测试覆盖

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
| `q_batt_set` | `Option<f64>` | [LEGACY v2.4~v2.6] 无功由实时控制模块闭环调节 | - |
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

    // 3. 无 p_ref 时默认通过（v2.7+ 双参数模式）
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
                                  ├── 决策接口 → ActionOutput
                                  └── 状态管理 → ModelStatus

数据流：
1. LSTM/TCN 时序预测（光伏出力/负荷）
2. MADDPG/PPO 基于预测结果决策
3. AiCommandValidator 校验 AI 指令安全性
4. 指令分发：
   - p_ref + k_droop → IntercoreClient → 实时控制模块（闭环下垂控制）
   - pv_limit → SouthCommandDispatcher → 光伏逆变器
   - load_shedding → SouthCommandDispatcher → 负荷控制装置
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
    pub p_ref: Option<f64>,                  // 有功基准点 (kW)，v2.7+
    pub k_droop: Option<f64>,                // 电压-有功下垂系数 (kW/V)，v2.7+
    pub q_batt_set: Option<f64>,             // [LEGACY] 无功由实时控制模块闭环调节
    pub phase_compensation: Option<[f64; 3]>, // 分相补偿系数 [预留]
    pub start_stop: Option<bool>,            // 启停命令
    pub priority: u8,                        // 优先级（0-3）
    pub pv_limit: Option<f64>,               // PV 限功率比例 (0.0-1.0)
    pub load_shedding: Option<f64>,          // 负荷切除功率 (kW)
}
```

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
│   └── ai_validator_test.rs      # AI 校验器单元测试（8 tests）
```

### lib.rs 模块导出

```rust
pub mod strategies;
pub mod peak_shaving;
pub mod demand_control;
pub mod anti_reverse;
pub mod ai_validator;
pub mod config;
pub mod errors;
pub mod ai_integration;       // Phase 3C

pub use peak_shaving::PeakShavingStrategy;
pub use demand_control::DemandControlStrategy;
pub use anti_reverse::AntiReverseStrategy;
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
| — | Q 控制 | 无功由实时控制模块闭环调节（v2.4+），ControlCommand 中 q_batt_set 为 LEGACY | 已关闭 |

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

**文档状态：** v1.1 当前版本
**合并来源：** 通信管理模块技术设计 v1.1 + Phase3A 实施计划 + 策略引擎 PRD v1.1
**对齐代码版本：** Strategy Engine Phase 3C（包括 ai_integration.rs）
**产出时间：** 2026-05-29

## v1.1 修订记录 (2026-06-10)

| 序号 | 修订项 | 修订位置 | 说明 |
|------|--------|----------|------|
| 1 | 农网台区参数更新 | DemandControlConfig | 变压器容量 500kVA→200kVA |
| 2 | 版本号更新 | 文档头部 | v1.0 → v1.1 |

**修订依据：** 农网台区新规格落地：变压器 200kVA、光伏 150kW、储能 50kW/100kWh、居民负荷 60kW、农业冲击负荷最高 120kW。代码默认值已同步更新。

---

## 15. Phase 3A 实现笔记

> 以下内容提取自 Phase 3A 实施计划（`2026-05-27-MUPC-Phase3A-实施计划.md`），为前述章节未覆盖的实现级细节。

### 15.1 高频遥测 Ring Buffer

Phase 3A 在 `HighFrequencyTelemetryImpl` 中使用 `VecDeque` 实现环形缓冲区，容量 60 条记录（对应 1Hz 上报下 60 秒窗口），通过 `Arc<Mutex<VecDeque<TelemetryPoint>>>` 共享：

```rust
fn push_to_buffer(&self, point: TelemetryPoint) {
    let mut buffer = self.buffer.lock().unwrap();
    if buffer.len() >= 60 {
        buffer.pop_front(); // Ring Buffer: 移除最旧的
    }
    buffer.push_back(point);
}
```

### 15.2 TelemetryPoint 结构体

`HighFrequencyTelemetryImpl` 内部使用 7 字段遥测点，通过 `get_current_value(&self, point_name: &str) -> Option<f64>` 按名称查询当前值：

| 字段 | 类型 | 说明 |
|------|------|------|
| `battery_soc` | `f64` | 电池 SOC |
| `battery_power` | `f64` | 电池功率 (kW) |
| `pv_output` | `f64` | 光伏出力 (kW) |
| `load_power` | `f64` | 负荷功率 (kW) |
| `grid_power` | `f64` | 电网功率 (kW) |
| `transformer_load` | `f64` | 变压器负载率 |

### 15.3 Timestamp 到小时的转换

削峰填谷策略中，从 Unix 时间戳提取小时（u64 截断到当日秒）：

```rust
let hour = (data.timestamp % 86400) / 3600;
```

### 15.4 防逆流策略的可变状态

`AntiReverseStrategy::evaluate_sync` 需要 `&mut self`，因其内部维护 `pv_limit_count: u8`，每次逆流且电池满时递增（`pv_limit_count += 1`），电网恢复正常时清零。渐进式 PV 限功率公式：

```rust
pv_limit = pv_power * (self.pv_limit_count as f64 * 0.1).min(0.5);
```

每次限幅 10%，上限 50%。

### 15.5 故障类型枚举 (FaultType)

data-processing 模块定义的故障类型（与策略引擎决策相关）：

| 枚举值 | SQL 标签 | 说明 |
|--------|----------|------|
| `BatteryOverTemp` | `BATTERY_OVER_TEMP` | 电池过温 |
| `BatteryUnderSoc` | `BATTERY_UNDER_SOC` | 电池 SOC 过低 |
| `GridOverload` | `GRID_OVERLOAD` | 电网过载（电压 > 420V） |
| `GridReverse` | `GRID_REVERSE` | 电网逆流 |
| `PvOutputLimit` | `PV_OUTPUT_LIMIT` | 光伏限功率 |
| `Unknown` | `UNKNOWN` | 未知故障 |

### 15.6 故障录波 SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS fault_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    fault_type TEXT NOT NULL,
    trigger_time INTEGER NOT NULL,
    over_voltage REAL,
    under_voltage REAL,
    over_current REAL,
    frequency_abnormal REAL,
    waveform_path TEXT
);

CREATE INDEX IF NOT EXISTS idx_trigger_time ON fault_records(trigger_time);
```

故障记录保留 30 天，支持按时间范围查询（`query_sync(start, end)`）。

### 15.7 TDD 实施方法论

Phase 3A 的 10 个任务均采用统一流程：

1. 写失败测试（验证模块/函数不存在 → 编译失败）
2. 运行测试确认失败
3. 编写实现代码
4. 运行测试确认通过
5. 提交（每任务独立 commit，14 条 commit message 带 `Co-Authored-By`）

覆盖范围：data-processing（4 任务：错误类型、DataCollector、HighFrequencyTelemetry、FaultRecorder/SQLite）+ strategy-engine（6 任务：错误类型、削峰填谷、需量控制、防逆流、AiValidator、模块导出），共 13 个文件、10 个单元测试。
