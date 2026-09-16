//! `mupc-local-display` —— MUPC 本地显示终端**渲染进程**（bin 入口，设计 §5.1/§5.2/§9）。
//!
//! 职责（设计 §5.2 的 ①–③ 段 + §9 的退出语义）：
//! 1. 解析 CLI（[`mupc_local_display::config::CliConfig`]，默认值取 display-proto 常量；
//!    **不读** `mupc_core_config.yaml`，零核心配置依赖）；
//! 2. 按 `--backend` 装配 LVGL 会话：`offscreen`（内存 sink，本机/CI 全链路）/ `fbdev`
//!    （Linux `/dev/fb0`，真机）；`drm` 未实现 → **明确报错**（不静默回退）；
//! 3. 跑**唯一阻塞点为 `poll`** 的事件循环（[`mupc_local_display::timing::run`]；
//!    LVGL 由 `lv_timer_handler` 驱动、通道为非阻塞状态机 —— §5.2 五条不变量）；
//! 4. `--smoke`：跑有限拍 → 逐页渲染 6 页 → 打印时序与逐页像素统计 →（可选）导出 PPM → 退出。
//!
//! 异常与自恢复（设计 §5.3/§9，PRD 4.3.1/6.7）：
//! - 通道失败**不是**致命错误（属正常展示态：≤3 s 切「与主进程数据通道断开」，恢复即回实时）；
//! - 启动期致命错误（LVGL 装配 / 后端打开 / 触摸致命错误）→ 打印明确原因 + 非零退出，
//!   交给 systemd `Restart=always` ≤3 s 拉起；
//! - 兜底 `catch_unwind`：任何 panic 打印后以非零码退出（**不吞 panic、不自循环空转**），
//!   进程隔离保证不影响 mupcd（渲染进程只经回环 GET 单向读，无任何下行写）。
//!
//! ⚠️ **本文件是 bin**（`[[bin]]`，不是 lib）：可测逻辑一律落在 `src/app.rs` /
//! `src/timing.rs` / `src/config.rs`（集成测试够得着），此处只留"装配 + 报告 + 退出码"。
//!
//! ⚠️ **`--font` 在 v2.0 已不生效**（字体改由构建期绑定的 LVGL 位图字库承担，见
//! `lvgl/font.rs` 与 `fonts/gen_fonts.sh`）：**保留参数但启动时响亮告警**（不静默忽略）。
//!
//! **裁定留痕（B3-2a 规格评审 建议 4，主控裁定）**：保留参数、不改成硬错误 —— 本进程由
//! systemd `Restart=always` 托管，硬错误会造成**重启循环**（服务起不来且无人值守）；
//! 而"参数被忽略"必须**可见** ⇒ 告警 + `--help` 如实标注（文案见
//! [`mupc_local_display::config::font_ignored_warning`]）。
//!
//! ⚠️ **`--control-channel` 已接线**（B3-2b-2）：`ConsoleClient` 的生产实例化点在
//! [`mupc_local_display::app::App`] 的装配里，`GET` 查询 + 受控 `POST` 写均经它发起。
//! 启动时**无条件回显实际取值**（与 `--channel` 同取向；文案见
//! [`mupc_local_display::config::control_channel_notice`]）—— 原先那条「本参数当前**不影响
//! 行为**、待 B3-2b 接线」的告警（`control_channel_pending_warning`）此刻失真，
//! **已按设计连同其调用点与 help 标注整体删除**。

use std::process::ExitCode;
use std::time::Instant;

use mupc_local_display::app::{self, App, StartupError};
use mupc_local_display::channel::DisplayChannelClient;
use mupc_local_display::config::{self, Backend, CliConfig, ConfigError};
use mupc_local_display::timing::{self, Clock, LoopConfig, LoopStats, Stop, StopAfter};
#[cfg(target_os = "linux")]
use mupc_local_display::timing::FdPoller;
#[cfg(not(target_os = "linux"))]
use mupc_local_display::timing::SleepPoller;

/// 参数错误退出码（与「运行期失败」区分，便于部署脚本诊断）。
const EXIT_USAGE: u8 = 2;
/// 运行期失败退出码。
const EXIT_RUNTIME: u8 = 1;
/// panic 兜底退出码（systemd 见非零即重启）。
const EXIT_PANIC: u8 = 70;
/// `--smoke` 自检失败退出码（六页中存在空页）：与"运行期失败"分开，CI 可据此判读。
const EXIT_SMOKE: u8 = 3;

