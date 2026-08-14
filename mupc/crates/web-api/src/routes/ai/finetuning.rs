//! 在线微调监控端点
//!
//! GET /api/v1/ai/finetuning — 获取在线微调状态

use crate::AppState;
use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::Arc;

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

/// OnlineUpdater 内部默认缓冲区容量，用于计算进度百分比
const DEFAULT_BUFFER_CAPACITY: usize = 1000;

/// GET /api/v1/ai/finetuning
pub async fn get_finetuning(State(state): State<Arc<AppState>>) -> Json<FinetuningResponse> {
    let updater = state.online_updater.read().await;
    let enabled = updater.is_enabled();
    let buffer_size = updater.buffer_size();
    let batch_size = updater.config().batch_size;
    let buffer_progress = buffer_size as f64 / DEFAULT_BUFFER_CAPACITY.max(1) as f64;

    Json(FinetuningResponse {
        enabled,
        state: if enabled { "active" } else { "idle" }.to_string(),
        buffer_size,
        batch_size,
        buffer_progress,
        progress_percent: None,
        total_epochs: None,
        completed_epochs: None,
        last_update: None,
        last_metrics: None,
        recent_history: vec![],
    })
}
