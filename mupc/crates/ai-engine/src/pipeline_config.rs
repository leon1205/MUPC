//! 预测增强管线配置结构体
//!
//! v1.0 (2026-06-21): VMD + Attention + BiLSTM/误差修正预留配置。
//! 支持 YAML 反序列化（serde），所有子段缺失时默认禁用增强。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// EnhancementLevel -- 增强等级（降级追踪）
// ============================================================================

/// 增强等级，值越小 = 功能越完整
///
/// Level 0-4 由 PredictionPipeline 内部管理，
/// Level 5（全零预测/安全兜底）由 ModelManager 调用方处理。
///
/// # v2.0 重新编号（插入 BiLstmVmdAttention）
///
/// | 枚举值 | 降级层级 | 管理者 | 说明 |
/// |--------|---------|--------|------|
/// | FullVmdAttentionCorrection (0) | Level 0 | Pipeline | Go 路径全功能 |
/// | BiLstmVmdAttention (1) | Level 1A | Pipeline | BiLSTM Go + VMD，无误差修正；或 error_correction.enabled=false 直接进入 |
/// | VmdAttention (2) | Level 1B/2 | Pipeline | No-Go 路径或 BiLSTM 降级 |
/// | AttentionOnly (3) | Level 3 | Pipeline | VMD 失败 |
/// | Baseline (4) | Level 4 | Pipeline | Attention 失败 |
/// | (无枚举) | Level 5 | ModelManager | 全零预测 |
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EnhancementLevel {
    /// 全功能：VMD + (Bi)LSTM/Attention + 误差修正（Go 路径）
    FullVmdAttentionCorrection = 0,
    /// BiLSTM + VMD + Attention（无误差修正，或 Go 路径误差修正降级到此层）
    BiLstmVmdAttention = 1,
    /// VMD + LSTM/Attention（No-Go 路径或 BiLSTM 降级）
    VmdAttention = 2,
    /// LSTM/Attention (无 VMD)
    AttentionOnly = 3,
    /// LSTM 基线 (无 VMD, 无 Attention) = v2.16 基线
    Baseline = 4,
}

impl EnhancementLevel {
    /// 返回用户友好的名称
    pub fn name(&self) -> &'static str {
        match self {
            EnhancementLevel::FullVmdAttentionCorrection => "VMD+Attention+修正",
            EnhancementLevel::BiLstmVmdAttention => "BiLSTM+VMD+Attention",
            EnhancementLevel::VmdAttention => "VMD+Attention",
            EnhancementLevel::AttentionOnly => "Attention",
            EnhancementLevel::Baseline => "基线LSTM",
        }
    }
}

// ============================================================================
// PipelineHealth -- 管线健康状态
// ============================================================================

/// 管线模块健康状态追踪（v2.0 扩展：误差修正 + BiLSTM 模块级追踪）
///
/// 首轮仅 VMD 需硬降级追踪（仅使用 vmd_* 字段），
/// R2 扩展为模块级健康状态数组，逐一追踪每个模块的降级/升级。
#[derive(Debug, Clone)]
pub struct PipelineHealth {
    /// VMD 连续失败次数
    pub vmd_consecutive_failures: u32,
    /// VMD 连续成功次数
    pub vmd_consecutive_successes: u32,
    /// 误差修正连续失败次数（R2 新增）
    pub ec_consecutive_failures: u32,
    /// 误差修正连续成功次数（R2 新增）
    pub ec_consecutive_successes: u32,
    /// BiLSTM 连续失败次数（R2 新增）
    pub bilstm_consecutive_failures: u32,
    /// BiLSTM 连续成功次数（R2 新增）
    pub bilstm_consecutive_successes: u32,
    /// 当前增强等级
    pub current_level: EnhancementLevel,
}

impl Default for PipelineHealth {
    fn default() -> Self {
        Self {
            vmd_consecutive_failures: 0,
            vmd_consecutive_successes: 0,
            ec_consecutive_failures: 0,
            ec_consecutive_successes: 0,
            bilstm_consecutive_failures: 0,
            bilstm_consecutive_successes: 0,
            current_level: EnhancementLevel::VmdAttention,
        }
    }
}

impl PipelineHealth {
    /// VMD 成功时调用：重置失败计数、递增成功计数
    pub fn on_success_vmd(&mut self) {
        self.vmd_consecutive_failures = 0;
        self.vmd_consecutive_successes += 1;
        // 注意：升级由 PredictionPipeline::try_promote 统一处理
    }

