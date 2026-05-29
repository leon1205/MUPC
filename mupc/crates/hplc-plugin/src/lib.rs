//! hplc-plugin - HPLC 高速电力线载波通信驱动插件
//!
//! 实现南向 HPLC 设备通信，支持高速电力线载波通信
//!
//! # 模块
//!
//! - [`config`] - HPLC 配置解析
//! - [`device`] - HPLC 设备驱动
//! - [`driver`] - HplcDriver trait 和 MockHplcDriver
//! - [`errors`] - 错误类型定义

pub mod config;
pub mod device;
pub mod driver;
pub mod errors;
pub mod mock;

// Re-export commonly used types
pub use device_trait::south_device::{HplcConfig, HplcError};
pub use device::HplcDevice;
pub use driver::HplcDriver;
pub use mock::MockHplcDriver;

// Re-export from device-trait
pub use device_trait::{DataFrame, DeviceError, DeviceStatus, SouthDevice};

// Plugin trait and types for dynamic loading
pub use device_trait::plugin::{Plugin, PluginState};
pub use device_trait::types::PluginMeta;

// ============================================================================
// FFI 入口点（用于动态加载）
// ============================================================================

#[cfg(feature = "ffi")]
mod ffi {
    use super::*;
    use device_trait::errors::PluginError;

    /// HPLC 插件元信息
    pub struct HplcPlugin {
        meta: PluginMeta,
    }

    impl HplcPlugin {
        /// 创建新的 HPLC 插件
        pub fn new() -> Self {
            Self {
                meta: PluginMeta::new(
                    "hplc-plugin",
                    "0.1.0",
                    "MUPC Team",
                    "HPLC driver plugin for southbound communication",
                ),
            }
        }
    }

    impl Default for HplcPlugin {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Plugin for HplcPlugin {
        fn meta(&self) -> PluginMeta {
            self.meta.clone()
        }

        fn init(&self, config: serde_json::Value) -> Result<(), PluginError> {
            tracing::info!("HPLC 插件初始化: {:?}", config);
            Ok(())
        }

        fn start(&self) -> Result<(), PluginError> {
            tracing::info!("HPLC 插件启动");
            Ok(())
        }

        fn stop(&self) -> Result<(), PluginError> {
            tracing::info!("HPLC 插件停止");
            Ok(())
        }

        fn shutdown(self: Box<Self>) -> Result<(), PluginError> {
            tracing::info!("HPLC 插件关闭");
            Ok(())
        }
    }

    /// 创建 HPLC 插件实例（FFI 入口点）
    ///
    /// # Safety
    /// - 必须通过 Box::from_raw 释放返回的指针
    /// - 同一插件实例不能同时被多个线程使用
    #[no_mangle]
    pub unsafe extern "C" fn create_plugin() -> *mut dyn Plugin {
        let plugin = HplcPlugin::new();
        Box::into_raw(Box::new(plugin)) as *mut dyn Plugin
    }

    /// 销毁 HPLC 插件实例（FFI 入口点）
    ///
    /// # Safety
    /// - 必须与 create_plugin 配对使用
    /// - 调用后指针无效，不能再使用
    #[no_mangle]
    pub unsafe extern "C" fn destroy_hplc_plugin(ptr: *mut dyn Plugin) {
        if !ptr.is_null() {
            let _ = Box::from_raw(ptr);
        }
    }

    /// 获取插件元信息（FFI 入口点）
    #[no_mangle]
    pub unsafe extern "C" fn plugin_meta() -> PluginMeta {
        HplcPlugin::new().meta()
    }
}

#[cfg(not(feature = "ffi"))]
mod ffi {
    // FFI 功能被禁用，不提供 FFI 入口点
}

pub use ffi::*;