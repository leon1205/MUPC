//! # mupc_local_display
//!
//! MUPC 本地显示终端（12-MUPC，RK3588 HDMI 屏运行状态展示）的**渲染端库**。
//!
//! 对齐 `[DESIGN_APPROVED]` 设计文档 §4.3/§5/§6/§9（技术设计）与 UI 设计 §4/§5/§6（视觉权威）：
//!
//! | 模块 | 职责 | 设计出处 |
//! |------|------|----------|
//! | [`channel`] | `DisplayChannelClient`：TCP 回环 `GET /v1/display/latest` 拉最新帧（无 TLS）；**非阻塞状态机**（`tick` 推进 / `Instant` 截止） | §3.1/§5.3/§5.5 |
//! | [`state`] | `DisplayState` + `UiSnapshot`：三态归一（正常 / `--`+角标 / 掉线）+ 新鲜度/通道态派生（纯逻辑）；v2 扩展 `ControlState`（控制通道态 / Toast 生命周期 / `confirm` / `hmi_channel` 本地覆盖） | §3.4/§5.3 + §5.4/§5.5 |
//! | [`console`] | `ConsoleClient`：控制通道 `/v1/console/*` 的非阻塞状态机（`tick` 推进、单次 5 s、幂等重试复用同一 `request_id`） | §5.5 + §3.3/§3.4 |
//! | [`app`] | **装配层**（B3-2a）：LVGL 会话 + 六页外壳 + 通道状态机 → 事件循环宿主（`timing::Host`）+ `--smoke` 自检 | §5.2/§9 |
//! | [`canvas`] | `Canvas` trait（宽 / 高 / `blit_pixels`）+ `fbdev::FbCanvas`（Linux `/dev/fb0` mmap，**升为 flush sink**）；v1.0 的 `OffscreenCanvas` / 绘制原语已在 **B3-2b-1** 删除 | §B1/§8.3 |
//! | [`config`] | 渲染侧 CLI 参数（`--channel/--poll-ms/--backend/--touch-*/--smoke` 等）+ 校验 | §7.2 |
//! | [`screen`] | flush_cb 的像素 sink（`MemorySink` / `FbCanvas`）+ `Blitter` 脏区搬运 | §1.1.1.1 |
//! | [`timing`] | 事件循环骨架：`poll` 唯一阻塞点 + `lv_timer_handler` 驱动 + 停止/统计 | §5.2 |
//! | [`touch`] | evdev 触摸栈（纯逻辑全平台可测；evdev 部分 `cfg(linux)`） | §1.2/§5.3 |
//! | [`lvgl`] | LVGL v9 薄安全层（唯一允许 `unsafe` 的 Rust 侧目录之一） | §1.1.1.2/§5.2 |
//! | [`ui`] | 界面层：主题单一真源 + 组合控件 + 6 页 + 应用外壳 | §5.1/§5.6 |
//! | [`error`] | 渲染端统一错误类型（通道/后端） | — |
//!
//! ## v2.0 已废弃的 v1.0 模块（B3-2a 删除，落点见交付报告）
//!
//! `font.rs`（ab_glyph 光栅化）、`layout.rs`（固定网格自绘）、`run.rs`（500 ms 定拍自绘循环）
//! 及其集成测试 `tests/full_chain.rs` 已随 LVGL 链路**整体删除**：文本/布局/重绘分别由
//! `lvgl::font` + `ui/theme`、`ui/pages`、`timing` 的 `lv_timer_handler` 驱动取代。
//!
//! ## 无屏可测性（设计 §10「离屏渲染测试」）
//!
//! 渲染链路的像素出口是 [`screen::PixelSink`]：`--backend offscreen` 走 [`screen::MemorySink`]
//! （可回读、可导出 PPM），真机走 `fbdev::FbCanvas` ——**两者共用同一条 flush 路径**
//! （只有 sink 不同）。因此布局/三态/降级均可在**无 HDMI、无 framebuffer** 的机器上
//! 确定性断言，真机差异只剩设备打开与像素格式（设计 §13 前置项 1）。
//!
//! ## 进程层（bin）
//!
//! `main.rs` 为可执行入口 `mupc-local-display`（设计 §5.1：本 crate = lib + bin）：CLI →
//! 装配 [`app::App`] → 跑 [`timing::run`] 事件循环 → 优雅退出；panic 由外层兜底捕获后
//! 非零退出，交给 systemd `Restart=always`（§5.3/§9）。不解析
//! `mupc_core_config.yaml`（§7.2 零核心依赖）。

