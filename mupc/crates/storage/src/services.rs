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

/// 缓冲保留上限的**生产缺省值**（点数）——U-68③「必须有界」的那个界。
///
/// 取值的依据（不是随手挑的数）：
/// - 生产装配是 `capacity=1000 / flush_interval_ms=5000`（`mupc-core-bin/src/startup.rs`），
///   故 10 000 = **10 个满批**：落库短暂抖动（一次 `busy_timeout`=5 s 量级）期间点数不会触顶，
///   真的落库长故障时又不会无限增长；
/// - 单点 `TelemetryPoint` 约 100 B 量级 ⇒ 满额约 1 MB，对 RK3588 是安全的内存上界。
pub const DEFAULT_MAX_BUFFERED_POINTS: usize = 10_000;

/// 写入缓冲管理器 — 双缓冲批量写入，事务保证原子性
///
/// # 失败语义（U-68③，2026-09-23 起）
///
/// 「先 `drain` 再 `await commit`」的形态保留（不改成"写成功后再移除"），但**写失败不再整批丢弃**：
/// 失败批次**回填到缓冲头部**（比期间新产的点更旧 ⇒ 保时间序），下次 flush 连同新点一起重试。
/// 回填是**有界**的（[`Self::max_points`]）：超限时丢**最旧**的并 `error!` + 计数
/// （[`Self::dropped_points`] / [`Self::dropped_batches`]，均为 `AtomicU64`，无新增依赖）。
///
/// 原语义的残留窗口也由同一机制兜住：任务在 `commit_batch` 的 await 上被 `abort`/panic 时，
/// future 被丢弃、连 `Err` 分支都走不到 ⇒ 由 [`BatchGuard::drop`] 回填（见 U-64）。
pub struct WriteBuffer {
    capacity: usize,
    flush_interval_ms: u64,
    /// 缓冲**保留上限**（点数）。同时约束"生产者新 push"与"失败回填"两条增长路径 ——
    /// 少了后者就是无界重试（落库长故障时内存无上限）。
    max_points: usize,
    buffer: Mutex<Vec<TelemetryPoint>>,
    /// **自上次提交尝试以来**新 push 的点数（不把回填的老点重复计入）。
    ///
    /// 为什么不用 `buffer.len()` 当容量触发判据：失败回填会让 `len` 长期 ≥ `capacity`，
    /// 于是**每个 push 都触发一次注定失败的大批提交**（活锁式的白烧 CPU）。用"新点数"
    /// 计数后，失败一次就要再攒够 `capacity` 个新点才会由 push 触发；**恢复主要靠定时任务**
    /// （生产 5 s 一拍），故不会把恢复延迟到"再攒 1000 点"。
    since_attempt: std::sync::atomic::AtomicUsize,
    dropped_points: std::sync::atomic::AtomicU64,
    dropped_batches: std::sync::atomic::AtomicU64,
    requeued_batches: std::sync::atomic::AtomicU64,
    pool: Arc<SqlitePool>,
}

impl WriteBuffer {
    pub fn new(capacity: usize, flush_interval_ms: u64, pool: Arc<SqlitePool>) -> Self {
        Self::new_with_max_points(
            capacity,
            flush_interval_ms,
            pool,
            DEFAULT_MAX_BUFFERED_POINTS,
        )
    }

    /// 显式指定保留上限的构造器（生产用 [`Self::new`]；测试/特殊部署用本函数）。
    pub fn new_with_max_points(
        capacity: usize,
        flush_interval_ms: u64,
        pool: Arc<SqlitePool>,
        max_points: usize,
    ) -> Self {
        Self {
            capacity,
            flush_interval_ms,
            // 至少 1（0 会让缓冲"存不住任何点"）。**不**额外向 `capacity` 靠：上限由调用方说了算，
            // 若上限小于一个满批，则容量触发那批也会被同一条裁剪规则约束（界依然成立）。
            max_points: max_points.max(1),
            buffer: Mutex::new(Vec::with_capacity(capacity)),
            since_attempt: std::sync::atomic::AtomicUsize::new(0),
            dropped_points: std::sync::atomic::AtomicU64::new(0),
            dropped_batches: std::sync::atomic::AtomicU64::new(0),
            requeued_batches: std::sync::atomic::AtomicU64::new(0),
            pool,
        }
    }

