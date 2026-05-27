use crate::errors::DataProcessingError;
use crate::recorder::FaultRecorder;
use crate::telemetry::{FaultCondition, WaveformData};
use async_trait::async_trait;
use chrono::Utc;
use mupc_common::MupcError;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

/// 故障类型枚举
#[derive(Debug, Clone, PartialEq)]
pub enum FaultType {
    BatteryOverTemp,
    BatteryUnderSoc,
    GridOverload,
    GridReverse,
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

        conn.execute(
            "CREATE TABLE IF NOT EXISTS fault_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                fault_type TEXT NOT NULL,
                trigger_time INTEGER NOT NULL,
                over_voltage REAL,
                under_voltage REAL,
                over_current REAL,
                frequency_abnormal REAL
            )",
            [],
        ).map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_trigger_time ON fault_records(trigger_time)",
            [],
        ).map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
            recording: Mutex::new(false),
        })
    }

    pub fn new_in_memory() -> Result<Self, DataProcessingError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        conn.execute(
            "CREATE TABLE fault_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                fault_type TEXT NOT NULL,
                trigger_time INTEGER NOT NULL,
                over_voltage REAL,
                under_voltage REAL,
                over_current REAL,
                frequency_abnormal REAL
            )",
            [],
        ).map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        Ok(Self {
            conn: Mutex::new(conn),
            recording: Mutex::new(false),
        })
    }

    pub fn trigger_sync(&self, condition: &FaultCondition) -> Result<(), DataProcessingError> {
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
            return FaultType::GridOverload;
        }
        if condition.over_current.is_some() && condition.over_current.unwrap() > 150.0 {
            return FaultType::BatteryOverTemp;
        }
        if condition.under_voltage.is_some() && condition.under_voltage.unwrap() < 200.0 {
            return FaultType::BatteryUnderSoc;
        }
        if condition.frequency_abnormal.is_some() {
            let freq = condition.frequency_abnormal.unwrap();
            if freq > 50.5 || freq < 49.5 {
                return FaultType::GridReverse;
            }
        }
        FaultType::Unknown
    }

    pub fn query_sync(&self, start: i64, end: i64) -> Result<Vec<FaultRecord>, DataProcessingError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, fault_type, trigger_time, over_voltage, under_voltage, over_current, frequency_abnormal
             FROM fault_records WHERE trigger_time BETWEEN ?1 AND ?2"
        ).map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        let records = stmt.query_map([start, end], |row| {
            Ok(FaultRecord {
                id: row.get(0)?,
                fault_type: row.get(1)?,
                trigger_time: row.get(2)?,
                over_voltage: row.get(3)?,
                under_voltage: row.get(4)?,
                over_current: row.get(5)?,
                frequency_abnormal: row.get(6)?,
            })
        }).map_err(|e| DataProcessingError::DatabaseError(e.to_string()))?;

        records.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DataProcessingError::DatabaseError(e.to_string()))
    }
}

#[async_trait]
impl FaultRecorder for FaultRecorderImpl {
    async fn trigger(&self, condition: &FaultCondition) -> Result<(), MupcError> {
        self.trigger_sync(condition)
            .map_err(|e| MupcError::new(mupc_common::ErrorCode::InternalError, &e.to_string()))
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
}