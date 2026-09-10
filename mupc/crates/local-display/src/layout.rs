//! 固定网格布局渲染：把归一化视图（`UiSnapshot`）画到 1024x768 画布。
//!
//! 对齐 `[DESIGN_APPROVED]` UI 设计 §4/§5/§6 坐标与色板（视觉权威）＋技术设计 §6.2/§6.3。
//! - 三区一屏：页眉 / SOC 主区（左 L1）/ PCS 状态主区（右 L1）/ 三相四卡（下 L2，A/B/C/总）。
//! - 颜色/字号阶梯按 UI §6.1/§6.3；数值等宽（tabular 约束由数字用字保证，UI §6.5）。
//! - 降级三态：字段失效显 `--` + 角标；SOC 双源皆失显 `--` + 源失效警示；通道断 → 整屏覆盖页。
//! - **脏区/抗闪烁（设计 §6.8 KISS 起步）**：本期 `render` 为**全屏重绘 + 静区重写同色**（无动画、
//!   值变即替换）；真机接入若需降 CPU 可后续在此收窄为脏矩形（每字段区域已聚成独立 draw_*，便于改）。
//! - 坐标/字号允许 ±10% 视觉对齐（UI §11）；分区与三态语义不变。

use mupc_display_proto::{RunState, SocSource};

use crate::canvas::{Canvas, Color, Rect};
use crate::font::TextKit;
use crate::state::{
    dash_badge, live_dot_for, soc_band, Freshness, NumView, ScreenMode, SocBand, SocView,
    UiSnapshot,
};

// ---------------------------------------------------------------------------
// 全局框架色（UI §6.1 / §6.2 色板，hex 字面 = #RRGGBB）
// ---------------------------------------------------------------------------
pub const BG: Color = 0x0B_12_20; // 画布底 近黑藏蓝 #0B1220
pub const CARD: Color = 0x14_1F_33; // 区块卡底色 #141F33
pub const CARD_DEEP: Color = 0x1B_29_42; // 卡内嵌条/胶囊底 #1B2942
pub const LINE: Color = 0x2A_3B_57; // 分隔/描边 #2A3B57
pub const TEXT_MAIN: Color = 0xF4_F7_FF; // 主文本 #F4F7FF
pub const TEXT_SUB: Color = 0xA6_B6_D6; // 次文本 #A6B6D6
pub const TEXT_WEAK: Color = 0x6E_7F_A0; // 弱文本 #6E7FA0
pub const DEGRADED: Color = 0x96_A2_BC; // "--"/降级灰 #96A2BC
pub const CAP_BG: Color = 0x2A_35_50; // 胶囊底 #2A3550
pub const CAP_BORDER: Color = 0x3B_4A_6B; // 胶囊描边 #3B4A6B

pub const CHARGE_GREEN: Color = 0x2F_DB_8A; // 充电绿 #2FDB8A
pub const DISCHARGE_BLUE: Color = 0x4E_A6_FF; // 放电蓝 #4EA6FF
pub const STOP_GRAY: Color = 0x8C_98_AC; // 停机冷灰 #8C98AC
pub const STANDBY_YELLOW: Color = 0xFF_D7_5E; // 待机明黄 #FFD75E
pub const SOC_CYAN: Color = 0x35_D0_C4; // SOC 正常青 #35D0C4
pub const SOC_RED: Color = 0xFF_6B_6B; // SOC ≤15 红 / 数据异常 #FF6B6B
pub const SOC_ORANGE: Color = 0xFF_A9_4D; // SOC ≥85 橙 #FFA94D
pub const AMBER: Color = 0xFF_B0_20; // 过期琥珀 #FFB020
pub const PINK: Color = 0xFF_5C_D0; // 方向不一致品红 #FF5CD0
pub const WHITE: Color = 0xFF_FF_FF;

// ---------------------------------------------------------------------------
// 分区网格坐标（UI §4.3；此处暴露供绘制与测试共用，避免数字漂移）
// ---------------------------------------------------------------------------
pub const SCREEN_W: i32 = 1024;
pub const SCREEN_H: i32 = 768;

/// 页眉 Y 16–72。
pub const HDR_X0: i32 = 16;
pub const HDR_Y0: i32 = 16;
pub const HDR_X1: i32 = 1008;
pub const HDR_Y1: i32 = 72;

/// SOC 主区（16,88,496,420）。
pub const SOC_X0: i32 = 16;
pub const SOC_Y0: i32 = 88;
pub const SOC_X1: i32 = 496;
pub const SOC_Y1: i32 = 420;

/// PCS 运行状态主区（528,88,1008,420）。
pub const PCS_X0: i32 = 528;
pub const PCS_Y0: i32 = 88;
pub const PCS_X1: i32 = 1008;
pub const PCS_Y1: i32 = 420;

/// 三相面板（16,444,1008,752）。
pub const PANEL_X0: i32 = 16;
pub const PANEL_Y0: i32 = 444;
pub const PANEL_X1: i32 = 1008;
pub const PANEL_Y1: i32 = 752;

/// 四卡（Y 508–736，卡高 228；A/B/C/总 x 区间）。
pub const CARD_TOP: i32 = 508;
pub const CARD_BOT: i32 = 736;
pub const CARD_A_X: (i32, i32) = (32, 260);
pub const CARD_B_X: (i32, i32) = (276, 504);
pub const CARD_C_X: (i32, i32) = (520, 748);
pub const CARD_T_X: (i32, i32) = (764, 992);

// SOC 主区内部（绘制基准）
pub const SOC_TITLE_FONT: f32 = 26.0;
pub const SOC_VALUE_FONT: f32 = 148.0; // UI §6.3 L1-特大
pub const SOC_PCT_FONT: f32 = 56.0;
pub const SOC_VALUE_TOP: i32 = 170;
pub const SOC_RANGE_TOP: i32 = 336;
pub const SOC_RANGE_H: i32 = 14;
pub const SOC_RANGE_W: i32 = 360;

