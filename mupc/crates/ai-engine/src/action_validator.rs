//! 动作约束校验器
//!
//! 对 RL 模型输出的 ActionOutput 执行约束校验，
//! 违反时自动 clamp 到安全边界并记录 WARN 日志。
//!
//! v2.4 分层控制架构说明：
//! - q_batt_set 由实时控制模块根据电压闭环管理，不由 AI 控制
//! - v2.4 模式下这两条规则被跳过，仅保留 ACT-01/04/05
//! - v2.5 动作空间参数可配置化：值域 clamp 使用 ActionSpaceConfig
//! - v2.6 双参数模式：新增 ACT-DUAL-01~04 校验规则

use crate::action_space::ActionSpaceConfig;
use crate::config::ActionConstraintConfig;
use crate::rl_model::ActionOutput;
use std::sync::RwLock;

/// 动作约束校验器
pub struct ActionValidator {
    config: ActionConstraintConfig,
    last_action: RwLock<Option<ActionOutput>>,
    /// v2.4 分层控制架构：q_batt_set 由实时控制模块管理，跳过相关约束
    v2_4_mode: bool,
    /// v2.6 双参数模式：k_droop 范围（由实时控制模块提供）
    droop_range: RwLock<(f64, f64)>,
    /// v2.6 双参数模式：启用 ACT-DUAL-01~04 校验
    dual_mode: bool,
}

/// 约束违规记录
#[derive(Debug, Clone)]
pub struct ViolationRecord {
    pub rule: &'static str,
    pub field: &'static str,
    pub original: f64,
    pub clamped: f64,
}

impl ActionValidator {
    /// 创建校验器（默认 v2.3 兼容模式）
    pub fn new(config: ActionConstraintConfig) -> Self {
        Self {
            config,
            last_action: RwLock::new(None),
            v2_4_mode: false,
            droop_range: RwLock::new((-100.0, 100.0)),
            dual_mode: false,
        }
    }

    /// 创建 v2.4 模式校验器（跳过 q_batt_set 相关约束）
    pub fn new_v2_4(config: ActionConstraintConfig) -> Self {
        Self {
            config,
            last_action: RwLock::new(None),
            v2_4_mode: true,
            droop_range: RwLock::new((-100.0, 100.0)),
            dual_mode: false,
        }
    }

    /// 创建 v2.6 双参数模式校验器（启用 ACT-DUAL-01~04）
    pub fn new_dual(config: ActionConstraintConfig, k_droop_min: f64, k_droop_max: f64) -> Self {
        Self {
            config,
            last_action: RwLock::new(None),
            v2_4_mode: true,
            droop_range: RwLock::new((k_droop_min, k_droop_max)),
            dual_mode: true,
        }
    }

    /// 更新 k_droop 范围（由 intercore 从实时控制模块获取后调用）
    pub fn update_droop_range(&self, k_min: f64, k_max: f64) {
        let mut range = self.droop_range.write().unwrap();
        *range = (k_min, k_max);
        tracing::debug!("Updated k_droop range: [{}, {}]", k_min, k_max);
    }

    /// 获取当前 k_droop 范围
    pub fn get_droop_range(&self) -> (f64, f64) {
        *self.droop_range.read().unwrap()
    }

