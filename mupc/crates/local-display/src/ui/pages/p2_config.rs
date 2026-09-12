//! # `ui/pages/p2_config.rs` —— P2 配置页（12-MUPC v2.0 工作单元 **B2b-2**，含写操作）
//!
//! 设计 §6.2「P2 配置页（F9，含写操作）」逐条落地；UI 设计 §6.2 给版式、§7.3 给弹层、
//! §2.5 给强确认分级、§5.1 #6/#7/#8 给控件尺寸、§5.2 给四态色值。
//!
//! ## 本页与 P1 / P6 的三点结构差异（**契约见 `ui/pages/mod.rs` 的「补充（B2b-2）」**）
//!
//! 1. **不由 [`PageInput`] 驱动**：P2 的数据来自**控制通道**（`GET /v1/console/config`，
//!    另 9811 端口），与 1 Hz 显示帧无关。注入入口见
//!    [`P2ConfigPage::set_config`] / [`P2ConfigPage::set_unavailable`] /
//!    [`P2ConfigPage::set_submitting`] / [`P2ConfigPage::show_result`]；
//!    **降级态只由注入值驱动**（本页不读时钟、不自行发请求）。
//! 2. **页根不是滚动容器**：UI §6.2 线框 `Y624 ┌ 固定操作条（不随滚动）┐` ⇒ 页根是普通容器，
//!    内部再分两层 —— 上为**滚动视口**（`992 × (624 − 72) = 992 × 552`，与 UI 线框「552 px
//!    视口」一致）、下为**固定操作条**（`992 × 72`）。故 `pages::page_root()`（"页根自身即
//!    滚动容器"）**未改语义**，本页自建视口。
//! 3. **「意图」与「请求」分离**：设计 §6.2 末行（UI ↔ 后端 确认-审计链路）——
//!    **确认完成前不发出任何请求**（未确认 = 无网络动作）；确认完成后本页经
//!    [`P2ConfigPage::set_on_submit`] 把 [`ConfigPatch`] **交回外部**，
//!    `request_id`(uuid) / `issued_at_ms` 由 B3 的 `console.rs` 生成（**本页不含 uuid /
//!    时间戳 / HTTP**）。
//!
//! ## 只读字段（设计 §6.2「只读字段」行 / PL-4）
//!
//! `editable == false`（`display.bind_addr` / `display.control_bind_addr`：回环是安全红线）：
//! 控件 `disabled` **且**附说明行 [`TEXT_READONLY_NOTE`]；字段**仍出现在列表中**
//! （以**可见性**换取现场可核查性）。
//!
//! ## ⚠️ 已知偏差登记（**独立编号 `PD`** —— `D1~D9` 是页面层编号、`CD1~CD7` 是
//! `ui/controls.rs` 编号，本表**不与二者冲突**；本表只登记本页的屏文 / 尺寸与契约不一致处）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 计划收口单元 |
//! |---|----------------------|------|--------------|
//! | PD1 | `gateway.listen_addr` 标签取 **`本机监听地址 · IEC 104`**（PM 裁定串为「本机监听地址（IEC 104）」） | **缺字**：全角括号 `（`/`）`（U+FF08/FF09）不在生成字体 cmap 内 ⇒ 照抄即豆腐块。取 cmap 内的 `·`（U+00B7）作分隔，语义不变（**仍是"本机监听地址"，不是"对端 IP"**） | 字库收口批（扩 §3.6 字符集并重跑 `gen_fonts.sh`）后逐字改回 PM 原串 |
//! | PD2 | `display.bind_addr` / `display.control_bind_addr` 标签取 **`本机监听地址 · 仅本机`**（PM 裁定串为「本机服务地址（仅回环 127.0.0.1）」） | 同上：`服`(U+670D) / `务`(U+52A1) / `环`(U+73AF) / 全角括号**均不在 cmap 内** ⇒ 只能改述。字面量与 P6 的 `TEXT_SERVICE_ADDR` **同源共用**（同一实体：本机回环绑定 —— 不另造第二份真源） | 同 PD1 |
//! | PD3 | 只读说明行取 **`仅本机访问 · 不可修改`**（设计写「仅本机回环，不可修改」） | 同上：`环`(U+73AF) 与全角逗号 `，`(U+FF0C) **不在 cmap 内** | 同 PD1 |
//! | PD4 | **卡内左右内边距 = 17**（描边 1 + `Dimens::GAP_MIN` 16），UI §6.2 写 **20** | 与 P6 同口径（`theme::card()` 的既有内边距 + 描边）；且 `theme` 无 20 px 档，**不为凑 3 px 引入裸数值**。后果：字段名 x=33（UI 写 36）、行型 A 控件右缘 x=991（UI 写 988），**整体 3 px** | 与 UI §6.2 一并复核（若 PM 要求逐像素，须先在 `theme` 增档） |
//! | PD5 | **行型 B 高 = 122**（上缝 16 + 字段名 26 + 缝 16 + 控件 64），UI §6.2 写 **120** | `theme` 无 2 px 档（同 `pages/mod.rs` 的 **D3** 同款理由） | 同 D3（随 theme 缺口上收一并处理） |
//! | PD6 | **字段错误态**取「行左缘 **4 px** 危险色竖条 + 字段名 / 原因**红字**」双通道；UI §5.2 写「描边 `#FF6B6B` **2 px** + 下方 24 px 红字原因」 | `theme.rs` **本批禁改**，其现有的危险描边只有 3 px 单边（`card_alert_*`）与 2 px **按钮**描边，**没有**「2 px 全框危险」样式；本页不得内联裸色值 ⇒ 复用既有的 `card_head_bar(Palette::DANGER)`（4 px 竖条）+ 红字 | `theme` 收口批：补 `field_error_frame()` 后改为整框 2 px |
//! | PD7 | 失败原因就地显示在**页顶提示行**（`Y80` 槽位，与页说明行**同槽互斥**）与**字段行**；UI §6.2 流程 6 写「**弹层内**就地显示 message」 | `ConfirmDialog`（`components.rs`，**本批禁改**）的「影响范围」文案在构造期固定，**没有**可变错误文案口；且弹层在 `show_result` 时即关闭（`ConfirmDialog::close` 不得在 LVGL 事件回调内调用，见其模块文档） | `components.rs` 补 `set_impact()` 后改回弹层内显示 |
//! | PD8 | 「保存中…」取 **`保存中...`**（三个 ASCII `.`，U+002E） | `…`(U+2026) **不在 cmap 内**；`.` 在（且 `components.rs` 的 `DOTS` 截断同款） | 同 PD1 |
//! | PD9 | `WriteMode::FullRewrite` 的 Toast 取 **`配置已保存 · 原有文字已不存在`**（设计 §4.3.2.1 写「配置文件已整体重写，原有注释不再保留」） | **缺字**：`整`(U+6574) / `写`(U+5199) / `注`(U+6CE8) / `留`(U+7559) / `再`(U+518D) 均不在 cmap 内 ⇒ 无法逐字照抄。改写串保留两条语义：**已保存** + **原有文字（注释）已不存在** | 同 PD1 |
//! | PD10 | **`WriteMode::FullRewrite` 的 Toast 由 `set_config` 统一触发**（`show_result` 成功路径不再叠加"保存成功"Toast —— UI §7.2「同一时刻仅 1 条」，**取信息量更大的那条**） | `ConfigView.write_mode` 的契约语义即「最近一次落盘写模式，`FullRewrite` 时 UI 须明示」（EDGE-23）⇒ 任何携带该值的视图都该明示，故收在唯一入口 | 无（有意） |
//! | PD11 | 「任一字段 `requires_reconnect` ⇒ L2+」按**字面**落地：判定用**视图内全部字段**（不限于本次改动）；`defaults_patch()` 覆盖**全部 `editable` 字段**（含已等于默认值者）；「涉及：」列表 = 全部 `requires_reconnect` 字段 | 设计 §6.2 保存行原文为「**无字段** `requires_reconnect` → L1；**任一字段** `requires_reconnect=true` → L2+」（与"本次改动"不同口径）；`ConfigPatch` 契约文档原文为「**全字段默认值**即『恢复默认值』的载荷」 | 无（**按字面**；若 PM 裁定改为"仅本次改动"，改 [`save_level`] / [`defaults_patch_of`] 两处即可） |
//! | PD12 | **只读字段不进 `defaults_patch()`** | `ConfigField::validate_value()`（契约）对 `editable=false` **一律拒绝**（"只读，不可修改"）⇒ 把只读字段放进 `changes` 会让**整个**恢复请求被后端二次校验打回（PL-4 的红线字段本就不可写） | 无（**有意**；与 PD11 的"全字段"口径差集即只读字段） |
//!
//! ## 纪律（逐条对应设计要求）
//!
//! - **零文本输入**（红线）：本文件只出现 `lv_button` / `lv_label` / `lv_buttonmatrix`
//!   （经 `Stepper` / `Ipv4Stepper` / `SegmentedControl` / `TextButton`）——
//!   `lv_keyboard` / `lv_textarea` / `lv_spinbox` **零出现**（`ui/tests.rs::p2_static_constraints`）；
//! - **无裸色值 / 裸尺寸**：一切外观经 [`theme`]；本页专属栅格常量集中在下方 `const` 块，
//!   **逐条由 theme 常量推导**（`ui/tests.rs::ui_layout_setters_use_theme_constants` /
//!   `ui_const_i32_definitions_derive_from_theme` 两张网扫本文件）；
//! - **所有权纪律**：凡 `Obj::create` / `TextButton::create` / `Label::create_*` / `Stepper` /
//!   `ConfirmDialog` / `Toast` 返回的**拥有型句柄**一律存进结构体字段（`FieldRow` /
//!   `GroupCard` / [`Core`]），**绝不**只作局部变量 —— 否则构造器返回时即被 `Drop`、
//!   LVGL 级联删除整棵子树（表现为"界面空白但无报错"，B1 出过此类 UAF 级缺陷）；
//! - **回调纪律**：回调内**不 panic**（无 `unwrap` / 无越界索引）、不做阻塞 I/O、
//!   不删除自身宿主；**不在 LVGL 事件回调内关闭弹层**（`ConfirmDialog::close` 的文档要求）
//!   —— 取消走 [`P2ConfigPage::tick`] 的延迟关闭，`show_result` / `set_config` 在事件之外；
//! - **不提供跨线程 API**：本页所有类型都含 LVGL 句柄（自动 `!Send` / `!Sync`），全部调用
//!   必须在事件循环线程内（设计 §5.2 不变量 4）。

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;

use mupc_display_proto::{
    ConfigField, ConfigGroup, ConfigKind, ConfigPatch, ConfigView, ControlCode, ControlResponse,
    FieldError, PatchSource, WriteMode,
};
use serde_json::{Map as JsonMap, Value};

use crate::lvgl::obj::Obj;
use crate::lvgl::style::Style;
use crate::lvgl::widgets::{self, Label, LongMode, ScrollContainer, TextButton};
use crate::lvgl::LvglError;
use crate::ui::components::{ConfirmDetail, ConfirmDialog, ConfirmSpec, Stepper, Toast, ToastTone};
use crate::ui::controls::{Ipv4Stepper, SegmentedControl};
use crate::ui::pages::p6_system::TEXT_SERVICE_ADDR;
use crate::ui::pages::{
    decor, display_safe, label, layout_box, set_style_index, set_visible, text_label,
};
use crate::ui::theme::{self, ConfirmLevel, Dimens, Palette, TextSlot};

