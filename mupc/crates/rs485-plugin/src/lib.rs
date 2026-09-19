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
// ⚠️ `dyn Plugin` / `PluginMeta` 非 C ABI 安全类型（`improper_ctypes_definitions`）。
// 这是**本项目的插件 ABI 约定**（见 CLAUDE.md「插件系统」）：导出与加载**两端都是 Rust**
// 且同编译器/同 target，`*mut dyn Plugin` 作为不透明句柄使用；改为真 C ABI（vtable 手写）
// 属插件系统重构，不在 lint 清理范围。显式放行并在此登记。
#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub unsafe extern "C" fn create_plugin() -> *mut dyn Plugin {
    let plugin = Rs485Plugin::new();
    Box::into_raw(Box::new(plugin)) as *mut dyn Plugin
}

/// 销毁 RS485 插件实例（FFI 入口点）
///
/// # Safety
/// - 必须与 create_plugin 配对使用
#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub unsafe extern "C" fn destroy_rs485_plugin(ptr: *mut dyn Plugin) {
    if !ptr.is_null() {
        let _ = Box::from_raw(ptr);
    }
}

/// 获取插件元信息（FFI 入口点）
///
/// # Safety
/// 无指针参数、无别名要求；标 `unsafe` 仅为与其余 FFI 入口点保持同一签名口径
/// （调用方需遵守「元信息为只读值」这一约定即可）。
#[allow(improper_ctypes_definitions)]
#[no_mangle]
pub unsafe extern "C" fn plugin_meta() -> PluginMeta {
    Rs485Plugin::new().meta()
}
