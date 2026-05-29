//! 数据上报模块
//!
//! 通过消息总线将处理后的遥测数据分发给下游消费者
//!（gateway 北向上送、strategy-engine 策略决策等）。
//!
//! # 消息主题
//!
//! | 主题 | 说明 |
//! |------|------|
//! | `telemetry.high_freq` | 高频遥测数据（>=1Hz） |
//! | `strategy.decision` | 策略决策结果 |

use async_trait::async_trait;
use mupc_common::MupcError;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;

use crate::telemetry::DataPackage;

/// 数据上报接口
///
/// 与设计文档 `DataReporter` trait 对齐：
/// - `report()` — 发布数据到消息总线
/// - `subscribe()` — 订阅指定主题
#[async_trait]
pub trait DataReporter: Send + Sync {
    /// 上报遥测数据到消息总线
    ///
    /// # 参数
    ///
    /// * `data` - 遥测数据包
    ///
    /// # 错误
    ///
    /// 消息总线不可用时返回错误
    async fn report(&self, data: &DataPackage) -> Result<(), MupcError>;

    /// 订阅指定主题
    ///
    /// # 参数
    ///
    /// * `topic` - 消息主题名称
    ///
    /// # 错误
    ///
    /// 主题不存在或不支持时返回错误
    fn subscribe(&mut self, topic: &str) -> Result<(), MupcError>;
}

/// 消息回调函数类型
pub type MessageCallback = Arc<dyn Fn(&DataPackage) + Send + Sync>;

/// 消息总线实现
///
/// 维护主题-订阅者的映射关系，支持发布/订阅模式。
pub struct MessageBus {
    /// 主题 → 订阅者列表
    subscribers: Arc<Mutex<HashMap<String, Vec<MessageCallback>>>>,
}

impl MessageBus {
    /// 创建新的消息总线
    pub fn new() -> Self {
        Self {
            subscribers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 发布消息到指定主题
    ///
    /// 所有订阅该主题的回调函数都将被调用。
    ///
    /// # 参数
    ///
    /// * `topic` - 主题名称
    /// * `data` - 数据包
    pub fn publish(&self, topic: &str, data: &DataPackage) {
        let subs = self.subscribers.lock();
        if let Some(callbacks) = subs.get(topic) {
            for cb in callbacks {
                cb(data);
            }
        }
    }

    /// 添加订阅
    ///
    /// # 参数
    ///
    /// * `topic` - 主题名称
    /// * `callback` - 收到消息时的回调函数
    pub fn add_subscriber<F>(&self, topic: &str, callback: F)
    where
        F: Fn(&DataPackage) + Send + Sync + 'static,
    {
        let mut subs = self.subscribers.lock();
        subs.entry(topic.to_string())
            .or_default()
            .push(Arc::new(callback));
    }

    /// 获取订阅者数量
    pub fn subscriber_count(&self, topic: &str) -> usize {
        let subs = self.subscribers.lock();
        subs.get(topic).map(|v| v.len()).unwrap_or(0)
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

/// DataReporter 实现
///
/// 基于 `MessageBus` 的消息发布/订阅模式实现遥测数据上报。
pub struct DataReporterImpl {
    /// 消息总线
    bus: Arc<MessageBus>,
    /// 默认发布主题
    default_topic: String,
}

impl DataReporterImpl {
    /// 创建新的 DataReporter 实现
    ///
    /// # 参数
    ///
    /// * `bus` - 共享的消息总线实例
    pub fn new(bus: Arc<MessageBus>) -> Self {
        Self {
            bus,
            default_topic: "telemetry.high_freq".to_string(),
        }
    }

    /// 设置默认发布主题
    pub fn with_default_topic(mut self, topic: &str) -> Self {
        self.default_topic = topic.to_string();
        self
    }

    /// 获取消息总线引用（用于外部单元测试验证）
    pub fn message_bus(&self) -> &Arc<MessageBus> {
        &self.bus
    }
}

#[async_trait]
impl DataReporter for DataReporterImpl {
    async fn report(&self, data: &DataPackage) -> Result<(), MupcError> {
        self.bus.publish(&self.default_topic, data);

        // 同时发布到遥测总主题
        self.bus.publish("telemetry", data);

        Ok(())
    }

    fn subscribe(&mut self, topic: &str) -> Result<(), MupcError> {
        // 订阅操作的 stub —— 在 DataReporter 的实现中，subscribe 用于注册消费端
        // 实际消费端的订阅通过 MessageBus::add_subscriber 完成
        let _ = topic;
        Ok(())
    }
}

// 兼容 telemetry.rs 中的旧接口
#[async_trait]
impl crate::telemetry::DataReporter for DataReporterImpl {
    async fn report(&self, data: &DataPackage) -> Result<(), MupcError> {
        // 调用 reporter 模块中的 DataReporter trait 实现
        <DataReporterImpl as DataReporter>::report(self, data).await
    }

    fn protocol(&self) -> &str {
        "message_bus"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{
        BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    fn make_test_package() -> DataPackage {
        DataPackage {
            electrical: ElectricalData {
                voltage: Some(220.0),
                current: Some(10.0),
                active_power: Some(2200.0),
                reactive_power: Some(0.0),
                cos_phi: Some(1.0),
                frequency: Some(50.0),
            },
            battery: BatteryData {
                soc: Some(80.0),
                soh: Some(95.0),
                temperature: Some(25.0),
            },
            device_status: DeviceStatus {
                inverter_status: InverterStatus::Running,
                pv_power: Some(5000.0),
                load_power: Some(3000.0),
                ev_charger_power: Some(0.0),
            },
            timestamp: 1700000000,
        }
    }

    #[tokio::test]
    async fn test_report_publishes_to_bus() {
        let bus = Arc::new(MessageBus::new());
        let reporter = DataReporterImpl::new(bus.clone());
        let package = make_test_package();

        let received = Arc::new(AtomicBool::new(false));
        let received_clone = received.clone();
        bus.add_subscriber("telemetry.high_freq", move |_data: &DataPackage| {
            received_clone.store(true, Ordering::SeqCst);
        });

        reporter.report(&package).await.unwrap();
        assert!(received.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_subscribe_returns_ok() {
        let bus = Arc::new(MessageBus::new());
        let mut reporter = DataReporterImpl::new(bus);
        let result = reporter.subscribe("test.topic");
        assert!(result.is_ok());
    }

    #[test]
    fn test_message_bus_subscriber_count() {
        let bus = MessageBus::new();
        assert_eq!(bus.subscriber_count("telemetry.high_freq"), 0);

        bus.add_subscriber("telemetry.high_freq", |_| {});
        assert_eq!(bus.subscriber_count("telemetry.high_freq"), 1);

        bus.add_subscriber("telemetry.high_freq", |_| {});
        assert_eq!(bus.subscriber_count("telemetry.high_freq"), 2);
    }
}
