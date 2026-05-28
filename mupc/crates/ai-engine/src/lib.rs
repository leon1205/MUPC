//! AI 优化引擎模块
//!
//! Phase 3C 实现：
//! - LSTM 时序预测
//! - MADDPG/PPO 强化学习决策
//! - RKNN Runtime 推理（RK3588 NPU）

pub mod error;
pub mod config;
pub mod rknn_runtime;
pub mod lstm_model;
pub mod rl_model;
pub mod model_manager;
pub mod online_updater;

pub use error::AiEngineError;
pub use config::{AiEngineConfig, LstmConfig, RlConfig, ModelType, QuantizationType, RlAlgorithm, OnlineUpdateConfig};
pub use model_manager::{ModelManager, ModelStatus};
pub use rknn_runtime::RknnRuntime;
pub use online_updater::{OnlineUpdater, DataPoint};