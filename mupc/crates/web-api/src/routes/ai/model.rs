//! 模型管理端点
//!
//! 查询 AI 模型状态和性能指标

use axum::{Json, extract::State};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/models/status — 获取模型运行状态
pub async fn get_model_status(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}

/// GET /api/v1/ai/models/metrics — 获取模型性能指标
pub async fn get_model_metrics(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
