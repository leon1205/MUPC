//! 帧状态模型 + 三态归一（纯逻辑，可单测）。
//!
//! 对齐 `[DESIGN_APPROVED]` 设计 §3.4/§5.3 与 UI §7：
//! - 通道态派生：`ChannelInit`（未首成功 GET）/ `Connected` / `ChannelDown`（无成功 >3s → 整屏态）。
//! - 新鲜度：`now − frame.ts_ms > stale_ms` → `Stale`（打「数据过期」标，保留最近值，不冒充实时）。
//! - 点级三态归一：`NumView::Value(v)` ↔ 正常展示；`NumView::Dash(flag)` ↔ 该字段显 `--`+对应角标
//!   （禁 0/陈旧值，PRD 不造假值）。SOC 双源皆失 → `SocView::Lost`（禁沿用旧值）。
//!
//! 本文件不含任何绘图/IO——`UiSnapshot` 为渲染层（layout）唯一消费的归一化视图。
//!
//! ## ⚠️ B3-1 偏差登记（设计 §5.4 的 `UiState` 草图 vs 页面实际接口）
//!
//! §5.4 的 `UiState` 是**示意草图，早于六页实现**。对账后**不照抄**，逐条如下：
//!
//! | # | 草图 | 现状（本文件 / 页面实际契约） | 原因 |
//! |---|------|-------------------------------|------|
//! | S-1 | 单体 `UiState { frame, channel, soc, … }` | 帧派生部分**已**由 v1.0 [`DisplayState`] + [`UiSnapshot`] 承担（逐条保留，未动）；v2 控制侧另立 [`ControlState`] | 草图把两代模型画成一个结构；合并会把已交付、已被六页消费的契约重写一遍（双份真源风险） |
//! | S-2 | `dirty: bool` | **不持有**：`dirty` 的真源在**页面** —— `P2ConfigPage::is_dirty()`，`Shell::tick` 每拍直读它（`shell.rs:918` 的空闲计时暂停、`shell.rs:1213` 的 EDGE-11 提示条；偏差 **SH2**）。本层曾加过一个 `dirty` 镜像字段 + `set_dirty`（B3-1 初版），**因全无消费者、且构成同一事实的第二份真源，已删除**（B3-1 规格评审阻塞 1） | `ui/**` 与 `shell.rs` 对 `set_dirty` / `dirty()` **零调用**；只写不读的镜像就是本项目明令禁止的死代码。草图 `dirty: bool` 的语义已由页面承担 |
//! | S-3 | `toast: Option<Toast>` | `Toast` 是**LVGL 句柄**（`ui/components.rs`，靠 `layer_top()` 建对象）⇒ 纯逻辑层持 [`ToastRecord`]（文本 + 截止时刻）；句柄仍归页面（`p2_config` 的 `toast` 字段） | 本文件**零 LVGL**（v1.0 起即是纯逻辑）；把句柄搬进来会让状态层依赖图形栈 |
//! | S-4 | `confirm: Option<ConfirmDialog>` | [`ControlState::confirm`] = `Option<ConsoleEndpoint>`（`is_some()` 即「弹层打开」） | 同上（句柄归页面）；`Shell::set_modal_open(state.confirm().is_some())` 是已登记契约，谓词语义一致。**⚠️ B3-2c 订正**：`state.confirm` 结构上**拿不到生产者**（弹层生命周期归页面）⇒ `app.tick` 改为直读 `P2ConfigPage::dialog_open() \|\| P4InterlockPage::dialog_open()`（两页新增的**生产可见**查询口）；本格的三个方法**零生产调用者**，保留但勿再当作真源 |
//! | S-5 | （未提）累计失败计数 | **不记**：累计数归 `crate::console::ConsoleClient::fail_streak()`；本层只记**失败时刻** `last_transport_failure_ms` | 同一事实不记两份（否则两个计数会漂移） |
//! | S-6 | `ControlCode → 上屏文案` | `Ok` → `None`（**成功文案按操作由页面给定**：P2「保存成功 · 已生效」/ P4「已释放联锁」…）；失败码见 [`control_code_text`] | 草图给不出"这一条成功是什么操作"；由本层硬给一份成功串 = 与页面 `show_result` **双份真源** |
//! | S-7 | 文案出处 | **全部转出 `ui/**` 既有字面量**（本文件不新增任何上屏字面量）；EDGE-18 取页面已落地的「审计不可用 · 操作未执行」 | 码表覆盖率的静态网只扫 `ui/**`（`ui/tests.rs::ui_texts_covered_by_font_cmap`）⇒ 在本文件自造新串，**豆腐块网照不到**。§8.3 原文的全角逗号 `，` 不在 cmap 内，页面已改写为 `·`（见 `p2_config` 的 PD 登记） |

use mupc_display_proto::peripherals_labels::{ui_text, FIRE_DETECTOR_STATE_BITS, FIRE_LEVEL_ENUM};
use mupc_display_proto::{
    BitMeta, CatalogPoint, ConsoleEndpoint, ControlCode, ControlResponse, DecodeFrom, Decompose,
    DisplayFrame, Field, FieldFlag, LinkState, PointValue, RunState, SocSource,
};

// 裁定 3：传输失败时的**本地合成**回执 —— 决策（读/写端点 ⇒ 哪个页面的既有入口）归
// `control_route`（纯映射层，那里的用例逐字段钉死取值），本层只负责"在清在途之前取出在途信息"。
use crate::control_route::RouteDecision;

// 控制回执 / 传输失败的**上屏文案**：全部**转出** `ui/**` 的既有字面量（本文件不新增任何
// 上屏字面量 —— 码表覆盖率的基线在 `ui/tests.rs`，只扫 `ui/**`；若在此自造新串，既有的
// 豆腐块静态网**照不到**它）。逐个出处见 [`control_code_text`] / [`TRANSPORT_FAIL_TEXT`]。
use crate::ui::pages::p2_config::TEXT_AUDIT_UNAVAILABLE;
use crate::ui::pages::p4_interlock::{TEXT_INTERNAL, TEXT_OP_BUSY, TEXT_RETRY_EXPIRED, TEXT_TOAST_FAIL};

/// 通道断判定阈值：无成功 GET 超过该时长 → 切「与主进程数据通道断开」整屏态（UI §7.5 / PRD 6.3）。
pub const CHANNEL_DOWN_MS: u64 = 3000;

/// 通道状态（设计 §5.3 四种态里与主进程连通性相关的三种；新鲜度另由 [`Freshness`] 表达）。
///
/// ⚠️ **既有 3 态语义不动**（N-12 / §15.6.2 的代码块注）；本增量**只增 1 个变体**
/// [`ChannelStatus::Incompatible`]（U-73 / EDGE-21 / F26.3）。`Stale` **不在**本枚举内
/// （它属 [`Freshness`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelStatus {
    /// 尚未首次成功 GET（渲染可先于 mupcd 启动 → 显示「初始化中」）。
    Init,
    /// 最近一次成功 GET 距今 ≤ 通道断阈值，链路通。
    Connected,
    /// 无成功 GET > `CHANNEL_DOWN_MS` → 整屏「与主进程数据通道断开」。
    Down,
    /// **帧版本不匹配**（EDGE-21 / F26.3；设计 §15.6.2 的代码块）。
    ///
    /// `got` = 帧内版本，`expected` = 本地 [`mupc_display_proto::PROTO_VERSION`]。
    /// 归因来源 = [`crate::Error::ProtoVersion`]（产生点 `channel.rs` 的版本校验），
    /// 由 [`DisplayState::record_incompatible`] 记入、**粘性**（须一次成功帧才清除，见
    /// [`DisplayState::channel_status`]）。
    Incompatible {
        /// 帧内 `version`。
        got: u8,
        /// 本地 `PROTO_VERSION`。
        expected: u8,
    },
}

/// 单帧数据新鲜度（PRD F5.3：now − ts_ms > stale_ms → 过期）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Stale,
}

/// 整屏展示模式（layout 据此选覆盖层文案/灰化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenMode {
    /// 通道断（>3s）→ 中央「与主进程数据通道断开」覆盖，底层冻结。
    ChannelDown,
    /// 尚未首成功 → 中央「正在连接数据通道…」。
    Init,
    /// 有实时/过期数据帧正常展示（过期仅在字段加「数据过期」标，非整屏覆盖）。
    Live,
    /// **版本不匹配**（EDGE-21 / F26.3；设计 §15.6.2 情形 ③）：灰底 +
    /// 「屏与主进程版本不匹配：屏 v`got` / 主进程 v`expected`，请刷同版本固件」+
    /// 「最后成功 …」；**不黑屏、不显示半帧 / 混版帧、不显示任何数值**。
    ///
    /// 与 [`ScreenMode::ChannelDown`] 的文案**字符串不相等**（T-16）——现场排障要能区分
    /// 「刷固件」与「查进程」。
    VersionMismatch {
        /// 帧内版本。
        got: u8,
        /// 本地 `PROTO_VERSION`。
        expected: u8,
    },
}

/// 数值字段三态归一：正常展示值 / 该字段显 `--` + 角标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumView {
    Value(f64),
    /// 点级降级：`--` + [`FieldFlag`] 对应角标（NotRead=未取数 / Offline=源离线 / RangeError=数据异常）。
    Dash(FieldFlag),
}

impl NumView {
    /// 由帧字段归一（设计：`flag == Valid` 且 `v` 有值 → 正常；否则 Dash）。
    pub fn from_field(f: &Field) -> Self {
        if f.flag == FieldFlag::Valid {
            match f.v {
                Some(v) => NumView::Value(v),
                // Valid 但无值 = 生产方异常；按数据异常降级，绝不补 0。
                None => NumView::Dash(FieldFlag::RangeError),
            }
        } else {
            NumView::Dash(f.flag)
        }
    }

    /// 是否降级（Dash）。
    pub fn is_degraded(&self) -> bool {
        matches!(self, NumView::Dash(_))
    }

    pub fn value(&self) -> Option<f64> {
        match self {
            NumView::Value(v) => Some(*v),
            NumView::Dash(_) => None,
        }
    }
}

/// 角标文案集中定义（与 font.rs 码表 / UI §6.6 同步）。
pub fn dash_badge(flag: FieldFlag) -> &'static str {
    match flag {
        FieldFlag::NotRead => "未取数",
        FieldFlag::Offline => "源离线",
        FieldFlag::RangeError => "数据异常",
        FieldFlag::Valid => "",
    }
}

/// SOC 主区三态（UI §7.2）：有值正常展示 / 双源皆失 → `--` + 「SOC 源失效」，禁沿用旧值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SocView {
    Value(f64),
    Lost,
}

impl SocView {
    pub fn value(&self) -> Option<f64> {
        match self {
            SocView::Value(v) => Some(*v),
            SocView::Lost => None,
        }
    }
}

/// SOC 展示警示档（PRD F1.3：≤15 红 / 15–85 青 / ≥85 橙）——仅驱动 UI 断点，非控制硬限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocBand {
    Low,
    Mid,
    High,
}

/// SOC 展示档**下限**阈值（%）：`≤` 此值 → [`SocBand::Low`]（PRD F1.3；UI §6.1 量程条 0–15 % 红段）。
///
/// **单一真源**（B2a 规格评审 Minor ⑤）：P1 的量程条分段几何（`ui/pages/p1_status.rs`）曾另抄一份
/// `SOC_LOW_PCT = 15`，与本文件 `soc_band` 的阈值字面量构成**双份真源**。阈值统一收在此处，
/// 两处共用 —— 改阈值只改这里。
pub const SOC_LOW_PCT: i32 = 15;
/// SOC 展示档**上限**阈值（%）：`≥` 此值 → [`SocBand::High`]（PRD F1.3；UI §6.1 量程条 85–100 % 橙段）。
pub const SOC_HIGH_PCT: i32 = 85;

/// SOC 值 → 展示档。SOC 值超出 0..100 亦 clamp 到端点档（生产方已域值化，此处兜底）。
///
/// 阈值取自 [`SOC_LOW_PCT`] / [`SOC_HIGH_PCT`]（单一真源，见其文档）。
pub fn soc_band(v: f64) -> SocBand {
    if v <= SOC_LOW_PCT as f64 {
        SocBand::Low
    } else if v >= SOC_HIGH_PCT as f64 {
        SocBand::High
    } else {
        SocBand::Mid
    }
}