    /// 入缓冲（满 `capacity` 触发一次批量提交；失败上抛但点**不丢**，见类型文档）。
    pub async fn buffer_telemetry(&self, point: TelemetryPoint) -> Result<(), StorageError> {
        let (maybe_batch, dropped) = {
            let mut buf = self.buffer.lock();
            buf.push(point);
            // 容量触发按**自上次尝试以来新 push 的点数**计（失败回填的老点不重复计入）——
            // 否则失败后 `len` 长期 ≥ capacity，每个 push 都会触发一次注定失败的大批提交。
            let since = self
                .since_attempt
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1;
            if since >= self.capacity {
                self.since_attempt
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                // 先 drain 再谈裁剪：这批马上要**尝试提交**，此刻裁掉就等于"丢掉本可以入库的点"。
                // drain 后缓冲已空 ⇒ 有界性自动成立；真提交失败时由 `requeue_front` 裁剪并计数
                // （那一刻才确认这些点没进库，丢最旧才是正确的）。
                let batch: Vec<TelemetryPoint> = buf.drain(..).collect();
                buf.reserve(self.capacity);
                (Some(batch), 0)
            } else {
                // 无提交可试 ⇒ 就在这里守住上限（`Vec` 尾插头删 ⇒ 头部就是"最旧"）。
                (None, self.trim_oldest(&mut buf))
            }
        };
        if dropped > 0 {
            self.log_dropped(dropped, "buffer_telemetry 入缓冲时超上限");
        }
        if let Some(batch) = maybe_batch {
            self.flush_batch(batch).await?;
        }
        Ok(())
    }

    /// 按 [`Self::max_points`] 裁掉**最旧**的溢出点（调用方须持 `buffer` 锁）。
    /// 返回本次裁掉的条数；同时累加计数器（原子量，持锁调用安全）。
    fn trim_oldest(&self, buf: &mut Vec<TelemetryPoint>) -> usize {
        if buf.len() <= self.max_points {
            return 0;
        }
        let excess = buf.len() - self.max_points;
        buf.drain(..excess);
        self.dropped_points
            .fetch_add(excess as u64, std::sync::atomic::Ordering::Relaxed);
        self.dropped_batches
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        excess
    }

    /// 真丢弃时的**响亮**日志（U-68③ 下半句：只在**真的**丢数据时出现）。
    fn log_dropped(&self, dropped: usize, where_: &str) {
        tracing::error!(
            dropped,
            buffered = self.buffered_points(),
            max_points = self.max_points,
            dropped_total = self.dropped_points(),
            "遥测缓冲超上限（{where_}）：丢弃**最旧**的 {dropped} 条（累计 dropped_total）"
        );
    }

    /// 把一批"已 drain、未提交"的点**回填到缓冲头部**，并按上限裁剪。
    ///
    /// 为什么回**头部**：失败批里的点比"期间新产的点"更旧，头插才能保持时间序
    /// （`trim_oldest` 从头删 = 丢最旧，语义一致）。
    ///
    /// 返回 `(回填后缓冲点数, 本次裁剪丢弃数)`。
    fn requeue_front(&self, failed: Vec<TelemetryPoint>) -> (usize, usize) {
        if failed.is_empty() {
            return (self.buffered_points(), 0);
        }
        let retained;
        let dropped;
        {
            let mut buf = self.buffer.lock();
            let mut merged = failed;
            merged.append(&mut buf);
            *buf = merged;
            dropped = self.trim_oldest(&mut buf);
            retained = buf.len();
        }
        self.requeued_batches
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        (retained, dropped)
    }

    /// 当前缓冲中的点数（观测用；含失败回填尚未重试的点）。
    pub fn buffered_points(&self) -> usize {
        self.buffer.lock().len()
    }

