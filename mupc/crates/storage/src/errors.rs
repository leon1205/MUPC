use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("数据库错误: {0}")]
    DatabaseError(String),
    #[error("未找到: {0}")]
    NotFound(String),
    #[error("序列化错误: {0}")]
    SerializationError(String),
    #[error("迁移错误: {0}")]
    MigrationError(String),
    #[error("连接池耗尽")]
    ConnectionPoolExhausted,
}
