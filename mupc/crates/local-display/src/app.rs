//! 渲染进程**装配层**（工作单元 B3-2a）：LVGL 会话 + 六页外壳 + 通道状态机 → 事件循环宿主。
//!
//! # 为什么有本模块（与设计 §5.1/§5.2 的对照）
//!
//! 设计 §5.2 把装配写成 `ui::App::new(disp, indev, channel, console, cli)`，即 App 预期落在
//! `src/ui/` 下。**本轮不行**：`src/ui/**` 是 B1/B2c 已双审通过的交付物，本单元被明确要求
//! **不得改动**它；而 `ui/mod.rs` 自己写着「`UiState` 扩展 / 控制通道接线 / 触摸设备初始化
//! 仍属 B3」。故装配层落在**新的平级模块** `src/app.rs`（`main.rs` 的 bin 无法承载可测代码：
//! bin 不进 lib、集成测试够不着）。**接口语义与设计逐条一致**，只是文件位置不同。
//!
//! # 装配顺序（设计 §5.2 骨架的 ①–③ 段）
//!
//! ```text
//! lvgl::init()                          // lv_init + lv_tick_set_cb（单调 Instant）
//! Display::create(w, h)                 // lv_display_create + PARTIAL 双缓冲
//! disp.set_flush_cb(Blitter → PixelSink) // offscreen = MemorySink；fbdev = FbCanvas（P-1）
//! Indev::create_pointer(&disp)          // lv_indev 的 read_cb 桥 + MODE_EVENT
//! indev.set_long_press_time(theme::Timing::long_press_u16())
//! Shell::new(&Obj::screen())            // 页眉 + 6 页 + 导航（B2c-3 已交付）
//! ```
//!
//! # 每拍做什么（[`Host`] 的五个方法 = §5.2 骨架的 ③–⑥ 步）
//!
//! | 步 | 方法 | 本实现 |
//! |----|------|--------|
//! | ③ | [`Host::pump`] | 读 evdev（**非阻塞**）→ 更新 `Indev` 快照；无事件 = 正常空闲拍 |
//! | ④ | [`Host::read_indev`] | `lv_indev_read()` → LVGL 命中 / 派发 `LV_EVENT_*` |
//! | ⑤ | [`Host::on_lv_events`] | **有意为空**（见 [`Host::on_lv_events`] 的说明：LVGL 回调同步派发，无异步事件队列可消费 —— 不是"静默吞"，是**没有源**） |
//! | — | [`Host::next_deadline_ms`] | 读通道下一次轮询时刻（空闲回归由外壳自己算，见下） |
//! | ⑥ | [`Host::tick`] | 推进通道状态机（非阻塞）→ `DisplayState` → 帧驱动页 `render` → `Shell::tick` |
//!
//! # 两条**刻意的**取舍（登记，供评审裁定）
//!
//! 1. **空闲回归（TT-12）不回收到本模块**：`ui/shell.rs`（B2c-3）已经实现了完整状态机
//!    （倒计时胶囊 / `dirty` 时不强制切页 / 弹层打开期间暂停）。本模块只把
//!    `--idle-timeout-secs` 经 [`Shell::set_idle_timeout`] 注入，**不**另建
//!    [`crate::timing::IdleTimer`]：两套计时器是"同一口径的第二份真源"，会各自触发一次回归。
//!    `timing::IdleTimer` 因此在生产装配中**未被消费**（如实登记，未删除）。
//! 2. **帧驱动页的 `render` 只在"语义键"变化时调用**（`通道态 / 新鲜度 / 帧序号` 三元组）：
//!    页面 `render` 会写 `lv_obj`（标脏），每拍无条件调用会把 500 ms 节拍变成"永远整屏重绘"。
//!    语义键不变 ⇒ 屏上不可能有新信息 ⇒ 跳过是**等价**的，不是"省事"（判据与 v1.0
//!    `run.rs::redraw_if_needed` 的 `(mode, freshness, seq)` 键逐条同源）。
//!    该判据的**调用点**由 [`App::renders`]（帧驱动页实测渲染次数）观测：`--smoke` 的
//!    集成用例断言 `renders < ticks`（恒真退化 ⇒ 两者相等 ⇒ 红；见 B3-2a 质量评审 重要 I-2）。

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::channel::{next_poll_at, poll_due, DisplayChannelClient, Progress};
use crate::config::CliConfig;
use crate::lvgl::display::Display;
use crate::lvgl::indev::Indev;
use crate::lvgl::obj::Obj;
use crate::screen::{Blitter, MemorySink, PixelSink};
use crate::state::{ChannelStatus, ControlState, DisplayState, Freshness};
use crate::timing::Host;
use crate::ui::pages::{self, PageInput};
use crate::ui::shell::{NavPage, Shell};
use crate::ui::theme::{Dimens, Palette, Timing};

/// 当前 Unix 毫秒（与帧 `ts_ms` 同口径；不受时区影响）——供**新鲜度/通道态**比对。
///
/// ⚠️ **两个时基不要混**（本模块同时用两个，各有明确用途）：
/// - **Unix 毫秒**（本函数）：`DisplayState` 的新鲜度判据（`frame.ts_ms` 是 Unix 毫秒）；
/// - **单调毫秒**（事件循环 [`crate::timing::Clock`]）：节拍、空闲计时、状态机截止 ——
///   不受系统时钟跳变影响（设计 §1.1.1.1）。
pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 页眉时钟文本 `HH:MM:SS`（**UTC**）。
///
/// **必须从既有格式化出口取**（[`pages::format_epoch_ms_utc`]）而不是自造 `format!`：
/// 该出口产出的字符（数字 / `/` / `:` / 空格）**都在生成字体的 cmap 内**；v1.0 的
/// `clock_text_hms` 末尾缀了 `UTC`，其中 `U`/`T`/`C` 在 `ASCII_DISPLAY_ALPHABET` 里
/// **只有 `U` 与 `C` 有字形**（`T` 没有）⇒ 真机上是豆腐块（见 `pages/mod.rs` 偏差 D4/D9）。
/// 页眉时钟槽宽 = [`Dimens::BTN_MIN_W`]（120 px，UI §4.1「时钟 26 px 等宽」）⇒ 只放 `HH:MM:SS`。
pub fn clock_text(epoch_ms: u64) -> String {
    match pages::format_epoch_ms_utc(epoch_ms).rsplit_once(' ') {
        Some((_, t)) => t.to_string(),
        // `format_epoch_ms_utc` 恒含一个空格 ⇒ 不可达；兜底给空串而非 panic。
        None => String::new(),
    }
}

