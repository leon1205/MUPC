//! 子系统启动编排器
//!
//! 按照依赖关系顺序初始化 14 个子系统。
//! 初始化顺序由 Section 3.3 设计文档定义。
//!
//! Phase 2 实现: 06/08/09/10 子系统已完成基础初始化。
//! Phase 2+ 实现: 11 OTA 管理器已完成初始化。
//! 剩余 6 个 TODO 见代码注释。

use device_trait::plugin_loader::PluginLoader;
use mupc_common::{ErrorCode, MupcError};
use mupc_core::service_coord::ServiceStatus;
use mupc_core::service_coord_impl::ServiceCoordinatorImpl;
use mupc_system_monitor::MetricCollector;
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
    /// 后台任务句柄（Phase 6 优雅退出时 abort）
    pub background_tasks: Vec<tokio::task::JoinHandle<()>>,
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
    let mut bg_tasks: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    // 错误路径守卫: 初始化中途失败时 abort 所有已启动的后台任务
    struct TaskGuard(Vec<tokio::task::JoinHandle<()>>);
    impl Drop for TaskGuard {
        fn drop(&mut self) {
            for h in &self.0 {
                h.abort();
            }
        }
    }
    let mut guard = TaskGuard(Vec::new());

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
    let db_str = db_path.to_str()
        .expect("数据目录路径包含非法 UTF-8 字符");
    let pool = mupc_storage::init_pool(db_str)
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
        // 自动加载配置的插件（libloading FFI 加载 .so）
        for plugin_name in &config.plugins.auto_load {
            let so_name = format!("lib{}.so", plugin_name);
            tracing::info!("加载插件: {}", so_name);
            match loader.load(&so_name, serde_json::json!({})) {
                Ok(()) => tracing::info!("  {} 加载成功", plugin_name),
                Err(e) => tracing::warn!("  {} 加载失败 (预期内，如 .so 未编译): {}", plugin_name, e),
            }
        }
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
    // DataCollectorImpl / HighFreqTelemetryImpl 为纯数据容器，延迟创建
    // FaultRecorderImpl::new() 初始化 SQLite 连接 + 建表（CREATE IF NOT EXISTS），
    // 完成后即可 drop（连接关闭，表已存在）。后续真正录波时重新打开连接。
    let _fault_recorder = mupc_data_processing::FaultRecorderImpl::new(&db_path)
        .map_err(|e| MupcError::new(ErrorCode::Unknown, format!("故障录波器初始化失败: {}", e), "startup"))?;
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
    ai_integrator.set_south_dispatcher(Arc::new(
        mupc_strategy_engine::SouthCommandDispatcher::with_mock(
            "pv_inverter_001",
            "load_ctrl_001",
        ),
    ));
    ai_integrator.set_model_manager(ai_engine.clone()).await;
    let ai_integrator = Arc::new(ai_integrator);
    coord.register_service("strategy_engine", ServiceStatus::Running);

    // AI 决策循环：周期执行决策并分发到核间/南向（RL 决策 <1s）
    let decision_integrator = ai_integrator.clone();
    guard.0.push(tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            if let Err(e) = decision_integrator.dispatch_ai_decision().await {
                tracing::debug!("AI 决策周期失败: {}", e);
            }
        }
    }));

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
    guard.0.push(tokio::spawn(async move {
        if let Err(e) = server_clone.start(cmd_handler).await {
            tracing::error!("IEC 104 服务器异常退出: {}", e);
        }
    }));
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

    let web_config = Arc::new(tokio::sync::RwLock::new(
        mupc_web_api::routes::config::AppConfig {
            gateway: Default::default(),
            intercore: Default::default(),
            system: Default::default(),
        },
    ));
    // Phase 2+ TODO: 从配置读取管理员用户名，当前硬编码
    let session_manager = mupc_web_api::SessionManager::new("admin".to_string());
    let sse_push = Arc::new(mupc_web_api::SsePushService::new(256));
    let audit_logger = Arc::new(
        mupc_web_api::AuditLogger::new(
            config.system.log_dir.join("audit").to_str().unwrap_or("/opt/mupc/logs/audit"),
        )
        .unwrap_or_else(|e| {
            tracing::warn!("审计日志初始化失败，降级使用 /tmp: {}", e);
            mupc_web_api::AuditLogger::new("/tmp/mupc-audit")
                .expect("审计日志初始化致命失败 — 磁盘满或 /tmp 不可写")
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
    guard.0.push(tokio::spawn(async move {
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
    }));
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
    let metrics_store = Arc::new(mupc_system_monitor::MetricsStore::new(
        config.system.data_dir.join("metrics").to_str().unwrap_or("/tmp/mupc-metrics"),
        30,
    ));
    if let Err(e) = metrics_store.init().await {
        tracing::warn!("指标存储初始化失败: {}", e);
    }
    let metrics_collector = mupc_system_monitor::FullCollector::new(60_000);
    let interval_ms = metrics_collector.collection_interval_ms();
    let collector = Arc::new(metrics_collector);
    let _healing_engine = Arc::new(mupc_system_monitor::SelfHealingEngine::new(3, 30));
    let metrics_bg = metrics_store.clone();
    guard.0.push(tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
            match collector.collect().await {
                Ok(snapshot) => {
                    tracing::debug!(
                        "系统指标: CPU={:.1}% MEM={:.1}% DISK={:.1}% TEMP={:.1}°C",
                        snapshot.cpu.usage_percent,
                        snapshot.memory.usage_percent,
                        snapshot.disk.usage_percent,
                        snapshot.temperature.cpu_temp_c,
                    );
                    if let Err(e) = metrics_bg.store(&snapshot).await {
                        tracing::warn!("保存系统指标失败: {}", e);
                    }
                }
                Err(e) => tracing::warn!("系统指标采集失败: {}", e),
            }
        }
    }));
    coord.register_service("system_monitor", ServiceStatus::Running);

    // ── 13. MQTT 桥接 ──
    tracing::info!("[13/14] 初始化 MQTT 桥接...");
    let _local_mqtt = mupc_mqtt_bridge::LocalMqttClient::new(
        &mupc_mqtt_bridge::LocalMqttConfig::default(),
    )
    .map(Arc::new)
    .inspect_err(|e| tracing::warn!("本地 MQTT 客户端初始化失败: {}", e))
    .ok();
    let _north_mqtt = mupc_mqtt_bridge::NorthMqttClient::new(
        &mupc_mqtt_bridge::NorthMqttConfig::default(),
    )
    .map(Arc::new)
    .inspect_err(|e| tracing::warn!("北向 MQTT 客户端初始化失败: {}", e))
    .ok();
    coord.register_service("mqtt_bridge", ServiceStatus::Running);

    // ── 14. 近场无线 ──
    tracing::info!("[14/14] 初始化近场无线...");
    // TODO (Phase 2+): 实例化 NoOp 无线驱动
    coord.register_service("wireless", ServiceStatus::Running);

    tracing::info!("所有 14 个子系统初始化完成 ({} 个 TODO 待阶段补全)", 2);

    // 初始化成功，取出 bg_tasks（防止 Drop abort）并移交 StartupContext
    let bg_tasks = std::mem::take(&mut guard.0);
    Ok(StartupContext {
        message_bus,
        storage,
        intercore,
        plugin_loader,
        ai_engine,
        ai_integrator,
        ota_manager,
        background_tasks: bg_tasks,
    })
}
