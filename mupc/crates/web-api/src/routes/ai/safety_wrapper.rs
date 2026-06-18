//! v2.17 安全包装器 API 端点
//!
//! GET /api/v1/safety_wrapper/status — 当前状态（边界、阻抗、指标）
//! GET /api/v1/safety_wrapper/stats  — 累计统计指标

use crate::AppState;
use axum::{extract::State, Json};
use serde::Serialize;
use std::sync::Arc;

/// 安全包装器状态响应
#[derive(Debug, Serialize)]
pub struct SafetyWrapperStatus {
    pub v_min: f64,
    pub v_max: f64,
    pub dv_dt_max: f64,
    pub soc_margin: f64,
    pub line_impedance_r_ohm: f64,
    pub line_impedance_x_ohm: f64,
    pub v_base: f64,
    pub total_checks: u64,
    pub total_rejected: u64,
    pub total_fallback: u64,
    pub rejection_rate: f64,
    pub avg_latency_us: u64,
    pub max_latency_us: u64,
}

/// GET /api/v1/safety_wrapper/status
pub async fn get_safety_status(State(state): State<Arc<AppState>>) -> Json<SafetyWrapperStatus> {
    let cfg = state.safety_wrapper.config();
    let stats = state.safety_wrapper.stats().await;

    Json(SafetyWrapperStatus {
        v_min: cfg.v_min,
        v_max: cfg.v_max,
        dv_dt_max: cfg.dv_dt_max,
        soc_margin: cfg.soc_margin,
        line_impedance_r_ohm: cfg.line_impedance_r_ohm,
        line_impedance_x_ohm: cfg.line_impedance_x_ohm,
        v_base: cfg.v_base,
        total_checks: stats.total_checks,
        total_rejected: stats.total_rejected,
        total_fallback: stats.total_fallback,
        rejection_rate: stats.rejection_rate_1h,
        avg_latency_us: stats.avg_latency_us,
        max_latency_us: stats.max_latency_us,
    })
}

/// GET /api/v1/safety_wrapper/stats
pub async fn get_safety_stats(State(state): State<Arc<AppState>>) -> Json<SafetyWrapperStatus> {
    // 复用 status 响应（简化设计，stats 与 status 返回相同结构）
    get_safety_status(State(state)).await
}
