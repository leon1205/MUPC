//! # mupc_display_proto
//!
//! MUPC 本地显示终端（12-MUPC，HDMI 屏运行状态展示）的**跨进程契约单一真源**。
//!
//! 承载（对齐 `[DESIGN_APPROVED]` 设计文档）：
//! - [`frame`]：数据通道帧模型（设计 §3.3）——`DisplayFrame` / `Field` / `FieldFlag` /
//!   `RunState` / `SocSource` 及帧协议常量。mupcd（发布方）与 mupc-local-display（订阅方）
//!   及测试桩共享。
//! - [`config`]：`DisplayConfig` / `DisplayRange` 结构定义 + 默认常量（设计 §7.1）。
//! - [`error`]：JSON/IO 轻量错误类型。
//!
//! 依赖刻意保持最小（serde / serde_json / thiserror），不引入 serde_yaml 或重型服务 crate。

pub mod config;
pub mod error;
pub mod frame;

pub use crate::config::{DisplayConfig, DisplayRange, DEFAULT_BIND, DEFAULT_CHANNEL_URL, DEFAULT_PUBLISH_MS};
pub use crate::error::{Error, Result};
pub use crate::frame::{
    DisplayFrame, Field, FieldFlag, RunState, SocSource, DEFAULT_STALE_MS, PROTO_VERSION,
};
