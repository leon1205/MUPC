//! AI 预测端点
//!
//! GET /api/v1/ai/predictions         — 获取预测列表
//! GET /api/v1/ai/predictions/current — 获取当前预测
//! GET /api/v1/ai/predictions/history — 获取历史预测

use crate::AppState;
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct PredictionQuery {
    pub prediction_type: Option<String>,
    pub horizon: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct PredictionListResponse {
    pub predictions: Vec<PredictionPoint>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct PredictionPoint {
    pub timestamp: String,
    pub pv_power_kw: f64,
    pub load_power_kw: f64,
    pub confidence: f64,
    pub horizon_minutes: u32,
}

/// GET /api/v1/ai/predictions
pub async fn get_predictions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PredictionQuery>,
) -> Json<PredictionListResponse> {
    let horizon = query.horizon.unwrap_or(60);
    let info = state.ai_integrator.engine_status().await;

    let predictions: Vec<PredictionPoint> = if info.lstm_ready {
        (0..horizon / 5)
            .map(|i| PredictionPoint {
                timestamp: chrono::Utc::now().to_rfc3339(),
                pv_power_kw: 0.0,
                load_power_kw: 0.0,
                confidence: 0.85,
                horizon_minutes: i * 5,
            })
            .collect()
    } else {
        vec![]
    };

    Json(PredictionListResponse {
        total: predictions.len(),
        predictions,
    })
}

/// GET /api/v1/ai/predictions/current
pub async fn get_current_prediction(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let info = state.ai_integrator.engine_status().await;
    Json(serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "lstm_ready": info.lstm_ready,
        "engine_status": info.engine_status.to_string(),
        "prediction": {
            "pv_power_kw": 0.0,
            "load_power_kw": 0.0,
            "confidence": 0.85
        }
    }))
}

/// GET /api/v1/ai/predictions/history
pub async fn get_prediction_history(
    State(state): State<Arc<AppState>>,
) -> Json<PredictionListResponse> {
    let info = state.ai_integrator.engine_status().await;
    Json(PredictionListResponse {
        total: 0,
        predictions: vec![],
    })
}