// PCS 主区内部
pub const PCS_TITLE_FONT: f32 = 26.0;
pub const PCS_WORD_FONT: f32 = 120.0;
pub const PCS_WORD_TOP: i32 = 172;
pub const PCS_SUB_TOP: i32 = 356;

// 卡内垂直（相对 CARD_TOP）
const PH_HDR_FONT: f32 = 28.0;
const PH_P_LABEL_TOP: i32 = 44;
const PH_P_VALUE_TOP: i32 = 68;
const PH_P_FONT: f32 = 64.0; // UI L2-主值
const PH_UNIT_FONT: f32 = 26.0;
const PH_I_LABEL_TOP: i32 = 136;
const PH_I_VALUE_TOP: i32 = 156;
const PH_I_FONT: f32 = 48.0; // UI L2-次值

/// 便捷：区域矩形构造（供绘制与测试共用）。
pub fn soc_card() -> Rect {
    Rect::new(SOC_X0, SOC_Y0, SOC_X1, SOC_Y1)
}
pub fn pcs_card() -> Rect {
    Rect::new(PCS_X0, PCS_Y0, PCS_X1, PCS_Y1)
}
pub fn phase_cards() -> [Rect; 4] {
    [
        Rect::new(CARD_A_X.0, CARD_TOP, CARD_A_X.1, CARD_BOT),
        Rect::new(CARD_B_X.0, CARD_TOP, CARD_B_X.1, CARD_BOT),
        Rect::new(CARD_C_X.0, CARD_TOP, CARD_C_X.1, CARD_BOT),
        Rect::new(CARD_T_X.0, CARD_TOP, CARD_T_X.1, CARD_BOT),
    ]
}
/// A/B/C 卡序号（0..3；总卡=3）。
pub fn phase_card(i: usize) -> Rect {
    phase_cards()[i]
}

/// SOC 大数字绘制区域（值存在时应有带色字形像素落于此——供离屏断言）。
pub fn soc_value_region() -> Rect {
    Rect::new(SOC_X0 + 30, SOC_VALUE_TOP - 10, SOC_X1 - 30, SOC_VALUE_TOP + 150)
}

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// 全屏渲染一帧（KISS：全屏重绘；脏区收窄为后续优化点，见模块注释）。
pub fn render(canvas: &mut dyn Canvas, tk: &TextKit, snap: &UiSnapshot) {
    canvas.clear(BG);
    match snap.mode {
        ScreenMode::Init => draw_init(canvas, tk),
        ScreenMode::ChannelDown => draw_channel_down(canvas, tk),
        ScreenMode::Live => draw_live(canvas, tk, snap),
    }
}

/// 单值整帧渲染入口（构造临时 canvas；供 bin 侧 `--smoke` / 测试直接用）。
pub fn render_frame(tk: &TextKit, snap: &UiSnapshot) -> crate::canvas::OffscreenCanvas {
    let mut cv = crate::canvas::OffscreenCanvas::new(SCREEN_W as u32, SCREEN_H as u32);
    render(&mut cv, tk, snap);
    cv
}

fn draw_init(canvas: &mut dyn Canvas, tk: &TextKit) {
    let cx = SCREEN_W as f32 / 2.0;
    tk.draw_centered_top(canvas, "正在连接数据通道…", cx, 320.0, 56.0, TEXT_SUB);
    tk.draw_centered_top(canvas, "MUPC 本地显示终端", cx, 400.0, 24.0, TEXT_WEAK);
}

fn draw_channel_down(canvas: &mut dyn Canvas, tk: &TextKit) {
    let cx = SCREEN_W as f32 / 2.0;
    tk.draw_centered_top(canvas, "与主进程数据通道断开", cx, 310.0, 56.0, AMBER);
    tk.draw_centered_top(canvas, "自动重连中，恢复后 ≤1s 回到实时…", cx, 392.0, 24.0, TEXT_SUB);
}

fn draw_live(canvas: &mut dyn Canvas, tk: &TextKit, snap: &UiSnapshot) {
    draw_header(canvas, tk, snap);
    draw_soc(canvas, tk, snap);
    draw_pcs(canvas, tk, snap);
    draw_panel(canvas, tk, snap);
}

// ---------------------------------------------------------------------------
// 页眉
// ---------------------------------------------------------------------------

fn draw_header(canvas: &mut dyn Canvas, tk: &TextKit, snap: &UiSnapshot) {
    // 标题（UI §4.2：MUPC · 台区储能装置运行状态）
    tk.draw_top(canvas, "MUPC · 台区储能装置运行状态", HDR_X0 as f32, 24.0, 26.0, TEXT_MAIN);
    // 右下通道/新鲜度
    let right = HDR_X1 as f32;
    let status_txt;
    let status_color: Color;
    let dot_color: Color;
    match snap.fresh {
        Freshness::Fresh => {
            status_txt = "实时 · 通道已连接";
            status_color = TEXT_SUB;
            dot_color = SOC_CYAN;
        }
        Freshness::Stale => {
            status_txt = "数据过期";
            status_color = AMBER;
            dot_color = AMBER;
        }
    }
    let status_w = tk.measure(status_txt, 22.0);
    // 时钟
    let clock_w = if snap.clock_text.is_empty() {
        0.0
    } else {
        tk.measure(&snap.clock_text, 24.0)
    };
    let dot_x = right - status_w - clock_w - 40.0;
    let dot_y = 30.0f32;
    fill_disc(canvas, dot_x as i32 + 4, dot_y as i32 + 6, 5, dot_color);
    tk.draw_top(canvas, status_txt, dot_x + 16.0, 26.0, 22.0, status_color);
    if !snap.clock_text.is_empty() {
        tk.draw_top(canvas, &snap.clock_text, right - clock_w, 24.0, 24.0, TEXT_MAIN);
    }
    // 页眉下分隔线
    canvas.fill_rect(&Rect::new(HDR_X0, HDR_Y1 - 2, HDR_X1, HDR_Y1), LINE);
}

