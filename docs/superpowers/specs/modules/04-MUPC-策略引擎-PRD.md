# MUPC 策略引擎模块产品需求文档（PRD）

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-29 | 需求分析师 | 待评审 |

> 本文档为策略引擎模块的权威需求文档。历史来源文档已在 v1.0 文档体系重构中合并，不再单独维护。

---

## 1. 产品概述

### 1.1 定位与职责

策略引擎（Strategy Engine）是 MUPC 通信管理模块的**本地决策核心**，负责在 AI 引擎正常工作时担任"安全校验闸门"，在 AI 引擎失效时无缝接管控制，保障台区基本安全与运行。

**核心职责：**
- 提供三种兜底策略：削峰填谷、需量控制、防逆流保护
- 对 AI 引擎输出的指令进行安全校验（AiValidator）
- 管理策略模式切换（AI 模式 / 本地兜底模式 / 基础模式）
- 通过消息总线接收遥测数据，输出控制指令

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

### 1.3 目标平台

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux (openEuler) |
| 硬件 | RK3588 |
| 编程语言 | Rust 1.75+ |
| 异步运行时 | Tokio 1.x |
| 消息总线 | tokio::sync::mpsc |

### 1.4 开发范围

| 模块 | 实现内容 | 优先级 |
|------|----------|--------|
| 削峰填谷策略 | 完整实现（固定时间表） | 高 |
| 需量控制策略 | 完整实现（三级需量控制） | 高 |
| 防逆流保护策略 | 完整实现（电池充电 + PV 限功率） | 高 |
| AI 指令校验 | AiCommandValidator 可插拔接口 | 高 |
| 策略模式切换 | 与 AI 集成器联动的模式管理 | 中 |
| 电压越限/无功补偿 | 接口预留，实现延后 | 低 |

---

## 2. 削峰填谷策略

### 2.1 概述

基于**固定电价时段表**的经济性策略，在谷时段充电、峰时段放电，实现峰谷套利和变压器削峰。

### 2.2 决策逻辑

```
充电优先级：
  1. 谷时 + PV 低 → 电网充电（15kW）
  2. 谷时 + PV 高 → PV 充电（min(PV, 30kW)）
  3. 平时 + PV 高 → PV 充电（待扩展）

放电优先级：
  1. 峰时 → 放电（25kW）
  2. 平时 + PV 低 + SOC 高 → 放电（待扩展）
  3. 电网峰值 → 放电（待扩展）

边界保护：
  - SOC < soc_charge_min → 强制充电（20kW）
  - SOC > soc_charge_max → 强制放电（-20kW）
  - 非峰非谷的平时段 → 待机（0kW，PowerRegulation 模式）
```

### 2.3 配置参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| peak_hours | `Vec<(u8, u8)>` | `[(8, 11), (18, 21)]` | 峰时段列表，(起始小时, 结束小时) |
| valley_hours | `Vec<(u8, u8)>` | `[(23, 7)]` | 谷时段列表，(起始小时, 结束小时) |
| soc_charge_max | `f64` | `80.0` | SOC 充电上限（%） |
| soc_charge_min | `f64` | `20.0` | SOC 充电下限（%） |
| battery_capacity | `f64` | `100.0` | 电池容量（kWh） |

### 2.4 时段检测规则

- 峰时段检测：遍历 `peak_hours`，对每个 `(start, end)`：
  - 若 `start <= end`：`hour >= start && hour < end`
  - 若 `start > end`（跨天）：`hour >= start || hour < end`
- 谷时段检测：同上规则

### 2.5 输出格式

| 字段 | 值 | 说明 |
|------|-----|------|
| cmd_id | 1 | 削峰填谷策略固定 ID |
| cmd_type | ChargeDischarge / PowerRegulation | 充放电控制 |
| p_batt_set | ±15~30 kW | 电池有功设定 |
| priority | 1 | 默认优先级 |

