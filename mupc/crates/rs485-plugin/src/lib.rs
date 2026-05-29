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
pub use device_trait::CrcMode;
pub use device::Rs485Device;
pub use errors::Rs485Error;