    /// 保留上限（点数），见 [`DEFAULT_MAX_BUFFERED_POINTS`]。
    pub fn max_points(&self) -> usize {
        self.max_points
    }

    /// **因超上限而被丢弃**的累计点数（U-68③；供观测，只增不减）。
    pub fn dropped_points(&self) -> u64 {
        self.dropped_points
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 发生"丢弃最旧"的累计次数。
    pub fn dropped_batches(&self) -> u64 {
        self.dropped_batches
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 落库失败后**回填**缓冲的批次数（含被 abort 时由 `BatchGuard` 回填的那些）。
    pub fn requeued_batches(&self) -> u64 {
        self.requeued_batches
            .load(std::sync::atomic::Ordering::Relaxed)
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
    /// 与 `flush()` 的**失败语义同源**：提交失败 ⇒ 本批**回填**缓冲待下次重试（U-68③），
    /// 故本任务只需在成功时记一条 debug；失败已由 `flush_batch` 统一响亮化，不重复打第二条。
    ///
    /// **收工（U-64）**：`stop` 收到停机信号即退出（`select!` 与 tick 二选一）。这一步是必须的：
    /// 本任务是**唯一会在运行期 drain 缓冲**的常驻者，若被 `abort` 在半路（已 drain、未提交），
    /// 即便有 `BatchGuard` 兜底也仍会与退出路径的 flush 抢时序 ⇒ 优雅退出的顺序是
    /// "先让它确认收工，再做最后一次 flush"（见 `mupc-core-bin` 的 `StartupContext::shutdown`）。
    pub fn spawn_flush_timer(
        self: Arc<Self>,
        mut stop: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
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
                tokio::select! {
                    // 停机信号（或发送端已 drop）⇒ 收工。剩余缓冲由退出路径的最后一次 flush 落盘。
                    _ = stop.changed() => {
                        tracing::debug!("停机信号：定时 flush 任务收工（剩余缓冲交退出路径 flush）");
                        break;
                    }
                    _ = ticker.tick() => {}
                }
                match self.flush().await {
                    Ok(0) => {}
                    Ok(n) => tracing::debug!(points = n, "定时 flush 已提交遥测批次"),
                    // 失败已由 `flush_batch` 统一响亮化（带"回填/丢弃"条数）⇒ 此处不重复打第二条。
                    // 下一周期会连同回填的旧点一起重试（见 `flush_batch`）。
                    Err(_) => {}
                }
            }
        })
    }

    /// 调用方负责定时调用（见 [`Self::spawn_flush_timer`] 与优雅退出路径）。使用事务保证批量写入原子性。
    ///
    /// 失败时本批**留在缓冲**（回填头部）并返回 `Err`（调用方据 `Err` 记日志/告警）；
    /// 成功时缓冲里该批已移除。`since_attempt` 随 drain 归零（那批点已算"尝试过一次"）。
    pub async fn flush(&self) -> Result<usize, StorageError> {
        let batch: Vec<TelemetryPoint> = {
            let mut buf = self.buffer.lock();
            let drained: Vec<TelemetryPoint> = buf.drain(..).collect();
            if !drained.is_empty() {
                buf.reserve(self.capacity);
                self.since_attempt
                    .store(0, std::sync::atomic::Ordering::Relaxed);
            }
            drained
        };
        self.flush_batch(batch).await
    }

