//! 子系统启动编排器
//!
//! 按照依赖关系顺序初始化 14 个子系统。
//! 初始化顺序由 Section 3.3 设计文档定义。
//!
//! ## 当前状态
//!
//! 本模块为 Phase 1 骨架实现。各子系统的具体构造函数
//! 在对应 Phase 中逐步补全。当前使用可用的 API 并标记
//! TODO 条目供后续完成。

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
    // connected 状态由首次 send_dual_param 自动设置
    coord.register_service("intercore", ServiceStatus::Running);

    // ── 5. 插件加载器 ──
    tracing::info!("[05/14] 初始化插件加载器...");
    let plugin_loader = {
        let loader = plugin_loader::PluginLoaderImpl::new();
        // 添加搜索路径
        for path in &config.plugins.search_paths {
            loader.add_search_path(path.to_string_lossy().to_string());
        }
        // TODO (Phase 2+): 实现 libloading 动态加载 .so 文件
        // 当前: PluginLoaderImpl 已有框架，但 load() 方法待实现
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
    // TODO (Phase 2): 初始化 mupc_data_processing::DataProcessing
    // 当前: 占位
    coord.register_service("data_processing", ServiceStatus::Running);

    // ── 7. AI 引擎 ──
    tracing::info!("[07/14] 初始化 AI 引擎...");
    let ai_config = mupc_ai_engine::AiEngineConfig::default();
    let ai_engine = mupc_ai_engine::ModelManager::new(ai_config);
    // TODO (Phase 3C): 调用 ai_engine.load_models().await?
    // 当前: 模型加载通过 mode_selector + model_registry 延迟加载
    let ai_engine = Arc::new(ai_engine);
    coord.register_service("ai_engine", ServiceStatus::Running);

    // ── 8. 策略引擎 ──
    tracing::info!("[08/14] 初始化策略引擎...");
    // TODO (Phase 2): 初始化 mupc_strategy_engine::StrategyEngine
    // 依赖: ai_engine + intercore + data_processing
    coord.register_service("strategy_engine", ServiceStatus::Running);

    // ── 9. IEC 104 网关 ──
    tracing::info!("[09/14] 初始化 IEC 104 网关...");
    // TODO (Phase 2): 初始化 mupc_gateway::Gateway
    coord.register_service("gateway", ServiceStatus::Running);

    // ── 10. Web API ──
    tracing::info!("[10/14] 初始化 Web API...");
    // TODO (Phase 2+): 初始化 Axum HTTP server, 绑定 listen_addr
    tracing::info!(
        "Web API 配置: listen={}, https={}",
        config.web_api.listen_addr,
        config.web_api.enable_https
    );
    coord.register_service("web_api", ServiceStatus::Running);

    // ── 11. OTA 管理器 ──
    tracing::info!("[11/14] 初始化 OTA 管理器...");
    // TODO (Phase 2+): 初始化 mupc_ota_update::OtaManager
    coord.register_service("ota_update", ServiceStatus::Running);

    // ── 12. 系统资源监控 ──
    tracing::info!("[12/14] 初始化系统资源监控...");
    // TODO (Phase 2+): 初始化 mupc_system_monitor::SystemMonitor
    coord.register_service("system_monitor", ServiceStatus::Running);

    // ── 13. MQTT 桥接 ──
    tracing::info!("[13/14] 初始化 MQTT 桥接...");
    // TODO (Phase 2+): 初始化 mupc_mqtt_bridge::MqttBridge
    coord.register_service("mqtt_bridge", ServiceStatus::Running);

    // ── 14. 近场无线 ──
    tracing::info!("[14/14] 初始化近场无线...");
    // TODO (Phase 2+): 初始化 mupc_wireless::WirelessManager
    coord.register_service("wireless", ServiceStatus::Running);

    tracing::info!("所有 14 个子系统初始化完成 ({} 个 TODO 待阶段补全)", 9);

    Ok(StartupContext {
        message_bus,
        storage,
        intercore,
        plugin_loader,
        ai_engine,
    })
}
