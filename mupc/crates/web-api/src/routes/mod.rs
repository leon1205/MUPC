//! Web API 路由

pub mod ai;
pub mod config;
pub mod status;
pub mod logs;
pub mod mode;

pub use config::ConfigHandler;
pub use status::StatusHandler;
pub use logs::LogsHandler;