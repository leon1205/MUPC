use crate::errors::DataProcessingError;
use crate::recorder::{ExportResult, FaultEventFilter, FaultRecorder, WaveformSummary};
use crate::telemetry::{FaultCondition, WaveformData};
use crate::waveform::export::{ComtradeExporter, CsvExporter, DEFAULT_CHANNEL_NAMES};
use crate::waveform::storage::WaveformReader;
use async_trait::async_trait;
use chrono::Utc;
use mupc_common::MupcError;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 故障类型枚举
#[derive(Debug, Clone, PartialEq)]
pub enum FaultType {
    BatteryOverTemp,
    BatteryUnderSoc,
    GridOverload,
    GridReverse,
    OverVoltage,
    UnderVoltage,
    OverCurrent,
    FrequencyAbnormal,
    PvOutputLimit,
    Unknown,
}

impl FaultType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FaultType::BatteryOverTemp => "BATTERY_OVER_TEMP",
            FaultType::BatteryUnderSoc => "BATTERY_UNDER_SOC",
            FaultType::GridOverload => "GRID_OVERLOAD",
            FaultType::GridReverse => "GRID_REVERSE",
            FaultType::OverVoltage => "OVER_VOLTAGE",
            FaultType::UnderVoltage => "UNDER_VOLTAGE",
            FaultType::OverCurrent => "OVER_CURRENT",
            FaultType::FrequencyAbnormal => "FREQUENCY_ABNORMAL",
            FaultType::PvOutputLimit => "PV_OUTPUT_LIMIT",
            FaultType::Unknown => "UNKNOWN",
        }
    }
}

/// 故障记录
#[derive(Debug, Clone)]
pub struct FaultRecord {
    pub id: i64,
    pub fault_type: String,
    pub trigger_time: i64,
    pub over_voltage: Option<f64>,
    pub under_voltage: Option<f64>,
    pub over_current: Option<f64>,
    pub frequency_abnormal: Option<f64>,
}

/// 故障录波器实现（SQLite）
pub struct FaultRecorderImpl {
    conn: Mutex<Connection>,
    recording: Mutex<bool>,
}

