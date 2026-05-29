//! MUPC Web API 层
//!
//! 提供 Web UI 配置管理、日志查看、状态监控等功能

pub mod routes;
pub mod auth;
pub mod ws;
pub mod sse;
pub mod audit;

pub use auth::{AuthHandler, SessionManager};
pub use ws::WsLogStreamer;
pub use sse::SsePushService;
pub use audit::AuditLogger;