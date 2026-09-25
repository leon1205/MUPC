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
//! | [`p2_config`] | P2 配置页（控制通道驱动，含写操作） | **B2b-2** |
//! | [`p4_interlock`] | P4 安全 / 联锁页（**帧驱动展示 + 控制通道意图**，含写操作） | **B2b-3** |
//! | [`p5_audit`] | P5 审计页（**控制通道驱动，只读**；F19） | **B2c-1** |
//! | [`filters`] | **共享**「时间范围筛选」件（P3 / P5 共用；`LogRange` 三档 + 自定义起止） | **B2c-1** |
//! | [`p3_logs`] | P3 日志页（**控制通道驱动，只读**；复用 [`filters`] 的时间范围件） | **B2c-2** |
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
//! ## 补充（**B2b-2**）：两类页的驱动方式与页根形态（**不改上面契约 1 / 2 的语义**）
//!
//! 上面的契约 1（"页面根是一支滚动容器"）与契约 2（"外部把帧 + 通道态 + 新鲜度打成
//! [`PageInput`] 注入"）是**为帧驱动页（P1 / P6）写的**，两者**原样保留**。
//! B2b-2 起的写操作页与之分属两类，逐条如下：
//!
//! - **契约 1′（页根形态，P2 不适用契约 1）**：UI §6.2 的线框在内容视口**底部**画了
//!   `Y624 ┌ 固定操作条（不随滚动）┐` ⇒ P2 的页根**不是**滚动容器，而是**普通容器**
//!   （`CONTENT_W × CONTENT_H`，自身 `(0,0)`），其内再分两层：上为**滚动视口**
//!   （`CONTENT_W × (CONTENT_H − 操作条高)`，`lv_obj` + `SCROLLABLE` +
//!   `scroll_dir = VER` + `scrollbar_mode = AUTO`，语义与契约 1 的根容器**逐条相同**，
//!   只是高度更矮）、下为**固定操作条**（`CONTENT_W × 72`，不随滚动）。
//!   `page_root()`（"页根自身即滚动容器"）的语义**未改**，P1 / P6 照旧；P2 自建视口
//!   （见 `p2_config::P2ConfigPage::new`）。
//! - **契约 2′（数据入口，P2 不适用契约 2）**：P2 的数据**不来自 1 Hz 显示帧**，而来自
//!   **控制通道**（`GET /v1/console/config`，另 9811 端口，设计 §3.4）。其注入入口是
//!   `p2_config::P2ConfigPage` 的 [`set_config`](p2_config::P2ConfigPage::set_config) /
//!   [`set_unavailable`](p2_config::P2ConfigPage::set_unavailable) /
//!   [`set_submitting`](p2_config::P2ConfigPage::set_submitting) /
//!   [`show_result`](p2_config::P2ConfigPage::show_result)，降级态**只由注入值驱动**
//!   （不读时钟、不自行发请求）。**P2 也不生成 `request_id`**：设计 §6.2 末行要求
//!   **确认完成前不发任何请求**；确认完成后本页经
//!   [`set_on_submit`](p2_config::P2ConfigPage::set_on_submit) 把 `ConfigPatch` 交回外部，
//!   `request_id`(uuid) + `issued_at_ms` 由 B3 的 `console.rs` 生成。
//! - **同类页面预告**：P5（审计）将由控制通道驱动，复用契约 2′。
//! - **B2b-3 补充（P4 = 两类驱动**混用**，两类契约各取一半）**：
//!   - **读路径走契约 2（[`PageInput`] 帧驱动）**：`InterlockSection` 是 1 Hz 显示帧的
//!     「慢拍 C（0.5 s）」分段（设计 §3.1）⇒ 展示态从 `input.frame.interlock` 取；`frame = None`
//!     ⇒ 契约缺省（`available = false`）⇒ 显「联锁状态不可用」，**绝不可**回落「未联锁」
//!     （IL-01.6 / UI §8.3 专行）。
//!   - **写路径走契约 2′（控制通道意图）**：**不发请求、不生成 `request_id`**；确认完成后经
//!     [`set_on_release`](p4_interlock::P4InterlockPage::set_on_release) /
//!     [`set_on_ack_m1`](p4_interlock::P4InterlockPage::set_on_ack_m1) 把
//!     `InterlockOpPayload`（UI **观测到**的 latch + 源名，EDGE-19 的乐观并发检查）交回外部；
//!     回执经 [`show_result`](p4_interlock::P4InterlockPage::show_result) 灌回。
//!   - **页根形态复用契约 1′**（UI §6.4 线框 `Y624 ┌ 固定操作条 ──┐`）；与 P2 的唯一差别：
//!     P4 在滚动视口与固定操作条之间多一条 **24 px 就地原因带**（UI §6.4 拒绝原因表 +
//!     §8.3 都要求「按钮正上方 24 px 就地原因」）⇒ 视口 528 而非 552，见该文件的偏差 **IL4**。
//! - **B2c-1 补充（P5 审计页 + 共享「时间范围」件）**：
//!   - **P5 = 纯契约 1（页根即滚动容器）+ 契约 2′（控制通道注入）**：UI §6.5 线框**没有**底部
//!     固定操作条 ⇒ 走 [`page_root`]；数据经 `set_page` / `set_ops` / `set_unavailable` 注入，
//!     **只读**（零写操作），筛选变化与「加载更多」只经意图回调
//!     （[`p5_audit::P5AuditPage::set_on_query`] / [`set_on_load_more`](p5_audit::P5AuditPage::set_on_load_more)）
//!     交回外部。
//!   - **共享件落点 = [`filters`]**：P3（B2c-2）与本页共用「时间范围」三档 + 自定义起止；
//!     P3 的接法写在 `filters.rs` 的模块文档里（含"档位变化后必须用 `filters::body_h` 重摆
//!     后续区块"这一条 —— 事件回调内读 `size()` 会拿到旧值）。
//!   - **两处结构性发现（已逐条登记）**：`AuditPage` **没有** `range_too_large` 字段
//!     （EDGE-15 在 P5 侧生产不可达，见 **AU5**）；薄层 `EventCode` **未镜像**
//!     `LV_EVENT_SCROLL` ⇒「滚动加载」的触发点在本层不可得（见 **AU6**）。
//! - **B2c-2 补充（P3 日志页）**：与 P5 **同型**（契约 1 的页根 + 契约 2′ 的控制通道注入 +
//!   只读 + 筛选意图回调），差异逐条：
//!   - **超限条是常驻构件**（P3 的契约 `LogPage.range_too_large` 是**必需字段** ⇒ 该态
//!     生产可达，与 P5 的 **AU5** 相反）；
//!   - **无「源不可用」态**（§6.3 明写：日志通道断由**通道条**表达）⇒ 页内**不建**
//!     `UnavailableState`，列表区只有「行 / 空态」两态；
//!   - **模块筛选是换行 chip 网格**（§6.3 ② / §2.7 禁横滚）——维度名独占一行、网格铺满
//!     内容宽（见 `p3_logs.rs` **LG3**）；
//!   - **增量拉取**以 [`p3_logs::P3LogsPage::request_increment`] 为触发入口（B3 的 500 ms
//!     节拍），意图载荷 `cursor` = **已见最大 `seq`**（不是 `next_cursor`，见该文件文档）；
//!   - **具名薄层缺口（R2，B4a 已收窄）**：`EventCode` **未镜像** `LV_EVENT_SCROLL` ⇒
//!     §6.3 的「手动上滚 ⇒ 停止自动滚动 + 浮现"回到最新"」在本层**结构性不可实现**。
//!     **【B4a 订正】** 原文续写「`allowlist.txt` 无任何滚动位置读 / 写 API
//!     （`lv_obj_get_scroll_y` / `lv_obj_scroll_to_y` 皆无）」—— **已过期**：两个符号均已放行
//!     且已在薄层封装（`Obj::scroll_to_y` / `Obj::scroll_y`，`lvgl/mod.rs` 的 **G3**）
//!     ⇒ 剩余缺口**只有输入侧事件**；
//!     本页只做恒显按钮 + 点击意图，缺口逐条列在 `p3_logs.rs` 的 **R2**。
//! - **B2c-3 补充（应用外壳装配，`ui/shell.rs`）**：
//!   - **页根摆放**：外壳把每个页根放在 `(Dimens::SIDE_PAD, Dimens::HEADER_H)`（相对外壳根），
//!     与契约 1 / 1′ 的「尺寸 `CONTENT_W × CONTENT_H`、自身 `(0,0)`、摆放由调用方负责」逐条一致；
//!     6 页**一次全部建好**、靠 `HIDDEN` 显隐切换（§5.3 的自建容器路线，**不用** `lv_tabview`
//!     —— 外壳要"一次建 6 页、常驻不销毁"，而 `lv_tabview` 的页生命周期归它自己管）。
//!   - **数据入口仍是契约 2 / 2′**：外壳**不替页做数据决策**，只把页句柄交出去
//!     （`Shell::p1()` … `Shell::p6()`）⇒ 真实数据源接线属 **B3**。
//!   - **两条新增的外壳级注入**（**B2c-3 时**页侧无生产可见查询口，见 `shell.rs` 偏差
//!     **SH2**；**弹层那一条已由 B3-2c 闭合** —— 真源改为页面的 `P2ConfigPage::dialog_open`
//!     / `P4InterlockPage::dialog_open`，接线层每拍取或喂入；注入位本身保留）：
//!     「是否有确认弹层打开」（暂停空闲计时）与「触摸设备是否可用」（EDGE-13 角标）；
//!     P2 的「未保存修改」**不**经注入 —— 外壳每拍读生产可见的
//!     [`p2_config::P2ConfigPage::is_dirty`]，提示条的「放弃修改」直接调
//!     [`p2_config::P2ConfigPage::discard_draft`]。
//!
//! ## ⚠️ 已知偏差登记（B2a 规格评审后；**集中、显式** —— 屏文 / 尺寸与契约不一致处
//! 一律在此列明，不得"悄悄地"不一致）
//!
//! | # | 偏差（现状 ≠ 契约） | 原因 | 计划收口单元 |
//! |---|----------------------|------|--------------|
//! | D1 | P1 页内**纵向坐标整体比 UI §6.1 的绝对坐标低约 40 px**（内容首卡在页内 y56 = 绝对 y120，UI 写 y80） | 页内仍保留「通道条」（通道断 / 未首连 / 数据过期）占去 32 + 16 px；UI §6.1 把这三种态画在页眉 / 整屏层 | **B2c**：外壳装配时移除页内通道条（与"通道断整屏降级"重复），页内 y 随之对齐 §6.1 |
//! | D2 | SOC 量程条**铺满卡内容宽**（424 px），UI §6.1 写 `(x58,y348,458,368) 360×20`（左右各缩进 58/42） | 设计还要求量程条下有 `0`/`100` 刻度，实际排布取"三段等比铺满 + 刻度两端对齐"；薄层无渐变通道，三段用并列色块表达（见 `p1_status` 文件头） | **B2c**（若 PM 要求逐像素对齐 §6.1） |
//! | D3 | 相卡实算高 **234**（内容 200 + 上下面 34），UI §6.1 写「各 236×236」 | §1.1.2 字号 / §3.5 栅格无 2 px 档，未为凑 2 px 引入裸数值 | B2c（随 theme 缺口上收一并处理） |
//! | D4 | 字体 **cmap 缺 U+2715(✕) / U+275A(❚)**；`font_subset_charset.txt` **缺** `-` `(` `)` **`，`（全角逗号 U+FF0C）** `服务` `环` `管理` `为` `是` `℃` `°` `天` | 字库资产（`gen_fonts.sh` + `extract_charset.py`）本轮按 PM 裁定**不动**（避免反复重生成）。**注**：控制源原串 `AI 引擎已停用，本地策略引擎为默认下发源` 的**两个**原因各占一半 —— 全角逗号 `，`（本行）与 `为`（同在本表）；二者都不在 cmap 内 ⇒ 取 `·` 变体（见 [`TEXT_AI_DISABLED`]） | **B2c 之后**：六页文案齐备时一次性扩 §3.6 / charset 并重跑 `gen_fonts.sh`；随后把被改写的屏文**改回契约原文**（现存变体：PCS 待机图标取 `○`、占位符 `--`→`–`、`℃`→`C`、`本机服务地址（仅回环）`→`本机监听地址 · 仅本机`、`设备管理 IP`→`装置 IP 地址`、控制源长句取 `·` 变体） |
//! | D5 | `format_uptime` 的「N **天** HH:MM:SS」→ 现「N **日** HH:MM:SS」：`天`(U+5929) **不在 cmap 内**（`font_subset_charset.txt` 未收该字），真机上是豆腐块；改取**在 cmap 内**且同义的 `日`(U+65E5)（`1 日` = 1 天） | 字库资产本轮按 PM 裁定**不动**（见 D4）；用**在 cmap 内的等价词**改写屏文，语义不变 | **B2c 之后**（同 D4 一次性扩 §3.6 / charset 并重跑 `gen_fonts.sh`）**改回 `天`**。在此之前 `ui/tests.rs::ui_texts_covered_by_font_cmap` 的 `KNOWN_MISSING` 已**清空**（源码不再含缺字，自证见 B2a 收尾报告） |
//! | D6 | 告警时间取 **UTC**（`YYYY/MM/DD HH:MM:SS`），不随真机本地时区 | 页面不读时钟 / 不做时区决策（时区归渲染端 run/bin 层） | **B3**：本地时文本由状态层注入 |
//! | D7 | PCS **状态词槽位**的文案改为 **2 字**（`PCS` 语义由卡头 `PCS 运行状态` + 图标承担）：契约 `PCS 离线` → 现 `离线`；`状态未知` → 现 `未知` | **PM 裁定（B2a 收尾）**：状态词槽位只放 2 字（`停机`/`待机`/`充电`/`放电`/`离线`），与 UI 线框只画 2 字态的视觉节奏、及 UI §3.3「L1-大 = PCS 状态词 112 px」一致；`状态未知` 同取 2 字 `未知`。原 `PCS 离线`(实测 **458.1 px**) / `状态未知`(**448.0 px**) 在 112 px 档下远超词区 `484 − 2×17 − 72 − 16 = **362 px**`，`DOTS` 必截成「PCS 离…」——根因是 **UI §6.1 自身过约束**（其线框只画 2 字） | **UI §6.1 回改**：把「PCS 离线」明确为**状态词槽位 2 字 + PCS 由卡头 / 图标表达**，或按其线框订正 |
//! | D8 | P1 装置状态网格取 UI §6.1 的 **8 项**，**不含「数据通道」**（该行只在 P6 运行信息卡）；P1 的通道状态由页内通道条 + 页眉承担 | 设计 §6.1 的 P1 布局行没有「数据通道」格；「数据通道」归 P6 §6.6 的运行信息卡（**此前该口径只写在 `ui/tests.rs` 的用例注释里、未进本登记表** —— B2a 代码质量评审 M5 补齐） | 无（**有意与 UI 一致**） |
//! | D9 | **契约直上屏字符串经 [`display_safe`] 改写**：`-`/`_` → `–`、小写 → 大写同族（`1.0.0-rc1` → `1.0.0–RC1`）、其余未知 ASCII → `?`（如 `T`/`Z`） | 这些字符串来自 `display-proto`（**契约冻结，本轮不得改**）或运行时帧；ASCII 子集里 `-` `(` `)` `T` `Z` 及多数小写**没有字形**，直上屏即豆腐块（C1 同类缺陷，B2a 代码质量评审 ③）。替换保证"上屏字符 ⊆ cmap"，**非 ASCII 契约词原样透传**（由 `contract_strings_emit_only_cmap_glyphs` 逐字核对） | **根治 = 扩 §3.6 字符集**（把 `-` `(` `)` 与 `T`/`Z` 等纳入子集并重跑 `gen_fonts.sh`），**B2c 之后**与 D4/D5 一次性处理；随后可撤掉 `display_safe` 的 ASCII 改写（保留"⊆ cmap"断言）。**自由文本**（告警 `message`、`model`/`serial` 之外的通道名）不在本轮口径内 —— 待 B3 定"注入侧约束 or 显示侧过滤" |
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

