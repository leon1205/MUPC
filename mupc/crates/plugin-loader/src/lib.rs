//! plugin-loader - 动态插件加载器
//!
//! 实现插件的动态加载、卸载、生命周期管理
//!
//! # 模块
//!
//! - [`loader`] - 插件加载器实现
//! - [`registry`] - 插件注册表
//! - [`errors`] - 错误类型定义

pub mod errors;
pub mod loader;
pub mod registry;

// Re-export commonly used types
pub use errors::LoaderError;
pub use loader::PluginLoaderImpl;
pub use registry::{PluginEntry, PluginRegistry, PluginState};