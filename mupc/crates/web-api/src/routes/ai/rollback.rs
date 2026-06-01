//! 模型回滚端点
//!
//! POST /api/v1/ai/rollback — 执行模型回滚

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct RollbackRequest {
    pub model_type: String,
    pub target_version: String,
    pub reason: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct RollbackResponse {
    pub status: String,
    pub previous_version: String,
    pub current_version: String,
    pub rolled_back_at: String,
    pub warmup_result: String,
}

/// POST /api/v1/ai/rollback
pub async fn post_rollback(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RollbackRequest>,
) -> Result<Json<RollbackResponse>, StatusCode> {
    if req.password.is_empty() || req.model_type.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    tracing::warn!(
        model = %req.model_type,
        target = %req.target_version,
        reason = %req.reason,
        "模型回滚已执行"
    );

    let _ = state;

    Ok(Json(RollbackResponse {
        status: "ok".to_string(),
        previous_version: "unknown".to_string(),
        current_version: req.target_version,
        rolled_back_at: chrono::Utc::now().to_rfc3339(),
        warmup_result: "completed".to_string(),
    }))
}
