//! 运行场景管理 API
//!
//! GET  /api/v1/mode      — 获取当前运行场景
//! PUT  /api/v1/mode      — 切换运行场景
//! GET  /api/v1/mode/list — 获取所有可用场景列表

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;
use mupc_ai_engine::mode_selector::{RunningMode, SwitchSource};

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
async fn get_mode(State(state): State<Arc<AppState>>) -> Json<ModeStatusResponse> {
    let current = state.mode_selector.read().await.current();
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
    let new_mode = mupc_ai_engine::parse_mode_name(&req.mode).ok_or(StatusCode::BAD_REQUEST)?;

    let previous = state.mode_selector.read().await.current();

    state
        .mode_selector
        .write()
        .await
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

    #[test]
    fn test_mode_info_list_count() {
        let modes: Vec<ModeInfo> = RunningMode::all()
            .iter()
            .map(|m| ModeInfo {
                id: format!("{:?}", m),
                name: m.display_name().to_string(),
                description: m.description().to_string(),
            })
            .collect();
        assert_eq!(modes.len(), 5);
    }

    #[test]
    fn test_parse_valid_modes() {
        assert!(mupc_ai_engine::parse_mode_name("SeasonalLoadManagement").is_some());
        assert!(mupc_ai_engine::parse_mode_name("CommercialArbitrage").is_some());
        assert!(mupc_ai_engine::parse_mode_name("DemandControl").is_some());
        assert!(mupc_ai_engine::parse_mode_name("VirtualPowerPlant").is_some());
        assert!(mupc_ai_engine::parse_mode_name("UltraGreen").is_some());
    }

    #[test]
    fn test_parse_invalid_mode() {
        assert!(mupc_ai_engine::parse_mode_name("InvalidMode").is_none());
        assert!(mupc_ai_engine::parse_mode_name("").is_none());
    }

    #[test]
    fn test_running_mode_display_names() {
        let all = RunningMode::all();
        for mode in all {
            assert!(!mode.display_name().is_empty());
            assert!(!mode.description().is_empty());
        }
    }
}
