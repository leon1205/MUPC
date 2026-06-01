//! 场景管理端点
//!
//! GET /api/v1/ai/scenes/current — 获取当前运行场景
//! GET /api/v1/ai/scenes/history — 获取历史场景切换记录

use axum::{Json, extract::State};
use serde::Serialize;
use std::sync::Arc;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct CurrentSceneResponse {
    pub current: String,
    pub display_name: String,
    pub description: String,
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct SceneHistoryEntry {
    pub previous: String,
    pub current: String,
    pub source: String,
    pub timestamp: String,
}

/// GET /api/v1/ai/scenes/current
pub async fn get_current_scene(
    State(state): State<Arc<AppState>>,
) -> Json<CurrentSceneResponse> {
    let current = state.mode_selector.current();
    Json(CurrentSceneResponse {
        current: format!("{:?}", current),
        display_name: current.display_name().to_string(),
        description: current.description().to_string(),
        source: "LocalConfig".to_string(),
    })
}

/// GET /api/v1/ai/scenes/history
pub async fn get_scene_history(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let _ = state;
    Json(serde_json::json!({
        "entries": [],
        "total": 0
    }))
}