// pub mod 一览（v2.0）：`font` / `layout` / `run` 已删除（见模块文档「已废弃的 v1.0 模块」）。
pub mod app;
pub mod canvas;
pub mod channel;
pub mod config;
// 控制通道客户端（开发单元 B3-1）：`/v1/console/*` 的非阻塞状态机 + 幂等重试（设计 §5.5）。
pub mod console;
// 控制通道的**纯映射层**（开发单元 B3-2b-2）：回执 → 页面路由 + 筛选意图 → 查询串（设计 §3.4）。
// 与 `console.rs`（线上状态机）分开：这一层**零 I/O、零 LVGL**，因而可在纯逻辑用例里逐端点钉住。
pub mod control_route;
pub mod error;
// LVGL 薄安全层（12-MUPC v2.0 工作单元 A1）：唯一允许 `unsafe` 的 Rust 侧位置之一。
// 只有本目录可以引用 `lvgl-sys`（设计 §1.1.1.2 unsafe 边界纪律 1）。
pub mod lvgl;
// 工作单元 C（v2.0）：显示/输入后端 + 事件循环骨架。
// screen = flush_cb 的像素 sink；touch = evdev 触摸栈；timing = 事件循环骨架。
pub mod screen;
pub mod state;
pub mod timing;
pub mod touch;
// 工作单元 B1（v2.0）：界面层（`ui/theme.rs` 外观单一真源 + 组合控件 + 6 页 + 外壳）。
// `ui/**` 仅经 `crate::lvgl` 薄安全层访问 LVGL（设计 §5.1 / §11.4）。
pub mod ui;

// B3-2b-1 收敛：`OffscreenCanvas` / `blend_over` / `hex` / `rgb` 已删（v1.0 自绘链路的残留，
// 无生产消费者）；逐项落点映射见 `canvas.rs` 文件头「死代码清理（开发单元 B3-2b-1）」表。
pub use crate::canvas::{Canvas, Color, Rect};
pub use crate::channel::{poll_due, ChannelEndpoint, DisplayChannelClient, Progress, GET_TIMEOUT};
pub use crate::error::{Error, Result};
pub use crate::screen::{BlitCounters, Blitter, MemorySink, PixelSink};
pub use crate::state::{
    ChannelStatus, DisplayState, Freshness, LiveDot, NumView, ScreenMode, SocView, UiSnapshot,
    CHANNEL_DOWN_MS,
};
/// 生产 `Poller`（Linux `poll(2)`；工作单元 C 评审 C-① 整改）。
#[cfg(target_os = "linux")]
pub use crate::timing::FdPoller;
pub use crate::timing::{
    apply_zero_timeout_clamp, compute_timeout_ms, install_stop_signals, poll_wait_target,
    remaining_ms, stop_requested_flag, timeout_ms_to_c_int, wait_with_retry, Clock, Host,
    LoopConfig, LoopStats, LvglTicker, PollFailure, PollOutcome, PollWait, Poller, RawPollResult,
    SleepPoller, Stop, StopAfter, SystemClock, Ticker, MAX_POLL_TIMEOUT_MS, POLL_FAIL_ABORT_AFTER,
    POLL_FAIL_FALLBACK_AFTER, STOP_FLAG, ZERO_CLAMP_LADDER_MS, ZERO_TIMEOUT_BURST_LIMIT,
};
pub use crate::touch::{
    AbsAxis, CalibBounds, Calibration, Candidate, DeviceCaps, RawEvent, RawState, TouchConfig,
    TouchError, TouchOverrides, TouchSource,
};

/// 默认分辨率（设计 §6.1：8 寸屏 1024x768）。
pub const SCREEN_W: u32 = 1024;
/// 默认分辨率高。
pub const SCREEN_H: u32 = 768;

// B3-2b-1 收敛：`new_offscreen_canvas()` 已删（**v1.0 遗留**，无生产消费者）。
// 离屏像素面的 v2.0 出口 = [`screen::MemorySink::new`]（与真机共用同一条 flush 路径）。
