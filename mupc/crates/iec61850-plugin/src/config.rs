//! IEC 61850 配置定义

use serde::Deserialize;

/// IEC 61850 插件配置
#[derive(Debug, Clone, Deserialize)]
pub struct Iec61850Config {
    pub local_ip: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,
    pub go_id: String,
    pub mms_timeout_ms: u64,
}

impl Default for Iec61850Config {
    fn default() -> Self {
        Self {
            local_ip: "0.0.0.0".to_string(),
            local_port: 102,
            remote_ip: "127.0.0.1".to_string(),
            remote_port: 102,
            go_id: "GOOSE1".to_string(),
            mms_timeout_ms: 5000,
        }
    }
}

/// GOOSE 配置
#[derive(Debug, Clone, Deserialize)]
pub struct GooseConfig {
    pub app_id: u32,
    pub go_id: String,
    pub dat_set: String,
}

impl Default for GooseConfig {
    fn default() -> Self {
        Self {
            app_id: 0,
            go_id: "GOOSE1".to_string(),
            dat_set: "DataSet1".to_string(),
        }
    }
}

/// MMS TLS 配置
#[derive(Debug, Clone, Deserialize)]
pub struct MmsTlsConfig {
    /// 是否启用 TLS
    pub enabled: bool,
    /// CA 证书路径
    pub ca_cert_path: String,
    /// 客户端证书路径
    pub client_cert_path: String,
    /// 客户端私钥路径
    pub client_key_path: String,
    /// 是否验证对端证书
    pub verify_peer: bool,
}

impl Default for MmsTlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ca_cert_path: String::new(),
            client_cert_path: String::new(),
            client_key_path: String::new(),
            verify_peer: true,
        }
    }
}

/// MMS 配置
#[derive(Debug, Clone, Deserialize)]
pub struct MmsConfig {
    pub local_ip: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,
    pub max_connections: u32,
    /// 连接超时（毫秒）
    pub connect_timeout_ms: u64,
    /// 读取超时（毫秒）
    pub read_timeout_ms: u64,
    /// TLS 配置
    pub tls: Option<MmsTlsConfig>,
}

impl Default for MmsConfig {
    fn default() -> Self {
        Self {
            local_ip: "0.0.0.0".to_string(),
            local_port: 102,
            remote_ip: "192.168.1.100".to_string(),
            remote_port: 102,
            max_connections: 10,
            connect_timeout_ms: 5000,
            read_timeout_ms: 3000,
            tls: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Iec61850Config::default();
        assert_eq!(config.local_port, 102);
        assert_eq!(config.remote_port, 102);
        assert_eq!(config.mms_timeout_ms, 5000);
    }

    #[test]
    fn test_goose_config_default() {
        let config = GooseConfig::default();
        assert_eq!(config.go_id, "GOOSE1");
    }
}
