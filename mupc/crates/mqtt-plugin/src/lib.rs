//! MQTT 北向插件
//!
//! 实现 MQTT 协议客户端，支持 TLS 加密和 QoS 0/1/2

mod client;
mod config;
mod errors;

pub use client::{MqttClient, MqttClientState};
pub use config::{MqttConfig, MqttQos};
pub use errors::{MqttError, Result};

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
}