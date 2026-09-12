//! # P1 主状态页（12-MUPC v2.0 工作单元 **B2a**）
//!
//! 设计 §6.1「P1 主状态页（默认页 / 超时回归目标页）」逐条落地；UI 设计 §6.1 给视觉规格。
//!
//! | 需求 | 内容 | 数据来源（帧 v2） | 本页落点 |
//! |------|------|--------------------|----------|
//! | F1 | SOC 主读数 148 px + 源胶囊 + 三段量程条 | `soc` / `soc_source` / `soc_flag` | [`P1StatusPage::render`] → `render_soc` |
//! | F2 | PCS 四态（文字 + 语义色 + 图标）+ 方向不一致角标 | `run_state` / `pcs_online` / `inconsistency` | `render_pcs` |
//! | F3/F4 | 三相 P（64 px）/ I（48 px）四卡 + 总有功 | `p_phase` / `p_total` / `i_phase` | `render_phases` |
//! | F5 | 「数据过期」角标 + 通道断条 | `ts_ms` + 注入的 [`ChannelStatus`] / [`Freshness`] | `render_channel` |
//! | F6 | 装置状态网格 8 卡 | `device` + `info` | `render_device` |
//! | F7 | 告警 ≤10 条（倒序）+ 空态 / 源不可用 | `alarms` | `render_alarms` |
//!
//! ## 只读（硬约束）
//!
//! 本页**无写操作**：不注册任何控制通道回调、不产生任何 `POST`（PL-01 / 设计 §6.1「无写操作」）。
//!
//! ## 降级语义（**严禁补 0**）
//!
//! - 字段 `FieldFlag != Valid` 或 `v = None` ⇒ 显 [`PLACEHOLDER`]（`Palette::PLACEHOLDER`），
//!   并在该卡卡头给出 `StatusChip` 原因（`未取数` / `源离线` / `数据异常`）；
//! - 相卡的**新鲜度点与降级角标由该相 P **与** I 的整体可用性驱动**（PRD F5.5「各字段独立降级」）：
//!   任一缺失即降级 —— 不得因 P 有效而把"仅 I 缺失"显示成「实时」（B2a 规格评审 ①）；
//! - `soc = None` 或 `soc_source = Lost` ⇒ `–` + 「SOC 源失效」红胶囊 + 卡顶 3 px 红描边
//!   + 量程条整体灰化（EDGE-02，**不沿用旧值**）；
//! - `run_state = None` / `pcs_online = false` ⇒ 状态词槽位「离线」（EDGE-01；**2 字**，
//!   `PCS` 由卡头 `PCS 运行状态` 表达 —— 见偏差登记 **D7**）；
//! - `alarms.available = false` ⇒ `UnavailableState`「告警源不可用」；**空列表**才是
//!   `EmptyState`「近 24 小时无告警」—— 二者**语义不同、不得互替**（EDGE-09 / UI §8.3）。
//!
//! ## 栅格常量（**全部由 theme 推导**）
//!
//! `theme.rs` 未收录"页级"栅格（卡高 / 列宽 / 段位移等），而 B2a 不得改 B1 的交付物 ⇒
//! 本文件顶部集中定义，**每个数都由 theme 常量算出**（无裸数值），推导写在注释里。
//!
//! ## 帧内文案与字体子集的冲突（**三处偏差，逐条标注**）
//!
//! 1. `SocSource::display_name()` 的 `PCS(REG1010)` 含 **ASCII 圆括号**，字符集与
//!    `lv_font_noto_sc_*.c` 字形表均无 `(`/`)` ⇒ 取 [`TEXT_SOC_SRC_PCS`]；
//! 2. `ControlSource::AiDisabled.display_name()` 含**全角逗号**（同样不在集合内）⇒ 取
//!    [`TEXT_AI_DISABLED`]；
//! 3. UI §6.1 的 PCS 图标 `❚❚待` 用 U+275A，字形表无该项 ⇒ 待机取 `○`。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mupc_display_proto::{
    AlarmLevel, ControlSource, FieldFlag, LinkState, RunState, SocSource,
};

use crate::lvgl::obj::Obj;
use crate::lvgl::style::{Color, Style};
use crate::lvgl::widgets::{Label, LongMode, ScrollContainer};
use crate::lvgl::LvglError;
use crate::state::{self, ChannelStatus, Freshness, LiveDot, NumView, SocBand};
use crate::ui::components::{
    EmptyState, LedIndicator, StatusChip, UnavailableKind, UnavailableState,
};
use crate::ui::pages::{
    decor, format_epoch_ms_utc, format_uptime, label, layout_box, page_root, sections,
    set_style_index, set_visible, show_only, text_label, PageInput, MISSING, NOT_READ, PLACEHOLDER,
};
use crate::ui::theme::{self, ChipSkin, Dimens, Palette, TextSlot};

// ═══════════════════════════════════════════════════════════════════════════
// 0. 上屏文案（**逐字取自 UI §3.6 全屏用字表**）
//
// 码表覆盖率走查见 `ui/tests.rs::ui_texts_covered_by_font_cmap` —— 其**基线是生成字体的
// 实际 cmap**（`fonts/lv_font_noto_sc_*.c` 的 `unicode_list`），而**非**本文件的常量清单
// 或 `font_subset_charset.txt`（后者本身缺字：实测无 `天`，且列的 `✕`/`❚` 已被
// `lv_font_conv` 丢弃 —— B2a 规格评审 ③）。
// ═══════════════════════════════════════════════════════════════════════════

/// 通道断条（EDGE-03）。
pub const TEXT_CHANNEL_DOWN: &str = "与主进程数据通道断开";
/// 尚未首连成功（UI §3.6）。
pub const TEXT_CHANNEL_CONNECTING: &str = "正在连接数据通道";
/// 过期角标（PRD F5.3）。
pub const TEXT_STALE: &str = "数据过期";

/// SOC 区标题。
pub const TEXT_SOC_TITLE: &str = "储能电池 SOC";
/// SOC 单位（档位 L1-单位 56 px）。
pub const TEXT_SOC_UNIT: &str = "%";
/// 量程条左刻度。
pub const TEXT_SOC_SCALE_LOW: &str = "0";
/// 量程条右刻度。
pub const TEXT_SOC_SCALE_HIGH: &str = "100";
/// SOC 回落源胶囊（⚠️ 偏差 1：圆括号不在字体子集内）。
pub const TEXT_SOC_SRC_PCS: &str = "PCS REG1010";

/// PCS 区标题。
pub const TEXT_PCS_TITLE: &str = "PCS 运行状态";
/// 主判据标注（PRD F2.2：判据 REG 1013）。
pub const TEXT_PCS_JUDGE: &str = "REG 1013";
/// PCS 离线（EDGE-01）。**2 字**（`离线`）—— 见偏差登记 **D7**（PM 裁定：状态词槽位只放
/// 2 字，`PCS` 语义由卡头 `PCS 运行状态` + 图标承担；契约原文 `PCS 离线` 在 112 px 档下
/// 实测 458.1 px，远超词区 362 px，必被 `DOTS` 截断）。
pub const TEXT_PCS_OFFLINE: &str = "离线";
/// 无帧时的 PCS 态。**2 字**（`未知`）—— 同 D7（契约 `状态未知` = 448.0 px 亦超宽）；
/// `未` / `知` 均在字体 cmap 内。
pub const TEXT_PCS_UNKNOWN: &str = "未知";
/// 方向一致佐证（PRD F2.3）。
pub const TEXT_PCS_CONSISTENT: &str = "方向一致";
/// 方向不一致角标（EDGE-06，**唯一专属色** `#FF5CD0`）。
pub const TEXT_PCS_INCONSISTENT: &str = "方向不一致";
/// 总有功前缀（`ΣP`）。
pub const TEXT_SIGMA_PREFIX: &str = "ΣP";
/// 有功单位。
pub const TEXT_KW: &str = "kW";
/// 电流单位。
pub const TEXT_A: &str = "A";

/// 三相相标 A。
pub const TEXT_PHASE_A: &str = "A 相";
/// 三相相标 B。
pub const TEXT_PHASE_B: &str = "B 相";
/// 三相相标 C。
pub const TEXT_PHASE_C: &str = "C 相";
/// 总有功卡标（REG 1032）。
pub const TEXT_PHASE_TOTAL: &str = "总有功";
/// 相卡字段名（有功）。
pub const TEXT_ACTIVE_POWER: &str = "有功功率";
/// 总卡字段名。
pub const TEXT_DEVICE_TOTAL_POWER: &str = "设备总有功";
/// 相卡字段名（电流）。
pub const TEXT_CURRENT: &str = "电流";

/// 装置状态区标题（F6）。
pub const TEXT_DEVICE_TITLE: &str = "装置状态";
/// 装置状态卡：固件版本。
pub const TEXT_FIRMWARE: &str = "固件版本";
/// 装置状态卡：编译时间。
pub const TEXT_BUILD_TIME: &str = "编译时间";
/// 装置状态卡：运行时长。
pub const TEXT_UPTIME: &str = "运行时长";
/// 装置状态卡：CPU 温度。
pub const TEXT_CPU_TEMP: &str = "CPU 温度";
/// 装置状态卡：内存使用率。
pub const TEXT_MEM: &str = "内存使用率";
/// 装置状态卡：调度主站连接。
pub const TEXT_LINK_IEC104: &str = "调度主站连接";
/// 装置状态卡：核间连接。
pub const TEXT_LINK_INTERCORE: &str = "核间连接";
/// 装置状态卡：当前控制源。
pub const TEXT_CONTROL_SOURCE: &str = "当前控制源";
/// 百分比单位（内存使用率）。
pub const TEXT_PERCENT: &str = "%";
/// 控制源固定文案的字符集内变体（⚠️ 偏差 2：原串 `AI 引擎已停用，本地策略引擎为默认下发源`
/// 含**全角逗号**与 **`为`** —— 两者都不在 §3.6 字符集 / 字体子集内 ⇒ 取 `·` 分隔并改述为
/// 「默认下发」。语义（AI 停用、本地策略引擎为默认下发源）不变）。
pub const TEXT_AI_DISABLED: &str = "AI 引擎已停用 · 本地策略引擎默认下发";