// ---------------------------------------------------------------------------
// SOC 主区（UI §5.1）
// ---------------------------------------------------------------------------

fn draw_soc(canvas: &mut dyn Canvas, tk: &TextKit, snap: &UiSnapshot) {
    let card = soc_card();
    canvas.fill_rect(&card, CARD);
    canvas.fill_rect(&Rect::new(card.x0, card.y0, card.x0 + 1, card.y1), LINE);

    // 顶 3px 状态描边：低/高/源失效 警示（UI §7.2 三态描边）
    let accent = match snap.soc {
        SocView::Value(v) => match soc_band(v) {
            SocBand::Low => SOC_RED,
            SocBand::High => SOC_ORANGE,
            SocBand::Mid => CARD,
        },
        SocView::Lost => SOC_RED,
    };
    canvas.fill_rect(&Rect::new(card.x0, card.y0, card.x1, card.y0 + 3), accent);

    // 标题 + 源标签
    tk.draw_top(canvas, "储能电池 SOC", card.x0 as f32 + 16.0, card.y0 as f32 + 8.0, SOC_TITLE_FONT, TEXT_MAIN);
    draw_source_capsule(canvas, tk, card, snap.soc_source);

    // 大数值（数字 148px + % 56px；区间色；Lost → "--" 降级灰）
    let center_x = (card.x0 + card.x1) as f32 / 2.0;
    let value_str: String = match snap.soc {
        SocView::Value(v) => format!("{}", v.round() as i64),
        SocView::Lost => "--".to_string(),
    };
    let value_color = match snap.soc {
        SocView::Value(v) => match soc_band(v) {
            SocBand::Low => SOC_RED,
            SocBand::High => SOC_ORANGE,
            SocBand::Mid => SOC_CYAN,
        },
        SocView::Lost => DEGRADED,
    };
    let w_big = tk.measure(&value_str, SOC_VALUE_FONT);
    let w_pct = tk.measure("%", SOC_PCT_FONT);
    let gap = 14.0f32;
    let total = w_big + gap + w_pct;
    let x0 = center_x - total / 2.0;
    tk.draw_top(canvas, &value_str, x0, SOC_VALUE_TOP as f32, SOC_VALUE_FONT, value_color);
    if matches!(snap.soc, SocView::Value(_)) {
        let pct_top = SOC_VALUE_TOP as f32 + (SOC_VALUE_FONT - SOC_PCT_FONT) / 2.0;
        tk.draw_top(canvas, "%", x0 + w_big + gap, pct_top, SOC_PCT_FONT, TEXT_SUB);
    }

    // 量程条（0-15 红 / 15-85 青 / 85-100 橙；UI §5.1）
    let bar_x0 = center_x as i32 - SOC_RANGE_W / 2;
    let mut seg = |a: i32, b: i32, c: Color| {
        let x0 = bar_x0 + (a as f32 * SOC_RANGE_W as f32 / 100.0) as i32;
        let x1 = bar_x0 + (b as f32 * SOC_RANGE_W as f32 / 100.0) as i32;
        canvas.fill_rect(&Rect::new(x0, SOC_RANGE_TOP, x1, SOC_RANGE_TOP + SOC_RANGE_H), c);
    };
    match snap.soc {
        SocView::Value(_) => {
            seg(0, 15, SOC_RED);
            seg(15, 85, SOC_CYAN);
            seg(85, 100, SOC_ORANGE);
        }
        SocView::Lost => canvas.fill_rect(
            &Rect::new(bar_x0, SOC_RANGE_TOP, bar_x0 + SOC_RANGE_W, SOC_RANGE_TOP + SOC_RANGE_H),
            CAP_BG,
        ),
    }
    // 值刻线（白三角指在当前值）
    if let SocView::Value(v) = snap.soc {
        let vv = v.clamp(0.0, 100.0);
        let mx = bar_x0 + (vv as f32 * SOC_RANGE_W as f32 / 100.0) as i32;
        fill_triangle_down(canvas, mx, SOC_RANGE_TOP - 8, 8, 5, WHITE);
    }
    // 刻度 0/100
    tk.draw_top(canvas, "0", bar_x0 as f32, (SOC_RANGE_TOP + SOC_RANGE_H + 4) as f32, 20.0, TEXT_WEAK);
    tk.draw_top(
        canvas,
        "100",
        (bar_x0 + SOC_RANGE_W - tk.measure("100", 20.0) as i32) as f32,
        (SOC_RANGE_TOP + SOC_RANGE_H + 4) as f32,
        20.0,
        TEXT_WEAK,
    );

    // 过期角标（保留最近值 + 不冒充实时）
    if snap.fresh == Freshness::Stale {
        let y = SOC_RANGE_TOP + SOC_RANGE_H + 30;
        tk.draw_top(canvas, "数据过期", card.x0 as f32 + 16.0, y as f32, 22.0, AMBER);
    }
}

