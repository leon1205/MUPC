//! # `ui/shell.rs` —— 应用外壳（开发单元 **B2c-3**）
//!
//! 设计出处：UI 设计文档 §4.1（三区固定框架）/ §4.2（六页 ≤2 次触摸导航 / `NavTab` 规格）/
//! §4.3（返回 / 超时回归主状态页）/ §7.5（触摸不可用降级）/ §8.3（EDGE-20 两种状态同显）/
//! §5.1 #1·#3·#13·#15 / §5.2（状态 × 色值矩阵）/ §5.3（页面路由实现要点）；
//! 技术设计 §5.4（页面与状态模型）/ §5.2（事件循环与不变量）。
//!
//! ## 装配结构（三区 + 顶层浮层）
//!
//! ```text
//! root（1024×768，CLICKABLE —— 「全屏输入对象」，PRESSED ⇒ 重置空闲计时）
//! ├── header (0,0,1024,72)     返回 64×64（仅 P2–P6）/ 标题 32 / 通道胶囊 / 触摸角标 / 倒计时胶囊 / 时钟 26
//! ├── content (0,72,1024,624)  6 个页根容器（**任一时刻只显一个**）
//! ├── banner (16,72,1008,120)  未保存修改提示条（h48，仅 P2 `dirty` 时可见）
//! └── nav (0,696,1024,768)     6 个 `NavTab`（170×72）
//! ```
//!
//! **弹层 / Toast 不在本文件**：P2 / P4 的 `ConfirmDialog` 与各页 `Toast` 都挂在
//! `lv_layer_top()`（见 `pages/p2_config.rs::show_dialog`），本层只提供
//! [`Shell::overlay_layer`] 作为 B3「整屏降级（EDGE-03）」的挂点。
//!
//! ## 本单元的边界
//!
//! **做**：外壳结构 / 路由 / 超时回归 / 未保存提示条 / 页眉右端（时钟 + 通道胶囊 + 触摸角标）/
//! 给 B3 的数据注入位。**不做**：`console.rs` / 通道客户端 / 真实 HTTP / 帧解析 / `request_id`
//! 生成 / 触摸设备初始化 / **整屏降级（EDGE-03）本身**（只留挂点）。
//!
//! ## 偏差登记（**编号 SH\*** —— 与 `D*` / `CD*` / `PD*` / `IL*` / `AU*` / `FR*` / `LG*` 不冲突）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 收口 |
//! |---|----------------------|------|------|
//! | SH1 | 导航页签**图标**用几何字形占位（`● ■ ▼ ⚠ ✓ ○`），非语义图标 | 生成字体 cmap 里的几何字形**只有 13 个**（U+2013/2190/2192/2212/2264/2265/25A0/25B2/25BC/25CB/25CF/26A0/2713，实测见 `fonts/lv_font_cmap.txt`），画不出「配置 / 日志 / 审计」这类专属图标；语义由 26 px **文字通道**承担（F14 的"文字 + 颜色"两通道齐备） | 字库扩充批（同 **D4/D5**）：§3.6 补图标清单 + 重跑 `gen_fonts.sh` |
//! | SH2 | 「弹层打开」在**生产侧**拿不到：P2 / P4 的 `with_dialog` 是 `#[cfg(test)]` ⇒ 本层用 [`Shell::set_modal_open`] **注入** | 页面侧没有生产可见的"弹层是否打开"查询口；B3 的 `UiState.confirm: Option<ConfirmDialog>` 本就是权威真源（技术设计 §5.4），由它注入即可 | **B3**：`app.tick` 内 `shell.set_modal_open(state.confirm.is_some())` |
//! | SH3 | **倒计时胶囊出现时，通道胶囊与触摸角标让位（隐藏）** | 页眉五者同时上屏的最小总宽 **> 1024 px**（算式：返回 64 + 标题 380 + 缝 16 + 通道胶囊 280 + 缝 16 + 倒计时胶囊 264 + 缝 16 + 时钟 120 + 右留白 16 = **1172 px**；即便去掉返回键仍超 1108 px）——UI §4.1 的"右端时钟 + 通道胶囊"与 §4.3 的"时钟左侧倒计时胶囊"在 1024 px 上**不可共存**。倒计时只出现在回归前 ≤10 s 且最紧急 ⇒ 由它优先占据右端 | 若 PM 裁定必须四者同显：需缩小胶囊 / 时钟（<24 px 违反 §3.3 下限）或改页眉分区，属**重新设计** |
//! | SH4 | 「放弃修改」按钮触区高 **48**（UI §4.3 写「64 高触区」） | 同一行又写提示条 **h48** ⇒ 48 高的条里放不下 64 高的按钮（子对象被父内容区裁切）。48 = `Dimens::TOUCH_MIN`，且 §2.1 的「关键操作 64×64」清单（保存 / 恢复默认值 / 联锁释放 / M1 授权 / 导航项 / **返回**）**不含**它 | 同 SH3（若 PM 改条高为 64，两者同时改） |
//! | SH5 | 「**任何**触摸事件重置计时」在本层只能覆盖**外壳自身**的按压：外壳根（全屏）+ 返回键 + 6 个页签 + 放弃修改键。**页面内部控件**上的按压**不会**重置 | `LV_OBJ_FLAG_EVENT_BUBBLE` **未**在 `crate::lvgl::obj::ObjFlag` 镜像（只有 HIDDEN / CLICKABLE / CHECKABLE / SCROLLABLE 四个）⇒ 页内控件（页根本身默认 `CLICKABLE`）吃掉 `PRESSED` 后**不上冒**；`crate::lvgl::indev::Indev` 也**未**暴露 `lv_indev_add_event_cb`（该符号**在** `lvgl-sys/allowlist.txt` 里，只是薄层没封装）⇒ `ui/**` 拿不到"任意按压"的全局钩子。**具名能力需求（二选一）**：① 把 `LV_OBJ_FLAG_EVENT_BUBBLE` 纳入 `ObjFlag`（外壳即可给页根批量置位）；② 给 `Indev` 加 `on(EventCode, F)`（`lv_indev_add_event_cb` 直投，`src/indev/lv_indev.c:997` 的 `send_event(LV_EVENT_PRESSED, indev_act)` 是**每次按压**都发） | **薄层（`src/lvgl/**`）**；本单元已在 `pages_chain` 里对"外壳根 / 页签 / 返回键"三条路径**逐条断言**重置 |
//! | SH6 | 倒计时胶囊的**出现判据取 ≤10 s**（§4.3 表「超时前 10 s」），故文案从 `10 秒后返回主状态页` 起数；§4.3 表内的示例文案写的是 `12 秒后返回主状态页` | **§4.3 自相矛盾**（判据 10 s vs 示例 12 s）。取**判据**（行为规格），文案按实际剩余秒数渲染 | 无（**有意**取行为规格）；若 PM 裁定 12 s，改 [`COUNTDOWN_WINDOW_SECS`] 一处即可 |
//! | SH7 | EDGE-20 的"页眉**左侧** `与主进程数据通道断开`（红）+ 右侧正常时钟"落成「**标题右侧**的红通道胶囊 + 右端时钟**同时可见**」 | §4.1 把"通道状态胶囊"定位在**页眉右端**（与时钟同区），§8.3 EDGE-20 却写"左侧" —— 二者对同一元素给出不同位置。本层取 §4.1 的**位置**（右端），并在此登记取 §8.3 的**语义**（红 = 读通道断 + 时钟仍正常 ⇒ "两种状态同显"，正是 EDGE-20 的判据）。**EDGE-20 的完整语义**（"控制通道**可达** vs 读通道**断**"这一二元区分）需要**两个**独立通道信号，本层只有一个 `ChannelStatus` 输入 ⇒ 归 **B3** | **B3**：`set_channel` 之外再注入"控制通道态"，届时红胶囊文案/位置按 §8.3 重排 |
//! | SH8 | **整屏降级（EDGE-03：压暗 20 % + 中央文案 + 恢复倒计时 + 冻结角标）未实现**，只留挂点 [`Shell::overlay_layer`] | 属 **B3**（需要通道客户端与恢复倒计时状态机）—— 任务书明确"不做" | **B3** |
//! | SH9 | 页内通道条（P1 的「与主进程数据通道断开」行）与页眉通道胶囊**重复表达**同一事实 | `pages/mod.rs` 的 **D1** 已登记"外壳装配时移除页内通道条"，但本单元**禁改 `ui/pages/**`**（硬约束 5）⇒ 重复仍在 | **B2c 收口 / 后续批**（需 PM 授权改 `p1_status.rs`） |
//! | SH10 | 导航 6 项各取 `Dimens::NAV_ITEM_W` = **170**，共 1020 px < 1024（右端余 **4 px** 无页签） | `Dimens::NAV_ITEM_W`（UI §5.1 #1 取整）是 theme 的**单一真源**，本层不得写 170.7；UI §4.2 写"每项宽 1024/6 ≈ 170.7" | 无（**有意**用 theme 常量；4 px 余量不构成可用触摸区） |
//!
//! ## 不变量（编码约束，逐条对应技术设计 §5.2）
//!
//! 1. **回调内不做阻塞 I/O、不 panic**：本文件全部回调只写 `Cell` / `RefCell` 与 LVGL 属性；
//! 2. **不在渲染回调内创建 / 删除对象**：唯一的对象创建在 [`Shell::new`] 与
//!    [`Shell::new`] 的同批装配内；运行期只做 `set_text` / `set_hidden` / `set_pos` / `set_size`；
//! 3. **不读时钟**：`now` 一律经 [`Shell::tick`] 注入 ⇒ 超时链路离屏可确定性复现；
//! 4. **`Rc` 不成环**：外壳持页（单向），页不持外壳；回调一律 `Weak<Core>` + `upgrade`；
//!    回调槽一律走 `pages::CbSlot`（不得手写 take / put-back）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::lvgl::event::EventCode;
use crate::lvgl::obj::{Obj, ObjFlag};
use crate::lvgl::style::{Color, Opa, State, Style, StyleSelector};
use crate::lvgl::widgets::{Label, LongMode, TextButton};
use crate::lvgl::LvglError;
use crate::state::ChannelStatus;
use crate::ui::components::StatusChip;
use crate::ui::pages::{
    decor, label, layout_box, p1_status, p2_config, p3_logs, p4_interlock, p5_audit, p6_system,
    set_style_index, set_visible, CbSlot,
};
use crate::ui::theme::{self, ButtonKind, ChipSkin, Dimens, Palette, Radius, Stroke, TextSlot};

