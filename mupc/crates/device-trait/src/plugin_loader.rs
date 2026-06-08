//! 插件加载器接口
//!
//! 定义插件的动态加载、卸载、查询能力

use crate::errors::PluginError;
use crate::plugin::Plugin;
use crate::types::PluginMeta;
use std::sync::Arc;

/// 插件加载器接口
///
/// 提供插件的动态加载、卸载、查询能力
pub trait PluginLoader: Send + Sync {
    /// 加载插件
    ///
    /// # Arguments
    /// - `plugin_path`: 插件路径
    /// - `config`: 插件配置（JSON 格式）
    ///
    /// # Returns
    /// - `Ok(())`: 加载成功
    /// - `Err(PluginError)`: 加载失败
    fn load(&self, plugin_path: &str, config: serde_json::Value) -> Result<(), PluginError>;

    /// 卸载插件
    ///
    /// # Arguments
    /// - `plugin_name`: 插件名称
    ///
    /// # Returns
    /// - `Ok(())`: 卸载成功
    /// - `Err(PluginError)`: 卸载失败
    fn unload(&self, plugin_name: &str) -> Result<(), PluginError>;

    /// 获取已加载插件列表
    ///
    /// # Returns
    /// 已加载插件的元信息列表
    fn list(&self) -> Vec<PluginMeta>;

    /// 获取插件实例
    ///
    /// # Arguments
    /// - `plugin_name`: 插件名称
    ///
    /// # Returns
    /// - `Some(Arc<dyn Plugin>)`: 插件存在
    /// - `None`: 插件不存在
    fn get(&self, plugin_name: &str) -> Option<Arc<dyn Plugin>>;

    /// 检查插件是否已加载
    fn is_loaded(&self, plugin_name: &str) -> bool;

    /// 获取已加载插件数量
    fn plugin_count(&self) -> usize;

    /// 卸载所有插件
    fn unload_all(&self) -> Result<(), PluginError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_loader_trait_exists() {
        // 验证 trait 可以被用作 trait bound
        fn check_plugin_loader<T: PluginLoader>() {}
        // 此函数不会实际调用，只是验证编译通过
    }
}
