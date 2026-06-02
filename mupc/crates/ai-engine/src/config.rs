//! AI 引擎配置结构

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// AI 引擎配置
#[derive(Debug, Clone, Deserialize, Serialize)]
#[derive(Default)]
pub struct AiEngineConfig {
    pub lstm: LstmConfig,
    pub rl: RlConfig,
    pub online_update: OnlineUpdateConfig,
    pub fusion: FusionConfig,
    pub mode: ModeConfig,
    pub action_constraint: ActionConstraintConfig,
    pub reward_weights: SceneWeights,
    pub npu: NpuConfig,
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
            output_horizon_secs: 900,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelType {
    LSTM,
    MADDPG,
    PPO,
}

/// 强化学习算法
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// 模式选择配置（v2.0 替代 SceneClassifierConfig）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModeConfig {
    /// 系统启动时的默认模式
    #[serde(default = "default_mode_name")]
    pub default_mode: String,
    /// 模式持久化文件路径
    #[serde(default = "default_mode_persist_path")]
    pub persist_path: String,
}

fn default_mode_name() -> String {
    "AgriculturalIrrigation".to_string()
}

fn default_mode_persist_path() -> String {
    "/var/lib/mupc/current_mode".to_string()
}

impl Default for ModeConfig {
    fn default() -> Self {
        Self {
            default_mode: default_mode_name(),
            persist_path: default_mode_persist_path(),
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