// ═══════════════════════════════════════════════════════════════════════════
// 1. 外壳专属栅格常量（**逐条由 theme 常量推导**；见 UI §4.1 / §4.2 / §4.3）
//
// 纪律（与 `pages/**` 同款）：本块是外壳版式的**唯一**数字来源；调用点的
// `set_size` / `set_pos` 实参一律是常量或变量（`ui/tests.rs::ui_layout_setters_use_theme_constants`
// 静态网逐条扫），且常量初始化式**不得**是裸十进制整数
// （`ui_const_i32_definitions_derive_from_theme` 静态网）。
// ═══════════════════════════════════════════════════════════════════════════

/// 页眉：返回键 x（UI §4.1「x 12–76」= 左安全边 16 内缩一个强调条宽 4）。
const HEADER_BACK_X: i32 = Dimens::SIDE_PAD - Dimens::ACCENT_BAR;
/// 页眉：返回键 y（72 − 64 后居中 → 4；UI §4.1「y 4–68」）。
const HEADER_BACK_Y: i32 = (Dimens::HEADER_H - Dimens::TOUCH_CRITICAL) / 2;
/// 页眉：P1 标题 x（UI §4.1「P1 左为标题文字 x 16」）。
const HEADER_TITLE_X: i32 = Dimens::SIDE_PAD;
/// 页眉：P2–P6 标题 x（UI §4.1「x 96 起」= 返回键 64 + 左右各 16）。
const HEADER_TITLE_X_PAGED: i32 = Dimens::TOUCH_CRITICAL + 2 * Dimens::SIDE_PAD;
/// 页眉：标题 y（72 − 32 后居中 → 20）。
const HEADER_TITLE_Y: i32 =
    theme::center_offset(Dimens::HEADER_H, TextSlot::PageTitle.px() as i32);
/// 页眉：时钟宽（UI §4.1「时钟 26 px 等宽」，`HH:MM:SS` 八字符）。
const HEADER_CLOCK_W: i32 = Dimens::BTN_MIN_W;
/// 页眉：时钟 x（贴右安全边）。
const HEADER_CLOCK_X: i32 = Dimens::SCREEN_W - Dimens::SIDE_PAD - HEADER_CLOCK_W;
/// 页眉：时钟 y（72 − 26 后居中 → 23）。
const HEADER_CLOCK_Y: i32 = theme::center_offset(Dimens::HEADER_H, TextSlot::Label.px() as i32);
/// 页眉：通道胶囊宽（`与主进程数据通道断开` 10 字 × 24 + 图标位 32 + 右留白 16 = 288 ⇒ 取 280+）。
const HEADER_CHIP_W: i32 = Dimens::BTN_MAIN_W + Dimens::TOUCH_CRITICAL + Dimens::GAP_MIN;
/// 页眉：通道胶囊 x（标题区右侧、页眉中段）。
const HEADER_CHIP_X: i32 = Dimens::CONTENT_W * 2 / 5 + Dimens::GAP_GROUP;
/// 页眉：胶囊类元素 y（72 − 32 后居中 → 20）。
const HEADER_CHIP_Y: i32 = theme::center_offset(Dimens::HEADER_H, Dimens::STATUS_CHIP_H);
/// 页眉：触摸不可用角标宽（`触摸不可用` 5 字 × 24 + 右留白）。
const HEADER_BADGE_W: i32 = Dimens::BTN_MIN_W + Dimens::TOUCH_MIN;
/// 页眉：触摸不可用角标 x（通道胶囊右侧一个呼吸缝）。
const HEADER_BADGE_X: i32 = HEADER_CHIP_X + HEADER_CHIP_W + Dimens::GAP_GROUP;
/// 页眉：触摸不可用角标 y（72 − 24 后居中 → 24）。
const HEADER_BADGE_Y: i32 = theme::center_offset(Dimens::HEADER_H, TextSlot::Body.px() as i32);
/// 页眉：倒计时胶囊宽（`10 秒后返回主状态页` 11 字形 × 24 + 两侧留白 = 264）。
const HEADER_CAPSULE_W: i32 = Dimens::BTN_MAIN_W + Dimens::TOUCH_CRITICAL;
/// 页眉：倒计时胶囊 x（UI §4.3「时钟左侧」）。
const HEADER_CAPSULE_X: i32 = HEADER_CLOCK_X - Dimens::GAP_GROUP - HEADER_CAPSULE_W;
/// 页眉：P2–P6 标题宽（到通道胶囊左侧一个呼吸缝为止）。
const HEADER_TITLE_W: i32 = HEADER_CHIP_X - HEADER_TITLE_X_PAGED - Dimens::GAP_GROUP;
/// 页眉：P1 标题宽（无返回键，故比 P2–P6 多出返回键与两缝的宽度）。
const HEADER_TITLE_W_P1: i32 = HEADER_CHIP_X - HEADER_TITLE_X - Dimens::GAP_GROUP;

/// 导航：图标 y（项内上沿；UI §4.2「图标 28 px，y 706–734」⇒ 项内 y 10，此处取 theme 的 8 px 档）。
const NAV_ICON_Y: i32 = Dimens::GAP_MIN / 2;
/// 导航：文字 y（图标下沿 + 半个呼吸缝）。
const NAV_TEXT_Y: i32 = NAV_ICON_Y + Dimens::ICON_SM + Dimens::GAP_MIN / 2;
/// 导航：相邻项竖分隔线宽（UI §4.2「1 px `#2A3B57`」）。
const NAV_DIVIDER_W: i32 = Stroke::THIN;
/// 导航：顶部选中条 y（贴项顶）。
const NAV_BAR_Y: i32 = 0;

/// 内容区：页根容器的 y（页眉之下）。
const CONTENT_Y: i32 = Dimens::HEADER_H;

/// 未保存提示条：高（UI §4.3「h 48」= 最小触摸目标）。
const BANNER_H: i32 = Dimens::TOUCH_MIN;
/// 未保存提示条：图标 x（UI §2.5 警示行左内边距）。
const BANNER_ICON_X: i32 = Dimens::GAP_MIN;
/// 未保存提示条：文案 x（图标 + 图标宽 + 呼吸缝）。
const BANNER_TEXT_X: i32 = BANNER_ICON_X + Dimens::ICON_SM + Dimens::GAP_MIN;
/// 未保存提示条：图标 y（48 − 28 后居中 → 10）。
const BANNER_ICON_Y: i32 = theme::center_offset(BANNER_H, Dimens::ICON_SM);
/// 未保存提示条：文案 y（48 − 24 后居中 → 12）。
const BANNER_TEXT_Y: i32 = theme::center_offset(BANNER_H, TextSlot::Body.px() as i32);
/// 未保存提示条：「放弃修改」按钮宽（逐字宽 = 字号；薄层无文本度量接口，与 `pages/**` 同口径）。
fn discard_w() -> i32 {
    TEXT_DISCARD.chars().count() as i32 * TextSlot::Label.px() as i32 + 2 * Dimens::GAP_MIN
}

/// 倒计时窗口：进入"超时前 N 秒"即显示胶囊（UI §4.3 表「超时前 10 s」）。
pub const COUNTDOWN_WINDOW_SECS: u64 = 10;

// ═══════════════════════════════════════════════════════════════════════════
// 2. 上屏固定文案（UI §3.6「页眉 / 底部导航 / 超时」行的**唯一真源**）
// ═══════════════════════════════════════════════════════════════════════════

/// 页眉返回键文案（UI §3.6 页眉行）。
pub const TEXT_BACK: &str = "返回";
/// 未保存提示条主文案（UI §3.6 超时行 / §4.3）。
pub const TEXT_DIRTY_BANNER: &str = "有未保存修改";
/// 未保存提示条右侧文字按钮（UI §3.6 超时行 / §4.3）。
pub const TEXT_DISCARD: &str = "放弃修改";
/// 未保存提示条图标（UI §4.3「图标 `⚠`」）。
pub const ICON_WARN: &str = "⚠";
/// 通道正常时页眉胶囊（UI §3.6 页眉「通道 / 新鲜度」行）。
pub const TEXT_CHANNEL_OK: &str = "通道已连接";
/// 触摸不可用角标（UI §7.5 / EDGE-13）。
pub const TEXT_TOUCH_UNAVAILABLE: &str = "触摸不可用";

// ═══════════════════════════════════════════════════════════════════════════
// 3. 纯逻辑：导航目标（**不触碰 LVGL** ⇒ 可独立 `#[test]`）
// ═══════════════════════════════════════════════════════════════════════════

/// 六个导航目标（页签索引 ↔ 页面的**单一对应表**）。
///
/// **为什么用枚举而不是裸 `usize`**：路由、页签选中态、页眉标题、返回键可见性、超时回归目标
/// 五处都要"由页得索引 / 由索引得页"，裸整数会让五处各写一份映射（本仓已出过"第二份真源"类
/// 缺陷）。此处**只有这一份**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NavPage {
    /// P1 主状态页（默认页 / 超时回归目标页）。
    Main,
    /// P2 配置页。
    Config,
    /// P3 日志页。
    Logs,
    /// P4 安全 / 联锁页。
    Interlock,
    /// P5 审计页。
    Audit,
    /// P6 系统 / 关于页。
    System,
}

