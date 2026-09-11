//! # mupc_display_proto
//!
//! MUPC 本地显示终端（12-MUPC，触摸式本地 HMI）的**跨进程契约单一真源**。
//!
//! 承载（对齐 `[DESIGN_APPROVED]` 设计文档 §2.2 / §3）：
//! - [`frame`]：读通道帧模型（设计 §3.1）——`DisplayFrame` v2（v1 段 + `device` / `alarms` /
//!   `info` / `interlock` 四新段）及 `Field` / `FieldFlag` / `RunState` / `SocSource` /
//!   `LinkState` / `ControlSource` / `AlarmLevel` / `ServiceScope` 等。mupcd（发布方）与
//!   mupc-local-display（订阅方）及测试桩共享。
//! - [`control`]：控制通道（设计 §3.3 / §3.4）——统一信封 `ControlRequest` / 回执
//!   `ControlResponse` / 错误码 `ControlCode` / 幂等键 / 端点清单 / 配置字段元数据。
//! - [`interlock`]：安全 / 联锁（设计 §4.6）——`InterlockApi` trait + `InterlockView` /
//!   `InterlockStatus` / `InterlockSourceStatus` + 结构化 `InterlockReject` + 写载荷 / 回执。
//! - [`log`]：日志 DTO（设计 §4.4）。
//! - [`audit`]：审计 DTO（设计 §4.5）。
//! - [`config`]：`DisplayConfig` / `DisplayRange` / `LogLimits` 结构定义 + 默认常量（设计 §7.1 / §8.3）。
//! - [`error`]：JSON/IO/版本/信封轻量错误类型。
//!
//! **依赖纪律**：契约层零 UI 依赖——只依赖 serde / serde_json / thiserror / async-trait，
//! 不引入 LVGL / FFI / serde_yaml / 重型服务 crate（设计 §5.1 注）。
//! `async-trait` 为落设计 §4.6 的 `InterlockApi`（`dyn` 擦除下的 async 方法）所必需，
//! 是过程宏而非 UI 依赖（PM 裁定）。

pub mod audit;
pub mod config;
pub mod control;
pub mod error;
pub mod frame;
pub mod interlock;
pub mod log;

pub use crate::audit::{AuditPage, AuditResult, ConsoleAuditEntry, ConsoleOp, OpOption, AUDIT_PAGE_SIZE};
pub use crate::config::{
    DisplayConfig, DisplayRange, LogLimits, DEFAULT_ALARM_PAGE_SIZE, DEFAULT_ALARM_POLL_MS,
    DEFAULT_BIND, DEFAULT_CHANNEL_URL, DEFAULT_CONTROL_BASE_URL, DEFAULT_CONTROL_BIND,
    DEFAULT_DEVICE_POLL_MS, DEFAULT_INTERLOCK_POLL_MS, DEFAULT_MIN_PUBLISH_INTERVAL_MS,
    DEFAULT_PUBLISH_MS, MAX_DEVICE_POLL_MS, MAX_SLOW_POLL_MS, MIN_LIVE_RING, MIN_MERGE_WINDOW_MS,
    MIN_PUBLISH_MS,
};
pub use crate::control::{
    ConfigField, ConfigGroup, ConfigKind, ConfigPatch, ConfigView, ConsoleEndpoint, ConsoleMethod,
    ControlCode, ControlEnvelopeError, ControlRequest, ControlResponse, FieldError,
    IdempotencyKey, OptionItem, PatchSource, WriteMode, AUDIT_FAIL_CLOSED, CONSOLE_OPERATOR,
    IDEMPOTENCY_CAPACITY, IDEMPOTENCY_TTL_MS, REPLAY_WINDOW_MS,
};
pub use crate::error::{Error, Result};
pub use crate::frame::{
    AlarmItem, AlarmLevel, AlarmsSection, ControlSource, DeviceSection, DisplayFrame, Field,
    FieldFlag, InfoSection, InterlockSection, InterlockSourceItem, LinkState, RunState,
    ServiceScope, SocSource, DEFAULT_STALE_MS, LATEST_PATH, MAX_ALARM_MESSAGE_BYTES,
    MAX_FRAME_BYTES, PROTO_VERSION,
};
pub use crate::interlock::{
    InterlockApi, InterlockOpAck, InterlockOpPayload, InterlockReject, InterlockSourceStatus,
    InterlockStatus, InterlockView,
};
pub use crate::log::{LogEntry, LogLevel, LogPage, LogRange};
