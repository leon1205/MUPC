//! 错误类型定义

use thiserror::Error;

/// 国密模块错误类型
#[derive(Debug, Error)]
pub enum GmError {
    #[error("密钥加载失败: {0}")]
    KeyLoadFailed(String),

    #[error("签名失败: {0}")]
    SignFailed(String),

    #[error("验签失败: {0}")]
    VerifyFailed(String),

    #[error("加密失败: {0}")]
    EncryptFailed(String),

    #[error("解密失败: {0}")]
    DecryptFailed(String),

    #[error("证书验证失败: {0}")]
    CertVerifyFailed(String),

    #[error("TLS 配置错误: {0}")]
    TlsConfigError(String),

    #[error("数据格式错误: {0}")]
    InvalidFormat(String),

    #[error("不支持的操作: {0}")]
    Unsupported(String),
}

/// Result 类型别名
pub type Result<T> = std::result::Result<T, GmError>;