    /// 提交一批（调用方已把它从缓冲取出）。**失败不丢**：整批回填缓冲头部（U-68③）。
    ///
    /// 三层保护，各挡一类形态：
    /// 1. 提交返回 `Err` ⇒ 显式回填 + 响亮日志（带"回填了多少 / 是否因超上限丢弃"）；
    /// 2. future 被 `abort` / 任务 panic ⇒ 走不到 `Err` 分支（`abort` 是丢弃 future，不是返回 `Err`）
    ///    ⇒ 由 [`BatchGuard::drop`] 回填；
    /// 3. 回填本身可能把缓冲顶到上限之上 ⇒ `requeue_front` 内按 [`Self::max_points`] 裁最旧的并计数。
    ///
    /// **为什么不会死锁/活锁**：回填只取一次 `buffer` 锁（不跨 await）、提交期不持锁；回填不重试，
    /// 重试由下一次 `flush()`（定时任务 5 s 一拍，或退出路径）驱动；失败后"每个 push 都重试"这条
    /// 风暴路径由 `since_attempt` 计数挡掉（见 `buffer_telemetry`）。
    async fn flush_batch(&self, batch: Vec<TelemetryPoint>) -> Result<usize, StorageError> {
        let count = batch.len();
        if count == 0 {
            return Ok(0);
        }
        let mut guard = BatchGuard::new(self, batch);
        match self.commit_batch(guard.points()).await {
            Ok(()) => {
                guard.disarm();
                Ok(count)
            }
            Err(e) => {
                let (retained, dropped) = guard.requeue_now();
                if dropped > 0 {
                    tracing::error!(
                        points = count,
                        retained,
                        dropped,
                        dropped_total = self.dropped_points(),
                        error = %e,
                        "遥测批量落库失败：本批 {count} 条已回填缓冲，但**超出上限**，丢弃最旧的 {dropped} 条"
                    );
                } else {
                    tracing::error!(
                        points = count,
                        retained,
                        error = %e,
                        "遥测批量落库失败：本批 {count} 条已**回填**缓冲待下次重试（不丢弃；下一周期连同新点一起重试）"
                    );
                }
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

/// 「已 `drain`、未提交」批次的守卫 —— 堵 U-64 的丢点窗口（**不丢**由结构保证，而不是靠时序）。
///
/// 为什么需要它：`abort()` 是**丢弃 future**，不是让 future 返回 `Err` ⇒ 任务恰好停在
/// `commit_batch` 的 await 上时，`flush_batch` 的 `Err` 分支**根本不会执行**，那一批点连日志
/// 都没有就没了。把批次交给本守卫后，无论走哪条路（正常失败 / future 被 abort / 任务 panic
/// 展开），未提交的点都会在 `Drop` 里回到缓冲头部，等退出路径或下一拍 flush 落盘。
///
/// 为什么**不会重复写入**：`tx.commit()` 返回后紧接着 `disarm()`，两者之间**没有 await 点**
/// ⇒ tokio 的取消只能在 await 点落地，不存在"已提交但还没 disarm"的可中断窗口。
struct BatchGuard<'a> {
    buffer: &'a WriteBuffer,
    points: Vec<TelemetryPoint>,
    armed: bool,
}

impl<'a> BatchGuard<'a> {
    fn new(buffer: &'a WriteBuffer, points: Vec<TelemetryPoint>) -> Self {
        Self {
            buffer,
            points,
            armed: true,
        }
    }

    fn points(&self) -> &[TelemetryPoint] {
        &self.points
    }

    /// 提交成功：解除守卫（`Drop` 不再回填）。
    fn disarm(&mut self) {
        self.armed = false;
        self.points.clear();
    }

    /// 显式回填（失败路径；顺带解除守卫，避免 `Drop` 二次回填）。返回 `(保留点数, 丢弃点数)`。
    fn requeue_now(&mut self) -> (usize, usize) {
        if !self.armed {
            return (self.buffer.buffered_points(), 0);
        }
        self.armed = false;
        self.buffer.requeue_front(std::mem::take(&mut self.points))
    }
}

impl Drop for BatchGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let (retained, dropped) = self.buffer.requeue_front(std::mem::take(&mut self.points));
        // 隐式回填 = 提交没走完就没了（abort / panic）⇒ 记 warn 供观测（不 panic：Drop 里不能）。
        tracing::warn!(
            retained,
            dropped,
            dropped_total = self.buffer.dropped_points(),
            "遥测批次提交**未完成即被中止**（abort/panic）：已回填缓冲避免静默丢点"
        );
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

/// `telemetry` 建表 DDL（**新库的形态**：`value REAL` 可空，03 设计 §9.1.4）。
///
/// 只此一处持有 DDL 文本：`run_migrations` 的建表语句与「可空化重建」共用它 ⇒ 重建后的表结构
/// 与新建的表**逐字一致**（否则重建产物与现场新装产物会漂移）。
const TELEMETRY_DDL: &str = "CREATE TABLE IF NOT EXISTS telemetry (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            metric_name TEXT NOT NULL,
            value REAL,
            quality INTEGER NOT NULL DEFAULT 0
        )";

/// `telemetry` 的索引 DDL（**两个都要**在可空化重建后重放：只重建
/// `idx_telemetry_metric_ts` 会让 `query_range` 的 `device_id + timestamp` 路径丢索引）。
const TELEMETRY_INDEX_DDL: [&str; 2] = [
    "CREATE INDEX IF NOT EXISTS idx_telemetry_device_ts
         ON telemetry(device_id, timestamp)",
    "CREATE INDEX IF NOT EXISTS idx_telemetry_metric_ts
         ON telemetry(metric_name, timestamp)",
];

/// 数据库迁移：建表与索引
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), StorageError> {
    let statements = [
        // 遥测表 — 按月分区建议用外部脚本，这里建基础表
        // ⚠️ `value REAL`（**可空**）：缺测行落**真 NULL**（03 设计 §9.1.4 / PRD R-11.2-E）。
        // 老库（`value REAL NOT NULL`）由本函数末尾的幂等「可空化重建」就地升级。
        TELEMETRY_DDL,
        TELEMETRY_INDEX_DDL[0],
        TELEMETRY_INDEX_DDL[1],
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

    // ── `telemetry.value` 可空化（03 设计 §9.1.4；本增量唯一的**结构变更**）──
    ensure_telemetry_value_nullable(pool).await?;

    Ok(())
}

/// 把老库的 `telemetry.value REAL NOT NULL` 就地升级为**可空**（缺测行要写真 `NULL`）。
///
/// # 为什么必须重建表
///
/// SQLite 不支持 `ALTER COLUMN` 去约束 ⇒ 只能「改名 → 按新 DDL 重建 → 搬数据 → 删旧表 →
/// 重放索引」。老库中**既有行全部为 `Some`**（此前 `value` 是 `f64`）⇒ 搬过去后**值逐行不变**、
/// 语义不变（PRD R-11.3-C 的「零行为变化」仍成立）。
///
/// # 幂等
///
/// 每次 `run_migrations` 先 `PRAGMA table_info(telemetry)` 读 `value` 列的 `notnull`：
/// **已为 0（可空）即跳过**（含「本进程刚建的新表」与「已迁移过的库」两种情形）。
/// 迁移动作整体在一个事务里 ⇒ 中途失败不留半成品。
///
/// # 必须一并重建**两个**索引
///
/// `RENAME TO` 后索引仍挂在旧表上、随 `DROP TABLE telemetry_old` 一并消失 ⇒ 不重放就会让
/// `query_range`（走 `idx_telemetry_device_ts`）与按 `metric_name` 的查询
/// （走 `idx_telemetry_metric_ts`）直到**下次启动**（`CREATE INDEX IF NOT EXISTS`）才恢复索引
/// （03 设计 §9.1.4 的勘误 ②：原稿只列一个索引，实际**两个都要重建**）。
async fn ensure_telemetry_value_nullable(pool: &SqlitePool) -> Result<(), StorageError> {
    // `notnull` 是 SQLite 关键字 ⇒ 取列时加双引号。`pragma_table_info` 是表值函数，可显式选列。
    let cols: Vec<(String, i64)> =
        sqlx::query_as("SELECT name, \"notnull\" FROM pragma_table_info('telemetry')")
            .fetch_all(pool)
            .await
            .map_err(|e| StorageError::MigrationError(e.to_string()))?;

    let notnull = cols
        .iter()
        .find(|(name, _)| name == "value")
        .map(|(_, notnull)| *notnull)
        .ok_or_else(|| {
            StorageError::MigrationError(
                "telemetry 表缺 value 列（建表语句与迁移前置不一致）".to_string(),
            )
        })?;

    // 已可空 ⇒ 幂等跳过（GRD-09：对已迁移库再跑不重复重建）。
    if notnull == 0 {
        return Ok(());
    }
    tracing::info!("telemetry.value 为 NOT NULL（老库）⇒ 执行一次性可空化重建");

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| StorageError::MigrationError(e.to_string()))?;

    let step = |e: sqlx::Error| StorageError::MigrationError(format!("telemetry 可空化失败: {e}"));

    sqlx::query("ALTER TABLE telemetry RENAME TO telemetry_old")
        .execute(&mut *tx)
        .await
        .map_err(step)?;
    // 同一 DDL 但 `value REAL`（去掉 NOT NULL）—— 与新建库逐字一致。
    sqlx::query(TELEMETRY_DDL)
        .execute(&mut *tx)
        .await
        .map_err(step)?;
    // 显式列名搬数据：老库既有行全为 `Some` ⇒ 逐行等值（`NULL` 行也原样搬）。
    sqlx::query(
        "INSERT INTO telemetry (id, device_id, timestamp, metric_name, value, quality)
         SELECT id, device_id, timestamp, metric_name, value, quality FROM telemetry_old",
    )
    .execute(&mut *tx)
    .await
    .map_err(step)?;
    sqlx::query("DROP TABLE telemetry_old")
        .execute(&mut *tx)
        .await
        .map_err(step)?;
    // 两个索引都必须重放（`RENAME`/`DROP` 把原索引一起带走了）。
    for ddl in TELEMETRY_INDEX_DDL {
        sqlx::query(ddl).execute(&mut *tx).await.map_err(step)?;
    }

    tx.commit()
        .await
        .map_err(|e| StorageError::MigrationError(format!("telemetry 可空化提交失败: {e}")))?;
    tracing::info!("telemetry.value 可空化完成（既有行数值不变）");
    Ok(())
}

#[cfg(test)]
mod tests {
    /// 本文件的生产段源码（`#[cfg(test)] mod tests` 之前；行尾归一化为 LF）。
    ///
    /// 同 `mupc-core-bin/src/startup.rs` 的 `production_src`：断言自身含被禁串 ⇒ 必须切段，
    /// 否则 `contains` 会因断言自己而恒真/恒假（自指坏网）。锚点缺失即 panic（不静默退化成空串）。
    fn production_src() -> String {
        let src = include_str!("services.rs").replace("\r\n", "\n");
        let (production, _) = src
            .split_once("\n#[cfg(test)]\nmod tests {")
            .expect("`#[cfg(test)] mod tests` 标记必须存在（生产段/测试段的分段锚点）");
        assert!(
            production.len() > 5_000,
            "分段锚点必须真的切出生产段，实得 {} 字节",
            production.len()
        );
        production.to_string()
    }

