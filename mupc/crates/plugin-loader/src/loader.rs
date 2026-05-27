//! 插件加载器实现

use crate::errors::LoaderError;
use device_trait::{Plugin, PluginError, PluginLoader, PluginMeta};
use libloading::{Library, Symbol};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// 插件句柄
struct PluginHandle {
    /// 插件实例
    plugin: Arc<dyn Plugin>,
    /// 动态库
    _library: Library,
    /// 插件元信息
    meta: PluginMeta,
}

/// 插件加载器实现
pub struct PluginLoaderImpl {
    /// 已加载插件
    plugins: RwLock<HashMap<String, PluginHandle>>,
    /// 插件搜索路径
    search_paths: RwLock<Vec<String>>,
}

impl PluginLoaderImpl {
    /// 创建新的插件加载器
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            search_paths: RwLock::new(Vec::new()),
        }
    }

    /// 添加插件搜索路径
    pub fn add_search_path(&self, path: impl Into<String>) {
        self.search_paths.write().push(path.into());
    }

    /// 查找插件路径
    fn find_plugin_path(&self, plugin_name: &str) -> Option<std::path::PathBuf> {
        let paths = self.search_paths.read();
        for path in paths.iter() {
            let plugin_path = Path::new(path).join(plugin_name);
            if plugin_path.exists() {
                return Some(plugin_path);
            }
        }
        None
    }
}

impl Default for PluginLoaderImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginLoader for PluginLoaderImpl {
    fn load(
        &self,
        plugin_path: &str,
        _config: serde_json::Value,
    ) -> Result<(), PluginError> {
        let path = Path::new(plugin_path);
        let plugin_name = path.file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| PluginError::load_failed("无效的插件路径"))?
            .to_string();

        // 检查插件是否已加载
        {
            let plugins = self.plugins.read();
            if plugins.contains_key(&plugin_name) {
                return Err(PluginError::load_failed(format!(
                    "插件 {} 已加载",
                    plugin_name
                )));
            }
        }

        // 加载动态库
        let library = unsafe { Library::new(plugin_path) }
            .map_err(|e| PluginError::LoadFailed(format!("加载动态库失败: {}", e)))?;

        // 获取插件符号 - 使用 unsafe 块
        let create_fn: Symbol<unsafe extern "C" fn() -> *mut dyn Plugin> = unsafe {
            library.get(b"create_plugin")
                .map_err(|e| PluginError::LoadFailed(format!("create_plugin: {}", e)))?
        };

        let meta_fn: Symbol<unsafe extern "C" fn() -> PluginMeta> = unsafe {
            library.get(b"plugin_meta")
                .map_err(|e| PluginError::LoadFailed(format!("plugin_meta: {}", e)))?
        };

        // 创建插件实例
        let plugin_ptr = unsafe { create_fn() };
        if plugin_ptr.is_null() {
            return Err(PluginError::load_failed("插件创建返回空指针"));
        }

        let plugin = unsafe { Arc::from_raw(plugin_ptr) };
        let meta = unsafe { meta_fn() };

        // 存储插件
        let handle = PluginHandle {
            plugin,
            _library: library,
            meta,
        };

        self.plugins.write().insert(plugin_name, handle);

        Ok(())
    }

    fn unload(&self, plugin_name: &str) -> Result<(), PluginError> {
        let mut plugins = self.plugins.write();
        if let Some(_handle) = plugins.remove(plugin_name) {
            // handle 被 drop，library 会被卸载
            Ok(())
        } else {
            Err(PluginError::not_found(plugin_name))
        }
    }

    fn list(&self) -> Vec<PluginMeta> {
        let plugins = self.plugins.read();
        plugins.values().map(|h| h.meta.clone()).collect()
    }

    fn get(&self, plugin_name: &str) -> Option<Arc<dyn Plugin>> {
        let plugins = self.plugins.read();
        plugins.get(plugin_name).map(|h| h.plugin.clone())
    }

    fn is_loaded(&self, plugin_name: &str) -> bool {
        self.plugins.read().contains_key(plugin_name)
    }

    fn plugin_count(&self) -> usize {
        self.plugins.read().len()
    }

    fn unload_all(&self) -> Result<(), PluginError> {
        let plugins = self.plugins.write();
        let names: Vec<String> = plugins.keys().cloned().collect();
        drop(plugins);

        for name in names {
            self.unload(&name)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_loader_creation() {
        let loader = PluginLoaderImpl::new();
        assert_eq!(loader.plugin_count(), 0);
    }

    #[test]
    fn test_plugin_loader_add_search_path() {
        let loader = PluginLoaderImpl::new();
        loader.add_search_path("/path/to/plugins");
        assert!(loader.plugin_count() == 0);
    }

    #[test]
    fn test_plugin_loader_is_loaded() {
        let loader = PluginLoaderImpl::new();
        assert!(!loader.is_loaded("test-plugin"));
    }

    #[test]
    fn test_plugin_loader_list_empty() {
        let loader = PluginLoaderImpl::new();
        let list = loader.list();
        assert!(list.is_empty());
    }
}