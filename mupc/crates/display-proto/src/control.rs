//! 控制通道（受控写接口）契约——设计 §3.3「通用信封与管线」+ §3.4「端点清单」。
//!
//! 语义要点：
//! - **统一信封**：所有 POST 共用 [`ControlRequest`]（`request_id` + `issued_at_ms` + `op` + `payload`），
//!   所有回执共用 [`ControlResponse`]（`ok` + [`ControlCode`] + `message` + `applied` +
//!   `field_errors` + `audit_id` + `duplicate`）。
//! - **幂等**：服务端按 [`IdempotencyKey`]（`(op, request_id)`）去重；命中且已完成 → 返回首次
//!   执行的原始回执（`duplicate=true`，`ok` 不变）；命中且处理中 → [`ControlCode::Busy`]。
//! - **防重放**：`|now − issued_at_ms| > `[`REPLAY_WINDOW_MS`] 即 [`ControlEnvelopeError::OutsideReplayWindow`]。
//! - **审计 fail-closed**：审计是唯一操作凭据（T-3 无登录），审计不可写 → **拒绝执行**，
//!   回 [`ControlResponse::audit_unavailable`]（[`ControlCode::AuditUnavailable`]，`ok=false`，
//!   `applied=None`）。设计 §3.3 / EDGE-18。
//! - 本模块**不含**任何 IO / 时钟 / 存储实现，仅契约与判据（两侧共用、可纯单测）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 重放窗口（设计 §3.3：服务端拒绝 `|now − issued_at| > 30_000` 的请求）。
pub const REPLAY_WINDOW_MS: u64 = 30_000;

/// 幂等表容量（设计 §3.3：30 s TTL 的有界 LRU，容量 256）。
pub const IDEMPOTENCY_CAPACITY: usize = 256;

/// 幂等表条目存活时长 / TTL（ms；设计 §3.3：30 s TTL 的有界 LRU）。
///
/// 与 [`REPLAY_WINDOW_MS`] **数值相同但语义不同**：前者是「同 `(op, request_id)` 请求在
/// 多久内视为重复」，后者是「请求签发时刻距服务端现在可允许多久」。二者各自为单一真源，
/// 实现者**不得**因「反正都是 30_000」而复用 [`REPLAY_WINDOW_MS`] 而把两者绑死
/// （将来任一被产品调整时，复用会静默改变另一处语义）。
pub const IDEMPOTENCY_TTL_MS: u64 = 30_000;

/// 审计 / 回执固定的操作者标识（T-3：无登录，operator 恒为 `local-console`）。
pub const CONSOLE_OPERATOR: &str = "local-console";

/// 控制通道路径前缀（设计 §3.3：全部为 `/v1/console/*`）。
pub const CONSOLE_PATH_PREFIX: &str = "/v1/console";

/// 审计 fail-closed 裁决（设计 §3.3 / EDGE-18）：`true` = 审计不可写则**拒绝执行**写操作。
///
/// 契约层显式暴露该裁决，使「fail-closed 是默认立场」不依赖各侧实现者的记忆；
/// 若未来 PM 裁定联锁释放等恢复安全态操作改为 fail-open，改动应发生在本常量与
/// [`ControlResponse::audit_unavailable`] 的调用点，而非散落各 handler。
pub const AUDIT_FAIL_CLOSED: bool = true;

/// 控制通道 HTTP 方法约定（设计 §3.3：查询用 GET，写操作用 POST）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleMethod {
    /// 查询（参数在 query）。
    Get,
    /// 写操作（JSON body）。
    Post,
}

/// 控制通道端点枚举（设计 §3.4 端点清单的机器可读形态）。
///
/// 契约层固化「路径 ↔ 方法 ↔ 写操作名」三元组，杜绝两侧各自拼串导致的路由漂移；
/// `op_name()` 即信封 `op` 字段的**唯一合法取值**（设计 §3.3：与路径末段一致，服务端校验，防误路由）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleEndpoint {
    /// `GET /v1/console/config` → `ConfigView`。
    Config,
    /// `POST /v1/console/config/apply` → `ConfigPatch` → `ConfigView`。
    ConfigApply,
    /// `GET /v1/console/logs` → `LogPage`。
    Logs,
    /// `GET /v1/console/logs/targets` → `Vec<String>`（模块选项，≤50）。
    LogsTargets,
    /// `GET /v1/console/audit` → `AuditPage`。
    Audit,
    /// `GET /v1/console/audit/ops` → `Vec<OpOption>`（操作类型选项）。
    AuditOps,
    /// `POST /v1/console/interlock/release` → `InterlockOpPayload` → `InterlockOpAck`。
    InterlockRelease,
    /// `POST /v1/console/interlock/ack_m1` → `InterlockOpPayload` → `InterlockOpAck`。
    InterlockAckM1,
}

impl ConsoleEndpoint {
    /// 全部端点（§3.4 清单，共 8 条；其中 3 条为写）。
    pub const ALL: [ConsoleEndpoint; 8] = [
        Self::Config,
        Self::ConfigApply,
        Self::Logs,
        Self::LogsTargets,
        Self::Audit,
        Self::AuditOps,
        Self::InterlockRelease,
        Self::InterlockAckM1,
    ];

