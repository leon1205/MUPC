//! 实时指标端点
//!
//! 提供 AI 引擎实时运行指标

use crate::AppState;
use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct RealtimeMetricsResponse {
    pub timestamp: String,
    pub engine_metrics: EngineMetrics,
    pub model_metrics: ModelRuntimeMetrics,
}

#[derive(Debug, Serialize)]
pub struct EngineMetrics {
    pub uptime_secs: u64,
    pub total_inferences: u64,
    pub total_decisions: u64,
    pub avg_decision_time_ms: f64,
    pub fallback_count: u64,
}

#[derive(Debug, Serialize)]
pub struct ModelRuntimeMetrics {
    pub lstm_inference_ms: f64,
    pub rl_inference_ms: f64,
    pub memory_usage_mb: f64,
    pub npu_utilization_pct: f64,
}

/// GET /api/v1/ai/metrics
pub async fn get_realtime_metrics(
    State(state): State<Arc<AppState>>,
) -> Json<RealtimeMetricsResponse> {
    let info = state.ai_integrator.engine_status().await;

    Json(RealtimeMetricsResponse {
        timestamp: chrono::Utc::now().to_rfc3339(),
        engine_metrics: EngineMetrics {
            uptime_secs: 0,
            total_inferences: 0,
            total_decisions: 0,
            avg_decision_time_ms: 0.0,
            fallback_count: if info.fallback_active { 1 } else { 0 },
        },
        model_metrics: ModelRuntimeMetrics {
            lstm_inference_ms: 0.0,
            rl_inference_ms: 0.0,
            memory_usage_mb: 0.0,
            npu_utilization_pct: 0.0,
        },
    })
}