impl FaultRecorderImpl {
    pub fn new(db_path: &PathBuf) -> Result<Self, DataProcessingError> {
        let conn = Connection::open(db_path)
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        // 启用 WAL 模式提升并发读写性能
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS fault_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                fault_type TEXT NOT NULL,
                trigger_time INTEGER NOT NULL,
                over_voltage REAL,
                under_voltage REAL,
                over_current REAL,
                frequency_abnormal REAL,
                waveform_path TEXT,
                sample_rate INTEGER,
                pre_trigger_ms INTEGER,
                post_trigger_ms INTEGER,
                channel_mask INTEGER,
                trigger_type TEXT,
                trigger_threshold REAL,
                file_size_bytes INTEGER,
                checksum TEXT,
                exported INTEGER DEFAULT 0
            )",
            [],
        )
        .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        // 对已存在的旧表执行 ALTER TABLE 添加新列（忽略"列已存在"错误）
        Self::migrate_waveform_columns(&conn);

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_trigger_time ON fault_records(trigger_time)",
            [],
        )
        .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        // 清理超过30天的旧记录
        if let Err(e) = Self::cleanup_old_records_impl(&conn, 30) {
            tracing::warn!("清理旧故障记录失败: {}", e);
        }

        Ok(Self {
            conn: Mutex::new(conn),
            recording: Mutex::new(false),
        })
    }

    /// 为已存在的旧表迁移添加波形元数据列（幂等：忽略"duplicate column"错误）
    fn migrate_waveform_columns(conn: &Connection) {
        let columns = [
            "ALTER TABLE fault_records ADD COLUMN waveform_path TEXT",
            "ALTER TABLE fault_records ADD COLUMN sample_rate INTEGER",
            "ALTER TABLE fault_records ADD COLUMN pre_trigger_ms INTEGER",
            "ALTER TABLE fault_records ADD COLUMN post_trigger_ms INTEGER",
            "ALTER TABLE fault_records ADD COLUMN channel_mask INTEGER",
            "ALTER TABLE fault_records ADD COLUMN trigger_type TEXT",
            "ALTER TABLE fault_records ADD COLUMN trigger_threshold REAL",
            "ALTER TABLE fault_records ADD COLUMN file_size_bytes INTEGER",
            "ALTER TABLE fault_records ADD COLUMN checksum TEXT",
            "ALTER TABLE fault_records ADD COLUMN exported INTEGER DEFAULT 0",
        ];
        for sql in columns {
            // 忽略"duplicate column name"错误（新表已通过 CREATE TABLE 包含全部列）
            let _ = conn.execute(sql, []);
        }
    }

    fn cleanup_old_records_impl(
        conn: &Connection,
        retention_days: i64,
    ) -> Result<usize, DataProcessingError> {
        let cutoff = Utc::now().timestamp() - (retention_days * 86400);
        let deleted = conn
            .execute(
                "DELETE FROM fault_records WHERE trigger_time < ?1",
                [cutoff],
            )
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;
        Ok(deleted)
    }

    pub fn cleanup_old_records(&self, retention_days: i64) -> Result<usize, DataProcessingError> {
        let conn = self.conn.lock().unwrap();
        Self::cleanup_old_records_impl(&conn, retention_days)
    }

    pub fn new_in_memory() -> Result<Self, DataProcessingError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        // 启用 WAL 模式
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        conn.execute(
            "CREATE TABLE fault_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                fault_type TEXT NOT NULL,
                trigger_time INTEGER NOT NULL,
                over_voltage REAL,
                under_voltage REAL,
                over_current REAL,
                frequency_abnormal REAL,
                waveform_path TEXT,
                sample_rate INTEGER,
                pre_trigger_ms INTEGER,
                post_trigger_ms INTEGER,
                channel_mask INTEGER,
                trigger_type TEXT,
                trigger_threshold REAL,
                file_size_bytes INTEGER,
                checksum TEXT,
                exported INTEGER DEFAULT 0
            )",
            [],
        )
        .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
            recording: Mutex::new(false),
        })
    }

    pub fn record_sync(&self, condition: &FaultCondition) -> Result<(), DataProcessingError> {
        let conn = self.conn.lock().unwrap();
        let trigger_time = Utc::now().timestamp();

        let fault_type = self.determine_fault_type(condition);

        conn.execute(
            "INSERT INTO fault_records (fault_type, trigger_time, over_voltage, under_voltage, over_current, frequency_abnormal)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                fault_type.as_str(),
                trigger_time,
                condition.over_voltage,
                condition.under_voltage,
                condition.over_current,
                condition.frequency_abnormal
            ],
        ).map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    fn determine_fault_type(&self, condition: &FaultCondition) -> FaultType {
        if condition.over_voltage.is_some() && condition.over_voltage.unwrap() > 420.0 {
            return FaultType::OverVoltage;
        }
        if condition.over_current.is_some() && condition.over_current.unwrap() > 150.0 {
            return FaultType::OverCurrent;
        }
        if condition.under_voltage.is_some() && condition.under_voltage.unwrap() < 200.0 {
            return FaultType::UnderVoltage;
        }
        if let Some(freq) = condition.frequency_abnormal {
            if !(49.5..=50.5).contains(&freq) {
                return FaultType::FrequencyAbnormal;
            }
        }
        FaultType::Unknown
    }

    pub fn query_sync(
        &self,
        start: i64,
        end: i64,
    ) -> Result<Vec<FaultRecord>, DataProcessingError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, fault_type, trigger_time, over_voltage, under_voltage, over_current, frequency_abnormal
             FROM fault_records WHERE trigger_time BETWEEN ?1 AND ?2"
        ).map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        let records = stmt
            .query_map([start, end], |row| {
                Ok(FaultRecord {
                    id: row.get(0)?,
                    fault_type: row.get(1)?,
                    trigger_time: row.get(2)?,
                    over_voltage: row.get(3)?,
                    under_voltage: row.get(4)?,
                    over_current: row.get(5)?,
                    frequency_abnormal: row.get(6)?,
                })
            })
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        records
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))
    }

    // === P1-16 新增方法 ===

    /// 高级事件查询（过滤+分页）
    ///
    /// TODO: 实现完整的过滤和分页逻辑
    pub fn query_events_sync(
        &self,
        filter: &FaultEventFilter,
    ) -> Result<crate::recorder::PaginatedEvents, DataProcessingError> {
        let conn = self.conn.lock().unwrap();
        let page = filter.page.unwrap_or(1).max(1);
        let page_size = filter.page_size.unwrap_or(20).min(100);
        let offset = (page - 1) * page_size;

        let mut sql = String::from(
            "SELECT id, fault_type, trigger_time, over_voltage, under_voltage, over_current, frequency_abnormal
             FROM fault_records WHERE 1=1"
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(start) = filter.start_time {
            sql.push_str(&format!(" AND trigger_time >= ?{}", params.len() + 1));
            params.push(Box::new(start));
        }
        if let Some(end) = filter.end_time {
            sql.push_str(&format!(" AND trigger_time <= ?{}", params.len() + 1));
            params.push(Box::new(end));
        }
        if let Some(ref ft) = filter.fault_type {
            sql.push_str(&format!(" AND fault_type = ?{}", params.len() + 1));
            params.push(Box::new(ft.clone()));
        }
        // has_waveform 字段暂未在表中实现（TODO）

        // 获取总数
        let count_sql = sql.replace(
            "SELECT id, fault_type, trigger_time, over_voltage, under_voltage, over_current, frequency_abnormal",
            "SELECT COUNT(*)",
        );
        let total: u64 = conn
            .query_row(
                &count_sql,
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                |row| row.get(0),
            )
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        // 获取分页数据
        sql.push_str(&format!(
            " ORDER BY trigger_time DESC LIMIT ?{} OFFSET ?{}",
            params.len() + 1,
            params.len() + 2
        ));
        params.push(Box::new(page_size as i64));
        params.push(Box::new(offset as i64));

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        let events = stmt
            .query_map(
                rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
                |row| {
                    Ok(FaultRecord {
                        id: row.get(0)?,
                        fault_type: row.get(1)?,
                        trigger_time: row.get(2)?,
                        over_voltage: row.get(3)?,
                        under_voltage: row.get(4)?,
                        over_current: row.get(5)?,
                        frequency_abnormal: row.get(6)?,
                    })
                },
            )
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        Ok(crate::recorder::PaginatedEvents {
            events,
            total,
            page,
            page_size,
        })
    }

    /// 按故障类型查询事件
    pub fn query_events_by_type_sync(
        &self,
        fault_type: &str,
    ) -> Result<Vec<FaultRecord>, DataProcessingError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, fault_type, trigger_time, over_voltage, under_voltage, over_current, frequency_abnormal
             FROM fault_records WHERE fault_type = ?1"
        ).map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        let records = stmt
            .query_map([fault_type], |row| {
                Ok(FaultRecord {
                    id: row.get(0)?,
                    fault_type: row.get(1)?,
                    trigger_time: row.get(2)?,
                    over_voltage: row.get(3)?,
                    under_voltage: row.get(4)?,
                    over_current: row.get(5)?,
                    frequency_abnormal: row.get(6)?,
                })
            })
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        records
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))
    }

    /// 按时间范围查询事件
    pub fn query_events_by_time_sync(
        &self,
        start: i64,
        end: i64,
    ) -> Result<Vec<FaultRecord>, DataProcessingError> {
        self.query_sync(start, end)
    }

    /// 从 DB 查询波形的路径和元数据列
    fn query_waveform_meta(
        conn: &Connection,
        event_id: i64,
    ) -> Result<Option<(String, u32, u32, u32, u16, String, f64)>, DataProcessingError> {
        let mut stmt = conn
            .prepare(
                "SELECT waveform_path, sample_rate, pre_trigger_ms, post_trigger_ms,
                        channel_mask, trigger_type, trigger_threshold
                 FROM fault_records WHERE id = ?1",
            )
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        let mut rows = stmt
            .query_map([event_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, u32>(3)?,
                    row.get::<_, u16>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, f64>(6)?,
                ))
            })
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        rows.next()
            .transpose()
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))
    }

    /// 按事件 ID 获取波形数据
    pub fn get_waveform_by_id_sync(
        &self,
        event_id: i64,
    ) -> Result<WaveformData, DataProcessingError> {
        let conn = self.conn.lock().unwrap();
        let meta = Self::query_waveform_meta(&conn, event_id)?;

        match meta {
            Some((waveform_path, sample_rate, pre_ms, post_ms, _channel_mask, trigger_type, _threshold)) => {
                let path = Path::new(&waveform_path);
                let (channels, duration_ms) = if path.exists() {
                    match WaveformReader::open(path) {
                        Ok(mut reader) => {
                            let duration = ((reader.meta.sample_count as f64
                                / reader.meta.sample_rate as f64)
                                * 1000.0) as u64;
                            match reader.read_all() {
                                Ok((ch, _ts)) => (ch, duration),
                                Err(e) => {
                                    tracing::warn!("波形文件读取失败 {}: {}", waveform_path, e);
                                    (vec![], (pre_ms + post_ms) as u64)
                                }
                            }
                        }
                        Err(e) => {
                            tracing::warn!("波形文件打开失败 {}: {}", waveform_path, e);
                            (vec![], (pre_ms + post_ms) as u64)
                        }
                    }
                } else {
                    (vec![], (pre_ms + post_ms) as u64)
                };

                Ok(WaveformData {
                    channels,
                    sample_rate: sample_rate as u64,
                    trigger_timestamp: 0,
                    duration_ms,
                })
            }
            None => {
                tracing::debug!("未找到 event_id={} 的波形记录", event_id);
                Ok(WaveformData {
                    channels: vec![],
                    sample_rate: 0,
                    trigger_timestamp: 0,
                    duration_ms: 0,
                })
            }
        }
    }

    /// 获取波形统计概要
    pub fn get_waveform_summary_sync(
        &self,
        event_id: i64,
    ) -> Result<WaveformSummary, DataProcessingError> {
        let conn = self.conn.lock().unwrap();
        let meta = Self::query_waveform_meta(&conn, event_id)?;

        match meta {
            Some((waveform_path, _sr, pre_ms, _post_ms, _cm, trigger_type, trigger_value)) => {
                let path = Path::new(&waveform_path);
                let (pre_stats, post_stats, ts) = if path.exists() {
                    match WaveformReader::open(path) {
                        Ok(mut reader) => {
                            let pre_samples = reader.meta.pre_trigger_samples as usize;
                            let (channels, timestamps) = reader.read_all().unwrap_or_default();
                            let trigger_ts = timestamps.first().copied().unwrap_or(0);
                            let ch_names: Vec<String> = DEFAULT_CHANNEL_NAMES
                                .iter()
                                .take(channels.len())
                                .map(|s| s.to_string())
                                .collect();
                            let stats = Self::compute_channel_stats(&channels, pre_samples, &ch_names);
                            (stats.pre, stats.post, trigger_ts)
                        }
                        Err(_) => (vec![], vec![], 0),
                    }
                } else {
                    (vec![], vec![], 0)
                };

                Ok(WaveformSummary {
                    event_id,
                    pre_trigger_stats: pre_stats,
                    post_trigger_stats: post_stats,
                    trigger_type,
                    trigger_value,
                    trigger_timestamp: ts,
                })
            }
            None => Ok(WaveformSummary {
                event_id,
                pre_trigger_stats: vec![],
                post_trigger_stats: vec![],
                trigger_type: String::new(),
                trigger_value: 0.0,
                trigger_timestamp: 0,
            }),
        }
    }

    /// 导出 COMTRADE 格式
    pub fn export_comtrade_sync(
        &self,
        event_id: i64,
        output_dir: &Path,
    ) -> Result<ExportResult, DataProcessingError> {
        let conn = self.conn.lock().unwrap();
        let meta = Self::query_waveform_meta(&conn, event_id)?;

        let (waveform_path, device_id) = match meta {
            Some((path, ..)) => (path, "MUPC001".to_string()),
            None => {
                return Ok(ExportResult {
                    files: vec![],
                    format: "COMTRADE".to_string(),
                });
            }
        };

        let waveform_path = Path::new(&waveform_path);
        if !waveform_path.exists() {
            return Ok(ExportResult {
                files: vec![],
                format: "COMTRADE".to_string(),
            });
        }

        let mut reader = WaveformReader::open(waveform_path).map_err(|e| {
            DataProcessingError::WaveformError(format!("打开波形文件失败: {}", e))
        })?;
        let (channels, _timestamps) = reader.read_all().map_err(|e| {
            DataProcessingError::WaveformError(format!("读取波形数据失败: {}", e))
        })?;

        let waveforms_dir = waveform_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let exporter = ComtradeExporter::new(
            waveforms_dir,
            output_dir.to_path_buf(),
            device_id,
        );

        let channel_names: Vec<String> =
            DEFAULT_CHANNEL_NAMES.iter().take(channels.len()).map(|s| s.to_string()).collect();

        let base = output_dir.join(format!("event_{}", event_id));
        let cfg_path = exporter
            .export_cfg(&base.with_extension("cfg"), &reader.meta, &channel_names)
            .map_err(|e| DataProcessingError::WaveformError(format!("COMTRADE cfg 导出失败: {}", e)))?;
        let dat_path = exporter
            .export_dat(&base.with_extension("dat"), &channels)
            .map_err(|e| DataProcessingError::WaveformError(format!("COMTRADE dat 导出失败: {}", e)))?;

        Ok(ExportResult {
            files: vec![cfg_path, dat_path],
            format: "COMTRADE".to_string(),
        })
    }

    /// 导出 CSV 格式
    pub fn export_csv_sync(
        &self,
        event_id: i64,
        output_dir: &Path,
    ) -> Result<ExportResult, DataProcessingError> {
        let conn = self.conn.lock().unwrap();
        let meta = Self::query_waveform_meta(&conn, event_id)?;

        let waveform_path = match meta {
            Some((path, ..)) => path,
            None => {
                return Ok(ExportResult {
                    files: vec![],
                    format: "CSV".to_string(),
                });
            }
        };

        let waveform_path = Path::new(&waveform_path);
        if !waveform_path.exists() {
            return Ok(ExportResult {
                files: vec![],
                format: "CSV".to_string(),
            });
        }

        let mut reader = WaveformReader::open(waveform_path).map_err(|e| {
            DataProcessingError::WaveformError(format!("打开波形文件失败: {}", e))
        })?;
        let (channels, _timestamps) = reader.read_all().map_err(|e| {
            DataProcessingError::WaveformError(format!("读取波形数据失败: {}", e))
        })?;

        let exporter = CsvExporter::new(output_dir.to_path_buf());
        let channel_names: Vec<String> =
            DEFAULT_CHANNEL_NAMES.iter().take(channels.len()).map(|s| s.to_string()).collect();

        let csv_path = output_dir.join(format!("event_{}.csv", event_id));
        let out = exporter
            .export_csv(&csv_path, &channels, &channel_names, reader.meta.sample_rate)
            .map_err(|e| DataProcessingError::WaveformError(format!("CSV 导出失败: {}", e)))?;

        Ok(ExportResult {
            files: vec![out],
            format: "CSV".to_string(),
        })
    }

    /// 计算各通道故障前/后的统计值
    fn compute_channel_stats(
        channels: &[Vec<f64>],
        pre_samples: usize,
        channel_names: &[String],
    ) -> PerTriggerStats {
        let mut pre = Vec::with_capacity(channels.len());
        let mut post = Vec::with_capacity(channels.len());

        for (i, ch) in channels.iter().enumerate() {
            let name = channel_names.get(i).cloned().unwrap_or_default();
            let (pre_slice, post_slice) = if pre_samples < ch.len() {
                (&ch[..pre_samples], &ch[pre_samples..])
            } else {
                (&ch[..], &[][..])
            };

            pre.push(compute_stats(pre_slice, &name));
            post.push(compute_stats(post_slice, &name));
        }

        PerTriggerStats { pre, post }
    }
}