    /// 请求路径。
    pub fn path(self) -> &'static str {
        match self {
            Self::Config => "/v1/console/config",
            Self::ConfigApply => "/v1/console/config/apply",
            Self::Logs => "/v1/console/logs",
            Self::LogsTargets => "/v1/console/logs/targets",
            Self::Audit => "/v1/console/audit",
            Self::AuditOps => "/v1/console/audit/ops",
            Self::InterlockRelease => "/v1/console/interlock/release",
            Self::InterlockAckM1 => "/v1/console/interlock/ack_m1",
        }
    }

    /// HTTP 方法。
    pub fn method(self) -> ConsoleMethod {
        match self {
            Self::Config | Self::Logs | Self::LogsTargets | Self::Audit | Self::AuditOps => {
                ConsoleMethod::Get
            }
            Self::ConfigApply | Self::InterlockRelease | Self::InterlockAckM1 => ConsoleMethod::Post,
        }
    }

    /// 写操作名 = 路径末段（GET 查询端点无信封 `op`，返回 `None`）。
    pub fn op_name(self) -> Option<&'static str> {
        match self.method() {
            ConsoleMethod::Get => None,
            ConsoleMethod::Post => self.path().rsplit('/').next(),
        }
    }

    /// 是否为写端点（POST）。
    pub fn is_write(self) -> bool {
        self.method() == ConsoleMethod::Post
    }

    /// 由路径反查端点（服务端路由用）。
    pub fn from_path(path: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.path() == path)
    }
}

/// 信封校验失败（设计 §3.3 管线第 2 步）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ControlEnvelopeError {
    /// `request_id` 为空 → 无法幂等去重 / 无法对拍审计，必须拒绝。
    #[error("control request_id must not be empty")]
    EmptyRequestId,

    /// `op` 与路由端点不匹配（防误路由；设计 §3.3）。
    #[error("control op `{op}` does not match endpoint `{expected}`")]
    OpMismatch {
        /// 客户端声明的 op。
        op: String,
        /// 端点要求的 op（路径末段）。
        expected: &'static str,
    },

    /// 请求落在重放窗口之外（设计 §3.3：`|now − issued_at| > 30 s`）。
    #[error(
        "control request issued_at_ms {issued_at_ms} outside replay window \
         {window_ms} ms (now {now_ms})"
    )]
    OutsideReplayWindow {
        /// 客户端签发时刻。
        issued_at_ms: u64,
        /// 服务端当前时刻。
        now_ms: u64,
        /// 允许窗口。
        window_ms: u64,
    },

    /// 对查询端点（GET）提交了写信封。
    #[error("endpoint `{0:?}` is a query endpoint and carries no control envelope")]
    NotAWriteEndpoint(ConsoleEndpoint),
}

/// 幂等表的键：`(op, request_id)`（设计 §3.3 管线第 3 步）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey {
    /// 操作名（路径末段）。
    pub op: String,
    /// 客户端生成的 UUID v4。
    pub request_id: String,
}

impl IdempotencyKey {
    /// 构造。
    pub fn new(op: impl Into<String>, request_id: impl Into<String>) -> Self {
        Self {
            op: op.into(),
            request_id: request_id.into(),
        }
    }
}

/// 写操作请求信封（所有 POST 共用；设计 §3.3）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlRequest<T> {
    /// 客户端生成的 UUID v4；服务端按 `(op, request_id)` 做幂等去重（防重放 / 防重复生效）。
    pub request_id: String,
    /// 客户端签发时刻（Unix ms）；服务端拒绝 `|now − issued_at| > 30_000` 的请求。
    pub issued_at_ms: u64,
    /// 操作名（与路径末段一致，服务端校验，防误路由）。
    pub op: String,
    /// 操作负载。
    pub payload: T,
}

impl<T> ControlRequest<T> {
    /// 构造。
    pub fn new(request_id: impl Into<String>, issued_at_ms: u64, op: impl Into<String>, payload: T) -> Self {
        Self {
            request_id: request_id.into(),
            issued_at_ms,
            op: op.into(),
            payload,
        }
    }

    /// 幂等键。
    pub fn idempotency_key(&self) -> IdempotencyKey {
        IdempotencyKey::new(self.op.clone(), self.request_id.clone())
    }

    /// 信封校验（设计 §3.3 管线第 2 步）：`request_id` 非空 + 时间窗内。
    pub fn validate_envelope(&self, now_ms: u64) -> std::result::Result<(), ControlEnvelopeError> {
        if self.request_id.trim().is_empty() {
            return Err(ControlEnvelopeError::EmptyRequestId);
        }
        if self.issued_at_ms.abs_diff(now_ms) > REPLAY_WINDOW_MS {
            return Err(ControlEnvelopeError::OutsideReplayWindow {
                issued_at_ms: self.issued_at_ms,
                now_ms,
                window_ms: REPLAY_WINDOW_MS,
            });
        }
        Ok(())
    }

    /// 信封 + 路由校验：`op` 必须等于目标端点路径末段（防误路由）。
    pub fn validate_for(
        &self,
        endpoint: ConsoleEndpoint,
        now_ms: u64,
    ) -> std::result::Result<(), ControlEnvelopeError> {
        self.validate_envelope(now_ms)?;
        match endpoint.op_name() {
            Some(expected) if expected == self.op => Ok(()),
            Some(expected) => Err(ControlEnvelopeError::OpMismatch {
                op: self.op.clone(),
                expected,
            }),
            None => Err(ControlEnvelopeError::NotAWriteEndpoint(endpoint)),
        }
    }
}

