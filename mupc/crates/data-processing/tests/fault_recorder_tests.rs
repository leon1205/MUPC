#[test]
fn test_fault_recorder_in_memory() {
    use mupc_data_processing::fault_recorder_impl::{FaultRecorderImpl, FaultType};
    use mupc_data_processing::telemetry::FaultCondition;

    let recorder = FaultRecorderImpl::new_in_memory().expect("failed to create in-memory recorder");
    assert!(!recorder.is_recording());

    let condition = FaultCondition {
        over_voltage: Some(425.0),
        under_voltage: None,
        over_current: None,
        frequency_abnormal: None,
    };

    recorder.trigger_sync(&condition).expect("trigger should succeed");

    let now = chrono::Utc::now().timestamp();
    let records = recorder.query_sync(now - 10, now + 10).expect("query should succeed");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].fault_type, FaultType::GridOverload.as_str());
}

#[test]
fn test_fault_type_determination() {
    use mupc_data_processing::fault_recorder_impl::FaultRecorderImpl;

    let recorder = FaultRecorderImpl::new_in_memory().expect("failed to create in-memory recorder");

    // Test over voltage -> GridOverload
    let condition = FaultCondition {
        over_voltage: Some(425.0),
        under_voltage: None,
        over_current: None,
        frequency_abnormal: None,
    };
    recorder.trigger_sync(&condition).expect("trigger should succeed");

    // Test over current -> BatteryOverTemp
    let condition2 = FaultCondition {
        over_voltage: None,
        under_voltage: None,
        over_current: Some(160.0),
        frequency_abnormal: None,
    };
    recorder.trigger_sync(&condition2).expect("trigger should succeed");

    let now = chrono::Utc::now().timestamp();
    let records = recorder.query_sync(now - 10, now + 10).expect("query should succeed");
    assert_eq!(records.len(), 2);
}

#[test]
fn test_query_no_results() {
    use mupc_data_processing::fault_recorder_impl::FaultRecorderImpl;
    use mupc_data_processing::telemetry::FaultCondition;

    let recorder = FaultRecorderImpl::new_in_memory().expect("failed to create in-memory recorder");

    let condition = FaultCondition {
        over_voltage: Some(300.0),
        under_voltage: None,
        over_current: None,
        frequency_abnormal: None,
    };
    recorder.trigger_sync(&condition).expect("trigger should succeed");

    // Query outside the range should return empty
    let records = recorder.query_sync(0, 100).expect("query should succeed");
    assert_eq!(records.len(), 0);
}

#[test]
fn test_fault_type_enum() {
    use mupc_data_processing::fault_recorder_impl::FaultType;

    assert_eq!(FaultType::BatteryOverTemp.as_str(), "BATTERY_OVER_TEMP");
    assert_eq!(FaultType::BatteryUnderSoc.as_str(), "BATTERY_UNDER_SOC");
    assert_eq!(FaultType::GridOverload.as_str(), "GRID_OVERLOAD");
    assert_eq!(FaultType::GridReverse.as_str(), "GRID_REVERSE");
    assert_eq!(FaultType::PvOutputLimit.as_str(), "PV_OUTPUT_LIMIT");
    assert_eq!(FaultType::Unknown.as_str(), "UNKNOWN");
}