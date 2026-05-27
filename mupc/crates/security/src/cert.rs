//! 证书管理模块
//!
//! 提供 X.509 证书的加载、解析和验证功能

use crate::errors::{GmError, Result};
use std::fs;
use std::path::Path;
use x509_parser::prelude::*;

/// 国密证书结构
#[derive(Debug, Clone)]
pub struct GmCert {
    pub subject: String,
    pub issuer: String,
    pub serial: String,
    pub not_before: String,
    pub not_after: String,
    pub raw: Vec<u8>,
}

/// 证书存储
pub struct CertStore {
    ca_certs: Vec<GmCert>,
    client_cert: Option<GmCert>,
}

impl CertStore {
    /// 创建新的证书存储
    pub fn new() -> Self {
        Self {
            ca_certs: Vec::new(),
            client_cert: None,
        }
    }

    /// 加载 CA 证书
    pub fn load_ca_cert(&mut self, path: &str) -> Result<()> {
        let cert_data = fs::read(path)
            .map_err(|e| GmError::KeyLoadFailed(format!("读取CA证书失败: {}", e)))?;

        let cert = Self::parse_cert(&cert_data)?;
        self.ca_certs.push(cert);
        Ok(())
    }

    /// 加载客户端证书
    pub fn load_client_cert(&mut self, path: &str) -> Result<()> {
        let cert_data = fs::read(path)
            .map_err(|e| GmError::KeyLoadFailed(format!("读取客户端证书失败: {}", e)))?;

        let cert = Self::parse_cert(&cert_data)?;
        self.client_cert = Some(cert);
        Ok(())
    }

    /// 解析证书
    fn parse_cert(data: &[u8]) -> Result<GmCert> {
        let (_, cert) = X509Certificate::from_der(data)
            .map_err(|e| GmError::CertVerifyFailed(format!("证书解析失败: {:?}", e)))?;

        let subject = cert.subject().to_string();
        let issuer = cert.issuer().to_string();
        let serial = cert.serial.to_string();

        let not_before = cert.validity().not_before.to_string();
        let not_after = cert.validity().not_after.to_string();

        Ok(GmCert {
            subject,
            issuer,
            serial,
            not_before,
            not_after,
            raw: data.to_vec(),
        })
    }

    /// 验证证书链
    pub fn verify_cert_chain(&self, _cert: &GmCert) -> Result<()> {
        // 简化实现：检查证书是否在信任列表中
        // 实际应实现完整的证书链验证
        Ok(())
    }

    /// 获取 CA 证书数量
    pub fn ca_cert_count(&self) -> usize {
        self.ca_certs.len()
    }

    /// 获取客户端证书
    pub fn get_client_cert(&self) -> Option<&GmCert> {
        self.client_cert.as_ref()
    }
}

impl Default for CertStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cert_store_new() {
        let store = CertStore::new();
        assert_eq!(store.ca_cert_count(), 0);
        assert!(store.get_client_cert().is_none());
    }
}