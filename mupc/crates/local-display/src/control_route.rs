//! 控制通道的**纯映射层**（开发单元 B3-2b-2，B3 的最后一块）。
//!
//! 本模块只做两件事，**全部是纯函数、零 LVGL、零 I/O**：
//!
//! | 方向 | 入口 | 产出 |
//! |------|------|------|
//! | 网络 → 屏（**回执路由**） | [`route`] | [`RouteDecision`]：**哪个端点 ⇒ 调哪个页的哪个方法**（设计 §3.4 的 8 端点 + §15.3.2 的 3 个外设端点，逐条对应） |
//! | 屏 → 网络（**意图 → 请求载荷**） | [`log_query_string`] / [`audit_query_string`] | §3.4 的查询串（多值一律**重复键**，2026-09-15 补充约定） |
//!
//! # 为什么要抽成纯函数（而不是直接写在 `app.rs` 的 `tick` 里）
//!
//! `app.rs` 的装配体在**任何一台开发机上都必须先建起 LVGL 会话**才能构造
//! （`App::new_offscreen` 会 `lv_init` + 建 display/indev/六页）⇒ 进程内没有第二个 LVGL 测试
//! 线程可用来跑"回执 → 页面"的断言（LVGL 全局态非线程安全，全仓只有 `src/lvgl/tests.rs`
//! 那**一个** `#[test]` 串行调起）。若路由判据写在 `tick` 里，它就**只**能被"起真进程 + 桩服务端
//! + 读屏面像素"间接覆盖 —— 而"路由错了页"在**像素上几乎看不出来**（页都在，只是内容不对）。
//!
//! ⇒ 把判据抽到这里，`app.rs` **只用**本模块的产出（`RouteDecision` 的 `match` 是唯一分派点），
//! 于是"把 `Logs` 回执改路由到 P2"这类错误在**纯逻辑用例**上当场变红（见本文件 `#[cfg(test)]`
//! 的 `routing_table_is_pinned_endpoint_by_endpoint` 与其破坏性探针记录）。
//!
//! # `serde_json::Value` 作为线上载荷（**刻意的**）
//!
//! [`crate::console::ConsoleClient::tick`] 的载荷类型 `T` 是**该次调用**的单一泛型参数
//! （`fn tick<T: DeserializeOwned>`），而 8 个端点的载荷类型两两不同（`ConfigView` / `LogPage`
//! / `AuditPage` / `Vec<String>` / `Vec<OpOption>` / `ControlResponse<..>`）⇒ 生产调用点只能给一个
//! **能容纳全部端点**的类型，`serde_json::Value` 就是它。**真正的类型化发生在 [`route`] 里**
//! （按 `outcome.endpoint` 逐端点 `from_value`），并且失败会走 [`RouteError::Decode`]
//! **响亮上抛**而不是静默丢弃 —— 即"线上多一层 `Value`"的代价被限制在本模块内，且**不吞错**。

use std::fmt;

use mupc_display_proto::{
    AuditPage, BmsAlarmPage, ConfigPatch, ConfigView, ConsoleEndpoint, ControlCode,
    ControlResponse, FireDetectorPage, InterlockOpAck, InterlockOpPayload, LogPage, LogRange,
    OpOption, PeripheralCatalog,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::console::{encode_query, ConsoleOutcome};
use crate::ui::pages::p3_logs::LogQuery;
use crate::ui::pages::p5_audit::AuditQuery;
use crate::ui::shell::NavPage;

// ═══════════════════════════════════════════════════════════════════════════
// 1. 线上载荷 + 路由决策
// ═══════════════════════════════════════════════════════════════════════════

/// 控制通道回执的**线上**载荷类型（见模块头"`serde_json::Value` 作为线上载荷"）。
pub type RawPayload = Value;

/// **回执 → 页面**的路由决策（[`route`] 的唯一产出）。
///
/// 每个变体**恰好**对应设计 §3.4 表里的一行 ⇒ 8 个端点、8 条分支，**没有"其它"兜底**
/// （兜底会让"新增端点了但忘了接页"变成静默 no-op）。
#[derive(Debug, Clone, PartialEq)]
pub enum RouteDecision {
    /// `GET /v1/console/config` 成功 ⇒ `P2ConfigPage::set_config(&ConfigView)`。
    Config(ConfigView),
    /// `POST /v1/console/config/apply` 回执 ⇒ `set_submitting(false)` + `show_result(&resp)`。
    ///
    /// **失败回执也走这里**（`ok = false` 时 `applied` 为 `None`）—— 与 P2 的
    /// [`crate::ui::pages::p2_config::P2ConfigPage::show_result`] 自带的成功/失败双分支同口径。
    ConfigApply(ControlResponse<ConfigView>),
    /// `GET /v1/console/logs` ⇒ `P3LogsPage::set_page(&LogPage)`。
    Logs(LogPage),
    /// `GET /v1/console/logs/targets` ⇒ `P3LogsPage::set_targets(&[String])`。
    LogsTargets(Vec<String>),
    /// `GET /v1/console/audit` ⇒ `P5AuditPage::set_page(&AuditPage)`。
    Audit(AuditPage),
    /// `GET /v1/console/audit/ops` ⇒ `P5AuditPage::set_ops(&[OpOption])`。
    AuditOps(Vec<OpOption>),
    /// 联锁写回执（`release` / `ack_m1`）⇒ `P4InterlockPage::show_result(&resp)`。
    ///
    /// **成功与一切失败都走这里**：页面按 `code` 自行分派，失败时就地展示服务端 `message` 里
    /// **具体**的原因（EDGE-10 / EDGE-12）。
    ///
    /// # ⚠️ 沿革（PM 裁定 1，2026-09-16：**不得谎报拒绝原因**）
    ///
    /// 本单元（B3-2b-2）最初按**任务书**给 `RejectedPrecondition` 单开了一条臂 ——
    /// 走 `P4InterlockPage::show_conflict()`（EDGE-19 的**固定**文案「联锁状态已变化 · 请刷新
    /// 后重试」）。该实现**已被推翻**：`IL12` 明写 `ControlCode::RejectedPrecondition` 把
    /// 「状态已变化」与「触发源未复位 / 保持时间不足 / latch / `StopPending`」**糊在同一个码**里、
    /// 回执**无**结构化 `InterlockReject` 字段 ⇒ 客户端**无从判定**到底是哪一种；一律显 EDGE-19
    /// 的固定文案 = **谎报原因**（违 §2.6）。故该臂**整条删除**，`RejectedPrecondition` 与其它
    /// 业务拒绝**同路径**（`show_result`），具体原因由**服务端** `message` 承担 ——
    /// 设计 TD:594 明写真·状态变化时服务端返回的 `message` **就是**「联锁状态已变化，请刷新后重试」
    /// ⇒ EDGE-19 的文案**照样按其本意出现**（由服务端判定，不由客户端猜）。
    ///
    /// 连带后果（如实登记）：`P4InterlockPage::show_conflict()` 因此**在生产路径上不可达**
    /// （它保留给"客户端**本地能判定**状态变化"的场合，见该方法的文档与 `p4_interlock.rs` 的
    /// **IL12**）。**不得**为了"让那个入口有用"而在本层重新按 `code` 猜语义。
    InterlockResult(ControlResponse<InterlockOpAck>),

    // ── U-73 外设（§15.3.2 的三个只读端点；T21c-3 接线）─────────────────────
    /// `GET /v1/console/peripherals/catalog` 成功 ⇒ **同一份 catalog 供两页**：
    /// `P4InterlockPage::set_catalog` **且** `P6SystemPage::set_catalog`
    /// （§15.3.1：P4/P6 首次进入或 `catalog_rev` 变化时**一次性**读取）。
    ///
    /// **没有"失败"变体**（刻意的）：取值失败时**什么都不做**才是正确语义 ——
    /// 页面保持既有态（从未取到 ⇒ 中文名位显「名称未获取」而**值照常显示**；
    /// 重取失败 ⇒ 保留旧 catalog）。**不得**用 `clear_catalog()` 表达失败：
    /// 那会把"已取到的名字"抹掉（§15.3.1 的「保留旧 catalog」）。
    PeripheralCatalog(PeripheralCatalog),
    /// `GET /v1/console/peripherals/fire_detectors` 成功 ⇒
    /// `P4InterlockPage::set_fire_page(&FireDetectorPage)`（F21.4 探测器明细分页）。
    FireDetectorPage(FireDetectorPage),
    /// 探测器明细端点**不可用**（非 2xx / 连接 / 超时 / 解码）⇒
    /// `P4InterlockPage::set_fire_page_failed()`：下钻视图显「明细不可用」+「重试」。
    ///
    /// **只传失败事实、不传错误串**（**R-4 产品裁定 2026-09-25**）：服务端 400/503 的原因串
    /// 含生成字体 cmap 外的字（上屏必出豆腐块），**只进日志 / 现场排障**。同款既有先例 =
    /// `post_config_apply` 的固定 `message` 口径。
    FireDetectorUnavailable,
    /// `GET /v1/console/peripherals/bms_alarms` 成功 ⇒
    /// `P6SystemPage::set_bms_page(&BmsAlarmPage)`（F22.3 的 288 位下钻）。
    BmsAlarmPage(BmsAlarmPage),
    /// BMS 告警下钻端点**不可用** ⇒ `P6SystemPage::set_bms_page_failed()`
    /// （文案与 R-4 口径同 [`RouteDecision::FireDetectorUnavailable`]）。
    BmsAlarmUnavailable,
}

/// 路由失败（**响亮**，不静默丢弃）。
#[derive(Debug)]
pub enum RouteError {
    /// 载荷**形态**与端点不符：写端点收到裸查询载荷 / 查询端点收到控制信封。
    ///
    /// 正常路径不可达（`console.rs::parse` 按 `endpoint.is_write()` 决定解哪个形状）
    /// ⇒ 出现即"两侧路由漂移"，必须看得见。
    Kind(ConsoleEndpoint),
    /// 载荷**解码**失败：服务端给本端点回了形状不符的 JSON。
    Decode(ConsoleEndpoint, serde_json::Error),
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Kind(ep) => write!(
                f,
                "控制通道回执形态与端点不符：`{}`（信封/裸 DTO 错配）",
                ep.path()
            ),
            Self::Decode(ep, e) => {
                write!(f, "控制通道回执解码失败：`{}`：{e}", ep.path())
            }
        }
    }
}

