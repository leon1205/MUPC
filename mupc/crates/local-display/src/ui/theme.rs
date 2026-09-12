//! # `ui/theme.rs` —— 界面外观的**单一真源**（12-MUPC 工作单元 B1）
//!
//! 设计出处（`[DESIGN_APPROVED]` 设计 §5.1 / §5.6 / §5.6-A）：
//!
//! > `ui/theme.rs` —— 外观数值的**唯一真源**：色板/尺寸/字号常量 → 经 `style.rs` 机制施加
//! > `lv_style`（页面内不得硬编码裸色值/裸尺寸）。
//!
//! UI 设计文档为**数值权威**：色板 §3.2、字号阶梯 §3.3、栅格与装饰常量 §3.5、
//! 触摸尺寸 §2.1、确认强度 §2.5/§7.3、Toast §7.2、滚动规范 §7.4。
//!
//! ## 本模块只做三件事
//!
//! 1. **常量**：[`Dimens`] / [`Radius`] / [`Stroke`] / [`Palette`] / [`Timing`] / [`TextSlot`]
//!    —— 每个数值在整个 `ui/**` 里**只出现一次**，就在这里；
//! 2. **字号取用**：[`font_of`] / [`apply_font`] —— 10 档位图字体（§1.1.2）的统一入口，含
//!    `noto-font` 未启用时的**明确降级**（不是 panic）；
//! 3. **样式构造**：把上述常量施加为 `Rc<Style>`（如 [`card`] / [`button`] / [`chip`] /
//!    [`scrollbar_idle`]）—— 页面与 `components.rs` **只引用**这些构造函数，不拼裸值。
//!
//! ## 三条纪律（评审逐条检查）
//!
//! - **色值只走类型化通道**：本模块用 [`Color::hex`] 把 UI 文档的 `#RRGGBB` 逐位抄录成
//!   `Color` 常量；**绝不**调用裸色值 C 接口（设计 §11.4 静态约束 ④′ 会静态扫 `ui/**`）。
//! - **`Style` 先配置、再 `Rc::new`**：薄层的 `Style` 一旦经 `Rc` 共享即**冻结**
//!   （`&mut` 只在 `Rc::new` 之前存在，见 `lvgl/style.rs` 模块文档），故本模块的每个
//!   构造函数都是"建 → 配 → `Rc::new`"三步，返回**只读**句柄。
//! - **样式集按 (part, state) 拆分**：LVGL v9 的 `lv_style_set_*` 没有 selector 参数，
//!   一个 `Style` 只描述"某一组 (part, state) 下的取值"。故 UI §5.2 的「状态 × 色值矩阵」
//!   在此表达为**多个 `Style` + 挂载时的 [`StyleSelector`]**（见 [`ButtonStyles`]）。

use std::rc::Rc;
use std::time::Duration;

use crate::lvgl::font::{Font, FontSize};
use crate::lvgl::obj::Obj;
use crate::lvgl::style::{BorderSide, Color, Opa, State, Style, StyleSelector};

// ═══════════════════════════════════════════════════════════════════════════
// 1. 尺寸与间距（UI §2.1 / §3.5 / §5.1 / §7.2 / §7.3）
// ═══════════════════════════════════════════════════════════════════════════

/// 尺寸常量（单位 px；LVGL 的几何接口是 `i32`，故此处统一 `i32`）。
///
/// 触摸目标的四个**硬常量**（设计 §5.6 TT-08 行点名）：
/// [`Dimens::TOUCH_MIN`] / [`Dimens::TOUCH_CRITICAL`] / [`Dimens::GAP_MIN`] /
/// [`Dimens::GAP_DANGER`] —— **所有**可点控件必须显式引用它们，不得出现裸数值尺寸。
pub struct Dimens;

impl Dimens {
    /// 画布宽（与 crate 级 `SCREEN_W` 同一数值，**不重复定义**）。
    pub const SCREEN_W: i32 = crate::SCREEN_W as i32;
    /// 画布高（与 crate 级 `SCREEN_H` 同一数值）。
    pub const SCREEN_H: i32 = crate::SCREEN_H as i32;

    /// **最小触摸目标** 48×48 px（UI §2.1；所有可点控件下限）。
    pub const TOUCH_MIN: i32 = 48;
    /// **关键操作触摸目标** 64×64 px（UI §2.1：保存/恢复默认值/联锁释放/M1 授权/导航项/返回）。
    pub const TOUCH_CRITICAL: i32 = 64;
    /// **相邻可点控件最小间距** 16 px（UI §2.1）。
    pub const GAP_MIN: i32 = 16;
    /// **破坏性/生效性操作与「取消/返回」的最小间距** 48 px（UI §2.1，强制）。
    pub const GAP_DANGER: i32 = 48;

    /// 页眉高度（UI §3.5，y 0–72）。
    pub const HEADER_H: i32 = 72;
    /// 内容区视口高度（UI §3.5，y 72–696）。
    pub const CONTENT_H: i32 = 624;
    /// 底部导航高度（UI §3.5，y 696–768）。
    pub const NAV_H: i32 = 72;
    /// 内容区左右安全边（UI §3.5，x 16–1008；有效宽 992）。
    pub const SIDE_PAD: i32 = 16;
    /// 内容区有效宽（UI §3.5「有效宽 **992 px**」）。
    pub const CONTENT_W: i32 = Self::SCREEN_W - 2 * Self::SIDE_PAD;
    /// 内容区上内边距（UI §3.5）。
    pub const CONTENT_PAD_TOP: i32 = 8;
    /// 内容区下内边距（UI §3.5）。
    pub const CONTENT_PAD_BOTTOM: i32 = 24;
    /// 同组区块间呼吸缝（UI §3.5）。
    pub const GAP_GROUP: i32 = 16;
    /// 跨区区块间呼吸缝（UI §3.5）。
    pub const GAP_SECTION: i32 = 24;

    /// 滚动条 Visual 宽度（UI §3.5 `scrollbar_w`；**纯指示、非触摸目标**）。
    pub const SCROLLBAR_W: i32 = 8;
    /// 滚动条距右边缘（UI §3.5）。
    pub const SCROLLBAR_MARGIN: i32 = 4;

    /// 底部导航项宽（UI §5.1 #1：170.7×72 → 取整 170）。
    pub const NAV_ITEM_W: i32 = 170;
    /// 底部导航项高（UI §5.1 #1）。
    pub const NAV_ITEM_H: i32 = 72;
    /// 导航项顶部选中条高度（UI §4.2 / §5.3）。
    pub const NAV_SELECT_BAR_H: i32 = 4;

    /// 次按钮最小宽（UI §5.1 #2）。
    pub const BTN_MIN_W: i32 = 120;
    /// 主/关键按钮宽（UI §5.1 #2）。
    pub const BTN_MAIN_W: i32 = 200;
    /// 次按钮高（UI §5.1 #2）。
    pub const BTN_H_SECONDARY: i32 = 48;
    /// 主/关键按钮高（UI §5.1 #2）。
    pub const BTN_H_PRIMARY: i32 = 64;

