//! MUPC 微电网特种调控装置通信管理模块 — 主入口
//!
//! ## 启动流程 (6 Phase Gate Model)
//!
//! Phase 0: CLI 解析 + --version / --help 快速退出
//! Phase 1: 配置加载 (mupc_core_config.yaml)
//! Phase 2: tracing 初始化 (JSON subscriber → file + stdout)
//! Phase 3: 子系统初始化 (14 个子系统按依赖顺序)
//! Phase 4: 注册信号处理 (SIGTERM / SIGINT)
//! Phase 5: 主循环 wait-for-shutdown
//! Phase 6: 优雅退出 (LIFO 逆序停止, 30s 超时保护)

mod cli;
mod console_host;
mod core_config;
mod display_host;
mod interlock;
mod signal_handler;
mod startup;

use clap::Parser;
use cli::Cli;
use core_config::CoreConfig;
use mupc_core::service_coord_impl::ServiceCoordinatorImpl;
use std::process;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() {
    // ── 进程启动零点（12-显示终端 F6 `device.uptime_secs` 的唯一真源口径）──
    // 设计 §4.1 明写 uptime「以 **mupcd 进程启动时刻**为准」⇒ 零点必须在**进程入口最顶部**取，
    // 并**传下去**（`initialize_all` → `SystemDeviceSource::new`）。若改在装配点取（`initialize_all`
    // 跑完 DB/intercore/gateway/AI/security 之后才构造），屏上 uptime 会系统性偏小。
    let process_started_at = std::time::Instant::now();

    // ── Phase 0: CLI 解析 ──
    let cli = Cli::parse();

    // --help / --version 由 clap 自动处理后退出，不会到达此处

    if let Err(e) = cli.validate() {
        eprintln!("FATAL: CLI 参数校验失败: {}", e);
        process::exit(1);
    }

    // ── Phase 1: 配置加载 ──
    let config = match CoreConfig::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "FATAL: 配置文件加载失败 ({}): {}",
                cli.config.display(),
                e
            );
            process::exit(1);
        }
    };

    if let Err(e) = config.validate() {
        eprintln!("FATAL: 配置校验失败: {}", e);
        process::exit(1);
    }

    // --validate-config: 仅校验配置文件后退出
    if cli.validate_config {
        println!("配置文件校验通过: {}", cli.config.display());
        process::exit(0);
    }

    // ── Phase 2: tracing 初始化 ──
    // 这两项在 `config` 被移入共享内存副本（见 Phase 3 前）之前取出：
    // `log_level` 供 tracing 初始化，`shutdown_timeout_sec` 供 Phase 6 优雅退出。
    let configured_log_level = config.system.log_level.clone();
    let shutdown_timeout_sec = config.system.shutdown_timeout_sec;
    let log_level = if cli.verbose {
        "debug"
    } else {
        configured_log_level.as_str()
    };

    // 确保日志目录存在
    if let Err(e) = std::fs::create_dir_all(&cli.log_dir) {
        eprintln!("FATAL: 无法创建日志目录 {}: {}", cli.log_dir.display(), e);
        process::exit(1);
    }

    let file_appender = tracing_appender::rolling::daily(&cli.log_dir, "mupc.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    if let Err(e) = tracing_subscriber::registry()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(non_blocking)
                .with_target(true),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stdout)
                .with_target(false),
        )
        .try_init()
    {
        eprintln!("FATAL: tracing 初始化失败: {}", e);
        process::exit(1);
    }

    tracing::info!("mupcd v{} 启动中...", env!("CARGO_PKG_VERSION"));
    tracing::info!("配置文件: {}", cli.config.display());
    tracing::info!("日志级别: {}", log_level);

    // ── Phase 3: 子系统初始化 ──
    let mut coord = ServiceCoordinatorImpl::new();

    // 12-显示终端 §4.3.2：`CoreConfig` **内存副本**（`Arc<RwLock<…>>`）是写入生效后的
    // 「进程内唯一权威读源」。**在这里**（Phase 2 tracing 之后、装配之前）就地建立并**移入**
    // 该副本（`initialize_all` 只借它，不再持有第二份 `CoreConfig` 的所有权）：
    // 若在装配点再 clone 一份，就出现"装配用 A、控制通道读 B"的双真源 —— 将来 G-2 写入只更新 B，
    // 而装配期的 A 已过期，属静默漂移。上面两项（`log_level` / `shutdown_timeout_sec`）已先取出。
    let core_config = std::sync::Arc::new(tokio::sync::RwLock::new(config));

    let ctx = match startup::initialize_all(&core_config, &coord, process_started_at).await {
        Ok(ctx) => ctx,
        Err(e) => {
            tracing::error!(error = %e, "子系统初始化失败，开始级联清理...");
            // 级联清理：停止已注册的子系统
            coord.stop_all().await;
            process::exit(1);
        }
    };

    tracing::info!("所有子系统就绪，进入主循环");

    // ── Phase 4: 注册信号处理 ──
    // 信号处理已内建于 wait_for_shutdown (Phase 5)

    // ── Phase 5: 主循环 ──
    tracing::info!(
        "mupcd 运行中 (PID: {})，等待信号...",
        std::process::id()
    );

    signal_handler::wait_for_shutdown().await;

    // ── Phase 6: 优雅退出 ──
    tracing::info!("Phase 6: 开始优雅退出 (超时 {} 秒)...", shutdown_timeout_sec);

    let shutdown_result = tokio::time::timeout(
        std::time::Duration::from_secs(shutdown_timeout_sec),
        graceful_shutdown(&coord, &ctx),
    )
    .await;

    match shutdown_result {
        Ok(()) => {
            tracing::info!("优雅退出完成");
            process::exit(0);
        }
        Err(_elapsed) => {
            tracing::error!("优雅退出超时 ({} 秒)，强制退出", shutdown_timeout_sec);
            process::exit(1);
        }
    }
}

/// 优雅退出流程：LIFO 逆序停止各子系统
async fn graceful_shutdown(
    coord: &ServiceCoordinatorImpl,
    ctx: &startup::StartupContext,
) {
    tracing::info!("停止所有子系统 (逆序)...");
    coord.stop_all().await;
    // abort 后台任务（此前 _ctx 被忽略，background_tasks 永不 abort）
    ctx.shutdown().await;
    tracing::info!("所有子系统已停止");
}
