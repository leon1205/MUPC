#[cfg(test)]
mod anti_reverse_test {
    use crate::anti_reverse::AntiReverseStrategy;
    use crate::config::AntiReverseConfig;
    use crate::strategies::{CommandType, FallbackStrategy};
    use mupc_data_processing::telemetry::{
        BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus,
    };

    fn create_test_data(grid_power: f64, pv_power: f64, battery_soc: f64) -> DataPackage {
        DataPackage {
            timestamp: 3600 * 8,
            electrical: ElectricalData {
                voltage: Some(220.0),
                current: Some(50.0),
                active_power: Some(grid_power),
                reactive_power: Some(0.0),
                cos_phi: None,
                frequency: Some(50.0),
            },
            device_status: DeviceStatus {
                inverter_status: InverterStatus::Running,
                pv_power: Some(pv_power),
                load_power: Some(10.0),
                ev_charger_power: None,
            },
            battery: BatteryData {
                soc: Some(battery_soc),
                soh: None,
                temperature: Some(25.0),
            },
        }
    }

    #[test]
    fn test_anti_reverse_charge_when_grid_reverse_and_battery_not_full() {
        let config = AntiReverseConfig::default();
        let mut strategy = AntiReverseStrategy::new(config);

        let data = create_test_data(-5.0, 20.0, 50.0);
        let cmd = strategy.evaluate_sync(&data);

        assert_eq!(cmd.cmd_id, 3);
        assert_eq!(cmd.cmd_type, CommandType::PowerRegulation);
        assert!(cmd.p_batt_set.is_some());
        assert!(cmd.p_batt_set.unwrap() > 0.0);
        assert_eq!(cmd.priority, 2);
    }

    #[test]
    fn test_anti_reverse_limit_pv_when_battery_full() {
        let config = AntiReverseConfig::default();
        let mut strategy = AntiReverseStrategy::new(config);

        let data = create_test_data(-5.0, 20.0, 90.0);
        let cmd = strategy.evaluate_sync(&data);

        assert_eq!(cmd.cmd_id, 3);
        assert_eq!(cmd.cmd_type, CommandType::PowerRegulation);
        assert_eq!(cmd.p_batt_set, Some(0.0));
        assert!(cmd.start_stop == Some(true));
    }

    #[test]
    fn test_anti_reverse_no_action_when_grid_normal() {
        let config = AntiReverseConfig::default();
        let mut strategy = AntiReverseStrategy::new(config);

        let data = create_test_data(10.0, 20.0, 50.0);
        let cmd = strategy.evaluate_sync(&data);

        assert_eq!(cmd.cmd_id, 3);
        assert_eq!(cmd.p_batt_set, Some(0.0));
        assert_eq!(cmd.priority, 2);
    }

    #[test]
    fn test_strategy_type() {
        let config = AntiReverseConfig::default();
        let strategy = AntiReverseStrategy::new(config);

        assert_eq!(
            strategy.strategy_type(),
            crate::strategies::StrategyType::Fallback
        );
    }

    #[test]
    fn test_strategy_name() {
        let config = AntiReverseConfig::default();
        let strategy = AntiReverseStrategy::new(config);

        assert_eq!(strategy.name(), "AntiReverseStrategy");
    }
}
