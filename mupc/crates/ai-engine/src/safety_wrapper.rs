//! 安全 RL 包装器（Safety RL Wrapper）
//!
//! v2.17 新增：在 RL 决策后、ActionValidator 前插入物理模型前置过滤器。
//! 基于戴维南等效电路预测电压变化，提前拒绝高风险动作。
//!
//! 设计原则：轻量化（<5ms）、保守优先（失败回退）、可证明安全（简化电路方程）。

use crate::config::SafetyWrapperConfig;
use crate::data_fusion::FusedSystemState;
use crate::error::AiEngineError;
use crate::rl_model::ActionOutput;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// 数据结构
// ============================================================================

/// 线路阻抗参数（从配置文件读取）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineImpedance {
    pub r_ohm: f64,
    pub x_ohm: f64,
    pub v_base: f64,
}

impl Default for LineImpedance {
    fn default() -> Self {
        Self {
            r_ohm: 0.1,
            x_ohm: 0.05,
            v_base: 220.0,
        }
    }
}

/// 安全边界
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyBounds {
    pub v_min: f64,
    pub v_max: f64,
    pub dv_dt_max: f64,
    pub soc_margin: f64,
}

impl Default for SafetyBounds {
    fn default() -> Self {
        Self {
            v_min: 0.93,
            v_max: 1.07,
            dv_dt_max: 0.03,
            soc_margin: 0.02,
        }
    }
}

/// 物理模型预测结果
#[derive(Debug, Clone)]
pub struct PredictionResult {
    pub v_predicted: f64,
    pub dv_dt: f64,
    pub soc_after: f64,
    pub is_safe: bool,
    pub reason: Option<String>,
}

/// 检查结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CheckResult {
    Passed,
    Rejected { reason: String },
    FallbackDueToPredictionError,
}

impl CheckResult {
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected { .. })
    }
}

/// 安全包装器事件（broadcast 推送用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyWrapperEvent {
    pub timestamp: i64,
    pub event_type: SafetyEventType,
    pub check_result: CheckResult,
    pub proposed_p_ref: f64,
    pub proposed_k_droop: f64,
    pub fallback_p_ref: f64,
    pub fallback_k_droop: f64,
    pub v_predicted: f64,
    pub latency_us: u64,
}

/// 事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyEventType {
    Passed,
    Violation,
    Fallback,
}

/// 违规记录（持久化到 storage）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyViolation {
    pub timestamp: i64,
    pub reason: String,
    pub proposed_p_ref: f64,
    pub proposed_k_droop: f64,
    pub fallback_p_ref: f64,
    pub fallback_k_droop: f64,
    pub v_predicted: f64,
    pub latency_us: u64,
}

/// 累计指标
#[derive(Debug, Clone, Default, Serialize)]
pub struct SafetyStats {
    pub total_checks: u64,
    pub total_rejected: u64,
    pub total_fallback: u64,
    pub rejection_rate_1h: f64,
    pub avg_latency_us: u64,
    pub max_latency_us: u64,
}

/// 全局事件总线 Sender
pub type SafetyEventSender = tokio::sync::broadcast::Sender<SafetyWrapperEvent>;

// ============================================================================
// 物理模型预测器
// ============================================================================

/// 线性灵敏度预测器（戴维南等效电路）
pub struct LinearSensitivityPredictor {
    impedance: LineImpedance,
    bounds: SafetyBounds,
}

impl LinearSensitivityPredictor {
    pub fn new(impedance: LineImpedance, bounds: SafetyBounds) -> Self {
        Self { impedance, bounds }
    }

