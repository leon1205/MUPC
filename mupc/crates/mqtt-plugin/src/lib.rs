//! MQTT 北向插件
//!
//! 实现 MQTT 协议客户端，支持 TLS 加密和 QoS 0/1/2

mod client;
mod config;
mod errors;

pub use client::{MqttClient, MqttClientState};
pub use config::{MqttConfig, MqttQos};
pub use errors::MqttError;

use device_trait::errors::PluginError;
use device_trait::plugin::Plugin;
use device_trait::types::PluginMeta;

/// MQTT 连接状态
#[derive(Debug, Clone, PartialEq)]
pub enum MqttConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl std::fmt::Display for MqttConnectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MqttConnectionState::Disconnected => write!(f, "Disconnected"),
            MqttConnectionState::Connecting => write!(f, "Connecting"),
            MqttConnectionState::Connected => write!(f, "Connected"),
            MqttConnectionState::Error(s) => write!(f, "Error: {}", s),
        }
    }
}

// ============================================================================
// Plugin trait 实现
// ============================================================================

/// MQTT 插件
pub struct MqttPlugin {
    meta: PluginMeta,
}

impl MqttPlugin {
    /// 创建新的 MQTT 插件
    pub fn new() -> Self {
        Self {
            meta: PluginMeta::new(
                "mqtt-plugin",
                "0.1.0",
                "MUPC Team",
                "MQTT northbound communication plugin",
            ),
        }
    }
}

impl Default for MqttPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for MqttPlugin {
    fn meta(&self) -> PluginMeta {
        self.meta.clone()
    }

    fn init(&self, _config: serde_json::Value) -> Result<(), PluginError> {
        Ok(())
    }

    fn start(&self) -> Result<(), PluginError> {
        Ok(())
    }

    fn stop(&self) -> Result<(), PluginError> {
        Ok(())
    }

    fn shutdown(self: Box<Self>) -> Result<(), PluginError> {
        Ok(())
    }
}

// ============================================================================
// FFI 入口点（用于动态加载）
// ============================================================================

/// 创建 MQTT 插件实例（FFI 入口点）
///
/// # Safety
/// - 必须通过 Box::from_raw 释放返回的指针
/// - 同一插件实例不能同时被多个线程使用
#[no_mangle]
#[allow(improper_ctypes_definitions)]
pub unsafe extern "C" fn create_plugin() -> *mut dyn Plugin {
    let plugin = MqttPlugin::new();
    Box::into_raw(Box::new(plugin)) as *mut dyn Plugin
}

/// 获取插件元信息（FFI 入口点）
#[no_mangle]
#[allow(improper_ctypes_definitions)]
pub unsafe extern "C" fn plugin_meta() -> PluginMeta {
    MqttPlugin::new().meta()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mqtt_qos() {
        assert_eq!(MqttQos::AtMostOnce as u8, 0);
        assert_eq!(MqttQos::AtLeastOnce as u8, 1);
        assert_eq!(MqttQos::ExactlyOnce as u8, 2);
    }

    #[test]
    fn test_connection_state_display() {
        assert_eq!(MqttConnectionState::Disconnected.to_string(), "Disconnected");
        assert_eq!(MqttConnectionState::Connected.to_string(), "Connected");
    }

    #[test]
    fn test_mqtt_plugin_meta() {
        let plugin = MqttPlugin::new();
        let meta = plugin.meta();
        assert_eq!(meta.name, "mqtt-plugin");
        assert_eq!(meta.version, "0.1.0");
    }
}