//! 历史记录端点
//!
//! 查询决策历史和预测准确率历史

use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(20) as usize;
    let limit = page_size * 5; // fetch more for in-memory pagination

    let records = state
        .storage
        .decisions
        .query_recent(limit)
        .await
        .map_err(|e| {
            tracing::error!(%e, "查询决策历史失败");
            e
        })
        .unwrap_or_default();

    let total = records.len() as u64;
    let skip = ((page - 1) as usize * page_size).min(records.len());
    let items: Vec<DecisionHistoryItem> = records
        .into_iter()
        .skip(skip)
        .take(page_size)
        .map(|r| {
            let action: serde_json::Value =
                serde_json::from_str(&r.action_json).unwrap_or_default();
            let action_summary = action
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let reward = action.get("reward").and_then(|v| v.as_f64()).unwrap_or(0.0);
            DecisionHistoryItem {
                id: r.id.map(|i| i.to_string()).unwrap_or_default(),
                timestamp: r.timestamp.to_rfc3339(),
                mode: r.scene_type,
                action_summary,
                reward,
            }
        })
        .collect();

    Json(DecisionHistoryResponse { total, page, items })
}

/// GET /api/v1/ai/history/accuracy
pub async fn get_prediction_accuracy_history(
    State(_state): State<Arc<AppState>>,
) -> Json<AccuracyHistoryResponse> {
    Json(AccuracyHistoryResponse {
        items: vec![],
        avg_accuracy: 0.0,
    })
}
