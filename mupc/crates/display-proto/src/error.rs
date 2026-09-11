//! 帧 / 配置 / 控制信封 JSON 编解码与校验的轻量错误类型。
//!
//! 设计文档未单独规定错误类型；此处按"最小实现"提供，供 display-proto 的序列化
//! 便捷函数及后续消费方（mupcd / local-display）复用，避免各侧重复定义。
//!
//! v2.0 新增 [`Error::FrameTooLarge`] / [`Error::ProtoVersionMismatch`] / [`Error::Envelope`]：
//! 把「版本不一致必须拒绝、不得静默坏值」（设计 §3.5 / PRD §4.4.1）、「畸形帧防护」
//! 与「控制信封非法即拒绝」（设计 §3.3 管线第 2 步）的**口径**固化在契约层单点，
//! 两侧（发布方 / 订阅方 / 测试桩）共用同一判据，避免各写一份而漂移。

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

    /// 帧体超过上限（设计 §3.5 条 3：畸形帧防护，客户端对帧大小设上限 64 KB）。
    ///
    /// **同源同值**：解码（[`crate::DisplayFrame::from_json_slice`]）与编码
    /// （[`crate::DisplayFrame::to_json_slice`]）共用 [`crate::MAX_FRAME_BYTES`] 单一常量，
    /// 发布方可在发包前自检，不必等对端整帧丢弃。
    #[error("display-proto frame too large: {len} bytes > limit {limit}")]
    FrameTooLarge { len: usize, limit: usize },

    /// 单条告警消息超过长度上限（编码侧自检；防止 10 条长文把整帧顶出 [`crate::MAX_FRAME_BYTES`]
    /// 后被对端静默丢弃，且发布方无法定位责任字段）。
    #[error(
        "display-proto alarm message too long: item {index} is {len} bytes > limit {limit}"
    )]
    AlarmMessageTooLong {
        /// 越界条目在 `alarms.items` 中的下标（发布方据此定位）。
        index: usize,
        /// 实际字节数（UTF-8）。
        len: usize,
        /// 上限（[`crate::MAX_ALARM_MESSAGE_BYTES`]）。
        limit: usize,
    },

    /// 配置校验失败（[`crate::DisplayConfig::validate`]；非法配置**拒绝启动**而非静默容忍）。
    ///
    /// 结构化给出 `field`（稳定键，如 `display.range.current_max_a`）与 `reason`（具体原因），
    /// 调用方可 `match` 本变体定位，而非只能 `to_string()` 打日志。
    #[error("display-proto invalid config at `{field}`: {reason}")]
    InvalidConfig {
        /// 稳定字段键（如 `display.publish_ms`）。
        field: String,
        /// 具体原因（含实际取值）。
        reason: String,
    },

    /// 帧协议版本与契约 [`crate::PROTO_VERSION`] 不一致（设计 §3.5 条 1 / PRD §4.4.1：
    /// 版本不一致必须显式拒绝，**不得静默按旧语义展示**）。
    #[error("display-proto protocol version mismatch: got {got}, expected {expected}")]
    ProtoVersionMismatch { got: u8, expected: u8 },

    /// 控制通道信封非法（设计 §3.3 管线第 2 步：request_id / 时间窗 / op 校验）。
    #[error("display-proto control envelope invalid: {0}")]
    Envelope(#[from] crate::control::ControlEnvelopeError),
}
