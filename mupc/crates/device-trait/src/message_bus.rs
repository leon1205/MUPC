//! 消息总线接口
//!
//! 提供南向通信模块内部的 publish/subscribe 消息机制

use crate::errors::BusError;
use crate::types::{Message, Topic};
use std::sync::Arc;

/// 消息处理器接口
///
/// 处理接收到的消息
pub trait MessageHandler: Send + Sync {
    /// 处理消息
    ///
    /// # Arguments
    /// - `msg`: 接收到的消息
    fn handle(&self, msg: Message);
}

/// 消息总线接口
///
/// 提供发布/订阅模式的消息传递能力
pub trait MessageBus: Send + Sync {
    /// 发布消息
    ///
    /// # Arguments
    /// - `msg`: 消息
    ///
    /// # Returns
    /// - `Ok(())`: 发布成功
    /// - `Err(BusError)`: 发布失败
    fn publish(&self, msg: Message) -> Result<(), BusError>;

    /// 订阅主题
    ///
    /// # Arguments
    /// - `topic`: 主题
    /// - `handler`: 消息处理器
    ///
    /// # Returns
    /// - `Ok(())`: 订阅成功
    /// - `Err(BusError)`: 订阅失败
    fn subscribe(&self, topic: Topic, handler: Arc<dyn MessageHandler>) -> Result<(), BusError>;

    /// 取消订阅
    ///
    /// # Arguments
    /// - `topic`: 主题
    ///
    /// # Returns
    /// - `Ok(())`: 取消订阅成功
    /// - `Err(BusError)`: 取消订阅失败
    fn unsubscribe(&self, topic: &Topic) -> Result<(), BusError>;

    /// 获取订阅者数量
    fn subscriber_count(&self, topic: &Topic) -> usize;

    /// 获取已订阅主题列表
    fn subscribed_topics(&self) -> Vec<Topic>;
}

/// 空消息处理器
///
/// 用于测试或默认实现
pub struct NoOpMessageHandler;

impl MessageHandler for NoOpMessageHandler {
    fn handle(&self, _msg: Message) {
        // 空操作
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_handler() {
        let handler = NoOpMessageHandler;
        let msg = Message::new(Topic::new("test"), vec![1, 2, 3]);
        handler.handle(msg);
    }
}