/// 逐字段/逐区新鲜度点语义（UI §5.3 状态点：● 实时 / ○ 停更 / 琥珀 过期）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveDot {
    /// 字段有效且帧新鲜 → 实心青 ●
    Live,
    /// 字段降级（未取数/离线/异常）或通道不实时 → 空心 ○
    Paused,
    /// 帧过期（值保留）→ 琥珀点。
    Expired,
}

/// 由「字段归一值 + 整帧新鲜度」推导该字段状态点（渲染侧三态语法，纯逻辑）。
pub fn live_dot_for(nv: &NumView, fresh: Freshness) -> LiveDot {
    match nv {
        NumView::Value(_) if fresh == Freshness::Fresh => LiveDot::Live,
        NumView::Value(_) => LiveDot::Expired,
        NumView::Dash(_) => LiveDot::Paused,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// U-73（T21c-1 / 落点 §15.11 #9）：外设上屏的**降级语义**与展示派生
//
// 设计 §15.6.2 的代码块在此**逐行落地**（枚举定义 / 语义 / 互异要求）。
// 本节的函数**全部是纯逻辑**（零 LVGL、零 I/O、不读时钟）⇒ 可离线复现（T-14 / T-15 / T-17）。
//
// **文案纪律**（硬约束 #1）：本节的每一条上屏中文都**转出** `display-proto` 的常量
// （[`ui_text`] / 位名与枚举来自 catalog 或 `display-proto` 的锁定表）——
// 本文件**不新增任何上屏字面量**（`ui/tests.rs` 的码表网会扫本文件）。
// ═══════════════════════════════════════════════════════════════════════════

/// **点级**降级原因（设计 §15.6.2 的代码块；六态）。
///
/// **各语义的字符串两两互异**（T-14）：`站离线` / `未取数` / `数据异常` / `未配置` /
/// `名称未获取` / `明细不可用` —— 判据是 [`MissingReason::text`] 的返回值集合**无重复**
/// （见 `ui/tests.rs` 的 T-14 用例）。
///
/// ⚠️ **不含站级态**：「站点未启用」（R-3 裁定 / 情形 ⑦）是**站级 / 段级**状态（该 role
/// 根本未配置 ⇒ 谈不上"这一点的值")，落 [`StationState`] —— 硬塞进本枚举会让"点级降级"
/// 与"整站缺席"两件事混为一谈（判据不同：一个是 `flag` / 配置，另一个是 `CatalogStation.enabled`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingReason {
    /// 站离线（站级降级；EDGE-18 / F25.4）。
    StationOffline,
    /// 点未取数（`flag = NotRead`；EDGE-19）。
    NotRead,
    /// 数据异常（非有限 / 越界；`flag = RangeError`；EDGE-20）。
    RangeError,
    /// 未配置（仅消防钢瓶气压：`cylinder_configured == Some(false)`；EDGE-23 / EX-11）。
    NotConfigured,
    /// 名称未获取（catalog 未取到 ⇒ **不臆造中文名**，§15.3.1）。
    NameUnavailable,
    /// 明细不可用（下钻端点失败；§15.6.2 ⑥ / R-4 产品裁定）。
    DetailUnavailable,
}

impl MissingReason {
    /// 全部六态（**升序遍历**用；T-14 的"两两互异"断言即遍历本数组）。
    pub const ALL: [MissingReason; 6] = [
        MissingReason::StationOffline,
        MissingReason::NotRead,
        MissingReason::RangeError,
        MissingReason::NotConfigured,
        MissingReason::NameUnavailable,
        MissingReason::DetailUnavailable,
    ];

    /// 上屏文案（**全部转出** `display-proto::peripherals_labels::ui_text`，本文件零字面量）。
    pub const fn text(self) -> &'static str {
        match self {
            MissingReason::StationOffline => ui_text::STATION_OFFLINE,
            MissingReason::NotRead => ui_text::NOT_READ,
            MissingReason::RangeError => ui_text::RANGE_ERROR,
            MissingReason::NotConfigured => ui_text::NOT_CONFIGURED,
            MissingReason::NameUnavailable => ui_text::NAME_UNKNOWN,
            MissingReason::DetailUnavailable => ui_text::DETAIL_UNAVAILABLE,
        }
    }
}

/// **站级**展示态（情形 ⑦ 的落点；设计 §15.6.2 ⑦ / §15.5.2 装置段 / R-3 产品裁定）。
///
/// 判据 = `catalog.CatalogStation.enabled`（该 role 是否**已配置**）∪ 帧内站的 `online`
/// —— 两者都是注入值，本层**不重判、不另立门限**（F25.1 单真源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationState {
    /// 已配置且在窗内（帧内站 `online = true`）。
    Online,
    /// 已配置但当前不可达（`online = false`）⇒ 段顶状态条 + 段内行均显「站离线」。
    Offline,
    /// **该 role 未配置**（catalog `enabled = false`；帧内**不含**该站）⇒
    /// 装置段该行显「**未启用**」、段内各段显「**站点未启用**」（**与「站离线」互异**）。
    Disabled,
    /// 两处都不可得（catalog 未取到 ∧ 帧内无该站）⇒ 「不可用」（**不猜**"未配置"或"离线"）。
    Unknown,
}

impl StationState {
    /// **装置段站状态行**文案（§15.5.2 装置段：`未启用` / `站离线` / `在线`）。
    pub const fn row_text(self) -> &'static str {
        match self {
            StationState::Online => ui_text::ONLINE,
            StationState::Offline => ui_text::STATION_OFFLINE,
            StationState::Disabled => ui_text::STATION_DISABLED,
            StationState::Unknown => ui_text::UNAVAILABLE,
        }
    }

    /// **段级**文案（该站各段；`None` = 站在线 ⇒ 无段级降级）。
    ///
    /// 「站点未启用」（`Disabled`，单站缺席）**≠**「外设数据不可用」（整段源不可得，
    /// §15.6.2 ⑤）—— 后者由 `PeripheralsSection::available == false` 承担。
    pub const fn section_text(self) -> Option<&'static str> {
        match self {
            StationState::Online => None,
            StationState::Offline => Some(ui_text::STATION_OFFLINE),
            StationState::Disabled => Some(ui_text::SECTION_STATION_DISABLED),
            StationState::Unknown => Some(ui_text::UNAVAILABLE),
        }
    }

    /// **点级**原因（只有 `Offline` 会给出点级「站离线」）。
    ///
    /// `Disabled` **不**给点级原因（该站无任何数据可谈 ⇒ 由段级文案承担，设计 §15.5.2：
    /// 段内显「站点未启用」）；`Online` 无降级。
    pub const fn point_missing(self) -> Option<MissingReason> {
        match self {
            StationState::Offline => Some(MissingReason::StationOffline),
            _ => None,
        }
    }
}

/// 站级态判据（**唯一真源**；纯函数 ⇒ 可单测）。
///
/// - `enabled`：catalog `CatalogStation.enabled`（`None` = catalog 未取到）；
/// - `online`：帧内该站 `PeripheralStation.online`（`None` = 帧内**不含**该站）。
///
/// **优先级**：`enabled == Some(false)` ⇒ [`StationState::Disabled`]（未配置优先 ——
/// 帧内本就不含该站，谈"离线"没有意义）；其后 `online` 的真值；两处皆缺 ⇒ `Unknown`。
pub const fn station_state(enabled: Option<bool>, online: Option<bool>) -> StationState {
    match (enabled, online) {
        (Some(false), _) => StationState::Disabled,
        (_, Some(true)) => StationState::Online,
        (_, Some(false)) => StationState::Offline,
        _ => StationState::Unknown,
    }
}

/// 消防钢瓶「未配置」判定（EDGE-23 / EX-11；**只对 role=Fire 的点 `fire_sys_2` 有意义**）。
///
/// `Some(false)` ⇒ [`MissingReason::NotConfigured`]，调用方须**忽略 `v`**（不得显 `0 kPa`）、
/// 且该点**不产告警条目**（组帧侧口径；HMI 侧即"不把它当数值展示"）。
/// `Some(true)` / `None` ⇒ 按值正常展示（`None` = 不可得，**不臆造**"未配置"）。
pub const fn cylinder_missing(configured: Option<bool>) -> Option<MissingReason> {
    match configured {
        Some(false) => Some(MissingReason::NotConfigured),
        _ => None,
    }
}

/// 单点的**展示视图**（U-73）：帧内点值 × 站级态 × catalog 元数据 → 屏上**一行**。
///
/// 派生规则（逐条可测）：
/// 1. **站级优先**：`Disabled` ⇒ 无点级原因（段级文案承担）；`Offline` ⇒
///    [`MissingReason::StationOffline`]（**不保留旧值**）；
/// 2. 点级：`flag != Valid` ⇒ [`MissingReason::NotRead`] / `RangeError` / `StationOffline`
///    （`flag = Offline` 即"该点所在源不可达"）；`Valid` 但 `v = None` / 非有限 ⇒ `RangeError`
///    （**不补 0**，与 [`NumView::from_field`] 同口径）；
/// 3. `名称未获取`：catalog 未取到（`meta = None`）⇒ `missing = NameUnavailable`
///    （**值照常显示**，只是没有中文名 —— §15.3.1）；
/// 4. **位语义 / 枚举文案 / 拆解规格**一律**照抄 catalog**（`meta`），**屏侧不猜**。
#[derive(Debug, Clone, PartialEq)]
pub struct PeriphView {
    /// 站级态（`Disabled` / `Offline` 决定段级与点级降级）。
    pub station: StationState,
    /// catalog 短标签（`None` = catalog 未取到 ⇒ 显 [`ui_text::NAME_UNKNOWN`]）。
    pub label: Option<String>,
    /// 单位（catalog；`None` = 无量纲 **或** 不可得）。
    pub unit: Option<String>,
    /// 小数位（catalog 给出；**由登记 `scale` 派生**，屏侧不自行决定）。
    pub decimals: u8,
    /// 展示值（`None` = 降级，原因见 [`PeriphView::missing`]）。
    pub value: Option<f64>,
    /// 降级原因（`None` = 正常展示）。
    pub missing: Option<MissingReason>,
    /// 位语义（照抄 catalog；空 = 标量点）。
    pub bits: Vec<BitMeta>,
    /// 枚举文案（照抄 catalog；空 = 非枚举 / 文案未登记）。
    pub enum_labels: Vec<(u16, String)>,
    /// 展示层拆解规格（照抄 catalog；空 = 不拆解、按整字显示）。
    pub decompose: Vec<Decompose>,
}

impl PeriphView {
    /// 由「站级态 + 帧内点值 + catalog 点（可缺）」派生（见结构体文档的四条规则）。
    pub fn derive(station: StationState, pv: &PointValue, meta: Option<&CatalogPoint>) -> Self {
        let station_reason = station.point_missing();
        let point_reason = match pv.flag {
            FieldFlag::Valid => match pv.v {
                Some(v) if v.is_finite() => None,
                // Valid 却无值 / 非有限 = 生产方异常 ⇒ 数据异常（**绝不补 0**，同 NumView）。
                _ => Some(MissingReason::RangeError),
            },
            FieldFlag::NotRead => Some(MissingReason::NotRead),
            // 帧内点级「源离线」= 该点所在源不可达 ⇒ 归到站级语义（`MissingReason` 无
            // 「源离线」态；站级才是它的判据面，设计 §15.6.2 ①）。
            FieldFlag::Offline => Some(MissingReason::StationOffline),
            FieldFlag::RangeError => Some(MissingReason::RangeError),
        };
        // **站级降级优先于点级**（站离线时组帧侧已把该站全部点置 `v=None`，此处再兜一层）。
        let missing = station_reason.or(point_reason);
        let (label, unit, decimals, bits, enum_labels, decompose) = match meta {
            Some(m) => (
                Some(m.label.clone()),
                m.unit.clone(),
                m.decimals,
                m.bits.clone(),
                m.enum_labels.clone(),
                m.decompose.clone(),
            ),
            None => (None, None, 0, Vec::new(), Vec::new(), Vec::new()),
        };
        // ⚠️ **名称未获取不参与值级降级**（§15.3.1：「**从未取到** catalog ⇒ 值照常显示
        // （按点名），中文名位显「名称未获取」」）⇒ 它是**名称槽**的降级，由
        // [`PeriphView::name_missing`] 单独回答；`missing` 只表达**值**不可用。
        Self {
            station,
            label,
            unit,
            decimals,
            value: if missing.is_none() {
                pv.v.filter(|v| v.is_finite())
            } else {
                None
            },
            missing,
            bits,
            enum_labels,
            decompose,
        }
    }

