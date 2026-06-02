//! 模型回滚端点
//!
//! POST /api/v1/ai/rollback — 执行模型回滚

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use mupc_ota_update::types::ModelType;
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

fn parse_model_type(s: &str) -> Option<ModelType> {
    match s.to_lowercase().as_str() {
        "lstm" => Some(ModelType::Lstm),
        "maddpg" => Some(ModelType::Maddpg),
        _ => None,
    }
}

/// POST /api/v1/ai/rollback
pub async fn post_rollback(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RollbackRequest>,
) -> Result<Json<RollbackResponse>, StatusCode> {
    if req.password.is_empty() || req.model_type.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let model_type = parse_model_type(&req.model_type)
        .ok_or(StatusCode::BAD_REQUEST)?;

    let previous_version = state.ota_manager
        .get_current_version(model_type)
        .map(|v| v.version.clone())
        .unwrap_or_else(|_| "unknown".to_string());

    match state.ota_manager.rollback(model_type).await {
        Ok(()) => {
            tracing::warn!(
                model = %req.model_type,
                target = %req.target_version,
                reason = %req.reason,
                "模型回滚已执行"
            );
            let current_version = state.ota_manager
                .get_current_version(model_type)
                .map(|v| v.version)
                .unwrap_or_else(|_| req.target_version.clone());

            Ok(Json(RollbackResponse {
                status: "ok".to_string(),
                previous_version,
                current_version,
                rolled_back_at: chrono::Utc::now().to_rfc3339(),
                warmup_result: "completed".to_string(),
            }))
        }
        Err(e) => {
            tracing::error!(error = %e, "模型回滚失败");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