impl NavPage {
    /// 全部页（**页签从左到右的顺序**，UI §4.2）。
    pub const ALL: [NavPage; 6] = [
        NavPage::Main,
        NavPage::Config,
        NavPage::Logs,
        NavPage::Interlock,
        NavPage::Audit,
        NavPage::System,
    ];

    /// 页签索引 → 页面（越界 ⇒ `None`，**不 panic**）。
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(NavPage::Main),
            1 => Some(NavPage::Config),
            2 => Some(NavPage::Logs),
            3 => Some(NavPage::Interlock),
            4 => Some(NavPage::Audit),
            5 => Some(NavPage::System),
            _ => None,
        }
    }

    /// 页面 → 页签索引（**与 [`NavPage::from_index`] 互逆**，由用例逐条往返断言）。
    pub const fn index(self) -> usize {
        match self {
            NavPage::Main => 0,
            NavPage::Config => 1,
            NavPage::Logs => 2,
            NavPage::Interlock => 3,
            NavPage::Audit => 4,
            NavPage::System => 5,
        }
    }

    /// 底部导航页签文案（UI §3.6 底部导航行）。
    pub const fn nav_label(self) -> &'static str {
        match self {
            NavPage::Main => "主状态",
            NavPage::Config => "配置",
            NavPage::Logs => "日志",
            NavPage::Interlock => "安全联锁",
            NavPage::Audit => "审计",
            NavPage::System => "系统",
        }
    }

    /// 页签几何图标（**SH1**：cmap 内几何字形占位，语义由文字通道承担）。
    pub const fn nav_icon(self) -> &'static str {
        match self {
            NavPage::Main => "●",
            NavPage::Config => "■",
            NavPage::Logs => "▼",
            NavPage::Interlock => "⚠",
            NavPage::Audit => "✓",
            NavPage::System => "○",
        }
    }

    /// 页眉标题（UI §3.6 页眉行：`台区储能装置运行状态` / `运行参数配置` / `系统信息` …）。
    pub const fn title(self) -> &'static str {
        match self {
            NavPage::Main => "台区储能装置运行状态",
            NavPage::Config => "运行参数配置",
            NavPage::Logs => "日志",
            NavPage::Interlock => "安全联锁",
            NavPage::Audit => "审计",
            NavPage::System => "系统信息",
        }
    }

    /// 页眉 `返回` 键是否可见（UI §4.1：**仅 P2–P6 显示，P1 不显示**）。
    pub const fn shows_back(self) -> bool {
        !matches!(self, NavPage::Main)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 纯逻辑：空闲超时（**时钟一律注入** ⇒ 离屏可确定性复现）
// ═══════════════════════════════════════════════════════════════════════════

/// 距超时还剩多少秒（**向上取整**；已到期 ⇒ `0`）。
///
/// 为什么向上取整：UI §4.3 要求文案「每秒递减」且判据是「超时前 10 s」—— `60 s` 的整拍上要
/// 恰好显示 `10 秒后返回主状态页`。向下取整会在 `t = 50 s` 显示 10、`t = 50.999 s` 仍是 10，
/// 到 `t = 51 s` 才跳到 9 ⇒ 实际是"每 1 s 递减"但起点错半拍；向上取整使**每一秒区间**
/// `(50, 51]` 都映射到同一个整数，与"每秒递减"逐拍一致。
pub fn remaining_secs(now: Instant, last_activity: Instant, timeout: Duration) -> u64 {
    let rem = last_activity
        .checked_add(timeout)
        .map(|deadline| deadline.saturating_duration_since(now))
        .unwrap_or_default();
    (rem.as_millis() as u64).div_ceil(1000)
}

/// 倒计时胶囊文案（UI §3.6 超时行 `秒后返回主状态页`）。
pub fn countdown_text(secs: u64) -> String {
    format!("{secs} 秒后返回主状态页")
}

/// 是否应显示倒计时胶囊（UI §4.3：「超时前 10 s」出现）。
pub const fn shows_countdown(secs: u64) -> bool {
    secs > 0 && secs <= COUNTDOWN_WINDOW_SECS
}

/// 是否应切回主状态页（UI §4.3：「到达 0 s 自动切 P1」）。
pub const fn should_return_home(secs: u64) -> bool {
    secs == 0
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 纯逻辑：页眉通道状态（UI §4.1 / §8.3 EDGE-20）
// ═══════════════════════════════════════════════════════════════════════════

/// 页眉通道胶囊的三态（由注入的 [`ChannelStatus`] 派生，**本层不自行判断**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderChannel {
    /// 读通道通（`●` 绿）。
    Connected,
    /// 尚未首连（`●` 琥珀）。
    Connecting,
    /// 读通道断（`!` 红）——EDGE-20「控制通道可达但读通道断」的**红**通道。
    Down,
}

impl HeaderChannel {
    /// 三态（**数组下标 = [`HeaderChannel::index`]**，装配时逐件建胶囊）。
    pub const ALL: [HeaderChannel; 3] = [
        HeaderChannel::Connected,
        HeaderChannel::Connecting,
        HeaderChannel::Down,
    ];

    /// 由状态层的通道态派生。
    pub const fn from_status(status: ChannelStatus) -> Self {
        match status {
            ChannelStatus::Connected => HeaderChannel::Connected,
            ChannelStatus::Init => HeaderChannel::Connecting,
            ChannelStatus::Down => HeaderChannel::Down,
        }
    }

    /// 三个胶囊在 [`Core::chips`] 数组里的下标（**唯一对应表**）。
    pub const fn index(self) -> usize {
        match self {
            HeaderChannel::Connected => 0,
            HeaderChannel::Connecting => 1,
            HeaderChannel::Down => 2,
        }
    }

    /// 胶囊文案。
    pub const fn text(self) -> &'static str {
        match self {
            HeaderChannel::Connected => TEXT_CHANNEL_OK,
            HeaderChannel::Connecting => p1_status::TEXT_CHANNEL_CONNECTING,
            HeaderChannel::Down => p1_status::TEXT_CHANNEL_DOWN,
        }
    }

    /// 胶囊图标（F14 三通道之一）。
    pub const fn icon(self) -> &'static str {
        match self {
            HeaderChannel::Connected | HeaderChannel::Connecting => "●",
            HeaderChannel::Down => "!",
        }
    }

    /// 胶囊皮肤（颜色通道）。
    pub const fn skin(self) -> ChipSkin {
        match self {
            HeaderChannel::Connected => ChipSkin::SUCCESS,
            HeaderChannel::Connecting => ChipSkin::NEUTRAL,
            HeaderChannel::Down => ChipSkin::FAILURE,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. 外壳内部类型
// ═══════════════════════════════════════════════════════════════════════════

/// 一个底部导航页签（UI §4.2 / §5.1 #1）。
///
/// **句柄全部存进结构体**（R1）：页签是"拥有型 LVGL 句柄"的密集处 —— 顶部选中条 / 分隔线 /
/// 图标 / 文案四个子对象若只在构造器里当局部变量，`Drop` 会级联删掉它们（屏上只剩空按钮而
/// 无任何报错）。故四个字段**都是字段**，并各配一个读回口供离屏断言。
struct NavTab {
    /// 页签按钮（`CHECKABLE`；选中态由 `LV_STATE_CHECKED` 表达）。
    btn: TextButton,
    /// 顶部 4 px 选中条（未选中时 `HIDDEN`）。
    bar: Obj,
    /// 左缘 1 px 竖分隔（第 1 项无；`None`）。
    divider: Option<Obj>,
    /// 几何图标（28 px）。
    icon: Rc<Label>,
    /// 页签文案（26 px）。
    text: Rc<Label>,
    /// 图标的两档样式（0 = 未选中 / 1 = 选中）—— 选中态**双通道**的"文字色"通道之一。
    icon_styles: [Rc<Style>; 2],
    /// 文案的两档样式。
    text_styles: [Rc<Style>; 2],
    /// 图标两档**色值**（与 `icon_styles` 逐项对应）。
    ///
    /// **应用标记**（与 `p5_audit.rs` 的 `immutable_skin` 同法）：薄层没有"已挂样式读回"通道
    /// ⇒ 本层把**送进 `theme::text(..)` 的那个色值**记下来，供离屏断言核对"选中态第二通道
    /// 真的换了色"。把 `icon_styles` 的两个色值对调 ⇒ [`Shell::tab_icon_color`] 随之变化 ⇒ 用例红。
    icon_colors: [Color; 2],
    /// 文案两档**色值**（同上）。
    text_colors: [Color; 2],
    /// 当前挂着的图标样式下标（`usize::MAX` = 尚未挂过）。
    icon_idx: Cell<usize>,
    /// 当前挂着的文案样式下标。
    text_idx: Cell<usize>,
}

/// 外壳的全部拥有型状态 + LVGL 句柄。
///
/// **`Rc` 不成环**：本类型持 6 页（单向允许）；页**不**持本类型；LVGL 回调只持
/// `Weak<Core>`（见 [`Shell::wire`]）⇒ `drop(Shell)` 必然级联删除整棵子树。
struct Core {
    /// 外壳根（全屏；`CLICKABLE` ⇒ 「全屏输入对象」）。
    root: Obj,
    /// 页眉容器。
    header: Obj,
    /// 返回键（仅 P2–P6 可见）。
    back: TextButton,
    /// 页标题。
    title: Rc<Label>,
    /// 时钟文本（**注入**）。
    clock: Rc<Label>,
    /// 通道胶囊三件（下标见 [`HeaderChannel::index`]）。
    chips: [StatusChip; 3],
    /// 倒计时胶囊（底色 + 描边 + 文案）。
    capsule: Obj,
    /// 倒计时胶囊文案。
    capsule_text: Rc<Label>,
    /// 触摸不可用角标（EDGE-13）。
    badge: Rc<Label>,
    /// 内容区容器（6 个页根的宿主）。
    content: Obj,
    /// 页根的直接宿主（`CONTENT_W × CONTENT_H`，`content` 内左移 `SIDE_PAD`）。
    ///
    /// ⚠️ **必须是字段**：写成 `Shell::new` 里的局部变量 ⇒ 构造器返回时 `Drop` 它会**级联删除
    /// 6 个页根**（屏上六页全空，而任何"构造没报错"的断言都不会发现）。这正是本仓复发了三次的
    /// `R1` 缺陷类；`shell_chain` 的"页根须存活"断言就是为它设的。
    page_host: Obj,
    /// 未保存修改提示条（EDGE-11）。
    banner: Obj,
    /// 提示条图标。
    banner_icon: Rc<Label>,
    /// 提示条文案。
    banner_text: Rc<Label>,
    /// 提示条右侧「放弃修改」文字按钮。
    discard: TextButton,
    /// 底部导航容器。
    nav: Obj,
    /// 6 个页签。
    tabs: [NavTab; 6],
    // ── 6 页（外壳**持**页；页不持外壳）──
    p1: p1_status::P1StatusPage,
    p2: p2_config::P2ConfigPage,
    p3: p3_logs::P3LogsPage,
    p4: p4_interlock::P4InterlockPage,
    p5: p5_audit::P5AuditPage,
    p6: p6_system::P6SystemPage,
    // ── 状态（全部 `Cell` / `RefCell`，回调经 `Weak` 写）──
    /// 当前页签下标。
    current: Cell<usize>,
    /// 最近一次"用户活动"时刻（**注入时钟**）。
    /// `None` = 尚未收到任何 `tick` ⇒ 构造期**不读** `Instant::now()`（首拍由 [`Shell::tick`] 落定）。
    last_activity: RefCell<Option<Instant>>,
    /// 空闲超时时长（`--idle-timeout-secs`；默认 [`theme::Timing::IDLE_TIMEOUT_SECS`]）。
    timeout: Cell<Duration>,
    /// 是否有 LVGL 事件（`PRESSED`）自上次 [`Shell::tick`] 以来到达 —— **待消费**。
    pending_activity: Cell<bool>,
    /// 是否有确认弹层打开（**注入**：页侧无生产可见查询口，见 **SH2**）。
    modal_open: Cell<bool>,
    /// 触摸设备是否可用（EDGE-13）。
    touch_available: Cell<bool>,
    /// 页眉通道态。
    channel: Cell<HeaderChannel>,
    /// 倒计时胶囊当前是否可见（**应用标记**：薄层没有"已挂样式读回"通道 ⇒ 记下本层送出的状态）。
    capsule_on: Cell<bool>,
    /// 未保存提示条当前是否可见（同上）。
    banner_on: Cell<bool>,
    /// 切页通知槽（**契约**：回调内不得再调 [`Shell::show`]；见 [`Shell::set_on_page_change`]）。
    on_page_change: CbSlot<NavPage>,
}

/// 应用外壳（页眉 + 内容区 + 底部导航 + 顶层提示条）。
pub struct Shell {
    /// **`Rc` 是必需的**：LVGL 回调要经 `Weak` 回指（否则 `Core` 无法在回调里被访问）。
    core: Rc<Core>,
}

impl Shell {
    /// 装配外壳：建根 / 页眉 / 内容区（**含 6 页**）/ 提示条 / 底部导航，并挂钩回调。
    ///
    /// `parent` 通常是活动屏（`Obj::screen()`），但也可以是任意容器（测试用宿主容器）。
    /// 外壳根是 `parent` 的**子对象**、尺寸 `SCREEN_W × SCREEN_H`、自身 `(0, 0)`。
    pub fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = Obj::create(parent)?;
        root.set_size(Dimens::SCREEN_W, Dimens::SCREEN_H);
        root.set_pos(0, 0);
        root.add_style(&theme::screen_bg(), StyleSelector::main());
        root.remove_flag(ObjFlag::SCROLLABLE);

        let header = layout_box(&root, Dimens::SCREEN_W, Dimens::HEADER_H)?;
        header.set_pos(0, 0);
        header.add_style(&header_bg(), StyleSelector::main());

        // ── 返回键（64×64，仅 P2–P6 可见）──
        let back = TextButton::create(&header, TEXT_BACK)?;
        back.set_size(Dimens::TOUCH_CRITICAL, Dimens::TOUCH_CRITICAL);
        back.set_pos(HEADER_BACK_X, HEADER_BACK_Y);
        theme::button(ButtonKind::Secondary).apply(&back);

        // ── 标题（32 px；位置 / 宽度随页切换）──
        let title = Rc::new(label(&header, TextSlot::PageTitle, Palette::TEXT_PRIMARY)?);
        title.set_long_mode(LongMode::DOTS);

        // ── 时钟（26 px，注入文本）──
        let clock = Rc::new(label(&header, TextSlot::Label, Palette::TEXT_SECOND)?);
        clock.set_size(HEADER_CLOCK_W, TextSlot::Label.px() as i32);
        clock.set_pos(HEADER_CLOCK_X, HEADER_CLOCK_Y);
        clock.set_long_mode(LongMode::CLIP);

        // ── 通道胶囊 ×3（三态各一件，**只显其一**）──
        let mut chips = Vec::with_capacity(HeaderChannel::ALL.len());
        for ch in HeaderChannel::ALL {
            let chip = StatusChip::new(&header, HEADER_CHIP_W, ch.icon(), ch.text(), ch.skin())?;
            chip.set_pos(HEADER_CHIP_X, HEADER_CHIP_Y);
            set_visible(chip.obj(), false);
            chips.push(chip);
        }
        let chips: [StatusChip; 3] = chips
            .try_into()
            .map_err(|_| LvglError::InvalidArgument("通道胶囊三态"))?;

        // ── 倒计时胶囊（底 `#3A2E12` 描边 `#FFB020`，文案 24 px `#FFD75E`）──
        let capsule = layout_box(&header, HEADER_CAPSULE_W, Dimens::STATUS_CHIP_H)?;
        capsule.set_pos(HEADER_CAPSULE_X, HEADER_CHIP_Y);
        capsule.add_style(&capsule_skin(), StyleSelector::main());
        let capsule_text = Rc::new(label(&capsule, TextSlot::Body, Palette::STANDBY)?);
        capsule_text.set_size(
            HEADER_CAPSULE_W - Dimens::GAP_MIN,
            TextSlot::Body.px() as i32,
        );
        capsule_text.set_pos(
            Dimens::GAP_MIN / 2,
            theme::center_offset(Dimens::STATUS_CHIP_H, TextSlot::Body.px() as i32),
        );
        capsule_text.set_long_mode(LongMode::CLIP);
        set_visible(&capsule, false);

        // ── 触摸不可用角标（EDGE-13；24 px `#FFB020`）──
        let badge = Rc::new(label(&header, TextSlot::Body, Palette::STALE)?);
        badge.set_text(TEXT_TOUCH_UNAVAILABLE);
        badge.set_size(HEADER_BADGE_W, TextSlot::Body.px() as i32);
        badge.set_pos(HEADER_BADGE_X, HEADER_BADGE_Y);
        badge.set_long_mode(LongMode::CLIP);
        set_visible(&badge, false);

        // ── 内容区（6 页的宿主；页根由外壳摆放在 `(SIDE_PAD, CONTENT_Y)`）──
        let content = layout_box(&root, Dimens::SCREEN_W, Dimens::CONTENT_H)?;
        content.set_pos(0, CONTENT_Y);
        let page_host = layout_box(&content, Dimens::CONTENT_W, Dimens::CONTENT_H)?;
        page_host.set_pos(Dimens::SIDE_PAD, 0);

        let p1 = p1_status::P1StatusPage::new(&page_host)?;
        let p2 = p2_config::P2ConfigPage::new(&page_host)?;
        let p3 = p3_logs::P3LogsPage::new(&page_host)?;
        let p4 = p4_interlock::P4InterlockPage::new(&page_host)?;
        let p5 = p5_audit::P5AuditPage::new(&page_host)?;
        let p6 = p6_system::P6SystemPage::new(&page_host)?;

        // ── 未保存修改提示条（EDGE-11；`h48`）──
        let banner = layout_box(&root, Dimens::CONTENT_W, BANNER_H)?;
        banner.set_pos(Dimens::SIDE_PAD, CONTENT_Y);
        banner.add_style(&banner_skin(), StyleSelector::main());
        let banner_icon = Rc::new(label(&banner, TextSlot::SectionTitle, Palette::STALE)?);
        banner_icon.set_text(ICON_WARN);
        banner_icon.set_size(Dimens::ICON_SM, Dimens::ICON_SM);
        banner_icon.set_pos(BANNER_ICON_X, BANNER_ICON_Y);
        let banner_text = Rc::new(label(&banner, TextSlot::Body, Palette::STANDBY)?);
        banner_text.set_text(TEXT_DIRTY_BANNER);
        banner_text.set_size(
            Dimens::CONTENT_W - BANNER_TEXT_X - discard_w() - Dimens::GAP_MIN,
            TextSlot::Body.px() as i32,
        );
        banner_text.set_pos(BANNER_TEXT_X, BANNER_TEXT_Y);
        banner_text.set_long_mode(LongMode::CLIP);
        let discard = TextButton::create(&banner, TEXT_DISCARD)?;
        // 触区高 = 提示条高（48 = `Dimens::TOUCH_MIN`；UI §4.3 的「64 高触区」与同一行的
        // 「条高 h48」冲突，取条高 —— 见偏差 **SH4**）。
        discard.set_size(discard_w(), BANNER_H);
        discard.set_pos(Dimens::CONTENT_W - discard_w() - Dimens::GAP_MIN, 0);
        theme::button(ButtonKind::Text).apply(&discard);
        set_visible(&banner, false);

        // ── 底部导航（6 项常驻，每项 `NAV_ITEM_W × NAV_ITEM_H`）──
        let nav = layout_box(&root, Dimens::SCREEN_W, Dimens::NAV_H)?;
        nav.set_pos(0, Dimens::HEADER_H + Dimens::CONTENT_H);
        nav.add_style(&nav_bg(), StyleSelector::main());
        let item_styles = [
            nav_item_default(),
            nav_item_selected(),
            nav_item_pressed(),
        ];
        let mut tabs = Vec::with_capacity(NavPage::ALL.len());
        for page in NavPage::ALL {
            tabs.push(build_tab(&nav, page, &item_styles)?);
        }
        let tabs: [NavTab; 6] = tabs
            .try_into()
            .map_err(|_| LvglError::InvalidArgument("导航六项"))?;

        let core = Rc::new(Core {
            root,
            header,
            back,
            title,
            clock,
            chips,
            capsule,
            capsule_text,
            badge,
            content,
            page_host,
            banner,
            banner_icon,
            banner_text,
            discard,
            nav,
            tabs,
            p1,
            p2,
            p3,
            p4,
            p5,
            p6,
            current: Cell::new(NavPage::Main.index()),
            last_activity: RefCell::new(None),
            timeout: Cell::new(Duration::from_secs(theme::Timing::IDLE_TIMEOUT_SECS)),
            pending_activity: Cell::new(false),
            modal_open: Cell::new(false),
            touch_available: Cell::new(true),
            channel: Cell::new(HeaderChannel::Connected),
            capsule_on: Cell::new(false),
            banner_on: Cell::new(false),
            on_page_change: CbSlot::new(),
        });
        let shell = Shell { core };
        shell.wire();
        // 初始页 = P1（首次进入不触发切页通知 —— 尚未有订阅者）。
        shell.core.select(NavPage::Main, false);
        shell.core.apply_header_channel();
        Ok(shell)
    }

    /// 挂钩全部 LVGL 回调（**回调一律 `Weak<Core>` + `upgrade`** —— 写成 `Rc<Core>` 即
    /// `Core → 控件 → 回调 → Rc<Core>` 强引用环，`drop(Shell)` 不释放任何对象、
    /// 建/拆第 2 次即 OOM；本仓 P5 已复发过一次）。
    fn wire(&self) {
        let weak = Rc::downgrade(&self.core);
        // ①**全屏输入对象**：任何落在"非控件"区域的按压都命中外壳根（UI §4.3「计时重置」）。
        //    页内控件上的按压不会上冒（**SH5**：`ObjFlag` 未镜像 `EVENT_BUBBLE`）。
        self.core.root.on(EventCode::PRESSED, {
            let weak = weak.clone();
            move |_e| {
                if let Some(c) = weak.upgrade() {
                    c.pending_activity.set(true);
                }
            }
        });
        // ② 返回键 ⇒ 切 P1（UI §4.3「主动返回」）。
        self.core.back.on_clicked({
            let weak = weak.clone();
            move |_e| {
                if let Some(c) = weak.upgrade() {
                    c.pending_activity.set(true);
                    c.select(NavPage::Main, true);
                }
            }
        });
        // ③ 6 个页签 ⇒ 1 次触摸直达（UI §4.2）。
        for (i, tab) in self.core.tabs.iter().enumerate() {
            let weak = weak.clone();
            tab.btn.on_clicked(move |_e| {
                if let Some(c) = weak.upgrade() {
                    c.pending_activity.set(true);
                    if let Some(page) = NavPage::from_index(i) {
                        c.select(page, true);
                    }
                }
            });
        }
        // ④ 「放弃修改」⇒ 丢弃 P2 草稿（脏态清 ⇒ 下一拍恢复倒计时）。
        self.core.discard.on_clicked({
            let weak = weak.clone();
            move |_e| {
                if let Some(c) = weak.upgrade() {
                    c.pending_activity.set(true);
                    c.p2.discard_draft();
                    c.apply_dirty();
                }
            }
        });
    }

    // ── 路由 ──────────────────────────────────────────────────────────────

    /// 切到指定页（**1 次触摸**；不做切换动画，UI §7.5）。
    pub fn show(&self, page: NavPage) {
        self.core.select(page, true);
    }

    /// 当前页。
    pub fn current(&self) -> NavPage {
        NavPage::from_index(self.core.current.get()).unwrap_or(NavPage::Main)
    }

    /// 注册「切页」通知（**仅在页真正变化时**触发一次；初始页不触发）。
    ///
    /// **契约**：回调**不得**再调 [`Shell::show`] / [`Shell::tick`]（重入布局）；
    /// 回调内自替换的语义与 `ui/components.rs` 同款（本次由旧回调跑完、新回调自下次生效）。
    pub fn set_on_page_change<F>(&self, f: F)
    where
        F: FnMut(NavPage) + 'static,
    {
        self.core.on_page_change.set(f);
    }

    // ── B3 注入位 ─────────────────────────────────────────────────────────

    /// 数据注入位 ①：各页（**真实数据源接线属 B3**）。
    pub fn p1(&self) -> &p1_status::P1StatusPage {
        &self.core.p1
    }

    /// 数据注入位 ①：P2 配置页（控制通道驱动，见 `pages/mod.rs` 契约 2′）。
    pub fn p2(&self) -> &p2_config::P2ConfigPage {
        &self.core.p2
    }

    /// 数据注入位 ①：P3 日志页。
    pub fn p3(&self) -> &p3_logs::P3LogsPage {
        &self.core.p3
    }

    /// 数据注入位 ①：P4 安全 / 联锁页。
    pub fn p4(&self) -> &p4_interlock::P4InterlockPage {
        &self.core.p4
    }

    /// 数据注入位 ①：P5 审计页。
    pub fn p5(&self) -> &p5_audit::P5AuditPage {
        &self.core.p5
    }

    /// 数据注入位 ①：P6 系统 / 关于页。
    pub fn p6(&self) -> &p6_system::P6SystemPage {
        &self.core.p6
    }

    /// 数据注入位 ③：读通道态 ⇒ 页眉通道胶囊（EDGE-20 的"红 + 时钟同显"落点）。
    pub fn set_channel(&self, channel: ChannelStatus) {
        self.core.channel.set(HeaderChannel::from_status(channel));
        self.core.apply_header_channel();
    }

    /// 数据注入位 ④：触摸设备可用性（EDGE-13）。
    ///
    /// `false` ⇒ 页眉右端常驻 24 px `#FFB020`「触摸不可用」角标；**数据刷新不受影响**。
    pub fn set_touch_available(&self, available: bool) {
        self.core.touch_available.set(available);
        self.core.apply_header_channel();
    }

    /// 数据注入位 ⑤：空闲超时时长（`--idle-timeout-secs`；默认 60 s）。
    pub fn set_idle_timeout(&self, secs: u64) {
        self.core.timeout.set(Duration::from_secs(secs));
    }

    /// 数据注入位 ⑥：是否有**确认弹层**打开（打开期间暂停计时且不显示倒计时，UI §4.3）。
    ///
    /// ⚠️ **页侧拿不到**（P2 / P4 的 `with_dialog` 是 `#[cfg(test)]`）⇒ 由 B3 从
    /// `UiState.confirm.is_some()` 注入 —— 见偏差 **SH2**。`false` 且 P2 不脏时恢复计时
    /// （从**满时长**重新起算）。
    pub fn set_modal_open(&self, open: bool) {
        self.core.modal_open.set(open);
    }

    /// 数据注入位 ⑦：整屏降级（**EDGE-03**）的挂点 —— 顶层浮层。
    ///
    /// 返回 `lv_layer_top()` 的**非拥有**句柄；B3 在此挂"压暗 20 % + 中央 `与主进程数据通道断开`
    /// + 秒级恢复倒计时 + 冻结角标"的整屏层。本单元**不实现**该层（见偏差 **SH8**）。
    pub fn overlay_layer(&self) -> Result<Obj, LvglError> {
        crate::lvgl::widgets::layer_top()
    }

    /// 手工记一次"用户活动"（回调之外的入口：B3 若在别处收到输入事件可直接调用）。
    ///
    /// 效果与 LVGL `PRESSED` 一致：下一次 [`Shell::tick`] 把空闲计时重置为**那一刻**。
    pub fn note_activity(&self) {
        self.core.pending_activity.set(true);
    }

    // ── 每拍推进 ──────────────────────────────────────────────────────────

    /// 每个事件循环拍调一次：刷新时钟文本、转发 P2 / P4 的 `tick`、消费 `PendingActivity`、
    /// 推进空闲超时状态机（胶囊 / 回归主状态页）、同步未保存提示条。
    ///
    /// **时钟经 `now` / `clock_text` 注入** ⇒ 本外壳不读 `Instant::now()`，超时链路离屏可确定性复现。
    pub fn tick(&self, now: Instant, clock_text: &str) {
        let c = &self.core;
        c.clock.set_text(clock_text);
        // 页内延迟动作（弹层关闭 / Toast 过期）—— 时钟同为注入。
        c.p2.tick(now);
        c.p4.tick(now);

        // ① 消费按压（**消费时刻即"活动时刻"**：真实循环里 `lv_indev_read` 与 `app.tick`
        //    同拍，故误差 ≤ 一拍；离屏用例则完全确定）。首拍（`None`）在此落定 —— 构造期
        //    **不取时钟**（本文件对"时钟一律注入"的纪律是逐条的）。
        let mut last = c.last_activity.borrow_mut();
        if c.pending_activity.replace(false) || last.is_none() {
            *last = Some(now);
        }

        // ② 暂停判据：弹层打开 **或** P2 有未保存修改 ⇒ 不倒计时、不强制切页（UI §4.3）。
        //    实现 = 把"最后活动时刻"钉在当下（等价于"计时暂停"）；放开后从**满时长**重算。
        let paused = c.modal_open.get() || c.p2.is_dirty();
        if paused {
            *last = Some(now);
        }

        let secs = remaining_secs(now, last.unwrap_or(now), c.timeout.get());
        if !paused && should_return_home(secs) {
            // 到 0 ⇒ 自动切 P1（**不产生审计记录**；UI §4.3）。
            *last = Some(now);
            drop(last); // 切页会读页句柄 / 写 LVGL，先放开借用（`select` 不碰 `last_activity`，此处仍求稳）。
            c.select(NavPage::Main, true);
        }
        let show_capsule = !paused && shows_countdown(secs);
        if show_capsule != c.capsule_on.get() {
            c.capsule_on.set(show_capsule);
            set_visible(&c.capsule, show_capsule);
        }
        if show_capsule {
            c.capsule_text.set_text(&countdown_text(secs));
        }

        // ③ 页眉右端：倒计时胶囊出现时通道胶囊与触摸角标让位（**SH3**）。
        c.apply_header_channel();

        // ④ 未保存提示条（EDGE-11）由 P2 的 `is_dirty()` 驱动。
        c.apply_dirty();
    }

    // ── 离屏断言口径（**只读**）───────────────────────────────────────────

    /// 外壳根对象（`drop` 它即整棵子树级联删除）。
    pub fn obj(&self) -> &Obj {
        &self.core.root
    }

    /// 页眉容器。
    pub fn header_obj(&self) -> &Obj {
        &self.core.header
    }

    /// 内容区容器（6 页的宿主）。
    pub fn content_obj(&self) -> &Obj {
        &self.core.content
    }

    /// 页根的直接宿主（`(SIDE_PAD, 0)`，尺寸 `CONTENT_W × CONTENT_H`）—— 6 个页根的父对象。
    ///
    /// **读回口存在的理由**（R1）：它是"拥有型句柄必须是字段"的**探针点** —— 写成局部变量时
    /// 构造器返回即级联删除 6 个页根（屏上全空、无报错）。
    pub fn page_host_obj(&self) -> &Obj {
        &self.core.page_host
    }

    /// 底部导航容器。
    pub fn nav_obj(&self) -> &Obj {
        &self.core.nav
    }

    /// 未保存提示条容器。
    pub fn banner_obj(&self) -> &Obj {
        &self.core.banner
    }

    /// 提示条右侧「放弃修改」按钮。
    pub fn discard_button(&self) -> &TextButton {
        &self.core.discard
    }

    /// 返回键（仅 P2–P6 可见）。
    pub fn back_button(&self) -> &TextButton {
        &self.core.back
    }

    /// 返回键当前是否可见（UI §4.1：P1 不显示）。
    pub fn back_visible(&self) -> bool {
        !self.core.back.is_hidden()
    }

    /// 页标题文本。
    pub fn title_text(&self) -> Option<String> {
        self.core.title.text()
    }

    /// 时钟文本（注入回读）。
    pub fn clock_text(&self) -> Option<String> {
        self.core.clock.text()
    }

    /// 页眉通道胶囊文本（**当前可见**的那一件；全部隐藏时 `None`）。
    pub fn channel_text(&self) -> Option<String> {
        let mut out = None;
        for chip in &self.core.chips {
            if !chip.obj().is_hidden() {
                out = chip.text();
            }
        }
        out
    }

    /// 未保存提示条是否可见。
    pub fn banner_visible(&self) -> bool {
        !self.core.banner.is_hidden()
    }

    /// 提示条文案。
    pub fn banner_text(&self) -> Option<String> {
        self.core.banner_text.text()
    }

    /// 提示条图标字形（`⚠`）。
    pub fn banner_icon_text(&self) -> Option<String> {
        self.core.banner_icon.text()
    }

    /// 倒计时胶囊是否可见。
    pub fn countdown_visible(&self) -> bool {
        !self.core.capsule.is_hidden()
    }

    /// 倒计时胶囊文案。
    pub fn countdown_text_value(&self) -> Option<String> {
        self.core.capsule_text.text()
    }

    /// 触摸不可用角标是否可见（EDGE-13）。
    pub fn touch_badge_visible(&self) -> bool {
        !self.core.badge.is_hidden()
    }

    /// 触摸不可用角标文案。
    pub fn touch_badge_text(&self) -> Option<String> {
        self.core.badge.text()
    }

    /// 第 `i` 个页签（按 `NavPage::ALL` 顺序）。
    pub fn tab(&self, page: NavPage) -> Option<&TextButton> {
        self.core.tabs.get(page.index()).map(|t| &t.btn)
    }

    /// 第 `i` 个页签的顶部选中条是否可见（**选中态双通道之一**）。
    pub fn tab_bar_visible(&self, page: NavPage) -> Option<bool> {
        self.core
            .tabs
            .get(page.index())
            .map(|t| !t.bar.is_hidden())
    }

    /// 第 `i` 个页签的左缘竖分隔线（**第 1 项无 ⇒ `None`**；UI §4.2）。
    pub fn tab_divider(&self, page: NavPage) -> Option<&Obj> {
        self.core
            .tabs
            .get(page.index())
            .and_then(|t| t.divider.as_ref())
    }

    /// 第 `i` 个页签**当前生效**的文字色（= 选中态双通道的"文字"通道）。
    ///
    /// `None` = 尚未摆过选中态（应有且仅有装配前的一瞬）。取值口径 = `text_idx` 指向的
    /// **实际被送进 `theme::text(..)` 的色值**（应用标记，见 `NavTab::text_colors`）。
    pub fn tab_text_color(&self, page: NavPage) -> Option<Color> {
        let t = self.core.tabs.get(page.index())?;
        t.text_colors.get(t.text_idx.get()).copied()
    }

    /// 第 `i` 个页签**当前生效**的图标色（同 [`Shell::tab_text_color`] 的口径）。
    pub fn tab_icon_color(&self, page: NavPage) -> Option<Color> {
        let t = self.core.tabs.get(page.index())?;
        t.icon_colors.get(t.icon_idx.get()).copied()
    }

    /// 第 `i` 个页签的图标字形。
    pub fn tab_icon_text(&self, page: NavPage) -> Option<String> {
        self.core
            .tabs
            .get(page.index())
            .and_then(|t| t.icon.text())
    }

    /// 第 `i` 个页签的文案。
    pub fn tab_text(&self, page: NavPage) -> Option<String> {
        self.core
            .tabs
            .get(page.index())
            .and_then(|t| t.text.text())
    }

    /// 6 页中当前可见的页根对象（**唯一**一个；用于"任一时刻只显一页"的离屏断言）。
    pub fn visible_pages(&self) -> Vec<NavPage> {
        let objs = self.core.page_objs();
        NavPage::ALL
            .into_iter()
            .filter(|p| !objs[p.index()].is_hidden())
            .collect()
    }

    /// 某页页根对象（装配断言口径）。
    pub fn page_obj(&self, page: NavPage) -> &Obj {
        self.core.page_objs()[page.index()]
    }
}

impl Core {
    /// 6 个页根（按 `NavPage::ALL` 顺序）—— **唯一**的"页 ↔ 下标"取值处。
    fn page_objs(&self) -> [&Obj; 6] {
        [
            self.p1.obj(),
            self.p2.obj(),
            self.p3.obj(),
            self.p4.obj(),
            self.p5.obj(),
            self.p6.obj(),
        ]
    }

    /// 切页：显隐页根 + 页签选中态（双通道）+ 页眉返回键 / 标题。
    ///
    /// `notify` 由调用方给：装配期的初始摆放**不**通知（那时还没有订阅者）。
    fn select(&self, page: NavPage, notify: bool) {
        let idx = page.index();
        let changed = self.current.get() != idx;
        self.current.set(idx);

        for (i, obj) in self.page_objs().iter().enumerate() {
            set_visible(obj, i == idx);
        }
        for (i, tab) in self.tabs.iter().enumerate() {
            let on = i == idx;
            tab.btn.set_checked(on);
            set_style_index(tab.icon.obj(), &tab.icon_styles, &tab.icon_idx, usize::from(on));
            set_style_index(tab.text.obj(), &tab.text_styles, &tab.text_idx, usize::from(on));
            set_visible(&tab.bar, on);
        }

        set_visible(&self.back, page.shows_back());
        self.title.set_text(page.title());
        if page.shows_back() {
            self.title.set_pos(HEADER_TITLE_X_PAGED, HEADER_TITLE_Y);
            self.title.set_size(HEADER_TITLE_W, TextSlot::PageTitle.px() as i32);
        } else {
            self.title.set_pos(HEADER_TITLE_X, HEADER_TITLE_Y);
            self.title.set_size(HEADER_TITLE_W_P1, TextSlot::PageTitle.px() as i32);
        }

        if changed && notify {
            self.on_page_change.fire(page);
        }
    }

    /// 页眉右端三件（通道胶囊 / 触摸角标 / 倒计时）的显隐 —— **唯一**落点。
    ///
    /// **SH3**：倒计时胶囊出现时，通道胶囊与触摸角标让位（1024 px 放不下五者）。
    fn apply_header_channel(&self) {
        let capsule = self.capsule_on.get();
        let ch = self.channel.get();
        for (i, chip) in self.chips.iter().enumerate() {
            set_visible(chip.obj(), !capsule && i == ch.index());
        }
        set_visible(&self.badge, !capsule && !self.touch_available.get());
    }

    /// 未保存提示条的显隐（EDGE-11）—— **唯一**落点。
    fn apply_dirty(&self) {
        let dirty = self.p2.is_dirty();
        if dirty != self.banner_on.get() {
            self.banner_on.set(dirty);
            set_visible(&self.banner, dirty);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. 样式与零件构造（色值 / 尺寸**一律**取 `theme` 命名常量；零裸值）
// ═══════════════════════════════════════════════════════════════════════════

/// 页眉底：与页面底色同档（`Palette::SURFACE`），下方 1 px 分隔线由 `nav` 一侧表达。
fn header_bg() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE);
    s.set_bg_opa(Opa::COVER);
    s.set_border_width(Stroke::NONE);
    s.set_pad_all(0);
    s.set_radius(Radius::NONE);
    Rc::new(s)
}

/// 底部导航底（UI §4.2 默认态底 `#141F33`）。
fn nav_bg() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE);
    s.set_bg_opa(Opa::COVER);
    s.set_border_width(Stroke::NONE);
    s.set_pad_all(0);
    s.set_radius(Radius::NONE);
    Rc::new(s)
}

