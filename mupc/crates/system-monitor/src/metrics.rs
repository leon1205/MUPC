use crate::collectors::SystemSnapshot;
use crate::errors::MonitorError;
use chrono::{DateTime, Utc};

/// 时序指标存储（Phase 2+ 实现，基于 SQLite）
pub struct MetricsStore {
    db_path: String,
    retention_days: u32,
}

impl MetricsStore {
    pub fn new(db_path: &str, retention_days: u32) -> Self {
        todo!("Phase 2+")
    }

    pub async fn init(&self) -> Result<(), MonitorError> {
        todo!("Phase 2+")
    }

    pub async fn store(&self, snapshot: &SystemSnapshot) -> Result<(), MonitorError> {
        todo!("Phase 2+")
    }

    pub async fn query_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<SystemSnapshot>, MonitorError> {
        todo!("Phase 2+")
    }

    pub async fn get_latest(&self) -> Result<Option<SystemSnapshot>, MonitorError> {
        todo!("Phase 2+")
    }

    pub async fn purge_old_data(&self) -> Result<usize, MonitorError> {
        todo!("Phase 2+")
    }
}
