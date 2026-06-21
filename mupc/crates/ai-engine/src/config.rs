//! AI 引擎配置结构

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// AI 引擎配置
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AiEngineConfig {
    pub lstm: LstmConfig,
    pub rl: RlConfig,
    pub online_update: OnlineUpdateConfig,
    pub fusion: FusionConfig,
    pub mode: ModeConfig,
    pub action_constraint: ActionConstraintConfig,
    pub reward_weights: SceneWeights,
    pub npu: NpuConfig,
    /// v2.5 奖励阈值配置（对应 PRD 4.1）
    #[serde(default)]
    pub reward_thresholds: RewardThresholdConfig,
    /// v2.17 安全 RL 包装器配置
    #[serde(default)]
    pub safety_wrapper: SafetyWrapperConfig,
    /// v1.0 预测增强配置（VMD + Attention）
    /// 缺失时全部增强功能禁用，运行于 v2.16 基线模式
    #[serde(default)]
    pub prediction_enhancement: Option<PredictionEnhancementConfig>,
}

/// LSTM 模型配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LstmConfig {
    pub model_path: PathBuf,
    pub input_window_secs: u64,
    pub output_horizon_secs: u64,
    /// v2.16 新增：采样步长（秒），统一输入输出步长计算
    ///
    /// 默认 900 秒（15 分钟），与 MUPC-AI2 训练管线对齐。
    /// 历史硬编码 `/ 60` 假设已废弃。
    pub step_seconds: u64,
    pub quantization: QuantizationType,
    /// SHA256 期望值（PRD 9.5），开发环境为 None 跳过校验
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

impl Default for LstmConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("/etc/mupc/models/lstm.rknn"),
            input_window_secs: 21_600,   // 6 小时
            output_horizon_secs: 22_500, // 225 分钟 = 15 步 × 15 分钟
            step_seconds: 900,           // 15 分钟步长
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
    /// v2.10 R1: 渐进式切换配置
    #[serde(default)]
    pub gradual_switch: GradualSwitchConfig,
}

impl Default for OnlineUpdateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            batch_size: 32,
            learning_rate: 0.001,
            gradual_switch: GradualSwitchConfig::default(),
        }
    }
}

/// 渐进式切换配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GradualSwitchConfig {
    /// 是否启用
    pub enabled: bool,
    /// 切换步数
    pub steps: usize,
    /// 每步间隔（秒）
    pub step_interval_secs: f64,
}

impl Default for GradualSwitchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            steps: 10,
            step_interval_secs: 1.0,
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
    "SeasonalLoadManagement".to_string()
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
            max_apparent_power_kva: 200.0,
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

/// v2.5 奖励阈值配置（对应 PRD 4.1）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RewardThresholdConfig {
    /// 电压死区（±5%），与现有设计一致
    pub voltage_deadband: f64,
    /// Q 裕度阈值：实时模块剩余容量低于此值视为"无功耗尽"
    pub q_margin_threshold: f64,
    /// 弃光前置电压阈值：电压高于此值时弃光奖励不计入
    pub voltage_high_limit: f64,
    /// SOC 极低保护阈值
    pub soc_critical: f64,
    /// 高电压侧电压惩罚系数（光伏超发）
    pub voltage_penalty_high: f64,
    /// 低电压侧电压惩罚系数（灌溉/炒茶/空调负荷）
    pub voltage_penalty_low: f64,
}

impl Default for RewardThresholdConfig {
    fn default() -> Self {
        Self {
            voltage_deadband: 0.05,
            q_margin_threshold: 0.10,
            voltage_high_limit: 1.05,
            soc_critical: 0.10,
            voltage_penalty_high: 2.0,
            voltage_penalty_low: 1.0,
        }
    }
}

/// v2.11 自适应权重优化器配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AdaptiveOptimizerConfig {
    /// 是否启用
    pub enabled: bool,
    /// 更新间隔（小时）
    pub update_interval_hours: u32,
    /// 元学习率
    pub meta_learning_rate: f64,
    /// 权重边界
    pub weight_bounds: WeightBounds,
    /// 约束条件
    pub constraints: WeightConstraints,
}

