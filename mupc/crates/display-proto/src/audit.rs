//! 审计（PL-1 / PL-2 / F19）契约 DTO——设计 §3.4 端点 `/v1/console/audit`、`/v1/console/audit/ops`
//! + §4.5 存储 schema。
//!
//! 字段口径（PL-1 定稿，设计 §4.5）：原 `WebAuditEntry` 的 `user / role / ip_address /
//! user_agent` **全部去除**（无登录、无网络面 → 这些字段失去语义）；新增 `request_id`
//! 作为可追溯标识（与写请求信封对应，现场对拍用）。
//!
//! 只读铁律（PL-02）：审计**仅追加、不可删改**；本模块**不含**任何删除 / 清空 / 导出入口。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 审计分页大小（设计 §3.4：`page_size=20`；AU-01「20 条/页」）。
pub const AUDIT_PAGE_SIZE: usize = 20;

/// 审计操作类型（设计 §4.5；选项式筛选的维度，亦是 `/audit/ops` 选项来源）。
///
/// 本期须审计的写操作集合（PRD PL-1）：配置保存 / 恢复默认值 / 联锁释放 / M1 授权。
/// **模式切换为暂停项、本期无入口**，故不在本枚举内（PRD §3.8 / T-3 口径补充）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsoleOp {
    /// 配置保存（`POST /config/apply`，`from=edit`）。
    ConfigApply,
    /// 恢复默认值（`POST /config/apply`，`from=reset_default`）。
    ConfigResetDefault,
    /// 联锁人工释放（`POST /interlock/release`）。
    InterlockRelease,
    /// M1 保护跳闸 / 停机人工授权重启（`POST /interlock/ack_m1`）。
    InterlockAckM1,
}

impl ConsoleOp {
    /// 全部操作类型（`/audit/ops` 选项清单）。
    pub const ALL: [ConsoleOp; 4] = [
        Self::ConfigApply,
        Self::ConfigResetDefault,
        Self::InterlockRelease,
        Self::InterlockAckM1,
    ];

    /// 中文标签（`/audit/ops` 返回的 `label`，UI 选项文案）。
    pub fn label(self) -> &'static str {
        match self {
            Self::ConfigApply => "配置保存",
            Self::ConfigResetDefault => "恢复默认值",
            Self::InterlockRelease => "联锁释放",
            Self::InterlockAckM1 => "M1 授权",
        }
    }
}

/// 审计结果（设计 §4.5：`Ok` / `Failed`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    /// 成功。
    Ok,
    /// 失败。
    Failed,
}

/// 控制通道审计条目（`{system.log_dir}/audit/console-audit-YYYY-MM-DD.jsonl`，append-only）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsoleAuditEntry {
    /// 审计记录 ID（uuid）。
    pub id: String,
    /// 记录时刻（Unix 毫秒）。
    pub ts_ms: u64,
    /// 操作者（固定 `local-console`，见 [`crate::control::CONSOLE_OPERATOR`]；T-3 无登录）。
    pub operator: String,
    /// 操作类型（选项式筛选维度）。
    pub op: ConsoleOp,
    /// 操作目标（如 `system.log_level` / `interlock.release`）。
    pub target: String,
    /// 变更前值；`None` = 不适用 / 未知。
    pub before: Option<Value>,
    /// 变更后值；`None` = 不适用 / 未知。
    pub after: Option<Value>,
    /// 结果。
    pub result: AuditResult,
    /// 失败原因（成功为 `None`）。
    pub reason: Option<String>,
    /// 与写请求信封对应（现场对拍用）。
    pub request_id: String,
}

/// 审计页（`GET /v1/console/audit` 返回；设计 §3.4）。
///
/// `entries` / `has_more` 为**必需**字段（与 [`crate::log::LogPage`] 同口径）：缺省即 `Err`，
/// 不得把「服务端漏发」静默读成「本页无记录 / 无更多」。
/// `available` 保留 `#[serde(default)]`——其缺省 `false` 是**安全方向**（显「审计记录不可用」，
/// 而非「无审计记录」，EDGE-17），这正是允许保留默认值的判断标准。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditPage {
    /// 本页条目（时间倒序，每页 20 条）。**必需**：缺失 ≠「无审计记录」。
    pub entries: Vec<ConsoleAuditEntry>,
    /// 当前页（1-based）。
    pub page: u32,
    /// 页大小（= [`AUDIT_PAGE_SIZE`]）。
    pub page_size: u32,
    /// 是否还有更多。**必需**：缺失即 `Err`（否则分页会静默停在第 1 页）。
    pub has_more: bool,
    /// 最近一条审计时间戳（F19.8：判断审计链路是否持续写入）；`None` = 无记录。
    pub newest_ts_ms: Option<u64>,
    /// 审计存储可用性。`false` → 屏显「审计记录不可用」，**不得**显「无审计记录」
    /// （EDGE-17，二者严格区分）。
    #[serde(default)]
    pub available: bool,
}

/// 操作类型选项（`GET /v1/console/audit/ops` 返回项；设计 §3.4 `Vec<{op, label}>`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpOption {
    /// 操作类型。
    pub op: ConsoleOp,
    /// 中文标签。
    pub label: String,
}

