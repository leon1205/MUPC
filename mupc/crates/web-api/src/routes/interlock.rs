//! 安全联锁 Web 面 API
//!
//! GET  /api/v1/interlock/status  — 查询联锁状态（DI 触发源 / latch / 停机失败 / 灯光）
//! POST /api/v1/interlock/release — 人工释放联锁（触发源复位且保持 >= release_hold_secs 才清 latch）
//! POST /api/v1/interlock/ack_m1  — M1 保护跳闸/停机人工授权重启（仅 !latch 生效）
//!
//! `AppState.interlock` 未注入（core-bin 尚未装配真实 controller）时，
//! 端点返回 503 语义 JSON（`enabled=false` / `code=1001`），不 panic。

use crate::audit::WebAuditEntry;
use crate::AppState;
use axum::{
    extract::State,
    response::Json,
    routing::{get, post},
    Router,
};
use serde_json::json;
use std::sync::Arc;

use crate::routes::ai::auth::RequireRole;

/// 联锁人工操作（release/ack_m1）的 Web 审计留痕，携带操作者身份。
///
/// 角色未分层（技术债 U-01）：`RequireRole::username()` 即已验证 session 用户名
/// （当前仅 admin 可登，恒为 "admin"）；真实 RBAC 落库后此处为真实操作者。
/// 审计写入失败仅告警，不阻断联锁操作结果返回。
async fn audit_interlock_op(
    state: &Arc<AppState>,
    operator: &str,
    action: &str,
    resource: &str,
    ok: bool,
) {
    let entry = WebAuditEntry {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        user: operator.to_string(),
        role: "admin".to_string(),
        action: action.to_string(),
        resource: resource.to_string(),
        method: "POST".to_string(),
        status_code: if ok { 200 } else { 500 },
        ip_address: "127.0.0.1".to_string(),
        user_agent: String::new(),
    };
    if let Err(e) = state.audit_logger.log(entry).await {
        tracing::warn!("联锁 {} 审计留痕失败: {}", action, e);
    }
}

/// GET /api/v1/interlock/status — 查询联锁状态
pub async fn get_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let Some(api) = state.interlock.as_deref() else {
        return Json(json!({
            "enabled": false,
            "error": "interlock 未启用",
        }));
    };
    let st = api.status().await;
    Json(serde_json::to_value(&st).unwrap_or_else(|_| json!({ "enabled": false })))
}

/// POST /api/v1/interlock/release — 人工释放联锁（含操作者审计留痕）
pub async fn post_release(
    op: RequireRole,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let operator = op.username().unwrap_or("admin(session)");
    let Some(api) = state.interlock.as_deref() else {
        audit_interlock_op(&state, operator, "interlock_release", "/api/v1/interlock/release", false)
            .await;
        return Json(json!({
            "code": 1001,
            "msg": "interlock 未启用",
        }));
    };
    match api.request_release().await {
        Ok(()) => {
            audit_interlock_op(&state, operator, "interlock_release", "/api/v1/interlock/release", true)
                .await;
            Json(json!({ "code": 0, "msg": "联锁已释放" }))
        }
        Err(e) => {
            audit_interlock_op(&state, operator, "interlock_release", "/api/v1/interlock/release", false)
                .await;
            Json(json!({ "code": 1001, "msg": e }))
        }
    }
}

/// POST /api/v1/interlock/ack_m1 — M1 保护跳闸/停机人工授权重启（含操作者审计留痕）
pub async fn post_ack_m1(
    op: RequireRole,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let operator = op.username().unwrap_or("admin(session)");
    let Some(api) = state.interlock.as_deref() else {
        audit_interlock_op(&state, operator, "interlock_ack_m1", "/api/v1/interlock/ack_m1", false)
            .await;
        return Json(json!({
            "code": 1001,
            "msg": "interlock 未启用",
        }));
    };
    match api.ack_m1().await {
        Ok(()) => {
            audit_interlock_op(&state, operator, "interlock_ack_m1", "/api/v1/interlock/ack_m1", true)
                .await;
            Json(json!({ "code": 0, "msg": "已授权 M1 重启" }))
        }
        Err(e) => {
            audit_interlock_op(&state, operator, "interlock_ack_m1", "/api/v1/interlock/ack_m1", false)
                .await;
            Json(json!({ "code": 1001, "msg": e }))
        }
    }
}

/// 创建安全联锁路由
pub fn create_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/interlock/status", get(get_status))
        .route("/api/v1/interlock/release", post(post_release))
        .route("/api/v1/interlock/ack_m1", post(post_ack_m1))
}
