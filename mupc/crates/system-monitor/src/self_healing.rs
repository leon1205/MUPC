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
    ///
    /// # ⚠️ 当前**全部动作均未实现**（framework-only）
    /// 本引擎只做"**评估 + 登记**"：没有任何动作真的被执行（重启服务 / 清缓存 / 轮转日志 /
    /// 限载 / NPU 降频 / 回退 CPU / 重启都要 OS 或其它 crate 的接口，尚未接线）。
    /// 故一律 `success: false`，文案明写"未实现 / 已登记"。
    ///
    /// **不得**回到 `success: true` + "已清理/已重启"这类谎报：那会让 PRD 07 §6.2 的
    /// "超限处置 10 s 内执行"看起来**已被满足**，而事实上什么都没做（判别力用例
    /// `unimplemented_actions_never_report_success_or_claim_execution` 钉住这一点）。
    pub fn execute(&mut self, action: HealingAction) -> Result<HealingResult, MonitorError> {
        // 动作明细（人读；**不**声称已执行）
        let detail = match &action {
            HealingAction::RestartService(name) => format!("重启服务 {name}"),
            HealingAction::ClearCache => "清理系统缓存".to_string(),
            HealingAction::RotateLogs => "日志轮转".to_string(),
            HealingAction::ReduceLoad => "降低系统负载".to_string(),
            HealingAction::ThrottleNpu => "NPU 降频".to_string(),
            HealingAction::FallbackToCpu => "AI 推理回退到 CPU".to_string(),
            HealingAction::NotifyOperator(msg) => format!("通知运维人员: {msg}"),
            HealingAction::Reboot => "系统重启".to_string(),
        };

        // 重试预算仍被记录/查询（`can_retry` 的语义面保留），但它**不**代表动作成功。
        let retryable = self.can_retry(&action);
        let message = format!(
            "【未实现】{detail} —— 已登记，未执行（自愈动作尚未接线）{}",
            if retryable { "" } else { "；重试预算已耗尽" }
        );

        tracing::warn!(
            action = ?action,
            retryable,
            "自愈动作未实现：仅登记，未执行任何处置（success=false）"
        );

        let result = HealingResult {
            action,
            success: false,
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
        // 旧实现断言 `result.success` 为真 —— 那是谎报（动作根本没接线）。改为如实断言。
        assert!(!result.success, "未实现的动作不得报成功");
        assert_eq!(engine.get_action_history().len(), 1);
    }

    /// 判别力：**所有**未实现动作都不得报成功，也不得在文案里声称"已执行"。
    ///
    /// 旧实现（`success = can_retry(...)` = true + "系统缓存已清理" 之类）⇒ 本用例红。
    #[test]
    fn unimplemented_actions_never_report_success_or_claim_execution() {
        let mut engine = SelfHealingEngine::new(3, 60);
        let actions = [
            HealingAction::RestartService("mupc-gateway".into()),
            HealingAction::ClearCache,
            HealingAction::RotateLogs,
            HealingAction::ReduceLoad,
            HealingAction::ThrottleNpu,
            HealingAction::FallbackToCpu,
            HealingAction::NotifyOperator("CPU 临界".into()),
            HealingAction::Reboot,
        ];
        for action in actions {
            let r = engine.execute(action).unwrap();
            assert!(!r.success, "未实现的动作不得 success=true: {:?}", r.action);
            assert!(
                r.message.contains("未实现") && r.message.contains("未执行"),
                "文案必须明写未实现/未执行: {}",
                r.message
            );
            for banned in [
                "已清理", "已轮转", "已启用", "已降低", "已切换", "已通知", "已请求", "已执行",
            ] {
                assert!(
                    !r.message.contains(banned),
                    "文案不得声称已执行（含「{banned}」）: {}",
                    r.message
                );
            }
        }
        assert_eq!(engine.get_action_history().len(), 8);
    }

    /// 冷却期仍须生效（不得因"动作未实现"就把节流逻辑一起丢掉）。
    #[test]
    fn cooldown_still_throttles_repeated_attempts() {
        let mut engine = SelfHealingEngine::new(3, 60);
        let result = make_result(
            AnalysisSeverity::Critical,
            vec!["CPU 使用率达到 96% (临界阈值 95%)"],
        );
        assert!(engine.auto_heal(&result).unwrap().is_some(), "首次登记");
        assert!(
            engine.auto_heal(&result).unwrap().is_none(),
            "冷却期内不得重复登记"
        );
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
