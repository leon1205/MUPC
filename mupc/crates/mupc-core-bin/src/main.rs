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

mod alert_feed;
mod bounded_io;
mod cli;
mod config_service;
mod console_audit;
mod console_host;
mod core_config;
mod display_host;
mod hot_apply;
mod idempotency;
mod interlock;
mod interlock_ops;
mod log_service;
mod signal_handler;
mod startup;
#[cfg(test)]
mod testutil;
mod yaml_edit;

use clap::Parser;
use cli::Cli;
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

    // ── Phase 1: 配置加载（含 `.bak` 兜底恢复；评审建议 6.4）──
    // `atomic_write` 的「rename(真源 → .bak) → rename(.tmp → 真源)」之间存在"真源不存在"的
    // 窗口；此刻掉电/被 kill ⇒ 下次启动无真源可读。`load_config_with_backup_recovery` 在
    // 真源读不出来而 `<config>.bak` 在时用它恢复（见 config_service.rs 的窗口登记）。
    // 恢复是**重要事件**，必须响亮：本处 tracing 尚未初始化（Phase 2 才建），故用 `eprintln!`
    // （systemd/journald 会收进日志）。
    let (mut config, recovered_from) = match config_service::load_config_with_backup_recovery(&cli.config)
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "FATAL: 配置文件加载失败 ({}): {}",
                cli.config.display(),
                e
            );
            process::exit(1);
        }
    };
    if let Some(bak) = &recovered_from {
        eprintln!(
            "WARN: 真源 {} 不可读，已用备份 {} 恢复（上次落盘的崩溃窗口；请核对配置内容）",
            cli.config.display(),
            bak.display()
        );
    }

    // ── 日志目录：**收敛成单一真源**（R2 整改）──
    // 写者（下面的 `rolling::daily`）与读者（`startup` 装配的 `LogService`）必须是**同一个值**。
    // 改法是"`--log-dir` 为可选覆盖（[`Cli::effective_log_dir`]）+ 就地**写回**
    // `config.system.log_dir`"：写回之后全进程只有这一个值 —— appender 写它、LogService 读它、
    // 审计目录（`{log_dir}/audit`）在它下面。
    // 整改前：appender 吃 `cli.log_dir`（默认 `/opt/mupc/logs`）、LogService 吃
    // `config.system.log_dir` ⇒ **两个值可以不一致且无人知晓**；最坏不是 503 而是**静默失实**
    // （`create_dir_all` 把读者那侧目录建出来 ⇒ 目录存在但为空 ⇒ 200 + `entries=[]` ⇒
    // 屏上「当前筛选条件下无日志」，而日志其实写在别处）。
    // 此刻 tracing 尚未初始化 ⇒ 用 `eprintln!`（同上面几条启动期告警，systemd/journald 会收）。
    if let Some(dir) = &cli.log_dir {
        if *dir != config.system.log_dir {
            eprintln!(
                "WARN: --log-dir {} 覆盖配置的 system.log_dir {}（本次生效值以 --log-dir 为准，\
                 appender / 日志服务 / 审计目录都用它）",
                dir.display(),
                config.system.log_dir.display()
            );
        }
    }
    config.system.log_dir = cli.effective_log_dir(&config.system.log_dir);

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

    // 确保日志目录存在（`config.system.log_dir` = 上面收敛出的**单一真源**）
    if let Err(e) = std::fs::create_dir_all(&config.system.log_dir) {
        eprintln!(
            "FATAL: 无法创建日志目录 {}: {}",
            config.system.log_dir.display(),
            e
        );
        process::exit(1);
    }

    let file_appender = tracing_appender::rolling::daily(&config.system.log_dir, "mupc.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    // 12-显示终端 §4.3.3「日志级别 | `tracing_subscriber::reload` handle」：filter 走
    // **reload 层**，句柄留在这里并传下去 ⇒ 屏上改 `system.log_level` 时由
    // `hot_apply.rs` 调 `handle.reload(..)` **当场换掉过滤规则**（≤1 s，无需重启）。
    // 不用 reload 层的话，`EnvFilter` 一旦 `.init()` 就再也改不动——屏上改日志级别会变成
    // "写进文件了、装置还是按老级别打"，即静默失实。
    let (filter_layer, log_reload) =
        tracing_subscriber::reload::Layer::new(env_filter);

    if let Err(e) = tracing_subscriber::registry()
        .with(filter_layer)
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
    // S-5：`coord` 只被 `&` 借用（`initialize_all(&coord, …)` / `graceful_shutdown(&coord, …)`），
    // `stop_all()` 的接收者是 `&self` ⇒ 不需要 `mut`（`-D warnings` 的 CI 会红）。
    let coord = ServiceCoordinatorImpl::new();

    // 12-显示终端 §4.3.2：`CoreConfig` **内存副本**（`Arc<RwLock<…>>`）是写入生效后的
    // 「进程内唯一权威读源」。**在这里**（Phase 2 tracing 之后、装配之前）就地建立并**移入**
    // 该副本（`initialize_all` 只借它，不再持有第二份 `CoreConfig` 的所有权）：
    // 若在装配点再 clone 一份，就出现"装配用 A、控制通道读 B"的双真源 —— 将来 G-2 写入只更新 B，
    // 而装配期的 A 已过期，属静默漂移。上面两项（`log_level` / `shutdown_timeout_sec`）已先取出。
    let core_config = std::sync::Arc::new(tokio::sync::RwLock::new(config));

    let ctx = match startup::initialize_all(
        &core_config,
        &coord,
        process_started_at,
        &cli.config,
        Some(log_reload),
    )
    .await
    {
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
    // abort 后台任务（此前 _ctx 被忽略，background_tasks 永不 abort）。
    // P0-1：`ctx.shutdown()` 内部顺序 = **先 abort（含遥测定时 flush 任务、南向采集生产者）→
    // 再 flush 遥测缓冲**，把不足一批的剩余数据落盘（修复前退出不 flush ⇒ 外设数据滞留内存、
    // 断电即丢）。顺序的理由见 `StartupContext::shutdown` 的文档注释。
    ctx.shutdown().await;
    tracing::info!("所有子系统已停止");
}
