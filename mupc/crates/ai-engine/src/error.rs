//! AI 引擎错误类型

use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum AiEngineError {
    #[error("模型加载失败: {0}")]
    ModelLoadFailed(String),

    #[error("模型文件校验失败: 期望 {expected}, 实际 {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("推理失败: {0}")]
    InferenceFailed(String),

    #[error("模型未加载")]
    ModelNotLoaded,

    #[error("输入形状不匹配: 期望 {expected:?}, 实际 {actual:?}")]
    InputShapeMismatch {
        expected: Vec<i32>,
        actual: Vec<i32>,
    },

    #[error("输出形状不匹配")]
    OutputShapeMismatch,

    #[error("RKNN Runtime 错误: {0}")]
    RknnError(String),

    #[error("模型版本不兼容: {0}")]
    VersionMismatch(String),

    #[error("在线微调失败: {0}")]
    OnlineUpdateFailed(String),

    #[error("数据融合失败: {0}")]
    FusionFailed(String),

    #[error("模式切换失败: {0}")]
    ModeSwitchFailed(String),

    #[error("动作校验失败: {0}")]
    ActionValidationFailed(String),

    #[error("数据源过期: {0}")]
    DataSourceStale(String),

    #[error("NPU 温度过高: current={current}°C, limit={limit}°C")]
    NpuOverheating { current: f32, limit: f32 },

    #[error("奖励计算错误: {0}")]
    RewardCalculationError(String),

    #[error("配置加载失败: {0}")]
    ConfigLoadFailed(String),

    #[error("配置不匹配: {0}")]
    ConfigMismatch(String),

    // --- v1.0 预测增强（VMD + Attention）错误变体 ---
    /// VMD 分解失败
    #[error("VMD 分解失败: {0}")]
    VmdFailed(String),

    /// VMD 迭代不收敛
    // 当前 VMD 不收敛时返回 VmdResult（converged=false），由 pipeline 降级处理；
    // 此错误变体为物理层硬失败预留（如迭代中数值溢出）
    #[error("VMD 迭代不收敛 (max_iter={max_iter}, 最终误差={final_error})")]
    VmdNotConverged { max_iter: usize, final_error: f64 },

    /// Attention 层退化（所有权重相等）
    // 第二轮预留：Rust 侧不实现 Attention 计算，退化检测由 ONNX 推理/训练管线负责
    #[error("Attention 层退化 (所有权重相等)")]
    AttentionDegraded,

    /// 误差修正失败
    // 第二轮预留：误差修正模型路径在 PipelineConfig 中已定义，实际推理逻辑待实现
    #[error("误差修正失败: {0}")]
    ErrorCorrectionFailed(String),

    /// 预测管线错误（通用）
    #[error("预测管线错误: {0}")]
    PipelineError(String),

    // --- v2.0 第二轮 (BiLSTM + 误差修正) 错误变体 ---

    /// 模型文件校验失败（metadata / SHA256 / 维度不匹配）
    #[error("模型校验失败: model={model_path}, reason={reason}")]
    ModelValidationFailed { model_path: String, reason: String },

    /// 残差缓冲不足（zero_init=false 且缓冲未满时触发）
    #[error("残差缓冲不足: filled={filled}/{capacity}")]
    ResidualBufferInsufficient { filled: usize, capacity: usize },

    /// BiLSTM 准入未通过（warn 级别，不影响运行）
    ///
    /// 当 `bilstm.enabled=true` 但 `gate_passed=false` 时，
    /// 系统回退到单向 LSTM 并记录此消息。
    #[error("BiLSTM 准入未通过: gate_passed=false，回退到单向 LSTM")]
    BiLstmGateNotPassed,
}

// ============================================================================
// 辅助方法（R2 新增）
// ============================================================================

impl AiEngineError {
    /// 判断是否为误差修正相关错误
    ///
    /// 用于降级逻辑：误差修正失败时仅降级 EC 模块，不影响主预测。
    pub fn is_error_correction_failure(&self) -> bool {
        matches!(
            self,
            AiEngineError::ErrorCorrectionFailed(_)
                | AiEngineError::ResidualBufferInsufficient { .. }
        )
    }

    /// 判断是否为 BiLSTM 相关错误
    ///
    /// 用于降级逻辑：BiLSTM 推理失败时回退单向 LSTM。
    pub fn is_bilstm_failure(&self) -> bool {
        matches!(self, AiEngineError::BiLstmGateNotPassed)
    }
}
