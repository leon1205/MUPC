//! # `ui/pages` —— 6 页布局（12-MUPC v2.0 工作单元 **B2**）
//!
//! 设计 §5.1 的 `ui/pages/` 子树：`p1_status.rs` … `p6_system.rs`。本文件承载**两页共用的**
//! 契约与私有助手（页面本身不重复造）。
//!
//! ## 本轮的交付范围（B2a = 两页）
//!
//! | 文件 | 页 | 状态 |
//! |------|----|------|
//! | [`p1_status`] | P1 主状态页（默认页 / 超时回归目标页） | **B2a** |
//! | `p2_config` / `p3_logs` / `p4_interlock` / `p5_audit` | 配置 / 日志 / 安全联锁 / 审计 | B2b / B2c |
//! | [`p6_system`] | P6 系统 / 关于页 | **B2a** |
//!
//! **本轮不做**：页面路由与底部导航装配（B2c）、`console.rs` / `main.rs` 改写与 `state.rs`
//! 扩展（B3）。
//!
//! ## 页面装配契约（**B2c / B3 须照此接线**）
//!
//! 1. **页面根**是一支 [`ScrollContainer`]（= `lv_obj` + `SCROLLABLE` + `scroll_dir = VER` +
//!    `scrollbar_mode = AUTO`），尺寸恰为 [`Dimens::CONTENT_W`] × [`Dimens::CONTENT_H`]，
//!    自身坐标 `(0, 0)`（相对其父）。**摆放由调用方负责**（B2c 把根容器放在
//!    `(SIDE_PAD, HEADER_H)`；页眉/底部导航是应用外壳的事，不在 `ui/pages`）。
//!    因此页内坐标 = UI 设计 §6.1 的**绝对坐标 − (16, 72) + 上内边距 8**。
//! 2. **页面不持有数据源、不读时钟**：外部把「帧 + 通道态 + 新鲜度」打成 [`PageInput`] 注入
//!    [`p1_status::P1StatusPage::render`] / [`p6_system::P6SystemPage::render`]。
//!    降级（`--` / 「未提供」/「不可用」）与过期/断连**只由注入值驱动** ⇒ 离屏可确定性复现。
//! 3. **P1/P6 都是只读页**：不注册任何写通道回调、不产生任何控制通道请求（写操作在 P2/P4）。
//! 4. **数值一律经 `theme`**：页内不出现裸色值 / 裸字号 / 裸触摸尺寸。页**专属的栅格常量**
//!    （卡高 / 列宽等 `theme` 未收录者，出处为 UI §6.1）集中在本模块与各页文件顶部的 `const`
//!    块，且**逐条由 theme 常量推导**（见各常量注释）。
//!
//! ## ⚠️ 已知偏差登记（B2a 规格评审后；**集中、显式** —— 屏文 / 尺寸与契约不一致处
//! 一律在此列明，不得"悄悄地"不一致）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 计划收口单元 |
//! |---|----------------------|------|--------------|
//! | D1 | P1 页内**纵向坐标整体比 UI §6.1 的绝对坐标低约 40 px**（内容首卡在页内 y56 = 绝对 y120，UI 写 y80） | 页内仍保留「通道条」（通道断 / 未首连 / 数据过期）占去 32 + 16 px；UI §6.1 把这三种态画在页眉 / 整屏层 | **B2c**：外壳装配时移除页内通道条（与"通道断整屏降级"重复），页内 y 随之对齐 §6.1 |
//! | D2 | SOC 量程条**铺满卡内容宽**（424 px），UI §6.1 写 `(x58,y348,458,368) 360×20`（左右各缩进 58/42） | 设计还要求量程条下有 `0`/`100` 刻度，实际排布取"三段等比铺满 + 刻度两端对齐"；薄层无渐变通道，三段用并列色块表达（见 `p1_status` 文件头） | **B2c**（若 PM 要求逐像素对齐 §6.1） |
//! | D3 | 相卡实算高 **234**（内容 200 + 上下面 34），UI §6.1 写「各 236×236」 | §1.1.2 字号 / §3.5 栅格无 2 px 档，未为凑 2 px 引入裸数值 | B2c（随 theme 缺口上收一并处理） |
//! | D4 | 字体 **cmap 缺 U+2715(✕) / U+275A(❚)**；`font_subset_charset.txt` **缺** `-` `(` `)` `服务` `环` `管理` `为` `是` `℃` `°` `天` | 字库资产（`gen_fonts.sh` + `extract_charset.py`）本轮按 PM 裁定**不动**（避免反复重生成） | **B2c 之后**：六页文案齐备时一次性扩 §3.6 / charset 并重跑 `gen_fonts.sh`；随后把被改写的屏文**改回契约原文**（现存变体：PCS 待机图标取 `○`、占位符 `--`→`–`、`℃`→`C`、`本机服务地址（仅回环）`→`本机监听地址 · 仅本机`、`设备管理 IP`→`装置 IP 地址`、控制源长句取 `·` 变体） |
//! | D5 | `format_uptime` 的「N **天** HH:MM:SS」→ 现「N **日** HH:MM:SS」：`天`(U+5929) **不在 cmap 内**（`font_subset_charset.txt` 未收该字），真机上是豆腐块；改取**在 cmap 内**且同义的 `日`(U+65E5)（`1 日` = 1 天） | 字库资产本轮按 PM 裁定**不动**（见 D4）；用**在 cmap 内的等价词**改写屏文，语义不变 | **B2c 之后**（同 D4 一次性扩 §3.6 / charset 并重跑 `gen_fonts.sh`）**改回 `天`**。在此之前 `ui/tests.rs::ui_texts_covered_by_font_cmap` 的 `KNOWN_MISSING` 已**清空**（源码不再含缺字，自证见 B2a 收尾报告） |
//! | D6 | 告警时间取 **UTC**（`YYYY/MM/DD HH:MM:SS`），不随真机本地时区 | 页面不读时钟 / 不做时区决策（时区归渲染端 run/bin 层） | **B3**：本地时文本由状态层注入 |
//! | D7 | PCS **状态词槽位**的文案改为 **2 字**（`PCS` 语义由卡头 `PCS 运行状态` + 图标承担）：契约 `PCS 离线` → 现 `离线`；`状态未知` → 现 `未知` | **PM 裁定（B2a 收尾）**：状态词槽位只放 2 字（`停机`/`待机`/`充电`/`放电`/`离线`），与 UI 线框只画 2 字态的视觉节奏、及 UI §3.3「L1-大 = PCS 状态词 112 px」一致；`状态未知` 同取 2 字 `未知`。原 `PCS 离线`(实测 **458.1 px**) / `状态未知`(**448.0 px**) 在 112 px 档下远超词区 `484 − 2×17 − 72 − 16 = **362 px**`，`DOTS` 必截成「PCS 离…」——根因是 **UI §6.1 自身过约束**（其线框只画 2 字） | **UI §6.1 回改**：把「PCS 离线」明确为**状态词槽位 2 字 + PCS 由卡头 / 图标表达**，或按其线框订正 |
//!
//! 本轮**已修**因而不在此列：主行卡高 294 → **320**（评审 ②）、相卡降级语义只看 P（评审 ①）、
//! 码表走查的构造性漏判（评审 ③）、裸尺寸静态约束缺失（评审 ④）、SOC 阈值双份真源（评审 ⑤）。
//! （评审 ⑥ 的「文案回改契约原文」已由 **D7 的 PM 裁定**取代 —— 状态词槽位回到 2 字。）
//!
//! ## 与 `theme.rs` / `components.rs` 的分工（诚实标注）
//!
//! `components.rs` 的三个内部助手（`layout_box` / `decor` / `text_label`）是**私有**的，
//! `theme.rs` 也没有「页根样式（底色 + 零内边距）」与「页级栅格常量」。本轮**不改**
//! A1/B1 的交付物，故在本模块复刻了这三个 8 行助手（语义逐条对齐），并自建页级栅格常量。
//! 建议后续把二者上收（见 B2a 报告「theme / 组件库缺口」）。

