//! 决策解释端点
//!
//! 提供 AI 决策可解释性分析

use axum::{Json, extract::State};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/explain — 获取 AI 决策解释
pub async fn get_explanation(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