/// 倒计时胶囊皮肤（UI §4.3：底 `#3A2E12`、描边 `#FFB020`、全圆端）。
fn capsule_skin() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::WARN_BG);
    s.set_bg_opa(Opa::COVER);
    s.set_border_color(Palette::STALE);
    s.set_border_width(Stroke::THIN);
    s.set_border_opa(Opa::COVER);
    s.set_radius(Radius::CHIP);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 未保存提示条皮肤（UI §4.3：底 `#3A2E12`、描边 `#FFB020`、h48）。
fn banner_skin() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::WARN_BG);
    s.set_bg_opa(Opa::COVER);
    s.set_border_color(Palette::STALE);
    s.set_border_width(Stroke::THIN);
    s.set_border_opa(Opa::COVER);
    s.set_radius(Radius::CTRL);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 导航项：**默认态**（透明底 —— 露出导航条的 `#141F33`）。
fn nav_item_default() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_opa(Opa::TRANSPARENT);
    s.set_border_width(Stroke::NONE);
    s.set_radius(Radius::NONE);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 导航项：**选中态**（UI §4.2：底 `surface_alt`）。
fn nav_item_selected() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE_ALT);
    s.set_bg_opa(Opa::COVER);
    s.set_border_width(Stroke::NONE);
    s.set_radius(Radius::NONE);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 导航项：**按下态**（UI §4.2 / §5.2：底 `#2E4066`，反馈 ≤100 ms）。