    /// 应用消防钢瓶「未配置」覆盖（EDGE-23）：`Some(false)` ⇒ **忽略 `v`**、置
    /// [`MissingReason::NotConfigured`]（断言**不含 `0 kPa`** —— 因为值已被清成 `None`）。
    pub fn apply_cylinder(&mut self, configured: Option<bool>) {
        if let Some(m) = cylinder_missing(configured) {
            self.value = None;
            self.missing = Some(m);
        }
    }

    /// 短标签（catalog 未取到 ⇒ [`ui_text::NAME_UNKNOWN`]，**不臆造中文名**）。
    pub fn label_text(&self) -> &str {
        self.label.as_deref().unwrap_or(ui_text::NAME_UNKNOWN)
    }

    /// 降级时该行右端的原因文案（正常 ⇒ `None`）。
    pub fn missing_text(&self) -> Option<&'static str> {
        self.missing.map(MissingReason::text)
    }

    /// **名称槽**的降级（catalog 未取到 ⇒ [`MissingReason::NameUnavailable`]）。
    ///
    /// 与 [`PeriphView::missing`] **分开**：名不可得**不影响**值照常展示（§15.3.1）。
    pub fn name_missing(&self) -> Option<MissingReason> {
        self.label
            .is_none()
            .then_some(MissingReason::NameUnavailable)
    }

    /// 值是否可展示（`true` ⇒ 按 [`PeriphView::value`] + 单位 + 小数位渲染）。
    pub fn is_shown(&self) -> bool {
        self.value.is_some()
    }
}

/// 位行文案（A1 逐位 16 行 / A3 / 明细表状态列的**唯一**构造点；F21.1 / EX-09）。
///
/// - `defined == false` ⇒ 「未定义位 `index`」（其后接位号；**不猜语义**，**禁止**为凑满
///   16 位而编造）；
/// - 已定义 ⇒ `"{位名} {活跃 / 非活跃}"`；`active_text`（catalog 给的活跃语义）优先于通用
///   「活跃」；`inverted == true`（极性反转位，R-41 追认前**无生产者**）⇒ `在线 / 离线`。
///
/// 位名 / `active_text` **一律来自 catalog**（帧外元数据），本函数不产出中文。
pub fn bit_text(
    index: u8,
    defined: bool,
    label: &str,
    active: bool,
    active_text: Option<&str>,
    inverted: bool,
) -> String {
    if !defined {
        return format!("{} {index}", ui_text::BIT_UNDEFINED);
    }
    let state = if inverted {
        if active {
            ui_text::ONLINE
        } else {
            ui_text::OFFLINE
        }
    } else if active {
        active_text.unwrap_or(ui_text::BIT_ACTIVE)
    } else {
        ui_text::BIT_INACTIVE
    };
    format!("{label} {state}")
}

/// 从整字 `raw` 取某一位的活跃态（`(raw >> index) & 1`；位点与标量在帧内同构）。
///
/// 非有限 `raw` ⇒ `false`（**不猜**）；`index ≥ 64` 由调用方按 catalog 的 0..16 位号约束。
pub fn bit_active(raw: f64, index: u8) -> bool {
    if !raw.is_finite() {
        return false;
    }
    let word = raw.round() as i64;
    (word >> index) & 1 == 1
}

/// 「数据 1」（`fire_sys_10` / `fire_det_{…+3}`）的**展示层**拆解（F21.5 / EX-13 / T-17）。
///
/// 唯一字节语义来源 = catalog 的 [`DecodeFrom`]（**屏侧不自行猜位序**）：
/// `HighByte` ⇒ 高字节（烟雾 `×0.1` `dB/M`）；`LowByte` ⇒ 低字节（温度 `raw−55` `℃`）；
/// `Whole` ⇒ 整字。
///
/// ⚠️ **帧内整字值不变**：本函数**只读** `raw`（`Copy` 语义，调用点拿到的
/// `PointValue.v` 与 `latest_values` 里的值都是**整字**，不被改写）。
///
/// 非有限 `raw` ⇒ 原样返回（`NaN` 会经 [`PeriphView::derive`] 的有限性判据转成降级）
/// —— **不 panic、不造数**。
pub fn decompose_value(from: DecodeFrom, raw: f64) -> f64 {
    if !raw.is_finite() {
        return raw;
    }
    let word = raw.round() as i64;
    let hi = ((word >> 8) & 0xFF) as f64;
    let lo = (word & 0xFF) as f64;
    match from {
        DecodeFrom::Whole { scale, offset } => raw * scale + offset,
        DecodeFrom::HighByte { scale, offset } => hi * scale + offset,
        DecodeFrom::LowByte { scale, offset } => lo * scale + offset,
    }
}

/// 火警等级文案（F21.2 / EX-10 / T-15）—— 两条来源，**都不许屏侧猜**：
///
/// 1. **catalog `enum_labels` 优先**（运行时真源；屏侧**逐字照抄**）；
/// 2. catalog 未取到 / 该点无 `enum_labels` ⇒ 回退 **`display-proto` 锁定的
///    [`FIRE_LEVEL_ENUM`]**（六值，唯一权威 = PRD §3.9 F21 展示表）；
/// 3. **表外值 ⇒ 「未知」**（**绝不落「正常」**）。
pub fn fire_level_text(value: f64, enum_labels: &[(u16, String)]) -> String {
    let key = if value.is_finite() {
        Some(value.round())
    } else {
        None
    };
    if !enum_labels.is_empty() {
        return match key.and_then(|k| {
            enum_labels
                .iter()
                .find(|(v, _)| f64::from(*v) == k)
                .map(|(_, t)| t.clone())
        }) {
            Some(t) => t,
            None => ui_text::ENUM_UNKNOWN.to_string(),
        };
    }
    fire_level_text_static(value).to_string()
}

/// [`fire_level_text`] 的**无 catalog 回退**（返回 `display-proto` 的锁定量，零分配）。
///
/// 与前者**同一值域判据**（六值齐全 + 表外「未知」）；`ui/tests.rs` 的 T-15 对两条路径
/// **各断言一次**（catalog 路径用构造的 `enum_labels`，回退路径用本函数）。
pub fn fire_level_text_static(value: f64) -> &'static str {
    if !value.is_finite() {
        return ui_text::ENUM_UNKNOWN;
    }
    let k = value.round();
    FIRE_LEVEL_ENUM
        .iter()
        .find(|(v, _)| f64::from(*v) == k)
        .map(|(_, t)| *t)
        .unwrap_or(ui_text::ENUM_UNKNOWN)
}

/// 探测器状态整字的**已定义位**判据（T-19b）：位号 ∈ [`FIRE_DETECTOR_STATE_BITS`]。
///
/// **bit15（通信状态）恒 `false`** —— 点表登记明令"不猜、不造判据"、PRD F21 未要求
/// （R-41 追认前不上屏）⇒ 屏显「未定义位 15」。若产品 / 厂方追认，只改 catalog 的
/// `BitMeta{index:15, inverted:true}` 即可（**屏侧代码零改动**，本函数不参与判据）。
pub fn fire_det_state_bit_defined(index: u8) -> bool {
    FIRE_DETECTOR_STATE_BITS.iter().any(|(i, _)| *i == index)
}

// ---------------------------------------------------------------------------
// 显示状态容器（DisplayState）：由 run 主循环喂「拉帧结果 + 时钟」，向外派生通道态/新鲜度
// ---------------------------------------------------------------------------

