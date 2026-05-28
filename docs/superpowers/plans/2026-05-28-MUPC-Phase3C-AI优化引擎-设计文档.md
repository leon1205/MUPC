# MUPC Phase 3C AI 优化引擎 - 技术设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-28 | 架构师 | ✅ 已批准 |

---

[DESIGN_APPROVED]

---

## 1. 需求概述

### 1.1 项目背景

Phase 3B 已实现分层 MQTT 消息总线，Phase 3C 需要在 strategy-engine 中集成 AI 优化引擎，作为兜底策略的智能增强。

### 1.2 目标

1. 实现 LSTM 时序预测模型（光伏出力/负荷预测）
2. 实现 MADDPG/PPO 强化学习决策模型
3. 支持离线预训练 + 在线微调
4. 支持 RK3588 NPU 部署（INT8 量化）
5. 模型管理器统一调度

---

## 2. 架构设计

### 2.1 整体架构

```
┌─────────────────────────────────────────────────────────────────┐
│                     AI 优化引擎 (Phase 3C)                         │
├─────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐        │
│  │ LSTM 预测    │    │MADDPG/PPO   │    │ 在线微调    │        │
│  │ (时序预测)   │    │ (决策优化)   │    │ (持续学习)   │        │
│  └──────┬──────┘    └──────┬──────┘    └──────┬──────┘        │
│         │                   │                   │               │
│         └───────────────────┼───────────────────┘               │
│                             │                                    │
│                    ┌────────▼────────┐                        │
│                    │   模型管理器     │                        │
│                    │ (ModelManager)  │                        │
│                    └────────┬────────┘                        │
│                             │                                    │
│                    ┌────────▼────────┐                        │
│                    │  tract ONNX     │                        │
│                    │  Runtime        │                        │
│                    └────────┬────────┘                        │
│                             │                                    │
│              ┌──────────────┼──────────────┐                  │
│              ▼              ▼              ▼                     │
│        ┌─────────┐   ┌─────────┐   ┌─────────┐               │
│        │ RK3588  │   │  x86    │   │ 混合    │               │
│        │ NPU     │   │ Server  │   │ 部署    │               │
│        └─────────┘   └─────────┘   └─────────┘               │
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 核心模块

| 模块 | 职责 |
|------|------|
| LSTMModel | 时序预测（光伏出力、负荷预测）|
| RLModel | MADDPG/PPO 强化学习决策 |
| ModelManager | 统一接口、模型加载、调度 |
| OnlineUpdater | 在线微调（增量学习）|
| TraitInference | tract ONNX 推理运行时 |

---

## 3. 模块设计

### 3.1 新增 crate：`ai-engine`

```
mupc/
├── crates/
│   ├── ai-engine/              # 新增：AI 优化引擎
│   │   ├── src/
│   │   │   ├── lib.rs         # 模块导出
│   │   │   ├── model_manager.rs  # 模型管理器
│   │   │   ├── lstm_model.rs     # LSTM 预测模型
│   │   │   ├── rl_model.rs       # MADDPG/PPO 决策模型
│   │   │   ├── online_updater.rs # 在线微调
│   │   │   ├── rknn_runtime.rs   # RKNN Runtime 推理 (RK3588 NPU)
│   │   │   ├── error.rs         # 错误类型
│   │   │   └── config.rs        # 配置结构
│   │   ├── Cargo.toml
│   │   └── tests/
```

### 3.2 核心 Trait 定义

```rust
/// AI 模型 trait（统一接口）
#[async_trait]
pub trait AiModel: Send + Sync {
    /// 推理预测
    async fn predict(&self, input: &ModelInput) -> Result<ModelOutput, AiEngineError>;

    /// 模型类型
    fn model_type(&self) -> ModelType;
}

/// 模型类型
pub enum ModelType {
    LSTM,
    MADDPG,
    PPO,
}

/// RKNN Runtime 推理器
pub struct RknnRuntime {
    ctx: RknnContext,
    model_path: PathBuf,
}

impl RknnRuntime {
    /// 创建 RKNN Runtime 推理器
    pub fn new(model_path: &Path) -> Result<Self, AiEngineError>;

    /// 执行推理
    pub fn run(&self, input: &[f32]) -> Result<Vec<f32>, AiEngineError>;

    /// 获取输入张量形状
    pub fn get_input_shape(&self) -> Vec<i32>;

