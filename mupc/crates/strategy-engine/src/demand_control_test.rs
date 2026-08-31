//! 需量控制策略测试

use crate::config::DemandControlConfig;
use crate::demand_control::DemandControlStrategy;
use crate::strategies::CommandType;
use mupc_data_processing::telemetry::{
    BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus,
};

/// 创建测试用电数据包
fn create_test_data(
    timestamp: u64,
    battery_soc: f64,
    load_power: f64,
    ev_power: f64,
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
            pv_power: Some(10.0),
            load_power: Some(load_power),
            ev_charger_power: Some(ev_power),
        },
        timestamp,
    }
}

/// 计算小时时间戳
fn hour_ts(hour: u64) -> u64 {
    hour * 3600
}

#[test]
fn test_transformer_load_calculation() {
    let config = DemandControlConfig::default();
    let strategy = DemandControlStrategy::new(config);

    // 负载 400kW, EV 0kW, 变压器容量 500kVA
    // 负载率 = 400 / 500 = 0.8 (80%)
    let data = create_test_data(hour_ts(10), 50.0, 400.0, 0.0);
    let load = strategy.get_transformer_load(&data);
    assert!((load - 0.8).abs() < f64::EPSILON);
}

#[test]
fn test_level_0_normal() {
    // 负载率 < 80%, 应待机
    let config = DemandControlConfig::default();
    let strategy = DemandControlStrategy::new(config);

    // 负载 300kW + EV 50kW = 350kW, 负载率 = 350/500 = 70%
    let data = create_test_data(hour_ts(10), 50.0, 300.0, 50.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(0.0));
    assert_eq!(cmd.cmd_type, CommandType::PowerRegulation);
    assert_eq!(cmd.priority, 0);
}

#[test]
fn test_level_1_warning() {
    // 负载率 80% < x <= 90%, 应电池放电补偿 (-10kW)
    let config = DemandControlConfig::default();
    let strategy = DemandControlStrategy::new(config);

    // 负载 400kW + EV 50kW = 450kW, 负载率 = 450/500 = 90%
    let data = create_test_data(hour_ts(10), 50.0, 400.0, 50.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(-10.0));
    assert_eq!(cmd.cmd_type, CommandType::PowerRegulation);
    assert_eq!(cmd.priority, 1);
}

#[test]
fn test_level_2_action() {
    // 负载率 90% < x <= 95%, 应电池放电 + 负荷切除
    let config = DemandControlConfig::default();
    let strategy = DemandControlStrategy::new(config);

    // 负载 420kW + EV 50kW = 470kW, 负载率 = 470/500 = 94%
    let data = create_test_data(hour_ts(10), 50.0, 420.0, 50.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(-20.0));
    assert_eq!(cmd.cmd_type, CommandType::SwitchControl);
    assert_eq!(cmd.priority, 2);
}

#[test]
fn test_level_3_emergency() {
    // 负载率 > 95%, 紧急放电 + 强制负荷切除
    let config = DemandControlConfig::default();
    let strategy = DemandControlStrategy::new(config);

    // 负载 450kW + EV 50kW = 500kW, 负载率 = 500/500 = 100%
    let data = create_test_data(hour_ts(10), 50.0, 450.0, 50.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(-30.0));
    assert_eq!(cmd.cmd_type, CommandType::SwitchControl);
    assert_eq!(cmd.priority, 3);
}

#[test]
fn test_low_soc_protection() {
    // SOC < 20% 时, 放电功率受限
    let config = DemandControlConfig::default();
    let strategy = DemandControlStrategy::new(config);

    // 负载 450kW + EV 50kW = 500kW, 负载率 = 100%, 但 SOC = 15%
    let data = create_test_data(hour_ts(10), 15.0, 450.0, 50.0);

    let cmd = strategy.evaluate_sync(&data);

    // Level 3 原本 p_batt = -30, 但 SOC < 20% 限制为 -10
    assert_eq!(cmd.p_batt_set, Some(-10.0));
}

#[test]
fn test_custom_thresholds() {
    // 自定义阈值
    let config = DemandControlConfig {
        transformer_capacity: 1000.0,
        demand_factor: 0.85,
        warning_threshold: 0.70,
        action_threshold: 0.80,
        emergency_threshold: 0.90,
    };
    let strategy = DemandControlStrategy::new(config);

    // 负载 750kW, 负载率 = 75%, 应触发 Level 1
    let data = create_test_data(hour_ts(10), 50.0, 750.0, 0.0);

    let cmd = strategy.evaluate_sync(&data);

    assert_eq!(cmd.p_batt_set, Some(-10.0));
    assert_eq!(cmd.priority, 1);
}
