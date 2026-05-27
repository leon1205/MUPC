//! IEC 61850 插件集成测试

use mupc_iec61850_plugin::{Iec61850Config, Iec61850DeviceImpl, GooseConfig};

#[tokio::test]
async fn test_iec61850_config_default() {
    let config = Iec61850Config::default();
    assert_eq!(config.local_port, 102);
    assert_eq!(config.remote_port, 102);
}

#[tokio::test]
async fn test_device_creation() {
    let config = Iec61850Config::default();
    let device = Iec61850DeviceImpl::new("test_ied_001".to_string(), config);
    assert_eq!(device.device_id(), "test_ied_001");
    assert_eq!(device.device_type(), "IEC61850");
}

#[test]
fn test_goose_config() {
    let config = GooseConfig::default();
    assert_eq!(config.go_id, "GOOSE1");
}