//! # P6「装置与外设」页（T21c-2；设计 §15.5 / UI §6.6.1）
//!
//! 本页 = **P6 改名**（「系统 / 关于」→「装置与外设」，T-8 裁定；**页数仍 6**）+
//! **页内 5 段分段控件**（装置 / 空调 / 电池 / 储能表 / PCS）+ **BMS 288 位下钻**。
//!
//! ## 五段（设计 §15.5.2；段序见 UI §6.6.1 线框图）
//!
//! | 段 | 内容 | 数据来源 |
//! |----|------|----------|
//! | 装置 | ① 外设站状态表（**恒 5 行**）② F8 既有三卡（**逐字保留**） | catalog `enabled` ∪ 帧 `online`；帧 `info` / `device` |
//! | 空调 | F20：4 组（测量值 / 运行状态 / 告警位（20） / 辅助状态位（10）） | catalog + 帧（站 `hvac`） |
//! | 电池 | F22：10 组（**`bms_alarm` 走下钻**，段内只放摘要卡 + 入口） | catalog + 帧（站 `bms`） |
//! | 储能表 | F23：6 组（**只列白名单键**） | catalog + 帧（站 `meter_batt`） |
//! | PCS | F24：6 组（**1046–1065 全排除**） | catalog + 帧（站 `pcs`） |
//!
//! ## 三条硬约束的落点
//!
//! 1. **页数不变 / 不新增页面层级**（§15.5.3 / T-20）：分段切换与下钻**都不改
//!    `current_page`**、不经任何导航路由 —— 本文件的**代码**里没有路由 / 页号 API
//!    （用例按"剥注释与字面量后源码不含 `current_page`"断言）。
//! 2. **降级不造假**（PRD §2.6 / F25）：每行降级一律 `–`（[`PLACEHOLDER`]）+ **契约给的
//!    原因**（「站离线」/「未取数」/「数据异常」/「未启用」/「站点未启用」，两两互异）；
//!    **不插值、不沿用旧值、不补 0**；原因**不屏侧重判**（来自 [`PeriphView::missing`] /
//!    [`StationState`]）。
//! 3. **值变化 / 取数成败都不改布局**（PRD §4.2.4）：行集合**由 catalog 驱动**（帧内取数
//!    成败只改行的**文本**，不改**行数与行高**）⇒ 降级行与在线时**同行高、同行数**。
//!
//! ## 窗口化（设计 §15.5.3「每段列表一律窗口化」—— 风险 A 的处置：**走"真窗口化"**）
//!
//! 薄层的滚动通道**已经可用**（不是 T21c-1 时那条"未镜像 `LV_EVENT_SCROLL`"的结论）：
//! [`EventCode::SCROLL`] 已镜像（B4b）、[`Obj::scroll_y`] 可读、`Obj::on(SCROLL, ..)` 可注册
//! ⇒ 行对象**可按滚动位置复用**。故：
//!
//! - **每个外设段**与**下钻视图**各有一支**行池**，池大小 = **可视行 ×1.5 + 1**（与行数
//!   **无关**）；滚动时按 `scroll_y` 把池里每一行重新绑到"当前窗口"里的那一行数据；
//! - **分组卡框**另有**小池**（`SEG_CARD_POOL` 个 `Obj`），按"当前可见的组"复用；卡框在池行
//!   **之前**创建 ⇒ 池行绘在卡框之上（偏差 **P6-1**）；
//! - **段「装置」不窗口化**（**已登记偏差 P6-5**）：它的行集是 `F8 三卡`的一次性固定行集
//!   （§6.6 已批准版面 + 既有断言面）⇒ 无"随点数增长"的风险。
//!
//! ## 偏差登记（**集中、显式**；屏文 / 尺寸与契约不一致处一律在此列明）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 计划收口 |
//! |---|----------------------|------|----------|
//! | **P6-1** | 池行**不是**卡框的子对象（行在卡框之上、由行池按滚动位置复用），卡内文字的 x 由 `CARD_INSET` 给出（与卡框内边距同值） | 窗口化行池的**结构性后果**：行对象必须能被任意组复用 ⇒ 不能"挂在某一个卡里" | 无需收口（视觉等价：卡框底板 + 内缩文字） |
//! | **P6-2** | ✅ **已收口（T21c-2-r1 / F2）**：站状态条按 UI §6.6.1 恢复**四列** —— 站名 │ 状态 │ **成功** `hh:mm:ss` │ **更新** `hh:mm:ss`（两个时刻列**各占一槽**，池行相应加第 4 个文字槽 `l4`）；**标签取缩短形态**（`ui_text::LAST_OK_SHORT` / `LAST_UPDATE_SHORT`，产品裁定 2026-09-25）。~~原登记：「时刻两列合并为一条 24 px 标签」+「段「装置」的 5 行表仍是四列」—— 后半句**不实**（两处用的是同一条 `PoolRow::bind`，段「装置」也是 3 槽）~~ | 合并串在 `last_ok_ms` 取真值时 = **424 px** > 段「装置」右槽 **398 px** ⇒ `DOTS` 必截断（R-38 落地即现）⇒ 拆列 + 缩标签（每列 146 px ≤ 165 px 槽） | —（已实现；版面断言见 `ui/tests.rs` 的 `pages_chain` ⑰，含 `23:59:59` 上界形态） |
//! | **P6-3** | **位行的状态词由 `LedIndicator` 的**文字通道**承担**（字号 = 该组件的 `TextSlot::Label` = 26 px），契约写 24 px | `LedIndicator` 是 F14 三通道的既有唯一构造点（圆 16 + 图标 + 文字），其文字槽固定 | 若要 24 px ⇒ 改 `components.rs` 的共享组件（影响 P1/P4 全部使用点） |
//! | **P6-4** | ✅ **已收口（T21c-2-r1 / F3；产品裁定 2026-09-25）**：`hvac_di_8`（系统运行）两态词 = **「运行」/「停止」**（PRD **EX-06**）—— **由 catalog 承载**：契约层 `BitMeta` 增 `inactive_text`（`#[serde(default)]`），生产侧 `console_host.rs::bit_state_words` 填 `运行`/`停止`（文案取自 `display-proto::ui_text`），屏侧 `state::bit_text` 逐位取用、缺 `inactive_text` 时回退「非活跃」 | ~~原登记："理由 = 契约的 `BitMeta` 只有 `active_text` ⇒ 屏侧只能取通用非活跃"~~ —— 该理由被**本仓自身证伪**（`ui_text::ENUM_STOPPED` / `ENUM_RUNNING` 早已在仓且零上屏）；`BitMeta` 无 `inactive_text` **属实**，但那是**待补的契约缺口**、不是"只能如此" | —（已实现；用例 = `ui/tests.rs::t14_hvac_run_bit_renders_the_two_state_words_carried_by_catalog` + `mupc-core-bin` 的同名生产侧用例） |
//! | **P6-5** | **段「装置」不窗口化**（4 个外设段与下钻**均窗口化**） | 该段行集 = 站状态表 5 行 + F8 三卡的**固定行集**（§6.6 已批准版面 + 既有断言面），与点数、与 catalog 无关 ⇒ 对象数由常量钉死 | 无需收口（设计 §15.5.3 已按 W-4 回写为「**含数据行驱动的段**一律窗口化」，豁免理由即本行） |
//! | **P6-6** | ✅ **下钻按"两位一行 + 池内复用"实现**（`page_size = 50`），池 = 可视行 ×1.5 + 1；**窗口的滚动重绑已接**（T21c-2-r1 / F1：`BmsDrill::wire_scroll` 与 4 个外设段同款 ⇒ 池外行**不再**永不可绘） | 同 P6-1 | —（已实现；F1 用例见 `pages_chain` ⑰ 的 ④″ 段）。⚠️ **P4 的下钻**：其"不窗口化"的依据（**IL29⑥** 的"薄层 `EventCode` 未镜像 `LV_SCROLL`"）**已被证伪**（该事件码自 **B4b `297b51b`，2026-09-16** 起即在仓）⇒ 那处是**无依据的降级**，**登记为待单独立项**（属 P4 的改动面，**本批未动 P4**） |
//! | **P6-8** | **摘要卡实算高 174**（= 卡头 40 + 摘要行 44 + 入口行 56 + **卡内缩 2×17**），而 UI §6.6.1 写的 140（= 40+44+56）**不含卡内缩** | 与 F8 三卡**同一口径**：UI §6.6 给的矩形高（如装置信息卡 348）同样不含 `theme::card()` 的 17×2 内缩，而实现一律「内容高 + 2×`CARD_INSET`」 | 无需收口（**卡内三项 40/44/56 逐条精确**，由 `theme` 常量断言钉住） |
//! | **P6-7** | ✅ **已收口**：下钻视图是**段「电池」面板的子对象**（切到别的段必被隐藏）；「切走 P6 再切回**不残留下钻态**」由**外壳的切页那一拍**调 [`P6SystemPage::close_drill`]（`shell.rs::Core::select` —— 页面**看不到**自己是否可见，故该语义只能落在页级切换处） | 页面无"自身可见性"信息（外壳按 `HIDDEN` 切页） | —（已实现；回归断言见 `pages_chain` 的 ⑰ 块） |
//!
//! ⚠️ **本文件不得出现**负向验收点名的**误用串**（EX-08 / EX-21 / EX-23：把 1046–1065
//! 那一段（STS / 负载区）的键标注成"PCS 对外出力"类语义、或把某个**并不存在**的室外温湿度
//! 数值、或把**不在本增量内**的配变侧量混进本页）—— 负向验收**不配可见声明**（§15.7.3 的
//! 刻意决定），判据 = **本源码里没有那些串**（`t19_p6_...` 按原文扫描）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mupc_display_proto::peripherals_labels::{ui_text, GROUP_TITLES};
use mupc_display_proto::{
    periph_whitelist_contains, BmsAlarmPage, CatalogBlockKind, CatalogPoint, CatalogStation,
    LinkState, PeriphRole, PeripheralCatalog, PeripheralStation, PeripheralsSection, PointValue,
    ServiceScope, DEFAULT_BIND, DEFAULT_CONTROL_BIND,
};

use crate::lvgl::display::Area;
use crate::lvgl::event::EventCode;
use crate::lvgl::obj::Obj;
use crate::lvgl::style::{Color, Style, StyleSelector};
use crate::lvgl::widgets::{Label, LongMode, ScrollContainer, TextButton};
use crate::lvgl::LvglError;
use crate::state::{bit_active, bit_text, station_state, MissingReason, PeriphView, StationState};
use crate::ui::components::{LedIndicator, StatusChip, WarnBanner};
use crate::ui::pages::{
    control_source_text, decor, display_safe, fmt_decimals, fmt_int0, format_epoch_ms_utc,
    format_uptime, frame_mark, label, layout_box, link_color, link_icon, page_root, sections,
    set_style_index, set_visible, text_label, CbSlot, PageInput, SegmentedTabs, LED_STATES,
    MISSING, PLACEHOLDER,
};
use crate::ui::theme::{self, Dimens, Palette, TextSlot};

// ═══════════════════════════════════════════════════════════════════════════
// 0. 上屏文案
//
// **纪律（D22 / §15.7.3）**：上屏中文**一律**取自 `display-proto`（`ui_text` / 分组标题 /
// 短标签表）或 catalog（`label` / `unit` / `bits[].label` / `enum_labels`）；本文件**零裸
// 中文字面量**（机器键 / 注释除外）。F8 三卡的文案沿用本文件的历史常量（B2a 起的分工）。
// ═══════════════════════════════════════════════════════════════════════════

/// 卡头：装置信息。
pub const TEXT_DEVICE_INFO: &str = "装置信息";
/// 字段：装置型号。
pub const TEXT_MODEL: &str = "装置型号";
/// 字段：序列号。
pub const TEXT_SERIAL: &str = "序列号";
/// 字段：固件版本。
pub const TEXT_FIRMWARE: &str = "固件版本";
/// 字段：编译时间。
pub const TEXT_BUILD_TIME: &str = "编译时间";
/// 卡头：运行信息。
pub const TEXT_RUN_INFO: &str = "运行信息";
/// 字段：系统运行时长。
pub const TEXT_UPTIME: &str = "系统运行时长";
/// 字段：CPU 温度。
pub const TEXT_CPU_TEMP: &str = "CPU 温度";
/// 字段：内存使用率。
pub const TEXT_MEM: &str = "内存使用率";
/// 字段：调度主站连接。
pub const TEXT_LINK_IEC104: &str = "调度主站连接";
/// 字段：核间连接。
pub const TEXT_LINK_INTERCORE: &str = "核间连接";
/// 字段：数据通道。
pub const TEXT_CHANNEL: &str = "数据通道";
/// 字段：当前控制源。
pub const TEXT_CONTROL_SOURCE: &str = "当前控制源";
/// 卡头：关于本屏。
pub const TEXT_ABOUT: &str = "关于本屏";
/// 字段：本地屏版本。
pub const TEXT_LOCAL_VERSION: &str = "本地屏版本";
/// 字段：本机服务地址（**仅回环**）。
///
/// ⚠️ **字符集偏差（B2a 报告，沿用）**：设计 / UI §6.6 的行名是「本机服务地址（仅回环）」，
/// 但 `服务` / `环` 与全角圆括号都不在生成字体 cmap 内 ⇒ 真机豆腐块。故取
/// 「本机**监听**地址 · 仅本机」（`监听` / `仅` / `本机` 均在集合内）。
pub const TEXT_SERVICE_ADDR: &str = "本机监听地址 · 仅本机";
/// 字段：设备管理 IP（与上一行**分列**，不得混为一谈）。
pub const TEXT_MGMT_IP: &str = "装置 IP 地址";
/// 说明行（UI §6.6）。
pub const TEXT_NO_REMOTE: &str = "本屏不提供远程访问与文件导出";
/// 控制源固定文案的字符集内变体（**M3**：唯一定义在 `pages/mod.rs`）。
pub const TEXT_AI_DISABLED: &str = crate::ui::pages::TEXT_AI_DISABLED;
/// 百分比单位。
pub const TEXT_PERCENT: &str = "%";
/// 温度单位（⚠️ `℃` / `°` 都不在字符集内 ⇒ F8 的 CPU 温度取 `C`；外设量纲走 catalog 单位）。
pub const TEXT_CELSIUS: &str = "C";
/// 服务地址分隔符（多端点并列）。
pub const TEXT_ADDR_SEP: &str = " / ";
/// 子句分隔符（站状态条的两个时刻列 / 下钻顶部条的三段）。
const TEXT_SEP: &str = " · ";
/// 分页分隔符（「第 X / Y 页」）。
const TEXT_PAGE_SEP: &str = " / ";
/// 空格（拼串用；ASCII，非中文文案）。
const TEXT_SPACE: &str = " ";

// ── 五段（段名全部来自 `ui_text`；段序 = UI §6.6.1 线框图）────────────────────

/// 段「装置」（含 F8 三卡）。
const TAB_LABEL_DEVICE: &str = ui_text::TAB_DEVICE;
/// 段「空调」。
const TAB_LABEL_HVAC: &str = ui_text::ROLE_HVAC;
/// 段「电池」。
const TAB_LABEL_BATTERY: &str = ui_text::ROLE_BATTERY;
/// 段「储能表」。
const TAB_LABEL_METER: &str = ui_text::ROLE_METER_BATT;
/// 段「PCS」。
const TAB_LABEL_PCS: &str = ui_text::ROLE_PCS;

/// 五段（顺序即段序；`None` = 段「装置」，无对应站）。
const SEGMENTS: [(&str, Option<PeriphRole>); 5] = [
    (TAB_LABEL_DEVICE, None),
    (TAB_LABEL_HVAC, Some(PeriphRole::Hvac)),
    (TAB_LABEL_BATTERY, Some(PeriphRole::Battery)),
    (TAB_LABEL_METER, Some(PeriphRole::MeterBatt)),
    (TAB_LABEL_PCS, Some(PeriphRole::Pcs)),
];

/// 段「装置」的下标。
pub const SEG_DEVICE: usize = 0;
/// 段「电池」的下标（288 位下钻的落点段）。
pub const SEG_BATTERY: usize = 2;
/// 站状态表的 role 全集（**5 个**：hvac / fire / bms / meter_batt / pcs；R-3 裁定）。
const STATION_ROLES: [PeriphRole; 5] = [
    PeriphRole::Hvac,
    PeriphRole::Fire,
    PeriphRole::Battery,
    PeriphRole::MeterBatt,
    PeriphRole::Pcs,
];

// ═══════════════════════════════════════════════════════════════════════════
// 1. 栅格常量（UI §6.6 / §6.6.1；**全部由 theme 常量推导**）
// ═══════════════════════════════════════════════════════════════════════════

/// 卡内容区原点相对卡外缘的偏移（描边 1 + 内边距 16 —— `theme::card()` 的 `pad_all`）。
const CARD_INSET: i32 = theme::Stroke::THIN + Dimens::GAP_MIN;
/// 卡头高（F8 既有三卡：区块标题 28 + 缝 16 = 44，UI §6.6）。
const CARD_HEAD_H: i32 = TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN;
/// 字段行高（UI §6.6「4 行 × 56 px」）。
const ROW_H: i32 = Dimens::ROW_SYS_H;
/// 「字段名 | 值」两列布局：值列起点（内容宽的 1/3）。
const VALUE_COL_X: i32 = Dimens::CONTENT_W / 3;
/// 卡内可用宽（F8 三卡）。
const INNER_W: i32 = Dimens::CONTENT_W - 2 * CARD_INSET;
/// 值列可用宽。
const VALUE_W: i32 = INNER_W - VALUE_COL_X;
/// 连接类行的指示灯宽。
const LED_W: i32 = VALUE_W;
/// 说明行高（正文 24 + 上缝 16）。
const NOTE_H: i32 = TextSlot::Body.px() as i32 + Dimens::GAP_MIN;
/// 角标 x（贴卡内容区右缘）。
const FROZEN_CHIP_X: i32 = INNER_W - crate::ui::pages::FROZEN_CHIP_W;

