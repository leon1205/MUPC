use thiserror::Error;

// v3.1: mupc-common 错误构造函数（替代原始 MupcError::new() 调用）
pub mod mupc_errors {
    use mupc_common::MupcError;
    mupc_common::define_error!(unknown_error, mupc_common::ErrorCode::Unknown, "data-processing");
    mupc_common::define_error!(io_error, mupc_common::ErrorCode::IoError, "data-processing");
}

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

    #[error("波形处理错误: {0}")]
    WaveformError(String),

    #[error("触发配置错误: {0}")]
    TriggerConfigError(String),

    #[error("导出错误: {0}")]
    ExportError(String),

    #[error("存储满: 已用 {used_bytes} bytes / 总计 {total_bytes} bytes")]
    StorageFull { used_bytes: u64, total_bytes: u64 },

    #[error("文件损坏: {path} - {reason}")]
    FileCorrupted { path: String, reason: String },
}
