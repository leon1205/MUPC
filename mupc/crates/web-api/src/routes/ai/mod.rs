//! AI 可视化与专家干预 REST API
//!
//! 提供 AI 引擎状态监控、决策可视化、场景管理、
//! 权重调整、A/B 测试、预测结果查询、审计日志等端点

mod decision;
mod prediction;
mod scene;
mod weights;
mod ab_test;
mod model;
mod status;
mod history;
mod intervention;
mod explain;
mod metrics;
mod rewards;
mod finetuning;
mod audit;
mod rollback;

use axum::{Router, routing::{get, put, post, delete}};
use std::sync::Arc;
use crate::AppState;

/// 注册 AI 可视化路由
///
/// Phase 2+ 需要提供 Arc<AppState> 状态来启用 AI 引擎查询。
pub fn ai_routes() -> Router {
    Router::new()
        // 预测 (v1.2 对齐: 统一根路径 + 可选 type 参数)
        .route("/api/v1/ai/predictions", get(prediction::get_predictions))
        .route("/api/v1/ai/predictions/current", get(prediction::get_current_prediction))
        .route("/api/v1/ai/predictions/history", get(prediction::get_prediction_history))
        // 决策 (v1.2 对齐: /api/v1/ai/decision 为主端点)
        .route("/api/v1/ai/decisions", get(decision::get_decisions))
        .route("/api/v1/ai/decisions/latest", get(decision::get_latest_decision))
        .route("/api/v1/ai/decisions/{id}", get(decision::get_decision_detail))
        // 场景
        .route("/api/v1/ai/scenes/current", get(scene::get_current_scene))
        .route("/api/v1/ai/scenes/history", get(scene::get_scene_history))
        // 权重 (v1.2 新增 PUT)
        .route("/api/v1/ai/weights", get(weights::get_weights).put(weights::put_update_weights))
        // 模型
        .route("/api/v1/ai/models/status", get(model::get_model_status))
        .route("/api/v1/ai/models/metrics", get(model::get_model_metrics))
        // A/B 测试
        .route("/api/v1/ai/ab-test/status", get(ab_test::get_ab_test_status))
        .route("/api/v1/ai/ab-test/results", get(ab_test::get_ab_test_results))
        .route("/api/v1/ai/ab-test", post(ab_test::post_create_ab_test))
        .route("/api/v1/ai/ab-test/{id}", delete(ab_test::delete_ab_test))
        // 干预历史
        .route("/api/v1/ai/interventions", get(intervention::get_interventions))
        // 系统状态
        .route("/api/v1/ai/status", get(status::get_ai_status))
        // 决策解释
        .route("/api/v1/ai/explain", get(explain::get_explanation))
        // 实时指标
        .route("/api/v1/ai/metrics", get(metrics::get_realtime_metrics))
        // 历史记录
        .route("/api/v1/ai/history/decisions", get(history::get_decision_history))
        .route("/api/v1/ai/history/accuracy", get(history::get_prediction_accuracy_history))
        // v1.2 新增: 奖励值
        .route("/api/v1/ai/rewards", get(rewards::get_rewards))
        // v1.2 新增: 在线微调
        .route("/api/v1/ai/finetuning", get(finetuning::get_finetuning))
        // v1.2 新增: 审计日志
        .route("/api/v1/ai/audit", get(audit::get_audit_log))
        // v1.2 新增: 模型回滚
        .route("/api/v1/ai/rollback", post(rollback::post_rollback))
}

/// 注册 SSE 实时推送路由（需要 AppState）
///
/// GET /api/v1/ai/stream
/// 与主 AI 路由分开，因为需要访问 SsePushService。
pub fn sse_route() -> Router {
    use axum::routing::get;
    Router::new()
        .route("/api/v1/ai/stream", get(crate::sse::sse_handler))
}