    /// VMD 失败时调用：重置成功计数、递增失败计数
    pub fn on_failure_vmd(&mut self) {
        self.vmd_consecutive_successes = 0;
        self.vmd_consecutive_failures += 1;
    }

    /// 误差修正成功时调用（R2 新增）
    pub fn on_success_ec(&mut self) {
        self.ec_consecutive_failures = 0;
        self.ec_consecutive_successes += 1;
    }

    /// 误差修正失败时调用（R2 新增）
    pub fn on_failure_ec(&mut self) {
        self.ec_consecutive_successes = 0;
        self.ec_consecutive_failures += 1;
    }

    /// BiLSTM 推理成功时调用（R2 新增）
    pub fn on_success_bilstm(&mut self) {
        self.bilstm_consecutive_failures = 0;
        self.bilstm_consecutive_successes += 1;
    }

    /// BiLSTM 推理失败时调用（R2 新增）
    pub fn on_failure_bilstm(&mut self) {
        self.bilstm_consecutive_successes = 0;
        self.bilstm_consecutive_failures += 1;
    }
}

// ============================================================================
// AttentionScoreType -- 注意力打分函数类型
// ============================================================================

/// Attention 打分函数类型（与 ONNX 模型元数据交叉校验）
///
/// 注意：Rust 侧不实现 Attention 计算；此枚举仅用于配置开关和元数据校验。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AttentionScoreType {
    /// 加性注意力（Bahdanau）
    #[default]
    Additive,
    /// 点积注意力（Luong dot）
    Dot,
    /// 通用注意力（Luong general）
    General,
}

// ============================================================================
// 子配置结构体
// ============================================================================

/// VMD 增强配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VmdEnhancementConfig {
    /// 是否启用 VMD
    #[serde(default = "default_false")]
    pub enabled: bool,
    /// 光伏模态数 K [2, 10]
    #[serde(default = "default_vmd_k_pv")]
    pub k_pv: usize,
    /// 负荷模态数 K [2, 10]
    #[serde(default = "default_vmd_k_load")]
    pub k_load: usize,
    /// 惩罚因子 [100, 5000]
    #[serde(default = "default_vmd_alpha")]
    pub alpha: f64,
    /// 噪声容忍度（Lagrangian 更新步长）
    /// tau=0.0 表示不做双升更新（标准 VMD 行为）
    #[serde(default = "default_vmd_tau")]
    pub tau: f64,
    /// 收敛容差 [1e-7, 1e-5]
    #[serde(default = "default_vmd_tol")]
    pub tol: f64,
    /// 最大迭代次数 [100, 2000]
    #[serde(default = "default_vmd_max_iter")]
    pub max_iter: usize,
}

impl Default for VmdEnhancementConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            k_pv: default_vmd_k_pv(),
            k_load: default_vmd_k_load(),
            alpha: default_vmd_alpha(),
            tau: 0.0,
            tol: default_vmd_tol(),
            max_iter: default_vmd_max_iter(),
        }
    }
}

/// Attention 配置
///
/// Attention 由 MUPC-AI2 训练时嵌入 ONNX 计算图，Rust 侧推理时自动生效。
/// 此配置用于：模型含 Attention 但运行时禁用（调试用途）、权重导出开关。
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct AttentionConfig {
    /// 是否启用 Attention（调试开关：模型含 Attention 时可临时禁用）
    #[serde(default = "default_false")]
    pub enabled: bool,
    /// 打分函数类型（与 ONNX 元数据交叉校验）
    #[serde(default)]
    pub score_type: AttentionScoreType,
    /// 是否导出注意力权重到日志（可视化/调试）
    #[serde(default)]
    pub export_weights: bool,
}

/// BiLSTM 配置（第二轮：双模型文件 + Go/No-Go 准入）
///
/// ## 双重门控
///
/// BiLSTM 的启用受双重门控约束：
/// 1. **配置门** `enabled`：运维人员在配置文件中主动开启
/// 2. **硬件验证门** `gate_passed`：RK3588 延迟摸底通过后由运维设为 true
///
/// 两个条件都满足（`enabled=true AND gate_passed=true`）才加载 BiLSTM 模型。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BiLstmConfig {
    /// 是否启用 BiLSTM（配置门）
    #[serde(default)]
    pub enabled: bool,
    /// Go/No-Go 准入标志：RK3588 硬件延迟摸底通过后设为 true（硬件验证门）
    #[serde(default)]
    pub gate_passed: bool,
    /// BiLSTM 模型文件路径（None 时使用默认路径 /etc/mupc/models/bilstm_attn.rknn）
    #[serde(default)]
    pub model_path: Option<PathBuf>,
    /// 隐状态维度覆盖（None = 使用模型内建值，仅调试用途）
    #[serde(default)]
    pub hidden_size_override: Option<usize>,
    /// BiLSTM 推理失败时是否自动回退单向 LSTM（默认 true）
    #[serde(default = "default_true")]
    pub fallback_on_failure: bool,
}

