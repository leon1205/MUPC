//! 联锁**写路径**（`InterlockApi` 的两个写方法 + 设计 §3.3 通用管线）——开发单元 **J**。
//!
//! 对应设计：§3.3 控制通道通用信封与管线（8 步）、§3.4 端点清单的两条 POST
//! （`/v1/console/interlock/release` 与 `/v1/console/interlock/ack_m1`）、§4.6 联锁控制接口
//! （错误结构化）、§4.5 / PL-1 审计、§9 EDGE-12 / EDGE-18 / EDGE-19。
//!
//! # 管线（**与 G-2 配置写路径同一套机制**，逐条点名）
//!
//! | 步 | 做什么 | 复用的既有件 |
//! |----|--------|--------------|
//! | 1 | 路由 / 方法 | axum 路由表（`console_host::router`，结构性保证；方法不符 405、未登记 404） |
//! | 2 | 信封校验（`request_id` 非空 / `op` 与端点一致 / ±30 s 防重放） | 契约 `ControlRequest::validate_for`（**同一份判据**） |
//! | 3 | 幂等（`(op, request_id)`：占位中 ⇒ `Busy`；已完成 ⇒ 首次回执 + `duplicate`） | [`crate::idempotency::IdempotencyTable`]（**同款有界 + TTL 表**，F17.5 / F18.5 / EDGE-14） |
//! | 4 | **EDGE-19 乐观并发**：`observed_*` 与服务端当前态比对 | 契约 `InterlockOpPayload`（设计 §3.4 补注） |
//! | 5 | **审计 intent 前置写入**（写不成 ⇒ `AuditUnavailable` 且**不执行**） | [`crate::console_audit::ConsoleAuditSink::record_intent`]（**fail-closed 同款**，EDGE-18 / PL-1） |
//! | 6 | 执行（`request_release` / `ack_m1`） | 契约 `InterlockApi`（真实现 = `interlock::InterlockController`） |
//! | 7 | 结果审计（成功与失败**都**留痕） | [`crate::console_audit::ConsoleAuditSink::record_outcome`] |
//! | 8 | 回执（`ControlResponse<InterlockOpAck>`） | 契约信封（**POST 一律 200 + 信封**，见 `console_host` 模块头） |
//!
//! # 拒绝通道（**为什么不是错误码而是 `message`**）
//!
//! 契约 `ControlResponse<InterlockOpAck>` **没有** `reject: Option<InterlockReject>` 字段
//! （`display-proto` 冻结，本单元禁改）⇒ 结构化拒绝**只能**经 `message` 上屏，这正是
//! 渲染端 `p4_interlock.rs` **IL12** 已登记的口径（「具体原因由服务端 `message` 承担」，
//! 客户端**不按 `code` 猜语义**——猜错即谎报原因）。故本模块：
//! - 一切业务拒绝 ⇒ `code = RejectedPrecondition` + `message = InterlockReject::user_message()`
//!   （**逐字**，契约文案即上屏文案）；
//! - EDGE-19 冲突 ⇒ 同上，`message` 取设计 §3.4 补注 / TD:601 的**原文**
//!   「联锁状态已变化，请刷新后重试」（**不由客户端猜**，见 `control_route.rs` 的沿革）。
//!
//! # 审计条目口径（PL-1：时间戳 / 操作者 / 操作类型 / 操作前后值 / 结果 / 失败原因）
//!
//! - `op` = [`ConsoleOp::InterlockRelease`] / [`ConsoleOp::InterlockAckM1`]；
//! - `target` = `"interlock.release"` / `"interlock.ack_m1"`（契约 `ConsoleAuditEntry::target`
//!   文档**点名**的两个值；渲染端 `p5_audit.rs` 的 `INTERLOCK_TARGETS` 按它们转出中文标签）；
//! - `before` / `after` = **操作前 / 后的 latch 态**（bool ⇒ 审计页显「开 / 关」，`p5_audit` AU12）。
//!   ⚠️ **取值口径（如实）**：`release` 的 `before→after` 就是本操作的**效果**（开→关）；
//!   `ack_m1` 的 latch 前后都是「关」（那正是它的**前置**）——它的真实效果（一次性放行
//!   `RUN_STATE=0` 下的重启）落在 **PCS 侧**，本进程的快照观测不到 ⇒ **不臆造**一个
//!   "变了"的前后值。UI 不消费该字段（`InterlockOpAck` 才是 UI 的立即刷新源），故无上屏后果；
//! - 失败 ⇒ `after = None`（操作**未生效**，状态未变；契约：`None` = 不适用 / 未知）、
//!   `reason = Some(具体原因)`（拒绝文案逐字；审计页经 `free_text_safe` 折叠后上屏）。
//! - `operator` 恒为契约常量 [`CONSOLE_OPERATOR`]（T-3 无登录）。
//!
//! # ⚠️ 已登记的硬门禁：字体码表 + 文案统一收口批（P4 真机验收前必须完成）
//!
//! 本模块上屏的 `message` 有**四个来源**（用字口径见 `console_host::receipt` 模块头）：
//! 1. **自拼的固定文案**（[`receipt::INTERLOCK_NOT_ENABLED`] / `BAD_ENVELOPE` / `BUSY` /
//!    `AUDIT_UNAVAILABLE` 等）—— 硬约束：**逐字 ⊆ cmap**；
//! 2. **契约 [`InterlockReject::user_message()`] 的逐字原文**（单元 J 禁改：改写契约原文即改语义）；
//! 3. **设计原文**的 EDGE-19 文案 [`receipt::INTERLOCK_CONFLICT`]——**自拼的固定文案，但 ∉ cmap**，
//!    是来源 1 那份"⊆ cmap"清单里的**唯一例外**（故上面第 1 条的清单里没有它）；
//! 4. **契约 `ControlResponse::ok()` 的缺省成功文案**「操作成功」（`display-proto/src/control.rs`，
//!    单元 J 不改写它）——字形在 cmap 内，**不出豆腐块**，故不计入下面的缺字并集。
//!
//! 来源 2、3 含生成字体 cmap 外的字 ⇒ **真机上会出豆腐块**。缺字集合由
//! `console_host::tests::interlock_receipt_messages_use_only_font_cmap_glyphs` 的 `PINNED_MISSING`
//! **逐字钉死**——**该表是唯一清单，收口范围以它的并集为准**，当前并集 **23 个码位**：
//! - **11 个 ASCII**：`,` `a` `c` `d` `e` `i` `l` `o` `p` `r` `t`
//! - **4 个全角**：`，` `（` `）` `：`
//! - **8 个 CJK**：`候` `理` `稍` `丢` `句` `柄` `误` `错`
//!
//! ⚠️ **上面这三个计数是手抄的**，与 `PINNED_MISSING` **无机械约束**，表一变则本处可能**静默过期**；
//! **以表为准**（改表时须同步本处）。
//!
//! ⚠️ **ASCII 那一档不是样本产物**：`latch` 的 `l`/`c` 与 `io` 的 `i`/`o` 出自**固定文案**
//! （`处于 latch 态` / `内部错误：io 句柄丢失`）⇒ **必然缺、必然出豆腐块**。故字库批若按
//! "扩 N 个 CJK / 全角字形"做，这两串**仍会留豆腐块**，该批作为 P4 硬门禁会**验收不通过**。
//!
//! 口径：该批是 **P4 真机验收的硬门禁**——并集全部补齐（10 档合计约 +5–15 KB）+ 文案统一收口，
//! 未完成前 P4 不得通过真机验收。详见 `console_host::receipt` 模块头的同款登记。