/// `--smoke` 的事件循环拍数（20 拍 × ≤25 ms 上限 ≈ 0.5 s；足够走完至少一次 GET 往返）。
const SMOKE_TICKS: u64 = 20;
/// `--smoke` 的 `poll` 超时上界（缩短节拍以让自检总时长可控；生产恒用 `--poll-ms`）。
const SMOKE_POLL_CAP_MS: u64 = 25;

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

/// 事件循环时钟：与 [`App`] 的时基原点**同源**的单调毫秒（时钟跳变不影响节拍/超时）。
struct LoopClock(Instant);

impl Clock for LoopClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
}

/// 停止条件 = 「收到 SIGINT/SIGTERM」**或**「`--smoke` 已跑满 N 拍」。
struct StopWhen {
    /// `--smoke` 的拍数上限（生产 ⇒ `None`，常驻直到信号）。
    after: Option<StopAfter>,
}

impl Stop for StopWhen {
    fn stop_requested(&self) -> bool {
        // 短路顺序有意为之（先看信号）：收到信号后不再推进 `StopAfter` 的计数。
        timing::STOP_FLAG.load(std::sync::atomic::Ordering::Relaxed)
            || self.after.as_ref().is_some_and(|a| a.stop_requested())
    }
}

/// 装配 + 跑主循环（panic 由 `main` 兜底）。
fn run_process(cfg: CliConfig) -> ExitCode {
    // `--font` 在 v2.0 已不生效（见文件头）：响亮告警而非静默忽略。
    if let Some(p) = cfg.font.as_deref() {
        eprintln!("[mupc-local-display] {}", config::font_ignored_warning(p));
    }
    // `--control-channel` **已接线**（B3-2b-2）：无条件回显实际取值（与 `--channel` 同取向）。
    // 原先那条"当前不影响行为、待 B3-2b 接线"的告警已完成使命、**按设计删除** —— 它此刻失真。
    eprintln!(
        "[mupc-local-display] {}",
        config::control_channel_notice(&cfg.control_channel)
    );
    // `--rotate` 非 0：**已在解析期硬错误**（B4a 整改「重要 2」）⇒ 本处**没有任何告警可打**
    // —— 能走到这里的 `cfg.rotate` 恒为 `Deg0`。原先那条 `rotate_pixel_gap_warning`
    // 启动告警随之**整体删除**（硬错误后它不可达 = 死代码），其语义由两处承担：
    // ① 解析期错误文案（点名"像素级旋转（sink 侧）尚未实现"）；② `--help` 的 `--rotate` 行。
    // 数据通道 URL 前置校验（解析失败 = 用法错误 ⇒ 退出码 2；不静默回退默认端点）。
    if let Err(e) = DisplayChannelClient::try_new(&cfg.channel) {
        eprintln!("[mupc-local-display] 数据通道 URL 非法：{e}");
        return ExitCode::from(EXIT_USAGE);
    }

    match cfg.backend {
        Backend::Offscreen => run_with(&cfg, App::new_offscreen),
        Backend::Fbdev => run_fbdev(&cfg),
        // drm 后端未实现（设计 §13 前置项 1）：明确报错，不静默回退 offscreen。
        Backend::Drm => {
            eprintln!("[mupc-local-display] {}", app::drm_unimplemented_error());
            ExitCode::from(EXIT_RUNTIME)
        }
    }
}

/// Linux framebuffer 真机路径（`--backend fbdev`）。
#[cfg(target_os = "linux")]
fn run_fbdev(cfg: &CliConfig) -> ExitCode {
    run_with(cfg, App::new_fbdev)
}

/// 非 Linux 平台上的 `fbdev` 分支：CLI 已拒绝（`Backend::available_on_this_platform`），
/// 此处仅为编译完整性（防御不可达路径，**不静默回退 offscreen**）。
#[cfg(not(target_os = "linux"))]
fn run_fbdev(_cfg: &CliConfig) -> ExitCode {
    eprintln!("[mupc-local-display] framebuffer 后端仅 Linux 支持（本机请用 --backend offscreen）");
    ExitCode::from(EXIT_RUNTIME)
}

