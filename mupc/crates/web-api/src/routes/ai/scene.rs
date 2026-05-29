//! 场景管理端点
//!
//! 查询当前场景分类和历史场景

use axum::{Json, extract::State};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/scenes/current — 获取当前场景
pub async fn get_current_scene(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}

/// GET /api/v1/ai/scenes/history — 获取历史场景
pub async fn get_scene_history(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
