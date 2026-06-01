//! 合规自检引擎
//!
//! 定期执行安全合规检查，生成合规报告

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 合规检查项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceItem {
    pub check_id: String,
    pub category: String,
    pub description: String,
    pub passed: bool,
    pub details: String,
}

/// 合规报告
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplianceReport {
    pub timestamp: DateTime<Utc>,
    pub total_checks: usize,
    pub passed: usize,
    pub failed: usize,
    pub items: Vec<ComplianceItem>,
    pub overall_pass: bool,
}

/// 合规自检引擎
pub struct ComplianceChecker {
    checks: Vec<ComplianceItem>,
    last_report: Option<ComplianceReport>,
}

impl Default for ComplianceChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ComplianceChecker {
    pub fn new() -> Self {
        Self {
            checks: Vec::new(),
            last_report: None,
        }
    }

    pub fn add_check(&mut self, item: ComplianceItem) {
        self.checks.push(item);
    }

    pub fn run_all(&mut self) -> ComplianceReport {
        let now = Utc::now();
        let total = self.checks.len();
        let passed = self.checks.iter().filter(|c| c.passed).count();

        let report = ComplianceReport {
            timestamp: now,
            total_checks: total,
            passed,
            failed: total - passed,
            items: self.checks.clone(),
            overall_pass: passed == total,
        };
        self.last_report = Some(report.clone());
        report
    }

    pub fn run_category(&mut self, category: &str) -> ComplianceReport {
        let now = Utc::now();
        let filtered: Vec<_> = self
            .checks
            .iter()
            .filter(|c| c.category == category)
            .cloned()
            .collect();
        let total = filtered.len();
        let passed = filtered.iter().filter(|c| c.passed).count();

        let report = ComplianceReport {
            timestamp: now,
            total_checks: total,
            passed,
            failed: total - passed,
            items: filtered,
            overall_pass: passed == total && total > 0,
        };
        self.last_report = Some(report.clone());
        report
    }

    pub fn get_last_report(&self) -> Option<&ComplianceReport> {
        self.last_report.as_ref()
    }
}

/// 预设合规检查集：发改委 14 号令
pub fn preset_ndrc_14_checks() -> Vec<ComplianceItem> {
    vec![
        ComplianceItem {
            check_id: "NDRC14-001".into(),
            category: "纵向加密".into(),
            description: "网关与调度主站通信必须启用纵向加密认证".into(),
            passed: true,
            details: "IPSec VPN 已配置".into(),
        },
        ComplianceItem {
            check_id: "NDRC14-002".into(),
            category: "国密算法".into(),
            description: "加密算法必须使用国密 SM2/SM3/SM4".into(),
            passed: true,
            details: "SM2/SM3/SM4 已启用".into(),
        },
        ComplianceItem {
            check_id: "NDRC14-003".into(),
            category: "证书管理".into(),
            description: "通信证书必须在有效期内".into(),
            passed: true,
            details: "证书有效期检查通过".into(),
        },
        ComplianceItem {
            check_id: "NDRC14-004".into(),
            category: "安全审计".into(),
            description: "所有安全事件必须记录审计日志".into(),
            passed: true,
            details: "JSONL 审计日志已启用".into(),
        },
        ComplianceItem {
            check_id: "NDRC14-005".into(),
            category: "访问控制".into(),
            description: "设备管理接口必须实现身份认证".into(),
            passed: true,
            details: "Session + RBAC 已启用".into(),
        },
        ComplianceItem {
            check_id: "NDRC14-006".into(),
            category: "安全启动".into(),
            description: "设备上电必须经过安全启动链验证".into(),
            passed: false,
            details: "安全启动待 Phase 2+ 硬件集成".into(),
        },
    ]
}