// ═══════════════════════════════════════════════════════════════════════════
// 1. 上屏文案（UI §3.6 P2 行；**落笔前逐字在 `fonts/lv_font_cmap.txt` 核对**）
//
// 与契约串的偏差逐条登记在文件头 `PD1~PD9`（缺字改写 / 缺字号改写），此处只放**成品串**。
// 码表覆盖率走查见 `ui/tests.rs::ui_texts_covered_by_font_cmap`（基线 = 生成字体的实际 cmap，
// 待查集合 = 扫 `ui/**` 源码字面量）。
// ═══════════════════════════════════════════════════════════════════════════

/// 保存按钮（UI §3.6 P2「控件 / 状态」行）。
pub const TEXT_SAVE: &str = "保存";
/// 保存中按钮（UI §3.6；⚠️ `…` 不在 cmap 内 —— 取三个 ASCII `.`，见 **PD8**）。
pub const TEXT_SAVING: &str = "保存中...";
/// 恢复默认值按钮 + 其确认弹层标题（UI §3.6 P2 / §6.2 流程 8）。
pub const TEXT_RESET_DEFAULT: &str = "恢复默认值";
/// 页顶说明行（UI §6.2 线框 `Y80`；全角逗号 `，` 不在 cmap 内 ⇒ 取 `·`）。
pub const TEXT_PAGE_NOTE: &str = "修改保存后立即生效 · 无需重启装置";
/// 只读字段的说明行（设计 §6.2「只读字段」行；⚠️ 见 **PD3**）。
pub const TEXT_READONLY_NOTE: &str = "仅本机访问 · 不可修改";
/// `gateway.listen_addr` 的标签（**PM 裁定**，设计 §6.2 / UI 附录 B U-1；⚠️ 见 **PD1**）。
///
/// **不得**表述为「对端 IP / 远程主站地址」—— 现网 `mupc_gateway::Iec104Server` 是**服务端**
/// （`bind` 后监听、接受调度主站连接），不存在"对端 IP"概念（设计 §4.3.4 / R-08）。
pub const TEXT_LISTEN_ADDR: &str = "本机监听地址 · IEC 104";
/// 回环服务地址字段的标签（`display.bind_addr` / `display.control_bind_addr`；⚠️ 见 **PD2**）。
///
/// **与 P6 的 [`TEXT_SERVICE_ADDR`] 同源共用**：两者是**同一实体**（本机回环绑定），
/// 不另造第二份真源。**与 [`TEXT_LISTEN_ADDR`] 分列**（本机回环服务地址 ≠ IEC 104 监听地址）。
pub const TEXT_LOOPBACK_ADDR: &str = TEXT_SERVICE_ADDR;
/// 保存确认弹层标题（UI §6.2 流程 3）。
pub const TEXT_DIALOG_TITLE_SAVE: &str = "确认保存运行参数";
/// 保存确认的「影响范围」段（UI §6.2 流程 3；全角逗号 ⇒ `·`）。
pub const TEXT_IMPACT_SAVE: &str = "修改将立即生效 · 无需重启装置";
/// 恢复默认值确认的「影响范围」段（设计 §6.2 恢复默认值行；`为` 不在 cmap 内 ⇒ 去之，语义不变）。
pub const TEXT_IMPACT_RESET: &str = "全部运行参数将恢复默认值并立即生效";
/// 保存成功 Toast（UI §3.6 全局；全角逗号 ⇒ `·`）。
pub const TEXT_TOAST_OK: &str = "保存成功 · 已生效";
/// 保存失败 Toast（UI §3.6 P2「保存失败」）。
pub const TEXT_TOAST_FAIL: &str = "保存失败";
/// `full_rewrite` 警示 Toast（EDGE-23 / 设计 §4.3.2.1；⚠️ 见 **PD9**）。
pub const TEXT_TOAST_FULL_REWRITE: &str = "配置已保存 · 原有文字已不存在";
/// 审计不可写 Toast（UI §8.3 `审计不可写（EDGE-18）`；fail-closed，**操作未执行**）。
pub const TEXT_AUDIT_UNAVAILABLE: &str = "审计不可用 · 操作未执行";
/// 配置不可用（控制通道取不到 `GET /v1/console/config` 时的降级标题；无对应 `UnavailableKind`，
/// 见 [`show_unavailable`] 的说明）。
pub const TEXT_CONFIG_UNAVAILABLE: &str = "配置不可用";
/// 取值范围分隔符（UI §6.2 行型 A 的「`1 – 65535`」；`–` U+2013 在 cmap 内）。
pub const TEXT_RANGE_SEP: &str = " – ";
/// Toast 图标：成功（UI §3.6 声明符号集内的 `✓` U+2713）。
pub const ICON_OK: &str = "✓";
/// Toast 图标：失败（UI 用 `✕`，但 U+2715 **不在 cmap 内** ⇒ 取 `!`）。
pub const ICON_FAIL: &str = "!";
/// Toast 图标：警示（`⚠` U+26A0，在 cmap 内；与 `WarnBanner` 同款）。
pub const ICON_WARN: &str = "⚠";

/// 本页上屏的**全部固定文案**（供 `ui/tests.rs::ui_texts_covered_by_font_cmap` 做"清册 ↔ 源码
/// 字面量"一致性走查 —— 清册**不是**覆盖率基线，见该用例文档）。
pub const ALL_TEXTS: &[&str] = &[
    TEXT_SAVE,
    TEXT_SAVING,
    TEXT_RESET_DEFAULT,
    TEXT_PAGE_NOTE,
    TEXT_READONLY_NOTE,
    TEXT_LISTEN_ADDR,
    TEXT_LOOPBACK_ADDR,
    TEXT_DIALOG_TITLE_SAVE,
    TEXT_IMPACT_SAVE,
    TEXT_IMPACT_RESET,
    TEXT_TOAST_OK,
    TEXT_TOAST_FAIL,
    TEXT_TOAST_FULL_REWRITE,
    TEXT_AUDIT_UNAVAILABLE,
    TEXT_CONFIG_UNAVAILABLE,
    TEXT_RANGE_SEP,
    ICON_OK,
    ICON_FAIL,
    ICON_WARN,
];

// ═══════════════════════════════════════════════════════════════════════════
// 2. 栅格常量（UI §6.2；**全部由 theme 常量推导** —— 本页不出现裸规格值）
// ═══════════════════════════════════════════════════════════════════════════

/// 卡内容区原点相对卡外缘的偏移（描边 1 + 内边距 16）—— 与 `p6_system.rs` 同口径（见 PD4）。
const CARD_INSET: i32 = theme::Stroke::THIN + Dimens::GAP_MIN;
/// 卡内可用宽。
const INNER_W: i32 = Dimens::CONTENT_W - 2 * CARD_INSET;
/// 卡头高（UI §6.2「卡头 44 px（分组名 28 px + 左侧 4 px 竖条）」= 28 + 同组缝 16）。
const CARD_HEAD_H: i32 = TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN;
/// 行型 A 高（UI §6.2「行型 A … 88 px」= 控件高 64 + 约束提示行 24）。
const ROW_A_H: i32 = Dimens::STEPPER_H + TextSlot::Body.px() as i32;
/// 行型 A 文本块（字段名 + 提示行）在行内的 y（垂直居中）。
const ROW_A_TEXT_Y: i32 = theme::center_offset(
    ROW_A_H,
    TextSlot::Label.px() as i32 + TextSlot::Body.px() as i32,
);
/// 行型 A 控件 y（行内垂直居中）。
const ROW_A_CTRL_Y: i32 = theme::center_offset(ROW_A_H, Dimens::STEPPER_H);
/// 行型 B 高（UI §6.2「行型 B … 120 px」；实算 122，见 **PD5**）。
const ROW_B_H: i32 = Dimens::GAP_MIN + TextSlot::Label.px() as i32 + Dimens::GAP_MIN + Dimens::STEPPER_H;
/// 行型 B 字段名 y（UI §6.2「字段名 26 px (x36, y+16)」）。
const ROW_B_LABEL_Y: i32 = Dimens::GAP_MIN;
/// 行型 B 控件 y（字段名行 + 同组缝）。
const ROW_B_CTRL_Y: i32 = ROW_B_LABEL_Y + TextSlot::Label.px() as i32 + Dimens::GAP_MIN;
/// 行型 B 控件 x（**负偏移**：抵消卡内边距 ⇒ 控件与卡外缘齐宽，UI §6.2「控件独占次行」）。
///
/// 为何必须负偏移：`Ipv4Stepper` 整件宽 = [`Dimens::CONTENT_W`]（=`ui/controls.rs` 的 CD2），
/// 而卡内容区只有 [`INNER_W`]（= 958）⇒ 若从 0 起排，右端 34 px 会被父对象裁剪（LVGL 子对象
/// 裁剪到父的**外缘**坐标，见 `ui/tests.rs::pages_chain` 的同类口径）。
const ROW_B_CTRL_X: i32 = -CARD_INSET;
/// 状态槽宽（约束提示 / 字段错误原因；两者**同槽互斥**，避免出错时整页重排）。
const STATUS_W: i32 = INNER_W / 2;
/// 状态槽 x（行型 B：与字段名**同行右侧**；行型 A 用 0）。
const STATUS_X: i32 = Dimens::CONTENT_W / 3;
/// 字段行左缘危险竖条宽（复用 `theme` 的"卡头 4 px 竖条"口径；见 **PD6**）。
const ERROR_BAR_W: i32 = Dimens::ACCENT_BAR;
/// 固定操作条高（UI §6.2 线框 `y 624–696` ⇒ 72 = 主按钮高 64 + 内容区上内边距 8）。
const ACTION_BAR_H: i32 = Dimens::BTN_H_PRIMARY + Dimens::CONTENT_PAD_TOP;
/// 滚动视口高（UI §6.2「552 px 视口」= 内容区 624 − 操作条 72）。
const SCROLL_H: i32 = Dimens::CONTENT_H - ACTION_BAR_H;
/// 操作条内左右内边距。
const ACTION_PAD: i32 = Dimens::GAP_MIN;
/// 操作条内按钮 y（垂直居中）。
const ACTION_BTN_Y: i32 = theme::center_offset(ACTION_BAR_H, Dimens::BTN_H_PRIMARY);
/// 分组卡之间的缝（跨区 24，UI §3.5「区块间呼吸缝」）。
const CARD_GAP: i32 = Dimens::GAP_SECTION;
/// 页说明行高（正文 24 + 下缝 16）。
const NOTE_H: i32 = TextSlot::Body.px() as i32 + Dimens::GAP_MIN;
/// 页说明行 y（UI §6.2 线框 `Y80` ⇒ 页内 = 8 = 内容区上内边距）。
const NOTE_Y: i32 = Dimens::CONTENT_PAD_TOP;
/// 首个分组卡 y。
const FIRST_CARD_Y: i32 = NOTE_Y + NOTE_H;
/// 单个 `Stepper` 整件宽（`−` + 值区 + `＋`；UI §5.1 #6）。
const STEPPER_TOTAL_W: i32 = Dimens::STEPPER_BTN_W * 2 + Dimens::STEPPER_VALUE_W;

// ═══════════════════════════════════════════════════════════════════════════
// 3. 纯逻辑（**不触碰 LVGL** ⇒ 可独立单测；页内逻辑一律经这里，保证可离线复现）
// ═══════════════════════════════════════════════════════════════════════════

/// 一个字段的**变更明细**（弹层「将修改的字段」段的原始材料，见 `ConfirmDetail`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeDetail {
    /// 稳定字段键（审计 `target`）。
    pub key: String,
    /// 上屏字段名（[`field_label_text`] 的结果）。
    pub label: String,
    /// 旧值 / 当前值（`text_weak`）。
    pub before: String,
    /// 新值（`#2FDB8A`）。
    pub after: String,
}

/// 遍历视图内全部字段（组序 × 组内序）—— 唯一的口径，避免各处再写嵌套循环。
pub fn iter_fields(view: &ConfigView) -> impl Iterator<Item = &ConfigField> {
    view.groups.iter().flat_map(|g| g.fields.iter())
}

