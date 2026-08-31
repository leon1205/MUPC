//! 策略模式管理 API
//!
//! GET  /api/v1/strategy-mode      — 获取当前策略模式（本地优先状态）
//! PUT  /api/v1/strategy-mode      — 切换本地优先模式（运行时热切换）

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

/// 策略模式状态响应
#[derive(Debug, Serialize)]
pub struct StrategyModeStatusResponse {
    /// 本地策略优先模式（true = 本地台区储能治理策略优先，AI 旁路；false = AI 优先）
    pub local_priority: bool,
    /// 当前生效的控制源描述
    pub control_source: String,
}

/// 策略模式切换请求
#[derive(Debug, Deserialize)]
pub struct StrategyModeSwitchRequest {
    pub local_priority: bool,
}

/// GET /api/v1/strategy-mode — 获取当前策略模式
async fn get_strategy_mode(State(state): State<Arc<AppState>>) -> Json<StrategyModeStatusResponse> {
    let local_priority = state.ai_integrator.is_local_priority().await;
    Json(StrategyModeStatusResponse {
        local_priority,
        control_source: if local_priority {
            "本地台区储能治理策略（AI 旁路）".to_string()
        } else {
            "AI 智能模式（失败降级本地）".to_string()
        },
    })
}

/// PUT /api/v1/strategy-mode — 切换本地优先模式
async fn switch_strategy_mode(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StrategyModeSwitchRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .ai_integrator
        .set_local_priority(req.local_priority)
        .await;
    let now = Utc::now().to_rfc3339();
    state.audit_logger.log_action(
        "operator",
        "strategy_mode_switch",
        &format!("local_priority -> {}", req.local_priority),
        "ok",
    );

    Ok(Json(serde_json::json!({
        "status": "ok",
        "local_priority": req.local_priority,
        "switched_at": now,
    })))
}

/// 创建策略模式路由
pub fn create_router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v1/strategy-mode",
            get(get_strategy_mode).put(switch_strategy_mode),
        )
}
