//! **配置写服务**（`POST /v1/console/config/apply` 的全部落点）——开发单元 **G-2**。
//!
//! 对应设计：
//! - §3.3 控制面**固定 8 步管线**（本文件 [`ConfigService::apply`] 逐步骤落，行内标步骤号）；
//! - §4.3.2 `ConfigService` 写流程（①校验 → ②生成新文本 → ③原子落盘 → ④进程内生效 →
//!   ⑤更新内存副本 + `revision++` → ⑥审计 + 回执）；任一步失败 ⇒ **不得半生效**（EDGE-10）；
//! - §4.3.2.1 **保留式编辑**为主、**仅当不可定位**时显式回退整体回写（EDGE-23）；
//! - §4.3.3 生效分发表（落在 [`crate::hot_apply`]）；
//! - §4.5 审计（落在 [`crate::console_audit`]）。
//!
//! # 三个**硬口径**（本单元自己拍的板，逐条给理由）
//!
//! 1. **不允许"部分应用"**（保存 = 全部）。理由：屏上 P2 的「保存」语义是"把我改的都生效"，
//!    而 UI 的失败回执会**逐字段标红并保留用户输入**（CF-02）——若后端"好的改了、坏的没改"，
//!    屏上就会出现"标红的字段其实已生效"的错位（且 EDGE-10 明禁半生效）。故**全量校验通过才执行**：
//!    任一字段非法 ⇒ 一条都不改（文件、内存副本、`revision` 全不动）。
//! 2. **审计先于执行**（fail-closed，设计 §3.3 / EDGE-18）：intent 写不成 ⇒ **直接拒**，
//!    回 [`ControlCode::AuditUnavailable`]，**不执行任何改动**。
//! 3. **结果审计失败不再回滚**（诚实优先）：此时改动**确实发生了**（文件已落盘、内存已更新），
//!    回滚会让"装置状态"与"审计记录"对不上，而谎报失败（`ok=false`）则是**假话**。
//!    故取：`ok=true` + `audit_id=None`（**不编造**审计号）+ `tracing::error!` 响亮记录。
//!    ——这与"intent 失败必须拒"是**两条不同的规则**，因为二者的**时序**不同（前者在执行前，
//!    后者在执行后）。**残余（如实登记）**：此路径下"操作已发生但无 outcome 记录"，只有
//!    intent 的哈希链条目 + journal 里的 error 可追溯。
//!
//! # 审计条目粒度（为什么是"逐字段一条"）
//!
//! 契约 `ConfigPatch` 文档明写「**逐字段键为审计 `target`**」（`display-proto/src/control.rs:571`）
//! ⇒ 一次保存写 N 条（N = 本次改动的字段数），每条 `target` 一个字段键、`before`/`after` 是
//! **标量**（**不得**塞对象：P5 审计页把对象渲染成「N 字段」，塞对象即等于把审计页的
//! 「前后值」列毁掉——`p5_audit.rs` AU12）。另外写一条 `config.write_mode` 条目（`after` 取
//! `text_preserve` / `full_rewrite` 字符串），这是设计 §4.3.2.1 要求的"审计记 write_mode"。
//! `request_id` 是同一批条目的关联键（契约 §4.5 明写其用途 = 现场对拍）。
//!
//! **逐字段条目的 `reason` 承载生效结论**（评审阻塞 2 的修复点，见 [`per_field_reason`]）：
//! `RestartRequired` 的键在审计里明写"需重启 mupcd 后生效：<原因>" ⇒ 事后查 F19 审计页
//! 不会把"已落盘"误读成"已生效"。`Applied` 的键保持原 `reason`（成功路径为 `None`）。
//!
//! # 回执 `message` 的**用字约束**（硬口径 4）
//!
//! `ControlResponse::message` 会被渲染端**上屏**，且成功路径**不过** `display_safe`
//! （`local-display/src/ui/pages/p2_config.rs::success_toast_text` 原样并入 Toast；失败路径过
//! `display_safe`，但后者对**非 ASCII 原样透传**，见 `ui/pages/mod.rs::display_safe_char`）
//! ⇒ 只要有一个字不在**生成字体的 cmap** 内，真机上就是**豆腐块**。
//!
//! 这条约束**不是**"自由文本不查码表"那条既有口径：那些串（告警 `message`、型号 / 序列号、
//! 外部库错误串）**不是我们生成的**，无从约束；而本模块的回执文案**逐字都是我们自己拼的**，
//! 完全可控 ⇒ **必须**只用 cmap 内的字符。清单真源 = `local-display/fonts/lv_font_cmap.txt`
//! （入库派生项，324 码位）；**逐字符**的网见 `console_host` 的
//! `config_receipt_messages_use_only_font_cmap_glyphs` 用例。
//!
//! 由此推出三条**写法规则**（本模块内不得破例）：
//!
//! a. **标点取 cmap 内的等价物**：全角 `（ ）` `；` `：` `，` 一律缺字形 ⇒ 用 `·`(U+00B7) /
//!    半角 `:`(U+003A)。注意**半角逗号也不在 cmap 内**（只有 `·` 能当分隔符）。
//! b. **不得点名机器键名**：`gateway.listen_port` 这类键**必然**含缺字形字符（小写 `t` 不在
//!    cmap 内、`_` 也不在）⇒ 屏上点名一律取字段表的 **`label`**（见 [`restart_labels`]）。
//! c. **不得内联外部错误串**：`serde_yaml` / `std::io` / 契约的 `ControlEnvelopeError`
//!    （后者是**全小写英文**）都含缺字形字符 ⇒ 详情走**审计 `reason` + `tracing`**，
//!    屏上只给固定文案。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use mupc_display_proto::{
    AuditResult, ConfigPatch, ConfigView, ConsoleAuditEntry, ConsoleEndpoint, ConsoleOp,
    ControlCode, ControlRequest, ControlResponse, FieldError, IdempotencyKey, WriteMode,
    CONSOLE_OPERATOR,
};
use serde_json::Value;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::console_audit::{AuditIntent, ConsoleAuditSink};
use crate::console_host::receipt as msg;
use crate::console_host::{config_view, field_meta, set_field, FIELDS};
use crate::core_config::CoreConfig;
use crate::hot_apply::{ApplyOutcome, HotApply};
use crate::idempotency::{IdempotencyTable, Reserve};
use crate::yaml_edit::{self, ScalarEdit};

/// 当前 Unix 毫秒（信封窗口与回执 `at_ms` 的唯一时钟真源）。
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// 配置写服务（**进程内唯一实例**；持真源路径、内存副本、审计、生效分发器、幂等表）。
pub struct ConfigService {
    /// 真源 yaml（`--config`）。
    path: PathBuf,
    /// 进程内权威读源（与读路径 `GET /v1/console/config` **同一个 `Arc`**，见 `startup.rs`）。
    core: Arc<RwLock<CoreConfig>>,
    /// 审计落点（fail-closed 的失败注入点）。
    audit: Arc<dyn ConsoleAuditSink>,
    /// 生效分发器（设计 §4.3.3）。
    hot: HotApply,
    /// 幂等表（有界 + TTL）。
    table: IdempotencyTable<ControlResponse<ConfigView>>,
    /// 成功落盘次数（`ConfigView.revision` 的真源；契约：每次成功写入递增）。
    revision: AtomicU64,
    /// 最近一次落盘的写模式（`ConfigView.write_mode` 的真源）。
    last_write_mode: Mutex<WriteMode>,
}

impl ConfigService {
    /// 构造（`audit` 由装配点注入；**审计不可用时应让写路径整体 `Unavailable`**，见
    /// `console_host::ApplySource`）。
    pub fn new(
        path: PathBuf,
        core: Arc<RwLock<CoreConfig>>,
        audit: Arc<dyn ConsoleAuditSink>,
        hot: HotApply,
    ) -> Self {
        Self {
            path,
            core,
            audit,
            hot,
            table: IdempotencyTable::with_contract_bounds(),
            revision: AtomicU64::new(0),
            last_write_mode: Mutex::new(WriteMode::TextPreserve),
        }
    }

    /// 进程内权威读源（句柄同一性由 `console_host` 的单测用 `Arc::ptr_eq` 钉死）。
    ///
    /// `#[cfg(test)]`：生产路径不消费它（生产是**注入**这个 `Arc`，而不是取回来）——
    /// 它与 G-1 的 `ConsoleHost::config_source` 同款，都只是**测试用的那把尺子**的取数口。
    #[cfg(test)]
    pub fn core(&self) -> &Arc<RwLock<CoreConfig>> {
        &self.core
    }

