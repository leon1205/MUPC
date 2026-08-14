//! rs485-plugin - RS485 串口通信驱动
//!
//! 实现南向 RS485 设备通信，支持 TTU、光伏逆变器、充电桩等设备
//!
//! # 模块
//!
//! - [`config`] - RS485 配置解析
//! - [`device`] - RS485 设备驱动
//! - [`errors`] - 错误类型定义
//! - [`handlers`] - 协议处理器（Modbus、TTU、逆变器、充电桩）
//! - [`protocol`] - 协议解析（Modbus RTU）

pub mod config;
pub mod device;
pub mod errors;
pub mod handlers;
pub mod protocol;

// Re-export commonly used types
pub use config::Config;
pub use device::Rs485Device;
pub use device_trait::CrcMode;
pub use errors::Rs485Error;

// ============================================================================
// FFI 入口点（用于动态加载）
// ============================================================================

use device_trait::errors::PluginError;
use device_trait::plugin::Plugin;
use device_trait::types::PluginMeta;

/// RS485 插件元信息
pub struct Rs485Plugin {
    meta: PluginMeta,
}

impl Rs485Plugin {
    pub fn new() -> Self {
        Self {
            meta: PluginMeta::new(
                "rs485-plugin",
                "0.1.0",
                "MUPC Team",
                "RS485 driver plugin for southbound communication",
            ),
        }
    }
}

impl Default for Rs485Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for Rs485Plugin {
    fn meta(&self) -> PluginMeta {
        self.meta.clone()
    }

    fn init(&self, config: serde_json::Value) -> Result<(), PluginError> {
        tracing::info!("RS485 插件初始化: {:?}", config);
        Ok(())
    }

    fn start(&self) -> Result<(), PluginError> {
        tracing::info!("RS485 插件启动");
        Ok(())
    }

    fn stop(&self) -> Result<(), PluginError> {
        tracing::info!("RS485 插件停止");
        Ok(())
    }

    fn shutdown(self: Box<Self>) -> Result<(), PluginError> {
        tracing::info!("RS485 插件关闭");
        Ok(())
    }
}

/// 创建 RS485 插件实例（FFI 入口点）
///
/// # Safety
/// - 必须通过 Box::from_raw 释放返回的指针
#[no_mangle]
pub unsafe extern "C" fn create_plugin() -> *mut dyn Plugin {
    let plugin = Rs485Plugin::new();
    Box::into_raw(Box::new(plugin)) as *mut dyn Plugin
}

/// 销毁 RS485 插件实例（FFI 入口点）
///
/// # Safety
/// - 必须与 create_plugin 配对使用
#[no_mangle]
pub unsafe extern "C" fn destroy_rs485_plugin(ptr: *mut dyn Plugin) {
    if !ptr.is_null() {
        let _ = Box::from_raw(ptr);
    }
}

/// 获取插件元信息（FFI 入口点）
#[no_mangle]
pub unsafe extern "C" fn plugin_meta() -> PluginMeta {
    Rs485Plugin::new().meta()
}