impl std::error::Error for RouteError {}

/// **唯一的**回执路由入口：按 `outcome.endpoint` **精确**分派（设计 §3.4）。
///
/// **不得**改成"按载荷形状猜端点"：`GET /logs` 的 `LogPage` 与任何别的端点都能被 `Value`
/// 装下，形状猜不出版本差异；而 `endpoint` 字段是 `console.rs` 从**请求描述**里原样带出来的
/// （不是从响应里解出来的）⇒ 它才是权威判据。
pub fn route(outcome: &ConsoleOutcome<RawPayload>) -> Result<RouteDecision, RouteError> {
    let ep = outcome.endpoint;
    if ep.is_write() {
        let resp = outcome.response().ok_or(RouteError::Kind(ep))?;
        match ep {
            ConsoleEndpoint::ConfigApply => Ok(RouteDecision::ConfigApply(typed(resp, ep)?)),
            // 联锁两写端点**只有**一条出路：`show_result`（成功 / 一切失败都由页面按 `code`
            // 分派 —— 包括 `RejectedPrecondition`；沿革与理由见 [`RouteDecision::InterlockResult`]）。
            ConsoleEndpoint::InterlockRelease | ConsoleEndpoint::InterlockAckM1 => {
                Ok(RouteDecision::InterlockResult(typed(resp, ep)?))
            }
            // 写端点清单与 `ConsoleEndpoint::ALL` 同源 ⇒ 本臂不可达；写出来是为了
            // **将来新增写端点时不静默走错分支**（宁可 `Err`）。
            _ => Err(RouteError::Kind(ep)),
        }
    } else {
        let payload = outcome.query().ok_or(RouteError::Kind(ep))?;
        match ep {
            ConsoleEndpoint::Config => Ok(RouteDecision::Config(decode(ep, payload)?)),
            ConsoleEndpoint::Logs => Ok(RouteDecision::Logs(decode(ep, payload)?)),
            ConsoleEndpoint::LogsTargets => Ok(RouteDecision::LogsTargets(decode(ep, payload)?)),
            ConsoleEndpoint::Audit => Ok(RouteDecision::Audit(decode(ep, payload)?)),
            ConsoleEndpoint::AuditOps => Ok(RouteDecision::AuditOps(decode(ep, payload)?)),
            // U-73 §15.3.2 的三个只读端点（**失败**不在这里 —— 见 `page_failure_decision`）。
            ConsoleEndpoint::PeripheralsCatalog => {
                Ok(RouteDecision::PeripheralCatalog(decode(ep, payload)?))
            }
            ConsoleEndpoint::PeripheralsFireDetectors => {
                Ok(RouteDecision::FireDetectorPage(decode(ep, payload)?))
            }
            ConsoleEndpoint::PeripheralsBmsAlarms => {
                Ok(RouteDecision::BmsAlarmPage(decode(ep, payload)?))
            }
            _ => Err(RouteError::Kind(ep)),
        }
    }
}

/// `Value` → 目标 DTO（失败走 [`RouteError::Decode`]）。
fn decode<T: DeserializeOwned>(ep: ConsoleEndpoint, v: &Value) -> Result<T, RouteError> {
    serde_json::from_value(v.clone()).map_err(|e| RouteError::Decode(ep, e))
}

