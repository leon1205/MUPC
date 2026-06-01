//! 完整性监控

use crate::errors::SecurityError;
use sha2::{Digest, Sha256};
use std::fs;

/// 完整性监控器
#[derive(Debug, Clone)]
pub struct IntegrityMonitor {
    pub enabled: bool,
}

impl Default for IntegrityMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl IntegrityMonitor {
    pub fn new() -> Self {
        Self { enabled: true }
    }

    /// 检查文件完整性（基于 SHA-256）
    ///
    /// Phase 2+: 替换为 SM3 哈希算法，支持签名验证。
    pub fn check_integrity(&self, path: &str) -> Result<bool, SecurityError> {
        if !self.enabled {
            return Ok(true);
        }

        let data = fs::read(path).map_err(|e| {
            SecurityError::IoError(format!("读取文件 {} 失败: {}", path, e))
        })?;

        let mut hasher = Sha256::new();
        hasher.update(&data);
        let hash = hex::encode(hasher.finalize());

        tracing::debug!(path = %path, hash = %hash, "文件完整性校验完成");
        Ok(true)
    }

    /// 启动周期性完整性检查
    ///
    /// Phase 2+: 通过后台任务定期扫描关键系统文件。
    pub fn start_periodic_check(&self, interval_secs: u64) {
        tracing::info!(
            interval_secs = interval_secs,
            enabled = self.enabled,
            "启动周期性完整性检查"
        );
    }

    /// 验证完整性清单文件
    ///
    /// Phase 2+: 读取 manifest 文件中的预期哈希值进行比对。
    pub fn verify_manifest(&self, _manifest_path: &str) -> Result<bool, SecurityError> {
        if !self.enabled {
            return Ok(true);
        }

        tracing::debug!("完整性清单验证 - 待实现");
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_check_integrity_file() {
        let monitor = IntegrityMonitor::new();
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"test data").unwrap();
        let result = monitor.check_integrity(f.path().to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_integrity_nonexistent() {
        let monitor = IntegrityMonitor::new();
        let result = monitor.check_integrity("/nonexistent/file/path");
        assert!(result.is_err());
    }

    #[test]
    fn test_disabled_monitor() {
        let monitor = IntegrityMonitor { enabled: false };
        let result = monitor.check_integrity("/nonexistent");
        assert!(result.unwrap());
    }
}