use std::sync::Arc;

use mupc_display_proto::{
    AuditResult, ConsoleAuditEntry, ConsoleEndpoint, ConsoleOp, ControlCode, ControlRequest,
    ControlResponse, FieldError, IdempotencyKey, InterlockApi, InterlockOpAck, InterlockOpPayload,
    InterlockReject, CONSOLE_OPERATOR,
};
use serde_json::Value;
use uuid::Uuid;

use crate::config_service::now_ms;
use crate::console_audit::{AuditIntent, ConsoleAuditSink};
use crate::console_host::receipt;
use crate::idempotency::{IdempotencyTable, Reserve};

/// 联锁写服务（**进程内唯一实例**；持后端、审计 sink、幂等表）。
///
/// `backend = None` 表示**联锁功能未启用**（`io.enabled=false` ⇒ 装配层不构造控制器）：
/// 此时两条写端点回 [`ControlCode::Unavailable`] + 固定文案「联锁功能未启用」——
/// **不是**「联锁状态不可用」（那是读路径的 `available=false`），也**不是**静默成功。
pub struct InterlockService {
    backend: Option<Arc<dyn InterlockApi>>,
    audit: Arc<dyn ConsoleAuditSink>,
    table: IdempotencyTable<ControlResponse<InterlockOpAck>>,
}

impl InterlockService {
    /// 构造（`audit` 由装配点注入；**审计不可用时应让写路径整体不可用**，见
    /// `console_host::InterlockOpsSource`）。
    pub fn new(backend: Option<Arc<dyn InterlockApi>>, audit: Arc<dyn ConsoleAuditSink>) -> Self {
        Self {
            backend,
            audit,
            table: IdempotencyTable::with_contract_bounds(),
        }
    }

    /// 两条写端点的**完整管线**（设计 §3.3 的 2–8 步；第 1 步"路由与方法"由 axum 路由表保证）。
    ///
    /// `ep` 必须是 `InterlockRelease` 或 `InterlockAckM1`（调用方 `console_host` 按路径给；
    /// 其它端点在 [`Self::op_of`] 里 `unreachable!()` —— 那条路径**不可达**，因为 handler
    /// 是逐路径注册的）。
    pub async fn handle(
        &self,
        ep: ConsoleEndpoint,
        req: &ControlRequest<InterlockOpPayload>,
    ) -> ControlResponse<InterlockOpAck> {
        let now = now_ms();
        let op = Self::op_of(ep);

        // ── 步骤 2：信封校验（request_id 非空 / op 与端点一致 / ±30 s 重放窗口）──────────
        //
        // **先于一切**（含"未启用"判定与幂等表）：失败时连"这是哪一次请求"都不可信 ⇒
        // 不记幂等表、不写审计。原因串（契约 `ControlEnvelopeError` 是**全小写英文**）**不进**
        // `message`（cmap 外 ⇒ 豆腐块）⇒ 走 `field_errors[0].reason`（结构化，与配置写同渠道）
        // + 一条 `warn`。
        if let Err(e) = req.validate_for(ep, now) {
            tracing::warn!(request_id = %req.request_id, error = %e,
                "联锁写请求信封非法（op 误路由 / request_id 为空 / 超出重放窗）⇒ 拒绝");
            return ControlResponse::rejected(
                req.request_id.clone(),
                ControlCode::RejectedValidation,
                receipt::BAD_ENVELOPE,
                vec![FieldError {
                    field: "request".to_string(),
                    reason: e.to_string(),
                }],
                None,
                now,
            );
        }

        // ── 未启用（io.enabled=false）：结构性事实，先于幂等与执行 ─────────────────────
        // 放在信封校验**之后**：审计条目要带一个可信的 `request_id`（否则未启用部署上会攒下
        // 一堆 `request_id=""` 的失败痕），且"报文都不合法"比"功能没开"更该先说。
        //
        // ⚠️ **已登记（单元 J 第二轮整改建议 2）：本判定先于 `table.reserve`（下方步骤 3）⇒
        // "未启用"部署不参与幂等表。** 后果（如实）：同一 `request_id` 重试时
        // **各留一条审计痕**、回执 `duplicate=false`——与 G-2 / 本管线"幂等先于一切执行"的
        // 口径**不同**。为什么接受：该态下**没有任何副作用**（不执行、不改状态），重复的只是
        // 一条"功能未启用"的失败痕，且它**带正确的 `request_id`**（可溯源）。
        // 备选修法（把 `reserve` 提到本判定之前）**不取**：那会让未启用部署上的重试回 `Busy`
        // ——用"上一操作正在处理中"盖掉"功能没开"这一更具体、更该先说的结构性事实。
        let Some(backend) = self.backend.as_ref() else {
            tracing::warn!(path = ep.path(), "联锁功能未启用，写请求被拒（Unavailable）");
            let audit_id = self.write_outcome(
                Ep::of(ep),
                None,
                None,
                AuditResult::Failed,
                Some(receipt::INTERLOCK_NOT_ENABLED.to_string()),
                req.request_id.clone(),
                now,
            );
            return ControlResponse::rejected(
                req.request_id.clone(),
                ControlCode::Unavailable,
                receipt::INTERLOCK_NOT_ENABLED,
                Vec::new(),
                audit_id,
                now,
            );
        };

        // ── 步骤 3：幂等查表（键 = `(op, request_id)`）──────────────────────────────────
        match self.table.reserve(IdempotencyKey::new(op, req.request_id.clone()), now) {
            // 命中且已完成 ⇒ **首次的原始回执** + `duplicate=true`（`ok` / `code` 不变）
            Reserve::Done(mut first) => {
                first.mark_duplicate();
                return first;
            }
            // 命中且处理中 ⇒ Busy（**不**排队、**不**重复执行）
            //
            // ⚠️ **已登记（单元 J 第二轮整改建议 3）：本臂直接返回，`audit_id = None`，
            // 即"管线级 Busy"不写审计** —— 与 PL-1 字面「成功与失败均留痕」有一处**字面缺口**。
            // 口径依据：这一臂**根本没进入执行**（连 intent 都没写），它拦下的是"同一次请求的
            // 重发"，**首次那一次**的 intent + outcome 已经留痕（真正的操作凭据在那边）；
            // G-2 配置写同款属既有口径。联锁是安全路径 ⇒ 此处**如实登记**而非扩面。
            // 与 [`receipt::BUSY`] 的登记**同处成对**：那条登记的是"两处 Busy 文案在屏上不可
            // 区分"，本条登记的是"管线级 Busy 无审计痕"——两处互相点名（`receipt::BUSY` 的
            // 文档里也指向本臂）。
            Reserve::InFlight => {
                return ControlResponse::rejected(
                    req.request_id.clone(),
                    ControlCode::Busy,
                    receipt::BUSY,
                    Vec::new(),
                    None,
                    now,
                )
            }
            Reserve::Fresh => {}
        }

        let resp = self.execute(ep, req, now, backend).await;
        // 终态入表（**含失败态**：契约要求"首次失败的重放同样保持失败"）。
        if !self.table.complete(&IdempotencyKey::new(op, req.request_id.clone()), resp.clone()) {
            tracing::warn!(
                request_id = %req.request_id,
                "迟到 complete：本次请求耗时超过幂等 TTL，占位已过期 ⇒ 回执不入表"
            );
        }
        resp
    }

