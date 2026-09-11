//! # mupc_local_display
//!
//! MUPC 本地显示终端（12-MUPC，RK3588 HDMI 屏运行状态展示）的**渲染端库**。
//!
//! 对齐 `[DESIGN_APPROVED]` 设计文档 §4.3/§5/§6/§9（技术设计）与 UI 设计 §4/§5/§6（视觉权威）：
//!
//! | 模块 | 职责 | 设计出处 |
//! |------|------|----------|
//! | [`channel`] | `DisplayChannelClient`：TCP 回环 `GET /v1/display/latest` 拉最新帧（无 TLS） | §3.1/§5.3 |
//! | [`state`] | `DisplayState` + `UiSnapshot`：三态归一（正常 / `--`+角标 / 掉线）+ 新鲜度/通道态派生（纯逻辑） | §3.4/§5.3 |
//! | [`canvas`] | `Canvas` trait + `OffscreenCanvas`（默认，内存缓冲可离屏断言）+ `fbdev::FbCanvas`（Linux `/dev/fb0` mmap，`cfg(unix)` 隔离） | §B1/§5.2/§6 |
//! | [`font`] | ab_glyph 光栅化 + 可插拔字库（外部路径 / `bundled-font` feature）+ 缺字库容错回退 | §5.4/§9/§13 前置项 8 |
//! | [`layout`] | 1024x768 固定网格：页眉 / SOC 主区 / PCS 状态区 / 三相四卡（A/B/C/总，上P下I） | §6.2/§6.3 + UI §4/§5 |
//! | [`config`] | 渲染侧 CLI 参数（`--channel/--interval/--stale-ms/--backend/--fbdev-path/--width/--height/--font`）+ 校验 | §7.2 |
//! | [`run`] | 主循环编排：`Renderer`（注入时钟，可测）+ `run_loop`（500ms 定拍、1Hz 整流、无忙等） | §5.3/§9 |
//! | [`error`] | 渲染端统一错误类型（通道/后端/字体） | — |
//!
//! ## 无屏可测性（设计 §10「离屏渲染测试」）
//!
//! 全部渲染路径面向 [`canvas::Canvas`] trait，`OffscreenCanvas` 与真屏共用同一套绘制命令，
//! 因此布局/三态/字体容错均可在**无 HDMI、无 framebuffer** 的机器上确定性断言。真机差异只剩
//! `fbdev::FbCanvas` 的设备打开与像素格式（设计 §13 前置项 1，部署阶段验证）。
//!
//! ## 进程层（bin）
//!
//! `main.rs` 为可执行入口 `mupc-local-display`（设计 §5.1：本 crate = lib + bin）：CLI →
//! 选后端（`offscreen` / Linux `fbdev` / `drm` 未实现明确报错）→ 起主循环 → 优雅退出；
//! panic 由外层兜底捕获后非零退出，交给 systemd `Restart=always`（§5.3/§9）。不解析
//! `mupc_core_config.yaml`（§7.2 零核心依赖）。

pub mod canvas;
pub mod channel;
pub mod config;
pub mod error;
pub mod font;
pub mod layout;
// LVGL 薄安全层（12-MUPC v2.0 工作单元 A1）：唯一允许 `unsafe` 的 Rust 侧位置之一。
// 只有本目录可以引用 `lvgl-sys`（设计 §1.1.1.2 unsafe 边界纪律 1）。
pub mod lvgl;
pub mod run;
pub mod state;

pub use crate::canvas::{Canvas, Color, OffscreenCanvas, Rect, blend_over, hex, rgb};
pub use crate::channel::{ChannelEndpoint, DisplayChannelClient, GET_TIMEOUT};
pub use crate::error::{Error, Result};
pub use crate::font::TextKit;
pub use crate::layout::{render, render_frame};
pub use crate::state::{
    ChannelStatus, DisplayState, Freshness, LiveDot, NumView, ScreenMode, SocView, UiSnapshot,
    CHANNEL_DOWN_MS,
};

/// 默认分辨率（设计 §6.1：8 寸屏 1024x768）。
pub const SCREEN_W: u32 = 1024;
/// 默认分辨率高。
pub const SCREEN_H: u32 = 768;

/// 便捷：按默认参数构造离屏画布（测试/`--backend offscreen` 用）。
pub fn new_offscreen_canvas() -> OffscreenCanvas {
    OffscreenCanvas::new(SCREEN_W, SCREEN_H)
}
