//! AI 决策端点
//!
//! 查询 RL 决策结果

use axum::{Json, extract::{State, Path}};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/decisions — 获取决策列表
pub async fn get_decisions(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}

/// GET /api/v1/ai/decisions/latest — 获取最新决策
pub async fn get_latest_decision(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}

/// GET /api/v1/ai/decisions/:id — 获取决策详情
pub async fn get_decision_detail(
    State(_state): State<Arc<AppState>>,
    Path(decision_id): Path<String>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
