//! 专家干预端点
//!
//! 查询人工专家干预记录

use axum::{Json, extract::{State, Query}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

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
    let _ = state;
    let page = query.page.unwrap_or(1);

    Json(InterventionResponse {
        total: 0,
        page,
        items: vec![],
    })
}
