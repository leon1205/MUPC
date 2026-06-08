//! 专家干预端点
//!
//! 查询人工专家干预记录

use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct InterventionQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct InterventionResponse {
    pub total: u64,
    pub page: u32,
    pub items: Vec<InterventionItem>,
}

#[derive(Debug, Serialize)]
pub struct InterventionItem {
    pub id: String,
    pub timestamp: String,
    pub operator: String,
    pub action_type: String,
    pub description: String,
    pub result: String,
}

/// GET /api/v1/ai/interventions
pub async fn get_interventions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<InterventionQuery>,
) -> Json<InterventionResponse> {
    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(20) as usize;

    let end = chrono::Utc::now();
    let start = end - chrono::Duration::days(30);
    let events = state
        .storage
        .events
        .query_range(start, end)
        .await
        .map_err(|e| {
            tracing::error!(%e, "查询干预记录失败");
            e
        })
        .unwrap_or_default();

    let intervention_events: Vec<&_> = events
        .iter()
        .filter(|e| e.event_type.contains("intervention") || e.event_type.contains("user_action"))
        .collect();

    let total = intervention_events.len() as u64;
    let skip = ((page - 1) as usize * page_size).min(intervention_events.len());
    let items: Vec<InterventionItem> = intervention_events
        .into_iter()
        .skip(skip)
        .take(page_size)
        .map(|e| InterventionItem {
            id: e.id.map(|i| i.to_string()).unwrap_or_default(),
            timestamp: e.timestamp.to_rfc3339(),
            operator: e.source.clone(),
            action_type: e.event_type.clone(),
            description: e.message.clone(),
            result: "success".to_string(),
        })
        .collect();

    Json(InterventionResponse { total, page, items })
}
