//! 控制通道宿主（`127.0.0.1:9811`）——开发单元 **G-1：控制通道宿主 + 配置读路径**
//! ＋ **G-2：配置写路径（`POST /v1/console/config/apply`）**。
//!
//! 对应设计（`docs/superpowers/plans/modules/12-MUPC-本地显示终端-设计文档.md`）：
//! - §3.3 控制通道：通用信封与管线（**G-1 落读、G-2 落写全 8 步**）；
//! - §3.4 控制通道端点清单（8 条；已落 4 条：`GET /v1/console/config` + `POST /v1/console/config/apply`
//!   + **单元 H 的 `GET /v1/console/logs` 与 `GET /v1/console/logs/targets`**）；
//! - §3.4 补注（2026-09-15）：**GET 返回裸 DTO、POST 走 `ControlResponse` 信封**；
//!   **GET 失败一律非 2xx**（渲染端 `console.rs` 落 `Error::HttpStatus`，不解析错误体）；
//!   **未实现的路由不得"假装成功"**（本模块对已登记但未实现的端点回 **501**，未知路径 **404**，
//!   方法不符 **405**——三者在 `router()` 里由「路径 → 方法」注册表结构性保证，非纪律要求）；
//! - §4.3.2 `ConfigFieldMeta` 静态表 / §4.3.3 ApplyMode 分发表（字段集与 `requires_reconnect` 的真源）；
//! - §4.9 启动装配（10.2 控制通道，与读通道 9810 **并存不冲突**：不同端口、不同 listener）。
//!
//! ## G-2（配置写）在本文件的落点
//!
//! - [`ApplySource`]：写路径的装配状态（`Ready(ConfigService)` / `Unavailable(原因)`）；
//! - [`post_config_apply`]：POST handler（**POST 的所有结局都回 HTTP 200 + 信封**，见下）；
//! - [`ConfigFieldMeta::set`] / [`set_field`]：字段表的**写侧**（读侧是 `current`）——
//!   读写共用一张表 ⇒ 不存在"屏上能改的键"与"装置里能写的键"两张清单。
//!
//! ## 本单元的范围与**未做**的部分（如实登记）
//!
//! - 其余 4 条端点（`audit` / `audit/ops` / `interlock/release` / `interlock/ack_m1`）**已登记路由
//!   但返回 501**——它们各自的 `ConsoleAuditService` / `InterlockOps` 属后续单元。路由**不隐藏**：
//!   屏上对未实现端点的请求会得到明确的 501（渲染端 → `HttpStatus(501)` 失败提示），而不是被静默
//!   当成"服务不可用"或"空数据"。
//! - **写路径的"生效"只到位一部分**（⚠️ 计数口径，评审重要 5 已更正）：字段表 `FIELDS` 共
//!   **9** 键，其中 `editable=true` 的**可写字段 7 个**；这 7 个里 **1 个真热生效**
//!   （`system.log_level`，`tracing_subscriber::reload`），**其余 6 个**（`intercore.*` 4 +
//!   `gateway.*` 2）本轮**未接线**（逐条原因见 `hot_apply.rs` 表）⇒ 回执 / 审计 / 日志
//!   **如实**声明"需重启"。
//!   ⚠️ **跨模块缺口（评审重要 4）——✅ 已裁定并修复（2026-09-16）**：渲染端曾**反向陈述**
//!   （`local-display/src/ui/pages/p2_config.rs` 的常驻说明与 `TEXT_IMPACT_SAVE` 都写
//!   「修改保存后立即生效 · **无需重启装置**」），且成功分支**丢弃**后端 `message`（只用固定
//!   「保存成功 · 已生效」）⇒ 用户看不到这条如实结论。**PM 裁定（2026-09-16）：接受降级
//!   （不投入"把 6 个字段做成真热生效"的改造）+ 改屏上文案 + 回写 PRD（CF-04 降级）**。
//!   渲染层已同步（单元 **B3-2c 文案收口**，偏差登记 PD24）：两处文案改为分级口径
//!   「日志级别立即生效 · 连接类参数需重启进程生效」，成功分支**并入回执 `message`**
//!   （`success_toast_text`）⇒ 后端（回执 `message` / 审计 `reason` / `tracing::warn!`）
//!   三处的真话**已直达屏面**。**后端侧本文档块以下的口径与行为均未变**（本单元只改注释）。
//!
//! ## POST 的错误通道（**与 GET 不同**，这是本单元拍的口径）
//!
//! §3.4 补注只规定了 **GET** 失败走 HTTP 状态码。**POST 的失败必须走信封**：
//! 渲染端 `console.rs::parse` 对写端点**只**解 `ControlResponse`，**非 200 一律收口为
//! `HttpStatus`**（→ 屏上只剩一句通用"操作失败"，**丢掉具体原因**，而 EDGE-10 / CF-02
//! 要求"具体原因"）。故本 handler 对**一切**写请求（含信封本身解析失败、写路径未装配）
//! 都回 **HTTP 200 + 一个 `ok=false` 的信封** —— 与 `501` 的分工是：
//! **"端点没实现"用 501（结构性事实），"实现但这次没做成"用信封**。
//!
//! ## GET 日志两条端点的错误通道（单元 H，与 POST 相反、与 `GET /config` 一致）
//!
//! 两条日志端点**都是 GET** ⇒ 失败**一律非 2xx**（§3.4 补注），落在四种 HTTP 状态上：
//!
//! | 情形 | HTTP | 说明 |
//! |------|------|------|
//! | 查询参数非法（未知键 / 逗号拼多值 / `limit` 越界 / `custom` 缺 `from`/`to` …） | **400** | 具体原因在**响应体**（人读；渲染端不解析、只记 `HttpStatus(400)`） |
//! | 日志源不可读（目录不存在 / 无权限 / 文件打不开） | **503** | **绝不**回 200 + 空列表 —— 那会被屏上读成 EDGE-08「没有日志」（静默失实） |
//! | 检索范围超限（EDGE-15） | **200** | ⚠️ **不是错误**：`LogPage{ entries: [], range_too_large: true }` 是**正常回包**，屏上据此显超限提示 |
//! | 正常 | **200** | 裸 `LogPage` / 裸 `Vec<String>` |
//!
//! 「空结果」与「超限」是**两个不同的正常回包**（`range_too_large` 区分），而「源不可用」是**失败**
//! （503）。三者互不替代：本模块把"我不知道"与"确实是空的"当成**两件事**（§8.3 硬口径）。
//!
//! ## `requires_reconnect` 的**唯一真源**
//!
//! 设计 §4.3.3 分发表（文件行 **783–791**）是 `requires_reconnect` 的判据来源；渲染端
//! `ui/pages/p2_config.rs::save_level` 据「**本次改动涉及**的字段」在 L1 与 L2+ 之间分级
//! （PM 裁定 2026-09-15，见设计文档行 30 与 §6.2 保存行）——**错一个字段就会让屏上的确认分级
//! 与副作用提示失真**。故本文件的每一行 `requires_reconnect` 都带设计行号引用，并由
//! `tests::requires_reconnect_matches_design_section_4_3_3_per_field` 逐字段钉死。
//!
//! ## 字段集的**逐行差异**（如实登记：比 UI §6.2 **少 3 行、多 2 行**）
//!
//! 本单元落地的 [`FIELDS`]（9 行）与 UI §6.2「组与字段」表（行 506–514，7 行）**并不相同**——
//! 两个方向都如实登记，不做"只报少、不报多"的半截陈述：
//!
//! - **少 3 行**：`gateway.heartbeat_interval`（IEC 104 心跳间隔）、`intercore.local_port`
//!   （核间本地端口）、`telemetry.report_interval_sec`（遥测上报周期）——三者在现网 `CoreConfig`
//!   中**没有承载字段**（设计 §4.3.3 自己标注"新增字段"或根本未建模）⇒ 本单元**不把它们
//!   放进 `ConfigView`**（放进去就等于造一个"屏上能改、装置里不存在"的键——正是 §11.3
//!   「配置元数据一致性测试」要防的静默失效）：见 [`PENDING_NO_CARRIER`]。
//! - **多 2 行**：`intercore.heartbeat_interval_sec` / `intercore.reconnect_interval_sec` ——
//!   二者在**设计 §4.3.3（行 788「核间心跳/重连间隔」，生效方式 `watch` → 心跳循环读新值、
//!   时效"下一拍"、副作用"无"）**里明确列出，**只是 UI §6.2 的字段表没有它们**。
//!   ⇒ 实现**忠实于设计 §4.3.3**；差异的性质是 **UI §6.2 与设计 §4.3.3 两份清单不同步**，
//!   **不是**实现擅自加字段（本文件每行 `requires_reconnect` 都带 §4.3.3 行号引用，见下）。
//!
//! （计数口径：与 UI §6.2 的**可编辑字段行**表比对。另有 `intercore.host` 一行出自设计 §4.3.3
//! 行 787「对端端口 `intercore.port` / `intercore.host`」，两行只读服务地址出自 §3.4 行 624–626
//! 与 §6.2 行 1232 的只读行——三者不在这两张表的差集口径内。）
//!
//! ⚠️ **"缺行"在屏上是静默缺失**：`ConfigView.groups` 只表达"有什么"，不表达"少了什么"——
//! 渲染端只能按收到的 `groups` 渲染，**无法**自行提示"设计里还有 3 项没上屏"。故这三行的
//! 处置（补承载字段 / 从设计撤下 / 写进用户文档明示）是 **PM 裁定项**，读路径无法自行了结。

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use mupc_display_proto::{
    ConfigField, ConfigGroup, ConfigKind, ConfigPatch, ConfigView, ConsoleEndpoint, ControlCode,
    ControlRequest, ControlResponse, FieldError, OptionItem, WriteMode,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

use crate::config_service::{now_ms, ConfigService};
use crate::core_config::CoreConfig;

// ═══════════════════════════════════════════════════════════════
// 配置源（内存副本 / 不可用）
// ═══════════════════════════════════════════════════════════════

/// `ConfigView` 的数据来源。
///
/// **为什么是枚举而不是"总是给 Arc"**：`GET` 的失败通道被契约定死为非 2xx（§3.4 补注）——
/// 后端必须有一个**显式**的"配置源不可用"态才能诚实地回 503。若只持有 `Arc<RwLock<CoreConfig>>`，
/// 一旦装配缺失就只能回一个**空视图**，那正是"用 200 + 空数据冒充成功"（本单元诚实性红线）。
#[derive(Clone)]
#[allow(dead_code)] // `Unavailable` 由单测构造（诚实性网 ②）；生产当前恒 `Ready`
pub enum ConfigSource {
    /// 装配完成：进程内唯一权威读源（设计 §4.3.2「内存副本」）。
    Ready(Arc<RwLock<CoreConfig>>),
    /// 配置源不可用（如装配缺失）。原因串**如实**进 503 响应体（仅日志用，不进屏）。
    Unavailable(&'static str),
}

/// **写路径**的装配状态（设计 §4.3.2 的 `ConfigService`；G-2 引入）。
///
/// 与 [`ConfigSource`] 同款"显式不可用态"的理由：写路径缺件（真源路径没传 / 审计建不起来）
/// 时**必须**有一个能回 `Unavailable` 的态，而**不是**"降级成一个没有审计的写路径"——
/// 后者正是 fail-closed 要防的事（设计 §3.3：审计是唯一操作凭据）。
#[derive(Clone)]
#[allow(dead_code)] // `Unavailable` 由单测构造（诚实性网）；生产装配两种态都可能出现
pub enum ApplySource {
    /// 写路径就绪。
    Ready(Arc<ConfigService>),
    /// 写路径不可用（原因串**如实**进 `tracing`；回执 `message` 取固定串
    /// [`receipt::WRITE_PATH_UNAVAILABLE`]，见 `receipt` 模块头的用字约束）。**一切写请求都会被拒**。
    ///
    /// 回执 `code` 取 [`ControlCode::AuditUnavailable`]（评审建议 6.1 的口径统一）：装配侧
    /// 产生本态的唯一成因是"审计 sink 建不起来"（`startup::console_apply_source`）⇒ 让屏上
    /// 落到 EDGE-18 的固定文案，而不是通用"操作失败"。
    Unavailable(&'static str),
}

/// **日志源**的装配状态（单元 H；`GET /v1/console/logs` 与 `/logs/targets` 的数据来源）。
///
/// 与 [`ConfigSource`] 同款"显式不可用态"的理由：日志**目录读不出来**（不存在 / 无权限）时
/// 必须有一个能回 **503** 的态，而**不是**降级成"空列表"——后者在屏上就是 EDGE-08
/// 「当前筛选条件下无日志」，把"读不到"伪装成"确实没有"（本项目硬红线）。
#[derive(Clone)]
#[allow(dead_code)] // `Unavailable` 由单测构造（诚实性网 ②）；生产当前恒 `Ready`
pub enum LogSource {
    /// 日志服务就绪（`{system.log_dir}` 的文件扫描；设计 §4.4）。
    Ready(Arc<crate::log_service::LogService>),
    /// 日志源不可用（装配缺失）。原因串**如实**进 503 响应体（仅现场排障，不进屏）。
    Unavailable(&'static str),
}

/// 宿主依赖（设计 §4.9 `ConsoleDeps` 的可落子集；其余字段（`interlock` / `apply_registry`）
/// 随后续单元引入）。
#[derive(Clone)]
pub struct ConsoleDeps {
    /// 配置读源。
    pub config: ConfigSource,
    /// 配置写源（G-2）。
    pub apply: ApplySource,
    /// 日志源（单元 H）。
    pub logs: LogSource,
}

/// 控制通道宿主：持有路由表与依赖，`serve()` 消费一个**已绑定**的 listener。
///
/// 端口注入方式与 `LoopbackHttpPublisher` 同款（`display_host.rs`）：**listener 由调用方绑定**，
/// 宿主只 `serve`。测试因此可绑 `127.0.0.1:0` 取随机端口（不与他例互撞），生产则由
/// `startup.rs` 按 `display.control_bind_addr` 绑定。
pub struct ConsoleHost {
    deps: ConsoleDeps,
}

impl ConsoleHost {
    /// 构造。
    pub fn new(deps: ConsoleDeps) -> Self {
        Self { deps }
    }

    /// **装配级「唯一真源」句柄**（仅测试可见）：返回宿主持有的配置读源本身。
    ///
    /// 存在理由是**一条网**，不是 API：`CoreConfig: Clone` ⇒ 装配点若把权威副本
    /// **深拷贝成新 `Arc`** 再接进来（`Arc::new(RwLock::new(arc.read().await.clone()))`），
    /// 类型系统与其余用例**都看不出来**，而后果是「装配写 A、控制通道读 B」——G-2 的写入
    /// 在屏上静默不生效。单测 `console_config_source_is_arc_identical_to_assembly_handle`
    /// 用 `Arc::ptr_eq` 把地址级同一性钉死，本方法即那把尺子的取数口。
    #[cfg(test)]
    pub(crate) fn config_source(&self) -> &ConfigSource {
        &self.deps.config
    }

    /// 路由表（**唯一真源 = `ConsoleEndpoint::ALL`**，不手抄路径串，杜绝路由漂移）。
    ///
    /// 三条诚实性保证由 axum 的 `MethodRouter` 结构性给出：
    /// - 路径**已登记**且方法相符 → 走 handler（本单元：`config` 200 / 其余 501）；
    /// - 路径已登记但**方法不符** → **405**（如 `POST /v1/console/config`）；
    /// - 路径**未登记** → **404**。
    pub fn router(&self) -> Router {
        let mut router = Router::new();
        for ep in ConsoleEndpoint::ALL {
            let path = ep.path();
            router = match ep.method() {
                mupc_display_proto::ConsoleMethod::Get if ep == ConsoleEndpoint::Config => {
                    router.route(path, get(get_config))
                }
                // 单元 H：日志两条 GET（设计 §3.4 / §4.4）
                mupc_display_proto::ConsoleMethod::Get if ep == ConsoleEndpoint::Logs => {
                    router.route(path, get(get_logs))
                }
                mupc_display_proto::ConsoleMethod::Get
                    if ep == ConsoleEndpoint::LogsTargets =>
                {
                    router.route(path, get(get_logs_targets))
                }
                mupc_display_proto::ConsoleMethod::Get => router.route(path, get(not_implemented)),
                mupc_display_proto::ConsoleMethod::Post
                    if ep == ConsoleEndpoint::ConfigApply =>
                {
                    router.route(path, axum::routing::post(post_config_apply))
                }
                mupc_display_proto::ConsoleMethod::Post => {
                    router.route(path, axum::routing::post(not_implemented))
                }
            };
        }
        router.with_state(HostState {
            config: self.deps.config.clone(),
            apply: self.deps.apply.clone(),
            logs: self.deps.logs.clone(),
        })
    }

    /// 服务循环。**只接受回环 listener**（PL-4 / EDGE-24 安全红线）。
    ///
    /// **这是二次兜底，不是唯一防线**：配置层 `CoreConfig::validate_display`
    /// （`core_config.rs:556-575`）**已**调用契约的 `DisplayConfig::validate()`
    /// （`display-proto/src/config.rs`），后者对 `display.bind_addr` 与
    /// `display.control_bind_addr` **两条**都强制字面量回环（`127.0.0.1` / `::1`）、端口 ≠ 0、
    /// 且两址不得相同 ⇒ 把 `display.control_bind_addr` 写成 `0.0.0.0:9811` 在 `mupcd` 启动期
    /// 即被 `validate()` 拒绝（fail-fast，进程根本不起）。
    ///
    /// 本 `serve` 的回环判断是**按实际绑定结果**的**二次兜底**：防的是「配置校验被绕过 /
    /// 被新增调用路径跳过」（例如将来某条路径自行 `bind` 后直接 `serve`，绕开 `CoreConfig`）。
    /// 取 `listener.local_addr()` 而非配置串：判的是**实际**绑定结果（`0.0.0.0` / `::` 一律拒绝）。
    pub async fn serve(self, listener: TcpListener) -> std::io::Result<()> {
        let addr = listener.local_addr()?;
        if !addr.ip().is_loopback() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "ConsoleHost 拒绝服务非回环地址 {addr}——控制通道仅允许 127.0.0.1/::1（PL-4 安全红线）"
                ),
            ));
        }
        tracing::info!(
            "本地显示终端控制通道已启动: http://{}{}（回环仅本机；已实现 GET {} / {} / {} 与 POST {}，其余 4 条端点 501）",
            addr,
            ConsoleEndpoint::Config.path(),
            ConsoleEndpoint::Config.path(),
            ConsoleEndpoint::Logs.path(),
            ConsoleEndpoint::LogsTargets.path(),
            ConsoleEndpoint::ConfigApply.path()
        );
        axum::serve(listener, self.router()).await
    }
}