fn nav_item_pressed() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE_PRESS);
    s.set_bg_opa(Opa::COVER);
    s.set_border_width(Stroke::NONE);
    s.set_radius(Radius::NONE);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 建一个导航页签（按钮 + 顶部选中条 + 左缘竖分隔 + 图标 + 文案）。
fn build_tab(parent: &Obj, page: NavPage, item_styles: &[Rc<Style>; 3]) -> Result<NavTab, LvglError> {
    let btn = TextButton::create(parent, "")?;
    btn.set_size(Dimens::NAV_ITEM_W, Dimens::NAV_ITEM_H);
    btn.set_pos(Dimens::NAV_ITEM_W * page.index() as i32, 0);
    btn.set_checkable(true);
    btn.add_style(&item_styles[0], StyleSelector::state_of(State::DEFAULT));
    btn.add_style(&item_styles[1], StyleSelector::state_of(State::CHECKED));
    btn.add_style(&item_styles[2], StyleSelector::state_of(State::PRESSED));

    // 顶部 4 px 选中条（UI §4.2；默认隐藏，选中时由 `Core::select` 点亮）。
    let bar = decor(
        &btn,
        Dimens::NAV_ITEM_W,
        Dimens::NAV_SELECT_BAR_H,
        &theme::card_head_bar(Palette::INFO),
    )?;
    bar.set_pos(0, NAV_BAR_Y);
    set_visible(&bar, false);

    // 左缘 1 px 竖分隔（UI §4.2：第 2–6 项左侧；第 1 项无）。
    let divider = if page.index() == 0 {
        None
    } else {
        let d = decor(&btn, NAV_DIVIDER_W, Dimens::NAV_ITEM_H, &theme::card_head_bar(Palette::DIVIDER))?;
        d.set_pos(0, 0);
        Some(d)
    };

    let icon_colors = [Palette::TEXT_WEAK, Palette::INFO];
    let text_colors = [Palette::TEXT_SECOND, Palette::TEXT_PRIMARY];
    let icon_styles = [
        theme::text(TextSlot::SectionTitle, icon_colors[0]),
        theme::text(TextSlot::SectionTitle, icon_colors[1]),
    ];
    let text_styles = [
        theme::text(TextSlot::Label, text_colors[0]),
        theme::text(TextSlot::Label, text_colors[1]),
    ];

    // 图标 / 文案标签**不预先挂颜色样式**：颜色是"选中态双通道"的一半，由
    // [`set_style_index`] 在 0（未选中）/ 1（选中）两档间切换（先挂一条固定色再叠加，
    // 会让 LVGL 样式表里永久留一条无用项 —— 见 `Core::select` 的第一次调用：`cur = usize::MAX`
    // ⇒ 直接挂目标档）。字体仍由档位样式给出（`icon_styles` / `text_styles` 内含 `set_text_font`）。
    let icon = Rc::new(Label::create_with_text(&btn, page.nav_icon())?);
    icon.remove_flag(ObjFlag::CLICKABLE);
    icon.remove_flag(ObjFlag::SCROLLABLE);
    icon.set_size(Dimens::ICON_SM, Dimens::ICON_SM);
    icon.set_pos(tab_text_x(page.nav_icon()), NAV_ICON_Y);
    icon.set_long_mode(LongMode::CLIP);

    let text = Rc::new(Label::create_with_text(&btn, page.nav_label())?);
    text.remove_flag(ObjFlag::CLICKABLE);
    text.remove_flag(ObjFlag::SCROLLABLE);
    text.set_size(tab_text_w(page.nav_label()), TextSlot::Label.px() as i32);
    text.set_pos(tab_text_x(page.nav_label()), NAV_TEXT_Y);
    text.set_long_mode(LongMode::CLIP);

    Ok(NavTab {
        btn,
        bar,
        divider,
        icon,
        text,
        icon_styles,
        text_styles,
        icon_colors,
        text_colors,
        icon_idx: Cell::new(usize::MAX),
        text_idx: Cell::new(usize::MAX),
    })
}

