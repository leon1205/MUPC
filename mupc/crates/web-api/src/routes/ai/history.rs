//! 历史记录端点
//!
//! 查询决策历史和预测准确率历史

use axum::{Json, extract::{State, Query}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct DecisionHistoryResponse {
    pub total: u64,
    pub page: u32,
    pub items: Vec<DecisionHistoryItem>,
}

#[derive(Debug, Serialize)]
pub struct DecisionHistoryItem {
    pub id: String,
    pub timestamp: String,
    pub mode: String,
    pub action_summary: String,
    pub reward: f64,
}

#[derive(Debug, Serialize)]
pub struct AccuracyHistoryResponse {
    pub items: Vec<AccuracyPoint>,
    pub avg_accuracy: f64,
}

#[derive(Debug, Serialize)]
pub struct AccuracyPoint {
    pub timestamp: String,
    pub accuracy: f64,
    pub sample_count: u32,
}

/// GET /api/v1/ai/history/decisions
pub async fn get_decision_history(
    State(state): State<Arc<AppState>>,
    Query(query): Query<HistoryQuery>,
) -> Json<DecisionHistoryResponse> {
    let _ = state.ai_integrator;
    let page = query.page.unwrap_or(1);

    Json(DecisionHistoryResponse {
        total: 0,
        page,
        items: vec![],
    })
}

/// GET /api/v1/ai/history/accuracy
pub async fn get_prediction_accuracy_history(
    State(state): State<Arc<AppState>>,
) -> Json<AccuracyHistoryResponse> {
    let _ = state.ai_integrator;

    Json(AccuracyHistoryResponse {
        items: vec![],
        avg_accuracy: 0.0,
    })
}
