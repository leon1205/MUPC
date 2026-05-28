//! AI 引擎错误类型

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AiEngineError {
    #[error("模型加载失败: {0}")]
    ModelLoadFailed(String),

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
}
