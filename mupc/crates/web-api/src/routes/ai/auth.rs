//! AI 路由认证与角色控制
//!
//! 提供 RequireRole 提取器，用于保护 AI 端点。
//! 通过 X-Session-Id 请求头进行 Session 验证。

use axum::{async_trait, extract::FromRequestParts, http::request::Parts, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

/// 用户角色
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    /// 管理员：完全访问
    Admin,
    /// 操作员：读 + 权重调整 + 回滚
    Operator,
    /// 只读用户
    Viewer,
}

/// 角色守卫提取器
///
/// 在 handler 签名中使用：
/// ```ignore
/// async fn post_rollback(
///     _role: RequireRole,
///     State(state): State<Arc<AppState>>,
///     ...
/// ) -> ...
/// ```
///
/// 默认要求 Admin 角色。使用 `RequireRole::operator()` 或 `RequireRole::viewer()` 放宽限制。
pub struct RequireRole {
    pub role: Role,
}

impl RequireRole {
    /// 要求管理员角色
    pub fn admin() -> Self {
        Self { role: Role::Admin }
    }

    /// 要求操作员角色
    pub fn operator() -> Self {
        Self {
            role: Role::Operator,
        }
    }

    /// 要求只读角色
    pub fn viewer() -> Self {
        Self { role: Role::Viewer }
    }
}

impl Default for RequireRole {
    fn default() -> Self {
        Self::admin()
    }
}

#[async_trait]
impl FromRequestParts<Arc<AppState>> for RequireRole {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let session_id = parts
            .headers
            .get("X-Session-Id")
            .and_then(|v| v.to_str().ok())
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                tracing::warn!("AI 路由认证失败: 缺少 X-Session-Id");
                StatusCode::UNAUTHORIZED
            })?;

        // 真正验证 session（存在 + 未过期），消除「任意非空字符串通过」漏洞
        state
            .session_manager
            .validate(session_id)
            .await
            .map_err(|_| {
                tracing::warn!("AI 路由认证失败: session 无效或已过期");
                StatusCode::UNAUTHORIZED
            })?;

        tracing::debug!(session_id = %session_id, "AI 路由认证通过");
        Ok(RequireRole::admin())
    }
}
