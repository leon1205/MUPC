//! 安全健康检查

use crate::errors::SecurityError;
use chrono::Utc;

/// 健康检查器
#[derive(Debug, Clone)]
pub struct HealthChecker {
    pub healthy: bool,
    pub last_check: Option<String>,
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthChecker {
    pub fn new() -> Self {
        Self {
            healthy: true,
            last_check: None,
        }
    }

    /// 执行健康检查
    ///
    /// Phase 2+: 扩展检查项：SM2 密钥可用性、证书有效期、CRL 状态。
    pub fn check_health(&self) -> Result<bool, SecurityError> {
        Ok(self.healthy)
    }

    /// 获取健康状态摘要
    pub fn get_status(&self) -> String {
        if self.healthy {
            "healthy".into()
        } else {
            "unhealthy".into()
        }
    }

    /// 执行自检
    pub fn perform_self_test(&mut self) -> Result<(), SecurityError> {
        let now = Utc::now().to_rfc3339();
        self.last_check = Some(now.clone());

        // Phase 2+: 检查密钥存储、证书链、CRL、安全策略
        tracing::info!(timestamp = %now, "安全自检完成");
        self.healthy = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_healthy() {
        let checker = HealthChecker::new();
        assert!(checker.healthy);
        assert_eq!(checker.get_status(), "healthy");
    }

    #[test]
    fn test_self_test_updates_timestamp() {
        let mut checker = HealthChecker::new();
        checker.perform_self_test().unwrap();
        assert!(checker.last_check.is_some());
    }
}
