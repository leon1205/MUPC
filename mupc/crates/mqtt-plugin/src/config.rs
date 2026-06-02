//! MQTT 配置定义

use serde::Deserialize;

/// MQTT QoS 级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[derive(Default)]
pub enum MqttQos {
    #[default]
    AtMostOnce = 0,   // QoS 0: 最多一次
    AtLeastOnce = 1,  // QoS 1: 至少一次
    ExactlyOnce = 2,  // QoS 2: 恰好一次
}


impl From<MqttQos> for rumqttc::QoS {
    fn from(qos: MqttQos) -> Self {
        match qos {
            MqttQos::AtMostOnce => rumqttc::QoS::AtMostOnce,
            MqttQos::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
            MqttQos::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
        }
    }
}

/// MQTT 配置
#[derive(Debug, Clone, Deserialize)]
pub struct MqttConfig {
    pub broker_addr: String,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub use_tls: bool,
    pub ca_cert: Option<String>,
    pub client_cert: Option<String>,
    pub client_key: Option<String>,
    pub qos: MqttQos,
    pub keepalive_secs: u16,
    pub clean_session: bool,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            broker_addr: "localhost:1883".to_string(),
            client_id: "mupc_client".to_string(),
            username: None,
            password: None,
            use_tls: false,
            ca_cert: None,
            client_cert: None,
            client_key: None,
            qos: MqttQos::AtMostOnce,
            keepalive_secs: 60,
            clean_session: true,
        }
    }
}

impl MqttConfig {
    /// 检查是否启用 TLS
    pub fn is_tls_enabled(&self) -> bool {
        self.use_tls
    }

    /// 获取 TLS 配置
    pub fn get_tls_config(&self) -> Option<TlsConfig> {
        if self.use_tls {
            Some(TlsConfig {
                ca_cert: self.ca_cert.clone(),
                client_cert: self.client_cert.clone(),
                client_key: self.client_key.clone(),
            })
        } else {
            None
        }
    }
}

/// TLS 配置
#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub ca_cert: Option<String>,
    pub client_cert: Option<String>,
    pub client_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mqtt_qos_default() {
        assert_eq!(MqttQos::default(), MqttQos::AtMostOnce);
    }

    #[test]
    fn test_mqtt_config_default() {
        let config = MqttConfig::default();
        assert_eq!(config.broker_addr, "localhost:1883");
        assert!(!config.use_tls);
        assert_eq!(config.qos, MqttQos::AtMostOnce);
    }

    #[test]
    fn test_mqtt_config_tls() {
        let mut config = MqttConfig::default();
        config.use_tls = true;
        config.ca_cert = Some("ca.pem".to_string());

        assert!(config.is_tls_enabled());
        assert!(config.get_tls_config().is_some());
    }

    #[test]
    fn test_mqtt_qos_conversion() {
        assert_eq!(rumqttc::QoS::AtMostOnce, MqttQos::AtMostOnce.into());
        assert_eq!(rumqttc::QoS::AtLeastOnce, MqttQos::AtLeastOnce.into());
        assert_eq!(rumqttc::QoS::ExactlyOnce, MqttQos::ExactlyOnce.into());
    }
}