    /// 步骤 4–8 的实体（`backend` 已在调用点确认为 `Some`）。
    async fn execute(
        &self,
        ep: ConsoleEndpoint,
        req: &ControlRequest<InterlockOpPayload>,
        now: u64,
        backend: &Arc<dyn InterlockApi>,
    ) -> ControlResponse<InterlockOpAck> {
        let e = Ep::of(ep);

        // ── 步骤 4：EDGE-19 乐观并发（`observed_*` vs 服务端当前态）──────────────────────
        //
        // 画面是**慢拍**（0.5 s 采集 + 1 Hz 组帧）⇒ 提交时可能已过期。比对口径：
        // ① `observed_latched` vs 当前 `latched`；
        // ② `observed_sources`（**全量名列表**，渲染端 IL16）vs 当前源名集合（**按集合比**，
        //    顺序是装配顺序的产物、不是语义 ⇒ 逐位比对会造出假冲突）。
        // 不符 ⇒ `RejectedPrecondition` + 设计原文文案，且**一个字节都不执行**。
        let st = backend.status().await;
        let mut observed = req.payload.observed_sources.clone();
        let mut current: Vec<String> = st.sources.iter().map(|s| s.name.clone()).collect();
        observed.sort_unstable();
        observed.dedup();
        current.sort_unstable();
        current.dedup();
        if req.payload.observed_latched != st.latched || observed != current {
            let which = if req.payload.observed_latched != st.latched {
                "latch"
            } else {
                "sources"
            };
            let reason = format!(
                "联锁状态已变化（{} 与装置当前态不符）：画面观测 latched={} sources={:?} / 当前 latched={} sources={:?}",
                which, req.payload.observed_latched, req.payload.observed_sources, st.latched, current
            );
            tracing::warn!(request_id = %req.request_id, %reason, "EDGE-19 乐观并发检查不符 ⇒ 拒绝执行");
            let audit_id = self.write_outcome(
                e,
                Some(Value::Bool(st.latched)),
                None,
                AuditResult::Failed,
                Some(reason),
                req.request_id.clone(),
                now,
            );
            return ControlResponse::rejected(
                req.request_id.clone(),
                ControlCode::RejectedPrecondition,
                receipt::INTERLOCK_CONFLICT,
                Vec::new(),
                audit_id,
                now,
            );
        }

        // ── 步骤 5：审计 intent **前置**写入（失败 ⇒ AuditUnavailable 并终止，不执行）──────
        let intent = AuditIntent {
            op: e.op,
            target: e.target.to_string(),
            request_id: req.request_id.clone(),
            summary: format!(
                "observed_latched={} observed_sources={:?} current_latched={}",
                req.payload.observed_latched, req.payload.observed_sources, st.latched
            ),
        };
        if let Err(err) = self.audit.record_intent(&intent) {
            tracing::error!(request_id = %req.request_id, error = %err,
                "审计 intent 写入失败 ⇒ 拒绝执行联锁写操作（fail-closed）");
            let mut r = ControlResponse::audit_unavailable(req.request_id.clone(), now);
            r.message = receipt::AUDIT_UNAVAILABLE.to_string();
            return r;
        }

        // ── 步骤 6：执行（真实现 = `InterlockController`；每一条拒绝都是结构化变体）──────
        let outcome = match e.kind {
            OpKind::Release => backend.request_release().await,
            OpKind::AckM1 => backend.ack_m1().await,
        };

        // ── 步骤 7–8：结果审计 + 回执 ──────────────────────────────────────────────────
        match outcome {
            Ok(()) => {
                // `applied` 取**操作后**的真实状态（不是"照抄请求"）：UI 据此立即刷屏
                // （F17.6 / IL-13：`stop_failed := !ack.stopped`），不等下一帧。
                let after = backend.status().await;
                let ack = InterlockOpAck {
                    latched: after.latched,
                    // ⚠️ **已登记（单元 J 第一轮整改 I2，J 不改）**：`stopped := !after.stop_failed`
                    // 与后端 `do_request_release` 的**无条件清 `stop_failed`**（含 override 放行
                    // 场景，见 `interlock.rs` 的 `had_stop_failed` 分支）叠加 ⇒ **一次 override
                    // 释放之后屏上不再显「停机失败」**，F16.3 在该场景失效。
                    // 这是**状态机既有语义**（非单元 J 引入）⇒ 登记给后续单元 / PM 裁决，
                    // 本单元**不改**（改它要动状态机的"人工放行即视为停机已确认"这条取舍）。
                    stopped: !after.stop_failed,
                };
                // 成功条目：`before`/`after` 必须来自**两个时刻**（操作前快照 / 操作后快照）
                let audit_id = self.write_outcome(
                    e,
                    Some(Value::Bool(st.latched)),
                    Some(Value::Bool(after.latched)),
                    AuditResult::Ok,
                    None,
                    req.request_id.clone(),
                    now,
                );
                ControlResponse::ok(req.request_id.clone(), Some(ack), audit_id, now)
            }
            Err(reject) => {
                let msg = reject.user_message();
                tracing::warn!(request_id = %req.request_id, op = ?e.kind, reject = ?reject,
                    "联锁写操作被拒（装置维持原状）: {msg}");
                let audit_id = self.write_outcome(
                    e,
                    Some(Value::Bool(st.latched)),
                    None,
                    AuditResult::Failed,
                    Some(msg.clone()),
                    req.request_id.clone(),
                    now,
                );
                ControlResponse::rejected(
                    req.request_id.clone(),
                    ControlCode::RejectedPrecondition,
                    msg,
                    Vec::new(),
                    audit_id,
                    now,
                )
            }
        }
    }

