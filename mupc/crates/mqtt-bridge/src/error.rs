//! MQTT 网桥错误类型

use thiserror::Error;

#[derive(Error, Debug)]
pub enum MqttBridgeError {
    #[error("连接失败: {0}")]
    ConnectionFailed(String),

    #[error("订阅失败: {0}")]
    SubscribeFailed(String),

    #[error("发布失败: {0}")]
    PublishFailed(String),

    #[error("TLS 错误: {0}")]
    TlsError(String),

    #[error("证书错误: {0}")]
    CertificateError(String),

    #[error("断开连接: {0}")]
    Disconnected(String),

    #[error("重连失败:已达到最大重试次数 {0}")]
    MaxReconnectAttemptsReached(usize),

    #[error("超时: {0}")]
    Timeout(String),
}
