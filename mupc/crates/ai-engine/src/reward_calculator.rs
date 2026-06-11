//! 奖励函数计算模块
//!
//! 根据当前运行场景选择对应奖励函数计算奖励值，
//! 用于在线微调的模型权重更新和 Web UI 决策质量展示。
//!
//! 5 种场景奖励函数：
//! - MODE-01 农网灌溉: R = w1*R_pv - w2*P_batt_deg - w3*P_trafo - w4*P_voltage - w5*R_ramp - w6*R_voltage_slope
//! - MODE-02 自主套利: R = w1*R_price - w2*P_batt_deg
//! - MODE-03 需量控制: R = w1*R_demand - w2*P_comfort
//! - MODE-04 虚拟电厂: R = w1*R_ancillary + w2*R_accuracy - w3*P_deadline
//! - MODE-05 极致绿色: R = w1*R_green + w2*R_carbon

use crate::config::SceneWeights;
use crate::data_fusion::FusedSystemState;
use crate::mode_selector::RunningMode;
use crate::rl_model::ActionOutput;
use std::sync::RwLock;

/// 奖励函数计算器
pub struct RewardCalculator {
    weights: SceneWeights,
    carbon_emission_factor: f64,
    demand_penalty_rate: f64,
    /// 电池退化系数（保留字段，v2.4 使用 C-rate² 简化计算）
    #[allow(dead_code)]
    battery_degradation_alpha: f64,
    /// 电池额定容量 (kWh)，用于 C-rate 计算和 R_ramp 归一化
    battery_capacity_kwh: f64,
    /// 上一周期电池有功功率设定值 (kW)，用于 R_ramp 计算
    last_p_batt_set: RwLock<f64>,
    /// 上一周期平均电压 (p.u.)，用于 R_voltage_slope 计算
    last_voltage: RwLock<f64>,
    /// 电压越限连续步数计数器（用于死区触发）
    voltage_violation_count: std::sync::atomic::AtomicU32,
    // v2.5 新增配置参数
    q_margin_threshold: f64,
    voltage_high_limit: f64,
    soc_critical: f64,
    voltage_penalty_high: f64,
    voltage_penalty_low: f64,
}

impl RewardCalculator {
    pub fn new(weights: SceneWeights) -> Self {
        Self {
            weights,
            carbon_emission_factor: 0.581,
            demand_penalty_rate: 50.0,
            battery_degradation_alpha: 0.01,
            battery_capacity_kwh: 100.0,
            last_p_batt_set: RwLock::new(0.0),
            last_voltage: RwLock::new(1.0),
            voltage_violation_count: std::sync::atomic::AtomicU32::new(0),
            q_margin_threshold: 0.10,
            voltage_high_limit: 1.05,
            soc_critical: 0.10,
            voltage_penalty_high: 2.0,
            voltage_penalty_low: 1.0,
        }
    }

    /// v2.5: 从配置创建（支持自定义阈值）
    pub fn new_with_thresholds(
        weights: SceneWeights,
        cfg: &crate::config::RewardThresholdConfig,
    ) -> Self {
        Self {
            weights,
            carbon_emission_factor: 0.581,
            demand_penalty_rate: 50.0,
            battery_degradation_alpha: 0.01,
            battery_capacity_kwh: 100.0,
            last_p_batt_set: RwLock::new(0.0),
            last_voltage: RwLock::new(1.0),
            voltage_violation_count: std::sync::atomic::AtomicU32::new(0),
            q_margin_threshold: cfg.q_margin_threshold,
            voltage_high_limit: cfg.voltage_high_limit,
            soc_critical: cfg.soc_critical,
            voltage_penalty_high: cfg.voltage_penalty_high,
            voltage_penalty_low: cfg.voltage_penalty_low,
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
            RunningMode::SeasonalLoadManagement => {
                let prev = *self.last_p_batt_set.read().unwrap();
                self.calc_agri_v2_5(state, action.p_batt_set, prev)
            }
            RunningMode::CommercialArbitrage => self.calc_arbitrage(action, state),
            RunningMode::DemandControl => self.calc_demand(action, state),
            RunningMode::VirtualPowerPlant => self.calc_vpp(action, state),
            RunningMode::UltraGreen => self.calc_green(state),
        }
    }

