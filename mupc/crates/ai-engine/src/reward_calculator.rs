//! 奖励函数计算模块
//!
//! 根据当前运行场景选择对应奖励函数计算奖励值，
//! 用于在线微调的模型权重更新和 Web UI 决策质量展示。
//!
//! 5 种场景奖励函数：
//! - MODE-01 农网灌溉: R = w1*R_pv - w2*P_batt_deg - w3*P_trafo
//! - MODE-02 自主套利: R = w1*R_price - w2*P_batt_deg
//! - MODE-03 需量控制: R = w1*R_demand - w2*P_comfort
//! - MODE-04 虚拟电厂: R = w1*R_ancillary + w2*R_accuracy - w3*P_deadline
//! - MODE-05 极致绿色: R = w1*R_green + w2*R_carbon

use crate::config::SceneWeights;
use crate::data_fusion::FusedSystemState;
use crate::mode_selector::RunningMode;
use crate::rl_model::ActionOutput;

/// 奖励函数计算器
pub struct RewardCalculator {
    weights: SceneWeights,
    carbon_emission_factor: f64,
    demand_penalty_rate: f64,
    battery_degradation_alpha: f64,
}

impl RewardCalculator {
    pub fn new(weights: SceneWeights) -> Self {
        Self {
            weights,
            carbon_emission_factor: 0.581,
            demand_penalty_rate: 50.0,
            battery_degradation_alpha: 0.01,
        }
    }

    /// 根据运行场景计算奖励值
    pub fn calculate(
        &self,
        mode: RunningMode,
        action: &ActionOutput,
        state: &FusedSystemState,
    ) -> f64 {
        match mode {
            RunningMode::AgriculturalIrrigation => self.calc_agri(state),
            RunningMode::CommercialArbitrage => self.calc_arbitrage(action, state),
            RunningMode::DemandControl => self.calc_demand(action, state),
            RunningMode::VirtualPowerPlant => self.calc_vpp(action, state),
            RunningMode::UltraGreen => self.calc_green(state),
        }
    }

    /// SCENE-01: 农网灌溉 — R = w1*R_pv - w2*P_batt_deg - w3*P_trafo
    fn calc_agri(&self, state: &FusedSystemState) -> f64 {
        let w = &self.weights.agricultural_irrigation;
        let r_pv = (state.pv_power.max(0.0) / (state.pv_power.max(0.0) + state.grid_power.max(0.0) + 1e-6))
            .min(1.0) * 100.0;
        let p_batt_deg = self.battery_degradation_alpha * (state.battery_power.abs() / 500.0) * 100.0;
        let p_trafo = 200.0 * (state.transformer_load - 1.0).max(0.0);
        w[0] * r_pv - w[1] * p_batt_deg - w[2] * p_trafo
    }

    /// SCENE-B1: 自主套利 — R = w1*R_price - w2*P_batt_deg
    fn calc_arbitrage(&self, action: &ActionOutput, state: &FusedSystemState) -> f64 {
        let w = &self.weights.commercial_arbitrage;
        let avg_price = (state.peak_price + state.valley_price) / 2.0;
        let spread = (state.current_electricity_price - avg_price) * action.p_batt_set * 0.001;
        let r_spread = spread * 100.0;
        let p_deg = 100.0 * action.p_batt_set.abs() / 500.0 * 0.01;
        w[0] * r_spread - w[1] * p_deg
    }

    /// SCENE-B2: 需量控制 — R = w1*R_demand - w2*P_comfort
    fn calc_demand(&self, action: &ActionOutput, state: &FusedSystemState) -> f64 {
        let w = &self.weights.demand_control;
        let demand_saved = (state.contract_demand - state.current_demand).max(0.0);
        let r_avoid = demand_saved * self.demand_penalty_rate;
        let p_comfort = action.load_shedding * 0.5;
        w[0] * r_avoid - w[1] * p_comfort
    }