/// `drm` 后端未实现时的**明确错误**（设计 §13 前置项 1：待真机校验 `/dev/fb0` 像素格式/位深
/// 后再决定是否需要 DRM dumb-buffer 主平面）。绝不静默回退 offscreen——渲染进程必须有真实输出。
pub fn drm_unimplemented_error() -> crate::Error {
    crate::Error::Backend(
        "backend drm 未实现：设计 §13 前置项 1 待真机校验 /dev/fb0 像素格式/位深/字节序后再定 \
         DRM dumb-buffer 后端；真机请用 --backend fbdev（或先 --backend offscreen 打通全链路）"
            .to_string(),
    )
}

/// 启动期**致命**错误（设计 §9：打印明确原因 + 非零退出，交给 systemd `Restart=always`）。
///
/// 「通道失败」「触摸不可用」**不在**此列 —— 它们是正常展示态（PRD 6.3 / EDGE-13）。
#[derive(Debug)]
pub enum StartupError {
    /// LVGL 初始化 / display 创建 / 外壳装配失败（内存池不足、薄层未初始化…）。
    Lvgl(String),
    /// 触摸设备**致命**错误（多候选 / 校准不可信；设计 §1.2 要求启动即报错，不猜设备）。
    Touch(String),
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartupError::Lvgl(m) => write!(f, "LVGL 初始化/装配失败：{m}"),
            StartupError::Touch(m) => write!(f, "触摸设备致命错误：{m}"),
        }
    }
}

impl std::error::Error for StartupError {}

/// `--smoke` 自检报告（逐页内容区非背景像素数 + 时序 + 导出结果）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmokeReport {
    /// `(页, 内容区非背景像素数)`，顺序 = [`NavPage::ALL`]。
    pub pages: Vec<(NavPage, usize)>,
    /// 逐页渲染走查的墙钟耗时。
    pub walk_ms: u128,
    /// 本进程已完成的事件循环拍数。
    pub ticks: u64,
    /// 事件循环内**帧驱动页实测渲染次数**（见 [`App::renders`]；自检判定口之一）。
    pub renders: u64,
    /// 成功取得的帧数 / 失败次数。
    pub frames_ok: u64,
    pub frames_fail: u64,
    /// flush 搬运 / 丢弃的脏区数（见 [`crate::screen::BlitCounters`]；自检判定口之一）。
    pub blits: u64,
    pub dropped: u64,
    /// 导出结果（路径, 字节数）；未导出 ⇒ `None`。
    pub export: Option<(PathBuf, u64)>,
}

impl SmokeReport {
    /// 六页内容区**是否都非空**（自检的判定口：任一页为空 ⇒ 自检失败）。
    ///
    /// 白屏（持有型句柄被留在局部变量 ⇒ Drop 即级联删除）在本项目的离屏测试里"全绿"过一次
    /// —— 那正是本条存在的理由（第六节自查 ⑤）。
    pub fn all_pages_non_empty(&self) -> bool {
        !self.pages.is_empty() && self.pages.iter().all(|(_, n)| *n > 0)
    }

    /// **节流生效**：帧驱动页的渲染次数必须**严格少于**拍数（见 [`needs_render`]）。
    ///
    /// 恒真退化（每拍都渲染）⇒ 两者相等 ⇒ 本判据为假（B3-2a 质量评审 重要 I-2：
    /// 判据本身可测，但**调用点**此前零覆盖）。`ticks == 0`（一拍没跑）也判失败 ——
    /// "没跑过"不能算"节流成功"。
    pub fn throttle_effective(&self) -> bool {
        self.ticks > 0 && self.renders < self.ticks
    }

    /// **没有整拍被静默跳过**（[`crate::screen::BlitCounters::dropped`] 恒为 0）。
    ///
    /// 丢帧 = 目标被借走 ⇒ 该拍像素**没写上去**而 LVGL 不知道（屏上留永久陈旧像素）。
    /// 此前该计数在接线后**读不到**（I-4），故无从断言。
    ///
    /// 口径边界：本判据抓的是"真丢帧"；**抓不到**"计数根本没接线"（那种情况下 `dropped`
    /// 恒 0）—— 后者由 [`SmokeReport::flush_observed`] 兜住。
    pub fn no_dropped_frames(&self) -> bool {
        self.dropped == 0
    }

    /// **flush 路径确实在计数**（`blits > 0`）—— `dropped == 0` 的**对偶哨**。
    ///
    /// 专项理由（I-4 的自证）：`dropped == 0` 单独一条**抓不到**"计数句柄没接上"
    /// （游离/未接线的计数恒为 0，看似完美）。而自检走查必然产生搬运 ⇒ `blits == 0`
    /// 只可能是"读口没接到真闭包上"。
    pub fn flush_observed(&self) -> bool {
        self.blits > 0
    }
}

/// 帧驱动页的**语义键**：`通道态 / 新鲜度 / 帧序号` 三元组（见模块头取舍 2）。
///
/// 键的构成与 v1.0 `run.rs::redraw_if_needed` 的 `(mode, freshness, seq)` **逐条同源**
/// （该项是 B3-2a 从 `run.rs` 迁入的行为，回归面窄但后果大：见 [`needs_render`]）。
pub type RenderKey = (ChannelStatus, Freshness, Option<u64>);

/// **是否需要重渲染** —— `App::render_pages_if_needed`（私有方法）的**唯一判据**
/// （纯函数，全平台可测）。
///
/// # 为什么抽成纯函数
/// 本判据决定「是否每拍重渲染」，而它的**两个退化方向都不可见**（屏照样出画，只是别处坏）：
/// - 恒真（永远重渲染）⇒ 页面 `render` 每拍写 `lv_obj` 标脏 ⇒ 500 ms 节拍退化成
///   **永远整屏重绘**（CPU 飙升 + 屏面闪烁）；
/// - 恒假（永不重渲染）⇒ 屏面**停更**（通道态/新鲜度/帧全变，屏上不动）。
///
/// 因屏上"看起来还行"，两者都只能靠**用例**抓，故判据从 `render_pages_if_needed` 里抽出、
/// 单独可测（本项目纪律：纯逻辑可测）。
///
/// # 判据
/// - `prev == None`（首拍）⇒ **必须**渲染（否则屏面永不更新）；
/// - `prev == Some(k)` 且 `k` **逐字段相同** ⇒ 不需要 —— 屏上不可能有新信息，跳过是**等价**的；
/// - 键的**任一分量**变化 ⇒ 需要渲染。三路**互相独立**：通道态与新鲜度可以在帧序号
///   **不动**时变化（断连时 `seq` 保持上一次的值，但通道态必须上屏）。
pub fn needs_render(prev: Option<RenderKey>, new: RenderKey) -> bool {
    prev != Some(new)
}

