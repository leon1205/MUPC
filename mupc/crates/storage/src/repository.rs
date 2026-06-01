use crate::errors::StorageError;
use crate::models::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::sync::Arc;

/// 将毫秒时间戳转为 DateTime<Utc>，异常值时记录 ERROR 日志
fn ts_to_datetime(raw: i64, context: &str) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(raw).unwrap_or_else(|| {
        tracing::error!(timestamp = raw, context, "数据库中的无效时间戳，使用 epoch 兜底");
        DateTime::default()
    })
}

#[async_trait]
pub trait TelemetryRepository: Send + Sync {
    async fn insert(&self, point: &TelemetryPoint) -> Result<i64, StorageError>;
    async fn query_range(
        &self,
        device_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<TelemetryPoint>, StorageError>;
    async fn get_latest(
        &self,
        device_id: &str,
        metric: &str,
    ) -> Result<Option<TelemetryPoint>, StorageError>;
    async fn delete_older_than(&self, before: DateTime<Utc>) -> Result<usize, StorageError>;
}

#[async_trait]
pub trait FaultRepository: Send + Sync {
    async fn insert(&self, event: &FaultEvent) -> Result<i64, StorageError>;
    async fn query_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<FaultEvent>, StorageError>;
    async fn acknowledge(&self, id: i64) -> Result<(), StorageError>;
}

#[async_trait]
pub trait DecisionRepository: Send + Sync {
    async fn insert(&self, record: &AiDecisionRecord) -> Result<i64, StorageError>;
    async fn query_recent(&self, limit: usize) -> Result<Vec<AiDecisionRecord>, StorageError>;
    async fn get_by_scene(
        &self,
        scene_type: &str,
        limit: usize,
    ) -> Result<Vec<AiDecisionRecord>, StorageError>;
}

#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn insert(&self, event: &SystemEvent) -> Result<i64, StorageError>;
    async fn query_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<SystemEvent>, StorageError>;
    async fn purge_older_than(&self, before: DateTime<Utc>) -> Result<usize, StorageError>;
}

#[async_trait]
pub trait AssetRepository: Send + Sync {
    async fn upsert(&self, asset: &AssetRecord) -> Result<i64, StorageError>;
    async fn get_by_device_id(&self, device_id: &str) -> Result<Option<AssetRecord>, StorageError>;
    async fn list_all(&self) -> Result<Vec<AssetRecord>, StorageError>;
    async fn list_by_type(&self, device_type: &str) -> Result<Vec<AssetRecord>, StorageError>;
}

/// 初始化 SQLite 连接池 (WAL 模式)
pub async fn init_pool(db_path: &str) -> Result<SqlitePool, StorageError> {
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect(db_path)
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

    sqlx::query("PRAGMA journal_mode = WAL")
        .execute(&pool)
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

    sqlx::query("PRAGMA synchronous = NORMAL")
        .execute(&pool)
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

    sqlx::query("PRAGMA busy_timeout = 5000")
        .execute(&pool)
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

    Ok(pool)
}

// ── SQLite 具体实现 ──

pub struct SqliteTelemetryRepo {
    pool: Arc<SqlitePool>,
}

