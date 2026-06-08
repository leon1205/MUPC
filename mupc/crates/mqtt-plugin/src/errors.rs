//! MQTT 错误类型定义

use thiserror::Error;

/// MQTT 插件错误类型
#[derive(Debug, Error)]
pub enum MqttError {
    #[error("连接失败: {0}")]
    ConnectFailed(String),

    #[error("认证失败: {0}")]
    AuthFailed(String),

    #[error("订阅失败: {0}")]
    SubscribeFailed(String),

    #[error("发布失败: {0}")]
    PublishFailed(String),

    #[error("连接已断开: {0}")]
    Disconnected(String),

    #[error("TLS 配置错误: {0}")]
    TlsConfigError(String),

    #[error("QoS 不支持: {0}")]
    QosNotSupported(String),

    #[error("协议错误: {0}")]
    ProtocolError(String),
}

/// Result 类型别名
#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, MqttError>;
