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
    /// SHA256 期望值（PRD 9.5），开发环境为 None 跳过校验
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

impl Default for LstmConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("/etc/mupc/models/lstm.rknn"),
            input_window_secs: 3600,
            output_horizon_secs: 900,
            quantization: QuantizationType::INT8,
            expected_sha256: None,
        }
    }
}

/// 强化学习模型配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RlConfig {
    pub model_path: PathBuf,
    pub algorithm: RlAlgorithm,
    pub quantization: QuantizationType,
    /// SHA256 期望值（PRD 9.5），开发环境为 None 跳过校验
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

impl Default for RlConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("/etc/mupc/models/rl.rknn"),
            algorithm: RlAlgorithm::MADDPG,
            quantization: QuantizationType::INT8,
            expected_sha256: None,
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
/// v2.3 添加 factory_scene, model_dir, model_manifest
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModeConfig {
    /// 设备出厂预装场景 (v2.3: 替代 default_mode)
    #[serde(default = "default_factory_scene")]
    pub factory_scene: String,
    /// 场景 RL 模型文件存储目录 (v2.3 新增)
    #[serde(default = "default_model_dir")]
    pub model_dir: String,
    /// 模型清单文件路径 (v2.3 新增)
    #[serde(default = "default_model_manifest")]
    pub model_manifest: String,
    /// 模式持久化文件路径
    #[serde(default = "default_mode_persist_path")]
    pub persist_path: String,
}

fn default_factory_scene() -> String {
    "AgriculturalIrrigation".to_string()
}

fn default_model_dir() -> String {
    "/var/lib/mupc/models".to_string()
}

fn default_model_manifest() -> String {
    "/etc/mupc/models/manifest.json".to_string()
}

fn default_mode_persist_path() -> String {
    "/var/lib/mupc/current_mode".to_string()
}

impl Default for ModeConfig {
    fn default() -> Self {
        Self {
            factory_scene: default_factory_scene(),
            model_dir: default_model_dir(),
            model_manifest: default_model_manifest(),
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
    /// 台区季节性负荷: [w1光伏消纳, w2电池损耗, w3变压器, w4电压质量, w5功率变化率]
    pub seasonal_load_management: [f64; 5],
    pub commercial_arbitrage: [f64; 2],
    pub demand_control: [f64; 2],
    pub virtual_power_plant: [f64; 3],
    pub ultra_green: [f64; 2],
}

impl Default for SceneWeights {
    fn default() -> Self {
        Self {
            seasonal_load_management: [1.0, 0.5, 2.0, 1.0, 0.5],
            commercial_arbitrage: [1.0, 1.0],
            demand_control: [1.0, 0.5],
            virtual_power_plant: [1.0, 2.0, 1.0],
            ultra_green: [1.0, 1.0],
        }
    }
}
