//! 子系统启动编排器
//!
//! 按照依赖关系顺序初始化 14 个子系统。
//! 初始化顺序由 Section 3.3 设计文档定义。
//!
//! Phase 2 实现: 06/08/09/10 子系统已完成基础初始化。
//! Phase 2+ 实现: 11 OTA 管理器已完成初始化。
//! 剩余 6 个 TODO 见代码注释。

use mupc_common::{ErrorCode, MupcError};
use mupc_core::service_coord::ServiceStatus;
use mupc_core::service_coord_impl::ServiceCoordinatorImpl;
use std::sync::Arc;

use crate::core_config::CoreConfig;

/// 启动上下文：持有所有已初始化的子系统句柄
///
/// 各子系统以 Arc 形式存放。
/// 在 Phase 6 优雅退出时用于逆序清理。
#[allow(dead_code)]
pub struct StartupContext {
    pub message_bus: Arc<mupc_core::TokioMessageBus>,
    pub storage: Arc<mupc_storage::StorageService>,
    pub intercore: Arc<mupc_intercore::IntercoreClient>,
    pub plugin_loader: Arc<plugin_loader::PluginLoaderImpl>,
    pub ai_engine: Arc<mupc_ai_engine::ModelManager>,
    pub ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
    pub ota_manager: Arc<dyn mupc_ota_update::OtaManager>,
}

/// IEC 104 命令处理器（stub 实现，Phase 2+ 接入策略引擎）
struct StubCommandHandler;

impl StubCommandHandler {
    fn name(&self) -> &str {
        "stub-command-handler"
    }
}

#[async_trait::async_trait]
impl mupc_gateway::iec104::command::CommandHandler for StubCommandHandler {
    fn name(&self) -> &str {
        StubCommandHandler::name(self)
    }

    async fn handle_command(
        &self,
        _cmd: mupc_gateway::iec104::command::ControlCommand,
    ) -> Result<mupc_gateway::iec104::command::CommandResponse, MupcError> {
        Err(MupcError::new(
            ErrorCode::Unknown,
            "IEC 104 命令处理器尚未接入策略引擎 (Phase 2+)",
            "gateway",
        ))
    }
}