// ── ⓪″ 段顶「名称表可能过期」提示条（U-73 / 设计 §15.3.1 第 2 / 3 句；T21c-3-r1）────
//
// 落点 = **段内容区最顶部**（分段页签之下 —— `Dimens::SECTION_Y`）：`stale` 时**各段的自带
// 滚动视口**（段「装置」的 `host` / 外设段的 `SegmentList::viewport` / 下钻的 `viewport`）
// 整体下移并等量变矮（**底缘不动** ⇒ 不越出段面板、不遮住既有内容、页面根不产生滚动条）；
// 默认 `stale == false` ⇒ 提示条不占位、各视口 `set_inset(0)` **逐像素回到既有版面**。
/// 提示条占用的**总让位高**（`WarnBanner` 全高 56 + 同组缝 16）。
const STALE_BAND_H: i32 = Dimens::BANNER_H + Dimens::GAP_GROUP;
/// 提示条本体宽（全宽 − 「重试」按钮槽 − 缝）。
const STALE_BANNER_W: i32 = Dimens::CONTENT_W - Dimens::TOUCH_MIN - Dimens::GAP_MIN;
/// 「重试」按钮 x（提示条右侧；净距 = `GAP_MIN`(16)）。
const STALE_RETRY_X: i32 = STALE_BANNER_W + Dimens::GAP_MIN;
/// 「重试」按钮 y（在 56 px 提示条内垂直居中）。
const STALE_RETRY_Y: i32 = theme::center_offset(Dimens::BANNER_H, Dimens::TOUCH_MIN);
/// `stale` ⇒ 让位高，否则 `0`（**让位判据只有这一处**；顺带避开 rustfmt 的
/// `single_line_if_else_max_width` 单行阈值）。
const fn stale_inset(stale: bool) -> i32 {
    if stale {
        STALE_BAND_H
    } else {
        0
    }
}

/// 装置信息卡行数（型号 / 序列号 / 固件版本 / 编译时间）。
const INFO_ROWS: usize = 4;
/// 运行信息卡行数（UI §6.6）。
const RUN_ROWS: usize = 7;
/// 关于本屏卡行数（本地屏版本 / 本机服务地址 / 设备管理 IP）。
const ABOUT_ROWS: usize = 3;
/// 装置信息卡高。
const INFO_CARD_H: i32 = CARD_HEAD_H + INFO_ROWS as i32 * ROW_H + 2 * CARD_INSET;
/// 运行信息卡高。
const RUN_CARD_H: i32 = CARD_HEAD_H + RUN_ROWS as i32 * ROW_H + 2 * CARD_INSET;
/// 关于本屏卡高（多一行说明）。
const ABOUT_CARD_H: i32 = CARD_HEAD_H + ABOUT_ROWS as i32 * ROW_H + NOTE_H + 2 * CARD_INSET;

// ── 站状态表（UI §6.6.1「站状态条…宽 992，行高 48」）────────────────────────

/// 站状态表的行数（**5 个 role 全集**；F25.5：与在线状态无关）。
const STATION_ROWS: usize = STATION_ROLES.len();
/// 站状态表的卡头（§6.6.1 的分组卡卡头 40）。
const ST_HEAD_H: i32 = Dimens::CARD_HEAD_H;
/// 站状态表行高（§6.6.1：48）。
const ST_ROW_H: i32 = Dimens::ROW_STATION_H;
/// 站状态条**第 1 列**（「站 `<角色中文名>`」26 px）的槽宽 = 内容宽的 **1/8**（124 px）。
///
/// 最长形态 = 「站 储能表」= **109 px**（实测 `adv_w` 求和，见 `ui/tests.rs` 的
/// `measured_text_px`）⇒ 124 留出余量；**改长标签（或改 role 名）越槽即红**。
const ST_NAME_W: i32 = Dimens::CONTENT_W / 8;
/// 第 2 列（状态词）槽宽 = 内容宽的 **1/12**（82 px）：最长 = 「站离线」/「未启用」= **72 px**
/// （24 px 档）⇒ 余量 10 px；把状态词改长（+1 字 = +24 px）即越槽。
const ST_STATE_W: i32 = Dimens::CONTENT_W / 12;
/// 第 3 / 4 列（两个时刻列）槽宽 = 内容宽的 **1/6**（165 px）：最长 = 「成功 12:03:44」=
/// **146 px**（24 px 档）⇒ 余量 19 px。
///
/// ⚠️ 该槽宽是 **R-38 落地（`last_ok_ms` 有真值 ⇒ 最长形态）** 的守门人：合并成一条时的
/// 424 px（「最后成功 … · 最近更新 …」）在 398 px 的 aux 槽里**必截断**（T21c-2 评审 F2）；
/// 四列 + 缩短标签后每列只需 146 px。**回退长标签**（「成功」→「最后成功」= 194 px）即越槽
/// ⇒ 由 `pages_chain` 的版面断言拦下。
const ST_TIME_W: i32 = Dimens::CONTENT_W / 6;
/// 站状态表整卡高（段「装置」）。
const ST_CARD_H: i32 = ST_HEAD_H + STATION_ROWS as i32 * ST_ROW_H + 2 * CARD_INSET;
/// 站状态表 y（段「装置」首件）。
const ST_CARD_Y: i32 = 0;
/// 段「装置」内容总高（站状态表 + F8 三卡 + 两道跨区缝）。
const DEVICE_TOTAL_H: i32 = ST_CARD_H
    + Dimens::GAP_GROUP
    + INFO_CARD_H
    + Dimens::GAP_SECTION
    + RUN_CARD_H
    + Dimens::GAP_SECTION
    + ABOUT_CARD_H;

/// F8 装置信息卡 y（排在站状态表之下）。
const INFO_CARD_Y: i32 = ST_CARD_Y + ST_CARD_H + Dimens::GAP_GROUP;
/// 运行信息卡 y。
const RUN_CARD_Y: i32 = INFO_CARD_Y + INFO_CARD_H + Dimens::GAP_SECTION;
/// 关于本屏卡 y。
const ABOUT_CARD_Y: i32 = RUN_CARD_Y + RUN_CARD_H + Dimens::GAP_SECTION;

// ── 行池（UI §6.6.1；设计 §15.5.3）────────────────────────────────────────

/// 单段可视行数（视口高 / 数据行高）。
const VISIBLE_ROWS: i32 = Dimens::SECTION_VIEW_H / Dimens::ROW_DATA_H;
/// 段行池大小 = **可视行 ×1.5 + 1**（设计 §15.5.3「可视行 ×1.5，沿用 §5.7 / R-22 范式」）。
///
/// **结构性要点**：本值与**行数无关** ⇒ "段内有 288 行还是 4 行"建出的行对象数相同
/// （T-18 / T-25 的"行数上界"断言即锚在这里）。
pub(crate) const SEG_ROW_POOL: usize = (VISIBLE_ROWS * 3 / 2 + 1) as usize;
/// 分组卡框池大小（最小卡高 = 卡头 + 一行 + 组缝 ⇒ 可视区内最多几张卡；+2 余量）。
const SEG_CARD_POOL: usize = (Dimens::SECTION_VIEW_H
    / (Dimens::CARD_HEAD_H + Dimens::ROW_DATA_H + Dimens::GAP_GROUP)
    + 2) as usize;

/// 下钻顶部条高（按钮高）。
const DRILL_TOP_H: i32 = Dimens::TOUCH_MIN;
/// 下钻行区 y。
const DRILL_BODY_Y: i32 = DRILL_TOP_H + Dimens::GAP_MIN;
/// 下钻行区高。
const DRILL_BODY_H: i32 = Dimens::SECTION_VIEW_H - DRILL_BODY_Y;
/// 下钻可视行数（两位一行）。
const DRILL_VISIBLE_ROWS: i32 = DRILL_BODY_H / Dimens::DRILL_ROW_H;
/// 下钻行池大小（可视行 ×1.5 + 1）。
pub(crate) const DRILL_ROW_POOL: usize = (DRILL_VISIBLE_ROWS * 3 / 2 + 1) as usize;
/// 下钻"两位一行"⇒ 每行两位。
const DRILL_ITEMS_PER_ROW: usize = 2;
/// 一页的位数（设计 §15.3.2 / UI §6.6.1：`page_size = 50`）。
const DRILL_PAGE_SIZE: u32 = 50;
/// 摘要卡「活跃位清单」最多几行（UI §6.6.1：**最多 4 行**）。
const BMS_SUMMARY_LIST_ROWS: usize = 4;
/// 下钻行的位号列宽（半宽的一部分：位号 + 短标签合成一条标签，占半区左侧）。
const DRILL_NAME_W: i32 = Dimens::CONTENT_W / 4;

// ═══════════════════════════════════════════════════════════════════════════
// 2. 纯逻辑：行模型（**不触碰 LVGL** ⇒ 可独立单测）
// ═══════════════════════════════════════════════════════════════════════════

/// 行的**种类**（决定排版与槽位用法）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// 分组卡头（标题行）。
    Header,
    /// 站状态条（**无卡框**：段「装置」的 5 行表与各外设段的段顶 1 行）。
    Station,
    /// 数值行（标签 + 值 + 单位）。
    Scalar,
    /// 位行（位名 + `LedIndicator`）。
    Bit,
    /// 枚举行（标签 + 枚举文案；表外值 ⇒ 「未知」）。
    Enum,
    /// **段级文案行**（「站点未启用」/「外设数据不可用」；无卡框、无缩进）。
    ///
    /// ⚠️ 与 §15.5.1 的"段内行"不同：它**不是数据行**，而是**整段的唯一文案**（情形 ⑤ / ⑦）。
    Notice,
    /// **特殊件**（页面自建、按本行的 y 摆放；如 BMS 告警摘要卡）。
    Special,
}

/// 一行（或一个特殊件）的**数据**（纯值；排版与绑定都只读它）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowData {
    /// 行种类。
    pub(crate) kind: RowKind,
    /// 行高（px；由 `theme` 常量给出）。
    pub(crate) h: i32,
    /// 左槽（分组标题 / 短标签 / 位名 / 站名）。
    pub(crate) label: String,
    /// 值槽（数值 / 枚举文案 / 位状态词 / 降级 `–`）。
    pub(crate) value: String,
    /// 右槽（单位 / 降级原因 / **站行的第 3 列**「成功 hh:mm:ss」）。
    pub(crate) aux: String,
    /// **第 4 槽**（**只有站状态行用**：第 4 列「更新 hh:mm:ss」；其余行恒空串 ⇒ 槽隐藏）。
    pub(crate) aux2: String,
    /// 位行的活跃态（`Some` ⇒ 用 `LedIndicator` 的灯通道）。
    pub(crate) active: Option<bool>,
    /// 该行是否降级（值槽被 `–` 占位）。
    pub(crate) degraded: bool,
}

impl RowData {
    /// 分组卡头。
    fn header(title: &str) -> Self {
        Self {
            kind: RowKind::Header,
            h: Dimens::CARD_HEAD_H,
            label: title.to_string(),
            value: String::new(),
            aux: String::new(),
            aux2: String::new(),
            active: None,
            degraded: false,
        }
    }

    /// 段级文案行（整段唯一文案；无卡、无缩进）。
    fn notice(text: &str) -> Self {
        Self {
            kind: RowKind::Notice,
            h: Dimens::ROW_DATA_H,
            label: text.to_string(),
            value: String::new(),
            aux: String::new(),
            aux2: String::new(),
            active: None,
            degraded: true,
        }
    }

    /// 站状态行。
    fn station(name: String, status: String, ok: String, update: String) -> Self {
        Self {
            kind: RowKind::Station,
            h: ST_ROW_H,
            label: name,
            value: status,
            aux: ok,
            aux2: update,
            active: None,
            degraded: false,
        }
    }
}

/// 一段的行模型（行 + 排版 + 分组卡矩形）。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SegmentModel {
    /// 行（顺序 = 分组序 → 块序 → `at` 升序）。
    pub(crate) rows: Vec<RowData>,
    /// 每行的内容 y（与 `rows` 等长）。
    pub(crate) ys: Vec<i32>,
    /// 分组卡矩形 `(y, h)`。
    pub(crate) cards: Vec<(i32, i32)>,
    /// 内容总高。
    pub(crate) total_h: i32,
}

/// 排版：**无卡行**（站状态条）贴顶摆放；其余按**分组**切段，每组一张卡
/// （卡高 = 2×`CARD_INSET` + 组内行高之和），组间 `GAP_GROUP`。
fn layout(rows: &[RowData]) -> (Vec<i32>, Vec<(i32, i32)>, i32) {
    let mut ys = Vec::with_capacity(rows.len());
    let mut cards: Vec<(i32, i32)> = Vec::new();
    let mut y = 0i32;
    let mut i = 0usize;
    while i < rows.len() {
        // 无卡行（站状态条 / 段级文案行）：不缩进、不加卡框、不留组缝。
        if matches!(rows[i].kind, RowKind::Station | RowKind::Notice) {
            ys.push(y);
            y += rows[i].h;
            i += 1;
            if i < rows.len() && !cards.is_empty() {
                y += Dimens::GAP_GROUP;
            }
            continue;
        }
        let card_y = y;
        y += CARD_INSET;
        let mut j = i + 1;
        while j < rows.len() && rows[j].kind != RowKind::Header {
            j += 1;
        }
        for row in &rows[i..j] {
            ys.push(y);
            y += row.h;
        }
        y += CARD_INSET;
        cards.push((card_y, y - card_y));
        y += Dimens::GAP_GROUP;
        i = j;
    }
    if !cards.is_empty() {
        y -= Dimens::GAP_GROUP;
    }
    (ys, cards, y)
}

/// 某 role 的**分组序**（§15.5.2 的组序；段内顺序固定 ⇒ 字段位置稳定不跳变）。
fn group_order(role: PeriphRole) -> &'static [&'static str] {
    const HVAC: &[&str] = &[
        machine_key("hvac_measure"),
        machine_key("hvac_run"),
        machine_key("hvac_alarm"),
        machine_key("hvac_state"),
    ];
    const BATTERY: &[&str] = &[
        machine_key("bms_core"),
        machine_key("bms_health"),
        machine_key("bms_cell_extreme"),
        machine_key("bms_delta"),
        machine_key("bms_power"),
        machine_key("bms_term"),
        machine_key("bms_energy"),
        machine_key("bms_pole_temp"),
        machine_key("bms_device"),
        machine_key("bms_alarm"),
    ];
    const METER: &[&str] = &[
        machine_key("mb_u_i"),
        machine_key("mb_freq"),
        machine_key("mb_power"),
        machine_key("mb_pf"),
        machine_key("mb_energy"),
        machine_key("mb_quality"),
    ];
    const PCS: &[&str] = &[
        machine_key("pcs_ac_u_f"),
        machine_key("pcs_ac_power"),
        machine_key("pcs_dc"),
        machine_key("pcs_temp"),
        machine_key("pcs_energy"),
        machine_key("pcs_mode"),
    ];
    match role {
        PeriphRole::Hvac => HVAC,
        PeriphRole::Battery => BATTERY,
        PeriphRole::MeterBatt => METER,
        PeriphRole::Pcs => PCS,
        PeriphRole::Fire | PeriphRole::Unknown => &[],
    }
}

/// 分组键 → 中文标题（`GROUP_TITLES` 是唯一真源；未知键 ⇒ `None`，**不臆造**）。
fn group_title(key: &str) -> Option<&'static str> {
    GROUP_TITLES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, t)| *t)
}

/// 点 → 一行（**唯一**的"点 → 屏上行"构造点）。
///
/// 判据：① 站级优先（[`PeriphView::derive`] 已把 [`StationState`] 并进 `missing`）；
/// ② 位块 ⇒ 位行（未定义位 ⇒ `state::bit_text` 给出的「未定义位 n」）；
/// ③ 有 `enum_labels` ⇒ 枚举行（表外值 ⇒ 「未知」）；④ 其余 ⇒ 数值行。
fn row_of_point(
    at: u16,
    is_bit_block: bool,
    st: StationState,
    pv: Option<&PointValue>,
    meta: Option<&CatalogPoint>,
) -> RowData {
    // 契约缺省：**帧内没有该点** ⇒ `NotRead`（**不补 0**）。
    let synthetic = PointValue {
        at,
        v: None,
        flag: mupc_display_proto::FieldFlag::NotRead,
    };
    let pv = pv.unwrap_or(&synthetic);
    let view = PeriphView::derive(st, pv, meta);

    if is_bit_block {
        let index = u8::try_from(at.saturating_sub(1)).unwrap_or(u8::MAX);
        let bit = meta.and_then(|m| m.bits.iter().find(|b| b.index == index));
        let defined = bit.map(|b| b.defined).unwrap_or(false);
        // **`bit_text` 是位行文案的唯一构造点**（`state.rs`）：① 未定义位 ⇒「未定义位 n」；
        // ② 已定义位 ⇒ **以空位名调用即得状态词**（catalog 的 `active_text` / `inactive_text`
        //    两态词优先，缺失时回退通用活跃 / 非活跃；`inverted` 位的在线｜离线）
        //    —— 本文件不另造第二份判据。
        if !defined {
            return RowData {
                kind: RowKind::Bit,
                h: Dimens::ROW_BIT_H,
                label: bit_text(index, false, "", false, None, None, false),
                value: String::new(),
                aux: String::new(),
                aux2: String::new(),
                active: None,
                degraded: false,
            };
        }
        let active = view
            .value
            .map(|raw| bit_active(raw, index))
            .unwrap_or(false);
        let active_text = bit.and_then(|b| b.active_text.as_deref());
        let inactive_text = bit.and_then(|b| b.inactive_text.as_deref());
        let inverted = bit.map(|b| b.inverted).unwrap_or(false);
        let state = bit_text(
            index,
            true,
            "",
            active,
            active_text,
            inactive_text,
            inverted,
        );
        let name = bit.map(|b| b.label.clone()).unwrap_or_default();
        return RowData {
            kind: RowKind::Bit,
            h: Dimens::ROW_BIT_H,
            label: if name.is_empty() {
                ui_text::NAME_UNKNOWN.to_string()
            } else {
                name
            },
            value: state.trim().to_string(),
            aux: String::new(),
            aux2: String::new(),
            active: Some(active),
            degraded: false,
        };
    }

    // 降级：值槽 `–` + 右槽原因（**只读契约给的原因**）。
    if let Some(reason) = view.missing {
        return RowData {
            kind: RowKind::Scalar,
            h: Dimens::ROW_DATA_H,
            label: view.label_text().to_string(),
            value: PLACEHOLDER.to_string(),
            aux: reason.text().to_string(),
            aux2: String::new(),
            active: None,
            degraded: true,
        };
    }

    // 枚举：catalog 给了值域 ⇒ 逐字照抄；**表外值 ⇒ 「未知」**（绝不落「正常」）。
    if let Some(labels) = meta
        .map(|m| m.enum_labels.as_slice())
        .filter(|l| !l.is_empty())
    {
        return RowData {
            kind: RowKind::Enum,
            h: Dimens::ROW_DATA_H,
            label: view.label_text().to_string(),
            value: enum_text(view.value, labels),
            aux: String::new(),
            aux2: String::new(),
            active: None,
            degraded: false,
        };
    }

    match view.value {
        Some(v) => RowData {
            kind: RowKind::Scalar,
            h: Dimens::ROW_DATA_H,
            label: view.label_text().to_string(),
            value: fmt_decimals(v, view.decimals),
            aux: view.unit.clone().unwrap_or_default(),
            aux2: String::new(),
            active: None,
            degraded: false,
        },
        // `missing = None` 且 `value = None` = 契约自相矛盾（构造上不可达）⇒ 按"数据异常"
        // 降级，**绝不补 0**（纵深防御）。
        None => RowData {
            kind: RowKind::Scalar,
            h: Dimens::ROW_DATA_H,
            label: view.label_text().to_string(),
            value: PLACEHOLDER.to_string(),
            aux: MissingReason::RangeError.text().to_string(),
            aux2: String::new(),
            active: None,
            degraded: true,
        },
    }
}

