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
/// - **③b 非原始串不得跨行** —— 合法 Rust 的普通 / 字节 / C 串**不能含裸换行**（唯一跨行途径
///   是 `\` 行续接，已被转义分支吃掉）⇒ 扫描中遇到裸换行即响亮失败。这一条正是为
///   `r"a\"` / `r#"…"#` 那类"越过真实闭引号、到**下一个无关引号**才闭合"的失真形态设的：
///   它们一旦失真，就必然跨过行边界（本单元实测：两形态**均可**被 ③b 抓到）。
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
fn strip_comments_and_literals(src: &str, name: &str) -> String {
    let cs: Vec<char> = src.chars().collect();
    let line_of = |at: usize| 1 + cs[..at.min(cs.len())].iter().filter(|&&c| c == '\n').count();
    let mut out = String::with_capacity(src.len());
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
                    if cs.get(i + 1) == Some(&'\n') {
                        out.push('\n');
                    }
                    i += 2;
                    continue;
                } else if cs[i] == '"' {
                    i += 1;
                    closed = true;
                    break;
                } else if cs[i] == '\n' {
                    // **自证 ③b**：合法 Rust 的**非原始**串不得含裸换行（唯一跨行途径是
                    // `\` 行续接，已在上一条被吃掉）⇒ 走到这里即坐实"这个引号不是真开引号"
                    //（典型成因：原始串前缀 `r"` / `r#"` / `br"` / `br#"` 没被识别）。
                    unclosed(
                        name,
                        line_of(i),
                        &format!("非原始字符串（起于第 {} 行）在闭合前跨了行", line_of(open)),
                    );
                }
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
                    // 故这里也必须补一个换行，否则"行数恒等"的自证会误报。
                    if cs.get(i + 1) == Some(&'\n') {
                        out.push('\n');
                    }
                    i += 2;
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
    out
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