/// 页签文案的估算宽（CJK 逐字宽 = 字号；薄层无文本度量接口，与 `pages/**` 同口径）。
fn tab_text_w(label: &str) -> i32 {
    label.chars().count() as i32 * TextSlot::Label.px() as i32
}

/// 页签内容的水平居中 x（`(项宽 − 文案宽) / 2`）。
fn tab_text_x(label: &str) -> i32 {
    (Dimens::NAV_ITEM_W - tab_text_w(label)) / 2
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. 纯逻辑单测（**不触碰 LVGL** —— LVGL 非线程安全，触碰它的用例只能由
//    `src/lvgl/tests.rs::lvgl_core_bridge_chain` 经 `ui/tests.rs` 串行调起）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    // ── 导航目标（页签索引 ↔ 页面）──

    /// **往返互逆**：`ALL[i].index() == i` 且 `from_index(i) == Some(ALL[i])`。
    ///
    /// **改什么会让本条变红**：把 [`NavPage::index`] 里任意两个分支对调（例如
    /// `Logs => 3` / `Interlock => 2`）⇒ 往返断言当场红；把 `ALL` 的顺序改成非"页签从左到右"
    /// （如把 `Audit` 提到 `Logs` 前）⇒ `ALL[i].index() == i` 红。
    #[test]
    fn nav_page_index_round_trips() {
        assert_eq!(NavPage::ALL.len(), 6, "六页签常驻（UI §4.2）");
        for (i, p) in NavPage::ALL.into_iter().enumerate() {
            assert_eq!(p.index(), i, "{p:?} 的页签下标必须是它在 ALL 里的位次");
            assert_eq!(
                NavPage::from_index(i),
                Some(p),
                "`from_index` 与 `index` 必须互逆"
            );
        }
        assert_eq!(NavPage::from_index(6), None, "越界索引 ⇒ None（不 panic）");
    }

    /// 页眉 `返回` 键**仅 P2–P6 显示**（UI §4.1）。
    ///
    /// **改什么会让本条变红**：把 [`NavPage::shows_back`] 改成 `true`（P1 也显示返回键）、
    /// 或把 `matches!` 反过来（只有 P1 显示）。
    #[test]
    fn back_key_hidden_on_main_page_only() {
        assert!(!NavPage::Main.shows_back(), "P1 不显示返回键（UI §4.1）");
        for p in [
            NavPage::Config,
            NavPage::Logs,
            NavPage::Interlock,
            NavPage::Audit,
            NavPage::System,
        ] {
            assert!(p.shows_back(), "{p:?} 必须显示返回键");
        }
    }

    // ── 空闲超时（给定 `now` 序列 ⇒ 胶囊出现 / 文案 / 到 0 切页）──

    /// 60 s 空闲的**逐拍**语义：11 s 前无胶囊、10 s 起有胶囊、到 0 该切页。
    ///
    /// **改什么会让本条变红**：把 [`COUNTDOWN_WINDOW_SECS`] 从 10 改成 12（`t=50` 那一条仍是
    /// 胶囊 —— 不红；但 `t=49` 会变成"应显示胶囊"⇒ 红）；把 [`remaining_secs`] 的
    /// `div_ceil` 换成 `as_secs()`（向下取整）⇒ `t=50` 的 `secs` 变 10 仍红不了，但
    /// `t=49.5`（下面用 49 s + 500 ms 覆盖）会落到 10 ⇒ 与本条的"11 s 前无胶囊"冲突而红。
    #[test]
    fn idle_countdown_boundaries() {
        let base = t0();
        let timeout = Duration::from_secs(theme::Timing::IDLE_TIMEOUT_SECS);
        let at = |s: u64, ms: u64| base + Duration::from_secs(s) + Duration::from_millis(ms);

        // t=0：满时长，无胶囊。
        assert_eq!(remaining_secs(at(0, 0), base, timeout), 60);
        assert!(!shows_countdown(60));
        // t=49：还剩 11 s ⇒ 不在窗口内。
        assert_eq!(remaining_secs(at(49, 0), base, timeout), 11);
        assert!(!shows_countdown(11), "超时前 11 s 尚不显示胶囊");
        // t=49.5：还剩 10.5 s ⇒ 向上取整 = 11 ⇒ 仍不显示（若改向下取整会得到 10 ⇒ 红）。
        assert_eq!(remaining_secs(at(49, 500), base, timeout), 11);
        assert!(!shows_countdown(11));
        // t=50：还剩 10 s ⇒ 窗口起点。
        assert_eq!(remaining_secs(at(50, 0), base, timeout), 10);
        assert!(shows_countdown(10), "超时前 10 s 起显示胶囊（UI §4.3）");
        assert_eq!(countdown_text(10), "10 秒后返回主状态页");
        // t=59：还剩 1 s。
        assert_eq!(remaining_secs(at(59, 0), base, timeout), 1);
        assert!(shows_countdown(1));
        assert_eq!(countdown_text(1), "1 秒后返回主状态页");
        // t=60：到 0 ⇒ 该切页。
        assert_eq!(remaining_secs(at(60, 0), base, timeout), 0);
        assert!(should_return_home(0));
        assert!(!shows_countdown(0), "到 0 时胶囊不再显示（本拍即切页）");
        // 超时后再推进：仍停在 0（饱和，不回绕）。
        assert_eq!(remaining_secs(at(90, 0), base, timeout), 0);
    }

    /// 胶囊显隐判据与"到 0 切页"判据**互斥**（同一 `secs` 不可能两者都真）。
    ///
    /// **改什么会让本条变红**：把 [`shows_countdown`] 的条件写成 `secs <= 10`（漏掉 `secs > 0`）
    /// ⇒ `secs == 0` 时两者同时为真 ⇒ 红。
    #[test]
    fn countdown_and_return_are_mutually_exclusive() {
        for secs in 0..=70u64 {
            assert!(
                !(shows_countdown(secs) && should_return_home(secs)),
                "secs={secs} 不得既显胶囊又切页"
            );
        }
        assert!(shows_countdown(1) && !should_return_home(1));
        assert!(!shows_countdown(0) && should_return_home(0));
    }

    /// 倒计时文案的整数口径（UI §3.6 页眉超时行）。
    ///
    /// **改什么会让本条变红**：把 [`countdown_text`] 的格式串改成 `{secs} 秒后返回` 一类
    /// （少字 / 换字 / 换分隔符）。
    #[test]
    fn countdown_text_matches_spec_wording() {
        assert_eq!(countdown_text(10), "10 秒后返回主状态页");
        assert_eq!(countdown_text(3), "3 秒后返回主状态页");
    }

    // ── 页眉通道状态（连接 / 断开 / EDGE-20 双状态）──

    /// 三态派生 + 文案 / 图标 / 皮肤（颜色通道）齐备。
    ///
    /// **改什么会让本条变红**：把 `ChannelStatus::Down` 映射到 [`HeaderChannel::Connecting`]
    /// （断开显示成"连接中"）⇒ 第一条断言红；把 `HeaderChannel::Down::skin()` 改成
    /// `ChipSkin::SUCCESS`（红变绿）⇒ 皮肤断言红。
    #[test]
    fn header_channel_maps_status_to_three_channel_chip() {
        assert_eq!(
            HeaderChannel::from_status(ChannelStatus::Connected),
            HeaderChannel::Connected
        );
        assert_eq!(
            HeaderChannel::from_status(ChannelStatus::Init),
            HeaderChannel::Connecting
        );
        assert_eq!(
            HeaderChannel::from_status(ChannelStatus::Down),
            HeaderChannel::Down
        );
        // F14 三通道：文字 / 图标 / 颜色**三者都有**，且三态互不相同。
        let all: Vec<(usize, &str, &str, ChipSkin)> = [
            HeaderChannel::Connected,
            HeaderChannel::Connecting,
            HeaderChannel::Down,
        ]
        .into_iter()
        .map(|c| (c.index(), c.text(), c.icon(), c.skin()))
        .collect();
        let idx: Vec<usize> = all.iter().map(|x| x.0).collect();
        assert_eq!(idx, vec![0, 1, 2], "三态的下标必须是 0/1/2（唯一对应表）");
        let texts: Vec<&str> = all.iter().map(|x| x.1).collect();
        assert_eq!(
            texts,
            vec![
                TEXT_CHANNEL_OK,
                p1_status::TEXT_CHANNEL_CONNECTING,
                p1_status::TEXT_CHANNEL_DOWN
            ]
        );
        assert_eq!(
            HeaderChannel::Down.text(),
            p1_status::TEXT_CHANNEL_DOWN,
            "EDGE-20 的红通道文案（与 P1 页内通道条**同一份字面量**）"
        );
        assert_eq!(HeaderChannel::Down.skin(), ChipSkin::FAILURE, "断 = 红");
        assert_eq!(HeaderChannel::Connected.skin(), ChipSkin::SUCCESS, "通 = 绿");
        assert_ne!(
            HeaderChannel::Connected.icon(),
            HeaderChannel::Down.icon(),
            "图标通道必须可区分（● vs !）"
        );
    }

    // ── 页眉栅格常量自洽（**不改 LVGL 也能查的版式算术**）──

    /// 页眉各槽位**不重叠**、且都在画布内（SH3 的让位规则是它们的**行为**面）。
    ///
    /// **改什么会让本条变红**：把 [`HEADER_CHIP_X`] 调大到与倒计时胶囊相交（如 `X = 600`）、
    /// 或把 [`HEADER_TITLE_W_P1`] 的推导改成"不减呼吸缝"（P1 标题右缘压到通道胶囊上）。
    #[test]
    fn header_slots_are_disjoint_and_inside_canvas() {
        let p1_title = (HEADER_TITLE_X, HEADER_TITLE_X + HEADER_TITLE_W_P1);
        let paged_title = (HEADER_TITLE_X_PAGED, HEADER_TITLE_X_PAGED + HEADER_TITLE_W);
        let chip = (HEADER_CHIP_X, HEADER_CHIP_X + HEADER_CHIP_W);
        let badge = (HEADER_BADGE_X, HEADER_BADGE_X + HEADER_BADGE_W);
        let capsule = (HEADER_CAPSULE_X, HEADER_CAPSULE_X + HEADER_CAPSULE_W);
        let clock = (HEADER_CLOCK_X, HEADER_CLOCK_X + HEADER_CLOCK_W);

        // 返回键独占 x 12–76（UI §4.1）。
        assert_eq!(HEADER_BACK_X, 12);
        assert_eq!(HEADER_BACK_X + Dimens::TOUCH_CRITICAL, 76);
        // 标题与通道胶囊不相交（P1 与 P2–P6 两条）。
        assert!(p1_title.1 <= chip.0, "P1 标题不得压到通道胶囊");
        assert!(paged_title.1 <= chip.0, "分页标题不得压到通道胶囊");
        assert!(paged_title.0 > HEADER_BACK_X + Dimens::TOUCH_CRITICAL, "标题须在返回键右侧");
        // 通道胶囊与触摸角标不相交（二者可**同显**）。
        assert!(chip.1 + Dimens::GAP_GROUP <= badge.0, "通道胶囊与触摸角标之间留呼吸缝");
        // 触摸角标与时钟不相交。
        assert!(badge.1 <= clock.0, "触摸角标不得压到时钟");
        // 画布内。
        assert!(clock.1 <= Dimens::SCREEN_W - Dimens::SIDE_PAD);
        // 倒计时胶囊**必然**与通道胶囊 / 触摸角标相交 ⇒ SH3 的让位是必需的，不是可选的。
        assert!(
            capsule.0 < chip.1,
            "倒计时胶囊与通道胶囊相交 ⇒ SH3 让位规则必需（若二者不再相交，SH3 应被删除）"
        );
        assert!(
            capsule.0 < badge.1 && badge.0 < capsule.1,
            "倒计时胶囊与触摸角标相交 ⇒ SH3 让位规则必需（同上，防登记腐化）"
        );
    }

    /// 导航 6 项铺满可视宽且不越界（`NAV_ITEM_W` 是 theme 的单一真源，见 SH10）。
    ///
    /// **改什么会让本条变红**：把 `NAV_ITEM_W` 用到第 7 项、或把项宽调大到越界。
    #[test]
    fn nav_items_fit_canvas() {
        let total = Dimens::NAV_ITEM_W * NavPage::ALL.len() as i32;
        assert!(total <= Dimens::SCREEN_W, "6 项合计 {total} 必须 ≤ 画布宽");
        assert_eq!(Dimens::HEADER_H + Dimens::CONTENT_H + Dimens::NAV_H, Dimens::SCREEN_H);
    }
}
