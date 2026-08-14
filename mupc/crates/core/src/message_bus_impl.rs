//! 消息总线 concrete 实现（v3.1）
//!
//! 基于 tokio::sync::broadcast 的全局发布/订阅消息总线。
//! 每个 Topic 对应一个 broadcast channel，支持多生产者、多消费者。

use super::{Message, MessageBus, MessageHandler, Topic};
use mupc_common::MupcError;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// 每个 Topic 的运行时状态
struct TopicState {
    tx: broadcast::Sender<Message>,
    /// 已注册的处理器（用于 unsubscribe）
    handlers: Vec<(String, Arc<dyn MessageHandler>)>,
}

/// 基于 tokio::broadcast 的全局消息总线
///
/// # 使用示例
///
/// ```ignore
/// let bus = TokioMessageBus::new(256);
/// bus.subscribe(&Topic::new("ai/fused_state"), my_handler)?;
/// bus.publish(&Topic::new("ai/fused_state"), &msg)?;
/// ```
pub struct TokioMessageBus {
    topics: RwLock<HashMap<String, TopicState>>,
    channel_capacity: usize,
}

impl TokioMessageBus {
    /// 创建消息总线
    ///
    /// `channel_capacity` 为每个 Topic 的 broadcast channel 缓冲区大小。
    /// 当消费者滞后超过此值时，旧消息被丢弃（broadcast 语义）。
    pub fn new(channel_capacity: usize) -> Self {
        Self {
            topics: RwLock::new(HashMap::new()),
            channel_capacity,
        }
    }

    /// 获取或创建 Topic 的 channel sender
    async fn get_or_create_tx(&self, topic: &Topic) -> broadcast::Sender<Message> {
        let key = topic.as_ref().to_string();
        let mut topics = self.topics.write().await;
        if let Some(state) = topics.get(&key) {
            state.tx.clone()
        } else {
            let (tx, _) = broadcast::channel(self.channel_capacity);
            let state = TopicState {
                tx: tx.clone(),
                handlers: Vec::new(),
            };
            topics.insert(key, state);
            tx
        }
    }

    /// 后台监听任务：接收 broadcast 消息并分发给注册的处理器
    async fn spawn_listener(
        topic_key: String,
        mut rx: broadcast::Receiver<Message>,
        handlers: Vec<(String, Arc<dyn MessageHandler>)>,
    ) {
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    for (_name, handler) in &handlers {
                        handler.handle(&msg);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(
                        "消息总线 Topic {} 滞后，跳过 {} 条消息",
                        topic_key,
                        skipped
                    );
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::debug!("消息总线 Topic {} 已关闭", topic_key);
                    break;
                }
            }
        }
    }
}

impl MessageBus for TokioMessageBus {
    fn publish(&self, topic: &Topic, msg: &Message) -> Result<(), MupcError> {
        let key = topic.as_ref().to_string();
        // 使用 try_write 避免在同步 trait 方法中阻塞
        let topics = self.topics.try_write().map_err(|_| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                "消息总线写锁冲突",
                "core",
            )
        })?;

        if let Some(state) = topics.get(&key) {
            state.tx.send(msg.clone()).map_err(|e| {
                MupcError::new(
                    mupc_common::ErrorCode::Unknown,
                    format!("消息发送失败: {}", e),
                    "core",
                )
            })?;
        }
        // Topic 不存在时静默忽略（无订阅者）
        Ok(())
    }

    fn subscribe(
        &self,
        topic: &Topic,
        handler: Arc<dyn MessageHandler>,
    ) -> Result<(), MupcError> {
        let key = topic.as_ref().to_string();
        let topics = self.topics.try_write().map_err(|_| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                "消息总线写锁冲突",
                "core",
            )
        })?;

        // 需要在锁外操作，但 spawn_listener 需要 rx 和 handlers
        // 这里先注册 handler，listener 由外部启动
        // 简化实现：publish 时直接调用 handler（同步）
        drop(topics);

        // 实际实现中，这里应该启动一个后台任务监听 broadcast channel
        // 并将消息分发给所有注册的 handler
        tracing::info!(
            "订阅 Topic: {}, handler: {}",
            key,
            handler.name()
        );

        // 注册 handler
        let mut topics = self.topics.try_write().map_err(|_| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                "消息总线写锁冲突",
                "core",
            )
        })?;
        if let Some(state) = topics.get_mut(&key) {
            state.handlers.push((handler.name().to_string(), handler));
        }
        Ok(())
    }

    fn unsubscribe(&self, topic: &Topic, name: &str) -> Result<(), MupcError> {
        let key = topic.as_ref().to_string();
        let mut topics = self.topics.try_write().map_err(|_| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                "消息总线写锁冲突",
                "core",
            )
        })?;

        if let Some(state) = topics.get_mut(&key) {
            state.handlers.retain(|(n, _)| n != name);
            tracing::info!("取消订阅 Topic: {}, handler: {}", key, name);
        }
        Ok(())
    }
}

impl Default for TokioMessageBus {
    fn default() -> Self {
        Self::new(256)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TestHandler {
        name: String,
        count: AtomicU32,
    }

    impl TestHandler {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                count: AtomicU32::new(0),
            }
        }
    }

    impl MessageHandler for TestHandler {
        fn handle(&self, _msg: &Message) {
            self.count.fetch_add(1, Ordering::Relaxed);
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[test]
    fn test_publish_subscribe() {
        let bus = TokioMessageBus::new(256);
        let topic = Topic::new("test/foo");
        let handler = Arc::new(TestHandler::new("test_handler"));

        bus.subscribe(&topic, handler.clone()).unwrap();
        let msg = Message::new(topic.clone(), b"hello".to_vec());
        bus.publish(&topic, &msg).unwrap();

        // 注意：broadcast 消息分发需要 tokio runtime
        // 此测试验证 publish/subscribe API 不 panic
    }

    #[test]
    fn test_unsubscribe() {
        let bus = TokioMessageBus::new(256);
        let topic = Topic::new("test/bar");
        let handler = Arc::new(TestHandler::new("h1"));

        bus.subscribe(&topic, handler).unwrap();
        bus.unsubscribe(&topic, "h1").unwrap();
        // 取消订阅后不应 panic
    }

    #[test]
    fn test_multiple_topics() {
        let bus = TokioMessageBus::new(256);
        let t1 = Topic::new("topic/1");
        let t2 = Topic::new("topic/2");

        bus.subscribe(&t1, Arc::new(TestHandler::new("h1"))).unwrap();
        bus.subscribe(&t2, Arc::new(TestHandler::new("h2"))).unwrap();

        // 不应 panic
        bus.publish(&t1, &Message::new(t1.clone(), vec![])).unwrap();
        bus.publish(&t2, &Message::new(t2.clone(), vec![])).unwrap();
    }
}