    /// 校验动作输出，执行约束规则
    ///
    /// v2.3 模式：ACT-01~05 全部生效
    /// v2.4 模式：跳过 ACT-02（q_batt 变化率）和 ACT-03（视在功率含 q_batt），
    /// 仅保留 ACT-01（p_batt 变化率）、ACT-05（调度约束）
    /// v2.5 动作空间参数可配置化：值域 clamp 使用 ActionSpaceConfig 中的参数
    /// v2.15: ACT-04（pv_limit 下限）和 ACT-06（load_shedding/confidence clamp）
    ///   已移除，pv_limit/load_shedding 下沉至策略引擎本地兜底策略
    pub fn validate(
        &self,
        action: &ActionOutput,
        dispatch_p_set: Option<f64>,
        _is_anti_reverse: bool, // v2.15: pv_limit clamp 已移除，该参数保留用于 API 兼容
        action_space_config: &ActionSpaceConfig,
    ) -> (ActionOutput, Vec<ViolationRecord>) {
        let mut validated = action.clone();
        let mut violations = Vec::new();

        let last = self.last_action.read().unwrap();

        // ACT-01: 有功变化率限制
        if let Some(ref prev) = *last {
            let delta = (action.p_ref - prev.p_ref).abs();
            if delta > self.config.p_batt_ramp_limit_kw {
                let sign = if action.p_ref > prev.p_ref { 1.0 } else { -1.0 };
                validated.p_ref = prev.p_ref + sign * self.config.p_batt_ramp_limit_kw;
                violations.push(ViolationRecord {
                    rule: "ACT-01",
                    field: "p_ref",
                    original: action.p_ref,
                    clamped: validated.p_ref,
                });
            }
        }

        // ACT-02: 无功变化率限制（v2.4 跳过，由实时控制模块管理）
        if !self.v2_4_mode {
            if let Some(ref prev) = *last {
                let delta = (action.k_droop - prev.k_droop).abs();
                if delta > self.config.q_batt_ramp_limit_kvar {
                    let sign = if action.k_droop > prev.k_droop {
                        1.0
                    } else {
                        -1.0
                    };
                    validated.k_droop = prev.k_droop + sign * self.config.q_batt_ramp_limit_kvar;
                    violations.push(ViolationRecord {
                        rule: "ACT-02",
                        field: "k_droop",
                        original: action.k_droop,
                        clamped: validated.k_droop,
                    });
                }
            }
        }

        // ACT-03: 视在功率圆约束（v2.4 跳过，仅约束 p_ref 在 S_max 内）
        if !self.v2_4_mode {
            let s = (validated.p_ref.powi(2) + validated.k_droop.powi(2)).sqrt();
            if s > self.config.max_apparent_power_kva {
                let scale = self.config.max_apparent_power_kva / s;
                validated.p_ref *= scale;
                validated.k_droop *= scale;
                violations.push(ViolationRecord {
                    rule: "ACT-03",
                    field: "p_ref+k_droop",
                    original: s,
                    clamped: self.config.max_apparent_power_kva,
                });
            }
        } else {
            // v2.4: 仅保证 p_ref 不超出 S_max（k_droop 由实时模块管理）
            let p_sq = validated.p_ref.powi(2);
            let s_max = self.config.max_apparent_power_kva;
            if p_sq > s_max * s_max {
                validated.p_ref = validated.p_ref.signum() * s_max;
                violations.push(ViolationRecord {
                    rule: "ACT-03",
                    field: "p_ref",
                    original: p_sq.sqrt(),
                    clamped: s_max,
                });
            }
        }

        // ACT-04 已移除（v2.15）：pv_limit 下限校验下沉至策略引擎本地防逆流策略

        // ACT-05: 调度指令权限约束
        if let Some(dp) = dispatch_p_set {
            if validated.p_ref.abs() > dp.abs() {
                let sign = validated.p_ref.signum();
                validated.p_ref = sign * dp.abs();
                violations.push(ViolationRecord {
                    rule: "ACT-05",
                    field: "p_ref",
                    original: action.p_ref,
                    clamped: validated.p_ref,
                });
            }
        }

        // 最终值域 clamp（v2.5 使用 ActionSpaceConfig 中的参数）
        validated.p_ref = validated.p_ref.clamp(
            -action_space_config.max_batt_discharge_power,
            action_space_config.max_batt_charge_power,
        );
        validated.k_droop = validated.k_droop.clamp(-300.0, 300.0);
        // v2.15: load_shedding/pv_limit/confidence clamp 已移除，这些字段不再属于 ActionOutput

        *self.last_action.write().unwrap() = Some(validated.clone());
        (validated, violations)
    }

