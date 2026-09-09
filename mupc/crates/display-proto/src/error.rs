//! 帧 / 配置 JSON 编解码与配置加载的轻量错误类型（KISS：仅 JSON + IO 两类）。
//!
//! 设计文档未单独规定错误类型；此处按"最小实现"提供，供 display-proto 的序列化
//! 便捷函数及后续消费方（mupcd / local-display）复用，避免各侧重复定义。

use thiserror::Error;

/// display-proto 统一结果类型。
pub type Result<T> = std::result::Result<T, Error>;

/// display-proto 错误。
#[derive(Debug, Error)]
pub enum Error {
    /// JSON 序列化/反序列化失败。
    #[error("display-proto json error: {0}")]
    Json(#[from] serde_json::Error),

    /// IO（配置/文件读写）失败。
    #[error("display-proto io error: {0}")]
    Io(#[from] std::io::Error),
}
