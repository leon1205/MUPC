//! AI 预测端点
//!
//! 查询 LSTM 预测结果（光伏出力/负荷）

use axum::{Json, extract::State};
use std::sync::Arc;
use crate::routes::config::AppState;

/// GET /api/v1/ai/predictions — 获取预测列表
pub async fn get_predictions(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}

/// GET /api/v1/ai/predictions/current — 获取当前预测
pub async fn get_current_prediction(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}

/// GET /api/v1/ai/predictions/history — 获取历史预测
pub async fn get_prediction_history(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}