/// 渲染进程装配体（事件循环宿主）。
///
/// **字段声明顺序 = 析构顺序**（Rust 保证）：`screen` → `shell` → `indev` → `display`
/// ——外壳（及其 6 页的 `lv_obj`）先于 display 释放，避免"删了 display 再删它的子对象"
/// 这类次序问题；[`Drop`] 里再调 [`crate::lvgl::deinit`]（它删除**全部** display/indev
/// 并回收各自的 `user_data`，此后所有句柄按世代失效 ⇒ 字段析构全部 no-op）。
pub struct App {
    /// 活动屏的**非拥有**句柄（`Obj::screen()`；`Drop` 不删屏，与设计 §5.2 的
    /// `Shell::new(&Obj::screen())` 一致）。
    #[allow(dead_code)] // 持有它只为固定"外壳的父"这一装配事实；句柄本身无需再读。
    screen: Obj,
    shell: Shell,
    #[cfg(target_os = "linux")]
    touch: Option<crate::touch::TouchDevice>,
    indev: Indev,
    /// LVGL display 句柄：**持有本身就是用途**（`Drop` 时删除 display 及其整棵子树）。
    /// 不读它的字段 ⇒ 显式 `allow(dead_code)`，避免"看起来没人用"被误删（删了屏就没了）。
    #[allow(dead_code)]
    display: Display,
    /// offscreen 后端的像素面（PPM 导出 / 逐页像素断言）；fbdev 后端 ⇒ `None`。
    mem_sink: Option<Rc<RefCell<MemorySink>>>,
    /// flush 搬运 / 丢帧计数（**接线后仍可读**，见 [`crate::screen::BlitCounters`]）。
    /// 丢帧此前不可见（I-4）：`dropped > 0` = 有该画的像素没画上去而 LVGL 不知道。
    blit_counters: Rc<crate::screen::BlitCounters>,
    /// 读通道客户端（非阻塞状态机）。
    channel: DisplayChannelClient,
    /// 帧/通道态（三态归一）。
    state: DisplayState,
    /// 控制通道态（本单元只接线其**确认弹层**入口；写操作生产者在 B3-2b）。
    control: ControlState,
    cfg: CliConfig,
    /// 事件循环单调时基原点（与循环 `Clock` **同源** ⇒ `now_ms` 可直接换算成 `Instant`）。
    /// 仅供 [`App::instant_at`] 内部换算使用（不对外暴露：循环自己持有同一个 `Instant`）。
    origin: Instant,
    /// 下一次允许发起 GET 的时刻（单调 ms；`None` = 首拍立即发起）。
    next_poll_ms: Option<u64>,
    /// 上次已渲染的语义键（`通道态 / 新鲜度 / 帧序号`）——见模块头取舍 2 与 [`needs_render`]。
    render_key: Option<RenderKey>,
    /// 已完成的拍数 / 取帧成功数 / 失败数（退出报告用）。
    ticks: u64,
    frames_ok: u64,
    frames_fail: u64,
    /// 帧驱动页的**实测渲染次数**（[`App::render_pages_if_needed`] 里语义键真变化的那一次）。
    ///
    /// 存在的理由（I-2）：[`needs_render`] 判据本身有用例，但**调用点**此前零覆盖 ——
    /// 把接线退化成恒真（永远渲染）时全部用例仍绿。本计数把它变成可观测的判据
    /// （`--smoke` 断言 `renders < ticks`）。
    renders: u64,
    /// 触摸读错误次数（EDGE-13：只读展示不受影响，故只计数 + 首条/每百条日志）。
    touch_errors: u64,
}

impl Drop for App {
    fn drop(&mut self) {
        // 先落 LVGL 会话（删除全部 display/indev + 回收 user_data）⇒ 之后字段析构全是 no-op
        // （句柄按世代/存活标志失效，见 `lvgl::obj::Obj::is_alive`）。
        crate::lvgl::deinit();
    }
}

impl App {
    /// 装配**离屏**（`--backend offscreen`）渲染进程：内存 sink + LVGL display/indev + 外壳。
    ///
    /// `origin` 必须与事件循环所用的时钟**同源**（`main.rs` 里同一个 `Instant`）。
    pub fn new_offscreen(cfg: &CliConfig, origin: Instant) -> Result<Self, StartupError> {
        let sink: Rc<RefCell<MemorySink>> =
            Rc::new(RefCell::new(MemorySink::new(cfg.width, cfg.height)));
        let mut app = Self::build(cfg, origin, Blitter::new(Rc::clone(&sink)))?;
        app.mem_sink = Some(sink);
        Ok(app)
    }

    /// 装配 **Linux framebuffer** 后端（`--backend fbdev`）。
    #[cfg(target_os = "linux")]
    pub fn new_fbdev(cfg: &CliConfig, origin: Instant) -> Result<Self, StartupError> {
        use crate::canvas::fbdev::FbCanvas;
        let fb = FbCanvas::open(&cfg.fbdev_path, cfg.width, cfg.height).map_err(|e| {
            StartupError::Lvgl(format!(
                "打开/映射 framebuffer `{}` 失败：{e}；\
                 需 root 或 video 组成员权限，可先用 --backend offscreen 验证全链路；\
                 像素格式/分辨率不符属设计 §13 前置项 1（真机首验）",
                cfg.fbdev_path
            ))
        })?;
        let app = Self::build(cfg, origin, Blitter::new(fb))?;
        Ok(app)
    }

