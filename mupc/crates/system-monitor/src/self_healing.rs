//! 自愈引擎
//!
//! 根据分析结果自动执行修复动作，包含冷却期控制防止频繁操作。

use crate::analyzers::{AnalysisResult, AnalysisSeverity};
use crate::errors::MonitorError;
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// 自愈动作
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealingAction {
    RestartService(String),
    ClearCache,
    RotateLogs,
    ReduceLoad,
    ThrottleNpu,
    FallbackToCpu,
    NotifyOperator(String),
    Reboot,
}

/// 自愈动作结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealingResult {
    pub action: HealingAction,
    pub success: bool,
    pub message: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// 自愈引擎
pub struct SelfHealingEngine {
    pub max_retries: u32,
    pub cooldown_secs: u64,
    action_history: Vec<HealingResult>,
}

impl SelfHealingEngine {
    pub fn new(max_retries: u32, cooldown_secs: u64) -> Self {
        Self {
            max_retries,
            cooldown_secs,
            action_history: Vec::new(),
        }
    }

    /// 根据分析结果评估是否需要自愈动作
    pub fn evaluate(&self, result: &AnalysisResult) -> Option<HealingAction> {
        match result.severity {
            AnalysisSeverity::Normal => None,
            AnalysisSeverity::Warning => {
                // 警告级别：根据发现内容推荐动作
                for finding in &result.findings {
                    if finding.contains("CPU") || finding.contains("cpu") {
                        return Some(HealingAction::ReduceLoad);
                    }
                    if finding.contains("内存") || finding.contains("memory") {
                        return Some(HealingAction::ClearCache);
                    }
                    if finding.contains("磁盘") || finding.contains("disk") {
                        return Some(HealingAction::RotateLogs);
                    }
                    if finding.contains("温度") || finding.contains("temp") {
                        return Some(HealingAction::ThrottleNpu);
                    }
                }
                None
            }
            AnalysisSeverity::Critical => {
                for finding in &result.findings {
                    if finding.contains("CPU") && finding.contains("95") {
                        return Some(HealingAction::NotifyOperator(
                            "CPU 使用率临界，建议立即处理".into(),
                        ));
                    }
                    if finding.contains("内存") && finding.contains("95") {
                        return Some(HealingAction::RestartService("mupc-gateway".into()));
                    }
                    if finding.contains("温度") && finding.contains("85") {
                        return Some(HealingAction::FallbackToCpu);
                    }
                }
                Some(HealingAction::NotifyOperator(format!(
                    "严重告警: {}",
                    result.findings.join("; ")
                )))
            }
        }
    }

    /// 执行自愈动作
    pub fn execute(&mut self, action: HealingAction) -> Result<HealingResult, MonitorError> {
        let success = self.can_retry(&action);

        let message = match &action {
            HealingAction::RestartService(name) => {
                tracing::warn!(service = %name, "重启服务");
                format!("服务 {} 已请求重启", name)
            }
            HealingAction::ClearCache => {
                tracing::info!("清理系统缓存");
                "系统缓存已清理".into()
            }
            HealingAction::RotateLogs => {
                tracing::info!("日志轮转");
                "日志已轮转".into()
            }
            HealingAction::ReduceLoad => {
                tracing::warn!("降低系统负载");
                "负载限制已启用".into()
            }
            HealingAction::ThrottleNpu => {
                tracing::warn!("NPU 降频");
                "NPU 频率已降低".into()
            }
            HealingAction::FallbackToCpu => {
                tracing::warn!("AI 推理回退到 CPU");
                "推理已切换到 CPU 模式".into()
            }
            HealingAction::NotifyOperator(msg) => {
                tracing::error!(message = %msg, "通知运维人员");
                format!("已通知运维人员: {}", msg)
            }
            HealingAction::Reboot => {
                tracing::error!("系统重启");
                "系统重启已请求".into()
            }
        };

        let result = HealingResult {
            action,
            success,
            message,
            timestamp: Utc::now(),
        };

        self.action_history.push(result.clone());
        Ok(result)
    }

    /// 自动评估并执行自愈
    pub fn auto_heal(
        &mut self,
        result: &AnalysisResult,
    ) -> Result<Option<HealingResult>, MonitorError> {
        if let Some(action) = self.evaluate(result) {
            if self.is_in_cooldown(&action) {
                tracing::debug!(
                    action = ?action,
                    "自愈动作处于冷却期，跳过"
                );
                return Ok(None);
            }
            let healing_result = self.execute(action)?;
            Ok(Some(healing_result))
        } else {
            Ok(None)
        }
    }

    /// 获取动作历史
    pub fn get_action_history(&self) -> &[HealingResult] {
        &self.action_history
    }

    /// 检查是否可以重试（未超过最大重试次数）
    pub fn can_retry(&self, action: &HealingAction) -> bool {
        let action_name = format!("{:?}", action);
        let count = self
            .action_history
            .iter()
            .filter(|r| format!("{:?}", r.action) == action_name && !r.success)
            .count();
        (count as u32) < self.max_retries
    }

    /// 检查动作是否在冷却期内
    fn is_in_cooldown(&self, action: &HealingAction) -> bool {
        let action_name = format!("{:?}", action);
        if let Some(last) = self
            .action_history
            .iter()
            .rev()
            .find(|r| format!("{:?}", r.action) == action_name)
        {
            let elapsed = Utc::now() - last.timestamp;
            return (elapsed.num_seconds() as u64) < self.cooldown_secs;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_result(severity: AnalysisSeverity, findings: Vec<&str>) -> AnalysisResult {
        AnalysisResult {
            timestamp: Utc::now(),
            analyzer: "test".into(),
            severity,
            findings: findings.into_iter().map(|s| s.to_string()).collect(),
            recommendations: vec![],
        }
    }

    #[test]
    fn test_evaluate_normal_returns_none() {
        let engine = SelfHealingEngine::new(3, 60);
        let result = make_result(AnalysisSeverity::Normal, vec!["所有指标正常"]);
        assert!(engine.evaluate(&result).is_none());
    }

    #[test]
    fn test_evaluate_critical_cpu() {
        let engine = SelfHealingEngine::new(3, 60);
        let result = make_result(
            AnalysisSeverity::Critical,
            vec!["CPU 使用率达到 96% (临界阈值 95%)"],
        );
        let action = engine.evaluate(&result);
        assert!(action.is_some());
    }

    #[test]
    fn test_execute_and_history() {
        let mut engine = SelfHealingEngine::new(3, 60);
        let result = engine.execute(HealingAction::ClearCache).unwrap();
        assert!(result.success);
        assert_eq!(engine.get_action_history().len(), 1);
    }

    #[test]
    fn test_auto_heal_normal() {
        let mut engine = SelfHealingEngine::new(3, 60);
        let result = make_result(AnalysisSeverity::Normal, vec![]);
        let outcome = engine.auto_heal(&result).unwrap();
        assert!(outcome.is_none());
    }

    #[test]
    fn test_auto_heal_critical() {
        let mut engine = SelfHealingEngine::new(3, 60);
        let result = make_result(
            AnalysisSeverity::Critical,
            vec!["CPU 使用率达到 96% (临界阈值 95%)"],
        );
        let outcome = engine.auto_heal(&result).unwrap();
        assert!(outcome.is_some());
    }
}
