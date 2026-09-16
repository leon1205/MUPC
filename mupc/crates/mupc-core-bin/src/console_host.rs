//! 控制通道宿主（`127.0.0.1:9811`）——开发单元 **G-1：控制通道宿主 + 配置读路径**。
//!
//! 对应设计（`docs/superpowers/plans/modules/12-MUPC-本地显示终端-设计文档.md`）：
//! - §3.3 控制通道：通用信封与管线（**本单元只实现读**）；
//! - §3.4 控制通道端点清单（8 条；本单元落 1 条：`GET /v1/console/config`）；
//! - §3.4 补注（2026-09-15）：**GET 返回裸 DTO、POST 走 `ControlResponse` 信封**；
//!   **GET 失败一律非 2xx**（渲染端 `console.rs` 落 `Error::HttpStatus`，不解析错误体）；
//!   **未实现的路由不得"假装成功"**（本模块对已登记但未实现的端点回 **501**，未知路径 **404**，
//!   方法不符 **405**——三者在 `router()` 里由「路径 → 方法」注册表结构性保证，非纪律要求）；
//! - §4.3.2 `ConfigFieldMeta` 静态表 / §4.3.3 ApplyMode 分发表（字段集与 `requires_reconnect` 的真源）；
//! - §4.9 启动装配（10.2 控制通道，与读通道 9810 **并存不冲突**：不同端口、不同 listener）。
//!
//! ## 本单元的范围与**未做**的部分（如实登记）
//!
//! - **只做读**：`GET /v1/console/config`。其余 7 条端点（`config/apply` / `logs` / `logs/targets`
//!   / `audit` / `audit/ops` / `interlock/release` / `interlock/ack_m1`）**已登记路由但返回 501**——
//!   它们各自的 `LogService` / `ConsoleAuditService` / `InterlockOps` / 写管线属后续单元（G-2…）。
//!   路由**不隐藏**：屏上对未实现端点的请求会得到明确的 501（渲染端 → `HttpStatus(501)` 失败提示），
//!   而不是被静默当成"服务不可用"或"空数据"。
//! - **不做写入**：`ConfigService` 的落盘 / 原子替换 / 幂等 / 审计全在 G-2；本单元**不读也不写 yaml**，
//!   视图的唯一数据源是 **`CoreConfig` 内存副本**（设计 D10：内存副本 + 原子落盘）。
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

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use mupc_display_proto::{
    ConfigField, ConfigGroup, ConfigKind, ConfigView, ConsoleEndpoint, OptionItem, WriteMode,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

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

/// 宿主依赖（设计 §4.9 `ConsoleDeps` 的最小可用子集；其余字段随 G-2… 引入）。
#[derive(Clone)]
pub struct ConsoleDeps {
    /// 配置读源（本单元**唯一**依赖）。
    pub config: ConfigSource,
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
                mupc_display_proto::ConsoleMethod::Get => router.route(path, get(not_implemented)),
                mupc_display_proto::ConsoleMethod::Post => {
                    router.route(path, axum::routing::post(not_implemented))
                }
            };
        }
        router.with_state(HostState {
            config: self.deps.config.clone(),
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
            "本地显示终端控制通道已启动: http://{}{}（回环仅本机；GET {} 已实现，其余端点 501）",
            addr,
            ConsoleEndpoint::Config.path(),
            ConsoleEndpoint::Config.path()
        );
        axum::serve(listener, self.router()).await
    }
}

/// 宿主状态（axum `State`）。
#[derive(Clone)]
struct HostState {
    config: ConfigSource,
}

// ═══════════════════════════════════════════════════════════════
// Handler
// ═══════════════════════════════════════════════════════════════