/// 枚举值 → 文案（**表外值 ⇒ 「未知」**；与 `state::fire_level_text` 同口径，但这里是
/// **通用**实现 —— 那个函数绑定火警的六值锁定表，属 P4）。
fn enum_text(value: Option<f64>, labels: &[(u16, String)]) -> String {
    let key = value.filter(|v| v.is_finite()).map(f64::round);
    match key.and_then(|k| {
        labels
            .iter()
            .find(|(v, _)| f64::from(*v) == k)
            .map(|(_, t)| t.clone())
    }) {
        Some(t) => t,
        None => ui_text::ENUM_UNKNOWN.to_string(),
    }
}

/// 站状态行的**第 3 列**（「成功 hh:mm:ss」）。
///
/// ⚠️ `last_ok_ms = 0` = **从未成功**（§15.2.2 的字段语义；R-38 未落地时恒 0）⇒ 显 `–`，
/// **不臆造**（T-24 口径）。
///
/// 标签取**缩短形态**（[`ui_text::LAST_OK_SHORT`] = 「成功」）：UI §6.6.1 的**四列口径不变**，
/// 缩短的是标签（「最后成功」/「最近更新」→「成功」/「更新」）、值仍是 `hh:mm:ss`
/// —— T21c-2-r1 / F2 的产品裁定（2026-09-25）。
fn station_ok_text(last_ok_ms: u64) -> String {
    format!(
        "{}{}{}",
        ui_text::LAST_OK_SHORT,
        TEXT_SPACE,
        clock_hms(last_ok_ms)
    )
}

/// 站状态行的**第 4 列**（「更新 hh:mm:ss」；标签缩短口径同 [`station_ok_text`]）。
fn station_update_text(block_ts_ms: u64) -> String {
    format!(
        "{}{}{}",
        ui_text::LAST_UPDATE_SHORT,
        TEXT_SPACE,
        clock_hms(block_ts_ms)
    )
}

/// Unix ms → `HH:MM:SS`（`0` ⇒ `–`）。**UTC**（与 `format_epoch_ms_utc` 同口径）。
fn clock_hms(ms: u64) -> String {
    if ms == 0 {
        return PLACEHOLDER.to_string();
    }
    let t = format_epoch_ms_utc(ms);
    t.split_once(' ')
        .map(|(_, time)| time.to_string())
        .unwrap_or(t)
}

/// role → 中文名（取自 `ui_text`；`Unknown` ⇒ 「未知」，**不臆造**）。
fn role_name(role: PeriphRole) -> &'static str {
    match role {
        PeriphRole::Hvac => ui_text::ROLE_HVAC,
        PeriphRole::Fire => ui_text::ROLE_FIRE,
        PeriphRole::Battery => ui_text::ROLE_BATTERY,
        PeriphRole::MeterBatt => ui_text::ROLE_METER_BATT,
        PeriphRole::Pcs => ui_text::ROLE_PCS,
        PeriphRole::Unknown => ui_text::ENUM_UNKNOWN,
    }
}

/// 站状态行（段「装置」的 5 行表 / 各外设段的段顶 1 行）。
fn station_rows(
    cat: Option<&PeripheralCatalog>,
    sec: &PeripheralsSection,
    roles: &[PeriphRole],
) -> Vec<RowData> {
    roles
        .iter()
        .map(|role| {
            let st = station_state_of(cat, sec, *role);
            let frame = frame_station(sec, *role);
            let last_ok = frame.map(|f| f.last_ok_ms).unwrap_or(0);
            let ts = frame
                .map(|f| f.blocks.iter().map(|b| b.ts_ms).max().unwrap_or(0))
                .unwrap_or(0);
            RowData::station(
                format!(
                    "{}{}{}",
                    ui_text::STATION_PREFIX,
                    TEXT_SPACE,
                    role_name(*role)
                ),
                st.row_text().to_string(),
                station_ok_text(last_ok),
                station_update_text(ts),
            )
        })
        .collect()
}

/// 建某段的模型（**唯一**的"段 → 行"构造点；纯函数）。
///
/// - `special_h`：段尾特殊件（仅电池段的 BMS 告警摘要卡）的高度；`Some(h)` 时在
///   `bms_alarm` 分组处放一行 [`RowKind::Special`]（卡片本体由页面建、按该行的 y 摆）。
/// - 行集合**由 catalog 驱动**（catalog 缺则退到帧：名位显「名称未获取」，**值照常显示**，
///   §15.3.1）；**白名单外的一律不产行**（T-19 的结构性前提）。
pub(crate) fn segment_model(
    role: PeriphRole,
    cat: Option<&PeripheralCatalog>,
    sec: &PeripheralsSection,
    special_h: Option<i32>,
) -> SegmentModel {
    let st = station_state_of(cat, sec, role);
    let cat_st = cat_station(cat, role);
    let frame_st = frame_station(sec, role);

    // 情形⑦（R-3 裁定）：该 role **根本未配置** ⇒ 段内**没有"这一点的值"可谈**
    // （`StationState::Disabled` 的点级原因恒 `None`）⇒ 整段只显段级文案「站点未启用」。
    if st == StationState::Disabled {
        let rows = vec![RowData::notice(ui_text::SECTION_STATION_DISABLED)];
        let (ys, cards, total_h) = layout(&rows);
        return SegmentModel {
            rows,
            ys,
            cards,
            total_h,
        };
    }

    struct Pt<'a> {
        group: &'static str,
        block: String,
        at: u16,
        is_bit: bool,
        meta: Option<&'a CatalogPoint>,
        pv: Option<&'a PointValue>,
    }
    let mut pts: Vec<Pt<'_>> = Vec::new();
    if let Some(st_cat) = cat_st {
        for block in &st_cat.blocks {
            let is_bit = block.kind == CatalogBlockKind::Discrete;
            for p in &block.points {
                if !periph_whitelist_contains(role, &block.name, p.at) {
                    continue;
                }
                let pv = frame_st.and_then(|s| {
                    s.blocks
                        .iter()
                        .find(|b| b.name == block.name)
                        .and_then(|b| b.values.iter().find(|v| v.at == p.at))
                });
                pts.push(Pt {
                    group: group_of_point(role, &block.name, p.at),
                    block: block.name.clone(),
                    at: p.at,
                    is_bit,
                    meta: Some(p),
                    pv,
                });
            }
        }
    } else if let Some(st_frame) = frame_st {
        for block in &st_frame.blocks {
            for pv in &block.values {
                if !periph_whitelist_contains(role, &block.name, pv.at) {
                    continue;
                }
                pts.push(Pt {
                    group: group_of_point(role, &block.name, pv.at),
                    block: block.name.clone(),
                    at: pv.at,
                    // 无 catalog ⇒ 判不出块形态（`kind` 在 catalog 里）⇒ 一律按数值行渲染
                    // （**已登记的降级**：名位是「名称未获取」，位块的点因此显示 `0` / `1`）。
                    is_bit: false,
                    meta: None,
                    pv: Some(pv),
                });
            }
        }
    }

    let mut rows: Vec<RowData> = Vec::new();
    // 情形⑤（EDGE-22）：整段源不可得 ⇒ **段顶声明**「外设数据不可用」；
    // 行仍照常列出（R-40 的口径：「常显」= 段内无隐藏条件，**不因取数成败隐藏**），
    // 每行的原因来自契约（`未取数` / `名称未获取`），**不显 0、不显「正常」**。
    if !sec.available {
        rows.push(RowData::notice(ui_text::PERIPH_UNAVAILABLE));
    }
    for key in group_order(role) {
        let mut mine: Vec<&Pt<'_>> = pts.iter().filter(|p| p.group == *key).collect();
        if mine.is_empty() {
            continue;
        }
        mine.sort_by(|a, b| (a.block.as_str(), a.at).cmp(&(b.block.as_str(), b.at)));
        let Some(title) = group_title(key) else {
            continue;
        };
        // `bms_alarm`：段内只放**摘要卡**（288 位走下钻，§15.5.2 的硬要求）。
        if *key == machine_key("bms_alarm") {
            rows.push(RowData::header(title));
            if let Some(h) = special_h {
                rows.push(RowData {
                    kind: RowKind::Special,
                    h,
                    label: String::new(),
                    value: String::new(),
                    aux: String::new(),
                    aux2: String::new(),
                    active: None,
                    degraded: false,
                });
            }
            continue;
        }
        rows.push(RowData::header(title));
        for p in mine {
            rows.push(row_of_point(p.at, p.is_bit, st, p.pv, p.meta));
        }
    }

    let (ys, cards, total_h) = layout(&rows);
    SegmentModel {
        rows,
        ys,
        cards,
        total_h,
    }
}