/// 宿主状态（axum `State`）。
#[derive(Clone)]
struct HostState {
    config: ConfigSource,
    apply: ApplySource,
    logs: LogSource,
}

// ═══════════════════════════════════════════════════════════════
// Handler
// ═══════════════════════════════════════════════════════════════

/// `GET /v1/console/config` → **裸 `ConfigView`**（§3.4 补注：GET 不走信封）。
///
/// 失败路径：配置源不可用 ⇒ **503**（非 2xx，渲染端落 `Error::HttpStatus`），
/// **绝不**回 `200` + 空视图冒充成功。
///
/// `revision` / `write_mode` 取自**写服务**（G-2 起不再是常量）：写路径未装配时回落
/// `(0, TextPreserve)`——即 G-1 的语义"本进程尚无成功写入"（那是当时**唯一不臆造**的取值，
/// 现在仍是不臆造的那一个）。
async fn get_config(State(st): State<HostState>) -> Response {
    match &st.config {
        ConfigSource::Ready(cfg) => {
            let guard = cfg.read().await;
            let view = match &st.apply {
                ApplySource::Ready(svc) => svc.view(&guard),
                ApplySource::Unavailable(_) => config_view(&guard, REVISION_INITIAL, WriteMode::TextPreserve),
            };
            Json(view).into_response()
        }
        ConfigSource::Unavailable(reason) => {
            tracing::error!(reason, "控制通道配置源不可用，GET /v1/console/config 回 503");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("config source unavailable: {reason}"),
            )
                .into_response()
        }
    }
}

/// `GET /v1/console/logs` → **裸 `LogPage`**（§3.4 补注：GET 不走信封）。
///
/// 参数解析**不用 `Query<T>` 结构体**：多值维度（`levels` / `targets`）在 §3.4 补注里
/// 定死为**重复键**，只有"键值对序列"这一形态能原样表达（结构体 + `Vec` 会把重复键与逗号
/// 拼接混为一谈）。⇒ 收 `Query<Vec<(String, String)>>` 后交给
/// [`crate::log_service::parse_query`]（纯函数、可单测）。
///
/// 错误通道：参数非法 **400** / 日志源不可读 **503** / 正常（含空页与**超限页**）**200**。
async fn get_logs(
    State(st): State<HostState>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Response {
    let LogSource::Ready(svc) = &st.logs else {
        let reason = match &st.logs {
            LogSource::Unavailable(r) => *r,
            LogSource::Ready(_) => unreachable!(),
        };
        tracing::error!(reason, "日志源未装配，GET /v1/console/logs 回 503");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("log source unavailable: {reason}"),
        )
            .into_response();
    };
    let q = match crate::log_service::parse_query(&pairs) {
        Ok(q) => q,
        Err(e) => {
            // 原因串只进响应体（渲染端不解析错误体 ⇒ 不上屏）与日志；**不进**任何上屏字段。
            tracing::warn!(error = %e, "GET /v1/console/logs 查询参数非法，回 400");
            return (StatusCode::BAD_REQUEST, format!("invalid query: {e}")).into_response();
        }
    };
    match svc.page(&q, now_ms()).await {
        Ok(page) => Json(page).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "日志源不可读，GET /v1/console/logs 回 503");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("log source unreadable: {e}"),
            )
                .into_response()
        }
    }
}

/// `GET /v1/console/logs/targets` → **裸 `Vec<String>`**（模块选项，≤ [`LOG_TARGETS_MAX`]）。
///
/// 无参（§3.4 请求列为「—」）⇒ 本 handler **不接** `Query`（多给参数也不影响语义）；
/// 失败口径与 [`get_logs`] 一致（源不可读 ⇒ 503，**不**回空列表）。
async fn get_logs_targets(State(st): State<HostState>) -> Response {
    let LogSource::Ready(svc) = &st.logs else {
        let reason = match &st.logs {
            LogSource::Unavailable(r) => *r,
            LogSource::Ready(_) => unreachable!(),
        };
        tracing::error!(reason, "日志源未装配，GET /v1/console/logs/targets 回 503");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("log source unavailable: {reason}"),
        )
            .into_response();
    };
    match svc.targets().await {
        Ok(t) => Json(t).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "日志源不可读，GET /v1/console/logs/targets 回 503");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                format!("log source unreadable: {e}"),
            )
                .into_response()
        }
    }
}

/// `POST /v1/console/config/apply` → **`ControlResponse<ConfigView>` 信封**（§3.4 / §3.3 管线）。
///
/// # 为什么不用 `Json<ControlRequest<ConfigPatch>>` 提取器
///
/// axum 的 `Json` 提取失败会回 **422/400 + 它自己的错误体** ⇒ 渲染端落 `HttpStatus`（**丢掉
/// 具体原因**，见模块头"POST 的错误通道"）。故收 [`Bytes`] **自己解**，把"body 不是合法
/// JSON / 不是完整信封"也变成一条**可读的信封回执**（`RejectedValidation` + `message`）。
///
/// # 结局与 HTTP 状态码的对应（只有两种）
///
/// | 结局 | HTTP | body |
/// |------|------|------|
/// | 管线跑完（成功 / 一切业务拒绝 / Busy / 审计不可用） | **200** | `ControlResponse`（`ok` 由 `code` 决定） |
/// | body 不是合法信封（含 JSON 语法错、缺字段、`op` 是别的端点） | **200** | `ControlResponse{code: RejectedValidation, ok: false}` + message |
///
/// **没有第三种**：写路径未装配也回 200 + `Unavailable` 信封（渲染端才有"具体原因"可显示）。
async fn post_config_apply(State(st): State<HostState>, body: Bytes) -> Response {
    let now = now_ms();
    let ApplySource::Ready(svc) = &st.apply else {
        let reason = match &st.apply {
            ApplySource::Unavailable(r) => *r,
            ApplySource::Ready(_) => unreachable!(),
        };
        // **口径统一为 `AuditUnavailable`**（评审建议 6.1）：装配侧**唯一**的 `Unavailable`
        // 成因就是"审计 sink 建不起来"（`startup::console_apply_source` 的 Err 分支）⇒
        // 它**本来就是**审计不可用。修复前这里回 `ControlCode::Unavailable`，而屏上 EDGE-18
        // 的**固定文案**「审计不可用，操作未执行」只绑 `AuditUnavailable`
        // （`p2_config.rs:1854`）⇒ 同一件事在两侧各叫一个名字，屏上落到通用"操作失败"，
        // 现场看到的原因反而比事实**更模糊**。统一后：屏上直接落到既有固定文案，
        // 无需为 `Unavailable` 再加一条文案（二选一，取"少改一端 + 语义更准"的那个）。
        tracing::error!(reason, "控制通道写路径未装配（审计不可用），apply 回 AuditUnavailable 信封");
        // ⚠️ 原因串**不进** `message`（它是外部装配错误串，含 cmap 外的字 ⇒ 真机豆腐块）：
        // 屏上这条按 `code` 走 EDGE-18 固定文案，`message` 只用于日志 / 现场对拍，
        // 故这里给**固定且 cmap 内**的一条；详情在上面的 `tracing::error!` 里。
        // 用字约束见 `receipt` 模块头。
        return Json(ControlResponse::<ConfigView>::rejected(
            "",
            ControlCode::AuditUnavailable,
            receipt::WRITE_PATH_UNAVAILABLE,
            Vec::new(),
            None,
            now,
        ))
        .into_response();
    };

    // 信封解析（管线第 2 步的"能不能解出来"部分；语义校验在 `ConfigService::apply` 里）
    let req: ControlRequest<ConfigPatch> = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            // 解析失败时 `request_id` **不可知** ⇒ 回空串（契约要求回显；回空串比编一个 uuid
            // 诚实：渲染端 `console.rs::parse` 只对**有在途**的请求比对 id，本响应是它自己那次的
            // 结局，不会被误判成"错位回执"）。
            // 同上：`serde_json` 的错误串是**英文 + 含位置偏移**（必然含 cmap 外的字），
            // **不进** `message` ⇒ 详情进 `field_errors[0].reason`（结构化，与其余失败同渠道）
            // + 一条 `warn`（现场排障不丢信息）；屏上只给固定文案。
            tracing::warn!(error = %e, "POST 体不是合法的控制信封（JSON 解析失败）");
            return Json(ControlResponse::<ConfigView>::rejected(
                "",
                ControlCode::RejectedValidation,
                receipt::BAD_ENVELOPE,
                vec![FieldError {
                    field: "request".to_string(),
                    reason: e.to_string(),
                }],
                None,
                now,
            ))
            .into_response();
        }
    };

    Json(svc.apply(&req).await).into_response()
}

/// 已登记但本单元未实现的端点 ⇒ **501 Not Implemented**。
///
/// 「诚实」在这里的含义：不返回空 DTO、不返回 200、也不假装 404（路径确实已登记）。
/// 渲染端会把它归为 `ConsoleError::HttpStatus(501)` = 明确的通道失败（可见），
/// 而不是"查询成功但没数据"（不可见、且会被误读为业务空态）。
async fn not_implemented() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        "console endpoint registered but not implemented yet (G-1 scope: GET /v1/console/config)",
    )
        .into_response()
}

// ═══════════════════════════════════════════════════════════════
// 字段元数据表（设计 §4.3.2 / §4.3.3）
// ═══════════════════════════════════════════════════════════════

/// 分组 ID（设计 §6.2「按 `groups` 渲染分组」）。
pub const GROUP_IEC104: &str = "iec104";
/// 核间分组。
pub const GROUP_INTERCORE: &str = "intercore";
/// 遥测与日志分组。
pub const GROUP_TELEMETRY_LOG: &str = "telemetry_log";
/// 本机地址分组（只读字段；UI §6.2 只读字段行 + §6.6 服务地址口径）。
pub const GROUP_LOCAL_ADDR: &str = "local_addr";

/// 分组表（**有序**：屏上分组顺序即此序；组内字段顺序即 [`FIELDS`] 序）。
///
/// 标签逐字取自 UI 设计文档 §6.2「组与字段」（行 506–514）与设计 §6.2 进入行（行 1230），
/// 且**逐字落在生成字体的 cmap 内**（`local-display/fonts/lv_font_cmap.txt`）——
/// 后端标签是自由文本，屏上是豆腐块的直接来源（渲染端 PD13 明写"后端字段表的 label 必须
/// 约束在 UI §3.6 用字表内，属联调/后端责任"）。
pub const GROUPS: [(&str, &str); 4] = [
    (GROUP_IEC104, "IEC 104 连接参数"),
    (GROUP_INTERCORE, "核间通信参数"),
    (GROUP_TELEMETRY_LOG, "遥测与日志"),
    (GROUP_LOCAL_ADDR, "本机地址"),
];

/// 控制面**回执文案**（`ControlResponse::message`）—— **每一个字符都必须在生成字体的 cmap 内**。
///
/// # 为什么这组文案要"查码表"（而告警 `message` 不查）
///
/// `local-display` 的既有口径是「**自由文本**（告警 `message`、型号 / 序列号 / 管理 IP）不处理」
/// —— 那些串**不是 mupcd 生成的**，后端无从约束。本组的性质**不同**：它们**逐字由我们自己拼**，
/// 完全可控，而且渲染端会**原样上屏**（成功路径连 `display_safe` 都不过，见
/// `p2_config.rs::success_toast_text`）⇒ 一个 cmap 外的字就是真机上的一个**豆腐块**，属
/// "自己造的缺陷"。故本组取**硬约束**：只用 cmap 内字符，并由
/// [`config_receipt_messages_use_only_font_cmap_glyphs`] 逐字符兜底。
///
/// **码表真源** = `local-display/fonts/lv_font_cmap.txt`（入库派生清单，324 码位）——
/// **不在此处复制一份副本**（那会变成第二真源，清单漂移时两边静默不一致）。
///
/// # 三条推导出的写法规则（与 `config_service` 模块头硬口径 4 同一条）
///
/// a. 全角 `（ ）` `；` `：` `，` 与**半角逗号**都不在 cmap 内 ⇒ 分隔符**只**能用 `·`(U+00B7)，
///    冒号用**半角** `:`(U+003A)。
/// b. 机器键名（`gateway.listen_port`）**必然**含缺字形字符（小写 `t` / `_` 都不在）⇒
///    屏上点名一律用 [`ConfigFieldMeta::label`]，见 `config_service::restart_labels`。
/// c. 外部错误串（`serde_yaml` / `std::io` / 契约 `ControlEnvelopeError` 的**全小写英文**）
///    一律**不进** `message` ⇒ 详情走审计 `reason` + `tracing`。
pub(crate) mod receipt {
    /// 成功保存（**热生效**路径：无「需重启」子句）。
    pub(crate) const SAVED: &str = "配置已保存";
    /// 「需重启」子句（**含前导分隔符**；后接 `restart_labels` 拼出的字段标签）。
    pub(crate) const RESTART_PREFIX: &str = " · 需重启进程生效: ";
    /// 本次改动与原值相同 ⇒ 未写盘（不是失败）。
    pub(crate) const NO_CHANGE: &str = "无字段变化 · 未保存";
    /// 逐字段校验失败 ⇒ **一条都不执行**（EDGE-10 不得半生效）。
    pub(crate) const VALIDATION_FAILED: &str = "配置未保存 · 字段取值无效";
    /// 同一 `request_id` 仍在处理中（幂等占位未释放）。
    pub(crate) const BUSY: &str = "正在执行 · 未重复下发 · 请重试";
    /// 信封非法（空 `request_id` / `op` 误路由 / 超出 ±30 s 重放窗）⇒ 原因串**不进 message**。
    pub(crate) const BAD_ENVELOPE: &str = "控制报文无效";
    /// 保留式编辑不可定位 ⇒ 回退整体保存（**原文注释与未建模键已丢失**，EDGE-23）。
    ///
    /// 取「原有文字已不存在」而非"已整体重写/保存"：`整`(U+6574) / `体`(U+4F53) / `写`(U+5199)
    /// **三个字都不在 cmap 内**（本条是网 `config_receipt_messages_use_only_font_cmap_glyphs`
    /// 逐字符抓出来的），而这半句是渲染端 `p2_config::TEXT_TOAST_FULL_REWRITE`
    /// （`"配置已保存 · 原有文字已不存在"`）**已经在屏上说的话** ⇒ 借它既合法又一致。
    pub(crate) const FULL_REWRITE_UNLOCATABLE: &str = "原有文字已不存在 · 按行定位失败";
    /// 真源文件读不出来 ⇒ 无"原文本"可保真，只能整体保存。
    pub(crate) const FULL_REWRITE_UNREADABLE: &str = "原有文字已不存在 · 配置读取失败";
    /// 执行期失败：字段取值落进 `CoreConfig` 时异常（校验已过 ⇒ 理论上不可达）。
    ///
    /// ⚠️ 不写"写入"：`写`(U+5199) **不在 cmap 内**（网抓出来的第二个缺字，第一个是 `整`)。
    pub(crate) const WRITE_FAILED_FIELD: &str = "配置保存失败 · 字段更新异常";
    /// 执行期失败：文本生成（序列化）异常。
    pub(crate) const WRITE_FAILED_SERIALIZE: &str = "配置保存失败 · 文本生成异常";
    /// 执行期失败：**写后自检**不过 ⇒ 未落盘、文件未改动。
    pub(crate) const WRITE_FAILED_SELFCHECK: &str = "配置保存失败 · 文件未改动";
    /// 执行期失败：原子落盘（tmp → fsync → bak → rename）失败 ⇒ 原文件保持旧内容。
    pub(crate) const WRITE_FAILED_FILE: &str = "配置保存失败 · 文件保存异常";
    /// 写路径未装配（成因唯一 = 审计 sink 建不起来 ⇒ `code` 恒为 `AuditUnavailable`）。
    ///
    /// ⚠️ 本串**不进屏**：渲染端对 `AuditUnavailable` 按 `code` 覆盖成 EDGE-18 固定文案
    /// （`state.rs::toast_text` 与 `p2_config::show_result` 两处都是）。留 cmap 内的用字是为了
    /// 让"回执文案"这条**没有例外**（例外会在下一次改动时被忘掉）。
    pub(crate) const WRITE_PATH_UNAVAILABLE: &str = "配置保存不可用";
    /// 审计不可写（`AuditUnavailable` 信封）。同 [`WRITE_PATH_UNAVAILABLE`]：**不进屏**，
    /// 屏上取渲染端 EDGE-18 固定串（两串**同义不同源**，由 `code` 决定取哪条 ⇒ 漂移无上屏后果）。
    pub(crate) const AUDIT_UNAVAILABLE: &str = "审计不可用 · 操作未执行";
    /// 未知字段（字段表查不到 key 时的兜底标签；`restart_labels` 的输入来自 [`FIELDS`] ⇒ 不可达）。
    pub(crate) const UNKNOWN_FIELD: &str = "未知字段";

