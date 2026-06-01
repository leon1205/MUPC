//! 防回滚保护

use crate::errors::SecurityError;

/// 防回滚保护
#[derive(Debug, Clone)]
pub struct RollbackProtection {
    pub min_version: u32,
    pub current_version: u32,
}

impl Default for RollbackProtection {
    fn default() -> Self {
        Self::new()
    }
}

impl RollbackProtection {
    pub fn new() -> Self {
        Self {
            min_version: 1,
            current_version: 1,
        }
    }

    /// 检查目标版本是否允许安装（不低于最低版本）
    pub fn check_version(&self, version: u32) -> Result<bool, SecurityError> {
        if version < self.min_version {
            tracing::warn!(
                proposed = version,
                min = self.min_version,
                "检测到回滚尝试"
            );
            return Ok(false);
        }
        Ok(true)
    }

    /// 记录当前版本
    pub fn record_version(&mut self, version: u32) -> Result<(), SecurityError> {
        if version > self.current_version {
            tracing::info!(
                old = self.current_version,
                new = version,
                "固件版本已更新"
            );
            self.current_version = version;
        }
        Ok(())
    }

    /// 检测回滚尝试
    pub fn is_rollback_detected(&self, proposed_version: u32) -> bool {
        proposed_version < self.current_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_version() {
        let rp = RollbackProtection::new();
        assert_eq!(rp.current_version, 1);
        assert_eq!(rp.min_version, 1);
    }

    #[test]
    fn test_version_upgrade_allowed() {
        let rp = RollbackProtection::new();
        assert!(rp.check_version(2).unwrap());
        assert!(rp.check_version(10).unwrap());
    }

    #[test]
    fn test_rollback_rejected() {
        let rp = RollbackProtection::new();
        assert!(!rp.check_version(0).unwrap());
    }

    #[test]
    fn test_record_new_version() {
        let mut rp = RollbackProtection::new();
        rp.record_version(3).unwrap();
        assert_eq!(rp.current_version, 3);
    }

    #[test]
    fn test_is_rollback_detected() {
        let mut rp = RollbackProtection::new();
        rp.record_version(5).unwrap();
        assert!(rp.is_rollback_detected(3));
        assert!(!rp.is_rollback_detected(6));
    }
}
