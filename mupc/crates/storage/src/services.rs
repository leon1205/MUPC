use crate::errors::StorageError;
use crate::models::*;
use crate::repository::*;
use chrono::Datelike;
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
        // ── v2.15 新增表（P1 补齐） ──
        // 设备铭牌参数表
        "CREATE TABLE IF NOT EXISTS device_nameplate (
            device_id TEXT PRIMARY KEY,
            rated_power REAL,
            rated_capacity REAL,
            rated_voltage REAL,
            rated_current REAL,
            max_charge_power REAL,
            max_discharge_power REAL,
            charge_efficiency REAL,
            discharge_efficiency REAL,
            soc_min REAL,
            soc_max REAL,
            rated_reactive_power REAL,
            protection_level TEXT,
            cooling_method TEXT,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        // 维护记录表
        "CREATE TABLE IF NOT EXISTS maintenance_record (
            record_id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            maintenance_date TEXT NOT NULL,
            maintenance_type TEXT NOT NULL,
            description TEXT NOT NULL,
            operator TEXT NOT NULL,
            result TEXT NOT NULL,
            next_maintenance_date TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        "CREATE INDEX IF NOT EXISTS idx_maintenance_device
         ON maintenance_record(device_id, maintenance_date DESC)",
        // 告警日志表
        "CREATE TABLE IF NOT EXISTS alarm_log (
            alarm_id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            alarm_type TEXT NOT NULL,
            severity TEXT NOT NULL,
            description TEXT NOT NULL,
            trigger_time INTEGER NOT NULL,
            acknowledge_time INTEGER,
            acknowledge_by TEXT,
            clear_time INTEGER,
            clear_by TEXT,
            status TEXT NOT NULL DEFAULT 'active'
        )",
        "CREATE INDEX IF NOT EXISTS idx_alarm_device_time
         ON alarm_log(device_id, trigger_time DESC)",
        "CREATE INDEX IF NOT EXISTS idx_alarm_status
         ON alarm_log(status)",
        // 事件记录表（与 events 表互补：event_log 侧重运维审计，events 侧重系统事件）
        "CREATE TABLE IF NOT EXISTS event_log (
            event_id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_type TEXT NOT NULL,
            event_time INTEGER NOT NULL,
            source TEXT NOT NULL,
            operator TEXT,
            description TEXT NOT NULL,
            detail TEXT NOT NULL DEFAULT '{}'
        )",
        "CREATE INDEX IF NOT EXISTS idx_event_log_type_time
         ON event_log(event_type, event_time DESC)",
        // 存储配置表（键值对）
        "CREATE TABLE IF NOT EXISTS storage_config (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    ];

    for stmt in &statements {
        sqlx::query(stmt)
            .execute(pool)
            .await
            .map_err(|e| StorageError::MigrationError(e.to_string()))?;
    }
    Ok(())
}

/// 创建指定月份的遥测分区表
///
/// 分区策略：按月分区 `telemetry_YYYYmm`，清理时直接 DROP TABLE。
/// SQLite 不支持原生分区，此函数创建独立物理表。
///
/// # 示例
///
/// ```ignore
/// create_telemetry_partition(&pool, "202606").await?;
/// ```
pub async fn create_telemetry_partition(
    pool: &SqlitePool,
    month: &str,
) -> Result<(), StorageError> {
    let table = format!("telemetry_{}", month);
    let create_sql = format!(
        "CREATE TABLE IF NOT EXISTS {} (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            phase_a_voltage REAL, phase_b_voltage REAL, phase_c_voltage REAL,
            phase_a_current REAL, phase_b_current REAL, phase_c_current REAL,
            total_active_power REAL, total_reactive_power REAL, total_apparent_power REAL,
            power_factor REAL, frequency REAL,
            phase_a_power REAL, phase_b_power REAL, phase_c_power REAL,
            total_import_energy REAL, total_export_energy REAL,
            quality TEXT NOT NULL DEFAULT 'good'
        )",
        table
    );
    sqlx::query(&create_sql)
        .execute(pool)
        .await
        .map_err(|e| StorageError::MigrationError(e.to_string()))?;

    let index_sql = format!(
        "CREATE INDEX IF NOT EXISTS idx_{}_dev_time ON {}(device_id, timestamp DESC)",
        table, table
    );
    sqlx::query(&index_sql)
        .execute(pool)
        .await
        .map_err(|e| StorageError::MigrationError(e.to_string()))?;

    tracing::info!(table, "遥测月度分区表已创建");
    Ok(())
}

/// 删除超出保留期的遥测分区表（基于表名前缀匹配）
///
/// 仅处理 `telemetry_` 前缀且匹配 `YYYYmm` 格式的表。
pub async fn cleanup_old_telemetry_partitions(
    pool: &SqlitePool,
    retention_months: u32,
) -> Result<usize, StorageError> {
    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::days(retention_months as i64 * 30);
    let cutoff_ym = format!("{:04}{:02}", cutoff.year(), cutoff.month());
    let prefix = "telemetry_";

    // 查询所有 telemetry_ 前缀的表
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type='table' AND name LIKE ?",
    )
    .bind(format!("{}%", prefix))
    .fetch_all(pool)
    .await
    .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

    let mut dropped = 0usize;
    for name in rows {
        // 提取 YYYYmm 部分
        let suffix = &name[prefix.len()..];
        if suffix.len() == 6 && suffix < cutoff_ym.as_str() {
            let sql = format!("DROP TABLE IF EXISTS {}", name);
            sqlx::query(&sql)
                .execute(pool)
                .await
                .map_err(|e| StorageError::MigrationError(e.to_string()))?;
            dropped += 1;
            tracing::info!(table = name, "已清理过期遥测分区");
        }
    }
    Ok(dropped)
}
