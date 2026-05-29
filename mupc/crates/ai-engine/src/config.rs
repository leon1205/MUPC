//! AI 引擎配置结构

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// AI 引擎配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AiEngineConfig {
    pub lstm: LstmConfig,
    pub rl: RlConfig,
    pub online_update: OnlineUpdateConfig,
    pub fusion: FusionConfig,
    pub scene_classifier: SceneClassifierConfig,
    pub action_constraint: ActionConstraintConfig,
    pub reward_weights: SceneWeights,
    pub npu: NpuConfig,
}

impl Default for AiEngineConfig {
    fn default() -> Self {
        Self {
            lstm: LstmConfig::default(),
            rl: RlConfig::default(),
            online_update: OnlineUpdateConfig::default(),
            fusion: FusionConfig::default(),
            scene_classifier: SceneClassifierConfig::default(),
            action_constraint: ActionConstraintConfig::default(),
            reward_weights: SceneWeights::default(),
            npu: NpuConfig::default(),
        }
    }
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

/// 数据融合配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FusionConfig {
    pub fusion_period_secs: u64,
    pub data_source_timeout_secs: u64,
    pub enable_health_monitoring: bool,
}

impl Default for FusionConfig {
    fn default() -> Self {
        Self {
            fusion_period_secs: 1,
            data_source_timeout_secs: 10,
            enable_health_monitoring: true,
        }
    }
}

/// 场景分类器配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SceneClassifierConfig {
    pub classification_period_secs: u64,
    pub min_confidence: f64,
    pub oscillation_lock_minutes: u32,
}

impl Default for SceneClassifierConfig {
    fn default() -> Self {
        Self {
            classification_period_secs: 60,
            min_confidence: 0.6,
            oscillation_lock_minutes: 30,
        }
    }
}

/// 动作约束配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionConstraintConfig {
    pub p_batt_ramp_limit_kw: f64,
    pub q_batt_ramp_limit_kvar: f64,
    pub max_apparent_power_kva: f64,
    pub pv_limit_min: f64,
}

impl Default for ActionConstraintConfig {
    fn default() -> Self {
        Self {
            p_batt_ramp_limit_kw: 50.0,
            q_batt_ramp_limit_kvar: 30.0,
            max_apparent_power_kva: 500.0,
            pv_limit_min: 0.1,
        }
    }
}

/// NPU 配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NpuConfig {
    pub temperature_limit_c: f32,
    pub throttle_factor: f64,
    pub enable_fallback_to_cpu: bool,
}

impl Default for NpuConfig {
    fn default() -> Self {
        Self {
            temperature_limit_c: 85.0,
            throttle_factor: 0.5,
            enable_fallback_to_cpu: true,
        }
    }
}

/// 场景权重映射
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SceneWeights {
    pub agricultural_irrigation: [f64; 4],
    pub commercial_arbitrage: [f64; 2],
    pub demand_control: [f64; 2],
    pub virtual_power_plant: [f64; 3],
    pub ultra_green: [f64; 2],
}

impl Default for SceneWeights {
    fn default() -> Self {
        Self {
            agricultural_irrigation: [0.25, 0.25, 0.25, 0.25],
            commercial_arbitrage: [0.5, 0.5],
            demand_control: [0.5, 0.5],
            virtual_power_plant: [0.4, 0.3, 0.3],
            ultra_green: [0.5, 0.5],
        }
    }
}
