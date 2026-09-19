//! 北向 emqx MQTT 客户端
//!
//! 连接 emqx (可配置地址:8883)，用于北向通信
//! 支持 TLS + 双向证书认证

use crate::config::NorthMqttConfig;
use crate::error::MqttBridgeError;
use async_trait::async_trait;
use device_trait::errors::PluginError;
use device_trait::MqttBridge;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS, Transport};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

/// 北向 MQTT 客户端实现
pub struct NorthMqttClient {
    client: AsyncClient,
    eventloop: Arc<Mutex<EventLoop>>,
    connected: Arc<AtomicBool>,
    reconnect_config: crate::config::ReconnectConfig,
}

impl NorthMqttClient {
    /// 创建新的 NorthMqttClient
    pub fn new(config: &NorthMqttConfig) -> Result<Self, MqttBridgeError> {
        let broker_addr = &config.broker_addr;
        let parts: Vec<&str> = broker_addr.split(':').collect();
        let host = parts.first().unwrap_or(&"mqtt.example.com");
        let port: u16 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(8883);

        let mut mqtt_options = MqttOptions::new(config.client_id.clone(), *host, port);
        mqtt_options.set_keep_alive(std::time::Duration::from_secs(config.keepalive_secs));
        // 北向通信需要持久化会话，支持断线重连后恢复
        mqtt_options.set_clean_session(false);

        // 配置 TLS（双向证书认证）
        let ca = std::fs::read(&config.tls.ca_cert)
            .map_err(|e| MqttBridgeError::CertificateError(format!("读取 CA 证书失败: {}", e)))?;
        let client_cert = std::fs::read(&config.tls.client_cert)
            .map_err(|e| MqttBridgeError::CertificateError(format!("读取客户端证书失败: {}", e)))?;
        let client_key = std::fs::read(&config.tls.client_key)
            .map_err(|e| MqttBridgeError::CertificateError(format!("读取客户端密钥失败: {}", e)))?;
        mqtt_options.set_transport(Transport::tls(ca, Some((client_cert, client_key)), None));

        let (client, eventloop) = AsyncClient::new(mqtt_options, 100);

        Ok(Self {
            client,
            eventloop: Arc::new(Mutex::new(eventloop)),
            // 与 `LocalMqttClient` 对齐：**未完成握手前不得自称已连接**（2026-09-20 评审修复）。
            connected: Arc::new(AtomicBool::new(false)),
            reconnect_config: config.reconnect.clone(),
        })
    }

    /// 处理事件循环，更新连接状态
    pub async fn process_events(&self) -> Result<(), MqttBridgeError> {
        let mut eventloop = self.eventloop.lock().await;
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                self.connected.store(true, Ordering::SeqCst);
                Ok(())
            }
            Ok(Event::Incoming(Packet::Disconnect)) => {
                self.connected.store(false, Ordering::SeqCst);
                Err(MqttBridgeError::Disconnected("连接断开".to_string()))
            }
            Ok(_) => Ok(()),
            Err(e) => {
                self.connected.store(false, Ordering::SeqCst);
                Err(MqttBridgeError::ConnectionFailed(e.to_string()))
            }
        }
    }

    /// 运行事件循环，支持自动重连
    /// 按照 reconnect_config 的策略进行指数退避重连
    pub async fn run(&self) -> Result<(), MqttBridgeError> {
        let mut interval_secs = self.reconnect_config.initial_interval_secs;

        loop {
            match self.process_events().await {
                Ok(_) => {
                    interval_secs = self.reconnect_config.initial_interval_secs;
                }
                Err(e) => {
                    tracing::warn!("MQTT 北向连接错误: {}, 等待 {} 秒后重连", e, interval_secs);
                    sleep(Duration::from_secs(interval_secs)).await;

                    let max_interval = self.reconnect_config.max_interval_secs;
                    interval_secs = ((interval_secs as f64)
                        * self.reconnect_config.backoff_multiplier)
                        .min(max_interval as f64) as u64;
                    interval_secs = interval_secs.max(1);
                }
            }
        }
    }

    /// 启动后台任务处理事件（不阻塞）
    pub fn start_event_loop(&self) {
        let eventloop = Arc::clone(&self.eventloop);
        let connected = Arc::clone(&self.connected);

        tokio::spawn(async move {
            let mut _interval_secs = 1u64;

            loop {
                let mut el = eventloop.lock().await;
                match el.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        connected.store(true, Ordering::SeqCst);
                        _interval_secs = 1;
                    }
                    Ok(Event::Incoming(Packet::Disconnect)) => {
                        connected.store(false, Ordering::SeqCst);
                    }
                    Ok(_) => {}
                    Err(_) => {
                        connected.store(false, Ordering::SeqCst);
                    }
                }
            }
        });
    }
}

#[async_trait]
impl MqttBridge for NorthMqttClient {
    async fn connect(&mut self) -> Result<(), PluginError> {
        // 同 `LocalMqttClient::connect`：不置位 `connected`（真值由事件循环在 ConnAck/断开
        // 时维护）。此前直接置 true 会让"broker 不可达"被报成在线（2026-09-20 评审修复）。
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), PluginError> {
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn publish(&self, topic: &str, payload: &[u8], qos: u8) -> Result<(), PluginError> {
        let qos = match qos {
            0 => QoS::AtMostOnce,
            1 => QoS::AtLeastOnce,
            _ => QoS::ExactlyOnce,
        };
        self.client
            .publish(topic, qos, false, payload)
            .await
            .map_err(|e| PluginError::Other(format!("发布失败: {}", e)))
    }

    async fn subscribe(&self, topic: &str, qos: u8) -> Result<(), PluginError> {
        let qos = match qos {
            0 => QoS::AtMostOnce,
            1 => QoS::AtLeastOnce,
            _ => QoS::ExactlyOnce,
        };
        self.client
            .subscribe(topic, qos)
            .await
            .map_err(|e| PluginError::Other(format!("订阅失败: {}", e)))
    }

    /// 是否已与 broker 完成握手。
    ///
    /// ⚠️ 用 `AtomicBool` 而非 `tokio::sync::Mutex<bool>`：后者只能 `blocking_lock()`（在任何
    /// 运行时线程内调用即 panic「Cannot block the current thread from within a runtime」），
    /// 而本方法是**同步 trait 方法**、必须可被任意上下文调用（2026-09-20 评审修复）。
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn name(&self) -> &'static str {
        "NorthMqttClient"
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