    /// 共用装配：LVGL init → display（flush 桥接 `blitter`）→ indev → 触摸 → 外壳。
    ///
    /// **持有型句柄全部进结构体**（不留在局部变量）：`Display` / `Indev` / `Shell` 一旦被
    /// 局部变量持有，函数返回时 `Drop` 会级联删除整棵控件树 ⇒ 屏空白但测试全绿（本项目
    /// 的高发缺陷类 ⑤）。
    fn build<S>(cfg: &CliConfig, origin: Instant, blitter: Blitter<S>) -> Result<Self, StartupError>
    where
        S: PixelSink + 'static,
    {
        crate::lvgl::init().map_err(|e| StartupError::Lvgl(e.to_string()))?;
        let mut display = Display::create(cfg.width, cfg.height)
            .map_err(|e| StartupError::Lvgl(e.to_string()))?;
        // flush 桥：LVGL 渲染好的脏区 → Blitter → sink。回调内**只搬像素**
        // （不阻塞、不做通道 I/O、不调 LVGL —— §5.2 不变量 3）。
        //
        // ⚠️ **必须先取计数句柄再接线**（I-4）：`into_flush_closure` 会把 `Blitter` move 进
        // 闭包，接线之后 `blits()`/`dropped()` 就再也读不到了 —— 而 `dropped` 是"该画的像素
        // 没画上"的唯一观测哨（屏上留永久陈旧像素而 LVGL 不知道）。
        let blit_counters = blitter.counters();
        display.set_flush_cb(blitter.into_flush_closure());

        let indev = Indev::create_pointer(&display)
            .map_err(|e| StartupError::Lvgl(e.to_string()))?;
        // 长按阈值：**逐 indev**（v9 语义），值取自 `ui/theme.rs` 单一真源（不得硬编码 1000）。
        indev.set_long_press_time(Timing::long_press_u16());

        #[cfg(target_os = "linux")]
        let touch = Self::open_touch(cfg)?;
        #[cfg(target_os = "linux")]
        if let Some(t) = touch.as_ref() {
            // 首拍前先落一次快照（否则 LVGL 在用户真正按下之前读到的是默认 (0,0) released）。
            indev.feed(t.snapshot());
        }

        let screen = Obj::screen().map_err(|e| StartupError::Lvgl(e.to_string()))?;
        let shell = Shell::new(&screen).map_err(|e| StartupError::Lvgl(e.to_string()))?;
        // TT-12 空闲回归参数**注入外壳**（外壳持有该状态机，见模块头取舍 1）。
        shell.set_idle_timeout(cfg.idle_timeout_secs);
        // EDGE-13：触摸不可用 ⇒ 页眉角标（只在真的不可用时置位，不猜）。
        #[cfg(target_os = "linux")]
        shell.set_touch_available(touch.is_some());
        #[cfg(not(target_os = "linux"))]
        shell.set_touch_available(false);

        let channel = DisplayChannelClient::try_new(&cfg.channel)
            .map_err(|e| StartupError::Lvgl(format!("数据通道 URL 非法：{e}")))?;
        let mut state = DisplayState::new();
        state.set_stale_ms(cfg.stale_ms);

        Ok(Self {
            screen,
            shell,
            #[cfg(target_os = "linux")]
            touch,
            indev,
            display,
            mem_sink: None,
            blit_counters,
            channel,
            state,
            control: ControlState::new(),
            cfg: cfg.clone(),
            origin,
            next_poll_ms: None,
            render_key: None,
            ticks: 0,
            frames_ok: 0,
            frames_fail: 0,
            renders: 0,
            touch_errors: 0,
        })
    }

    /// 打开触摸设备（仅 Linux）。
    ///
    /// 失败语义（设计 §1.2 / EDGE-13）：
    /// - [`TouchError::is_fatal`]（多候选 / 校准不可信）⇒ **启动即报错**
    ///   （[`StartupError::Touch`]，不猜设备、不用不可信校准）；
    /// - 其余（无设备 / 打不开 / 非触摸设备）⇒ warn + **降级只读展示**
    ///   （返回 `None`，页眉挂「触摸不可用」角标，数据刷新照常）。
    #[cfg(target_os = "linux")]
    fn open_touch(cfg: &CliConfig) -> Result<Option<crate::touch::TouchDevice>, StartupError> {
        match crate::touch::TouchDevice::open(&cfg.touch_config()) {
            Ok(t) => {
                let b = &t.calibration().bounds;
                eprintln!(
                    "[mupc-local-display] 触摸设备 {} 已打开（{}；原始量程 x[{}..{}] y[{}..{}]）",
                    t.path().display(),
                    t.source().as_str(),
                    b.x_min,
                    b.x_max,
                    b.y_min,
                    b.y_max
                );
                Ok(Some(t))
            }
            Err(e) if e.is_fatal() => Err(StartupError::Touch(e.to_string())),
            Err(e) => {
                eprintln!(
                    "[mupc-local-display] 触摸不可用（EDGE-13）：{e} —— {}；\
                     继续只读展示（数据刷新不受影响），页眉挂「触摸不可用」角标",
                    e.degrade_hint()
                );
                Ok(None)
            }
        }
    }

    // ── 只读观测口（退出报告 / smoke / 集成测试）─────────────────────────────

    /// 已完成的拍数。
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// 取帧成功 / 失败次数。
    pub fn frame_counts(&self) -> (u64, u64) {
        (self.frames_ok, self.frames_fail)
    }

    /// 触摸读错误次数（EDGE-13 的诊断哨：**持续增长说明设备在掉线**，但数据刷新不受影响）。
    pub fn touch_errors(&self) -> u64 {
        self.touch_errors
    }

    /// 帧驱动页的**实测渲染次数**（I-2 的调用点观测口；自检断言 `renders < ticks`）。
    pub fn renders(&self) -> u64 {
        self.renders
    }

    /// flush 搬运 / 丢弃计数（I-4 的读口：`dropped > 0` = 有像素该画而没画上）。
    pub fn flush_stats(&self) -> (u64, u64) {
        (self.blit_counters.blits(), self.blit_counters.dropped())
    }

    /// 离屏像素面（仅 `--backend offscreen`；fbdev ⇒ `None`）。
    ///
    /// **当前无生产消费者**：B3-2b 的控制通道回执要走"写操作后回读屏面确认"（如 P2 应用后的
    /// 参数回显自检），届时由集成测试与自检路径消费 —— 故按要求保留并登记，不删。
    pub fn memory_sink(&self) -> Option<&Rc<RefCell<MemorySink>>> {
        self.mem_sink.as_ref()
    }

    /// 读通道客户端的只读句柄（**退出统计行**用：真机排障要看解析后的 `addr`、单次超时与
    /// 连续失败数 —— 这三个正是 I-5 裁定"接进 `report_exit` 而不是删掉"的三个量）。
    ///
    /// 只读（`&`）：`begin`/`tick`/`cancel` 都要 `&mut`，外部拿不到，单线程纪律不变。
    pub fn channel(&self) -> &DisplayChannelClient {
        &self.channel
    }

    /// 触摸设备 fd（交给 `poll(2)`；**唯一阻塞点**的监听目标）。
    ///
    /// `None` = 无触摸设备（降级只读展示）⇒ 循环改走 `poll(NULL,0,t)` / 纯超时阻塞
    /// —— **仍是唯一阻塞点**，不是忙等（设计 §5.2 不变量 1，`timing::PollWait`）。
    #[cfg(target_os = "linux")]
    pub fn touch_fd(&self) -> Option<std::os::fd::RawFd> {
        self.touch.as_ref().map(|t| t.fd())
    }

    // ── 每拍推进 ──────────────────────────────────────────────────────────