use mupc_display_proto::{ControlSource, DisplayFrame, LinkState};

use crate::lvgl::obj::Obj;
use crate::lvgl::style::{Color, Style, StyleSelector};
use crate::lvgl::widgets::{Label, ScrollContainer};
use crate::lvgl::LvglError;
use crate::state::{ChannelStatus, Freshness};
use crate::ui::components::StatusChip;
use crate::ui::theme::{self, ChipSkin, Dimens, Palette, TextSlot};

pub mod filters;
pub mod p1_status;
pub mod p2_config;
pub mod p3_logs;
pub mod p4_interlock;
pub mod p5_audit;
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

/// 区块级**「冻结」角标**文案（UI §8.3 EDGE-03 原文用字：「保留最近有效帧并**每区块打
/// `冻结` 角标**」；逐字取自 §3.6 全屏用字表，**不新增上屏字**）。
///
/// **为什么要一个共享常量**（B3-2c）：EDGE-03 / EDGE-20 的「打标」落在**帧驱动的页**上
/// （`DeviceSection` / `InfoSection` / `InterlockSection` 所辖的各卡），而各页各自写一份
/// 字面量会让"同一语义两种写法"漂移。判据的**唯一真源**是 [`frame_mark`]。
pub const TEXT_FROZEN: &str = "冻结";

