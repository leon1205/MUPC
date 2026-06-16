//! 证书生命周期管理
//!
//! 管理 SM2 证书的申请、导入、更新、吊销全生命周期

use crate::cert::Sm2Cert;
use crate::errors::SecurityError;
use chrono::{DateTime, Utc};

/// 证书类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertType {
    /// CA 根证书
    Ca,
    /// 客户端证书
    Client,
}

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

impl CertMeta {
    fn from_cert(cert: &Sm2Cert) -> Self {
        Self {
            serial_number: cert.serial_number(),
            subject: cert.subject(),
            issuer: cert.issuer(),
            not_before: cert.not_before(),
            not_after: cert.not_after(),
            fingerprint_sm3: "待 SM3 集成".into(),
        }
    }
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

    /// 从原始数据导入 CRL 并写入文件
    pub fn import_crl(&mut self, data: &[u8]) -> Result<(), SecurityError> {
        std::fs::write(&self.crl_path, data)
            .map_err(|e| SecurityError::IoError(format!("写入 CRL 文件失败: {}", e)))?;
        self.load_crl()
    }

    /// 获取 CRL 中已吊销的序列号列表
    pub fn revoked_serials(&self) -> &[String] {
        &self.revoked
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
    ca_cert_path: Option<String>,
    client_cert: Option<Sm2Cert>,
    client_cert_path: Option<String>,
    client_key: Option<Vec<u8>>,
    client_key_path: Option<String>,
    crl: CrlManager,
}

impl CertManager {
    pub fn new(crl_path: &str) -> Self {
        Self {
            ca_cert: None,
            ca_cert_path: None,
            client_cert: None,
            client_cert_path: None,
            client_key: None,
            client_key_path: None,
            crl: CrlManager::new(crl_path),
        }
    }

    // ── 加载 ──

    pub fn load_ca_cert(&mut self, path: &str) -> Result<(), SecurityError> {
        let cert = crate::cert::load_sm2_certificate(path)?;
        self.ca_cert_path = Some(path.to_string());
        self.ca_cert = Some(cert);
        Ok(())
    }

    pub fn load_client_cert(&mut self, path: &str) -> Result<(), SecurityError> {
        let cert = crate::cert::load_sm2_certificate(path)?;
        self.client_cert_path = Some(path.to_string());
        self.client_cert = Some(cert);
        Ok(())
    }

    pub fn load_client_key(&mut self, path: &str) -> Result<(), SecurityError> {
        let key = std::fs::read(path).map_err(|e| SecurityError::IoError(format!("{}", e)))?;
        self.client_key_path = Some(path.to_string());
        self.client_key = Some(key);
        Ok(())
    }

    // ── 导入 ──

    /// 导入证书 — 将原始 DER/PEM 数据写入文件并加载
    pub fn import_cert(
        &mut self,
        cert_type: CertType,
        target_path: &str,
        data: &[u8],
    ) -> Result<(), SecurityError> {
        // 确保目标目录存在
        if let Some(parent) = std::path::Path::new(target_path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| SecurityError::IoError(format!("创建证书目录失败: {}", e)))?;
        }
        std::fs::write(target_path, data)
            .map_err(|e| SecurityError::IoError(format!("写入证书文件失败: {}", e)))?;

        match cert_type {
            CertType::Ca => self.load_ca_cert(target_path),
            CertType::Client => self.load_client_cert(target_path),
        }
    }

    /// 导入 CRL — 将原始数据写入 CRL 文件并加载
    pub fn import_crl(&mut self, data: &[u8]) -> Result<(), SecurityError> {
        self.crl.import_crl(data)
    }

    // ── 重载 ──

    /// 从原始路径重新加载所有证书和密钥
    pub fn reload(&mut self) -> Result<(), SecurityError> {
        if let Some(ref path) = self.ca_cert_path.clone() {
            self.load_ca_cert(&path)?;
        }
        if let Some(ref path) = self.client_cert_path.clone() {
            self.load_client_cert(&path)?;
        }
        if let Some(ref path) = self.client_key_path.clone() {
            self.load_client_key(&path)?;
        }
        self.crl.refresh()?;
        tracing::info!("证书和 CRL 已重新加载");
        Ok(())
    }

    // ── 查询 ──

    /// 列出所有已加载证书的元信息
    pub fn list_certs(&self) -> Vec<CertMeta> {
        let mut certs = Vec::new();
        if let Some(ref cert) = self.ca_cert {
            certs.push(CertMeta::from_cert(cert));
        }
        if let Some(ref cert) = self.client_cert {
            certs.push(CertMeta::from_cert(cert));
        }
        certs
    }

    // ── 状态 ──

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

    // ── 生命周期 ──

    pub fn renew_client_cert(&mut self) -> Result<(), SecurityError> {
        tracing::warn!("证书续期需通过外部 CA 服务完成");
        Err(SecurityError::ConfigError(
            "证书续期功能待实现，请通过外部 CA 服务手动更新".into(),
        ))
    }

    pub fn revoke_cert(&mut self, serial: &str) -> Result<(), SecurityError> {
        self.crl.revoked.push(serial.to_string());
        tracing::info!(serial, "证书已加入本地吊销列表");
        Ok(())
    }
}