### 2.6 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| PS-01 | 峰时段（如 10:00）且 SOC > 80% 时，应放电（p_batt = -20kW） | 单元测试 |
| PS-02 | 谷时段（如 02:00）且 SOC < 20% 时，应充电（p_batt = 20kW） | 单元测试 |
| PS-03 | 谷时段 + PV 充足时，按 PV 功率充电（min(PV, 30kW)） | 单元测试 |
| PS-04 | 峰时段、SOC 正常时，应放电（p_batt = -25kW） | 单元测试 |
| PS-05 | 非峰非谷的平时段，应待机（p_batt = 0，PowerRegulation） | 单元测试 |
| PS-06 | SOC 低于下限（20%）时，强制充电 | 单元测试 |
| PS-07 | SOC 高于上限（80%）时，强制放电 | 单元测试 |
| PS-08 | 跨天谷时段（23:00-07:00）检测正确 | 单元测试 |

---

## 3. 需量控制策略

### 3.1 概述

基于**变压器负载率**的阶梯式控制策略，防止变压器过载，保障设备安全运行。

### 3.2 决策逻辑

```
负载率计算：transformer_load = (load_power + ev_charger_power) / transformer_capacity

Level 0 (负载率 ≤ 80%):
  无动作。电池待机，不放电不充电。

Level 1 (80% < 负载率 ≤ 90%):
  电池放电补偿（-10kW）
  不切除负荷
  优先级：1

Level 2 (90% < 负载率 ≤ 95%):
  电池放电（-20kW）
  切除次要负荷（10kW）
  优先级：2

Level 3 (负载率 > 95%):
  紧急放电（-30kW）
  强制切除非重要负荷（20kW）
  优先级：3

低 SOC 保护：
  当 SOC < 20% 且需要放电时，放电功率限制为 max(-10kW)
```

### 3.3 配置参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| transformer_capacity | `f64` | `500.0` | 变压器容量（kVA） |
| demand_factor | `f64` | `0.85` | 需量因子 |
| warning_threshold | `f64` | `0.80` | 预警阈值（Level 1 触发） |
| action_threshold | `f64` | `0.90` | 行动阈值（Level 2 触发） |
| emergency_threshold | `f64` | `0.95` | 紧急阈值（Level 3 触发） |

### 3.4 输出格式

| 字段 | 值 | 说明 |
|------|-----|------|
| cmd_id | 2 | 需量控制策略固定 ID |
| cmd_type | PowerRegulation / SwitchControl | Level 1 为功率调节，Level ≥ 2 为开关控制 |
| p_batt_set | -10/-20/-30 kW | 按等级决定放电功率 |
| load_shedding | 0/10/20 kW | Level ≥ 2 时执行负荷切除 |
| priority | 0~3 | 对应策略等级 |

### 3.5 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| DC-01 | 负载率 ≤ 80% 时无动作（p_batt = 0, priority = 0） | 单元测试 |
| DC-02 | 负载率 80%-90% 时电池放电 -10kW（Level 1） | 单元测试 |
| DC-03 | 负载率 90%-95% 时放电 -20kW + 切负荷 10kW（Level 2） | 单元测试 |
| DC-04 | 负载率 > 95% 时紧急放电 -30kW + 切负荷 20kW（Level 3） | 单元测试 |
| DC-05 | SOC < 20% 时放电功率受限（max -10kW） | 单元测试 |
| DC-06 | 支持自定义阈值配置 | 单元测试 |
| DC-07 | 负载率计算正确包含 EV 充电桩功率 | 单元测试 |

---

## 4. 防逆流保护策略

### 4.1 概述

防止光伏发电过剩时向电网逆向送电，通过电池充电消纳余电，电池满载时限制 PV 出力。

### 4.2 决策逻辑

