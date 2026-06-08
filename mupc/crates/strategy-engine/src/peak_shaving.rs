use crate::config::PeakShavingConfig;
use crate::strategies::{CommandType, ControlCommand, FallbackStrategy, StrategyType};
use async_trait::async_trait;
use mupc_data_processing::telemetry::DataPackage;

/// 削峰填谷策略
pub struct PeakShavingStrategy {
    config: PeakShavingConfig,
}

impl PeakShavingStrategy {
    pub fn new(config: PeakShavingConfig) -> Self {
        Self { config }
    }

    /// 同步评估（用于测试）
    pub fn evaluate_sync(&self, data: &DataPackage) -> ControlCommand {
        let hour = (data.timestamp % 86400) / 3600;

        let is_peak = self.is_peak_hour(hour as u8);
        let is_valley = self.is_valley_hour(hour as u8);

        let battery_soc = data.battery.soc.unwrap_or(50.0);
        let pv_power = data.device_status.pv_power.unwrap_or(0.0);
        let load_power = data.device_status.load_power.unwrap_or(0.0);

        let (p_batt, cmd_type) = self.decide(battery_soc, pv_power, load_power, is_peak, is_valley);

        ControlCommand {
            cmd_id: 1,
            cmd_type,
            p_batt_set: Some(p_batt),
            q_batt_set: None,
            phase_compensation: None,
            start_stop: Some(true),
            priority: 1,
            pv_limit: None,
            load_shedding: None,
        }
    }

    pub(crate) fn is_peak_hour(&self, hour: u8) -> bool {
        self.config.peak_hours.iter().any(|(start, end)| {
            if *start <= *end {
                hour >= *start && hour < *end
            } else {
                hour >= *start || hour < *end
            }
        })
    }

    pub(crate) fn is_valley_hour(&self, hour: u8) -> bool {
        self.config.valley_hours.iter().any(|(start, end)| {
            if *start <= *end {
                hour >= *start && hour < *end
            } else {
                hour >= *start || hour < *end
            }
        })
    }

    fn decide(
        &self,
        battery_soc: f64,
        pv_power: f64,
        _load_power: f64,
        is_peak: bool,
        is_valley: bool,
    ) -> (f64, CommandType) {
        let p_batt: f64;
        let cmd_type: CommandType;

        if battery_soc < self.config.soc_charge_min {
            p_batt = 20.0;
            cmd_type = CommandType::ChargeDischarge;
        } else if battery_soc > self.config.soc_charge_max {
            p_batt = -20.0;
            cmd_type = CommandType::ChargeDischarge;
        } else if is_valley {
            if pv_power > 10.0 {
                p_batt = pv_power.min(30.0);
            } else {
                p_batt = 15.0;
            }
            cmd_type = CommandType::ChargeDischarge;
        } else if is_peak {
            p_batt = -25.0;
            cmd_type = CommandType::ChargeDischarge;
        } else {
            p_batt = 0.0;
            cmd_type = CommandType::PowerRegulation;
        }

        (p_batt, cmd_type)
    }
}

#[async_trait]
impl FallbackStrategy for PeakShavingStrategy {
    async fn evaluate(&self, data: &DataPackage) -> Result<ControlCommand, mupc_common::MupcError> {
        Ok(self.evaluate_sync(data))
    }

    fn strategy_type(&self) -> StrategyType {
        StrategyType::Fallback
    }

    fn name(&self) -> &str {
        "PeakShavingStrategy"
    }
}
