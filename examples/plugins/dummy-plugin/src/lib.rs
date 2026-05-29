//! Dummy Plugin - MUPC 动态插件示例
//!
//! 这是一个最小化的插件实现示例，用于测试动态加载功能

use device_trait::{Plugin, PluginError, PluginMeta};

/// Dummy Plugin
pub struct DummyPlugin {
    meta: PluginMeta,
    initialized: bool,
}

impl DummyPlugin {
    /// 创建新的 DummyPlugin 实例
    pub fn new() -> Self {
        Self {
            meta: PluginMeta::new(
                "dummy-plugin",
                "0.1.0",
                "MUPC Team",
                "Example plugin demonstrating the FFI binding interface",
            ),
            initialized: false,
        }
    }
}

impl Default for DummyPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for DummyPlugin {
    fn meta(&self) -> PluginMeta {
        self.meta.clone()
    }

    fn init(&self, config: serde_json::Value) -> Result<(), PluginError> {
        tracing::info!("DummyPlugin 初始化中, config: {:?}", config);
        tracing::info!("DummyPlugin 初始化完成");
        Ok(())
    }

    fn start(&self) -> Result<(), PluginError> {
        tracing::info!("DummyPlugin 启动");
        Ok(())
    }

    fn stop(&self) -> Result<(), PluginError> {
        tracing::info!("DummyPlugin 停止");
        Ok(())
    }

    fn shutdown(self: Box<Self>) -> Result<(), PluginError> {
        tracing::info!("DummyPlugin 关闭");
        Ok(())
    }
}

// ========== FFI 导出函数 ==========

/// 插件工厂函数
///
/// # Safety
/// 调用者负责在不需要时正确释放插件实例
#[no_mangle]
pub unsafe extern "C" fn create_plugin() -> *mut dyn Plugin {
    let plugin = DummyPlugin::new();
    Box::into_raw(Box::new(plugin)) as *mut dyn Plugin
}

/// 获取插件元信息
#[no_mangle]
pub unsafe extern "C" fn plugin_meta() -> PluginMeta {
    DummyPlugin::new().meta()
}

/// 插件版本信息（可选扩展）
#[no_mangle]
pub unsafe extern "C" fn plugin_version() -> *const std::os::raw::c_char {
    "0.1.0\0".as_ptr() as *const std::os::raw::c_char
}