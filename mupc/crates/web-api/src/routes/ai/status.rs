//! AI 状态概览端点
//!
//! GET /api/v1/ai/status — 获取 AI 引擎整体运行状态

use axum::{Json, extract::State};
use serde::Serialize;
use std::sync::Arc;
use crate::AppState;

/// AI 状态响应
#[derive(Debug, Serialize)]
pub struct AiStatusResponse {
    pub engine_status: String,
    pub model_status: ModelStatusInfo,
    pub running_mode: Option<RunningModeInfo>,
    pub uptime_secs: u64,
    pub ai_engine_enabled: bool,
    pub fallback_active: bool,
}

#[derive(Debug, Serialize)]
pub struct ModelStatusInfo {
    pub lstm: String,
    pub rl: String,
}

#[derive(Debug, Serialize)]
pub struct RunningModeInfo {
    pub current: String,
    pub display_name: String,
    pub source: String,
    pub switched_at: String,
}

/// GET /api/v1/ai/status
pub async fn get_ai_status(
    State(state): State<Arc<AppState>>,
) -> Json<AiStatusResponse> {
    let info = state.ai_integrator.engine_status().await;
    Json(AiStatusResponse {
        engine_status: info.engine_status.to_string(),
        model_status: ModelStatusInfo {
            lstm: if info.lstm_ready { "ready".to_string() } else { "unloaded".to_string() },
            rl: if info.rl_ready { "ready".to_string() } else { "unloaded".to_string() },
        },
        running_mode: info.current_mode.map(|m| RunningModeInfo {
            current: m.id,
            display_name: m.display_name,
            source: "LocalConfig".to_string(),
            switched_at: String::new(),
        }),
        uptime_secs: 0,
        ai_engine_enabled: info.ai_engine_enabled,
        fallback_active: info.fallback_active,
    })
}
