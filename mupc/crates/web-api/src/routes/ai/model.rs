//! 模型管理端点
//!
//! 查询 AI 模型状态和性能指标

use axum::{Json, extract::State};
use serde::Serialize;
use std::sync::Arc;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct ModelStatusResponse {
    pub models: Vec<ModelInfo>,
    pub ai_engine_enabled: bool,
    pub fallback_active: bool,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub name: String,
    pub status: String,
    pub version: String,
    pub loaded_at: Option<String>,
    pub inference_count: u64,
    pub avg_inference_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct ModelMetricsResponse {
    pub metrics: Vec<ModelMetric>,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct ModelMetric {
    pub model_name: String,
    pub accuracy: f64,
    pub loss: f64,
    pub samples_processed: u64,
}

/// GET /api/v1/ai/models/status
pub async fn get_model_status(
    State(state): State<Arc<AppState>>,
) -> Json<ModelStatusResponse> {
    let info = state.ai_integrator.engine_status().await;

    let models = vec![
        ModelInfo {
            name: "LSTM".to_string(),
            status: if info.lstm_ready { "ready" } else { "unloaded" }.to_string(),
            version: "1.0.0".to_string(),
            loaded_at: None,
            inference_count: 0,
            avg_inference_ms: 0.0,
        },
        ModelInfo {
            name: "MADDPG".to_string(),
            status: if info.rl_ready { "ready" } else { "unloaded" }.to_string(),
            version: "1.0.0".to_string(),
            loaded_at: None,
            inference_count: 0,
            avg_inference_ms: 0.0,
        },
    ];

    Json(ModelStatusResponse {
        models,
        ai_engine_enabled: info.ai_engine_enabled,
        fallback_active: info.fallback_active,
    })
}

/// GET /api/v1/ai/models/metrics
pub async fn get_model_metrics(
    State(state): State<Arc<AppState>>,
) -> Json<ModelMetricsResponse> {
    let _ = state.ai_integrator;

    Json(ModelMetricsResponse {
        metrics: vec![
            ModelMetric {
                model_name: "LSTM".to_string(),
                accuracy: 0.0,
                loss: 0.0,
                samples_processed: 0,
            },
            ModelMetric {
                model_name: "MADDPG".to_string(),
                accuracy: 0.0,
                loss: 0.0,
                samples_processed: 0,
            },
        ],
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}
