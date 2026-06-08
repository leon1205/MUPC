//! 启动审计日志

use crate::errors::SecurityError;
use chrono::{DateTime, Utc};

/// 启动审计条目
#[derive(Debug, Clone)]
pub struct BootAuditLog {
    pub timestamp: DateTime<Utc>,
    pub event: String,
    pub result: bool,
    pub details: String,
}

/// 启动审计日志记录器
#[derive(Debug, Clone)]
pub struct BootAuditLogger {
    logs: Vec<BootAuditLog>,
}

impl Default for BootAuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl BootAuditLogger {
    pub fn new() -> Self {
        Self { logs: Vec::new() }
    }

    pub fn record(
        &mut self,
        event: &str,
        result: bool,
        details: &str,
    ) -> Result<(), SecurityError> {
        self.logs.push(BootAuditLog {
            timestamp: Utc::now(),
            event: event.to_string(),
            result,
            details: details.to_string(),
        });
        Ok(())
    }

    pub fn get_logs(&self) -> &[BootAuditLog] {
        &self.logs
    }

    pub fn clear(&mut self) {
        self.logs.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_audit_record() {
        let mut logger = BootAuditLogger::new();
        logger.record("boot_verify", true, "链验证通过").unwrap();
        assert_eq!(logger.get_logs().len(), 1);

        logger.record("image_check", false, "镜像校验失败").unwrap();
        assert_eq!(logger.get_logs().len(), 2);
    }

    #[test]
    fn test_boot_audit_clear() {
        let mut logger = BootAuditLogger::new();
        logger.record("test", true, "").unwrap();
        logger.clear();
        assert!(logger.get_logs().is_empty());
    }
}
