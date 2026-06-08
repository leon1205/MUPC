#[tokio::test]
async fn test_collector_start_stop() {
    use mupc_data_processing::{DataCollector, DataCollectorImpl};

    let mut collector = DataCollectorImpl::new();
    // 初始状态应能启动
    assert!(collector.start().await.is_ok());
    // 重复启动应不报错
    assert!(collector.start().await.is_ok());
    // 停止
    assert!(collector.stop().await.is_ok());
}

#[tokio::test]
async fn test_collector_default() {
    use mupc_data_processing::{DataCollector, DataCollectorImpl};

    let mut collector = DataCollectorImpl::default();
    assert!(collector.start().await.is_ok());
}

#[test]
fn test_collector_get_latest_data() {
    use mupc_data_processing::{DataCollector, DataCollectorImpl};

    let collector = DataCollectorImpl::new();
    // 初始状态为 None
    assert!(collector.get_latest_data().is_none());
}
