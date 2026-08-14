use std::sync::atomic::{AtomicU8, Ordering};

use crate::config::AntiReverseConfig;
use crate::strategies::{CommandType, ControlCommand, FallbackStrategy, StrategyType};
use async_trait::async_trait;
use mupc_common::MupcError;
use mupc_data_processing::telemetry::DataPackage;

/// 防逆流策略
pub struct AntiReverseStrategy {
    config: AntiReverseConfig,
    pv_limit_count: AtomicU8,
}

impl AntiReverseStrategy {
    pub fn new(config: AntiReverseConfig) -> Self {
        Self {
            config,
            pv_limit_count: AtomicU8::new(0),
        }
    }

    pub fn evaluate_sync(&self, data: &DataPackage) -> ControlCommand {
        let grid_power = data.electrical.active_power.unwrap_or(0.0);
        let pv_power = data.device_status.pv_power.unwrap_or(0.0);
        let battery_soc = data.battery.soc.unwrap_or(50.0);

        let (p_batt, pv_limit) = self.decide(grid_power, pv_power, battery_soc);

        ControlCommand {
            cmd_id: 3,
            cmd_type: CommandType::PowerRegulation,
            p_batt_set: Some(p_batt),
            q_batt_set: None,
            phase_compensation: None,
            start_stop: Some(true),
            priority: 2,
            pv_limit: if pv_limit > 0.0 { Some(pv_limit) } else { None },
            load_shedding: None,
        }
    }

    fn decide(&self, grid_power: f64, pv_power: f64, battery_soc: f64) -> (f64, f64) {
        if grid_power < self.config.reverse_power_threshold {
            if battery_soc < self.config.soc_charge_max {
                self.pv_limit_count.store(0, Ordering::SeqCst);
                ((pv_power * 0.8).min(self.config.max_charge_power), 0.0)
            } else {
                let count = self.pv_limit_count.fetch_add(1, Ordering::SeqCst) + 1;
                let pv_limit = pv_power * ((count as f64) * 0.1).min(0.5);
                (0.0, pv_limit)
            }
        } else {
            self.pv_limit_count.store(0, Ordering::SeqCst);
            (0.0, 0.0)
        }
    }
}

#[async_trait]
impl FallbackStrategy for AntiReverseStrategy {
    async fn evaluate(&self, data: &DataPackage) -> Result<ControlCommand, MupcError> {
        Ok(self.evaluate_sync(data))
    }

    fn strategy_type(&self) -> StrategyType {
        StrategyType::Fallback
    }

    fn name(&self) -> &str {
        "AntiReverseStrategy"
    }
}