/// 告警区标题（F7）。
pub const TEXT_ALARM_TITLE: &str = "最近告警";
/// 告警表头：时间列。
pub const TEXT_ALARM_TIME: &str = "时间";
/// 告警表头：级别列。
pub const TEXT_ALARM_LEVEL: &str = "级别";
/// 告警表头：消息列。
pub const TEXT_ALARM_MESSAGE: &str = "消息";
/// 告警空态（**确实无告警**）。
pub const TEXT_ALARM_EMPTY: &str = "近 24 小时无告警";
/// 级别文字：严重（ERROR）。
pub const TEXT_LEVEL_ERROR: &str = "严重";
/// 级别文字：警告（WARN）。
pub const TEXT_LEVEL_WARN: &str = "警告";
/// 级别文字：提示（INFO）。
pub const TEXT_LEVEL_INFO: &str = "提示";

// ═══════════════════════════════════════════════════════════════════════════
// 1. 栅格常量（UI §6.1；全部由 theme 常量推导）
// ═══════════════════════════════════════════════════════════════════════════

/// 卡内容区原点相对卡**外缘**的偏移 = 描边 1 + 内边距 16（`theme::card()` 的样式）。
const CARD_INSET: i32 = theme::Stroke::THIN + Dimens::GAP_MIN;
/// 卡描边宽。
const CARD_BORDER: i32 = theme::Stroke::THIN;
/// 页顶上内边距（UI §3.5）。
const PAD_TOP: i32 = Dimens::CONTENT_PAD_TOP;
/// 通道条高（= `StatusChip` 高 32）。
const STRIP_H: i32 = Dimens::STATUS_CHIP_H;
/// 主内容起点 y。
const CONTENT_TOP: i32 = PAD_TOP + STRIP_H + Dimens::GAP_GROUP;
/// 主行卡宽 = (992 − 24) / 2 = 484。
const MAIN_CARD_W: i32 = (Dimens::CONTENT_W - Dimens::GAP_SECTION) / 2;
/// 主行卡内可用宽。
const MAIN_CARD_INNER_W: i32 = MAIN_CARD_W - 2 * CARD_INSET;
/// 量程条高 = 状态点 12 + 缝 8 = 20。
const SOC_BAR_H: i32 = Dimens::STATUS_DOT + Dimens::GAP_MIN / 2;
/// 紧凑缝（4 px）。
const TIGHT_GAP: i32 = Dimens::SCROLLBAR_MARGIN;
/// **主行卡高 = UI §6.1 的契约值 `484×320` 之 320**（B2a 规格评审 ②：此前按"内容区高 + 上下
/// （描边 + 内边距）"倒推出 294，比契约矮 26 px，把三相 / 装置 / 告警各区块整体上移）。
/// 契约值是**给定值**，不是从内容反推的结果 —— 内部排布按它**留白**（见
/// [`MAIN_CARD_BODY_H`] 与各 y 常量），**不得**反过来缩卡高。
const MAIN_CARD_H: i32 = 320;
/// 主行卡**内容区**高 = 卡高 − 上下（描边 1 + 内边距 16）= 320 − 34 = **286**。
/// 卡内各件在此高度内排布（顶行贴顶、尾行**贴底**，中缝吸收余量）。
const MAIN_CARD_BODY_H: i32 = MAIN_CARD_H - 2 * CARD_INSET;
/// SOC 数值行 y。
const SOC_VALUE_Y: i32 = STRIP_H + Dimens::GAP_GROUP;
/// SOC 数值槽宽 = 2 × 148 = 296（容纳 3 位等宽数字；薄层无字体度量 API，见报告）。
const SOC_VALUE_SLOT_W: i32 = 2 * TextSlot::SocValue.px() as i32;
/// SOC 源胶囊宽。
const SOC_CHIP_W: i32 = 2 * Dimens::CHIP_MIN_W;
/// 量程条 y —— **贴底**排布：其下依次为刻度行（缝 [`TIGHT_GAP`]）与卡内下边距，
/// 即 `[量程条 20] + 4 + [刻度 24]` 恰好抵到内容区底缘（= 286）。
const SOC_BAR_Y: i32 =
    MAIN_CARD_BODY_H - SOC_BAR_H - TIGHT_GAP - TextSlot::Body.px() as i32;
/// 量程条当前值竖刻线宽（UI §6.1「4 px 高亮」）。
const SOC_MARKER_W: i32 = Dimens::ACCENT_BAR;
/// 刻度行 y。
const SOC_SCALE_Y: i32 = SOC_BAR_Y + SOC_BAR_H + TIGHT_GAP;
/// 刻度文字占位宽。
const SOC_SCALE_LABEL_W: i32 = Dimens::TOUCH_MIN;
// SOC 区间阈值（%）：**单一真源在状态层**（[`crate::state::SOC_LOW_PCT`] /
// [`crate::state::SOC_HIGH_PCT`]，与 `state::soc_band` 的数值着色同源）。此处直接引用，
// 不再另抄一份字面量（B2a 规格评审 Minor ⑤ 的双份真源）。

/// PCS 状态图标槽（UI §6.1「图标 72 px」）。
const PCS_ICON: i32 = Dimens::ICON_XL;
/// PCS 状态行 y。
const PCS_STATE_Y: i32 = STRIP_H + Dimens::GAP_GROUP;
/// PCS 状态行高（= 状态词 112）。
const PCS_STATE_H: i32 = TextSlot::PcsState.px() as i32;
/// PCS 判据标注占位宽（`REG 1013`）。
const PCS_JUDGE_W: i32 = Dimens::BTN_MIN_W;
/// PCS 佐证行 y —— **贴底**排布（行内最高件是 `StatusChip`，高
/// [`Dimens::STATUS_CHIP_H`]；UI §6.1 的佐证行 y360 亦在卡内下段）。
const PCS_CONSIST_Y: i32 = MAIN_CARD_BODY_H - Dimens::STATUS_CHIP_H;
/// 方向不一致胶囊宽。
const PCS_INCONSIST_CHIP_W: i32 = 2 * Dimens::CHIP_MIN_W;
/// `ΣP` 值文本 x（佐证行内）。
const PCS_SIGMA_X: i32 = PCS_INCONSIST_CHIP_W + Dimens::GAP_MIN;

/// 三卡行 y。
const PHASE_Y: i32 = CONTENT_TOP + MAIN_CARD_H + Dimens::GAP_GROUP;
/// 相卡宽 = (992 − 3 × 16) / 4 = 236。
const PHASE_CARD_W: i32 = (Dimens::CONTENT_W - 3 * Dimens::GAP_GROUP) / 4;
/// 相卡内容区宽。
const PHASE_INNER_W: i32 = PHASE_CARD_W - 2 * CARD_INSET;
/// 相卡卡头高（= 状态胶囊 32；降级原因胶囊落在这里）。
const PHASE_HEAD_H: i32 = Dimens::STATUS_CHIP_H;
/// 降级原因胶囊宽。
const PHASE_CHIP_W: i32 = Dimens::CHIP_MIN_W + Dimens::GAP_MIN;
/// 「有功功率」标签 y。
const PHASE_P_LABEL_Y: i32 = PHASE_HEAD_H + TIGHT_GAP;
/// P 数值 y。
const PHASE_P_Y: i32 = PHASE_P_LABEL_Y + TextSlot::Body.px() as i32;
/// 「电流」标签 y。
const PHASE_I_LABEL_Y: i32 = PHASE_P_Y + TextSlot::PhasePower.px() as i32 + TIGHT_GAP;
/// I 数值 y。
const PHASE_I_Y: i32 = PHASE_I_LABEL_Y + TextSlot::Body.px() as i32;
/// 相卡内容区高。
const PHASE_BODY_H: i32 = PHASE_I_Y + TextSlot::PhaseCurrent.px() as i32;
/// 相卡高。
const PHASE_CARD_H: i32 = PHASE_BODY_H + 2 * CARD_INSET;
/// P 数值槽宽（2 × 64 = 128）。
const PHASE_P_SLOT_W: i32 = 2 * TextSlot::PhasePower.px() as i32;
/// I 数值槽宽（2 × 48 = 96）。
const PHASE_I_SLOT_W: i32 = 2 * TextSlot::PhaseCurrent.px() as i32;
/// 相卡方向箭头 x（= P 数值槽宽 + 跨区缝 24）。
const PHASE_ARROW_X: i32 = PHASE_P_SLOT_W + Dimens::GAP_SECTION;
/// 方向箭头字号（UI §6.1「箭头 28 px」）。
const PHASE_ARROW_PX: i32 = Dimens::ICON_SM;

