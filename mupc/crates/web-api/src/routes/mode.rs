//! 运行场景管理 API
//!
//! GET  /api/v1/mode      — 获取当前运行场景
//! PUT  /api/v1/mode      — 切换运行场景
//! GET  /api/v1/mode/list — 获取所有可用场景列表

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use mupc_ai_engine::mode_selector::{RunningMode, SwitchSource};
use crate::AppState;

/// 模式状态响应
#[derive(Debug, Serialize)]
pub struct ModeStatusResponse {
    pub current: String,
    pub display_name: String,
    pub description: String,
}

/// 模式切换请求
#[derive(Debug, Deserialize)]
pub struct SwitchModeRequest {
    pub mode: String,
}

/// 模式信息
#[derive(Debug, Serialize)]
pub struct ModeInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// 模式列表响应
#[derive(Debug, Serialize)]
pub struct ModeListResponse {
    pub modes: Vec<ModeInfo>,
}

/// GET /api/v1/mode — 获取当前运行场景
async fn get_mode(
    State(state): State<Arc<AppState>>,
) -> Json<ModeStatusResponse> {
    let current = state.mode_selector.current();
    Json(ModeStatusResponse {
        current: format!("{:?}", current),
        display_name: current.display_name().to_string(),
        description: current.description().to_string(),
    })
}

/// PUT /api/v1/mode — 切换运行场景
async fn switch_mode(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SwitchModeRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let new_mode = mupc_ai_engine::parse_mode_name(&req.mode)
        .ok_or(StatusCode::BAD_REQUEST)?;

    let previous = state.mode_selector.current();

    state
        .mode_selector
        .switch(
            new_mode,
            SwitchSource::LocalWeb {
                username: "operator".to_string(),
            },
        )
        .await
        .map_err(|e| {
            tracing::error!("模式切换失败: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let now = Utc::now().to_rfc3339();
    state.audit_logger.log_action(
        "operator",
        "mode_switch",
        &format!("{} -> {}", previous, new_mode),
        "ok",
    );

    // SSE 推送模式变更
    let _ = state.sse_push.push_mode_switch(previous, new_mode);

    Ok(Json(serde_json::json!({
        "status": "ok",
        "previous_mode": format!("{:?}", previous),
        "current_mode": format!("{:?}", new_mode),
        "display_name": new_mode.display_name(),
        "switched_at": now,
    })))
}

/// GET /api/v1/mode/list — 获取所有可用模式列表
async fn list_modes() -> Json<ModeListResponse> {
    let modes: Vec<ModeInfo> = RunningMode::all()
        .iter()
        .map(|m| ModeInfo {
            id: format!("{:?}", m),
            name: m.display_name().to_string(),
            description: m.description().to_string(),
        })
        .collect();

    Json(ModeListResponse { modes })
}

/// 创建模式管理路由
pub fn create_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/mode", get(get_mode).put(switch_mode))
        .route("/api/v1/mode/list", get(list_modes))
}

// ═══════════════════════════════════════════════════════════════
// 测试
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use mupc_ai_engine::mode_selector::{ModeSelector, RunningMode};
    use mupc_strategy_engine::AiIntegrator;
    use crate::audit::AuditLogger;
    use crate::auth::SessionManager;
    use crate::sse::SsePushService;
    use tower::ServiceExt;

    fn make_test_state() -> Arc<AppState> {
        let config = crate::routes::config::AppConfig::default();
        let mode_selector = ModeSelector::new(RunningMode::AgriculturalIrrigation, None);
        Arc::new(AppState {
            config: Arc::new(tokio::sync::RwLock::new(config)),
            ai_integrator: Arc::new(AiIntegrator::new()),
            mode_selector: Arc::new(mode_selector),
            sse_push: Arc::new(SsePushService::new(64)),
            audit_logger: Arc::new(AuditLogger::new_noop()),
            session_manager: SessionManager::new("test".to_string()),
        })
    }

    fn make_test_app() -> Router {
        Router::new()
            .route("/api/v1/mode", get(get_mode).put(switch_mode))
            .route("/api/v1/mode/list", get(list_modes))
            .with_state(make_test_state())
    }

    #[tokio::test]
    async fn test_get_mode_returns_200() {
        let app = make_test_app();
        let response = app
            .oneshot(Request::builder().uri("/api/v1/mode").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_mode_list_returns_200() {
        let app = make_test_app();
        let response = app
            .oneshot(Request::builder().uri("/api/v1/mode/list").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_switch_mode_valid_returns_200() {
        let app = make_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/mode")
                    .method("PUT")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"mode":"CommercialArbitrage"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_switch_mode_invalid_returns_400() {
        let app = make_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/mode")
                    .method("PUT")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"mode":"InvalidMode"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
