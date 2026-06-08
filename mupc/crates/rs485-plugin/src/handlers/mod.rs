//! RS485 协议处理器模块
//!
//! 从 device-trait 重新导出协议处理器（权威来源）
//!
//! - [`ModbusHandler`] - ModbusRTU 协议处理器
//! - [`TtuHandler`] - TTU 配变终端协议处理器
//! - [`InverterHandler`] - 光伏逆变器协议处理器
//! - [`ChargerHandler`] - 充电桩 GB/T 27930 协议处理器

pub use device_trait::south_device::{ChargerHandler, InverterHandler, ModbusHandler, TtuHandler};
// device-trait 中的 ProtocolHandlerRegistry 接受 Rs485Config，
// 本 crate 提供适配层将 Config 转换为 Rs485Config
use device_trait::south_device::ProtocolHandlerRegistry as DeviceProtocolHandlerRegistry;
use device_trait::ProtocolHandler;

use crate::config::Config;
use std::sync::Arc;

/// 协议处理器注册表（rs485-plugin 适配器）
///
/// 将 Config 转换为 Rs485Config 后委托给 device_trait 的 ProtocolHandlerRegistry
pub struct ProtocolHandlerRegistry;

impl ProtocolHandlerRegistry {
    /// 根据名称获取协议处理器实例
    ///
    /// # Arguments
    /// - `name`: 协议处理器名称（modbus/ttu/inverter/charger）
    /// - `config`: 设备配置（包含 device_addr 等参数）
    ///
    /// # Returns
    /// 协议处理器实例，如果名称不匹配则返回 None
    pub fn get(name: &str, config: &Config) -> Option<Arc<dyn ProtocolHandler>> {
        let rs485_config = config.to_rs485_config();
        DeviceProtocolHandlerRegistry::get(name, &rs485_config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_get_modbus() {
        let config = Config::default();
        let handler = ProtocolHandlerRegistry::get("modbus", &config);
        assert!(handler.is_some());
        assert_eq!(handler.unwrap().name(), "ModbusRTU");
    }

    #[test]
    fn test_registry_get_unknown() {
        let config = Config::default();
        let handler = ProtocolHandlerRegistry::get("unknown", &config);
        assert!(handler.is_none());
    }

    #[test]
    fn test_registry_get_all_types() {
        let config = Config::default();

        let modbus = ProtocolHandlerRegistry::get("modbus", &config);
        let ttu = ProtocolHandlerRegistry::get("ttu", &config);
        let inverter = ProtocolHandlerRegistry::get("inverter", &config);
        let charger = ProtocolHandlerRegistry::get("charger", &config);

        assert!(modbus.is_some());
        assert!(ttu.is_some());
        assert!(inverter.is_some());
        assert!(charger.is_some());

        assert_eq!(modbus.unwrap().name(), "ModbusRTU");
        assert_eq!(ttu.unwrap().name(), "TTU");
        assert_eq!(inverter.unwrap().name(), "Inverter");
        assert_eq!(charger.unwrap().name(), "ChargerGB27930");
    }
}
