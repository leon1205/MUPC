//! 认证模块
//!
//! Session 登录认证

use axum::{
    extract::{State, rejection::JsonRejection, FromRequestParts},
    http::{StatusCode, HeaderMap, HeaderName, HeaderValue},
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use chrono::{DateTime, Utc, Duration};

use mupc_common::{MupcError, ErrorCode};

/// 登录请求
#[derive(Debug, Clone, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub remember: bool,
}

/// 登录响应
#[derive(Debug, Clone, Serialize)]
pub struct LoginResponse {
    pub session_id: String,
    pub expires_at: String,
    pub username: String,
}

/// Session 信息
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub username: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Session 过期检查容差时间（秒）
const SESSION_EXPIRY_TOLERANCE_SECS: i64 = 30;

impl Session {
    pub fn is_expired(&self) -> bool {
        Utc::now() + Duration::seconds(SESSION_EXPIRY_TOLERANCE_SECS) > self.expires_at
    }
}

/// Session 管理器
#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    default_admin_password: String,
}

impl SessionManager {
    pub fn new(default_admin_password: String) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            default_admin_password,
        }
    }

    /// 登录
    pub async fn login(&self, username: &str, password: &str, remember: bool) -> Result<Session, MupcError> {
        // 验证用户名和密码
        // Phase 1: 简单验证 admin 用户
        if username != "admin" || password != self.default_admin_password {
            return Err(MupcError::new(ErrorCode::AuthFailed, "Invalid username or password", "web-api"));
        }

        let now = Utc::now();
        let expires_at = if remember {
            now + Duration::days(30)
        } else {
            now + Duration::hours(24)
        };

        let session = Session {
            id: Uuid::new_v4().to_string(),
            username: username.to_string(),
            created_at: now,
            expires_at,
        };

        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session.clone());

        Ok(session)
    }

    /// 验证 session
    pub async fn validate(&self, session_id: &str) -> Result<Session, MupcError> {
        let sessions = self.sessions.read().await;

        let session = sessions.get(session_id)
            .ok_or_else(|| MupcError::new(ErrorCode::InvalidSession, "Session not found", "web-api"))?;

        if session.is_expired() {
            return Err(MupcError::new(ErrorCode::InvalidSession, "Session expired", "web-api"));
        }

        Ok(session.clone())
    }

    /// 登出
    pub async fn logout(&self, session_id: &str) -> Result<(), MupcError> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(())
    }
}

/// 认证处理器
#[derive(Clone)]
pub struct AuthHandler {
    session_manager: SessionManager,
}

impl AuthHandler {
    pub fn new(session_manager: SessionManager) -> Self {
        Self { session_manager }
    }

    pub fn session_manager(&self) -> &SessionManager {
        &self.session_manager
    }
}

const SESSION_HEADER: &str = "X-Session-Id";

/// POST /api/auth/login - 登录
async fn login(
    State(handler): State<AuthHandler>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    let session = handler
        .session_manager()
        .login(&req.username, &req.password, req.remember)
        .await
        .map_err(|e| {
            tracing::warn!("Login failed: {}", e);
            StatusCode::UNAUTHORIZED
        })?;

    Ok(Json(LoginResponse {
        session_id: session.id,
        expires_at: session.expires_at.to_rfc3339(),
        username: session.username,
    }))
}