    /// 结果审计（**成功与失败都写**；PL-1）。写不成**不回滚**已生效的操作（与 G-2 硬口径 3
    /// 同款）：`tracing::error!` 响亮记录 + 回执 `audit_id=None`（**不编造**一个号）。
    #[allow(clippy::too_many_arguments)]
    fn write_outcome(
        &self,
        e: Ep,
        before: Option<Value>,
        after: Option<Value>,
        result: AuditResult,
        reason: Option<String>,
        request_id: String,
        now: u64,
    ) -> Option<String> {
        let entry = ConsoleAuditEntry {
            id: Uuid::new_v4().to_string(),
            ts_ms: now,
            operator: CONSOLE_OPERATOR.to_string(),
            op: e.op,
            target: e.target.to_string(),
            before,
            // 失败 ⇒ 传 `None`（操作未生效，状态未变；契约：`None` = 不适用 / 未知）
            after,
            result,
            reason,
            request_id: request_id.clone(),
        };
        match self.audit.record_outcome(&entry) {
            Ok(()) => Some(entry.id),
            Err(err) => {
                tracing::error!(request_id = %request_id, error = %err,
                    "联锁结果审计写入失败：操作结果**已按事实处置**（不回滚、不谎报），但本次无审计条目");
                None
            }
        }
    }

    /// 端点 ⇒ 信封 `op` 名（= 路径末段，契约唯一合法取值）。
    fn op_of(ep: ConsoleEndpoint) -> &'static str {
        match ep {
            ConsoleEndpoint::InterlockRelease => "release",
            ConsoleEndpoint::InterlockAckM1 => "ack_m1",
            other => unreachable!("InterlockService 只服务两条联锁写端点，实得 {other:?}"),
        }
    }

    /// 端点 ⇒ 审计 `target`（契约 `ConsoleAuditEntry::target` 点名的两个值）。
    // 由用例断言映射（渲染端 `p5_audit::INTERLOCK_TARGETS` 按这两个值转标签）。
    // 单元 J 第一轮整改（N2）：已集中登记（见 `console_host::receipt` 模块头
    // 「登记：测试专用尺子」），现状可接受、不扩大改动面。
    #[allow(dead_code)]
    pub(crate) fn target_of(ep: ConsoleEndpoint) -> &'static str {
        Ep::of(ep).target
    }
}

/// 操作种类（回执 / 审计 / 分派三者共用一份映射，避免"三处各写一遍"漂移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpKind {
    /// 人工释放联锁。
    Release,
    /// M1 授权重启。
    AckM1,
}

/// 端点 → （`ConsoleOp` / `target` / 分派臂）三元组。
#[derive(Debug, Clone, Copy)]
struct Ep {
    kind: OpKind,
    op: ConsoleOp,
    target: &'static str,
}

impl Ep {
    fn of(ep: ConsoleEndpoint) -> Self {
        match ep {
            ConsoleEndpoint::InterlockRelease => Self {
                kind: OpKind::Release,
                op: ConsoleOp::InterlockRelease,
                target: "interlock.release",
            },
            ConsoleEndpoint::InterlockAckM1 => Self {
                kind: OpKind::AckM1,
                op: ConsoleOp::InterlockAckM1,
                target: "interlock.ack_m1",
            },
            other => unreachable!("非联锁写端点：{other:?}"),
        }
    }
}

/// 契约 `InterlockReject` **全部 7 个变体**的 `user_message()`（设计 §11.3：一个都不能少）。
///
/// 典型值取**真机可能的实参**（`remaining` 取源 token —— 渲染端 IL16 明确 `observed_sources`
/// 用的是机器名，故这里的取值形态与线上一致）。
// 只被用字网消费（同上）。单元 J 第一轮整改（N2）：已集中登记（见 `console_host::receipt`
// 模块头「登记：测试专用尺子」），现状可接受、不扩大改动面。
#[allow(dead_code)]
pub(crate) fn reject_messages() -> Vec<String> {
    [
        InterlockReject::SourcesNotReset {
            remaining: vec!["estop".to_string(), "door".to_string()],
        },
        InterlockReject::HoldNotElapsed {
            need_secs: 30,
            remaining_secs: 12,
        },
        InterlockReject::Latched,
        InterlockReject::StopPending,
        InterlockReject::NotEnabled,
        InterlockReject::Busy,
        InterlockReject::Internal("io 句柄丢失".to_string()),
    ]
    .iter()
    .map(InterlockReject::user_message)
    .collect()
}

// ═══════════════════════════════════════════════════════════════
// 测试替身（**跨模块共用**：`console_host` 的端点用例也从这里取）
// ═══════════════════════════════════════════════════════════════

/// 用例共用的测试替身与装配小工具。
///
/// 放在这里而不是各测试模块各写一份：联锁后端的桩**只有一份**（[`testkit::FakeBackend`]），
/// 端点用例（`console_host`）与管线用例（本模块）用的是同一个桩 —— 两边对"契约长什么样"的
/// 假设不可能漂移。
#[cfg(test)]
pub(crate) mod testkit {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    use mupc_display_proto::{InterlockSourceStatus, InterlockView};

    /// 可编程的联锁后端桩：状态可改、拒绝可注入、调用次数可查。
    #[derive(Default)]
    pub(crate) struct FakeBackend {
        view: Mutex<InterlockView>,
        /// 写方法的返回值（`None` = `Ok(())`）。
        reject: Mutex<Option<InterlockReject>>,
        release_calls: AtomicU32,
        ack_calls: AtomicU32,
        /// **进入**写方法的次数（含被拒绝的）——"一个动作都没发"类断言的判据。
        entered: AtomicU32,
        /// 置 1 ⇒ 写方法成功后把 `latched` 置 false（模拟真控制器"操作后状态"）。
        flip_on_write: AtomicU32,
        /// 装上后，写方法会**先等一次 `Notify`** 再返回 —— 供并发用例把"某次写还在途"
        /// 这一瞬间确定性地造出来（不靠 sleep 猜时序）。
        gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
        /// **进入**写方法时的唤醒点（与 `entered` 计数器同步发信号）——供"等在途"的用例
        /// **确定性**等待，替代 `sleep(2ms)` 轮询 500 次（单元 J 第二轮整改建议 8）。
        entered_notify: tokio::sync::Notify,
    }

    impl FakeBackend {
        pub(crate) fn new(view: InterlockView) -> Self {
            Self {
                view: Mutex::new(view),
                ..Default::default()
            }
        }

        /// 注入写方法的拒绝原因。
        pub(crate) fn reject_with(&self, r: InterlockReject) {
            *self.reject.lock().unwrap() = Some(r);
        }

        /// 改当前状态视图（EDGE-19 用例据此让"服务端当前态"与画面观测不符）。
        pub(crate) fn set_view(&self, v: InterlockView) {
            *self.view.lock().unwrap() = v;
        }

        /// 写方法成功后把 `latched` 置 false（造出"操作后状态与操作前不同"）。
        pub(crate) fn flip_latched_on_write(&self) {
            self.flip_on_write.store(1, Ordering::SeqCst);
        }