/// 后端无关的公共流程：装配 → 选 poller → 跑事件循环 → （可选）自检 → 退出报告。
fn run_with<F>(cfg: &CliConfig, make: F) -> ExitCode
where
    F: FnOnce(&CliConfig, Instant) -> Result<App, StartupError>,
{
    // 信号 → 停止标志（Linux；本机 Windows 无 handler，走默认终止，见 timing::STOP_FLAG）。
    timing::install_stop_signals();
    let origin = Instant::now();
    let mut app = match make(cfg, origin) {
        Ok(a) => a,
        Err(e) => {
            // 启动期致命错误：明确原因 + 非零退出（交给 systemd Restart=always）。
            eprintln!("[mupc-local-display] 启动失败：{e}");
            return ExitCode::from(EXIT_RUNTIME);
        }
    };

    let clock = LoopClock(origin);
    let mut ticker = timing::LvglTicker;
    // ── poller：唯一阻塞点，且**必须真阻塞**（§5.2 不变量 1）──
    // Linux：`poll(2)` 包住触摸 evdev fd；无触摸设备时 `poll(NULL,0,t)`（仍是唯一阻塞点）。
    #[cfg(target_os = "linux")]
    let mut poller = match app.touch_fd() {
        Some(fd) => FdPoller::new(fd),
        None => FdPoller::without_fd(),
    };
    // 非 Linux（开发机/CI）：本平台无 poll(2)/evdev，用等价语义的纯超时阻塞（仍非忙等）。
    #[cfg(not(target_os = "linux"))]
    let mut poller = SleepPoller::new();

    let loop_cfg = LoopConfig::new(if cfg.smoke {
        SMOKE_POLL_CAP_MS
    } else {
        cfg.interval_ms
    });
    let stop = StopWhen {
        after: cfg.smoke.then(|| StopAfter::new(SMOKE_TICKS)),
    };

    let outcome = timing::run(&clock, &mut poller, &mut ticker, &mut app, &stop, &loop_cfg);

    let mut code = ExitCode::SUCCESS;
    match &outcome {
        Ok(stats) => report_exit(cfg, &app, stats),
        Err(pf) => {
            // poll 已不可用（连续失败超上限）：**有界终止**并把 errno 交给 systemd 重启。
            eprintln!("[mupc-local-display] {pf}");
            code = ExitCode::from(EXIT_RUNTIME);
        }
    }

    if cfg.smoke && outcome.is_ok() {
        code = run_smoke(&mut app, cfg, &outcome.unwrap_or_default());
    }
    code
}

/// `--smoke` 一键自检（设计 §9）：逐页渲染 6 页 → 打印统计 →（可选）导出 PPM。
///
/// 六页**任一为空** ⇒ 返回码 [`EXIT_SMOKE`]（自检必须能失败，否则它只是"打印"）。
fn run_smoke(app: &mut App, cfg: &CliConfig, stats: &LoopStats) -> ExitCode {
    let report = match app.smoke(cfg.smoke_out.as_deref()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[smoke] 自检失败：{e}");
            return ExitCode::from(EXIT_SMOKE);
        }
    };
    // 自检结论走 **stdout**（人读 + CI 解析双用；诊断/时序走 stderr 由 `report_exit` 打）。
    // `renders` = 帧驱动页实测渲染次数（I-2 的调用点判据：恒真退化 ⇒ renders == ticks）；
    // `blits`/`dropped` = flush 搬运 / 丢帧计数（I-4：丢帧此前在接线后读不到）。
    println!(
        "[smoke] ticks={} renders={} poll_calls={} frames_ok={} frames_fail={} blits={} dropped={} walk_ms={}",
        report.ticks,
        report.renders,
        stats.poll_calls,
        report.frames_ok,
        report.frames_fail,
        report.blits,
        report.dropped,
        report.walk_ms
    );
    for (page, px) in &report.pages {
        // 页名用 P1..P6（ASCII，CI 可直接 grep；中文页名见 NavPage::title）。
        println!("[smoke] page=P{} active_px={px}", page.index() + 1);
    }
    // **LVGL 定容池余量**（B4a；设计 §10 / §14 R-24 的量化口）。
    // ⚠️ 读的是 `LV_MEM_SIZE`（1 MiB）那一块**定容池** —— 装的是对象树 / 样式属性表 /
    // 定时器 / 事件项；**不含绘制缓冲**（后者走系统堆，见 `lvgl::MemStats`）。
    // 真机（fbdev）同样可读 ⇒ 与离屏自检判据一致。
    println!(
        "[smoke] mem_total={} mem_free={} mem_free_cnt={} mem_free_biggest={} mem_used_cnt={} \
         mem_max_used={} mem_used_pct={} mem_frag_pct={} mem_headroom={}",
        report.mem.total_size,
        report.mem.free_size,
        report.mem.free_cnt,
        report.mem.free_biggest_size,
        report.mem.used_cnt,
        report.mem.max_used,
        report.mem.used_pct,
        report.mem.frag_pct,
        if report.mem.has_headroom() { "ok" } else { "tight" },
    );
    match &report.export {
        Some((path, bytes)) => {
            println!("[smoke] export={} bytes={bytes}", path.display());
        }
        None => println!("[smoke] export=none（未指定 --smoke-out）"),
    }
    // 五条判定口**互相独立**，任一不成立即 FAIL（结论行点名是哪一条，便于 CI 判读）。
    // `FAIL_NO_FLUSH` 是 `dropped == 0` 的对偶哨：抓"计数根本没接线"（否则恒 0 看着完美）。
    // `FAIL_MEM_TIGHT`（B4a 新增）把设计 §10 / §14 **R-24**「1 MB 定容池余量未量化」变成
    // 可判：峰值打满 95 % 即失败（**未读到**池 —— `total_size == 0` —— 同样失败，
    // 否则"什么都没读到"会被当成"余量充足"）。
    let verdict = if !report.all_pages_non_empty() {
        "FAIL_EMPTY_PAGE"
    } else if !report.throttle_effective() {
        "FAIL_NO_THROTTLE"
    } else if !report.flush_observed() {
        "FAIL_NO_FLUSH"
    } else if !report.no_dropped_frames() {
        "FAIL_DROPPED_FRAME"
    } else if !report.mem.has_headroom() {
        "FAIL_MEM_TIGHT"
    } else {
        "OK"
    };
    println!("[smoke] result={verdict}");
    if verdict == "OK" {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_SMOKE)
    }
}

