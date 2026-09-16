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
//! | ⑤ | [`Host::on_lv_events`] | 消费**接线层的意图队列**（B3-2b-2；`ui/**` 仍无异步事件队列，见该方法的说明）—— **T-3 门禁的落点** |
//! | — | [`Host::next_deadline_ms`] | 读通道下一次轮询时刻（空闲回归由外壳自己算，见下） |
//! | ⑥ | [`Host::tick`] | 推进读/控制两条通道状态机（非阻塞）→ 回执路由 → `DisplayState` → 帧驱动页 `render` → `Shell::tick` |
//!
//! # 两条**刻意的**取舍（登记，供评审裁定）
//!
//! 1. **空闲回归（TT-12）不回收到本模块**：`ui/shell.rs`（B2c-3）已经实现了完整状态机
//!    （倒计时胶囊 / `dirty` 时不强制切页 / 弹层打开期间暂停）。本模块只把
//!    `--idle-timeout-secs` 经 [`Shell::set_idle_timeout`] 注入，**不**另建
//!    `timing::IdleTimer`：两套计时器是"同一口径的第二份真源"，会各自触发一次回归。
//!    该类型**已在 B3-2b-1 删除**（不再是"登记在册但留着"—— 见当时的整改：TT-12 状态机由
//!    外壳 [`Shell::tick`] **单点**承担，删掉第二份真源）。
//! 2. **帧驱动页的 `render` 只在"语义键"变化时调用**（`通道态 / 新鲜度 / 帧序号` 三元组）：
//!    页面 `render` 会写 `lv_obj`（标脏），每拍无条件调用会把 500 ms 节拍变成"永远整屏重绘"。
//!    语义键不变 ⇒ 屏上不可能有新信息 ⇒ 跳过是**等价**的，不是"省事"（判据与 v1.0
//!    `run.rs::redraw_if_needed` 的 `(mode, freshness, seq)` 键逐条同源）。
//!    该判据的**调用点**由 [`App::renders`]（帧驱动页实测渲染次数）观测：`--smoke` 的
//!    集成用例断言 `renders < ticks`（恒真退化 ⇒ 两者相等 ⇒ 红；见 B3-2a 质量评审 重要 I-2）。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mupc_display_proto::{ConfigPatch, ConsoleEndpoint, InterlockOpPayload};

use crate::channel::{next_poll_at, poll_due, DisplayChannelClient, Progress};
use crate::config::CliConfig;
use crate::console::{ConsoleClient, ConsoleClock, ConsoleResult};
use crate::control_route::{
    audit_query_string, log_query_string, p3_connected, route, ControlIntent, RawPayload,
    RouteDecision,
};
use crate::lvgl::display::Display;
use crate::lvgl::indev::Indev;
use crate::lvgl::obj::Obj;
use crate::screen::{Blitter, MemorySink, PixelSink};
use crate::state::{ChannelStatus, ControlState, DisplayState, Freshness};
use crate::timing::Host;
use crate::ui::pages::p3_logs::LogQuery;
use crate::ui::pages::p5_audit::AuditQuery;
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
    /// **控制通道客户端建不起来**（`--control-channel` 非法；B3-2b-2）。
    ///
    /// 单列一个变体（而不是塞进 `Lvgl`）：两者的**排障入口完全不同** —— 前者改 CLI / unit，
    /// 后者查 LVGL 装配。且"控制通道建不起来"**绝不静默降级**为"无控制通道的只读屏"：
    /// 那会让 P2/P4 的保存与释放按钮永远无效而屏上无任何解释（PRD §2.6 降级必须可见）。
    Control(String),
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartupError::Lvgl(m) => write!(f, "LVGL 初始化/装配失败：{m}"),
            StartupError::Touch(m) => write!(f, "触摸设备致命错误：{m}"),
            StartupError::Control(m) => write!(f, "控制通道客户端创建失败：{m}"),
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

/// P3 增量拉取的节拍（设计 §6.3「实时追加」行：**每 500 ms 拉 `cursor` 增量**）。
///
/// **不取 `--poll-ms`**：该值是**读通道**（帧）的轮询节拍，它的上界 500 ms 是 F7.3 / F16.5
/// 端到端时延算式的输入（设计 §5.5 的硬上界说明）；日志增量是**另一条通道**的节拍，
/// 复用会让"改帧节拍"顺带改掉日志时延（耦合两件不相干的事）。设计给的数就是 500 ms。
const CONSOLE_INCREMENT_MS: u64 = 500;