```
触发条件：grid_power < reverse_power_threshold（默认 -0.1kW，允许微小逆流）

Step 1 - 电池有余量（SOC < soc_charge_max）：
  电池充电功率 = min(PV出力 × 0.8, max_charge_power)
  不限制 PV 出力

Step 2 - 电池已满（SOC ≥ soc_charge_max）：
  限制 PV 出力 = PV出力 × (pv_limit_count × 0.1)，上限 50%
  电池不充电

Step 3 - PV 已限制且仍逆流：
  触发告警（通过消息总线发送）

恢复正常：
  当 grid_power ≥ reverse_power_threshold 时，恢复正常运行
  pv_limit_count 清零
```

### 4.3 配置参数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| reverse_power_threshold | `f64` | `-0.1` | 逆功率阈值（kW），允许微小逆流 |
| pv_limit_step | `f64` | `0.10` | 每次 PV 限制幅度（比例） |
| max_charge_power | `f64` | `50.0` | 最大充电功率（kW） |
| soc_charge_max | `f64` | `80.0` | SOC 充电上限（%） |

### 4.4 状态管理

| 状态 | 初始值 | 说明 |
|------|--------|------|
| pv_limit_count | 0 | 连续 PV 限制次数，每次逆流且电池满时递增，恢复正常时清零 |

### 4.5 输出格式

| 字段 | 值 | 说明 |
|------|-----|------|
| cmd_id | 3 | 防逆流策略固定 ID |
| cmd_type | PowerRegulation | 功率调节 |
| p_batt_set | 正数（充电）/ 0 | 电池充电功率 |
| pv_limit | 0.0~0.5 | PV 限功率比例，无限制时为 None |
| priority | 2 | 默认优先级 |

### 4.6 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| AR-01 | 检测到电网逆流（-5kW）且电池未满时，增加电池充电 | 单元测试 |
| AR-02 | 检测到电网逆流且电池已满时，限制 PV 出力 | 单元测试 |
| AR-03 | 电网正常（正向功率 10kW）时无动作 | 单元测试 |
| AR-04 | 策略类型标记为 Fallback | 单元测试 |
| AR-05 | 策略名称返回 "AntiReverseStrategy" | 单元测试 |

---

## 5. 电压越限与三相不平衡无功补偿

### 5.1 概述

通过电池逆变器提供无功功率支撑，改善台区电压质量和三相不平衡度。当前为**接口预留阶段**，完整的决策逻辑和实现延后至后续 Phase。

### 5.2 已预留接口

ControlCommand 中已包含以下字段，供无功补偿策略使用：

| 字段 | 类型 | 用途 |
|------|------|------|
| `q_batt_set` | `Option<f64>` | 电池无功设定值（kVar），范围 -1000 ~ +1000 |
| `phase_compensation` | `Option<[f64; 3]>` | A/B/C 三相分相补偿系数 |

### 5.3 计划策略（Phase 2+）

| 策略 | 触发条件 | 动作 |
|------|----------|------|
| 电压越限补偿 | 电压超出额定范围 ±7%（或 ±10%，按国标要求） | 电池吸收/发出无功 |
| 三相不平衡补偿 | 三相电流不平衡度 > 15% | 分相无功补偿 |

### 5.4 验收标准（预留）

| ID | 标准 | 状态 |
|----|------|------|
| VC-01 | 电压越限时正确计算无功补偿量 | Phase 2+ |
| VC-02 | 三相不平衡补偿比例正确 | Phase 2+ |

---

## 6. AI 指令安全校验（AiCommandValidator）

### 6.1 概述

AiCommandValidator 作为 AI 引擎与执行层之间的**安全闸门**，对所有 AI 决策指令进行校验。校验不通过时自动降级至本地兜底模式。

### 6.2 接口定义

