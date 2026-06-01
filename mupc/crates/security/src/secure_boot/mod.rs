//! 安全启动模块
//!
//! 实现安全启动链验证、完整性监控和防回滚机制

pub mod audit;
pub mod health;
pub mod monitor;
pub mod rollback;
pub mod status;

use crate::errors::SecurityError;

/// 启动链状态
#[derive(Debug, Clone, PartialEq)]
pub enum BootChainStatus {
    NotVerified,
    Verified,
    Failed(String),
}

/// 安全启动管理器
///
/// 组合启动状态、完整性监控、审计、健康检查和防回滚保护。
pub struct SecureBootManager {
    pub status: status::BootStatus,
    pub monitor: monitor::IntegrityMonitor,
    pub audit_log: audit::BootAuditLogger,
    pub health: health::HealthChecker,
    pub rollback: rollback::RollbackProtection,
}

impl Default for SecureBootManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SecureBootManager {
    pub fn new() -> Self {
        Self {
            status: status::BootStatus::new(),
            monitor: monitor::IntegrityMonitor::new(),
            audit_log: audit::BootAuditLogger::new(),
            health: health::HealthChecker::new(),
            rollback: rollback::RollbackProtection::new(),
        }
    }

    /// 验证启动链
    ///
    /// Phase 2+: 集成 U-Boot verified boot 进行完整链验证。
    /// 当前 stub 返回 Verified。
    pub fn verify_boot_chain(&mut self) -> Result<BootChainStatus, SecurityError> {
        tracing::info!("启动链验证开始");

        // Phase 2+: 读取并验证 SPL → U-Boot → Kernel → RootFS 链
        self.audit_log
            .record("boot_chain_verify", true, "启动链验证通过 (stub)")?;

        tracing::info!("启动链验证通过");
        Ok(BootChainStatus::Verified)
    }

    /// 验证固件镜像签名
    ///
    /// Phase 2+: 使用 SM2 签名验证镜像完整性。
    pub fn verify_image(
        &self,
        _image_path: &str,
        _signature_path: &str,
    ) -> Result<bool, SecurityError> {
        // Phase 2+: 读取镜像文件 → SM3 哈希 → SM2 验签
        tracing::debug!("镜像签名验证 - 待实现");
        Ok(true)
    }

    /// 获取安全启动证明报告
    ///
    /// Phase 2+: 生成包含 TPM 度量值的证明报告。
    pub fn get_attestation_report(&self) -> Result<String, SecurityError> {
        let report = serde_json::json!({
            "boot_verified": self.status.is_verified(),
            "chain_status": self.status.chain_status,
            "boot_count": self.status.boot_count,
            "rollback_protection": {
                "current_version": self.rollback.current_version,
                "min_version": self.rollback.min_version,
            },
            "integrity_monitor": self.monitor.enabled,
            "health": self.health.get_status(),
        });

        serde_json::to_string_pretty(&report)
            .map_err(|e| SecurityError::BootError(format!("生成证明报告失败: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secure_boot_manager_new() {
        let mgr = SecureBootManager::new();
        assert_eq!(mgr.rollback.current_version, 1);
        assert!(mgr.monitor.enabled);
    }

    #[test]
    fn test_verify_boot_chain() {
        let mut mgr = SecureBootManager::new();
        let result = mgr.verify_boot_chain().unwrap();
        assert_eq!(result, BootChainStatus::Verified);
    }

    #[test]
    fn test_verify_image_stub() {
        let mgr = SecureBootManager::new();
        assert!(mgr.verify_image("image.bin", "image.sig").unwrap());
    }

    #[test]
    fn test_attestation_report() {
        let mgr = SecureBootManager::new();
        let report = mgr.get_attestation_report().unwrap();
        assert!(report.contains("boot_verified"));
    }
}
