//! rs485-plugin 单元测试

use rs485_plugin::{Config, CrcMode};
use device_trait::{DeviceStatus, DeviceType, Parity};

#[test]
fn test_config_default() {
    let config = Config::default();
    assert_eq!(config.port, "/dev/ttyUSB0");
    assert_eq!(config.baud_rate, 9600);
    assert_eq!(config.data_bits, 8);
    assert_eq!(config.stop_bits, 1);
    assert_eq!(config.parity, Parity::None);
    assert_eq!(config.timeout_ms, 1000);
    assert_eq!(config.device_addr, 0x01);
    assert_eq!(config.crc_mode, CrcMode::Crc16Modbus);
}

#[test]
fn test_config_validation_valid() {
    let config = Config::default();
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_invalid_baud_rate() {
    let mut config = Config::default();
    config.baud_rate = 0;
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_invalid_data_bits() {
    let mut config = Config::default();
    config.data_bits = 9;
    assert!(config.validate().is_err());
}

#[test]
fn test_config_from_json() {
    let json = r#"{
        "port": "/dev/ttyUSB1",
        "baud_rate": 19200,
        "data_bits": 8,
        "stop_bits": 1,
        "parity": "even",
        "timeout_ms": 2000,
        "device_addr": 2,
        "crc_mode": "crc16_modbus"
    }"#;
    let config = Config::from_json(json).unwrap();
    assert_eq!(config.port, "/dev/ttyUSB1");
    assert_eq!(config.baud_rate, 19200);
    assert_eq!(config.device_addr, 2);
    assert_eq!(config.parity, Parity::Even);
}

#[test]
fn test_crc_mode_as_str() {
    assert_eq!(format!("{:?}", CrcMode::None), "None");
    assert_eq!(format!("{:?}", CrcMode::Crc16Modbus), "Crc16Modbus");
    assert_eq!(format!("{:?}", CrcMode::Crc16Xmodem), "Crc16Xmodem");
    assert_eq!(format!("{:?}", CrcMode::Crc8), "Crc8");
}

#[test]
fn test_parity_from_str() {
    assert_eq!(Parity::None.as_str(), "none");
    assert_eq!(Parity::Even.as_str(), "even");
    assert_eq!(Parity::Odd.as_str(), "odd");
}

#[test]
fn test_device_status_is_online() {
    assert!(DeviceStatus::Online.is_online());
    assert!(!DeviceStatus::Offline.is_online());
    assert!(!DeviceStatus::Error("test".to_string()).is_online());
}

#[test]
fn test_device_type_conversion() {
    assert_eq!(DeviceType::from_str("ttu"), DeviceType::Ttu);
    assert_eq!(DeviceType::from_str("inverter"), DeviceType::Inverter);
    assert_eq!(DeviceType::from_str("charger"), DeviceType::Charger);
    assert_eq!(DeviceType::from_str("unknown"), DeviceType::Unknown);
}

#[test]
fn test_device_type_as_str() {
    assert_eq!(DeviceType::Ttu.as_str(), "ttu");
    assert_eq!(DeviceType::Inverter.as_str(), "inverter");
    assert_eq!(DeviceType::Charger.as_str(), "charger");
}