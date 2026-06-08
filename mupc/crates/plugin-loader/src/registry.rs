//! 插件注册表
//!
//! 管理已注册插件的元信息和生命周期

use crate::errors::LoaderError;
use device_trait::PluginMeta;
use parking_lot::RwLock;
use std::collections::HashMap;

/// 插件注册表条目
#[derive(Debug, Clone)]
pub struct PluginEntry {
    /// 插件名称
    pub name: String,
    /// 插件元信息
    pub meta: PluginMeta,
    /// 插件路径
    pub path: String,
    /// 插件状态
    pub state: PluginState,
}

/// 插件状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// 初始状态
    Loaded,
    /// 已初始化
    Initialized,
    /// 运行中
    Running,
    /// 已停止
    Stopped,
    /// 已卸载
    Unloaded,
}

impl PluginEntry {
    /// 创建新的插件条目
    pub fn new(name: String, meta: PluginMeta, path: String) -> Self {
        Self {
            name,
            meta,
            path,
            state: PluginState::Loaded,
        }
    }

    /// 检查插件是否可用
    pub fn is_available(&self) -> bool {
        matches!(self.state, PluginState::Initialized | PluginState::Running)
    }
}

/// 插件注册表
pub struct PluginRegistry {
    /// 已注册插件
    entries: RwLock<HashMap<String, PluginEntry>>,
}

impl PluginRegistry {
    /// 创建新的插件注册表
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// 注册插件
    pub fn register(
        &self,
        name: String,
        meta: PluginMeta,
        path: String,
    ) -> Result<(), LoaderError> {
        let mut entries = self.entries.write();
        if entries.contains_key(&name) {
            return Err(LoaderError::load_failed(format!("插件 {} 已注册", name)));
        }

        let entry = PluginEntry::new(name.clone(), meta, path);
        entries.insert(name, entry);
        Ok(())
    }

    /// 注销插件
    pub fn unregister(&self, name: &str) -> Result<(), LoaderError> {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.remove(name) {
            if entry.state == PluginState::Running {
                return Err(LoaderError::unload_failed(format!(
                    "插件 {} 正在运行，无法注销",
                    name
                )));
            }
            Ok(())
        } else {
            Err(LoaderError::not_found(name))
        }
    }

    /// 获取插件条目
    pub fn get(&self, name: &str) -> Option<PluginEntry> {
        let entries = self.entries.read();
        entries.get(name).cloned()
    }

    /// 获取所有插件名称
    pub fn names(&self) -> Vec<String> {
        let entries = self.entries.read();
        entries.keys().cloned().collect()
    }

    /// 按状态查询插件
    pub fn query_by_state(&self, state: PluginState) -> Vec<PluginEntry> {
        let entries = self.entries.read();
        entries
            .values()
            .filter(|e| e.state == state)
            .cloned()
            .collect()
    }

    /// 更新插件状态
    pub fn update_state(&self, name: &str, state: PluginState) -> Result<(), LoaderError> {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(name) {
            entry.state = state;
            Ok(())
        } else {
            Err(LoaderError::not_found(name))
        }
    }

    /// 获取插件总数
    pub fn count(&self) -> usize {
        let entries = self.entries.read();
        entries.len()
    }

    /// 清除所有插件
    pub fn clear(&self) {
        let mut entries = self.entries.write();
        entries.clear();
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_entry() -> PluginEntry {
        let meta = PluginMeta::new("test-plugin", "1.0.0", "Test Author", "A test plugin");
        PluginEntry::new(
            "test-plugin".to_string(),
            meta,
            "/path/to/test-plugin.so".to_string(),
        )
    }

    #[test]
    fn test_registry_creation() {
        let registry = PluginRegistry::new();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_registry_register() {
        let registry = PluginRegistry::new();
        let entry = create_test_entry();
        let meta = entry.meta.clone();

        registry
            .register(
                "test-plugin".to_string(),
                meta,
                "/path/to/plugin.so".to_string(),
            )
            .unwrap();
        assert_eq!(registry.count(), 1);
    }

    #[test]
    fn test_registry_register_duplicate() {
        let registry = PluginRegistry::new();
        let entry = create_test_entry();
        let meta = entry.meta.clone();

        registry
            .register(
                "test-plugin".to_string(),
                meta.clone(),
                "/path/to/plugin.so".to_string(),
            )
            .unwrap();

        let result = registry.register(
            "test-plugin".to_string(),
            meta,
            "/path/to/plugin.so".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_registry_unregister() {
        let registry = PluginRegistry::new();
        let entry = create_test_entry();
        let meta = entry.meta.clone();

        registry
            .register(
                "test-plugin".to_string(),
                meta,
                "/path/to/plugin.so".to_string(),
            )
            .unwrap();
        assert_eq!(registry.count(), 1);

        registry.unregister("test-plugin").unwrap();
        assert_eq!(registry.count(), 0);
    }

    #[test]
    fn test_registry_get() {
        let registry = PluginRegistry::new();
        let entry = create_test_entry();
        let meta = entry.meta.clone();

        registry
            .register(
                "test-plugin".to_string(),
                meta,
                "/path/to/plugin.so".to_string(),
            )
            .unwrap();

        let retrieved = registry.get("test-plugin");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test-plugin");
    }

    #[test]
    fn test_registry_names() {
        let registry = PluginRegistry::new();

        let meta1 = PluginMeta::new("plugin1", "1.0.0", "Author", "Plugin 1");
        let meta2 = PluginMeta::new("plugin2", "1.0.0", "Author", "Plugin 2");

        registry
            .register(
                "plugin1".to_string(),
                meta1,
                "/path/to/plugin1.so".to_string(),
            )
            .unwrap();
        registry
            .register(
                "plugin2".to_string(),
                meta2,
                "/path/to/plugin2.so".to_string(),
            )
            .unwrap();

        let names = registry.names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"plugin1".to_string()));
        assert!(names.contains(&"plugin2".to_string()));
    }

    #[test]
    fn test_plugin_entry_is_available() {
        let entry = create_test_entry();
        // Initial state is Loaded, which is NOT Available
        assert!(!entry.is_available());

        let mut running_entry = entry;
        running_entry.state = PluginState::Running;
        assert!(running_entry.is_available());
    }
}
