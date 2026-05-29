//! 权重管理端点
//!
//! 查询 AI 模型权重配置

use axum::{Json, extract::State};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/weights — 获取 AI 模型权重
pub async fn get_weights(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
