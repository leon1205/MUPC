//! MqttBridge trait 定义

use async_trait::async_trait;
use crate::error::MqttBridgeError;

/// MQTT 网桥 trait
/// 统一抽象 LocalMqttClient 和 NorthMqttClient
#[async_trait]
pub trait MqttBridge: Send + Sync {
    /// 发布消息到指定 Topic
    async fn publish(&self, topic: &str, payload: &[u8], qos: u8) -> Result<(), MqttBridgeError>;

    /// 订阅指定 Topic
    async fn subscribe(&self, topic: &str, qos: u8) -> Result<(), MqttBridgeError>;

    /// 获取连接状态
    fn is_connected(&self) -> bool;
}
