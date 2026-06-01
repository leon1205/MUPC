//! 插件加载器
//!
//! Phase 1 仅定义接口，Phase 2 使用 libloading 实现

use mupc_common::MupcError;
use std::any::Any;

/// 插件实例
pub trait PluginInstance: Send + Sync {
    /// 获取插件名称
    fn name(&self) -> &str;

    /// 获取插件版本
    fn version(&self) -> &str;

    /// 初始化插件
    fn init(&mut self) -> Result<(), MupcError>;

    /// 获取插件元数据
    fn as_any(&self) -> &dyn Any;
}

/// 插件加载器 trait
pub trait PluginLoader: Send + Sync {
    /// 加载插件
    fn load(&self, path: &str) -> Result<Box<dyn PluginInstance>, MupcError>;

    /// 卸载插件
    fn unload(&self, id: &str) -> Result<(), MupcError>;

    /// 获取已加载插件列表
    fn list_loaded(&self) -> Vec<String>;

    /// 检查插件是否已加载
    fn is_loaded(&self, id: &str) -> bool;
}