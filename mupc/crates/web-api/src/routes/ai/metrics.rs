//! 实时指标端点
//!
//! 提供 AI 引擎实时运行指标

use axum::{Json, extract::State};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/metrics — 获取 AI 实时运行指标
pub async fn get_realtime_metrics(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
