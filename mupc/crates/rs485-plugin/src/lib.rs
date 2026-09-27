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
pub use device::{unpack_bits, Rs485Device};
pub use device_trait::CrcMode;
// S3b-2 T5：具名导出校验位，供 `mupc-southd` 透传站级 `parity` 使用（设计 §11.3/§11.4.5）
// —— downstream 经本 crate 引用即可，**不必**再依赖 `device-trait`。
pub use device_trait::Parity;
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
    // SAFETY: 本函数是 `unsafe fn`，**契约由调用方（loader）承担**，本体内不 deref 任何
    // 外来指针：`Box::new` 在堆上建实例后用 `Box::into_raw` **交出所有权**（不 drop），
    // 返回的 fat pointer（数据指针 + vtable 指针）恒非空、对齐、指向有效对象。
    // 调用方须保证：① 该指针**只经 [`destroy_rs485_plugin`]** 归还（`Box::from_raw`，
    // 且类型必须是 `*mut Rs485Plugin` 而非 `dyn`，否则释放时 vtable/尺寸不符 ——
    // 这正是"两端都是 Rust、同编译器/同 target"约定必须成立的原因）；
    // ② 不跨进程/不跨动态库边界传递（fat pointer 的 vtable 地址在别的映像里无意义）；
    // ③ 同一指针不得被多个线程同时使用（本插件无内部可变共享状态，但 `dyn Plugin`
    // 本身不承诺 `Sync`）。
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
        // SAFETY: 前置条件（调用方保证）—— `ptr` 必须是 [`create_plugin`] 返回、且**尚未
        // 被销毁过**的指针（双重销毁是 UB），且类型确为 `Rs485Plugin`（见 create_plugin 的
        // 契约①）。后置：`Box::from_raw` 取回所有权后立即 drop，堆内存释放、`dyn Plugin`
        // 的 vtable 槽位不再可用。空指针分支已先行挡掉（`Box::from_raw(null)` 是 UB）。
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
    // 仅为与同文件其余 FFI 入口点保持**同一签名口径**。调用方须保证的是**返回值语义**：
    // `PluginMeta` 按值返回（含 `String` 等堆字段），跨 ABI 返回后所有权归调用方；
    // 与 create/destroy 同理，只在"两端都是 Rust、同编译器/同 target"的约定下成立。
    Rs485Plugin::new().meta()
}
