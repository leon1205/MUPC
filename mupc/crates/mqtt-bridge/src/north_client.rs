//! 北向 emqx MQTT 客户端
//!
//! 连接 emqx (可配置地址:8883)，用于北向通信
//! 支持 TLS + 双向证书认证

use async_trait::async_trait;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS, Event, Transport, TlsOptions};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::error::MqttBridgeError;
use crate::client::MqttBridge;
use crate::config::NorthMqttConfig;

/// 北向 MQTT 客户端实现
pub struct NorthMqttClient {
    client: AsyncClient,
    eventloop: Arc<Mutex<EventLoop>>,
    connected: Arc<Mutex<bool>>,
}

impl NorthMqttClient {
    /// 创建新的 NorthMqttClient
    pub fn new(config: &NorthMqttConfig) -> Result<Self, MqttBridgeError> {
        let broker_addr = &config.broker_addr;
        let parts: Vec<&str> = broker_addr.split(':').collect();
        let host = parts.first().unwrap_or(&"mqtt.example.com");
        let port: u16 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(8883);

        let mut mqtt_options = MqttOptions::new(
            config.client_id.clone(),
            host,
            port,
        );
        mqtt_options.set_keep_alive(std::time::Duration::from_secs(config.keepalive_secs));

        // 配置 TLS
        let tls_options = TlsOptions::new()
            .ca_file(&config.tls.ca_cert)
            .client_auth(&config.tls.client_cert, &config.tls.client_key);
        mqtt_options.set_transport(Transport::tls(tls_options));

        let (client, eventloop) = AsyncClient::new(mqtt_options, 100);

        Ok(Self {
            client,
            eventloop: Arc::new(Mutex::new(eventloop)),
            connected: Arc::new(Mutex::new(true)),
        })
    }

    /// 处理事件循环，更新连接状态
    pub async fn process_events(&self) -> Result<(), MqttBridgeError> {
        let mut eventloop = self.eventloop.lock().await;
        match eventloop.poll().await {
            Ok(Event::Connected(_)) => {
                *self.connected.lock().unwrap() = true;
                Ok(())
            }
            Ok(Event::Disconnected) => {
                *self.connected.lock().unwrap() = false;
                Err(MqttBridgeError::Disconnected("连接断开".to_string()))
            }
            Ok(_) => Ok(()),
            Err(e) => {
                *self.connected.lock().unwrap() = false;
                Err(MqttBridgeError::ConnectionFailed(e.to_string()))
            }
        }
    }
}

#[async_trait]
impl MqttBridge for NorthMqttClient {
    async fn publish(&self, topic: &str, payload: &[u8], qos: u8) -> Result<(), MqttBridgeError> {
        let qos = match qos {
            0 => QoS::AtMostOnce,
            1 => QoS::AtLeastOnce,
            _ => QoS::ExactlyOnce,
        };
        self.client
            .publish(topic, qos, false, payload)
            .await
            .map_err(|e| MqttBridgeError::PublishFailed(e.to_string()))
    }

    async fn subscribe(&self, topic: &str, qos: u8) -> Result<(), MqttBridgeError> {
        let qos = match qos {
            0 => QoS::AtMostOnce,
            1 => QoS::AtLeastOnce,
            _ => QoS::ExactlyOnce,
        };
        self.client
            .subscribe(topic, qos)
            .await
            .map_err(|e| MqttBridgeError::SubscribeFailed(e.to_string()))
    }

    fn is_connected(&self) -> bool {
        *self.connected.blocking_lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TlsConfig;
    use std::path::PathBuf;

    #[test]
    fn test_reconnect_config_default() {
        let config = crate::config::ReconnectConfig::default();
        assert_eq!(config.initial_interval_secs, 1);
        assert_eq!(config.max_interval_secs, 60);
        assert_eq!(config.backoff_multiplier, 2.0);
    }
}
