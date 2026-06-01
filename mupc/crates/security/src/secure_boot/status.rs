//! 启动状态跟踪

use chrono::{DateTime, Utc};

/// 启动状态
#[derive(Debug, Clone)]
pub struct BootStatus {
    pub last_boot: Option<DateTime<Utc>>,
    pub verified: bool,
    pub chain_status: String,
    pub boot_count: u64,
}

impl Default for BootStatus {
    fn default() -> Self {
        Self::new()
    }
}

impl BootStatus {
    pub fn new() -> Self {
        Self {
            last_boot: None,
            verified: false,
            chain_status: "not_verified".into(),
            boot_count: 0,
        }
    }

    /// 记录一次启动事件
    pub fn record_boot(&mut self) {
        self.last_boot = Some(Utc::now());
        self.boot_count += 1;
    }

    /// 检查启动链是否已验证通过
    pub fn is_verified(&self) -> bool {
        self.verified
    }

    /// 获取距上次启动的运行时间（秒）
    pub fn get_uptime_secs(&self) -> Option<u64> {
        self.last_boot.map(|boot_time| {
            let elapsed = Utc::now() - boot_time;
            elapsed.num_seconds() as u64
        })
    }

    /// 标记启动链验证通过
    pub fn mark_verified(&mut self) {
        self.verified = true;
        self.chain_status = "verified".into();
    }

    /// 标记启动链验证失败
    pub fn mark_failed(&mut self, reason: &str) {
        self.verified = false;
        self.chain_status = format!("failed: {}", reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let status = BootStatus::new();
        assert!(!status.is_verified());
        assert_eq!(status.boot_count, 0);
        assert_eq!(status.chain_status, "not_verified");
    }

    #[test]
    fn test_record_boot() {
        let mut status = BootStatus::new();
        status.record_boot();
        assert_eq!(status.boot_count, 1);
        assert!(status.last_boot.is_some());
    }

    #[test]
    fn test_mark_verified() {
        let mut status = BootStatus::new();
        status.mark_verified();
        assert!(status.is_verified());
        assert_eq!(status.chain_status, "verified");
    }

    #[test]
    fn test_mark_failed() {
        let mut status = BootStatus::new();
        status.mark_failed("签名校验不通过");
        assert!(!status.is_verified());
        assert!(status.chain_status.contains("failed"));
    }
}
