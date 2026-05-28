use thiserror::Error;

#[derive(Error, Debug)]
pub enum DataProcessingError {
    #[error("数据采集失败: {0}")]
    CollectionFailed(String),

    #[error("消息发送失败: {0}")]
    MessageSendFailed(String),

    #[error("数据库错误: {0}")]
    DatabaseError(String),

    #[error("配置错误: {0}")]
    ConfigError(String),
}