```rust
/// AI 指令校验器 Trait（可插拔）
pub trait AiCommandValidator: Send + Sync {
    async fn validate(&self, cmd: &ControlCommand) -> ValidationResult;
    fn name(&self) -> &str;
}

/// 校验结果
pub struct ValidationResult {
    pub valid: bool,                       // 是否通过
    pub message: String,                   // 错误消息
    pub suggested_command: Option<ControlCommand>,  // 建议命令
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

### 6.3 校验规则

| 规则 | 条件 | 处理 |
|------|------|------|
| 有模型校验 | 已挂载 AiModel | 比较 AI 推荐值与实际命令差值，若差值 > 10kW 且置信度 < 0.7 则拒绝 |
| 无模型校验 | 未挂载 AiModel | 默认通过（返回 Valid） |
| 开关命令 | cmd_type != PowerRegulation | 直接通过，不校验 |
| 功率命令 | cmd_type == PowerRegulation | 调用模型预测后校验 |

### 6.4 降级流程

```
AiCommandValidator.validate(cmd)
  ├── 校验通过 → 指令继续下发
  └── 校验不通过 →
        ├── 记录告警日志
        ├── 丢弃 AI 指令
        ├── 切换至本地兜底模式
        └── FallbackStrategy.evaluate(data) 生成兜底指令
```

### 6.5 实现阶段

| Phase | 实现 | 说明 |
|-------|------|------|
| Phase 1 | MockAiModel（默认返回 confidence=0.5） | 仅接口定义，校验默认通过 |
| Phase 3A | AiCommandValidatorImpl + MockAiModel | 完整接口实现，含模拟预测逻辑 |
| Phase 3C | LSTM/TCN 真实模型替换 | 高精度 AI 校验 |

### 6.6 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| AV-01 | 无模型时校验默认通过（Valid） | 单元测试 |
| AV-02 | 有模型时调用 predict 进行校验 | 单元测试 |
| AV-03 | 开关命令直接通过校验 | 单元测试 |
| AV-04 | MockAiModel 在 SOC 高时推荐放电 | 单元测试 |
| AV-05 | MockAiModel 在 SOC 低时推荐充电 | 单元测试 |
| AV-06 | MockAiModel 在 SOC 中等时推荐待机 | 单元测试 |

---

## 7. 策略模式切换

### 7.1 模式定义

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrategyType {
    Basic,        // 基础模式 - 无自动控制
    Intelligent,  // 智能模式 - AI 引擎决策
    Fallback,     // 兜底模式 - 本地策略引擎
}
```

### 7.2 切换触发器

| 当前模式 | 切换条件 | 目标模式 |
|----------|----------|----------|
| Intelligent | AI 引擎心跳超时 / 状态异常 | Fallback |
| Intelligent | AiValidator 校验不通过 | Fallback |
| Fallback | AI 引擎恢复 | Intelligent |
| Any | 运维人员手动切换 | Basic / Intelligent / Fallback |
| Basic | 运维人员手动切换 | Intelligent / Fallback |

### 7.3 AI 集成器（AiIntegrator）

负责管理 AI 模型生命周期，提供 AI 决策接口：

```rust
pub struct AiIntegrator {
    model_manager: Arc<RwLock<Option<ModelManager>>>,
    status: Arc<RwLock<ModelStatus>>,
}
```

**关键方法：**
- `initialize(config)` —— 加载 AI 模型
- `get_decision(state)` —— 获取 AI 决策
- `is_ready()` —— 检查 AI 是否就绪
- `status()` —— 获取当前状态（Unloaded / Loading / Ready / Error）

### 7.4 状态管理

| AiIntegrator 状态 | 策略模式 | 说明 |
|--------------------|----------|------|
| Unloaded | Fallback / Basic | 模型未加载，使用兜底策略 |
| Loading | Fallback | 模型加载中，暂用兜底策略 |
| Ready | Intelligent | 模型就绪，AI 决策 + Validator 校验 |
| Error | Fallback | 模型异常，自动降级 |

### 7.5 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| SM-01 | AiIntegrator 创建时状态为 Unloaded | 单元测试 |
| SM-02 | AI 引擎就绪时正常返回决策 | 集成测试 |
| SM-03 | AI 引擎异常时自动降级至兜底模式 | 集成测试 |

---

## 8. 错误类型

### 8.1 StrategyError

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