/// 渲染进程显示状态（单线程使用，非 Sync）。
#[derive(Debug)]
pub struct DisplayState {
    frame: Option<DisplayFrame>,
    /// 最近一次成功 GET 的单调时钟 ms（虚拟时钟：run 注入 `now_ms`）。
    last_ok_ms: Option<u64>,
    /// 首次尝试拉帧时刻（用于 Init → Down 的超时判定）。
    first_attempt_ms: Option<u64>,
    /// 连续失败计数（诊断/日志；本身不直接驱动通道态——驱动靠 last_ok 时间）。
    fail_streak: u32,
    /// 乱序（回退）丢弃计数（W3：设计 §3.3「渲染端判连续/重排」——丢旧帧不做展示回退）。
    reorder_dropped: u64,
    /// 过期阈值（设计：默认取 display-proto `DEFAULT_STALE_MS=2000`，可 `--stale-ms` 覆盖）。
    stale_ms: u64,
    /// **粘性的**帧版本不匹配（EDGE-21 / F26.3；T-16）。
    ///
    /// `Some((got, expected))` = 已观测到版本不匹配、且**尚未**收到任何成功帧 ⇒
    /// [`ChannelStatus::Incompatible`]。**为什么粘性**：版本不匹配是**部署态**（两端不同版本
    /// 发布），不会因一次超时消失；若随 `record_fail` / 时钟回落就抖动，屏面会在
    /// 「版本不匹配」与「通道断开」之间闪 —— 现场无法判断该刷固件还是查进程（设计 §15.6.2 ③）。
    /// 唯一清除路径 = [`DisplayState::record_success`]（**一次成功帧**才清除）。
    incompatible: Option<(u8, u8)>,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayState {
    pub fn new() -> Self {
        Self {
            frame: None,
            last_ok_ms: None,
            first_attempt_ms: None,
            fail_streak: 0,
            reorder_dropped: 0,
            stale_ms: mupc_display_proto::DEFAULT_STALE_MS,
            incompatible: None,
        }
    }

    pub fn set_stale_ms(&mut self, ms: u64) {
        self.stale_ms = ms;
    }

    pub fn stale_ms(&self) -> u64 {
        self.stale_ms
    }

    /// 记录一次拉帧成功（更新最新帧 + 成功时刻）。
    ///
    /// **乱序保护（W3，设计 §3.3「渲染端判连续/重排」）**：`seq` 与 `ts_ms` **双双**回退
    /// （即确定更旧的帧）时丢弃，不做展示回退——但通道仍记成功（链路是通的）。用双条件而非
    /// 仅比 `seq`：mupcd 重启后 `seq` 清零（设计 §3.3），此时 `ts_ms` 更新，须接受新发布序号
    /// 周期的帧；仅比 `seq` 会让屏面冻结至 `seq` 追平旧值（最坏数十小时）。
    pub fn record_success(&mut self, frame: DisplayFrame, now_ms: u64) {
        self.first_attempt_ms.get_or_insert(now_ms);
        self.last_ok_ms = Some(now_ms);
        self.fail_streak = 0;
        // **一次成功帧即清除粘性「版本不匹配」**（§15.6.2 ③ / T-16 的唯一清除路径）。
        self.incompatible = None;
        if let Some(prev) = &self.frame {
            if frame.seq < prev.seq && frame.ts_ms <= prev.ts_ms {
                self.reorder_dropped = self.reorder_dropped.saturating_add(1);
                return; // 旧帧/乱序：保留较新帧，不把屏面回退到旧值
            }
        }
        self.frame = Some(frame);
    }

    /// 记录一次拉帧失败（不丢帧——保留最近有效帧供冻结展示，设计 §6.3）。
    pub fn record_fail(&mut self, now_ms: u64) {
        self.first_attempt_ms.get_or_insert(now_ms);
        self.fail_streak = self.fail_streak.saturating_add(1);
    }

    /// 记录一次**版本不匹配**（EDGE-21 / F26.3；落点见 §15.11 #9 / #10）。
    ///
    /// 语义（**粘性**）：一旦记入，[`Self::channel_status`] 即返回
    /// [`ChannelStatus::Incompatible`]，**直到** [`Self::record_success`] 收到一帧。
    /// 本方法**不**替代失败记账 —— 调用方（`app.rs::absorb`）在同一分支上照常调
    /// [`Self::record_fail`]（`Err(ProtoVersion)` 同时也是"本拍拉帧失败"）。
    ///
    /// ⚠️ **`now_ms` 只用于"首次尝试"基准**（与 [`Self::record_fail`] 同口径），
    /// 不参与新判据 —— 禁止在此另造时间阈值（F25.1 单一新鲜度真源）。
    pub fn record_incompatible(&mut self, got: u8, expected: u8, now_ms: u64) {
        self.first_attempt_ms.get_or_insert(now_ms);
        self.incompatible = Some((got, expected));
    }

    /// 当前是否处于粘性「版本不匹配」态（诊断 / 装配断言用）。
    pub fn incompatible(&self) -> Option<(u8, u8)> {
        self.incompatible
    }

    // ⚠️ **已删除 `update(res, now_ms)`**（B3-2a 质量评审 建议 I-5 的死代码清单：
    // 「仅测试用 ⇒ 改测试直调 `record_*` 或删除」）。
    // 删除理由：它只是 `Ok → record_success` / `Err → record_fail` 的转发，而**生产唯一**
    // 消费点 `App::absorb` 必须**自己**匹配 `Result`（它要在两条分支上分别累计
    // `frames_ok`/`frames_fail` 并做有限日志），故 `update` 在生产路径上**零调用**；
    // 唯一调用者是 `channel.rs` 的一条用例（那是"自比自"式的覆盖：测的是转发本身）。
    // 该用例已改为直调 [`DisplayState::record_fail`]（并显式断言传输层确实产出 `Err`），
    // 保留其真正要守的性质：**传输失败 ⇒ 状态层进入「通道断」展示态**。

    pub fn frame(&self) -> Option<&DisplayFrame> {
        self.frame.as_ref()
    }

    pub fn last_ok_ms(&self) -> Option<u64> {
        self.last_ok_ms
    }

    pub fn fail_streak(&self) -> u32 {
        self.fail_streak
    }

    /// 乱序（seq+ts 双双回退）被丢弃的帧数（诊断用；W3）。
    pub fn reorder_dropped(&self) -> u64 {
        self.reorder_dropped
    }

    /// 通道态派生（纯逻辑）。`now_ms` 为注入时钟。
    ///
    /// **优先级**：粘性 [`ChannelStatus::Incompatible`] **最高**（§15.6.2 ③：版本不匹配期间
    /// 不显示任何数值 ⇒ 屏面不该退回「通道断开」/「实时」），其后才是既有的
    /// `Connected` / `Down` / `Init` 三态（语义不动）。
    pub fn channel_status(&self, now_ms: u64) -> ChannelStatus {
        if let Some((got, expected)) = self.incompatible {
            return ChannelStatus::Incompatible { got, expected };
        }
        match self.last_ok_ms {
            Some(t) if now_ms.saturating_sub(t) < CHANNEL_DOWN_MS => ChannelStatus::Connected,
            Some(_) => ChannelStatus::Down,
            None => match self.first_attempt_ms {
                None => ChannelStatus::Init,
                Some(t) if now_ms.saturating_sub(t) >= CHANNEL_DOWN_MS => ChannelStatus::Down,
                Some(_) => ChannelStatus::Init,
            },
        }
    }

    /// 有帧时的单帧新鲜度（无帧 → Fresh 占位，layout 用 ScreenMode=Init 不会消费数值）。
    pub fn freshness(&self, now_ms: u64) -> Freshness {
        match &self.frame {
            Some(f) if now_ms.saturating_sub(f.ts_ms) > self.stale_ms => Freshness::Stale,
            _ => Freshness::Fresh,
        }
    }

    /// 整屏展示模式（layout 据此选覆盖层/灰化）。
    pub fn screen_mode(&self, now_ms: u64) -> ScreenMode {
        match self.channel_status(now_ms) {
            ChannelStatus::Down => ScreenMode::ChannelDown,
            ChannelStatus::Init => ScreenMode::Init,
            ChannelStatus::Connected => ScreenMode::Live,
            ChannelStatus::Incompatible { got, expected } => {
                ScreenMode::VersionMismatch { got, expected }
            }
        }
    }

    /// 渲染层唯一消费的归一化视图快照。
    pub fn snapshot(&self, now_ms: u64) -> UiSnapshot {
        let f = self.frame.as_ref();
        let mode = self.screen_mode(now_ms);
        let fresh = self.freshness(now_ms);
        UiSnapshot {
            mode,
            fresh,
            soc: match f {
                Some(f) if f.soc.is_some() && f.soc_source != SocSource::Lost => {
                    SocView::Value(f.soc.unwrap())
                }
                _ => SocView::Lost,
            },
            soc_source: f.map(|f| f.soc_source).unwrap_or(SocSource::Lost),
            run_state: f.and_then(|f| f.run_state),
            pcs_online: f.map(|f| f.pcs_online).unwrap_or(false),
            inconsistency: f.map(|f| f.inconsistency).unwrap_or(false),
            p_phase: f.map(|f| norm3(&f.p_phase)).unwrap_or(DASH3),
            p_total: f
                .map(|f| NumView::from_field(&f.p_total))
                .unwrap_or(NumView::Dash(FieldFlag::NotRead)),
            i_phase: f.map(|f| norm3(&f.i_phase)).unwrap_or(DASH3),
            clock_text: String::new(),
        }
    }
}

/// 无帧/字段缺失时的三相占位：全 `--` + 「未取数」角标。
const DASH3: [NumView; 3] = [NumView::Dash(FieldFlag::NotRead); 3];

/// `[Field; 3]` → `[NumView; 3]` 逐点归一（借位遍历，避免 `[Field;3]` 非 Copy 的移动）。
fn norm3(a: &[Field; 3]) -> [NumView; 3] {
    [
        NumView::from_field(&a[0]),
        NumView::from_field(&a[1]),
        NumView::from_field(&a[2]),
    ]
}

/// 渲染视图快照：layout 只依赖本结构，与底层帧/通道解耦（KISS + 可测）。
#[derive(Debug, Clone)]
pub struct UiSnapshot {
    pub mode: ScreenMode,
    pub fresh: Freshness,
    /// SOC 数值（Lost = 双源皆失 → `--` + 源失效警示）。
    pub soc: SocView,
    pub soc_source: SocSource,
    /// PCS 状态（None = 离线/无有效态 → 「PCS 离线」）。
    pub run_state: Option<RunState>,
    pub pcs_online: bool,
    pub inconsistency: bool,
    /// 三相有功（已 ×0.1 kW）。
    pub p_phase: [NumView; 3],
    pub p_total: NumView,
    /// 三相电流（已 ×0.1 A）。
    pub i_phase: [NumView; 3],
    /// 页眉时钟文本（ASCII HH:MM:SS，渲染端 run/bin 层负责生成；空则 layout 不画）。
    pub clock_text: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// v2.0 / B3-1：`hmi_channel` 本地覆盖（设计 §5.4 / §5.5）
// ═══════════════════════════════════════════════════════════════════════════

/// 屏侧自身通道态 → `DeviceSection.hmi_channel` 的展示态。
///
/// 设计 §5.4 / §5.5：`DeviceSection.hmi_channel` 由 **HMI 本地覆盖**为自身 [`ChannelStatus`]
/// （服务端给 `Unknown` —— 它无从知道客户端自己的连通性）。语义要点：
/// **`Init` 不得映射为「已连接」**（尚未首成功 ⇒ 「连接中」），**`Down` 不得映射为「正常」**
/// （与 F6.5「`Unknown`/`NotConfigured` 绝不落入正常」同一取向）。
pub fn hmi_link_state(status: ChannelStatus) -> LinkState {
    match status {
        ChannelStatus::Init => LinkState::Connecting,
        ChannelStatus::Connected => LinkState::Connected,
        ChannelStatus::Down => LinkState::Disconnected,
        // **版本不匹配 ⇒ 「未知」（不得落「已连接」）**：链路本身可能是通的，但**帧不可用**
        // ⇒ 既非 Connected 也非 Disconnected。版本不匹配的**专属文案**由
        // [`ScreenMode::VersionMismatch`] 承担（§15.6.2 ③），本枚举只表达"链路态不可信"。
        ChannelStatus::Incompatible { .. } => LinkState::Unknown,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// v2.0 / B3-1：控制通道侧状态（`ControlState`）
//
// **为什么不是设计 §5.4 草图的单体 `UiState`**（偏差登记，见本文件末尾「B3-1 偏差」表）：
// 草图里的 `frame` / `channel` / SOC / 三相 `NumView` **已经**由 v1.0 的 [`DisplayState`] +
// [`UiSnapshot`] 承担（逐条保留，未动）；`toast` / `confirm` 在草图中写作**LVGL 句柄**
// （`Option<Toast>` / `Option<ConfirmDialog>`），而本文件是**纯逻辑、零 LVGL** —— 句柄归页面
// 持有（`p2_config` 的 `toast` 字段、各页的 `ConfirmDialog`）。故 v2 新增的控制侧状态单独成
// 一个容器，**不**把帧派生视图搬第二遍（避免双份真源）。
// ═══════════════════════════════════════════════════════════════════════════

/// Toast 存活时长（**3 s 自动消失**，F9.5 / 设计 §5.4）。
pub const TOAST_TTL_MS: u64 = 3_000;

/// 控制通道**传输层**失败的上屏兜底文案。
///
/// 既有串里没有「请求超时 / 连接失败 / 响应异常」一类专串（UI §3.6 用字表逐字核对），
/// 故取全局 Toast 行的「操作失败」—— **不臆造新串**。具体分类仍由
/// `crate::console::ConsoleError` 承载（诊断 / 日志用），上屏不区分细分原因。
pub const TRANSPORT_FAIL_TEXT: &str = TEXT_TOAST_FAIL;

/// [`crate::console::ConsoleError`] → **专属**上屏文案（`None` = 无专属出路，用通用兜底
/// [`TRANSPORT_FAIL_TEXT`]）。
///
/// # 为什么只有 `RetryWindowExpired` 有专属串（B3-2c）
///
/// 其余传输层错误（超时 / 连接失败 / 非 200 / 解码失败）的处置**都一样**（重发即可），
/// 而 `RetryWindowExpired` 的处置**不一样**：原样重发**必被服务端防重放窗口先拒**，
/// 唯一出路是**当作新操作**重发（新 uuid）并按 T-3 **重新确认** —— 通用「操作失败」会
/// 让现场以为"什么都没发生"而再点一次（`console.rs` 模块头第 6 条登记的静默语义偏差）。
///
/// # 可达性（**如实登记，不得高估**）
///
/// 该错误在当前生产路径上**不可达**：它唯一的产生点是
/// [`crate::console::ConsoleClient::retry`]，而 `app` 层**从不调用** `retry`
/// （`begin_*` 按设计不校验重放窗口）。本函数把**映射**建好 ⇒ 一旦接线层将来补上
/// 「重试是显式动作」的入口，文案自动生效（无需再改本层）。
///
/// ⚠️ **文案字面量的唯一落点在 `ui/**`**（码表静态网只扫那 13 个文件）；本层只**转出**。
pub fn console_error_text(e: &crate::console::ConsoleError) -> Option<&'static str> {
    match e {
        crate::console::ConsoleError::RetryWindowExpired { .. } => Some(TEXT_RETRY_EXPIRED),
        _ => None,
    }
}

/// [`ControlCode`] → 上屏兜底文案（**只在服务端 `message` 为空时使用**）。
///
/// - `Ok` → `None`：**成功文案按操作而异**（P2「保存成功 · 已生效」/ P4「已释放联锁」…），
///   由**页面**在 `show_result` 里给定（页面是真源，本层不另存一份 ⇒ 免双份真源）；
/// - `AuditUnavailable` → EDGE-18 固定串（**不取 `message`**，见 [`LastControlResult::toast_text`]）；
/// - `Busy` / `Internal` → 有**语义精确**的既有串，直接用；
/// - 其余拒绝类（前置条件 / 校验 / 生效失败 / 后端不可用）→ 「操作失败」；
///   **具体原因在 `message` 里**（EDGE-10 / EDGE-12 要求明示原因），本兜底不冒充原因。
pub fn control_code_text(code: ControlCode) -> Option<&'static str> {
    match code {
        ControlCode::Ok => None,
        ControlCode::RejectedPrecondition
        | ControlCode::RejectedValidation
        | ControlCode::ApplyFailed
        | ControlCode::Unavailable => Some(TEXT_TOAST_FAIL),
        ControlCode::AuditUnavailable => Some(TEXT_AUDIT_UNAVAILABLE),
        ControlCode::Busy => Some(TEXT_OP_BUSY),
        ControlCode::Internal => Some(TEXT_INTERNAL),
    }
}

/// 在途控制请求（`None` = 无在飞请求）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InflightRequest {
    /// 目标端点（决定成功文案 / 回执形状）。
    pub endpoint: ConsoleEndpoint,
    /// 信封 `request_id`（查询端点无信封 ⇒ `None`）。
    pub request_id: Option<String>,
}

impl InflightRequest {
    /// 信封 `op`（查询端点 ⇒ `None`）。
    pub fn op(&self) -> Option<&'static str> {
        self.endpoint.op_name()
    }
}