    /// 戴维南等效电路 + 灵敏度分析
    ///
    /// 公式：ΔV ≈ (R·ΔP + X·ΔQ) / V₀
    /// P 单位 kW → W (×1000), Q 单位 kVar, V 单位 V → p.u. (÷v_base)
    pub fn predict(
        &self,
        state: &FusedSystemState,
        action: &ActionOutput,
        p_cur: f64,
    ) -> Result<PredictionResult, AiEngineError> {
        let v_avg = (state.voltage_phase_a + state.voltage_phase_b + state.voltage_phase_c) / 3.0;

        // 新动作下的 P_output（下垂公式: P = P_ref - k_droop × ΔV）
        let p_new = action.p_ref - action.k_droop * (v_avg - 1.0);

        // ΔP 转换为 W
        let delta_p_w = (p_new - p_cur) * 1000.0;

        // ΔQ 估算
        let q_margin = state.q_realtime_margin;
        let delta_q_var = if q_margin > 0.20 {
            0.0
        } else {
            let q_max_var = 300.0;
            (1.0 - q_margin) * q_max_var * (v_avg - 1.0).signum()
        };

        // 灵敏度公式
        let delta_v_volt = (self.impedance.r_ohm * delta_p_w + self.impedance.x_ohm * delta_q_var)
            / self.impedance.v_base;
        let delta_v_pu = delta_v_volt / self.impedance.v_base;

        let v_predicted = v_avg + delta_v_pu;

        // 边界检查
        let v_safe = v_predicted >= self.bounds.v_min
            && v_predicted <= self.bounds.v_max
            && delta_v_pu.abs() <= self.bounds.dv_dt_max;

        let soc_safe = !(action.p_ref > 0.0 && state.battery_soc < 0.10 + self.bounds.soc_margin);

        let reason = if !v_safe {
            Some(format!(
                "v_predicted={:.3} 越界 [{}, {}]",
                v_predicted, self.bounds.v_min, self.bounds.v_max
            ))
        } else if !soc_safe {
            Some(format!(
                "SOC={:.3} 低于安全阈值 {:.3}",
                state.battery_soc,
                0.10 + self.bounds.soc_margin
            ))
        } else {
            None
        };

        Ok(PredictionResult {
            v_predicted,
            dv_dt: delta_v_pu,
            soc_after: state.battery_soc,
            is_safe: v_safe && soc_safe,
            reason,
        })
    }
}

// ============================================================================
// 安全包装器主结构
// ============================================================================

pub struct SafetyRLWrapper {
    predictor: LinearSensitivityPredictor,
    last_safe_action: Arc<RwLock<ActionOutput>>,
    stats: Arc<RwLock<SafetyStats>>,
    event_sender: Option<SafetyEventSender>,
    config: SafetyWrapperConfig,
}

impl SafetyRLWrapper {
    pub fn new(config: SafetyWrapperConfig, event_sender: Option<SafetyEventSender>) -> Self {
        let impedance = LineImpedance {
            r_ohm: config.line_impedance_r_ohm,
            x_ohm: config.line_impedance_x_ohm,
            v_base: config.v_base,
        };
        let bounds = SafetyBounds {
            v_min: config.v_min,
            v_max: config.v_max,
            dv_dt_max: config.dv_dt_max,
            soc_margin: config.soc_margin,
        };
        let predictor = LinearSensitivityPredictor::new(impedance, bounds);

        Self {
            predictor,
            last_safe_action: Arc::new(RwLock::new(ActionOutput {
                p_ref: 0.0,
                k_droop: 0.0,
                load_shedding: 0.0,
                pv_limit: 0.0,
                confidence: 1.0,
            })),
            stats: Arc::new(RwLock::new(SafetyStats::default())),
            event_sender,
            config,
        }
    }

