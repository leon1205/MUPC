//! plugin-loader 单元测试

use plugin_loader::{PluginLoaderImpl, PluginRegistry, PluginState};
use device_trait::{PluginLoader, PluginMeta};

#[test]
fn test_plugin_registry_creation() {
    let registry = PluginRegistry::new();
    assert_eq!(registry.count(), 0);
}

#[test]
fn test_plugin_registry_register() {
    let registry = PluginRegistry::new();
    let meta = PluginMeta::new("test-plugin", "1.0.0", "Test", "Test plugin");

    let result = registry.register("test-plugin".to_string(), meta, "/path/to/plugin.so".to_string());
    assert!(result.is_ok());
    assert_eq!(registry.count(), 1);
}

#[test]
fn test_plugin_registry_unregister() {
    let registry = PluginRegistry::new();
    let meta = PluginMeta::new("test-plugin", "1.0.0", "Test", "Test plugin");

    registry
        .register("test-plugin".to_string(), meta, "/path/to/plugin.so".to_string())
        .unwrap();

    let result = registry.unregister("test-plugin");
    assert!(result.is_ok());
    assert_eq!(registry.count(), 0);
}

#[test]
fn test_plugin_registry_get() {
    let registry = PluginRegistry::new();
    let meta = PluginMeta::new("test-plugin", "1.0.0", "Test", "Test plugin");

    registry
        .register("test-plugin".to_string(), meta, "/path/to/plugin.so".to_string())
        .unwrap();

    let entry = registry.get("test-plugin");
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().name, "test-plugin");
}

#[test]
fn test_plugin_registry_names() {
    let registry = PluginRegistry::new();

    let meta1 = PluginMeta::new("plugin1", "1.0.0", "Author", "Plugin 1");
    let meta2 = PluginMeta::new("plugin2", "1.0.0", "Author", "Plugin 2");

    registry
        .register("plugin1".to_string(), meta1, "/path/to/plugin1.so".to_string())
        .unwrap();
    registry
        .register("plugin2".to_string(), meta2, "/path/to/plugin2.so".to_string())
        .unwrap();

    let names = registry.names();
    assert_eq!(names.len(), 2);
}

#[test]
fn test_plugin_registry_query_by_state() {
    let registry = PluginRegistry::new();
    let meta = PluginMeta::new("test-plugin", "1.0.0", "Test", "Test plugin");

    registry
        .register("test-plugin".to_string(), meta, "/path/to/plugin.so".to_string())
        .unwrap();

    let entries = registry.query_by_state(PluginState::Loaded);
    assert_eq!(entries.len(), 1);

    let entries = registry.query_by_state(PluginState::Running);
    assert_eq!(entries.len(), 0);
}

#[test]
fn test_plugin_registry_update_state() {
    let registry = PluginRegistry::new();
    let meta = PluginMeta::new("test-plugin", "1.0.0", "Test", "Test plugin");

    registry
        .register("test-plugin".to_string(), meta, "/path/to/plugin.so".to_string())
        .unwrap();

    let result = registry.update_state("test-plugin", PluginState::Running);
    assert!(result.is_ok());

    let entry = registry.get("test-plugin").unwrap();
    assert_eq!(entry.state, PluginState::Running);
}

#[test]
fn test_plugin_loader_impl_new() {
    let loader = PluginLoaderImpl::new();
    assert_eq!(loader.plugin_count(), 0);
}

#[test]
fn test_plugin_loader_impl_is_loaded() {
    let loader = PluginLoaderImpl::new();
    assert!(!loader.is_loaded("nonexistent"));
}

#[test]
fn test_plugin_loader_impl_list() {
    let loader = PluginLoaderImpl::new();
    let list = loader.list();
    assert!(list.is_empty());
}

#[test]
fn test_plugin_state_values() {
    assert_eq!(PluginState::Loaded, PluginState::Loaded);
    assert_eq!(PluginState::Initialized, PluginState::Initialized);
    assert_eq!(PluginState::Running, PluginState::Running);
    assert_eq!(PluginState::Stopped, PluginState::Stopped);
    assert_eq!(PluginState::Unloaded, PluginState::Unloaded);
}