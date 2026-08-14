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

// ============================================================================
// MqttBridge trait - MQTT 桥接接口
// ============================================================================

/// MQTT 桥接 trait
///
/// 定义 MQTT 客户端与设备之间的桥接接口
#[async_trait::async_trait]
pub trait MqttBridge: Send + Sync {
    /// 连接到 MQTT Broker
    async fn connect(&mut self) -> Result<(), PluginError>;

    /// 断开连接
    async fn disconnect(&mut self) -> Result<(), PluginError>;

    /// 发布消息
    async fn publish(&self, topic: &str, payload: &[u8], qos: u8) -> Result<(), PluginError>;

    /// 订阅主题
    async fn subscribe(&self, topic: &str, qos: u8) -> Result<(), PluginError>;

    /// 是否已连接
    fn is_connected(&self) -> bool;

    /// 获取 Bridge 名称
    fn name(&self) -> &'static str;
}

// Re-export commonly used types
pub use device::{Device, DeviceCommand};
pub use errors::{BusError, DeviceError, PluginError, RegistryError};
// 抽象层预留：MessageBus/DeviceRegistry 未接线，被简化实现（SouthCommandDispatcher + Rs485Device）取代
pub use message_bus::{MessageBus, MessageHandler, NoOpMessageHandler};
pub use plugin::{NoOpPlugin, Plugin, PluginState};
pub use plugin_loader::PluginLoader;
pub use registry::DeviceRegistry;
pub use south_device::{
    ChargerHandler, HplcConfig, HplcDriver, HplcError, InverterHandler, ModbusHandler,
    ProtocolHandler, ProtocolHandlerRegistry, SouthDevice, TtuHandler,
};
pub use types::{
    crc16_modbus, CrcMode, DataFrame, DataQuality, DeviceStatus, DeviceType, Message, Parity,
    PluginMeta, Rs485Config, Topic,
};
