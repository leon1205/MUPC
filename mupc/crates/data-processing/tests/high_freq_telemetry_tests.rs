#[test]
fn test_telemetry_impl_new() {
    use mupc_data_processing::HighFreqTelemetryImpl;

    let telemetry = HighFreqTelemetryImpl::new(1000);
    assert_eq!(telemetry.period(), 1000);
    assert!(!telemetry.is_running());
}

#[test]
fn test_telemetry_start_stop() {
    use mupc_data_processing::HighFreqTelemetryImpl;

    let telemetry = HighFreqTelemetryImpl::new(1000);
    assert!(!telemetry.is_running());

    // Start telemetry
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        telemetry.start().await.unwrap();
    });
    assert!(telemetry.is_running());

    // Stop telemetry
    rt.block_on(async {
        telemetry.stop().await.unwrap();
    });
    assert!(!telemetry.is_running());
}

#[test]
fn test_telemetry_period() {
    use mupc_data_processing::HighFreqTelemetryImpl;

    let telemetry = HighFreqTelemetryImpl::new(500);
    assert_eq!(telemetry.period(), 500);

    let telemetry2 = HighFreqTelemetryImpl::new(2000);
    assert_eq!(telemetry2.period(), 2000);
}

#[test]
fn test_telemetry_get_current_value_initial() {
    use mupc_data_processing::HighFreqTelemetryImpl;

    let telemetry = HighFreqTelemetryImpl::new(1000);
    // 初始状态，buffer为空，应返回None
    assert!(telemetry.get_current_value("battery_soc").is_none());
    assert!(telemetry.get_current_value("battery_power").is_none());
    assert!(telemetry.get_current_value("pv_output").is_none());
    assert!(telemetry.get_current_value("load_power").is_none());
    assert!(telemetry.get_current_value("grid_power").is_none());
    assert!(telemetry.get_current_value("transformer_load").is_none());
    // 未知字段
    assert!(telemetry.get_current_value("unknown").is_none());
}