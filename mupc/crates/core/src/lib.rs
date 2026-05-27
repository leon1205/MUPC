//! MUPC 核心层
//!
//! 提供插件加载、设备抽象、消息总线、服务协调等核心基础设施

pub mod plugin_loader;
pub mod device_registry;
pub mod message_bus;
pub mod service_coord;

pub use plugin_loader::{PluginLoader, PluginInstance};
pub use device_registry::{DeviceRegistry, Device, DeviceError, ReadRequest, ReadResponse, WriteRequest, WriteResponse, ControlCommand, ControlResponse, HealthStatus, DataQuality, Value};
pub use message_bus::{MessageBus, Message, Topic};
pub use service_coord::{ServiceCoordinator, ServiceStatus};