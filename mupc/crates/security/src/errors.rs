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

    #[error("密钥派生失败: {0}")]
    KeyDeriveFailed(String),

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

    #[error("无效参数: {0}")]
    InvalidParam(String),

    #[error("密钥长度无效: {0}")]
    InvalidKeyLength(String),

    #[error("IO 错误: {0}")]
    IoError(String),

    #[error("隧道错误: {0}")]
    TunnelError(String),

    #[error("审计错误: {0}")]
    AuditError(String),

    #[error("策略错误: {0}")]
    PolicyError(String),

    #[error("合规检查失败: {0}")]
    ComplianceError(String),

    #[error("告警错误: {0}")]
    AlertError(String),

    #[error("安全启动错误: {0}")]
    BootError(String),

    #[error("完整性校验失败: {0}")]
    IntegrityError(String),

    #[error("加密操作错误: {0}")]
    CryptoError(String),

    #[error("配置错误: {0}")]
    ConfigError(String),

    #[error("未找到: {0}")]
    NotFound(String),
}

/// Result 类型别名
pub type Result<T> = std::result::Result<T, GmError>;

/// SecurityError 类型别名（供 Phase 2+ 新模块使用）
pub type SecurityError = GmError;