/// 最近一次控制回执的**摘要**（原始 DTO 归调用方 / 页面；本层只留总线上屏与诊断要用的字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastControlResult {
    /// 回执 `request_id`。
    pub request_id: String,
    /// 是否成功（**真值判据**）。
    pub ok: bool,
    /// 结构化错误码。
    pub code: ControlCode,
    /// 服务端人读消息（失败时即 EDGE-10 / EDGE-12 的「具体原因」）。
    pub message: String,
    /// 审计记录 ID（成功与失败均返回）。
    pub audit_id: Option<String>,
    /// **幂等命中标记**：`true` 表示本条是重复请求的**首次原始结果**（**不是错误**）。
    pub duplicate: bool,
    /// 服务端回执时刻（Unix ms）。
    pub at_ms: u64,
}

impl LastControlResult {
    /// 由回执 DTO 取摘要（**原样搬运 `duplicate`**，不得当成错误或丢弃）。
    pub fn from_response<T>(resp: &ControlResponse<T>) -> Self {
        Self {
            request_id: resp.request_id.clone(),
            ok: resp.ok,
            code: resp.code,
            message: resp.message.clone(),
            audit_id: resp.audit_id.clone(),
            duplicate: resp.duplicate,
            at_ms: resp.at_ms,
        }
    }

    /// 上屏文案（**唯一规则**；`None` = 本层不弹 Toast）：
    ///
    /// 1. `AuditUnavailable` → **EDGE-18 固定串**「审计不可用 · 操作未执行」（fail-closed；
    ///    **不取** `message` —— 该行的判据原文就是这条固定 Toast，见 UI §8.3）；
    /// 2. `Ok` → `None`（成功文案按操作由**页面**给定）；
    /// 3. 其余 → 服务端 `message`（EDGE-10 / EDGE-12 的「具体原因」）经
    ///    [`crate::ui::pages::display_safe`] 过滤（**ASCII 大写化 / `-`→`–` 等逐字符改写**），
    ///    `message` 为空才回落 [`control_code_text`]。
    ///
    /// ⚠️ **口径订正（B3-1 评审重要 5）**：此处**曾**声称过滤后"字符 ⊆ cmap"，**该保证不成立** ——
    /// [`display_safe`] 对**非 ASCII 原样透传**，其自身文档亦写明「**不**处理自由文本」
    /// （`ui/pages/mod.rs`）⇒ **服务端 `message` 里的 cmap 外汉字仍会出豆腐块**。
    /// 这是 `p2_config` / `p4_interlock` 的**既有共有局限**（本单元不修），但**不得**留着
    /// 不实的保证声明。
    pub fn toast_text(&self) -> Option<String> {
        if self.code == ControlCode::AuditUnavailable {
            return Some(TEXT_AUDIT_UNAVAILABLE.to_string());
        }
        if self.code == ControlCode::Ok {
            return None;
        }
        let msg = self.message.trim();
        if msg.is_empty() {
            return control_code_text(self.code).map(str::to_string);
        }
        Some(crate::ui::pages::display_safe(msg))
    }
}

/// 一条 Toast 的**纯逻辑**记录（3 s 生命周期；LVGL 侧的 `Toast` 句柄归页面持有）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastRecord {
    text: String,
    until_ms: u64,
}

impl ToastRecord {
    /// 以 `now_ms` 为起点建一条（存活 [`TOAST_TTL_MS`]）。
    pub fn new(text: impl Into<String>, now_ms: u64) -> Self {
        Self {
            text: text.into(),
            until_ms: now_ms.saturating_add(TOAST_TTL_MS),
        }
    }

    /// 文案（cmap 保证的**实际边界**见 [`LastControlResult::toast_text`] 的口径订正）。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 过期时刻（Unix ms）。
    pub fn until_ms(&self) -> u64 {
        self.until_ms
    }

    /// 是否已到期（`now ≥ until`）。
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.until_ms
    }
}

/// 控制通道 + 外壳层所需的状态（B3-1）。
///
/// **不含**：帧派生视图（归 [`DisplayState`] / [`UiSnapshot`]）、LVGL 句柄（归页面）、
/// 传输层累计失败计数（归 `crate::console::ConsoleClient::fail_streak()` —— 同一事实不记两份）。
#[derive(Debug, Default)]
pub struct ControlState {
    inflight: Option<InflightRequest>,
    last: Option<LastControlResult>,
    /// 最近一次**传输层**失败的注入时刻（`None` = 从未失败）；用于「刚失败过」提示与诊断。
    last_transport_failure_ms: Option<u64>,
    toast: Option<ToastRecord>,
    /// 模态确认弹层（`Some` = 打开）：`Shell::set_modal_open(state.confirm().is_some())`
    /// 是已登记契约（`ui/shell.rs` 偏差 **SH2**）。
    ///
    /// **本层不持有「配置页有未保存修改」的镜像**：该事实的真源是
    /// `P2ConfigPage::is_dirty()`，`Shell::tick` 每拍直读（见偏差表 **S-2**）。
    confirm: Option<ConsoleEndpoint>,
}

impl ControlState {
    /// 空状态。
    pub fn new() -> Self {
        Self::default()
    }

    // ── 在途 ──────────────────────────────────────────────────────────────

    /// 记一条在途请求（由接线层在 `ConsoleClient::begin_*` 成功后调用）。
    pub fn begin(&mut self, endpoint: ConsoleEndpoint, request_id: Option<&str>) {
        self.inflight = Some(InflightRequest {
            endpoint,
            request_id: request_id.map(str::to_string),
        });
    }

    /// 在途请求（`None` = 无在飞）。
    pub fn inflight(&self) -> Option<&InflightRequest> {
        self.inflight.as_ref()
    }

    /// 是否有在途请求。
    pub fn is_busy(&self) -> bool {
        self.inflight.is_some()
    }

    /// **清在途**（B3-2b-2 新增）：**查询**成功路径用。
    ///
    /// 为什么需要它：[`Self::record_response`] 的入参是 `ControlResponse<T>` **信封**，而按契约
    /// **查询端点返回裸 DTO**（设计 §3.4：信封只用于 POST 写操作）⇒ 查询完成时**没有**能传给
    /// `record_response` 的东西，而 [`Self::record_transport_failure`] 会误记一次失败并弹
    /// 「操作失败」。缺了本方法，接线层只能在"查询永远算在途"与"谎记一次失败"之间二选一。
    ///
    /// **语义**：只动"在途"这一格 —— **不碰** `last`（最近一次**回执**摘要仍归写操作）、
    /// 不碰 Toast、不碰 `last_transport_failure_ms`。
    pub fn finish(&mut self) {
        self.inflight = None;
    }

    // ── 结果 ──────────────────────────────────────────────────────────────

    /// 记一次回执（**清在途** + 存摘要 + 按需弹 Toast）。
    ///
    /// `duplicate=true` **原样保留**且**不**影响 Toast 规则（幂等命中是成功路径的一种）。
    ///
    /// ⚠️ **谁该用它**：只有当这条回执**没有**别的上屏通道时才用本方法。**写端点**（P2 配置
    /// 保存 / P4 两个联锁写）的回执会被接线层送进页面 `show_result`（页面自己弹 Toast）⇒ 用
    /// [`Self::record_response_without_toast`]，否则同拍两条 Toast（见该方法的说明）。
    pub fn record_response<T>(&mut self, resp: &ControlResponse<T>, now_ms: u64) {
        let summary = LastControlResult::from_response(resp);
        if let Some(text) = summary.toast_text() {
            self.push_toast(text, now_ms);
        }
        self.last = Some(summary);
        self.inflight = None;
    }

    /// **记回执的状态、不压 Toast**（B3-2c 整改 **阻塞项**；与
    /// [`Self::record_failure_state`] / [`Self::record_transport_failure_with_text`] 的
    /// 切分**同款**：状态记账与"要不要弹 Toast"分成两处）。
    ///
    /// # 为什么需要它（同拍双 Toast）
    ///
    /// [`LastControlResult::toast_text`] 对**一切非 `Ok` 的 `ControlCode`** 与**任何非空
    /// `message`** 都返回 `Some` ⇒ 生产高频的失败回执（P2 字段校验被拒 / P4 前置条件被拒）
    /// 会让本层弹一条；而接线层**紧接着**把**同一份回执**送进 P2 / P4 的 `show_result`
    /// （页面各自 `show_toast` 再弹一条）⇒ **同拍两条 Toast** 同挂 `lv_layer_top()`、
    /// 同坐标（`Dimens::TOAST_X/Y`）同文案 —— 违 UI §7.2「同一时刻仅 1 条」。
    ///
    /// **PM 裁定（B3-2c 整改）**：**页面负责上屏、状态层只记账**。写端点走本方法；
    /// 读端点（**没有**页面回执通道）仍走 [`Self::record_response`] 保留兜底 Toast。
    ///
    /// **记账一个不少**：`last`（最近回执摘要）与"清在途"照记 —— 唯一不碰的是 `toast`
    /// 这一格（状态部分与 [`Self::record_response`] **逐字相同**）。
    ///
    /// ⚠️ **为什么入参没有 `now_ms`**：本方法不产生时间信息（`at_ms` 来自回执本身，
    /// Toast 的到期时刻归 [`Self::record_response`] 压的那条）。
    ///
    /// **改什么会让本条变红**：把 [`crate::app::record_receipt`] 的写端点分支换回
    /// `record_response` ⇒ `ui/tests.rs` 的
    /// `write_receipt_is_shown_by_the_page_and_not_by_the_state_layer` 当场红。
    pub fn record_response_without_toast<T>(&mut self, resp: &ControlResponse<T>) {
        self.last = Some(LastControlResult::from_response(resp));
        self.inflight = None;
    }

    /// 记一次**传输层失败**（超时 / 连接失败 / 非 200 / 解码失败）。
    ///
    /// 清在途、记失败时刻、弹兜底 Toast（[`TRANSPORT_FAIL_TEXT`]）。**累计次数**不在此记
    /// —— 那是 `ConsoleClient::fail_streak()` 的活（同一事实不记两份）。
    pub fn record_transport_failure(&mut self, now_ms: u64) {
        self.record_transport_failure_with_text(now_ms, TRANSPORT_FAIL_TEXT);
    }

    /// 同 [`Self::record_transport_failure`]，但**上屏文案由调用方给**（B3-2c）。
    ///
    /// 用途：有**专属出路**的错误（今仅 `RetryWindowExpired`，判据由
    /// [`console_error_text`] 单一分派）不该被通用「操作失败」盖掉 —— 接线层用
    /// `crate::app::transport_failure_text(&e)` 取文案后从这里灌进来，
    /// **其余错误照旧回落** [`TRANSPORT_FAIL_TEXT`]（`record_transport_failure` 即其别名路径）。
    pub fn record_transport_failure_with_text(&mut self, now_ms: u64, text: &str) {
        self.record_failure_state(now_ms);
        self.push_toast(text, now_ms);
    }

