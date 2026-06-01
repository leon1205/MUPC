//! TLS 连接器模块
//!
//! 提供基于 rustls 的 TLS 1.2+ 连接支持

use crate::cert::CertStore;
use crate::errors::Result;

/// TLS 客户端配置
#[derive(Debug, Clone)]
pub struct TlsClientConfig {
    pub ca_cert_path: String,
    pub client_cert_path: String,
    pub client_key_path: String,
    pub verify_server: bool,
}

impl TlsClientConfig {
    pub fn new(ca_cert_path: String, client_cert_path: String, client_key_path: String) -> Self {
        Self {
            ca_cert_path,
            client_cert_path,
            client_key_path,
            verify_server: true,
        }
    }

    pub fn set_verify_server(mut self, verify: bool) -> Self {
        self.verify_server = verify;
        self
    }
}

/// TLS 连接器
pub struct TlsConnector {
    config: TlsClientConfig,
    cert_store: CertStore,
}

impl TlsConnector {
    pub fn new(config: TlsClientConfig) -> Self {
        Self {
            config,
            cert_store: CertStore::new(),
        }
    }

    pub fn init_certs(&mut self) -> Result<()> {
        self.cert_store.load_ca_cert(&self.config.ca_cert_path)?;
        if !self.config.client_cert_path.is_empty() {
            self.cert_store
                .load_client_cert(&self.config.client_cert_path)?;
        }
        Ok(())
    }

    pub fn cert_store(&self) -> &CertStore {
        &self.cert_store
    }

    pub fn should_verify_server(&self) -> bool {
        self.config.verify_server
    }
}

/// 构建 rustls ClientConfig
///
/// Phase 2+: 集成 SM2/SM4 加密套件后替换默认 provider
#[allow(dead_code)]
pub fn build_rustls_config(_connector: &TlsConnector) -> Result<rustls::ClientConfig> {
    let root_store = rustls::RootCertStore::empty();
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_client_config() {
        let config = TlsClientConfig::new(
            "ca.pem".to_string(),
            "client.pem".to_string(),
            "client_key.pem".to_string(),
        );
        assert!(config.verify_server);
    }

    #[test]
    fn test_tls_connector_verify() {
        let config = TlsClientConfig::new(
            "ca.pem".to_string(),
            "client.pem".to_string(),
            "client_key.pem".to_string(),
        );
        let connector = TlsConnector::new(config);
        assert!(connector.should_verify_server());
    }
}
