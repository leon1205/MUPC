#[test]
fn test_collector_name() {
    use mupc_data_processing::DataCollectorImpl;

    let collector = DataCollectorImpl::new();
    assert_eq!(collector.name(), "DataCollectorImpl");
}

#[test]
fn test_collector_default() {
    use mupc_data_processing::DataCollectorImpl;

    let collector = DataCollectorImpl::default();
    assert_eq!(collector.name(), "DataCollectorImpl");
}

#[test]
fn test_collector_get_latest_data() {
    use mupc_data_processing::DataCollectorImpl;

    let collector = DataCollectorImpl::new();
    // 初始状态为 None
    assert!(collector.get_latest_data().is_none());
}