    /// **U-68③ 日志网**：旧的那句「本批 N 条已从缓冲取出，**提交失败即丢弃**」是**无条件**丢弃
    /// 语义的自述；新语义下它不得再出现，改为「回填 + 仅真丢弃时告警」。
    ///
    /// 行为级证据在 `crates/storage/tests/integration.rs`（失败不丢 / 超限丢最旧 / abort 回填）；
    /// 本网只挡"日志与实际语义脱钩"这一最容易漏的形态（文案没改 = 现场按旧语义误判数据已丢）。
    ///
    /// 改什么会让本条变红：把 `flush_batch` 的失败分支写回"整批丢弃"文案（或删掉回填路径）。
    #[test]
    fn failed_commit_log_does_not_claim_unconditional_drop() {
        let production = production_src();
        assert!(
            !production.contains("提交失败即丢弃"),
            "旧的无条件丢弃文案必须消失（新语义是回填待重试）"
        );
        assert!(
            production.contains("回填"),
            "生产段必须体现回填语义（失败批次回缓冲头部）"
        );
        // 计数器可读：丢弃/回填都要有 AtomicU64 计数（供后续观测）
        assert!(
            production.contains("dropped_points") && production.contains("requeued_batches"),
            "丢弃/回填必须可计数（U-68③ 要求计数器可读）"
        );
    }
}