    /// 把一次取帧结果灌进状态层（成功 → `record_success`；失败 → `record_fail` + 有限日志）。
    ///
    /// 日志节奏与 v1.0 一致：**首失败与每 10 次**各一条（通道长期不通时不刷屏，
    /// 设计 §9 的断连 CPU/日志友好）。
    fn absorb(&mut self, res: Result<mupc_display_proto::DisplayFrame, crate::Error>, epoch_ms: u64) {
        match res {
            Ok(frame) => {
                self.frames_ok += 1;
                if self.frames_ok == 1 {
                    eprintln!(
                        "[mupc-local-display] 首帧到达：seq={} ts_ms={}",
                        frame.seq, frame.ts_ms
                    );
                }
                self.state.record_success(frame, epoch_ms);
            }
            Err(e) => {
                self.frames_fail += 1;
                if self.frames_fail == 1 || self.frames_fail % 10 == 0 {
                    eprintln!(
                        "[mupc-local-display] 拉帧失败(第 {} 次)：{e}；\
                         超过 3 s 无成功将切「与主进程数据通道断开」态",
                        self.frames_fail
                    );
                }
                self.state.record_fail(epoch_ms);
            }
        }
    }

    /// 帧驱动页的 `render`（**只在语义键变化时**）——见模块头取舍 2；
    /// 判据本身是纯函数 [`needs_render`]（可测，见 `tests::render_key_*`），
    /// **调用点**由 [`App::renders`] 计数（I-2：判据可测但调用点曾零覆盖）。
    fn render_pages_if_needed(&mut self, epoch_ms: u64) {
        let channel = self.state.channel_status(epoch_ms);
        let freshness = self.state.freshness(epoch_ms);
        let seq = self.state.frame().map(|f| f.seq);
        let key: RenderKey = (channel, freshness, seq);
        if !needs_render(self.render_key, key) {
            return;
        }
        self.render_key = Some(key);
        // **计数点在判据之后、渲染之前**：恒真退化 ⇒ 本计数 = 拍数（自检据此判红）。
        self.renders = self.renders.saturating_add(1);
        let input = PageInput::new(self.state.frame(), channel, freshness);
        // 三页是**帧驱动**（契约 2）；P2/P3/P5 由控制通道驱动（契约 2′，接线属 B3-2b）。
        self.shell.p1().render(&input);
        self.shell.p6().render(&input);
        self.shell.p4().render(&input);
    }

    /// 把单调毫秒换算成 `Instant`（与 [`Self::origin`] 同源）。
    fn instant_at(&self, now_ms: u64) -> Instant {
        self.origin + Duration::from_millis(now_ms)
    }
}

impl Host for App {
    /// ③ 读 evdev → 更新 `Indev` 快照。**绝不阻塞**（fd 在 `TouchDevice::open` 时已置非阻塞）。
    ///
    /// 「无事件」是**正常空闲拍**（`Ok(false)`），不是错误（评审 Critical 1：把它当错误会
    /// 每拍刷一行日志并把触摸永久判死）。
    fn pump(&mut self) {
        #[cfg(target_os = "linux")]
        {
            let outcome = self
                .touch
                .as_mut()
                .map(|t| t.pump().map(|changed| (changed, t.snapshot())));
            match outcome {
                None => {} // 无触摸设备（降级只读展示）
                Some(Ok((true, snap))) => self.indev.feed(snap),
                Some(Ok((false, _))) => {} // 空闲拍：无新事件
                Some(Err(e)) => {
                    self.touch_errors += 1;
                    if self.touch_errors == 1 || self.touch_errors % 100 == 0 {
                        eprintln!(
                            "[mupc-local-display] 触摸读失败(第 {} 次)：{e}；\
                             数据刷新不受影响（EDGE-13）",
                            self.touch_errors
                        );
                    }
                }
            }
        }
    }

    /// ④ `lv_indev_read()` 主动投递 ⇒ LVGL 命中 / z-order / 弹层拦截 / `LV_EVENT_*` 派发。
    fn read_indev(&mut self) {
        self.indev.read();
    }

    /// ⑤ 消费 LVGL 事件队列 → 业务动作。
    ///
    /// **本实现有意为空，且不是"静默吞信息"**：`ui/**`（B1/B2c）**没有**异步事件队列 ——
    /// 所有 UI 动作都在 LVGL 事件回调内**同步**落到 `Shell` / 页对象（切页、防抖、草稿、
    /// 意图回调），投递点就是上一行的 [`Host::read_indev`]。故此处**没有源可消费**：
    /// 加一个"取出即丢弃"的队列反而会制造"事件被谁处理了"的歧义。
    /// 若将来 B3-2b 引入需要**延后**处理的动作（例如控制通道回执要在循环里而非回调里落库），
    /// 其队列入口就挂在这里。
    fn on_lv_events(&mut self) {}

    /// 本拍到下一次必须唤醒的时刻：读通道下一次轮询（TT-12 空闲回归由外壳在 `Shell::tick`
    /// 内部计，不需要循环级截止，见模块头取舍 1）。
    ///
    /// # ⚠️ **在途请求期间本值有意是"已过期"的**（B3-2a 质量评审 建议 I-6，登记不走样）
    ///
    /// `next_poll_ms` 停在「上一拍发起时刻 + `--poll-ms`」；在途请求持续期间它**不会前移**，
    /// 于是 `compute_timeout_ms` 恒算得 0，进而触发 [`crate::timing::apply_zero_timeout_clamp`]
    /// 的退避阶梯（1→2→…→500 ms）。**这是有意的、也是必需的**：
    /// - 非阻塞状态机（[`DisplayChannelClient::tick`]）需要**立即唤醒**才能尽快推进
    ///   （连接交回 → 写出 → 读入，一拍一步）；若在途期间返回 `None`，本拍就只剩 LVGL 与
    ///   `poll` 上界（默认 500 ms）唤醒 ⇒ 一次 GET 要 2~3 拍 ≈ 1.0~1.5 s，逼近 2 s 截止。
    /// - 阶梯正是"立即唤醒"的节流器（否则 `poll(0)` 自旋，§5.2 不变量 1）。
    ///
    /// **代价（如实登记，不得当成故障信号）**：`LoopStats::zero_timeout_clamps` /
    /// `zero_timeout_iters` 在**正常**的在途路径上也会增长（约 10 拍内饱和到阶梯顶端）。
    /// 该字段的判据已在 [`crate::timing::LoopStats::zero_timeout_clamps`] 处登记来源。
    fn next_deadline_ms(&self) -> Option<u64> {
        self.next_poll_ms
    }