/// 装置状态区标题 y。
const DEVICE_TITLE_Y: i32 = PHASE_Y + PHASE_CARD_H + Dimens::GAP_SECTION;
/// 装置状态卡文本列 x（图标 28 + 缝 8 = 36）。
const DEVICE_TEXT_X: i32 = Dimens::ICON_SM + Dimens::GAP_MIN / 2;
/// 装置状态卡第一行 y。
const DEVICE_ROW1_Y: i32 = DEVICE_TITLE_Y + TextSlot::SectionTitle.px() as i32 + Dimens::GAP_GROUP;
/// 装置状态卡第二行 y。
const DEVICE_ROW2_Y: i32 = DEVICE_ROW1_Y + Dimens::CARD_STATUS_H + Dimens::GAP_GROUP;
/// 装置卡内容区宽。
const DEVICE_INNER_W: i32 = Dimens::CARD_STATUS_W - 2 * CARD_INSET;
/// 装置卡内容区高。
const DEVICE_INNER_H: i32 = Dimens::CARD_STATUS_H - 2 * CARD_INSET;
/// 连接类卡片的指示灯宽。
const DEVICE_LED_W: i32 = DEVICE_INNER_W - DEVICE_TEXT_X;

/// 告警区标题 y。
const ALARM_TITLE_Y: i32 = DEVICE_ROW2_Y + Dimens::CARD_STATUS_H + Dimens::GAP_SECTION;
/// 告警卡 y。
const ALARM_CARD_Y: i32 = ALARM_TITLE_Y + TextSlot::SectionTitle.px() as i32 + Dimens::GAP_GROUP;
/// 告警表头高（UI §6.1「表头 y980–1016」= 36）。
const ALARM_HEAD_H: i32 = Dimens::ROW_LOG_H - Dimens::GAP_MIN / 2;
/// 告警最多行数（PRD F7.1）。
const ALARM_MAX_ROWS: usize = 10;
/// 告警时间列宽（内容宽 / 4 = 248）。
const ALARM_TIME_COL_W: i32 = Dimens::CONTENT_W / 4;
/// 告警级别列宽（= 次按钮最小宽 120）。
const ALARM_LEVEL_COL_W: i32 = Dimens::BTN_MIN_W;
/// 告警消息列 x。
const ALARM_MSG_COL_X: i32 = ALARM_TIME_COL_W + ALARM_LEVEL_COL_W;
/// 告警级别色块宽。
const ALARM_LEVEL_BLOCK_W: i32 = Dimens::ACCENT_BAR;
/// 告警级别文字 x。
const ALARM_LEVEL_TEXT_X: i32 = ALARM_TIME_COL_W + ALARM_LEVEL_BLOCK_W + Dimens::GAP_MIN / 2;
/// 告警卡高 = 表头 + 10 行。
const ALARM_CARD_H: i32 = ALARM_HEAD_H + ALARM_MAX_ROWS as i32 * Dimens::ROW_LOG_H;
/// 告警卡内可用宽。
const ALARM_INNER_W: i32 = Dimens::CONTENT_W - 2 * CARD_INSET;

// ═══════════════════════════════════════════════════════════════════════════
// 2. 视图枚举（离屏断言口径）
// ═══════════════════════════════════════════════════════════════════════════

/// 告警区当前形态（**三态互斥**：行 / 空态 / 源不可用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmView {
    /// 有告警行（时间倒序）。
    Rows,
    /// 空态：**确实无告警**。
    Empty,
    /// 不可用：**无法获知有无告警**（EDGE-09，**不得**显「无告警」）。
    Unavailable,
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 子结构（每个字段都是**拥有型句柄**：不存住 ⇒ `drop` 会把对象从树上删掉）
// ═══════════════════════════════════════════════════════════════════════════

/// 相卡（A/B/C 或总）。
struct PhaseCard {
    /// 卡片根（`_` 前缀 = 仅作存活锚点，暂无读回需求）。
    _obj: Obj,
    /// 卡头标签（仅存活锚点）。
    _head: Label,
    dot: Obj,
    dot_style: Cell<usize>,
    reason_chip: Rc<StatusChip>,
    p_label: Label,
    p_value: Rc<Label>,
    _p_unit: Label,
    arrow: Label,
    arrow_style: Cell<usize>,
    _i_label: Label,
    i_value: Rc<Label>,
    _i_unit: Label,
    /// 是否为总卡（无电流行）。
    total: bool,
}

/// 装置状态卡（连接类用 5 个指示灯替代文本值）。
struct DeviceCard {
    /// 卡片根（存活锚点）。
    _obj: Obj,
    value: Rc<Label>,
    leds: Option<[Rc<LedIndicator>; 5]>,
}

/// 告警行。
struct AlarmRow {
    obj: Obj,
    time: Rc<Label>,
    level_block: Obj,
    level_block_style: Cell<usize>,
    level_text: Rc<Label>,
    level_text_style: Cell<usize>,
    message: Rc<Label>,
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 页面
// ═══════════════════════════════════════════════════════════════════════════

/// P1 主状态页（**只读**）。
///
/// 构造即建好**全部**控件（含各降级态的预留件）；[`P1StatusPage::render`] 只做
/// `set_text` / 颜色 / 可见性更新 —— **不重建对象**（设计 §5.6 控件策略「数值更新只调
/// `lv_label_set_text`」，也是"内存不随帧数增长"（NF-03）的前提）。
pub struct P1StatusPage {
    root: ScrollContainer,
    // ── 通道条 ──
    channel_down_chip: Rc<StatusChip>,
    channel_init_chip: Rc<StatusChip>,
    stale_chip: Rc<StatusChip>,
    // ── SOC 卡 ──
    soc_card: Obj,
    soc_top_alert: Obj,
    soc_value: Rc<Label>,
    soc_value_style: Cell<usize>,
    soc_unit: Label,
    soc_chips: [Rc<StatusChip>; 3],
    _soc_segments: [Obj; 3],
    soc_gray: Obj,
    soc_marker: Obj,
    // ── PCS 卡 ──
    /// PCS 卡根（**存活锚点**：卡被 `drop` 会级联删掉卡内全部子对象 —— 见 `_keep` 注）。
    _pcs_card: Obj,
    pcs_icon: Rc<Label>,
    pcs_icon_style: Cell<usize>,
    pcs_state: Rc<Label>,
    pcs_state_style: Cell<usize>,
    pcs_bar_top: Obj,
    pcs_bar_top_style: Cell<usize>,
    pcs_bar_left: Obj,
    pcs_bar_left_style: Cell<usize>,
    pcs_consistent: Label,
    pcs_inconsistent: Rc<StatusChip>,
    pcs_sigma: Rc<Label>,
    // ── 三相四卡 / 装置网格 / 告警行 ──
    phases: Vec<PhaseCard>,
    devices: Vec<DeviceCard>,
    alarm_card: Obj,
    alarm_empty: Rc<EmptyState>,
    alarm_unavailable: Rc<UnavailableState>,
    alarm_rows: Vec<AlarmRow>,
    /// 静态子树（标题 / 表头 / 刻度…）的**存活锚点**：LVGL 的对象是拥有型句柄，
    /// 句柄 `drop` 即 `lv_obj_delete`（连子对象一起），故所有不需读回的静态件都存这里。
    _keep: Vec<Obj>,
    // ── 缓存样式（**只建一次**：避免每帧 new Style 与无界挂样式）──
    soc_styles: Vec<Rc<Style>>,
    pcs_state_styles: Vec<Rc<Style>>,
    pcs_icon_styles: Vec<Rc<Style>>,
    pcs_bar_styles: Vec<Rc<Style>>,
    dot_styles: Vec<Rc<Style>>,
    arrow_styles: Vec<Rc<Style>>,
    level_text_styles: Vec<Rc<Style>>,
    level_block_styles: Vec<Rc<Style>>,
    // ── 渲染期状态（断言口径）──
    soc_color: Cell<Color>,
    pcs_color: Cell<Color>,
    alarm_view: Cell<AlarmView>,
    device_texts: RefCell<Vec<Option<String>>>,
}

impl P1StatusPage {
    /// 在 `parent` 下建页（`parent` 由 B2c 的页容器给出；页根尺寸/位置见 `pages` 模块文档）。
    pub fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = page_root(parent)?;
        let mut keep: Vec<Obj> = Vec::new();

        // ── 样式缓存（只建一次）──
        let soc_styles = vec![
            theme::text(TextSlot::SocValue, Palette::DANGER),      // 0 = ≤15 %
            theme::text(TextSlot::SocValue, Palette::SOC_OK),      // 1 = 15–85 %
            theme::text(TextSlot::SocValue, Palette::SOC_HIGH),    // 2 = ≥85 %
            theme::text(TextSlot::SocValue, Palette::PLACEHOLDER), // 3 = 降级
        ];
        let pcs_state_styles = vec![
            theme::text(TextSlot::PcsState, Palette::OK),      // 0 = 充电
            theme::text(TextSlot::PcsState, Palette::INFO),    // 1 = 放电
            theme::text(TextSlot::PcsState, Palette::STOPPED), // 2 = 停机 / 离线 / 未知
            theme::text(TextSlot::PcsState, Palette::STANDBY), // 3 = 待机
        ];
        let pcs_icon_styles = vec![
            theme::icon(PCS_ICON, Palette::OK),
            theme::icon(PCS_ICON, Palette::INFO),
            theme::icon(PCS_ICON, Palette::STOPPED),
            theme::icon(PCS_ICON, Palette::STANDBY),
        ];
        let pcs_bar_styles = vec![
            theme::card_head_bar(Palette::OK),
            theme::card_head_bar(Palette::INFO),
            theme::card_head_bar(Palette::STOPPED),
            theme::card_head_bar(Palette::STANDBY),
        ];
        let dot_styles = vec![
            theme::card_head_bar(Palette::SOC_OK),        // 0 = 实时
            theme::card_head_bar(Palette::BORDER_CTRL),   // 1 = 停更
            theme::card_head_bar(Palette::STALE),         // 2 = 过期
        ];
        let arrow_styles = vec![
            theme::icon(PHASE_ARROW_PX, Palette::OK),
            theme::icon(PHASE_ARROW_PX, Palette::INFO),
        ];
        let level_text_styles = vec![
            theme::text(TextSlot::Body, Palette::LOG_ERROR),
            theme::text(TextSlot::Body, Palette::LOG_WARN),
            theme::text(TextSlot::Body, Palette::LOG_INFO),
        ];
        let level_block_styles = vec![
            theme::card_head_bar(Palette::LOG_ERROR),
            theme::card_head_bar(Palette::LOG_WARN),
            theme::card_head_bar(Palette::LOG_INFO),
        ];