/// 字段级校验错误（配置页逐字段标红用，CF-02 要求「具体错误」；设计 §3.3）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldError {
    /// 稳定字段键（如 `intercore.port`）。
    pub field: String,
    /// 具体原因（UI 就地展示）。
    pub reason: String,
}

/// 控制回执错误码（设计 §3.3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlCode {
    /// 成功。
    Ok,
    /// 前置条件不满足（联锁触发源未复位 / 保持时间不足 / 处于 latch 态）→ 必须明示原因（EDGE-12）。
    RejectedPrecondition,
    /// 参数校验失败（越界 / 非法枚举）→ 逐字段原因（EDGE-10）。
    RejectedValidation,
    /// 执行 / 落盘 / 生效失败（EDGE-10：装置须保持原配置运行，不得半生效）。
    ApplyFailed,
    /// 审计不可写（fail-closed，见 [`AUDIT_FAIL_CLOSED`]）。
    AuditUnavailable,
    /// 后端不可用（联锁未启用 / 日志或审计源不可用）。
    Unavailable,
    /// 同 `request_id` 请求仍在处理中。
    Busy,
    /// 内部错误。
    Internal,
}

impl ControlCode {
    /// 是否成功。
    pub fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }

    /// 是否为「拒绝类」（未执行；装置维持原状）。
    pub fn is_rejection(self) -> bool {
        matches!(
            self,
            Self::RejectedPrecondition
                | Self::RejectedValidation
                | Self::ApplyFailed
                | Self::AuditUnavailable
                | Self::Unavailable
                | Self::Busy
                | Self::Internal
        )
    }
}

/// 回执（所有写操作共用；设计 §3.3）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlResponse<T> {
    /// 回显请求 `request_id`。
    pub request_id: String,
    /// 是否成功（真值判据；`duplicate` 命中时保持首次结果）。
    pub ok: bool,
    /// 结构化错误码。
    pub code: ControlCode,
    /// 人读消息；UI 直接展示（失败时即 EDGE-10 / EDGE-12 要求的「具体原因」）。
    pub message: String,
    /// 成功时的生效回执（新值 / 新状态），供 UI 立即刷新（不等下一帧）。
    pub applied: Option<T>,
    /// 字段级校验错误（配置页逐字段标红用）。
    ///
    /// **必需字段**（Important 4）：无 `#[serde(default)]`——缺省即整帧 `Err`。
    /// 若默认成空列表，后端明明校验失败却无逐字段原因，UI 无从标红（CF-02 落空）。
    pub field_errors: Vec<FieldError>,
    /// 审计记录 ID（成功与失败均返回；便于现场对拍）。
    pub audit_id: Option<String>,
    /// 幂等命中标记：`true` 表示本条为重复请求，返回的是首次执行的**原始结果**。
    ///
    /// **必需字段**（Important 4）：无 `#[serde(default)]`——缺省即整帧 `Err`。
    /// 若默认成 `false`，重复请求会被当作首次处理，UI 的幂等命中提示丢失。
    pub duplicate: bool,
    /// 服务端回执时刻（Unix ms）。
    pub at_ms: u64,
}

impl<T> ControlResponse<T> {
    /// 成功回执。
    pub fn ok(
        request_id: impl Into<String>,
        applied: Option<T>,
        audit_id: Option<String>,
        at_ms: u64,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            ok: true,
            code: ControlCode::Ok,
            message: "操作成功".to_string(),
            applied,
            field_errors: Vec::new(),
            audit_id,
            duplicate: false,
            at_ms,
        }
    }

    /// 失败回执（`field_errors` 供配置页逐字段标红）。
    pub fn rejected(
        request_id: impl Into<String>,
        code: ControlCode,
        message: impl Into<String>,
        field_errors: Vec<FieldError>,
        audit_id: Option<String>,
        at_ms: u64,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            ok: false,
            code,
            message: message.into(),
            applied: None,
            field_errors,
            audit_id,
            duplicate: false,
            at_ms,
        }
    }

    /// **审计不可写 → 拒绝执行**（fail-closed，设计 §3.3 / EDGE-18）。
    ///
    /// 这是 fail-closed 裁决在契约层的唯一落点：`ok=false`、`code=AuditUnavailable`、
    /// `applied=None`（**操作未生效**）。调用方（ConsoleHost）在审计占位写入失败时必须
    /// **短路执行**并返回本条，而非「尽力执行 + 记录失败」。
    pub fn audit_unavailable(request_id: impl Into<String>, at_ms: u64) -> Self {
        Self {
            request_id: request_id.into(),
            ok: false,
            code: ControlCode::AuditUnavailable,
            message: "审计不可写，已拒绝执行写操作（装置维持原配置运行）".to_string(),
            applied: None,
            field_errors: Vec::new(),
            audit_id: None,
            duplicate: false,
            at_ms,
        }
    }

    /// 标记为幂等命中：返回首次执行的原始结果，**`ok` 与 `code` 不变**（设计 §3.3 管线第 3 步）。
    pub fn mark_duplicate(&mut self) {
        self.duplicate = true;
    }
}