/// 字段的**上屏标签**（设计 §6.2「监听地址字段口径」PM 裁定行 + UI 附录 B U-1）。
///
/// **契约的 `label` 是默认路径**（元数据驱动 UI）；仅当键命中**PM 裁定表**
/// （[`LABEL_OVERRIDES`]）时改用它 —— 目的是让"不得表述为『对端 IP / 远程主站地址』"这条
/// 裁定在**屏上**成立（后端标签漂移时页仍不违规）。
pub fn field_label_text(field: &ConfigField) -> String {
    LABEL_OVERRIDES
        .iter()
        .find(|(key, _)| *key == field.key)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| field.label.clone())
}

/// 配置字段的**机器键**（**不上屏** —— 只用于与 `ConfigField.key` 比对）。
///
/// 存在的唯一理由：`ui/tests.rs::ui_texts_covered_by_font_cmap` 把 `ui/**` 里**所有**字符串
/// 字面量一律当"上屏候选"逐字查字形（宁可多查），而机器键是小写 ASCII（`gateway.listen_addr`
/// 里的 `g`/`a`/`t`/`e`/`w` 等**在生成字体里没有字形**）。经本函数标注 = 声明"这个字面量
/// **从不进 `lv_label`**"，与 `invalid_argument(` / `env!(` 一类**同一条**非屏显口径
/// （见 `ui/tests.rs::NON_DISPLAY_SINKS`；那里的登记是**逐条列出、不靠正则猜**）。
///
/// ⚠️ 它不是"为了过网而把文案写残"：键本来就**不上屏**，此处只是把这一事实**写在调用点**。
const fn config_key(key: &'static str) -> &'static str {
    key
}

/// PM 裁定的标签口径覆盖表（键 → 上屏标签）—— **逐条登记、只增不改**。
///
/// | 键 | 标签 | 出处 |
/// |----|------|------|
/// | `gateway.listen_addr` | [`TEXT_LISTEN_ADDR`] | 设计 §6.2 PM 裁定行（R-08 / U-1） |
/// | `display.bind_addr` | [`TEXT_LOOPBACK_ADDR`] | 设计 §6.2 只读字段行（PL-4 回环红线） |
/// | `display.control_bind_addr` | [`TEXT_LOOPBACK_ADDR`] | 同上 |
pub const LABEL_OVERRIDES: [(&str, &str); 3] = [
    (config_key("gateway.listen_addr"), TEXT_LISTEN_ADDR),
    (config_key("display.bind_addr"), TEXT_LOOPBACK_ADDR),
    (config_key("display.control_bind_addr"), TEXT_LOOPBACK_ADDR),
];

/// 值 → 上屏文本（弹层明细与断言口径的唯一出口）。
///
/// - `Ipv4`：字符串原样（点分十进制全在 cmap 内）；
/// - `U16`/`U64`：十进制整数 + 可选单位（单位由契约给，如 `秒`）；
/// - `Enum`：命中选项 → **选项标签**；未命中 → 原值（不静默改写成"未知值"）。
pub fn format_value(kind: &ConfigKind, value: &Value, unit: Option<&str>) -> String {
    match kind {
        ConfigKind::Ipv4 => value.as_str().unwrap_or_default().to_string(),
        ConfigKind::U16 { .. } | ConfigKind::U64 { .. } => {
            let n = int_of(value).unwrap_or_default();
            match unit.filter(|u| !u.is_empty()) {
                Some(u) => format!("{n} {u}"),
                None => n.to_string(),
            }
        }
        ConfigKind::Enum { options } => {
            let raw = value.as_str().unwrap_or_default();
            options
                .iter()
                .find(|o| o.value == raw)
                .map(|o| o.label.clone())
                .unwrap_or_else(|| raw.to_string())
        }
    }
}

/// 约束提示（UI §6.2 行型 A「字段名下方 24 px `text_weak` 显示约束『1 – 65535』」）。
///
/// `Ipv4` / `Enum` 返回 `None`：UI §6.2 的行型 B 与 Enum 行**不画**约束提示
/// （四段 0–255 与选项本身已由控件表达）。
pub fn range_hint(kind: &ConfigKind, unit: Option<&str>) -> Option<String> {
    let (lo, hi) = match kind {
        ConfigKind::U16 { min, max, .. } => (u64::from(*min), u64::from(*max)),
        ConfigKind::U64 { min, max, .. } => (*min, *max),
        ConfigKind::Ipv4 | ConfigKind::Enum { .. } => return None,
    };
    let base = format!("{lo}{TEXT_RANGE_SEP}{hi}");
    Some(match unit.filter(|u| !u.is_empty()) {
        Some(u) => format!("{base} {u}"),
        None => base,
    })
}

/// 注入视图 → **当前值**快照（`key → value`）。
pub fn initial_values(view: &ConfigView) -> BTreeMap<String, Value> {
    iter_fields(view)
        .map(|f| (f.key.clone(), f.value.clone()))
        .collect()
}

/// 草稿补丁：**只含值与视图不同**的字段（`from = Edit`）。
///
/// "不同"由 `serde_json::Value` 的相等判定（整数一律 `PosInt`，故 `2404` 与 `2404u64` 相等）。
pub fn draft_patch(view: &ConfigView, current: &BTreeMap<String, Value>) -> ConfigPatch {
    let mut changes = JsonMap::new();
    for f in iter_fields(view) {
        let Some(now) = current.get(&f.key) else {
            continue;
        };
        if *now != f.value {
            changes.insert(f.key.clone(), now.clone());
        }
    }
    ConfigPatch {
        changes,
        from: PatchSource::Edit,
    }
}

/// 是否有未保存修改（= 草稿非空）。
pub fn is_dirty(view: &ConfigView, current: &BTreeMap<String, Value>) -> bool {
    !draft_patch(view, current).changes.is_empty()
}

/// 恢复默认值补丁：**全部 `editable` 字段** → 其 `default`（`from = ResetDefault`）。
///
/// **只读字段一律不进 `changes`**（见 **PD12**）：契约 [`ConfigField::validate_value`] 对
/// `editable=false` **一律拒绝**，混进去会让整个恢复请求被后端二次校验打回。
pub fn defaults_patch_of(view: &ConfigView) -> ConfigPatch {
    let mut changes = JsonMap::new();
    for f in iter_fields(view).filter(|f| f.editable) {
        changes.insert(f.key.clone(), f.default.clone());
    }
    ConfigPatch {
        changes,
        from: PatchSource::ResetDefault,
    }
}

/// 视图内是否存在 `requires_reconnect` 字段（配置写入是否会瞬断链路的判据）。
pub fn has_reconnect_field(view: &ConfigView) -> bool {
    iter_fields(view).any(|f| f.requires_reconnect)
}

/// 保存的确认强度（设计 §6.2 保存行 / UI §2.5）：
/// **无字段** `requires_reconnect` → [`ConfirmLevel::L1`]（单击生效）；
/// **任一字段** `requires_reconnect=true` → [`ConfirmLevel::L2Plus`]（危险色 + 长按 1.0 s +
/// **必出 `WarnBanner`**）。
pub fn save_level(view: &ConfigView) -> ConfirmLevel {
    if has_reconnect_field(view) {
        ConfirmLevel::L2Plus
    } else {
        ConfirmLevel::L1
    }
}

/// 恢复默认值的确认强度（设计 §6.2 恢复默认值行）：**最低 L2**（生效性写、不得只有间距保护）；
/// 涉及 `requires_reconnect` 字段时升为 [`ConfirmLevel::L2Plus`]（追加 `WarnBanner`）。
pub fn reset_level(view: &ConfigView) -> ConfirmLevel {
    if has_reconnect_field(view) {
        ConfirmLevel::L2Plus
    } else {
        ConfirmLevel::L2
    }
}

/// L2+ 的「涉及：<字段名列表>」—— 全部 `requires_reconnect` 字段的上屏标签（UI §2.5）。
pub fn reconnect_field_labels(view: &ConfigView) -> Vec<String> {
    iter_fields(view)
        .filter(|f| f.requires_reconnect)
        .map(field_label_text)
        .collect()
}

/// 保存路径的变更明细（逐字段「旧值 → 新值」，UI §7.3 明细列表）。
pub fn save_details(view: &ConfigView, current: &BTreeMap<String, Value>) -> Vec<ChangeDetail> {
    let mut out = Vec::new();
    for f in iter_fields(view) {
        let Some(now) = current.get(&f.key) else {
            continue;
        };
        if *now == f.value {
            continue;
        }
        out.push(ChangeDetail {
            key: f.key.clone(),
            label: field_label_text(f),
            before: format_value(&f.kind, &f.value, f.unit.as_deref()),
            after: format_value(&f.kind, now, f.unit.as_deref()),
        });
    }
    out
}

/// 恢复默认值路径的变更明细（「字段：当前值 → 默认值」；范围与 [`defaults_patch_of`] 一致）。
pub fn reset_details(view: &ConfigView, current: &BTreeMap<String, Value>) -> Vec<ChangeDetail> {
    let mut out = Vec::new();
    for f in iter_fields(view).filter(|f| f.editable) {
        let Some(now) = current.get(&f.key) else {
            continue;
        };
        out.push(ChangeDetail {
            key: f.key.clone(),
            label: field_label_text(f),
            before: format_value(&f.kind, now, f.unit.as_deref()),
            after: format_value(&f.kind, &f.default, f.unit.as_deref()),
        });
    }
    out
}

/// 配置不可用时的提示文本（`reason` 为空则只显标题）。
pub fn unavailable_text(reason: &str) -> String {
    let r = display_safe(reason.trim());
    if r.is_empty() {
        TEXT_CONFIG_UNAVAILABLE.to_string()
    } else {
        format!("{TEXT_CONFIG_UNAVAILABLE} · {r}")
    }
}

/// `Value` → `i64`（`U16`/`U64` 两类整数；负数与越界由 `Stepper` 自身 clamp）。
fn int_of(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_u64().and_then(|u| i64::try_from(u).ok()))
        .or_else(|| v.as_f64().map(|f| f as i64))
}

/// 字段的初始四段（`Ipv4`）：值不可解析时退回 `default`，两者都不可解析才退回 `0.0.0.0`
/// （**不静默**：往 stderr 留一条诊断）。
fn octets_of(field: &ConfigField) -> [u8; 4] {
    let parse = |v: &Value| {
        v.as_str()
            .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok())
            .map(|a| a.octets())
    };
    parse(&field.value)
        .or_else(|| parse(&field.default))
        .unwrap_or_else(|| {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr(),
                "P2 配置页：IPv4 字段 `{}` 的值与默认值都无法解析，按 0.0.0.0 显示（不静默）",
                field.key
            );
            [0, 0, 0, 0]
        })
}

/// 字段的初始整数值（`U16`/`U64`）；不可得时取 `default`，再不可得取 0。
fn initial_int(field: &ConfigField) -> i64 {
    int_of(&field.value)
        .or_else(|| int_of(&field.default))
        .unwrap_or_default()
}

/// `Enum` 值 → 选项下标（未命中取 0）。
fn enum_index(kind: &ConfigKind, v: &Value) -> usize {
    if let ConfigKind::Enum { options } = kind {
        if let Some(s) = v.as_str() {
            if let Some(i) = options.iter().position(|o| o.value == s) {
                return i;
            }
        }
    }
    0
}