fn draw_source_capsule(canvas: &mut dyn Canvas, tk: &TextKit, card: Rect, src: SocSource) {
    let (label, bg, border, fg) = match src {
        SocSource::Bms | SocSource::PcsReg1010 => (
            src.display_name(),
            CAP_BG,
            CAP_BORDER,
            TEXT_SUB,
        ),
        SocSource::Lost => ("SOC 源失效", 0x35_16_1A, SOC_RED, WHITE),
    };
    let font = 20.0f32;
    let h = 28i32;
    let w = tk.measure(label, font) as i32 + 20;
    let x0 = card.x1 - w - 10;
    let y0 = card.y0 + 8;
    let cap = Rect::new(x0, y0, x0 + w, y0 + h);
    canvas.fill_rect(&cap, bg);
    canvas.fill_rect(&Rect::new(cap.x0, cap.y0, cap.x0 + 1, cap.y1), border);
    canvas.fill_rect(&Rect::new(cap.x1 - 1, cap.y0, cap.x1, cap.y1), border);
    canvas.fill_rect(&Rect::new(cap.x0, cap.y0, cap.x1, cap.y0 + 1), border);
    canvas.fill_rect(&Rect::new(cap.x0, cap.y1 - 1, cap.x1, cap.y1), border);
    tk.draw_top(canvas, label, x0 as f32 + 10.0, (y0 + 2) as f32, font, fg);
}

// ---------------------------------------------------------------------------
// PCS 运行状态主区（UI §5.2）
// ---------------------------------------------------------------------------

fn run_palette(rs: Option<RunState>) -> Color {
    match rs {
        Some(RunState::Charge) => CHARGE_GREEN,
        Some(RunState::Discharge) => DISCHARGE_BLUE,
        Some(RunState::Stop) => STOP_GRAY,
        Some(RunState::Standby) => STANDBY_YELLOW,
        None => DEGRADED,
    }
}

fn draw_pcs(canvas: &mut dyn Canvas, tk: &TextKit, snap: &UiSnapshot) {
    let card = pcs_card();
    canvas.fill_rect(&card, CARD);
    // 左 6px 状态条 + 顶 3px（语义色，UI §5.2 四态三重冗余）
    let sem = run_palette(snap.run_state);
    canvas.fill_rect(&Rect::new(card.x0, card.y0, card.x0 + 6, card.y1), sem);
    canvas.fill_rect(&Rect::new(card.x0, card.y0, card.x1, card.y0 + 3), sem);

    tk.draw_top(canvas, "PCS 运行状态", card.x0 as f32 + 20.0, card.y0 as f32 + 8.0, PCS_TITLE_FONT, TEXT_MAIN);
    // 右上 REG1013 弱注
    let reg = "REG1013";
    let reg_x = card.x1 - tk.measure(reg, 20.0) as i32 - 14;
    tk.draw_top(canvas, reg, reg_x as f32, (card.y0 + 10) as f32, 20.0, TEXT_WEAK);

    let cx = (card.x0 + card.x1) as f32 / 2.0;
    // 状态字 + 图标（水平整体居中）
    let word = match snap.run_state {
        Some(rs) => rs.display_name().to_string(),
        None => "PCS 离线".to_string(),
    };
    let word_w = tk.measure(&word, PCS_WORD_FONT);
    let icon_w = 64i32;
    let gap = 18.0f32;
    let total = icon_w as f32 + gap + word_w;
    let start_x = cx - total / 2.0;
    // 图标
    let icon_cx = (start_x + icon_w as f32 / 2.0) as i32;
    let icon_top = PCS_WORD_TOP + 28;
    match snap.run_state {
        Some(RunState::Charge) => fill_triangle_down(canvas, icon_cx, icon_top, 44, 26, CHARGE_GREEN),
        Some(RunState::Discharge) => fill_triangle_up(canvas, icon_cx, icon_top, 44, 26, DISCHARGE_BLUE),
        Some(RunState::Stop) => canvas.fill_rect(
            &Rect::new(icon_cx - 20, icon_top, icon_cx + 20, icon_top + 44),
            STOP_GRAY,
        ),
        Some(RunState::Standby) => {
            canvas.fill_rect(&Rect::new(icon_cx - 20, icon_top, icon_cx - 4, icon_top + 44), STANDBY_YELLOW);
            canvas.fill_rect(&Rect::new(icon_cx + 4, icon_top, icon_cx + 20, icon_top + 44), STANDBY_YELLOW);
        }
        None => fill_disc_outline(canvas, icon_cx, icon_top + 22, 20, DEGRADED),
    }
    // 状态字（离线 → 降级灰；否则语义色）
    let word_color = run_palette(snap.run_state);
    tk.draw_top(canvas, &word, start_x + icon_w as f32 + gap, PCS_WORD_TOP as f32, PCS_WORD_FONT, word_color);

    // 佐证行（UI §5.2 / §7.3）：inconsistency → 品红胶囊；否则 ΣP 汇总
    if snap.run_state.is_some() {
        let sub_y = PCS_SUB_TOP as f32;
        if snap.inconsistency {
            let label = "方向不一致";
            let w = tk.measure(label, 22.0) as i32 + 24;
            let x0 = cx as i32 - w / 2;
            canvas.fill_rect(&Rect::new(x0, sub_y as i32, x0 + w, sub_y as i32 + 30), 0x3A_12_2E);
            canvas.fill_rect(&Rect::new(x0, sub_y as i32, x0 + w, sub_y as i32 + 1), PINK);
            canvas.fill_rect(&Rect::new(x0, sub_y as i32 + 29, x0 + w, sub_y as i32 + 30), PINK);
            canvas.fill_rect(&Rect::new(x0, sub_y as i32, x0 + 1, sub_y as i32 + 30), PINK);
            canvas.fill_rect(&Rect::new(x0 + w - 1, sub_y as i32, x0 + w, sub_y as i32 + 30), PINK);
            tk.draw_top(canvas, label, cx - w as f32 / 2.0, sub_y + 2.0, 22.0, PINK);
        } else {
            let p_text = match snap.p_total.value() {
                Some(v) => format!("方向一致  ΣP = {:.1} kW", v),
                None => "方向一致".to_string(),
            };
            let w = tk.measure(&p_text, 22.0);
            tk.draw_top(canvas, &p_text, cx - w / 2.0, sub_y + 4.0, 22.0, TEXT_SUB);
        }
    }

    // 过期角标
    if snap.fresh == Freshness::Stale && snap.run_state.is_some() {
        tk.draw_top(canvas, "数据过期", card.x1 as f32 - tk.measure("数据过期", 22.0) - 16.0, (card.y0 + 40) as f32, 22.0, AMBER);
    }
}