// ═══════════════════════════════════════════════════════════════
// 配置（F9）—— 字段元数据驱动 UI，后端字段表为「零键盘」的类型约束来源
// ═══════════════════════════════════════════════════════════════

/// 配置页视图（设计 §3.4）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigView {
    /// 分组（IEC 104 / 核间 / 遥测与日志…）。
    pub groups: Vec<ConfigGroup>,
    /// 配置版本号（每次成功写入递增）。
    pub revision: u64,
    /// 最近一次落盘的写模式（`FullRewrite` 时 UI 须 Toast 明示注释丢失，EDGE-23）。
    pub write_mode: WriteMode,
}

/// yaml 回写模式（设计 §4.3.2.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteMode {
    /// 保留式编辑（正常路径；注释 / 未建模键未动）。
    TextPreserve,
    /// 无法定位目标键 → 整体序列化回写（**既有注释已丢失**，UI 须明示）。
    FullRewrite,
}

/// 配置分组。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigGroup {
    /// 分组 ID。
    pub id: String,
    /// 中文分组标签。
    pub label: String,
    /// 组内字段。
    pub fields: Vec<ConfigField>,
}

/// 配置字段元数据（**驱动 UI 控件生成**，使 UI 无需硬编码；设计 §3.4）。
///
/// 「零键盘」由本结构的 `kind` **类型层面**保证：字段表里不存在「自由文本」这一 kind，
/// UI 因此**无处可放文本输入框**（设计 §3.4 / D9）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigField {
    /// 稳定键（如 `system.log_level`）。
    pub key: String,
    /// 中文标签。
    pub label: String,
    /// 控件类型（驱动 UI 控件选择）。
    pub kind: ConfigKind,
    /// 当前值。
    pub value: Value,
    /// 默认值（「恢复默认值」明细取此处）。
    pub default: Value,
    /// 单位；`None` = 无单位。
    pub unit: Option<String>,
    /// `true` → 弹层须提示「生效时链路将短暂中断」。
    pub requires_reconnect: bool,
    /// `false` → UI 只读（控件 disabled）。用于 `display.bind_addr` / `display.control_bind_addr`：
    /// 回环是安全红线（PL-4），不可经屏修改（设计 §3.4 / §6.2）。
    pub editable: bool,
}

impl ConfigField {
    /// 后端二次校验（设计 §3.3 管线第 4 步 / §4.3.2 ①）。
    ///
    /// 拒绝只读字段改动与越界 / 非法值（返回**具体原因**，供 `field_errors` 逐字段标红）。
    /// 注意：UI 控件层已使越界不可达（TT-03），此处是**独立**的第二道校验——
    /// 不依赖前端（设计 §6.2「后端二次校验」）。
    pub fn validate_value(&self, value: &Value) -> std::result::Result<(), String> {
        if !self.editable {
            return Err(format!("字段 `{}` 为只读，不可修改", self.key));
        }
        self.kind.validate_value(value)
    }
}

/// 字段控件类型（设计 §3.4；**无「自由文本」variant**——零键盘的类型层约束）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigKind {
    /// IPv4 地址（四段受约束步进，每段 0–255）。
    Ipv4,
    /// `u16` 受约束步进器。
    U16 {
        /// 下限。
        min: u16,
        /// 上限。
        max: u16,
        /// 步长。
        step: u16,
    },
    /// `u64` 受约束步进器。
    U64 {
        /// 下限。
        min: u64,
        /// 上限。
        max: u64,
        /// 步长。
        step: u64,
    },
    /// 选项列表（`lv_dropdown` / `lv_buttonmatrix`）。
    Enum {
        /// 可选项。
        options: Vec<OptionItem>,
    },
}

impl ConfigKind {
    /// 值域校验；`Err(reason)` 为 UI 可展示的具体原因。
    pub fn validate_value(&self, value: &Value) -> std::result::Result<(), String> {
        match self {
            Self::Ipv4 => {
                let s = value
                    .as_str()
                    .ok_or_else(|| "IPv4 字段应为字符串".to_string())?;
                s.parse::<std::net::Ipv4Addr>()
                    .map(|_| ())
                    .map_err(|_| format!("`{s}` 不是合法 IPv4 地址"))
            }
            Self::U16 { min, max, step } => {
                let n = value.as_u64().ok_or_else(|| "应为非负整数".to_string())?;
                if n > u16::MAX as u64 {
                    return Err(format!("{n} 超出 u16 上限 {}", u16::MAX));
                }
                let n = n as u16;
                if n < *min || n > *max {
                    return Err(format!("{n} 越界，允许区间 [{min}, {max}]"));
                }
                if *step != 0 && (n - *min) % *step != 0 {
                    return Err(format!("{n} 非步长 {step} 的整数倍"));
                }
                Ok(())
            }
            Self::U64 { min, max, step } => {
                let n = value.as_u64().ok_or_else(|| "应为非负整数".to_string())?;
                if n < *min || n > *max {
                    return Err(format!("{n} 越界，允许区间 [{min}, {max}]"));
                }
                if *step != 0 && (n - *min) % *step != 0 {
                    return Err(format!("{n} 非步长 {step} 的整数倍"));
                }
                Ok(())
            }
            Self::Enum { options } => {
                let s = value
                    .as_str()
                    .ok_or_else(|| "枚举字段应为字符串".to_string())?;
                if options.iter().any(|o| o.value == s) {
                    Ok(())
                } else {
                    Err(format!("`{s}` 不在允许选项内"))
                }
            }
        }
    }
}

