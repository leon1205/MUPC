# MUPC AI 引擎 - 模块设计文档（合并版）

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v2.0 | 2026-05-29 | 架构师 | 合并版（含预设运行场景改造） |

---

[DESIGN_APPROVED] — Phase 3C AI 优化引擎基础架构

[DESIGN_APPROVED]: true — RKNN Runtime FFI 设计（6 项验收标准验证通过）

[DESIGN_APPROVED]: true — AI 场景自适应与强化学习完整设计（6 个修复项验证通过，设计评审批准）

[DESIGN_APPROVED]: true — AI 预设运行场景与互斥模式选择（ModeSelector 替代 SceneClassifier，设计评审批准）

---

**来源文档：**

| 文档 | 路径 | 状态 |
|------|------|------|
| Phase 3C AI 优化引擎设计文档 | `docs/superpowers/plans/2026-05-28-MUPC-Phase3C-AI优化引擎-设计文档.md` | [DESIGN_APPROVED] |
| Phase 3C 实施计划 | `docs/superpowers/plans/2026-05-28-MUPC-Phase3C-AI优化引擎-实施计划.md` | 已归档 |
| RKNN Runtime FFI 设计文档 | `docs/superpowers/plans/2026-05-28-RKNN-Runtime-FFI实现-设计文档.md` | [DESIGN_APPROVED] |
| AI 场景自适应与 RL 设计文档 | `docs/superpowers/plans/2026-05-29-MUPC-AI场景自适应与RL-设计文档.md` | [DESIGN_APPROVED]（第 4 章已被 v2.0 废弃） |
| AI 预设运行场景与互斥模式选择设计 | `docs/superpowers/plans/2026-05-29-MUPC-AI预设运行场景与互斥模式选择-设计文档.md` | **[DESIGN_APPROVED]** |
| AI 引擎 PRD | `docs/superpowers/specs/modules/05-MUPC-AI引擎-PRD.md` | v2.0 |
| 预设运行场景 PRD | `docs/superpowers/specs/2026-05-29-MUPC-AI预设运行场景与互斥模式选择-PRD.md` | [REVIEWED: PASS] |

---

## 目录

