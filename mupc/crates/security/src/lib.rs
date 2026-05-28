//! MUPC Security Module - 国密 SM2/SM4 和 TLS 支持
//!
//! 提供国密算法实现和 TLS 加密通信能力

mod cert;
mod errors;
mod sm2;
mod sm3;
mod sm4;
mod tls;

pub use cert::{CertStore, Sm2Cert, load_sm2_certificate};
pub use errors::{GmError, Result};
pub use sm2::{sm2_sign, sm2_verify, sm2_key_generate, sm2_derive_shared_key, Sm2KeyPair, Sm2Signature};
pub use sm3::{sm3_hash, sm3_derive_key};
pub use sm4::{Sm4Key, sm4_cbc_encrypt, sm4_cbc_decrypt, sm4_gcm_encrypt, sm4_gcm_decrypt, generate_iv};
pub use tls::{TlsClientConfig, TlsConnector};

/// 国密配置
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GmConfig {
    pub enabled: bool,
    pub sm2_private_key: Option<String>,
    pub sm2_public_key: Option<String>,
    pub sm4_key: Option<String>,
    pub ca_cert: String,
    pub client_cert: String,
    pub client_key: String,
}

impl Default for GmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sm2_private_key: None,
            sm2_public_key: None,
            sm4_key: None,
            ca_cert: String::new(),
            client_cert: String::new(),
            client_key: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gm_config_default() {
        let config = GmConfig::default();
        assert!(config.enabled);
        assert!(!config.ca_cert.is_empty() == false);
    }
}