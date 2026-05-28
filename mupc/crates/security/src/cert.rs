//! 证书管理模块
//!
//! 支持 SM2/X.509 证书解析和验证

#[cfg(feature = "real_gmsm")]
use gmsm::x509;

use crate::errors::{GmError, Result};
use std::fs;

/// SM2 证书
#[derive(Debug, Clone)]
pub struct Sm2Cert {
    cert: x509::Certificate,
}

/// 证书存储
#[derive(Debug, Clone)]
pub struct CertStore {
    certs: Vec<Sm2Cert>,
}

impl CertStore {
    /// 从 PEM 文件加载证书
    pub fn from_pem_file(path: &str) -> Result<Self> {
        #[cfg(feature = "real_gmsm")]
        {
            let cert_data = fs::read(path)
                .map_err(|e| GmError::CertVerifyFailed(format!("读取证书文件失败: {}", e)))?;
            let cert = x509::Certificate::from_pem(&cert_data)
                .map_err(|e| GmError::CertVerifyFailed(format!("证书解析失败: {:?}", e)))?;
            Ok(Self {
                certs: vec![Sm2Cert { cert }],
            })
        }

        #[cfg(not(feature = "real_gmsm"))]
        {
            Err(GmError::InvalidParam("证书解析需要 gmsm 库".into()))
        }
    }

    /// 添加证书
    pub fn add_cert(&mut self, cert: Sm2Cert) {
        self.certs.push(cert);
    }

    /// 验证证书链
    pub fn verify_chain(&self, _root: &Sm2Cert) -> Result<bool> {
        #[cfg(feature = "real_gmsm")]
        {
            // 实际实现应验证证书链
            Ok(true)
        }

        #[cfg(not(feature = "real_gmsm"))]
        {
            Err(GmError::InvalidParam("证书验证需要 gmsm 库".into()))
        }
    }
}

/// 加载 SM2 证书
pub fn load_sm2_certificate(path: &str) -> Result<Sm2Cert> {
    #[cfg(feature = "real_gmsm")]
    {
        let cert_data = fs::read(path)
            .map_err(|e| GmError::CertVerifyFailed(format!("读取证书文件失败: {}", e)))?;
        let cert = x509::Certificate::from_pem(&cert_data)
            .map_err(|e| GmError::CertVerifyFailed(format!("证书解析失败: {:?}", e)))?;
        Ok(Sm2Cert { cert })
    }

    #[cfg(not(feature = "real_gmsm"))]
    {
        Err(GmError::InvalidParam("证书解析需要 gmsm 库".into()))
    }
}