/// EDGE-19 / **M9** 的「补发一次 GET」判据（**纯函数**，可测）。
///
/// P4 在"提交时状态已变化"（或被前置条件拒绝）时置 `refresh_requested` 标志
/// （`p4_interlock.rs` 的 `show_conflict` / `show_result` 两处置位）；**B3 必须消费它**，
/// 否则「自动刷新」的语义落空（M9 原文）。消费动作 = 把**读通道**的下一次轮询提前到下一拍
/// （`next_poll_ms = None` ⇒ `channel::poll_due(_, None)` 恒真）—— 联锁态的真源是**显示帧**，
/// 不是控制通道的某个 GET，故"补发"补的是帧 GET。
///
/// 抽成自由函数（而不是写在 `tick` 里）：`App` 的构造需要 LVGL 会话，纯逻辑用例够不着；
/// 判据抽出来后可独立断言，**调用点**由 [`App::p4_refresh_forced`] 计数并打印进退出统计行
/// （与 [`needs_render`] 同款："判据可测 + 调用点有观测口"）。
pub fn apply_refresh_request(refresh_requested: bool, next_poll_ms: &mut Option<u64>) -> bool {
    if !refresh_requested {
        return false;
    }
    *next_poll_ms = None;
    true
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
    /// **控制通道客户端**（`--control-channel`；B3-2b-2 接线）。
    ///
    /// 单条在飞：`begin_write` / `begin_query` 在 `is_busy()` 时**响亮失败**（`Busy`）
    /// ⇒ 接线层必须先判在途（见 [`App::begin_write_intent`] / [`App::begin_query_intent`]）。
    console: ConsoleClient,
    /// **意图队列**（屏 → 网络；`Rc<RefCell<..>>` 是因为页面回调持有的是它、而不是 `&mut App`）。
    ///
    /// 生产者 = 六页的意图回调（P2 提交 / P4 释放 · M1 授权 / P3 · P5 的筛选与分页）；
    /// 消费者 = [`Host::on_lv_events`]。**写操作只能从这里产生**（T-3 门禁的结构性保证）。
    intents: Rc<RefCell<VecDeque<ControlIntent>>>,
    /// **启动期**要发起的读清单（5 条；GET 串行：同一时刻仅 1 条在飞）。
    ///
    /// ⚠️ **订正（B3-2b-2 整改 建议 4）**：此前本行写「启动期 / **切页期**」，**不实** ——
    /// 全仓**没有**往本清单追加的生产者（唯一写入点是 [`App::build`] 的装配段）。
    /// 切页 / 筛选变化由**意图队列**真做（`ControlIntent::LogQuery` / `AuditQuery` /
    /// `AuditLoadMore` → [`App::begin_query_intent`]）。
    pending_reads: VecDeque<ConsoleEndpoint>,
    /// 最近一次发出的日志查询（「回到最新」用它重取首屏 —— 该意图载荷是 `()`）。
    last_log_query: Option<LogQuery>,
    /// 最近一次发出的审计查询（切页 / 刷新时重发同一条，免去在接线层复制页面筛选态）。
    last_audit_query: Option<AuditQuery>,
    /// 上一次注入 P3 的通道条态（`None` = 尚未注入）—— 只在**翻转时**才碰 LVGL，
    /// 免得每拍都 `Core::layout()`。
    p3_connected_injected: Option<bool>,
    /// P3 增量拉取的下一次到期时刻（单调 ms；`None` = 立即）。
    next_increment_ms: Option<u64>,
    /// EDGE-19 的「补发一次 GET」生效次数（判据 = [`apply_refresh_request`]）。
    p4_refresh_forced: u64,
    /// 在途期间**被丢弃**的写意图数。
    ///
    /// ⚠️ **订正（B3-2b-2 整改 重要 3）**：此前本行写「丢弃已上屏提示」——**不成立**：
    /// ① 丢弃路径落的 [`ControlState::push_toast`] 那条「操作进行中」**没有任何页面消费者**
    ///    （`App` 与 `ui/**` 都不读 `ControlState::toast()` / `toast_text()`；上屏的 Toast
    ///    一律由页面**自己的** `show_toast` 建）；
    /// ② 该分支**生产不可达**：提交中两页的按钮均已 disabled（P2 `refresh_actions` 的
    ///    `usable = available && !submitting` ⇒ `save` / `reset` 同灰；P4 `op_state` 的
    ///    `busy` ⇒ 两按钮同灰）⇒ 在途期间用户**无法**再次触发确认回调 ⇒ 队列里不会有写意图。
    /// ⇒ 本路径为**防御性**：**只计数**，屏上无提示。与裁定 3 的传输失败出口**同因**
    /// （均待页面补 Toast 入口；那是 `src/ui/**` 改动，需单独立项）。
    write_intents_dropped: u64,
    /// 在途期间**被丢弃**的读意图数（读意图密集，丢弃**只计数**、不弹 Toast —— 见报告"选择"）。
    read_intents_dropped: u64,
    /// 回执**路由失败**数（形态 / 解码不符；正常恒为 0）。
    route_errors: u64,
    /// 帧/通道态（三态归一）。
    state: DisplayState,
    /// 控制通道态（在途 / 回执摘要 / Toast 生命周期 / 确认弹层标记）。
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
        // 控制通道客户端（`--control-channel`；B3-2b-2 接实）。**建不起来即启动失败**，
        // 不静默降级成"无控制通道的只读屏"（见 [`StartupError::Control`] 的说明）。
        let console = ConsoleClient::new(&cfg.control_channel)
            .map_err(|e| StartupError::Control(format!("`{}`：{e}", cfg.control_channel)))?;
        let mut state = DisplayState::new();
        state.set_stale_ms(cfg.stale_ms);

        // 意图队列 + 六页的意图回调接线（**唯一的意图生产者**）。
        let intents: Rc<RefCell<VecDeque<ControlIntent>>> =
            Rc::new(RefCell::new(VecDeque::new()));
        Self::bind_intents(&shell, &intents);

        // 启动期的读取清单（设计 §5.5：查询可在启动期发起；写操作**不在此列**）。
        let mut pending_reads: VecDeque<ConsoleEndpoint> = VecDeque::new();
        for ep in [
            ConsoleEndpoint::Config,
            ConsoleEndpoint::Logs,
            ConsoleEndpoint::LogsTargets,
            ConsoleEndpoint::Audit,
            ConsoleEndpoint::AuditOps,
        ] {
            pending_reads.push_back(ep);
        }

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
            console,
            intents,
            pending_reads,
            last_log_query: None,
            last_audit_query: None,
            p3_connected_injected: None,
            next_increment_ms: None,
            p4_refresh_forced: 0,
            write_intents_dropped: 0,
            read_intents_dropped: 0,
            route_errors: 0,
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

    /// 把六页的**意图回调**接到共享队列上（**唯一的意图生产者**）。
    ///
    /// # 为什么经队列而不是在回调里直接发请求
    ///
    /// ① 回调在 **LVGL 事件派发**里跑（`Host::read_indev` 内），而 `App` 此刻正被 `&mut` 借走
    ///    ⇒ 回调**拿不到** `ConsoleClient`；`Rc<RefCell<..>>` 是唯一不引入第二份真源的形态。
    /// ② **T-3 门禁**：这三条写回调只在**确认完成**时触发（P2 的 L1/L2+ 双步 + 长按、
    ///    P4 的 L2 长按）——未确认 ⇒ 回调不跑 ⇒ 队列为空 ⇒ 一包都不发。
    /// ③ 设计 §5.2 不变量 3：LVGL 回调内**不得**做 I/O；队列把动作推到 `Host::on_lv_events`。
    ///
    /// ⚠️ **禁止在回调里回灌页面数据**（`p3::set_targets` 会删正在派发的 chip ⇒ UAF 级，
    /// 见 `p3_logs.rs` 的调用方约束）—— 本函数只登记"意图"，所有回灌都发生在 `tick` 路径。
    fn bind_intents(shell: &Shell, intents: &Rc<RefCell<VecDeque<ControlIntent>>>) {
        {
            let q = Rc::clone(intents);
            shell.p2().set_on_submit(move |patch: ConfigPatch, _level| {
                q.borrow_mut().push_back(ControlIntent::ConfigApply(patch));
            });
        }
        {
            let q = Rc::clone(intents);
            shell.p4().set_on_release(move |p: InterlockOpPayload| {
                q.borrow_mut().push_back(ControlIntent::InterlockRelease(p));
            });
        }
        {
            let q = Rc::clone(intents);
            shell.p4().set_on_ack_m1(move |p: InterlockOpPayload| {
                q.borrow_mut().push_back(ControlIntent::InterlockAckM1(p));
            });
        }
        {
            let q = Rc::clone(intents);
            shell.p3().set_on_query(move |query: LogQuery| {
                q.borrow_mut().push_back(ControlIntent::LogQuery(query));
            });
        }
        {
            let q = Rc::clone(intents);
            shell.p3().set_on_increment(move |query: LogQuery| {
                q.borrow_mut().push_back(ControlIntent::LogIncrement(query));
            });
        }
        {
            let q = Rc::clone(intents);
            shell.p3().set_on_back_to_latest(move || {
                q.borrow_mut().push_back(ControlIntent::LogBackToLatest);
            });
        }
        {
            let q = Rc::clone(intents);
            shell.p5().set_on_query(move |query: AuditQuery| {
                q.borrow_mut().push_back(ControlIntent::AuditQuery(query));
            });
        }
        {
            let q = Rc::clone(intents);
            shell.p5().set_on_load_more(move |query: AuditQuery| {
                q.borrow_mut().push_back(ControlIntent::AuditLoadMore(query));
            });
        }
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

    // ═══════════════════════════════════════════════════════════════════════
    // 控制通道接线（B3-2b-2）：意图 → 请求 → 回执 → 页面
    // ═══════════════════════════════════════════════════════════════════════

    /// 控制通道客户端的只读句柄（退出统计行 / 集成自证用）。
    pub fn console(&self) -> &ConsoleClient {
        &self.console
    }

    /// 退出时的控制通道连续失败数（**P3 通道条态的真源**，见 [`control_route::p3_connected`]）。
    pub fn console_fail_streak(&self) -> u32 {
        self.console.fail_streak()
    }

    /// 当前注入 P3 的通道条态（`true` = 「实时日志已连接」）。
    ///
    /// **口径**：由**控制通道**可达性派生（[`control_route::p3_connected`]），**不是**帧通道
    /// —— 理由见该函数的文档。退出统计行会打印 `p3_channel=up|down`，进程级用例据此断言。
    pub fn p3_channel_connected(&self) -> bool {
        let connected = p3_connected(self.console.fail_streak());
        // 未注入过 ⇒ 报告**将要**注入的值（上电初值 = 已连接），与实际注入点同源。
        self.p3_connected_injected.unwrap_or(connected)
    }

    /// 在途期间被丢弃的**写**意图数（丢弃同时经 ControlState 上屏一条「操作进行中」）。
    pub fn write_intents_dropped(&self) -> u64 {
        self.write_intents_dropped
    }

    /// 在途期间被丢弃的**读**意图数（丢弃**只计数**，不弹 Toast —— 见交付报告"选择"）。
    pub fn read_intents_dropped(&self) -> u64 {
        self.read_intents_dropped
    }

    /// 回执**路由失败**数（形态 / 解码不符；正常恒为 0）。
    pub fn route_errors(&self) -> u64 {
        self.route_errors
    }

    /// EDGE-19 的「补发一次 GET」实际生效次数（M9：B3 必须消费 `take_refresh_request`）。
    pub fn p4_refresh_forced(&self) -> u64 {
        self.p4_refresh_forced
    }

    /// 消费意图队列（**`Host::on_lv_events` 调用**；LVGL 事件派发之外）。
    ///
    /// **T-3 门禁的结构性落点**：写请求的 `begin_write` 调用点**只有**
    /// [`App::begin_write_intent`]，而它的调用点**只有**本函数的
    /// `ConfigApply` / `Interlock*` 三条分支；这三条分支的唯一生产者是页面**确认完成**回调
    /// 压入的队列 ⇒ **未确认 = 队列为空 = 零网络动作**。
    fn handle_control_intents(&mut self, epoch_ms: u64) {
        loop {
            let Some(intent) = self.intents.borrow_mut().pop_front() else {
                return;
            };
            match intent {
                ControlIntent::ConfigApply(patch) => self.begin_write_intent(
                    ConsoleEndpoint::ConfigApply,
                    "apply",
                    &patch,
                    epoch_ms,
                ),
                ControlIntent::InterlockRelease(p) => self.begin_write_intent(
                    ConsoleEndpoint::InterlockRelease,
                    "release",
                    &p,
                    epoch_ms,
                ),
                ControlIntent::InterlockAckM1(p) => self.begin_write_intent(
                    ConsoleEndpoint::InterlockAckM1,
                    "ack_m1",
                    &p,
                    epoch_ms,
                ),
                ControlIntent::LogQuery(q) => {
                    let qs = log_query_string(&q);
                    self.last_log_query = Some(q);
                    self.begin_query_intent(ConsoleEndpoint::Logs, qs);
                }
                ControlIntent::LogIncrement(q) => {
                    let qs = log_query_string(&q);
                    self.last_log_query = Some(LogQuery { cursor: None, ..q });
                    self.begin_query_intent(ConsoleEndpoint::Logs, qs);
                }
                ControlIntent::LogBackToLatest => {
                    // 「回到最新」= 用**上一次**查询（游标归零）重取首屏；页面载荷是 `()`，
                    // 拉不到筛选态 ⇒ 以接线层留存的上一次查询为准（`None` ⇒ 契约默认档）。
                    let q = self.last_log_query.clone().unwrap_or_default();
                    let qs = log_query_string(&LogQuery { cursor: None, ..q.clone() });
                    self.last_log_query = Some(LogQuery { cursor: None, ..q });
                    self.begin_query_intent(ConsoleEndpoint::Logs, qs);
                }
                ControlIntent::AuditQuery(q) => {
                    let qs = audit_query_string(&q);
                    self.last_audit_query = Some(q);
                    self.begin_query_intent(ConsoleEndpoint::Audit, qs);
                }
                ControlIntent::AuditLoadMore(q) => {
                    let qs = audit_query_string(&q);
                    self.last_audit_query = Some(q);
                    self.begin_query_intent(ConsoleEndpoint::Audit, qs);
                }
            }
        }
    }

    /// **写**意图 → `begin_write`（`begin_write` 在本 crate 的**唯一**生产调用点）。
    ///
    /// 在途 ⇒ **丢弃**：计数 + 记一条既有 §3.6 文案「操作进行中」
    /// （`p4_interlock::TEXT_OP_BUSY`，**不自造新串**）。**不排队、不自动重试**
    /// （重试是显式动作；`RetryWindowExpired` 的出路见交付报告"未决 ③"）。
    ///
    /// ⚠️ **"可见"不成立（B3-2b-2 整改 重要 3，如实订正）**：那条 Toast 落在
    /// [`ControlState::push_toast`]，而 `ControlState` 的 toast 当前**无页面消费者**；
    /// 且提交中两页按钮已 disabled ⇒ 本分支**防御性、生产不可达**。与裁定 3 的传输失败
    /// 出口**同因**（均待页面补 Toast 入口）。判据与理由见 [`App::write_intents_dropped`]。
    fn begin_write_intent<P: serde::Serialize>(
        &mut self,
        ep: ConsoleEndpoint,
        op: &str,
        payload: &P,
        epoch_ms: u64,
    ) {
        debug_assert!(ep.is_write(), "写意图必须落在写端点上");
        if self.console.is_busy() {
            // ⚠️ **记账 ≠ 上屏**（重要 3 订正）：`ControlState` 的 toast 无页面消费者，
            // 且本分支生产不可达（提交中两页按钮已 disabled）⇒ 此处只计数，屏上无提示。
            // 保留 `push_toast` 是**防御性**记账（将来页面补上 Toast 入口即自然可见）。
            self.write_intents_dropped += 1;
            self.control
                .push_toast(crate::ui::pages::p4_interlock::TEXT_OP_BUSY, epoch_ms);
            return;
        }
        match self.console.begin_write(op, payload, ConsoleClock::now()) {
            Ok(request_id) => {
                self.control.begin(ep, Some(&request_id));
                // 提交中：按钮 disabled + 「保存中...」（P2）/ 两按钮 disabled（P4）。
                self.set_submitting(ep, true);
            }
            Err(e) => {
                // ⚠️ **登记（B3-2b-2 整改 建议 4，本轮只登记不实现）**：本路径只有 stderr，
                // **无上屏出口** —— 与传输失败（`absorb_console` 里本地合成回执 → `show_result`）
                // 的口径**不同**。可达性：`begin_write` 自身失败近乎结构不可达（写端点 + 已拼好的
                // 载荷）；待页面补通用 Toast 入口后再统一上屏口径。
                self.set_submitting(ep, false);
                self.control.record_transport_failure(epoch_ms);
                eprintln!("[mupc-local-display] 控制通道写请求发起失败（{}）：{e}", ep.path());
            }
        }
    }

    /// **读**意图 → `begin_query`。
    ///
    /// 在途处理（**写优先，绝不打断**）：
    /// - 在途的是**查询** ⇒ 旧查询已过期（用户换了筛选条件）⇒ `cancel()` 作废后发新的
    ///   （`ConsoleClient::cancel` 的登记用途即此）；
    /// - 在途的是**写** ⇒ **丢掉**本次读意图并计数（写请求绝不能被打断；**不弹 Toast**：
    ///   读意图密集，逐条弹会刷屏，见交付报告"选择"）。
    fn begin_query_intent(&mut self, ep: ConsoleEndpoint, query: String) {
        if self.console.is_busy() {
            if self.console.inflight_request_id().is_none() {
                self.console.cancel();
                self.control.finish();
            } else {
                self.read_intents_dropped += 1;
                return;
            }
        }
        if let Err(e) = self.console.begin_query(ep, &query, ConsoleClock::now()) {
            // 只可能是 `QueryTooLarge` / `BadQuery`（拼串 bug）⇒ 响亮，不静默。
            self.read_intents_dropped += 1;
            eprintln!(
                "[mupc-local-display] 控制通道读请求发起失败（{}）：{e}",
                ep.path()
            );
        }
    }

    /// 复位「提交中」态（**失败 / 传输错误也必须复位**，否则按钮永久禁用 +「保存中...」）。
    fn set_submitting(&self, ep: ConsoleEndpoint, on: bool) {
        match ep {
            ConsoleEndpoint::ConfigApply => self.shell.p2().set_submitting(on),
            ConsoleEndpoint::InterlockRelease | ConsoleEndpoint::InterlockAckM1 => {
                self.shell.p4().set_submitting(on)
            }
            _ => {}
        }
    }

    /// 某读端点的查询串（GET 参数在 query；写端点不可达）。
    fn query_for(&self, ep: ConsoleEndpoint) -> String {
        match ep {
            ConsoleEndpoint::Logs => log_query_string(&self.last_log_query.clone().unwrap_or_default()),
            ConsoleEndpoint::Audit => {
                audit_query_string(&self.last_audit_query.clone().unwrap_or_default())
            }
            // `config` / `logs/targets` / `audit/ops` 无参（§3.4 请求列为「—」）。
            _ => String::new(),
        }
    }

    /// 消化一次控制通道完成事件：**路由 / 失败**两条出路。
    fn absorb_console(&mut self, res: ConsoleResult<crate::console::ConsoleOutcome<RawPayload>>, epoch_ms: u64) {
        let outcome = match res {
            Ok(o) => o,
            Err(e) => {
                // 传输失败（连接 / 超时 / 非 200 / 解码）：**按既有口径**记入 `ControlState`
                // （清在途 + 「操作失败」Toast，文案 `state::TRANSPORT_FAIL_TEXT`，**不自造**）；
                // 另把"提交中"复位，否则按钮永久禁用。
                //
                // **裁定 3（B3-2b-2 整改）**：那条兜底 Toast 的句柄归页面、而**没有任何页面
                // 暴露通用 Toast 入口** ⇒ 光记账 = 用户按「保存」时**屏上什么都不发生**。
                // 故对**写**端点再补一条**本地合成**的「不可用」回执，走页面**既有**的
                // `show_result`（复用上屏路径；`src/ui/**` 零改动）。读端点不合成（它们的降级
                // 出口是 P3 通道条态）。合成回执**不是**服务端回执，字段取值理由见
                // `control_route::transport_failure_decision`。
                //
                // ⚠️ 返回值**必须**被消费 —— 它带**显式** `#[must_use]`（**不要**指望 `Option`
                // 自带该属性：本工具链实测**不成立** —— 裸调用不报任何告警，见 `state.rs` 该方法的
                // 注）：漏掉下面那句 `apply_route`，回执就"只造不送" —— 屏上依旧是"什么都不发生"，
                // 而 `unused_must_use` 告警会在"零警告"判据上当场变红。
                // 先取在途端点（`record_*` 会清掉在途）；在途信息由 `record_*_with_receipt`
                // 自己取出，此处只为复位"提交中"。
                let ep = self.control.inflight().map(|i| i.endpoint);
                if let Some(decision) = self.control.record_transport_failure_with_receipt(epoch_ms)
                {
                    self.apply_route(decision);
                }
                if let Some(ep) = ep {
                    self.set_submitting(ep, false);
                }
                eprintln!("[mupc-local-display] 控制通道请求失败：{e}");
                return;
            }
        };
        let decision = match route(&outcome) {
            Ok(d) => d,
            Err(e) => {
                // ⚠️ **登记（B3-2b-2 整改 建议 4，本轮只登记不实现）**：回执**形态 / 解码不符**
                // 这条出口同样只有 stderr、**无上屏出口**，与传输失败的合成回执口径**不同**
                // （那一条走 `show_result` 上屏）。待页面补通用 Toast 入口后再统一。
                self.route_errors += 1;
                let ep = self.control.inflight().map(|i| i.endpoint);
                self.control.record_transport_failure(epoch_ms);
                if let Some(ep) = ep {
                    self.set_submitting(ep, false);
                }
                eprintln!("[mupc-local-display] {e}（回执已丢弃，不冒充成功）");
                return;
            }
        };
        // 回执摘要（写操作才带信封）：清在途 + 按 `toast_text` 规则弹 Toast。
        // 查询载荷不是 `ControlResponse` ⇒ 走 `finish()` 清在途（不记摘要）。
        match outcome.response() {
            Some(resp) => self.control.record_response(resp, epoch_ms),
            None => self.control.finish(),
        }
        self.apply_route(decision);
    }

    /// **唯一的**回执 → 页面分派点（判据来自 [`control_route::route`]，此处只做 `match`）。
    fn apply_route(&mut self, decision: RouteDecision) {
        match decision {
            RouteDecision::Config(view) => {
                if let Err(e) = self.shell.p2().set_config(&view) {
                    Self::report_lvgl(e);
                }
            }
            RouteDecision::ConfigApply(resp) => {
                self.shell.p2().set_submitting(false);
                if let Err(e) = self.shell.p2().show_result(&resp) {
                    Self::report_lvgl(e);
                }
            }
            // ⚠️ `set_targets` **只在这里**调用（tick 路径）：它删在屏 chip 并重建，
            // 在 LVGL 事件回调里回灌会删正在派发的对象（UAF 级，见 `p3_logs.rs` 的调用方约束）。
            RouteDecision::LogsTargets(t) => self.shell.p3().set_targets(&t),
            RouteDecision::Logs(page) => self.shell.p3().set_page(&page),
            RouteDecision::Audit(page) => self.shell.p5().set_page(&page),
            RouteDecision::AuditOps(o) => self.shell.p5().set_ops(&o),
            // 联锁写回执：**成功与一切失败**都走这里（含 `RejectedPrecondition`）—— 具体原因
            // 由服务端 `message` 承担，页面**不**按消息串猜语义（PM 裁定 1，见
            // `control_route::RouteDecision::InterlockResult` 的沿革段）。
            RouteDecision::InterlockResult(resp) => {
                self.shell.p4().set_submitting(false);
                if let Err(e) = self.shell.p4().show_result(&resp) {
                    Self::report_lvgl(e);
                }
            }
        }
    }

    /// 页面注入失败（`LvglError`）：**只诊断、不 panic、不改业务状态**（屏上少一块 ≠ 数据错）。
    fn report_lvgl(e: crate::lvgl::LvglError) {
        eprintln!("[mupc-local-display] 控制回执注入页面失败：{e}");
    }

    /// 每拍推进控制通道（**绝不阻塞**：`tick` 内只有非阻塞调用）。
    fn tick_console(&mut self, now_ms: u64, epoch_ms: u64) {
        // ① 推进在途（连接 / 写 / 读三段状态机，一拍一步）。
        if self.console.is_busy() {
            if let crate::console::Progress::Done(res) =
                self.console.tick::<RawPayload>(self.instant_at(now_ms))
            {
                self.absorb_console(res, epoch_ms);
            }
        }
        // ② 空闲 ⇒ 发下一条读（**只**消费启动期清单；切页 / 筛选变化走意图队列，
        //    见 [`App::pending_reads`] 的订正段）。
        if !self.console.is_busy() {
            if let Some(ep) = self.pending_reads.pop_front() {
                let q = self.query_for(ep);
                if let Err(e) = self.console.begin_query(ep, &q, ConsoleClock::now()) {
                    self.read_intents_dropped += 1;
                    eprintln!(
                        "[mupc-local-display] 控制通道读请求发起失败（{}）：{e}",
                        ep.path()
                    );
                }
            }
        }
        // ③ P3 增量拉取节拍（设计 §6.3：「每 500 ms 拉 cursor 增量」）。
        //    只在**P3 在前台**且空闲时触发（后台页不空转请求）。
        if self.shell.current() == NavPage::Logs {
            // MSRV = 1.75（workspace `rust-version`）⇒ 不用 `Option::is_none_or`（1.82 才稳定）。
            let due = self.next_increment_ms.map_or(true, |t| now_ms >= t);
            if due && !self.console.is_busy() {
                self.next_increment_ms = Some(now_ms.saturating_add(CONSOLE_INCREMENT_MS));
                self.shell.p3().request_increment();
            }
        }
        // ④ P3 通道条态：**控制通道**可达性（不是帧通道，理由见 `control_route::p3_connected`）。
        let connected = p3_connected(self.console.fail_streak());
        if self.p3_connected_injected != Some(connected) {
            self.p3_connected_injected = Some(connected);
            self.shell.p3().set_channel(connected);
        }
        // ⑤ EDGE-19：被拒 ⇒ 补发一次读通道 GET（M9 登记的"B3 必须消费"）。
        let refresh = self.shell.p4().take_refresh_request();
        if apply_refresh_request(refresh, &mut self.next_poll_ms) {
            self.p4_refresh_forced += 1;
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
    /// # 本实现在 B3-2b-2 起**不再为空**（登记：`ui/**` 仍无异步事件队列）
    ///
    /// `ui/**`（B1/B2c）**没有**异步事件队列 —— 所有 UI 动作都在 LVGL 事件回调内**同步**
    /// 落到 `Shell` / 页对象（切页、防抖、草稿、意图回调），投递点就是上一行的
    /// [`Host::read_indev`]。故这里**消费的不是 LVGL 事件**，而是**接线层自己的意图队列**
    /// （[`crate::control_route::ControlIntent`]）：页面回调把"要发的请求"压进队列
    /// （回调运行在 `&mut App` 被借走的 LVGL 派发帧内、且按 §5.2 不变量 3 不得做 I/O），
    /// 真正的 `begin_write` / `begin_query` 在本拍、**事件派发之外**执行。
    ///
    /// **这一步就是 T-3 门禁的落点**：队列的写侧生产者只有"确认完成"回调
    /// （见 [`App::bind_intents`]），未确认 ⇒ 队列为空 ⇒ 零网络动作。
    fn on_lv_events(&mut self) {
        self.handle_control_intents(now_epoch_ms());
    }

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
        // ⑤ **控制通道**每拍推进（B3-2b-2）：非阻塞状态机 → 回执路由 → P3 通道条态。
        //    排在帧路径之后：`apply_refresh_request` 把 `next_poll_ms` 置 `None`，效果落在**下一拍**
        //    的 ② 段（≤1 拍延迟，设计 §6.4 的"自动触发一次状态刷新"只要求"尽早"，不要求同拍）。
        self.tick_console(now_ms, epoch_ms);
        // ⑥ 外壳每拍：页眉通道胶囊 / 时钟文本 / 空闲回归 / 未保存提示条 / 页内延迟动作。
        self.shell.set_channel(self.state.channel_status(epoch_ms));
        // 确认弹层打开期间暂停空闲回归（TT-13）——**接线已登记的契约**。
        // ⚠️ **登记（B3-2b-2，未闭合）**：`ControlState.confirm` 的生产者仍缺 —— 页面侧
        //    **没有**任何生产可见的"弹层是否已打开"查询口（`p2.with_dialog` 是 `#[cfg(test)]`，
        //    `ui/shell.rs` 偏差 **SH2** 原文即如此）⇒ 接线层**无从**在"弹层刚打开、用户尚未确认"
        //    这一段置位。可在"确认完成（意图回调）→ 回执到达"那段置位，但那只是窗口的一半，
        //    半对的模态标记比恒 `false` 更难推理 ⇒ 本单元**不动它**，如实登记待契约定夺
        //    （见交付报告"未决/存疑 ④"）。
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

    /// EDGE-19 的「补发一次 GET」（M9：B3 必须消费 `take_refresh_request`）。
    ///
    /// **改什么会让本条变红**（**实测**）：把 `apply_refresh_request` 里的
    /// `*next_poll_ms = None` 改成空语句（"消费了但没生效"）⇒ 第 2 条红；
    /// 把 `if !refresh_requested` 去掉（恒真消费）⇒ 第 1 / 3 条红。
    #[test]
    fn refresh_request_forces_the_next_frame_poll() {
        // ① 未请求刷新 ⇒ **不动**已有的节拍（不得凭空虚增请求）
        let mut due = Some(12_345);
        assert!(!apply_refresh_request(false, &mut due), "未请求 ⇒ 不生效");
        assert_eq!(due, Some(12_345), "未请求刷新时下一次轮询时刻不得被改写");
        // ② 请求刷新 ⇒ 下一次轮询提前到**下一拍**（`poll_due(_, None)` 恒真）
        assert!(apply_refresh_request(true, &mut due), "请求 ⇒ 生效");
        assert_eq!(due, None, "必须把 next_poll_ms 置 None，否则「补发 GET」是空话");
        // ③ 已经是 None（首拍 / 已在途）⇒ 仍然报告"生效"（调用点可计数），且不改语义
        let mut already = None;
        assert!(apply_refresh_request(true, &mut already));
        assert_eq!(already, None);
        // ④ 对偶：`None` 必须真的让 `poll_due` 判真（判据与 channel.rs 同源，不另立一份）
        assert!(crate::channel::poll_due(0, None), "None = 首拍即轮询");
        assert!(!crate::channel::poll_due(0, Some(1)), "未到期 ⇒ 不轮询");
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

    /// 裁定 3 的**调用点**哨：失败分支必须把本地合成回执**送进**唯一分派点（只造不送 = 屏上
    /// 依旧什么都不发生）。
    ///
    /// 判据本体（"写端点才合、字段取什么值"）在 `control_route::tests` 与
    /// `state::tests::transport_failure_hands_back_a_local_receipt_for_inflight_writes`；
    /// 本条守的是**这一行的存在**。
    ///
    /// # 能力边界（**如实登记，不得高估**）
    ///
    /// 源码扫描证明的是"这一行在源码里"（删掉 ⇒ 红），**不是**"它在运行期被执行过" ——
    /// 离屏自检里**没有任何写意图**（T-3 门禁要求如此：未确认 = 零写请求），故这条路径在
    /// 进程级用例中**不可达**。运行期的存在性由**结构**保证：该分支**只有**这一条路径、
    /// 且 `record_transport_failure_with_receipt` 带**显式** `#[must_use]` ⇒ 漏消费即
    /// `unused_must_use` 编译告警（本仓判据 = 零警告；该属性是**实测**加上去的：
    /// `Option` 本身在本工具链**不触发**该告警）。
    ///
    /// **改什么会让本条变红**（**已实测**，见报告「探针 4」）：删掉
    /// `self.apply_route(decision);` ⇒ 第 2 条断言红（同一改动还会报 `unused_must_use` 告警）。
    #[test]
    fn transport_failure_branch_dispatches_the_receipt_it_built() {
        const SRC: &str = include_str!("app.rs");
        /// 「就近」的窗口宽度（字符）：取用点与送入口之间隔着的就是那个 `if let Some(..) {`。
        const NEAR_WINDOW: usize = 400;
        // 只扫**生产段**（测试段自身含同样的字面量，会自证失真 —— 本项目踩过"扫描器失真"）。
        let prod = SRC
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .expect("app.rs 应能切出生产段");
        assert_ne!(prod.len(), SRC.len(), "未切出生产段：扫描器失真，本用例必须响亮失败");
        // 判据必须**就近**：`self.apply_route(decision);` 在 `absorb_console` 的成功路径上
        // 也有一处（同一行文本）—— 只查"全文含有"会被**那一处**满足，探测力归零
        // （B3-2b-2 整改实测踩过：探针 4 第一版**没红**）。
        let at = prod
            .find("record_transport_failure_with_receipt(epoch_ms)")
            .expect("失败分支必须取用本地合成回执（否则控制通道挂掉时屏上什么都不发生）");
        let tail = &prod[at..];
        let near = &tail[..tail.len().min(NEAR_WINDOW)];
        assert!(
            near.contains("self.apply_route(decision)"),
            "合成回执**只造不送**：取用点之后 {NEAR_WINDOW} 字符内没有送进唯一分派点 ⇒ 上屏路径没走到"
        );
    }

}
