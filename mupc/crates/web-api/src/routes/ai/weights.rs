//! 权重管理端点
//!
//! GET /api/v1/ai/weights — 查询 AI 模型权重配置
//! PUT /api/v1/ai/weights — 更新优化目标权重

use axum::{Json, extract::State, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

/// 权重配置响应
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

/// 权重更新请求
#[derive(Debug, Deserialize)]
pub struct UpdateWeightsRequest {
    pub weights: Vec<WeightChange>,
}

#[derive(Debug, Deserialize)]
pub struct WeightChange {
    pub name: String,
    pub value: f64,
}

/// 权重更新响应
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
    State(_state): State<Arc<AppState>>,
) -> Json<WeightsResponse> {
    todo!("Phase 2+ — 从 ai-engine/strategy-engine 查询当前权重")
}

/// PUT /api/v1/ai/weights
///
/// 校验: 权重名称合法性 + 值范围 0.0-5.0。
/// 写入审计日志，持久化到 /etc/mupc/weights.toml。
pub async fn put_update_weights(
    State(_state): State<Arc<AppState>>,
    Json(req): Json<UpdateWeightsRequest>,
) -> Result<Json<UpdateWeightsResponse>, StatusCode> {
    // 校验
    for change in &req.weights {
        if !validate_weight_name(&change.name) {
            return Err(StatusCode::BAD_REQUEST);
        }
        if !validate_weight_value(change.value) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    todo!("Phase 2+ — 调用 AiIntegrator::apply_weight_changes()，持久化，写审计日志")
}
