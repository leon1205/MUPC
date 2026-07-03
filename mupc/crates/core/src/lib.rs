//! MUPC 核心层
//!
//! 提供插件加载、设备抽象、消息总线、服务协调等核心基础设施

pub mod device_registry;
pub mod message_bus;
pub mod message_bus_impl;
pub mod plugin_loader;
pub mod service_coord;
pub mod service_coord_impl;

pub use device_registry::{
    ControlCommand, ControlResponse, DataQuality, Device, DeviceError, DeviceRegistry,
    HealthStatus, ReadRequest, ReadResponse, Value, WriteRequest, WriteResponse,
};
pub use message_bus::{Message, MessageBus, MessageHandler, Topic};
pub use message_bus_impl::TokioMessageBus;
pub use plugin_loader::{PluginInstance, PluginLoader};
pub use service_coord::{ServiceCoordinator, ServiceStatus};
pub use service_coord_impl::{ServiceCoordinatorImpl, ServiceInfo};