/// [`PageInput`] → 区块级**帧可信度标记**（EDGE-03 / EDGE-20 的「保留最近有效帧**并打标**」）。
///
/// # 判据（**唯一输入 = [`PageInput`] 的三要素**，与 P1 同源，不另立第二套）
///
/// | 通道态 | 新鲜度 | 有帧 | 结果 | 理由 |
/// |--------|--------|:----:|------|------|
/// | `Down` | 任意 | 有 | [`TEXT_FROZEN`] | EDGE-03：通道断 ⇒ **保留**最近有效帧（**不得**清成占位），并打「冻结」 |
/// | `Down` | 任意 | 无 | `None` | 数值本就是 `–` 占位，不是"冻结的旧值" ⇒ 打标即**谎报** |
/// | `Connected` | `Stale` | 有 | [`p1_status::TEXT_STALE`] | 与 P1 的「数据过期」角标**同一串**（§3.6 既有） |
/// | 其余（`Init` / `Fresh`） | — | — | `None` | 无标记可打 |
///
/// ⚠️ **通道断优先于帧旧**：两条**不并存**（与 P1 的 `render_channel` 同口径 —— 通道断已由
/// 「冻结」表达，再叠一条「数据过期」只是噪声）。
///
/// ⚠️ **打标 ≠ 清值**：本函数只回答"打哪个标"，由调用方**只切角标可见性** —— 数值一律
/// 沿用冻结帧（EDGE-03 / EDGE-20 的明文要求，也是操作者据以决策的那份数据）。
pub fn frame_mark(input: &PageInput<'_>) -> Option<&'static str> {
    match (input.channel, input.freshness, input.frame.is_some()) {
        (ChannelStatus::Down, _, true) => Some(TEXT_FROZEN),
        (ChannelStatus::Connected, Freshness::Stale, true) => Some(p1_status::TEXT_STALE),
        _ => None,
    }
}

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

// ── 区块级「冻结 / 数据过期」角标（EDGE-03 / EDGE-20；B3-2c）───────────────────

/// 角标宽（`2 × CHIP_MIN_W` = 192：最长文案 `数据过期` 4 字 × 24 px = 96 + 图标槽 32 = 128，
/// 留出字形宽度余量；与 P1 的「数据过期」胶囊**同一档**，不另立新尺寸）。
///
/// **单一真源**（B3-2c 整改 · 重要 4）：四张卡（P4 总态 / P4 触发源 / P6 装置信息 / P6 运行）
/// 的角标尺寸只有这一份 —— 此前 P4 与 P6 **各自写了一份逐字相同的表达式**。
pub(crate) const FROZEN_CHIP_W: i32 = 2 * Dimens::CHIP_MIN_W;

/// 角标 y（在卡头行内垂直居中；`STATUS_CHIP_H` 32 ⇒ 与卡头文字同顶 ⇒ 收敛到 0）。
///
/// 卡头行高 = `SectionTitle` 行高 + `GAP_MIN`（P4 / P6 **各自**持同名私有常量 `CARD_HEAD_H`，
/// 表达式与本行**逐字相同** —— 欲改须三处同改，见各页常量注释）。
pub(crate) const FROZEN_CHIP_Y: i32 = theme::center_offset(
    TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN,
    Dimens::STATUS_CHIP_H,
);

/// 建一张卡头右端的**区块级「冻结 / 数据过期」角标**（EDGE-03 / EDGE-20）。
///
/// **四处共用**（P4 的两张卡 + P6 的两张卡）—— 构造口径只有这一处：尺寸 / 皮肤 / 图标 /
/// 初始文案 / y 偏移。`x` 由调用方给（各卡头右端的可用段不同：P6 三卡贴右缘，P4 总态卡
/// 要避开 latch 胶囊）。**建好即隐藏**，运行期由各页 `render` **只切可见性 + 换文案**。
///
/// ⚠️ **为什么必须收口到这里**（B3-2c 整改 · 重要 4）：此前只有 P6 走这个构造点，P4 在
/// 构造处**内联复制**了 `StatusChip::new(.., FROZEN_CHIP_W, ICON_WARN, TEXT_FROZEN,
/// ChipSkin::WARNING)` 两次，而 `p6_system.rs` 的注释却声称"三处都由它统一口径" ——
/// 当时 `grep -rn frozen_chip src/` **只命中 `p6_system.rs`**，那句话是**不实陈述**。
/// 收口后，"同一构造点"才成立：**改本函数的返回值 ⇒ P4 与 P6 的断言同时红**。
///
/// **回归**：`ui/tests.rs::pages_chain` 的「四处角标同构造点」段（读回尺寸 / 皮肤 / 图标 /
/// 文案四项 + 源码哨）。
pub(crate) fn frozen_chip(parent: &Obj, x: i32) -> Result<StatusChip, LvglError> {
    let chip = StatusChip::new(
        parent,
        FROZEN_CHIP_W,
        p2_config::ICON_WARN,
        TEXT_FROZEN,
        ChipSkin::WARNING,
    )?;
    chip.set_pos(x, FROZEN_CHIP_Y);
    set_visible(chip.obj(), false);
    Ok(chip)
}

// ── 角标的**图标-only 形态**（P4 联锁总态卡的**不重叠替代布局**；IL29②）──────────

/// 图标-only 角标宽（`ICON_SM` = 28 px；**不带**「冻结 / 数据过期」文字）。
///
/// **为什么需要第二形态**（产品裁定 2026-09-25 / P4 的 **IL29②**）：P4 联锁总态卡按
/// UI §6.4.1「几何单一真源」只有 **488 px**（[`Dimens::BAND_CARD_W`]），卡内容区 454 px；
/// 而该卡头行还要放标题（`联锁状态` 4 字 × 28 = **112**）与 latch 胶囊（`CARD_STATUS_W` =
/// **236**）⇒ 「标题 + [`FROZEN_CHIP_W`](192) + 胶囊」= **540 > 454**，**必然与胶囊重叠**。
/// 取 28 px 图标形态后落在标题与胶囊之间的空槽（112 < x < 218），**三件互不重叠**。
///
/// **代价（如实登记）**：失去「冻结 / 数据过期」的**文字**区分 —— 改由图标通道承担
/// （[`frozen_mark_icon`]：⚠ / `!`，与 P1 的两个角标同源）。EDGE-03 的「**每区块**打标」
/// 语义**保留**（可见性判据仍是 [`frame_mark`] 单一真源）。
pub(crate) const FROZEN_BADGE_W: i32 = Dimens::ICON_SM;

/// 标记（[`frame_mark`] 的返回值）→ 图标-only 角标的**图标通道**字形。
///
/// **两个来源都是既有件**（不新增字形、不新增色值）：
/// `冻结` ⇒ [`p2_config::ICON_WARN`]（`⚠`，与 [`frozen_chip`] 的图标通道**同一取值**）；
/// `数据过期` ⇒ [`p2_config::ICON_FAIL`]（`!`，与 P1 的 `stale_chip` 同一取向 —— 该处亦是
/// `WARNING` 皮肤 + `!`）。**非 `冻结` 一律按「数据过期」**（[`frame_mark`] 的值域只有这两条）。
pub(crate) fn frozen_mark_icon(mark: &str) -> &'static str {
    if mark == TEXT_FROZEN {
        p2_config::ICON_WARN
    } else {
        p2_config::ICON_FAIL
    }
}