    /// 更新上一周期电池功率设定值（在决策周期结束时调用）
    pub fn update_last_p_batt(&self, p_batt_set: f64) {
        *self.last_p_batt_set.write().unwrap() = p_batt_set;
    }

    /// SCENE-01: 农网灌溉
    /// R = w1*R_pv - w2*P_batt_deg - w3*P_trafo - w4*P_voltage_deviation - w5*R_ramp
    fn calc_agri(&self, state: &FusedSystemState, p_batt_set: f64, prev_p_batt: f64) -> f64 {
        let w = &self.weights.seasonal_load_management;
        let r_pv = (state.pv_power.max(0.0)
            / (state.pv_power.max(0.0) + state.grid_power.max(0.0) + 1e-6))
            .min(1.0)
            * 100.0;

        let c_rate = state.battery_power.abs() / self.battery_capacity_kwh;
        let p_batt_deg = c_rate * c_rate;

        let p_trafo = self.overload_penalty(state.transformer_load);

        let v_avg = (state.voltage_phase_a + state.voltage_phase_b + state.voltage_phase_c) / 3.0;
        let p_voltage = self.voltage_penalty_with_deadband(v_avg);

        let delta_p = (p_batt_set - prev_p_batt).abs();
        let r_ramp = w[4] * delta_p / self.battery_capacity_kwh;

        w[0] * r_pv - w[1] * p_batt_deg - w[2] * p_trafo - w[3] * p_voltage - r_ramp
    }

    /// SCENE-01: 台区季节性负荷模式 v2.6
    /// R = w1*R_pv - w2*P_batt_deg - w3*P_trafo - w4*P_voltage - w5*R_ramp - w6*R_voltage_slope
    fn calc_agri_v2_5(&self, state: &FusedSystemState, p_batt_set: f64, prev_p_batt: f64) -> f64 {
        let w = &self.weights.seasonal_load_management;

        // 1. 弃光奖励（含电压安全前置条件）
        // v_avg >= voltage_high_limit 时置零
        let v_avg = (state.voltage_phase_a + state.voltage_phase_b + state.voltage_phase_c) / 3.0;
        let r_pv = if v_avg >= self.voltage_high_limit {
            0.0 // 电压偏高，弃光无意义
        } else {
            (state.pv_power.max(0.0) / (state.pv_power.max(0.0) + state.grid_power.max(0.0) + 1e-6))
                .min(1.0)
                * 100.0
        };

        // 2. 自适应损耗系数 α(s)
        let alpha = self.compute_alpha(state);

        // 3. 电池损耗
        let c_rate = state.battery_power.abs() / self.battery_capacity_kwh;
        let p_batt_deg = alpha * c_rate * c_rate;

        // 4. 变压器过载
        let p_trafo = self.overload_penalty(state.transformer_load);

        // 5. 条件触发电压惩罚
        let p_voltage = self.conditional_voltage_penalty(state);

        // 6. 变化率惩罚
        let r_ramp = w[4] * (p_batt_set - prev_p_batt).abs() / self.battery_capacity_kwh;

        // 7. v2.6 新增：电压变化斜率惩罚 R_voltage_slope = |ΔV|
        let prev_v = *self.last_voltage.read().unwrap();
        let r_voltage_slope = (v_avg - prev_v).abs();

        w[0] * r_pv
            - w[1] * p_batt_deg
            - w[2] * p_trafo
            - w[3] * p_voltage
            - w[4] * r_ramp
            - w[5] * r_voltage_slope
    }

    /// 计算自适应损耗系数 α(s)
    /// 优先级：SOC 极低保护 > 电压支撑模式 > 常规调度
    fn compute_alpha(&self, state: &FusedSystemState) -> f64 {
        // SOC 极低保护：优先级最高
        if state.battery_soc < self.soc_critical {
            return 3.0;
        }

        // 电压支撑模式：q_realtime_margin <= 阈值 且 电压越限连续2步
        let v_avg = (state.voltage_phase_a + state.voltage_phase_b + state.voltage_phase_c) / 3.0;
        let v_dev = (v_avg - 1.0).abs();
        let in_voltage_violation = v_dev > 0.05 && self.voltage_violation_count() >= 2;
        let q_exhausted = state.q_realtime_margin <= self.q_margin_threshold;

        if q_exhausted && in_voltage_violation {
            return 0.2; // 电压支撑模式，鼓励果断放电
        }

        1.0 // 常规调度
    }