/// **机器键**的显式豁免口（T21c-2）。
///
/// 帧内块名 / 分组键是**小写 ASCII**（如块名与分组键），**从不上屏**（上屏的是 catalog 的
/// 短标签与 `GROUP_TITLES` 的中文标题），而生成字体里没有这些小写字形 ⇒ 源码里的这些字面量
/// 必须经本函数，**且**本函数已登记在 `ui/tests.rs` 的
/// [`NON_DISPLAY_SINKS`](crate::ui::tests) 里（码表网的豁免面）。
///
/// **为什么不能直接写字面量**：`ui_texts_covered_by_font_cmap` 会扫 `ui/**` 的**全部**字符串
/// 字面量，小写机器键会被误判成"缺字形 ⇒ 真机豆腐块"。
///
/// **防滥用**（与 P4 的 `block_key` / `group_title` 同一条纪律）：静态用例
/// `p6_static_constraints` 钉住本函数的**调用点计数**，并断言**每个实参都是纯 ASCII**
/// （`[a-z0-9_]`）—— 把上屏中文塞进来会当场红。
const fn machine_key(key: &'static str) -> &'static str {
    key
}

/// `(role, block, at)` → 分组键（**静态唯一真源** = `display-proto::group_of`）。
fn group_of_point(role: PeriphRole, block: &str, at: u16) -> &'static str {
    mupc_display_proto::peripherals_labels::group_of(role, block, at)
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 契约查找助手（**与 `p4_interlock.rs` 的同名私有助手逐字同源**）
//
// ⚠️ **重复的理由（如实登记）**：这些助手是 P4 与本页**都要**的"catalog / 帧 → 展示视图"
// 通路。按 **M3** 的惯例应上收 `pages/mod.rs` 共用；但**本批任务书明令不改 P4 页**
// ⇒ 只在本文件复刻（语义逐条相同），上收留给"P4 页的改动面"那一次独立动作。
// ═══════════════════════════════════════════════════════════════════════════

/// catalog 里的站（`None` = 该站未进 catalog / catalog 未取到）。
fn cat_station(cat: Option<&PeripheralCatalog>, role: PeriphRole) -> Option<&CatalogStation> {
    cat.and_then(|c| c.stations.iter().find(|s| s.role == role))
}

/// 帧内的站（`None` = 帧内**不含**该站 ⇒ 缺席，§15.2.2）。
fn frame_station(sec: &PeripheralsSection, role: PeriphRole) -> Option<&PeripheralStation> {
    sec.stations.iter().find(|s| s.role == role)
}

/// 站级态（catalog `enabled` ∪ 帧内 `online`；**判据单一** = `state::station_state`）。
fn station_state_of(
    cat: Option<&PeripheralCatalog>,
    sec: &PeripheralsSection,
    role: PeriphRole,
) -> StationState {
    station_state(
        cat_station(cat, role).map(|s| s.enabled),
        frame_station(sec, role).map(|s| s.online),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 池行（窗口化的复用单元）
// ═══════════════════════════════════════════════════════════════════════════

/// 池里一行 = 容器 + 3 个文字槽（+ **含位行的段**才有 `LedIndicator`）。
///
/// **为什么"3 个固定槽 + 运行期改字体/颜色"**：窗口化要求**同一个对象**能被复用到任意一行
/// ⇒ 槽的数量与位置必须固定，而"这一槽此刻是标题还是单位"由 [`PoolRow::bind`] 按行种类设定
/// （字体与颜色一律经 `theme::font_of` 与 `Palette`，**页面不写裸值**）。
pub(crate) struct PoolRow {
    obj: Obj,
    l1: Rc<Label>,
    l2: Rc<Label>,
    l3: Rc<Label>,
    /// **第 4 槽**：站状态行的第二个时刻列（「更新 hh:mm:ss」）。
    ///
    /// 站状态条按 UI §6.6.1 是**四列**（站名 │ 状态 │ 成功 │ 更新）⇒ 池行必须能承载 4 个文字
    /// 槽（T21c-2-r1 / F2 的四列返工）。非站行一律隐藏它（**对象数不变**：槽在构造期一次建齐，
    /// 滚动期只改文本与可见性 ⇒ 不违反"滚动不得新建对象"）。
    l4: Rc<Label>,
    /// 位行的指示灯（只有"该段含位行"的池才建它；否则省 4 个对象/行）。
    led: Option<LedIndicator>,
    /// 本行的宽度（段「装置」的站状态表比内容区窄 2×`CARD_INSET`）。
    width: i32,
}

impl PoolRow {
    /// 建一行。
    fn new(parent: &Obj, width: i32, with_led: bool) -> Result<Self, LvglError> {
        let obj = layout_box(parent, width, Dimens::ROW_DATA_H)?;
        let l1 = Rc::new(label(&obj, TextSlot::SectionTitle, Palette::TEXT_SECOND)?);
        let l2 = Rc::new(label(&obj, TextSlot::CardValue, Palette::TEXT_PRIMARY)?);
        let l3 = Rc::new(label(&obj, TextSlot::Body, Palette::TEXT_SECOND)?);
        let l4 = Rc::new(label(&obj, TextSlot::Body, Palette::TEXT_SECOND)?);
        for l in [&l1, &l2, &l3, &l4] {
            l.set_long_mode(LongMode::DOTS);
        }
        let led = if with_led {
            Some(LedIndicator::new(
                &obj,
                Dimens::ICON_SM,
                "●",
                ui_text::BIT_INACTIVE,
                Palette::LINK_UNCONFIGURED,
            )?)
        } else {
            None
        };
        Ok(Self {
            obj,
            l1,
            l2,
            l3,
            l4,
            led,
            width,
        })
    }

    /// 一个文字槽的排版（字体槽 + 颜色 + x + 宽 + y）。
    fn place(slot: &Label, x: i32, w: i32, y: i32, font: TextSlot, color: Color) {
        slot.set_size(w.max(0), font.px() as i32);
        slot.set_pos(x, y);
        slot.set_text_font(&theme::font_of(font));
        slot.set_text_color(color);
    }

    /// 把本行绑到 `row`（`y` = 行顶，**内容坐标**）。
    fn bind(&self, row: &RowData, y: i32) {
        self.obj.set_pos(0, y);
        self.obj.set_size(self.width, row.h);
        for l in [&self.l1, &self.l2, &self.l3, &self.l4] {
            set_visible(l, false);
        }
        if let Some(led) = &self.led {
            set_visible(led.obj(), false);
        }
        // **卡内行**左缩 `CARD_INSET`（无卡行不缩）—— 见偏差 P6-1。
        let inset = if matches!(
            row.kind,
            RowKind::Station | RowKind::Notice | RowKind::Special
        ) {
            0
        } else {
            CARD_INSET
        };
        let inner_x = inset + Dimens::GAP_MIN;
        let inner_w = (self.width - inset * 2 - 2 * Dimens::GAP_MIN).max(0);
        let name_y = theme::center_offset(row.h, TextSlot::SectionTitle.px() as i32);
        match row.kind {
            RowKind::Header => {
                Self::place(
                    &self.l1,
                    inner_x,
                    inner_w,
                    theme::center_offset(row.h, TextSlot::SectionTitle.px() as i32),
                    TextSlot::SectionTitle,
                    Palette::TEXT_PRIMARY,
                );
                self.l1.set_text(&row.label);
                set_visible(&self.l1, true);
            }
            RowKind::Station => {
                // **四列**（UI §6.6.1）：站名 │ 在线 / 站离线 │ 成功 hh:mm:ss │ 更新 hh:mm:ss。
                // 时刻两列**各占一列**（不再合并成一条 —— T21c-2-r1 / F2 按产品裁定恢复四列；
                // 标签取缩短形态 `ui_text::LAST_OK_SHORT` / `LAST_UPDATE_SHORT`）。
                let body_y = theme::center_offset(row.h, TextSlot::Body.px() as i32);
                Self::place(
                    &self.l1,
                    inner_x,
                    ST_NAME_W,
                    theme::center_offset(row.h, TextSlot::Label.px() as i32),
                    TextSlot::Label,
                    Palette::TEXT_PRIMARY,
                );
                self.l1.set_text(&row.label);
                set_visible(&self.l1, true);
                let state_x = inner_x + ST_NAME_W + Dimens::GAP_MIN;
                Self::place(
                    &self.l2,
                    state_x,
                    ST_STATE_W,
                    body_y,
                    TextSlot::Body,
                    station_color(&row.value),
                );
                self.l2.set_text(&row.value);
                set_visible(&self.l2, true);
                // 两个时刻列**右对齐**在行内容右缘（与行宽无关的口径 ⇒ 段「装置」的 5 行表
                // 与各段顶**逐字同款**；见 `station_col_spans` 的版面断言）。
                let upd_x = self.width - inner_x - ST_TIME_W;
                let ok_x = upd_x - Dimens::GAP_MIN - ST_TIME_W;
                Self::place(
                    &self.l3,
                    ok_x,
                    ST_TIME_W,
                    body_y,
                    TextSlot::Body,
                    Palette::TEXT_SECOND,
                );
                self.l3.set_text(&row.aux);
                set_visible(&self.l3, true);
                Self::place(
                    &self.l4,
                    upd_x,
                    ST_TIME_W,
                    body_y,
                    TextSlot::Body,
                    Palette::TEXT_SECOND,
                );
                self.l4.set_text(&row.aux2);
                set_visible(&self.l4, true);
            }
            RowKind::Bit => {
                let name_w = inner_w / 2;
                Self::place(
                    &self.l1,
                    inner_x,
                    name_w,
                    theme::center_offset(row.h, TextSlot::Body.px() as i32),
                    TextSlot::Body,
                    Palette::TEXT_SECOND,
                );
                self.l1.set_text(&row.label);
                set_visible(&self.l1, true);
                match (&self.led, row.active) {
                    (Some(led), Some(active)) => {
                        // `LedIndicator` 的**文字通道** = 状态词（偏差 P6-3 的字号）。
                        led.set_text(&row.value);
                        led.set_color(if active {
                            Palette::LINK_OK
                        } else {
                            Palette::LINK_UNCONFIGURED
                        });
                        led.set_lit(active);
                        led.obj().set_pos(
                            inner_x + name_w,
                            theme::center_offset(row.h, Dimens::ICON_SM),
                        );
                        set_visible(led.obj(), true);
                    }
                    _ => {
                        // 未定义位（`active = None`）：**无灯**，状态词放值槽（不臆造语义）。
                        Self::place(
                            &self.l2,
                            inner_x + name_w,
                            inner_w - name_w,
                            theme::center_offset(row.h, TextSlot::Body.px() as i32),
                            TextSlot::Body,
                            Palette::TEXT_WEAK,
                        );
                        self.l2.set_text(&row.value);
                        set_visible(&self.l2, true);
                    }
                }
            }
            RowKind::Scalar | RowKind::Enum => {
                Self::place(
                    &self.l1,
                    inner_x,
                    inner_w,
                    name_y,
                    TextSlot::SectionTitle,
                    Palette::TEXT_SECOND,
                );
                self.l1.set_text(&row.label);
                set_visible(&self.l1, true);
                let (font, color) = if row.degraded {
                    (TextSlot::Body, Palette::TEXT_WEAK)
                } else {
                    (TextSlot::CardValue, Palette::TEXT_PRIMARY)
                };
                // 值列相对**内容区**（不是相对行）对齐 —— 与 F8 三卡同一条竖线。
                let value_x = VALUE_COL_X;
                Self::place(
                    &self.l2,
                    value_x,
                    (self.width - value_x - Dimens::GAP_MIN).max(0),
                    theme::center_offset(row.h, font.px() as i32),
                    font,
                    color,
                );
                self.l2.set_text(&row.value);
                set_visible(&self.l2, true);
                if !row.aux.is_empty() {
                    let aux_x = value_x + TextSlot::CardValue.px() as i32 * 3;
                    Self::place(
                        &self.l3,
                        aux_x,
                        (self.width - aux_x - Dimens::GAP_MIN).max(0),
                        theme::center_offset(row.h, TextSlot::Body.px() as i32),
                        TextSlot::Body,
                        if row.degraded {
                            Palette::TEXT_WEAK
                        } else {
                            Palette::TEXT_SECOND
                        },
                    );
                    self.l3.set_text(&row.aux);
                    set_visible(&self.l3, true);
                }
            }
            RowKind::Notice => {
                // 段级文案（24 px `text_weak`；如「站点未启用」/「外设数据不可用」）。
                Self::place(
                    &self.l1,
                    0,
                    self.width,
                    theme::center_offset(row.h, TextSlot::Body.px() as i32),
                    TextSlot::Body,
                    Palette::TEXT_WEAK,
                );
                self.l1.set_text(&row.label);
                set_visible(&self.l1, true);
            }
            RowKind::Special => {}
        }
    }

    /// 本行的**四**槽文本（隐藏槽 ⇒ `None`；断言口径）。
    fn texts(
        &self,
    ) -> (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        let pick = |l: &Label| if l.is_hidden() { None } else { l.text() };
        (
            pick(&self.l1),
            pick(&self.l2),
            pick(&self.l3),
            pick(&self.l4),
        )
    }

    /// 本行**可见槽**的实测版面（`(x1, x2, 槽宽)`，`coords()` 为屏内**绝对**坐标）。
    ///
    /// F2 的版面断言读口：四列的互不重叠（相邻列的 `x2 < 下一列 x1`）与**不越槽**
    /// （`measured_text_px(文本, 字号) ≤ 槽宽`）都从这里量，**不看常量名**。
    fn col_spans(&self) -> Vec<(i32, i32, i32)> {
        [&self.l1, &self.l2, &self.l3, &self.l4]
            .iter()
            .filter(|l| !l.is_hidden())
            .map(|l| {
                let c = l.coords();
                (c.x1, c.x2, l.size().0)
            })
            .collect()
    }
}

/// 站状态词 → 语义色（「在线」绿 / 「站离线」红 / 其余中性）。
fn station_color(status: &str) -> Color {
    if status == ui_text::ONLINE {
        Palette::OK
    } else if status == ui_text::STATION_OFFLINE {
        Palette::DANGER
    } else {
        Palette::TEXT_WEAK
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 段列表（窗口化的行池 + 卡框池 + 特殊件）
// ═══════════════════════════════════════════════════════════════════════════

/// 一段的行列表（**窗口化**：行对象按滚动位置复用；设计 §15.5.3）。
pub(crate) struct SegmentList {
    /// 滚动视口（段内容，`CONTENT_W × SECTION_VIEW_H`）。
    viewport: ScrollContainer,
    /// 内容占位（高 = 模型总高；池行 / 卡框 / 特殊件都是它的子对象 ⇒ 随滚动移动）。
    spacer: Obj,
    /// 行池（**大小固定**，与行数无关）。
    rows: Vec<PoolRow>,
    /// 卡框池（同样固定大小）。
    cards: Vec<Obj>,
    /// 当前模型。
    model: RefCell<SegmentModel>,
    /// 特殊件（页面自建；本列表只按模型的 `Special` 行摆它）。
    special: RefCell<Option<Obj>>,
    /// 最近一次绑定的窗口起点。
    window_start: Cell<Option<usize>>,
}

impl SegmentList {
    /// 在 `parent`（段内容容器）里建列表；`with_led` = 该段是否含位行。
    pub(crate) fn new(parent: &Obj, with_led: bool) -> Result<Self, LvglError> {
        let viewport = ScrollContainer::create(parent)?;
        viewport.set_size(Dimens::CONTENT_W, Dimens::SECTION_VIEW_H);
        viewport.set_pos(0, 0);
        viewport.add_style(&theme::transparent(), StyleSelector::main());

        let spacer = layout_box(&viewport, Dimens::CONTENT_W, Dimens::SECTION_VIEW_H)?;
        spacer.set_pos(0, 0);

        // **卡框先建、池行后建** ⇒ LVGL 按创建序绘制 ⇒ 池行绘在卡框之上（偏差 P6-1）。
        let mut cards = Vec::with_capacity(SEG_CARD_POOL);
        for _ in 0..SEG_CARD_POOL {
            let card = decor(
                &spacer,
                Dimens::CONTENT_W,
                Dimens::CARD_HEAD_H,
                &theme::card(),
            )?;
            set_visible(&card, false);
            cards.push(card);
        }
        let mut rows = Vec::with_capacity(SEG_ROW_POOL);
        for _ in 0..SEG_ROW_POOL {
            let r = PoolRow::new(&spacer, Dimens::CONTENT_W, with_led)?;
            set_visible(&r.obj, false);
            rows.push(r);
        }
        Ok(Self {
            viewport,
            spacer,
            rows,
            cards,
            model: RefCell::new(SegmentModel::default()),
            special: RefCell::new(None),
            window_start: Cell::new(None),
        })
    }

    /// 视口对象（摆放 / 可见性 / 断言用）。
    pub(crate) fn obj(&self) -> &Obj {
        &self.viewport
    }

    /// 行池大小（**"行数上界"断言的读口**：与模型行数无关）。
    pub(crate) fn pool_size(&self) -> usize {
        self.rows.len()
    }

    /// 让列表接管一个**特殊件**（父改到内容占位上 ⇒ 随滚动移动、被视口裁剪）。
    pub(crate) fn adopt_special(&self, obj: &Obj) {
        obj.set_parent(&self.spacer);
        *self.special.borrow_mut() = Some(obj.share_borrowed());
    }

    /// 设模型并**立即按当前滚动位置重绑**。
    pub(crate) fn set_model(&self, model: SegmentModel) {
        self.spacer
            .set_size(Dimens::CONTENT_W, model.total_h.max(Dimens::SECTION_VIEW_H));
        if let Some(sp) = self.special.borrow().as_ref() {
            let placed = model
                .rows
                .iter()
                .zip(model.ys.iter())
                .find(|(r, _)| r.kind == RowKind::Special)
                .map(|(r, y)| (*y, r.h));
            match placed {
                Some((y, h)) => {
                    sp.set_size(Dimens::CONTENT_W - 2 * CARD_INSET, h);
                    sp.set_pos(CARD_INSET, y);
                    set_visible(sp, true);
                }
                None => set_visible(sp, false),
            }
        }
        *self.model.borrow_mut() = model;
        self.rebind();
    }

    /// 按滚动位置重绑窗口内的池行 / 卡框。
    fn rebind(&self) {
        let model = self.model.borrow();
        if model.rows.is_empty() {
            for r in &self.rows {
                set_visible(&r.obj, false);
            }
            for c in &self.cards {
                set_visible(c, false);
            }
            self.window_start.set(None);
            return;
        }
        let top = self.viewport.scroll_y().max(0);
        let bottom = top + Dimens::SECTION_VIEW_H;
        // 窗口起点 = 第一个"底边越过视口顶"的行。
        let start = model
            .ys
            .iter()
            .enumerate()
            .position(|(i, y)| y + model.rows[i].h > top)
            .unwrap_or(model.rows.len().saturating_sub(1));
        self.window_start.set(Some(start));
        for (k, row_widget) in self.rows.iter().enumerate() {
            match (model.rows.get(start + k), model.ys.get(start + k)) {
                (Some(row), Some(y)) => {
                    row_widget.bind(row, *y);
                    set_visible(&row_widget.obj, true);
                }
                _ => set_visible(&row_widget.obj, false),
            }
        }
        // 卡框：取"与视口相交"的前 `SEG_CARD_POOL` 张。
        let mut slot = 0usize;
        for (cy, ch) in &model.cards {
            if *cy + *ch <= top || *cy >= bottom {
                continue;
            }
            if slot >= self.cards.len() {
                break;
            }
            let card = &self.cards[slot];
            card.set_pos(0, *cy);
            card.set_size(Dimens::CONTENT_W, *ch);
            set_visible(card, true);
            slot += 1;
        }
        for card in self.cards.iter().skip(slot) {
            set_visible(card, false);
        }
    }

    /// 滚动事件入口（生产路径：段内容的 `SCROLL` ⇒ 重绑窗口）。
    pub(crate) fn on_scroll(&self) {
        self.rebind();
    }

    /// 注册"滚动 ⇒ 重绑"（**唯一**的注册点；`Weak` 避免与事件项构成引用环）。
    pub(crate) fn wire_scroll(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.viewport.on(EventCode::SCROLL, move |_| {
            if let Some(l) = weak.upgrade() {
                l.on_scroll();
            }
        });
    }

    /// 当前窗口起点（**仅测试**：断言窗口随滚动移动）。
    #[cfg(test)]
    pub(crate) fn window_start(&self) -> Option<usize> {
        self.window_start.get()
    }

    /// 当前**可见**池行数。
    pub(crate) fn visible_count(&self) -> usize {
        self.rows.iter().filter(|r| !r.obj.is_hidden()).count()
    }

    /// 按**数据行下标**读四槽文本（`None` = 不在当前窗口内）。
    pub(crate) fn text_at(&self, index: usize) -> Option<(String, String, String, String)> {
        let start = self.window_start.get()?;
        let k = index.checked_sub(start)?;
        let r = self.rows.get(k)?;
        let (a, b, c, d) = r.texts();
        Some((
            a.unwrap_or_default(),
            b.unwrap_or_default(),
            c.unwrap_or_default(),
            d.unwrap_or_default(),
        ))
    }

    /// 按**数据行下标**读该行**可见槽**的实测版面（`(x1, x2, 槽宽)` 列表）。
    pub(crate) fn col_spans_at(&self, index: usize) -> Option<Vec<(i32, i32, i32)>> {
        let start = self.window_start.get()?;
        let k = index.checked_sub(start)?;
        self.rows.get(k).map(|r| r.col_spans())
    }

    /// 行模型（`(kind, 行高)` 列表 —— **降级与在线时的行数、行高必须相同**）。
    pub(crate) fn model_rows(&self) -> Vec<(RowKind, i32)> {
        self.model
            .borrow()
            .rows
            .iter()
            .map(|r| (r.kind, r.h))
            .collect()
    }

    /// 模型行数。
    pub(crate) fn row_count(&self) -> usize {
        self.model.borrow().rows.len()
    }

    /// 模型总高。
    pub(crate) fn total_h(&self) -> i32 {
        self.model.borrow().total_h
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. BMS 告警摘要卡（段「电池」；288 位的**入口**）
// ═══════════════════════════════════════════════════════════════════════════

/// 摘要卡（卡头 + 摘要行 + 活跃位清单（最多 4 行）+ 入口按钮）。
///
/// **三态文案**（EDGE-24 / EX-16，**两两互异**；由组帧侧的 `available` 与站级态给出，
/// **屏侧不重判**）：「无活跃告警位」（`available ∧ active_total == 0`）/
/// 「BMS 告警源不可用」（`!available`）/「站离线」（站级优先）。
pub(crate) struct BmsAlarmCard {
    card: Obj,
    title: Rc<Label>,
    summary: Rc<Label>,
    list: Vec<Rc<Label>>,
    entry: Rc<TextButton>,
}

impl BmsAlarmCard {
    /// 建摘要卡（`open_req` 与页面共享）。
    fn new(parent: &Obj, open_req: Rc<Cell<bool>>) -> Result<Self, LvglError> {
        let card = decor(
            parent,
            Dimens::CONTENT_W - 2 * CARD_INSET,
            Self::height(0),
            &theme::card(),
        )?;
        let title = Rc::new(text_label(
            &card,
            "",
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?);
        title.set_pos(0, 0);
        let summary = Rc::new(text_label(
            &card,
            "",
            TextSlot::CardValue,
            Palette::TEXT_PRIMARY,
        )?);
        summary.set_pos(0, Dimens::CARD_HEAD_H);
        let mut list = Vec::with_capacity(BMS_SUMMARY_LIST_ROWS);
        for i in 0..BMS_SUMMARY_LIST_ROWS {
            let l = Rc::new(label(&card, TextSlot::Body, Palette::TEXT_SECOND)?);
            l.set_long_mode(LongMode::DOTS);
            l.set_pos(
                0,
                Dimens::CARD_HEAD_H + Dimens::ROW_SUMMARY_H + i as i32 * Dimens::ROW_SUMMARY_H,
            );
            set_visible(&l, false);
            list.push(l);
        }
        let entry = Rc::new(TextButton::create(&card, ui_text::VIEW_ALL)?);
        entry.set_size(Dimens::DRILL_ENTRY_W, Dimens::TOUCH_MIN);
        theme::button(theme::ButtonKind::Secondary).apply(&entry);
        entry.set_pos(
            0,
            Dimens::CARD_HEAD_H
                + Dimens::ROW_SUMMARY_H
                + BMS_SUMMARY_LIST_ROWS as i32 * Dimens::ROW_SUMMARY_H,
        );
        {
            let flag = Rc::clone(&open_req);
            entry.on_clicked(move |_| flag.set(true));
        }
        let card = Self {
            card,
            title,
            summary,
            list,
            entry,
        };
        card.place_entry(0);
        Ok(card)
    }

    /// 卡高（`清单行数` ∈ 0..=4；**0 行时 = `CARD_HEAD_H + ROW_SUMMARY_H + ROW_ENTRY_H + 2×INSET`
    /// = 140 + 2×17**，与 UI §6.6.1 的算式同构：卡头 40 + 摘要行 44 + 入口行 56 = 140）。
    fn height(list_rows: usize) -> i32 {
        let n = list_rows.min(BMS_SUMMARY_LIST_ROWS) as i32;
        Dimens::CARD_HEAD_H
            + Dimens::ROW_SUMMARY_H
            + n * Dimens::ROW_SUMMARY_H
            + Dimens::ROW_ENTRY_H
            + 2 * CARD_INSET
    }

    /// 「查看全部 288 位」按钮的文案（**取 `VIEW_ALL` + 总数 + 量词**；UI §6.6.1 原文
    /// 「查看全部 288 位」）。
    fn entry_text(total: u32) -> String {
        format!(
            "{}{}{}{}{}",
            ui_text::VIEW_ALL,
            TEXT_SPACE,
            total,
            TEXT_SPACE,
            ui_text::BITS_UNIT
        )
    }

    /// 按清单行数摆入口按钮。
    fn place_entry(&self, list_rows: usize) {
        self.entry.set_pos(
            0,
            Dimens::CARD_HEAD_H
                + Dimens::ROW_SUMMARY_H
                + list_rows.min(BMS_SUMMARY_LIST_ROWS) as i32 * Dimens::ROW_SUMMARY_H,
        );
    }

    /// 卡片本体。
    fn obj(&self) -> &Obj {
        &self.card
    }

    /// 渲染（`label_of` = 位 → 短标签，由 catalog 给出；`None` ⇒ 「名称未获取」）。
    fn render(
        &self,
        page: Option<&BmsAlarmPage>,
        station: StationState,
        label_of: impl Fn(u16) -> Option<String>,
    ) {
        self.title
            .set_text(group_title("bms_alarm").unwrap_or(ui_text::ENUM_UNKNOWN));
        let (text, rows): (String, Vec<String>) = if station == StationState::Offline {
            (ui_text::STATION_OFFLINE.to_string(), Vec::new())
        } else if station == StationState::Disabled {
            (ui_text::SECTION_STATION_DISABLED.to_string(), Vec::new())
        } else {
            match page {
                Some(p) if !p.available => (
                    ui_text::BMS_ALARM_SOURCE_UNAVAILABLE.to_string(),
                    Vec::new(),
                ),
                Some(p) => {
                    if p.active_total == 0 {
                        (ui_text::NO_ACTIVE_ALARM_BIT.to_string(), Vec::new())
                    } else {
                        (
                            format!(
                                "{}{}{}{}{}",
                                ui_text::BMS_ACTIVE_BITS_PREFIX,
                                TEXT_SPACE,
                                p.active_total,
                                TEXT_SPACE,
                                ui_text::COUNT_SUFFIX
                            ),
                            p.items
                                .iter()
                                .filter(|i| i.active)
                                .take(BMS_SUMMARY_LIST_ROWS)
                                .map(|i| {
                                    let name = label_of(i.at)
                                        .unwrap_or_else(|| ui_text::NAME_UNKNOWN.to_string());
                                    format!("{name}{}{}", TEXT_SPACE, i.at)
                                })
                                .collect(),
                        )
                    }
                }
                None => (
                    ui_text::BMS_ALARM_SOURCE_UNAVAILABLE.to_string(),
                    Vec::new(),
                ),
            }
        };
        self.summary.set_text(&text);
        for (i, l) in self.list.iter().enumerate() {
            match rows.get(i) {
                Some(t) => {
                    l.set_text(t);
                    set_visible(l, true);
                }
                None => set_visible(l, false),
            }
        }
        self.card
            .set_size(Dimens::CONTENT_W - 2 * CARD_INSET, Self::height(rows.len()));
        self.place_entry(rows.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. BMS 288 位下钻视图（§15.5.2 / UI §6.6.1；**不构成页面**）
// ═══════════════════════════════════════════════════════════════════════════

/// 下钻的一行 = 容器 + **两位**（每位："位号 + 短标签"一条标签 + `LedIndicator`）。
struct DrillRow {
    obj: Obj,
    names: [Rc<Label>; DRILL_ITEMS_PER_ROW],
    leds: [LedIndicator; DRILL_ITEMS_PER_ROW],
}

/// 288 位下钻视图（顶部条 + 窗口化行池）。
pub(crate) struct BmsDrill {
    root: Obj,
    title: Rc<Label>,
    prev: Rc<TextButton>,
    next: Rc<TextButton>,
    collapse: Rc<TextButton>,
    fail: Rc<Label>,
    retry: Rc<TextButton>,
    viewport: ScrollContainer,
    spacer: Obj,
    rows: Vec<DrillRow>,
    open: Cell<bool>,
    /// 最近一次绑定的**窗口起点**（0 基行号；`None` = 无页数据 / 失败态 ⇒ 无窗口）。
    ///
    /// T21c-2-r1 / F1：它是"窗口真的随滚动前移"的断言读口；与 [`SegmentList::window_start`]
    /// 同款（同一个范式）。
    window_start: Cell<Option<usize>>,
    /// 最近一次注入的页（`None` = 未取到 / 失败）。
    page: RefCell<Option<BmsAlarmPage>>,
    /// 失败态（端点非 2xx / 传输失败）。
    failed: Cell<bool>,
    /// 位 → 短标签（catalog；未取到 ⇒ 行内显「名称未获取」）。
    labels: RefCell<Vec<(u16, String)>>,
    /// 意图（页面轮询后交给接线层 / 自行收起 —— 页面**不自行发请求**）。
    page_req: Rc<Cell<Option<u32>>>,
    collapse_req: Rc<Cell<bool>>,
}

/// 下钻翻页意图的**哨兵值**（页码 ∈ 1..=N ⇒ 这两个值不冲突）。
const DRILL_REQ_NEXT: u32 = u32::MAX;
/// 「上一页」哨兵。
const DRILL_REQ_PREV: u32 = u32::MAX - 1;
/// 「重试当前页」哨兵。
const DRILL_REQ_RETRY: u32 = 0;

impl BmsDrill {
    /// 建下钻视图（`parent` = 段「电池」的内容容器；视图占满整个段内容区）。
    fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = layout_box(parent, Dimens::CONTENT_W, Dimens::SECTION_VIEW_H)?;
        root.set_pos(0, 0);
        let title = Rc::new(text_label(
            &root,
            "",
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?);
        title.set_size(
            Dimens::CONTENT_W - 3 * Dimens::DRILL_BTN_W - 4 * Dimens::GAP_MIN,
            TextSlot::SectionTitle.px() as i32,
        );
        title.set_long_mode(LongMode::DOTS);
        title.set_pos(
            0,
            theme::center_offset(DRILL_TOP_H, TextSlot::SectionTitle.px() as i32),
        );
        // 顶部条右端：上一页 / 下一页 / **收起**（UI §6.6.1：120×48、间距 16）。
        let collapse = Rc::new(TextButton::create(&root, ui_text::COLLAPSE)?);
        collapse.set_size(Dimens::DRILL_BTN_W, Dimens::TOUCH_MIN);
        collapse.set_pos(Dimens::CONTENT_W - Dimens::DRILL_BTN_W, 0);
        theme::button(theme::ButtonKind::Secondary).apply(&collapse);
        let next = Rc::new(TextButton::create(&root, ui_text::NEXT_PAGE)?);
        next.set_size(Dimens::DRILL_BTN_W, Dimens::TOUCH_MIN);
        next.set_pos(
            Dimens::CONTENT_W - 2 * Dimens::DRILL_BTN_W - Dimens::GAP_MIN,
            0,
        );
        theme::button(theme::ButtonKind::Secondary).apply(&next);
        let prev = Rc::new(TextButton::create(&root, ui_text::PREV_PAGE)?);
        prev.set_size(Dimens::DRILL_BTN_W, Dimens::TOUCH_MIN);
        prev.set_pos(
            Dimens::CONTENT_W - 3 * Dimens::DRILL_BTN_W - 2 * Dimens::GAP_MIN,
            0,
        );
        theme::button(theme::ButtonKind::Secondary).apply(&prev);

        let viewport = ScrollContainer::create(&root)?;
        viewport.set_size(Dimens::CONTENT_W, DRILL_BODY_H);
        viewport.set_pos(0, DRILL_BODY_Y);
        viewport.add_style(&theme::transparent(), StyleSelector::main());
        let spacer = layout_box(&viewport, Dimens::CONTENT_W, DRILL_BODY_H)?;
        spacer.set_pos(0, 0);
        let mut rows = Vec::with_capacity(DRILL_ROW_POOL);
        for _ in 0..DRILL_ROW_POOL {
            rows.push(Self::build_row(&spacer)?);
        }
        let fail = Rc::new(text_label(
            &root,
            ui_text::DETAIL_UNAVAILABLE,
            TextSlot::SectionTitle,
            Palette::DANGER,
        )?);
        fail.set_pos(0, DRILL_BODY_Y);
        set_visible(&fail, false);
        let retry = Rc::new(TextButton::create(&root, ui_text::RETRY)?);
        retry.set_size(Dimens::DRILL_BTN_W, Dimens::TOUCH_MIN);
        retry.set_pos(
            0,
            DRILL_BODY_Y + TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN,
        );
        theme::button(theme::ButtonKind::Secondary).apply(&retry);
        set_visible(&retry, false);

        // 意图接线：翻页 / 重试 / 收起（**只登记意图**，请求由接线层发）。
        let page_req: Rc<Cell<Option<u32>>> = Rc::new(Cell::new(None));
        let collapse_req: Rc<Cell<bool>> = Rc::new(Cell::new(false));
        {
            let flag = Rc::clone(&collapse_req);
            collapse.on_clicked(move |_| flag.set(true));
        }
        let drill = Self {
            root,
            title,
            prev,
            next,
            collapse,
            fail,
            retry,
            viewport,
            spacer,
            rows,
            open: Cell::new(false),
            window_start: Cell::new(None),
            page: RefCell::new(None),
            failed: Cell::new(false),
            labels: RefCell::new(Vec::new()),
            page_req,
            collapse_req,
        };
        drill.wire_page_buttons();
        set_visible(&drill.root, false);
        Ok(drill)
    }

    /// 翻页 / 重试的意图接线（与 [`BmsDrill::page`] 的当前页一起算目标页）。
    fn wire_page_buttons(&self) {
        for (btn, req) in [(&self.prev, DRILL_REQ_PREV), (&self.next, DRILL_REQ_NEXT)] {
            let flag = Rc::clone(&self.page_req);
            btn.on_clicked(move |_| flag.set(Some(req)));
        }
        // 重试 = 重发**当前页**（哨兵由页面换成当前页）。
        let flag = Rc::clone(&self.page_req);
        self.retry
            .on_clicked(move |_| flag.set(Some(DRILL_REQ_RETRY)));
    }

    /// 建一行（两位）。
    fn build_row(parent: &Obj) -> Result<DrillRow, LvglError> {
        let obj = layout_box(parent, Dimens::CONTENT_W, Dimens::DRILL_ROW_H)?;
        let half = Dimens::CONTENT_W / 2;
        let mut names = Vec::with_capacity(DRILL_ITEMS_PER_ROW);
        let mut leds = Vec::with_capacity(DRILL_ITEMS_PER_ROW);
        for i in 0..DRILL_ITEMS_PER_ROW {
            let x = i as i32 * half;
            let l = Rc::new(label(&obj, TextSlot::Label, Palette::TEXT_SECOND)?);
            l.set_size(DRILL_NAME_W, TextSlot::Label.px() as i32);
            l.set_long_mode(LongMode::DOTS);
            l.set_pos(
                x,
                theme::center_offset(Dimens::DRILL_ROW_H, TextSlot::Label.px() as i32),
            );
            names.push(l);
            let led = LedIndicator::new(
                &obj,
                Dimens::ICON_SM,
                "●",
                ui_text::BIT_INACTIVE,
                Palette::LINK_UNCONFIGURED,
            )?;
            led.obj().set_pos(
                x + DRILL_NAME_W + Dimens::GAP_MIN,
                theme::center_offset(Dimens::DRILL_ROW_H, Dimens::ICON_SM),
            );
            leds.push(led);
        }
        Ok(DrillRow {
            obj,
            names: names
                .try_into()
                .map_err(|_| LvglError::InvalidArgument("BmsDrill: 两位一行"))?,
            leds: leds
                .try_into()
                .map_err(|_| LvglError::InvalidArgument("BmsDrill: 两位一行"))?,
        })
    }

    /// **滚动事件入口**（生产路径：下钻行区的 `SCROLL` ⇒ 按当前位置重绑窗口）。
    ///
    /// T21c-2-r1 / F1：与 [`SegmentList::on_scroll`] 同一范式 —— 下钻的窗口**不只在注入拍**
    /// 重绑，而是随滚动位置重绑 ⇒ 一页 25 行（`page_size 50` / 两位一行）时**池外行**
    /// （`DRILL_ROW_POOL` = 16）滚到哪儿都被绑上，不再静默空白。
    pub(crate) fn on_scroll(&self) {
        self.render();
    }

    /// 注册"滚动 ⇒ 重绑"（**下钻的唯一注册点**；`Weak` 避免与事件项构成引用环）。
    pub(crate) fn wire_scroll(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.viewport.on(EventCode::SCROLL, move |_| {
            if let Some(d) = weak.upgrade() {
                d.on_scroll();
            }
        });
    }

    /// 当前窗口起点（**仅测试**：断言窗口随滚动移动）。
    #[cfg(test)]
    pub(crate) fn window_start(&self) -> Option<usize> {
        self.window_start.get()
    }

    /// 池行 `k` 的**两位文本**（`None` = 该位不在页内 / 行不在池内；内层 `None` = 该半区隐藏）。
    ///
    /// **仅测试**：F1 的判据读口（"任意滚动位置都能看到该位置应有的行"）。
    #[cfg(test)]
    pub(crate) fn row_names(&self, k: usize) -> Option<(Option<String>, Option<String>)> {
        let row = self.rows.get(k)?;
        let pick = |i: usize| {
            let l = &row.names[i];
            if l.is_hidden() {
                None
            } else {
                l.text()
            }
        };
        Some((pick(0), pick(1)))
    }

    /// 行池大小（"行数上界"断言的读口）。
    pub(crate) fn pool_size(&self) -> usize {
        self.rows.len()
    }

    /// 视图是否打开。
    pub(crate) fn is_open(&self) -> bool {
        self.open.get() && !self.root.is_hidden()
    }

    /// 打开 / 收起（**不改 `current_page`**：本视图不是页面）。
    pub(crate) fn set_open(&self, open: bool) {
        self.open.set(open);
        set_visible(&self.root, open);
        if !open {
            self.viewport.scroll_to_y(0);
        }
    }

    /// **意图读口**：翻页（`Some(n)` = 目标页，1 起；`Some(0)` = 重试当前页）。
    pub(crate) fn take_page_request(&self) -> Option<u32> {
        let v = self.page_req.get();
        if v.is_some() {
            self.page_req.set(None);
        }
        v
    }

    /// **意图读口**：收起。
    pub(crate) fn take_collapse_request(&self) -> bool {
        let v = self.collapse_req.get();
        if v {
            self.collapse_req.set(false);
        }
        v
    }

    /// **段顶提示条的让位**（T21c-3-r1）：`inset` = `0` / `STALE_BAND_H` —— 本视图整体下移
    /// 并等量变矮（顶部条「收起 / 上一页 / 下一页」与失败带随之整体平移；**行区视口同减**
    /// ⇒ 其滚动范围照旧覆盖全部行，池外行仍可滚到）。
    ///
    /// **为什么整视图平移 + 两次 `set_size`**：视图占满整个段内容区（**P6-7**），
    /// 只移行区会让顶部条压在提示条上（且「收起」与提示条右侧的「重试」会**重叠** ⇒
    /// 触碰命中区歧义）；只移视图不缩行区则行区底缘越出段面板、进而把页根撑出滚动条。
    /// `inset = 0` 即构造期几何（本方法**只改几何、不建不删对象**）。
    pub(crate) fn set_inset(&self, inset: i32) {
        let inset = inset.max(0);
        self.root.set_pos(0, inset);
        self.root
            .set_size(Dimens::CONTENT_W, Dimens::SECTION_VIEW_H - inset);
        self.viewport
            .set_size(Dimens::CONTENT_W, DRILL_BODY_H - inset);
    }

    /// 收起键对象（版面 / 触摸断言用）。
    pub(crate) fn collapse_button(&self) -> &Obj {
        &self.collapse
    }

    /// 根容器矩形（提示条让位断言用）。
    pub(crate) fn root_coords(&self) -> Area {
        self.root.coords()
    }

    /// 上一页 / 下一页按钮。
    pub(crate) fn page_buttons(&self) -> (&Obj, &Obj) {
        (&self.prev, &self.next)
    }

    /// 顶部条标题文字（断言口径）。
    pub(crate) fn title_text(&self) -> Option<String> {
        self.title.text()
    }

    /// 失败带是否可见（R-4：**只显本地固定文案**）。
    pub(crate) fn fail_visible(&self) -> bool {
        !self.fail.is_hidden()
    }

    /// 重试按钮是否可见。
    pub(crate) fn retry_visible(&self) -> bool {
        !self.retry.is_hidden()
    }

    /// 当前页数据。
    pub(crate) fn page(&self) -> Option<BmsAlarmPage> {
        self.page.borrow().clone()
    }

    /// 注入位 → 短标签（catalog）。
    pub(crate) fn set_labels(&self, labels: Vec<(u16, String)>) {
        *self.labels.borrow_mut() = labels;
    }

    /// 注入一页数据（`failed` ⇒ 端点失败：**只显本地固定文案**「明细不可用」+「重试」，
    /// 服务端原因串**只进日志**——R-4）。
    pub(crate) fn set_page(&self, page: Option<BmsAlarmPage>, failed: bool) {
        self.failed.set(failed);
        *self.page.borrow_mut() = page;
        self.render();
    }

    /// 重绘（顶部条标题 + 失败带 + 行池窗口）。
    pub(crate) fn render(&self) {
        let page = self.page.borrow().clone();
        let failed = self.failed.get();
        set_visible(&self.fail, failed);
        set_visible(&self.retry, failed);
        // 顶部条标题**恒设**（未取到 / 失败 ⇒ 全 0；UI §6.6.1 原文的字段序不变）。
        let cur = page.as_ref().map(|p| p.page).unwrap_or(0);
        let total = page.as_ref().map(|p| p.total).unwrap_or(0);
        let active = page.as_ref().map(|p| p.active_total).unwrap_or(0);
        // 页数口径：**以服务端给的 `page_size` 为准**（缺省 0 时才回退到 UI §6.6.1 的 50）
        // —— 否则接线层改了每页位数后，`第 X / Y 页` 的 `Y` 会与真实分页不符。
        let page_size = page
            .as_ref()
            .map(|p| p.page_size)
            .filter(|n| *n > 0)
            .unwrap_or(DRILL_PAGE_SIZE);
        let pages = total.div_ceil(page_size);
        self.title.set_text(&format!(
            "{t}{s}{p} {cur}{ps}{pages} {suf}{s}{a} {act}{ps}{total}",
            t = ui_text::BMS_ALARM_BITS_TITLE,
            s = TEXT_SEP,
            p = ui_text::PAGE_PREFIX,
            cur = cur,
            ps = TEXT_PAGE_SEP,
            pages = pages,
            suf = ui_text::PAGE_SUFFIX,
            a = ui_text::BIT_ACTIVE,
            act = active,
            total = total,
        ));
        let Some(p) = page.filter(|_| !failed) else {
            for r in &self.rows {
                set_visible(&r.obj, false);
            }
            self.window_start.set(None);
            return;
        };
        let row_count = p.items.len().div_ceil(DRILL_ITEMS_PER_ROW);
        let total_h = (row_count as i32 * Dimens::DRILL_ROW_H).max(DRILL_BODY_H);
        self.spacer.set_size(Dimens::CONTENT_W, total_h);
        let start = (self.viewport.scroll_y().max(0) / Dimens::DRILL_ROW_H) as usize;
        self.window_start.set(Some(start));
        for (k, row) in self.rows.iter().enumerate() {
            let idx = start + k;
            if idx >= row_count {
                set_visible(&row.obj, false);
                continue;
            }
            row.obj.set_pos(0, idx as i32 * Dimens::DRILL_ROW_H);
            for j in 0..DRILL_ITEMS_PER_ROW {
                match p.items.get(idx * DRILL_ITEMS_PER_ROW + j) {
                    Some(it) => {
                        let name = self
                            .labels
                            .borrow()
                            .iter()
                            .find(|(at, _)| *at == it.at)
                            .map(|(_, l)| l.clone())
                            .unwrap_or_else(|| ui_text::NAME_UNKNOWN.to_string());
                        row.names[j].set_text(&format!("{name}{}{}", TEXT_SPACE, it.at));
                        row.names[j].set_text_color(if it.active {
                            Palette::TEXT_PRIMARY
                        } else {
                            Palette::TEXT_SECOND
                        });
                        let led = &row.leds[j];
                        led.set_lit(it.active);
                        led.set_color(if it.active {
                            Palette::LINK_OK
                        } else {
                            Palette::LINK_UNCONFIGURED
                        });
                        led.set_text(if it.active {
                            ui_text::BIT_ACTIVE
                        } else {
                            ui_text::BIT_INACTIVE
                        });
                        set_visible(&row.names[j], true);
                        set_visible(led.obj(), true);
                    }
                    None => {
                        set_visible(&row.names[j], false);
                        set_visible(row.leds[j].obj(), false);
                    }
                }
            }
            set_visible(&row.obj, true);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. 页面
// ═══════════════════════════════════════════════════════════════════════════

/// 一个「字段名 | 值」行（F8 既有三卡用；连接类另带 5 个 `LedIndicator`）。
struct SysRow {
    name: Label,
    value: Rc<Label>,
    value_style: Cell<usize>,
    leds: Option<Vec<Rc<LedIndicator>>>,
}

impl SysRow {
    /// 在 `parent` 的内容区里建一行（`y` 为行顶）。
    fn new(
        parent: &Obj,
        y: i32,
        name: &str,
        styles: &[Rc<Style>; 2],
        link: bool,
    ) -> Result<Self, LvglError> {
        let name_l = text_label(parent, name, TextSlot::Label, Palette::TEXT_SECOND)?;
        name_l.set_pos(
            0,
            y + theme::center_offset(ROW_H, TextSlot::Label.px() as i32),
        );
        let value = Rc::new(label(parent, TextSlot::CardValue, Palette::TEXT_PRIMARY)?);
        value.set_text(PLACEHOLDER);
        value.set_size(VALUE_W, TextSlot::CardValue.px() as i32);
        value.set_long_mode(LongMode::DOTS);
        value.set_pos(
            VALUE_COL_X,
            y + theme::center_offset(ROW_H, TextSlot::CardValue.px() as i32),
        );
        let leds = if link {
            let mut arr: Vec<Rc<LedIndicator>> = Vec::with_capacity(LED_STATES.len());
            for st in LED_STATES {
                let led = Rc::new(LedIndicator::new(
                    parent,
                    LED_W,
                    link_icon(st),
                    st.display_name(),
                    link_color(st),
                )?);
                led.obj().set_pos(
                    VALUE_COL_X,
                    y + theme::center_offset(ROW_H, Dimens::ICON_SM),
                );
                set_visible(led.obj(), false);
                arr.push(led);
            }
            set_visible(value.obj(), false);
            Some(arr)
        } else {
            None
        };
        let row = Self {
            name: name_l,
            value,
            value_style: Cell::new(usize::MAX),
            leds,
        };
        set_style_index(row.value.obj(), styles, &row.value_style, 0);
        Ok(row)
    }

    /// 设文本值（`None` ⇒ 「未提供」+ 弱注样式）。
    fn set_text(&self, text: Option<&str>, styles: &[Rc<Style>; 2]) {
        match text {
            Some(t) => {
                self.value.set_text(t);
                set_style_index(self.value.obj(), styles, &self.value_style, 0);
                set_visible(self.value.obj(), true);
            }
            None => {
                self.value.set_text(MISSING);
                set_style_index(self.value.obj(), styles, &self.value_style, 1);
                set_visible(self.value.obj(), true);
            }
        }
    }

    /// 设链路态（连接类行；非连接行 no-op）。
    fn set_link(&self, state: LinkState) {
        let Some(leds) = &self.leds else { return };
        let idx = LED_STATES.iter().position(|s| *s == state).unwrap_or(4);
        let visible: Vec<&Obj> = leds.iter().map(|l| l.obj()).collect();
        crate::ui::pages::show_only(&visible, Some(idx));
    }

    /// 行名（"分列"断言口径）。
    fn name_text(&self) -> Option<String> {
        self.name.text()
    }

    /// 当前值文字（连接类为可见指示灯的文字）。
    fn value_text(&self) -> Option<String> {
        match &self.leds {
            Some(leds) => leds
                .iter()
                .find(|l| !l.obj().is_hidden())
                .and_then(|l| l.text()),
            None => self.value.text(),
        }
    }
}

/// 段「装置」的固定件（站状态表 + F8 三卡）—— 与页面**同生命周期**的句柄集。
struct DeviceSegment {
    /// 段内容的滚动容器（段「装置」的整段纵向滚动）。
    host: ScrollContainer,
    /// 固定件的**存活锚点**（卡 / 卡头标签的句柄 drop 即删除底层对象 ⇒ 必须存住）。
    /// 下划线前缀 = "有意不读"（它不是数据，是生命周期锚）。
    _keep: Vec<Obj>,
    station_rows: Vec<PoolRow>,
    info: Vec<SysRow>,
    run: Vec<SysRow>,
    about: Vec<SysRow>,
    note: Label,
    info_frozen: StatusChip,
    run_frozen: StatusChip,
}

/// P6「装置与外设」页（F8 + F20 / F22 / F23 / F24；**只读**）。
pub struct P6SystemPage {
    root: ScrollContainer,
    /// 五段分段页签（段内容由它惰性创建 + 只切可见性）。
    tabs: Rc<SegmentedTabs>,
    /// 段「装置」的固定件。
    device: DeviceSegment,
    /// 外设段（`None` = 尚未进入过 ⇒ 未创建）。
    segs: RefCell<Vec<Option<Rc<SegmentList>>>>,
    /// 段「电池」的告警摘要卡。
    bms_card: RefCell<Option<Rc<BmsAlarmCard>>>,
    /// 下钻视图（首次打开才创建）。
    drill: RefCell<Option<Rc<BmsDrill>>>,
    /// 「查看全部 288 位」的打开意图（由卡片写入、页面轮询）。
    drill_req: Rc<Cell<bool>>,
    /// 外设数据入口（契约 2：页面不自行发请求）。
    periph: RefCell<PeripheralsSection>,
    catalog: RefCell<Option<PeripheralCatalog>>,
    bms_page: RefCell<Option<BmsAlarmPage>>,
    bms_failed: Cell<bool>,
    /// 下钻「翻页」意图的出口（交回接线层发请求）。
    on_bms_page: CbSlot<u32>,
    // ── ⓪″ 段顶「名称表可能过期」提示条（U-73 / 设计 §15.3.1 第 2 / 3 句；T21c-3-r1）──
    /// 提示条本体（[`WarnBanner`]：全宽 − 按钮槽 × 56、**非交互**；UI §5.1 #12）。
    stale_banner: WarnBanner,
    /// 「重试」按钮（`TOUCH_MIN`(48)×`TOUCH_MIN`(48)；**只投意图**）。
    stale_retry: Rc<TextButton>,
    /// catalog 处于「重取失败 ⇒ 名称表可能过期」态（`false` = 提示条不占位）。
    catalog_stale: Cell<bool>,
    /// 「重试 catalog」意图槽（`Rc` 共享给按钮闭包；**只投意图**）。
    on_catalog_retry: Rc<CbSlot<()>>,
    /// 已应用的让位（幂等对账用；`0` / `STALE_BAND_H`）。
    stale_inset: Cell<i32>,
    /// 值样式（F8 三卡）。
    value_styles: [Rc<Style>; 2],
    /// 存活锚点（页面根下的固定件句柄）。
    _keep: Vec<Obj>,
}

impl P6SystemPage {
    /// 建页。
    pub fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = page_root(parent)?;
        let value_styles = [
            theme::text(TextSlot::CardValue, Palette::TEXT_PRIMARY),
            theme::text(TextSlot::Body, Palette::TEXT_WEAK),
        ];

        // ── 分段页签（5 段；段内容**惰性创建**）──
        let labels: Vec<&str> = SEGMENTS.iter().map(|(l, _)| *l).collect();
        let tabs = Rc::new(SegmentedTabs::new(&root, &labels, Dimens::TABS_Y)?);
        // 段名 / 段数的**构造期自证**（改文案或改段数即在离屏用例里响亮失败）。
        debug_assert_eq!(tabs.count(), SEGMENTS.len());
        for (i, (label, _)) in SEGMENTS.iter().enumerate() {
            debug_assert_eq!(tabs.tab_text(i).as_deref(), Some(*label));
        }

        // ── 段「装置」的内容（面板由 `SegmentedTabs` 惰性创建 ⇒ 先 select 再填）──
        tabs.select(SEG_DEVICE);
        let mut device: Option<DeviceSegment> = None;
        tabs.with_panel(SEG_DEVICE, true, |panel| {
            device = Some(Self::build_device_segment(panel, &value_styles).ok()?);
            Some(())
        });
        let Some(device) = device else {
            return Err(LvglError::InvalidArgument("P6: 段「装置」内容构建失败"));
        };

        // ── ⓪″ 段顶「名称表可能过期」提示条 + 「重试」（设计 §15.3.1 第 2 / 3 句）──────
        // 建在**页根**、落在**段内容区最顶部**（`Dimens::SECTION_Y`）：不重叠任何既有件
        // （`stale` 时各段的滚动视口让位 72 px，见 [`Self::apply_stale_inset`]），
        // 默认 `stale == false` ⇒ 两者皆隐、几何零差异。
        let stale_banner = WarnBanner::new(&root, STALE_BANNER_W, ui_text::CATALOG_STALE, &[])?;
        stale_banner.obj().set_pos(0, Dimens::SECTION_Y);
        stale_banner.obj().set_hidden(true);
        let stale_retry = Rc::new(TextButton::create(&root, ui_text::RETRY)?);
        stale_retry.set_size(Dimens::TOUCH_MIN, Dimens::TOUCH_MIN);
        stale_retry.set_pos(STALE_RETRY_X, Dimens::SECTION_Y + STALE_RETRY_Y);
        stale_retry.label().center();
        theme::button(theme::ButtonKind::Secondary).apply(&stale_retry);
        set_visible(&stale_retry, false);
        // 按钮**只投意图**（闭包体内零页面调用 —— 回调纪律见 [`Self::set_on_catalog_retry`]）。
        let retry_slot: Rc<CbSlot<()>> = Rc::new(CbSlot::new());
        {
            let slot = Rc::clone(&retry_slot);
            stale_retry.on_clicked(move |_| slot.fire(()));
        }

        let page = Self {
            root,
            tabs: Rc::clone(&tabs),
            device,
            segs: RefCell::new((0..SEGMENTS.len()).map(|_| None).collect()),
            bms_card: RefCell::new(None),
            drill: RefCell::new(None),
            drill_req: Rc::new(Cell::new(false)),
            periph: RefCell::new(PeripheralsSection::default()),
            catalog: RefCell::new(None),
            bms_page: RefCell::new(None),
            bms_failed: Cell::new(false),
            on_bms_page: CbSlot::new(),
            stale_banner,
            stale_retry,
            catalog_stale: Cell::new(false),
            on_catalog_retry: retry_slot,
            stale_inset: Cell::new(0),
            value_styles,
            _keep: Vec::new(),
        };
        page.render(&PageInput::init());
        tabs.wire_clicks();
        Ok(page)
    }

    /// 建段「装置」的内容（站状态表 + F8 三卡；**逐字保留** B2a 的版面）。
    fn build_device_segment(
        parent: &Obj,
        value_styles: &[Rc<Style>; 2],
    ) -> Result<DeviceSegment, LvglError> {
        let host = ScrollContainer::create(parent)?;
        host.set_size(Dimens::CONTENT_W, Dimens::SECTION_VIEW_H);
        host.set_pos(0, 0);
        host.add_style(&theme::transparent(), StyleSelector::main());
        let mut keep: Vec<Obj> = Vec::new();

        // ── ① 站状态表（卡头 + 5 行；行集合 = 5 个 role 全集）──
        let st_card = decor(&host, Dimens::CONTENT_W, ST_CARD_H, &theme::card())?;
        st_card.set_pos(0, ST_CARD_Y);
        let st_title = text_label(
            &host,
            station_group_title(),
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        st_title.set_pos(CARD_INSET + Dimens::GAP_MIN, ST_CARD_Y + CARD_INSET);
        let mut station_rows = Vec::with_capacity(STATION_ROWS);
        for i in 0..STATION_ROWS {
            let row = PoolRow::new(&host, Dimens::CONTENT_W - 2 * CARD_INSET, false)?;
            row.obj.set_pos(
                CARD_INSET,
                ST_CARD_Y + CARD_INSET + ST_HEAD_H + i as i32 * ST_ROW_H,
            );
            station_rows.push(row);
        }

        // ── ② F8 装置信息卡 ──
        let info_card = decor(&host, Dimens::CONTENT_W, INFO_CARD_H, &theme::card())?;
        info_card.set_pos(0, INFO_CARD_Y);
        let info_head = text_label(
            &info_card,
            TEXT_DEVICE_INFO,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        info_head.set_pos(0, 0);
        let info_frozen = crate::ui::pages::frozen_chip(&info_card, FROZEN_CHIP_X)?;
        let mut info = Vec::with_capacity(INFO_ROWS);
        for (i, name) in [TEXT_MODEL, TEXT_SERIAL, TEXT_FIRMWARE, TEXT_BUILD_TIME]
            .iter()
            .enumerate()
        {
            info.push(SysRow::new(
                &info_card,
                CARD_HEAD_H + i as i32 * ROW_H,
                name,
                value_styles,
                false,
            )?);
        }

        // ── ③ F8 运行信息卡（3/4/5 行为连接类）──
        let run_card = decor(&host, Dimens::CONTENT_W, RUN_CARD_H, &theme::card())?;
        run_card.set_pos(0, RUN_CARD_Y);
        let run_head = text_label(
            &run_card,
            TEXT_RUN_INFO,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        run_head.set_pos(0, 0);
        let run_frozen = crate::ui::pages::frozen_chip(&run_card, FROZEN_CHIP_X)?;
        let mut run = Vec::with_capacity(RUN_ROWS);
        for (i, name) in [
            TEXT_UPTIME,
            TEXT_CPU_TEMP,
            TEXT_MEM,
            TEXT_LINK_IEC104,
            TEXT_LINK_INTERCORE,
            TEXT_CHANNEL,
            TEXT_CONTROL_SOURCE,
        ]
        .iter()
        .enumerate()
        {
            run.push(SysRow::new(
                &run_card,
                CARD_HEAD_H + i as i32 * ROW_H,
                name,
                value_styles,
                matches!(i, 3..=5),
            )?);
        }

        // ── ④ F8 关于本屏卡（**有意不打冻帧标**：3 行里 2 行是编译期常量）──
        let about_card = decor(&host, Dimens::CONTENT_W, ABOUT_CARD_H, &theme::card())?;
        about_card.set_pos(0, ABOUT_CARD_Y);
        let about_head = text_label(
            &about_card,
            TEXT_ABOUT,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        about_head.set_pos(0, 0);
        let mut about = Vec::with_capacity(ABOUT_ROWS);
        for (i, name) in [TEXT_LOCAL_VERSION, TEXT_SERVICE_ADDR, TEXT_MGMT_IP]
            .iter()
            .enumerate()
        {
            about.push(SysRow::new(
                &about_card,
                CARD_HEAD_H + i as i32 * ROW_H,
                name,
                value_styles,
                false,
            )?);
        }
        let note = text_label(
            &about_card,
            TEXT_NO_REMOTE,
            TextSlot::Body,
            Palette::TEXT_WEAK,
        )?;
        note.set_pos(0, CARD_HEAD_H + ABOUT_ROWS as i32 * ROW_H);
        // 段内容总高自证（改任一卡的版面常量 ⇒ 在此响亮失败）。
        debug_assert_eq!(ABOUT_CARD_Y + ABOUT_CARD_H, DEVICE_TOTAL_H);

        // 卡 / 卡头标签的**存活锚点**（借用全部结束后再 move 进 keep）。
        keep.push(st_card);
        keep.push(st_title.into_obj());
        keep.push(info_card);
        keep.push(info_head.into_obj());
        keep.push(run_card);
        keep.push(run_head.into_obj());
        keep.push(about_card);
        keep.push(about_head.into_obj());

        Ok(DeviceSegment {
            host,
            _keep: keep,
            station_rows,
            info,
            run,
            about,
            note,
            info_frozen,
            run_frozen,
        })
    }

    /// 把**分段页签的点击**接到本页的段内容构建上（**必须在 `Rc<Self>` 上调用一次**）。
    ///
    /// 为什么要 `Rc`：`SegmentedTabs` 的点击回调是 `'static` 闭包，而要建段内容就必须拿到
    /// `&self`（periph / catalog / segs 都在页面上）⇒ 用 `Weak<Self>` 捕获（**不构成**
    /// 「对象 → 事件项 → 闭包 → 对象」的强引用环）。
    ///
    /// **可选**：即使不调用，[`P6SystemPage::render`] 的**对账**也会在下一拍补建（代价 = 最多
    /// 一拍延迟）；生产路径（`Shell::new`）调用它以保证"点段即切"（TT-05 的 ≤300 ms 口径）。
    pub fn wire_tabs(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.tabs.set_on_change(move |i| {
            if let Some(me) = weak.upgrade() {
                me.ensure_segment(i);
            }
        });
    }

    /// 渲染一帧（**只读**；不建 / 不删 LVGL 对象 —— 例外仅"对账"补建段内容那一支）。
    pub fn render(&self, input: &PageInput<'_>) {
        // ⓪ 意图：下钻打开 / 收起 / 翻页（**页面只产意图、不自行发请求**）。
        if self.drill_req.get() {
            self.drill_req.set(false);
            self.open_drill();
        }
        if let Some(d) = self.drill.borrow().clone() {
            if d.take_collapse_request() {
                d.set_open(false);
                self.tabs.select(SEG_BATTERY);
                self.ensure_segment(SEG_BATTERY);
            }
            while let Some(req) = d.take_page_request() {
                // 哨兵 → 目标页（页码 1 起；越界的"下一页"由接线层按 `has_more` 裁决）。
                let cur = d.page().map(|p| p.page).unwrap_or(1);
                let target = match req {
                    DRILL_REQ_RETRY => cur,
                    DRILL_REQ_NEXT => cur.saturating_add(1),
                    DRILL_REQ_PREV => cur.saturating_sub(1).max(1),
                    n => n,
                };
                self.on_bms_page.fire(target);
            }
        }

        // ⓪′ 段内容对账（唯一允许在 `render` 里建对象的支路）：当前选中的段若尚未建内容
        //     （例如接线层没调 `wire_tabs`，或点击与建段之间跨了拍）⇒ 在此补建一次。
        //     **常态下不发生**（段「装置」恒已建；其余段由点击路径即时建）。
        let sel = self.tabs.selected();
        if !self.segment_built(sel) {
            self.ensure_segment(sel);
        }

        // ① 区块级可信度打标（判据 = [`frame_mark`]，与 P1 同源）。
        let mark = frame_mark(input);
        if let Some(m) = mark {
            self.device.info_frozen.set_text(m);
            self.device.run_frozen.set_text(m);
        }
        set_visible(self.device.info_frozen.obj(), mark.is_some());
        set_visible(self.device.run_frozen.obj(), mark.is_some());

        let (device, _alarms, info) = sections(input);

        // ② 装置信息（F8.3：一次性读取；契约串一律过 `display_safe`）。
        let model = non_empty_opt(&info.model).map(display_safe);
        let serial = non_empty_opt(&info.serial).map(display_safe);
        let firmware = non_empty(&info.firmware_version).map(display_safe);
        let build_time = non_empty_opt(&info.build_time).map(display_safe);
        self.device.info[0].set_text(model.as_deref(), &self.value_styles);
        self.device.info[1].set_text(serial.as_deref(), &self.value_styles);
        self.device.info[2].set_text(firmware.as_deref(), &self.value_styles);
        self.device.info[3].set_text(build_time.as_deref(), &self.value_styles);

        // ③ 运行信息（与 P1 同源）。
        self.device.run[0].set_text(
            device.uptime_secs.map(format_uptime).as_deref(),
            &self.value_styles,
        );
        self.device.run[1].set_text(
            device
                .cpu_temp_c
                .map(|v| format!("{} {TEXT_CELSIUS}", fmt_int0(v)))
                .as_deref(),
            &self.value_styles,
        );
        self.device.run[2].set_text(
            device
                .mem_used_pct
                .map(|v| format!("{} {TEXT_PERCENT}", fmt_int0(v)))
                .as_deref(),
            &self.value_styles,
        );
        self.device.run[3].set_link(device.iec104);
        self.device.run[4].set_link(device.intercore);
        self.device.run[5].set_link(device.hmi_channel);
        self.device.run[6].set_text(
            Some(control_source_text(device.control_source)),
            &self.value_styles,
        );

        // ④ 关于本屏。
        self.device.about[0].set_text(Some(env!("CARGO_PKG_VERSION")), &self.value_styles);
        let service_addr = loopback_service_text();
        self.device.about[1].set_text(Some(&service_addr), &self.value_styles);
        let mgmt_ip = device_mgmt_ip(&info.mgmt_ipv4).map(display_safe);
        self.device.about[2].set_text(mgmt_ip.as_deref(), &self.value_styles);

        // ⑤ 站状态表（行集合由 catalog 驱动；**行数与在线状态无关** —— F25.5）。
        for (i, row) in self.device.station_rows.iter().enumerate() {
            match self.station_row_data(i) {
                Some(data) => row.bind(&data, self.station_row_y(i)),
                None => set_visible(&row.obj, false),
            }
        }

        // ⑥ 已创建的段（**只刷新，不创建**）⇒ 值变化不改布局。
        for i in 0..SEGMENTS.len() {
            if self.segs.borrow().get(i).and_then(|s| s.as_ref()).is_some() {
                self.refresh_segment(i);
            }
        }
    }

    /// 段「装置」第 `i` 行的数据。
    fn station_row_data(&self, i: usize) -> Option<RowData> {
        let periph = self.periph.borrow();
        let cat = self.catalog.borrow();
        let rows = station_rows(cat.as_ref(), &periph, &STATION_ROLES);
        rows.get(i).cloned()
    }

    /// 段「装置」第 `i` 行的 y（版面常量）。
    fn station_row_y(&self, i: usize) -> i32 {
        ST_CARD_Y + CARD_INSET + ST_HEAD_H + i as i32 * ST_ROW_H
    }

    // ── 外设数据入口（契约 2 / 2′：页面不自行发请求）─────────────────────────

    /// 注入外设段（1 Hz 帧的 `peripherals` 段）。
    pub fn set_periph(&self, sec: &PeripheralsSection) {
        *self.periph.borrow_mut() = sec.clone();
    }

    /// 注入点表目录（控制通道 `GET /peripherals/catalog`）。
    pub fn set_catalog(&self, cat: &PeripheralCatalog) {
        *self.catalog.borrow_mut() = Some(cat.clone());
    }

    /// catalog 失效 / 未取到 ⇒ 显「名称未获取」（**值照常显示**，§15.3.1）。
    ///
    /// ⚠️ **重取失败不得调本方法**（§15.3.1「重取失败 ⇒ **保留旧 catalog**」）——失败面只置
    /// [`P6SystemPage::set_catalog_stale`]。
    pub fn clear_catalog(&self) {
        *self.catalog.borrow_mut() = None;
    }

    /// **【⓪″ 段顶提示】名称表可能过期**（U-73 / 设计 §15.3.1 第 2 句；**T21c-3-r1**）。
    ///
    /// `stale = true` ⇒ 显「名称表可能过期」提示条（[`WarnBanner`]）+「重试」按钮
    /// （≥ `TOUCH_MIN`(48)×48，净距 `GAP_MIN`(16)），并把**当前段的滚动视口**下移
    /// `STALE_BAND_H`(72) 且等量变矮 —— 落点在**段内容区最顶部**、底缘不动 ⇒ 不遮住既有内容、
    /// 段外框与页面根几何不变；默认 `stale == false` ⇒ 两者皆隐 + 几何**逐像素回既有版面**。
    ///
    /// **谁调 / 何时调**：接线层（`app.rs`）在 catalog **重取失败**时置 `true`（**保留旧 catalog**
    /// —— 不调 [`P6SystemPage::clear_catalog`]）、**重取成功**时置 `false`。
    /// **每次调用都直接落屏**（幂等；重复置同值不做事）；切段时的让位由
    /// [`P6SystemPage::ensure_segment`] 对**新段**补落一次。
    pub fn set_catalog_stale(&self, stale: bool) {
        if self.catalog_stale.replace(stale) == stale {
            return;
        }
        set_visible(self.stale_banner.obj(), stale);
        set_visible(&self.stale_retry, stale);
        self.apply_stale_inset(self.tabs.selected());
    }

    /// 注册「重试」意图回调（无载荷）—— 点段顶提示条右侧的「重试」即触发；**本页不发请求**。
    ///
    /// **回调纪律**（本仓成文教训，见 `app.rs` 的 `bind_intents` 函数头）：闭包体内**只许投意图**
    /// （`push_back(..)`），**不得**回灌页面数据 —— 该回调用在 LVGL 事件派发内被同步触发，
    /// 在里面删 / 建对象会 UAF。页面本体的写入发生在接线层的 tick / `apply_route` 那一拍。
    pub fn set_on_catalog_retry<F>(&self, mut f: F)
    where
        F: FnMut() + 'static,
    {
        // `CbSlot` 的载荷恒有类型（此处 `()`）⇒ 无参回调包一层（**不**改共享槽的契约）。
        self.on_catalog_retry.set(move |()| f());
    }

    /// 提示条**是否在显**（断言 / 装配口径；默认 `false`）。
    pub fn catalog_stale_visible(&self) -> bool {
        !self.stale_banner.obj().is_hidden()
    }

    /// 提示条文案（断言口径；恒 = `ui_text::CATALOG_STALE`）。
    pub fn catalog_stale_text(&self) -> Option<String> {
        self.stale_banner.text()
    }

    /// 「重试」按钮（尺寸 / 点位 / 点击投意图的断言用）。
    pub fn catalog_retry_button(&self) -> &TextButton {
        &self.stale_retry
    }

    /// 提示条本体对象（版面断言用：矩形 / 与段内容、按钮的净距）。
    pub fn catalog_stale_banner_obj(&self) -> &Obj {
        self.stale_banner.obj()
    }

    /// 段 `i` 的**内容滚动视口**对象（提示条让位 / 版面断言用；未创建 ⇒ `None`）。
    /// 段「装置」= 其 `host`；外设段 = `SegmentList` 的视口（**非拥有句柄** ⇒ 不增对象计数）。
    pub fn segment_viewport_obj(&self, i: usize) -> Option<Obj> {
        if i == SEG_DEVICE {
            return Some(self.device.host.share_borrowed());
        }
        self.segs
            .borrow()
            .get(i)
            .and_then(|s| s.as_ref())
            .map(|l| l.obj().share_borrowed())
    }

    /// 下钻视图**根容器**矩形（提示条让位断言用；未创建 ⇒ `None`）。
    pub fn drill_root_coords(&self) -> Option<Area> {
        self.drill.borrow().as_ref().map(|d| d.root_coords())
    }

    /// 当前生效的让位（`0` / `STALE_BAND_H`；版面断言读口）。
    pub fn stale_inset(&self) -> i32 {
        self.stale_inset.get()
    }

    /// 注册下钻「翻页」意图回调（载荷 = 目标页码，1 起）。
    pub fn set_on_bms_page<F>(&self, f: F)
    where
        F: FnMut(u32) + 'static,
    {
        self.on_bms_page.set(f);
    }

    /// 注入一页 BMS 告警位（`None` 由 [`P6SystemPage::set_bms_page_failed`] 表达）。
    pub fn set_bms_page(&self, page: &BmsAlarmPage) {
        self.bms_failed.set(false);
        *self.bms_page.borrow_mut() = Some(page.clone());
        self.refresh_bms();
    }

    /// 下钻端点失败 ⇒ **只显本地固定文案**「明细不可用」+「重试」（R-4；服务端串只进日志）。
    pub fn set_bms_page_failed(&self) {
        self.bms_failed.set(true);
        *self.bms_page.borrow_mut() = None;
        self.refresh_bms();
    }

    /// 刷新摘要卡与下钻（三态文案由 `available` / 站级态给出，**屏侧不重判**）。
    fn refresh_bms(&self) {
        let page = self.bms_page.borrow().clone();
        let failed = self.bms_failed.get();
        let st = self.station_of(PeriphRole::Battery);
        if let Some(card) = self.bms_card.borrow().as_ref() {
            card.render(page.as_ref(), st, |at| self.bms_label(at));
        }
        if let Some(drill) = self.drill.borrow().as_ref() {
            drill.set_page(page, failed);
        }
    }

    /// 位 → 短标签（catalog；未取到 ⇒ `None`）。
    fn bms_label(&self, at: u16) -> Option<String> {
        self.bms_labels()
            .into_iter()
            .find(|(a, _)| *a == at)
            .map(|(_, l)| l)
    }

    /// 位 → 短标签表（catalog 的 `bms_alarm` 块）。
    fn bms_labels(&self) -> Vec<(u16, String)> {
        self.catalog
            .borrow()
            .as_ref()
            .and_then(|c| c.stations.iter().find(|s| s.role == PeriphRole::Battery))
            .and_then(|s| s.blocks.iter().find(|b| b.name == machine_key("bms_alarm")))
            .map(|b| b.points.iter().map(|p| (p.at, p.label.clone())).collect())
            .unwrap_or_default()
    }

    // ── 段内容（惰性创建 + 只切可见性 + 刷新）───────────────────────────────

    /// 切到第 `i` 段（**不改 `current_page`**；段内容惰性创建）。
    pub fn select_segment(&self, i: usize) {
        // 明确的段切换（含点回本段）= 收起下钻（用户的可达出口）。
        if let Some(d) = self.drill.borrow().as_ref() {
            d.set_open(false);
        }
        self.tabs.select(i);
        self.ensure_segment(i);
    }

    /// 确保段 `i` 的内容已建（**首次进入才建**）并刷新。
    fn ensure_segment(&self, i: usize) {
        if i == SEG_DEVICE {
            // 段「装置」在 `new()` 里已建 ⇒ 只需把当前让位状态落一次（切回本段时）。
            self.apply_stale_inset(SEG_DEVICE);
            return;
        }
        let Some(role) = SEGMENTS.get(i).and_then(|(_, r)| *r) else {
            return;
        };
        if let Some(list) = self.segs.borrow().get(i).and_then(|s| s.as_ref()).cloned() {
            self.refresh_with(role, &list);
            self.apply_stale_inset(i);
            return;
        }
        let mut created: Option<Rc<SegmentList>> = None;
        self.tabs.with_panel(i, true, |panel| {
            // ⚠️ 面板是**普通容器**（不是滚动容器）：每段自己带滚动视口
            // （外设段 = `SegmentList` 的视口；段「装置」= `host`）。
            if let Ok(list) = SegmentList::new(panel, role == PeriphRole::Hvac) {
                let list = Rc::new(list);
                list.wire_scroll();
                // 电池段：摘要卡（特殊件 ⇒ 由列表按模型里的 `Special` 行摆放）。
                if role == PeriphRole::Battery {
                    if let Ok(card) = BmsAlarmCard::new(panel, Rc::clone(&self.drill_req)) {
                        let card = Rc::new(card);
                        list.adopt_special(card.obj());
                        *self.bms_card.borrow_mut() = Some(card);
                    }
                }
                created = Some(Rc::clone(&list));
            }
        });
        if let Some(list) = created {
            self.refresh_with(role, &list);
            if let Some(slot) = self.segs.borrow_mut().get_mut(i) {
                *slot = Some(list);
            }
            self.apply_stale_inset(i);
        }
    }

    /// **段顶提示条的让位**（T21c-3-r1）：`stale` 时把段 `i` 的自带滚动视口下移 `STALE_BAND_H`
    /// 并等量变矮（**底缘不动**）；否则 `set_inset(0)` 复原。只动几何（`set_pos` / `set_size`），
    /// **不建不删对象**（页面在 `render` 里"只改不改建"的纪律）。
    ///
    /// **为什么只在"当前段"落地**：同一时刻只有一个段面板可见 —— 切段时
    /// [`Self::ensure_segment`] 会对新段补落一次（含段「装置」的早退支）；被隐藏的段保持
    /// 旧几何不影响任何可见像素，切回时同样经 `ensure_segment` 归位。
    fn apply_stale_inset(&self, i: usize) {
        let inset = stale_inset(self.catalog_stale.get());
        if i == SEG_DEVICE {
            self.device.host.set_pos(0, inset);
            self.device
                .host
                .set_size(Dimens::CONTENT_W, Dimens::SECTION_VIEW_H - inset);
        }
        if let Some(list) = self.segs.borrow().get(i).and_then(|s| s.as_ref()) {
            list.obj().set_pos(0, inset);
            list.obj()
                .set_size(Dimens::CONTENT_W, Dimens::SECTION_VIEW_H - inset);
        }
        // 下钻是段「电池」面板里的覆盖视图（同一个段内容区）⇒ 一并让位。
        if i == SEG_BATTERY {
            if let Some(d) = self.drill.borrow().as_ref() {
                d.set_inset(inset);
            }
        }
        self.stale_inset.set(inset);
    }

    /// 用当前注入数据刷新段 `i`。
    fn refresh_segment(&self, i: usize) {
        let list = self.segs.borrow().get(i).and_then(|s| s.as_ref()).cloned();
        if let (Some(role), Some(list)) = (SEGMENTS.get(i).and_then(|(_, r)| *r), list) {
            self.refresh_with(role, &list);
        }
    }

    /// 刷新某段的模型（站状态行 + 分组行）并重绑窗口。
    fn refresh_with(&self, role: PeriphRole, list: &SegmentList) {
        let periph = self.periph.borrow().clone();
        let cat = self.catalog.borrow().clone();
        let st = station_state_of(cat.as_ref(), &periph, role);
        let special_h = (role == PeriphRole::Battery).then(|| {
            let rows = self
                .bms_page
                .borrow()
                .as_ref()
                .map(|p| {
                    p.items
                        .iter()
                        .filter(|x| x.active)
                        .count()
                        .min(BMS_SUMMARY_LIST_ROWS)
                })
                .unwrap_or(0);
            BmsAlarmCard::height(rows)
        });
        let mut model = segment_model(role, cat.as_ref(), &periph, special_h);
        // **段顶站状态行**（§6.6.1「站状态条（每段顶…）」）插在模型最前。
        let mut head = station_rows(cat.as_ref(), &periph, &[role]);
        head.append(&mut model.rows);
        model.rows = head;
        let (ys, cards, total) = layout(&model.rows);
        model.ys = ys;
        model.cards = cards;
        model.total_h = total;
        list.set_model(model);
        if role == PeriphRole::Battery {
            if let Some(card) = self.bms_card.borrow().as_ref() {
                let page = self.bms_page.borrow().clone();
                card.render(page.as_ref(), st, |at| self.bms_label(at));
            }
        }
    }

    /// 段 `i` 的站级态。
    fn station_of(&self, role: PeriphRole) -> StationState {
        station_state_of(self.catalog.borrow().as_ref(), &self.periph.borrow(), role)
    }

    // ── 下钻（BMS 288 位；**不改 `current_page`**）──────────────────────────

    /// 打开下钻视图（首次打开才建；**当前段仍为段「电池」**）。
    pub fn open_drill(&self) {
        // 下钻属于段「电池」⇒ 先把该段设为当前段并确保其内容已建（面板已存在）。
        self.tabs.select(SEG_BATTERY);
        self.ensure_segment(SEG_BATTERY);
        if self.drill.borrow().is_none() {
            let mut created: Option<Rc<BmsDrill>> = None;
            self.tabs.with_panel(SEG_BATTERY, true, |panel| {
                if let Ok(d) = BmsDrill::new(panel) {
                    let d = Rc::new(d);
                    // **滚动 ⇒ 重绑窗口**（F1：与 4 个外设段同款；不注册 ⇒ 池外行永不绘）。
                    d.wire_scroll();
                    created = Some(d);
                }
            });
            *self.drill.borrow_mut() = created;
        }
        let Some(drill) = self.drill.borrow().clone() else {
            return;
        };
        drill.set_labels(self.bms_labels());
        let page = self.bms_page.borrow().clone();
        drill.set_page(page, self.bms_failed.get());
        drill.set_open(true);
        // **首次打开时下钻刚被建出来**（构造默认几何 = 无让位）⇒ 必须在建之后补落一次
        // 段顶提示条的让位，否则下钻顶部条的「收起」会与提示条右侧的「重试」**重叠**
        // （两块可点区域叠在一起 ⇒ 触碰命中区歧义；T21c-3-r1）。
        self.apply_stale_inset(SEG_BATTERY);
        // 段「电池」的段内容让位（下钻占满整个段内容区；摘要卡是段内容的子对象 ⇒ 随之一并隐藏）。
        if let Some(list) = self.segs.borrow().get(SEG_BATTERY).and_then(|s| s.as_ref()) {
            set_visible(list.obj(), false);
        }
    }

    /// 收起下钻视图（回到段「电池」的摘要卡）。
    pub fn collapse_drill(&self) {
        self.close_drill();
        self.tabs.select(SEG_BATTERY);
        self.ensure_segment(SEG_BATTERY);
    }

    /// **关闭下钻**（收起视图 + 恢复段「电池」的段内容可见性）。
    ///
    /// ⚠️ 接线层**必须在离开 P6 时调用**（否则"切走再切回"会残留打开态 —— 偏差 **P6-7**；
    /// `shell::on_page_change` 已具备回调位）。
    pub fn close_drill(&self) {
        if let Some(drill) = self.drill.borrow().as_ref() {
            drill.set_open(false);
        }
        if let Some(list) = self.segs.borrow().get(SEG_BATTERY).and_then(|s| s.as_ref()) {
            set_visible(list.obj(), true);
        }
    }

    /// 下钻是否打开。
    pub fn drill_open(&self) -> bool {
        self.drill
            .borrow()
            .as_ref()
            .map(|d| d.is_open())
            .unwrap_or(false)
    }

    /// 收起按钮对象（`(宽, 高)` 版面断言用；未创建 ⇒ `None`）。
    pub fn drill_collapse_size(&self) -> Option<(i32, i32)> {
        self.drill
            .borrow()
            .as_ref()
            .map(|d| d.collapse_button().size())
    }

    /// 上一页 / 下一页按钮尺寸。
    pub fn drill_page_button_sizes(&self) -> Option<((i32, i32), (i32, i32))> {
        self.drill
            .borrow()
            .as_ref()
            .map(|d| (d.page_buttons().0.size(), d.page_buttons().1.size()))
    }

    /// 下钻顶部条标题（断言口径）。
    pub fn drill_title(&self) -> Option<String> {
        self.drill.borrow().as_ref().and_then(|d| d.title_text())
    }

    /// 下钻失败带是否可见（R-4：**只显本地固定文案**）。
    pub fn drill_fail_visible(&self) -> bool {
        self.drill
            .borrow()
            .as_ref()
            .map(|d| d.fail_visible())
            .unwrap_or(false)
    }

    /// 下钻「重试」是否可见。
    pub fn drill_retry_visible(&self) -> bool {
        self.drill
            .borrow()
            .as_ref()
            .map(|d| d.retry_visible())
            .unwrap_or(false)
    }

    /// 下钻行池大小（**行数上界断言的读口**）。
    pub fn drill_pool_size(&self) -> usize {
        self.drill
            .borrow()
            .as_ref()
            .map(|d| d.pool_size())
            .unwrap_or(0)
    }

    /// **仅测试**：把下钻行区滚到 `y`（**只发滚动** —— 与生产 `SCROLL` 事件同源；
    /// 不另调任何重绑入口 ⇒ 删掉 `wire_scroll` 注册即红，见 F1 / W-a）。
    #[cfg(test)]
    pub(crate) fn scroll_drill(&self, y: i32) {
        if let Some(d) = self.drill.borrow().as_ref() {
            d.viewport.scroll_to_y(y);
        }
    }

    /// **仅测试**：下钻当前窗口起点（`None` = 无页数据 / 失败态）。
    #[cfg(test)]
    pub(crate) fn drill_window_start(&self) -> Option<usize> {
        self.drill.borrow().as_ref().and_then(|d| d.window_start())
    }

    /// **仅测试**：下钻池行 `k` 的两位文本（`None` = 行不在池内）。
    #[cfg(test)]
    pub(crate) fn drill_row_names(&self, k: usize) -> Option<(Option<String>, Option<String>)> {
        self.drill.borrow().as_ref().and_then(|d| d.row_names(k))
    }

    /// **仅测试**：下钻行区可见行数（池内已绑且在显的行）。
    pub fn drill_visible_rows(&self) -> usize {
        self.drill
            .borrow()
            .as_ref()
            .map(|d| d.rows.iter().filter(|r| !r.obj.is_hidden()).count())
            .unwrap_or(0)
    }

    /// **仅测试**：下钻「翻页」按钮的点击（走生产事件路径）。
    #[cfg(test)]
    pub(crate) fn click_drill_page(&self, next: bool) {
        if let Some(d) = self.drill.borrow().as_ref() {
            let (prev, nxt) = d.page_buttons();
            let b = if next { nxt } else { prev };
            b.send_event(EventCode::CLICKED);
        }
    }

    /// **仅测试**：下钻「收起」的点击（走生产事件路径）。
    #[cfg(test)]
    pub(crate) fn click_drill_collapse(&self) {
        if let Some(d) = self.drill.borrow().as_ref() {
            d.collapse_button().send_event(EventCode::CLICKED);
        }
    }

    /// **仅测试**：摘要卡「查看全部 288 位」的点击（走生产事件路径）。
    #[cfg(test)]
    pub(crate) fn click_bms_entry(&self) {
        if let Some(c) = self.bms_card.borrow().as_ref() {
            c.entry.send_event(EventCode::CLICKED);
        }
    }

    // ── 段断言的读口 ────────────────────────────────────────────────────────

    /// 段 `i` 是否已创建（惰性创建的读回口）。
    ///
    /// ⚠️ 段「装置」**恒为 `true`**（其内容 = 站状态表 + F8 三卡，随页装配，不惰性）；
    /// 4 个外设段才走惰性（`segs[i].is_some()`）。
    pub fn segment_built(&self, i: usize) -> bool {
        if i == SEG_DEVICE {
            return true;
        }
        self.segs
            .borrow()
            .get(i)
            .map(Option::is_some)
            .unwrap_or(false)
    }

    /// 段 `i` 的行模型（`(kind, 行高)`；未创建 ⇒ `None`）。
    pub fn segment_rows(&self, i: usize) -> Option<Vec<(RowKind, i32)>> {
        self.segs
            .borrow()
            .get(i)
            .and_then(|s| s.as_ref())
            .map(|l| l.model_rows())
    }

    /// 段 `i` 的行池大小（**与行数无关**的上界读口）。
    pub fn segment_pool_size(&self, i: usize) -> Option<usize> {
        self.segs
            .borrow()
            .get(i)
            .and_then(|s| s.as_ref())
            .map(|l| l.pool_size())
    }

    /// 段 `i` 第 `k` 行的四槽文本（窗口内；`(label, value, aux, aux2)`）。
    pub fn segment_row_text(&self, i: usize, k: usize) -> Option<(String, String, String, String)> {
        self.segs
            .borrow()
            .get(i)
            .and_then(|s| s.as_ref())
            .and_then(|l| l.text_at(k))
    }

    /// 段 `i` 第 `k` 行**可见槽**的实测版面（`(x1, x2, 槽宽)`；F2 的版面断言读口）。
    pub fn segment_row_col_spans(&self, i: usize, k: usize) -> Option<Vec<(i32, i32, i32)>> {
        self.segs
            .borrow()
            .get(i)
            .and_then(|s| s.as_ref())
            .and_then(|l| l.col_spans_at(k))
    }

    /// 段 `i` 当前**可见**池行数。
    pub fn segment_visible_rows(&self, i: usize) -> Option<usize> {
        self.segs
            .borrow()
            .get(i)
            .and_then(|s| s.as_ref())
            .map(|l| l.visible_count())
    }

    /// 段 `i` 的行数（模型行数）。
    pub fn segment_row_count(&self, i: usize) -> Option<usize> {
        self.segs
            .borrow()
            .get(i)
            .and_then(|s| s.as_ref())
            .map(|l| l.row_count())
    }

    /// 段 `i` 的内容总高。
    pub fn segment_total_h(&self, i: usize) -> Option<i32> {
        self.segs
            .borrow()
            .get(i)
            .and_then(|s| s.as_ref())
            .map(|l| l.total_h())
    }

    /// 段 `i` 的内容容器是否可见。
    pub fn segment_visible(&self, i: usize) -> bool {
        self.tabs.panel_visible(i)
    }

    /// 段 `i` 的段名文字。
    pub fn segment_label(&self, i: usize) -> Option<String> {
        self.tabs.tab_text(i)
    }

    /// 分段控件本体（版面断言用）。
    pub fn tabs_obj(&self) -> &Obj {
        self.tabs.obj()
    }

    /// 第 `i` 段按钮对象（版面断言用）。
    pub fn tab_obj(&self, i: usize) -> Option<Obj> {
        self.tabs.tab(i).map(|o| o.share_borrowed())
    }

    /// 当前段下标。
    pub fn selected_segment(&self) -> usize {
        self.tabs.selected()
    }

    /// 段内容容器在页内的 y（版面断言用）。
    pub fn segment_content_y(&self) -> i32 {
        self.tabs.panel_y()
    }

    /// **仅测试**：把一次点击送到第 `i` 段（走生产事件路径 ⇒ 同时建段内容）。
    #[cfg(test)]
    pub(crate) fn click_segment(&self, i: usize) {
        self.tabs.click_tab(i);
    }

    /// **仅测试**：把段 `i` 滚到 `y`。
    ///
    /// ⚠️ **只发滚动、不另调重绑入口**（T21c-2-r1 / W-a）：重绑必须由生产注册的
    /// `SCROLL` 回调（[`SegmentList::wire_scroll`]）驱动 ⇒ 删掉任一注册点，本节断言即红
    /// （此前这里显式调 `on_scroll()` ⇒ 注册点**无网**）。
    #[cfg(test)]
    pub(crate) fn scroll_segment(&self, i: usize, y: i32) {
        if let Some(list) = self.segs.borrow().get(i).and_then(|s| s.as_ref()) {
            list.obj().scroll_to_y(y);
        }
    }

    /// **仅测试**：段 `i` 当前窗口起点。
    #[cfg(test)]
    pub(crate) fn segment_window_start(&self, i: usize) -> Option<usize> {
        self.segs
            .borrow()
            .get(i)
            .and_then(|s| s.as_ref())
            .and_then(|l| l.window_start())
    }

    // ── 只读断言口径（F8 三卡；**B2a 起的既有断言面，逐条保留**）─────────────

    /// 页面根。
    pub fn obj(&self) -> &Obj {
        &self.root
    }

    /// 段「装置」的内容宿主（可滚动容器）。
    pub fn device_host_obj(&self) -> &Obj {
        &self.device.host
    }

    /// 装置信息卡第 `i` 行的行名。
    pub fn info_label(&self, i: usize) -> Option<String> {
        self.device.info.get(i).and_then(|r| r.name_text())
    }

    /// 装置信息卡第 `i` 行的值文字。
    pub fn info_value(&self, i: usize) -> Option<String> {
        self.device.info.get(i).and_then(|r| r.value_text())
    }

    /// 运行信息卡第 `i` 行的行名。
    pub fn run_label(&self, i: usize) -> Option<String> {
        self.device.run.get(i).and_then(|r| r.name_text())
    }

    /// 运行信息卡第 `i` 行的值文字（连接类为指示灯的文字）。
    pub fn run_value(&self, i: usize) -> Option<String> {
        self.device.run.get(i).and_then(|r| r.value_text())
    }

    /// 关于本屏卡第 `i` 行的行名。
    pub fn about_label(&self, i: usize) -> Option<String> {
        self.device.about.get(i).and_then(|r| r.name_text())
    }

    /// 关于本屏卡第 `i` 行的值文字。
    pub fn about_value(&self, i: usize) -> Option<String> {
        self.device.about.get(i).and_then(|r| r.value_text())
    }

    /// 本地屏版本。
    pub fn local_version(&self) -> Option<String> {
        self.about_value(0)
    }

    /// 「本机服务地址」行的值（**恒为回环地址**）。
    pub fn service_address(&self) -> Option<String> {
        self.about_value(1)
    }

    /// 「本机服务地址」行的行名。
    pub fn service_label(&self) -> Option<String> {
        self.about_label(1)
    }

    /// 「设备管理 IP」行的值（缺失 ⇒ 「未提供」）。
    pub fn mgmt_ipv4(&self) -> Option<String> {
        self.about_value(2)
    }

    /// 「设备管理 IP」行的行名。
    pub fn mgmt_label(&self) -> Option<String> {
        self.about_label(2)
    }

    /// 说明行文字。
    pub fn note_text(&self) -> Option<String> {
        self.device.note.text()
    }

    /// 装置信息卡的「冻结 / 数据过期」角标是否可见。
    pub fn info_frozen_visible(&self) -> bool {
        !self.device.info_frozen.obj().is_hidden()
    }

    /// 上述角标当前文案。
    pub fn info_frozen_text(&self) -> Option<String> {
        self.device.info_frozen.text()
    }

    /// 运行信息卡同类角标是否可见。
    pub fn run_frozen_visible(&self) -> bool {
        !self.device.run_frozen.obj().is_hidden()
    }

    /// 运行信息卡角标当前文案。
    pub fn run_frozen_text(&self) -> Option<String> {
        self.device.run_frozen.text()
    }

    /// **仅测试**：装置信息卡角标的构造侧读回。
    #[cfg(test)]
    pub(crate) fn info_frozen_chip(&self) -> &StatusChip {
        &self.device.info_frozen
    }

    /// **仅测试**：运行信息卡角标的构造侧读回。
    #[cfg(test)]
    pub(crate) fn run_frozen_chip(&self) -> &StatusChip {
        &self.device.run_frozen
    }

    /// 装置信息卡行数。
    pub fn info_row_count(&self) -> usize {
        self.device.info.len()
    }

    /// 运行信息卡行数。
    pub fn run_row_count(&self) -> usize {
        self.device.run.len()
    }

    /// 关于本屏卡行数。
    pub fn about_row_count(&self) -> usize {
        self.device.about.len()
    }

    /// 站状态表的行数（**恒 5**）。
    pub fn station_row_count(&self) -> usize {
        self.device.station_rows.len()
    }

    /// 站状态表第 `i` 行的**四列**文本（`(站名, 状态, 成功, 更新)`；UI §6.6.1 的四段分栏）。
    pub fn station_row_text(&self, i: usize) -> Option<(String, String, String, String)> {
        let (a, b, c, d) = self.device.station_rows.get(i)?.texts();
        Some((
            a.unwrap_or_default(),
            b.unwrap_or_default(),
            c.unwrap_or_default(),
            d.unwrap_or_default(),
        ))
    }

    /// 站状态表第 `i` 行**可见列**的实测版面（`(x1, x2, 槽宽)`；F2 的版面断言读口）。
    pub fn station_row_col_spans(&self, i: usize) -> Option<Vec<(i32, i32, i32)>> {
        self.device.station_rows.get(i).map(|r| r.col_spans())
    }

    /// 站状态表第 `i` 行的行高（**降级行与在线时同行高**的断言口径）。
    pub fn station_row_height(&self, i: usize) -> Option<i32> {
        self.device.station_rows.get(i).map(|r| r.obj.size().1)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. 小工具
// ═══════════════════════════════════════════════════════════════════════════

/// 「本机服务地址」行的值：读 / 控制通道端点（**端点常量取自 `display-proto`**）。
fn loopback_service_text() -> String {
    format!("{DEFAULT_BIND}{TEXT_ADDR_SEP}{DEFAULT_CONTROL_BIND}")
}

/// 设备管理 IP 的显示值：`None` → 「未提供」（EDGE-16，**不臆造**）。
fn device_mgmt_ip(mgmt: &Option<String>) -> Option<&str> {
    non_empty_opt(mgmt)
}

/// 空串 / `None` 都算"未提供"。
fn non_empty(v: &str) -> Option<&str> {
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// `Option<String>` 版本的 [`non_empty`]。
fn non_empty_opt(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|s| !s.is_empty())
}

/// 服务监听口径文字（`ServiceScope` 的展示名）。
pub fn service_scope_text() -> &'static str {
    ServiceScope::LoopbackOnly.display_name()
}

/// 站状态表的组标题（`GROUP_TITLES` 的唯一取值点；供用例核对字面量）。
pub fn station_group_title() -> &'static str {
    group_title("station_status").unwrap_or(ui_text::ENUM_UNKNOWN)
}

/// 段「电池」的 `bms_alarm` 组标题（同上）。
pub fn bms_alarm_group_title() -> &'static str {
    group_title("bms_alarm").unwrap_or(ui_text::ENUM_UNKNOWN)
}

/// 摘要卡入口按钮的文案（「查看全部 288 位」；供用例核对字面量）。
pub fn bms_entry_text() -> String {
    BmsAlarmCard::entry_text(u32::from(u16::try_from(288).unwrap_or(u16::MAX)))
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. 测试模块标记（**空模块** —— 用例全部写在 `ui/tests.rs`）
//
// 码表静态网 `ui/tests.rs::ui_texts_covered_by_font_cmap` 用"第一个其后紧跟 `mod tests` 的
// `#[cfg(test)]`"划分生产区 / 测试区 ⇒ 本文件必须**在末尾**留一个 `mod tests`（哪怕是空的），
// 否则该网判定"截断前提不成立"并响亮失败。
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {}
