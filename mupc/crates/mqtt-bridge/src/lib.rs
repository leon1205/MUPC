//! MQTT 网桥模块
//!
//! Phase 3B 实现分层 MQTT 架构
//! - LocalMqttClient: 连接本地 mosquitto (进程间通信)
//! - NorthMqttClient: 连接 emqx (北向通信)

pub mod error;
pub mod config;
pub mod topics;
pub mod client;
pub mod local_client;
pub mod north_client;

pub use error::MqttBridgeError;
pub use config::{MqttConfig, LocalMqttConfig, NorthMqttConfig, TlsConfig};
pub use topics::{LOCAL_TELEMETRY, LOCAL_STRATEGY_COMMAND, LOCAL_AI_READY, NORTH_TELEMETRY, NORTH_FAULT, NORTH_STRATEGY_COMMAND, NORTH_STATUS};
// MqttBridge trait 迁移至 device-trait，在此 re-export 以保持兼容性
pub use device_trait::MqttBridge;
pub use local_client::LocalMqttClient;
pub use north_client::NorthMqttClient;