    /// 步进器 `−`/`＋` 按钮宽（UI §5.1 #6）。
    pub const STEPPER_BTN_W: i32 = 64;
    /// 步进器值区宽（UI §5.1 #6：`−`64 + 值 120 + `+`64 = 248）。
    pub const STEPPER_VALUE_W: i32 = 120;
    /// 步进器高（UI §5.1 #6）。
    pub const STEPPER_H: i32 = 64;
    /// IPv4 四段步进值区宽（UI §5.1 #7）。
    pub const IPV4_VALUE_W: i32 = 64;
    /// IPv4 段间间距（UI §5.1 #7）。
    pub const IPV4_GAP: i32 = 8;
    /// 日期时间五列步进的列宽（UI §5.1 #8：5 列 × 112 + 4×8 = 592）。
    pub const DATETIME_COL_W: i32 = 112;
    /// 日期时间列间间距（UI §5.1 #8）。
    pub const DATETIME_GAP: i32 = 8;

    /// 多选 Chip 高度（UI §5.1 #5「Chip 高 48」）。
    pub const CHIP_H: i32 = 48;
    /// 多选 Chip 最小宽度（UI §5.1 #5「最小宽 96」）。
    pub const CHIP_MIN_W: i32 = 96;
    /// 状态胶囊高度（UI §5.1 #13「高 32」）。
    pub const STATUS_CHIP_H: i32 = 32;
    /// 指示灯圆直径（UI §5.1 #14「圆 16 px」）。
    pub const LED_DIA: i32 = 16;

    /// 警示行高度（UI §2.5 / §5.1 #12「高 56」）。
    pub const BANNER_H: i32 = 56;
    /// 警示行内边距（UI §2.5）。
    pub const BANNER_PAD: i32 = 16;

    /// 确认弹层宽（UI §7.3「宽 720，x 152–872」）。
    pub const DIALOG_W: i32 = 720;
    /// 确认弹层最小高（UI §7.3「最小 420」）。
    pub const DIALOG_MIN_H: i32 = 420;
    /// 弹层内边距（UI §7.3）。
    pub const DIALOG_PAD: i32 = 24;
    /// 弹层顶部级别色条高（UI §2.5 / §7.3「4 px 级别色条」）。
    pub const DIALOG_BAR_H: i32 = 4;
    /// 弹层明细行高（UI §7.3「行高 36」）。
    pub const DIALOG_ROW_H: i32 = 36;

    /// Toast 宽（UI §7.2「≤480×60」）。
    pub const TOAST_W: i32 = 480;
    /// Toast 高（UI §7.2）。
    pub const TOAST_H: i32 = 60;
    /// Toast 左缘色条宽（UI §7.2「左缘 4 px 色条」）。
    pub const TOAST_ACCENT_W: i32 = 4;
    /// Toast 定位 x（UI §7.2：(272,544,752,604)）。
    pub const TOAST_X: i32 = 272;
    /// Toast 定位 y（UI §7.2）。
    pub const TOAST_Y: i32 = 544;

    /// 日志/告警行高（UI §5.1 #10）。
    pub const ROW_LOG_H: i32 = 44;
    /// 审计行高（UI §5.1 #10 / §6.5）。
    pub const ROW_AUDIT_H: i32 = 60;
    /// 系统页字段行高（UI §5.1 #10 / §6.6）。
    pub const ROW_SYS_H: i32 = 56;
    /// 装置状态卡尺寸（UI §5.1 #19：236×92）。
    pub const CARD_STATUS_W: i32 = 236;
    /// 装置状态卡高。
    pub const CARD_STATUS_H: i32 = 92;

    /// 图标：小（UI §3.3「图标 28–72」下沿；图标按钮 / 警示行图标）。
    pub const ICON_SM: i32 = 28;
    /// 图标：中大（空态 / 不可用态图标）。
    pub const ICON_LG: i32 = 64;
    /// 图标：特大（P4 联锁总态图标）。
    pub const ICON_XL: i32 = 72;
    /// 字段状态点直径（UI §8.2「卡片右上 12 px」）。
    pub const STATUS_DOT: i32 = 12;
    /// 通用强调竖条/横条宽（卡头 4 px、Toast 4 px、审计不可篡改条 4 px）。
    pub const ACCENT_BAR: i32 = 4;
    /// 联锁总态卡左缘竖条宽（UI §6.4「卡左缘 6 px」）。
    pub const INTERLOCK_BAR: i32 = 6;
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. 圆角与描边（UI §3.5）
// ═══════════════════════════════════════════════════════════════════════════

/// 圆角（UI §3.5）。
pub struct Radius;

impl Radius {
    /// 卡片圆角 8 px。
    pub const CARD: i32 = 8;
    /// 控件圆角 6 px。
    pub const CTRL: i32 = 6;
    /// 胶囊圆角（全圆端）。
    pub const CHIP: i32 = 999;
    /// 直角（不圆角）。
    pub const NONE: i32 = 0;
}

/// 描边宽度（UI §3.5 / §5.2）。
pub struct Stroke;

impl Stroke {
    /// 无描边。
    pub const NONE: i32 = 0;
    /// 卡片 / 控件默认描边 1 px。
    pub const THIN: i32 = 1;
    /// 危险按钮描边 2 px（UI §5.2 `Button`-危险）。
    pub const DANGER: i32 = 2;
    /// 警示描边 3 px（UI §3.5「值越界 / 危险操作」；亦为弹层默认焦点环宽）。
    pub const ALERT: i32 = 3;
}

/// 不透明度（UI §7.3 遮罩 62 %）。
pub struct Opacity;

impl Opacity {
    /// 模态遮罩不透明度（UI §7.3「底 `#0B1220`，62 %」）。
    pub const MASK_PERCENT: u8 = 62;
    /// 危险确认按钮内的长按进度填充不透明度（UI §7.3「40 % 透明度」）。
    pub const PROGRESS_PERCENT: u8 = 40;
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 色板（UI §3.2 —— 逐位抄录，禁止改值）
// ═══════════════════════════════════════════════════════════════════════════

/// 色板（UI §3.2 全表）。
///
/// 写法约定：与 UI 文档的 `#RRGGBB` **逐位对应**（[`Color::hex`] 的入参即 `0xRRGGBB`），
/// 便于评审逐条比对。
///
/// **两套色系并存（有意）**：主读数语义色（高亮度，服务 0.5–1.5 m 远读）与 PRD 指定色
/// （指示灯 / 日志级别，服务 0.5 m 近读）**不在同一区块并排使用**（UI §3.2 注）。
pub struct Palette;

impl Palette {
    // ── 基础框架色 ────────────────────────────────────────────────────────
    /// `bg` 画布底 / 页面底色。
    pub const BG: Color = Color::hex(0x0B_12_20);
    /// `surface` 卡片 / 面板 / 导航底 / 弹层。
    pub const SURFACE: Color = Color::hex(0x14_1F_33);
    /// `surface_alt` 卡内嵌区（胶囊底 / 斑马纹奇数行 / Toast 底）。
    pub const SURFACE_ALT: Color = Color::hex(0x1B_29_42);
    /// `surface_high` 控件底（步进器 / 分段控件 / Chip 未选中底）。
    pub const SURFACE_HIGH: Color = Color::hex(0x24_33_4F);
    /// `surface_press` 全部控件的 pressed 态底色。
    pub const SURFACE_PRESS: Color = Color::hex(0x2E_40_66);
    /// `divider` 卡片描边 / 行分隔 / 滚动条轨道。
    pub const DIVIDER: Color = Color::hex(0x2A_3B_57);
    /// `border_ctrl` 控件默认描边 / 滚动条 thumb。
    pub const BORDER_CTRL: Color = Color::hex(0x3B_4A_6B);
    /// `text_primary` 数值 / 主标签。
    pub const TEXT_PRIMARY: Color = Color::hex(0xF4_F7_FF);
    /// `text_second` 单位 / 佐证 / 说明。
    pub const TEXT_SECOND: Color = Color::hex(0xA6_B6_D6);
    /// `text_weak` 图例 / 刻度 / 极弱注。
    pub const TEXT_WEAK: Color = Color::hex(0x6E_7F_A0);
    /// `text_disabled` 禁用文字。
    pub const TEXT_DISABLED: Color = Color::hex(0x5A_67_80);
    /// `placeholder` `--` 占位与降级数值。
    pub const PLACEHOLDER: Color = Color::hex(0x96_A2_BC);