impl Default for BiLstmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            gate_passed: false,
            model_path: None,
            hidden_size_override: None,
            fallback_on_failure: true,
        }
    }
}

/// 误差修正配置（第二轮：独立 BiLSTM 残差修正管线）
///
/// ## 启用条件
///
/// 1. `enabled = true`
/// 2. 主预测模型存在系统性偏差（Bias 绝对值 > 3% MAPE）
/// 3. 残差缓冲已填充足够历史数据（或 `zero_init = true` 允许零填充）
///
/// ## 降级策略
///
/// 误差修正失败 → 连续 `auto_disable_after_failures` 次后自动禁用。
/// 恢复：OTA 下发新版 error_correction.rknn 后手动重新启用。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ErrorCorrectionConfig {
    /// 是否启用误差修正
    #[serde(default)]
    pub enabled: bool,
    /// 误差修正模型文件路径（None 时使用默认路径 /etc/mupc/models/error_correction.rknn）
    #[serde(default)]
    pub model_path: Option<PathBuf>,
    /// 残差窗口步数（默认 24，与主预测 input_window 对齐）
    #[serde(default = "default_residual_window_steps")]
    pub residual_window_steps: usize,
    /// 冷启动/缓冲未满时是否零向量填充（true = 零填充跳过修正，false = 拒绝推理）
    #[serde(default = "default_true")]
    pub zero_init: bool,
    /// 连续失败 N 次后自动禁用误差修正（0 = 不自动禁用）
    #[serde(default = "default_auto_disable_after_failures")]
    pub auto_disable_after_failures: u32,
    /// 是否启用系统性偏差检测（主预测 |Bias| > 3% MAPE 才启用修正）
    #[serde(default = "default_true")]
    pub enable_bias_check: bool,
}

impl Default for ErrorCorrectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model_path: None,
            residual_window_steps: default_residual_window_steps(),
            zero_init: true,
            auto_disable_after_failures: default_auto_disable_after_failures(),
            enable_bias_check: true,
        }
    }
}

/// 特征筛选配置（MIC 离线分析结果引用）
///
/// 注意：MIC 筛选在 MUPC-AI2 Python 训练管线中离线执行。
/// Rust 侧仅引用 mic_top_k 值以确认特征维度。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeatureSelectionConfig {
    /// MIC 筛选 Top-K 特征数
    #[serde(default = "default_mic_top_k")]
    pub mic_top_k: usize,
}

impl Default for FeatureSelectionConfig {
    fn default() -> Self {
        Self {
            mic_top_k: default_mic_top_k(),
        }
    }
}

// ============================================================================
// PredictionEnhancementConfig -- 顶层增强配置
// ============================================================================

/// 预测增强顶层配置
///
/// 挂载于 `AiEngineConfig.prediction_enhancement`。
/// 缺失时（None）全部增强功能禁用，运行于 v2.16 基线模式。
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PredictionEnhancementConfig {
    /// VMD 配置
    #[serde(default)]
    pub vmd: VmdEnhancementConfig,
    /// Attention 配置
    #[serde(default)]
    pub attention: AttentionConfig,
    /// BiLSTM 配置（预留）
    #[serde(default)]
    pub bilstm: BiLstmConfig,
    /// 误差修正配置（预留）
    #[serde(default)]
    pub error_correction: ErrorCorrectionConfig,
    /// 特征筛选配置
    #[serde(default)]
    pub feature_selection: FeatureSelectionConfig,
}

// ============================================================================
// Serde 默认值函数
// ============================================================================

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

fn default_vmd_k_pv() -> usize {
    5
}

fn default_vmd_k_load() -> usize {
    6
}

fn default_vmd_alpha() -> f64 {
    2000.0
}

fn default_vmd_tau() -> f64 {
    0.0
}

fn default_vmd_tol() -> f64 {
    1.0e-6
}

