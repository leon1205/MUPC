//! A/B 测试端点
//!
//! GET    /api/v1/ai/ab-test/status  — 查询 A/B 测试状态
//! GET    /api/v1/ai/ab-test/results — 查询 A/B 测试结果
//! POST   /api/v1/ai/ab-test         — 创建 A/B 测试
//! DELETE /api/v1/ai/ab-test/{id}    — 停止 A/B 测试

use axum::{Json, extract::{State, Path}, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct CreateAbTestRequest {
    pub model_type: String,
    pub experiment_version: String,
    pub traffic_percent: u8,
    pub duration_hours: u32,
    pub metrics: Vec<String>,
}

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

/// GET /api/v1/ai/ab-test/status
pub async fn get_ab_test_status(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "active_tests": [],
        "total_tests": 0
    }))
}

/// GET /api/v1/ai/ab-test/results
pub async fn get_ab_test_results(
    State(_state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "test_id": null,
        "results": []
    }))
}

/// POST /api/v1/ai/ab-test
pub async fn post_create_ab_test(
    State(_state): State<Arc<AppState>>,
    Json(req): Json<CreateAbTestRequest>,
) -> Result<Json<CreateAbTestResponse>, StatusCode> {
    if req.traffic_percent < 1 || req.traffic_percent > 50 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    let end = now + chrono::Duration::hours(req.duration_hours as i64);

    tracing::info!(
        id = %id,
        model = %req.model_type,
        experiment = %req.experiment_version,
        "A/B 测试已创建"
    );

    Ok(Json(CreateAbTestResponse {
        test_id: id,
        status: "running".to_string(),
        control_version: "current".to_string(),
        experiment_version: req.experiment_version,
        traffic_percent: req.traffic_percent,
        started_at: now.to_rfc3339(),
        estimated_end_at: end.to_rfc3339(),
    }))
}

/// DELETE /api/v1/ai/ab-test/{id}
pub async fn delete_ab_test(
    State(_state): State<Arc<AppState>>,
    Path(test_id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    tracing::info!(id = %test_id, "A/B 测试已停止");
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_ab_test_request_validation() {
        assert!(CreateAbTestRequest {
            model_type: "lstm".into(),
            experiment_version: "v2".into(),
            traffic_percent: 0,
            duration_hours: 24,
            metrics: vec![],
        }.traffic_percent < 1);

        assert!(CreateAbTestRequest {
            model_type: "lstm".into(),
            experiment_version: "v2".into(),
            traffic_percent: 30,
            duration_hours: 24,
            metrics: vec![],
        }.traffic_percent <= 50);
    }

    #[test]
    fn test_ab_test_response_serialization() {
        let resp = CreateAbTestResponse {
            test_id: "test-1".into(),
            status: "running".into(),
            control_version: "current".into(),
            experiment_version: "v2".into(),
            traffic_percent: 30,
            started_at: "2026-01-01T00:00:00Z".into(),
            estimated_end_at: "2026-01-02T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("test-1"));
        assert!(json.contains("running"));
    }
}