    // ── 语义色 · 主读数体系 ───────────────────────────────────────────────
    /// 充电 / 正常 / 成功。
    pub const OK: Color = Color::hex(0x2F_DB_8A);
    /// 放电 / 选中 / 信息（导航与控件选中态、滚动条滚动中）。
    pub const INFO: Color = Color::hex(0x4E_A6_FF);
    /// 停机。
    pub const STOPPED: Color = Color::hex(0x8C_98_AC);
    /// 待机 / 琥珀警示文字。
    pub const STANDBY: Color = Color::hex(0xFF_D7_5E);
    /// SOC 正常（主数值 / 量程条中段 / 审计不可篡改标识条）。
    pub const SOC_OK: Color = Color::hex(0x35_D0_C4);
    /// SOC ≤15 % 下限邻近 / 数据异常 / 危险操作（描边与文字）。
    pub const DANGER: Color = Color::hex(0xFF_6B_6B);
    /// SOC ≥85 % 上限邻近。
    pub const SOC_HIGH: Color = Color::hex(0xFF_A9_4D);
    /// 过期 / 警示（过期角标、超时倒计时胶囊、警示行描边）。
    pub const STALE: Color = Color::hex(0xFF_B0_20);
    /// 方向不一致（**唯一专属色**，任何其他状态不用）。
    pub const INCONSISTENT: Color = Color::hex(0xFF_5C_D0);
    /// 危险按钮底（与深底强对比）。
    pub const DANGER_BG: Color = Color::hex(0x3A_1F_26);
    /// 危险按钮 pressed 态底（UI §5.2 `Button`-危险 按下）。
    pub const DANGER_PRESS_BG: Color = Color::hex(0x4A_26_30);
    /// 主按钮底（UI §5.2 `Button`-主 / `SegmentedControl` 选中段底）。
    pub const PRIMARY_BG: Color = Color::hex(0x1E_4E_8C);
    /// 警示行底（UI §2.5）
    pub const WARN_BG: Color = Color::hex(0x3A_2E_12);
    /// 缺数据胶囊底（UI §8.2「缺数据类：底 `#2A3550`」）。
    pub const CHIP_BG: Color = Color::hex(0x2A_35_50);
    /// 审计「不可篡改」说明条底（UI §6.5）。
    pub const AUDIT_BG: Color = Color::hex(0x14_23_1F);

    // ── PRD 指定色（指示灯 / 日志级别，逐字对齐 PRD §3.1 / §3.3）──────────
    /// 已连接 / 正常（PRD 指定）。
    pub const LINK_OK: Color = Color::hex(0x28_A7_45);
    /// 连接中 / 等待（PRD 指定；闪烁 ≤3 周期后转常亮，UI §7.5）。
    pub const LINK_PENDING: Color = Color::hex(0xFF_C1_07);
    /// 断开 / 错误（PRD 指定）。
    pub const LINK_DOWN: Color = Color::hex(0xDC_35_45);
    /// 未配置（PRD 指定；**空心圆**，UI §3.2）。
    pub const LINK_UNCONFIGURED: Color = Color::hex(0x5F_63_68);
    /// 日志级别 ERROR（PRD 指定）。
    pub const LOG_ERROR: Color = Color::hex(0xDC_35_45);
    /// 日志级别 WARN（PRD 指定）。
    pub const LOG_WARN: Color = Color::hex(0xFF_C1_07);
    /// 日志级别 INFO（PRD 指定）。
    pub const LOG_INFO: Color = Color::hex(0x17_A2_B8);
    /// 日志级别 DEBUG（PRD 指定）。
    pub const LOG_DEBUG: Color = Color::hex(0x9A_A0_A6);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 交互时长（UI §7.1 / §7.2 / §7.3 / §7.5；设计 §5.6）
// ═══════════════════════════════════════════════════════════════════════════

/// 交互时长常量（单一真源：`components.rs` 与后续页面一律引用，不得写字面量）。
pub struct Timing;

impl Timing {
    /// TT-10：写操作按钮的防重窗口（设计 §5.6 / UI §7.3「≥500 ms」）。
    pub const DEBOUNCE_MS: u64 = 500;
    /// L2 / L2+ 确认按钮的长按保持时长（设计 §5.6 / UI §7.3；⚠️ 阈值为**逐 indev**，
    /// 由事件循环注册触摸设备时经 `lvgl::indev::Indev::set_long_press_time` 施加，
    /// 见本模块 [`Timing::long_press`]）。
    pub const LONG_PRESS_MS: u64 = 1000;
    /// Toast 自动消失时长（UI §7.2「3 s」）。
    pub const TOAST_MS: u64 = 3000;
    /// F15 / TT-12：无触摸 60 s 回归主状态页。
    pub const IDLE_TIMEOUT_SECS: u64 = 60;
    /// 动效纪律：仅允许状态切换类动画，且 ≤200 ms（设计 §5.6「动效纪律」；
    /// **禁止**循环 / 装饰性动画）。
    pub const ANIM_MS: u64 = 200;

    /// [`Timing::LONG_PRESS_MS`] 的 `Duration` 形式。
    pub const fn long_press() -> Duration {
        Duration::from_millis(Self::LONG_PRESS_MS)
    }

    /// TT-10 防重窗口的 `Duration` 形式。
    pub const fn debounce() -> Duration {
        Duration::from_millis(Self::DEBOUNCE_MS)
    }

    /// Toast 存活时长的 `Duration` 形式。
    pub const fn toast() -> Duration {
        Duration::from_millis(Self::TOAST_MS)
    }

