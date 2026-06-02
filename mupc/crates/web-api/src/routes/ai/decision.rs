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

fn parse_action_json(action_json: &str) -> serde_json::Value {
    serde_json::from_str(action_json).unwrap_or(serde_json::json!({}))
}

fn empty_system_state() -> SystemStateSnapshot {
    SystemStateSnapshot {
        battery_soc: 0.0,
        pv_power_kw: 0.0,
        load_power_kw: 0.0,
        grid_power_kw: 0.0,
        transformer_load_kw: 0.0,
    }
}

/// GET /api/v1/ai/decisions
pub async fn get_decisions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DecisionQuery>,
) -> Json<DecisionListResponse> {
    let page = query.page.unwrap_or(1);
    let page_size = query.page_size.unwrap_or(20) as usize;

    let records = state.storage.decisions.query_recent(page_size).await
        .unwrap_or_default();

    let total = records.len() as u64;
    let decisions: Vec<DecisionSummary> = records.into_iter().map(|r| {
        let action = parse_action_json(&r.action_json);
        let action_summary = action
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        DecisionSummary {
            id: r.id.map(|i| i.to_string()).unwrap_or_default(),
            timestamp: r.timestamp.to_rfc3339(),
            mode: r.scene_type,
            action_summary,
        }
    }).collect();

    Json(DecisionListResponse { total, page, decisions })
}

/// GET /api/v1/ai/decisions/latest
pub async fn get_latest_decision(
    State(state): State<Arc<AppState>>,
) -> Json<DecisionDetailResponse> {
    let info = state.ai_integrator.engine_status().await;
    let records = state.storage.decisions.query_recent(1).await.unwrap_or_default();

    if let Some(record) = records.into_iter().next() {
        let action = parse_action_json(&record.action_json);
        Json(DecisionDetailResponse {
            timestamp: record.timestamp.to_rfc3339(),
            system_state: empty_system_state(),
            action: ActionSnapshot {
                p_batt_set_kw: action.get("p_batt_set_kw").and_then(|v| v.as_f64()).unwrap_or(0.0),
                load_shedding_kw: action.get("load_shedding_kw").and_then(|v| v.as_f64()).unwrap_or(0.0),
                pv_limit_ratio: action.get("pv_limit_ratio").and_then(|v| v.as_f64()).unwrap_or(1.0),
                confidence: record.confidence,
            },
            mode: ModeSnapshot {
                current: record.scene_type,
                display_name: String::new(),
                source: "LocalConfig".to_string(),
                switched_at: String::new(),
            },
            reward_breakdown: vec![],
            ai_engine_enabled: info.ai_engine_enabled,
        })
    } else {
        Json(DecisionDetailResponse {
            timestamp: chrono::Utc::now().to_rfc3339(),
            system_state: empty_system_state(),
            action: ActionSnapshot {
                p_batt_set_kw: 0.0, load_shedding_kw: 0.0,
                pv_limit_ratio: 1.0, confidence: 0.0,
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
}

/// GET /api/v1/ai/decisions/{id}
pub async fn get_decision_detail(
    State(state): State<Arc<AppState>>,
    Path(decision_id): Path<String>,
) -> Json<DecisionDetailResponse> {
    let info = state.ai_integrator.engine_status().await;

    if let Ok(id) = decision_id.parse::<i64>() {
        if let Ok(Some(record)) = state.storage.decisions.get_by_id(id).await {
            let action = parse_action_json(&record.action_json);
            return Json(DecisionDetailResponse {
                timestamp: record.timestamp.to_rfc3339(),
                system_state: empty_system_state(),
                action: ActionSnapshot {
                    p_batt_set_kw: action.get("p_batt_set_kw").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    load_shedding_kw: action.get("load_shedding_kw").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    pv_limit_ratio: action.get("pv_limit_ratio").and_then(|v| v.as_f64()).unwrap_or(1.0),
                    confidence: record.confidence,
                },
                mode: ModeSnapshot {
                    current: record.scene_type,
                    display_name: String::new(),
                    source: "LocalConfig".to_string(),
                    switched_at: String::new(),
                },
                reward_breakdown: vec![],
                ai_engine_enabled: info.ai_engine_enabled,
            });
        }
    }

    Json(DecisionDetailResponse {
        timestamp: chrono::Utc::now().to_rfc3339(),
        system_state: empty_system_state(),
        action: ActionSnapshot {
            p_batt_set_kw: 0.0, load_shedding_kw: 0.0,
            pv_limit_ratio: 1.0, confidence: 0.0,
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