    /// 成功落盘次数。
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    /// 最近一次落盘的写模式。
    pub fn last_write_mode(&self) -> WriteMode {
        *self
            .last_write_mode
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// 视图（`GET /v1/console/config` 与写回执**共用同一个构造函数** ⇒ 两处口径不可能漂移）。
    pub fn view(&self, cfg: &CoreConfig) -> ConfigView {
        config_view(cfg, self.revision(), self.last_write_mode())
    }

    /// **完整管线**（设计 §3.3 的 2–8 步；第 1 步"路由与方法"由 axum 的路由表结构性保证）。
    ///
    /// 本函数**不 panic**、**不阻塞**（内部同步文件 IO 见模块头的说明）。
    pub async fn apply(&self, req: &ControlRequest<ConfigPatch>) -> ControlResponse<ConfigView> {
        let now = now_ms();

        // ── 步骤 2：信封校验（request_id 非空 / op 与端点一致 / ±30 s 重放窗口）──────────
        //
        // 失败**不**记入幂等表（此刻连"这是哪一次请求"都不可信：`request_id` 可能为空、
        // `op` 可能是别的端点）。渲染端在窗口外**不发包**（`console.rs::retry`），故这条
        // 路径只在"时钟不同步 / 手工构造请求"时可达——但**必须**拒，不能静默接受过期请求。
        if let Err(e) = req.validate_for(ConsoleEndpoint::ConfigApply, now) {
            // ⚠️ `e`（契约 `ControlEnvelopeError`）是**全小写英文** ⇒ 含 cmap 外的字，
            // **不进** `message`（硬口径 4c）：屏上给固定文案，原因走 `field_errors`
            // （结构化，与逐字段失败同渠道）+ 一条 `warn`（现场排障不丢信息）。
            // 用例 `op_mismatch_and_empty_request_id_are_rejected` 据此断言"误路由仍被点名"。
            tracing::warn!(request_id = %req.request_id, error = %e,
                "配置请求信封非法（op 误路由 / request_id 为空 / 超出重放窗）⇒ 拒绝");
            return ControlResponse::rejected(
                req.request_id.clone(),
                ControlCode::RejectedValidation,
                msg::BAD_ENVELOPE,
                vec![FieldError {
                    field: "request".to_string(),
                    reason: e.to_string(),
                }],
                None,
                now,
            );
        }

        // ── 步骤 3：幂等查表 ────────────────────────────────────────────────────────────
        let key = IdempotencyKey::new(
            ConsoleEndpoint::ConfigApply.op_name().unwrap_or("apply"),
            req.request_id.clone(),
        );
        match self.table.reserve(key.clone(), now) {
            // 命中且已完成 ⇒ **首次的原始回执** + duplicate=true（`ok` / `code` 不变）
            Reserve::Done(mut first) => {
                first.mark_duplicate();
                return first;
            }
            // 命中且处理中 ⇒ Busy（**不**排队、**不**重复执行）
            Reserve::InFlight => {
                return ControlResponse::rejected(
                    req.request_id.clone(),
                    ControlCode::Busy,
                    msg::BUSY,
                    Vec::new(),
                    None,
                    now,
                )
            }
            Reserve::Fresh => {}
        }

        // ── 步骤 4–8：校验 / intent 审计 / 执行 / 结果审计 / 回执 ─────────────────────────
        let resp = self.execute(req, now).await;
        // 终态入表（**含失败态**：契约要求"首次失败的重放同样保持失败"）。
        // `false` = **迟到 complete**（本次请求跑了超过 TTL，占位已被 sweep 清掉）⇒ 回执
        // 已被丢弃、不会作为重复请求的凭据。这是**可观测事实**，不静默：打一条 `warn`
        // （现场若频繁出现，说明"单次保存耗时 > TTL"——那是需要现场关注的性能事实）。
        if !self.table.complete(&key, resp.clone()) {
            tracing::warn!(
                request_id = %req.request_id,
                "迟到 complete：本次请求耗时超过幂等 TTL，占位已过期 ⇒ 回执不入表（同 id 再来会作为新操作执行）"
            );
        }
        resp
    }

    /// 步骤 4–8 的实体。
    async fn execute(
        &self,
        req: &ControlRequest<ConfigPatch>,
        now: u64,
    ) -> ControlResponse<ConfigView> {
        // 写锁覆盖"读旧值 → 校验 → 生成文本 → 落盘 → 更新内存副本"整段：否则两个并发保存
        // 会各自基于**同一个 before** 生成文本，后写者覆盖前写者（丢改动）。锁内**无 await**
        // （文件 IO 是同步的）⇒ 不会长时间占锁，也不会让 GET 等一次网络往返。
        let mut guard = self.core.write().await;
        let before = guard.clone();

        // ── 步骤 4：逐字段校验（**全量**通过才执行；见模块头硬口径 1）────────────────────
        let mut field_errors: Vec<FieldError> = Vec::new();
        // 本次**将真正写盘**的字段（值与原值相同者不算改动：保存一个没改的值不该动文件，
        // 否则每次点保存都会刷新 mtime + 重写一行文本）
        let mut planned: Vec<(String, Value)> = Vec::new();
        for (key, value) in &req.payload.changes {
            let Some(meta) = field_meta(key) else {
                field_errors.push(FieldError {
                    field: key.clone(),
                    reason: format!("未知字段 `{key}`（不在本机配置字段表内）"),
                });
                continue;
            };
            // 复用契约的二次校验（`editable` + `ConfigKind::validate_value` 值域）
            let probe = mupc_display_proto::ConfigField {
                key: meta.key.to_string(),
                label: String::new(),
                kind: (meta.kind)(),
                value: (meta.current)(&before),
                default: (meta.default)(),
                unit: None,
                requires_reconnect: meta.requires_reconnect,
                editable: meta.editable,
            };
            if let Err(reason) = probe.validate_value(value) {
                field_errors.push(FieldError {
                    field: key.clone(),
                    reason,
                });
                continue;
            }
            if (meta.current)(&before) != *value {
                planned.push((key.clone(), value.clone()));
            }
        }
        if !field_errors.is_empty() {
            // 校验失败 ⇒ **一条都不执行**；但**要留痕**（PL-1：成功与失败均写）。
            // 审计写不成 ⇒ 按 fail-closed 取更严格的信号 `AuditUnavailable`（此时也确实
            // "什么都没执行"，UI §8.3 的固定文案「审计不可用，操作未执行」逐字为真）。
            let audited = self.audit_failure(&req.request_id, &field_errors, now);
            return match audited {
                Ok(audit_id) => ControlResponse::rejected(
                    req.request_id.clone(),
                    ControlCode::RejectedValidation,
                    // 逐字段的**具体**原因在 `field_errors`（渲染端就地标红，CF-02）；
                    // `message` 只给"一条都没存"的结论（用字约束见硬口径 4）。
                    msg::VALIDATION_FAILED,
                    field_errors,
                    // 契约：「审计记录 ID（**成功与失败均返回**；便于现场对拍）」⇒ 失败回执也要带号
                    audit_id,
                    now,
                ),
                Err(e) => audit_unavailable(&req.request_id, now, &e),
            };
        }

        // 无字段变化 ⇒ 不写盘、不递增 revision（不是失败：用户点了一次"保存"但内容一致）
        if planned.is_empty() {
            drop(guard);
            tracing::info!(request_id = %req.request_id, "配置保存：无字段变化，未写盘");
            return with_message(
                ControlResponse::ok(req.request_id.clone(), Some(self.view(&before)), None, now),
                msg::NO_CHANGE,
            );
        }

        // ── 步骤 5：审计 intent **前置**写入（失败 ⇒ AuditUnavailable 并终止，**不执行**）──
        let target = planned
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let intent = AuditIntent {
            op: match req.payload.from {
                mupc_display_proto::PatchSource::Edit => ConsoleOp::ConfigApply,
                mupc_display_proto::PatchSource::ResetDefault => ConsoleOp::ConfigResetDefault,
            },
            target: target.clone(),
            request_id: req.request_id.clone(),
            summary: format!(
                "changes={} from={:?} keys=[{target}]",
                planned.len(),
                req.payload.from
            ),
        };
        if let Err(e) = self.audit.record_intent(&intent) {
            // **fail-closed**：审计写不成 ⇒ 一个字节都不改（内存副本、文件、revision 全不动）
            drop(guard);
            tracing::error!(request_id = %req.request_id, error = %e,
                "审计 intent 写入失败 ⇒ 拒绝执行配置写入（fail-closed）");
            return audit_unavailable(&req.request_id, now, &e);
        }

        // ── 步骤 6：执行（生成文本 → 写后自检 → 原子落盘）───────────────────────────────
        let mut after = before.clone();
        for (key, value) in &planned {
            if let Err(e) = set_field(&mut after, key, value) {
                // 校验已过 ⇒ 不可达；仍写成分支而不是 `unwrap`（本模块不 panic）
                drop(guard);
                return self.finish_failure(
                    req,
                    now,
                    &planned,
                    ControlCode::Internal,
                    msg::WRITE_FAILED_FIELD,
                    format!("应用字段 `{key}` 失败: {e}"),
                );
            }
        }
        // `mode_note` 进**成功回执**（`message`）⇒ 必须是 cmap 内的固定串；原因（`{e}`，外部
        // 错误串）进 `tracing`（**不进屏**，硬口径 4c 与 `receipt` 模块头）。
        let (text, mode, mode_note) = match std::fs::read_to_string(&self.path) {
            Ok(src) => match self.preserve_edit(&src, &planned) {
                Ok(t) => (t, WriteMode::TextPreserve, None),
                Err(e) => {
                    // **回退路径**：不可定位 ⇒ 整体序列化回写（**必须显式**：EDGE-23）
                    tracing::warn!(path = %self.path.display(), error = %e,
                        "保留式编辑无法定位：回退整体序列化回写（注释与未建模键将丢失）");
                    match serde_yaml::to_string(&after) {
                        Ok(t) => (
                            t,
                            WriteMode::FullRewrite,
                            Some(msg::FULL_REWRITE_UNLOCATABLE),
                        ),
                        Err(e2) => {
                            drop(guard);
                            return self.finish_failure(
                                req,
                                now,
                                &planned,
                                ControlCode::ApplyFailed,
                                msg::WRITE_FAILED_SERIALIZE,
                                format!("整体序列化失败: {e2}"),
                            );
                        }
                    }
                }
            },
            Err(e) => {
                // 文件读不出来（不存在 / 权限）⇒ 无"原文本"可保真，只能整体回写
                tracing::warn!(path = %self.path.display(), error = %e,
                    "配置文件读取失败：回退整体序列化回写（写路径无从保留原文本）");
                match serde_yaml::to_string(&after) {
                    Ok(t) => (
                        t,
                        WriteMode::FullRewrite,
                        Some(msg::FULL_REWRITE_UNREADABLE),
                    ),
                    Err(e2) => {
                        drop(guard);
                        return self.finish_failure(
                            req,
                            now,
                            &planned,
                            ControlCode::ApplyFailed,
                            msg::WRITE_FAILED_SERIALIZE,
                            format!("整体序列化失败: {e2}"),
                        );
                    }
                }
            }
        };

        // 写后自检（设计 §4.3.2.1）：① 可解析为 `CoreConfig`；② 只有目标键变化。
        // 不满足 ⇒ **不落盘**，返回 ApplyFailed（文件保持旧内容，**不半生效**）。
        if let Err(e) = self_post_check(&text, &after) {
            drop(guard);
            return self.finish_failure(
                req,
                now,
                &planned,
                ControlCode::ApplyFailed,
                msg::WRITE_FAILED_SELFCHECK,
                format!("写后自检失败（未落盘）: {e}"),
            );
        }

        // 原子落盘（设计 §4.3.2 ③：tmp → fsync → bak → rename）
        if let Err(e) = atomic_write(&self.path, &text) {
            drop(guard);
            return self.finish_failure(
                req,
                now,
                &planned,
                ControlCode::ApplyFailed,
                msg::WRITE_FAILED_FILE,
                e,
            );
        }

        // ── 步骤 6'：进程内生效 + 步骤 7 的一半（内存副本 / revision / write_mode）──────
        *guard = after.clone();
        drop(guard);
        let prev_mode = self.last_write_mode();
        self.revision.fetch_add(1, Ordering::SeqCst);
        *self
            .last_write_mode
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = mode;

        // ── 步骤 4'（设计 §4.3.3）：逐项热生效（结论**如实**进回执 / 审计 / 日志）───────
        let applied_results: Vec<(String, Value, ApplyOutcome)> = planned
            .iter()
            .map(|(k, v)| (k.clone(), v.clone(), self.hot.apply(k, v)))
            .collect();
        let restart: Vec<&str> = applied_results
            .iter()
            .filter(|(_, _, o)| !o.is_applied())
            .map(|(k, _, _)| k.as_str())
            .collect();

        // ── 步骤 7：结果审计（outcome：before/after/result/reason + write_mode）──────────
        let entries = self.outcome_entries(
            req,
            now,
            &applied_results,
            &before,
            prev_mode,
            mode,
            AuditResult::Ok,
            None,
        );
        let audit_id = self.write_entries(&entries, req);

        // ── 步骤 8：回执（`applied` = **新** `ConfigView`）────────────────────────────────
        //
        // ⚠️ 本串**直接用字面量拼给屏看**（渲染端成功分支原样上屏，见本文件模块头硬口径 4）
        // ⇒ 每一处都必须取 `receipt` 里的固定串 / 字段 `label`：
        // 全角括号与「项」都缺字形，`mupcd` 的小写字母也缺字形（原串三处都踩了）。
        let view = self.view(&after);
        let mut screen = String::from(msg::SAVED);
        if let Some(note) = mode_note {
            screen.push_str(" · ");
            screen.push_str(note);
        }
        if !restart.is_empty() {
            screen.push_str(msg::RESTART_PREFIX);
            screen.push_str(&restart_labels(&restart));
        }
        tracing::info!(request_id = %req.request_id, revision = view.revision,
            write_mode = ?mode, audit_id = ?audit_id, "配置保存完成: {screen}");
        with_message(
            ControlResponse::ok(req.request_id.clone(), Some(view), audit_id, now),
            screen,
        )
    }

    /// 保留式编辑：把 `planned` 翻成 [`ScalarEdit`] 并走 [`yaml_edit::apply_edits`]。
    ///
    /// 定位串取 **`meta.yaml_path`**（设计 §4.3.2.1 明写"以 `yaml_path` 定位"），
    /// **不是**随手 `split('.')` 出来的键：`yaml_path` 是字段表里**显式登记**的那一栏
    /// （G-1 的 `yaml_path_matches_key_for_every_field` 保证它与 `key` 同形），
    /// 将来若要给某个键换 yaml 落点（如 `gateway.port` 这种 UI 文案里的别名），
    /// 只改表里那一栏即可 —— 若在此处 `split`，两处就会各说各话。
    fn preserve_edit(
        &self,
        src: &str,
        planned: &[(String, Value)],
    ) -> Result<String, yaml_edit::EditError> {
        let edits: Vec<ScalarEdit> = planned
            .iter()
            .map(|(k, v)| {
                let path = field_meta(k).map_or(k.as_str(), |m| m.yaml_path);
                ScalarEdit::from_key(path, v.clone()).ok_or(yaml_edit::EditError::KeyMissing {
                    path: path.to_string(),
                })
            })
            .collect::<Result<_, _>>()?;
        yaml_edit::apply_edits(src, &edits)
    }

    /// 校验失败的留痕（逐字段写 Failed 条目，`before`/`after` 为 `None`），返回首条 id 供回执带上。
    fn audit_failure(
        &self,
        request_id: &str,
        field_errors: &[FieldError],
        now: u64,
    ) -> Result<Option<String>, String> {
        let entries: Vec<ConsoleAuditEntry> = field_errors
            .iter()
            .map(|fe| ConsoleAuditEntry {
                id: Uuid::new_v4().to_string(),
                ts_ms: now,
                operator: CONSOLE_OPERATOR.to_string(),
                op: ConsoleOp::ConfigApply,
                target: fe.field.clone(),
                before: None,
                after: None,
                result: AuditResult::Failed,
                reason: Some(fe.reason.clone()),
                request_id: request_id.to_string(),
            })
            .collect();
        for e in &entries {
            self.audit.record_outcome(e)?;
        }
        Ok(entries.first().map(|e| e.id.clone()))
    }

    /// 执行期失败（self-check / 落盘 / 序列化）：逐字段 Failed 条目 + 统一回执。
    ///
    /// **`screen` 与 `reason` 是两条不同的串**（硬口径 4c）：
    /// - `screen` 进**回执 `message`** ⇒ 必须是 `receipt` 里的**固定文案**（cmap 内，可上屏）；
    /// - `reason` 是**外部错误串拼出来的详情**（`serde_yaml` / `std::io` 的英文 + 路径）⇒
    ///   含 cmap 外的字，**只**进审计条目 `reason` 与 `tracing`（现场排障不丢信息）。
    ///
    /// 修复前二者是**同一条**串（`message == reason`）⇒ 屏上是"豆腐块 + 半截英文"。
    fn finish_failure(
        &self,
        req: &ControlRequest<ConfigPatch>,
        now: u64,
        planned: &[(String, Value)],
        code: ControlCode,
        screen: &str,
        reason: String,
    ) -> ControlResponse<ConfigView> {
        tracing::error!(request_id = %req.request_id, code = ?code, reason = %reason,
            "配置写入失败（装置维持原配置运行）: {screen}");
        let entries: Vec<ConsoleAuditEntry> = planned
            .iter()
            .map(|(k, _)| ConsoleAuditEntry {
                id: Uuid::new_v4().to_string(),
                ts_ms: now,
                operator: CONSOLE_OPERATOR.to_string(),
                op: ConsoleOp::ConfigApply,
                target: k.clone(),
                before: None,
                after: None,
                result: AuditResult::Failed,
                reason: Some(reason.clone()),
                request_id: req.request_id.clone(),
            })
            .collect();
        let audit_id = self.write_entries(&entries, req);
        ControlResponse::rejected(
            req.request_id.clone(),
            code,
            screen,
            Vec::new(),
            audit_id,
            now,
        )
    }

    /// 结果审计条目（**逐字段一条** + 一条 `config.write_mode`；见模块头"审计条目粒度"）。
    ///
    /// `prev_mode` 由调用点传入**本次更新之前**的写模式：`config.write_mode` 条目的
    /// `before → after` 必须是**真的两个不同时刻的值**（用同一次的值会让审计页显示
    /// `text_preserve → text_preserve` 这种假前后值）。
    #[allow(clippy::too_many_arguments)]
    fn outcome_entries(
        &self,
        req: &ControlRequest<ConfigPatch>,
        now: u64,
        applied: &[(String, Value, ApplyOutcome)],
        before: &CoreConfig,
        prev_mode: WriteMode,
        mode: WriteMode,
        result: AuditResult,
        reason: Option<String>,
    ) -> Vec<ConsoleAuditEntry> {
        let op = match req.payload.from {
            mupc_display_proto::PatchSource::Edit => ConsoleOp::ConfigApply,
            mupc_display_proto::PatchSource::ResetDefault => ConsoleOp::ConfigResetDefault,
        };
        let mut out: Vec<ConsoleAuditEntry> = applied
            .iter()
            .map(|(k, v, outcome)| ConsoleAuditEntry {
                id: Uuid::new_v4().to_string(),
                ts_ms: now,
                operator: CONSOLE_OPERATOR.to_string(),
                op,
                target: k.clone(),
                // `before` / `after` **必须是标量**（P5 把对象渲染成「N 字段」⇒ 会毁掉
                // 审计页的「前后值」列，见 `p5_audit.rs` AU12）
                before: field_meta(k).map(|m| (m.current)(before)),
                after: Some(v.clone()),
                result,
                // **生效结论必须进审计，不得丢**（评审**阻塞 2**）：修复前这里写作 `|(k, v, _)|`，
                // 把 `ApplyOutcome` 丢进 `_` ⇒ `reason` 恒 `None`，审计条目**看不出该键需重启
                // 才变**——与 `hot_apply.rs`「该键进 `restart_required` + 回执 message 明写」
                // 的自述不符（回执 ✅ / 日志 ✅ / **审计 ✗**）。现场事后查审计（F19 页）只能看到
                // 「before → after」而看不到"这次改动其实还没生效"，正是本单元最忌讳的**静默失实**。
                reason: per_field_reason(reason.as_deref(), outcome),
                request_id: req.request_id.clone(),
            })
            .collect();
        // write_mode 条目（设计 §4.3.2.1：审计须记 write_mode）
        out.push(ConsoleAuditEntry {
            id: Uuid::new_v4().to_string(),
            ts_ms: now,
            operator: CONSOLE_OPERATOR.to_string(),
            op,
            target: "config.write_mode".to_string(),
            before: Some(Value::String(short_mode(prev_mode).to_string())),
            after: Some(Value::String(short_mode(mode).to_string())),
            result,
            reason,
            request_id: req.request_id.clone(),
        });
        out
    }

    /// 写结果审计条目；返回**第一条**的 id 作为回执 `audit_id`（整批由 `request_id` 关联）。
    ///
    /// 失败**不**回滚（模块头硬口径 3）：`ok=true` + `audit_id=None` + 响亮记录。
    fn write_entries(
        &self,
        entries: &[ConsoleAuditEntry],
        req: &ControlRequest<ConfigPatch>,
    ) -> Option<String> {
        for e in entries {
            if let Err(err) = self.audit.record_outcome(e) {
                tracing::error!(request_id = %req.request_id, error = %err,
                    "结果审计写入失败：改动**已生效**（不回滚、不谎报失败），但本次无审计条目");
                return None;
            }
        }
        entries.first().map(|e| e.id.clone())
    }
}

/// `WriteMode` 的线格式短名（与契约 serde 名一致：`text_preserve` / `full_rewrite`）。
fn short_mode(m: WriteMode) -> &'static str {
    match m {
        WriteMode::TextPreserve => "text_preserve",
        WriteMode::FullRewrite => "full_rewrite",
    }
}

