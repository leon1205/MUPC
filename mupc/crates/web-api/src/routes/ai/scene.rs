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
    let end = chrono::Utc::now();
    let start = end - chrono::Duration::days(7);
    let events = state.storage.events.query_range(start, end).await
        .map_err(|e| { tracing::error!(%e, "查询场景历史失败"); e })
        .unwrap_or_default();

    let entries: Vec<serde_json::Value> = events
        .into_iter()
        .filter(|e| e.event_type.contains("mode_switch") || e.event_type.contains("scene"))
        .map(|e| serde_json::json!({
            "timestamp": e.timestamp.to_rfc3339(),
            "event_type": e.event_type,
            "source": e.source,
            "message": e.message,
        }))
        .collect();

    Json(serde_json::json!({
        "entries": entries,
        "total": entries.len()
    }))
}
