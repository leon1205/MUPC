//! MUPC Security Module - 国密 SM2/SM4 和 TLS 支持
//!
//! 提供国密算法实现和 TLS 加密通信能力

pub mod cert;
mod errors;
mod sm2;
mod sm3;
mod sm4;
mod tls;

// Phase 2+ 模块（全部为 stub 骨架，需要硬件/外部SDK支持）
//
// 阻塞条件汇总：
// ┌────────────────────┬──────────────────────────────────┬───────────┐
// │ 模块               │ 阻塞条件                          │ 目标      │
// ├────────────────────┼──────────────────────────────────┼───────────┤
// │ secure_boot        │ 硬件 TPM/信任根 + 安全 ROM 公钥  │ Phase 2+  │
// │ lea / lea_vici     │ Linux 内核 IPSec + VICI Unix 套接字 │ Phase 2+ │
// │ cert_mgr (自动续期) │ CA 服务器 API + ACME 协议        │ Phase 2+  │
// │ compliance         │ 国网/南网合规检查清单定稿          │ Phase 2+  │
// │ alarm              │ 告警北向推送通道（MQTT/IEC104）   │ Phase 2+  │
// └────────────────────┴──────────────────────────────────┴───────────┘
pub mod alarm;
pub mod audit;
pub mod cert_mgr;
pub mod compliance;
pub mod lea;
pub mod lea_vici;
pub mod policy;
pub mod secure_boot;
pub mod tls_sm2;

// 基础国密原语
pub use cert::{load_sm2_certificate, CertStore, Sm2Cert};
pub use errors::{GmError, Result, SecurityError};
pub use sm2::{
    rs_to_signature, signature_to_rs, sm2_derive_shared_key, sm2_key_generate, sm2_sign,
    sm2_verify, Sm2KeyPair, Sm2Signature,
};
pub use sm3::{sm3_derive_key, sm3_hash};
pub use sm4::{
    generate_iv, sm4_cbc_decrypt, sm4_cbc_encrypt, sm4_gcm_decrypt, sm4_gcm_encrypt, Sm4Key,
};
pub use tls::{TlsClientConfig, TlsConnector};

// Phase 2+ 重导出
pub use alarm::{AlertEvent, AlertManager, AlertSeverity, AlertSink, AlertType};
pub use audit::{AuditEventType, AuditLogEntry, AuditLogger, AuditSeverity};
pub use cert_mgr::{CertManager, CertMeta, CrlManager};
pub use compliance::{ComplianceChecker, ComplianceItem, ComplianceReport};
pub use lea::{LeaConfig, LeaManager, TunnelState};
pub use lea_vici::ViciClient;
pub use policy::{ChannelPolicy, PolicyManager};
pub use secure_boot::{BootChainStatus, SecureBootManager};
pub use tls_sm2::{Sm2ServerCertVerifier, SmCryptoProvider};

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
        assert!(config.ca_cert.is_empty());
    }
}
