//! 审计日志查询端点
//!
//! GET /api/v1/ai/audit — 查询干预操作审计日志

use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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

fn parse_time(
    s: &Option<String>,
    fallback: chrono::DateTime<chrono::Utc>,
) -> chrono::DateTime<chrono::Utc> {
    s.as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or(fallback)
}

/// GET /api/v1/ai/audit
pub async fn get_audit_log(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AuditQuery>,
) -> Json<AuditResponse> {
    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(20) as usize;

    let end = parse_time(&query.end, chrono::Utc::now());
    let start = parse_time(&query.start, end - chrono::Duration::days(7));

    let user_filter = query.operator.as_deref();
    let entries = state
        .audit_logger
        .query(start, end, user_filter)
        .await
        .map_err(|e| {
            tracing::error!(%e, "查询审计日志失败");
            e
        })
        .unwrap_or_default();

    let action_filter = query.action_type.as_deref();
    let filtered: Vec<_> = entries
        .into_iter()
        .filter(|e| action_filter.map_or(true, |af| e.action.contains(af)))
        .collect();

    let total = filtered.len() as u64;
    let skip = ((page - 1) as usize * page_size).min(filtered.len());
    let items: Vec<AuditEntry> = filtered
        .into_iter()
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
