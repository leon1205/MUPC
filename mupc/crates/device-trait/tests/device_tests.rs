//! device-trait 单元测试

use device_trait::{
    DataFrame, DataQuality, DeviceStatus, DeviceType, Message, Parity, PluginMeta, Rs485Config,
    Topic,
};
use std::sync::Arc;

#[test]
fn test_device_status_is_online() {
    assert!(DeviceStatus::Online.is_online());
    assert!(!DeviceStatus::Offline.is_online());
    assert!(!DeviceStatus::Error("test".to_string()).is_online());
}

#[test]
fn test_data_quality_variants() {
    assert_eq!(DataQuality::Good, DataQuality::Good);
    assert_eq!(DataQuality::Invalid, DataQuality::Invalid);
    assert_ne!(DataQuality::Good, DataQuality::Invalid);
}

#[test]
fn test_data_frame_creation() {
    let frame = DataFrame::new("device_001".to_string(), vec![1, 2, 3]);
    assert_eq!(frame.device_id, "device_001");
    assert_eq!(frame.data, vec![1, 2, 3]);
    assert_eq!(frame.quality, DataQuality::Good);
}

#[test]
fn test_data_frame_with_timestamp() {
    let frame = DataFrame::with_timestamp("device_001".to_string(), 1234567890, vec![1, 2, 3]);
    assert_eq!(frame.timestamp, 1234567890);
}

#[test]
fn test_data_frame_with_quality() {
    let frame = DataFrame::new("device_001".to_string(), vec![1, 2, 3])
        .with_quality(DataQuality::Invalid);
    assert_eq!(frame.quality, DataQuality::Invalid);
}

#[test]
fn test_device_type_from_str() {
    assert_eq!(DeviceType::from_str("ttu"), DeviceType::Ttu);
    assert_eq!(DeviceType::from_str("inverter"), DeviceType::Inverter);
    assert_eq!(DeviceType::from_str("charger"), DeviceType::Charger);
    assert_eq!(DeviceType::from_str("unknown"), DeviceType::Unknown);
}

#[test]
fn test_device_type_as_str() {
    assert_eq!(DeviceType::Ttu.as_str(), "ttu");
    assert_eq!(DeviceType::Inverter.as_str(), "inverter");
    assert_eq!(DeviceType::Unknown.as_str(), "unknown");
}

#[test]
fn test_topic_creation() {
    let topic = Topic::new("test/topic");
    assert_eq!(topic.as_str(), "test/topic");
}

#[test]
fn test_topic_from_string() {
    let topic: Topic = String::from("test/topic").into();
    assert_eq!(topic.as_str(), "test/topic");
}

#[test]
fn test_topic_display() {
    let topic = Topic::new("test/topic");
    assert_eq!(format!("{}", topic), "test/topic");
}

#[test]
fn test_message_creation() {
    let msg = Message::new(Topic::new("test"), vec![1, 2, 3]);
    assert_eq!(msg.topic.as_str(), "test");
    assert_eq!(msg.payload, vec![1, 2, 3]);
}

#[test]
fn test_message_with_topic() {
    let msg = Message::with_topic("test/topic", vec![1, 2, 3]);
    assert_eq!(msg.topic.as_str(), "test/topic");
}

#[test]
fn test_plugin_meta_creation() {
    let meta = PluginMeta::new("rs485-plugin", "1.0.0", "MUPC Team", "RS485 driver");
    assert_eq!(meta.name, "rs485-plugin");
    assert_eq!(meta.version, "1.0.0");
    assert_eq!(meta.author, "MUPC Team");
    assert_eq!(meta.description, "RS485 driver");
}

#[test]
fn test_rs485_config_default() {
    let config = Rs485Config::default();
    assert_eq!(config.port, "/dev/ttyUSB0");
    assert_eq!(config.baud_rate, 9600);
    assert_eq!(config.data_bits, 8);
    assert_eq!(config.stop_bits, 1);
    assert_eq!(config.parity, Parity::None);
    assert_eq!(config.timeout_ms, 1000);
}

#[test]
fn test_parity_as_str() {
    assert_eq!(Parity::None.as_str(), "none");
    assert_eq!(Parity::Even.as_str(), "even");
    assert_eq!(Parity::Odd.as_str(), "odd");
}

#[test]
fn test_topic_into_string() {
    let topic = Topic::new("test");
    let s = topic.into_string();
    assert_eq!(s, "test");
}