//! 专家干预端点
//!
//! 查询人工专家干预记录

use axum::{Json, extract::State};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/interventions — 获取专家干预记录
pub async fn get_interventions(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