    /// 长按阈值的 `u16` 形式（`lv_indev_set_long_press_time` 的形参类型；设计 §5.6 订正：
    /// v9.5.0 的长按阈值**收敛于 indev**，没有逐控件 API）。
    pub const fn long_press_u16() -> u16 {
        Self::LONG_PRESS_MS as u16
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 字号档位（UI §3.3 阶梯 → §1.1.2 的 10 档位图字体）
// ═══════════════════════════════════════════════════════════════════════════

/// 文本语义槽位 —— UI §3.3 的**行语义**到 `FontSize` 档位的映射。
///
/// 页面**不得**直接写 `FontSize::S148` 这类档位名（与"不硬编码色值"同口径，设计 §5.6
/// NF-04 行）：一律用本枚举表达"这是什么文字"，由本模块决定它多大。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextSlot {
    /// L1-特大 148 px：P1 SOC 主数值。
    SocValue,
    /// L1-大 112 px：P1 PCS 状态词。
    PcsState,
    /// L1-大 96 px：P4 联锁总态词。
    InterlockState,
    /// L1-单位 56 px：`%`。
    Unit,
    /// L2-主值 64 px：三相 P 数值。
    PhasePower,
    /// L2-次值 48 px：三相 I 数值。
    PhaseCurrent,
    /// L2-页标题 32 px：页眉标题。
    PageTitle,
    /// L2-数值 32 px：装置状态卡值 / 审计值。
    CardValue,
    /// 标签-大 28 px：区块标题 / 相标。
    SectionTitle,
    /// 标签 26 px：字段名 / 控件文字 / 按钮字。
    Label,
    /// 正文 24 px：列表正文 / 弹层正文。
    Body,
    /// 弱注 24 px（全屏下限）：图例 / 刻度 / 说明行。
    Weak,
}

impl TextSlot {
    /// 全部槽位（升序，便于遍历断言）。
    pub const ALL: [TextSlot; 12] = [
        TextSlot::SocValue,
        TextSlot::PcsState,
        TextSlot::InterlockState,
        TextSlot::Unit,
        TextSlot::PhasePower,
        TextSlot::PhaseCurrent,
        TextSlot::PageTitle,
        TextSlot::CardValue,
        TextSlot::SectionTitle,
        TextSlot::Label,
        TextSlot::Body,
        TextSlot::Weak,
    ];

    /// 本槽位对应的字号档位（UI §3.3 → §1.1.2 的 10 档）。
    pub const fn size(self) -> FontSize {
        match self {
            TextSlot::SocValue => FontSize::S148,
            TextSlot::PcsState => FontSize::S112,
            TextSlot::InterlockState => FontSize::S96,
            TextSlot::Unit => FontSize::S56,
            TextSlot::PhasePower => FontSize::S64,
            TextSlot::PhaseCurrent => FontSize::S48,
            TextSlot::PageTitle => FontSize::S32,
            TextSlot::CardValue => FontSize::S32,
            TextSlot::SectionTitle => FontSize::S28,
            TextSlot::Label => FontSize::S26,
            TextSlot::Body => FontSize::S24,
            TextSlot::Weak => FontSize::S24,
        }
    }

    /// 本槽位对应的像素高度（= UI §3.3 表里的字号列）。
    pub const fn px(self) -> u32 {
        self.size().px()
    }
}

/// 取某槽位的字体 —— **含降级路径，不 panic**。
///
/// - 启用 `noto-font`：返回对应档位的 CJK 位图字体；
/// - **未启用（默认构建）**：`Font::of` 返回 `None` ⇒ 退回
///   [`Font::fallback`]（LVGL 内置 `lv_font_montserrat_14`；ASCII 与几何符号可读，
///   中文字形由 LVGL 的字形占位机制画占位框）。
///
/// 这条降级让**开发机（无字库）与真机（有字库）跑同一份 `ui/**` 代码**，
/// 不必在页面里写 `#[cfg(feature = ...)]`（设计 §5.6 NF-04 行 / `lvgl/font.rs` 模块文档）。
pub fn font_of(slot: TextSlot) -> Font {
    Font::of(slot.size()).unwrap_or_else(Font::fallback)
}

/// 把某槽位的字体施加到 `style` 上（先配置、再 `Rc::new` —— 见模块文档纪律 2）。
pub fn apply_font(style: &mut Style, slot: TextSlot) {
    style.set_text_font(&font_of(slot));
}

/// 文字样式：字号取 [`TextSlot`]、颜色取 [`Palette`]（两个通道都来自本模块）。
pub fn text(slot: TextSlot, color: Color) -> Rc<Style> {
    let mut s = Style::new();
    apply_font(&mut s, slot);
    s.set_text_color(color);
    Rc::new(s)
}

/// 居中的偏移量：`(outer − inner) / 2`（负数收敛到 0）。
///
/// 用途：薄层的定位能力只有 [`Obj::set_pos`] / [`Obj::center`]（**没有** `lv_obj_align` /
/// 文本对齐样式），故"把 24 px 文案垂直居中在 56 px 条内"这类需求必须由调用方**算出**
/// 坐标。本函数把该算式收成一处，避免 `(56 - 24) / 2` 这种裸表达式散落各处。
pub const fn center_offset(outer: i32, inner: i32) -> i32 {
    let d = outer - inner;
    if d > 0 {
        d / 2
    } else {
        0
    }
}

/// 图标字号 → 阶梯槽位。
///
/// ⚠️ **已知档位缺口（如实标注，见 B1 报告）**：UI §3.3 写「图标 28–72 px」，但 §1.1.2 的
/// 字体阶梯只有 **10 档**（148/112/96/64/56/48/32/28/26/24），**没有 72 px 档**。
/// 故 72 px 图标映射到 **64 px 档**（有意收敛，不放任取一个"近似值"）。
pub const fn icon_slot(px: i32) -> TextSlot {
    match px {
        // 28 px 图标 = 「标签-大」档
        28 => TextSlot::SectionTitle,
        // 64 / 72 px 图标 = 「L2-主值」档（72 ⇒ 64，见上）
        64 | 72 => TextSlot::PhasePower,
        // 其余按最近的下档收敛
        n if n <= 24 => TextSlot::Body,
        n if n <= 26 => TextSlot::Label,
        n if n <= 32 => TextSlot::PageTitle,
        n if n <= 48 => TextSlot::PhaseCurrent,
        _ => TextSlot::Unit,
    }
}

/// 图标样式（几何 / 方向图标一律走此入口，字号档位由 [`icon_slot`] 收敛）。
pub fn icon(px: i32, color: Color) -> Rc<Style> {
    text(icon_slot(px), color)
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. 确认强度分级（UI §2.5 / §7.3）
// ═══════════════════════════════════════════════════════════════════════════

/// 操作确认强度（UI §2.5 三级 + L2+ 后缀）。
///
/// **L0（只读 / 浏览 / 筛选 / 切页）不在本枚举内**：L0 的语义是"**不弹确认**"，故它由
/// "不调用 `ConfirmDialog`"表达 —— 若给 L0 也留一个变体，就会给"随手传个 `L0` 也能建出
/// 弹层"留后门（设计 §5.6「确认强度分级」行的口径是"分级决定弹层形态"，不是"分级决定是否
/// 建弹层"）。
///
/// **调用方必须显式传入**（`ConfirmDialog::new` 的 `level` 参数**无默认值** —— Rust 没有
/// 默认参数，故"漏配"在**编译期**即不成立；本类型亦不实现 `Default`，进一步杜绝"顺手取个
/// 默认值"）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfirmLevel {
    /// L1：一般写（无连接类字段的配置保存）。双步确认：点击 → 模态弹层，默认焦点「取消」，
    /// 确认按钮**单击生效**。级别色：`#4EA6FF`（UI §2.5）。
    L1 = 1,
    /// L2：破坏性 / 生效性写（联锁释放、M1 授权、含连接类字段的保存、恢复默认值）。
    /// 双步确认 + **长按保持 1.0 s**。级别色：危险色 `#FF6B6B`（UI §2.5）。
    L2 = 2,
    /// L2+：将瞬断链路的写（任一字段 `requires_reconnect == true`）。
    /// 同 L2，且弹层**必须**插入 `WarnBanner`（UI §2.5）。
    L2Plus = 3,
}

impl ConfirmLevel {
    /// 弹层顶部级别色条与描边色（UI §2.5 / §7.3）。
    pub const fn accent(self) -> Color {
        match self {
            ConfirmLevel::L1 => Palette::INFO,
            ConfirmLevel::L2 | ConfirmLevel::L2Plus => Palette::DANGER,
        }
    }

