//! 时序指标存储
//!
//! 基于 JSONL 文件的轻量级指标存储（Phase 2+ SQLite 迁移准备就绪）

use crate::collectors::SystemSnapshot;
use crate::errors::MonitorError;
use chrono::{DateTime, Utc};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// 时序指标存储
pub struct MetricsStore {
    db_path: String,
    retention_days: u32,
}

impl MetricsStore {
    pub fn new(db_path: &str, retention_days: u32) -> Self {
        Self {
            db_path: db_path.to_string(),
            retention_days,
        }
    }

    /// 初始化存储目录
    pub async fn init(&self) -> Result<(), MonitorError> {
        let dir = PathBuf::from(&self.db_path);
        fs::create_dir_all(&dir)
            .map_err(|e| MonitorError::StorageError(format!("创建指标存储目录失败: {}", e)))?;
        Ok(())
    }

    /// 存储系统快照
    pub async fn store(&self, snapshot: &SystemSnapshot) -> Result<(), MonitorError> {
        let file_path = self.today_file();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| MonitorError::StorageError(format!("打开指标文件失败: {}", e)))?;

        let json = serde_json::to_string(snapshot)
            .map_err(|e| MonitorError::StorageError(format!("序列化快照失败: {}", e)))?;

        writeln!(file, "{}", json)
            .map_err(|e| MonitorError::StorageError(format!("写入快照失败: {}", e)))?;

        Ok(())
    }

    /// 按时间范围查询
    pub async fn query_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<SystemSnapshot>, MonitorError> {
        let mut results = Vec::new();
        let dir = PathBuf::from(&self.db_path);

        if !dir.exists() {
            return Ok(results);
        }

        for entry in fs::read_dir(&dir)
            .map_err(|e| MonitorError::StorageError(format!("读取目录失败: {}", e)))?
        {
            let entry = entry
                .map_err(|e| MonitorError::StorageError(format!("读取目录条目失败: {}", e)))?;
            let path = entry.path();
            if !path.is_file() || !path.to_string_lossy().ends_with(".jsonl") {
                continue;
            }

            let file = File::open(&path)
                .map_err(|e| MonitorError::StorageError(format!("打开文件失败: {}", e)))?;
            let reader = BufReader::new(file);

            for line in reader.lines() {
                let line =
                    line.map_err(|e| MonitorError::StorageError(format!("读取行失败: {}", e)))?;
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(snapshot) = serde_json::from_str::<SystemSnapshot>(&line) {
                    if snapshot.timestamp >= start && snapshot.timestamp <= end {
                        results.push(snapshot);
                    }
                }
            }
        }

        results.sort_by_key(|s| s.timestamp);
        Ok(results)
    }

    /// 获取最新快照
    pub async fn get_latest(&self) -> Result<Option<SystemSnapshot>, MonitorError> {
        let all = self.query_range(DateTime::UNIX_EPOCH, Utc::now()).await?;
        Ok(all.into_iter().last())
    }

    /// 清理过期数据
    pub async fn purge_old_data(&self) -> Result<usize, MonitorError> {
        let cutoff = Utc::now() - chrono::Duration::days(self.retention_days as i64);
        let dir = PathBuf::from(&self.db_path);

        if !dir.exists() {
            return Ok(0);
        }

        let mut purged = 0usize;
        for entry in fs::read_dir(&dir)
            .map_err(|e| MonitorError::StorageError(format!("读取目录失败: {}", e)))?
        {
            let entry = entry
                .map_err(|e| MonitorError::StorageError(format!("读取目录条目失败: {}", e)))?;
            let path = entry.path();

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // 文件名格式: metrics_YYYY-MM-DD.jsonl
                if name.starts_with("metrics_") && name.ends_with(".jsonl") {
                    let date_str = &name[8..18]; // "YYYY-MM-DD"
                    if let Ok(file_date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                        let file_dt: DateTime<Utc> =
                            file_date.and_hms_opt(0, 0, 0).unwrap().and_utc();
                        if file_dt < cutoff {
                            fs::remove_file(&path).ok();
                            purged += 1;
                        }
                    }
                }
            }
        }

        tracing::info!(count = purged, "已清理过期指标文件");
        Ok(purged)
    }

    fn today_file(&self) -> PathBuf {
        let today = Utc::now().format("%Y-%m-%d");
        PathBuf::from(&self.db_path).join(format!("metrics_{}.jsonl", today))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collectors::*;

    fn make_snapshot() -> SystemSnapshot {
        SystemSnapshot {
            timestamp: Utc::now(),
            cpu: CpuMetrics {
                usage_percent: 50.0,
                per_core: vec![],
                load_avg_1m: 1.0,
                load_avg_5m: 1.0,
                load_avg_15m: 1.0,
                temperature_c: None,
            },
            memory: MemoryMetrics {
                total_mb: 8192,
                used_mb: 4096,
                available_mb: 4096,
                swap_total_mb: 0,
                swap_used_mb: 0,
                usage_percent: 50.0,
            },
            disk: DiskMetrics {
                total_mb: 65536,
                used_mb: 32768,
                available_mb: 32768,
                usage_percent: 50.0,
                read_iops: 0,
                write_iops: 0,
            },
            temperature: TemperatureMetrics {
                cpu_temp_c: 45.0,
                npu_temp_c: None,
                board_temp_c: 40.0,
                ambient_temp_c: None,
            },
            processes: vec![],
        }
    }

    #[tokio::test]
    async fn test_store_and_query() {
        let dir = tempfile::tempdir().unwrap();
        let store = MetricsStore::new(dir.path().to_str().unwrap(), 30);
        store.init().await.unwrap();

        let snapshot = make_snapshot();
        store.store(&snapshot).await.unwrap();

        let results = store
            .query_range(DateTime::UNIX_EPOCH, Utc::now())
            .await
            .unwrap();
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_get_latest() {
        let dir = tempfile::tempdir().unwrap();
        let store = MetricsStore::new(dir.path().to_str().unwrap(), 30);
        store.init().await.unwrap();

        store.store(&make_snapshot()).await.unwrap();
        let latest = store.get_latest().await.unwrap();
        assert!(latest.is_some());
    }

    #[tokio::test]
    async fn test_purge_old_data() {
        let dir = tempfile::tempdir().unwrap();
        let store = MetricsStore::new(dir.path().to_str().unwrap(), 365);
        store.init().await.unwrap();
        let purged = store.purge_old_data().await.unwrap();
        assert_eq!(purged, 0); // 新数据不应被清理
    }
}
