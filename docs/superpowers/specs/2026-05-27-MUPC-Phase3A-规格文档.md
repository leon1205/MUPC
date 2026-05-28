# MUPC Phase 3A 规格文档 - data-processing + strategy-engine 完整实现

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-27 | 需求分析师 | ✅ 已评审 |

---

## 1. 概述

### 1.1 项目背景

MUPC Phase 3A 实现 data-processing 和 strategy-engine 模块的完整功能，作为消息总线和 AI 优化引擎的前置依赖。

**目标**：
- 完成 data-processing 四个接口的完整实现
- 完成 strategy-engine 三个兜底策略的完整实现
- 建立模块间消息通信机制

### 1.2 范围

| 模块 | 实现内容 | 优先级 |
|------|----------|--------|
| data-processing | DataCollector, HighFrequencyTelemetry, DataReporter, FaultRecorder | 高 |
| strategy-engine | FallbackStrategy (削峰填谷/需量控制/防逆流), AiCommandValidator | 高 |
| core (扩展) | 消息总线实现，支持多消费者 | 中 |

---

## 2. 功能列表

### 2.1 data-processing 模块

#### 2.1.1 DataCollector - 数据采集

**职责**：从 intercore 模块接收实时控制模块的数据

**接口**：
```rust
pub trait DataCollector {
    async fn start(&mut self) -> Result<(), DataProcessingError>;
    async fn stop(&mut self) -> Result<(), DataProcessingError>;
    fn get_latest_data(&self) -> Option<TelemetryData>;
}
```

**数据来源**：intercore 模块（TCP/RJ45）

**采集数据类型**：
| 数据类型 | 说明 | 单位 |
|----------|------|------|
| battery_soc | 电池荷电状态 | % |
| battery_power | 电池充放电功率 | kW |
| pv_output | 光伏出力 | kW |
| load_power | 负荷功率 | kW |
| grid_power | 电网功率（有功）| kW |
| transformer_load | 变压器负载率 | % |

#### 2.1.2 HighFrequencyTelemetry - 高频遥测

**职责**：以 ≥1Hz 频率上报遥测数据

**接口**：
```rust
pub trait HighFrequencyTelemetry {
    async fn start(&mut self) -> Result<(), DataProcessingError>;
    async fn stop(&mut self) -> Result<(), DataProcessingError>;
    fn get_current_value(&self, point: &str) -> Option<f64>;
}
```

**上报频率**：1Hz（可配置）

**内存缓冲**：保留最近 1 分钟数据（60 条记录）

#### 2.1.3 DataReporter - 数据上报

**职责**：通过消息总线将处理后的数据发送给消费者

**接口**：
```rust
pub trait DataReporter {
    async fn report(&self, data: TelemetryData) -> Result<(), DataProcessingError>;
    fn subscribe(&mut self, topic: &str) -> Result<(), DataProcessingError>;
}
```

**消息主题**：
| 主题 | 内容 |
|------|------|
| `telemetry.high_freq` | 高频遥测数据 |
| `telemetry.fault` | 故障事件 |
| `strategy.decision` | 策略决策结果 |

#### 2.1.4 FaultRecorder - 故障录波

**职责**：记录故障事件并持久化到 SQLite

**接口**：
```rust
pub trait FaultRecorder {
    async fn record(&self, event: FaultEvent) -> Result<(), DataProcessingError>;
    async fn query(&self, start: DateTime, end: DateTime) -> Result<Vec<FaultRecord>, DataProcessingError>;
}
```

**故障类型**：
| 类型 | 触发条件 |
|------|----------|
| BATTERY_OVER_TEMP | 电池温度 > 60°C |
| BATTERY_UNDER_SOC | 电池 SOC < 10% |
| GRID_OVERLOAD | 电网功率 > 额定 110% |
| GRID_REVERSE | 检测到逆向功率流（防逆流） |
| PV_OUTPUT_LIMIT | 光伏出力被限制 |

**存储**：SQLite，保留 30 天

### 2.2 strategy-engine 模块

#### 2.2.1 削峰填谷策略 (Peak Shaving & Valley Filling)

**触发条件**：
- 电价时段配置

**策略逻辑**：
```
充电优先级：
  1. 谷时 + PV 低 → 电网充电
  2. 谷时 + PV 高 → PV 充电
  3. 平时 + PV 高 → PV 充电

放电优先级：
  1. 峰时 → 放电
  2. 平时 + PV 低 + SOC 高 → 放电
  3. 电网峰值 → 放电
```

**配置参数**：
| 参数 | 类型 | 默认值 |
|------|------|--------|
| peak_hours | Vec<(start, end)> | [08:00-11:00, 18:00-21:00] |
| valley_hours | Vec<(start, end)> | [23:00-07:00] |
| soc_charge_max | f64 | 80% |
| soc_charge_min | f64 | 20% |
| battery_capacity | f64 | 100 kWh |

#### 2.2.2 需量控制策略 (Demand Control)

**触发条件**：
- 变压器负载率 > 预警阈值（80%）

**策略逻辑**：
```
Level 1 (80% < 负载率 ≤ 90%):
  → 电池放电补偿

Level 2 (负载率 > 90%):
  → 电池放电 + 切除次要负荷
  → 优先切除：空调 > 照明 > 其他

Level 3 (负载率 > 95%):
  → 紧急放电 + 强制切除非重要负荷
```

**配置参数**：
| 参数 | 类型 | 默认值 |
|------|------|--------|
| transformer_capacity | f64 | 500 kVA |
| demand_factor | f64 | 0.85 |
| warning_threshold | f64 | 0.80 |
| action_threshold | f64 | 0.90 |
| emergency_threshold | f64 | 0.95 |

#### 2.2.3 防逆流策略 (Anti-Reverse Power)

