//! MUPC Web API 层
//!
//! 提供 Web UI 配置管理、日志查看、状态监控等功能

pub mod routes;
pub mod auth;
pub mod ws;

pub use auth::{AuthHandler, SessionManager};
pub use ws::WsLogStreamer;