impl SqliteTelemetryRepo {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TelemetryRepository for SqliteTelemetryRepo {
    async fn insert(&self, point: &TelemetryPoint) -> Result<i64, StorageError> {
        let row = sqlx::query(
            "INSERT INTO telemetry (device_id, timestamp, metric_name, value, quality)
             VALUES (?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(&point.device_id)
        .bind(point.timestamp.timestamp_millis())
        .bind(&point.metric_name)
        .bind(point.value)
        .bind(point.quality)
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(row.get(0))
    }

    async fn query_range(
        &self,
        device_id: &str,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<TelemetryPoint>, StorageError> {
        let rows = sqlx::query_as::<_, TelemetryRow>(
            "SELECT id, device_id, timestamp, metric_name, value, quality
             FROM telemetry
             WHERE device_id = ? AND timestamp >= ? AND timestamp <= ?
             ORDER BY timestamp DESC
             LIMIT 10000",
        )
        .bind(device_id)
        .bind(start.timestamp_millis())
        .bind(end.timestamp_millis())
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn get_latest(
        &self,
        device_id: &str,
        metric: &str,
    ) -> Result<Option<TelemetryPoint>, StorageError> {
        let row = sqlx::query_as::<_, TelemetryRow>(
            "SELECT id, device_id, timestamp, metric_name, value, quality
             FROM telemetry
             WHERE device_id = ? AND metric_name = ?
             ORDER BY timestamp DESC
             LIMIT 1",
        )
        .bind(device_id)
        .bind(metric)
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(row.map(|r| r.into()))
    }

    async fn delete_older_than(&self, before: DateTime<Utc>) -> Result<usize, StorageError> {
        let result = sqlx::query("DELETE FROM telemetry WHERE timestamp < ?")
            .bind(before.timestamp_millis())
            .execute(self.pool.as_ref())
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(result.rows_affected() as usize)
    }
}

pub struct SqliteFaultRepo {
    pool: Arc<SqlitePool>,
}

impl SqliteFaultRepo {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FaultRepository for SqliteFaultRepo {
    async fn insert(&self, event: &FaultEvent) -> Result<i64, StorageError> {
        let row = sqlx::query(
            "INSERT INTO faults (device_id, timestamp, fault_type, severity, waveform_path, acknowledged)
             VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(&event.device_id)
        .bind(event.timestamp.timestamp_millis())
        .bind(&event.fault_type)
        .bind(event.severity)
        .bind(&event.waveform_path)
        .bind(event.acknowledged)
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(row.get(0))
    }

    async fn query_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<FaultEvent>, StorageError> {
        let rows = sqlx::query_as::<_, FaultRow>(
            "SELECT id, device_id, timestamp, fault_type, severity, waveform_path, acknowledged
             FROM faults
             WHERE timestamp >= ? AND timestamp <= ?
             ORDER BY timestamp DESC
             LIMIT 5000",
        )
        .bind(start.timestamp_millis())
        .bind(end.timestamp_millis())
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn acknowledge(&self, id: i64) -> Result<(), StorageError> {
        let result = sqlx::query("UPDATE faults SET acknowledged = 1 WHERE id = ?")
            .bind(id)
            .execute(self.pool.as_ref())
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        if result.rows_affected() == 0 {
            return Err(StorageError::NotFound(format!("fault id={}", id)));
        }
        Ok(())
    }
}

pub struct SqliteDecisionRepo {
    pool: Arc<SqlitePool>,
}

impl SqliteDecisionRepo {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl DecisionRepository for SqliteDecisionRepo {
    async fn insert(&self, record: &AiDecisionRecord) -> Result<i64, StorageError> {
        let row = sqlx::query(
            "INSERT INTO decisions (timestamp, scene_type, action_json, confidence, model_version)
             VALUES (?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(record.timestamp.timestamp_millis())
        .bind(&record.scene_type)
        .bind(&record.action_json)
        .bind(record.confidence)
        .bind(&record.model_version)
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(row.get(0))
    }

    async fn query_recent(&self, limit: usize) -> Result<Vec<AiDecisionRecord>, StorageError> {
        let rows = sqlx::query_as::<_, DecisionRow>(
            "SELECT id, timestamp, scene_type, action_json, confidence, model_version
             FROM decisions
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn get_by_scene(
        &self,
        scene_type: &str,
        limit: usize,
    ) -> Result<Vec<AiDecisionRecord>, StorageError> {
        let rows = sqlx::query_as::<_, DecisionRow>(
            "SELECT id, timestamp, scene_type, action_json, confidence, model_version
             FROM decisions
             WHERE scene_type = ?
             ORDER BY timestamp DESC
             LIMIT ?",
        )
        .bind(scene_type)
        .bind(limit as i64)
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }
}

pub struct SqliteEventRepo {
    pool: Arc<SqlitePool>,
}

impl SqliteEventRepo {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl EventRepository for SqliteEventRepo {
    async fn insert(&self, event: &SystemEvent) -> Result<i64, StorageError> {
        let row = sqlx::query(
            "INSERT INTO events (timestamp, event_type, source, message)
             VALUES (?, ?, ?, ?) RETURNING id",
        )
        .bind(event.timestamp.timestamp_millis())
        .bind(&event.event_type)
        .bind(&event.source)
        .bind(&event.message)
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(row.get(0))
    }

    async fn query_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<SystemEvent>, StorageError> {
        let rows = sqlx::query_as::<_, EventRow>(
            "SELECT id, timestamp, event_type, source, message
             FROM events
             WHERE timestamp >= ? AND timestamp <= ?
             ORDER BY timestamp DESC
             LIMIT 10000",
        )
        .bind(start.timestamp_millis())
        .bind(end.timestamp_millis())
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn purge_older_than(&self, before: DateTime<Utc>) -> Result<usize, StorageError> {
        let result = sqlx::query("DELETE FROM events WHERE timestamp < ?")
            .bind(before.timestamp_millis())
            .execute(self.pool.as_ref())
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(result.rows_affected() as usize)
    }
}

pub struct SqliteAssetRepo {
    pool: Arc<SqlitePool>,
}

impl SqliteAssetRepo {
    pub fn new(pool: Arc<SqlitePool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AssetRepository for SqliteAssetRepo {
    async fn upsert(&self, asset: &AssetRecord) -> Result<i64, StorageError> {
        let row = sqlx::query(
            "INSERT INTO assets (device_id, device_type, manufacturer, model, firmware_version, installed_at, last_maintenance)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(device_id) DO UPDATE SET
               device_type = excluded.device_type,
               manufacturer = excluded.manufacturer,
               model = excluded.model,
               firmware_version = excluded.firmware_version,
               last_maintenance = excluded.last_maintenance
             RETURNING id",
        )
        .bind(&asset.device_id)
        .bind(&asset.device_type)
        .bind(&asset.manufacturer)
        .bind(&asset.model)
        .bind(&asset.firmware_version)
        .bind(asset.installed_at.timestamp_millis())
        .bind(asset.last_maintenance.map(|t| t.timestamp_millis()))
        .fetch_one(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(row.get(0))
    }

    async fn get_by_device_id(&self, device_id: &str) -> Result<Option<AssetRecord>, StorageError> {
        let row = sqlx::query_as::<_, AssetRow>(
            "SELECT id, device_id, device_type, manufacturer, model, firmware_version, installed_at, last_maintenance
             FROM assets WHERE device_id = ?",
        )
        .bind(device_id)
        .fetch_optional(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(row.map(|r| r.into()))
    }

    async fn list_all(&self) -> Result<Vec<AssetRecord>, StorageError> {
        let rows = sqlx::query_as::<_, AssetRow>(
            "SELECT id, device_id, device_type, manufacturer, model, firmware_version, installed_at, last_maintenance
             FROM assets ORDER BY device_type, device_id",
        )
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    async fn list_by_type(&self, device_type: &str) -> Result<Vec<AssetRecord>, StorageError> {
        let rows = sqlx::query_as::<_, AssetRow>(
            "SELECT id, device_id, device_type, manufacturer, model, firmware_version, installed_at, last_maintenance
             FROM assets WHERE device_type = ? ORDER BY device_id",
        )
        .bind(device_type)
        .fetch_all(self.pool.as_ref())
        .await
        .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.into()).collect())
    }
}

// ── sqlx 行映射 ──

#[derive(sqlx::FromRow)]
struct TelemetryRow {
    id: i64,
    device_id: String,
    timestamp: i64,
    metric_name: String,
    value: f64,
    quality: i32,
}

impl From<TelemetryRow> for TelemetryPoint {
    fn from(r: TelemetryRow) -> Self {
        Self {
            id: Some(r.id),
            device_id: r.device_id,
            timestamp: ts_to_datetime(r.timestamp, "telemetry"),
            metric_name: r.metric_name,
            value: r.value,
            quality: r.quality,
        }
    }
}

#[derive(sqlx::FromRow)]
struct FaultRow {
    id: i64,
    device_id: String,
    timestamp: i64,
    fault_type: String,
    severity: i32,
    waveform_path: Option<String>,
    acknowledged: bool,
}

impl From<FaultRow> for FaultEvent {
    fn from(r: FaultRow) -> Self {
        Self {
            id: Some(r.id),
            device_id: r.device_id,
            timestamp: ts_to_datetime(r.timestamp, "fault"),
            fault_type: r.fault_type,
            severity: r.severity,
            waveform_path: r.waveform_path,
            acknowledged: r.acknowledged,
        }
    }
}

#[derive(sqlx::FromRow)]
struct DecisionRow {
    id: i64,
    timestamp: i64,
    scene_type: String,
    action_json: String,
    confidence: f64,
    model_version: String,
}

impl From<DecisionRow> for AiDecisionRecord {
    fn from(r: DecisionRow) -> Self {
        Self {
            id: Some(r.id),
            timestamp: ts_to_datetime(r.timestamp, "decision"),
            scene_type: r.scene_type,
            action_json: r.action_json,
            confidence: r.confidence,
            model_version: r.model_version,
        }
    }
}

#[derive(sqlx::FromRow)]
struct EventRow {
    id: i64,
    timestamp: i64,
    event_type: String,
    source: String,
    message: String,
}

impl From<EventRow> for SystemEvent {
    fn from(r: EventRow) -> Self {
        Self {
            id: Some(r.id),
            timestamp: ts_to_datetime(r.timestamp, "event"),
            event_type: r.event_type,
            source: r.source,
            message: r.message,
        }
    }
}

#[derive(sqlx::FromRow)]
struct AssetRow {
    id: i64,
    device_id: String,
    device_type: String,
    manufacturer: String,
    model: String,
    firmware_version: String,
    installed_at: i64,
    last_maintenance: Option<i64>,
}

impl From<AssetRow> for AssetRecord {
    fn from(r: AssetRow) -> Self {
        Self {
            id: Some(r.id),
            device_id: r.device_id,
            device_type: r.device_type,
            manufacturer: r.manufacturer,
            model: r.model,
            firmware_version: r.firmware_version,
            installed_at: ts_to_datetime(r.installed_at, "asset.installed_at"),
            last_maintenance: r
                .last_maintenance
                .map(|t| ts_to_datetime(t, "asset.last_maintenance")),
        }
    }
}
