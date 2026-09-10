//! `mupc-local-display` —— MUPC 本地显示终端**渲染进程**（bin 入口，设计 §5.2 `main.rs`）。
//!
//! 职责（设计 §5.3/§7.2/§9）：
//! 1. 解析 CLI（[`mupc_local_display::config::CliConfig`]，默认值取 display-proto 常量；
//!    **不读** `mupc_core_config.yaml`）；
//! 2. 加载字库（[`TextKit`]：外部 `--font` → 捆绑子集 feature → 内置 ASCII 回退，永不 panic）；
//! 3. 按 `--backend` 选画布：`offscreen`（离屏内存，本机/CI 全链路）/ `fbdev`（Linux `/dev/fb0`
//!    mmap，真机）；`drm` 未实现 → 明确报错（不静默回退）；
//! 4. 起 [`run::run_loop`] 主循环；Ctrl-C/SIGTERM 置停止标志优雅退出。
//!
//! 异常与自恢复（设计 §5.3/§9，PRD 4.3.1/6.7）：
//! - 通道失败**不是**致命错误（属正常展示态：≤3s 切「与主进程数据通道断开」，恢复即回实时）；
//! - 启动期致命错误（后端打开失败等）→ 打印明确原因 + 非零退出，交给 systemd `Restart=always`
//!   ≤3s 拉起；
//! - 兜底 `catch_unwind`：任何 panic 打印后以非零码退出（**不吞 panic、不自循环空转**），
//!   进程隔离保证不影响 mupcd（渲染进程只经回环 GET 单向读，无任何下行写）。

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mupc_local_display::canvas::OffscreenCanvas;
use mupc_local_display::channel::DisplayChannelClient;
use mupc_local_display::config::{self, Backend, CliConfig, ConfigError};
use mupc_local_display::font::TextKit;
use mupc_local_display::run::{self, Renderer, RunStats};

/// 参数错误退出码（与「运行期失败」区分，便于部署脚本诊断）。
const EXIT_USAGE: u8 = 2;
/// 运行期失败退出码。
const EXIT_RUNTIME: u8 = 1;
/// panic 兜底退出码（systemd 见非零即重启）。
const EXIT_PANIC: u8 = 70;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = match CliConfig::parse(&args) {
        Ok(c) => c,
        Err(ConfigError::Help) => {
            print!("{}", config::help_text());
            return ExitCode::SUCCESS;
        }
        Err(ConfigError::Version) => {
            println!("mupc-local-display {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            // 明确报错（不静默取默认值）：非法参数绝不带着默认值继续跑。
            eprintln!("[mupc-local-display] 参数错误：{e}");
            eprintln!("用法：mupc-local-display [OPTIONS]（--help 查看全部选项）");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    // 外层兜底：panic 打印后非零退出，交给 systemd Restart=always（设计 §5.3）。
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_process(cfg))) {
        Ok(code) => code,
        Err(_) => {
            eprintln!(
                "[mupc-local-display] 渲染进程异常退出（panic 已捕获）；\
                 本进程不影响 mupcd，等待 systemd Restart=always（RestartSec≤3s）拉起"
            );
            ExitCode::from(EXIT_PANIC)
        }
    }
}

/// 装配 + 跑主循环（在运行时内执行；panic 由 `main` 兜底）。
fn run_process(cfg: CliConfig) -> ExitCode {
    let tk = TextKit::load(cfg.font.as_deref());
    if !tk.is_real() {
        eprintln!(
            "[mupc-local-display] 未加载到真实字库：中文将显示占位盒、数字/拉丁走内置 ASCII 回退。\
             部署请用 --font <字库> 或按 font.rs 顶部命令子集化后以 bundled-font feature 编译。"
        );
    }

    // URL 已在解析期校验；此处仍显式处理，避免任何静默回退到默认端点。
    let client = match DisplayChannelClient::try_new(&cfg.channel) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[mupc-local-display] 数据通道 URL 非法：{e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[mupc-local-display] 异步运行时初始化失败：{e}");
            return ExitCode::from(EXIT_RUNTIME);
        }
    };

    let stop = Arc::new(AtomicBool::new(false));
    rt.block_on(async move {
        spawn_signal_handlers(Arc::clone(&stop));
        match cfg.backend {
            Backend::Offscreen => {
                let canvas = OffscreenCanvas::new(cfg.width, cfg.height);
                let mut r = Renderer::new(tk, canvas, cfg.stale_ms);
                let stats = run::run_loop(&cfg, &client, &mut r, &stop, None).await;
                report_exit(&cfg, stats);
                ExitCode::SUCCESS
            }
            Backend::Fbdev => run_fbdev(&cfg, tk, &client, &stop).await,
            // drm 后端未实现（设计 §13 前置项 1）：明确报错，不静默回退 offscreen。
            Backend::Drm => {
                eprintln!("[mupc-local-display] {}", run::drm_unimplemented_error());
                ExitCode::from(EXIT_RUNTIME)
            }
        }
    })
}

