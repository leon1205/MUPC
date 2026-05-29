//! 模型回滚端点
//!
//! POST /api/v1/ai/rollback — 执行模型回滚

use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};

/// 回滚请求
#[derive(Debug, Deserialize)]
pub struct RollbackRequest {
    pub model_type: String,
    pub target_version: String,
    pub reason: String,
    pub password: String,
}

/// 回滚响应
#[derive(Debug, Serialize)]
pub struct RollbackResponse {
    pub status: String,
    pub previous_version: String,
    pub current_version: String,
    pub rolled_back_at: String,
    pub warmup_result: String,
}

/// POST /api/v1/ai/rollback
///
/// 需要三级确认（前端） + 密码二次身份验证。
/// 回滚执行时间 < 60 秒（含模型加载和预热）。
pub async fn post_rollback(
    Json(_req): Json<RollbackRequest>,
) -> Result<Json<RollbackResponse>, StatusCode> {
    todo!("Phase 2+ — 校验密码，执行模型回滚流程: 备份当前 → 加载目标版本 → 预热推理 → 更新 manifest.json → 写审计日志")
}