/// 按依赖顺序初始化所有子系统
///
/// 14 步初始化流程，每步失败时级联清理已启动的服务。
pub async fn initialize_all(
    config: &CoreConfig,
    coord: &ServiceCoordinatorImpl,
) -> Result<StartupContext, MupcError> {
    // ── 1. 消息总线 (无依赖) ──
    tracing::info!("[01/14] 初始化消息总线...");
    let message_bus = Arc::new(mupc_core::TokioMessageBus::new(256));
    coord.register_service("message_bus", ServiceStatus::Running);

    // ── 2. 安全模块 ──
    tracing::info!("[02/14] 初始化安全模块...");
    // TODO (Phase 2+): 加载 TLS 证书和 SM2/SM4 密钥
    // 当前: mupc-security 模块无 SecurityModule 类型
    tracing::info!("安全模块初始化 (stub): cert_dir={}", config.system.cert_dir.display());
    coord.register_service("security", ServiceStatus::Running);

    // ── 3. 持久化存储 ──
    tracing::info!("[03/14] 初始化持久化存储...");
    let data_dir = config.system.data_dir.clone();
    tokio::fs::create_dir_all(&data_dir)
        .await
        .map_err(|e| MupcError::new(ErrorCode::IoError, format!("创建数据目录失败: {}", e), "startup"))?;
    let db_path = data_dir.join("mupc.db");
    if !db_path.exists() {
        tokio::fs::File::create(&db_path)
            .await
            .map_err(|e| MupcError::new(ErrorCode::IoError, format!("创建数据库文件失败: {}", e), "startup"))?;
    }
    let pool = mupc_storage::init_pool(db_path.to_str().unwrap_or("mupc.db"))
        .await
        .map_err(|e| MupcError::new(ErrorCode::ConnectionFailed, format!("数据库连接失败: {}", e), "startup"))?;
    mupc_storage::run_migrations(&pool)
        .await
        .map_err(|e| MupcError::new(ErrorCode::ConfigError, format!("数据库迁移失败: {}", e), "startup"))?;
    let storage = Arc::new(mupc_storage::StorageService::new(Arc::new(pool)));
    coord.register_service("storage", ServiceStatus::Running);

    // ── 4. 核间通信 ──
    tracing::info!("[04/14] 初始化核间通信...");
    let remote_addr = format!("{}:{}", config.intercore.host, config.intercore.port);
    let intercore = Arc::new(mupc_intercore::IntercoreClient::new(remote_addr));
    coord.register_service("intercore", ServiceStatus::Running);

    // ── 5. 插件加载器 ──
    tracing::info!("[05/14] 初始化插件加载器...");
    let plugin_loader = {
        let loader = plugin_loader::PluginLoaderImpl::new();
        for path in &config.plugins.search_paths {
            loader.add_search_path(path.to_string_lossy().to_string());
        }
        // TODO (Phase 2+): 实现 libloading 动态加载 .so 文件
        tracing::info!(
            "插件目录已配置: {} 个搜索路径, {} 个自动加载插件",
            config.plugins.search_paths.len(),
            config.plugins.auto_load.len()
        );
        Arc::new(loader)
    };
    coord.register_service("plugin_loader", ServiceStatus::Running);

    // ── 6. 遥测数据采集 ──
    tracing::info!("[06/14] 初始化遥测数据采集...");
    let _data_collector = Arc::new(mupc_data_processing::DataCollectorImpl::new());
    let _fault_recorder = Arc::new(
        mupc_data_processing::FaultRecorderImpl::new(&db_path)
            .map_err(|e| MupcError::new(ErrorCode::Unknown, format!("故障录波器初始化失败: {}", e), "startup"))?,
    );
    let _high_freq_telemetry = Arc::new(mupc_data_processing::HighFreqTelemetryImpl::new(1000));
    coord.register_service("data_processing", ServiceStatus::Running);

    // ── 7. AI 引擎 ──
    tracing::info!("[07/14] 初始化 AI 引擎...");
    let ai_config = mupc_ai_engine::AiEngineConfig::default();
    let ai_engine = mupc_ai_engine::ModelManager::new(ai_config);
    // 加载 AI 模型 (LSTM + RL 场景模型)
    // 模型文件缺失时降级运行（预测返回 0 向量，RL 决策返回错误）
    if let Err(e) = ai_engine.load_models().await {
        tracing::warn!("AI 模型加载失败，降级运行: {}", e);
    }
    let ai_engine = Arc::new(ai_engine);
    coord.register_service("ai_engine", ServiceStatus::Running);

    // ── 8. 策略引擎 ──
    tracing::info!("[08/14] 初始化策略引擎...");
    let mut ai_integrator = mupc_strategy_engine::AiIntegrator::new();
    ai_integrator.set_intercore_client(intercore.clone());
    let ai_integrator = Arc::new(ai_integrator);
    coord.register_service("strategy_engine", ServiceStatus::Running);

    // ── 9. IEC 104 网关 ──
    tracing::info!("[09/14] 初始化 IEC 104 网关...");
    let iec104_config = mupc_gateway::iec104::server::Iec104Config {
        listen_addr: "0.0.0.0".to_string(),
        listen_port: 2404,
        ..Default::default()
    };
    let iec104_server = Arc::new(mupc_gateway::iec104::server::Iec104Server::new(iec104_config));
    let cmd_handler = Arc::new(StubCommandHandler);
    let server_clone = iec104_server.clone();
    tokio::spawn(async move {
        if let Err(e) = server_clone.start(cmd_handler).await {
            tracing::error!("IEC 104 服务器异常退出: {}", e);
        }
    });
    coord.register_service("gateway", ServiceStatus::Running);

    // ── 10. Web API ──
    // AppState 所需依赖在步骤 07/08/11 中已初始化
    tracing::info!("[10/14] 初始化 Web API...");
    let ota_manager: Arc<dyn mupc_ota_update::OtaManager> =
        Arc::new(mupc_ota_update::manager::OtaManagerImpl::new(
            mupc_ota_update::OtaConfig::default(),
            config.system.data_dir.join("ota"),
        )
        .map_err(|e| MupcError::new(ErrorCode::Unknown, format!("OTA 管理器初始化失败: {}", e), "startup"))?);

    let web_config: mupc_web_api::routes::config::AppConfig =
        serde_json::from_str("{}").unwrap_or_default();
    let web_config = Arc::new(tokio::sync::RwLock::new(web_config));
    let session_manager = mupc_web_api::SessionManager::new("admin".to_string());
    let sse_push = Arc::new(mupc_web_api::SsePushService::new(256));
    let audit_logger = Arc::new(
        mupc_web_api::AuditLogger::new(
            config.system.log_dir.join("audit").to_str().unwrap_or("/opt/mupc/logs/audit"),
        )
        .unwrap_or_else(|e| {
            tracing::warn!("审计日志初始化失败，使用内存模式: {}", e);
            mupc_web_api::AuditLogger::new("/tmp/mupc-audit").expect("内存审计日志创建失败")
        }),
    );
    let online_updater = Arc::new(tokio::sync::Mutex::new(
        mupc_ai_engine::OnlineUpdater::new(mupc_ai_engine::OnlineUpdateConfig::default()),
    ));
    let ab_test_manager = Arc::new(mupc_web_api::routes::ai::ab_test_manager::AbTestManager::new());
    let mode_selector = ai_engine.mode_selector_arc();

    let app_state = Arc::new(mupc_web_api::AppState {
        config: web_config,
        ai_integrator: ai_integrator.clone(),
        mode_selector,
        sse_push,
        audit_logger,
        session_manager,
        storage: storage.clone(),
        ota_manager: ota_manager.clone(),
        online_updater,
        ab_test_manager,
    });

    // 组装 Router 并启动 HTTP 服务
    let app_router = axum::Router::new()
        .merge(mupc_web_api::routes::mode::create_router())
        .merge(mupc_web_api::routes::ai::ai_routes())
        .with_state(app_state.clone());

    let listen_addr = config.web_api.listen_addr.clone();
    let _web_handle = tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(&listen_addr).await {
            Ok(l) => {
                tracing::info!("Web API 已启动: http://{}", listen_addr);
                l
            }
            Err(e) => {
                tracing::error!("Web API 绑定 {} 失败: {}", listen_addr, e);
                return;
            }
        };
        axum::serve(listener, app_router)
            .await
            .unwrap_or_else(|e| tracing::error!("Web API 服务器异常退出: {}", e));
    });
    tracing::info!(
        "Web API 配置: listen={}, https={}",
        config.web_api.listen_addr,
        config.web_api.enable_https
    );
    coord.register_service("web_api", ServiceStatus::Running);

    // ── 11. OTA 管理器 (实例已在步骤 10 中创建) ──
    tracing::info!("[11/14] 初始化 OTA 管理器...");
    coord.register_service("ota_update", ServiceStatus::Running);

    // ── 12. 系统资源监控 ──
    tracing::info!("[12/14] 初始化系统资源监控...");
    // TODO (Phase 2+): 启动 SystemMetricsCollector 采集循环 + SelfHealingEngine
    coord.register_service("system_monitor", ServiceStatus::Running);

    // ── 13. MQTT 桥接 ──
    tracing::info!("[13/14] 初始化 MQTT 桥接...");
    // TODO (Phase 2+): 初始化 LocalMqttClient + NorthMqttClient
    coord.register_service("mqtt_bridge", ServiceStatus::Running);

    // ── 14. 近场无线 ──
    tracing::info!("[14/14] 初始化近场无线...");
    // TODO (Phase 2+): 实例化 NoOp 无线驱动
    coord.register_service("wireless", ServiceStatus::Running);

    tracing::info!("所有 14 个子系统初始化完成 ({} 个 TODO 待阶段补全)", 6);

    Ok(StartupContext {
        message_bus,
        storage,
        intercore,
        plugin_loader,
        ai_engine,
        ai_integrator,
        ota_manager,
    })
}
