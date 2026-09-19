use mupc_data_processing::fault_recorder_impl::{FaultRecorderImpl, FaultType};
use mupc_data_processing::recorder::FaultRecorder;
use mupc_data_processing::telemetry::FaultCondition;
use std::path::PathBuf;

/// 每个用例**各自一个库文件**。
///
/// ⚠️ 原版只按进程号命名 ⇒ 同一进程内并行运行的多个用例共用同一个 SQLite 文件，
/// 相互抢锁 ⇒ 间歇 `DatabaseError("database is locked")`（2026-09-19 实测复现，
/// 与 `test_fault_recorder_new` 并发时必现）。按用例名再加一段后缀即彻底隔离。
fn create_temp_db(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mupc_test_{}_{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("test_faults.db")
}

#[test]
fn test_fault_type_enum() {
    assert_eq!(FaultType::BatteryOverTemp.as_str(), "BATTERY_OVER_TEMP");
    assert_eq!(FaultType::BatteryUnderSoc.as_str(), "BATTERY_UNDER_SOC");
    assert_eq!(FaultType::GridOverload.as_str(), "GRID_OVERLOAD");
    assert_eq!(FaultType::GridReverse.as_str(), "GRID_REVERSE");
    assert_eq!(FaultType::PvOutputLimit.as_str(), "PV_OUTPUT_LIMIT");
    assert_eq!(FaultType::Unknown.as_str(), "UNKNOWN");
}

#[test]
fn test_fault_recorder_new() {
    let db_path = create_temp_db("new");
    let recorder = FaultRecorderImpl::new(&db_path);
    assert!(recorder.is_ok());
    let recorder = recorder.unwrap();
    assert!(!recorder.is_recording());
}

#[tokio::test]
async fn test_fault_recorder_record_and_query() {
    let db_path = create_temp_db("record_and_query");
    let recorder = FaultRecorderImpl::new(&db_path).unwrap();

    let condition = FaultCondition {
        over_voltage: Some(425.0),
        under_voltage: None,
        over_current: None,
        frequency_abnormal: None,
    };

    // Record a fault
    assert!(recorder.record(&condition).await.is_ok());

    // Query by time range
    let now = chrono::Utc::now().timestamp_millis();
    let records = recorder.query(now - 10000, now + 10000).await;
    assert!(records.is_ok());
    assert_eq!(records.unwrap().len(), 1);
}
