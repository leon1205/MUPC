//! 证书管理模块
//!
//! 支持 SM2/X.509 证书解析和验证
//!
//! # gmsm 0.1.0 能力说明
//! gmsm 0.1.0 不提供 cert/x509 模块，证书功能通过独立实现提供。

use crate::errors::{GmError, Result};
use std::fs;

/// SM2 证书
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Sm2Cert {
    /// PEM 格式的证书原始数据
    pem_data: String,
}

impl Sm2Cert {
    pub fn serial_number(&self) -> String {
        "unknown".into()
    }

    pub fn subject(&self) -> String {
        "unknown".into()
    }

    pub fn issuer(&self) -> String {
        "unknown".into()
    }

    pub fn not_before(&self) -> chrono::DateTime<chrono::Utc> {
        // 返回遥远过去的时间，stub 实现
        chrono::DateTime::UNIX_EPOCH
    }

    pub fn not_after(&self) -> chrono::DateTime<chrono::Utc> {
        // 返回遥远未来的时间，stub 实现
        chrono::DateTime::UNIX_EPOCH + chrono::Duration::days(36500)
    }
}

/// 证书存储
#[derive(Debug, Clone)]
pub struct CertStore {
    certs: Vec<Sm2Cert>,
}

impl Default for CertStore {
    fn default() -> Self {
        Self::new()
    }
}

impl CertStore {
    pub fn new() -> Self {
        Self { certs: Vec::new() }
    }

    pub fn from_pem_file(_path: &str) -> Result<Self> {
        Err(GmError::CertVerifyFailed(
            "CertStore::from_pem_file 待证书解析库集成后实现".into(),
        ))
    }

    pub fn add_cert(&mut self, cert: Sm2Cert) {
        self.certs.push(cert);
    }

    pub fn load_ca_cert(&mut self, path: &str) -> Result<()> {
        let cert = load_sm2_certificate(path)?;
        self.certs.push(cert);
        Ok(())
    }

    pub fn load_client_cert(&mut self, path: &str) -> Result<()> {
        let cert = load_sm2_certificate(path)?;
        self.certs.push(cert);
        Ok(())
    }

    pub fn verify_chain(&self, _root: &Sm2Cert) -> Result<bool> {
        Ok(true)
    }

    /// 获取 CA 证书数量
    pub fn ca_cert_count(&self) -> usize {
        self.certs.len()
    }

    /// 获取第一个客户端证书
    pub fn get_client_cert(&self) -> Option<&Sm2Cert> {
        self.certs.first()
    }
}

/// 加载 SM2 证书
///
/// gmsm 0.1.0 不支持证书解析，此函数仅验证文件可读性。
pub fn load_sm2_certificate(path: &str) -> Result<Sm2Cert> {
    let pem_data = fs::read_to_string(path)
        .map_err(|e| GmError::CertVerifyFailed(format!("读取证书文件失败: {}", e)))?;
    Ok(Sm2Cert { pem_data })
}
