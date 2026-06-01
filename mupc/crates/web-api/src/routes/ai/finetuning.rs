//! 在线微调监控端点
//!
//! GET /api/v1/ai/finetuning — 获取在线微调状态

use axum::{Json, extract::State};
use serde::Serialize;
use std::sync::Arc;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct FinetuningResponse {
    pub enabled: bool,
    pub state: String,
    pub buffer_size: usize,
    pub batch_size: usize,
    pub buffer_progress: f64,
    pub progress_percent: Option<f64>,
    pub total_epochs: Option<usize>,
    pub completed_epochs: Option<usize>,
    pub last_update: Option<String>,
    pub last_metrics: Option<FinetuningMetrics>,
    pub recent_history: Vec<FinetuningHistoryEntry>,
}

#[derive(Debug, Serialize)]
pub struct FinetuningMetrics {
    pub loss_before: f64,
    pub loss_after: f64,
    pub improvement: f64,
}

#[derive(Debug, Serialize)]
pub struct FinetuningHistoryEntry {
    pub completed_at: String,
    pub loss_before: f64,
    pub loss_after: f64,
    pub improvement: f64,
}

/// GET /api/v1/ai/finetuning
pub async fn get_finetuning(
    State(state): State<Arc<AppState>>,
) -> Json<FinetuningResponse> {
    let _ = state;

    Json(FinetuningResponse {
        enabled: false,
        state: "idle".to_string(),
        buffer_size: 1000,
        batch_size: 32,
        buffer_progress: 0.0,
        progress_percent: None,
        total_epochs: None,
        completed_epochs: None,
        last_update: None,
        last_metrics: None,
        recent_history: vec![],
    })
}
