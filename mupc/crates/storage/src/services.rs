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
    // 参数即表列（`action_space_config` 的写入面），拆结构体属公开 API 变更，
    // 会波及 ai-engine 侧调用点 ⇒ 此处按签名原样放行。
    #[allow(clippy::too_many_arguments)]
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
    // 同 `update_action_space_config`：14 个参数 = 全字段写入面，放行理由一致。
    #[allow(clippy::too_many_arguments)]
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

    /// 启动**周期 flush** 任务 —— 时间触发半边（设计 03:1321「100ms 或积累 100 条触发批量事务
    /// 提交（**先到先执行**）」）。
    ///
    /// 修复前 `flush_interval_ms` **无任何读取方**（只在 `new` 里存下、只被 getter 读出）：
    /// 只有"攒满 `capacity`"一条触发路径 ⇒ 不满一批的数据**永久滞留内存**、断电即丢。定时任务
    /// 补上另一条：到点即提交当前批次，与容量触发**互为先到先执行**（两者都只是调用 `flush()`，
    /// 由 `buffer` 互斥锁串行化，谁先拿到谁提交，不会重复提交同一批）。
    ///
    /// **为什么放在 storage 而不是调用方**：`flush_interval_ms` 是 `WriteBuffer` 自己的字段，
    /// 契约（容量 OR 时间）应由持有该字段的类型自己解释；放到 core-bin 就得把间隔再读一遍、
    /// 在装配层重建一遍节拍。core-bin 的职责只剩"spawn + 句柄入 TaskGuard"（装配期失败即 abort）。
    ///
    /// 调用方**必须**持有返回句柄：它只负责周期触发，**不负责退出落盘** —— 优雅退出的最后一批由
    /// `flush()` 承担（见 `mupc-core-bin` 的 `StartupContext::shutdown`）。
    ///
    /// 与 `flush()` 的**失败语义同源**：`flush()` 内部已先 drain 再提交，提交失败即整批丢弃
    /// （不重试，设计未见落库侧背压/重试条款）⇒ 本任务只做**响亮化**（`flush_batch` 内 error 日志
    /// 带丢弃条数），不引入自创重试。
    pub fn spawn_flush_timer(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        // `interval(0)` 会 panic；0 视为"每个 tick 立即到点"的最小正周期（1ms），
        // 不静默退化成"永不触发"。
        let period = std::time::Duration::from_millis(self.flush_interval_ms.max(1));
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(period);
            // 落后时按"顺延"而不是"追赶补打"：补打只会连续产生空批（数据早已被上一批带走），
            // 白占一次事务。写阻塞后恢复时按原节拍继续即可。
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // 第一次 `tick()` 立即返回（缓冲刚建、必然为空）⇒ 先吞掉，避免启动瞬间一次空 flush。
            ticker.tick().await;
            loop {
                ticker.tick().await;
                match self.flush().await {
                    Ok(0) => {}
                    Ok(n) => tracing::debug!(points = n, "定时 flush 已提交遥测批次"),
                    // 失败已由 `flush_batch` 统一响亮化（那里才知道"丢弃了多少条"）⇒ 此处不重复
                    // 打第二条日志。下一周期自然重试**新**数据（本批不重试，见 `flush_batch`）。
                    Err(_) => {}
                }
            }
        })
    }

    /// 调用方负责定时调用（见 [`Self::spawn_flush_timer`] 与优雅退出路径）。使用事务保证批量写入原子性。
    pub async fn flush(&self) -> Result<usize, StorageError> {
        let batch: Vec<TelemetryPoint> = {
            let mut buf = self.buffer.lock();
            let drained = buf.drain(..).collect();
            buf.reserve(self.capacity);
            drained
        };
        self.flush_batch(batch).await
    }

    /// 失败语义（P0-1 处置口径）：`batch` 已由调用方 `drain` 出缓冲 ⇒ 只要走到这里，提交失败
    /// 就是**整批丢弃**（丢的是内存里那批，不在库里）。设计**未见**落库侧重试/背压明文
    /// （审查报告将此条定性为【部分】）⇒ 本轮**不自创**重试/背压，只做**响亮化**：把"丢了多少条"
    /// 明确打进 error 日志——避免"批量写入静默丢数据"这一最坏的静默失实形态。
    async fn flush_batch(&self, batch: Vec<TelemetryPoint>) -> Result<usize, StorageError> {
        let count = batch.len();
        if count == 0 {
            return Ok(0);
        }
        match self.commit_batch(&batch).await {
            Ok(()) => Ok(count),
            Err(e) => {
                tracing::error!(
                    points = count,
                    error = %e,
                    "遥测批量落库失败：本批 {} 条已从缓冲取出，提交失败即丢弃（设计无重试/背压条款，本轮不重试）",
                    count
                );
                Err(e)
            }
        }
    }

    /// 真正的批量事务提交（`flush_batch` 只负责失败计数与日志）。
    async fn commit_batch(&self, batch: &[TelemetryPoint]) -> Result<(), StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        for point in batch {
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
        Ok(())
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
    ];

    for stmt in &statements {
        sqlx::query(stmt)
            .execute(pool)
            .await
            .map_err(|e| StorageError::MigrationError(e.to_string()))?;
    }

    // v2.6 扩展字段（幂等: 先检查列是否存在再 ALTER TABLE）
    let existing_cols: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_table_info('action_space_config')",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| StorageError::MigrationError(e.to_string()))?;

    let alter_stmts = [
        ("transformer_kva",     "ALTER TABLE action_space_config ADD COLUMN transformer_kva REAL NOT NULL DEFAULT 0.0"),
        ("battery_capacity_kwh", "ALTER TABLE action_space_config ADD COLUMN battery_capacity_kwh REAL NOT NULL DEFAULT 0.0"),
        ("soc_min",             "ALTER TABLE action_space_config ADD COLUMN soc_min REAL NOT NULL DEFAULT 0.0"),
        ("soc_max",             "ALTER TABLE action_space_config ADD COLUMN soc_max REAL NOT NULL DEFAULT 1.0"),
        ("overload_threshold",  "ALTER TABLE action_space_config ADD COLUMN overload_threshold REAL NOT NULL DEFAULT 1.2"),
    ];

    for (col_name, stmt) in &alter_stmts {
        if !existing_cols.iter().any(|c| c == col_name) {
            sqlx::query(stmt)
                .execute(pool)
                .await
                .map_err(|e| StorageError::MigrationError(e.to_string()))?;
        }
    }

    Ok(())
}