    /// SCENE-B3: 虚拟电厂 — R = w1*R_ancillary + w2*R_accuracy - w3*P_deadline
    fn calc_vpp(&self, action: &ActionOutput, state: &FusedSystemState) -> f64 {
        let w = &self.weights.virtual_power_plant;
        match state.dispatch_p_set {
            Some(p_target) => {
                let r_accuracy = 100.0 * (1.0 - (action.p_batt_set - p_target).abs() / 100.0).max(0.0);
                w[0] * p_target.abs() * 0.01 + w[1] * r_accuracy - w[2] * 0.0
            }
            None => 0.0,
        }
    }

    /// SCENE-B5: 极致绿色 — R = w1*R_green + w2*R_carbon
    fn calc_green(&self, state: &FusedSystemState) -> f64 {
        let w = &self.weights.ultra_green;
        let total = state.load_power.max(1e-6);
        let green_consume = state.pv_power.max(0.0);
        let r_green = 100.0 * (green_consume / total).min(1.0);
        let c_baseline = self.carbon_emission_factor;
        let c_actual = state.grid_power.max(0.0) * c_baseline / 1000.0;
        let r_carbon = if c_baseline > 0.0 {
            100.0 * (c_baseline - c_actual).max(0.0) / c_baseline
        } else {
            0.0
        };
        w[0] * r_green + w[1] * r_carbon
    }
}

impl SceneWeights {
    /// 根据运行模式返回对应的权重数组引用
    pub fn lookup(&self, mode: RunningMode) -> &[f64] {
        match mode {
            RunningMode::AgriculturalIrrigation => &self.agricultural_irrigation[..3],
            RunningMode::CommercialArbitrage => &self.commercial_arbitrage,
            RunningMode::DemandControl => &self.demand_control,
            RunningMode::VirtualPowerPlant => &self.virtual_power_plant,
            RunningMode::UltraGreen => &self.ultra_green,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> FusedSystemState {
        FusedSystemState {
            pv_power: 100.0,
            load_power: 50.0,
            grid_power: 10.0,
            transformer_load: 0.9,
            battery_power: -50.0,
            current_electricity_price: 0.8,
            peak_price: 0.8,
            valley_price: 0.3,
            ..Default::default()
        }
    }

    fn make_action() -> ActionOutput {
        ActionOutput {
            p_batt_set: -50.0,
            q_batt_set: 10.0,
            load_shedding: 0.0,
            pv_limit: 1.0,
            confidence: 0.8,
        }
    }

    #[test]
    fn test_agri_full_pv_reward() {
        let calc = RewardCalculator::new(SceneWeights::default());
        let r = calc.calculate(
            RunningMode::AgriculturalIrrigation,
            &make_action(),
            &make_state(),
        );
        assert!(r > 0.0, "完全光伏消纳应产生正奖励");
    }

    #[test]
    fn test_agri_overload_penalty() {
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = make_state();
        state.transformer_load = 1.2;
        let r = calc.calculate(RunningMode::AgriculturalIrrigation, &make_action(), &state);
        assert!(r < 0.0, "变压器过载应产生负奖励");
    }

    #[test]
    fn test_arbitrage_positive_spread() {
        let calc = RewardCalculator::new(SceneWeights::default());
        let r = calc.calculate(RunningMode::CommercialArbitrage, &make_action(), &make_state());
        assert!(r > 0.0);
    }

    #[test]
    fn test_green_reward() {
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = make_state();
        state.pv_power = 100.0;
        state.load_power = 10.0;
        state.grid_power = 0.0;
        let r = calc.calculate(RunningMode::UltraGreen, &make_action(), &state);
        assert!(r > 0.0);
    }

    #[test]
    fn test_weights_lookup() {
        let w = SceneWeights::default();
        assert_eq!(w.lookup(RunningMode::AgriculturalIrrigation).len(), 3);
        assert_eq!(w.lookup(RunningMode::CommercialArbitrage).len(), 2);
    }
}