    /// v2.6 双参数模式校验（ACT-DUAL-01~04）
    ///
    /// ACT-DUAL-01: p_ref ∈ [-max_batt_discharge_power, max_batt_charge_power]
    /// ACT-DUAL-02: k_droop ∈ [k_droop_min, k_droop_max]
    /// ACT-DUAL-03: Δp_ref 变化率 <= p_batt_ramp_limit_kw / 步
    /// ACT-DUAL-04: 当 dispatch_p_set 有效时，p_ref 绝对值不得超过 dispatch_p_set 绝对值
    pub fn validate_dual(
        &self,
        action: &ActionOutput,
        dispatch_p_set: Option<f64>,
        _is_anti_reverse: bool, // v2.15: pv_limit clamp 已移除，该参数保留用于 API 兼容
        action_space_config: &ActionSpaceConfig,
    ) -> (ActionOutput, Vec<ViolationRecord>) {
        let mut validated = action.clone();
        let mut violations = Vec::new();
        let last = self.last_action.read().unwrap();

        // ACT-DUAL-01: p_ref 值域约束
        let p_ref_min = -action_space_config.max_batt_discharge_power;
        let p_ref_max = action_space_config.max_batt_charge_power;
        if validated.p_ref < p_ref_min {
            violations.push(ViolationRecord {
                rule: "ACT-DUAL-01",
                field: "p_ref",
                original: action.p_ref,
                clamped: p_ref_min,
            });
            validated.p_ref = p_ref_min;
        } else if validated.p_ref > p_ref_max {
            violations.push(ViolationRecord {
                rule: "ACT-DUAL-01",
                field: "p_ref",
                original: action.p_ref,
                clamped: p_ref_max,
            });
            validated.p_ref = p_ref_max;
        }

        // ACT-DUAL-02: k_droop 值域约束
        let (k_min, k_max) = *self.droop_range.read().unwrap();
        if validated.k_droop < k_min {
            violations.push(ViolationRecord {
                rule: "ACT-DUAL-02",
                field: "k_droop",
                original: action.k_droop,
                clamped: k_min,
            });
            validated.k_droop = k_min;
        } else if validated.k_droop > k_max {
            violations.push(ViolationRecord {
                rule: "ACT-DUAL-02",
                field: "k_droop",
                original: action.k_droop,
                clamped: k_max,
            });
            validated.k_droop = k_max;
        }

        // ACT-DUAL-03: p_ref 变化率约束
        if let Some(ref prev) = *last {
            let delta = (action.p_ref - prev.p_ref).abs();
            if delta > self.config.p_batt_ramp_limit_kw {
                let sign = if action.p_ref > prev.p_ref { 1.0 } else { -1.0 };
                validated.p_ref = prev.p_ref + sign * self.config.p_batt_ramp_limit_kw;
                violations.push(ViolationRecord {
                    rule: "ACT-DUAL-03",
                    field: "p_ref",
                    original: action.p_ref,
                    clamped: validated.p_ref,
                });
            }
        }

        // ACT-DUAL-04: 调度指令权限约束
        if let Some(dp) = dispatch_p_set {
            if validated.p_ref.abs() > dp.abs() {
                let sign = validated.p_ref.signum();
                validated.p_ref = sign * dp.abs();
                violations.push(ViolationRecord {
                    rule: "ACT-DUAL-04",
                    field: "p_ref",
                    original: action.p_ref,
                    clamped: validated.p_ref,
                });
            }
        }

        // ACT-DUAL-05 已移除（v2.15）：pv_limit 下限校验下沉至策略引擎本地防逆流策略
        // load_shedding/confidence clamp 已移除，这些字段不再属于 ActionOutput

        *self.last_action.write().unwrap() = Some(validated.clone());
        (validated, violations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_action(p: f64, q: f64) -> ActionOutput {
        ActionOutput {
            p_ref: p,
            k_droop: q,
        }
    }

    fn default_action_space_config() -> ActionSpaceConfig {
        ActionSpaceConfig::default_config()
    }

    #[test]
    fn test_act01_p_batt_ramp() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        let cfg = default_action_space_config();
        // 先设置一个历史值
        v.validate(&make_action(0.0, 0.0), None, false, &cfg);
        // 再次调用，delta=100kW > 50kW limit
        let (a, violations) = v.validate(&make_action(150.0, 0.0), None, false, &cfg);
        assert!(violations.iter().any(|r| r.rule == "ACT-01"));
        assert!(a.p_ref <= 50.0);
    }

    #[test]
    fn test_act03_power_circle_clamp() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        let cfg = default_action_space_config();
        let (a, violations) = v.validate(&make_action(400.0, 400.0), None, false, &cfg);
        let s = (a.p_ref.powi(2) + a.k_droop.powi(2)).sqrt();
        assert!(s <= 500.0 + 1e-6);
        assert!(!violations.is_empty());
    }

