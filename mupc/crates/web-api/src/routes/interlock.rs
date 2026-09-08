//! 安全联锁 Web 面 API
//!
//! GET  /api/v1/interlock/status  — 查询联锁状态（DI 触发源 / latch / 停机失败 / 灯光）
//! POST /api/v1/interlock/release — 人工释放联锁（触发源复位且保持 >= release_hold_secs 才清 latch）
//! POST /api/v1/interlock/ack_m1  — M1 保护跳闸/停机人工授权重启（仅 !latch 生效）
//!
//! `AppState.interlock` 未注入（core-bin 尚未装配真实 controller）时，
//! 端点返回 503 语义 JSON（`enabled=false` / `code=1001`），不 panic。

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

/// POST /api/v1/interlock/release — 人工释放联锁
pub async fn post_release(
    _op: RequireRole,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let Some(api) = state.interlock.as_deref() else {
        return Json(json!({
            "code": 1001,
            "msg": "interlock 未启用",
        }));
    };
    match api.request_release().await {
        Ok(()) => Json(json!({ "code": 0, "msg": "联锁已释放" })),
        Err(e) => Json(json!({ "code": 1001, "msg": e })),
    }
}

/// POST /api/v1/interlock/ack_m1 — M1 保护跳闸/停机人工授权重启
pub async fn post_ack_m1(
    _op: RequireRole,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let Some(api) = state.interlock.as_deref() else {
        return Json(json!({
            "code": 1001,
            "msg": "interlock 未启用",
        }));
    };
    match api.ack_m1().await {
        Ok(()) => Json(json!({ "code": 0, "msg": "已授权 M1 重启" })),
        Err(e) => Json(json!({ "code": 1001, "msg": e })),
    }
}

/// 创建安全联锁路由
pub fn create_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/interlock/status", get(get_status))
        .route("/api/v1/interlock/release", post(post_release))
        .route("/api/v1/interlock/ack_m1", post(post_ack_m1))
}
