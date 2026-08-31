//! 削峰填谷策略测试

use crate::config::PeakShavingConfig;
use crate::peak_shaving::PeakShavingStrategy;
use crate::strategies::CommandType;
use mupc_data_processing::telemetry::{
    BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus,
};

/// 创建测试用电数据包
fn create_test_data(
    timestamp: u64,
    battery_soc: f64,
    pv_power: f64,
    load_power: f64,
) -> DataPackage {
    DataPackage {
        electrical: ElectricalData {
            voltage: Some(220.0),
            current: Some(10.0),
            active_power: Some(5.0),
            reactive_power: Some(1.0),
            cos_phi: Some(0.95),
            frequency: Some(50.0),
            phase: None,
        },
        battery: BatteryData {
            soc: Some(battery_soc),
            soh: Some(95.0),
            temperature: Some(25.0),
        },
        device_status: DeviceStatus {
            inverter_status: InverterStatus::Running,
            pv_power: Some(pv_power),
            load_power: Some(load_power),
            ev_charger_power: Some(0.0),
        },
        timestamp,
    }
}

/// 计算小时时间戳
fn hour_ts(hour: u64) -> u64 {
    hour * 3600
}

#[test]
fn test_peak_hours_detection() {
    let config = PeakShavingConfig::default();
    let strategy = PeakShavingStrategy::new(config);

    // 测试峰时段: 08:00-11:00, 18:00-21:00
    assert!(strategy.is_peak_hour(8));
    assert!(strategy.is_peak_hour(10));
    assert!(strategy.is_peak_hour(18));
    assert!(strategy.is_peak_hour(20));
    assert!(!strategy.is_peak_hour(12));
    assert!(!strategy.is_peak_hour(22));
}

#[test]
fn test_valley_hours_detection() {
    let config = PeakShavingConfig::default();
    let strategy = PeakShavingStrategy::new(config);

    // 测试谷时段: 23:00-07:00
    assert!(strategy.is_valley_hour(0));
    assert!(strategy.is_valley_hour(3));
    assert!(strategy.is_valley_hour(6));
    assert!(strategy.is_valley_hour(23));
    assert!(!strategy.is_valley_hour(8));
    assert!(!strategy.is_valley_hour(12));
}

#[test]
fn test_discharge_at_peak_when_soc_high() {
    // 峰时段, SOC > 80%, 应放电
    let strategy = PeakShavingStrategy::new(PeakShavingConfig::default());
    let data = create_test_data(hour_ts(10), 85.0, 5.0, 10.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(-20.0));
    assert_eq!(cmd.cmd_type, CommandType::ChargeDischarge);
}

#[test]
fn test_charge_at_valley_when_soc_low() {
    // 谷时段, SOC < 20%, 应充电
    let strategy = PeakShavingStrategy::new(PeakShavingConfig::default());
    let data = create_test_data(hour_ts(2), 15.0, 5.0, 10.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(20.0));
    assert_eq!(cmd.cmd_type, CommandType::ChargeDischarge);
}

#[test]
fn test_charge_at_valley_with_pv() {
    // 谷时段, 光伏充足, 按光伏功率充电
    let strategy = PeakShavingStrategy::new(PeakShavingConfig::default());
    let data = create_test_data(hour_ts(2), 50.0, 25.0, 5.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(25.0)); // min(25, 30) = 25
    assert_eq!(cmd.cmd_type, CommandType::ChargeDischarge);
}

#[test]
fn test_discharge_at_peak() {
    // 峰时段, SOC 正常, 应放电
    let strategy = PeakShavingStrategy::new(PeakShavingConfig::default());
    let data = create_test_data(hour_ts(10), 50.0, 5.0, 10.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(-25.0));
    assert_eq!(cmd.cmd_type, CommandType::ChargeDischarge);
}

#[test]
fn test_idle_at_normal_hours() {
    // 正常时段(非峰非谷), 应待机
    let strategy = PeakShavingStrategy::new(PeakShavingConfig::default());
    let data = create_test_data(hour_ts(14), 50.0, 5.0, 10.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(0.0));
    assert_eq!(cmd.cmd_type, CommandType::PowerRegulation);
}

#[test]
fn test_soc_too_low_force_charge() {
    // SOC 低于下限, 强制充电
    let strategy = PeakShavingStrategy::new(PeakShavingConfig::default());
    let data = create_test_data(hour_ts(14), 10.0, 5.0, 10.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(20.0));
    assert_eq!(cmd.cmd_type, CommandType::ChargeDischarge);
}

#[test]
fn test_soc_too_high_force_discharge() {
    // SOC 高于上限, 强制放电
    let strategy = PeakShavingStrategy::new(PeakShavingConfig::default());
    let data = create_test_data(hour_ts(14), 90.0, 5.0, 10.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(-20.0));
    assert_eq!(cmd.cmd_type, CommandType::ChargeDischarge);
}