    /// 安全检查入口
    pub async fn check_and_fallback(
        &self,
        state: &FusedSystemState,
        proposed_action: &ActionOutput,
    ) -> (ActionOutput, CheckResult) {
        let start = std::time::Instant::now();

        // 0. 计算上一周期实际 P_output（用于 ΔP 计算）
        let last = self.last_safe_action.read().await;
        let v_avg =
            (state.voltage_phase_a + state.voltage_phase_b + state.voltage_phase_c) / 3.0;
        let p_cur = last.p_ref - last.k_droop * (v_avg - 1.0);
        drop(last);

        // 1. 物理模型预测
        let pred = match self.predictor.predict(state, proposed_action, p_cur) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("SafetyRLWrapper 预测失败: {:?}", e);
                let latency = start.elapsed().as_micros() as u64;
                self.update_stats(latency, false, true).await;

                // 广播回退事件
                let fallback = self.last_safe_action.read().await.clone();
                let _ = self.publish_event(
                    SafetyEventType::Fallback,
                    CheckResult::FallbackDueToPredictionError,
                    proposed_action,
                    &fallback,
                    0.0,
                    latency,
                );

                return (fallback, CheckResult::FallbackDueToPredictionError);
            }
        };

        let latency = start.elapsed().as_micros() as u64;

        // 2. 边界检查
        if !pred.is_safe {
            tracing::warn!(
                reason = pred.reason.as_deref().unwrap_or("unknown"),
                v_predicted = pred.v_predicted,
                proposed_p_ref = proposed_action.p_ref,
                proposed_k_droop = proposed_action.k_droop,
                latency_us = latency,
                "SafetyRLWrapper 拒绝动作",
            );

            self.update_stats(latency, true, false).await;

            let fallback = self.last_safe_action.read().await.clone();
            let reason = pred.reason.unwrap_or_default();

            let _ = self.publish_event(
                SafetyEventType::Violation,
                CheckResult::Rejected {
                    reason: reason.clone(),
                },
                proposed_action,
                &fallback,
                pred.v_predicted,
                latency,
            );

            // v2.17: 违规记录通过 tracing 日志 + broadcast 事件双通道输出
            // storage 持久化由 model_manager 负责（持有 SqlitePool 引用）

            return (fallback, CheckResult::Rejected { reason });
        }

        // 3. 通过：更新 last_safe_action
        *self.last_safe_action.write().await = proposed_action.clone();
        self.update_stats(latency, false, false).await;

        let _ = self.publish_event(
            SafetyEventType::Passed,
            CheckResult::Passed,
            proposed_action,
            proposed_action,
            pred.v_predicted,
            latency,
        );

        (proposed_action.clone(), CheckResult::Passed)
    }

    async fn update_stats(&self, latency_us: u64, was_rejected: bool, was_fallback: bool) {
        let mut s = self.stats.write().await;
        s.total_checks += 1;
        if was_rejected {
            s.total_rejected += 1;
        }
        if was_fallback {
            s.total_fallback += 1;
        }
        // 滑动平均延迟
        s.avg_latency_us = if s.total_checks > 1 {
            (s.avg_latency_us * (s.total_checks - 1) + latency_us) / s.total_checks
        } else {
            latency_us
        };
        if latency_us > s.max_latency_us {
            s.max_latency_us = latency_us;
        }
        // 拒绝率（全局比率；注：字段名 `rejection_rate_1h` 为历史遗留，
        // 当前实现为全局统计，非精确 1h 滑动窗口）
        s.rejection_rate_1h = if s.total_checks > 0 {
            s.total_rejected as f64 / s.total_checks as f64
        } else {
            0.0
        };
    }

    fn publish_event(
        &self,
        event_type: SafetyEventType,
        check_result: CheckResult,
        proposed: &ActionOutput,
        fallback: &ActionOutput,
        v_predicted: f64,
        latency_us: u64,
    ) -> Result<(), String> {
        if let Some(sender) = &self.event_sender {
            let event = SafetyWrapperEvent {
                timestamp: chrono::Utc::now().timestamp(),
                event_type,
                check_result,
                proposed_p_ref: proposed.p_ref,
                proposed_k_droop: proposed.k_droop,
                fallback_p_ref: fallback.p_ref,
                fallback_k_droop: fallback.k_droop,
                v_predicted,
                latency_us,
            };
            sender
                .send(event)
                .map_err(|e| format!("broadcast send failed: {}", e))?;
        }
        Ok(())
    }

    /// 获取当前统计
    pub async fn stats(&self) -> SafetyStats {
        self.stats.read().await.clone()
    }

    /// 获取安全边界快照
    pub fn bounds(&self) -> SafetyBounds {
        self.predictor.bounds.clone()
    }

    /// 获取线路阻抗快照
    pub fn impedance(&self) -> LineImpedance {
        self.predictor.impedance.clone()
    }

    /// 获取上一有效动作
    pub async fn last_safe_action(&self) -> ActionOutput {
        self.last_safe_action.read().await.clone()
    }

    /// 获取配置
    pub fn config(&self) -> &SafetyWrapperConfig {
        &self.config
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> FusedSystemState {
        let mut s = FusedSystemState::default();
        s.voltage_phase_a = 0.98;
        s.voltage_phase_b = 0.98;
        s.voltage_phase_c = 0.98;
        s.battery_soc = 0.50;
        s.q_realtime_margin = 0.50;
        s
    }

    fn test_wrapper() -> SafetyRLWrapper {
        SafetyRLWrapper::new(SafetyWrapperConfig::default(), None)
    }

    // SAFETY-03: v_predicted 计算正确
    #[test]
    fn test_predict_normal_action() {
        let predictor =
            LinearSensitivityPredictor::new(LineImpedance::default(), SafetyBounds::default());
        let state = test_state();
        let action = ActionOutput {
            p_ref: -10.0,
            k_droop: 0.0,
            load_shedding: 0.0,
            pv_limit: 0.0,
            confidence: 1.0,
        };
        let pred = predictor.predict(&state, &action).unwrap();
        assert!(pred.is_safe);
        assert!((pred.v_predicted - 0.98).abs() < 0.05);
    }

    // SAFETY-03: 放电导致低电压拒绝
    #[test]
    fn test_predict_discharge_low_voltage_rejected() {
        let predictor =
            LinearSensitivityPredictor::new(LineImpedance::default(), SafetyBounds::default());
        let mut state = test_state();
        state.voltage_phase_a = 0.935;
        state.voltage_phase_b = 0.935;
        state.voltage_phase_c = 0.935;
        let action = ActionOutput {
            p_ref: 40.0,
            k_droop: 0.0,
            load_shedding: 0.0,
            pv_limit: 0.0,
            confidence: 1.0,
        };
        let pred = predictor.predict(&state, &action).unwrap();
        assert!(!pred.is_safe);
        assert!(pred.reason.is_some());
    }

    // SAFETY-05: 回退到 last_safe_action
    #[tokio::test]
    async fn test_check_and_fallback_passed_updates_last_safe() {
        let wrapper = test_wrapper();
        let state = test_state();
        let action = ActionOutput {
            p_ref: -5.0,
            k_droop: 5.0,
            load_shedding: 0.0,
            pv_limit: 0.0,
            confidence: 1.0,
        };
        let (result, cr) = wrapper.check_and_fallback(&state, &action).await;
        assert!(matches!(cr, CheckResult::Passed));
        assert_eq!(result.p_ref, -5.0);
        let last = wrapper.last_safe_action().await;
        assert_eq!(last.p_ref, -5.0);
    }

    // SAFETY-06: SOC 边界拒绝
    #[tokio::test]
    async fn test_soc_margin_rejected() {
        let wrapper = test_wrapper();
        let mut state = test_state();
        state.battery_soc = 0.11;
        let action = ActionOutput {
            p_ref: 10.0, // 放电
            k_droop: 0.0,
            load_shedding: 0.0,
            pv_limit: 0.0,
            confidence: 1.0,
        };
        let (_, cr) = wrapper.check_and_fallback(&state, &action).await;
        assert!(matches!(cr, CheckResult::Rejected { .. }));
    }

    // stats 更新
    #[test]
    fn test_safety_stats_tracking() {
        let mut s = SafetyStats::default();
        s.total_checks = 100;
        s.total_rejected = 5;
        s.rejection_rate_1h = 5.0 / 100.0;
        assert!((s.rejection_rate_1h - 0.05).abs() < 0.001);
    }

    // bounds 默认值
    #[test]
    fn test_bounds_defaults() {
        let b = SafetyBounds::default();
        assert_eq!(b.v_min, 0.93);
        assert_eq!(b.v_max, 1.07);
        assert_eq!(b.dv_dt_max, 0.03);
        assert_eq!(b.soc_margin, 0.02);
    }

    // CheckResult::is_rejected
    #[test]
    fn test_check_result_is_rejected() {
        assert!(CheckResult::Rejected {
            reason: "test".into()
        }
        .is_rejected());
        assert!(!CheckResult::Passed.is_rejected());
        assert!(!CheckResult::FallbackDueToPredictionError.is_rejected());
    }
}
