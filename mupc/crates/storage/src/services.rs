use crate::errors::StorageError;
use crate::models::*;
use crate::repository::*;
use parking_lot::Mutex;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

/// 存储服务 — 统一入口
pub struct StorageService {
    pub telemetry: Arc<dyn TelemetryRepository>,
    pub faults: Arc<dyn FaultRepository>,
    pub decisions: Arc<dyn DecisionRepository>,
    pub events: Arc<dyn EventRepository>,
    pub assets: Arc<dyn AssetRepository>,
    pool: Arc<SqlitePool>,
}

impl StorageService {
    /// 用共享连接池创建所有 Repository
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self {
            telemetry: Arc::new(SqliteTelemetryRepo::new(pool.clone())),
            faults: Arc::new(SqliteFaultRepo::new(pool.clone())),
            decisions: Arc::new(SqliteDecisionRepo::new(pool.clone())),
            events: Arc::new(SqliteEventRepo::new(pool.clone())),
            assets: Arc::new(SqliteAssetRepo::new(pool.clone())),
            pool,
        }
    }

    pub async fn health_check(&self) -> Result<bool, StorageError> {
        sqlx::query("SELECT 1")
            .execute(self.pool.as_ref())
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(true)
    }

    /// 获取底层数据库连接池（供外部模块使用）
    pub fn pool(&self) -> &Arc<SqlitePool> {
        &self.pool
    }

    /// 更新动作空间配置（upsert 语义）
    ///
    /// 若 transformer_id 已存在则更新，若不存在则插入。
    pub async fn update_action_space_config(
        &self,
        transformer_id: &str,
        max_batt_charge_power: f64,
        max_batt_discharge_power: f64,
        max_load_shedding: f64,
        max_apparent_power_kva: f64,
        p_batt_ramp_limit_kw: f64,
        q_batt_ramp_limit_kvar: f64,
        pv_limit_min: f64,
    ) -> Result<(), StorageError> {
        // 先尝试更新
        let affected = sqlx::query(
            "UPDATE action_space_config SET
                max_batt_charge_power = ?,
                max_batt_discharge_power = ?,
                max_load_shedding = ?,
                max_apparent_power_kva = ?,
                p_batt_ramp_limit_kw = ?,
                q_batt_ramp_limit_kvar = ?,
                pv_limit_min = ?,
                updated_at = CURRENT_TIMESTAMP
             WHERE transformer_id = ?",
        )
        .bind(max_batt_charge_power)
        .bind(max_batt_discharge_power)
        .bind(max_load_shedding)
        .bind(max_apparent_power_kva)
        .bind(p_batt_ramp_limit_kw)
        .bind(q_batt_ramp_limit_kvar)
        .bind(pv_limit_min)
        .bind(transformer_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        // 若未更新到任何行，则插入
        if affected.rows_affected() == 0 {
            sqlx::query(
                "INSERT INTO action_space_config
                    (transformer_id, max_batt_charge_power, max_batt_discharge_power,
                     max_load_shedding, max_apparent_power_kva, p_batt_ramp_limit_kw,
                     q_batt_ramp_limit_kvar, pv_limit_min)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(transformer_id)
            .bind(max_batt_charge_power)
            .bind(max_batt_discharge_power)
            .bind(max_load_shedding)
            .bind(max_apparent_power_kva)
            .bind(p_batt_ramp_limit_kw)
            .bind(q_batt_ramp_limit_kvar)
            .bind(pv_limit_min)
            .execute(self.pool.as_ref())
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        }

        Ok(())
    }

    /// 更新动作空间配置（完整字段，upsert 语义）
    ///
    /// v2.6 扩展：新增 transformer_kva, battery_capacity_kwh,
    /// soc_min, soc_max, overload_threshold 字段。
    pub async fn update_action_space_config_full(
        &self,
        transformer_id: &str,
        max_batt_charge_power: f64,
        max_batt_discharge_power: f64,
        max_load_shedding: f64,
        max_apparent_power_kva: f64,
        p_batt_ramp_limit_kw: f64,
        q_batt_ramp_limit_kvar: f64,
        pv_limit_min: f64,
        transformer_kva: f64,
        battery_capacity_kwh: f64,
        soc_min: f64,
        soc_max: f64,
        overload_threshold: f64,
    ) -> Result<(), StorageError> {
        // 先尝试更新
        let affected = sqlx::query(
            "UPDATE action_space_config SET
                max_batt_charge_power = ?,
                max_batt_discharge_power = ?,
                max_load_shedding = ?,
                max_apparent_power_kva = ?,
                p_batt_ramp_limit_kw = ?,
                q_batt_ramp_limit_kvar = ?,
                pv_limit_min = ?,
                transformer_kva = ?,
                battery_capacity_kwh = ?,
                soc_min = ?,
                soc_max = ?,
                overload_threshold = ?,
                updated_at = CURRENT_TIMESTAMP
             WHERE transformer_id = ?",
        )
        .bind(max_batt_charge_power)
        .bind(max_batt_discharge_power)
        .bind(max_load_shedding)
        .bind(max_apparent_power_kva)
        .bind(p_batt_ramp_limit_kw)
        .bind(q_batt_ramp_limit_kvar)
        .bind(pv_limit_min)
        .bind(transformer_kva)
        .bind(battery_capacity_kwh)
        .bind(soc_min)
        .bind(soc_max)
        .bind(overload_threshold)
        .bind(transformer_id)
        .execute(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        // 若未更新到任何行，则插入
        if affected.rows_affected() == 0 {
            sqlx::query(
                "INSERT INTO action_space_config
                    (transformer_id, max_batt_charge_power, max_batt_discharge_power,
                     max_load_shedding, max_apparent_power_kva, p_batt_ramp_limit_kw,
                     q_batt_ramp_limit_kvar, pv_limit_min,
                     transformer_kva, battery_capacity_kwh, soc_min, soc_max, overload_threshold)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(transformer_id)
            .bind(max_batt_charge_power)
            .bind(max_batt_discharge_power)
            .bind(max_load_shedding)
            .bind(max_apparent_power_kva)
            .bind(p_batt_ramp_limit_kw)
            .bind(q_batt_ramp_limit_kvar)
            .bind(pv_limit_min)
            .bind(transformer_kva)
            .bind(battery_capacity_kwh)
            .bind(soc_min)
            .bind(soc_max)
            .bind(overload_threshold)
            .execute(self.pool.as_ref())
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        }

        Ok(())
    }
}

/// 写入缓冲管理器 — 双缓冲批量写入，事务保证原子性
pub struct WriteBuffer {
    capacity: usize,
    flush_interval_ms: u64,
    buffer: Mutex<Vec<TelemetryPoint>>,
    pool: Arc<SqlitePool>,
}

impl WriteBuffer {
    pub fn new(capacity: usize, flush_interval_ms: u64, pool: Arc<SqlitePool>) -> Self {
        Self {
            capacity,
            flush_interval_ms,
            buffer: Mutex::new(Vec::with_capacity(capacity)),
            pool,
        }
    }

    pub async fn buffer_telemetry(&self, point: TelemetryPoint) -> Result<(), StorageError> {
        let maybe_batch = {
            let mut buf = self.buffer.lock();
            buf.push(point);
            if buf.len() >= self.capacity {
                let batch: Vec<TelemetryPoint> = buf.drain(..).collect();
                buf.reserve(self.capacity);
                Some(batch)
            } else {
                None
            }
        };
        if let Some(batch) = maybe_batch {
            self.flush_batch(batch).await?;
        }
        Ok(())
    }

    /// 调用方负责定时调用。使用事务保证批量写入原子性。
    pub async fn flush(&self) -> Result<usize, StorageError> {
        let batch: Vec<TelemetryPoint> = {
            let mut buf = self.buffer.lock();
            let drained = buf.drain(..).collect();
            buf.reserve(self.capacity);
            drained
        };
        self.flush_batch(batch).await
    }

    async fn flush_batch(&self, batch: Vec<TelemetryPoint>) -> Result<usize, StorageError> {
        let count = batch.len();
        if count == 0 {
            return Ok(0);
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        for point in &batch {
            sqlx::query(
                "INSERT INTO telemetry (device_id, timestamp, metric_name, value, quality)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&point.device_id)
            .bind(point.timestamp.timestamp_millis())
            .bind(&point.metric_name)
            .bind(point.value)
            .bind(point.quality)
            .execute(&mut *tx)
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(count)
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn flush_interval_ms(&self) -> u64 {
        self.flush_interval_ms
    }
}

/// 数据保留策略管理器
pub struct RetentionManager {
    telemetry_retention_days: u32,
    event_retention_days: u32,
}

impl RetentionManager {
    pub fn new(telemetry_days: u32, event_days: u32) -> Self {
        Self {
            telemetry_retention_days: telemetry_days,
            event_retention_days: event_days,
        }
    }

    pub async fn enforce(&self, service: &StorageService) -> Result<RetentionReport, StorageError> {
        let now = chrono::Utc::now();
        let telemetry_before = now - chrono::Duration::days(self.telemetry_retention_days as i64);
        let event_before = now - chrono::Duration::days(self.event_retention_days as i64);

        let telemetry_deleted = service
            .telemetry
            .delete_older_than(telemetry_before)
            .await?;
        let events_deleted = service.events.purge_older_than(event_before).await?;

        Ok(RetentionReport {
            telemetry_deleted,
            events_deleted,
        })
    }

    pub fn telemetry_retention_days(&self) -> u32 {
        self.telemetry_retention_days
    }

    pub fn event_retention_days(&self) -> u32 {
        self.event_retention_days
    }
}

pub struct RetentionReport {
    pub telemetry_deleted: usize,
    pub events_deleted: usize,
}

/// 数据库迁移：建表与索引
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), StorageError> {
    let statements = [
        // 遥测表 — 按月分区建议用外部脚本，这里建基础表
        "CREATE TABLE IF NOT EXISTS telemetry (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            metric_name TEXT NOT NULL,
            value REAL NOT NULL,
            quality INTEGER NOT NULL DEFAULT 0
        )",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_device_ts
         ON telemetry(device_id, timestamp)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_metric_ts
         ON telemetry(metric_name, timestamp)",
        // 故障表
        "CREATE TABLE IF NOT EXISTS faults (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            fault_type TEXT NOT NULL,
            severity INTEGER NOT NULL,
            waveform_path TEXT,
            acknowledged INTEGER NOT NULL DEFAULT 0
        )",
        "CREATE INDEX IF NOT EXISTS idx_faults_ts ON faults(timestamp)",
        // 决策表
        "CREATE TABLE IF NOT EXISTS decisions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            scene_type TEXT NOT NULL,
            action_json TEXT NOT NULL,
            confidence REAL NOT NULL,
            model_version TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_decisions_scene_ts
         ON decisions(scene_type, timestamp)",
        // 事件表
        "CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            source TEXT NOT NULL,
            message TEXT NOT NULL
        )",
        "CREATE INDEX IF NOT EXISTS idx_events_type_ts
         ON events(event_type, timestamp)",
        // 资产表
        "CREATE TABLE IF NOT EXISTS assets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL UNIQUE,
            device_type TEXT NOT NULL,
            manufacturer TEXT NOT NULL DEFAULT '',
            model TEXT NOT NULL DEFAULT '',
            firmware_version TEXT NOT NULL DEFAULT '',
            installed_at INTEGER NOT NULL,
            last_maintenance INTEGER
        )",
        "CREATE INDEX IF NOT EXISTS idx_assets_type ON assets(device_type)",
        // v2.5 动作空间配置表
        "CREATE TABLE IF NOT EXISTS action_space_config (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            transformer_id TEXT NOT NULL UNIQUE,
            max_batt_charge_power REAL NOT NULL,
            max_batt_discharge_power REAL NOT NULL,
            max_load_shedding REAL NOT NULL,
            max_apparent_power_kva REAL NOT NULL DEFAULT 200.0,
            p_batt_ramp_limit_kw REAL NOT NULL DEFAULT 50.0,
            q_batt_ramp_limit_kvar REAL NOT NULL DEFAULT 30.0,
            pv_limit_min REAL NOT NULL DEFAULT 0.1,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT DEFAULT CURRENT_TIMESTAMP
        )",
        // v2.6 扩展字段（ALTER TABLE 兼容已有数据库）
        "ALTER TABLE action_space_config ADD COLUMN transformer_kva REAL NOT NULL DEFAULT 0.0",
        "ALTER TABLE action_space_config ADD COLUMN battery_capacity_kwh REAL NOT NULL DEFAULT 0.0",
        "ALTER TABLE action_space_config ADD COLUMN soc_min REAL NOT NULL DEFAULT 0.0",
        "ALTER TABLE action_space_config ADD COLUMN soc_max REAL NOT NULL DEFAULT 1.0",
        "ALTER TABLE action_space_config ADD COLUMN overload_threshold REAL NOT NULL DEFAULT 1.2",
    ];

    for stmt in &statements {
        sqlx::query(stmt)
            .execute(pool)
            .await
            .map_err(|e| StorageError::MigrationError(e.to_string()))?;
    }
    Ok(())
}
