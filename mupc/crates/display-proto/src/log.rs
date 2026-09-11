//! 日志（F10）契约 DTO——设计 §3.4 端点 `/v1/console/logs`、`/v1/console/logs/targets` + §4.4。
//!
//! 设计口径（T-1 裁定）：
//! - 检索**仅选项式**（级别多选 + 模块多选 + 预设时间区间），**无关键字**、**无导出**——
//!   故本模块**不存在**任何自由文本查询字段（零键盘的类型层体现）。
//! - 增量拉取：`cursor` = 单调 `seq`；HMI 每 500 ms 拉一次 `seq > cursor` 的条目。
//! - 超限（EDGE-15）：单次请求扫描文件数 / 总行数超限即 **拒绝并置 `range_too_large=true`**，
//!   **不执行全库检索**。

use serde::{Deserialize, Serialize};

/// 单次请求返回条数上限（设计 §3.4：`limit ≤ 200`）。
pub const LOG_PAGE_LIMIT_MAX: usize = 200;

/// 模块选项数上限（设计 §3.4：`/logs/targets` ≤50）。
pub const LOG_TARGETS_MAX: usize = 50;

/// 日志 ring 容量（设计 §4.4：有界 `VecDeque`，内存上界固定 → 满足「内存不得单调增长」）。
pub const LOG_RING_CAPACITY: usize = 2000;

/// 历史扫描：单次请求最多文件数（设计 §4.4 限额）。
pub const LOG_SCAN_MAX_FILES: usize = 5;

/// 历史扫描：单次请求总行数上限（设计 §4.4 限额；超限 → `range_too_large`）。
pub const LOG_SCAN_MAX_LINES: usize = 50_000;

/// 日志级别（选项式多选维度；JSON 小写，与 `tracing` 及既有日志行口径一致）。
///
/// `Trace` 一并保留：日志 ring 含「当前级别可见」的条目，若 `log_level=trace`，
/// 缺该变体会导致整页反序列化失败——保留它比过滤掉它更安全（**不得静默坏值**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// 错误。
    Error,
    /// 警告。
    Warn,
    /// 提示。
    Info,
    /// 调试。
    Debug,
    /// 追踪。
    Trace,
}

impl LogLevel {
    /// UI 筛选 chip 文案（PRD §3.3 F10：ERROR / WARN / INFO / DEBUG）。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        }
    }
}

/// 时间范围筛选（预设区间选项；设计 §3.4 `range=1h|24h|custom`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogRange {
    /// 最近 1 小时。
    #[serde(rename = "1h")]
    H1,
    /// 最近 24 小时。
    #[serde(rename = "24h")]
    H24,
    /// 自定义起止（`from` / `to` 由 UI 选项式步进给出）。
    #[serde(rename = "custom")]
    Custom,
}

/// 单条日志（设计 §4.4：已格式化条目字段）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    /// 单调序号（`cursor` 增量拉取的依据）。
    pub seq: u64,
    /// 条目时刻（Unix 毫秒）。
    pub ts_ms: u64,
    /// 级别。
    pub level: LogLevel,
    /// 模块（`tracing` target）。
    pub target: String,
    /// 消息文本。
    pub message: String,
}