fn default_vmd_max_iter() -> usize {
    500
}

fn default_mic_top_k() -> usize {
    7
}

fn default_residual_window_steps() -> usize {
    24
}

fn default_auto_disable_after_failures() -> u32 {
    3
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_full_default() {
        let cfg = PredictionEnhancementConfig::default();
        // 默认：所有增强功能禁用
        assert!(!cfg.vmd.enabled);
        assert!(!cfg.attention.enabled);
        assert!(!cfg.bilstm.enabled);
        assert!(!cfg.error_correction.enabled);
        assert_eq!(cfg.vmd.k_pv, 5);
        assert_eq!(cfg.vmd.k_load, 6);
        assert_eq!(cfg.vmd.alpha, 2000.0);
        assert_eq!(cfg.feature_selection.mic_top_k, 7);
    }

    #[test]
    fn test_config_yaml_deserialize_missing_section() {
        // 模拟 YAML 中 prediction_enhancement 段完全缺失的场景
        let yaml = "{}";
        let cfg: PredictionEnhancementConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!cfg.vmd.enabled, "缺失 VMD 段时默认禁用");
        assert!(!cfg.attention.enabled, "缺失 Attention 段时默认禁用");
        assert!(!cfg.bilstm.enabled, "缺失 BiLSTM 段时默认禁用");
        assert!(!cfg.error_correction.enabled, "缺失误差修正段时默认禁用");
    }

    #[test]
    fn test_config_yaml_deserialize_vmd_enabled() {
        let yaml = r#"
vmd:
  enabled: true
  k_pv: 4
  k_load: 7
  alpha: 1500.0
  tol: 5.0e-7
  max_iter: 300
"#;
        let cfg: PredictionEnhancementConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.vmd.enabled);
        assert_eq!(cfg.vmd.k_pv, 4);
        assert_eq!(cfg.vmd.k_load, 7);
        assert_eq!(cfg.vmd.alpha, 1500.0);
        assert_eq!(cfg.vmd.tol, 5.0e-7);
        assert_eq!(cfg.vmd.max_iter, 300);
        // 其他段保持默认
        assert!(!cfg.attention.enabled);
    }

    #[test]
    fn test_config_yaml_deserialize_attention_enabled() {
        let yaml = r#"
attention:
  enabled: true
  score_type: dot
  export_weights: true
"#;
        let cfg: PredictionEnhancementConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.attention.enabled);
        assert_eq!(cfg.attention.score_type, AttentionScoreType::Dot);
        assert!(cfg.attention.export_weights);
        assert!(!cfg.vmd.enabled, "未显式启用 VMD");
    }

    #[test]
    fn test_enhancement_level_ordering() {
        assert!(
            EnhancementLevel::FullVmdAttentionCorrection
                < EnhancementLevel::BiLstmVmdAttention
        );
        assert!(EnhancementLevel::BiLstmVmdAttention < EnhancementLevel::VmdAttention);
        assert!(EnhancementLevel::VmdAttention < EnhancementLevel::AttentionOnly);
        assert!(EnhancementLevel::AttentionOnly < EnhancementLevel::Baseline);
    }

    #[test]
    fn test_enhancement_level_names() {
        assert_eq!(
            EnhancementLevel::FullVmdAttentionCorrection.name(),
            "VMD+Attention+修正"
        );
        assert_eq!(
            EnhancementLevel::BiLstmVmdAttention.name(),
            "BiLSTM+VMD+Attention"
        );
        assert_eq!(EnhancementLevel::VmdAttention.name(), "VMD+Attention");
        assert_eq!(EnhancementLevel::AttentionOnly.name(), "Attention");
        assert_eq!(EnhancementLevel::Baseline.name(), "基线LSTM");
    }

    #[test]
    fn test_pipeline_health_success_resets_failures() {
        let mut health = PipelineHealth::default();
        health.vmd_consecutive_failures = 3;
        health.vmd_consecutive_successes = 1;
        health.on_success_vmd();
        assert_eq!(health.vmd_consecutive_failures, 0);
        assert_eq!(health.vmd_consecutive_successes, 2);
    }

    #[test]
    fn test_pipeline_health_failure_resets_successes() {
        let mut health = PipelineHealth::default();
        health.vmd_consecutive_successes = 4;
        health.on_failure_vmd();
        assert_eq!(health.vmd_consecutive_successes, 0);
        assert_eq!(health.vmd_consecutive_failures, 1);
    }

    #[test]
    fn test_pipeline_health_ec_success_resets_failures() {
        let mut health = PipelineHealth::default();
        health.ec_consecutive_failures = 2;
        health.ec_consecutive_successes = 1;
        health.on_success_ec();
        assert_eq!(health.ec_consecutive_failures, 0);
        assert_eq!(health.ec_consecutive_successes, 2);
    }

    #[test]
    fn test_pipeline_health_ec_failure_resets_successes() {
        let mut health = PipelineHealth::default();
        health.ec_consecutive_successes = 4;
        health.on_failure_ec();
        assert_eq!(health.ec_consecutive_successes, 0);
        assert_eq!(health.ec_consecutive_failures, 1);
    }

    #[test]
    fn test_pipeline_health_bilstm_tracking() {
        let mut health = PipelineHealth::default();
        health.on_success_bilstm();
        assert_eq!(health.bilstm_consecutive_successes, 1);
        assert_eq!(health.bilstm_consecutive_failures, 0);

        health.on_failure_bilstm();
        assert_eq!(health.bilstm_consecutive_successes, 0);
        assert_eq!(health.bilstm_consecutive_failures, 1);
    }

    #[test]
    fn test_vmd_config_default_values() {
        let cfg = VmdEnhancementConfig::default();
        assert_eq!(cfg.k_pv, 5);
        assert_eq!(cfg.k_load, 6);
        assert_eq!(cfg.alpha, 2000.0);
        assert!(!cfg.enabled, "默认禁用 VMD");
    }

    #[test]
    fn test_attention_score_type_default() {
        assert_eq!(AttentionScoreType::default(), AttentionScoreType::Additive);
    }

    #[test]
    fn test_config_yaml_deserialize_bilstm_full() {
        let yaml = r#"
bilstm:
  enabled: true
  gate_passed: true
  model_path: "/etc/mupc/models/bilstm_attn.rknn"
  hidden_size_override: null
  fallback_on_failure: true
"#;
        let cfg: PredictionEnhancementConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.bilstm.enabled);
        assert!(cfg.bilstm.gate_passed);
        assert_eq!(
            cfg.bilstm.model_path,
            Some(std::path::PathBuf::from(
                "/etc/mupc/models/bilstm_attn.rknn"
            ))
        );
        assert!(cfg.bilstm.hidden_size_override.is_none());
        assert!(cfg.bilstm.fallback_on_failure);
    }

    #[test]
    fn test_config_yaml_deserialize_error_correction_full() {
        let yaml = r#"
error_correction:
  enabled: true
  model_path: "/etc/mupc/models/error_correction.rknn"
  residual_window_steps: 24
  zero_init: true
  auto_disable_after_failures: 3
  enable_bias_check: true
"#;
        let cfg: PredictionEnhancementConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.error_correction.enabled);
        assert_eq!(
            cfg.error_correction.model_path,
            Some(std::path::PathBuf::from(
                "/etc/mupc/models/error_correction.rknn"
            ))
        );
        assert_eq!(cfg.error_correction.residual_window_steps, 24);
        assert!(cfg.error_correction.zero_init);
        assert_eq!(cfg.error_correction.auto_disable_after_failures, 3);
        assert!(cfg.error_correction.enable_bias_check);
    }

    #[test]
    fn test_attention_score_type_serde() {
        let additive: AttentionScoreType = serde_yaml::from_str("additive").unwrap();
        assert_eq!(additive, AttentionScoreType::Additive);
        let dot: AttentionScoreType = serde_yaml::from_str("dot").unwrap();
        assert_eq!(dot, AttentionScoreType::Dot);
        let general: AttentionScoreType = serde_yaml::from_str("general").unwrap();
        assert_eq!(general, AttentionScoreType::General);
    }

    #[test]
    fn test_bilstm_config_default() {
        let cfg = BiLstmConfig::default();
        assert!(!cfg.enabled);
        assert!(!cfg.gate_passed);
        assert!(cfg.model_path.is_none());
        assert!(cfg.hidden_size_override.is_none());
        assert!(cfg.fallback_on_failure);
    }

    #[test]
    fn test_error_correction_config_default() {
        let cfg = ErrorCorrectionConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.model_path.is_none());
        assert_eq!(cfg.residual_window_steps, 24);
        assert!(cfg.zero_init);
        assert_eq!(cfg.auto_disable_after_failures, 3);
        assert!(cfg.enable_bias_check);
    }
}
