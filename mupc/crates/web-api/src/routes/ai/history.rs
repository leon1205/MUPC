//! 历史记录端点
//!
//! 查询决策历史和预测准确率历史

use axum::{Json, extract::State};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/history/decisions — 获取决策历史
pub async fn get_decision_history(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}

/// GET /api/v1/ai/history/accuracy — 获取预测准确率历史
pub async fn get_prediction_accuracy_history(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