// ---------------------------------------------------------------------------
// 三相四卡（UI §5.3）
// ---------------------------------------------------------------------------

fn draw_panel(canvas: &mut dyn Canvas, tk: &TextKit, snap: &UiSnapshot) {
    // 面板标题行
    tk.draw_top(canvas, "三相输出 A/B/C", PANEL_X0 as f32, 454.0, 26.0, TEXT_MAIN);
    let hint = "单位 kW/A · 方向随 PCS 状态";
    let hint_x = PANEL_X0 as f32 + 260.0;
    tk.draw_top(canvas, hint, hint_x, 460.0, 20.0, TEXT_WEAK);

    let run = snap.run_state;
    let arrow_up = matches!(run, Some(RunState::Discharge));
    let arrow_down = matches!(run, Some(RunState::Charge));

    let cards = phase_cards();
    // A/B/C/总
    for (i, card) in cards.iter().enumerate() {
        canvas.fill_rect(card, CARD);
        canvas.fill_rect(&Rect::new(card.x0, card.y0, card.x0 + 1, card.y1), LINE);
        canvas.fill_rect(&Rect::new(card.x0, card.y0, card.x1, card.y0 + 1), LINE);
        let head = match i {
            0 => "A 相",
            1 => "B 相",
            2 => "C 相",
            _ => "总有功",
        };
        tk.draw_top(canvas, head, card.x0 as f32 + 14.0, (card.y0 + 6) as f32, PH_HDR_FONT, TEXT_MAIN);
        if i == 3 {
            tk.draw_top(canvas, "1032", (card.x1 - 14) as f32 - tk.measure("1032", 18.0), (card.y0 + 14) as f32, 18.0, TEXT_WEAK);
        }

        // 数值：0..3 = A/B/C 相取 p_phase[i]；第 4 张「总」卡取 p_total
        // （p_phase 仅 3 元素——曾在此越界 panic，测试 `render_frame_is_1024x768` 覆盖）。
        let pv: &NumView = if i < 3 { &snap.p_phase[i] } else { &snap.p_total };
        let dot = live_dot_for(pv, snap.fresh);
        let arrow_kind: Option<bool> = if i < 3 {
            if arrow_up {
                Some(true)
            } else if arrow_down {
                Some(false)
            } else {
                None
            }
        } else {
            None // 总卡无方向箭头
        };
        let iv = if i < 3 { Some(&snap.i_phase[i]) } else { None };

        draw_card_value(
            canvas,
            tk,
            card,
            pv,
            "有功功率",
            "kW",
            PH_P_LABEL_TOP,
            PH_P_VALUE_TOP,
            PH_P_FONT,
            dot,
            arrow_kind,
        );
        if let Some(iv) = iv {
            draw_card_value(
                canvas,
                tk,
                card,
                iv,
                "电流",
                "A",
                PH_I_LABEL_TOP,
                PH_I_VALUE_TOP,
                PH_I_FONT,
                dot,
                None,
            );
        }
        // 总卡底部标注
        if i == 3 {
            tk.draw_top(canvas, "设备总有功", card.x0 as f32 + 14.0, (card.y1 - 34) as f32, 20.0, TEXT_WEAK);
        }
    }
}

/// 卡内一行「标签 + 主值 + 单位(+可选方向箭头)」。降级 → `--` + 角标。
#[allow(clippy::too_many_arguments)]
fn draw_card_value(
    canvas: &mut dyn Canvas,
    tk: &TextKit,
    card: &Rect,
    nv: &NumView,
    label: &str,
    unit: &str,
    label_top: i32,
    value_top: i32,
    value_font: f32,
    _dot: crate::state::LiveDot,
    arrow_up: Option<bool>,
) {
    let m = 14i32;
    let x = card.x0 + m;
    // 标签 + 单位
    tk.draw_top(canvas, label, x as f32, (card.y0 + label_top) as f32, 20.0, TEXT_SUB);
    let unit_color = if nv.is_degraded() { DEGRADED } else { TEXT_SUB };

    // 数值统一用主文本色（等宽数字防抖由字体保证）；方向语义由箭头色表达——
    // UI §2.3 要求不依赖单一颜色信道，降级态由 `--` + 角标承担。
    let text = match nv {
        NumView::Value(v) => format!("{:.1}", v),
        NumView::Dash(_) => "--".to_string(),
    };

    // 数值前缀：方向箭头（▲放 / ▼充，色=语义色）
    let mut xcursor = x;
    if let Some(is_up) = arrow_up {
        let color = if is_up { DISCHARGE_BLUE } else { CHARGE_GREEN };
        let icon_cx = xcursor + 15;
        let icon_top = card.y0 + value_top + 18;
        if is_up {
            fill_triangle_up(canvas, icon_cx, icon_top, 22, 13, color);
        } else {
            fill_triangle_down(canvas, icon_cx, icon_top, 22, 13, color);
        }
        xcursor += 40;
    }
    tk.draw_top(canvas, &text, xcursor as f32, (card.y0 + value_top) as f32, value_font, TEXT_MAIN);
    // 单位
    let unit_x = xcursor + tk.measure(&text, value_font) as i32 + 8;
    let unit_top = card.y0 + value_top + (value_font as i32 - PH_UNIT_FONT as i32) + 2;
    tk.draw_top(canvas, unit, unit_x as f32, unit_top as f32, PH_UNIT_FONT, unit_color);
    // 降级角标（未取数/源离线/数据异常）
    if let NumView::Dash(flag) = nv {
        let badge = dash_badge(*flag);
        let bw = tk.measure(badge, 18.0);
        tk.draw_top(
            canvas,
            badge,
            (card.x1 - m) as f32 - bw,
            (card.y0 + value_top - 4) as f32,
            18.0,
            TEXT_WEAK,
        );
    }
}