实现 `std::error::Error` trait，支持错误链传递。

---

## 9. 数据流架构

### 9.1 整体数据流

```
intercore (TCP/RJ45)
    │
    ▼
DataCollector → HighFrequencyTelemetry (1Hz)
    │
    ▼
Message Bus (tokio::sync::mpsc)
    │
    ├──→ DataReporter (暂不使用)
    ├──→ FaultRecorder (SQLite)
    │
    ▼
strategy-engine
    ├── 削峰填谷 (PeakShavingStrategy)
    ├── 需量控制 (DemandControlStrategy)
    ├── 防逆流 (AntiReverseStrategy)
    │
    ▼
AiCommandValidator (可插拔 AI 模型)
    │
    ▼
ControlCommand → Message Bus → intercore → 实时控制模块
```

### 9.2 消息主题

| 主题 | 生产者 | 消费者 | 说明 |
|------|--------|--------|------|
| `telemetry.high_freq` | DataCollector | strategy-engine | 高频遥测数据 |
| `telemetry.fault` | FaultRecorder | 外部 | 故障事件 |
| `strategy.command` | strategy-engine | intercore | 控制指令 |
| `strategy.decision` | strategy-engine | DataReporter | 策略决策结果 |

### 9.3 模块依赖

```
strategy-engine
  ├── core (message_bus)
  ├── data-processing (DataPackage 消费)
  ├── common (MupcError)
  └── mupc-ai-engine (ModelManager, SystemState, ActionOutput)
```

---

## 10. 非功能性需求

### 10.1 性能需求

| 指标 | 要求 |
|------|------|
| 策略决策延迟 | < 100ms（从收到数据到输出控制命令） |
| 内存占用 | 每个策略实例 < 10MB |
| 并发评估 | 支持三个策略同时运行，互不阻塞 |

### 10.2 可靠性需求

| 指标 | 要求 |
|------|------|
| AI 失效检测时间 | < 1 个心跳周期（1 秒） |
| 模式切换时间 | < 50ms |
| 无单点故障 | 任一策略故障不影响其他策略运行 |

### 10.3 可维护性需求

| 需求 | 说明 |
|------|------|
| 可插拔策略 | 新增策略只需实现 `FallbackStrategy` trait |
| 可插拔 AI 模型 | 替换校验模型只需实现 `AiModel` trait |
| 运行时配置 | 策略参数支持配置文件热加载（Phase 2+） |
| 日志 | 每次决策记录 TRACE 级别日志，包含输入参数和输出结果 |

### 10.4 安全需求

| 需求 | 说明 |
|------|------|
| 指令范围校验 | AI 指令的 P/Q 设定值必须在额定范围内 |
| 变化率限制 | AI 指令变化率不超过每周期 10%（Phase 2+） |
| 降级保护 | 校验不通过时自动降级，不阻塞控制 |

---

## 11. 验收标准汇总

### 11.1 削峰填谷（PS）

| ID | 标准 | 优先级 |
|----|------|--------|
| PS-01 | 峰时段 + SOC > 80% 放电 -20kW | P0 |
| PS-02 | 谷时段 + SOC < 20% 充电 20kW | P0 |
| PS-03 | 谷时段 + PV 充足，按 PV 充电 | P0 |
| PS-04 | 峰时段 + SOC 正常，放电 -25kW | P0 |
| PS-05 | 平时段待机 | P0 |
| PS-06 | SOC < 下限强制充电 | P0 |
| PS-07 | SOC > 上限强制放电 | P0 |
| PS-08 | 跨天谷时段检测正确 | P0 |

### 11.2 需量控制（DC）

