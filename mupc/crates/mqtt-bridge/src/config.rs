//! MQTT 配置结构

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// MQTT 全局配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MqttConfig {
    pub local: LocalMqttConfig,
    pub north: NorthMqttConfig,
}

/// 本地 mosquitto 配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocalMqttConfig {
    pub broker_addr: String,
    pub client_id: String,
    pub clean_session: bool,
    pub keepalive_secs: u64,
    pub reconnect: ReconnectConfig,
}

impl Default for LocalMqttConfig {
    fn default() -> Self {
        Self {
            broker_addr: "127.0.0.1:1883".to_string(),
            client_id: "mupc-local".to_string(),
            clean_session: true,
            keepalive_secs: 60,
            reconnect: ReconnectConfig::default(),
        }
    }
}

/// 北向 emqx 配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NorthMqttConfig {
    pub broker_addr: String,
    pub client_id: String,
    pub keepalive_secs: u64,
    pub tls: TlsConfig,
    pub reconnect: ReconnectConfig,
}

/// TLS 配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    pub ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}

/// 重连配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReconnectConfig {
    /// 初始重连间隔（秒）
    pub initial_interval_secs: u64,
    /// 最大重连间隔（秒）
    pub max_interval_secs: u64,
    /// 退避乘数
    pub backoff_multiplier: f64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_interval_secs: 1,
            max_interval_secs: 60,
            backoff_multiplier: 2.0,
        }
    }
}
