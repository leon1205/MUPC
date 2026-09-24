//! MQTT 网桥模块
//!
//! Phase 3B 实现分层 MQTT 架构
//! - LocalMqttClient: 连接本地 mosquitto (进程间通信)
//! - NorthMqttClient: 连接 emqx (北向通信)

pub mod client;
pub mod config;
pub mod error;
pub mod local_client;
pub mod north_client;
pub mod topics;

pub use config::{LocalMqttConfig, MqttConfig, NorthMqttConfig, ReconnectConfig, TlsConfig};
pub use error::MqttBridgeError;
// ⚠️ `NORTH_TELEMETRY` 已废弃（§9.3.4 改为 `north_telemetry(station_id)` 函数）。
// 这里 re-export 是**兼容性**动作（既有调用方仍可 `use mqtt_bridge::NORTH_TELEMETRY`），
// 而非鼓励使用 ⇒ 就地 `allow(deprecated)`，不把废弃告警转嫁给下游。
#[allow(deprecated)]
pub use topics::{
    north_event, north_telemetry, LOCAL_AI_READY, LOCAL_STRATEGY_COMMAND, LOCAL_TELEMETRY,
    NORTH_FAULT, NORTH_STATUS, NORTH_STRATEGY_COMMAND, NORTH_TELEMETRY,
};
// MqttBridge trait 迁移至 device-trait，在此 re-export 以保持兼容性
pub use device_trait::MqttBridge;
pub use local_client::LocalMqttClient;
pub use north_client::{CertExpiry, NorthMqttClient, CERT_EXPIRY_WARN_DAYS};
