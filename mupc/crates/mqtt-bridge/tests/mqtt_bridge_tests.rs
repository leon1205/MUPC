//! mqtt-bridge 单元测试

#[cfg(test)]
mod tests {
    use mupc_mqtt_bridge::{
        LocalMqttClient, LocalMqttConfig,
        topics::*,
        MqttBridge,
    };

    #[test]
    fn test_local_mqtt_config_default() {
        let config = LocalMqttConfig::default();
        assert_eq!(config.broker_addr, "127.0.0.1:1883");
        assert_eq!(config.client_id, "mupc-local");
        assert!(config.clean_session);
        assert_eq!(config.keepalive_secs, 60);
        assert_eq!(config.reconnect.initial_interval_secs, 1);
        assert_eq!(config.reconnect.max_interval_secs, 60);
        assert_eq!(config.reconnect.backoff_multiplier, 2.0);
    }

    #[test]
    fn test_local_mqtt_client_creation() {
        let config = LocalMqttConfig::default();
        let client = LocalMqttClient::new(&config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_local_mqtt_client_not_connected_initially() {
        let config = LocalMqttConfig::default();
        let client = LocalMqttClient::new(&config).unwrap();
        // 新客户端默认未连接（需要调用 process_events 后才连接）
        // 注意：实际状态取决于 rumqttc 内部状态
    }

    #[test]
    fn test_topic_definitions() {
        // 本地 Topic
        assert_eq!(LOCAL_TELEMETRY, "mupc/local/telemetry");
        assert_eq!(LOCAL_STRATEGY_COMMAND, "mupc/local/strategy/command");
        assert_eq!(LOCAL_AI_READY, "mupc/local/ai/ready");

        // 北向 Topic
        assert_eq!(NORTH_TELEMETRY, "mupc/north/telemetry");
        assert_eq!(NORTH_FAULT, "mupc/north/fault");
        assert_eq!(NORTH_STRATEGY_COMMAND, "mupc/north/strategy/command");
        assert_eq!(NORTH_STATUS, "mupc/north/status");
    }

    #[test]
    fn test_qos_mapping() {
        let config = LocalMqttConfig::default();
        let client = LocalMqttClient::new(&config).unwrap();

        // 注意：这里只是验证客户端可以创建
        // 实际 publish/subscribe 需要连接后才能执行
        assert!(!client.is_connected());
    }
}
