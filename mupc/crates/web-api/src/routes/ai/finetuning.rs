//! 在线微调监控端点
//!
//! GET /api/v1/ai/finetuning — 获取在线微调状态

use axum::Json;
use serde::Serialize;

/// 微调状态响应
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
pub async fn get_finetuning() -> Json<FinetuningResponse> {
    todo!("Phase 2+ — 从 ai-engine OnlineUpdater 查询微调状态")
}
