//! 应用共享状态
//!
//! 聚合 web-api 层所需的所有共享状态组件，
//! 通过 Axum State extractor 注入到各路由处理器。

use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use mupc_ai_engine::mode_selector::ModeSelector;
use mupc_ai_engine::online_updater::OnlineUpdater;
use tokio::sync::RwLock as TokioRwLock;
use mupc_ota_update::manager::OtaManager;
use mupc_storage::services::StorageService;
use mupc_strategy_engine::AiIntegrator;
use crate::audit::AuditLogger;
use crate::auth::SessionManager;
use crate::routes::ai::ab_test_manager::AbTestManager;
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
    /// 运行模式选择器（v2.3: RwLock 以支持初始化阶段注入 ModelRegistry）
    pub mode_selector: Arc<TokioRwLock<ModeSelector>>,
    /// SSE 推送服务
    pub sse_push: Arc<SsePushService>,
    /// 审计日志记录器
    pub audit_logger: Arc<AuditLogger>,
    /// Session 管理器
    pub session_manager: SessionManager,
    /// 持久化存储服务
    pub storage: Arc<StorageService>,
    /// OTA 管理器
    pub ota_manager: Arc<dyn OtaManager>,
    /// 在线微调更新器
    pub online_updater: Arc<Mutex<OnlineUpdater>>,
    /// A/B 测试管理器
    pub ab_test_manager: Arc<AbTestManager>,
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
        storage: StorageService,
        ota_manager: Arc<dyn OtaManager>,
        online_updater: OnlineUpdater,
        ab_test_manager: AbTestManager,
    ) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            ai_integrator: Arc::new(ai_integrator),
            mode_selector: Arc::new(TokioRwLock::new(mode_selector)),
            sse_push: Arc::new(sse_push),
            audit_logger: Arc::new(audit_logger),
            session_manager,
            storage: Arc::new(storage),
            ota_manager,
            online_updater: Arc::new(Mutex::new(online_updater)),
            ab_test_manager: Arc::new(ab_test_manager),
        }
    }
}
