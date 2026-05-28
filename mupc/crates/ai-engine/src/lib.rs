//! AI 优化引擎模块
//!
//! Phase 3C 实现：
//! - LSTM 时序预测
//! - MADDPG/PPO 强化学习决策
//! - RKNN Runtime 推理（RK3588 NPU）

pub mod config;
pub mod error;
pub mod lstm_model;
pub mod model_manager;
pub mod online_updater;
pub mod rknn_runtime;
pub mod rknn_types;
pub mod rl_model;

pub use config::{
    AiEngineConfig, LstmConfig, ModelType, OnlineUpdateConfig, QuantizationType, RlAlgorithm,
    RlConfig,
};
pub use error::AiEngineError;
pub use model_manager::{ModelManager, ModelStatus};
pub use online_updater::{DataPoint, OnlineUpdater};
pub use rknn_runtime::RknnRuntime;