impl From<ConsoleOp> for OpOption {
    fn from(op: ConsoleOp) -> Self {
        Self {
            op,
            label: op.label().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_op_vocabulary_and_labels() {
        for (op, literal, label) in [
            (ConsoleOp::ConfigApply, "\"config_apply\"", "配置保存"),
            (ConsoleOp::ConfigResetDefault, "\"config_reset_default\"", "恢复默认值"),
            (ConsoleOp::InterlockRelease, "\"interlock_release\"", "联锁释放"),
            (ConsoleOp::InterlockAckM1, "\"interlock_ack_m1\"", "M1 授权"),
        ] {
            assert_eq!(serde_json::to_string(&op).unwrap(), literal);
            assert_eq!(op.label(), label);
            assert_eq!(OpOption::from(op).label, label);
        }
        assert_eq!(ConsoleOp::ALL.len(), 4, "本期须审计写操作集合 = 4 类");
        // 暂停项（模式切换）不得出现在审计维度中
        assert!(!serde_json::to_string(&ConsoleOp::ALL[0])
            .unwrap()
            .contains("mode"));
    }

    #[test]
    fn audit_entry_literal_json_with_before_after() {
        let json = r#"{
            "id": "a1b2c3d4-0000-0000-0000-000000000001",
            "ts_ms": 1757412000123,
            "operator": "local-console",
            "op": "config_apply",
            "target": "system.log_level",
            "before": "info",
            "after": "debug",
            "result": "ok",
            "reason": null,
            "request_id": "0f7a3e10-1111-4222-8333-444455556666"
        }"#;
        let e: ConsoleAuditEntry = serde_json::from_str(json).unwrap();
        assert_eq!(e.operator, crate::control::CONSOLE_OPERATOR);
        assert_eq!(e.op, ConsoleOp::ConfigApply);
        assert_eq!(e.target, "system.log_level");
        assert_eq!(e.before, Some(Value::from("info")));
        assert_eq!(e.after, Some(Value::from("debug")));
        assert_eq!(e.result, AuditResult::Ok);
        assert_eq!(e.reason, None, "成功条目无失败原因");
        assert_eq!(e.request_id, "0f7a3e10-1111-4222-8333-444455556666");
        // 往返稳定
        let back: ConsoleAuditEntry =
            serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn audit_entry_failure_carries_reason() {
        let e = ConsoleAuditEntry {
            id: "id-2".into(),
            ts_ms: 2,
            operator: crate::control::CONSOLE_OPERATOR.into(),
            op: ConsoleOp::InterlockRelease,
            target: "interlock.release".into(),
            before: Some(Value::from(true)),
            after: None,
            result: AuditResult::Failed,
            reason: Some("触发源未复位：estop".into()),
            request_id: "rid-2".into(),
        };
        assert_eq!(e.result, AuditResult::Failed);
        assert!(e.reason.as_deref().unwrap().contains("estop"));
        assert_eq!(
            serde_json::to_string(&AuditResult::Failed).unwrap(),
            "\"failed\""
        );
        // before/after 的 None 序列化为 null（不臆造）
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"after\":null"));
    }

    /// EDGE-17：`available=false`（审计源不可用）与「无审计记录」是两个不同信号。
    #[test]
    fn unavailable_audit_is_not_empty_audit() {
        let unavailable = AuditPage {
            entries: vec![],
            page: 1,
            page_size: AUDIT_PAGE_SIZE as u32,
            has_more: false,
            newest_ts_ms: None,
            available: false,
        };
        let empty_ok = AuditPage {
            available: true,
            ..unavailable.clone()
        };
        assert!(!unavailable.available && unavailable.entries.is_empty());
        assert!(empty_ok.available && empty_ok.entries.is_empty());
        assert_ne!(unavailable, empty_ok, "「不可用」与「无记录」必须可区分");
        assert_eq!(AUDIT_PAGE_SIZE, 20);

        // 字面量 JSON：两个信号落在不同字段上
        let json = r#"{"entries":[],"page":1,"page_size":20,"has_more":false,
            "newest_ts_ms":null,"available":false}"#;
        let p: AuditPage = serde_json::from_str(json).unwrap();
        assert!(!p.available);
        assert_eq!(p.newest_ts_ms, None);
        assert_eq!(p.page_size, 20);
    }

    /// 与 `log::LogPage` 同口径：`entries` / `has_more` 缺失即 `Err`
    /// （不得静默读成「本页无记录」）；
    /// `available` 缺省 `false` 是**安全方向**，允许保留（显「审计不可用」而非「无记录」）。
    #[test]
    fn audit_page_missing_required_fields_fail_loudly() {
        for json in [
            // 缺 entries → 不得当作「无记录」
            r#"{"page":1,"page_size":20,"has_more":false,"available":true}"#,
            // 缺 has_more → 不得当作「无更多」
            r#"{"entries":[],"page":1,"page_size":20,"available":true}"#,
        ] {
            let res = serde_json::from_str::<AuditPage>(json);
            assert!(
                res.is_err(),
                "{json} 缺必需字段必须 Err（不得静默取默认），实际: {res:?}"
            );
        }
        // available 缺省 → false（安全方向：显「审计不可用」，绝不显「无记录」）
        let p: AuditPage =
            serde_json::from_str(r#"{"entries":[],"page":1,"page_size":20,"has_more":false}"#)
                .unwrap();
        assert!(!p.available, "缺省必须落在「不可用」而非「无记录」");
        assert_eq!(p.newest_ts_ms, None);
    }
}
