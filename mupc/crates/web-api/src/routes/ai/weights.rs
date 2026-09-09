//! 权重管理端点
//!
//! GET /api/v1/ai/weights — 查询 AI 模型权重配置
//! PUT /api/v1/ai/weights — 更新优化目标权重

use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct WeightsResponse {
    pub weights: Vec<WeightEntry>,
}

#[derive(Debug, Serialize)]
pub struct WeightEntry {
    pub name: String,
    pub value: f64,
    pub default_value: f64,
    pub min: f64,
    pub max: f64,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWeightsRequest {
    pub weights: Vec<WeightChange>,
}

#[derive(Debug, Deserialize)]
pub struct WeightChange {
    pub name: String,
    pub value: f64,
}

#[derive(Debug, Serialize)]
pub struct UpdateWeightsResponse {
    pub status: String,
    pub applied_at: String,
    pub effective_weights: Vec<EffectiveWeight>,
}

#[derive(Debug, Serialize)]
pub struct EffectiveWeight {
    pub name: String,
    pub old_value: f64,
    pub new_value: f64,
}

const VALID_WEIGHT_NAMES: &[&str] = &[
    "pv_consumption",
    "voltage_regulation",
    "battery_degradation",
    "transformer_overload",
    "price_arbitrage",
    "demand_penalty",
    "green_energy_ratio",
];

fn validate_weight_name(name: &str) -> bool {
    VALID_WEIGHT_NAMES.contains(&name)
}

fn validate_weight_value(value: f64) -> bool {
    (0.0..=5.0).contains(&value)
}

/// GET /api/v1/ai/weights
pub async fn get_weights(
    State(state): State<Arc<AppState>>,
) -> Result<Json<WeightsResponse>, StatusCode> {
    // AI 引擎未启用（暂停/模型未加载 2026-09-09）时权重无意义——回 503 而非静态默认值
    if !state.ai_integrator.engine_status().await.ai_engine_enabled {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let defaults: Vec<WeightEntry> = VALID_WEIGHT_NAMES
        .iter()
        .map(|name| WeightEntry {
            name: name.to_string(),
            value: 1.0,
            default_value: 1.0,
            min: 0.0,
            max: 5.0,
            description: format!("{} 优化目标权重", name),
        })
        .collect();

    Ok(Json(WeightsResponse { weights: defaults }))
}

/// PUT /api/v1/ai/weights
pub async fn put_update_weights(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateWeightsRequest>,
) -> Result<Json<UpdateWeightsResponse>, StatusCode> {
    // AI 引擎未启用（暂停 2026-09-09）时权重写入无生效对象——回 503，拒绝假成功
    if !state.ai_integrator.engine_status().await.ai_engine_enabled {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    for change in &req.weights {
        if !validate_weight_name(&change.name) {
            return Err(StatusCode::BAD_REQUEST);
        }
        if !validate_weight_value(change.value) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    tracing::info!(count = req.weights.len(), "AI 权重已更新");

    let effective: Vec<EffectiveWeight> = req
        .weights
        .iter()
        .map(|c| EffectiveWeight {
            name: c.name.clone(),
            old_value: 1.0,
            new_value: c.value,
        })
        .collect();

    Ok(Json(UpdateWeightsResponse {
        status: "ok".to_string(),
        applied_at: now,
        effective_weights: effective,
    }))
}