        /// 装闸：之后的写方法会停在 await 点上（并发 / 在途用例）。
        pub(crate) fn set_gate(&self, n: Arc<tokio::sync::Notify>) {
            *self.gate.lock().unwrap() = Some(n);
        }

        pub(crate) fn release_calls(&self) -> u32 {
            self.release_calls.load(Ordering::SeqCst)
        }
        pub(crate) fn ack_calls(&self) -> u32 {
            self.ack_calls.load(Ordering::SeqCst)
        }
        /// 进入写方法的次数（**含被拒的**）。
        pub(crate) fn entered(&self) -> u32 {
            self.entered.load(Ordering::SeqCst)
        }

        /// 等"已有写方法**进入**后端"的**确定性**唤醒点——不靠 `sleep` 猜时序。
        ///
        /// 用 `notify_one`（不是 `notify_waiters`）：前者在"尚无等待者"时**存一个许可** ⇒
        /// 只要已经（或正在）进入过一次，`notified()` 立刻返回，**不存在"睡过窗口"的竞态**。
        /// 范式与 `interlock.rs` 的并发用例（`Notify` + 可观测的 `op_in_flight`）同款。
        pub(crate) async fn wait_entered(&self) {
            self.entered_notify.notified().await;
        }
    }

    /// 写方法的公共收尾：先看注入的拒绝，再按需改状态。
    fn finish(b: &FakeBackend) -> Result<(), InterlockReject> {
        if let Some(r) = b.reject.lock().unwrap().clone() {
            return Err(r);
        }
        if b.flip_on_write.load(Ordering::SeqCst) == 1 {
            b.view.lock().unwrap().latched = false;
        }
        Ok(())
    }

    #[async_trait::async_trait]
    impl InterlockApi for FakeBackend {
        async fn status(&self) -> InterlockView {
            self.view.lock().unwrap().clone()
        }
        async fn request_release(&self) -> Result<(), InterlockReject> {
            self.entered.fetch_add(1, Ordering::SeqCst);
            self.entered_notify.notify_one(); // 唤醒 wait_entered()（**先于** wait_gate ⇒ 唤醒点 = "已进入"）
            self.release_calls.fetch_add(1, Ordering::SeqCst);
            self.wait_gate().await;
            finish(self)
        }
        async fn ack_m1(&self) -> Result<(), InterlockReject> {
            self.entered.fetch_add(1, Ordering::SeqCst);
            self.entered_notify.notify_one();
            self.ack_calls.fetch_add(1, Ordering::SeqCst);
            self.wait_gate().await;
            finish(self)
        }
    }

    impl FakeBackend {
        /// 闸（若装了）——`notify_one` 语义：放闸方先发信号也不会丢。
        async fn wait_gate(&self) {
            let g = self.gate.lock().unwrap().clone();
            if let Some(n) = g {
                n.notified().await;
            }
        }
    }

    /// 恒失败的审计（**步骤 5** fail-closed 的注入点：intent 写不进去 ⇒ 不执行）。
    pub(crate) struct BrokenSink;
    impl ConsoleAuditSink for BrokenSink {
        fn record_intent(&self, _i: &AuditIntent) -> Result<(), String> {
            Err("注入失败：审计目录不可写".to_string())
        }
        fn record_outcome(&self, _e: &ConsoleAuditEntry) -> Result<(), String> {
            Err("注入失败：审计文件不可写".to_string())
        }
    }

    /// **只**失败 `record_outcome` 的审计（`record_intent` 正常返回）——**步骤 7** 的注入点。
    ///
    /// # 为什么必须单有一个（单元 J 第二轮整改 I-1 的覆盖缺口）
    ///
    /// [`BrokenSink`] 两个方法都失败 ⇒ 它在 `handle` 里**永远停在步骤 5**（intent 失败 ⇒
    /// `AuditUnavailable` + 不执行）⇒ **步骤 7 的结果审计站点根本走不到**。于是"结果审计写不成
    /// ⇒ 操作**已按事实处置**（不回滚）、回执 `audit_id=None`（**不编造**编号）"这条安全相关
    /// 保证此前**没有任何用例执行到**——评审实证：把该分支改成谎报一个审计号 ⇒ 290 条全绿。
    /// 本 sink 把那条分支变成**可达**，见
    /// `outcome_audit_failure_does_not_roll_back_and_invents_no_audit_id`。
    pub(crate) struct OutcomeBrokenSink;
    impl ConsoleAuditSink for OutcomeBrokenSink {
        fn record_intent(&self, _i: &AuditIntent) -> Result<(), String> {
            Ok(())
        }
        fn record_outcome(&self, _e: &ConsoleAuditEntry) -> Result<(), String> {
            Err("注入失败：审计文件不可写（intent 已成功）".to_string())
        }
    }