/// **图标-only** 的区块级「冻结 / 数据过期」角标（EDGE-03 / EDGE-20）。
///
/// 与 [`frozen_chip`] **同类但不共形**：那个是「图标 + 文字」的 192 px 胶囊（触发源卡 / P6
/// 两卡），本形态只有一支**可换字形的图标**（见 [`FROZEN_BADGE_W`] 的几何论证）。
/// **为什么不是 `StatusChip`**：`components.rs` 的 `StatusChip` **没有 `set_icon`**
/// （图标只能建时给）⇒ 无法在运行期按标记换 ⚠ / `!`；且其文字槽固定在 x=32（> 28 px 宽），
/// 在图标形态里恒被裁掉（多一个永不显示的对象）。故此处用「容器 + 单图标标签」两件实现，
/// 皮肤取 [`ChipSkin::WARNING`] 的**同一份样式**（圆角 / 底 / 描边 / 字色不新增真源）。
///
/// **建好即隐藏**，运行期由各页 `render` 只切可见性 + 换图标（[`FrozenBadge::set_mark`]）。
pub(crate) fn frozen_icon_badge(parent: &Obj, x: i32, y: i32) -> Result<FrozenBadge, LvglError> {
    let obj = layout_box(parent, FROZEN_BADGE_W, Dimens::STATUS_CHIP_H)?;
    obj.add_style(
        &ChipSkin::WARNING.style(),
        crate::lvgl::style::StyleSelector::main(),
    );
    obj.set_pos(x, y);
    let icon = text_label(
        &obj,
        frozen_mark_icon(TEXT_FROZEN),
        TextSlot::Body,
        ChipSkin::WARNING.text,
    )?;
    let icon_px = TextSlot::Body.px() as i32;
    icon.set_size(FROZEN_BADGE_W, icon_px);
    icon.set_pos(
        theme::center_offset(FROZEN_BADGE_W, icon_px),
        theme::center_offset(Dimens::STATUS_CHIP_H, icon_px),
    );
    set_visible(&obj, false);
    Ok(FrozenBadge {
        obj,
        icon: Rc::new(icon),
    })
}

/// 图标-only 角标的句柄（见 [`frozen_icon_badge`]）。
///
/// 与 `components.rs` 的 [`StatusChip`] 同族：只暴露**只读**读回口（行为断言用）与
/// **换标记**（热路径唯一允许的写操作）。
///
/// ⚠️ **皮肤不设读回口**：`skin()` 只被 `ui/tests.rs` 用到 ⇒ 在非 test 构建里会触发
/// `dead_code`（门禁是"零新增告警"）。该形态的皮肤由构造点 [`frozen_icon_badge`] 的
/// **源码哨**（`pages::frozen_icon_badge(` 计数）与 `ui/**` 零裸色值网共同兜住。
#[derive(Debug)]
pub(crate) struct FrozenBadge {
    obj: Obj,
    icon: Rc<Label>,
}

impl FrozenBadge {
    /// 底层容器（改位置 / 切可见性用）。
    pub(crate) fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 按标记换图标（⚠ / `!`）；**标记 → 字形**的唯一映射点见 [`frozen_mark_icon`]。
    pub(crate) fn set_mark(&self, mark: &str) {
        self.icon.set_text(frozen_mark_icon(mark));
    }