    /// **记失败的状态、不压 Toast**（B3-2c 整改 **重要 4**；见
    /// [`Self::record_transport_failure_with_receipt`] 的"为何恰好一条"）。
    ///
    /// 状态部分与 [`Self::record_transport_failure_with_text`] **逐字相同**
    /// （清在途 + 记失败时刻）—— 唯一差别是**不碰** `toast` 这一格。
    fn record_failure_state(&mut self, now_ms: u64) {
        self.inflight = None;
        self.last_transport_failure_ms = Some(now_ms);
    }

    /// 传输失败 + **给页面补一条本地合成的「不可用」回执**（B3-2b-2 整改 · PM 裁定 3）。
    ///
    /// # 为什么需要它（降级必须有**上屏**出口）
    ///
    /// [`Self::record_transport_failure`] 只把失败记进本层（清在途 + 一条兜底 Toast），而
    /// **Toast 的 LVGL 句柄归页面、本层这条没有任何页面暴露通用入口** ⇒ 用户按「保存」/
    /// 「人工释放联锁」遇控制通道挂掉时，**屏上什么都不发生**（只落 stderr）—— 违 §2.6
    /// 「降级可见」。故本方法把**同一个失败**同时交回一条**本地合成**的回执，由接线层送进
    /// 页面**既有**的 `show_result`（`src/ui/**` 零改动，见
    /// [`crate::control_route::transport_failure_decision`] 的字段取值理由）。
    ///
    /// **返回值**：`Some(决策)` = 该次在途请求是**写**端点（P2 配置保存 / P4 两个联锁写），
    /// 须由调用方送进唯一分派点；`None` = 读端点（其降级出口是 P3 通道条态）或**当时无在途**。
    ///
    /// # 两个生产调用点（**PM 裁定 2 · 2026-09-16 · SH20**）
    ///
    /// ① 传输失败（连接 / 超时 / 非 200 / 解码）；
    /// ② [`crate::control_route::route`] 自己的 `Err`（回执**形态 / 解码**不符）——
    ///    **写端点同样走页面通道**：app 层兜底 Toast 与确认弹层同挂 `lv_layer_top()` 而弹层
    ///    建得晚 ⇒ 弹层打开期间被完全遮住（`ui/shell.rs` 偏差 **SH20**），而页面 Toast 建在
    ///    弹层之后 ⇒ 可见。两处调用点都在 [`crate::app::App::absorb_console`]，**同一份机制**，
    ///    不为 ② 另造一份合成逻辑。
    ///
    /// ⚠️ **返回值必须被消费**：只造不送 = 屏上依旧什么都不发生。这条约束由**显式的**
    /// `#[must_use]`（见下行）在**编译期**兜住 —— 判据是"零警告"。⚠️ **不要**依赖
    /// "`Option` 自带 `must_use`"这一常见说法：本工具链实测**不成立**（裸调用
    /// `self.control.record_transport_failure_with_receipt(..);` 不报任何告警，
    /// B3-2b-2 整改时实测过），故此处**必须**写属性而不是靠类型。
    ///
    /// **与 [`Self::record_transport_failure`] 的关系**：状态部分**内含**它（不另记一份失败、
    /// 不另记一个失败时刻 —— 同一事件只有一份记账），只是在清在途**之前**先把"是哪一次在途
    /// 请求"取出来，再据此合成回执。
    ///
    /// # 为何**恰好一条** Toast（B3-2c 整改 **重要 4**，PM 裁定）
    ///
    /// 写端点：失败**已经**由合成回执经页面 `show_result` 上屏（页面自己的 `show_toast`）。
    /// 若本方法照旧再 `push_toast` 一条，则**同拍**会有**两个** Toast 对象同挂 `layer_top`、
    /// 同坐标同文案 ⇒ 违 UI §7.2「同一时刻仅 1 条」。故写端点走
    /// [`Self::record_failure_state`]（**不** `push_toast`）；**失败记账一个不少**
    /// （在途清空 / `last_transport_failure_ms` 照记）。
    ///
    /// 无在途（读端点从不入 `inflight`）：**没有**合成回执可送 ⇒ 此时再不给 app 层 Toast
    /// 就变成**屏上什么都不发生**（违 §2.6）⇒ 该分支**保留** app 层兜底 Toast。
    /// 两条分支各自**恰有一条**上屏路径。
    #[must_use = "本地合成回执必须送进页面（只造不送 = 屏上什么都不发生）；调用方须把它交给 `apply_route`"]
    pub fn record_transport_failure_with_receipt(&mut self, now_ms: u64) -> Option<RouteDecision> {
        // 在途信息必须在下面那句之前取 —— `record_failure_state` 会**清掉在途**。
        let ep = self.inflight.as_ref().map(|i| i.endpoint);
        let rid = self
            .inflight
            .as_ref()
            .and_then(|i| i.request_id.clone())
            .unwrap_or_default();
        let Some(ep) = ep else {
            // 无在途（读端点从不入 `inflight`）⇒ 没有"哪一次操作"可言 ⇒ 不合成；
            // 也没有页面上屏出口 ⇒ 由 app 层 Toast 承担（恰一条）。
            self.record_transport_failure(now_ms);
            return None;
        };
        // 写端点：**只记状态**，失败由上屏的合成回执（页面 `show_result`）承担 —— 见上文。
        self.record_failure_state(now_ms);
        crate::control_route::transport_failure_decision(ep, &rid, TRANSPORT_FAIL_TEXT, now_ms)
    }

    /// 最近一次回执摘要。
    pub fn last(&self) -> Option<&LastControlResult> {
        self.last.as_ref()
    }

    /// 最近一次传输层失败的注入时刻。
    pub fn last_transport_failure_ms(&self) -> Option<u64> {
        self.last_transport_failure_ms
    }

    // ── Toast ─────────────────────────────────────────────────────────────

    /// 弹一条 Toast（同一时刻仅 1 条：新的覆盖旧的，UI §7.2）。
    pub fn push_toast(&mut self, text: impl Into<String>, now_ms: u64) {
        self.toast = Some(ToastRecord::new(text, now_ms));
    }

    /// 当前 Toast。
    pub fn toast(&self) -> Option<&ToastRecord> {
        self.toast.as_ref()
    }

    /// 当前 Toast 文案。
    pub fn toast_text(&self) -> Option<&str> {
        self.toast.as_ref().map(ToastRecord::text)
    }

    /// 是否已到期（调用方据此决定是否清理；返回 `false` 表示没有 Toast）。
    pub fn toast_expired(&self, now_ms: u64) -> bool {
        self.toast.as_ref().is_some_and(|t| t.is_expired(now_ms))
    }

    /// 每拍调用：到期即清（**3 s 自动消失**）。返回本次是否清掉了。
    pub fn expire_toast(&mut self, now_ms: u64) -> bool {
        if self.toast_expired(now_ms) {
            self.toast = None;
            return true;
        }
        false
    }

    // ── 外壳层 ────────────────────────────────────────────────────────────

    /// 模态确认弹层（`Some` = 打开）。
    ///
    /// ⚠️ **B3-2c 订正（如实登记，勿高估）**：本节三个方法（`confirm` / `set_confirm` /
    /// `confirm_open`）**零生产调用者** —— `set_confirm` 只在 `state.rs` 自己的用例里被调，
    /// 而 [`Self::confirm_open`] 原先的唯一生产调用点（`app.rs::tick` 喂
    /// `Shell::set_modal_open`）已改为直读**页面**的生产可见查询口
    /// （`P2ConfigPage::dialog_open` / `P4InterlockPage::dialog_open`）。
    ///
    /// **为什么改读页面**：弹层的**生命周期**（建 / 关，且关闭延迟到下一拍）完全由页面掌握，
    /// 接线层看不到"用户按下按钮"的那一刻 ⇒ 本层的 `confirm` 结构上**永远拿不到生产者**，
    /// 恒 `None` 的"权威真源"比没有更危险（下一手读者会以为它在工作）。设计 §5.4 把
    /// `UiState.confirm` 写作权威真源的前提是"句柄归状态层"，而本仓的句柄**归页面**
    /// （本文件零 LVGL，见本模块头 S-3）—— 真源因此必须在页面。
    pub fn confirm(&self) -> Option<ConsoleEndpoint> {
        self.confirm
    }

    /// 设置 / 清除模态弹层标记（接线层在**打开 / 关闭页面弹层的同一处**调用）。
    ///
    /// ⚠️ 同上：B3-2c 后**无生产调用者**。保留是**有意的**（设计与 `ui/shell.rs` 偏差 SH2
    /// 都登记过这条契约；删除须先订正文档）。
    pub fn set_confirm(&mut self, endpoint: Option<ConsoleEndpoint>) {
        self.confirm = endpoint;
    }

    /// 是否有模态弹层打开（= `Shell::set_modal_open(..)` 的取值）。
    ///
    /// ⚠️ 同上：**生产路径已不再使用**本谓词（改读页面的 `dialog_open()`）。
    pub fn confirm_open(&self) -> bool {
        self.confirm.is_some()
    }
}

