//! 决策解释端点
//!
//! 提供 AI 决策可解释性分析

use axum::{Json, extract::{State, Query}};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ExplainQuery {
    pub decision_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExplanationResponse {
    pub decision_id: Option<String>,
    pub summary: String,
    pub factors: Vec<FactorContribution>,
    pub timestamp: String,
}

#[derive(Debug, Serialize)]
pub struct FactorContribution {
    pub factor: String,
    pub contribution: f64,
    pub description: String,
}

/// GET /api/v1/ai/explain
pub async fn get_explanation(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ExplainQuery>,
) -> Json<ExplanationResponse> {
    let mut summary = "AI 决策基于当前系统状态的综合评估".to_string();

    if let Some(ref decision_id) = query.decision_id {
        if let Ok(id) = decision_id.parse::<i64>() {
            if let Ok(Some(record)) = state.storage.decisions.get_by_id(id).await {
                let action: serde_json::Value = serde_json::from_str(&record.action_json)
                    .unwrap_or_default();
                if let Some(action_type) = action.get("type").and_then(|v| v.as_str()) {
                    summary = format!(
                        "决策类型: {} (置信度: {:.1}%, 场景: {})",
                        action_type,
                        record.confidence * 100.0,
                        record.scene_type,
                    );
                }
            }
        }
    }

    Json(ExplanationResponse {
        decision_id: query.decision_id,
        summary,
        factors: vec![
            FactorContribution {
                factor: "电价".to_string(),
                contribution: 0.35,
                description: "当前电价水平影响充放电决策".to_string(),
            },
            FactorContribution {
                factor: "光伏出力".to_string(),
                contribution: 0.25,
                description: "光伏发电功率预测".to_string(),
            },
            FactorContribution {
                factor: "负荷需求".to_string(),
                contribution: 0.25,
                description: "当前及预测负荷水平".to_string(),
            },
            FactorContribution {
                factor: "电池状态".to_string(),
                contribution: 0.15,
                description: "电池 SOC 和健康状态".to_string(),
            },
        ],
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}
