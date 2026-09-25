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
//! TT-12「**任何**触摸事件重置」（UI §4.3；偏差 **SH5**）**没有装配期的挂号动作** ——
//! 它就是输入泵的一步：[`apply_touch_snapshot`] 把 evdev 快照交给 `Indev` 时，若
//! `pressed` 就顺带调一次 [`Shell::note_activity`]。详见该函数的文档。
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
use crate::config::{CliConfig, Rotate};
use crate::console::{ConsoleClient, ConsoleClock, ConsoleResult};
use crate::control_route::{
    audit_query_string, log_query_string, p3_connected, route, ControlIntent, RawPayload,
    RouteDecision,
};
use crate::lvgl::display::{Display, Rotation};
use crate::lvgl::indev::{Indev, TouchSnapshot};
use crate::lvgl::obj::Obj;
use crate::screen::{Blitter, MemorySink, PixelSink};
use crate::state::{self, ChannelStatus, ControlState, DisplayState, Freshness};
use crate::ui::components::{Toast, ToastTone};
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

/// CLI 的 [`Rotate`] → 薄层的 [`Rotation`]（**B4a**）。
///
/// # 为什么是"两个枚举 + 一处映射"（而不是让 `config.rs` 直接用 `lvgl::display::Rotation`）
///
/// unsafe 边界纪律（`lvgl/mod.rs` 纪律 1）：`config.rs` 位于 `lvgl/**` **之外**，不得触碰
/// `lvgl_sys`。而 [`Rotation`] 的 `to_sys()` 要写 C 侧枚举常量 ⇒ 它必须留在 `lvgl/**` 内。
/// 两边各有一个枚举、由本函数做**唯一**一次映射，于是"CLI 取值集合"与"LVGL 取值集合"的
/// 耦合点收敛到一处；两边的 `match` 都是**穷尽匹配**，任一侧增删取值都在这里**编译期**暴露。
///
/// **改什么会让本条变红**：把任一分支映射成别的角度（`rotation_of(Deg90) == Rotation::Deg180`）
/// ⇒ `app::tests::cli_rotate_maps_to_the_thin_layer_rotation` 逐值断言即红。
pub fn rotation_of(r: Rotate) -> Rotation {
    match r {
        Rotate::Deg0 => Rotation::Deg0,
        Rotate::Deg90 => Rotation::Deg90,
        Rotate::Deg180 => Rotation::Deg180,
        Rotate::Deg270 => Rotation::Deg270,
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
    /// **LVGL 定容池余量快照**（B4a；设计 §10 / §14 风险 **R-24** 的量化口）。
    ///
    /// ⚠️ 读的是 `LV_MEM_SIZE` 那一块 **1 MiB 定容池**（对象树 / 样式 / 定时器 / 事件项），
    /// **不含**绘制缓冲 —— 后者由 `display::AlignedBuf` 走**系统堆**（见
    /// [`crate::lvgl::MemStats`] 的边界说明）。真机（fbdev）同样可读，判据一致。
    pub mem: crate::lvgl::MemStats,
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

/// **一次成功路径回执的记账分派**（**纯函数**，可测）：这条回执**要不要**由状态层弹 Toast。
///
/// # 判据 = 这条回执**有没有别的上屏通道**
///
/// **写端点**（`RouteDecision::ConfigApply` / `RouteDecision::InterlockResult`）的回执由
/// [`App::apply_route`] 送进 P2 / P4 的 `show_result` —— **页面自己 `show_toast` 弹一条**。
/// 状态层若照旧再弹一条（[`state::ControlState::record_response`] 对**一切非 `Ok` 的码**与
/// **任何非空 `message`** 都弹），同一拍就会有**两个** Toast 对象同挂 `lv_layer_top()`、
/// 同坐标（`Dimens::TOAST_X/Y`）同文案 ⇒ 违 UI §7.2「同一时刻仅 1 条」。
/// 生产高频可达：P2 字段校验被拒 / P4 前置条件被拒。
///
/// **PM 裁定（B3-2c 整改 · 阻塞项）**：**页面负责上屏、状态层只记账** ⇒ 写端点走
/// [`state::ControlState::record_response_without_toast`]。
///
/// **其余决策**（读端点：`Config` / `Logs` / `LogsTargets` / `Audit` / `AuditOps`）的回执
/// **没有**页面上屏通道 ⇒ 保留状态层兜底 Toast（否则退化成"屏上什么都不发生"，违 §2.6）。
/// 判据**按决策而非按端点**写：将来新增写端点若不慎让它落到"无页面上屏通道"的新决策上，
/// 会走**保留**分支（多一条 Toast 是噪声，少一条是静默）—— 取 fail-visible 的那一侧。
///
/// 抽成自由函数（同 [`apply_refresh_request`] 的理由）：`App` 的构造需要 LVGL 会话，
/// 纯逻辑用例够不着，而这条分派**必须能红的回归**（回退成 `record_response` = 同拍双 Toast，
/// 屏上表现为"两条一模一样、叠在同一坐标"的 Toast，肉眼分不出是 bug）。
/// 回归：`ui/tests.rs::write_receipt_is_shown_by_the_page_and_not_by_the_state_layer`。
pub fn record_receipt(
    control: &mut state::ControlState,
    decision: &RouteDecision,
    resp: &mupc_display_proto::ControlResponse<RawPayload>,
    epoch_ms: u64,
) {
    match decision {
        RouteDecision::ConfigApply(_) | RouteDecision::InterlockResult(_) => {
            control.record_response_without_toast(resp)
        }
        _ => control.record_response(resp, epoch_ms),
    }
}

/// 传输层失败的**上屏文案**（**纯函数**，可测）。
///
/// - 有**专属出路**的错误（今仅 `RetryWindowExpired`，判据在
///   [`state::console_error_text`]）⇒ 用 `ui/**` 的专属串；
/// - 其余 ⇒ 回落既有通用兜底 [`state::TRANSPORT_FAIL_TEXT`]（「操作失败」）。
///
/// **为什么不在这里自造串**：上屏文案的唯一真源在 `ui/**`（码表静态网只扫那 13 个文件）；
/// 本函数只做**二选一**，一个字面量都不产生。
///
/// # 它**不是**"三条失败路径的唯一分派点"（B3-2c 整改 重要 4 · 订正）
///
/// 本函数**只**被三条失败路径里的**第 ② 条**调用 —— [`App::begin_write_intent`] 里
/// `ConsoleClient::begin_write` **自身发起失败**那一支。另两条各有其文案来源，**不经此处**：
///
/// | 路径 | 落点 | 文案来源 |
/// |------|------|----------|
/// | ① 写意图在途被丢弃（`is_busy()`） | [`state::ControlState::push_toast`] | `p4_interlock::TEXT_OP_BUSY`（页面既有串） |
/// | ② `begin_write` 自身失败 | [`state::ControlState::record_transport_failure_with_text`] | **本函数** |
/// | ③ 回执形态 / 解码不符（`route` 的 `Err`，**写端点**） | [`state::ControlState::record_transport_failure_with_receipt`] → **页面** `show_result` | 合成回执的 `message` = 既有 §3.6 串「操作失败」（**不经本函数**：该错的类型是 [`control_route::RouteError`]，不是 [`crate::console::ConsoleError`]） |
/// | ③′ 同上（**读端点**） | [`state::ControlState::record_transport_failure`]（无在途 ⇒ 机制内部回落） | 通用兜底 `TRANSPORT_FAIL_TEXT` |
///
/// （③ / ③′ 的这一句 [`App::absorb_console`] 自己也明写。此前本行写"三条失败路径出 Toast 时的
/// 唯一分派点"，与同文件的实现和注释**自相矛盾**；③ 的页面通道由 **PM 裁定 2（2026-09-16，
/// SH20）** 确立 —— 写端点的失败反馈落在被弹层遮住的 app Toast 上等于屏上什么都不发生。）
///
/// 抽成自由函数（同 [`apply_refresh_request`] 的理由）：`App` 的构造需要 LVGL 会话，
/// 纯逻辑用例够不着，而这条分派**必须有能红的回归**（写错 `unwrap_or` 的方向即静默降级）。
pub fn transport_failure_text(e: &crate::console::ConsoleError) -> &'static str {
    crate::state::console_error_text(e).unwrap_or(crate::state::TRANSPORT_FAIL_TEXT)
}

/// 本拍 **app 层 Toast** 应呈现的样子（**纯函数**，可测）：`Some(文案)` = 显示该文案，
/// `None` = 隐藏。
///
/// # 它是三条失败路径的唯一上屏出口（B3-2c）
///
/// 接线层有三条失败路径原先**只落 stderr**、屏上无任何反馈（违 §2.6「降级可见」）：
///
/// 1. 写意图在途被丢弃（[`App::begin_write_intent`] 的 `is_busy()` 分支）—— 记一条
///    [`state::ControlState::push_toast`]（「操作进行中」）；
/// 2. `begin_write` **自身**发起失败 —— [`state::ControlState::record_transport_failure_with_text`]；
/// 3. 回执**形态 / 解码不符**（[`crate::control_route::route`] 的 `Err`）——
///    **写端点**：走**页面通道**（合成回执 ⇒ 页面 `show_result`，PM 裁定 2 / SH20），
///    app 层这一格子**不再**被写；**读端点**（无在途）：仍落
///    [`state::ControlState::record_transport_failure`]。
///
/// ⇒ 落在 [`state::ControlState`] 的**同一个** Toast 格子里的只有 ①②③′ 三条 ⇒ 屏上只需要
/// **一个**消费者（就是本函数 + [`App`] 持有的那个 `Toast`）。文案一律取自 `state.rs` 的既有映射
/// （**不自造上屏字**）。
///
/// # 生命周期
///
/// 3 s（[`state::TOAST_TTL_MS`] / UI §7.2），**时钟由调用方注入**（`epoch_ms`）⇒ 离屏可确定性
/// 复现；过期即返回 `None`（屏上那条随之隐藏）。
///
/// **改什么会让本条变红**：去掉 `is_expired` 过滤 ⇒
/// `app::tests::failure_paths_reach_the_app_toast` 的「3 s 到期 ⇒ 隐藏」断言红；
/// 恒返回 `None` ⇒ 三条路径的断言**全红**。
pub fn toast_view(control: &ControlState, epoch_ms: u64) -> Option<&str> {
    control
        .toast()
        .filter(|t| !t.is_expired(epoch_ms))
        .map(state::ToastRecord::text)
}

/// 把 [`toast_view`] 的结果**落到那一个 app 层 Toast 对象上**（**自由函数**，B3-2c 整改
/// **重要 1**）。
///
/// - `Some(文案)` ⇒ 就地换文本 + 切可见；
/// - `None` ⇒ 只切可见（**不清文本** —— 隐藏的对象不参与命中与绘制，文本留作诊断）。
///
/// # 为什么把它从 `App::sync_toast` 里抽出来（评审实测的覆盖缺口）
///
/// 这是**读 evdev / 推进各客户端 / 渲染 6 页**之外，`App::tick` 里**每拍都会跑**的一段
/// LVGL 写操作。而 `App::tick` 只在真实二进制里跑（`App` 的构造需要 LVGL 会话 + 控制通道
/// 客户端）⇒ 本仓的**离屏链路看不到它**：在 `App::sync_toast` 首行插一次
/// `Obj::add_style`（`Obj::add_style` **只增不删** ⇒ 样式表无界增长、对象数纹丝不动）
/// ⇒ 整改前实测 **354 + 6 + 6 全绿、0 failed**。
///
/// `shell_chain` 里那条"双零增长"网盖不到这里：它推的是 `Shell::tick`，而 `Shell::tick`
/// **不渲染页面、也不碰 app 层 Toast**。抽成本函数后，`ui/tests.rs::ui_chain` 可以**直接
/// 调它 N 次**并断言 `PROBE_MOUNTS` / `PROBE_STYLE_ATTACHES` **双零增长** —— 被断言的那个
/// 函数体与生产跑的**是同一个体**（`App::sync_toast` 只有一行转调）。
///
/// ⚠️ **能力边界（如实登记）**：本函数**不碰** `App::sync_toast` 的**调用点**（`App::tick`
/// 的 ⑦ 段）—— "每拍都调"这件事在离屏链路里观测不到，由**源码哨**
/// `app::tests::app_toast_is_built_once_and_synced_from_tick` 钉住（它断言 `tick` 的函数体里
/// 出现 `self.sync_toast(`，且 `sync_toast` 转调本函数）。两者合起来才是完整的网。
///
/// **改什么会让本条变红**（**已实测**）：在本函数里插任何一次
/// `toast.obj().add_style(..)` / `Obj::create` ⇒ `ui_chain` 的双零增长断言当场红。
pub fn sync_toast_view(toast: &Toast, view: Option<&str>) {
    match view {
        Some(text) => {
            toast.set_text(text);
            pages::set_visible(toast.obj(), true);
        }
        None => pages::set_visible(toast.obj(), false),
    }
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
    /// **app 层的唯一 Toast**（B3-2c；挂 `lv_layer_top()`）—— 三条失败路径的上屏出口。
    ///
    /// # 为什么要它（原缺口）
    ///
    /// `ControlState` 的 Toast 记录（纯逻辑）在 B3-1 就有，但**当时没有任何生产消费者**：
    /// P2 / P4 的上屏 Toast 是**页面自己** `show_toast` 新建的对象（只服务"操作回执"这条
    /// 路径），而「写意图被丢弃 / `begin_write` 自身失败 / 回执解码失败」三条**不经页面**
    /// ⇒ 只落 stderr，屏上什么都不发生（违 §2.6「降级可见」）。
    ///
    /// # 形态（PM 裁定，UI 文档附录 A.8）
    ///
    /// **启动时建一次、运行期只切可见性 / 换文本** —— 绝不在 `tick` 或 LVGL 事件回调里
    /// 建 / 删对象（本仓已有"回调内建删对象 = UAF 级"的先例与断言）。更新点**只有**
    /// [`App::sync_toast`]，而它只在 `App::tick` 里被调。
    ///
    /// ⚠️ **遮挡登记（B3-2c 整改 重要 3）**：本 Toast 与 P2 / P4 的确认弹层**同挂
    /// `lv_layer_top()`**，而弹层**建得晚** ⇒ 弹层打开期间它被面板盖住 + 遮罩压暗，
    /// **用户看不到**（几何实测值 / 可达路径 / 最小改法选项见 `ui/shell.rs` 偏差表
    /// **SH20** —— 本仓"偏差 / 缺口"的单一登记处）。
    ///
    /// **PM 裁定（2026-09-16，UI 文档附录 A.8）**：① **不挪位置、不改版式**；② **写路径**的
    /// 失败反馈（含 `route` 的形态 / 解码 `Err`）**统一走页面通道** ⇒ 本 Toast 在生产上只服务
    /// **读端点**的失败；③ 读端点残余（本 Toast 在弹层打开期间被遮）**接受** —— 读通道降级
    /// 另有可见通道（页眉通道胶囊 + EDGE-03 整屏降级），本 Toast 属**补充**。
    ///
    /// ⚠️ **`Toast` 组件自带的 `expires_at`（`Toast::expires_at` / `is_expired`）在 app 这条
    /// 路径上是死字段 —— 不要拿它判断"该不该隐藏"**（B3-2c 整改 · 建议 1）：它的起点是
    /// **装配时刻**（`Toast::new` 里的 `Instant::now()`），此后**永不刷新** ⇒ 它早在启动后
    /// 3 s 就已过期，而 app 层的可见性判据完全在别处 —— [`state::TOAST_TTL_MS`] +
    /// [`state::ToastRecord::is_expired`]（时钟是每拍注入的 `epoch_ms`，见 [`toast_view`]）。
    /// 误调 `self.toast.is_expired(now)` 会恒得 `true` ⇒ 每次失败都"立即隐藏"（静默不显）。
    /// 生产路径**不调用**这两个方法；只读它的离屏用例（若将来要写）须自建 `new_at` 起点。
    toast: Toast,
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
    /// ⚠️ **订正（B3-2c 整改 重要 2）**：此前本行（以及 [`App::begin_write_intent`] 的
    /// 同款注）写「`ControlState` 的 toast **无页面消费者** ⇒ 屏上无提示」「均待页面补
    /// Toast 入口」——**B3-2c 起已不成立、且与本文件自身相反**：
    /// ① **app 层 Toast 已是消费方**：[`App::toast`] 装配期建一次，[`toast_view`] +
    ///    [`App::sync_toast`] 每拍把 [`ControlState::toast()`] 的记录落到屏上（三条失败
    ///    路径的出口；见 `app.rs` 模块头三条路径 + `app::tests::failure_paths_reach_the_app_toast`）
    ///    ⇒ 若本分支真的被走到，那条「操作进行中」**会上屏**（页面自己的 `show_toast` 只是
    ///    另一条独立出口，本页 P4 的「操作进行中」就是它建的）；
    /// ② 真正的事实是**生产不可达**：提交中两页的按钮均已 disabled（P2 `refresh_actions` 的
    ///    `usable = available && !submitting` ⇒ `save` / `reset` 同灰；P4 `op_state` 的
    ///    `busy` ⇒ 两按钮同灰）⇒ 在途期间用户**无法**再次触发确认回调 ⇒ 队列里不会有写意图。
    /// ⇒ 「屏上无提示」是**走不到**的结果，**不是没有上屏出口**；本路径按**防御性记账**保留
    /// （**只计数** + 照旧压那条 toast，将来若可达即自然可见）。
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
        // 屏旋转（B4a）：在此施加 **CLI 解析出来的**旋转（薄层能力自 B4a 具备）。
        // 必须在建**任何**对象之前落定 —— `update_resolution()` 会把 screen / 各图层的矩形
        // 换成互换后的逻辑宽高，晚于外壳装配则已建控件仍按旧版式定位。
        // 绘制缓冲**不必重建**：`lv_display_set_buffers` 用的是**未旋转**的原始尺寸。
        // ⚠️ **接线保留、但 CLI 侧已不放行非 0**（B4a 整改「重要 2」）：`CliConfig::parse`
        // 对非 0 硬错误 ⇒ 生产上 `cfg.rotate` 恒为 `Deg0`、本行恒施加 `Rotation::Deg0`。
        // 之所以不把这条链路删掉：**sink 侧像素旋转实现后摘除那道 guard 即可**（无需重建）。
        // 只换逻辑分辨率、像素面不动 = **主动画错版式**，这正是 CLI 侧 fail-fast 的理由。
        display.set_rotation(rotation_of(cfg.rotate));
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
        // TT-12「**任何**触摸事件重置」（UI §4.3；偏差 **SH5**）**不需要在这里挂号** ——
        // 它的唯一落点是输入泵 [`apply_touch_snapshot`]（喂快照时就地调
        // [`Shell::note_activity`]），而那条路**与 LVGL 的命中结果无关**。此处 `indev`
        // 已建好、尚未喂过任何数据。
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

        // **app 层 Toast**（B3-2c）：**装配期建一次**，挂顶层图层；`tick` 只切可见性 / 换文本。
        //
        // 建好即隐藏 —— 初始文案取既有通用兜底（`state::TRANSPORT_FAIL_TEXT`，**不自造串**），
        // 它只是"占位"，真正上屏的文案每次都由 [`App::sync_toast`] 写入。
        // 图标取 P4 的失败字形（`!`；UI 写 `✕` 但 U+2715 缺字，见该常量注释）。
        //
        // ⚠️ **建点唯一**：本行是本文件生产段唯一的 `Toast::new` 调用（判据 =
        // `app::tests::app_toast_is_built_once_and_synced_from_tick` 的计数断言）—— 挪进
        // `tick` / 事件回调 = 每次失败都新建对象（定容池迟早耗尽，且回调内建删对象属 UAF 级）。
        let toast = Toast::new(
            &crate::lvgl::widgets::layer_top()
                .map_err(|e| StartupError::Lvgl(format!("顶层浮层不可用（app Toast）：{e}")))?,
            ToastTone::Failure,
            crate::ui::pages::p4_interlock::ICON_FAIL,
            state::TRANSPORT_FAIL_TEXT,
        )
        .map_err(|e| StartupError::Lvgl(format!("app Toast 装配失败：{e}")))?;
        crate::ui::pages::set_visible(toast.obj(), false);

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
            toast,
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
                // U-73（§15.6.2 ③ / §15.11 #10）：版本不匹配**另记**（粘性）—— 它同时
                // 也是一次"本拍失败"，故上面那句 record_fail 照旧（两件事、两份记账）。
                // 归一逻辑在 `channel.rs`（错误分类的真源），此处只做转交。
                if let Some((got, expected)) = crate::channel::version_mismatch(&e) {
                    self.state.record_incompatible(got, expected, epoch_ms);
                }
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
    /// ⚠️ **订正（B3-2c 整改 重要 2）**：那条 Toast 落在 [`ControlState::push_toast`]，
    /// 而它的**上屏出口是 app 层 Toast**（[`App::toast`] + [`toast_view`] / [`App::sync_toast`]，
    /// 每拍一次）—— **不是"无消费者"**。本分支之所以看不到提示，只是因为**生产不可达**
    /// （提交中两页按钮已 disabled ⇒ 防御性分支）。判据与理由见 [`App::write_intents_dropped`]。
    fn begin_write_intent<P: serde::Serialize>(
        &mut self,
        ep: ConsoleEndpoint,
        op: &str,
        payload: &P,
        epoch_ms: u64,
    ) {
        debug_assert!(ep.is_write(), "写意图必须落在写端点上");
        if self.console.is_busy() {
            // ⚠️ **此处只计数不额外上屏**（B3-2c 整改 重要 2 订正）：`ControlState` 的 toast
            // **有** app 层消费者（`App::sync_toast`，每拍一次）⇒ 下面那条 `push_toast`
            // 照旧**会上屏**；本分支看不到提示的原因只是**生产不可达**（提交中两页按钮已
            // disabled），**不是**"没有上屏出口"。
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
                // **B3-2c 收口**：本路径原先只有 stderr（无声失败）。现在与另两条失败路径
                // 同走 **app 层 Toast**（`control.push_toast` → `App::sync_toast`），
                // 文案按 [`transport_failure_text`] 分派：有专属出路者（`RetryWindowExpired`）
                // 用 `ui/**` 的专属串，其余回落既有通用兜底（**不自造新串**）。
                // 可达性（如实）：`begin_write` 自身失败近乎结构不可达（写端点 + 已拼好的载荷）。
                self.set_submitting(ep, false);
                self.control
                    .record_transport_failure_with_text(epoch_ms, transport_failure_text(&e));
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
                // 传输失败（连接 / 超时 / 非 200 / 解码）：记入 `ControlState`（清在途 +
                // 失败时刻）；另把"提交中"复位，否则按钮永久禁用。
                //
                // **上屏只剩一条**（B3-2c 整改 重要 4）：写端点在途时**不**再压 app 层
                // 「操作失败」Toast —— 该次失败已由下面那条合成回执经**页面**的
                // `show_result` 上屏（页面 Toast），两条同拍会违 UI §7.2「同一时刻仅 1 条」；
                // 无在途（读端点）时才由 app 层兜底 Toast 承担。判据见
                // [`state::ControlState::record_transport_failure_with_receipt`]。
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
                // **【PM 裁定 2 · 2026-09-16 · SH20 收口】写端点改走页面通道**。
                //
                // 回执**形态 / 解码不符**这条出口原先只推 `ControlState` 的 app 层兜底 Toast
                // （B3-2c），而**那条 Toast 在确认弹层打开期间根本不可见**（几何实测与理由：
                // `ui/shell.rs` 偏差 **SH20** —— 两者同挂 `lv_layer_top()`、弹层**建得晚**
                // ⇒ 绘在其上；而**页面** Toast 建得比弹层更晚 ⇒ 绘在弹层**之上**，可见）。
                // 写端点的路由错误恰恰**落在弹层打开的那段时间**（P4 失败不关弹层；
                // P2 关弹层后亦有页面通道）⇒ 走 app Toast 等于**屏上什么都不发生**（违 §2.6）。
                //
                // 修法 = **复用传输失败那一套既有机制**（见上一条 `Err` 分支与
                // [`crate::control_route::transport_failure_decision`]）：由 `ControlState`
                // **本地合成**一条 `ControlCode::Unavailable` 回执（`message` = 既有 §3.6 串
                // 「操作失败」⇒ **零新增上屏字**、`request_id` = 该次在途 id、`at_ms` = 本地
                // 时钟、`duplicate = false`、`audit_id = None`、`applied = None`、
                // `field_errors = []`）⇒ 经 [`App::apply_route`] 送对应页的 `show_result`。
                //
                // **读端点不合成**（其降级出口是页眉通道胶囊 / EDGE-03 整屏降级）⇒
                // `record_transport_failure_with_receipt` 内部回落 app 层兜底 Toast ——
                // 该残余被弹层遮挡一事 **PM 裁定 3 = 接受**（登记于 SH20）。
                //
                // ⚠️ 文案**不**走 [`transport_failure_text`]：本条错的类型是
                // [`RouteError`]（形态 / 解码），不是 `ConsoleError`，没有专属出路。
                // ⚠️ 合成回执是**本地合成、非服务端回执**（不得冒充服务端判决，理由逐条见
                // `control_route::transport_failure_decision`）。
                self.route_errors += 1;
                let ep = self.control.inflight().map(|i| i.endpoint);
                if let Some(decision) = self.control.record_transport_failure_with_receipt(epoch_ms)
                {
                    self.apply_route(decision);
                }
                if let Some(ep) = ep {
                    self.set_submitting(ep, false);
                }
                eprintln!("[mupc-local-display] {e}（回执已丢弃，不冒充成功）");
                return;
            }
        };
        // 回执摘要（写操作才带信封）：清在途 + **按"这条回执有没有页面上屏通道"决定**要不要
        // 在状态层再弹一条 Toast（判据与理由见 [`record_receipt`]）。
        // 查询载荷不是 `ControlResponse` ⇒ 走 `finish()` 清在途（不记摘要）。
        match outcome.response() {
            Some(resp) => record_receipt(&mut self.control, &decision, resp, epoch_ms),
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

    /// 把 [`state::ControlState`] 的 Toast 记录**落到屏上**（B3-2c；三条失败路径的出口）。
    ///
    /// # 形态（裁定：**只切可见性 / 换文本**）
    ///
    /// 判据在纯函数 [`toast_view`]（本拍该显什么），本函数只做**机械应用**：
    /// 到期 ⇒ 隐藏；有文案 ⇒ 写文本 + 显示。**不建不删任何 LVGL 对象** ⇒
    /// 可安全放在 `tick`（`Toast::close` 之类的析构只在 `App` 析构时发生）。
    ///
    /// # 为什么必须在 `tick`（不能在事件回调）
    ///
    /// LVGL 事件回调运行在**派发帧**内；在其中增删对象会让正在派发的对象树失效
    /// （本仓对 `p3_logs::set_targets` 有同款明令）。这条路径全程只有 `lv_label_set_text`
    /// 与 `lv_obj_add/clear_flag`，即便如此也**只从 `tick` 调**（保持单一更新点）。
    ///
    /// # 调用点
    ///
    /// `App::tick` ⑦ 段，每拍一次（`app::tests::app_toast_is_built_once_and_synced_from_tick`
    /// 以源码哨钉住调用点存在）。
    ///
    /// # 函数体在 [`sync_toast_view`]（B3-2c 整改 **重要 1**）
    ///
    /// 本函数只做两件事：**判定**（[`toast_view`]）+ **转调**（[`sync_toast_view`]）。
    /// 真正的 LVGL 写操作全在那个自由函数里 —— 这样 `ui/tests.rs::ui_chain` 能**直接调它
    /// N 次**并断言双零增长（`App::tick` 在离屏链路里跑不到，见该函数的说明）。
    /// **不要**把 `set_text` / `set_visible` 搬回本函数：搬回来 = 那段代码重新脱离零增长网。
    fn sync_toast(&mut self, epoch_ms: u64) {
        // 到期即清（3 s；清掉后下一次 `toast_view` 自然返回 `None` ⇒ 屏上那条隐藏）。
        self.control.expire_toast(epoch_ms);
        sync_toast_view(&self.toast, toast_view(&self.control, epoch_ms));
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

/// **把一次 evdev 快照投进输入管线**（TT-12「**任何**触摸事件重置计时」；UI §4.3 / 偏差 **SH5**）。
///
/// **这是该能力的唯一机制、也是唯一真源**：喂快照（[`Indev::feed`]）+ 若 `pressed` 则
/// [`Shell::note_activity`]。抽成自由函数是为了让"生产跑的那个体"与"离屏用例断言的那个体"
/// **是同一个**（`ui/tests.rs::shell_chain` 的 ④″ 段直接调本函数；否则"把这一句删掉"这类
/// 破坏在离屏链路上**没有任何网** —— 本项目 B3-2c 的 Toast 调用点踩过同款）。
///
/// # 为什么挂在**快照**上，而不是给 `Indev` 挂设备级按下回调
///
/// UI §4.3 的原文是「**任何**触摸事件重置计时」。两条候选都不够：
///
/// 1. **对象级**（外壳根挂 `PRESSED`）：LVGL **默认不上冒**（`lv_obj_event.c:391` 的
///    `event_is_bubbled` 要求链上每层自带 `LV_OBJ_FLAG_EVENT_BUBBLE`），且
///    `lv_indev_search_obj`（`vendor/lvgl/src/indev/lv_indev.c:618`）取"命中的**最深**可点对象"
///    —— `lv_obj` 构造时**默认 `CLICKABLE`**（`lv_obj.c:584`）、三区又恰好铺满画布 ⇒
///    真实按压永远落不到外壳根上；
/// 2. **设备级**（`lv_indev_add_event_cb` 挂 `LV_EVENT_PRESSED`）：**在禁用态控件上不成立** ——
///    `lv_indev.c:1339` 的 `is_enabled = !lv_obj_has_state(indev_obj_act, LV_STATE_DISABLED)`
///    把 `send_event(LV_EVENT_PRESSED, …)`（`:1342`）整个包住，而 `lv_obj_hit_test`
///    **不排除** `DISABLED`、`lv_indev_search_obj` 照样返回那个禁用对象 ⇒ **回调根本不触发**
///    （实测：120×120 且置 `DISABLED` 的子对象，按其中点，回调计数 `2 → 2`）。生产命中面真实
///    存在：`p2_config.rs`（无改动时「保存」置灰）、`components.rs`（步进器越界禁用）、
///    `p4_interlock.rs`。
///
/// evdev 快照是**真实输入源**，与 LVGL 的命中测试**无关** ⇒ 天然覆盖**禁用态控件 /
/// `lv_layer_top()` 上的弹层与 Toast / 空白**。**幂等**：`pressed` 跨多拍重复到达只会重复
/// 置位（[`Shell::note_activity`] 只写一个 `Cell`），不会有副作用累积。
///
/// # 边界（如实）
///
/// - **不改** LVGL 的命中 / 派发语义：`indev.read()` 仍照常把 `PRESSED` 投给命中对象
///   （禁用对象照旧收不到）—— 本条只负责"计时器知道有人在按"；
/// - 触摸**不可用**时（`touch` 为 `None`）本函数根本不会被调用 ⇒ 计时只由页内 3 个控件的
///   对象级挂钩推进（EDGE-13 的降级形态）。
pub fn apply_touch_snapshot(shell: &Shell, indev: &Indev, snap: TouchSnapshot) {
    indev.feed(snap);
    // ⚠️ **只认按下**（§4.3 点名的就是它；抬手不是"用户活动"的新证据）。长按必先有按下
    // ⇒ 不会漏；同一按压跨多拍重复喂入时这里会重复置位，幂等（见上）。
    if snap.pressed {
        shell.note_activity();
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
                Some(Ok((true, snap))) => apply_touch_snapshot(&self.shell, &self.indev, snap),
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
        // ⑥′ 确认弹层打开期间暂停空闲回归（TT-13 / UI §4.3）——**B3-2c 闭合**。
        //
        // 判据的真源**就在页面上**：两页各有一个"弹层是否打开"的**生产可见**查询口
        // （`P2ConfigPage::dialog_open` / `P4InterlockPage::dialog_open`，B3-2c 新增），
        // 两者**取或**即"屏上此刻有任一确认弹层"。原先这里喂 `ControlState` 的模态谓词
        // —— 它**恒为 `false`**（该字段没有生产者），契约名存实亡（偏差 **SH2**）。
        // （`tests/control_channel.rs` 有一条源码哨禁止这个旧写法复活。）
        //
        // ⚠️ **为什么读页面而不是读 `ControlState::confirm`**：弹层的**生命周期**完全由页面
        // 掌握（`open_dialog` 建、`close_dialog` 关，且关闭**延迟到下一拍**），接线层**看不到**
        // 用户按下按钮的那一刻（事件派发期 `App` 正被 `&mut` 借走）。页面是唯一能如实回答
        // "此刻屏上有没有弹层"的地方。
        //
        // **改什么会让本条变红**：把它换回常量 `false`、或改回读 `ControlState` 的模态谓词 ⇒
        // `tests/control_channel.rs::app_feeds_modal_open_from_the_pages_production_query`
        // **当场变红**（那是**源码哨**）。⚠️ 如实标注它的**能力边界**：`--smoke` 路径里
        // 任何弹层都不会打开（T-3 门禁要求"未确认 = 零写动作"）⇒ **进程级用例观测不到**这条
        // 路径；外壳侧的**语义**（弹层打开 ⇒ 暂停计时）由 `ui/tests.rs::shell_chain` ⑤″ 段
        // 以**真弹层 + 对象级断言**证 —— 两段合起来才是完整的门禁，任一单独都不够。
        self.shell
            .set_modal_open(self.shell.p2().dialog_open() || self.shell.p4().dialog_open());
        self.shell.tick(now, &clock_text(epoch_ms));
        // ⑦ **app 层 Toast**（B3-2c）——三条失败路径的唯一上屏出口。
        //    排在最后：本拍（含 ⑤ 段）产生的 Toast 记录**当拍**就上屏，延迟 ≤0 拍。
        //    ⚠️ 只改可见性 / 文本，**不建不删**对象（见 `App` 的 `toast` 字段注）。
        self.sync_toast(epoch_ms);
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
        // 定容池余量同样在**全部泵结束后**取：此时六页控件树 + 走查期间建的临时对象
        // 都还在（峰值最有代表性）。⚠️ 不含绘制缓冲（见 `SmokeReport::mem` 的边界说明）。
        let mem = crate::lvgl::mem_monitor();
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
            mem,
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

    /// **源码哨的"去注释"视图**（B4b 整改「建议 3.4」）：剔掉**整行注释**（`//` 起首，允许
    /// 前导空白）后再交给各哨做 `matches` / `contains`。
    ///
    /// # 为什么必须有它
    ///
    /// 本文件的源码哨此前直接扫 `include_str!("app.rs")` 的**原始文本** ⇒ 把要守的那一行
    /// **注释掉**照样满足 `matches(..) == 1` / `contains(..)`，哨当场退化成摆设。
    /// **B4b 整改实测（探针 D / D′）**：把 `pump` 的那一句改成注释形态
    /// （`// apply_touch_snapshot(&self.shell, &self.indev, snap);`）后，
    /// 用**原始文本**判 ⇒ 该哨**照样全绿**（伪通过）；换成 `without_line_comments` 后
    /// ⇒ 当场红（`left: 0, right: 1`）。剔注释后"注释掉即红"。
    ///
    /// # 能力边界（如实）
    ///
    /// 只剔**整行注释** —— **块注释**（`/* … */`）与**行尾注释**（`let x = 1; // …`）不处理。
    /// 对本文件的四个哨足够：它们守的都是**独立语句**，不存在"行尾注释里带该串"的形态；
    /// 而残留的块 / 行尾注释只会让计数**偏大**（判据是 `== 1` / `!contains`）⇒ 偏大即红，
    /// 落在"响亮失败"这一侧，不会静默放行。
    fn without_line_comments(src: &str) -> String {
        src.lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

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
            // B4a：定容池快照只承载"读数"，不参与本组判定口的判据 ⇒ 取默认（全 0）。
            mem: crate::lvgl::MemStats::default(),
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

    /// **B4a**：`--rotate` 的 CLI 取值 → 薄层旋转枚举的映射（**唯一**耦合点）。
    ///
    /// **改什么会让本条变红**：把 `rotation_of` 的任一分支写错（如 `Deg90 => Rotation::Deg180`）
    /// 或把 `Rotate`/`Rotation` 的语义弄反 ⇒ 逐值相等断言即红。
    ///
    /// ⚠️ 本用例**不触碰 LVGL**（纯枚举映射），故可以安全地作为独立 `#[test]` 并行执行。
    #[test]
    fn cli_rotate_maps_to_the_thin_layer_rotation() {
        for (cli, thin) in [
            (Rotate::Deg0, Rotation::Deg0),
            (Rotate::Deg90, Rotation::Deg90),
            (Rotate::Deg180, Rotation::Deg180),
            (Rotate::Deg270, Rotation::Deg270),
        ] {
            assert_eq!(rotation_of(cli), thin, "{cli:?} 须映射为 {thin:?}");
            // 轴互换口径两边必须一致（`app.rs` 用 `Rotate::swaps_axes` 决定版式相关的分支，
            // 而物理互换由 `Rotation` 承担 —— 两者口径漂移 = 逻辑分辨率与版式解算打架）。
            assert_eq!(
                cli.swaps_axes(),
                thin.swaps_axes(),
                "{cli:?} / {thin:?} 的「是否互换宽高」口径必须一致"
            );
        }
        assert_eq!(rotation_of(CliConfig::default().rotate), Rotation::Deg0);
    }

    /// **B4a**：定容池余量的判定口 [`crate::lvgl::MemStats::has_headroom`] **能失败**。
    ///
    /// `--smoke` 的判定口若写成恒真就只是"打印"（本项目明令禁止的伪门禁）⇒ 这里逐条钉死
    /// 它判假的两个方向。
    #[test]
    fn mem_headroom_verdict_can_fail() {
        use crate::lvgl::MemStats;
        let ok = MemStats {
            total_size: 1024 * 1024,
            max_used: 1024 * 1024 / 2,
            ..MemStats::default()
        };
        assert!(ok.has_headroom(), "半池峰值 ⇒ 有余量");
        // ① 峰值打满池：再建一个对象就可能分配失败 ⇒ 必须判失败。
        let full = MemStats {
            max_used: 1024 * 1024,
            ..ok
        };
        assert!(!full.has_headroom(), "峰值打满 1 MiB 池必须判失败");
        // ② 池还没读数（未 init / 读口没接上）⇒ `total_size == 0`，必须判失败
        //    （否则"什么都没读到"会被当成"余量充足"）。
        let unread = MemStats::default();
        assert!(!unread.has_headroom(), "全 0 快照（读口没接上）必须判失败");
        // ③ 阈值边界：95 % 是开区间上界。
        let just_over = MemStats {
            max_used: 1024 * 1024 * 951 / 1000,
            ..ok
        };
        assert!(!just_over.has_headroom(), "峰值 > 95 % 即判失败（给新增对象留 5 %）");
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

    /// **三条失败路径各自都能上屏**（B3-2c：`toast_view` = `App::sync_toast` 写进 LVGL 的
    /// 那一条文案，逐字相同的取值）。
    ///
    /// 三条路径原先**只落 stderr**（屏上无反馈，违 §2.6）；现在统一由 app 层 Toast 承担。
    /// **判据在此**（纯逻辑），**调用点**由 `tests/control_channel.rs` 的源码哨 + `App::tick`
    /// 的 ⑦ 段顺序保证（`App` 的构造需要 LVGL 会话 ⇒ 进程内起不了第二条 LVGL 线程，
    /// 见 `src/ui/tests.rs` 模块头；这是本仓对"接线层不可进程内断言"的既有边界）。
    ///
    /// **改什么会让本条变红**（**已实测**）：
    /// - `toast_view` 恒返回 `None` ⇒ 三条断言全红；
    /// - 去掉 `is_expired` 过滤 ⇒ 「3 s 后仍显示」红；
    /// - 把 `record_transport_failure_with_text` 改回恒用通用兜底 ⇒ ② 红。
    #[test]
    fn failure_paths_reach_the_app_toast() {
        use crate::ui::pages::p4_interlock::{TEXT_OP_BUSY, TEXT_RETRY_EXPIRED, TEXT_TOAST_FAIL};
        const T0: u64 = 1_000;

        // ① 写意图在途被丢弃（`begin_write_intent` 的 `is_busy()` 分支）
        let mut st = ControlState::new();
        st.push_toast(TEXT_OP_BUSY, T0);
        assert_eq!(toast_view(&st, T0), Some(TEXT_OP_BUSY), "①「操作进行中」必须上屏");

        // ② `begin_write` 自身失败 —— 文案分派见 [`transport_failure_text`]
        let mut st = ControlState::new();
        let expired = crate::console::ConsoleError::RetryWindowExpired {
            op: "apply".to_string(),
            issued_at_ms: 0,
            age_ms: 30_000,
            window_ms: 30_000,
        };
        st.record_transport_failure_with_text(T0, transport_failure_text(&expired));
        assert_eq!(
            toast_view(&st, T0),
            Some(TEXT_RETRY_EXPIRED),
            "② 过期重发必须显**专属**出路文案（不是通用「操作失败」）"
        );
        // 同一分支的普通错误仍显通用兜底
        let mut st2 = ControlState::new();
        st2.record_transport_failure_with_text(
            T0,
            transport_failure_text(&crate::console::ConsoleError::Idle),
        );
        assert_eq!(toast_view(&st2, T0), Some(TEXT_TOAST_FAIL));

        // ③ 回执形态 / 解码不符（`absorb_console` 的 `route` `Err` 分支）
        let mut st = ControlState::new();
        st.record_transport_failure(T0);
        assert_eq!(
            toast_view(&st, T0),
            Some(TEXT_TOAST_FAIL),
            "③ 回执解码失败必须上屏（不再是「只有 stderr」）"
        );

        // 生命周期：3 s 内可见、到期即隐（UI §7.2 / `TOAST_TTL_MS`）
        assert!(toast_view(&st, T0 + state::TOAST_TTL_MS - 1).is_some(), "3 s 内仍显示");
        assert_eq!(
            toast_view(&st, T0 + state::TOAST_TTL_MS),
            None,
            "3 s 到期 ⇒ 屏上那条隐藏（`now >= until` 即过期）"
        );
        // 空状态 ⇒ 从不显示（不得"建好就常显"）
        assert_eq!(toast_view(&ControlState::new(), T0), None);
    }

    /// **写端点回执不压状态层 Toast**（B3-2c 整改 **阻塞项**；纯逻辑那一半）。
    ///
    /// 评审实测的双 Toast：`record_response` 先弹一条（`toast_text()` 对**一切非 `Ok` 的码**
    /// 与**任何非空 `message`** 都返回 `Some`），紧接着 `apply_route` 把**同一份回执**送进
    /// P2 / P4 的 `show_result` ⇒ **页面再弹一条** —— 两条同挂 `lv_layer_top()`、同坐标同文案
    /// （违 UI §7.2「同一时刻仅 1 条」）。生产高频：P2 字段校验被拒 / P4 前置条件被拒。
    ///
    /// **改什么会让本条变红**（**已实测**）：把 [`record_receipt`] 的写端点分支换回
    /// `control.record_response(resp, epoch_ms)` ⇒ 第 1 条断言红（状态层多出一条）。
    ///
    /// ⚠️ **口径局限（如实登记）**：本用例**只**数"状态层"这一侧 —— 它证明的是"状态层为空"，
    /// **不是**"屏上恰好一条"。合账要两半：**页面侧另有出口**由 `ui/tests.rs` 的
    /// `write_receipt_is_shown_by_the_page_and_not_by_the_state_layer` 在同一处证
    /// （P4 `show_result` ⇒ 页面 Toast 出现）。两半合起来：
    ///
    /// - 上屏出口 = 页面 Toast（1 条）
    /// - app 层 Toast（其文案由 [`toast_view`] 独取 `ControlState::toast()` ⇒ 状态层为空 ⇒ 隐藏）
    ///
    /// ⇒ **恰好一条**。
    #[test]
    fn record_receipt_keeps_the_state_layer_silent_for_write_endpoints() {
        use mupc_display_proto::{ControlCode, ControlResponse};
        const T0: u64 = 1_000;
        // 一条**生产高频**的失败回执：P4 前置条件被拒（`message` 非空 ⇒ `toast_text()` = Some）。
        let resp: ControlResponse<RawPayload> = ControlResponse::rejected(
            "rid-1",
            ControlCode::RejectedPrecondition,
            "联锁状态已变化",
            Vec::new(),
            None,
            T0,
        );
        // ① 写端点（P4 联锁写）：页面 `show_result` 已上屏 ⇒ 状态层**必须为空**。
        let mut st = ControlState::new();
        st.begin(ConsoleEndpoint::InterlockRelease, Some("rid-1"));
        record_receipt(
            &mut st,
            &RouteDecision::InterlockResult(ControlResponse::rejected(
                "rid-1",
                ControlCode::RejectedPrecondition,
                "联锁状态已变化",
                Vec::new(),
                None,
                T0,
            )),
            &resp,
            T0,
        );
        assert!(
            st.toast().is_none(),
            "写端点回执由页面 `show_result` 上屏 ⇒ 状态层不得再压一条（同拍双 Toast）"
        );
        assert!(!st.is_busy(), "记账一个不少：在途必须照清");
        assert!(
            st.last().is_some(),
            "记账一个不少：最近回执摘要照记（页面 `show_result` 与诊断都用它）"
        );
        assert_eq!(st.toast_text(), None, "（同上，取文案的那个口子也必须为空）");

        // ② 对照（**证明①不是恒真**）：同一份回执走**无页面上屏通道**的决策 ⇒ 保留兜底 Toast。
        let mut st_read = ControlState::new();
        record_receipt(&mut st_read, &RouteDecision::LogsTargets(vec![]), &resp, T0);
        assert_eq!(
            st_read.toast_text(),
            Some("联锁状态已变化"),
            "读/无页面上屏通道 ⇒ 状态层**保留**兜底 Toast（否则退化成「屏上什么都不发生」）"
        );
    }

    /// **app Toast 的装配点唯一、且不在运行期**（B3-2c 源码哨）。
    ///
    /// # 为什么需要它
    ///
    /// 「启动时建一次、运行期只切可见/换文本」是 PM 裁定（UI 附录 A.8），而**违反它的代价
    /// 在屏上完全不可见**：把 `Toast::new` 挪进 `sync_toast`（每次失败新建一个）在离屏用例
    /// 里照常"能显示"，只是 LVGL 的**定容池**（1 MB）被逐次吃光 —— 到某个时刻整屏建不出来。
    /// 本仓对同类退化（常驻对象预算 `SHELL_OBJECT_BUDGET`、`PROBE_MOUNTS`）都有判据。
    ///
    /// # 能力边界（如实登记）
    ///
    /// 源码扫描证明"**这一行在源码里**"，**不是**"运行期它没被重复执行"。
    /// 运行期的唯一性由**结构**保证：`Toast::new` 在本文件生产段**只出现一次**（本条即判据），
    /// 且 `App::toast` 是**拥有型字段**（`Drop` 时随 `App` 析构）。
    ///
    /// **改什么会让本条变红**（**已实测**）：在 `sync_toast` 里再写一个 `Toast::new(` ⇒
    /// 计数变 2 ⇒ 第 1 条红；把 `sync_toast(epoch_ms)` 的调用从 `tick` 摘掉 ⇒ 第 2 条红；
    /// 把装配点**注释掉** ⇒ 去注释后计数变 0 ⇒ 第 1 条红（B4b 整改「建议 3.4」）。
    #[test]
    fn app_toast_is_built_once_and_synced_from_tick() {
        const SRC: &str = include_str!("app.rs");
        // 归一化 CRLF：本机 `core.autocrlf=true` ⇒ 工作区是 CRLF、CI 上是 LF。切分/长度判据
        // 一律取归一化后的文本，**不得**依赖换行符（否则守卫会因换行符而误触发）。
        let src = SRC.replace("\r\n", "\n");
        let prod = src
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .expect("app.rs 应能切出生产段");
        assert_ne!(prod.len(), src.len(), "未切出生产段：扫描器失真，本用例必须响亮失败");
        // ⚠️ 判据在**去注释**后的文本上取（[`without_line_comments`]）：只数原始字符时，
        // 把 `let toast = Toast::new(` **注释掉**照样通过 —— 本哨会退化成摆设。
        let live = without_line_comments(prod);
        assert_eq!(
            live.matches("Toast::new(").count(),
            1,
            "app Toast 必须**只装配一次**（写在 `tick`/回调里 = 每次失败新建对象 ⇒ 定容池耗尽）"
        );
        // 调用点：`tick` 函数体里必须有 `self.sync_toast(`。
        //
        // ⚠️ 窗口按**字符**取（本文件是 UTF-8，`&s[..n]` 会切在多字节字符中间而 panic ——
        // 本项目"扫描器失真"的又一形态）。
        const FN_HEAD: &str = "fn tick(&mut self, now_ms: u64) {";
        let at = live.find(FN_HEAD).expect("`App::tick` 必须存在（唯一更新点）");
        let body: String = live[at + FN_HEAD.len()..].chars().take(4_000).collect();
        assert!(
            body.contains("self.sync_toast("),
            "`App::tick` 里必须调 `sync_toast` —— 否则三条失败路径产生的 Toast 记录**永不落屏**"
        );
        // 反向：同步点必须**只**做可见性 / 文本（不得在运行期建删对象），且**函数体必须转调
        // 「被零增长网罩住的那个自由函数」**（B3-2c 整改 重要 1）。
        //
        // ⚠️ **为什么判据是"转调 [`sync_toast_view`]"而不是"体内有 `set_text(`"**：
        // 整改前 `set_text` / `set_visible` 就写在本函数体里，而**没有任何离屏网盖得到它**
        // （`App::tick` 在离屏链路里跑不到 —— 评审实测：在首行插一次 `add_style` 全绿）。
        // 判据改成"转调"后，**"被断言的"与"生产跑的"才是同一个体**：那个自由函数由
        // `ui/tests.rs::ui_chain` 连推 N 拍做**双零增长**断言。
        // 把 `set_text` / `set_visible` 搬回本函数（哪怕只是留一份副本）⇒ 本条当场红。
        let sync_at = live
            .find("fn sync_toast(&mut self, epoch_ms: u64) {")
            .expect("`App::sync_toast` 必须存在");
        let sync_body: String = live[sync_at..].chars().take(1_200).collect();
        assert!(
            sync_body.contains("sync_toast_view("),
            "`App::sync_toast` 必须**转调** `sync_toast_view` —— 否则这段 LVGL 写操作重新脱离 \
             `ui_chain` 的双零增长网（评审实测：旧写法下往里插 `add_style` 全绿）"
        );
        assert!(
            !sync_body.contains("Toast::new(") && !sync_body.contains(".close()"),
            "`sync_toast` 不得建 / 删 Toast 对象（运行期对象 churn）"
        );
        // 自由函数那一侧：**只**做可见性 / 文本（不得建删对象、不得改样式表）。
        let view_at = live
            .find("pub fn sync_toast_view(toast: &Toast, view: Option<&str>) {")
            .expect("`sync_toast_view` 必须存在（`ui_chain` 的双零增长断言的**同一体**）");
        let view_body: String = live[view_at..].chars().take(1_200).collect();
        assert!(
            view_body.contains("set_text(") && view_body.contains("set_visible("),
            "`sync_toast_view` 必须就地换文本 / 切可见性（不得新建对象）"
        );
        assert!(
            !view_body.contains("Toast::new(") && !view_body.contains(".close()"),
            "`sync_toast_view` 不得建 / 删 Toast 对象（运行期对象 churn）"
        );
    }

    /// **TT-12「任何触摸事件重置」的输入泵接线哨**（B4b 整改；`ui/shell.rs` 偏差 **SH5**）。
    ///
    /// # 为什么要一个源码哨
    ///
    /// 这一条能力的**机制本体**（[`apply_touch_snapshot`]：喂快照 + `pressed` ⇒
    /// [`Shell::note_activity`]）与它的**行为**都已经有网：`ui/tests.rs::shell_chain` 的 ④″ 段
    /// **直接调本函数**（含"**禁用态控件上的按压也重置计时**"的回归锁）。但那一节是**测试
    /// 自己**调的 —— **生产**那条接线（[`App::pump`] 里的那一句）若被摘掉，离屏链路**照样
    /// 全绿**（与 B3-2c 的 Toast 调用点同款：**能力**与**接线**是两件事，评审实测过"删掉调用点
    /// 全绿"）。本哨把"生产确实走了这条路"变成可断言的事实。
    ///
    /// # 能力边界（如实）
    ///
    /// 它证明"这一行**在源码里**"，**不**证明运行期真的生效 —— 后者由 ④″ 的用例覆盖
    /// （两者合起来才是完整的网）。且 `pump` 的 evdev 分支带 `cfg(target_os = "linux")`，
    /// 离屏链路**根本跑不到** ⇒ 这里**只能**是源码哨。
    ///
    /// **改什么会让本条变红**（**已实测**）：删掉 `pump` 里的
    /// `apply_touch_snapshot(&self.shell, &self.indev, snap)` ⇒ 第 1 条红；把它**注释掉**
    /// ⇒ 去注释后同样红；让它退回裸 `self.indev.feed(snap)`（只喂不记事）⇒ 第 2 条红；
    /// 摘掉自由函数里的 `if snap.pressed` / `shell.note_activity();` ⇒ 第 3 条红。
    #[test]
    fn pump_routes_touch_snapshots_through_apply_touch_snapshot() {
        const SRC: &str = include_str!("app.rs");
        // 归一化 CRLF：本机 `core.autocrlf=true` ⇒ 工作区是 CRLF、CI 上是 LF。切分/长度判据
        // 一律取归一化后的文本，**不得**依赖换行符（否则守卫会因换行符而误触发）。
        let src = SRC.replace("\r\n", "\n");
        let prod = src
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .expect("app.rs 应能切出生产段");
        assert_ne!(prod.len(), src.len(), "未切出生产段：扫描器失真，本用例必须响亮失败");
        // ⚠️ 判据一律在**去注释**后的文本上取（见 [`without_line_comments`]）：否则把要守的
        // 那些行**注释掉**照样通过 —— 本哨（及其同款的三个哨）会当场退化成摆设。
        let live = without_line_comments(prod);
        assert_eq!(
            live.matches("apply_touch_snapshot(&self.shell, &self.indev, snap)")
                .count(),
            1,
            "生产输入泵必须**恰好**经过 `apply_touch_snapshot`（§4.3「任何触摸事件重置」；\
             删掉它 ⇒ 只有返回键 / 6 页签 / 放弃修改这 3 个控件会重置计时）"
        );
        // 反向：`pump` 里**不得**再有裸 `self.indev.feed(` —— 那正是"绕开活动记录"的老路
        // （`App::build` 里那次首拍 `indev.feed(t.snapshot())` 是**局部变量** `indev` 上的
        // 调用，与 `self.indev` 不同，不受本条约束）。
        assert_eq!(
            live.matches("self.indev.feed(").count(),
            0,
            "`pump` 不得绕过 `apply_touch_snapshot` 直接喂快照（那样按压不会被记为活动）"
        );
        // 机制本体：那一条自由函数必须**在按下时**记活动（不是"无条件记"、也不是"从不记"）。
        const FN_HEAD: &str =
            "pub fn apply_touch_snapshot(shell: &Shell, indev: &Indev, snap: TouchSnapshot) {";
        let at = live
            .find(FN_HEAD)
            .expect("`apply_touch_snapshot` 必须存在（输入泵与离屏用例的**同一体**）");
        let body: String = live[at..].chars().take(400).collect();
        assert!(
            body.contains("indev.feed(snap);") && body.contains("shell.note_activity();"),
            "`apply_touch_snapshot` 必须把「喂快照」与「记一次活动」两件事都做"
        );
        assert!(
            body.contains("if snap.pressed"),
            "活动记录必须**只在按下时**发生（抬手不是「用户活动」的新证据；§4.3 点名的是 PRESSED）"
        );
    }

    /// **失败路径的上屏文案分派**（B3-2c 收口）：有专属出路的错误用专属串，其余回落通用兜底。
    ///
    /// 这条分派原先**不存在**（所有传输层失败一律「操作失败」）⇒ `RetryWindowExpired` 的
    /// 出路指引只躺在英文 `Display` 里（`console.rs` 模块头第 6 条登记的静默语义偏差）。
    ///
    /// **改什么会让本条变红**（**已实测**）：
    /// - 把 `unwrap_or` 的两个分支对调（`console_error_text(e).or(Some(TRANSPORT_FAIL_TEXT))`
    ///   一类）⇒ 第 2 条红；
    /// - 删掉 `console_error_text` 的 `RetryWindowExpired` 分支 ⇒ 第 1 / 2 条红；
    /// - 把它改成自造字面量（不经 `ui/**`）⇒ 第 3 条红。
    #[test]
    fn transport_failure_text_routes_by_error_kind() {
        let expired = crate::console::ConsoleError::RetryWindowExpired {
            op: "apply".to_string(),
            issued_at_ms: 0,
            age_ms: 30_000,
            window_ms: 30_000,
        };
        // ① 专属文案确实取自 `ui/**`（码表网扫得到的那一份）
        assert_eq!(
            transport_failure_text(&expired),
            crate::ui::pages::p4_interlock::TEXT_RETRY_EXPIRED,
            "过期重发的出路指引必须上屏（不得落到通用「操作失败」）"
        );
        // ② 与通用兜底**不同**（否则本条退化成恒真）
        assert_ne!(transport_failure_text(&expired), crate::state::TRANSPORT_FAIL_TEXT);
        // ③ 其余错误（超时 / 忙 / 无在途）⇒ **回落**通用兜底，不得乱套专属串
        assert_eq!(
            transport_failure_text(&crate::console::ConsoleError::Idle),
            crate::state::TRANSPORT_FAIL_TEXT
        );
        assert_eq!(
            transport_failure_text(&crate::console::ConsoleError::Busy("apply".into())),
            crate::state::TRANSPORT_FAIL_TEXT
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
    /// `self.apply_route(decision);` ⇒ 第 2 条断言红（同一改动还会报 `unused_must_use` 告警）；
    /// 把 `self.apply_route(decision);` **注释掉** ⇒ 去注释后同样红（B4b 整改「建议 3.4」）。
    #[test]
    fn transport_failure_branch_dispatches_the_receipt_it_built() {
        const SRC: &str = include_str!("app.rs");
        /// 「就近」的窗口宽度（字符）：取用点与送入口之间隔着的就是那个 `if let Some(..) {`。
        const NEAR_WINDOW: usize = 400;
        // 只扫**生产段**（测试段自身含同样的字面量，会自证失真 —— 本项目踩过"扫描器失真"）。
        //
        // 归一化 CRLF：本机 `core.autocrlf=true` ⇒ 工作区是 CRLF、CI 上是 LF。切分/长度判据
        // 一律取归一化后的文本，**不得**依赖换行符（否则守卫会因换行符而误触发）。
        let src = SRC.replace("\r\n", "\n");
        let prod = src
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .expect("app.rs 应能切出生产段");
        assert_ne!(prod.len(), src.len(), "未切出生产段：扫描器失真，本用例必须响亮失败");
        // ⚠️ 窗口取在**去注释**后的文本上（[`without_line_comments`]）：否则把
        // `self.apply_route(decision);` **注释掉**照样满足 `contains` ⇒ 哨退化成摆设。
        let live = without_line_comments(prod);
        // 判据必须**就近**：`self.apply_route(decision);` 在 `absorb_console` 的成功路径上
        // 也有一处（同一行文本）—— 只查"全文含有"会被**那一处**满足，探测力归零
        // （B3-2b-2 整改实测踩过：探针 4 第一版**没红**）。
        let at = live
            .find("record_transport_failure_with_receipt(epoch_ms)")
            .expect("失败分支必须取用本地合成回执（否则控制通道挂掉时屏上什么都不发生）");
        let tail = &live[at..];
        let near = &tail[..tail.len().min(NEAR_WINDOW)];
        assert!(
            near.contains("self.apply_route(decision)"),
            "合成回执**只造不送**：取用点之后 {NEAR_WINDOW} 字符内没有送进唯一分派点 ⇒ 上屏路径没走到"
        );
    }

    /// **SH20 裁定 2**：`route` 的 `Err`（回执**形态 / 解码**不符）对**写端点**必须走**页面
    /// 通道**（复用传输失败同款的**本地合成**回执），**不得**只推 app 层兜底 Toast —— 后者在确认
    /// 弹层打开期间被面板完全遮住（几何实测与理由 = `ui/shell.rs` 偏差 **SH20**），而写端点的
    /// 路由错误**恰恰**落在弹层开着的那段时间（P4 失败按 IL11 不关弹层）。只推 app Toast 等于
    /// **屏上什么都不发生**（违 §2.6「降级可见」）。
    ///
    /// # 判据为什么落成**源码哨**（能力边界如实登记）
    ///
    /// 判据**本体**（写端点才合成 / 读端点回落 / 逐字段取值）在
    /// `control_route::tests::transport_failure_synthesizes_an_unavailable_receipt_for_write_endpoints_only`
    /// 与 `state::tests::transport_failure_hands_back_a_local_receipt_for_inflight_writes`；
    /// 而 [`App::absorb_console`] 要先建起 LVGL 会话才能构造（`src/ui/tests.rs` 模块头：全仓只有
    /// **一个** `#[test]` 能串行调起 LVGL）⇒ **运行期**这一步只能由结构保证（该分支只有一条路径 +
    /// `record_transport_failure_with_receipt` 的**显式** `#[must_use]`）。本条守的是
    /// "**这一行在源码里**"，与上面那条传输失败分支的哨同款。
    ///
    /// **改什么会让本条变红**（**已实测**，见交付报告「探针 1」）：把这一支改回
    /// `self.control.record_transport_failure(epoch_ms);`（= "只推 app Toast"）⇒ 第 1 / 3 条红；
    /// 把 `record_transport_failure_with_receipt(epoch_ms)` 或 `self.apply_route(decision)`
    /// **注释掉** ⇒ 去注释后同样红（B4b 整改「建议 3.4」）。
    #[test]
    fn route_error_branch_hands_write_failures_to_the_page_channel() {
        const SRC: &str = include_str!("app.rs");
        /// 判据窗口（**按字符**取）：本文件是 UTF-8，`&s[..n]` 按**字节**切会切进多字节字符而
        /// panic（本项目"扫描器失真"的又一形态）；`route` 的 `Err` 分支实测 ≈1 985 字符。
        const WINDOW_CHARS: usize = 2_400;
        // 归一化 CRLF：本机 `core.autocrlf=true` ⇒ 工作区是 CRLF、CI 上是 LF。切分/长度判据
        // 一律取归一化后的文本，**不得**依赖换行符（否则守卫会因换行符而误触发）。
        let src = SRC.replace("\r\n", "\n");
        let prod = src
            .split("#[cfg(test)]\nmod tests {")
            .next()
            .expect("app.rs 应能切出生产段");
        assert_ne!(prod.len(), src.len(), "未切出生产段：扫描器失真，本用例必须响亮失败");
        // ⚠️ 窗口取在**去注释**后的文本上（[`without_line_comments`]）：否则把被守的那几行
        // **注释掉**照样满足 `contains` ⇒ 哨退化成摆设。
        let live = without_line_comments(prod);
        // 锚点取 `route(&outcome)` 的分派点自身：`record_transport_failure_with_receipt` 在
        // **上面**那条传输失败分支里也有一处 ⇒ 只查"全文含有"会被**那一处**满足、探测力归零
        // （B3-2b-2 整改踩过同款坑，见 `transport_failure_branch_dispatches_the_receipt_it_built`）。
        let at = live
            .find("let decision = match route(&outcome) {")
            .expect("`App::absorb_console` 里必须有 `route` 的分派点");
        let near: String = live[at..].chars().take(WINDOW_CHARS).collect();
        assert!(
            near.contains("route_errors += 1"),
            "{WINDOW_CHARS} 字符的窗口没盖住 `route` 的 `Err` 分支 ⇒ 扫描器失真，后两条断言不可信"
        );
        assert!(
            near.contains("record_transport_failure_with_receipt(epoch_ms)"),
            "`route` 的 `Err` 必须走**页面通道**（本地合成回执）—— 否则写端点的失败反馈落在被弹层遮住的 app Toast 上（SH20 / 违 §2.6）"
        );
        assert!(
            near.contains("self.apply_route(decision)"),
            "合成回执**只造不送**：`route` 的 `Err` 分支里没有送进唯一分派点 ⇒ 页面上屏路径没走到"
        );
        // 反向：这一支**不得**再直接推 app 层兜底 Toast（写端点会与页面 Toast 同拍双条 / 被弹层遮住；
        // 读端点的兜底**由机制内部裁决**，不在这里直呼）。
        assert!(
            !near.contains("self.control.record_transport_failure(epoch_ms)"),
            "`route` 的 `Err` 分支不得直接推 app 兜底 Toast（写端点走页面通道；读端点的兜底由 `record_transport_failure_with_receipt` 内部裁决）"
        );
    }

    /// **SH20 裁定 3**（**防"一刀切"**）：读端点的 `route` `Err` **仍**走 app 层兜底 Toast
    /// —— 读端点从不入 `inflight` ⇒ **没有**合成回执可送 ⇒ 若连 app Toast 也一并去掉，就退化成
    /// 「屏上什么都不发生」（静默失败，违 §2.6）。其**被弹层遮挡**的残余 **PM 裁定 = 接受**
    /// （读通道降级另有可见通道：页眉通道胶囊 + EDGE-03 整屏降级，该 Toast 属**补充**）。
    ///
    /// # 与既有用例的关系（**如实登记，不重复计数**）
    ///
    /// ① 与 `control_route::tests` 里那条的判据**同源**（都取契约端点清单 `is_write`）——
    /// 本条的增量是**在 SH20 的上下文里**把"读端点不得走页面通道"钉住，并**同时**钉住
    /// "兜底 Toast 不得丢"这一侧的机制行为（后者原先只在 `state.rs` 的用例里、不在本文件的
    /// 接线上下文里）。
    ///
    /// **改什么会让本条变红**（**均已实测**，见交付报告「探针 2 / 探针 2′」）：
    /// - 让读端点也合成（把 `transport_failure_decision` 的 `_ => None` 改成造一条回执）
    ///   ⇒ 第 ① 段红；
    /// - 把 `record_transport_failure_with_receipt` 的"无在途"分支从 `record_transport_failure`
    ///   改成 `record_failure_state`（读端点不再给兜底 Toast）⇒ 第 ② 段红。
    #[test]
    fn read_endpoint_route_errors_still_fall_back_to_the_app_toast() {
        const T0: u64 = 1_000;
        // ① 读端点**不合成**：给它们造"写回执"会把语义接到没有 `show_result` 的页上。
        //    判据用**契约的端点清单**（`is_write`）⇒ 将来新增读端点自动落网。
        for ep in ConsoleEndpoint::ALL.into_iter().filter(|e| !e.is_write()) {
            assert!(
                crate::control_route::transport_failure_decision(
                    ep,
                    "rid-read",
                    state::TRANSPORT_FAIL_TEXT,
                    T0
                )
                .is_none(),
                "{ep:?} 是读端点 ⇒ 不得合成回执（无页面上屏出口）"
            );
        }
        // ② 无在途（读端点的生产形态）⇒ 无回执可送，**但** app 层兜底 Toast 必须留着：
        //    否则读端点失败 = 屏上什么都不发生。
        let mut st = ControlState::new();
        assert!(
            st.record_transport_failure_with_receipt(T0).is_none(),
            "无在途 ⇒ 没有「哪一次操作」可言，不合成"
        );
        assert_eq!(
            toast_view(&st, T0),
            Some(state::TRANSPORT_FAIL_TEXT),
            "读端点失败必须仍落在 app 层兜底 Toast 上（读通道降级的**补充**通道；遮挡残余 PM 已裁定接受，但**不得**因此整条去掉）"
        );
        // 对照（证明 ② 不是恒真）：同一时刻、同样是写端点 ⇒ 走页面通道、app 层这一格子**空**。
        let mut w = ControlState::new();
        w.begin(ConsoleEndpoint::InterlockRelease, Some("rid-w"));
        assert!(
            w.record_transport_failure_with_receipt(T0).is_some(),
            "写端点 ⇒ 必须交回合成回执（送页面 `show_result`）"
        );
        assert_eq!(toast_view(&w, T0), None, "写端点 ⇒ 上屏只走页面 Toast（UI §7.2 同一时刻仅 1 条）");
    }

}
