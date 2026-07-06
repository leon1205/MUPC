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
}