use std::cell::Cell;
use std::rc::Rc;

use mupc_display_proto::DisplayFrame;

use crate::lvgl::obj::Obj;
use crate::lvgl::style::{Color, Style, StyleSelector};
use crate::lvgl::widgets::{Label, ScrollContainer};
use crate::lvgl::LvglError;
use crate::state::{ChannelStatus, Freshness};
use crate::ui::theme::{self, Dimens};

pub mod p1_status;
pub mod p6_system;

// ═══════════════════════════════════════════════════════════════════════════
// 1. 降级占位符（**与文档的逐字偏差见下**）
// ═══════════════════════════════════════════════════════════════════════════

/// 字段级降级占位符（PRD F1.4 / F3.4 / F4.3「显 `--`，**严禁补 0**」）。
///
/// ⚠️ **偏差（如实标注，见 B2a 报告）**：设计与 PRD 写的是 `--`（两个 **ASCII 连字符**
/// U+002D）。但 §3.6 声明的字符集与 `fonts/font_subset_charset.txt`（实测 326 字符）
/// **都不含 U+002D**（该表只有 `–` U+2013 与 `−` U+2212），实测 `lv_font_noto_sc_24.c`
/// 的字形表亦无 U+002D/U+0028/U+0029 ⇒ 照抄 `--` 在真机上是**两个豆腐块**。
/// 故此处取字符集内的 `–`（U+2013 短破折）。语义（"无值、不是 0"）不变。
pub const PLACEHOLDER: &str = "–";

