//! 策略模式管理 API
//!
//! GET  /api/v1/strategy-mode      — 获取当前策略模式（本地优先状态）
//! PUT  /api/v1/strategy-mode      — 切换本地优先模式（运行时热切换）

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::audit::WebAuditEntry;
use crate::routes::ai::auth::RequireRole;
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

/// 策略模式切换写操作审计留痕，携带已验证 session 操作者身份（仿 interlock T8）。
///
/// 角色未分层（技术债 U-01）：`RequireRole::username()` 即已验证 session 用户名
/// （当前仅 admin 可登，恒为 "admin"）；真实 RBAC 落库后此处为真实操作者。
/// 审计写入失败仅告警，不阻断切换结果返回。
async fn audit_strategy_mode_switch(state: &Arc<AppState>, operator: &str) {
    let entry = WebAuditEntry {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        user: operator.to_string(),
        role: "admin".to_string(),
        action: "strategy_mode_switch".to_string(),
        resource: "/api/v1/strategy-mode".to_string(),
        method: "PUT".to_string(),
        status_code: 200,
        ip_address: "127.0.0.1".to_string(),
        user_agent: String::new(),
    };
    if let Err(e) = state.audit_logger.log(entry).await {
        tracing::warn!("策略模式切换审计留痕失败: {}", e);
    }
}

/// PUT /api/v1/strategy-mode — 切换本地优先模式
async fn switch_strategy_mode(
    op: RequireRole,
    State(state): State<Arc<AppState>>,
    Json(req): Json<StrategyModeSwitchRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let operator = op.username().unwrap_or("admin(session)");
    // AI 引擎暂停（2026-09-09）：本地策略为唯一下发引擎，local_priority 不可切换——
    // 置 false（AI 优先）无对象，置 true 又改变部署默认。暂停期一律拒绝写，GET 照常读当前值。
    let info = state.ai_integrator.engine_status().await;
    if !info.ai_engine_enabled {
        tracing::warn!("AI 引擎暂停中，local_priority 不可切换（engine_status={}）", info.engine_status);
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    state
        .ai_integrator
        .set_local_priority(req.local_priority)
        .await;
    let now = Utc::now().to_rfc3339();
    audit_strategy_mode_switch(&state, operator).await;

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