/// 创建认证路由
pub fn create_router(handler: AuthHandler) -> Router {
    Router::new()
        .route("/api/auth/login", post(login))
        .with_state(handler)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试用密码（从环境变量获取或使用默认值供测试）
    fn get_test_password() -> String {
        std::env::var("TEST_ADMIN_PASSWORD").unwrap_or_else(|_| "test_password_for_unit_tests_only".to_string())
    }

    // ========== Session Tests ==========

    #[test]
    fn test_session_is_expired() {
        use chrono::{Duration, Utc};

        // 创建未过期的 session
        let valid_session = Session {
            id: "test-session".to_string(),
            username: "admin".to_string(),
            created_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(1),
        };
        assert!(!valid_session.is_expired());

        // 创建已过期的 session
        let expired_session = Session {
            id: "expired-session".to_string(),
            username: "admin".to_string(),
            created_at: Utc::now() - Duration::hours(2),
            expires_at: Utc::now() - Duration::hours(1),
        };
        assert!(expired_session.is_expired());
    }

    // ========== SessionManager Tests ==========

    #[tokio::test]
    async fn test_session_manager_login_success() {
        let password = get_test_password();
        let manager = SessionManager::new(password.clone());
        let result = manager.login("admin", &password, false).await;

        assert!(result.is_ok());
        let session = result.unwrap();
        assert_eq!(session.username, "admin");
        assert!(!session.id.is_empty());
    }

    #[tokio::test]
    async fn test_session_manager_login_wrong_password() {
        let manager = SessionManager::new(get_test_password());
        let result = manager.login("admin", "wrongpassword", false).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_session_manager_login_wrong_username() {
        let manager = SessionManager::new(get_test_password());
        let result = manager.login("wronguser", "secret123", false).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_session_manager_login_remember() {
        let password = get_test_password();
        let manager = SessionManager::new(password.clone());

        // 不记住登录
        let session_short = manager.login("admin", &password, false).await.unwrap();
        let short_duration = session_short.expires_at - session_short.created_at;
        assert!(short_duration.num_hours() <= 24);

        // 记住登录
        let session_long = manager.login("admin", &password, true).await.unwrap();
        let long_duration = session_long.expires_at - session_long.created_at;
        assert!(long_duration.num_days() >= 30);
    }

    #[tokio::test]
    async fn test_session_manager_validate_success() {
        let password = get_test_password();
        let manager = SessionManager::new(password.clone());
        let session = manager.login("admin", &password, false).await.unwrap();

        let result = manager.validate(&session.id).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, session.id);
    }

    #[tokio::test]
    async fn test_session_manager_validate_not_found() {
        let manager = SessionManager::new(get_test_password());
        let result = manager.validate("non-existent-session").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_session_manager_validate_expired() {
        use chrono::{Duration, Utc};

        let manager = SessionManager::new(get_test_password());

        // 手动创建一个已过期的 session
        let mut sessions = manager.sessions.write().await;
        sessions.insert(
            "expired-session".to_string(),
            Session {
                id: "expired-session".to_string(),
                username: "admin".to_string(),
                created_at: Utc::now() - Duration::hours(2),
                expires_at: Utc::now() - Duration::hours(1),
            },
        );
        drop(sessions);

        let result = manager.validate("expired-session").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_session_manager_logout() {
        let password = get_test_password();
        let manager = SessionManager::new(password.clone());
        let session = manager.login("admin", &password, false).await.unwrap();

        // 验证 session 存在
        let result = manager.validate(&session.id).await;
        assert!(result.is_ok());

        // 登出
        let logout_result = manager.logout(&session.id).await;
        assert!(logout_result.is_ok());

        // 验证 session 不存在
        let result = manager.validate(&session.id).await;
        assert!(result.is_err());
    }
}

// ============================================================
// 四角色权限系统（Phase 2+ 实现 JWT/Session 集成）
// ============================================================

/// 用户角色
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    /// 查看者：只读访问仪表盘和报告
    Viewer,
    /// 操作员：可执行设备控制和参数调整
    Operator,
    /// AI 专家：可调整 AI 模型权重和执行 A/B 测试
    AiExpert,
    /// 管理员：完整系统管理权限
    Admin,
}

impl UserRole {
    /// 获取角色权限级别（数字越大权限越高）
    pub fn level(&self) -> u8 {
        match self {
            UserRole::Viewer => 0,
            UserRole::Operator => 1,
            UserRole::AiExpert => 2,
            UserRole::Admin => 3,
        }
    }

    /// 检查是否有足够权限
    pub fn can_access(&self, required: &UserRole) -> bool {
        self.level() >= required.level()
    }

    /// 从字符串解析角色
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "viewer" => Some(UserRole::Viewer),
            "operator" => Some(UserRole::Operator),
            "aiexpert" | "ai_expert" => Some(UserRole::AiExpert),
            "admin" => Some(UserRole::Admin),
            _ => None,
        }
    }
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserRole::Viewer => write!(f, "viewer"),
            UserRole::Operator => write!(f, "operator"),
            UserRole::AiExpert => write!(f, "ai_expert"),
            UserRole::Admin => write!(f, "admin"),
        }
    }
}

/// 用户会话信息
#[derive(Debug, Clone)]
pub struct UserSession {
    /// 用户唯一标识
    pub user_id: String,
    /// 用户名
    pub username: String,
    /// 用户角色
    pub role: UserRole,
    /// JWT / Session 令牌
    pub token: String,
}

/// 权限守卫 —— 从请求中提取用户角色并进行权限检查
///
/// 作为 Axum extractor 使用:
/// ```ignore
/// async fn admin_only(RequireRole(UserRole::Admin): RequireRole) -> impl IntoResponse {
///     // 只有 Admin 能访问
/// }
/// ```
pub struct RequireRole(pub UserRole);

impl<S> FromRequestParts<S> for RequireRole
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(
        _parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        todo!("Phase 2+ — extract JWT/session token, verify, check role")
    }
}

/// 角色权限检查中间件（Phase 2+ 实现）
///
/// 创建指定角色才能访问的中间件层。
pub fn require_role(_required: UserRole) {
    todo!("Phase 2+ — tower middleware that validates user role from JWT/session")
}