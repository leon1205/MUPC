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
use crate::lvgl::obj::Obj;
// `Part` / `State` / `ScrollContainer` 不在此处 import：下游用例一律走
// `crate::lvgl::style::…` / `crate::lvgl::widgets::…` 全限定路径引用，短名反而无人使用。
use crate::lvgl::style::{Color, StyleSelector};
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
/// 剥离注释与字符串/字符字面量，供静态约束扫描使用。
///
/// 只做"扫描前预处理"，不求完备的 Rust 词法：块注释、行注释、`"…"`、`'…'`
/// （含转义）一律置空。近似之处：生命周期 `'a` 会被当作字符字面量吞掉——对本扫描
/// 无影响（关切的符号均在标识符/调用位置）。
fn strip_comments_and_literals(src: &str) -> String {
    let cs: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    while i < cs.len() {
        let c = cs[i];
        if c == '/' && i + 1 < cs.len() && cs[i + 1] == '/' {
            while i < cs.len() && cs[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < cs.len() && cs[i + 1] == '*' {
            i += 2;
            while i + 1 < cs.len() && !(cs[i] == '*' && cs[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(cs.len());
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            i += 1;
            while i < cs.len() {
                if cs[i] == '\\' {
                    i += 2;
                    continue;
                }
                if cs[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            out.push(' ');
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
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
        let lower = strip_comments_and_literals(src).to_ascii_lowercase();
        for needle in FORBIDDEN_UI_SYMBOLS {
            assert!(
                !lower.contains(needle),
                "{name} 不得出现 `{needle}`（设计 §11.1/§11.4 静态约束）"
            );
        }
    }

    // 色值构造只允许出现在 `theme.rs`（页面 / 组合控件不得内联裸色值）
    let comp = include_str!("components.rs");
    let comp = strip_comments_and_literals(comp);
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
/// 纳入会大面积误报。**这是唯一的整文件剔除**，其余 6 个文件全查。
const UI_PROD_SOURCES: [(&str, &str); 6] = [
    ("ui/mod.rs", include_str!("mod.rs")),
    ("ui/theme.rs", include_str!("theme.rs")),
    ("ui/components.rs", include_str!("components.rs")),
    ("ui/pages/mod.rs", include_str!("pages/mod.rs")),
    ("ui/pages/p1_status.rs", include_str!("pages/p1_status.rs")),
    ("ui/pages/p6_system.rs", include_str!("pages/p6_system.rs")),
];

/// **非屏显出口**白名单：紧跟这些 token 的字符串字面量**不会**被画到屏上（逐条列出，
/// 不靠正则猜）。除此之外**一律**当上屏候选查（宁可多查）：
///
/// - `InvalidArgument(` —— `LvglError` 的错误消息（原型 `LvglError::InvalidArgument("…")`），
///   只在 `Err` 里流转；薄层没有任何"把 `LvglError` 画上屏"的路径 ⇒ 从不屏显；
/// - `debug_struct(` / `.field(` —— `std::fmt::Debug` 实现的字段名（供日志 / 断言阅读）；
/// - `env!(` / `option_env!(` —— 环境变量**键名**（屏上取到的是它的**值**，键名不屏显）。
///
/// 另有两类"不是字面量文本"的排除（写在 [`ui_source_chars`] 里）：
/// ① `format!` 模板的 `{…}` 占位符内容（`"{d} 日"` 里 `d` 不是字形，值才是）；
/// ② `#[cfg(test)]` 区（各文件的测试模块都在文件末尾，且**断言形态**后若不符即响亮失败）。
const NON_DISPLAY_SINKS: [&str; 5] = [
    "InvalidArgument(",
    "debug_struct(",
    ".field(",
    "env!(",
    "option_env!(",
];

/// **已登记**的字库缺口：扫源码确实用到、但生成字体的 cmap 里**没有**的字形。
///
/// **现为空**：B2a 收尾时实测 `format_uptime` 的 `天`（唯一缺字）不在 cmap 内，已按
/// `ui/pages/mod.rs` 偏差登记 **D5** 改为在 cmap 内的同义词 `日`（`1 日` = 1 天）
/// ⇒ 源码层不再有缺字。**本清单必须与实测缺字集合相等**（新增缺字 ⇒ 红；字库补齐后
/// 条目未删 ⇒ 也红 —— **有意**如此：防止登记腐化）。
const KNOWN_MISSING: [(char, &str); 0] = [];

/// 从 `lv_font_noto_sc_*.c` 解析**实际支持的码点集合**。
///
/// **本版本 `lv_font_conv` 生成物的确切形态**（2026-09-11 对 10 个字号逐一实测，
/// 不是猜的）：
///
/// ```c
/// static const uint16_t unicode_list_0[] = { 0x0, 0x1, 0x5, /* …升序… */ };
/// static const lv_font_fmt_txt_cmap_t cmaps[] = { {
///     .range_start = 32, .range_length = 40633, .glyph_id_start = 1,
///     .unicode_list = unicode_list_0, .glyph_id_ofs_list = NULL,
///     .list_length = 324, .type = LV_FONT_FMT_TXT_CMAP_SPARSE_TINY } };
/// ```
///
/// ⇒ **码点 = `.range_start + unicode_list_0[i]`**（数组存的是相对偏移，**不是**码点本身；
/// 直接当码点用会漏掉几乎全部 CJK）。形态与解析前提不符时**响亮失败** —— 否则"解析不到"
/// 会伪装成"全部覆盖"（正是要修的那类缺陷）。
fn font_cmap_from_c(src: &str, name: &str) -> std::collections::BTreeSet<char> {
    let lists = src.matches("static const uint16_t unicode_list_0[]").count();
    let ranges = src.matches(".range_start =").count();
    assert!(
        lists == 1 && ranges == 1,
        "{name}：`lv_font_conv` 输出形态已变（unicode_list_0 × {lists}、range_start × {ranges}）\
         —— 本解析器只认「单 cmap + 单 unicode_list」形态，请据此更新（不得静默跳过）"
    );
    assert!(
        src.contains("LV_FONT_FMT_TXT_CMAP_SPARSE_TINY"),
        "{name}：cmap 类型不是 SPARSE_TINY（`unicode_list` 的偏移语义随之不同），请复核解析"
    );
    let start = src.find("static const uint16_t unicode_list_0[]").expect("已断言存在");
    let body_open = start + src[start..].find('{').expect("数组体");
    let body = &src[body_open + 1..];
    let body = &body[..body.find("};").expect("数组结束")];
    let rs_at = src.find(".range_start =").expect("已断言存在");
    let rs: u32 = src[rs_at + ".range_start =".len()..]
        .trim_start()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|s| s.parse().ok())
        .expect("range_start 应是十进制整数");
    let mut set = std::collections::BTreeSet::new();
    let b = body.as_bytes();
    let mut i = 0usize;
    while i + 1 < b.len() {
        if b[i] == b'0' && b[i + 1] == b'x' {
            let mut j = i + 2;
            while j < b.len() && b[j].is_ascii_hexdigit() {
                j += 1;
            }
            if let Ok(v) = u32::from_str_radix(&body[i + 2..j], 16) {
                if let Some(ch) = char::from_u32(rs + v) {
                    set.insert(ch);
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    assert!(!set.is_empty(), "{name}：unicode_list 解析出 0 个码点 —— 解析器与生成物脱节");
    set
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

/// 扫一段 `ui/**` 源码，取出**会取字形的字符**、出处字面量与**行号**（1 起）。
///
/// 口径（逐条）：剥注释；取 `"…"` 字符串字面量与 `'x'` / `'\u{…}'` 字符字面量
/// （**生命周期 `'a` 不是字面量**，按普通字符跳过）；`\u{XXXX}` 转义**解码成真字符**
/// （否则"用转义写的上屏字"会漏判）；忽略 [`NON_DISPLAY_SINKS`] 之后的字面量；
/// 剥 `{…}` 占位符；掐掉 `#[cfg(test)]` 区。空白字符不计。
/// **行号**让缺字报错能点名「文件:行 ← 文案」，而不是只报一个裸字形（B2a 收尾订正）。
fn ui_source_chars(src: &str, name: &str) -> Vec<(char, String, usize)> {
    let src = match src.find("#[cfg(test)]") {
        Some(i) => {
            assert!(
                src[i..].contains("mod tests"),
                "{name}：`#[cfg(test)]` 之后不是 `mod tests` —— 本扫描「截断到首个 \
                 #[cfg(test)]」的前提不成立，请改扫描工具（不得静默放过）"
            );
            &src[..i]
        }
        None => src,
    };
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
/// c) 实测 2026-09-11：10 档 cmap **完全相同**（各 324 码位，并集 − 交集 = 0）⇒ 当前
/// 两种取法**结果一致**，选交集只是把"未来某档掉字"这件事**钉在红灯上**。
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
    for (name, src) in UI_PROD_SOURCES {
        for (ch, lit, line) in ui_source_chars(src, name) {
            literals.insert(lit.clone());
            if !cmap.contains(&ch) {
                missing.push((ch, lit, name, line));
            }
        }
    }

    // 清册（`ALL_TEXTS`）**不是基线**，但必须与源码不脱节：**每个条目都得整串出现在某个
    // `ui/**` 源码字面量里**（M4：只断"每个字都出现过"太弱 —— 例如 `%` 能从**任何**含 `%`
    // 的字面量里"借光"通过 —— 见 `literals` 的注释）。
    //
    // ⚠️ **诚实标注本检查的边界**：常量条目**自身的声明**就是一个字面量 ⇒ "声明了但没人用"
    // （`TEXT_DEVICE_TOTAL_POWER` 那种）仍能通过这一条。本检查管的是"清册 ↔ 源码字面量
    // **串**是否一致"（改了源码文案却忘了改清册 ⇒ 红），**不**管"常量是否真的被引用"
    //（那是 `dead_code` 与该常量读者 / 评审的活）。
    for t in ALL_TEXTS.iter().chain(crate::ui::pages::ALL_TEXTS.iter()) {
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
    for ms in RUNTIME_FMT_MS_INPUTS {
        cases.push((format!("format_epoch_ms_utc({ms})"), pages::format_epoch_ms_utc(ms)));
    }
    for s in RUNTIME_FMT_SECS_INPUTS {
        cases.push((format!("format_uptime({s})"), pages::format_uptime(s)));
    }
    // 占位符是**上屏**的固定字形（`--` 会出豆腐块 —— 见 `PLACEHOLDER` 文档）。
    cases.push(("PLACEHOLDER".to_string(), pages::PLACEHOLDER.to_string()));

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
fn frame_healthy() -> mupc_display_proto::DisplayFrame {
    use mupc_display_proto::*;
    DisplayFrame {
        version: PROTO_VERSION,
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

    drop(host);
    drop(screen);
    drop(disp);
    lvgl::deinit();
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
        let lower = strip_comments_and_literals(src).to_ascii_lowercase();
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
            !strip_comments_and_literals(src).contains("unsafe"),
            "{name} 不得出现 `unsafe`（设计 §1.1.1.2 纪律 1）"
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
        let code = strip_comments_and_literals(src);
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