    /// 是否为"危险变体"（UI §7.3：确认按钮套危险样式）。
    pub const fn is_dangerous(self) -> bool {
        matches!(self, ConfirmLevel::L2 | ConfirmLevel::L2Plus)
    }

    /// 是否要求长按保持 1.0 s（UI §7.3）。
    pub const fn requires_long_press(self) -> bool {
        self.is_dangerous()
    }

    /// 是否**必须**插入 `WarnBanner`（UI §2.5：L2+ 强制）。
    pub const fn requires_warn_banner(self) -> bool {
        matches!(self, ConfirmLevel::L2Plus)
    }

    /// 确认按钮文案（UI §7.3：L1「确认执行」/ L2·L2+「按住确认」）。
    pub const fn confirm_text(self) -> &'static str {
        if self.requires_long_press() {
            "按住确认"
        } else {
            "确认执行"
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. 样式构造函数（常量 → `Rc<Style>`）
// ═══════════════════════════════════════════════════════════════════════════

/// 把一组 (样式, 选择器) 一次性挂到对象上（页面装配的便利入口）。
///
/// 选择器与样式的对应关系是 UI §5.2「状态 × 色值矩阵」的直接落点。
pub fn apply_all(obj: &Obj, styles: &[(&Rc<Style>, StyleSelector)]) {
    for (style, sel) in styles {
        obj.add_style(style, *sel);
    }
}

/// 页面底色（UI §3.1）。
pub fn screen_bg() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::BG);
    s.set_bg_opa(Opa::COVER);
    s.set_border_width(Stroke::NONE);
    Rc::new(s)
}

/// 卡片 / 分区卡（UI §5.3 `SectionCard` / `StatusCard`：底 `#141F33` + 1 px `#2A3B57` + 圆角 8）。
pub fn card() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(Radius::CARD);
    s.set_border_width(Stroke::THIN);
    s.set_border_color(Palette::DIVIDER);
    s.set_border_opa(Opa::COVER);
    s.set_border_side(BorderSide::FULL);
    s.set_pad_all(Dimens::GAP_MIN);
    Rc::new(s)
}

/// 卡内嵌区（胶囊底 / Toast 底 / 斑马纹奇数行，UI §3.2 `surface_alt`）。
pub fn surface_alt() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE_ALT);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(Radius::CTRL);
    s.set_border_width(Stroke::NONE);
    Rc::new(s)
}

/// 卡缘警示描边（UI §3.5「警示描边（值越界 / 危险操作）3 px（卡顶 / 卡左缘）」）。
fn card_alert(side: BorderSide, accent: Color) -> Rc<Style> {
    let mut s = Style::new();
    s.set_border_width(Stroke::ALERT);
    s.set_border_color(accent);
    s.set_border_opa(Opa::COVER);
    s.set_border_side(side);
    Rc::new(s)
}

/// 卡顶 3 px 警示描边（EDGE-02 SOC 源失效、UI §8.2）。
pub fn card_alert_top(accent: Color) -> Rc<Style> {
    card_alert(BorderSide::TOP, accent)
}

/// 卡左缘 3 px 警示描边（UI §8.2「整块不可用时区块左缘 3 px」）。
pub fn card_alert_left(accent: Color) -> Rc<Style> {
    card_alert(BorderSide::LEFT, accent)
}

/// 区块卡头的左侧强调竖条（UI §6.2「卡头 44 px（分组名 28 px + 左侧 4 px 竖条）」）。
pub fn card_head_bar(accent: Color) -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(accent);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(Radius::NONE);
    s.set_border_width(Stroke::NONE);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 按钮的四种语义（UI §5.2 / §5.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonKind {
    /// 主按钮（保存 / 确认执行）。
    Primary,
    /// 次按钮（取消 / 返回）。
    Secondary,
    /// 危险按钮（联锁释放 / M1 授权 / 恢复默认值）。
    Danger,
    /// 文字按钮（放弃修改 / 回到最新）。
    Text,
}

/// 一套按钮样式 = 5 个 `Style` + 各自的 `StyleSelector`（UI §5.2 的「正常 / 按下 / 禁用 /
/// 选中」四态 + §5.2「焦点环（弹层默认焦点）」的 `LV_STATE_FOCUSED`）。
///
/// LVGL v9 的样式一旦挂上即"一组 (part, state) 下的属性值"，故这里把矩阵拆成 5 份；
/// 用 [`ButtonStyles::apply`] 一次挂完。
pub struct ButtonStyles {
    /// 正常态。
    pub normal: Rc<Style>,
    /// 按下态（`LV_STATE_PRESSED`，反馈 ≤100 ms，UI §7.1）。
    pub pressed: Rc<Style>,
    /// 禁用态（`LV_STATE_DISABLED`；TT-10 防重反馈、步进器越界）。
    pub disabled: Rc<Style>,
    /// 选中态（`LV_STATE_CHECKED`；导航项 / 多选 Chip）。
    pub checked: Rc<Style>,
    /// **聚焦态**（`LV_STATE_FOCUSED`）—— 即 UI §5.2 的「焦点环」，TT-09「弹层默认焦点
    /// 『取消』」的**视觉**落点（值恒为 [`focus_ring`]）。
    ///
    /// ⚠️ 此前本样式集只有四态、且 `apply` 不挂 `FOCUSED` ⇒ 默认焦点**只有状态位、没有
    /// 视觉环**（注释与实现不符，评审 Important 2）。
    pub focused: Rc<Style>,
}

impl ButtonStyles {
    /// 本样式集覆盖的 (样式, 选择器) 列表 —— [`ButtonStyles::apply`] 与用例断言**共用同一
    /// 真源**（薄层没有"某选择器下挂了哪些样式"的读回 API，故"确已挂上"的断言只能以本列表
    /// 为据，见 `ui/tests.rs` 的 Important 2 断言与 B1 质量复审报告）。
    pub fn entries(&self) -> [(&Rc<Style>, StyleSelector); 5] {
        [
            (&self.normal, StyleSelector::state_of(State::DEFAULT)),
            (&self.pressed, StyleSelector::state_of(State::PRESSED)),
            (&self.disabled, StyleSelector::state_of(State::DISABLED)),
            (&self.checked, StyleSelector::state_of(State::CHECKED)),
            (&self.focused, StyleSelector::state_of(State::FOCUSED)),
        ]
    }