/// 日志页（`GET /v1/console/logs` 返回；设计 §3.4）。
///
/// **无 `#[serde(default)]`**（Critical 2 修复）：`entries` / `has_more` / `range_too_large`
/// 是**必需**字段，缺失即整帧反序列化 `Err`（HMI 显示旧帧 / 错误态），
/// **不得**被静默解读为「无日志」。否则服务端漏发 `range_too_large` 时，HMI 会显示
/// 「无日志」而不是 EDGE-15 要求的「检索范围超限」——正是本模块要杜绝的「缺失伪装成有效」。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LogPage {
    /// 本页条目（时间倒序，窗口化列表消费）。**必需**：缺失 ≠「无日志」。
    pub entries: Vec<LogEntry>,
    /// 下一页游标；`None` = 无更多（`Option` 缺省语义即 `None`，与 `has_more` 互为佐证）。
    pub next_cursor: Option<u64>,
    /// 是否还有更多。**必需**：缺失即 `Err`。
    pub has_more: bool,
    /// 检索范围超限（EDGE-15）：`true` → UI 提示「检索范围超限，请缩小时间范围」，
    /// 且**本次未执行全库检索**（`entries` 不代表完整结果）。**必需**：缺失即 `Err`，
    /// 绝不默认 `false`（那会把「超限」伪装成「正常结果」）。
    pub range_too_large: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_level_json_lowercase_and_chips() {
        for (lvl, literal, chip) in [
            (LogLevel::Error, "\"error\"", "ERROR"),
            (LogLevel::Warn, "\"warn\"", "WARN"),
            (LogLevel::Info, "\"info\"", "INFO"),
            (LogLevel::Debug, "\"debug\"", "DEBUG"),
            (LogLevel::Trace, "\"trace\"", "TRACE"),
        ] {
            assert_eq!(serde_json::to_string(&lvl).unwrap(), literal);
            assert_eq!(lvl.display_name(), chip);
        }
        // 日志行（tracing JSON）里的小写级别可直接落本枚举
        assert_eq!(
            serde_json::from_str::<LogLevel>("\"warn\"").unwrap(),
            LogLevel::Warn
        );
    }

    #[test]
    fn log_range_json_presets() {
        assert_eq!(serde_json::to_string(&LogRange::H1).unwrap(), "\"1h\"");
        assert_eq!(serde_json::to_string(&LogRange::H24).unwrap(), "\"24h\"");
        assert_eq!(serde_json::to_string(&LogRange::Custom).unwrap(), "\"custom\"");
        assert_eq!(serde_json::from_str::<LogRange>("\"24h\"").unwrap(), LogRange::H24);
    }

    #[test]
    fn log_page_literal_json_roundtrip() {
        let json = r#"{
            "entries": [
                {"seq":101,"ts_ms":1757412000000,"level":"error","target":"mupc_intercore",
                 "message":"核间心跳超时"},
                {"seq":100,"ts_ms":1757411999000,"level":"info","target":"mupc_gateway",
                 "message":"IEC104 连接建立"}
            ],
            "next_cursor": 101,
            "has_more": true,
            "range_too_large": false
        }"#;
        let page: LogPage = serde_json::from_str(json).unwrap();
        assert_eq!(page.entries.len(), 2);
        assert_eq!(page.entries[0].seq, 101);
        assert_eq!(page.entries[0].level, LogLevel::Error);
        assert_eq!(page.entries[0].target, "mupc_intercore");
        assert_eq!(page.entries[0].message, "核间心跳超时");
        assert_eq!(page.next_cursor, Some(101));
        assert!(page.has_more && !page.range_too_large);
        let back: LogPage = serde_json::from_str(&serde_json::to_string(&page).unwrap()).unwrap();
        assert_eq!(back, page);
    }

    /// Critical 2：日志页关键字段**缺失必须失败得响亮**（字面量 JSON 反例）。
    ///
    /// 探针实测（修复前）：`{}` → `Ok(entries:[], has_more:false, range_too_large:false)`，
    /// 服务端漏发 `range_too_large` 时 HMI 显「无日志」而非「检索范围超限」（EDGE-15）。
    #[test]
    fn log_page_missing_required_fields_fail_loudly() {
        // 反例：缺任一必需字段 → Err（不得静默取默认）
        for json in [
            // 全缺（修复前 `{}` 会 Ok）
            r#"{}"#,
            // 缺 entries（修复前 → 空列表 =「无日志」）
            r#"{"has_more":false,"range_too_large":false}"#,
            // 缺 has_more（修复前 → false）
            r#"{"entries":[],"range_too_large":false}"#,
            // 缺 range_too_large（修复前 → false，即把「超限」伪装成「正常」）
            r#"{"entries":[],"has_more":false}"#,
        ] {
            let res = serde_json::from_str::<LogPage>(json);
            assert!(
                res.is_err(),
                "{json} 缺必需字段必须 Err（不得把缺失伪装成有效数据），实际: {res:?}"
            );
        }
        // 正例：必需字段齐备 → Ok，且超限态可被读出（≠ 空态）
        let ok: LogPage =
            serde_json::from_str(r#"{"entries":[],"has_more":false,"range_too_large":true}"#)
                .unwrap();
        assert!(ok.range_too_large && !ok.has_more && ok.entries.is_empty());
    }

    /// EDGE-08 / EDGE-15：空态与超限态是**两个不同**的显式信号，不得混淆。
    #[test]
    fn empty_page_vs_range_too_large_are_distinct() {
        let empty = LogPage::default();
        assert!(empty.entries.is_empty() && !empty.has_more && !empty.range_too_large);
        let too_large = LogPage {
            range_too_large: true,
            ..Default::default()
        };
        assert!(too_large.range_too_large && too_large.entries.is_empty());
        // 超限时不得被当作「无结果」空态：二者由不同标志区分
        assert_ne!(empty, too_large);
        // 限额常量与设计一致
        assert_eq!(LOG_PAGE_LIMIT_MAX, 200);
        assert_eq!(LOG_TARGETS_MAX, 50);
        assert_eq!(LOG_RING_CAPACITY, 2000);
        assert_eq!(LOG_SCAN_MAX_FILES, 5);
        assert_eq!(LOG_SCAN_MAX_LINES, 50_000);
    }
}