/// **自证 ③ 的失败出口**：一类定界符"扫到 EOF 仍未闭合"或"非原始串跨了行" ⇒ **响亮失败**
/// （永不返回）。
///
/// 抽成独立函数只为让三处调用点共用一段文案（`-> !` ⇒ 调用点不必写 `return`）。判据、为什么
/// 合法 Rust 里零误报、以及**残余边界**见 [`strip_comments_and_literals`] 的"自证 ③"一节。
fn unclosed(name: &str, line: usize, what: &str) -> ! {
    panic!(
        "{name}:{line} —— {what}。\n\
         合法 Rust 源码里**不存在**未闭合的字面量 / 块注释，也不存在**含裸换行**的非原始\
         字符串 ⇒ 这是**扫描器失真**（不是源码问题）：多半是某类字面量**前缀**没被识别\
         （`r\"…\"` / `r#\"…\"#` / `b\"…\"` / `br#\"…\"#`），于是把非字面量处的引号当成开\
         引号、越过真实闭引号一路吞下去。被吞掉的源码对下游**全部静态网不可见**（静默失明）。\
         请修 [`literal_prefix`] / [`strip_comments_and_literals`]（判据与前缀形态清单见\
         二者文档）。"
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
const UI_PROD_SOURCES: [(&str, &str); 9] = [
    ("ui/mod.rs", include_str!("mod.rs")),
    ("ui/theme.rs", include_str!("theme.rs")),
    ("ui/components.rs", include_str!("components.rs")),
    ("ui/controls.rs", include_str!("controls.rs")),
    ("ui/pages/mod.rs", include_str!("pages/mod.rs")),
    ("ui/pages/p1_status.rs", include_str!("pages/p1_status.rs")),
    ("ui/pages/p2_config.rs", include_str!("pages/p2_config.rs")),
    ("ui/pages/p4_interlock.rs", include_str!("pages/p4_interlock.rs")),
    ("ui/pages/p6_system.rs", include_str!("pages/p6_system.rs")),
];

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
const NON_DISPLAY_SINKS: [&str; 8] = [
    "InvalidArgument(",
    "debug_struct(",
    ".field(",
    "stderr(),",
    "env!(",
    "option_env!(",
    "config_key(",
    "source_key(",
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
    for t in ALL_TEXTS
        .iter()
        .chain(crate::ui::pages::ALL_TEXTS.iter())
        .chain(crate::ui::controls::ALL_TEXTS.iter())
        .chain(crate::ui::pages::p2_config::ALL_TEXTS.iter())
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
        let mut applied = p2_view(false);
        applied.revision = 8;
        p2.show_result(&ControlResponse::ok(
            "rid-3",
            Some(applied.clone()),
            Some("audit-2".into()),
            1_002,
        ))
        .expect("show_result (ok)");
        assert_eq!(
            p2.field_value_text("gateway.port").as_deref(),
            Some("2404"),
            "成功 ⇒ 用回执 `applied` 刷新本地值"
        );
        assert!(!p2.is_dirty(), "刷新后回到不脏");
        assert_eq!(p2.toast_tone(), Some(components::ToastTone::Success));
        assert_eq!(p2.toast_text().as_deref(), Some(p2_config::TEXT_TOAST_OK));
        assert_eq!(p2.field_error_visible("gateway.port"), Some(false), "成功清错误");

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
        assert_eq!(
            p4.scroll_obj().size(),
            (Dimens::CONTENT_W, 528),
            "滚动视口 = 624 − 操作条 72 − **就地原因带 24** = 528（⚠️ UI 线框写 552 —— \
             见 p4_interlock.rs 的 **IL4**：原因带落在**视口末 24 px**（绝对 y600–624），\
             线框只是**未画**该行；固定操作条仍与线框逐像素一致）"
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

        // 两张主卡的尺寸锚点（总态卡 `174` 见 **IL3**；触发源卡在 2 源时 `202`）。
        // **这两条同时是"存活锚点"回归锁**：`Core::state_card` 曾是 `new()` 的局部变量 ⇒ 返回时
        // 被 `Drop`、总态卡**整棵子树级联删除**（屏幕上一块空白、控制台无任何报错）；本用例的
        // 「骨架态 ⇒ 联锁状态不可用」断言（读卡内标签的文本）当场抓出了它（B2b-3 实测）。
        assert_eq!(
            p4.state_card_obj().size().1,
            174,
            "联锁总态卡高（UI 线框 180，−6 见 **IL3**）"
        );
        assert!(p4.state_card_obj().is_alive(), "总态卡必须存活（否则卡内全部子件被级联删除）");
        assert!(p4.source_card_obj().is_alive(), "触发源卡必须存活");

        // ── ② 骨架态 = **无帧** ⇒ 不可用（**绝不**「未联锁」）─────────────────────
        // 「改什么会让本条变红」：把 `new()` 末尾的 `apply_section(default)` 去掉（或把
        // `state_view` 的 `!available` 分支删掉）⇒ 下面「联锁状态不可用」那条立刻变红。
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE),
            "无帧 ⇒ 契约缺省 = 不可用"
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
        // 「改什么会让本条变红」：把 `state_view` 改成 `if s.latched {..} else if !available` 或
        // 把 `!available` 分支回落成 `Unlatched`（fail-open）⇒ 第 2 / 3 条立刻变红。
        p4.set_section(&il(false, true, false));
        assert_eq!(
            p4.state_text().as_deref(),
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE),
            "状态源不可用 ⇒ 「联锁状态不可用」（F16.6 / IL-01）"
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
        assert_eq!(
            p4.toast_text().as_deref(),
            Some(p4_interlock::TEXT_TOAST_FAIL)
        );
        assert!(
            p4.take_refresh_request(),
            "RejectedPrecondition ⇒ 置「请求一次状态刷新」标志（IL12 / EDGE-19 的刷新语义）"
        );
        assert!(!p4.take_refresh_request(), "取走即清（只刷一次）");

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
            Some(p4_interlock::TEXT_STATE_UNAVAILABLE),
            "无帧 ⇒ 不可用（**不**沿用上一帧的「未联锁」—— 缺帧 ≠ 确知未联锁）"
        );
        assert!(p4.release_disabled() && p4.restart_disabled());
        // 渲染后确有像素（装配 → 布局 → 像素全链）。
        let painted4 = sink.borrow().iter().filter(|b| **b != 0).count();
        assert!(painted4 > 10_000, "P4 渲染后 sink 中应有成片非背景像素（实际 {painted4}）");

        drop(p4);
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
const CONST_I32_SCAN_SOURCES: [(&str, &str); 8] = [
    ("ui/mod.rs", include_str!("mod.rs")),
    ("ui/components.rs", include_str!("components.rs")),
    ("ui/controls.rs", include_str!("controls.rs")),
    ("ui/pages/mod.rs", include_str!("pages/mod.rs")),
    ("ui/pages/p1_status.rs", include_str!("pages/p1_status.rs")),
    ("ui/pages/p2_config.rs", include_str!("pages/p2_config.rs")),
    ("ui/pages/p4_interlock.rs", include_str!("pages/p4_interlock.rs")),
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
