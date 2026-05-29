//! Web API 路由

pub mod config;
pub mod status;
pub mod logs;
pub mod ai;

pub use config::ConfigHandler;
pub use status::StatusHandler;
pub use logs::LogsHandler;
pub use ai::ai_routes;