/// `ControlResponse<Value>` → `ControlResponse<T>`（只重解 `applied`；信封其余字段原样搬运）。
///
/// **`applied` 为 `None` 不是错误**：失败回执（`ok = false`）按契约就是 `applied = None`
/// （见 `display-proto` 的 `ControlResponse::rejected` / `audit_unavailable`）。
fn typed<T: DeserializeOwned>(
    resp: &ControlResponse<Value>,
    ep: ConsoleEndpoint,
) -> Result<ControlResponse<T>, RouteError> {
    let applied = match resp.applied.as_ref() {
        None => None,
        Some(v) => Some(decode::<T>(ep, v)?),
    };
    Ok(ControlResponse {
        request_id: resp.request_id.clone(),
        ok: resp.ok,
        code: resp.code,
        message: resp.message.clone(),
        applied,
        field_errors: resp.field_errors.clone(),
        audit_id: resp.audit_id.clone(),
        duplicate: resp.duplicate,
        at_ms: resp.at_ms,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// 1′. 传输失败 → **本地合成**的「不可用」回执（B3-2b-2 整改 · PM 裁定 3）
// ═══════════════════════════════════════════════════════════════════════════

/// 传输失败（连接 / 超时 / 非 200 / 解码）时，给**写端点**补一条**本地合成**的「不可用」
/// 回执；读端点返回 `None`。
///
/// # 第二个调用点（**PM 裁定 2 · 2026-09-16 · SH20**）
///
/// 除传输失败外，[`route`] 自己的 `Err`（回执**形态 / 解码**不符）在**写端点**上也走本函数
/// （**同一份机制，不为该场景另造**）—— 理由是 app 层兜底 Toast 在确认弹层打开期间被完全
/// 遮挡（几何实测见 `ui/shell.rs` 偏差 **SH20**），而**页面** Toast 建在弹层之后 ⇒ 可见。
/// 生产调用点因此有**两处**，都在 [`crate::app::App::absorb_console`]（传输失败分支 /
/// `route` 的 `Err` 分支），且**都**经
/// [`crate::state::ControlState::record_transport_failure_with_receipt`] —— 该入口按"在途端点
/// 是不是写端点"裁决（写端点 ⇒ 合成回执 + **不**压 app Toast；无在途 ⇒ 回落 app 兜底 Toast），
/// 两条分支各自**恰有一条**上屏通道（UI §7.2）。
///
/// # 为什么要有它（裁定 3：降级必须有**上屏**出口）
///
/// 控制通道挂掉时用户按「保存」/「人工释放联锁」：`ConsoleClient` 报传输失败，接线层过去
/// 只写一行 stderr + 在 `ControlState` 里记一条兜底 Toast，而**没有任何页面暴露通用 Toast
/// 入口**（P2 / P4 的 `show_toast` 是私有的）⇒ **屏上什么都不发生**。这与 §2.6「降级可见」
/// 相悖，也是"只落日志"的静默失败。
///
/// 修法（**不改 `src/ui/**`**）：由接线层**本地合成**一条回执，走页面**既有**的
/// `show_result` 路径 —— 复用既有上屏口（P4 的就地原因带 + 失败 Toast / P2 的失败提示），
/// **不**给页面加公开入口、**不**新增上屏字。调用点是
/// [`crate::state::ControlState::record_transport_failure_with_receipt`]（App 的失败分支只
/// 消费它的产出并送进 [`route`] 之外**唯一**的分派点）。
///
/// # ⚠️ 这是**本地合成**，不是服务端回执（每个字段的取值理由逐条给出）
///
/// - `request_id`：**回显该次在途请求的** `request_id`（屏上/日志里对得上"是哪一次操作"）；
///   写端点的在途恒带该值 —— 查询不发信封（契约 §3.3），故读端点在本函数**第一行**就返回
///   `None`（不合成，见下）；
/// - `ok = false` + `code = Unavailable`：事实就是"**没拿到结果**、后端不可达"。取
///   `Unavailable`（契约里"后端不可用"的那一格），**不**冒充 `ApplyFailed` /
///   `RejectedValidation` 一类**服务端**判决（那是"请求到了、后端判了"的意思）；
/// - `message`：**本地**错误文案（调用方传 [`crate::state::TRANSPORT_FAIL_TEXT`]）——
///   拿不到服务端的具体原因就**不编**原因：本地文案只说"操作失败"，如实、可上屏（cmap 内）；
/// - `audit_id = None`：**审计记录根本没写成**（请求没到服务端）⇒ 不得给出一个假审计号
///   （现场会拿它去审计库里查）；
/// - `duplicate = false`：幂等命中是**服务端**的事实（同一 `request_id` 的首次结果），
///   本地无任何依据判定 ⇒ 只能取 `false`；
/// - `applied = None`：**操作未生效**（契约：失败回执的 `applied` 即 `None`）⇒ 屏上不得
///   出现"已生效"的新状态；
/// - `field_errors = []`：本地不存在**逐字段**校验结论 ⇒ 空表，**不编**字段错误。
///
/// 取值经契约自带的 [`ControlResponse::rejected`]（它固定了 `ok=false` / `applied=None` /
/// `duplicate=false` 三条口径），本层只补 `code` 与 `message`。
pub fn transport_failure_decision(
    endpoint: ConsoleEndpoint,
    request_id: &str,
    message: &str,
    at_ms: u64,
) -> Option<RouteDecision> {
    match endpoint {
        ConsoleEndpoint::ConfigApply => Some(RouteDecision::ConfigApply(local_unavailable(
            request_id, message, at_ms,
        ))),
        ConsoleEndpoint::InterlockRelease | ConsoleEndpoint::InterlockAckM1 => Some(
            RouteDecision::InterlockResult(local_unavailable(request_id, message, at_ms)),
        ),
        // 读端点**不合成**：它们的降级出口是**另一条**既有通道 —— P3 的通道条态
        // （连续 2 次失败 ⇒ 「已断开」，设计 §6.3，见 [`p3_connected`]）与页面自身的
        // 数据陈旧态；给读端点造一条"写回执"会把语义接到**错的页**上。
        //
        // ⚠️ **SH20 裁定 3（2026-09-16）**：读端点这条残余（此时上屏只能靠 app 层兜底
        // Toast，而它在弹层打开期间被遮）**PM 已裁定 = 接受** —— 读通道降级**另有可见通道**
        // （页眉通道胶囊 + EDGE-03 整屏降级），该 Toast 属**补充**。**不得**为了"让它可见"
        // 而给读端点在这里造回执（那会把写回执塞进没有 `show_result` 的页）。
        _ => None,
    }
}

/// 本地合成的「不可用」回执（字段理由见 [`transport_failure_decision`]）。
fn local_unavailable<T>(request_id: &str, message: &str, at_ms: u64) -> ControlResponse<T> {
    ControlResponse::rejected(
        request_id,
        ControlCode::Unavailable,
        message,
        Vec::new(),
        None,
        at_ms,
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// 1″. U-73 外设端点：**失败面** + **catalog 的读取时机**（T21c-3 接线）
// ═══════════════════════════════════════════════════════════════════════════

/// **有页面就地失败面**的读端点：`endpoint` 失败（非 2xx / 连接 / 超时 / 解码）⇒
/// 该端点的「不可用」决策；**无**页面失败面的端点 ⇒ `None`（走既有降级通道）。
///
/// # 为什么单开一条（而不是塞进 [`transport_failure_decision`]）
///
/// [`transport_failure_decision`] 是**写端点**的"本地合成回执"通道：它把失败包装成一条
/// `ControlResponse` 送进页面的 `show_result`。而本函数服务的两条端点**没有回执入口**，
/// 页面暴露的是**无参**的 `set_fire_page_failed()` / `set_bms_page_failed()`
/// （§15.6.2 ⑥：屏上「明细不可用」+「重试」是**本地固定文案**）⇒ 决策本身就是"失败事实"
/// 的全部，**不携带任何服务端串**（**R-4**：400/503 的原因串只进日志）。
/// 两种"失败"的**载荷形态**不同（`ControlResponse` vs 无载荷），合并会把 R-4 的口径搞糊。
///
/// # 为什么 catalog **不在**此列（刻意的）
///
/// §15.3.1 给的降级是「**保留旧 catalog** + 中文名位显「名称未获取」而**值照常显示**」——
/// 这是一个**不抹掉已有名字**即可达成的态（页面默认就没有 catalog）。若在这里返回一个
/// "catalog 不可用"决策去调 `clear_catalog()`，反而会把**已经取到的名字抹掉**，
/// 与「保留旧 catalog」相反。故 catalog 失败 ⇒ **无决策**。
///
/// ⚠️ 「无决策」**不等于**「无页面失败面」（T21c-3-r1）：catalog 的下钻失败面是 §15.3.1 的
/// 顶部「名称表可能过期」+「重试」，由 `App::on_catalog_read_failed` 直接落到两页
/// （`set_catalog_stale`），**不经过** `RouteDecision`。判"要不要**交出**一条决策"与判
/// "页面**有没有**自己的失败面"是**两问** ⇒ 后者见 [`page_owns_failure_surface`]。
pub fn page_failure_decision(endpoint: ConsoleEndpoint) -> Option<RouteDecision> {
    match endpoint {
        ConsoleEndpoint::PeripheralsFireDetectors => Some(RouteDecision::FireDetectorUnavailable),
        ConsoleEndpoint::PeripheralsBmsAlarms => Some(RouteDecision::BmsAlarmUnavailable),
        _ => None,
    }
}

/// 该端点失败时，**页面是否已有就地失败面**（⇒ app 层**不得**再压一条通用「操作失败」Toast）。
///
/// # 为什么要与 [`page_failure_decision`] 分开（两问，不是一问）
///
/// - **问一**（[`page_failure_decision`]）：要不要给接线层**交一条 `RouteDecision`**？
///   只有两个明细端点有（页面暴露的是**无参**的 `set_*_page_failed()`）；
/// - **问二**（本函数）：页面**有没有**自己的失败面？—— catalog **有**（§15.3.1 的
///   顶部提示条 + 「重试」，由 `App::on_catalog_read_failed` 上屏）但**没有**决策。
///
/// 若把两问合成一问，二者之一必然失真：catalog 要么被塞一条会 `clear_catalog` 的决策
/// （违"保留旧 catalog"），要么被当成"没有页面失败面"而**同时**压一条通用 Toast
/// （违 UI §7.2「同一时刻仅 1 条」，且通用「操作失败」对一个**取数**动作是**误导** —— 它不是
/// 用户发起的操作；口径见 `App::absorb_console` 与 `state::ControlState::record_read_failure`）。
///
/// # 为什么**不得**放宽成"所有读端点"
///
/// 其余读端点（`Config` / `Logs` / `LogsTargets` / `Audit` / `AuditOps`）**没有**任何页面
/// 失败面 ⇒ app 层兜底 Toast 是它们**唯一**的上屏通道（去掉 = 静默失败，违 §2.6「降级可见」）。
/// 判据是**白名单**（恰好三条），不是"读端点"这一类。
///
/// **改什么会让本条变红**：把实现改成 `!endpoint.is_write()`（一刀切）⇒
/// `failure_surface_roster_...` 用例红（其余读端点被误免）⇒ 那正是本仓最忌讳的
/// "把有出路与没出路混为一谈"。
pub fn page_owns_failure_surface(endpoint: ConsoleEndpoint) -> bool {
    // 白名单**恰好**三条（两个明细端点 + catalog）；`page_failure_decision` 的 `Some` 集
    // 合是本名单的**子集** —— 该包含关系由用例逐条钉住（防两处漂移）。
    page_failure_decision(endpoint).is_some()
        || matches!(endpoint, ConsoleEndpoint::PeripheralsCatalog)
}

/// 该页是否会用到外设元数据（catalog）——**首次进入即取**的判据（设计 §15.3.1）。
///
/// P4（`Interlock`，F21 消防）与 P6（`System`，F20/F22–F24「装置与外设」）**同一份 catalog**。
pub fn periph_metadata_page(p: NavPage) -> bool {
    matches!(p, NavPage::Interlock | NavPage::System)
}

/// catalog 的**读取时机**判据（设计 §15.3.1；**纯函数**，无 LVGL、无 I/O）。
///
/// 两个触发源（**只有**这两个）：
/// 1. **首次进入** P4 / P6（`on_periph_page && !entered_before`）；
/// 2. 帧内 `catalog_rev` 与本地持有的 `rev` **不等**（`frame_rev != held_rev`）。
///
/// # 「一次性」语义怎么成立（`held_rev` 的契约）
///
/// 本函数**只**看"本拍的两个 rev 是否相等"。接线层（`App::tick_periph_catalog`）在**发起请求
/// 时**就把 `held_rev` 预置成当拍的 `frame_rev`（回执到达后再用响应里的 `cat.rev` 校正）⇒
/// 同一 rev **不会**在 1 Hz 主拍上被反复取（这正是 §15.3.1 的「一次性」）。
/// 相反，若接线层只在**回执到达后**才更新 `held_rev`，那么在飞期间的每一拍都会命中
/// "rev 不等" ⇒ 反复发起（被 `is_busy()` 挡住 ⇒ 表现为"永远在取 catalog"）。
///
/// # 首次进入判据为何要求 `on_periph_page`
///
/// 帧从第 1 拍就有，而用户可能从头到尾不进 P4 / P6 ⇒ **不得**在开机时就取 catalog
/// （那是"每次开机一次无用请求"，与"首次进入才取"的措辞相反）。
///
/// # `failed_rev`（**W-1 收口**）：失败过的那个 rev **不自动重发**
///
/// 回执**失败**时接线层会把预置的 `held_rev` **回滚**成"失败前的值"（W-1：`held_rev` 的语义
/// 是"本地真实持有"，不该被在飞预置永久污染）。回滚的**代价**是：若回滚后的 `held_rev` 与
/// 当拍 `frame_rev` 不同，第 2 支判据会**每拍都成立** ⇒ 被 `is_busy()` 挡成"永远在取 catalog"
/// （**这正是本任务最容易踩的一脚**）。故接线层把"最近一次失败于哪个 rev"作为 `failed_rev`
/// 传进来：
///
/// - `failed_rev == frame_rev` ⇒ **判否**（同一 rev 不自动重发；恢复路径 = §15.3.1 第 3 句给的
///   「**重试**」按钮 —— 它把 `failed_rev` 与"首次进入资格"一起清掉即可强制重取一次）；
/// - `failed_rev != frame_rev` ⇒ 照旧走第 2 支 ⇒ **帧内 rev 一变就自动重取**
///   （"失败后不可自愈"的缺陷由此收口；评审 W-1 的原文要求）。
///
/// 比对用的"本地 rev"取 `held_rev.or(failed_rev)`：**"尝试过的 rev"也算两侧都知道**
/// —— 否则"从未取到（`held_rev = None`）+ 重试失败"之后，帧内 rev 再怎么变都会落进
/// `(Some, None) => false` 而**永不自动重取**。回滚本身**不会**造成重触发：`(Some(r), None)`
/// 判否，抑制只由 `failed_rev` 承担（**两件事分开**，各有一条用例）。
pub fn catalog_due(
    on_periph_page: bool,
    entered_before: bool,
    frame_rev: Option<u32>,
    held_rev: Option<u32>,
    failed_rev: Option<u32>,
) -> bool {
    if on_periph_page && !entered_before {
        return true;
    }
    // W-1：同一 rev 失败过 ⇒ 不自动重发（否则每次失败都会演成"每拍反复发起"）。
    if failed_rev.is_some() && failed_rev == frame_rev {
        return false;
    }
    // 只在**两侧都知道**时比：两者皆 `None` = 还没拿到过、也没试过任何 rev（页面自会显
    // 「名称未获取」），此时**不**把它当成"rev 变化"（否则每拍都会命中）。
    match (frame_rev, held_rev.or(failed_rev)) {
        (Some(r), Some(h)) => r != h,
        _ => false,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. P3「日志通道态」的驱动源（控制通道**可达性**，不是帧通道）
// ═══════════════════════════════════════════════════════════════════════════

/// P3 判「已断开」所需的**连续**控制通道失败数（设计 §6.3「实时通道状态」行原文：
/// **「断开由控制通道请求失败判定（连续 2 次失败），恢复后 ≤1 s 回绿」**）。
pub const P3_DOWN_FAIL_STREAK: u32 = 2;

/// 控制通道连续失败数 ⇒ P3 通道条态（`true` = 「实时日志已连接」）。
///
/// # ⚠️ 为什么是**控制通道**而不是帧通道（本单元修的现存缺陷）
///
/// P3 的日志行**全部**来自 `GET /v1/console/logs`（控制通道，设计 §3.4），**不来自**
/// `GET /v1/display/latest` 的帧：帧里根本没有日志条目这个字段。⇒ 决定"日志还能不能续上"
/// 的唯一事实是**控制通道是否可达**；用帧通道的 `ChannelStatus` 驱动会得到一个**语义倒置**的
/// 结论 —— `EDGE-20`（控制通道可达但读通道断）下，帧断了而日志照常可查，屏上却报「实时日志已断开」；
/// 反过来帧正常而控制通道挂了，屏上会**假报**「已连接」，而日志其实已经不再更新（**静默陈旧**，
/// 正是本项目最忌讳的一类）。设计 §6.3 也把口径写死为"由控制通道请求失败判定"。
///
/// # 为什么用 `fail_streak` 而不是"最近一次请求成败"
///
/// 单次失败不足以判死（一次抖动 / 一次 5 s 超时不该立刻报断开）；契约给的口径就是
/// **连续 2 次**。`ConsoleClient::fail_streak()` 是这一事实的**唯一真源**
/// （成功即清零，见 `console.rs::parse`）⇒ 本层不另记一份计数（同一事实不记两份）。
///
/// **上电初值**：`fail_streak == 0` ⇒ `true`（"已连接"）。这与设计一致（断开是**失败驱动**
/// 的降级态，不是"还没连上"的初始态）；真实不可达会在**前两条**请求失败后（本机回环为毫秒级）
/// 翻红。
pub fn p3_connected(control_fail_streak: u32) -> bool {
    control_fail_streak < P3_DOWN_FAIL_STREAK
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 屏 → 网络：筛选意图 → §3.4 查询串
// ═══════════════════════════════════════════════════════════════════════════

/// `LogLevel` 的**线上**名（与 §3.4 `levels=` 多值一致）。
///
/// **不另抄一份字面量**：本函数与 `display-proto` 的 serde 形态由
/// `log_level_wire_names_match_serde` 逐条对拍（改契约即变红）。
pub fn log_level_wire(l: mupc_display_proto::LogLevel) -> &'static str {
    use mupc_display_proto::LogLevel as L;
    match l {
        L::Error => "error",
        L::Warn => "warn",
        L::Info => "info",
        L::Debug => "debug",
        L::Trace => "trace",
    }
}

/// `ConsoleOp` 的**线上**名（与 §3.4 `ops=` 多值一致；由 `console_op_wire_names_match_serde` 对拍）。
pub fn console_op_wire(op: mupc_display_proto::ConsoleOp) -> &'static str {
    use mupc_display_proto::ConsoleOp as O;
    match op {
        O::ConfigApply => "config_apply",
        O::ConfigResetDefault => "config_reset_default",
        O::InterlockRelease => "interlock_release",
        O::InterlockAckM1 => "interlock_ack_m1",
    }
}

/// `LogRange` 的**线上**名（与 §3.4 `range=1h|24h|custom` 一致）。
fn log_range_wire(r: LogRange) -> &'static str {
    match r {
        LogRange::H1 => "1h",
        LogRange::H24 => "24h",
        LogRange::Custom => "custom",
    }
}

/// `(键, 值)` 序列 → 查询串（**多值用重复键**，值经百分号编码）。
fn encode_pairs(pairs: &[(String, String)]) -> String {
    let refs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    encode_query(&refs)
}

/// 日志筛选意图 ⇒ `GET /v1/console/logs` 的查询串（设计 §3.4 请求列）。
///
/// - `range` **恒发**（档位是服务端算相对窗口的依据）；
/// - `from` / `to` **仅 `Custom` 档且两侧都给出时**才发 —— 相对档位下服务端按档位算，
///   发一对 `None` 折算出的 `0` 会把窗口钉死在 1970（**静默错窗口**，比不发危险得多）；
/// - `levels` / `targets` 空集合 ⇒ **不发**该键（契约：空 = 不筛，不是"筛空集"）；
/// - `cursor` 仅在 `Some` 时发（`None` = 从头取一页）；
/// - `limit` **恒发**（让请求侧不超量，`ROW_MAX` 由页面给定）。
pub fn log_query_string(q: &LogQuery) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let k = |s: &str| s.to_string();
    pairs.push((k("range"), log_range_wire(q.range).to_string()));
    if q.range == LogRange::Custom {
        if let (Some(from), Some(to)) = (q.from_ms, q.to_ms) {
            pairs.push((k("from"), from.to_string()));
            pairs.push((k("to"), to.to_string()));
        }
    }
    for l in &q.levels {
        pairs.push((k("levels"), log_level_wire(*l).to_string()));
    }
    for t in &q.targets {
        pairs.push((k("targets"), t.clone()));
    }
    if let Some(c) = q.cursor {
        pairs.push((k("cursor"), c.to_string()));
    }
    pairs.push((k("limit"), q.limit.to_string()));
    encode_pairs(&pairs)
}

/// 审计筛选意图 ⇒ `GET /v1/console/audit` 的查询串（设计 §3.4 请求列）。
///
/// `from` / `to` 的口径同 [`log_query_string`]（**仅 `Custom` 且两侧齐全**）；
/// `ops` 空集合 ⇒ 不发（契约：空 = 不筛）；`page` / `page_size` 恒发
/// （`page_size` 取契约常量 [`mupc_display_proto::AUDIT_PAGE_SIZE`]）。
pub fn audit_query_string(q: &AuditQuery) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let k = |s: &str| s.to_string();
    if q.range == LogRange::Custom {
        if let (Some(from), Some(to)) = (q.from_ms, q.to_ms) {
            pairs.push((k("from"), from.to_string()));
            pairs.push((k("to"), to.to_string()));
        }
    }
    for op in &q.ops {
        pairs.push((k("ops"), console_op_wire(*op).to_string()));
    }
    pairs.push((k("page"), q.page.to_string()));
    pairs.push((
        k("page_size"),
        mupc_display_proto::AUDIT_PAGE_SIZE.to_string(),
    ));
    encode_pairs(&pairs)
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 屏 → 网络：意图（**写操作只能由这里产生**）
// ═══════════════════════════════════════════════════════════════════════════

/// 页面意图回调交出的**控制意图**（纯数据）。
///
/// # 为什么意图要过队列（而不是在回调里直接发请求）
///
/// ① **T-3 门禁的结构性保证**：这些意图**只**由页面的确认完成回调产生（P2 的
///    长按/双步确认、P4 的 L2 长按）—— 未确认 ⇒ 回调不触发 ⇒ 队列为空 ⇒
///    **零 `begin_write` 调用**。写操作的发起点在 `app.rs` 里**只有** `ControlIntent` 的
///    `ConfigApply` / `Interlock*` 三条分支（见 `App::handle_control_intents`），而这三条
///    的唯一生产者是队列；
/// ② LVGL 事件回调内**不得**做网络动作（设计 §5.2 不变量 3 / §11.4），队列把"发请求"推迟到
///    事件派发之外的 `Host::on_lv_events`。
#[derive(Debug, Clone, PartialEq)]
pub enum ControlIntent {
    /// 写：配置保存 / 恢复默认值（`ConfigPatch.from` 区分，由页面给出）。
    ConfigApply(ConfigPatch),
    /// 写：联锁人工释放（`observed_*` 由页面给出，EDGE-19 的乐观并发检查载荷）。
    InterlockRelease(InterlockOpPayload),
    /// 写：M1 授权重启。
    InterlockAckM1(InterlockOpPayload),
    /// 读：日志「筛选变化」（`cursor` 恒 `None`，由页面给出）。
    LogQuery(LogQuery),
    /// 读：日志「增量拉取」（`cursor = Some(已见最大 seq)`）。
    LogIncrement(LogQuery),
    /// 读：日志「回到最新」（载荷 `()` ⇒ 由接线层用**上一次**查询重取首屏）。
    LogBackToLatest,
    /// 读：审计「筛选变化」（`page` 恒 1）。
    AuditQuery(AuditQuery),
    /// 读：审计「加载更多」（`page` = 当前页 + 1）。
    AuditLoadMore(AuditQuery),
    /// 读：P4 消防「查看明细 / 上一页 / 下一页」（载荷 = 目标页码，1 起；§15.3.2
    /// `GET /peripherals/fire_detectors`）。页大小由接线层按契约常量给（不由页面带）。
    FireDetectorPage(u32),
    /// 读：P6 电池段「查看全部 288 位」下钻的翻页（载荷 = 目标页码，1 起；§15.3.2
    /// `GET /peripherals/bms_alarms`）。
    BmsAlarmPage(u32),
    /// 读：**强制重取 catalog**（载荷 `()`；设计 §15.3.1 第 3 句的「重试」按钮；T21c-3-r1）。
    ///
    /// **为什么单开一条**：catalog 的读取**不走** `pending_reads` 查询队列 —— 它的时机判据是
    /// [`catalog_due`]（"首次进入 / rev 变化 / 不反复取"），发起点在 `App::tick_periph_catalog`。
    /// 故本意图的**处置**不是"发一条请求"，而是"把两个已消费的判据复位"（首次进入资格 +
    /// "失败过的 rev"抑制）—— 让**下一拍**的 `catalog_due` 重新成立，发起仍在 tick 路径上
    /// （与"页面回调只投意图、不在回调里发请求"同口径）。
    CatalogRetry,
}

impl ControlIntent {
    /// 是否为**写**意图（T-3 门禁的判据本体：写与读在"在途"时的**处置不同** ——
    /// 读意图可被新查询 `cancel()` 顶掉，写意图则**绝不被打断**、只能丢弃）。
    ///
    /// ⚠️ **订正（B3-2c 整改 重要 2）**：此前本行写「在途时被丢弃**并上屏提示**」——
    /// 当时（B3-2b-2）判为「不成立」，理由是丢弃路径落的 `ControlState` toast
    /// **无页面消费者**；**B3-2c 起前半句已不成立**：该 toast **有**消费者 —— app 层 Toast
    /// （`App::toast` + `toast_view` / `App::sync_toast`，每拍一次）。丢弃**只计数**
    /// （`App::write_intents_dropped` / `read_intents_dropped`）的真正原因是**该分支生产
    /// 不可达**（提交中两页按钮已 disabled）⇒ 屏上**走不到**，**不是没有上屏出口**。
    pub fn is_write(&self) -> bool {
        matches!(
            self,
            Self::ConfigApply(_) | Self::InterlockRelease(_) | Self::InterlockAckM1(_)
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. 纯逻辑单测（**不触碰 LVGL**）
//
// 每条断言旁写「改什么会让本条变红」。
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::OutcomeKind;
    use mupc_display_proto::{ControlCode, FieldError};

    fn outcome(ep: ConsoleEndpoint, kind: OutcomeKind<Value>) -> ConsoleOutcome<Value> {
        ConsoleOutcome { endpoint: ep, kind }
    }

    fn query(ep: ConsoleEndpoint, json: &str) -> ConsoleOutcome<Value> {
        outcome(ep, OutcomeKind::Query(serde_json::from_str(json).unwrap()))
    }

    fn resp(ep: ConsoleEndpoint, json: &str) -> ConsoleOutcome<Value> {
        let r: ControlResponse<Value> = serde_json::from_str(json).unwrap();
        outcome(ep, OutcomeKind::Response(r))
    }

    /// 一条最小 `ConfigView` 字面量（**字面量 JSON**，不用 display-proto 序列化 —— 避免
    /// "同源同错"掩盖契约偏差，与 `tests/offscreen_smoke.rs` 同法）。
    const CONFIG_VIEW: &str = r#"{"groups":[],"revision":3,"write_mode":"text_preserve"}"#;
    /// 一条最小 `LogPage` 字面量（四个必需字段齐全）。
    const LOG_PAGE: &str =
        r#"{"entries":[],"next_cursor":null,"has_more":false,"range_too_large":false}"#;
    /// 一条最小 `AuditPage` 字面量。
    const AUDIT_PAGE: &str = r#"{"entries":[],"page":1,"page_size":20,"has_more":false,"newest_ts_ms":null,"available":true}"#;
    /// 一条最小 `PeripheralCatalog` 字面量（`rev` 非 0 ⇒ 可判"解到的是本端点"）。
    const CATALOG: &str = r#"{"rev":7,"generated_ms":1,"stations":[]}"#;
    /// 一条最小 `FireDetectorPage` 字面量。
    const FIRE_PAGE: &str = r#"{"page":2,"page_size":20,"total":null,"expanded":3,
        "has_more":false,"available":true,"items":[]}"#;
    /// 一条最小 `BmsAlarmPage` 字面量（含 1 条位项 ⇒ 可判载荷真的解开了）。
    const BMS_PAGE: &str = r#"{"page":1,"page_size":50,"total":288,"active_total":4,
        "has_more":true,"available":true,"items":[{"at":5,"active":true}]}"#;

    // ── 路由表：11 个端点**逐条**钉死（8 个 B3 端点 + U-73 的 3 个外设端点）─────

    /// **本单元最关键的一条**：B3 的 8 个端点 → 目标页面/方法，逐条断言。
    ///
    /// **改什么会让本条变红**（**实测**，见交付报告「破坏性探针清单」）：把 [`route`] 里
    /// `ConsoleEndpoint::Logs` 那一臂改成 `RouteDecision::Config(..)` ⇒ 第 3 条立刻红
    /// （`left: Config` / `right: Logs`）；把 `LogsTargets` 与 `AuditOps` 互换 ⇒ 第 4 / 6 条红。
    /// 这一条就是"路由错了会红"的证据 —— 它**不需要**起真进程（像素上几乎看不出路由错页）。
    #[test]
    fn routing_table_is_pinned_endpoint_by_endpoint() {
        // ① Config → P2.set_config
        assert!(
            matches!(route(&query(ConsoleEndpoint::Config, CONFIG_VIEW)), Ok(RouteDecision::Config(v)) if v.revision == 3),
            "Config 端点必须路由到 P2 的 set_config 决策"
        );
        // ② ConfigApply → P2.show_result
        let apply_ok = format!(
            r#"{{"request_id":"r1","ok":true,"code":"ok","message":"m","applied":{CONFIG_VIEW},
                "field_errors":[],"audit_id":null,"duplicate":false,"at_ms":1}}"#
        );
        assert!(
            matches!(route(&resp(ConsoleEndpoint::ConfigApply, &apply_ok)), Ok(RouteDecision::ConfigApply(r)) if r.ok),
            "ConfigApply 回执必须路由到 P2 的 show_result 决策"
        );
        // ③ Logs → P3.set_page
        assert!(
            matches!(
                route(&query(ConsoleEndpoint::Logs, LOG_PAGE)),
                Ok(RouteDecision::Logs(_))
            ),
            "Logs 端点必须路由到 P3（set_page）"
        );
        // ④ LogsTargets → P3.set_targets
        assert_eq!(
            route(&query(ConsoleEndpoint::LogsTargets, r#"["sys","io"]"#)).unwrap(),
            RouteDecision::LogsTargets(vec!["sys".into(), "io".into()]),
            "LogsTargets 端点必须路由到 P3 的 set_targets 决策"
        );
        // ⑤ Audit → P5.set_page
        assert!(
            matches!(
                route(&query(ConsoleEndpoint::Audit, AUDIT_PAGE)),
                Ok(RouteDecision::Audit(_))
            ),
            "Audit 端点必须路由到 P5（set_page）"
        );
        // ⑥ AuditOps → P5.set_ops
        let ops = r#"[{"op":"config_apply","label":"配置保存"}]"#;
        assert!(
            matches!(route(&query(ConsoleEndpoint::AuditOps, ops)), Ok(RouteDecision::AuditOps(v)) if v.len() == 1),
            "AuditOps 端点必须路由到 P5 的 set_ops 决策"
        );
        // ⑦ InterlockRelease → P4.show_result
        let rel_ok = r#"{"request_id":"r2","ok":true,"code":"ok","message":"m",
            "applied":{"latched":false,"stopped":true},"field_errors":[],"audit_id":"a1",
            "duplicate":false,"at_ms":2}"#;
        assert!(
            matches!(
                route(&resp(ConsoleEndpoint::InterlockRelease, rel_ok)),
                Ok(RouteDecision::InterlockResult(r)) if r.applied.as_ref().is_some_and(|a| !a.latched)
            ),
            "InterlockRelease 成功回执必须路由到 P4 的 show_result 决策"
        );
        // ⑧ InterlockAckM1 → 同 ⑦（**两个端点都必须在表里**，漏一个就是静默不显）
        assert!(
            matches!(
                route(&resp(ConsoleEndpoint::InterlockAckM1, rel_ok)),
                Ok(RouteDecision::InterlockResult(_))
            ),
            "InterlockAckM1 必须与 release 走同一条（但各自成臂）"
        );
    }

    /// 失败回执的两条分支：`applied = None` **不是**路由错误；**一切**业务拒绝（含
    /// `RejectedPrecondition`）都走 `show_result` —— 让服务端 `message` 承担**具体原因**。
    ///
    /// # ⚠️ 本用例的语义订正（PM 裁定 1，2026-09-16）
    ///
    /// 它**曾经**断言「`RejectedPrecondition` ⇒ `RouteDecision::InterlockConflict`」
    /// （即 EDGE-19 的**固定**文案）—— 那条臂**锁的是错的语义**：`RejectedPrecondition` 把
    /// 「状态已变化」与「触发源未复位 / 保持时间不足 / latch / `StopPending`」糊在一个码里，
    /// 一律显固定文案 = **谎报原因**（见 `RouteDecision::InterlockResult` 的沿革段）。故该臂
    /// 已整条删除，本用例随之**改锁正确语义**：`code` 原样到达页面 + **服务端 `message` 逐字不被顶替**。
    ///
    /// **改什么会让本条变红**：把 [`typed`] 里的 `None => None` 改成 `None => Err(..)` ⇒ 第 1 条红；
    /// **在 `route` 里给 `RejectedPrecondition` 另开一条臂**（重写成固定 EDGE-19 文案、或改成
    /// 别的 `RouteDecision`）⇒ 第 2 条红（**已实测**，见报告「探针 1」）。
    #[test]
    fn failed_receipts_route_by_code_without_applied() {
        let rejected = r#"{"request_id":"r3","ok":false,"code":"rejected_validation","message":"端口越界",
            "applied":null,"field_errors":[],"audit_id":"a2","duplicate":false,"at_ms":3}"#;
        assert!(
            matches!(route(&resp(ConsoleEndpoint::ConfigApply, rejected)), Ok(RouteDecision::ConfigApply(r)) if !r.ok && r.applied.is_none()),
            "失败回执（applied = null）是**正常路由路径**，不得被当成错误"
        );

        // `RejectedPrecondition` 与其它拒绝**同一条路**：交给页面，服务端 `message` 是唯一真源。
        // 这里刻意用一份"真·状态变化"的 `message`（设计 TD:594 的服务端文案）——它应当**原样**
        // 到达页面（客户端不猜、也不替换成 EDGE-19 的固定串）。
        let conflict = r#"{"request_id":"r4","ok":false,"code":"rejected_precondition",
            "message":"联锁状态已变化，请刷新后重试","applied":null,"field_errors":[],"audit_id":"a3",
            "duplicate":false,"at_ms":4}"#;
        for ep in [
            ConsoleEndpoint::InterlockRelease,
            ConsoleEndpoint::InterlockAckM1,
        ] {
            let Ok(RouteDecision::InterlockResult(r)) = route(&resp(ep, conflict)) else {
                panic!("{ep:?} 的 RejectedPrecondition 必须与其它拒绝同路径（show_result）");
            };
            assert_eq!(
                r.code,
                ControlCode::RejectedPrecondition,
                "{ep:?}：`code` 必须原样到达页面（页面按它置「请求刷新」标志）"
            );
            assert_eq!(
                r.message, "联锁状态已变化，请刷新后重试",
                "{ep:?}：服务端 message 必须**逐字**到达页面 —— 客户端不得替换 / 猜测拒绝原因"
            );
        }
    }

    /// 裁定 3：传输失败 ⇒ **本地合成**一条「不可用」回执（写端点才合），字段逐条钉死。
    ///
    /// **改什么会让本条变红**（**均已实测**，见报告「探针 3 / 探针 5」）：
    /// - 把合成去掉（写端点 `=> None`）⇒ **①** 立刻红（"写端点必须有回执"）；
    /// - 改动**任一**字段（`code` → `ApplyFailed` 冒充服务端判决 / `message` 换成自造原因 /
    ///   `audit_id` 填一个 ID / `duplicate` 置真 / `applied` 给值）⇒ **①** 红
    ///   （`facts` 把全部字段拍成一个串，一并比对）；
    /// - 让读端点也合成 ⇒ **②** 红（会把"写回执"塞进没有 `show_result` 的页）。
    #[test]
    fn transport_failure_synthesizes_an_unavailable_receipt_for_write_endpoints_only() {
        let (rid, msg, at) = ("r-77", "操作失败", 12_345);
        /// 一条回执的**全部字段**拍扁成一个串（两种载荷形状共用一个判据 —— 免得为两个泛型
        /// 各写一份断言；少一项、多一项都会让下面的 `assert_eq!` 变红）。
        fn facts<T>(r: &ControlResponse<T>) -> String {
            format!(
                "ok={} code={:?} msg={} rid={} at={} applied_none={} fe_empty={} dup={} audit_none={}",
                r.ok,
                r.code,
                r.message,
                r.request_id,
                r.at_ms,
                r.applied.is_none(),
                r.field_errors.is_empty(),
                r.duplicate,
                r.audit_id.is_none(),
            )
        }
        // ① 三个写端点 ⇒ 各自的**页面既有入口** + 逐字段取值（见 `facts`）。
        let want = format!(
            "ok=false code=Unavailable msg={msg} rid={rid} at={at} applied_none=true \
             fe_empty=true dup=false audit_none=true"
        );
        for (ep, interlock) in [
            (ConsoleEndpoint::ConfigApply, false),
            (ConsoleEndpoint::InterlockRelease, true),
            (ConsoleEndpoint::InterlockAckM1, true),
        ] {
            let d = transport_failure_decision(ep, rid, msg, at).unwrap_or_else(|| {
                panic!("{ep:?} 是写端点 ⇒ 必须有本地合成回执（否则屏上什么都不发生）")
            });
            let got = match (&d, interlock) {
                (RouteDecision::ConfigApply(r), false) => facts(r),
                (RouteDecision::InterlockResult(r), true) => facts(r),
                _ => panic!("{ep:?} 必须落在本页既有的 show_result 入口上，实得 {d:?}"),
            };
            assert_eq!(
                got, want,
                "{ep:?}：本地合成回执的字段口径（成因 = 某字段被改动 / 被「顺手补齐」）"
            );
        }
        // ② 读端点**不**合成：它们的降级出口是 P3 通道条态（设计 §6.3），不是"写回执"。
        //    判据用**契约的端点清单**（`is_write`）⇒ 将来新增端点会落到这条断言上。
        for ep in ConsoleEndpoint::ALL.into_iter().filter(|e| !e.is_write()) {
            assert!(
                transport_failure_decision(ep, rid, msg, at).is_none(),
                "{ep:?} 是读端点 ⇒ 不该往页面的写回执入口塞东西"
            );
        }
    }

    /// 形态错配 / 解码失败**必须响亮**（不得静默当成"空数据"）。
    ///
    /// **改什么会让本条变红**：把 `route` 的两个 `ok_or(RouteError::Kind(..))` 改成 `.unwrap_or`
    /// 兜底 ⇒ 前两条红；把 `decode` 的 `map_err` 改成 `unwrap_or_default` ⇒ 第 3 条红
    /// （`LogPage: Default` 会让"畸形 JSON"静默变成"无日志"）。
    #[test]
    fn kind_and_decode_mismatches_are_loud() {
        // 查询端点收到控制信封
        let as_resp = resp(ConsoleEndpoint::Logs, rejected());
        assert!(matches!(
            route(&as_resp),
            Err(RouteError::Kind(ConsoleEndpoint::Logs))
        ));
        // 写端点收到裸 DTO
        let as_query = query(ConsoleEndpoint::InterlockRelease, "{}");
        assert!(matches!(
            route(&as_query),
            Err(RouteError::Kind(ConsoleEndpoint::InterlockRelease))
        ));
        // 解码失败：合法 JSON 但不是 `LogPage`（缺必需字段）
        let bad = query(ConsoleEndpoint::Logs, r#"{"entries":[]}"#);
        assert!(
            matches!(
                route(&bad),
                Err(RouteError::Decode(ConsoleEndpoint::Logs, _))
            ),
            "缺必需字段必须响亮失败，不得默认成「无日志」"
        );
        // 写回执的 `applied` 形状不对也要响亮
        let bad_applied = r#"{"request_id":"r5","ok":true,"code":"ok","message":"m",
            "applied":{"latched":false},"field_errors":[],"audit_id":null,"duplicate":false,"at_ms":5}"#;
        assert!(matches!(
            route(&resp(ConsoleEndpoint::InterlockRelease, bad_applied)),
            Err(RouteError::Decode(ConsoleEndpoint::InterlockRelease, _))
        ));
    }

    fn rejected() -> &'static str {
        r#"{"request_id":"r9","ok":false,"code":"rejected_validation","message":"m",
            "applied":null,"field_errors":[],"audit_id":null,"duplicate":false,"at_ms":9}"#
    }

    /// 幂等命中（`duplicate = true`）**原样搬运**，不得被当成错误或丢弃。
    #[test]
    fn duplicate_flag_is_carried_through_route() {
        let dup = r#"{"request_id":"r6","ok":true,"code":"ok","message":"m","applied":null,
            "field_errors":[{"field":"a.b","reason":"越界"}],"audit_id":"a4","duplicate":true,"at_ms":6}"#;
        let Ok(RouteDecision::ConfigApply(r)) = route(&resp(ConsoleEndpoint::ConfigApply, dup))
        else {
            panic!("应路由到 ConfigApply");
        };
        assert!(r.duplicate, "duplicate 是幂等命中的唯一标记，不得丢");
        assert_eq!(
            r.field_errors,
            vec![FieldError {
                field: "a.b".into(),
                reason: "越界".into(),
            }],
            "逐字段错误必须原样搬运（P2 靠它逐字段标红，CF-02）"
        );
    }

    // ── U-73 外设三端点：路由 / 失败面 / catalog 读取时机 ───────────────────

    /// 三个外设端点各自路由到**自己的**决策（**不得**互相串台，也不得落进 8 端点里的任何一条）。
    ///
    /// **改什么会让本条变红**：把 `PeripheralsFireDetectors` 那一臂写成
    /// `RouteDecision::PeripheralCatalog(..)` ⇒ 第 2 条红；把 `BmsAlarmPage` 与
    /// `FireDetectorPage` 互换 ⇒ 第 2 / 3 条红；删掉任一臂（落进 `_ => Err(Kind)`）⇒ 该条红。
    #[test]
    fn peripheral_endpoints_route_to_their_own_decisions() {
        // ① catalog（`rev` 真的解出来了 ⇒ 不是"随便一个 Default"）
        assert!(
            matches!(
                route(&query(ConsoleEndpoint::PeripheralsCatalog, CATALOG)),
                Ok(RouteDecision::PeripheralCatalog(c)) if c.rev == 7
            ),
            "catalog 必须路由到 PeripheralCatalog 决策（供 P4 + P6 两页）"
        );
        // ② 探测器分页
        assert!(
            matches!(
                route(&query(ConsoleEndpoint::PeripheralsFireDetectors, FIRE_PAGE)),
                Ok(RouteDecision::FireDetectorPage(p)) if p.page == 2 && p.expanded == 3
            ),
            "fire_detectors 必须路由到 P4 的 set_fire_page 决策"
        );
        // ③ BMS 告警分页（`items` 真的解开了）
        assert!(
            matches!(
                route(&query(ConsoleEndpoint::PeripheralsBmsAlarms, BMS_PAGE)),
                Ok(RouteDecision::BmsAlarmPage(p))
                    if p.total == 288 && p.items.len() == 1 && p.items[0].at == 5
            ),
            "bms_alarms 必须路由到 P6 的 set_bms_page 决策"
        );
        // ④ 三个都是**读**端点 ⇒ 形态判据走 `query()`（收到控制信封 ⇒ 响亮 `Kind`）
        for ep in [
            ConsoleEndpoint::PeripheralsCatalog,
            ConsoleEndpoint::PeripheralsFireDetectors,
            ConsoleEndpoint::PeripheralsBmsAlarms,
        ] {
            assert!(
                matches!(route(&resp(ep, rejected())), Err(RouteError::Kind(e)) if e == ep),
                "{ep:?}：裸 DTO 端点收到控制信封必须响亮拒绝（不得静默）"
            );
            // 载荷形状不符也必须响亮。⚠️ 判据**不能**用"缺字段的对象"或"短数组"：
            // 三个 DTO 都是 `#[serde(default)]`（§15.3.2）⇒ 空对象合法解析成 `Default`，
            // 连**序列**形态的 `[]` 也会被 serde 的位置式 `visit_seq` + 容器级 default 补成
            // `Default`（实测：本条第一版正是被这个坑判红）。取**标量**（既非 map 也非 seq）。
            assert!(
                matches!(route(&query(ep, "7")), Err(RouteError::Decode(e, _)) if e == ep),
                "{ep:?}：载荷类型不符必须响亮，不得默认成「空数据」"
            );
        }
    }

    /// 失败**决策**：**只有**两条"明细"端点有（§15.6.2 ⑥，页面暴露的是无参
    /// `set_*_page_failed()`）；catalog 与其余端点**不得**被塞决策。
    ///
    /// ⚠️ 本用例只管"**要不要交一条 `RouteDecision`**"这一问（T21c-3-r1 起它与"页面有没有
    /// 失败面"是**两问**：catalog 有面而**无**决策，见 [`page_owns_failure_surface`]）——
    /// 故此处**不**断言"catalog 失败 = 什么都不做"（那句在 T21c-3-r1 后不再准确：catalog
    /// 失败要置顶部提示条 + 回滚预置，只是不经 `RouteDecision`）。
    ///
    /// **改什么会让本条变红**：给 catalog 也返回一个决策（例如
    /// `RouteDecision::FireDetectorUnavailable`）⇒ 第 3 条红；把 `page_failure_decision`
    /// 写成恒 `None` 或把判断写反（`_ => Some(..)`）⇒ 第 1 / 2 条红。
    #[test]
    fn only_the_two_drill_endpoints_have_a_page_failure_surface() {
        assert_eq!(
            page_failure_decision(ConsoleEndpoint::PeripheralsFireDetectors),
            Some(RouteDecision::FireDetectorUnavailable),
            "探测器明细失败 ⇒ P4 就地显「明细不可用」+「重试」"
        );
        assert_eq!(
            page_failure_decision(ConsoleEndpoint::PeripheralsBmsAlarms),
            Some(RouteDecision::BmsAlarmUnavailable),
            "BMS 下钻失败 ⇒ P6 就地显「明细不可用」+「重试」"
        );
        for ep in ConsoleEndpoint::ALL {
            if matches!(
                ep,
                ConsoleEndpoint::PeripheralsFireDetectors | ConsoleEndpoint::PeripheralsBmsAlarms
            ) {
                continue;
            }
            assert!(
                page_failure_decision(ep).is_none(),
                "{ep:?} 没有失败**决策**（catalog 亦无：给它会调 `clear_catalog` ⇒ 把已取到的\
                 名字抹掉，与 §15.3.1「保留旧 catalog」相反；它的页面面是**提示条**，不经决策）"
            );
        }
    }

    /// 「哪些页用外设元数据」= P4（`Interlock`）/ P6（`System`）**两页**，其余四页不是。
    #[test]
    fn only_p4_and_p6_are_peripheral_metadata_pages() {
        for p in NavPage::ALL {
            assert_eq!(
                periph_metadata_page(p),
                matches!(p, NavPage::Interlock | NavPage::System),
                "{p:?} 的「是否外设页」判据漂移（首次进入即取 catalog 的落点，§15.3.1）"
            );
        }
    }

    /// **W-3（T21c-3-r1）**：「页面已有就地失败面」的**白名单恰好三条** —— 两个明细端点 +
    /// catalog；**其余读端点必须留在名单外**（它们唯一的降级通道就是 app 层兜底 Toast）。
    ///
    /// # 这条钉的正是"不得一刀切"
    ///
    /// `App::console_failure_decision` 用本函数决定"这一条失败要不要压通用「操作失败」Toast"。
    /// 若把实现放宽成 `!endpoint.is_write()`（"读端点都不压"），catalog 之外的 5 个读端点
    /// 会在失败时**屏上什么都不发生**（它们没有页面失败面）—— 违 §2.6「降级可见」，且是
    /// 本仓反复中招的"把有出路和没出路混为一谈"。
    ///
    /// **改什么会让本条变红**：实现里 `matches!(endpoint, PeripheralsCatalog)` 改成
    /// `!endpoint.is_write()`（或 `true`）⇒ 第 2 条红；漏掉 catalog ⇒ 第 1 条红；
    /// 给某读端点新增页面失败面却忘了登记 ⇒ 第 2 条红（同一清单两处漂移的防线）。
    #[test]
    fn failure_surface_roster_covers_catalog_without_widening_to_every_read_endpoint() {
        // ① 白名单**恰好**三条（枚举序取，便于对拍）
        let roster: Vec<ConsoleEndpoint> = ConsoleEndpoint::ALL
            .into_iter()
            .filter(|e| page_owns_failure_surface(*e))
            .collect();
        assert_eq!(
            roster,
            vec![
                ConsoleEndpoint::PeripheralsCatalog,
                ConsoleEndpoint::PeripheralsFireDetectors,
                ConsoleEndpoint::PeripheralsBmsAlarms,
            ],
            "「有页面就地失败面」的读端点必须**恰好**是 catalog + 两个明细端点"
        );
        // ② 其余读端点**一律不入列**（它们没有页面面 ⇒ app 层兜底 Toast 是唯一通道）
        for ep in ConsoleEndpoint::ALL
            .into_iter()
            .filter(|e| !e.is_write() && !page_owns_failure_surface(*e))
        {
            assert!(
                matches!(
                    ep,
                    ConsoleEndpoint::Config
                        | ConsoleEndpoint::Logs
                        | ConsoleEndpoint::LogsTargets
                        | ConsoleEndpoint::Audit
                        | ConsoleEndpoint::AuditOps
                ),
                "{ep:?} 不在白名单却也没有页面失败面 —— 白名单与端点清单漂移（新端点必须显式裁定）"
            );
        }
        // ③ 两处判据的**包含关系**：有 `RouteDecision` ⇒ 必在"有页面失败面"名单里
        //    （否则会出现"交了决策却仍压通用 Toast"这种自相矛盾的接线）。
        for ep in ConsoleEndpoint::ALL {
            if page_failure_decision(ep).is_some() {
                assert!(
                    page_owns_failure_surface(ep),
                    "{ep:?} 有失败决策却不在「有页面失败面」名单里 ⇒ 接线会同时上屏两条（UI §7.2）"
                );
            }
        }
    }

    /// **catalog 读取时机**（§15.3.1）：首次进入 P4/P6 取一次；此后**只有** `rev` 变化才重取；
    /// **不得**在 1 Hz 主拍上反复取。
    ///
    /// 本用例是**探针 ①②** 的靶子（见交付报告）：
    /// ① 删掉 `on_periph_page && !entered_before` 那一支 ⇒ 第 1 / 2 条红；
    /// ② 删掉 `(Some(r), Some(h)) => r != h` 那一支 ⇒ 第 3 条红。
    #[test]
    fn catalog_is_fetched_once_per_entry_and_then_only_on_rev_change() {
        // ① **首次进入** P4 或 P6：即使**还没有帧**（`frame_rev = None`）也要取一次 ——
        //    否则"进入页面时名称一片「名称未获取」、要等下一帧才补"（§15.3.1 的口径是进入即取）。
        assert!(
            catalog_due(true, false, None, None, None),
            "首次进入 P4/P6 ⇒ 必须取一次"
        );
        assert!(
            catalog_due(true, false, Some(7), Some(7), None),
            "首次进入优先于 rev 判据"
        );
        // ② **不在** P4/P6（P1/P2/P3/P5）⇒ 从不取（哪怕从未取过）
        for p in [
            NavPage::Main,
            NavPage::Config,
            NavPage::Logs,
            NavPage::Audit,
        ] {
            assert!(
                !periph_metadata_page(p)
                    && !catalog_due(periph_metadata_page(p), false, Some(7), None, None),
                "{p:?} 不是外设页 ⇒ 不得在开机时就取 catalog"
            );
        }
        // ③ `rev` 变化 ⇒ 重取；`rev` 相同 ⇒ **不取**（这就是"一次性"的判据本体）
        assert!(
            catalog_due(true, true, Some(8), Some(7), None),
            "帧内 rev 变了 ⇒ 重取"
        );
        assert!(
            !catalog_due(true, true, Some(7), Some(7), None),
            "rev 未变 ⇒ 不得重取（1 Hz 主拍上不许反复取）"
        );
        // ④ 还没拿到过 catalog（`held = None`）⇒ **不**把"没有"当成"变了"（否则每拍都命中）
        assert!(
            !catalog_due(true, true, Some(7), None, None),
            "从未取到时不按 rev 变化反复发起"
        );
        // ⑤ 还没有帧（`frame = None`）且已进入过 ⇒ 不取（等帧到了再比）
        assert!(!catalog_due(true, true, None, Some(7), None));
        assert!(!catalog_due(true, true, None, None, None));
    }

    /// **W-1 + 「重试」的时机语义**（T21c-3-r1；设计 §15.3.1 第 2 / 3 句）：
    /// ① 失败过的那个 rev **不自动重发**（否则"回滚 ⇒ rev 不等"会每拍命中）；
    /// ② 帧内 rev **一变**即恢复自动重取（含"从未取到"这一态 ⇒ 自动路径在该态也有效）；
    /// ③ 「重试」= 复位资格 + 清抑制 ⇒ 立刻再 due **一次**，随后**不再**每拍 due。
    ///
    /// **改什么会让本条变红**：
    /// ① 删掉 `failed_rev` 那一支 ⇒ 第 1 / 2 条红；
    /// ② 把比对用的 `held_rev.or(failed_rev)` 改回 `held_rev` ⇒ 第 4 条红（"从未取到 + 重试失败"
    ///    之后 rev 变化**永不**自动重取 —— 这正是回滚接 W-1 前的那半个坑）；
    /// ③ 删掉首进入那一支 ⇒ 第 5 条红。
    #[test]
    fn catalog_failure_suppresses_the_same_rev_and_retry_forces_exactly_one_more_fetch() {
        // ① `held` 已被 W-1 回滚成**旧值 5**（= 失败前的值），失败 rev = 7、帧内 rev 也是 7
        //    ⇒ **不取**（抑制生效；无抑制时第 2 支会真 ⇒ 每拍命中）。
        assert!(
            !catalog_due(true, true, Some(7), Some(5), Some(7)),
            "同一 rev 失败过 ⇒ 不得自动重发（否则回滚把'一次性'变成'每拍一次'）"
        );
        // ② **从未取到**（回滚成 `None`）+ 重试失败 ⇒ 同一 rev 同样不重发。
        assert!(
            !catalog_due(true, true, Some(7), None, Some(7)),
            "从未取到时同 rev 也不得自动重发"
        );
        // ③ 帧内 rev **变了** ⇒ 自动重取恢复（两种 held 都成立）。
        assert!(
            catalog_due(true, true, Some(8), Some(5), Some(7)),
            "帧内 rev 变化 ⇒ 自动路径仍有效（W-1 的诉求）"
        );
        assert!(
            catalog_due(true, true, Some(8), None, Some(7)),
            "从未取到 + 失败过 ⇒ 帧内 rev 一变仍要重取（比对取 `held.or(failed)`）"
        );
        // ④ 「重试」= 复位首进入资格 + 清失败抑制 ⇒ 立刻再 due。
        assert!(
            catalog_due(true, false, Some(7), None, None),
            "「重试」必须能强制再取一次（否则提示条上的按钮点了没反应）"
        );
        // ⑤ 但**发起之后**（资格已消费 + `catalog_rev` 已预置成当拍 rev）⇒ 不再 due
        //    —— 「重试」**不会**变成每拍重复发起。
        assert!(
            !catalog_due(true, true, Some(7), Some(7), None),
            "重试发起后不得每拍反复发起（预置 + 资格消费把两支同时封住）"
        );
        // ⑥ 帧内没有目录信息（`frame = None`）⇒ 恒否（等帧到了再比）。
        assert!(!catalog_due(true, true, None, Some(5), Some(7)));
    }

    // ── P3 通道态 ─────────────────────────────────────────────────────────

    /// P3 通道态 = **控制通道**连续失败 ≥2 ⇒ 断开；成功一次即回绿。
    ///
    /// **改什么会让本条变红**：把阈值从 2 改成 1 ⇒ 第 2 条红；把判据写成恒真 ⇒ 第 3 条红。
    /// **"换成帧通道驱动会红"的证据**在进程级用例
    /// `tests/control_channel.rs::p3_channel_state_follows_the_control_channel_not_the_frame_channel`：
    /// 帧通道**接了就断**、控制通道**正常**时，退出报告必须是 `p3_channel=up` ——
    /// 若驱动源被换成帧通道，那里就会打印 `down`，用例当场红。
    #[test]
    fn p3_channel_uses_control_fail_streak_with_threshold_two() {
        assert!(
            p3_connected(0),
            "上电初值 = 已连接（断开是失败驱动的降级态）"
        );
        assert!(
            p3_connected(1),
            "单次失败不足以判死（设计 §6.3：连续 2 次）"
        );
        assert!(!p3_connected(2), "连续 2 次失败 ⇒ 断开");
        assert!(!p3_connected(u32::MAX), "长时间失败必须仍是断开");
        assert_eq!(P3_DOWN_FAIL_STREAK, 2, "阈值来源 = 设计 §6.3 原文");
    }

    // ── 查询串 ───────────────────────────────────────────────────────────

    /// 日志查询串：相对档位**不发** `from`/`to`；多值**重复键**；空集合不发键。
    ///
    /// **改什么会让本条变红**：把 `if q.range == LogRange::Custom` 去掉 ⇒ 第 1 条红
    /// （相对档位会带上 `from=0&to=0`，把窗口钉死在 1970）。
    #[test]
    fn log_query_string_matches_section_3_4() {
        let q = LogQuery {
            range: LogRange::H1,
            from_ms: None,
            to_ms: None,
            levels: vec![
                mupc_display_proto::LogLevel::Error,
                mupc_display_proto::LogLevel::Warn,
            ],
            targets: vec!["mupc::io".to_string()],
            cursor: None,
            limit: 20,
        };
        let s = log_query_string(&q);
        assert_eq!(
            s, "range=1h&levels=error&levels=warn&targets=mupc%3A%3Aio&limit=20",
            "实得 {s}"
        );
        assert!(
            !s.contains("from="),
            "相对档位不得发 from（会把窗口钉死在 1970）"
        );
        assert!(!s.contains("cursor="), "cursor = None 不发该键");

        // Custom 档：两侧齐全才发
        let custom = LogQuery {
            range: LogRange::Custom,
            from_ms: Some(1_789_000_000_000),
            to_ms: Some(1_789_003_600_000),
            levels: Vec::new(),
            targets: Vec::new(),
            cursor: Some(77),
            limit: 20,
        };
        let s = log_query_string(&custom);
        assert_eq!(
            s, "range=custom&from=1789000000000&to=1789003600000&cursor=77&limit=20",
            "实得 {s}"
        );
        assert!(!s.contains("levels="), "空级别集合 = 不筛，不得发该键");
    }

    /// 审计查询串：`ops` 多值重复键 + `page` / `page_size` 恒发。
    #[test]
    fn audit_query_string_matches_section_3_4() {
        let q = AuditQuery {
            range: LogRange::H24,
            from_ms: None,
            to_ms: None,
            ops: vec![
                mupc_display_proto::ConsoleOp::InterlockRelease,
                mupc_display_proto::ConsoleOp::InterlockAckM1,
            ],
            page: 3,
        };
        assert_eq!(
            audit_query_string(&q),
            "ops=interlock_release&ops=interlock_ack_m1&page=3&page_size=20",
        );
        // 空 ops ⇒ 不筛（不发键）；page/page_size 仍在
        let all = AuditQuery {
            ops: Vec::new(),
            ..q.clone()
        };
        assert_eq!(audit_query_string(&all), "page=3&page_size=20");
    }

    /// 线上名与 `display-proto` 的 serde 形态**逐条对拍**（防两份真源漂移）。
    ///
    /// **改什么会让本条变红**：把 `log_level_wire` 的 `"warn"` 写成 `"warning"` ⇒ 红。
    #[test]
    fn wire_names_match_serde_forms() {
        use mupc_display_proto::{ConsoleOp, LogLevel};
        let quote = |v: serde_json::Value| v.as_str().unwrap().to_string();
        for l in [
            LogLevel::Error,
            LogLevel::Warn,
            LogLevel::Info,
            LogLevel::Debug,
            LogLevel::Trace,
        ] {
            assert_eq!(
                log_level_wire(l),
                quote(serde_json::to_value(l).unwrap()),
                "LogLevel 线上名与契约 serde 形态不一致"
            );
        }
        for op in ConsoleOp::ALL {
            assert_eq!(
                console_op_wire(op),
                quote(serde_json::to_value(op).unwrap()),
                "ConsoleOp 线上名与契约 serde 形态不一致"
            );
        }
        for r in [LogRange::H1, LogRange::H24, LogRange::Custom] {
            assert_eq!(log_range_wire(r), quote(serde_json::to_value(r).unwrap()));
        }
    }

    /// 写意图是 T-3 门禁的判据本体：只有 `ConfigApply` / `Interlock*` 三条算"写"。
    #[test]
    fn write_intents_are_exactly_the_three_post_endpoints() {
        let w = [
            ControlIntent::ConfigApply(ConfigPatch {
                changes: serde_json::Map::new(),
                from: mupc_display_proto::PatchSource::Edit,
            }),
            ControlIntent::InterlockRelease(InterlockOpPayload {
                observed_latched: false,
                observed_sources: Vec::new(),
            }),
            ControlIntent::InterlockAckM1(InterlockOpPayload {
                observed_latched: false,
                observed_sources: Vec::new(),
            }),
        ];
        assert_eq!(w.iter().filter(|i| i.is_write()).count(), 3);
        assert!(!ControlIntent::LogBackToLatest.is_write());
        assert!(!ControlIntent::LogQuery(LogQuery::default()).is_write());
        assert!(!ControlIntent::AuditLoadMore(AuditQuery::default()).is_write());
        // U-73：两个下钻翻页是**读**意图（`is_write` 的判据本体 —— 写意图在途时绝不被打断）
        assert!(!ControlIntent::FireDetectorPage(1).is_write());
        assert!(!ControlIntent::BmsAlarmPage(1).is_write());
        // 写意图的条数必须与契约的写端点条数一致（新增写端点却忘了加意图 ⇒ 红）
        assert_eq!(
            w.len(),
            ConsoleEndpoint::ALL
                .into_iter()
                .filter(|e| e.is_write())
                .count()
        );
    }

    // ⚠️ **已删除的恒真用例（B3-2b-2 整改 建议 4）**：
    // `progress_typing_is_usable_from_this_module` 自造一个 `Progress::Pending` 再匹配
    // `Progress::Pending`（`matches!` 的两侧是同一个被构造出来的值）⇒ **永不失败**，
    // 探测力为零，且不构成"输入形态"的回归锚点：本模块的 `route` 入参是
    // `&ConsoleOutcome<..>`、**根本不消费** `Progress`（消费点在
    // `app.rs::App::tick_console` 的 `Progress::Done` 解构）。
    // 连带删除 tests 段里仅供它使用的 `Progress` 导入（否则 `unused_imports` 告警）。
}
