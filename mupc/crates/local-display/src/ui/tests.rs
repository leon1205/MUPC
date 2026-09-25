//! # `ui` 层自测（工作单元 B1：`theme.rs` + `components.rs`）
//!
//! ## ⚠️ 为什么 LVGL 部分没有自己的 `#[test]`
//!
//! LVGL 全局状态**非线程安全**（`lvcfg`：`LV_USE_OS = LV_OS_NONE`），`cargo test` 默认多线程
//! 跑测试函数。因此触碰 LVGL 的用例（[`ui_chain`]）**不另起 `#[test]`**，而是沿用 A1/A2/A3 的
//! 串行化做法 —— 由 `src/lvgl/tests.rs::lvgl_core_bridge_chain`（**唯一的** LVGL `#[test]`）
//! 在同一线程内顺序调起。
//!
//! > 该挂钩点已由 PM 授权，并落在 `src/lvgl/tests.rs::lvgl_core_bridge_chain` 末尾
//! > （`crate::ui::tests::ui_chain();`）——B1 不改 `src/lvgl/**` 的实现，仅由 PM 追加这一行。
//!
//! 纯逻辑用例（主题常量、字号阶梯、确认分级、防重、静态扫描、码表走查）**不触碰 LVGL**，
//! 故可以安全地作为独立 `#[test]` 并行执行。
//!
//! ## 覆盖（按验收口径逐条）
//!
//! | # | 验收项 | 落点 |
//! |---|--------|------|
//! | ① | `theme` 常量与 UI §3.2 / §3.3 / §3.5 数值一致 | [`theme_matches_ui_spec`] |
//! | ② | 组件能创建并施加主题样式（尺寸 / 颜色读回） | [`ui_chain`] |
//! | ③ | `StatusChip` / `LedIndicator` 三通道强制 | [`ui_chain`] + [`chip_skin_matches_ui_spec`] |
//! | ④ | `UnavailableState` 与 `EmptyState` 可区分 | [`ui_chain`] + [`unavailable_is_not_empty`] |
//! | ⑤ | `ConfirmDialog` 的 `level` 无默认值 | [`confirm_level_has_no_default`] |
//! | ⑥ | `ui/**` 静态约束（禁裸色值 / 文本输入控件 / 直连绑定 / `refr`） | [`ui_static_constraints`] |

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::lvgl::display::{Area, Display};
use crate::lvgl::event::EventCode;
use crate::lvgl::font::FontSize;
// 真实触摸链路（B4b：SH5 的"任何触摸事件重置"走 `lv_indev`，不再用合成投递代理）。
use crate::lvgl::indev::{Indev, TouchSnapshot};
use crate::lvgl::obj::Obj;
// `Part` / `State` / `ScrollContainer` 不在此处 import：下游用例一律走
// `crate::lvgl::style::…` / `crate::lvgl::widgets::…` 全限定路径引用，短名反而无人使用。
use crate::lvgl::style::{Color, Opa, StyleSelector};
use crate::lvgl::{self, LvglError};
use crate::ui::components::{
    self, ConfirmDetail, ConfirmDialog, ConfirmSpec, Debounce, EmptyState, LedIndicator,
    MultiSelectChips, StateSemantics, StatusChip, Stepper, Toast, ToastTone, UnavailableKind,
    UnavailableState, WarnBanner, ALL_TEXTS, TEXT_CHECK_PREFIX, TEXT_WARN_BANNER,
    TEXT_WARN_FIELD_SEP, TEXT_WARN_FIELDS_PREFIX,
};
use crate::ui::theme::{
    self, ChipSkin, ConfirmLevel, Dimens, Opacity, Palette, Radius, Stroke, TextSlot, Timing,
};
// U-73（T21c-1）的 H-2 / T-23 覆盖率网与 P4 消防用例（下同）：
// `ui_text` 是**上屏中文的唯一来源**（`display-proto`），`GROUP_TITLES` 是分组标题的唯一来源。
use mupc_display_proto::peripherals_labels::{ui_text, GROUP_TITLES};

// ═══════════════════════════════════════════════════════════════════════════
// 小工具
// ═══════════════════════════════════════════════════════════════════════════

/// `Color` → `0xRRGGBB`（与 UI §3.2 的写法逐位对应，便于逐条比对）。
fn hex(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | (c.b as u32)
}

// ═══════════════════════════════════════════════════════════════════════════
// ① 主题常量 vs UI §3.2 / §3.3 / §3.5
// ═══════════════════════════════════════════════════════════════════════════

/// ① 栅格 / 触摸尺寸 / 圆角 / 描边 / 不透明度（UI §2.1 / §3.5 / §7.3）。
#[test]
fn theme_matches_ui_spec() {
    // ── UI §2.1 触摸人机工程：四个硬常量（设计 §5.6 TT-08 行点名）──
    assert_eq!(Dimens::TOUCH_MIN, 48, "TT-08：最小触摸目标 48×48");
    assert_eq!(Dimens::TOUCH_CRITICAL, 64, "TT-08：关键操作 64×64");
    assert_eq!(Dimens::GAP_MIN, 16, "TT-08：相邻可点控件间距 ≥16");
    assert_eq!(Dimens::GAP_DANGER, 48, "TT-08：破坏性操作与取消间距 ≥48");

    // ── UI §3.5 栅格与装饰常量 ──
    assert_eq!(Dimens::SCREEN_W, 1024, "画布 1024×768");
    assert_eq!(Dimens::SCREEN_H, 768);
    assert_eq!(Dimens::HEADER_H, 72, "页眉高度 72");
    assert_eq!(Dimens::CONTENT_H, 624, "内容区 624");
    assert_eq!(Dimens::NAV_H, 72, "底部导航 72");
    assert_eq!(Dimens::SIDE_PAD, 16, "内容区左右安全边 16");
    assert_eq!(Dimens::CONTENT_W, 992, "有效宽 992");
    assert_eq!(Dimens::CONTENT_PAD_TOP, 8, "上内边距 8");
    assert_eq!(Dimens::CONTENT_PAD_BOTTOM, 24, "下内边距 24");
    assert_eq!(Dimens::GAP_GROUP, 16, "同组呼吸缝 16");
    assert_eq!(Dimens::GAP_SECTION, 24, "跨区呼吸缝 24");
    assert_eq!(Radius::CARD, 8, "卡片圆角 8");
    assert_eq!(Radius::CTRL, 6, "控件圆角 6");
    assert_eq!(Radius::CHIP, 999, "胶囊圆角 999（全圆端）");
    assert_eq!(Stroke::THIN, 1, "卡片描边 1");
    assert_eq!(Stroke::ALERT, 3, "警示描边 3");
    assert_eq!(Dimens::SCROLLBAR_W, 8, "滚动条宽 8");
    assert_eq!(Dimens::SCROLLBAR_MARGIN, 4, "滚动条距右边缘 4");
    assert_eq!(Opacity::MASK_PERCENT, 62, "弹层遮罩 62%");
    // 整屏降级（EDGE-03）的压暗档：与模态遮罩**不是同一件事**（底层须仍可读 ⇒ 取 20 %）。
    assert_eq!(Opacity::DEGRADE_PERCENT, 20, "EDGE-03 整屏遮罩压暗 20%");

    // ── UI §5.1 控件尺寸（抽查被组件直接依赖的那几个）──
    assert_eq!(Dimens::STATUS_CHIP_H, 32, "StatusChip 高 32");
    assert_eq!(Dimens::LED_DIA, 16, "LedIndicator 圆 16");
    assert_eq!(Dimens::CHIP_H, 48, "多选 Chip 高 48");
    assert_eq!(Dimens::CHIP_MIN_W, 96, "多选 Chip 最小宽 96");
    assert_eq!(Dimens::STEPPER_BTN_W, 64, "步进器 −/＋ 64");
    assert_eq!(Dimens::STEPPER_VALUE_W, 120, "步进器值区 120");
    assert_eq!(Dimens::STEPPER_H, 64, "步进器高 64");
    assert_eq!(
        Dimens::STEPPER_BTN_W * 2 + Dimens::STEPPER_VALUE_W,
        248,
        "UI §5.1 #6 步进器总宽 248"
    );
    assert_eq!(Dimens::BTN_MAIN_W, 200, "主按钮宽 200");
    assert_eq!(Dimens::BTN_H_PRIMARY, 64, "主按钮高 64");
    assert_eq!(Dimens::DIALOG_W, 720, "弹层宽 720");
    assert_eq!(Dimens::DIALOG_MIN_H, 420, "弹层最小高 420");
    assert_eq!(Dimens::DIALOG_BAR_H, 4, "级别色条 4");
    assert_eq!(Dimens::DIALOG_ROW_H, 36, "明细行高 36");
    assert_eq!(Dimens::BANNER_H, 56, "警示行高 56");
    assert_eq!(Dimens::TOAST_W, 480, "Toast 宽 480");
    assert_eq!(Dimens::TOAST_H, 60, "Toast 高 60");
    assert_eq!(Dimens::TOAST_ACCENT_W, 4, "Toast 左缘色条 4");
    assert_eq!(Dimens::ICON_LG, 64, "空态图标 64");
    assert_eq!(Dimens::ICON_SM, 28, "小图标 28");

    // ── UI §7.1 / §7.2 / §7.3 / §7.5 时长 ──
    assert_eq!(Timing::DEBOUNCE_MS, 500, "TT-10 防重 500 ms");
    assert_eq!(Timing::LONG_PRESS_MS, 1000, "L2 长按保持 1.0 s");
    assert_eq!(Timing::long_press_u16(), 1000, "长按阈值传给 indev 的口径");
    assert_eq!(Timing::TOAST_MS, 3000, "Toast 3 s");
    assert_eq!(Timing::IDLE_TIMEOUT_SECS, 60, "F15/TT-12 60 s 回归");
    assert_eq!(Timing::ANIM_MS, 200, "动效纪律 ≤200 ms");
}

/// ① 色板逐条对齐 UI §3.2（**逐位**比对 `#RRGGBB`）。
#[test]
fn palette_matches_ui_spec() {
    // 基础框架色
    assert_eq!(hex(Palette::BG), 0x0B_12_20, "bg 画布底");
    assert_eq!(hex(Palette::SURFACE), 0x14_1F_33, "surface 卡 / 面板 / 导航底");
    assert_eq!(hex(Palette::SURFACE_ALT), 0x1B_29_42, "surface_alt 卡内嵌区");
    assert_eq!(hex(Palette::SURFACE_HIGH), 0x24_33_4F, "surface_high 控件底（新增）");
    assert_eq!(hex(Palette::SURFACE_PRESS), 0x2E_40_66, "surface_press 按压底（新增）");
    assert_eq!(hex(Palette::DIVIDER), 0x2A_3B_57, "divider 分隔线");
    assert_eq!(hex(Palette::BORDER_CTRL), 0x3B_4A_6B, "border_ctrl 控件描边");
    assert_eq!(hex(Palette::TEXT_PRIMARY), 0xF4_F7_FF, "text_primary");
    assert_eq!(hex(Palette::TEXT_SECOND), 0xA6_B6_D6, "text_second");
    assert_eq!(hex(Palette::TEXT_WEAK), 0x6E_7F_A0, "text_weak");
    assert_eq!(hex(Palette::TEXT_DISABLED), 0x5A_67_80, "text_disabled");
    assert_eq!(hex(Palette::PLACEHOLDER), 0x96_A2_BC, "placeholder `--`");

    // 语义色 · 主读数体系（UI §3.2 第二张表，逐字不变）
    assert_eq!(hex(Palette::OK), 0x2F_DB_8A, "充电 / 正常 / 成功");
    assert_eq!(hex(Palette::INFO), 0x4E_A6_FF, "放电 / 选中 / 信息");
    assert_eq!(hex(Palette::STOPPED), 0x8C_98_AC, "停机 / 不可用灰");
    assert_eq!(hex(Palette::STANDBY), 0xFF_D7_5E, "待机 / 警示文字");
    assert_eq!(hex(Palette::SOC_OK), 0x35_D0_C4, "SOC 正常");
    assert_eq!(hex(Palette::DANGER), 0xFF_6B_6B, "SOC 低 / 数据异常 / 危险操作");
    assert_eq!(hex(Palette::SOC_HIGH), 0xFF_A9_4D, "SOC 高");
    assert_eq!(hex(Palette::STALE), 0xFF_B0_20, "过期 / 警示");
    assert_eq!(hex(Palette::INCONSISTENT), 0xFF_5C_D0, "方向不一致（唯一专属色）");
    assert_eq!(hex(Palette::DANGER_BG), 0x3A_1F_26, "危险操作底（新增）");
    assert_eq!(hex(Palette::PRIMARY_BG), 0x1E_4E_8C, "主按钮底 / 选中段底");
    assert_eq!(hex(Palette::WARN_BG), 0x3A_2E_12, "警示行底");
    assert_eq!(hex(Palette::CHIP_BG), 0x2A_35_50, "缺数据胶囊底");

    // PRD 指定色（UI §3.2 第三张表，不得改值）
    assert_eq!(hex(Palette::LINK_OK), 0x28_A7_45, "已连接 / 正常");
    assert_eq!(hex(Palette::LINK_PENDING), 0xFF_C1_07, "连接中 / 等待");
    assert_eq!(hex(Palette::LINK_DOWN), 0xDC_35_45, "断开 / 错误");
    assert_eq!(hex(Palette::LINK_UNCONFIGURED), 0x5F_63_68, "未配置");
    assert_eq!(hex(Palette::LOG_ERROR), 0xDC_35_45, "日志 ERROR");
    assert_eq!(hex(Palette::LOG_WARN), 0xFF_C1_07, "日志 WARN");
    assert_eq!(hex(Palette::LOG_INFO), 0x17_A2_B8, "日志 INFO");
    assert_eq!(hex(Palette::LOG_DEBUG), 0x9A_A0_A6, "日志 DEBUG");
}

/// ① 字号阶梯：§1.1.2 的 **10 档** + §3.3 的**行语义 → 档位**映射。
#[test]
fn font_ladder_matches_ui_spec() {
    let px: Vec<u32> = FontSize::ALL.iter().map(|f| f.px()).collect();
    assert_eq!(
        px,
        vec![24, 26, 28, 32, 48, 56, 64, 96, 112, 148],
        "§1.1.2 十档（升序）：24/26/28/32/48/56/64/96/112/148"
    );
    assert_eq!(FontSize::ALL.len(), 10, "档位数恒为 10");

    // §3.3 逐行落位
    assert_eq!(TextSlot::SocValue.px(), 148, "L1-特大 SOC 数值");
    assert_eq!(TextSlot::PcsState.px(), 112, "L1-大 PCS 状态词");
    assert_eq!(TextSlot::InterlockState.px(), 96, "L1-大 联锁总态词");
    assert_eq!(TextSlot::Unit.px(), 56, "L1-单位 %");
    assert_eq!(TextSlot::PhasePower.px(), 64, "L2-主值 三相 P");
    assert_eq!(TextSlot::PhaseCurrent.px(), 48, "L2-次值 三相 I");
    assert_eq!(TextSlot::PageTitle.px(), 32, "L2-页标题");
    assert_eq!(TextSlot::CardValue.px(), 32, "L2-数值 装置卡值 / 审计值");
    assert_eq!(TextSlot::SectionTitle.px(), 28, "标签-大 区块标题");
    assert_eq!(TextSlot::Label.px(), 26, "标签 字段名 / 控件文字");
    assert_eq!(TextSlot::Body.px(), 24, "正文");
    assert_eq!(TextSlot::Weak.px(), 24, "弱注（全屏下限）");

    // 槽位全覆盖 + 每个槽位都落在 10 档内
    assert_eq!(TextSlot::ALL.len(), 12, "槽位数 12");
    for slot in TextSlot::ALL {
        assert!(
            FontSize::ALL.contains(&slot.size()),
            "{slot:?} 的档位必须落在 §1.1.2 的 10 档内"
        );
    }
    // NF-04 下限：正文 / 弱注不得低于 24
    assert!(TextSlot::Body.px() >= 24 && TextSlot::Weak.px() >= 24, "NF-04 下限 24 px");
    // 图标档位收敛（§3.3「图标 28–72」与 10 档的缺口，见 `theme::icon_slot` 文档）
    assert_eq!(theme::icon_slot(28), TextSlot::SectionTitle, "28 px 图标 = 28 档");
    assert_eq!(theme::icon_slot(64), TextSlot::PhasePower, "64 px 图标 = 64 档");
    assert_eq!(theme::icon_slot(72), TextSlot::PhasePower, "72 px 图标收敛到 64 档（无 72 档）");
}

/// ③ `ChipSkin` 的四种文档化皮肤（UI §8.2 / §6.4）。
#[test]
fn chip_skin_matches_ui_spec() {
    let check = |s: ChipSkin, bg: u32, border: u32, text: u32, what: &str| {
        assert_eq!(hex(s.bg), bg, "{what} 底");
        assert_eq!(hex(s.border), border, "{what} 描边");
        assert_eq!(hex(s.text), text, "{what} 字色");
    };
    check(ChipSkin::MISSING_DATA, 0x2A_35_50, 0x3B_4A_6B, 0x96_A2_BC, "缺数据类");
    check(ChipSkin::WARNING, 0x3A_2E_12, 0xFF_B0_20, 0xFF_D7_5E, "警示类");
    check(ChipSkin::UNAVAILABLE, 0x2A_35_50, 0x2A_3B_57, 0x96_A2_BC, "不可用类");
    // 「颜色通道」= `accent`（F14 断言口径）
    assert_eq!(ChipSkin::WARNING.accent(), Palette::STANDBY, "警示类 accent = 文字色");
    assert_eq!(ChipSkin::SUCCESS.accent(), Palette::OK, "成功类 accent");
    assert_eq!(ChipSkin::FAILURE.accent(), Palette::DANGER, "失败类 accent");
    // 三色通道各自都来自 theme 常量（此处只需保证存在且互不相同的主张成立）
    assert_ne!(hex(ChipSkin::NEUTRAL.bg), hex(ChipSkin::NEUTRAL.text), "中性类底色与字色不同");
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑤ 确认分级：`level` 无默认值 + 分级语义
// ═══════════════════════════════════════════════════════════════════════════

/// ⑤ `ConfirmDialog::new` 的 `level` **必须显式传入**（无默认值）。
///
/// 做法是**类型层面的断言**：把构造函数强制转换成"带满三个参数"的函数指针类型 ——
/// 少一个参数、或参数类型不符，**编译期**即失败。
///
/// Rust 没有默认参数，故"漏配 `level`"在语法上就不成立；`ConfirmLevel` 也**不实现
/// `Default`**，杜绝"顺手取个默认值"。这个断言把该口径钉在测试里，防止后人给
/// `ConfirmDialog::new` 加一个"默认 L1"的重载。
#[test]
fn confirm_level_has_no_default() {
    let _ctor: fn(&Obj, &ConfirmSpec<'static>, ConfirmLevel) -> Result<ConfirmDialog, LvglError> =
        ConfirmDialog::new;

    // 分级语义（UI §2.5 / §7.3）
    assert!(!ConfirmLevel::L1.is_dangerous(), "L1 不是危险变体");
    assert!(ConfirmLevel::L2.is_dangerous(), "L2 是危险变体");
    assert!(ConfirmLevel::L2Plus.is_dangerous(), "L2+ 是危险变体");

    assert!(!ConfirmLevel::L1.requires_long_press(), "L1 单击生效");
    assert!(ConfirmLevel::L2.requires_long_press(), "L2 长按 1.0 s");
    assert!(ConfirmLevel::L2Plus.requires_long_press(), "L2+ 长按 1.0 s");

    assert!(!ConfirmLevel::L1.requires_warn_banner(), "L1 无 WarnBanner");
    assert!(!ConfirmLevel::L2.requires_warn_banner(), "L2 无 WarnBanner");
    assert!(ConfirmLevel::L2Plus.requires_warn_banner(), "L2+ **必须**有 WarnBanner");

    assert_eq!(ConfirmLevel::L1.accent(), Palette::INFO, "L1 级别色 = 蓝");
    assert_eq!(ConfirmLevel::L2.accent(), Palette::DANGER, "L2 级别色 = 危险红");
    assert_eq!(ConfirmLevel::L2Plus.accent(), Palette::DANGER, "L2+ 级别色 = 危险红");

    assert_eq!(ConfirmLevel::L1.confirm_text(), "确认执行", "L1 按钮文案");
    assert_eq!(ConfirmLevel::L2.confirm_text(), "按住确认", "L2 按钮文案");
    assert_eq!(ConfirmLevel::L2Plus.confirm_text(), "按住确认", "L2+ 按钮文案");
}

/// ④ 不可用态**不等价于**"确知没有"（UI §8.3 联锁专行 / EDGE-09 / EDGE-17）。
#[test]
fn unavailable_is_not_empty() {
    assert_ne!(
        EmptyState::SEMANTICS,
        UnavailableState::SEMANTICS,
        "空态与不可用态必须是两个语义标记"
    );
    assert_eq!(EmptyState::SEMANTICS, StateSemantics::Empty);
    assert_eq!(UnavailableState::SEMANTICS, StateSemantics::Unavailable);

    // 三种不可用场景的标题逐字对齐 §3.6 用字表，且两两不同
    assert_eq!(UnavailableKind::AlertSource.title(), "告警源不可用", "EDGE-09");
    assert_eq!(UnavailableKind::Audit.title(), "审计记录不可用", "EDGE-17");
    assert_eq!(UnavailableKind::Interlock.title(), "联锁状态不可用", "F16.6 / IL-01");
    let mut titles: Vec<&str> = UnavailableKind::ALL.iter().map(|k| k.title()).collect();
    titles.sort_unstable();
    titles.dedup();
    assert_eq!(titles.len(), UnavailableKind::ALL.len(), "三个场景标题互不相同");

    // **关键约束**：联锁「不可用」不得显示为「未联锁」（fail-closed）
    assert_ne!(
        UnavailableKind::Interlock.title(),
        "未联锁",
        "F16.6：『不可用』= 无法获知，『未联锁』= 确知安全，二者不得互替"
    );
    for k in UnavailableKind::ALL {
        assert!(!k.means_nothing_happened(), "{k:?} 不得被当作『确知无事』");
        assert_eq!(k.accent(), Palette::STOPPED, "{k:?} 灰化色 #8C98AC");
        assert_ne!(k.icon(), "✓", "{k:?} 不得复用 ✓（§8.3 联锁专行）");
        assert_ne!(k.icon(), "⚠", "{k:?} 不得复用 ⚠（§8.3 联锁专行）");
    }
}

/// TT-10 防重窗口（纯逻辑，时钟注入 ⇒ 不真等 500 ms）。
#[test]
fn debounce_window_is_500ms() {
    let d = Debounce::new(Timing::debounce());
    assert_eq!(d.window(), Duration::from_millis(500), "TT-10 窗口 = 500 ms");

    let t0 = Instant::now();
    assert!(d.try_accept(t0), "首次触发放行");
    assert!(!d.try_accept(t0 + Duration::from_millis(499)), "窗口内第二次拒绝");
    // 「拒绝**不刷新**窗口起点」由下一条**边界**断言证明：
    // 若 t0+499 的拒绝刷新了起点，窗口会顺延到 t0+999，t0+500 就该被拒；
    // 它被放行 ⇒ 起点仍是首次放行的 t0（否则"连点续命"会让按钮永不恢复）。
    assert!(
        d.try_accept(t0 + Duration::from_millis(500)),
        "自**首次**放行起满 500 ms 后放行（同时证明拒绝未刷新窗口起点）"
    );
    d.reset();
    assert!(d.try_accept(t0 + Duration::from_millis(500)), "重置后再触发放行");
    assert!(Debounce::default().window() == Duration::from_millis(500), "默认窗口取 theme");

    // Toast 存活窗口同源
    assert_eq!(Timing::toast(), Duration::from_millis(3000), "Toast 3 s");
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥ `ui/**` 静态约束（设计 §11.1 / §11.4）
// ═══════════════════════════════════════════════════════════════════════════

/// ⑥ 静态扫描：`ui/**` 不得出现裸色值调用、文本输入控件符号、直连底层绑定、强制刷新。
///
/// 符号名一律由 `concat!` **拼接**构造 —— 这样本文件（同属 `ui/**`）不会被同一条规则误伤，
/// 与 `src/lvgl/tests_a3.rs` 的既有做法一致。
/// [`strip_comments_and_literals`] 的**行数自证**：输出与输入的 `\n` 计数必须**完全一致**。
///
/// 抽成自由函数是为了让"自证"本身**能被探针直接驱动**（把剥离器改回吞行版本 ⇒ 本函数
/// 报错），而不是把判据埋在一个大函数的末尾。
fn assert_line_count_kept(name: &str, src: &str, out: &str) {
    let before = src.matches('\n').count();
    let after = out.matches('\n').count();
    assert_eq!(
        after, before,
        "{name}：剥离注释 / 字面量后**行数变了**（{before} → {after}）—— 剥离器把源码吞了。\n\
         本函数的契约是「只把注释与字面量**内容**换成空格、**换行一律原样保留**」⇒ 行数必须\n\
         **完全一致**（不设阈值：任何差异都说明有整行源码对下游若干张静态网**不可见**，\n\
         它们会静默失明 —— 评审实测：旧实现下 `theme.rs` 1214 行只剩 551 行可见、\n\
         `controls.rs` 第 629 行插入的真裸整数**点不到名**）。\n\
         请修 [`strip_comments_and_literals`]（多半是某个撇号被判成字符字面量后一路吞到\n\
         下一个撇号，成对吞行）。"
    );
}

/// Rust **字符字面量**的最大**字符数**（含两侧引号）：`'\u{10FFFF}'`
/// = `'` + `\u{` + 6 个十六进制位 + `}` + `'` = **12**。
///
/// 这是**由文法给出的硬界**（不是"看着差不多"的经验阈值）：[`strip_comments_and_literals`]
/// 的**自证 ②** 用它判定"被判成字面量的撇号其实是生命周期"—— 超界即报错并点名文件 / 行号 /
/// 被吞内容（旧实现的吞行形态长数千字符）。
const MAX_CHAR_LITERAL_LEN: usize = 12;

/// 剥离注释与字符串 / 字符字面量，供静态约束扫描使用。
///
/// 只做"扫描前预处理"，不求完备的 Rust 词法：
///
/// - **注释**（`//` / `/* … */`）、**字符串字面量**（`"…"`、`b"…"`、`c"…"`、原始串
///   `r"…"` / `r#"…"#`（**任意 N 个 `#`**）/ `br"…"` / `br#"…"#`）、**字符字面量**
///   （`'x'` / `'\n'` / `'\x41'` / `'\u{4E2D}'` / `b'x'` 的 `'…'` 部分）一律置空（内容换成
///   空格）；
/// - **原始串的语义**（Rust 文法）：**不做转义处理**（`\` 是普通字符），终止判据 =
///   `"` 后紧跟**与开启时相同个数**的 `#`。`b` / `c` 前缀只标记字面量种类，规则不变。
///   前缀识别（含**词法边界**判定，`my_r"x"` 那类不成词的形态不误判）见 [`literal_prefix`]；
/// - **三条"响亮自证"**（本函数**自己**报错，不靠下游用例才发现"网瞎了"）：
///   **① 行数恒等** —— 换行一律原样保留（含块注释里、多行原始串里、`\` 行续接处）
///   ⇒ 输出与输入的 `\n` 计数**必须完全一致**，由 [`assert_line_count_kept`] 断言；
///   **② 字符字面量长度 ≤ [`MAX_CHAR_LITERAL_LEN`]** —— 因为 ① 单独**抓不到**"撇号吞行"
///   （吞掉的内容里换行也被保留 ⇒ 行数照样相等、内容却已不见）；而"`'` 与闭引号之间的
///   跨度"有**文法硬界** 12 ⇒ 超界即坐实"被判成字面量的是生命周期撇号"；
///   **③ 闭合判据**（本单元新增）—— 见下节；
/// - **撇号 `'` 只有构成合法字符字面量时才按字面量消费**（[`is_char_literal`] 判据）：
///   `'\…'`（转义式，闭引号紧邻）与 `'x'`（单字符 + 紧跟 `'`）是字面量；其余 ——
///   `'static` / `'a` / `'_` / `'x`（后面不是 `'`）/ 落单的 `'` —— 一律**是生命周期**，
///   按**普通字符**跳过：既不进入"字面量"状态，**也不吞行**。
///
///   ⚠️ **旧实现（B1 引入）的实测危害**：它对**任何** `'` 都按"字符字面量"处理 ⇒ 遇到
///   `&'static str` / `&'a str` 这类生命周期就一路吞到**下一个撇号**（成对吞行）⇒ 被吞过的
///   源码对**全部**静态网不可见（实测行丢失：`theme.rs` 1214→551、`components.rs`
///   2039→1231、`pages/p1_status.rs` −482、`pages/p6_system.rs` −164、`pages/mod.rs` −34、
///   `ui/controls.rs` 约 −43）。后果是**诊断行号错位**（报 926 而实际在第 954 行）与
///   **新网可被静默绕过**（`controls.rs:629` 插一个真裸整数 `= 48` 完全不可见）。
///
/// ## 自证 ③（闭合判据）与其**已知边界**
///
/// ①② 对"**引号类**吞行"不敏感：`r#"a " b"#` 这类形态下，旧实现把 `r` / `#` 当普通字符、
/// 把内容里的 `"` 当成开引号 ⇒ 越过真实闭引号继续吞，而**换行全被保留** ⇒ ①② 双双抓不到
/// （上一轮验证员实测：`newlines_kept=true` 而 `sentinel_visible=false`；本单元修复前实测
/// `fn f() {\n let _a = r#"has " quote"#;\n const SENTINEL: i32 = 48;\n}` 的剥离结果 =
/// `"fn f() {\n    let _a = r#  quote\n\n\n "`，行数相等、哨兵不可见）。故新增：
///
/// - **③a 未闭合即响亮失败** —— 字符串 / 原始串 / 块注释扫到 **EOF 仍无闭合定界符** ⇒
///   `panic!`（[`unclosed`]）并点名 `文件:行` 与形态。合法 Rust 里不存在未闭合字面量 ⇒ 零误报；
/// - ~~**③b 非原始串不得跨行**~~ —— **已于工作单元 L 删除**：前提错误（Rust **允许**普通
///   字符串含裸换行，见下文"已修误报"）。删除的代价与替代控制见
///   [`strip_comments_and_literals`] 里对应分支的注释。
///
/// **残余边界（如实登记，不声称覆盖）**：若某形态的失真**恰好在同一行内**遇到下一个无关引号
/// 而闭合（例如 `let a = r#"x"#; let b = "y";` 这一行的旧实现），③ 不报错。此时被吞的仅限于
/// **同一行**内两引号之间的源码 —— 行号与行首都还在（行数断言、`文件:行` 点名仍成立），危害
/// 远小于跨行吞行；且**主干修复（[`literal_prefix`] 正确识别前缀）已使该形态不再产生**。
/// 发现新形态请补进本段，**不要默默放行**。
///
/// ## 已知边界（**非覆盖**）：块注释**可嵌套**，本实现**不嵌套**
///
/// Rust 的块注释**可嵌套**（`/* a /* b */ c */` 是**一枚**完整注释），本实现按"遇首个 `*/`
/// 即结束"处理 ⇒ ` c */` 里会残留 ` c ` 与尾随 `*/` 被当成"代码"。**后果是误报（把注释尾部
/// 当代码检查），不是漏检** —— 残留文字**仍会被下游各网看到**，最坏是"注释里写了禁词却被判
/// 违规"（可消解：改注释措辞）；**结构上不会漏检**（不会把真代码藏起来）。**不实现嵌套**的
/// 理由：`ui/**` 当前零嵌套块注释（实测），而嵌套扫描要引入深度计数与又一条"未闭合即失败"
/// 判据 —— 收益低、改动面更大。**若有人在 `ui/**` 写嵌套块注释 ⇒ 先补本判据再写**。
///
/// ## 已知边界（**曾误报，已修**）：**CRLF 行尾** ⇒ ③b 把正常源码判成"跨行字符串"
///
/// 本仓库 `core.autocrlf = true`（git 检出时把 LF 落成 `\r\n`）⇒ **Windows 全新克隆 /
/// 重新检出后的工作区全是 CRLF**。原实现在转义分支里只认 `` `\` `` **紧跟** `\n`，而
/// CRLF 下 `` `\` `` 后面是 `\r` ⇒ 行续接判据落空、控制流掉进**当时还在的** ③b 判据
/// ⇒ **合法源码被响亮误报**。2026-09-15 实测：仅仅把 `ui/shell.rs` 的换行风格由 LF 改成
/// CRLF（**内容一字未改**），`shell_static_constraints` / `ui_layout_setters_use_theme_constants` /
/// `ui_const_i32_definitions_derive_from_theme` **三处同时变红**，报
/// `ui/shell.rs:1635 —— 非原始字符串在闭合前跨了行`。
///
/// **修法**：两个转义分支统一走 [`line_continuation`]（认 `\r\n` 并把它一并推进掉）；
/// 回归用例 [`scanner_accepts_crlf_line_continuation_and_multi_line_strings`]。
///
/// ## 已知边界（**曾误报，已修**）：**裸换行在非原始串里是合法的**（工作单元 L 订正）
///
/// 同一条判据（旧 ③b）还咬到第二种合法源码：**非原始串含裸换行**。Rust **允许**普通字符串
/// 字面量跨行（独立 crate 实测：`let s = "\<LF>abc<LF>def";` 编译通过、产出 `abc\ndef`）——
/// 旧 ③b 的"唯一跨行途径是 `\` 行续接"是**错的**。首例误报 = `src/config.rs::help_text`
/// 的 53 行帮助文本（一直在用），只有当扫描面从 `ui/**` 扩到 `src/**`（工作单元 L 的
/// 架构边界网）时才暴露。**修法**：删除旧 ③b，裸换行按普通字符处理（补回换行、内容剥空）；
/// 回归用例同上（第 ② 段），并保留 ③a（EOF 未闭合仍响亮失败）。
///
/// **教训（写给下一条判据）**：③ 系列的前提是"合法 Rust 源码里不存在未闭合字面量"，
/// 而**换行风格也在这个前提内** —— 任何依赖"两字符紧邻"的判据都要先问一句
/// **"CRLF 下还紧邻吗？"**（这是本网首条、也是唯一一条**误报**形态；其余边界一律偏漏检方向）。
fn strip_comments_and_literals(src: &str, name: &str) -> String {
    strip_comments_and_literals_audited(src, name).0
}

/// 同 [`strip_comments_and_literals`] —— **同一份实现**（本函数是唯一扫描器，上面那个只是
/// 丢掉审计输出的薄壳，**不是第二份扫描器**），额外返回**被识别为原始串**的位置清单
/// `(1 起行号, `#` 个数)`。
///
/// **为什么需要这个出口**：旧 ③b 判据删除后（见 [`strip_comments_and_literals`] 的
/// "已知边界：裸换行在非原始串里是合法的"），"某类字面量**前缀**没被识别 ⇒ 把非字面量处的
/// 引号当成开引号、越过真实闭引号一路吞下去"这一失真形态**再无自动哨**：吞掉的内容里换行
/// 也被保留 ⇒ 行数自证 ① 抓不到，③a 只在"恰好扫到 EOF 仍未闭合"时才炸。本出口让下游能把
/// "**扫描面里已知的原始串是否真被认出来**"变成一条可对拍的硬判据
/// —— 见 [`scanner_recognizes_every_raw_string_on_the_scan_face`]。
fn strip_comments_and_literals_audited(src: &str, name: &str) -> (String, Vec<(usize, usize)>) {
    let cs: Vec<char> = src.chars().collect();
    let line_of = |at: usize| 1 + cs[..at.min(cs.len())].iter().filter(|&&c| c == '\n').count();
    let mut out = String::with_capacity(src.len());
    // 每枚**被 `literal_prefix` 判为原始串**的字面量记一条 `(1 起行号, `#` 个数)`。
    let mut raw_opens: Vec<(usize, usize)> = Vec::new();
    let mut i = 0usize;
    while i < cs.len() {
        let c = cs[i];

        // ── 行注释：内容置空；**换行留在原处**（行号守恒）。行注释无"未闭合"形态 ──
        if c == '/' && cs.get(i + 1) == Some(&'/') {
            while i < cs.len() && cs[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // ── 块注释：内容置空、换行保留。**不嵌套**（见函数文档"已知边界"）──
        if c == '/' && cs.get(i + 1) == Some(&'*') {
            let open = i;
            i += 2;
            let mut closed = false;
            while i < cs.len() {
                if cs[i] == '*' && cs.get(i + 1) == Some(&'/') {
                    i += 2;
                    closed = true;
                    break;
                }
                if cs[i] == '\n' {
                    out.push('\n'); // 注释内容置空，但**不吞行**
                }
                i += 1;
            }
            if !closed {
                // **自证 ③a**
                unclosed(name, line_of(open), "块注释 `/* … */` 扫到文件尾仍未闭合");
            }
            continue;
        }

        // ── 字符串字面量：普通 / 字节 / C / 原始（前缀判据见 [`literal_prefix`]）──
        //
        // **必须判在"普通字符"之前**：`r` / `b` / `c` 本身也是普通标识符字符，靠词法边界
        // （前一字符不是标识符字符）区分。
        if let Some(lp) = literal_prefix(&cs, i) {
            let open = i;
            if lp.raw {
                // 审计出口：本处**被认出是原始串**（供"前缀失认"判据对拍，见函数文档）。
                raw_opens.push((line_of(open), lp.hashes));
            }
            i = lp.quote + 1;
            let mut closed = false;
            while i < cs.len() {
                if lp.raw {
                    // 原始串：**不做转义处理**（`\` 是普通字符）；终止 = `"` 后紧跟**相同
                    // 个数**的 `#`（`r"…"` 是 N = 0 的特例）。
                    if cs[i] == '"' && (0..lp.hashes).all(|k| cs.get(i + 1 + k) == Some(&'#')) {
                        i += 1 + lp.hashes;
                        closed = true;
                        break;
                    }
                } else if cs[i] == '\\' {
                    // **行续接**（`\` + 换行）在字符串里合法（内容被续接）—— 但**行还在**，
                    // 故这里也必须补一个换行，否则"行数恒等"的自证会误报。
                    //
                    // ⚠️ 形参经 [`line_continuation`] 一并认 **CRLF**：本仓库
                    // `core.autocrlf=true` ⇒ Windows 全新克隆时每个行尾都是 `\r\n`，若此处
                    // 只认 `\n`，`\` 之后紧跟的 `\r` 会让本分支落空、控制流掉进 ③b 的裸换行
                    // 判据 ⇒ **正常源码被误报为"跨行字符串"**（见函数文档"已知边界：CRLF"）。
                    let (kept_nl, skip) = line_continuation(&cs, i);
                    if kept_nl {
                        out.push('\n');
                    }
                    i += skip;
                    continue;
                } else if cs[i] == '"' {
                    i += 1;
                    closed = true;
                    break;
                }
                // ⚠️ **旧的 ③b（"非原始串不得含裸换行 ⇒ 遇见即响亮失败"）已于工作单元 L 删除**
                // —— 它的**前提本身是错的**：Rust **允许**普通字符串字面量含裸换行
                // （`let s = "\<LF>abc<LF>def";` 合法，产出 `abc\ndef`；L 单元用独立 crate
                // 实测确认）。首例误报 = `src/config.rs::help_text` 的 53 行帮助文本（合法、
                // 在用），把扫描面从 `ui/**` 扩到 `src/**` 的当天即触发。
                // **代价（如实登记）**：失去"前缀没被识别 ⇒ 跨越真实闭引号吞行"的自动哨 ——
                // 该风险的**替代控制**是各条静态网自带的**扫描面自证**（must-contain token，
                // 见 `no_text_input_widget_symbol_anywhere_in_src` / `binding_crate_is_not_used…`），
                // 以及 [`literal_prefix`] 已逐条列出的前缀形态清单（`r` / `b` / `c` / `br` / `cr`
                // + 任意 `#`）。EOF 未闭合仍由 ③a 响亮失败。
                //
                // ⚠️ **订正（L 收尾，2026-09-18）**：上面那段"替代控制"曾把话说过头 —— **实测**
                // 往 [`literal_prefix`] 注入"`r` 前缀失认"（`if cs[i] == 'r' { return None; }`）
                // ⇒ 全量 **368 passed / 0 failed**（该红却没红），即那三条替代控制**一条都没接住**。
                // 现已**补真控制**：[`scanner_recognizes_every_raw_string_on_the_scan_face`]
                // 用"扫描面内**已知 15 处**原始串（逐文件条数 + 端到端剥空）"对拍，
                // 同款注入下**必红**（实测），是本失真的**唯一**自动哨。
                if cs[i] == '\n' {
                    out.push('\n'); // 字面量内容置空，但**不吞行**
                }
                i += 1;
            }
            if !closed {
                // **自证 ③a**
                unclosed(name, line_of(open), "字符串字面量（起引号）扫到文件尾仍未闭合");
            }
            out.push(' ');
            continue;
        }

        // ── 字符字面量（只认 [`is_char_literal`] 认可的形态）──
        if c == '\'' && is_char_literal(&cs, i) {
            let quote = c;
            let open = i;
            i += 1;
            while i < cs.len() {
                if cs[i] == '\\' {
                    // **行续接**（`\` + 换行）在字符串里是合法的（内容被续接）—— 但**行还在**，
                    // 故这里也必须补一个换行，否则"行数恒等"的自证会误报。CRLF 形态同
                    // [`line_continuation`]。
                    let (kept_nl, skip) = line_continuation(&cs, i);
                    if kept_nl {
                        out.push('\n');
                    }
                    i += skip;
                    continue;
                }
                if cs[i] == quote {
                    i += 1;
                    break;
                }
                if cs[i] == '\n' {
                    out.push('\n'); // 字面量内容置空，但**不吞行**
                }
                i += 1;
            }
            // ── **自证 ②**：撇号若真是字符字面量，其长度**必然** ≤ [`MAX_CHAR_LITERAL_LEN`] ──
            // "行数恒等"（自证 ①）**catch 不到撇号吞行**：本实现把字面量里的换行也保留
            // （那是行号准确的前提）⇒ 即便某个撇号一路吞到下一个撇号，行数照样相等、源码
            // 内容却已大片不可见。故这里再钉一条**由 Rust 文法给出的硬界**：`'` 与它的闭
            // 引号之间最多 `'\u{10FFFF}'` 共 11 个字符 ⇒ 整枚字面量的跨度 ≤ 12。超界即
            // 说明"被判成字面量的其实是生命周期撇号"（B1 旧实现的缺陷形态），**响亮报错**。
            if quote == '\'' {
                assert!(
                    i - open <= MAX_CHAR_LITERAL_LEN,
                    "{name}:{} —— 撇号被判成**字符字面量**后吞掉了 {} 个字符：\n  {}\n\
                     合法字符字面量最长 {} 个字符（`'\\u{{10FFFF}}'`，含两侧引号）⇒ 超界说明\
                     被判成字面量的其实是**生命周期撇号**（`'static` / `'a` / `'_`），必须按\
                     普通字符跳过（见 [`is_char_literal`]）。**被吞掉的源码对下游全部静态网\
                     不可见**（旧实现实测：theme.rs 1214 行只剩 551 行可见）。",
                    1 + cs[..open].iter().filter(|&&c| c == '\n').count(),
                    i - open,
                    cs[open..i].iter().collect::<String>().escape_debug(),
                    MAX_CHAR_LITERAL_LEN
                );
            }
            out.push(' ');
            continue;
        }
        out.push(c);
        i += 1;
    }
    assert_line_count_kept(name, src, &out);
    (out, raw_opens)
}

/// 前缀字面量的识别结果（[`literal_prefix`] 返回）。
struct LiteralPrefix {
    /// 开引号 `"` 在字符数组里的下标。
    quote: usize,
    /// 是否**原始串**（`r"…"` / `r#"…"#` / `br"…"` / `br#"…"#`）—— 原始串**不做转义处理**。
    raw: bool,
    /// 原始串的 `#` 个数（终止判据 = `"` 后紧跟**相同个数**的 `#`）；非原始串恒 0。
    hashes: usize,
}

/// 判 `cs[i]` 处是否开启一个**字符串字面量**；是则给出**开引号位置**与**原始串参数**。
///
/// 识别的形态（**逐条列出，不靠正则猜**）：
///
/// | 形态 | `raw` | `hashes` |
/// |------|-------|----------|
/// | `"…"` | false | 0 |
/// | `b"…"` / `c"…"`（字节串 / C 串） | false | 0 |
/// | `r"…"` | true | 0 |
/// | `r#"…"#` / `r##"…"##` / …（**任意 N 个 `#`**） | true | N |
/// | `br"…"` / `br#"…"#` / …（原始字节串） | true | N |
///
/// **词法边界**：前缀字母（`r` / `b` / `c`）的**前一个字符**不得是标识符字符（字母数字 /
/// `_`）—— 否则它是某个标识符的尾巴（`my_r"x"` 那类本就不成词），返回 `None`，由主循环按
/// 普通字符处理（其后的 `"` 仍会命中**裸引号**分支被正确消费）。**裸 `"` 不做边界判定**。
///
/// **`b'x'`（字节字符字面量）不在此列**：返回 `None`（`b` 自身是普通字符），其撇号部分由主
/// 循环的字符字面量分支经 [`is_char_literal`] 消费。
///
/// **不识别的前缀形态**（如实登记为**边界**）：`b#"…"#`（非法的字节串写法，Rust 里没有）、
/// 以及将来可能出现的新前缀。（当前 `ui/**` 零使用。）
fn literal_prefix(cs: &[char], i: usize) -> Option<LiteralPrefix> {
    match cs[i] {
        // 裸引号：无条件（`"` 前面不可能是标识符字符而仍成立字面量）。
        '"' => Some(LiteralPrefix {
            quote: i,
            raw: false,
            hashes: 0,
        }),
        'r' | 'b' | 'c' => {
            // 词法边界：前一字符若是标识符字符，则本字母属于某个标识符。
            if i > 0 && (cs[i - 1].is_alphanumeric() || cs[i - 1] == '_') {
                return None;
            }
            // `r` / `br` / `cr` ⇒ 原始串；`b` / `c` ⇒ 普通（非转义豁免）串。
            let (hashes_from, raw) = if cs[i] == 'r' {
                (i + 1, true)
            } else if cs.get(i + 1) == Some(&'r') {
                (i + 2, true)
            } else {
                (i + 1, false)
            };
            let mut j = hashes_from;
            while cs.get(j) == Some(&'#') {
                j += 1;
            }
            if cs.get(j) != Some(&'"') {
                return None;
            }
            if !raw && j != hashes_from {
                return None; // `b#"…"#` 不是合法形态（`#` 只属于原始串）
            }
            Some(LiteralPrefix {
                quote: j,
                raw,
                hashes: j - hashes_from,
            })
        }
        _ => None,
    }
}

/// `cs[i]`（=`\`）是否开启**行续接**，并给出该推进的字符数（**含反斜杠本身**）。
///
/// 返回 `(是否吃到换行, 推进字符数)`：`` `\` `` + `\n` ⇒ `(true, 2)`；
/// `` `\` `` + `\r\n` ⇒ `(true, 3)`；其余（转义普通字符）⇒ `(false, 2)`。
///
/// **两个调用点共用本判据**：字符串字面量与字符字面量里的转义分支。
/// **为什么必须认 `\r\n`**：见 [`strip_comments_and_literals`] 的
/// "已知边界（曾误报，已修）：CRLF 行尾" —— 本仓库 `core.autocrlf = true`，
/// 只认 `\n` 会把正常源码误报成"跨行字符串"。
///
/// 返回**推进字符数**而不是"有没有换行"，是为了让 `\r` 被一并消费掉：
/// 若只推进 2，`\r` 会被当成普通字符留在原地，行数守恒的账目就对不上了。
fn line_continuation(cs: &[char], i: usize) -> (bool, usize) {
    match (cs.get(i + 1), cs.get(i + 2)) {
        (Some(&'\r'), Some(&'\n')) => (true, 3),
        (Some(&'\n'), _) => (true, 2),
        _ => (false, 2),
    }
}

/// **自证 ③a 的失败出口**：某类定界符"扫到 EOF 仍未闭合" ⇒ **响亮失败**（永不返回）。
///
/// 抽成独立函数只为让调用点共用一段文案（`-> !` ⇒ 调用点不必写 `return`）。判据、为什么
/// 合法 Rust 里零误报、以及**残余边界**见 [`strip_comments_and_literals`] 的"自证 ③"一节。
///
/// ⚠️ **沿革（工作单元 L）**：原文案还断言"非原始串不得含裸换行"并据此判"跨行字符串"失真 ——
/// 该前提**为假**（Rust 允许普通串含裸换行，首例误报 = `src/config.rs::help_text`），
/// 那条判据（旧 ③b）已删除，本函数现只负责 EOF 未闭合。
fn unclosed(name: &str, line: usize, what: &str) -> ! {
    panic!(
        "{name}:{line} —— {what}。\n\
         合法 Rust 源码里**不存在**未闭合的字面量 / 块注释 ⇒ 这是**扫描器失真**（不是源码\
         问题）：多半是某类字面量**前缀**没被识别（`r\"…\"` / `r#\"…\"#` / `b\"…\"` / \
         `br#\"…\"#` / `c\"…\"`），于是把非字面量处的引号当成开引号、越过真实闭引号一路吞下去。\
         被吞掉的源码对下游**全部静态网不可见**（静默失明）。请修 [`literal_prefix`] / \
         [`strip_comments_and_literals`]（判据与前缀形态清单见二者文档）。"
    )
}

/// 判定 `cs[i]`（=`'`）是否**开启一个字符字面量**（而不是生命周期撇号）。
///
/// **只认"闭引号紧邻"的两种形态**：
///
/// - `'` + `\` + 转义体 + `'`：转义体长度**按 Rust 文法**算，其后**紧邻**必须是 `'` ——
///   `\u{…}` 取到 `}` 为止；**`\x` + 2 位十六进制位（`'\x41'`）记 4 个字符**（`\` `x` `4` `1`）；
///   其余（`\n` / `\'` / `\\` / `\0`）记 1 个字符；
/// - `'` + 一个「非 `'`、非 `\`」字符 + `'`：如 `'x'` / `'_'` / `'0'` / `'中'`。
///
/// 其余**一律判非**（按普通字符跳过，**不吞行**）：`'static` / `'a` / `'_` / `'x`（后面不是
/// 引号）/ 落单的 `'` / 空的 `''`。
///
/// ⚠️ **订正记录（Minor 1）**：旧实现把 `\x` 与 `\n` 一视同仁（"其余转义记 1 个字符"），于是
/// `'\x41'` 被判成**不是**字符字面量 ⇒ 该撇号被当普通字符、`\x41'` 残留为"代码"（**误报面**：
/// 注释 / 字面量里的 `\x41` 会被下游各网当作源码文本检查）。现按文法消费 4 个字符 ⇒ 整枚
/// 字面量跨度 6 ≤ [`MAX_CHAR_LITERAL_LEN`]。
fn is_char_literal(cs: &[char], i: usize) -> bool {
    debug_assert_eq!(cs.get(i), Some(&'\''));
    let Some(&c1) = cs.get(i + 1) else {
        return false; // 落单的 `'`（文件末尾）
    };
    if c1 == '\\' {
        // 转义体：`\u{…}` 整体跳过；`\x` + **2 位十六进制位**（4 个字符）整体跳过；
        // 其余转义（`\n` / `\'` / `\\`）跳过 1 个字符。
        let after = if cs.get(i + 2) == Some(&'u') && cs.get(i + 3) == Some(&'{') {
            match cs[i + 4..].iter().position(|&c| c == '}') {
                Some(p) => i + 4 + p + 1,
                None => return false, // 未闭合的 `\u{…}` ⇒ 不按字面量消费
            }
        } else if cs.get(i + 2) == Some(&'x')
            && cs.get(i + 3).is_some_and(|c| c.is_ascii_hexdigit())
            && cs.get(i + 4).is_some_and(|c| c.is_ascii_hexdigit())
        {
            i + 5 // `\x41` = `\` `x` `4` `1` 共 4 个字符（Minor 1）
        } else {
            i + 3
        };
        return cs.get(after) == Some(&'\'');
    }
    if c1 == '\'' {
        return false; // `''` 不是合法字符字面量（空字符）
    }
    cs.get(i + 2) == Some(&'\'')
}

/// `ui/**` 静态约束的**共用**禁用符号清单（**M6**：此前 [`ui_static_constraints`] 与
/// [`pages_static_constraints`] 逐字重复这 7 条）。
///
/// 符号名一律由 `concat!` **拼接**构造 —— 这样本文件（同属 `ui/**`）不会被同一条规则误伤，
/// 与 `src/lvgl/tests_a3.rs` 的既有做法一致。
const FORBIDDEN_UI_SYMBOLS: [&str; 7] = [
    // ④′ 裸色值调用（色值必须经 `theme` + 类型化通道）
    concat!("lv_color", "_hex"),
    concat!("lv_color", "_make"),
    // ① 零文本输入（F12 红线）
    concat!("lv_", "text", "area"),
    concat!("lv_", "key", "board"),
    concat!("lv_", "spin", "box"),
    // ⑥ 只有测试可用强制渲染
    concat!("lv_refr", "_now"),
    // ⑤ 不安全边界收敛（`ui` 不得直连底层绑定）
    concat!("lvgl", "_sys"),
];

#[test]
fn ui_static_constraints() {
    let sources: [(&str, &str); 3] = [
        ("ui/mod.rs", include_str!("mod.rs")),
        ("ui/theme.rs", include_str!("theme.rs")),
        ("ui/components.rs", include_str!("components.rs")),
    ];
    for (name, src) in sources {
        // 先剥注释与字符串/字符字面量再匹配：否则"文档注释里解释**为什么**不用
        // `lv_spinbox`"会被误判为违规（与 `src/lvgl/tests_a3.rs` / `lvgl-sys/tests/
        // allowlist_consistency.rs` 的既有做法一致）。
        let lower = strip_comments_and_literals(src, name).to_ascii_lowercase();
        for needle in FORBIDDEN_UI_SYMBOLS {
            assert!(
                !lower.contains(needle),
                "{name} 不得出现 `{needle}`（设计 §11.1/§11.4 静态约束）"
            );
        }
    }

    // 色值构造只允许出现在 `theme.rs`（页面 / 组合控件不得内联裸色值）
    let comp = include_str!("components.rs");
    let comp = strip_comments_and_literals(comp, "ui/components.rs");
    for ctor in ["Color::hex(", "Color::rgb("] {
        assert!(
            !comp.contains(ctor),
            "components.rs 不得内联色值构造 `{ctor}`（必须经 theme 的命名常量）"
        );
    }
    // 组合控件的尺寸不得内联字面量：抽查"组件直接依赖的关键尺寸"只出现在 theme
    let theme_src = include_str!("theme.rs");
    assert!(
        theme_src.contains("pub const TOUCH_MIN: i32 = 48;"),
        "四个触摸硬常量必须在 theme 里有唯一定义"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ①′ / ⑤′ 架构边界静态约束（工作单元 **L** 补缺；设计 §11.1 静态约束行 ① ⑤ + §11.4 CI 六条）
//
// **为什么必须补这两条**（L 单元覆盖度审计的实测结论，逐条 grep 复核）：
// - ⑤「`lvgl-sys` 不得被 `ui`/`state`/`channel`/`console` 直接 `use`」——
//   `FORBIDDEN_UI_SYMBOLS` 里**有** `lvgl_sys` 这一项，但它只被各条 `*_static_constraints`
//   用在**各自那几个 `ui/**` 文件**上；设计点名的 `state` / `channel` / `console` **三个模块
//   此前零覆盖**（全库 grep：无任何用例读这三个文件的源码）。
// - ①「`ui/**` **与 `src/**`** 不得引用 `lv_textarea`/`lv_keyboard`/`lv_spinbox`」——
//   此前只有 `lvgl/tests_a3.rs` 里对 **`widgets.rs` 一个文件**的扫描；
//   `src/**` 其余 20+ 个文件**零覆盖**。
// ⇒ 两条都收敛为**同一张显式文件清单**（本文件是扫描面的唯一真源，含自证段防清单腐化）。
// ═══════════════════════════════════════════════════════════════════════════

/// **薄层之外**的生产源清单（`src/**` 去掉 `src/lvgl/**`；设计 §1.1.1.2 纪律 1 的"业务模块"面）。
///
/// 逐条显式列出（不用 glob / 不靠遍历目录：`include_str!` 指错文件时**编译期**就红，
/// 而目录遍历会把"文件被删"伪装成"扫描通过"）。
const NON_THIN_LAYER_SOURCES: [(&str, &str); 26] = [
    ("src/lib.rs", include_str!("../lib.rs")),
    ("src/main.rs", include_str!("../main.rs")),
    ("src/app.rs", include_str!("../app.rs")),
    ("src/canvas.rs", include_str!("../canvas.rs")),
    ("src/channel.rs", include_str!("../channel.rs")),
    ("src/config.rs", include_str!("../config.rs")),
    ("src/console.rs", include_str!("../console.rs")),
    ("src/control_route.rs", include_str!("../control_route.rs")),
    ("src/error.rs", include_str!("../error.rs")),
    ("src/screen.rs", include_str!("../screen.rs")),
    ("src/state.rs", include_str!("../state.rs")),
    ("src/timing.rs", include_str!("../timing.rs")),
    ("src/touch.rs", include_str!("../touch.rs")),
    ("ui/mod.rs", include_str!("mod.rs")),
    ("ui/theme.rs", include_str!("theme.rs")),
    ("ui/components.rs", include_str!("components.rs")),
    ("ui/controls.rs", include_str!("controls.rs")),
    ("ui/shell.rs", include_str!("shell.rs")),
    ("ui/pages/mod.rs", include_str!("pages/mod.rs")),
    ("ui/pages/filters.rs", include_str!("pages/filters.rs")),
    ("ui/pages/p1_status.rs", include_str!("pages/p1_status.rs")),
    ("ui/pages/p2_config.rs", include_str!("pages/p2_config.rs")),
    ("ui/pages/p3_logs.rs", include_str!("pages/p3_logs.rs")),
    ("ui/pages/p4_interlock.rs", include_str!("pages/p4_interlock.rs")),
    ("ui/pages/p5_audit.rs", include_str!("pages/p5_audit.rs")),
    ("ui/pages/p6_system.rs", include_str!("pages/p6_system.rs")),
];

/// **薄层自身 + 其测试**的源清单（`src/lvgl/**`；设计 §11.1 静态约束 ① 的 `src/**` 面之另一半）。
///
/// 薄层**允许** `use lvgl_sys`（那正是它的职责）⇒ 清单 ⑤ 的检查**不含**本组；
/// 但**文本输入控件零出现**（约束 ①）对薄层同样成立（薄层也不提供文本框封装）。
const THIN_LAYER_SOURCES: [(&str, &str); 12] = [
    ("lvgl/mod.rs", include_str!("../lvgl/mod.rs")),
    ("lvgl/obj.rs", include_str!("../lvgl/obj.rs")),
    ("lvgl/style.rs", include_str!("../lvgl/style.rs")),
    ("lvgl/font.rs", include_str!("../lvgl/font.rs")),
    ("lvgl/display.rs", include_str!("../lvgl/display.rs")),
    ("lvgl/event.rs", include_str!("../lvgl/event.rs")),
    ("lvgl/indev.rs", include_str!("../lvgl/indev.rs")),
    ("lvgl/widgets.rs", include_str!("../lvgl/widgets.rs")),
    ("lvgl/tests.rs", include_str!("../lvgl/tests.rs")),
    ("lvgl/tests_a2.rs", include_str!("../lvgl/tests_a2.rs")),
    ("lvgl/tests_a3.rs", include_str!("../lvgl/tests_a3.rs")),
    ("lvgl/tests_b4.rs", include_str!("../lvgl/tests_b4.rs")),
];

/// 静态约束 ⑤（设计 §11.1 / §11.4）：**`lvgl-sys` 不得被薄层之外的生产模块直接引用**。
///
/// 判据 = 剥掉注释与字面量后，源码里**不出现** `lvgl_sys` / `lvgl-sys` 这两个 token
/// （`use lvgl_sys as sys;` 与 `lvgl_sys::LV_…` 两种形态**一并**被这条覆盖 ——
/// 只查 `use` 会漏掉全限定路径，那是本约束最常见的绕过形态）。
///
/// **改什么会让本条变红**：在 `src/state.rs`（或 `channel`/`console`/任一 `ui/**`）里写
/// `use lvgl_sys as sys;` / `lvgl_sys::lv_obj_t` —— 这正是"unsafe 边界扩散"的起点。
/// **反向探针（L 交付报告）**：往 `src/console.rs` 生产区插入一行 `use lvgl_sys as sys;` ⇒ 红。
#[test]
fn binding_crate_is_not_used_outside_the_thin_layer() {
    // 自证：清单必须真的含设计点名的四个模块（否则"扫了 26 个文件"里可能恰好漏了它们）。
    for must in ["src/state.rs", "src/channel.rs", "src/console.rs", "ui/mod.rs"] {
        assert!(
            NON_THIN_LAYER_SOURCES.iter().any(|(n, _)| *n == must),
            "扫描面自证失败：`{must}` 不在 NON_THIN_LAYER_SOURCES 内"
        );
    }
    for (name, src) in NON_THIN_LAYER_SOURCES {
        let code = strip_comments_and_literals(src, name);
        let lower = code.to_ascii_lowercase();
        for needle in [concat!("lvgl", "_sys"), concat!("lvgl", "-sys")] {
            assert!(
                !lower.contains(needle),
                "{name} 不得引用 `{needle}`（设计 §11.1 静态约束 ⑤：unsafe 边界收敛，\
                 只有 `src/lvgl/**` 薄安全层可直接触碰绑定）"
            );
        }
    }
    // **反向自证（防"扫了个空文件"）**：清单里的代表文件必须含其生产 token。
    let app = NON_THIN_LAYER_SOURCES
        .iter()
        .find(|(n, _)| *n == "src/app.rs")
        .map(|(_, s)| *s)
        .expect("src/app.rs 在清单内");
    assert!(
        app.contains("pub struct App"),
        "src/app.rs 未含 `pub struct App` —— include_str! 指错文件（本用例会构造性全绿）"
    );
}

/// 静态约束 ①（设计 §11.1 / §11.4，F12 零键盘红线）：**`ui/**` 与 `src/**` 均不得引用
/// `lv_textarea` / `lv_keyboard` / `lv_spinbox` 符号**。
///
/// 与既有网的分工（**互补，不重叠**）：
/// - 编译期：`lv_conf.h` 三者置 0 ⇒ 产物里根本不存在这些控件（**最强**，但只到"当前配置"为止）；
/// - 源码面：本用例覆盖 **`src/**` 全量**（薄层 + 业务层 + `ui/**`，共 38 个文件）；
///   `ui/**` 另由各条 `*_static_constraints` 覆盖（口径同 [`FORBIDDEN_UI_SYMBOLS`]）。
///
/// 符号名一律 `concat!` 拼接 —— 本文件同属 `src/**`，直写会被自己的规则误伤
/// （`lvgl/tests_a3.rs` 同法）。
///
/// **改什么会让本条变红**：把 P2 的步进器改成 `lv_spinbox`、或在薄层加一个 `lv_textarea`
/// 封装（后者尤其要紧：薄层一旦封装，`ui/**` 只需一次调用就绕开了"零键盘"）。
#[test]
fn no_text_input_widget_symbol_anywhere_in_src() {
    let forbidden = [
        concat!("lv_", "text", "area"),
        concat!("lv_", "key", "board"),
        concat!("lv_", "spin", "box"),
    ];
    for (name, src) in NON_THIN_LAYER_SOURCES.iter().chain(THIN_LAYER_SOURCES.iter()) {
        let lower = strip_comments_and_literals(src, name).to_ascii_lowercase();
        for needle in forbidden {
            assert!(
                !lower.contains(needle),
                "{name} 不得出现文本输入控件符号 `{needle}`（F12 / TT-02 零键盘红线；\
                 设计 §11.1 静态约束 ① 的扫描面 = `ui/**` **与** `src/**`）"
            );
        }
    }
    // **反向自证**：清单确实覆盖到薄层（否则 `src/**` 只剩业务层，约束 ① 只做了一半）。
    let widgets = THIN_LAYER_SOURCES
        .iter()
        .find(|(n, _)| *n == "lvgl/widgets.rs")
        .map(|(_, s)| *s)
        .expect("lvgl/widgets.rs 在清单内");
    assert!(
        widgets.contains("ScrollContainer"),
        "lvgl/widgets.rs 未含 `ScrollContainer` —— include_str! 指错文件"
    );
}

/// 静态约束 ②（设计 §11.1 / §11.4，§4.4.6 禁直连）：**`local-display` 的依赖图不得含
/// `mupc-intercore` / `mupc-southd` / `mupc-gateway`**（渲染进程只能经回环通道取数，
/// 不得直连核间总线 / 南向 / 北向）。
///
/// 判据 = 解析 `crates/local-display/Cargo.toml` 的**依赖键名**（`[dependencies]` 与
/// 全部 `[…dependencies]` 段；与 crate 名同形，含 path 依赖的**键**）。
/// 设计原文给的是 `cargo tree` 断言 —— 这里取**清单层**等价物：清单里没有，依赖图里就不可能有
/// （`cargo tree` 只多出传递依赖，而传递依赖的源头同样只能来自本清单）。
///
/// **自证（防解析失手 ⇒ 恒真）**：解析结果**必须**含 `display-proto` 与 `lvgl-sys`
/// （两者是这个 crate 的既有硬依赖）；解析不出 ⇒ 响亮失败，绝不允许"解析不到"伪装成"没违规"。
///
/// **改什么会让本条变红**：在 `Cargo.toml` 里加一行 `mupc-intercore = { path = "../intercore" }`。
#[test]
fn local_display_manifest_has_no_direct_dependency_on_core_crates() {
    /// 本 crate **允许**的依赖（写死；新增依赖 ⇒ 本条红 —— 渲染端的依赖面是设计 §5.1 的
    /// "刻意保持最小"承诺，新增必须过评审，不能悄悄长出来）。
    const ALLOWED: [&str; 8] = [
        "display-proto",
        "lvgl-sys",
        "serde",
        "serde_json",
        "thiserror",
        "uuid",
        "libc",
        "evdev",
    ];
    let manifest = include_str!("../../Cargo.toml");
    let mut in_deps = false;
    let mut seen: Vec<String> = Vec::new();
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            // `[dependencies]` / `[dev-dependencies]` / `[target.'cfg(..)'.dependencies]`
            in_deps = line.contains("dependencies]");
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !key.is_empty() {
            seen.push(key.to_string());
        }
    }
    // 自证：解析必须真的读到依赖（否则下面的断言是恒真的空转）。
    for must in ["display-proto", "lvgl-sys"] {
        assert!(
            seen.iter().any(|k| k == must),
            "Cargo.toml 依赖解析未读到 `{must}` —— 解析式失效（本条会构造性全绿）：{seen:?}"
        );
    }
    for forbidden in ["mupc-intercore", "mupc-southd", "mupc-gateway", "intercore", "southd"] {
        assert!(
            !seen.iter().any(|k| k == forbidden),
            "`local-display` 不得直接依赖 `{forbidden}`（设计 §11.1 静态约束 ② / §4.4.6 禁直连；\
             渲染端只能经回环读/控制通道取数）：实测依赖 = {seen:?}"
        );
    }
    // 依赖面**恰为**允许清单（防"新增一个没人审的依赖"）。
    let mut got = seen.clone();
    got.sort();
    let mut want = ALLOWED.to_vec();
    want.sort();
    assert_eq!(
        got, want,
        "`local-display` 的依赖面与设计 §5.1 的既定集合不符（新增依赖须同步本条并过评审）"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥′ 码表覆盖率（**B2a 规格评审 ③ 重写**：基线 = 生成字体的实际 cmap，待查集合 = 扫源码）
//
// 旧实现的**构造性漏判**（评审实测）：以手写常量 `ALL_TEXTS` 为待查集合、以
// `fonts/font_subset_charset.txt` 为基线 ⇒ ① 手写清单只含"已替代后的串"，源码里新写的
// 上屏字不会进清单；② 该 `.txt` 自身缺字（实测缺 `天`），且它列的 `U+2715(✕)` /
// `U+275A(❚)` 已被 `lv_font_conv` **丢弃**（`--symbols` 里有、生成的 cmap 里没有）。
// 因此"没抓到缺字"是构造使然 —— 现在两条腿都换掉。
// ═══════════════════════════════════════════════════════════════════════════

/// 生产 `ui/**` 源文件清单（**不含 `ui/tests.rs`**）。
///
/// 剔除口径：`tests.rs` 的字面量是**断言 / 诊断文案**（`assert_eq!` 的期望值、`eprintln!`
/// 的跳过说明、构造帧用的示例告警文案…），永不进 `lv_label`，且含大量 `format!` / 路径串，
/// 纳入会大面积误报。**这是唯一的整文件剔除**，其余 7 个文件全查。
///
/// B2b-1 起纳入 `ui/controls.rs`（此前只有 6 个文件）：三个输入控件同样要过**码表覆盖率**
/// 与**裸尺寸**两张网 —— 新控件里的中文列头（`年/月/日/时/分`）正是码表走查的对象。
/// B2b-2 起再纳入 `ui/pages/p2_config.rs`（P2 配置页：中文分组 / 字段 / 屏文最多的一页）。
/// B2b-3 起再纳入 `ui/pages/p4_interlock.rs`（P4 安全联锁页：三态总态词 / 触发源 / 灯卡 /
/// 就地原因 / 弹层屏文）。
/// B2c-1 起再纳入 `ui/pages/filters.rs`（共享时间范围筛选件：三档文案 + 起止名）与
/// `ui/pages/p5_audit.rs`（P5 审计页：头部三条 / 表头 / 行内容 / 底部状态行）。
/// B2c-2 起再纳入 `ui/pages/p3_logs.rs`（P3 日志页：通道条 / 筛选区 / 表头 / 行内容 /
/// 状态行 / 说明行 / 「回到最新」按钮）。
/// B2c-3 起再纳入 `ui/shell.rs`（应用外壳：页眉返回键 / 页标题 / 通道胶囊 / 触摸角标 /
/// 倒计时胶囊 / 未保存提示条 / 6 个导航页签）。
const UI_PROD_SOURCES: [(&str, &str); 13] = [
    ("ui/mod.rs", include_str!("mod.rs")),
    ("ui/theme.rs", include_str!("theme.rs")),
    ("ui/components.rs", include_str!("components.rs")),
    ("ui/controls.rs", include_str!("controls.rs")),
    ("ui/shell.rs", include_str!("shell.rs")),
    ("ui/pages/mod.rs", include_str!("pages/mod.rs")),
    ("ui/pages/filters.rs", include_str!("pages/filters.rs")),
    ("ui/pages/p1_status.rs", include_str!("pages/p1_status.rs")),
    ("ui/pages/p2_config.rs", include_str!("pages/p2_config.rs")),
    ("ui/pages/p3_logs.rs", include_str!("pages/p3_logs.rs")),
    ("ui/pages/p4_interlock.rs", include_str!("pages/p4_interlock.rs")),
    ("ui/pages/p5_audit.rs", include_str!("pages/p5_audit.rs")),
    ("ui/pages/p6_system.rs", include_str!("pages/p6_system.rs")),
];

/// **`ui/**` 之外**、但同样**产出上屏文案**的生产源（设计 §11.1 码表覆盖率行原文：
/// "静态扫描 **`ui/**` + `state.rs`** 中的中文字面量"）。
///
/// **为什么此前只有 `ui/**`**：`state.rs` 的模块头自陈"文案字面量的唯一落点在 `ui/**`，
/// 本层只转出"，且 `control_texts_are_aliases_of_existing_ui_literals` 等用例逐条比对过
/// **别名**关系。但"逐条比对"只覆盖**被点名的那几个出口** —— `dash_badge`（`未取数` /
/// `源离线` / `数据异常` 三个屏上角标词）的**唯一**断言是 `dash_badge_words_match_ui`
/// （拿函数与自己的字面量比 ⇒ **自证式恒真**，对"缺字"零判别力），而它是 `p1_status.rs`
/// 生产路径（`card.reason_chip.set_text(state::dash_badge(flag))`）直接上屏的字符串。
/// 工作单元 L 的覆盖度审计据此把本文件纳入扫描面（设计原文即如此要求）。
const UI_ADJACENT_PROD_SOURCES: [(&str, &str); 1] = [("src/state.rs", include_str!("../state.rs"))];

/// **非屏显出口**白名单：紧跟这些 token 的字符串字面量**不会**被画到屏上（逐条列出，
/// 不靠正则猜）。除此之外**一律**当上屏候选查（宁可多查）：
///
/// - `InvalidArgument(` —— `LvglError` 的错误消息（原型 `LvglError::InvalidArgument("…")`），
///   只在 `Err` 里流转；薄层没有任何"把 `LvglError` 画上屏"的路径 ⇒ 从不屏显；
/// - `debug_struct(` / `.field(` —— `std::fmt::Debug` 实现的字段名（供日志 / 断言阅读）；
/// - `env!(` / `option_env!(` —— 环境变量**键名**（屏上取到的是它的**值**，键名不屏显）；
/// - `stderr(),` —— 标准错误诊断（原型 `writeln!(std::io::stderr(), "…")`）。**为何它是
///   非屏显出口**：stderr 的内容从不进 LVGL 部件，**结构上不可能**成为上屏文案；若不登记，
///   诊断文案会被本网按"上屏候选"逐字要求字形齐备，逼得实现者用 `!` / `.` 之类去凑一份
///   仅 cmap 内的生硬措辞 —— 那是对**判据**的迁就，不是对**屏显**的保证。
///   其它诊断形态（`eprintln!(` / `eprint!(` 等）当前未用到，**用到时各自登记一条**
///   （本清单是**后缀匹配**，且**逐条列出、不靠正则猜**）。
///
/// 另有两类"不是字面量文本"的排除（写在 [`ui_source_chars`] 里）：
/// ① `format!` 模板的 `{…}` 占位符内容（`"{d} 日"` 里 `d` 不是字形，值才是）；
/// ② **测试模块区**（`#[cfg(test)]` + 紧随的 `mod tests`，各文件都在文件末尾）—— 判据是
/// "属性**之后紧跟**测试模块"而非"出现过该 token"，故注释里的 token 与生产区的
/// `#[cfg(test)] pub fn …` 测试访问器**都在扫描面内**（见 [`truncate_before_test_module`]）；
/// 形态若不符即响亮失败。
///
/// **B2b-2 新增一条 `config_key(`**：`ui/pages/p2_config.rs` 的**标签口径覆盖表**必须逐条写出
/// 配置字段的**机器键**（`gateway.listen_addr` 一类），而键是**小写 ASCII 且从不上屏**
/// （`g`/`a`/`t`/`e`/… 在生成字体里没有字形）。经该常量函数标注即声明"此字面量不进 `lv_label`"
/// ——与 `InvalidArgument(` / `env!(` **同一条**非屏显口径（**不改变任何上屏文案**，
/// 只是把"键不上屏"这一事实写在调用点）。
///
/// **B2b-3 新增一条 `source_key(`**：同款机制、同款理由 —— `ui/pages/p4_interlock.rs` 的
/// **触发源名映射表**必须写出**机器名 token**（`estop` / `door`），而 token 是**小写 ASCII
/// 且从不上屏**（上屏的是中文名 `急停` / `门禁`，未登记名经 `display_safe` 归一）。
/// 该文件里 `source_key("…")` 的**计数由 `p4_static_constraints` 钉死为恰 2 处**（防把**上屏串**
/// 塞进 `source_key(..)` 从而静默逃过码表网 —— 与 `config_key(` 同一条自证纪律）。
///
/// **B2c-1 新增一条 `audit_key(`**：同款机制、同款理由 —— `ui/pages/p5_audit.rs` 的
/// **审计 `target` 键映射表**必须写出**机器键**（4 个配置字段键：`gateway.port` /
/// `system.log_level` / `telemetry.interval` / `gateway.listen_addr`；2 个联锁键：
/// `interlock.release` / `interlock.ack_m1`），而键是**小写 ASCII 且从不上屏**
/// （已登记键上屏的是中文标签；**未登记键**上屏的是 `display_safe` **归一后**的形态，
/// 也不是原始小写键，见该文件的 **AU9**）。该文件里 `audit_key("…")` 的**计数由
/// `p5_static_constraints` 钉死为恰 6 处**。
///
/// ⚠️ **边界（B2b-3 代码质量整改 M7，如实登记）**：本清单是**后缀匹配**（`prefix.ends_with(s)`），
/// 且上面的计数**只数 `p4_interlock.rs` / `p5_audit.rs` 各自一个文件**。⇒ 两点已知面：
/// ① 任何**以这些后缀结尾的标识符**（如 `my_source_key("…")`）同样被豁免 —— 豁免面比
///    "调用该页那个常量函数"更宽；
/// ② 别的文件里写 `source_key("上屏串")` / `audit_key("上屏串")` **不受**计数约束。
/// 本批**不修**（收紧需把后缀匹配改成"精确调用点集合"，属另一批的结构变更）；此处**登记**以免
/// 后人把这两条当成"已被网住"。
/// **B2c-2 新增一条 `module_key(`**：同款机制、同款理由 —— `ui/pages/p3_logs.rs` 的
/// **日志模块名映射表**必须写出**机器名 token**（`intercore` / `gateway` / `audit`），而
/// token 是**小写 ASCII 且从不上屏**（上屏的是中文名 `核间` / `主站` / `审计`，未登记名经
/// `display_safe` 归一）。该文件里 `module_key("…")` 的计数由 `p3_static_constraints`
/// 钉死为恰 3 处。
///
/// **L 单元新增一条 `#[must_use =`**：把扫描面扩到 `src/state.rs`（[`UI_ADJACENT_PROD_SOURCES`]）
/// 后首个命中的就是它 —— `#[must_use = "本地合成回执必须送进页面（…）"]`。它是**编译器诊断**
/// （`unused_must_use` 的提示语），与 `env!(` / `stderr(),` **同一条**非屏显口径：
/// **结构上不可能**成为上屏文案。若不登记，它会按"上屏候选"被逐字要求字形齐备
/// （`（` `）` `；` 均不在 cmap 内）—— 那是对**判据**的迁就，不是对**屏显**的保证。
///
/// **T21c-1 新增一条 `group_title(`**：同款机制、同款理由 —— `ui/pages/p4_interlock.rs` 的
/// **分组标题查找**必须写出**分组键**（`fire_level` / `fire_sys_status` / …），而键是
/// **小写 ASCII 且从不上屏**（上屏的是它查出来的中文标题；键本身在生成字体里没有字形）。
/// 与 `config_key(` / `source_key(` 同一条自证纪律：`p4_static_constraints` 钉死本文件里
/// `group_title("…")`（恰 6 处）与 `block_key("…")`（恰 12 处）的**调用点计数**（防把**上屏串**
/// 塞进这两个函数从而静默逃过码表网）。
/// ⚠️ **T21c-2 补一条**：`machine_key(`（`p6_system.rs` 的机器键豁免口，与 `block_key(` 同款）。
/// 该函数的**实参必须纯 ASCII**（`[a-z0-9_]`）由 `p6_static_constraints` 钉住。
const NON_DISPLAY_SINKS: [&str; 14] = [
    "machine_key(",
    "InvalidArgument(",
    "debug_struct(",
    ".field(",
    "stderr(),",
    "env!(",
    "option_env!(",
    "config_key(",
    "source_key(",
    "audit_key(",
    "module_key(",
    "group_title(",
    "block_key(",
    "#[must_use =",
];

/// **已登记**的字库缺口：扫源码确实用到、但生成字体的 cmap 里**没有**的字形。
///
/// **现为空**：B2a 收尾时实测 `format_uptime` 的 `天`（唯一缺字）不在 cmap 内，已按
/// `ui/pages/mod.rs` 偏差登记 **D5** 改为在 cmap 内的同义词 `日`（`1 日` = 1 天）
/// ⇒ 源码层不再有缺字。**本清单必须与实测缺字集合相等**（新增缺字 ⇒ 红；字库补齐后
/// 条目未删 ⇒ 也红 —— **有意**如此：防止登记腐化）。
const KNOWN_MISSING: [(char, &str); 0] = [];

/// 从 cmap 结构体文本片段里取**十进制**字段（如 `.range_start = 32`）。
///
/// 字段缺失 ⇒ **响亮 `panic`**（不返回默认值）：形态变了就必须有人来改解析器，而不是让
/// "读到 0" 伪装成合法取值。
fn cmap_field_u32(chunk: &str, key: &str, name: &str) -> u32 {
    let at = chunk.find(key).unwrap_or_else(|| {
        panic!("{name}：cmap 结构体缺字段 `{key}` —— 产物形态已变，请同步更新本解析器（不得静默跳过）")
    });
    chunk[at + key.len()..]
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("{name}：`{key}` 后不是十进制整数（该字段应为十进制，请复核解析）"))
}

/// 从 cmap 结构体文本片段里取**标识符**字段（如 `.unicode_list = unicode_list_1` / `NULL`）。
fn cmap_field_ident<'a>(chunk: &'a str, key: &str, name: &str) -> &'a str {
    let at = chunk.find(key).unwrap_or_else(|| {
        panic!("{name}：cmap 结构体缺字段 `{key}` —— 产物形态已变，请同步更新本解析器（不得静默跳过）")
    });
    let tail = chunk[at + key.len()..].trim_start();
    let n = tail
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(tail.len());
    &tail[..n]
}

/// 取 `static const <decl> <array_name>[] = { … };` 数组体里的**全部整数**。
///
/// **两种进制都要认**：`unicode_list` 写成十六进制（`0x…`）、`glyph_id_ofs_list` 写成十进制
/// （`lv_font_conv` 的实际写法）⇒ 只认十六进制会**漏读** `glyph_id_ofs_list`。
///
/// **必须从声明行之后的 `{` 起切数组体**：数组名（`unicode_list_1`）与 `uint8_t` 里都含数字，
/// 从声明行起切会把它们当成元素读进来（实测会多出 2 个假元素）。
fn c_array_u32(src: &str, decl: &str, array_name: &str, name: &str) -> Vec<u32> {
    let head = format!("static const {decl} {array_name}[]");
    let at = src.find(&head).unwrap_or_else(|| {
        panic!(
            "{name}：找不到数组声明 `{head}` —— `lv_font_conv` 产物形态与解析器脱节\
             （请按 LVGL `get_glyph_dsc_id` 的语义同步更新；**不得静默跳过**）"
        )
    });
    let open = at
        + src[at..]
            .find('{')
            .unwrap_or_else(|| panic!("{name}：`{head}` 声明后找不到 `{{`"));
    let end = open
        + src[open..]
            .find("};")
            .unwrap_or_else(|| panic!("{name}：`{array_name}[]` 数组体未闭合"));
    let body = &src[open + 1..end];
    let b = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'0' && i + 1 < b.len() && (b[i + 1] | 0x20) == b'x' {
            let mut j = i + 2;
            while j < b.len() && b[j].is_ascii_hexdigit() {
                j += 1;
            }
            if let Ok(v) = u32::from_str_radix(&body[i + 2..j], 16) {
                out.push(v);
            }
            i = j;
        } else if b[i].is_ascii_digit() {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if let Ok(v) = body[i..j].parse::<u32>() {
                out.push(v);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// 从 `lv_font_noto_sc_*.c` 的 `cmaps[]` 解析**全部 `(码位, glyph_id)` 条目**。
///
/// **本解析器是 [`font_cmap_from_c`] 与 [`adv_w_from_c`] 的公用底座**（两处口径必须同源，
/// 否则一条网说"覆盖"、另一条说"宽度缺" —— 正是要避免的漂移）。
/// 语义**逐条照抄 LVGL** `get_glyph_dsc_id`
/// （`vendor/lvgl/src/font/fmt_txt/lv_font_fmt_txt.c:280-330`）：
///
/// | cmap 类型 | 码位 | glyph_id |
/// |---|---|---|
/// | `FORMAT0_TINY`  | `range_start + rcp`，`rcp ∈ 0..range_length` | `glyph_id_start + rcp` |
/// | `FORMAT0_FULL`  | `range_start + rcp`，**但 `glyph_id_ofs_list[rcp] == 0 && rcp != 0` ⇒ 无字形、跳过** | `glyph_id_start + glyph_id_ofs_list[rcp]` |
/// | `SPARSE_TINY`   | `range_start + unicode_list[i]` | `glyph_id_start + i` |
/// | `SPARSE_FULL`   | `range_start + unicode_list[i]` | `glyph_id_start + glyph_id_ofs_list[i]` |
///
/// **产物形态（2026-09-25 订正，实测 10 档，不是猜的）**：`cmaps[]` **可以是 N 个**（N ≥ 1），
/// 且**每个 cmap 类型可不同**。T21a 把码表从 326 扩到 464 字符（U-73 外设上屏的 `ppm` / `kvar`
/// / `Hz` / `PACK` 等单位所需小写拉丁字母）后，ASCII 段变"密" ⇒ `lv_font_conv` **不再**产出
/// 单个 `SPARSE_TINY`，而是切成
///
/// ```c
/// static const uint8_t  glyph_id_ofs_list_0[] = { 0, 1, 0, /* …十进制… */ };
/// static const uint16_t unicode_list_1[]       = { 0x0, 0x2, /* …十六进制… */ };
/// static const lv_font_fmt_txt_cmap_t cmaps[] = {
///     { .range_start = 32, .range_length = 56, .glyph_id_start = 1,
///       .unicode_list = NULL, .glyph_id_ofs_list = glyph_id_ofs_list_0, .list_length = 56,
///       .type = LV_FONT_FMT_TXT_CMAP_FORMAT0_FULL },
///     { .range_start = 97, .range_length = 65193, .glyph_id_start = 39,
///       .unicode_list = unicode_list_1, .glyph_id_ofs_list = NULL, .list_length = 423,
///       .type = LV_FONT_FMT_TXT_CMAP_SPARSE_TINY } };
/// ```
///
/// ⚠️ **数组名不固定**：旧形态是 `unicode_list_0`，本次是 `unicode_list_1` ⇒ **按声明行找、
/// 不按固定名字找**。⚠️ **`glyph_id` 是全局的**（跨 cmap 连续编排），`adv_w` 必须用**真实
/// `glyph_id`** 去索引 `glyph_dsc[]`，不能再用"第 i 个 = i+1"的特例。
///
/// **漂移检测的口径一致性（强制）**：本函数与 `fonts/gen_fonts.sh` 里的 Python
/// `cmap_entries()` **必须逐条一致**（后者产出入库清单 `lv_font_cmap.txt` /
/// `lv_font_metrics.txt`）；产物形态再变（如新增 cmap 类型）⇒ **两侧都要改**，否则
/// `load_font_cmap` 的漂移检测会红。
///
/// 形态不符 / 找不到 `cmaps[]` / 单条 cmap 解析不出 / 未知 cmap 类型 ⇒ **响亮 `panic`**，
/// **绝不**静默跳过或返回空集 —— 否则"解析不到"会伪装成"全部覆盖"（正是要修的那类缺陷）。
fn cmap_entries_from_c(src: &str, name: &str) -> Vec<(u32, usize)> {
    const HEAD: &str = "static const lv_font_fmt_txt_cmap_t cmaps[]";
    const T_FORMAT0_TINY: &str = "LV_FONT_FMT_TXT_CMAP_FORMAT0_TINY";
    const T_FORMAT0_FULL: &str = "LV_FONT_FMT_TXT_CMAP_FORMAT0_FULL";
    const T_SPARSE_TINY: &str = "LV_FONT_FMT_TXT_CMAP_SPARSE_TINY";
    const T_SPARSE_FULL: &str = "LV_FONT_FMT_TXT_CMAP_SPARSE_FULL";

    let at = src.find(HEAD).unwrap_or_else(|| {
        panic!(
            "{name}：找不到 cmap 数组声明 `{HEAD}` —— `lv_font_conv` 产物形态与解析器脱节\
             （请按 LVGL `get_glyph_dsc_id` 的语义同步更新；**不得静默跳过**）"
        )
    });
    let open = at
        + src[at..]
            .find('{')
            .unwrap_or_else(|| panic!("{name}：`{HEAD}` 后找不到 `{{`"));
    let end = open
        + src[open..]
            .find("};")
            .unwrap_or_else(|| panic!("{name}：`cmaps[]` 数组体未闭合"));
    let body = &src[open + 1..end];

    // `(码位, glyph_id)`（`rcp` = 相对码位；`gid` = 该 cmap 语义下的**全局** glyph id）。
    let entry = |rs: u32, rcp: u32, gid: usize| -> (u32, usize) {
        let cp = rs.checked_add(rcp).unwrap_or_else(|| {
            panic!("{name}：码位溢出（range_start = {rs} + 相对码位 {rcp}）—— 产物形态异常")
        });
        (cp, gid)
    };

    let mut entries: Vec<(u32, usize)> = Vec::new();
    let mut cmap_count = 0usize;
    for chunk in body.split('}') {
        if !chunk.contains(".range_start") {
            continue;
        }
        cmap_count += 1;
        let rs = cmap_field_u32(chunk, ".range_start =", name);
        let rl = cmap_field_u32(chunk, ".range_length =", name);
        let gis = cmap_field_u32(chunk, ".glyph_id_start =", name);
        let list_len = cmap_field_u32(chunk, ".list_length =", name);
        let ul = cmap_field_ident(chunk, ".unicode_list =", name);
        let ofs = cmap_field_ident(chunk, ".glyph_id_ofs_list =", name);
        let ty = cmap_field_ident(chunk, ".type =", name);
        match ty {
            T_FORMAT0_TINY => {
                for rcp in 0..rl {
                    entries.push(entry(rs, rcp, (gis as usize) + rcp as usize));
                }
            }
            T_FORMAT0_FULL => {
                let tbl = c_array_u32(src, "uint8_t", ofs, name);
                assert!(
                    tbl.len() == rl as usize,
                    "{name}：FORMAT0_FULL 的 `{ofs}` 长度（{}）≠ range_length（{rl}）—— 产物形态异常",
                    tbl.len()
                );
                for (rcp, ofs_rcp) in tbl.iter().enumerate() {
                    // LVGL 原文：首字符必有效、其 offset 恒 0；其余位置 offset == 0 ⇒ **该码位无字形**。
                    if *ofs_rcp == 0 && rcp != 0 {
                        continue;
                    }
                    entries.push(entry(rs, rcp as u32, (gis as usize) + *ofs_rcp as usize));
                }
            }
            T_SPARSE_TINY => {
                let lst = c_array_u32(src, "uint16_t", ul, name);
                assert!(
                    lst.len() == list_len as usize,
                    "{name}：SPARSE_TINY 的 `{ul}` 长度（{}）≠ list_length（{list_len}）—— 产物形态异常",
                    lst.len()
                );
                for (i, v) in lst.iter().enumerate() {
                    entries.push(entry(rs, *v, (gis as usize) + i));
                }
            }
            T_SPARSE_FULL => {
                let lst = c_array_u32(src, "uint16_t", ul, name);
                let o16 = c_array_u32(src, "uint16_t", ofs, name);
                assert!(
                    o16.len() >= lst.len(),
                    "{name}：SPARSE_FULL 的 `{ofs}` 长度（{}）< `{ul}` 长度（{}）—— 产物形态异常",
                    o16.len(),
                    lst.len()
                );
                for (i, v) in lst.iter().enumerate() {
                    entries.push(entry(rs, *v, (gis as usize) + o16[i] as usize));
                }
            }
            other => panic!(
                "{name}：未知 cmap 类型 `{other}` —— 解析器与生成物脱节，请按 LVGL \
                 `get_glyph_dsc_id` 的语义同步更新（**不得静默跳过**）"
            ),
        }
    }
    assert!(
        cmap_count >= 1 && !entries.is_empty(),
        "{name}：`cmaps[]` 解析出 {cmap_count} 个 cmap / {} 条码位条目 —— 解析器与生成物脱节\
         （**不得静默跳过**；旧版断言精神：形态已变请据此更新）",
        entries.len()
    );
    entries
}

/// 从 `lv_font_noto_sc_*.c` 解析**实际支持的码点集合**（基于 [`cmap_entries_from_c`]）。
///
/// 码位语义、N 个 cmap、四种类型与产物形态说明**见 [`cmap_entries_from_c`]**（公用底座，
/// 两处口径必须同源）。形态与解析前提不符 / 解析出 0 个码点 ⇒ **响亮失败**，不静默跳过。
fn font_cmap_from_c(src: &str, name: &str) -> std::collections::BTreeSet<char> {
    let mut set = std::collections::BTreeSet::new();
    for (cp, _gid) in cmap_entries_from_c(src, name) {
        let ch = char::from_u32(cp).unwrap_or_else(|| {
            panic!("{name}：码位 U+{cp:04X} 不是合法 Unicode 标量 —— 产物异常或解析器脱节")
        });
        set.insert(ch);
    }
    assert!(
        !set.is_empty(),
        "{name}：cmaps[] 解析出 0 个码点 —— 解析器与生成物脱节（不得静默跳过）"
    );
    set
}

// ─────────────────────────────────────────────────────────────────────────────
// `cmap_entries_from_c` 的**合成入参**单测：覆盖本仓 10 档产物**从不出现**的两条分支
// ─────────────────────────────────────────────────────────────────────────────
//
// 背景（T21a 评审 (D)-20 / 建议 G-8）：本仓 10 档 `.c` 实测只出现 `FORMAT0_FULL`
// + `SPARSE_TINY` ⇒ `FORMAT0_TINY` / `SPARSE_FULL` 两条分支**只经代码走查、零执行**。
// 用**手写的最小 `.c` 片段**当纯函数入参即可覆盖（不依赖 `lv_font_conv` / 不依赖产物存在），
// 成本极低。**只加测、不改生产逻辑。**

/// `FORMAT0_TINY`：`码位 = range_start + rcp`、`glyph_id = glyph_id_start + rcp`
/// （`rcp ∈ 0..range_length`，**无** `glyph_id_ofs_list`、**不跳任何码位**）。
#[test]
fn cmap_format0_tiny_maps_range_start_plus_rcp() {
    let src = r#"
static const lv_font_fmt_txt_cmap_t cmaps[] = {
    { .range_start = 32, .range_length = 4, .glyph_id_start = 7,
      .unicode_list = NULL, .glyph_id_ofs_list = NULL, .list_length = 4,
      .type = LV_FONT_FMT_TXT_CMAP_FORMAT0_TINY } };
"#;
    assert_eq!(
        cmap_entries_from_c(src, "synthetic_f0_tiny"),
        vec![(32, 7), (33, 8), (34, 9), (35, 10)],
        "FORMAT0_TINY：码位 = range_start + rcp、glyph_id = glyph_id_start + rcp"
    );
}

/// `SPARSE_FULL`：`码位 = range_start + unicode_list[i]`、
/// `glyph_id = glyph_id_start + glyph_id_ofs_list[i]`（**两张表按下标配对**，
/// 与 `SPARSE_TINY` 的"glyph_id = glyph_id_start + i"不同 —— 用**非递增**的 ofs 表
/// 才能区分二者，故此处刻意取 `[3, 1, 0]`）。
///
/// ⚠️ **入参约定**：`range_start` 必须是**十进制**（两侧解析器一致：Rust `cmap_field_u32`
/// 只认十进制数字，`gen_fonts.sh` 的 Python 正则也是 `= (\d+)`）。`lv_font_conv` 的实际写法
/// 就是十进制（如 `.range_start = 32` / `= 97`）⇒ 这里按同一约定构造，不引入产物里没有的形态。
///
/// `unicode_list` = `{0x0, 0x5, 0x9}`（十六进制，`lv_font_conv` 的写法），
/// `glyph_id_ofs_list` = `{3, 1, 0}`（十进制，同产物写法）⇒ 顺带覆盖"两种进制都要读"。
#[test]
fn cmap_sparse_full_pairs_unicode_list_with_glyph_id_ofs_list() {
    let src = r#"
static const uint16_t unicode_list_0[] = { 0x0, 0x5, 0x9 };
static const uint16_t glyph_id_ofs_list_0[] = { 3, 1, 0 };
static const lv_font_fmt_txt_cmap_t cmaps[] = {
    { .range_start = 19968, .range_length = 65193, .glyph_id_start = 11,
      .unicode_list = unicode_list_0, .glyph_id_ofs_list = glyph_id_ofs_list_0,
      .list_length = 3,
      .type = LV_FONT_FMT_TXT_CMAP_SPARSE_FULL } };
"#;
    assert_eq!(
        cmap_entries_from_c(src, "synthetic_sparse_full"),
        vec![(19968, 14), (19973, 12), (19977, 11)],
        "SPARSE_FULL：glyph_id 必须取 glyph_id_ofs_list[i]（不是 i）"
    );
}

/// 未知 cmap 类型 ⇒ **响亮 `panic`**（"解析不到"不得伪装成"全部覆盖"）。
#[test]
#[should_panic(expected = "未知 cmap 类型")]
fn cmap_unknown_type_panics_loudly() {
    let src = r#"
static const lv_font_fmt_txt_cmap_t cmaps[] = {
    { .range_start = 32, .range_length = 4, .glyph_id_start = 7,
      .unicode_list = NULL, .glyph_id_ofs_list = NULL, .list_length = 4,
      .type = LV_FONT_FMT_TXT_CMAP_SPARSE_FULL_V2_FUTURE } };
"#;
    let _ = cmap_entries_from_c(src, "synthetic_unknown_type");
}

/// 剥掉 `format!` 模板里的 `{…}` 占位符（其内容是**表达式**，屏上出现的是它的**值**）。
fn strip_format_placeholders(lit: &str) -> String {
    let mut out = String::with_capacity(lit.len());
    let mut depth = 0u32;
    for ch in lit.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            _ => out.push(ch),
        }
    }
    out
}

/// 掐掉**测试模块**（`#[cfg(test)]` + 其后紧跟的 `mod tests` 项）**及其后**的全部内容，
/// 只留**生产区**（[`ui_source_chars`] 的扫描面）。
///
/// **判据（B3 订正）**：截断点 = **第一个"`#[cfg(test)]` 之后（跳过空白与可见性修饰）就是
/// `mod tests`"的属性项**，而**不是**"文件里首个 `#[cfg(test)]`"。旧判据（首个出现处 +
/// "其后某处含 `mod tests`"守卫）有两个真实反例：
///
/// ① 生产区**本来就有** `#[cfg(test)] pub fn …` 形态的**仅测试可见访问器**
///    （`controls.rs` 的 `matrix()` / `segment()` / `column()` / `fallback_index()`）；
/// ② **注释里写出这个 token 串**同样命中（`controls.rs` 头部与上述访问器的文档注释都有）。
///
/// ⇒ 旧判据会把截断点提到生产区中部甚至**文件头**（注释反例），其后的生产字面量**全部漏扫**
/// —— 网看似还在、实则漏了一大片（实测：头部注释里出现该串 ⇒
/// `ui_texts_covered_by_font_cmap` 误报「清册条目 `年` 未出现」）。本判据对注释里的 token
/// **免疫**（注释里该串之后跟的是注释文字，不是 `mod tests`）。
///
/// ⇒ **生产文件里的 `#[cfg(test)] pub fn …` 测试访问器仍在扫描面内**（截断点在其之后），
/// **这是有意的**：它们是生产文件的一部分，其字面量同样是上屏候选。
///
/// 形态已变（文件**有** `#[cfg(test)]` 却**无**其后紧跟 `mod tests` 的项：测试模块被改名 /
/// 与属性之间**夹了注释或其他属性** / 可见性修饰不在下方已列形态内）⇒ **响亮 assert 失败**，
/// 不静默放过；纯生产文件（全文件无 `#[cfg(test)]`，如 `theme.rs`）⇒ 扫描**整个文件**（原行为）。
fn truncate_before_test_module<'a>(src: &'a str, name: &str) -> &'a str {
    const ATTR: &str = "#[cfg(test)]";
    let mut any_attr = false;
    for (i, _) in src.match_indices(ATTR) {
        any_attr = true;
        let tail = src[i + ATTR.len()..].trim_start();
        // 允许**可见性修饰**：`ui/mod.rs` 的测试模块写作 `pub(crate) mod tests;`
        // （LVGL 侧唯一 `#[test]` 需在同一线程调起它），故不能只认裸 `mod tests`。
        //
        // 形态清单（**本单元补齐 `pub(in …)`**）：`pub` / `pub(crate)` / `pub(super)` /
        // `pub(self)` / `pub(in <路径>)`。前四者剥前缀即可；`pub(in …)` 剥掉 `pub` 后余下
        // `(in <路径>) …`，再取到匹配的 `)` 之后。
        let tail = ["pub(crate)", "pub(super)", "pub(self)", "pub"]
            .iter()
            .find_map(|p| tail.strip_prefix(*p))
            .map(str::trim_start)
            .unwrap_or(tail);
        let tail = match tail.strip_prefix("(in").map(str::trim_start) {
            Some(rest) => match rest.find(')') {
                // 形态合法 ⇒ 跳到闭括号之后；不合法 ⇒ **保持原样**（下面 starts_with 不成立，
                // 落到响亮 assert，而不是静默误截断）。
                Some(close) => rest[close + 1..].trim_start(),
                None => tail,
            },
            None => tail,
        };
        if tail.starts_with("mod tests") {
            return &src[..i];
        }
    }
    assert!(
        !any_attr,
        "{name}：本文件有 `#[cfg(test)]` 但**无**其后紧跟 `mod tests` 的项 —— 本扫描\
         「截断到测试模块之前」的前提不成立。成因（逐条自查）：① 测试模块已**改名**；\
         ② `#[cfg(test)]` 与 `mod tests` **之间夹了注释**；③ 二者之间**夹了其他属性**\
         （如 `#[allow(...)]`）；④ 可见性修饰不在 `pub` `pub(crate)` `pub(super)` \
         `pub(self)` `pub(in …)` 之内（形态清单见本函数体内注释）。\
         请改本扫描工具（不得静默放过）"
    );
    src
}

/// **M6′**：[`truncate_before_test_module`] 的**形态清单**自证 ——
/// ① 五种可见性修饰（含本单元补的 `pub(in …)`）都必须被接受；
/// ② 生产区里的 `#[cfg(test)] pub fn …` 访问器**不得**成为截断点；
/// ③ `#[cfg(test)]` 与 `mod tests` 之间夹注释 ⇒ **响亮失败**（不静默误截断）。
///
/// 纯逻辑（不触碰 LVGL）。**敏感性**：把可见性清单退回只认 `pub` / `pub(crate)` ⇒
/// `pub(in crate::ui)` 那一轮变红；把"最末 assert"删掉 ⇒ ③ 变红（静默放过）。
#[test]
fn truncate_before_test_module_forms_are_accepted_or_loud() {
    let keep = "const PROBE: i32 = 1;\n";
    for vis in [
        "",
        "pub ",
        "pub(crate) ",
        "pub(super) ",
        "pub(self) ",
        "pub(in crate::ui) ",
    ] {
        let src = format!("{keep}#[cfg(test)]\n{vis}mod tests {{\n}}\n");
        assert_eq!(
            truncate_before_test_module(&src, "probe.rs"),
            keep,
            "可见性 `{vis}` 的测试模块必须被识别（截断点在其之前）"
        );
    }
    // ② 访问器在前、测试模块在后 ⇒ 截断点必须是**后者**（访问器留在扫描面内）。
    let src = format!(
        "{keep}#[cfg(test)]\npub fn accessor() {{}}\n#[cfg(test)]\nmod tests {{\n}}\n"
    );
    let kept = truncate_before_test_module(&src, "probe.rs");
    assert!(
        kept.contains("pub fn accessor() {}"),
        "`#[cfg(test)] pub fn …` 访问器**必须留在扫描面内**（它**不是**截断点）：{kept:?}"
    );
    assert!(
        !kept.contains("mod tests"),
        "截断点必须是**其后紧跟 `mod tests`** 的那个 `#[cfg(test)]`（测试模块不得留在扫描面内）"
    );
    // ③ 形态不符 ⇒ 响亮失败（严禁"截断点悄悄前移 / 文件整段漏扫"）。
    let broken = format!("{keep}#[cfg(test)]\n// 注释夹在属性与 `mod tests` 之间\nmod tests {{}}\n");
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        truncate_before_test_module(&broken, "probe.rs")
    }));
    assert!(
        r.is_err(),
        "`#[cfg(test)]` 与 `mod tests` 之间夹注释 ⇒ 必须**响亮失败**（旧行为是静默漏扫）"
    );
}

/// **CRLF 行尾** + **多行字符串**回归（见 [`strip_comments_and_literals`] 的"已知边界"两节）。
///
/// 判据：① `` `\` `` 行续接在 **CRLF 源码**里必须被正常吃掉；② **非原始串里的裸换行是合法
/// Rust**（工作单元 L 实测订正）⇒ 必须被接受、内容剥空、**行数守恒**，且**不得**吞掉其后的源码。
///
/// **改什么会让本条变红**：把 [`line_continuation`] 的 `(Some(&'\r'), Some(&'\n'))` 分支
/// 删掉（退回"只认 `\n`"）⇒ 第 ① 段直接 panic（**已实测**：`shell_static_constraints`
/// 三网红于同一形态）；把多行串的长度按"到下一个引号为止"乱吞（不补换行）⇒ 第 ② 段的行数
/// 自证红。
///
/// ⚠️ **沿革（工作单元 L，如实登记）**：本条此前还有一条**反向探针**——"真·裸换行必须响亮
/// 失败"。该断言的前提（"合法 Rust 的非原始串不得含裸换行"）**经实测为假**：Rust 允许字符串
/// 字面量含裸换行（独立 crate 实测 `let s = "\<LF>abc<LF>def";` 合法，产出两行），
/// 首例误报是 `src/config.rs::help_text`（53 行帮助文本，一直在用）——只有当扫描面从
/// `ui/**` 扩到 `src/**` 时才被触发。故旧 ③b 判据连同该反向探针一并删除，
/// **替以第 ② 段的正面判据**（接受了什么、剥成什么、行数如何）。
#[test]
fn scanner_accepts_crlf_line_continuation_and_multi_line_strings() {
    // 源码本身仍是 LF；被测的 CRLF 由**字面转义**给出 ⇒ 不依赖检出时的换行风格。
    let crlf_src = "fn f() {\r\n    let s = \"a\\\r\nb\";\r\n    const SENTINEL: i32 = 48;\r\n}\r\n";
    let out = strip_comments_and_literals(crlf_src, "crlf-probe.rs");
    assert!(
        out.contains("SENTINEL"),
        "CRLF 下剥离不得吞掉后续源码（哨兵须可见）：{out:?}"
    );
    assert_eq!(
        out.matches('\n').count(),
        crlf_src.matches('\n').count(),
        "行数须守恒 —— `\\r` 必须随 `\\n` 一并被推进掉，否则账目对不上"
    );

    // ── ② 多行字符串（**裸换行**）：合法，必须被接受 + 剥空 + 行数守恒 + 不吞后续源码 ──
    let multi = "fn f() {\n    let s = \"line1\nLINE2_SENTINEL_INSIDE\nline3\";\n    const AFTER: i32 = 48;\n}\n";
    let out = strip_comments_and_literals(multi, "multi-line-string-probe.rs");
    assert!(
        !out.contains("LINE2_SENTINEL_INSIDE"),
        "多行串的**内容**必须被剥空（否则字面量会被下游静态网当成源码）：{out:?}"
    );
    assert!(
        out.contains("const AFTER"),
        "多行串之后的源码必须仍然可见（不许一路吞到文件尾）：{out:?}"
    );
    assert_eq!(
        out.matches('\n').count(),
        multi.matches('\n').count(),
        "多行串里的裸换行**必须逐个补回**（行号/行数账目守恒）：{out:?}"
    );

    // ── 反向探针（③a 仍在岗）：**EOF 未闭合**的字符串必须响亮失败 ──
    let unclosed_src = "fn f() {\n    let s = \"never closed\n";
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        strip_comments_and_literals(unclosed_src, "eof-unclosed-probe.rs")
    }));
    assert!(
        r.is_err(),
        "扫到 EOF 仍未闭合的字符串必须响亮失败（③a 未被上一条的放宽削弱）"
    );
}

/// 扫描面内**已知的原始串**清单：`(文件, 条数)`。由
/// `grep -cE '(^|[^A-Za-z0-9_])r#*"'` 在 [`NON_THIN_LAYER_SOURCES`] + [`THIN_LAYER_SOURCES`]
/// 上逐文件抄录（2026-09-18 实测：`src/control_route.rs` 13 / `src/channel.rs` 1 /
/// `src/console.rs` 1，合计 **15**，薄层 0；**2026-09-25 T21c-3 增量**：`control_route.rs`
/// 13 → **16** —— U-73 三端点的路由用例新增 3 枚字面量 JSON（`CATALOG` / `FIRE_PAGE` /
/// `BMS_PAGE`），实测 16，清单合计 **18**）。
///
/// **未列出的文件隐含期望 0** —— 所以往任一扫描面文件里新增一枚原始串都会让本条变红：
/// 那不是误报，是**清单需要显式登记**（这条判据的强度正来自"清单必须与源码同步"）。
const EXPECTED_RAW_STRING_COUNTS: [(&str, usize); 3] = [
    ("src/control_route.rs", 16),
    ("src/channel.rs", 1),
    ("src/console.rs", 1),
];

/// **"原始串前缀失认"失真**的自动哨（L 收尾补，见 [`strip_comments_and_literals`] 的订正段）。
///
/// ## 为什么需要它（判据删除后的实测空白）
///
/// 旧 ③b 判据（"非原始串不得含裸换行"）因**前提为假**被删除后，其注释曾声称"替代控制 =
/// 扫描面自证 + `literal_prefix` 前缀清单"。**该声称经实测不成立**：往 [`literal_prefix`]
/// 注入 `if cs[i] == 'r' { return None; }`（正是"原始串前缀失认"，本失真专防的形态）
/// ⇒ 全量 **368 passed / 0 failed** —— 三条替代控制**一条都没接住**。
///
/// ## 判据（两段，互为补充）
///
/// 1. **端到端**：把扫描面里**最险的真实形态**（原始串**内含 `"`**，正是失认后会把内容
///    泄漏成"代码"、并让闭引号错位的那类）喂给扫描器，断言内容**整枚剥空**、其后源码**仍然
///    可见**、行数**守恒**（含**多行**原始串 —— `src/channel.rs:849` 就是跨行的）；
/// 2. **对拍**：扫**全部扫描面文件**，用 [`strip_comments_and_literals_audited`] 取"被识别为
///    原始串"的条数，逐文件与 [`EXPECTED_RAW_STRING_COUNTS`] 相等（未列出的文件 = 0）。
///    失认 ⇒ 条数掉到 0 ⇒ **红**。
///
/// **改什么会让本条变红**（实测口径）：把 [`literal_prefix`] 的 `r` 分支改成 `return None`
/// （前身判据失认），或把原始串当普通串处理（`\` 转义 / 裸引号闭合）⇒ 第 1 段内容泄漏、
/// 第 2 段条数归零，**两段同时红**。
#[test]
fn scanner_recognizes_every_raw_string_on_the_scan_face() {
    // ── ① 端到端：原始串**内含引号**（失认后泄漏面最大）⇒ 必须整枚剥空、不吞后续、行数守恒 ──
    let inner_quote = "fn f() {\n    let j = r#\"{\"INNER_SENTINEL\":1}\"#;\n    const AFTER: i32 = 48;\n}\n";
    let out = strip_comments_and_literals(inner_quote, "raw-inner-quote-probe.rs");
    assert!(
        !out.contains("INNER_SENTINEL"),
        "原始串的**内容**（含其中的裸引号）必须被整枚剥空 —— 泄漏即说明前缀没被认出\
         （裸 `\"` 被当成开引号 ⇒ 内部引号错位闭合 ⇒ 内容被下游静态网当成源码）：{out:?}"
    );
    assert!(
        out.contains("const AFTER"),
        "原始串之后的源码必须仍然可见（不许吞到文件尾）：{out:?}"
    );
    assert_eq!(
        out.matches('\n').count(),
        inner_quote.matches('\n').count(),
        "行数须守恒：{out:?}"
    );

    // ── ② 端到端：**跨行**原始串（`src/channel.rs:849` 的真实形态）──
    let multi_raw =
        "fn f() {\n    let j = r#\"{\nMULTI_RAW_SENTINEL\n}\"#;\n    const AFTER: i32 = 48;\n}\n";
    let out = strip_comments_and_literals(multi_raw, "raw-multi-line-probe.rs");
    assert!(
        !out.contains("MULTI_RAW_SENTINEL"),
        "跨行原始串的内容同样必须被剥空：{out:?}"
    );
    assert!(
        out.contains("const AFTER"),
        "跨行原始串之后的源码必须仍然可见：{out:?}"
    );
    assert_eq!(
        out.matches('\n').count(),
        multi_raw.matches('\n').count(),
        "跨行原始串里的换行必须逐个补回（行号/行数账目守恒）：{out:?}"
    );

    // ── ③ 对拍：自证清单里的每个文件都真的在扫描面内（否则下面的断言是空转）──
    for (must, want) in EXPECTED_RAW_STRING_COUNTS {
        assert!(want > 0, "清单里的 `{must}` 期望条数为 0 ⇒ 该条不构成判据");
        assert!(
            NON_THIN_LAYER_SOURCES
                .iter()
                .chain(THIN_LAYER_SOURCES.iter())
                .any(|(n, _)| *n == must),
            "扫描面自证失败：清单登记的 `{must}` 不在 [`NON_THIN_LAYER_SOURCES`] / \
             [`THIN_LAYER_SOURCES`] 内 ⇒ 本条对它恒真（清单腐化）"
        );
    }

    // ── ④ 对拍：逐文件条数必须与清单相等（未列出的文件 = 0）──
    let mut total = 0usize;
    for (name, src) in NON_THIN_LAYER_SOURCES
        .iter()
        .chain(THIN_LAYER_SOURCES.iter())
    {
        let (_, raw_opens) = strip_comments_and_literals_audited(src, name);
        let want = EXPECTED_RAW_STRING_COUNTS
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| *c)
            .unwrap_or(0);
        let got = raw_opens.len();
        total += got;
        assert_eq!(
            got, want,
            "{name}：被识别为**原始串**的字面量有 {got} 枚，清单期望 {want} 枚。\n\
             盘点（实测）：{raw_opens:?}。\n\
             少 ⇒ **前缀失认**（[`literal_prefix`] 没认出 `r\"…\"` / `r#\"…\"#` / `br#\"…\"#`）——\
             被吞掉的源码对下游全部静态网不可见；\n\
             多 ⇒ 源码里新增了原始串，请把条数登记进 [`EXPECTED_RAW_STRING_COUNTS`]。"
        );
    }
    assert_eq!(
        total,
        EXPECTED_RAW_STRING_COUNTS.iter().map(|(_, c)| *c).sum::<usize>(),
        "扫描面原始串**总数**必须与清单一致（防『清单只对拍了几条、其余漂移没人管』）"
    );
}

/// 扫一段 `ui/**` 源码，取出**会取字形的字符**、出处字面量与**行号**（1 起）。
///
/// 口径（逐条）：剥注释；取 `"…"` 字符串字面量与 `'x'` / `'\u{…}'` 字符字面量
/// （**生命周期 `'a` 不是字面量**，按普通字符跳过）；`\u{XXXX}` 转义**解码成真字符**
/// （否则"用转义写的上屏字"会漏判）；忽略 [`NON_DISPLAY_SINKS`] 之后的字面量；
/// 剥 `{…}` 占位符；**掐掉测试模块（`#[cfg(test)]\nmod tests`）及其后**（见
/// [`truncate_before_test_module`] —— 生产文件里的 `#[cfg(test)] pub fn …` **测试访问器
/// 仍在扫描面内**，这是有意的）。空白字符不计。
/// **行号**让缺字报错能点名「文件:行 ← 文案」，而不是只报一个裸字形（B2a 收尾订正）。
fn ui_source_chars(src: &str, name: &str) -> Vec<(char, String, usize)> {
    let src = truncate_before_test_module(src, name);
    let bytes = src.as_bytes();
    let mut out: Vec<(char, String, usize)> = Vec::new();
    // 字节偏移 → 1 起的行号（报错点名用）。
    let line_at = |byte: usize| 1 + src[..byte].matches('\n').count();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            b'"' => {
                let open = i;
                i += 1;
                let mut lit = String::new();
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        // `\u{XXXX}` → 真字形；其余转义（`\n` / 引号…）不是上屏字符。
                        if bytes.get(i + 1) == Some(&b'u') && bytes.get(i + 2) == Some(&b'{') {
                            if let Some(close) = src[i + 3..].find('}') {
                                if let Ok(v) = u32::from_str_radix(&src[i + 3..i + 3 + close], 16) {
                                    if let Some(ch) = char::from_u32(v) {
                                        lit.push(ch);
                                    }
                                }
                                i = i + 3 + close + 1;
                                continue;
                            }
                        }
                        lit.push(' ');
                        i += 2;
                        continue;
                    }
                    if bytes[i] == b'"' {
                        i += 1;
                        break;
                    }
                    let ch = src[i..].chars().next().expect("源文件是合法 UTF-8");
                    lit.push(ch);
                    i += ch.len_utf8();
                }
                let prefix = src[..open].trim_end();
                if !NON_DISPLAY_SINKS.iter().any(|s| prefix.ends_with(s)) {
                    let shown = strip_format_placeholders(&lit);
                    for ch in shown.chars() {
                        if !ch.is_whitespace() {
                            out.push((ch, lit.clone(), line_at(open)));
                        }
                    }
                }
            }
            b'\'' => {
                let line = line_at(i);
                let rest = &src[i + 1..];
                if rest.starts_with("\\u{") {
                    if let Some(close) = rest.find('}') {
                        if let Ok(v) = u32::from_str_radix(&rest[3..close], 16) {
                            if let Some(ch) = char::from_u32(v) {
                                if !ch.is_whitespace() {
                                    out.push((ch, format!("'{ch}'"), line));
                                }
                            }
                        }
                        i += 1 + close + 1;
                        continue;
                    }
                }
                // 字符字面量 `'x'`；否则是生命周期（`'a` / `'static`）⇒ 按普通字符跳过。
                let mut it = rest.chars();
                if let (Some(ch), Some('\'')) = (it.next(), it.next()) {
                    if !ch.is_whitespace() && ch != '\\' {
                        out.push((ch, format!("'{ch}'"), line));
                        i += 1 + ch.len_utf8() + 1;
                        continue;
                    }
                }
                i += 1;
            }
            _ => {
                let ch = src[i..].chars().next().expect("源文件是合法 UTF-8");
                i += ch.len_utf8();
            }
        }
    }
    out
}

/// **入库的码表基线清单**（`fonts/` 下；每行一个 `U+XXXX`，`#` 开头为说明行）。
///
/// 它是**派生清单而非字体产物**：由 `fonts/gen_fonts.sh` 在生成 `.c` 的同时产出，
/// 体积极小，**允许且必须入库**（`.gitignore` 有显式反选）。作用是让码表检查在
/// **没有 `.c` 产物**（干净 clone / CI，见下）时**仍有权威基线**。
const CMAP_MANIFEST: &str = "lv_font_cmap.txt";

/// 解析入库清单（每行 `U+XXXX`；大小写均可）。格式不符 / 解不出码位 ⇒ **响亮失败**
/// （"解析不到"绝不允许伪装成"全部覆盖"）。
fn parse_cmap_manifest(src: &str, name: &str) -> std::collections::BTreeSet<char> {
    let mut set = std::collections::BTreeSet::new();
    for (i, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let hex = line.strip_prefix("U+").unwrap_or_else(|| {
            panic!("{name}:{}：`{line}` 不是 `U+XXXX` 形态 —— 清单格式已变，请同步解析器", i + 1)
        });
        let v = u32::from_str_radix(hex, 16)
            .unwrap_or_else(|_| panic!("{name}:{}：`{line}` 不是合法码位", i + 1));
        let ch = char::from_u32(v)
            .unwrap_or_else(|| panic!("{name}:{}：U+{v:04X} 不是合法字符", i + 1));
        set.insert(ch);
    }
    assert!(
        !set.is_empty(),
        "{name}：清单解析出 0 个码位 —— 文件损坏或与解析器脱节（**不得**当作已覆盖）"
    );
    set
}

/// 码点集合 → `U+XXXX` 逐字列表（诊断用）。
fn cps(set: &std::collections::BTreeSet<char>) -> Vec<String> {
    set.iter().map(|c| format!("U+{:04X}", *c as u32)).collect()
}

/// 取码表基线（**I3**：两张最强的网 —— [`ui_texts_covered_by_font_cmap`] 与
/// [`runtime_formatters_emit_only_cmap_glyphs`] —— 都依赖它）。**读取顺序（PM 裁定）**：
///
/// ① **`fonts/lv_font_noto_sc_*.c` 存在**（跑过 `gen_fonts.sh` 的机器）⇒ 用其**实际 cmap**
///    ＝各档 **交集**（见下），并**断言与入库清单 [`CMAP_MANIFEST`] 一致**（**漂移检测**：
///    不一致即失败，提示重跑 `gen_fonts.sh`）；
/// ② **无 `.c`**（干净 clone / CI 的常态 —— 产物不入库）⇒ 用**入库清单**，**不再跳过**；
/// ③ **两者皆缺**（= 仓库损坏：入库清单都没了）⇒ 打印说明后跳过。
///
/// ⚠️ 本版本**不再看 `cfg!(feature = "noto-font")`**（I3 的根因）：此前把它当"能否跳过"的
/// 判据，而 CI 是 `cargo test --workspace`（`.github/workflows/build-ubuntu.yml`，**未启用该
/// feature**）⇒ 两张网在 CI 上恒走"静默跳过"（"没抓到缺字"其实是"压根没查"）。
///
/// **交集 vs 并集（10 档）**：取**交集**。理由：a) 走查的语义是"该字符在**任何**字号档下
/// 上屏都得出字形"，只有交集能**保证**这一点；并集会让"只在部分档存在"的字符蒙混过关
/// （真机某档即豆腐块 = 漏）。b) 交集可能"误报"（某字只在小档用到、且该档有它），但
/// **误报可消解**（把字补进全部档 / 登记缺字），漏报在屏上是静默的豆腐块。
/// c) 实测 2026-09-11：10 档 cmap **完全相同**（当时各 324 码位，并集 − 交集 = 0）⇒ 当前
/// 两种取法**结果一致**，选交集只是把"未来某档掉字"这件事**钉在红灯上**。
/// **2026-09-25 T21a 后重测：10 档仍完全相同，各 461 码位**（扩到 464 字符的码表里，有 3 个
/// 码位**字库源本身就没有字形**、故不在任何档的 cmap 内：U+2082 / U+2715 / U+275A
/// —— 已用 `fontTools` 对 `NotoSansSC-Regular.otf` 实测确认 ⇒ 交集只有 461）—— 两种取法仍一致。
/// **2026-09-25 T21c-2-r1 后复测（口径订正，评审 N-4）**：入库 `font_subset_charset.txt`
/// 实测 = **464 字符、单行无尾换行**（`\n` / `\r` 计数皆 0；**不是**"463 唯一字符 + 尾换行"
/// —— 该说法对该产物不成立，唯一字符数同样是 **464**）。本批新增 `U+7A7A`（`空调` 用）、
/// 且 `U+2082` 已随 R-1 移除 ⇒ 缺口降为 **2**（`U+2715` / `U+275A`，存量、生产未用）
/// ⇒ 当前 10 档 cmap 交集 = **462**（= 464 − 2，与入库清单 `lv_font_cmap.txt` 的
/// "本清单码位数：462" 一致），两种取法**仍一致**。
fn load_font_cmap() -> Option<std::collections::BTreeSet<char>> {
    let fonts_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts");
    let manifest_path = fonts_dir.join(CMAP_MANIFEST);
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&fonts_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("lv_font_noto_sc_") && n.ends_with(".c"))
                })
                .collect()
        })
        .unwrap_or_default();
    if files.is_empty() {
        // ② / ③：无字体产物（干净 clone / CI 的常态）。
        if let Ok(src) = std::fs::read_to_string(&manifest_path) {
            return Some(parse_cmap_manifest(&src, CMAP_MANIFEST));
        }
        eprintln!(
            "跳过码表走查：{} 下**既无** `lv_font_noto_sc_*.c`（字体产物不入库，见设计 §1.1.2）\
             **也无**入库清单 `{CMAP_MANIFEST}` —— 这属**仓库损坏**（清单是入库派生项，见 \
             `fonts/gen_fonts.sh` 顶部注释）；请先从版本库恢复该清单（或跑 `fonts/gen_fonts.sh`：\
             需 node/python 与字库源）。**只在本情形**允许跳过。",
            fonts_dir.display()
        );
        return None;
    }
    files.sort();
    let mut cmap: Option<std::collections::BTreeSet<char>> = None;
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(f).expect("读字体生成物");
        let one = font_cmap_from_c(&src, &name);
        cmap = Some(match cmap {
            None => one,
            Some(prev) => prev.intersection(&one).copied().collect(),
        });
    }
    let cmap = cmap.expect("至少一个字体文件");
    // ① **漂移检测**：生成物的实际 cmap 必须与入库清单**集合相等**。
    let src = std::fs::read_to_string(&manifest_path).unwrap_or_else(|e| {
        panic!(
            "找到字体产物（`lv_font_noto_sc_*.c`）却读不到入库清单 `{}`（{e}）—— 仓库损坏：\
             清单是**入库项**，请 `fonts/gen_fonts.sh` 重新生成并连同清单一起入库。",
            manifest_path.display()
        )
    });
    let manifest = parse_cmap_manifest(&src, CMAP_MANIFEST);
    if cmap != manifest {
        let only_products: std::collections::BTreeSet<char> =
            cmap.difference(&manifest).copied().collect();
        let only_manifest: std::collections::BTreeSet<char> =
            manifest.difference(&cmap).copied().collect();
        panic!(
            "**码表漂移**：`fonts/lv_font_noto_sc_*.c` 的实际 cmap 与入库清单 `{CMAP_MANIFEST}` \
             不一致。\n  仅生成物有（清单该补，共 {} 个）：{:?}\n  仅清单有（生成物已无，共 {} 个）：{:?}\n\
             请重跑 `fonts/gen_fonts.sh` 让二者同步（脚本会顺带重写 `{CMAP_MANIFEST}`），\
             并把新的清单一并入库。",
            only_products.len(),
            cps(&only_products),
            only_manifest.len(),
            cps(&only_manifest),
        );
    }
    Some(cmap)
}

/// **入库的宽度基线清单**（`fonts/lv_font_metrics.txt`；与 [`CMAP_MANIFEST`] 同一次生成、
/// 同一份"派生项入库、产物不入库"约定，见 `fonts/gen_fonts.sh` 顶部注释）。
const METRICS_MANIFEST: &str = "lv_font_metrics.txt";

/// 从**生产字体产物** `fonts/lv_font_noto_sc_{px}.c` 取 `adv_w` 表（码位 → 1/16 px）。
/// `None` = 该档产物不存在（干净 clone / CI 常态）。
///
/// **口径（2026-09-25 订正）**：码位 → `glyph_id` 走 [`cmap_entries_from_c`]（**支持 N 个
/// cmap、四种 cmap 类型**，语义同 LVGL `get_glyph_dsc_id`）；`glyph_dsc[]` 的 `adv_w` 按
/// **glyph id 顺序**出现 ⇒ 取 `adv[glyph_id]`。
///
/// ⚠️ 旧实现是**单 cmap 特例**（`unicode_list_0[i]` ⇒ glyph id `i + 1`、码位 `v + 32`），
/// 扩字库后产物切成两个 cmap ⇒ 该特例失效（`glyph_id_start` 不再是 1）。**不得回退**：
/// 一律用**真实 `glyph_id`** 索引，并保留 `gid < adv.len()` 的越界保护。
///
/// 某个码位**找不到对应 `adv_w`** ⇒ **响亮 `panic`**（与 `fonts/gen_fonts.sh` 的 Python
/// `adv_of` 同款判据）：静默丢条目会让宽度网少测一片且无人察觉。
fn adv_w_from_c(px: u32) -> Option<std::collections::BTreeMap<u32, u64>> {
    let name = format!("lv_font_noto_sc_{px}.c");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fonts")
        .join(&name);
    let src = std::fs::read_to_string(path).ok()?;
    let adv: Vec<u64> = src
        .split(".adv_w = ")
        .skip(1)
        .map(|s| {
            s.split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap_or("0")
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect();
    let mut map = std::collections::BTreeMap::new();
    let mut missing: Vec<u32> = Vec::new();
    for (cp, gid) in cmap_entries_from_c(&src, &name) {
        if gid < adv.len() {
            map.insert(cp, adv[gid]);
        } else {
            missing.push(cp);
        }
    }
    assert!(
        missing.is_empty(),
        "{name}：cmap 里有 {} 个码位在 `glyph_dsc[]` 里没有对应 `adv_w`（最大码位 \
         U+{:04X}）—— 解析器与生成物脱节（与 `fonts/gen_fonts.sh` 的 Python `adv_of` 同款判断）",
        missing.len(),
        missing.iter().copied().max().unwrap_or(0)
    );
    assert!(
        !map.is_empty(),
        "{name}：`adv_w` 表解析出 0 个码位 —— 解析器与生成物脱节（不得静默返回空表）"
    );
    Some(map)
}

/// **基线文件头的「档位自证」行**（`# 本基线档位：24 26 …`）—— 档位清单的**唯一真源**。
///
/// 由 `fonts/gen_fonts.sh` 写出（B2c-1 收口 ⑥）：此前档位清单**第二份真源**硬编码在用例里
/// （`[24u32, 26, …]`），基线头只自证"档位数" ⇒ 新增一档时两处**都可能**不更新，而
/// "畸形值静默变 0 ⇒ `w <= 列宽` 恒真"的失效模式没有任何网。见 [`metrics_tiers`]。
const METRICS_TIERS_PREFIX: &str = "# 本基线档位：";

/// **基线文件头的「档位数自证」行**（`# 本基线档位数：10`）—— 与档位表**互相钉住**。
const METRICS_TIER_COUNT_PREFIX: &str = "# 本基线档位数：";

/// 从**入库宽度基线**的文件头解析**档位清单**（[`METRICS_TIERS_PREFIX`]）。
///
/// **响亮失败（B2c-1 收口 ⑥）**：自证行缺失 / 空表 / 非数字 / 重复档号，或与
/// [`METRICS_TIER_COUNT_PREFIX`] 自证的档位数不符 ⇒ 一律 `panic`。**不得**静默返回空表
/// （空表会让"档位清单"这张网失去意义且无人察觉）。
fn metrics_tiers(src: &str) -> Vec<u32> {
    let line = src
        .lines()
        .find(|l| l.trim_start().starts_with(METRICS_TIERS_PREFIX))
        .unwrap_or_else(|| {
            panic!(
                "{METRICS_MANIFEST} 缺少档位自证行 `{METRICS_TIERS_PREFIX}…`（它是档位清单的\
                 **唯一真源**：删掉它 ⇒ 本网**响亮失败**，而不是静默退化）"
            )
        });
    let tiers: Vec<u32> = line
        .trim_start()
        .trim_start_matches(METRICS_TIERS_PREFIX)
        .split_whitespace()
        .map(|t| {
            t.parse::<u32>().unwrap_or_else(|_| {
                panic!("{METRICS_MANIFEST} 的档位自证行里有**非数字**档号：`{t}`（行 = `{line}`）")
            })
        })
        .collect();
    assert!(
        !tiers.is_empty(),
        "{METRICS_MANIFEST} 的档位自证行为**空表**（`{line}`）—— 空表会让宽度网失去意义"
    );
    let uniq: std::collections::BTreeSet<u32> = tiers.iter().copied().collect();
    assert_eq!(
        uniq.len(),
        tiers.len(),
        "{METRICS_MANIFEST} 的档位自证行有**重复档号**：{tiers:?}"
    );
    let count_line = src
        .lines()
        .find(|l| l.trim_start().starts_with(METRICS_TIER_COUNT_PREFIX))
        .unwrap_or_else(|| {
            panic!(
                "{METRICS_MANIFEST} 缺少档位数自证行 `{METRICS_TIER_COUNT_PREFIX}…`\
                 （两行都是基线头的一部分，必须同步）"
            )
        });
    let n: usize = count_line
        .trim_start()
        .trim_start_matches(METRICS_TIER_COUNT_PREFIX)
        .trim()
        .parse()
        .unwrap_or_else(|_| {
            panic!("{METRICS_MANIFEST} 的档位数自证行不是数字：`{count_line}`")
        });
    assert_eq!(
        n,
        tiers.len(),
        "{METRICS_MANIFEST}：档位数自证 `{n}` ≠ 档位表长度 `{}`（两行均由 gen_fonts.sh 写出，\
         改一处忘另一处 ⇒ 本网响亮失败）",
        tiers.len()
    );
    tiers
}

/// 从**入库宽度基线** `fonts/lv_font_metrics.txt` 取某档的 `adv_w` 表（码位 → 1/16 px）。
/// `None` = **基线文件缺失**（仓库损坏；那时 [`adv_w_map`] 会带指引地 `panic`）。
///
/// 对齐口径：基线每行 `<px> <v1> <v2> … <vN>` 的 N 个值与 [`CMAP_MANIFEST`] 的码位
/// **按升序一一对应**（两者由 `fonts/gen_fonts.sh` **同一次运行**写出）。
///
/// **响亮失败（B2c-1 收口 ⑥ —— 原实现用 `?` / `unwrap_or(0)` 静默吞畸形行）**：
/// - `px` **不在**基线的档位自证表（[`metrics_tiers`]）内 ⇒ `panic`（而不是返回 `None`：
///   返回 `None` 会让 [`adv_w_map`] 误报"仓库损坏"，真实原因是"档位清单与字体阶梯脱节"）；
/// - 数据行首 token 非 `<px>`、或任一 adv_w 非数字 ⇒ `panic`（此前 `unwrap_or(0)` 会把
///   畸形值**静默变成 0** —— 而 `w <= 列宽` 对 0 恒真 ⇒ **宽度网静默失效**，最难查的一类）；
/// - 自证表里有该档却**没有数据行** ⇒ `panic`（基线与自证表脱节）。
fn adv_w_from_baseline(px: u32) -> Option<std::collections::BTreeMap<u32, u64>> {
    let fonts_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts");
    let manifest_src = std::fs::read_to_string(fonts_dir.join(CMAP_MANIFEST)).ok()?;
    let cps: Vec<u32> = manifest_src
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            u32::from_str_radix(
                l.strip_prefix("U+").unwrap_or_else(|| {
                    panic!("{CMAP_MANIFEST}：`{l}` 不是 `U+XXXX` 形态（与码表解析器脱节）")
                }),
                16,
            )
            .unwrap_or_else(|_| panic!("{CMAP_MANIFEST}：`{l}` 不是合法码位"))
        })
        .collect();
    let metrics_src = std::fs::read_to_string(fonts_dir.join(METRICS_MANIFEST)).ok()?;
    // ① 档位自证（唯一真源）+ 询问方必须在表内。
    let tiers = metrics_tiers(&metrics_src);
    assert!(
        tiers.contains(&px),
        "{METRICS_MANIFEST} 的档位自证表里**没有 `{px}` 档**，但本档被宽度断言问到 ⇒ \
         **基线档位与代码的字体阶梯脱节**（新增档位后忘了重跑 `fonts/gen_fonts.sh`）；\
         基线现有档位 = {tiers:?}"
    );
    // ② 定位数据行（畸形的首 token **响亮失败**，不再走 `?` 静默返回 `None`）。
    for line in metrics_src.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let Some(head) = it.next() else { continue };
        let got_px: u32 = head.parse().unwrap_or_else(|_| {
            panic!(
                "{METRICS_MANIFEST}：数据行的首 token `{head}` 不是合法 `<px>` 档号（畸形行 \
                 —— 原实现用 `?` 静默跳过，见 B2c-1 收口 ⑥；行 = `{line}`）"
            )
        });
        if got_px != px {
            continue;
        }
        let vals: Vec<u64> = it
            .map(|v| {
                v.parse::<u64>().unwrap_or_else(|_| {
                    panic!(
                        "{METRICS_MANIFEST} 的 `{px}` 档里有**非数字 adv_w**：`{v}` —— 原实现用 \
                         `unwrap_or(0)` 把它静默吞成 0，而 `w <= 列宽` 对 0 **恒真** ⇒ 宽度网\
                         **静默失效**（B2c-1 收口 ⑥）"
                    )
                })
            })
            .collect();
        assert_eq!(
            vals.len(),
            cps.len(),
            "{METRICS_MANIFEST} 的 `{px}` 档有 {} 个 adv_w，但 {CMAP_MANIFEST} 有 {} 个码位 \
             —— 两份入库清单**不是同一次生成**（请重跑 fonts/gen_fonts.sh 并一起提交）",
            vals.len(),
            cps.len()
        );
        return Some(cps.iter().copied().zip(vals).collect());
    }
    panic!(
        "{METRICS_MANIFEST} 的档位自证表里有 `{px}`，但**没有它的数据行** —— 自证表与数据\
         脱节（请重跑 fonts/gen_fonts.sh）"
    );
}

/// 某档字号的 `adv_w` 表（**读取顺序 = [`load_font_cmap`] 同款**）：
///
/// ① 有 `.c` 产物（跑过 `gen_fonts.sh` 的机器）⇒ 用它；**若入库基线也在，则逐值交叉校验**
///    （漂移即**响亮失败**，提示重跑 `fonts/gen_fonts.sh`）；
/// ② 无 `.c`（干净 clone / CI 常态）⇒ 用**入库基线** `fonts/lv_font_metrics.txt`（**不再跳过**）；
/// ③ 两者皆缺（仓库损坏 / 该档未入库）⇒ **响亮失败** —— 宽度类断言**不得**静默空转
///    （B2c-1 代码质量评审 ⑤：此前"读不到就 `return`" ⇒ 宽度网在 CI 上**整段跳过**且照绿）。
fn adv_w_map(px: u32) -> std::collections::BTreeMap<u32, u64> {
    let from_c = adv_w_from_c(px);
    let from_base = adv_w_from_baseline(px);
    if let (Some(c), Some(b)) = (&from_c, &from_base) {
        assert_eq!(
            c, b,
            "**宽度基线漂移**：`fonts/lv_font_noto_sc_{px}.c` 的 adv_w 与入库基线 \
             `{METRICS_MANIFEST}` 不一致 —— 请重跑 `fonts/gen_fonts.sh` 并提交新的基线"
        );
    }
    from_c.or(from_base).unwrap_or_else(|| {
        panic!(
            "既无字体产物 `fonts/lv_font_noto_sc_{px}.c`（产物不入库，干净 clone / CI 常态），\
             入库基线 `{METRICS_MANIFEST}` 里也没有 `{px}` 档 —— 宽度类断言**无法执行**。\
             请跑 `fonts/gen_fonts.sh` 重生成产物与基线（本检查**故意不静默跳过**：\
             「没量到」不等于「没问题」，见 B2c-1 代码质量评审 ⑤）"
        )
    })
}

/// 用**生产字体**（`.c` 的 `adv_w` 表，或 CI 上的**入库基线**）实测一段文本的**单行自然宽**
/// （px）。**缺基线即 `panic`**（见 [`adv_w_map`]）—— 调用方不再需要处理 `None`。
///
/// **为何必须读生成物 / 基线、而不是在用例里 `size()` 量标签**：`noto-font` feature 默认
/// **未启用**（`local-display/Cargo.toml`）⇒ 离屏用例里的标签走**降级字体**（无 CJK 字形）
/// ⇒ 量出来的宽度**不是真机宽度**（本仓库实测：同一串「审计服务连接超时，操作未执行：请检查
/// 审计服务后重试」在降级字体下 ≈250 px，在生产 24 px 档 ≈600 px）。判"长文本放得下"只能读
/// **真机的字形宽度表** —— 与 [`ui_texts_covered_by_font_cmap`] 的码表口径同源。
///
/// 口径：`adv_w` 单位为 **1/16 px**，逐字求和、**不计 kerning**（⇒ 结果是**上界**，偏保守）；
/// cmap 外 / 缺字按整字宽兜底（同样是上界）。
fn measured_text_px(text: &str, px: u32) -> i32 {
    let map = adv_w_map(px);
    let mut w16: u64 = 0;
    for ch in text.chars() {
        w16 += match map.get(&(ch as u32)) {
            Some(a) => *a,
            None => u64::from(px) * 16, // cmap 外 / 缺字 ⇒ 整字宽兜底（上界）
        };
    }
    (w16 / 16) as i32
}

/// 码表覆盖率（设计 §11.1「码表覆盖率」）—— **基线 = 生成字体的实际 cmap**
/// （`fonts/lv_font_noto_sc_*.c` 的 `unicode_list`，无产物时 = 入库清单 [`CMAP_MANIFEST`]），
/// **待查集合 = 扫 `ui/**` 源码字面量**。
///
/// **只有 `.c` 与入库清单都缺失**（仓库损坏）才跳过 —— 见 [`load_font_cmap`]。
#[test]
fn ui_texts_covered_by_font_cmap() {
    let Some(cmap) = load_font_cmap() else {
        return;
    };

    // 待查集合：扫源码字面量（非 ASCII + 需字形的符号一视同仁）。
    let mut missing: Vec<(char, String, &str, usize)> = Vec::new();
    // 扫到的**字面量原文**（供清册做**逐字**核对；M4：此前只断"每个字都出现过"，
    // 一条只含单个通用字的清册条目（如 `%`）能从**任何**含该字的字面量里"借光"通过 ——
    // 弱于"该串确实在源码里"）。
    let mut literals: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    // 扫描面 = `ui/**`（13 个文件）+ `src/state.rs`（设计 §11.1 明写的 "+ state.rs"；
    // 理由见 [`UI_ADJACENT_PROD_SOURCES`]）。
    for (name, src) in UI_PROD_SOURCES.iter().chain(UI_ADJACENT_PROD_SOURCES.iter()) {
        for (ch, lit, line) in ui_source_chars(src, name) {
            literals.insert(lit.clone());
            if !cmap.contains(&ch) {
                missing.push((ch, lit, *name, line));
            }
        }
    }
    // **扫描面自证**：`state.rs` 的三个屏上角标词必须真的进了待查集合 —— 否则
    // `include_str!` 指错文件 / 截断点失效（`state.rs` 无 `#[cfg(test)] mod tests` 时
    // 会把测试区一并扫入，反之若路径写错则**整份文件不漏**）时，本条会**构造性全绿**。
    for must in ["未取数", "源离线", "数据异常"] {
        assert!(
            literals.contains(must),
            "扫描面自证失败：`src/state.rs` 的屏上角标词 `{must}` 未进入待查集合 —— \
             UI_ADJACENT_PROD_SOURCES / 截断点失效（本条会构造性全绿）"
        );
    }

    // 清册（`ALL_TEXTS`）**不是基线**，但必须与源码不脱节：**每个条目都得整串出现在某个
    // `ui/**` 源码字面量里**（M4：只断"每个字都出现过"太弱 —— 例如 `%` 能从**任何**含 `%`
    // 的字面量里"借光"通过 —— 见 `literals` 的注释）。
    //
    // ⚠️ **诚实标注本检查的边界**：常量条目**自身的声明**就是一个字面量 ⇒ "声明了但没人用"
    // （`TEXT_DEVICE_TOTAL_POWER` 那种）仍能通过这一条。本检查管的是"清册 ↔ 源码字面量
    // **串**是否一致"（改了源码文案却忘了改清册 ⇒ 红），**不**管"常量是否真的被引用"
    //（那是 `dead_code` 与该常量读者 / 评审的活）。
    for t in ALL_TEXTS
        .iter()
        .chain(crate::ui::pages::ALL_TEXTS.iter())
        .chain(crate::ui::controls::ALL_TEXTS.iter())
        .chain(crate::ui::pages::p2_config::ALL_TEXTS.iter())
        .chain(crate::ui::pages::filters::ALL_TEXTS.iter())
        .chain(crate::ui::pages::p5_audit::ALL_TEXTS.iter())
        .chain(crate::ui::pages::p3_logs::ALL_TEXTS.iter())
        // P4 的清册（B3-2c 整改 建议 8）：此前**只有** `p4_interlock.rs` 自己的用例读它
        // （`all_texts_manifest_is_sane` / 运行时常量扫描），**没进过**这条"清册 ↔ 源码字面量"
        // 的逐字对账链 ⇒ 改了源码文案却忘改清册时，本网照绿。P1 / P6 的文案本就在
        // [`crate::ui::pages::ALL_TEXTS`]（上面那条）里，**不存在**"p1/p6 漏链"。
        .chain(crate::ui::pages::p4_interlock::ALL_TEXTS.iter())
    {
        assert!(
            literals.iter().any(|l| l.contains(t)),
            "清册条目 `{t}` 未**逐字**出现在任何 `ui/**` 源码字面量里 —— \
             清册与源码脱节（清册已不是覆盖率基线，见本条文档）"
        );
    }

    // 与登记缺口**集合相等**（多一个 = 真缺字；少一个 = 字库已补齐、登记该删）。
    let mut found: Vec<char> = missing.iter().map(|(c, _, _, _)| *c).collect();
    found.sort_unstable();
    found.dedup();
    let mut known: Vec<char> = KNOWN_MISSING.iter().map(|(c, _)| *c).collect();
    known.sort_unstable();
    // 缺字出处**逐条点名**：`文件:行 ← 文案`（只报裸字形无法定位，是 B2a 收尾订正的内容）。
    let detail = missing
        .iter()
        .map(|(c, lit, name, line)| format!("U+{:04X} `{c}` @ {name}:{line} ← 字面量 `{lit}`", *c as u32))
        .collect::<Vec<_>>()
        .join("\n          ");
    assert_eq!(
        found.iter().map(|c| *c as u32).collect::<Vec<_>>(),
        known.iter().map(|c| *c as u32).collect::<Vec<_>>(),
        "生成字体的 cmap 里缺了源码用到的字形；实际缺字 = {found:?}，已登记缺口 = {known:?}\n\
         未登记的缺字会出豆腐块，必须补进 §3.6 / charset 并重跑 gen_fonts.sh；\
         若字库已补齐，请同步删除 `KNOWN_MISSING` / D5 的对应条目。\n\
         逐条缺字出处（文件:行 ← 文案）：\n          {detail}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥‴ **运行时格式化器**的字符集检查（**C1 的根因封堵**）
//
// 上面两条（`ui_static_constraints` / `ui_texts_covered_by_font_cmap`）都只扫**源码字面量**
// ⇒ `format!` 的**运行时输出**永远查不到。C1 就是这么漏的：`format!("{v:.1}")` 在**源码里是
// 干净的**（模板 `"{v:.1}"` 只有 `{v:.1}` 占位符，剥掉后**空**），而负值实际产出 **ASCII `-`**
// （U+002D，生成字体 cmap **没有**该字形 —— `unicode_list_0` 由 `0xb` 直跳 `0xe`）
// ⇒ P 反向时负号是**豆腐块**、负值看着像正值（安全相关）。
//
// 本节让**每个数值 / 时间格式化器**吃一组**代表性输入**（含负值 / 极值 / 小数 / 时间），
// 断言其**产出的每一个字符**都落在生成字体 cmap 内。
// ═══════════════════════════════════════════════════════════════════════════

/// ⑥‴ 运行时格式化器的产出字符**必须全部落在生成字体 cmap 内**。
///
/// **覆盖的格式化器**（`ui/pages` 里全部"数值 / 时间 → 文本"出口）：
///
/// | 格式化器 | 语义 | 输入集 |
/// |----------|------|--------|
/// | [`pages::fmt_signed_1dp`] | 1 位小数（三相 P/I、总卡、ΣP 幅值） | 见 [`RUNTIME_FMT_NUM_INPUTS`] |
/// | [`pages::fmt_int0`] | 整数（SOC / CPU 温度 / 内存率） | 同上 |
/// | [`pages::fmt_sigma_kw`] | `ΣP ±x.x kW`（**含 `+` / `−` 前缀**） | 同上 |
/// | [`pages::fmt_decimals`] | catalog 驱动小数位（A2 压力 / 温度等 P4 数值） | 见 [`RUNTIME_FMT_NUM_INPUTS`] × `decimals ∈ {0..3, 越界}` + 非有限 |
/// | [`pages::format_epoch_ms_utc`] | 告警时间 `YYYY/MM/DD HH:MM:SS` | 见 [`RUNTIME_FMT_MS_INPUTS`] |
/// | [`pages::format_uptime`] | 运行时长 `N 日 HH:MM:SS` | 见 [`RUNTIME_FMT_SECS_INPUTS`] |
///
/// **输入集**必须含**负值**（`-1.0` / `-123.4` / `-0.0`）、**极值**（`f64::MIN` / `f64::MAX` /
/// `u64::MAX`）、**小数**（`0.05` / `12.5` / `999.9`）与**时间**（纪元原点 / UI §6.5 的示例时刻 /
/// 闰日 / 亚秒截断）—— **这正是能抓住 C1 的那类输入**：把 [`pages::fmt_signed_1dp`] 的负号
/// 改回 ASCII `-`，本用例立刻变红（自证见 B2a 代码质量评审收尾报告）。
#[test]
fn runtime_formatters_emit_only_cmap_glyphs() {
    use crate::ui::pages;

    /// 数值类输入（**必须含负值与极值**）。
    const RUNTIME_FMT_NUM_INPUTS: [f64; 12] = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        12.5,
        -123.4,
        0.05,
        -0.05,
        999.9,
        -999.9,
        f64::MIN,
        f64::MAX,
    ];
    /// 毫秒时间戳输入（纪元原点 / UI §6.5 示例 / 闰日 / 亚秒 / 上界）。
    const RUNTIME_FMT_MS_INPUTS: [u64; 5] = [
        0,
        1_789_047_727_999,
        1_709_210_096_000,
        946_684_800_000,
        u64::MAX,
    ];
    /// 秒数（运行时长）输入。
    const RUNTIME_FMT_SECS_INPUTS: [u64; 5] = [0, 59, 86_400, 2 * 86_400 + 23 * 3600 + 59 * 60 + 59, u64::MAX];

    let Some(cmap) = load_font_cmap() else {
        return;
    };

    let mut cases: Vec<(String, String)> = Vec::new();
    for v in RUNTIME_FMT_NUM_INPUTS {
        cases.push((format!("fmt_signed_1dp({v})"), pages::fmt_signed_1dp(v)));
        cases.push((format!("fmt_int0({v})"), pages::fmt_int0(v)));
        cases.push((format!("fmt_sigma_kw({v})"), pages::fmt_sigma_kw(v)));
    }
    // `fmt_decimals`（A2 压力 / 温度等 **P4 上屏数值** 的出口；T21c-1-r1 补入本网 —— 评审
    // **F4②**：`ui/pages/mod.rs` 的文档早就声称"由本用例构造性校验"，而 `cases` 里**没有它**）。
    // 输入集 = 数值集 × `decimals ∈ {0,1,2,3}`（登记表值域 **加**越界 4 / 255 ⇒ clamp 3）+ 非有限
    // （NaN / ±∞ ⇒ `--`）—— 与 `fmt_signed_1dp` 同款地覆盖**负号那一支**（`\u{2212}`）。
    for v in RUNTIME_FMT_NUM_INPUTS {
        for d in [0u8, 1, 2, 3, 4, 255] {
            cases.push((format!("fmt_decimals({v}, {d})"), pages::fmt_decimals(v, d)));
        }
    }
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        cases.push((format!("fmt_decimals({v}, 2)"), pages::fmt_decimals(v, 2)));
    }
    for ms in RUNTIME_FMT_MS_INPUTS {
        cases.push((format!("format_epoch_ms_utc({ms})"), pages::format_epoch_ms_utc(ms)));
    }
    for s in RUNTIME_FMT_SECS_INPUTS {
        cases.push((format!("format_uptime({s})"), pages::format_uptime(s)));
    }
    // 占位符是**上屏**的固定字形（`--` 会出豆腐块 —— 见 `PLACEHOLDER` 文档）。
    cases.push(("PLACEHOLDER".to_string(), pages::PLACEHOLDER.to_string()));
    // 外壳（B2c-3）的倒计时文案：静态模板已被 `ui_texts_covered_by_font_cmap` 覆盖，
    // 但**运行时的秒数数字**只有经本用例才真正走一遍码表（10 s 窗口内全部取值）。
    // 2026-09-15 代码质量评审建议补：此前 `countdown_text` 的**数字字形**无直接网。
    for s in 0..=crate::ui::shell::COUNTDOWN_WINDOW_SECS {
        cases.push((
            format!("shell::countdown_text({s})"),
            crate::ui::shell::countdown_text(s),
        ));
    }
    // 外壳（B3-2b-1）**整屏降级（EDGE-03）**的「已断开时长」文案：同上 —— 静态模板已由
    // `ui_texts_covered_by_font_cmap` 覆盖（` 秒` 的 `秒` 在 cmap 内），此处补**运行时数字**
    // 那一半（位数变化是唯一会引入新字形的路径）。输入含**位数跨界**与 `u64::MAX`。
    for s in [0u64, 1, 9, 10, 59, 60, 3599, 3600, 99_999, u64::MAX] {
        cases.push((
            format!("shell::disconnect_elapsed_text({s})"),
            crate::ui::shell::disconnect_elapsed_text(s),
        ));
    }

    // 先自证"输入集真的含负值"（否则本用例会退化成"只查了正数"而静默失效）。
    assert!(
        cases.iter().any(|(what, _)| what.contains("(-1)")),
        "输入集必须含负值（否则抓不到 C1 那类缺陷）"
    );

    for (what, text) in cases {
        for ch in text.chars() {
            if ch.is_whitespace() {
                continue;
            }
            assert!(
                cmap.contains(&ch),
                "**运行时**格式化器 `{what}` 产出的字符 U+{:04X} `{ch}` **不在生成字体的 cmap 内**\
                 （真机上是豆腐块）—— 产出文本 = `{text}`。\
                 数值格式化必须经 `ui/pages` 的唯一出口（负号恒 `\\u{{2212}}`，ASCII `-` 无字形）。",
                ch as u32
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥⁗ **契约字符串直上屏**的字符集检查（**C1 残留**，B2a 代码质量评审 ③）
//
// 前两条网都够不到**契约字符串**：它们的字面量在 `display-proto`（`LinkState::display_name()`
// 等）或**运行时的帧**里（`InfoSection.firmware_version` / `build_time` / `model` / `serial` /
// `mgmt_ipv4`）—— 既不在 `ui/**` 源码里、也不是 `format!("{v:.1}")` 那类数值出口。
// 后果与 C1 同类：版本号含 `-`（`1.0.0-rc1`）或型号含 `-`（`BECG-3568`）时，ASCII `-`
// （U+002D）在生成字体里**没有字形** ⇒ 真机上是豆腐块。**契约冻结（`display-proto` 不得改）**
// ⇒ 收口在**显示侧**：[`pages::display_safe`]（登记为偏差 **D9**）。
// ═══════════════════════════════════════════════════════════════════════════

/// ⑥⁗ **契约直上屏字符串**经 [`pages::display_safe`] 后，每个字符必须在 cmap 内。
///
/// 覆盖三类：
///
/// 1. **帧内自由串**（`InfoSection` 的 `firmware_version` / `build_time` / `model` / `serial` /
///    `mgmt_ipv4`）—— **代表性取值必须含带 `-` 的版本号**（`1.0.0-rc1`）与含 `/` 的时间戳
///    （UI 的告警时间口径），以及 ISO 8601 的 `T`/`Z` 变体（最坏情形）；
/// 2. **PG 面板的服务地址**（`127.0.0.1:9810 / 127.0.0.1:9811`）；
/// 3. **契约枚举的 `display_name()`**（[`LinkState`] / [`RunState`] / [`ServiceScope`]）——
///    这些**不经改写**直上屏（无 cmap 外字符，故原样透传），逐字核对。
///
/// 另断言 [`pages::ASCII_DISPLAY_ALPHABET`]（`display_safe` 的"安全表"）**每个字符都在基线
/// cmap 内** —— 这样它就不是"第二份真源"（码表变 ⇒ 本条红）。
#[test]
fn contract_strings_emit_only_cmap_glyphs() {
    use crate::ui::pages;
    use mupc_display_proto::{LinkState, RunState};

    let Some(cmap) = load_font_cmap() else {
        return;
    };

    // 安全字母表 ⊆ 基线 cmap（`display_safe` 的替换目标必须真有字形）。
    for ch in pages::ASCII_DISPLAY_ALPHABET.chars() {
        assert!(
            cmap.contains(&ch),
            "`ASCII_DISPLAY_ALPHABET` 含 cmap 外字符 U+{:04X} `{ch}` —— 该表已与基线脱节",
            ch as u32
        );
    }

    // 代表性契约取值（**必须含带 `-` 的版本号**）。
    const CONTRACT_VALUES: [(&str, &str); 8] = [
        ("firmware_version", "0.1.0"),
        ("firmware_version", "1.0.0-rc1"),
        ("build_time", "2026/09/10 13:42:07"),
        ("build_time", "2026-09-10T13:42:07Z"),
        ("model", "BECG-3568"),
        ("serial", "SN-2026-0001"),
        ("mgmt_ipv4", "192.168.3.118"),
        ("service_addr", "127.0.0.1:9810 / 127.0.0.1:9811"),
    ];
    // 自证：取值里**确实**有 cmap 外字符（否则本条会退化成"只查了本来就干净的串"）。
    assert!(
        CONTRACT_VALUES
            .iter()
            .any(|(_, raw)| raw.chars().any(|c| !cmap.contains(&c))),
        "代表性取值必须含 cmap 外字符（否则本用例结构上抓不到 ③ 那类缺陷）"
    );
    for (what, raw) in CONTRACT_VALUES {
        let shown = pages::display_safe(raw);
        for ch in shown.chars() {
            if ch.is_whitespace() {
                continue;
            }
            assert!(
                cmap.contains(&ch),
                "契约字符串 `{what}` **上屏后**仍含 cmap 外字符 U+{:04X} `{ch}`\
                 （原文 `{raw}` → 上屏 `{shown}`）—— 真机上是豆腐块。\
                 契约串必须经 `ui/pages::display_safe` 再上屏（见 D9）。",
                ch as u32
            );
        }
    }
    // 改写**真的发生**（回归锚点：去掉 `display_safe` 的 ASCII 改写即红）。
    assert_eq!(
        pages::display_safe("1.0.0-rc1"),
        "1.0.0\u{2013}RC1",
        "带 `-` 的版本号必须改写（`-` → U+2013；小写 → 大写同族）"
    );
    assert_ne!(
        pages::display_safe("BECG-3568"),
        "BECG-3568",
        "型号里的 `-` 必须改写（cmap 无 U+002D 字形）"
    );

    // 契约枚举 `display_name()`：**不经改写**直上屏 ⇒ 逐字必须在 cmap 内。
    let links = [
        LinkState::Connected,
        LinkState::Connecting,
        LinkState::Disconnected,
        LinkState::NotConfigured,
        LinkState::Unknown,
    ];
    for st in links {
        let t = st.display_name();
        for ch in t.chars() {
            assert!(
                cmap.contains(&ch),
                "`LinkState::{st:?}.display_name()` = `{t}` 含 cmap 外字符 U+{:04X} `{ch}`",
                ch as u32
            );
        }
    }
    let runs = [
        RunState::Stop,
        RunState::Standby,
        RunState::Charge,
        RunState::Discharge,
    ];
    for rs in runs {
        let t = rs.display_name();
        for ch in t.chars() {
            assert!(
                cmap.contains(&ch),
                "`RunState::{rs:?}.display_name()` = `{t}` 含 cmap 外字符 U+{:04X} `{ch}`",
                ch as u32
            );
        }
    }
    // ⚠️ **有意不在此列**的契约 `display_name()`（页面**不用它上屏**、另取 cmap 内的
    //    页面常量）—— 逐条登记理由，避免"漏查"与"该查的没查"混淆：
    //    · `SocSource::PcsReg1010` = `PCS(REG1010)`：`(`/`)` 无字形 ⇒ 页面取 `TEXT_SOC_SRC_PCS`；
    //    · `ControlSource::AiDisabled`：含全角 `，` 与 `为`（均无字形）⇒ 页面取 `TEXT_AI_DISABLED`；
    //    · `ServiceScope::LoopbackOnly` = `仅回环 127.0.0.1`：**`环`(U+73AF) 不在 cmap 内**
    //      （`font_subset_charset.txt` 未收，实测生成字体亦无）⇒ 页面取 `TEXT_SERVICE_ADDR`
    //      = `本机监听地址 · 仅本机`，`service_scope_text()` 仅供评审核对口径、**不直上屏**。
    //    （本用例第一版把 `ServiceScope` 也列了进来 ⇒ 立刻红并点名 `U+73AF 环` —— 这说明
    //     本网**确实**敏感；此处改为如实登记"为何不列"。）
}

/// 组件固定文案的**形状**断言（与文档的逐字偏差在 `components.rs` 里已注明）。
#[test]
fn component_texts_shape() {
    assert_eq!(components::TEXT_CANCEL, "取消");
    assert_eq!(components::TEXT_ARROW, "→");
    assert_eq!(TEXT_CHECK_PREFIX, "✓", "UI §5.2 选中前缀");
    // ⚠️ 与 UI §2.5 的偏差：圆括号不在 §3.6 声明字符集内（见 B1 报告）
    assert_eq!(TEXT_WARN_BANNER, "生效瞬间通信将短暂中断 ≤ 5 s");
    assert!(TEXT_WARN_FIELDS_PREFIX.starts_with("涉及"), "「涉及:」前缀");
    assert_eq!(TEXT_WARN_FIELD_SEP, "/", "字段分隔符");
    assert!(ALL_TEXTS.contains(&TEXT_WARN_BANNER), "固定文案登记进 ALL_TEXTS");
}

// ═══════════════════════════════════════════════════════════════════════════
// ②③④ LVGL 链路（由 `src/lvgl/tests.rs` 的唯一 `#[test]` 串行调起）
// ═══════════════════════════════════════════════════════════════════════════

/// UI 层全部场景（**必须**由 `src/lvgl/tests.rs` 在同一线程内顺序调起）。
///
/// 本函数自带 `init` / `deinit` 配对（与 `tests_a2::obj_style_font_chain` /
/// `tests_a3::widgets_chain` 同构）。
///
/// **不做渲染**：本链路不调用任何强制重绘入口（UI 层的静态约束 ⑥），故只做
/// **尺寸 / 文本 / 状态 / 颜色的读回**断言。
///
/// B1 的全部 LVGL 读回覆盖（验收项 ②③④）—— 本工作单元的主要交付物。
///
/// **不另起 `#[test]`**：由 `src/lvgl/tests.rs::lvgl_core_bridge_chain`（LVGL 侧唯一的
/// `#[test]`）在同一线程内顺序调起（LVGL 非线程安全）。自带 `init`/`deinit` 配对。
pub(crate) fn ui_chain() {
    lvgl::init().expect("lvgl::init (ui)");
    let mut disp = Display::create(Dimens::SCREEN_W as u32, Dimens::SCREEN_H as u32)
        .expect("Display::create (ui)");
    // 本链路不渲染，flush 回调只作占位（LVGL 要求 display 有 flush 通路）。
    disp.set_flush_cb(|_area: Area, _px: &[u8]| {});
    let screen = Obj::screen().expect("Obj::screen (ui)");

    // ── ② 主题样式能被施加（不直连绑定、不 panic）────────────────────────
    {
        let card = Obj::create(&screen).expect("Obj::create (card)");
        card.set_size(100, 50);
        card.add_style(&theme::card(), StyleSelector::main());
        // `coords` 由布局趟写入 ⇒ 读尺寸前必须强制渲染一趟（见 `refr_now_for_test`）。
        disp.refr_now_for_test();
        assert_eq!(card.size(), (100, 50), "卡片样式不得改写调用方设定的尺寸");
        assert!(card.is_alive(), "施加卡片样式后对象仍存活");

        card.add_style(&theme::card_alert_left(Palette::DANGER), StyleSelector::main());
        card.add_style(&theme::control_pressed(), StyleSelector::state_of(crate::lvgl::style::State::PRESSED));
        card.add_style(&theme::focus_ring(), StyleSelector::state_of(crate::lvgl::style::State::FOCUSED));
        theme::button(theme::ButtonKind::Text).apply(&card);

        // 滚动条三色（§3.5 / §5.6-A）：能构造 + 能挂载即视为通道打通
        let sc = crate::lvgl::widgets::ScrollContainer::create(&screen).expect("ScrollContainer");
        sc.set_size(80, 40);
        sc.add_style(&theme::scrollbar_idle(), StyleSelector::part_of(crate::lvgl::style::Part::SCROLLBAR));
        sc.add_style(&theme::scrollbar_active(), StyleSelector::new(crate::lvgl::style::Part::SCROLLBAR, crate::lvgl::style::State::SCROLLED));
        let _track = theme::scrollbar_track();
        assert!(sc.is_alive(), "滚动容器 + 滚动条样式可用");
        drop(sc);
    }

    // ── ③ StatusChip：三通道（图标 / 文字 / 颜色）都能读回 ────────────────
    {
        let chip = StatusChip::new(&screen, 240, "✓", "成功", ChipSkin::SUCCESS).expect("StatusChip");
        assert_eq!(chip.text().as_deref(), Some("成功"), "文字通道");
        assert_eq!(chip.icon_text().as_deref(), Some("✓"), "图标通道");
        assert_eq!(chip.accent(), Palette::OK, "颜色通道（skinc accent）");
        assert_eq!(chip.skin(), ChipSkin::SUCCESS);
        disp.refr_now_for_test();
        assert_eq!(chip.size().1, Dimens::STATUS_CHIP_H, "状态胶囊高 32");
        // 位置口径（防"容器 padding 与子对象外缘坐标叠加"这一类缺陷复现）：
        // 胶囊样式的 pad 为 0 ⇒ 子图标只应被**描边**内缩，且必须**完全落在胶囊内**
        // （不得因 padding 叠加而位移/底溢）。
        let chip_area = chip.obj().coords();
        let icon_area = chip.icon_obj().coords();
        assert_eq!(
            icon_area.x1 - chip_area.x1,
            Stroke::THIN,
            "子图标只应被描边内缩（pad 必须为 0）"
        );
        assert!(
            icon_area.y2 <= chip_area.y2 && icon_area.y1 >= chip_area.y1,
            "子图标必须完全落在胶囊内（不得底溢）"
        );
        chip.set_text("失败");
        assert_eq!(chip.text().as_deref(), Some("失败"), "改文字应生效");
        assert_eq!(chip.skin(), ChipSkin::SUCCESS, "改文字不得改颜色通道");
        drop(chip);
    }

    // ── ③ LedIndicator：灯 + 文字 + 图形三件并列 ─────────────────────────
    {
        let led = LedIndicator::new(&screen, 300, "●", "已连接", Palette::LINK_OK)
            .expect("LedIndicator");
        assert_eq!(led.icon_text().as_deref(), Some("●"), "图标通道");
        assert_eq!(led.text().as_deref(), Some("已连接"), "文字通道");
        assert_eq!(led.color(), Palette::LINK_OK, "颜色通道");
        assert!(led.is_lit(), "默认点亮");
        let bright_on = led.brightness();
        led.set_lit(false);
        assert!(!led.is_lit());
        let bright_off = led.brightness();
        assert!(
            bright_on > bright_off,
            "点亮亮度应高于熄灭亮度（{bright_on} vs {bright_off}）"
        );
        drop(led);
    }

    // ── ② Stepper：`lv_button` + `lv_label` 组合、越界禁用、clamp ─────────
    {
        let st = Stepper::new(&screen, 1, 65535, 2404, 1).expect("Stepper");
        assert_eq!(st.value(), 2404);
        assert_eq!(st.display().as_deref(), Some("2404"), "值区为纯 lv_label");
        assert_eq!(st.min(), 1);
        assert_eq!(st.max(), 65535);
        disp.refr_now_for_test();
        assert_eq!(
            st.size(),
            (Dimens::STEPPER_BTN_W * 2 + Dimens::STEPPER_VALUE_W, Dimens::STEPPER_H),
            "步进器口径 248×64（UI §5.1 #6）"
        );
        assert!(!st.minus_disabled() && !st.plus_disabled(), "中值处两端可用");

        st.set_value(0);
        assert_eq!(st.value(), 1, "类型层 clamp 到 min（TT-03 越界不可达）");
        assert_eq!(st.display().as_deref(), Some("1"));
        assert!(st.minus_disabled(), "value == min ⇒ − 禁用");
        assert!(!st.plus_disabled());

        st.set_value(999_999);
        assert_eq!(st.value(), 65535, "类型层 clamp 到 max");
        assert!(st.plus_disabled(), "value == max ⇒ ＋ 禁用");
        assert!(!st.minus_disabled());

        st.set_disabled(true);
        assert!(st.minus_disabled() && st.plus_disabled(), "显式禁用两端都禁用");
        st.set_value(1000);
        assert_eq!(st.value(), 1000, "禁用态下 set_value 仍生效（程序化设值不受限）");
        assert!(st.minus_disabled() && st.plus_disabled(), "显式禁用优先于越界判定");
        st.set_disabled(false);
        assert!(!st.minus_disabled() && !st.plus_disabled(), "恢复后按值重算");

        // IPv4 段宽（UI §5.1 #7 的公式口径：4×(64+64+64) + 3×8）
        let seg = Stepper::with_value_width(&screen, 0, 255, 192, 1, Dimens::IPV4_VALUE_W)
            .expect("Stepper (ipv4 seg)");
        disp.refr_now_for_test();
        assert_eq!(
            seg.size().0,
            Dimens::STEPPER_BTN_W * 2 + Dimens::IPV4_VALUE_W,
            "IPv4 单段宽 = 192（⚠️ UI §5.1 #7 正文写 856，与其自身公式不符 —— 见报告）"
        );
        drop(seg);
        drop(st);
    }

    // ── ② MultiSelectChips：勾选态 / 前缀 / 选中集合 ──────────────────────
    {
        let ms = MultiSelectChips::new(&screen, &["全部", "配置保存"], 2, Dimens::CHIP_MIN_W)
            .expect("MultiSelectChips");
        assert_eq!(ms.len(), 2);
        assert_eq!(ms.columns(), 2);
        assert_eq!(ms.option(1), Some("配置保存"));
        assert!(ms.selected().is_empty(), "默认全不选（P3 语义由页面决定）");
        assert_eq!(ms.chip_display(1).as_deref(), Some("配置保存"), "未选中无前缀");

        ms.set_selected(1, true);
        assert_eq!(ms.selected(), vec![1], "勾选态读回");
        assert_eq!(
            ms.chip_display(1).as_deref(),
            Some("✓配置保存"),
            "选中加前缀 ✓（UI §5.2）"
        );
        ms.clear();
        assert!(ms.selected().is_empty(), "清空全选");
        // 越界索引 no-op（不得 panic）
        ms.set_selected(99, true);
        assert!(ms.selected().is_empty());
        drop(ms);
    }

    // ── ②③④ ConfirmDialog：L1 / L2 / L2+ 三档形态 ────────────────────────
    {
        let details = [
            ConfirmDetail { field: "端口", before: "2404", after: "2405" },
            ConfirmDetail { field: "心跳间隔", before: "10", after: "15" },
        ];

        // L1：无进度条、无 WarnBanner、默认焦点「取消」、单击生效文案
        let spec1 = ConfirmSpec {
            title: "确认保存运行参数",
            impact: "修改将立即生效，无需重启装置",
            details: &details,
            warn_fields: &[],
        };
        let d1 = ConfirmDialog::new(&screen, &spec1, ConfirmLevel::L1).expect("ConfirmDialog L1");
        // **遮罩可点**（拦穿透）—— 与 EDGE-03 的整屏遮罩（**必须**可穿透）语义相反：
        // 模态弹层要求"点外面不落到页面上"，通道断要求"写路径仍可用"（§8.3 EDGE-20）。
        // 两者各有一条断言（另一条 = `shell_chain` 的 `overlay_obj()` 无 `CLICKABLE`）。
        assert!(
            d1.mask().has_flag(crate::lvgl::obj::ObjFlag::CLICKABLE),
            "ConfirmDialog 的遮罩**故意**可点（拦穿透）—— 若变红，先确认不是把 EDGE-03 的\
             可穿透语义误套到模态遮罩上"
        );
        assert_eq!(d1.level(), ConfirmLevel::L1);
        assert!(!d1.has_progress(), "L1 无长按进度条");
        assert!(!d1.has_warn_banner(), "L1 无 WarnBanner");
        assert!(d1.default_focus_is_cancel(), "TT-09 默认焦点「取消」");
        assert_eq!(d1.confirm_button().text().as_deref(), Some("确认执行"));
        disp.refr_now_for_test();
        assert_eq!(
            d1.confirm_button().size(),
            (Dimens::BTN_MAIN_W, Dimens::BTN_H_PRIMARY),
            "确认按钮 200×64"
        );
        // ① 静态子树必须真的在树上：标题 / 色条 / 分隔线都是 `panel` 的子对象，若其拥有型
        // 句柄在 `new` 返回时被 Drop，LVGL 会级联删除它们（**此前无断言覆盖 ⇒ 缺陷潜伏**）。
        assert!(d1.title().is_alive(), "标题不得被构造器 Drop 级联删除");
        assert_eq!(
            d1.title().text().as_deref(),
            Some("确认保存运行参数"),
            "标题文本可读回（句柄失效则 text() = None）"
        );
        assert!(d1.level_bar().is_alive(), "顶部级别色条仍在树上");
        assert_eq!(
            d1.level_bar().size(),
            (Dimens::DIALOG_W, Dimens::DIALOG_BAR_H),
            "级别色条 = 弹层宽 × 4（布局趟后落定）"
        );
        assert!(d1.divider().is_alive(), "分隔线仍在树上");
        assert_eq!(d1.cancel_button().text().as_deref(), Some(components::TEXT_CANCEL));
        assert!(!d1.confirm_disabled(), "未点击前不禁用");
        assert!(d1.panel().is_alive() && d1.mask().is_alive(), "遮罩 + 弹层都在");

        // ── Important 2：焦点环确实挂到 `FOCUSED` 态 ─────────────────────────
        // ⚠️ 薄层**没有样式读回 API**（`Obj` 不暴露"某选择器下挂了哪些样式"，`Style` 也
        // 没有属性 getter）⇒ 无法直接问 LVGL"焦点环挂上了吗"。故这里断**样式集的选择器
        // 真源**（`ButtonStyles::entries`，`apply` 与它共用同一份列表；此前 `apply` 压根
        // 没挂 `FOCUSED`，本断言即会红），配合下方已断的**运行时 FOCUSED 状态位**
        // （`default_focus_is_cancel`）⇒ "『取消』在 FOCUSED 态下确有焦点环样式"。
        // （薄层缺读回能力一事已上报 PM。）
        let bs = theme::button(theme::ButtonKind::Secondary);
        let entries = bs.entries();
        assert_eq!(entries.len(), 5, "按钮样式集 5 态（含 FOCUSED）");
        assert_eq!(
            entries[4].1,
            StyleSelector::state_of(crate::lvgl::style::State::FOCUSED),
            "TT-09：焦点环必须挂 FOCUSED 态（此前只有 DEFAULT/PRESSED/DISABLED/CHECKED）"
        );
        assert!(
            !Rc::ptr_eq(entries[4].0, entries[0].0),
            "焦点环是独立 Style（不是复用 normal）"
        );
        // 「取消」/「确认」按钮都用这套样式装配 ⇒ 焦点环的施加对象正确。
        assert!(d1.default_focus_is_cancel(), "TT-09：默认焦点落在「取消」上");

        // ── Important 3：L1 单击 → 防重 → 通知（此前**从未被驱动**）──────────
        let hits = Rc::new(Cell::new(0u32));
        {
            let h = hits.clone();
            d1.set_on_confirm(move || h.set(h.get() + 1));
        }
        let t_first = Instant::now();
        d1.confirm_button().send_event(EventCode::CLICKED);
        assert_eq!(hits.get(), 1, "L1 单击应触发一次通知");
        assert!(d1.confirm_disabled(), "提交后按钮禁用（TT-10 视觉反馈）");
        d1.confirm_button().send_event(EventCode::CLICKED);
        // 防重窗口用**真实时钟**（`ConfirmDialog` 内部 `Instant::now()`，无法注入）。
        // 若两次派发之间真的耗满了一个窗口（极慢/被强占的机器），则"第二次应被拒"这一
        // 时序前提不再成立 —— 此时**显式跳过并打印**，而不是让 CI 偶发变红（假失败）。
        // 窗口本身的正确性由上面 `debounce_window_is_500ms` 的**注入时钟**用例覆盖。
        if t_first.elapsed() < Timing::debounce() {
            assert_eq!(hits.get(), 1, "500 ms 内第二次点击被防重拒绝（TT-10）");
        } else {
            eprintln!(
                "跳过防重时序断言：本次两次派发耗时 {:?} ≥ 窗口 {:?}（环境过慢），\
                 该时序前提不成立；窗口逻辑另由注入时钟用例覆盖。",
                t_first.elapsed(),
                Timing::debounce()
            );
        }
        // 重置窗口后再点一次应当**放行** —— 证明上面那次拒绝确由 500 ms 窗口所致，
        // 而不是"回调根本没通"造成的假象（防重的正/反两面都覆盖）。
        d1.debounce().reset();
        d1.confirm_button().send_event(EventCode::CLICKED);
        assert_eq!(hits.get(), 2, "窗口重置后再点放行（证明拒绝来自防重窗口）");
        drop(d1);

        // L2：有进度条、无 WarnBanner、危险文案「按住确认」
        let spec2 = ConfirmSpec {
            title: "确认释放联锁",
            impact: "将清除联锁自锁状态，装置可恢复运行",
            details: &details,
            warn_fields: &[],
        };
        let d2 = ConfirmDialog::new(&screen, &spec2, ConfirmLevel::L2).expect("ConfirmDialog L2");
        assert_eq!(d2.level(), ConfirmLevel::L2);
        assert!(d2.has_progress(), "L2 有长按进度条");
        assert!(!d2.has_warn_banner(), "L2 不强制 WarnBanner");
        assert!(d2.default_focus_is_cancel());
        assert_eq!(d2.confirm_button().text().as_deref(), Some("按住确认"));
        assert_eq!(d2.progress_value(), Some(0), "进度初值 0（未按下）");
        d2.set_progress(140);
        assert_eq!(d2.progress_value(), Some(100), "进度 clamp 到 100");
        d2.set_progress(-5);
        assert_eq!(d2.progress_value(), Some(0), "进度 clamp 到 0");
        assert_eq!(d2.debounce().window(), Duration::from_millis(500), "TT-10 窗口");

        // ── Important 1/3：L2 长按状态机（PRESSED / RELEASED / PRESS_LOST /
        // LONG_PRESSED 四条分支此前**从未被驱动**）────────────────────────────
        let hits2 = Rc::new(Cell::new(0u32));
        {
            let h = hits2.clone();
            d2.set_on_confirm(move || h.set(h.get() + 1));
        }
        d2.confirm_button().send_event(EventCode::PRESSED);
        assert_eq!(d2.progress_value(), Some(0), "按下 ⇒ 进度归零并显形");
        disp.refr_now_for_test();
        // Important 1：长按进度条必须**填满确认按钮**（用 `coords()` 而非 `size()`）。
        let pbc = d2.progress_bar().expect("L2 有进度条").coords();
        let cbc = d2.confirm_button().coords();
        assert_eq!(
            (pbc.x1, pbc.y1, pbc.x2, pbc.y2),
            (cbc.x1, cbc.y1, cbc.x2, cbc.y2),
            "长按进度条须填满确认按钮（此前内缩 18 px、右/下各被裁 2 px）"
        );
        d2.confirm_button().send_event(EventCode::RELEASED);
        assert_eq!(hits2.get(), 0, "未满 1.0 s 松手 ⇒ 不提交（RELEASED 取消）");
        d2.confirm_button().send_event(EventCode::PRESSED);
        d2.confirm_button().send_event(EventCode::PRESS_LOST);
        assert_eq!(hits2.get(), 0, "按下后滑出控件 ⇒ 不提交（PRESS_LOST 取消）");
        d2.confirm_button().send_event(EventCode::LONG_PRESSED);
        assert_eq!(hits2.get(), 1, "满 1.0 s ⇒ 提交一次（LONG_PRESSED）");
        assert_eq!(d2.progress_value(), Some(100), "长按满 ⇒ 进度满格");
        assert!(d2.confirm_disabled(), "提交后按钮禁用（TT-10）");
        d2.confirm_button().send_event(EventCode::LONG_PRESSED);
        assert_eq!(hits2.get(), 1, "防重窗口内第二次长按被拒绝");
        drop(d2);

        // L2+：**强制** WarnBanner（含「涉及:」第二行）
        let spec3 = ConfirmSpec {
            title: "确认保存运行参数",
            impact: "修改将立即生效，无需重启装置",
            details: &details,
            warn_fields: &["端口"],
        };
        let d3 = ConfirmDialog::new(&screen, &spec3, ConfirmLevel::L2Plus)
            .expect("ConfirmDialog L2+");
        assert!(d3.has_warn_banner(), "L2+ **必须**出现 WarnBanner");
        assert!(d3.has_progress(), "L2+ 仍是长按确认");
        assert_eq!(d3.confirm_button().text().as_deref(), Some("按住确认"));
        let wb = d3.warn_banner().expect("WarnBanner");
        assert_eq!(wb.text().as_deref(), Some(TEXT_WARN_BANNER), "警示文案固定");
        assert_eq!(wb.icon_text().as_deref(), Some("⚠"), "警示图标");
        assert!(wb.has_fields_line(), "「涉及:端口」第二行应出现");
        assert!(d3.default_focus_is_cancel(), "危险级别同样默认焦点「取消」");
        // 无瞬断字段时仍出 WarnBanner（级别强制），只是第二行隐藏
        let spec4 = ConfirmSpec { warn_fields: &[], ..spec3 };
        let d4 = ConfirmDialog::new(&screen, &spec4, ConfirmLevel::L2Plus)
            .expect("ConfirmDialog L2+ (no fields)");
        assert!(d4.has_warn_banner());
        assert!(
            !d4.warn_banner().expect("WarnBanner").has_fields_line(),
            "无字段则第二行隐藏"
        );
        drop(d4);
        drop(d3);
    }

    // ── Minor 1：非法参数**响亮失败**（反例单测）+ Minor 3：影响范围盒内截断 ──
    {
        // 反例（此前一律被静默"修正"成"正常"，把调用方的真 bug 粉饰掉）。
        assert!(
            matches!(Stepper::new(&screen, 10, 1, 5, 1), Err(LvglError::InvalidArgument(_))),
            "min > max 必须 Err，不得静默交换"
        );
        assert!(
            matches!(Stepper::new(&screen, 0, 10, 5, 0), Err(LvglError::InvalidArgument(_))),
            "step <= 0 必须 Err，不得静默当 1"
        );
        assert!(
            matches!(
                Stepper::with_value_width(&screen, 0, 10, 5, 1, Dimens::IPV4_VALUE_W / 2),
                Err(LvglError::InvalidArgument(_))
            ),
            "值区宽 < TOUCH_MIN 必须 Err，不得静默抬到下限"
        );
        assert!(
            matches!(
                MultiSelectChips::new(&screen, &["a", "b"], 0, Dimens::CHIP_MIN_W),
                Err(LvglError::InvalidArgument(_))
            ),
            "columns == 0 必须 Err，不得静默 max(1)"
        );
        assert!(
            matches!(
                MultiSelectChips::new(&screen, &["a", "b"], 2, Dimens::CHIP_MIN_W - 1),
                Err(LvglError::InvalidArgument(_))
            ),
            "cell_w < CHIP_MIN_W 必须 Err，不得静默抬到 96"
        );
        // 正例：边界值不得被误伤（合法性判据只挡非法组合）。
        let ok = Stepper::with_value_width(&screen, 0, 10, 5, 1, Dimens::TOUCH_MIN);
        assert!(ok.is_ok(), "value_w == TOUCH_MIN 是合法下界");
        drop(ok);

        // Minor 3：超长「影响范围」文案必须在固定 2 行盒内**截断**（`DOTS`），
        // 不得增高把后续「将修改的字段」段标题压下去。
        let long_impact = "修改将立即生效无需重启装置并将重写全部运行参数包括端口号心跳间隔与并网策略请仔细核对全部字段";
        let spec_long = ConfirmSpec {
            title: "确认保存运行参数",
            impact: long_impact,
            details: &[],
            warn_fields: &[],
        };
        let dl = ConfirmDialog::new(&screen, &spec_long, ConfirmLevel::L1)
            .expect("ConfirmDialog (long impact)");
        disp.refr_now_for_test();
        let box_h = 2 * TextSlot::Body.px() as i32;
        assert_eq!(
            dl.impact().coords().height() as i32,
            box_h,
            "影响范围恒在固定 2 行盒内（DOTS 截断 ⇒ 不溢出、不压后续标题）"
        );
        drop(dl);
    }

    // ── ② WarnBanner 单独可用（P2 未保存提示 / P3 超限提示同款）───────────
    {
        let b = WarnBanner::new(&screen, Dimens::CONTENT_W, "有未保存修改", &[]).expect("WarnBanner");
        assert_eq!(b.text().as_deref(), Some("有未保存修改"));
        disp.refr_now_for_test();
        assert_eq!(b.size().1, Dimens::BANNER_H, "警示行高 56");
        assert!(!b.has_fields_line(), "无字段时不出第二行");
        let b2 = WarnBanner::new(&screen, Dimens::CONTENT_W, "检索范围超限，请缩小时间范围", &[]);
        // ⚠️ 该文案含全角逗号，不在字符集内 —— 这里只断言**不 panic**（真机文案改半角即可）
        assert!(b2.is_ok());
        drop(b2);
        drop(b);
    }

    // ── ②④ Toast：3 s 生命周期（注入时钟）+ 语义色 ───────────────────────
    {
        let t0 = Instant::now();
        let toast = Toast::new_at(&screen, ToastTone::Success, "✓", "已保存并生效", t0)
            .expect("Toast");
        assert_eq!(toast.tone(), ToastTone::Success);
        assert_eq!(toast.tone().accent(), Palette::OK, "成功 = 绿");
        assert_eq!(ToastTone::Failure.accent(), Palette::DANGER, "失败 = 红");
        assert_eq!(ToastTone::Warning.accent(), Palette::STALE, "警示 = 琥珀");
        assert_eq!(toast.text().as_deref(), Some("已保存并生效"));
        assert_eq!(toast.icon_text().as_deref(), Some("✓"));
        disp.refr_now_for_test();
        assert_eq!(
            toast.obj().size(),
            (Dimens::TOAST_W, Dimens::TOAST_H),
            "Toast ≤480×60"
        );
        // ① 左缘色条是独立装饰子对象：此前无断言覆盖，拥有型句柄若被 Drop 即整条消失。
        assert!(toast.accent_bar().is_alive(), "左缘色条仍在树上");
        assert_eq!(
            toast.accent_bar().size(),
            (Dimens::TOAST_ACCENT_W, Dimens::TOAST_H),
            "左缘色条 = 4×60（布局趟后落定）"
        );
        // ①′ **Important 1 的坐标断言（`coords()`，不是 `size()`）**：色条必须**贴 Toast
        // 外缘左缘 + 通高**。此前 `theme::toast` 带 16 px 内边距而色条按外缘写坐标，二者
        // 叠加 ⇒ 实测 `accent.x1 = toast.x1 + 17`、可视高被父内容区裁到 28 px。只看
        // `size()`（= 4×60 的设定值）永远看不出这种错位 —— 这正是该缺陷此前被放过的原因。
        let tb = toast.obj().coords();
        let ab = toast.accent_bar().coords();
        assert_eq!(ab.x1, tb.x1, "左缘色条须贴 Toast 左缘（不再内缩 pad+border）");
        assert_eq!(ab.y1, tb.y1, "左缘色条须贴 Toast 上缘");
        assert_eq!(ab.y2, tb.y2, "左缘色条须与 Toast **通高**（底部不被裁）");
        assert_eq!(
            ab.x2,
            tb.x1 + Dimens::TOAST_ACCENT_W - 1,
            "左缘色条宽 = 4 px（UI §7.2）"
        );
        assert_eq!(
            toast.expires_at(),
            t0 + Duration::from_millis(Timing::TOAST_MS),
            "过期时刻 = 起点 + 3 s"
        );
        assert!(!toast.is_expired(t0 + Duration::from_millis(2999)), "未满 3 s 不过期");
        assert!(toast.is_expired(t0 + Duration::from_millis(3000)), "满 3 s 过期");
        assert_eq!(toast.remaining(t0), Duration::from_millis(3000));
        assert_eq!(toast.remaining(t0 + Duration::from_secs(10)), Duration::ZERO);
        drop(toast);
    }

    // ── ④ EmptyState / UnavailableState：语义可区分且都真的建出来了 ───────
    {
        let empty = EmptyState::new(&screen, "○", "近 24 小时无告警").expect("EmptyState");
        assert_eq!(empty.semantics(), StateSemantics::Empty);
        assert_eq!(empty.text().as_deref(), Some("近 24 小时无告警"));
        assert_eq!(empty.icon_text().as_deref(), Some("○"));
        disp.refr_now_for_test();
        assert_eq!(empty.size().0, Dimens::CONTENT_W, "空态占内容区宽 992");
        // ① 两行容器（图标行 / 文字行）都必须真的在树上（句柄被 Drop ⇒ 只剩空壳）。
        assert_eq!(empty.obj().child_count(), 2, "空态两行容器都在树上");

        let unus = UnavailableState::new(
            &screen,
            UnavailableKind::Interlock,
            "无法获知联锁状态",
        )
        .expect("UnavailableState");
        assert_eq!(unus.semantics(), StateSemantics::Unavailable);
        assert_ne!(
            unus.semantics(),
            empty.semantics(),
            "④ 二者语义必须不同（不可用 ≠ 空）"
        );
        assert_eq!(unus.title().as_deref(), Some("联锁状态不可用"));
        assert_ne!(unus.title().as_deref(), Some("未联锁"), "F16.6：不得退化成「未联锁」");
        assert_eq!(unus.icon_text().as_deref(), Some("?"), "问号图形，不复用 ✓ / ⚠");
        assert_eq!(unus.reason().as_deref(), Some("无法获知联锁状态"));
        // ① 三行容器（图标 / 标题 / 原因）都必须真的在树上。
        assert_eq!(unus.obj().child_count(), 3, "不可用态三行容器都在树上");

        // 另两个场景的标题由枚举给定（调用方无从写错）
        let a = UnavailableState::new(&screen, UnavailableKind::AlertSource, "采集进程未上报")
            .expect("UnavailableState (alert)");
        assert_eq!(a.title().as_deref(), Some("告警源不可用"));
        let u = UnavailableState::new(&screen, UnavailableKind::Audit, "审计目录不可写")
            .expect("UnavailableState (audit)");
        assert_eq!(u.title().as_deref(), Some("审计记录不可用"));

        drop(u);
        drop(a);
        drop(unus);
        drop(empty);
    }

    // ── ⑦ B2b-1：三个**输入型**控件（SegmentedControl / Ipv4Stepper / DateTimeStepper）──
    {
        use crate::ui::controls::{
            DateTimeStepper, DateTimeValue, Ipv4Stepper, SegmentedControl, DAY_MAX, DAY_MIN,
            DATETIME_HEADERS, DATETIME_TOTAL_H, DATETIME_TOTAL_W, HOUR_MAX, IPV4_TOTAL_H,
            IPV4_TOTAL_W, MINUTE_MAX, MONTH_MAX, MONTH_MIN, SEGMENT_H, SEGMENT_MIN_W, YEAR_MAX,
            YEAR_MIN,
        };

        // ═══ SegmentedControl ═══════════════════════════════════════════════
        let seg_w = 4 * SEGMENT_MIN_W;
        let seg = SegmentedControl::new(&screen, &["ERROR", "WARN", "INFO", "DEBUG"], seg_w, 0)
            .expect("SegmentedControl");
        assert_eq!(seg.count(), 4, "段数 = 选项数");
        // 若实现漏了构造期的 `bm.set_selected(start)`，"回退值"会让 selected() 仍是 0 ——
        // 故**必须**同时断 LVGL 侧原值（raw_selected），这条才是敏感的。
        assert_eq!(seg.selected(), 0, "构造给 0 ⇒ 选中 0");
        assert_eq!(seg.raw_selected(), Some(0), "**LVGL 侧**确实是第 0 段（不是回退值）");
        // ── ⑦.0 控制位读回（**CD5 的回归锁**：分段控件"选中态"是否真的会产生）────────
        // 读回通道 = `ButtonMatrix::has_ctrl`（LVGL 侧真值，**不是** Rust 侧缓存/回退值）。
        use crate::lvgl::widgets::{CTRL_CHECKABLE, CTRL_CHECKED, CTRL_DISABLED};
        let bm = seg.matrix();
        // 缺陷本体：v9.5.0 的 toggle **要求**该段 CHECKABLE，否则点击永不产生 CHECKED。
        // 改什么会让本条变红：删掉构造期的 `set_ctrl_all(CTRL_CHECKABLE)` ⇒ 四段全 false。
        for i in 0..4 {
            assert!(
                bm.has_ctrl(i, CTRL_CHECKABLE),
                "第 {i} 段必须 CHECKABLE（删掉 set_ctrl_all(CTRL_CHECKABLE) 即红）"
            );
        }
        // **互斥性**（单选语义的核心）：恰有一段 CHECKED、其余全 false。
        // 改什么会让本条变红：构造期漏 `set_ctrl(start, CTRL_CHECKED)` ⇒ 第 0 段 false。
        assert!(
            bm.has_ctrl(0, CTRL_CHECKED),
            "初始选中段必须带 CHECKED 控制位（否则 §5.2 的选中样式永不绘制）"
        );
        for i in 1..4 {
            assert!(!bm.has_ctrl(i, CTRL_CHECKED), "第 {i} 段不得 CHECKED（单选互斥）");
        }
        // 段文本逐条核对：写错地图 / 少一段 / 顺序颠倒 ⇒ 红。
        for (i, want) in ["ERROR", "WARN", "INFO", "DEBUG"].iter().enumerate() {
            assert_eq!(seg.option(i).as_deref(), Some(*want), "第 {i} 段文本");
        }
        assert_eq!(seg.option(4), None, "越界选项 ⇒ None（不得 panic）");

        seg.set_selected(2);
        assert_eq!(seg.selected(), 2, "程序化设选中可读回");
        assert_eq!(
            seg.raw_selected(),
            Some(2),
            "set_selected 必须真的落到 LVGL（只改 Rust 侧缓存即红）"
        );
        // 切换后：新选中带位 **且旧选中必须已清** —— 只看新选中会漏掉"两个都亮"的缺陷。
        // 本条的实际回归锁是构造期的 `set_one_checked(true)`，**不是** `set_selected` 自己清位：
        // LVGL 在 `one_check` 路径里自动清位（`lv_buttonmatrix.c:165–167`），故去掉
        // `set_selected` 里的 `clear_ctrl_all(CTRL_CHECKED)` ⇒ 本条**仍绿**（已实测；该行已删）。
        // 已实测：注释掉 `set_one_checked(true)` ⇒ 本条变红。
        assert!(bm.has_ctrl(2, CTRL_CHECKED), "set_selected(2) ⇒ 第 2 段带 CHECKED");
        assert!(
            !bm.has_ctrl(0, CTRL_CHECKED),
            "旧选中段（0）的 CHECKED 必须被清除 —— 否则界面上两段同时高亮"
        );
        seg.set_selected(99);
        assert_eq!(seg.selected(), 3, "越界下标**夹取**到最后一 段（不得 panic）");
        assert_eq!(seg.raw_selected(), Some(3), "夹取后的值同样落到 LVGL");
        // 越界路径的**状态自洽**：夹到第 3 段带位、第 2 段已清、全局恰有一段亮。
        assert!(bm.has_ctrl(3, CTRL_CHECKED), "越界夹到第 3 段 ⇒ 该段 CHECKED");
        assert!(!bm.has_ctrl(2, CTRL_CHECKED), "越界切换后第 2 段必须已清");
        let checked_count = (0..4).filter(|&i| bm.has_ctrl(i, CTRL_CHECKED)).count();
        assert_eq!(checked_count, 1, "任意时刻**恰有一段** CHECKED（单选不变量）");
        disp.refr_now_for_test();
        assert_eq!(seg.size(), (seg_w, SEGMENT_H), "分段控件 = 调用方给定宽 × 高 48（§5.1 #4）");

        // 禁用：读回口径是 **LVGL 的状态位**（输入路径据此拒发事件，见 `lv_indev.c`）
        assert!(!seg.is_disabled(), "默认可用");
        seg.set_disabled(true);
        assert!(seg.is_disabled(), "set_disabled(true) ⇒ LV_STATE_DISABLED 置位");
        // 逐段 DISABLED 控制位（§5.2「禁用 字 `#5A6780`」的**触发源**）。
        // 改什么会让本条变红：`set_disabled` 里去掉 `set_ctrl_all(CTRL_DISABLED)`。
        assert!(bm.has_ctrl(0, CTRL_DISABLED), "禁用 ⇒ 每段带 DISABLED 控制位");
        seg.set_disabled(false);
        assert!(!seg.is_disabled(), "恢复后状态位清掉");
        assert!(!bm.has_ctrl(0, CTRL_DISABLED), "恢复 ⇒ DISABLED 控制位被清除");

        // `on_change` 接线：派发 `VALUE_CHANGED`（键矩阵类处理器在按下时派发的正是它）⇒
        // 回调查到的应当是**当前**下标。初值取 usize::MAX：回调若压根没接上，断言即红。
        let hits = Rc::new(Cell::new(usize::MAX));
        {
            let h = hits.clone();
            seg.set_on_change(move |i| h.set(i));
        }
        seg.send_event(EventCode::VALUE_CHANGED);
        assert_eq!(hits.get(), 3, "回调载荷 = 当前选中段（接线断了则仍是 usize::MAX）");
        seg.set_selected(1);
        seg.send_event(EventCode::VALUE_CHANGED);
        assert_eq!(
            hits.get(),
            1,
            "回调读的是**实时**下标（改成构造期缓存值 / 常量即红）"
        );

        // ── ⑦.0b 回退值（`current`）必须由**事件回调写回共享 Cell** ──────────────
        // 序列复刻真机路径 ①②③：点选段 2 → LVGL 更新 `btn_id_sel` 并派发
        // `VALUE_CHANGED` → 之后 `PRESS_LOST` 把 `btn_id_sel` 复位为 NONE。
        // 这里不经真实触摸：用 `matrix().set_selected` 直接驱动 LVGL 的 `btn_id_sel`
        // （就是 `lv_buttonmatrix_set_selected_button`），再派发 `VALUE_CHANGED`。
        seg.set_selected(0); // current = 0、btn_id_sel = 0（点选前的状态）
        seg.matrix().set_selected(2); // 模拟"用户点选了段 2"（绕过本类型的 clamp）
        seg.send_event(EventCode::VALUE_CHANGED); // 触发回调 ⇒ 应把 2 写回共享 Cell
        // 改什么会让本条变红：`current` 退回裸 `Cell<usize>` 且回调里写 `current.clone()`
        // （`Cell` 的 clone 是**值拷贝** ⇒ 死写副本）—— 此时 `fallback_index()` 仍为 0。
        assert_eq!(
            seg.fallback_index(),
            2,
            "事件回调必须把选中下标写回共享 Cell（死写 ⇒ 仍为 0）"
        );

        // 模拟 `PRESS_LOST`：LVGL 把 `btn_id_sel` 复位为 `BUTTONMATRIX_BUTTON_NONE`（0xFFFF）。
        // 只用 `matrix().set_selected`（本类型的 `set_selected` 会把 0xFFFF 夹成 count−1）。
        seg.matrix().set_selected(0xFFFF);
        assert_eq!(seg.raw_selected(), None, "前提：LVGL 已报无选中");
        // 改什么会让本条变红：同 a)（死写 ⇒ current 仍为构造值 0，回退到点选前的旧下标）。
        assert_eq!(
            seg.selected(),
            2,
            "无选中时的回退值必须是用户最后选中的段（死写 ⇒ 退回构造值 0）"
        );
        // 该步（`matrix().set_selected`）只动 LVGL，不得反过来改本类型的回退值。
        assert_eq!(seg.fallback_index(), 2, "回退值不因 LVGL 报无选中而改变");
        seg.set_selected(0); // 复位，避免影响后续断言的状态基线

        // 参数非法**响亮失败**（不静默修正）
        assert!(
            matches!(
                SegmentedControl::new(&screen, &["a", "b"], SEGMENT_MIN_W, 0),
                Err(LvglError::InvalidArgument(_))
            ),
            "整控件宽 < 段数 × 96 必须 Err（不得静默压缩段宽）"
        );
        assert!(
            matches!(
                SegmentedControl::new(&screen, &[], seg_w, 0),
                Err(LvglError::InvalidArgument(_))
            ),
            "空选项必须 Err"
        );
        assert!(
            matches!(
                SegmentedControl::new(&screen, &["a"], Dimens::CONTENT_W + SEGMENT_MIN_W, 0),
                Err(LvglError::InvalidArgument(_))
            ),
            "整控件宽超内容区必须 Err"
        );

        // ── ⑦.0c **I1** 回调内自替换 `set_on_change`：不得 panic、回调体必须跑完 ────────
        // 复刻评审探针 **P4**：旧实现在调用用户回调期间持着槽的 `try_borrow_mut`，用户回调里
        // 再调 `set_on_change` ⇒ 其中的 `borrow_mut()` panic；该 panic 被 `src/lvgl/event.rs`
        // 的 `catch_unwind` 拦下 ⇒ **回调体后半段不执行且用例仍全绿**（静默丢通知 + 半执行）。
        // 修法（「取出 → 转发 → 槽仍为空才放回」，放回走 `PutBack` 守卫）见 `controls.rs`
        // 的 `fire_index` 上方语义说明。
        // 改什么会红：把 `fire_*` / `replace_*` 改回"调用期持借用 + `borrow_mut`"⇒ panic 被
        // 桥吞掉 ⇒ 下面 `body_completed` 仍为 false ⇒ 本断言变红（**已实测**，见交付报告）。
        let seg = Rc::new(seg); // 回调内需要再次拿到 `&SegmentedControl`（自替换）
        let body_completed = Rc::new(Cell::new(false));
        let replaced_ran = Rc::new(Cell::new(false));
        {
            let weak = Rc::downgrade(&seg);
            let done = Rc::clone(&body_completed);
            let ran = Rc::clone(&replaced_ran);
            seg.set_on_change(move |_i| {
                if let Some(s) = weak.upgrade() {
                    let ran = Rc::clone(&ran);
                    // **回调内自替换**：旧实现此处 panic（槽正被可变借用持有）。
                    s.set_on_change(move |_j| ran.set(true));
                }
                done.set(true); // 回调体末尾哨兵
            });
        }
        seg.send_event(EventCode::VALUE_CHANGED);
        assert!(
            body_completed.get(),
            "回调体内自替换 set_on_change 必须**不 panic**且回调体跑完（旧实现 panic 被 \
             event.rs 的 catch_unwind 吞掉 ⇒ 本哨兵仍为 false）"
        );
        assert!(
            !replaced_ran.get(),
            "替换语义：本次通知仍由旧回调执行，新回调自**下一次**通知起生效"
        );
        seg.send_event(EventCode::VALUE_CHANGED);
        assert!(
            replaced_ran.get(),
            "下一次通知必须由**替换后**的新回调执行（否则新回调根本没接上）"
        );
        drop(seg);

        // ── ⑦.0d **I1′** 用户回调 panic ⇒ 槽**不得永久空置**（后续事件仍必须到达）──────
        // 缺陷本体（本单元评审实测）：旧写法 `take_cb → f(v) → put_back_cb` 里，用户回调
        // panic ⇒ 展开**跳过**"放回" ⇒ 槽永久 `None` ⇒ 此后**所有**通知静默丢失。
        // `event.rs` 的桥只报"本次事件作废"（`catch_unwind` 在桥），**看不见**槽已被抽空；
        // 评审探针实测（SegmentedControl + `send_event` ×3）：`hits_after_three_events = 1`。
        // 修法 = `controls.rs` 的 `PutBack` 守卫（`Drop` 里放回，见其文档）。
        // 改什么会红：把 `fire_index` 改回"`f(v)` 之后手动放回"⇒ 计数停在 1（**已实测**）。
        {
            let seg = SegmentedControl::new(&screen, &["A", "B"], 2 * SEGMENT_MIN_W, 0)
                .expect("SegmentedControl（panic 回归）");
            let hits = Rc::new(Cell::new(0usize));
            let panics_left = Rc::new(Cell::new(1usize));
            {
                let hits = Rc::clone(&hits);
                let panics_left = Rc::clone(&panics_left);
                seg.set_on_change(move |_i| {
                    hits.set(hits.get() + 1);
                    if panics_left.get() > 0 {
                        panics_left.set(panics_left.get() - 1);
                        // 用户回调**故意** panic（由 event.rs 桥的 catch_unwind 拦下，
                        // **不会**跨 C 帧展开；测试自身因此不红）。
                        panic!("E2E 回归：用户回调故意 panic（回归后不得再出现）");
                    }
                });
            }
            seg.send_event(EventCode::VALUE_CHANGED);
            assert_eq!(hits.get(), 1, "第一次事件到达回调（panic 发生在回调体内）");
            seg.send_event(EventCode::VALUE_CHANGED);
            seg.send_event(EventCode::VALUE_CHANGED);
            assert_eq!(
                hits.get(),
                3,
                "**panic 之后的每次事件仍必须到达**（旧写法：槽永久空置 ⇒ take_cb 恒 None \
                 ⇒ 计数停在 1，且无任何报错；评审实测 hits_after_three_events=1）"
            );
        }

        // ═══ Ipv4Stepper ════════════════════════════════════════════════════
        let ipv4 = Ipv4Stepper::new(&screen, [192, 168, 1, 10]).expect("Ipv4Stepper");
        assert_eq!(ipv4.octets(), [192, 168, 1, 10], "四段值往返（读自四个 Stepper）");
        assert_eq!(ipv4.text().as_deref(), Some("192.168.1.10"), "汇总标签文本");
        disp.refr_now_for_test();
        assert_eq!(
            ipv4.obj().size(),
            (IPV4_TOTAL_W, IPV4_TOTAL_H),
            "整件 = 四段 792 + 缝 + 汇总 = 992 × 64（`IPV4_TOTAL_W == CONTENT_W`；CD1/CD2）"
        );
        // ── 子树存活断链（**所有权纪律**的探针）──
        // 行容器应有 5 个子对象（4 段 + 1 汇总）、每段应有 3 个（`−` / 值区 / `＋`）。
        // 任何**拥有型句柄被构造器 Drop** ⇒ LVGL 级联删除它 ⇒ child_count 立刻变小；
        // 值区句柄若被 Drop，`display()` 同时变 `None`（本文件的 `_value_box` 同款缺陷）。
        assert_eq!(ipv4.obj().child_count(), 5, "4 段 + 1 汇总标签都必须挂在行容器上");
        for i in 0..4 {
            let s = ipv4.segment(i).expect("段");
            assert_eq!(s.obj().child_count(), 3, "第 {i} 段 = − / 值区 / ＋ 三件");
            assert!(
                s.display().is_some(),
                "第 {i} 段值区文本可读回（为 None ⇒ 值区子树已被级联删除）"
            );
        }
        assert!(ipv4.segment(4).is_none(), "越界段 ⇒ None（不得 panic）");
        assert_eq!(ipv4.segment(0).expect("段 0").display().as_deref(), Some("192"));
        assert_eq!(ipv4.segment(3).expect("段 3").display().as_deref(), Some("10"));

        // ── 段内封闭 0–255 + **跨段不进位** + 越界夹取 ──
        let s0 = ipv4.segment(0).expect("段 0");
        s0.set_value(255);
        assert_eq!(s0.value(), 255);
        assert!(s0.plus_disabled(), "value == 255（段上界）⇒ ＋ 禁用（TT-03）");
        assert!(!s0.minus_disabled(), "上界处 − 仍可用");
        assert_eq!(ipv4.octets()[0], 255, "`octets()` 读的确实是 LVGL 真值");
        assert_eq!(
            ipv4.octets()[1],
            168,
            "**跨段不进位**：第 0 段到顶不得改动第 1 段（UI §5.3）"
        );
        s0.set_value(9999);
        assert_eq!(s0.value(), 255, "越界输入被夹取（不得写进 9999 / 不得 panic）");
        s0.set_value(0);
        assert!(s0.minus_disabled(), "value == 0（段下界）⇒ − 禁用");
        assert!(!s0.plus_disabled(), "下界处 ＋ 仍可用");

        // 程序化设四段：值 + 汇总文本都要同步（汇总漏更新 ⇒ 红）
        ipv4.set_octets([10, 0, 0, 1]);
        assert_eq!(ipv4.octets(), [10, 0, 0, 1], "set_octets 落到四个段上");
        assert_eq!(ipv4.text().as_deref(), Some("10.0.0.1"), "汇总文本随 set_octets 同步");
        assert_eq!(
            ipv4.segment(1).expect("段 1").display().as_deref(),
            Some("0"),
            "第 1 段文本随 set_octets 同步"
        );

        // 整件禁用 ⇒ 逐段两端都禁用；恢复后按当前值重算
        ipv4.set_disabled(true);
        let s1 = ipv4.segment(1).expect("段 1");
        assert!(
            s1.minus_disabled() && s1.plus_disabled(),
            "整件禁用 ⇒ 该段两端都禁用（转发到 Stepper::set_disabled）"
        );
        ipv4.set_disabled(false);
        let s_off_bound = ipv4.segment(0).expect("段 0"); // 值 10：非任何一端的边界
        assert!(
            !s_off_bound.minus_disabled() && !s_off_bound.plus_disabled(),
            "恢复后按当前值重算：值 10 在 0–255 中间 ⇒ 两端都可用（若仍禁用 ⇒ 恢复没生效）"
        );
        assert!(
            ipv4.segment(1).expect("段 1").minus_disabled(),
            "同一次恢复里，值为 0 的那一段仍应 − 禁用（按值逐个重算，**不是**一刀切清掉）"
        );
        drop(ipv4);

        // ═══ DateTimeStepper ════════════════════════════════════════════════
        let want = DateTimeValue::from_parts(2026, 5, 20, 13, 42);
        let dt = DateTimeStepper::new(&screen, want).expect("DateTimeStepper");
        // **I3**：列头顺序取自生产侧**真源** [`DATETIME_HEADERS`]，不再硬编码一份（此前此处
        // 写死 `["年","月","日","时","分"]`，正是 `controls.rs` 注释声称"已避免"的第二真源）。
        assert_eq!(
            dt.column_headers(),
            DATETIME_HEADERS.map(String::from),
            "列头 5 个、逐字、顺序 == DATETIME_HEADERS 真源（§5.1 #8）"
        );
        assert_eq!(dt.value(), want, "五分量往返");
        disp.refr_now_for_test();
        assert_eq!(
            dt.obj().size(),
            (DATETIME_TOTAL_W, DATETIME_TOTAL_H),
            "五列铺满内容区有效宽 × 106（列头 26 + 缝 16 + 步进 64；CD3/CD4）"
        );
        assert_eq!(
            dt.obj().child_count(),
            10,
            "5 个列头 + 5 列步进；任一拥有型句柄被 Drop ⇒ 立刻小于 10"
        );
        assert_eq!(dt.column(0).expect("年列").display().as_deref(), Some("2026"));
        assert_eq!(dt.column(4).expect("分列").display().as_deref(), Some("42"));
        assert!(dt.column(5).is_none(), "越界列 ⇒ None（不得 panic）");

        // ── 分量封闭 + **跨分量不进位**（UI §5.3：进借位在应用侧）──
        let month = dt.column(1).expect("月列");
        month.set_value(MONTH_MAX);
        assert_eq!(dt.value().month, 12, "月列可到 12");
        assert!(month.plus_disabled(), "月到 12 ⇒ ＋ 禁用（分量封闭）");
        month.set_value(MONTH_MAX + 1);
        assert_eq!(dt.value().month, 12, "越上界被夹 ⇒ 停在 12");
        assert_eq!(
            dt.value().year,
            2026,
            "**跨分量不进位**：月到顶不得推高年（进位归应用侧）"
        );
        let day = dt.column(2).expect("日列");
        day.set_value(DAY_MAX);
        assert_eq!(dt.value().day, 31, "日列可到 31");
        assert!(day.plus_disabled(), "日到 31 ⇒ ＋ 禁用");
        let hour = dt.column(3).expect("时列");
        hour.set_value(HOUR_MAX);
        assert!(hour.plus_disabled(), "时到 23 ⇒ ＋ 禁用");
        hour.set_value(HOUR_MAX - 1);
        assert!(!hour.plus_disabled(), "时 = 22 ⇒ ＋ 可用（禁用只在界上）");
        let minute = dt.column(4).expect("分列");
        minute.set_value(MINUTE_MAX);
        assert!(minute.plus_disabled(), "分到 59 ⇒ ＋ 禁用");
        let year = dt.column(0).expect("年列");
        year.set_value(YEAR_MAX);
        assert!(year.plus_disabled(), "年到 2100（上界）⇒ ＋ 禁用");
        year.set_value(YEAR_MIN);
        assert!(year.minus_disabled(), "年到 1970（下界）⇒ − 禁用");

        // `set_value` 逐分量夹取（结构体字面量可越界 —— 公开字段，故必须由 set_value 收口）
        dt.set_value(DateTimeValue {
            year: 3000,
            month: 0,
            day: 0,
            hour: 99,
            minute: 99,
        });
        assert_eq!(
            dt.value(),
            DateTimeValue::from_parts(YEAR_MAX, MONTH_MIN, DAY_MIN, HOUR_MAX, MINUTE_MAX),
            "越界分量逐个夹到封闭区间（不得 panic / 不得写进 3000）"
        );

        // 整件禁用 ⇒ 逐列两端都禁用
        dt.set_disabled(true);
        assert!(
            year.minus_disabled() && year.plus_disabled(),
            "整件禁用 ⇒ 该列两端都禁用"
        );
        dt.set_disabled(false);
        assert!(year.plus_disabled(), "恢复后按当前值（2100）重算 ⇒ ＋ 仍禁用");
        drop(dt);
    }

    // ── ⑩ app 层 Toast 的**同步函数**：连推 50 拍必须**双零增长** ──────────────
    //
    // **B3-2c 整改 重要 1**：`app.rs::sync_toast_view` 是 `App::tick` **每拍都会跑**的一段
    // LVGL 写操作（把状态层的 Toast 记录落到那**一个**对象上），而 `App::tick` **只在真实
    // 二进制里跑**（`App` 的构造需要 LVGL 会话 + 控制通道客户端）⇒ 本仓的离屏链路
    // **看不到它**。评审实测：在 `App::sync_toast` 首行插一次 `add_style`
    // （`Obj::add_style` **只增不删** ⇒ 样式表无界增长、对象数纹丝不动）⇒
    // 整改前 **354 + 6 + 6 全绿、0 failed**。`shell_chain` 那条双零增长网盖不到：
    // 它推的是 `Shell::tick`，而 `Shell::tick` **既不渲染页面、也不碰 app 层 Toast**。
    //
    // 修法 = 把那段函数体抽成**自由函数** `sync_toast_view`（`App::sync_toast` 只剩一行转调），
    // 于是本链路（**有 `Display`、读得到 `PROBE_MOUNTS` / `PROBE_STYLE_ATTACHES`**）能**直接
    // 调它 N 次** —— "被断言的"与"生产跑的"是**同一个体**。`App::tick` 的**调用点**离屏看不到，
    // 由源码哨 `app::tests::app_toast_is_built_once_and_synced_from_tick` 钉住
    // （这一局限在 `sync_toast_view` 的文档里如实登记）。
    //
    // **改什么会让本条变红**（**已实测**）：在 `sync_toast_view` 里插一次
    // `toast.obj().add_style(..)` ⇒ 第 2 条断言红；插一次 `Obj::create(..)` ⇒ 第 1 条红。
    {
        use crate::ui::components::{Toast, ToastTone};
        use std::sync::atomic::Ordering;

        let top = crate::lvgl::widgets::layer_top().expect("layer_top（app Toast 的父）");
        let toast = Toast::new(
            &top,
            ToastTone::Failure,
            crate::ui::pages::p4_interlock::ICON_FAIL,
            crate::state::TRANSPORT_FAIL_TEXT,
        )
        .expect("app 层 Toast 的等价体");
        crate::ui::pages::set_visible(toast.obj(), false);

        let mounts_before = crate::lvgl::obj::PROBE_MOUNTS.load(Ordering::SeqCst);
        let attaches_before = crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(Ordering::SeqCst);
        for i in 0..50u64 {
            // **两支都走**：显（有文案 ⇒ `set_text` + 显示）/ 隐（`None` ⇒ 只切可见性）。
            // 只推一支的话，另一支里的对象 churn 与挂样式都抓不到。
            let view = if i % 2 == 0 {
                Some("操作失败")
            } else {
                None
            };
            crate::app::sync_toast_view(&toast, view);
            // **逐拍复核**（同款理由：等推完 50 拍会先撞 LVGL 的单对象样式上限
            // `lv_obj_style.c:110`，那是挂死不是红）。
            assert_eq!(
                crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(Ordering::SeqCst) - attaches_before,
                0,
                "app 层 Toast 同步第 {i} 拍往对象上挂样式了（`Obj::add_style` 只增不删）"
            );
        }
        assert_eq!(
            crate::lvgl::obj::PROBE_MOUNTS.load(Ordering::SeqCst) - mounts_before,
            0,
            "app 层 Toast 同步连推 50 拍（显隐交替）不得新建任何 LVGL 对象 —— \
             对象在装配期建一次，运行期只切可见性 / 换文本"
        );
        assert_eq!(
            crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(Ordering::SeqCst) - attaches_before,
            0,
            "app 层 Toast 同步连推 50 拍**也不得往对象上挂任何样式**：`Obj::add_style` 只增不删 \
             ⇒ 「每拍挂一条样式」= 样式表无界增长，而**对象数不变** —— 评审的破坏性探针 \
             （在 `sync_toast` 首行插 `add_style`）正是靠本条才抓得住"
        );
        // 语义顺带读回：末拍 `i = 49`（奇 ⇒ `None`）⇒ 那条应处于隐藏态；再推一拍 `Some` ⇒ 显示。
        assert!(toast.obj().is_hidden(), "`None` ⇒ 隐藏（末拍是 `None` 支）");
        crate::app::sync_toast_view(&toast, Some("操作失败"));
        assert!(!toast.obj().is_hidden(), "`Some(文案)` ⇒ 显示");
        assert_eq!(toast.text().as_deref(), Some("操作失败"), "文案就地写入");
        drop(toast);
    }

    // 释放顺序：先控件树，再 display，最后 deinit（与 A1/A2/A3 同口径）。
    drop(screen);
    drop(disp);
    lvgl::deinit();
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑦ B2a：P1 / P6 两页离屏链路（由 `src/lvgl/tests.rs` 的唯一 `#[test]` 串行调起）
// ═══════════════════════════════════════════════════════════════════════════

/// UI §6.1 的 P1 **主行卡高契约值**（`484×320`）。
///
/// 刻意把**契约值**（而非实现常量）钉在用例里：若实现回退成"按内容倒推卡高"（曾经是 294），
/// 本用例即变红 —— 这正是 B2a 规格评审 ② 要防的回归。
const MAIN_CARD_H_CONTRACT: i32 = 320;

/// 建一帧**全正常**的 v2 帧（各段齐备）—— 离屏用例的注入源。
///
/// 用例**自己造帧**（而不是去连 mupcd）：这正是"页面只吃注入参数"的可测性收益
/// （设计 §11.1「HMI 离屏渲染」是本模块最强的可测性支点）。
///
/// **`pub(crate)`**（B3-2c）：`ui/pages/mod.rs` 的 `frame_mark` 纯逻辑用例也要用同一份
/// 帧源 —— 判据（区块级冻结标记）与"帧长什么样"必须**同一真源**，否则两处各自造帧会漂移。
pub(crate) fn frame_healthy() -> mupc_display_proto::DisplayFrame {
    use mupc_display_proto::*;
    DisplayFrame {
        version: PROTO_VERSION,
        // v3 新增段（§15.2.2）：健康帧用例不涉外设 ⇒ 取默认（available=false）
        peripherals: Default::default(),
        seq: 7,
        ts_ms: 1_789_047_727_000,
        soc: Some(62.0),
        soc_source: SocSource::Bms,
        soc_flag: FieldFlag::Valid,
        run_state: Some(RunState::Charge),
        pcs_online: true,
        p_phase: [
            Field { v: Some(12.5), flag: FieldFlag::Valid },
            Field { v: Some(12.1), flag: FieldFlag::Valid },
            Field { v: Some(12.3), flag: FieldFlag::Valid },
        ],
        p_total: Field { v: Some(36.9), flag: FieldFlag::Valid },
        i_phase: [
            Field { v: Some(45.6), flag: FieldFlag::Valid },
            Field { v: Some(45.2), flag: FieldFlag::Valid },
            Field { v: Some(45.4), flag: FieldFlag::Valid },
        ],
        inconsistency: false,
        device: DeviceSection {
            ts_ms: 1_789_047_727_000,
            uptime_secs: Some(90_000),
            cpu_temp_c: Some(48.4),
            mem_used_pct: Some(31.2),
            iec104: LinkState::Connected,
            intercore: LinkState::Connecting,
            hmi_channel: LinkState::Unknown,
            control_source: ControlSource::LocalStrategy,
        },
        alarms: AlarmsSection {
            ts_ms: 1_789_047_727_000,
            available: true,
            items: vec![
                AlarmItem {
                    ts_ms: 1_789_047_727_000,
                    level: AlarmLevel::Warn,
                    message: "核间链路抖动".to_string(),
                },
                AlarmItem {
                    ts_ms: 1_789_047_000_000,
                    level: AlarmLevel::Error,
                    message: "直流侧过压".to_string(),
                },
            ],
        },
        info: InfoSection {
            firmware_version: "0.1.0".to_string(),
            build_time: None,
            model: Some("BECG-3568".to_string()),
            serial: None,
            service_scope: ServiceScope::LoopbackOnly,
            mgmt_ipv4: Some("192.168.3.118".to_string()),
        },
        interlock: InterlockSection::default(),
    }
}

/// 把一帧"打残"：三相各带不同降级标志 / 总卡未取数 / 告警源不可用 / 管理 IP 缺失。
fn frame_degraded() -> mupc_display_proto::DisplayFrame {
    use mupc_display_proto::*;
    let mut f = frame_healthy();
    f.p_phase[0] = Field { v: None, flag: FieldFlag::Offline };
    f.p_phase[1] = Field { v: None, flag: FieldFlag::NotRead };
    f.p_phase[2] = Field { v: None, flag: FieldFlag::RangeError };
    f.p_total = Field { v: None, flag: FieldFlag::NotRead };
    f.i_phase = [Field { v: None, flag: FieldFlag::Offline }; 3];
    f.alarms.available = false;
    f.alarms.items.clear();
    f.info.mgmt_ipv4 = None;
    f.info.firmware_version = String::new();
    f.device.uptime_secs = None;
    f.device.hmi_channel = LinkState::Disconnected;
    f
}

/// B2a 两页的全部场景（**必须**由 `src/lvgl/tests.rs` 在同一线程内顺序调起）。
///
/// 本链路**真的渲染**（内存 display + flush sink 逐行拷贝），故覆盖「装配 → 布局 → 像素」
/// 全链（验收项 ①）；文本 / 颜色 / 可见性读回覆盖验收项 ②③④⑤。
pub(crate) fn pages_chain() {
    use crate::state::{ChannelStatus, Freshness};
    use crate::ui::pages::{self, p1_status, p6_system, PageInput};
    use mupc_display_proto::{Field, FieldFlag, RunState, SocSource};

    lvgl::init().expect("lvgl::init (pages)");
    const W: u32 = Dimens::SCREEN_W as u32;
    const H: u32 = Dimens::SCREEN_H as u32;
    const BPP: usize = crate::lvgl::display::BYTES_PER_PIXEL;
    let sink = Rc::new(RefCell::new(vec![0u8; (W * H) as usize * BPP]));
    let mut disp = Display::create(W, H).expect("Display::create (pages)");
    {
        let s = sink.clone();
        disp.set_flush_cb(move |area: Area, px: &[u8]| {
            let mut b = s.borrow_mut();
            let row_bytes = area.width() as usize * BPP;
            for row in 0..area.height() as usize {
                let dy = area.y1 as usize + row;
                let dx = area.x1 as usize;
                let off = (dy * W as usize + dx) * BPP;
                let src = &px[row * row_bytes..(row + 1) * row_bytes];
                b[off..off + row_bytes].copy_from_slice(src);
            }
        });
    }
    let screen = Obj::screen().expect("Obj::screen (pages)");

    // 页容器：模拟 B2c 的摆放（内容区左上角 = (SIDE_PAD, HEADER_H)）。
    let host = Obj::create(&screen).expect("page host");
    host.set_pos(Dimens::SIDE_PAD, Dimens::HEADER_H);
    host.set_size(Dimens::CONTENT_W, Dimens::CONTENT_H);
    host.add_style(&theme::transparent(), StyleSelector::main());

    // ═══ P1 主状态页 ═══════════════════════════════════════════════════════
    {
        let p1 = p1_status::P1StatusPage::new(&host).expect("P1StatusPage::new");

        // ── ① 装配契约：页根尺寸 / 位置，内容首卡 y，且真的画出了像素 ──
        disp.refr_now_for_test();
        assert_eq!(
            p1.obj().size(),
            (Dimens::CONTENT_W, Dimens::CONTENT_H),
            "页根 = 内容区视口 992×624（B2c 的装配契约）"
        );
        let root_c = p1.obj().coords();
        assert_eq!(
            (root_c.x1, root_c.y1),
            (Dimens::SIDE_PAD, Dimens::HEADER_H),
            "页根由调用方摆放"
        );
        let soc_c = p1.soc_card().coords();
        assert_eq!(
            (soc_c.x1, soc_c.y1),
            (
                Dimens::SIDE_PAD,
                Dimens::HEADER_H
                    + Dimens::CONTENT_PAD_TOP
                    + Dimens::STATUS_CHIP_H
                    + Dimens::GAP_GROUP
            ),
            "SOC 卡落在「上内边距 + 通道条 + 同组缝」之后"
        );
        // ── ② 主行卡高 = UI §6.1 的契约 320（B2a 规格评审：此前按内容倒推得 294，
        //    把三相 / 装置 / 告警各区块整体上移 ~26 px）──
        // 两列主卡（SOC / PCS）在实现里共用同一个卡高常量，故 SOC 卡这一条即覆盖两卡。
        assert_eq!(
            p1.soc_card().size().1,
            MAIN_CARD_H_CONTRACT,
            "主行卡高 = UI §6.1 契约 320（此前 294 ⇒ 后续区块整体上移）"
        );
        // 后续区块位置：三相行 y = 内容顶 + 主卡高 + 同组缝（各数都由 theme 常量 + 契约 320 给出）。
        let phase_y = Dimens::HEADER_H
            + Dimens::CONTENT_PAD_TOP
            + Dimens::STATUS_CHIP_H
            + Dimens::GAP_GROUP
            + MAIN_CARD_H_CONTRACT
            + Dimens::GAP_GROUP;
        assert_eq!(
            p1.phase_card_obj(0).expect("A 相卡").coords().y1,
            phase_y,
            "三相行紧随 320 高的主卡之后（评审 ② 的「后续区块位置正确」）"
        );
        assert!(p1.alarm_card().is_alive(), "告警卡已建");
        let painted = sink.borrow().iter().filter(|b| **b != 0).count();
        assert!(painted > 10_000, "渲染后 sink 中应有成片非背景像素（实际 {painted}）");

        // ── 骨架无帧（Init）：全降级，且**不得**出现 0 ──
        assert_eq!(p1.soc_text().as_deref(), Some(pages::PLACEHOLDER), "无帧 ⇒ 占位符");
        assert_eq!(
            p1.channel_text().as_deref(),
            Some(p1_status::TEXT_CHANNEL_CONNECTING),
            "无帧 ⇒ 「正在连接数据通道」"
        );
        assert_eq!(p1.alarm_view(), p1_status::AlarmView::Unavailable, "无帧 ⇒ 告警不可用");

        // ── ② 正常帧 ──
        let f = frame_healthy();
        p1.render(&PageInput::live(&f));
        assert_eq!(p1.channel_text(), None, "通道正常 ⇒ 无通道条");
        assert!(!p1.stale_visible(), "帧新鲜 ⇒ 无「数据过期」标");
        assert_eq!(p1.soc_text().as_deref(), Some("62"), "SOC 整数位（PRD F1.1）");
        assert_eq!(p1.soc_color(), Palette::SOC_OK, "62 % 落在 15–85 ⇒ 青色");
        assert!(p1.soc_marker_visible() && !p1.soc_gray_visible(), "正常态有刻线、不灰化");
        assert_eq!(p1.soc_source_text().as_deref(), Some("BMS"), "F1.2 源标注");
        assert_eq!(p1.soc_source_skin(), Some(ChipSkin::NEUTRAL));
        assert_eq!(p1.pcs_state_text().as_deref(), Some("充电"), "F2 主判据 REG 1013");
        assert_eq!(p1.pcs_icon_text().as_deref(), Some("▼"));
        assert_eq!(p1.pcs_color(), Palette::OK, "充电 = 绿");
        assert!(!p1.inconsistent_visible(), "方向一致 ⇒ 无「方向不一致」角标");
        assert_eq!(p1.sigma_text().as_deref(), Some("ΣP +36.9 kW"), "佐证行 ΣP");
        assert_eq!(p1.phase_p_text(0).as_deref(), Some("12.5"), "F3 三相 P（1 位小数）");
        assert_eq!(p1.phase_i_text(0).as_deref(), Some("45.6"), "F4 三相 I");
        assert_eq!(p1.phase_arrow(0).as_deref(), Some("▼"), "方向取自 F2 状态机");
        assert_eq!(p1.phase_reason(0), None, "正常相无降级角标");
        assert_eq!(p1.phase_dot_color(0), Palette::SOC_OK, "● 实时（UI §8.2）");
        assert_eq!(p1.phase_p_text(3).as_deref(), Some("36.9"), "总卡 = 设备总有功 REG 1032");
        assert_eq!(p1.phase_i_text(3).as_deref(), Some("–"), "总卡无电流行 ⇒ 占位符");
        assert_eq!(p1.device_card_count(), 8, "装置状态网格 8 卡（UI §6.1）");
        assert_eq!(p1.device_text(0).as_deref(), Some("0.1.0"), "固件版本");
        assert_eq!(p1.device_text(1).as_deref(), Some(pages::MISSING), "编译时间缺失 ⇒ 未提供");
        assert_eq!(p1.device_text(2).as_deref(), Some("1 日 01:00:00"), "运行时长（`日` 在 cmap 内）");
        assert_eq!(p1.device_text(3).as_deref(), Some("48 C"), "CPU 温度（℃ 不在字符集 ⇒ C）");
        assert_eq!(p1.device_text(4).as_deref(), Some("31 %"), "内存使用率");
        assert_eq!(p1.device_text(5).as_deref(), Some("已连接"), "IEC 104 链路（灯 + 文字双通道）");
        assert_eq!(p1.device_text(6).as_deref(), Some("连接中"), "核间链路");
        assert_eq!(p1.device_text(7).as_deref(), Some("本地策略引擎"), "当前控制源");
        assert_eq!(p1.alarm_view(), p1_status::AlarmView::Rows);
        assert_eq!(p1.alarm_row_message(0).as_deref(), Some("核间链路抖动"));
        assert_eq!(
            p1.alarm_row_time(0).as_deref(),
            Some("2026/09/10 13:42:07"),
            "时间（UTC，/ 分隔；`-` 不在字体子集内）"
        );
        assert_eq!(p1.alarm_row_message(1).as_deref(), Some("直流侧过压"));
        assert_eq!(p1.alarm_row_message(2), None, "第 3 行无数据 ⇒ 隐藏");

        // ── ⑥ 卡内元素 `coords()` 断言（**B2a 代码质量评审「补断言」**）─────────────
        // 此前"320 重排后不重叠 / 不越界"是评审员**手算**得出的（`pages_chain` 只断 `soc_card`
        // 的 y / 高与 A 相卡 y1），卡内元素**零 `coords()` 断言** ⇒ 这里把"贴底排布"钉住。
        disp.refr_now_for_test();
        // ① SOC 量程条**贴底**：底缘不越卡内容区底缘，且与底缘的距离 ≤ 2×GAP_MIN（= 其下
        //    "缝 4 + 刻度行"的量级 ⇒ 是"贴底"而非"漂浮"）。
        let card_c = p1.soc_card().coords();
        let bar_c = p1.soc_bar_obj().coords();
        let inner_bottom = card_c.y2 - (Stroke::THIN + Dimens::GAP_MIN);
        assert!(
            bar_c.y2 <= inner_bottom,
            "SOC 量程条底缘 {} 不得越过卡内容区底缘 {inner_bottom}",
            bar_c.y2
        );
        assert!(
            inner_bottom - bar_c.y2 <= 2 * Dimens::GAP_MIN,
            "SOC 量程条应贴底（其下只剩 缝 + 刻度行），实测间隙 {}",
            inner_bottom - bar_c.y2
        );
        // ② 刻度行在量程条之下、**卡外缘之内**（M1：行高 = px + 2 ⇒ 可低出内容区底缘 2 px，
        //    但距卡外缘仍有余量）。
        let scale_c = p1.soc_scale_obj().coords();
        assert!(
            scale_c.y1 >= bar_c.y2 && scale_c.y2 < card_c.y2,
            "刻度行必须在量程条下方、卡外缘之内（bar.y2 {} / scale.y1..y2 {}..{} / card.y2 {}）",
            bar_c.y2,
            scale_c.y1,
            scale_c.y2,
            card_c.y2
        );
        // ③ **I1 的 `coords()` 版** —— 分两层：
        //    (a) **结构上敏感**的**槽宽**断言（B2a 代码质量评审 ①）：数值 label 的显式宽度
        //        必须**恰为** `PHASE_P_SLOT_W` ⇒ 去掉 `p_value.set_size(..)` 即变红。
        //        ⚠️ 为什么**必须**断宽度：无显式宽度时 LVGL label 按文本自增，而 `coords()`
        //        此时返回的是**陈旧的 obj 盒**（评审实测 32 px）而非绘出的文本外延 ⇒ 只断
        //        "数值右缘 < 单位左缘"**结构上不可能**抓到"文本越槽"（评审把 `set_size` +
        //        `LongMode::DOTS` 注释掉后，旧断言仍全绿）。`coords()` 是**闭区间**：
        //        `Area::width() = x2 - x1 + 1`。
        //    (b) **语义**断言（保留）：数值槽不压单位 / 箭头、不越相卡右缘。
        let mut fl = frame_healthy();
        fl.p_phase[0] = Field { v: Some(123.4), flag: FieldFlag::Valid };
        fl.p_phase[1] = Field { v: Some(-123.4), flag: FieldFlag::Valid };
        p1.render(&PageInput::live(&fl));
        disp.refr_now_for_test();
        let pc_c = p1.phase_card_obj(0).expect("A 相卡").coords();
        let pv_c = p1.phase_p_obj(0).expect("A 相 P 值").coords();
        let unit_c = p1.phase_unit_obj(0).expect("A 相 kW").coords();
        let arrow_c = p1.phase_arrow_obj(0).expect("A 相箭头").coords();
        assert_eq!(
            pv_c.width() as i32,
            p1_status::PHASE_P_SLOT_W,
            "P 数值槽必须有**显式宽度约束**（`set_size(PHASE_P_SLOT_W, ..)`）：实测宽 {} ≠ \
             槽宽 {} —— 去掉 `set_size` 会让数值按文本自增、压住 `kW` / 箭头",
            pv_c.width(),
            p1_status::PHASE_P_SLOT_W
        );
        let pi_c = p1.phase_i_obj(0).expect("A 相 I 值").coords();
        assert_eq!(
            pi_c.width() as i32,
            p1_status::PHASE_I_SLOT_W,
            "I 数值槽同样必须有显式宽度约束（同 I1）"
        );
        assert!(
            pv_c.x2 < unit_c.x1 && pv_c.x2 < arrow_c.x1,
            "超长数值必须**截断在槽内**，不得压住单位（x1 = {}）或箭头（x1 = {}）；实测数值右缘 {}",
            unit_c.x1,
            arrow_c.x1,
            pv_c.x2
        );
        assert!(
            pv_c.x2 <= pc_c.x2,
            "数值槽右缘 {} 不得越出相卡右缘 {}",
            pv_c.x2,
            pc_c.x2
        );
        assert_eq!(
            p1.phase_p_text(0).as_deref(),
            Some("123.4"),
            "`DOTS` 只影响**绘制**，文本属性仍完整（截断不是丢数据）"
        );
        // **C1 的页面级回归锁**：负值必须用 U+2212（cmap 内有字形），**不得**是 ASCII `-`。
        assert_eq!(
            p1.phase_p_text(1).as_deref(),
            Some("\u{2212}123.4"),
            "P 反向（负值）必须显 U+2212 —— 用 ASCII `-` 在真机上是豆腐块、负值看着像正值"
        );
        assert!(
            !p1.phase_p_text(1).unwrap().contains('-'),
            "页面输出里**不得**再出现 ASCII `-`（C1）"
        );
        drop(p1);

        // ═══ ③ 逐字段降级（**显占位符而不是 0**）══════════════════════════
        let p1 = p1_status::P1StatusPage::new(&host).expect("P1StatusPage::new (degraded)");
        let bad = frame_degraded();
        p1.render(&PageInput::live(&bad));
        assert_eq!(p1.phase_p_text(0).as_deref(), Some(pages::PLACEHOLDER), "A 相离线 ⇒ 占位符");
        assert_eq!(p1.phase_reason(0).as_deref(), Some("源离线"), "降级原因（EDGE-01）");
        assert_eq!(p1.phase_reason(1).as_deref(), Some("未取数"), "未取数（EDGE-04）");
        assert_eq!(p1.phase_reason(2).as_deref(), Some("数据异常"), "数据异常（EDGE-05）");
        assert_eq!(p1.phase_dot_color(0), Palette::BORDER_CTRL, "降级 ⇒ 停更点");
        assert_eq!(p1.phase_arrow(0), None, "无有效值 ⇒ 不画方向箭头");
        for i in 0..4 {
            let t = p1.phase_p_text(i).expect("有文本");
            assert_ne!(t, "0.0", "**严禁补 0**（PRD F3.4）");
            assert_ne!(t, "0", "**严禁补 0**（PRD F3.4）");
        }
        assert_eq!(p1.device_text(2).as_deref(), Some("未取数"), "uptime 不可得 ⇒ 未取数");
        assert_eq!(p1.device_text(0).as_deref(), Some(pages::MISSING), "空版本串 ⇒ 未提供");
        // ⚠️ P1 的装置网格取 UI §6.1 的 **8 项**（固件版本 / 编译时间 / 运行时长 / CPU 温度 /
        // 内存使用率 / 调度主站连接 / 核间连接 / 当前控制源）—— **不含「数据通道」**：
        // 通道状态归页眉（B2c/B3），P1 只在通道条上体现断连（设计 §6.1 的布局行）。
        // 「数据通道」一行在 P6 的运行信息卡（见下方 P6 用例）。
        assert_eq!(p1.device_text(5).as_deref(), Some("已连接"), "调度主站连接");
        assert_eq!(p1.device_text(7).as_deref(), Some("本地策略引擎"), "控制源不随其它字段降级");
        assert_eq!(p1.alarm_view(), p1_status::AlarmView::Unavailable);
        assert_eq!(
            p1.alarm_unavailable_title().as_deref(),
            Some("告警源不可用"),
            "EDGE-09：源不可用 ≠ 无告警"
        );
        assert_ne!(
            p1.alarm_unavailable_title().as_deref(),
            Some(p1_status::TEXT_ALARM_EMPTY)
        );
        drop(p1);

        // ═══ ④ SOC 三源标注 × 三档区间色 ═══════════════════════════════════
        let p1 = p1_status::P1StatusPage::new(&host).expect("P1StatusPage::new (soc)");
        let mut f2 = frame_healthy();
        f2.soc_source = SocSource::PcsReg1010;
        p1.render(&PageInput::live(&f2));
        assert_eq!(p1.soc_source_text().as_deref(), Some(p1_status::TEXT_SOC_SRC_PCS));
        f2.soc = None;
        f2.soc_source = SocSource::Lost;
        p1.render(&PageInput::live(&f2));
        assert_eq!(p1.soc_source_text().as_deref(), Some("SOC 源失效"), "EDGE-02 双源皆失");
        assert_eq!(p1.soc_source_skin(), Some(ChipSkin::FAILURE), "源失效 = 红胶囊");
        assert_eq!(p1.soc_text().as_deref(), Some(pages::PLACEHOLDER));
        assert_eq!(p1.soc_color(), Palette::PLACEHOLDER);
        assert!(p1.soc_gray_visible() && !p1.soc_marker_visible(), "量程条灰化");
        for (v, expect, what) in [
            (10.0, Palette::DANGER, "≤15 % ⇒ 红"),
            (50.0, Palette::SOC_OK, "中段 ⇒ 青"),
            (90.0, Palette::SOC_HIGH, "≥85 % ⇒ 橙"),
        ] {
            let mut fx = frame_healthy();
            fx.soc = Some(v);
            p1.render(&PageInput::live(&fx));
            assert_eq!(p1.soc_color(), expect, "{what}（PRD F1.3）");
        }
        drop(p1);

        // ═══ ⑤ PCS 四态：文字 + 语义色 + 图标 ═══════════════════════════════
        let p1 = p1_status::P1StatusPage::new(&host).expect("P1StatusPage::new (pcs)");
        for (rs, text, color, icon) in [
            (RunState::Stop, "停机", Palette::STOPPED, "■"),
            (RunState::Standby, "待机", Palette::STANDBY, "○"),
            (RunState::Charge, "充电", Palette::OK, "▼"),
            (RunState::Discharge, "放电", Palette::INFO, "▲"),
        ] {
            let mut fx = frame_healthy();
            fx.run_state = Some(rs);
            p1.render(&PageInput::live(&fx));
            assert_eq!(p1.pcs_state_text().as_deref(), Some(text), "F2.1 四态之一");
            assert_eq!(p1.pcs_color(), color, "F2.1 语义色（{text}）");
            assert_eq!(p1.pcs_icon_text().as_deref(), Some(icon), "F2.1 图标（{text}）");
            let want_arrow = match rs {
                RunState::Charge | RunState::Discharge => Some(icon),
                _ => None,
            };
            assert_eq!(p1.phase_arrow(0).as_deref(), want_arrow, "方向随 F2；停 / 待不画（{text}）");
        }
        // 离线（EDGE-01）与无帧
        let mut fx = frame_healthy();
        fx.pcs_online = false;
        fx.run_state = None;
        p1.render(&PageInput::live(&fx));
        assert_eq!(p1.pcs_state_text().as_deref(), Some(p1_status::TEXT_PCS_OFFLINE));
        assert_eq!(p1.pcs_color(), Palette::STOPPED, "离线 = 灰（不得用语义色冒充）");
        assert_eq!(p1.pcs_icon_text().as_deref(), Some("?"));
        // 方向不一致角标（EDGE-06；唯一专属色）
        let mut fx = frame_healthy();
        fx.inconsistency = true;
        p1.render(&PageInput::live(&fx));
        assert!(p1.inconsistent_visible(), "EDGE-06 角标");
        assert_eq!(p1.pcs_state_text().as_deref(), Some("充电"), "角标**不覆盖**主判据 1013");
        drop(p1);

        // ═══ ⑥ 通道断 / 过期（F5.3 / EDGE-03）══════════════════════════════
        let p1 = p1_status::P1StatusPage::new(&host).expect("P1StatusPage::new (channel)");
        let f = frame_healthy();
        p1.render(&PageInput::down(Some(&f)));
        assert_eq!(
            p1.channel_text().as_deref(),
            Some(p1_status::TEXT_CHANNEL_DOWN),
            "EDGE-03：>3 s 无成功 ⇒ 断连条"
        );
        p1.render(&PageInput::new(
            Some(&f),
            ChannelStatus::Connected,
            Freshness::Stale,
        ));
        assert!(p1.stale_visible(), "F5.3：>2 s ⇒ 「数据过期」标（保留数值）");
        assert_eq!(p1.soc_text().as_deref(), Some("62"), "过期仍保留最近有效值");
        assert_eq!(p1.phase_dot_color(0), Palette::STALE, "过期 ⇒ 琥珀点");
        // 帧加载点字段仍降级（点级独立降级 —— PRD F5.5）
        let mut fx = frame_healthy();
        fx.p_phase[1] = Field { v: None, flag: FieldFlag::NotRead };
        p1.render(&PageInput::live(&fx));
        assert_eq!(p1.phase_p_text(0).as_deref(), Some("12.5"), "B 相失败不影响 A 相");
        assert_eq!(p1.phase_reason(1).as_deref(), Some("未取数"));

        // ── ①【评审 ①】某相 **P 有效但 I 单独缺失**：状态点与降级角标必须反映
        //    「该相 P 与 I 的整体可用性」，不得仍显「实时」且无角标（PRD F5.5 各字段独立降级）──
        let mut fi = frame_healthy();
        fi.i_phase[0] = Field { v: None, flag: FieldFlag::Offline };
        p1.render(&PageInput::live(&fi));
        assert_eq!(p1.phase_p_text(0).as_deref(), Some("12.5"), "P 有效 ⇒ 照常显示数值");
        assert_eq!(
            p1.phase_i_text(0).as_deref(),
            Some(pages::PLACEHOLDER),
            "I 缺失 ⇒ 占位符（**严禁补 0**）"
        );
        assert_eq!(
            p1.phase_reason(0).as_deref(),
            Some("源离线"),
            "I 单独缺失也必须有降级角标（评审 ①：此前只看 P，角标不出现）"
        );
        assert_ne!(
            p1.phase_dot_color(0),
            Palette::SOC_OK,
            "「实时」点必须消失（评审 ①）"
        );
        assert_eq!(p1.phase_dot_color(0), Palette::BORDER_CTRL, "降级 ⇒ 停更点");
        // 同帧的 B / C 相与总卡不受影响（点级独立降级；总卡无电流行是**构造**，不是缺失）。
        assert_eq!(p1.phase_p_text(1).as_deref(), Some("12.1"), "B 相不受影响");
        assert_eq!(p1.phase_reason(1), None, "B 相仍实时");
        assert_eq!(p1.phase_dot_color(1), Palette::SOC_OK);
        assert_eq!(p1.phase_reason(3), None, "总卡不因「无电流行」被误判为降级");
        assert_eq!(p1.phase_dot_color(3), Palette::SOC_OK);
        drop(p1);
    }

    // ═══ P6 系统 / 关于页 ══════════════════════════════════════════════════
    {
        let p6 = p6_system::P6SystemPage::new(&host).expect("P6SystemPage::new");
        disp.refr_now_for_test();
        assert_eq!(p6.info_row_count(), 4, "装置信息 4 行（UI §6.6）");
        assert_eq!(p6.run_row_count(), 7, "运行信息 7 行（UI §6.6）");
        assert_eq!(p6.about_row_count(), 3, "关于本屏 3 行（含服务地址 / 管理 IP 两行）");

        let f = frame_healthy();
        p6.render(&PageInput::live(&f));
        assert_eq!(p6.info_label(0).as_deref(), Some(p6_system::TEXT_MODEL));
        // **D9**：契约串 `BECG-3568` 的 `-` 在 cmap 里没字形 ⇒ 上屏前经 `display_safe`
        // 改写为 U+2013（真机不出豆腐块）。帧内仍保留契约原值（`frame_healthy` 未改）。
        assert_eq!(
            p6.info_value(0).as_deref(),
            Some("BECG\u{2013}3568"),
            "装置型号（F8.1；`-` 经 display_safe 改写，见 D9）"
        );
        assert!(
            !p6.info_value(0).unwrap().contains('-'),
            "上屏文本里**不得**出现 cmap 外的 ASCII `-`（D9）"
        );
        assert_eq!(p6.info_value(1).as_deref(), Some(pages::MISSING), "序列号无可靠真源 ⇒ 未提供");
        assert_eq!(p6.info_value(2).as_deref(), Some("0.1.0"), "固件版本");
        assert_eq!(p6.info_value(3).as_deref(), Some(pages::MISSING), "编译时间缺失 ⇒ 未提供");
        assert_eq!(p6.run_value(0).as_deref(), Some("1 日 01:00:00"), "系统运行时长");
        assert_eq!(p6.run_value(1).as_deref(), Some("48 C"), "CPU 温度");
        assert_eq!(p6.run_value(2).as_deref(), Some("31 %"), "内存使用率");
        assert_eq!(p6.run_value(3).as_deref(), Some("已连接"), "调度主站连接（灯 + 文字）");
        assert_eq!(p6.run_value(4).as_deref(), Some("连接中"), "核间连接");
        assert_eq!(p6.run_value(6).as_deref(), Some("本地策略引擎"), "当前控制源");
        assert_eq!(
            p6.local_version().as_deref(),
            Some(env!("CARGO_PKG_VERSION")),
            "本地屏版本 = 本 crate 编译版本"
        );

        // ── ⑤ 服务地址口径：两行**分列**、后者缺失显「未提供」（PM 裁定 / EDGE-24）──
        assert_ne!(
            p6.service_label(),
            p6.mgmt_label(),
            "「本机服务地址」与「设备管理 IP」必须是两行、行名不同"
        );
        assert_eq!(p6.service_label().as_deref(), Some(p6_system::TEXT_SERVICE_ADDR));
        assert_eq!(p6.mgmt_label().as_deref(), Some(p6_system::TEXT_MGMT_IP));
        let svc = p6.service_address().expect("服务地址恒有值");
        assert!(svc.contains("127.0.0.1"), "服务地址 = 回环端点（实际 {svc}）");
        assert_ne!(svc, pages::MISSING, "服务地址不适用「未提供」");
        assert_eq!(p6.mgmt_ipv4().as_deref(), Some("192.168.3.118"), "设备管理 IP");
        assert!(
            !svc.contains("192.168.3.118"),
            "不得把管理 IP 与服务端口并列成「访问地址」（EDGE-24）"
        );
        assert!(p6_system::service_scope_text().contains("仅回环"), "口径来自契约枚举");
        assert_eq!(p6.note_text().as_deref(), Some(p6_system::TEXT_NO_REMOTE), "说明行");

        // ── ⑥ 区块级「冻结 / 数据过期」打标（EDGE-03 / EDGE-20；B3-2c）──────────────
        // 判据 = `pages::frame_mark`（与 P1 的通道条 / `数据过期` 角标**同源**）。
        // **改什么会让本条变红**：把 `P6SystemPage::render` 的打标段删掉 / 写成恒隐藏
        // ⇒ 下列可见性断言全红；把 `Down` 时的帧清成缺省（`frame = None`）⇒
        // 「数值仍为冻结帧」两条红（EDGE-03 明文要求**保留**最近有效帧，不得清成占位符）。
        assert!(
            !p6.info_frozen_visible() && !p6.run_frozen_visible(),
            "实时（通道通 + 帧新鲜）⇒ 无角标"
        );
        p6.render(&PageInput::down(Some(&f)));
        assert!(p6.info_frozen_visible(), "通道断 + 保留帧 ⇒ 装置信息卡打「冻结」");
        assert!(p6.run_frozen_visible(), "运行信息卡同理（同一拍、同一判据）");
        assert_eq!(
            p6.info_frozen_text().as_deref(),
            Some(pages::TEXT_FROZEN),
            "角标文案 = §3.6 既有串 `冻结`（EDGE-03 原文用字，零新上屏字）"
        );
        assert_eq!(p6.run_frozen_text().as_deref(), Some(pages::TEXT_FROZEN));
        // **打标 ≠ 清值**：冻结帧的数值必须仍在屏上（这两条就是 EDGE-03 的"保留"要求）。
        assert_eq!(
            p6.info_value(0).as_deref(),
            Some("BECG\u{2013}3568"),
            "通道断 ⇒ 装置信息**沿用冻结帧**（不得清成「未提供」）"
        );
        assert_eq!(p6.run_value(1).as_deref(), Some("48 C"), "运行信息同样沿用冻结帧");
        // 通道恢复 ⇒ 标记**当拍撤除**。
        p6.render(&PageInput::live(&f));
        assert!(
            !p6.info_frozen_visible() && !p6.run_frozen_visible(),
            "通道恢复 ⇒ 角标当拍撤除"
        );
        // 通道正常但帧旧 ⇒ **同一件**角标改文案（同一位置、同一对象，不新建）。
        p6.render(&PageInput::new(
            Some(&f),
            ChannelStatus::Connected,
            Freshness::Stale,
        ));
        assert!(p6.info_frozen_visible(), "帧过期 ⇒ 仍要打标（§8.2 的可信度是逐区块属性）");
        assert_eq!(
            p6.info_frozen_text().as_deref(),
            Some(p1_status::TEXT_STALE),
            "通道通 + 帧旧 ⇒ 文案改「数据过期」（与 P1 同一串）"
        );
        // 从未收到帧 ⇒ **不打标**（此时数值本就是 `–` 占位，不是"冻结的旧值"）。
        p6.render(&PageInput::init());
        assert!(
            !p6.info_frozen_visible() && !p6.run_frozen_visible(),
            "无帧 ⇒ 无角标（打「冻结」会谎报「有冻结帧」）"
        );

        // ── ⑦ 角标的**构造侧读回**（B3-2c 整改 重要 4；与 P4 的同款断言成对）──────────
        // 判据 = [`crate::ui::pages::frozen_chip`]（**四张卡的唯一构造点**）：尺寸与皮肤
        // **只由那里给定**，`render` 不覆盖（它只切可见性 + 换文案）⇒ 改那个函数的返回，
        // **两页的断言同时红**（这正是"同一个构造点"的行为判据；P4 侧见其块内 ⑰）。
        // **改什么会让本条变红**：改 `pages::frozen_chip` 的宽度 / 皮肤 ⇒ 本页两条红；
        // 改 P6 内联构造（绕开共享点）⇒ 本页仍绿、但 P4 那两条会与它**分叉**。
        disp.refr_now_for_test();
        for (n, chip) in [("装置信息", p6.info_frozen_chip()), ("运行信息", p6.run_frozen_chip())]
        {
            assert_eq!(
                chip.size().0,
                pages::FROZEN_CHIP_W,
                "{n}卡角标宽 = `pages::FROZEN_CHIP_W`（构造点唯一，不得各页自定）"
            );
            assert_eq!(
                chip.skin(),
                crate::ui::theme::ChipSkin::WARNING,
                "{n}卡角标皮肤 = §8.2 警示类（`数据过期` / `冻结` 同款）"
            );
        }

        // ── ⑧ 渲染热路径的**双零增长**（B3-2c 整改 重要 2）───────────────────────
        // 评审实测：在 `P6SystemPage::render` 里插一次 `add_style` ⇒ 整改前 **354 全绿**
        // ⇒「每拍只切可见性」此前**只有源码阅读作证**。本段连推 50 拍 `render(..)`，
        // **三态轮转**（断 ⇒ 打「冻结」/ 通+帧旧 ⇒ 改「数据过期」/ 通+帧新 ⇒ 撤标）——
        // 三个分支都走，「误挂样式」最可能出在"改文案"那两支里。
        //
        // **改什么会让本条变红**（**已实测**）：在 `P6SystemPage::render` 里插一次
        // `add_style` ⇒ 第 2 条红；插一次 `Obj::create` ⇒ 第 1 条红。
        //
        // ⚠️ **测量区间前有一轮"热身"**（如实说明，避免把"有界的样式切换"误判成缺陷）：
        // 上一段把 `p6` 推到了 `init` 态（各值 = `–` 占位）⇒ **占位样式 ⇒ 实际值样式**那一档
        // 切换（`set_style_index` = remove + add 的**配对**操作）会挂上几条样式 —— 实测
        // **恰好 6 条**（= 6 个值行）。那是**由内容变化驱动、有界**的，不是"每拍挂一条"；
        // 本段要钉的是**稳态**：**同一帧**连推 N 拍，样式挂载数**一个字都不许涨**。
        {
            use std::sync::atomic::Ordering;
            for _ in 0..3 {
                p6.render(&PageInput::down(Some(&f)));
                p6.render(&PageInput::new(Some(&f), ChannelStatus::Connected, Freshness::Stale));
                p6.render(&PageInput::live(&f));
            }
            let mounts_before = crate::lvgl::obj::PROBE_MOUNTS.load(Ordering::SeqCst);
            let attaches_before = crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(Ordering::SeqCst);
            for i in 0..50u64 {
                let input = match i % 3 {
                    0 => PageInput::down(Some(&f)),
                    1 => PageInput::new(Some(&f), ChannelStatus::Connected, Freshness::Stale),
                    _ => PageInput::live(&f),
                };
                p6.render(&input);
                // ⚠️ **逐拍复核，不等推完 50 拍**（理由，实测）：LVGL 对**单个对象的样式条数**
                // 有硬上限 —— `lv_obj_style.c:110` 断言 `style_cnt < 63`（该断言在本仓配置下
                // **打印后继续**，随后 `lv_realloc` 把样式数组推到上限之外 ⇒ 进程**挂在循环中途**，
                // 连断言都跑不到 —— 评审原味探针实测就是挂死，不是红）。逐拍断 ⇒ **第一次越界
                // 的那一拍立刻红**，回归输出干净可读。
                assert_eq!(
                    crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(Ordering::SeqCst) - attaches_before,
                    0,
                    "P6 `render` 第 {i} 拍往对象上挂样式了（`Obj::add_style` 只增不删）"
                );
            }
            assert_eq!(
                crate::lvgl::obj::PROBE_MOUNTS.load(Ordering::SeqCst) - mounts_before,
                0,
                "P6 连推 50 拍 `render`（断 / 帧旧 / 实时三态轮转）不得新建任何 LVGL 对象 \
                 —— 角标在构造期建好，`render` 是 1 Hz 热路径"
            );
            assert_eq!(
                crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(Ordering::SeqCst) - attaches_before,
                0,
                "P6 连推 50 拍 `render` **也不得往对象上挂任何样式**：`Obj::add_style` 只增不删 \
                 ⇒「渲染路径每拍挂一条样式」= 样式表无界增长，而**对象数不变** —— \
                 评审的破坏性探针（在 `render` 里插 `add_style`）正是靠本条才抓得住"
            );
        }
        drop(p6);

        // 缺失帧 ⇒ 管理 IP「未提供」，而**服务地址仍有值**（两行不联动 —— 分列的实质）
        let p6 = p6_system::P6SystemPage::new(&host).expect("P6SystemPage::new (degraded)");
        let bad = frame_degraded();
        p6.render(&PageInput::live(&bad));
        assert_eq!(p6.mgmt_ipv4().as_deref(), Some(pages::MISSING), "EDGE-16");
        assert!(
            p6.service_address().is_some_and(|s| s.contains("127.0.0.1")),
            "服务地址仍如实显示（恒有值）"
        );
        assert_eq!(p6.run_value(5).as_deref(), Some("断开"), "数据通道");
        drop(p6);
    }

    // ═══ P2 配置页（B2b-2：控制通道驱动 + 写操作 + 固定操作条）═══════════════════
    //
    // 本段的每条断言都标了「改什么会让本条变红」——**均为实测**（见 B2b-2 报告的三条探针）。
    {
        use crate::lvgl::event::EventCode;
        use crate::ui::pages::p2_config::{self, P2ConfigPage};
        use mupc_display_proto::{
            ConfigField, ConfigGroup, ConfigKind, ConfigPatch, ConfigView, ControlCode,
            ControlResponse, FieldError, OptionItem, PatchSource, WriteMode,
        };
        use serde_json::Value;

        // 合成字段（默认全部可编辑 / 不瞬断，用例只覆写关心的那几项）。
        #[allow(clippy::too_many_arguments)]
        fn fld(
            key: &str,
            label: &str,
            kind: ConfigKind,
            value: Value,
            unit: Option<&str>,
            reconnect: bool,
            editable: bool,
        ) -> ConfigField {
            ConfigField {
                key: key.into(),
                label: label.into(),
                kind,
                default: value.clone(),
                value,
                unit: unit.map(str::to_string),
                requires_reconnect: reconnect,
                editable,
            }
        }

        /// 合成视图：**3 组**，含 `Ipv4` + `U16` + `U64` + `Enum` + 一个 `editable=false` 字段。
        /// `reconnect` = 是否把「监听地址」标成瞬断字段（决定 L1 / L2+）。
        fn p2_view(reconnect: bool) -> ConfigView {
            let u16k = ConfigKind::U16 {
                min: 1,
                max: 65535,
                step: 1,
            };
            // 「遥测上报周期」的**契约默认值（60）与当前值（1）不同** —— 这样
            // `defaults_patch()` 的"取 default 而不是当前值"才有可断言的差别。
            let mut period = fld(
                "telemetry.period",
                "遥测上报周期",
                ConfigKind::U64 {
                    min: 1,
                    max: 300,
                    step: 1,
                },
                Value::from(1u64),
                Some("秒"),
                false,
                true,
            );
            period.default = Value::from(60u64);
            ConfigView {
                groups: vec![
                    ConfigGroup {
                        id: "iec104".into(),
                        label: "IEC 104 连接参数".into(),
                        fields: vec![
                            // 注入的 label 是**旧口径**「对端 IP 地址」——页须按 PM 裁定改写。
                            // 敏感性：把 `field_label_text` 的覆盖分支去掉（直接返回
                            // `field.label`）⇒ 下面 ③ 的标签断言会读到「对端 IP 地址」而变红。
                            fld(
                                "gateway.listen_addr",
                                "对端 IP 地址",
                                ConfigKind::Ipv4,
                                Value::from("127.0.0.1"),
                                None,
                                reconnect,
                                true,
                            ),
                            fld(
                                "gateway.port",
                                "端口",
                                u16k.clone(),
                                Value::from(2404),
                                None,
                                false,
                                true,
                            ),
                        ],
                    },
                    ConfigGroup {
                        id: "intercore".into(),
                        label: "核间通信参数".into(),
                        fields: vec![fld(
                            "intercore.port",
                            "本地端口",
                            u16k.clone(),
                            Value::from(2500),
                            None,
                            false,
                            true,
                        )],
                    },
                    ConfigGroup {
                        id: "telemetry".into(),
                        label: "遥测与日志".into(),
                        fields: vec![
                            period,
                            fld(
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
                                None,
                                false,
                                true,
                            ),
                            // 只读字段（PL-4 回环红线）：控件 disabled + 说明行。
                            fld(
                                "display.bind_addr",
                                "本机服务地址（仅回环）",
                                ConfigKind::Ipv4,
                                Value::from("127.0.0.1"),
                                None,
                                false,
                                false,
                            ),
                        ],
                    },
                ],
                revision: 7,
                write_mode: WriteMode::TextPreserve,
            }
        }

        // ── ① 骨架 + 固定操作条（UI §6.2 线框 `Y624`）────────────────────────
        let p2 = P2ConfigPage::new(&host).expect("P2ConfigPage::new");
        disp.refr_now_for_test();
        assert_eq!(
            p2.obj().size(),
            (Dimens::CONTENT_W, Dimens::CONTENT_H),
            "页根 = 内容区整幅 992×624（**不是**滚动容器 —— 见 pages/mod.rs 契约 1′）"
        );
        assert_eq!(
            p2.scroll_obj().size(),
            (Dimens::CONTENT_W, 552),
            "滚动视口 = 624 − 操作条 72 = **552**（UI §6.2 线框「552 px 视口」）"
        );
        let root_c = p2.obj().coords();
        let bar_c = p2.action_bar_obj().coords();
        assert_eq!(
            (bar_c.x1, bar_c.y1),
            (root_c.x1, root_c.y1 + 552),
            "固定操作条贴在页根底缘（不随滚动）"
        );
        assert_eq!(
            p2.action_bar_obj().size().1,
            72,
            "操作条高 72（UI §6.2 线框 y624–696）"
        );

        // ── ② 未注入配置 ⇒ **不可用**（不是"空"）────────────────────────────
        assert_eq!(p2.group_count(), 0, "无配置 ⇒ 无分组卡");
        assert!(!p2.is_available());
        assert!(p2.fail_visible() && !p2.note_visible(), "降级 ⇒ 原因行替换说明行");
        assert_eq!(
            p2.fail_text().as_deref(),
            Some(p2_config::TEXT_CONFIG_UNAVAILABLE),
            "降级标题（**不得**写成「无配置」）"
        );
        assert!(p2.save_disabled() && p2.reset_disabled(), "不可用 ⇒ 两个按钮皆禁用");

        // ── ③ 注入视图 ⇒ 分组 / 字段 / 值文本 ────────────────────────────────
        let v = p2_view(false);
        p2.set_config(&v).expect("set_config");
        assert!(p2.is_available() && !p2.fail_visible());
        assert!(p2.note_visible(), "正常态显说明行（与失败行同槽互斥）");
        assert_eq!(p2.note_text().as_deref(), Some(p2_config::TEXT_PAGE_NOTE));
        assert_eq!(p2.group_count(), 3, "3 个分组卡");
        assert_eq!(p2.group_label(0).as_deref(), Some("IEC 104 连接参数"));
        assert_eq!(p2.group_label(2).as_deref(), Some("遥测与日志"));
        assert_eq!((p2.field_count(0), p2.field_count(1), p2.field_count(2)), (2, 1, 3));
        // 字段标签：PM 裁定键**改写**；其余键**透传**契约标签（元数据驱动）。
        assert_eq!(
            p2.field_label(0, 0).as_deref(),
            Some(p2_config::TEXT_LISTEN_ADDR),
            "gateway.listen_addr 必须是「本机监听地址 · IEC 104」（**不是**对端 IP）"
        );
        assert_ne!(
            p2.field_label(0, 0).as_deref(),
            Some("对端 IP 地址"),
            "注入的旧口径标签必须被 page 口径覆盖"
        );
        assert_eq!(p2.field_label(0, 1).as_deref(), Some("端口"));
        // 值文本（三类控件的读回口径）。
        assert_eq!(
            p2.field_value_text("gateway.listen_addr").as_deref(),
            Some("127.0.0.1"),
            "Ipv4Stepper 汇总标签"
        );
        assert_eq!(p2.field_value_text("gateway.port").as_deref(), Some("2404"));
        assert_eq!(
            p2.field_value_text("system.log_level").as_deref(),
            Some("INFO"),
            "Enum 段控件显**选项标签**（不是机器值 info）"
        );
        assert_eq!(
            p2.field_status_text("gateway.port").as_deref(),
            Some("1 – 65535"),
            "行型 A 的约束提示（UI §6.2）"
        );
        assert_eq!(
            p2.field_status_text("telemetry.period").as_deref(),
            Some("1 – 300 秒"),
            "带单位的约束提示"
        );

        // ── ④ 只读字段：控件 disabled **且**说明行在（设计 §6.2 只读字段行）────
        assert_eq!(p2.field_disabled("display.bind_addr"), Some(true), "只读 ⇒ 控件禁用");
        assert_eq!(p2.field_disabled("gateway.port"), Some(false), "可编辑 ⇒ 不禁用");
        assert_eq!(
            p2.field_note_text("display.bind_addr").as_deref(),
            Some(p2_config::TEXT_READONLY_NOTE),
            "只读字段必须带说明行（**可见性换现场可核查性**）"
        );
        assert_eq!(p2.field_note_visible("display.bind_addr"), Some(true));
        assert_eq!(p2.field_note_text("gateway.port"), None, "可编辑字段无说明行");
        assert_eq!(
            p2.field_label(2, 2).as_deref(),
            Some(p2_config::TEXT_LOOPBACK_ADDR),
            "回环绑定字段用回环标签（与 IEC 104 监听地址**分列**）"
        );

        // ── ⑤ 草稿 / 脏标记 / 放弃修改 ──────────────────────────────────────
        assert!(!p2.is_dirty(), "刚注入 ⇒ 不脏");
        assert!(p2.save_disabled(), "无改动 ⇒ 保存置灰");
        assert!(p2.set_field_value("gateway.port", &Value::from(2405)));
        assert!(p2.is_dirty(), "改了一个字段 ⇒ 脏");
        assert_eq!(p2.field_value_text("gateway.port").as_deref(), Some("2405"));
        let d = p2.draft();
        assert_eq!(d.from, PatchSource::Edit);
        assert_eq!(d.changes.len(), 1, "草稿只含被改的字段");
        assert_eq!(d.changes.get("gateway.port"), Some(&Value::from(2405)));
        assert!(!p2.save_disabled(), "有改动 ⇒ 保存可用");
        p2.discard_draft();
        assert!(!p2.is_dirty(), "放弃修改 ⇒ 不脏");
        assert_eq!(
            p2.field_value_text("gateway.port").as_deref(),
            Some("2404"),
            "放弃修改 ⇒ 值回退到注入值"
        );
        let dp = p2.defaults_patch();
        assert_eq!(dp.from, PatchSource::ResetDefault);
        assert!(dp.changes.contains_key("gateway.port"));
        assert!(
            !dp.changes.contains_key("display.bind_addr"),
            "只读字段不得进恢复默认值补丁（后端二次校验会拒绝整单）"
        );
        assert_eq!(
            p2.defaults_patch().changes.get("telemetry.period"),
            Some(&Value::from(60u64)),
            "取的是契约 default（60）而不是当前值（1）"
        );

        // ── ⑤′ 恢复默认值的**最低分级**：无瞬断字段 ⇒ **恰好 L2**（不是 L1、也不升 L2+）──
        // 敏感性：把 `reset_level` 的 `else` 分支改成 `L1` ⇒ 本条立刻变红；
        // 把键集合判定（`reconnect_in`）换成"视图口径" ⇒ 本视图无瞬断字段，不受影响；
        // 真正锁住口径的是 ⑧(a) 与 `p2_config.rs::save_level_scopes_to_changed_keys_not_view`。
        p2.reset_button()
            .button()
            .obj()
            .send_event(EventCode::CLICKED);
        assert_eq!(
            p2.with_dialog(|d| d.level()),
            Some(crate::ui::theme::ConfirmLevel::L2),
            "恢复默认值**必须走 L2**（生效性写，不得只靠间距保护）"
        );
        assert!(
            p2.with_dialog(|d| !d.has_warn_banner()).unwrap_or(false),
            "无瞬断字段 ⇒ L2 不强制 WarnBanner"
        );
        // 关掉它（模拟"新视图到达"这一拍），让下一拍的「保存」能开新弹层。
        p2.set_config(&v).expect("set_config（关掉上一弹层）");

        // ── ⑥ 保存：**未确认不发任何意图**（设计 §6.2 末行）───────────────────
        // 「改什么会让本条变红」：把 `save.on_clicked` 的回调从 `open_dialog(..)` 改成直接
        // `fire_submit(..)`（绕开弹层）⇒ 「未确认时 on_submit 未被调用」那条立刻变红
        // （探针 P2 实测）。
        let got: Rc<RefCell<Vec<(ConfigPatch, crate::ui::theme::ConfirmLevel)>>> =
            Rc::new(RefCell::new(Vec::new()));
        {
            let got = Rc::clone(&got);
            p2.set_on_submit(move |patch, level| got.borrow_mut().push((patch, level)));
        }
        assert!(p2.set_field_value("gateway.port", &Value::from(2405)));
        // 点「保存」= 向保存按钮派发 CLICKED（与真实 indev 同一条派发路径）。
        p2.save_button()
            .button()
            .obj()
            .send_event(EventCode::CLICKED);
        assert!(
            got.borrow().is_empty(),
            "**未确认 ⇒ 不发出任何提交意图**（未确认 = 无网络动作）"
        );
        assert_eq!(
            p2.with_dialog(|d| d.level()),
            Some(crate::ui::theme::ConfirmLevel::L1),
            "无 requires_reconnect ⇒ L1"
        );
        assert_eq!(
            p2.with_dialog(|d| d.title().text()).flatten().as_deref(),
            Some(p2_config::TEXT_DIALOG_TITLE_SAVE)
        );
        assert!(
            p2.with_dialog(|d| d.default_focus_is_cancel()).unwrap_or(false),
            "TT-09：默认焦点「取消」"
        );
        assert!(
            p2.with_dialog(|d| !d.has_warn_banner() && !d.has_progress()).unwrap_or(false),
            "L1：无 WarnBanner、无长按进度"
        );
        // L1 = 单击生效。
        p2.with_dialog(|d| {
            d.confirm_button()
                .button()
                .obj()
                .send_event(EventCode::CLICKED)
        });
        assert_eq!(got.borrow().len(), 1, "确认完成 ⇒ 恰好发一次意图");
        {
            let g = got.borrow();
            assert_eq!(g[0].1, crate::ui::theme::ConfirmLevel::L1);
            assert_eq!(g[0].0.from, PatchSource::Edit);
            assert_eq!(g[0].0.changes.len(), 1);
            assert_eq!(g[0].0.changes.get("gateway.port"), Some(&Value::from(2405)));
        }

        // ── ⑦ 取消：**延迟关闭**（不得在 LVGL 事件回调内删弹层）───────────────
        p2.with_dialog(|d| {
            d.cancel_button()
                .button()
                .obj()
                .send_event(EventCode::CLICKED)
        });
        assert!(
            p2.with_dialog(|d| d.is_alive()).unwrap_or(false),
            "取消回调内不得删弹层（`ConfirmDialog::close` 的要求）"
        );
        p2.tick(std::time::Instant::now());
        assert!(
            p2.with_dialog(|d| d.is_alive()).is_none(),
            "tick 里执行延迟关闭"
        );

        // ── ⑧ 分级与「涉及：」按**本次改动**判定（PD11）+ 瞬断字段 ⇒ L2+ / WarnBanner（UI §2.5）
        //
        // 敏感性（**探针 ① 的页级姊妹网**）：把 `save_level` / `reconnect_field_labels` 改回
        // "视图口径"（`has_reconnect_field`）⇒ 下面 (a) 的 **L1 / 无 WarnBanner** 两条立刻变红。
        let vr = p2_view(true); // `gateway.listen_addr` = 瞬断字段（视图里**存在**它）
        p2.set_config(&vr).expect("set_config (reconnect)");

        // (a) 本次只改**非**瞬断字段（端口）⇒ **L1**：视图里有瞬断字段也不得升级/弹警示
        //     —— 否则只改一个端口却弹「生效瞬间通信将短暂中断」= 谎报副作用（§2.6）。
        assert!(p2.set_field_value("gateway.port", &Value::from(2406)));
        p2.save_button()
            .button()
            .obj()
            .send_event(EventCode::CLICKED);
        assert_eq!(
            p2.with_dialog(|d| d.level()),
            Some(crate::ui::theme::ConfirmLevel::L1),
            "视图含瞬断字段、但**本次没改它** ⇒ L1（PD11）"
        );
        assert!(
            p2.with_dialog(|d| !d.has_warn_banner()).unwrap_or(false),
            "未触及瞬断字段 ⇒ 不得出现 WarnBanner（不得谎报副作用）"
        );
        assert!(
            p2.with_dialog(|d| !d.has_progress()).unwrap_or(false),
            "L1：无长按进度条"
        );
        // 关掉它，让下一拍的「保存」能开新弹层。
        p2.set_config(&vr).expect("set_config（关掉上一弹层）");

        // (b) 本次改的**就是**瞬断字段（监听地址）⇒ L2+ + WarnBanner + 「涉及：」
        assert!(p2.set_field_value("gateway.listen_addr", &Value::from("10.0.0.1")));
        p2.save_button()
            .button()
            .obj()
            .send_event(EventCode::CLICKED);
        assert_eq!(
            p2.with_dialog(|d| d.level()),
            Some(crate::ui::theme::ConfirmLevel::L2Plus),
            "本次改动**含** requires_reconnect 字段 ⇒ **L2+**"
        );
        assert!(
            p2.with_dialog(|d| d.has_warn_banner()).unwrap_or(false),
            "L2+：WarnBanner 强制出现"
        );
        assert!(
            p2.with_dialog(|d| d
                .warn_banner()
                .map(|w| w.has_fields_line())
                .unwrap_or(false))
                .unwrap_or(false),
            "WarnBanner 第二行「涉及：<字段名列表>」（= `reconnect_field_labels` 的输出）"
        );
        assert!(
            p2.with_dialog(|d| d.has_progress()).unwrap_or(false),
            "L2+：长按进度条就位（1.0 s 保持）"
        );
        let n_before = got.borrow().len();
        p2.with_dialog(|d| {
            d.confirm_button()
                .button()
                .obj()
                .send_event(EventCode::LONG_PRESSED)
        });
        assert_eq!(
            got.borrow().len(),
            n_before + 1,
            "L2+：长按满 1.0 s ⇒ 恰好发一次意图"
        );
        {
            let g = got.borrow();
            let last = g.last().expect("末条");
            assert_eq!(last.1, crate::ui::theme::ConfirmLevel::L2Plus);
            assert_eq!(
                last.0.changes.get("gateway.listen_addr"),
                Some(&Value::from("10.0.0.1"))
            );
        }

        // ── ⑨ 恢复默认值：**独立危险按钮** + L2 + 间距 ≥ 48 px ────────────────
        //
        // 前置：上一拍的弹层**仍在**（设计如此 —— 确认后弹层保持到回执到达，`show_result`
        // 才关它；这样"提交中"期间弹层不会被误关）。这里用一次视图注入把它关掉，模拟
        // "回执已到 / 视图已刷新"这一拍 —— 于是下一次点击才会**开新弹层**。
        p2.set_config(&vr)
            .expect("set_config（重置前一拍：关掉上一弹层）");
        assert!(
            p2.with_dialog(|d| d.is_alive()).is_none(),
            "注入新视图 ⇒ 关闭旧弹层"
        );
        assert!(p2.set_field_value("gateway.port", &Value::from(2406)));
        disp.refr_now_for_test();
        let save_c = p2.save_button().button().obj().coords();
        let reset_c = p2.reset_button().button().obj().coords();
        // 间距按**闭区间**口径算：`coords()` 的 `x2` 是最后一个像素列 ⇒ 真实缝隙 = x1 − x2 − 1。
        assert!(
            save_c.x1 - reset_c.x2 > Dimens::GAP_DANGER,
            "「恢复默认值」与「保存」的缝隙必须 ≥ 48 px（实测 {} px；UI §6.2 流程 8 / CF-07）",
            save_c.x1 - reset_c.x2 - 1
        );
        assert!(
            reset_c.x1 < save_c.x1,
            "「恢复默认值」在左、「保存」在右（UI §6.2 线框）"
        );
        p2.reset_button()
            .button()
            .obj()
            .send_event(EventCode::CLICKED);
        assert_eq!(
            p2.with_dialog(|d| d.level()),
            Some(crate::ui::theme::ConfirmLevel::L2Plus),
            "恢复默认值**必须走 L2 起**（涉及瞬断字段 ⇒ L2+）—— 不得只有间距保护"
        );
        let n_before = got.borrow().len();
        p2.with_dialog(|d| {
            d.confirm_button()
                .button()
                .obj()
                .send_event(EventCode::LONG_PRESSED)
        });
        assert_eq!(got.borrow().len(), n_before + 1);
        {
            let g = got.borrow();
            let last = g.last().expect("末条");
            assert_eq!(
                last.0.from,
                PatchSource::ResetDefault,
                "恢复默认值的补丁来源 = ResetDefault"
            );
            assert!(
                last.0.changes.len() >= 4,
                "恢复默认值覆盖全部可编辑字段（实测 {}）",
                last.0.changes.len()
            );
            assert!(!last.0.changes.contains_key("display.bind_addr"));
        }

        // ── ⑩ 提交中（F9.6）──────────────────────────────────────────────────
        p2.set_submitting(true);
        assert_eq!(
            p2.save_text().as_deref(),
            Some(p2_config::TEXT_SAVING),
            "保存中文案"
        );
        assert!(p2.save_disabled() && p2.reset_disabled(), "提交中禁重复触发");
        assert!(p2.is_submitting(), "提交态读回口径（F9.6）");
        p2.set_submitting(false);

        // ── ⑪ 失败回执：**保留已输入值** + 逐字段标红 + 具体原因 + 保存置灰 ────
        assert!(p2.set_field_value("gateway.port", &Value::from(2500)));
        let fail = ControlResponse::rejected(
            "rid-1",
            ControlCode::RejectedValidation,
            "字段 `gateway.port` 越界，允许区间 [1, 65535]",
            vec![FieldError {
                field: "gateway.port".into(),
                reason: "越界，允许区间 [1, 65535]".into(),
            }],
            Some("audit-1".into()),
            1_000,
        );
        p2.show_result(&fail).expect("show_result (fail)");
        assert_eq!(
            p2.field_error_visible("gateway.port"),
            Some(true),
            "该字段行须显形错误通道（红字 + 红竖条）"
        );
        assert!(
            p2.field_status_text("gateway.port")
                .unwrap_or_default()
                .contains("越界"),
            "**具体**原因必须上屏（CF-02：不得泛化提示）"
        );
        assert_eq!(
            p2.field_value_text("gateway.port").as_deref(),
            Some("2500"),
            "EDGE-10：失败必须保留用户已输入值"
        );
        assert!(p2.save_disabled(), "有字段错误 ⇒ 保存置灰");
        assert!(p2.fail_visible(), "失败原因就地显示（页顶提示行）");
        assert_eq!(
            p2.toast_tone(),
            Some(components::ToastTone::Failure),
            "失败 Toast（红条）"
        );
        // 审计不可写（EDGE-18）：固定文案 + fail-closed。
        let audit = ControlResponse::audit_unavailable("rid-2", 1_001);
        p2.show_result(&audit).expect("show_result (audit)");
        assert_eq!(
            p2.toast_text().as_deref(),
            Some(p2_config::TEXT_AUDIT_UNAVAILABLE),
            "EDGE-18：审计不可写 ⇒ **Toast 必须显「操作未执行」**（fail-closed；`code` 一改即红）"
        );
        assert_eq!(
            p2.fail_text().as_deref(),
            Some(p2_config::TEXT_AUDIT_UNAVAILABLE),
            "页顶原因行与 Toast 同口径"
        );

        // ── ⑫ 成功回执：用 `applied` 立即刷新（不等下一帧）+ Toast ──────────────
        //
        // **PD24（CF-04 降级，PM 裁定 2026-09-16）**：成功 Toast 由固定 `TEXT_TOAST_OK`
        // 改为**并入回执 `message`** —— 后端 G-2 在这里逐字点名"哪个键需重启"。
        // **敏感性（破坏性探针）**：把 `success_toast_text` 改回 `TEXT_TOAST_OK.to_string()`
        // （= 修复前形态）⇒ 下面"并入 message / 需重启 / 点名该字段"三条**立即变红**。
        //
        // ⚠️ **样例串随 PD25 更新**：后端回执文案已改为**只用字体 cmap 内的字符**
        // （修复前 `（项）；mupcd` 与机器键名都含缺字形字符 ⇒ 真机豆腐块），点名对象由
        // **机器键名**改为**字段 `label`**（`intercore.port` → `对端端口`）。
        // 下面两条串**逐字取自后端真实拼装**（`mupc-core-bin/src/config_service.rs`
        // 步骤 8：`receipt::SAVED` + `· ` + `receipt::RESTART_PREFIX` + `restart_labels`）。
        let mut applied = p2_view(false);
        applied.revision = 8;
        let mut ok_resp = ControlResponse::ok(
            "rid-3",
            Some(applied.clone()),
            Some("audit-2".into()),
            1_002,
        );
        ok_resp.message = "配置已保存 · 需重启进程生效: 对端端口 · 心跳间隔".into();
        p2.show_result(&ok_resp).expect("show_result (ok)");
        assert_eq!(
            p2.field_value_text("gateway.port").as_deref(),
            Some("2404"),
            "成功 ⇒ 用回执 `applied` 刷新本地值"
        );
        assert!(!p2.is_dirty(), "刷新后回到不脏");
        assert_eq!(p2.toast_tone(), Some(components::ToastTone::Success));
        assert_eq!(
            p2.toast_text().as_deref(),
            Some(ok_resp.message.as_str()),
            "成功 Toast **并入**回执 `message`（不再只用固定 [`TEXT_TOAST_OK`]）"
        );
        assert!(
            p2.toast_text().as_deref().unwrap().contains("需重启"),
            "需重启的键 ⇒ 屏上必须明说（固定串「已生效」对 6/7 个字段是反向陈述）"
        );
        // 逐字段点名（后端点的是 label；`intercore.port` → `对端端口`）。
        for label in ["对端端口", "心跳间隔"] {
            assert!(
                p2.toast_text().as_deref().unwrap().contains(label),
                "必须**点名**需重启的字段 `{label}`（G-2 的如实结论不得被渲染层吞掉）：{:?}",
                p2.toast_text()
            );
        }
        // **反面对照**：机器键名**不得**出现在屏上（后端已保证不点名它们；渲染层若哪天
        // 自己去拼键名，这条会红 —— 键名含缺字形的 `t` / `_`，真机即豆腐块）。
        assert!(
            !p2.toast_text().as_deref().unwrap().contains("intercore.port"),
            "机器键名不得上屏（缺字形 ⇒ 豆腐块）：{:?}",
            p2.toast_text()
        );
        assert_eq!(p2.field_error_visible("gateway.port"), Some(false), "成功清错误");

        // **反向判据**（PD24 ①）：只改热生效字段（`system.log_level`）的回执不含「需重启」
        // ⇒ 屏上**不得**冒出"需重启"（否则从"不说真话"翻到"谎报副作用"，同违 §2.6）。
        let mut hot_resp = ControlResponse::ok(
            "rid-3b",
            Some(applied.clone()),
            Some("audit-3".into()),
            1_003,
        );
        hot_resp.message = "配置已保存".into();
        p2.show_result(&hot_resp)
            .expect("show_result (ok, hot-only)");
        assert_eq!(
            p2.toast_text().as_deref(),
            Some("配置已保存"),
            "热生效路径照抄后端回执"
        );
        assert!(
            !p2.toast_text().as_deref().unwrap().contains("需重启"),
            "热生效字段**不得**被说成需重启：{:?}",
            p2.toast_text()
        );

        // ── ⑬ WriteMode::FullRewrite ⇒ Toast 明示（EDGE-23）──────────────────
        applied.write_mode = WriteMode::FullRewrite;
        p2.set_config(&applied).expect("set_config (full rewrite)");
        assert_eq!(
            p2.toast_tone(),
            Some(components::ToastTone::Warning),
            "整体回写 ⇒ 警示色（不是成功色）"
        );
        assert_eq!(
            p2.toast_text().as_deref(),
            Some(p2_config::TEXT_TOAST_FULL_REWRITE),
            "EDGE-23：必须明示原有文字（注释）已不存在"
        );
        // Toast 过期由 tick 关闭（3 s，UI §7.2）。
        p2.tick(std::time::Instant::now() + theme::Timing::toast() * 2);
        assert_eq!(p2.toast_text(), None, "过期后 tick 关掉 Toast");

        // ── ⑭ **注入非法值 ⇒ 降级可见 + 不可提交**（C1 的页级回归锁）──────────────
        //
        // 评审探针实测的旧形态：注入 `system.log_level = "trace"`（不在 options 内）⇒ 控件把它
        // 静默改写成 `options[0]`、页面**未经任何用户操作**即 `is_dirty()==true`、保存按钮可用
        // ⇒ 一次点击就把屏上从未展示过的值写进装置。
        //
        // 敏感性（**探针实测**）：把 `Core::after_field_edit` 里的 `touched` 记账去掉、
        // `Core::is_dirty` 改回"与注入值不等即脏"⇒ 下面每组的 `!is_dirty()` /
        // `draft().changes.is_empty()` / `save_disabled()` 三条立刻变红。
        {
            let n_before = got.borrow().len();

            // (a) `Enum` 注入值不在选项内。
            let mut bad_enum = p2_view(false);
            for g in &mut bad_enum.groups {
                for f in &mut g.fields {
                    if f.key == "system.log_level" {
                        f.value = Value::from("trace"); // 不在 [error, info] 内
                    }
                }
            }
            p2.set_config(&bad_enum).expect("set_config (非法 Enum 注入值)");
            assert_eq!(
                p2.field_error_visible("system.log_level"),
                Some(true),
                "非法注入值 ⇒ 该行**错误态**（红竖条 + 红字；不再是静默改写）"
            );
            assert_eq!(
                p2.field_disabled("system.log_level"),
                Some(true),
                "非法注入值 ⇒ 控件 `disabled`（不可交互）"
            );
            assert_eq!(
                p2.field_status_text("system.log_level").as_deref(),
                Some(p2_config::TEXT_INVALID_VALUE),
                "就地说明该值是无效的（不假装它是 ERROR）"
            );
            assert_eq!(p2.field_status_visible("system.log_level"), Some(true));
            assert!(!p2.is_dirty(), "**注入本身永不置脏**（关键的一半）");
            assert!(p2.draft().changes.is_empty(), "非法键不进草稿");
            // **B3**：非法注入值**不得**把该键排除出「恢复默认值」—— `default`（`info`，合法）正是
            // 修复该字段**唯一可用**的合法值。旧口径下"控件 `disabled`（上一条）+ 恢复补丁排除"
            // 叠加 ⇒ 该字段**页内无任何修复路径**（复审实测 `P-R2①/②/③`）。页级回归锁见 ⑯。
            assert_eq!(
                p2.defaults_patch().changes.get("system.log_level"),
                Some(&Value::from("info")),
                "非法注入值仍必须可经「恢复默认值」修好（B3）"
            );
            // **B4 页级回归锁**：`invalid` 门的页级独占性 —— 即便把该字段**旁路改成脏**
            // （`set_field_value` 走 `after_field_edit` ⇒ 记入 `touched`，等价于绕过 `disabled`
            // 的 rogue touch），它的键也必须被 `invalid` 门**挡在草稿之外**（否则一次点击即可
            // 把屏上不可核查的近似值写进装置 —— C1 的另一半）。
            //
            // 敏感性（**探针实测**）：把 `DraftScope::admits` 的 `!self.invalid.contains(key)`
            // 去掉 ⇒ 下面三条立刻变红，且下一拍「保存」会开出弹层。
            assert!(p2.set_field_value("system.log_level", &Value::from("error")));
            assert!(
                p2.draft().changes.is_empty(),
                "旁路改脏后，非法键仍不得进草稿（`invalid` 是否决票）"
            );
            assert!(
                !p2.is_dirty(),
                "非法键不得产生脏标记（`touched` 里的那条不算数）"
            );
            assert!(p2.save_disabled(), "⇒ 保存仍不可用");
            p2.discard_draft();
            assert!(p2.save_disabled(), "无可提交内容 ⇒ 保存不可用");
            // 直接派发 CLICKED（旁路 GUI 的禁用判定）—— 意图侧**也**必须拦住（双保险）。
            p2.save_button()
                .button()
                .obj()
                .send_event(EventCode::CLICKED);
            assert!(
                p2.with_dialog(|d| d.is_alive()).is_none(),
                "不得开出保存弹层"
            );
            assert_eq!(
                got.borrow().len(),
                n_before,
                "**一次点击不产生任何提交意图**（非法值不可经屏写进装置）"
            );
            // 降级只作用于该字段：同视图的合法字段照常可编辑、可提交。
            assert!(p2.set_field_value("gateway.port", &Value::from(2407)));
            assert!(p2.is_dirty());
            let d = p2.draft();
            assert_eq!(d.changes.len(), 1, "草稿只含**用户真改过**的合法字段");
            assert!(
                !d.changes.contains_key("system.log_level"),
                "草稿里不得出现非法键"
            );
            p2.discard_draft();
            assert!(!p2.is_dirty());
            assert_eq!(
                p2.field_error_visible("system.log_level"),
                Some(true),
                "「放弃修改」**不**清除非法态（它是注入的固有状态，不是草稿）"
            );

            // (b) `U16` 注入值类型错配（字符串 `"abc"`）—— 旧形态会显示 `min` 并置脏。
            let mut bad_u16 = p2_view(false);
            for g in &mut bad_u16.groups {
                for f in &mut g.fields {
                    if f.key == "gateway.port" {
                        f.value = Value::from("abc");
                    }
                }
            }
            p2.set_config(&bad_u16).expect("set_config (非法 U16 注入值)");
            assert_eq!(p2.field_error_visible("gateway.port"), Some(true));
            assert_eq!(p2.field_disabled("gateway.port"), Some(true));
            assert!(!p2.is_dirty(), "类型错配不得置脏");
            assert!(p2.draft().changes.is_empty());
            assert!(p2.save_disabled());
            p2.save_button()
                .button()
                .obj()
                .send_event(EventCode::CLICKED);
            assert_eq!(got.borrow().len(), n_before, "同样不产生意图");

            // (c) **正向对照**：注入**合法**值 ⇒ 一切照常（防把正常路径也判成非法）。
            let ok = p2_view(false);
            p2.set_config(&ok).expect("set_config (合法值)");
            assert_eq!(p2.field_error_visible("gateway.port"), Some(false));
            assert_eq!(p2.field_disabled("gateway.port"), Some(false), "合法 ⇒ 可编辑");
            assert_eq!(p2.field_disabled("display.bind_addr"), Some(true), "只读仍禁用");
            assert!(!p2.is_dirty(), "注入 ⇒ 不脏");
            assert!(p2.draft().changes.is_empty());
            assert!(p2.save_disabled(), "无改动 ⇒ 保存置灰");
            assert!(p2.set_field_value("gateway.port", &Value::from(2408)));
            assert!(p2.is_dirty(), "合法字段被改 ⇒ 脏");
            p2.discard_draft();

            // (d) **合法但控件表示不了**的值（`U64::MAX` 超出 `Stepper` 的 `i64` 值域）——
            //     该值**合法**（`invalid` 集合挡不住它），控件渲染的是 `i64::MAX`（≠ 注入值），
            //     拦住"注入即置脏"的**只有** touched 门（C1 第 3 条）。见 PD21(ii)。
            //
            //     敏感性（**探针实测**）：把 [`p2_config::DraftScope::admits`] 的
            //     `touched.contains(key)` 去掉 ⇒ 本组 `!is_dirty()` / `draft().changes.is_empty()`
            //     / `save_disabled()` 三条立刻变红。
            let mut wide = p2_view(false);
            for g in &mut wide.groups {
                for f in &mut g.fields {
                    if f.key == "telemetry.period" {
                        f.kind = ConfigKind::U64 {
                            min: 0,
                            max: u64::MAX,
                            step: 1,
                        };
                        f.value = Value::from(u64::MAX);
                        f.default = Value::from(u64::MAX);
                    }
                }
            }
            p2.set_config(&wide).expect("set_config (合法但超出控件值域)");
            assert_eq!(
                p2.field_error_visible("telemetry.period"),
                Some(false),
                "**合法值**不是错误态（invalid 挡不住它）"
            );
            assert_eq!(p2.field_disabled("telemetry.period"), Some(false));
            assert!(
                p2.field_value_text("telemetry.period").as_deref() != Some("18446744073709551615"),
                "自证：控件确实**表示不了**该值（渲染成 i64 侧的值）"
            );
            assert!(!p2.is_dirty(), "**用户没碰过 ⇒ 不脏**（哪怕控件值与注入值不同）");
            assert!(p2.draft().changes.is_empty());
            assert!(p2.save_disabled());
            p2.discard_draft();
        }

        // ── ⑮ **单个字段的合法元数据不得让整页拒绝渲染**（I1 / PD15 / PD16）──────────
        //
        // 旧形态两处整页 `Err`：① `U16{step:0}`（契约 `control.rs:528` 写 `if *step != 0`
        // ⇒ **显式容忍**，而 `Stepper::new` 只收正步长）；② `Enum` 选项 ≥ 10（`10 × 96 = 960 >
        // INNER_W 958`）。两条都必须降为**字段级**（其它字段照常）。
        {
            // ① 步长 0 ⇒ 折为 1，页面照常、该字段仍可编辑。
            let mut z = p2_view(false);
            for g in &mut z.groups {
                for f in &mut g.fields {
                    if f.key == "intercore.port" {
                        f.kind = ConfigKind::U16 {
                            min: 1,
                            max: 65535,
                            step: 0,
                        };
                    }
                }
            }
            p2.set_config(&z)
                .expect("`step == 0` 契约合法 ⇒ 页面必须成功渲染（不得整页 Err）");
            assert!(p2.is_available());
            assert_eq!(p2.group_count(), 3, "其余分组照常（不是白屏）");
            assert_eq!(
                p2.field_disabled("intercore.port"),
                Some(false),
                "该字段仍可编辑（步长视为 1）"
            );
            assert!(p2.set_field_value("intercore.port", &Value::from(2600)));
            assert!(p2.is_dirty(), "步长折算后照样能改 ⇒ 进草稿");
            p2.discard_draft();

            // ② 10 个选项 ⇒ 窗口截断（不整页 Err），且**窗口必含当前值**（`v9` = 最后一个）。
            let mut many = p2_view(false);
            for g in &mut many.groups {
                for f in &mut g.fields {
                    if f.key == "system.log_level" {
                        f.kind = ConfigKind::Enum {
                            // 标签取 `N0..N9`：`N` 在 `ASCII_DISPLAY_ALPHABET` 内 ⇒
                            // `display_safe` 对它是**恒等**（用 `L` 会被兜底成 `?`，
                            // 断言就会读到 `?9` 而看不出窗口对不对）。
                            options: (0..10)
                                .map(|i| OptionItem {
                                    value: format!("v{i}"),
                                    label: format!("N{i}"),
                                })
                                .collect(),
                        };
                        f.value = Value::from("v9");
                    }
                }
            }
            p2.set_config(&many)
                .expect("10 个选项 ⇒ 字段级降级，不得整页 Err");
            assert!(p2.is_available());
            assert_eq!(p2.group_count(), 3, "其余分组照常");
            assert_eq!(
                p2.field_value_text("system.log_level").as_deref(),
                Some("N9"),
                "窗口必须**含当前值**（否则会把合法值静默改写成 N0）"
            );
            assert_eq!(
                p2.field_status_text("system.log_level").as_deref(),
                Some("仅显示 9 段"),
                "截断必须**上屏**说明（被隐藏的段数可见；B1：用 `段` 而非缺字的 `项`）"
            );
            assert_eq!(
                p2.field_error_visible("system.log_level"),
                Some(false),
                "合法值 ⇒ 不是错误态"
            );
            assert!(p2.set_field_value("gateway.port", &Value::from(2409)));
            assert!(p2.is_dirty(), "其它字段仍可正常渲染与编辑");
            p2.discard_draft();
        }

        // ── ⑯ **B3 页级回归锁**：非法注入值必须**能**经「恢复默认值」修好（页内修复路径）──
        //
        // 旧形态（复审实测 `P-R2①/②/③`）：非法值的键被排除在恢复补丁之外，叠加该行控件
        // `disabled` ⇒ **页内无任何修复路径**（该字段永久不可改，除非重启进程或后端改值），
        // 且弹层文案「全部运行参数将恢复默认值并立即生效」与实际少写一个键**不符**（对操作者失实）。
        //
        // 敏感性（**探针实测**）：把 `p2_config::resettable` 改回"按 `invalid` 排除**当前非法值**"
        // （旧口径）⇒ 下面 ① 与 ③ 两条立刻变红。
        {
            let mut fixme = p2_view(false);
            for g in &mut fixme.groups {
                for f in &mut g.fields {
                    if f.key == "system.log_level" {
                        f.value = Value::from("trace"); // 当前值非法（不在选项内）
                        f.default = Value::from("info"); // default 合法 ⇒ 唯一可用的修复值
                    }
                }
            }
            p2.set_config(&fixme)
                .expect("set_config（非法注入值 + 合法 default）");
            assert_eq!(p2.field_error_visible("system.log_level"), Some(true));
            assert_eq!(
                p2.field_disabled("system.log_level"),
                Some(true),
                "前提：该行控件确实被禁用 ⇒「恢复默认值」是**唯一**剩下的修复路径"
            );
            // ① 补丁口径：`default` 合法 ⇒ 必须纳入（值 = default）。
            assert_eq!(
                p2.defaults_patch().changes.get("system.log_level"),
                Some(&Value::from("info")),
                "非法注入值不得被排除出「恢复默认值」补丁（B3）"
            );
            // ② 只读字段仍不进（PD12 不得回退）。
            assert!(
                !p2.defaults_patch()
                    .changes
                    .contains_key("display.bind_addr"),
                "只读字段仍不得进补丁（PD12 不得回退）"
            );
            // ③ 经弹层确认后**实际提交**的补丁同样含它（明细与补丁逐条一致）。
            let n0 = got.borrow().len();
            p2.reset_button()
                .button()
                .obj()
                .send_event(EventCode::CLICKED);
            assert_eq!(
                p2.with_dialog(|d| d.level()),
                Some(crate::ui::theme::ConfirmLevel::L2),
                "无瞬断字段 ⇒ 恢复默认值走 L2（长按 1.0 s）"
            );
            p2.with_dialog(|d| {
                d.confirm_button()
                    .button()
                    .obj()
                    .send_event(EventCode::LONG_PRESSED)
            });
            assert_eq!(got.borrow().len(), n0 + 1, "确认完成 ⇒ 恰好发一次意图");
            {
                let g = got.borrow();
                let last = g.last().expect("末条");
                assert_eq!(last.0.from, PatchSource::ResetDefault);
                assert_eq!(
                    last.0.changes.get("system.log_level"),
                    Some(&Value::from("info")),
                    "**实际提交**的恢复补丁必须含该键（否则该字段页内永久不可改）"
                );
            }
            // ④ `default` **自身非法** ⇒ **不**纳入（PD22：混入会让整单被后端二次校验打回）；
            //    其余字段照常可恢复。
            let mut bad_def = p2_view(false);
            for g in &mut bad_def.groups {
                for f in &mut g.fields {
                    if f.key == "intercore.port" {
                        f.default = Value::from(99999u64); // 超出 u16::MAX（65535）
                    }
                }
            }
            p2.set_config(&bad_def)
                .expect("set_config（default 自身非法）");
            assert!(
                !p2.defaults_patch().changes.contains_key("intercore.port"),
                "default 不合法的键不得混入（PD22；否则整单被后端打回）"
            );
            assert!(
                p2.defaults_patch().changes.contains_key("gateway.port"),
                "其余可恢复字段照常"
            );
            p2.discard_draft();
        }

        drop(p2);
    }

    // ═══ P4 安全 / 联锁页（B2b-3；F16–F18，含写操作）═══════════════════════════
    //
    // 覆盖：装配契约（契约 1′）／三态总态（含 **fail-closed** 回归锁）／latch 胶囊／按钮矩阵／
    // 触发源行／三卡（含 `None` ⇒ 未知）／就地原因带（含**倒计时**）／弹层与**意图**载荷／
    // 失败路径（弹层不自动关闭 + 冲突文案 + 审计不可写）。
    {
        use crate::ui::pages::p4_interlock;
        use mupc_display_proto::{
            ControlCode, ControlResponse, InterlockOpAck, InterlockReject, InterlockSourceItem,
        };

        /// 造联锁段（默认全 false = 契约缺省 = **不可用**）。
        fn il(available: bool, enabled: bool, latched: bool) -> mupc_display_proto::InterlockSection {
            mupc_display_proto::InterlockSection {
                available,
                enabled,
                latched,
                ..Default::default()
            }
        }

        // ── ① 装配契约（**契约 1′**：页根容器 + 固定操作条；线框 `Y624`）───────────
        let p4 = p4_interlock::P4InterlockPage::new(&host).expect("P4InterlockPage::new");
        disp.refr_now_for_test();
        assert_eq!(
            p4.obj().size(),
            (Dimens::CONTENT_W, Dimens::CONTENT_H),
            "页根 = 内容区整幅 992×624（**不是**滚动容器 —— 见 pages/mod.rs 契约 1′）"
        );
        // U-73（T21c-1）：滚动视口下移到常驻总览带之下 ⇒ 高 342（= 624 − 上边距 8 − 带 162
        // − 同组缝 16 − 原因带 24 − 操作条 72）。**首屏可行性**：§B 全量 = 源卡 202 + 16 +
        // 灯卡 106 = 324 ≤ 342 ⇒ 既有联锁区仍**首屏全可见**（UI §6.4.1 的核心不变量；
        // 差值见 p4_interlock.rs 的 **IL29③**）。
        assert_eq!(
            p4.scroll_obj().size(),
            (Dimens::CONTENT_W, 342),
            "滚动视口 = 624 − 上边距 8 − 总览带 162 − 缝 16 − 原因带 24 − 操作条 72 = 342"
        );
        assert!(
            p4.scroll_obj().size().1 >= 340,
            "§B 既有内容全量 340（触发源 216 + 缝 16 + 灯卡 108，按 UI 口径）⇒ 视口必须 ≥ 340"
        );
        let root_c = p4.obj().coords();
        let bar_c = p4.action_bar_obj().coords();
        assert_eq!(
            (bar_c.x1, bar_c.y1),
            (root_c.x1, root_c.y1 + 552),
            "固定操作条贴在页根底缘（不随滚动；绝对 y624 —— UI §6.4 线框逐像素一致）"
        );
        assert_eq!(p4.action_bar_obj().size().1, 72, "操作条高 72（UI 线框 y624–696）");
        // 两个危险按钮：320×64、间距 ≥48（UI §6.4 固定操作条行逐字）。
        let rel_c = p4.release_button().button().obj().coords();
        let rst_c = p4.restart_button().button().obj().coords();
        assert_eq!(
            (rel_c.x1, rel_c.y1),
            (root_c.x1, root_c.y1 + 556),
            "「人工释放联锁」左对齐、在操作条内垂直居中（绝对 y628；线框 630，−2 见 **IL3**）"
        );
        assert_eq!(
            p4.release_button().button().obj().size(),
            (320, 64),
            "危险按钮 320×64（UI §6.4：320×64；设计 F17-1 ≥64×64）"
        );
        assert_eq!(rst_c.y1, rel_c.y1, "两危险按钮同排");
        assert!(
            rst_c.x1 - rel_c.x2 - 1 == Dimens::GAP_DANGER,
            "两危险按钮间距必须 = 48 px（UI §6.4「两者间距 48 px」；实测 {} px）",
            rst_c.x1 - rel_c.x2 - 1
        );
        assert_eq!(
            rel_c.x1 - root_c.x1,
            0,
            "左按钮贴内容区左缘（页内 x0 = 绝对 x16 —— UI 线框 (16,630,336,694)）"
        );
        assert_eq!(
            p4.audit_note_text().as_deref(),
            Some(p4_interlock::TEXT_AUDIT_NOTE),
            "右端弱注「操作将记入审计」（UI §6.4 固定操作条行）"
        );
        // 三张状态卡的卡头（UI §6.4 线框：停机失败 / 故障灯 / 运行灯）。
        for (i, head) in [
            p4_interlock::TEXT_CARD_STOP,
            p4_interlock::TEXT_CARD_FAULT_LAMP,
            p4_interlock::TEXT_CARD_RUN_LAMP,
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(p4.lamp_head_text(i).as_deref(), Some(*head));
        }

        // 两张主卡的尺寸锚点（总态卡 `162`/宽 `488` 见 **IL29**；触发源卡在 2 源时 `202`）。
        // **这两条同时是"存活锚点"回归锁**：`Core::state_card` 曾是 `new()` 的局部变量 ⇒ 返回时
        // 被 `Drop`、总态卡**整棵子树级联删除**（屏幕上一块空白、控制台无任何报错）；本用例的
        // 「骨架态 ⇒ 联锁状态不可用」断言（读卡内标签的文本）当场抓出了它（B2b-3 实测）。
        assert_eq!(
            p4.state_card_obj().size(),
            (Dimens::BAND_CARD_W, 162),
            "联锁总态卡 = 488×162（UI §6.4.1 写 488×164；差 2 px 见 **IL29③**）"
        );
        assert!(p4.state_card_obj().is_alive(), "总态卡必须存活（否则卡内全部子件被级联删除）");
        assert!(p4.source_card_obj().is_alive(), "触发源卡必须存活");

        // ── ② 骨架态 = **无帧** ⇒ 不可用（**绝不**「未联锁」）─────────────────────
        // 「改什么会让本条变红」：把 `new()` 末尾的 `apply_section(default)` 去掉（或把
        // `state_view` 的 `!available` 分支删掉）⇒ 下面「联锁状态不可用」那条立刻变红。
        // ⚠️ **IL29① 订正（2026-09-25）**：96 px 主值槽只有 366 px，「联锁状态不可用」7 字
        // × 96 = 672 px **放不下**（`DOTS` 截断）⇒ 该槽取**缩短形态**「不可用」；全串仍是
        // **就地原因带 / 触发源卡 / latch 胶囊**三处的落点（④ 段逐条断言，见
        // `reason_left_text()` == 全串）⇒ **语义不缩水，只是落点不同**。
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE_SHORT),
            "无帧 ⇒ 契约缺省 = 不可用（96 px 主值槽取缩短形态，见 IL29①）"
        );
        assert_ne!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNLATCHED),
            "**不可用 ≠ 未联锁**（fail-closed；缩短的是字形数，不是语义）"
        );
        assert!(!p4.ever_available(), "尚未注入过有效帧");

        // ── ③ 正常态（available = true / latched = false / 无触发源）──────────────
        let normal = il(true, true, false);
        p4.set_section(&normal);
        disp.refr_now_for_test();
        assert_eq!(
            p4.state_view(),
            p4_interlock::StateView::Unlatched,
            "确知未联锁"
        );
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNLATCHED),
            "总态词「未联锁」（UI §3.6 P4「总态」行）"
        );
        assert_eq!(
            p4.state_color(),
            Palette::OK,
            "未联锁 = 绿 #2FDB8A（UI §6.4 区块规格）"
        );
        assert_eq!(p4.state_icon_text().as_deref(), Some("✓"));
        assert_eq!(
            p4.latch_chip_text().as_deref(),
            Some(p4_interlock::TEXT_LATCH_UNHELD),
            "available = true 时才可显「未保持」"
        );
        assert_eq!(p4.latch_chip_color(), Palette::TEXT_SECOND, "未保持胶囊字色 #A6B6D6");
        assert_eq!(p4.latch_chip_visible_count(), 1, "三态胶囊互斥：恒有且仅有 1 个在显");
        assert!(
            p4.source_empty_visible() && !p4.source_unavailable_visible(),
            "无触发源 ⇒ **空态**（不是不可用态）"
        );
        assert_eq!(
            p4.source_empty_text().as_deref(),
            Some(p4_interlock::TEXT_SOURCES_EMPTY),
            "空态文案「当前无联锁触发源」（IL-01.2）"
        );
        assert_eq!(
            p4.sources_title_text().as_deref(),
            Some("触发源 · 0"),
            "卡头含数量（UI §6.4：卡头 28 px 含数量）"
        );
        assert!(
            !p4.release_disabled() && !p4.restart_disabled(),
            "正常态两按钮皆可用"
        );
        assert!(
            !p4.reason_left_visible() && !p4.reason_right_visible(),
            "正常态无就地原因"
        );
        assert_eq!(p4.stop_card_text().as_deref(), Some(p4_interlock::TEXT_STOP_OK));
        // `fault_lamp = None` / `run_lamp = None` ⇒ **「未知」**（**绝不**臆造「灯灭」！）
        // 「改什么会让本条变红」：把 `tri_view(None)` 改成 `TriView::Off` ⇒ 两条立刻变红。
        assert_eq!(
            p4.fault_lamp_text().as_deref(),
            Some(p4_interlock::TEXT_LAMP_UNKNOWN),
            "灯态未知 ⇒ 「未知」（契约 Option<bool> 的三态语义，F16-4）"
        );
        assert_ne!(
            p4.fault_lamp_text().as_deref(),
            Some(p4_interlock::TEXT_LAMP_OFF),
            "**不得**把 None 臆造成「灯灭」"
        );
        assert_eq!(
            p4.run_lamp_text().as_deref(),
            Some(p4_interlock::TEXT_LAMP_UNKNOWN)
        );

        // ── ④ **fail-closed 回归锁**（`available = false`）───────────────────────
        // UI §8.3 联锁专行：不可用 ⇒ 两按钮 disabled + 就地原因 + **不得显示「未联锁」**。
        // 「改什么会让本条变红」：把 `state_view` 的 `!available` 分支**删掉**、或把它回落成
        // `Unlatched`（fail-open）⇒ 本段断言立刻变红。
        //
        // ⚠️ **订正（B2b-3 代码质量整改 ④）**：此处原先写「把 `state_view` 改成
        // `if s.latched {..} else if !available` ⇒ 第 2/3 条**立刻变红**」—— **实测该变异下本段
        // 全绿**（只有离线单测 `p4_interlock.rs::unavailable_state_never_falls_back_to_unlatched`
        // 变红）：本段此前只注入 `available = false && latched = false` 这一组合，而它在两种写法下
        // **同判** `Unavailable`（先判 `latched` 也进不去）。漏掉的自由度是
        // **`available = false && latched = true`** ⇒ 已在 ④′ 补段钉死。
        p4.set_section(&il(false, true, false));
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE_SHORT),
            "状态源不可用 ⇒ 主值槽「不可用」（**缩短形态**，IL29①）"
        );
        assert_ne!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNLATCHED),
            "**不可用 ≠ 未联锁**：无法获知 ≠ 确知安全（fail-closed）"
        );
        assert_eq!(p4.state_color(), Palette::STOPPED, "灰 #8C98AC");
        assert_eq!(
            p4.state_icon_text().as_deref(),
            Some("?"),
            "问号 / 断链语义 —— **不得**复用 ✓ 与 ⚠（§8.3 明文）"
        );
        assert_eq!(
            p4.latch_chip_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE),
            "latch 胶囊一并转灰"
        );
        assert_ne!(
            p4.latch_chip_text().as_deref(),
            Some(p4_interlock::TEXT_LATCH_UNHELD),
            "**不得**显示「未保持」"
        );
        assert_eq!(p4.latch_chip_color(), Palette::PLACEHOLDER, "灰底灰字 #96A2BC");
        assert!(p4.release_disabled() && p4.restart_disabled(), "两按钮均 disabled（fail-closed）");
        assert!(p4.reason_left_visible(), "就地原因可见");
        assert_eq!(
            p4.reason_left_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE),
            "就地原因 = 「联锁状态不可用」（按钮上方 24 px 就地表述）"
        );
        assert!(
            !p4.source_empty_visible() && p4.source_unavailable_visible(),
            "触发源卡同步转**不可用态**（**不得**显「无触发源」—— 空态 vs 不可用态语义不同）"
        );
        assert_eq!(
            p4.source_unavailable_title().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE)
        );
        assert_ne!(
            p4.source_empty_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE),
            "空态文案与不可用态文案必须**各自独立**（§8.3「不得互替」）"
        );
        // 三灯卡**同步灰化**：不得显「灯灭 / 正常」这类**确知**结论。
        for i in 0..3 {
            assert_eq!(
                p4.lamp_value_text(i).as_deref(),
                Some("? 联锁状态不可用"),
                "第 {i} 张卡 unavailable=false 时转灰（**不臆造**灯态 / 停机结论）"
            );
        }
        assert_ne!(p4.stop_card_text().as_deref(), Some(p4_interlock::TEXT_STOP_OK));
        assert!(
            !p4.take_refresh_request(),
            "纯展示降级不置刷新标志"
        );

        // ── ④′ **漏掉的自由度**：`available = false` **且** `latched = true` ───────────
        // （B2b-3 代码质量整改 ④：④ 段只测了 `latched = false`，而 `state_view` 若被改成
        //  先判 `latched`，`latched = false` 的组合在两种写法下**同判** `Unavailable`
        //  ⇒ 该变异能从 ④ 段下溜过去。本段补上 `latched = true` 这一组合，把它钉死。）
        //
        // **可测性论证**：缺帧 / 状态源不可用时，`latched` 这一比特**本身不可信**（可能只是
        // 结构体缺省 `false`，也可能残留上一拍的真值）⇒ 它**不得**参与判定；屏上**只能**出现
        // 「联锁状态不可用」。
        // 「改什么会让本条变红」：把 `state_view` 改成 `if s.latched {..} else if !s.available`
        // （或任何先认 `latched` 的写法）⇒ 本段第 1 / 2 条立刻变红（实测，见整改报告探针输出）。
        p4.set_section(&il(false, true, true));
        assert_eq!(
            p4.state_view(),
            p4_interlock::StateView::Unavailable,
            "`available = false && latched = true` ⇒ **仍不可用**（`latched` 不可信）"
        );
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE_SHORT),
            "屏上主值槽**只能**是不可用（缩短形态）—— **绝不**因 `latched = true` 显示「已联锁」"
        );
        assert_ne!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_LATCHED),
            "**不可用 ≠ 已联锁**（fail-closed 的另一半：不得把「无法获知」报成「已联锁」）"
        );
        assert_ne!(p4.state_icon_text().as_deref(), Some(p4_interlock::ICON_STATE_LATCHED));
        assert_ne!(
            p4.latch_chip_text().as_deref(),
            Some(p4_interlock::TEXT_LATCH_HELD),
            "latch 胶囊**不得**因 `latched = true` 显示「已保持」（§8.3）"
        );
        assert!(p4.release_disabled() && p4.restart_disabled(), "两按钮仍 disabled");
        assert_eq!(
            p4.reason_left_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE)
        );

        // ── ⑤ 触发源 2 项（含数量 / 行形态 / 色通道 / 机器名→中文）────────────────
        let mut two = il(true, true, true);
        two.enabled = true;
        two.sources = vec![
            InterlockSourceItem {
                name: "estop".into(),
                tripped: true,
            },
            InterlockSourceItem {
                name: "door".into(),
                tripped: false,
            },
        ];
        two.stop_failed = false;
        p4.set_section(&two);
        assert_eq!(
            p4.sources_title_text().as_deref(),
            Some("触发源 · 2"),
            "卡头含数量（IL-01.2）；`（2）` 的全角括号缺字 ⇒ 取 `· 2`（IL1）"
        );
        assert_eq!(p4.source_rows_visible(), 2, "全部源一次性列出，不折叠");
        assert!(
            !p4.source_empty_visible() && !p4.source_unavailable_visible(),
            "有源 ⇒ 既非空态也非不可用态"
        );
        assert_eq!(
            p4.source_row_name(0).as_deref(),
            Some(p4_interlock::TEXT_SRC_ESTOP),
            "机器名 `estop` → 中文名「急停」（UI §3.6 P4 触发源行）"
        );
        assert_eq!(
            p4.source_row_status(0).as_deref(),
            Some(p4_interlock::TEXT_SRC_TRIPPED)
        );
        assert_eq!(p4.source_row_tripped(0), Some(true), "已触发 ⇒ 实心形态");
        assert_eq!(
            p4.source_row_color(0),
            Some(Palette::LINK_DOWN),
            "已触发 = #DC3545 实心（UI §6.4 触发源卡行）"
        );
        assert_eq!(
            p4.source_row_name(1).as_deref(),
            Some(p4_interlock::TEXT_SRC_DOOR)
        );
        assert_eq!(
            p4.source_row_status(1).as_deref(),
            Some(p4_interlock::TEXT_SRC_UNTRIPPED)
        );
        assert_eq!(p4.source_row_tripped(1), Some(false), "未触发 ⇒ 空心形态");
        assert_eq!(
            p4.source_row_color(1),
            Some(Palette::LINK_UNCONFIGURED),
            "未触发 = #5F6368 空心"
        );
        // 卡高随源数自适应：2 行 ⇒ 卡头 44 + 空态高 124 + 上下内边距 34 = 202（UI 写 200，**IL3**）。
        disp.refr_now_for_test(); // 几何读回须先布局（`Obj::size()` 读的是 `coords`）
        assert_eq!(p4.source_card_height(), 202, "触发源卡高（UI 线框 200，+2 见 IL3）");
        // **漂移锁**：页内推导的两个卡体高常量必须与**组件实测高**一致
        //（`EMPTY_H` / `UNAVAILABLE_H` 是按组件构件算式推导的 —— 组件改版式 ⇒ 本条变红）。
        assert_eq!(
            p4.source_empty_obj().size().1,
            p4_interlock::EMPTY_H,
            "空态实测高必须 = 页内推导的 EMPTY_H（构造期读 `size()` 恒为 0，故不能读组件，见 EMPTY_H 注）"
        );
        assert_eq!(
            p4.source_unavailable_obj().size().1,
            p4_interlock::UNAVAILABLE_H,
            "不可用态实测高必须 = 页内推导的 UNAVAILABLE_H"
        );

        // ── ⑤′ **源数超出行池上限**（5 源）⇒ 差额必须**可见**（B2b-3 代码质量整改 ②）──────
        // 评审实测（5 源）：`title = 触发源 · 5`、`rows_visible = 4`、`row4 = None`、**屏上零提示**
        // —— 与 UI §6.4（`：599`）「**全部源一次性列出，不折叠**」的明文冲突（且 IL9 的"钉死"是
        // 单向的：后端 `source_token()` 加变体只会让 `source_token()` 编译失败，**不会**让
        // `SOURCE_ROW_POOL` 报错 ⇒ 无网兜住）。本段把「差额可见」钉死。
        // 「改什么会让本条变红」：把 `sources_overflow_note` 改成恒返回空串（= 去掉提示）
        // ⇒ 下面第 ③ 条立刻变红（实测，见整改报告探针输出）。
        let mut five = il(true, true, false);
        five.sources = vec![
            InterlockSourceItem {
                name: "estop".into(),
                tripped: true,
            },
            InterlockSourceItem {
                name: "flood".into(),
                tripped: true,
            },
            InterlockSourceItem {
                name: "fire".into(),
                tripped: false,
            },
            InterlockSourceItem {
                name: "door".into(),
                tripped: true,
            },
            InterlockSourceItem {
                name: "spare".into(),
                tripped: false,
            },
        ];
        p4.set_section(&five);
        // ① 行池上限**没有被悄悄扩容**（本处置是"可见提示"，不是"多铺一行"）。
        assert_eq!(
            p4.source_rows_visible(),
            4,
            "行池恒 4（IL9 上限不动）；第 5 条**不铺行**"
        );
        assert_eq!(p4.source_row_tripped(4), None, "第 5 行不存在（`None` = 隐藏 / 未建）");
        // ② 卡头仍报**真实**总数（数量 ≠ 行数）。
        assert!(
            p4.sources_title_text()
                .as_deref()
                .is_some_and(|t| t.starts_with("触发源 · 5")),
            "卡头报真实总数 5（实际 {:?}）",
            p4.sources_title_text()
        );
        // ③ **差额在屏上可见**（补偿；`还`/`有`/`条` 与数字逐字在 cmap 内）。
        assert_eq!(
            p4.sources_title_text().as_deref(),
            Some("触发源 · 5 · 还有 1 条"),
            "超限 ⇒ 卡头**可见地**说出差额（不得静默；UI §6.4「全部源一次性列出」的补偿）"
        );
        // ④ 未超限时**不得**多出提示（卡头逐字不变）—— 下一段（⑥）正好以 `two` 起手。
        p4.set_section(&two);
        assert_eq!(
            p4.sources_title_text().as_deref(),
            Some("触发源 · 2"),
            "未超限 ⇒ 卡头逐字不变（提示只在超出时出现）"
        );

        // ── ⑥ latch 态 ⇒ **只**禁用 M1 + **按钮正上方**就地原因（IL-03，不得静默失败）──
        p4.set_section(&two); // latched = true
        assert!(
            !p4.release_disabled(),
            "释放是安全正向操作 ⇒ 不因 latch 置灰（**IL18**：否则 EDGE-12 的具体原因路径不可达）"
        );
        assert!(p4.restart_disabled(), "latch 态 ⇒ M1 授权重启 disabled");
        assert!(
            p4.reason_right_visible(),
            "M1 按钮**正上方** 24 px 就地原因必须可见（IL-03）"
        );
        assert_eq!(
            p4.reason_right_text().as_deref(),
            Some(p4_interlock::TEXT_REASON_LATCHED),
            "原因文案「处于自锁态 · 须先释放联锁」（`，` 缺字 ⇒ `·`，IL1）"
        );
        disp.refr_now_for_test();
        let rst_x = p4.restart_button().button().obj().coords().x1;
        assert!(rst_x > root_c.x1, "M1 按钮在右（与左按钮间距 48）");
        // `stop_failed = true` ⇒ **两按钮仍可用、无就地原因**（B2b-3 评审整改 ①）。
        // §6.4「操作与拒绝原因」表 / §9 F18 **只**要求 latch 态置灰 M1；若在此本地预判
        // 「停机未确认」，后端 `RejectedPrecondition[StopPending]` 的**具体**原因就永远到不了屏
        // （现场只看到灰按钮、看不到为什么）⇒ 与同页 **IL18** 对 `release` 的取向自相矛盾。
        // 放开后前端按钮可用 ⇒ 后端必回 `StopPending` ⇒ 该具体原因可达（EDGE-12）。
        let mut sf = il(true, true, false);
        sf.stop_failed = true;
        p4.set_section(&sf);
        assert_eq!(
            p4.stop_card_text().as_deref(),
            Some(p4_interlock::TEXT_STOP_FAIL),
            "停机失败照旧由**状态卡**表达（整改 ① 只移除**按钮级本地预判**，不动屏显）"
        );
        assert!(
            !p4.restart_disabled(),
            "`stop_failed` 不得本地预判置灰 M1（评审整改 ①）"
        );
        assert!(
            !p4.reason_right_visible(),
            "`stop_failed` 不得产出本地就地原因（评审整改 ①）"
        );
        assert!(!p4.release_disabled());

        // ── ⑦ 未确认 ⇒ **不发任何意图**；确认（L2：长按满 1.0 s）⇒ 交出观测载荷 ──────
        // 「改什么会让本条变红」：把按钮回调里的 `open_dialog(..)` 换成直接 `fire_release(..)`
        // （绕开弹层）⇒ 「未确认时无意图」那条立刻变红（与 P2 的探针 P2 同款）。
        let got_rel: Rc<RefCell<Vec<mupc_display_proto::InterlockOpPayload>>> =
            Rc::new(RefCell::new(Vec::new()));
        let got_m1: Rc<RefCell<Vec<mupc_display_proto::InterlockOpPayload>>> =
            Rc::new(RefCell::new(Vec::new()));
        {
            let g = Rc::clone(&got_rel);
            p4.set_on_release(move |p| g.borrow_mut().push(p));
        }
        {
            let g = Rc::clone(&got_m1);
            p4.set_on_ack_m1(move |p| g.borrow_mut().push(p));
        }
        p4.set_section(&two); // 重新回到 latch 态（源 = estop 已触发 / door 未触发）
        p4.release_button()
            .button()
            .obj()
            .send_event(EventCode::CLICKED);
        assert!(
            got_rel.borrow().is_empty(),
            "**未确认 ⇒ 不发出任何意图**（未确认 = 无网络动作）"
        );
        assert!(
            p4.with_dialog(|d| d.level())
                .map(|l| l == crate::ui::theme::ConfirmLevel::L2)
                .unwrap_or(false),
            "联锁释放 = **L2**（危险色 + 长按 1.0 s，UI §2.5 / §7.3）"
        );
        assert_eq!(
            p4.with_dialog(|d| d.title().text()).flatten().as_deref(),
            Some(p4_interlock::TEXT_DIALOG_TITLE_RELEASE)
        );
        assert!(
            p4.with_dialog(|d| d.default_focus_is_cancel()).unwrap_or(false),
            "TT-09：默认焦点「取消」"
        );
        assert!(
            p4.with_dialog(|d| d.has_progress()).unwrap_or(false),
            "L2：长按进度条就位（1.0 s 保持）"
        );
        assert!(
            p4.with_dialog(|d| !d.has_warn_banner()).unwrap_or(false),
            "本页无瞬断字段 ⇒ L2 不强制 WarnBanner（**不是** L2+）"
        );
        assert_eq!(
            p4.with_dialog(|d| d.impact().text()).flatten().as_deref(),
            Some(p4_interlock::TEXT_IMPACT_RELEASE),
            "L2 **必须**出现「影响范围」段且为**具体副作用**（§7.3）"
        );
        // 长按满 1.0 s ⇒ 恰好一次意图，载荷 = **UI 观测态**（机器名 + latch 比特）。
        p4.with_dialog(|d| {
            d.confirm_button()
                .button()
                .obj()
                .send_event(EventCode::LONG_PRESSED)
        });
        assert_eq!(got_rel.borrow().len(), 1, "确认完成 ⇒ 恰好发一次意图");
        {
            let g = got_rel.borrow();
            assert!(g[0].observed_latched, "观测到的 latch 态");
            assert_eq!(
                g[0].observed_sources,
                vec!["estop".to_string(), "door".to_string()],
                "载荷是**机器名**（后端按 token 比对，IL16）—— 不是中文标签"
            );
        }
        assert!(got_m1.borrow().is_empty(), "不得误触 M1 授权槽");

        // ── ⑧ 失败回执（RejectedPrecondition）⇒ 就地原因 + **弹层不自动关闭** ────────
        // 「改什么会让本条变红」：在 `show_result` 的失败分支加 `close_dialog()` ⇒ 「弹层仍在」
        // 那条变红；把 `set_plain_reason` 的调用删掉 ⇒ 就地原因那条变红。
        let rejected: ControlResponse<InterlockOpAck> = ControlResponse::rejected(
            "req-1",
            ControlCode::RejectedPrecondition,
            "触发源未复位 · 急停/门禁",
            Vec::new(),
            Some("audit-1".into()),
            1_789_047_727_000,
        );
        p4.show_result(&rejected).expect("show_result(失败)");
        assert!(
            p4.with_dialog(|d| d.is_alive()).unwrap_or(false),
            "失败 ⇒ **弹层不自动关闭**（原因常驻可读，IL11）"
        );
        assert_eq!(
            p4.reason_left_text().as_deref(),
            Some("触发源未复位 · 急停/门禁"),
            "就地红字显示**具体**原因（EDGE-12：不得静默失败）"
        );
        assert!(p4.reason_left_visible());
        assert_eq!(p4.toast_tone(), Some(ToastTone::Failure));
        // **① 无遮挡通道**（B2b-3 代码质量整改 ①）：评审 `PROBE-OCCL` 实测弹层面板底 ≈ y607、
        // 就地原因带在 y600–624 ⇒ 原因带**顶部约 7 px 被面板压住**，而拒绝时弹层**恰不关**
        // ⇒「就地可见」在最需要时**被削弱**。⇒ 同一份**具体原因**必须同时进 `Toast`
        // （`layer_top`，且**弹层之后创建** ⇒ 同图层内绘在其上，结构上无遮挡）。
        // 「改什么会让本条变红」：把失败分支的 `show_toast(.., &text)` 改回
        // `TEXT_TOAST_FAIL`（通用「操作失败」）⇒ 下面断言立刻变红（实测，见整改报告探针输出）。
        assert_eq!(
            p4.toast_text().as_deref(),
            Some("触发源未复位 · 急停/门禁"),
            "失败 Toast 必须携带**同一份具体原因**（无遮挡通道；IL23 ①）"
        );
        assert_ne!(
            p4.toast_text().as_deref(),
            Some(p4_interlock::TEXT_TOAST_FAIL),
            "**不得**只剩通用「操作失败」——那正是 ① 要修的形态"
        );
        assert_eq!(
            p4.toast_text().as_deref(),
            p4.reason_left_text().as_deref(),
            "两条通道（Toast / 就地红字）**同一份原因**，不两说"
        );
        // **③ 两槽共存时的宽度**（IL23 ②的**残余**面）：此刻 `two` 是 latch 态 ⇒ 右槽在显
        // （M1 专属原因）⇒ 左槽**必须收回 352 px**（否则两槽重叠，违 IL5）；长 `message` 在这
        // 一组合下仍是 `DOTS` 省略号（**如实登记**：此时唯一不截断的通道只有 Toast，且它自身
        // 文本槽 400 px 亦有上限）。
        disp.refr_now_for_test();
        assert_eq!(
            p4.reason_left_width(),
            352,
            "右槽在显 ⇒ 左槽收回 352 px（IL5 两槽不重叠；长 message 在此组合下仍截断 = IL23 残余）"
        );
        assert!(
            p4.take_refresh_request(),
            "RejectedPrecondition ⇒ 置「请求一次状态刷新」标志（IL12 / EDGE-19 的刷新语义）"
        );
        assert!(!p4.take_refresh_request(), "取走即清（只刷一次）");

        // ⑧⁺ **长 `message` 的不截断通道**（B2b-3 代码质量整改 ③ / IL23 ②）───────────────
        // 契约 `control.rs` 的 `message` 字段文档明写它是「失败时即 EDGE-10 / EDGE-12 要求的
        // **具体原因**」—— 而最需要具体时（长文本）恰恰被 352 px 槽的 `DOTS` 削掉。
        // **处置**：右槽不显时左槽**加宽到整幅 992 px**（`REASON_LEFT_FULL_W`）。
        // 本段用**实测文本自然宽**证明「确实放得下」，而不是只看槽宽数字。
        p4.set_section(&il(true, true, false)); // 非 latch ⇒ 右槽不显 ⇒ 左槽可独占整条
        disp.refr_now_for_test();
        assert_eq!(
            p4.reason_left_width(),
            Dimens::CONTENT_W,
            "右槽不显 ⇒ 左槽独占整条原因带（992 px；IL23 ②）—— \
             「改什么会让本条变红」：把 `refresh_actions` 的 `REASON_LEFT_FULL_W` 改回 `REASON_LEFT_W`"
        );
        // **实测文本自然宽**（口径见 [`measured_text_px`]）：**用生产字体**的字形宽度表量
        // （无 `.c` 的干净 clone / CI 上走**入库基线** `fonts/lv_font_metrics.txt`；两者皆缺
        // ⇒ **响亮失败**，不再静默跳过 —— B2c-1 代码质量评审 ⑤），而不是量离屏标签：
        // `noto-font` 默认未启用，离屏标签走降级字体（无 CJK 字形），量出来只有真机的 ~2/5。
        let long_msg = "审计服务连接超时，操作未执行：请检查审计服务后重试";
        {
            let natural = measured_text_px(long_msg, TextSlot::Body.px());
            assert!(
                natural > 352,
                "长文案自然宽必须 **> 352 px**（否则本段证明不了「352 槽会截断它」；实测 {natural} px）"
            );
            assert!(
                natural <= p4.reason_left_width(),
                "长文案自然宽 {natural} px 必须 ≤ 加宽后的左槽 {} px ⇒ **单行不截断**",
                p4.reason_left_width()
            );
        }
        let long_rejected: ControlResponse<InterlockOpAck> = ControlResponse::rejected(
            "req-long",
            ControlCode::RejectedPrecondition,
            long_msg,
            Vec::new(),
            Some("audit-long".into()),
            1_789_047_727_000,
        );
        p4.show_result(&long_rejected).expect("show_result(长 message)");
        assert_eq!(
            p4.reason_left_text().as_deref(),
            Some(long_msg),
            "长 message 就地**原样**上屏（无 ASCII 需改写 ⇒ `display_safe` 恒等）"
        );
        assert_eq!(
            p4.toast_text().as_deref(),
            Some(long_msg),
            "同一份长原因也在 Toast（无遮挡通道）里 —— 两条通道都给全"
        );
        assert_eq!(
            p4.reason_left_width(),
            Dimens::CONTENT_W,
            "长 message 期间左槽保持整幅（渲染期只改尺寸，不新建对象）"
        );

        // ⑧′ 结构化拒绝（直连 `InterlockApi` 路径）⇒ 逐变体具体文案（含**源名**）。
        p4.show_reject(&InterlockReject::SourcesNotReset {
            remaining: vec!["estop".into(), "flood".into()],
        });
        assert_eq!(
            p4.reason_left_text().as_deref(),
            Some("触发源未复位 · 急停/F?OOD"),
            "未登记名 `flood` 走 display_safe（**不伪造**中文名、不出豆腐块；IL6）"
        );
        assert_eq!(
            p4.last_reject(),
            Some(InterlockReject::SourcesNotReset {
                remaining: vec!["estop".into(), "flood".into()]
            })
        );

        // ⑧″ **保持时间倒计时**（IL14）：`tick(now)` 注入时钟推进，页面不读 `Instant::now()`。
        // 「改什么会让本条变红」：把 `tick` 里的倒计时分支删掉 ⇒ 后面两条「9 秒」变红。
        let t0 = Instant::now();
        p4.show_reject(&InterlockReject::HoldNotElapsed {
            need_secs: 30,
            remaining_secs: 12,
        });
        assert_eq!(p4.countdown_secs(), Some(12), "拒绝里的剩余秒数是**唯一**绝对量（IL14）");
        assert_eq!(
            p4.reason_left_text().as_deref(),
            Some("保持时间不足 · 还需 12 秒"),
            "按钮旁倒计时（UI §6.4「保持时间不足」行；琥珀 #FFB020）"
        );
        p4.tick(t0); // 首个 tick 取基准
        assert_eq!(p4.countdown_secs(), Some(12), "基准拍不递减");
        p4.tick(t0 + Duration::from_secs(3));
        assert_eq!(p4.countdown_secs(), Some(9), "按已过秒数递减");
        assert_eq!(
            p4.reason_left_text().as_deref(),
            Some("保持时间不足 · 还需 9 秒")
        );
        p4.tick(t0 + Duration::from_secs(99));
        assert_eq!(p4.countdown_secs(), Some(0), "下溢钳到 0（saturating，不 panic）");
        assert_eq!(
            p4.reason_left_text().as_deref(),
            Some("保持时间不足 · 还需 0 秒"),
            "到 0 **不自作主张**放行（是否可操作仍由后端判定）"
        );

        // ⑧″′ **陈旧倒计时必须被新帧清掉**（B2b-3 代码质量整改 M3 / IL28）──────────────
        // 评审 `PROBE-CD3`：倒计时归零后**跨帧常驻**「还需 0 秒」，且左槽优先级**高于**
        // `last_reject` ⇒ 会把新到的结构化拒绝原因**顶掉**（屏上是过期的倒计时，不是新原因）。
        // 判据 = 「新帧的**展示相关**字段变了」（`p4_interlock::section_display_eq`，忽略 `ts_ms`）。
        // 「改什么会让本条变红」：把 `Core::apply_section` 里的 `clear_countdown()` 调用删掉
        // ⇒ 下面「真·变帧后倒计时归零」那条立刻变红（实测，见整改报告探针输出）。
        // 先落一帧 S 作为「当前帧」，再重新武装倒计时（前面的 ⑧″ 已把它推到 0）。
        let mut s_frame = il(true, true, false);
        s_frame.sources = two.sources.clone();
        p4.set_section(&s_frame);
        p4.show_reject(&InterlockReject::HoldNotElapsed {
            need_secs: 30,
            remaining_secs: 5,
        });
        assert_eq!(p4.countdown_secs(), Some(5), "重新武装倒计时");
        // ① **仅 `ts_ms` 变** ⇒ 不清（否则 1 Hz 心跳每秒杀它，IL14 形同虚设）。
        let mut same = s_frame.clone();
        same.ts_ms = 123_456;
        p4.set_section(&same);
        assert_eq!(
            p4.countdown_secs(),
            Some(5),
            "**仅 `ts_ms` 变**的帧 ⇒ 不清倒计时（帧内容相同 = 没有新事实）"
        );
        assert_eq!(
            p4.reason_left_text().as_deref(),
            Some("保持时间不足 · 还需 5 秒")
        );
        // ② 真·变帧（`latched` 翻转）⇒ 旧基准失效 ⇒ 倒计时必须消失，让位给新原因。
        let mut flipped = s_frame.clone();
        flipped.latched = true;
        p4.set_section(&flipped);
        assert_eq!(
            p4.countdown_secs(),
            None,
            "展示态变了 ⇒ 陈旧倒计时必须被清（IL28：否则它会顶掉新原因）"
        );
        assert!(
            !p4.reason_left_visible(),
            "清掉后左槽落回「无原因」⇒ **隐藏**（不再常驻过期文案）—— \
             **可见性**才是 `PROBE-CD3` 的判据：隐藏标签的文本缓冲区仍留着旧串，\
             `reason_left_text()` 单独**不能**当「屏上有没有」的判据"
        );

        // ⑧‴ EDGE-19：提交时状态已变化（结构化路径的固定文案 + 刷新标志）。
        p4.show_conflict();
        assert!(p4.conflict_visible());
        assert_eq!(
            p4.reason_left_text().as_deref(),
            Some(p4_interlock::TEXT_CONFLICT),
            "「联锁状态已变化 · 请刷新后重试」（EDGE-19）"
        );
        assert!(p4.take_refresh_request(), "EDGE-19 ⇒ 自动触发一次状态刷新（取标志）");

        // ⑧⁗ EDGE-18：审计不可写（fail-closed ⇒ 操作未执行）。
        p4.show_audit_unavailable().expect("show_audit_unavailable");
        assert_eq!(
            p4.toast_text().as_deref(),
            Some(p4_interlock::TEXT_AUDIT_UNAVAILABLE),
            "Toast「审计不可用 · 操作未执行」（§8.3 EDGE-18）"
        );
        assert_eq!(p4.toast_tone(), Some(ToastTone::Failure));

        // ── ⑨ 取消 ⇒ **延迟关闭**（不得在 LVGL 事件回调内删弹层）──────────────────
        p4.with_dialog(|d| {
            d.cancel_button()
                .button()
                .obj()
                .send_event(EventCode::CLICKED)
        });
        assert!(
            p4.with_dialog(|d| d.is_alive()).unwrap_or(false),
            "取消回调内不得删弹层（`ConfirmDialog::close` 的要求）"
        );
        p4.tick(Instant::now());
        assert!(
            p4.with_dialog(|d| d.is_alive()).is_none(),
            "tick 里执行延迟关闭（用户可关闭后重试）"
        );

        // ── ⑩ 成功回执 ⇒ 用 `applied` **立即**刷新（不等下一帧）+ Toast 成功 ─────────
        // 「改什么会让本条变红」：把 `show_result` 成功分支里的 `apply_ack` 删掉 ⇒ 总态词
        // 仍停在「已联锁」⇒ 本条变红（F17.6 / IL-02「≤2 s 内更新」的立即路径）。
        let mut latched_view = il(true, true, true);
        latched_view.sources = two.sources.clone();
        p4.set_section(&latched_view);
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_LATCHED)
        );
        let ok_resp: ControlResponse<InterlockOpAck> = ControlResponse::ok(
            "req-2",
            Some(InterlockOpAck {
                latched: false,
                stopped: true,
            }),
            Some("audit-2".into()),
            1_789_047_727_000,
        );
        p4.show_result(&ok_resp).expect("show_result(成功)");
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNLATCHED),
            "回执 `applied.latched = false` ⇒ **立即**刷成「未联锁」（不等下一帧）"
        );
        assert!(
            !p4.restart_disabled(),
            "latch 已清 ⇒ M1 授权重启立即解除禁用（同一回执立即刷新）"
        );
        assert_eq!(
            p4.toast_text().as_deref(),
            Some(p4_interlock::TEXT_TOAST_RELEASED),
            "Toast「已释放联锁」（UI §3.6 全局行）"
        );
        assert_eq!(p4.toast_tone(), Some(ToastTone::Success));
        assert!(!p4.reason_left_visible(), "成功后清旧原因");

        // ── ⑪ M1 授权重启：意图走**独立回调**（载荷同样带观测源名）────────────────
        p4.restart_button()
            .button()
            .obj()
            .send_event(EventCode::CLICKED);
        assert_eq!(
            p4.with_dialog(|d| d.title().text()).flatten().as_deref(),
            Some(p4_interlock::TEXT_DIALOG_TITLE_ACK_M1)
        );
        assert_eq!(
            p4.with_dialog(|d| d.impact().text()).flatten().as_deref(),
            Some(p4_interlock::TEXT_IMPACT_ACK_M1)
        );
        assert!(got_m1.borrow().is_empty(), "未确认仍无意图");
        p4.with_dialog(|d| {
            d.confirm_button()
                .button()
                .obj()
                .send_event(EventCode::LONG_PRESSED)
        });
        assert_eq!(got_m1.borrow().len(), 1, "M1 授权确认 ⇒ 恰好一次意图");
        {
            let g = got_m1.borrow();
            assert_eq!(g[0].observed_sources.len(), 2);
            assert!(!g[0].observed_latched, "上一步成功后 latch 已清 ⇒ 观测为 false");
        }
        assert_eq!(got_rel.borrow().len(), 1, "释放槽不得被 M1 的确认触发");

        // ── ⑫ 提交中 ⇒ 两按钮 disabled（IL15：无就地文案 —— §3.6 无该行）─────────────
        p4.set_submitting(true);
        assert!(p4.release_disabled() && p4.restart_disabled());
        p4.set_submitting(false);
        assert!(!p4.release_disabled());

        // ── ⑬ 帧驱动路径：`render(&PageInput)` 走 `frame.interlock`（本页的**读路径**）──
        // 「改什么会让本条变红」：把 `render` 里 `input.frame` 的分支写错（如恒取
        // `InterlockSection::default()`）⇒ 下面两条立刻变红。
        let mut f = frame_healthy();
        f.interlock = il(true, true, false);
        p4.render(&PageInput::live(&f));
        assert!(
            p4.ever_available(),
            "有效帧经 `render` 注入 ⇒ `available` 被读到"
        );
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNLATCHED)
        );
        // `frame = None` ⇒ 该段取契约缺省 ⇒ **不可用**（不得沿用上一帧的「未联锁」）。
        p4.render(&PageInput::init());
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE_SHORT),
            "无帧 ⇒ 不可用（**不**沿用上一帧的「未联锁」—— 缺帧 ≠ 确知未联锁；IL29① 缩短形态）"
        );
        assert!(p4.release_disabled() && p4.restart_disabled());
        // 渲染后确有像素（装配 → 布局 → 像素全链）。
        let painted4 = sink.borrow().iter().filter(|b| **b != 0).count();
        assert!(painted4 > 10_000, "P4 渲染后 sink 中应有成片非背景像素（实际 {painted4}）");

        // ── ⑭ 区块级「冻结 / 数据过期」打标（EDGE-03 / EDGE-20；B3-2c）──────────────
        // 判据 = `pages::frame_mark`（与 P1 的通道条 / 数据过期角标**同源**）。
        // **改什么会让本条变红**：删掉 `render` 里的 `apply_frame_mark` 调用（或把可见性
        // 写成恒隐藏）⇒ 下列可见性断言全红；把 `Down` 时的帧清成缺省 ⇒
        // 「联锁总态仍为冻结帧」那条红（EDGE-03 明文要求**保留**最近有效帧）。
        let mut fm = frame_healthy();
        let mut sec = il(true, true, false); // 未联锁 + 有效
        sec.sources = two.sources.clone(); // 两个源（让"沿用冻结帧"的断言有区分度）
        fm.interlock = sec;
        p4.render(&PageInput::live(&fm));
        assert!(
            !p4.state_frozen_visible() && !p4.source_frozen_visible(),
            "实时 ⇒ 两张卡都无角标"
        );
        p4.render(&PageInput::down(Some(&fm)));
        // ⚠️ **U-73 / IL29② 订正（2026-09-25）**：联锁总态卡的角标**已恢复**，但取
        // **图标-only** 形态（28 px 落在标题与 latch 胶囊之间的空槽 —— 192 px 文字胶囊
        // 与 latch 胶囊在本卡宽内**必然重叠**，见 `pages::FROZEN_BADGE_W` 的几何论证）。
        // **图标-only ⇒ 结构上没有文字通道** ⇒ 标记身份由 `state_frozen_icon()`
        // （⚠ = 冻结 / `!` = 数据过期）承担，而**不是**恒 `None` 的 `text()`。
        //
        // **改什么会让本条变红**（本条是"冻结态"的**正半边**）：
        // ① 把 `Core::apply_frame_mark` 里对本卡的 `set_visible` 删掉（或写死 `false`）
        //    ⇒ 下面"可见"一条红；
        // ② 把 `pages::frozen_mark_icon` 的两支对调（⚠ ↔ `!`）⇒ 字形一条红。
        // **对偶**：同段的"实时 ⇒ 两张卡都无角标"（不可见半边）与下方"帧旧 ⇒ `!`"
        // 共同构成 **可见 ⇄ 不可见 × 冻结 ⇄ 过期** 的四格，任一格失效即红。
        assert!(
            p4.state_frozen_visible(),
            "通道断 + 保留帧 ⇒ 联锁总态卡打「冻结」（IL29② 恢复；与触发源卡同一拍、同一判据）"
        );
        assert_eq!(
            p4.state_frozen_icon().as_deref(),
            Some("⚠"),
            "图标-only 形态的**唯一**标记通道 = 字形（此处 = `冻结` ⇒ ⚠，与 `p2_config::ICON_WARN` 同值）"
        );
        assert!(p4.source_frozen_visible(), "触发源卡打「冻结」（同一拍、同一判据）");
        assert_eq!(p4.source_frozen_text().as_deref(), Some(pages::TEXT_FROZEN));
        // **打标 ≠ 清值**：联锁段的数据照常沿用冻结帧。
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNLATCHED),
            "通道断 ⇒ 联锁总态**沿用冻结帧**（不得回落「不可用」）"
        );
        assert_eq!(
            p4.sources_title_text().as_deref(),
            Some(p4_interlock::sources_title(2).as_str()),
            "触发源计数同样沿用冻结帧"
        );
        // 通道恢复 ⇒ 当拍撤除。
        p4.render(&PageInput::live(&fm));
        assert!(
            !p4.state_frozen_visible() && !p4.source_frozen_visible(),
            "通道恢复 ⇒ 角标当拍撤除"
        );
        // 通道正常但帧旧 ⇒ 同一判据改标记：触发源卡（文字胶囊）换**文案**，
        // 联锁总态卡（图标-only，IL29②）换**字形** —— 两条通道同为 `frame_mark` 单一真源。
        p4.render(&PageInput::new(
            Some(&fm),
            ChannelStatus::Connected,
            Freshness::Stale,
        ));
        assert_eq!(
            p4.source_frozen_text().as_deref(),
            Some(p1_status::TEXT_STALE),
            "通道通 + 帧旧 ⇒ 文案改「数据过期」（触发源卡）"
        );
        assert!(
            p4.state_frozen_visible(),
            "通道通 + 帧旧 ⇒ 联锁总态卡**同样**打标（EDGE-20；IL29② 恢复的正是这张）"
        );
        assert_eq!(
            p4.state_frozen_icon().as_deref(),
            Some("!"),
            "帧旧 ⇒ 字形改「数据过期」= `!`（与 `冻结` 的 ⚠ **成对**；两支对调即红）"
        );
        // 无帧 ⇒ 不打标（此时联锁段已是「不可用」，不是"冻结的旧值"）。
        p4.render(&PageInput::init());
        assert!(
            !p4.state_frozen_visible() && !p4.source_frozen_visible(),
            "无帧 ⇒ 无角标"
        );

        // ── ⑮ 写端点失败回执的**合账口径**（B3-2c 整改 · **阻塞项**）───────────────
        //
        // # 缺陷（评审实测）
        //
        // `absorb_console` 的成功路径先 `record_response(resp, epoch_ms)`，而
        // `ControlState::record_response` 在 `toast_text()` 为 `Some` 时 `push_toast`
        // （`toast_text()` 对**一切非 `Ok` 的 `ControlCode`** 与**任何非空 `message`** 都返回
        // `Some`）；紧接着 `apply_route` 把**同一份回执**送进 P4 `show_result` ⇒ **页面再弹
        // 一条**。两条同挂 `lv_layer_top()`、同坐标（`Dimens::TOAST_X/Y`）同文案
        // ⇒ 违 UI §7.2「同一时刻仅 1 条」。**生产高频**：P2 字段校验被拒 / P4 前置条件被拒。
        //
        // # 本用例的合账口径（**恰好一条**）
        //
        // | 上屏出口 | 期望 | 本段的判据 |
        // |----------|------|------------|
        // | 页面 Toast（P4 `show_result`） | **1 条**（可见） | `p4.toast_text()` 有值 |
        // | app 层 Toast（`App::sync_toast`） | **0 条**（隐藏） | 状态层为空 ∧ app 层文案**独取**状态层（`toast_view` ⇒ `ControlState::toast()`） |
        //
        // ⚠️ **局限（如实登记）**：app 层那一条**没有**在本段里直接观测（`App` 的构造需要
        // LVGL 会话 + 控制通道客户端，进程内起不了第二条 LVGL 线程）—— 本段证的是"**状态层
        // 为空**"，再由 `toast_view` 的定义（**唯一**输入 = `ControlState::toast()`）推出
        // "app 层那条必然隐藏"。**故"恰好一条"是"状态层空 + 页面侧有出口"这两半合起来的
        // 结论，不是任何单条断言能独立给出的。**
        //
        // **改什么会让本条变红**（**已实测**）：把 `app::record_receipt` 的写端点分支换回
        // `record_response` ⇒ 第 1 条断言红（状态层多出一条 ⇒ app 层与页面侧同拍两条）。
        {
            use crate::console::{ConsoleOutcome, OutcomeKind};
            use crate::control_route::{route, RawPayload, RouteDecision};
            use crate::state::ControlState;
            use mupc_display_proto::{ConsoleEndpoint, ControlCode, ControlResponse, InterlockOpAck};

            const T0: u64 = 1_000;
            const MSG: &str = "联锁状态已变化";
            let typed: ControlResponse<InterlockOpAck> = ControlResponse::rejected(
                "rid-1", ControlCode::RejectedPrecondition, MSG, Vec::new(), None, T0,
            );
            // 生产路径：`console.rs` 解出**裸 JSON 信封**（`RawPayload` = `Value`），
            // 由 `route()` 给出决策 + 类型化回执。本段照走这条链，不手搓 `RouteDecision`。
            let raw: ControlResponse<RawPayload> = ControlResponse::rejected(
                "rid-1", ControlCode::RejectedPrecondition, MSG, Vec::new(), None, T0,
            );
            let outcome = ConsoleOutcome {
                endpoint: ConsoleEndpoint::InterlockRelease,
                kind: OutcomeKind::Response(raw.clone()),
            };
            let decision = route(&outcome).expect("联锁写回执必须路由到 `InterlockResult`");
            assert_eq!(
                decision,
                RouteDecision::InterlockResult(typed.clone()),
                "两侧必须是**同一份回执**，否则本用例证的不是生产路径"
            );

            let mut st = ControlState::new();
            st.begin(ConsoleEndpoint::InterlockRelease, Some("rid-1"));
            crate::app::record_receipt(&mut st, &decision, &raw, T0);
            assert!(
                st.toast().is_none(),
                "写端点回执由 P4 `show_result` 上屏 ⇒ **状态层必须为空**（否则同拍两条 Toast）"
            );
            assert!(!st.is_busy(), "记账一个不少：在途照清");

            // 同一拍、同一份回执的**页面侧出口**（这就是"另一条出口"，也是唯一的可见条）。
            p4.show_result(&typed).expect("P4 show_result");
            assert_eq!(
                p4.toast_text().as_deref(),
                Some(MSG),
                "页面侧另有出口：P4 的 Toast 必须出现（上屏的就是这一条）"
            );
        }

        // ── ⑯ 渲染热路径的**双零增长**（B3-2c 整改 重要 2）───────────────────────
        // 评审实测：在 `P4InterlockPage::render` 里插一次 `add_style` ⇒ 整改前 **354 全绿**
        // ⇒「每拍只切可见性」此前**只有源码阅读作证**（同款缺口在 P6 上同样存在，见该页块）。
        // 本段连推 50 拍 `render(..)`，**三态轮转**（断 ⇒ 打「冻结」/ 通+帧旧 ⇒ 改「数据过期」/
        // 通+帧新 ⇒ 撤标）—— 三个分支都走，「误挂样式」最可能出在"改文案"那两支里。
        //
        // **改什么会让本条变红**（**已实测**）：在 `P4InterlockPage::render` 里插一次
        // `add_style` ⇒ 第 2 条红；插一次 `Obj::create` ⇒ 第 1 条红。
        //
        // ⚠️ **热身一轮的理由同 P6 段**（上一段把 `p4` 推到了 `init` 态 ⇒「占位样式 ⇒ 实际值
        // 样式」那一次**有界**的配对切换不计入本区间）。判据是**稳态**零增长。
        {
            use std::sync::atomic::Ordering;
            for _ in 0..3 {
                p4.render(&PageInput::down(Some(&fm)));
                p4.render(&PageInput::new(Some(&fm), ChannelStatus::Connected, Freshness::Stale));
                p4.render(&PageInput::live(&fm));
            }
            let mounts_before = crate::lvgl::obj::PROBE_MOUNTS.load(Ordering::SeqCst);
            let attaches_before = crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(Ordering::SeqCst);
            for i in 0..50u64 {
                let input = match i % 3 {
                    0 => PageInput::down(Some(&fm)),
                    1 => PageInput::new(Some(&fm), ChannelStatus::Connected, Freshness::Stale),
                    _ => PageInput::live(&fm),
                };
                p4.render(&input);
                // **逐拍复核**（理由同 P6 段：等推完 50 拍会先撞 LVGL 的单对象样式上限
                // `lv_obj_style.c:110`，那是挂死不是红）。
                assert_eq!(
                    crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(Ordering::SeqCst) - attaches_before,
                    0,
                    "P4 `render` 第 {i} 拍往对象上挂样式了（`Obj::add_style` 只增不删）"
                );
            }
            assert_eq!(
                crate::lvgl::obj::PROBE_MOUNTS.load(Ordering::SeqCst) - mounts_before,
                0,
                "P4 连推 50 拍 `render`（断 / 帧旧 / 实时三态轮转）不得新建任何 LVGL 对象 —— \
                 `render` 是 1 Hz 热路径，角标与行池都在构造期建好、运行期只切可见性 + 换文案"
            );
            assert_eq!(
                crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(Ordering::SeqCst) - attaches_before,
                0,
                "P4 连推 50 拍 `render` **也不得往对象上挂任何样式**：`Obj::add_style` 只增不删 \
                 ⇒「渲染路径每拍挂一条样式」= 样式表无界增长，而**对象数不变** —— \
                 评审的破坏性探针（在 `render` 里插 `add_style`）正是靠本条才抓得住"
            );
        }

        // ── ⑰ 四张冻结角标**同一构造点**（B3-2c 整改 重要 4）──────────────────────
        // 整改前：P6 走 `p6_system.rs::frozen_chip`、P4 在构造处**内联复制**了两次
        // `StatusChip::new(.., FROZEN_CHIP_W, ICON_WARN, TEXT_FROZEN, ChipSkin::WARNING)`，
        // 常量也双份；而 `p6_system.rs` 的注释却写"三处都由它统一口径"（`grep -rn frozen_chip
        // src/` 当时**只命中 P6**）—— **不实陈述**。现四张卡全部走
        // `pages/mod.rs::frozen_chip`（唯一构造点，尺寸 / 皮肤 / 图标 / 初始文案 / y 全在那一处）。
        //
        // 本段是**源码哨**（与 `app.rs` 的 `Toast::new(` 计数同款）：证明**这一行在源码里**。
        // 行为侧另一半 = **构造侧读回**（本页四条 + P6 块内四条）：尺寸与皮肤只由那个构造点
        // 给定、`render` 不覆盖 ⇒ 改 `frozen_chip` 的返回 ⇒ **两页共 8 条断言同时红**。
        //
        // **改什么会让本条变红**：把任一页的角标改回内联 `StatusChip::new(..)` ⇒ 第 1 / 2 条红；
        // 把文案 / 皮肤搬回任一页（即该页源码重新出现 `TEXT_FROZEN` 实参）⇒ 第 3 条红。
        disp.refr_now_for_test();
        // ⚠️ **U-73 / IL29② 订正（2026-09-25）**：联锁总态卡的角标**已恢复**，取**图标-only**
        // 形态 —— 488 宽的总览带卡内「标题(112) + 192 px 文字胶囊 + latch 胶囊(236)」
        // = 540 px > 卡内容区 454 px ⇒ **文字胶囊必然与 latch 胶囊重叠**；28 px 图标徽标
        // 落进标题与胶囊之间的空槽（112 < 174 < 202 < 218）⇒ 三件互不重叠（见 `IL29②`）。
        // ⇒ 四张打标件现分属 `pages/mod.rs` 的**同一构造点族的两支**：三张文字胶囊
        // （P4 触发源卡 + P6 ×2）经 `pages::frozen_chip`，一张图标徽标（P4 总态卡）经
        // `pages::frozen_icon_badge`。**本段两支都断**（尺寸 + 皮肤 + 源码哨），
        // 否则"总态卡角标由谁构造"就又回到无网状态。
        {
            let chip = p4.source_frozen_chip();
            assert_eq!(
                chip.size().0,
                pages::FROZEN_CHIP_W,
                "触发源卡角标宽 = `pages::FROZEN_CHIP_W`（与 P6 的两张**同一构造点**，不得各页自定）"
            );
            assert_eq!(
                chip.skin(),
                crate::ui::theme::ChipSkin::WARNING,
                "触发源卡角标皮肤 = §8.2 警示类（与 P6 的两张**同一构造点**）"
            );
        }
        {
            let badge = p4.state_frozen_badge();
            assert_eq!(
                badge.obj().size().0,
                pages::FROZEN_BADGE_W,
                "总态卡角标宽 = `pages::FROZEN_BADGE_W`(28) —— **图标-only** 形态（IL29②）；\
                 它落在标题与 latch 胶囊之间的空槽（192 px 文字胶囊会重叠）"
            );
        }
        {
            let p4_src = include_str!("pages/p4_interlock.rs");
            let p6_src = include_str!("pages/p6_system.rs");
            // 判据用**带路径前缀**的形式：本文件里的读回口 `*_frozen_chip(&self)` 也含
            // `frozen_chip(` 子串，裸串计数会把它们算进去（实测：裸串 = 4），故锚定调用点全路径。
            // ⚠️ **U-73 / IL29② 订正**：联锁总态卡的角标**已恢复**但换形态 ⇒ P4 的文字胶囊
            // 只剩**触发源卡**一张（`frozen_chip(` 计数 2 → 1），另有**一张图标徽标**经
            // `pages::frozen_icon_badge`（下方单独计数）。P6 侧不变（两张，仍文字胶囊）。
            assert_eq!(
                p4_src.matches("pages::frozen_chip(").count(),
                1,
                "P4 现只**触发源卡**经 `pages::frozen_chip`（总态卡因 488 宽卡内放不下文字胶囊\
                 而改走 `frozen_icon_badge`，见 IL29②；内联构造 = 双份口径，仍禁用）。\
                 若计数变 2，请同时确认 IL29② 的重叠问题已解"
            );
            assert_eq!(
                p4_src.matches("pages::frozen_icon_badge(").count(),
                1,
                "总态卡的**图标-only** 角标必须经 `pages::frozen_icon_badge`（同一模块的构造点族）——\
                 页面里内联建（容器 + 图标两件）即口径分裂，且本段/⑭ 的读回断言全部失锚"
            );
            assert_eq!(
                p6_src.matches("pages::frozen_chip(").count(),
                2,
                "P6 的两张卡同理"
            );
            assert!(
                !p4_src.contains("TEXT_FROZEN,") && !p6_src.contains("TEXT_FROZEN,"),
                "冻结角标的文案只许写在 `pages::frozen_chip` 一处 —— 任一页里再出现\
                 `TEXT_FROZEN` **作实参**（故判据带尾逗号，与文档链接 `[`…TEXT_FROZEN`]` 区分）\
                 即口径分裂"
            );
            assert!(
                p4_src.contains("pages::frozen_chip(") && p6_src.contains("pages::frozen_chip("),
                "两页都必须显式指向**同一个**构造点（不是各自的同名私有函数）"
            );
        }

        // ═══ ⑯ U-73（T21c-1）：P4 消防区 + 下钻视图的**离屏渲染**（T-18）+ 版面常量（T-25）═══
        //
        // 判据（设计 §15.8）：T-18 = 「P4 各段渲染非空；关键区域语义色；降级态；中文墨量密度；
        // 导出 PNG」；T-25 = 「总览带两卡间隙 == GAP_MIN(16)、总览带↔滚动区 == GAP_GROUP(16)、
        // 滚动区↔操作条 == GAP_SECTION(24)、火警卡↔危险按钮 ≥ GAP_DANGER(48)；`ui/**` 零裸尺寸 /
        // 零裸色值（承既有两张网）」。PNG 导出按本仓**已登记的偏离**（评审 C-④）取 PPM ——
        // 本用例只断言**像素非空 + 中文墨量密度**（导出链路由 `screen.rs::write_ppm` 与进程级
        // `offscreen_smoke.rs` 承担）。
        {
            // ── 版面常量（T-25；几何从**实测 `coords()`** 读出，不看常量名）──
            let band_c = p4.band_obj().coords();
            let state_c = p4.state_card_obj().coords();
            let fire_c = p4.fire_card_obj().coords();
            let sc_c = p4.scroll_obj().coords();
            let bar_c2 = p4.action_bar_obj().coords();
            assert_eq!(
                state_c.x2 - state_c.x1 + 1,
                Dimens::BAND_CARD_W,
                "联锁总态卡宽 = `Dimens::BAND_CARD_W`（488；UI §6.4.1）"
            );
            // ── S-1（T21c-1-r1 复核整改）：卡头三件**互不重叠** ──────────────────────
            //
            // IL29② 的产品裁定「角标取图标-only 形态」**成立的全部理由就是"不重叠"** ——
            // 但此前该不变量只写在注释 / 文档里：把 `pages::FROZEN_BADGE_W` 从 28 改回 192
            // （即**复现** IL29② 要避免的「标题 + 角标 + latch 胶囊」三件重叠）**测试全绿**
            // （复核探针 P-F 实证）。本条把它钉住：三件同处卡头一行 ⇒ 判据 = **横向分离**
            // （`coords()` 是闭区间，`x2` 为最后一个像素列 ⇒ 不重叠 ⟺ 前一件 `x2 <` 后一件 `x1`）。
            //
            // **几何口径（两路子断言，互相独立）**：
            // ① **角标 / latch 胶囊**：读**实测 `coords()`** —— 二者都是显式 `set_size` 的
            //    容器（`FROZEN_BADGE_W` / `Dimens::CARD_STATUS_W`）⇒ 布局趟后可靠。
            // ② **标题**：本卡标题是这块卡上**唯一没有显式宽**的标签（`text_label` 之后不
            //    `set_size`）⇒ 按本文件 P1 的 **I1** 段既有登记「无显式宽度时 `coords()` 返回的
            //    是**陈旧的 obj 盒**而非绘出的文本外延」；且本用例默认构建**未开 `noto-font`**
            //    （`Font::of` 走 `fallback()` = montserrat_14）⇒ 那个盒量到的是 **4 个占位框**
            //    （实测 40×16）而**不是** CJK 文本的 112 px。故标题右缘取「**实测左缘**
            //    `coords().x1` ＋ **生产字体 `adv_w` 实测宽**」= `title_right`
            //    （`measured_text_px`，与下方 D-2 列宽判据同一口径；求和为上界、不计 kerning）。
            //    两路**都断**：① 钉住布局趟里的 obj 盒，② 钉住真机上真正画出来的文本外延。
            //
            // **为什么不是"自比式"**：三件的坐标全部取自**布局趟落定后的实测值**，标题宽取自
            // **字体资产**的 `adv_w`；全式**不含**任何"把 `STATE_FROZEN_X` / `LATCH_CHIP_X`
            // 再算一遍再与常量比"的项 —— 那种写法在常量本身写错时**恒绿**、零判别力。
            // 这里只有「实测右缘 < 实测左缘」这一条关系式，外加一条「实测缝 == `GAP_MIN`」。
            //
            // **改什么会让本条变红**：把 `pages::FROZEN_BADGE_W` 由 28 加宽（如复原 192）
            // ⇒ `STATE_FROZEN_X` 左移（218 − W − 16 = 10）、徽标同时撞上标题 ⇒ 第 ①②③ 条红
            // （**这正是 IL29② 要避免的重叠**：192 的文字胶囊更会直接压住 latch 胶囊）；
            // 把标题文案改长（越过标题与角标之间的空槽）⇒ 第 ②③ 条红；
            // 把 latch 胶囊左移 ⇒ 第 ②③④ 条红；改角标落位的缝 ⇒ 第 ⑤ 条红。
            let title_c = p4.state_title_obj().coords();
            let badge_c = p4.state_frozen_badge().obj().coords();
            let latch_c = p4.state_latch_chip_obj().coords();
            let title_px =
                measured_text_px(p4_interlock::TEXT_CARD_STATE, TextSlot::SectionTitle.px());
            let title_right = title_c.x1 + title_px - 1;
            assert!(
                title_c.x2 < badge_c.x1,
                "卡头标题的**布局趟 obj 盒**右缘（实测 x2 = {}）必须 < 冻结角标左缘（实测 x1 = {}）\
                 —— 否则「标题 ↔ 角标」重叠（IL29②：取 28 px 图标-only 形态的全部理由）",
                title_c.x2,
                badge_c.x1
            );
            assert!(
                title_right < badge_c.x1,
                "卡头标题的**真机文本外延**右缘（实测左缘 {} + `TEXT_CARD_STATE` 在 {} px 档的 \
                 `adv_w` 和 {} − 1 = {}）必须 < 冻结角标左缘（实测 x1 = {}）—— 这是 IL29② 的\
                 几何论证直接依赖的那条不变量（标题 112 之后才轮到 174 的角标槽）",
                title_c.x1,
                TextSlot::SectionTitle.px(),
                title_px,
                title_right,
                badge_c.x1
            );
            assert!(
                badge_c.x2 < latch_c.x1,
                "冻结角标右缘（实测 x2 = {}）必须 < latch 胶囊左缘（实测 x1 = {}）—— \
                 否则「角标 ↔ 胶囊」重叠：192 px 文字胶囊撞的正是这里（IL29②）",
                badge_c.x2,
                latch_c.x1
            );
            assert!(
                title_right < latch_c.x1,
                "标题真机外延右缘（实测 {title_right}）必须 < latch 胶囊左缘（实测 x1 = {}）—— \
                 卡头三件两两不重叠的传递闭合（标题与胶囊之间还夹着角标槽）",
                latch_c.x1
            );
            assert_eq!(
                latch_c.x1 - badge_c.x2 - 1,
                Dimens::GAP_MIN,
                "冻结角标 ↔ latch 胶囊的缝必须 == `GAP_MIN`(16)（角标 x = 胶囊左缘 − 角标宽 − 缝，\
                 见 `p4_interlock.rs::STATE_FROZEN_X`；`coords()` 闭区间 ⇒ 缝 = x1 − x2 − 1）"
            );
            assert_eq!(
                fire_c.x1 - state_c.x2 - 1,
                Dimens::GAP_MIN,
                "总览带两卡间隙必须 == `GAP_MIN`(16)（T-25）"
            );
            assert_eq!(
                fire_c.y1, band_c.y1,
                "两卡同排同高（UI §6.4.1「两卡并排」）"
            );
            assert_eq!(
                sc_c.y1 - (band_c.y1 + band_c.y2 - band_c.y1 + 1),
                Dimens::GAP_GROUP,
                "总览带 ↔ 滚动区必须 == `GAP_GROUP`(16)（T-25）"
            );
            assert_eq!(
                bar_c2.y1 - (sc_c.y1 + sc_c.y2 - sc_c.y1 + 1),
                Dimens::GAP_SECTION,
                "滚动区 ↔ 固定操作条必须 == `GAP_SECTION`(24)（T-25；那 24 px 就是就地原因带）"
            );
            // 火警等级卡 ↔ 危险按钮（**纵向**口径；UI §6.4.1 明写"远超 48 px"）
            let fire_bottom = fire_c.y2 + 1;
            let danger_top = p4.release_button().button().obj().coords().y1;
            assert!(
                danger_top - fire_bottom >= Dimens::GAP_DANGER,
                "火警等级卡 ↔ 危险按钮必须 ≥ `GAP_DANGER`(48)：实测 {} px",
                danger_top - fire_bottom
            );
            // A4「查看明细」按钮 ≥ TOUCH_MIN×TOUCH_MIN
            let det_c = p4.a4_detail_button().button().obj().coords();
            assert!(
                det_c.x2 - det_c.x1 + 1 >= Dimens::TOUCH_CRITICAL
                    && det_c.y2 - det_c.y1 + 1 >= Dimens::TOUCH_MIN,
                "「查看明细」= 200×48 ≥ 键盘/关键操作目标（UI §6.4.1）"
            );
            // 下钻表头「状态」列**两个位名都完整可读**（W-2 回归网；T21c-1-r1 订正）
            //
            // 判据 = 用**生产字体的 `adv_w`**（口径同 `measured_text_px`；求和为上界、不计
            // kerning）量**表头那一串**，必须 ≤ 该列宽 `DRILL_COL_STATUS`（328）。旧形态
            // `状态 · 报警总状态 / 故障总状态` 实测 **342 px** ⇒ 第 2 个位名被 `DOTS` 截成
            // 「故障总…」；现取紧凑式 `报警总状态/故障总状态`（**249 px**）。
            //
            // **改什么会让本条变红**：把列名或分隔空格加回表头串（如 `… · … / …`）⇒
            // 第 1 条红（342 > 328）；把任一 `BIT_*` 常量改长 / 改短到放不下 ⇒ 第 1 条红；
            // 把两个位名之一从表头串里删掉 ⇒ 第 2 条红。
            let head_status = p4_interlock::drill_head_texts()[2].clone();
            let head_status_w = measured_text_px(&head_status, TextSlot::Weak.px());
            assert!(
                head_status_w <= p4_interlock::DRILL_COL_STATUS,
                "表头「状态」列串 `{head_status}` 实测 {head_status_w} px > 列宽 {} px ⇒ \
                 第 2 个位名会被 `DOTS` 截断（W-2）",
                p4_interlock::DRILL_COL_STATUS
            );
            assert!(
                head_status.contains(ui_text::BIT_ALARM_TOTAL)
                    && head_status.contains(ui_text::BIT_FAULT_TOTAL),
                "bit12 / bit14 两个位名**逐字**都在表头串内 ⇒ 都完整可读（实测 `{head_status}`）"
            );

            // ── 数据：catalog + 外设段 + 明细页 ──
            let (sec, cat) = u73_fire_fixture(3, 3, Some(true), 2.0);
            p4.set_catalog(&cat);
            p4.set_periph(&sec);
            disp.refr_now_for_test();

            // T-18：火警等级卡（三重冗余：文案 + 图标 + 语义色档）
            assert_eq!(
                p4.fire_value_text().as_deref(),
                Some("二级火警"),
                "枚举文案来自 catalog 的 `enum_labels`（`fire_sys_6`）"
            );
            assert_eq!(p4.fire_icon_text().as_deref(), Some("⚠"), "图标通道");
            assert_eq!(
                p4.fire_slot(),
                p4_interlock::fire_level_slot(2.0),
                "语义色档由值映射（与 p4 的纯函数同源）"
            );
            assert_eq!(
                p4_interlock::fire_slot_color(p4.fire_slot()),
                Palette::DANGER
            );
            // A1：已定义位写字、未定义位显「未定义位 n」
            let a1_14 = p4.a1_row_text(14).unwrap_or_default();
            assert!(
                a1_14.contains("主电故障") && a1_14.contains("活跃"),
                "A1 第 15 行（bit14 主电故障）= 位名 + 活跃文字，实测 `{a1_14}`"
            );
            let a1_15 = p4.a1_row_text(15).unwrap_or_default();
            assert!(
                a1_15.contains("未定义位") && a1_15.contains("15"),
                "A1 第 16 行（bit15）= 「未定义位 15」（**不猜语义**），实测 `{a1_15}`"
            );
            // A1 **行数断言**（§15.4「逐位 16 行」；T21c-1-r1 / 评审 W-3②）
            //
            // 位行恒 16、**恒从 bit0 起**（通告不占位行）⇒ 两个方向都要锁：
            // ① 池大小恰 16（`a1_row_count()`）：多一行 = "逐位 16 行"被破坏；少一行 = 末位不可读；
            // ② 第 0 行是 bit0 的语义（`未定义位 0`），**不是**通告。
            //
            // **改什么会让本条变红**：`A1_ROWS` 改成 15/17 ⇒ 第 1 条红；把通告写回第 0 行
            // （即 `refresh_fire` 里再出现 `(0, Some(n))` 那种分派）⇒ 第 2 条红。
            assert_eq!(
                p4.a1_row_count(),
                p4_interlock::A1_ROWS,
                "A1 位行数 = `A1_ROWS`(16) —— §15.4「逐位 16 行」"
            );
            let a1_0 = p4.a1_row_text(0).unwrap_or_default();
            assert!(
                a1_0.contains("未定义位") && a1_0.contains('0'),
                "A1 第 1 行 = **bit0 的语义**（此夹具下 bit0 未定义 ⇒ 「未定义位 0」）—— \
                 通告**不得**占用位行（T21c-1-r1；实测 `{a1_0}`）"
            );
            assert!(
                !p4.a1_notice_visible(),
                "真实数据 ⇒ A1 段顶通告行**不显**（恒留白 ⇒ 位行 y 稳定）"
            );
            // A2：值 + 单位
            assert_eq!(p4.a2_text().as_deref(), Some("250 kPa"));
            // A3：三段（含「预留」不显 0/1）
            let a3_0 = p4.a3_row_text(0).unwrap_or_default();
            assert!(
                a3_0.contains("干接点触发") && a3_0.contains("复合触发") && a3_0.contains("预留"),
                "A3 第 1 行三段齐全（含「预留」），实测 `{a3_0}`"
            );
            assert!(
                !a3_0.contains("预留 活跃") && !a3_0.contains("预留 非活跃"),
                "「预留」**不得**显 0/1 语义"
            );
            // A4 汇总（**§15.4 逐字口径**：登记 3 只 / 可读 3 只 / 报警 1 / 故障 1 / 离线 0）
            //
            // ⚠️ **T21c-1-r1 订正（评审 W-3①）**：本节曾以**单个空格**分隔五元组，与 §15.4
            // 原文（含 `/`）不符 ⇒ 现按合同补回。断言同时锁**分隔符个数**（4 个 ` / `）与
            // **逐字全文**（顺序 + 量词 + 数字）—— 只判 `contains` 抓不到"分隔符退化成空格"。
            //
            // **改什么会让本条变红**：把 `.join(" / ")` 换回空格分隔 ⇒ 第 1 条红；
            // 调换五元组顺序 / 漏掉量词 `只` ⇒ 第 2 条红。
            let a4 = p4.a4_summary_text().unwrap_or_default();
            assert_eq!(
                a4.matches(" / ").count(),
                4,
                "A4 汇总行以 ` / ` 分隔**五**元组 ⇒ 恰 4 个分隔符（§15.4 原文），实测 `{a4}`"
            );
            assert_eq!(
                a4,
                format!(
                    "{} 3 {} / {} 3 {} / {} 1 / {} 1 / {} 0",
                    ui_text::REGISTERED,
                    ui_text::COUNT_UNIT,
                    ui_text::READABLE,
                    ui_text::COUNT_UNIT,
                    ui_text::COUNT_ALARM,
                    ui_text::COUNT_FAULT,
                    ui_text::OFFLINE,
                ),
                "A4 汇总行**逐字** = §15.4 口径（登记 N 只 / 可读 M 只 / 报警 x / 故障 y / 离线 z）"
            );
            let sum = p4.fire_summary();
            assert_eq!(sum.registered, Some(3));
            assert_eq!(sum.readable, 3);
            assert_eq!(sum.alarm, 1, "第 1 只状态字 bit12 活跃");
            assert_eq!(sum.fault, 1);
            assert_eq!(sum.offline, 0);
            assert!(!p4.a4_mismatch_visible(), "N == M ⇒ 无「不一致」提示");

            // 降级：值班侧数据不可用 ⇒ 段级文案（「外设数据不可用」），**不是 0 / 正常**
            p4.set_periph(&mupc_display_proto::PeripheralsSection::default());
            assert_eq!(p4.a2_text().as_deref(), Some(ui_text::PERIPH_UNAVAILABLE));
            // A1：通告落**段顶通告行**（**不静默**），而**位行仍是 bit0..bit15**（W-3② 订正）
            assert!(
                p4.a1_notice_visible(),
                "段级降级 ⇒ A1 段顶通告行**可见**（不得静默）"
            );
            assert_eq!(
                p4.a1_notice_text().as_deref(),
                Some(ui_text::PERIPH_UNAVAILABLE),
                "通告行文案 = 段级文案（与 A2 / A4 同一真源）"
            );
            assert_eq!(
                p4.a1_row_count(),
                p4_interlock::A1_ROWS,
                "降级态下**位行数不变**（恒 16）—— 通告另占一行，不挤掉 bit0"
            );
            let a1_0_down = p4.a1_row_text(0).unwrap_or_default();
            assert!(
                a1_0_down.contains("未定义位") && a1_0_down.contains('0'),
                "降级态下第 1 行**仍是 bit0 的语义**（此夹具 bit0 未定义 ⇒「未定义位 0」）—— \
                 这是「通告不占位行」的直接判据；实测 `{a1_0_down}`"
            );
            assert_eq!(
                p4.a4_summary_text().as_deref(),
                Some(ui_text::PERIPH_UNAVAILABLE)
            );
            // 恢复
            p4.set_periph(&sec);
            disp.refr_now_for_test();

            // ── N != M ⇒ **显式提示**（不得静默裁剪，F21.4 / EX-12）──
            let (sec_mm, cat_mm) = u73_fire_fixture(5, 3, Some(true), 2.0);
            p4.set_catalog(&cat_mm);
            p4.set_periph(&sec_mm);
            assert!(
                p4.a4_mismatch_visible(),
                "登记 5 ≠ 可读 3 ⇒ 必须显式提示不一致（EX-12）"
            );
            assert_eq!(
                p4.a4_mismatch_text().as_deref(),
                Some(ui_text::REGISTERED_READABLE_MISMATCH)
            );
            // ── 钢瓶未配置 ⇒ 「未配置」且**不含 0 kPa**（EDGE-23 / EX-11）──
            let (sec_cyl, cat_cyl) = u73_fire_fixture(3, 3, Some(false), 0.0);
            p4.set_catalog(&cat_cyl);
            p4.set_periph(&sec_cyl);
            assert_eq!(p4.a2_text().as_deref(), Some(ui_text::NOT_CONFIGURED));
            assert!(!p4.a2_text().unwrap_or_default().contains("0"));
            // ── 表外枚举值 ⇒ 「未知」+ 中性色档 ──
            let (sec_unk, cat_unk) = u73_fire_fixture(3, 3, Some(true), 9.0);
            p4.set_catalog(&cat_unk);
            p4.set_periph(&sec_unk);
            assert_eq!(p4.fire_value_text().as_deref(), Some(ui_text::ENUM_UNKNOWN));
            assert_eq!(
                p4_interlock::fire_slot_color(p4.fire_slot()),
                Palette::TEXT_WEAK,
                "「未知」= 中性色（**绝不用绿**）"
            );

            // ── 下钻视图：进入 → 渲染明细 → 收起（**不改 current_page**）──
            assert!(!p4.drill_open(), "初始态不在下钻");
            p4.set_fire_page_size(20);
            p4.show_detail();
            assert!(p4.drill_open(), "「查看明细」⇒ 进下钻");
            assert_eq!(p4.fire_page_req(), 1, "首次请求第 1 页");
            let page = u73_fire_page(3, 20, 1, true, 3);
            p4.set_fire_page(&page);
            disp.refr_now_for_test();
            // ── 新增只读控件 ↔ 危险按钮（§15.4 相邻关系表：「下钻『收起』↔ 危险按钮
            //    ≥ `GAP_DANGER`(48)」，与 T-25 同族；本断言取**实测**值）──
            //
            // **实测口径（T21c-1-r1 订正）**：收起按钮在下钻视图顶部（滚动区首行，
            // 页内 y = 视口顶 186），高 `TOUCH_MIN`(48) ⇒ 底 = 234；危险按钮 top = 556
            // ⇒ **322 px**（≥ 48 ✓）。⚠️ 设计 §15.4 该行写「纵向 **316 px**」是按**线框**
            // 的 y260–308 复算的，实现的视口起点是页内 y186（总览带 162 + `GAP_GROUP` 16 +
            // 内容区上内边距 8）⇒ 差 6 px，两者都 ≫48。**自报「392 px」复现不出**（参照点
            // 混用），本断言以**实测 322** 为准并锁死。
            //
            // **改什么会让本条变红**：把下钻顶部条移出滚动区顶部 / 改其高 ⇒ 本条红。
            let collapse_c = p4.drill_collapse_button().button().obj().coords();
            let danger_top2 = p4.release_button().button().obj().coords().y1;
            assert_eq!(
                danger_top2 - (collapse_c.y2 + 1),
                322,
                "「收起」↔ 危险按钮实测 **322 px**（≥ `GAP_DANGER`(48) ✓；设计 §15.4 的 \
                 316 px 系线框口径、自报 392 px 不可复现 ⇒ 以本实测值为准）"
            );
            assert!(!p4.drill_fail_visible(), "有数据 ⇒ 不显失败态");
            assert!(p4.drill_row_visible(0) && p4.drill_row_visible(2));
            assert!(!p4.drill_row_visible(3), "第 4 行无数据 ⇒ 隐藏（不补空行）");
            let title = p4.drill_title_text().unwrap_or_default();
            assert!(
                title.contains('1') && title.contains('1'),
                "标题「探测器 · 第 1 / 1 页」，实测 `{title}`"
            );
            assert_eq!(
                p4.drill_head_text(0).as_deref(),
                Some(ui_text::COL_SEQ),
                "明细表第 1 列 = 序号"
            );
            assert!(p4.drill_head_text(1).as_deref() == Some("地址"));
            let head2 = p4.drill_head_text(2).unwrap_or_default();
            assert!(
                head2.contains(ui_text::BIT_ALARM_TOTAL)
                    && head2.contains(ui_text::BIT_FAULT_TOTAL),
                "状态列的表头含两个位名，实测 `{head2}`"
            );
            // 第 1 行：序号 / 地址 / 状态（bit12+bit14 活跃）/ 烟雾（拆解）/ 温度
            assert_eq!(p4.drill_cell_text(0, 0).as_deref(), Some("1"));
            assert_eq!(p4.drill_cell_text(0, 1).as_deref(), Some("1"));
            let st12 = p4.drill_cell_text(0, 2).unwrap_or_default();
            assert!(
                st12.contains("活跃"),
                "状态格 = 圆点 + 活跃（双通道），实测 `{st12}`"
            );
            assert_eq!(
                p4.drill_cell_text(0, 4).as_deref(),
                Some("1.0"),
                "烟雾 = 高字节 0x0A × 0.1（**展示层拆解**）"
            );
            assert_eq!(
                p4.drill_cell_text(0, 5).as_deref(),
                Some("\u{2212}12"),
                "温度 = 低字节 0x2B − 55 = −12 ℃（负号是 U+2212）"
            );
            // 分页：末页 ⇒ 「下一页」禁用；首页 ⇒ 「上一页」禁用
            assert!(p4.drill_prev_disabled(), "首页 ⇒ 上一页 disabled");
            assert!(
                p4.drill_next_disabled(),
                "has_more = false ⇒ 下一页 disabled"
            );
            // 收起 ⇒ 回列表（`current_page` 结构性不变：本页代码零路由 API，见 T-20）
            p4.collapse_detail();
            assert!(!p4.drill_open(), "「收起」⇒ 退出下钻（F11.3）");
            // 失败态：端点非 2xx ⇒ 屏上只显本地固定文案 + 重试（R-4）
            p4.show_detail();
            p4.set_fire_page_failed();
            disp.refr_now_for_test();
            assert_eq!(
                p4.drill_fail_text().as_deref(),
                Some(ui_text::DETAIL_UNAVAILABLE)
            );
            assert!(p4.drill_fail_visible(), "失败态可见（**不静默**）");
            assert!(p4.drill_retry_visible(), "失败态给「重试」出路");
            // 消防源不可用 ⇒ 另一条文案（与「明细不可用」互异）
            let bad = u73_fire_page(0, 20, 1, false, 0);
            p4.set_fire_page(&bad);
            assert_eq!(
                p4.drill_fail_text().as_deref(),
                Some(ui_text::FIRE_SOURCE_UNAVAILABLE)
            );
            p4.collapse_detail();
            p4.set_fire_page(&page);
            disp.refr_now_for_test();

            // ── 中文墨量密度（T-18；§11.1 判据：成片非背景像素）──
            let painted = sink.borrow().iter().filter(|b| **b != 0).count();
            assert!(
                painted > 10_000,
                "P4 消防区渲染后 sink 应有成片非背景像素（实际 {painted}）—— \
                 总览带 + 四组 + 下钻共 200+ 个文本件，墨量必须显著"
            );

            // ── T21c-3-r1：顶部「名称表可能过期」提示条 + 「重试」（设计 §15.3.1 第 2 / 3 句）──
            //
            // 判据（全部取**实测 `coords()`**）：① 默认**不显**且滚动视口 = 既有版面；
            // ② `set_catalog_stale(true)` ⇒ 提示条可见 + 文案 = `ui_text::CATALOG_STALE`、
            //    落在滚动区**顶部**（底缘不动 ⇒ 与操作条的 24 px 缝不变）、滚动视口等量变矮；
            // ③ 「重试」= `TOUCH_MIN`(48)×48、与提示条净距 `GAP_MIN`(16)、**点击投出意图**；
            // ④ `set_catalog_stale(false)` ⇒ 逐像素复原（"无 catalog 时的既有布局"不破）。
            //
            // **改什么会让本条变红**：去掉 `Core::set_catalog_stale` 里的 `set_hidden` / 让位两行
            // ⇒ ①②④ 红；把按钮尺寸改成 <48 或挪掉 `GAP_MIN` ⇒ ③ 红；删掉按钮的 `on_clicked`
            // 接线（`fire_catalog_retry`）⇒ ③ 的意图计数恒 0 ⇒ 红。
            {
                let hits = Rc::new(Cell::new(0u32));
                {
                    let h = Rc::clone(&hits);
                    p4.set_on_catalog_retry(move || h.set(h.get() + 1));
                }
                // ① 默认态 = 既有版面（滚动视口 y / 高与 T-25 判的那两个几何同源）
                let sc_base = p4.scroll_obj().coords();
                let band_c3 = p4.band_obj().coords();
                assert!(
                    !p4.catalog_stale_visible(),
                    "默认**不显**提示条（常驻占位会让正常态也少一截视口）"
                );
                assert_eq!(
                    sc_base.y1 - (band_c3.y2 + 1),
                    Dimens::GAP_GROUP,
                    "默认态：总览带 ↔ 滚动区 = `GAP_GROUP`（与 T-25 同一条几何）"
                );
                // ② stale ⇒ 提示条在滚动区顶部，视口让位（**底缘不动**）
                p4.set_catalog_stale(true);
                disp.refr_now_for_test();
                assert!(p4.catalog_stale_visible(), "stale ⇒ 提示条可见");
                assert_eq!(
                    p4.catalog_stale_text().as_deref(),
                    Some(ui_text::CATALOG_STALE),
                    "提示条文案必须取契约常量（不得自造串）"
                );
                let ban_c = p4.catalog_stale_banner_obj().coords();
                let sc_stale = p4.scroll_obj().coords();
                let bar_c4 = p4.action_bar_obj().coords();
                assert_eq!(
                    ban_c.y1, sc_base.y1,
                    "提示条落在**滚动区原位**（= 顶部），不是别处"
                );
                assert_eq!(
                    ban_c.y2 - ban_c.y1 + 1,
                    Dimens::BANNER_H,
                    "提示条高 = `Dimens::BANNER_H`(56)（UI §5.1 #12）"
                );
                assert_eq!(
                    sc_stale.y1 - (ban_c.y2 + 1),
                    Dimens::GAP_GROUP,
                    "提示条 ↔ 滚动区 = `GAP_GROUP`(16)"
                );
                assert_eq!(
                    sc_stale.y1 - sc_base.y1,
                    Dimens::BANNER_H + Dimens::GAP_GROUP,
                    "stale ⇒ 滚动视口**下移** 72 px（提示条 + 缝）"
                );
                assert_eq!(
                    sc_stale.y2, sc_base.y2,
                    "**底缘不动** ⇒ 与固定操作条的缝不变（避免遮住操作条 / 越出页根）"
                );
                assert_eq!(
                    bar_c4.y1 - (sc_stale.y2 + 1),
                    Dimens::GAP_SECTION,
                    "stale 态下「滚动区 ↔ 操作条」仍 = `GAP_SECTION`(24)（T-25 的同一几何）"
                );
                // ③ 「重试」：48×48 + 与提示条净距 16 + 点击投意图
                let rt_c = p4.catalog_retry_button().button().obj().coords();
                assert_eq!(
                    (rt_c.x2 - rt_c.x1 + 1, rt_c.y2 - rt_c.y1 + 1),
                    (Dimens::TOUCH_MIN, Dimens::TOUCH_MIN),
                    "「重试」必须 ≥ `TOUCH_MIN`(48)×48（UI §5.1 触摸目标）"
                );
                assert_eq!(
                    rt_c.x1 - ban_c.x2 - 1,
                    Dimens::GAP_MIN,
                    "「重试」↔ 提示条净距 = `GAP_MIN`(16)"
                );
                assert!(
                    rt_c.y1 >= ban_c.y1 && rt_c.y2 <= ban_c.y2,
                    "「重试」与提示条**同行**（不额外占高 —— 逐字：右侧或下一行）"
                );
                p4.catalog_retry_button()
                    .button()
                    .obj()
                    .send_event(EventCode::CLICKED);
                assert_eq!(
                    hits.get(),
                    1,
                    "点「重试」必须**投出恰好一次**意图（回调只投意图、不发请求）"
                );
                // ③′ 下钻覆盖层打开 ⇒ **显式收起**提示条与「重试」（否则「重试」x944–992 与
                //     下钻「收起」x872–992 **重叠** ⇒ 两块可点区域叠在一起）；收起 ⇒ 复原。
                p4.show_detail();
                disp.refr_now_for_test();
                assert!(p4.drill_open(), "先进入下钻");
                assert!(
                    !p4.catalog_stale_visible(),
                    "下钻覆盖层占同一片区域 ⇒ 提示条必须收起（不得与「收起」重叠）"
                );
                p4.collapse_detail();
                disp.refr_now_for_test();
                assert!(
                    p4.catalog_stale_visible(),
                    "收起下钻 ⇒ 提示条按当前 stale 态**复原**"
                );
                // ④ 复原（逐像素）
                p4.set_catalog_stale(false);
                disp.refr_now_for_test();
                assert!(!p4.catalog_stale_visible(), "stale 解除 ⇒ 提示条收起");
                assert_eq!(
                    p4.scroll_obj().coords(),
                    sc_base,
                    "解除后滚动视口必须**逐像素**回到既有版面（`set_catalog_stale(false)` 幂等 + 复原）"
                );
            }
        }

        drop(p4);
    }

    // ═══ ⑰ T21c-2：P6「装置与外设」五段 + BMS 288 位下钻（T-18 / T-25 / 窗口化 / 惰性）═══
    //
    // 覆盖：**T-25 的实测几何**（段宽 185 / 段高 48 / 段间**净距** 16 —— 从 `coords()` 读出，
    // 不看常量名）／站状态表（**恒 5 行** + 三态文案 + 降级不改行高行数）／**惰性创建**
    // （未进入的段无对象）+ **切换只改可见性**（对象计数不增）／**窗口化**（行数 > 池大小；
    // 滚动 ⇒ 窗口起点前移而**对象数不变**）／BMS 下钻（标题字面量 / 池大小 / 按钮尺寸 /
    // 失败态只显本地文案 + 重试 / 翻页意图 / 收起）／**中文墨量密度**（T-18）／
    // 全部段 + 下钻创建后的**对象预算**（实测值，见 `P6_ALL_SEGMENTS_BUDGET`）。
    {
        use crate::ui::pages::p6_system::{
            self, RowKind, DRILL_ROW_POOL, SEG_BATTERY, SEG_DEVICE, SEG_ROW_POOL,
        };
        use mupc_display_proto::{BmsAlarmItem, BmsAlarmPage, PeriphRole};

        // ── 夹具：5 个 role（fire 离线 / pcs 缺席）+ 帧内 3 站在线 + 一页 BMS 告警位 ──
        let cat = p6_catalog(&[
            (PeriphRole::Hvac, true),
            (PeriphRole::Fire, true),
            (PeriphRole::Battery, true),
            (PeriphRole::MeterBatt, true),
            (PeriphRole::Pcs, false),
        ]);
        // `last_ok_ms = 0` ⇒ 该列必须显 `–`（**不臆造**，R-38 未落地的口径）。
        let sec = p6_section(&cat, &[PeriphRole::Hvac, PeriphRole::Battery, PeriphRole::MeterBatt], 0);
        let bms_page = BmsAlarmPage {
            page: 1,
            page_size: 50,
            total: 288,
            active_total: 2,
            has_more: true,
            available: true,
            items: (1..=50u16)
                .map(|at| BmsAlarmItem {
                    at,
                    active: at <= 2,
                })
                .collect(),
        };

        // `Rc` + `wire_tabs()` = **生产路径**（`Shell::new` 的同款接线）：点段 ⇒ 立即建/刷该段。
        let p6 = Rc::new(p6_system::P6SystemPage::new(&host).expect("P6SystemPage::new"));
        p6.wire_tabs();
        let base = crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst);
        p6.set_catalog(&cat);
        p6.set_periph(&sec);
        p6.set_bms_page(&bms_page);
        let f = frame_healthy();
        p6.render(&PageInput::live(&f));
        disp.refr_now_for_test();

        // ── ① T-25 实测几何：段宽 / 段高 / **段间净距** / 总宽 / 段内容 y ───────────
        let mut spans: Vec<(i32, i32)> = Vec::new();
        for i in 0..Dimens::TAB_COUNT as usize {
            let c = p6.tab_obj(i).expect("第 i 段按钮").coords();
            assert_eq!(c.x2 - c.x1 + 1, Dimens::TAB_W, "段宽 = 185（UI §6.6.1）");
            assert_eq!(c.y2 - c.y1 + 1, Dimens::TAB_H, "段高 = 48 ≥ TOUCH_MIN");
            spans.push((c.x1, c.x2));
        }
        for w in spans.windows(2) {
            assert_eq!(
                w[1].0 - w[0].1 - 1,
                Dimens::SEG_GAP,
                "段间**净距**必须 = `SEG_GAP`(16) —— `lv_button` 命中区无外扩（R-44）"
            );
        }
        assert_eq!(
            spans[0].0,
            Dimens::SIDE_PAD,
            "分段控件自页根 x=0 起（页根已在 (SIDE_PAD, HEADER_H)）"
        );
        assert_eq!(spans.last().unwrap().1 - spans[0].0 + 1, 989, "五段总宽 989 ≤ 992");
        assert_eq!(
            p6.tabs_obj().size(),
            (989, Dimens::TAB_H),
            "分段控件本体 = 5×185 + 4×16（UI §15.5.1 的算式）"
        );
        assert_eq!(
            p6.segment_content_y(),
            Dimens::SECTION_Y,
            "段内容 y = 128 + 16 − 72 = 72（UI §6.6.1）"
        );
        for (i, want) in [
            "装置",
            "空调",
            "电池",
            "储能表",
            "PCS",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(
                p6.segment_label(i).as_deref(),
                Some(*want),
                "段名必须取自契约（`ui_text`）"
            );
        }

        // ── ② 站状态表：恒 5 行 + 三态 + `–` 占位 ────────────────────────────────
        assert_eq!(p6.station_row_count(), 5, "行集合 = 5 个 role 全集（F25.5）");
        let r0 = p6.station_row_text(0).expect("第 0 行");
        assert_eq!(r0.0, "站 空调", "站名 = 「站」+ role 中文名");
        assert_eq!(r0.1, ui_text::ONLINE, "hvac 站在线");
        // **四列**（UI §6.6.1 / F2 的四列返工）：第 3 / 4 列**各占一列**，标签取缩短形态。
        assert!(
            r0.2.starts_with(ui_text::LAST_OK_SHORT)
                && r0.2.contains(crate::ui::pages::PLACEHOLDER),
            "第 3 列 = 「成功 `–`」（`last_ok_ms = 0` ⇒ 不可得，**不臆造**）：{}",
            r0.2
        );
        assert!(
            r0.3.starts_with(ui_text::LAST_UPDATE_SHORT)
                && !r0.3.contains(crate::ui::pages::PLACEHOLDER),
            "第 4 列 = 「更新 `12:03:46`」（块时标有值 ⇒ **不是**占位符）：{}",
            r0.3
        );
        let states: Vec<String> = (0..5)
            .map(|i| p6.station_row_text(i).unwrap().1)
            .collect();
        assert!(states.contains(&ui_text::STATION_OFFLINE.to_string()), "fire 缺席帧内 ⇒ 站离线");
        assert!(
            states.contains(&ui_text::STATION_DISABLED.to_string()),
            "pcs `enabled = false` ⇒ 「未启用」（情形⑦）"
        );
        assert!(
            states.iter().all(|s| !s.is_empty()),
            "每行都必须有状态词（不得留空）"
        );
        // 行高基线（降级前后必须相同）
        let heights: Vec<Option<i32>> = (0..5).map(|i| p6.station_row_height(i)).collect();
        // 注入「全部站离线」⇒ 行数 / 行高**不变**，只有文本变（PRD §4.2.4）
        let sec_off = p6_section(&cat, &[], 0);
        p6.set_periph(&sec_off);
        p6.render(&PageInput::live(&f));
        disp.refr_now_for_test();
        assert_eq!(p6.station_row_count(), 5, "站离线**不得**删行（F25.5）");
        assert_eq!(
            (0..5).map(|i| p6.station_row_height(i)).collect::<Vec<_>>(),
            heights,
            "降级行与在线时**同行高**"
        );
        assert_eq!(
            p6.station_row_text(0).unwrap().1,
            ui_text::STATION_OFFLINE,
            "离线后该行取「站离线」"
        );
        assert_ne!(
            p6.station_row_text(0).unwrap().1,
            p6.station_row_text(4).unwrap().1,
            "「站离线」≠「未启用」（四条文案两两互异）"
        );
        p6.set_periph(&sec);
        p6.render(&PageInput::live(&f));
        disp.refr_now_for_test();

        // ── ③ 惰性创建 + 切换只改可见性 ─────────────────────────────────────────
        for i in 1..5 {
            assert!(!p6.segment_built(i), "段 {i} 未进入过 ⇒ 不得有段内容对象");
        }
        assert_eq!(p6.selected_segment(), SEG_DEVICE, "默认段 = 装置");
        assert!(p6.segment_built(SEG_DEVICE), "段「装置」（含 F8 三卡）随页装配");
        let n_before_lazy =
            crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst);
        p6.click_segment(1); // 用户点「空调」
        let n_after_lazy =
            crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst);
        assert!(p6.segment_built(1), "首次进入 ⇒ 惰性创建段内容");
        assert_eq!(p6.selected_segment(), 1, "分段切换只改段下标（**不改 current_page**）");
        assert!(p6.segment_visible(1) && !p6.segment_visible(SEG_DEVICE), "只切可见性");
        assert!(
            n_after_lazy > n_before_lazy,
            "首次进入必须真的建出段内容（否则「惰性」名不副实）"
        );

        // 段「空调」行数 = 站状态行 1 + 4 个组头 + 33 个白名单点 = 38；池 = 19（**与行数无关**）
        assert_eq!(p6.segment_row_count(1), Some(38), "段「空调」行数（白名单 33 + 4 组头 + 1 站行）");
        assert_eq!(
            p6.segment_pool_size(1),
            Some(SEG_ROW_POOL),
            "行池 = 可视行 ×1.5 + 1（与行数**无关**）"
        );
        assert!(
            p6.segment_row_count(1).unwrap() > SEG_ROW_POOL,
            "行数 > 池大小 ⇒ 窗口化确实在起作用（否则该段不会被窗口化）"
        );
        let vis = p6.segment_visible_rows(1).expect("可见行数");
        assert!(vis <= SEG_ROW_POOL, "可见行数不得超过池（{vis} > {SEG_ROW_POOL}）");
        assert!(vis >= 5, "视口内至少应有一屏行（实测 {vis}）");
        // 段顶第 0 行 = 站状态行；其后是组头与数据行
        let (l0, v0, _a0, u0) = p6.segment_row_text(1, 0).expect("段内第 0 行");
        assert_eq!(l0, "站 空调", "段顶是站状态条（UI §6.6.1）");
        assert_eq!(v0, ui_text::ONLINE);
        assert!(
            u0.starts_with(ui_text::LAST_UPDATE_SHORT),
            "段顶第 4 列 = 「更新 …」（四列口径，与段「装置」同款）：{u0}"
        );

        // ── ③′ **F2：站状态条四列**（UI §6.6.1）—— 段「装置」5 行表与各段顶**同款判据** ──
        // 判据只读**实测几何**（`coords()` + 生产字体的 `adv_w` 之和），不看常量名：
        // ① 恰四列；② 相邻列不重叠；③ 每列文本 ≤ 该列槽宽。
        // `last_ok_ms` 注入**真值**（= R-38 落地后的最长形态「成功 12:03:44」；
        // 「最后成功 … · 最近更新 …」合并串 424 px 在旧实现里 > aux 槽 398 px ⇒ 必截断）。
        let sec_real = p6_section(
            &cat,
            &[PeriphRole::Hvac, PeriphRole::Battery, PeriphRole::MeterBatt],
            1_789_047_727_000,
        );
        p6.set_periph(&sec_real);
        p6.render(&PageInput::live(&f));
        disp.refr_now_for_test();
        // 第 3 列的两种**合法**形态：① 帧内**有**该站 ⇒ 真值 `hh:mm:ss`（= R-38 落地后的
        // **最长形态**）；② 帧内**无**该站（`pcs` 未启用 ⇒ 不进帧）⇒ `last_ok_ms = 0` ⇒ `–`
        // （**不臆造**，T-24 口径）。⚠️ **不能按"是否在线"分**：站离线仍可能带着上次成功的
        // 时标（`last_ok_ms` 的语义 = 最后一次**成功**，不是"当前在线"）。
        let want_ok = format!("{} 13:42:07", ui_text::LAST_OK_SHORT);
        let want_none = format!(
            "{} {}",
            ui_text::LAST_OK_SHORT,
            crate::ui::pages::PLACEHOLDER
        );
        let want_upd = format!("{} 00:00:01", ui_text::LAST_UPDATE_SHORT);
        let want_none_upd = format!(
            "{} {}",
            ui_text::LAST_UPDATE_SHORT,
            crate::ui::pages::PLACEHOLDER
        );
        let mut real_rows = 0usize;
        for i in 0..5 {
            let (name, st, ok, upd) = p6.station_row_text(i).expect("站状态行");
            assert!(
                ok == want_ok || ok == want_none,
                "第 {i} 行第 3 列只允许「成功 <时刻>」或「成功 –」两种形态：{ok}"
            );
            assert!(
                upd == want_upd || upd == want_none_upd,
                "第 {i} 行第 4 列只允许「更新 <时刻>」或「更新 –」两种形态：{upd}"
            );
            real_rows += usize::from(ok == want_ok);
            assert_station_four_columns(
                &p6.station_row_col_spans(i).expect("列几何"),
                &[name, st, ok, upd],
                &format!("段「装置」站状态表第 {i} 行"),
            );
        }
        assert!(real_rows >= 3, "帧内的 3 个站必须显真值时刻（实测 {real_rows} 行）");
        let r4 = p6.station_row_text(4).unwrap();
        assert_eq!(
            (&r4.2, &r4.3),
            (&want_none, &want_none_upd),
            "`pcs`（未启用 ⇒ 不进帧）两列都为 `–`（**不臆造**）"
        );
        // 段顶（段「空调」第 0 行）—— **同款判据**（口径一致性由"同一条 `PoolRow::bind`"保证）
        let (n1, s1, o1, up1) = p6.segment_row_text(1, 0).expect("段顶站状态行");
        assert_station_four_columns(
            &p6.segment_row_col_spans(1, 0).expect("列几何"),
            &[n1, s1, o1, up1],
            "段「空调」段顶站状态条",
        );
        // **R-38 的守门人**（不依赖注入值）：`hh:mm:ss` 的**上界形态**也必须放进槽里 ——
        // 两个时刻列的槽宽从**实测几何**取（`spans[k].2`），故改长标签（如回到「最后成功」）
        // 即红；把槽改窄（裁掉必要宽度）同样红。
        for (k, label) in [
            (2usize, ui_text::LAST_OK_SHORT),
            (3usize, ui_text::LAST_UPDATE_SHORT),
        ] {
            let spans = p6.station_row_col_spans(0).expect("列几何");
            let longest = format!("{label} 23:59:59");
            let w = measured_text_px(&longest, crate::ui::theme::TextSlot::Body.px());
            assert!(
                w <= spans[k].2,
                "「{longest}」实测 {w} px > 第 {k} 列槽宽 {} px —— 真值时刻会被 `DOTS` 截断\
                 （改长标签 / 改窄槽都会让本条红）",
                spans[k].2
            );
        }
        p6.set_periph(&sec);
        p6.render(&PageInput::live(&f));
        disp.refr_now_for_test();

        let kinds = p6.segment_rows(1).expect("行模型");
        assert_eq!(kinds[0].0, RowKind::Station);
        assert_eq!(kinds[1].0, RowKind::Header, "第 1 行 = 首组组头（测量值）");
        assert_eq!(kinds.iter().filter(|(k, _)| *k == RowKind::Header).count(), 4, "空调段 4 组");
        // 位行（告警位 / 辅助状态位）必须是 `Bit` 且行高 40
        assert!(
            kinds.iter().any(|(k, h)| *k == RowKind::Bit && *h == Dimens::ROW_BIT_H),
            "段「空调」含位行（行高 = ROW_BIT_H 40）"
        );
        // **窗口化**：滚到中段 ⇒ 窗口起点前移、对象数不变
        let n_win = crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst);
        let w0 = p6.segment_window_start(1);
        p6.scroll_segment(1, 600);
        let w1 = p6.segment_window_start(1);
        assert!(w1 > w0, "滚动后窗口起点必须前移（{w0:?} → {w1:?}）");
        assert_eq!(
            p6.segment_visible_rows(1),
            Some(vis),
            "滚动只**重绑**池行 ⇒ 可见行数不变"
        );
        assert_eq!(
            crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst),
            n_win,
            "滚动**不得**新建任何对象（窗口化 = 复用池行）"
        );
        // 切换段 ⇒ 只切可见性（不销毁重建、不新建）
        p6.click_segment(0);
        p6.click_segment(1);
        assert_eq!(
            crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst),
            n_win,
            "已建段的切换只改可见性（设计 §15.5.3 的容器策略）"
        );
        assert!(p6.segment_visible(1) && !p6.segment_visible(0));

        // ── ③″ **滚动注册点（W-a）**：4 个外设段**都**注册了 `SCROLL` ─────────────
        // 判据 = **只发滚动、不另调重绑入口**（`scroll_segment` 已不含 `on_scroll()`）⇒
        // 窗口起点前移**只能**由生产注册的回调驱动。**删掉任一注册点 ⇒ 该段红**。
        // ⚠️ ② / ③ 的夹具里 `pcs` 是 `enabled=false` ⇒ **段 4 只有一条段级文案**（内容高 44 <
        // 视口 528 ⇒ **不可滚动**，用"窗口前移"判别不出来）⇒ 本段临时换一份 **5 站全启用**
        // 的夹具，使 4 段都可滚动；段 1–3 的行为不受影响（同一份白名单展开）。
        let cat_all = p6_catalog(&[
            (PeriphRole::Hvac, true),
            (PeriphRole::Fire, true),
            (PeriphRole::Battery, true),
            (PeriphRole::MeterBatt, true),
            (PeriphRole::Pcs, true),
        ]);
        let sec_all = p6_section(
            &cat_all,
            &[
                PeriphRole::Hvac,
                PeriphRole::Battery,
                PeriphRole::MeterBatt,
                PeriphRole::Pcs,
            ],
            0,
        );
        p6.set_catalog(&cat_all);
        p6.set_periph(&sec_all);
        for i in [1usize, 2, 3, 4] {
            p6.click_segment(i); // 惰性建段（段 3 / 4 首次进入）
            p6.render(&PageInput::live(&f));
            disp.refr_now_for_test();
            assert!(p6.segment_built(i), "段 {i} 已建");
            assert!(
                p6.segment_row_count(i).unwrap_or(0) > SEG_ROW_POOL,
                "段 {i} 的行数必须 > 池（否则该段不可滚动 ⇒ 本段断言退化）"
            );
            // ⚠️ 先复位到 0：**位置不变 ⇒ LVGL 不发 `SCROLL`**（同值滚动是空操作）⇒ 必须
            // 让两次滚动**真的改变位置**，否则本段会退化成"窗口本来就在原地"的恒真断言。
            p6.scroll_segment(i, 0);
            let before = p6.segment_window_start(i);
            assert_eq!(before, Some(0), "段 {i} 滚回顶部 ⇒ 窗口起点 = 0");
            p6.scroll_segment(i, 600);
            let after = p6.segment_window_start(i);
            assert!(
                after > before,
                "段 {i} 的窗口起点必须随滚动前移（{before:?} → {after:?}）—— \
                 不动 ⇒ 该段**没有** `SCROLL` 注册（或 `rebind` 没被回调驱动）"
            );
            p6.scroll_segment(i, 0);
            assert_eq!(
                p6.segment_window_start(i),
                Some(0),
                "段 {i} 滚回顶部 ⇒ 窗口起点回 0"
            );
        }
        // 复原 ② / ③ 的夹具（后续 ④ 的下钻断言按它给的口径）
        p6.set_catalog(&cat);
        p6.set_periph(&sec);
        p6.render(&PageInput::live(&f));
        disp.refr_now_for_test();

        // ── ③‴ **`lv_group` 单选不变量（W-c）**：任一时刻**恰有一段** CHECKED ──────
        // `select()` 的互斥是**双通道**表达（`CHECKED` 状态位 + 组内只留选中段），
        // 这里量的是**状态位**那一半（组通道在指针类 indev 下不产生键导航 ⇒ 语义冗余）。
        use crate::lvgl::style::State;
        use crate::lvgl::widgets::has_state;
        for i in [1usize, 3, 0, 4, 2] {
            p6.click_segment(i);
            let checked = (0..Dimens::TAB_COUNT as usize)
                .filter(|k| {
                    let tab = p6.tab_obj(*k).expect("第 k 段按钮");
                    has_state(&tab, State::CHECKED)
                })
                .count();
            assert_eq!(
                checked, 1,
                "切到段 {i} 后必须**恰有一段**处于 `CHECKED`（实测 {checked}）—— \
                 `select()` 的互斥逻辑回归（同时选中两段 = 单选不变量破）"
            );
        }
        p6.click_segment(1);

        // ── ④ 段「电池」+ BMS 288 位下钻 ────────────────────────────────────────
        p6.click_segment(SEG_BATTERY);
        assert!(p6.segment_built(SEG_BATTERY));
        let b_rows = p6.segment_rows(SEG_BATTERY).expect("电池段行模型");
        assert_eq!(
            b_rows.iter().filter(|(k, _)| *k == RowKind::Special).count(),
            1,
            "288 位**不铺进段内**：段内只有 1 个特殊件（摘要卡）"
        );
        // ── ④″ T21c-3-r1：段顶「名称表可能过期」提示条 + 「重试」（设计 §15.3.1 第 2 / 3 句）──
        //
        // 判定同 P4 段（显隐 / 文案 / 按钮 48×48 / 净距 16 / 点击只投意图），**差别在落点**：
        // 提示条在**段内容区最顶部**（页内 y = `Dimens::SECTION_Y`，页签之下），让位落在
        // **该段的滚动视口**（段「装置」= `host`；外设段 = `SegmentList` 视口；下钻 = 根容器）。
        // 全部几何取**实测 `coords()`**（页根屏幕坐标 + 页内偏移比对，不看常量名）。
        //
        // **改什么会让本条变红**：去掉 `apply_stale_inset` 的 `set_pos` / `set_size` ⇒ ②③ 红；
        // 去掉 `ensure_segment` 里的补落 ⇒ ③（切段）红；去掉 `open_drill` 里的补落 ⇒ ④ 红；
        // 去掉按钮 `on_clicked` 接线 ⇒ ⑤ 的计数恒 0 ⇒ 红。
        {
            const SEG_HVAC: usize = 1;
            let hits = Rc::new(Cell::new(0u32));
            {
                let h = Rc::clone(&hits);
                p6.set_on_catalog_retry(move || h.set(h.get() + 1));
            }
            // ① 默认态：不显 + 零让位 + 段「装置」视口贴段内容区顶部
            p6.select_segment(SEG_DEVICE);
            p6.render(&PageInput::live(&f));
            disp.refr_now_for_test();
            assert!(
                !p6.catalog_stale_visible(),
                "默认**不显**提示条（常驻占位会让正常态也行高少一截）"
            );
            assert_eq!(p6.stale_inset(), 0, "默认零让位");
            let page_y = p6.obj().coords().y1;
            let host_base = p6
                .segment_viewport_obj(SEG_DEVICE)
                .expect("段「装置」宿主")
                .coords();
            assert_eq!(
                host_base.y1,
                page_y + Dimens::SECTION_Y,
                "段「装置」视口默认贴段内容区顶部（页内 y = `SECTION_Y`）"
            );
            // ② stale ⇒ 提示条在段内容区顶部 + 段视口下移 72（**底缘不动**）
            p6.set_catalog_stale(true);
            disp.refr_now_for_test();
            assert!(p6.catalog_stale_visible(), "stale ⇒ 提示条可见");
            assert_eq!(
                p6.catalog_stale_text().as_deref(),
                Some(ui_text::CATALOG_STALE),
                "提示条文案必须取契约常量（不得自造串）"
            );
            let ban = p6.catalog_stale_banner_obj().coords();
            let host_stale = p6
                .segment_viewport_obj(SEG_DEVICE)
                .expect("段「装置」宿主")
                .coords();
            assert_eq!(
                ban.y1, host_base.y1,
                "提示条落在**段内容区最顶部**（原视口位）"
            );
            assert_eq!(
                ban.y2 - ban.y1 + 1,
                Dimens::BANNER_H,
                "提示条高 = `Dimens::BANNER_H`(56)（UI §5.1 #12）"
            );
            assert_eq!(
                host_stale.y1 - (ban.y2 + 1),
                Dimens::GAP_GROUP,
                "提示条 ↔ 段内容 = `GAP_GROUP`(16)"
            );
            assert_eq!(
                host_stale.y1 - host_base.y1,
                Dimens::BANNER_H + Dimens::GAP_GROUP,
                "stale ⇒ 段内容视口下移 72 px"
            );
            assert_eq!(
                host_stale.y2, host_base.y2,
                "**底缘不动**（不越出段面板、不把页根撑出滚动条）"
            );
            assert_eq!(
                p6.stale_inset(),
                Dimens::BANNER_H + Dimens::GAP_GROUP,
                "让位读口 = 72"
            );
            // ③ 切段 ⇒ **新段**的视口同样让位（`ensure_segment` 补落）
            p6.select_segment(SEG_HVAC);
            p6.render(&PageInput::live(&f));
            disp.refr_now_for_test();
            let hv = p6
                .segment_viewport_obj(SEG_HVAC)
                .expect("段「空调」视口")
                .coords();
            assert_eq!(
                hv.y1,
                page_y + Dimens::SECTION_Y + Dimens::BANNER_H + Dimens::GAP_GROUP,
                "切段后新段视口也必须让位（否则新段的顶行被提示条压住）"
            );
            // ④ 下钻在 stale 态打开 ⇒ 根容器同样让位（**不与提示条 / 重试按钮重叠**）
            p6.open_drill();
            p6.render(&PageInput::live(&f));
            disp.refr_now_for_test();
            let dr = p6.drill_root_coords().expect("下钻根容器");
            assert_eq!(
                dr.y1,
                page_y + Dimens::SECTION_Y + Dimens::BANNER_H + Dimens::GAP_GROUP,
                "下钻整体让位（顶部条不得与提示条 / 「重试」叠在一起 —— 命中区歧义）"
            );
            p6.close_drill();
            p6.select_segment(SEG_DEVICE);
            p6.render(&PageInput::live(&f));
            disp.refr_now_for_test();
            // ⑤ 「重试」：48×48 + 净距 16 + 点击**投出恰好一次**意图
            let rt = p6.catalog_retry_button().button().obj().coords();
            assert_eq!(
                (rt.x2 - rt.x1 + 1, rt.y2 - rt.y1 + 1),
                (Dimens::TOUCH_MIN, Dimens::TOUCH_MIN),
                "「重试」必须 ≥ `TOUCH_MIN`(48)×48"
            );
            assert_eq!(
                rt.x1 - ban.x2 - 1,
                Dimens::GAP_MIN,
                "「重试」↔ 提示条净距 = `GAP_MIN`(16)"
            );
            assert!(
                rt.x1 > ban.x2,
                "「重试」在提示条**右侧**（横向不重叠：重叠 = 触碰命中区歧义）"
            );
            assert!(
                rt.y1 >= ban.y1 && rt.y2 <= ban.y2,
                "「重试」与提示条**同行**（不额外占高 —— §15.3.1 的「右侧或下一行」取右侧）"
            );
            p6.catalog_retry_button()
                .button()
                .obj()
                .send_event(EventCode::CLICKED);
            assert_eq!(
                hits.get(),
                1,
                "点「重试」必须投出恰好一次意图（回调只投意图、不发请求）"
            );
            // ⑥ 复原（逐像素）
            p6.set_catalog_stale(false);
            disp.refr_now_for_test();
            assert!(!p6.catalog_stale_visible(), "解除 ⇒ 提示条收起");
            assert_eq!(p6.stale_inset(), 0, "解除 ⇒ 让位归零");
            assert_eq!(
                p6.segment_viewport_obj(SEG_DEVICE)
                    .expect("段「装置」宿主")
                    .coords(),
                host_base,
                "解除后段内容视口必须**逐像素**回到既有版面"
            );
        }

        // 点摘要卡的「查看全部 288 位」⇒ 打开下钻（生产事件路径 ⇒ 意图 ⇒ render）
        p6.click_bms_entry();
        p6.render(&PageInput::live(&f));
        disp.refr_now_for_test();
        assert!(p6.drill_open(), "入口按钮 ⇒ 打开下钻");
        assert_eq!(
            p6.selected_segment(),
            SEG_BATTERY,
            "下钻**不改**段下标（更不改 `current_page`）"
        );
        let title = p6.drill_title().expect("下钻标题");
        assert!(title.contains(ui_text::BMS_ALARM_BITS_TITLE), "标题含「BMS 告警位」：{title}");
        assert!(title.contains(ui_text::PAGE_PREFIX), "标题含「第」：{title}");
        assert!(title.contains(ui_text::PAGE_SUFFIX), "标题含「页」：{title}");
        assert!(title.contains(ui_text::BIT_ACTIVE), "标题含「活跃」：{title}");
        assert!(title.contains(" / 288"), "标题含 `/ 288`（288 = 总数）：{title}");
        assert_eq!(
            p6.drill_collapse_size(),
            Some((Dimens::DRILL_BTN_W, Dimens::TOUCH_MIN)),
            "「收起」= 120×48（UI §6.6.1）"
        );
        assert_eq!(
            p6.drill_page_button_sizes(),
            Some((
                (Dimens::DRILL_BTN_W, Dimens::TOUCH_MIN),
                (Dimens::DRILL_BTN_W, Dimens::TOUCH_MIN)
            )),
            "上一页 / 下一页 = 120×48"
        );
        assert_eq!(p6.drill_pool_size(), DRILL_ROW_POOL, "下钻行池 = 可视行 ×1.5 + 1");
        // 翻页意图（点击 ⇒ 回调载荷 = 目标页；**页面不自行发请求**）
        let got: Rc<Cell<u32>> = Rc::new(Cell::new(0));
        {
            let got = Rc::clone(&got);
            p6.set_on_bms_page(move |page| got.set(page));
        }
        p6.click_drill_page(true);
        p6.render(&PageInput::live(&f));
        assert_eq!(got.get(), 2, "「下一页」⇒ 意图载荷 = 当前页 + 1");
        p6.click_drill_page(false);
        p6.render(&PageInput::live(&f));
        assert_eq!(got.get(), 1, "「上一页」⇒ 当前页 − 1（首页不为负）");
        // 失败态：端点非 2xx ⇒ **只显本地固定文案**「明细不可用」+「重试」（R-4；服务端串只进日志）
        p6.set_bms_page_failed();
        disp.refr_now_for_test();
        assert!(p6.drill_fail_visible(), "失败态可见（**不静默**）");
        assert!(p6.drill_retry_visible(), "失败态给「重试」出路");
        assert!(
            p6.drill_title().expect("标题").contains(ui_text::BMS_ALARM_BITS_TITLE),
            "失败态仍显示顶部条标题（分页口径不变）"
        );
        p6.set_bms_page(&bms_page);
        assert!(p6.drill_open(), "重新注入一页后下钻仍打开");

        // ── ④″ **F1：下钻窗口随滚动重绑**（池 16 行 vs 一页 **25** 行）───────────
        // 一页 = `page_size 50` 位 = **25 行**（两位一行），池 = `DRILL_ROW_POOL` = 16
        // ⇒ **池外 9 行**在"只在注入拍重绑"的实现里**永不建对象**（静默丢内容）。
        // 判据用**滚动位置应有的行号**（`drill_window_start` = 0 基行号）：窗口起点必须
        // 随滚动前移，且**页尾两位（49 / 50）**必须真的被绘到池内某行上。
        // **改什么会让本段红**：① 去掉 `BmsDrill::wire_scroll()`（无非注册 ⇒ 窗口不动）；
        // ② 把 `render()` 的窗口起点钉死为 0（`start = 0`）—— 两条都实测过（见报告）。
        assert_eq!(
            p6.drill_pool_size(),
            DRILL_ROW_POOL,
            "下钻行池 = 可视行 ×1.5 + 1（与页大小**无关**）"
        );
        let ws0 = p6.drill_window_start().expect("有页数据 ⇒ 窗口存在");
        assert_eq!(ws0, 0, "未滚动 ⇒ 窗口起点 = 0");
        let vis0 = p6.drill_visible_rows();
        assert!(vis0 > 0, "未滚动 ⇒ 应有可见行（实测 {vis0}）");
        p6.scroll_drill(600);
        let ws1 = p6.drill_window_start().expect("窗口");
        assert!(
            ws1 > ws0,
            "下钻窗口起点必须随滚动前移（{ws0} → {ws1}）—— 不动 ⇒ 池外行永不绘（F1）"
        );
        // 池内已绑行数 = 「页尾剩余行数」——即池**覆盖到页尾**（不是"可见行数不变"：
        // 滚到尾部时页内剩余行本就少于池大小，那是**正确**的窗口内容）。
        assert_eq!(
            p6.drill_visible_rows(),
            25 - ws1,
            "滚到行 {ws1} 时池须覆盖到**页尾**（一页 50 位 = 25 行 ⇒ 剩余 {} 行）",
            25 - ws1
        );
        // 滚到底（LVGL 自行 clamp）：窗口起点落在**页尾**，此时页尾两位必须可见。
        p6.scroll_drill(10_000);
        let ws_end = p6.drill_window_start().expect("窗口");
        assert!(
            ws_end > ws1,
            "滚到底 ⇒ 窗口起点继续前移（{ws1} → {ws_end}）"
        );
        let mut drawn: Vec<String> = Vec::new();
        for k in 0..p6.drill_pool_size() {
            if let Some((a, b)) = p6.drill_row_names(k) {
                drawn.extend(a.into_iter().chain(b));
            }
        }
        for tail in [" 49", " 50"] {
            assert!(
                drawn.iter().any(|t| t.ends_with(tail)),
                "滚到底后页尾位号 `{tail}` 必须在池内某行上被绘（**池外行不绘 = 静默丢内容**）：{drawn:?}"
            );
        }
        assert!(
            drawn.len() >= p6.drill_visible_rows() * 2 - 1,
            "每可见行须绑两位（实测 {} 位 / {} 行）",
            drawn.len(),
            p6.drill_visible_rows()
        );
        // 回到顶部：窗口起点复位（同一条重绑通道的反向证据）
        p6.scroll_drill(0);
        assert_eq!(p6.drill_window_start(), Some(0), "滚回顶部 ⇒ 窗口起点回 0");

        // 收起 ⇒ 回段「电池」（不构成子页 ⇒ 无"返回"语义，但必须可退出）
        p6.click_drill_collapse();
        p6.render(&PageInput::live(&f));
        assert!(!p6.drill_open(), "「收起」⇒ 退出下钻（F11.3）");
        assert!(p6.segment_visible(SEG_BATTERY), "收起后段内容恢复可见");

        // ── ④′ 离开 P6 ⇒ 收起下钻（§15.5.3 的"不残留"用例；由外壳切页那一拍收口）──
        p6.click_bms_entry();
        p6.render(&PageInput::live(&f));
        assert!(p6.drill_open(), "先打开下钻");
        p6.close_drill(); // = `shell::Core::select` 在"切离 P6"那一拍做的事
        assert!(!p6.drill_open(), "切离 P6 ⇒ 下钻**不得**残留打开态");
        assert!(
            p6.segment_visible(SEG_BATTERY),
            "收起后段内容恢复（回来时看到的是摘要卡，不是残留的下钻）"
        );

        // ── ⑤ 中文墨量密度（T-18）─────────────────────────────────────────────
        p6.render(&PageInput::live(&f));
        disp.refr_now_for_test();
        let painted = sink.borrow().iter().filter(|b| **b != 0).count();
        assert!(
            painted > 10_000,
            "P6 渲染后 sink 应有成片非背景像素（实际 {painted}）—— \
             分段控件 + 站状态表 + 段内 30+ 行文本件，墨量必须显著"
        );

        // ── ⑥ 对象预算（**全部段 + 下钻创建后**；`Shell::new` 区间只含段「装置」）────
        let mounted =
            crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst) - base;
        println!("[P6] 全部段 + 下钻创建后新增对象 = {mounted}");
        assert!(
            mounted <= P6_ALL_SEGMENTS_BUDGET,
            "P6 的全部段内容 + 下钻视图新增了 {mounted} 个对象（预算 {P6_ALL_SEGMENTS_BUDGET}）\
             —— 窗口化的意义就是**不让对象数随行数增长**；超预算先减池，再谈抬预算"
        );
        drop(p6);
    }

    // ═══ P5 审计页（B2c-1；F19，**只读** + 共享时间范围筛选件）═══════════════════
    //
    // 覆盖：装配契约（契约 1：页根即滚动容器）／共享件（三档 + 自定义展开）／操作类型 chip 组
    // （含「全部」复位语义）／**只读约束**（运行期读 LVGL 标志）／三态（行 / 空态 / 不可用，
    // EDGE-08 vs EDGE-17 **不得互替**）／超限条（EDGE-15，**AU5** 的显式注入入口）／
    // 意图回调（筛选变化**去重** + 加载更多）／选项以注入为准（重建 chip 组）。
    {
        use crate::lvgl::widgets::Dir;
        use crate::ui::pages::{filters, p5_audit};
        // `AuditQuery` 是重导出（`p5_audit` 的公开面），不是 `mupc_display_proto` 的直接项。
        use crate::ui::pages::p5_audit::AuditQuery;
        use mupc_display_proto::{
            AuditPage, AuditResult, ConsoleAuditEntry, ConsoleOp, LogRange, OpOption,
            AUDIT_PAGE_SIZE,
        };

        /// 造一条审计记录（`before` / `after` / `reason` 由调用方给）。
        #[allow(clippy::too_many_arguments)]
        fn audit_entry(
            id: &str,
            ts_ms: u64,
            op: ConsoleOp,
            result: AuditResult,
            target: &str,
            before: Option<serde_json::Value>,
            after: Option<serde_json::Value>,
            reason: Option<&str>,
        ) -> ConsoleAuditEntry {
            ConsoleAuditEntry {
                id: id.into(),
                ts_ms,
                operator: mupc_display_proto::CONSOLE_OPERATOR.into(),
                op,
                target: target.into(),
                before,
                after,
                result,
                reason: reason.map(str::to_string),
                request_id: "rid-1".into(),
            }
        }

        /// 造一页审计结果。
        fn audit_page(
            entries: Vec<ConsoleAuditEntry>,
            available: bool,
            has_more: bool,
            newest_ts_ms: Option<u64>,
        ) -> AuditPage {
            AuditPage {
                entries,
                page: 1,
                page_size: AUDIT_PAGE_SIZE as u32,
                has_more,
                newest_ts_ms,
                available,
            }
        }

        let p5 = p5_audit::P5AuditPage::new(&host).expect("P5AuditPage::new");
        disp.refr_now_for_test();

        // ── ① 装配契约（**契约 1**：页根即滚动容器；§6.5 线框无底部操作条）──────────
        assert_eq!(
            p5.obj().size(),
            (Dimens::CONTENT_W, Dimens::CONTENT_H),
            "页根 = 内容区视口 992×624（B2c 的装配契约）"
        );
        assert_eq!(
            crate::lvgl::widgets::scroll_dir(p5.obj()),
            Dir::VER,
            "页根是**纵向**滚动容器（结构性禁横滚，§2.7 / §7.4）"
        );
        let root_c = p5.obj().coords();
        // 头部三条：最近审计条 36 / 不可篡改说明条 48（**紧邻**，§6.5 线框 Y80/Y116）。
        assert_eq!(
            p5.immutable_obj().coords().y1,
            root_c.y1 + Dimens::CONTENT_PAD_TOP + 36,
            "说明条紧接最近审计条（页内 y44 = 绝对 y116）"
        );
        assert_eq!(p5.immutable_obj().size().1, 48, "说明条高 48（§6.5）");
        assert_eq!(
            p5.immutable_text().as_deref(),
            Some(p5_audit::TEXT_IMMUTABLE),
            "「审计记录仅追加 · 不可修改或删除」（全角逗号改写见 AU1）"
        );
        assert_eq!(
            p5.immutable_icon().as_deref(),
            Some(p5_audit::TEXT_LOCK_ICON),
            "锁形取几何块 ■（§3.6 明写『以几何锁形替代』）"
        );
        assert!(p5.immutable_obj().is_alive(), "说明条必须存活（含色条与文案子树）");
        assert!(p5.immutable_bar_obj().is_alive());
        assert_eq!(
            p5.immutable_bar_obj().size(),
            (4, 48),
            "左缘 4 px 色条通高（§6.5：底 #14231F + 左缘 4 px #35D0C4）"
        );
        // **页面确实挂了 audit 这一档样式**（评审 ② 的整改：原 `audit_banner_hue_is_exclusive`
        // 只比 `Palette` 常量 ⇒ 把 `audit_banner_style()` 改成 `WARN_BG` 照样全绿）。
        // 薄层**没有**"已挂样式读回"通道 ⇒ 读页面记录的**应用标记**（见 `immutable_bg` 文档）。
        assert_eq!(
            p5.immutable_skin(),
            p5_audit::BannerSkin::Audit,
            "说明条必须应用 **audit** 那一档底色（改成 WARN_BG ⇒ 本行变红）
             —— 实际色值 {:?}",
            p5.immutable_bg()
        );
        assert_ne!(
            p5.immutable_skin(),
            p5_audit::BannerSkin::Warn,
            "不得与警示条同档（§6.5「合规凭据」专属色）"
        );
        // **像素读回（评审 ⑥ 的整改）**：上面那条读的是页面**自己记的**应用标记（"我打算用
        // 哪一档"），单看它**证明不了**样式真的被挂上去了（把 `set_bg_color(..)` 的实参换成
        // 别的色值、标记不动 ⇒ 原断言照绿）。故在这里直接读**渲染结果**：说明条内的取样点
        // 必须是 `Palette::AUDIT_BG`（内存序 `B,G,R`，见 `lvgl::tests::lv_color`）。
        // 「改什么会让本条变红」：把 `audit_banner_style()` 里的 `set_bg_color(BANNER_BG)`
        // 换成任何别的色值 ⇒ 下面 `assert_eq!` 立刻红（**这是唯一从"实际施加"派生的判据**）。
        {
            let c = p5.immutable_obj().coords();
            let (px, py) = (c.x1 + 700, c.y1 + 24); // 条内右侧空白处（文案列之外、圆角之内）
            assert!(
                px < c.x2 && py < c.y2,
                "取样点必须落在说明条内（coords = {c:?}）"
            );
            let off = (py as usize * W as usize + px as usize) * BPP;
            let got = [sink.borrow()[off], sink.borrow()[off + 1], sink.borrow()[off + 2]];
            let want = [Palette::AUDIT_BG.b, Palette::AUDIT_BG.g, Palette::AUDIT_BG.r];
            assert_eq!(
                got, want,
                "说明条**实际渲染**的底色必须是 §6.5 的 #14231F（B,G,R = {want:?}）；\
                 实际 = {got:?}（`set_bg_color` 被改 ⇒ 本行红）"
            );
            assert_ne!(
                got,
                [Palette::WARN_BG.b, Palette::WARN_BG.g, Palette::WARN_BG.r],
                "不得渲染成警示条底色"
            );
        }

        // ── ② 共享「时间范围」件（P3 / P5 共用；UI §6.5 筛选区「同 P3」）───────────
        assert_eq!(
            p5.filter().obj().size().1,
            filters::body_h(LogRange::H1),
            "缺省 = 最近 1 小时档 ⇒ 不展开自定义块（体高 48）"
        );
        assert_eq!(
            p5.filter().title_text().as_deref(),
            Some(filters::TEXT_RANGE_LABEL)
        );
        assert_eq!(p5.filter().seg().count(), 3, "三档分段控件（§5.1 #4）");
        for (i, t) in [
            filters::TEXT_RANGE_H1,
            filters::TEXT_RANGE_H24,
            filters::TEXT_RANGE_CUSTOM,
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(p5.filter().seg().option(i).as_deref(), Some(*t));
        }
        assert!(
            !p5.filter().custom_visible(),
            "非「自定义」档**不得**展开 DateTimeStepper（UI §6.3 ③ / LG-04）"
        );
        assert_eq!(
            p5.filter().name_text(0).as_deref(),
            Some(filters::TEXT_START)
        );
        assert_eq!(p5.filter().name_text(1).as_deref(), Some(filters::TEXT_END));
        assert_eq!(
            p5.filter().stepper(0).map(|s| s.column_headers().len()),
            Some(5),
            "起始 = 5 列步进器（年/月/日/时/分）"
        );

        // ── ③ 操作类型 chip 组（`[全部] + ConsoleOp::ALL`；§6.5 线框）───────────────
        assert_eq!(
            p5.ops_label_text().as_deref(),
            Some(p5_audit::TEXT_OPS_LABEL)
        );
        assert_eq!(
            p5.ops_chip_count(),
            ConsoleOp::ALL.len() + 1,
            "「全部」固定首位 + 4 类操作（UI §6.5 线框：全部/配置保存/恢复默认值/联锁释放/M1 授权）"
        );
        // 缺省勾选 = 「全部」（= 不按操作类型筛）⇒ 该 chip 带选中前缀 `✓`（UI §5.2）。
        assert_eq!(
            p5.ops_chip_display(0).as_deref(),
            Some(format!("{TEXT_CHECK_PREFIX}{}", p5_audit::TEXT_OPS_ALL).as_str())
        );
        for (i, op) in ConsoleOp::ALL.iter().enumerate() {
            assert_eq!(
                p5.ops_chip_display(i + 1).as_deref(),
                Some(op.label()),
                "选项文案 = 契约 label()（AU3）"
            );
        }
        assert_eq!(p5.ops_selected(), vec![0], "缺省仅「全部」勾选");

        // ── ④ 骨架态 = 无注入 ⇒ **不可用**（EDGE-17），**绝不**显「无审计记录」────────
        // 「改什么会让本条变红」：把 `Core::view` 的 `!available` 分支删掉（或让它先看
        // `shown == 0`）⇒ 下面「不可用」两条立刻变红（这正是 §8.3 禁止的语义互替）。
        assert_eq!(p5.list_view(), p5_audit::ListView::Unavailable);
        assert!(p5.unavailable_visible() && !p5.empty_visible());
        assert_eq!(
            p5.unavailable_title().as_deref(),
            Some(p5_audit::TEXT_UNAVAILABLE),
            "EDGE-17：「审计记录不可用」"
        );
        assert_eq!(p5.unavailable_kind(), UnavailableKind::Audit);
        assert_ne!(
            p5.empty_text().as_deref(),
            Some(p5_audit::TEXT_UNAVAILABLE),
            "空态与不可用态文案**不得相同**"
        );
        // 最近审计条：无记录 ⇒ 占位符（**不编造时间**）
        let newest0 = p5.newest_text().expect("最近审计条");
        assert!(newest0.contains(crate::ui::pages::PLACEHOLDER));
        assert!(!newest0.contains("1970") && !newest0.contains("20"));

        // ── ⑤ 正常页：1 成功 + 1 失败（带原因）+ 1 带 before/after ──────────────────
        let e_ok = audit_entry(
            "id-1",
            1_789_047_727_000,
            ConsoleOp::ConfigApply,
            AuditResult::Ok,
            "system.log_level",
            Some(serde_json::json!("info")),
            Some(serde_json::json!("debug")),
            None,
        );
        let e_fail = audit_entry(
            "id-2",
            1_789_047_062_000,
            ConsoleOp::InterlockRelease,
            AuditResult::Failed,
            "interlock.release",
            Some(serde_json::json!(true)),
            None,
            Some("触发源未复位：estop"),
        );
        let e_port = audit_entry(
            "id-3",
            1_789_000_000_000,
            ConsoleOp::ConfigApply,
            AuditResult::Ok,
            "gateway.port",
            Some(serde_json::json!(2404)),
            Some(serde_json::json!(2405)),
            None,
        );
        p5.set_page(&audit_page(
            vec![e_ok.clone(), e_fail.clone(), e_port.clone()],
            true,
            true,
            Some(1_789_047_727_000),
        ));
        disp.refr_now_for_test();
        assert_eq!(p5.list_view(), p5_audit::ListView::Rows);
        assert_eq!(p5.visible_rows(), 3, "三条注入 ⇒ 三行在显");
        assert_eq!(p5.rows_alive(), 3, "行对象**存活**（拥有型句柄的锚定回归锁）");
        assert!(!p5.empty_visible() && !p5.unavailable_visible(), "有行时不显空/不可用态");
        assert_eq!(
            p5.newest_text().as_deref(),
            Some("最近一条审计: 2026/09/10 13:42:07"),
            "F19.8：最近一条审计时间戳"
        );
        // 行 1：时间 / 操作者 / 结果胶囊
        assert_eq!(p5.row_time(0).as_deref(), Some("2026/09/10 13:42:07"));
        assert_eq!(
            p5.row_operator(0).as_deref(),
            Some(p5_audit::TEXT_OPERATOR_LOCAL),
            "操作者 = 本地控制台（§3.6 P5 列表行；机器名 local-console 不上屏）"
        );
        assert_eq!(p5.row_result(0).as_deref(), Some(p5_audit::TEXT_RESULT_OK));
        assert_eq!(
            p5.row_result_accent(0),
            Some(Palette::OK),
            "成功胶囊的色通道（§3.6：`● 成功` 绿）"
        );
        assert_eq!(p5.row_result(1).as_deref(), Some(p5_audit::TEXT_RESULT_FAIL));
        assert_eq!(
            p5.row_result_accent(1),
            Some(Palette::DANGER),
            "失败胶囊的色通道（`✕ 失败` 红）"
        );
        // 行 2：操作类型（契约 label）+ 前后值摘要
        assert_eq!(
            p5.row_op(0).as_deref(),
            Some(ConsoleOp::ConfigApply.label()),
            "操作类型用 ConsoleOp::label()"
        );
        assert_eq!(
            p5.row_summary(2).as_deref(),
            Some("端口: 2404 → 2405"),
            "§6.5 行内容示例（已知键带中文标签；全角冒号改写见 AU1）"
        );
        assert_eq!(
            p5.row_summary(0).as_deref(),
            Some("日志级别: INFO → DEBUG"),
            "字符串值经 display_safe（小写机器值 → 大写同族）"
        );
        assert_eq!(
            p5.row_summary(1).as_deref(),
            Some("联锁释放: 开 → –"),
            "`interlock.release` 是**契约点名**的已知键 ⇒ 中文标签（评审 ③ 整改；原实现下这里是裸 `开 → –`）\
             + bool ⇒ 开/关；`None` ⇒ 占位符（**绝不补 0**，AU12 / C1 前车之鉴）"
        );
        // ── ⑤′ **时间倒序由本页保证**（§6.5；**乱序注入 ⇒ 屏上仍倒序**）──────────────────
        // 「改什么会让本条变红」：删掉 `Core::apply_page` 里的 `row_order(..)`（改用注入序）
        // ⇒ 下面第 1 / 2 行立刻红。
        //
        // ⚠️ **三条记录必须用不同的 `ConsoleOp`**（B2c-1 代码质量评审 ⑦）：三条**同为**
        // `ConfigApply` 时，"行序"与"标签刷新序"的错位**不可观测**（第 0 行不管取哪一条记录，
        // 操作类型都一样）⇒ 把 `refresh_op_labels` 改成注入序也**照样全绿**（原断言的缺陷）。
        {
            let t_new = audit_entry(
                "id-s1",
                1_789_047_700_000,
                ConsoleOp::InterlockRelease, // ← 最新一条：与另两条**不同**
                AuditResult::Ok,
                "gateway.port",
                None,
                None,
                None,
            );
            let t_mid = audit_entry(
                "id-s2",
                1_789_047_000_000,
                ConsoleOp::ConfigApply,
                AuditResult::Ok,
                "gateway.port",
                None,
                None,
                None,
            );
            let t_old = audit_entry(
                "id-s3",
                1_789_000_000_000,
                ConsoleOp::InterlockAckM1, // ← 最旧一条：与另两条**不同**
                AuditResult::Ok,
                "gateway.port",
                None,
                None,
                None,
            );
            // **故意乱序**注入（中 / 新 / 旧）。
            p5.set_page(&audit_page(
                vec![t_mid.clone(), t_new.clone(), t_old.clone()],
                true,
                false,
                Some(1_789_047_700_000),
            ));
            disp.refr_now_for_test();
            assert_eq!(
                p5.row_time(0).as_deref(),
                Some("2026/09/10 13:41:40"),
                "第 0 行必须是**最新**一条（时间倒序，§6.5）"
            );
            assert_eq!(
                p5.row_time(2).as_deref(),
                Some("2026/09/10 00:26:40"),
                "末行必须是最旧一条"
            );
            // 操作类型列也必须按**同一行序**刷新（[`row_order`] 是两处的共用真源）：
            // 给三个 op 各注入一个**互不相同**的哨兵标签 ⇒ 任何错位都能被下面三条抓住。
            p5.set_ops(&[
                OpOption {
                    op: ConsoleOp::ConfigApply,
                    label: "A1".into(),
                },
                OpOption {
                    op: ConsoleOp::InterlockRelease,
                    label: "B1".into(),
                },
                OpOption {
                    op: ConsoleOp::InterlockAckM1,
                    label: "C1".into(),
                },
            ]);
            assert_eq!(
                p5.row_op(0).as_deref(),
                Some("B1"),
                "第 0 行 = 最新一条（InterlockRelease）⇒ 标签刷新**必须**跟着同一行序走\
                 （改成注入序 ⇒ 第 0 行取到 t_mid 的 `A1`，本例立刻红）"
            );
            assert_eq!(p5.row_op(1).as_deref(), Some("A1"), "第 1 行 = 中间那条");
            assert_eq!(p5.row_op(2).as_deref(), Some("C1"), "第 2 行 = 最旧那条");
            p5.set_ops(&p5_audit::canonical_ops());
        }

        // ── ⑤″ **未登记 `target` 键在屏上仍可辨认 + 不同键可区分**（评审 ③④）────────────
        // 「改什么会让本条变红」：把 `summary_text` 的未登记分支改回"只留值对"（原实现）
        // ⇒ 第 2 条（starts_with）立刻红（屏上就没有字段标识了）；把键的截断改回**保头**
        // ⇒ 第 3 条（flood ≠ force）立刻红（原实现下两者屏上完全相同）。
        {
            let unknown = audit_entry(
                "id-u1",
                1_789_047_500_000,
                ConsoleOp::ConfigApply,
                AuditResult::Ok,
                "interlock.flood",
                Some(serde_json::json!(1)),
                Some(serde_json::json!(2)),
                None,
            );
            p5.set_page(&audit_page(vec![unknown], true, false, Some(1)));
            disp.refr_now_for_test();
            let shown = p5.row_summary(0).expect("值对文案");
            assert!(
                shown.starts_with(p5_audit::TEXT_ELLIPSIS),
                "未登记键 ⇒ 屏上仍有**可辨认**的机器键（**保尾**截断，实际：{shown}）"
            );
            assert!(
                !shown.contains("interlock.flood"),
                "原始小写机器键不上屏（实际：{shown}）"
            );
            assert!(shown.contains("1 → 2"), "值对不得被键挤掉（实际：{shown}）");
            // **不同未登记键 ⇒ 屏上产物必须不同**（评审 ④ 的补网；原实现下两者相同）。
            let unknown2 = audit_entry(
                "id-u3",
                1_789_047_500_000,
                ConsoleOp::ConfigApply,
                AuditResult::Ok,
                "interlock.force",
                Some(serde_json::json!(1)),
                Some(serde_json::json!(2)),
                None,
            );
            p5.set_page(&audit_page(vec![unknown2], true, false, Some(1)));
            disp.refr_now_for_test();
            let shown2 = p5.row_summary(0).expect("值对文案");
            assert_ne!(
                shown, shown2,
                "`interlock.flood` 与 `interlock.force` 在屏上**必须可区分**\
                 （保头截断会让两者同形（实际：{shown} vs {shown2}））"
            );
            // **不臆造**：不得出现任何已登记键的中文标签。
            for (_, label) in p5_audit::TARGET_LABELS
                .iter()
                .map(|(k, v)| (*k, *v))
                .chain(
                    p5_audit::INTERLOCK_TARGETS
                        .iter()
                        .map(|(k, op)| (*k, op.label())),
                )
            {
                for s in [&shown, &shown2] {
                    assert!(
                        !s.contains(label),
                        "未登记键**不得**借用中文标签 `{label}`（实际：{s}）"
                    );
                }
            }
            // 契约点名的两个联锁键 ⇒ 带**中文**标签（**不再**是裸值对）。
            let release = audit_entry(
                "id-u2",
                1_789_047_500_000,
                ConsoleOp::InterlockRelease,
                AuditResult::Ok,
                "interlock.release",
                Some(serde_json::json!(true)),
                None,
                None,
            );
            p5.set_page(&audit_page(vec![release], true, false, Some(1)));
            disp.refr_now_for_test();
            assert_eq!(
                p5.row_summary(0).as_deref(),
                Some(format!("{}: 开 → –", ConsoleOp::InterlockRelease.label()).as_str()),
                "`interlock.release` 是**已知键**（契约点名）⇒ 中文标签 + 值对"
            );
        }

        // ── ⑤‴ **时间 / 操作者两列的定长等宽**（§6.5「时间 24 px 等宽」）────────────────
        // 「改什么会让本条变红」：时间戳格式不再定长（例如把 `format_epoch_ms_utc` 的补零去掉）
        // ⇒ 第 1 条（字符数）立刻红；数字字形宽度不等（换掉等宽数字字体）⇒ 第 2 条红；
        // 把列宽改小到装不下定长文本 ⇒ 第 3 / 4 条红。
        {
            let t0 = p5.row_time(0).expect("时间列");
            let t1 = "2026/01/02 03:04:05";
            assert_eq!(
                t0.chars().count(),
                t1.chars().count(),
                "时间戳必须**定长**（等宽的前提；实际 `{t0}` vs `{t1}`）"
            );
            {
                let (w0, w1) = (measured_text_px(&t0, 24), measured_text_px(t1, 24));
                assert_eq!(
                    w0, w1,
                    "不同时刻的时间戳像素宽必须**相等**（等宽数字；`{t0}` vs `{t1}`）"
                );
                assert!(
                    w0 <= p5_audit::ROW_TIME_W,
                    "定长时间戳 {w0} px 必须放得进时间列 {} px",
                    p5_audit::ROW_TIME_W
                );
            }
            let op = p5.row_operator(0).expect("操作者列");
            assert_eq!(
                op,
                p5_audit::TEXT_OPERATOR_LOCAL,
                "操作者列是**定长**的 §3.6 中文名（未知名才需 DOTS 截断）"
            );
            {
                let wop = measured_text_px(&op, 24);
                assert!(
                    wop <= p5_audit::ROW_OP_W,
                    "操作者名 {wop} px 必须放得进操作者列 {} px",
                    p5_audit::ROW_OP_W
                );
            }
        }

        // 复原第 ⑤ 段的三条页（本段上面的 ⑤′/⑤″ 改了注入内容），供后续行内容 / 几何断言使用。
        p5.set_page(&audit_page(
            vec![e_ok.clone(), e_fail.clone(), e_port.clone()],
            true,
            true,
            Some(1_789_047_727_000),
        ));
        disp.refr_now_for_test();

        // 失败行有原因；成功行**无**（§6.5：`原因：…`（仅失败行））
        assert!(p5.row_reason_visible(1), "失败行必须显原因（EDGE-12 同族：不得静默）");
        assert!(p5.row_reason(1)
            .expect("原因文案")
            .starts_with(p5_audit::TEXT_REASON_PREFIX));
        assert!(
            !p5.row_reason_visible(0) && !p5.row_reason_visible(2),
            "成功行**不得**显原因列"
        );
        // 行几何（§6.5：行高 60；左缘 3 px 竖条**每条都有**）
        assert_eq!(
            p5.row_size(0),
            Some((Dimens::CONTENT_W, Dimens::ROW_AUDIT_H)),
            "行 992×60"
        );
        assert_eq!(
            p5.row_bar_size(0),
            Some((3, Dimens::ROW_AUDIT_H)),
            "左缘 3 px 竖条（每条都有）"
        );
        assert_eq!(p5.row_pos(1).map(|(_, y)| y - p5.row_pos(0).unwrap().1), Some(60));
        // 底部状态行（§3.6 P5 列表行：`加载中` / `已加载全部`）
        assert_eq!(
            p5.footer_text().as_deref(),
            Some(p5_audit::TEXT_FOOTER_LOADING),
            "has_more = true ⇒ 加载中"
        );
        assert!(p5.footer_visible());
        // 只读说明行（AU11）
        assert_eq!(
            p5.note_text().as_deref(),
            Some("本地屏不支持审计导出 · 无文件与下载通道")
        );
        // 表头（§6.5 线框 Y300：5 列）—— ⚠️ **只断"容器存活 / 高 36"是不够的**（B2c-1 的
        // 阻断级缺陷：5 列名 + 4 竖线 + 底线当时是 `new()` 的**局部句柄** ⇒ 返回即 `Drop`
        // ⇒ LVGL **级联删除整棵子树** ⇒ 表头整块空白，而容器仍存活、高仍是 36）。
        // 故这里**读 LVGL 的实际子件数**并**逐列读回文案**：
        assert!(p5.head_obj().is_alive());
        assert_eq!(p5.head_obj().size().1, 36);
        assert_eq!(
            p5.head_child_count(),
            p5_audit::HEAD_CHILD_COUNT,
            "表头**实际子件数**必须 = 5 列名 + 4 竖分隔线 + 1 底线（局部句柄被 `Drop` ⇒ 级联删子树 ⇒ 本例从 10 掉到 0）"
        );
        for (i, want) in [
            p5_audit::TEXT_HEAD_TIME,
            p5_audit::TEXT_HEAD_OPERATOR,
            p5_audit::TEXT_HEAD_OP,
            p5_audit::TEXT_HEAD_VALUE,
            p5_audit::TEXT_HEAD_RESULT,
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(
                p5.head_col_text(i).as_deref(),
                Some(*want),
                "第 {i} 列表头文案必须在屏上可读回（§6.5 线框：时间│操作者│操作类型│前后值│结果）"
            );
        }
        assert_eq!(p5.head_divs_alive(), 4, "4 条竖分隔线全部存活");
        assert!(p5.head_rule_alive(), "表头底线存活");
        // ── **只读约束**（UI §6.5「只读约束」行 / PL-02）：运行期读 LVGL 标志 ─────────
        // 「改什么会让本条变红」：给行加任何可点子对象（按钮 / 可点容器 / 长按菜单入口）
        // ⇒ `clickable_parts` > 0 ⇒ 本条立刻红。
        assert_eq!(
            p5.list_clickable_count(),
            0,
            "列表区**不得**存在任何可点对象（无编辑 / 删除 / 清空 / 导出入口）"
        );
        assert_eq!(p5_audit::P5AuditPage::WRITE_ENTRIES.len(), 0);

        // ── ⑥ 意图回调：筛选变化（**变化才发、未变化不发**）────────────────────────
        let fires = Rc::new(RefCell::new(Vec::new()));
        {
            let f = Rc::clone(&fires);
            p5.set_on_query(move |q| f.borrow_mut().push(q));
        }
        // 模拟"用户点了第 2 段（最近 24 小时）"：键矩阵的 `btn_id_sel` 已指向它，再派发
        // `VALUE_CHANGED`（= 键矩阵类处理器在按下时派发的正是它）。
        p5.filter().seg().set_selected(1);
        p5.filter().seg().send_event(EventCode::VALUE_CHANGED);
        assert_eq!(fires.borrow().len(), 1, "筛选变化 ⇒ **恰发一次**意图");
        {
            let q = &fires.borrow()[0];
            assert_eq!(q.range, LogRange::H24);
            assert_eq!(q.page, 1, "筛选变化恒为第 1 页");
            assert_eq!(q.from_ms, None);
            assert_eq!(q.to_ms, None, "相对窗口由服务端按档位算（UI 不读时钟）");
            assert!(q.ops.is_empty(), "仅「全部」⇒ 不按操作类型筛");
        }
        // **未变化时不发意图**：重复点同一段（键矩阵照发 `VALUE_CHANGED`）⇒ 不得重发。
        // 「改什么会让本条变红」：去掉 `Core::fire_query` 里的去重判断 ⇒ 本条立刻红。
        p5.filter().seg().send_event(EventCode::VALUE_CHANGED);
        assert_eq!(fires.borrow().len(), 1, "同一筛选条件**不重复**发意图（去重）");
        // 切到「自定义」⇒ 展开两个步进器 + 意图携带起止（**页面不读时钟**，故起止为
        // 可表示全区间；见 filters.rs **FR1**）。
        p5.filter().seg().set_selected(2);
        p5.filter().seg().send_event(EventCode::VALUE_CHANGED);
        disp.refr_now_for_test();
        assert_eq!(fires.borrow().len(), 2);
        {
            let q = &fires.borrow()[1];
            assert_eq!(q.range, LogRange::Custom);
            assert_eq!(q.from_ms, Some(0), "缺省起始 = 纪元原点（可表示下界）");
            assert!(q.to_ms.is_some() && q.to_ms.unwrap() > q.from_ms.unwrap());
            assert_eq!(q.page, 1);
        }
        assert!(p5.filter().custom_visible(), "「自定义」档必须展开步进器");
        assert_eq!(
            p5.filter().obj().size().1,
            filters::body_h(LogRange::Custom),
            "展开后体高 48 → 284（其下区块由 layout() 重摆）"
        );
        assert_eq!(p5.filter().stepper(0).map(|s| s.column_headers().len()), Some(5));
        assert_eq!(p5.filter().stepper(1).map(|s| s.column_headers().len()), Some(5));
        // 回到「最近 1 小时」（复原，避免影响后续断言）。
        p5.filter().seg().set_selected(0);
        p5.filter().seg().send_event(EventCode::VALUE_CHANGED);
        assert_eq!(fires.borrow().len(), 3);
        assert!(!p5.filter().custom_visible(), "切回其他档 ⇒ 收起自定义块");

        // ── ⑦ 「加载更多」（**AU6**：滚动事件在本层不可得 ⇒ 显式触发入口）───────────
        let more = Rc::new(RefCell::new(Vec::new()));
        {
            let m = Rc::clone(&more);
            p5.set_on_load_more(move |q| m.borrow_mut().push(q));
        }
        p5.request_next_page();
        assert_eq!(more.borrow().len(), 1);
        assert_eq!(
            more.borrow()[0].page,
            2,
            "page = 最近一次注入的 AuditPage.page + 1"
        );
        assert_eq!(more.borrow()[0].range, LogRange::H1);
        // `has_more = false` ⇒ **不发**（最后一页不再空转）。
        p5.set_page(&audit_page(vec![e_port.clone()], true, false, Some(1)));
        p5.request_next_page();
        assert_eq!(more.borrow().len(), 1, "无更多 ⇒ 不发意图");
        assert_eq!(
            p5.footer_text().as_deref(),
            Some(p5_audit::TEXT_FOOTER_ALL),
            "has_more = false ⇒ 已加载全部"
        );

        // ── ⑦″ **滚到接近底部 ⇒ 自动发「加载更多」**（B4b：AU6 的触发点接线）────────────
        //
        // AU6 原先登记的是"滚动事件在本层不可得 ⇒ 触发与节流归 B3"。B4b 给薄层镜了
        // `LV_EVENT_SCROLL`（[`EventCode::SCROLL`]，数值早已随 `lv_event_code_t` 生成）
        // ⇒ 本页**在自己的滚动容器上**注册回调，滚到接近底部时调 `request_next_page()`。
        // 判"接近底部"用的是**本页自己的布局算术**（`y_list + list_h` vs 视口高）——
        // 薄层没有"内容高 / 剩余可滚量"读回口（补它要放行 `lv_obj_get_scroll_bottom`）。
        {
            let seen: Rc<RefCell<Vec<AuditQuery>>> = Rc::new(RefCell::new(Vec::new()));
            {
                let m = Rc::clone(&seen);
                p5.set_on_load_more(move |q| m.borrow_mut().push(q));
            }
            // 满页 20 行（列表远高于视口 ⇒ 真能滚）+ `has_more = true`。
            let full: Vec<_> = (0..AUDIT_PAGE_SIZE)
                .map(|i| {
                    audit_entry(
                        &format!("id-scroll-{i}"),
                        1_900_000_000_000 + i as u64,
                        ConsoleOp::ConfigApply,
                        AuditResult::Ok,
                        "gateway.port",
                        None,
                        None,
                        None,
                    )
                })
                .collect();
            p5.set_page(&audit_page(full, true, true, Some(1)));
            disp.refr_now_for_test();
            assert_eq!(p5.visible_rows(), AUDIT_PAGE_SIZE, "⑦″ 前置：满页 20 行");

            // ── 反例：**只滚到中段** ⇒ **不发**（"接近底部"不是恒真判据）──
            // 注：本行必须**真的产生一次滚动**（`scroll_y` 真的变了），否则"不发"是空断言。
            p5.obj().scroll_to_y(400);
            let mid = p5.obj().scroll_y();
            assert!(
                mid > 200,
                "⑦″ 前置：必须真的滚到中段（实得 scroll_y = {mid}）—— 为 0 则下面的\
                 「不发」断言恒真"
            );
            assert_eq!(seen.borrow().len(), 0, "中段 ⇒ **不发**「加载更多」");

            // ── 正例：滚到**底部**（越界值由 LVGL 夹到最大滚动量）⇒ 恰好发一次 ──
            p5.obj().scroll_to_y(100_000);
            let bottom = p5.obj().scroll_y();
            assert!(
                bottom > mid,
                "⑦″ 前置：已到最大滚动量（实得 {bottom} > {mid}）"
            );
            assert_eq!(
                seen.borrow().len(),
                1,
                "**滚到底 ⇒ 发一次「加载更多」**（`LV_EVENT_SCROLL` 驱动；改什么会让本条变红：\
                 删掉 `P5AuditPage::new` 里那条 `EventCode::SCROLL` 注册、或删掉\
                 `Core::on_scroll` 的 `fire_load_more()`）"
            );
            assert_eq!(seen.borrow()[0].page, 2, "page = 最近一次注入的 page(1) + 1");
            assert_eq!(seen.borrow()[0].range, LogRange::H1, "筛选态随意图带出");

            // ── 闩的语义（**边沿触发**）：一次拖动会连发上百个 SCROLL 事件 ⇒ 进区间只发一次 ──
            p5.obj().scroll_to_y(400); // 离开底部区间
            assert_eq!(seen.borrow().len(), 1, "离开底部区间 ⇒ 不补发");
            p5.obj().scroll_to_y(100_000); // 再次进入区间
            assert_eq!(
                seen.borrow().len(),
                2,
                "再次滚到底 ⇒ **再发一次**（闩在离开区间时复位 —— 否则「到底后拿不到下一页」）"
            );

            // ── **真连发**（B4b 整改「重要 2」）：1 px 微步进 × 200 次，末段停在底部区间 ──────
            //
            // **为什么必须补这一段**：上面两条走的都是 `scroll_to_y` 的**单次**跳变 —— 全链路
            // 每次只发**一个** `SCROLL`，于是"闩的语义（边沿触发）"那段**恒真**（实测：把
            // `Core::on_scroll` 的判据 `load_more_armed.replace(false)` 改成恒真，⑦″ **照样
            // 全绿**）。真实拖动的形态是**连发**：`lv_obj_scroll_by_raw` 每移动一次就派发一个
            // `LV_EVENT_SCROLL`（`vendor/lvgl/src/core/lv_obj_scroll.c:427`），一次手指拖动会发
            // 上百个 ⇒ 无闩会把上层意图队列灌爆。本段以 1 px/次复现该形态。
            assert!(
                bottom > 400,
                "⑦″ 前置：最大滚动量必须 > 400（实得 {bottom}）—— 否则下面的 1 px 微步进\
                 复现不出「从中段拖到底」的完整轨迹"
            );
            p5.obj().scroll_to_y(400); // 回到中段（离开区间 ⇒ 闩复位）
            assert_eq!(seen.borrow().len(), 2, "⑦″ 前置：回到中段 ⇒ 不补发");
            // 1 px/次从 400 拖到最大滚动量：末段 ~60 次全部落在底部区间内 —— 这正是"一次真实
            // 拖动"的形态（`lv_obj_scroll_by_raw` 每移动一次就派发一个 `LV_EVENT_SCROLL`，
            // `vendor/lvgl/src/core/lv_obj_scroll.c:427`；一次手指拖动会连发上百个）。
            for y in 400..=bottom {
                p5.obj().scroll_to_y(y);
            }
            assert_eq!(
                p5.obj().scroll_y(),
                bottom,
                "⑦″ 前置：微步进必须真的停在最大滚动量（否则本段退化成空断言）"
            );
            assert_eq!(
                seen.borrow().len(),
                3,
                "**连发 {} 个 `SCROLL`、末段一直停在底部区间 ⇒ 意图只产生一次**（边沿闩；\
                 改什么会让本条变红：把 `Core::on_scroll` 的 \
                 `if self.load_more_armed.replace(false)` 改成无条件 `fire_load_more()`）",
                bottom - 400 + 1
            );

            // 复原（避免影响后续单元：⑧ 起的注入都只有几行）。
            p5.obj().scroll_to_y(0);
            assert_eq!(p5.obj().scroll_y(), 0, "⑦″ 收尾：回顶");
        }

        // ── ⑧ 空态（EDGE-08）：`entries` 空 + `available = true` ────────────────────
        p5.set_page(&audit_page(vec![], true, false, None));
        disp.refr_now_for_test();
        assert_eq!(p5.list_view(), p5_audit::ListView::Empty);
        assert!(p5.empty_visible() && !p5.unavailable_visible());
        assert_eq!(
            p5.empty_text().as_deref(),
            Some(p5_audit::TEXT_EMPTY),
            "EDGE-08：「当前筛选条件下无审计记录」"
        );
        assert_ne!(
            p5.empty_text().as_deref(),
            Some(p5_audit::TEXT_UNAVAILABLE),
            "**不得**把空态显示成不可用（语义不同，§8.3 两行专行）"
        );
        assert!(!p5.footer_visible(), "无行时**不显**状态行");

        // ── ⑨ 不可用（EDGE-17）：`available = false`（即使有 entries 也不显行）────────
        p5.set_page(&audit_page(vec![e_ok.clone()], false, false, None));
        disp.refr_now_for_test();
        assert_eq!(p5.list_view(), p5_audit::ListView::Unavailable);
        assert!(p5.unavailable_visible() && !p5.empty_visible());
        assert_eq!(p5.visible_rows(), 0, "不可用时**不得**显行（entries 不可信）");
        assert_ne!(
            p5.unavailable_title().as_deref(),
            Some(p5_audit::TEXT_EMPTY),
            "**不得**把不可用显示成「无审计记录」"
        );
        // 显式入口（整条控制通道读失败时 B3 用它）：原因经 `free_text_safe`（含全角标点折叠）。
        p5.set_unavailable("审计目录不可读：permission denied");
        assert!(p5.unavailable_visible());
        assert_eq!(
            p5.unavailable_title().as_deref(),
            Some(p5_audit::TEXT_UNAVAILABLE)
        );

        // ── ⑩ 超限（EDGE-15）：**AU5** —— 契约无该字段 ⇒ 显式注入入口 ────────────────
        assert!(!p5.warn_visible(), "缺省不显（`AuditPage` 推不出该标志 —— 见 AU5）");
        p5.set_range_too_large(true);
        disp.refr_now_for_test();
        assert!(p5.warn_visible(), "注入 true ⇒ 列表区上方出现 WarnBanner");
        assert_eq!(
            p5.warn_text().as_deref(),
            Some(p5_audit::TEXT_RANGE_TOO_LARGE)
        );
        p5.set_range_too_large(false);
        assert!(!p5.warn_visible());

        // ── ⑪ 选项以注入为准（重建 chip 组 + 刷新已上屏的操作类型列）────────────────
        assert_eq!(p5.ops_options().len(), ConsoleOp::ALL.len());
        p5.set_page(&audit_page(vec![e_ok.clone()], true, false, Some(1)));
        let injected = vec![OpOption {
            op: ConsoleOp::ConfigApply,
            label: "debug".into(),
        }];
        p5.set_ops(&injected);
        assert_eq!(p5.ops_options().len(), 1, "选项集合以注入为准");
        assert_eq!(p5.ops_chip_count(), 2, "「全部」+ 1 项");
        assert_eq!(p5.ops_chip_display(1).as_deref(), Some("DEBUG"));
        assert_eq!(
            p5.row_op(0).as_deref(),
            Some("DEBUG"),
            "已上屏行的操作类型列随注入标签刷新（同一出口）"
        );
        // 复原（避免影响后续单元）。
        p5.set_ops(&p5_audit::canonical_ops());
        assert_eq!(p5.ops_chip_count(), ConsoleOp::ALL.len() + 1);
        assert_eq!(
            p5.row_op(0).as_deref(),
            Some(ConsoleOp::ConfigApply.label())
        );

        // ── ⑫ **行池容量**（**AU8**）：注入契约规定的每页条数必须成功且不 OOM ─────────────
        // 实测（AU8）：20 / 22 条成功，**24 条即 `lv_realloc` 失败 + 挂死** ⇒ `ROW_MAX` 收敛到
        // 20（纯逻辑上界另有 `p5_audit::tests::row_pool_capacity_is_measured`，那条改坏时
        // **当场红**，不会把测试跑挂 —— 本段只在**合法上界内**验证真实渲染路径）。
        {
            let page_full: Vec<_> = (0..AUDIT_PAGE_SIZE)
                .map(|i| {
                    audit_entry(
                        &format!("id-full-{i}"),
                        1_700_000_000_000 + i as u64,
                        ConsoleOp::ConfigApply,
                        AuditResult::Ok,
                        "gateway.port",
                        Some(serde_json::json!(i)),
                        None,
                        None,
                    )
                })
                .collect();
            p5.set_page(&audit_page(page_full, true, true, Some(1)));
            disp.refr_now_for_test();
            assert_eq!(
                p5.visible_rows(),
                AUDIT_PAGE_SIZE,
                "注入契约规定的每页条数（{AUDIT_PAGE_SIZE}）⇒ 整页可见（不 OOM）"
            );
            assert_eq!(p5.rows_alive(), AUDIT_PAGE_SIZE, "行对象全部存活（非「有行但空白」）");
            // 超过上界 ⇒ **只渲染上界条**（有界，不增长；`apply_page` 是"整体替换窗口"语义）。
            let over: Vec<_> = (0..AUDIT_PAGE_SIZE * 3)
                .map(|i| {
                    audit_entry(
                        &format!("id-over-{i}"),
                        1_800_000_000_000 + i as u64,
                        ConsoleOp::ConfigApply,
                        AuditResult::Ok,
                        "telemetry.interval",
                        None,
                        None,
                        None,
                    )
                })
                .collect();
            p5.set_page(&audit_page(over, true, false, Some(1)));
            disp.refr_now_for_test();
            assert_eq!(
                p5.row_pool_len(),
                AUDIT_PAGE_SIZE,
                "行池**有界**：注入 3 页也只建一页（AU8；原 100 的承诺会 OOM 挂死）"
            );
            assert_eq!(p5.visible_rows(), AUDIT_PAGE_SIZE);
        }

        // 渲染后确有像素（装配 → 布局 → 像素全链）。
        let painted5 = sink.borrow().iter().filter(|b| **b != 0).count();
        assert!(painted5 > 10_000, "P5 渲染后 sink 中应有成片非背景像素（实际 {painted5}）");

        // ── ⑬ **`drop(P5AuditPage)` 必须释放整页**（**C1**：`Rc<Core>` 强引用环的回归网）─────
        // 「改什么会让本条变红」：把 chip 回调槽里的 `Weak<Core>` 改回 `Rc<Core>` ——
        // 环 = `Core → ops → chips.on_change → Rc<Core>` ⇒ `drop` **不释放任何对象**（整棵树
        // 连 `root` 一起永久留在宿主上）⇒ 下面第 1 条断言（`!is_alive()`）**当场红**。
        // **为什么用存活探针而不是"再建一页"**：环存在时"再建一页"会先在 LVGL 里
        // `lv_realloc` 失败 + `lv_array_resize` 断言 ⇒ **挂死**（不是红）；而 `is_alive()` 是
        // LVGL 的 DELETE 事件**确定性**置位的标志 ⇒ 立刻、无分配地拿到判定。
        // 探针句柄是**非拥有**的共享句柄（`share_borrowed`：同 `alive`、不重复挂回调）。
        {
            let probes = [
                ("root（页根滚动容器）", p5.obj().share_borrowed()),
                ("最近审计条", p5.newest_obj().share_borrowed()),
                ("不可篡改说明条", p5.immutable_obj().share_borrowed()),
                ("共享时间范围件", p5.filter().obj().share_borrowed()),
                ("操作类型 chip 组容器", p5.ops_obj().share_borrowed()),
                ("表头", p5.head_obj().share_borrowed()),
                ("列表容器", p5.list_obj().share_borrowed()),
            ];
            for (what, o) in &probes {
                assert!(o.is_alive(), "探针建立时 `{what}` 必须存活");
            }
            drop(p5);
            for (what, o) in &probes {
                assert!(
                    !o.is_alive(),
                    "`drop(P5AuditPage)` 之后 `{what}` 必须已被级联删除 —— 仍存活 ⇒ 存在 \
                     `Rc` 强引用环（整页泄漏；C1）"
                );
            }
        }

        // ── ⑭ **连续建 / 拆 P5 页（满行）必须全部成功**（**C1** 的生产路径回归网）──────────
        // 此时宿主上**没有**其它页（`p5` 已在 ⑬ 拆掉）⇒ 每轮都是"单页满行"，与 AU8 的
        // 单页测量前提一致。**环存在时**：第 1 轮 `drop` 泄漏整页 ⇒ 第 2 轮 `new` 即
        // `OutOfMemory`（⑬ 的探针会先一步变红，故本条不会跑到挂死点）。
        for round in 0..3 {
            let p = p5_audit::P5AuditPage::new(&host).expect("第 N 轮建 P5 页（环存在时这里 OOM）");
            let full: Vec<_> = (0..AUDIT_PAGE_SIZE)
                .map(|i| {
                    audit_entry(
                        &format!("id-churn-{round}-{i}"),
                        1_700_000_000_000 + i as u64,
                        ConsoleOp::ConfigApply,
                        AuditResult::Ok,
                        "gateway.port",
                        Some(serde_json::json!(i)),
                        None,
                        None,
                    )
                })
                .collect();
            p.set_page(&audit_page(full, true, true, Some(1)));
            disp.refr_now_for_test();
            assert_eq!(
                p.visible_rows(),
                AUDIT_PAGE_SIZE,
                "第 {round} 轮：满行（{AUDIT_PAGE_SIZE} 条）必须建成"
            );
            assert_eq!(p.rows_alive(), AUDIT_PAGE_SIZE, "第 {round} 轮：行对象全部存活");
            drop(p);
        }

        // ── ⑮ **两页共存预算**（B2c-1 代码质量评审 ②；P3 未实现 ⇒ 用两个 P5 实例作代理）──
        // **为什么不是"两页各满行"**：实测（AU8 的 ②，2026-09-13）在 256 KB 堆下
        // 2 页 × 5 行尚可、**2 页 × 6 行即 OOM 挂死**，且"单页满行(20) + 第二个空页"也 OOM
        // ⇒ 两页各满行**不可能**（空页本身 ≈ 12 行的开销）。故共存网按实测预算
        // `COEXIST_ROWS_PER_PAGE`（= 4，对挂死点 6 留 ≥33% 余量）断言"两页**都能建成**"。
        // 「改什么会让本条变红」：把该常量抬到 6+ ⇒ 本段 OOM（挂死）；把页构造成本推高
        // （多建构件）⇒ 4 行也可能建不出 ⇒ 本段红。
        {
            let a = p5_audit::P5AuditPage::new(&host).expect("共存 A：建页");
            let b = p5_audit::P5AuditPage::new(&host).expect("共存 B：建页");
            let mk = |tag: &str| -> Vec<mupc_display_proto::ConsoleAuditEntry> {
                (0..p5_audit::COEXIST_ROWS_PER_PAGE)
                    .map(|i| {
                        audit_entry(
                            &format!("id-{tag}-{i}"),
                            1_700_000_000_000 + i as u64,
                            ConsoleOp::ConfigApply,
                            AuditResult::Ok,
                            "gateway.port",
                            Some(serde_json::json!(i)),
                            None,
                            None,
                        )
                    })
                    .collect()
            };
            a.set_page(&audit_page(mk("ca"), true, true, Some(1)));
            b.set_page(&audit_page(mk("cb"), true, true, Some(1)));
            disp.refr_now_for_test();
            for (what, p) in [("A", &a), ("B", &b)] {
                assert_eq!(
                    p.visible_rows(),
                    p5_audit::COEXIST_ROWS_PER_PAGE,
                    "共存页 {what}：{} 行必须整页可见（不 OOM）",
                    p5_audit::COEXIST_ROWS_PER_PAGE
                );
                assert_eq!(
                    p.rows_alive(),
                    p5_audit::COEXIST_ROWS_PER_PAGE,
                    "共存页 {what}：行对象全部存活"
                );
                assert_eq!(p.list_view(), p5_audit::ListView::Rows);
            }
            drop(a);
            drop(b);
        }

        // ── ⑯ **真件**：共享时间范围件「回调内自替换」全链（B2c-1 收口 ①）──────────────
        // **为什么另起一段真件**（不能只靠 `ui/pages/mod.rs::tests` 的机制用例）：机制用例
        // 是"机制桩"（自己 `take`、自己 `fire`）—— 调用点把槽换回"持 `try_borrow_mut` 借用
        // 直调"时它**照样全绿**（上一轮 ① 的探针就是因此没红）。本段用**真件 + 真事件**：
        // `SegmentedControl::VALUE_CHANGED` → `TimeRangeFilter::wire` 的闭包 →
        // `TimeRangeFilter::fire` → `CbSlot::fire` → 用户回调，**整条调用点**都在网内。
        // 「改什么会让本条变红」（**已实测**，见交付报告探针 ①B）：把 `CbSlot::fire` 换回
        // "调用期持 `try_borrow_mut` 直调" ⇒ 回调内 `set_on_change`（→ `CbSlot::set`）的
        // `try_borrow_mut` 失败 ⇒ 新回调**被静默丢弃** ⇒ **第 2 条断言**拿到
        // `["旧","旧尾","旧","旧尾"]`（不是 `["旧","旧尾","新"]`）。
        // 第 1 条断言是**另一条**失效模式的哨兵：若将来把 `CbSlot::set` 里的 `try_borrow_mut`
        // 换成 `borrow_mut`，自替换处会 panic 并被 `event.rs` 的 `catch_unwind` 吞掉 ⇒ 回调体
        // 半途截断 ⇒ 缺 `旧尾`。两条断言各管一种，**都不**是恒真式（①B 实测第 1 条仍绿、
        // 第 2 条红 —— 与上面的分工一致）。
        {
            let f = filters::build(&host, filters::TimeRangeChange::default())
                .expect("建共享时间范围件（真件自替换回归）");
            let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
            {
                let weak = Rc::downgrade(&f);
                let log = Rc::clone(&log);
                f.set_on_change(move |_c: filters::TimeRangeChange| {
                    log.borrow_mut().push("旧");
                    // 回调内**自替换**：此刻槽不得被任何借用持有。
                    if let Some(g) = weak.upgrade() {
                        let log = Rc::clone(&log);
                        g.set_on_change(move |_c: filters::TimeRangeChange| {
                            log.borrow_mut().push("新")
                        });
                    }
                    // 哨兵：回调体必须**跑完**（`borrow_mut` 变体会在此处 panic 并被事件桥吞掉）。
                    log.borrow_mut().push("旧尾");
                });
            }
            f.seg().send_event(EventCode::VALUE_CHANGED);
            assert_eq!(
                *log.borrow(),
                vec!["旧", "旧尾"],
                "本次通知必须由**旧**回调执行到**末尾**（缺 `旧尾` ⇒ 自替换处 panic 被 \
                 `event.rs` 的 catch_unwind 吞掉 = 回调半执行且无任何报错）"
            );
            f.seg().send_event(EventCode::VALUE_CHANGED);
            assert_eq!(
                *log.borrow(),
                vec!["旧", "旧尾", "新"],
                "新回调必须**自下一次通知起生效**（持借用直调 ⇒ 新回调被静默丢弃 ⇒ 这里仍是 `旧`）"
            );
            drop(f);
        }
    }

    // ═══ P3 日志页（B2c-2；F10，**只读** + 选项式筛选 + 共享时间范围件）══════════════
    //
    // 覆盖：装配契约（契约 1：页根即滚动容器）／通道条两态 + **重连不清内容**（LG-07）／
    // 筛选区三行（级别 4 chip / **模块换行网格** / 共享时间范围件）／**只读约束**（运行期读
    // LVGL 标志）／两态（行 / 空态，EDGE-08）／超限条（EDGE-15，契约字段 ⇒ **可达**）／
    // 意图回调（筛选变化**去重** + 增量拉取 + 「回到最新」）／行序（新行在顶部）／斑马纹 /
    // 级别色块 / 行高 / 列宽 / 所有权锚定 / `Rc` 环泄漏探针（C1）。
    {
        use crate::lvgl::widgets::{Dir, ScrollMode};
        use crate::ui::pages::{filters, p3_logs};
        use mupc_display_proto::{LogEntry, LogLevel, LogPage, LogRange};

        /// 造一条日志（`ts_ms` 随 `seq` 递增）。
        fn log_entry(seq: u64, level: LogLevel, target: &str, message: &str) -> LogEntry {
            LogEntry {
                seq,
                ts_ms: 1_789_047_727_000 + seq,
                level,
                target: target.into(),
                message: message.into(),
            }
        }

        /// 造一页日志。
        fn log_page(
            entries: Vec<LogEntry>,
            has_more: bool,
            range_too_large: bool,
        ) -> LogPage {
            LogPage {
                entries,
                next_cursor: None,
                has_more,
                range_too_large,
            }
        }

        let p3 = p3_logs::P3LogsPage::new(&host).expect("P3LogsPage::new");
        disp.refr_now_for_test();

        // ── ① 装配契约（**契约 1**：页根即滚动容器；§6.3 线框无底部操作条）─────────────
        assert_eq!(
            p3.obj().size(),
            (Dimens::CONTENT_W, Dimens::CONTENT_H),
            "页根 = 内容区视口 992×624"
        );
        let root_c = p3.obj().coords();
        assert_eq!(
            (root_c.x1, root_c.y1),
            (Dimens::SIDE_PAD, Dimens::HEADER_H),
            "页根由调用方摆放"
        );
        assert_eq!(
            crate::lvgl::widgets::scroll_dir(p3.obj()),
            Dir::VER,
            "**仅纵向**滚动（§2.7 / §7.4：全页禁横滚）"
        );
        assert_eq!(
            crate::lvgl::widgets::scrollbar_mode(p3.obj()),
            ScrollMode::AUTO,
            "滚动条纯指示、仅滚动时显现"
        );

        // ── ② 通道条（两态；缺省 = **断开**，fail-closed，见 LG8）─────────────────────
        assert!(!p3.channel_connected(), "未注入通道态 ⇒ 按未连接（不臆造「已连接」）");
        assert_eq!(
            p3.channel_text().as_deref(),
            Some(p3_logs::TEXT_CHANNEL_DOWN),
            "骨架态显断开文案（LG8）"
        );
        assert!(p3.channel_down_visible());
        assert_eq!(
            p3.channel_dot_colors(),
            (Palette::LINK_OK, Palette::LINK_DOWN),
            "灯色：已连接 #28A745 / 断开 #DC3545（PRD §3.1）"
        );
        p3.set_channel(true);
        disp.refr_now_for_test();
        assert_eq!(
            p3.channel_text().as_deref(),
            Some(p3_logs::TEXT_CHANNEL_OK),
            "已连接文案（§3.6 P3 通道条行逐字）"
        );
        assert!(!p3.channel_down_visible(), "两态互斥（各自独立对象）");

        // ── ③ 筛选区（三行：级别 / 模块 / 时间范围；**常驻**、无「应用」按钮）──────────
        assert_eq!(
            p3.level_label_text().as_deref(),
            Some(p3_logs::TEXT_LEVEL_LABEL)
        );
        assert_eq!(p3.level_chip_count(), 4, "① 级别 = 4 项（§6.3）");
        assert_eq!(p3.level_chip_columns(), 4, "四项一排（不换行、不横滚）");
        assert_eq!(
            p3.level_chip_display(0).as_deref(),
            Some(p3_logs::TEXT_LEVEL_ERROR),
            "未选中 ⇒ 无 `✓` 前缀（§5.2）"
        );
        assert_eq!(
            p3.level_chip_display(3).as_deref(),
            Some(p3_logs::TEXT_LEVEL_DEBUG)
        );
        assert!(p3.level_selected().is_empty(), "缺省全不选 = 不按级别筛");
        assert_eq!(
            p3.module_label_text().as_deref(),
            Some(p3_logs::TEXT_MODULE_LABEL)
        );
        assert_eq!(p3.module_chip_count(), 1, "未注入 targets ⇒ 仅「全部」");
        assert_eq!(
            p3.module_chip_display(0).as_deref(),
            Some(format!("{}全部", "✓").as_str()),
            "② 模块首位固定「全部」且**缺省勾选**（§6.3 ②）"
        );
        assert_eq!(p3.module_selected(), vec![0], "缺省 = 不按模块筛");
        assert_eq!(p3.module_grid_rows(), 1, "仅 1 项 ⇒ 1 行（§6.3：仅 1 行时高 48 px）");
        assert_eq!(
            p3.module_grid_size(),
            (Dimens::CONTENT_W, Dimens::CHIP_H),
            "网格**铺满内容宽 992**（LG3；右缘与表头 / 列表对齐）"
        );
        // 共享时间范围件（③）—— 接口与 P5 完全一致（P3 复用 `filters.rs`）。
        assert_eq!(
            p3.filter().obj().size().1,
            filters::body_h(LogRange::H1),
            "③ 时间范围缺省 = 最近 1 小时档（48 px）"
        );
        assert_eq!(p3.filter().seg().count(), 3, "三档（§5.1 #4）");

        // ── ④ 表头（4 列名 + 3 条竖分隔线 + 底线；所有权锚定）─────────────────────────
        assert!(p3.head_obj().is_alive());
        assert_eq!(p3.head_obj().size().1, 36, "表头 36 px（§6.3）");
        assert_eq!(
            p3.head_child_count(),
            8,
            "表头**实际子件数** = 4 列名 + 3 竖分隔线 + 1 底线（局部句柄被 `Drop` ⇒ 级联删子树 ⇒ 本条掉到 0）"
        );
        for (i, want) in [
            p3_logs::TEXT_HEAD_TIME,
            p3_logs::TEXT_HEAD_LEVEL,
            p3_logs::TEXT_HEAD_MODULE,
            p3_logs::TEXT_HEAD_MESSAGE,
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(
                p3.head_col_text(i).as_deref(),
                Some(*want),
                "第 {i} 列表头文案必须在屏上可读回（§6.3 线框：时间│级别│模块│消息）"
            );
        }
        assert_eq!(p3.head_divs_alive(), 3, "3 条竖分隔线全部存活");
        assert!(p3.head_rule_alive(), "表头底线存活");

        // ── ⑤ 空态（EDGE-08；骨架态 = 无注入）：**不得显「加载中」或空白** ─────────────
        assert_eq!(p3.list_view(), p3_logs::ListView::Empty);
        assert!(p3.empty_visible());
        assert_eq!(
            p3.empty_text().as_deref(),
            Some(p3_logs::TEXT_EMPTY),
            "EDGE-08：「当前筛选条件下无日志」"
        );
        assert_ne!(
            p3.empty_text().as_deref(),
            Some(p3_logs::TEXT_FOOTER_LOADING),
            "**不得**把空态显示成「加载中」（EDGE-08 明写「不得显未加载」；本行是探针 P2 的网）"
        );
        assert!(!p3.footer_visible(), "无行时不显状态行");
        assert!(
            p3.note_visible(),
            "「本地屏不支持日志导出 · 无文件与下载通道」**常驻**（§6.3「不支持导出」节）"
        );
        assert!(
            p3.note_y_in_list() >= p3.empty_h(),
            "空态下说明行必须在**空态之下**（y = {} ≥ 空态高 {} —— 否则说明行压在空态图标 / 文案上）",
            p3.note_y_in_list(),
            p3.empty_h()
        );
        assert_eq!(
            p3.note_text().as_deref(),
            Some(
                format!(
                    "{}{}{}",
                    p3_logs::TEXT_NO_EXPORT,
                    p3_logs::TEXT_CLAUSE_SEP,
                    p3_logs::TEXT_NO_EXPORT2
                )
                .as_str()
            )
        );
        assert!(!p3.warn_visible(), "超限条缺省不显（契约字段 = false）");
        assert!(!p3.back_visible(), "空态下无「最新」可回 ⇒ 按钮隐");

        // ── ⑥ 意图：增量拉取在**未见过任何条目**时不发（首屏归 `set_on_query`）─────────
        let inc = Rc::new(RefCell::new(Vec::new()));
        {
            let s = Rc::clone(&inc);
            p3.set_on_increment(move |q| s.borrow_mut().push(q));
        }
        p3.request_increment();
        assert_eq!(
            inc.borrow().len(),
            0,
            "**B3 尚未把本页拉起**（还没发过任何筛选意图）⇒ 不发增量（首屏归 set_on_query；LG13）"
        );
        assert!(!p3.increment_active(), "骨架态：增量路径未激活（LG13）");

        // ── ⑦ 注入 3 条（**乱序**注入 ⇒ 屏上按 `seq` 降序 = 新行在顶部）────────────────
        let entries = vec![
            log_entry(101, LogLevel::Error, "mupc_intercore", "核间心跳超时"),
            log_entry(103, LogLevel::Info, "hplc", "module=offline"),
            log_entry(102, LogLevel::Warn, "audit", "审计写入重试"),
        ];
        p3.set_page(&log_page(entries.clone(), true, false));
        disp.refr_now_for_test();
        assert_eq!(p3.list_view(), p3_logs::ListView::Rows);
        assert_eq!(p3.visible_rows(), 3, "三条注入 ⇒ 三行在显");
        assert_eq!(p3.rows_alive(), 3, "行对象**存活**（拥有型句柄的锚定回归锁）");
        assert!(!p3.empty_visible() && !p3.warn_visible());
        // **新行插入顶部**：`seq` 最大者（103）在第 0 行，最小者（101）在末行。
        assert_eq!(
            p3.row_time(0).as_deref(),
            Some(
                crate::ui::pages::format_epoch_ms_utc(1_789_047_727_103).as_str()
            ),
            "第 0 行 = `seq` 最大的那条（§6.3 实时追加：新行插入顶部）"
        );
        assert_eq!(
            p3.row_time(2).as_deref(),
            Some(
                crate::ui::pages::format_epoch_ms_utc(1_789_047_727_101).as_str()
            ),
            "末行 = 最旧的一条"
        );
        assert_eq!(p3.row_level(0).as_deref(), Some(p3_logs::TEXT_LEVEL_INFO));
        assert_eq!(p3.row_level(1).as_deref(), Some(p3_logs::TEXT_LEVEL_WARN));
        assert_eq!(p3.row_level(2).as_deref(), Some(p3_logs::TEXT_LEVEL_ERROR));
        assert_eq!(
            p3.row_level_color(1),
            Some(Palette::LOG_WARN),
            "级别色块颜色 = PRD 指定色（#FFC107）"
        );
        assert_eq!(p3.row_level_color(2), Some(Palette::LOG_ERROR));
        assert_eq!(
            p3.row_module(0).as_deref(),
            Some("hP?C"),
            "未登记模块名经 display_safe 归一（LG4：chip 预算 2 字，行预算 5 字）"
        );
        assert_eq!(
            p3.row_module(1).as_deref(),
            Some(p3_logs::TEXT_MODULE_AUDIT),
            "已知模块名 ⇒ 中文标签"
        );
        assert_eq!(
            p3.row_module(2).as_deref(),
            Some(p3_logs::TEXT_MODULE_INTERCORE),
            "契约示例的 `mupc_` 前缀形态同样命中（归一表）"
        );
        assert_eq!(
            p3.row_message(0).as_deref(),
            Some("MODU?E?OFF?INE"),
            "消息经 free_text_safe（小写 → 大写同族；`l` 与 `=` 无字形 ⇒ `?` —— D9 的残余，\
             由 `p3_runtime_texts_emit_only_cmap_glyphs` 保证不出豆腐块）"
        );
        assert_eq!(p3.row_message(2).as_deref(), Some("核间心跳超时"));
        // 行几何：行高 44（§6.3 / LG-06）、级别色块 88×28、四列不重叠。
        assert_eq!(p3.row_size(0), Some((Dimens::CONTENT_W, Dimens::ROW_LOG_H)));
        assert_eq!(
            p3.row_pos(1).map(|(_, y)| y - p3.row_pos(0).unwrap().1),
            Some(Dimens::ROW_LOG_H),
            "行距 = 行高 44（窗口化按定高摆放）"
        );
        assert_eq!(
            p3.row_level_block_size(0),
            Some((p3_logs::ROW_LEVEL_W, 28)),
            "级别色块 88×28（§6.3 行规格）"
        );
        // 斑马纹（§6.3：偶行 #141F33 / 奇行 #1B2942）。
        assert_eq!(p3.row_stripe(0), Some(Palette::SURFACE), "偶行 #141F33");
        assert_eq!(p3.row_stripe(1), Some(Palette::SURFACE_ALT), "奇行 #1B2942");
        assert_eq!(p3.row_stripe(2), Some(Palette::SURFACE));
        // 底部状态行（§3.6 P3 列表/状态行；LG7 的落点）。
        assert_eq!(
            p3.footer_text().as_deref(),
            Some(p3_logs::TEXT_FOOTER_LOADING),
            "has_more = true ⇒ 加载中"
        );
        assert!(p3.footer_visible());
        p3.set_page(&log_page(entries.clone(), false, false));
        assert_eq!(
            p3.footer_text().as_deref(),
            Some(p3_logs::TEXT_FOOTER_ALL),
            "has_more = false ⇒ 已加载全部"
        );
        // 「回到最新」按钮（**恒显**；R2 的可实现部分）。
        assert!(p3.back_visible());
        assert!(p3.back_clickable(), "按钮必须**可点**（正向控制）");
        assert_eq!(p3.back_size(), (92, 92), "92×92（§6.3）");
        assert_eq!(
            p3.back_text().as_deref(),
            Some(p3_logs::TEXT_BACK_TO_LATEST)
        );

        // ── ⑧ **只读约束**（§6.3「不支持导出」节 / PRD T-2）：运行期读 LVGL 标志 ─────────
        // 「改什么会让本条变红」：给行加任何可点子对象（按钮 / 可点容器 / 长按菜单入口）
        // ⇒ `clickable_parts` > 0 ⇒ 本条立刻红。
        assert_eq!(
            p3.rows_clickable_parts(),
            0,
            "列表行**不得**存在任何可点对象（无编辑 / 删除 / 清空 / 导出入口）"
        );
        assert_eq!(p3_logs::P3LogsPage::WRITE_ENTRIES.len(), 0);

        // ── ⑨ 通道断（**LG-07：重连期间不清空已展示内容**）───────────────────────────
        p3.set_channel(false);
        disp.refr_now_for_test();
        assert_eq!(
            p3.visible_rows(),
            3,
            "LG-07：通道断**不得**清空已展示内容（只改通道条）"
        );
        assert_eq!(p3.rows_alive(), 3);
        assert_eq!(
            p3.channel_text().as_deref(),
            Some(p3_logs::TEXT_CHANNEL_DOWN)
        );
        p3.set_channel(true);

        // ── ⑩ **模块换行网格**（§6.3 ② / §2.7：不横滚、全项可达）──────────────────────
        // 注入 20 个模块（含已知 / 未登记 / 契约示例形态）⇒ 21 个 chip（含「全部」）。
        let targets: Vec<String> = (0..20)
            .map(|i| match i {
                0 => "mupc_intercore".to_string(),
                1 => "mupc_gateway".to_string(),
                2 => "audit".to_string(),
                _ => format!("mod_{i}"),
            })
            .collect();
        p3.set_targets(&targets);
        disp.refr_now_for_test();
        assert_eq!(p3.injected_targets().len(), 20);
        assert_eq!(p3.module_chip_count(), 21, "「全部」+ 20 项");
        assert_eq!(p3.module_chip_columns(), 8, "8 项/行（§6.3 ② 的保守核算）");
        assert_eq!(
            p3.module_grid_rows(),
            (21 + 7) / 8,
            "21 项 ⇒ 3 行（**多行**而非横滚）"
        );
        assert!(
            p3.module_grid_rows() >= 2,
            "≥20 个模块必须**换行**（若行数为 1，说明退化成横滚 / 截断）"
        );
        assert!(
            p3.module_grid_rows() <= 7,
            "行数不得超过 §6.3 ② 的 7 行上限"
        );
        let (gw, _gh) = p3.module_grid_size();
        assert!(
            gw <= Dimens::CONTENT_W,
            "网格宽 {gw} ≤ 内容宽 {} ⇒ **不横滚**（全部模块可达）",
            Dimens::CONTENT_W
        );
        assert_eq!(p3.module_chip_display(1).as_deref(), Some("核间"));
        assert_eq!(
            p3.module_chip_display(2).as_deref(),
            Some("主站"),
            "已知键取中文标签"
        );
        assert_eq!(p3.module_chip_display(3).as_deref(), Some("审计"));
        // 未登记键（`mod_3` ⇒ 归一 `MOD?3`，4 字）：chip 上 = **`...` + 尾 1 字**（LG4）
        // —— **超出即带可见省略标记**（B2c-2 规格评审整改 ⑤：原方案"保尾 2 字、**无标记**"）。
        let chip4 = p3.module_chip_display(4).expect("未登记模块 chip");
        let norm4 = p3_logs::module_label("mod_3");
        assert_eq!(
            chip4,
            format!("...{}", norm4.chars().next_back().unwrap()),
            "超预算 ⇒ `...` + 尾 1 字（归一形态 `{norm4}` 的末字）"
        );
        assert!(
            chip4.starts_with("..."),
            "超预算的 chip 文案**必须**带可见省略标记（LG4；退回纯保尾 ⇒ 本条红）"
        );
        assert_eq!(chip4.chars().count(), 4, "产物 = `...`（3 字）+ 尾 1 字（LG4）");
        // 选项文案清册（读回）：首位恒「全部」、逐项不超预算（**原样预算 + 标记**）、数量与注入一致。
        let opts = p3.module_option_texts();
        assert_eq!(opts.len(), 21);
        assert_eq!(opts[0], p3_logs::TEXT_ALL);
        for o in &opts {
            assert!(
                o.chars().count()
                    <= p3_logs::MODULE_CHIP_MAX_CHARS + "...".chars().count(),
                "chip 文案 `{o}` 超预算（LG4）"
            );
            // 逐项自证：**超预算的项一律以标记开头**（"不完整"必须可见）。
            assert!(
                o.chars().count() <= p3_logs::MODULE_CHIP_MAX_CHARS || o.starts_with("..."),
                "chip 文案 `{o}` 超预算却**无可见标记**（LG4 / §2.6）"
            );
        }
        // **区分度（LG12）**：两个"尾巴不同"的长机器名 ⇒ chip 产物**必须不同**（可见化补偿的
        // 最低要求）；而"头尾都同、只差中段"的两个名字**仍会同形** —— 该残余由
        // `p3_logs::tests::marked_truncation_always_shows_the_marker` 与 LG12 明写，不在本链断言。
        {
            let a = p3_logs::clip_chip_label(&p3_logs::module_label("mupc_gateway::iec104"));
            let b = p3_logs::clip_chip_label(&p3_logs::module_label("mupc_gateway::rs485"));
            assert_ne!(a, b, "尾字不同的两个长名 ⇒ chip 产物必须可区分");
            let ra = p3_logs::clip_row_label(&p3_logs::module_label("mupc_gateway::iec104"));
            let rb = p3_logs::clip_row_label(&p3_logs::module_label("mupc_gateway::rs485"));
            assert_ne!(ra, rb, "尾字不同的两个长名 ⇒ 行产物必须可区分");
            // 已知残余（**LG12**，如实锁定）：头尾都相同、只差中段 ⇒ 仍同形。
            let ca = p3_logs::clip_chip_label(&p3_logs::module_label(
                "mupc_data_processing::iec104",
            ));
            assert_eq!(
                a, ca,
                "**已登记的撞形残余（LG12）**：`mupc_gateway::iec104` 与 \
                 `mupc_data_processing::iec104` 头（`mupc_`）尾（`::iec104`）都相同 ⇒ 字符预算内\
                 取不到中段。**若本条变红**（两者已可区分）⇒ 说明撞形被根治，请同步更新 LG12"
            );
        }

        // ── ⑪ 意图回调：筛选变化（**变化才发、未变化不发**）────────────────────────────
        let fires = Rc::new(RefCell::new(Vec::new()));
        {
            let f = Rc::clone(&fires);
            p3.set_on_query(move |q| f.borrow_mut().push(q));
        }
        // 级别：勾选 ERROR（走页面侧分派路径 —— chip 的点击事件由 `components.rs` 承担，
        // 离屏链拿不到 chip 的 `Obj`，见 `dispatch_level_selection` 的文档）。
        p3.dispatch_level_selection(vec![0]);
        assert_eq!(fires.borrow().len(), 1, "筛选变化 ⇒ **恰发一次**意图");
        {
            let q = &fires.borrow()[0];
            assert_eq!(q.levels, vec![LogLevel::Error]);
            assert_eq!(q.cursor, None, "筛选变化 ⇒ 全新查询（无游标）");
            assert_eq!(q.range, LogRange::H1);
            assert_eq!(q.limit, p3_logs::ROW_MAX, "limit = 本页行池上界（LG6）");
        }
        // **未变化时不发意图**：再派发同一选择 ⇒ 不重发。
        // 「改什么会让本条变红」：去掉 `Core::fire_query` 里的去重判断 ⇒ 本条立刻红。
        p3.dispatch_level_selection(vec![0]);
        assert_eq!(
            fires.borrow().len(),
            1,
            "同一筛选条件**不重复**发意图（去重）"
        );
        // 模块：「全部 + 具体项」归一化（点具体项 ⇒ 让出「全部」）。
        p3.dispatch_module_selection(vec![0, 2]);
        assert_eq!(fires.borrow().len(), 2);
        assert_eq!(p3.module_selected(), vec![2], "归一化：让出「全部」");
        {
            let q = &fires.borrow()[1];
            assert_eq!(
                q.targets,
                vec!["mupc_gateway".to_string()],
                "查询携带**机器键**（下标 2 − 1 = targets[1]）"
            );
            assert_eq!(q.levels, vec![LogLevel::Error], "级别筛选沿用当前态");
        }
        // 「全部」快捷复位（1 次触摸清空该维度）。
        p3.dispatch_module_selection(vec![0, 2]);
        assert_eq!(p3.module_selected(), vec![0]);
        assert_eq!(fires.borrow().len(), 3);
        assert!(fires.borrow()[2].targets.is_empty(), "「全部」⇒ 不按模块筛");
        // 时间范围：档位变化（共享件的事件路径 —— 与 P5 同一入口）。
        p3.filter().seg().set_selected(1);
        p3.filter().seg().send_event(EventCode::VALUE_CHANGED);
        assert_eq!(fires.borrow().len(), 4);
        assert_eq!(fires.borrow()[3].range, LogRange::H24);
        // 「自定义」⇒ 展开步进器 + 意图携带起止（**页面不读时钟** ⇒ 起止为可表示全区间）。
        p3.filter().seg().set_selected(2);
        p3.filter().seg().send_event(EventCode::VALUE_CHANGED);
        disp.refr_now_for_test();
        assert_eq!(fires.borrow().len(), 5);
        assert_eq!(fires.borrow()[4].range, LogRange::Custom);
        assert!(fires.borrow()[4].from_ms.is_some() && fires.borrow()[4].to_ms.is_some());
        assert!(p3.filter().custom_visible(), "「自定义」档必须展开步进器");
        assert_eq!(
            p3.filter().obj().size().1,
            filters::body_h(LogRange::Custom),
            "展开后体高 48 → 284（其下区块由 layout() 重摆）"
        );
        // 回到「最近 1 小时」（复原）。
        p3.filter().seg().set_selected(0);
        p3.filter().seg().send_event(EventCode::VALUE_CHANGED);
        assert_eq!(fires.borrow().len(), 6);

        // ── ⑫ 增量拉取（设计 §4.4：`cursor` = **已见最大 `seq`**）──────────────────────
        p3.set_page(&log_page(entries.clone(), true, false));
        assert_eq!(p3.last_seq(), 103, "游标 = 注入窗口的最大 `seq`");
        p3.request_increment();
        assert_eq!(inc.borrow().len(), 1, "有游标 ⇒ 发一次增量意图");
        {
            let q = &inc.borrow()[0];
            assert_eq!(q.cursor, Some(103), "游标不是 `next_cursor`（契约语义不同）");
            assert_eq!(q.range, LogRange::H1, "增量沿用当前筛选");
            assert_eq!(q.levels, vec![LogLevel::Error], "级别筛选沿用当前态");
            assert_eq!(q.limit, p3_logs::ROW_MAX);
        }
        // 筛选变化后游标**清零**（新窗口整体替换 ⇒ 旧游标无意义），但**增量路径不得停摆**
        // （**B2c-2 规格评审整改 ⑦ / LG13**）。
        //
        // 「改什么会让本条变红」：把 `fire_increment` 的判据写回 `last_seq == 0 ⇒ return`
        // （原实现）⇒ 下面的 `inc.borrow().len()` 不会增长 ⇒ **当场红**（原有用例正是把这个
        // 停摆反向锁死了，本批已改为正向断言）。
        p3.dispatch_level_selection(vec![]);
        assert_eq!(p3.last_seq(), 0, "筛选变化 ⇒ 游标清零");
        assert!(p3.increment_active(), "筛选意图已发出 ⇒ 增量路径激活（LG13）");
        p3.request_increment();
        assert_eq!(
            inc.borrow().len(),
            2,
            "游标清零后增量**仍要能推进**（空窗口下「继续」= 重新拉首页）"
        );
        {
            let q = &inc.borrow()[1];
            assert_eq!(q.cursor, None, "无游标 ⇒ `cursor = None`（重新拉首页，不是停摆）");
            assert_eq!(q.range, LogRange::H1, "沿用当前筛选档位");
            assert!(q.levels.is_empty(), "沿用当前级别筛选（已全部取消）");
            assert_eq!(q.limit, p3_logs::ROW_MAX);
        }
        // 再次触发：窗口仍空（B3 还没回灌新窗口）⇒ 仍以 `None` 继续推进（**每次都推进**，
        // 不再出现"空窗口之后永久沉默"）。
        p3.request_increment();
        assert_eq!(inc.borrow().len(), 3);
        assert_eq!(inc.borrow()[2].cursor, None, "窗口仍空 ⇒ 仍为 None（直到注入新窗口）");
        // 注入新窗口 ⇒ 游标回到 `max(seq)`，增量恢复"真增量"形态（`Some(..)`）。
        p3.set_page(&log_page(entries.clone(), true, false));
        assert_eq!(p3.last_seq(), 103);
        p3.request_increment();
        assert_eq!(inc.borrow().len(), 4);
        assert_eq!(inc.borrow()[3].cursor, Some(103), "有游标 ⇒ 回到 max(seq) 增量语义");

        // ── ⑬ 「回到最新」（**R2** 的可实现部分：点击 ⇒ 意图 + 复位 auto_follow）────────
        let backs = Rc::new(RefCell::new(0usize));
        {
            let b = Rc::clone(&backs);
            p3.set_on_back_to_latest(move || *b.borrow_mut() += 1);
        }
        p3.set_auto_follow(false);
        assert!(!p3.auto_follow());
        p3.back_obj().send_event(EventCode::CLICKED);
        assert_eq!(*backs.borrow(), 1, "点击 ⇒ **恰一次**意图");
        assert!(p3.auto_follow(), "点击后复位自动跟随（B3 注入态的读回口径）");
        // ⚠️ **R2 的能力缺口（T21c-2-r1 订正后如实标注）**：本段**不断言**"手动上滚 ⇒ 停止
        // 自动跟随"—— 缺口**不是**"读不到滚动事件或位置"（`EventCode::SCROLL` 自 **B4b
        // `297b51b`** 起已镜像；`Obj::scroll_to_y` / `scroll_y` 自 **B4a** 起已封装，见
        // `lvgl/mod.rs` 的 **G3** —— B4a 那一轮写的"剩余缺口只有输入侧事件"**同样不成立**），
        // 而是**页内浮动件**（页根即滚动容器 ⇒ 按钮随内容滚走；见 `p3_logs.rs` 的 **LG11** /
        // `ui/pages/mod.rs` 的 R2）。
        // 本段**不**断言"回到顶部"：那是**外壳行为**、不在本段（页面层）的断言范围，
        // 在此断言只会是恒真式。

        // ── ⑭ 超限（EDGE-15）：契约字段 ⇒ **生产可达**（与 P5 的 AU5 相反）─────────────
        p3.set_page(&log_page(entries.clone(), false, true));
        disp.refr_now_for_test();
        assert!(p3.warn_visible(), "`range_too_large = true` ⇒ 列表区上方出现 WarnBanner");
        assert_eq!(
            p3.warn_text().as_deref(),
            Some(p3_logs::TEXT_RANGE_TOO_LARGE)
        );
        assert_eq!(p3.visible_rows(), 3, "超限**不隐藏**已返回的条目（契约：`entries` 不代表完整结果）");
        p3.set_page(&log_page(entries.clone(), false, false));
        assert!(!p3.warn_visible(), "标志复位 ⇒ 条隐（**常驻构件**，只改可见性）");

        // ── ⑮ 空态（EDGE-08）：`entries` 空 + `range_too_large = false` ────────────────
        p3.set_page(&log_page(Vec::new(), false, false));
        disp.refr_now_for_test();
        assert_eq!(p3.list_view(), p3_logs::ListView::Empty);
        assert!(p3.empty_visible() && !p3.warn_visible());
        assert_eq!(
            p3.empty_text().as_deref(),
            Some(p3_logs::TEXT_EMPTY),
            "EDGE-08 逐字"
        );
        assert_eq!(p3.visible_rows(), 0, "空态不显行");
        assert!(!p3.back_visible(), "空态无「最新」可回 ⇒ 按钮隐");
        assert!(
            !p3.incomplete_visible(),
            "**正向对照**：`range_too_large = false` 的空窗口 ⇒ **不得**显「不完整」态\
             （否则正常空态会被误判成超限）"
        );

        // ── ⑮′ **空态 × 超限互斥**（B2c-2 规格评审整改 ①；评审探针 `PROBE-EDGE08-15` 的现场）
        //
        // 原缺陷：`entries = [] ∧ range_too_large = true` ⇒ 空态「当前筛选条件下无日志」与
        // 超限条**同时**在显 —— 而 `range_too_large = true` 的语义是"**本次未执行全库检索、
        // `entries` 不代表完整结果**" ⇒ 说"确实没有"就是把"无法获知"冒充成"确实没有"
        // （违 §8.3「语义不同的态必须可区分、不得互替」+ §2.6「降级可见、绝不造假」）。
        //
        // 「改什么会让本条变红」：把 `list_view_of` 写回"只看行数"（忽略 `range_too_large`）
        // ⇒ 下面第 ① 组断言（空态**不可见**）当场红。
        {
            // ① 零行 + 超限 ⇒ **不完整态**：超限条在显、空态**不可见**、中性文案在显。
            p3.set_page(&log_page(Vec::new(), false, true));
            disp.refr_now_for_test();
            assert!(p3.warn_visible(), "超限 ⇒ 超限提示条必须在显");
            assert!(
                !p3.empty_visible(),
                "**超限优先**：零行 + 超限时**不得**显空态「当前筛选条件下无日志」\
                 （那是把「无法获知」冒充成「确实没有」—— §8.3 / §2.6）"
            );
            assert!(p3.incomplete_visible(), "改显中性文案（LG9）");
            assert_eq!(p3.list_view(), p3_logs::ListView::Incomplete);
            assert_eq!(
                p3.incomplete_text().as_deref(),
                Some(p3_logs::TEXT_INCOMPLETE)
            );
            assert_ne!(
                p3.incomplete_text().as_deref(),
                Some(p3_logs::TEXT_EMPTY),
                "中性文案**不得**复用「无日志」那句（语义互替）"
            );
            assert_eq!(p3.visible_rows(), 0);
            // ② **正向对照**：同一 `entries = []`，只把标志复位 ⇒ 空态回来、不完整态下去。
            p3.set_page(&log_page(Vec::new(), false, false));
            disp.refr_now_for_test();
            assert_eq!(p3.list_view(), p3_logs::ListView::Empty);
            assert!(p3.empty_visible() && !p3.warn_visible() && !p3.incomplete_visible());
            // ③ 有行 + 超限 ⇒ **行态**（超限不隐藏已返回的条目；也不给不完整态）。
            p3.set_page(&log_page(entries.clone(), false, true));
            disp.refr_now_for_test();
            assert_eq!(p3.list_view(), p3_logs::ListView::Rows);
            assert_eq!(p3.visible_rows(), 3);
            assert!(!p3.incomplete_visible());
        }

        // ── ⑮″ **长消息的截断必须可见**（B2c-2 规格评审整改 ③④ / **LG5 / LG14**）──────────
        //
        // 消息列 = `LongMode::DOTS`（行高恒 44 px 下 `WRAP` 会**静默裁掉第二行**）。
        // `DOTS` 由 LVGL 把**溢出的尾部换成 `.`×3**，交换进 `label->text` 的**同一缓冲区**
        // ⇒ `Label::text()`（`lv_label_get_text`）能读回"被截断"这一**可见后果**。
        //
        // 「改什么会让本条变红」：把消息列改回 `WRAP` ⇒ 读回串是**完整原文**、不以 `...` 结尾
        // ⇒ 第 ① 组断言当场红（这正是原静态锁做不到的"区分度"）。
        {
            let long_msg = "abcdefghijklmnopqrstuvwxyz0123456789".repeat(3); // 108 字
            let one = vec![log_entry(1, LogLevel::Info, "audit", &long_msg)];
            p3.set_page(&log_page(one, false, false));
            disp.refr_now_for_test();
            let shown = p3.row_message(0).expect("长消息行");
            assert!(
                shown.ends_with("..."),
                "长消息必须**可见地**被截断（`DOTS` 把尾部换成 `...`）；实际读回 = `{shown}`\
                 （若为完整原文 ⇒ 消息列多半被改回了 `WRAP`，见 LG5）"
            );
            assert!(
                shown.len() < 108,
                "截断后的读回串必须**短于**原文（实际 {} 字）",
                shown.len()
            );
            // 短消息（放得下一行）⇒ **不**加省略号（不得无谓截断）。
            let short = vec![log_entry(2, LogLevel::Info, "audit", "核间心跳超时")];
            p3.set_page(&log_page(short, false, false));
            disp.refr_now_for_test();
            assert_eq!(
                p3.row_message(0).as_deref(),
                Some("核间心跳超时"),
                "短消息逐字原样（无 `...`）"
            );
        }

        // ── ⑮‴ 「回到最新」按钮**不得与任何一行相交**（整改 ② / **LG11**）──────────────────
        //
        // 评审实测：原实现按钮屏坐标 (916,380)-(1007,471)，而第 0/1 行占 y372-416 / y416-460
        // ⇒ 压住消息列尾部 92 px（且底 `SURFACE_HIGH` **不透明** ⇒ 实遮正文）。
        // 现按钮独占列表区之下的**一条带** ⇒ 与**每一行**的矩形都不相交。
        //
        // 「改什么会让本条变红」：把按钮的 y 改回 `y_list + TIGHT_GAP`（叠在行上）⇒ 第 ① 组
        // 断言当场红。
        {
            let full: Vec<LogEntry> = (0..p3_logs::ROW_MAX)
                .map(|i| log_entry(1_900_000_000_000 + i as u64, LogLevel::Info, "audit", "m"))
                .collect();
            p3.set_page(&log_page(full, false, false));
            disp.refr_now_for_test();
            let b = p3.back_obj().coords();
            assert!(p3.back_visible(), "有行 ⇒ 按钮在显（恒显口径，LG11）");
            assert!(b.x2 > b.x1 && b.y2 > b.y1, "按钮矩形非退化：{b:?}");
            assert_eq!(p3.visible_rows(), p3_logs::ROW_MAX);
            for i in 0..p3.visible_rows() {
                let (x, y) = p3.row_pos(i).expect("行坐标");
                let (w, h) = p3.row_size(i).expect("行尺寸");
                let (rx2, ry2) = (x + w, y + h);
                let disjoint = b.x2 <= x || rx2 <= b.x1 || b.y2 <= y || ry2 <= b.y1;
                assert!(
                    disjoint,
                    "第 {i} 行矩形 ({x},{y})-({rx2},{ry2}) 与「回到最新」按钮矩形 \
                     ({},{})-({},{}) **相交** —— 按钮实遮日志正文（不透明底），见 LG11",
                    b.x1, b.y1, b.x2, b.y2
                );
            }
            // 按钮**整体**在最后一行之下（带语义的结构性自证：不是"碰巧不相交"）。
            let last_bottom = p3.row_pos(p3.visible_rows() - 1).expect("末行").1
                + p3.row_size(p3.visible_rows() - 1).expect("末行尺寸").1;
            assert!(
                b.y1 >= last_bottom,
                "按钮的 y1 = {} 必须 ≥ 末行底 {}（专属带在列表区之下）",
                b.y1,
                last_bottom
            );
        }

        // ── ⑯ **行池预算**（**LG6** / **R3**）：注入契约上限的条数也只渲染行池上界 ────────
        // 实测（见 `p3_logs::MEASURED_ROW_CAPACITY`）：单页独活时 P3 可容 **44 行**（48 挂死）；
        // `ROW_MAX` = 20 是对挂死点留 >50% 余量的保守值（纯逻辑断言另见
        // `p3_logs::tests::row_pool_capacity_is_measured`，那条改坏时**当场红**、不会挂死）。
        {
            let full: Vec<LogEntry> = (0..p3_logs::ROW_MAX)
                .map(|i| {
                    log_entry(
                        1_700_000_000_000 + i as u64,
                        LogLevel::Debug,
                        "gateway",
                        "x",
                    )
                })
                .collect();
            p3.set_page(&log_page(full, true, false));
            disp.refr_now_for_test();
            assert_eq!(
                p3.visible_rows(),
                p3_logs::ROW_MAX,
                "注入行池上界（{}）⇒ 整页可见（不 OOM）",
                p3_logs::ROW_MAX
            );
            assert_eq!(p3.rows_alive(), p3_logs::ROW_MAX, "行对象全部存活");
            // 超过上界 ⇒ **只渲染上界条**（有界，不增长；`set_page` 是"整体替换窗口"语义）。
            let over: Vec<LogEntry> = (0..p3_logs::ROW_MAX * 3)
                .map(|i| {
                    log_entry(
                        1_800_000_000_000 + i as u64,
                        LogLevel::Info,
                        "hplc",
                        "y",
                    )
                })
                .collect();
            p3.set_page(&log_page(over, false, false));
            disp.refr_now_for_test();
            assert_eq!(
                p3.row_pool_len(),
                p3_logs::ROW_MAX,
                "行池**有界**：注入 3 倍也只建一页（LG6）"
            );
            assert_eq!(p3.visible_rows(), p3_logs::ROW_MAX);
        }

        // 渲染后确有像素（装配 → 布局 → 像素全链）。
        let painted3 = sink.borrow().iter().filter(|b| **b != 0).count();
        assert!(
            painted3 > 10_000,
            "P3 渲染后 sink 中应有成片非背景像素（实际 {painted3}）"
        );

        // ── ⑰ **`drop(P3LogsPage)` 必须释放整页**（**C1**：`Rc<Core>` 强引用环的回归网）────
        // 「改什么会让本条变红」：把模块 chip 回调槽里的 `Weak<Core>` 改回 `Rc<Core>` ——
        // 环 = `Core → modules → chips.on_change → Rc<Core>` ⇒ `drop` **不释放任何对象** ⇒
        // 下面第 2 组断言（`!is_alive()`）**当场红**（且第 3 段的 churn 会 OOM 挂死）。
        {
            let probes = [
                ("root（页根滚动容器）", p3.obj().share_borrowed()),
                ("通道条（已连接文案）", p3.channel_ok_obj().share_borrowed()),
                ("级别 chip 组容器", p3.level_box_obj().share_borrowed()),
                ("模块 chip 组容器", p3.module_box_obj().share_borrowed()),
                ("共享时间范围件", p3.filter().obj().share_borrowed()),
                ("超限提示条", p3.warn_obj().share_borrowed()),
                ("表头", p3.head_obj().share_borrowed()),
                ("列表容器", p3.list_obj().share_borrowed()),
                ("「回到最新」按钮", p3.back_obj().share_borrowed()),
            ];
            for (what, o) in &probes {
                assert!(o.is_alive(), "探针建立时 `{what}` 必须存活");
            }
            drop(p3);
            for (what, o) in &probes {
                assert!(
                    !o.is_alive(),
                    "`drop(P3LogsPage)` 之后 `{what}` 必须已被级联删除 —— 仍存活 ⇒ 存在 \
                     `Rc` 强引用环（整页泄漏；C1）"
                );
            }
        }

        // ── ⑱ **连续建 / 拆 P3 页（满行）必须全部成功**（C1 的生产路径回归网）────────────
        for round in 0..3 {
            let p = p3_logs::P3LogsPage::new(&host).expect("第 N 轮建 P3 页（环存在时这里 OOM）");
            let full: Vec<LogEntry> = (0..p3_logs::ROW_MAX)
                .map(|i| {
                    log_entry(1_700_000_000_000 + i as u64, LogLevel::Info, "audit", "z")
                })
                .collect();
            p.set_page(&log_page(full, true, false));
            disp.refr_now_for_test();
            assert_eq!(
                p.visible_rows(),
                p3_logs::ROW_MAX,
                "第 {round} 轮：满行（{} 条）必须建成",
                p3_logs::ROW_MAX
            );
            assert_eq!(p.rows_alive(), p3_logs::ROW_MAX, "第 {round} 轮：行对象全部存活");
            drop(p);
        }

        // ── ⑲ **P3 + P5 两页共存预算**（**R3**；B2c-1 的 AU8 教训：256 KB 堆里"第二个满行页
        //        即 OOM 挂死"）──────────────────────────────────────────────────────────
        // 实测口径：**一个 P5（`p5_audit::COEXIST_ROWS_PER_PAGE` 行）+ 一个 P3
        // （`p3_logs::COEXIST_ROWS_PER_PAGE` 行）** 同时存活 —— 两个列表页的**行构造成本不同**
        // （P5 每行 ≈ 10 个对象：行 + 竖条 + 5 文字 + 2 个胶囊（各 2 对象）；P3 每行 = 6 个对象：
        // 行 + 色块 + 4 文字）⇒ 共存预算**分别标定**，P3 不沿用 AU8 的 4（实测见交付报告）。
        // 「改什么会让本条变红」：把 `p3_logs::COEXIST_ROWS_PER_PAGE` 抬到超过共存堆预算 ⇒
        // 本段 OOM（**挂死**，不是红）；把页构造成本推高（多建构件）⇒ 同样行数也可能建不出。
        {
            use crate::ui::pages::p5_audit;
            use mupc_display_proto::{AuditPage, AuditResult, ConsoleAuditEntry, ConsoleOp};

            let p3c = p3_logs::P3LogsPage::new(&host).expect("共存：建 P3 页");
            let p5c = p5_audit::P5AuditPage::new(&host).expect("共存：建 P5 页");
            let p3_full: Vec<LogEntry> = (0..p3_logs::COEXIST_ROWS_PER_PAGE)
                .map(|i| log_entry(1_700_000_000_000 + i as u64, LogLevel::Info, "audit", "c"))
                .collect();
            p3c.set_page(&log_page(p3_full, true, false));
            let p5_full: Vec<ConsoleAuditEntry> = (0..p5_audit::COEXIST_ROWS_PER_PAGE)
                .map(|i| ConsoleAuditEntry {
                    id: format!("ca-{i}"),
                    ts_ms: 1_700_000_000_000 + i as u64,
                    operator: mupc_display_proto::CONSOLE_OPERATOR.into(),
                    op: ConsoleOp::ConfigApply,
                    target: "gateway.port".into(),
                    before: None,
                    after: None,
                    result: AuditResult::Ok,
                    reason: None,
                    request_id: "rid-c".into(),
                })
                .collect();
            p5c.set_page(&AuditPage {
                entries: p5_full,
                page: 1,
                page_size: mupc_display_proto::AUDIT_PAGE_SIZE as u32,
                has_more: false,
                newest_ts_ms: Some(1),
                available: true,
            });
            disp.refr_now_for_test();
            assert_eq!(
                p3c.visible_rows(),
                p3_logs::COEXIST_ROWS_PER_PAGE,
                "共存：P3 页 {} 行必须整页可见（不 OOM）",
                p3_logs::COEXIST_ROWS_PER_PAGE
            );
            assert_eq!(p3c.rows_alive(), p3_logs::COEXIST_ROWS_PER_PAGE);
            assert_eq!(
                p5c.visible_rows(),
                p5_audit::COEXIST_ROWS_PER_PAGE,
                "共存：P5 页 {} 行必须整页可见（不 OOM）",
                p5_audit::COEXIST_ROWS_PER_PAGE
            );
            assert_eq!(p5c.rows_alive(), p5_audit::COEXIST_ROWS_PER_PAGE);
            drop(p3c);
            drop(p5c);
        }

        // ── ⑥′ **6 页共存**（外壳 B2c-3 的硬前提，**不变量回归锁**）──────────────
        //
        // 外壳要「同时持有 6 页、靠显隐切换」。这**不是**可推定的性质：LVGL 对象树
        // 分配自定容池 `LV_MEM_SIZE`，早期 256 KB 时**建到第 4 页即 `lv_realloc`
        // 失败**（本用例当时红过）。故只要有人把 `lvgl-sys/lv_conf.h` 的
        // `LV_MEM_SIZE` 调回去、或给某页加出大量常驻对象，这里就会红。
        //
        // 断言刻意写成「**建完就验内容**」而不是"构造没报错"——构造成功只说明分配
        // 没失败，不说明该页真拿到了数据（本项目出过"局部句柄被 drop ⇒ 屏上空白、
        // 而测试全绿"的缺陷类）。
        //
        // ⚠️ 构建顺序**不要改**：按 P1→P6→P2→P4→P5→P3 复现"第 4 页失败"的原始序列。
        {
            use crate::ui::pages::p3_logs::P3LogsPage;
            use crate::ui::pages::p4_interlock::P4InterlockPage;
            use crate::ui::pages::p5_audit::P5AuditPage;
            use crate::ui::pages::p6_system::P6SystemPage;
            use crate::ui::pages::{p1_status::P1StatusPage, p2_config::P2ConfigPage};
            use mupc_display_proto::{
                AuditPage, AuditResult, ConfigField, ConfigGroup, ConfigKind, ConfigView,
                ConsoleAuditEntry, ConsoleOp, InterlockSection, InterlockSourceItem, WriteMode,
            };
            use serde_json::Value;

            let p1 = P1StatusPage::new(&host).expect("6 页共存：建 P1");
            p1.render(&PageInput::live(&frame_healthy()));
            assert!(
                p1.obj().is_alive() && p1.obj().child_count() > 0,
                "P1 须存活且非空壳"
            );

            let p6 = P6SystemPage::new(&host).expect("6 页共存：建 P6");
            p6.render(&PageInput::live(&frame_healthy()));
            assert!(
                p6.obj().is_alive() && p6.obj().child_count() > 0,
                "P6 须存活且非空壳"
            );

            let p2 = P2ConfigPage::new(&host).expect("R1: P2");
            p2.set_config(&ConfigView {
                groups: vec![
                    ConfigGroup {
                        id: "iec104".into(),
                        label: "IEC 104 连接参数".into(),
                        fields: vec![
                            ConfigField {
                                key: "gateway.listen_addr".into(),
                                label: "本机监听地址".into(),
                                kind: ConfigKind::Ipv4,
                                default: Value::from("127.0.0.1"),
                                value: Value::from("127.0.0.1"),
                                unit: None,
                                requires_reconnect: false,
                                editable: false,
                            },
                            ConfigField {
                                key: "gateway.port".into(),
                                label: "端口".into(),
                                kind: ConfigKind::U16 {
                                    min: 1,
                                    max: 65535,
                                    step: 1,
                                },
                                default: Value::from(2404),
                                value: Value::from(2404),
                                unit: None,
                                requires_reconnect: false,
                                editable: true,
                            },
                        ],
                    },
                    ConfigGroup {
                        id: "system".into(),
                        label: "遥测与日志".into(),
                        fields: vec![ConfigField {
                            key: "system.log_level".into(),
                            label: "日志级别".into(),
                            kind: ConfigKind::Enum {
                                options: vec![],
                            },
                            default: Value::from("info"),
                            value: Value::from("info"),
                            unit: None,
                            requires_reconnect: false,
                            editable: true,
                        }],
                    },
                ],
                revision: 7,
                write_mode: WriteMode::TextPreserve,
            })
            .expect("6 页共存：P2 set_config");
            assert!(
                p2.obj().is_alive() && p2.obj().child_count() > 0,
                "P2 须存活且非空壳"
            );

            let p4 = P4InterlockPage::new(&host).expect("6 页共存：建 P4");
            p4.set_section(&InterlockSection {
                ts_ms: 1_789_047_727_000,
                available: true,
                enabled: true,
                latched: true,
                stop_failed: false,
                sources: vec![
                    InterlockSourceItem {
                        name: "estop".into(),
                        tripped: true,
                    },
                    InterlockSourceItem {
                        name: "door".into(),
                        tripped: false,
                    },
                ],
                fault_lamp: Some(true),
                run_lamp: Some(false),
                release_hold_secs: 3,
            });
            assert!(
                p4.obj().is_alive() && p4.obj().child_count() > 0,
                "P4 须存活且非空壳"
            );

            let p5 = P5AuditPage::new(&host).expect("6 页共存：建 P5");
            let p5_full: Vec<ConsoleAuditEntry> = (0..20)
                .map(|i| ConsoleAuditEntry {
                    id: format!("ca-{i}"),
                    ts_ms: 1_700_000_000_000 + i as u64,
                    operator: mupc_display_proto::CONSOLE_OPERATOR.into(),
                    op: ConsoleOp::ConfigApply,
                    target: "gateway.port".into(),
                    before: None,
                    after: None,
                    result: AuditResult::Ok,
                    reason: None,
                    request_id: "rid-r1".into(),
                })
                .collect();
            p5.set_page(&AuditPage {
                entries: p5_full,
                page: 1,
                page_size: mupc_display_proto::AUDIT_PAGE_SIZE as u32,
                has_more: false,
                newest_ts_ms: Some(1),
                available: true,
            });
            assert_eq!(p5.visible_rows(), 20, "P5 须真的渲染出 20 行");
            assert_eq!(p5.rows_alive(), 20);

            let p3 = P3LogsPage::new(&host).expect("6 页共存：建 P3");
            let p3_full: Vec<LogEntry> = (0..20)
                .map(|i| log_entry(1_700_000_000_000 + i as u64, LogLevel::Info, "audit", "c"))
                .collect();
            p3.set_page(&log_page(p3_full, true, false));
            assert_eq!(p3.visible_rows(), 20, "P3 须真的渲染出 20 行");
            assert_eq!(p3.rows_alive(), 20);

            // 六页同屏渲染一次（外壳切页前的最坏情形）
            disp.refr_now_for_test();

            // 六页全部析构：确认互不持有、且宿主不被级联删除
            drop(p1);
            drop(p6);
            drop(p2);
            drop(p4);
            drop(p5);
            drop(p3);
            assert!(
                host.is_alive(),
                "6 页析构后 host 须仍存活（页不得持有宿主，否则级联删除）"
            );
        }

        // ── ⑦ **应用外壳**（B2c-3）────────────────────────────────────────────
        //
        // **挂钩方式（不改 `src/lvgl/**`）**：外壳链路 [`shell_chain`] 是 `pub(crate) fn`
        // 而非独立 `#[test]`（LVGL 非线程安全），由本函数在**同一 LVGL 会话内**顺序调起
        // —— 与 `ui_chain` / `pages_chain` 的既有做法同款。此处调用时上一批 6 页**已析构**，
        // 故外壳与它自己的 6 页是在"空堆"上装配的（这正是 R4 的严苛口径）。
        shell_chain(&mut disp, &screen);

        // ── ⑦′ **U-73 外设接线**（T21c-3）──────────────────────────────────
        //
        // 同款挂钩（`pub(crate) fn`，非独立 `#[test]`）：`shell_chain` 已把它自己的外壳
        // 析构掉 ⇒ 本链在自己的空堆上再建一个外壳，验"帧段 / catalog 回执**真的**到了
        // P4 / P6 的屏上"（T21c-1/2 的入口此前**零生产调用点**）。
        u73_wiring_chain(&mut disp, &screen);
    }

    drop(host);
    drop(screen);
    drop(disp);
    lvgl::deinit();
}

/// 外壳链路用的**最小 P2 配置视图**（只含一个可编辑字段 —— 足以驱动"脏 ⇒ 提示条"）。
///
/// **为什么另写一份而不复用 `pages_chain` 的内联件**：那份是**块内**定义的（作用域到不了本函数），
/// 且本链路只需要"一个能改脏的可编辑字段"，与页级用例的字段覆盖目标不同。
fn shell_p2_view() -> mupc_display_proto::ConfigView {
    use mupc_display_proto::{ConfigField, ConfigGroup, ConfigKind, ConfigView, WriteMode};
    use serde_json::Value;

    ConfigView {
        groups: vec![ConfigGroup {
            id: "iec104".into(),
            label: "IEC 104 连接参数".into(),
            fields: vec![ConfigField {
                key: "gateway.port".into(),
                label: "端口".into(),
                kind: ConfigKind::U16 {
                    min: 1,
                    max: 65535,
                    step: 1,
                },
                default: Value::from(2404),
                value: Value::from(2404),
                unit: None,
                requires_reconnect: false,
                editable: true,
            }],
        }],
        revision: 7,
        write_mode: WriteMode::TextPreserve,
    }
}

/// **应用外壳**（B2c-3）全部离屏用例。
///
/// **前提**：`lvgl::init()` 已由 [`pages_chain`] 调过（本函数**不**再 init/deinit），且
/// `disp` 是同一会话的内存 display（写 `sink` 的那一个）。
///
/// ## 覆盖（任务书 §六.2 逐条）
///
/// | # | 断言 | 段 |
/// |---|------|----|
/// | ① | 外壳 + 六页装配成功（**R4 第一道门**）；任一时刻只显一页 | ① |
/// | ② | 点第 3 个页签 ⇒ 显 P3；返回键 ⇒ 显 P1；6 个页签逐一点通 | ② |
/// | ③ | 返回按钮**仅 P2–P6 显示**；标题随页变化 | ② |
/// | ④ | 超时：50 s ⇒ 胶囊 + 文案；60 s ⇒ 切 P1；`PRESSED` ⇒ 计时重置 | ④ |
/// | ⑤ | P2 脏 ⇒ 无胶囊 + 提示条 + 「放弃修改」可点（点击清脏） | ⑤ |
/// | ⑥ | **建 / 拆外壳 ≥2 次**（R1：环 / 泄漏） | ⑥ |
pub(crate) fn shell_chain(disp: &mut Display, screen: &Obj) {
    use crate::state::ChannelStatus;
    use crate::ui::shell::{self, NavPage, Shell, TabState, COUNTDOWN_WINDOW_SECS};

    // 外壳的宿主（模拟 `main.rs` 的 `Obj::screen()` 直挂）。
    //
    // ⚠️ **必须显式清内边距**：裸 `Obj` 会吃到 LVGL 默认主题的 `card` 样式
    // （`pad_all` ≈ 20 px），那样宿主就不再是"屏"的忠实替身 —— 三区会被宿主的
    // 内边距顶偏，且**掩盖**外壳自己是否清干净了边距（生产路径的屏自带 `pad_all = 0`）。
    // 加上 `transparent()`（`pad_all(0)` + `radius NONE`）后，本宿主与 `Obj::screen()`
    // 在几何上等价 ⇒ 下面⑦的**绝对坐标**断言才对生产路径有约束力。
    let home = Obj::create(screen).expect("shell host");
    home.set_size(Dimens::SCREEN_W, Dimens::SCREEN_H);
    home.set_pos(0, 0);
    home.add_style(&theme::transparent(), StyleSelector::main());

    let base = Instant::now();
    let at = |secs: u64| base + Duration::from_secs(secs);
    const CLOCK: &str = "13:42:07";

    // ═══ ① 装配 + 「任一时刻只显一页」+ 页眉常态 ═══════════════════════════════
    {
        // **R4 第一道门**：外壳（页眉 + 6 页签 + 提示条 + 胶囊 + 3 个通道胶囊）**与**
        // 它自己的 6 页同时驻留 1 MB 的 LVGL 定容池。OOM ⇒ 本行 `expect` 即失败。
        // **R4 对象预算护栏**（实测 2026-09-15：外壳 + 6 页 + **整屏降级层** = **730** 个
        // LVGL 对象，成功；B2c-3 时为 727，B3-2b-1 的 EDGE-03 整屏层 +3 件 —— 1 个遮罩容器
        // + 中央大字 + 时长行，**常驻**（层建一次、只切可见性，见 `shell.rs` 模块头裁定 ①））。
        // LVGL 的堆是**定容池**（`lvgl-sys/lv_conf.h` 的 `LV_MEM_SIZE = 1 MB`）⇒ "能建出来"
        // 不是可推定的性质（早期 256 KB 时建到第 4 页即 `lv_realloc` 失败）。本护栏把
        // **对象数量**钉住：往任一处加构件把总量推过 [`SHELL_OBJECT_BUDGET`]，这里会先变红
        // 并给出准确数字，而不是等到某台机器上 OOM 挂死。
        //
        // **B3-2c 重新实测（2026-09-16）：742** —— 730 → 742 的来源 = **4 个区块级
        // 「冻结 / 数据过期」角标**（P4 总态卡 / P4 触发源卡 / P6 装置信息卡 / P6 运行信息卡），
        // 每个 `StatusChip` = 3 个对象（容器 + 图标 + 文字）⇒ +12。**零余量**地重钉在这里
        // （角标是 EDGE-03 的明文要求，属"确需新增"；不再顺手调其它构件）。
        //
        // **预算 = 实测值（零余量）**：`mounted ≤ 预算`，故 `mounted == 742` 绿、
        // **把预算改成 741（实测 −1）必红** —— 这是"预算不是摆设"的判据（探针实测见交付报告）。
        // 下一批若**确需**新增常驻构件：先在此**重新实测**并同步本行数字与注释，不得只抬预算。
        //
        // **⚠️ 口径（B3-2b-1 规格符合性评审 建议 5；如实登记）**：`PROBE_MOUNTS` 计的是
        // **句柄数**，不是"底层 LVGL 对象数" —— 它的自增点在 `lvgl::obj::Obj::from_raw`
        // （`obj.rs:276`），而 **`Obj::create`（owns=true）与 `Obj::adopt_borrowed`（owns=false）
        // 都经 `from_raw`** ⇒ 两者**都**自增（`Obj::share_borrowed` 复用探针、**不**自增）。
        // 本测量区间（`Shell::new`，见下 `before` / `after`）内**没有**任何 `adopt_borrowed`
        // 调用（外壳只 `Obj::create`；`layer_top()` / `screen()` 这类"借用既有单例"的路径
        // **不在此区间**）⇒ 本区间的 `mounted` **数值上等价于"新建对象数"**。
        // 但若日后把 `layer_top()` 一类**借用**调用搬进 `Shell::new`，本计数会**多算**
        // （借用也 +1）—— 届时要么把口径改成"新建数"（另设计数器），要么把借用调用挪出区间。
        //
        // **改什么会让本条变红**：给 6 页 / 外壳 / 整屏层任一处加出 ≥1 个常驻对象。
        //
        // ⚠️ **口径缺口（B3-2c 整改 建议 2；如实登记）**：本预算**只量 `Shell::new`** 这一个
        // 区间（外壳 + 6 页 + 整屏层）。**另有 app 层 4 个常驻对象不在本预算内**：
        // `App::new` 建的 app 层 Toast（`App::toast`）**常驻整场**，实测 = **4 个** LVGL 对象
        // （容器 `obj` + 左缘色条 `_accent` + 图标 `Rc<Label>` + 文本 `Rc<Label>`）。
        // **为什么不把它并进来**：`App` 的构造需要 LVGL 会话 **+ 控制通道客户端**
        // （`ConsoleClient::new`）⇒ 本链路（只有 `Display` + `Shell`）**建不出 `App`**，
        // 那个区间在这里**测不到**（进程内也起不了第二条 LVGL 线程）。故如实注明而不并账 ——
        // 真实的常驻对象上限 = `SHELL_OBJECT_BUDGET` + **4**（若日后新增"能在离屏链路里建出来的"
        // 常驻件，仍按本预算判）。
        //
        // **U-73 / T21c-1-r1 重新实测（2026-09-25）：981** —— 742 → 981 的来源 = **P4 消防区**
        // （逐条可核，`p4_interlock.rs`）。**账** = 978（T21c-1 首轮实测，**但那一轮把总态卡的
        // 冻结角标误移除了** —— 评审 F2/F3 的返工项）+ **2**（IL29② 恢复该角标的 **图标-only**
        // 形态 = 容器 + 图标两件）+ **1**（A1 段顶通告行 = 评审 W-3② 订正：通告独立成行）
        // = **981**（本用例**实测**兜底）：
        // | 构件 | 对象数 |
        // |------|-------|
        // | 【安全总览带】容器 + 火警等级卡（卡 1 + 色条 1 + 标题 1 + 图标 1 + 文案 1）
        //   + 总态卡**冻结徽标**（容器 1 + 图标 1；IL29② 的图标-only 形态） | 8 |
        // | §A1 消防系统状态（卡 1 + 标题 1 + **段顶通告行 1** + **逐位 16 行**） | 19 |
        // | §A2 灭火瓶压力（卡 + 标题 + 主值） | 3 |
        // | §A3 探测器触发（卡 + 标题 + 3 行） | 5 |
        // | §A4 探测器汇总（卡 + 标题 + 汇总行 + 不一致提示 + 「查看明细」按钮 2） | 7 |
        // | 下钻视图（容器 1 + 标题 1 + 表头 8 + 行区容器 1 + **20 行 × 9 格** 180
        //   + 上一页/下一页/收起 6 + 页码 1 + 失败文案 1 + 重试 2） | 201 |
        // | 合计 | **≈243**（其中 §B 联锁区变动件 = 0；其余为既有外壳 + 6 页基线） |
        //
        // **为什么是"确需新增"**：§A 四组（F21）与下钻视图（F21.4 的分页明细）都是
        // **本增量的交付物**，对象数与"逐位 16 行 / 逐只明细"的呈现规格**同构**（设计 §15.4）。
        // **未做窗口化**（设计 §15.5.3 的"可视行 ×1.5"）⇒ 下钻的 180 个格对象**一次性建齐**；
        // 这是**已登记的偏差**（`p4_interlock.rs` **IL29⑥**）。⚠️ **订正（T21c-2-r1）**：本段
        // 原写"根因 = 薄层无滚动事件（`EventCode` 未镜像 `LV_SCROLL`）⇒ 结构性不可实现"——
        // **不实**：该事件码自 **B4b（`297b51b`，2026-09-16）** 起已镜像，P6 的
        // `SegmentList` / `BmsDrill` 与 P5 均已按它消费并各有探针实测 ⇒ 这是**一处无依据的
        // 降级**（已登记为**待单独立项**；P6 已给出可复用的窗口化范式）。**真机余量须复核**
        // （设计 §15.9 **R-34** / 真机档 D-5：`lv_mem_monitor`）。
        // **代价已量化**：981 个对象按 LVGL `lv_obj` 量级（≈150–200 B）≈ **150–200 KB**，
        // 在 `LV_MEM_SIZE = 1 MB` 池内（此前 256 KB 池建到第 4 页即失败的记录见上）。
        // ⚠️ **余量（自洽口径；2026-09-25 / W-g 订正）**：按 LVGL `lv_obj` 量级（≈150–200 B）
        // 与**当前**常驻预算 1031 件 ⇒ 常驻 ≈ **150–200 KB** / 1 MB 池 ⇒ **占用 ≈15–20 %、
        // 余量 ≈80–85 %**（**不是 0.8 %** —— 此前同句的"余量 ~0.8 %"与它自己的算式
        // 150–200 KB / 1 MB **差约两个数量级**，属单位 / 数量级笔误，已随本条与设计 §15.9
        // **R-34** 同批订正）。再叠加"5 段都点过 + 开过一次下钻"的**惰性半区**
        // （[`P6_ALL_SEGMENTS_BUDGET`] = 694，与本文件 1031 相加 = **稳态上界 1725 件**）
        // ⇒ ≈ **257–343 KB** ⇒ 占用 ≈25–33 %、**余量 ≈67–75 %**。
        // 上述均为**账上推算、非实测**（真机 `lv_mem_monitor` 复核为验收项：设计 §15.9
        // **R-34** / 真机档 D-5；见 **IL29⑥**）。
        // **T21c-2（2026-09-25）重新实测：1016** —— 981 → **1016** 的来源 = **P6 段「装置」的
        // 新增件**（逐条可核，`p6_system.rs`），**零余量**地重钉在这里：
        //
        // | 构件 | 对象数 |
        // |------|-------|
        // | `SegmentedTabs`：行容器 1 + 5 段 ×（`lv_button` 1 + `lv_label` 1） | 11 |
        // | 段「装置」的段内容容器 + 其滚动宿主（`ScrollContainer`） | 2 |
        // | 站状态表：卡 1 + 卡头标题 1 + **5 行 × 5**（行容器 + **4 个文字槽** —— T21c-2-r1 / F2 的四列返工在每行加了第 4 槽） | 27 |
        // | 合计 | **40** |
        //
        // **T21c-2-r1（2026-09-25）重新实测：1021** —— 1016 → **1021** 的来源 = 站状态表
        // **每行 +1 个文字槽**（四列返工：站名 / 状态 / 成功 / 更新）⇒ 5 行 +5 件。
        //
        // **T21c-3-r1（2026-09-25）重新实测：1031** —— 1021 → **1031** 的来源 = **两页各一枚**
        // 「名称表可能过期」提示条 + 「重试」（设计 §15.3.1 第 2 / 3 句；本批交付）：
        //
        // | 构件 | 对象数 |
        // |------|-------|
        // | P4：`WarnBanner`（容器 1 + ⚠ 图标标签 1 + 文案标签 1）+ `TextButton`（按钮 1 + 标签 1） | 5 |
        // | P6：同上（页级单件，落在段内容区顶部） | 5 |
        // | 合计 | **+10** |
        //
        // ⚠️ **口径（必须与 P6 的另一半合读）**：本区间（`Shell::new`）**只含段「装置」**
        // —— 4 个外设段与下钻视图都是**惰性创建**的（设计 §15.5.3 的容器策略）⇒ 它们的对象
        // 不在本区间内，另由 `P6_ALL_SEGMENTS_BUDGET`（**实测 694**，见
        // `ui/tests.rs` 的 T21c-2 段与 `pages_chain` 的 ⑰ 块）兜住。**稳态上界 ≈ 1725**。
        // （P6 的提示条落在 `P6SystemPage::new` 里 ⇒ 计入本区间、**不**计入那一半。）
        const SHELL_OBJECT_BUDGET: usize = 1031;
        let before = crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst);
        let sh = Shell::new(&home).expect("外壳 + 6 页装配（LVGL_MEM 1 MB）");
        let mounted =
            crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst) - before;
        assert!(
            mounted <= SHELL_OBJECT_BUDGET,
            "外壳 + 6 页创建了 {mounted} 个 LVGL 对象（预算 {SHELL_OBJECT_BUDGET}）—— \
             超过预算即逼近 1 MB 定容池的上限（先减构件，再谈抬预算）"
        );
        // 尺寸读回须等一次布局趟（`Obj::size` 是"布局趟落定后的实际值"）。
        disp.refr_now_for_test();

        assert_eq!(
            sh.obj().size(),
            (Dimens::SCREEN_W, Dimens::SCREEN_H),
            "外壳根 = 全屏 1024×768"
        );
        assert_eq!(
            sh.header_obj().size(),
            (Dimens::SCREEN_W, Dimens::HEADER_H),
            "页眉 = 1024×72（UI §4.1）"
        );
        assert_eq!(
            sh.nav_obj().size(),
            (Dimens::SCREEN_W, Dimens::NAV_H),
            "底部导航 = 1024×72（UI §3.5）"
        );
        assert_eq!(
            sh.banner_obj().size(),
            (Dimens::CONTENT_W, Dimens::TOUCH_MIN),
            "未保存提示条 = 992×48（UI §4.3）"
        );
        // **⑦ 三区绝对坐标**（SH14 收口 —— 此前只断尺寸，`root` 的内边距缺陷**无网**）。
        //
        // 判据取**屏内绝对坐标**：宿主已与 `Obj::screen()` 几何等价（见上方 `home` 注释），
        // 故这里的期望值就是生产路径上的实际落点（LVGL 闭区间，`x2 = x1 + w - 1`）。
        // **改什么会让本条变红**：① 去掉 `theme::screen_bg()` 的 `set_pad_all(0)`
        // ⇒ 三区整体 +(20,20)（2026-09-15 实测：header `(20,20)-(1043,91)`，右缘出屏 20 px、
        // nav 底 `787` 出屏 20 px）；② 摘掉宿主的 `transparent()` ⇒ 再 +(22,22)。
        assert_eq!(
            home.coords(),
            Area {
                x1: 0,
                y1: 0,
                x2: Dimens::SCREEN_W - 1,
                y2: Dimens::SCREEN_H - 1
            },
            "宿主须与屏几何等价 —— 这是本组绝对坐标断言的前提"
        );
        let (hx1, hy1, hx2, hy2) = {
            let c = sh.header_obj().coords();
            (c.x1, c.y1, c.x2, c.y2)
        };
        assert_eq!(
            (hx1, hy1, hx2, hy2),
            (0, 0, Dimens::SCREEN_W - 1, Dimens::HEADER_H - 1),
            "页眉须自屏原点起算且右侧不出屏（UI §4.1；见 SH14）"
        );
        let (nx1, ny1, nx2, ny2) = {
            let c = sh.nav_obj().coords();
            (c.x1, c.y1, c.x2, c.y2)
        };
        assert_eq!(
            (nx1, ny1, nx2, ny2),
            (
                0,
                Dimens::SCREEN_H - Dimens::NAV_H,
                Dimens::SCREEN_W - 1,
                Dimens::SCREEN_H - 1
            ),
            "底部导航须贴屏底且不越界（UI §3.5；见 SH14）"
        );
        let (px1, py1) = {
            let c = sh.page_host_obj().coords();
            (c.x1, c.y1)
        };
        assert_eq!(
            (px1, py1),
            (Dimens::SIDE_PAD, Dimens::HEADER_H),
            "页根宿主落点 = (SIDE_PAD, HEADER_H)（pages 契约 1 的调用方一侧）"
        );
        // ── ⑦′ 其余两区 + 页根宿主的**四边**（2026-09-15 代码质量评审补网：此前
        // `content` 与 `banner` **零**绝对坐标断言、`page_host` 只断了左上两点 ——
        // 于是把 `banner.set_pos(SIDE_PAD, CONTENT_Y)` 改成 `(0, CONTENT_Y)`、或把
        // `content` 的高度写错，全套用例都不会响）──
        let ct = sh.content_obj().coords();
        assert_eq!(
            (ct.x1, ct.y1, ct.x2, ct.y2),
            (
                0,
                Dimens::HEADER_H,
                Dimens::SCREEN_W - 1,
                Dimens::HEADER_H + Dimens::CONTENT_H - 1
            ),
            "内容区必须从页眉下缘铺到导航上缘、左右贴屏（UI §4.1）"
        );
        let ph = sh.page_host_obj().coords();
        assert_eq!(
            (ph.x1, ph.y1, ph.x2, ph.y2),
            (
                Dimens::SIDE_PAD,
                Dimens::HEADER_H,
                Dimens::SIDE_PAD + Dimens::CONTENT_W - 1,
                Dimens::HEADER_H + Dimens::CONTENT_H - 1
            ),
            "页根宿主 = 内容区有效框（SIDE_PAD 起、CONTENT_W 宽，四边都要对）"
        );
        let bn = sh.banner_obj().coords();
        assert_eq!(
            (bn.x1, bn.y1, bn.x2, bn.y2),
            (
                Dimens::SIDE_PAD,
                Dimens::HEADER_H,
                Dimens::SIDE_PAD + Dimens::CONTENT_W - 1,
                Dimens::HEADER_H + Dimens::TOUCH_MIN - 1
            ),
            "未保存提示条须与内容区同宽、且自内容区上缘起（UI §4.3；h48 = TOUCH_MIN）"
        );
        // **任一时刻只显一页**：六页全建（存活、非空壳），但可见的**恰一个**。
        for p in NavPage::ALL {
            assert!(sh.page_obj(p).is_alive(), "{p:?} 页根须存活");
            assert!(sh.page_obj(p).child_count() > 0, "{p:?} 页须非空壳");
        }
        assert_eq!(
            sh.visible_pages(),
            vec![NavPage::Main],
            "初始只显 P1（默认页 / 超时回归目标页）"
        );
        assert_eq!(sh.current(), NavPage::Main);
        // 页眉常态：P1 无返回键、标题为契约原文；时钟已由 tick 注入；通道胶囊 = 已连接。
        assert!(!sh.back_visible(), "P1 不显示返回键（UI §4.1）");
        assert_eq!(sh.title_text().as_deref(), Some("台区储能装置运行状态"));
        sh.tick(at(0), CLOCK);
        assert_eq!(sh.clock_text().as_deref(), Some(CLOCK));
        assert_eq!(sh.channel_text().as_deref(), Some(shell::TEXT_CHANNEL_OK));
        // **② 通道胶囊在页眉右端**（落到**对象**上的那一半断言；常量层那一半在
        // `shell.rs::tests::header_slots_are_disjoint_and_inside_canvas`）。
        //
        // **判据是"与常量精确相等"，不是"落在右半区"**：早先写成 `>= SCREEN_W/2`，
        // 有 **80 px 盲窗** —— 把调用点写成 512（与触摸角标重叠 64 px、离契约位 80 px）
        // 时**纯逻辑网与本条同时保持绿**（2026-09-15 代码质量评审探针 P5 实测 290 全绿）。
        // 精确相等之所以稳定：宿主已与屏几何等价、`root` 已清内边距 ⇒ 对象的屏内左缘
        // 恰等于 `HEADER_CHIP_X`。
        // **改什么会让本条变红**：把 `Shell::new` 里 `chip.set_pos(HEADER_CHIP_X, ..)` 的
        // 实参换成 412（中段）**或 512**（半区边界内）⇒ 两条都红。
        assert_eq!(
            sh.channel_chip_x(),
            Some(shell::HEADER_CHIP_X),
            "通道胶囊（§4.1「右端」）的屏内左缘必须**恰等于** HEADER_CHIP_X"
        );
        assert!(!sh.touch_badge_visible(), "触摸可用 ⇒ 无角标");
        assert!(!sh.countdown_visible(), "t=0 ⇒ 无倒计时胶囊");
        assert!(!sh.banner_visible(), "P2 不脏 ⇒ 无提示条");
        assert!(
            sh.tab_bar_visible(NavPage::Main).unwrap(),
            "P1 页签选中条必须亮（装配即摆好初始页）"
        );
        assert!(
            !sh.tab_bar_visible(NavPage::Config).unwrap(),
            "未选中页签不得亮选中条"
        );
        for p in NavPage::ALL {
            assert_eq!(sh.tab_text(p).as_deref(), Some(p.nav_label()));
            assert_eq!(sh.tab_icon_text(p).as_deref(), Some(p.nav_icon()));
        }
        assert!(
            sh.tab_divider(NavPage::Main).is_none(),
            "第 1 项无左缘竖分隔线（UI §4.2）"
        );
        assert!(
            sh.tab_divider(NavPage::Config).is_some(),
            "第 2–6 项各有左缘竖分隔线"
        );

        // ═══ ② 路由：1 次触摸到任意页 + 返回键 ═════════════════════════════════
        // **改什么会让本条变红**：把 `Core::select` 里的 `i == idx` 写成恒真（六页同显 ⇒
        // `visible_pages()` 长度变 6）；把页签回调里的 `NavPage::from_index(i)` 删掉（点页签
        // 不再切页 ⇒ `cur` 不变）。把返回键的回调改成 `select(page, ..)`（返回不到 P1）。
        for p in NavPage::ALL {
            sh.tab(p).expect("页签").send_event(EventCode::CLICKED);
            assert_eq!(sh.current(), p, "点 {p:?} 页签 ⇒ 1 次触摸直达");
            assert_eq!(sh.visible_pages(), vec![p], "{p:?} 显示时其余五页必须隐藏");
            assert!(sh.tab_bar_visible(p).unwrap(), "{p:?} 选中条亮");
            assert_eq!(sh.title_text().as_deref(), Some(p.title()), "标题随页变化");
            assert_eq!(
                sh.back_visible(),
                p.shows_back(),
                "{p:?} 的返回键可见性（仅 P2–P6）"
            );
        }
        // 点第 3 个页签 ⇒ 显 P3（任务书逐条点名的那一条）。
        sh.tab(NavPage::Logs).expect("页签").send_event(EventCode::CLICKED);
        assert_eq!(sh.current(), NavPage::Logs);
        assert_eq!(sh.visible_pages(), vec![NavPage::Logs]);
        // 返回 ⇒ P1。
        sh.back_button().send_event(EventCode::CLICKED);
        assert_eq!(sh.current(), NavPage::Main, "返回键 ⇒ 切 P1（UI §4.3）");
        assert_eq!(sh.visible_pages(), vec![NavPage::Main]);
        // 页签选中态**双通道**：底色（`set_checked` ⇒ `LV_STATE_CHECKED`）+ 顶部 4 px 条。
        assert!(
            sh.tab(NavPage::Main).expect("页签").is_checked(),
            "选中页签的 `LV_STATE_CHECKED` 必须置位（底色通道）"
        );
        assert!(
            !sh.tab(NavPage::Config).expect("页签").is_checked(),
            "未选中页签不得置 CHECKED"
        );
        // **双通道**的另一半：文字 / 图标**色**（应用标记读回）。
        // **改什么会让本条变红**：把 `build_tab` 里 `text_colors` / `icon_colors` 的两档对调。
        assert_eq!(
            sh.tab_text_color(NavPage::Main),
            Some(Palette::TEXT_PRIMARY),
            "选中页签文字 = `text_primary`（UI §4.2）"
        );
        assert_eq!(
            sh.tab_text_color(NavPage::Config),
            Some(Palette::TEXT_SECOND),
            "未选中页签文字 = `text_second`（UI §4.2）"
        );
        assert_eq!(
            sh.tab_icon_color(NavPage::Main),
            Some(Palette::INFO),
            "选中页签图标 = `#4EA6FF`（UI §4.2）"
        );
        assert_eq!(
            sh.tab_icon_color(NavPage::Config),
            Some(Palette::TEXT_WEAK),
            "未选中页签图标 = `#6E7FA0`（UI §4.2）"
        );

        // **① 选中 / 按下态的「底色」通道**（应用标记读回；**B2c-3 规格评审 ①** 补的回归锁）。
        //
        // 背景：原文只有**文字 / 图标色**有回归锁，**底色**（§4.2「选中 底 `surface_alt`」/
        // 「按下 底 `#2E4066`」）被评审探针 ⑦⑧ 改掉后**全绿** ⇒ 该回归锁是补上的。
        //
        // 口径：`tab_bg(.., TabState)` 读的是**应用标记**（建样式时记下的、送进
        // `Style::set_bg_color(..)` 的那个值；唯一真源 = `shell::NAV_BG_*`），不是从 LVGL
        // 读回的实际底色（薄层无该通道）—— 如实标注见 `shell.rs::Shell::tab_bg`。
        //
        // **改什么会让本条变红**：① 把 `shell::NAV_BG_SELECTED` 的 `Palette::SURFACE_ALT`
        // 换成**另一个** `Palette` 常量（如 `SURFACE_HIGH`）⇒ 第 1 条红；② 把
        // `shell::NAV_BG_PRESSED` 换掉 ⇒ 第 2 条红；③ 把 `NAV_BG_DEFAULT` 从 `Transparent`
        // 改成任何实心底 ⇒ 第 3 条红；④ 三档写成同一个色（"全都一样"的蒙混形态）⇒ 第 4 条红。
        assert_eq!(
            sh.tab_bg(NavPage::Main, TabState::Selected),
            Some(shell::TabBg::Solid(Palette::SURFACE_ALT)),
            "选中页签底色 = `surface_alt`（UI §4.2「选中：底 `#1B2942`」）"
        );
        assert_eq!(
            sh.tab_bg(NavPage::Main, TabState::Pressed),
            Some(shell::TabBg::Solid(Palette::SURFACE_PRESS)),
            "按下页签底色 = `surface_press`（UI §4.2「按下 底 `#2E4066`」/ §5.2 `NavTab` 行）"
        );
        assert_eq!(
            sh.tab_bg(NavPage::Config, TabState::Default),
            Some(shell::TabBg::Transparent),
            "未选中页签底色 = **透明**（UI §4.2「未选中：底色透明」）"
        );
        // **互斥**（防"三档一个色"从"选中态有底色"的断言下蒙混过关）：未选中档必须**不等于**
        // 选中档；且六项逐项一致（不是只有第 1 项被特殊对待）。
        assert_ne!(
            sh.tab_bg(NavPage::Config, TabState::Default),
            sh.tab_bg(NavPage::Main, TabState::Selected),
            "未选中档底色不得等于选中档（否则'选中态底色'这一通道等于不存在）"
        );
        for p in NavPage::ALL {
            assert_eq!(
                sh.tab_bg(p, TabState::Selected),
                Some(shell::TabBg::Solid(Palette::SURFACE_ALT)),
                "{p:?} 的选中档底色必须同为 `surface_alt`（六项同一套皮肤）"
            );
            assert_eq!(
                sh.tab_bg(p, TabState::Pressed),
                Some(shell::TabBg::Solid(Palette::SURFACE_PRESS)),
                "{p:?} 的按下档底色必须同为 `surface_press`"
            );
            assert_eq!(
                sh.tab_bg(p, TabState::Default),
                Some(shell::TabBg::Transparent),
                "{p:?} 的未选中档底色必须为透明"
            );
        }

        // **切页通知**（B3 用它感知"当前页变了"）：仅在页**真正变化**时触发。
        // **改什么会让本条变红**：把 `Core::select` 的 `changed && notify` 改成 `notify`
        // （重复切同一页也通知）⇒ 第 2 条断言拿到 3 个元素。
        let seen: Rc<RefCell<Vec<NavPage>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let seen = Rc::clone(&seen);
            sh.set_on_page_change(move |p| seen.borrow_mut().push(p));
        }
        sh.show(NavPage::Audit);
        sh.show(NavPage::Audit); // 同页重复切 ⇒ **不**再通知
        sh.show(NavPage::System);
        assert_eq!(
            *seen.borrow(),
            vec![NavPage::Audit, NavPage::System],
            "切页通知只在页真正变化时触发一次"
        );
        sh.show(NavPage::Main);

        // 六页同屏渲染一次（真实渲染链路：切页 → 失效 → flush）。
        disp.refr_now_for_test();

        // ═══ ④ 超时回归（**注入时钟** ⇒ 逐拍可断言）═══════════════════════════
        // 先切到 P4（非 P1，才有"回归"可言），并重置活动时刻 = base。
        sh.tab(NavPage::Interlock).expect("页签").send_event(EventCode::CLICKED);
        sh.tick(at(0), CLOCK); // 消费上述按压 ⇒ last_activity = base
        assert_eq!(sh.current(), NavPage::Interlock);
        // t=49：还剩 11 s ⇒ 无胶囊。
        sh.tick(at(49), CLOCK);
        assert!(
            !sh.countdown_visible(),
            "超时前 11 s 不得出现胶囊（窗口 = {COUNTDOWN_WINDOW_SECS} s）"
        );
        // t=50：还剩 10 s ⇒ 胶囊出现且文案逐字正确。
        // **改什么会让本条变红**：把 `shows_countdown` 的窗口从 10 改小（如 5）⇒ 这里不显；
        // 把 `countdown_text` 的措辞改掉 ⇒ 文案断言红。
        sh.tick(at(50), CLOCK);
        assert!(sh.countdown_visible(), "超时前 10 s 出现倒计时胶囊（UI §4.3）");
        assert_eq!(
            sh.countdown_text_value().as_deref(),
            Some("10 秒后返回主状态页"),
            "胶囊文案 = `N 秒后返回主状态页`（UI §3.6 超时行）"
        );
        // **SH3**：胶囊占位时通道胶囊让位（页眉放不下五者，见 shell.rs 偏差表）。
        assert_eq!(
            sh.channel_text(),
            None,
            "倒计时胶囊出现 ⇒ 通道胶囊让位（SH3）"
        );
        // t=55.5：向上取整 ⇒ 仍显示 5 秒（若改向下取整会显示 4 ⇒ 红）。
        sh.tick(at(55) + Duration::from_millis(500), CLOCK);
        assert_eq!(sh.countdown_text_value().as_deref(), Some("5 秒后返回主状态页"));
        // 中途派发 `PRESSED`（**全屏输入对象**）⇒ 计时重置 ⇒ 胶囊消失。
        // **改什么会让本条变红**：把 `Core::tick` 里的 `pending_activity.replace(false)` 删掉、
        // 或把 `Shell::wire` 的根 `PRESSED` 挂钩删掉 ⇒ 这里胶囊仍在、且 t=60 会切页。
        sh.obj().send_event(EventCode::PRESSED);
        sh.tick(at(55) + Duration::from_millis(500), CLOCK);
        assert!(
            !sh.countdown_visible(),
            "任意触摸 ⇒ 计时重置 ⇒ 胶囊即刻消失（UI §4.3）"
        );
        assert_eq!(sh.current(), NavPage::Interlock, "重置后仍停在原页");
        // t=60（自原点起算）：**若上一步的重置没生效，这里就已经切回 P1 了** —— 先钉住"没切"。
        sh.tick(at(60), CLOCK);
        assert_eq!(
            sh.current(),
            NavPage::Interlock,
            "重置后时钟须从**重置那一刻**重新起算（60 s 未到）"
        );
        // 从重置时刻（t≈55.5）再推进 60 s ⇒ 切 P1 + 胶囊消失 + 计时重新起算。
        // **改什么会让本条变红**：把 `should_return_home` 的返回值反过来、或把 `select(Main)`
        // 那一句删掉 ⇒ 这里仍停在 P4。
        sh.tick(at(115) + Duration::from_millis(500), CLOCK);
        assert_eq!(sh.current(), NavPage::Main, "到 0 s 自动切 P1（UI §4.3）");
        assert!(!sh.countdown_visible(), "切页后胶囊消失");
        assert_eq!(
            sh.visible_pages(),
            vec![NavPage::Main],
            "超时回归落在 P1 且只显 P1"
        );
        // 回归后计时从**满时长**重算（不是"又立刻超时"）。
        sh.tick(at(116) + Duration::from_millis(500), CLOCK);
        assert_eq!(sh.current(), NavPage::Main, "回归后计时已重置，不得连续切页");

        // ═══ ④″ **计时重置的完整语义**（原"覆盖边界"节；B4b 整改改写，偏差 **SH5**）════
        //
        // **改写说明（原注释明令"改写、不要删掉"）**：本节最早锁的是"页内按压**不**重置"
        // —— 当时薄层没有输入设备级钩子：LVGL 把 `PRESSED` 投给**命中的最深可点对象**
        // （`vendor/lvgl/src/indev/lv_indev.c:618::lv_indev_search_obj`），`lv_obj` 构造时又
        // **默认 `CLICKABLE`**（`lv_obj.c:584`）、三区铺满画布 ⇒ 外壳根收不到；子对象的事件
        // 也**默认不上冒**。B4b 首版改成"外壳挂**设备级** `Indev::on(PRESSED)` 钩子"，
        // **B4b 整改**判定该机制**在 `LV_STATE_DISABLED` 的命中对象上不成立**
        // （`lv_indev.c:1339` 的 `is_enabled` 把 `send_event(PRESSED)`（`:1342`）整个包住，而
        // `lv_obj_hit_test` **不排除** `DISABLED` ⇒ `lv_indev_search_obj` 照样返回那个禁用
        // 控件 ⇒ 回调**根本不触发**；实测：120×120 且置 `DISABLED` 的子对象按其中点，
        // 回调计数 `2 → 2`）。生产命中面真实存在（P2 无改动时置灰的「保存」/ 步进器越界禁用 /
        // P4 联锁）⇒ 换机制：**evdev 快照驱动**（[`crate::app::apply_touch_snapshot`] —— 喂
        // 快照时就地调 [`shell::Shell::note_activity`]），与 LVGL 的**命中结果无关**。
        {
            // **生产同款路径**：`App` 也是"建真实 indev → 每拍 `apply_touch_snapshot`"。
            let indev = Indev::create_pointer(disp).expect("④″：建真实 indev");
            // 外壳根的 `PRESSED` 挂钩**仍在**（"非控件 / 合成投递"那一面，见 `Shell::wire` ①）。
            // 本节给它加一个**计数探针**：证明下面那次按压**没有**落到外壳根上（即真的是
            // "页内控件"，而**不是**屏对象 / 外壳根 —— LVGL 给每个 `lv_obj` 默认置
            // `CLICKABLE`，无更深命中时返回的是**屏对象本身**）。
            let root_hits = Rc::new(Cell::new(0u32));
            {
                let c = Rc::clone(&root_hits);
                sh.obj().on(EventCode::PRESSED, move |_| c.set(c.get() + 1));
            }

            // 落点 = **P1 页内控件**（契约 1 的 SOC 卡）的矩形中心 —— 由对象自己的 `coords()`
            // 算出，而不是拍一个"大概在页面里"的坐标。
            let card = sh.p1().soc_card().coords();
            let (px, py) = ((card.x1 + card.x2) / 2, (card.y1 + card.y2) / 2);
            assert_eq!(sh.current(), NavPage::Main, "④″ 前置：P1 可见（卡片坐标才有意义）");

            // 接近超时（剩 10 s ⇒ 胶囊在）—— 与既有 ④ 段同款时间口径。
            sh.note_activity();
            sh.tick(at(5000), CLOCK); // 消费 ⇒ last_activity = 5000
            sh.tick(at(5050), CLOCK);
            assert!(
                sh.countdown_visible(),
                "④″ 前置：剩 10 s ⇒ 倒计时胶囊应在（否则本条测不到「重置」）"
            );

            let hits_before = root_hits.get();
            crate::app::apply_touch_snapshot(
                &sh,
                &indev,
                TouchSnapshot {
                    pressed: true,
                    x: px,
                    y: py,
                },
            );
            indev.read(); // `lv_indev_read()` → 命中判定 → `LV_EVENT_*` 派发（产品同款）
            assert_eq!(
                root_hits.get(),
                hits_before,
                "④″ 前置：本次按压**不得**落到外壳根（`({px},{py})` 是 P1 SOC 卡的矩形中心；\
                 若落到根上，下面那条断言就证明不了「页内控件也重置」）"
            );
            crate::app::apply_touch_snapshot(
                &sh,
                &indev,
                TouchSnapshot {
                    pressed: false,
                    x: px,
                    y: py,
                },
            );
            indev.read(); // 抬手（回弹 / 滚动判定在这一步收敛）

            sh.tick(at(5050) + Duration::from_millis(500), CLOCK);
            assert!(
                !sh.countdown_visible(),
                "**页内控件上的按压也重置计时**（§4.3「任何触摸事件」；SH5）—— 改什么会让本条\
                 变红：删掉 `app::apply_touch_snapshot` 里的 `shell.note_activity()`，或把 \
                 `App::pump` 的 `apply_touch_snapshot(..)` 接线删掉"
            );
            assert_eq!(sh.current(), NavPage::Main, "重置不切页");

            // ── **禁用态控件上的按压也重置计时**（B4b 整改「重要 1」的回归锁）──────────────
            //
            // 造一个 `set_disabled(true)` 的可点控件 —— 这正是旧机制（设备级 `PRESSED` 钩子）
            // 漏掉的那一类。两段合起来才完整：① **启用态**下同一坐标能收到**对象级** `PRESSED`
            // ⇒ 证明这个坐标确实命中该控件；② **置禁用**后对象级回调消失（LVGL 的 `is_enabled`
            // 门），而**计时照旧重置** ⇒ 证明机制确实与命中无关。
            let hits = Rc::new(Cell::new(0u32));
            let probe_btn = crate::lvgl::widgets::TextButton::create(sh.page_obj(NavPage::Main), "禁用")
                .expect("④″：造一个可禁用的可点控件");
            probe_btn.set_pos(8, 8);
            probe_btn.set_size(160, Dimens::TOUCH_MIN);
            {
                let h = Rc::clone(&hits);
                probe_btn.on(EventCode::PRESSED, move |_e| h.set(h.get() + 1));
            }
            disp.refr_now_for_test();
            let d = probe_btn.coords();
            let (dx, dy) = ((d.x1 + d.x2) / 2, (d.y1 + d.y2) / 2);

            // ① 启用态：对象级 `PRESSED` 收到 ⇒ `(dx,dy)` 确实命中这个控件。
            sh.note_activity();
            sh.tick(at(5100), CLOCK); // 消费 ⇒ last_activity = 5100
            sh.tick(at(5150), CLOCK);
            assert!(sh.countdown_visible(), "④″ 禁用段前置：剩 10 s");
            crate::app::apply_touch_snapshot(
                &sh,
                &indev,
                TouchSnapshot {
                    pressed: true,
                    x: dx,
                    y: dy,
                },
            );
            indev.read();
            assert_eq!(
                hits.get(),
                1,
                "④″ 前置：启用态下 `({dx},{dy})` 确实命中该控件（对象级 `PRESSED` 恰好一次）"
            );
            crate::app::apply_touch_snapshot(
                &sh,
                &indev,
                TouchSnapshot {
                    pressed: false,
                    x: dx,
                    y: dy,
                },
            );
            indev.read();

            // ② 置禁用：同一坐标、同一条投递路径。
            probe_btn.set_disabled(true);
            assert!(
                crate::lvgl::widgets::has_state(&probe_btn, crate::lvgl::style::State::DISABLED),
                "④″ 前置：`LV_STATE_DISABLED` 必须真的置上"
            );
            sh.note_activity();
            sh.tick(at(5200), CLOCK); // 消费 ⇒ last_activity = 5200
            sh.tick(at(5250), CLOCK);
            assert!(sh.countdown_visible(), "④″ 禁用段前置：剩 10 s");
            crate::app::apply_touch_snapshot(
                &sh,
                &indev,
                TouchSnapshot {
                    pressed: true,
                    x: dx,
                    y: dy,
                },
            );
            indev.read();
            assert_eq!(
                hits.get(),
                1,
                "④″ 对照：LVGL 在 `LV_STATE_DISABLED` 的**命中对象**上不发 `PRESSED`\
                 （`lv_indev.c:1339/1342`）—— 这正是旧机制（设备级 `PRESSED` 钩子）漏掉的一幕"
            );
            crate::app::apply_touch_snapshot(
                &sh,
                &indev,
                TouchSnapshot {
                    pressed: false,
                    x: dx,
                    y: dy,
                },
            );
            indev.read();
            sh.tick(at(5250) + Duration::from_millis(500), CLOCK);
            assert!(
                !sh.countdown_visible(),
                "**禁用态控件上的按压也重置计时**（§4.3「任何触摸事件」；B4b 整改「重要 1」的\
                 回归锁）—— 改什么会让本条变红：删掉 `app::apply_touch_snapshot` 里的 \
                 `shell.note_activity()`；或把机制退回「设备级 `PRESSED` 钩子」（该钩子在 \
                 DISABLED 命中对象上不触发，见上一条的对照断言）"
            );
            drop(probe_btn); // 临时控件用完即删（不影响后续段）

            // **对照（机制边界，如实锁住）**：**合成投递**（`Obj::send_event`）**不经** evdev
            // 快照这条路 ⇒ 它**不**会重置计时。这不是产品缺口（产品里的触摸**只**走
            // `pump` → `apply_touch_snapshot`），而是"为什么必须在输入泵上做"这条机制的锚点：
            // 光有外壳根那条对象级挂钩**不够**。
            sh.show(NavPage::Interlock);
            sh.note_activity();
            sh.tick(at(5300), CLOCK);
            sh.tick(at(5350), CLOCK);
            assert!(sh.countdown_visible(), "对照段前置：剩 10 s");
            sh.content_obj().send_event(EventCode::PRESSED); // 合成投递，不经输入泵
            sh.tick(at(5350) + Duration::from_millis(500), CLOCK);
            assert!(
                sh.countdown_visible(),
                "**合成投递**（`send_event`）不经输入泵 ⇒ 不重置计时（机制边界，非产品缺口：\
                 产品里的触摸一律经 `App::pump` 的 evdev 快照）"
            );

            // 收尾：复位到 P1 且**无胶囊**（后续 ④′ 依赖"角标可见"，而角标在胶囊占位时让位）。
            sh.show(NavPage::Main);
            sh.note_activity();
            sh.tick(at(6000), CLOCK);
            assert!(!sh.countdown_visible(), "收尾：活动后回到满时长，无胶囊");
        }

        // ═══ ④‴ 超时回归 ⇒ **P1 始终从顶部开始**（UI §4.3；偏差 **SH12**）════════════
        //
        // §4.3 原文（"超时回归本身"行）：「不产生审计记录；不改变页面滚动位置以外的状态
        // （**P1 始终从顶部开始**）」。该行主语是**超时回归这一条路径**，故回顶只落在
        // `Core::tick` 的强制切页分支，**不**放进 `Core::select`（那会连带"主动返回 / 点页签"
        // 也清滚动位置 —— §4.3 的"主动返回"行只写"切 P1；按下即反馈"，未提滚动位置）。
        //
        // **口径（B4b 整改「建议 3.2」收窄）**：「回归」隐含"**从别页回来**"。超时那一刻
        // **已经在 P1** 时**不**动滚动位置 —— 否则"用户正在 P1 阅读"会被强制回顶，丢掉阅读
        // 位置（那已超出「超时**回归**」的字面：无页可回归）。两条断言分别钉住这两面。
        {
            let p1_root = sh.page_obj(NavPage::Main);
            sh.show(NavPage::Main);
            disp.refr_now_for_test();
            // **先滚离顶部**：否则"回顶"断言会退化成"恒真"（本项目明令禁止的伪门禁）。
            p1_root.scroll_to_y(80);
            let before = p1_root.scroll_y();
            assert!(
                before > 0,
                "④‴ 前置：P1 必须真的能滚（内容高于视口）—— 实得 scroll_y = {before}；\
                 若为 0 则本段的'回顶'断言恒真、必须换更长的注入数据"
            );

            // 已在 P1 ⇒ 超时**不**回顶（阅读位置保留）。
            sh.note_activity();
            sh.tick(at(6100), CLOCK); // 消费 ⇒ last_activity = 6100
            sh.tick(at(6160), CLOCK); // 60 s 到（已在 P1）
            assert_eq!(sh.current(), NavPage::Main, "④‴：已在 P1");
            assert_eq!(
                p1_root.scroll_y(),
                before,
                "**已在 P1 时超时不得回顶**（「回归」隐含从别页回来；回顶会丢掉用户正在读的\
                 位置）—— 改什么会让本条变红：把 `Core::tick` 强制切页分支的守卫\
                 `c.current.get() != NavPage::Main.index()` 摘掉"
            );

            // 切走 → 空闲到 0 s ⇒ 强制回归（**从别页回来**）。
            sh.show(NavPage::Interlock);
            sh.note_activity();
            sh.tick(at(7000), CLOCK); // 消费 ⇒ last_activity = 7000
            sh.tick(at(7060), CLOCK); // 60 s 到 ⇒ 自动切 P1
            assert_eq!(sh.current(), NavPage::Main, "④‴：到 0 s 自动切 P1");
            assert_eq!(
                p1_root.scroll_y(),
                0,
                "**超时回归后 P1 必须从顶部开始**（UI §4.3；SH12）—— 改什么会让本条变红：\
                 删掉 `Core::tick` 强制切页分支里的 `reset_primary_scroll()`"
            );
        }

        // ═══ ④⁗ 手势语义：**滑动不误触发点击**（TT-11）+ **滚动条纯指示不可拖**（§5.6-A）══
        //
        // 工作单元 **L** 补缺：设计 §11.1「HMI 交互」层**逐条点名**这两项，此前**零用例**
        // （全库 grep：无任何用例经 lv_indev 投递"按下→移动→抬起"手势 —— 既有用例的移动
        // 都是程序化 scroll_by_raw / scroll_to_y，证明不了框架的**点击/滚动判别**）。
        //
        // 判据都在**真实命中链**上：Indev::feed → lv_indev_read → lv_indev_search_obj。
        {
            use crate::lvgl::indev::{Indev, TouchSnapshot};

            let p1_root = sh.page_obj(NavPage::Main);
            sh.show(NavPage::Main);
            p1_root.scroll_to_y(0);
            disp.refr_now_for_test();

            /// 投递一套「按下 → 纵向拖 drag_px → 抬起」手势，返回手势前后的 scroll_y 增量。
            ///
            /// drag_px 必须**大于** LV_INDEV_DEF_SCROLL_LIMIT（默认 10 px）—— 这正是 LVGL 的
            /// "点击 / 滚动"判别阈值：拖出阈值 ⇒ 手势被判为滚动，CLICKED 不再派发。
            ///
            /// **每次手势用一枚全新 Indev**（与 ④′ 段同法）：LVGL 的 indev 在抬手后会留下
            /// 抛掷 / 方向锁状态，复用同一枚会让两次手势的增量不同（L 单元实测：同 x 连投三次
            /// 会得到 ≈拖动量与 ≈2 倍拖动量两族值 —— 该现象**与 x 无关**，见 ② 的口径说明）。
            fn swipe_y(disp: &Display, root: &Obj, x: i32, y: i32, drag_px: i32, steps: i32) -> i32 {
                let indev = Indev::create_pointer(disp).expect("④⁗：建真实 indev");
                let before = root.scroll_y();
                indev.feed(TouchSnapshot {
                    pressed: true,
                    x,
                    y,
                });
                indev.read();
                for k in 1..=steps {
                    indev.feed(TouchSnapshot {
                        pressed: true,
                        x,
                        y: y - drag_px * k / steps,
                    });
                    indev.read();
                }
                indev.feed(TouchSnapshot {
                    pressed: false,
                    x,
                    y: y - drag_px,
                });
                indev.read();
                let delta = root.scroll_y() - before;
                drop(indev);
                delta
            }

            // 探针控件 = P1 页根（滚动容器）的**可点子件**（内容区靠上位置）。
            let hits_pressed = Rc::new(Cell::new(0u32));
            let hits_clicked = Rc::new(Cell::new(0u32));
            let probe = crate::lvgl::widgets::TextButton::create(p1_root, "滑动")
                .expect("④⁗：造一个可点探针控件");
            probe.set_pos(120, 120);
            probe.set_size(240, Dimens::TOUCH_MIN);
            let _keep_p = probe.on(EventCode::PRESSED, {
                let p = Rc::clone(&hits_pressed);
                move |_e| p.set(p.get() + 1)
            });
            let _keep_c = probe.on(EventCode::CLICKED, {
                let c = Rc::clone(&hits_clicked);
                move |_e| c.set(c.get() + 1)
            });
            disp.refr_now_for_test();

            let btn = probe.coords();
            let (bx, by) = ((btn.x1 + btn.x2) / 2, (btn.y1 + btn.y2) / 2);
            let root_c = p1_root.coords();
            assert!(
                (root_c.x1..root_c.x2).contains(&bx) && (root_c.y1..root_c.y2).contains(&by),
                "④⁗ 前置：探针中心 ({bx},{by}) 须在 P1 页根 {}..{} × {}..{} 内",
                root_c.x1,
                root_c.x2,
                root_c.y1,
                root_c.y2
            );

            // ── ① TT-11：在控件上滑动 ⇒ 不产生点击，但**真的滚了** ──
            let delta_btn = swipe_y(disp, p1_root, bx, by, 60, 6);
            assert_eq!(
                hits_pressed.get(),
                1,
                "④⁗ 前置：手势起点 ({bx},{by}) 必须真的命中探针控件（对象级 PRESSED 恰一次）"
            );
            assert_eq!(
                hits_clicked.get(),
                0,
                "**TT-11：滑动不得误触发列表项/控件点击**（按下后纵向拖出 60 px ⇒ LVGL 判为                 滚动，CLICKED 不派发）—— 改什么会让本条变红：把 LV_INDEV_DEF_SCROLL_LIMIT                 调大到 60 px 以上 ⇒ 同一次拖动会被当成点击"
            );
            assert!(
                delta_btn > 0,
                "④⁗ 反空壳：这一拖必须**真的滚动**容器（实得 Δscroll_y = {delta_btn}）——                 若为 0，上一条「没触发点击」是恒真的废话（手势根本没被消费）"
            );

            // ── ② §5.6-A：滚动条**不是对象**⇒ 结构上无处可挂拖拽逻辑 ──────────────
            //
            // ⚠️ **口径（L 单元实测订正，如实登记）**：设计 §11.1 的原文判据是"在滚动条带
            // （x ∈ [右缘−8, 右缘]）注入按下→移动→抬起，断言 scroll_y 变化量与**同等手势落在
            // 内容区完全一致**"。**该判据在本机不可靠**：实测（P1 页根、同 y、同 60 px 拖动、
            // 同一 x 连投 3 次）增量呈**双峰且与 x 无关** —— [60, 110, 113] / [60, 60, 113] /
            // [60, 113, 113]…（x = 36…1006 逐点复现，两族 = 拖动量 60 与 ≈113）。既然**量本身**
            // 就是双峰的，"与内容区完全一致"这一比较无法区分"滚动条行为"与"框架残留状态" ⇒
            // 改为断言**条带内起手仍然拖动内容**（不是被吞掉的死区），且增量在拖动量的合理
            // 倍数内（抓 thumb 会把内容**跳到**与拖动无关的位置）。结构性主张（滚动条是绘制
            // 部件、不是对象）由 `lvgl/tests_a3.rs` 的 ScrollContainer 段 + 设计 §5.6-A 的
            // 源码引用（lv_obj.c::draw_scrollbar 仅在 DRAW_POST，输入侧无命中测试）承担。
            let band_x = root_c.x2 - Dimens::SCROLLBAR_MARGIN - Dimens::SCROLLBAR_W / 2;
            let band_y = root_c.y1 + 200;
            assert!(
                band_x > root_c.x1 && band_x < root_c.x2,
                "④⁗ 前置：条带中点 x={band_x} 须在容器内（{root_c:?}）"
            );
            // 对照：**同一 y** 的内容区起手（与条带内起手配对，见上"口径"说明）。
            p1_root.scroll_to_y(0);
            let delta_content = swipe_y(disp, p1_root, root_c.x2 - 60, band_y, 60, 6);
            assert!(
                delta_content > 0,
                "④⁗ 对照：同 y 的内容区起手必须能滚动（实得 Δ = {delta_content}）"
            );
            p1_root.scroll_to_y(0);
            let delta_band = swipe_y(disp, p1_root, band_x, band_y, 60, 6);
            assert!(
                delta_band > 0,
                "④⁗：条带内起手必须**仍然拖动内容**（实得 Δscroll_y = {delta_band}）——\
                 为 0 即「滚动条吃掉了手势」（死区 / 不可穿透），那会让「纯指示」变成「不可用」"
            );
            assert!(
                delta_band <= 3 * 60,
                "④⁗：条带内起手的滚动量 {delta_band} 超出合理倍数（> 3× 拖动量）——\
                 抓 thumb 会把内容**跳到**与拖动无关的位置（实测对照：内容区 = {delta_content}）"
            );
            drop(_keep_p);
            drop(_keep_c);
            drop(probe);
            p1_root.scroll_to_y(0);
            disp.refr_now_for_test();
        }

        // ═══ ④′ 触摸不可用角标（EDGE-13）+ 通道断（EDGE-20 的"两状态同显"）══════
        sh.set_touch_available(false);
        assert!(sh.touch_badge_visible(), "EDGE-13：触摸不可用角标常驻");
        assert_eq!(sh.touch_badge_text().as_deref(), Some(shell::TEXT_TOUCH_UNAVAILABLE));
        assert!(
            sh.channel_text().is_some(),
            "角标与通道胶囊**可同显**（二者槽位不相交，见 shell.rs 的栅格自洽用例）"
        );
        sh.set_touch_available(true);
        assert!(!sh.touch_badge_visible());
        // EDGE-20：读通道断 ⇒ 页眉出现红通道胶囊，且**时钟仍在**（"两种状态同显"）。
        sh.set_channel(ChannelStatus::Down);
        assert_eq!(
            sh.channel_text().as_deref(),
            Some(crate::ui::pages::p1_status::TEXT_CHANNEL_DOWN),
            "EDGE-20：页眉红通道胶囊"
        );
        assert_eq!(sh.clock_text().as_deref(), Some(CLOCK), "时钟不受通道态影响");
        sh.set_channel(ChannelStatus::Connected);
        assert_eq!(sh.channel_text().as_deref(), Some(shell::TEXT_CHANNEL_OK));

        // ═══ ⑤ 未保存修改提示条（EDGE-11）═════════════════════════════════════
        sh.show(NavPage::Config);
        // 先注入一份最小配置（`Shell::new` 只建页，不喂数据 —— 数据接线属 B3）。
        sh.p2()
            .set_config(&shell_p2_view())
            .expect("P2 set_config（外壳链路）");
        sh.tick(at(117), CLOCK);
        assert!(!sh.banner_visible(), "未编辑 ⇒ 无提示条");
        // 程序化改一个可编辑字段（走与控件回调**同一条簿记**：脏标记 / 清错 / 刷按钮）。
        let hit = sh
            .p2()
            .set_field_value("gateway.port", &serde_json::Value::from(2405));
        assert!(hit, "P2 必须含 `gateway.port` 字段");
        assert!(sh.p2().is_dirty(), "改字段 ⇒ `is_dirty()` 为真");
        sh.tick(at(117), CLOCK);
        assert!(sh.banner_visible(), "P2 脏 ⇒ 提示条常驻（EDGE-11）");
        assert_eq!(sh.banner_text().as_deref(), Some(shell::TEXT_DIRTY_BANNER));
        assert_eq!(sh.banner_icon_text().as_deref(), Some(shell::ICON_WARN));
        assert_eq!(
            sh.discard_button().text().as_deref(),
            Some(shell::TEXT_DISCARD)
        );
        // **脏态与计时互斥**：推进到原超时点，**不得**出现胶囊、**不得**切页。
        // **改什么会让本条变红**：把 `Core::tick` 的 `paused` 判据里 `p2.is_dirty()` 删掉 ⇒
        // 这里胶囊出现（甚至切回 P1）⇒ 两条断言同时红。
        sh.tick(at(200), CLOCK);
        assert!(
            !sh.countdown_visible(),
            "P2 脏 ⇒ **不倒计时**（UI §4.3 / EDGE-11）"
        );
        assert_eq!(sh.current(), NavPage::Config, "P2 脏 ⇒ **不强制切页**");
        // 「放弃修改」⇒ 调 `discard_draft()` ⇒ 脏态清除 + 提示条收起。
        sh.discard_button().send_event(EventCode::CLICKED);
        assert!(!sh.p2().is_dirty(), "放弃修改 ⇒ 草稿丢弃（脏态清）");
        assert!(!sh.banner_visible(), "脏态清 ⇒ 提示条即刻收起");
        // 恢复计时：从放弃那一刻**重新起算满时长**（此刻已不再暂停）。
        sh.tick(at(201), CLOCK);
        assert!(!sh.countdown_visible(), "恢复计时 ⇒ 从满时长起算，无胶囊");
        assert_eq!(sh.current(), NavPage::Config);

        // ═══ ⑤′ 确认弹层打开 ⇒ 暂停计时且不显示倒计时（UI §4.3）════════════════
        // **SH2（B2c-3 时的口径）**：当时页侧拿不到"弹层是否打开"（`with_dialog` 是
        // `#[cfg(test)]`）⇒ 只能经注入位驱动。**B3-2c 已闭合**（真源 = 页面的 `dialog_open()`，
        // 见下面 ⑤″ / ⑤″·P4）；本段（⑤′）保留为"外壳**暂停语义**本身"的单独一网：判据经注入位
        // 喂入，故与页面真源**解耦**，两者各自可红。
        sh.set_modal_open(true);
        sh.tick(at(300), CLOCK);
        assert!(
            !sh.countdown_visible(),
            "弹层打开 ⇒ 暂停计时且不显示倒计时（TT-13）"
        );
        assert_eq!(sh.current(), NavPage::Config, "弹层打开 ⇒ 不强制切页");
        sh.set_modal_open(false);
        sh.tick(at(301), CLOCK);
        assert_eq!(sh.current(), NavPage::Config, "弹层关闭 ⇒ 恢复计时（从满时长）");

        // ═══ ⑤″ **生产可见的**弹层查询口（B3-2c：闭合 SH2 的「页侧拿不到」）═══════════
        //
        // ⑤′ 证明的是外壳**暂停语义**本身（经注入位）；本节证明驱动它的**判据现在是可得的**
        // —— 页面侧新增的 `P2ConfigPage::dialog_open()` / `P4InterlockPage::dialog_open()`
        // 必须如实反映"此刻屏上有没有弹层"（`app.tick` 每拍喂的两者取或就取自它们）。
        //
        // **改什么会让本条变红**（**已实测**）：
        // - 把 `P2ConfigPage::dialog_open` 写成恒 `false` ⇒ 「点『保存』⇒ 查询口为真」当场红；
        // - 把 `sh.set_modal_open(..)` 那一行改回常量 `false` ⇒ 「90 s 后仍停在 P2」红。
        sh.show(NavPage::Config);
        assert!(
            !sh.p2().dialog_open() && !sh.p4().dialog_open(),
            "无弹层 ⇒ 两个查询口都为 false"
        );
        // 改一个可编辑字段（走与控件回调同一条簿记）⇒ 有草稿可保存。
        assert!(sh.p2().set_field_value("gateway.port", &serde_json::Value::from(2405)));
        sh.note_activity();
        sh.tick(at(305), CLOCK); // 消费活动 ⇒ 计时基线 = at(305)
        // 「保存」= 真实派发路径（向保存按钮派 `CLICKED`），弹层由页面自己开。
        sh.p2()
            .save_button()
            .button()
            .obj()
            .send_event(EventCode::CLICKED);
        assert!(
            sh.p2().dialog_open(),
            "点「保存」⇒ 弹层已在屏上，**生产可见的**查询口必须为真（恒 false ⇒ 红）"
        );
        assert!(!sh.p4().dialog_open(), "P4 的弹层没打开（两页各持自己的口，互不借光）");

        // 接线层每拍的喂入值 = 两页取或 ⇒ 弹层打开期间计时**暂停**。
        sh.set_modal_open(sh.p2().dialog_open() || sh.p4().dialog_open());
        sh.tick(at(395), CLOCK); // 距基线 90 s ≫ 60 s 的超时 ⇒ 未暂停的话早已切回 P1
        assert!(!sh.countdown_visible(), "弹层打开 ⇒ 不倒计时（TT-13 / UI §4.3）");
        assert_eq!(sh.current(), NavPage::Config, "弹层打开 ⇒ 不强制切页");

        // 关弹层：走**真实取消路径**（事件回调内不关，延迟到下一拍 —— 见 `P2ConfigPage::tick`）。
        sh.p2().with_dialog(|d| {
            d.cancel_button().send_event(EventCode::CLICKED);
        });
        assert!(
            sh.p2().dialog_open(),
            "取消点击发生在 LVGL 回调内 ⇒ 弹层**此刻仍在屏上**，查询口必须如实为真"
        );
        sh.tick(at(396), CLOCK);
        assert!(!sh.p2().dialog_open(), "下一拍的延迟关闭已执行 ⇒ 查询口变假");
        // 脏草稿也会暂停计时（EDGE-11）⇒ 先放弃修改，才能观察"弹层关 ⇒ 恢复计时"。
        sh.p2().discard_draft();
        assert!(!sh.p2().is_dirty(), "前置：脏态已清（否则暂停判据仍为真）");
        sh.set_modal_open(sh.p2().dialog_open() || sh.p4().dialog_open());
        sh.tick(at(397), CLOCK);
        assert_eq!(
            sh.current(),
            NavPage::Config,
            "弹层关闭且无脏草稿 ⇒ 暂停解除（计时从满时长重算，此刻不切页）"
        );
        assert!(!sh.countdown_visible(), "恢复计时 ⇒ 距超时还有 60 s，无胶囊");
        sh.show(NavPage::Main); // 复位：后续 ③ 段从主状态页起
        sh.note_activity();
        sh.tick(at(398), CLOCK);

        // ═══ ⑤″·P4 **P4 侧的**生产可见查询口（B3-2c 规格符合性评审**阻塞 1** 整改）═══════
        //
        // **为什么必须补这一节**：⑤″ 里 P4 只有 `assert!(!sh.p4().dialog_open(), ..)` 这类
        // **否定式**断言（不点 P4 的按钮 ⇒ 它恒成立）⇒ 评审实测：把
        // `P4InterlockPage::dialog_open()` 改成**恒 `false`**（= 本单元开工前的退化形态，
        // 接线层恒喂 `false`）时**354/0/6/6/3 全绿、0 failed** —— TT-13 在 P4 这半
        // **静默失效**。本节按 P2 的**同款真实路径**（向按钮派 `CLICKED`，弹层由页面自己开）
        // 把 P4 走一遍，且**每条断言都同时钉住"本页为真"与"另一页为假"** ⇒
        // 接线层取或（`p2.dialog_open() || p4.dialog_open()`）的**两个操作数都是真的**。
        //
        // **改什么会让本条变红**（**已实测**，见交付报告）：
        // - `P4InterlockPage::dialog_open` 写成**恒 `false`** ⇒ 「点『人工释放联锁』⇒ 查询口为真」
        //   当场红（这正是"恒 false 全套仍绿"缺的那张网）；
        // - 写成**恒 `true`** ⇒ 本节「注入后、点击前：P4 无弹层」先红（若把它删掉，
        //   「关闭后查询口变假」也会红）。
        {
            use mupc_display_proto::InterlockSection;
            // 本节的**独立时间基**（`at(600)` 起，与 ⑤″ 的 305–398 段互不干扰）；
            // 收尾再显式把基线**复位**回 `at(399)`，好让 ③′ 段仍从 `at(400)` 起算。
            sh.show(NavPage::Interlock);
            // P4 的操作按钮在**无帧**时 fail-closed 置灰（§8.3「联锁状态不可用」）⇒
            // 先注入一份可用帧（`available && enabled`，`latched = false` ⇒ 两按钮都可用）。
            sh.p4().set_section(&InterlockSection {
                available: true,
                enabled: true,
                latched: false,
                ..Default::default()
            });
            assert!(
                !sh.p4().dialog_open(),
                "注入可用帧后、**点击前**：P4 屏上无弹层（恒 true 的口在这里先红）"
            );
            assert!(!sh.p2().dialog_open(), "P2 的弹层此刻也不该在（互不借光）");
            sh.note_activity();
            sh.tick(at(600), CLOCK); // 消费活动 ⇒ 计时基线 = at(600)
            // 真实派发路径：向「人工释放联锁」按钮派 `CLICKED`，弹层由页面自己开。
            sh.p4()
                .release_button()
                .button()
                .obj()
                .send_event(EventCode::CLICKED);
            assert!(
                sh.p4().dialog_open(),
                "点「人工释放联锁」⇒ P4 弹层已在屏上，**生产可见的**查询口必须为真\
                 （恒 false ⇒ 红；B3-2c 评审阻塞 1）"
            );
            assert!(
                !sh.p2().dialog_open(),
                "P4 弹层 ≠ P2 弹层（两页各持自己的口，**互不借光**）"
            );
            // 接线层每拍的喂入值 = 两页取或 ⇒ P4 弹层打开期间**同样暂停计时**（P4 侧也守一次 TT-13）。
            sh.set_modal_open(sh.p2().dialog_open() || sh.p4().dialog_open());
            sh.tick(at(690), CLOCK); // 距基线 90 s ≫ 60 s 的超时 ⇒ 未暂停的话早已切回 P1
            assert!(!sh.countdown_visible(), "P4 弹层打开 ⇒ 不倒计时（TT-13 / UI §4.3）");
            assert_eq!(sh.current(), NavPage::Interlock, "P4 弹层打开 ⇒ 不强制切页");
            // 关弹层：走**真实取消路径**（事件回调内不关，延迟到下一拍 —— 同 `P2ConfigPage::tick`）。
            sh.p4().with_dialog(|d| {
                d.cancel_button().send_event(EventCode::CLICKED);
            });
            assert!(
                sh.p4().dialog_open(),
                "取消点击发生在 LVGL 回调内 ⇒ 弹层**此刻仍在屏上**，查询口必须如实为真"
            );
            sh.tick(at(691), CLOCK);
            assert!(!sh.p4().dialog_open(), "下一拍延迟关闭已执行 ⇒ P4 查询口变假（恒 true ⇒ 红）");
            sh.set_modal_open(sh.p2().dialog_open() || sh.p4().dialog_open());
            sh.tick(at(692), CLOCK);
            assert_eq!(
                sh.current(),
                NavPage::Interlock,
                "P4 弹层关闭（本页无草稿概念）⇒ 暂停解除（计时从满时长重算，此刻不切页）"
            );
            assert!(!sh.countdown_visible(), "恢复计时 ⇒ 距超时还有 60 s，无胶囊");
            // **基线复位**：后面的 ③′ 段仍以 `at(400)` 为独立时间基。
            // `note_activity()` 的消费分支**无条件**把 `last_activity` 覆盖为 `now`
            // （见 `Shell::tick` ①），故这里能把基线拉回到 at(399)。
            sh.show(NavPage::Main);
            assert!(!sh.p4().dialog_open() && !sh.p2().dialog_open(), "复位前：两页弹层都已关");
            sh.set_modal_open(sh.p2().dialog_open() || sh.p4().dialog_open());
            sh.note_activity();
            sh.tick(at(399), CLOCK);
        }

        // ═══ ③ 顶层图层挂点（弹层 / Toast；**SH8 收口后不再是 EDGE-03 的落点**）═══════════
        // 「整屏降级层在它**之下**」这一拓扑是 EDGE-20 的成立条件（通道断时弹层与写路径仍可用）。
        // ⚠️ 该拓扑**无可离屏断言**（薄层无 parent / z-order 读回口）—— 实测：把整屏层改建到
        // `lv_layer_top()` 之下，全套用例仍绿（2026-09-15 探针；原因见 ⑥ 的"整屏降级层"注释）。
        // 故它**如实登记为评审复核项**，单点落在 `Shell::new` 的装配行。
        //
        // **B3-2b-1 规格符合性评审整改（建议 4）**：原先这里调 `Shell::overlay_layer()` ——
        // 那个方法**已删**（与 `widgets::layer_top()` 等价、只多包一层 `Result`，且**无生产
        // 消费者**：P2/P4 弹层与 Toast 直接调 `widgets::layer_top()`）。本断言的目标改为
        // **直接调薄层挂点**，仍证"挂点本身可用"（弹层 / Toast 的宿主**不必**依赖整屏层存在）。
        let layer = crate::lvgl::widgets::layer_top().expect("顶层浮层可用");
        assert!(layer.is_alive(), "顶层浮层挂点必须返回可用对象");

        // ═══ ③′ **整屏降级（EDGE-03）**（B3-2b-1 新增实现；四条裁定见 `shell.rs` 模块头）═══
        //
        // 判据逐条对应 §8.3 EDGE-03 原文 + 主控的四条裁定。时间基取 `at(400)` 起的**独立**
        // 一段（与前面各段的超时链路互不干扰）。
        sh.show(NavPage::Main);
        sh.set_channel(ChannelStatus::Connected);
        let t_down = at(400);
        sh.tick(t_down, CLOCK); // 落定"通道正常"这一拍
        assert!(
            !sh.overlay_visible(),
            "通道正常 ⇒ 无整屏遮罩（EDGE-03 只在通道断时出现）"
        );
        // **裁定 ① 的静态那一半**：层在 `Shell::new` 里建好、常驻（写成局部变量 ⇒ 早已被级联删除）。
        assert!(
            sh.overlay_obj().is_alive(),
            "整屏层必须常驻（装配期建好、只切可见性）—— 写成本地变量会在这里红"
        );
        assert!(
            !sh.overlay_obj().has_flag(crate::lvgl::obj::ObjFlag::CLICKABLE),
            "整屏遮罩**不得**带 CLICKABLE（**可穿透输入**：EDGE-20 要求 P2/P4 写操作仍可用）"
        );

        // ── 通道断 ⇒ 遮罩 + 中央 64 px 大字 + 时长从 0 起 ──
        sh.set_channel(ChannelStatus::Down);
        sh.tick(t_down, CLOCK);
        assert!(sh.overlay_visible(), "通道断 ⇒ 整屏遮罩可见（EDGE-03）");
        assert_eq!(
            sh.overlay_title_text().as_deref(),
            Some(crate::ui::pages::p1_status::TEXT_CHANNEL_DOWN),
            "中央大字 = §3.6 全局降级行原文（与页眉红胶囊同一份字面量）"
        );
        assert_eq!(
            sh.overlay_elapsed_text().as_deref(),
            Some("0 秒"),
            "时长**从 0 起**（断开始刻 = 进入断态的第一拍）"
        );
        assert_eq!(sh.overlay_elapsed_secs(), Some(0));
        assert_eq!(sh.overlay_text_writes(), 1, "首次上屏写一次文本");
        // 中央大字的字号档 = 64 px（UI §8.3「中央 64 px」）。
        disp.refr_now_for_test();
        assert_eq!(
            sh.overlay_obj().size(),
            (Dimens::SCREEN_W, Dimens::SCREEN_H),
            "整屏遮罩 = 全屏 1024×768（`pad_all(0)` 已清 ⇒ 不缩进）"
        );
        let tc = sh.overlay_obj().coords();
        assert_eq!(
            (tc.x1, tc.y1, tc.x2, tc.y2),
            (0, 0, Dimens::SCREEN_W - 1, Dimens::SCREEN_H - 1),
            "遮罩须**整屏**贴边（吃默认主题内边距 ⇒ 四边露白 ⇒ 红）"
        );

        // ── **对象级字号档断言**（B3-2b-1 规格评审 **重要 1+2**）────────────────────────
        //
        // **为什么需要它**（历史动因，**B4a 之前**）：当时薄层**没有**字号 / 字色读回
        // （`bg_opa` / `text_font` / `text_color` 三个 getter 都缺，见 `src/lvgl/mod.rs`
        // 的「薄层能力缺口登记」）⇒ 评审把中央大字由 `TextSlot::PhasePower`(64 px) +
        // `Palette::STALE` 换成 `TextSlot::Body` + `Palette::DANGER` 时**全套用例全绿**
        // （屏上字变小变红，无网）。根因是"建标签"与"设尺寸"两处**各写各的常量**
        // ⇒ 只改一处不影响另一处。
        //
        // **【B4a 订正】** 三个 getter **已补齐**（`Obj::bg_opa` / `text_color` / `text_font`）
        // ⇒ 字色从此**有真读回口**（见下方 §B4a 补网）；而本节的**尺寸**口径仍**照旧需要**
        // —— 它是**独立于**样式读回的第二条路（"字号滑档而 `set_size` 没跟"这类退化，
        // 只有"对象实高 == 槽档高"这条能抓）。
        //
        // **收口**：`shell.rs` 把两处收敛成同一个绑定（`OVERLAY_TITLE_SLOT` /
        // `OVERLAY_ELAPSED_SLOT`），`set_size` 与"建标签"同源；而 `Obj::size()` **可读**
        // ⇒ 断言对象实际高度 == 契约槽档高，"换错槽"当场红（探针实测见交付报告）。
        //
        // **改什么会让本条变红**：把 `shell.rs` 的 `OVERLAY_TITLE_SLOT` 换成 `TextSlot::Body`
        // （⇒ 大字对象高 24 ≠ 64）、或把 `OVERLAY_ELAPSED_SLOT` 换成 `TextSlot::PhasePower`
        // （⇒ 时长行对象高 64 ≠ 24）、或把两者的 `set_size` 实参写死成另一个数。
        let title_h = sh.overlay_title_obj().size().1;
        let elapsed_h = sh.overlay_elapsed_obj().size().1;
        assert_eq!(
            title_h,
            TextSlot::PhasePower.px() as i32,
            "中央大字对象高必须 == `TextSlot::PhasePower`.px()（UI §8.3「中央 64 px」）\
             —— 换错字号槽（如 Body 的 24）在这里红"
        );
        assert_eq!(
            elapsed_h,
            TextSlot::Body.px() as i32,
            "时长行对象高必须 == `TextSlot::Body`.px()（UI §8.3「下方 24 px」）\
             —— 换错字号槽（如 PhasePower 的 64）在这里红"
        );
        assert_ne!(
            title_h, elapsed_h,
            "两行的字号槽必须不同（64 ≠ 24）：同高即「两处写反 / 写成同一个槽」，两行分不出主次"
        );

        // ── **遮罩不透明度档位**（重要 1+2 的第 3 步）──────────────────────────────────
        //
        // 收口 = 把档位提成**单一真源** `shell::OVERLAY_MASK_OPACITY`，遮罩只许经它建
        // ⇒ 先断言该常量本身（"对象**实际生效值**"那一层见下方 §B4a 补网）。
        //
        // **【B4a 订正】** 原文续写「薄层无 `lv_obj_get_style_bg_opa` ⇒ "遮罩实际用了哪一档"
        // 在对象层**不可判**（评审实测：把 20 % 偷换成 62 %，335 全绿）…**残余**：绕过该常量
        // 直接写死别的档位，用例仍抓不到（补读回属「薄层收口批」）」—— **已过期**，
        // 且与**正下方**新增的 `sh.overlay_obj().bg_opa()` 断言**自相矛盾**。
        // 事实：薄层**已补** `Obj::bg_opa`（`lvgl/mod.rs` 的 **G1**），那条断言读的就是
        // **LVGL 实际生效值** ⇒"绕开常量写死 62 %"**当场红**。
        // 于是上面这两条"常量相等"退居**第一道网**（钉住"单点真源"本身，保证换档只改一处）。
        assert_eq!(
            shell::OVERLAY_MASK_OPACITY,
            Opacity::DEGRADE_PERCENT,
            "整屏降级遮罩档位 == 20 %（UI §8.3 EDGE-03「整屏遮罩压暗 20 %」）"
        );
        assert_ne!(
            shell::OVERLAY_MASK_OPACITY,
            Opacity::MASK_PERCENT,
            "整屏降级遮罩**不得**取模态遮罩那一档（62 %）：EDGE-20 要求底层仍可读 ⇒ 二者必须分档"
        );

        // ── ═══ **B4a 补网：样式读回**（把上面两条"常量相等"升级为"对象实际值相等"）═══ ──
        //
        // **为什么必须补**：上面那两条只证明"**常量**是 20 而非 62" —— 若有人**绕过**
        // `shell::OVERLAY_MASK_OPACITY`、直接在 `overlay_mask()` 的样式上写死别的档位
        // （或把 `Opacity::DEGRADE_PERCENT` 本身改掉而没人同步），**B4a 之前**对象层读不回
        // ⇒ 全部用例仍绿（B3-2b-1 评审与 `shell.rs` 的登记都点名过这条残余）。
        //
        // B4a 补齐薄层的 `Obj::bg_opa` / `text_color` / `text_font`（三个
        // `lv_obj_get_style_*` 的等价读回，见 `src/lvgl/obj.rs`）后，这三件**在对象上可读**，
        // 于是改为断言 **LVGL 实际生效值**。
        //
        // **改什么会让下面每条变红**（探针实测见 B4a 交付报告）：
        // ① 把遮罩档位由 20 % 换成 62 %（含绕过常量直接写死）⇒ `bg_opa()` 读回 158 ≠ 51；
        // ② 把 `OVERLAY_TITLE_COLOR` 由 `Palette::STALE`(`#FFB020`) 换成 `Palette::DANGER` ⇒ 字色读回不等；
        // ③ **【B4a 整改「建议 3.1」按实测订正】** 原文写「把大字标签的样式换成不带字体的那条
        //    ⇒ `text_font()` 变 `None`（或不再是 `font_of(槽)`）」—— **两个分支都不成立**。
        //    实测（探针：把 `pages::label` 的 `theme::text(slot, color)` 换成一条**只设字色、
        //    不设字体**的样式，其余原样）：`shell_chain` **仍绿** ⇒ `text_font()` 既**没**变
        //    `None`、也**仍** `== font_of(槽)`。**原因**：LVGL 默认主题给每个 `lv_obj` 挂
        //    `LV_FONT_DEFAULT`（= `lv_font_montserrat_14`），而默认构建下
        //    `theme::font_of(槽)` 对**所有**槽都回落 [`Font::fallback`] = **同一个**
        //    `lv_font_montserrat_14` 指针 ⇒ "样式里没设字体"与"设了槽的字体"**读回同值**。
        //    ∴ ③ 在当前构建下**不是**一条能红的判据（见下方字体断言处的如实标注）。
        assert_eq!(
            sh.overlay_obj().bg_opa(),
            Opa::percent(Opacity::DEGRADE_PERCENT),
            "整屏降级遮罩**实际生效的** `bg_opa` 必须 == 20 %（= `Opa::percent(20)` = 51/255）\
             —— 绕开 `shell::OVERLAY_MASK_OPACITY` 直接写死 62 % 在这里红"
        );
        assert_eq!(
            sh.overlay_obj().bg_opa().raw(),
            51,
            "遮罩 `bg_opa` 的**原始 C 取值**必须 == 51（`Opa::percent(20)` 的算式在这条上被钉死）"
        );
        assert_ne!(
            sh.overlay_obj().bg_opa(),
            Opa::percent(Opacity::MASK_PERCENT),
            "遮罩**实读**值不得等于模态那一档（62 % = 158）—— 与上面 `!= 62` 的常量断言同向，\
             但这条查的是 **LVGL 真正渲染用的那个值**"
        );
        assert_eq!(
            sh.overlay_title_obj().text_color(),
            shell::OVERLAY_TITLE_COLOR,
            "中央大字的**实际字色**必须 == `OVERLAY_TITLE_COLOR`（UI §8.3 `#FFB020`）\
             —— 由 STALE 换成 DANGER 红即在此红（原先「色在对象层不可判」的残余）"
        );
        assert_ne!(
            sh.overlay_title_obj().text_color(),
            Palette::DANGER,
            "中央大字**不得**用 DANGER 红（EDGE-03 原文给的是 `#FFB020` 琥珀）"
        );
        assert_eq!(
            sh.overlay_elapsed_obj().text_color(),
            shell::OVERLAY_ELAPSED_COLOR,
            "时长行的**实际字色**必须 == `OVERLAY_ELAPSED_COLOR`"
        );
        assert_eq!(
            sh.overlay_title_obj().text_font(),
            Some(theme::font_of(shell::OVERLAY_TITLE_SLOT)),
            "中央大字的**实际字体指针**必须 == `font_of(OVERLAY_TITLE_SLOT)`\
             —— ⚠️ **判别力受构建配置限制（B4a 整改「建议 3.1」按实测订正）**：\
             默认构建（未启用 `noto-font`）下各槽位都降级到**同一个** fallback 指针，\
             而 LVGL 默认主题给裸 `lv_obj` 挂的 `LV_FONT_DEFAULT` 也是那**同一个**\
             `lv_font_montserrat_14` ⇒ 本条此时**连「字体被设过」都证不了**\
             （探针实测：把标签样式换成不带字体的那条，本条**仍绿**）。\
             只有启用 `noto-font` 构建（各槽拿到不同指针）后，本条才有\
             「换错槽 / 漏设字体即红」的判别力（边界见 `Obj::text_font` 的说明）"
        );
        assert_eq!(
            sh.overlay_elapsed_obj().text_font(),
            Some(theme::font_of(shell::OVERLAY_ELAPSED_SLOT)),
            "时长行的**实际字体指针**必须 == `font_of(OVERLAY_ELAPSED_SLOT)`（边界同上一行）"
        );

        // ── **只在整秒变化时改文本**（每拍刷 LVGL 文本 = 红线行为 + 使"秒级"无从验证）──
        // **改什么会让本条变红**：把 `Core::apply_overlay` 的 `!= Some(secs)` 判据删掉
        // （无条件 `set_text`）⇒ 下面每一条 `overlay_text_writes()` 断言都红。
        sh.tick(t_down + Duration::from_millis(500), CLOCK);
        assert_eq!(sh.overlay_text_writes(), 1, "半秒拍：整秒未变 ⇒ 不得重写文本");
        assert_eq!(sh.overlay_elapsed_text().as_deref(), Some("0 秒"));
        sh.tick(t_down + Duration::from_secs(1), CLOCK);
        assert_eq!(sh.overlay_elapsed_text().as_deref(), Some("1 秒"), "整秒拍 ⇒ 时长 +1");
        assert_eq!(sh.overlay_text_writes(), 2);
        for i in 2..=5u64 {
            sh.tick(t_down + Duration::from_secs(i) + Duration::from_millis(300), CLOCK);
        }
        assert_eq!(sh.overlay_elapsed_secs(), Some(5));
        assert_eq!(sh.overlay_elapsed_text().as_deref(), Some("5 秒"));
        assert_eq!(
            sh.overlay_text_writes(),
            6,
            "0..=5 s 恰好 6 次整秒变化 ⇒ 恰好 6 次写入"
        );

        // ═══ ③‴ **可穿透输入的"行为级"证据**（B3-2b-1 代码质量评审 建议 4）════════════
        //
        // **为什么不能只留旗标断言**：上面那条 `!overlay_obj().has_flag(CLICKABLE)` 只证明
        // "**标志位**没被误加"，不证明"触摸**真的**穿过去了" —— 它无法发现"遮罩的子对象
        // 被加了 `CLICKABLE`"这类改法（旗标断言只问遮罩**本身**）。本仓薄层**已有**
        // `Indev`（`lvgl/indev.rs`，与真机同一条命中链：`read_cb` 快照 → `lv_indev_read` →
        // `lv_indev_search_obj`），故**真实命中测试是可行的**（无需改薄层、无新增生产代码）。
        //
        // **两个投递点**（各投一次"按下"，各自一个独立 `Indev`，**从不**投抬手 ⇒ 全程无
        // `CLICKED`、不切页）：
        //
        // - **A = 未选中页签的中心**（`Config` 页签，y ≈ 732）：证明"**下层控件真的收到**"
        //   —— 该点在**导航条**上，遮罩**整屏覆盖**它，故只有"可穿透"成立时页签才收得到
        //   （§8.3 EDGE-20「P2/P4 写操作仍可用」）。
        // - **B = 中央大字的中心**（`overlay_title_obj`）：证明"**遮罩整棵子树都不吃事件**"
        //   —— 该点由遮罩的**子标签**覆盖（A 点不覆盖），故它专抓"给子标签加回 `CLICKABLE`"
        //   这一类改法（**旗标断言只问遮罩本身，抓不到它**）。
        //
        // 命中链依据：`lv_indev_read` → `lv_indev_search_obj`（`vendor/lvgl/src/indev/lv_indev.c:617`）
        // —— `hit_test_ok == false`（即不带 `CLICKABLE`）的对象**不**被返回，但**仍会**递归
        // 其子树；整棵子树都无命中时才交还给更早的兄弟。
        //
        // **改什么会让本条变红**：① 给整屏遮罩 `add_flag(CLICKABLE)`（`lv_obj` 构造时**默认带**
        // `CLICKABLE`，`vendor/lvgl/src/core/lv_obj.c:584` ⇒ 删掉 `Shell::new` 里那行
        // `remove_flag(CLICKABLE)` 即等价）⇒ A 点：页签计数 0、遮罩计数 1；② 给中央大字
        // （或时长行）加回 `CLICKABLE` ⇒ B 点：遮罩子树计数 1。
        sh.set_channel(ChannelStatus::Down);
        sh.tick(t_down + Duration::from_millis(5_500), CLOCK);
        disp.refr_now_for_test();
        assert!(
            sh.overlay_visible(),
            "前置：遮罩可见（这一拍命中链上真的压着它）"
        );
        {
            use crate::lvgl::indev::{Indev, TouchSnapshot};
            let hits_tab = Rc::new(Cell::new(0u32));
            let hits_title = Rc::new(Cell::new(0u32));
            let hits_elapsed = Rc::new(Cell::new(0u32));
            let hits_overlay = Rc::new(Cell::new(0u32));
            {
                let tab = sh
                    .tab(NavPage::Config)
                    .expect("第 2 个页签")
                    .button()
                    .obj();
                let c = Rc::clone(&hits_tab);
                let _keep = tab.on(EventCode::PRESSED, move |_e| c.set(c.get() + 1));
            }
            for (o, c) in [
                (sh.overlay_obj(), Rc::clone(&hits_overlay)),
                (sh.overlay_title_obj(), Rc::clone(&hits_title)),
                (sh.overlay_elapsed_obj(), Rc::clone(&hits_elapsed)),
            ] {
                let _keep = o.on(EventCode::PRESSED, move |_e| c.set(c.get() + 1));
            }
            // 命中判定读的是 `obj->coords`（布局趟写入）⇒ 上面已 `refr_now_for_test()` 落定。
            let centre = |o: &Obj| {
                let c = o.coords();
                ((c.x1 + c.x2) / 2, (c.y1 + c.y2) / 2)
            };
            // ── A：页签中心 ──
            let (ax, ay) = centre(
                sh.tab(NavPage::Config)
                    .expect("第 2 个页签")
                    .button()
                    .obj(),
            );
            assert!(
                (0..Dimens::SCREEN_W).contains(&ax) && (0..Dimens::SCREEN_H).contains(&ay),
                "前置：页签中心 ({ax},{ay}) 须在屏内（否则本条测的不是「屏上被遮罩盖住的点」）"
            );
            let indev = Indev::create_pointer(disp)
                .expect("Indev::create_pointer（可穿透输入的行为级证据 A）");
            indev.feed(TouchSnapshot {
                pressed: true,
                x: ax,
                y: ay,
            });
            indev.read();
            assert_eq!(
                hits_tab.get(),
                1,
                "A：遮罩可见时投递一次按下，下层页签**必须收到**（§8.3 EDGE-20）\
                 —— 收到 0 次 ⇒ 触摸被整屏遮罩拦住（「可穿透」不成立）"
            );
            assert_eq!(
                hits_overlay.get(),
                0,
                "A：整屏遮罩**自身**不得收到该事件 —— 收到 1 次 ⇒ 遮罩可点 = 拦穿透，EDGE-20 失效"
            );
            drop(indev);
            // ── B：中央大字中心（专抓"子标签被加回 CLICKABLE"）──
            let (bx, by) = centre(sh.overlay_title_obj());
            let indev = Indev::create_pointer(disp)
                .expect("Indev::create_pointer（可穿透输入的行为级证据 B）");
            indev.feed(TouchSnapshot {
                pressed: true,
                x: bx,
                y: by,
            });
            indev.read();
            assert_eq!(
                (hits_overlay.get(), hits_title.get(), hits_elapsed.get()),
                (0, 0, 0),
                "B：投递点 ({bx},{by}) 由遮罩的**子标签**覆盖 ⇒ 遮罩整棵子树（容器 / 中央大字 / \
                 时长行）**一个都不得收到**（收到即「该子件可拦截触摸」，遮罩的「可穿透」被绕过）"
            );
            drop(indev);
        }
        // **不影响后续**：全程只投 `pressed`、**不**投抬手 ⇒ 无 `CLICKED` ⇒ 不切页
        // （页签的切页挂钩在 `on_clicked` 上）。
        assert_eq!(sh.current(), NavPage::Main, "行为级用例不得切页（只按下、未抬手）");

        // ═══ ③⁗ `Init` 态**不显**整屏降级层（B3-2b-1 代码质量评审 建议 3）════════════
        //
        // **理由**：§8.3 的异常态字典里**没有** `Init` 的整屏行（见 `shell.rs` 模块头裁定 ④）
        // —— 首次连接中的既有落点是**页眉黄胶囊**（`HeaderChannel::Connecting`）+ **P1 首连条**。
        // 若把 `Init` 也算作"断"，启动瞬间就会全屏压暗、"尚未首次连上"被渲染成"连接已断开"。
        //
        // **改什么会让本条变红**：把 `apply_overlay` 的触发判据由 `HeaderChannel::Down` 放宽为
        // `!matches!(self.channel.get(), HeaderChannel::Connected)`（或任何把 `Init` 归入
        // "需降级"的写法）⇒ 遮罩在本条位置可见 ⇒ 红。
        sh.set_channel(ChannelStatus::Init);
        sh.tick(t_down + Duration::from_millis(5_800), CLOCK);
        assert!(
            !sh.overlay_visible(),
            "`Init`（首次连接中）**不显**整屏降级层 —— §8.3 无该行；既有落点 = 页眉黄胶囊 + P1 首连条"
        );
        assert_eq!(
            sh.channel_text().as_deref(),
            Some(crate::ui::pages::p1_status::TEXT_CHANNEL_CONNECTING),
            "同一拍上：`Init` 的落点是页眉胶囊（「正在连接数据通道」），不是整屏层 \
             —— §8.3 的整屏行只归 `Down`"
        );

        // ── **裁定 ① 的动态那一半**：渲染路径（`tick`）**绝不建 / 删 LVGL 对象** ──
        //
        // ⚠️ **本段曾是一条"看着在把关、实则没把住"的假网（2026-09-15 探针实测，如实登记）**：
        // 初版连推 50 拍用的是**同一个时刻**（`t_down + 5.9 s`）⇒ 整秒数恒为 5 ⇒
        // `Core::apply_overlay` 的"整秒变化才写"分支**一次都不进** ⇒ 把 `Obj::create` 塞进
        // 那个分支的破坏性探针**照样全绿**（实测）。修法 = 第二次循环**逐拍推进 1 s**
        // （每拍都跨整秒 ⇒ 写入分支必走），并**交替通道态**（显 / 隐两支都走）。
        // 这样"建对象"无论写在哪一支里都会被本段抓住。
        //
        // **⚠️ 双计数器（B3-2b-1 代码质量评审 重要 1；2026-09-16 整改）**：只断言
        // `PROBE_MOUNTS` **不够** —— 它数的是"挂了几个对象句柄"，抓不到**另一条独立形态**：
        // `Obj::add_style` **只增不删**，故"每拍挂一次样式（对象只建一次）"会让 LVGL 样式表
        // 无界增长、整屏层照样把内存吃光，而对象数**纹丝不动**（评审实测：在
        // `Core::apply_overlay` 写分支插一次 `add_style` ⇒ 原 335 条全绿）。故本段**同时**
        // 断言 `PROBE_STYLE_ATTACHES` **零增长** —— 与对象数**同款口径**（同为测试专用
        // 静态计数器、同为区间增量、自增点同为薄层唯一入口）。
        let mounts_before = crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst);
        let attaches_before =
            crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(std::sync::atomic::Ordering::SeqCst);
        for i in 0..50u64 {
            sh.set_channel(if i % 2 == 0 {
                ChannelStatus::Connected
            } else {
                ChannelStatus::Down
            });
            sh.tick(t_down + Duration::from_secs(6 + i), CLOCK);
        }
        assert_eq!(
            crate::lvgl::obj::PROBE_MOUNTS.load(std::sync::atomic::Ordering::SeqCst) - mounts_before,
            0,
            "断态连推 50 拍（逐拍跨整秒 + 显隐交替）不得新建任何 LVGL 对象（层建一次、只切可见性）"
        );
        assert_eq!(
            crate::lvgl::obj::PROBE_STYLE_ATTACHES.load(std::sync::atomic::Ordering::SeqCst)
                - attaches_before,
            0,
            "断态连推 50 拍**也不得往对象上挂任何样式**：`Obj::add_style` 只增不删 \
             （`ui/pages/mod.rs::set_style_index` 有先例），「渲染路径每拍挂一条样式」= 样式表无界 \
             增长 ⇒ 整屏层照样把 LVGL 定容池吃光，而**对象数不变** —— 故本条与上一条必须成对"
        );
        // 收尾：回到断态并落定，供下面"恢复即撤"用（上一轮 i=49 ⇒ 奇数 ⇒ Down）。
        sh.set_channel(ChannelStatus::Down);
        sh.tick(t_down + Duration::from_secs(56), CLOCK);
        assert!(sh.overlay_visible());

        // ── **恢复 ≤1 s 回实时**：`Down → Connected` ⇒ **下一拍即撤** ──
        // **改什么会让本条变红**：把 `apply_overlay` 的非断态分支删掉（遮罩留在屏上）。
        sh.set_channel(ChannelStatus::Connected);
        sh.tick(t_down + Duration::from_secs(57), CLOCK);
        assert!(
            !sh.overlay_visible(),
            "通道恢复 ⇒ **本拍即撤**遮罩（≤1 s 回实时）"
        );
        assert_eq!(
            sh.overlay_elapsed_secs(),
            None,
            "恢复 ⇒ 断开始刻清零（下次断开重新从 0 起算）"
        );
        // 再次断开 ⇒ **从 0 重新起算**（不是接着上次的 6 s）。
        sh.set_channel(ChannelStatus::Down);
        sh.tick(t_down + Duration::from_secs(100), CLOCK);
        assert_eq!(
            sh.overlay_elapsed_secs(),
            Some(0),
            "重新断开 ⇒ 时长从 0 重新起算"
        );
        assert_eq!(sh.overlay_elapsed_text().as_deref(), Some("0 秒"));
        sh.set_channel(ChannelStatus::Connected);
        sh.tick(t_down + Duration::from_secs(101), CLOCK);
        assert!(!sh.overlay_visible());

        // ═══ ③″ `--idle-timeout-secs 0` = **禁用**空闲回归（原 `timing::IdleTimer` 语义，
        //      该类型已按 B3-2b-1 任务书删除 ⇒ 唯一落点搬到 `Core::tick`，见偏差 **SH17**）═══
        //
        // **改什么会让本条变红**：删掉 `Core::tick` 里的 `let disabled = c.timeout.get().is_zero();`
        // ⇒ `remaining_secs` 恒 0 ⇒ `should_return_home(0)` 恒真 ⇒ **每拍强制回 P1**（下述第一条红）。
        sh.set_idle_timeout(0);
        sh.show(NavPage::Interlock);
        sh.tick(at(700), CLOCK);
        sh.tick(at(7000), CLOCK); // 远超默认 60 s（禁用 ⇒ 不得切页、不得出胶囊）
        assert_eq!(
            sh.current(),
            NavPage::Interlock,
            "--idle-timeout-secs 0 ⇒ **禁用**空闲回归，不得强制切页"
        );
        assert!(!sh.countdown_visible(), "禁用 ⇒ 不显示倒计时胶囊");
        // 恢复默认 60 s ⇒ 语义照旧。
        sh.set_idle_timeout(theme::Timing::IDLE_TIMEOUT_SECS);
        sh.note_activity();
        sh.tick(at(7100), CLOCK);
        sh.tick(at(7160), CLOCK);
        assert_eq!(sh.current(), NavPage::Main, "恢复 60 s 后超时回归照常生效");
    }

    // ═══ ⑥ 建 / 拆外壳 ≥2 次（R1：`Rc` 环 / 泄漏）══════════════════════════════
    //
    // **改什么会让本条变红**：把 `Shell::wire` 里的 `Rc::downgrade(&self.core)` 改成
    // `Rc::clone`（回调持强引用）⇒ 环 = `Core → 控件 → 回调 → Rc<Core>` ⇒ `drop(shell)`
    // **不释放任何对象** ⇒ 本节第 2 轮起 `Shell::new` 在 1 MB 定容池上 **OOM 挂死**（不是红）；
    // 把 `Shell::new` 里某个拥有型句柄改成局部变量（如 `let capsule = layout_box(..)` 不存字段）
    // ⇒ 该子树随返回被删除 ⇒ 断言"装配后须存活"当场红。
    for round in 0..3 {
        let sh = Shell::new(&home).unwrap_or_else(|e| panic!("第 {round} 轮建外壳失败：{e:?}"));
        // 探针：装配后**此刻**这些子树必须真的在树上（局部变量写法在这里就红）。
        let probes = [
            ("页眉", sh.header_obj().share_borrowed()),
            ("内容区", sh.content_obj().share_borrowed()),
            ("页根宿主", sh.page_host_obj().share_borrowed()),
            ("底部导航", sh.nav_obj().share_borrowed()),
            ("提示条", sh.banner_obj().share_borrowed()),
            // 整屏降级层（EDGE-03）：**随外壳一起释放**。
            //
            // ⚠️ **本条目到底锁了什么（2026-09-15 探针实测订正，勿再写错）**：它锁的是
            // 「整屏层是 `Core` 的**拥有型字段**」，**不是**"父对象是 `root`"。实测：把整屏层
            // 改建到会话级单例 `lv_layer_top()` 之下，**三条断言全绿** —— 因为拥有型句柄的
            // `Drop` 本身就会 `lv_obj_delete`，与父是谁无关。**能红**的改法是：把它写成
            // 非拥有句柄（`adopt_borrowed` / 不再存进 `Core` 而只留局部）。
            //
            // 「整屏层**在** `lv_layer_top()` 之下（⇒ 弹层 / Toast 压得住它）」这一拓扑
            // **无可离屏断言**（薄层没有 parent / z-order 读回口）⇒ 如实登记为**评审复核项**，
            // 单点落在 `Shell::new` 的那一行（见 `shell.rs` 装配注释）。
            ("整屏降级层", sh.overlay_obj().share_borrowed()),
            ("返回键", sh.back_button().share_borrowed()),
            ("取消修改键", sh.discard_button().share_borrowed()),
        ];
        for (what, o) in &probes {
            assert!(o.is_alive(), "第 {round} 轮：探针建立时 `{what}` 必须存活");
        }
        let pages = [
            sh.page_obj(NavPage::Main).share_borrowed(),
            sh.page_obj(NavPage::Config).share_borrowed(),
            sh.page_obj(NavPage::Logs).share_borrowed(),
            sh.page_obj(NavPage::Interlock).share_borrowed(),
            sh.page_obj(NavPage::Audit).share_borrowed(),
            sh.page_obj(NavPage::System).share_borrowed(),
        ];
        disp.refr_now_for_test();
        drop(sh);
        for (what, o) in &probes {
            assert!(
                !o.is_alive(),
                "第 {round} 轮：`drop(Shell)` 之后 `{what}` 必须已被级联删除 —— 仍存活 ⇒ \
                 存在 `Rc` 强引用环（整壳泄漏）"
            );
        }
        for (i, o) in pages.iter().enumerate() {
            assert!(
                !o.is_alive(),
                "第 {round} 轮：`drop(Shell)` 之后第 {i} 页必须已被级联删除（壳持页 = 单向）"
            );
        }
        assert!(
            home.is_alive(),
            "第 {round} 轮：外壳析构后宿主须仍存活（壳不得持有宿主）"
        );
    }

    drop(home);
}

/// **U-73 外设接线的离屏链路（T21c-3）**：两条生产入口 —— 帧段（[`apply_frame_sections`]）
/// 与 catalog 回执（[`apply_catalog_to_pages`]）—— 各自"**真的把数据送到了页面上**"。
///
/// # 为什么必须是**行为**用例（而不是源码哨）
///
/// T21c-1 / T21c-2 交付的两页**已经**有 `set_periph` / `set_catalog` 入口，但它们此前
/// **零生产调用点** ⇒ 屏上 P4 消防区与 P6 四个外设段**恒为「外设数据不可用」**、BMS 288 位
/// 下钻永远打不开。这种"入口写好了但没接线"的缺陷在**源码哨**下完全隐形（两边都在），
/// 只有**跨层的行为断言**能抓。
///
/// # 判据（三段）
///
/// | # | 注入 | 断言 |
/// |---|------|------|
/// | ① | **无帧**（`PageInput::init`） | 两页都显「外设数据不可用」（§15.6.2 ⑤）：P4 的段顶通告 + P6 段「空调」的段级文案行 |
/// | ② | 有帧（`available = true`，含 hvac + fire 站） | 两页都出现**真值**：P6 段顶站状态条显在线 + 数据行**不是 `--`**；P4 的 A1 段顶通告收起、火警等级出帧值（1.0 ⇒ 「一级报警」）。**此时无 catalog** ⇒ 名位一律「名称未获取」/「未定义位 n」 |
/// | ③ | `apply_catalog_to_pages`（**一次**调用） | **两页都**拿到同一份 catalog：P6 名位出**短标签**（不再是「名称未获取」）、P4 的 A1 位行出**位名**（不再是「未定义位 14」） |
///
/// **探针 ③ 的靶子**：把 [`apply_frame_sections`] 里的两行 `set_periph` 摘掉 ⇒ 第 ①② 段
/// 立刻红（两页都回到「外设数据不可用」/ 名位恒「名称未获取」）。
///
/// **前提**：`lvgl::init()` 已由 [`pages_chain`] 调过（本函数**不**再 init/deinit）；
/// `disp` 是同一会话的内存 display。
pub(crate) fn u73_wiring_chain(disp: &mut Display, screen: &Obj) {
    use crate::app::{apply_catalog_to_pages, apply_frame_sections};
    use crate::ui::pages::p6_system::RowKind;
    use crate::ui::pages::PageInput;
    use crate::ui::shell::Shell;
    use mupc_display_proto::peripherals_labels::ui_text;
    use mupc_display_proto::PeripheralCatalog;

    // 宿主与 [`shell_chain`] 同款（屏的忠实替身：清内边距 + 透明）。
    let home = Obj::create(screen).expect("u73 host");
    home.set_size(Dimens::SCREEN_W, Dimens::SCREEN_H);
    home.set_pos(0, 0);
    home.add_style(&theme::transparent(), StyleSelector::main());
    let shell = Shell::new(&home).expect("Shell::new (u73)");

    // 段「空调」（`SEGMENTS[1]`）—— P6 外设段的代表（P4 走消防片）。
    const SEG_HVAC: usize = 1;

    /// 段 `i` 里**第一个**指定种类的行的下标（行模型由页面给出 ⇒ 不依赖版式常量）。
    fn find_row(shell: &Shell, seg: usize, want: RowKind) -> usize {
        shell
            .p6()
            .segment_rows(seg)
            .expect("段行模型")
            .iter()
            .position(|(k, _)| *k == want)
            .unwrap_or_else(|| panic!("段 {seg} 里没有 {want:?} 行"))
    }

    shell.p6().select_segment(SEG_HVAC);

    // ── ① 无帧 ⇒ 两页都显「外设数据不可用」（§15.6.2 ⑤；**不得**显 0 / 「正常」）──
    apply_frame_sections(&shell, &PageInput::init());
    assert!(
        shell.p4().a1_notice_visible(),
        "无帧时 P4 消防区必须显段顶通告（「外设数据不可用」）"
    );
    assert_eq!(
        shell.p4().a1_notice_text().as_deref(),
        Some(ui_text::PERIPH_UNAVAILABLE),
        "P4 的不可用文案必须取 `ui_text`（不得自造串）"
    );
    let at = find_row(&shell, SEG_HVAC, RowKind::Notice);
    let (n1, _, _, _) = shell
        .p6()
        .segment_row_text(SEG_HVAC, at)
        .expect("段级文案行");
    assert_eq!(
        n1,
        ui_text::PERIPH_UNAVAILABLE,
        "无帧时 P6 各段必须显「外设数据不可用」（实得「{n1}」）"
    );

    // ── ② 有帧（available = true）⇒ **两页都拿到值**（但尚无 catalog）────────────
    //
    // 夹具与 P4 / P6 各自的既有用例同源（`u73_fire_fixture` / `p6_catalog` + `p6_section`）。
    // **只造一次**：帧内段（第 ② 段）+ 同一份 catalog（第 ③ 段）。
    let (fire_sec, fire_cat) = u73_fire_fixture(3, 3, None, 1.0);
    let mut sec = p6_section(
        &p6_catalog(&[(mupc_display_proto::PeriphRole::Hvac, true)]),
        &[mupc_display_proto::PeriphRole::Hvac],
        900,
    );
    // 把消防站并进同一帧（两页共用一帧 ⇒ 与生产一致）。
    sec.stations.extend(fire_sec.stations);
    sec.catalog_rev = 7;
    let mut frame = frame_healthy();
    frame.peripherals = sec;
    apply_frame_sections(&shell, &PageInput::live(&frame));

    assert!(
        !shell.p4().a1_notice_visible(),
        "有帧且站在线时 P4 的段顶通告必须收起（否则「外设数据不可用」会常驻）"
    );
    assert_eq!(
        shell.p4().fire_value_text().as_deref(),
        Some("一级报警"),
        "P4 的火警等级必须取到**帧内值**（夹具 at6 = 1.0；无 catalog 时走 `fire_level_text_static` 回退）"
    );
    // P6：段顶站状态条 + 数据行**真值**。
    let st_at = find_row(&shell, SEG_HVAC, RowKind::Station);
    let (n2, st2, ok2, _) = shell
        .p6()
        .segment_row_text(SEG_HVAC, st_at)
        .expect("段顶站状态条");
    assert!(
        n2.starts_with(ui_text::STATION_PREFIX),
        "段顶站状态条第 1 列 = 「站 <角色>」（实得「{n2}」）"
    );
    assert_eq!(
        st2,
        ui_text::ONLINE,
        "帧内 `online = true` ⇒ 站状态条显在线"
    );
    // 第 3 列 = 「成功 <时刻>」。**不锁具体时刻**（那是时区相关的另一条用例的判据）：
    // 只锁"有**真值**时刻"（区别于帧内无该站时的 `–`）。
    assert!(
        ok2.starts_with(ui_text::LAST_OK_SHORT) && !ok2.contains(crate::ui::pages::PLACEHOLDER),
        "第 3 列必须出**帧内**的成功时刻（实得「{ok2}」）"
    );
    let sc_at = find_row(&shell, SEG_HVAC, RowKind::Scalar);
    let (lbl, val, _u, _) = shell
        .p6()
        .segment_row_text(SEG_HVAC, sc_at)
        .expect("数值行");
    assert_ne!(
        val,
        crate::ui::pages::PLACEHOLDER,
        "帧内的值必须真的铺上屏（不得是 `--`）"
    );
    // **无 catalog** ⇒ 名位只能是「名称未获取」（**不臆造中文名**，§15.3.1）。
    assert_eq!(
        lbl,
        ui_text::NAME_UNKNOWN,
        "未注入 catalog 时名位必须显「名称未获取」（实得「{lbl}」）"
    );
    // P4 同理：无 catalog ⇒ 位名不可得 ⇒ A1 的 bit14 显「名称未获取」（§15.3.1 的"中文名位"；
    // ⚠️ 与「未定义位 n」**互异** —— 后者是"catalog 在、但该位不在表内"，见 `refresh_fire`）。
    let a1_14 = shell.p4().a1_row_text(14).unwrap_or_default();
    assert_eq!(
        a1_14,
        ui_text::NAME_UNKNOWN,
        "无 catalog 时 A1 位行必须显「名称未获取」（实得「{a1_14}」）"
    );

    // ── ③ catalog **同一份供两页**（一次调用 ⇒ 两页都必须变）──────────────
    let cat: PeripheralCatalog = PeripheralCatalog {
        stations: {
            let mut s = p6_catalog(&[(mupc_display_proto::PeriphRole::Hvac, true)]).stations;
            s.extend(fire_cat.stations);
            s
        },
        ..p6_catalog(&[(mupc_display_proto::PeriphRole::Hvac, true)])
    };
    assert_eq!(cat.rev, 7, "夹具前提：与帧内 `catalog_rev` 同源同值");
    apply_catalog_to_pages(&shell, &cat);
    // ⚠️ **P6 的段重绘在"下一次帧驱动 render"上**（生产：catalog 回执当拍注入，下一帧的
    // `render_pages_if_needed` 按新 catalog 重算行 —— `p6.set_catalog` 只存不重绘，
    // 这是页面既有的 dirty 机制，**不另造第二条重绘路径**）。此处显式补一次同一帧的
    // `apply_frame_sections` 来代表"下一帧"（生产上线后 ≤1 帧 = ≤1 s 内可见）。
    apply_frame_sections(&shell, &PageInput::live(&frame));

    // ③-a **P6**：名位出短标签（catalog 真的到了这一页）。
    let sc_at = find_row(&shell, SEG_HVAC, RowKind::Scalar);
    let (label, _, _, _) = shell
        .p6()
        .segment_row_text(SEG_HVAC, sc_at)
        .expect("数值行");
    assert_ne!(
        label,
        ui_text::NAME_UNKNOWN,
        "catalog 注入后 P6 的名位必须出短标签（漏喂这一页只表现为「名称未获取」）"
    );
    assert!(!label.is_empty(), "短标签不得为空（实得「{label}」）");
    // ③-b **P4**：A1 的 bit14 出位名（catalog 真的到了这一页）。
    let a1_14 = shell.p4().a1_row_text(14).unwrap_or_default();
    assert!(
        a1_14.contains("主电故障"),
        "catalog 注入后 P4 的 A1 位行必须出**位名**（实得「{a1_14}」）—— 两页都要拿到 catalog"
    );

    disp.refr_now_for_test();
    drop(shell);
    assert!(home.is_alive(), "外壳析构后宿主须仍存活（壳不得持有宿主）");
    drop(home);
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥′ 页面层静态约束 + 码表覆盖率（`ui/pages/**` **不在** `ui_static_constraints`
//     的扫描范围内 —— 该用例只扫 `ui/mod.rs` / `theme.rs` / `components.rs`，
//     此处为三个新文件补一条**独立**用例，不改动既有用例）
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn pages_static_constraints() {
    let sources: [(&str, &str); 3] = [
        ("ui/pages/mod.rs", include_str!("pages/mod.rs")),
        ("ui/pages/p1_status.rs", include_str!("pages/p1_status.rs")),
        ("ui/pages/p6_system.rs", include_str!("pages/p6_system.rs")),
    ];
    // 共用清单（[`FORBIDDEN_UI_SYMBOLS`]，M6）+ 页面专属的裸色值构造两条。
    let forbidden = FORBIDDEN_UI_SYMBOLS;
    for (name, src) in sources {
        let lower = strip_comments_and_literals(src, name).to_ascii_lowercase();
        for needle in forbidden {
            assert!(
                !lower.contains(needle),
                "{name} 不得出现 `{needle}`（设计 §11.1/§11.4 静态约束）"
            );
        }
        // 页面不得出现 `Color::hex` / `Color::rgb` 裸构造（必须走 theme 命名常量）
        for needle in ["Color::hex(", "Color::rgb("] {
            assert!(
                !lower.contains(&needle.to_ascii_lowercase()),
                "{name} 不得出现 `{needle}`（设计 §11.1/§11.4 静态约束）"
            );
        }
        // 额外：页面不得出现 `unsafe`（薄层是唯一允许处）
        assert!(
            !strip_comments_and_literals(src, name).contains("unsafe"),
            "{name} 不得出现 `unsafe`（设计 §1.1.1.2 纪律 1）"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥⁽⁵⁾ **输入控件层**静态约束（`ui/controls.rs`，B2b-1）
//
// 既有的 [`ui_static_constraints`] 只扫 `ui/mod.rs` / `theme.rs` / `components.rs`；
// [`pages_static_constraints`] 只扫 `ui/pages/**`。B2b-1 新增的 `ui/controls.rs` 同样属
// `ui/**`，故按 B2a 的先例**追加**一条独立用例（**不改动**既有用例的扫描面）。
// `ui/controls.rs` 的**裸尺寸**与**码表**两条网另由 [`UI_PROD_SOURCES`] 的扩展覆盖
// （[`ui_layout_setters_use_theme_constants`] / [`ui_texts_covered_by_font_cmap`]）。
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn controls_static_constraints() {
    let sources: [(&str, &str); 1] = [("ui/controls.rs", include_str!("controls.rs"))];
    for (name, src) in sources {
        let code = strip_comments_and_literals(src, name);
        let lower = code.to_ascii_lowercase();
        // ① 零文本输入（F12 红线）/ 裸色值 / `lv_refr_now` / 直连绑定（共用清单）
        for needle in FORBIDDEN_UI_SYMBOLS {
            assert!(
                !lower.contains(needle),
                "{name} 不得出现 `{needle}`（设计 §11.1/§11.4 静态约束）"
            );
        }
        // ② 色值只准出现在 `theme.rs`（命名常量）
        for needle in ["Color::hex(", "Color::rgb("] {
            assert!(
                !lower.contains(&needle.to_ascii_lowercase()),
                "{name} 不得出现 `{needle}`（必须经 theme 的命名常量）"
            );
        }
        // ③ `unsafe` 只准出现在 `src/lvgl/**`
        assert!(
            !code.contains("unsafe"),
            "{name} 不得出现 `unsafe`（设计 §1.1.1.2 纪律 1）"
        );
        // ④ **自证本扫描真的覆盖到了输入控件**（若 `include_str!` 指错文件 / 文件被清空，
        //    上面三条会**构造性全绿** —— 这正是"看着在把关、实则没把住"的典型形态）。
        for must in ["SegmentedControl", "Ipv4Stepper", "DateTimeStepper", "set_one_checked"] {
            assert!(
                code.contains(must),
                "{name} 未包含 `{must}` —— 本用例的扫描面与预期不符（先修用例再谈实现）"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥⁽⁵⁾″ **应用外壳**静态约束（`ui/shell.rs`，B2c-3）
//
// 既有的四条静态用例的扫描面各写死自己的文件（**本批不改它们**）⇒ 按
// `controls_static_constraints` 的先例**追加**一条独立用例。外壳的**裸尺寸**与**码表**两条网
// 另由 [`UI_PROD_SOURCES`] 的扩展覆盖（[`ui_layout_setters_use_theme_constants`] /
// [`ui_texts_covered_by_font_cmap`]），**常量定义式**一条由 [`CONST_I32_SCAN_SOURCES`] 覆盖。
// ═══════════════════════════════════════════════════════════════════════════

/// 外壳的静态约束 + **扫描面自证**（对应任务书探针 **P1**）。
///
/// **改什么会让本条变红**：把 `ui/shell.rs` 从 [`UI_PROD_SOURCES`] / [`CONST_I32_SCAN_SOURCES`]
/// 任一张清单里删掉 ⇒ ⓪ 段响亮失败（那两张网会**静默失去**对外壳的覆盖）；在 `shell.rs` 里写
/// `lv_textarea_create` / `Color::hex(..)` / `unsafe` ⇒ ① ② ③ 段点名文件与 token。
#[test]
fn shell_static_constraints() {
    const SHELL: &str = "ui/shell.rs";
    // ⓪ 扫描面自证：两张清单**必须真的含本文件**（与 `p2_static_constraints` 同法）。
    assert!(
        UI_PROD_SOURCES.iter().any(|(n, _)| *n == SHELL),
        "{SHELL} 必须在 UI_PROD_SOURCES 内（码表覆盖率 + 裸尺寸两条网覆盖它）"
    );
    assert!(
        CONST_I32_SCAN_SOURCES.iter().any(|(n, _)| *n == SHELL),
        "{SHELL} 必须在 CONST_I32_SCAN_SOURCES 内（常量定义式一条网覆盖它）"
    );

    let src = include_str!("shell.rs");
    let code = strip_comments_and_literals(src, SHELL);
    let lower = code.to_ascii_lowercase();
    // ① 零文本输入 / 裸色值调用 / 强制渲染 / 直连绑定（共用清单）
    for needle in FORBIDDEN_UI_SYMBOLS {
        assert!(
            !lower.contains(needle),
            "{SHELL} 不得出现 `{needle}`（设计 §11.1/§11.4 静态约束）"
        );
    }
    // ② 色值只准出现在 `theme.rs`（命名常量）
    for needle in ["Color::hex(", "Color::rgb("] {
        assert!(
            !lower.contains(&needle.to_ascii_lowercase()),
            "{SHELL} 不得出现 `{needle}`（必须经 theme 的命名常量）"
        );
    }
    // ③ `unsafe` 只准出现在 `src/lvgl/**`
    assert!(
        !code.contains("unsafe"),
        "{SHELL} 不得出现 `unsafe`（设计 §1.1.1.2 纪律 1）"
    );
    // ④ 自证扫描面未指错文件（`include_str!` 指到别的文件 ⇒ 上面三条构造性全绿）。
    for must in ["NavPage", "Shell", "COUNTDOWN_WINDOW_SECS", "set_on_page_change"] {
        assert!(
            code.contains(must),
            "{SHELL} 未包含 `{must}` —— 扫描面与预期不符（先修用例再谈实现）"
        );
    }
    // ⑤ **回调槽纪律**（R1）：外壳不得手写 `RefCell<Option<Box<dyn FnMut(..)>>>` 形状的裸槽
    //    —— 一律走 `pages::CbSlot`（它自带"取出 → 调用 → 放回"与 panic 守卫）。
    assert!(
        code.contains("CbSlot"),
        "{SHELL} 的切页通知槽必须走 `pages::CbSlot`（不得手写 take / put-back）"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥⁽⁵⁾′ **P2 配置页**静态约束（`ui/pages/p2_config.rs`，B2b-2）
//
// 既有的 [`pages_static_constraints`] 的扫描面写死为 B2a 的三个文件（**本批不改既有用例的
// 扫描面**）⇒ 按 `controls_static_constraints` 的先例**追加**一条独立用例。
// 本文件的**裸尺寸**与**码表**两条网另由 [`UI_PROD_SOURCES`] 的扩展覆盖
// （[`ui_layout_setters_use_theme_constants`] / [`ui_texts_covered_by_font_cmap`]）。
// ═══════════════════════════════════════════════════════════════════════════

/// P2 配置页的静态约束 + **扫描面自证**。
///
/// **零文本输入红线**（UI §2.4 / §5.3「界面不存在任何可编辑文本区」）在这里逐 token 断言：
/// 删掉/绕过任一控件而改用 `lv_textarea` / `lv_spinbox` / `lv_keyboard` ⇒ 本条变红。
///
/// **敏感性（探针 P3 的姊妹网）**：往 `p2_config.rs` 的生产区插入 `lv_textarea_create(..)`
/// ⇒ 本条立刻点名文件并失败（码表网管不到这一条 —— 它只管字形）。
#[test]
fn p2_static_constraints() {
    // ⓪ **扫描面自证**（探针 P1）：`UI_PROD_SOURCES`（码表覆盖率 + 裸尺寸）与
    //    `CONST_I32_SCAN_SOURCES`（常量定义式）**必须真的含本文件** —— 把本文件从任一张网的
    //    清单里删掉，那两张网会**静默失去对 P2 的覆盖**（"网看着还在、实则漏了一片"，
    //    正是本项目反复点名的失效形态）。本条把这件事变成**响亮失败**。
    for (list, label) in [
        (UI_PROD_SOURCES.as_slice(), "UI_PROD_SOURCES"),
        (CONST_I32_SCAN_SOURCES.as_slice(), "CONST_I32_SCAN_SOURCES"),
    ] {
        assert!(
            list.iter().any(|(n, _)| *n == "ui/pages/p2_config.rs"),
            "`{label}` 未含 `ui/pages/p2_config.rs` —— 该网的**扫描面**已把 P2 漏掉\
             （先修清单：新文件必须纳入，否则静态网对新代码是空的）"
        );
    }

    // ⑤ **`config_key(` 的计数自证**（B2b-2 规格评审 ⑤）：`NON_DISPLAY_SINKS` 里的
    //    `config_key(` 是**后缀匹配**——紧跟它的字面量会被**豁免**出"上屏候选"走查。
    //    该豁免在本页**正当**（配置字段的**机器键**确不上屏：`gateway.listen_addr` 里的小写
    //    ASCII 在生成字体里没有字形），但它同时是一条**可能被误用的后门**：谁把**上屏串**
    //    写成 `config_key("…")`，那条串就**静默逃过**码表网。
    //    ⇒ 把"恰好 3 处"钉死（= `LABEL_OVERRIDES` 的三个键）。新增/删除键 ⇒ 本条**响亮失败**，
    //    强制下一个改动者停下来复核"它真的是机器键吗"。
    //
    //    口径：在生产区（掐掉测试模块）里数 `config_key("` —— **带引号**，故 `const fn config_key(`
    //    的定义式**不计入**（定义是"实现"，3 处调用才是"标注点"）。注释里若出现同一 token
    //    也会计入 ⇒ 那是有意的：宁可响亮失败也不要静默豁免。
    {
        let (name, src) = ("ui/pages/p2_config.rs", include_str!("pages/p2_config.rs"));
        let prod = truncate_before_test_module(src, name);
        assert_eq!(
            prod.matches("config_key(\"").count(),
            3,
            "{name}：`config_key(\"…\")` 必须**恰为 3 处**（`LABEL_OVERRIDES` 的三个机器键）。\
             计数变化 = 有新的字面量被声明为「非屏显」——请逐条复核它**确实是机器键**\
             （键不上屏才可豁免；**上屏文案**放进 `config_key(..)` 会从码表网里消失）"
        );
        assert_eq!(
            crate::ui::pages::p2_config::LABEL_OVERRIDES.len(),
            3,
            "覆盖表条目数必须与上面的计数一致（两处一起改才自洽）"
        );
    }

    let sources: [(&str, &str); 1] = [("ui/pages/p2_config.rs", include_str!("pages/p2_config.rs"))];
    for (name, src) in sources {
        let code = strip_comments_and_literals(src, name);
        let lower = code.to_ascii_lowercase();
        // ① 零文本输入（F12 红线）/ 裸色值 / `lv_refr_now` / 直连绑定（共用清单）。
        for needle in FORBIDDEN_UI_SYMBOLS {
            assert!(
                !lower.contains(needle),
                "{name} 不得出现 `{needle}`（设计 §11.1/§11.4 静态约束）"
            );
        }
        // ② 色值只准出现在 `theme.rs`（命名常量）。
        for needle in ["Color::hex(", "Color::rgb("] {
            assert!(
                !lower.contains(&needle.to_ascii_lowercase()),
                "{name} 不得出现 `{needle}`（必须经 theme 的命名常量）"
            );
        }
        // ③ `unsafe` 只准出现在 `src/lvgl/**`。
        assert!(
            !code.contains("unsafe"),
            "{name} 不得出现 `unsafe`（设计 §1.1.1.2 纪律 1）"
        );
        // ④ **自证本扫描真的覆盖到了 P2 配置页**（若 `include_str!` 指错文件 / 文件被清空，
        //    上面三条会**构造性全绿** —— "看着在把关、实则没把住"的典型形态）。
        for must in [
            "P2ConfigPage",
            "set_config",
            "set_unavailable",
            "show_result",
            "Ipv4Stepper",
            "SegmentedControl",
            "ConfirmDialog",
        ] {
            assert!(
                code.contains(must),
                "{name} 未包含 `{must}` —— 本用例的扫描面与预期不符（先修用例再谈实现）"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥⁽⁵⁾″ **P4 安全 / 联锁页**静态约束（`ui/pages/p4_interlock.rs`，B2b-3）
//
// 既有的 [`pages_static_constraints`] 的扫描面写死为 B2a 的三个文件、[`p2_static_constraints`]
// 写死为 P2（**本批不改既有用例的扫描面**）⇒ 按前两条的先例**追加**一条独立用例。
// 本文件的**裸尺寸**与**码表**两条网另由 [`UI_PROD_SOURCES`] 的扩展覆盖
// （[`ui_layout_setters_use_theme_constants`] / [`ui_texts_covered_by_font_cmap`]）。
// ═══════════════════════════════════════════════════════════════════════════

/// P4 安全 / 联锁页的静态约束 + **扫描面自证** + **`source_key(` 计数自证**。
///
/// **零文本输入红线**（UI §2.4 / §5.3「界面不存在任何可编辑文本区」）在这里逐 token 断言：
/// 删掉/绕过任一控件而改用 `lv_textarea` / `lv_spinbox` / `lv_keyboard` ⇒ 本条变红。
///
/// **敏感性（探针 P3 的姊妹网）**：往 `p4_interlock.rs` 的生产区插入 `lv_textarea_create(..)`
/// ⇒ 本条立刻点名文件并失败（码表网管不到这一条 —— 它只管字形）。
#[test]
fn p4_static_constraints() {
    // ⓪ **扫描面自证（探针 P1）**：`UI_PROD_SOURCES`（码表覆盖率 + 裸尺寸）与
    //    `CONST_I32_SCAN_SOURCES`（常量定义式）**必须真的含本文件** —— 把本文件从任一张网的
    //    清单里删掉，那两张网会**静默失去对 P4 的覆盖**（"网看着还在、实则漏了一片"，
    //    正是本项目反复点名的失效形态）。本条把这件事变成**响亮失败**。
    for (list, label) in [
        (UI_PROD_SOURCES.as_slice(), "UI_PROD_SOURCES"),
        (CONST_I32_SCAN_SOURCES.as_slice(), "CONST_I32_SCAN_SOURCES"),
    ] {
        assert!(
            list.iter().any(|(n, _)| *n == "ui/pages/p4_interlock.rs"),
            "`{label}` 未含 `ui/pages/p4_interlock.rs` —— 该网的**扫描面**已把 P4 漏掉\
             （先修清单：新文件必须纳入，否则静态网对新代码是空的）"
        );
    }

    // ⑤ **`source_key(` 的计数自证**：`NON_DISPLAY_SINKS` 里的 `source_key(` 是**后缀匹配** ——
    //    紧跟它的字面量会被**豁免**出"上屏候选"走查。该豁免在 P4 **正当**（触发源**机器名**
    //    确不上屏：`estop` 里的小写 ASCII 在生成字体里没有字形，上屏的是中文名），但它同时是
    //    一条**可能被误用的后门**：谁把**上屏串**写成 `source_key("…")`，那条串就**静默逃过**
    //    码表网。⇒ 把"恰好 2 处"钉死（= `SOURCE_LABELS` 的两个 token）。
    {
        let (name, src) = (
            "ui/pages/p4_interlock.rs",
            include_str!("pages/p4_interlock.rs"),
        );
        let prod = truncate_before_test_module(src, name);
        assert_eq!(
            prod.matches("source_key(\"").count(),
            2,
            "{name}：`source_key(\"…\")` 必须**恰为 2 处**（`SOURCE_LABELS` 的两个机器名 token）。\
             计数变化 = 有新的字面量被声明为「非屏显」——请逐条复核它**确实是机器名**\
             （机器名不上屏才可豁免；**上屏文案**放进 `source_key(..)` 会从码表网里消失）"
        );
        assert_eq!(
            crate::ui::pages::p4_interlock::SOURCE_LABELS.len(),
            2,
            "映射表条目数必须与上面的计数一致（两处一起改才自洽）"
        );
    }

    // ⑤′ **U-73（T21c-1）新增的两条豁免后门**的计数自证 —— 同 ⑤ 的机制与理由：
    //     `group_title("…")` / `block_key("…")` 紧跟的字面量会被**豁免**出"上屏候选"走查。
    //     两处在 P4 **正当**（分组键与块名是**小写 ASCII 机器键**，上屏的是它们查出来的中文），
    //     但同样可能被误用（把**上屏串**写成 `group_title("…")` ⇒ 静默逃过码表网）⇒ 钉死计数。
    {
        let (name, src) = (
            "ui/pages/p4_interlock.rs",
            include_str!("pages/p4_interlock.rs"),
        );
        let prod = truncate_before_test_module(src, name);
        assert_eq!(
            prod.matches("group_title(\"").count(),
            6,
            "{name}：`group_title(\"…\")` 必须**恰为 6 处**（新页 5 张卡的分组标题 + 下钻标题 1 处）。\
             计数变化 = 有新的字面量被声明为「非屏显」——请逐条复核它**确实是分组键**"
        );
        assert_eq!(
            prod.matches("block_key(\"").count(),
            12,
            "{name}：`block_key(\"…\")` 必须**恰为 12 处**（`fire_sys` / `fire_det` 两个块名的\
             全部查找点）。计数变化 = 有新的字面量被声明为「非屏显」——请逐条复核它**确实是块名**"
        );
        // **键确实存在**：本页用到的 6 个分组键都必须在 `GROUP_TITLES` 里有非空标题
        // （漏一个 ⇒ 屏上那块**没有标题**，属静默降级）。
        for key in [
            "fire_level",
            "fire_sys_status",
            "fire_cylinder",
            "fire_trigger",
            "fire_detector",
        ] {
            let t = crate::ui::pages::p4_interlock::group_title(key);
            assert!(
                !t.is_empty(),
                "分组键 `{key}` 在 `GROUP_TITLES` 里没有标题（屏上无处安放）"
            );
        }
    }

    let sources: [(&str, &str); 1] = [(
        "ui/pages/p4_interlock.rs",
        include_str!("pages/p4_interlock.rs"),
    )];
    for (name, src) in sources {
        let code = strip_comments_and_literals(src, name);
        let lower = code.to_ascii_lowercase();
        // ① 零文本输入（F12 红线）/ 裸色值 / `lv_refr_now` / 直连绑定（共用清单）。
        for needle in FORBIDDEN_UI_SYMBOLS {
            assert!(
                !lower.contains(needle),
                "{name} 不得出现 `{needle}`（设计 §11.1/§11.4 静态约束）"
            );
        }
        // ② 色值只准出现在 `theme.rs`（命名常量）。
        for needle in ["Color::hex(", "Color::rgb("] {
            assert!(
                !lower.contains(&needle.to_ascii_lowercase()),
                "{name} 不得出现 `{needle}`（必须经 theme 的命名常量）"
            );
        }
        // ③ `unsafe` 只准出现在 `src/lvgl/**`。
        assert!(
            !code.contains("unsafe"),
            "{name} 不得出现 `unsafe`（设计 §1.1.1.2 纪律 1）"
        );
        // ④ **自证本扫描真的覆盖到了 P4 联锁页**（若 `include_str!` 指错文件 / 文件被清空，
        //    上面三条会**构造性全绿** —— "看着在把关、实则没把住"的典型形态）。
        for must in [
            "P4InterlockPage",
            "InterlockOpPayload",
            "set_on_release",
            "set_on_ack_m1",
            "show_result",
            "show_reject",
            "ConfirmDialog",
            "UnavailableKind",
            "LedIndicator",
            "StatusChip",
        ] {
            assert!(
                code.contains(must),
                "{name} 未包含 `{must}` —— 本用例的扫描面与预期不符（先修用例再谈实现）"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥⁽⁵⁾‴ **P5 审计页 + 共享筛选件**的静态约束（`ui/pages/p5_audit.rs` / `ui/pages/filters.rs`，
//         B2c-1）
//
// 既有的 [`pages_static_constraints`]（B2a 三文件）/ [`p2_static_constraints`]（P2）/
// [`p4_static_constraints`]（P4）的扫描面都写死（**本批不改既有用例的扫描面**）⇒ 按前三条的
// 先例**追加**独立用例。两个新文件的**裸尺寸**与**码表**两条网另由 [`UI_PROD_SOURCES`] /
// [`CONST_I32_SCAN_SOURCES`] 的扩展覆盖。
// ═══════════════════════════════════════════════════════════════════════════

/// P5 审计页 + 共享筛选件的静态约束 + **扫描面自证** + **`audit_key(` 计数自证** +
/// **只读约束（写入口结构性不存在）**。
///
/// **零文本输入红线**（UI §2.4 / §5.3）：删掉 / 绕过任一控件而改用 `lv_textarea` /
/// `lv_spinbox` / `lv_keyboard` ⇒ 本条变红（共用清单 [`FORBIDDEN_UI_SYMBOLS`]）。
///
/// **只读红线**（UI §6.5「只读约束」行 / PL-02）：本页**不构造**任何按钮 / 弹层 / Toast，
/// 也不出现"删除 / 清空 / 导出"类的**代码标识符** ⇒ 现场无法在屏上找到写入口。
/// ⚠️ **本条的边界（如实）**：它扫的是**代码文本**（剥掉注释与字面量），拦的是"有人加了
/// 按钮 / 删改 API"；**上屏文案**里的写操作词由 `p5_audit.rs::tests::write_entries_is_empty`
/// 管，**运行期**的可点对象数由 `pages_chain` 的 `list_clickable_count() == 0` 管。
/// 三条合起来才构成本页的只读网（任一条单独都不充分）。
#[test]
fn p5_static_constraints() {
    // ⓪ **扫描面自证（探针 P1）**：三张清单必须真的含两个新文件 —— 把文件从任一张网的清单里
    //    删掉，那张网会**静默失去覆盖**（"网看着还在、实则漏了一片"）。本条让它响亮失败。
    for (list, label) in [
        (UI_PROD_SOURCES.as_slice(), "UI_PROD_SOURCES"),
        (CONST_I32_SCAN_SOURCES.as_slice(), "CONST_I32_SCAN_SOURCES"),
    ] {
        for f in ["ui/pages/p5_audit.rs", "ui/pages/filters.rs"] {
            assert!(
                list.iter().any(|(n, _)| *n == f),
                "`{label}` 未含 `{f}` —— 该网的**扫描面**漏了新文件\
                 （先修清单：新文件必须纳入，否则静态网对新代码是空的）"
            );
        }
    }
    // 码表覆盖率网还必须挂上两个新文件的 `ALL_TEXTS`（清册 ↔ 源码字面量一致性）。
    for t in crate::ui::pages::filters::ALL_TEXTS
        .iter()
        .chain(crate::ui::pages::p5_audit::ALL_TEXTS.iter())
    {
        assert!(!t.is_empty(), "清册条目不得为空串");
    }

    // ⑤ **`audit_key(` 的计数自证**：`NON_DISPLAY_SINKS` 里的 `audit_key(` 是**后缀匹配** ——
    //    紧跟它的字面量会被**豁免**出"上屏候选"走查。该豁免在 P5 **正当**（审计 `target` 的
    //    **机器键**确不上屏：`gateway.port` 里的小写 ASCII 在生成字体里没有字形，上屏的是中文
    //    标签 —— 未登记键上屏的是 `display_safe` **归一后**的形态，**不是**原始小写键），
    //    但它同时是一条**可能被误用的后门**：谁把**上屏串**写成 `audit_key("…")`，那条串
    //    就**静默逃过**码表网。⇒ 把"恰好 6 处"钉死（= `TARGET_LABELS` 4 个配置字段键
    //    + `INTERLOCK_TARGETS` 2 个联锁键）。
    {
        let (name, src) = ("ui/pages/p5_audit.rs", include_str!("pages/p5_audit.rs"));
        let prod = truncate_before_test_module(src, name);
        assert_eq!(
            prod.matches("audit_key(\"").count(),
            6,
            "{name}：`audit_key(\"…\")` 必须**恰为 6 处**（`TARGET_LABELS` 的 4 个配置字段键 \
             + `INTERLOCK_TARGETS` 的 2 个联锁键 —— 契约点名：`interlock.release` / \
             `interlock.ack_m1`）。计数变化 = 有新的字面量被声明为「非屏显」——请逐条复核它\
             **确实是机器键**（键不上屏才可豁免；**上屏文案**放进 `audit_key(..)` 会从码表网里消失）"
        );
        assert_eq!(
            crate::ui::pages::p5_audit::TARGET_LABELS.len()
                + crate::ui::pages::p5_audit::INTERLOCK_TARGETS.len(),
            6,
            "两张映射表的条目数之和必须与上面的计数一致（三处一起改才自洽）"
        );
        assert_eq!(
            crate::ui::pages::p5_audit::TARGET_LABELS.len(),
            4,
            "配置字段键表仍是 4 条（联锁键不并进本表：标签转出契约 `label()`，见 AU9）"
        );
    }

    let sources: [(&str, &str); 2] = [
        ("ui/pages/p5_audit.rs", include_str!("pages/p5_audit.rs")),
        ("ui/pages/filters.rs", include_str!("pages/filters.rs")),
    ];
    for (name, src) in sources {
        let code = strip_comments_and_literals(src, name);
        let lower = code.to_ascii_lowercase();
        // ① 零文本输入（F12 红线）/ 裸色值 / `lv_refr_now` / 直连绑定（共用清单）。
        for needle in FORBIDDEN_UI_SYMBOLS {
            assert!(
                !lower.contains(needle),
                "{name} 不得出现 `{needle}`（设计 §11.1/§11.4 静态约束）"
            );
        }
        // ② 色值只准出现在 `theme.rs`（命名常量）。
        for needle in ["Color::hex(", "Color::rgb("] {
            assert!(
                !lower.contains(&needle.to_ascii_lowercase()),
                "{name} 不得出现 `{needle}`（必须经 theme 的命名常量）"
            );
        }
        // ③ `unsafe` 只准出现在 `src/lvgl/**`。
        assert!(
            !code.contains("unsafe"),
            "{name} 不得出现 `unsafe`（设计 §1.1.1.2 纪律 1）"
        );
        // ④ 控件策略：一律经 `crate::lvgl` 薄层与既有组合控件，**不得**直造原生 widget。
        for needle in ["lv_button", "lv_list", "lv_msgbox", "lv_obj_delete"] {
            assert!(
                !lower.contains(needle),
                "{name} 不得直造原生控件 `{needle}`（§5.3：控件策略 = 内置控件 + 主题，\
                 且 `ui/**` 只经薄层安全层）"
            );
        }
    }

    // ⑤′ **只读约束（P5 专属）**：页面**不得**构造任何写操作入口。
    {
        let (name, src) = ("ui/pages/p5_audit.rs", include_str!("pages/p5_audit.rs"));
        let code = strip_comments_and_literals(src, name);
        for needle in [
            "TextButton",      // 按钮（唯一可点控件的构造点在筛选区的组合控件内部）
            "ConfirmDialog",   // 确认弹层（写操作的前置）
            "Toast",           // 操作结果提示（写操作的反馈）
            "ButtonMatrix",    // 键矩阵（分段控件内部件 —— 本页不得直造）
            "Obj::delete",     // 删对象（唯一合法的删除是 `Drop` 级联）
        ] {
            assert!(
                !code.contains(needle),
                "{name} 不得出现 `{needle}` —— 本页是**只读页**（UI §6.5「只读约束」行 / PL-02：\
                 无编辑 / 删除 / 清空 / 导出入口）"
            );
        }
        // 自证扫描面真的覆盖到了 P5 审计页（`include_str!` 指错文件 / 文件被清空时，
        // 上面几条会**构造性全绿** —— "看着在把关、实则没把住"的典型形态）。
        for must in [
            "P5AuditPage",
            "set_page",
            "set_ops",
            "set_unavailable",
            "set_range_too_large",
            "set_on_query",
            "set_on_load_more",
            "AuditQuery",
            "MultiSelectChips",
            "EmptyState",
            "UnavailableState",
            "WarnBanner",
            "TARGET_LABELS",
            "INTERLOCK_TARGETS",
            "HEAD_CHILD_COUNT",
            "BannerSkin",
            "row_order",
            "free_text_safe",
            "list_clickable_count",
        ] {
            assert!(
                code.contains(must),
                "{name} 未包含 `{must}` —— 本用例的扫描面与预期不符（先修用例再谈实现）"
            );
        }
    }
    // ⑤″ **共享件**（`filters.rs`）的扫描面自证：P3 / P5 共用的接口必须**在位**
    //（删掉 `build` / `body_h` 会让 P3 复用无从下手；删掉 `set_on_change` 会让两页都收不到意图）。
    {
        let (name, src) = ("ui/pages/filters.rs", include_str!("pages/filters.rs"));
        let code = strip_comments_and_literals(src, name);
        for must in [
            "TimeRangeFilter",
            "TimeRangeChange",
            "pub fn build",
            "pub const fn body_h",
            "set_on_change",
            "datetime_to_epoch_ms",
            "RANGE_ORDER",
            "LogRange",
        ] {
            assert!(code.contains(must), "{name} 未包含 `{must}`（共享件接口缺失）");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥⁽⁵⁾⁗ **P3 日志页**的静态约束（`ui/pages/p3_logs.rs`，B2c-2）
//
// 既有的四条静态用例的扫描面都写死（**本批不改既有用例的扫描面**）⇒ 按前四条的先例
// **追加**独立用例。新文件的**裸尺寸**与**码表**两条网另由 [`UI_PROD_SOURCES`] /
// [`CONST_I32_SCAN_SOURCES`] 的扩展覆盖。
// ═══════════════════════════════════════════════════════════════════════════

/// P3 日志页的静态约束 + **扫描面自证** + **`module_key(` 计数自证** + **只读约束**。
///
/// **零文本输入红线**（UI §2.4 / §5.3）：删掉 / 绕过任一控件而改用 `lv_textarea` /
/// `lv_spinbox` / `lv_keyboard` ⇒ 本条变红（共用清单 [`FORBIDDEN_UI_SYMBOLS`]）。
///
/// **只读红线**（UI §6.3「不支持导出」节 / PRD T-2）：本页**不构造**确认弹层 / Toast /
/// 删改对象，也不出现"导出 / 删除 / 清空"类的**代码标识符**。
/// ⚠️ **本条的边界（如实）**：它扫的是**代码文本**（剥掉注释与字面量）；**运行期**的可点
/// 对象由 `pages_chain` 的 `rows_clickable_parts() == 0` 管，**上屏文案**里的"导出"说明由
/// `p3_logs.rs::tests` 管。三条合起来才是本页的只读网。
#[test]
fn p3_static_constraints() {
    // ⓪ **扫描面自证**：两张清单必须真的含本文件 —— 把文件从任一张网的清单里删掉，那张网会
    //    **静默失去覆盖**（"网看着还在、实则漏了一片"）。本条让它响亮失败。
    for (list, label) in [
        (UI_PROD_SOURCES.as_slice(), "UI_PROD_SOURCES"),
        (CONST_I32_SCAN_SOURCES.as_slice(), "CONST_I32_SCAN_SOURCES"),
    ] {
        assert!(
            list.iter().any(|(n, _)| *n == "ui/pages/p3_logs.rs"),
            "`{label}` 未含 `ui/pages/p3_logs.rs` —— 该网的**扫描面**漏了新文件\
             （先修清单：新文件必须纳入，否则静态网对新代码是空的）"
        );
    }
    // 码表覆盖率网还必须挂上本文件的 `ALL_TEXTS`（清册 ↔ 源码字面量一致性）。
    for t in crate::ui::pages::p3_logs::ALL_TEXTS {
        assert!(!t.is_empty(), "清册条目不得为空串");
    }

    // ⑤ **`module_key(` 的计数自证**：`NON_DISPLAY_SINKS` 里的 `module_key(` 是**后缀匹配** ——
    //    紧跟它的字面量会被**豁免**出"上屏候选"走查。该豁免在本页**正当**（日志模块的
    //    **机器名**确不上屏：`intercore` 里的小写 ASCII 在生成字体里没有字形，上屏的是中文名
    //    `核间` / `主站` / `审计`，未登记名经 `display_safe` 归一），但它同时是一条**可能被
    //    误用的后门**：谁把**上屏串**写成 `module_key("…")`，那条串就**静默逃过**码表网。
    //    ⇒ 把"恰好 3 处"钉死（= `MODULE_LABELS` 的三个机器名 token）。
    {
        let (name, src) = (
            "ui/pages/p3_logs.rs",
            include_str!("pages/p3_logs.rs"),
        );
        let prod = truncate_before_test_module(src, name);
        assert_eq!(
            prod.matches("module_key(\"").count(),
            5,
            "{name}：`module_key(\"…\")` 必须**恰为 5 处**（`MODULE_LABELS` 的五个机器名 token）。\
             计数变化 = 有新的字面量被声明为「非屏显」——请逐条复核它**确实是机器名**\
             （机器名不上屏才可豁免；**上屏文案**放进 `module_key(..)` 会从码表网里消失）"
        );
        assert_eq!(
            crate::ui::pages::p3_logs::MODULE_LABELS.len(),
            5,
            "映射表条目数必须与上面的计数一致（两处一起改才自洽）"
        );
    }

    let sources: [(&str, &str); 1] = [(
        "ui/pages/p3_logs.rs",
        include_str!("pages/p3_logs.rs"),
    )];
    for (name, src) in sources {
        let code = strip_comments_and_literals(src, name);
        let lower = code.to_ascii_lowercase();
        // ① 零文本输入（F12 红线）/ 裸色值 / `lv_refr_now` / 直连绑定（共用清单）。
        for needle in FORBIDDEN_UI_SYMBOLS {
            assert!(
                !lower.contains(needle),
                "{name} 不得出现 `{needle}`（设计 §11.1/§11.4 静态约束）"
            );
        }
        // ② 色值只准出现在 `theme.rs`（命名常量）。
        for needle in ["Color::hex(", "Color::rgb("] {
            assert!(
                !lower.contains(&needle.to_ascii_lowercase()),
                "{name} 不得出现 `{needle}`（必须经 theme 的命名常量）"
            );
        }
        // ③ `unsafe` 只准出现在 `src/lvgl/**`。
        assert!(
            !code.contains("unsafe"),
            "{name} 不得出现 `unsafe`（设计 §1.1.1.2 纪律 1）"
        );
        // ④ 控件策略：一律经 `crate::lvgl` 薄层与既有组合控件，**不得**直造原生 widget。
        for needle in ["lv_button", "lv_list", "lv_msgbox", "lv_obj_delete"] {
            assert!(
                !lower.contains(needle),
                "{name} 不得直造原生控件 `{needle}`（§5.3：控件策略 = 内置控件 + 主题）"
            );
        }
        // ⑤′ **只读约束（P3 专属）**：不得构造写操作 / 弹层 / 结果提示入口。
        //    ⚠️ `TextButton` **不在**禁用列（「回到最新」是**只读交互**、不是写操作 —— 见 R2），
        //    这正是本条与 `p5_static_constraints` 的唯一差别。
        for needle in [
            "ConfirmDialog",   // 确认弹层（写操作的前置）
            "Toast",           // 操作结果提示（写操作的反馈）
            "Obj::delete",     // 删对象（唯一合法的删除是 `Drop` 级联）
            "export",          // 导出入口的代码标识符（T-2：**不存在**任何导出入口）
            "delete_all",      // 清空入口
        ] {
            assert!(
                !code.contains(needle),
                "{name} 不得出现 `{needle}` —— 本页是**只读页**（UI §6.3「不支持导出」节 / \
                 PRD T-2：无导出 / 编辑 / 删除 / 清空入口）"
            );
        }
        // ⑥ **LG5 / LG14（B2c-2 规格评审整改 ③④）**：消息列必须是 **`DOTS`**（可见截断 +
        //    省略号），且**本文件生产区**的 `LongMode::WRAP` **恰 1 处**（只允许「回到最新」
        //    按钮的 `92×92` 折行）。
        //
        //    **旧写法（无区分度，已删）**：只断 `code.contains("LongMode::WRAP")` —— 该串
        //    被**同一文件里 `back.label()` 的 WRAP** 满足 ⇒ 把消息列改回 `WRAP` 时**仍然绿**
        //    （评审探针 ⑤ 实测）。现改为**两条一起**：
        //    ① **锚定消息列那一行**（`message.set_long_mode(LongMode::DOTS);` 必须逐字在）；
        //    ② **计数**——`LongMode::WRAP` 恰 1 处（把消息列改回 `WRAP` ⇒ 变 2 ⇒ 红）。
        //    ⚠️ **边界（LG14）**：薄层**无** `LongMode` 读回口（allowlist 只有 setter）⇒ 这是
        //    源码级锁；**行为级**的锁另见 `pages_chain` 的
        //    「长消息截断必须可见」段（读 LVGL 缓冲区里被换成 `...` 的文本）。
        assert!(
            code.contains("message.set_long_mode(LongMode::DOTS);"),
            "{name} 未含 `message.set_long_mode(LongMode::DOTS);` —— 消息列必须是 **DOTS**\
             （§6.3 行高恒 44 下 `WRAP` 会**静默裁掉第二行**、无标记，违 §2.6；见 LG5）"
        );
        assert!(
            !code.contains("message.set_long_mode(LongMode::WRAP);"),
            "{name} 仍把消息列设成 `WRAP` —— 静默裁切（无可见标记），见 LG5"
        );
        assert_eq!(
            code.matches("LongMode::WRAP").count(),
            1,
            "{name}：**代码里** `LongMode::WRAP` 必须**恰 1 处**（只有「回到最新」按钮的 92×92 \
             折行）。计数 > 1 ⇒ 多半有人把消息列改回了 `WRAP`（旧断言正是被它蒙混过关的）。\
             注：判据用**剥掉注释 / 字面量**后的 `code` —— 否则 LG14 里写的 token 串会把它数成 2"
        );
        // 自证扫描面真的覆盖到了 P3 日志页（`include_str!` 指错文件 / 文件被清空时，
        // 上面几条会**构造性全绿** —— "看着在把关、实则没把住"的典型形态）。
        for must in [
            "P3LogsPage",
            "set_page",
            "set_targets",
            "set_channel",
            "set_on_query",
            "set_on_increment",
            "request_increment",
            "set_on_back_to_latest",
            "LogQuery",
            "MODULE_LABELS",
            "free_text_safe",
            "MultiSelectChips",
            "EmptyState",
            "WarnBanner",
            "rows_clickable_parts",
            "WRITE_ENTRIES",
        ] {
            assert!(
                code.contains(must),
                "{name} 未包含 `{must}` —— 本用例的扫描面与预期不符（先修用例再谈实现）"
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥‴″ **P3 的运行时文本**：级别文字 / 模块名 / 消息 —— 逐字查 cmap
//
// 与 [`p5_runtime_texts_emit_only_cmap_glyphs`] 同口径：这些出口的输入来自契约 / 运行时
// （`LogEntry` 的 `target` / `message` / `level`），**不在**源码字面量走查面内 ⇒ 只有本用例
// 能管住它们（**R1** 的第二道网）。
// ═══════════════════════════════════════════════════════════════════════════

/// **P3 运行时文本的字符集检查**（C1 残留族：契约字符串直上屏）。
///
/// **改什么会让本条变红**：
/// - 把 [`crate::ui::pages::p3_logs::level_text`] 改成直接返回 `display_name()`（`Trace` 的
///   `T`(U+0054) 不在 cmap 内 ⇒ 豆腐块）；
/// - 把 [`crate::ui::pages::p3_logs::module_label`] 改成直接返回原始机器名（小写 ASCII 缺字形）；
/// - 把消息列改回原串（不经 `free_text_safe`）而消息里带全角标点。
#[test]
fn p3_runtime_texts_emit_only_cmap_glyphs() {
    use crate::ui::pages::p3_logs::{
        level_color, level_options, level_text, module_label, module_options, row_order,
        selected_levels, selected_targets,
    };
    use mupc_display_proto::{LogEntry, LogLevel};

    let Some(cmap) = load_font_cmap() else {
        return;
    };

    let mut cases: Vec<(String, String)> = Vec::new();
    // 级别文字（含 `Trace` 的归一形态）。
    for l in [
        LogLevel::Error,
        LogLevel::Warn,
        LogLevel::Info,
        LogLevel::Debug,
        LogLevel::Trace,
    ] {
        cases.push((format!("level_text({l:?})"), level_text(l)));
    }
    cases.push(("level_options()".into(), level_options().join(" ")));
    // 模块名：已知键（中文）/ 未登记键（归一形态）/ 契约示例的 `mupc_` 前缀形态。
    for t in [
        "mupc_intercore",
        "mupc_gateway",
        "intercore",
        "audit",
        "hplc",
        "meter_grid",
        "core-bin",
        "ota",
        "",
    ] {
        cases.push((format!("module_label({t})"), module_label(t)));
    }
    cases.push((
        "module_options(契约示例)".into(),
        module_options(&[
            "mupc_intercore".to_string(),
            "hplc".to_string(),
            "meter_grid".to_string(),
        ])
        .join(" "),
    ));
    // 自由文本（消息列）：全角标点 + 小写 ASCII + 中文。
    cases.push((
        "free_text_safe(消息)".into(),
        crate::ui::pages::p5_audit::free_text_safe("核间心跳超时，正在重连；模块=audit"),
    ));
    // 时间列（行的时间戳出口）。
    cases.push((
        "format_epoch_ms_utc".into(),
        crate::ui::pages::format_epoch_ms_utc(1_789_047_727_000),
    ));
    // 颜色是数值通道（无文本），但级别色的**可读性**由 `p3_logs` 的单测锁住。
    let _ = level_color(LogLevel::Trace);
    // 查询组装与行序不产文本，但它们的输入面（级别 / 模块）已在上方覆盖。
    let q = crate::ui::pages::p3_logs::log_query(
        &crate::ui::pages::filters::TimeRangeChange::default(),
        &selected_levels(&[0, 1, 2, 3]),
        &selected_targets(&[0, 1], &["hplc".to_string()]),
        None,
    );
    cases.push((
        "filters::range_text(query.range)".into(),
        crate::ui::pages::filters::range_text(q.range).to_string(),
    ));
    let es = vec![LogEntry {
        seq: 1,
        ts_ms: 0,
        level: LogLevel::Info,
        target: "gateway".into(),
        message: "x".into(),
    }];
    let _ = row_order(&es);

    for (what, text) in cases {
        for ch in text.chars() {
            if ch.is_whitespace() {
                continue;
            }
            assert!(
                cmap.contains(&ch),
                "**运行时**出口 `{what}` 产出的字符 U+{:04X} `{ch}` **不在生成字体的 cmap 内**\
                 （真机上是豆腐块）—— 产出文本 = `{text}`。\
                 级别文字必须经 `p3_logs::level_text`（display_safe）；模块名必须经 \
                 `p3_logs::module_label`（已知键取中文 / 未登记键走 display_safe）；\
                 消息必须经 `p5_audit::free_text_safe`；新增上屏文案前先查 \
                 `fonts/lv_font_cmap.txt`。",
                ch as u32
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥‴‴ **P3 的版式预算**：时间列 / 模块列 / 级别色块 / 模块 chip —— 用生产字体的 `adv_w` 实测
// ═══════════════════════════════════════════════════════════════════════════

/// **P3 的列宽预算**（用**生产字体的 `adv_w`** 实测；口径同 [`measured_text_px`]）。
///
/// **改什么会让本条变红**：
/// - 把 [`crate::ui::pages::p3_logs::ROW_TIME_W`] 改回 §6.3 的 160（时间戳 **224.0 px** 装不下
///   ⇒ DOTS 截断 —— 见 **LG2**）；
/// - 把 [`crate::ui::pages::p3_logs::ROW_MODULE_MAX_CHARS`] 调大（模块列被截断）；
/// - 把 [`crate::ui::pages::p3_logs::MODULE_CHIP_MAX_CHARS`] 调到 3（`✓` + 3 汉字 = 95.75 px
///   > chip 内区 76 px ⇒ 选中态折行 —— 见 **LG4**）；
/// - 把级别色块改窄到放不下 `DEBUG`（实测 86.9 px）。
#[test]
fn p3_column_budgets_fit_measured_text() {
    use crate::ui::pages::p3_logs::{
        clip_chip_label, clip_row_label, module_label, LEVEL_CHIP_W, MODULE_CHIP_MAX_CHARS,
        MODULE_CHIP_W, ROW_LEVEL_W, ROW_MODULE_MAX_CHARS, ROW_MODULE_TAIL_CHARS, ROW_MODULE_W,
        ROW_TIME_W,
    };

    // ① 时间列：定长时间戳必须**整条**放得下；且 §6.3 的 160 确实装不下（偏差 LG2 的证据）。
    //   ⚠️ **B2c-2 规格评审整改 ⑥**：原稿写"236.4 px"，与入库基线 `lv_font_metrics.txt` 复算的
    //   **224.0 px** 不符（差 12.4 px）。本用例用的是 `measured_text_px`（同一份基线的同一口径）
    //   ⇒ 下面的断言值就是**订正后**的 224.0。
    let ts = "2026/09/10 13:42:07";
    let ts_w = measured_text_px(ts, 24);
    assert_eq!(ts_w, 224, "订正后的实测值（LG2；原稿 236.4 是不实数字）");
    assert!(
        ts_w <= ROW_TIME_W,
        "时间戳 `{ts}` 实测 {ts_w} px > 时间列宽 {ROW_TIME_W} px"
    );
    assert!(
        ts_w > 160,
        "时间戳实测 {ts_w} px —— 若 ≤ 160 则 LG2 的列宽偏差**不再成立**，请复核 §6.3 的 160"
    );
    // 160 px 只够 `HH:MM:SS`（§6.3 未定义时间格式，这是"160 是规格缺口"的证据）。
    let hms_w = measured_text_px("13:42:07", 24);
    assert_eq!(hms_w, 93, "`HH:MM:SS` 实测 93.25 px（订正后口径；LG2）");
    assert!(hms_w <= 160, "`HH:MM:SS` 装得进 §6.3 的 160");

    // ② 模块列：`ROW_MODULE_MAX_CHARS` 个汉字**原样**放得下（未超出时的上界）。
    let m_w = measured_text_px(&"汉".repeat(ROW_MODULE_MAX_CHARS), 24);
    assert!(
        m_w <= ROW_MODULE_W,
        "{ROW_MODULE_MAX_CHARS} 个汉字实测 {m_w} px > 模块列宽 {ROW_MODULE_W} px"
    );
    // ②′ **截断产物**（`...` + 尾 `ROW_MODULE_TAIL_CHARS` 字 = 4 汉字）也放得下；再多一个字就破线。
    let row_tail = format!("...{}", "汉".repeat(ROW_MODULE_TAIL_CHARS));
    let row_tail_w = measured_text_px(&row_tail, 24);
    assert!(
        row_tail_w <= ROW_MODULE_W,
        "行截断产物 `{row_tail}` 实测 {row_tail_w} px > 模块列宽 {ROW_MODULE_W} px（LG4）"
    );
    // ⚠️ 边界：`...` + 5 汉字 = **140.0625 px**（真值，按 `adv_w` 的 1/16 px 逐字求和），
    //   而 `measured_text_px` 截断到整数后报 **140** = `ROW_MODULE_W`（恰好触边）。
    //   ⇒ 判据写 `>=`（"到边 / 越界"），并在此写明真值 > 140 的事实。
    let row_over = format!("...{}", "汉".repeat(ROW_MODULE_TAIL_CHARS + 1));
    let row_over_w = measured_text_px(&row_over, 24);
    assert!(
        row_over_w >= ROW_MODULE_W,
        "尾保留再多一字（`{row_over}`）实测 {row_over_w} px —— 若 < {ROW_MODULE_W} px 则可调大\
         ROW_MODULE_TAIL_CHARS（当前偏保守）"
    );
    assert_eq!(
        row_tail_w + 24,
        row_over_w,
        "每多一个汉字恰 +24 px（`adv_w` 口径自证：乘积取整未吞掉真实差）"
    );

    // ③ 模块 chip：**两条都要过** —— ①原样预算（选中态）②截断产物（选中态）。
    //   ⚠️ 选中前缀是 `chip_text()` 的 `"✓" + label`（**无空格**，`components.rs`），
    //   预算按**真内区** `MODULE_CHIP_W − 2×GAP_MIN − 2×描边(1 px)` 算。
    let chip_inner = MODULE_CHIP_W - 2 * Dimens::GAP_MIN - 2 * Stroke::THIN;
    let sel_w = measured_text_px(&format!("✓{}", "汉".repeat(MODULE_CHIP_MAX_CHARS)), 26);
    assert!(
        sel_w <= chip_inner,
        "选中态 `✓ + {MODULE_CHIP_MAX_CHARS} 汉字` 实测 {sel_w} px > chip 内区 {chip_inner} px \
         （LG4 的预算被突破 ⇒ 选中态 chip 会折行 / 裁切）"
    );
    let over_w = measured_text_px(&format!("✓{}", "汉".repeat(MODULE_CHIP_MAX_CHARS + 1)), 26);
    assert!(
        over_w > chip_inner,
        "多一字（{} 汉字）实测 {over_w} px —— 若它也 ≤ {chip_inner} px，则\
         MODULE_CHIP_MAX_CHARS 可以调大（当前预算偏保守）",
        MODULE_CHIP_MAX_CHARS + 1
    );
    // ③′ 截断产物（`...` + 尾 1 字）必须放得下 —— 且**"2 字 + 标记"必须放不下**（这才是
    //    "chip 上放不下 2 字 + 可见标记"这条取舍（LG4）的**实测证据**）。
    let chip_tail = format!("✓{}", clip_chip_label(&"汉".repeat(4)));
    let chip_tail_w = measured_text_px(&chip_tail, 26);
    assert!(
        chip_tail_w <= chip_inner,
        "chip 截断产物 `{chip_tail}` 实测 {chip_tail_w} px > chip 内区 {chip_inner} px（LG4）"
    );
    let two_plus_marker = measured_text_px(&format!("✓{}汉汉", "..."), 26);
    assert!(
        two_plus_marker > chip_inner,
        "「2 汉字 + `...`」实测 {two_plus_marker} px —— 若 ≤ {chip_inner} px，则 chip 上\
         **放得下**「2 字 + 标记」，LG4 的取舍（标记优先于多留 1 字）应改为「标记 + 2 字」"
    );
    // ③″ 截断策略自证：产物**含可见标记**、且两个"尾字不同"的长名**可区分**（LG12 的补偿）。
    assert!(clip_chip_label(&module_label("mupc_gateway::iec104")).starts_with("..."));
    assert_ne!(
        clip_chip_label(&module_label("mupc_gateway::iec104")),
        clip_chip_label(&module_label("mupc_gateway::rs485")),
        "尾字不同的两个长名 ⇒ chip 产物必须可区分（LG12）"
    );
    assert_ne!(
        clip_row_label(&module_label("mupc_gateway::iec104")),
        clip_row_label(&module_label("mupc_gateway::rs485")),
        "尾字不同的两个长名 ⇒ 行产物必须可区分（LG12）"
    );
    assert!(
        std::hint::black_box(MODULE_CHIP_W) >= std::hint::black_box(Dimens::CHIP_MIN_W),
        "chip 不得低于最小宽 96"
    );

    // ④ 级别色块：最长的级别文字（`DEBUG`）放得下。
    let lv_w = measured_text_px("DEBUG", 24);
    assert!(
        lv_w <= ROW_LEVEL_W,
        "`DEBUG` 实测 {lv_w} px > 级别色块宽 {ROW_LEVEL_W} px"
    );
    // ⑤ 级别 chip：`✓ ` + 最长选项（选中态）放得进 chip 内区。
    let lvl_sel = measured_text_px("✓ DEBUG", 26);
    let lvl_inner = LEVEL_CHIP_W - 2 * Dimens::GAP_MIN;
    assert!(
        lvl_sel <= lvl_inner,
        "选中态 `✓ DEBUG` 实测 {lvl_sel} px > 级别 chip 内区 {lvl_inner} px"
    );
    // ⑥ 「回到最新」按钮：`回到最新` 在 WRAP 下折成 2 行 2 字（按钮 92×92）。
    let word_w = measured_text_px("回到", 26);
    let two_line_h = 2 * TextSlot::Label.px() as i32;
    let back_inner_w = 92 - 2 * Dimens::GAP_MIN;
    let back_inner_h = 92 - 2 * Dimens::GAP_MIN;
    assert!(
        word_w <= back_inner_w && two_line_h <= back_inner_h,
        "「回到最新」两行折行实测 {word_w}×{two_line_h} px 放不进 92×92 按钮的内区 \
         {back_inner_w}×{back_inner_h} px"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥‴′ **P5 的运行时文本**：值对 / 原因 / 操作者 / 最近审计条 —— 逐字查 cmap
//
// 与 [`runtime_formatters_emit_only_cmap_glyphs`] 同口径，但对象是 **P5 新增的运行时出口**
// （它们的主要输入来自契约 / 运行时，**不在**源码字面量走查面内 ⇒ 只有本用例能管住它们）。
// ═══════════════════════════════════════════════════════════════════════════

/// **P5 运行时文本的字符集检查**（C1 残留族：契约字符串直上屏）。
///
/// **改什么会让本条变红**：
/// - 把 `p5_audit::free_text_safe` 改回纯 `display_safe`（全角冒号不再折叠）；
/// - 把 `value_text` 的 `None` 分支改成 `"0"`（`0` 在 cmap 内 ⇒ **不会**变红：那条由
///   `none_is_placeholder_never_zero` 管，本条只管字形）；
/// - 把 `TARGET_LABELS` 的某个中文标签改成含缺字的词（如 `水浸`）。
#[test]
fn p5_runtime_texts_emit_only_cmap_glyphs() {
    use crate::ui::pages::p5_audit::{
        free_text_safe, newest_text, operator_text, reason_text, result_text, side_text,
        summary_text, value_text, AuditQuery, SUMMARY_MAX_CHARS,
    };
    use mupc_display_proto::{AuditResult, ConsoleOp, CONSOLE_OPERATOR};

    let Some(cmap) = load_font_cmap() else {
        return;
    };

    let mut cases: Vec<(String, String)> = Vec::new();
    // 值文本：覆盖每种 JSON 形态（含负值 / 复合 / 超长）。
    for v in [
        serde_json::json!(null),
        serde_json::json!(true),
        serde_json::json!(false),
        serde_json::json!(2404),
        serde_json::json!(-1.5),
        serde_json::json!("debug"),
        serde_json::json!("127.0.0.1"),
        serde_json::json!([1, 2, 3]),
        serde_json::json!({"a": 1, "b": 2}),
    ] {
        cases.push((format!("value_text({v})"), value_text(&v)));
    }
    cases.push(("side_text(None)".into(), side_text(None)));
    // 值对：已知键 / 未登记键（降级显示机器键 ⇒ 也须逐字过 cmap）/ 两侧缺 / 超长。
    for (b, a, t) in [
        (
            Some(serde_json::json!(2404)),
            Some(serde_json::json!(2405)),
            "gateway.port",
        ),
        (None, None, "gateway.port"),
        (
            Some(serde_json::json!(true)),
            None,
            "interlock.release",
        ),
        (
            Some(serde_json::json!(true)),
            None,
            "interlock.ack_m1",
        ),
        // **未登记键**（评审 ③ 整改）：`display_safe(键)` 的产物必须逐字在 cmap 内。
        (
            Some(serde_json::json!(1)),
            Some(serde_json::json!(2)),
            "interlock.flood",
        ),
        (
            None,
            None,
            "display.publish_ms",
        ),
        (
            Some(serde_json::json!("a".repeat(80))),
            Some(serde_json::json!([1])),
            "system.log_level",
        ),
    ] {
        cases.push((
            format!("summary_text({b:?}, {a:?}, {t})"),
            summary_text(b.as_ref(), a.as_ref(), t),
        ));
    }
    // 原因 / 操作者 / 最近审计条 / 结果胶囊。
    for r in [None, Some("触发源未复位：estop、door"), Some("  ")] {
        cases.push((
            format!("reason_text({r:?})"),
            reason_text(AuditResult::Failed, r).unwrap_or_default(),
        ));
    }
    cases.push((
        "reason_text(ok, Some(..))".into(),
        reason_text(AuditResult::Ok, Some("不该出现")).unwrap_or_default(),
    ));
    for o in [CONSOLE_OPERATOR, "hmi", "本地控制台"] {
        cases.push((format!("operator_text({o})"), operator_text(o)));
    }
    for ms in [None, Some(0), Some(1_789_047_727_999)] {
        cases.push((format!("newest_text({ms:?})"), newest_text(ms)));
    }
    for r in [AuditResult::Ok, AuditResult::Failed] {
        cases.push((format!("result_text({r:?})"), result_text(r).to_string()));
    }
    // 全角标点折叠（**AU15**）：六个源字符逐一进输入。
    cases.push((
        "free_text_safe(全角标点)".into(),
        free_text_safe("a，b；c（d）e：f、g"),
    ));
    // 截断上限处的产物（最长可能文本）。
    cases.push((
        format!("summary_text(超长, SUMMARY_MAX_CHARS={SUMMARY_MAX_CHARS})"),
        summary_text(
            Some(&serde_json::json!("x".repeat(50))),
            Some(&serde_json::json!("y".repeat(50))),
            "system.log_level",
        ),
    ));
    // 查询意图本身不含文本，但其 `Default` 的档位文案会经 filters 上屏 ⇒ 一并查。
    let q = AuditQuery::default();
    cases.push((
        "query_default.range".into(),
        crate::ui::pages::filters::range_text(q.range).to_string(),
    ));
    cases.push((
        "ConsoleOp::label()".into(),
        ConsoleOp::ALL
            .iter()
            .map(|o| o.label())
            .collect::<Vec<_>>()
            .join(" "),
    ));

    for (what, text) in cases {
        for ch in text.chars() {
            if ch.is_whitespace() {
                continue;
            }
            assert!(
                cmap.contains(&ch),
                "**运行时**出口 `{what}` 产出的字符 U+{:04X} `{ch}` **不在生成字体的 cmap 内**\
                 （真机上是豆腐块）—— 产出文本 = `{text}`。\
                 自由文本必须经 `p5_audit::free_text_safe`（`display_safe` + 全角标点折叠）；\
                 数值必须经 `ui/pages` 的唯一出口；新增上屏文案前先查 `fonts/lv_font_cmap.txt`。",
                ch as u32
            );
        }
    }
}

/// **值对截断上限的几何依据**：`SUMMARY_MAX_CHARS` 个汉字与 §6.5 的示例都必须放得进
/// [`crate::ui::pages::p5_audit::SUMMARY_MAX_CHARS`] 对应的列宽（用**生产字体的 `adv_w`**
/// 实测，口径同 [`measured_text_px`]）。
///
/// **改什么会让本条变红**：把 `SUMMARY_MAX_CHARS` 调大（超过列宽）⇒ 上屏即被 `LongMode::DOTS`
/// 截成省略号，"截断上限"名存实亡；把 `ROW_SUMMARY_W` 调小到装不下契约示例 ⇒ 第二条红。
#[test]
fn p5_summary_limit_fits_its_column() {
    use crate::ui::pages::p5_audit::{ROW_SUMMARY_W, SUMMARY_MAX_CHARS};
    // 无 `.c` 的干净 clone / CI ⇒ 走**入库基线** `fonts/lv_font_metrics.txt`；两者皆缺 ⇒
    // **响亮失败**（不再静默跳过 —— 否则本网在 CI 上恒空转，见 B2c-1 代码质量评审 ⑤）。
    let cjk_w = measured_text_px(&"汉".repeat(SUMMARY_MAX_CHARS), 24);
    assert!(
        cjk_w <= ROW_SUMMARY_W,
        "截断上限 {SUMMARY_MAX_CHARS} 个汉字实测 {cjk_w} px > 值对列宽 {ROW_SUMMARY_W} px \
         —— 上屏会被 DOTS 截断，上限形同虚设"
    );
    // §6.5 的行内容示例必须**整条**放得下（它是本页唯一的文案范本，且不得被截断）。
    let example = "端口: 2404 → 2405";
    let ex_w = measured_text_px(example, 24);
    assert!(
        ex_w <= ROW_SUMMARY_W,
        "§6.5 示例 `{example}` 实测 {ex_w} px > 值对列宽 {ROW_SUMMARY_W} px"
    );
    assert!(
        example.chars().count() <= SUMMARY_MAX_CHARS,
        "示例长度 {} 字必须 ≤ 截断上限 {SUMMARY_MAX_CHARS}（否则示例本身会被截断）",
        example.chars().count()
    );
}

/// **入库宽度基线本身不是摆设**（B2c-1 代码质量评审 ⑤ 的配套网）。
///
/// 宽度类断言在**没有 `.c`** 的干净 clone / CI 上完全依赖 `fonts/lv_font_metrics.txt`
/// ⇒ 若该文件缺失、错位或退化成全 0，"宽度网"会以**另一种方式**空转（值全为 0 ⇒ 所有
/// `w <= 列宽` 恒真）。本用例把那三种情形都钉死。
///
/// **改什么会让本条变红**：删掉基线文件（第 1 条）；把某档的值改成 0 / 删掉几个值
/// （第 2 / 3 条）；把两档的值写成同一份（第 4 条 —— 字号不同、adv_w 必然不同）。
#[test]
fn font_metrics_baseline_is_present_and_meaningful() {
    let fonts_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts");
    let src = std::fs::read_to_string(fonts_dir.join(METRICS_MANIFEST)).unwrap_or_else(|e| {
        panic!(
            "读不到入库宽度基线 `{}`（{e}）—— 它是**入库项**（与 `{}` 同一次生成），\
             干净 clone / CI 的宽度断言全靠它；请跑 `fonts/gen_fonts.sh` 恢复",
            fonts_dir.join(METRICS_MANIFEST).display(),
            CMAP_MANIFEST
        )
    });
    let cps = std::fs::read_to_string(fonts_dir.join(CMAP_MANIFEST))
        .map(|s| parse_cmap_manifest(&s, CMAP_MANIFEST))
        .expect("读入库码表清单");
    // ① 档位清单 = **基线头自证**（[`metrics_tiers`]，单一真源；**不再**在用例里写第二份
    //    10 档清单 —— B2c-1 收口 ⑥）。自证行缺失 / 空表 / 非数字 / 重复 / 与档位数自证不符
    //    ⇒ `metrics_tiers` 内部当场 `panic`（**响亮失败**）。
    let tiers = metrics_tiers(&src);
    // ①′ 交叉核对 = **代码侧的字体阶梯**（`FontSize::ALL`，即真正编译进来的 10 档）：
    //     这才是"基线是否覆盖了全部字号"的判据（比硬编码 10 个数字强 —— 扩档位时只改
    //     `fonts/gen_fonts.sh` 的 `SIZES` 而忘重跑 ⇒ 这里**红**）。
    let ladder: std::collections::BTreeSet<u32> =
        FontSize::ALL.iter().map(|f| f.px()).collect();
    assert_eq!(
        tiers.iter().copied().collect::<std::collections::BTreeSet<u32>>(),
        ladder,
        "基线档位自证表必须与字体阶梯 `FontSize::ALL` **逐个相等**（{METRICS_MANIFEST}）"
    );
    assert!(
        tiers.windows(2).all(|w| w[0] < w[1]),
        "基线档位自证表必须是**升序去重**形态（gen_fonts.sh 按 `sorted(sizes)` 写出）：{tiers:?}"
    );
    // ② 每档都必须能取到，且**每个值都 > 0**（0 = "没有这个字形"，会让宽度断言恒真）。
    for px in tiers {
        let map = adv_w_from_baseline(px)
            .unwrap_or_else(|| panic!("入库基线里缺 `{px}` 档（{METRICS_MANIFEST}）"));
        assert_eq!(
            map.len(),
            cps.len(),
            "`{px}` 档的值数必须等于 {CMAP_MANIFEST} 的码位数"
        );
        assert!(
            map.values().all(|v| *v > 0),
            "`{px}` 档存在 0 值的 adv_w ⇒ 该字形的宽度断言会恒真（基线退化）"
        );
        // ②′ 抽查"汉字必须比 ASCII 宽"（`时` U+65F6 在码表内；`汉` **不在** —— 它只在用例里
        //    当"典型汉字宽度"的探针，缺字时走整字宽兜底，不得拿来查基线）。
        let uniq: std::collections::BTreeSet<u64> = map.values().copied().collect();
        assert!(uniq.len() > 3, "`{px}` 档的 adv_w 几乎全同 ⇒ 基线不像真实字形表");
        let cjk = map[&('时' as u32)];
        let ascii = map[&('1' as u32)];
        assert!(cjk > ascii, "汉字 `时` 的步进宽必须大于数字 `1`（{cjk} vs {ascii}）");
    }
    // ③ 不同档的字号**必须**给出不同的宽度（否则说明值是按档复制的）。
    let a = adv_w_from_baseline(24).expect("24 档");
    let b = adv_w_from_baseline(48).expect("48 档");
    assert_ne!(a, b, "24 档与 48 档的 adv_w 表不得相同（字号不同 ⇒ 步进宽不同）");
    assert!(
        b[&('时' as u32)] > a[&('时' as u32)],
        "48 档汉字步进宽必须大于 24 档"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥⁗ **常量定义式**静态约束（I2：`const` 定义式里的裸数字缺口）
//
// ⑥″（[`ui_layout_setters_use_theme_constants`]）只扫**调用实参**（`set_size(..)` /
// `decor(..)`），**扫不到** `const` **定义式**里的裸数字 —— B2b-1 代码质量评审探针 **P7**
// 实测：把 `pub const SEGMENT_H: i32 = Dimens::CHIP_H;` 改成 `pub const SEGMENT_H: i32 = 48;`，
// ⑥″ 与全部 171 条用例**全绿**（代数恒真式 + 无定义式扫描 ⇒ 这个缺口当时没有任何网）。
// 本用例补上这条网。
// ═══════════════════════════════════════════════════════════════════════════

/// **常量定义式**扫描面（**刻意不含 `ui/theme.rs`** —— 它是这些数字的**根真源**，设计栅格值
/// 本来就该在那里以字面量出现；本网要抓的是"**派生**常量直接抄数字"）。
///
/// 与 ⑥″ 各自列清单而不复用 [`UI_PROD_SOURCES`]：后者含 `theme.rs`（必须豁免）。
const CONST_I32_SCAN_SOURCES: [(&str, &str); 12] = [
    ("ui/mod.rs", include_str!("mod.rs")),
    ("ui/components.rs", include_str!("components.rs")),
    ("ui/controls.rs", include_str!("controls.rs")),
    ("ui/shell.rs", include_str!("shell.rs")),
    ("ui/pages/mod.rs", include_str!("pages/mod.rs")),
    ("ui/pages/filters.rs", include_str!("pages/filters.rs")),
    ("ui/pages/p1_status.rs", include_str!("pages/p1_status.rs")),
    ("ui/pages/p2_config.rs", include_str!("pages/p2_config.rs")),
    ("ui/pages/p3_logs.rs", include_str!("pages/p3_logs.rs")),
    ("ui/pages/p4_interlock.rs", include_str!("pages/p4_interlock.rs")),
    ("ui/pages/p5_audit.rs", include_str!("pages/p5_audit.rs")),
    ("ui/pages/p6_system.rs", include_str!("pages/p6_system.rs")),
];

/// **允许的裸整数常量值：只有 `0`**（逐条说明理由，不靠"看着没风险"）。
///
/// `0` 是"**无偏移 / 顶行零点**"的单位元，**不是**设计栅格值 —— 本批唯一命中项是
/// `controls.rs` 的 `DATETIME_HEADER_Y = 0`（列头行贴容器顶部）。与 ⑥″ 的
/// [`ALLOWED_BARE_GEOMETRY_LITERAL`]（`set_pos(0, y)` 的 `0`）**同口径**。
///
/// **为什么放行不构成风险**：把某条"真尺寸"误写成 `0`（`const W: i32 = 0;` 当宽用）逃不过
/// 别的网 —— 离屏 `size()` 断言会立刻变红；而"该写 `0` 却写了非 `0`"由本用例的
/// **非零即违规**规则直接拦下。**宁可先严格**：将来若确需放行别的值，必须在此逐条加并写明理由。
const ALLOWED_BARE_CONST_I32: [&str; 1] = ["0"];

/// **已登记放行的裸整数常量**（`(文件, 常量名, 理由)`）——**逐条登记，不作正则豁免**。
///
/// 唯一一条：`ui/pages/p1_status.rs` 的 `MAIN_CARD_H = 320` —— 它是 UI §6.1 的**契约给定值**
/// （主行卡 `484×320`），B2a 代码质量评审 **M5** 已在 `p1_status.rs` 模块文档里登记为
/// "**唯一的例外**"；该文件**本批禁改** ⇒ 此处只登记、不动手。
///
/// **登记不得腐化**：本用例断言"每条登记**确实仍命中**一个违规点"；若将来该常量迁入 `theme`
/// 或改为派生式，本用例会**变红**并提示删除该条（与 `KNOWN_MISSING` 的"防登记腐化"同法）。
const REGISTERED_BARE_CONST_I32: [(&str, &str, &str); 1] = [(
    "ui/pages/p1_status.rs",
    "MAIN_CARD_H",
    "UI §6.1 契约值 484×320（B2a 代码质量评审 M5 已登记的唯一例外；本批禁改 pages/**）",
)];

// ── ⚠️ **已知边界**（评审实测的 11 种绕过形态）：**这是边界，不是覆盖** ──────────────
//
// 本网（⑥⁗）只认"**单行** + **`const`** + **类型恰为 `i32`** + **初始化式就是一个十进制裸
// 整数**"这一形态。下列 11 种写法**当前都查不到**（逐条是评审实测、不是推测）：
//
// | # | 形态 | 例 | 为何漏 |
// |---|------|----|--------|
// | 1 | 初始化式**跨行** | `const W: i32 =\n    48;` | [`bare_i32_const`] 逐行解析，跨行 ⇒ 两边都不成形 |
// | 2 | 括号包裹 | `const W: i32 = (48);` | 初始化式非"纯十进制"（含 `(` `)`） |
// | 3 | 加零 / 算式 | `const W: i32 = 48 + 0;` | 含算术 ⇒ 与派生式同形，放行（否则大面积误报） |
// | 4 | 十六进制 | `const W: i32 = 0x30;` | 只认十进制数字 |
// | 5 | 带后缀 | `const W: i32 = 48i32;` | 初始化式含后缀字符 |
// | 6 | 前导零（八进制字面量） | `const W: i32 = 048;` | 数字串通过，**但值**是 40（八进制）⇒ 见下"为何不扩" |
// | 7 | `static` 而非 `const` | `static W: i32 = 48;` | 只认 `const ` 前缀 |
// | 8 | `pub(in …)` 可见性 | `pub(in crate::ui) const W: i32 = 48;` | 只剥 `pub` / `pub(crate)` / `pub(super)` |
// | 9 | 类型 `u32` | `const W: u32 = 48;` | 只认 `i32` |
// | 10 | 类型 `usize` | `const W: usize = 48;` | 同上 |
// | 11 | 类型 `f64` | `const W: f64 = 48.0;` | 同上 |
//
// **为何当前不扩（如实，不粉饰）**：本网是**减速带**（见用例文档的"定位"），不是形式化保证；
// 上面 11 条里 1–5、7–11 都是"**把裸值写得更绕一点**"——扩它们要把判据变成一个小型表达式
// 解析器（`KISS` 原则：不为一条减速带造解析器），而**收益很低**：真正的兜底另有两处 ——
// `theme.rs` 的单一真源 + [`sizes_are_derived_from_theme_constants`]（`controls.rs` 内）的
// **契约字面量锚定**，以及 `ui_layout_setters_use_theme_constants`（⑥″，扫**调用实参**）与
// 离屏 `size()` 断言。第 6 条（`048`）更特殊：它**是**十进制数字串、会被本网抓到，但
// 抓到时若有人"按字面理解"改成 `48` 反而是**改错方向**（`048` 的真值是 40）—— 这属于
// 另一类问题（可读性），不靠扩本网解决。
//
// **本表的存在意义**：把"网没覆盖什么"写在网旁边，避免下一位读者把"用例全绿"误读成
// "裸整数已绝迹"。**发现新形态请补进本表**，不要默默放行。

/// 从**单行**源码解析 `const <NAME>: i32 = <整数>;`（整条初始化式就是一个裸十进制整数）。
///
/// 返回 `(常量名, 字面量文本)`；不符即 `None`。**刻意只认 `i32` 且只认单行**：
/// - 类型非 `i32`（`usize` / `i64` / `u32`…）**不查** —— 本项目里它们装的是**语义量**
///   （`IPV4_OCTETS = 4` / `STEP = 1`），不是设计栅格值；
/// - 初始化式跨行 / 含 `Dimens::` / 含算术 / 含 `as` ⇒ 自然不是裸整数，不命中。
///
/// 手写而不引正则：本 crate 无 `regex` 依赖，且判据够简单（不为它造一套解析器）。
fn bare_i32_const(line: &str) -> Option<(&str, &str)> {
    let l = line.trim();
    // 可见性前缀（`pub` / `pub(crate)` / `pub(super)`）整条剥掉。
    let l = l
        .strip_prefix("pub(crate)")
        .or_else(|| l.strip_prefix("pub(super)"))
        .or_else(|| l.strip_prefix("pub"))
        .unwrap_or(l)
        .trim_start();
    let rest = l.strip_prefix("const ")?;
    let (name, rest) = rest.split_once(':')?;
    let name = name.trim();
    let rest = rest.trim_start().strip_prefix("i32")?.trim_start();
    let rest = rest.strip_prefix('=')?.trim();
    let init = rest.strip_suffix(';')?.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let digits = init.strip_prefix('-').unwrap_or(init);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((name, init))
}

/// ⑥⁗ **常量定义式**：`ui/**` 生产源码里 `const <NAME>: i32 = <整数>;` 一律违规
/// （例外 = [`ALLOWED_BARE_CONST_I32`] + [`REGISTERED_BARE_CONST_I32`]，见各自文档）。
///
/// **定位（如实，与 ⑥″ 同级）**：这是一条**减速带** —— 拦"定义式里直接写死一个数字"，
/// 不追求形式化保证（数值仍可经 `Dimens` 间接传递；那由 `theme.rs` 的单一真源 +
/// `controls.rs::tests::sizes_are_derived_from_theme_constants` 的**字面量锚定**共同负责）。
#[test]
fn ui_const_i32_definitions_derive_from_theme() {
    // ① 探测器**正 / 负对照**（防"解析器写坏 ⇒ 恒返回 None ⇒ 构造性全绿"，这正是本项目
    //    两轮评审反复点名的"看着在把关、实则没把住"）。
    assert_eq!(
        bare_i32_const("pub const SEGMENT_H: i32 = 48;"),
        Some(("SEGMENT_H", "48")),
        "正对照：裸整数定义式必须被抓到（P7 的变异形态）"
    );
    assert_eq!(
        bare_i32_const("const DATETIME_HEADER_Y: i32 = 0;"),
        Some(("DATETIME_HEADER_Y", "0")),
        "正对照：`0` 也先被抓到，再由 ALLOWED_BARE_CONST_I32 放行"
    );
    assert_eq!(
        bare_i32_const("pub const SEGMENT_H: i32 = Dimens::CHIP_H;"),
        None,
        "负对照：派生式不得误报"
    );
    assert_eq!(
        bare_i32_const(
            "pub const IPV4_SEG_W: i32 = Dimens::STEPPER_BTN_W * 2 + Dimens::IPV4_VALUE_W;"
        ),
        None,
        "负对照：算术式不得误报"
    );
    assert_eq!(
        bare_i32_const("const IPV4_OCTETS: usize = 4;"),
        None,
        "负对照：非 i32 不查（语义量，不是栅格值）"
    );
    assert_eq!(bare_i32_const("const STEP: i64 = 1;"), None, "负对照：非 i32 不查");

    // ② **扫描面自证**：清单必须真的含 `controls.rs`（若 `include_str!` 指错文件 / 文件被
    //    清空，下面的逐行循环会构造性全绿）。
    assert!(
        CONST_I32_SCAN_SOURCES.iter().any(|(n, _)| *n == "ui/controls.rs"),
        "扫描面必须覆盖 ui/controls.rs（否则本用例对 B2b-1 无意义）"
    );
    let controls_src = CONST_I32_SCAN_SOURCES
        .iter()
        .find(|(n, _)| *n == "ui/controls.rs")
        .map(|(_, s)| *s)
        .unwrap_or("");
    for must in ["SEGMENT_H", "DATETIME_VALUE_W", "IPV4_TOTAL_W"] {
        assert!(
            controls_src.contains(must),
            "ui/controls.rs 的扫描面未含 `{must}` —— include_str! 指错文件 / 内容不符预期"
        );
    }

    // ③ 逐行扫描（先剥注释与字符串 / 字符字面量 ⇒ 文档里解释性的 `const .. = 48` 不误伤）。
    let mut registered_hit = [false; REGISTERED_BARE_CONST_I32.len()];
    for (name, src) in CONST_I32_SCAN_SOURCES {
        let code = strip_comments_and_literals(src, name);
        for (idx, line) in code.lines().enumerate() {
            let Some((cname, value)) = bare_i32_const(line) else {
                continue;
            };
            if ALLOWED_BARE_CONST_I32.contains(&value) {
                continue;
            }
            if let Some(pos) = REGISTERED_BARE_CONST_I32
                .iter()
                .position(|(f, n, _)| *f == name && *n == cname)
            {
                registered_hit[pos] = true;
                continue;
            }
            panic!(
                "{name}:{} —— `const {cname}: i32 = {value};` 的初始化式是**裸整数**\
                 （设计 §11.4 ④：屏上尺寸一律经 `theme` 常量推导 / 由 theme 分项算出）。\n\
                 行内容：{}\n\
                 若这是「零点 / 单位元」，请加入 `ALLOWED_BARE_CONST_I32` 并写明理由；\
                 若是文档给定的**契约值**，请登记 `REGISTERED_BARE_CONST_I32`（附出处）；\
                 否则请改为由 `theme` 常量推导。",
                idx + 1,
                line.trim()
            );
        }
    }

    // ④ 登记不得腐化（放在**全部扫描之后** —— 登记条目所在文件在清单里靠后）：
    for (hit, (f, n, why)) in registered_hit.iter().zip(REGISTERED_BARE_CONST_I32.iter()) {
        assert!(
            *hit,
            "登记条目 `{f}` / `{n}`（{why}）已**失效** —— 源码里已不再有该裸整数定义式，\
             请从 `REGISTERED_BARE_CONST_I32` 删除它"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ⑥″ **裸尺寸**静态约束（设计 §11.4 ④：`ui/**` 不得出现「字面量尺寸」）
//
// 此前两条静态用例（[`ui_static_constraints`] / [`pages_static_constraints`]）只查裸色值、
// 零键盘、直连绑定、`lv_refr_now`、`unsafe` —— **都没查裸尺寸**（B2a 规格评审 ④）。
// ═══════════════════════════════════════════════════════════════════════════

/// **几何 setter** 清单：其参数是"尺寸 / 位置 / 边距"，一律须经 `theme`（或由 theme 推导的
/// 页级 `const`），**不得**写裸数字字面量。
///
/// **刻意不含**非几何 setter：`set_value` / `set_range` / `set_style_index` / `set_progress` /
/// `set_brightness` / `set_selected` 等的实参是**语义量 / 索引 / 百分比**（如 `set_range(0, 100)`
/// 是进度量程、`set_style_index(.., 0)` 是样式下标），不是设计栅格值，纳入会大面积误报。
const GEOMETRY_SETTERS: [&str; 11] = [
    "set_size",
    "set_pos",
    "set_width",
    "set_radius",
    "set_border_width",
    "set_column_width",
    "set_pad_all",
    "set_pad_top",
    "set_pad_bottom",
    "set_pad_left",
    "set_pad_right",
];

/// **带 `w`/`h` 实参的页面级几何 helper**（及其「宽 / 高」在实参表中的下标）—— **I2**。
///
/// **为什么需要它**（评审实测的**已证实绕过路径**）：只扫 `set_*` 是**不保证**的 —— 把
/// `p1_status.rs` 的 `decor(&root, 484, 320, ..)` 改成裸数字后，仅扫 setter 的版本**仍然全绿**
/// （helper 的实参不在扫描面内），而改 `set_pos(0, 8)` 会红并点名行号。⇒ 把页面里**直接出现**
/// 的几何 helper 调用点一并纳入。
///
/// ⚠️ **价值定位（如实，不得夸张）**：这是一条**减速带** —— 它能拦住"**直接实参**写死数字"，
/// **不是形式化保证**：经局部变量 / `const` / 函数间**间接传递**的裸值仍可绕过（静态文本扫描
/// 的固有限度）。`text_label(..)` **不在此表**：它的形参表是 `(parent, text, slot, color)`，
/// **没有 `w`/`h` 实参**可查。
const GEOMETRY_HELPERS: [(&str, &[usize]); 2] = [("decor", &[1, 2]), ("layout_box", &[1, 2])];

/// **允许的例外：`0`，且仅此一个**（`ui/**` 实测的全部裸数字实参都是它）。语义有二：
///
/// - `set_pos(0, y)` / `set_pos(x, 0)` —— 该轴**无偏移**（左对齐 / 上对齐），是"无值"的零点，
///   不是设计栅格值；
/// - `set_pad_*（0)` —— **零内边距**（`theme::transparent()` / `card_head_bar()` /
///   `warn_banner()` / `dialog_panel()` / `toast()` / `ChipSkin::style()` 都显式要求"内边距一律
///   为 0"，位置改由调用方 `set_pos` 给出）。
///
/// 除 `0` 外**任何**裸数字（含负号、小数、如 `set_size(100, 50)`）都判违规。
const ALLOWED_BARE_GEOMETRY_LITERAL: &str = "0";

/// 取 `(` 之后到配对 `)` 的实参文本（含嵌套括号）。
fn balanced_args(src: &str, open_paren: usize) -> &str {
    let b = src.as_bytes();
    let mut depth = 1i32;
    let mut i = open_paren + 1;
    while i < b.len() {
        match b[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return &src[open_paren + 1..i];
                }
            }
            _ => {}
        }
        i += 1;
    }
    &src[open_paren + 1..]
}

/// 顶层逗号切分（忽略括号 / 方括号 / 花括号内的逗号）。
fn split_top_level_args(args: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in args.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&args[start..]);
    parts
}

/// ⑥″ 裸尺寸：`ui/**` 的**几何 setter**（[`GEOMETRY_SETTERS`]）与**几何 helper 调用点**
/// （[`GEOMETRY_HELPERS`]）不得出现裸数字字面量（例外仅 `0`，见
/// [`ALLOWED_BARE_GEOMETRY_LITERAL`]）。
///
/// 扫描面无字面量（先剥注释与字符串 / 字符字面量）⇒ 不会被文案里的数字误伤；
/// 也不扫 `ui/tests.rs`（测试构造控件时用裸数字是**故意**的）。
///
/// ⚠️ **诚实定位**：本用例是**减速带**，不是形式化保证（局部变量 / 间接传递仍可绕过）——
/// 见 [`GEOMETRY_HELPERS`] 的文档。
#[test]
fn ui_layout_setters_use_theme_constants() {
    /// 裸数字判定（`-12` / `484` / `1.5` 是；`MAIN_CARD_W`、`A - B`、`x as i32` 不是）。
    fn bare_number(a: &str) -> bool {
        !a.is_empty()
            && matches!(a.chars().next(), Some(c) if c.is_ascii_digit() || c == '-' || c == '+')
            && a.parse::<f64>().is_ok()
    }

    for (name, src) in UI_PROD_SOURCES {
        let code = strip_comments_and_literals(src, name);
        let lines: Vec<&str> = code.lines().collect();
        let report = |what: &str, a: &str, at: usize| {
            let line = code[..at].matches('\n').count() + 1;
            panic!(
                "{name} 第 {line} 行：{what} 出现裸尺寸字面量 `{a}` \
                 （行内容：{}）—— 设计 §11.4 ④ 要求尺寸一律经 `theme` 常量；\
                 只允许例外 `0`（无偏移 / 零内边距，见 [`ALLOWED_BARE_GEOMETRY_LITERAL`] 文档）",
                lines.get(line - 1).copied().unwrap_or("").trim()
            );
        };
        // ── ① 几何 setter（`o.set_size(..)` 一类）──
        for setter in GEOMETRY_SETTERS {
            let mut from = 0usize;
            while let Some(rel) = code[from..].find(setter) {
                let at = from + rel;
                from = at + setter.len();
                // 必须是"独立调用"：前一个字符不是标识符 / `_`（`o.set_pos` 的 `.` 可）
                let before = code[..at].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let Some((_open, args)) = call_args(&code, at, setter.len()) else {
                    continue;
                };
                for arg in split_top_level_args(&args) {
                    let a = arg.trim();
                    if bare_number(a) && a != ALLOWED_BARE_GEOMETRY_LITERAL {
                        report(&format!("`{setter}`"), a, at);
                    }
                }
            }
        }
        // ── ② 几何 helper 调用点（`decor(&root, 484, 320, ..)` 一类；**I2**）──
        for (helper, arg_idxs) in GEOMETRY_HELPERS {
            let mut from = 0usize;
            while let Some(rel) = code[from..].find(helper) {
                let at = from + rel;
                from = at + helper.len();
                let before = code[..at].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let Some((_open, args)) = call_args(&code, at, helper.len()) else {
                    continue;
                };
                let parts = split_top_level_args(&args);
                for &idx in arg_idxs {
                    let Some(arg) = parts.get(idx) else { continue };
                    let a = arg.trim();
                    if bare_number(a) && a != ALLOWED_BARE_GEOMETRY_LITERAL {
                        report(&format!("`{helper}` 的第 {} 个实参（w/h）", idx + 1), a, at);
                    }
                }
            }
        }
    }
}

/// 从 `name` 之后取该次**独立调用**的括号内实参文本；不是"名字后紧跟 `(`"则 `None`。
///
/// 返回 `(开括号偏移, 实参文本)`。名字后到 `(` 之间只允许空白（挡住 import 列表、
/// 文档引用等非调用出现）。
fn call_args(code: &str, at: usize, name_len: usize) -> Option<(usize, String)> {
    let rest = &code[at + name_len..];
    let pad = rest.len() - rest.trim_start().len();
    let open_rel = rest.find('(')?;
    if !rest[..open_rel].trim().is_empty() {
        return None;
    }
    let open = at + name_len + pad + open_rel;
    Some((open, balanced_args(code, open).to_string()))
}

// ═══════════════════════════════════════════════════════════════════════════
// U-73（T21c-1）新增用例：H-2 / T-23 覆盖率网 + P4 消防的 T-14 / T-15 / T-16 / T-17 /
// T-19 / T-19b / T-20（**纯逻辑**，不触碰 LVGL ⇒ 可独立并行跑）。
//
// 离屏渲染类（T-18）与版面常量类（T-25）落在 `pages_chain` 的 P4 段（那里有 `Display` 会话）。
// ═══════════════════════════════════════════════════════════════════════════

/// `display-proto::peripherals_labels::ui_text` 的**全部常量**（名 → 值，逐条列出）。
///
/// **为什么手工列**：H-2 / T-23 的待查集合必须包含「UI 固定文案常量表」（设计 §15.7.3 的
/// 第 2–7 行），而 `ui_text` 是**扁平常量模块**（无 `ALL` 数组 ⇒ 无反射可枚举）。手工列表由
/// **计数自证**（本表的长度钉死）兜住"新增常量忘了进表"的漏项。
const UI_TEXT_CONSTANTS: [(&str, &str); 69] = [
    // 站位 / 取值降级（7）
    ("STATION_OFFLINE", ui_text::STATION_OFFLINE),
    ("NOT_READ", ui_text::NOT_READ),
    ("RANGE_ERROR", ui_text::RANGE_ERROR),
    ("NOT_CONFIGURED", ui_text::NOT_CONFIGURED),
    ("NAME_UNKNOWN", ui_text::NAME_UNKNOWN),
    ("DETAIL_UNAVAILABLE", ui_text::DETAIL_UNAVAILABLE),
    ("UNAVAILABLE", ui_text::UNAVAILABLE),
    // 站点未启用（2）
    ("STATION_DISABLED", ui_text::STATION_DISABLED),
    (
        "SECTION_STATION_DISABLED",
        ui_text::SECTION_STATION_DISABLED,
    ),
    // 段级降级（5）
    ("PERIPH_UNAVAILABLE", ui_text::PERIPH_UNAVAILABLE),
    ("FIRE_SOURCE_UNAVAILABLE", ui_text::FIRE_SOURCE_UNAVAILABLE),
    ("NO_ACTIVE_ALARM_BIT", ui_text::NO_ACTIVE_ALARM_BIT),
    (
        "BMS_ALARM_SOURCE_UNAVAILABLE",
        ui_text::BMS_ALARM_SOURCE_UNAVAILABLE,
    ),
    ("PAGE_DEVICE_AND_PERIPH", ui_text::PAGE_DEVICE_AND_PERIPH),
    // 位语义（9）
    ("BIT_UNDEFINED", ui_text::BIT_UNDEFINED),
    ("BIT_RESERVED", ui_text::BIT_RESERVED),
    ("BIT_ACTIVE", ui_text::BIT_ACTIVE),
    ("BIT_INACTIVE", ui_text::BIT_INACTIVE),
    ("ONLINE", ui_text::ONLINE),
    ("OFFLINE", ui_text::OFFLINE),
    ("BIT_ALARM_TOTAL", ui_text::BIT_ALARM_TOTAL),
    ("BIT_FAULT_TOTAL", ui_text::BIT_FAULT_TOTAL),
    ("BIT_COMM_STATE", ui_text::BIT_COMM_STATE),
    // 枚举文案（9）
    ("ENUM_NORMAL", ui_text::ENUM_NORMAL),
    ("ENUM_FIRE_LEVEL1", ui_text::ENUM_FIRE_LEVEL1),
    ("ENUM_FIRE_LEVEL2", ui_text::ENUM_FIRE_LEVEL2),
    ("ENUM_FIRE_UNDEFINED", ui_text::ENUM_FIRE_UNDEFINED),
    ("ENUM_FIRE_EMG_START", ui_text::ENUM_FIRE_EMG_START),
    ("ENUM_FIRE_EMG_STOP", ui_text::ENUM_FIRE_EMG_STOP),
    ("ENUM_UNKNOWN", ui_text::ENUM_UNKNOWN),
    ("ENUM_STOPPED", ui_text::ENUM_STOPPED),
    ("ENUM_RUNNING", ui_text::ENUM_RUNNING),
    // 时刻与提示（14；**含 T21c-1 补的三条**：COUNT_UNIT / REGISTERED_READABLE_MISMATCH / COL_SEQ
    //  与 T21c-2-r1 补的两条缩短标签：LAST_OK_SHORT / LAST_UPDATE_SHORT）
    ("LAST_OK", ui_text::LAST_OK),
    ("LAST_UPDATE", ui_text::LAST_UPDATE),
    ("LAST_OK_SHORT", ui_text::LAST_OK_SHORT),
    ("LAST_UPDATE_SHORT", ui_text::LAST_UPDATE_SHORT),
    ("REGISTERED", ui_text::REGISTERED),
    ("READABLE", ui_text::READABLE),
    ("COUNT_ALARM", ui_text::COUNT_ALARM),
    ("COUNT_FAULT", ui_text::COUNT_FAULT),
    ("COUNT_UNIT", ui_text::COUNT_UNIT),
    (
        "REGISTERED_READABLE_MISMATCH",
        ui_text::REGISTERED_READABLE_MISMATCH,
    ),
    ("COL_SEQ", ui_text::COL_SEQ),
    ("CATALOG_STALE", ui_text::CATALOG_STALE),
    ("TRUNCATED_TO", ui_text::TRUNCATED_TO),
    ("ENUM_PENDING_VENDOR", ui_text::ENUM_PENDING_VENDOR),
    // 分页与下钻（8）
    ("VIEW_DETAIL", ui_text::VIEW_DETAIL),
    ("VIEW_ALL", ui_text::VIEW_ALL),
    ("PREV_PAGE", ui_text::PREV_PAGE),
    ("NEXT_PAGE", ui_text::NEXT_PAGE),
    ("COLLAPSE", ui_text::COLLAPSE),
    ("RETRY", ui_text::RETRY),
    ("PAGE_PREFIX", ui_text::PAGE_PREFIX),
    ("PAGE_SUFFIX", ui_text::PAGE_SUFFIX),
    // 版本降级（3）
    ("VERSION_MISMATCH", ui_text::VERSION_MISMATCH),
    ("FLASH_SAME_VERSION", ui_text::FLASH_SAME_VERSION),
    ("CHANNEL_DOWN_RETRYING", ui_text::CHANNEL_DOWN_RETRYING),
    // T21c-2（P6「装置与外设」）补的 12 条（段名 / role 中文名 / 站前缀 / 下钻与摘要文案）
    ("TAB_DEVICE", ui_text::TAB_DEVICE),
    ("ROLE_HVAC", ui_text::ROLE_HVAC),
    ("ROLE_FIRE", ui_text::ROLE_FIRE),
    ("ROLE_BATTERY", ui_text::ROLE_BATTERY),
    ("ROLE_METER_BATT", ui_text::ROLE_METER_BATT),
    ("ROLE_PCS", ui_text::ROLE_PCS),
    ("STATION_PREFIX", ui_text::STATION_PREFIX),
    ("BMS_ALARM_BITS_TITLE", ui_text::BMS_ALARM_BITS_TITLE),
    ("BMS_ACTIVE_BITS_PREFIX", ui_text::BMS_ACTIVE_BITS_PREFIX),
    ("COUNT_SUFFIX", ui_text::COUNT_SUFFIX),
    ("BITS_UNIT", ui_text::BITS_UNIT),
    ("MODE_PREFIX", ui_text::MODE_PREFIX),
];

/// **H-2 / T-23 覆盖率网**（设计 §15.7.2 H-2 / §15.8 T-23）。
///
/// 待查集合 = ① `ui/**` + `state.rs` 的既有字面量（**由既有**
/// [`ui_texts_covered_by_font_cmap`]**承担**，本条不重扫）∪ ② `display-proto` 短标签表的
/// **全部 `label` / `unit`** ∪ ③ **UI 固定文案常量表**（[`UI_TEXT_CONSTANTS`]，含 §15.7.3 的
/// 七行）∪ ④ **分组标题**（`GROUP_TITLES`，按**字面量**：33 项含全角括号与数字 10 / 20 / 288）
/// ∪ ⑤ `display-proto` 的**消防锁定表**（位名 / 枚举 / 拆解标签与单位 —— §15.3.2 / §15.4）。
///
/// 基线 = [`load_font_cmap`]（**与既有那条网同一份读取逻辑**：有 `.c` 产物取 10 档交集并做
/// 漂移检测，无产物取入库清单 `lv_font_cmap.txt`；两者皆缺才跳过）—— **不另造第二套扫描器**。
///
/// **失败时打印缺失码位集合**（`U+XXXX` + 出处），便于**直接补** `font_subset_charset.txt`
/// 并重跑 `gen_fonts.sh`（§15.7 的收口口径）。
///
/// **为什么必须有它**：H-2 的使命是消除 F-5「运行时字符串扫不到」的盲区 —— 短标签表 /
/// 分组标题 / 固定文案在 T21a 之后**都在 `display-proto` 的源码里**，但**本 crate 的既有
/// 扫描面（`ui/**` + `state.rs`）看不到它们**（不在清单内）⇒ 本条把它们**逐字**拉进同一张
/// 码表网。
#[test]
fn h2_peripheral_texts_are_covered_by_font_cmap() {
    let Some(cmap) = load_font_cmap() else {
        return;
    };
    // 缺失码位 → 出处（去重后按码位升序打印）。
    let mut missing: std::collections::BTreeMap<char, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    let mut push = |s: &str, from: String| {
        for ch in s.chars() {
            if !ch.is_whitespace() && !cmap.contains(&ch) {
                missing.entry(ch).or_default().insert(from.clone());
            }
        }
    };

    // ② 短标签表（每条 label + unit）
    let mut labels = 0usize;
    for (label, unit) in mupc_display_proto::peripherals_labels::PERIPH_LABELS {
        push(label, format!("短标签 `{label}`"));
        if let Some(u) = unit {
            push(u, format!("单位 `{u}`（随 `{label}`）"));
        }
        labels += 1;
    }
    assert_eq!(labels, 447, "短标签表行数 = 白名单 447（W-4 / H-3）");

    // ④ 分组标题（**按字面量** —— 全角括号与数字 10 / 20 / 288 都是字面量的一部分）
    for (key, title) in GROUP_TITLES {
        push(title, format!("分组标题 `{key}`"));
    }
    assert_eq!(GROUP_TITLES.len(), 33, "§15.7.3 按字面量 33 项（去重 31）");
    assert!(
        GROUP_TITLES.iter().any(|(_, t)| t.contains('（')),
        "分组标题集合必须含全角括号字面量（`告警位（20）` 等）—— 否则本条的缺口核算失真"
    );

    // ③ UI 固定文案常量表（含 T21c-1 补的三条）
    assert_eq!(
        UI_TEXT_CONSTANTS.len(),
        69,
        "`ui_text` 常量表长度自证：新增常量必须同步进本表（否则本条会漏扫它）"
    );
    // **常量表完备性自证**（比"长度 == 55"强得多）：`ui_text` 模块里**每一条** `pub const`
    // 都必须在 [`UI_TEXT_CONSTANTS`] 里 ⇒ 「加了新常量却忘了进表」⇒ **本条亮红**
    // （否则本表的"全覆盖"只是自说自话 —— 那正是 H-2 要防的静默漏判形态）。
    {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../display-proto/src/peripherals_labels.rs");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读 `{}` 失败：{e}", path.display()));
        let start = src.find("pub mod ui_text {").expect("`ui_text` 模块声明");
        let body = &src[start..];
        let end = body.find("\n}").expect("`ui_text` 模块闭合");
        let declared = body[..end].matches("pub const ").count();
        assert_eq!(
            declared,
            UI_TEXT_CONSTANTS.len(),
            "`display-proto::peripherals_labels::ui_text` 模块里声明了 {declared} 条 `pub const`，\
             而 [`UI_TEXT_CONSTANTS`] 只列了 {} 条 ⇒ **新增的常量没进本表**（本条会漏扫它的字形）。\
             请把它补进本表（并在需要时同步字库）。",
            UI_TEXT_CONSTANTS.len()
        );
    }
    for (name, text) in UI_TEXT_CONSTANTS {
        push(text, format!("ui_text::{name}"));
    }

    // ⑤ display-proto 的消防锁定表（位名 / 枚举 / 拆解标签与单位）
    for (_, t) in mupc_display_proto::peripherals_labels::FIRE_LEVEL_ENUM {
        push(t, "FIRE_LEVEL_ENUM".to_string());
    }
    for (_, t) in mupc_display_proto::peripherals_labels::FIRE_SYS_BITS {
        push(t, "FIRE_SYS_BITS".to_string());
    }
    for (_, t) in mupc_display_proto::peripherals_labels::FIRE_TRIGGER_BITS {
        push(t, "FIRE_TRIGGER_BITS".to_string());
    }
    for (_, t) in mupc_display_proto::peripherals_labels::FIRE_DETECTOR_STATE_BITS {
        push(t, "FIRE_DETECTOR_STATE_BITS".to_string());
    }
    for t in mupc_display_proto::peripherals_labels::FIRE_DET_TEMPLATE_LABELS {
        push(t, "FIRE_DET_TEMPLATE_LABELS".to_string());
    }
    for d in mupc_display_proto::peripherals_labels::data1_decompose() {
        push(&d.label, "data1_decompose.label".to_string());
        if let Some(u) = &d.unit {
            push(u, "data1_decompose.unit".to_string());
        }
    }

    // **扫描面自证**：待查集合必须真的捞到东西（否则"全覆盖"是构造性全绿）。
    for must in [
        "未取数",
        "消防系统状态",
        "登记数与可读数不一致",
        "灭火瓶压力",
    ] {
        assert!(
            UI_TEXT_CONSTANTS.iter().any(|(_, t)| *t == must)
                || GROUP_TITLES.iter().any(|(_, t)| *t == must)
                || mupc_display_proto::peripherals_labels::PERIPH_LABELS
                    .iter()
                    .any(|(l, _)| *l == must),
            "扫描面自证失败：待查集合里没有 `{must}` —— 本条会构造性全绿"
        );
    }

    let detail = missing
        .iter()
        .map(|(ch, froms)| {
            format!(
                "U+{:04X} `{ch}` ← {}",
                *ch as u32,
                froms.iter().cloned().collect::<Vec<_>>().join(" / ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n          ");
    assert!(
        missing.is_empty(),
        "H-2 / T-23：以下码位**不在生成字体 cmap 内** ⇒ 真机豆腐块（共 {} 个码位）：\n          {detail}\n\
         处置：把缺失字符并入 `mupc/crates/local-display/fonts/font_subset_charset.txt`\
         （单行、无换行、无注释）→ 重跑 `fonts/gen_fonts.sh` → 同批提交 `lv_font_cmap.txt`\
         （+ `lv_font_metrics.txt` 若变）。详见设计 §15.7。",
        missing.len()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// U-73（T21c-1）离屏用例的**数据夹具**：一个「已配置 / 在线」的 fire 站 + 对应 catalog
// + 一页探测器明细。**全部照契约构造**（`PeriphView` / catalog 的位语义 / 枚举 / 拆解均按
// §15.3.2 的语义填）—— **不用生产端代码**（那属 T21b；本 crate 也不得依赖 `mupc-core-bin`）。
// ═══════════════════════════════════════════════════════════════════════════

/// 消防站 `fire_sys` 块里第 1 只探测器的首点（`at` 8..13）。
const U73_FIRE_SYS_FIRST_DET_AT: u16 = 8;

/// 位语义：按 `FIRE_SYS_BITS` 的 6 个已定义位 + 10 个 `defined: false` 构造 16 位。
fn u73_fire_sys_bits() -> Vec<mupc_display_proto::BitMeta> {
    (0..16u8)
        .map(|i| {
            let hit = mupc_display_proto::peripherals_labels::FIRE_SYS_BITS
                .iter()
                .find(|(idx, _)| *idx == i);
            mupc_display_proto::BitMeta {
                index: i,
                label: hit.map(|(_, t)| (*t).to_string()).unwrap_or_default(),
                class: if hit.is_some() {
                    mupc_display_proto::CatalogBitClass::Alarm
                } else {
                    mupc_display_proto::CatalogBitClass::Reserved
                },
                defined: hit.is_some(),
                active_text: None,
                inactive_text: None,
                // R-41 追认前**无生产者** ⇒ 一律 false（T-19b 的判据面）
                inverted: false,
            }
        })
        .collect()
}

/// 探测器状态整字的 16 位：只在 `FIRE_DETECTOR_STATE_BITS`（{12, 14}）上 `defined = true`。
fn u73_det_state_bits() -> Vec<mupc_display_proto::BitMeta> {
    (0..16u8)
        .map(|i| {
            let hit = mupc_display_proto::peripherals_labels::FIRE_DETECTOR_STATE_BITS
                .iter()
                .find(|(idx, _)| *idx == i);
            mupc_display_proto::BitMeta {
                index: i,
                label: hit.map(|(_, t)| (*t).to_string()).unwrap_or_default(),
                class: if hit.is_some() {
                    mupc_display_proto::CatalogBitClass::State
                } else {
                    mupc_display_proto::CatalogBitClass::Reserved
                },
                defined: hit.is_some(),
                active_text: None,
                inactive_text: None,
                inverted: false,
            }
        })
        .collect()
}

/// 一个 catalog 点（标签 / 单位 / 分组键由 `display-proto` 的短标签表给出 —— **与生产同源**）。
fn u73_cat_pt(
    role: mupc_display_proto::PeriphRole,
    block: &str,
    at: u16,
    bits: Vec<mupc_display_proto::BitMeta>,
    enum_labels: Vec<(u16, String)>,
    decompose: Vec<mupc_display_proto::Decompose>,
) -> mupc_display_proto::CatalogPoint {
    mupc_display_proto::CatalogPoint {
        at,
        label: mupc_display_proto::peripherals_labels::label_for(role, block, at)
            .unwrap_or_default()
            .to_string(),
        unit: mupc_display_proto::peripherals_labels::unit_for(role, block, at).map(str::to_string),
        decimals: 0,
        bits,
        enum_labels,
        decompose,
        group: mupc_display_proto::group_of(role, block, at).to_string(),
    }
}

/// 消防夹具：`(外设段, catalog)`。
///
/// - `registered`：catalog 的 `fire_det_count`（登记只数）；
/// - `readable_units`：帧内**实际携带**的探测器只数（第 1 只在 `fire_sys` 的 at 8..13，
///   其余在 `fire_det`）—— `registered != readable_units` 即 `N != M` 场景；
/// - `cylinder`：`fire_sys_2` 的「已配置」标志；
/// - `level`：火警等级的**原始值**（帧内 `fire_sys_6`）。
fn u73_fire_fixture(
    registered: u16,
    readable_units: usize,
    cylinder: Option<bool>,
    level: f64,
) -> (
    mupc_display_proto::PeripheralsSection,
    mupc_display_proto::PeripheralCatalog,
) {
    use mupc_display_proto::{
        CatalogBlock, CatalogBlockKind, CatalogStation, Decompose, FieldFlag, PeriphRole,
        PeripheralBlock, PeripheralCatalog, PeripheralStation, PeripheralsSection, PointValue,
    };

    let pt = |at: u16, v: Option<f64>| PointValue {
        at,
        v,
        flag: if v.is_some() {
            FieldFlag::Valid
        } else {
            FieldFlag::NotRead
        },
    };

    // ── 帧内 `fire_sys`（at 1..13）──
    // at1 = 系统状态字（bit14 主电故障）；at6 = 火警等级；at7 = 登记数；
    // at8..13 = 第 1 只探测器（状态字 bit12+bit14 活跃；数据 1 = 0x0A2B ⇒ 烟雾 1.0 dB/M、温度 −12 ℃）
    let mut sys_values = vec![
        pt(1, Some(f64::from(1u16 << 14))),
        pt(2, Some(250.0)),
        pt(3, Some(3.0)),
        pt(4, Some(0.0)),
        pt(5, Some(0.0)),
        pt(6, Some(level)),
        pt(7, Some(f64::from(registered))),
        pt(U73_FIRE_SYS_FIRST_DET_AT, Some(1.0)), // +0 地址
        pt(U73_FIRE_SYS_FIRST_DET_AT + 1, Some(f64::from(0x5000u16))), // +1 状态
        pt(U73_FIRE_SYS_FIRST_DET_AT + 2, Some(f64::from(0x0A2Bu16))), // +2 数据 1
        pt(U73_FIRE_SYS_FIRST_DET_AT + 3, Some(3.0)), // +3 CO
        pt(U73_FIRE_SYS_FIRST_DET_AT + 4, Some(4.0)), // +4 VOC
        pt(U73_FIRE_SYS_FIRST_DET_AT + 5, Some(5.0)), // +5 H2
    ];
    sys_values.sort_by_key(|p| p.at);

    // 第 2..readable_units 只在 `fire_det`（每只 6 点）
    let mut det_values: Vec<PointValue> = Vec::new();
    if readable_units >= 2 {
        for k in 2..=readable_units {
            let base = 6 * (k as u16 - 2);
            det_values.push(pt(base + 1, Some(k as f64))); // 地址
            det_values.push(pt(base + 2, Some(0.0))); // 状态
            det_values.push(pt(base + 3, Some(55.0))); // 数据 1 ⇒ 温度 0 ℃
            det_values.push(pt(base + 4, Some(0.0)));
            det_values.push(pt(base + 5, Some(0.0)));
            det_values.push(pt(base + 6, Some(0.0)));
        }
    }

    let sec = PeripheralsSection {
        ts_ms: 1_000,
        available: true,
        catalog_rev: 7,
        truncated: Vec::new(),
        stations: vec![PeripheralStation {
            id: "fire".to_string(),
            role: PeriphRole::Fire,
            online: true,
            last_ok_ms: 900,
            cylinder_configured: cylinder,
            blocks: vec![
                PeripheralBlock {
                    name: "fire_sys".to_string(),
                    ts_ms: 900,
                    renames: vec![(7, "fire_det_count".to_string())],
                    values: sys_values,
                },
                PeripheralBlock {
                    name: "fire_det".to_string(),
                    ts_ms: 900,
                    renames: Vec::new(),
                    values: det_values,
                },
            ],
        }],
    };

    // ── catalog：与帧同构的元数据（标签 / 单位取自短标签表；位语义 / 枚举 / 拆解按 §15.3.2）──
    let mut sys_points: Vec<_> = Vec::new();
    for at in 1..=13u16 {
        let bits = match at {
            1 => u73_fire_sys_bits(),
            9 => u73_det_state_bits(),
            _ => Vec::new(),
        };
        let enums: Vec<(u16, String)> = if at == 6 {
            mupc_display_proto::peripherals_labels::FIRE_LEVEL_ENUM
                .iter()
                .map(|(v, t)| (*v, (*t).to_string()))
                .collect()
        } else {
            Vec::new()
        };
        let dec: Vec<Decompose> = if at == 10 {
            mupc_display_proto::peripherals_labels::data1_decompose()
        } else {
            Vec::new()
        };
        sys_points.push(u73_cat_pt(
            PeriphRole::Fire,
            "fire_sys",
            at,
            bits,
            enums,
            dec,
        ));
    }
    let mut det_points: Vec<_> = Vec::new();
    for at in 1..=6u16 {
        let dec: Vec<Decompose> = if at == 3 {
            mupc_display_proto::peripherals_labels::data1_decompose()
        } else {
            Vec::new()
        };
        let bits = if at == 2 {
            u73_det_state_bits()
        } else {
            Vec::new()
        };
        det_points.push(u73_cat_pt(
            PeriphRole::Fire,
            "fire_det",
            at,
            bits,
            Vec::new(),
            dec,
        ));
    }
    let cat = PeripheralCatalog {
        rev: 7,
        generated_ms: 1_000,
        stations: vec![CatalogStation {
            id: "fire".to_string(),
            role: PeriphRole::Fire,
            enabled: true,
            blocks: vec![
                CatalogBlock {
                    name: "fire_sys".to_string(),
                    kind: CatalogBlockKind::Scalar,
                    renames: vec![(7, "fire_det_count".to_string())],
                    points: sys_points,
                },
                CatalogBlock {
                    name: "fire_det".to_string(),
                    kind: CatalogBlockKind::Scalar,
                    renames: Vec::new(),
                    points: det_points,
                },
            ],
        }],
    };
    (sec, cat)
}

/// 一页探测器明细（字段按 §15.3.2 的 `FireDetectorItem` 填；第 1 只 bit12 + bit14 活跃）。
fn u73_fire_page(
    total: u16,
    page_size: u32,
    page: u32,
    available: bool,
    items: u16,
) -> mupc_display_proto::FireDetectorPage {
    use mupc_display_proto::{FieldFlag, FireDetectorItem, FireDetectorPage, PointValue};
    let v = |x: f64| PointValue {
        at: 1,
        v: Some(x),
        flag: FieldFlag::Valid,
    };
    FireDetectorPage {
        page,
        page_size,
        total: Some(total),
        expanded: total,
        has_more: page * page_size < u32::from(total),
        available,
        items: (1..=items)
            .map(|i| FireDetectorItem {
                index: i,
                addr: v(f64::from(i)),
                state: v(if i == 1 { f64::from(0x5000u16) } else { 0.0 }),
                data1: v(f64::from(0x0A2Bu16)),
                co: v(3.0),
                voc: v(4.0),
                h2: v(5.0),
            })
            .collect(),
    }
}

/// 构造一个 `CatalogPoint` 夹具（T-14 / T-15 用）。
fn u73_cat_point(
    label: &str,
    unit: Option<&str>,
    decimals: u8,
) -> mupc_display_proto::CatalogPoint {
    mupc_display_proto::CatalogPoint {
        at: 1,
        label: label.to_string(),
        unit: unit.map(str::to_string),
        decimals,
        ..Default::default()
    }
}

/// **T-14**：`MissingReason` 各语义的字符串**两两互异**；消防钢瓶 `Some(false)` ⇒「未配置」
/// 且**忽略 `v`**（断言屏上文本**不含 `0 kPa`**）；站级态（情形 ⑦）**不塞进点级枚举**。
#[test]
fn t14_missing_reasons_are_pairwise_distinct_and_cylinder_ignores_value() {
    use crate::state::{cylinder_missing, dash_badge, MissingReason, PeriphView, StationState};
    use mupc_display_proto::{FieldFlag, PointValue};

    // ① 六态齐全且**两两互异**
    assert_eq!(MissingReason::ALL.len(), 6, "六态（§15.6.2 的代码块）");
    let texts: Vec<&str> = MissingReason::ALL.iter().map(|m| m.text()).collect();
    for i in 0..texts.len() {
        for j in (i + 1)..texts.len() {
            assert_ne!(
                texts[i], texts[j],
                "MissingReason 的文案必须两两互异（T-14）：第 {i} / {j} 项都是 `{}`",
                texts[i]
            );
        }
    }
    // ② 与既有 `dash_badge` **同一语义同一串**（不另抄一份）
    assert_eq!(
        MissingReason::NotRead.text(),
        dash_badge(FieldFlag::NotRead)
    );
    assert_eq!(
        MissingReason::RangeError.text(),
        dash_badge(FieldFlag::RangeError)
    );
    assert_eq!(
        MissingReason::StationOffline.text(),
        ui_text::STATION_OFFLINE
    );

    // ③ `cylinder_configured == Some(false)` ⇒ 「未配置」+ 忽略 v（**不含 `0 kPa`**）
    let pv = PointValue {
        at: 2,
        v: Some(0.0),
        flag: FieldFlag::Valid,
    };
    let meta = u73_cat_point("灭火瓶压力", Some("kPa"), 0);
    let mut view = PeriphView::derive(StationState::Online, &pv, Some(&meta));
    assert_eq!(view.value, Some(0.0), "未覆盖前按值正常展示");
    view.apply_cylinder(Some(false));
    assert_eq!(view.missing, Some(MissingReason::NotConfigured));
    assert_eq!(view.missing_text(), Some(ui_text::NOT_CONFIGURED));
    assert!(
        view.value.is_none(),
        "「未配置」⇒ **忽略 `v`**（值须被清掉）"
    );
    let (text, slot) = crate::ui::pages::p4_interlock::a2_display(Some(&view), None, false);
    assert_eq!(text, ui_text::NOT_CONFIGURED);
    assert!(
        !text.contains('0') && !text.contains("kPa"),
        "「未配置」行的屏上文本**不得**含 `0` / `kPa`（EDGE-23 / EX-11）：`{text}`"
    );
    assert_eq!(slot, 1, "「未配置」取弱注档（32 px `text_weak`）");

    // ④ `Some(true)` ⇒ 按值正常展示（带单位）—— 与 ③ 成对照
    let mut ok = PeriphView::derive(StationState::Online, &pv, Some(&meta));
    ok.apply_cylinder(Some(true));
    let (text, slot) = crate::ui::pages::p4_interlock::a2_display(Some(&ok), None, false);
    assert_eq!(text, "0 kPa", "已配置 ⇒ 按值 + 单位展示");
    assert_eq!(slot, 0);

    // ⑤ `None`（不可得）⇒ **不臆造**"未配置"
    assert_eq!(cylinder_missing(None), None);
    assert_eq!(cylinder_missing(Some(true)), None);

    // ⑥ 站离线 ⇒ 点级「站离线」；站**未启用**（Disabled）⇒ **无点级原因**（由段级文案承担）
    let offline = PeriphView::derive(StationState::Offline, &pv, Some(&meta));
    assert_eq!(offline.missing, Some(MissingReason::StationOffline));
    assert!(offline.value.is_none(), "站离线 ⇒ 不保留旧值");
    let disabled = PeriphView::derive(StationState::Disabled, &pv, Some(&meta));
    assert_eq!(
        disabled.missing, None,
        "「未启用」是**站级**语义，不塞进点级枚举"
    );
    assert_eq!(
        StationState::Disabled.section_text(),
        Some(ui_text::SECTION_STATION_DISABLED)
    );
    assert_ne!(
        ui_text::STATION_DISABLED,
        ui_text::STATION_OFFLINE,
        "「未启用」≠「站离线」（R-3 裁定 / §15.6.2 ⑦）"
    );
    assert_ne!(
        ui_text::SECTION_STATION_DISABLED,
        ui_text::PERIPH_UNAVAILABLE,
        "「站点未启用」≠「外设数据不可用」（单站缺席 vs 整段源不可得）"
    );

    // ⑦ catalog 未取到 ⇒ **只有名称槽降级**（值照常展示，§15.3.1）
    let no_cat = PeriphView::derive(StationState::Online, &pv, None);
    assert_eq!(
        no_cat.value,
        Some(0.0),
        "缺 catalog **不影响**值展示（按点名）"
    );
    assert_eq!(no_cat.missing, None);
    assert_eq!(no_cat.name_missing(), Some(MissingReason::NameUnavailable));
    assert_eq!(no_cat.label_text(), ui_text::NAME_UNKNOWN);
}

/// **T-14 族 / F3（T21c-2-r1）**：`hvac_di_8`（系统运行位）的两态词**由 catalog 承载**
/// ⇒ 屏侧逐位行显「**运行** / **停止**」（PRD **EX-06**；不再落通用「活跃 / 非活跃」）。
///
/// 三态判据（都在屏侧的生产构造点 `segment_model` → `state::bit_text` 上）：
/// ① `inactive_text = 停止` 生效 ⇒ 非活跃侧显「停止」；
/// ② `active_text = 运行` 生效 ⇒ 活跃侧显「运行」；
/// ③ **回退**：只给 `active_text` 的位（catalog 未登记 `inactive_text`）非活跃侧仍显通用
///    「非活跃」（**不臆造**，D22）。
///
/// **改什么会让本条红**：① 把 catalog 的 `inactive_text` 抹掉（改 `None`）⇒ ① / ③ 那条红
/// （回退成「非活跃」）；② 让屏侧忽略 `inactive_text`（`bit_text` 只用 `active_text`）⇒ ① 红。
#[test]
fn t14_hvac_run_bit_renders_the_two_state_words_carried_by_catalog() {
    use crate::ui::pages::p6_system::{segment_model, RowKind};
    use mupc_display_proto::peripherals_labels::ui_text;
    use mupc_display_proto::{FieldFlag, PeriphRole};

    // 夹具 = 生产同源的白名单 catalog；再把 `hvac_di_8` 的位语义补成**生产者给的两态词**
    // （`console_host::bit_state_words` 的同款值；两 crate 的契约面 = `BitMeta`）。
    let mut cat = p6_catalog(&[(PeriphRole::Hvac, true)]);
    let hvac = cat
        .stations
        .iter_mut()
        .find(|s| s.role == PeriphRole::Hvac)
        .expect("hvac 站");
    let di = hvac
        .blocks
        .iter_mut()
        .find(|b| b.name == "hvac_di")
        .expect("hvac_di 块");
    let p8 = di.points.iter_mut().find(|p| p.at == 8).expect("hvac_di_8");
    assert_eq!(p8.bits.len(), 1, "离散位块的点恰 1 项位语义");
    p8.bits[0].active_text = Some(ui_text::ENUM_RUNNING.to_string());
    p8.bits[0].inactive_text = Some(ui_text::ENUM_STOPPED.to_string());

    // 帧：把 `hvac_di` 块的值位 1 置 1 / 置 0（逐位块在帧内是**整字**，位 7 = 系统运行）。
    // ⚠️ 两个助手都**显式收 `&cat` 实参**（不捕获）—— 本用例在 ③ 要改 `cat`（抹 `inactive_text`）。
    let mk_sec = |cat: &mupc_display_proto::PeripheralCatalog, word: f64| {
        let mut sec = p6_section(cat, &[PeriphRole::Hvac], 0);
        let st = sec
            .stations
            .iter_mut()
            .find(|s| s.role == PeriphRole::Hvac)
            .expect("帧内 hvac 站");
        let blk = st.blocks.iter_mut().find(|b| b.name == "hvac_di").expect("hvac_di");
        blk.values.clear();
        blk.values.push(mupc_display_proto::PointValue {
            at: 8,
            v: Some(word),
            flag: FieldFlag::Valid,
        });
        sec
    };
    let bit_row = |cat: &mupc_display_proto::PeripheralCatalog,
                   sec: &mupc_display_proto::PeripheralsSection| {
        let m = segment_model(PeriphRole::Hvac, Some(cat), sec, None);
        let r = m
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Bit && r.label == "系统运行")
            .expect("`hvac_di_8` 的位行（短标签 = 「系统运行」，取自短标签表）");
        (r.value.clone(), r.active)
    };

    // ① 位 = 1 ⇒ 「运行」；② 位 = 0 ⇒ 「停止」（两态词**逐字**来自 catalog）
    let (v_on, a_on) = bit_row(&cat, &mk_sec(&cat, 128.0));
    assert_eq!(v_on, ui_text::ENUM_RUNNING, "活跃侧 = 「运行」（EX-06）");
    assert_eq!(a_on, Some(true), "活跃态进灯通道");
    let (v_off, a_off) = bit_row(&cat, &mk_sec(&cat, 0.0));
    assert_eq!(
        v_off,
        ui_text::ENUM_STOPPED,
        "非活跃侧 = 「停止」（catalog 的 `inactive_text` 生效；抹掉它 ⇒ 回退「非活跃」⇒ 本条红）"
    );
    assert_eq!(a_off, Some(false));
    assert_ne!(v_on, v_off, "两态词必须**互异**（EX-06）");
    assert_ne!(
        v_off,
        ui_text::BIT_INACTIVE,
        "「停止」**不得**等于通用「非活跃」（否则该位等于没登记语义）"
    );

    // ③ 回退：把 catalog 的 `inactive_text` 抹掉 ⇒ 非活跃侧回到通用「非活跃」
    //   （**同一个构造点**的对照断言 ⇒ 「catalog 承载」不是自说自话）
    let hvac = cat
        .stations
        .iter_mut()
        .find(|s| s.role == PeriphRole::Hvac)
        .unwrap();
    let di = hvac
        .blocks
        .iter_mut()
        .find(|b| b.name == "hvac_di")
        .unwrap();
    di.points.iter_mut().find(|p| p.at == 8).unwrap().bits[0].inactive_text = None;
    let (v_fb, _) = bit_row(&cat, &mk_sec(&cat, 0.0));
    assert_eq!(
        v_fb,
        ui_text::BIT_INACTIVE,
        "缺 `inactive_text` ⇒ 回退通用「非活跃」（**不臆造**，D22）"
    );
}

/// **T-15**：火警等级 6 值文案齐全（值域出处 = PRD F21 展示表）＋ **表外值 ⇒ 「未知」**
/// （**绝不落「正常」**）＋ catalog 路径与回退路径**各断言一次** ＋ 语义色档（未知 ⇒ 中性）。
#[test]
fn t15_fire_level_is_complete_and_out_of_table_is_unknown() {
    use crate::state::{fire_level_text, fire_level_text_static};
    use crate::ui::pages::p4_interlock::{fire_level_slot, fire_slot_color};

    // ① 回退路径（无 catalog ⇒ 用 display-proto 的锁定表）：六值齐全
    for (v, t) in mupc_display_proto::peripherals_labels::FIRE_LEVEL_ENUM {
        assert_eq!(fire_level_text_static(f64::from(v)), t, "值 {v}");
        assert_eq!(
            fire_level_text(f64::from(v), &[]),
            t,
            "空 enum_labels ⇒ 回退锁定表"
        );
    }
    // ② 表外值 ⇒ 「未知」且**不等于「正常」**
    assert_eq!(fire_level_text_static(6.0), ui_text::ENUM_UNKNOWN);
    assert_ne!(fire_level_text_static(6.0), ui_text::ENUM_NORMAL);
    assert_eq!(fire_level_text_static(f64::NAN), ui_text::ENUM_UNKNOWN);
    assert_eq!(fire_level_text(6.0, &[]), ui_text::ENUM_UNKNOWN);

    // ③ catalog 路径（运行时真源优先）
    let labels: Vec<(u16, String)> = mupc_display_proto::peripherals_labels::FIRE_LEVEL_ENUM
        .iter()
        .map(|(v, t)| (*v, t.to_string()))
        .collect();
    for (v, t) in &labels {
        assert_eq!(fire_level_text(f64::from(*v), &labels), *t);
    }
    assert_eq!(
        fire_level_text(99.0, &labels),
        ui_text::ENUM_UNKNOWN,
        "catalog 有表 ⇒ 表外值仍显「未知」（**绝不落「正常」**）"
    );

    // ④ 语义色档：**未知 / 未定义 ⇒ 中性，绝不用绿**（F21.2 / EX-10）
    assert_eq!(
        fire_slot_color(fire_level_slot(0.0)),
        Palette::OK,
        "正常 = 绿"
    );
    assert_eq!(
        fire_slot_color(fire_level_slot(6.0)),
        Palette::TEXT_WEAK,
        "表外值 = 中性色"
    );
    assert_eq!(
        fire_slot_color(fire_level_slot(3.0)),
        Palette::TEXT_WEAK,
        "「未定义」（值 3 预留）同样是中性色"
    );
    assert_ne!(fire_slot_color(fire_level_slot(6.0)), Palette::OK);
    assert_eq!(
        fire_slot_color(fire_level_slot(2.0)),
        Palette::DANGER,
        "二级火警 = 危险红"
    );
}

/// **T-16**：版本不匹配 vs 通道断开的**文案字符串不相等**；`Incompatible` **粘性**
/// （须**一次成功帧**才清除）；`got / expected` 数值正确落进屏侧状态。
#[test]
fn t16_version_mismatch_is_distinct_from_channel_down_and_sticky() {
    use crate::state::{ChannelStatus, DisplayState, ScreenMode};

    // ① 两条降级的文案**不相等**（现场排障要能区分"刷固件"与"查进程"）
    assert_ne!(ui_text::VERSION_MISMATCH, ui_text::CHANNEL_DOWN_RETRYING);
    assert_ne!(ui_text::FLASH_SAME_VERSION, ui_text::CHANNEL_DOWN_RETRYING);

    // ② 归一：**只有** `Error::ProtoVersion` 才是版本不匹配
    assert_eq!(
        crate::channel::version_mismatch(&crate::Error::ProtoVersion("u".into(), 2, 3)),
        Some((2, 3))
    );
    assert_eq!(
        crate::channel::version_mismatch(&crate::Error::Timeout("u".into())),
        None,
        "超时**不是**版本问题（不得点亮「版本不匹配」）"
    );
    assert_eq!(
        crate::channel::version_mismatch(&crate::Error::HttpStatus(503, "u".into())),
        None
    );

    // ③ 粘性：记入后**即使有旧成功帧 + 在窗内**，通道态仍是 `Incompatible`
    let mut st = DisplayState::new();
    st.record_success(frame_healthy(), 1_000);
    assert_eq!(st.channel_status(1_000), ChannelStatus::Connected);
    st.record_incompatible(2, 3, 1_100);
    assert_eq!(
        st.channel_status(1_100),
        ChannelStatus::Incompatible {
            got: 2,
            expected: 3
        },
        "got / expected 数值必须原样落到屏侧状态"
    );
    assert_eq!(
        st.screen_mode(1_100),
        ScreenMode::VersionMismatch {
            got: 2,
            expected: 3
        }
    );
    // 后续失败**不清除**粘性态（版本不匹配是部署态，不会因一次超时消失）
    st.record_fail(1_200);
    st.record_fail(5_000);
    assert_eq!(
        st.channel_status(5_000),
        ChannelStatus::Incompatible {
            got: 2,
            expected: 3
        },
        "粘性：失败不清除（否则屏面会在两种降级之间抖动）"
    );
    // ④ **一次成功帧**才清除
    st.record_success(frame_healthy(), 5_100);
    assert_eq!(st.channel_status(5_100), ChannelStatus::Connected);
    assert_eq!(st.incompatible(), None);
    // ⑤ 版本不匹配态下 `hmi_channel` 落「未知」（**不得**落「已连接」）
    let mut st2 = DisplayState::new();
    st2.record_incompatible(3, 4, 0);
    assert_eq!(
        crate::state::hmi_link_state(st2.channel_status(0)),
        mupc_display_proto::LinkState::Unknown
    );
}

/// **T-17**：探测器「数据 1」拆解 = `(raw >> 8) * 0.1` `dB/M` 与 `(raw & 0xFF) − 55` `℃`；
/// **帧内整字值不变**（EX-13）。
#[test]
fn t17_data1_decompose_matches_spec_and_keeps_the_word_intact() {
    use crate::state::decompose_value;
    use mupc_display_proto::{FieldFlag, PointValue};

    let specs = mupc_display_proto::peripherals_labels::data1_decompose();
    assert_eq!(specs.len(), 2, "拆解恰两项（烟雾 / 温度）");
    assert_eq!(specs[0].label, "烟雾");
    assert_eq!(specs[0].unit.as_deref(), Some("dB/M"));
    assert_eq!(specs[1].label, "温度");
    assert_eq!(specs[1].unit.as_deref(), Some("℃"));

    // raw = 0x0A2B ⇒ 高字节 0x0A = 10 → 1.0 dB/M；低字节 0x2B = 43 → −12 ℃
    let pv = PointValue {
        at: 3,
        v: Some(f64::from(0x0A2B_u16)),
        flag: FieldFlag::Valid,
    };
    let smoke = decompose_value(specs[0].from, pv.v.unwrap());
    let temp = decompose_value(specs[1].from, pv.v.unwrap());
    assert!((smoke - 1.0).abs() < 1e-9, "高字节 × 0.1：实测 {smoke}");
    assert_eq!(temp, -12.0, "低字节 − 55：实测 {temp}");

    // 边界：55 ⇒ 0 ℃；255 ⇒ 200 ℃；高字节 0xFF ⇒ 25.5 dB/M
    assert_eq!(decompose_value(specs[1].from, 55.0), 0.0);
    assert_eq!(decompose_value(specs[1].from, 255.0), 200.0);
    assert!((decompose_value(specs[0].from, f64::from(0xFF00_u16)) - 25.5).abs() < 1e-9);

    // **帧内整字值不变**（拆解只发生在展示层：本函数只读 `raw`，`PointValue.v` 未被改写）
    assert_eq!(
        pv.v,
        Some(f64::from(0x0A2B_u16)),
        "拆解**不得**写回帧内的整字值"
    );
    // 非有限 ⇒ 原样返回（不 panic、不造数）
    assert!(decompose_value(specs[0].from, f64::NAN).is_nan());
}

/// **T-19 负向验收（结构性）**：屏上**不存在**「柜外温度 / 柜外湿度」**数值**行；
/// **不存在**「台区 / 关口 / 总表」命名的 `meter_batt` 字段；**不存在**取自 1046–1065 的
/// 键**被标注为**「PCS 输出 / 充放功率 / 台区负荷」；**另按本设计的选择**（§15.5.2 选项 A /
/// R-42）**单独断言** `pcs_3zone_50`（1049）**整体不在白名单** —— **该条是设计决策的断言，
/// 不是 EX-23 的判据**。
#[test]
fn t19_negative_acceptance_is_structural() {
    use mupc_display_proto::{periph_whitelist_contains, PeriphRole, PERIPH_WHITELIST};

    for (i, (role, block, at)) in PERIPH_WHITELIST.iter().enumerate() {
        let (label, _unit) = mupc_display_proto::peripherals_labels::PERIPH_LABELS[i];
        // ① 无「柜外温度 / 柜外湿度」**数值**行（「柜外温感故障」等**告警位名**允许上屏）
        assert!(
            !matches!(label, "柜外温度" | "柜外湿度"),
            "屏上不得出现「{label}」数值字段（F20.4 / EX-08；点表只登记 3 个 HVAC 标量）"
        );
        // ② 无「台区 / 关口 / 总表」命名的 meter_batt 字段（F23 / EX-21）
        if *role == PeriphRole::MeterBatt {
            assert!(
                !label.contains("台区") && !label.contains("关口") && !label.contains("总表"),
                "储能表段不得有「台区/关口/总表」命名：`{label}`（{block}_{at}）"
            );
        }
        // ③ 1046–1065 不得**在表内**（因而无从被标注为 PCS 输出 / 充放功率 / 台区负荷）
        if *role == PeriphRole::Pcs && *block == "pcs_3zone" {
            assert!(
                !(47..=66).contains(at),
                "1046–1065（STS / 负载区）不得上屏：pcs_3zone_{at}（PRD §7 #11）"
            );
        }
    }
    // ④ 设计决策的**单独**断言（选项 A / R-42）：1049 = `pcs_3zone_50` 整体不在白名单
    assert!(
        !periph_whitelist_contains(PeriphRole::Pcs, "pcs_3zone", 50),
        "`pcs_3zone_50`（1049 STS 电网电压幅值）按本设计的选择**不上屏**（PRD §7 #11 侧）"
    );
    // ⑤ 屏侧源码**不得**出现把 1046–1065 误标为 PCS 输出 / 充放 / 台区负荷的串
    let src = include_str!("pages/p4_interlock.rs");
    for bad in [
        "PCS 输出",
        "充放功率",
        "台区负荷",
        "柜外温度",
        "柜外湿度",
        "pcs_3zone_50",
    ] {
        assert!(
            !src.contains(bad),
            "P4 源码不得出现 `{bad}`（EX-08 / EX-21 / EX-23 的误用禁止；\
             负向验收**不配可见声明**，§15.7.3 的刻意决定）"
        );
    }
}

/// **T-19b**：探测器状态整字 **bit15 `defined == false` 且无 `label`**；`BitMeta.inverted`
/// 在屏侧**无生产者**（R-41 追认前）；已定义位**恰 `{12, 14}`**。
#[test]
fn t19b_bit15_is_not_enabled_and_inverted_has_no_producer() {
    use mupc_display_proto::peripherals_labels::FIRE_DETECTOR_STATE_BITS;

    // ① 已定义位恰 {12, 14}（PRD F21 展示表只要求这两条）
    let idx: Vec<u8> = FIRE_DETECTOR_STATE_BITS.iter().map(|(i, _)| *i).collect();
    assert_eq!(idx, vec![12, 14]);
    assert!(FIRE_DETECTOR_STATE_BITS.iter().all(|(_, t)| !t.is_empty()));
    // ② bit15（通信状态）**不启用**（R-41 未追认）⇒ 屏显「未定义位 15」
    assert!(!crate::state::fire_det_state_bit_defined(15));
    assert!(crate::state::fire_det_state_bit_defined(12));
    assert!(crate::state::fire_det_state_bit_defined(14));
    assert_eq!(
        crate::state::bit_text(15, false, "", false, None, None, false),
        "未定义位 15",
        "未定义位 ⇒ 「未定义位 n」（**不猜语义**，F21.1 / EX-09）"
    );
    // ③ `inverted` 在**屏侧代码**里**无生产者**
    let src = include_str!("pages/p4_interlock.rs");
    assert!(
        !src.contains("inverted: true") && !src.contains("inverted = true"),
        "屏侧不得设置 `inverted`：R-41 追认前**无生产者**（catalog 构建器一律填 false）"
    );
    // ④ 「不猜」纪律：未定义位的位名恒不参与文案（屏上不出现编造语义）
    assert!(crate::state::bit_text(0, false, "", false, None, None, false).starts_with("未定义位"));
    // ⑤ 已定义位=「位名 + 活跃/非活跃」；`inverted` 位若被追认启用则走「在线 / 离线」
    assert_eq!(
        crate::state::bit_text(12, true, "报警总状态", true, None, None, false),
        "报警总状态 活跃"
    );
    assert_eq!(
        crate::state::bit_text(15, true, "通信状态", true, None, None, true),
        "通信状态 在线",
        "`inverted` 位的文案通道（追认后才可能启用；**屏侧代码零改动**）"
    );
}

/// **T-20**：页数 == 6；**无新增路由项**；下钻**不改 `current_page`**（§15.5.3）。
#[test]
fn t20_page_count_is_six_and_the_drill_is_not_a_page() {
    use crate::ui::shell::NavPage;

    assert_eq!(NavPage::ALL.len(), 6, "页数不增（T-8 裁定 / §15.5.3）");
    assert_eq!(
        mupc_display_proto::ConsoleEndpoint::ALL.len(),
        11,
        "端点清单不变（8 读 + 3 写；外设三端点在 T20 已入册，**本增量不新增**）"
    );
    // 下钻**不是页面**：页面**代码**（**剥掉注释与字面量** —— 文档里当然会写"不改
    // `current_page`"这句话）里不存在任何路由 / 页号 API ⇒ 结构性不可能改 `current_page`。
    let src = include_str!("pages/p4_interlock.rs");
    let code = strip_comments_and_literals(src, "ui/pages/p4_interlock.rs");
    for bad in ["current_page", "set_act(", "tabview", "NavPage"] {
        assert!(
            !code.contains(bad),
            "P4 的**代码**不得出现 `{bad}` —— 下钻视图**不构成页面**\
             （无独立页面状态、不经导航路由 ⇒ `current_page` 结构性不变）"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// T21c-2（P6「装置与外设」）用例：夹具 + 纯逻辑（T-19 / T-20 / 行不变量 / 版面常量 /
// 源码哨）。离屏渲染类（T-18 的墨量 / T-25 的实测几何）落在 `pages_chain` 的 P6 段。
// ═══════════════════════════════════════════════════════════════════════════

/// **F2 的版面判据**（站状态条**四列**；T21c-2-r1 / UI §6.6.1）。
///
/// `spans` = 该行**可见列**的实测几何 `(x1, x2, 槽宽)`（`Obj::coords()` + `Label::size()`）；
/// `texts` = 对应列的绑定文本。三条判据：
/// ① **恰四列**；② **相邻列不重叠**（`前一列 x2 < 后一列 x1`）；③ **不越槽**
/// （`measured_text_px(文本, 该列字号) ≤ 槽宽`，字号 = 列序 0 → 26 / 其余 24）。
///
/// **改什么会让它红**：① 把两个时刻列合并回一条（槽数 4 → 3）；② 把某一列的 x 摆到与相邻列
/// 相交；③ 把某列标签改长（如 `LAST_OK_SHORT` 回退到「最后成功」⇒ 194 px > 152 槽）。
fn assert_station_four_columns(spans: &[(i32, i32, i32)], texts: &[String], what: &str) {
    use crate::ui::theme::TextSlot;
    assert_eq!(
        spans.len(),
        4,
        "{what}：站状态条必须恰**四列**（UI §6.6.1）"
    );
    assert_eq!(texts.len(), 4, "{what}：四列文本");
    let fonts = [TextSlot::Label, TextSlot::Body, TextSlot::Body, TextSlot::Body];
    let names = ["站名", "状态", "成功列", "更新列"];
    for k in 0..4 {
        assert!(!texts[k].is_empty(), "{what}：{} 不得为空", names[k]);
        let w = measured_text_px(&texts[k], fonts[k].px());
        assert!(
            w <= spans[k].2,
            "{what}：{} 的文本「{}」实测 {w} px > 槽宽 {} px（`DOTS` 会截断）",
            names[k],
            texts[k],
            spans[k].2
        );
    }
    for k in 1..4 {
        assert!(
            spans[k - 1].1 < spans[k].0,
            "{what}：{} 与 {} **重叠**（前一列 x2 = {} ≥ 后一列 x1 = {}）",
            names[k - 1],
            names[k],
            spans[k - 1].1,
            spans[k].0
        );
    }
}

/// **W-b 的契约面**：P6 的 4 个 role 上，「`group_of` 有归键」的点**全部**在白名单内
/// ⇒ 「白名单外的一律不产行」这半句在**屏侧过滤**之前就由**分组键**成立了
/// （真正把关的是 catalog 生产者）。
///
/// **为什么必须这么写**（T21c-2 评审 W-5 的建议"夹具加一条白名单外的点"经实测**无判别力**）：
/// 白名单外的点若其 `group_of` 落到 `GROUP_UNKNOWN`，`segment_model` 的 `group_order` 循环
/// **本来就不会**给它产行 ⇒ 把过滤点改成 `if false` 该夹具**照样不产行**（探针全绿）。
/// 穷举（本用例即穷举）表明：全部 P6 role 上都**不存在**"有分组键 ∧ 不在白名单"的点
/// （唯一的例外形态是 Fire `fire_det` at ≥ 7，而 P6 **不承载 fire**，属 P4）。
///
/// **本用例的意义**：把该冗余关系**值化**。一旦它红，说明出现了"有归键但不在白名单"的点
/// ⇒ **从那一刻起屏侧过滤是承载路径**，必须为它补一条**行为**用例（而不是只留本不变量）。
/// 过滤点**存在性**另由 `t19_p6_negative_acceptance_and_shell_sentinels` 的源码计数网兜住。
#[test]
fn p6_whitelist_filter_is_redundant_with_group_keys_for_its_roles() {
    use mupc_display_proto::{group_of, periph_whitelist_contains, PeriphRole, GROUP_UNKNOWN};
    // 候选块名 = 白名单里出现过的全部块名（`group_of` 只认这些块；未登记的块恒 `unknown`）。
    let mut blocks: Vec<&str> = mupc_display_proto::PERIPH_WHITELIST
        .iter()
        .map(|(_, b, _)| *b)
        .collect();
    blocks.sort_unstable();
    blocks.dedup();
    assert!(blocks.len() > 10, "块名枚举应覆盖白名单的全部块（实测 {}）", blocks.len());
    for role in [
        PeriphRole::Hvac,
        PeriphRole::Battery,
        PeriphRole::MeterBatt,
        PeriphRole::Pcs,
    ] {
        for b in &blocks {
            for at in 1..=300u16 {
                if group_of(role, b, at) != GROUP_UNKNOWN {
                    assert!(
                        periph_whitelist_contains(role, b, at),
                        "{role:?}/{b}/{at}：有分组键 `{}` 却**不在白名单** ⇒ \
                         屏侧的白名单过滤从此是**承载路径**（本不变量失效）⇒ \
                         必须补一条行为用例（见本用例文档）",
                        group_of(role, b, at)
                    );
                }
            }
        }
    }
}

/// P6 夹具：按 role 造 catalog（点集**从白名单展开** —— 与生产构建器同源）。
///
/// `enabled` 逐站给出（`false` ⇒ 该站缺席 = 情形⑦「站点未启用」的判据面）。
/// 块形态：`hvac_di` / `bms_alarm` 取 `Discrete`（逐位一点），其余 `Scalar`。
fn p6_catalog(
    enabled: &[(mupc_display_proto::PeriphRole, bool)],
) -> mupc_display_proto::PeripheralCatalog {
    use mupc_display_proto::peripherals_labels::{label_for, unit_for};
    use mupc_display_proto::{
        BitMeta, CatalogBitClass, CatalogBlock, CatalogBlockKind, CatalogPoint, CatalogStation,
        PeripheralCatalog, PERIPH_WHITELIST,
    };

    let mut stations = Vec::new();
    for (role, en) in enabled {
        let mut blocks: Vec<CatalogBlock> = Vec::new();
        for (r, block, at) in PERIPH_WHITELIST {
            if r != role {
                continue;
            }
            let label = label_for(*r, block, *at).unwrap_or_default().to_string();
            let discrete = *block == "hvac_di" || *block == "bms_alarm";
            let bits: Vec<BitMeta> = if discrete {
                vec![BitMeta {
                    index: u8::try_from(at.saturating_sub(1)).unwrap_or(0),
                    label: label.clone(),
                    class: CatalogBitClass::Alarm,
                    defined: true,
                    active_text: None,
                    inactive_text: None,
                    inverted: false,
                }]
            } else {
                Vec::new()
            };
            let point = CatalogPoint {
                at: *at,
                label,
                unit: unit_for(*r, block, *at).map(str::to_string),
                decimals: 1,
                bits,
                enum_labels: Vec::new(),
                decompose: Vec::new(),
                group: mupc_display_proto::peripherals_labels::group_of(*r, block, *at).to_string(),
            };
            match blocks.iter_mut().find(|b| b.name == *block) {
                Some(b) => b.points.push(point),
                None => blocks.push(CatalogBlock {
                    name: (*block).to_string(),
                    kind: if discrete {
                        CatalogBlockKind::Discrete
                    } else {
                        CatalogBlockKind::Scalar
                    },
                    renames: Vec::new(),
                    points: vec![point],
                }),
            }
        }
        stations.push(CatalogStation {
            id: role_name_for_fixture(*role).to_string(),
            role: *role,
            enabled: *en,
            blocks,
        });
    }
    PeripheralCatalog {
        rev: 7,
        generated_ms: 1_000,
        stations,
    }
}

/// 夹具用的站 id（ASCII，避免把中文带进 `id`）。
fn role_name_for_fixture(role: mupc_display_proto::PeriphRole) -> &'static str {
    match role {
        mupc_display_proto::PeriphRole::Hvac => "hvac",
        mupc_display_proto::PeriphRole::Fire => "fire",
        mupc_display_proto::PeriphRole::Battery => "bms",
        mupc_display_proto::PeriphRole::MeterBatt => "meter_batt",
        mupc_display_proto::PeriphRole::Pcs => "pcs",
        mupc_display_proto::PeriphRole::Unknown => "unknown",
    }
}

/// P6 夹具：按 catalog 造帧内段（`value = 1.0`；`online` 里没列到的站取 `online = false`）。
fn p6_section(
    cat: &mupc_display_proto::PeripheralCatalog,
    online: &[mupc_display_proto::PeriphRole],
    last_ok_ms: u64,
) -> mupc_display_proto::PeripheralsSection {
    use mupc_display_proto::{
        FieldFlag, PeripheralBlock, PeripheralStation, PeripheralsSection, PointValue,
    };
    let stations = cat
        .stations
        .iter()
        .filter(|s| s.enabled)
        .map(|s| PeripheralStation {
            id: s.id.clone(),
            role: s.role,
            online: online.contains(&s.role),
            last_ok_ms,
            cylinder_configured: None,
            blocks: s
                .blocks
                .iter()
                .map(|b| PeripheralBlock {
                    name: b.name.clone(),
                    ts_ms: 1_000,
                    renames: Vec::new(),
                    values: b
                        .points
                        .iter()
                        .map(|p| PointValue {
                            at: p.at,
                            v: Some(1.0),
                            flag: FieldFlag::Valid,
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect();
    PeripheralsSection {
        ts_ms: 1_000,
        available: true,
        catalog_rev: 7,
        truncated: Vec::new(),
        stations,
    }
}

/// **T-19 的 P6 半边（负向验收，结构性）** + **T-20** + **T-25 的常量半边** + 源码哨。
#[test]
fn t19_p6_negative_acceptance_and_shell_sentinels() {
    use crate::ui::pages::p6_system;
    use crate::ui::theme::Dimens;
    use mupc_display_proto::{periph_whitelist_contains, PeriphRole};

    let src = include_str!("pages/p6_system.rs");

    // ① 误用禁止（负向验收**不配可见声明**，§15.7.3 的刻意决定）。
    for bad in [
        "PCS 输出",
        "充放功率",
        "台区负荷",
        "柜外温度",
        "柜外湿度",
        "台区",
        "关口",
        "总表",
    ] {
        assert!(
            !src.contains(bad),
            "P6 源码不得出现 `{bad}`（EX-08 / EX-21 / EX-23 的误用禁止）"
        );
    }
    // ② 1046–1065 整体不在白名单（含 1049）—— 屏上因此**无从**把它们标注成 PCS 输出。
    for at in 47..=66u16 {
        assert!(
            !periph_whitelist_contains(PeriphRole::Pcs, "pcs_3zone", at),
            "1046–1065（STS / 负载区）不得上屏：pcs_zone_{at}（PRD §7 #11）"
        );
    }
    assert!(
        !periph_whitelist_contains(PeriphRole::Pcs, "pcs_3zone", 50),
        "1049 按本设计的选择不上屏（§15.5.2 选项 A / R-42）"
    );
    // ③ 页数不变 + 无新增路由项 + P6 代码面无路由 API（T-20）。
    assert_eq!(crate::ui::shell::NavPage::ALL.len(), 6, "页数不增（T-8 裁定）");
    assert_eq!(
        mupc_display_proto::ConsoleEndpoint::ALL.len(),
        11,
        "端点清单不变（8 读 + 3 写；T21c-2 **不新增**端点）"
    );
    let code = strip_comments_and_literals(src, "ui/pages/p6_system.rs");
    for bad in ["current_page", "set_act(", "tabview", "NavPage"] {
        assert!(
            !code.contains(bad),
            "P6 的**代码**不得出现 `{bad}` —— 分段切换与下钻都**不构成页面**\
             （无独立页面状态、不经导航路由 ⇒ `current_page` 结构性不变）"
        );
    }
    // ③′ **白名单过滤点（W-b）**：`segment_model` 的两条分支（catalog / 帧）各一处，
    // 且形态必须是 `if !periph_whitelist_contains(`。
    //
    // **改什么会让本条红**：把任一处改成 `if false`（评审探针 P11 的形态）、注释掉、或删掉
    // ⇒ 计数 ≠ 2。**计数面 = 已剥注释与字面量的 `code`**（不是 raw `src`；2026-09-25 /
    // 评审 W-f：原先数 `src` ⇒ 把过滤点**用行注释注释掉、文本仍在**时计数仍为 2 ⇒ 本网
    // 对"注释掉"这一形态**静默失效**；换成 `code` 后同一改动**立刻红**，探针实测见报告）。
    // **为什么只做存在性网而不加"白名单外的点"夹具**：那种夹具**无判别力**
    // —— `group_of` 有归键的点**全部**在白名单内（穷举值化在
    // `p6_whitelist_filter_is_redundant_with_group_keys_for_its_roles`）⇒ 白名单外的点
    // 因 `group_order` 循环本就不产行，过滤点关掉也照样绿。真正把关的是 catalog 生产者。
    let filters = code.matches("if !periph_whitelist_contains(").count();
    assert_eq!(
        filters, 2,
        "`segment_model` 的 catalog / 帧两条分支**各须**一处白名单过滤（实测 {filters} 处）—— \
         过滤点是「白名单 ⇒ 结构性不上屏」的最后一道保险（生产者之外的冗余面）"
    );

    // ④ 机器键豁免口的**双判据**（调用点计数 + 实参纯 ASCII）。
    let calls = code.matches("machine_key(").count();
    assert_eq!(
        calls,
        MACHINE_KEY_CALLS,
        "`machine_key` 的调用点数变了（{calls} ≠ {MACHINE_KEY_CALLS}）—— \
         本口是**码表网的豁免面**（`NON_DISPLAY_SINKS`）：加一处就要在此同步计数，\
         否则新的豁免点会**静默扩大**（上屏中文可能借道逃过码表网）"
    );
    // ⚠️ 实参检查必须在**保留字面量**的文本上做（`strip_comments_and_literals` 会把字面量
    // 内容剥空 ⇒ 那样只能数到 `machine_key()`，查不出实参）。这里自带一个"只剥行注释"的
    // 轻量面（p6 不用块注释；`///` 亦属行注释）。
    let raw_nc: String = src
        .lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("
");
    let mut from = 0usize;
    let mut checked = 0usize;
    while let Some(rel) = raw_nc[from..].find("machine_key(\"") {
        let at = from + rel + "machine_key(\"".len();
        let endq = raw_nc[at..]
            .find('"')
            .unwrap_or_else(|| panic!("`machine_key(` 的实参引号未闭合"));
        let lit = &raw_nc[at..at + endq];
        assert!(
            lit.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "`machine_key` 的实参 `{lit}` **必须**是纯 ASCII 机器键（`[a-z0-9_]`）——              上屏文案不得经本口逃过码表网（`ui_texts_covered_by_font_cmap`）"
        );
        checked += 1;
        from = at + endq;
    }
    assert_eq!(
        checked + 1,
        calls,
        "除定义处外每处 `machine_key(` 都应是带字符串实参的调用（实测 {checked} + 定义 1 ≠ {calls}）"
    );
    // ⑤ T-25 的常量半边（几何从 `theme` 的单一真源读出）。
    assert_eq!(Dimens::TAB_COUNT, 5, "UI §6.6.1：5 段");
    assert_eq!(Dimens::TAB_W, 185, "UI §6.6.1：每段 185 宽");
    assert_eq!(Dimens::TAB_H, Dimens::TOUCH_MIN, "段高 = 触摸下限 48");
    assert_eq!(Dimens::SEG_GAP, 16, "段间隙 16（F14.2）");
    assert_eq!(Dimens::TAB_COUNT * Dimens::TAB_W + 4 * Dimens::SEG_GAP, 989);
    assert_eq!(
        Dimens::SECTION_Y,
        Dimens::TABS_Y + Dimens::TAB_H + Dimens::GAP_GROUP
    );
    assert_eq!(Dimens::SECTION_VIEW_H, 528, "UI §6.6.1：段内容视口 992×528");
    assert_eq!(Dimens::CARD_HEAD_H, 40, "UI §6.6.1：分组卡卡头 40");
    assert_eq!(Dimens::ROW_DATA_H, 44, "UI §6.6.1：数值行 44");
    assert_eq!(Dimens::ROW_BIT_H, 40, "UI §6.6.1：位行 40");
    assert_eq!(Dimens::ROW_STATION_H, 48, "UI §6.6.1：站状态条行高 48");
    assert_eq!(Dimens::DRILL_ROW_H, 44, "UI §6.6.1：下钻行高 44");
    assert_eq!(Dimens::DRILL_BTN_W, 120, "UI §6.6.1：分页 / 收起 120×48");
    assert_eq!(Dimens::DRILL_ENTRY_W, 200, "UI §6.6.1：入口按钮 200×48");
    // 段名 / 组标题取自契约（不臆造）。
    assert_eq!(p6_system::station_group_title(), "外设站状态");
    assert_eq!(p6_system::bms_alarm_group_title(), "告警位（288）");
    assert_eq!(p6_system::bms_entry_text(), "查看全部 288 位");
}

/// **P6 全部段（4 个外设段）+ 下钻视图创建后**新增的 LVGL 对象上界（**实测值**）。
///
/// **实测账（2026-09-25；T21c-2-r1 重测）**：`694`，逐项可核（构件件数由组件构造点给出）：
///
/// | 构件 | 对象数 |
/// |------|-------|
/// | 4 个外设段：每段 1 视口 + 1 内容占位 + 7 张卡框 | 4 × 9 = **36** |
/// | 4 个外设段的**段内容容器**（`SegmentedTabs::with_panel` 的 `layout_box`，惰性创建 ⇒ 每段 1 件） | **4** |
/// | 池行：`SEG_ROW_POOL` = **19 行/段**；空调段（含位行）每行 = 容器 1 + **4 个文字槽** + `LedIndicator` 4 = **9**；其余 3 段每行 = 1 + 4 = **5** | 19×9 + 3×19×5 = **456** |
/// | 段「电池」的 BMS 摘要卡（卡 1 + 卡头 1 + 摘要 1 + 4 行清单 + 入口按钮 2） | **9** |
/// | 下钻视图（根 1 + 标题 1 + 翻页/收起 3×2 + 行区视口 1 + 占位 1 + **16 个池行 × 11 件** + 失败文案 1 + 重试 2） | **189** |
/// | 合计（实测值）：36 + 4 + 456 + 9 + 189 | **694** |
///
/// ⚠️ **两处订正（T21c-2 评审 F2 / W-e）**：① 池行是 **`DRILL_ROW_POOL` = 16**（原注释写
/// 「15 个池行」**数字错**）；② **4 个外设段都真的被创建了**（原用例只点过段 1 / 段 2 ⇒
/// 「全部段」名不副实；现在 ③″ 的滚动注册点网会把 4 段都点开）。
///
/// ⚠️ **第三处订正（T21c-2-r1 复核 N-1，2026-09-25）**：本台账原先**逐项相加 = 36 + 456 +
/// 9 + 189 = 690 ≠ 694**（差额 **4**）—— 漏登的正是 **4 个段内容容器**（`with_panel` 的
/// `layout_box`，每段 1 件，见上表新增行）。**总数 694 是实测**（`pages_chain` ⑥ 的
/// `PROBE_MOUNTS` 增量 + 反向探针：预算 `694 → 693` 即红），故此处是**台账补全**、
/// 断言不受影响。
///
/// **口径（与 `SHELL_OBJECT_BUDGET` 的差别必须说清）**：那个预算量的是 `Shell::new` 区间
/// （**只有段「装置」** —— 4 个外设段与下钻都是**惰性创建**的，装配期不存在）；本常量量的是
/// "用户把 5 段都点过 + 打开过一次下钻"之后的**上界**。两者相加才是稳态常驻量级。
const P6_ALL_SEGMENTS_BUDGET: usize = 694;

/// `machine_key` 在 `p6_system.rs` 代码面（**剥注释后**）的**出现次数** = 定义 1 处 + 调用 28 处。
///
/// 调用 28 = `group_order` 的 4 个常量数组共 **26** 个分组键 + 2 处块名比较（`bms_alarm`）。
/// **加一处就要在此同步**（否则新豁免点静默扩大，见该用例的断言消息）。
const MACHINE_KEY_CALLS: usize = 29;

/// **T-18 的纯逻辑半边：行集合与行高与"取数成败"无关**（PRD §4.2.4 / F25.5）。
#[test]
fn t18_p6_row_set_and_row_heights_are_independent_of_fetch_outcome() {
    use crate::ui::pages::p6_system::{segment_model, RowKind};
    use mupc_display_proto::{FieldFlag, PeriphRole};

    let cat = p6_catalog(&[
        (PeriphRole::Hvac, true),
        (PeriphRole::Battery, true),
        (PeriphRole::MeterBatt, true),
        (PeriphRole::Pcs, true),
    ]);

    for role in [
        PeriphRole::Hvac,
        PeriphRole::Battery,
        PeriphRole::MeterBatt,
        PeriphRole::Pcs,
    ] {
        let online = p6_section(&cat, &[role], 1_000);
        let offline = p6_section(&cat, &[], 1_000);
        let m_on = segment_model(role, Some(&cat), &online, None);
        let m_off = segment_model(role, Some(&cat), &offline, None);
        // ① 行数相同
        assert_eq!(
            m_on.rows.len(),
            m_off.rows.len(),
            "{role:?}：站离线**不得**改变行数（F25.5）"
        );
        assert!(m_on.rows.len() > 5, "{role:?}：夹具应有可观行数");
        // ② 逐行高 / 种类相同
        for (a, b) in m_on.rows.iter().zip(m_off.rows.iter()) {
            assert_eq!(
                (a.kind, a.h),
                (b.kind, b.h),
                "{role:?}：降级不得改行高 / 种类"
            );
        }
        assert_eq!(m_on.total_h, m_off.total_h, "{role:?}：降级不得改内容总高");
        // ③ 离线 ⇒ 数值行全降级（`–` + 「站离线」），**不补 0**
        let mut degraded = 0usize;
        for row in m_off.rows.iter().filter(|r| r.kind == RowKind::Scalar) {
            assert!(row.degraded, "{role:?}：站离线 ⇒ 数值行必须降级");
            assert_eq!(row.value, crate::ui::pages::PLACEHOLDER);
            assert_eq!(row.aux, ui_text::STATION_OFFLINE, "三语义之一：站离线");
            degraded += 1;
        }
        assert!(degraded > 0, "{role:?}：夹具应含数值行");
        let shown = m_on
            .rows
            .iter()
            .filter(|r| r.kind == RowKind::Scalar && !r.degraded)
            .count();
        assert!(shown > 0, "{role:?}：在线时数值行可展示");

        // ④ 点位「未取数」⇒ 该行 `–` + 「未取数」（与「站离线」互异）
        let mut partial = p6_section(&cat, &[role], 1_000);
        if let Some(st) = partial.stations.iter_mut().find(|s| s.role == role) {
            if let Some(b) = st.blocks.first_mut() {
                if let Some(p) = b.values.first_mut() {
                    p.v = None;
                    p.flag = FieldFlag::NotRead;
                }
            }
        }
        let m_part = segment_model(role, Some(&cat), &partial, None);
        assert!(
            m_part
                .rows
                .iter()
                .any(|r| r.degraded && r.aux == ui_text::NOT_READ),
            "{role:?}：点位未取数 ⇒ 该行显「未取数」（与「站离线」互异）"
        );
    }

    // ⑤ 站**未启用**（catalog `enabled = false`）⇒ 段内**不产行**（站级文案承担，情形⑦）
    let cat2 = p6_catalog(&[(PeriphRole::Pcs, false)]);
    let sec2 = p6_section(&cat2, &[], 0);
    let m_disabled = segment_model(PeriphRole::Pcs, Some(&cat2), &sec2, None);
    assert_eq!(
        m_disabled.rows.len(),
        1,
        "「站点未启用」⇒ 段内**只有段级文案行**（`StationState::Disabled` 无点级原因）"
    );
    assert_eq!(m_disabled.rows[0].label, ui_text::SECTION_STATION_DISABLED);
    assert_ne!(
        m_disabled.rows[0].label,
        ui_text::PERIPH_UNAVAILABLE,
        "「站点未启用」（单站未配置）≠「外设数据不可用」（整段源不可得）"
    );
    // ⑤′ 整段不可用（EDGE-22）⇒ 段顶声明 + **行照常列出**（R-40：不因取数成败隐藏）
    let cat4 = p6_catalog(&[(PeriphRole::Hvac, true)]);
    let mut sec_na = p6_section(&cat4, &[PeriphRole::Hvac], 0);
    sec_na.available = false;
    let m_na = segment_model(PeriphRole::Hvac, Some(&cat4), &sec_na, None);
    assert_eq!(m_na.rows[0].label, ui_text::PERIPH_UNAVAILABLE);
    assert!(
        m_na.rows
            .iter()
            .filter(|r| r.kind == RowKind::Scalar)
            .count()
            > 0,
        "整段不可用也**不隐藏**数据行（R-40 的常显口径）"
    );
    // ⑥ catalog 缺 ⇒ 名位「名称未获取」但**值照常显示**（§15.3.1）
    let cat3 = p6_catalog(&[(PeriphRole::Hvac, true)]);
    let sec3 = p6_section(&cat3, &[PeriphRole::Hvac], 0);
    let m_no_cat = segment_model(PeriphRole::Hvac, None, &sec3, None);
    assert!(!m_no_cat.rows.is_empty(), "缺 catalog ⇒ 退到帧建行");
    assert!(
        m_no_cat.rows.iter().any(|r| r.label == ui_text::NAME_UNKNOWN),
        "缺 catalog ⇒ 名位「名称未获取」（不臆造）"
    );
    assert!(
        m_no_cat
            .rows
            .iter()
            .any(|r| r.kind == RowKind::Scalar && !r.degraded),
        "缺 catalog **不影响**值展示（§15.3.1）"
    );
}

/// **BMS 摘要卡 / 下钻的三态文案两两互异**（EDGE-24 / EX-16）与分页口径。
#[test]
fn t18_p6_bms_three_states_are_distinct_and_page_count_follows_page_size() {
    assert_ne!(
        ui_text::NO_ACTIVE_ALARM_BIT,
        ui_text::BMS_ALARM_SOURCE_UNAVAILABLE
    );
    assert_ne!(ui_text::NO_ACTIVE_ALARM_BIT, ui_text::STATION_OFFLINE);
    assert_ne!(
        ui_text::BMS_ALARM_SOURCE_UNAVAILABLE,
        ui_text::STATION_OFFLINE
    );
    assert_ne!(ui_text::STATION_DISABLED, ui_text::STATION_OFFLINE);
    assert_ne!(
        ui_text::SECTION_STATION_DISABLED,
        ui_text::PERIPH_UNAVAILABLE
    );
    assert_eq!(ui_text::BMS_ALARM_BITS_TITLE, "BMS 告警位");
    assert_eq!(ui_text::BMS_ACTIVE_BITS_PREFIX, "活跃告警位");
    assert_eq!(ui_text::COUNT_SUFFIX, "个");
    assert_eq!(ui_text::BITS_UNIT, "位");
    assert_eq!(
        288u32.div_ceil(50),
        6,
        "288 位 / 50 位一页 ⇒ 6 页（第 X / 6 页）"
    );
}