    /// 获取输出张量形状
    pub fn get_output_shape(&self) -> Vec<i32>;
}
```

### 3.3 配置结构

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AiEngineConfig {
    /// LSTM 模型配置
    pub lstm: LstmConfig,
    /// 强化学习模型配置
    pub rl: RlConfig,
    /// 在线微调配置
    pub online_update: OnlineUpdateConfig,
    /// 推理运行时配置
    pub runtime: RuntimeConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LstmConfig {
    pub model_path: PathBuf,
    pub input_window_secs: u64,   // 输入窗口（秒）
    pub output_horizon_secs: u64,  // 输出预测范围（秒）
    pub quantization: QuantizationType,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RlConfig {
    pub model_path: PathBuf,
    pub algorithm: RlAlgorithm,
    pub action_space: Vec<ActionConfig>,
    pub quantization: QuantizationType,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum QuantizationType {
    FP32,
    FP16,
    INT8,
}
```

---

## 4. 数据流设计

### 4.1 LSTM 预测数据流

```
历史数据 → LSTMModel.predict() → 光伏/负荷预测值 → 供 RL 模型使用
```

### 4.2 RL 决策数据流

```
LSTM 预测 + 当前状态 → RLModel.decide() → 最优动作 → StrategyEngine
```

### 4.3 在线微调数据流

```
新数据积累 → OnlineUpdater.update() → 模型权重更新 → 保存
```

---

## 5. 技术选型

| 组件 | 选择 | 说明 |
|------|------|------|
| 模型格式 | ONNX | 跨框架通用格式 |
| 量化工具 | rknn-toolkit2 | x86 服务器上运行，将 ONNX 量化为 INT8 |
| 推理框架 | RKNN Runtime | RK3588 NPU 专用，支持 INT8 加速 |
| 训练框架 | PyTorch | MADDPG/PPO/LSTM 实现 |
| 时序预测 | LSTM | PyTorch 实现，导出 ONNX |

### 5.1 推理流程

```
训练阶段 (x86 服务器):
PyTorch → ONNX → rknn-toolkit2 量化 → INT8 模型 (.rknn)

部署阶段 (RK3588):
INT8 模型 → RKNN Runtime → NPU 推理
```

### 5.2 部署架构

```
┌─────────────────────────────────────────────────────────────────┐
│                      x86 服务器 (训练)                            │
│  PyTorch → ONNX → rknn-toolkit2 → INT8 模型 (.rknn)              │
└─────────────────────────────────────────────────────────────────┘
                              ↓
                        复制到 RK3588
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│                      RK3588 (部署)                               │
│  INT8 模型 → RKNN Runtime → NPU 推理 (< 100ms)                   │
└─────────────────────────────────────────────────────────────────┘
```

---

## 6. 接口设计

### 6.1 ModelManager 接口

```rust
#[async_trait]
pub trait ModelManager: Send + Sync {
    /// 加载模型
    async fn load_model(&self, model_path: &Path) -> Result<(), AiEngineError>;

    /// 预测（LSTM）
    async fn predict(&self, input: &ModelInput) -> Result<PredictionOutput, AiEngineError>;

    /// 决策（RL）
    async fn decide(&self, state: &SystemState) -> Result<ActionOutput, AiEngineError>;

    /// 在线微调
    async fn update(&self, new_data: &[DataPoint]) -> Result<(), AiEngineError>;

    /// 获取模型状态
    fn get_status(&self) -> ModelStatus;
}
```

### 6.2 输入/输出结构

```rust
/// 模型输入
pub struct ModelInput {
    pub battery_soc: f64,
    pub pv_power: f64,
    pub load_power: f64,
    pub grid_power: f64,
    pub timestamp: i64,
}

/// 模型输出（预测）
pub struct PredictionOutput {
    pub pv_forecast: Vec<f64>,      // 光伏预测（未来N个时间步）
    pub load_forecast: Vec<f64>,   // 负荷预测
    pub confidence: f64,            // 置信度
}

/// 模型输出（决策）
pub struct ActionOutput {
    pub p_batt_set: f64,            // 电池功率设定 (kW)
    pub load_shedding: f64,         // 负荷切除 (kW)
    pub pv_limit: f64,              // PV 限功率 (0-1)
    pub confidence: f64,            // 决策置信度
}
```

---

## 7. 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| AI-01 | LSTM 模型加载成功 | 单元测试 |
| AI-02 | RL 模型加载成功 | 单元测试 |
| AI-03 | LSTM 预测延迟 < 1s | 性能测试 |
| AI-04 | RL 决策延迟 < 1s | 性能测试 |
| AI-05 | ONNX 模型格式正确 | 模型验证 |
| AI-06 | RK3588 NPU INT8 量化支持 | 集成测试 |
| AI-07 | 在线微调功能正常 | 单元测试 |
| AI-08 | 与 strategy-engine 集成正确 | 集成测试 |

---

## 8. 未来扩展

| Phase | 内容 |
|-------|------|
| 3C.2 | 模型自动更新（OTA）|
| 3C.3 | 多目标优化（Pareto）|
| 3C.4 | 联邦学习支持 |
