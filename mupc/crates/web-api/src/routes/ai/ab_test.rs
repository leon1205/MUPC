//! A/B 测试端点
//!
//! GET    /api/v1/ai/ab-test/status  — 查询 A/B 测试状态
//! GET    /api/v1/ai/ab-test/results — 查询 A/B 测试结果
//! POST   /api/v1/ai/ab-test         — 创建 A/B 测试
//! DELETE /api/v1/ai/ab-test/{id}    — 停止 A/B 测试

use axum::{Json, extract::{State, Path}, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::routes::config::AppState;

/// 创建 A/B 测试请求
#[derive(Debug, Deserialize)]
pub struct CreateAbTestRequest {
    pub model_type: String,
    pub experiment_version: String,
    pub traffic_percent: u8,
    pub duration_hours: u32,
    pub metrics: Vec<String>,
}

/// 创建 A/B 测试响应
#[derive(Debug, Serialize)]
pub struct CreateAbTestResponse {
    pub test_id: String,
    pub status: String,
    pub control_version: String,
    pub experiment_version: String,
    pub traffic_percent: u8,
    pub started_at: String,
    pub estimated_end_at: String,
}

/// GET /api/v1/ai/ab-test/status — 获取 A/B 测试状态
pub async fn get_ab_test_status(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}

/// GET /api/v1/ai/ab-test/results — 获取 A/B 测试结果
pub async fn get_ab_test_results(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    todo!("Phase 2+")
}

/// POST /api/v1/ai/ab-test — 创建 A/B 测试
///
/// 校验规则:
/// - experiment_version 必须为 standby 状态
/// - traffic_percent 必须在 1-50 之间
/// - 同一 model_type 不能有运行中的 A/B 测试
pub async fn post_create_ab_test(
    State(_state): State<Arc<AppState>>,
    Json(_req): Json<CreateAbTestRequest>,
) -> Result<Json<CreateAbTestResponse>, StatusCode> {
    todo!("Phase 2+ — 校验实验组版本状态，创建 AbTestRouter 条目，5s 内生效，写审计日志")
}

/// DELETE /api/v1/ai/ab-test/{id} — 停止 A/B 测试
pub async fn delete_ab_test(
    State(_state): State<Arc<AppState>>,
    Path(_test_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    todo!("Phase 2+ — 从 AbTestRouter 移除，5s 内恢复全量流量到对照组，生成最终报告")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::{Request, StatusCode}, routing::get, Router};
    use tower::ServiceExt;

    fn make_test_router() -> Router {
        Router::new()
            .route("/api/v1/ai/ab-test/status", get(get_ab_test_status))
            .route("/api/v1/ai/ab-test/results", get(get_ab_test_results))
    }

    #[tokio::test]
    async fn test_get_ab_test_status_route_exists() {
        let app = make_test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ai/ab-test/status")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // todo!() panics → 500, but route is registered
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_get_ab_test_results_route_exists() {
        let app = make_test_router();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/ai/ab-test/results")
                    .method("GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
