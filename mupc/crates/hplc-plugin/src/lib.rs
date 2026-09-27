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
pub use device::HplcDevice;
pub use device_trait::south_device::{HplcConfig, HplcError};
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
    // ⚠️ `dyn Plugin` / `PluginMeta` 非 C ABI 安全类型（`improper_ctypes_definitions`）。
    // 这是**本项目的插件 ABI 约定**（见 CLAUDE.md「插件系统」）：导出与加载**两端都是 Rust**
    // 且同编译器/同 target，`*mut dyn Plugin` 作为不透明句柄使用；改为真 C ABI（vtable 手写）
    // 属插件系统重构，不在 lint 清理范围。显式放行并在此登记。
    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    pub unsafe extern "C" fn create_plugin() -> *mut dyn Plugin {
        let plugin = HplcPlugin::new();
        // SAFETY: 本函数为 `unsafe fn`，**契约由调用方（loader）承担**，本体内不 deref
        // 任何外来指针：`Box::into_raw` 交出堆实例所有权（不 drop），返回的 fat pointer
        // （数据指针 + vtable 指针）恒非空、对齐、指向有效对象。调用方须保证：
        // ① 只经 [`destroy_hplc_plugin`] 归还（类型须为 `*mut HplcPlugin`，用 `dyn` 释放
        //    会因 vtable/尺寸不符而 UB —— 故"导出与加载两端都是 Rust、同编译器/同 target"
        //    是本项目插件 ABI 的前提）；② 不跨进程/不跨动态库边界传递；
        // ③ 同一指针不同时被多线程使用。
        Box::into_raw(Box::new(plugin)) as *mut dyn Plugin
    }

    /// 销毁 HPLC 插件实例（FFI 入口点）
    ///
    /// # Safety
    /// - 必须与 create_plugin 配对使用
    /// - 调用后指针无效，不能再使用
    #[allow(improper_ctypes_definitions)]
    #[no_mangle]
    pub unsafe extern "C" fn destroy_hplc_plugin(ptr: *mut dyn Plugin) {
        if !ptr.is_null() {
            // SAFETY: 前置条件（调用方保证）—— `ptr` 必须是 [`create_plugin`] 返回、
            // 且**尚未销毁过**的指针（双重销毁 UB），类型确为 `HplcPlugin`。后置：
            // `Box::from_raw` 取回所有权后立即 drop，堆内存释放、vtable 槽位失效。
            // 空指针分支已先行挡掉（`Box::from_raw(null)` 是 UB）。
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
        // SAFETY: 无指针入参、无别名/对齐要求，本体内不 deref 任何外部内存 —— 标 `unsafe`
        // 仅为与同模块其余 FFI 入口点保持**同一签名口径**。调用方须保证的是返回值语义：
        // `PluginMeta` 按值返回（含 `String` 堆字段），所有权随返回值移交；跨 ABI 成立同样
        // 以"两端都是 Rust、同编译器/同 target"为前提。
        HplcPlugin::new().meta()
    }
}

#[cfg(not(feature = "ffi"))]
mod ffi {
    // FFI 功能被禁用，不提供 FFI 入口点
}

#[allow(unused_imports)]
pub use ffi::*;