1. [模块架构](#1-模块架构)
2. [LSTM 模型设计](#2-lstm-模型设计)
3. [多源数据融合设计](#3-多源数据融合设计)
4. [场景分类器设计](#4-场景分类器设计)
5. [强化学习模型设计](#5-强化学习模型设计)
6. [奖励函数计算模块](#6-奖励函数计算模块)
7. [RKNN Runtime 设计](#7-rknn-runtime-设计)
8. [ModelManager 统一调度设计](#8-modelmanager-统一调度设计)
9. [与策略引擎集成设计](#9-与策略引擎集成设计)
10. [文件结构](#10-文件结构)
11. [配置结构](#11-配置结构)
12. [错误类型](#12-错误类型)
13. [消息总线集成](#13-消息总线集成)
14. [技术决策记录](#14-技术决策记录)

---

## 1. 模块架构

### 1.1 整体架构

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         AI 优化引擎 (ai-engine)                                 │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  数据源层                       融合层                 决策层               │
│  ┌──────────┐              ┌──────────────┐       ┌──────────────┐        │
│  │intercore │────TCP──────▶│              │       │ SceneClassifier│       │
│  │(实时数据) │              │ DataFusion   │──────▶│ (场景识别)    │       │
│  ├──────────┤              │ Engine       │       └──────┬───────┘        │
│  │ LSTM     │────预测─────▶│ (1Hz融合)    │              │                 │
│  │ (预测值)  │              │              │       ┌──────▼───────┐        │
│  ├──────────┤              │ 输出:        │       │ RewardCalculator│      │
│  │气象 API  │────拉取─────▶│ FusedSystem  │──────▶│ (奖励计算)    │       │
│  │          │              │ State        │       └──────┬───────┘        │
│  ├──────────┤              │ (50维向量)   │              │                 │
│  │物联平台   │────订阅─────▶│              │       ┌──────▼───────┐        │
│  │(电价)    │              │              │       │ RLModel       │        │
│  ├──────────┤              │              │       │ (决策模型)    │        │
│  │gateway   │────事件─────▶│              │       └──────┬───────┘        │
│  │(调度指令) │              └──────────────┘              │                 │
│  └──────────┘                                          │                 │
│                                                  ┌──────▼───────┐        │
│                                                  │ ActionValidator│       │
│                                                  │ (约束校验)    │       │
│                                                  └──────┬───────┘        │
│                                                         │                 │
│                                                  ┌──────▼───────┐        │
│                                                  │ ModelManager  │        │
│                                                  │ (统一调度)    │        │
│                                                  └──────┬───────┘        │
│                                                         │                 │
│                    ┌────────▼────────┐                   │                 │
│                    │  RKNN Runtime   │ ─── FFI ─── librknnrt.so            │
│                    │  (NPU 推理)     │                   │                 │
│                    └────────┬────────┘                   │                 │
│                             │                            │                 │
│              ┌──────────────┼──────────────┐             │                 │
│              ▼              ▼              ▼              │                 │
│        ┌─────────┐   ┌─────────┐   ┌─────────┐          │                 │
│        │ RK3588  │   │  x86    │   │ 混合    │          │                 │
│        │ NPU     │   │ Server  │   │ 部署    │          │                 │
│        └─────────┘   └─────────┘   └─────────┘          │                 │
└──────────────────────────────────────────────────────────┼──────────────────┘
                                                           │
                                                    ┌──────▼───────┐
                                                    │ strategy-engine │
                                                    │ (AiIntegrator)  │
                                                    │ (AiCommandValidator)│
                                                    └──────┬───────┘
                                                           │
                                                    ┌──────▼───────┐
                                                    │ intercore    │
                                                    │ (实时控制模块) │
                                                    └──────────────┘
```

### 1.2 核心模块职责

| 模块 | 职责 |
|------|------|
| `DataFusionEngine` | 1Hz 频率从 5 个数据源汇聚 23 个字段，输出 FusedSystemState |
| `ModeSelector` | 运行场景选择器，互斥保证，接收远程/本地切换指令（v2.0 替代 SceneClassifier） |
| `RewardCalculator` | 根据场景选择奖励函数计算奖励值，用于在线微调 |
| `LSTMModel` | 时序预测（光伏出力、负荷预测未来 15 分钟）|
| `RLModel` | MADDPG/PPO 强化学习决策，输出 7 维动作空间 |
| `ActionValidator` | 6 条约束规则校验，物理约束强制 clamp |
| `ModelManager` | 统一接口、模型加载、full_decision_cycle 调度 |
| `OnlineUpdater` | 在线微调（增量学习，Phase 3C.2）|
| `RknnRuntime` | RK3588 NPU 推理运行时（FFI 绑定）|

### 1.3 核心设计原则

1. **模块单一职责**：每个新增模块只负责一个明确的职能，通过消息总线（共享内存 + Tokio 广播通道）解耦
2. **降级优先**：所有外部依赖（气象 API、物联平台、调度指令）均设计为可选，缺失时使用缓存值或默认值，不阻塞主流程
3. **延迟预算严格**：每个处理阶段有明确的延迟上限预算，超过预算时触发降级
4. **可观测性**：所有模块通过 tracing 产生结构化日志，关键决策点记录输入输出快照以便回放

### 1.4 数据流

```
历史数据 → LSTMModel.predict() → 光伏/负荷预测值 → 供 RL 模型使用
                                                         ↓
远程指令/本地选择 → ModeSelector → 运行模式 → 权重映射 + 奖励函数选择
                                                         ↓
融合数据 + 预测值 → RLModel.decide_fused() → ActionOutput
                                                         ↓
ActionOutput → ActionValidator (6条约束规则) → strategy-engine (指令校验 + 兜底)
                                                         ↓
新数据积累 → OnlineUpdater.update() → 模型权重增量更新 → 保存
```

### 1.5 完整决策周期

```
ModelManager.full_decision_cycle():
  1. DataFusionEngine.fuse_once()       — 数据融合 (<1ms)
  2. mode_selector.current()            — 获取当前运行模式 (<0.001ms，v2.0 替代场景识别)
  3. RLModel.decide_fused()             — RL 决策 (<100ms)
  4. ActionValidator.validate()         — 动作约束校验 (<0.5ms)
  5. RewardCalculator.calculate(mode)   — 奖励计算 (<1ms，传入 RunningMode)
  6. 输出 DecisionCycleResult           — 总延迟 <120ms
```

---

## 2. LSTM 模型设计

### 2.1 概述

LSTM（Long Short-Term Memory）时序预测模型，负责预测未来一段时间内的光伏出力和负荷功率，为强化学习决策模型提供前瞻性输入。

### 2.2 预测规格

| 项目 | 值 |
|------|-----|
| 预测目标 | 光伏出力预测（PV forecast）、负荷功率预测（Load forecast） |
| 预测范围 | 未来 15 分钟，每分钟一个采样点（共 15 个点）|
| 输入窗口 | 历史 1 小时数据（默认 60 个数据点，每分钟 1 点）|
| 输入特征 | 历史光伏出力、历史负荷功率、光照强度、环境温度 |
| 输出格式 | 2 个向量，各 15 个元素 |
| 模型格式 | ONNX（训练）→ INT8 量化后部署为 .rknn |

### 2.3 精度要求

| 指标 | 要求 | 测量方法 |
|------|------|----------|
| 光伏预测 MAPE | <= 10%（15 分钟预测范围） | 回测验证，Mean Absolute Percentage Error |
| 负荷预测 MAPE | <= 15%（15 分钟预测范围） | 回测验证 |

### 2.4 接口定义

```rust
/// LSTM 模型输入
#[derive(Debug, Clone)]
pub struct LstmInput {
    /// 历史时间序列数据
    pub history: Vec<f32>,
    /// 时间戳（UTC 秒）
    pub timestamp: i64,
}

/// LSTM 模型输出
#[derive(Debug, Clone)]
pub struct LstmOutput {
    /// 预测值
    pub predictions: Vec<f32>,
    /// 置信度
    pub confidence: f64,
}
```

### 2.5 ONNX 导出与量化流程

```
训练阶段 (x86 服务器):
PyTorch → ONNX → rknn-toolkit2 量化 → INT8 模型 (.rknn)

部署阶段 (RK3588):
INT8 模型 → RKNN Runtime → NPU 推理 (< 100ms)
```

| 阶段 | 工具 | 输出 |
|------|------|------|
| 模型训练 | PyTorch | .pt 文件 |
| 格式转换 | torch.onnx.export | .onnx 文件 |
| INT8 量化 | rknn-toolkit2 | .rknn 文件 |
| 部署推理 | RKNN Runtime (librknnrt.so) | NPU 推理结果 |

---

## 3. 多源数据融合设计

### 3.1 DataFusionEngine

**文件：** `mupc/crates/ai-engine/src/data_fusion.rs`

DataFusionEngine 是 AI 引擎的数据入口，负责以 1Hz 频率从 5 个数据源汇聚 23 个字段，输出 `FusedSystemState` 供场景分类器和 RL 决策器使用。

### 3.2 数据源适配器架构

每个数据源抽象为一个 Trait，便于单元测试和替换：

```rust
/// 数据源适配器 trait
#[async_trait]
pub trait DataSourceAdapter: Send + Sync {
    /// 数据源名称
    fn name(&self) -> &str;
    /// 获取最新数据
    async fn fetch_latest(&self) -> Result<DataSourceValue, FusionError>;
    /// 数据源健康状态
    fn health(&self) -> DataSourceHealth;
}

/// 数据源健康状态
#[derive(Debug, Clone)]
pub struct DataSourceHealth {
    pub connected: bool,
    pub last_update: Option<Instant>,
    pub consecutive_misses: u32,
    pub status: HealthStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Normal,
    Warning,   // 连续 3 周期无更新
    Error,     // 连续 10 周期无更新
    Disconnected,
}

/// 数据源值（带时间戳的泛型包装）
#[derive(Debug, Clone)]
pub struct DataSourceValue {
    pub timestamp: i64,       // UTC 毫秒时间戳
    pub data: serde_json::Value,
    pub source: String,
}
```

### 3.3 5 个数据源适配器

| 适配器 | 数据来源 | 通信方式 | 更新频率 | 连续缺失阈值 |
|--------|----------|----------|----------|--------------|
| `IntercoreAdapter` | intercore 模块 | 核间 TCP 通道 | 1 Hz | 3 周期 |
| `WeatherAdapter` | 气象 API (data-processing 转发) | 消息总线订阅 | 15 分钟 | 10 周期 |
| `PriceAdapter` | 物联平台 (MQTT 通道) | 消息总线订阅 | 15 分钟 / 事件 | 3 周期 |
| `DispatchAdapter` | gateway (IEC 104/61850) | 消息总线订阅 | 事件驱动 | 不适用 |
| `DemandAdapter` | data-processing 加工 | 消息总线订阅 | 1 Hz | 3 周期 |

### 3.4 FusedSystemState 结构定义

```rust
/// 融合系统状态（完整 RL 状态空间）
///
/// 总共 25 个字段，7 个大类
/// 注意: peak_price / valley_price 仅用于奖励函数计算，不纳入 RL 推理输入向量
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusedSystemState {
    // ── D1: 实时数据 (6个字段) ──
    /// 时间戳 (UTC 毫秒)
    pub timestamp: i64,
    /// 电池荷电状态 [0.0, 1.0]
    pub battery_soc: f64,
    /// 光伏出力 (kW), [-1000.0, 1000.0]
    pub pv_power: f64,
    /// 负荷功率 (kW), [-1000.0, 1000.0]
    pub load_power: f64,
    /// 电网交换功率 (kW), [-1000.0, 1000.0], 正值=购电
    pub grid_power: f64,
    /// 变压器负载率 [0.0, 2.0], 1.0=额定, >1.0=过载
    pub transformer_load: f64,
    /// 电池充放电功率 (kW), [-500.0, 500.0], 负值=充电
    pub battery_power: f64,

    // ── D2: 预测数据 (2个向量) ──
    /// 未来15分钟光伏预测 (kW), 每分钟1个点, 固定15个元素
    pub pv_forecast_15min: Vec<f64>,
    /// 未来15分钟负荷预测 (kW), 每分钟1个点, 固定15个元素
    pub load_forecast_15min: Vec<f64>,

    // ── D3: 电价 (5个字段) ──
    /// 当前实时电价 (元/kWh), [0.0, 2.0]
    pub current_electricity_price: f64,
    /// 下一时段电价 (元/kWh), [0.0, 2.0]
    pub next_period_price: f64,
    /// 分时电价时段: 0=谷, 1=平, 2=峰, 3=尖峰
    pub price_tariff_id: u8,
    /// 峰时电价 (元/kWh), [0.0, 2.0], 用于套利价差计算
    pub peak_price: f64,
    /// 谷时电价 (元/kWh), [0.0, 2.0], 用于套利价差计算
    pub valley_price: f64,

    // ── D4: 需量状态 (3个字段) ──
    /// 当前实际需量 (kW), [0.0, 10000.0]
    pub current_demand: f64,
    /// 需量合同值 (kW), [0.0, 10000.0]
    pub contract_demand: f64,
    /// 本月最大需量 (kW), [0.0, 10000.0]
    pub peak_demand_this_month: f64,

    // ── D5: 电能质量 (5个字段) ──
    /// A 相电压标幺值 [0.8, 1.2] p.u.
    pub voltage_phase_a: f64,
    /// B 相电压标幺值 [0.8, 1.2] p.u.
    pub voltage_phase_b: f64,
    /// C 相电压标幺值 [0.8, 1.2] p.u.
    pub voltage_phase_c: f64,
    /// 三相电压不平衡度 [0.0, 0.05]
    pub voltage_unbalance: f64,
    /// 电网频率 [49.5, 50.5] Hz
    pub frequency: f64,

    // ── D6: 气象 (2个字段) ──
    /// 光照强度 (W/m^2), [0.0, 1500.0]
    pub solar_irradiance: f64,
    /// 环境温度 (deg C), [-20.0, 60.0]
    pub temperature: f64,

    // ── D7: 调度指令 (2个Option字段) ──
    /// 调度有功设定值 (kW), None=无指令
    pub dispatch_p_set: Option<f64>,
    /// 调度无功设定值 (kVar), None=无指令
    pub dispatch_q_set: Option<f64>,
}
```

**状态空间总维度：** 9 个标量 + 2 个 Option 字段 + 2 个向量字段（各 15 维） + 2 个气象字段 + 5 个电能质量字段。序列化为推理输入向量时，各维度按顺序拼接。

### 3.5 序列化为推理输入向量

```rust
impl FusedSystemState {
    /// 序列化为 RKNN Runtime 输入向量
    ///
    /// 向量长度 = 50
    /// 顺序排列:
    ///   [0..5]   D1 标量 (6个, 不含timestamp)
    ///   [6..20]  D2 pv_forecast_15min (15个)
    ///   [21..35] D2 load_forecast_15min (15个)
    ///   [36..38] D3 电价 (3个)
    ///   [39..41] D4 需量 (3个)
    ///   [42..46] D5 电能质量 (5个)
    ///   [47..48] D6 气象 (2个)
    ///   [49]     D7 dispatch_p_set (Option→f64, None=0.0)
    ///   注: dispatch_q_set 暂不纳入输入向量（影响输出约束）
    pub fn to_input_vector(&self) -> Vec<f32> {
        let mut v = Vec::with_capacity(50);

        // D1: 实时数据 (6个)
        v.push(self.battery_soc as f32);
        v.push(self.pv_power as f32);
        v.push(self.load_power as f32);
        v.push(self.grid_power as f32);
        v.push(self.transformer_load as f32);
        v.push(self.battery_power as f32);

        // D2: 预测数据 (30个)
        for &val in self.pv_forecast_15min.iter().take(15) {
            v.push(val as f32);
        }
        while v.len() < 6 + 15 { v.push(0.0); }
        for &val in self.load_forecast_15min.iter().take(15) {
            v.push(val as f32);
        }
        while v.len() < 6 + 15 + 15 { v.push(0.0); }

        // D3: 电价 (3个)
        v.push(self.current_electricity_price as f32);
        v.push(self.next_period_price as f32);
        v.push(self.price_tariff_id as f32);

        // D4: 需量 (3个)
        v.push(self.current_demand as f32);
        v.push(self.contract_demand as f32);
        v.push(self.peak_demand_this_month as f32);

        // D5: 电能质量 (5个)
        v.push(self.voltage_phase_a as f32);
        v.push(self.voltage_phase_b as f32);
        v.push(self.voltage_phase_c as f32);
        v.push(self.voltage_unbalance as f32);
        v.push(self.frequency as f32);

        // D6: 气象 (2个)
        v.push(self.solar_irradiance as f32);
        v.push(self.temperature as f32);

        // D7: 调度指令 (1个, dispatch_p_set)
        v.push(self.dispatch_p_set.unwrap_or(0.0) as f32);

        debug_assert!(v.len() == 50, "Input vector must be 50, got {}", v.len());
        v
    }
}
```

### 3.6 融合主循环

```rust
/// 数据融合引擎
pub struct DataFusionEngine {
    config: FusionConfig,
    // 5 个数据源适配器
    intercore: Arc<Box<dyn DataSourceAdapter>>,
    weather: Arc<Box<dyn DataSourceAdapter>>,
    price: Arc<Box<dyn DataSourceAdapter>>,
    dispatch: Arc<Box<dyn DataSourceAdapter>>,
    demand_proc: Arc<Box<dyn DataSourceAdapter>>,
    // LSTM 预测数据提供器
    lstm_provider: Arc<Box<dyn LstmProvider>>,
    // 上一周期的完整状态（用于缺失填充）
    last_state: Arc<RwLock<Option<FusedSystemState>>>,
    // 融合输出广播通道
    state_tx: broadcast::Sender<FusedSystemState>,
    // 健康监控
    health_monitor: Arc<RwLock<HashMap<String, DataSourceHealth>>>,
    // 融合周期
    fusion_interval: Duration,
}

impl DataFusionEngine {
    /// 启动融合循环
    pub async fn run(&self) -> Result<(), FusionError> {
        let mut interval = tokio::time::interval(self.fusion_interval);
        loop {
            interval.tick().await;
            let fused = self.fuse_once().await?;
            self.state_tx.send(fused)?;
        }
    }

    /// 单次融合
    async fn fuse_once(&self) -> Result<FusedSystemState, FusionError> {
        // 1. 并行获取所有数据源
        let (intercore_data, weather_data, price_data, dispatch_data, demand_data, lstm_data) =
            tokio::join!(
                self.safe_fetch(&self.intercore),
                self.safe_fetch(&self.weather),
                self.safe_fetch(&self.price),
                self.safe_fetch(&self.dispatch),
                self.safe_fetch(&self.demand_proc),
                self.lstm_provider.get_latest_forecast(),
            );

        // 2. 更新健康监控
        self.update_health("intercore", &intercore_data).await;
        self.update_health("weather", &weather_data).await;
        self.update_health("price", &price_data).await;
        self.update_health("dispatch", &dispatch_data).await;
        self.update_health("demand", &demand_data).await;

        // 3. 使用上一周期值填充缺失字段
        let prev = self.last_state.read().await.clone();

        // 4. 构建融合状态
        let state = self.build_fused_state(
            intercore_data, weather_data, price_data,
            dispatch_data, demand_data, lstm_data, prev,
        );

        // 5. 更新上一状态
        *self.last_state.write().await = Some(state.clone());

        Ok(state)
    }

    /// 安全获取数据（异常不传播）
    async fn safe_fetch(&self, adapter: &Box<dyn DataSourceAdapter>)
        -> Option<DataSourceValue>
    {
        match adapter.fetch_latest().await {
            Ok(val) => Some(val),
            Err(_) => {
                tracing::warn!("数据源 [{}] 获取失败", adapter.name());
                None
            }
        }
    }
}
```

### 3.7 缺失数据处理策略

| 缺失数据源 | 填充策略 | 告警级别 | 降级触发 |
|------------|----------|----------|----------|
| intercore | 使用上一周期值 | WARN (3周期) → ERROR (10周期) | 10周期 → AI降级 |
| LSTM 预测 | 全零向量 | WARN | 不触发 |
| 电价 | 使用默认分时电价表 | WARN (3周期) | 不触发 |
| 气象 | 使用上一周期值 | WARN (10周期) | R_green 置 0 |
| 调度指令 | 置 None | INFO | 不触发 |
| 需量数据 | 使用上周期值 | WARN (3周期) | 不触发 |

---

## 4. 场景分类器设计 ~~→ v2.0 废弃，替换为 ModeSelector~~

> **v2.0 设计变更：** 本章 SceneClassifier 自动分类器已被废弃。运行场景确定方式从"规则引擎自动分类"改为"预设互斥模式选择"。详见 `2026-05-29-MUPC-AI预设运行场景与互斥模式选择-设计文档.md` [DESIGN_APPROVED]。
>
> 以下为 v1.1 原设计内容（保留作为历史参考，实现时请使用 ModeSelector 替代）。

### 4.1 SceneClassifier（已废弃）

**文件：** `mupc/crates/ai-engine/src/scene_classifier.rs`

SceneClassifier 接收 `FusedSystemState`，基于最近 30 分钟的平均负荷特征识别当前运行场景，输出 `SceneRecognitionResult`。

### 4.2 算法选型：规则引擎 + 置信度校准

采用 **规则引擎为主，轻量 ML 为辅助校验** 方案。

**选择理由：**

| 对比维度 | 规则引擎 | 轻量 ML (决策树/逻辑回归) | 深度学习 |
|----------|----------|--------------------------|----------|
| 参数可解释性 | 高，每条规则可审查 | 中 | 低 |
| 部署复杂度 | 零依赖，纯逻辑实现 | 需要额外模型文件 | 需要 NPU 资源 |
| 训练数据需求 | 无需训练 | 中等 (500+ 样本) | 大 (5000+ 样本) |
| 准确率 (5 场景) | ~93-96% (规则覆盖充分时) | ~95-98% | ~97-99% |
| 运行时资源 | <0.1ms, 0 MB | <0.5ms, ~1MB | 10-50ms, ~5MB |
| 热更新能力 | 配置热加载即可 | 需要模型热加载 | 需要模型热加载 |

由于：
1. 5 种场景的特征规则非常清晰（负荷占比、电价时段、VPP 指令等硬边界条件）
2. 规则引擎可以达到 PRD 要求的 >=95% 准确率
3. 运维人员可以直观理解和调整分类规则
4. 零训练数据依赖，部署即用

**采用规则引擎为主方案，同时预留 ML 接口**：
- 主分类器：`RuleBasedClassifier` - 基于确定性规则
- 辅助校验：可选 `MlClassifier` - 用于规则边界模糊时的置信度校准
- 最终输出取两者的加权融合

### 4.3 5 种场景定义

| 场景 ID | 场景名称 | 特征规则 | 典型时段 |
|---------|----------|----------|----------|
| SCENE-01 | 农网灌溉模式 (A) | 灌溉负荷占比 > 60% & 当前月份在灌溉季 (4月~9月) | 4月~9月 |
| SCENE-02 | 工商业模式-自主套利 (B1) | 工商业负荷占比 > 70% & 分时电价在峰时段 | 峰时段 (如 10:00~12:00, 15:00~19:00) |
| SCENE-03 | 工商业模式-需量控制 (B2) | 当前需量 > 需量阈值的 90% & 上月最大需量 > 需量合同值 | 每月最后一周 |
| SCENE-04 | 工商业模式-虚拟电厂 (B3) | VPP 调度指令有效 & 已注册 VPP 服务 | VPP 调度时段 |
| SCENE-05 | 工商业模式-极致绿色 (B5) | 绿色电力消纳比例 < 50% & 碳排强度高于区域均值 | 全天 |

### 4.4 接口定义

```rust
/// 运行场景枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatingScene {
    AgriculturalIrrigation,   // 农网灌溉模式
    CommercialArbitrage,      // 工商业模式-自主套利
    DemandControl,            // 工商业模式-需量控制
    VirtualPowerPlant,        // 工商业模式-虚拟电厂
    UltraGreen,               // 工商业模式-极致绿色
    Default,                  // 未识别/默认
}

/// 场景识别结果
#[derive(Debug, Clone, Serialize)]
pub struct SceneRecognitionResult {
    pub scene: OperatingScene,
    pub confidence: f64,                                    // 置信度 (0.0 ~ 1.0)
    pub scene_probabilities: HashMap<OperatingScene, f64>,  // 各场景概率分布
    pub features_summary: SceneFeatures,                    // 判断依据的特征摘要
    pub timestamp: i64,
}

/// 场景特征输入
#[derive(Debug, Clone)]
pub struct SceneFeatures {
    pub irrigation_load_ratio: f64,     // 灌溉负荷占比 (0.0 ~ 1.0)
    pub commercial_load_ratio: f64,     // 工商业负荷占比 (0.0 ~ 1.0)
    pub demand_ratio: f64,              // 当前需量与需量合同值之比 (0.0 ~ 2.0)
    pub vpp_command_active: bool,       // VPP 调度指令是否有效
    pub pv_consumption_ratio: f64,      // 光伏消纳比例 (0.0 ~ 1.0)
    pub green_energy_ratio: f64,        // 绿色电力消纳比例 (0.0 ~ 1.0)
    pub tariff_period: TariffPeriod,    // 分时电价时段标识
}
```

### 4.5 规则引擎设计

```rust
/// 规则引擎场景分类器
pub struct RuleBasedClassifier {
    /// 规则集（可热加载）
    rules: Vec<SceneRule>,
    /// 场景权重映射
    weight_map: HashMap<OperatingScene, SceneWeights>,
}

/// 场景规则
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneRule {
    pub scene: OperatingScene,
    pub priority: u8,            // 优先级 (越低越优先)
    pub conditions: Vec<RuleCondition>,
    pub min_confidence: f64,     // 最低置信度
}

/// 规则条件
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleCondition {
    IrrigationLoadRatio { operator: Comparison, threshold: f64 },
    CommercialLoadRatio { operator: Comparison, threshold: f64 },
    DemandRatio { operator: Comparison, threshold: f64 },
    VppCommandActive,
    GreenEnergyRatio { operator: Comparison, threshold: f64 },
    IsIrrigationSeason,
    TariffPeriodMatch { periods: Vec<TariffPeriod> },
    PvConsumptionRatio { operator: Comparison, threshold: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Comparison { Gt, Gte, Lt, Lte, Eq }
```

### 4.6 默认规则实现

```rust
impl RuleBasedClassifier {
    pub fn default_rules() -> Vec<SceneRule> {
        vec![
            // SCENE-01: 农网灌溉 (优先级最高)
            SceneRule {
                scene: OperatingScene::AgriculturalIrrigation,
                priority: 1,
                conditions: vec![
                    RuleCondition::IrrigationLoadRatio { operator: Comparison::Gt, threshold: 0.6 },
                    RuleCondition::IsIrrigationSeason,
                ],
                min_confidence: 0.7,
            },
            // SCENE-04: 虚拟电厂 (优先级 2, 指令到达即触发)
            SceneRule {
                scene: OperatingScene::VirtualPowerPlant,
                priority: 2,
                conditions: vec![
                    RuleCondition::VppCommandActive,
                ],
                min_confidence: 0.9,
            },
            // SCENE-03: 需量控制 (优先级 3)
            SceneRule {
                scene: OperatingScene::DemandControl,
                priority: 3,
                conditions: vec![
                    RuleCondition::DemandRatio { operator: Comparison::Gte, threshold: 0.9 },
                ],
                min_confidence: 0.7,
            },
            // SCENE-02: 自主套利 (优先级 4)
            SceneRule {
                scene: OperatingScene::CommercialArbitrage,
                priority: 4,
                conditions: vec![
                    RuleCondition::CommercialLoadRatio { operator: Comparison::Gt, threshold: 0.7 },
                    RuleCondition::TariffPeriodMatch {
                        periods: vec![TariffPeriod::Peak, TariffPeriod::SharpPeak],
                    },
                ],
                min_confidence: 0.7,
            },
            // SCENE-05: 极致绿色 (优先级 5)
            SceneRule {
                scene: OperatingScene::UltraGreen,
                priority: 5,
                conditions: vec![
                    RuleCondition::GreenEnergyRatio { operator: Comparison::Lt, threshold: 0.5 },
                ],
                min_confidence: 0.6,
            },
        ]
    }
}
```

### 4.7 SceneFeatureExtractor

```rust
/// 出线负荷类型配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutletConfig {
    pub outlet_id: String,
    pub load_type: LoadType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoadType {
    Irrigation,
    Commercial,
    Residential,
    Industrial,
}

/// 从 FusedSystemState 提取场景分类特征
pub struct SceneFeatureExtractor {
    /// 滑动窗口大小 (默认 30 个点, 对应 30 秒)
    window_size: usize,
    /// 历史状态缓冲区
    history: VecDeque<FusedSystemState>,
    /// 出线负荷类型配置（从配置文件加载）
    outlet_config: Vec<OutletConfig>,
}

impl SceneFeatureExtractor {
    pub fn new(window_size: usize, outlet_config: Vec<OutletConfig>) -> Self {
        Self {
            window_size,
            history: VecDeque::with_capacity(window_size),
            outlet_config,
        }
    }

    pub fn push_and_extract(&mut self, state: FusedSystemState) -> SceneFeatures {
        self.history.push_back(state);
        while self.history.len() > self.window_size {
            self.history.pop_front();
        }
        self.compute_features()
    }

    fn compute_features(&self) -> SceneFeatures {
        // 负荷分解: 基于出线静态配置标记 + per-outlet 实时负荷
        // 每条出线在配置文件中标注负荷类型 (irrigation/commercial/residential/industrial)
        // 总负荷 = Σ各出线负荷，各类型占比 = Σ该类型出线负荷 / 总负荷
        let n = self.history.len() as f64;
        if n == 0.0 { return SceneFeatures::default(); }

        let total_load: f64 = self.history.iter().map(|s| s.load_power.abs()).sum::<f64>();
        let outlet_count = self.outlet_config.len().max(1) as f64;

        let mut load_by_type: HashMap<&LoadType, f64> = HashMap::new();
        for config in &self.outlet_config {
            *load_by_type.entry(&config.load_type).or_insert(0.0) += total_load / outlet_count;
        }

        let sum_irrigation = load_by_type.get(&LoadType::Irrigation).copied().unwrap_or(0.0);
        let sum_commercial = load_by_type.get(&LoadType::Commercial).copied().unwrap_or(0.0);

        let max_demand = self.history.iter()
            .map(|s| s.current_demand)
            .fold(0.0_f64, f64::max);
        let avg_pv = self.history.iter().map(|s| s.pv_power).sum::<f64>() / n;
        let avg_load = self.history.iter().map(|s| s.load_power).sum::<f64>() / n;

        SceneFeatures {
            irrigation_load_ratio: sum_irrigation / total_load.max(1.0),
            commercial_load_ratio: sum_commercial / total_load.max(1.0),
            demand_ratio: max_demand / self.history.back()
                .map(|s| s.contract_demand.max(1.0)).unwrap_or(1.0),
            vpp_command_active: self.history.back()
                .map(|s| s.dispatch_p_set.is_some()).unwrap_or(false),
            pv_consumption_ratio: (avg_load / avg_pv.max(1.0)).min(1.0),
            green_energy_ratio: self.compute_green_energy_ratio(),
            tariff_period: self.history.back()
                .map(|s| match s.price_tariff_id {
                    0 => TariffPeriod::Valley,
                    1 => TariffPeriod::Flat,
                    2 => TariffPeriod::Peak,
                    3 => TariffPeriod::SharpPeak,
                    _ => TariffPeriod::Flat,
                }).unwrap_or(TariffPeriod::Flat),
            current_month: chrono_now().month(),
        }
    }
}
```

### 4.8 场景分类器主模块

```rust
/// 场景分类器
pub struct SceneClassifier {
    feature_extractor: Arc<RwLock<SceneFeatureExtractor>>,
    rule_classifier: RuleBasedClassifier,
    ml_classifier: Option<MlClassifier>,  // 可选 ML 辅助
    current_scene: Arc<RwLock<SceneRecognitionResult>>,
    manual_override: Arc<RwLock<Option<ManualOverride>>>,
    change_tx: broadcast::Sender<SceneRecognitionResult>,
    oscillation_lock: Arc<RwLock<Option<Instant>>>,
}

/// 手动覆盖
struct ManualOverride {
    scene: OperatingScene,
    expires_at: Instant,
}

impl SceneClassifier {
    /// 单次场景识别（每次融合后调用）
    pub async fn recognize(&self, state: &FusedSystemState) -> SceneRecognitionResult {
        // 1. 检查手动覆盖
        if let Some(manual) = self.manual_override.read().await.as_ref() {
            if manual.expires_at > Instant::now() {
                return SceneRecognitionResult {
                    scene: manual.scene,
                    confidence: 1.0,
                    scene_probabilities: HashMap::new(),
                    features_summary: SceneFeatures::default(),
                    timestamp: chrono_now().timestamp_millis(),
                };
            }
        }

        // 2. 提取特征
        let features = self.feature_extractor.write().await
            .push_and_extract(state.clone());

        // 3. 规则引擎分类
        let mut rule_result = self.rule_classifier.classify(&features);

        // 4. 如果配置了 ML 辅助，进行加权融合
        if let Some(ref ml) = self.ml_classifier {
            let ml_result = ml.classify(&features);
            rule_result = self.fuse_results(rule_result, ml_result);
        }

        // 5. 振荡检测
        self.check_oscillation(&rule_result).await;

        // 6. 更新当前场景
        *self.current_scene.write().await = rule_result.clone();

        rule_result
    }

    /// 振荡检测：5分钟内切换 >= 3 次则锁定当前场景 30 分钟
    async fn check_oscillation(&self, result: &SceneRecognitionResult) {
        // 实现逻辑见边界条件处理
    }
}
```

### 4.9 边界条件处理

| 条件 | 处理逻辑 | 输出场景 |
|------|----------|----------|
| 所有规则不命中 | `confidence < 0.4` | `Default` |
| 最高置信度 < 0.6 | 切换至 Default | `Default` |
| 高频振荡 (5min >= 3 次) | 锁定当前场景 30 分钟 | 当前场景 |
| 手动覆盖到期 | Default 模式运行 5 分钟后再自动识别 | `Default` → 自动 |

---

## 5. 强化学习模型设计

### 5.1 RLModel

**文件：** `mupc/crates/ai-engine/src/rl_model.rs`

RLModel 使用 MADDPG（多智能体深度确定性策略梯度）或 PPO（近端策略优化）算法，基于融合状态、LSTM 预测值和场景标签，输出 7 维动作空间的最优控制指令。

### 5.2 状态空间定义（7 大类，23 个字段）

| 大类 | 字段名 | 类型 | 范围 | 单位 | 来源 |
|------|--------|------|------|------|------|
| **D1-实时 (6)** | battery_soc | f64 | [0.0, 1.0] | - | intercore |
| | pv_power | f64 | [-1000.0, 1000.0] | kW | intercore |
| | load_power | f64 | [-1000.0, 1000.0] | kW | intercore |
| | grid_power | f64 | [-1000.0, 1000.0] | kW | intercore |
| | transformer_load | f64 | [0.0, 2.0] | - | intercore |
| | battery_power | f64 | [-500.0, 500.0] | kW | intercore |
| **D2-预测 (2x15)** | pv_forecast_15min | Vec<f64>(15) | [-1000.0, 1000.0] | kW | LSTM |
| | load_forecast_15min | Vec<f64>(15) | [-1000.0, 1000.0] | kW | LSTM |
| **D3-电价 (3)** | current_electricity_price | f64 | [0.0, 2.0] | 元/kWh | 物联平台 |
| | next_period_price | f64 | [0.0, 2.0] | 元/kWh | 物联平台 |
| | price_tariff_id | u8 | {0=谷,1=平,2=峰,3=尖峰} | 枚举 | 物联平台 |
| **D4-需量 (3)** | current_demand | f64 | [0.0, 10000.0] | kW | intercore |
| | contract_demand | f64 | [0.0, 10000.0] | kW | 配置 |
| | peak_demand_this_month | f64 | [0.0, 10000.0] | kW | data-processing |
| **D5-电能质量 (5)** | voltage_phase_a/b/c | f64 | [0.8, 1.2] | p.u. | intercore |
| | voltage_unbalance | f64 | [0.0, 0.05] | - | intercore |
| | frequency | f64 | [49.5, 50.5] | Hz | intercore |
| **D6-气象 (2)** | solar_irradiance | f64 | [0.0, 1500.0] | W/m^2 | 气象 API |
| | temperature | f64 | [-20.0, 60.0] | deg C | 气象 API |
| **D7-调度 (2)** | dispatch_p_set | Option<f64> | [-1000.0, 1000.0] | kW | gateway |
| | dispatch_q_set | Option<f64> | [-1000.0, 1000.0] | kVar | gateway |

**序列化输入向量维度：** 50 维

### 5.3 动作空间定义（7 个动作维度）

| 维度 | 字段名 | 类型 | 范围 | 单位 | 说明 |
|------|--------|------|------|------|------|
| A1 | p_batt_set | f64 | [-500.0, 500.0] | kW | 电池有功功率（负值=充电）|
| A2 | q_batt_set | f64 | [-300.0, 300.0] | kVar | 无功功率（负值=感性）|
| A3a | compens_factor_a | f64 | [-1.0, 1.0] | - | A 相补偿系数 |
| A3b | compens_factor_b | f64 | [-1.0, 1.0] | - | B 相补偿系数 |
| A3c | compens_factor_c | f64 | [-1.0, 1.0] | - | C 相补偿系数 |
| A4 | load_shedding | f64 | [0.0, 500.0] | kW | 可中断负荷切除 |
| A5 | pv_limit | f64 | [0.0, 1.0] | - | 光伏限功率比例 |
| - | confidence | f64 | [0.0, 1.0] | - | 决策置信度 |

### 5.4 RLModel 接口（扩展后）

```rust
/// 扩展后的 RL 模型
pub struct RLModel {
    config: RlConfig,
    runtime: RknnRuntime,
    input_dim: usize,    // = 50
    output_dim: usize,   // = 7 + 1 (7个动作 + confidence)
}

impl RLModel {
    /// 使用完整融合状态进行决策
    pub async fn decide_fused(&self, state: &FusedSystemState) -> Result<ActionOutput, AiEngineError> {
        if !self.runtime.is_loaded().await {
            return Err(AiEngineError::ModelNotLoaded);
        }

        // 1. 序列化为 50 维输入向量
        let input = state.to_input_vector();
        debug_assert!(input.len() == self.input_dim);

        // 2. NPU 推理
        let output = self.runtime.run(&input).await?;

        // 3. 解析 8 维输出
        self.parse_action_output(&output, state)
    }

    /// 解析动作输出，考虑调度指令约束
    fn parse_action_output(&self, raw: &[f32], state: &FusedSystemState) -> Result<ActionOutput, AiEngineError> {
        let mut action = ActionOutput {
            p_batt_set:         raw.get(0).copied().unwrap_or(0.0) as f64,
            q_batt_set:         raw.get(1).copied().unwrap_or(0.0) as f64,
            compens_factor_a:   raw.get(2).copied().unwrap_or(0.0) as f64,
            compens_factor_b:   raw.get(3).copied().unwrap_or(0.0) as f64,
            compens_factor_c:   raw.get(4).copied().unwrap_or(0.0) as f64,
            load_shedding:      raw.get(5).copied().unwrap_or(0.0) as f64,
            pv_limit:           raw.get(6).copied().unwrap_or(1.0) as f64,
            confidence:         raw.get(7).copied().unwrap_or(0.5) as f64,
        };

        // 应用调度指令约束
        if let Some(p_set) = state.dispatch_p_set {
            action.p_batt_set = action.p_batt_set.clamp(-p_set.abs(), p_set.abs());
        }

        Ok(action)
    }
}
```

### 5.5 动作输出完整定义

```rust
/// RL 决策输出（完整动作空间）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOutput {
    /// 电池有功功率设定值 (kW), [-500.0, 500.0], 负值=充电
    pub p_batt_set: f64,
    /// 可中断负荷切除量 (kW), [0.0, 500.0]
    pub load_shedding: f64,
    /// 光伏限功率比例, [0.0, 1.0], 0.0=完全限功率
    pub pv_limit: f64,
    /// 决策置信度 [0.0, 1.0]
    pub confidence: f64,
    /// 装置无功功率设定值 (kVar), [-300.0, 300.0], 负值=感性
    pub q_batt_set: f64,
    /// A 相分相补偿系数, [-1.0, 1.0]
    pub compens_factor_a: f64,
    /// B 相分相补偿系数, [-1.0, 1.0]
    pub compens_factor_b: f64,
    /// C 相分相补偿系数, [-1.0, 1.0]
    pub compens_factor_c: f64,
}
```

### 5.6 动作约束校验（ActionValidator）

**文件：** `mupc/crates/ai-engine/src/action_validator.rs`

#### 6 条约束规则

| 规则 ID | 约束条件 | 默认值 | 说明 |
|---------|----------|--------|------|
| ACT-01 | p_batt_set 变化率 <= 50 kW/周期 | 50 kW/s | 防止电池功率突变 |
| ACT-02 | q_batt_set 变化率 <= 30 kVar/周期 | 30 kVar/s | 防止无功突变 |
| ACT-03 | sqrt(P^2 + Q^2) <= S_max | 500 kVA | 功率圆限制 |
| ACT-04 | compens_factor_a + b + c = 0 | - | 三相补偿仅调节不平衡 |
| ACT-05 | pv_limit >= 0.1（防逆流场景除外） | 0.1 | 光伏限功率下限 |
| ACT-06 | p_batt_set 绝对值 <= dispatch_p_set | - | 调度指令权限 |

```rust
/// 动作约束校验器
pub struct ActionValidator {
    config: ActionConstraintConfig,
}

/// 约束配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionConstraintConfig {
    pub s_max: f64,                  // 视在功率上限 (kVA), 默认 500.0
    pub p_batt_rate_limit: f64,      // 有功变化率上限 (kW/周期), 默认 50.0
    pub q_batt_rate_limit: f64,      // 无功变化率上限 (kVar/周期), 默认 30.0
    pub pv_limit_min: f64,           // PV 限功率下限, 默认 0.1
    pub last_action: RwLock<Option<ActionOutput>>,
}

impl ActionValidator {
    pub fn validate(
        &self,
        action: &mut ActionOutput,
        state: &FusedSystemState,
        scene: Option<OperatingScene>,
    ) -> ValidationReport {
        let mut report = ValidationReport::new();
        report.is_anti_reverse = scene == Some(OperatingScene::UltraGreen);

        let last = self.last_action.read().unwrap();

        // ACT-01: p_batt_set 变化率限制
        if let Some(ref last_a) = *last {
            let delta_p = (action.p_batt_set - last_a.p_batt_set).abs();
            if delta_p > self.config.p_batt_rate_limit {
                // clamp 操作
                let clamped = if action.p_batt_set > last_a.p_batt_set {
                    last_a.p_batt_set + self.config.p_batt_rate_limit
                } else {
                    last_a.p_batt_set - self.config.p_batt_rate_limit
                };
                action.p_batt_set = clamped;
                report.add_violation("ACT-01", format!("有功变化率 {:.1} 超过上限 {:.1}", delta_p, self.config.p_batt_rate_limit));
            }
        }

        // ACT-02: q_batt_set 变化率限制
        if let Some(ref last_a) = *last {
            let delta_q = (action.q_batt_set - last_a.q_batt_set).abs();
            if delta_q > self.config.q_batt_rate_limit {
                let clamped = if action.q_batt_set > last_a.q_batt_set {
                    last_a.q_batt_set + self.config.q_batt_rate_limit
                } else {
                    last_a.q_batt_set - self.config.q_batt_rate_limit
                };
                action.q_batt_set = clamped;
                report.add_violation("ACT-02", format!("无功变化率 {:.1} 超过上限 {:.1}", delta_q, self.config.q_batt_rate_limit));
            }
        }

        // ACT-03: 视在功率圆约束
        let s = (action.p_batt_set.powi(2) + action.q_batt_set.powi(2)).sqrt();
        if s > self.config.s_max {
            let scale = self.config.s_max / s;
            action.p_batt_set *= scale;
            action.q_batt_set *= scale;
            report.add_violation("ACT-03", format!("视在功率 {:.1} 超过上限 {:.1}", s, self.config.s_max));
        }

        // ACT-04: 三相补偿系数和为 0
        let sum = action.compens_factor_a + action.compens_factor_b + action.compens_factor_c;
        if sum.abs() > 1e-6 {
            let offset = sum / 3.0;
            action.compens_factor_a -= offset;
            action.compens_factor_b -= offset;
            action.compens_factor_c -= offset;
            report.add_violation("ACT-04", "三相补偿系数和 != 0, 自动归零");
        }

        // ACT-05: pv_limit >= 0.1 (防逆流场景除外)
        if action.pv_limit < self.config.pv_limit_min {
            if !report.is_anti_reverse {
                action.pv_limit = self.config.pv_limit_min;
                report.add_violation("ACT-05", format!("pv_limit {:.3} 低于下限 {:.3}", action.pv_limit, self.config.pv_limit_min));
            }
        }

        // ACT-06: 调度指令权限约束
        if let Some(p_set) = state.dispatch_p_set {
            if action.p_batt_set.abs() > p_set.abs() {
                action.p_batt_set = action.p_batt_set.clamp(-p_set.abs(), p_set.abs());
                report.add_violation("ACT-06", "有功设定超过调度指令约束");
            }
        }

        // 最终值域 clamp
        action.p_batt_set = action.p_batt_set.clamp(-500.0, 500.0);
        action.q_batt_set = action.q_batt_set.clamp(-300.0, 300.0);
        action.compens_factor_a = action.compens_factor_a.clamp(-1.0, 1.0);
        action.compens_factor_b = action.compens_factor_b.clamp(-1.0, 1.0);
        action.compens_factor_c = action.compens_factor_c.clamp(-1.0, 1.0);
        action.load_shedding = action.load_shedding.clamp(0.0, 500.0);
        action.pv_limit = action.pv_limit.clamp(0.0, 1.0);
        action.confidence = action.confidence.clamp(0.0, 1.0);

        *self.last_action.write().unwrap() = Some(action.clone());
        report
    }
}

/// 校验报告
#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub violations: Vec<ConstraintViolation>,
    pub is_anti_reverse: bool,
    pub total_delay_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConstraintViolation {
    pub rule_id: String,
    pub message: String,
}
```

---

## 6. 奖励函数计算模块

### 6.1 RewardCalculator

**文件：** `mupc/crates/ai-engine/src/reward_calculator.rs`

根据当前场景选择对应的奖励函数模块进行计算，输出奖励值用于在线微调。

### 6.2 模块化架构

```rust
/// 奖励函数计算器
pub struct RewardCalculator {
    reward_fns: HashMap<OperatingScene, Box<dyn SceneRewardFunction>>,
    weights: Arc<RwLock<WeightConfig>>,
}

/// 场景奖励函数接口
#[async_trait]
pub trait SceneRewardFunction: Send + Sync {
    fn scene(&self) -> OperatingScene;
    async fn calculate(&self, params: &RewardParams) -> f64;
    fn config(&self) -> RewardFnConfig;
}

/// 奖励计算参数
#[derive(Debug, Clone)]
pub struct RewardParams<'a> {
    pub current_state: &'a FusedSystemState,
    pub previous_state: &'a FusedSystemState,
    pub action: &'a ActionOutput,
    pub scene: OperatingScene,
    pub weights: &'a SceneWeights,
    pub timestamp: i64,
}

/// 各场景切片的奖励值
#[derive(Debug, Clone, Serialize)]
pub struct SceneRewardValue {
    pub scene: OperatingScene,
    pub total: f64,
    pub components: HashMap<String, f64>,
    pub weights: SceneWeights,
    pub timestamp: i64,
}
```

### 6.3 SCENE-01: 农网灌溉

**目标：** 最大化光伏消纳 + 电压治理，最小化变压器过载

```
R_agri = w1 * R_pv_consumption + w2 * R_voltage_quality - w3 * P_battery_degradation - w4 * P_transformer_overload

R_pv_consumption = min(P_self_consume / P_total, 1.0) * 100
R_voltage_quality = 100 * max(0, 1 - |V_a-1.0|/0.1 - |V_b-1.0|/0.1 - |V_c-1.0|/0.1)
P_battery_degradation = alpha * |delta_SOC| / SOC_range * 100
P_transformer_overload = 200 * max(0, L_transformer - 1.0)
```

| 权重 | 默认值 | 说明 |
|------|--------|------|
| w1 (primary_reward) | 1.0 | 光伏消纳奖励权重 |
| w2 (secondary_reward) | 1.0 | 电压质量奖励权重 |
| w3 (degradation_penalty) | 0.5 | 电池损耗惩罚 |
| w4 (overload_penalty) | 2.0 | 变压器过载惩罚 |

### 6.4 SCENE-B1: 自主套利

**目标：** 最大化峰谷电价差收益，最小化电池损耗

```
R_arbitrage = w1 * R_price_spread - w2 * P_battery_degradation

R_price_spread = P_batt * delta_t * (price_sell - price_buy) * conversion_factor
P_battery_degradation = beta * |P_batt| * delta_t / E_total * 100
```

| 权重 | 默认值 | 说明 |
|------|--------|------|
| w1 (primary_reward) | 1.0 | 电价差收益权重 |
| w2 (degradation_penalty) | 1.0 | 电池损耗惩罚权重 |

### 6.5 SCENE-B2: 需量控制

**目标：** 减免需量罚金

```
R_demand = w1 * R_demand_penalty_avoidance - w2 * P_comfort_loss

R_demand_penalty_avoidance = max(0, D_peak_baseline - D_peak_actual) * penalty_rate
P_comfort_loss = gamma * P_load_shed * delta_t * price_loss
```

| 权重 | 默认值 | 说明 |
|------|--------|------|
| w1 (primary_reward) | 1.0 | 需量罚金减免权重 |
| w2 (overload_penalty) | 0.5 | 舒适度损失惩罚 |

### 6.6 SCENE-B3: 虚拟电厂

**目标：** 最大化辅助服务收益 + 响应精度

```
R_vpp = w1 * R_ancillary_service + w2 * R_response_accuracy - w3 * P_deadline_deviation

R_ancillary_service = P_reg * capacity_price
R_response_accuracy = 100 * max(0, 1 - |P_actual - P_target| / P_target_range)
P_deadline_deviation = delta_t_response / T_allowed * 100
```

| 权重 | 默认值 | 说明 |
|------|--------|------|
| w1 (primary_reward) | 1.0 | 辅助服务收益权重 |
| w2 (secondary_reward) | 2.0 | 响应精度权重（VPP 考核重点）|
| w3 (degradation_penalty) | 1.0 | 响应延迟惩罚 |

### 6.7 SCENE-B5: 极致绿色

**目标：** 最大化绿电消纳比例，最小化碳排放

```
R_green = w1 * R_green_consumption + w2 * R_carbon_reduction

R_green_consumption = 100 * E_green / E_total
R_carbon_reduction = 100 * (C_baseline - C_actual) / C_baseline
```

| 权重 | 默认值 | 说明 |
|------|--------|------|
| w1 (primary_reward) | 1.0 | 绿电消纳比例权重 |
| w2 (secondary_reward) | 1.0 | 碳减排量权重 |

### 6.8 场景-权重映射表

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneWeights {
    pub primary_reward: f64,         // 主目标奖励权重
    pub secondary_reward: f64,       // 次目标奖励权重
    pub degradation_penalty: f64,    // 电池衰减惩罚权重
    pub overload_penalty: f64,       // 过载/切负荷惩罚权重
}

pub fn default_scene_weights() -> HashMap<OperatingScene, SceneWeights> {
    let mut m = HashMap::new();
    m.insert(OperatingScene::AgriculturalIrrigation, SceneWeights { primary_reward: 1.0, secondary_reward: 1.0, degradation_penalty: 0.5, overload_penalty: 2.0 });
    m.insert(OperatingScene::CommercialArbitrage,  SceneWeights { primary_reward: 1.0, secondary_reward: 0.0, degradation_penalty: 1.0, overload_penalty: 0.0 });
    m.insert(OperatingScene::DemandControl,        SceneWeights { primary_reward: 1.0, secondary_reward: 0.0, degradation_penalty: 0.0, overload_penalty: 0.5 });
    m.insert(OperatingScene::VirtualPowerPlant,    SceneWeights { primary_reward: 1.0, secondary_reward: 2.0, degradation_penalty: 1.0, overload_penalty: 0.0 });
    m.insert(OperatingScene::UltraGreen,           SceneWeights { primary_reward: 1.0, secondary_reward: 1.0, degradation_penalty: 0.0, overload_penalty: 0.0 });
    m.insert(OperatingScene::Default,              SceneWeights { primary_reward: 1.0, secondary_reward: 1.0, degradation_penalty: 0.5, overload_penalty: 0.5 });
    m
}
```

---

## 7. RKNN Runtime 设计

### 7.1 概述

RKNN Runtime 是 Rockchip 提供的 NPU 推理引擎，通过 FFI 调用 `librknnrt.so` C 库，在 RK3588 NPU 上执行 INT8/FP16 量化模型推理。所有 FFI 调用使用 `tokio::task::spawn_blocking` 在后台线程执行，不阻塞 Tokio async runtime。

### 7.2 架构位置

```
┌─────────────────────────────────────────────────────────────┐
│                     MUPC AI Engine                          │
├─────────────────────────────────────────────────────────────┤
│  rknn_runtime.rs (高层接口)                                  │
│       │                                                     │
│       ▼                                                     │
│  rknn_runtime_sys.rs (FFI 绑定)  ←─────────────────────────│
│       │                        librknnrt.so (C 库)           │
└───────┼─────────────────────────────────────────────────────┘
        │
        ▼
┌──────────────────┐
│   RK3588 NPU     │
└──────────────────┘
```

### 7.3 模块结构

```
mupc/crates/ai-engine/src/
├── rknn_runtime.rs      # 高层接口（线程安全、异步封装）
├── rknn_runtime_sys.rs  # C API 绑定（FFI extern 声明）
├── rknn_types.rs        # 类型定义（Rust 原生封装）
└── error.rs             # 错误类型
```

### 7.4 FFI 绑定

```rust
// rknn_runtime_sys.rs

use std::os::raw::{c_char, c_int, c_void};

#[repr(C)]
pub struct rknn_input {
    pub index: u32,
    pub buf: *mut c_void,
    pub size: u32,
    pub pass_timestamp: c_int,
}

#[repr(C)]
pub struct rknn_output {
    pub buf: *mut c_void,
    pub size: u32,
    pub is_preallocated: c_int,
}

#[link(name = "rknnrt")]
extern "C" {
    pub fn rknn_init(ctx: *mut u64, model_path: *const c_char, model_type: c_int, flag: c_int) -> c_int;
    pub fn rknn_inputs_set(ctx: u64, n: u32, inputs: *mut rknn_input) -> c_int;
    pub fn rknn_run(ctx: u64, reserved: *mut u64) -> c_int;
    pub fn rknn_outputs_get(ctx: u64, n: u32, outputs: *mut rknn_output) -> c_int;
    pub fn rknn_destroy(ctx: u64) -> c_int;
    pub fn rknn_query(ctx: u64, cmd: c_int, info: *mut c_void, size: u32) -> c_int;
}
```

### 7.5 类型定义

```rust
// rknn_types.rs

/// RKNN 输入张量
#[derive(Debug, Clone)]
pub struct RknnInput {
    pub index: u32,
    pub buf: Vec<u8>,
    pub pass_timestamp: c_int,
}

/// RKNN 输出张量
#[derive(Debug)]
pub struct RknnOutput {
    pub buf: Vec<u8>,
}

impl RknnOutput {
    /// 安全地将输出缓冲区转换为 f32 数组
    pub fn as_f32(&self) -> Vec<f32> {
        let (prefix, aligned, suffix) = self.buf.align_to::<f32>();
        let mut result = Vec::with_capacity(
            prefix.len() / 4 + aligned.len() + suffix.len() / 4
        );
        for chunk in prefix.chunks_exact(4) {
            result.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        result.extend(aligned.iter());
        for chunk in suffix.chunks_exact(4) {
            result.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        result
    }
}
```

### 7.6 高层接口（异步封装 + RAII）

```rust
// rknn_runtime.rs

/// RKNN 上下文（RAII 资源管理）
struct RknnContext {
    ctx: u64,
    input_count: u32,
    output_count: u32,
}

impl Drop for RknnContext {
    fn drop(&mut self) {
        unsafe { rknn_runtime_sys::rknn_destroy(self.ctx) }
    }
}

/// RKNN Runtime 推理器
pub struct RknnRuntime {
    model_path: std::path::PathBuf,
    ctx: Arc<RwLock<Option<RknnContext>>>,
    // 预分配缓冲区（优化用）
    output_buffer: RwLock<Vec<f32>>,
    input_tensor: RwLock<rknn_input>,
    output_tensor: RwLock<rknn_output>,
}

impl RknnRuntime {
    /// 创建推理器
    pub fn new(model_path: &Path) -> Result<Self, AiEngineError>;

    /// 加载模型（异步）
    pub async fn load(&self) -> Result<(), AiEngineError>;

    /// 执行推理（异步）
    pub async fn run(&self, input: &[f32]) -> Result<Vec<f32>, AiEngineError>;

    /// 释放资源（异步）
    pub async fn destroy(&self) -> Result<(), AiEngineError>;
}

// Safety: Send + Sync 实现
unsafe impl Send for RknnRuntime {}
unsafe impl Sync for RknnRuntime {}
```

### 7.7 错误码映射

```rust
fn map_rknn_error(code: c_int) -> Result<(), AiEngineError> {
    match code {
        0 => Ok(()),
        -1 => Err(AiEngineError::ModelLoadFailed("初始化失败".into())),
        -2 => Err(AiEngineError::ModelLoadFailed("模型格式错误".into())),
        -3 => Err(AiEngineError::ModelLoadFailed("模型不符合框架要求".into())),
        -4 => Err(AiEngineError::ModelLoadFailed("SDK 版本不匹配".into())),
        -5 => Err(AiEngineError::InferenceFailed("输入数量不匹配".into())),
        -6 => Err(AiEngineError::InferenceFailed("输出数量不匹配".into())),
        -7 => Err(AiEngineError::InferenceFailed("输入格式错误".into())),
        -8 => Err(AiEngineError::InferenceFailed("输出格式错误".into())),
        -9 => Err(AiEngineError::InferenceFailed("推理超时".into())),
        -10 => Err(AiEngineError::InferenceFailed("上下文无效".into())),
        _ => Err(AiEngineError::InferenceFailed(format!("未知错误: {}", code))),
    }
}
```

### 7.8 NPU 推理降级

```rust
pub enum InferenceBackend {
    Npu(RknnRuntime),
    Cpu(TractRuntime),    // 降级后端
}

pub struct FallbackRuntime {
    primary: RknnRuntime,
    fallback: Option<TractRuntime>,
    backend: Arc<RwLock<InferenceBackend>>,
}

impl FallbackRuntime {
    pub async fn run(&self, input: &[f32]) -> Result<Vec<f32>, AiEngineError> {
        let backend = self.backend.read().await;
        match &*backend {
            InferenceBackend::Npu(npu) => {
                match npu.run(input).await {
                    Ok(output) => Ok(output),
                    Err(e) => {
                        drop(backend);
                        self.fallback_to_cpu(input).await
                    }
                }
            }
            InferenceBackend::Cpu(cpu) => {
                cpu.run(input).await
            }
        }
    }

    async fn fallback_to_cpu(&self, input: &[f32]) -> Result<Vec<f32>, AiEngineError> {
        tracing::warn!("NPU 推理失败，降级至 CPU 推理");
        *self.backend.write().await = InferenceBackend::Cpu(
            self.fallback.clone().unwrap()
        );
        // CPU 推理 ...
    }
}
```

### 7.9 NPU 温度监控

```rust
pub struct NpuThermalMonitor {
    temp_path: PathBuf,           // /sys/class/thermal/thermal_zone*/temp
    throttle_threshold: f64,      // 85°C
}

impl NpuThermalMonitor {
    pub fn new() -> Self {
        Self {
            temp_path: PathBuf::from("/sys/class/thermal/thermal_zone1/temp"),
            throttle_threshold: 85.0,
        }
    }

    pub fn read_temperature(&self) -> Result<f64, AiEngineError> {
        let content = std::fs::read_to_string(&self.temp_path)
            .map_err(|e| AiEngineError::InferenceFailed(e.to_string()))?;
        let millidegrees: f64 = content.trim().parse().unwrap_or(0.0);
        Ok(millidegrees / 1000.0)
    }

    pub fn is_throttled(&self) -> bool {
        self.read_temperature().unwrap_or(0.0) >= self.throttle_threshold
    }
}
```

### 7.10 专用推理线程架构（优化方向）

```rust
/// 专用推理线程架构
pub struct RknnRuntimeAsync {
    cmd_tx: mpsc::Sender<InferenceCommand>,
    result_rx: mpsc::Receiver<InferenceResult>,
}

enum InferenceCommand {
    Run(Vec<f32>),
    Load(PathBuf),
    Destroy,
}
```

### 7.11 NPU 推理优化措施

1. **模型量化优化**：混合量化策略，对敏感层保留 FP16，其余层 INT8；输入归一化融合到模型第一层
2. **零拷贝输入输出**：预分配缓冲区，避免每次 run 时动态分配
3. **NPU 核心独占**：绑定 AI 推理线程到 Cortex-A76 大核，设置 `SCHED_FIFO` 实时调度优先级
4. **异步推理免锁**：维护专用推理线程，通过通道传递请求，避免每次 `spawn_blocking`
5. **推理失败降级**：NPU 失败自动重试 1 次，仍失败则切换至 CPU 推理，降级延迟 < 5s

---

## 8. ModelManager 统一调度设计

### 8.1 ModelManager 结构

**文件：** `mupc/crates/ai-engine/src/model_manager.rs`

```rust
/// 模型状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelStatus {
    Unloaded,
    Loading,
    Ready,
    Error,
}

/// 模型管理器
pub struct ModelManager {
    config: AiEngineConfig,
    lstm_model: Arc<RwLock<Option<LstmModel>>>,
    rl_model: Arc<RwLock<Option<RLModel>>>,
    // v2.0: scene_classifier → mode_selector
    mode_selector: Arc<ModeSelector>,
    data_fusion: Arc<RwLock<Option<DataFusionEngine>>>,
    reward_calculator: Arc<RwLock<Option<RewardCalculator>>>,
    action_validator: Arc<RwLock<Option<ActionValidator>>>,
    // 运行时后端
    inference_backend: Arc<RwLock<FallbackRuntime>>,
    // 状态
    status: Arc<RwLock<ModelStatus>>,
    current_scene: Arc<RwLock<SceneRecognitionResult>>,
    // 上一周期状态（用于 delta_SOC 等差分奖励计算）
    previous_state: Arc<RwLock<Option<FusedSystemState>>>,
}
```

### 8.2 完整决策周期

```rust
/// 决策周期结果
#[derive(Debug, Clone, Serialize)]
pub struct DecisionCycleResult {
    pub fused_state: FusedSystemState,
    pub scene: SceneRecognitionResult,
    pub action: ActionOutput,
    pub validation: ValidationReport,
    pub reward: Option<SceneRewardValue>,
    pub cycle_duration_us: u64,
}

impl ModelManager {
    /// 完整决策流程（含场景识别 + 融合 + 推理 + 校验）
    pub async fn full_decision_cycle(&self) -> Result<DecisionCycleResult, AiEngineError> {
        let start = Instant::now();

        // 1. 数据融合
        let fused_state = self.data_fusion.read().await
            .as_ref().ok_or(AiEngineError::ModelNotLoaded)?
            .fuse_once().await?;

        // 2. 场景识别
        let scene = self.scene_classifier.read().await
            .as_ref().ok_or(AiEngineError::ModelNotLoaded)?
            .recognize(&fused_state).await;

        // 3. 更新权重
        let weights = self.reward_calculator.read().await
            .as_ref().map(|rc| rc.get_weights(scene.scene));

        // 4. RL 决策
        let raw_action = self.rl_model.read().await
            .as_ref().ok_or(AiEngineError::ModelNotLoaded)?
            .decide_fused(&fused_state).await?;

        // 5. 动作校验
        let mut validated_action = raw_action.clone();
        let validation = self.action_validator.read().await
            .as_ref().ok_or(AiEngineError::ModelNotLoaded)?
            .validate(&mut validated_action, &fused_state, Some(scene.scene));

        // 6. 奖励计算
        let reward_val = if let Some(ref rc) = *self.reward_calculator.read().await {
            let prev = self.previous_state.read().await.clone()
                .unwrap_or_else(|| fused_state.clone());
            let params = RewardParams {
                current_state: &fused_state,
                previous_state: &prev,
                action: &validated_action,
                scene: scene.scene,
                weights: &weights.unwrap_or_default(),
                timestamp: fused_state.timestamp,
            };
            Some(rc.calculate(&params).await)
        } else { None };

        // 保存上一周期状态
        *self.previous_state.write().await = Some(fused_state.clone());
        *self.current_scene.write().await = scene.clone();

        let elapsed = start.elapsed();

        Ok(DecisionCycleResult {
            fused_state,
            scene,
            action: validated_action,
            validation,
            reward: reward_val,
            cycle_duration_us: elapsed.as_micros() as u64,
        })
    }

    /// 加载所有模型
    pub async fn load_models(&self) -> Result<(), AiEngineError> { ... }

    /// 预测（LSTM）
    pub async fn predict(&self, input: &LstmInput) -> Result<LstmOutput, AiEngineError> { ... }

    /// 决策（RL）
    pub async fn decide(&self, state: &SystemState) -> Result<ActionOutput, AiEngineError> { ... }

    /// 获取状态
    pub async fn get_status(&self) -> ModelStatus { ... }
}
```

---

## 9. 与策略引擎集成设计

### 9.1 AiIntegrator

**文件：** `mupc/crates/strategy-engine/src/ai_integration.rs`

```rust
use mupc_ai_engine::{
    FusedSystemState, SceneRecognitionResult, SceneRewardValue,
    ActionValidator, ValidationReport, DecisionCycleResult,
};

/// AI 集成器
pub struct AiIntegrator {
    model_manager: Arc<RwLock<Option<ModelManager>>>,
    status: Arc<RwLock<ModelStatus>>,
    scene_rx: broadcast::Receiver<SceneRecognitionResult>,
    context: Arc<RwLock<AiIntegrationContext>>,
}

#[derive(Debug, Clone)]
pub struct AiIntegrationContext {
    pub current_scene: SceneRecognitionResult,
    pub last_action: Option<ActionOutput>,
    pub last_reward: Option<f64>,
    pub cycle_count: u64,
    pub last_cycle_duration_us: u64,
}

impl AiIntegrator {
    /// 完整决策周期（供 strategy-engine 主循环调用）
    pub async fn run_full_cycle(&self) -> Result<DecisionCycleResult, AiEngineError> {
        let manager = self.model_manager.read().await;
        let manager = manager.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;
        let result = manager.full_decision_cycle().await?;

        let mut ctx = self.context.write().await;
        ctx.current_scene = result.scene.clone();
        ctx.last_action = Some(result.action.clone());
        ctx.last_reward = result.reward;
        ctx.cycle_count += 1;
        ctx.last_cycle_duration_us = result.cycle_duration_us;

        self.notify_scene_change(&result.scene).await;
        Ok(result)
    }

    /// 链式校验: ActionValidator + AiCommandValidator
    pub async fn validate_action(
        &self, action: &ActionOutput, state: &FusedSystemState,
    ) -> (mupc_engine::ValidationResult, mupc_ai_engine::ValidationReport) {
        // 1. ai-engine 的 ActionValidator (物理约束)
        let action_validator = self.model_manager.read().await
            .as_ref().unwrap().action_validator();
        let mut action_clone = action.clone();
        let scene = self.context.read().await.current_scene.scene;
        let ai_report = action_validator.validate(&mut action_clone, state, Some(scene));

        // 2. strategy-engine 的 AiCommandValidator (策略约束)
        let cmd = self.action_to_command(&action_clone);
        let strategy_result = self.cmd_validator.validate(&cmd).await;

        (strategy_result, ai_report)
    }
}
```

### 9.2 AiCommandValidator 扩展

**文件：** `mupc/crates/strategy-engine/src/ai_validator.rs`

```rust
pub struct AiCommandValidatorImpl {
    model: Option<Box<dyn AiModel>>,
    action_validator: Option<Arc<ActionValidator>>,
}

impl AiCommandValidatorImpl {
    /// 增强校验: 组合策略约束 + 物理约束
    pub async fn validate_enhanced(
        &self, cmd: &ControlCommand, state: &FusedSystemState, scene: Option<OperatingScene>,
    ) -> ValidationResult {
        // 1. 策略级校验
        let base_result = self.validate_sync(cmd);
        if !base_result.valid { return base_result; }

        // 2. 物理约束校验
        if let Some(ref av) = self.action_validator {
            let mut action = ActionOutput {
                p_batt_set: cmd.p_batt_set.unwrap_or(0.0),
                q_batt_set: cmd.q_batt_set.unwrap_or(0.0),
                compens_factor_a: cmd.phase_compensation.map(|p| p[0]).unwrap_or(0.0),
                compens_factor_b: cmd.phase_compensation.map(|p| p[1]).unwrap_or(0.0),
                compens_factor_c: cmd.phase_compensation.map(|p| p[2]).unwrap_or(0.0),
                load_shedding: cmd.load_shedding.unwrap_or(0.0),
                pv_limit: cmd.pv_limit.unwrap_or(1.0),
                confidence: 0.8,
            };
            let _report = av.validate(&mut action, state, scene);

            let mut cmd_clone = cmd.clone();
            cmd_clone.p_batt_set = Some(action.p_batt_set);
            cmd_clone.q_batt_set = Some(action.q_batt_set);
            // ... 回写校验后的值

            return ValidationResult {
                valid: base_result.valid,
                message: "通过物理约束校验".into(),
                suggested_command: Some(cmd_clone),
            };
        }

        base_result
    }
}
```

### 9.3 兜底策略联动

不同场景下本地兜底策略参数自动适配：

| 场景 | 防逆流阈值 | 需量控制阈值 | 削峰填谷策略 |
|------|-----------|-------------|-------------|
| 农网灌溉 | 降低至 5% | 正常 (90%) | 侧重光伏消纳 |
| 自主套利 | 正常 10% | 正常 | 侧重峰谷套利 |
| 需量控制 | 正常 | 降低至 80% | 侧重削峰 |
| 虚拟电厂 | 正常 | 正常 | 跟随 VPP 指令 |
| 极致绿色 | 降低至 3% | 正常 | 侧重绿电消纳 |

---

## 10. 文件结构

### 10.1 ai-engine crate 完整结构

```
mupc/crates/ai-engine/
├── Cargo.toml
├── src/
│   ├── lib.rs                    # 模块导出 + re-export
│   ├── model_manager.rs          # 模型管理器（统一调度）
│   ├── lstm_model.rs             # LSTM 预测模型
│   ├── rl_model.rs               # MADDPG/PPO 决策模型（含 SystemState、ActionOutput）
│   ├── scene_classifier.rs       # 场景分类器（SceneClassifier + RuleBasedClassifier）
│   ├── reward_calculator.rs      # 奖励函数计算器（5 种场景奖励函数）
│   ├── data_fusion.rs            # 多源数据融合引擎（5 个数据源适配器）
│   ├── action_validator.rs       # 动作约束校验器（6 条约束规则）
│   ├── online_updater.rs         # 在线微调（Phase 3C.2 实现）
│   ├── rknn_runtime.rs           # RKNN Runtime 推理（RK3588 NPU FFI 高层接口）
│   ├── rknn_runtime_sys.rs       # RKNN Runtime C API FFI 绑定
│   ├── rknn_types.rs             # RKNN Runtime 类型定义
│   ├── error.rs                  # 错误类型定义
│   └── config.rs                 # 配置结构定义
└── tests/
    ├── ai_engine_tests.rs
    ├── rknn_runtime_tests.rs
    ├── lstm_model_tests.rs
    └── rl_model_tests.rs
```

### 10.2 strategy-engine crate 变更

| 文件 | 变更类型 | 说明 |
|------|----------|------|
| `src/ai_integration.rs` | 修改 | AiIntegrator 扩展: run_full_cycle(), validate_action(), 场景通知 |
| `src/ai_validator.rs` | 修改 | AiCommandValidatorImpl 扩展: validate_enhanced() + 物理约束集成 |
| `src/lib.rs` | 修改 | 重新导出新类型 |
| `Cargo.toml` | 修改 | 新增 mupc-ai-engine 依赖 |

---

## 11. 配置结构

```rust
// config.rs

/// AI 引擎配置（完整版）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AiEngineConfig {
    pub lstm: LstmConfig,
    pub rl: RlConfig,
    pub online_update: OnlineUpdateConfig,
    pub fusion: FusionConfig,
    pub scene_classifier: SceneClassifierConfig,
    pub action_constraint: ActionConstraintConfig,
    pub reward_weights: HashMap<String, SceneWeights>,
    pub npu: NpuConfig,
}

/// LSTM 模型配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LstmConfig {
    pub model_path: PathBuf,             // 默认 /etc/mupc/models/lstm.rknn
    pub input_window_secs: u64,          // 默认 3600 (1小时)
    pub output_horizon_secs: u64,        // 默认 900 (15分钟)
    pub quantization: QuantizationType,  // 默认 INT8
}

/// 强化学习模型配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RlConfig {
    pub model_path: PathBuf,             // 默认 /etc/mupc/models/rl.rknn
    pub algorithm: RlAlgorithm,          // MADDPG / PPO
    pub quantization: QuantizationType,  // 默认 INT8
}

/// 在线微调配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OnlineUpdateConfig {
    pub enabled: bool,          // 默认 false
    pub batch_size: usize,      // 默认 32
    pub learning_rate: f64,     // 默认 0.001
}

/// 融合配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FusionConfig {
    pub interval_secs: u64,           // 默认 1
    pub max_missing_cycles: u32,      // 默认 10
    pub enable_health_monitor: bool,  // 默认 true
}

/// 场景分类器配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SceneClassifierConfig {
    pub enabled: bool,                       // 默认 true
    pub window_size: usize,                  // 默认 30
    pub classification_interval_secs: u64,   // 默认 60
    pub min_confidence: f64,                 // 默认 0.6
    pub oscillation_window_mins: u64,        // 默认 5
    pub oscillation_lock_mins: u64,          // 默认 30
    pub use_ml_auxiliary: bool,              // 默认 false
}

/// 动作约束配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionConstraintConfig {
    pub s_max: f64,               // 默认 500.0
    pub p_batt_rate_limit: f64,   // 默认 50.0
    pub q_batt_rate_limit: f64,   // 默认 30.0
    pub pv_limit_min: f64,        // 默认 0.1
}

/// NPU 配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NpuConfig {
    pub core_id: usize,               // 默认 4 (Cortex-A76)
    pub use_realtime_sched: bool,     // 默认 true
    pub temperature_threshold: f64,   // 默认 85.0
    pub enable_fallback: bool,        // 默认 true
}

/// 量化类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum QuantizationType { FP32, FP16, INT8 }

/// 模型类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelType { LSTM, MADDPG, PPO }

/// 强化学习算法
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RlAlgorithm { MADDPG, PPO }
```

---

## 12. 错误类型

```rust
// error.rs

#[derive(Error, Debug)]
pub enum AiEngineError {
    #[error("模型加载失败: {0}")]
    ModelLoadFailed(String),

    #[error("推理执行失败: {0}")]
    InferenceFailed(String),

    #[error("模型未加载")]
    ModelNotLoaded,

    #[error("输入形状不匹配: 期望 {expected:?}, 实际 {actual:?}")]
    InputShapeMismatch { expected: Vec<i32>, actual: Vec<i32> },

    #[error("输出形状不匹配")]
    OutputShapeMismatch,

    #[error("RKNN Runtime 错误: {0}")]
    RknnError(String),

    #[error("模型版本不兼容: {0}")]
    VersionMismatch(String),

    #[error("在线微调失败: {0}")]
    OnlineUpdateFailed(String),

    #[error("数据融合失败: {0}")]
    FusionFailed(String),

    #[error("场景分类失败: {0}")]
    SceneClassificationFailed(String),

    #[error("动作约束校验失败: {0}")]
    ActionValidationFailed(String),

    #[error("数据源异常: {source} 连续{cycles}周期无更新")]
    DataSourceStale { source: String, cycles: u32 },

    #[error("NPU 温度过高: {temperature}°C")]
    NpuOverheating { temperature: f64 },

    #[error("奖励函数计算异常: {0}")]
    RewardCalculationError(String),
}
```

---

## 13. 消息总线集成

使用 Tokio `broadcast` 通道实现进程内消息总线：

```rust
/// AI 消息总线
pub struct AiMessageBus {
    pub fused_state_tx: broadcast::Sender<FusedSystemState>,
    pub scene_change_tx: broadcast::Sender<SceneRecognitionResult>,
    pub action_output_tx: broadcast::Sender<ActionOutput>,
    pub reward_value_tx: broadcast::Sender<SceneRewardValue>,
    pub model_status_tx: broadcast::Sender<ModelStatusMessage>,
}

impl AiMessageBus {
    pub fn new() -> Self {
        let (fused_state_tx, _) = broadcast::channel(64);
        let (scene_change_tx, _) = broadcast::channel(64);
        let (action_output_tx, _) = broadcast::channel(64);
        let (reward_value_tx, _) = broadcast::channel(64);
        let (model_status_tx, _) = broadcast::channel(64);
        Self { /* ... */ }
    }
}
```

### 消息 Topic 定义

| Topic | 发布者 | 订阅者 | 频率 |
|-------|--------|--------|------|
| `ai/fused_state` | DataFusionEngine | RLModel, SceneClassifier | 1Hz |
| `ai/mode_switch` | ModeSelector | RewardCalculator, ModelManager, Web UI | 事件驱动 |
| `ai/action_output` | ModelManager | strategy-engine, intercore | 1Hz |
| `ai/reward_value` | RewardCalculator | OnlineUpdater, Web UI | 1Hz |
| `ai/model_status` | ModelManager | Web UI, 告警模块 | 1Hz |

---

## 14. 技术决策记录

### ADR-01: 规则引擎 vs 深度学习方案选型

**上下文：** 场景分类器的算法选型。

**决策：** 采用规则引擎为主方案，同时预留 ML 辅助接口。

**理由：**
1. 5 种场景的特征规则非常清晰（负荷占比、电价时段、VPP 指令等硬边界条件）
2. 规则引擎可以达到 PRD 要求的 >=95% 准确率
3. 运维人员可以直观理解和调整分类规则
4. 零训练数据依赖，部署即用
5. 运行时资源 <0.1ms，不需额外模型文件

### ADR-02: FFI 采用静态链接 vs 动态加载

**上下文：** 链接 `librknnrt.so` 的方式。

**决策：** 静态链接 `#[link(name = "rknnrt")]` 优先，兜底采用 `libloading` 动态加载。

**理由：**
1. 静态链接在编译时检查符号完整性，减少运行时符号缺失风险
2. `libloading` 作为兜底，允许在没有 NPU 驱动的开发环境上运行（模拟模式）

### ADR-03: 并行推理模型选择

**上下文：** CPU 推理降级后端的选型。

**决策：** 使用 tract ONNX Runtime 作为 CPU 降级后端。

**理由：**
1. tract 是纯 Rust 实现，无 C 依赖，编译部署简单
2. 支持 ONNX 模型直接加载，不需要额外的模型转换
3. 在 ARM64 上有较好的性能表现
4. 社区活跃，持续维护

### ADR-04: SceneWeights 采用具名字段

**上下文：** 权重配置的命名方式。

**决策：** 使用 `primary_reward` / `secondary_reward` / `degradation_penalty` / `overload_penalty` 具名字段替代 w1/w2/w3/w4。

**理由：**
1. 不同类型权重的语义在不同场景下含义不同，具名字段消除了歧义
2. 配置文件的 self-documenting 能力增强
3. 设计评审中确认修复方案

### ADR-05: FusedSystemState 中保留 peak_price / valley_price 但不纳入推理输入

**上下文：** 电价字段的用途划分。

**决策：** `peak_price` 和 `valley_price` 仅用于奖励函数计算中的套利价差计算，不纳入 RL 推理输入向量（50 维向量中不含这两个字段）。

**理由：**
1. RL 模型的输入状态空间应只包含模型决策需要的信息
2. 套利价差是奖励函数的计算参数，不是决策的输入特征
3. 避免输入向量维度膨胀，减少 NPU 推理的计算量

### ADR-06: 异步封装使用 spawn_blocking

**上下文：** FFI 调用与 Tokio 异步运行时的集成。

**决策：** 所有 FFI 调用使用 `tokio::task::spawn_blocking` 在后台线程执行，不阻塞 Tokio async runtime。

**理由：**
1. C 语言的 `rknn_run` 是同步阻塞调用，直接调用会阻塞 Tokio worker 线程
2. `spawn_blocking` 将阻塞调用移到专用线程池，不会影响其他异步任务的执行
3. Tokio 官方推荐的 C FFI 集成模式

---

## 附录 A：性能指标与延迟预算

| 处理阶段 | 预算上限 | 当前设计估计 |
|----------|----------|-------------|
| 数据融合 (单次) | <1ms | ~0.5ms |
| 场景分类 (规则引擎) | <5s (含 30s 窗口) | ~0.2ms (规则) |
| 状态序列化 (50维) | <5ms | ~0.01ms |
| NPU 推理 | <100ms P99 | ~80ms (INT8) |
| 动作约束校验 | <0.5ms | ~0.05ms |
| 奖励函数计算 | <1ms | ~0.1ms |
| 完整决策周期 | <120ms | ~85ms |
| 在线微调 (batch=32) | <=10s | TBD |

## 附录 B：验收标准汇总

| 模块 | ID 范围 | 优先级 |
|------|---------|--------|
| LSTM 推理 | LSTM-01~05, AI-01, AI-03, AI-05 | P0 |
| RL 推理 | RL-01~03, AI-02, AI-04 | P0 |
| RKNN Runtime | RK-01~08, NPU-01~06 | P0 |
| 数据融合 | FUSION-01~10 | P0/P1 |
| 场景识别 | SCENE-01~06 | P0 |
| 状态/动作空间 | STATE-01~05, ACT-07~11 | P0 |
| 动作约束校验 | ACT-01~06 | P0 |
| 奖励函数 | REWARD-A1~E4 | P0 |
| 动态权重 | WEIGHT-01~05 | P1 |
| 在线微调 | UPDATE-01~04, AI-07 | P1 |
| 策略集成 | AI-08 | P0 |
| 模型部署 | 模型大小/内存/MTBF/降级 | P0/P1/P2 |

## 附录 C：模型退化处理

| 退化场景 | 检测条件 | 处理措施 |
|----------|----------|----------|
| 推理精度持续下降 | loss 连续 10 个周期不下降或上升 | 停止在线微调，回滚至上一检查点 |
| 推理延迟持续超标 | 连续 100 次推理中 > 10% 超出 150ms | 降级至 CPU 推理模式，记录 ALERT 日志 |
| 模型文件损坏 | SHA256 校验失败 | 拒绝加载，尝试从 OTA 备份恢复 |
| 奖励函数计算异常 | 奖励值偏离正常范围（超出 [0, 200]）| 截断至边界值，记录 ERROR 日志 |

## 附录 D：数据融合异常降级流程

```
任一数据源连续3个周期无更新
    ↓
产生 WARN 告警
    ↓
使用上一有效值填充（最多持续 10 个周期）
    ↓
超过 10 个周期仍未恢复
    ↓
触发 AI 降级流程
    ↓
strategy-engine 进入兜底模式
    ↓
本地策略引擎接管控制
    ↓
待 AI 所需全部数据源恢复 5 个连续周期后，自动切回 AI 模式
```

## 附录 E：待澄清问题

| 序号 | 问题 | 优先级 | 影响评估 |
|------|------|--------|----------|
| 1 | 气象数据的外部来源是何种 API（如和风天气、中国气象局）？是否需要额外商务授权？ | 高 | 影响 DataFusionEngine 的气象数据获取实现 |
| 2 | 电价数据是直接来自物联平台下发，还是需要通过 MUPC 本地配置？ | 高 | 影响 DataFusionEngine 的电价数据管道设计 |
| 3 | 分相补偿系数的硬件限制（实际的 SVG/APF 能否按系数调节三相无功）？ | 高 | 影响 ActionValidator 的硬件约束规则 |
| 4 | VPP 辅助服务的容量价格和里程价格是否有标准合同模板？还是由 VPP 平台实时下发？ | 中 | 影响 R_ancillary_service 的参数来源 |
| 5 | 在线微调是否需要经过审批流程（安全考虑）？还是自动触发？ | 中 | 影响 OnlineUpdater 的触发策略 |
| 6 | 气象数据连续缺失时长 10 个周期是融合周期（10 秒）还是 10 个 15 分钟气象更新周期（150 分钟）？ | 中 | 影响 FUSION 告警阈值配置 |

---

## v2.0 修订记录

| 序号 | 修订项 | 修订位置 | 说明 |
|------|--------|----------|------|
| 1 | SceneClassifier → ModeSelector | 1.1~1.5、4、8、10、11、13 | 删除 rule-based 自动场景分类器，替换为 ModeSelector 互斥模式选择器 |
| 2 | 更新架构图与数据流 | 1.1、1.4、1.5 | 架构图新增选择层（IEC 104/61850/Web UI → ModeSelector）；数据流移除 SceneClassifier；决策周期步骤 2 从 scene_classifier.recognize() 改为 mode_selector.current() |
| 3 | 章节 4 标记废弃 | 4. 标题 + 4.1 | SceneClassifier 设计保留作为历史参考，添加 v2.0 废弃说明和迁移指引 |
| 4 | ModelManager 结构更新 | 8.1 | scene_classifier → mode_selector 字段替换 |
| 5 | 消息总线 topic 更新 | 13. 消息 Topic 定义 | ai/scene_change → ai/mode_switch |
| 6 | 新增设计文档引用 | 文档头部参考表 | 新增 `2026-05-29-MUPC-AI预设运行场景与互斥模式选择-设计文档.md` [DESIGN_APPROVED] |
| 7 | 版本号更新 | 文档头部 | v1.0 → v2.0 |

**修订依据：** `docs/superpowers/specs/2026-05-29-MUPC-AI预设运行场景与互斥模式选择-PRD.md` [REVIEWED: PASS]
**配套设计：** `docs/superpowers/plans/2026-05-29-MUPC-AI预设运行场景与互斥模式选择-设计文档.md` [DESIGN_APPROVED]