| ID | 标准 | 优先级 |
|----|------|--------|
| DC-01 | 负载率 ≤ 80% 无动作 | P0 |
| DC-02 | 负载率 80%-90% 放电 -10kW（Level 1） | P0 |
| DC-03 | 负载率 90%-95% 放电 -20kW + 切负荷（Level 2） | P0 |
| DC-04 | 负载率 > 95% 紧急放电 -30kW + 切负荷（Level 3） | P0 |
| DC-05 | SOC < 20% 放电受限 | P0 |
| DC-06 | 支持自定义阈值 | P1 |
| DC-07 | 负载率正确包含 EV 功率 | P1 |

### 11.3 防逆流（AR）

| ID | 标准 | 优先级 |
|----|------|--------|
| AR-01 | 逆流时电池充电（电池未满） | P0 |
| AR-02 | 逆流 + 电池满，限制 PV | P0 |
| AR-03 | 正常功率时无动作 | P0 |
| AR-04 | 策略类型为 Fallback | P1 |
| AR-05 | 策略名称正确 | P1 |

### 11.4 AI 校验（AV）

| ID | 标准 | 优先级 |
|----|------|--------|
| AV-01 | 无模型时默认通过 | P0 |
| AV-02 | 有模型时调用 predict | P0 |
| AV-03 | 开关命令直接通过 | P0 |
| AV-04 | SOC 高时推荐放电 | P1 |
| AV-05 | SOC 低时推荐充电 | P1 |
| AV-06 | SOC 中等时推荐待机 | P1 |

### 11.5 模式切换（SM）

| ID | 标准 | 优先级 |
|----|------|--------|
| SM-01 | 创建时状态为 Unloaded | P0 |
| SM-02 | 就绪时正常返回决策 | P0 |
| SM-03 | 异常时自动降级 | P0 |

### 11.6 功能回归验证

| 验证项 | 验证方法 | 条件 |
|--------|----------|------|
| `cargo build --release` 编译成功 | CI | 每次变更 |
| `cargo clippy` 无警告 | CI | 每次变更 |
| `cargo test -p mupc-strategy-engine` 全通过 | CI | 每次变更 |
| `cargo fmt` 格式化通过 | CI | 每次变更 |

---

## 12. 未来扩展

| Phase | 内容 | 说明 |
|-------|------|------|
| Phase 3B | 消息总线扩展（AMQP/MQTT） | 支持更多消费者 |
| Phase 3C | AI 优化引擎集成（LSTM/TCN + MADDPG/PPO） | 替换 MockAiModel |
| Phase 2+ | 电压越限无功补偿 | 完整策略实现 |
| Phase 2+ | 三相不平衡补偿 | 分相无功补偿 |
| Phase 2+ | 运行时配置热加载 | 配置修改无需重启 |

---

## 附录 A：ControlCommand 完整定义

```rust
pub struct ControlCommand {
    pub cmd_id: u16,                          // 命令 ID
    pub cmd_type: CommandType,                // 命令类型
    pub p_batt_set: Option<f64>,             // 电池有功设定 (kW)
    pub q_batt_set: Option<f64>,             // 电池无功设定 (kVar)
    pub phase_compensation: Option<[f64; 3]>, // 分相补偿系数
    pub start_stop: Option<bool>,            // 启停命令
    pub priority: u8,                        // 优先级
    pub pv_limit: Option<f64>,               // PV 限功率比例
    pub load_shedding: Option<f64>,          // 负荷切除功率 (kW)
}

pub enum CommandType {
    SwitchControl,      // 开关控制
    PowerRegulation,    // 功率调节
    ChargeDischarge,    // 充放电控制
}
```

## 附录 B：策略 ID 分配

| 策略 | cmd_id | 说明 |
|------|--------|------|
| 削峰填谷 | 1 | 固定 ID，用于日志追踪和消息路由 |
| 需量控制 | 2 | 固定 ID |
| 防逆流 | 3 | 固定 ID |
| 保留 | 4-10 | 供 Phase 2+ 扩展策略使用 |

## 附录 C：术语表

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

---

**文档状态：** 初版（v1.0）
**合并来源：** 通信管理模块 PRD v1.3 + Phase3A 规格文档 v1.0
**产出时间：** 2026-05-29
