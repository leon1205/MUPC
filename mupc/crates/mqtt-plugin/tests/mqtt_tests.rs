//! MQTT 插件单元测试

use mupc_mqtt_plugin::{MqttConfig, MqttQos, MqttClient};

#[test]
fn test_mqtt_config_default() {
    let config = MqttConfig::default();
    assert_eq!(config.broker_addr, "localhost:1883");
    assert!(!config.use_tls);
    assert_eq!(config.qos, MqttQos::AtMostOnce);
}

#[test]
fn test_mqtt_qos() {
    assert_eq!(MqttQos::AtMostOnce as u8, 0);
    assert_eq!(MqttQos::AtLeastOnce as u8, 1);
    assert_eq!(MqttQos::ExactlyOnce as u8, 2);
}

#[test]
fn test_mqtt_client_creation() {
    let config = MqttConfig::default();
    let client = MqttClient::new(config);
    assert_eq!(client.device_id(), "mupc_client");
    assert_eq!(client.device_type(), "MQTT");
}

#[test]
fn test_mqtt_config_tls() {
    let mut config = MqttConfig::default();
    config.use_tls = true;
    config.ca_cert = Some("ca.pem".to_string());
    assert!(config.is_tls_enabled());
}

#[test]
fn test_mqtt_config_with_auth() {
    let mut config = MqttConfig::default();
    config.username = Some("user".to_string());
    config.password = Some("pass".to_string());
    let opts = config;
    assert!(opts.username.is_some());
}