/// `GET /v1/console/config` → **裸 `ConfigView`**（§3.4 补注：GET 不走信封）。
///
/// 失败路径：配置源不可用 ⇒ **503**（非 2xx，渲染端落 `Error::HttpStatus`），
/// **绝不**回 `200` + 空视图冒充成功。
async fn get_config(State(st): State<HostState>) -> Response {
    match &st.config {
        ConfigSource::Ready(cfg) => {
            let guard = cfg.read().await;
            Json(config_view(&guard)).into_response()
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
    /// yaml 定位路径（§4.3.2.1）。本单元（只读）不消费，但由单测 `yaml_path_matches_key` 钉死，
    /// 供 G-2 的保留式编辑直接取用——**不得**在 G-2 另起一套路径串。
    #[allow(dead_code)]
    pub yaml_path: &'static str,
    /// 默认值（「恢复默认值」明细取此处）。
    pub default: fn() -> Value,
    /// 当前值提取（从 `CoreConfig` **内存副本**取；不读 yaml）。
    pub current: fn(&CoreConfig) -> Value,
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

/// `ConfigView.revision` 的初值：**本进程尚无任何成功写入**（写路径属 G-2）。
///
/// 契约语义是「每次成功写入递增」（`display-proto/src/control.rs:412`）——没有任何写入时取 0
/// 是其**唯一不臆造**的取值。G-2 接管后此常量应删除。
pub const REVISION_INITIAL: u64 = 0;

/// 由 `CoreConfig` **内存副本**生成视图（设计 §4.3.2：内存副本是"进程内唯一权威读源"）。
///
/// `write_mode` 取 `TextPreserve`：契约该字段的语义是「**最近一次落盘**的写模式」
/// （`display-proto/src/control.rs:412-413`），而本进程**尚无落盘** ⇒ 取正常路径值。
/// 这是**契约粗糙处**（`WriteMode` 不是 `Option`，无法表达"还没有过写入"），已登记为待裁定项；
/// 取 `TextPreserve` 的**风险**仅为"渲染端不弹 `full_rewrite` Toast"——而它本来也不该弹。
pub fn config_view(cfg: &CoreConfig) -> ConfigView {
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
        revision: REVISION_INITIAL,
        write_mode: WriteMode::TextPreserve,
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
    use std::net::SocketAddr;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // ── 测试脚手架 ──────────────────────────────────────────────────────────

    /// 起一个绑定在随机回环端口上的宿主，返回 (地址, JoinHandle)。
    async fn spawn_host(config: ConfigSource) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let host = ConsoleHost::new(ConsoleDeps { config });
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

    /// ① 未实现的 POST 路由 ⇒ **非 2xx**（不得假成功）。
    #[tokio::test]
    async fn unimplemented_write_endpoint_returns_non_2xx_not_fake_success() {
        let (addr, h) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(test_config())))).await;
        // 完整合法的信封体：即使请求本身合法，未实现也不得回 200
        let body = r#"{"request_id":"0f7a3e10-1111-4222-8333-444455556666","issued_at_ms":1,
            "op":"apply","payload":{"changes":{},"from":"edit"}}"#;
        let (status, resp) = http(addr, "POST", ConsoleEndpoint::ConfigApply.path(), Some(body)).await;
        assert_eq!(status, 501, "未实现端点须 501，实际 {status}: {resp}");
        assert!(
            serde_json::from_str::<mupc_display_proto::ControlResponse<ConfigView>>(&resp).is_err(),
            "未实现端点**不得**回一个 ControlResponse 信封冒充处理结果: {resp}"
        );
        h.abort();
    }

    /// ①' 其余 7 条端点：**逐条**非 2xx，且**逐条**不能回 404（404 = 路由没登记 = 屏上无法区分
    /// "服务没实现"与"服务根本没这个端点"）。
    #[tokio::test]
    async fn every_registered_but_unimplemented_endpoint_is_honest_per_endpoint() {
        let (addr, h) = spawn_host(ConfigSource::Ready(Arc::new(RwLock::new(test_config())))).await;
        for ep in ConsoleEndpoint::ALL {
            if ep == ConsoleEndpoint::Config {
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
        }
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
        let va = config_view(&a);
        let vb = config_view(&b);
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
}
