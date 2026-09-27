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
    /// **写入被闸门拒绝**（U-74 审查 A-7/B-1）：磁盘水位 ≥98% 或 DB 完整性降级 ⇒
    /// 按 03 PRD §7.6/§8.1 主动**拒写**（而非让磁盘写满 / 往受损库继续写）。
    ///
    /// 独立变体的理由：调用方需要能与"真·数据库错误"区分（前者是**设计动作**、
    /// 后者是**故障**）——混进 `DatabaseError` 会让排障时无法判断该修磁盘还是修库。
    #[error("写入被闸门拒绝: {0}")]
    WriteGated(String),
}