/// 退出报告（统计行落 journal，便于与设计 §9 预算对照）。
///
/// **通道诊断三个量**（I-5 裁定：接进本行，而不是删掉 `DisplayChannelClient` 的三个读口）：
/// `channel_addr` = **解析后**的目标地址（与发起连接用的是同一个 `SocketAddr`，不是 CLI 里
/// 那个未解析的 URL）、`channel_timeout_ms` = 单次 GET 超时、`fail_streak` = 退出时的
/// **连续**失败数。真机排障（"到底连的谁 / 超时多长 / 是不是一直在失败"）就靠这三个。
///
/// `zero_clamps` 的非零在**在途请求**期间是正常的（见 `App::next_deadline_ms` 的登记）。
///
/// **控制通道六个量**（B3-2b-2）：`console_addr`（解析后的目标地址）、
/// `console_fail_streak`（P3 通道条态的**真源**）、`p3_channel`（**实际注入 P3 的值**，
/// `up`/`down` —— 进程级用例据此断言"P3 由控制通道而非帧通道驱动"）、
/// `write_dropped` / `read_dropped`（在途期间被丢弃的意图数）、
/// `route_errors`（回执形态/解码不符数）、`p4_refresh`（EDGE-19 补发 GET 的生效次数）。
fn report_exit(cfg: &CliConfig, app: &App, s: &LoopStats) {
    let (ok, fail) = app.frame_counts();
    let ch = app.channel();
    let (blits, dropped) = app.flush_stats();
    let ctl = app.console();
    eprintln!(
        "[mupc-local-display] 退出：backend={} channel={} channel_addr={} channel_timeout_ms={} \
         fail_streak={} ticks={} ok={} fail={} renders={} blits={} dropped={} touch={} \
         iterations={} poll_calls={} ready={} timeouts={} last_timeout={}ms lv_next={}ms \
         zero_iters={} zero_clamps={} poll_failures={} \
         console={} console_addr={} console_fail_streak={} p3_channel={} write_dropped={} \
         read_dropped={} route_errors={} p4_refresh={}",
        cfg.backend.as_str(),
        cfg.channel,
        ch.addr(),
        ch.timeout().as_millis(),
        ch.fail_streak(),
        app.ticks(),
        ok,
        fail,
        app.renders(),
        blits,
        dropped,
        app.touch_errors(),
        s.iterations,
        s.poll_calls,
        s.ready_events,
        s.iterations.saturating_sub(s.ready_events),
        s.last_timeout_ms,
        s.lv_next_ms,
        s.zero_timeout_iters,
        s.zero_timeout_clamps,
        s.poll_failures,
        cfg.control_channel,
        ctl.addr(),
        app.console_fail_streak(),
        if app.p3_channel_connected() { "up" } else { "down" },
        app.write_intents_dropped(),
        app.read_intents_dropped(),
        app.route_errors(),
        app.p4_refresh_forced(),
    );
}