    /// 按 UI §5.2 的矩阵挂载五态（选择器由 [`ButtonStyles::entries`] 给出，调用方不必记
    /// LVGL 状态名）。`FOCUSED` 态挂的是 [`focus_ring`] ⇒ 获得焦点的按钮（弹层默认焦点
    /// 「取消」）自带 3 px `#4EA6FF` 焦点环。
    pub fn apply(&self, obj: &Obj) {
        apply_all(obj, &self.entries());
    }
}

fn btn_style(
    bg: Color,
    border: Color,
    border_w: i32,
    text: Color,
    radius: i32,
    pad: i32,
) -> Style {
    let mut s = Style::new();
    s.set_bg_color(bg);
    s.set_bg_opa(Opa::COVER);
    s.set_border_color(border);
    s.set_border_width(border_w);
    s.set_border_opa(Opa::COVER);
    s.set_border_side(BorderSide::FULL);
    s.set_radius(radius);
    s.set_pad_all(pad);
    apply_font(&mut s, TextSlot::Label);
    s.set_text_color(text);
    s
}

/// 按钮四态样式集（UI §5.2「控件状态 × 色值矩阵」逐行抄录）。
pub fn button(kind: ButtonKind) -> ButtonStyles {
    let r = Radius::CTRL;
    let pad = Dimens::GAP_MIN;
    match kind {
        // 底 #1E4E8C 描边 #4EA6FF 字 #F4F7FF；按下 #2E4066；禁用 #1B2942/#5A6780/#2A3B57
        ButtonKind::Primary => ButtonStyles {
            normal: Rc::new(btn_style(
                Palette::PRIMARY_BG,
                Palette::INFO,
                Stroke::THIN,
                Palette::TEXT_PRIMARY,
                r,
                pad,
            )),
            pressed: Rc::new(btn_style(
                Palette::SURFACE_PRESS,
                Palette::INFO,
                Stroke::THIN,
                Palette::TEXT_PRIMARY,
                r,
                pad,
            )),
            disabled: Rc::new(btn_style(
                Palette::SURFACE_ALT,
                Palette::DIVIDER,
                Stroke::THIN,
                Palette::TEXT_DISABLED,
                r,
                pad,
            )),
            checked: Rc::new(btn_style(
                Palette::PRIMARY_BG,
                Palette::INFO,
                Stroke::THIN,
                Palette::TEXT_PRIMARY,
                r,
                pad,
            )),
            focused: focus_ring(),
        },
        // 底 #1B2942 描边 #3B4A6B 字 #A6B6D6；按下 #2E4066；禁用同主按钮
        ButtonKind::Secondary => ButtonStyles {
            normal: Rc::new(btn_style(
                Palette::SURFACE_ALT,
                Palette::BORDER_CTRL,
                Stroke::THIN,
                Palette::TEXT_SECOND,
                r,
                pad,
            )),
            pressed: Rc::new(btn_style(
                Palette::SURFACE_PRESS,
                Palette::BORDER_CTRL,
                Stroke::THIN,
                Palette::TEXT_SECOND,
                r,
                pad,
            )),
            disabled: Rc::new(btn_style(
                Palette::SURFACE_ALT,
                Palette::DIVIDER,
                Stroke::THIN,
                Palette::TEXT_DISABLED,
                r,
                pad,
            )),
            checked: Rc::new(btn_style(
                Palette::PRIMARY_BG,
                Palette::INFO,
                Stroke::THIN,
                Palette::TEXT_PRIMARY,
                r,
                pad,
            )),
            focused: focus_ring(),
        },
        // 底 #3A1F26 描边 #FF6B6B 2 px 字 #FF6B6B；按下 #4A2630；禁用同主按钮
        ButtonKind::Danger => ButtonStyles {
            normal: Rc::new(btn_style(
                Palette::DANGER_BG,
                Palette::DANGER,
                Stroke::DANGER,
                Palette::DANGER,
                r,
                pad,
            )),
            pressed: Rc::new(btn_style(
                Palette::DANGER_PRESS_BG,
                Palette::DANGER,
                Stroke::DANGER,
                Palette::DANGER,
                r,
                pad,
            )),
            disabled: Rc::new(btn_style(
                Palette::SURFACE_ALT,
                Palette::DIVIDER,
                Stroke::THIN,
                Palette::TEXT_DISABLED,
                r,
                pad,
            )),
            checked: Rc::new(btn_style(
                Palette::DANGER_BG,
                Palette::DANGER,
                Stroke::DANGER,
                Palette::DANGER,
                r,
                pad,
            )),
            focused: focus_ring(),
        },
        // 透明底 字 #4EA6FF；按下底 #1B2942；禁用字 #5A6780
        ButtonKind::Text => ButtonStyles {
            normal: Rc::new(btn_style(
                Palette::BG,
                Palette::BG,
                Stroke::NONE,
                Palette::INFO,
                r,
                pad,
            )),
            pressed: Rc::new(btn_style(
                Palette::SURFACE_ALT,
                Palette::SURFACE_ALT,
                Stroke::NONE,
                Palette::INFO,
                r,
                pad,
            )),
            disabled: Rc::new(btn_style(
                Palette::BG,
                Palette::BG,
                Stroke::NONE,
                Palette::TEXT_DISABLED,
                r,
                pad,
            )),
            checked: Rc::new(btn_style(
                Palette::SURFACE_ALT,
                Palette::SURFACE_ALT,
                Stroke::NONE,
                Palette::TEXT_PRIMARY,
                r,
                pad,
            )),
            focused: focus_ring(),
        },
    }
}

/// 控件底（未选中的分段控件 / 多选 Chip，UI §3.2 `surface_high`）。
///
/// 文字属性（字号 / 颜色）设在**容器**上：LVGL 的文本属性对子对象**可继承**，故 Chip /
/// 分段控件的子 `lv_label` 会随容器状态（`LV_STATE_CHECKED`）一起变色，不需要给每个
/// 标签单独挂样式。
pub fn control_surface() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE_HIGH);
    s.set_bg_opa(Opa::COVER);
    s.set_border_color(Palette::BORDER_CTRL);
    s.set_border_width(Stroke::THIN);
    s.set_border_opa(Opa::COVER);
    s.set_border_side(BorderSide::FULL);
    s.set_radius(Radius::CTRL);
    s.set_pad_all(Dimens::GAP_MIN);
    apply_font(&mut s, TextSlot::Label);
    s.set_text_color(Palette::TEXT_SECOND);
    Rc::new(s)
}

/// **透明布局容器**（无底、无描边、零内边距）。
///
/// 用途：`components.rs` 与页面里的"排布用"容器（行 / 列 / 居中壳）—— LVGL 的基础
/// `lv_obj` **自带默认底色与内边距**，若不显式置空，会把设计值撑歪（设计 §5.6 控件策略行：
/// 外观一律由我们的样式覆盖，不得沿用默认主题配色）。
pub fn transparent() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_opa(Opa::TRANSPARENT);
    s.set_border_width(Stroke::NONE);
    s.set_pad_all(0);
    s.set_radius(Radius::NONE);
    Rc::new(s)
}

