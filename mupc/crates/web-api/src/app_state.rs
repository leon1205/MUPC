//! 应用共享状态
//!
//! 聚合 web-api 层所需的所有共享状态组件，
//! 通过 Axum State extractor 注入到各路由处理器。

use std::sync::Arc;
use tokio::sync::RwLock;

use mupc_ai_engine::mode_selector::ModeSelector;
use mupc_strategy_engine::AiIntegrator;
use crate::audit::AuditLogger;
use crate::auth::SessionManager;
use crate::routes::config::AppConfig;
use crate::sse::SsePushService;

/// 集成应用状态
///
/// 所有路由处理器通过 `State<Arc<AppState>>` 访问。
#[derive(Clone)]
pub struct AppState {
    /// 系统配置（可读写）
    pub config: Arc<RwLock<AppConfig>>,
    /// AI 引擎集成器
    pub ai_integrator: Arc<AiIntegrator>,
    /// 运行模式选择器
    pub mode_selector: Arc<ModeSelector>,
    /// SSE 推送服务
    pub sse_push: Arc<SsePushService>,
    /// 审计日志记录器
    pub audit_logger: Arc<AuditLogger>,
    /// Session 管理器
    pub session_manager: SessionManager,
}

impl AppState {
    /// 创建应用状态
    pub fn new(
        config: AppConfig,
        ai_integrator: AiIntegrator,
        mode_selector: ModeSelector,
        sse_push: SsePushService,
        audit_logger: AuditLogger,
        session_manager: SessionManager,
    ) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            ai_integrator: Arc::new(ai_integrator),
            mode_selector: Arc::new(mode_selector),
            sse_push: Arc::new(sse_push),
            audit_logger: Arc::new(audit_logger),
            session_manager,
        }
    }
}
