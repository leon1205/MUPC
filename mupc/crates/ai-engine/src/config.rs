//! AI 引擎配置结构

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// AI 引擎配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AiEngineConfig {
    pub lstm: LstmConfig,
    pub rl: RlConfig,
    pub online_update: OnlineUpdateConfig,
}

/// LSTM 模型配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LstmConfig {
    pub model_path: PathBuf,
    pub input_window_secs: u64,
    pub output_horizon_secs: u64,
    pub quantization: QuantizationType,
}

impl Default for LstmConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("/etc/mupc/models/lstm.rknn"),
            input_window_secs: 3600,
            output_horizon_secs: 1800,
            quantization: QuantizationType::INT8,
        }
    }
}

/// 强化学习模型配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RlConfig {
    pub model_path: PathBuf,
    pub algorithm: RlAlgorithm,
    pub quantization: QuantizationType,
}

impl Default for RlConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("/etc/mupc/models/rl.rknn"),
            algorithm: RlAlgorithm::MADDPG,
            quantization: QuantizationType::INT8,
        }
    }
}

/// 在线微调配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OnlineUpdateConfig {
    pub enabled: bool,
    pub batch_size: usize,
    pub learning_rate: f64,
}

impl Default for OnlineUpdateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            batch_size: 32,
            learning_rate: 0.001,
        }
    }
}

/// 量化类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum QuantizationType {
    FP32,
    FP16,
    INT8,
}

/// 模型类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelType {
    LSTM,
    MADDPG,
    PPO,
}

/// 强化学习算法
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RlAlgorithm {
    MADDPG,
    PPO,
}
