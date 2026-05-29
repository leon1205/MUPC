//! device-trait - 南向通信设备抽象层
//!
//! 提供南向设备的统一抽象，包括 Device、DeviceRegistry、MessageBus 等核心 trait
//!
//! # 模块
//!
//! - [`device`] - 设备抽象接口
//! - [`registry`] - 设备注册表接口
//! - [`message_bus`] - 消息总线接口
//! - [`plugin`] - 插件接口
//! - [`plugin_loader`] - 插件加载器接口
//! - [`types`] - 公共类型定义
//! - [`errors`] - 错误类型定义

pub mod device;
pub mod errors;
pub mod message_bus;
pub mod plugin;
pub mod plugin_loader;
pub mod registry;
pub mod south_device;
pub mod types;

// Re-export commonly used types
pub use device::{Device, DeviceCommand};
pub use errors::{BusError, DeviceError, PluginError, RegistryError};
pub use message_bus::{MessageBus, MessageHandler, NoOpMessageHandler};
pub use plugin::{NoOpPlugin, Plugin, PluginState};
pub use plugin_loader::PluginLoader;
pub use registry::DeviceRegistry;
pub use south_device::{
    ChargerHandler, HplcConfig, HplcDriver, HplcError, InverterHandler, ModbusHandler,
    ProtocolHandler, ProtocolHandlerRegistry, SouthDevice, TtuHandler,
};
pub use types::{
    CrcMode, DataFrame, DataQuality, DeviceStatus, DeviceType, Message, Parity, PluginMeta,
    Rs485Config, Topic, crc16_modbus,
};