    /// **全部**回执文案（网的构造性半边：这条清单里每一个字符都必须 ⊆ cmap）。
    ///
    /// 新增文案时**必须**加进来 —— 漏加不会被 `receipt` 的其它用例发现（这正是本清单存在的理由）。
    #[allow(dead_code)] // 只被 `console_host` 的回执用字网消费（与同文件其它"测试用的尺子"同款）
    pub(crate) const ALL: &[&str] = &[
        SAVED,
        RESTART_PREFIX,
        NO_CHANGE,
        VALIDATION_FAILED,
        BUSY,
        BAD_ENVELOPE,
        FULL_REWRITE_UNLOCATABLE,
        FULL_REWRITE_UNREADABLE,
        WRITE_FAILED_FIELD,
        WRITE_FAILED_SERIALIZE,
        WRITE_FAILED_SELFCHECK,
        WRITE_FAILED_FILE,
        WRITE_PATH_UNAVAILABLE,
        AUDIT_UNAVAILABLE,
        UNKNOWN_FIELD,
    ];
}

/// 一行字段元数据（设计 §4.3.2 的 `ConfigFieldMeta`）。
///
/// `yaml_path` 本单元**不消费**（只读不落盘），但**现在就钉死**：§4.3.2.1 的保留式编辑以它为
/// 定位依据，G-2 若另起一套路径串就等于两个真源。单测 `yaml_path_matches_key` 保证它与 `key` 同形。
pub struct ConfigFieldMeta {
    /// 所属分组 ID。
    pub group: &'static str,
    /// 稳定键（= 契约 `ConfigField.key`；也是审计 `target`）。
    pub key: &'static str,
    /// 中文标签（**上屏**；须在字体 cmap 内）。
    pub label: &'static str,
    /// 控件类型（`fn` 而非值：`ConfigKind::Enum` 含 `Vec`，表要能放进 `static`）。
    pub kind: fn() -> ConfigKind,
    /// 单位（`None` = 无单位）。
    pub unit: Option<&'static str>,
    /// **`true` → 弹层须提示「生效时链路将短暂中断」**（真源 = 设计 §4.3.3，见每个字段的行号注释）。
    pub requires_reconnect: bool,
    /// `false` → 屏上只读（回环安全红线 PL-4）。
    pub editable: bool,
    /// yaml 定位路径（§4.3.2.1）。G-2 的保留式编辑**直接取用**——**不得**另起一套路径串。
    pub yaml_path: &'static str,
    /// 默认值（「恢复默认值」明细取此处）。
    pub default: fn() -> Value,
    /// 当前值提取（从 `CoreConfig` **内存副本**取；不读 yaml）。
    pub current: fn(&CoreConfig) -> Value,
    /// **写侧**（G-2）：把校验过的值写进 `CoreConfig` 的副本。
    ///
    /// 与 `current` 同表 ⇒ "屏上能读的键"、"屏上能改的键"、"装置里能写的键"**只有一份清单**。
    /// 只读字段（`editable=false`）的实现**恒 Err**（第二道防线：即便调用方漏了 `editable`
    /// 判定，也写不进去）。
    pub set: fn(&mut CoreConfig, &Value) -> Result<(), String>,
}

impl ConfigFieldMeta {
    /// 生成契约字段（值取当前内存副本）。
    pub fn to_field(&self, cfg: &CoreConfig) -> ConfigField {
        ConfigField {
            key: self.key.to_string(),
            label: self.label.to_string(),
            kind: (self.kind)(),
            value: (self.current)(cfg),
            default: (self.default)(),
            unit: self.unit.map(str::to_string),
            requires_reconnect: self.requires_reconnect,
            editable: self.editable,
        }
    }
}

/// 按稳定键取字段元数据（`None` = 该键不在本机字段表内 ⇒ 写路径**必须拒**）。
pub fn field_meta(key: &str) -> Option<&'static ConfigFieldMeta> {
    FIELDS.iter().find(|m| m.key == key)
}

/// 把校验过的值写进配置副本（写路径的唯一入口）。
///
/// 只读字段拒绝：这是**第二道**防线（第一道是 [`ConfigFieldMeta::editable`] 在管线第 4 步的
/// 判定）。两道都要在：前者是"UI 控件 disabled"的后端对称面，后者防"将来新增调用路径忘了判"。
pub fn set_field(cfg: &mut CoreConfig, key: &str, value: &Value) -> Result<(), String> {
    let meta = field_meta(key).ok_or_else(|| format!("未知字段 `{key}`"))?;
    if !meta.editable {
        return Err(format!("字段 `{key}` 为只读，不可修改"));
    }
    (meta.set)(cfg, value)
}

// ── 写侧实现（逐字段；只读字段恒 Err）────────────────────────────────────────────

/// 写只读字段（恒 Err 的占位实现；`display.*` 用）。
fn set_read_only(_c: &mut CoreConfig, _v: &Value) -> Result<(), String> {
    Err("只读字段（回环安全红线 PL-4）不可修改".to_string())
}

fn set_gateway_listen_addr(c: &mut CoreConfig, v: &Value) -> Result<(), String> {
    c.gateway.listen_addr = v.as_str().ok_or("应为字符串")?.to_string();
    Ok(())
}

fn set_gateway_listen_port(c: &mut CoreConfig, v: &Value) -> Result<(), String> {
    let n = v.as_u64().ok_or("应为非负整数")?;
    c.gateway.listen_port = u16::try_from(n).map_err(|_| format!("{n} 超出端口范围"))?;
    Ok(())
}

fn set_intercore_host(c: &mut CoreConfig, v: &Value) -> Result<(), String> {
    c.intercore.host = v.as_str().ok_or("应为字符串")?.to_string();
    Ok(())
}

fn set_intercore_port(c: &mut CoreConfig, v: &Value) -> Result<(), String> {
    let n = v.as_u64().ok_or("应为非负整数")?;
    c.intercore.port = u16::try_from(n).map_err(|_| format!("{n} 超出端口范围"))?;
    Ok(())
}

fn set_intercore_heartbeat(c: &mut CoreConfig, v: &Value) -> Result<(), String> {
    c.intercore.heartbeat_interval_sec = v.as_u64().ok_or("应为非负整数")?;
    Ok(())
}

fn set_intercore_reconnect(c: &mut CoreConfig, v: &Value) -> Result<(), String> {
    c.intercore.reconnect_interval_sec = v.as_u64().ok_or("应为非负整数")?;
    Ok(())
}

fn set_log_level(c: &mut CoreConfig, v: &Value) -> Result<(), String> {
    c.system.log_level = v.as_str().ok_or("应为字符串")?.to_string();
    Ok(())
}

/// 字段表。**唯一真源**：设计 §4.3.3（行 783–791）+ UI §6.2（行 506–514）。
///
/// 每行末尾注释给出 `requires_reconnect` 的**设计依据行号**——不是"看起来像"，是逐行对账。
pub const FIELDS: &[ConfigFieldMeta] = &[
    // ── IEC 104 连接参数（UI §6.2 行 508–510）──────────────────────────────
    // 设计 §4.3.3 行 790：「IEC 104 监听地址/端口 … 调度通道瞬断（requires_reconnect=true，高风险须明示）」
    ConfigFieldMeta {
        group: GROUP_IEC104,
        key: "gateway.listen_addr",
        label: "本机监听地址 · IEC 104",
        kind: || ConfigKind::Ipv4,
        unit: None,
        requires_reconnect: true,
        editable: true,
        yaml_path: "gateway.listen_addr",
        default: || json!(def().gateway.listen_addr),
        current: |c| json!(c.gateway.listen_addr),
        set: set_gateway_listen_addr,
    },
    ConfigFieldMeta {
        group: GROUP_IEC104,
        key: "gateway.listen_port",
        label: "端口",
        kind: || ConfigKind::U16 {
            min: 1,
            max: 65535,
            step: 1,
        },
        unit: None,
        requires_reconnect: true,
        editable: true,
        yaml_path: "gateway.listen_port",
        default: || json!(def().gateway.listen_port),
        current: |c| json!(c.gateway.listen_port),
        set: set_gateway_listen_port,
    },
    // ── 核间通信参数（UI §6.2 行 511–512）──────────────────────────────────
    // 设计 §4.3.3 行 787：「核间本地端口 / 对端端口 intercore.port / intercore.host …
    // 链路瞬断（须 requires_reconnect=true，弹层明示）」
    ConfigFieldMeta {
        group: GROUP_INTERCORE,
        key: "intercore.host",
        label: "对端地址",
        kind: || ConfigKind::Ipv4,
        unit: None,
        requires_reconnect: true,
        editable: true,
        yaml_path: "intercore.host",
        default: || json!(def().intercore.host),
        current: |c| json!(c.intercore.host),
        set: set_intercore_host,
    },
    ConfigFieldMeta {
        group: GROUP_INTERCORE,
        key: "intercore.port",
        label: "对端端口",
        kind: || ConfigKind::U16 {
            min: 1,
            max: 65535,
            step: 1,
        },
        unit: None,
        requires_reconnect: true,
        editable: true,
        yaml_path: "intercore.port",
        default: || json!(def().intercore.port),
        current: |c| json!(c.intercore.port),
        set: set_intercore_port,
    },
    // 设计 §4.3.3 行 788：「核间心跳/重连间隔 … 时效=下一拍，副作用=无」⇒ **不**要求重连提示
    ConfigFieldMeta {
        group: GROUP_INTERCORE,
        key: "intercore.heartbeat_interval_sec",
        label: "心跳间隔",
        kind: || ConfigKind::U64 {
            min: 1,
            max: 3600,
            step: 1,
        },
        unit: Some("秒"),
        requires_reconnect: false,
        editable: true,
        yaml_path: "intercore.heartbeat_interval_sec",
        default: || json!(def().intercore.heartbeat_interval_sec),
        current: |c| json!(c.intercore.heartbeat_interval_sec),
        set: set_intercore_heartbeat,
    },
    ConfigFieldMeta {
        group: GROUP_INTERCORE,
        key: "intercore.reconnect_interval_sec",
        label: "重连间隔",
        kind: || ConfigKind::U64 {
            min: 1,
            max: 3600,
            step: 1,
        },
        unit: Some("秒"),
        requires_reconnect: false,
        editable: true,
        yaml_path: "intercore.reconnect_interval_sec",
        default: || json!(def().intercore.reconnect_interval_sec),
        current: |c| json!(c.intercore.reconnect_interval_sec),
        set: set_intercore_reconnect,
    },
    // ── 遥测与日志（UI §6.2 行 513–514）────────────────────────────────────
    // 设计 §4.3.3 行 785：「日志级别 system.log_level … 时效 ≤1 s，副作用 无」⇒ 不要求重连提示
    ConfigFieldMeta {
        group: GROUP_TELEMETRY_LOG,
        key: "system.log_level",
        label: "日志级别",
        kind: || ConfigKind::Enum {
            options: vec![
                OptionItem {
                    value: "error".to_string(),
                    label: "ERROR".to_string(),
                },
                OptionItem {
                    value: "warn".to_string(),
                    label: "WARN".to_string(),
                },
                OptionItem {
                    value: "info".to_string(),
                    label: "INFO".to_string(),
                },
                OptionItem {
                    value: "debug".to_string(),
                    label: "DEBUG".to_string(),
                },
            ],
        },
        unit: None,
        requires_reconnect: false,
        editable: true,
        yaml_path: "system.log_level",
        default: || json!(def().system.log_level),
        current: |c| json!(c.system.log_level),
        set: set_log_level,
    },
    // ── 本机地址（**只读**；设计 §3.4 行 624–626 / §6.2 行 1232 / §4.9）──────
    //
    // 回环是 PL-4 安全红线 ⇒ `editable=false`，且**不参与** L2+ 分级（不可改 ⇒ 永不进「本次改动」；
    // 设计 §4.3.3 未列它们，故 `requires_reconnect=false` 与其"改了才要重连"的物理语义一致）。
    ConfigFieldMeta {
        group: GROUP_LOCAL_ADDR,
        key: "display.bind_addr",
        label: "本机地址 · 仅本机",
        kind: || ConfigKind::Ipv4,
        unit: None,
        requires_reconnect: false,
        editable: false,
        yaml_path: "display.bind_addr",
        default: || json!(loopback_host_of(&def().display.bind_addr)),
        current: |c| json!(loopback_host_of(&c.display.bind_addr)),
        set: set_read_only,
    },
    ConfigFieldMeta {
        group: GROUP_LOCAL_ADDR,
        key: "display.control_bind_addr",
        label: "本机地址 · 仅本机",
        kind: || ConfigKind::Ipv4,
        unit: None,
        requires_reconnect: false,
        editable: false,
        yaml_path: "display.control_bind_addr",
        default: || json!(loopback_host_of(&def().display.control_bind_addr)),
        current: |c| json!(loopback_host_of(&c.display.control_bind_addr)),
        set: set_read_only,
    },
];

/// 设计 §4.3.3 列了、但**现网 `CoreConfig` 无承载字段**的三行（本单元**不**放进视图）。
///
/// 逐条给出"为什么不能进"：
/// 1. `gateway.heartbeat_interval`（设计行 789「IEC 104 心跳间隔 | `gateway.*`（**新增字段**）」）——
///    设计自己标注为**新增**；`GatewayConfig`（`core_config.rs:281-288`）只有
///    `listen_addr` / `listen_port`，**没有**心跳间隔。造一个键 = 屏上能改、装置里不存在。
/// 2. `intercore.local_port`（设计行 787「核间**本地端口**」）——`InterCoreConfig`
///    （`core_config.rs:152-171`）只有 `host` / `port`（**对端**地址与端口）/ `heartbeat_interval_sec`
///    / `reconnect_interval_sec` / `transport` / `modbus_rtu`，**没有**本地绑定端口。
/// 3. `telemetry.report_interval_sec`（设计行 786「遥测上报周期 | 上送任务节拍（`startup.rs` 上送路径）」）——
///    上送节拍在 `startup.rs` 是**硬编码常量**，`CoreConfig` 无对应项（行 786 的"现网真实 key"一栏
///    写的是一句**代码位置描述**，不是配置键——这本身就是该项无配置承载的证据）。
///
/// 三者都是**写路径 + 配置结构**的净新增（属 G-2 或 PM 裁定），不是读路径能"补"出来的。
#[allow(dead_code)] // 清单本身是**记录**（读路径不消费），由单测 `metadata_table_has_no_duplicate_or_pending_keys` 钉死
pub const PENDING_NO_CARRIER: [&str; 3] = [
    "gateway.heartbeat_interval",
    "intercore.local_port",
    "telemetry.report_interval_sec",
];

/// `ConfigView.revision` 的初值：**本进程尚无任何成功写入**。
///
/// 契约语义是「每次成功写入递增」（`display-proto/src/control.rs:412`）——没有任何写入时取 0
/// 是其**唯一不臆造**的取值。G-2 起由 [`crate::config_service::ConfigService::revision`]
/// 提供真值；本常量只剩"写路径未装配"时的取值（语义仍是"没有过写入"，不臆造）。
pub const REVISION_INITIAL: u64 = 0;

/// 由 `CoreConfig` **内存副本**生成视图（设计 §4.3.2：内存副本是"进程内唯一权威读源"）。
///
/// `revision` / `write_mode` 由**调用方注入真值**（G-2 起 = `ConfigService` 的状态）——
/// 不再是常量：契约语义分别是「每次成功写入递增」与「**最近一次落盘**的写模式」
/// （`display-proto/src/control.rs:405-413`）。写路径未装配时传
/// `(REVISION_INITIAL, TextPreserve)`（= "本进程尚无写入"，当时与现在的**唯一不臆造**取值）。
///
/// 契约粗糙处（登记）：`WriteMode` 不是 `Option`，无法表达"还没有过写入"。
pub fn config_view(cfg: &CoreConfig, revision: u64, write_mode: WriteMode) -> ConfigView {
    let groups = GROUPS
        .iter()
        .map(|(id, label)| ConfigGroup {
            id: (*id).to_string(),
            label: (*label).to_string(),
            fields: FIELDS
                .iter()
                .filter(|m| m.group == *id)
                .map(|m| m.to_field(cfg))
                .collect(),
        })
        .collect();
    ConfigView {
        groups,
        revision,
        write_mode,
    }
}