impl Default for AdaptiveOptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            update_interval_hours: 168, // 周级更新
            meta_learning_rate: 0.001,
            weight_bounds: WeightBounds::default(),
            constraints: WeightConstraints::default(),
        }
    }
}

/// v2.11 权重边界
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WeightBounds {
    pub min: f64,
    pub max: f64,
}

impl Default for WeightBounds {
    fn default() -> Self {
        Self {
            min: 0.01,
            max: 10.0,
        }
    }
}

/// v2.11 权重约束
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WeightConstraints {
    /// 权重和归一化基准
    pub sum_normalized: f64,
    /// 单次最大调整幅度
    pub max_adjustment_per_update: f64,
}

impl Default for WeightConstraints {
    fn default() -> Self {
        Self {
            sum_normalized: 8.3,
            max_adjustment_per_update: 0.2,
        }
    }
}

/// v2.11 NSGA-II Pareto 优化器配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ParetoOptimizerConfig {
    pub enabled: bool,
    pub population_size: usize,
    pub generations: usize,
    pub crossover_rate: f64,
    pub mutation_rate: f64,
}

impl Default for ParetoOptimizerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            population_size: 100,
            generations: 50,
            crossover_rate: 0.9,
            mutation_rate: 0.1,
        }
    }
}

/// 场景权重映射
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SceneWeights {
    /// 台区季节性负荷: [w1光伏消纳, w2电池损耗, w3变压器, w4PQ协同度, w5功率变化率, w6电压斜率, w7下垂平滑, w8安全覆盖]
    pub seasonal_load_management: [f64; 8], // v2.10: 7 → 8
    pub commercial_arbitrage: [f64; 2],
    pub demand_control: [f64; 2],
    pub virtual_power_plant: [f64; 3],
    pub ultra_green: [f64; 2],
}

impl Default for SceneWeights {
    fn default() -> Self {
        Self {
            seasonal_load_management: [1.0, 0.5, 2.0, 1.0, 0.5, 0.5, 0.3, 1.0], // v2.10: w8=1.0
            commercial_arbitrage: [1.0, 1.0],
            demand_control: [1.0, 0.5],
            virtual_power_plant: [1.0, 2.0, 1.0],
            ultra_green: [1.0, 1.0],
        }
    }
}

/// 预测增强配置（v1.0，2026-06-21）
///
/// 挂载在 `AiEngineConfig.prediction_enhancement` 下。
/// 缺失时（None）全部增强功能禁用，运行于 v2.16 基线模式。
pub type PredictionEnhancementConfig = crate::pipeline_config::PredictionEnhancementConfig;

/// v2.17 安全 RL 包装器配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SafetyWrapperConfig {
    /// 线路电阻 R（Ω）
    pub line_impedance_r_ohm: f64,
    /// 线路电抗 X（Ω）
    pub line_impedance_x_ohm: f64,
    /// 基准电压（V）
    pub v_base: f64,
    /// 电压下限（p.u.）
    pub v_min: f64,
    /// 电压上限（p.u.）
    pub v_max: f64,
    /// 电压变化率上限（p.u./s）
    pub dv_dt_max: f64,
    /// SOC 安全裕度（比临界 10% 多此值）
    pub soc_margin: f64,
    /// 单次检查最大延迟（ms）
    pub max_check_latency_ms: u64,
    /// 拒绝率告警阈值
    pub alert_rejection_rate: f64,
}

impl Default for SafetyWrapperConfig {
    fn default() -> Self {
        Self {
            line_impedance_r_ohm: 0.1,
            line_impedance_x_ohm: 0.05,
            v_base: 220.0,
            v_min: 0.93,
            v_max: 1.07,
            dv_dt_max: 0.03,
            soc_margin: 0.02,
            max_check_latency_ms: 5,
            alert_rejection_rate: 0.20,
        }
    }
}
