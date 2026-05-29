//! 证书生命周期管理
//!
//! 管理 SM2 证书的申请、导入、更新、吊销全生命周期

use crate::cert::Sm2Cert;
use crate::errors::SecurityError;
use chrono::{DateTime, Utc};

/// 证书元信息
#[derive(Debug, Clone)]
pub struct CertMeta {
    pub serial_number: String,
    pub subject: String,
    pub issuer: String,
    pub not_before: DateTime<Utc>,
    pub not_after: DateTime<Utc>,
    pub fingerprint_sm3: String,
}

/// CRL 管理器（Phase 2+ 实现）
pub struct CrlManager {
    crl_path: String,
    cache: Vec<String>,
}

impl CrlManager {
    pub fn new(crl_path: &str) -> Self {
        todo!("Phase 2+")
    }

    pub fn load_crl(&mut self) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn is_revoked(&self, serial: &str) -> bool {
        todo!("Phase 2+")
    }

    pub fn refresh(&mut self) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }
}

/// 证书管理器（Phase 2+ 实现）
pub struct CertManager {
    ca_cert: Option<Sm2Cert>,
    client_cert: Option<Sm2Cert>,
    client_key: Option<Vec<u8>>,
    crl: CrlManager,
}

impl CertManager {
    pub fn new(crl_path: &str) -> Self {
        todo!("Phase 2+")
    }

    pub fn load_ca_cert(&mut self, path: &str) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn load_client_cert(&mut self, path: &str) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn load_client_key(&mut self, path: &str) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn is_cert_valid(&self) -> bool {
        todo!("Phase 2+")
    }

    pub fn days_until_expiry(&self) -> Option<i64> {
        todo!("Phase 2+")
    }

    pub fn renew_client_cert(&mut self) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn revoke_cert(&mut self, serial: &str) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }
}
