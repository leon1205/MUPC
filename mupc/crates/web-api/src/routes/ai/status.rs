//! AI 状态概览端点
//!
//! 获取 AI 引擎整体运行状态概览

use axum::{Json, extract::State};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/status — 获取 AI 系统概览状态
pub async fn get_ai_status(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