    #[test]
    fn test_act04_removed_v2_15() {
        // v2.15: ACT-04 (pv_limit clamp) 已从 AI 校验器移除，
        // pv_limit 下沉至策略引擎本地防逆流策略
        let v = ActionValidator::new(ActionConstraintConfig::default());
        let cfg = default_action_space_config();
        let (_a, violations) = v.validate(&make_action(0.0, 0.0), None, false, &cfg);
        // ACT-04 不再由 ActionValidator 触发
        assert!(!violations.iter().any(|r| r.rule == "ACT-04"));
    }

    #[test]
    fn test_act05_dispatch_constraint() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        let cfg = default_action_space_config();
        let (a, violations) =
            v.validate(&make_action(150.0, 0.0), Some(100.0), false, &cfg);
        assert!(a.p_ref.abs() <= 100.0);
        assert!(violations.iter().any(|r| r.rule == "ACT-05"));
    }

    #[test]
    fn test_v2_4_mode_skips_act02_and_act03_q() {
        let v = ActionValidator::new_v2_4(ActionConstraintConfig::default());
        let cfg = default_action_space_config();
        // 先设置历史值
        v.validate(&make_action(0.0, 0.0), None, false, &cfg);
        // v2.4 模式：k_droop 变化不受限（由实时模块控制）
        let (a, _violations) = v.validate(&make_action(100.0, 200.0), None, false, &cfg);
        assert_eq!(a.k_droop, 200.0); // k_droop 未被 clamp
    }

    #[test]
    fn test_v2_4_mode_applies_p_batt_only() {
        let v = ActionValidator::new_v2_4(ActionConstraintConfig::default());
        let cfg = default_action_space_config();
        let (a, violations) = v.validate(&make_action(600.0, 0.0), None, false, &cfg);
        // v2.4: p_ref clamp 到 S_max
        assert!(a.p_ref.abs() <= 500.0);
        assert!(violations.iter().any(|r| r.rule == "ACT-03"));
    }

    #[test]
    fn test_no_violations_for_valid_action() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        let cfg = default_action_space_config();
        let (_a, violations) = v.validate(&make_action(100.0, 50.0), None, false, &cfg);
        assert!(!violations.iter().any(|r| r.rule == "ACT-03"));
    }

    #[test]
    fn test_action_space_config_clamp_values() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        // 自定义配置：充电功率上限 30kW，放电功率上限 40kW
        let mut cfg = ActionSpaceConfig::default_config();
        cfg.max_batt_charge_power = 30.0;
        cfg.max_batt_discharge_power = 40.0;

        // p_ref = 100（充电）应被 clamp 到 30
        let (a, _) = v.validate(&make_action(100.0, 0.0), None, false, &cfg);
        assert!(a.p_ref <= 30.0);

        // p_ref = -100（放电）应被 clamp 到 -40
        let (a, _) = v.validate(&make_action(-100.0, 0.0), None, false, &cfg);
        assert!(a.p_ref >= -40.0);
    }
}