// ---------------------------------------------------------------------------
// 几何图标原语（三重冗余中的图标信道；纯几何，不依赖字形）
// ---------------------------------------------------------------------------

fn fill_disc(canvas: &mut dyn Canvas, cx: i32, cy: i32, r: i32, color: Color) {
    for y in (cy - r)..=(cy + r) {
        for x in (cx - r)..=(cx + r) {
            let dx = (x - cx) as i64;
            let dy = (y - cy) as i64;
            if dx * dx + dy * dy <= (r as i64) * (r as i64) {
                canvas.set_px(x, y, color);
            }
        }
    }
}

fn fill_disc_outline(canvas: &mut dyn Canvas, cx: i32, cy: i32, r: i32, color: Color) {
    for y in (cy - r - 1)..=(cy + r + 1) {
        for x in (cx - r - 1)..=(cx + r + 1) {
            let d = (((x - cx).pow(2) + (y - cy).pow(2)) as f64).sqrt();
            if (d - r as f64).abs() <= 1.2 {
                canvas.set_px(x, y, color);
            }
        }
    }
}

/// 上三角 ▲（顶点在下，能量向上送出 = 放电意象；UI §2.3/§6.2：放=蓝）。
fn fill_triangle_up(canvas: &mut dyn Canvas, cx: i32, top: i32, h: i32, half_base: i32, color: Color) {
    for yy in 0..h {
        // 顶部宽 → 底部窄（顶点在下）
        let halfw = ((half_base as f32) * (1.0 - (yy as f32 / h as f32))) as i32;
        let row_top = top + yy;
        let row_bot = top + yy + 1;
        for xx in (cx - halfw)..=(cx + halfw) {
            canvas.fill_rect(&Rect::new(xx, row_top, xx + 1, row_bot), color);
        }
    }
}

/// 下三角 ▼（顶点在上，能量向下存入电池 = 充电意象；UI：充=绿）。
fn fill_triangle_down(canvas: &mut dyn Canvas, cx: i32, top: i32, h: i32, half_base: i32, color: Color) {
    for yy in 0..h {
        let f = yy as f32 / h as f32;
        let halfw = ((half_base as f32) * f) as i32;
        let row_top = top + yy;
        let row_bot = top + yy + 1;
        for xx in (cx - halfw)..=(cx + halfw) {
            canvas.fill_rect(&Rect::new(xx, row_top, xx + 1, row_bot), color);
        }
    }
}

