//! Web API 路由

pub mod config;
pub mod status;
pub mod logs;
pub mod ai;

pub use config::{ConfigRouter, ConfigHandler};
pub use status::{StatusRouter, StatusHandler};
pub use logs::{LogsRouter, LogsHandler};
pub use ai::ai_routes;