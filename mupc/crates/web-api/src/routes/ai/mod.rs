//! AI 可视化与专家干预 REST API
//!
//! 提供 AI 引擎状态监控、决策可视化、场景管理、
//! 权重调整、A/B 测试和预测结果查询等 14 个端点

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

pub use decision::*;
pub use prediction::*;
pub use scene::*;
pub use weights::*;
pub use ab_test::*;
pub use model::*;
pub use status::*;
pub use history::*;
pub use intervention::*;
pub use explain::*;
pub use metrics::*;

use axum::{Router, routing::get};
use std::sync::Arc;
use crate::routes::config::AppState;

/// 注册 AI 可视化路由
///
/// Phase 2+ 需要提供 Arc<AppState> 状态来启用 AI 引擎查询。
pub fn ai_routes() -> Router {
    Router::new()
        // 预测
        .route("/api/v1/ai/predictions", get(prediction::get_predictions))
        .route("/api/v1/ai/predictions/current", get(prediction::get_current_prediction))
        .route("/api/v1/ai/predictions/history", get(prediction::get_prediction_history))
        // 决策
        .route("/api/v1/ai/decisions", get(decision::get_decisions))
        .route("/api/v1/ai/decisions/latest", get(decision::get_latest_decision))
        .route("/api/v1/ai/decisions/:id", get(decision::get_decision_detail))
        // 场景
        .route("/api/v1/ai/scenes/current", get(scene::get_current_scene))
        .route("/api/v1/ai/scenes/history", get(scene::get_scene_history))
        // 权重
        .route("/api/v1/ai/weights", get(weights::get_weights))
        // 模型
        .route("/api/v1/ai/models/status", get(model::get_model_status))
        .route("/api/v1/ai/models/metrics", get(model::get_model_metrics))
        // A/B测试
        .route("/api/v1/ai/ab-test/status", get(ab_test::get_ab_test_status))
        .route("/api/v1/ai/ab-test/results", get(ab_test::get_ab_test_results))
        // 干预历史
        .route("/api/v1/ai/interventions", get(intervention::get_interventions))
}