/// 逐字段审计条目的 `reason`：把**生效结论**写进去（评审**阻塞 2**）。
///
/// - [`ApplyOutcome::RestartRequired`] ⇒ `Some("需重启 mupcd 后生效：<原因>")`：
///   审计条目**自证**"该键需重启才变"，事后查审计（F19 页）不会把"已落盘"误读成"已生效"。
/// - [`ApplyOutcome::Applied`] ⇒ 沿用调用方传入的 `reason`（成功路径下恒 `None`）：
///   真热生效的键**不得**被写成"需重启"（那会把审计页变成反向失真）。
///
/// 文案固定含「需重启」：与回执 `message`（`"配置已保存 · 需重启进程生效: <标签>"`）与
/// `hot_apply` 的 `tracing::warn!`（`"配置已保存但运行行为需重启才变…"`）三处共享**同一个词**
/// ⇒ 现场仍可用 `grep 需重启` 三处对拍（**本串属 P5 审计页的自由文本渠道**，由该页的
/// `free_text_safe` 与 AU9 残余口径处置，**不**受本单元"回执 `message` 逐字 ⊆ cmap"的约束）。
fn per_field_reason(reason: Option<&str>, outcome: &ApplyOutcome) -> Option<String> {
    match outcome {
        ApplyOutcome::Applied => reason.map(str::to_string),
        ApplyOutcome::RestartRequired { reason: why } => {
            Some(format!("需重启 mupcd 后生效：{why}"))
        }
    }
}

