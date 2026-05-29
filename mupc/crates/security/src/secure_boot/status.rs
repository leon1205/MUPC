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

impl BootStatus {
    pub fn new() -> Self {
        todo!("Phase 2+")
    }

    pub fn record_boot(&mut self) {
        todo!("Phase 2+")
    }

    pub fn is_verified(&self) -> bool {
        todo!("Phase 2+")
    }

    pub fn get_uptime_secs(&self) -> Option<u64> {
        todo!("Phase 2+")
    }
}