/// Linux framebuffer 真机路径（`--backend fbdev`；Windows 本机不编译该分支，设计 §B1）。
#[cfg(target_os = "linux")]
async fn run_fbdev(
    cfg: &CliConfig,
    tk: TextKit,
    client: &DisplayChannelClient,
    stop: &AtomicBool,
) -> ExitCode {
    use mupc_local_display::canvas::fbdev::FbCanvas;
    // 打开失败 → 明确报错 + 非零退出（systemd 重启；不静默黑屏运行）。
    let fb = match FbCanvas::open(&cfg.fbdev_path, cfg.width, cfg.height) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "[mupc-local-display] 打开/映射 framebuffer 失败：{e}\n\
                 提示：需 root 或 video 组成员权限；可先用 --backend offscreen 验证全链路；\
                 像素格式/分辨率不符属设计 §13 前置项 1（真机首验）。"
            );
            return ExitCode::from(EXIT_RUNTIME);
        }
    };
    eprintln!(
        "[mupc-local-display] framebuffer {} 已映射 {}x{}（32bpp XRGB 假定，真机首验项见设计 §13）",
        cfg.fbdev_path, cfg.width, cfg.height
    );
    let mut r = Renderer::new(tk, fb, cfg.stale_ms);
    let stats = run::run_loop(cfg, client, &mut r, stop, None).await;
    report_exit(cfg, stats);
    ExitCode::SUCCESS
}

/// 非 Linux 平台上的 `fbdev` 分支：CLI 已拒绝，此处仅为编译完整性（防御不可达路径）。
#[cfg(not(target_os = "linux"))]
async fn run_fbdev(
    _cfg: &CliConfig,
    _tk: TextKit,
    _client: &DisplayChannelClient,
    _stop: &AtomicBool,
) -> ExitCode {
    eprintln!("[mupc-local-display] framebuffer 后端仅 Linux 支持（本机请用 --backend offscreen）");
    ExitCode::from(EXIT_RUNTIME)
}

/// SIGINT(Ctrl-C) / SIGTERM(systemd stop) → 置停止标志（优雅退出：在 ≤1 个轮询周期内收尾并打印统计）。
/// ⚠️ SIGTERM 分支为 unix-only（`tokio::signal::unix`），本机 Windows 不编译——需 Linux 验证。
fn spawn_signal_handlers(stop: Arc<AtomicBool>) {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            match signal(SignalKind::terminate()) {
                Ok(mut term) => {
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {}
                        _ = term.recv() => {}
                    }
                }
                Err(e) => {
                    // 拿不到 SIGTERM 处理器不致命：仍监听 Ctrl-C（systemd 停服务时由默认处理终止进程）。
                    eprintln!("[mupc-local-display] SIGTERM 处理器注册失败({e})，仅监听 Ctrl-C");
                    let _ = tokio::signal::ctrl_c().await;
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        eprintln!("[mupc-local-display] 收到中断/终止信号，退出主循环");
        stop.store(true, Ordering::Relaxed);
    });
}

/// 退出报告（统计行落 journal，便于与设计 §9 预算对照）。
fn report_exit(cfg: &CliConfig, s: RunStats) {
    eprintln!(
        "[mupc-local-display] 退出：backend={} channel={} ticks={} ok={} fail={} redraws={} \
         last_draw={}ms max_draw={}ms",
        cfg.backend.as_str(),
        cfg.channel,
        s.ticks,
        s.ok,
        s.fail,
        s.redraws,
        s.last_draw_ms,
        s.max_draw_ms
    );
}
