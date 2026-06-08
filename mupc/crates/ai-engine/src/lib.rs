//! AI 优化引擎模块
//!
//! Phase 3C 实现：
//! - LSTM 时序预测
//! - MADDPG/PPO 强化学习决策（v2.3: 5 个场景独立模型）
//! - RKNN Runtime 推理（RK3588 NPU）
//! - 多源数据融合（DataFusionEngine）
//! - 动作约束校验（ActionValidator）
//! - 奖励函数计算（RewardCalculator）
//! - 模型注册表（v2.3: ModelRegistry + 双缓冲热切换）

pub mod action_validator;
pub mod config;
pub mod data_fusion;
pub mod error;
pub mod lstm_model;
pub mod mode_selector;
pub mod model_manager;
pub mod model_registry;
pub mod online_updater;
pub mod reward_calculator;
pub mod rknn_runtime;
pub mod rknn_runtime_sys;
pub mod rknn_types;
pub mod rl_model;

pub use action_validator::{ActionValidator, ViolationRecord};
pub use config::{
    ActionConstraintConfig, AiEngineConfig, FusionConfig, LstmConfig, ModeConfig, ModelType,
    NpuConfig, OnlineUpdateConfig, QuantizationType, RlAlgorithm, RlConfig, SceneWeights,
};
pub use data_fusion::{
    validate_input_vector, DataFusionEngine, DataSourceAdapter, FusedSystemState, HealthStatus,
    SourceHealth, SourceType,
};
pub use error::AiEngineError;
pub use lstm_model::{LstmInput, LstmModel, LstmOutput};
pub use mode_selector::{
    parse_mode_name, ModeSelector, ModeSwitchEvent, RunningMode, SwitchSource,
};
pub use model_manager::{ModelManager, ModelStatus};
pub use model_registry::{ModelManifestEntry, ModelRegistry, SceneModelState, SceneSwitchResult};
pub use online_updater::{DataPoint, OnlineUpdater};
pub use reward_calculator::RewardCalculator;
pub use rknn_runtime::RknnRuntime;
pub use rl_model::{parse_action_output, ActionOutput, RLModel, SystemState};
