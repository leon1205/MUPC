//! AI 决策端点
//!
//! GET /api/v1/ai/decisions        — 获取决策列表（分页）
//! GET /api/v1/ai/decisions/latest — 获取最新决策
//! GET /api/v1/ai/decisions/{id}   — 获取决策详情

use axum::{Json, extract::{State, Path, Query}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

/// 决策查询参数
#[derive(Debug, Deserialize)]
pub struct DecisionQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

/// 决策列表响应
#[derive(Debug, Serialize)]
pub struct DecisionListResponse {
    pub total: u64,
    pub page: u32,
    pub decisions: Vec<DecisionSummary>,
}

#[derive(Debug, Serialize)]
pub struct DecisionSummary {
    pub id: String,
    pub timestamp: String,
    pub mode: String,
    pub action_summary: String,
}

/// 最新决策详情响应
#[derive(Debug, Serialize)]
pub struct DecisionDetailResponse {
    pub timestamp: String,
    pub system_state: SystemStateSnapshot,
    pub action: ActionSnapshot,
    pub mode: ModeSnapshot,
    pub reward_breakdown: Vec<RewardItem>,
    pub ai_engine_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct SystemStateSnapshot {
    pub battery_soc: f64,
    pub pv_power_kw: f64,
    pub load_power_kw: f64,
    pub grid_power_kw: f64,
    pub transformer_load_kw: f64,
}

#[derive(Debug, Serialize)]
pub struct ActionSnapshot {
    pub p_batt_set_kw: f64,
    pub load_shedding_kw: f64,
    pub pv_limit_ratio: f64,
    pub confidence: f64,
}

#[derive(Debug, Serialize)]
pub struct ModeSnapshot {
    pub current: String,
    pub display_name: String,
    pub source: String,
    pub switched_at: String,
}

#[derive(Debug, Serialize)]
pub struct RewardItem {
    pub name: String,
    pub value: f64,
    pub weight: f64,
    pub percentage: f64,
}

/// GET /api/v1/ai/decisions
pub async fn get_decisions(
    State(_state): State<Arc<AppState>>,
    Query(_query): Query<DecisionQuery>,
) -> Json<DecisionListResponse> {
    todo!("Phase 2+ — 从 decision cache 或 SQLite 查询历史决策")
}

/// GET /api/v1/ai/decisions/latest
pub async fn get_latest_decision(
    State(_state): State<Arc<AppState>>,
) -> Json<DecisionDetailResponse> {
    todo!("Phase 2+ — 从 AiIntegrator 获取最新决策快照")
}

/// GET /api/v1/ai/decisions/{id}
pub async fn get_decision_detail(
    State(_state): State<Arc<AppState>>,
    Path(_decision_id): Path<String>,
) -> Json<DecisionDetailResponse> {
    todo!("Phase 2+ — 按 ID 查询决策详情")
}