        // ── 通道条（页顶；仅在 Down / Init / Stale 时可见）──
        let channel_down_chip = Rc::new(StatusChip::new(
            &root,
            3 * Dimens::CHIP_MIN_W,
            "!",
            TEXT_CHANNEL_DOWN,
            ChipSkin::WARNING,
        )?);
        channel_down_chip.obj().set_pos(0, PAD_TOP);
        let channel_init_chip = Rc::new(StatusChip::new(
            &root,
            3 * Dimens::CHIP_MIN_W,
            "?",
            TEXT_CHANNEL_CONNECTING,
            ChipSkin::MISSING_DATA,
        )?);
        channel_init_chip.obj().set_pos(0, PAD_TOP);
        let stale_chip = Rc::new(StatusChip::new(
            &root,
            2 * Dimens::CHIP_MIN_W,
            "!",
            TEXT_STALE,
            ChipSkin::WARNING,
        )?);
        stale_chip
            .obj()
            .set_pos(3 * Dimens::CHIP_MIN_W + Dimens::GAP_MIN, PAD_TOP);

        // ── SOC 卡 ──
        let soc_card = decor(&root, MAIN_CARD_W, MAIN_CARD_H, &theme::card())?;
        soc_card.set_pos(0, CONTENT_TOP);
        // 卡顶 3 px 警示描边（EDGE-02）：与外缘齐平 ⇒ 反向抵消描边 + 内边距。
        let soc_top_alert = decor(
            &soc_card,
            MAIN_CARD_W - 2 * CARD_BORDER,
            theme::Stroke::ALERT,
            &theme::card_head_bar(Palette::DANGER),
        )?;
        soc_top_alert.set_pos(-CARD_INSET, -CARD_INSET);
        set_visible(&soc_top_alert, false);

