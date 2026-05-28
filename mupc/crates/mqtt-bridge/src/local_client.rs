//! 本地 mosquitto MQTT 客户端
//!
//! 连接本地 mosquitto (127.0.0.1:1883)，用于进程间通信

use async_trait::async_trait;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS, Event};
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::error::MqttBridgeError;
use crate::client::MqttBridge;
use crate::config::LocalMqttConfig;

/// 本地 MQTT 客户端实现
pub struct LocalMqttClient {
    client: AsyncClient,
    eventloop: Arc<Mutex<EventLoop>>,
    connected: Arc<Mutex<bool>>,
}

impl LocalMqttClient {
    /// 创建新的 LocalMqttClient
    pub fn new(config: &LocalMqttConfig) -> Result<Self, MqttBridgeError> {
        let broker_addr = &config.broker_addr;
        let parts: Vec<&str> = broker_addr.split(':').collect();
        let host = parts.first().unwrap_or(&"127.0.0.1");
        let port: u16 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1883);

        let mut mqtt_options = MqttOptions::new(
            config.client_id.clone(),
            host,
            port,
        );
        mqtt_options.set_clean_session(config.clean_session);
        mqtt_options.set_keep_alive(std::time::Duration::from_secs(config.keepalive_secs));

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
impl MqttBridge for LocalMqttClient {
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

    #[test]
    fn test_local_mqtt_config_default() {
        let config = LocalMqttConfig::default();
        assert_eq!(config.broker_addr, "127.0.0.1:1883");
        assert_eq!(config.client_id, "mupc-local");
        assert!(config.clean_session);
        assert_eq!(config.keepalive_secs, 60);
    }

    #[test]
    fn test_local_mqtt_client_creation() {
        let config = LocalMqttConfig::default();
        let client = LocalMqttClient::new(&config);
        assert!(client.is_ok());
    }
}