    /// 当前图标字形（**上屏那一枚**；断言口径 = [`frozen_mark_icon`]）。
    pub(crate) fn icon_text(&self) -> Option<String> {
        self.icon.text()
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
// 4′. 数值 → 文本（**唯一出口**；负号口径 = U+2212）
//
// **C1（安全相关）**：此前各页直接用 `format!("{v:.1}")` / `format!("{v:.0}")`，负值产出
// **ASCII `-`（U+002D）**；而生成字体的 cmap **没有该字形**（`unicode_list_0` 由 `0xb` 直跳
// `0xe`，即 U+002B(+) 有、U+002D(−) 无）⇒ P 反向（光伏倒送）时负号是**豆腐块**，负值看着
// 像正值；同一文件里 ΣP 又手写 `\u{2212}`，**口径自相矛盾**。
//
// ⇒ 所有"数值 → 文本"一律经本节三个 helper（**负号恒为 `\u{2212}`**），页面不得再出现
// `format!("{v:.N}")` 直排。**运行时产出**的字符集由
// `ui/tests.rs::runtime_formatters_emit_only_cmap_glyphs` **构造性**校验（含负值输入）。
// ═══════════════════════════════════════════════════════════════════════════

/// 1 位小数；负号一律 `\u{2212}`（U+2212，cmap 内有该字形；ASCII `-` 没有）。
///
/// 用 `is_sign_negative()`（而非 `< 0.0`）⇒ `-0.0` 也走负号分支，不会漏成 `-0.0`。
pub fn fmt_signed_1dp(v: f64) -> String {
    if v.is_sign_negative() {
        format!("\u{2212}{:.1}", -v)
    } else {
        format!("{v:.1}")
    }
}

/// 0 位小数（整数）；负号口径同 [`fmt_signed_1dp`]。
pub fn fmt_int0(v: f64) -> String {
    if v.is_sign_negative() {
        format!("\u{2212}{:.0}", -v)
    } else {
        format!("{v:.0}")
    }
}

/// **N 位小数**（U-73：小数位由 catalog 的 `decimals` 给出 —— 登记 `scale` 的派生值，
/// 屏侧不得自行决定，§15.3.2）；负号口径同 [`fmt_signed_1dp`]。
///
/// `decimals` 由 catalog 给出且登记表只取 0..=3（§15.3.2 的四值映射）⇒ 越界值 clamp 到 3
/// （**不 panic**、不生成长尾浮点串）。非有限值 ⇒ `--` 占位（**不造数**）。
///
/// **为什么必须走本函数而不是页面里 `format!("{v:.p$}")`**：ASCII `-`（U+002D）在生成字体
/// 里**没有字形**（C1 同族缺陷）⇒ 负号必须一律 `\u{2212}`。运行时产出字符集由
/// `ui/tests.rs::runtime_formatters_emit_only_cmap_glyphs` 构造性校验（含负值输入）。
pub fn fmt_decimals(v: f64, decimals: u8) -> String {
    if !v.is_finite() {
        return PLACEHOLDER.to_string();
    }
    let p = usize::from(decimals.min(3));
    if v.is_sign_negative() {
        format!("\u{2212}{:.p$}", -v, p = p)
    } else {
        format!("{v:.p$}", p = p)
    }
}

/// ΣP 佐证文本（`ΣP +36.9 kW` / `ΣP −36.9 kW`）—— 前缀与单位取自
/// [`p1_status::TEXT_SIGMA_PREFIX`] / [`p1_status::TEXT_KW`]，**不另抄一份字面量**。
///
/// 非负值带显式 `+`（"倒送"与"正放"一眼可辨）；负值带 `\u{2212}`。
pub fn fmt_sigma_kw(v: f64) -> String {
    let mag = fmt_signed_1dp(v);
    let signed = if v.is_sign_negative() {
        mag
    } else {
        format!("+{mag}")
    };
    format!(
        "{} {} {}",
        p1_status::TEXT_SIGMA_PREFIX,
        signed,
        p1_status::TEXT_KW
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// 4″. 契约字符串 → 上屏文本的**安全显示侧处理**
//
// **C1 残留（B2a 代码质量评审 ③）**：`InfoSection` 的 `firmware_version` / `build_time` /
// `model` / `serial` / `mgmt_ipv4` 与各 `*::display_name()`（`LinkState` / `RunState` …）是
// **契约字符串直上屏**：字面量不在 `ui/**`（在 `display-proto` 与运行时帧里）⇒ 既有的
// 「源码字面量走查」看不到它们，「运行时格式化器检查」也覆盖不到（它们不是 `format!("{v:.1}")`
// 那一类数值出口）。后果与 C1 同类：版本号含 `-`（`1.0.0-rc1`）或型号含 `-`（`BECG-3568`）时，
// ASCII `-`（U+002D）在生成字体里**没有字形** ⇒ 真机上是豆腐块。
//
// **处置（PM 裁定口径：不改契约 —— `display-proto` 冻结）**：在**显示侧**做安全替换，
// 保证"上屏字符 ⊆ cmap"，并把残余替换登记为偏差 **D9**（见下表 / 收口项）。
// ═══════════════════════════════════════════════════════════════════════════

/// 上屏安全的 **ASCII 字母表** = 生成字体 cmap 里**实际存在**的可打印 ASCII 子集
/// （从入库基线 `fonts/lv_font_cmap.txt` 实测导出，2026-09-11 对 10 档逐一核对）。
///
/// ⚠️ **这不是"第二份真源"**：`ui/tests.rs::contract_strings_emit_only_cmap_glyphs` 断言
/// 本串**每个字符都在入库基线 cmap 内**（码表变 ⇒ 本串不做数即红）。
pub const ASCII_DISPLAY_ALPHABET: &str = " !%+./0123456789:?ABCDEFGIMNOPRSUWhks";

/// 契约字符串 → 上屏文本：**保证输出字符全部在生成字体 cmap 内**（真机不出豆腐块）。
///
/// 逐字规则（**永不丢语义关键字符**：替换是"同族等价"，不是删除）：
///
/// | 输入 | 输出 | 理由 |
/// |------|------|------|
/// | 非 ASCII（中文契约词 / 全角） | 原样 | 语义载体，替换即失真；其字形由 `contract_strings_emit_only_cmap_glyphs` 逐字核对（`LinkState` / `RunState` 各变体） |
/// | 在 [`ASCII_DISPLAY_ALPHABET`] 内 | 原样 | 已有字形 |
/// | `-`(U+002D) / `_`(U+005F) | `–`(U+2013) | 版本号 / ISO 时间戳的**分隔符**；取同族短破折（cmap 内，与 [`PLACEHOLDER`] 同款字形） |
/// | 其余 ASCII | 先试**大写同族**（`rc1` → `RC1`，仍可读），仍不在表内则 `?` | 小写字母在 cmap 里几乎全缺、大写多数在；`?` 是"有字形但信息有限"的**最后兜底** |
///
/// **不**处理自由文本（告警 `message` / 通道名），它们不是契约枚举、内容不可预判
/// —— 该口径见 `pages/mod.rs` 顶部登记表收口项。
pub fn display_safe(text: &str) -> String {
    text.chars().map(display_safe_char).collect()
}

/// [`display_safe`] 的单字符规则（拆出便于逐条阅读 / 单测）。
///
/// ⚠️ **写法约束（有意为之）**：本函数是 `ui/**` 生产源码，会被
/// `ui/tests.rs::ui_texts_covered_by_font_cmap` 的字面量走查**逐字**检查 ⇒ 这里
/// **不得**出现 `'-'` / `"_"` 这类"缺字形字符"的**字符 / 字符串字面量**（会被判成
/// "源码用了 cmap 外的字"）。因此判据写成**码位数值**（`0x2D` / `0x5F`），替换目标
/// 则取 cmap 内的 `\u{2013}` / `?`。
fn display_safe_char(c: char) -> char {
    // 非 ASCII：原样（中文契约词由测试逐字核对，不在此改写字义）。
    if !c.is_ascii() {
        return c;
    }
    if ASCII_DISPLAY_ALPHABET.contains(c) {
        return c;
    }
    // U+002D 连字符 / U+005F 下划线 → U+2013 短破折（分隔语义不变）。
    if c as u32 == 0x2D || c as u32 == 0x5F {
        return '\u{2013}';
    }
    // 其余 ASCII：大写同族能上屏就用大写（`rc1` → `RC1`）。
    let up = c.to_ascii_uppercase();
    if up != c && ASCII_DISPLAY_ALPHABET.contains(up) {
        return up;
    }
    // 最后兜底：`?`（cmap 内，有字形）。
    '?'
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 上屏固定文案（码表覆盖率走查的输入；UI §3.6 用字表是全屏真源）
//
// **码表基线的读取顺序（I3，B2a 代码质量评审 ②）**：① 有 `fonts/lv_font_noto_sc_*.c` ⇒
// 用其**实际 cmap** 并断言与**入库清单** `fonts/lv_font_cmap.txt` 一致（漂移检测）；
// ② 无 `.c`（干净 clone / CI 常态）⇒ 用**入库清单**（**不再跳过** —— 此前依赖
// `cfg!(feature = "noto-font")`，而 CI 未启用该 feature ⇒ 两条最强的网在 CI 上恒被跳过）；
// ③ 两者皆缺（仓库损坏）⇒ 才跳过。见 `ui/tests.rs::load_font_cmap`。
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
    // 共享（B3-2c）：区块级「冻结」角标（EDGE-03；`数据过期` 复用 P1 的同名串）
    TEXT_FROZEN,
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
    // 两页共享（M3 上收；`p1_status::TEXT_AI_DISABLED` = `p6_system::TEXT_AI_DISABLED`
    // = 本模块的这一条，**同一份字面量**）
    TEXT_AI_DISABLED,
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
    // P4 安全 / 联锁页（B2b-3）
    p4_interlock::TEXT_CARD_STATE,
    p4_interlock::TEXT_STATE_LATCHED,
    p4_interlock::TEXT_STATE_UNLATCHED,
    p4_interlock::TEXT_STATE_UNAVAILABLE,
    p4_interlock::ICON_STATE_LATCHED,
    p4_interlock::ICON_STATE_UNLATCHED,
    p4_interlock::ICON_STATE_UNAVAILABLE,
    p4_interlock::ICON_FILLED,
    p4_interlock::ICON_HOLLOW,
    p4_interlock::TEXT_LATCH_HELD,
    p4_interlock::TEXT_LATCH_UNHELD,
    p4_interlock::TEXT_SOURCES_TITLE,
    p4_interlock::TEXT_SOURCES_EMPTY,
    p4_interlock::TEXT_SRC_TRIPPED,
    p4_interlock::TEXT_SRC_UNTRIPPED,
    p4_interlock::TEXT_SRC_ESTOP,
    p4_interlock::TEXT_SRC_DOOR,
    p4_interlock::TEXT_NAME_SEP,
    p4_interlock::TEXT_CLAUSE_SEP,
    p4_interlock::TEXT_CARD_STOP,
    p4_interlock::TEXT_CARD_FAULT_LAMP,
    p4_interlock::TEXT_CARD_RUN_LAMP,
    p4_interlock::TEXT_STOP_OK,
    p4_interlock::TEXT_STOP_FAIL,
    p4_interlock::TEXT_LAMP_ON,
    p4_interlock::TEXT_LAMP_OFF,
    p4_interlock::TEXT_LAMP_UNKNOWN,
    p4_interlock::TEXT_RELEASE,
    p4_interlock::TEXT_ACK_M1,
    p4_interlock::TEXT_AUDIT_NOTE,
    p4_interlock::TEXT_REASON_LATCHED,
    p4_interlock::TEXT_NOT_ENABLED,
    p4_interlock::TEXT_STOP_PENDING,
    p4_interlock::TEXT_CONFLICT,
    p4_interlock::TEXT_REJECT_SOURCES,
    p4_interlock::TEXT_REJECT_HOLD,
    p4_interlock::TEXT_HOLD_MORE,
    p4_interlock::TEXT_SECONDS,
    p4_interlock::TEXT_HOLD_NEED,
    p4_interlock::TEXT_STOP_PENDING_TRAIL,
    p4_interlock::TEXT_INTERNAL,
    p4_interlock::TEXT_OP_BUSY,
    p4_interlock::TEXT_DIALOG_TITLE_RELEASE,
    p4_interlock::TEXT_DIALOG_TITLE_ACK_M1,
    p4_interlock::TEXT_IMPACT_RELEASE,
    p4_interlock::TEXT_IMPACT_ACK_M1,
    p4_interlock::TEXT_DETAIL_SOURCES,
    p4_interlock::TEXT_DETAIL_LATCH,
    p4_interlock::TEXT_DETAIL_STOP,
    p4_interlock::TEXT_NONE,
    p4_interlock::TEXT_TOAST_RELEASED,
    p4_interlock::TEXT_TOAST_ACKED,
    p4_interlock::TEXT_TOAST_FAIL,
    p4_interlock::TEXT_AUDIT_UNAVAILABLE,
    p4_interlock::ICON_OK,
    p4_interlock::ICON_FAIL,
    // 共享「时间范围」筛选件（B2c-1；P3 / P5 共用）
    filters::TEXT_RANGE_LABEL,
    filters::TEXT_RANGE_H1,
    filters::TEXT_RANGE_H24,
    filters::TEXT_RANGE_CUSTOM,
    filters::TEXT_START,
    filters::TEXT_END,
    // P5 审计页（B2c-1）
    p5_audit::TEXT_NEWEST_PREFIX,
    p5_audit::TEXT_IMMUTABLE,
    p5_audit::TEXT_LOCK_ICON,
    p5_audit::TEXT_OPS_LABEL,
    p5_audit::TEXT_OPS_ALL,
    p5_audit::TEXT_HEAD_TIME,
    p5_audit::TEXT_HEAD_OPERATOR,
    p5_audit::TEXT_HEAD_OP,
    p5_audit::TEXT_HEAD_VALUE,
    p5_audit::TEXT_HEAD_RESULT,
    p5_audit::TEXT_OPERATOR_LOCAL,
    p5_audit::TEXT_RESULT_OK,
    p5_audit::TEXT_RESULT_FAIL,
    p5_audit::TEXT_REASON_PREFIX,
    p5_audit::TEXT_PAIR_ARROW,
    p5_audit::TEXT_LABEL_SEP,
    p5_audit::TEXT_CLAUSE_SEP,
    p5_audit::TEXT_VALUE_ON,
    p5_audit::TEXT_VALUE_OFF,
    p5_audit::TEXT_UNIT_ITEMS,
    p5_audit::TEXT_UNIT_FIELDS,
    p5_audit::TEXT_ELLIPSIS,
    p5_audit::TEXT_FOOTER_LOADING,
    p5_audit::TEXT_FOOTER_ALL,
    p5_audit::TEXT_EXPORT_NOTE,
    p5_audit::TEXT_EXPORT_NOTE2,
    p5_audit::TEXT_EMPTY,
    p5_audit::TEXT_RANGE_TOO_LARGE,
    p5_audit::TEXT_UNAVAILABLE,
    p5_audit::TEXT_FIELD_PORT,
    p5_audit::TEXT_FIELD_LOG_LEVEL,
    p5_audit::TEXT_FIELD_TELEMETRY,
    p5_audit::TEXT_EMPTY_ICON,
    // P3 日志页（B2c-2）
    p3_logs::TEXT_CHANNEL_OK,
    p3_logs::TEXT_CHANNEL_DOWN,
    p3_logs::TEXT_LEVEL_LABEL,
    p3_logs::TEXT_MODULE_LABEL,
    p3_logs::TEXT_ALL,
    p3_logs::TEXT_LEVEL_ERROR,
    p3_logs::TEXT_LEVEL_WARN,
    p3_logs::TEXT_LEVEL_INFO,
    p3_logs::TEXT_LEVEL_DEBUG,
    p3_logs::TEXT_HEAD_TIME,
    p3_logs::TEXT_HEAD_LEVEL,
    p3_logs::TEXT_HEAD_MODULE,
    p3_logs::TEXT_HEAD_MESSAGE,
    p3_logs::TEXT_EMPTY,
    p3_logs::TEXT_EMPTY_ICON,
    p3_logs::TEXT_RANGE_TOO_LARGE,
    // 超限下的中性文案（**LG9**：自造串 —— §3.6 无此句）
    p3_logs::TEXT_INCOMPLETE,
    p3_logs::TEXT_NO_EXPORT,
    p3_logs::TEXT_NO_EXPORT2,
    p3_logs::TEXT_CLAUSE_SEP,
    p3_logs::TEXT_FOOTER_LOADING,
    p3_logs::TEXT_FOOTER_ALL,
    p3_logs::TEXT_BACK_TO_LATEST,
    p3_logs::TEXT_MODULE_INTERCORE,
    p3_logs::TEXT_MODULE_GATEWAY,
    p3_logs::TEXT_MODULE_AUDIT,
    // 应用外壳（B2c-3）：页眉返回键 / 未保存提示条 / 通道胶囊 / 触摸不可用角标。
    crate::ui::shell::TEXT_BACK,
    crate::ui::shell::TEXT_DIRTY_BANNER,
    crate::ui::shell::TEXT_DISCARD,
    crate::ui::shell::ICON_WARN,
    crate::ui::shell::TEXT_CHANNEL_OK,
    crate::ui::shell::TEXT_TOUCH_UNAVAILABLE,
];

// ═══════════════════════════════════════════════════════════════════════════
// 6. 两页共享的小工具（B2a 代码质量评审 **M3**：此前 P1 / P6 **逐字重复 ~50 行**）
// ═══════════════════════════════════════════════════════════════════════════

/// 控制源固定文案的字符集内变体（⚠️ 偏差 2：原串 `AI 引擎已停用，本地策略引擎为默认下发源`
/// 含**全角逗号 `，` 与 `为`** —— 两者都不在 §3.6 字符集 / 生成字体 cmap 内 ⇒ 取 `·` 分隔
/// 并改述为「默认下发」。语义（AI 停用、本地策略引擎为默认下发源）不变）。
///
/// **两页共享**（M3）：`p1_status::TEXT_AI_DISABLED` / `p6_system::TEXT_AI_DISABLED` 都是
/// 本常量的转出别名，**只有这一份字面量**。
pub const TEXT_AI_DISABLED: &str = "AI 引擎已停用 · 本地策略引擎默认下发";

/// 指示灯五态顺序（与 `LedIndicator` 数组一一对应；`Unknown` 兜底在末位）。
///
/// **两页共享**（M3：此前 P1 / P6 各抄一份完全相同的数组）。
pub(crate) const LED_STATES: [LinkState; 5] = [
    LinkState::Connected,
    LinkState::Connecting,
    LinkState::Disconnected,
    LinkState::NotConfigured,
    LinkState::Unknown,
];

/// 链路态 → 灯色（PRD §3.1 指定色；`Unknown` 取"未配置"灰，**绝不落入"正常"绿** —— F6.5）。
pub(crate) fn link_color(state: LinkState) -> Color {
    match state {
        LinkState::Connected => Palette::LINK_OK,
        LinkState::Connecting => Palette::LINK_PENDING,
        LinkState::Disconnected => Palette::LINK_DOWN,
        LinkState::NotConfigured | LinkState::Unknown => Palette::LINK_UNCONFIGURED,
    }
}

/// 链路态 → 几何字形（F14 的"图标"通道）。
pub(crate) fn link_icon(state: LinkState) -> &'static str {
    match state {
        LinkState::Connected | LinkState::Connecting => "●",
        LinkState::Disconnected => "!",
        LinkState::NotConfigured | LinkState::Unknown => "○",
    }
}

/// 控制源 → 上屏文案（`AiDisabled` 取在 cmap 内的 [`TEXT_AI_DISABLED`] 变体）。
pub(crate) fn control_source_text(src: ControlSource) -> &'static str {
    match src {
        ControlSource::LocalStrategy => ControlSource::LocalStrategy.display_name(),
        ControlSource::AiDisabled => TEXT_AI_DISABLED,
        ControlSource::Unknown => ControlSource::Unknown.display_name(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6′. 页面层共用的栅格常量（M3：此前 `filters.rs` / `p5_audit.rs` **各抄一份**）
// ═══════════════════════════════════════════════════════════════════════════

/// 两个块的**紧缝**（`theme` 无 8 px 档 ⇒ 取 `GAP_MIN / 2` 的推导值）。
///
/// `pub(crate)`：`filters.rs`（时间范围件首行与自定义块之间）与 `p5_audit.rs`
/// （最近审计条 / 说明条 / 超限条 / 表头之间的缝）**共用同一份** —— 两处各写一遍
/// `Dimens::GAP_MIN / 2` 是"同一口径的第二份真源"（B2c-1 代码质量评审 **M3**）。
pub(crate) const TIGHT_GAP: i32 = Dimens::GAP_MIN / 2;

// ═══════════════════════════════════════════════════════════════════════════
// 6″. 回调槽的**重入安全**取用（B2c-1 代码质量评审 ③）
//
// 与 `ui/controls.rs`（I1 / I1′）**同一惯用法**，此处的三个槽（`filters::RangeSlot`、
// `p5_audit::QuerySlot` ×2）**类型不完全相同**（`Box<dyn FnMut(常量各异的载荷)>`），
// 故按 `T: ?Sized` 泛化后上收到本模块 —— 页面层不再各写一份"取出 → 调用 → 放回"。
//
// **为什么必须这么做（本单元修的 Important ③）**：原实现在调用用户回调期间**持有**
// `try_borrow_mut` 的可变借用 ⇒ 回调内再调 `set_on_change` 时那里的 `try_borrow_mut`
// **必然失败** ⇒ 新回调被**静默丢弃**、旧回调继续生效（且无任何报错）；而 `filters.rs`
// 当时的文档却写着"新回调自下一次通知起生效（与 `ui/controls.rs` 同款）"—— **不实**
// （`ui/controls.rs` 用的是本文件这一套 take/put-back，不是持借用直调）。
//
// **语义（契约）**：回调内自替换 ⇒ **本次通知仍由旧回调执行完毕，新回调自下一次通知起
// 生效**。触发时先把回调**从槽里取出**（槽置 `None`、借用当场释放），再在**不持有任何
// 借用**的状态下调用它，最后"槽仍为空才放回"。
//
// **"放回"必须走 `Drop` 守卫**（[`PutBack`]）：写成 `take → f(v) → put_back` 时，用户回调
// panic ⇒ 展开**跳过**最后一句 ⇒ 槽**永久空置**、此后通知全静默丢失。放进 `Drop` 即
// "正常返回与展开两条路径都放回"。
// ═══════════════════════════════════════════════════════════════════════════

// ⚠️ **本块的可见性是"封死"的一部分**（B2c-1 收口 ①）：`take_cb` / `put_back_cb` /
// `PutBack` **只在本私有模块内可见** —— 它们**不再**是页面层可调用的 API。
// 对外（`filters.rs` / `p5_audit.rs` / 同 crate 其它模块）只经由下面的
// [`CbSlot::set`] / [`CbSlot::fire`] 两个动作使用回调槽。
//
// **为什么必须收进私有模块（而不是只去掉 `pub(crate)`）**：Rust 的私有可见性对
// **子模块**开放 —— 若把 `take_cb` 直接写在 `pages/mod.rs` 里、仅去掉 `pub(crate)`，
// 那么 `pages::filters` / `pages::p5_audit`（`pages` 的子模块）**仍然看得见**它 ⇒
// 调用点照旧能写出"自己 take、自己 fire"的旧写法，"封死"就只是口头约定。
// 放进同文件的私有 `mod sealed` 后，`pages::filters` **不是** `sealed` 的后代 ⇒
// 连名字都解析不到 ⇒ **类型层面**写不出来。
mod sealed {
    use std::cell::RefCell;

    /// 槽的承载类型（抽别名避免 `type_complexity` 告警；与 `ui/controls.rs` 同款）。
    type Slot<A> = RefCell<Option<Box<dyn FnMut(A)>>>;

    /// 从槽里**取出**回调并把槽置空（借用在本函数返回前已释放 ⇒ 调用期不持借用）。
    ///
    /// 拿不到借用（未来若出现其它长借用路径）⇒ `None`，**不 panic**（静默跳过本次通知）。
    fn take_cb<T: ?Sized>(slot: &RefCell<Option<Box<T>>>) -> Option<Box<T>> {
        match slot.try_borrow_mut() {
            Ok(mut s) => s.take(),
            Err(_) => None,
        }
    }

    /// **放回**回调：**仅当槽仍为空**（回调内没有自替换）时放回；否则丢弃旧回调
    /// （新回调自下一次通知起生效，见上方语义说明）。
    ///
    /// **不 panic**：拿不到借用即**不放回**（`Drop` 路径上再 panic = 双重 panic ⇒ abort）。
    fn put_back_cb<T: ?Sized>(slot: &RefCell<Option<Box<T>>>, f: Box<T>) {
        if let Ok(mut s) = slot.try_borrow_mut() {
            if s.is_none() {
                *s = Some(f);
            }
        }
    }

    /// **放回守卫**：把 [`take_cb`] 取出的回调临时托管在自己身上，**作用域结束时**（正常返回
    /// **与 panic 展开两条路径**）执行"槽仍为空才放回"。
    ///
    /// `completed` 的判据与诊断口径与 `ui/controls.rs::PutBack` **逐条一致**：调用方在
    /// `f(v)` **正常返回**之后才置 `true`；`Drop` 时它仍为 `false` ⇒ 本次触发途中确实发生了
    /// 展开 ⇒ 向 stderr 留一条诊断（**放回仍照做**，否则就是"槽永久空置"的缺陷）。
    /// 写 stderr 用 `let _ = writeln!(..)` 而非 `eprintln!`：后者写失败时**自身 panic**，
    /// 展开路径上二次 panic = abort。
    /// ⚠️ **刻意无 `pub`**（= 仅 `sealed` 内可见）：写成 `pub(super)` 会让
    /// `pages::filters` / `pages::p5_audit`（`pages` 的后代）重新看得见它 ⇒ 封死失效。
    struct PutBack<'a, T: ?Sized> {
        /// 回调取出前所在的槽（放回目标）。
        slot: &'a RefCell<Option<Box<T>>>,
        /// 托管中的回调；`Drop` 里 `take()` 走（保证只放回一次）。
        cb: Option<Box<T>>,
        /// **本帧**是否"回调已正常返回"（初值 `false` 是故意的，见上方说明）。
        completed: bool,
    }

    impl<'a, T: ?Sized> PutBack<'a, T> {
        /// 取出并托管槽里的回调（槽当场置空 ⇒ 调用期不持借用）。
        fn take(slot: &'a RefCell<Option<Box<T>>>) -> Self {
            Self {
                slot,
                cb: take_cb(slot),
                completed: false,
            }
        }

        /// 调用托管中的回调（**不持有任何借用**；槽里没有回调则什么都不做）。
        fn fire<V>(&mut self, v: V)
        where
            T: FnMut(V),
        {
            if let Some(f) = self.cb.as_mut() {
                f(v);
            }
            self.completed = true;
        }
    }

    impl<T: ?Sized> Drop for PutBack<'_, T> {
        fn drop(&mut self) {
            if !self.completed {
                use std::io::Write;
                // ⚠️ 诊断文案**写成单行字符串字面量**（不用 `\` 续行）：`ui/tests.rs` 的
                // `strip_comments_and_literals` 把"非原始字符串里出现裸换行"一律判成**扫描器
                // 失真**并响亮失败（它不认 `\`+换行这种续行写法）—— 续行会让该文件的静态网
                // 全部失明。保持单行即可（与 `p5_audit.rs` 的 stderr 诊断同口径）。
                let _ = writeln!(
                    std::io::stderr(),
                    "回调槽：本次通知的回调**未正常返回**（panic 展开）—— 通知被截断；槽已放回，后续通知不受影响（该回调的内部状态可能已不一致）。"
                );
            }
            if let Some(f) = self.cb.take() {
                put_back_cb(self.slot, f);
            }
        }
    }

    /// **回调槽**（页面层唯一的用户回调容器；B2c-1 收口 ①）。
    ///
    /// **只暴露 [`CbSlot::set`] / [`CbSlot::fire`] 两个动作**：承载槽的字段是**私有**的
    /// （且本类型所在的 `sealed` 模块对 `pages::filters` / `pages::p5_audit` 不可达）⇒
    /// 调用点**在类型层面无法**写出"调用用户回调期间持着 `try_borrow_mut` 借用"的旧写法。
    /// 那正是本单元修的 Important ③：持借用直调 ⇒ 回调内 `set_on_change` 的
    /// `try_borrow_mut` **必然失败** ⇒ 新回调被**静默丢弃**、旧回调继续生效、无任何报错。
    ///
    /// **语义（契约，与 `ui/controls.rs` 同款）**：回调内自替换 ⇒ 本次通知仍由**旧**回调
    /// 执行完毕，新回调自**下一次**通知起生效。
    ///
    /// **绝不 panic**：拿不到槽的借用 ⇒ 本次通知**静默跳过**；用户回调 panic ⇒
    /// [`PutBack`] 的 `Drop` 守卫仍把槽放回（否则槽永久空置 ⇒ 此后所有通知静默丢失）。
    ///
    /// **探针（收口 ①）**：把调用点改回旧的"持借用直调"写法 ⇒ **编译不过**
    /// （`CbSlot` 没有 `try_borrow_mut`、私有字段也拿不到）；把 [`CbSlot::fire`] 的函数体
    /// 换回"持借用直调" ⇒ `pages_chain` 的**真件**自替换用例当场变红。
    pub struct CbSlot<A> {
        /// **私有**（见类型文档：这是"调用点写不出旧写法"的机制本体）。
        slot: Slot<A>,
    }

    impl<A> CbSlot<A> {
        /// 建一个空槽。
        pub const fn new() -> Self {
            Self {
                slot: RefCell::new(None),
            }
        }

        /// 注册回调（**覆盖**旧回调；不触发）。
        ///
        /// 拿不到借用时**静默不注册**而**不是** panic：事件回调内的 panic 会被事件桥
        /// `catch_unwind` 吞掉（屏上无任何迹象 —— 那才是更难查的失效）。
        pub fn set<F>(&self, f: F)
        where
            F: FnMut(A) + 'static,
        {
            if let Ok(mut s) = self.slot.try_borrow_mut() {
                *s = Some(Box::new(f));
            }
        }

        /// 触发：**取出 → 调用 → （槽仍为空才）放回**，调用期**不持任何借用**。
        pub fn fire(&self, v: A) {
            let mut guard: PutBack<'_, dyn FnMut(A)> = PutBack::take(&self.slot);
            guard.fire(v);
        }
    }
}

/// 对外只导出 [`sealed::CbSlot`]（`take_cb` / `put_back_cb` / `PutBack` 留在私有模块内，
/// **不可**从 `pages::filters` / `pages::p5_audit` 触达 —— 见 `mod sealed` 的说明）。
pub(crate) use sealed::CbSlot;

#[cfg(test)]
mod tests {
    use super::*;
    // `RefCell` 只在测试里用（生产侧的回调槽走 `sealed::CbSlot`，其内部借用不外露）。
    use std::cell::RefCell;

    // ── 区块级帧可信度标记（EDGE-03 / EDGE-20；纯逻辑，不触碰 LVGL）──

    /// **冻结 / 数据过期**标记的判据（唯一真源 = [`frame_mark`]）。
    ///
    /// **改什么会让本条变红**：
    /// - 把 `Down` 分支的 `frame.is_some()` 守卫去掉（无帧也打「冻结」）⇒ 第 4 条红
    ///   （无帧时数值本就是 `–` 占位，不是"冻结的旧值"，打标即**谎报**）；
    /// - 把 `Stale` 分支的 `channel == Connected` 守卫去掉（通道断也走「数据过期」）⇒
    ///   第 2 条红（通道断必须显「冻结」，否则与 EDGE-03 的用字不符）；
    /// - 把 `frame_mark` 改成恒 `None` ⇒ 第 2 / 3 条红。
    #[test]
    fn frame_mark_covers_frozen_stale_and_live() {
        let f = crate::ui::tests::frame_healthy();
        // ① 实时（通道通 + 帧新鲜）⇒ **不打标**
        assert_eq!(frame_mark(&PageInput::live(&f)), None);
        // ② 通道断 + 保留最近有效帧 ⇒ 「冻结」（EDGE-03 原文用字）
        assert_eq!(frame_mark(&PageInput::down(Some(&f))), Some(TEXT_FROZEN));
        // ③ 通道通但帧旧 ⇒ 「数据过期」（与 P1 的角标**同一串**，不另造）
        assert_eq!(
            frame_mark(&PageInput::new(
                Some(&f),
                ChannelStatus::Connected,
                Freshness::Stale
            )),
            Some(p1_status::TEXT_STALE)
        );
        // ④ 通道断且**无帧** ⇒ 数值本就是占位符，**不得**打「冻结」（否则谎报"有冻结帧"）
        assert_eq!(frame_mark(&PageInput::down(None)), None);
        // ⑤ 尚未首连（`Init`）⇒ 不打标
        assert_eq!(frame_mark(&PageInput::init()), None);
        // ⑥ 通道断 + 帧旧 ⇒ **仍是**「冻结」（通道级降级优先，两条不并存 —— 与 P1 同口径）
        assert_eq!(
            frame_mark(&PageInput::new(
                Some(&f),
                ChannelStatus::Down,
                Freshness::Stale
            )),
            Some(TEXT_FROZEN)
        );
    }

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

    // ── 回调槽（[`CbSlot`]；B2c-1 代码质量评审 ③ / 收口 ①；**纯逻辑**，不触碰 LVGL）──

    /// 测试用槽：`Rc<CbSlot<i32>>`（回调内需要**再拿到槽**以自替换 ⇒ 外面套一层 `Rc`；
    /// 回调只持 `Weak` ⇒ **不构成 `Rc` 环**）。
    ///
    /// **与生产侧同型**：`filters::RangeSlot` / `p5_audit::QuerySlot` **就是**
    /// [`CbSlot`]`<载荷>` ⇒ 本节的网直接罩住那两个调用点用的类型（收口 ①：此前本节复刻的是
    /// 裸 `RefCell` 的**机制桩**，调用点把 `CbSlot` 换回裸槽时本节照样全绿）。
    type Slot = Rc<CbSlot<i32>>;

    /// **回调内自替换 ⇒ 本次由旧回调跑完、新回调自下一次生效**。
    ///
    /// **改什么会让本条变红**：把 [`CbSlot::fire`] 换回"触发时持 `try_borrow_mut` 直调"
    /// （原实现）—— 回调内 [`CbSlot::set`] 的 `try_borrow_mut` 失败 ⇒ 新回调**被静默丢弃**
    /// ⇒ 第二次 `fire` 仍是"旧回调" ⇒ 第 2 组断言拿到 `["旧", "旧"]` 而不是 `["旧", "新"]`。
    /// **这正是被修的缺陷**（`filters.rs` 当时还写着"新回调自下一次通知起生效"）。
    #[test]
    fn callback_slot_self_replacement_takes_effect_next_time() {
        let slot: Slot = Rc::new(CbSlot::new());
        let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let weak = Rc::downgrade(&slot);
            let log = Rc::clone(&log);
            slot.set(move |_v: i32| {
                log.borrow_mut().push("旧");
                // 回调内**自替换**：此刻不得持有任何借用（否则本次 `try_borrow_mut` 失败）。
                if let Some(s) = weak.upgrade() {
                    let log = Rc::clone(&log);
                    s.set(move |_v: i32| log.borrow_mut().push("新"));
                }
            });
        }
        slot.fire(1);
        assert_eq!(*log.borrow(), vec!["旧"], "本次通知由**旧**回调执行完毕");
        slot.fire(2);
        assert_eq!(
            *log.borrow(),
            vec!["旧", "新"],
            "新回调必须**自下一次通知起生效**（若为持借用直调 ⇒ 这里仍是 `旧`）"
        );
        slot.fire(3);
        assert_eq!(*log.borrow(), vec!["旧", "新", "新"], "此后恒为新回调");
    }

    /// **回调 panic ⇒ 槽仍被放回**（`Drop` 守卫；此后通知照常到达）。
    ///
    /// **改什么会让本条变红**：把 `sealed::PutBack` 的 `Drop` 换成"调用后手动 `put_back`"
    /// —— panic 展开会跳过那一句 ⇒ 槽**永久空置** ⇒ 第 2 次 `fire` 静默无事 ⇒ 断言拿到
    /// `["前"]`（而不是 `["前", "前"]`）。
    #[test]
    fn callback_slot_put_back_survives_panic() {
        let slot: Slot = Rc::new(CbSlot::new());
        let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let n: Rc<Cell<usize>> = Rc::new(Cell::new(0));
        {
            let log = Rc::clone(&log);
            let n = Rc::clone(&n);
            slot.set(move |_v: i32| {
                let k = n.get();
                n.set(k + 1);
                log.borrow_mut().push("前");
                if k == 0 {
                    panic!("第一次就炸（守卫仍需把槽放回）");
                }
            });
        }
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| slot.fire(1)));
        assert_eq!(*log.borrow(), vec!["前"], "第一次调用确实展开了");
        // 槽已被守卫放回 ⇒ 第二次调用仍然到达（本帧不 panic）。
        slot.fire(2);
        assert_eq!(
            *log.borrow(),
            vec!["前", "前"],
            "panic 后槽**不得**永久空置（后续通知必须照常到达）"
        );
    }
}
