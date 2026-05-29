//! 审计日志查询端点
//!
//! GET /api/v1/ai/audit — 查询干预操作审计日志

use axum::{Json, extract::Query};
use serde::{Deserialize, Serialize};

/// 审计查询参数
#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub operator: Option<String>,
    pub action_type: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

/// 审计分页响应
#[derive(Debug, Serialize)]
pub struct AuditResponse {
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
    pub items: Vec<AuditEntry>,
    pub export_supported: bool,
}

/// 审计条目
#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp: String,
    pub operator: String,
    pub action_type: String,
    pub action_detail: serde_json::Value,
    pub result: String,
    pub fail_reason: Option<String>,
    pub source_ip: String,
}

/// GET /api/v1/ai/audit
///
/// action_type 枚举: weight_adjust | mode_switch | ab_test_start | ab_test_stop | model_rollback
pub async fn get_audit_log(
    Query(_query): Query<AuditQuery>,
) -> Json<AuditResponse> {
    todo!("Phase 2+ — 从 audit storage 查询并分页返回")
}