// ---------------------------------------------------------------------------
// 离屏渲染测试（设计 §10「离屏渲染测试」）：无 HDMI / 无 framebuffer / 无中文字库均可跑。
// 断言方式 = 关键区域「像素存在/颜色落区」，字形细节不做断言（ASCII 回退与真字库皆可过）。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::OffscreenCanvas;
    use crate::state::{NumView, SocView, UiSnapshot};
    use mupc_display_proto::FieldFlag;

    fn snap_live() -> UiSnapshot {
        UiSnapshot {
            mode: ScreenMode::Live,
            fresh: Freshness::Fresh,
            soc: SocView::Value(65.0),
            soc_source: SocSource::Bms,
            run_state: Some(RunState::Charge),
            pcs_online: true,
            inconsistency: false,
            p_phase: [NumView::Value(12.3); 3],
            p_total: NumView::Value(36.1),
            i_phase: [NumView::Value(22.5); 3],
            clock_text: "12:00:00".to_string(),
        }
    }

    /// 用 ASCII 回退字体渲染（本机无中文字库 → 中文占位盒，布局仍然成立）。
    fn draw(snap: &UiSnapshot) -> OffscreenCanvas {
        render_frame(&TextKit::fallback(), snap)
    }

    #[test]
    fn render_frame_is_1024x768() {
        let cv = draw(&snap_live());
        assert_eq!((cv.width(), cv.height()), (SCREEN_W as u32, SCREEN_H as u32));
    }

    #[test]
    fn background_cleared_and_cards_filled_at_ui_coordinates() {
        let cv = draw(&snap_live());
        // 画布底 = #0B1220（UI §6.1）
        assert_eq!(cv.pixel(5, 5), Some(BG));
        // SOC/PCS 主区底色 = 卡色（UI §4.3 坐标）
        assert!(cv.count_color(&soc_card(), CARD) > 0, "SOC 主区应铺卡底");
        assert!(cv.count_color(&pcs_card(), CARD) > 0, "PCS 主区应铺卡底");
        // 四卡（A/B/C/总）各自铺底
        for (i, c) in phase_cards().iter().enumerate() {
            assert!(cv.count_color(c, CARD) > 0, "第 {i} 张卡应铺卡底");
        }
        // 页眉分隔线（LINE）落在 HDR_Y1-2
        assert!(cv.count_color(&Rect::new(HDR_X0, HDR_Y1 - 2, HDR_X1, HDR_Y1), LINE) > 0);
    }

    #[test]
    fn soc_value_and_range_bar_drawn_in_main_area() {
        let cv = draw(&snap_live());
        // 大数字（"65"）落在 SOC 数值区（ASCII 回退可画数字 → 必须有字形像素）
        assert!(
            cv.has_non_background(&soc_value_region(), CARD),
            "SOC 大数字应绘制在数值区（区域应含非卡底像素）"
        );
        // 量程条三段（0-15 红 / 15-85 青 / 85-100 橙，UI §5.1）
        let bar_y = SOC_RANGE_TOP..SOC_RANGE_TOP + SOC_RANGE_H;
        let bar_x = SOC_X0 + (SOC_X1 - SOC_X0 - SOC_RANGE_W) / 2;
        let seg = |a: i32, b: i32| {
            Rect::new(
                bar_x + a * SOC_RANGE_W / 100 + 1,
                bar_y.start,
                bar_x + b * SOC_RANGE_W / 100 - 1,
                bar_y.end,
            )
        };
        assert!(cv.count_color(&seg(0, 15), SOC_RED) > 0, "低段应红");
        assert!(cv.count_color(&seg(15, 85), SOC_CYAN) > 0, "中段应青");
        assert!(cv.count_color(&seg(85, 100), SOC_ORANGE) > 0, "高段应橙");
    }

    #[test]
    fn soc_band_drives_accent_and_value_color() {
        // 低 SOC → 顶部 3px 警示描边红 + 数值红
        let mut s = snap_live();
        s.soc = SocView::Value(8.0);
        let cv = draw(&s);
        let accent = Rect::new(SOC_X0, SOC_Y0, SOC_X1, SOC_Y0 + 3);
        assert!(cv.count_color(&accent, SOC_RED) > 0, "≤15 顶部描边应红");
        assert!(cv.count_color(&soc_value_region(), SOC_RED) > 0, "≤15 数值应红");

        // 高 SOC → 橙
        let mut s = snap_live();
        s.soc = SocView::Value(92.0);
        let cv = draw(&s);
        let accent = Rect::new(SOC_X0, SOC_Y0, SOC_X1, SOC_Y0 + 3);
        assert!(cv.count_color(&accent, SOC_ORANGE) > 0, "≥85 顶部描边应橙");
        assert!(cv.count_color(&soc_value_region(), SOC_ORANGE) > 0, "≥85 数值应橙");
    }

    #[test]
    fn soc_lost_shows_dash_and_warning_capsule_no_value_color() {
        let mut s = snap_live();
        s.soc = SocView::Lost;
        s.soc_source = SocSource::Lost;
        let cv = draw(&s);
        // 顶部描边红（源失效警示，UI §7.2）
        let accent = Rect::new(SOC_X0, SOC_Y0, SOC_X1, SOC_Y0 + 3);
        assert!(cv.count_color(&accent, SOC_RED) > 0, "源失效应有红色警示描边");
        // `--` 用降级灰（绝不显示 0.0）
        assert!(
            cv.count_color(&soc_value_region(), DEGRADED) > 0,
            "双源皆失应画 `--` 降级灰"
        );
        // 量程条转为空槽（CAP_BG），不再有青/橙实心段
        let bar_x = SOC_X0 + (SOC_X1 - SOC_X0 - SOC_RANGE_W) / 2;
        let bar = Rect::new(bar_x, SOC_RANGE_TOP, bar_x + SOC_RANGE_W, SOC_RANGE_TOP + SOC_RANGE_H);
        assert!(cv.count_color(&bar, CAP_BG) > 0, "源失效量程条应为空槽");
    }

    #[test]
    fn pcs_semantic_bar_matches_run_state() {
        let cases = [
            (Some(RunState::Charge), CHARGE_GREEN, "充电"),
            (Some(RunState::Discharge), DISCHARGE_BLUE, "放电"),
            (Some(RunState::Stop), STOP_GRAY, "停机"),
            (Some(RunState::Standby), STANDBY_YELLOW, "待机"),
            (None, DEGRADED, "离线"),
        ];
        for (rs, color, name) in cases {
            let mut s = snap_live();
            s.run_state = rs;
            let cv = draw(&s);
            // 左侧 6px 状态条 + 顶部 3px 由语义色铺满（UI §5.2 三重冗余）
            let bar = Rect::new(PCS_X0, PCS_Y0, PCS_X0 + 6, PCS_Y1);
            assert!(
                cv.count_color(&bar, color) > 0,
                "{name}: PCS 左状态条应为语义色"
            );
            let top = Rect::new(PCS_X0, PCS_Y0, PCS_X1, PCS_Y0 + 3);
            assert!(cv.count_color(&top, color) > 0, "{name}: PCS 顶描边应为语义色");
        }
    }

    #[test]
    fn phase_cards_render_value_text_in_each_card() {
        let cv = draw(&snap_live());
        for (i, card) in phase_cards().iter().enumerate() {
            // 卡内数值行（P 值 64px）应有字形像素（数字 → ASCII 回退可画）
            let row = Rect::new(card.x0 + 14, card.y0 + PH_P_VALUE_TOP, card.x1 - 14, card.y0 + PH_P_VALUE_TOP + 60);
            assert!(
                cv.has_non_background(&row, CARD),
                "第 {i} 卡的有功数值行应绘制数字"
            );
        }
    }

    #[test]
    fn direction_arrow_only_in_phase_cards_and_follows_run_state() {
        // 放电 → ▲ 蓝（卡片区出现 DISCHARGE_BLUE 几何箭头；总卡无箭头）
        let mut s = snap_live();
        s.run_state = Some(RunState::Discharge);
        let cv = draw(&s);
        let a = phase_cards()[0];
        assert!(
            cv.count_color(&a, DISCHARGE_BLUE) > 0,
            "放电时 A 相应有 ▲ 蓝色箭头"
        );
        assert!(
            cv.count_color(&phase_cards()[3], DISCHARGE_BLUE) == 0,
            "总卡不应有方向箭头"
        );

        // 充电 → ▼ 绿
        let mut s = snap_live();
        s.run_state = Some(RunState::Charge);
        let cv = draw(&s);
        assert!(cv.count_color(&phase_cards()[0], CHARGE_GREEN) > 0, "充电时 A 相应有 ▼ 绿色箭头");
        assert!(cv.count_color(&phase_cards()[3], CHARGE_GREEN) == 0, "总卡不应有方向箭头");

        // 停机 → 卡内无箭头（语义色只出现在 PCS 卡，不在四卡）
        let mut s = snap_live();
        s.run_state = Some(RunState::Stop);
        let cv = draw(&s);
        let a = phase_cards()[0];
        assert_eq!(cv.count_color(&a, CHARGE_GREEN), 0, "停机时四卡不应有充电绿箭头");
        assert_eq!(cv.count_color(&a, DISCHARGE_BLUE), 0, "停机时四卡不应有放电蓝箭头");
    }

    #[test]
    fn degraded_field_renders_dash_not_zero() {
        let mut s = snap_live();
        s.p_phase[1] = NumView::Dash(FieldFlag::Offline);
        let cv = draw(&s);
        let b = phase_cards()[1];
        // B 相数值行仍应有绘制（`--` + 角标），且降级灰出现在值行
        assert!(cv.has_non_background(&Rect::new(b.x0 + 14, b.y0 + PH_P_VALUE_TOP, b.x1 - 14, b.y0 + PH_P_VALUE_TOP + 60), CARD));
        assert!(
            cv.count_color(&Rect::new(b.x0, b.y0 + PH_P_VALUE_TOP, b.x1, b.y0 + PH_P_VALUE_TOP + 70), DEGRADED) > 0,
            "降级字段应画降级灰 `--`"
        );
    }

    #[test]
    fn total_card_has_no_current_row() {
        // UI §5.3：总卡只画 P（无 I 行）。断言「电流」标签带（PH_I_LABEL_TOP..PH_I_VALUE_TOP）
        // 在 A 相有内容、在总卡为纯卡底（避开总卡底部 y1-34 的「设备总有功」脚注）。
        let cv = draw(&snap_live());
        let band = |c: &Rect| {
            Rect::new(
                c.x0 + 14,
                c.y0 + PH_I_LABEL_TOP,
                c.x1 - 14,
                c.y0 + PH_I_VALUE_TOP,
            )
        };
        assert!(
            cv.has_non_background(&band(&phase_cards()[0]), CARD),
            "A 相应绘制电流标签/数值"
        );
        assert!(
            !cv.has_non_background(&band(&phase_cards()[3]), CARD),
            "总卡不应绘制电流行（设备总有功卡无 I）"
        );
    }

    #[test]
    fn inconsistency_shows_magenta_badge_in_pcs_area() {
        let mut s = snap_live();
        s.inconsistency = true;
        let cv = draw(&s);
        let sub = Rect::new(PCS_X0, PCS_SUB_TOP, PCS_X1, PCS_SUB_TOP + 32);
        assert!(cv.count_color(&sub, PINK) > 0, "方向不一致应画品红警示胶囊");
    }

    #[test]
    fn stale_frame_shows_expiry_badge_but_keeps_values() {
        let mut s = snap_live();
        s.fresh = Freshness::Stale;
        let cv = draw(&s);
        // 保留数值（不冒充实时：值仍在，另加「数据过期」琥珀标）
        assert!(cv.has_non_background(&soc_value_region(), CARD), "过期仍保留最近值");
        let badge = Rect::new(SOC_X0, SOC_RANGE_TOP + SOC_RANGE_H + 30, SOC_X1, SOC_RANGE_TOP + SOC_RANGE_H + 56);
        assert!(cv.count_color(&badge, AMBER) > 0, "应画琥珀「数据过期」标");
        // 页眉状态点转琥珀
        let hdr = Rect::new(HDR_X0, HDR_Y0, HDR_X1, HDR_Y1);
        assert!(cv.count_color(&hdr, AMBER) > 0, "页眉应提示数据过期");
    }

    #[test]
    fn channel_down_and_init_are_full_screen_states_without_cards() {
        // 通道断：整屏琥珀提示，无数据卡（判定区不铺卡底）
        let mut s = snap_live();
        s.mode = ScreenMode::ChannelDown;
        let cv = draw(&s);
        let mid = Rect::new(0, 200, SCREEN_W, 460);
        assert!(cv.count_color(&mid, AMBER) > 0, "通道断应画琥珀提示");
        assert_eq!(cv.count_color(&soc_card(), CARD), 0, "通道断不应画 SOC 卡底");
        assert_eq!(cv.count_color(&pcs_card(), CARD), 0, "通道断不应画 PCS 卡底");

        // 初始化中：仅文本，无卡
        let mut s = snap_live();
        s.mode = ScreenMode::Init;
        let cv = draw(&s);
        assert!(cv.has_non_background(&mid, BG), "初始化中应有提示文本");
        assert_eq!(cv.count_color(&soc_card(), CARD), 0, "初始化中不应画 SOC 卡底");
    }

    #[test]
    fn render_never_panics_without_cjk_font_and_masks_chinese_as_boxes() {
        // 关键回归：本机无中文字库时，全链路渲染不得 panic，且中文落到占位盒（区域非空）。
        let cv = draw(&snap_live());
        let hdr = Rect::new(HDR_X0, HDR_Y0, HDR_X1, HDR_Y1);
        assert!(cv.has_non_background(&hdr, BG), "页眉中文标题应绘制（回退占位盒）");
    }
}