/// 行高（由 `kind` 驱动，UI §6.2 的两版式）。
fn row_height(kind: &ConfigKind) -> i32 {
    match kind {
        ConfigKind::Ipv4 => ROW_B_H,
        _ => ROW_A_H,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 字段行 / 分组卡（**拥有型句柄的存活锚点**，见模块文档「所有权纪律」）
// ═══════════════════════════════════════════════════════════════════════════

/// 一个字段的输入控件（三类，由 `ConfigField.kind` 驱动）。
enum FieldControl {
    /// `Ipv4` → 四段步进 + 汇总标签（UI §5.1 #7）。
    Ipv4(Rc<Ipv4Stepper>),
    /// `U16`/`U64` → 受约束步进器（UI §5.1 #6）。
    Int(Rc<Stepper>),
    /// `Enum` → 分段控件（UI §5.1 #4；**不用 `lv_dropdown`** —— §5.1 未选它，且其展开列表
    /// 在离屏不可断言）。
    Enum(Rc<SegmentedControl>),
}

impl FieldControl {
    /// 当前值（**读自控件** —— 唯一真源，不另存影子副本）。
    fn current(&self, kind: &ConfigKind) -> Value {
        match self {
            Self::Ipv4(s) => Value::from(ipv4_text(s.octets())),
            Self::Int(s) => Value::from(s.value()),
            Self::Enum(s) => {
                let idx = s.raw_selected().unwrap_or_else(|| s.selected());
                let raw = match kind {
                    ConfigKind::Enum { options } => options
                        .get(idx)
                        .map(|o| o.value.clone())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                Value::from(raw)
            }
        }
    }

    /// 编程式设值（**不触发 `on_change`** —— `Stepper` / `Ipv4Stepper` / `SegmentedControl`
    /// 的 `set_*` 都是这个语义，故调用方必须自己做变更后的簿记）。
    fn set(&self, kind: &ConfigKind, v: &Value) {
        match self {
            Self::Ipv4(s) => s.set_octets(octets_of_value(kind, v)),
            Self::Int(s) => s.set_value(int_of(v).unwrap_or_default()),
            Self::Enum(s) => s.set_selected(enum_index(kind, v)),
        }
    }

    /// 显式禁用 / 恢复。
    fn set_disabled(&self, on: bool) {
        match self {
            Self::Ipv4(s) => s.set_disabled(on),
            Self::Int(s) => s.set_disabled(on),
            Self::Enum(s) => s.set_disabled(on),
        }
    }

    /// 控件当前是否禁用（**读自 LVGL**，不是本页自己的标志位）。
    ///
    /// `Ipv4Stepper` 没有整件的 `is_disabled()` 读回口（`ui/controls.rs` 本批禁改）⇒ 取第 0 段
    /// 的读回值（四段由 [`Ipv4Stepper::set_disabled`] 一次全设，取一段即整件）。
    ///
    /// **`#[cfg(test)]`**：`Ipv4Stepper::segment` 本身只在测试构建下提供（同因），且生产侧
    /// 不需要"读回控件禁用态"（置位才是生产需求）。
    #[cfg(test)]
    fn is_disabled(&self) -> bool {
        match self {
            Self::Ipv4(s) => s.segment(0).map(Stepper::is_disabled).unwrap_or(false),
            Self::Int(s) => s.is_disabled(),
            Self::Enum(s) => s.is_disabled(),
        }
    }

    /// 当前值区文字（离屏断言口径）。
    fn display(&self) -> Option<String> {
        match self {
            Self::Ipv4(s) => s.text(),
            Self::Int(s) => s.display(),
            Self::Enum(s) => {
                let idx = s.raw_selected().unwrap_or_else(|| s.selected());
                s.option(idx)
            }
        }
    }
}

/// 一个字段行（`UI §6.2` 的两种版式共用本结构，差别只在坐标）。
struct FieldRow {
    /// 稳定字段键。
    key: String,
    /// 控件类型（驱动取值 / 默认值 / 设值）。
    kind: ConfigKind,
    /// 注入时该字段的值（**唯一旧值真源**：脏判定与"放弃修改"回退都用它）。
    initial: Value,
    /// 字段名标签（离屏断言口径）。
    label: Label,
    /// 字段名两态样式（[0] 正常 / [1] 危险红）。
    label_styles: [Rc<Style>; 2],
    label_style: Cell<usize>,
    /// 状态槽：约束提示 **或** 字段错误原因（同槽互斥 ⇒ 出错不重排）。
    status: Label,
    /// 状态槽两态样式（[0] `text_weak` / [1] 危险红）。
    status_styles: [Rc<Style>; 2],
    status_style: Cell<usize>,
    /// 只读字段的说明行（`editable == false` 才有；与状态槽同坐标，二者互斥显示）。
    note: Option<Label>,
    /// 行左缘危险竖条（隐藏态；字段错误时显形 —— 见 **PD6**）。
    error_bar: Obj,
    /// 输入控件。
    control: FieldControl,
    /// 约束提示文本（`Ipv4` / `Enum` 为 `None` —— UI §6.2 不画）。
    hint: Option<String>,
}

impl FieldRow {
    /// 按当前错误态刷新本行的视觉（`Some(reason)` = 标红 + 就地表原因）。
    fn refresh_error(&self, reason: Option<&str>) {
        match reason {
            Some(r) => {
                self.error_bar.set_hidden(false);
                set_style_index(self.label.obj(), &self.label_styles, &self.label_style, 1);
                set_style_index(self.status.obj(), &self.status_styles, &self.status_style, 1);
                self.status.set_text(r);
                set_visible(self.status.obj(), true);
                if let Some(n) = &self.note {
                    set_visible(n.obj(), false);
                }
            }
            None => {
                self.error_bar.set_hidden(true);
                set_style_index(self.label.obj(), &self.label_styles, &self.label_style, 0);
                set_style_index(self.status.obj(), &self.status_styles, &self.status_style, 0);
                // 无错误 ⇒ 恢复"提示 / 只读说明 / 都不显示"三取一。
                if let Some(n) = &self.note {
                    n.set_text(TEXT_READONLY_NOTE);
                    set_visible(n.obj(), true);
                    set_visible(self.status.obj(), false);
                } else if let Some(hint) = &self.hint {
                    self.status.set_text(hint);
                    set_visible(self.status.obj(), true);
                } else {
                    set_visible(self.status.obj(), false);
                }
            }
        }
    }
}

/// 一个分组卡（卡头 + 字段行 + 分隔线）。
struct GroupCard {
    /// 卡对象（**拥有型**：`Drop` 即 `lv_obj_delete`，级联删除整棵子树 ⇒ 换视图 = 换句柄）。
    /// **仅为保持子树存活**（本页不直接读卡对象；`Deref` 亦不需要）。
    _obj: Obj,
    /// 分组名标签（离屏断言口径）。
    label: Label,
    /// 卡头竖条与行分隔线（**仅为保持子树存活** —— 局部变量随函数返回被 `Drop` ⇒ 屏幕上少一根线）。
    _decor: Vec<Obj>,
    /// 组内字段行（含控件句柄）。
    rows: Vec<FieldRow>,
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 页面核心（**共享可变态**：控件回调经 `Weak<Core>` 回访，避免 `Rc` 环）
// ═══════════════════════════════════════════════════════════════════════════

/// 页面的共享核心：控件回调只持有它的 `Weak`（强引用在 [`P2ConfigPage`] 上）——
/// `Stepper → 回调闭包 → Rc<Core> → Stepper` 会成环，句柄永不落地。
struct Core {
    /// 页根容器（`992 × 624`，**不滚动** —— 见模块文档第 2 条）。
    root: Obj,
    /// 滚动视口（`992 × 552`）。
    scroll: ScrollContainer,
    /// 固定操作条（`992 × 72`，不随滚动）。
    action_bar: Obj,
    /// 保存按钮（主按钮）。
    save: Rc<TextButton>,
    /// 恢复默认值按钮（危险按钮，与保存间距 ≥ [`Dimens::GAP_DANGER`]）。
    reset: Rc<TextButton>,
    /// 页说明行（常态文案）。
    note: Label,
    /// 失败 / 降级原因行（与 `note` **同槽互斥**；见 **PD7**）。
    fail: Label,
    /// 当前视图产生的分组卡（换视图即整体替换 ⇒ 旧句柄 `Drop` ⇒ 级联删子树）。
    cards: RefCell<Vec<GroupCard>>,
    /// 注入的视图快照（`None` = 尚未加载 / 不可用）。
    view: RefCell<Option<ConfigView>>,
    /// 后端二次校验的**逐字段**原因（`key → reason`；CF-02）。
    errors: RefCell<BTreeMap<String, String>>,
    /// 提交中（F9.6：保存按钮 `disabled` + 文案「保存中...」）。
    submitting: Cell<bool>,
    /// 配置是否可用（控制通道注入成功）。
    available: Cell<bool>,
    /// **延迟关闭弹层**的标志（`ConfirmDialog::close` 不得在 LVGL 事件回调内调用
    /// —— 取消按钮的回调只置本标志，真正的关闭在 [`P2ConfigPage::tick`] 里做）。
    pending_close: Cell<bool>,
    /// 当前打开的确认弹层（同一时刻至多 1 个）。
    dialog: RefCell<Option<ConfirmDialog>>,
    /// 当前 Toast（同一时刻至多 1 条，UI §7.2）。
    toast: RefCell<Option<Toast>>,
    /// 「提交意图」回调（确认完成时调用一次；**本页不生成 `request_id`**）。
    on_submit: SubmitSlot,
}

/// 提交意图回调槽：`RefCell<Option<Box<dyn FnMut(ConfigPatch, ConfirmLevel)>>>`。
///
/// 语义同 `ui/components.rs` 的三个 `*Callback` 别名：`Option` = "尚未接线"（未接线即 no-op）、
/// `Box<dyn FnMut>` 让调用方传任意捕获闭包。**不出现在任何 `pub` 签名里**（对外入口是泛型
/// [`P2ConfigPage::set_on_submit`]），故不导出；起别名只为让 [`Core`] 的字段类型可读。
type SubmitSlot = RefCell<Option<Box<dyn FnMut(ConfigPatch, ConfirmLevel)>>>;

/// 弹层种类（决定标题 / 影响范围 / 分级 / 明细 / 补丁来源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    /// 保存（`from = Edit`）。
    Save,
    /// 恢复默认值（`from = ResetDefault`）。
    Reset,
}

impl Core {
    /// 逐字段原因写入 + 行内刷新。
    fn apply_field_errors(&self, errs: &[FieldError]) {
        let map: BTreeMap<String, String> = errs
            .iter()
            .map(|e| (e.field.clone(), display_safe(&e.reason)))
            .collect();
        for card in self.cards.borrow().iter() {
            for row in &card.rows {
                row.refresh_error(map.get(&row.key).map(String::as_str));
            }
        }
        *self.errors.borrow_mut() = map;
    }

    /// 用户改了某个字段（控件回调入口）：清该字段的错误、撤下上一次失败提示、刷新按钮态。
    fn after_field_edit(&self, key: &str) {
        self.errors.borrow_mut().remove(key);
        self.show_note();
        self.refresh_actions();
    }

    /// 全部字段的**当前值**快照（读自控件）。
    fn current_values(&self) -> BTreeMap<String, Value> {
        let mut m = BTreeMap::new();
        for card in self.cards.borrow().iter() {
            for row in &card.rows {
                m.insert(row.key.clone(), row.control.current(&row.kind));
            }
        }
        m
    }

    /// 草稿补丁（只含被改动的字段）。
    fn draft(&self) -> ConfigPatch {
        match self.view.borrow().as_ref() {
            Some(v) => draft_patch(v, &self.current_values()),
            None => ConfigPatch {
                changes: JsonMap::new(),
                from: PatchSource::Edit,
            },
        }
    }

    /// 恢复默认值补丁。
    fn defaults_patch(&self) -> ConfigPatch {
        match self.view.borrow().as_ref() {
            Some(v) => defaults_patch_of(v),
            None => ConfigPatch {
                changes: JsonMap::new(),
                from: PatchSource::ResetDefault,
            },
        }
    }

    /// 是否有未保存修改。
    fn is_dirty(&self) -> bool {
        match self.view.borrow().as_ref() {
            Some(v) => is_dirty(v, &self.current_values()),
            None => false,
        }
    }

    /// 放弃修改：全部字段回退到注入值，清掉错误与失败提示。
    fn discard(&self) {
        for card in self.cards.borrow().iter() {
            for row in &card.rows {
                row.control.set(&row.kind, &row.initial);
                row.refresh_error(None);
            }
        }
        self.errors.borrow_mut().clear();
        self.show_note();
        self.refresh_actions();
    }

    /// 显示常态说明行（隐藏失败行）。
    fn show_note(&self) {
        set_visible(self.fail.obj(), false);
        set_visible(self.note.obj(), true);
    }

    /// 显示失败 / 降级原因行（隐藏常态说明行）。
    fn show_fail(&self, text: &str) {
        self.fail.set_text(text);
        set_visible(self.note.obj(), false);
        set_visible(self.fail.obj(), true);
    }

    /// 刷新操作条（保存 / 恢复默认值的可用态 + 保存文案）。
    fn refresh_actions(&self) {
        let submitting = self.submitting.get();
        self.save.set_text(if submitting { TEXT_SAVING } else { TEXT_SAVE });
        let usable = self.available.get() && !submitting;
        // 无改动 / 有字段错误 ⇒ 保存置灰（EDGE-10 / CF-02 的"保存按钮置灰"）。
        let can_save = usable && self.is_dirty() && self.errors.borrow().is_empty();
        self.save.set_disabled(!can_save);
        self.reset.set_disabled(!usable);
    }

    /// 关闭弹层（**只可在 LVGL 事件回调之外调用**）。
    fn close_dialog(&self) {
        if let Some(d) = self.dialog.borrow_mut().take() {
            d.close();
        }
    }

    /// 关闭 Toast（同上）。
    fn close_toast(&self) {
        if let Some(t) = self.toast.borrow_mut().take() {
            t.close();
        }
    }

    /// 弹一条 Toast（同一时刻仅 1 条：新的覆盖旧的，UI §7.2）。
    fn show_toast(&self, tone: ToastTone, icon: &str, text: &str) -> Result<(), LvglError> {
        self.close_toast();
        let layer = widgets::layer_top()?;
        let t = Toast::new(&layer, tone, icon, text)?;
        *self.toast.borrow_mut() = Some(t);
        Ok(())
    }

    /// 触发「提交意图」回调（**不发起任何请求**；`request_id` 由外部生成）。
    fn fire_submit(&self, patch: ConfigPatch, level: ConfirmLevel) {
        if let Ok(mut slot) = self.on_submit.try_borrow_mut() {
            if let Some(f) = slot.as_mut() {
                f(patch, level);
            }
        }
    }

    /// 按 key 访问字段行（`RefCell::borrow` 借用的生命周期**不外泄** ⇒ 用闭包）。
    fn with_row<R>(&self, key: &str, f: impl FnOnce(&FieldRow) -> R) -> Option<R> {
        for card in self.cards.borrow().iter() {
            for row in &card.rows {
                if row.key == key {
                    return Some(f(row));
                }
            }
        }
        None
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. 卡片装配
// ═══════════════════════════════════════════════════════════════════════════

/// 按视图建全部分组卡（**先建后换**：任一环节失败 ⇒ 返回 `Err` 且旧视图原样保留）。
fn build_cards(core: &Rc<Core>, view: &ConfigView) -> Result<Vec<GroupCard>, LvglError> {
    let label_styles = [
        theme::text(TextSlot::Label, Palette::TEXT_PRIMARY),
        theme::text(TextSlot::Label, Palette::DANGER),
    ];
    let status_styles = [
        theme::text(TextSlot::Body, Palette::TEXT_WEAK),
        theme::text(TextSlot::Body, Palette::DANGER),
    ];

    let mut cards: Vec<GroupCard> = Vec::with_capacity(view.groups.len());
    let mut y = FIRST_CARD_Y;
    for g in &view.groups {
        cards.push(build_card(
            core,
            g,
            y,
            &label_styles,
            &status_styles,
        )?);
        y += card_height(g) + CARD_GAP;
    }
    Ok(cards)
}

/// 分组卡高（卡头 + 各行 + 行间分隔线 + 上下内边距）。
fn card_height(group: &ConfigGroup) -> i32 {
    let rows: i32 = group.fields.iter().map(|f| row_height(&f.kind)).sum();
    let dividers = if group.fields.is_empty() {
        0
    } else {
        (group.fields.len() as i32 - 1) * theme::Stroke::THIN
    };
    CARD_HEAD_H + rows + dividers + 2 * CARD_INSET
}

/// 建一张分组卡。
fn build_card(
    core: &Rc<Core>,
    group: &ConfigGroup,
    y: i32,
    label_styles: &[Rc<Style>; 2],
    status_styles: &[Rc<Style>; 2],
) -> Result<GroupCard, LvglError> {
    let obj = decor(&core.scroll, Dimens::CONTENT_W, card_height(group), &theme::card())?;
    obj.set_pos(0, y);

    // 卡头：4 px 强调竖条 + 分组名 28 px（UI §6.2「卡头 44 px」）。
    let bar = decor(
        &obj,
        Dimens::ACCENT_BAR,
        CARD_HEAD_H,
        &theme::card_head_bar(Palette::INFO),
    )?;
    bar.set_pos(0, 0);
    let label = text_label(
        &obj,
        &group.label,
        TextSlot::SectionTitle,
        Palette::TEXT_PRIMARY,
    )?;
    label.set_pos(
        Dimens::ACCENT_BAR + Dimens::GAP_MIN,
        theme::center_offset(CARD_HEAD_H, TextSlot::SectionTitle.px() as i32),
    );

    let mut decor_keep = vec![bar];
    let mut rows = Vec::with_capacity(group.fields.len());
    let mut ry = CARD_HEAD_H;
    for (i, f) in group.fields.iter().enumerate() {
        rows.push(build_row(core, &obj, f, ry, label_styles, status_styles)?);
        ry += row_height(&f.kind);
        // 行间 1 px 分隔（UI §6.2「最后一行不画」）。
        if i + 1 < group.fields.len() {
            let d = decor(
                &obj,
                INNER_W,
                theme::Stroke::THIN,
                &theme::card_head_bar(Palette::DIVIDER),
            )?;
            d.set_pos(0, ry);
            decor_keep.push(d);
            ry += theme::Stroke::THIN;
        }
    }

    Ok(GroupCard {
        _obj: obj,
        label,
        _decor: decor_keep,
        rows,
    })
}

/// 建一个字段行（两种版式由 `kind` 决定）。
fn build_row(
    core: &Rc<Core>,
    card: &Obj,
    f: &ConfigField,
    y: i32,
    label_styles: &[Rc<Style>; 2],
    status_styles: &[Rc<Style>; 2],
) -> Result<FieldRow, LvglError> {
    let ipv4 = matches!(f.kind, ConfigKind::Ipv4);
    let hint = range_hint(&f.kind, f.unit.as_deref());

    // 字段名。
    let name_l = text_label(card, &field_label_text(f), TextSlot::Label, Palette::TEXT_PRIMARY)?;
    let label_y = if ipv4 {
        y + ROW_B_LABEL_Y
    } else {
        y + ROW_A_TEXT_Y
    };
    name_l.set_pos(0, label_y);
    let label_style = Cell::new(usize::MAX);
    set_style_index(name_l.obj(), label_styles, &label_style, 0);

    // 状态槽（约束提示 / 错误原因）。
    let status = label(card, TextSlot::Body, Palette::TEXT_WEAK)?;
    status.set_size(STATUS_W, TextSlot::Body.px() as i32);
    status.set_long_mode(LongMode::DOTS);
    let status_y = if ipv4 {
        y + ROW_B_LABEL_Y
            + theme::center_offset(TextSlot::Label.px() as i32, TextSlot::Body.px() as i32)
    } else {
        y + ROW_A_TEXT_Y + TextSlot::Label.px() as i32
    };
    status.set_pos(if ipv4 { STATUS_X } else { 0 }, status_y);
    let status_style = Cell::new(usize::MAX);
    set_style_index(status.obj(), status_styles, &status_style, 0);

    // 只读字段的说明行（设计 §6.2：控件 `disabled` + 说明行）。
    let note = if f.editable {
        None
    } else {
        let n = text_label(card, TEXT_READONLY_NOTE, TextSlot::Body, Palette::TEXT_WEAK)?;
        n.set_size(STATUS_W, TextSlot::Body.px() as i32);
        n.set_long_mode(LongMode::DOTS);
        n.set_pos(if ipv4 { STATUS_X } else { 0 }, status_y);
        Some(n)
    };

    // 行左缘危险竖条（隐藏态；出错显形）。
    let error_bar = decor(
        card,
        ERROR_BAR_W,
        row_height(&f.kind),
        &theme::card_head_bar(Palette::DANGER),
    )?;
    error_bar.set_pos(0, y);
    error_bar.set_hidden(true);

    // 输入控件（`kind` 驱动；**零文本输入**）。
    let control = match &f.kind {
        ConfigKind::Ipv4 => {
            let s = Rc::new(Ipv4Stepper::new(card, octets_of(f))?);
            s.obj().set_pos(ROW_B_CTRL_X, y + ROW_B_CTRL_Y);
            FieldControl::Ipv4(s)
        }
        ConfigKind::U16 { .. } | ConfigKind::U64 { .. } => {
            let (lo, hi, step) = int_bounds(&f.kind);
            let s = Rc::new(Stepper::new(card, lo, hi, initial_int(f), step)?);
            s.obj()
                .set_pos(INNER_W - STEPPER_TOTAL_W, y + ROW_A_CTRL_Y);
            FieldControl::Int(s)
        }
        ConfigKind::Enum { options } => {
            let labels: Vec<String> = options.iter().map(|o| display_safe(&o.label)).collect();
            let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            let width = enum_width(options.len())?;
            let idx = enum_index(&f.kind, &f.value);
            let s = Rc::new(SegmentedControl::new(card, &refs, width, idx)?);
            s.obj().set_pos(INNER_W - width, y + ROW_A_CTRL_Y);
            FieldControl::Enum(s)
        }
    };
    if !f.editable {
        control.set_disabled(true);
    }

    // ── 变更回调：只报"这一行被改了"，簿记在 [`Core::after_field_edit`] ──
    {
        let w = Rc::downgrade(core);
        let key = f.key.clone();
        let on_edit = move || {
            if let Some(c) = w.upgrade() {
                c.after_field_edit(&key);
            }
        };
        match &control {
            FieldControl::Ipv4(s) => {
                let cb = on_edit.clone();
                s.set_on_change(move |_o| cb());
            }
            FieldControl::Int(s) => {
                let cb = on_edit.clone();
                s.set_on_change(move |_v| cb());
            }
            FieldControl::Enum(s) => {
                s.set_on_change(move |_i| on_edit());
            }
        }
    }

    let row = FieldRow {
        key: f.key.clone(),
        kind: f.kind.clone(),
        initial: f.value.clone(),
        label: name_l,
        label_styles: [Rc::clone(&label_styles[0]), Rc::clone(&label_styles[1])],
        label_style,
        status,
        status_styles: [Rc::clone(&status_styles[0]), Rc::clone(&status_styles[1])],
        status_style,
        note,
        error_bar,
        control,
        hint,
    };
    row.refresh_error(None);
    Ok(row)
}

/// `U16`/`U64` → `(min, max, step)` 的 `i64` 形式（`u64` 超 `i64` 时收敛到 `i64::MAX`）。
fn int_bounds(kind: &ConfigKind) -> (i64, i64, i64) {
    match kind {
        ConfigKind::U16 { min, max, step } => {
            (i64::from(*min), i64::from(*max), i64::from(*step))
        }
        ConfigKind::U64 { min, max, step } => (
            i64::try_from(*min).unwrap_or(i64::MAX),
            i64::try_from(*max).unwrap_or(i64::MAX),
            i64::try_from(*step).unwrap_or(1),
        ),
        _ => (0, 0, 1),
    }
}

/// 分段控件宽（每段 ≥ [`ui::controls::SEGMENT_MIN_W`]；装不进卡内容区则**响亮失败**）。
fn enum_width(count: usize) -> Result<i32, LvglError> {
    let n = i32::try_from(count)
        .map_err(|_| LvglError::InvalidArgument("P2: Enum 选项过多"))?;
    let w = n
        .checked_mul(crate::ui::controls::SEGMENT_MIN_W)
        .ok_or(LvglError::InvalidArgument("P2: Enum 宽溢出"))?;
    if w > INNER_W {
        return Err(LvglError::InvalidArgument(
            "P2: Enum 选项数超出卡内容区宽度",
        ));
    }
    Ok(w)
}

/// `Value` → 四段（`Ipv4` 设值用；不可解析取 `0.0.0.0`）。
fn octets_of_value(_kind: &ConfigKind, v: &Value) -> [u8; 4] {
    v.as_str()
        .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok())
        .map(|a| a.octets())
        .unwrap_or([0, 0, 0, 0])
}

/// `Ipv4Stepper::octets()` → 文本（`192.168.1.10`；分隔符 `.` 在 cmap 内）。
fn ipv4_text(o: [u8; 4]) -> String {
    let [a, b, c, d] = o;
    format!("{a}.{b}.{c}.{d}")
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. 页面
// ═══════════════════════════════════════════════════════════════════════════

/// P2 配置页（**控制通道驱动**；见模块文档第 1 条）。
///
/// # 意图如何交给外部
///
/// 本页**不发请求、不生成 `request_id`**：用户点「保存」/「恢复默认值」只**开确认弹层**；
/// 弹层确认完成（L1 单击 / L2·L2+ 长按满 1.0 s）经 [`P2ConfigPage::set_on_submit`] 注册的
/// 回调交出一份 [`ConfigPatch`] 与所用 [`ConfirmLevel`]；外部（B3 `console.rs`）据此生成
/// `request_id`(uuid) + `issued_at_ms` 后 POST `/v1/console/config/apply`，并把回执经
/// [`P2ConfigPage::show_result`] 灌回本页。
pub struct P2ConfigPage {
    core: Rc<Core>,
}

impl P2ConfigPage {
    /// 在 `parent` 下建页（页根 `992 × 624`；**摆放由调用方负责**，与 P1/P6 同口径）。
    pub fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = layout_box(parent, Dimens::CONTENT_W, Dimens::CONTENT_H)?;

        // 滚动视口（页根自建 —— `pages::page_root()` 的"页根即滚动容器"语义留给 P1/P6）。
        let scroll = ScrollContainer::create(&root)?;
        scroll.set_size(Dimens::CONTENT_W, SCROLL_H);
        scroll.set_pos(0, 0);
        scroll.add_style(&theme::transparent(), crate::lvgl::style::StyleSelector::main());

        // 固定操作条（不随滚动；UI §6.2 线框 `Y624`）。
        //
        // 样式取 `card_head_bar(SURFACE)`：它**零内边距 + 无描边 + 直角**（`theme.rs` 里唯一
        // "整块纯色"的组合），与本页"用绝对坐标排布操作条内元素"的需要一致 ——
        // `theme::surface_alt()` 未显式置零内边距，会被默认主题的内边距把按钮推偏。
        let action_bar = decor(
            &root,
            Dimens::CONTENT_W,
            ACTION_BAR_H,
            &theme::card_head_bar(Palette::SURFACE),
        )?;
        action_bar.set_pos(0, SCROLL_H);

        let save = Rc::new(TextButton::create(&action_bar, TEXT_SAVE)?);
        save.set_size(Dimens::BTN_MAIN_W, Dimens::BTN_H_PRIMARY);
        save.set_pos(
            Dimens::CONTENT_W - ACTION_PAD - Dimens::BTN_MAIN_W,
            ACTION_BTN_Y,
        );
        save.label().center();
        theme::button(theme::ButtonKind::Primary).apply(&save);

        let reset = Rc::new(TextButton::create(&action_bar, TEXT_RESET_DEFAULT)?);
        reset.set_size(Dimens::BTN_MAIN_W, Dimens::BTN_H_PRIMARY);
        reset.set_pos(ACTION_PAD, ACTION_BTN_Y);
        reset.label().center();
        theme::button(theme::ButtonKind::Danger).apply(&reset);

        // 页说明行 / 失败行（**同槽互斥**；都在滚动区内 = UI 线框 `Y80` 随页滚动）。
        let note = text_label(&scroll, TEXT_PAGE_NOTE, TextSlot::Body, Palette::TEXT_WEAK)?;
        note.set_pos(0, NOTE_Y);
        let fail = text_label(&scroll, "", TextSlot::Body, Palette::DANGER)?;
        fail.set_size(Dimens::CONTENT_W, TextSlot::Body.px() as i32);
        fail.set_long_mode(LongMode::DOTS);
        fail.set_pos(0, NOTE_Y);
        fail.set_hidden(true);

        let core = Rc::new(Core {
            root,
            scroll,
            action_bar,
            save,
            reset,
            note,
            fail,
            cards: RefCell::new(Vec::new()),
            view: RefCell::new(None),
            errors: RefCell::new(BTreeMap::new()),
            submitting: Cell::new(false),
            available: Cell::new(false),
            pending_close: Cell::new(false),
            dialog: RefCell::new(None),
            toast: RefCell::new(None),
            on_submit: RefCell::new(None),
        });

        // 按钮：只**开弹层**（确认完成前不发任何请求 —— 设计 §6.2 末行）。
        {
            let w = Rc::downgrade(&core);
            core.save.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                if let Err(e) = open_dialog(&c, DialogKind::Save) {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr(), "P2 配置页：打开保存确认弹层失败：{e}");
                }
            });
        }
        {
            let w = Rc::downgrade(&core);
            core.reset.on_clicked(move |_| {
                let Some(c) = w.upgrade() else { return };
                if let Err(e) = open_dialog(&c, DialogKind::Reset) {
                    use std::io::Write;
                    let _ = writeln!(std::io::stderr(), "P2 配置页：打开恢复默认值确认弹层失败：{e}");
                }
            });
        }

        let page = Self { core };
        page.set_unavailable("");
        Ok(page)
    }

    /// 页根对象。
    pub fn obj(&self) -> &Obj {
        &self.core.root
    }

    /// 滚动视口对象（装配断言口径）。
    pub fn scroll_obj(&self) -> &Obj {
        &self.core.scroll
    }

    /// 固定操作条对象（装配断言口径）。
    pub fn action_bar_obj(&self) -> &Obj {
        &self.core.action_bar
    }

    /// 注册「提交意图」回调：确认完成时**调用一次**，载荷 = 待写补丁 + 所用确认强度。
    ///
    /// ⚠️ **回调内不得回灌本页会关弹层的方法**（[`P2ConfigPage::show_result`] /
    /// [`P2ConfigPage::set_config`]）：它们会 `ConfirmDialog::close`，而本回调正是从该弹层的
    /// 事件回调里出来的（`components.rs` 明确要求"关闭由调用方在自己的 tick 里做"）。
    /// 正确姿势：回调内只把补丁交给网络任务，结果回来后在**事件循环的下一拍**调
    /// [`P2ConfigPage::show_result`]。
    pub fn set_on_submit<F>(&self, f: F)
    where
        F: FnMut(ConfigPatch, ConfirmLevel) + 'static,
    {
        *self.core.on_submit.borrow_mut() = Some(Box::new(f));
    }

    /// 注入配置视图（`GET /v1/console/config` 的结果 / 成功回执的 `applied`）。
    ///
    /// `WriteMode::FullRewrite` ⇒ Toast 明示「原有文字不再存在」（EDGE-23，见 **PD9/PD10**）。
    /// **先建后换**：建卡失败 ⇒ 返回 `Err` 且**旧视图原样保留**（不留半成品界面）。
    pub fn set_config(&self, view: &ConfigView) -> Result<(), LvglError> {
        self.core.close_dialog();
        let cards = build_cards(&self.core, view)?;
        *self.core.cards.borrow_mut() = cards;
        *self.core.view.borrow_mut() = Some(view.clone());
        self.core.errors.borrow_mut().clear();
        self.core.available.set(true);
        self.core.show_note();
        self.core.refresh_actions();
        if view.write_mode == WriteMode::FullRewrite {
            self.core
                .show_toast(ToastTone::Warning, ICON_WARN, TEXT_TOAST_FULL_REWRITE)?;
        }
        Ok(())
    }

    /// 控制通道取不到配置时的降级态（**只由注入值驱动**）。
    ///
    /// ⚠️ 本页**不**用 [`crate::ui::components::UnavailableState`]：它的
    /// [`crate::ui::components::UnavailableKind`] 只有「告警源 / 审计 / 联锁」三个场景，
    /// **没有配置场景**（`components.rs` 本批禁改）⇒ 自建"标题 + 原因"两段式提示行，
    /// 语义仍是**不可用（无法获知）**，不是"空"（UI §8.3 的"空 vs 不可用"口径不破）。
    pub fn set_unavailable(&self, reason: &str) {
        self.core.close_dialog();
        self.core.cards.borrow_mut().clear();
        *self.core.view.borrow_mut() = None;
        self.core.errors.borrow_mut().clear();
        self.core.available.set(false);
        self.core.show_fail(&unavailable_text(reason));
        self.core.refresh_actions();
    }

    /// 提交中（F9.6）：保存按钮 `disabled` + 文案「保存中...」。
    pub fn set_submitting(&self, on: bool) {
        self.core.submitting.set(on);
        self.core.refresh_actions();
    }

    /// 回执注入（成功 / 失败两条路径，设计 §6.2「成功」「失败」两行）。
    ///
    /// - **成功**：用 `applied` 立即刷新本地值（不等下一帧）+ Toast；
    /// - **失败**：Toast + **保留用户已输入值**（EDGE-10）+ 逐字段标红并显示**具体**原因
    ///   （CF-02）+ 保存按钮置灰；`AuditUnavailable` 走 UI §8.3 的固定文案（EDGE-18）。
    pub fn show_result(&self, resp: &ControlResponse<ConfigView>) -> Result<(), LvglError> {
        self.core.close_dialog();
        if resp.ok {
            let full_rewrite = resp
                .applied
                .as_ref()
                .is_some_and(|v| v.write_mode == WriteMode::FullRewrite);
            if let Some(applied) = resp.applied.clone() {
                self.set_config(&applied)?;
            }
            // `full_rewrite` 的信息量更大（数据损失提示），此时 `set_config` 已弹警示 Toast
            // —— 不再叠加"保存成功"（UI §7.2：同一时刻仅 1 条）。见 PD10。
            if !full_rewrite {
                self.core
                    .show_toast(ToastTone::Success, ICON_OK, TEXT_TOAST_OK)?;
            }
        } else {
            self.core.apply_field_errors(&resp.field_errors);
            // EDGE-18（审计不可写）走 UI §8.3 的**固定文案**，且**落在 Toast 上**
            // （该行判据原文即「Toast『审计不可用，操作未执行』」）；其余失败按
            // UI §6.2 流程 6：Toast「保存失败」+ 就地（页顶行 / 字段行）显示**具体**原因。
            let audit = resp.code == ControlCode::AuditUnavailable;
            let text = if audit {
                TEXT_AUDIT_UNAVAILABLE.to_string()
            } else {
                display_safe(resp.message.trim())
            };
            self.core.show_fail(&text);
            let toast = if audit {
                TEXT_AUDIT_UNAVAILABLE
            } else {
                TEXT_TOAST_FAIL
            };
            self.core.show_toast(ToastTone::Failure, ICON_FAIL, toast)?;
            self.core.refresh_actions();
        }
        Ok(())
    }

    /// 每个事件循环拍调一次：执行**延迟动作**（取消后的弹层关闭 / Toast 过期）。
    ///
    /// 时钟**注入**（`now`）⇒ 离屏可确定性驱动；本页自身不读时钟做业务判断。
    pub fn tick(&self, now: Instant) {
        if self.core.pending_close.get() {
            self.core.pending_close.set(false);
            self.core.close_dialog();
        }
        let expired = self
            .core
            .toast
            .borrow()
            .as_ref()
            .is_some_and(|t| t.is_expired(now));
        if expired {
            self.core.close_toast();
        }
    }

    /// 是否有未保存修改（EDGE-11）。
    pub fn is_dirty(&self) -> bool {
        self.core.is_dirty()
    }

    /// 放弃修改：全部字段回退到注入值。
    pub fn discard_draft(&self) {
        self.core.discard();
    }

    /// 草稿补丁（只含被改动的字段，`from = Edit`）。
    pub fn draft(&self) -> ConfigPatch {
        self.core.draft()
    }

    /// 恢复默认值补丁（全部 `editable` 字段，`from = ResetDefault`）。
    pub fn defaults_patch(&self) -> ConfigPatch {
        self.core.defaults_patch()
    }

    /// 配置是否可用。
    pub fn is_available(&self) -> bool {
        self.core.available.get()
    }

    /// 是否提交中。
    pub fn is_submitting(&self) -> bool {
        self.core.submitting.get()
    }

    // ── 离屏断言口径 ─────────────────────────────────────────────────────

    /// 分组数。
    pub fn group_count(&self) -> usize {
        self.core.cards.borrow().len()
    }

    /// 第 `gi` 组的分组名。
    pub fn group_label(&self, gi: usize) -> Option<String> {
        self.core.cards.borrow().get(gi).and_then(|c| c.label.text())
    }

    /// 第 `gi` 组的字段数。
    pub fn field_count(&self, gi: usize) -> usize {
        self.core
            .cards
            .borrow()
            .get(gi)
            .map(|c| c.rows.len())
            .unwrap_or(0)
    }

    /// 第 `gi` 组第 `fi` 个字段的**上屏标签**。
    pub fn field_label(&self, gi: usize, fi: usize) -> Option<String> {
        self.core
            .cards
            .borrow()
            .get(gi)
            .and_then(|c| c.rows.get(fi))
            .and_then(|r| r.label.text())
    }

    /// 某字段当前**显示值文本**（读自控件：值区 / 汇总标签 / 选中段）。
    pub fn field_value_text(&self, key: &str) -> Option<String> {
        self.core.with_row(key, |r| r.control.display()).flatten()
    }

    /// 某字段的只读说明行文本（非只读字段为 `None`）。
    pub fn field_note_text(&self, key: &str) -> Option<String> {
        self.core
            .with_row(key, |r| r.note.as_ref().and_then(|n| n.text()))
            .flatten()
    }

    /// 某字段的只读说明行是否**可见**（`None` = 该字段没有说明行）。
    pub fn field_note_visible(&self, key: &str) -> Option<bool> {
        self.core
            .with_row(key, |r| r.note.as_ref().map(|n| !n.obj().is_hidden()))
            .flatten()
    }

    /// 某字段状态槽当前文本（无提示 / 无错误 ⇒ 空串）。
    pub fn field_status_text(&self, key: &str) -> Option<String> {
        self.core.with_row(key, |r| r.status.text()).flatten()
    }

    /// 某字段状态槽是否可见（约束提示或错误原因）。
    pub fn field_status_visible(&self, key: &str) -> Option<bool> {
        self.core
            .with_row(key, |r| !r.status.obj().is_hidden())
    }

    /// 某字段的错误竖条是否显形（**字段级标红**的读回口径）。
    pub fn field_error_visible(&self, key: &str) -> Option<bool> {
        self.core
            .with_row(key, |r| !r.error_bar.is_hidden())
    }

    /// 保存按钮。
    pub fn save_button(&self) -> &TextButton {
        &self.core.save
    }

    /// 恢复默认值按钮。
    pub fn reset_button(&self) -> &TextButton {
        &self.core.reset
    }

    /// 保存按钮当前文案。
    pub fn save_text(&self) -> Option<String> {
        self.core.save.text()
    }

    /// 保存按钮当前是否禁用（读自 LVGL 状态位）。
    pub fn save_disabled(&self) -> bool {
        widgets::has_state(self.core.save.button().obj(), crate::lvgl::style::State::DISABLED)
    }

    /// 恢复默认值按钮当前是否禁用。
    pub fn reset_disabled(&self) -> bool {
        widgets::has_state(self.core.reset.button().obj(), crate::lvgl::style::State::DISABLED)
    }

    /// 当前 Toast 文案（无 Toast ⇒ `None`）。
    pub fn toast_text(&self) -> Option<String> {
        self.core.toast.borrow().as_ref().and_then(|t| t.text())
    }

    /// 当前 Toast 语义（无 ⇒ `None`）。
    pub fn toast_tone(&self) -> Option<ToastTone> {
        self.core.toast.borrow().as_ref().map(|t| t.tone())
    }

    /// 页顶**失败 / 降级**行文本（即使当前不可见也可读回）。
    pub fn fail_text(&self) -> Option<String> {
        self.core.fail.text()
    }

    /// 页顶失败行是否可见。
    pub fn fail_visible(&self) -> bool {
        !self.core.fail.obj().is_hidden()
    }

    /// 页顶**常态说明行**文本。
    pub fn note_text(&self) -> Option<String> {
        self.core.note.text()
    }

    /// 页顶常态说明行是否可见。
    pub fn note_visible(&self) -> bool {
        !self.core.note.obj().is_hidden()
    }

    /// **仅测试**：以闭包访问当前弹层（`RefCell` 借用不外泄）—— 给离屏用例读分级 / 标题 /
    /// `WarnBanner`，并向「确认」按钮派发事件（`Obj::send_event`）。
    ///
    /// **`#[cfg(test)]`**：生产侧不需要"从外部拿弹层"，故编译期即不提供该路径。
    #[cfg(test)]
    pub fn with_dialog<R>(&self, f: impl FnOnce(&ConfirmDialog) -> R) -> Option<R> {
        self.core.dialog.borrow().as_ref().map(f)
    }

    /// **仅测试**：程序化设某字段的值（**走与控件回调同一条簿记**：脏标记 / 清错 / 刷按钮）。
    ///
    /// 为什么必须有它：`Stepper` 的 `−` / `＋` 点击闭包挂在**子按钮**上，而
    /// `Obj::send_event` **只向本对象派发并向父链冒泡**（不下行）⇒ 离屏用例无法经事件驱动
    /// 步进器（`ui/controls.rs` 模块文档「薄层缺能力」5 的同款事实）。返回 `false` = 无此字段。
    #[cfg(test)]
    pub fn set_field_value(&self, key: &str, value: &Value) -> bool {
        let hit = self.core.with_row(key, |r| {
            r.control.set(&r.kind, value);
        });
        if hit.is_none() {
            return false;
        }
        self.core.after_field_edit(key);
        true
    }

    /// **仅测试**：某字段控件当前是否禁用（读自 LVGL —— 生产侧无此读回需求，见
    /// [`FieldControl::is_disabled`]；`Ipv4Stepper` 的读回口本身也只在 `cfg(test)` 下提供）。
    #[cfg(test)]
    pub fn field_disabled(&self, key: &str) -> Option<bool> {
        self.core.with_row(key, |r| r.control.is_disabled())
    }
}

/// 开确认弹层（**唯一的弹层构造点**；确认完成前不发任何请求）。
fn open_dialog(core: &Rc<Core>, kind: DialogKind) -> Result<(), LvglError> {
    // 同一时刻至多 1 个弹层；不可用 / 提交中不开。
    if core.dialog.borrow().is_some() || !core.available.get() || core.submitting.get() {
        return Ok(());
    }
    let Some(view) = core.view.borrow().as_ref().cloned() else {
        return Ok(());
    };
    let current = core.current_values();

    let (title, impact, level, details_src, warn_src) = match kind {
        DialogKind::Save => {
            // 无改动不弹（「保存」是"提交草稿"，不是"重放"）。
            if !is_dirty(&view, &current) {
                return Ok(());
            }
            (
                TEXT_DIALOG_TITLE_SAVE,
                TEXT_IMPACT_SAVE,
                save_level(&view),
                save_details(&view, &current),
                reconnect_field_labels(&view),
            )
        }
        DialogKind::Reset => (
            TEXT_RESET_DEFAULT,
            TEXT_IMPACT_RESET,
            reset_level(&view),
            reset_details(&view, &current),
            reconnect_field_labels(&view),
        ),
    };

    let details: Vec<ConfirmDetail> = details_src
        .iter()
        .map(|d| ConfirmDetail {
            field: &d.label,
            before: &d.before,
            after: &d.after,
        })
        .collect();
    let warn: Vec<&str> = warn_src.iter().map(|s| s.as_str()).collect();
    let spec = ConfirmSpec {
        title,
        impact,
        details: &details,
        warn_fields: &warn,
    };

    let layer = widgets::layer_top()?;
    let dialog = ConfirmDialog::new(&layer, &spec, level)?;

    // 确认完成 ⇒ **只报意图**（不发请求、不生成 `request_id`）。
    {
        let w = Rc::downgrade(core);
        dialog.set_on_confirm(move || {
            let Some(c) = w.upgrade() else { return };
            let patch = match kind {
                DialogKind::Save => c.draft(),
                DialogKind::Reset => c.defaults_patch(),
            };
            c.fire_submit(patch, level);
        });
    }
    // 取消 ⇒ 只置"待关闭"标志（不得在事件回调里删弹层，见 `Core::pending_close`）。
    {
        let w = Rc::downgrade(core);
        dialog.set_on_cancel(move || {
            if let Some(c) = w.upgrade() {
                c.pending_close.set(true);
            }
        });
    }

    *core.dialog.borrow_mut() = Some(dialog);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. 纯逻辑单测（**不触碰 LVGL** —— LVGL 非线程安全，触碰它的用例只能由
//    `src/lvgl/tests.rs::lvgl_core_bridge_chain` 串行调起，见 `ui/tests.rs`）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use mupc_display_proto::OptionItem;

    /// 造一个字段（**默认全部可编辑、无单位、不瞬断**，用例只覆写关心的那几项）。
    fn field(key: &str, label: &str, kind: ConfigKind, value: Value) -> ConfigField {
        ConfigField {
            key: key.into(),
            label: label.into(),
            kind,
            default: value.clone(),
            value,
            unit: None,
            requires_reconnect: false,
            editable: true,
        }
    }

    /// 三组视图：IEC 104（Ipv4 + U16）/ 核间（U16）/ 遥测与日志（U64 + Enum + 只读 Ipv4）。
    fn view() -> ConfigView {
        let u16k = ConfigKind::U16 {
            min: 1,
            max: 65535,
            step: 1,
        };
        let mut port = field("gateway.port", "端口", u16k.clone(), Value::from(2404));
        port.default = Value::from(2404);
        let mut listen = field(
            "gateway.listen_addr",
            "对端 IP 地址",
            ConfigKind::Ipv4,
            Value::from("127.0.0.1"),
        );
        listen.default = Value::from("127.0.0.1");
        let mut period = field(
            "telemetry.interval",
            "遥测上报周期",
            ConfigKind::U64 {
                min: 1,
                max: 300,
                step: 1,
            },
            Value::from(1u64),
        );
        period.unit = Some("秒".into());
        period.default = Value::from(60u64);
        let level = field(
            "system.log_level",
            "日志级别",
            ConfigKind::Enum {
                options: vec![
                    OptionItem {
                        value: "error".into(),
                        label: "ERROR".into(),
                    },
                    OptionItem {
                        value: "info".into(),
                        label: "INFO".into(),
                    },
                ],
            },
            Value::from("info"),
        );
        let mut bind = field(
            "display.bind_addr",
            "本机服务地址（仅回环）",
            ConfigKind::Ipv4,
            Value::from("127.0.0.1"),
        );
        bind.editable = false;
        ConfigView {
            groups: vec![
                ConfigGroup {
                    id: "iec104".into(),
                    label: "IEC 104 连接参数".into(),
                    fields: vec![listen, port],
                },
                ConfigGroup {
                    id: "intercore".into(),
                    label: "核间通信参数".into(),
                    fields: vec![field(
                        "intercore.port",
                        "本地端口",
                        u16k.clone(),
                        Value::from(2500),
                    )],
                },
                ConfigGroup {
                    id: "telemetry".into(),
                    label: "遥测与日志".into(),
                    fields: vec![period, level, bind],
                },
            ],
            revision: 7,
            write_mode: WriteMode::TextPreserve,
        }
    }

    /// 草稿只含**被改动**的字段；改动前后的脏判定对称。
    ///
    /// 敏感性：把 [`draft_patch`] 的 `*now != f.value` 改成无条件插入 ⇒ 第 1、4 条变红；
    /// 改成恒 `false` ⇒ 第 2、3 条变红。
    #[test]
    fn draft_patch_contains_only_changed_fields() {
        let v = view();
        let mut cur = initial_values(&v);

        // 未改动 ⇒ 空补丁、不脏。
        assert!(draft_patch(&v, &cur).changes.is_empty());
        assert!(!is_dirty(&v, &cur));

        // 改一个 ⇒ 只含该字段。
        cur.insert("gateway.port".into(), Value::from(2405));
        let p = draft_patch(&v, &cur);
        assert_eq!(p.from, PatchSource::Edit);
        assert_eq!(p.changes.len(), 1);
        assert_eq!(p.changes.get("gateway.port"), Some(&Value::from(2405)));
        assert!(is_dirty(&v, &cur));

        // 改回 ⇒ 又不脏（**值相等判定**，不是"碰过就脏"）。
        cur.insert("gateway.port".into(), Value::from(2404));
        assert!(!is_dirty(&v, &cur));
        assert!(draft_patch(&v, &cur).changes.is_empty());
    }

    /// 恢复默认值补丁：**覆盖全部 `editable` 字段** + `from = ResetDefault` +
    /// **只读字段不得混入**（契约 `validate_value` 会拒绝整单，见 PD12）。
    ///
    /// 敏感性：把 `filter(|f| f.editable)` 去掉 ⇒ 第 3 条变红；把 `f.default` 写成 `f.value`
    /// ⇒ 第 2 条变红（`telemetry.interval` 的当前值 1 ≠ 默认值 60）。
    #[test]
    fn defaults_patch_covers_all_editable_fields() {
        let v = view();
        let p = defaults_patch_of(&v);
        assert_eq!(p.from, PatchSource::ResetDefault);
        let editable: Vec<&str> = iter_fields(&v).filter(|f| f.editable).map(|f| f.key.as_str()).collect();
        assert_eq!(p.changes.len(), editable.len(), "覆盖全部可编辑字段");
        for k in &editable {
            assert!(p.changes.contains_key(*k), "缺字段 {k}");
        }
        assert!(
            !p.changes.contains_key("display.bind_addr"),
            "只读字段不得进补丁（后端二次校验会拒绝整单）"
        );
        assert_eq!(
            p.changes.get("telemetry.interval"),
            Some(&Value::from(60u64)),
            "取的是 default 而不是当前值"
        );
    }

    /// 分级：**任一**字段 `requires_reconnect` ⇒ 保存 L2+；否则 L1。
    ///
    /// 敏感性：把 [`save_level`] 的 `has_reconnect_field` 判据改成"看第一个字段"⇒ 第 2 条变红
    /// （本视图里瞬断字段排在最后）。
    #[test]
    fn save_level_follows_requires_reconnect() {
        let mut v = view();
        assert_eq!(save_level(&v), ConfirmLevel::L1, "无瞬断字段 ⇒ L1");
        let last = v.groups.last_mut().expect("组");
        let f = last.fields.first_mut().expect("字段");
        f.requires_reconnect = true;
        assert_eq!(
            save_level(&v),
            ConfirmLevel::L2Plus,
            "任一字段 requires_reconnect ⇒ L2+"
        );
        assert_eq!(
            reconnect_field_labels(&v),
            vec!["遥测上报周期".to_string()],
            "「涉及：」= 瞬断字段的上屏名"
        );
        assert_eq!(reset_level(&v), ConfirmLevel::L2Plus, "涉及瞬断 ⇒ 恢复默认值升 L2+");
    }

    /// 恢复默认值**最低 L2**（生效性写，不得只靠间距保护）。
    ///
    /// 敏感性：把 [`reset_level`] 的 `else` 分支改成 `L1` ⇒ 本条变红。
    #[test]
    fn reset_level_is_at_least_l2() {
        let v = view();
        assert_eq!(reset_level(&v), ConfirmLevel::L2);
        assert_ne!(reset_level(&v), ConfirmLevel::L1);
    }

    /// 字段标签口径（**PM 裁定**，设计 §6.2 / UI 附录 B U-1）：
    /// `gateway.listen_addr` **不得**上屏为「对端 IP 地址」；两个回环绑定字段用回环标签；
    /// 其余字段**透传契约标签**（元数据驱动）。
    ///
    /// 敏感性：把 [`LABEL_OVERRIDES`] 删掉 ⇒ 第 1、3 条变红；把 `find` 的键写成别的
    /// ⇒ 第 1 条变红。
    #[test]
    fn field_labels_follow_pm_ruling() {
        let v = view();
        let listen = iter_fields(&v).find(|f| f.key == "gateway.listen_addr").expect("字段");
        assert_eq!(field_label_text(listen), TEXT_LISTEN_ADDR);
        assert_ne!(field_label_text(listen), "对端 IP 地址", "不得表述为「对端 IP」");
        assert!(field_label_text(listen).contains("监听"), "口径 = 本机监听地址");
        let bind = iter_fields(&v).find(|f| f.key == "display.bind_addr").expect("字段");
        assert_eq!(field_label_text(bind), TEXT_LOOPBACK_ADDR);
        assert_ne!(field_label_text(bind), TEXT_LISTEN_ADDR, "回环服务地址与 IEC 104 监听地址**分列**");
        let port = iter_fields(&v).find(|f| f.key == "gateway.port").expect("字段");
        assert_eq!(field_label_text(port), "端口", "非裁定键**透传**契约标签");
    }

    /// 值格式化：整数带单位 / 枚举取**选项标签** / IPv4 原样 / 枚举未命中不臆造。
    ///
    /// 敏感性：把 `Enum` 分支改成返回 `raw` ⇒ 第 3 条变红；去掉单位拼接 ⇒ 第 2 条变红。
    #[test]
    fn format_value_shapes() {
        let u16k = ConfigKind::U16 {
            min: 1,
            max: 300,
            step: 1,
        };
        assert_eq!(format_value(&u16k, &Value::from(30), None), "30");
        assert_eq!(format_value(&u16k, &Value::from(30), Some("秒")), "30 秒");
        assert_eq!(
            format_value(&ConfigKind::Ipv4, &Value::from("127.0.0.1"), None),
            "127.0.0.1"
        );
        let ek = ConfigKind::Enum {
            options: vec![OptionItem {
                value: "info".into(),
                label: "信息".into(),
            }],
        };
        assert_eq!(format_value(&ek, &Value::from("info"), None), "信息");
        assert_eq!(format_value(&ek, &Value::from("trace"), None), "trace");
    }

    /// 约束提示：整数类给「min – max [单位]」，`Ipv4` / `Enum` 不画（UI §6.2 两版式）。
    ///
    /// 敏感性：给 `Ipv4` 分支也返回 `Some` ⇒ 第 3 条变红（行型 B 会多出一行提示）。
    #[test]
    fn range_hint_shapes() {
        let u64k = ConfigKind::U64 {
            min: 1,
            max: 300,
            step: 1,
        };
        assert_eq!(range_hint(&u64k, None).as_deref(), Some("1 – 300"));
        assert_eq!(range_hint(&u64k, Some("秒")).as_deref(), Some("1 – 300 秒"));
        assert_eq!(range_hint(&ConfigKind::Ipv4, None), None);
        assert_eq!(
            range_hint(
                &ConfigKind::Enum {
                    options: vec![]
                },
                None
            ),
            None
        );
    }

    /// 不可用文案：空 reason 只显标题；非空则「标题 · 原因」且原因经 `display_safe`。
    ///
    /// 敏感性：去掉 `display_safe` ⇒ 第 3 条变红（ASCII `-` 须改写成 `–` U+2013，且小写
    /// 须转大写同族 —— 生成字体 cmap 里没有 `-`、多数小写也没有字形）。
    #[test]
    fn unavailable_text_shapes() {
        assert_eq!(unavailable_text(""), TEXT_CONFIG_UNAVAILABLE);
        assert_eq!(unavailable_text("   "), TEXT_CONFIG_UNAVAILABLE);
        // 自证：`display_safe("ab-cd")` 必须是 `AB–CD`（否则本条第 3 条断言的"改写确实发生"
        // 就不成立；`s` 是小写里少数有字形的字符之一，故取不含 `s` 的样例）。
        assert_eq!(display_safe("ab-cd"), "AB\u{2013}CD");
        assert_eq!(unavailable_text("ab-cd"), "配置不可用 · AB\u{2013}CD");
    }

    /// 变更明细：保存路径只列**真变化**的字段；恢复路径列**全部可编辑**字段。
    ///
    /// 敏感性：把 `save_details` 的 `*now == f.value` 判断删掉 ⇒ 第 1 条变红；
    /// 把 `reset_details` 的 `filter(|f| f.editable)` 删掉 ⇒ 第 2 条变红。
    #[test]
    fn change_details_shapes() {
        let v = view();
        let mut cur = initial_values(&v);
        assert!(save_details(&v, &cur).is_empty(), "无改动 ⇒ 明细为空");
        cur.insert("gateway.port".into(), Value::from(2405));
        let d = save_details(&v, &cur);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].label, "端口");
        assert_eq!(d[0].before, "2404");
        assert_eq!(d[0].after, "2405");

        let r = reset_details(&v, &cur);
        let editable = iter_fields(&v).filter(|f| f.editable).count();
        assert_eq!(r.len(), editable, "恢复默认值列出全部可编辑字段");
        assert!(
            r.iter().all(|d| d.key != "display.bind_addr"),
            "只读字段不进明细"
        );
    }

    /// 整数边界：`u64` 超 `i64` 不 panic（收敛到 `i64::MAX`）。
    #[test]
    fn int_bounds_saturates() {
        let k = ConfigKind::U64 {
            min: 0,
            max: u64::MAX,
            step: 1,
        };
        assert_eq!(int_bounds(&k), (0, i64::MAX, 1));
        let u = ConfigKind::U16 {
            min: 1,
            max: 65535,
            step: 5,
        };
        assert_eq!(int_bounds(&u), (1, 65535, 5));
    }
}
