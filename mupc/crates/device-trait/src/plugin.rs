//! 插件接口定义
//!
//! 定义插件的生命周期和元信息

use crate::errors::PluginError;
use crate::types::PluginMeta;

/// 插件接口
///
/// 所有插件都需要实现此 trait
pub trait Plugin: Send + Sync {
    /// 获取插件元信息
    fn meta(&self) -> PluginMeta;

    /// 初始化插件
    ///
    /// # Arguments
    /// - `config`: 插件配置（JSON 格式）
    ///
    /// # Returns
    /// - `Ok(())`: 初始化成功
    /// - `Err(PluginError)`: 初始化失败
    fn init(&self, config: serde_json::Value) -> Result<(), PluginError>;

    /// 启动插件
    ///
    /// # Returns
    /// - `Ok(())`: 启动成功
    /// - `Err(PluginError)`: 启动失败
    fn start(&self) -> Result<(), PluginError>;

    /// 停止插件
    ///
    /// # Returns
    /// - `Ok(())`: 停止成功
    /// - `Err(PluginError)`: 停止失败
    fn stop(&self) -> Result<(), PluginError>;

    /// 销毁插件（释放资源）
    ///
    /// 使用 Box<Self> 确保可通过 trait object 调用
    fn shutdown(self: Box<Self>) -> Result<(), PluginError>;
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

impl PluginState {
    /// 检查插件是否可用
    pub fn is_available(&self) -> bool {
        matches!(self, PluginState::Initialized | PluginState::Running)
    }
}

/// 空插件实现
///
/// 用于测试或默认实现
pub struct NoOpPlugin {
    meta: PluginMeta,
}

impl NoOpPlugin {
    /// 创建新的空插件
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            meta: PluginMeta::new(name, version, "MUPC Team", "No-op plugin"),
        }
    }
}

impl Plugin for NoOpPlugin {
    fn meta(&self) -> PluginMeta {
        self.meta.clone()
    }

    fn init(&self, _config: serde_json::Value) -> Result<(), PluginError> {
        Ok(())
    }

    fn start(&self) -> Result<(), PluginError> {
        Ok(())
    }

    fn stop(&self) -> Result<(), PluginError> {
        Ok(())
    }

    fn shutdown(self: Box<Self>) -> Result<(), PluginError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_state_is_available() {
        assert!(!PluginState::Loaded.is_available());
        assert!(PluginState::Initialized.is_available());
        assert!(PluginState::Running.is_available());
        assert!(!PluginState::Stopped.is_available());
        assert!(!PluginState::Unloaded.is_available());
    }

    #[test]
    fn test_noop_plugin() {
        let plugin = NoOpPlugin::new("test-plugin", "1.0.0");
        let meta = plugin.meta();
        assert_eq!(meta.name, "test-plugin");
        assert_eq!(meta.version, "1.0.0");

        assert!(plugin.init(serde_json::json!({})).is_ok());
        assert!(plugin.start().is_ok());
        assert!(plugin.stop().is_ok());
        assert!(Box::new(plugin).shutdown().is_ok());
    }
}