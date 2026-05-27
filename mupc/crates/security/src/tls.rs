//! TLS 连接器模块
//!
//! 提供基于 rustls 的 TLS 1.2+ 连接支持

use crate::cert::CertStore;
use crate::errors::{GmError, Result};
use std::sync::Arc;
use std::time::SystemTime;

/// TLS 客户端配置
#[derive(Debug, Clone)]
pub struct TlsClientConfig {
    pub ca_cert_path: String,
    pub client_cert_path: String,
    pub client_key_path: String,
    pub verify_server: bool,
}

impl TlsClientConfig {
    /// 创建 TLS 客户端配置
    pub fn new(
        ca_cert_path: String,
        client_cert_path: String,
        client_key_path: String,
    ) -> Self {
        Self {
            ca_cert_path,
            client_cert_path,
            client_key_path,
            verify_server: true,
        }
    }

    /// 设置是否验证服务器证书
    pub fn set_verify_server(mut self, verify: bool) -> Self {
        self.verify_server = verify;
        self
    }
}

/// TLS 连接器
pub struct TlsConnector {
    config: TlsClientConfig,
    cert_store: Arc<CertStore>,
}

impl TlsConnector {
    /// 创建 TLS 连接器
    pub fn new(config: TlsClientConfig) -> Self {
        Self {
            config,
            cert_store: Arc::new(CertStore::new()),
        }
    }

    /// 初始化证书存储
    pub fn init_certs(&mut self) -> Result<()> {
        self.cert_store.load_ca_cert(&self.config.ca_cert_path)?;
        if !self.config.client_cert_path.is_empty() {
            self.cert_store.load_client_cert(&self.config.client_cert_path)?;
        }
        Ok(())
    }

    /// 获取证书存储
    pub fn cert_store(&self) -> &Arc<CertStore> {
        &self.cert_store
    }

    /// 是否启用服务器证书验证
    pub fn should_verify_server(&self) -> bool {
        self.config.verify_server
    }
}

/// 构建 rustls ClientConfig
pub fn build_rustls_config(connector: &TlsConnector) -> Result<rustls::ClientConfig> {
    let mut config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(DummyVerifier))
        .with_no_client_auth();

    // 设置客户端证书
    if !connector.config.client_cert_path.is_empty() {
        let cert_data = std::fs::read(&connector.config.client_cert_path)
            .map_err(|e| GmError::TlsConfigError(format!("读取客户端证书失败: {}", e)))?;
        let key_data = std::fs::read(&connector.config.client_key_path)
            .map_err(|e| GmError::TlsConfigError(format!("读取客户端密钥失败: {}", e)))?;

        let certs = rustls::pemfile::certs(&mut cert_data.as_ref())
            .map_err(|e| GmError::TlsConfigError(format!("解析证书失败: {:?}", e)))?;
        let key = rustls::pemfile::private_key(&mut key_data.as_ref())
            .map_err(|e| GmError::TlsConfigError(format!("解析密钥失败: {:?}", e)))?;

        config.set_single_cert(certs, key)
            .map_err(|e| GmError::TlsConfigError(format!("设置证书失败: {:?}", e)))?;
    }

    Ok(config)
}

/// 虚拟证书验证器（实际使用时替换为真实验证）
struct DummyVerifier;

impl rustls::client::danger::ServerCertVerifier for DummyVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer,
        _intermediate_certs: &[rustls::pki_types::CertificateDer],
        _server_name: &rustls::pki_types::ServerName,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
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