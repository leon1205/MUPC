//! A/B 测试端点
//!
//! 查询 A/B 测试状态和结果

use axum::{Json, extract::State};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/ab-test/status — 获取 A/B 测试状态
pub async fn get_ab_test_status(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}

/// GET /api/v1/ai/ab-test/results — 获取 A/B 测试结果
pub async fn get_ab_test_results(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