// ═══════════════════════════════════════════════════════════════
// 默认值真源
// ═══════════════════════════════════════════════════════════════

/// 最小可解析 yaml：**只给 `CoreConfig` 里没有 `#[serde(default)]` 的顶层段**，
/// 其余段留空表 `{}` ⇒ 各字段落到 `core_config.rs` 的 `default_*()` 函数值。
///
/// **为什么不把默认值硬抄一遍**：抄一遍就有两个真源，`core_config.rs` 改默认值而这里不动
/// ⇒ 屏上「恢复默认值」明细会显示一个装置**不会**变成的"默认值"（静默失实）。
/// 走 serde ⇒ 默认值的唯一真源就是 `core_config.rs` 自己。
///
/// `version` 写一个非空占位：`version` 是 `CoreConfig` 里**唯一**无 serde 默认的字符串字段，
/// 而 `CoreConfig::validate()` 要求它非空。它**不是** UI 字段（不进 `ConfigView`），
/// 故占位值不上屏；取 `"0.1.0"` 仅为让 `def().validate()` 这一自洽断言可用。
///
/// `web_api.tls_cert` / `tls_key` 显式写 `null`（`Option` 字段，写 null 与缺省同为 `None`，二者皆可）。
const MINIMAL_CORE_YAML: &str = r#"
version: "0.1.0"
system: {}
intercore: {}
web_api:
  tls_cert: null
  tls_key: null
ai_engine: {}
plugins: {}
"#;

/// 缺省 `CoreConfig`（仅供 `default` 提取；**不是**运行时配置）。
///
/// 解析失败即 panic：内嵌字面量一旦与 `CoreConfig` 结构脱节（新增必填字段），
/// 必须**启动期可见地**失败，而不是让"默认值"静默变成 `null`
/// （单测 `minimal_core_yaml_parses_and_yields_core_config_defaults` 同时钉死它）。
///
/// **解析结果进程内缓存一次**：`FIELDS` 有 9 行、每行 `default` 闭包各调一次 ⇒ 每个
/// `GET /v1/console/config` 请求原本要跑 9 次 `serde_yaml::from_str`（有界但纯浪费）。
/// `OnceLock` 让解析**恰好发生一次**，之后每请求只做一次字段读取（返回 `&'static`，零拷贝）。
fn def() -> &'static CoreConfig {
    static DEF: std::sync::OnceLock<CoreConfig> = std::sync::OnceLock::new();
    DEF.get_or_init(|| {
        serde_yaml::from_str(MINIMAL_CORE_YAML).expect("内嵌最小 yaml 必须可解析为 CoreConfig")
    })
}