/// 单通道统计值
fn compute_stats(samples: &[f64], channel_name: &str) -> crate::recorder::ChannelStats {
    if samples.is_empty() {
        return crate::recorder::ChannelStats {
            channel_name: channel_name.to_string(),
            max: 0.0,
            min: 0.0,
            avg: 0.0,
            rms: 0.0,
            thd: None,
        };
    }

    let max = samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let min = samples.iter().cloned().fold(f64::INFINITY, f64::min);
    let sum: f64 = samples.iter().sum();
    let avg = sum / samples.len() as f64;
    let rms = (samples.iter().map(|&x| x * x).sum::<f64>() / samples.len() as f64).sqrt();

    crate::recorder::ChannelStats {
        channel_name: channel_name.to_string(),
        max,
        min,
        avg,
        rms,
        thd: None,
    }
}

/// 故障前/后统计分组
struct PerTriggerStats {
    pre: Vec<crate::recorder::ChannelStats>,
    post: Vec<crate::recorder::ChannelStats>,
}

#[async_trait]
impl FaultRecorder for FaultRecorderImpl {
    async fn record(&self, condition: &FaultCondition) -> Result<(), MupcError> {
        self.record_sync(condition).map_err(|e| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                e.to_string(),
                "data-processing",
            )
        })
    }

    async fn query(&self, start: i64, end: i64) -> Result<Vec<FaultRecord>, MupcError> {
        self.query_sync(start, end).map_err(|e| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                e.to_string(),
                "data-processing",
            )
        })
    }

    async fn get_waveform(&self) -> Result<WaveformData, MupcError> {
        Ok(WaveformData {
            channels: vec![],
            sample_rate: 0,
            trigger_timestamp: 0,
            duration_ms: 0,
        })
    }

    fn is_recording(&self) -> bool {
        *self.recording.lock().unwrap()
    }

    // === P1-16 新增方法实现 ===

    async fn query_events(
        &self,
        filter: &FaultEventFilter,
    ) -> Result<crate::recorder::PaginatedEvents, MupcError> {
        self.query_events_sync(filter).map_err(|e| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                e.to_string(),
                "data-processing",
            )
        })
    }

    async fn query_events_by_type(&self, fault_type: &str) -> Result<Vec<FaultRecord>, MupcError> {
        self.query_events_by_type_sync(fault_type).map_err(|e| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                e.to_string(),
                "data-processing",
            )
        })
    }

    async fn query_events_by_time(
        &self,
        start: i64,
        end: i64,
    ) -> Result<Vec<FaultRecord>, MupcError> {
        self.query_events_by_time_sync(start, end).map_err(|e| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                e.to_string(),
                "data-processing",
            )
        })
    }

    async fn get_waveform_by_id(&self, event_id: i64) -> Result<WaveformData, MupcError> {
        self.get_waveform_by_id_sync(event_id).map_err(|e| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                e.to_string(),
                "data-processing",
            )
        })
    }

    async fn get_waveform_summary(&self, event_id: i64) -> Result<WaveformSummary, MupcError> {
        self.get_waveform_summary_sync(event_id).map_err(|e| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                e.to_string(),
                "data-processing",
            )
        })
    }

    async fn export_comtrade(
        &self,
        event_id: i64,
        output_dir: &Path,
    ) -> Result<ExportResult, MupcError> {
        self.export_comtrade_sync(event_id, output_dir)
            .map_err(|e| {
                MupcError::new(
                    mupc_common::ErrorCode::Unknown,
                    e.to_string(),
                    "data-processing",
                )
            })
    }

    async fn export_csv(
        &self,
        event_id: i64,
        output_dir: &Path,
    ) -> Result<ExportResult, MupcError> {
        self.export_csv_sync(event_id, output_dir).map_err(|e| {
            MupcError::new(
                mupc_common::ErrorCode::Unknown,
                e.to_string(),
                "data-processing",
            )
        })
    }
}
