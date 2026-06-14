//! 奖励函数计算模块
//!
//! 根据当前运行场景选择对应奖励函数计算奖励值，
//! 用于在线微调的模型权重更新和 Web UI 决策质量展示。
//!
//! 5 种场景奖励函数：
//! - MODE-01 农网灌溉: R = w1*R_pv - w2*P_batt_deg - w3*P_trafo + w4*R_PQ_coordination - w5*R_ramp - w6*R_voltage_slope - w7*R_smooth
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
    /// 高电压侧电压惩罚系数（v2.8废弃，仅保留用于兼容）
    #[allow(dead_code)]
    voltage_penalty_high: f64,
    /// 低电压侧电压惩罚系数（v2.8废弃，仅保留用于兼容）
    #[allow(dead_code)]
    voltage_penalty_low: f64,
    // v2.8 新增配置参数
    /// 上一周期下垂系数 (k_droop)，用于 R_smooth 计算
    last_k_droop: RwLock<f64>,
    /// 弃光差异化：高电压时放电惩罚系数
    pv_high_voltage_penalty: f64,
    /// 下垂系数平滑惩罚系数 λ
    smooth_lambda: f64,
    /// 下垂系数硬上限 K_MAX
    k_droop_max: f64,
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
            last_k_droop: RwLock::new(10.0),
            pv_high_voltage_penalty: 20.0,
            smooth_lambda: 10.0,
            k_droop_max: 30.0,
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
            last_k_droop: RwLock::new(10.0),
            // v2.8 参数目前硬编码，暂不支持通过 RewardThresholdConfig 自定义
            // TODO(v2.10): 扩展 RewardThresholdConfig 以支持 v2.8 参数自定义
            pv_high_voltage_penalty: 20.0,
            smooth_lambda: 10.0,
            k_droop_max: 30.0,
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
                self.calc_agri_v2_8(state, action, prev)
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

    /// 更新上一周期下垂系数（在决策周期结束时调用）
    pub fn update_last_k_droop(&self, k_droop: f64) {
        *self.last_k_droop.write().unwrap() = k_droop;
    }

    /// 农网灌溉 (legacy, v2.8废弃)
    #[allow(dead_code)]
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

    /// SCENE-01: 台区季节性负荷模式 v2.10
    /// R = w1*R_pv - w2*P_batt_deg - w3*P_trafo + w4*R_PQ_coordination - w5*R_ramp - w6*R_voltage_slope - w7*R_smooth - w8*R_safety_override
    fn calc_agri_v2_8(
        &self,
        state: &FusedSystemState,
        action: &ActionOutput,
        prev_p_batt: f64,
    ) -> f64 {
        let w = &self.weights.seasonal_load_management;
        let v_avg = (state.voltage_phase_a + state.voltage_phase_b + state.voltage_phase_c) / 3.0;

        // 1. 弃光奖励（含高电压差异化）
        let r_pv = self.calc_pv_reward_v2_8(state, action.p_ref, v_avg);

        // 2. 自适应损耗系数 α(s)
        let alpha = self.compute_alpha(state);

        // 3. 电池损耗
        let c_rate = state.battery_power.abs() / self.battery_capacity_kwh;
        let p_batt_deg = alpha * c_rate * c_rate;

        // 4. 变压器过载
        let p_trafo = self.overload_penalty(state.transformer_load);

        // 5. P-Q 协同度奖励（v2.8 核心）
        let r_pq = self.calc_pq_coordination(state, action.p_ref);

        // 6. 变化率惩罚
        let r_ramp = w[4] * (action.p_ref - prev_p_batt).abs() / self.battery_capacity_kwh;

        // 7. 电压变化斜率惩罚
        let prev_v = *self.last_voltage.read().unwrap();
        let r_voltage_slope = (v_avg - prev_v).abs();

        // 8. 下垂系数平滑惩罚（v2.8 新增）
        let r_smooth = self.calc_smooth_penalty(action.k_droop);

        // 9. 安全覆盖惩罚（v2.10 新增）
        let r_safety_override = self.safety_override_penalty(state);

        w[0] * r_pv - w[1] * p_batt_deg - w[2] * p_trafo + w[3] * r_pq
            - w[4] * r_ramp
            - w[5] * r_voltage_slope
            - w[6] * r_smooth
            - w[7] * r_safety_override
    }

    /// 安全覆盖感知奖励调整（v2.10 新增）
    ///
    /// 当 safety_override_active=true 时，AI 应记录此次事件并学习避免触发。
    /// 惩罚值根据触发原因分级：
    /// - voltage_violation: -50.0（电压越限触发）
    /// - q_exhausted: -30.0（无功耗尽触发）
    /// - emergency: -100.0（紧急情况，最高惩罚）
    fn safety_override_penalty(&self, state: &FusedSystemState) -> f64 {
        if !state.safety_override_active {
            return 0.0;
        }

        let reason = state.safety_override_reason.as_deref().unwrap_or("unknown");

        match reason {
            "voltage_violation" => -50.0,
            "q_exhausted" => -30.0,
            "emergency" => -100.0,
            _ => -20.0,
        }
    }

    /// v2.8 弃光奖励（含高电压差异化）
    fn calc_pv_reward_v2_8(&self, state: &FusedSystemState, p_ref: f64, v_avg: f64) -> f64 {
        if v_avg >= self.voltage_high_limit {
            // 高电压场景：检查 AI 动作方向
            if p_ref < 0.0 {
                // 充电消纳光伏，正常奖励
                (state.pv_power.max(0.0)
                    / (state.pv_power.max(0.0) + state.grid_power.max(0.0) + 1e-6))
                    .min(1.0)
                    * 100.0
            } else {
                // 高电压时放电，严厉惩罚
                -self.pv_high_voltage_penalty
            }
        } else {
            // 正常电压，标准计算
            (state.pv_power.max(0.0) / (state.pv_power.max(0.0) + state.grid_power.max(0.0) + 1e-6))
                .min(1.0)
                * 100.0
        }
    }

    /// v2.8 P-Q 协同度奖励
    ///
    /// 当 |V_deviation| > 5% 时：
    /// - Q 有裕度（q_margin > 10%）：奖励"偷懒"省电池策略
    /// - Q 已饱和（q_margin <= 10%）：奖励正确出手（低压放电/高压充电）
    fn calc_pq_coordination(&self, state: &FusedSystemState, p_ref: f64) -> f64 {
        let v_avg = (state.voltage_phase_a + state.voltage_phase_b + state.voltage_phase_c) / 3.0;
        let v_dev = (v_avg - 1.0).abs();

        // 电压在死区内，无 P-Q 协同问题
        if v_dev <= 0.05 {
            return 0.0;
        }

        let q_margin = state.q_realtime_margin;
        const Q_THRESHOLD: f64 = 0.10;
        const P_THRESHOLD: f64 = 5.0;

        if q_margin > Q_THRESHOLD {
            // Q 有裕度：AI 最优解是"偷懒"省电池
            if p_ref.abs() < P_THRESHOLD {
                50.0 // 大额奖励"偷懒"
            } else {
                -5.0 // 轻微惩罚（不必要的电池动作）
            }
        } else {
            // Q 已饱和：AI 必须正确出手
            let v_low = v_avg < 1.0;
            let v_high = v_avg > 1.0;

            if v_low && p_ref < 0.0 {
                50.0 // 低电压 + 放电（正确）
            } else if v_high && p_ref > 0.0 {
                50.0 // 高电压 + 充电（正确）
            } else if v_low && p_ref >= 0.0 {
                -30.0 // 低电压 + 不放电（失职）
            } else if v_high && p_ref <= 0.0 {
                -30.0 // 高电压 + 不充电（失职）
            } else {
                0.0
            }
        }
    }

    /// v2.8 下垂系数平滑惩罚
    ///
    /// R_smooth = -|Δk_droop| - λ·max(0, k_droop - K_MAX)
    /// 防止 AI 设置极大 k_droop 导致系统振荡
    fn calc_smooth_penalty(&self, k_droop: f64) -> f64 {
        let last_k = *self.last_k_droop.read().unwrap();
        let delta_k = (k_droop - last_k).abs();
        let excess = (k_droop - self.k_droop_max).max(0.0);

        -(delta_k + self.smooth_lambda * excess)
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

    /// 条件触发电压惩罚 (legacy, v2.8废弃)
    #[allow(dead_code)]
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

    /// 电压惩罚（±5% 死区）(legacy, v2.8废弃)
    #[allow(dead_code)]
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
        let spread = (state.current_electricity_price - avg_price) * action.p_ref * 0.001;
        let r_spread = spread * 100.0;
        let p_deg = 100.0 * action.p_ref.abs() / 500.0 * 0.01;
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
                let r_accuracy = 100.0 * (1.0 - (action.p_ref - p_target).abs() / 100.0).max(0.0);
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
            p_ref: -50.0,
            k_droop: 10.0,
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
        assert_eq!(w.lookup(RunningMode::SeasonalLoadManagement).len(), 8); // v2.10: 7 → 8
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
            p_ref: -50.0,
            k_droop: 10.0,
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

    // ===== v2.8 专项测试 =====

    #[test]
    fn test_v2_8_pq_coordination_q_margin_sufficient_idle() {
        // q_margin > 10%, p_ref near 0 → reward 50 (偷懒)
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.voltage_phase_a = 1.08;
        state.voltage_phase_b = 1.08;
        state.voltage_phase_c = 1.08;
        state.q_realtime_margin = 0.5; // > 10%

        let r = calc.calc_pq_coordination(&state, 0.0); // p_ref near 0
        assert!((r - 50.0).abs() < 1e-6, "偷懒应奖励50");
    }

    #[test]
    fn test_v2_8_pq_coordination_q_margin_exhausted_low_voltage_discharge() {
        // q_margin <= 10%, low voltage, p_ref < 0 → reward 50 (correct)
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.voltage_phase_a = 0.92;
        state.voltage_phase_b = 0.92;
        state.voltage_phase_c = 0.92;
        state.q_realtime_margin = 0.05; // <= 10%

        let r = calc.calc_pq_coordination(&state, -10.0); // p_ref < 0 (discharge)
        assert!((r - 50.0).abs() < 1e-6, "低电压放电应奖励50");
    }

    #[test]
    fn test_v2_8_smooth_penalty_exceed_k_max() {
        // k_droop > k_droop_max → penalty includes λ * excess
        // 设置 last_k_droop = 10.0, k_droop = 40.0, k_droop_max = 30.0, smooth_lambda = 10.0
        // delta_k = |40 - 10| = 30
        // excess = max(0, 40 - 30) = 10
        // R_smooth = -(30 + 10 * 10) = -130
        let calc = RewardCalculator::new(SceneWeights::default());
        let r = calc.calc_smooth_penalty(40.0);
        assert!(r < -100.0, "超过K_MAX应有严厉惩罚");
    }

    #[test]
    fn test_v2_8_pv_high_voltage_discharge_penalty() {
        // v_avg >= voltage_high_limit, p_ref > 0 → penalty -20
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.pv_power = 100.0;
        state.grid_power = 10.0;
        state.voltage_phase_a = 1.06;
        state.voltage_phase_b = 1.06;
        state.voltage_phase_c = 1.06;

        let r = calc.calc_pv_reward_v2_8(&state, 10.0, 1.06); // p_ref > 0 (discharge)
        assert!((r - (-20.0)).abs() < 1e-6, "高电压放电应惩罚-20");
    }

    #[test]
    fn test_v2_8_pv_high_voltage_charge_normal() {
        // v_avg >= voltage_high_limit, p_ref < 0 → normal reward
        let calc = RewardCalculator::new(SceneWeights::default());
        let mut state = FusedSystemState::default();
        state.pv_power = 100.0;
        state.grid_power = 10.0;
        state.voltage_phase_a = 1.06;
        state.voltage_phase_b = 1.06;
        state.voltage_phase_c = 1.06;

        let r = calc.calc_pv_reward_v2_8(&state, -10.0, 1.06); // p_ref < 0 (charge)
        assert!(r > 0.0, "高电压充电应正常奖励");
    }
}
