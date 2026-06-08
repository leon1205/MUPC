//! 错误类型定义

use thiserror::Error;

/// IEC 61850 插件错误类型
#[derive(Debug, Error)]
pub enum Iec61850Error {
    #[error("MMS 连接失败: {0}")]
    MmsConnectFailed(String),

    #[error("MMS 请求超时: {0}")]
    MmsTimeout(String),

    #[error("MMS 响应无效: {0}")]
    MmsInvalidResponse(String),

    #[error("GOOSE 订阅失败: {0}")]
    GooseSubscribeFailed(String),

    #[error("GOOSE 消息解析失败: {0}")]
    GooseParseFailed(String),

    #[error("数据对象不存在: {0}")]
    DataObjectNotFound(String),

    #[error("写操作失败: {0}")]
    WriteFailed(String),

    #[error("配置错误: {0}")]
    ConfigError(String),

    #[error("协议错误: {0}")]
    ProtocolError(String),

    #[error("ASN.1 编码失败: {0}")]
    Asn1EncodeFailed(String),

    #[error("MMS 协议错误: {0}")]
    MmsProtocolError(String),
}

/// Result 类型别名
pub type Result<T> = std::result::Result<T, Iec61850Error>;
