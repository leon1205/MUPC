//! 防回滚保护

use crate::errors::SecurityError;

/// 防回滚保护
#[derive(Debug, Clone)]
pub struct RollbackProtection {
    pub min_version: u32,
    pub current_version: u32,
}

impl RollbackProtection {
    pub fn new() -> Self {
        todo!("Phase 2+")
    }

    pub fn check_version(&self, version: u32) -> Result<bool, SecurityError> {
        todo!("Phase 2+")
    }

    pub fn record_version(&mut self, version: u32) -> Result<(), SecurityError> {
        todo!("Phase 2+")
    }

    pub fn is_rollback_detected(&self, proposed_version: u32) -> bool {
        todo!("Phase 2+")
    }
}
