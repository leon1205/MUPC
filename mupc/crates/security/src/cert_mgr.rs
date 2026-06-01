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

/// CRL 管理器
pub struct CrlManager {
    crl_path: String,
    revoked: Vec<String>,
}

impl CrlManager {
    pub fn new(crl_path: &str) -> Self {
        Self {
            crl_path: crl_path.to_string(),
            revoked: Vec::new(),
        }
    }

    pub fn load_crl(&mut self) -> Result<(), SecurityError> {
        let data = std::fs::read_to_string(&self.crl_path)
            .map_err(|e| SecurityError::IoError(format!("{}", e)))?;
        self.revoked.clear();
        for line in data.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                self.revoked.push(trimmed.to_string());
            }
        }
        Ok(())
    }

    pub fn is_revoked(&self, serial: &str) -> bool {
        self.revoked.iter().any(|s| s == serial)
    }

    pub fn refresh(&mut self) -> Result<(), SecurityError> {
        self.load_crl()
    }
}

/// 证书管理器
pub struct CertManager {
    ca_cert: Option<Sm2Cert>,
    client_cert: Option<Sm2Cert>,
    client_key: Option<Vec<u8>>,
    crl: CrlManager,
}

impl CertManager {
    pub fn new(crl_path: &str) -> Self {
        Self {
            ca_cert: None,
            client_cert: None,
            client_key: None,
            crl: CrlManager::new(crl_path),
        }
    }

    pub fn load_ca_cert(&mut self, path: &str) -> Result<(), SecurityError> {
        let cert = crate::cert::load_sm2_certificate(path)?;
        self.ca_cert = Some(cert);
        Ok(())
    }

    pub fn load_client_cert(&mut self, path: &str) -> Result<(), SecurityError> {
        let cert = crate::cert::load_sm2_certificate(path)?;
        self.client_cert = Some(cert);
        Ok(())
    }

    pub fn load_client_key(&mut self, path: &str) -> Result<(), SecurityError> {
        let key = std::fs::read(path).map_err(|e| SecurityError::IoError(format!("{}", e)))?;
        self.client_key = Some(key);
        Ok(())
    }

    pub fn is_cert_valid(&self) -> bool {
        self.client_cert.as_ref().is_some_and(|c| {
            let serial = c.serial_number();
            !self.crl.is_revoked(&serial)
        })
    }

    pub fn days_until_expiry(&self) -> Option<i64> {
        let cert = self.client_cert.as_ref()?;
        let now = Utc::now();
        let remaining = cert.not_after() - now;
        Some(remaining.num_days())
    }

    pub fn renew_client_cert(&mut self) -> Result<(), SecurityError> {
        // Phase 2+: 对接 CA 服务签发新证书
        // 当前 stub: 标记需人工更新
        tracing::warn!("证书续期需通过外部 CA 服务完成");
        Err(SecurityError::ConfigError("证书续期功能待实现，请通过外部 CA 服务手动更新".into()))
    }

    pub fn revoke_cert(&mut self, serial: &str) -> Result<(), SecurityError> {
        self.crl.revoked.push(serial.to_string());
        tracing::info!(serial, "证书已加入本地吊销列表");
        Ok(())
    }
}
