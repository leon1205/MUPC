//! 本地 mosquitto MQTT 客户端
//!
//! 连接本地 mosquitto (127.0.0.1:1883)，用于进程间通信

use crate::config::LocalMqttConfig;
use crate::error::MqttBridgeError;
use async_trait::async_trait;
use device_trait::errors::PluginError;
use device_trait::MqttBridge;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

/// 本地 MQTT 客户端实现
pub struct LocalMqttClient {
    client: AsyncClient,
    eventloop: Arc<Mutex<EventLoop>>,
    connected: Arc<AtomicBool>,
    reconnect_config: crate::config::ReconnectConfig,
}

impl LocalMqttClient {
    /// 创建新的 LocalMqttClient
    pub fn new(config: &LocalMqttConfig) -> Result<Self, MqttBridgeError> {
        let broker_addr = &config.broker_addr;
        let parts: Vec<&str> = broker_addr.split(':').collect();
        let host = parts.first().unwrap_or(&"127.0.0.1");
        let port: u16 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1883);

        let mut mqtt_options = MqttOptions::new(config.client_id.clone(), *host, port);
        mqtt_options.set_clean_session(config.clean_session);
        mqtt_options.set_keep_alive(std::time::Duration::from_secs(config.keepalive_secs));

        let (client, eventloop) = AsyncClient::new(mqtt_options, 100);

        Ok(Self {
            client,
            eventloop: Arc::new(Mutex::new(eventloop)),
            // ⚠️ 初值是 **false**：`AsyncClient::new` 只建客户端与事件循环，**尚未**与
            // broker 完成握手 ⇒ 此刻自称"已连接"是假值（2026-09-19 修正；原为 `true`，
            // 与测试 `test_qos_mapping` 的断言「新建客户端不应处于已连接」相悖）。
            // 真值由事件循环在 ConnAck / 断开事件时置位（见 `handle_event` 与 `run`）。
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
                    // 重置退避间隔
                    interval_secs = self.reconnect_config.initial_interval_secs;
                }
                Err(e) => {
                    tracing::warn!("MQTT 连接错误: {}, 等待 {} 秒后重连", e, interval_secs);
                    sleep(Duration::from_secs(interval_secs)).await;

                    // 计算下一次重连间隔（指数退避）
                    let max_interval = self.reconnect_config.max_interval_secs;
                    interval_secs = ((interval_secs as f64)
                        * self.reconnect_config.backoff_multiplier)
                        .min(max_interval as f64) as u64;
                    interval_secs = interval_secs.max(1); // 最小1秒
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
                        _interval_secs = 1; // 重置
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
impl MqttBridge for LocalMqttClient {
    async fn connect(&mut self) -> Result<(), PluginError> {
        // ⚠️ 本方法**不置位** `connected`：真正的握手发生在事件循环处理 `ConnAck` 时
        // （见 `process_events` / `start_event_loop`）。此前这里直接置 true 是**假值** ——
        // broker 不可达时 `is_connected()` 也会报 true，上层据此跳过重连/误报在线
        // （2026-09-20 评审修复）。连接动作由调用方启动事件循环（`run()`）完成，本方法保持
        // 幂等以兼容既有调用序。
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
        "LocalMqttClient"
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
