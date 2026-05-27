//! 消息总线
//!
//! Phase 1 使用 tokio::sync::mpsc 实现

use mupc_common::MupcError;
use std::any::Any;
use tokio::sync::mpsc;
use std::sync::Arc;

/// 消息主题
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct Topic(pub String);

impl Topic {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

impl AsRef<str> for Topic {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// 消息
#[derive(Debug, Clone)]
pub struct Message {
    pub topic: Topic,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

impl Message {
    pub fn new(topic: Topic, payload: Vec<u8>) -> Self {
        Self {
            topic,
            payload,
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }
}

/// 消息处理器
pub trait MessageHandler: Send + Sync {
    /// 处理消息
    fn handle(&self, msg: &Message);

    /// 获取处理器名称
    fn name(&self) -> &str;
}

/// 消息总线 trait
pub trait MessageBus: Send + Sync {
    /// 发布消息
    fn publish(&self, topic: &Topic, msg: &Message) -> Result<(), MupcError>;

    /// 订阅主题
    fn subscribe(&self, topic: &Topic, handler: Arc<dyn MessageHandler>) -> Result<(), MupcError>;

    /// 取消订阅
    fn unsubscribe(&self, topic: &Topic, name: &str) -> Result<(), MupcError>;
}