        let soc_title = text_label(
            &soc_card,
            TEXT_SOC_TITLE,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        soc_title.set_pos(
            0,
            theme::center_offset(STRIP_H, TextSlot::SectionTitle.px() as i32),
        );
        keep.push(soc_title.into_obj());

        let soc_chips = [
            Rc::new(StatusChip::new(
                &soc_card,
                SOC_CHIP_W,
                "●",
                SocSource::Bms.display_name(),
                ChipSkin::NEUTRAL,
            )?),
            Rc::new(StatusChip::new(
                &soc_card,
                SOC_CHIP_W,
                "●",
                TEXT_SOC_SRC_PCS,
                ChipSkin::NEUTRAL,
            )?),
            Rc::new(StatusChip::new(
                &soc_card,
                SOC_CHIP_W,
                "!",
                SocSource::Lost.display_name(),
                ChipSkin::FAILURE,
            )?),
        ];
        for c in &soc_chips {
            c.obj().set_pos(MAIN_CARD_INNER_W - SOC_CHIP_W, 0);
            set_visible(c.obj(), false);
        }

        let value_row = layout_box(&soc_card, MAIN_CARD_INNER_W, TextSlot::SocValue.px() as i32)?;
        value_row.set_pos(0, SOC_VALUE_Y);
        let value_slot = layout_box(&value_row, SOC_VALUE_SLOT_W, TextSlot::SocValue.px() as i32)?;
        value_slot.center();
        let soc_value = Rc::new(label(&value_slot, TextSlot::SocValue, Palette::PLACEHOLDER)?);
        soc_value.set_text(PLACEHOLDER);
        soc_value.center();
        let soc_unit = text_label(&value_row, TEXT_SOC_UNIT, TextSlot::Unit, Palette::TEXT_SECOND)?;
        // 单位紧贴数值槽右缘（槽已水平居中于行内）。
        soc_unit.set_pos(
            (MAIN_CARD_INNER_W - SOC_VALUE_SLOT_W) / 2 + SOC_VALUE_SLOT_W + Dimens::GAP_MIN,
            theme::center_offset(TextSlot::SocValue.px() as i32, TextSlot::Unit.px() as i32),
        );
        keep.push(value_row);
        keep.push(value_slot);

        // 三段量程条（0–15 % 红 / 15–85 % 青 / 85–100 % 橙）——用三个并列色块表达
        // （`Style` 机制层无渐变通道，见 B2a 报告）。
        let seg_low_w = MAIN_CARD_INNER_W * state::SOC_LOW_PCT / 100;
        let seg_high_w = MAIN_CARD_INNER_W * (100 - state::SOC_HIGH_PCT) / 100;
        let seg_mid_w = MAIN_CARD_INNER_W - seg_low_w - seg_high_w;
        let seg_low = decor(&soc_card, seg_low_w, SOC_BAR_H, &theme::card_head_bar(Palette::DANGER))?;
        seg_low.set_pos(0, SOC_BAR_Y);
        let seg_mid = decor(&soc_card, seg_mid_w, SOC_BAR_H, &theme::card_head_bar(Palette::SOC_OK))?;
        seg_mid.set_pos(seg_low_w, SOC_BAR_Y);
        let seg_high = decor(&soc_card, seg_high_w, SOC_BAR_H, &theme::card_head_bar(Palette::SOC_HIGH))?;
        seg_high.set_pos(seg_low_w + seg_mid_w, SOC_BAR_Y);
        // 灰化覆盖（EDGE-02：SOC 双源皆失 ⇒ 量程条整体灰化）。**最后建** ⇒ 覆盖在三段之上。
        let soc_gray = decor(
            &soc_card,
            MAIN_CARD_INNER_W,
            SOC_BAR_H,
            &theme::card_head_bar(Palette::STOPPED),
        )?;
        soc_gray.set_pos(0, SOC_BAR_Y);
        set_visible(&soc_gray, false);
        let soc_marker = decor(
            &soc_card,
            SOC_MARKER_W,
            SOC_BAR_H,
            &theme::card_head_bar(Palette::TEXT_PRIMARY),
        )?;
        soc_marker.set_pos(0, SOC_BAR_Y);
        set_visible(&soc_marker, false);

        let scale_low = text_label(&soc_card, TEXT_SOC_SCALE_LOW, TextSlot::Weak, Palette::TEXT_WEAK)?;
        scale_low.set_pos(0, SOC_SCALE_Y);
        let scale_high = text_label(&soc_card, TEXT_SOC_SCALE_HIGH, TextSlot::Weak, Palette::TEXT_WEAK)?;
        scale_high.set_pos(MAIN_CARD_INNER_W - SOC_SCALE_LABEL_W, SOC_SCALE_Y);
        keep.push(scale_low.into_obj());
        keep.push(scale_high.into_obj());

        // ── PCS 卡 ──
        let pcs_card = decor(&root, MAIN_CARD_W, MAIN_CARD_H, &theme::card())?;
        pcs_card.set_pos(MAIN_CARD_W + Dimens::GAP_SECTION, CONTENT_TOP);
        let pcs_title = text_label(
            &pcs_card,
            TEXT_PCS_TITLE,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        pcs_title.set_pos(
            0,
            theme::center_offset(STRIP_H, TextSlot::SectionTitle.px() as i32),
        );
        let pcs_judge = text_label(&pcs_card, TEXT_PCS_JUDGE, TextSlot::Weak, Palette::TEXT_WEAK)?;
        pcs_judge.set_size(PCS_JUDGE_W, TextSlot::Body.px() as i32);
        pcs_judge.set_long_mode(LongMode::DOTS);
        pcs_judge.set_pos(MAIN_CARD_INNER_W - PCS_JUDGE_W, TIGHT_GAP);
        keep.push(pcs_title.into_obj());
        keep.push(pcs_judge.into_obj());

        // 卡顶 3 px + 卡左 6 px 语义色条（UI §6.1）——用独立装饰块而非卡片自身描边：
        // LVGL 的 `border_*` 是一组单值属性，两条不同 `border_side` 的样式叠加只会
        // "后者全胜"，做不到"顶 + 左"同时成立。
        let pcs_bar_top = decor(
            &pcs_card,
            MAIN_CARD_W - 2 * CARD_BORDER,
            theme::Stroke::ALERT,
            &pcs_bar_styles[2],
        )?;
        pcs_bar_top.set_pos(-CARD_INSET, -CARD_INSET);
        let pcs_bar_left = decor(
            &pcs_card,
            Dimens::INTERLOCK_BAR,
            MAIN_CARD_H - 2 * CARD_BORDER - theme::Stroke::ALERT,
            &pcs_bar_styles[2],
        )?;
        pcs_bar_left.set_pos(-CARD_INSET, -CARD_INSET + theme::Stroke::ALERT);

        let pcs_icon = Rc::new(label(&pcs_card, theme::icon_slot(PCS_ICON), Palette::STOPPED)?);
        pcs_icon.set_size(PCS_ICON, PCS_ICON);
        pcs_icon.set_text("?");
        pcs_icon.set_pos(0, PCS_STATE_Y + theme::center_offset(PCS_STATE_H, PCS_ICON));
        let pcs_state = Rc::new(label(&pcs_card, TextSlot::PcsState, Palette::STOPPED)?);
        pcs_state.set_text(TEXT_PCS_UNKNOWN);
        pcs_state.set_size(MAIN_CARD_INNER_W - PCS_ICON - Dimens::GAP_MIN, PCS_STATE_H);
        pcs_state.set_long_mode(LongMode::DOTS);
        pcs_state.set_pos(PCS_ICON + Dimens::GAP_MIN, PCS_STATE_Y);

        let pcs_consistent = text_label(
            &pcs_card,
            TEXT_PCS_CONSISTENT,
            TextSlot::Body,
            Palette::TEXT_SECOND,
        )?;
        pcs_consistent.set_pos(0, PCS_CONSIST_Y + TIGHT_GAP);
        let pcs_inconsistent = Rc::new(StatusChip::new(
            &pcs_card,
            PCS_INCONSIST_CHIP_W,
            "!",
            TEXT_PCS_INCONSISTENT,
            ChipSkin::INCONSISTENT,
        )?);
        pcs_inconsistent.obj().set_pos(0, PCS_CONSIST_Y);
        set_visible(pcs_inconsistent.obj(), false);
        let pcs_sigma = Rc::new(label(&pcs_card, TextSlot::Body, Palette::TEXT_SECOND)?);
        pcs_sigma.set_text(TEXT_SIGMA_PREFIX);
        pcs_sigma.set_pos(PCS_SIGMA_X, PCS_CONSIST_Y + TIGHT_GAP);

        // ── 三相四卡 ──
        let mut phases = Vec::with_capacity(4);
        for i in 0..4usize {
            let total = i == 3;
            let x = i as i32 * (PHASE_CARD_W + Dimens::GAP_GROUP);
            let card = decor(&root, PHASE_CARD_W, PHASE_CARD_H, &theme::card())?;
            card.set_pos(x, PHASE_Y);
            let head_text = match i {
                0 => TEXT_PHASE_A,
                1 => TEXT_PHASE_B,
                2 => TEXT_PHASE_C,
                _ => TEXT_PHASE_TOTAL,
            };
            let head = text_label(&card, head_text, TextSlot::SectionTitle, Palette::TEXT_PRIMARY)?;
            head.set_pos(
                0,
                theme::center_offset(PHASE_HEAD_H, TextSlot::SectionTitle.px() as i32),
            );
            let dot = decor(&card, Dimens::STATUS_DOT, Dimens::STATUS_DOT, &dot_styles[1])?;
            dot.set_pos(
                PHASE_INNER_W - Dimens::STATUS_DOT,
                theme::center_offset(PHASE_HEAD_H, Dimens::STATUS_DOT),
            );
            // 降级原因胶囊（F14 三通道：图标 + 文字 + 颜色；与状态点**互斥显示**）。
            let reason_chip = Rc::new(StatusChip::new(
                &card,
                PHASE_CHIP_W,
                "?",
                NOT_READ,
                ChipSkin::MISSING_DATA,
            )?);
            reason_chip.obj().set_pos(PHASE_INNER_W - PHASE_CHIP_W, 0);
            set_visible(reason_chip.obj(), false);

            let p_label = text_label(&card, TEXT_ACTIVE_POWER, TextSlot::Body, Palette::TEXT_SECOND)?;
            p_label.set_pos(0, PHASE_P_LABEL_Y);
            let p_value = Rc::new(label(&card, TextSlot::PhasePower, Palette::PLACEHOLDER)?);
            p_value.set_text(PLACEHOLDER);
            p_value.set_pos(0, PHASE_P_Y);
            let p_unit = text_label(&card, TEXT_KW, TextSlot::Body, Palette::TEXT_SECOND)?;
            p_unit.set_pos(
                PHASE_P_SLOT_W,
                PHASE_P_Y
                    + theme::center_offset(
                        TextSlot::PhasePower.px() as i32,
                        TextSlot::Body.px() as i32,
                    ),
            );
            let arrow = label(&card, theme::icon_slot(PHASE_ARROW_PX), Palette::OK)?;
            arrow.set_text("▲");
            arrow.set_size(PHASE_ARROW_PX, PHASE_ARROW_PX);
            arrow.set_pos(
                PHASE_ARROW_X,
                PHASE_P_Y
                    + theme::center_offset(TextSlot::PhasePower.px() as i32, PHASE_ARROW_PX),
            );
            set_visible(&arrow, false);

            let i_label = text_label(&card, TEXT_CURRENT, TextSlot::Body, Palette::TEXT_SECOND)?;
            i_label.set_pos(0, PHASE_I_LABEL_Y);
            let i_value = Rc::new(label(&card, TextSlot::PhaseCurrent, Palette::PLACEHOLDER)?);
            i_value.set_text(PLACEHOLDER);
            i_value.set_pos(0, PHASE_I_Y);
            let i_unit = text_label(&card, TEXT_A, TextSlot::Body, Palette::TEXT_SECOND)?;
            i_unit.set_pos(
                PHASE_I_SLOT_W,
                PHASE_I_Y
                    + theme::center_offset(
                        TextSlot::PhaseCurrent.px() as i32,
                        TextSlot::Body.px() as i32,
                    ),
            );
            set_visible(&i_unit, !total);

            phases.push(PhaseCard {
                _obj: card,
                _head: head,
                dot,
                dot_style: Cell::new(usize::MAX),
                reason_chip,
                p_label,
                p_value,
                _p_unit: p_unit,
                arrow,
                arrow_style: Cell::new(usize::MAX),
                _i_label: i_label,
                i_value,
                _i_unit: i_unit,
                total,
            });
        }

        // ── 装置状态网格（8 卡 4×2；UI §6.1）──
        let device_title = text_label(
            &root,
            TEXT_DEVICE_TITLE,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        device_title.set_pos(0, DEVICE_TITLE_Y);
        keep.push(device_title.into_obj());
        let device_names = [
            TEXT_FIRMWARE,
            TEXT_BUILD_TIME,
            TEXT_UPTIME,
            TEXT_CPU_TEMP,
            TEXT_MEM,
            TEXT_LINK_IEC104,
            TEXT_LINK_INTERCORE,
            TEXT_CONTROL_SOURCE,
        ];
        let mut devices = Vec::with_capacity(8);
        for (i, name) in device_names.iter().enumerate() {
            let x = (i % 4) as i32 * (Dimens::CARD_STATUS_W + Dimens::GAP_GROUP);
            let y = if i < 4 { DEVICE_ROW1_Y } else { DEVICE_ROW2_Y };
            let card = decor(&root, Dimens::CARD_STATUS_W, Dimens::CARD_STATUS_H, &theme::card())?;
            card.set_pos(x, y);
            let icon = text_label(&card, "●", theme::icon_slot(Dimens::ICON_SM), Palette::TEXT_WEAK)?;
            icon.set_size(Dimens::ICON_SM, Dimens::ICON_SM);
            icon.set_pos(0, theme::center_offset(DEVICE_INNER_H, Dimens::ICON_SM));
            let name_l = text_label(&card, name, TextSlot::Body, Palette::TEXT_SECOND)?;
            name_l.set_size(DEVICE_INNER_W - DEVICE_TEXT_X, TextSlot::Body.px() as i32);
            name_l.set_long_mode(LongMode::DOTS);
            name_l.set_pos(DEVICE_TEXT_X, 0);
            // 图标与字段名都是**静态件**（不随帧变），句柄必须存住 —— 否则本循环迭代结束时
            // `drop` 会把它们从卡片里删掉。
            keep.push(icon.into_obj());
            keep.push(name_l.into_obj());
            let value = Rc::new(label(&card, TextSlot::CardValue, Palette::TEXT_PRIMARY)?);
            value.set_text(PLACEHOLDER);
            value.set_size(DEVICE_INNER_W - DEVICE_TEXT_X, TextSlot::CardValue.px() as i32);
            value.set_long_mode(LongMode::DOTS);
            value.set_pos(DEVICE_TEXT_X, TextSlot::Body.px() as i32 + TIGHT_GAP);
            // 连接类卡片（调度主站 / 核间）：五态各一个 `LedIndicator`，只显示其一
            // （`Led` 的色是**控件字段**、无 `set_color` ⇒ 逐态预建，见 B2a 报告）。
            let leds = if matches!(i, 5 | 6) {
                let mut arr = Vec::with_capacity(LED_STATES.len());
                for st in LED_STATES {
                    let led = Rc::new(LedIndicator::new(
                        &card,
                        DEVICE_LED_W,
                        link_icon(st),
                        st.display_name(),
                        link_color(st),
                    )?);
                    led.obj()
                        .set_pos(DEVICE_TEXT_X, theme::center_offset(DEVICE_INNER_H, Dimens::ICON_SM));
                    set_visible(led.obj(), false);
                    arr.push(led);
                }
                let arr: [Rc<LedIndicator>; 5] = arr.try_into().expect("固定 5 态");
                set_visible(value.obj(), false);
                Some(arr)
            } else {
                None
            };
            devices.push(DeviceCard {
                _obj: card,
                value,
                leds,
            });
        }

        // ── 告警区（标题 + 卡：卡内首行即表头，其后 10 行）──
        let alarm_title = text_label(
            &root,
            TEXT_ALARM_TITLE,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        alarm_title.set_pos(0, ALARM_TITLE_Y);
        keep.push(alarm_title.into_obj());
        let alarm_card = decor(&root, Dimens::CONTENT_W, ALARM_CARD_H, &theme::card())?;
        alarm_card.set_pos(0, ALARM_CARD_Y);
        let head_y = theme::center_offset(ALARM_HEAD_H, TextSlot::Body.px() as i32);
        let head_time = text_label(&alarm_card, TEXT_ALARM_TIME, TextSlot::Body, Palette::TEXT_SECOND)?;
        head_time.set_pos(0, head_y);
        let head_level = text_label(&alarm_card, TEXT_ALARM_LEVEL, TextSlot::Body, Palette::TEXT_SECOND)?;
        head_level.set_pos(ALARM_TIME_COL_W, head_y);
        let head_msg = text_label(&alarm_card, TEXT_ALARM_MESSAGE, TextSlot::Body, Palette::TEXT_SECOND)?;
        head_msg.set_pos(ALARM_MSG_COL_X, head_y);
        keep.push(head_time.into_obj());
        keep.push(head_level.into_obj());
        keep.push(head_msg.into_obj());

        let mut alarm_rows = Vec::with_capacity(ALARM_MAX_ROWS);
        for i in 0..ALARM_MAX_ROWS {
            let y = ALARM_HEAD_H + i as i32 * Dimens::ROW_LOG_H;
            let row = layout_box(&alarm_card, ALARM_INNER_W, Dimens::ROW_LOG_H)?;
            row.set_pos(0, y);
            let text_y = theme::center_offset(Dimens::ROW_LOG_H, TextSlot::Body.px() as i32);
            let time = Rc::new(text_label(&row, "", TextSlot::Body, Palette::TEXT_WEAK)?);
            time.set_size(ALARM_TIME_COL_W - Dimens::GAP_MIN, TextSlot::Body.px() as i32);
            time.set_long_mode(LongMode::DOTS);
            time.set_pos(0, text_y);
            let level_block = decor(
                &row,
                ALARM_LEVEL_BLOCK_W,
                TextSlot::Body.px() as i32,
                &level_block_styles[2],
            )?;
            level_block.set_pos(ALARM_TIME_COL_W, text_y);
            let level_text = Rc::new(text_label(&row, "", TextSlot::Body, Palette::LOG_INFO)?);
            level_text.set_pos(ALARM_LEVEL_TEXT_X, text_y);
            let message = Rc::new(text_label(&row, "", TextSlot::Body, Palette::TEXT_SECOND)?);
            message.set_size(ALARM_INNER_W - ALARM_MSG_COL_X, TextSlot::Body.px() as i32);
            message.set_long_mode(LongMode::DOTS);
            message.set_pos(ALARM_MSG_COL_X, text_y);
            row.set_hidden(true);
            alarm_rows.push(AlarmRow {
                obj: row,
                time,
                level_block,
                level_block_style: Cell::new(usize::MAX),
                level_text,
                level_text_style: Cell::new(usize::MAX),
                message,
            });
        }

        // 空态 / 不可用态（互斥显示；落在列表区首行）。
        let alarm_empty = Rc::new(EmptyState::new(&alarm_card, "○", TEXT_ALARM_EMPTY)?);
        alarm_empty.set_pos(0, ALARM_HEAD_H + Dimens::ROW_LOG_H);
        set_visible(alarm_empty.obj(), false);
        let alarm_unavailable = Rc::new(UnavailableState::new(
            &alarm_card,
            UnavailableKind::AlertSource,
            TEXT_CHANNEL_DOWN,
        )?);
        alarm_unavailable.set_pos(0, ALARM_HEAD_H + Dimens::ROW_LOG_H);
        set_visible(alarm_unavailable.obj(), false);

        let page = Self {
            root,
            channel_down_chip,
            channel_init_chip,
            stale_chip,
            soc_card,
            soc_top_alert,
            soc_value,
            soc_value_style: Cell::new(usize::MAX),
            soc_unit,
            soc_chips,
            _soc_segments: [seg_low, seg_mid, seg_high],
            soc_gray,
            soc_marker,
            _pcs_card: pcs_card,
            pcs_icon,
            pcs_icon_style: Cell::new(usize::MAX),
            pcs_state,
            pcs_state_style: Cell::new(usize::MAX),
            pcs_bar_top,
            pcs_bar_top_style: Cell::new(usize::MAX),
            pcs_bar_left,
            pcs_bar_left_style: Cell::new(usize::MAX),
            pcs_consistent,
            pcs_inconsistent,
            pcs_sigma,
            phases,
            devices,
            alarm_card,
            alarm_empty,
            alarm_unavailable,
            alarm_rows,
            _keep: keep,
            soc_styles,
            pcs_state_styles,
            pcs_icon_styles,
            pcs_bar_styles,
            dot_styles,
            arrow_styles,
            level_text_styles,
            level_block_styles,
            soc_color: Cell::new(Palette::PLACEHOLDER),
            pcs_color: Cell::new(Palette::STOPPED),
            alarm_view: Cell::new(AlarmView::Unavailable),
            device_texts: RefCell::new(vec![None; 8]),
        };
        // 首帧渲染（占位态）——保证"建好即自洽"，不留给调用方一个半成品画面。
        page.render(&PageInput::init());
        Ok(page)
    }

    /// 渲染一帧（**只读**：不产生任何网络 / 写动作）。
    pub fn render(&self, input: &PageInput<'_>) {
        self.render_channel(input);
        self.render_soc(input);
        self.render_pcs(input);
        self.render_phases(input);
        self.render_device(input);
        self.render_alarms(input);
    }

    // ── 通道条 + 新鲜度（F5.3 / EDGE-03）──────────────────────────────────
    fn render_channel(&self, input: &PageInput<'_>) {
        let down = matches!(input.channel, ChannelStatus::Down);
        let init = matches!(input.channel, ChannelStatus::Init);
        set_visible(self.channel_down_chip.obj(), down);
        set_visible(self.channel_init_chip.obj(), init);
        // 「数据过期」只在**通道正常但帧旧**时打（通道断已由断连条覆盖，避免两条并存）。
        let stale = matches!(input.freshness, Freshness::Stale)
            && matches!(input.channel, ChannelStatus::Connected)
            && input.frame.is_some();
        set_visible(self.stale_chip.obj(), stale);
    }

    // ── SOC（F1）─────────────────────────────────────────────────────────
    fn render_soc(&self, input: &PageInput<'_>) {
        let frame = input.frame;
        let lost = match frame {
            Some(f) => f.soc.is_none() || f.soc_source == SocSource::Lost,
            None => true,
        };
        let value = frame.and_then(|f| f.soc).filter(|_| !lost);
        let src = frame.map(|f| f.soc_source).unwrap_or(SocSource::Lost);

        // 源胶囊（三态互斥；Lost 红胶囊）。
        let chip_idx = match src {
            SocSource::Bms => 0,
            SocSource::PcsReg1010 => 1,
            SocSource::Lost => 2,
        };
        show_only(
            &[
                self.soc_chips[0].obj(),
                self.soc_chips[1].obj(),
                self.soc_chips[2].obj(),
            ],
            Some(chip_idx),
        );

        // 数值 + 区间色（PRD F1.1 整数位；F1.3 区间着色）。
        let style_idx = match value {
            Some(v) => match state::soc_band(v) {
                SocBand::Low => 0,
                SocBand::Mid => 1,
                SocBand::High => 2,
            },
            None => 3,
        };
        self.soc_value.set_text(
            &value
                .map(|v| format!("{v:.0}"))
                .unwrap_or_else(|| PLACEHOLDER.to_string()),
        );
        set_style_index(
            self.soc_value.obj(),
            &self.soc_styles,
            &self.soc_value_style,
            style_idx,
        );
        self.soc_color.set(match style_idx {
            0 => Palette::DANGER,
            1 => Palette::SOC_OK,
            2 => Palette::SOC_HIGH,
            _ => Palette::PLACEHOLDER,
        });

        // 单位在降级态一并隐藏（避免出现「– %」这种读起来像有值的组合）。
        set_visible(&self.soc_unit, value.is_some());
        // 卡顶警示描边 + 量程条灰化（EDGE-02）。
        set_visible(&self.soc_top_alert, lost);
        set_visible(&self.soc_gray, lost);
        // 当前值竖刻线（仅正常态；位置按 0–100 线性映射）。
        match value {
            Some(v) => {
                let span = (MAIN_CARD_INNER_W - SOC_MARKER_W).max(1);
                let x = ((v.clamp(0.0, 100.0) / 100.0) * span as f64) as i32;
                self.soc_marker.set_pos(x, SOC_BAR_Y);
                set_visible(&self.soc_marker, true);
            }
            None => set_visible(&self.soc_marker, false),
        }
    }

    // ── PCS 四态（F2 / EDGE-01 / EDGE-06）────────────────────────────────
    fn render_pcs(&self, input: &PageInput<'_>) {
        let frame = input.frame;
        let state = frame
            .and_then(|f| f.run_state)
            .filter(|_| frame.is_some_and(|f| f.pcs_online));
        let style_idx = match state {
            Some(RunState::Charge) => 0,
            Some(RunState::Discharge) => 1,
            Some(RunState::Stop) => 2,
            Some(RunState::Standby) => 3,
            None => 2, // 停机灰：离线 / 未知共用（由文字区分）
        };
        let text = match state {
            Some(rs) => rs.display_name(),
            None if frame.is_some() => TEXT_PCS_OFFLINE,
            None => TEXT_PCS_UNKNOWN,
        };
        let icon = match state {
            Some(RunState::Charge) => "▼",
            Some(RunState::Discharge) => "▲",
            Some(RunState::Stop) => "■",
            // ⚠️ UI §6.1 的「❚❚待」用 U+275A，字体子集内无该字形 ⇒ 取 ○（见文件头）。
            Some(RunState::Standby) => "○",
            None => "?",
        };
        self.pcs_state.set_text(text);
        self.pcs_icon.set_text(icon);
        set_style_index(
            self.pcs_state.obj(),
            &self.pcs_state_styles,
            &self.pcs_state_style,
            style_idx,
        );
        set_style_index(
            self.pcs_icon.obj(),
            &self.pcs_icon_styles,
            &self.pcs_icon_style,
            style_idx,
        );
        set_style_index(
            &self.pcs_bar_top,
            &self.pcs_bar_styles,
            &self.pcs_bar_top_style,
            style_idx,
        );
        set_style_index(
            &self.pcs_bar_left,
            &self.pcs_bar_styles,
            &self.pcs_bar_left_style,
            style_idx,
        );
        self.pcs_color.set(match style_idx {
            0 => Palette::OK,
            1 => Palette::INFO,
            2 => Palette::STOPPED,
            _ => Palette::STANDBY,
        });

        // 佐证行：方向一致性（主判据仍是 REG 1013，角标不覆盖主状态 —— PRD F2.3）。
        let inconsistent = frame.is_some_and(|f| f.inconsistency) && state.is_some();
        set_visible(self.pcs_inconsistent.obj(), inconsistent);
        set_visible(&self.pcs_consistent, !inconsistent && state.is_some());
        // ΣP：取设备总有功（`p_total`），降级时显占位符。
        let sigma = frame
            .map(|f| NumView::from_field(&f.p_total))
            .unwrap_or(NumView::Dash(FieldFlag::NotRead));
        let sigma_text = match sigma.value() {
            Some(v) if v >= 0.0 => format!("{TEXT_SIGMA_PREFIX} +{v:.1} {TEXT_KW}"),
            Some(v) => format!("{TEXT_SIGMA_PREFIX} \u{2212}{:.1} {TEXT_KW}", -v),
            None => format!("{TEXT_SIGMA_PREFIX} {PLACEHOLDER}"),
        };
        self.pcs_sigma.set_text(&sigma_text);
        set_visible(&self.pcs_sigma, state.is_some() || inconsistent);
    }

    // ── 三相四卡（F3 / F4）──────────────────────────────────────────────
    fn render_phases(&self, input: &PageInput<'_>) {
        let frame = input.frame;
        let state = frame.and_then(|f| f.run_state);
        for (i, card) in self.phases.iter().enumerate() {
            // ⚠️ 索引边界：总卡（i = 3）取 `p_total`，**不得**索引 `p_phase[3]`
            //（`p_phase` 是 `[Field; 3]`，A/B/C 三相）。
            let pv = if card.total {
                frame
                    .map(|f| NumView::from_field(&f.p_total))
                    .unwrap_or(NumView::Dash(FieldFlag::NotRead))
            } else {
                frame
                    .map(|f| NumView::from_field(&f.p_phase[i]))
                    .unwrap_or(NumView::Dash(FieldFlag::NotRead))
            };
            let iv = if card.total {
                // 总卡（REG 1032）无电流量：恒为占位符（UI §6.1「总卡无电流行」）。
                NumView::Dash(FieldFlag::NotRead)
            } else {
                frame
                    .map(|f| NumView::from_field(&f.i_phase[i]))
                    .unwrap_or(NumView::Dash(FieldFlag::NotRead))
            };
            card.p_value.set_text(&num_text(&pv));
            if !card.total {
                card.i_value.set_text(&num_text(&iv));
            }
            // 该相卡的**整体可用性** = P 与 I 的合取（PRD F5.5「各字段独立降级」）。
            //
            // ⚠️ 此前只看 `pv`（有功）：某相 **I 单独缺失**（P 有效）时状态点仍显「实时」且无
            // 降级角标 —— 与 F5.5 不符（B2a 规格评审 ①）。总卡无电流行（`iv` 恒为
            // `Dash(NotRead)` 的**构造占位**，非真实缺失）⇒ 总卡只看 P。
            let degraded: Option<FieldFlag> = match &pv {
                NumView::Dash(f) => Some(*f),
                // 总卡无电流行 ⇒ 不把构造占位当降级；非总卡则把 I 一并纳入。
                NumView::Value(_) if card.total => None,
                NumView::Value(_) => match &iv {
                    NumView::Dash(f) => Some(*f),
                    NumView::Value(_) => None,
                },
            };
            // 降级原因（**点级独立降级**：单相失败不影响其余相；同相内 P/I 任一失败即降级）。
            match degraded {
                Some(flag) => {
                    card.reason_chip.set_text(state::dash_badge(flag));
                    set_visible(card.reason_chip.obj(), true);
                    // 字段名只随**该字段自身**是否降级隐藏（I 缺失不该把「有功功率」藏掉）。
                    set_visible(&card.p_label, pv.value().is_some());
                }
                None => {
                    set_visible(card.reason_chip.obj(), false);
                    set_visible(&card.p_label, true);
                }
            }
            // 状态点（UI §8.2：实时 / 停更 / 过期）——同样由**该相整体可用性**驱动。
            // `live_dot_for` 只看"该值是否降级 + 整帧新鲜度"，故把合取结果折回一个
            // `NumView`（降级 ⇒ `Dash(该原因)`；否则任一 `Value` 都是等价输入，其数值不被使用）。
            let dot_src = match degraded {
                Some(f) => NumView::Dash(f),
                None => NumView::Value(0.0),
            };
            let dot_idx = match state::live_dot_for(&dot_src, input.freshness) {
                LiveDot::Live => 0,
                LiveDot::Paused => 1,
                LiveDot::Expired => 2,
            };
            set_style_index(&card.dot, &self.dot_styles, &card.dot_style, dot_idx);
            // 方向箭头（色 = F2 语义色；仅充 / 放两态出现；总卡不画）。
            // **值降级时不画**：箭头是用来标注**这个数值**的方向的，值已是 `–` 时画箭头
            // 等于给"没有的读数"配方向（EDGE-01 的相卡只给 `–` + 「源离线」角标）。
            let arrow_idx = match (state, &pv) {
                (Some(RunState::Charge), NumView::Value(_)) if !card.total => Some(0),
                (Some(RunState::Discharge), NumView::Value(_)) if !card.total => Some(1),
                _ => None,
            };
            match arrow_idx {
                Some(idx) => {
                    card.arrow.set_text(if idx == 0 { "▼" } else { "▲" });
                    set_style_index(&card.arrow, &self.arrow_styles, &card.arrow_style, idx);
                    set_visible(&card.arrow, true);
                }
                None => set_visible(&card.arrow, false),
            }
        }
    }

    // ── 装置状态网格（F6）────────────────────────────────────────────────
    fn render_device(&self, input: &PageInput<'_>) {
        let (device, _alarms, info) = sections(input);
        // ⚠️ 两类缺失语义**不同**（设计 §3.1 的字段族差异）：
        // - `info` 段（版本 / 编译时间）是"该字段根本没有" ⇒ **「未提供」**（EDGE-16）；
        // - `device` 段（uptime / 温度 / 内存）是"这一拍没采到" ⇒ **「未取数」**。
        let texts: [String; 8] = [
            non_empty(&info.firmware_version).unwrap_or_else(|| MISSING.to_string()),
            info.build_time.clone().unwrap_or_else(|| MISSING.to_string()),
            device
                .uptime_secs
                .map(format_uptime)
                .unwrap_or_else(|| NOT_READ.to_string()),
            device
                .cpu_temp_c
                .map(|v| format!("{v:.0} C"))
                .unwrap_or_else(|| NOT_READ.to_string()),
            device
                .mem_used_pct
                .map(|v| format!("{v:.0} {TEXT_PERCENT}"))
                .unwrap_or_else(|| NOT_READ.to_string()),
            String::new(), // 连接类：由指示灯承担
            String::new(),
            control_source_text(device.control_source).to_string(),
        ];
        let mut out: Vec<Option<String>> = Vec::with_capacity(8);
        for (i, card) in self.devices.iter().enumerate() {
            match &card.leds {
                Some(leds) => {
                    let st = if i == 5 { device.iec104 } else { device.intercore };
                    let idx = LED_STATES.iter().position(|s| *s == st).unwrap_or(4);
                    let visible: Vec<&Obj> = leds.iter().map(|l| l.obj()).collect();
                    show_only(&visible, Some(idx));
                    out.push(Some(st.display_name().to_string()));
                }
                None => {
                    card.value.set_text(&texts[i]);
                    out.push(Some(texts[i].clone()));
                }
            }
        }
        *self.device_texts.borrow_mut() = out;
    }

    // ── 告警（F7 / EDGE-09）─────────────────────────────────────────────
    fn render_alarms(&self, input: &PageInput<'_>) {
        let (_device, alarms, _info) = sections(input);
        let view = if input.frame.is_none() || !alarms.available {
            AlarmView::Unavailable
        } else if alarms.items.is_empty() {
            AlarmView::Empty
        } else {
            AlarmView::Rows
        };
        self.alarm_view.set(view);
        match view {
            AlarmView::Unavailable => {
                for r in &self.alarm_rows {
                    r.obj.set_hidden(true);
                }
                set_visible(self.alarm_empty.obj(), false);
                set_visible(self.alarm_unavailable.obj(), true);
            }
            AlarmView::Empty => {
                for r in &self.alarm_rows {
                    r.obj.set_hidden(true);
                }
                set_visible(self.alarm_empty.obj(), true);
                set_visible(self.alarm_unavailable.obj(), false);
            }
            AlarmView::Rows => {
                set_visible(self.alarm_empty.obj(), false);
                set_visible(self.alarm_unavailable.obj(), false);
                for (i, row) in self.alarm_rows.iter().enumerate() {
                    match alarms.items.get(i) {
                        Some(item) => {
                            row.time.set_text(&format_epoch_ms_utc(item.ts_ms));
                            let lv = level_index(item.level);
                            row.level_text.set_text(level_text(item.level));
                            set_style_index(
                                row.level_text.obj(),
                                &self.level_text_styles,
                                &row.level_text_style,
                                lv,
                            );
                            set_style_index(
                                &row.level_block,
                                &self.level_block_styles,
                                &row.level_block_style,
                                lv,
                            );
                            row.message.set_text(&item.message);
                            row.obj.set_hidden(false);
                        }
                        None => row.obj.set_hidden(true),
                    }
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 只读断言口径（离屏用例读回；生产由 LVGL 渲染）
    // ═══════════════════════════════════════════════════════════════════════

    /// 页面根（滚动容器）。
    pub fn obj(&self) -> &Obj {
        &self.root
    }

    /// 通道条文案（可见时；`Down` 优先于 `Init`）。
    pub fn channel_text(&self) -> Option<String> {
        if !self.channel_down_chip.is_hidden() {
            self.channel_down_chip.text()
        } else if !self.channel_init_chip.is_hidden() {
            self.channel_init_chip.text()
        } else {
            None
        }
    }

    /// 「数据过期」角标是否可见（PRD F5.3）。
    pub fn stale_visible(&self) -> bool {
        !self.stale_chip.is_hidden()
    }

    /// SOC 主读数字面（降级态为 [`PLACEHOLDER`]）。
    pub fn soc_text(&self) -> Option<String> {
        self.soc_value.text()
    }

    /// SOC 当前区间色（`Palette::PLACEHOLDER` = 降级）。
    pub fn soc_color(&self) -> Color {
        self.soc_color.get()
    }

    /// 当前可见的 SOC 源胶囊文字（三态之一）。
    pub fn soc_source_text(&self) -> Option<String> {
        self.soc_chips
            .iter()
            .find(|c| !c.obj().is_hidden())
            .and_then(|c| c.text())
    }

    /// 当前可见的 SOC 源胶囊皮肤。
    pub fn soc_source_skin(&self) -> Option<ChipSkin> {
        self.soc_chips
            .iter()
            .find(|c| !c.obj().is_hidden())
            .map(|c| c.skin())
    }

    /// 量程条当前值竖刻线是否可见。
    pub fn soc_marker_visible(&self) -> bool {
        !self.soc_marker.is_hidden()
    }

    /// 量程条是否灰化（EDGE-02）。
    pub fn soc_gray_visible(&self) -> bool {
        !self.soc_gray.is_hidden()
    }

    /// PCS 状态词。
    pub fn pcs_state_text(&self) -> Option<String> {
        self.pcs_state.text()
    }

    /// PCS 状态图标字形。
    pub fn pcs_icon_text(&self) -> Option<String> {
        self.pcs_icon.text()
    }

    /// PCS 语义色（停机灰 = 离线 / 未知共用）。
    pub fn pcs_color(&self) -> Color {
        self.pcs_color.get()
    }

    /// 「方向不一致」角标是否可见（EDGE-06）。
    pub fn inconsistent_visible(&self) -> bool {
        !self.pcs_inconsistent.obj().is_hidden()
    }

    /// ΣP 佐证文本。
    pub fn sigma_text(&self) -> Option<String> {
        self.pcs_sigma.text()
    }

    /// 相卡 P 值字面。
    pub fn phase_p_text(&self, i: usize) -> Option<String> {
        self.phases.get(i).and_then(|c| c.p_value.text())
    }

    /// 相卡 I 值字面（总卡恒为占位符）。
    pub fn phase_i_text(&self, i: usize) -> Option<String> {
        self.phases.get(i).and_then(|c| c.i_value.text())
    }

    /// 相卡降级原因（可见时；否则 `None`）。
    pub fn phase_reason(&self, i: usize) -> Option<String> {
        let c = self.phases.get(i)?;
        if c.reason_chip.obj().is_hidden() {
            None
        } else {
            c.reason_chip.text()
        }
    }

    /// 相卡新鲜度点当前色（UI §8.2）。
    pub fn phase_dot_color(&self, i: usize) -> Color {
        match self.phases.get(i).map(|c| c.dot_style.get()) {
            Some(0) => Palette::SOC_OK,
            Some(1) => Palette::BORDER_CTRL,
            _ => Palette::STALE,
        }
    }

    /// 相卡方向箭头字形（不可见时 `None`）。
    pub fn phase_arrow(&self, i: usize) -> Option<String> {
        let c = self.phases.get(i)?;
        if c.arrow.is_hidden() {
            None
        } else {
            c.arrow.text()
        }
    }

    /// 装置状态卡值（连接类返回 `LinkState` 的文字态）。
    pub fn device_text(&self, i: usize) -> Option<String> {
        self.device_texts.borrow().get(i).cloned().flatten()
    }

    /// 装置状态卡数量（离屏断言用）。
    pub fn device_card_count(&self) -> usize {
        self.devices.len()
    }

    /// 告警区形态。
    pub fn alarm_view(&self) -> AlarmView {
        self.alarm_view.get()
    }

    /// 告警「不可用」态的标题（EDGE-09 口径断言）。
    pub fn alarm_unavailable_title(&self) -> Option<String> {
        self.alarm_unavailable.title()
    }

    /// 告警空态文案。
    pub fn alarm_empty_text(&self) -> Option<String> {
        self.alarm_empty.text()
    }

    /// 第 `i` 行告警的消息文本（不可见时为 `None`）。
    pub fn alarm_row_message(&self, i: usize) -> Option<String> {
        let r = self.alarm_rows.get(i)?;
        if r.obj.is_hidden() {
            None
        } else {
            r.message.text()
        }
    }

    /// 第 `i` 行告警的时间文本（UTC `YYYY/MM/DD HH:MM:SS`）。
    pub fn alarm_row_time(&self, i: usize) -> Option<String> {
        let r = self.alarm_rows.get(i)?;
        if r.obj.is_hidden() {
            None
        } else {
            r.time.text()
        }
    }

    /// SOC 卡对象（离屏断言坐标 / 尺寸用）。
    pub fn soc_card(&self) -> &Obj {
        &self.soc_card
    }

    /// 第 `i` 张相卡（0=A / 1=B / 2=C / 3=总）对象 —— 离屏断言"主卡高 320 后三相行落位"用
    /// （B2a 规格评审 ②）。
    pub fn phase_card_obj(&self, i: usize) -> Option<&Obj> {
        self.phases.get(i).map(|c| &c._obj)
    }

    /// 告警卡对象。
    pub fn alarm_card(&self) -> &Obj {
        &self.alarm_card
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 小工具
// ═══════════════════════════════════════════════════════════════════════════

/// 指示灯五态顺序（与 `LedIndicator` 数组一一对应；`Unknown` 兜底在末位）。
const LED_STATES: [LinkState; 5] = [
    LinkState::Connected,
    LinkState::Connecting,
    LinkState::Disconnected,
    LinkState::NotConfigured,
    LinkState::Unknown,
];

/// 链路态 → 灯色（PRD §3.1 指定色；`Unknown` 取"未配置"灰，**绝不落入"正常"绿**，
/// F6.5）。
fn link_color(state: LinkState) -> Color {
    match state {
        LinkState::Connected => Palette::LINK_OK,
        LinkState::Connecting => Palette::LINK_PENDING,
        LinkState::Disconnected => Palette::LINK_DOWN,
        LinkState::NotConfigured | LinkState::Unknown => Palette::LINK_UNCONFIGURED,
    }
}

/// 链路态 → 几何字形（F14 的"图标"通道）。
fn link_icon(state: LinkState) -> &'static str {
    match state {
        LinkState::Connected | LinkState::Connecting => "●",
        LinkState::Disconnected => "!",
        LinkState::NotConfigured | LinkState::Unknown => "○",
    }
}

/// 控制源文案（F6 备注的固定文案；⚠️ `AiDisabled` 取字符集内变体，见文件头偏差 2）。
fn control_source_text(src: ControlSource) -> &'static str {
    match src {
        ControlSource::LocalStrategy => ControlSource::LocalStrategy.display_name(),
        ControlSource::AiDisabled => TEXT_AI_DISABLED,
        ControlSource::Unknown => ControlSource::Unknown.display_name(),
    }
}

/// `NumView` → 显示字面（`Value` → 1 位小数；`Dash` → 占位符，**严禁补 0**）。
fn num_text(nv: &NumView) -> String {
    match nv.value() {
        Some(v) => format!("{v:.1}"),
        None => PLACEHOLDER.to_string(),
    }
}

/// 空串等价于"未提供"（EDGE-16 / F8.4）。
fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// 告警级别 → 样式下标（0/1/2 = 严重 / 警告 / 提示）。
fn level_index(level: AlarmLevel) -> usize {
    match level {
        AlarmLevel::Error => 0,
        AlarmLevel::Warn => 1,
        AlarmLevel::Info => 2,
    }
}

/// 告警级别 → 中文文字（UI §6.1「级别 = 色块 + 文字（严重 / 警告 / 提示）」；
/// ⚠️ 不用 `AlarmLevel::display_name()` —— 那是 P3 日志页的 `ERROR/WARN/INFO` 口径）。
fn level_text(level: AlarmLevel) -> &'static str {
    match level {
        AlarmLevel::Error => TEXT_LEVEL_ERROR,
        AlarmLevel::Warn => TEXT_LEVEL_WARN,
        AlarmLevel::Info => TEXT_LEVEL_INFO,
    }
}