/// 取 `host:port` 的 host 部分（`"127.0.0.1:9810"` → `"127.0.0.1"`）。
///
/// **契约缺口（本单元如实处置，未改契约）**：`ConfigKind` 只有 `Ipv4` / `U16` / `U64` / `Enum`
/// 四态（`display-proto/src/control.rs:477-505`）——**没有**"host:port"这一档，而
/// `display.bind_addr` / `display.control_bind_addr` 的 yaml 值是 `"127.0.0.1:9810"` 这种形态，
/// 直接用 `Ipv4` 承载会**校验失败**（`"127.0.0.1:9810".parse::<Ipv4Addr>()` 必 Err）。
/// 本单元取 **host 部分**作为字段值（回环地址的 host 部分即 PL-4 红线的实质内容，
/// 且 §6.2 只读字段行展示的正是「本机服务地址」），`editable=false` ⇒ **不存在**"改半个地址"
/// 的写风险。**残余**：若现场 yaml 写 IPv6 回环（`"[::1]:9811"`），host 段为 `::1`，非合法
/// `Ipv4` ⇒ 渲染端走 PD21(i)「注入值非法」路径（该行显红 + 控件 disabled，**可见**降级，非静默）。
/// 根治需 PM 裁定：拆成 `host` + `port` 两个字段，或给契约加"端点"kind（**不得**由本单元擅改契约）。
pub fn loopback_host_of(addr: &str) -> String {
    let t = addr.trim();
    // 快路径：std 能解析就与 std 同语义（含 `[::1]:9811`）
    if let Ok(sa) = t.parse::<std::net::SocketAddr>() {
        return sa.ip().to_string();
    }
    let stripped = t.trim_start_matches('[').trim_end_matches(']');
    match stripped.rsplit_once(':') {
        // 仅当"冒号前不再有冒号（⇒ 不是裸 IPv6）"且"右段确为数字端口"时才剥离；
        // 否则 `"::1"` 会被截成 `":"`（裸 IPv6 必须原样返回，交给渲染端走 PD21(i) 可见降级）。
        Some((h, p))
            if !h.is_empty()
                && !h.contains(':')
                && !p.is_empty()
                && p.chars().all(|c| c.is_ascii_digit()) =>
        {
            h.to_string()
        }
        _ => stripped.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mupc_display_proto::{LogLevel, LogPage}; // 契约 DTO（断言 / 解码用）
    use std::net::SocketAddr;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // ── 测试脚手架 ──────────────────────────────────────────────────────────

    /// 起一个绑定在随机回环端口上的宿主，返回 (地址, JoinHandle)。
    ///
    /// **写路径 `Unavailable`**（G-1 的读路径用例不需要写服务）。写路径用例走
    /// [`spawn_host_with_apply`]。
    async fn spawn_host(config: ConfigSource) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        spawn_host_with_apply(config, ApplySource::Unavailable("测试未装配写路径")).await
    }

    /// 同上，但注入写路径（G-2 用例）。
    async fn spawn_host_with_apply(
        config: ConfigSource,
        apply: ApplySource,
    ) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        spawn_host_full(config, apply, LogSource::Unavailable("本用例不验日志源")).await
    }

    /// 同上，但注入日志源（单元 H 用例）。
    async fn spawn_log_host(
        logs: LogSource,
    ) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        spawn_host_full(
            ConfigSource::Ready(Arc::new(RwLock::new(test_config()))),
            ApplySource::Unavailable("本用例不验写路径"),
            logs,
        )
        .await
    }

    /// 全量注入版（三个源都在参数里 ⇒ 用例显式声明它验哪一条通道）。
    async fn spawn_host_full(
        config: ConfigSource,
        apply: ApplySource,
        logs: LogSource,
    ) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let host = ConsoleHost::new(ConsoleDeps { config, apply, logs });
        let h = tokio::spawn(async move {
            let _ = host.serve(listener).await;
        });
        (addr, h)
    }

    /// 渲染端同款线协议：`GET/POST <path> HTTP/1.1` + `Connection: close`，读全响应。
    ///
    /// 不用 HTTP 客户端库：要验的正是**线格式**（状态行 / `Content-Length` / 裸 DTO），
    /// 与 `local-display/src/console.rs::head_bytes` 的发送口径一致。
    async fn http(addr: SocketAddr, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAccept: application/json\r\nConnection: close\r\n"
        );
        if let Some(b) = body {
            req.push_str("Content-Type: application/json\r\n");
            req.push_str(&format!("Content-Length: {}\r\n", b.len()));
        }
        req.push_str("\r\n");
        if let Some(b) = body {
            req.push_str(b);
        }
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut raw))
            .await
            .expect("读响应超时")
            .unwrap();
        let text = String::from_utf8_lossy(&raw).to_string();
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        let body = text
            .split_once("\r\n\r\n")
            .map(|(_, b)| b.to_string())
            .unwrap_or_default();
        (status, body)
    }

    /// 测试用配置：在缺省值上改几个可辨认的值（防止"默认值恰好等于期望值"的假通过）。
    fn test_config() -> CoreConfig {
        let mut c = def().clone();
        c.system.log_level = "debug".to_string();
        c.intercore.host = "10.0.0.7".to_string();
        c.intercore.port = 9999;
        c.intercore.heartbeat_interval_sec = 7;
        c.intercore.reconnect_interval_sec = 11;
        c.gateway.listen_addr = "192.168.3.10".to_string();
        c.gateway.listen_port = 2405;
        c.display.bind_addr = "127.0.0.1:9810".to_string();
        c.display.control_bind_addr = "127.0.0.1:9811".to_string();
        c
    }

    async fn view_from(addr: SocketAddr) -> ConfigView {
        let (status, body) = http(addr, "GET", ConsoleEndpoint::Config.path(), None).await;
        assert_eq!(status, 200, "GET /config 应 200，实际 {status}: {body}");
        serde_json::from_str::<ConfigView>(&body)
            .unwrap_or_else(|e| panic!("裸 ConfigView 解码失败（渲染端同款路径）: {e}\n{body}"))
    }

    fn field<'a>(v: &'a ConfigView, key: &str) -> &'a ConfigField {
        v.groups
            .iter()
            .flat_map(|g| g.fields.iter())
            .find(|f| f.key == key)
            .unwrap_or_else(|| panic!("视图缺字段 `{key}`"))
    }

    // ── ① GET /config 端到端（渲染端同款裸 JSON 解码路径）──────────────────

    #[tokio::test]
    async fn get_config_end_to_end_exposes_groups_fields_and_live_values() {
        let cfg = test_config();
        let (addr, h) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(cfg.clone())))).await;
        let view = view_from(addr).await;
        h.abort();

        // 分组：4 组、顺序与标签与 UI §6.2 一致，且每组非空（空组会在屏上留白卡）
        let ids: Vec<&str> = view.groups.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![GROUP_IEC104, GROUP_INTERCORE, GROUP_TELEMETRY_LOG, GROUP_LOCAL_ADDR]
        );
        assert_eq!(view.groups[0].label, "IEC 104 连接参数");
        assert_eq!(view.groups[1].label, "核间通信参数");
        assert_eq!(view.groups[2].label, "遥测与日志");
        for g in &view.groups {
            assert!(!g.fields.is_empty(), "分组 `{}` 不得为空", g.id);
        }

        // 字段数 = 元数据表长度（不多不少）
        let total: usize = view.groups.iter().map(|g| g.fields.len()).sum();
        assert_eq!(total, FIELDS.len(), "字段数须等于元数据表");

        // **具体字段的 key 与当前值**必须等于内存副本里的真值（不是默认值、不是 yaml 静态串）
        assert_eq!(field(&view, "system.log_level").value, json!("debug"));
        assert_eq!(field(&view, "intercore.host").value, json!("10.0.0.7"));
        assert_eq!(field(&view, "intercore.port").value, json!(9999));
        assert_eq!(field(&view, "intercore.heartbeat_interval_sec").value, json!(7));
        assert_eq!(field(&view, "intercore.reconnect_interval_sec").value, json!(11));
        assert_eq!(field(&view, "gateway.listen_addr").value, json!("192.168.3.10"));
        assert_eq!(field(&view, "gateway.listen_port").value, json!(2405));
        // 只读字段取 **host 部分**（yaml 真值是 host:port，见 `loopback_host_of` 的契约缺口说明）
        assert_eq!(field(&view, "display.bind_addr").value, json!("127.0.0.1"));
        assert_eq!(field(&view, "display.control_bind_addr").value, json!("127.0.0.1"));

        // 视图自身：本进程尚无写入 ⇒ revision=0、write_mode=正常路径
        assert_eq!(view.revision, REVISION_INITIAL);
        assert_eq!(view.write_mode, WriteMode::TextPreserve);

        // 「值的真源是内存副本」（改副本 ⇒ 视图跟着变）**不在本用例**，而在下一用例
        // `get_config_reflects_memory_copy_not_yaml_file`——本用例只钉"端到端 + 字段/值"，
        // 那条断言需要**同一宿主跨两次请求**，故独立成例（勿在此处再写一遍）。
    }

    #[tokio::test]
    async fn get_config_reflects_memory_copy_not_yaml_file() {
        let shared = Arc::new(RwLock::new(test_config()));
        let (addr, h) = spawn_host(ConfigSource::Ready(shared.clone())).await;
        assert_eq!(field(&view_from(addr).await, "intercore.port").value, json!(9999));
        // 就地改内存副本（模拟 G-2 写入后的内存生效）⇒ 视图随之变化
        shared.write().await.intercore.port = 9101;
        assert_eq!(
            field(&view_from(addr).await, "intercore.port").value,
            json!(9101),
            "视图必须由内存副本生成（设计 D10），不得缓存首次结果"
        );
        h.abort();
    }

    // ── ④ **装配级「唯一真源」网** ───────────────────────────────────────────

    /// 评审 **重要-6** 的网：控制通道与装配点必须共用**同一个** `Arc<RwLock<CoreConfig>>`，
    /// **不是**"另行深拷贝出的第二份副本"。
    ///
    /// **为什么非有这条网**：`CoreConfig: Clone`。把装配点改成
    /// `Arc::new(RwLock::new(core_config.read().await.clone()))`（**深拷贝成新 Arc**）后，
    /// 类型系统看不出来、编译通过、其余用例全绿——而"装配用 A、控制通道读 B"的漂移**静默生效**：
    /// 屏上读到的是永不更新的旧快照，G-2 的写入落地后**无声失效**。
    ///
    /// 本用例走**真实装配函数** `startup::console_config_source`（生产 `initialize_all` 用的就是它）
    /// + 宿主的测试可见句柄 `ConsoleHost::config_source`，用 `Arc::ptr_eq` 判**地址级同一性**，
    /// 并给出语义后果（改装配副本 ⇒ 经宿主句柄可见；深拷贝副本 ⇒ **不**可见）。
    #[tokio::test]
    async fn console_config_source_is_arc_identical_to_assembly_handle() {
        // `assembly` 模拟 `main.rs:125` 的权威内存副本（装配读它、G-2 也写它）
        let assembly = Arc::new(RwLock::new(test_config()));
        let host = ConsoleHost::new(ConsoleDeps {
            config: crate::startup::console_config_source(&assembly),
            apply: ApplySource::Unavailable("本用例只验读源同一性"),
            logs: LogSource::Unavailable("本用例只验读源同一性"),
        });
        let got = match host.config_source() {
            ConfigSource::Ready(a) => a.clone(),
            ConfigSource::Unavailable(r) => panic!("装配完成后不得为 Unavailable（{r}）"),
        };
        assert!(
            Arc::ptr_eq(&assembly, &got),
            "控制通道必须与装配共用同一 Arc（唯一真源）；当前是两份副本 ⇒ G-2 写入会静默失效"
        );

        // 语义后果（正向）：改装配副本 ⇒ 经宿主句柄可见
        assembly.write().await.intercore.port = 9101;
        assert_eq!(
            got.read().await.intercore.port,
            9101,
            "装配点改动必须经控制通道可见（同一份内存副本）"
        );

        // 语义后果（负向 = 评审实测的漂移形态）：深拷贝成新 Arc ⇒ 不共享 ⇒ 上面那条断言会红
        let drifted = Arc::new(RwLock::new(assembly.read().await.clone()));
        assert!(
            !Arc::ptr_eq(&assembly, &drifted),
            "负对照失效：ptr_eq 若对「不同分配」也判等，本网就没有鉴别力"
        );
        assert_eq!(
            drifted.read().await.intercore.port,
            9101,
            "负对照前提：漂移副本是快照"
        );
        assembly.write().await.intercore.port = 9102;
        assert_eq!(
            drifted.read().await.intercore.port,
            9101,
            "负对照：漂移副本**看不到**装配点的新值——这正是「有两份真源」的症状"
        );
    }

    // ── ② 诚实性网 ─────────────────────────────────────────────────────────

    /// ① **写路径未装配**的 `config/apply` ⇒ **HTTP 200 + `AuditUnavailable` 信封**
    /// （G-2 起该端点**已实现**，故不再是 501；但**也绝不能假成功**）。
    ///
    /// 这条用例取代 G-1 的 `unimplemented_write_endpoint_returns_non_2xx_not_fake_success`
    /// （那条断言的 501 是"G-1 阶段未实现"的事实，G-2 落地后**该事实已改变**）——
    /// **保留其精神**：未装配也**不得**回 `ok=true`、不得回空 `ConfigView` 冒充处理结果。
    ///
    /// ⚠️ **口径变更（评审建议 6.1，断言由 `Unavailable` 改为 `AuditUnavailable`，是收紧/对齐
    /// 而非放松）**：装配侧该态的唯一成因 = 审计 sink 建不起来 ⇒ 回 `AuditUnavailable` 让屏上
    /// 落到 EDGE-18 的既有固定文案（理由见 handler 内注释）。`ok=false` / `applied=None` /
    /// `audit_id=None` 三条**原样不动**。
    #[tokio::test]
    async fn apply_without_a_write_path_is_a_rejection_not_a_fake_success() {
        let (addr, h) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(test_config())))).await;
        let issued = now_ms();
        let body = format!(
            r#"{{"request_id":"0f7a3e10-1111-4222-8333-444455556666","issued_at_ms":{issued},
            "op":"apply","payload":{{"changes":{{"intercore.port":2405}},"from":"edit"}}}}"#
        );
        let (status, resp) = http(addr, "POST", ConsoleEndpoint::ConfigApply.path(), Some(&body)).await;
        assert_eq!(status, 200, "写端点的结局**一律**走信封（非 2xx 会让渲染端丢掉具体原因）");
        let r: mupc_display_proto::ControlResponse<ConfigView> =
            serde_json::from_str(&resp).expect("必须是可解析的信封（渲染端同款路径）");
        assert!(!r.ok, "未装配写路径 ⇒ 不得 ok=true");
        assert_eq!(
            r.code,
            ControlCode::AuditUnavailable,
            "口径统一（评审建议 6.1）：装配侧 Unavailable 的唯一成因=审计不可用 ⇒ 屏上须落到 EDGE-18 固定文案"
        );
        assert!(r.applied.is_none(), "不得回一个空 ConfigView 冒充处理结果");
        // ⚠️ 口径变更（回执用字网）：`message` 固定为 `receipt::WRITE_PATH_UNAVAILABLE`；
        // 装配原因（外部错误串，含 cmap 外的字）**只在 `tracing`**（本 handler 的 `error!`）。
        // 屏上这条按 `code` 走 EDGE-18 固定文案 ⇒ 信息不丢、屏上不多一个豆腐块。
        assert_eq!(r.message, receipt::WRITE_PATH_UNAVAILABLE);
        assert!(r.audit_id.is_none());
        h.abort();
    }

    /// ①' 其余 **4** 条端点（G-1/G-2 的 config 读 + 写、单元 H 的 logs 两条均已实现）：
    /// **逐条** 501，且**逐条**不能回 404
    /// （404 = 路由没登记 = 屏上无法区分"服务没实现"与"服务根本没这个端点"）。
    #[tokio::test]
    async fn every_registered_but_unimplemented_endpoint_is_honest_per_endpoint() {
        let (addr, h) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(test_config())))).await;
        let mut checked = 0;
        for ep in ConsoleEndpoint::ALL {
            // 已实现的四条不在"未实现"清单内
            if matches!(
                ep,
                ConsoleEndpoint::Config
                    | ConsoleEndpoint::ConfigApply
                    | ConsoleEndpoint::Logs
                    | ConsoleEndpoint::LogsTargets
            ) {
                continue;
            }
            let method = match ep.method() {
                mupc_display_proto::ConsoleMethod::Get => "GET",
                mupc_display_proto::ConsoleMethod::Post => "POST",
            };
            let body = (method == "POST").then_some("{}");
            let (status, resp) = http(addr, method, ep.path(), body).await;
            assert_ne!(status, 404, "`{}` 已登记，不得回 404（{resp}）", ep.path());
            assert!(!(200..300).contains(&status), "`{}` 未实现却回 {status}", ep.path());
            assert_eq!(status, 501, "`{}` 未实现须 501，实际 {status}", ep.path());
            checked += 1;
        }
        assert_eq!(checked, 4, "未实现端点应为 4 条（8 条契约端点 − 4 条已实现）");
        h.abort();
    }

    /// ② 配置源不可用 ⇒ **非 2xx**（不得用 200 + "空视图"冒充成功）。
    #[tokio::test]
    async fn unavailable_config_source_is_non_2xx_not_empty_view() {
        let (addr, h) = spawn_host(ConfigSource::Unavailable("配置内存副本未装配")).await;
        let (status, body) = http(addr, "GET", ConsoleEndpoint::Config.path(), None).await;
        assert!(!(200..300).contains(&status), "不可用不得回 2xx，实际 {status}");
        assert_eq!(status, 503, "配置源不可用应 503，实际 {status}");
        assert!(
            serde_json::from_str::<ConfigView>(&body).is_err(),
            "不可用响应体不得是一个可解析的（哪怕是空的）ConfigView: {body}"
        );
        // 反面对照：同一路径在 Ready 态确实回 200 —— 证明上面的 503 是"不可用"而非"路由坏了"
        h.abort();
        let (addr2, h2) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(test_config())))).await;
        assert_eq!(http(addr2, "GET", ConsoleEndpoint::Config.path(), None).await.0, 200);
        h2.abort();
    }

    /// ③ 只允许回环：非回环 listener 必须**拒绝服务**（PL-4）。
    #[tokio::test]
    async fn non_loopback_listener_is_refused() {
        let host = ConsoleHost::new(ConsoleDeps {
            config: ConfigSource::Ready(Arc::new(RwLock::new(test_config()))),
            apply: ApplySource::Unavailable("本用例只验回环裁决"),
            logs: LogSource::Unavailable("本用例只验回环裁决"),
        });
        let listener = TcpListener::bind("0.0.0.0:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(!addr.ip().is_loopback(), "本用例前提：0.0.0.0 非回环");
        // **带超时**：守卫一旦失效（serve 照常进入服务循环）这里必须**红**，而不是把用例**挂死**
        // （挂死的用例在 CI 上表现为整体超时，定位成本远高于一条失败断言）。
        let out = tokio::time::timeout(Duration::from_secs(2), host.serve(listener)).await;
        let err = out
            .expect("非回环 listener 必须被立即拒绝：serve 不得进入服务循环")
            .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("PL-4"), "原因须可定位安全红线: {err}");
        // 正例：回环 listener 正常服务（同一路径、同一构造）
        let (addr2, h2) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(test_config())))).await;
        assert_eq!(http(addr2, "GET", ConsoleEndpoint::Config.path(), None).await.0, 200);
        h2.abort();
    }

    /// 路由注册表的两个结构性行为：未知路径 404 / 已登记路径方法不符 405。
    #[tokio::test]
    async fn unknown_path_404_and_method_mismatch_405() {
        let (addr, h) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(test_config())))).await;
        // 未知路径 → 404
        let (s404, _) = http(addr, "GET", "/v1/console/nope", None).await;
        assert_eq!(s404, 404);
        // 已登记的 GET 路径收到 POST → 405（且**不是** 404、**不是** 501：路径存在、方法不对）
        let (s405, _) = http(addr, "POST", ConsoleEndpoint::Config.path(), Some("{}")).await;
        assert_eq!(s405, 405);
        // 已登记的 POST 路径收到 GET → 405
        let (s405b, _) = http(addr, "GET", ConsoleEndpoint::ConfigApply.path(), None).await;
        assert_eq!(s405b, 405);
        h.abort();
    }

    /// 与**读通道**（9810，`LoopbackHttpPublisher`）并存不冲突：两条独立 listener、两套路径集。
    ///
    /// 用同一进程内的真实实现（而非 mock）验证：读通道未就绪时的 503 是它自己的语义，
    /// 不影响控制通道回 200；两条通道的路径集**不相交**（各回 404）。
    #[tokio::test]
    async fn coexists_with_read_channel_publisher_on_distinct_ports() {
        let latest: crate::display_host::SharedLatest = Arc::new(std::sync::Mutex::new(None));
        let publisher = crate::display_host::LoopbackHttpPublisher::new(latest);
        let l1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let a1 = l1.local_addr().unwrap();
        let h1 = tokio::spawn(publisher.serve(l1));
        let (a2, h2) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(test_config())))).await;
        assert_ne!(a1, a2, "读 / 控制须各占一端点（设计 §4.9）");

        // 读通道：未发布过帧 ⇒ 503（其自身语义）
        assert_eq!(
            http(a1, "GET", mupc_display_proto::LATEST_PATH, None).await.0,
            503
        );
        // 控制通道同时正常服务（互不干扰）
        assert_eq!(http(a2, "GET", ConsoleEndpoint::Config.path(), None).await.0, 200);
        // 路径集不相交：控制路径在读通道上 404，读路径在控制通道上 404
        assert_eq!(http(a1, "GET", ConsoleEndpoint::Config.path(), None).await.0, 404);
        assert_eq!(
            http(a2, "GET", mupc_display_proto::LATEST_PATH, None).await.0,
            404
        );
        h1.abort();
        h2.abort();
    }

    // ── ③ requires_reconnect 对账网（**逐字段**，不是数个数）─────────────────

    /// 设计 §4.3.3（行 783–791）里 `requires_reconnect=true` 的字段**逐个**必须为 `true`。
    ///
    /// 判据不是"数量对得上"，而是**逐键**比对：多一个 ⇒ 屏上谎报"链路瞬断"（L1 被抬成 L2+）；
    /// 少一个 ⇒ 屏上漏报（危险操作被降级成 L1）。
    #[tokio::test]
    async fn requires_reconnect_matches_design_section_4_3_3_per_field() {
        let (addr, h) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(test_config())))).await;
        let view = view_from(addr).await;
        h.abort();

        // 设计 §4.3.3 逐行：`true` 的键（行 787 核间端点、行 790 IEC 104 监听地址/端口）
        for key in [
            "intercore.host",      // §4.3.3 行 787
            "intercore.port",      // §4.3.3 行 787
            "gateway.listen_addr", // §4.3.3 行 790
            "gateway.listen_port", // §4.3.3 行 790
        ] {
            assert!(
                field(&view, key).requires_reconnect,
                "`{key}` 在设计 §4.3.3 里副作用为「链路瞬断」⇒ requires_reconnect 必须为 true"
            );
        }
        // 设计 §4.3.3 逐行：`false` 的键（行 785 日志级别、行 788 核间心跳/重连）
        for key in [
            "system.log_level",                // §4.3.3 行 785（时效 ≤1 s，无副作用）
            "intercore.heartbeat_interval_sec", // §4.3.3 行 788（下一拍，无）
            "intercore.reconnect_interval_sec", // §4.3.3 行 788（下一拍，无）
            "display.bind_addr",               // 只读字段（不可改 ⇒ 永不进「本次改动」）
            "display.control_bind_addr",       // 同上
        ] {
            assert!(
                !field(&view, key).requires_reconnect,
                "`{key}` 不涉及链路重建 ⇒ requires_reconnect 必须为 false（否则 L1 永不达、属谎报）"
            );
        }

        // 「true 的键集合」**恰好**等于设计所列（多/少都红）
        let mut got: Vec<&str> = view
            .groups
            .iter()
            .flat_map(|g| g.fields.iter())
            .filter(|f| f.requires_reconnect)
            .map(|f| f.key.as_str())
            .collect();
        got.sort_unstable();
        assert_eq!(
            got,
            vec![
                "gateway.listen_addr",
                "gateway.listen_port",
                "intercore.host",
                "intercore.port"
            ],
            "requires_reconnect=true 的字段集必须与设计 §4.3.3 逐键一致"
        );

        // 渲染端的可用性前提：**既非全 true 也非全 false**（全 true ⇒ L2+ 恒亮；全 false ⇒ 弹层永不提瞬断）
        let total = view.groups.iter().map(|g| g.fields.len()).sum::<usize>();
        assert!(!got.is_empty() && got.len() < total, "分级判据须有区分度");
    }

    // ── 元数据表与 CoreConfig 的一致性（设计 §11.3「防止 UI 能改一个不存在的键」）──

    /// 每个 `key` 对应的 `current` 提取器必须真的从 `CoreConfig` 取出值：
    /// 用**两份改过不同值的配置**对比，逐字段断言"值跟着变" ⇒ 排除常量/copy-paste 的假实现。
    #[tokio::test]
    async fn every_field_extractor_reads_live_core_config() {
        let a = test_config();
        let mut b = test_config();
        b.system.log_level = "warn".to_string();
        b.intercore.host = "192.168.1.1".to_string();
        b.intercore.port = 1;
        b.intercore.heartbeat_interval_sec = 2;
        b.intercore.reconnect_interval_sec = 3;
        b.gateway.listen_addr = "127.0.0.1".to_string();
        b.gateway.listen_port = 1;
        let va = config_view(&a, 0, WriteMode::TextPreserve);
        let vb = config_view(&b, 0, WriteMode::TextPreserve);
        let mut changed = 0;
        for m in FIELDS {
            let fa = field(&va, m.key);
            let fb = field(&vb, m.key);
            if fa.value != fb.value {
                changed += 1;
            }
        }
        // 除 2 个只读 display 字段（两份配置都写 127.0.0.1）外，其余 7 个字段的值都应随之变化
        assert_eq!(
            changed,
            FIELDS.len() - 2,
            "每个可写字段的 current 必须读内存副本（未变化的字段 = 疑似常量）"
        );
        // 逐字段：值的 JSON 形态必须过自身 kind 的校验（否则屏上该行恒为「注入值非法」红行）
        for m in FIELDS {
            let f = field(&va, m.key);
            assert!(
                f.kind.validate_value(&f.value).is_ok(),
                "`{}` 的当前值 {:?} 未过自身 kind 校验（屏上恒红）",
                m.key,
                f.value
            );
            // **default 也必须过自身 kind 的校验**（渲染端 PD22：default 非法会让「恢复默认值」
            // 对整单被打回；后端字段表不得自相矛盾）
            assert!(
                f.kind.validate_value(&f.default).is_ok(),
                "`{}` 的 default {:?} 未过自身 kind 校验（字段表自相矛盾）",
                m.key,
                f.default
            );
        }
    }

    #[test]
    fn metadata_table_has_no_duplicate_or_pending_keys() {
        let mut keys: Vec<&str> = FIELDS.iter().map(|m| m.key).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n, "字段表有重复 key");
        for p in PENDING_NO_CARRIER {
            assert!(
                !FIELDS.iter().any(|m| m.key == p),
                "`{p}` 无 CoreConfig 承载，**不得**进视图（否则就是「屏上能改、装置里不存在」）"
            );
        }
    }

    #[test]
    fn yaml_path_matches_key_for_every_field() {
        // §4.3.2.1：保留式编辑以 yaml_path 定位标量行；本单元虽不消费，仍须与 key 同形，
        // 否则 G-2 会出现两套路径串（一个真源变两个）。
        for m in FIELDS {
            assert_eq!(m.yaml_path, m.key, "yaml_path 必须与 key 同形（§4.3.2.1）");
        }
    }

    #[test]
    fn minimal_core_yaml_parses_and_yields_core_config_defaults() {
        // 内嵌 yaml 必须能解析（否则 def() panic）
        let d = def();
        // 默认值的真源 = core_config.rs 的 default_*()：逐项对齐（core_config.rs:323-432）
        assert_eq!(d.system.log_level, "info");
        assert_eq!(d.intercore.host, "127.0.0.1");
        assert_eq!(d.intercore.port, 9100);
        assert_eq!(d.intercore.heartbeat_interval_sec, 5);
        assert_eq!(d.intercore.reconnect_interval_sec, 3);
        assert_eq!(d.gateway.listen_addr, "0.0.0.0");
        assert_eq!(d.gateway.listen_port, 2404);
        // display 段默认来自 display-proto 的 DisplayConfig::default()
        assert_eq!(d.display.bind_addr, mupc_display_proto::DEFAULT_BIND);
        assert_eq!(
            d.display.control_bind_addr,
            mupc_display_proto::DEFAULT_CONTROL_BIND
        );
        // 缺省配置自洽（若将来 CoreConfig 新增必填段，本用例会先红，而不是屏上默认值静默变 null）
        assert!(d.validate().is_ok(), "缺省配置须自洽: {:?}", d.validate());
    }

    #[test]
    fn loopback_host_of_splits_host_and_port() {
        assert_eq!(loopback_host_of("127.0.0.1:9810"), "127.0.0.1");
        assert_eq!(loopback_host_of("127.0.0.1"), "127.0.0.1");
        assert_eq!(loopback_host_of("[::1]:9811"), "::1");
        assert_eq!(loopback_host_of("::1"), "::1");
        assert_eq!(loopback_host_of("localhost:9811"), "localhost");
    }

    /// label / 分组标签**逐字落在生成字体 cmap 内**（渲染端 PD13：豆腐块的直接来源是后端文案）。
    ///
    /// 读 `local-display/fonts/lv_font_cmap.txt`；文件不在（如交叉编译的纯净树）则跳过——**不**用
    /// "文件不在就当成通过"以外的弱断言：这里显式 `return` 并打印（跳过可见）。
    #[test]
    fn every_label_is_inside_generated_font_cmap() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../local-display/fonts/lv_font_cmap.txt"
        );
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("跳过：{path} 不可读（非本仓库开发树）");
            return;
        };
        let mut cps = std::collections::HashSet::new();
        for line in text.lines() {
            let t = line.trim();
            if let Some(hex) = t.strip_prefix("U+").or_else(|| t.split("U+").nth(1)) {
                let hex: String = hex.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    cps.insert(n);
                }
            }
        }
        assert!(!cps.is_empty(), "cmap 解析为空，本用例已失效");
        let check = |s: &str, what: &str| {
            let missing: Vec<char> = s.chars().filter(|c| !cps.contains(&(*c as u32))).collect();
            assert!(
                missing.is_empty(),
                "{what} `{s}` 含 cmap 外字符 {missing:?} ⇒ 真机豆腐块"
            );
        };
        for (_, label) in GROUPS {
            check(label, "分组标签");
        }
        for m in FIELDS {
            check(m.label, "字段标签");
            if let Some(u) = m.unit {
                check(u, "单位");
            }
            if let ConfigKind::Enum { options } = (m.kind)() {
                for o in options {
                    check(&o.label, "枚举选项标签");
                }
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // G-2：写路径（`POST /v1/console/config/apply`）**端到端**（渲染端同款线协议）
    // ═══════════════════════════════════════════════════════════════════════

    use crate::config_service::ConfigService;
    use crate::console_audit::{AuditIntent, ConsoleAuditSink};
    use crate::testutil::TempDir;
    use mupc_display_proto::ConsoleAuditEntry;

    /// 端到端样例 yaml：含必需段（`CoreConfig` 无 serde 默认的那几段）+ 注释 + 未建模键。
    const WRITE_YAML: &str = r#"# 现场 yaml（注释必须保留）
version: "0.1.0"
system:
  log_level: info        # 现场调过
intercore:
  host: 10.0.0.7
  port: 9100   # PCS 端口
web_api:
  tls_cert: null
  tls_key: null
ai_engine: {}
plugins: {}
gateway:
  listen_addr: 0.0.0.0
  listen_port: 2404
  future_key: keep-me
"#;

    /// 恒失败的审计（fail-closed 的端到端注入点）。
    struct BrokenSink;

    impl ConsoleAuditSink for BrokenSink {
        fn record_intent(&self, _i: &AuditIntent) -> Result<(), String> {
            Err("注入失败：审计目录不可写".to_string())
        }
        fn record_outcome(&self, _e: &ConsoleAuditEntry) -> Result<(), String> {
            Err("注入失败：审计文件不可写".to_string())
        }
    }

    /// 写路径端到端宿主（真实 `FileAuditSink` + 真实 `ConfigService` + 真实 axum）。
    struct WriteHost {
        _dir: TempDir,
        yaml_path: std::path::PathBuf,
        core: Arc<RwLock<CoreConfig>>,
        addr: SocketAddr,
        task: tokio::task::JoinHandle<()>,
        svc: Arc<ConfigService>,
    }

    impl WriteHost {
        fn disk(&self) -> String {
            std::fs::read_to_string(&self.yaml_path).unwrap()
        }
    }

    impl Drop for WriteHost {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    /// 装配写宿主。`yaml = None` ⇒ 用 [`WRITE_YAML`]；`sink = None` ⇒ 真实文件审计。
    ///
    /// `HotApply::new(None)` ⇒ **连 `system.log_level` 也报"需重启"**（"无 reload handle"的诚实
    /// 回退）。要测"热生效路径的回执"用 [`spawn_write_host_hot`]。
    async fn spawn_write_host(
        tag: &str,
        yaml: Option<&str>,
        sink: Option<Arc<dyn ConsoleAuditSink>>,
    ) -> WriteHost {
        spawn_write_host_hot(tag, yaml, sink, crate::hot_apply::HotApply::new(None)).await
    }

    /// 同上，但**注入指定的 `HotApply`**（回执文案用例需要"真热生效"与"需重启"两条分支）。
    async fn spawn_write_host_hot(
        tag: &str,
        yaml: Option<&str>,
        sink: Option<Arc<dyn ConsoleAuditSink>>,
        hot: crate::hot_apply::HotApply,
    ) -> WriteHost {
        let dir = TempDir::new(tag);
        let text = yaml.unwrap_or(WRITE_YAML);
        let yaml_path = dir.write("mupc_core_config.yaml", text);
        // 内存副本 **由同一份文本解析**（与生产一致：启动期读的同一文件）
        let cfg: CoreConfig = serde_yaml::from_str(text).expect("写用例样例 yaml 必须可解析");
        let core = Arc::new(RwLock::new(cfg));
        let audit: Arc<dyn ConsoleAuditSink> =
            sink.unwrap_or_else(|| Arc::new(crate::console_audit::FileAuditSink::open(dir.path()).unwrap()));
        let svc = Arc::new(ConfigService::new(yaml_path.clone(), core.clone(), audit, hot));
        let (addr, task) = spawn_host_with_apply(
            crate::startup::console_config_source(&core),
            ApplySource::Ready(svc.clone()),
        )
        .await;
        WriteHost {
            _dir: dir,
            yaml_path,
            core,
            addr,
            task,
            svc,
        }
    }

    /// 发一次写请求（渲染端 `ConsoleClient::begin_write` 同款线格式：POST + JSON body + `op=apply`）。
    async fn post_apply(
        addr: SocketAddr,
        request_id: &str,
        changes: serde_json::Value,
        op: &str,
    ) -> (u16, ControlResponse<ConfigView>) {
        post_apply_from(addr, request_id, changes, op, "edit").await
    }

    /// 同上，但**指定 `PatchSource`**（`edit` / `reset_default`）—— 契约两条来源走同一条管线，
    /// 但回执文案必须**两条都**过用字网（`config_receipt_messages_use_only_font_cmap_glyphs`）。
    async fn post_apply_from(
        addr: SocketAddr,
        request_id: &str,
        changes: serde_json::Value,
        op: &str,
        from: &str,
    ) -> (u16, ControlResponse<ConfigView>) {
        let body = serde_json::json!({
            "request_id": request_id,
            "issued_at_ms": now_ms(),
            "op": op,
            "payload": {"changes": changes, "from": from},
        })
        .to_string();
        let (status, resp) =
            http(addr, "POST", ConsoleEndpoint::ConfigApply.path(), Some(&body)).await;
        let parsed = serde_json::from_str::<ControlResponse<ConfigView>>(&resp)
            .unwrap_or_else(|e| panic!("回执必须是合法信封（渲染端同款路径）: {e}\n{resp}"));
        (status, parsed)
    }

    /// 正常保存端到端：**线协议**（200 + 信封）→ 落盘 → `GET` 立刻反映新值 + `revision=1`。
    #[tokio::test]
    async fn post_apply_over_the_wire_persists_and_get_shows_the_new_value() {
        let w = spawn_write_host("apply-e2e", None, None).await;
        let before = w.disk();
        let (status, resp) = post_apply(
            w.addr,
            "0f7a3e10-1111-4222-8333-444455556666",
            json!({"intercore.port": 2405}),
            "apply",
        )
        .await;
        assert_eq!(status, 200, "写端点回执一律走信封（HTTP 200）");
        assert!(resp.ok, "{resp:?}");
        let applied = resp.applied.expect("成功必须带新视图");
        assert_eq!(field(&applied, "intercore.port").value, json!(2405));
        assert_eq!(applied.revision, 1);
        assert_eq!(applied.write_mode, WriteMode::TextPreserve, "正常路径 = 保留式编辑");
        assert!(resp.audit_id.is_some(), "成功回执带审计号（现场对拍）");

        // 文件：只改目标行，注释与未建模键逐字保留
        assert_eq!(w.disk(), before.replace("port: 9100", "port: 2405"));
        assert!(w.disk().contains("future_key: keep-me"));

        // GET 立刻反映（**同一份内存副本**）：这是 UI「保存后不等下一帧就刷新」的依据
        let view = view_from(w.addr).await;
        assert_eq!(field(&view, "intercore.port").value, json!(2405));
        assert_eq!(view.revision, 1, "GET 的 revision 必须来自写服务（不是常量 0）");
    }

    /// 幂等端到端：同一个 `request_id` 再发一次（渲染端超时重试的真实形态）⇒ `duplicate=true`，
    /// 且**文件不再被写一遍**。
    #[tokio::test]
    async fn post_apply_duplicate_over_the_wire_is_flagged_and_not_re_executed() {
        let w = spawn_write_host("apply-dup", None, None).await;
        let (_, first) = post_apply(w.addr, "rid-dup", json!({"intercore.port": 2405}), "apply").await;
        let text = w.disk();
        let (status, second) =
            post_apply(w.addr, "rid-dup", json!({"intercore.port": 2405}), "apply").await;
        assert_eq!(status, 200);
        assert!(second.duplicate, "重复请求必须带 duplicate=true（幂等命中标记）");
        assert_eq!(second.code, first.code);
        assert_eq!(second.audit_id, first.audit_id, "复用首次审计记录");
        assert_eq!(w.disk(), text, "不得被第二次请求再写一遍");
        assert_eq!(w.svc.revision(), 1, "只真正保存了一次");
    }

    /// 校验失败端到端：`field_errors` 逐字段到达屏上（CF-02），且**无副作用**。
    #[tokio::test]
    async fn post_apply_rejected_validation_carries_field_errors_to_the_screen() {
        let w = spawn_write_host("apply-inv", None, None).await;
        let before = w.disk();
        let (status, resp) = post_apply(
            w.addr,
            "rid-inv",
            json!({"intercore.port": 0, "system.log_level": "debug"}),
            "apply",
        )
        .await;
        assert_eq!(status, 200, "业务拒绝也走 200 + 信封（渲染端才有「具体原因」可显示）");
        assert!(!resp.ok);
        assert_eq!(resp.code, ControlCode::RejectedValidation);
        assert_eq!(resp.field_errors.len(), 1, "逐字段标红的数据源");
        assert_eq!(resp.field_errors[0].field, "intercore.port");
        assert!(resp.field_errors[0].reason.contains("越界"));
        assert!(resp.applied.is_none());
        assert_eq!(w.disk(), before, "拒绝 ⇒ 文件一个字节都不动");
        assert_eq!(view_from(w.addr).await.revision, 0);
    }

    /// body 不是合法信封 ⇒ **仍然是 HTTP 200 + 可读信封**（不得回 422 让渲染端丢原因）。
    #[tokio::test]
    async fn malformed_body_still_gets_a_readable_receipt() {
        let w = spawn_write_host("apply-bad-body", None, None).await;
        for bad in ["", "not json at all", r#"{"request_id":"r1"}"#] {
            let (status, body) =
                http(w.addr, "POST", ConsoleEndpoint::ConfigApply.path(), Some(bad)).await;
            assert_eq!(status, 200, "畸形 body 也不得回非 2xx（`{bad}`）");
            let r: ControlResponse<ConfigView> = serde_json::from_str(&body)
                .unwrap_or_else(|e| panic!("必须是可解析信封（`{bad}`）: {e}\n{body}"));
            assert!(!r.ok && r.code == ControlCode::RejectedValidation, "`{bad}` → {r:?}");
            assert!(!r.message.is_empty(), "必须给得出具体原因");
        }
    }

    /// 审计不可写 ⇒ **fail-closed 端到端**：`AuditUnavailable` + 值未变（渲染端 EDGE-18 的固定文案）。
    #[tokio::test]
    async fn audit_unavailable_is_fail_closed_over_the_wire() {
        let w = spawn_write_host("apply-nofailclosed", None, Some(Arc::new(BrokenSink))).await;
        let before = w.disk();
        let (status, resp) =
            post_apply(w.addr, "rid-audit", json!({"intercore.port": 2405}), "apply").await;
        assert_eq!(status, 200);
        assert!(!resp.ok && resp.code == ControlCode::AuditUnavailable);
        assert!(resp.applied.is_none(), "操作未生效");
        assert!(resp.audit_id.is_none(), "不得编造审计号");
        assert_eq!(w.disk(), before);
        assert_eq!(w.core.read().await.intercore.port, 9100, "内存副本不得变");
        assert_eq!(view_from(w.addr).await.revision, 0);
    }

    /// 不可定位 ⇒ 整体回写，`write_mode=full_rewrite` **在回执里可见**（EDGE-23 的 UI Toast 判据）。
    #[tokio::test]
    async fn full_rewrite_fallback_is_declared_in_the_receipt() {
        // 样例里**没有** `gateway:` 段 ⇒ 改 gateway.listen_port 无法定位
        let yaml = "# 注释会丢\nversion: \"0.1.0\"\nsystem:\n  log_level: info\n\
                    intercore:\n  host: 10.0.0.7\n  port: 9100\n\
                    web_api:\n  tls_cert: null\n  tls_key: null\n\
                    ai_engine: {}\nplugins: {}\n";
        let w = spawn_write_host("apply-fallback", Some(yaml), None).await;
        let (_, resp) =
            post_apply(w.addr, "rid-fb", json!({"gateway.listen_port": 2405}), "apply").await;
        assert!(resp.ok, "{resp:?}");
        let applied = resp.applied.unwrap();
        assert_eq!(applied.write_mode, WriteMode::FullRewrite, "回执必须显式声明整体重写");
        // 明示降级的话术取 `receipt::FULL_REWRITE_UNLOCATABLE`（`整`/`体`/`写` 都不在 cmap 内
        // ⇒ 不能写"整体重写"）；与渲染端 `p2_config::TEXT_TOAST_FULL_REWRITE` 同款措辞。
        // **逐字**比整条串（比原来的 `contains("整体重写")` 更严）：`gateway.listen_port`
        // 同时是"需重启"键 ⇒ 两段子句必须**都在**、顺序与分隔符也不许漂。
        assert_eq!(
            resp.message,
            format!(
                "{} · {} · 需重启进程生效: 端口",
                receipt::SAVED,
                receipt::FULL_REWRITE_UNLOCATABLE
            ),
            "降级 + 需重启两段子句都要在（且用字 ⊆ cmap）"
        );
        assert!(w.disk().contains("listen_port: 2405"), "值确实落盘");
        // 后续 GET 也带着这个模式（UI 在任何视图上都该明示）
        assert_eq!(view_from(w.addr).await.write_mode, WriteMode::FullRewrite);
    }

    /// **写服务与装配共用同一份内存副本**（G-1 那条 `Arc::ptr_eq` 网在写侧的对称面）：
    /// 若写服务拿到的是**深拷贝**，则"写 A、GET 读 B"——保存后屏上仍是旧值（静默失实效）。
    #[tokio::test]
    async fn write_service_shares_the_assembly_arc() {
        let w = spawn_write_host("apply-samearc", None, None).await;
        // 写服务持有的是哪一份？（`ConsoleDeps.config` 给的又是哪一份？——两者必须**同一地址**）
        let from_write = w.svc.core().clone();
        let from_read = match crate::startup::console_config_source(&w.core) {
            ConfigSource::Ready(a) => a,
            ConfigSource::Unavailable(r) => panic!("装配后不得 Unavailable（{r}）"),
        };
        assert!(
            Arc::ptr_eq(&from_write, &from_read),
            "写服务与读路径必须共用同一份 `Arc<RwLock<CoreConfig>>`（否则保存后屏上仍是旧值）"
        );
        // 负对照：深拷贝出的新 Arc **不会** ptr_eq（否则上面那条断言没有鉴别力）
        let drifted = Arc::new(RwLock::new(w.core.read().await.clone()));
        assert!(!Arc::ptr_eq(&from_write, &drifted), "负对照失效");
    }

    /// 路由表**不得**漏登记契约端点（`ConsoleEndpoint::ALL` 是唯一真源）。
    #[tokio::test]
    async fn router_registers_every_contract_endpoint_path() {
        let (addr, h) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(test_config())))).await;
        for ep in ConsoleEndpoint::ALL {
            let method = match ep.method() {
                mupc_display_proto::ConsoleMethod::Get => "GET",
                mupc_display_proto::ConsoleMethod::Post => "POST",
            };
            let body = (method == "POST").then_some("{}");
            let (status, _) = http(addr, method, ep.path(), body).await;
            assert_ne!(status, 404, "契约端点 `{}` 未登记进路由表", ep.path());
            assert_ne!(status, 405, "契约端点 `{}` 的方法注册错误", ep.path());
        }
        h.abort();
    }

    // ═══ 回执文案的**用字网**（cmap ⊆）═══════════════════════════════════════════

    /// 生成字体的 cmap 清单（**入库真源**：`local-display/fonts/lv_font_cmap.txt`，324 码位）。
    ///
    /// # 为什么读**兄弟 crate 的文件**而不是"在 mupc-core-bin 里抄一份清单"
    ///
    /// 抄一份 = **第二真源**：渲染端与后端各存一份"能上屏的字"，字库一变（跑 `gen_fonts.sh`
    /// 重新选字）两边会**静默不一致**，而后端这份抄件没有任何东西在看着它。
    ///
    /// # 为什么用 `include_str!` 而不是运行时 `read_to_string(相对路径)`
    ///
    /// ① **与工作目录无关**（`cargo test` 的 CWD 是实现细节，`include_str!` 按**源文件**相对
    ///    路径在**编译期**读）⇒ 不会有"CI 上刚好读不到于是静默跳过"这种事；
    /// ② 清单改了 ⇒ 本 crate **必须重编**（漂移当场暴露，而不是等到下一轮 CI）。
    /// 路径校验：`src/console_host.rs` → `../../local-display/fonts/lv_font_cmap.txt`。
    fn font_cmap() -> std::collections::BTreeSet<char> {
        let src = include_str!("../../local-display/fonts/lv_font_cmap.txt");
        let set: std::collections::BTreeSet<char> = src
            .lines()
            .filter_map(|l| l.trim().strip_prefix("U+"))
            .filter_map(|h| u32::from_str_radix(h, 16).ok())
            .filter_map(char::from_u32)
            .collect();
        // 清单读空 ⇒ 后面的断言会**全部通过**（空集之外全是"缺字"… 反了：空 baseline 会让
        // `is_subset` 恒 false，红得很响）。这里仍显式挡一道，防"解析器与清单格式脱节"被
        // 误读成"文案有问题"。
        assert!(
            set.len() >= 300,
            "码表清单解析出 {} 个码位（应 ≥300）—— 清单损坏或解析器与格式脱节",
            set.len()
        );
        set
    }

    /// 逐字符判定 + **诊断串**（缺字逐个列 `U+XXXX`，不靠肉眼）。
    fn missing_glyphs(text: &str, cmap: &std::collections::BTreeSet<char>) -> Vec<String> {
        let mut out: Vec<String> = text
            .chars()
            .filter(|c| !cmap.contains(c))
            .map(|c| format!("{:?}(U+{:04X})", c, c as u32))
            .collect();
        out.dedup();
        out
    }

    /// **本任务的核心网**：控制面**回执 `message`** 的用字必须 ⊆ 生成字体的 cmap。
    ///
    /// # 为什么这条网必须存在（而不是"自由文本不查码表"的既有口径）
    ///
    /// `local-display` 的既有口径是「**自由文本**（告警 `message` / 型号 / 序列号 / 管理 IP）
    /// 不处理」—— 那些串**不是 mupcd 生成的**。本组文案**性质不同**：逐字由我们自己拼，
    /// 且渲染端**原样上屏**（成功路径连 `display_safe` 都不过，见
    /// `p2_config.rs::success_toast_text`；失败路径过的 `display_safe` 对**非 ASCII 原样透传**）
    /// ⇒ 一个 cmap 外的字 = 真机上一个**豆腐块**，是**自己造的缺陷**。
    /// 修复前的原串 `配置已保存（1 项）；1 项需重启 mupcd 生效: gateway.listen_port`
    /// 在这一条下面**有 8 类缺字**：`（`/`）`/`项`/`；` 全是缺字形，`mupcd` 与键名的小写字母
    /// （`m`/`u`/`p`/`c`/`d`/`t`/`_`…）也全是。
    ///
    /// # 网分**两半**（不然就是构造性漏判）
    ///
    /// - **构造性半边**：`receipt::ALL` 的每一条常量逐字符 ⊆ cmap；
    /// - **运行期半边**：**真的**发 HTTP 请求走完管线，把**回执里真实的 `message`** 抓回来
    ///   再过一次网 —— 防"常量是干净的被测对象，拼出来的串却是脏的"（正是本次修复前的形态：
    ///   模板与拼接各改一半也能骗过只看常量的网）。
    ///
    /// # 敏感性（破坏性探针，实测见交付报告）
    ///
    /// ① 把 `receipt::SAVED` 改成 `"配置已保存（1 项）"`（= 修复前的用字）⇒ 构造性半边当场红；
    /// ② 把 `config_service` 的成功拼接改回 `format!("配置已保存（{} 项）", ..)` ⇒ 运行期半边红；
    /// ③ 把 `restart_labels` 改回 `restart.join(",")`（点名机器键名）⇒ 运行期半边红；
    /// ④ 把下面的自检探针（`POISON`）换成 cmap 内的字 ⇒ 红（证明这条网**真的有牙**，
    ///    而不是"恒真断言"）。
    #[tokio::test]
    async fn config_receipt_messages_use_only_font_cmap_glyphs() {
        let cmap = font_cmap();

        // ⓪ **自检探针**：网本身必须能判出缺字（否则整条用例是恒真的摆设）。
        //    取样覆盖本次修复前的**每一类**缺字：汉字 / 全角标点 / 小写 ASCII / 下划线。
        for poison in ["项", "（", "）", "；", "：", "，", "m", "t", "_", "⇒"] {
            assert!(
                !missing_glyphs(poison, &cmap).is_empty(),
                "探针 `{poison}` 被判成「cmap 内」⇒ 这条网认不出缺字，是恒真断言"
            );
        }
        assert!(
            missing_glyphs("配置已保存 · 需重启进程生效: 端口", &cmap).is_empty(),
            "正对照：修复后的措辞必须在 cmap 内"
        );

        // ① **构造性半边**：`receipt::ALL` 逐条逐字符。
        for text in receipt::ALL {
            let miss = missing_glyphs(text, &cmap);
            assert!(
                miss.is_empty(),
                "回执文案 `{text}` 含 cmap 外字符（真机豆腐块）：{}",
                miss.join(" ")
            );
        }

        // ② 字段 `label` 也会被拼进成功回执（`restart_labels`）⇒ 一并过网。
        //    （`label` 来自 `FIELDS`，是屏上每一行的标题，**必须**在 cmap 内。）
        for m in FIELDS {
            let miss = missing_glyphs(m.label, &cmap);
            assert!(
                miss.is_empty(),
                "字段 `{}` 的 label `{}` 含 cmap 外字符（拼接后进回执）：{}",
                m.key,
                m.label,
                miss.join(" ")
            );
        }

        // ③ **运行期半边**：真发请求，抓真实回执。
        let mut seen: Vec<String> = Vec::new();
        let check = |tag: &str, msg: &str, seen: &mut Vec<String>| {
            let miss = missing_glyphs(msg, &cmap);
            assert!(
                miss.is_empty(),
                "[{tag}] 真实回执 `message` 含 cmap 外字符（真机豆腐块）：{}\n  实得: {msg}",
                miss.join(" ")
            );
            seen.push(msg.to_string());
        };

        // ③-a 成功 + **热生效**（`system.log_level` 真接线）⇒ 回执**不含**「需重启」。
        //     真实 reload 句柄（不初始化全局订阅者：句柄只是 layer 的遥控器，与 `hot_apply`
        //     单测同款构造）⇒ `system.log_level` 落 `Applied`。
        let (_layer, reload) =
            tracing_subscriber::reload::Layer::new(tracing_subscriber::EnvFilter::new("info"));
        {
            let w = spawn_write_host_hot(
                "cmap-hot",
                None,
                None,
                crate::hot_apply::HotApply::new(Some(reload)),
            )
            .await;
            let (_, r) = post_apply(
                w.addr,
                "cmap-rid-hot",
                json!({"system.log_level": "debug"}),
                "apply",
            )
            .await;
            assert!(r.ok, "{r:?}");
            assert_eq!(
                r.message,
                receipt::SAVED,
                "热生效路径不得冒出「需重启」子句"
            );
            check("成功/热生效", &r.message, &mut seen);
        }

        // ③-b 成功 + **需重启**：全部 6 个未接线键一起改 ⇒ 逐标签点名（**6 条都**要在串里）。
        {
            let w = spawn_write_host("cmap-restart", None, None).await;
            let (_, r) = post_apply(
                w.addr,
                "cmap-rid-restart",
                json!({
                    "gateway.listen_addr": "127.0.0.1",
                    "gateway.listen_port": 2405,
                    "intercore.host": "192.168.3.21",
                    "intercore.port": 2405,
                    "intercore.heartbeat_interval_sec": 11,
                    "intercore.reconnect_interval_sec": 12,
                }),
                "apply",
            )
            .await;
            assert!(r.ok, "{r:?}");
            assert!(
                r.message.contains("需重启"),
                "6 键全未接线 ⇒ 必须说需重启: {}",
                r.message
            );
            for key in [
                "gateway.listen_addr",
                "gateway.listen_port",
                "intercore.host",
                "intercore.port",
                "intercore.heartbeat_interval_sec",
                "intercore.reconnect_interval_sec",
            ] {
                let label = field_meta(key).expect("键必须在 FIELDS 内").label;
                assert!(
                    r.message.contains(label),
                    "回执须**点名** `{key}`（屏上取其 label `{label}`）: {}",
                    r.message
                );
                // 反面对照：**机器键名本身**不得出现在串里（它必然含缺字形的 `t` / `_`）
                assert!(
                    !r.message.contains(key),
                    "机器键名不得进回执（含缺字形字符，点名也会被打散）: {}",
                    r.message
                );
            }
            check("成功/需重启", &r.message, &mut seen);
        }

        // ③-c 成功 + **无字段变化**（不写盘）。
        {
            let w = spawn_write_host("cmap-noop", None, None).await;
            let (_, r) = post_apply(
                w.addr,
                "cmap-rid-noop",
                json!({"intercore.port": 9100}), // 与 WRITE_YAML 同值
                "apply",
            )
            .await;
            assert!(r.ok, "{r:?}");
            assert_eq!(r.message, receipt::NO_CHANGE);
            check("成功/无变化", &r.message, &mut seen);
        }

        // ③-d 校验失败（逐字段拒绝）。
        {
            let w = spawn_write_host("cmap-invalid", None, None).await;
            let (_, r) = post_apply(
                w.addr,
                "cmap-rid-invalid",
                json!({"intercore.port": 0}),
                "apply",
            )
            .await;
            assert!(!r.ok && r.code == ControlCode::RejectedValidation);
            assert_eq!(r.message, receipt::VALIDATION_FAILED);
            check("失败/校验", &r.message, &mut seen);
        }

        // ③-e 信封非法（`op` 误路由 ⇒ 外部原因串**不进** `message`，改由 `field_errors` 承载）。
        {
            let w = spawn_write_host("cmap-envelope", None, None).await;
            let (_, r) = post_apply(
                w.addr,
                "cmap-rid-envelope",
                json!({"intercore.port": 2405}),
                "release",
            )
            .await;
            assert!(!r.ok && r.code == ControlCode::RejectedValidation);
            assert_eq!(r.message, receipt::BAD_ENVELOPE);
            assert!(
                r.field_errors.iter().any(|e| e.reason.contains("release")),
                "误路由的原因必须**不丢**（只是换了渠道：field_errors）: {:?}",
                r.field_errors
            );
            check("失败/信封", &r.message, &mut seen);
        }

        // ③-f 畸形 body（连 JSON 都不是）。
        {
            let w = spawn_write_host("cmap-badbody", None, None).await;
            let (status, body) = http(
                w.addr,
                "POST",
                ConsoleEndpoint::ConfigApply.path(),
                Some("not json"),
            )
            .await;
            assert_eq!(status, 200);
            let r: ControlResponse<ConfigView> = serde_json::from_str(&body).unwrap();
            assert_eq!(r.message, receipt::BAD_ENVELOPE);
            check("失败/畸形 body", &r.message, &mut seen);
        }

        // ③-g **审计不可用**（`BrokenSink` ⇒ fail-closed）。
        {
            let w = spawn_write_host("cmap-nosink", None, Some(Arc::new(BrokenSink))).await;
            let (_, r) = post_apply(
                w.addr,
                "cmap-rid-nosink",
                json!({"intercore.port": 2405}),
                "apply",
            )
            .await;
            assert!(!r.ok && r.code == ControlCode::AuditUnavailable);
            assert_eq!(r.message, receipt::AUDIT_UNAVAILABLE);
            check("失败/审计不可用", &r.message, &mut seen);
        }

        // ③-h **写路径未装配**（装配侧 `Unavailable` ⇒ 统一口径为 `AuditUnavailable`）。
        {
            let (addr, task) = spawn_host_with_apply(
                ConfigSource::Ready(Arc::new(RwLock::new(test_config()))),
                ApplySource::Unavailable("审计 sink 建不起来（注入）"),
            )
            .await;
            let (_, r) = post_apply(
                addr,
                "cmap-rid-nopath",
                json!({"intercore.port": 2405}),
                "apply",
            )
            .await;
            assert_eq!(r.code, ControlCode::AuditUnavailable);
            assert_eq!(r.message, receipt::WRITE_PATH_UNAVAILABLE);
            check("失败/写路径未装配", &r.message, &mut seen);
            task.abort();
        }

        // ③-i `ResetDefault` **来源**（与 `Edit` 同一条管线；回执同样要过网）。
        {
            let w = spawn_write_host("cmap-reset", None, None).await;
            let (_, r) = post_apply_from(
                w.addr,
                "cmap-rid-reset",
                json!({"gateway.listen_port": 2405}),
                "apply",
                "reset_default",
            )
            .await;
            assert!(r.ok, "{r:?}");
            check("成功/恢复默认值来源", &r.message, &mut seen);
        }

        // 兜底：运行期半边**确实**跑过（防"半边被注释掉"）。
        assert!(
            seen.len() >= 9,
            "运行期半边覆盖不足（只抓到 {} 条回执）",
            seen.len()
        );
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 单元 H：`GET /v1/console/logs` + `GET /v1/console/logs/targets`（端到端）
    //
    // 解码方式与渲染端**同款**：`serde_json::from_str::<LogPage>` / `<Vec<String>>`
    // （`console.rs::tick` 的 `decode` 就是这一条路径）。**不做**任何自定义解析。
    // ═══════════════════════════════════════════════════════════════════════

    /// 日志夹具：目录里一个 `mupc.log.<date>`（`count` 条，1 s 一条，级别交替）。
    fn log_fixture(t: &crate::testutil::TempDir, count: u64) -> u64 {
        const BASE_MS: u64 = 1_757_412_000_000;
        let mut body = String::new();
        for i in 0..count {
            let ts = chrono::DateTime::from_timestamp_millis((BASE_MS + i * 1000) as i64)
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            let lvl = if i % 2 == 0 { "ERROR" } else { "INFO" };
            body.push_str(
                &serde_json::json!({
                    "timestamp": ts, "level": lvl, "target": "mupc_gateway",
                    "fields": { "message": format!("msg-{i}") },
                })
                .to_string(),
            );
            body.push('\n');
        }
        t.write("mupc.log.2025-09-09", &body);
        BASE_MS
    }

    /// 渲染端同款查询串（`control_route.rs::log_query_string` 的逐字形态）。
    fn log_qs(range: &str, extra: &str) -> String {
        format!("/v1/console/logs?range={range}{extra}")
    }

    async fn logs_from(addr: SocketAddr, qs: &str) -> (u16, LogPage) {
        let (status, body) = http(addr, "GET", qs, None).await;
        let page = serde_json::from_str::<LogPage>(&body)
            .unwrap_or_else(|e| panic!("裸 LogPage 解码失败（渲染端同款路径）: {e}\n{body}"));
        (status, page)
    }

    /// ① 端到端：真发 HTTP、按渲染端同款方式解 DTO；顺序 / 分页 / 筛选一次覆盖。
    #[tokio::test]
    async fn get_logs_end_to_end_decodes_like_the_render_side() {
        let t = crate::testutil::TempDir::new("h-e2e");
        let base = log_fixture(&t, 30);
        let svc = crate::log_service::LogService::new(
            t.path(),
            mupc_display_proto::config::LogLimits::default(),
        );
        let (addr, h) = spawn_log_host(LogSource::Ready(Arc::new(svc))).await;

        // 渲染端首屏查询：range=1h&limit=20（`LogQuery::default()` 的形态）。夹具时间在 2025，
        // 故这里用 custom 窗口 —— 相对档位用的是**服务端时钟**（生产即是如此）。
        let from = base;
        let to = base + 60_000;
        let qs = log_qs(
            "custom",
            &format!("&from={from}&to={to}&limit=10&levels=error&levels=warn"),
        );
        let (status, page) = logs_from(addr, &qs).await;
        assert_eq!(status, 200);
        assert!(!page.range_too_large, "30 条远未超限");
        assert!(page.has_more, "命中 15 条 > limit=10 ⇒ 还有更早的");
        assert_eq!(page.entries.len(), 10, "limit=10");
        for w in page.entries.windows(2) {
            assert!(w[0].seq > w[1].seq, "必须 seq 降序");
        }
        assert!(page.entries.iter().all(|e| e.level == LogLevel::Error));
        assert_eq!(page.next_cursor, page.entries.last().map(|e| e.seq));

        // 500 ms 增量：cursor = 已见最大 seq（`p3_logs.rs::fire_increment` 的形态）
        let max_seq = page.entries[0].seq;
        let (s2, inc) = logs_from(
            addr,
            &log_qs(
                "custom",
                &format!(
                    "&from={from}&to={to}&cursor={max_seq}&limit=10&levels=error&levels=warn"
                ),
            ),
        )
        .await;
        assert_eq!(s2, 200);
        assert!(inc.entries.iter().all(|e| e.seq > max_seq), "只回更新的");
        assert!(inc.entries.is_empty(), "已到最新 ⇒ 空页（不是重复拉取）");
        assert!(!inc.has_more && !inc.range_too_large);

        // `/logs/targets`（无参 GET）⇒ 裸 Vec<String>
        let (s3, body) = http(addr, "GET", ConsoleEndpoint::LogsTargets.path(), None).await;
        assert_eq!(s3, 200);
        let targets: Vec<String> = serde_json::from_str(&body).expect("裸 Vec<String>");
        assert_eq!(targets, vec!["mupc_gateway".to_string()]);
        h.abort();
    }

    /// ② 多值参数**必须**是重复键；逗号拼接 ⇒ 400（**不是** 200 + 当成两个级别）。
    ///
    /// ⚠️ **整改五 E-3a**：本用例此前用 `range=1h`（夹具时间在 2025、服务端 `now` 是 2026）
    /// ⇒ 窗口恒为空 ⇒ 断言 `status == 200` 与注释所称的"两个级别都解析出来"**在断言层面
    /// 无从体现**（只验了"没报 400"）。现改为 `custom` + 夹具时间窗，并**用命中内容反证**
    /// 两个重复键都被解析：`levels=error&levels=info` 必须把**两种级别各一条**都取回来
    /// （只解析出一个键就会少一条），逗号拼接则仍是 400。
    #[tokio::test]
    async fn multi_value_query_over_the_wire_is_repeated_keys_only() {
        let t = crate::testutil::TempDir::new("h-multi");
        let base = log_fixture(&t, 2); // i=0 ERROR / i=1 INFO
        let svc = crate::log_service::LogService::new(
            t.path(),
            mupc_display_proto::config::LogLimits::default(),
        );
        let (addr, h) = spawn_log_host(LogSource::Ready(Arc::new(svc))).await;

        // 重复键 ⇒ 200，且**两个级别都被解析出来**（命中内容为证）
        let qs = log_qs(
            "custom",
            &format!("&from={base}&to={}&limit=20&levels=error&levels=info", base + 60_000),
        );
        let (ok, page) = logs_from(addr, &qs).await;
        assert_eq!(ok, 200, "{page:?}");
        assert_eq!(page.entries.len(), 2, "两种级别的重复键都必须解析出来：{:?}", page.entries);
        let mut levels: Vec<String> = page.entries.iter().map(|e| format!("{:?}", e.level)).collect();
        levels.sort();
        assert_eq!(
            levels,
            vec!["Error".to_string(), "Info".to_string()],
            "只解析出一个键的话这里会只剩一种级别：{:?}",
            page.entries
        );
        assert!(!page.range_too_large);

        // 逗号拼接 ⇒ 400（**不是**把 "error,info" 当成两个级别）
        let (bad, body) = http(
            addr,
            "GET",
            &log_qs(
                "custom",
                &format!("&from={base}&to={}&limit=20&levels=error,info", base + 60_000),
            ),
            None,
        )
        .await;
        assert_eq!(bad, 400, "逗号拼多值必须被拒: {body}");
        h.abort();
    }

    /// ③ 超限（EDGE-15）：**200 + `range_too_large=true`** —— 它不是错误，是**正常回包**；
    /// **且带回已收集的最新 `limit` 条**（整改五 A 组裁定：不再是空页），与"无日志"
    /// （`range_too_large=false`）**不得互替**。
    #[tokio::test]
    async fn range_too_large_is_a_200_normal_page_distinct_from_empty() {
        let t = crate::testutil::TempDir::new("h-limit");
        let base = log_fixture(&t, 3);
        // 造超限：3 行 > 2 行
        let limits = mupc_display_proto::config::LogLimits {
            max_lines: 2,
            ..Default::default()
        };
        let svc = crate::log_service::LogService::new(t.path(), limits);
        let (addr, h) = spawn_log_host(LogSource::Ready(Arc::new(svc))).await;

        let (status, page) = logs_from(
            addr,
            &log_qs(
                "custom",
                &format!("&from={base}&to={}&limit=20", base + 60_000),
            ),
        )
        .await;
        assert_eq!(status, 200, "超限是正常回包，不是 HTTP 错误");
        assert!(page.range_too_large, "必须显式置位");
        // ⚠️ A 组裁定（本用例原断言 `entries.is_empty()`）：超限页**带回已收集的条目**，
        // 否则渲染端 `p3_logs.rs::apply_page` 见空 ⇒ `shown=0` ⇒ 列表被清空（1h 已是最小档，
        // "再缩也没用"）。这里钉死**具体内容**：3 行里超限发生在第 3 行 ⇒ 交付最新的 2 条。
        assert_eq!(page.entries.len(), 2, "已收集的 2 条必须带上：{:?}", page.entries);
        assert_eq!(
            page.entries.iter().map(|e| e.message.as_str()).collect::<Vec<_>>(),
            vec!["msg-2", "msg-1"],
            "必须是**最新**的两条且倒序"
        );
        assert!(!page.has_more, "2 条 < limit=20");
        assert_eq!(page.next_cursor, None);

        h.abort();

        // 对照：**同样为空**但 range_too_large=false —— 两个信号由字段区分（不是靠 HTTP 状态）。
        // 必须换一个**干净目录**：这里的超限来自上一条 svc 把 `max_lines` 配成 2，而同一时间窗
        // （`from=base, to=base+60000`）在该目录里**确实有 3 行**（R3 之后判据 = **窗口内容**，
        // 与"扫描了多少行"无关）⇒ 复用同一目录会得到同一个超限，测不出"空 ≠ 超限"。
        // ⚠️ S-4：此处原注释写"超限是'扫描工作量'判据（与时间窗无关）"——那是 **R3 之前**的语义，
        // 已过期；判据现为窗口内容 ⇒ 收窄窗口**可以**让同一个目录不再超限（这正是 R3 的要点）。
        let t2 = crate::testutil::TempDir::new("h-limit-empty");
        let svc2 = crate::log_service::LogService::new(
            t2.path(),
            mupc_display_proto::config::LogLimits::default(),
        );
        let (a2, h2) = spawn_log_host(LogSource::Ready(Arc::new(svc2))).await;
        let (s, empty) = logs_from(a2, &log_qs("custom", &format!("&from={base}&to={}&limit=20", base + 60_000))).await;
        assert_eq!(s, 200);
        assert!(empty.entries.is_empty() && !empty.range_too_large, "无日志 ≠ 超限");
        h2.abort();
    }

    /// ④ 日志源不可读 ⇒ **503**（**不得** 200 + 空列表冒充"没有日志"）。
    #[tokio::test]
    async fn unreadable_log_source_is_503_not_empty_page() {
        let t = crate::testutil::TempDir::new("h-503");
        let missing = t.path().join("absent");
        let svc = crate::log_service::LogService::new(
            &missing,
            mupc_display_proto::config::LogLimits::default(),
        );
        let (addr, h) = spawn_log_host(LogSource::Ready(Arc::new(svc))).await;

        let (s1, b1) = http(addr, "GET", "/v1/console/logs?range=1h&limit=20", None).await;
        assert_eq!(s1, 503, "源不可读必须非 2xx: {b1}");
        assert!(
            serde_json::from_str::<LogPage>(&b1).is_err(),
            "错误体不得是一个可解析的（哪怕是空的）LogPage: {b1}"
        );
        let (s2, _) = http(addr, "GET", ConsoleEndpoint::LogsTargets.path(), None).await;
        assert_eq!(s2, 503);

        // 装配缺失（`Unavailable`）走同一条非 2xx 通道
        let (a2, h2) = spawn_log_host(LogSource::Unavailable("日志服务未装配")).await;
        let (s3, _) = http(a2, "GET", "/v1/console/logs?range=1h&limit=20", None).await;
        assert_eq!(s3, 503);
        let (s4, _) = http(a2, "GET", ConsoleEndpoint::LogsTargets.path(), None).await;
        assert_eq!(s4, 503);
        h.abort();
        h2.abort();
    }

    /// ⑤ 只读：写方法打到这两条路径 ⇒ **405**（结构性保证，不执行、不落任何副作用）。
    ///
    /// ⚠️ **整改五 E-3b**：本用例此前跑在**空页**上（夹具时间 2025 + `range=1h` ⇒ 窗口 = 2026
    /// ⇒ 0 条）⇒ 注释所称的"内容与写前**逐字段一致**"在断言层面**无从体现**（空页当然一致）。
    /// 现改为 `custom` + 夹具时间窗，并对**写前 / 写后**两个完整 `LogPage`（含 `entries` 的
    /// 五个字段）做**逐字段比对**，且先断言这一页**非空**（否则比对仍是恒真）。
    #[tokio::test]
    async fn log_endpoints_are_get_only_and_reject_writes_with_405() {
        let t = crate::testutil::TempDir::new("h-405");
        let base = log_fixture(&t, 2);
        let svc = crate::log_service::LogService::new(
            t.path(),
            mupc_display_proto::config::LogLimits::default(),
        );
        let (addr, h) = spawn_log_host(LogSource::Ready(Arc::new(svc))).await;

        let qs = log_qs("custom", &format!("&from={base}&to={}&limit=20", base + 60_000));
        let (_, before) = logs_from(addr, &qs).await;
        let (_, before_targets) = http(addr, "GET", ConsoleEndpoint::LogsTargets.path(), None).await;
        assert!(!before.entries.is_empty(), "写前必须是**非空页**（否则下面的比对是恒真的）");

        for path in [
            ConsoleEndpoint::Logs.path(),
            ConsoleEndpoint::LogsTargets.path(),
        ] {
            for m in ["POST", "PUT", "DELETE", "PATCH"] {
                let (s, _) = http(addr, m, path, (m == "POST").then_some("{}")).await;
                assert_eq!(s, 405, "{m} {path} 必须 405（只读端点），实际 {s}");
            }
        }

        // 只读的**实测**证据：8 次写尝试后，两个端点读到的内容与写前**逐字段一致**（无副作用）
        let (_, after) = logs_from(addr, &qs).await;
        assert_eq!(after, before, "`/logs` 的整页（含 entries 五个字段）必须与写前逐字段一致");
        let (_, after_targets) = http(addr, "GET", ConsoleEndpoint::LogsTargets.path(), None).await;
        assert_eq!(after_targets, before_targets, "`/logs/targets` 的裸 Vec 必须与写前一致");
        h.abort();
    }

    /// ⑥ 非法查询 ⇒ **400**（非 2xx）；错误体**不是**可解析的 `LogPage`（渲染端不解析错误体，
    /// 一旦它是合法 DTO，屏上就可能把"参数错"读成"无日志"）。
    #[tokio::test]
    async fn illegal_query_is_400_with_a_non_dto_body() {
        let t = crate::testutil::TempDir::new("h-400");
        log_fixture(&t, 2);
        let svc = crate::log_service::LogService::new(
            t.path(),
            mupc_display_proto::config::LogLimits::default(),
        );
        let (addr, h) = spawn_log_host(LogSource::Ready(Arc::new(svc))).await;
        for qs in [
            "/v1/console/logs?range=1h&limit=201",
            "/v1/console/logs?range=1h&limit=0",
            "/v1/console/logs?range=1h&nope=1",
            "/v1/console/logs?range=1h&from=1",
            "/v1/console/logs?range=custom&from=1",
            "/v1/console/logs?range=custom&from=9&to=1",
            "/v1/console/logs?range=1h&levels=fatal",
        ] {
            let (s, b) = http(addr, "GET", qs, None).await;
            assert_eq!(s, 400, "{qs} 应 400，实际 {s}: {b}");
            assert!(serde_json::from_str::<LogPage>(&b).is_err(), "{qs} 错误体: {b}");
        }
        h.abort();
    }

    /// ⑥' 百分号编码的**多字节 / 保留字符** target 必须原样解出（渲染端 `encode_query`
    /// 把 `核间/gateway` 编成 `%E6%A0%B8%E9%97%B4%2Fgateway`；服务端解错 = 模块筛选静默失效）。
    #[tokio::test]
    async fn percent_encoded_multi_byte_target_is_decoded_to_the_raw_key() {
        let t = crate::testutil::TempDir::new("h-pct");
        let base = 1_757_412_000_000u64;
        let ts = chrono::DateTime::from_timestamp_millis(base as i64)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut body = String::new();
        for target in ["核间/gateway", "mupc_gateway"] {
            body.push_str(
                &serde_json::json!({
                    "timestamp": ts, "level": "INFO", "target": target,
                    "fields": { "message": "m" },
                })
                .to_string(),
            );
            body.push('\n');
        }
        t.write("mupc.log.2025-09-09", &body);
        let svc = crate::log_service::LogService::new(
            t.path(),
            mupc_display_proto::config::LogLimits::default(),
        );
        let (addr, h) = spawn_log_host(LogSource::Ready(Arc::new(svc))).await;

        // `%2F`（`/`）与 UTF-8 字节序列都必须被解码；`+` 不被当作空格（渲染端把 `+` 编成 `%2B`）
        let qs = log_qs(
            "custom",
            &format!(
                "&from={base}&to={}&limit=20&targets=%E6%A0%B8%E9%97%B4%2Fgateway",
                base + 1000
            ),
        );
        let (status, page) = logs_from(addr, &qs).await;
        assert_eq!(status, 200);
        assert_eq!(page.entries.len(), 1, "只命中那个多字节 target");
        assert_eq!(page.entries[0].target, "核间/gateway");

        // `/logs/targets` 回的也是**原始键**（不是编码形态）
        let (_, b) = http(addr, "GET", ConsoleEndpoint::LogsTargets.path(), None).await;
        let targets: Vec<String> = serde_json::from_str(&b).unwrap();
        assert_eq!(targets, vec!["mupc_gateway".to_string(), "核间/gateway".to_string()]);
        h.abort();
    }

    /// ⑦ `LogPage` 的三条**线上字段**一个都不能少（契约 `Critical 2`：缺失 ⇒ 渲染端整帧 `Err`）。
    /// 本用例把"服务端确实发了全部四个字段"钉死（不是靠 proto 的 derive 保证）。
    #[tokio::test]
    async fn wire_json_always_carries_all_four_logpage_fields() {
        let t = crate::testutil::TempDir::new("h-wire");
        let base = log_fixture(&t, 1);
        let svc = crate::log_service::LogService::new(
            t.path(),
            mupc_display_proto::config::LogLimits::default(),
        );
        let (addr, h) = spawn_log_host(LogSource::Ready(Arc::new(svc))).await;
        let qs = log_qs("custom", &format!("&from={base}&to={}&limit=20", base + 1000));
        let (_, body) = http(addr, "GET", &qs, None).await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        for k in ["entries", "next_cursor", "has_more", "range_too_large"] {
            assert!(v.get(k).is_some(), "回包缺 `{k}`（渲染端会整帧 Err）: {body}");
        }
        // 单条日志的五个字段同样齐全（`LogEntry`）
        let e = &v["entries"][0];
        for k in ["seq", "ts_ms", "level", "target", "message"] {
            assert!(e.get(k).is_some(), "条目缺 `{k}`: {body}");
        }
        h.abort();
    }

    /// ⑧ 单条消息超 1 KiB ⇒ **截断且标注可见**（跨侧约定；渲染端 `MAX_BODY_BYTES` 按此推算）。
    #[tokio::test]
    async fn over_long_message_is_truncated_and_the_marker_is_visible_on_the_wire() {
        let t = crate::testutil::TempDir::new("h-trunc");
        let long = "x".repeat(4096);
        let ts = chrono::DateTime::from_timestamp_millis(1_757_412_000_000)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        t.write(
            "mupc.log.2025-09-09",
            &format!(
                "{}\n",
                serde_json::json!({
                    "timestamp": ts, "level": "INFO", "target": "mupc_gateway",
                    "fields": { "message": long },
                })
            ),
        );
        let svc = crate::log_service::LogService::new(
            t.path(),
            mupc_display_proto::config::LogLimits::default(),
        );
        let (addr, h) = spawn_log_host(LogSource::Ready(Arc::new(svc))).await;
        let qs = log_qs("custom", "&from=1757412000000&to=1757412001000&limit=20");
        let (status, page) = logs_from(addr, &qs).await;
        assert_eq!(status, 200);
        assert_eq!(page.entries.len(), 1, "夹具恰好一条");
        let m = &page.entries[0].message;
        assert!(m.len() <= crate::log_service::MESSAGE_MAX_BYTES, "超长必须截断: {}", m.len());
        assert!(m.ends_with(crate::log_service::TRUNCATION_MARKER), "截断必须可见");
        h.abort();
    }

    /// ⑨ `/logs/targets` 的选项 ≤ [`mupc_display_proto::log::LOG_TARGETS_MAX`]（契约硬上限），
    /// 且**在整条链路上**（HTTP → 裸 `Vec<String>`）保持。
    #[tokio::test]
    async fn log_targets_endpoint_respects_the_contract_cap_over_the_wire() {
        let t = crate::testutil::TempDir::new("h-targets");
        let ts = chrono::DateTime::from_timestamp_millis(1_757_412_000_000)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut body = String::new();
        for i in 0..80 {
            body.push_str(
                &serde_json::json!({
                    "timestamp": ts, "level": "INFO", "target": format!("mod{i:03}"),
                    "fields": { "message": "m" },
                })
                .to_string(),
            );
            body.push('\n');
        }
        t.write("mupc.log.2025-09-09", &body);
        let svc = crate::log_service::LogService::new(
            t.path(),
            mupc_display_proto::config::LogLimits::default(),
        );
        let (addr, h) = spawn_log_host(LogSource::Ready(Arc::new(svc))).await;
        let (s, b) = http(addr, "GET", ConsoleEndpoint::LogsTargets.path(), None).await;
        assert_eq!(s, 200);
        let out: Vec<String> = serde_json::from_str(&b).unwrap();
        assert_eq!(out.len(), mupc_display_proto::log::LOG_TARGETS_MAX);
        let mut sorted = out.clone();
        sorted.sort();
        assert_eq!(out, sorted, "字典序升序（稳定顺序）");
        h.abort();
    }
}