**触发条件**：
- 检测到电网功率 < 0（反向送电）

**策略逻辑**：
```
Step 1: 增加电池充电功率（消纳光伏余电）
Step 2: 如果电池满载 → 限制 PV 出力
Step 3: 如果 PV 已限制 → 触发告警
```

**配置参数**：
| 参数 | 类型 | 默认值 |
|------|------|--------|
| reverse_power_threshold | f64 | -0.1 kW（允许微小逆流）|
| pv_limit_step | f64 | 10%（每次限制幅度）|

#### 2.2.4 AiCommandValidator - AI 指令校验（可插拔）

**接口**：
```rust
pub trait AiCommandValidator: Send + Sync {
    async fn validate(&self, cmd: &StrategyCommand) -> ValidationResult;
    fn set_model(&mut self, model: Box<dyn AiModel>);
}

pub trait AiModel: Send + Sync {
    fn predict(&self, input: &ModelInput) -> ModelOutput;
}
```

**当前实现**：
- 默认返回 `ValidationResult::Valid`
- Phase 3C 替换为真正的 LSTM/TCN 模型

---

## 3. 数据流设计

### 3.1 整体数据流

```
┌─────────────────────────────────────────────────────────────────┐
│                        intercore                                 │
│                    (实时控制模块数据)                             │
└─────────────────────────────┬───────────────────────────────────┘
                              │ TCP/RJ45
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     DataCollector                                │
│  - 接收数据                                                      │
│  - 缓存最新值                                                    │
└─────────────────────────────┬───────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                 HighFrequencyTelemetry                           │
│  - 1Hz 频率上报                                                  │
│  - 内存缓冲（60条）                                               │
│  - 通过 mpsc 发送到消息总线                                       │
└─────────────────────────────┬───────────────────────────────────┘
                              │ mpsc channel
                              ▼
                    ┌─────────────────┐
                    │   Message Bus    │
                    │   (core 模块)    │
                    └────────┬────────┘
                             │
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
    ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
    │ DataReporter│  │FaultRecorder│  │ 其他消费者  │
    │  (暂不使用) │  │  (SQLite)   │  │  (Phase 3B) │
    └─────────────┘  └─────────────┘  └─────────────┘
              │
              ▼
┌─────────────────────────────────────────────────────────────────┐
│                   strategy-engine                                │
│  ┌───────────────┐ ┌───────────────┐ ┌───────────────┐          │
│  │ 削峰填谷      │ │ 需量控制      │ │ 防逆流        │          │
│  │ FallbackStrat │ │ FallbackStrat │ │ FallbackStrat │          │
│  └───────────────┘ └───────────────┘ └───────────────┘          │
│                          │                                       │
│                          ▼                                       │
│                   AiCommandValidator                            │
│                   (可插拔 AI 模型)                               │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 消息总线主题

| 主题 | 生产者 | 消费者 | 说明 |
|------|--------|--------|------|
| `telemetry.high_freq` | DataCollector | strategy-engine, 外部 | 高频遥测数据 |
| `telemetry.fault` | FaultRecorder | 外部 | 故障事件 |
| `strategy.command` | strategy-engine | intercore | 控制指令 |

---

## 4. 架构设计

### 4.1 模块依赖

```
data-processing
  ├── core (message_bus)
  ├── intercore (数据来源)
  └── device-trait (设备抽象)

strategy-engine
  ├── core (message_bus)
  ├── data-processing (数据消费)
  └── device-trait (设备抽象)
```

### 4.2 错误类型

```rust
#[derive(Error, Debug)]
pub enum DataProcessingError {
    #[error("数据采集失败: {0}")]
    CollectionFailed(String),

    #[error("消息发送失败: {0}")]
    MessageSendFailed(String),

    #[error("数据库错误: {0}")]
    DatabaseError(String),

    #[error("配置错误: {0}")]
    ConfigError(String),
}

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

---

## 5. 验收标准

### 5.1 data-processing 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| DP-01 | DataCollector 能从 intercore 接收数据 | 单元测试 |
| DP-02 | HighFrequencyTelemetry 以 1Hz 上报数据 | 单元测试 |
| DP-03 | 数据在内存中缓冲 60 条 | 单元测试 |
| DP-04 | DataReporter 通过消息总线发送数据 | 单元测试 |
| DP-05 | FaultRecorder 正确记录故障到 SQLite | 集成测试 |
| DP-06 | 故障记录查询功能正常 | 单元测试 |

### 5.2 strategy-engine 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| SE-01 | 削峰填谷策略根据电价时段正确决策 | 单元测试 |
| SE-02 | 需量控制在负载率 >80% 时触发电池放电 | 单元测试 |
| SE-03 | 防逆流在检测到逆功率时限制 PV 出力 | 单元测试 |
| SE-04 | AiCommandValidator 接口可替换 | 单元测试 |
| SE-05 | 策略决策结果发送到消息总线 | 单元测试 |

### 5.3 非功能需求

| 类型 | 要求 |
|------|------|
| 延迟 | 策略决策 < 100ms |
| 内存 | 每个模块 < 10MB |
| 存储 | SQLite 数据库 < 100MB（30天数据）|

---

## 6. 技术栈

| 组件 | 选择 |
|------|------|
| 语言 | Rust 1.75+ |
| 异步运行时 | Tokio 1.x |
| 数据库 | SQLite (rusqlite) |
| 消息总线 | tokio::sync::mpsc |

---

## 7. 未来扩展 (Phase 3B/3C)

| Phase | 内容 |
|-------|------|
| 3B | 消息总线扩展（AMQP/MQTT）|
| 3C | AI 优化引擎（LSTM/TCN + MADDPG/PPO）|

---

**评审状态**：待评审