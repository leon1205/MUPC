//! SM2 TLS 加密提供者
//!
//! 为 rustls 提供 SM2 证书校验和 SM4 对称加密支持

use crate::cert::Sm2Cert;
use crate::errors::SecurityError;

/// SM2 加密提供者（Phase 2+ 实现）
pub struct SmCryptoProvider {
    server_cert: Option<Sm2Cert>,
    client_cert: Option<Sm2Cert>,
}

impl SmCryptoProvider {
    pub fn new() -> Self {
        todo!("Phase 2+")
    }

    pub fn with_server_cert(cert: Sm2Cert) -> Self {
        todo!("Phase 2+")
    }

    pub fn with_client_cert(cert: Sm2Cert) -> Self {
        todo!("Phase 2+")
    }
}

/// SM2 服务端证书校验器（Phase 2+ 实现）
pub struct Sm2ServerCertVerifier {
    trusted_certs: Vec<Sm2Cert>,
}

impl Sm2ServerCertVerifier {
    pub fn new(trusted_certs: Vec<Sm2Cert>) -> Self {
        todo!("Phase 2+")
    }

    pub fn verify(&self, cert: &Sm2Cert) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }
}
