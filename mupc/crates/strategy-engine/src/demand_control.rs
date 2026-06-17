use crate::config::DemandControlConfig;
use crate::strategies::{CommandType, ControlCommand, FallbackStrategy, StrategyType};
use async_trait::async_trait;
use mupc_common::MupcError;
use mupc_data_processing::telemetry::DataPackage;

/// 需量控制策略（v2.15: 本地策略独立执行，非 AI 动作维度）
pub struct DemandControlStrategy {
    config: DemandControlConfig,
}

impl DemandControlStrategy {
    pub fn new(config: DemandControlConfig) -> Self {
        Self { config }
    }

    pub fn evaluate_sync(&self, data: &DataPackage) -> ControlCommand {
        let transformer_load = self.get_transformer_load(data);
        let battery_soc = data.battery.soc.unwrap_or(50.0);

        let (p_batt, load_shedding, level) = self.decide(transformer_load, battery_soc);

        ControlCommand {
            cmd_id: 2,
            cmd_type: if load_shedding > 0.0 {
                CommandType::SwitchControl
            } else {
                CommandType::PowerRegulation
            },
            p_batt_set: Some(p_batt),
            p_ref: None,
            k_droop: None,
            q_batt_set: None,
            phase_compensation: None,
            start_stop: Some(true),
            priority: if level >= 3 { 3 } else { level },
            pv_limit: None,
            load_shedding: if load_shedding > 0.0 {
                Some(load_shedding)
            } else {
                None
            },
        }
    }

    pub(crate) fn get_transformer_load(&self, data: &DataPackage) -> f64 {
        let load_power = data.device_status.load_power.unwrap_or(0.0);
        let ev_power = data.device_status.ev_charger_power.unwrap_or(0.0);
        (load_power + ev_power) / self.config.transformer_capacity
    }

    fn decide(&self, transformer_load: f64, battery_soc: f64) -> (f64, f64, u8) {
        let level: u8;
        let mut p_batt: f64;
        let load_shedding: f64;

        if transformer_load > self.config.emergency_threshold {
            level = 3;
            p_batt = -30.0;
            load_shedding = 20.0;
        } else if transformer_load > self.config.action_threshold {
            level = 2;
            p_batt = -20.0;
            load_shedding = 10.0;
        } else if transformer_load > self.config.warning_threshold {
            level = 1;
            p_batt = -10.0;
            load_shedding = 0.0;
        } else {
            level = 0;
            p_batt = 0.0;
            load_shedding = 0.0;
        }

        if battery_soc < 20.0 && p_batt < 0.0 {
            p_batt = p_batt.max(-10.0);
        }

        (p_batt, load_shedding, level)
    }
}

#[async_trait]
impl FallbackStrategy for DemandControlStrategy {
    async fn evaluate(&self, data: &DataPackage) -> Result<ControlCommand, MupcError> {
        Ok(self.evaluate_sync(data))
    }

    fn strategy_type(&self) -> StrategyType {
        StrategyType::Fallback
    }

    fn name(&self) -> &str {
        "DemandControlStrategy"
    }
}