impl DisplayState {
    /// **本地覆盖** `DeviceSection.hmi_channel`（设计 §5.4 / §5.5）。
    ///
    /// 由接线层在**每拍渲染前**调用：值按**当前**通道态（`channel_status(now_ms)`）重算并写入
    /// 最近帧 —— 因此通道转 `Down` 后即刻变「断开」，**不会**跟着冻帧停在「已连接」
    /// （若改在 `record_success` 里写死，就会随冻帧一起变旧 —— 那正是「由服务端报告客户端
    /// 自己的连接状态」这类语义倒置的另一副面孔）。无帧时是 no-op（尚无内容可覆盖）。
    pub fn apply_hmi_channel(&mut self, now_ms: u64) {
        let state = hmi_link_state(self.channel_status(now_ms));
        if let Some(f) = self.frame.as_mut() {
            f.device.hmi_channel = state;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_route::RouteDecision;
    use mupc_display_proto::{DisplayFrame, Field};

    fn frame(seq: u64, ts_ms: u64) -> DisplayFrame {
        DisplayFrame {
            version: mupc_display_proto::PROTO_VERSION,
            // v3 新增段（§15.2.2）：本用例只关心既有字段 ⇒ 取默认（available=false）
            peripherals: Default::default(),
            seq,
            ts_ms,
            soc: Some(65.0),
            soc_source: SocSource::Bms,
            soc_flag: FieldFlag::Valid,
            run_state: Some(RunState::Charge),
            pcs_online: true,
            p_phase: [Field { v: Some(12.3), flag: FieldFlag::Valid }; 3],
            p_total: Field { v: Some(36.1), flag: FieldFlag::Valid },
            i_phase: [Field { v: Some(22.5), flag: FieldFlag::Valid }; 3],
            inconsistency: false,
            // v2 契约新增的四段：本测试桩只关心 v1 字段，四段一律取契约缺省
            // （`DeviceSection` 等均 `#[serde(default)]` + `Default`，语义 = 「未提供」）。
            device: Default::default(),
            alarms: Default::default(),
            info: Default::default(),
            interlock: Default::default(),
        }
    }

    // ---- 三态归一 ----
    #[test]
    fn num_view_valid_value() {
        let f = Field { v: Some(12.3), flag: FieldFlag::Valid };
        assert_eq!(NumView::from_field(&f), NumView::Value(12.3));
    }

    #[test]
    fn num_view_degraded_flags_map_to_dash() {
        for flag in [
            FieldFlag::NotRead,
            FieldFlag::Offline,
            FieldFlag::RangeError,
        ] {
            let f = Field { v: None, flag };
            assert_eq!(NumView::from_field(&f), NumView::Dash(flag));
        }
        // Valid 但无值 → RangeError 兜底（不补 0）
        let f = Field { v: None, flag: FieldFlag::Valid };
        assert_eq!(NumView::from_field(&f), NumView::Dash(FieldFlag::RangeError));
    }

    #[test]
    fn dash_badge_words_match_ui() {
        assert_eq!(dash_badge(FieldFlag::NotRead), "未取数");
        assert_eq!(dash_badge(FieldFlag::Offline), "源离线");
        assert_eq!(dash_badge(FieldFlag::RangeError), "数据异常");
        assert_eq!(dash_badge(FieldFlag::Valid), "");
    }

    #[test]
    fn soc_band_thresholds() {
        assert_eq!(soc_band(0.0), SocBand::Low);
        assert_eq!(soc_band(15.0), SocBand::Low);
        assert_eq!(soc_band(50.0), SocBand::Mid);
        assert_eq!(soc_band(85.0), SocBand::High);
        assert_eq!(soc_band(100.0), SocBand::High);
    }

    #[test]
    fn snapshot_soc_lost_when_none_or_source_lost() {
        let mut st = DisplayState::new();
        let mut f = frame(1, 1000);
        f.soc = None;
        f.soc_source = SocSource::Lost;
        st.record_success(f, 1000);
        let snap = st.snapshot(1000);
        assert_eq!(snap.soc, SocView::Lost);
        assert_eq!(snap.soc_source, SocSource::Lost);
    }

    // ---- 通道态 / 新鲜度 ----
    #[test]
    fn channel_init_then_connected() {
        let st = DisplayState::new();
        assert_eq!(st.channel_status(0), ChannelStatus::Init);
        let mut st = st;
        st.record_fail(0);
        assert_eq!(st.channel_status(500), ChannelStatus::Init);
        st.record_success(frame(1, 500), 500);
        assert_eq!(st.channel_status(2500), ChannelStatus::Connected);
        // 超过 3s 无成功 → Down（即使最后一次是成功的，只要没再来成功）
        assert_eq!(st.channel_status(500 + CHANNEL_DOWN_MS), ChannelStatus::Down);
    }

    #[test]
    fn init_timeout_becomes_down() {
        let mut st = DisplayState::new();
        st.record_fail(0);
        // 尚未 3s → 仍 Init
        assert_eq!(st.channel_status(1000), ChannelStatus::Init);
        // ≥3s 仍无成功 → Down
        assert_eq!(st.channel_status(CHANNEL_DOWN_MS), ChannelStatus::Down);
    }

    #[test]
    fn reconnect_after_down_recovers_on_next_success() {
        let mut st = DisplayState::new();
        st.record_success(frame(1, 0), 0);
        assert_eq!(st.channel_status(10000), ChannelStatus::Down);
        st.record_success(frame(2, 10000), 10000);
        assert_eq!(st.channel_status(10500), ChannelStatus::Connected);
    }

    #[test]
    fn freshness_stale_after_threshold() {
        let mut st = DisplayState::new();
        st.set_stale_ms(2000);
        st.record_success(frame(1, 1000), 1000);
        assert_eq!(st.freshness(2000), Freshness::Fresh);
        // now - ts = 2500 > 2000
        assert_eq!(st.freshness(1000 + 2500), Freshness::Stale);
    }

    /// W3：旧帧（seq 与 ts 双双回退）丢弃——屏面不回退到旧值，但通道仍记成功。
    #[test]
    fn out_of_order_older_frame_is_dropped_but_channel_ok() {
        let mut st = DisplayState::new();
        st.record_success(frame(10, 10_000), 10_000);
        // 乱序旧帧：seq 9 + ts 更旧 → 丢弃
        st.record_success(frame(9, 9_000), 10_100);
        assert_eq!(st.frame().unwrap().seq, 10, "更旧的帧不得覆盖较新帧");
        assert_eq!(st.reorder_dropped(), 1);
        assert_eq!(st.fail_streak(), 0, "通道是通的，仍记成功");
        assert_eq!(st.last_ok_ms(), Some(10_100));
        // 同 seq 重取（轮询同一帧）→ 幂等接受，不计乱序
        st.record_success(frame(10, 10_000), 10_200);
        assert_eq!(st.reorder_dropped(), 1);
    }

    /// W3：mupcd 重启 → seq 清零但 ts 更新 → 必须接受（否则屏面冻结到 seq 追平旧值）。
    #[test]
    fn seq_restart_with_newer_ts_is_accepted() {
        let mut st = DisplayState::new();
        st.record_success(frame(86_400, 10_000), 10_000);
        st.record_success(frame(0, 10_100), 10_100); // seq 回退但 ts 更新 = 新发布周期
        assert_eq!(st.frame().unwrap().seq, 0);
        assert_eq!(st.reorder_dropped(), 0, "重启清零不得被当作乱序丢弃");
    }

    #[test]
    fn fail_streak_preserves_last_frame() {
        let mut st = DisplayState::new();
        st.record_success(frame(5, 0), 0);
        st.record_fail(10);
        st.record_fail(510);
        st.record_fail(1010);
        assert_eq!(st.fail_streak(), 3);
        // 帧保留（冻结展示用），通道断但不清数值
        assert_eq!(st.frame().unwrap().seq, 5);
        assert_eq!(st.screen_mode(1010 + CHANNEL_DOWN_MS), ScreenMode::ChannelDown);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // B3-1：`hmi_channel` 本地覆盖
    // ═══════════════════════════════════════════════════════════════════════

    /// 三态映射：**`Init` 不得映射为「已连接」**，`Down` 不得映射为「已连接 / 未配置」。
    #[test]
    fn hmi_link_state_never_falls_into_connected() {
        assert_eq!(hmi_link_state(ChannelStatus::Init), LinkState::Connecting);
        assert_eq!(hmi_link_state(ChannelStatus::Connected), LinkState::Connected);
        assert_eq!(hmi_link_state(ChannelStatus::Down), LinkState::Disconnected);
        assert_ne!(hmi_link_state(ChannelStatus::Init), LinkState::Connected);
        assert_ne!(hmi_link_state(ChannelStatus::Down), LinkState::Connected);
        // 展示词与 UI §3.6 用字表一致（「连接中」/「已连接」/「断开」）
        assert_eq!(LinkState::Connecting.display_name(), "连接中");
        assert_eq!(LinkState::Connected.display_name(), "已连接");
        assert_eq!(LinkState::Disconnected.display_name(), "断开");
    }

    /// **本地覆盖按「当前通道态」重算，不跟着冻帧变旧**。
    ///
    /// **改什么会让本条变红**：把覆盖挪进 `record_success`（写死成收到帧那一刻的状态）⇒
    /// 第三条断言拿到 `Connected`（冻帧值）而不是 `Disconnected`。
    #[test]
    fn apply_hmi_channel_follows_current_status_not_the_frozen_frame() {
        let mut st = DisplayState::new();
        st.apply_hmi_channel(0); // 尚无帧 ⇒ no-op（不得 panic）
        assert!(st.frame().is_none());

        st.record_success(frame(1, 1_000), 1_000);
        st.apply_hmi_channel(1_000);
        assert_eq!(
            st.frame().unwrap().device.hmi_channel,
            LinkState::Connected,
            "通道通 ⇒ 覆盖为「已连接」"
        );

        // 超过 3 s 无成功 ⇒ 通道 Down；冻帧仍是老帧，但覆盖值必须变「断开」
        st.apply_hmi_channel(1_000 + CHANNEL_DOWN_MS);
        assert_eq!(
            st.frame().unwrap().device.hmi_channel,
            LinkState::Disconnected,
            "通道已断，`hmi_channel` 不得停在冻帧里的「已连接」"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // B3-1：回执 → 上屏文案
    // ═══════════════════════════════════════════════════════════════════════

    /// **本层不新增任何上屏字面量**：每个出口都必须与 `ui/**` 的既有字面量**逐字相等**。
    ///
    /// **改什么会让本条变红**：把任一支改成新造串（如 `ControlCode::Busy => Some("请求处理中")`）
    /// ⇒ 对应断言红（那条串不在 `ui/**` 里，码表静态网扫不到它 = 真机会出豆腐块）。
    #[test]
    fn control_texts_are_aliases_of_existing_ui_literals() {
        assert_eq!(
            control_code_text(ControlCode::AuditUnavailable),
            Some(crate::ui::pages::p2_config::TEXT_AUDIT_UNAVAILABLE)
        );
        assert_eq!(
            control_code_text(ControlCode::Busy),
            Some(crate::ui::pages::p4_interlock::TEXT_OP_BUSY)
        );
        assert_eq!(
            control_code_text(ControlCode::Internal),
            Some(crate::ui::pages::p4_interlock::TEXT_INTERNAL)
        );
        for code in [
            ControlCode::RejectedPrecondition,
            ControlCode::RejectedValidation,
            ControlCode::ApplyFailed,
            ControlCode::Unavailable,
        ] {
            assert_eq!(
                control_code_text(code),
                Some(crate::ui::pages::p4_interlock::TEXT_TOAST_FAIL),
                "{code:?} 的兜底文案必须取自 UI 既有串"
            );
        }
        assert_eq!(
            control_code_text(ControlCode::Ok),
            None,
            "成功文案按操作由**页面**给定（页面是真源，本层不另存一份）"
        );
        assert_eq!(
            TRANSPORT_FAIL_TEXT,
            crate::ui::pages::p4_interlock::TEXT_TOAST_FAIL
        );
    }

    /// **`RetryWindowExpired` 有专属上屏文案**（不是通用「操作失败」）：它必须点明出路
    /// （作为新操作重发 + 重新确认），否则现场会把它当成"没发生"再点一次
    /// （`console.rs` 模块头第 6 条登记的那条静默语义偏差）。
    ///
    /// **改什么会让本条变红**：
    /// - 删掉 `console_error_text` 的 `RetryWindowExpired` 分支（回落 `None`）⇒ 第 1 条红；
    /// - 把它改成 `Some(TEXT_TOAST_FAIL)` ⇒ 第 2 条红（"有专属文案"变成谎话）；
    /// - 把文案写进 `state.rs` 而不是转出 `ui/**` 的常量 ⇒ 第 3 条红（码表网扫不到 = 真机豆腐块）。
    #[test]
    fn retry_window_expired_has_its_own_screen_text() {
        let e = crate::console::ConsoleError::RetryWindowExpired {
            op: "apply".to_string(),
            issued_at_ms: 0,
            age_ms: 30_000,
            window_ms: 30_000,
        };
        // ① 有专属文案（不是"没有专属出路"）
        let text = console_error_text(&e).expect("RetryWindowExpired 必须有专属上屏文案");
        // ② 与通用兜底**不同**（否则"专属"是空话）
        assert_ne!(text, TRANSPORT_FAIL_TEXT, "不得落到通用「操作失败」");
        // ③ 逐字取自 `ui/**` 的既有字面量（本层不得自造上屏字）
        assert_eq!(text, crate::ui::pages::p4_interlock::TEXT_RETRY_EXPIRED);
        // ④ 文案点明出路
        assert!(text.contains("重新确认"), "必须点明 T-3 的出路：{text}");
        // ⑤ 对偶：**其余** ConsoleError 没有专属文案（回落通用兜底，不得乱套专属串）
        assert_eq!(console_error_text(&crate::console::ConsoleError::Idle), None);
        assert_eq!(
            console_error_text(&crate::console::ConsoleError::Busy("apply".into())),
            None
        );
    }

    /// EDGE-18：审计不可写 → **固定串**「审计不可用 · 操作未执行」（**不取**服务端 `message`）。
    #[test]
    fn audit_unavailable_maps_to_the_edge18_fixed_string() {
        let resp: ControlResponse<serde_json::Value> = ControlResponse::audit_unavailable("rid-1", 42);
        let last = LastControlResult::from_response(&resp);
        let text = last.toast_text().expect("EDGE-18 必须给出上屏文案");
        assert_eq!(text, "审计不可用 · 操作未执行");
        assert!(text.contains("审计不可用") && text.contains("操作未执行"));
        assert_eq!(
            text,
            crate::ui::pages::p2_config::TEXT_AUDIT_UNAVAILABLE,
            "必须与 UI §8.3 / P2 / P4 页用**同一串**（不另抄）"
        );
        assert!(
            !text.contains("审计不可写"),
            "取的是 UI 落地串（§8.3 原文的全角逗号不在 cmap 内 ⇒ 页面已改写为 `·`）"
        );
    }

    /// 其余失败：**服务端 `message` 优先**（EDGE-10 / EDGE-12 的「具体原因」），
    /// 且进屏前经 `display_safe`（ASCII 大写化 / `-`→`–`；**非 ASCII 原样透传** ——
    /// 自由文本的 cmap 保证不在本层，见 [`LastControlResult::toast_text`]）；`message` 为空才回落兜底串。
    #[test]
    fn server_message_wins_and_is_display_safe() {
        // message 含 ASCII `-`（无字形）⇒ 必须被改写成 U+2013
        let resp: ControlResponse<serde_json::Value> = ControlResponse::rejected(
            "rid-2",
            ControlCode::RejectedValidation,
            "字段 intercore-port 越界",
            vec![],
            Some("aud-2".into()),
            7,
        );
        let last = LastControlResult::from_response(&resp);
        let text = last.toast_text().expect("失败必须给出文案");
        assert!(text.contains("越界"), "具体原因须保留：{text}");
        assert!(
            !text.contains('-'),
            "上屏不得出现 ASCII 连字符（真机无字形）：{text}"
        );
        assert!(text.contains('\u{2013}'), "应改写为 U+2013：{text}");

        // message 为空 ⇒ 回落按码兜底
        let empty: ControlResponse<serde_json::Value> = ControlResponse::rejected(
            "rid-3",
            ControlCode::Busy,
            "   ",
            vec![],
            None,
            8,
        );
        assert_eq!(
            LastControlResult::from_response(&empty).toast_text().as_deref(),
            Some(crate::ui::pages::p4_interlock::TEXT_OP_BUSY)
        );
    }

    /// `duplicate=true` **原样保留**，且**不**被当成错误（成功路径照走）。
    #[test]
    fn duplicate_flag_is_preserved_and_is_not_an_error() {
        let mut resp: ControlResponse<serde_json::Value> =
            ControlResponse::ok("rid-4", Some(serde_json::json!({"latched": false})), Some("aud-4".into()), 9);
        resp.mark_duplicate();
        let mut st = ControlState::new();
        st.begin(ConsoleEndpoint::InterlockRelease, Some("rid-4"));
        st.record_response(&resp, 100);
        let last = st.last().expect("最近回执");
        assert!(last.duplicate, "幂等命中标记不得丢");
        assert!(last.ok && last.code == ControlCode::Ok);
        assert_eq!(last.audit_id.as_deref(), Some("aud-4"));
        assert!(!st.is_busy(), "回执到达 ⇒ 清在途");
        assert_eq!(st.toast_text(), None, "成功文案由页面给定 ⇒ 本层不弹 Toast");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // B3-1：Toast 生命周期（3 s）
    // ═══════════════════════════════════════════════════════════════════════

    /// **3 s 自动消失**（边界：`now == until` 即到期）。
    ///
    /// **改什么会让本条变红**：把 [`TOAST_TTL_MS`] 改成非 3 s，或把 `is_expired` 的 `>=`
    /// 改成 `>` ⇒ 边界断言红。
    #[test]
    fn toast_expires_after_exactly_three_seconds() {
        assert_eq!(TOAST_TTL_MS, 3_000, "F9.5 / 设计 §5.4：3 s");
        let mut st = ControlState::new();
        st.push_toast("x", 1_000);
        let t = st.toast().expect("刚刚弹的");
        assert_eq!(t.until_ms(), 4_000);
        assert!(!st.toast_expired(3_999), "3 s 未到 ⇒ 仍在");
        assert!(st.toast_expired(4_000), "到点即到期（闭区间）");
        assert!(!st.expire_toast(3_999));
        assert!(st.toast().is_some());
        assert!(st.expire_toast(4_000));
        assert!(st.toast().is_none(), "到期必须清掉（不得残留）");
        assert!(!st.expire_toast(9_999), "已清 ⇒ 再清是 no-op");
        // 新 Toast 覆盖旧的（UI §7.2「同一时刻仅 1 条」）
        st.push_toast("a", 10_000);
        st.push_toast("b", 10_500);
        assert_eq!(st.toast_text(), Some("b"));
        assert_eq!(st.toast().map(ToastRecord::until_ms), Some(13_500));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // B3-1：传输失败 / 在途 / 外壳层标志
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn transport_failure_clears_inflight_and_toasts_the_fallback() {
        let mut st = ControlState::new();
        st.begin(ConsoleEndpoint::ConfigApply, Some("rid-5"));
        assert!(st.is_busy());
        assert_eq!(st.inflight().and_then(InflightRequest::op), Some("apply"));
        st.record_transport_failure(500);
        assert!(!st.is_busy(), "超时 / 连接失败 ⇒ 在途必须当场作废");
        assert_eq!(st.last_transport_failure_ms(), Some(500));
        assert_eq!(
            st.toast_text(),
            Some(TRANSPORT_FAIL_TEXT),
            "传输失败必须给出上屏兜底文案（不静默）"
        );
        assert!(st.toast().is_some_and(|t| t.until_ms() == 500 + TOAST_TTL_MS));
    }

    /// 裁定 3：传输失败时**给页面补一条本地合成的「不可用」回执**（写端点才补）。
    ///
    /// **为什么必须有这条判据**：本层原来只记失败 + 弹一条**没有任何页面入口**的兜底 Toast
    /// ⇒ 控制通道挂掉时用户按「保存」/「人工释放联锁」，**屏上什么都不发生**（违 §2.6 降级可见）。
    ///
    /// **改什么会让本条变红**（**已实测**，见报告「探针 3」）：把
    /// `record_transport_failure_with_receipt` 里的合成去掉（直接 `return None`）⇒ 第 1 段红；
    /// 把在途信息取在 `record_transport_failure` **之后**（在途已被清 ⇒ `ep` 恒 `None`）⇒
    /// 第 1 段红（这正是"顺序敏感"的哨）；把记账那句删掉 ⇒ 第 2 段红（失败记账不得被本整改削弱）。
    ///
    /// **B3-2c 整改 重要 4 追加的那条**（第 ② 段末）：写端点的合成回执路径**不得**再压一条
    /// `ControlState` Toast（否则与页面 `show_result` 的 Toast 同拍同屏 ⇒ 违 UI §7.2
    /// 「同一时刻仅 1 条」）。**改什么会让它变红**：把该分支改回
    /// `record_transport_failure_with_text(..)`（或 `record_transport_failure(..)`）
    /// ⇒ 「写端点：`toast()` 必须为空」当场红。
    #[test]
    fn transport_failure_hands_back_a_local_receipt_for_inflight_writes() {
        let mut st = ControlState::new();
        st.begin(ConsoleEndpoint::InterlockRelease, Some("rid-9"));
        let d = st
            .record_transport_failure_with_receipt(700)
            .expect("写端点在途 + 通道死 ⇒ 必须有本地合成回执（否则屏上什么都不发生）");
        let RouteDecision::InterlockResult(r) = d else {
            panic!("联锁写必须在 P4 的 show_result 入口上");
        };
        assert_eq!(r.code, ControlCode::Unavailable);
        assert!(!r.ok && r.applied.is_none(), "操作未生效");
        assert_eq!(r.request_id, "rid-9", "回显该次在途请求（同一次操作对得上）");
        assert_eq!(r.message, TRANSPORT_FAIL_TEXT, "message = 本地错误文案（不编原因）");
        assert_eq!(r.at_ms, 700, "at_ms 取本地时钟");
        assert!(r.audit_id.is_none() && !r.duplicate && r.field_errors.is_empty());
        // ② 既有记账**不许**被本整改削弱（同一事件只记一份）。
        assert!(!st.is_busy(), "在途必须清掉");
        assert_eq!(st.last_transport_failure_ms(), Some(700));
        // ②′ **恰好一条**（B3-2c 整改 重要 4，PM 裁定）：写端点的失败**已经**由上面那条
        // 合成回执经页面 `show_result` 上屏 ⇒ `ControlState` 这一格**不得**再压一条
        // （否则同拍两个 Toast 同挂 `layer_top`、同坐标同文案 ⇒ 违 UI §7.2）。
        assert!(
            st.toast().is_none(),
            "写端点传输失败 ⇒ 上屏只走页面 Toast（合成回执），app 层不得再压一条（UI §7.2）"
        );
        // ③ 无在途（查询不入 `inflight`）⇒ 不合成（读端点的出口是 P3 通道条态）；
        //    此时**没有**页面上屏出口 ⇒ app 层兜底 Toast **必须**保留（否则静默失败，违 §2.6）。
        let mut idle = ControlState::new();
        assert!(idle.record_transport_failure_with_receipt(700).is_none());
        assert_eq!(
            idle.toast_text(),
            Some(TRANSPORT_FAIL_TEXT),
            "无在途（读端点）⇒ 没有合成回执可送 ⇒ app 层兜底 Toast 必须保留"
        );
        assert_eq!(idle.last_transport_failure_ms(), Some(700), "记账照旧");
    }

    /// **查询**成功路径用 [`ControlState::finish`]（B3-2b-2 新增）：清在途，
    /// 但**不**碰回执摘要、**不**弹 Toast、**不**记"传输失败时刻"。
    ///
    /// 为什么必须有这条判据：查询端点按契约返回**裸 DTO**（没有 `ControlResponse` 信封）
    /// ⇒ 接线层拿不到能传给 `record_response` 的东西。若没有 `finish`，只剩两条歧路：
    /// ① 让查询永远算在途（后续写操作被判 `Busy` 而丢弃）；② 用
    /// `record_transport_failure` 清在途（**谎记一次失败**并弹「操作失败」）。
    ///
    /// **改什么会让本条变红**：把 `finish` 实现成 `record_transport_failure` 的别名
    /// （多弹一条 Toast / 多记一次失败时刻）⇒ 第 3 / 4 段红；把 `finish` 写成空实现
    /// ⇒ 第 2 段红。
    #[test]
    fn finish_clears_inflight_without_faking_a_failure() {
        let mut st = ControlState::new();
        st.begin(ConsoleEndpoint::Audit, None);
        assert!(st.is_busy());
        st.finish();
        assert!(!st.is_busy(), "查询完成 ⇒ 在途必须清掉（否则后续写操作恒被判在途）");
        assert_eq!(st.last_transport_failure_ms(), None, "查询成功**不得**记成传输失败");
        assert_eq!(st.toast_text(), None, "查询成功不得弹任何 Toast（尤其不是「操作失败」）");
        assert!(st.last().is_none(), "查询载荷不是信封 ⇒ 不产生回执摘要");
        // 对偶：清完之后**可以**再起一条（否则连接池被卡死）
        st.begin(ConsoleEndpoint::ConfigApply, Some("rid-fin"));
        assert!(st.is_busy());
    }

    /// 在途请求记的是**查询端点也无 `op`**（GET 无信封）；写端点记 `op` 与 `request_id`。
    #[test]
    fn inflight_records_op_and_request_id() {
        let mut st = ControlState::new();
        assert!(!st.is_busy() && st.inflight().is_none());
        st.begin(ConsoleEndpoint::Audit, None);
        let inf = st.inflight().expect("在途");
        assert_eq!(inf.op(), None, "查询端点无信封 op");
        assert_eq!(inf.request_id, None);
        st.begin(ConsoleEndpoint::InterlockAckM1, Some("rid-6"));
        assert_eq!(st.inflight().and_then(InflightRequest::op), Some("ack_m1"));
        assert_eq!(
            st.inflight().and_then(|i| i.request_id.as_deref()),
            Some("rid-6")
        );
    }

    /// `confirm` 的 `is_some()` 即 `Shell::set_modal_open(..)` 的取值
    /// （`ui/shell.rs` 偏差 **SH2** 的已登记契约）。
    ///
    /// ※ 本用例原为 `confirm_and_dirty_flags_feed_the_shell_contract`，同批断言了一个
    /// `dirty` 镜像字段。该字段**无任何消费者**（`ui/**` + `shell.rs` 零调用，真源是
    /// `P2ConfigPage::is_dirty()`）⇒ 按 B3-1 规格评审阻塞 1 **删除字段与其断言**；
    /// 用例随之改名、只保留 `confirm` 契约（该断言的保护力未减）。
    #[test]
    fn confirm_flag_feeds_the_shell_contract() {
        let mut st = ControlState::new();
        assert!(!st.confirm_open() && st.confirm().is_none());
        st.set_confirm(Some(ConsoleEndpoint::InterlockRelease));
        assert!(st.confirm_open(), "弹层打开 ⇒ 空闲计时暂停（TT-12 / TT-13）");
        assert_eq!(st.confirm(), Some(ConsoleEndpoint::InterlockRelease));
        st.set_confirm(None);
        assert!(!st.confirm_open());
    }
}
