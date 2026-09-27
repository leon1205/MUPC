//! 证书生命周期管理
//!
//! 管理 SM2 证书的申请、导入、更新、吊销全生命周期
//!
//! 证书管理：framework-only（国密证书合规延后）。

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

/// 有效期判据（**含两端**：`not_before <= now <= not_after`）。
///
/// 抽成纯函数 ⇒ 可用"人造区间"直接钉住判别力，不依赖真实证书解析（同 crate `tls_sm2` 的口径）。
fn cert_in_validity_window(cert: &Sm2Cert, now: DateTime<Utc>) -> bool {
    now >= cert.not_before() && now <= cert.not_after()
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

    /// 客户端证书是否可用（**吊销 + 有效期**双检查）。
    ///
    /// # 为什么必须查有效期（PRD 06 LEA-58）
    /// 此前只查 CRL ⇒ 一张**已过期**的证书照样"有效"。口径与同 crate 的
    /// [`crate::tls_sm2::Sm2ServerCertVerifier::verify`]（`not_after` / `not_before` 双检查）一致：
    /// 过期或尚未生效一律拒。
    pub fn is_cert_valid(&self) -> bool {
        self.client_cert.as_ref().is_some_and(|c| {
            let serial = c.serial_number();
            !self.crl.is_revoked(&serial) && cert_in_validity_window(c, Utc::now())
        })
    }

    /// 仅供测试注入客户端证书（生产装配走 [`CertManager::load_client_cert`]）。
    #[cfg(test)]
    fn set_client_cert_for_test(&mut self, cert: Sm2Cert) {
        self.client_cert = Some(cert);
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn cert(not_before: DateTime<Utc>, not_after: DateTime<Utc>) -> Sm2Cert {
        Sm2Cert::new("PEM-STUB".to_string(), not_before, not_after)
    }

    fn mgr_with(c: Sm2Cert) -> CertManager {
        let mut m = CertManager::new("/nonexistent/crl.pem");
        m.set_client_cert_for_test(c);
        m
    }

    /// 判别力：**已过期**证书必须判无效。旧实现只查 CRL ⇒ 本用例红。
    #[test]
    fn expired_cert_is_invalid() {
        let now = Utc::now();
        let m = mgr_with(cert(now - Duration::days(10), now - Duration::days(1)));
        assert!(!m.is_cert_valid(), "已过期证书必须判无效（PRD 06 LEA-58）");
    }

    /// 判别力：**尚未生效**证书必须判无效（与 `tls_sm2` 的 `not_before` 检查同口径）。
    #[test]
    fn not_yet_valid_cert_is_invalid() {
        let now = Utc::now();
        let m = mgr_with(cert(now + Duration::days(1), now + Duration::days(10)));
        assert!(!m.is_cert_valid(), "尚未生效的证书必须判无效");
    }

    #[test]
    fn cert_within_window_and_not_revoked_is_valid() {
        let now = Utc::now();
        let m = mgr_with(cert(now - Duration::days(1), now + Duration::days(1)));
        assert!(m.is_cert_valid(), "区间内且未吊销 ⇒ 有效（防误杀）");
    }

    #[test]
    fn revoked_cert_is_invalid() {
        let now = Utc::now();
        let mut m = mgr_with(cert(now - Duration::days(1), now + Duration::days(1)));
        // `Sm2Cert::serial_number()` 当前是 stub（恒 "unknown"）⇒ 按同一序列号吊销即可命中
        m.revoke_cert("unknown").unwrap();
        assert!(!m.is_cert_valid(), "CRL 命中仍须判无效（有效期以外的既有判据不得丢）");
    }

    /// 边界口径：**含两端**（`now == not_after` 仍有效；越界 1ns 即无效）。
    #[test]
    fn validity_window_is_inclusive_at_both_ends() {
        let nb = Utc::now();
        let na = nb + Duration::seconds(60);
        let c = cert(nb, na);
        assert!(cert_in_validity_window(&c, nb), "起点含");
        assert!(cert_in_validity_window(&c, na), "终点含");
        assert!(!cert_in_validity_window(&c, na + Duration::nanoseconds(1)));
        assert!(!cert_in_validity_window(&c, nb - Duration::nanoseconds(1)));
    }

    #[test]
    fn no_client_cert_is_invalid() {
        let m = CertManager::new("/nonexistent/crl.pem");
        assert!(!m.is_cert_valid());
    }
}
