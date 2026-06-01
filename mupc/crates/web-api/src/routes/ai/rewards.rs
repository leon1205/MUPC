//! 奖励值查询端点
//!
//! GET /api/v1/ai/rewards — 获取奖励值（含历史趋势）

use axum::{Json, extract::{State, Query}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct RewardsQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub range: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RewardsResponse {
    pub current: CurrentReward,
    pub history: Vec<HistoryReward>,
    pub stats: RewardStats,
}

#[derive(Debug, Serialize)]
pub struct CurrentReward {
    pub total: f64,
    pub components: Vec<RewardComponent>,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct RewardComponent {
    pub name: String,
    pub value: f64,
    pub weight: f64,
}

#[derive(Debug, Serialize)]
pub struct HistoryReward {
    pub timestamp: String,
    pub total_reward: f64,
    pub components: Vec<RewardComponent>,
}

#[derive(Debug, Serialize)]
pub struct RewardStats {
    pub max: f64,
    pub min: f64,
    pub avg: f64,
}

/// GET /api/v1/ai/rewards
pub async fn get_rewards(
    State(state): State<Arc<AppState>>,
    Query(_query): Query<RewardsQuery>,
) -> Json<RewardsResponse> {
    let _ = state;

    Json(RewardsResponse {
        current: CurrentReward {
            total: 0.0,
            components: vec![],
            timestamp: chrono::Utc::now().to_rfc3339(),
        },
        history: vec![],
        stats: RewardStats {
            max: 0.0,
            min: 0.0,
            avg: 0.0,
        },
    })
}
