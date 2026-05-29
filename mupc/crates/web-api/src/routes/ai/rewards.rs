//! 奖励值查询端点
//!
//! GET /api/v1/ai/rewards — 获取奖励值（含历史趋势）

use axum::{Json, extract::Query};
use serde::{Deserialize, Serialize};

/// 奖励查询参数
#[derive(Debug, Deserialize)]
pub struct RewardsQuery {
    pub start: Option<String>,
    pub end: Option<String>,
    pub range: Option<String>,
}

/// 奖励值响应
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
    Query(_query): Query<RewardsQuery>,
) -> Json<RewardsResponse> {
    todo!("Phase 2+ — 从 SQLite/data-processing 查询奖励历史")
}
