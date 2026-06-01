//! AI 决策端点
//!
//! GET /api/v1/ai/decisions        — 获取决策列表（分页）
//! GET /api/v1/ai/decisions/latest — 获取最新决策
//! GET /api/v1/ai/decisions/{id}   — 获取决策详情

use axum::{Json, extract::{State, Path, Query}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct DecisionQuery {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

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
    State(state): State<Arc<AppState>>,
    Query(query): Query<DecisionQuery>,
) -> Json<DecisionListResponse> {
    let page = query.page.unwrap_or(1);
    let _page_size = query.page_size.unwrap_or(20);
    let info = state.ai_integrator.engine_status().await;

    let summary = if info.rl_ready {
        DecisionSummary {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            mode: "auto".to_string(),
            action_summary: "RL 决策已就绪".to_string(),
        }
    } else {
        DecisionSummary {
            id: "fallback".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            mode: "fallback".to_string(),
            action_summary: "使用兜底策略".to_string(),
        }
    };

    Json(DecisionListResponse {
        total: 1,
        page,
        decisions: vec![summary],
    })
}

/// GET /api/v1/ai/decisions/latest
pub async fn get_latest_decision(
    State(state): State<Arc<AppState>>,
) -> Json<DecisionDetailResponse> {
    let info = state.ai_integrator.engine_status().await;

    Json(DecisionDetailResponse {
        timestamp: chrono::Utc::now().to_rfc3339(),
        system_state: SystemStateSnapshot {
            battery_soc: 0.0,
            pv_power_kw: 0.0,
            load_power_kw: 0.0,
            grid_power_kw: 0.0,
            transformer_load_kw: 0.0,
        },
        action: ActionSnapshot {
            p_batt_set_kw: 0.0,
            load_shedding_kw: 0.0,
            pv_limit_ratio: 1.0,
            confidence: 0.0,
        },
        mode: ModeSnapshot {
            current: "auto".to_string(),
            display_name: "自动模式".to_string(),
            source: "LocalConfig".to_string(),
            switched_at: String::new(),
        },
        reward_breakdown: vec![],
        ai_engine_enabled: info.ai_engine_enabled,
    })
}

/// GET /api/v1/ai/decisions/{id}
pub async fn get_decision_detail(
    State(state): State<Arc<AppState>>,
    Path(decision_id): Path<String>,
) -> Json<DecisionDetailResponse> {
    tracing::debug!(id = %decision_id, "查询决策详情");
    get_latest_decision(State(state)).await
}