/// 缺失文案（`InfoSection` 的 `build_time` / `model` / `serial` / `mgmt_ipv4` 为 `None`
/// 时；EDGE-16 / F8.4，**不臆造**）。
pub const MISSING: &str = "未提供";

/// 装置段（`DeviceSection`）`Option` 字段为 `None` 时的文案（=`不可得`，与
/// [`MISSING`] 的「未提供」区分：前者是**该拍没采到**，后者是**该字段根本没有**）。
pub const NOT_READ: &str = "未取数";

// ═══════════════════════════════════════════════════════════════════════════
// 2. 页面输入（状态层 → 页面的唯一数据入口）
// ═══════════════════════════════════════════════════════════════════════════

/// 一页的渲染输入：**帧 + 通道态 + 新鲜度**（三者都由状态层派生，页面不自行判断）。
///
/// - `frame = None`：尚无任何有效帧（通道 `Init` / 首连失败）⇒ 全字段降级；
/// - [`ChannelStatus`]：`Init` / `Connected` / `Down`（`Down` = 无成功 GET > 3 s，
///   设计 §3.5 条 2）⇒ 页内通道条显示「与主进程数据通道断开」；
/// - [`Freshness`]：`Stale` = `now − frame.ts_ms > 2 s`（PRD F5.3）⇒ 打「数据过期」标。
///
/// ⚠️ **`Freshness` 由调用方（B3 接线 / `state.rs`）注入**：页面**不读时钟**，
/// 否则离屏用例无法确定性复现过期态（设计 §11.1 离屏渲染用例的前提）。
#[derive(Debug, Clone, Copy)]
pub struct PageInput<'a> {
    /// 最近一帧（`None` = 尚未收到任何有效帧）。
    pub frame: Option<&'a DisplayFrame>,
    /// 跨进程数据通道态（设计 §5.4 / §3.5 条 2）。
    pub channel: ChannelStatus,
    /// 单帧新鲜度（PRD F5.3）。
    pub freshness: Freshness,
}

impl<'a> PageInput<'a> {
    /// 由三要素构造。
    pub const fn new(
        frame: Option<&'a DisplayFrame>,
        channel: ChannelStatus,
        freshness: Freshness,
    ) -> Self {
        Self {
            frame,
            channel,
            freshness,
        }
    }

    /// 实时正常态（通道通 + 帧新鲜）—— 也是最常见的生产路径。
    pub const fn live(frame: &'a DisplayFrame) -> Self {
        Self {
            frame: Some(frame),
            channel: ChannelStatus::Connected,
            freshness: Freshness::Fresh,
        }
    }

    /// 初始化态（尚无帧）。
    pub const fn init() -> Self {
        Self {
            frame: None,
            channel: ChannelStatus::Init,
            freshness: Freshness::Fresh,
        }
    }

    /// 通道断态（保留最近帧 → 打「冻结」；无帧则全降级）。
    pub const fn down(frame: Option<&'a DisplayFrame>) -> Self {
        Self {
            frame,
            channel: ChannelStatus::Down,
            freshness: Freshness::Fresh,
        }
    }
}

