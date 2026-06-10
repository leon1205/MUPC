//! 动作约束校验器
//!
//! 对 RL 模型输出的 ActionOutput 执行 5 条物理约束校验，
//! 违反时自动 clamp 到安全边界并记录 WARN 日志。
//!
//! v2.4 分层控制架构说明：
//! - q_batt_set 由实时控制模块根据电压闭环管理，不由 AI 控制
//! - ACT-02（q_batt 变化率）和 ACT-03（视在功率含 q_batt）不再适用于 AI 输出
//! - v2.4 模式下这两条规则被跳过，仅保留 ACT-01/04/05

use crate::config::ActionConstraintConfig;
use crate::rl_model::ActionOutput;
use std::sync::RwLock;

/// 动作约束校验器
pub struct ActionValidator {
    config: ActionConstraintConfig,
    last_action: RwLock<Option<ActionOutput>>,
    /// v2.4 分层控制架构：q_batt_set 由实时控制模块管理，跳过相关约束
    v2_4_mode: bool,
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
        }
    }

    /// 创建 v2.4 模式校验器（跳过 q_batt_set 相关约束）
    pub fn new_v2_4(config: ActionConstraintConfig) -> Self {
        Self {
            config,
            last_action: RwLock::new(None),
            v2_4_mode: true,
        }
    }

    /// 校验动作输出，执行约束规则
    ///
    /// v2.3 模式：ACT-01~05 全部生效
    /// v2.4 模式：跳过 ACT-02（q_batt 变化率）和 ACT-03（视在功率含 q_batt），
    /// 仅保留 ACT-01（p_batt 变化率）、ACT-04（pv_limit 下限）、ACT-05（调度约束）
    pub fn validate(
        &self,
        action: &ActionOutput,
        dispatch_p_set: Option<f64>,
        is_anti_reverse: bool,
    ) -> (ActionOutput, Vec<ViolationRecord>) {
        let mut validated = action.clone();
        let mut violations = Vec::new();

        let last = self.last_action.read().unwrap();

        // ACT-01: 有功变化率限制
        if let Some(ref prev) = *last {
            let delta = (action.p_batt_set - prev.p_batt_set).abs();
            if delta > self.config.p_batt_ramp_limit_kw {
                let sign = if action.p_batt_set > prev.p_batt_set {
                    1.0
                } else {
                    -1.0
                };
                validated.p_batt_set = prev.p_batt_set + sign * self.config.p_batt_ramp_limit_kw;
                violations.push(ViolationRecord {
                    rule: "ACT-01",
                    field: "p_batt_set",
                    original: action.p_batt_set,
                    clamped: validated.p_batt_set,
                });
            }
        }

        // ACT-02: 无功变化率限制（v2.4 跳过，由实时控制模块管理）
        if !self.v2_4_mode {
            if let Some(ref prev) = *last {
                let delta = (action.q_batt_set - prev.q_batt_set).abs();
                if delta > self.config.q_batt_ramp_limit_kvar {
                    let sign = if action.q_batt_set > prev.q_batt_set {
                        1.0
                    } else {
                        -1.0
                    };
                    validated.q_batt_set =
                        prev.q_batt_set + sign * self.config.q_batt_ramp_limit_kvar;
                    violations.push(ViolationRecord {
                        rule: "ACT-02",
                        field: "q_batt_set",
                        original: action.q_batt_set,
                        clamped: validated.q_batt_set,
                    });
                }
            }
        }

        // ACT-03: 视在功率圆约束（v2.4 跳过，仅约束 p_batt_set 在 S_max 内）
        if !self.v2_4_mode {
            let s = (validated.p_batt_set.powi(2) + validated.q_batt_set.powi(2)).sqrt();
            if s > self.config.max_apparent_power_kva {
                let scale = self.config.max_apparent_power_kva / s;
                validated.p_batt_set *= scale;
                validated.q_batt_set *= scale;
                violations.push(ViolationRecord {
                    rule: "ACT-03",
                    field: "p_batt_set+q_batt_set",
                    original: s,
                    clamped: self.config.max_apparent_power_kva,
                });
            }
        } else {
            // v2.4: 仅保证 p_batt_set 不超出 S_max（q_batt_set 由实时模块管理）
            let p_sq = validated.p_batt_set.powi(2);
            let s_max = self.config.max_apparent_power_kva;
            if p_sq > s_max * s_max {
                validated.p_batt_set = validated.p_batt_set.signum() * s_max;
                violations.push(ViolationRecord {
                    rule: "ACT-03",
                    field: "p_batt_set",
                    original: p_sq.sqrt(),
                    clamped: s_max,
                });
            }
        }

        // ACT-04: 光伏限功率下限（防逆流场景除外）
        if !is_anti_reverse && validated.pv_limit < self.config.pv_limit_min {
            validated.pv_limit = self.config.pv_limit_min;
            violations.push(ViolationRecord {
                rule: "ACT-04",
                field: "pv_limit",
                original: action.pv_limit,
                clamped: validated.pv_limit,
            });
        }

        // ACT-05: 调度指令权限约束
        if let Some(dp) = dispatch_p_set {
            if validated.p_batt_set.abs() > dp.abs() {
                let sign = validated.p_batt_set.signum();
                validated.p_batt_set = sign * dp.abs();
                violations.push(ViolationRecord {
                    rule: "ACT-05",
                    field: "p_batt_set",
                    original: action.p_batt_set,
                    clamped: validated.p_batt_set,
                });
            }
        }

        // 最终值域 clamp
        validated.p_batt_set = validated.p_batt_set.clamp(-50.0, 50.0);
        validated.q_batt_set = validated.q_batt_set.clamp(-300.0, 300.0);
        validated.load_shedding = validated.load_shedding.clamp(0.0, 60.0);
        validated.pv_limit = validated.pv_limit.clamp(0.0, 1.0);
        validated.confidence = validated.confidence.clamp(0.0, 1.0);

        *self.last_action.write().unwrap() = Some(validated.clone());
        (validated, violations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_action(p: f64, q: f64, ls: f64, pv: f64) -> ActionOutput {
        ActionOutput {
            p_batt_set: p,
            q_batt_set: q,
            load_shedding: ls,
            pv_limit: pv,
            confidence: 0.8,
        }
    }

    #[test]
    fn test_act01_p_batt_ramp() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        // 先设置一个历史值
        v.validate(&make_action(0.0, 0.0, 0.0, 1.0), None, false);
        // 再次调用，delta=100kW > 50kW limit
        let (a, violations) = v.validate(&make_action(150.0, 0.0, 0.0, 1.0), None, false);
        assert!(violations.iter().any(|r| r.rule == "ACT-01"));
        assert!(a.p_batt_set <= 50.0);
    }

    #[test]
    fn test_act03_power_circle_clamp() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        let (a, violations) = v.validate(&make_action(400.0, 400.0, 0.0, 1.0), None, false);
        let s = (a.p_batt_set.powi(2) + a.q_batt_set.powi(2)).sqrt();
        assert!(s <= 500.0 + 1e-6);
        assert!(!violations.is_empty());
    }

    #[test]
    fn test_act04_pv_limit_clamp() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        let (a, violations) = v.validate(&make_action(0.0, 0.0, 0.0, 0.05), None, false);
        assert!((a.pv_limit - 0.1).abs() < 1e-6);
        assert!(violations.iter().any(|r| r.rule == "ACT-04"));
    }

    #[test]
    fn test_act04_anti_reverse_allows_zero() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        let (a, violations) = v.validate(&make_action(0.0, 0.0, 0.0, 0.0), None, true);
        assert!((a.pv_limit - 0.0).abs() < 1e-6);
        assert!(!violations.iter().any(|r| r.rule == "ACT-04"));
    }

    #[test]
    fn test_act05_dispatch_constraint() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        let (a, violations) = v.validate(&make_action(150.0, 0.0, 0.0, 1.0), Some(100.0), false);
        assert!(a.p_batt_set.abs() <= 100.0);
        assert!(violations.iter().any(|r| r.rule == "ACT-05"));
    }

    #[test]
    fn test_v2_4_mode_skips_act02_and_act03_q() {
        let v = ActionValidator::new_v2_4(ActionConstraintConfig::default());
        // 先设置历史值
        v.validate(&make_action(0.0, 0.0, 0.0, 1.0), None, false);
        // v2.4 模式：q_batt_set 变化不受限（由实时模块控制）
        let (a, _violations) = v.validate(&make_action(100.0, 200.0, 0.0, 1.0), None, false);
        assert_eq!(a.q_batt_set, 200.0); // q_batt_set 未被 clamp
    }

    #[test]
    fn test_v2_4_mode_applies_p_batt_only() {
        let v = ActionValidator::new_v2_4(ActionConstraintConfig::default());
        let (a, violations) = v.validate(&make_action(600.0, 0.0, 0.0, 1.0), None, false);
        // v2.4: p_batt_set clamp 到 S_max
        assert!(a.p_batt_set.abs() <= 500.0);
        assert!(violations.iter().any(|r| r.rule == "ACT-03"));
    }

    #[test]
    fn test_no_violations_for_valid_action() {
        let v = ActionValidator::new(ActionConstraintConfig::default());
        let (_a, violations) = v.validate(&make_action(100.0, 50.0, 0.0, 1.0), None, false);
        assert!(!violations.iter().any(|r| r.rule == "ACT-03"));
        assert!(!violations.iter().any(|r| r.rule == "ACT-04"));
    }
}
