//! 审计日志查询端点
//!
//! GET /api/v1/ai/audit — 查询干预操作审计日志

use axum::{Json, extract::{State, Query}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub operator: Option<String>,
    pub action_type: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct AuditResponse {
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
    pub items: Vec<AuditEntry>,
    pub export_supported: bool,
}

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
pub async fn get_audit_log(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AuditQuery>,
) -> Json<AuditResponse> {
    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(20) as usize;

    let end = chrono::Utc::now();
    let start = end - chrono::Duration::days(7);

    let entries = state.audit_logger.query(start, end, None).await
        .unwrap_or_default();

    let total = entries.len() as u64;
    let skip = ((page - 1) as usize * page_size).min(entries.len());
    let items: Vec<AuditEntry> = entries.into_iter()
        .skip(skip)
        .take(page_size)
        .map(|e| AuditEntry {
            id: e.id,
            timestamp: e.timestamp.to_rfc3339(),
            operator: e.user,
            action_type: e.action,
            action_detail: serde_json::json!({"resource": e.resource, "method": e.method}),
            result: "success".to_string(),
            fail_reason: None,
            source_ip: String::new(),
        })
        .collect();

    Json(AuditResponse {
        total,
        page,
        page_size: page_size as u32,
        items,
        export_supported: true,
    })
}