    /// 电压越限计数器读取（辅助 compute_alpha）
    fn voltage_violation_count(&self) -> u32 {
        self.voltage_violation_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 条件触发电压惩罚
    /// 仅当 q_realtime_margin <= 阈值 且 电压越限连续2步时才返回惩罚值
    fn conditional_voltage_penalty(&self, state: &FusedSystemState) -> f64 {
        let v_avg = (state.voltage_phase_a + state.voltage_phase_b + state.voltage_phase_c) / 3.0;
        let dev = (v_avg - 1.0).abs();

        // 死区内，无惩罚，计数器清零
        if dev <= 0.05 {
            self.voltage_violation_count
                .store(0, std::sync::atomic::Ordering::Relaxed);
            return 0.0;
        }

        // 越限，计数器 +1
        let count = self
            .voltage_violation_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;

        // 不足 2 步，不惩罚
        if count < 2 {
            return 0.0;
        }

        // q 裕度充足，不惩罚（电压问题是实时模块的责任）
        if state.q_realtime_margin > self.q_margin_threshold {
            return 0.0;
        }

        // 触发惩罚
        let dev_excess = dev - 0.05;
        if v_avg < 1.0 {
            self.voltage_penalty_low * dev_excess * dev_excess // 低电压侧，斜率更高
        } else {
            self.voltage_penalty_high * dev_excess * dev_excess // 高电压侧
        }
    }

    /// 电压惩罚（±5% 死区，越限连续2步才触发）
    fn voltage_penalty_with_deadband(&self, v_avg: f64) -> f64 {
        const V_DEAD: f64 = 0.05;
        const V_REF: f64 = 1.0;
        let dev = (v_avg - V_REF).abs();

        if dev <= V_DEAD {
            self.voltage_violation_count
                .store(0, std::sync::atomic::Ordering::Relaxed);
            return 0.0;
        }

        let count = self
            .voltage_violation_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;

        if count < 2 {
            return 0.0;
        }

        let dev_excess = dev - V_DEAD;
        let normalized = dev_excess / 0.10;
        if v_avg < V_REF - V_DEAD {
            2.0 * normalized * normalized
        } else {
            1.0 * normalized * normalized
        }
    }

    /// 变压器过载惩罚（75% 以上开始惩罚）
    fn overload_penalty(&self, load: f64) -> f64 {
        if load <= 0.75 {
            0.0
        } else {
            200.0 * (load - 1.0).powi(2)
        }
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
                let r_accuracy =
                    100.0 * (1.0 - (action.p_batt_set - p_target).abs() / 100.0).max(0.0);
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
            RunningMode::SeasonalLoadManagement => &self.seasonal_load_management,
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
            RunningMode::SeasonalLoadManagement,
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
        let r = calc.calculate(RunningMode::SeasonalLoadManagement, &make_action(), &state);
        assert!(r < 0.0, "变压器过载应产生负奖励");
    }

    #[test]
    fn test_arbitrage_positive_spread() {
        let calc = RewardCalculator::new(SceneWeights::default());
        let r = calc.calculate(
            RunningMode::CommercialArbitrage,
            &make_action(),
            &make_state(),
        );
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
        assert_eq!(w.lookup(RunningMode::SeasonalLoadManagement).len(), 6);
        assert_eq!(w.lookup(RunningMode::CommercialArbitrage).len(), 2);
    }

    #[test]
    fn test_voltage_deadband_no_penalty_in_deadband() {
        let calc = RewardCalculator::new(SceneWeights::default());
        // v_avg = 1.0 (reference) → 死区内
        assert!((calc.voltage_penalty_with_deadband(1.0) - 0.0).abs() < 1e-6);
        // v_avg = 1.03 → 死区边界内
        assert!((calc.voltage_penalty_with_deadband(1.03) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_voltage_deadband_triggers_after_2_steps() {
        let calc = RewardCalculator::new(SceneWeights::default());
        // 第一次越限（v_avg = 1.08 > 1.05），count=1，不触发
        calc.voltage_penalty_with_deadband(1.08);
        // 第二次越限，count=2，触发惩罚
        let penalty = calc.voltage_penalty_with_deadband(1.08);
        assert!(penalty > 0.0, "越限连续2步应触发惩罚");
    }

    // ===== v2.5 专项测试 =====

    #[test]
    fn test_v2_5_conditional_voltage_penalty_q_margin_sufficient() {
        // q_realtime_margin > threshold 时，即使电压越限也不惩罚
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.voltage_phase_a = 1.08;
        state.voltage_phase_b = 1.08;
        state.voltage_phase_c = 1.08;
        state.q_realtime_margin = 0.5; // > 0.10，裕度充足

        // 触发越限计数
        calc.conditional_voltage_penalty(&state);
        calc.conditional_voltage_penalty(&state);

        let penalty = calc.conditional_voltage_penalty(&state);
        assert!((penalty - 0.0).abs() < 1e-6, "q裕度充足时不应触发电压惩罚");
    }

    #[test]
    fn test_v2_5_conditional_voltage_penalty_q_margin_exhausted() {
        // q_realtime_margin <= threshold 且电压越限2步 → 触发惩罚
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.voltage_phase_a = 1.08;
        state.voltage_phase_b = 1.08;
        state.voltage_phase_c = 1.08;
        state.q_realtime_margin = 0.05; // <= 0.10，裕度耗尽

        calc.conditional_voltage_penalty(&state);
        let penalty = calc.conditional_voltage_penalty(&state);
        assert!(penalty > 0.0, "q裕度耗尽+越限2步应触发电压惩罚");
    }

    #[test]
    fn test_v2_5_r_pv_zero_when_voltage_high() {
        // v_avg >= 1.05 时，弃光奖励应为 0
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.pv_power = 100.0;
        state.grid_power = 10.0;
        state.voltage_phase_a = 1.06;
        state.voltage_phase_b = 1.06;
        state.voltage_phase_c = 1.06;

        let action = ActionOutput {
            p_batt_set: -50.0,
            q_batt_set: 10.0,
            load_shedding: 0.0,
            pv_limit: 1.0,
            confidence: 0.8,
        };

        let r = calc.calculate(RunningMode::SeasonalLoadManagement, &action, &state);
        // R_pv 分量应为 0（因为电压 >= 1.05），所以总奖励会很低或为负
        assert!(r < 50.0, "高电压时弃光奖励应为0，总奖励应较低");
    }

    #[test]
    fn test_v2_5_alpha_soc_critical() {
        // SOC < 10% 时 α = 3.0
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.battery_soc = 0.05; // < 10%

        let alpha = calc.compute_alpha(&state);
        assert!((alpha - 3.0).abs() < 1e-6, "SOC极低时α应为3.0");
    }

    #[test]
    fn test_v2_5_alpha_voltage_support() {
        // q裕度耗尽 + 电压越限2步 → α = 0.2
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.battery_soc = 0.5; // 正常SOC
        state.q_realtime_margin = 0.05; // <= 0.10
        state.voltage_phase_a = 1.08;
        state.voltage_phase_b = 1.08;
        state.voltage_phase_c = 1.08;

        // 先触发越限计数（需要 count >= 2）
        calc.conditional_voltage_penalty(&state);
        calc.conditional_voltage_penalty(&state);

        let alpha = calc.compute_alpha(&state);
        assert!((alpha - 0.2).abs() < 1e-6, "电压支撑模式α应为0.2");
    }

    #[test]
    fn test_v2_5_alpha_normal() {
        // 常规调度：SOC正常，q裕度充足 → α = 1.0
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.battery_soc = 0.5;
        state.q_realtime_margin = 0.5;
        state.voltage_phase_a = 1.0;
        state.voltage_phase_b = 1.0;
        state.voltage_phase_c = 1.0;

        let alpha = calc.compute_alpha(&state);
        assert!((alpha - 1.0).abs() < 1e-6, "常规调度α应为1.0");
    }

    #[test]
    fn test_v2_5_alpha_priority_soc_over_voltage_support() {
        // SOC 极低和电压支撑都满足时，SOC极低优先（α=3.0）
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.battery_soc = 0.05; // SOC极低
        state.q_realtime_margin = 0.05; // 电压支撑条件也满足
        state.voltage_phase_a = 1.08;
        state.voltage_phase_b = 1.08;
        state.voltage_phase_c = 1.08;

        calc.conditional_voltage_penalty(&state);
        calc.conditional_voltage_penalty(&state);

        let alpha = calc.compute_alpha(&state);
        assert!((alpha - 3.0).abs() < 1e-6, "SOC极低保护优先级最高");
    }
}