/// 分段控件 / 多选 Chip 的**选中**态（UI §5.2：底 `#1E4E8C` 描边 `#4EA6FF` 字 `#F4F7FF`）。
pub fn control_selected() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::PRIMARY_BG);
    s.set_bg_opa(Opa::COVER);
    s.set_border_color(Palette::INFO);
    s.set_border_width(Stroke::THIN);
    s.set_border_opa(Opa::COVER);
    s.set_border_side(BorderSide::FULL);
    s.set_radius(Radius::CTRL);
    s.set_pad_all(Dimens::GAP_MIN);
    apply_font(&mut s, TextSlot::Label);
    s.set_text_color(Palette::TEXT_PRIMARY);
    Rc::new(s)
}

/// 控件**按下**态（UI §5.2：底 `#2E4066`，全部控件通用）。
pub fn control_pressed() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE_PRESS);
    s.set_bg_opa(Opa::COVER);
    Rc::new(s)
}

/// 步进器值区（UI §5.2 `Stepper`：「值区底 `#24334F`」）。
pub fn stepper_value() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE_HIGH);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(Radius::CTRL);
    s.set_border_width(Stroke::NONE);
    apply_font(&mut s, TextSlot::Label);
    s.set_text_color(Palette::TEXT_PRIMARY);
    Rc::new(s)
}

/// 步进器 `−`/`＋` 按钮（UI §5.2：「`−/＋` 底 `#1B2942` 字 `#F4F7FF`」）。
pub fn stepper_button() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE_ALT);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(Radius::CTRL);
    s.set_border_width(Stroke::NONE);
    apply_font(&mut s, TextSlot::Label);
    s.set_text_color(Palette::TEXT_PRIMARY);
    Rc::new(s)
}

/// 滚动条 **idle**（UI §3.5 / §5.6-A：thumb `border_ctrl`，宽 8 px）。
///
/// ⚠️ **v9.5.0 源码事实**（设计 §5.6-A）：滚动条是**纯绘制部件**，LVGL 只画**一个**矩形
/// （比例 thumb），**没有**轨道/thumb 两个部件，也**没有**命中测试 —— 故本函数挂
/// `LV_PART_SCROLLBAR`，而 [`scrollbar_active`] 挂
/// `LV_PART_SCROLLBAR | LV_STATE_SCROLLED`（滚动中变色）。
pub fn scrollbar_idle() -> Rc<Style> {
    let mut s = Style::new();
    s.set_width(Dimens::SCROLLBAR_W);
    s.set_bg_color(Palette::BORDER_CTRL);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(Radius::CHIP);
    s.set_border_width(Stroke::NONE);
    Rc::new(s)
}

/// 滚动条**滚动中**（UI §3.2 `#4EA6FF`「滚动条滚动中（纯指示、不可拖）」）。
pub fn scrollbar_active() -> Rc<Style> {
    let mut s = Style::new();
    s.set_width(Dimens::SCROLLBAR_W);
    s.set_bg_color(Palette::INFO);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(Radius::CHIP);
    s.set_border_width(Stroke::NONE);
    Rc::new(s)
}

/// 滚动条**轨道**色（UI §3.5「轨道 `#141F33`」）。
///
/// 用途：设计要求的三色之一。原生滚动条部件在 v9.5.0 不画轨道（见 [`scrollbar_idle`] 注），
/// 故本样式供 §5.6-A **方案 B**（自绘 8 px hit-transparent 指示条）使用；方案 A 下不用。
pub fn scrollbar_track() -> Rc<Style> {
    let mut s = Style::new();
    s.set_width(Dimens::SCROLLBAR_W);
    s.set_bg_color(Palette::SURFACE);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(Radius::CHIP);
    s.set_border_width(Stroke::NONE);
    Rc::new(s)
}

/// 确认弹层主体（UI §7.3：底 `#141F33`、圆角 8、描边 1 px 按级别）。
///
/// **不设内边距**：顶部级别色条要求与弹层上缘齐平（UI §7.3 线框「▌4 px 级别色条」在
/// 最顶），故弹层内容由 `components.rs` 以 [`Dimens::DIALOG_PAD`] 显式定位。
pub fn dialog_panel(level: ConfirmLevel) -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE);
    s.set_bg_opa(Opa::COVER);
    s.set_radius(Radius::CARD);
    s.set_border_width(Stroke::THIN);
    s.set_border_color(level.accent());
    s.set_border_opa(Opa::COVER);
    s.set_border_side(BorderSide::FULL);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 确认弹层遮罩（UI §7.3：全屏 `#0B1220`、62 %）。遮罩**点击不关闭**弹层。
pub fn dialog_mask() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::BG);
    s.set_bg_opa(Opa::percent(Opacity::MASK_PERCENT));
    s.set_border_width(Stroke::NONE);
    s.set_radius(Radius::NONE);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 弹层顶部级别色条（UI §7.3：4 px，L1 `#4EA6FF` / L2 `#FF6B6B`）。
pub fn dialog_level_bar(level: ConfirmLevel) -> Rc<Style> {
    card_head_bar(level.accent())
}

/// 长按进度条的**底槽**（透明；进度值由 `lv_bar` 的值驱动，不用动画 —— 设计 §5.6 动效纪律）。
pub fn progress_track() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_opa(Opa::TRANSPARENT);
    s.set_border_width(Stroke::NONE);
    s.set_radius(Radius::NONE);
    s.set_pad_all(0);
    Rc::new(s)
}

/// 长按进度条的**填充**（UI §7.3：`#FF6B6B`、40 % 透明度）。挂 `LV_PART_INDICATOR`。
pub fn progress_indicator() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::DANGER);
    s.set_bg_opa(Opa::percent(Opacity::PROGRESS_PERCENT));
    s.set_radius(Radius::NONE);
    s.set_border_width(Stroke::NONE);
    Rc::new(s)
}

/// `WarnBanner`（UI §2.5：底 `#3A2E12`、描边 `#FFB020` 1 px、圆角 6）。
///
/// **不设内边距**：图标与文案由 `components.rs` 用 [`Dimens::BANNER_PAD`] 与
/// [`center_offset`] 显式定位（`lv_bar` 之外的定位能力在薄层只有 `Obj::set_pos` /
/// `Obj::center`，见 `components.rs` 模块文档「布局手法」）。
pub fn warn_banner() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::WARN_BG);
    s.set_bg_opa(Opa::COVER);
    s.set_border_color(Palette::STALE);
    s.set_border_width(Stroke::THIN);
    s.set_border_opa(Opa::COVER);
    s.set_border_side(BorderSide::FULL);
    s.set_radius(Radius::CTRL);
    s.set_pad_all(0);
    apply_font(&mut s, TextSlot::Body);
    s.set_text_color(Palette::STANDBY);
    Rc::new(s)
}