    /// ⑥ 推进通道状态机（非阻塞）→ 归一 → 渲染帧驱动页 → 外壳每拍。
    fn tick(&mut self, now_ms: u64) {
        self.ticks += 1;
        let epoch_ms = now_epoch_ms();
        let now = self.instant_at(now_ms);

        // ① 推进在途请求（**非阻塞**：一拍的推进量由 `MAX_STEPS_PER_TICK` 封顶）。
        if self.channel.is_busy() {
            if let Progress::Done(res) = self.channel.tick(now) {
                self.absorb(res, epoch_ms);
            }
        }
        // ② 按 `--poll-ms` 节拍发起下一拍（首拍立即发起：`next_poll_ms == None`）。
        if !self.channel.is_busy() && poll_due(now_ms, self.next_poll_ms) {
            if let Err(e) = self.channel.begin(now) {
                // 结构上不可达（上面刚判过 `!is_busy()`）⇒ 出现即装配逻辑错位，必须看得见。
                eprintln!("[mupc-local-display] 通道发起请求失败（装配逻辑异常）：{e}");
            }
            self.next_poll_ms = Some(next_poll_at(now_ms, self.cfg.interval_ms));
        }
        // ③ 设计 §5.5：`DeviceSection.hmi_channel` 由 HMI **本地覆盖**为自身通道态
        //    （服务端恒给 `Unknown` —— 避免"由服务端报告客户端自己的连接状态"这一语义倒置）。
        self.state.apply_hmi_channel(epoch_ms);
        // ④ 帧驱动页渲染（语义键变化才做）。
        self.render_pages_if_needed(epoch_ms);
        // ⑤ 外壳每拍：页眉通道胶囊 / 时钟文本 / 空闲回归 / 未保存提示条 / 页内延迟动作。
        self.shell.set_channel(self.state.channel_status(epoch_ms));
        // 确认弹层打开期间暂停空闲回归（TT-13）——**接线已登记的契约**；生产者（P2/P4 的
        // 确认流）属 B3-2b，此刻恒为 `false`（无弹层）。
        self.shell.set_modal_open(self.control.confirm_open());
        self.shell.tick(now, &clock_text(epoch_ms));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// `--smoke` 一键自检（设计 §9）
// ═══════════════════════════════════════════════════════════════════════════

/// ⚠️ **登记（B3-2a 规格评审 建议 7）：自检路径的后置渲染泵 —— 仅 `--smoke` 用，
/// 不属于生产事件循环**。
///
/// 组成 = [`SMOKE_PUMP_ROUNDS`] × [`SMOKE_PUMP_STEP`]（真 `sleep`）+ 每轮一次
/// [`crate::lvgl::timer_handler`]；调用点只有 [`App::smoke`] 的逐页走查与其后的「回主状态页」
/// 收尾（`let _ = self.pump_render(|| false);`）。
///
/// **生产路径没有这个泵、也不得有**：生产渲染**只由 `lv_timer_handler` 驱动**
/// （设计 §5.2 不变量 2：禁止 `lv_refr_now`；渲染时机由 LVGL 刷新定时器决定）。
/// 自检需要"等到这一页真的落了像素"才能统计，故只能在**自检**里补一个有界的小循环；
/// 它出现在生产路径 = 设计回归（评审请按此判）。
///
/// 逐页走查时，每页最多等多少轮渲染泵（每轮 = 一次 `lv_timer_handler` + 一段真等待）。
///
/// **为什么必须真等**：LVGL 的重绘由它自己的刷新定时器（`lv_conf.h` 的默认刷新周期 33 ms）
/// 驱动，`lv_timer_handler()` 只在"该定时器到点"时才落脏区。生产路径**禁止** `lv_refr_now`
/// （§5.2 不变量 2）⇒ 只能用"多次驱动 + 短等待"把刷新定时器喂到点。25 × 20 ms = 500 ms 上界。
const SMOKE_PUMP_ROUNDS: u32 = 25;
/// 每轮渲染泵的等待步长。
const SMOKE_PUMP_STEP: Duration = Duration::from_millis(20);

impl App {
    /// 渲染泵：驱动 LVGL 直到 `done()` 为真或轮数用尽（返回实际轮数）。
    fn pump_render(&self, mut done: impl FnMut() -> bool) -> u32 {
        for round in 1..=SMOKE_PUMP_ROUNDS {
            crate::lvgl::timer_handler();
            if done() {
                return round;
            }
            std::thread::sleep(SMOKE_PUMP_STEP);
        }
        SMOKE_PUMP_ROUNDS
    }

    /// 内容区（页眉之下、导航之上）非背景像素数 —— 「这一页真的画了东西」的判据。
    fn content_pixels(&self) -> usize {
        let Some(sink) = self.mem_sink.as_ref() else {
            return 0; // fbdev 后端没有可回读的内存面（自检只支持 offscreen，见 `smoke`）
        };
        let g = sink.borrow();
        let (w, h) = (g.width(), g.height());
        let y0 = Dimens::HEADER_H.max(0) as u32;
        let y1 = (Dimens::HEADER_H + Dimens::CONTENT_H).min(h as i32).max(0) as u32;
        let mut n = 0usize;
        for y in y0..y1 {
            for x in 0..w {
                if g.pixel(x as i32, y as i32).is_some_and(|p| !is_screen_bg(p)) {
                    n += 1;
                }
            }
        }
        n
    }

    /// 把像素面清成背景色（逐页统计前调用，保证统计的是**本页**画出来的像素）。
    fn clear_screen(&self) {
        let Some(sink) = self.mem_sink.as_ref() else {
            return;
        };
        let (w, h) = {
            let g = sink.borrow();
            (g.width(), g.height())
        };
        let bg = vec![screen_bg_pixel(); (w as usize) * (h as usize)];
        sink.borrow_mut().write_pixels(0, 0, w, h, &bg);
    }

    /// `--smoke` 一键自检：逐页渲染 6 页 → 统计内容区非背景像素 →（可选）导出 PPM。
    ///
    /// 只支持 `--backend offscreen`（fbdev 没有可回读的像素面 ⇒ 返回 `Err`，**不静默给 0**）。
    ///
    /// 每页走查的两步（顺序有意义）：先 `show` **另一页**、再 `show` 目标页 ——
    /// `Shell::select` 会把 6 个页容器逐个 `set_hidden`，但"切到已经是当前页的那一页"是否
    /// 触发 LVGL 失效**依赖于薄层/内核的早退实现**。先切走再切回，则"隐藏旧页 + 显示新页"
    /// 两次状态翻转必然发生 ⇒ **不依赖**该实现细节也能保证本页被重绘。
    pub fn smoke(&mut self, out: Option<&Path>) -> Result<SmokeReport, String> {
        if self.mem_sink.is_none() {
            return Err(
                "--smoke 自检需要可回读的离屏像素面：请用 --backend offscreen（fbdev 无回读口）"
                    .to_string(),
            );
        }
        let t0 = Instant::now();
        let mut pages_out = Vec::with_capacity(NavPage::ALL.len());
        for (i, page) in NavPage::ALL.into_iter().enumerate() {
            let other = NavPage::ALL[(i + 1) % NavPage::ALL.len()];
            self.clear_screen();
            self.shell.show(other);
            self.shell.show(page);
            // **真的切到本页了吗**：不校验的话，六次统计会退化成"同一页画了六遍"，而
            // 「六页都非空」的结论就成了**恒真断言**（本项目明令禁止的伪门禁）。
            if self.shell.current() != page {
                return Err(format!(
                    "自检走查页切换失败：请求 {page:?}，当前 {:?}（六页统计将退化为同一页的重复计数）",
                    self.shell.current()
                ));
            }
            // 见到本页像素即收工；空页则耗尽 500 ms 上界（这是"失败也要有界"的一拍）。
            let _rounds = self.pump_render(|| self.content_pixels() > 0);
            pages_out.push((page, self.content_pixels()));
        }
        let walk_ms = t0.elapsed().as_millis();

        let export = match out {
            None => None,
            Some(path) => {
                let bytes = self.export_ppm(path)?;
                Some((path.to_path_buf(), bytes))
            }
        };
        // 自检结束回主状态页（P1；与 TT-12 的回归目标页一致）。
        // ⚠️ 这里的 `pump_render` 是**自检收尾的后置渲染泵**（见 [`SMOKE_PUMP_ROUNDS`] 的登记）：
        // 只为把"回主状态页"这一拍泵到落像素；**生产事件循环没有它**。
        self.shell.show(NavPage::Main);
        let _ = self.pump_render(|| false);

        // flush 计数在此处（**全部泵都结束后**）取：`dropped > 0` = 走查期间有像素该画而没画上。
        let (blits, dropped) = self.flush_stats();
        Ok(SmokeReport {
            pages: pages_out,
            walk_ms,
            ticks: self.ticks,
            renders: self.renders,
            frames_ok: self.frames_ok,
            frames_fail: self.frames_fail,
            blits,
            dropped,
            export,
        })
    }

    /// 导出当前像素面为 **PPM(P6)**，返回写入字节数（头 + `w*h*3`）。
    ///
    /// 为什么不 PNG：本 crate 依赖面刻意最小（设计 §5.1「依赖刻意保持最小」），PNG 需引编码
    /// crate；PPM 是零依赖、无损、Linux 图像工具可直接查看的等价替代
    /// （见 [`MemorySink::write_ppm`]，该偏离已在工作单元 C 评审 C-④ 判可接受）。
    pub fn export_ppm(&self, path: &Path) -> Result<u64, String> {
        let Some(sink) = self.mem_sink.as_ref() else {
            return Err("fbdev 后端没有可导出的内存像素面".to_string());
        };
        let (w, h) = {
            let g = sink.borrow();
            (g.width(), g.height())
        };
        sink.borrow()
            .write_ppm(path)
            .map_err(|e| format!("导出 PPM `{}` 失败：{e}", path.display()))?;
        Ok(ppm_bytes(w, h))
    }
}

/// 屏幕底色在**像素面**里的位串（`screen::Blitter` 的装箱口径：LVGL 缓冲内存序 `B,G,R,X`）。
///
/// 用 `u32::from_ne_bytes`（而不是自己移位）与 `screen.rs` **同源**：字节序知识只保留一份。
/// alpha 字节填 0 —— 判据侧只看 RGB（见 [`is_screen_bg`]），故该值不参与比较。
fn screen_bg_pixel() -> crate::canvas::Color {
    let c = Palette::BG;
    u32::from_ne_bytes([c.b, c.g, c.r, 0])
}

/// 像素是否为**屏幕底色**（只比 RGB 三字节，**忽略 alpha**）。
///
/// 为什么不直接比 `u32`：`Blitter` 把 LVGL 的 4 字节像素（内存序 `B,G,R,X`）原样装成 u32，
/// 而 X（alpha）字节的内容由 LVGL 内部决定（本层无从假定）⇒ 逐字节比 RGB 既正确又与字节序无关。
fn is_screen_bg(px: crate::canvas::Color) -> bool {
    let b = px.to_ne_bytes();
    let c = Palette::BG;
    b[0] == c.b && b[1] == c.g && b[2] == c.r
}

/// PPM(P6) 文件的期望字节数：`"P6\n{w} {h}\n255\n"` 头 + `w*h*3` 像素字节。
///
/// 供自检与集成测试**共用同一条算式**（避免"测试里另写一份尺寸算式"而漂移）。
pub fn ppm_bytes(w: u32, h: u32) -> u64 {
    let head = format!("P6\n{w} {h}\n255\n").len() as u64;
    head + (w as u64) * (h as u64) * 3
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 页眉时钟：**只由既有格式化出口派生**，且字符全在生成字体的 cmap 内。
    ///
    /// **改什么会让本条变红**：把实现换成自造 `format!("{:02}:{:02}:{:02} UTC", …)`
    /// （v1.0 `run.rs::clock_text_hms` 的形态）⇒ 断言 ① 仍过（长度 12），但断言 ② 立刻抓住
    /// `UTC` 里的 `T`（不在 `ASCII_DISPLAY_ALPHABET` 内 ⇒ 真机豆腐块）。
    #[test]
    fn clock_text_is_short_and_uses_only_cmap_glyphs() {
        let t = clock_text(1_789_047_727_000); // 2026/09/10 13:42:07 UTC
        assert_eq!(t, "13:42:07", "页眉时钟恒为 HH:MM:SS（槽宽 120 px）");
        for c in t.chars() {
            assert!(
                pages::ASCII_DISPLAY_ALPHABET.contains(c),
                "字符 `{c}` 不在上屏安全字母表内（真机豆腐块风险）"
            );
        }
        // 跨天回绕仍同形
        assert_eq!(clock_text(1_789_047_727_000 + 86_400_000), "13:42:07");
        assert_eq!(clock_text(0), "00:00:00");
    }

    #[test]
    fn epoch_ms_is_a_plausible_unix_millisecond() {
        // 2026-09-15T00:00:00Z = 1789603200 s；下界取 2020 年（明显早于本仓活动期）。
        assert!(now_epoch_ms() > 1_577_836_800_000);
    }

    #[test]
    fn drm_backend_error_is_explicit_with_alternative() {
        let msg = drm_unimplemented_error().to_string();
        assert!(msg.contains("drm"));
        assert!(msg.contains("fbdev"), "应给出可用替代，不静默：{msg}");
    }

    /// PPM 期望字节数 = 头 + `w*h*3`（自检与集成测试共用同一条算式）。
    #[test]
    fn ppm_expected_size_matches_header_and_pixels() {
        // 头按 PPM 规范拼出（与 `MemorySink::write_ppm` 的格式逐字一致）⇒ 断言不引入第二份算式。
        assert_eq!(
            ppm_bytes(1024, 768),
            "P6\n1024 768\n255\n".len() as u64 + 1024 * 768 * 3
        );
        // 头长随尺寸位数变化：两位数尺寸的头短 3 字节（`1024 768` → `10 10`）
        assert_eq!(ppm_bytes(10, 10), "P6\n10 10\n255\n".len() as u64 + 300);
    }

    /// 自检报告的**三条判定口**：六页非空 / 节流生效 / 无丢帧。
    ///
    /// **改什么会让本条变红**：
    /// - `SmokeReport::throttle_effective` 改成恒真 ⇒ ①`renders == ticks` 那组立刻红；
    /// - `no_dropped_frames` 改成 `true` ⇒ ②`dropped = 1` 那组红（详见交付报告「探针实测」）。
    #[test]
    fn smoke_report_requires_every_page_non_empty() {
        let base = SmokeReport {
            pages: vec![
                (NavPage::Main, 10),
                (NavPage::Config, 20),
                (NavPage::Logs, 0),
            ],
            walk_ms: 1,
            ticks: 20,
            renders: 3,
            frames_ok: 0,
            frames_fail: 0,
            blits: 100,
            dropped: 0,
            export: None,
        };
        assert!(!base.all_pages_non_empty(), "空页必须判失败");
        let all_ok = SmokeReport {
            pages: vec![(NavPage::Main, 1), (NavPage::Config, 2)],
            ..base.clone()
        };
        assert!(all_ok.all_pages_non_empty());
        let none = SmokeReport {
            pages: Vec::new(),
            ..base.clone()
        };
        assert!(!none.all_pages_non_empty(), "一页都没走查 ≠ 全通过");

        // ① 节流：`renders < ticks` 才算生效；相等（恒真退化）与"一拍没跑"都判失败。
        assert!(base.throttle_effective(), "3 < 20：节流生效");
        let always = SmokeReport {
            renders: 20,
            ..base.clone()
        };
        assert!(
            !always.throttle_effective(),
            "每拍都渲染（renders == ticks）= 恒真退化，必须判失败"
        );
        let no_tick = SmokeReport {
            ticks: 0,
            renders: 0,
            ..base.clone()
        };
        assert!(!no_tick.throttle_effective(), "一拍没跑 ≠ 节流生效");

        // ② 丢帧：`dropped == 0` 才算通过（目标被借走 ⇒ 该拍像素没画上而 LVGL 不知道）。
        assert!(base.no_dropped_frames());
        assert!(base.flush_observed(), "100 次搬运 ⇒ flush 路径确实在计数");
        let lost = SmokeReport { dropped: 1, ..base.clone() };
        assert!(!lost.no_dropped_frames(), "丢帧必须判失败（屏上留永久陈旧像素）");
        // ②′ 对偶哨：**一次搬运都没有** ⇒ 计数根本没接线（此时 `dropped == 0` 是假象）。
        let unwired = SmokeReport { blits: 0, ..base };
        assert!(unwired.no_dropped_frames(), "（假象：没接线时 dropped 也是 0）");
        assert!(
            !unwired.flush_observed(),
            "blits == 0 必须判失败：自检走查必然产生搬运，否则说明读口没接上"
        );
    }


    /// 语义键节流 ①：**同键连推两拍 ⇒ 只渲染 1 次**（首拍渲染 + 第二拍跳过）。
    ///
    /// **为什么必须有本条**：`render_pages_if_needed` 决定「是否每拍重渲染」，而它的
    /// 恒真退化（永远重渲染）在屏上**看起来正常** —— 只是 500 ms 节拍变成永远整屏重绘
    /// （CPU/闪烁）。评审实测：该函数全仓**零用例**，两个退化方向都不可见。
    ///
    /// **改什么会让本条变红**（**实测**）：把 [`needs_render`] 改成恒真 ⇒ 第二拍照样返回
    /// `true` ⇒ `renders` 实得 2（见交付报告「探针实测」）。
    #[test]
    fn render_key_throttles_identical_consecutive_ticks() {
        let k: RenderKey = (ChannelStatus::Connected, Freshness::Fresh, Some(7));
        let mut prev: Option<RenderKey> = None;
        let mut renders = 0u32;
        // 连推两拍，语义键逐字段不变（模拟"同一帧被连读两次、通道态也没变"）。
        for _ in 0..2 {
            if needs_render(prev, k) {
                renders += 1;
                prev = Some(k);
            }
        }
        assert_eq!(
            renders, 1,
            "同键第二拍必须跳过：否则每拍都调页面 render（标脏）= 永远整屏重绘"
        );
    }

    /// 语义键节流 ②：**键变 ⇒ 必须渲染**，且**三路互相独立**（通道态 / 新鲜度 / 帧序号
    /// 任一变一位都要重渲染）—— 对偶退化方向「节流过度 ⇒ 屏面停更」的判据。
    ///
    /// **改什么会让本条变红**（**实测**）：把 [`needs_render`] 改成恒假 ⇒ 首拍断言立刻红
    /// （下界）；改成"只比 seq" ⇒ 通道态/新鲜度两路红（见交付报告「探针实测」）。
    #[test]
    fn render_key_changes_on_any_component() {
        let base: RenderKey = (ChannelStatus::Connected, Freshness::Fresh, Some(7));
        assert!(needs_render(None, base), "首拍必须渲染（否则屏面永不更新）");
        assert!(!needs_render(Some(base), base), "键逐字段相同 ⇒ 跳过");
        // ① 通道态：断连时 `seq` **不动**（屏上数据是上一次的），但通道态必须上屏
        let ch: RenderKey = (ChannelStatus::Down, Freshness::Fresh, Some(7));
        assert!(needs_render(Some(base), ch), "通道态变化必须渲染");
        // ② 新鲜度：`now − ts_ms > stale_ms` 翻转时 seq 同样不动
        let fr: RenderKey = (ChannelStatus::Connected, Freshness::Stale, Some(7));
        assert!(needs_render(Some(base), fr), "新鲜度变化必须渲染");
        // ③ 帧序号
        let sq: RenderKey = (ChannelStatus::Connected, Freshness::Fresh, Some(8));
        assert!(needs_render(Some(base), sq), "帧序号变化必须渲染");
        // ④ 帧从有到无（首帧前 / 状态复位）也是一种变化（`Option<u64>` 的 `None` 侧）
        let no: RenderKey = (ChannelStatus::Init, Freshness::Stale, None);
        assert!(needs_render(Some(base), no), "seq 消失（None）必须渲染");
        assert!(needs_render(None, no), "首拍即无帧也要渲染（初始化中/断连态文案）");
    }

}