    /// `FileAuditSink` 落盘的全部条目（按文件 / 行序）。
    pub(crate) fn read_entries(dir: &std::path::Path) -> Vec<ConsoleAuditEntry> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(dir) else {
            return out;
        };
        let mut files: Vec<std::path::PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("console-audit-") && n.ends_with(".jsonl"))
            })
            .collect();
        files.sort();
        for f in files {
            let Ok(text) = std::fs::read_to_string(&f) else {
                continue;
            };
            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                out.push(serde_json::from_str(line).expect("审计行必须是合法 JSONL"));
            }
        }
        out
    }

    /// 一个"看着像真机"的联锁视图（`estop` 已触发 + latch）。
    pub(crate) fn view_latched() -> InterlockView {
        InterlockView {
            ts_ms: 1_700_000_000_000,
            available: true,
            enabled: true,
            latched: true,
            stop_failed: false,
            sources: vec![InterlockSourceStatus {
                name: "estop".to_string(),
                tripped: true,
            }],
            fault_lamp: Some(true),
            run_lamp: Some(false),
            release_hold_secs: 0,
        }
    }

    /// 同上但未 latch（`ack_m1` 的可授权态）。
    pub(crate) fn view_unlatched() -> InterlockView {
        let mut v = view_latched();
        v.latched = false;
        v.sources[0].tripped = false;
        v.fault_lamp = Some(false);
        v.run_lamp = Some(true);
        v
    }

    /// 渲染端 `op_payload` 的等价物（UI 观测到的 latch + **全量**源名列表，IL16）。
    pub(crate) fn payload_of(v: &InterlockView) -> InterlockOpPayload {
        InterlockOpPayload {
            observed_latched: v.latched,
            observed_sources: v.sources.iter().map(|s| s.name.clone()).collect(),
        }
    }

    /// 构造一个在重放窗内的合法信封。
    pub(crate) fn request(
        ep: ConsoleEndpoint,
        request_id: &str,
        payload: InterlockOpPayload,
    ) -> ControlRequest<InterlockOpPayload> {
        ControlRequest::new(
            request_id,
            super::now_ms(),
            ep.op_name().unwrap_or("release"),
            payload,
        )
    }

    /// 每用例独立的审计落点（真实 `FileAuditSink`，不 mock）。
    pub(crate) struct Fixture {
        dir: crate::testutil::TempDir,
    }

    impl Fixture {
        pub(crate) fn new(tag: &str) -> Self {
            Self {
                dir: crate::testutil::TempDir::new(tag),
            }
        }
        pub(crate) fn sink(&self) -> Arc<dyn ConsoleAuditSink> {
            Arc::new(crate::console_audit::FileAuditSink::open(self.dir.path()).unwrap())
        }
        pub(crate) fn entries(&self) -> Vec<ConsoleAuditEntry> {
            read_entries(self.dir.path())
        }
        /// "后端就绪 + 真实审计"的服务。
        pub(crate) fn service(&self, backend: &Arc<FakeBackend>) -> InterlockService {
            let b: Arc<dyn InterlockApi> = backend.clone();
            InterlockService::new(Some(b), self.sink())
        }
        /// 未启用（`io.enabled=false`）的服务。
        pub(crate) fn service_disabled(&self) -> InterlockService {
            InterlockService::new(None, self.sink())
        }
        /// 审计不可用的服务（fail-closed 注入）。
        pub(crate) fn service_broken_audit(&self, backend: &Arc<FakeBackend>) -> InterlockService {
            let b: Arc<dyn InterlockApi> = backend.clone();
            InterlockService::new(Some(b), Arc::new(BrokenSink))
        }
        /// **只**结果审计写不进去的服务（步骤 7 的口径专用，见 [`OutcomeBrokenSink`]）。
        pub(crate) fn service_outcome_broken_audit(
            &self,
            backend: &Arc<FakeBackend>,
        ) -> InterlockService {
            let b: Arc<dyn InterlockApi> = backend.clone();
            InterlockService::new(Some(b), Arc::new(OutcomeBrokenSink))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;
    use std::sync::Arc;

    const REL: ConsoleEndpoint = ConsoleEndpoint::InterlockRelease;
    const ACK: ConsoleEndpoint = ConsoleEndpoint::InterlockAckM1;

    /// ① 成功路径（释放）：`applied` 取**操作后**状态、审计 intent + outcome 都落
    /// （before/after / result / target / request_id 逐字段可读）。
    #[tokio::test]
    async fn release_happy_path_applies_audits_and_returns_post_state() {
        let f = Fixture::new("ilk-release-ok");
        let b = Arc::new(FakeBackend::new(view_latched()));
        b.flip_latched_on_write(); // 操作后 latch=false（模拟真控制器）
        let svc = f.service(&b);

        let resp = svc
            .handle(REL, &request(REL, "rid-1", payload_of(&view_latched())))
            .await;
        assert!(resp.ok, "{resp:?}");
        assert_eq!(resp.code, ControlCode::Ok);
        assert!(!resp.duplicate);
        let ack = resp.applied.expect("成功必须带 applied（UI 立即刷屏用）");
        assert!(!ack.latched, "applied 取**操作后**状态，不是照抄请求");
        assert!(ack.stopped, "停机已确认 = true（UI 的 stop_failed := !stopped）");
        assert!(resp.audit_id.is_some(), "成功回执带审计号（现场对拍）");
        assert_eq!(b.release_calls(), 1);

        // 审计 outcome 逐字段核对
        let es = f.entries();
        assert_eq!(es.len(), 1, "成功 ⇒ 恰一条 outcome");
        let e = &es[0];
        assert_eq!(e.op, ConsoleOp::InterlockRelease);
        assert_eq!(e.target, "interlock.release");
        assert_eq!(e.target, InterlockService::target_of(REL));
        assert_eq!(e.result, AuditResult::Ok);
        assert_eq!(e.reason, None, "成功条目无失败原因");
        assert_eq!(e.request_id, "rid-1");
        assert_eq!(e.operator, CONSOLE_OPERATOR, "T-3：无登录 ⇒ 固定标识");
        assert_eq!(e.before, Some(Value::Bool(true)), "操作前 latch=开");
        assert_eq!(e.after, Some(Value::Bool(false)), "操作后 latch=关");
        assert_eq!(e.id, resp.audit_id.clone().unwrap(), "回执带的就是这条的 id");
    }

    /// ① 成功路径（M1 授权）：目标 / 操作类型与释放**不同**（防两臂串味）。
    #[tokio::test]
    async fn ack_m1_happy_path_uses_its_own_op_and_target() {
        let f = Fixture::new("ilk-ack-ok");
        let b = Arc::new(FakeBackend::new(view_unlatched()));
        let svc = f.service(&b);

        let resp = svc
            .handle(ACK, &request(ACK, "rid-ack", payload_of(&view_unlatched())))
            .await;
        assert!(resp.ok, "{resp:?}");
        assert_eq!(b.ack_calls(), 1);
        assert_eq!(b.release_calls(), 0, "ack 臂不得碰 release");
        let es = f.entries();
        assert_eq!(es[0].op, ConsoleOp::InterlockAckM1);
        assert_eq!(es[0].target, "interlock.ack_m1");
        assert_eq!(es[0].target, InterlockService::target_of(ACK));
        assert_eq!(es[0].after, Some(Value::Bool(false)));
    }

    /// ② **七个** `InterlockReject` 变体逐一映射：`RejectedPrecondition` + **逐字**
    /// `user_message()` + `applied=None` + 一条 Failed 审计（EDGE-12 / 设计 §11.3）。
    #[tokio::test]
    async fn every_reject_variant_keeps_its_specific_message_and_leaves_a_trace() {
        let variants = [
            InterlockReject::SourcesNotReset {
                remaining: vec!["estop".to_string(), "door".to_string()],
            },
            InterlockReject::HoldNotElapsed {
                need_secs: 30,
                remaining_secs: 12,
            },
            InterlockReject::Latched,
            InterlockReject::StopPending,
            InterlockReject::NotEnabled,
            InterlockReject::Busy,
            InterlockReject::Internal("io 句柄丢失".to_string()),
        ];
        assert_eq!(variants.len(), 7, "七个变体一个都不能少");
        let f = Fixture::new("ilk-rejects");
        let b = Arc::new(FakeBackend::new(view_latched()));
        let svc = f.service(&b);

        for (i, r) in variants.iter().enumerate() {
            b.reject_with(r.clone());
            let rid = format!("rid-rej-{i}");
            let resp = svc
                .handle(REL, &request(REL, &rid, payload_of(&view_latched())))
                .await;
            assert!(!resp.ok, "{r:?} ⇒ 不得 ok=true");
            assert_eq!(
                resp.code,
                ControlCode::RejectedPrecondition,
                "{r:?} 是业务拒绝（渲染端按 `message` 展示具体原因）"
            );
            assert_eq!(
                resp.message,
                r.user_message(),
                "{r:?}：`message` 必须逐字等于契约文案（上屏文案的真源）"
            );
            assert!(resp.applied.is_none(), "{r:?}：拒绝不得带 applied");
            assert!(resp.audit_id.is_some(), "{r:?}：失败也要留痕（PL-1）");
        }
        // 七条拒绝 ⇒ 七条 Failed 审计，且 reason 与各自文案逐条对应
        let es = f.entries();
        assert_eq!(es.len(), 7, "七条拒绝 ⇒ 七条审计");
        for (e, r) in es.iter().zip(variants.iter()) {
            assert_eq!(e.result, AuditResult::Failed);
            assert_eq!(e.after, None, "拒绝 ⇒ 操作未生效，after 必须为 None");
            assert_eq!(e.reason.as_deref(), Some(r.user_message().as_str()));
        }
        // 去重计数 = 7：证明这条不是"七条塞同一句"的假覆盖（EDGE-12 要「**具体**原因」）
        let mut uniq: Vec<String> = es.iter().filter_map(|e| e.reason.clone()).collect();
        uniq.sort();
        uniq.dedup();
        assert_eq!(
            uniq.len(),
            7,
            "七个变体的文案必须两两不同（否则屏上看到的是同一句话），实得 {uniq:?}"
        );
    }

    /// ③ EDGE-19：观测态与服务端当前态不符 ⇒ 设计原文文案 + **一个动作都不发**。
    ///
    /// 两个方向都造：① latch 不符；② 源列表不符。
    #[tokio::test]
    async fn edge19_conflict_on_stale_observed_state_blocks_execution() {
        let f = Fixture::new("ilk-conflict");
        let b = Arc::new(FakeBackend::new(view_latched()));
        let svc = f.service(&b);

        // ① 画面看到"未联锁"，装置当前"已联锁"
        let mut stale = payload_of(&view_latched());
        stale.observed_latched = false;
        let resp = svc.handle(REL, &request(REL, "rid-c1", stale)).await;
        assert!(!resp.ok);
        assert_eq!(resp.code, ControlCode::RejectedPrecondition);
        assert_eq!(
            resp.message,
            receipt::INTERLOCK_CONFLICT,
            "EDGE-19 文案逐字取设计原文（服务端判定，不由客户端猜）"
        );
        assert!(resp.applied.is_none());

        // ② 源列表不符（装置侧多了一个源）
        let mut v2 = view_latched();
        v2.sources.push(mupc_display_proto::InterlockSourceStatus {
            name: "door".to_string(),
            tripped: false,
        });
        b.set_view(v2);
        let resp2 = svc
            .handle(REL, &request(REL, "rid-c2", payload_of(&view_latched())))
            .await;
        assert_eq!(resp2.code, ControlCode::RejectedPrecondition);
        assert_eq!(resp2.message, receipt::INTERLOCK_CONFLICT);

        assert_eq!(b.entered(), 0, "乐观并发不符 ⇒ 一个动作都不许发（EDGE-19 的全部意义）");
        let es = f.entries();
        assert_eq!(es.len(), 2, "两次冲突各留一条 Failed 痕");
        assert!(es.iter().all(|e| e.result == AuditResult::Failed));
        assert!(
            es.iter()
                .all(|e| e.reason.as_deref().is_some_and(|r| r.contains("已变化"))),
            "审计 reason 要点明冲突，实得 {:?}",
            es.iter().map(|e| e.reason.clone()).collect::<Vec<_>>()
        );
    }

    /// ③ 正对照：观测态确实与当前态一致 ⇒ 放行（证明上面那条不是"恒拒绝"）。
    ///
    /// 同时钉死**源列表按集合比**：顺序不同但集合相同 ⇒ 不算冲突。
    #[tokio::test]
    async fn edge19_allows_matching_observation_and_ignores_source_order() {
        let f = Fixture::new("ilk-conflict-ok");
        let mut v = view_latched();
        v.sources = vec![
            mupc_display_proto::InterlockSourceStatus {
                name: "estop".to_string(),
                tripped: true,
            },
            mupc_display_proto::InterlockSourceStatus {
                name: "door".to_string(),
                tripped: false,
            },
        ];
        let b = Arc::new(FakeBackend::new(v));
        let svc = f.service(&b);
        // 观测顺序与装置顺序相反（渲染端按帧内顺序发；顺序不是语义）
        let p = InterlockOpPayload {
            observed_latched: true,
            observed_sources: vec!["door".to_string(), "estop".to_string()],
        };
        let resp = svc.handle(REL, &request(REL, "rid-ok", p)).await;
        assert!(resp.ok, "集合相同 ⇒ 不得误判为冲突: {resp:?}");
        assert_eq!(b.entered(), 1);
    }

    /// ④ 幂等：同 `request_id` 重放（渲染端超时重试的真实形态）⇒ 首次的原始回执 +
    /// `duplicate=true`，且**不**第二次执行。
    #[tokio::test]
    async fn duplicate_request_replays_first_receipt_without_re_executing() {
        let f = Fixture::new("ilk-dup");
        let b = Arc::new(FakeBackend::new(view_latched()));
        b.flip_latched_on_write();
        let svc = f.service(&b);
        let p = payload_of(&view_latched());

        let first = svc.handle(REL, &request(REL, "rid-dup", p.clone())).await;
        assert!(first.ok && !first.duplicate);
        // ⚠️ 第一次已把 latch 清掉 ⇒ 若**重新执行**会先撞 EDGE-19；幂等命中必须在乐观并发
        // **之前**短路，否则"重试"会被误判成"冲突"（这正是两步顺序的判别力所在）。
        let second = svc.handle(REL, &request(REL, "rid-dup", p)).await;
        assert!(second.duplicate, "同 `(op, request_id)` ⇒ duplicate=true");
        assert!(second.ok, "首次成功 ⇒ 重放仍成功（不得因重放改判）");
        assert_eq!(second.audit_id, first.audit_id, "复用首次审计，不重复留痕");
        assert_eq!(b.entered(), 1, "不得第二次执行");
        assert_eq!(f.entries().len(), 1, "不得重复留痕");

        // 不同 `request_id`（渲染端每次点击新 uuid）⇒ 是新操作（幂等键是 `(op, request_id)`）
        let third = svc
            .handle(REL, &request(REL, "rid-other", payload_of(&view_unlatched())))
            .await;
        assert!(!third.duplicate);
    }

    /// ⑤ 审计 intent 写不进去 ⇒ **fail-closed**：回 `AuditUnavailable` 且一个动作都不发。
    #[tokio::test]
    async fn audit_intent_failure_is_fail_closed_and_executes_nothing() {
        let f = Fixture::new("ilk-failclosed");
        let b = Arc::new(FakeBackend::new(view_latched()));
        let svc = f.service_broken_audit(&b);

        let resp = svc
            .handle(REL, &request(REL, "rid-fc", payload_of(&view_latched())))
            .await;
        assert!(!resp.ok);
        assert_eq!(resp.code, ControlCode::AuditUnavailable, "EDGE-18");
        assert_eq!(resp.message, receipt::AUDIT_UNAVAILABLE);
        assert!(resp.applied.is_none(), "审计不可写 ⇒ 操作未执行（applied 必须空）");
        assert!(resp.audit_id.is_none(), "连审计号都没有 ⇒ 不得编一个");
        assert_eq!(b.entered(), 0, "一个动作都不许发（fail-closed 的全部意义）");
        // 排障信息不丢：原因由 `tracing::error!` 承载（P3 日志页可查）
    }

    /// ⑦ **结果审计写入失败**（步骤 7）⇒ 操作**已按事实处置**：不回滚、不谎报、`audit_id=None`。
    ///
    /// # 为什么单独立这条（单元 J 第二轮整改 I-1）
    ///
    /// 三条 fail-closed 口径里，前两条（步骤 5 intent 失败 ⇒ 不执行；步骤 2 信封非法 ⇒ 早退）
    /// 都有用例；**唯独步骤 7 的"结果写不成也不回滚"没有** —— `BrokenSink` 两个方法都失败 ⇒
    /// 管线**永远停在步骤 5**，这条分支**零覆盖**。评审实证：把 `write_outcome` 的 `Err` 分支
    /// 改成返回 `Some("REVJ4-FABRICATED-AUDIT-ID")`（**谎报审计号**）⇒ 整个 crate 290 条全绿
    /// ⇒ 这条安全相关保证当时**只由注释承担**。
    ///
    /// **改什么会让本条变红**：把 `write_outcome` 的 `Err` 分支改成 `Some(..)`（编造编号）；
    /// 或把执行结果改成"审计失败 ⇒ 回滚/改判"。
    #[tokio::test]
    async fn outcome_audit_failure_does_not_roll_back_and_invents_no_audit_id() {
        let f = Fixture::new("ilk-outcome-broken");
        let b = Arc::new(FakeBackend::new(view_latched()));
        b.flip_latched_on_write(); // 操作后 latch=false（模拟真控制器）
        let svc = f.service_outcome_broken_audit(&b);

        let resp = svc
            .handle(REL, &request(REL, "rid-ob", payload_of(&view_latched())))
            .await;
        // ① 结局**不因审计降级**：操作已生效，回执照常成功
        assert!(
            resp.ok,
            "结果审计写不进去**不改**操作结局（不回滚、不谎报）: {resp:?}"
        );
        assert_eq!(resp.code, ControlCode::Ok);
        assert!(
            resp.applied.is_some(),
            "applied 照常带回（UI 的立即刷屏不因审计降级而丢）"
        );
        // ② **不编造**审计号（写不成就是没有；编一个会让现场对拍查到一条不存在的痕）
        assert!(
            resp.audit_id.is_none(),
            "**不得**编造审计号：实得 {:?}",
            resp.audit_id
        );
        // ③ **确实执行过**（"不回滚"的可观测判据：后端被调用了一次，且状态真的变了）
        assert_eq!(b.entered(), 1, "操作必须**照常执行**（不回滚）");
        assert_eq!(b.release_calls(), 1);
        assert!(!b.status().await.latched, "操作效果真实发生（latch 已清）");
        // ④ 写不进去就是一条都没有（不得出现"半条"/占位条目）
        assert!(f.entries().is_empty(), "结果写不成 ⇒ 一条审计都不落");
    }

    /// ⑤ 未启用（后端 `None`）：`Unavailable` + 固定文案 + 仍然留痕（PL-1：失败也留痕）。
    #[tokio::test]
    async fn disabled_backend_reports_unavailable_and_still_audits() {
        let f = Fixture::new("ilk-disabled");
        let svc = f.service_disabled();
        // ① 报文不合法 ⇒ 哪怕功能没开也先说"报文无效"（信封校验**先于**未启用判定）
        let wrong = ControlRequest::new("rid-off-bad", now_ms(), "ack_m1", payload_of(&view_latched()));
        let bad = svc.handle(REL, &wrong).await;
        assert_eq!(bad.code, ControlCode::RejectedValidation);
        assert!(
            f.entries().is_empty(),
            "报文不合法 ⇒ 不写审计（`request_id` 不可信）"
        );

        // ② 合法信封 + 未启用 ⇒ `Unavailable` + 固定文案 + 留痕
        let resp = svc
            .handle(REL, &request(REL, "rid-off", payload_of(&view_latched())))
            .await;
        assert!(!resp.ok);
        assert_eq!(resp.code, ControlCode::Unavailable);
        assert_eq!(resp.message, receipt::INTERLOCK_NOT_ENABLED);
        assert!(resp.applied.is_none());
        let es = f.entries();
        assert_eq!(es.len(), 1);
        assert_eq!(es[0].request_id, "rid-off", "审计条目带**可信**的 request_id");
        assert_eq!(es[0].result, AuditResult::Failed);
        assert_eq!(es[0].reason.as_deref(), Some(receipt::INTERLOCK_NOT_ENABLED));
    }

    /// ⑥ 信封非法 ⇒ `RejectedValidation` + `BAD_ENVELOPE`，**不执行、不写审计**
    /// （此刻连"这是哪一次请求"都不可信）。
    #[tokio::test]
    async fn bad_envelope_is_rejected_before_any_side_effect() {
        let f = Fixture::new("ilk-envelope");
        let b = Arc::new(FakeBackend::new(view_latched()));
        let svc = f.service(&b);

        // ① `op` 误路由（把 `ack_m1` 的信封投给 release 端点）
        let wrong = ControlRequest::new("rid-op", now_ms(), "ack_m1", payload_of(&view_latched()));
        let r = svc.handle(REL, &wrong).await;
        assert_eq!(r.code, ControlCode::RejectedValidation);
        assert_eq!(r.message, receipt::BAD_ENVELOPE);
        assert!(!r.field_errors.is_empty(), "误路由也要点名（结构化原因，只是不上屏）");
        assert!(r.field_errors[0].reason.contains("ack_m1"));

        // ② 空 `request_id`
        let mut empty = request(REL, "rid-empty", payload_of(&view_latched()));
        empty.request_id = "   ".to_string();
        let r2 = svc.handle(REL, &empty).await;
        assert_eq!(r2.code, ControlCode::RejectedValidation);

        // ③ 超出 ±30 s 重放窗
        let mut stale = request(REL, "rid-stale", payload_of(&view_latched()));
        stale.issued_at_ms = now_ms().saturating_sub(mupc_display_proto::REPLAY_WINDOW_MS + 5_000);
        let r3 = svc.handle(REL, &stale).await;
        assert_eq!(r3.code, ControlCode::RejectedValidation);

        assert_eq!(b.entered(), 0, "信封非法 ⇒ 一个动作都不发");
        assert!(f.entries().is_empty(), "信封非法 ⇒ 不写审计（请求身份不可信）");
    }
}