/// Toast 主体（UI §7.2：底 `#1B2942` + 描边 `#3B4A6B` + 圆角 8）。
///
/// **不设内边距**（与 [`warn_banner`] / [`dialog_panel`] 同一口径）：UI §7.2 要求「左缘 4 px
/// 色条」贴弹层**外缘**并通高，而 `components.rs` 的子对象坐标是相对父**内容区**
/// （= 外缘 + 描边 + 内边距）的 ⇒ 若这里再设 `GAP_MIN` 内边距，色条会内缩
/// `pad + border` px 且高度被父内容区裁掉（此前实测 `accent.x1 = toast.x1 + 17`、
/// 可视高仅 `60 - 2×16 = 28`）。故内边距一律为 0，位置由 `components.rs` 显式给出。
pub fn toast() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE_ALT);
    s.set_bg_opa(Opa::COVER);
    s.set_border_color(Palette::BORDER_CTRL);
    s.set_border_width(Stroke::THIN);
    s.set_border_opa(Opa::COVER);
    s.set_border_side(BorderSide::FULL);
    s.set_radius(Radius::CARD);
    s.set_pad_all(0);
    apply_font(&mut s, TextSlot::Body);
    s.set_text_color(Palette::TEXT_PRIMARY);
    Rc::new(s)
}

/// Toast 左缘 4 px 结果色条（UI §7.2：成功 `#2FDB8A` / 失败 `#FF6B6B` / 警示 `#FFB020`）。
pub fn toast_accent(color: Color) -> Rc<Style> {
    card_head_bar(color)
}

/// 默认焦点环（UI §5.2「焦点环（弹层默认焦点）：描边 `#4EA6FF` 3 px」）。
///
/// 挂 `LV_STATE_FOCUSED`：TT-09「默认焦点『取消』」的**视觉**落点由它承担，语义落点由
/// `components.rs` 的输入组（`lv_group`）承担。
pub fn focus_ring() -> Rc<Style> {
    let mut s = Style::new();
    s.set_border_width(Stroke::ALERT);
    s.set_border_color(Palette::INFO);
    s.set_border_opa(Opa::COVER);
    s.set_border_side(BorderSide::FULL);
    Rc::new(s)
}

/// 只读 / 不可用灰色块（UI §6.2「不可用态灰化」、§8.2「整块不可用时区块左缘 3 px」）。
pub fn disabled_surface() -> Rc<Style> {
    let mut s = Style::new();
    s.set_bg_color(Palette::SURFACE_ALT);
    s.set_bg_opa(Opa::COVER);
    s.set_border_color(Palette::DIVIDER);
    s.set_border_width(Stroke::THIN);
    s.set_border_opa(Opa::COVER);
    s.set_border_side(BorderSide::FULL);
    s.set_radius(Radius::CTRL);
    Rc::new(s)
}

/// 状态胶囊皮肤（UI §8.2 / §6.4 的四种胶囊）。
///
/// **色通道**：`bg` / `border` / `text` 三色**全部取自 [`Palette`]**（本模块）。F14 的
/// 「三通道（文字 + 颜色 + 图标）」中的**颜色通道**即由本类型承载；`components.rs` 的
/// `StatusChip` 构造签名要求调用方**显式**给出皮肤，不存在"纯色块"的实例化路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChipSkin {
    /// 胶囊底。
    pub bg: Color,
    /// 胶囊描边。
    pub border: Color,
    /// 胶囊文字（含图标）颜色。
    pub text: Color,
}

impl ChipSkin {
    /// 缺数据类（UI §8.2：底 `#2A3550` 描边 `#3B4A6B` 字 `#96A2BC`）——`未取数` / `源离线` / `数据异常`。
    pub const MISSING_DATA: Self = Self {
        bg: Palette::CHIP_BG,
        border: Palette::BORDER_CTRL,
        text: Palette::PLACEHOLDER,
    };
    /// 警示类（UI §8.2：底 `#3A2E12` 描边 `#FFB020` 字 `#FFD75E`）——`数据过期` / `冻结` / latch 已保持。
    pub const WARNING: Self = Self {
        bg: Palette::WARN_BG,
        border: Palette::STALE,
        text: Palette::STANDBY,
    };
    /// 中性类（UI §6.4「自锁保持 · 未保持」底 `#1B2942` 字 `#A6B6D6`）。
    pub const NEUTRAL: Self = Self {
        bg: Palette::SURFACE_ALT,
        border: Palette::BORDER_CTRL,
        text: Palette::TEXT_SECOND,
    };
    /// 不可用类（UI §6.4「latch 胶囊转『不可用』灰底 `#2A3550` 字 `#96A2BC`」）。
    pub const UNAVAILABLE: Self = Self {
        bg: Palette::CHIP_BG,
        border: Palette::DIVIDER,
        text: Palette::PLACEHOLDER,
    };
    /// 成功类（UI §6.5 `● 成功`）。
    pub const SUCCESS: Self = Self {
        bg: Palette::SURFACE_ALT,
        border: Palette::OK,
        text: Palette::OK,
    };
    /// 失败类（UI §6.5 `✕ 失败`）。
    pub const FAILURE: Self = Self {
        bg: Palette::DANGER_BG,
        border: Palette::DANGER,
        text: Palette::DANGER,
    };
    /// 方向不一致（UI §8.3 EDGE-06：品红专属胶囊）。
    pub const INCONSISTENT: Self = Self {
        bg: Palette::SURFACE_ALT,
        border: Palette::INCONSISTENT,
        text: Palette::INCONSISTENT,
    };

    /// 全部皮肤（遍历 / 断言用）。
    pub const ALL: [ChipSkin; 7] = [
        Self::MISSING_DATA,
        Self::WARNING,
        Self::NEUTRAL,
        Self::UNAVAILABLE,
        Self::SUCCESS,
        Self::FAILURE,
        Self::INCONSISTENT,
    ];

    /// **颜色通道**的取值（= 文字色；F14 三重冗余断言用）。
    pub const fn accent(self) -> Color {
        self.text
    }

    /// 该胶囊的 LVGL 样式（圆角全圆端、高 32 由 `components.rs` 设）。
    pub fn style(self) -> Rc<Style> {
        let mut s = Style::new();
        s.set_bg_color(self.bg);
        s.set_bg_opa(Opa::COVER);
        s.set_border_color(self.border);
        s.set_border_width(Stroke::THIN);
        s.set_border_opa(Opa::COVER);
        s.set_border_side(BorderSide::FULL);
        s.set_radius(Radius::CHIP);
        // **内边距一律为 0**（与 `warn_banner()` / `dialog_panel()` / `toast()` 同口径）：
        // 胶囊内的图标/文字由 `components.rs` **显式 `set_pos`** 定位；若此处再设 padding，
        // 子对象的"外缘坐标"假设会与 padding 叠加，导致整体位移（并可能底溢）。
        // 竖直居中改由子对象的 `center_offset(STATUS_CHIP_H, 行高)` 承担。
        s.set_pad_top(0);
        s.set_pad_bottom(0);
        s.set_pad_left(0);
        s.set_pad_right(0);
        apply_font(&mut s, TextSlot::Body);
        s.set_text_color(self.text);
        Rc::new(s)
    }
}