/// 枚举选项（值 + 中文标签）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionItem {
    /// 机器值。
    pub value: String,
    /// 中文标签。
    pub label: String,
}

/// 配置写入负载（`POST /v1/console/config/apply`；设计 §3.4）。
///
/// `changes` 为 `key → 新值` 映射；**全字段默认值**即「恢复默认值」的载荷。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigPatch {
    /// 待改字段（key → 新值）——逐字段键为审计 `target`。
    pub changes: serde_json::Map<String, Value>,
    /// 写入来源（`edit` = 编辑保存 / `reset_default` = 恢复默认值）。
    pub from: PatchSource,
}

/// 配置写入来源（设计 §3.4：`from: "edit"|"reset_default"`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchSource {
    /// 编辑保存。
    Edit,
    /// 恢复默认值。
    ResetDefault,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(op: &str, issued_at_ms: u64) -> ControlRequest<ConfigPatch> {
        ControlRequest::new(
            "0f7a3e10-1111-4222-8333-444455556666",
            issued_at_ms,
            op,
            ConfigPatch {
                changes: serde_json::Map::new(),
                from: PatchSource::Edit,
            },
        )
    }

    // ---- 端点清单（§3.4）：路径 / 方法 / op 三元组 ----
    #[test]
    fn endpoint_paths_and_methods_match_design() {
        let cases = [
            (ConsoleEndpoint::Config, "/v1/console/config", ConsoleMethod::Get, None),
            (ConsoleEndpoint::ConfigApply, "/v1/console/config/apply", ConsoleMethod::Post, Some("apply")),
            (ConsoleEndpoint::Logs, "/v1/console/logs", ConsoleMethod::Get, None),
            (ConsoleEndpoint::LogsTargets, "/v1/console/logs/targets", ConsoleMethod::Get, None),
            (ConsoleEndpoint::Audit, "/v1/console/audit", ConsoleMethod::Get, None),
            (ConsoleEndpoint::AuditOps, "/v1/console/audit/ops", ConsoleMethod::Get, None),
            (ConsoleEndpoint::InterlockRelease, "/v1/console/interlock/release", ConsoleMethod::Post, Some("release")),
            (ConsoleEndpoint::InterlockAckM1, "/v1/console/interlock/ack_m1", ConsoleMethod::Post, Some("ack_m1")),
        ];
        assert_eq!(cases.len(), ConsoleEndpoint::ALL.len());
        for (ep, path, method, op) in cases {
            assert_eq!(ep.path(), path, "路径漂移");
            assert_eq!(ep.method(), method, "方法漂移");
            assert_eq!(ep.op_name(), op, "op 名（= 路径末段）漂移");
            assert!(path.starts_with(CONSOLE_PATH_PREFIX), "端点须在 /v1/console 下");
            assert_eq!(ConsoleEndpoint::from_path(path), Some(ep), "路径反查失败");
        }
        assert!(!ConsoleEndpoint::Config.is_write(), "查询端点非写");
        assert!(ConsoleEndpoint::InterlockRelease.is_write());
    }

    // ---- 信封：字面量 JSON 往返 + 关键字段断言 ----
    #[test]
    fn control_request_literal_json_roundtrip() {
        let json = r#"{
            "request_id": "0f7a3e10-1111-4222-8333-444455556666",
            "issued_at_ms": 1757412000000,
            "op": "apply",
            "payload": {
                "changes": {"system.log_level": "debug"},
                "from": "reset_default"
            }
        }"#;
        let req: ControlRequest<ConfigPatch> = serde_json::from_str(json).unwrap();
        assert_eq!(req.op, "apply");
        assert_eq!(req.issued_at_ms, 1757412000000);
        assert_eq!(req.payload.from, PatchSource::ResetDefault);
        assert_eq!(
            req.payload.changes.get("system.log_level"),
            Some(&Value::String("debug".to_string()))
        );
        // 编码后回解码稳定
        let back: ControlRequest<ConfigPatch> =
            serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn control_response_literal_json_and_code_vocabulary() {
        let json = r#"{
            "request_id": "0f7a3e10-1111-4222-8333-444455556666",
            "ok": false,
            "code": "rejected_validation",
            "message": "字段 `intercore.port` 越界，允许区间 [1, 65535]",
            "applied": null,
            "field_errors": [{"field":"intercore.port","reason":"越界，允许区间 [1, 65535]"}],
            "audit_id": "a1b2c3",
            "duplicate": false,
            "at_ms": 1757412000123
        }"#;
        let resp: ControlResponse<ConfigView> = serde_json::from_str(json).unwrap();
        assert!(!resp.ok);
        assert_eq!(resp.code, ControlCode::RejectedValidation);
        assert!(resp.code.is_rejection() && !resp.code.is_ok());
        assert_eq!(resp.applied, None, "拒绝回执不得带生效值");
        assert_eq!(resp.field_errors.len(), 1);
        assert_eq!(resp.field_errors[0].field, "intercore.port");
        assert_eq!(resp.audit_id.as_deref(), Some("a1b2c3"));
        assert!(!resp.duplicate);
        // 错误码 JSON 词表（两端共用，防拼写漂移）
        for (code, literal) in [
            (ControlCode::Ok, "\"ok\""),
            (ControlCode::RejectedPrecondition, "\"rejected_precondition\""),
            (ControlCode::RejectedValidation, "\"rejected_validation\""),
            (ControlCode::ApplyFailed, "\"apply_failed\""),
            (ControlCode::AuditUnavailable, "\"audit_unavailable\""),
            (ControlCode::Unavailable, "\"unavailable\""),
            (ControlCode::Busy, "\"busy\""),
            (ControlCode::Internal, "\"internal\""),
        ] {
            assert_eq!(serde_json::to_string(&code).unwrap(), literal);
        }
    }

    /// Important 4：`field_errors` / `duplicate` **缺省不得静默取默认**（字面量 JSON 反例）。
    ///
    /// 修复前二者带 `#[serde(default)]`：缺 `field_errors` → UI 无逐字段标红（CF-02 落空）；
    /// 缺 `duplicate` → 幂等命中被当作首次（提示丢失）。
    #[test]
    fn response_missing_semantic_fields_fail_loudly() {
        // 缺 field_errors（修复前 → 静默空列表）
        let no_field_errors = r#"{"request_id":"r1","ok":false,"code":"rejected_validation",
            "message":"m","applied":null,"audit_id":null,"duplicate":false,"at_ms":1}"#;
        // 缺 duplicate（修复前 → 静默 false，按「非重复」处理）
        let no_duplicate = r#"{"request_id":"r1","ok":true,"code":"ok","message":"m",
            "applied":null,"field_errors":[],"audit_id":null,"at_ms":1}"#;
        // 二者皆缺
        let neither = r#"{"request_id":"r1","ok":true,"code":"ok","message":"m",
            "applied":null,"audit_id":null,"at_ms":1}"#;
        for json in [no_field_errors, no_duplicate, neither] {
            let res = serde_json::from_str::<ControlResponse<ConfigView>>(json);
            assert!(
                res.is_err(),
                "缺少关键语义字段必须 Err（不得默认为空 / false），实际: {res:?}"
            );
        }
        // 正例：字段齐备 → Ok，且逐字段原因与幂等标记可读出
        let ok: ControlResponse<ConfigView> = serde_json::from_str(
            r#"{"request_id":"r1","ok":false,"code":"rejected_validation","message":"m",
                "applied":null,"field_errors":[{"field":"a.b","reason":"越界"}],
                "audit_id":null,"duplicate":true,"at_ms":1}"#,
        )
        .unwrap();
        assert_eq!(ok.field_errors[0].field, "a.b");
        assert!(ok.duplicate);
    }

    /// Minor 8：同一类型两套词表（serde 名 `config_apply` vs 线上 `op="apply"`）须被测试钉死，
    /// 并说明二者是**不同概念**（枚举判别名 vs 路径末段），防止将来有人「顺手统一」而改坏线格式。
    #[test]
    fn endpoint_serde_name_and_wire_op_name_mapping_is_pinned() {
        for (ep, serde_literal, wire_op) in [
            (ConsoleEndpoint::ConfigApply, "\"config_apply\"", "apply"),
            (
                ConsoleEndpoint::InterlockRelease,
                "\"interlock_release\"",
                "release",
            ),
            (ConsoleEndpoint::InterlockAckM1, "\"interlock_ack_m1\"", "ack_m1"),
        ] {
            // serde 名（枚举判别名，用于路由枚举本体）
            assert_eq!(serde_json::to_string(&ep).unwrap(), serde_literal);
            // 线格式 op = 路径末段（信封 `op` 字段的唯一合法取值）
            assert_eq!(ep.op_name(), Some(wire_op));
            assert_eq!(ep.path().rsplit('/').next(), Some(wire_op));
            assert_ne!(
                serde_literal.trim_matches('"'),
                wire_op,
                "两套词表确实不同——{ep:?} 的映射须由本测试钉死"
            );
            // 反查：由路径重建的端点保序（路由用）
            assert_eq!(ConsoleEndpoint::from_path(ep.path()), Some(ep));
        }
        // 审计维度词表（`ConsoleOp`）与端点判别名同形 `config_apply`，
        // 但它**不是**信封 op（信封 op 为 `apply`）——三者关系一并固化。
        assert_eq!(
            serde_json::to_string(&crate::audit::ConsoleOp::ConfigApply).unwrap(),
            "\"config_apply\""
        );
        // GET 端点无信封 op
        for ep in [ConsoleEndpoint::Config, ConsoleEndpoint::Logs] {
            assert_eq!(ep.op_name(), None);
        }
        // 幂等 TTL 常量为独立单一真源（Minor 6），今日与防重放窗口同值但语义不同
        assert_eq!(IDEMPOTENCY_TTL_MS, 30_000);
        assert_eq!(IDEMPOTENCY_TTL_MS, REPLAY_WINDOW_MS);
        assert_eq!(IDEMPOTENCY_CAPACITY, 256);
    }

    // ---- 信封校验：request_id / 时间窗 / op ----
    #[test]
    fn envelope_empty_request_id_rejected() {
        let mut r = req("apply", 1_000);
        r.request_id = "   ".to_string();
        assert_eq!(
            r.validate_envelope(1_000),
            Err(ControlEnvelopeError::EmptyRequestId)
        );
    }

    #[test]
    fn envelope_replay_window_boundary() {
        let now = 1_000_000;
        // 窗口内（恰为 ±30 s 边界 → 允许）
        assert!(req("apply", now - REPLAY_WINDOW_MS).validate_envelope(now).is_ok());
        assert!(req("apply", now + REPLAY_WINDOW_MS).validate_envelope(now).is_ok());
        // 越界 → 拒绝（不得静默接受过期/超前请求）
        let err = req("apply", now - REPLAY_WINDOW_MS - 1)
            .validate_envelope(now)
            .unwrap_err();
        assert!(matches!(
            err,
            ControlEnvelopeError::OutsideReplayWindow {
                issued_at_ms,
                now_ms,
                window_ms,
            } if issued_at_ms == now - REPLAY_WINDOW_MS - 1
                && now_ms == now
                && window_ms == REPLAY_WINDOW_MS
        ));
        assert!(req("apply", now + REPLAY_WINDOW_MS + 1).validate_envelope(now).is_err());
    }

    #[test]
    fn envelope_op_must_match_endpoint_last_segment() {
        let now = 5_000;
        // op 与端点末段一致 → 通过
        assert!(req("apply", now)
            .validate_for(ConsoleEndpoint::ConfigApply, now)
            .is_ok());
        // op 写成别的端点 → 拒绝（防误路由）
        let err = req("release", now)
            .validate_for(ConsoleEndpoint::ConfigApply, now)
            .unwrap_err();
        assert_eq!(
            err,
            ControlEnvelopeError::OpMismatch {
                op: "release".to_string(),
                expected: "apply"
            }
        );
        // 对 GET 查询端点提交写信封 → 拒绝
        assert_eq!(
            req("apply", now).validate_for(ConsoleEndpoint::Config, now),
            Err(ControlEnvelopeError::NotAWriteEndpoint(ConsoleEndpoint::Config))
        );
        // 时间窗越界优先于 op 校验（信封解析在路由校验之前，§3.3 管线顺序）
        assert!(matches!(
            req("release", 0).validate_for(ConsoleEndpoint::ConfigApply, now + 100_000),
            Err(ControlEnvelopeError::OutsideReplayWindow { .. })
        ));
    }

    // ---- 幂等：键语义 + duplicate 不改变首次结果 ----
    #[test]
    fn idempotency_key_is_op_plus_request_id() {
        let key = req("apply", 1).idempotency_key();
        assert_eq!(key.op, "apply");
        assert_eq!(key.request_id, "0f7a3e10-1111-4222-8333-444455556666");
        // 同 request_id 但不同 op ≠ 同一幂等条目（不同操作不得互相吞并）
        let mut other = req("release", 1).idempotency_key();
        other.request_id = key.request_id.clone();
        assert_ne!(other, key);
        // 同 (op, request_id) → 相等（幂等命中判据）
        assert_eq!(key, IdempotencyKey::new("apply", "0f7a3e10-1111-4222-8333-444455556666"));
        assert_eq!(IDEMPOTENCY_CAPACITY, 256);
    }

    #[test]
    fn duplicate_response_preserves_first_result() {
        // 首次执行：成功 + 生效值
        let mut first: ControlResponse<ConfigView> = ControlResponse::ok(
            "rid-1",
            Some(ConfigView {
                groups: vec![],
                revision: 7,
                write_mode: WriteMode::TextPreserve,
            }),
            Some("audit-1".to_string()),
            1_000,
        );
        assert!(first.ok && !first.duplicate);
        // 重放同 request_id → 回首次结果，仅 duplicate 置位（真幂等）
        let mut replay = first.clone();
        replay.mark_duplicate();
        assert!(replay.duplicate && replay.ok, "重复请求的结果必须与首次一致");
        assert_eq!(replay.code, first.code);
        assert_eq!(replay.applied.as_ref().unwrap().revision, 7);
        assert_eq!(replay.audit_id, first.audit_id, "重复请求复用首次审计记录，不重复留痕");
        // 首次失败的重放同样保持失败（不得因重放变为成功）
        first.ok = false;
        first.code = ControlCode::ApplyFailed;
        first.applied = None;
        let mut replay_fail = first.clone();
        replay_fail.mark_duplicate();
        assert!(!replay_fail.ok && replay_fail.code == ControlCode::ApplyFailed);
    }

    // ---- 审计 fail-closed（EDGE-18）----
    #[test]
    fn audit_failure_is_fail_closed() {
        let fail_closed: bool = AUDIT_FAIL_CLOSED;
        assert!(fail_closed, "契约默认裁决必须为 fail-closed");
        let resp: ControlResponse<ConfigView> = ControlResponse::audit_unavailable("rid-9", 42);
        assert!(!resp.ok, "审计不可写 → 不得 ok=true");
        assert_eq!(resp.code, ControlCode::AuditUnavailable);
        assert!(resp.code.is_rejection(), "fail-closed 属拒绝类（未执行）");
        assert_eq!(resp.applied, None, "审计失败 → 操作必须未生效（applied=None）");
        assert_eq!(resp.audit_id, None);
        assert!(!resp.duplicate);
        assert_eq!(resp.at_ms, 42);
        // 消息须可人读（UI 直接展示）
        assert!(resp.message.contains("审计不可写"));
        // JSON 词表
        assert_eq!(
            serde_json::to_string(&ControlCode::AuditUnavailable).unwrap(),
            "\"audit_unavailable\""
        );
    }

    // ---- 配置字段校验：越界 / 非法值 / 只读 一律拒绝（不静默接受）----
    #[test]
    fn config_kind_u16_rejects_out_of_range_and_bad_step() {
        let kind = ConfigKind::U16 { min: 1, max: 61, step: 5 };
        assert!(kind.validate_value(&Value::from(1)).is_ok());
        assert!(kind.validate_value(&Value::from(61)).is_ok());
        // 越界
        assert!(kind.validate_value(&Value::from(0)).unwrap_err().contains("越界"));
        assert!(kind.validate_value(&Value::from(62)).unwrap_err().contains("越界"));
        // 非步长倍数
        assert!(kind.validate_value(&Value::from(7)).unwrap_err().contains("步长"));
        // 类型非法
        assert!(kind.validate_value(&Value::from("30")).unwrap_err().contains("非负整数"));
        assert!(kind.validate_value(&Value::from(30.5)).unwrap_err().contains("非负整数"));
        assert!(kind
            .validate_value(&Value::from(-1))
            .unwrap_err()
            .contains("非负整数"));
        // 超 u16
        assert!(kind.validate_value(&Value::from(70_000u64)).is_err());
    }

    #[test]
    fn config_kind_u64_and_ipv4_and_enum_validation() {
        let u64k = ConfigKind::U64 { min: 100, max: 2000, step: 100 };
        assert!(u64k.validate_value(&Value::from(1000)).is_ok());
        assert!(u64k.validate_value(&Value::from(50)).unwrap_err().contains("越界"));
        assert!(u64k.validate_value(&Value::from(1001)).unwrap_err().contains("步长"));

        let ip = ConfigKind::Ipv4;
        assert!(ip.validate_value(&Value::from("127.0.0.1")).is_ok());
        assert!(ip.validate_value(&Value::from("127.0.0.256")).is_err());
        assert!(ip
            .validate_value(&Value::from("not-an-ip"))
            .unwrap_err()
            .contains("IPv4"));

        let enumk = ConfigKind::Enum {
            options: vec![
                OptionItem { value: "info".into(), label: "信息".into() },
                OptionItem { value: "debug".into(), label: "调试".into() },
            ],
        };
        assert!(enumk.validate_value(&Value::from("debug")).is_ok());
        assert!(enumk
            .validate_value(&Value::from("trace"))
            .unwrap_err()
            .contains("不在允许选项内"));
    }

    #[test]
    fn config_field_rejects_read_only_edit() {
        // display.bind_addr：回环安全红线，editable=false（设计 §3.4 / §6.2）
        let f = ConfigField {
            key: "display.bind_addr".into(),
            label: "本机服务地址（仅回环）".into(),
            kind: ConfigKind::Ipv4,
            value: Value::from("127.0.0.1"),
            default: Value::from("127.0.0.1"),
            unit: None,
            requires_reconnect: false,
            editable: false,
        };
        let err = f.validate_value(&Value::from("0.0.0.0")).unwrap_err();
        assert!(err.contains("只读"), "只读字段改动必须被后端二次校验拒绝: {err}");
        // 可编辑字段正常放行
        let mut ok_field = f.clone();
        ok_field.editable = true;
        assert!(ok_field.validate_value(&Value::from("127.0.0.1")).is_ok());
    }

    #[test]
    fn config_kind_json_is_ui_driveable() {
        // 字面量 JSON：kind 判别式 + 约束随体传输（UI 据此生成步进器，无需硬编码）
        let json = r#"{"kind":"u16","min":1,"max":65535,"step":1}"#;
        let k: ConfigKind = serde_json::from_str(json).unwrap();
        assert_eq!(k, ConfigKind::U16 { min: 1, max: 65535, step: 1 });
        assert_eq!(serde_json::to_string(&k).unwrap(), json);
        assert_eq!(
            serde_json::to_string(&ConfigKind::Ipv4).unwrap(),
            r#"{"kind":"ipv4"}"#
        );
        let e = ConfigKind::Enum {
            options: vec![OptionItem { value: "x".into(), label: "X".into() }],
        };
        let round: ConfigKind = serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(round, e);
    }

    #[test]
    fn config_view_write_mode_literal() {
        let json = r#"{"groups":[],"revision":3,"write_mode":"full_rewrite"}"#;
        let v: ConfigView = serde_json::from_str(json).unwrap();
        assert_eq!(v.write_mode, WriteMode::FullRewrite, "EDGE-23：UI 须据此 Toast 明示");
        assert_eq!(v.revision, 3);
        assert_eq!(
            serde_json::to_string(&WriteMode::TextPreserve).unwrap(),
            "\"text_preserve\""
        );
    }
}
