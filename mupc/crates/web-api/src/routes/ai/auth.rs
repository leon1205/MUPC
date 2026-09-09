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
/// ⚠️ **角色模型未实现（技术债 U-01）**：本提取器仅校验 session 有效（存在 + 未过期），
/// 且恒返回最严 `Admin`——`operator()`/`viewer()` 等所要求角色**不被兑现**。
/// **勿据此做降权/放权判断**（不要相信"要求了 viewer 就真按只读放行"）。
/// 真实 RBAC（users/session 落库、按角色鉴权）待 U-01 另行实现。
/// `username` 承载已验证 session 的操作者名（真实 RBAC 前恒为 "admin"），供 handler 审计留痕。
pub struct RequireRole {
    pub role: Role,
    /// 已验证 session 的用户名（角色未分层 U-01 前恒为 "admin"）
    username: Option<String>,
}

impl RequireRole {
    /// 要求管理员角色
    ///
    /// ⚠️ 角色未分层（U-01）：`from_request_parts` 未按此 role 授权，有效 session 即返回最严 Admin。
    pub fn admin() -> Self {
        Self {
            role: Role::Admin,
            username: None,
        }
    }

    /// 要求操作员角色
    ///
    /// ⚠️ 角色未分层（U-01）：当前不被兑现（恒按 Admin 放行），详见结构 doc。
    pub fn operator() -> Self {
        Self {
            role: Role::Operator,
            username: None,
        }
    }

    /// 要求只读角色
    ///
    /// ⚠️ 角色未分层（U-01）：当前不被兑现（恒按 Admin 放行），详见结构 doc。
    pub fn viewer() -> Self {
        Self {
            role: Role::Viewer,
            username: None,
        }
    }

    /// 已验证 session 的操作者用户名（供 handler 审计留痕；角色未分层 U-01 前恒为 "admin"）
    pub fn username(&self) -> Option<&str> {
        self.username.as_deref()
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
        let session = state
            .session_manager
            .validate(session_id)
            .await
            .map_err(|_| {
                tracing::warn!("AI 路由认证失败: session 无效或已过期");
                StatusCode::UNAUTHORIZED
            })?;

        tracing::debug!(session_id = %session_id, username = %session.username, "AI 路由认证通过");
        // 角色模型未实现（U-01）：有效 session 即按最严 Admin 放行（不兑现 operator/viewer 要求），
        // username 携操作者身份供 handler 审计留痕。
        Ok(RequireRole {
            role: Role::Admin,
            username: Some(session.username),
        })
    }
}
