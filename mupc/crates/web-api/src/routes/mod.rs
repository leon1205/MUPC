//! Web API 路由

pub mod ai;
pub mod config;
pub mod logs;
pub mod mode;
pub mod status;

pub use config::ConfigHandler;
pub use logs::LogsHandler;
pub use status::StatusHandler;