/// 需重启字段的**屏上点名**：取字段表 [`FIELDS`] 的 **`label`**（配置页每一行的现成标题），
/// **不是** `key`。
///
/// ⚠️ **为什么不能点名 `key`**（硬口径 4b）：机器键名**必然**含缺字形字符 —— `t`(U+0074) 与
/// `_`(U+005F) 都不在 cmap 内 ⇒ `gateway.listen_port` 原样上屏是豆腐块，过 `display_safe`
/// 又会被打散成 `GA?EWAY.LIS?EN?POR?`（点名落空，渲染端 PD24 有实测记录）。
/// `label` 来自**同一张字段表**（零新增文案），操作者按它能在 P2 页上直接找到那一行。
///
/// 标签之间用 `·` 分隔（**半角逗号也不在 cmap 内**，只有 `·` 能当分隔符）。
fn restart_labels(keys: &[&str]) -> String {
    keys.iter()
        .map(|k| field_meta(k).map_or(msg::UNKNOWN_FIELD, |m| m.label))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// `AuditUnavailable` 回执（fail-closed 的唯一出口；`applied=None` ⇒ 操作未生效）。
fn audit_unavailable(
    request_id: &str,
    now: u64,
    reason: &str,
) -> ControlResponse<ConfigView> {
    // 契约（`display-proto`，**冻结禁改**）的固定文案含 `写` / 全角括号 / 逗号，**都不在 cmap 内**
    // ⇒ 在它后面接 `({reason})` 只会让豆腐块更长。这里**整条换掉**：
    // - 屏上：P2 / `state.rs` 对 `AuditUnavailable` **按 `code` 覆盖**成 EDGE-18 固定串，
    //   两者**同义**（`receipt::AUDIT_UNAVAILABLE`）⇒ 漂移无上屏后果；
    // - 现场：原因（外部装配错误串，含 cmap 外的字）改由下面的 `tracing::error!` 承载，
    //   信息量**不比原来少**（原来读的人是现场排障，读的还是日志）。
    tracing::error!(
        request_id,
        reason,
        "审计不可写 ⇒ fail-closed 拒绝执行配置写入"
    );
    let mut r = ControlResponse::audit_unavailable(request_id.to_string(), now);
    r.message = crate::console_host::receipt::AUDIT_UNAVAILABLE.to_string();
    r
}

/// 覆盖人读消息（契约的 `ok` / `rejected` 构造器把人读文案写死了，而本单元的成功文案必须
/// 带上"哪些字段需重启生效 / 原文是否已不存在"——那是**事实**，不能省）。
///
/// ⚠️ 传进来的串必须满足用字约束（模块头硬口径 4：逐字 ⊆ 生成字体的 cmap）——
/// 成功路径的 `message` 被渲染端**原样上屏**。
///
/// 实现为**自由函数**而非 `impl ControlResponse<ConfigView>`：`ControlResponse` 是
/// `display-proto`（**冻结契约，本单元禁改**）的类型，Rust 不允许对它写 inherent impl
/// （E0116 orphan rule）。字段是 `pub` ⇒ 直接改字段，不需要契约侧开口子。
fn with_message<T>(mut r: ControlResponse<T>, message: impl Into<String>) -> ControlResponse<T> {
    r.message = message.into();
    r
}

/// **写后自检**（设计 §4.3.2.1）：新文本必须①能被 `serde_yaml` 解析为 `CoreConfig`，
/// ②解析结果**只有目标键变化**（逐字段与"内存副本的下一态"比对）。
///
/// ⚠️ 这条自检同时是**序列化/反序列化对称性**的运行期网：`Serialize` 派生一旦与
/// `Deserialize` 不对称（`skip_serializing_if` / `default` 配错），整体回写路径会**当场**
/// 自检失败（而不是把配置写坏）。
fn self_post_check(text: &str, expected: &CoreConfig) -> Result<(), String> {
    let parsed: CoreConfig =
        serde_yaml::from_str(text).map_err(|e| format!("新文本无法解析为 CoreConfig: {e}"))?;
    for m in FIELDS {
        let got = (m.current)(&parsed);
        let want = (m.current)(expected);
        if got != want {
            return Err(format!(
                "字段 `{}` 与预期不符（期望 {want}，实得 {got}）",
                m.key
            ));
        }
    }
    Ok(())
}

/// **原子落盘**（设计 §4.3.2 ③）：`写 .tmp → fsync → 备份 .bak → rename`。
///
/// 任一步失败：`.tmp` 丢弃（尽力）、`.bak` **不动**、真源文件保持原内容 —— 即"装置维持原配置
/// 运行"（EDGE-10）。Windows 的 `std::fs::rename` 目标存在即失败 ⇒ 先移走 `.bak` / 原文件。
///
/// # ⚠️ 崩溃窗口与恢复手段（评审建议 6.4，逐条如实登记）
///
/// 1. **"真源暂时不存在"的窗口**：②③ 两步之间真源文件不在（已改名成 `.bak`，`.tmp` 还没顶上）。
///    此刻掉电/被 kill ⇒ 下次启动**无真源可读**（评审理据：全仓原先**没有任何 `.bak` 恢复
///    逻辑**）。**已处置**：启动期加载改走 [`load_config_with_backup_recovery`]（真源缺失而
///    `.bak` 在 ⇒ 用它恢复真源并**响亮记录**），窗口由"无配置可读"降级为"用上一份好配置启动"。
///    **残余**：恢复是**事后**的（恢复前的这次启动窗口内进程读不到配置 ⇒ `mupcd` 起不来），
///    要彻底消除需改写入序（如"先 rename tmp→path、再复制到 .bak"），那会让 `.bak` 与真源
///    的对应关系变松 —— 属后续单元/PM 裁定项，**本单元不改写序**。
/// 2. **目录 fsync 未做（如实登记，未实现）**：`rename` 的目录项落盘依赖文件系统排序，掉电时
///    理论上可能丢 rename。修法是 `rename` 后 `File::open(parent)?.sync_all()`——**仅 `cfg(unix)`
///    有意义**（Windows 上 `sync_all` 对目录句柄不可用），而本单元的开发/验证环境是 Windows
///    （见任务书铁律 5）⇒ 写了也**无法在本单元给出可红的回归**。故**登记不实现**：现场（Linux）
///    若要补，落点在 `atomic_write` 的 ③ 之后，一行 `#[cfg(unix)]` 块。
/// 3. **新文件权限继承未做（如实登记，未实现）**：`.tmp` 由 `File::create` 创建 ⇒ 权限取
///    **umask 默认**（典型 0644），而现场真源可能是 0600（含串口/网口等部署细节的配置）。
///    修法是"rename 前把真源的 `permissions()` 复制给 `.tmp`"，同样**仅 `cfg(unix)` 可验证**
///    （Windows 的 `set_permissions` 只表达只读位）⇒ 与第 2 条同理由登记不实现。落点：
///    `atomic_write` 的 ① 与 ② 之间。
///    **注**：`.bak` 由真源 `rename` 而来 ⇒ 保留原权限；故从崩溃窗口恢复出来的真源会**带上
///    `.bak` 的权限**（比新文件更接近现场口径），这是恢复路径的附带好处。
fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    let tmp = sibling(path, ".tmp");
    let bak = sibling(path, ".bak");

    // ① 写 tmp + fsync（先落数据，再动真源文件：任何时刻磁盘上至少有一份完整配置）
    {
        let mut f = std::fs::File::create(&tmp)
            .map_err(|e| format!("创建临时文件 {} 失败: {e}", tmp.display()))?;
        std::io::Write::write_all(&mut f, text.as_bytes())
            .map_err(|e| format!("写临时文件 {} 失败: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("fsync 临时文件 {} 失败: {e}", tmp.display()))?;
    }

    // ② 备份现有真源文件（`.bak` 每次覆盖；不存在则跳过 —— 首次保存的场景）
    if path.exists() {
        if bak.exists() {
            std::fs::remove_file(&bak)
                .map_err(|e| format!("清理旧备份 {} 失败: {e}", bak.display()))?;
        }
        std::fs::rename(path, &bak)
            .map_err(|e| format!("备份 {} 失败: {e}", path.display()))?;
    }

    // ③ rename 顶替（同目录内 rename 是原子的；失败则把备份还原回去）
    if let Err(e) = std::fs::rename(&tmp, path) {
        if bak.exists() {
            let _ = std::fs::rename(&bak, path);
        }
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("rename {} 失败: {e}", path.display()));
    }
    Ok(())
}

/// 同目录兄弟路径（`x.yaml` + `.tmp` ⇒ `x.yaml.tmp`）。
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

/// **启动期加载真源配置，含 `.bak` 兜底恢复**（评审**建议 6.4**；`main.rs` Phase 1 的唯一入口）。
///
/// 触发条件**刻意收窄**到"真源**读不出来**"（不存在 / 不可读）——这正是 [`atomic_write`] 的
/// 崩溃窗口留下的磁盘形态（真源已改名成 `.bak`，`.tmp` 还没顶上就被 kill）。
///
/// # 三条硬口径
///
/// 1. **只在真源读不出来时才看 `.bak`**：真源存在且能读 ⇒ 一律以真源为准，**不碰** `.bak`
///    （否则会把"现场手工改回正确配置"这件事悄悄回退掉）。
/// 2. **先解析、后落盘**：`.bak` 文本先 `serde_yaml` 解成 `CoreConfig`；解析不过 ⇒ 直接 `Err`
///    （**不**拿坏文本覆盖真源，也**不**静默改用内嵌默认值——那会让装置跑在与现场无关的配置上）。
/// 3. **恢复动作本身走同一套原子写**：`写 .tmp → fsync → rename`（此时真源不存在，故无备份步）
///    ⇒ 恢复出来的真源是完整文件，不会留下半截。
///
/// 返回 `(配置, Some(备份路径))` 表示**本次发生了恢复**，调用方**必须**响亮记录（`main.rs`
/// Phase 1 的 tracing 尚未初始化，故用 `eprintln!`：现场 `journalctl` 可见）。
pub fn load_config_with_backup_recovery(path: &Path) -> Result<(CoreConfig, Option<PathBuf>), String> {
    // 正常路径走 `CoreConfig::load`（与 Phase 1 历史行为逐字一致：真源在 ⇒ 行为不变）
    match CoreConfig::load(path) {
        Ok(c) => Ok((c, None)),
        Err(load_err) => {
            let bak = sibling(path, ".bak");
            let text = std::fs::read_to_string(&bak).map_err(|bak_err| {
                format!(
                    "加载 {} 失败（{load_err}）；备份 {} 也不可读（{bak_err}）",
                    path.display(),
                    bak.display()
                )
            })?;
            let cfg: CoreConfig = serde_yaml::from_str(&text).map_err(|e| {
                format!(
                    "真源 {} 不可读（{load_err}），备份 {} 解析失败（{e}）——不拿坏内容覆盖真源",
                    path.display(),
                    bak.display()
                )
            })?;
            // 恢复：把备份文本写回真源（同目录 tmp → rename；真源此刻不存在 ⇒ 无备份步）
            let tmp = sibling(path, ".tmp");
            let write_back = || -> Result<(), String> {
                {
                    let mut f = std::fs::File::create(&tmp)
                        .map_err(|e| format!("创建 {} 失败: {e}", tmp.display()))?;
                    std::io::Write::write_all(&mut f, text.as_bytes())
                        .map_err(|e| format!("写 {} 失败: {e}", tmp.display()))?;
                    f.sync_all()
                        .map_err(|e| format!("fsync {} 失败: {e}", tmp.display()))?;
                }
                std::fs::rename(&tmp, path).map_err(|e| {
                    let _ = std::fs::remove_file(&tmp);
                    format!("rename {} → {} 失败: {e}", tmp.display(), path.display())
                })
            };
            write_back()?;
            Ok((cfg, Some(bak)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console_audit::{AuditError, AuditIntent, ConsoleAuditSink};
    use crate::testutil::TempDir;
    use mupc_display_proto::{PatchSource, REPLAY_WINDOW_MS};
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use std::time::Duration;

    /// 现场样例 yaml：**含注释 + 含 legacy `web_api:` 段 + 含未建模键 + 非字母序**。
    ///
    /// ⚠️ 必须含 `CoreConfig` 里**没有 `#[serde(default)]` 的顶层段**（`system` / `intercore` /
    /// `ai_engine` / `plugins`）——缺一个就解析不了，而解析不了的文件**根本不可能**出现在现场
    /// （`mupcd` 启动期就会失败）。测试样例若"偷懒少写段"，测的就不是真实输入。
    ///
    /// 单元 K：`web_api:` **不再是**必需段（该字段随 crate 删除，见 `core_config.rs` 记）；此处
    /// **故意保留**它——它现在是本样例里「**现场 legacy 段**」的实证，本文件的字节级用例正好证明
    /// 「保存配置不会抹掉它」（设计 §7.3 兼容性主张后半）。
    const YAML: &str = r#"# 现场 yaml（注释必须保留）
version: "0.1.0"
system:
  log_level: info        # 现场调过
  log_dir: /var/log/mupc
intercore:
  host: 10.0.0.7
  port: 9100   # PCS 端口
  heartbeat_interval_sec: 5
  reconnect_interval_sec: 3
  transport: tcp
web_api:                   # 现场 legacy 段（单元 K 后 CoreConfig 已无此字段）：保存必须逐字保留
  listen_addr: 0.0.0.0:8080
  tls_cert: null
  tls_key: null
ai_engine: {}
plugins: {}
gateway:
  listen_addr: 0.0.0.0
  listen_port: 2404
  future_key: keep-me      # 未建模键（字段表里没有它）
legacy_top:                # 未建模的**顶层段**（CoreConfig 不认，必须原样保留）
  a: 1
"#;

    /// 与 [`YAML`] 同形，但**故意不含 `gateway:` 段**（"不可定位 ⇒ 整体回写"回退路径的输入；
    /// 其余必需段齐全，否则它连 `CoreConfig` 都解析不出来，不构成现场输入）。
    ///
    /// 单元 K：这里保留 `web_api:`（现为**未建模段**）——整体回写**确实会丢掉它**，而这正是
    /// 回退路径已登记的代价（`WriteMode::FullRewrite` 在回执/审计里可见，EDGE-23）。
    /// 不在此断言它"应当幸存"：那会与"未建模段在整体回写下整段消失"的既有结论自相矛盾。
    const YAML_NO_GATEWAY: &str = "# 注释会丢\nversion: \"0.1.0\"\nsystem:\n  log_level: info\n\
                    intercore:\n  host: 10.0.0.7\n  port: 9100\n\
                    web_api:\n  tls_cert: null\n  tls_key: null\n\
                    ai_engine: {}\nplugins: {}\n";

    /// 「恢复默认值」用例的输入：**7 个可写键全部与默认值不同** ⇒ 一次重置必须全部落盘
    /// （只改一个键的样例测不出"全量应用"，也测不出"逐字段审计条目"）。
    ///
    /// 单元 K：保留 `web_api:`（现为**未建模段**）——同 [`YAML`]，用最少的字节顺带覆盖
    /// "批量写入也不得碰它"。
    const YAML_ALL_DIFFERENT: &str = "\
# 恢复默认值用例
version: \"0.1.0\"
system:
  log_level: debug
intercore:
  host: 10.0.0.7
  port: 9999
  heartbeat_interval_sec: 9
  reconnect_interval_sec: 8
web_api:
  tls_cert: null
  tls_key: null
ai_engine: {}
plugins: {}
gateway:
  listen_addr: 192.168.3.10
  listen_port: 2405
";

    /// 可注入的审计桩（**计数**是"没有重复执行"的可观测证据；`fail` 是 fail-closed 的注入点；
    /// `fail_outcome` 单点注入"结果审计失败"；`gate` 把首个请求**按在 intent 里**以稳定复现
    /// "处理中"（覆盖 `Busy`）——用门闸而不是 `sleep`：`sleep` 的时序在负载下不可靠，
    /// 而"等对端观察到 `entered`"是确定性同步）。
    #[derive(Default)]
    struct ProbeSink {
        intents: AtomicUsize,
        outcomes: AtomicUsize,
        fail: AtomicBool,
        fail_outcome: AtomicBool,
        /// 门闸开关 + 已进入 intent 的标志 + 放行标志。
        gate: AtomicBool,
        entered: AtomicBool,
        release: AtomicBool,
        /// 已写入的结果审计条目（**取值断言**的数据源：评审阻塞 2 / 建议 6.3 要求断言**取值**
        /// 而不是只数条数 ⇒ 桩必须把条目留下来，否则"审计里写了什么"无法被验证）。
        entries: Mutex<Vec<ConsoleAuditEntry>>,
    }

    impl ProbeSink {
        fn failing() -> Self {
            let s = Self::default();
            s.fail.store(true, Ordering::SeqCst);
            s
        }
        fn gated() -> Self {
            let s = Self::default();
            s.gate.store(true, Ordering::SeqCst);
            s
        }

        /// 已写入的结果审计条目（按写入顺序）。
        fn entries(&self) -> Vec<ConsoleAuditEntry> {
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
        }
    }

    impl ConsoleAuditSink for ProbeSink {
        fn record_intent(&self, _i: &AuditIntent) -> Result<(), AuditError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err("注入失败：审计目录不可写".to_string());
            }
            if self.gate.load(Ordering::SeqCst) {
                self.entered.store(true, Ordering::SeqCst);
                // 最多等 5 s：若用例逻辑坏了也不**挂死**（挂死的用例在 CI 上表现为整体超时，
                // 定位成本远高于一条失败断言——与 `console_host` 的守卫同款理由）。
                let t0 = std::time::Instant::now();
                while !self.release.load(Ordering::SeqCst)
                    && t0.elapsed() < Duration::from_secs(5)
                {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            self.intents.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn record_outcome(&self, e: &ConsoleAuditEntry) -> Result<(), AuditError> {
            if self.fail.load(Ordering::SeqCst) || self.fail_outcome.load(Ordering::SeqCst) {
                return Err("注入失败：审计文件不可写".to_string());
            }
            self.entries
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(e.clone());
            self.outcomes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// 测试装配：临时目录里的 yaml + 内存副本 + 桩审计。
    struct Harness {
        _dir: TempDir,
        path: PathBuf,
        core: Arc<RwLock<CoreConfig>>,
        sink: Arc<ProbeSink>,
        /// `Arc` 是为了让并发用例能 `tokio::spawn` 出真正的并发任务（单线程 runtime 下
        /// `join!` 会把两个 future 排在**同一线程**上跑，压根测不到"处理中"）。
        svc: Arc<ConfigService>,
    }

    fn harness_with(sink: Arc<ProbeSink>, yaml: &str) -> Harness {
        harness_with_hot(sink, yaml, HotApply::new(None))
    }

    /// 同上，但**注入指定的 `HotApply`**（`log_load` 用例需要"真热生效"的装配形态——
    /// 默认的 `HotApply::new(None)` 会让 `system.log_level` 也落到 `RestartRequired`，
    /// 那样就测不出"真生效的键**不得**被审计写成需重启"这条反向断言）。
    fn harness_with_hot(sink: Arc<ProbeSink>, yaml: &str, hot: HotApply) -> Harness {
        let dir = TempDir::new("cfg");
        let path = dir.write("mupc_core_config.yaml", yaml);
        let cfg: CoreConfig = serde_yaml::from_str(yaml).expect("样例 yaml 必须可解析");
        let core = Arc::new(RwLock::new(cfg));
        let svc = Arc::new(ConfigService::new(path.clone(), core.clone(), sink.clone(), hot));
        Harness {
            _dir: dir,
            path,
            core,
            sink,
            svc,
        }
    }

    /// 真实的 `tracing` reload 句柄（不初始化全局订阅者：句柄只是 layer 的遥控器）——
    /// 让 `system.log_level` 成为**真热生效**的键（与 `hot_apply` 的单测同款构造）。
    #[allow(clippy::type_complexity)]
    fn reload_handle() -> (
        tracing_subscriber::reload::Layer<
            tracing_subscriber::EnvFilter,
            tracing_subscriber::Registry,
        >,
        crate::hot_apply::LogReloadHandle,
    ) {
        tracing_subscriber::reload::Layer::new(tracing_subscriber::EnvFilter::new("info"))
    }

    fn harness() -> Harness {
        harness_with(Arc::new(ProbeSink::default()), YAML)
    }

    fn request(rid: &str, changes: &[(&str, Value)], from: PatchSource) -> ControlRequest<ConfigPatch> {
        let mut map = serde_json::Map::new();
        for (k, v) in changes {
            map.insert(k.to_string(), v.clone());
        }
        ControlRequest::new(rid, now_ms(), "apply", ConfigPatch { changes: map, from })
    }

    fn ok_request(rid: &str, changes: &[(&str, Value)]) -> ControlRequest<ConfigPatch> {
        request(rid, changes, PatchSource::Edit)
    }

    fn disk(h: &Harness) -> String {
        std::fs::read_to_string(&h.path).unwrap()
    }

    /// 当前内存副本生成的视图（GET 的同款路径）。
    async fn view_now(h: &Harness) -> ConfigView {
        let guard = h.core.read().await;
        h.svc.view(&guard)
    }

    // ═══ ① 正常保存 ═══════════════════════════════════════════════════════════

    /// 正常保存：`Ok` + `applied` 是新视图 + `audit_id` 非空 + 文件只改目标行 + 内存副本已变 +
    /// `.bak` 是保存前的内容。
    #[tokio::test]
    async fn apply_persists_atomically_and_returns_the_new_view() {
        let h = harness();
        let before_text = disk(&h);
        let resp = h
            .svc
            .apply(&ok_request("rid-1", &[("intercore.port", serde_json::json!(2405))]))
            .await;

        assert!(resp.ok, "正常保存必须成功: {resp:?}");
        assert_eq!(resp.code, ControlCode::Ok);
        assert!(!resp.duplicate);
        let view = resp.applied.expect("成功回执必须带新视图");
        assert_eq!(view.revision, 1, "revision 递增");
        assert_eq!(view.write_mode, WriteMode::TextPreserve);
        let port = view
            .groups
            .iter()
            .flat_map(|g| g.fields.iter())
            .find(|f| f.key == "intercore.port")
            .unwrap();
        assert_eq!(port.value, serde_json::json!(2405), "回执须带**新**值（供 UI 立即刷新）");
        assert!(resp.audit_id.is_some(), "成功回执必须带 audit_id");

        // 文件：只有目标行变化，其余**逐字节**不变
        let after_text = disk(&h);
        assert_eq!(
            after_text,
            before_text.replace("port: 9100", "port: 2405"),
            "落盘必须走保留式编辑（注释 / legacy 段 / 未建模键逐字保留）"
        );
        assert!(after_text.contains("future_key: keep-me"));

        // 原子落的中间物：`.bak` = 保存前内容；`.tmp` **不残留**
        let bak = std::fs::read_to_string(sibling(&h.path, ".bak")).unwrap();
        assert_eq!(bak, before_text, ".bak 必须是保存前的原文");
        assert!(!sibling(&h.path, ".tmp").exists(), "不得残留 .tmp");

        // 内存副本（GET 的真源）已经变
        assert_eq!(h.core.read().await.intercore.port, 2405);
        // 审计：1 条 intent + 2 条 outcome（1 字段 + 1 write_mode）
        assert_eq!(h.sink.intents.load(Ordering::SeqCst), 1);
        assert_eq!(h.sink.outcomes.load(Ordering::SeqCst), 2);
    }

    /// **配置向后兼容 ②「写了不丢」**（设计 §7.2 Step 4 末段 / §7.3 兼容性主张后半，单元 K）。
    ///
    /// 端到端（真 `ConfigService` + 真临时文件）：输入含**现场 legacy `web_api:` 段**（且该段
    /// 带行内注释 —— 比原样更严的形态），改一个**无关键**后断言：
    /// 1. 该段连同其注释**逐字节**出现在保存后的文件里（"能读"之外还要"写了不丢"）；
    /// 2. 整个文件除目标那一行外**逐行字节不变**（含注释行数守恒）；
    /// 3. 写模式是 `TextPreserve`（**不是** `FullRewrite`——后者才会丢未建模段）。
    ///
    /// 与 `core_config.rs::legacy_web_api_section_still_loads_and_is_ignored` 合起来 = 完整主张：
    /// **"能读"且"写了不丢"**。前者单测"忽略未知段不报错"，本条单测"保存不抹掉它"。
    ///
    /// **改什么会让本条变红**：把写路径改回"整棵树序列化回写"（`web_api` 已不在模型内 ⇒
    /// 第 1 条立刻红）；或把保留式编辑降级成 FullRewrite ⇒ 第 3 条红。
    #[tokio::test]
    async fn legacy_web_api_section_survives_a_real_save_verbatim() {
        let yaml = YAML; // 含 legacy `web_api:` 段（带行内注释）+ 未建模键 + 未建模顶层段
        assert!(
            yaml.contains("web_api:                   # 现场 legacy 段"),
            "本用例前提：样例里必须有带注释的 legacy `web_api:` 段"
        );
        let h = harness_with(Arc::new(ProbeSink::default()), yaml); // 解析失败即 panic ⇒ 兼容①
        let before = disk(&h);
        let block_before = legacy_block(&before);
        assert_eq!(
            block_before,
            "web_api:                   # 现场 legacy 段（单元 K 后 CoreConfig 已无此字段）：保存必须逐字保留\n  listen_addr: 0.0.0.0:8080\n  tls_cert: null\n  tls_key: null\n",
            "取块函数必须先取对（否则下面的断言是恒真）"
        );

        let resp = h
            .svc
            .apply(&ok_request("rid-legacy", &[("intercore.port", serde_json::json!(2405))]))
            .await;
        assert!(resp.ok, "保存必须成功: {resp:?}");
        assert_eq!(
            resp.applied.unwrap().write_mode,
            WriteMode::TextPreserve,
            "本用例前提：走保留式编辑（回退路径本就会丢未建模段，已由 EDGE-23 登记）"
        );

        let after = disk(&h);
        assert_eq!(
            legacy_block(&after),
            block_before,
            "legacy `web_api:` 段（含行内注释）必须**逐字节**保留——不得被抹掉、不得被重排"
        );
        // 逐行比：恰有目标行不同（其余含注释 / 未建模段 / 行尾全不动）
        let (a, b): (Vec<&str>, Vec<&str>) =
            (before.split_inclusive('\n').collect(), after.split_inclusive('\n').collect());
        assert_eq!(a.len(), b.len(), "行数不得变化");
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        assert_eq!(diff.len(), 1, "恰有 1 行被替换，实得 {diff:?}");
        assert_eq!(
            b[diff[0]],
            "  port: 2405   # PCS 端口\n",
            "且被替换的就是目标行（行内注释与空格原样）"
        );
        // 内存副本同步（写 A 读 B 的静默失实在此不适用）
        assert_eq!(h.core.read().await.intercore.port, 2405);
    }

    /// 从 yaml 文本里**逐字**取出 `web_api:` 段（含其后缩进行，到下一个顶层键或文末为止）。
    fn legacy_block(text: &str) -> String {
        let mut out = String::new();
        let mut inside = false;
        for line in text.split_inclusive('\n') {
            let top_level = !line.starts_with(' ') && !line.starts_with('\t') && !line.starts_with('#');
            if top_level {
                if line.starts_with("web_api:") {
                    inside = true;
                } else if inside {
                    break;
                }
            }
            if inside {
                out.push_str(line);
            }
        }
        out
    }

    /// 保存一个"值没变"的字段 ⇒ **不写盘**（mtime / 字节都不动），但仍是成功回执。
    #[tokio::test]
    async fn saving_an_unchanged_value_does_not_touch_the_file() {
        let h = harness();
        let before_text = disk(&h);
        let resp = h
            .svc
            .apply(&ok_request("rid-same", &[("intercore.port", serde_json::json!(9100))]))
            .await;
        assert!(resp.ok);
        assert_eq!(resp.applied.unwrap().revision, 0, "没写盘 ⇒ revision 不动");
        assert_eq!(disk(&h), before_text);
        assert!(!sibling(&h.path, ".tmp").exists());
        assert_eq!(h.sink.intents.load(Ordering::SeqCst), 0, "无改动 ⇒ 连 intent 都不该写");
    }

    // ═══ ② fail-closed ════════════════════════════════════════════════════════

    /// **审计不可写 ⇒ 拒绝执行且值未变**（设计 §3.3 / EDGE-18）。
    ///
    /// "值未变"用**三重可观测证据**：文件字节、内存副本字段、`revision` —— 三者任一被改动
    /// 都会让本条红（只查其中一处不足以证明"没有半生效"）。
    #[tokio::test]
    async fn audit_failure_is_fail_closed_and_changes_nothing() {
        let h = harness_with(Arc::new(ProbeSink::failing()), YAML);
        let before_text = disk(&h);
        let before_port = h.core.read().await.intercore.port;

        let resp = h
            .svc
            .apply(&ok_request("rid-audit", &[("intercore.port", serde_json::json!(2405))]))
            .await;

        assert!(!resp.ok, "审计不可写 ⇒ 不得 ok=true");
        assert_eq!(resp.code, ControlCode::AuditUnavailable);
        assert!(resp.code.is_rejection());
        assert!(resp.applied.is_none(), "fail-closed ⇒ applied 必须为 None（操作未生效）");
        assert!(resp.audit_id.is_none(), "连审计号都没有 ⇒ 不得编一个");
        // ⚠️ **口径变更（回执用字网）**：`message` 固定为 `receipt::AUDIT_UNAVAILABLE`
        // （与渲染端 EDGE-18 上屏文案同义）。**原因串不再进 `message`** —— 它是外部装配错误串
        // （`BrokenSink` 注入的「注入失败：审计目录不可写」），含 cmap 外的字 ⇒ 真机豆腐块。
        // 排障信息没丢：它由 `audit_unavailable` 的 `tracing::error!` 承载（P3 日志页可查）。
        assert_eq!(
            resp.message,
            crate::console_host::receipt::AUDIT_UNAVAILABLE,
            "回执文案必须是固定的 cmap 内串"
        );
        assert!(
            !resp.message.contains("注入失败"),
            "外部原因串**不得**漏进 message（它在 cmap 外的字上屏即豆腐块）: {}",
            resp.message
        );

        // 三重证据
        assert_eq!(disk(&h), before_text, "文件不得被改动");
        assert_eq!(h.core.read().await.intercore.port, before_port, "内存副本不得被改动");
        assert_eq!(h.svc.revision(), 0, "revision 不得递增");
        assert!(!sibling(&h.path, ".tmp").exists(), "连临时文件都不该产生");
    }

    /// **结果审计失败**（intent 成功、执行成功、outcome 写不成）⇒ `ok=true` + **无** `audit_id`
    /// + 改动**确实生效**（不回滚、不谎报失败），且 `revision` 照常递增。
    ///
    /// **改什么会让本条变红**：把"outcome 失败"改成回滚 / 改成 `ok=false` ⇒ 第 1、2 条红；
    /// 把 `audit_id=None` 改成随便编一个 id ⇒ 第 3 条红（现场会拿假审计号去审计库里查）。
    #[tokio::test]
    async fn outcome_audit_failure_keeps_the_change_and_reports_no_audit_id() {
        let sink = Arc::new(ProbeSink::default());
        let h = harness_with(sink.clone(), YAML);
        sink.fail_outcome.store(true, Ordering::SeqCst);
        let resp = h
            .svc
            .apply(&ok_request("rid-o1", &[("intercore.port", serde_json::json!(2405))]))
            .await;
        assert!(resp.ok, "改动已发生 ⇒ 不得谎报失败");
        assert_eq!(resp.code, ControlCode::Ok);
        assert!(resp.applied.is_some(), "已生效 ⇒ 必须带新视图");
        assert!(resp.audit_id.is_none(), "**没有**审计条目 ⇒ 不得编一个 id");
        assert_eq!(h.core.read().await.intercore.port, 2405, "改动必须已生效（不回滚）");
        assert_eq!(h.svc.revision(), 1);
        assert!(disk(&h).contains("port: 2405"), "文件也已落盘");
        assert_eq!(sink.intents.load(Ordering::SeqCst), 1, "intent 是写成了的");
        assert_eq!(sink.outcomes.load(Ordering::SeqCst), 0, "outcome 写不成（本用例前提）");
    }

    // ═══ ③ 幂等 ═══════════════════════════════════════════════════════════════

    /// 同 `(op, request_id)` 第二次 ⇒ 返回**首次原始回执** + `duplicate=true`，且**未再执行**
    /// （证据：intent 计数仍为 1、revision 仍为 1、文件字节未变）。
    #[tokio::test]
    async fn duplicate_request_replays_the_first_receipt_without_re_executing() {
        let h = harness();
        let req = ok_request("rid-dup", &[("intercore.port", serde_json::json!(2405))]);
        let first = h.svc.apply(&req).await;
        let text_after_first = disk(&h);
        let intents_after_first = h.sink.intents.load(Ordering::SeqCst);

        // 第二次：**同一个 request_id**（模拟渲染端 5 s 超时后的 `retry` 原样重发）
        let second = h.svc.apply(&req).await;

        assert!(second.ok && second.duplicate, "重复请求必须 duplicate=true 且 ok 不变");
        assert_eq!(second.code, first.code);
        assert_eq!(second.audit_id, first.audit_id, "复用首次审计记录，不重复留痕");
        assert_eq!(
            second.applied.unwrap().revision,
            first.applied.unwrap().revision,
            "回的是**首次**视图（revision 不再涨）"
        );
        assert_eq!(h.svc.revision(), 1, "只真正写过一次");
        assert_eq!(
            h.sink.intents.load(Ordering::SeqCst),
            intents_after_first,
            "**没有第二次 intent** ⇒ 管线没有重跑到执行段（这就是「未再执行」的证据）"
        );
        assert_eq!(disk(&h), text_after_first, "文件不得被第二次请求再写一遍");

        // 首次是**失败**时，重放也必须是失败（不得因重放变成功）
        let bad = ok_request("rid-bad", &[("intercore.port", serde_json::json!(99999))]);
        let f1 = h.svc.apply(&bad).await;
        let f2 = h.svc.apply(&bad).await;
        assert!(!f1.ok && !f2.ok);
        assert_eq!(f2.code, ControlCode::RejectedValidation);
        assert!(f2.duplicate);
    }

    /// "处理中" ⇒ `Busy`（不排队、**不重复执行**）。
    ///
    /// 复现方式：桩审计的**门闸**把首个请求按在管线中段（此刻它已占位但未完成），
    /// 主线程随后发第二个同键请求 ⇒ 必须 `Busy`，而**不是**"第二个也执行一遍"。
    ///
    /// **改什么会让本条变红**：把 `Reserve::InFlight` 分支写成 `Fresh`（即"处理中也放行"）
    /// ⇒ 第 1 条红、且 `revision` 会变成 2。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_same_request_id_gets_busy_not_a_second_execution() {
        let sink = Arc::new(ProbeSink::gated());
        let h = harness_with(sink.clone(), YAML);
        let req = ok_request("rid-busy", &[("intercore.port", serde_json::json!(2405))]);

        let svc = h.svc.clone();
        let req_a = req.clone();
        let a = tokio::spawn(async move { svc.apply(&req_a).await });
        // 等 A **真的进了** intent 段（确定性同步，不靠 sleep 猜时序）
        let t0 = std::time::Instant::now();
        while !sink.entered.load(Ordering::SeqCst) {
            assert!(t0.elapsed() < Duration::from_secs(5), "A 未进入 intent 段");
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        // 此刻 A 已占位（`InFlight`）且未完成 ⇒ 第二个同键请求必须 Busy
        let b = h.svc.apply(&req).await;
        assert_eq!(b.code, ControlCode::Busy, "处理中必须回 Busy，实得 {:?}", b.code);
        assert!(!b.ok && b.applied.is_none(), "Busy 不得带生效值");
        assert!(!b.duplicate, "Busy 不是幂等命中（首次还没完成）");
        sink.release.store(true, Ordering::SeqCst);
        let a = a.await.unwrap();
        assert!(a.ok && !a.duplicate, "首次必须成功且非重复");
        assert_eq!(h.svc.revision(), 1, "只执行了一次");
        assert_eq!(sink.intents.load(Ordering::SeqCst), 1, "intent 只写了一次");
    }

    // ═══ ④ 防重放窗口 ═════════════════════════════════════════════════════════

    /// 窗口外（早于 30 s / 晚于 30 s）⇒ 拒；**窗口内**（提前 5 s 留余量）⇒ 放行。
    ///
    /// 两侧都测：只测"越界被拒"无法排除"把窗口开得极小"（那会让正常重试全被误杀，
    /// 渲染端 `console.rs::retry` 的 5 s 超时重试就废了）。窗口边界 ±0 的**精确**判据由契约
    /// 自己的单测钉死（`display-proto/src/control.rs::envelope_replay_window_boundary`），
    /// 此处不重复、也不赌"服务端读钟与测试读钟差 0 ms"（那不可能是稳定断言）。
    #[tokio::test]
    async fn replay_window_rejects_outside_and_accepts_inside() {
        let h = harness();
        let now = now_ms();
        // 太旧（超出窗口 5 s）⇒ 拒
        let mut stale = ok_request("rid-old", &[("intercore.port", serde_json::json!(2405))]);
        stale.issued_at_ms = now - REPLAY_WINDOW_MS - 5_000;
        let r = h.svc.apply(&stale).await;
        assert!(!r.ok && r.code == ControlCode::RejectedValidation, "过期请求必须拒");
        // 原因**换个渠道但一个字没少**：`message` 只给 `receipt::BAD_ENVELOPE`（cmap 内），
        // 契约的英文原因串（含小写字母 ⇒ cmap 外）落在 `field_errors`（结构化渠道）。
        assert_eq!(r.message, crate::console_host::receipt::BAD_ENVELOPE);
        let envelope_reason = r.field_errors[0].reason.as_str();
        assert!(
            envelope_reason.contains("replay window") || envelope_reason.contains("issued_at_ms"),
            "原因须可定位（`field_errors` 是它现在的落点）: {envelope_reason}"
        );
        // 太超前（未来 90 s）⇒ 拒
        let mut future = ok_request("rid-future", &[("intercore.port", serde_json::json!(2405))]);
        future.issued_at_ms = now + REPLAY_WINDOW_MS + 60_000;
        let rf = h.svc.apply(&future).await;
        assert!(!rf.ok, "超前请求必须拒");
        assert_eq!(rf.code, ControlCode::RejectedValidation);
        assert_eq!(disk(&h), YAML, "两次拒绝都不得有副作用");

        // 窗口内（提前 5 s）⇒ 放行：证明窗口**没被开小**
        let mut inside = ok_request("rid-in", &[("intercore.port", serde_json::json!(2405))]);
        inside.issued_at_ms = now_ms().saturating_sub(REPLAY_WINDOW_MS - 5_000);
        let ri = h.svc.apply(&inside).await;
        assert!(ri.ok, "窗口内必须放行（否则渲染端 5 s 超时重试会被误杀）: {ri:?}");
        assert_eq!(disk(&h).matches("port: 2405").count(), 1);
    }

    /// `op` 与端点不符（防误路由）与空 `request_id` 也走信封校验 ⇒ 拒绝。
    #[tokio::test]
    async fn op_mismatch_and_empty_request_id_are_rejected() {
        let h = harness();
        let mut wrong_op = ok_request("rid-op", &[("intercore.port", serde_json::json!(2405))]);
        wrong_op.op = "release".to_string();
        let r = h.svc.apply(&wrong_op).await;
        assert!(!r.ok && r.code == ControlCode::RejectedValidation);
        // 误路由仍被**点名**，只是换了渠道：`message` 固定（cmap 内），原因在 `field_errors`。
        assert_eq!(r.message, crate::console_host::receipt::BAD_ENVELOPE);
        assert!(
            r.field_errors.iter().any(|e| e.reason.contains("release")),
            "原因须点名误路由: {:?}",
            r.field_errors
        );

        let mut empty = ok_request("   ", &[("intercore.port", serde_json::json!(2405))]);
        empty.request_id = "  ".to_string();
        let r = h.svc.apply(&empty).await;
        assert!(!r.ok && r.code == ControlCode::RejectedValidation);
        assert_eq!(h.core.read().await.intercore.port, 9100, "无副作用");
    }

    // ═══ ⑤ 字段校验（全量通过才执行）═══════════════════════════════════════════

    /// **一条合法 + 一条非法 ⇒ 一条都不生效**（"保存 = 全部"口径），且 `field_errors` **只**列
    /// 非法的那条（合法的不得被误标红）。
    ///
    /// **改什么会让本条变红**：把"先全量校验再执行"改成"边校验边改" ⇒ 第 2/3/4 条红。
    #[tokio::test]
    async fn one_bad_field_blocks_the_whole_batch() {
        let h = harness();
        let before_text = disk(&h);
        let resp = h
            .svc
            .apply(&ok_request(
                "rid-mix",
                &[
                    ("system.log_level", serde_json::json!("debug")), // 合法
                    ("intercore.port", serde_json::json!(0)),         // 越界（端口下限 1）
                ],
            ))
            .await;
        assert!(!resp.ok && resp.code == ControlCode::RejectedValidation);
        assert_eq!(resp.field_errors.len(), 1, "只标红**非法**字段: {:?}", resp.field_errors);
        assert_eq!(resp.field_errors[0].field, "intercore.port");
        assert!(resp.field_errors[0].reason.contains("越界"));
        assert!(resp.applied.is_none(), "整批拒绝 ⇒ 不得带生效值");
        // 合法字段也**没有**生效
        assert_eq!(h.core.read().await.system.log_level, "info", "合法字段不得被单独应用");
        assert_eq!(disk(&h), before_text, "文件不得被改动");
        assert_eq!(h.svc.revision(), 0);
        // 失败留痕（PL-1：成功与失败均写）
        assert_eq!(h.sink.outcomes.load(Ordering::SeqCst), 1, "失败也要写一条审计");
    }

    /// 逐类非法值：未知键 / 只读键 / 非法 IP / 非法枚举 —— **逐条**都要有 field_errors。
    #[tokio::test]
    async fn each_invalid_field_kind_is_reported_field_by_field() {
        let cases: Vec<(&str, Value, &str)> = vec![
            ("no.such.key", serde_json::json!(1), "未知字段"),
            ("display.bind_addr", serde_json::json!("0.0.0.0"), "只读"),
            ("gateway.listen_addr", serde_json::json!("999.1.1.1"), "IPv4"),
            ("system.log_level", serde_json::json!("trace"), "不在允许选项内"),
            ("intercore.host", serde_json::json!(7), "IPv4"),
        ];
        for (key, value, want) in cases {
            let h = harness();
            let resp = h.svc.apply(&ok_request("rid-inv", &[(key, value)])).await;
            assert!(!resp.ok, "`{key}` 非法值必须拒");
            assert_eq!(resp.code, ControlCode::RejectedValidation, "`{key}`");
            assert_eq!(resp.field_errors.len(), 1, "`{key}` 逐字段原因缺失: {resp:?}");
            assert_eq!(resp.field_errors[0].field, key);
            assert!(
                resp.field_errors[0].reason.contains(want),
                "`{key}` 原因须含「{want}」，实得 {}",
                resp.field_errors[0].reason
            );
            assert_eq!(disk(&h), YAML, "`{key}`：拒绝时文件必须原样");
        }
    }

    // ═══ ⑥ 保留式 vs 整体回写（D16 / EDGE-23）═════════════════════════════════

    /// **不可定位 ⇒ 整体回写，且 `WriteMode::FullRewrite` 在回执里可见**，注释确实丢失，
    /// 而目标键**确实生效**（回退不是"放弃"，是"降级但完成"）。
    #[tokio::test]
    async fn unlocatable_key_falls_back_to_full_rewrite_visibly() {
        // 这份 yaml **故意不含** `gateway:` 段 ⇒ 改 gateway.listen_port 无法定位
        // （其余必需段齐全，否则它连 `CoreConfig` 都解析不出来，不构成现场输入）
        let yaml = YAML_NO_GATEWAY;
        let h = harness_with(Arc::new(ProbeSink::default()), yaml);
        let resp = h
            .svc
            .apply(&ok_request(
                "rid-fb",
                &[("gateway.listen_port", serde_json::json!(2405))],
            ))
            .await;
        assert!(resp.ok, "回退路径仍是**成功**（值已落盘）: {resp:?}");
        let view = resp.applied.unwrap();
        assert_eq!(
            view.write_mode,
            WriteMode::FullRewrite,
            "EDGE-23：回执必须显式声明整体重写（UI 据此 Toast）"
        );
        // ⚠️ 文案由「已整体重写配置文件」改为「原有文字已不存在 …」（`整`/`体`/`写` 三字都不在
        // cmap 内，逐字网抓出来的）；语义不变，且与渲染端 `TEXT_TOAST_FULL_REWRITE` 同款措辞。
        assert!(
            resp.message.contains("原有文字已不存在"),
            "回执文案须明示降级: {}",
            resp.message
        );
        let text = disk(&h);
        assert!(text.contains("listen_port: 2405"), "目标键必须真的写进去");
        assert!(!text.contains("# 注释会丢"), "整体回写 ⇒ 注释确实丢失（降级有测试断言）");
        // 内存副本与落盘一致
        assert_eq!(h.core.read().await.gateway.listen_port, 2405);
        // 下一次 GET 会看到 FullRewrite（写模式真源）
        assert_eq!(h.svc.last_write_mode(), WriteMode::FullRewrite);
    }

    /// 保留式路径下**多项批量**：恰有 N 行被替换，其余逐字节不变。
    #[tokio::test]
    async fn batch_save_replaces_exactly_the_target_lines() {
        let h = harness();
        let before_text = disk(&h);
        let resp = h
            .svc
            .apply(&ok_request(
                "rid-batch",
                &[
                    ("system.log_level", serde_json::json!("debug")),
                    ("gateway.listen_port", serde_json::json!(2405)),
                    ("intercore.host", serde_json::json!("192.168.3.21")),
                ],
            ))
            .await;
        assert!(resp.ok);
        let after_text = disk(&h);
        let (a, b): (Vec<&str>, Vec<&str>) =
            (before_text.lines().collect(), after_text.lines().collect());
        assert_eq!(a.len(), b.len(), "行数不得变化");
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        assert_eq!(diff.len(), 3, "恰有 3 行被替换，实得 {diff:?}");
        assert_eq!(b[diff[0]], "  log_level: debug        # 现场调过");
    }

    /// **往返一致性**（设计 §4.3.2.1 往返单测 4）：`CoreConfig` → yaml → `CoreConfig` 逐字段相等。
    ///
    /// 这是整体回写路径的**地基**：`Serialize` 一旦与 `Deserialize` 不对称（`skip_serializing_if` /
    /// `default` 配错），字段会被静默写丢——本用例让它当场红。
    #[test]
    fn core_config_serialize_roundtrip_preserves_every_field() {
        let cfg: CoreConfig = serde_yaml::from_str(YAML).unwrap();
        let text = serde_yaml::to_string(&cfg).unwrap();
        // ① 能解回来
        let back: CoreConfig = serde_yaml::from_str(&text).unwrap();
        // ② 逐字段相等（用 `Serialize` 的 JSON 形态比对：字段级、可定位到具体键）
        let a = serde_json::to_value(&cfg).unwrap();
        let b = serde_json::to_value(&back).unwrap();
        assert_eq!(a, b, "CoreConfig 序列化往返必须逐字段相等（丢字段会在这里暴露）");
        // ③ 对照：只比较 FIELDS 的 9 个键也全部相等（更强的可读断言）
        for m in FIELDS {
            assert_eq!((m.current)(&cfg), (m.current)(&back), "字段 `{}` 往返丢失", m.key);
        }
        // ④ 二次序列化稳定（幂等）
        assert_eq!(serde_yaml::to_string(&back).unwrap(), text);
    }

    /// 写后自检**真的会拦**：直接把文本与"预期配置"错配 ⇒ 必须 Err（否则自检是摆设）。
    #[test]
    fn self_post_check_rejects_any_mismatch() {
        let cfg: CoreConfig = serde_yaml::from_str(YAML).unwrap();
        let text = serde_yaml::to_string(&cfg).unwrap();
        assert!(self_post_check(&text, &cfg).is_ok(), "同源文本必须通过自检");
        let mut other = cfg.clone();
        other.intercore.port = 1;
        assert!(self_post_check(&text, &other).is_err(), "字段不符必须被自检拦住");
        assert!(self_post_check("这不是 yaml: [", &cfg).is_err(), "不可解析必须被拦住");
    }

    // ═══ ⑦ 视图口径（GET / 回执共用）════════════════════════════════════════════

    /// 视图的 `revision` / `write_mode` 取服务状态（**不是**常量）；两次保存后递增。
    #[tokio::test]
    async fn view_revision_follows_successful_writes() {
        let h = harness();
        assert_eq!(view_now(&h).await.revision, 0);
        h.svc
            .apply(&ok_request("v1", &[("intercore.port", serde_json::json!(2405))]))
            .await;
        assert_eq!(view_now(&h).await.revision, 1);
        h.svc
            .apply(&ok_request("v2", &[("intercore.port", serde_json::json!(2406))]))
            .await;
        assert_eq!(view_now(&h).await.revision, 2);
        assert_eq!(view_now(&h).await.write_mode, WriteMode::TextPreserve);
    }

    // ═══ ⑧ G-2 整改（评审 3 阻塞 + 2 重要 + 6 建议）════════════════════════════════

    /// **阻塞 2 回归**：审计条目必须**自证生效结论** —— `RestartRequired` 的键在 `reason` 里
    /// 明写"需重启"，而**真热生效**的键**不得**被写成需重启（不然是反向失真）。
    ///
    /// 同时钉住**重要 4 ①**：后端**受理端**（回执 `message`）确实**有**这条真话——
    /// 用户在屏上看不到它是**渲染层**的缺口（本单元只登记不改）。**登记落点 = 设计 §4.3.5
    /// 末段「⚠️ 残余（如实登记）」+ UI 设计文档 **PD24**（`local-display/src/ui/pages/p2_config.rs:58`）**：
    /// `Toast` 文本区 400 px（`DOTS` 截断）⇒ 长回执的键名可能被截掉；回执 `message` 的用字
    /// （`项` / 全角括号等）不在字体码表控制面内。
    /// ⚠️ 原句写"见**交付报告**"——该报告**不随仓库分发**（全仓只有 `docs/superpowers/reports/`
    /// 下 2026-05-27 的两份 Phase1/2 报告）⇒ 悬空引用，K 收尾 Q-4 已改为就地写清依据。
    ///
    /// **改什么会让本条变红**：把 `outcome_entries` 的 `|(k, v, outcome)|` 改回
    /// `|(k, v, _)|`（丢弃 `ApplyOutcome`）⇒ 第 2 条断言红（`reason` 变回 `None`）；
    /// 把回执文案里的"需重启"删掉 ⇒ 第 1 条红。
    #[tokio::test]
    async fn audit_entries_self_evidence_the_restart_required_conclusion() {
        let (_layer, reload) = reload_handle();
        let sink = Arc::new(ProbeSink::default());
        // 真热生效的装配（有 reload handle ⇒ `system.log_level` 是 `Applied`）
        let h = harness_with_hot(sink.clone(), YAML, HotApply::new(Some(reload)));
        let resp = h
            .svc
            .apply(&ok_request(
                "rid-concl",
                &[
                    ("system.log_level", serde_json::json!("debug")), // 真热生效
                    ("gateway.listen_port", serde_json::json!(2405)), // 未接线 ⇒ 需重启
                ],
            ))
            .await;
        assert!(resp.ok, "{resp:?}");

        // ① 回执（后端受理端的真话）。⚠️ 点名用字段 **label**（`端口`）而非机器键名：
        //    后者含 cmap 外的字符（小写 `t` / `_`）⇒ 上屏即豆腐块，见 `restart_labels`。
        assert!(
            resp.message.contains("需重启"),
            "回执必须明写需重启（重要 4 ①）：{}",
            resp.message
        );
        assert!(
            resp.message.contains("端口"),
            "回执须点名**具体**哪些键需重启（`gateway.listen_port` 的 label）: {}",
            resp.message
        );
        assert!(
            !resp.message.contains("日志级别"),
            "真热生效的键（`system.log_level` 的 label）不得出现在需重启清单里: {}",
            resp.message
        );
        assert!(
            !resp.message.contains("gateway.listen_port"),
            "机器键名不得进回执（含缺字形字符）: {}",
            resp.message
        );

        // ② 审计条目（本次整改的核心：修复前这里恒 None）
        let entries = sink.entries();
        let find = |k: &str| {
            entries
                .iter()
                .find(|e| e.target == k)
                .unwrap_or_else(|| {
                    panic!(
                        "审计缺 `{k}` 条目（实得 targets={:?}）",
                        entries.iter().map(|e| e.target.as_str()).collect::<Vec<_>>()
                    )
                })
        };
        let gl = find("gateway.listen_port");
        let reason = gl.reason.as_deref().unwrap_or("");
        assert!(
            reason.contains("需重启"),
            "未接线键的审计条目必须**自证**需重启（阻塞 2），实得 reason={:?}",
            gl.reason
        );
        assert!(reason.contains("IEC 104"), "原因须可定位到该键: {reason}");
        assert_eq!(gl.result, AuditResult::Ok);
        assert_eq!(gl.before, Some(serde_json::json!(2404)), "before/after 仍是**标量**（P5 AU12）");
        assert_eq!(gl.after, Some(serde_json::json!(2405)));

        let ll = find("system.log_level");
        assert!(
            ll.reason.as_deref().is_none_or(|r| !r.contains("需重启")),
            "真热生效的键不得被审计写成需重启（反向失真）: {:?}",
            ll.reason
        );
    }

    /// **建议 6.3**：`config.write_mode` 审计条目要断言**取值**（不只是"有一条"）——
    /// 且 `before → after` 必须是**真的两个时刻**：先整体回写（`full_rewrite`），
    /// 再保留式编辑（`text_preserve`）⇒ 第二条入口的 `before` 必须是 `full_rewrite`。
    ///
    /// **改什么会让本条变红**：把 `prev_mode` 改成与 `mode` 同值（即在更新 `last_write_mode`
    /// **之后**再取 prev）⇒ 第 3 条断言红（`before` 会变成 `text_preserve`，审计页显示假前后值）。
    #[tokio::test]
    async fn write_mode_audit_entry_records_the_real_before_and_after() {
        let sink = Arc::new(ProbeSink::default());
        let h = harness_with(sink.clone(), YAML_NO_GATEWAY);
        // 第一次：`gateway:` 段缺失 ⇒ 整体回写
        let r1 = h
            .svc
            .apply(&ok_request("rid-wm-1", &[("gateway.listen_port", serde_json::json!(2405))]))
            .await;
        assert_eq!(r1.applied.unwrap().write_mode, WriteMode::FullRewrite);
        // 第二次：整体回写后的文件已含 `gateway:` ⇒ 回到保留式编辑
        let r2 = h
            .svc
            .apply(&ok_request("rid-wm-2", &[("intercore.port", serde_json::json!(2406))]))
            .await;
        assert_eq!(r2.applied.unwrap().write_mode, WriteMode::TextPreserve);

        let wm: Vec<ConsoleAuditEntry> = sink
            .entries()
            .into_iter()
            .filter(|e| e.target == "config.write_mode")
            .collect();
        assert_eq!(wm.len(), 2, "两次写入各一条 write_mode 条目");
        // 第一条：本进程的初值 `text_preserve` → `full_rewrite`
        assert_eq!(wm[0].before, Some(serde_json::json!("text_preserve")));
        assert_eq!(wm[0].after, Some(serde_json::json!("full_rewrite")));
        // 第二条：**真**前值 = 上一次落盘的模式（不是同值的假前后）
        assert_eq!(
            wm[1].before,
            Some(serde_json::json!("full_rewrite")),
            "before 必须取**本次更新之前**的写模式（审计页的前后值要能自证降级发生过）"
        );
        assert_eq!(wm[1].after, Some(serde_json::json!("text_preserve")));
        // 全部条目的 result 都是 Ok（成功路径）
        assert!(wm.iter().all(|e| e.result == AuditResult::Ok));
    }

    /// **建议 6.2**：`ResetDefault`（渲染端「恢复默认值」发 `from=reset_default`）在 core-bin 内
    /// 原先**零用例**。本用例端到端覆盖两条：
    /// ① 成功路径：**7 个可写键全部**落盘 + 审计 `op` 是 `ConfigResetDefault`（逐一断言，不是数个数）；
    /// ② 审计不可用 ⇒ **一个字节都不改**（EDGE-10 不得部分应用），回 `AuditUnavailable`。
    ///
    /// **改什么会让本条变红**：把 `outcome_entries`/`record_intent` 的 op 判据写成常量
    /// `ConfigApply` ⇒ ①的第 2 条红；把"审计失败即终止"改成"继续执行" ⇒ ②全红。
    #[tokio::test]
    async fn reset_default_applies_every_default_and_is_audited_as_reset_default() {
        let sink = Arc::new(ProbeSink::default());
        let h = harness_with(sink.clone(), YAML_ALL_DIFFERENT);
        let before_text = disk(&h);
        // 「恢复默认值」= 把 7 个可写键的**默认值**一次性下发（与现状全不同 ⇒ 全部应落盘）
        let resp = h
            .svc
            .apply(&request(
                "rid-reset",
                &[
                    ("system.log_level", serde_json::json!("info")),
                    ("intercore.host", serde_json::json!("127.0.0.1")),
                    ("intercore.port", serde_json::json!(9100)),
                    ("intercore.heartbeat_interval_sec", serde_json::json!(5)),
                    ("intercore.reconnect_interval_sec", serde_json::json!(3)),
                    ("gateway.listen_addr", serde_json::json!("0.0.0.0")),
                    ("gateway.listen_port", serde_json::json!(2404)),
                ],
                PatchSource::ResetDefault,
            ))
            .await;
        assert!(resp.ok, "{resp:?}");
        assert_eq!(resp.applied.as_ref().unwrap().revision, 1);

        // ① 内存副本 + 落盘 + 审计逐字段
        {
            let g = h.core.read().await;
            assert_eq!(g.system.log_level, "info");
            assert_eq!(g.intercore.host, "127.0.0.1");
            assert_eq!(g.intercore.port, 9100);
            assert_eq!(g.intercore.heartbeat_interval_sec, 5);
            assert_eq!(g.intercore.reconnect_interval_sec, 3);
            assert_eq!(g.gateway.listen_addr, "0.0.0.0");
            assert_eq!(g.gateway.listen_port, 2404);
        }
        let after_text = disk(&h);
        assert_ne!(after_text, before_text, "7 个键都变了 ⇒ 文件必被写");
        for want in [
            "log_level: info",
            "host: 127.0.0.1",
            "port: 9100",
            "heartbeat_interval_sec: 5",
            "reconnect_interval_sec: 3",
            "listen_addr: 0.0.0.0",
            "listen_port: 2404",
        ] {
            assert!(after_text.contains(want), "落盘缺 `{want}`:\n{after_text}");
        }

        let entries = sink.entries();
        // 7 个字段 + 1 条 write_mode
        assert_eq!(entries.len(), 8, "逐字段一条 + write_mode 一条");
        for e in &entries {
            assert_eq!(
                e.op,
                ConsoleOp::ConfigResetDefault,
                "`{}` 的审计 op 必须是 ConfigResetDefault（屏上「恢复默认值」与「保存」在审计里必须可区分）",
                e.target
            );
        }
        for key in [
            "system.log_level",
            "intercore.host",
            "intercore.port",
            "intercore.heartbeat_interval_sec",
            "intercore.reconnect_interval_sec",
            "gateway.listen_addr",
            "gateway.listen_port",
        ] {
            let e = entries
                .iter()
                .find(|e| e.target == key)
                .unwrap_or_else(|| panic!("审计缺 `{key}` 条目"));
            assert!(e.before.is_some() && e.after.is_some(), "`{key}` 须有前后值");
            assert_ne!(e.before, e.after, "`{key}` 的前后值必须不同");
        }

        // ② 审计不可用 ⇒ 不得部分应用（EDGE-10）
        let h2 = harness_with(Arc::new(ProbeSink::failing()), YAML_ALL_DIFFERENT);
        let before2 = disk(&h2);
        let r2 = h2
            .svc
            .apply(&request(
                "rid-reset-fail",
                &[
                    ("system.log_level", serde_json::json!("info")),
                    ("gateway.listen_port", serde_json::json!(2404)),
                ],
                PatchSource::ResetDefault,
            ))
            .await;
        assert!(!r2.ok && r2.code == ControlCode::AuditUnavailable);
        assert!(r2.applied.is_none(), "未执行 ⇒ 不得带生效值");
        assert_eq!(disk(&h2), before2, "文件一个字节都不改（不得部分应用）");
        assert_eq!(h2.core.read().await.gateway.listen_port, 2405, "内存副本不得改");
        assert_eq!(h2.svc.revision(), 0);
    }

    /// **阻塞 3 回归**：以**真实部署配置**（`deploy/config/` 两份真文件，`include_str!` ⇒ 与
    /// 现场逐字节同源）为输入的保留式编辑往返。
    ///
    /// 对**每一个** `editable=true` 的键各做一次编辑，断言：
    /// ① 编辑**可定位**（不可定位 ⇒ 上层整体回写 ⇒ **注释全丢**，正是评审实证的现场场景）；
    /// ② **除目标那一行外逐字节不变**（按 `split_inclusive('\n')` 逐行比对 ⇒ 含行尾风格）；
    /// ③ 注释行数守恒；④ 编辑结果仍可解析且**只有该键**变化。
    ///
    /// **改什么会让本条变红**：把已补的 `gateway:` 段从任一 deploy yaml 删掉 ⇒
    /// `gateway.listen_addr` / `gateway.listen_port` 落进 `fallback` ⇒ 第 1 条断言红
    /// （= 评审阻塞 3 的实证："首次设 IEC-104 监听地址就会抹掉 49 行注释"）。
    #[test]
    fn deploy_configs_support_preserve_edit_for_every_editable_field() {
        for (name, text) in [
            (
                "mupc_core_config.yaml",
                include_str!("../../../deploy/config/mupc_core_config.yaml"),
            ),
            (
                "mupc_core_config.production.yaml",
                include_str!("../../../deploy/config/mupc_core_config.production.yaml"),
            ),
        ] {
            // 真实现场输入的前提：必须能解析并通过校验（否则 mupcd 启动期就会失败）
            let cfg: CoreConfig = serde_yaml::from_str(text)
                .unwrap_or_else(|e| panic!("`{name}` 必须能解析为 CoreConfig: {e}"));
            assert!(cfg.validate().is_ok(), "`{name}` 必须过 validate(): {:?}", cfg.validate());
            let comments = |s: &str| {
                s.lines().filter(|l| l.trim_start().starts_with('#')).count()
            };
            let src_lines: Vec<&str> = text.split_inclusive('\n').collect();

            let mut fallback: Vec<&str> = Vec::new();
            for m in FIELDS.iter().filter(|m| m.editable) {
                let cur = (m.current)(&cfg);
                let new = probe_value_for(m.key);
                assert_ne!(new, cur, "`{name}`/`{}`：探针值必须与原值不同", m.key);
                let edit = ScalarEdit::from_key(m.yaml_path, new.clone())
                    .expect("yaml_path 与 key 同形（两段）");
                let out = match yaml_edit::apply_edits(text, &[edit]) {
                    Ok(out) => out,
                    Err(e) => {
                        // 如实记录：这些键**当前**会落到整体回写（WriteMode=FullRewrite）
                        eprintln!("`{name}`/`{}` 不可定位（{e}）⇒ 会整体回写、丢注释", m.key);
                        fallback.push(m.key);
                        continue;
                    }
                };
                // ② 逐行（含行尾）比对：恰有 1 行不同，其余**逐字节**不变
                let out_lines: Vec<&str> = out.split_inclusive('\n').collect();
                assert_eq!(
                    src_lines.len(),
                    out_lines.len(),
                    "`{name}`/`{}`：行数不得变化（保留式编辑的硬要求）",
                    m.key
                );
                let diff: Vec<usize> = (0..src_lines.len())
                    .filter(|&i| src_lines[i] != out_lines[i])
                    .collect();
                assert_eq!(
                    diff.len(),
                    1,
                    "`{name}`/`{}`：恰有 1 行被替换，实得 {diff:?}",
                    m.key
                );
                assert_eq!(comments(text), comments(&out), "`{name}`/`{}`：注释行数必须守恒", m.key);
                // ③ 编辑结果仍可解析，且**只有该键**变化（防"改一个值顺带改坏别人"）
                let back: CoreConfig = serde_yaml::from_str(&out).unwrap_or_else(|e| {
                    panic!("`{name}`/`{}`：编辑后不可解析: {e}", m.key)
                });
                for mm in FIELDS {
                    let got = (mm.current)(&back);
                    if mm.key == m.key {
                        assert_eq!(got, new, "`{name}`/`{}`：目标键未被改成新值", m.key);
                    } else {
                        assert_eq!(
                            got,
                            (mm.current)(&cfg),
                            "`{name}`/`{}`：顺带改了 `{}`",
                            m.key,
                            mm.key
                        );
                    }
                }
            }
            assert!(
                fallback.is_empty(),
                "`{name}`：以下键**当前**会落到整体回写（WriteMode=FullRewrite ⇒ 现场全部注释丢失）: {fallback:?}\
                 —— 补 `gateway:` 段后这些键都应在保留式路径上"
            );
        }
    }

    /// 可写键的**探针值**（与 deploy yaml 现状值必不相同，且各自过自身 kind 校验）。
    ///
    /// 表驱动而非随机：随机值无法断言"预期的具体字节"，也难在红时定位。新增可写字段必须
    /// **同时**在此登记（否则 `panic!` ⇒ 强制登记，不留"新字段没被往返覆盖"的缝）。
    fn probe_value_for(key: &str) -> serde_json::Value {
        match key {
            "system.log_level" => serde_json::json!("debug"),
            "intercore.host" | "gateway.listen_addr" => serde_json::json!("192.168.3.21"),
            "intercore.port" => serde_json::json!(9101),
            "gateway.listen_port" => serde_json::json!(2405),
            "intercore.heartbeat_interval_sec" => serde_json::json!(9),
            "intercore.reconnect_interval_sec" => serde_json::json!(8),
            other => panic!("新增可写字段 `{other}` 未在此登记探针值"),
        }
    }

    /// **建议 6.4 回归**：`atomic_write` 崩溃窗口留下的磁盘形态（**真源缺失、`.bak` 在**）
    /// ⇒ 启动期加载必须用 `.bak` 恢复，而不是"无配置可读 ⇒ 进程起不来"。
    ///
    /// **改什么会让本条变红**：把 `main.rs` 的加载改回 `CoreConfig::load`（= 修复前的行为）
    /// ⇒ 第 1 条断言红（真源缺失时只会 `Err`）。
    #[test]
    fn startup_load_recovers_the_source_from_backup_after_a_crash_window() {
        let t = TempDir::new("cfg-recover");
        let path = t.join("mupc_core_config.yaml");
        // 崩溃窗口后的磁盘形态：真源不在（已改名成 .bak），`.tmp` 还没顶上
        std::fs::write(sibling(&path, ".bak"), YAML).unwrap();
        let bak = sibling(&path, ".bak");
        assert!(!path.exists(), "本用例前提：真源缺失");

        let (cfg, recovered) =
            load_config_with_backup_recovery(&path).expect("真源缺失而 .bak 在 ⇒ 必须能恢复");
        assert_eq!(cfg.intercore.port, 9100);
        assert_eq!(recovered.as_deref(), Some(bak.as_path()), "须报告恢复来源（调用方要响亮记录）");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), YAML, "真源须被恢复成 .bak 的内容");
        assert!(!sibling(&path, ".tmp").exists(), "不得残留 .tmp");

        // 正常路径：真源在 ⇒ 用真源、**不碰** .bak（现场手工改回的配置不得被悄悄回退）
        let (cfg2, rec2) = load_config_with_backup_recovery(&path).expect("真源在 ⇒ 正常加载");
        assert_eq!(cfg2.intercore.port, 9100);
        assert!(rec2.is_none(), "真源可读 ⇒ 不得报告恢复");

        // 两者都没有 ⇒ 必须 Err（不得静默用内嵌默认值，让装置跑在与现场无关的配置上）
        let t2 = TempDir::new("cfg-norecover");
        assert!(load_config_with_backup_recovery(&t2.join("nope.yaml")).is_err());
    }
}
