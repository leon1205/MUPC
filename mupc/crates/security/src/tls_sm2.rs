//! SM2 TLS 加密提供者
//!
//! 为 rustls 提供 SM2 证书校验和国密加密套件支持。
//!
//! Phase 2+ 当前实现基于证书链的校验逻辑，
//! 后续集成 GmSSL 动态库实现完整的 SM2/SM4 加密套件。

use crate::cert::Sm2Cert;
use crate::errors::SecurityError;

/// SM2 加密提供者（Phase 2+ 开发占位）
///
/// 当前为开发占位实现。Phase 2+ 将集成 GmSSL 动态库，实现 rustls::CryptoProvider trait。
/// 在此之前，TLS 加密由 ring 默认 provider 提供。
pub struct SmCryptoProvider {
    server_cert: Option<Sm2Cert>,
    client_cert: Option<Sm2Cert>,
}

impl SmCryptoProvider {
    pub fn new() -> Self {
        Self {
            server_cert: None,
            client_cert: None,
        }
    }

    pub fn with_server_cert(mut self, cert: Sm2Cert) -> Self {
        self.server_cert = Some(cert);
        self
    }

    pub fn with_client_cert(mut self, cert: Sm2Cert) -> Self {
        self.client_cert = Some(cert);
        self
    }

    /// 检查是否已配置服务端证书
    pub fn has_server_cert(&self) -> bool {
        self.server_cert.is_some()
    }

    /// 检查是否已配置客户端证书
    pub fn has_client_cert(&self) -> bool {
        self.client_cert.is_some()
    }
}

impl Default for SmCryptoProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// SM2 服务端证书校验器
///
/// 基于信任证书列表验证对端证书。
pub struct Sm2ServerCertVerifier {
    trusted_certs: Vec<Sm2Cert>,
}

impl Sm2ServerCertVerifier {
    pub fn new(trusted_certs: Vec<Sm2Cert>) -> Self {
        Self { trusted_certs }
    }

    /// 验证证书是否在信任列表中
    ///
    /// Phase 2+: 实现完整的 X.509 证书链验证和 SM2 签名校验。
    pub fn verify(&self, cert: &Sm2Cert) -> Result<(), SecurityError> {
        let subject = cert.subject();

        // 检查证书是否受信任
        let trusted = self
            .trusted_certs
            .iter()
            .any(|tc| tc.issuer() == cert.issuer());

        if !trusted {
            return Err(SecurityError::CertVerifyFailed(format!(
                "证书 {} 不在信任列表中",
                subject
            )));
        }

        // 检查证书是否过期
        let now = chrono::Utc::now();
        if now > cert.not_after() {
            return Err(SecurityError::CertVerifyFailed(format!(
                "证书 {} 已过期",
                subject
            )));
        }
        if now < cert.not_before() {
            return Err(SecurityError::CertVerifyFailed(format!(
                "证书 {} 尚未生效",
                subject
            )));
        }

        tracing::info!(subject = %subject, "SM2 证书校验通过");
        Ok(())
    }

    /// 添加信任证书
    pub fn add_trusted_cert(&mut self, cert: Sm2Cert) {
        self.trusted_certs.push(cert);
    }

    /// 获取信任证书数量
    pub fn trusted_count(&self) -> usize {
        self.trusted_certs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cert::load_sm2_certificate as _;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_pem_file(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{}", content).unwrap();
        f
    }

    #[test]
    fn test_sm_crypto_provider_default() {
        let provider = SmCryptoProvider::new();
        assert!(!provider.has_server_cert());
        assert!(!provider.has_client_cert());
    }

    #[test]
    fn test_cert_verifier_empty_trust_list() {
        let verifier = Sm2ServerCertVerifier::new(vec![]);
        assert_eq!(verifier.trusted_count(), 0);
    }
}
