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
    let records = state.storage.decisions.query_recent(50).await
        .map_err(|e| { tracing::error!(%e, "查询奖励值失败"); e })
        .unwrap_or_default();

    let mut reward_values: Vec<f64> = Vec::new();
    let mut history: Vec<HistoryReward> = Vec::new();

    for r in &records {
        let action: serde_json::Value = serde_json::from_str(&r.action_json)
            .unwrap_or_default();
        let total_reward = action
            .get("reward")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        reward_values.push(total_reward);
        history.push(HistoryReward {
            timestamp: r.timestamp.to_rfc3339(),
            total_reward,
            components: vec![],
        });
    }

    let (max, min, avg) = if reward_values.is_empty() {
        (0.0, 0.0, 0.0)
    } else {
        let max = reward_values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min = reward_values.iter().cloned().fold(f64::INFINITY, f64::min);
        let avg = reward_values.iter().sum::<f64>() / reward_values.len() as f64;
        (max, min, avg)
    };

    let current_total = reward_values.first().copied().unwrap_or(0.0);

    Json(RewardsResponse {
        current: CurrentReward {
            total: current_total,
            components: vec![],
            timestamp: chrono::Utc::now().to_rfc3339(),
        },
        history,
        stats: RewardStats { max, min, avg },
    })
}
