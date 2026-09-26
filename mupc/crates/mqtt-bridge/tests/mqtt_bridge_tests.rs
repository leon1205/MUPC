//! mqtt-bridge 单元测试（§9.3.4 主题函数化后的形态）

#[cfg(test)]
mod tests {
    // `NORTH_TELEMETRY` 已废弃（§9.3.4）——本文件仍断言它的字面值，作为"旧常量未被删名"
    // 的兼容性锚点 ⇒ 就地放行废弃告警（**不是**在推广使用）。
    #![allow(deprecated)]
    use mupc_mqtt_bridge::{topics::*, LocalMqttClient, LocalMqttConfig, MqttBridge};

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
        // 新客户端默认未连接（握手发生在事件循环的 poll 中）——2026-09-20 评审修复的语义锚点
        assert!(!client.is_connected());
    }

    #[test]
    fn test_topic_definitions() {
        // 本地 Topic
        assert_eq!(LOCAL_TELEMETRY, "mupc/local/telemetry");
        assert_eq!(LOCAL_STRATEGY_COMMAND, "mupc/local/strategy/command");
        assert_eq!(LOCAL_AI_READY, "mupc/local/ai/ready");

        // 北向 Topic（§9.3.4：遥测/事件**函数化**；状态/故障/策略常量不变）
        assert_eq!(
            north_telemetry("grid_meter"),
            "mupc/north/telemetry/grid_meter"
        );
        assert_eq!(north_event("fire"), "mupc/north/event/fire");
        assert_eq!(NORTH_FAULT, "mupc/north/fault");
        assert_eq!(NORTH_STRATEGY_COMMAND, "mupc/north/strategy/command");
        assert_eq!(NORTH_STATUS, "mupc/north/status");
        // 旧常量保留（兼容性锚点）：值不变、不再用于发布
        assert_eq!(NORTH_TELEMETRY, "mupc/north/telemetry");
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
