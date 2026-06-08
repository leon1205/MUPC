#[test]
fn test_error_display() {
    use mupc_data_processing::errors::DataProcessingError;

    let err = DataProcessingError::CollectionFailed("timeout".to_string());
    assert_eq!(err.to_string(), "数据采集失败: timeout");
}
