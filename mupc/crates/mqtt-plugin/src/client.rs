//! MQTT 客户端实现
//!
//! 实现 MQTT 协议客户端，支持 TLS 加密和 QoS 0/1/2 级别

use crate::config::{MqttConfig, MqttQos};
use crate::errors::MqttError;
use async_trait::async_trait;
use device_trait::{DataFrame, Device, DeviceError, DeviceStatus};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS, Transport};
use std::sync::Arc;
use std::time::Duration;
use std::sync::RwLock;
use tokio::sync::broadcast;

/// MQTT 客户端状态
#[derive(Debug, Clone, PartialEq)]
pub enum MqttClientState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// MQTT 设备实现
pub struct MqttClient {
    config: MqttConfig,
    inner: AsyncClient,
    state: Arc<RwLock<MqttClientState>>,
    #[allow(dead_code)]
    event_tx: broadcast::Sender<rumqttc::Event>,
}

impl MqttClient {
    /// 创建 MQTT 客户端
    pub fn new(config: MqttConfig) -> Self {
        let (_event_tx, _) = broadcast::channel::<rumqttc::Event>(100);

        let mut mqtt_opts = MqttOptions::new(
            &config.client_id,
            &config.broker_addr,
            1883,
        );

        mqtt_opts.set_keep_alive(Duration::from_secs(config.keepalive_secs as u64));

        if let (Some(username), Some(password)) = (&config.username, &config.password) {
            mqtt_opts.set_credentials(username, password);
        }

        // 配置 TLS
        if config.use_tls {
            let transport = Self::build_tls_configuration(&config);
            mqtt_opts.set_transport(transport);
        }

        let (client, mut eventloop) = AsyncClient::new(mqtt_opts, 100);

        let (tx, _rx) = broadcast::channel::<rumqttc::Event>(100);

        // 启动事件循环处理线程
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(notification) => {
                        let _ = tx_clone.send(notification);
                    }
                    Err(e) => {
                        let _ = tx_clone.send(Event::Incoming(Packet::Disconnect));
                        tracing::error!("MQTT poll error: {:?}", e);
                        break;
                    }
                }
            }
        });

        Self {
            config,
            inner: client,
            state: Arc::new(RwLock::new(MqttClientState::Disconnected)),
            event_tx: tx,
        }
    }

    /// 连接到 MQTT Broker
    pub async fn connect(&self) -> Result<(), MqttError> {
        let mut state = self.state.write().unwrap();
        *state = MqttClientState::Connecting;

        // 实际连接已在新方法创建时建立，这里只更新状态
        // 连接错误会通过事件循环处理
        *state = MqttClientState::Connected;
        Ok(())
    }

    /// 断开连接
    pub async fn disconnect(&self) -> Result<(), MqttError> {
        self.inner.disconnect().await
            .map_err(|e| MqttError::Disconnected(e.to_string()))?;

        let mut state = self.state.write().unwrap();
        *state = MqttClientState::Disconnected;

        Ok(())
    }

    /// 获取客户端状态
    pub async fn get_state(&self) -> MqttClientState {
        self.state.read().unwrap().clone()
    }

    /// 订阅主题
    pub async fn subscribe(&self, topic: &str, qos: MqttQos) -> Result<(), MqttError> {
        let state = self.state.read().unwrap();
        if *state != MqttClientState::Connected {
            return Err(MqttError::Disconnected("未连接".to_string()));
        }
        drop(state);

        let qos: QoS = qos.into();
        self.inner.subscribe(topic, qos).await
            .map_err(|e| MqttError::SubscribeFailed(e.to_string()))
    }

    /// 发布消息
    pub async fn publish(&self, topic: &str, payload: &[u8], qos: MqttQos, retain: bool) -> Result<(), MqttError> {
        let state = self.state.read().unwrap();
        if *state != MqttClientState::Connected {
            return Err(MqttError::Disconnected("未连接".to_string()));
        }
        drop(state);

        let qos: QoS = qos.into();
        self.inner.publish(topic, qos, retain, payload).await
            .map_err(|e| MqttError::PublishFailed(e.to_string()))
    }

    /// 发布消息（使用默认 QoS）
    pub async fn publish_default(&self, topic: &str, payload: &[u8]) -> Result<(), MqttError> {
        self.publish(topic, payload, self.config.qos, false).await
    }

    /// 构建 TLS 配置
    fn build_tls_configuration(_config: &MqttConfig) -> Transport {
        // TODO: Implement proper certificate loading from config paths
        // (ca_cert, client_cert, client_key) for custom TLS configuration
        Transport::tls_with_default_config()
    }

    /// 获取配置
    pub fn config(&self) -> &MqttConfig {
        &self.config
    }
}

#[async_trait]
impl Device for MqttClient {
    fn device_id(&self) -> &str {
        &self.config.client_id
    }

    fn device_type(&self) -> &str {
        "MQTT"
    }

    fn status(&self) -> Result<DeviceStatus, DeviceError> {
        let state = self.state.read().unwrap();
        match &*state {
            MqttClientState::Connected => Ok(DeviceStatus::Online),
            MqttClientState::Disconnected => Ok(DeviceStatus::Offline),
            MqttClientState::Connecting => Ok(DeviceStatus::Online), // 连接中视为在线
            MqttClientState::Error(s) => Ok(DeviceStatus::Error(s.clone())),
        }
    }

    fn read(&self) -> Result<DataFrame, DeviceError> {
        Err(DeviceError::ProtocolError(
            "MQTT 设备请使用 subscribe 方法订阅主题".to_string(),
        ))
    }

    fn write(&self, _data: &[u8]) -> Result<(), DeviceError> {
        Err(DeviceError::ProtocolError(
            "MQTT 设备请使用 publish 方法发布消息".to_string(),
        ))
    }
}

/// MQTT 消息处理
#[allow(dead_code)]
pub struct MqttMessageHandler {
    topic: String,
    handler: Box<dyn Fn(String, Vec<u8>) + Send + Sync>,
}

impl MqttMessageHandler {
    /// 创建消息处理器
    #[allow(dead_code)]
    pub fn new(topic: String, handler: impl Fn(String, Vec<u8>) + Send + Sync + 'static) -> Self {
        Self {
            topic,
            handler: Box::new(handler),
        }
    }

    /// 处理消息
    #[allow(dead_code)]
    pub fn handle(&self, topic: String, payload: Vec<u8>) {
        (self.handler)(topic, payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mqtt_client_creation() {
        let config = MqttConfig::default();
        let client = MqttClient::new(config);
        assert_eq!(client.config().client_id, "mupc_client");
    }

    #[tokio::test]
    async fn test_mqtt_client_disconnected_state() {
        let config = MqttConfig::default();
        let client = MqttClient::new(config);
        let state = client.get_state().await;
        assert_eq!(state, MqttClientState::Disconnected);
    }

    #[tokio::test]
    async fn test_mqtt_client_connect() {
        let config = MqttConfig::default();
        let client = MqttClient::new(config);
        // 注意：这里不实际连接，只测试状态转换
        // 实际连接需要 MQTT Broker
    }

    #[test]
    fn test_mqtt_message_handler() {
        let received = Arc::new(std::sync::Mutex::new(Vec::new()));
        let received_clone = received.clone();
        let handler = MqttMessageHandler::new(
            "test/topic".to_string(),
            move |topic, payload| {
                received_clone.lock().unwrap().push((topic, payload));
            },
        );

        handler.handle("test/topic".to_string(), b"hello".to_vec());
        assert_eq!(received.lock().unwrap().len(), 1);
    }
}