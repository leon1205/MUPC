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
mod core_config;
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
    let log_level = if cli.verbose {
        "debug"
    } else {
        &config.system.log_level
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

    let ctx = match startup::initialize_all(&config, &coord).await {
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
    tracing::info!("Phase 6: 开始优雅退出 (超时 {} 秒)...", config.system.shutdown_timeout_sec);

    let shutdown_result = tokio::time::timeout(
        std::time::Duration::from_secs(config.system.shutdown_timeout_sec),
        graceful_shutdown(&coord, &ctx),
    )
    .await;

    match shutdown_result {
        Ok(()) => {
            tracing::info!("优雅退出完成");
            process::exit(0);
        }
        Err(_elapsed) => {
            tracing::error!(
                "优雅退出超时 ({} 秒)，强制退出",
                config.system.shutdown_timeout_sec
            );
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