/// `PageInput` 里帧的四个 v2 分节（缺帧时取契约缺省 —— 与 `#[serde(default)]` 同语义，
/// 即「不可用」，**不伪装成正常**）。
pub(crate) fn sections(input: &PageInput<'_>) -> (
    mupc_display_proto::DeviceSection,
    mupc_display_proto::AlarmsSection,
    mupc_display_proto::InfoSection,
) {
    match input.frame {
        Some(f) => (f.device.clone(), f.alarms.clone(), f.info.clone()),
        None => Default::default(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 页面私有助手（与 `components.rs` 的同名私有助手语义逐条对齐）
// ═══════════════════════════════════════════════════════════════════════════

/// 建一个透明布局容器（行 / 列 / 居中壳）：**零内边距**（LVGL 默认主题会给裸 `lv_obj`
/// 加底色与内边距，不显式置空会把设计坐标撑歪）。
pub(crate) fn layout_box(parent: &Obj, w: i32, h: i32) -> Result<Obj, LvglError> {
    let o = Obj::create(parent)?;
    o.set_size(w, h);
    o.add_style(&theme::transparent(), StyleSelector::main());
    o.remove_flag(crate::lvgl::obj::ObjFlag::SCROLLABLE);
    Ok(o)
}

/// 建一个纯装饰块（色条 / 分隔线 / 状态点）：不可点、不可滚。
pub(crate) fn decor(parent: &Obj, w: i32, h: i32, style: &Rc<Style>) -> Result<Obj, LvglError> {
    let o = Obj::create(parent)?;
    o.set_size(w, h);
    o.add_style(style, StyleSelector::main());
    o.remove_flag(crate::lvgl::obj::ObjFlag::CLICKABLE);
    o.remove_flag(crate::lvgl::obj::ObjFlag::SCROLLABLE);
    Ok(o)
}

/// 建一个空文本标签（字号 + 颜色都来自 `theme`；后续用 [`Label::set_text`] 更新）。
pub(crate) fn label(parent: &Obj, slot: theme::TextSlot, color: Color) -> Result<Label, LvglError> {
    let l = Label::create_with_text(parent, "")?;
    l.add_style(&theme::text(slot, color), StyleSelector::main());
    l.remove_flag(crate::lvgl::obj::ObjFlag::CLICKABLE);
    l.remove_flag(crate::lvgl::obj::ObjFlag::SCROLLABLE);
    Ok(l)
}

/// 建一个带初值的文本标签。
pub(crate) fn text_label(
    parent: &Obj,
    text: &str,
    slot: theme::TextSlot,
    color: Color,
) -> Result<Label, LvglError> {
    let l = label(parent, slot, color)?;
    l.set_text(text);
    Ok(l)
}

/// 建页面根（纵向滚动容器，`CONTENT_W × CONTENT_H`，自身 `(0,0)`）—— 见模块文档契约 1。
///
/// 底色**不由页面画**（应用外壳的屏/页容器负责 `Palette::BG`）；页根透明，故不遮蔽外壳。
pub(crate) fn page_root(parent: &Obj) -> Result<ScrollContainer, LvglError> {
    let root = ScrollContainer::create(parent)?;
    root.set_size(Dimens::CONTENT_W, Dimens::CONTENT_H);
    root.set_pos(0, 0);
    root.add_style(&theme::transparent(), StyleSelector::main());
    Ok(root)
}

/// 把 `styles[next]` 挂到对象上、摘掉上一次挂的那个 —— 用于「值变化只改颜色」这类
/// **无界增长防护**（`Obj::add_style` 只增不删；1 Hz 每拍挂一条新样式必然把 LVGL 内存
/// 吃光）。`cur` 由调用方持有，初值用 `usize::MAX`（表示"尚未挂过"）。
pub(crate) fn set_style_index(
    obj: &Obj,
    styles: &[Rc<Style>],
    cur: &Cell<usize>,
    next: usize,
) {
    if styles.is_empty() {
        return;
    }
    let next = next.min(styles.len() - 1);
    let prev = cur.get();
    if prev == next {
        return;
    }
    if let Some(old) = styles.get(prev) {
        obj.remove_style(old, StyleSelector::main());
    }
    obj.add_style(&styles[next], StyleSelector::main());
    cur.set(next);
}

/// 显示 / 隐藏（`hidden` 的对象不参与布局与命中 ⇒ 对**绝对定位**的页内元素无副作用）。
pub(crate) fn set_visible(obj: &Obj, visible: bool) {
    obj.set_hidden(!visible);
}

/// 从一组候选对象里只显示第 `idx` 个（越界则全部隐藏）。
pub(crate) fn show_only(objs: &[&Obj], idx: Option<usize>) {
    for (i, o) in objs.iter().enumerate() {
        set_visible(o, Some(i) == idx);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 纯逻辑：时间戳格式化（可独立单测；**不触碰 LVGL**）
// ═══════════════════════════════════════════════════════════════════════════

/// Unix 毫秒 → `YYYY/MM/DD HH:MM:SS`（**UTC**）。
///
/// 用途：P1 告警行的时间列。**为什么是 `/` 而不是 `-`**：§3.6 的字符集与字体子集都不含
/// ASCII `-`（见 [`PLACEHOLDER`]），用 `-` 会出豆腐块；`/` 与 `:` 都在集合内。
///
/// **为什么 UTC 而非本地时**：本地时区需要 `TZ`/libc 环境，且设计把「时钟」归**渲染端
/// run/bin 层 / 状态层**（`UiSnapshot.clock_text` 的先例），不在页面里做时区决策。此处只
/// 提供**确定性**的 UTC 文本，真机若要求本地时，应由状态层把文本注入（见 B2a 报告）。
///
/// 算法：以「儒略日序号」为中介（Howard Hinnant 的 `civil_from_days`，**无外部依赖**）。
pub fn format_epoch_ms_utc(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    // 日内秒（`rem_euclid` 对负值也落在 [0, 86400)）。
    let sod = secs.rem_euclid(86_400);
    let days = (secs - sod) / 86_400; // 自 1970-01-01 起的天数（可负）
    let (h, m, s) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    let (y, mo, d) = civil_from_days(days);
    format!("{y:04}/{mo:02}/{d:02} {h:02}:{m:02}:{s:02}")
}

/// 自 1970-01-01 起的天数 → `(年, 月, 日)`（Howard Hinnant 的 `civil_from_days`）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 运行时长（秒）→ `N 日 HH:MM:SS` / `HH:MM:SS`。
///
/// ⚠️ **D5 偏差**：契约（UI §3.6 / 本函数原稿）作 `N 天 HH:MM:SS`，但 `天`（U+5929）
/// **不在生成字体的 cmap 内**（`fonts/lv_font_noto_sc_*.c` 由 `font_subset_charset.txt` 生成，
/// 该 txt 未收 `天`）⇒ 真机上是豆腐块。改取**同义且在 cmap 内**的 `日`（U+65E5，实测 cmap
/// 内），语义不变（`1 日` = 1 天）。收口见 `pages/mod.rs` 顶部登记表 D5。
pub fn format_uptime(secs: u64) -> String {
    let (d, rem) = (secs / 86_400, secs % 86_400);
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    if d > 0 {
        format!("{d} 日 {h:02}:{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 上屏固定文案（码表覆盖率走查的输入；UI §3.6 用字表是全屏真源）
// ═══════════════════════════════════════════════════════════════════════════

/// 本模块（两页 + 共享层）上屏的**全部固定文案**（**人的清册，不是覆盖率的基线**）。
///
/// ⚠️ **B2a 规格评审 ③ 订正**：码表覆盖率的**基线**是**生成字体的实际 cmap**
/// （`fonts/lv_font_noto_sc_*.c` 的 `unicode_list`），**待查集合**是**扫 `ui/**` 源码字面量**
/// —— 见 `ui/tests.rs::ui_texts_covered_by_font_cmap`（同处一并断言本清册的每个条目确实
/// 出现在源码字面量里，防止清册腐化）。**此前**以本清单 + `font_subset_charset.txt` 为基线，
/// 属"构造性漏判"：清单是手写的、且 `.txt` 自身缺字（实测缺 `天`，且它列的 `✕`/`❚` 已被
/// `lv_font_conv` 丢弃），所以"没抓到缺字"并不等于没问题。
///
/// **不含**帧内动态文本（告警消息、型号 / 序列号 / 管理 IP 等由 mupcd 提供，页面无从约束）。
pub const ALL_TEXTS: &[&str] = &[
    // 共享
    PLACEHOLDER,
    MISSING,
    NOT_READ,
    // P1 共享/通道条
    p1_status::TEXT_CHANNEL_DOWN,
    p1_status::TEXT_CHANNEL_CONNECTING,
    p1_status::TEXT_STALE,
    // P1 SOC 区
    p1_status::TEXT_SOC_TITLE,
    p1_status::TEXT_SOC_UNIT,
    p1_status::TEXT_SOC_SCALE_LOW,
    p1_status::TEXT_SOC_SCALE_HIGH,
    p1_status::TEXT_SOC_SRC_PCS,
    // P1 PCS 区
    p1_status::TEXT_PCS_TITLE,
    p1_status::TEXT_PCS_JUDGE,
    p1_status::TEXT_PCS_OFFLINE,
    p1_status::TEXT_PCS_UNKNOWN,
    p1_status::TEXT_PCS_CONSISTENT,
    p1_status::TEXT_PCS_INCONSISTENT,
    p1_status::TEXT_SIGMA_PREFIX,
    p1_status::TEXT_KW,
    p1_status::TEXT_A,
    // P1 三相区
    p1_status::TEXT_PHASE_A,
    p1_status::TEXT_PHASE_B,
    p1_status::TEXT_PHASE_C,
    p1_status::TEXT_PHASE_TOTAL,
    p1_status::TEXT_ACTIVE_POWER,
    p1_status::TEXT_DEVICE_TOTAL_POWER,
    p1_status::TEXT_CURRENT,
    // P1 装置状态区
    p1_status::TEXT_DEVICE_TITLE,
    p1_status::TEXT_FIRMWARE,
    p1_status::TEXT_BUILD_TIME,
    p1_status::TEXT_UPTIME,
    p1_status::TEXT_CPU_TEMP,
    p1_status::TEXT_MEM,
    p1_status::TEXT_LINK_IEC104,
    p1_status::TEXT_LINK_INTERCORE,
    p1_status::TEXT_CONTROL_SOURCE,
    p1_status::TEXT_PERCENT,
    p1_status::TEXT_AI_DISABLED,
    // P1 告警区
    p1_status::TEXT_ALARM_TITLE,
    p1_status::TEXT_ALARM_TIME,
    p1_status::TEXT_ALARM_LEVEL,
    p1_status::TEXT_ALARM_MESSAGE,
    p1_status::TEXT_ALARM_EMPTY,
    p1_status::TEXT_LEVEL_ERROR,
    p1_status::TEXT_LEVEL_WARN,
    p1_status::TEXT_LEVEL_INFO,
    // P6
    p6_system::TEXT_DEVICE_INFO,
    p6_system::TEXT_MODEL,
    p6_system::TEXT_SERIAL,
    p6_system::TEXT_FIRMWARE,
    p6_system::TEXT_BUILD_TIME,
    p6_system::TEXT_RUN_INFO,
    p6_system::TEXT_UPTIME,
    p6_system::TEXT_CPU_TEMP,
    p6_system::TEXT_MEM,
    p6_system::TEXT_LINK_IEC104,
    p6_system::TEXT_LINK_INTERCORE,
    p6_system::TEXT_CHANNEL,
    p6_system::TEXT_CONTROL_SOURCE,
    p6_system::TEXT_ABOUT,
    p6_system::TEXT_LOCAL_VERSION,
    p6_system::TEXT_SERVICE_ADDR,
    p6_system::TEXT_MGMT_IP,
    p6_system::TEXT_NO_REMOTE,
    p6_system::TEXT_AI_DISABLED,
];

#[cfg(test)]
mod tests {
    use super::*;

    // ── 时间戳格式化（纯逻辑；不触碰 LVGL ⇒ 可独立 `#[test]`）──

    #[test]
    fn epoch_formats_known_instants() {
        assert_eq!(format_epoch_ms_utc(0), "1970/01/01 00:00:00");
        // 2000-01-01T00:00:00Z = 946684800 s
        assert_eq!(format_epoch_ms_utc(946_684_800_000), "2000/01/01 00:00:00");
        // 2026-09-10T13:42:07Z = 1789047727 s（UI §6.5 线框里的示例时刻）
        assert_eq!(
            format_epoch_ms_utc(1_789_047_727_000),
            "2026/09/10 13:42:07"
        );
        // 闰日：2024-02-29T12:34:56Z = 1709210096 s
        assert_eq!(format_epoch_ms_utc(1_709_210_096_000), "2024/02/29 12:34:56");
        // 亚秒截断（不进位）
        assert_eq!(format_epoch_ms_utc(1_789_047_727_999), "2026/09/10 13:42:07");
    }

    #[test]
    fn uptime_formats_days_and_clock() {
        assert_eq!(format_uptime(0), "00:00:00");
        assert_eq!(format_uptime(59), "00:00:59");
        assert_eq!(format_uptime(3600 + 4 * 60 + 5), "01:04:05");
        assert_eq!(format_uptime(86_400 + 3 * 3600), "1 日 03:00:00");
        assert_eq!(format_uptime(2 * 86_400 + 23 * 3600 + 59 * 60 + 59), "2 日 23:59:59");
    }

    /// 占位符**必须是字符集内的字形**（`--` 会出豆腐块 —— 见 [`PLACEHOLDER`] 文档）。
    #[test]
    fn placeholder_is_not_ascii_hyphens() {
        assert_ne!(PLACEHOLDER, "--", "ASCII 连字符不在字体子集内");
        assert_eq!(PLACEHOLDER, "\u{2013}", "取 §3.6 字符集内的 U+2013");
    }
}
