//! `AlertFeed`——告警即时投递环（12-本地显示终端 设计 **§4.7**，开发单元 **K**）。
//!
//! # 处置口径（**逐字对齐设计 §4.7，不得自行加码**）
//!
//! 本结构是 `mupc_web_api::SsePushService` 的**替代物**：后者唯一的现有用途是「`SouthSink`
//! 在写 `SystemEvent` 之后推一条 SSE 事件给 Web 客户端」——`web-api` crate 整体删除后，这条
//! 推送落到本结构上（另两个既有推送点：联锁 major 事件、策略下发事件，同 crate 内一并迁移）。
//!
//! **本期只做最小形态**：一个 `tokio::sync::broadcast`，容量 [`ALERT_FEED_CAPACITY`]（= 64）。
//!
//! ⚠️ **它不是 F7 的真源，也不得被提升为真源**：
//! - F7（告警上屏）的真源是 `storage.events`（设计 §4.1 #3 / D11），本结构**不参与**组帧、
//!   **不被** `display_host::DisplayDataProvider` 读取；
//! - 本结构只作「**未落库也能上屏**」的**可选增强**留给后续——是否把它与 `storage.events`
//!   合并成 F7 的一路源（`available = 任一可用`），是**待 PM/评审裁决**的范围点
//!   （设计 §14 **R-07**），**本轮不裁**、本单元**不实现**。
//!
//! # 有界性与「无人订阅」口径
//!
//! `broadcast` 是**有界环**：订阅者落后超过容量即被告知 `Lagged`，生产者永不阻塞、不无界增长
//! （这是选它而非 `Vec` 的直接原因）。**无订阅者时投递返回 0 且被调用方忽略**——那是正常态
//! （本期生产过程确实无人订阅），**不是**错误、不得 panic、不得降级为 `unwrap()`。

use tokio::sync::broadcast;

/// 最小形态的环容量（设计 §4.7：「`tokio::sync::broadcast`（容量 64）」）。
pub const ALERT_FEED_CAPACITY: usize = 64;

/// 告警来源分类（决定「未落库也能上屏」时 UI 的落位；契约侧无对应类型 ⇒ 本层自定义）。
///
/// ⚠️ **命名订正（S-1）**：本类型原名 `AlertSource`，与**同 crate** `display_host::AlarmSource`
/// （F7 告警源的 **trait**，`read_alarms() -> Vec<display_proto::AlarmItem>`）**只差一个字母**
/// 而语义完全无关，且契约类型 `display_proto::AlarmItem` 与旧名 `AlertItem` 同形近音。
/// 设计 §4.7 的字面稿用的正是 `AlarmItem` ⇒ 若将来 R-07 裁"提升为一路源"，两者极易被混用。
/// 故整对改名为 `FeedOrigin` / [`FeedItem`]：**本模块的条目不是 F7 的告警条目**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedOrigin {
    /// 南向站状态事件（`SouthSink::on_station_data`，与 `storage.events` 落库同源同刻）。
    System,
    /// 安全联锁 major 事件（`InterlockController` 触发 / 清除 / 停机未确认 / M1 授权）。
    Interlock,
    /// 策略下发（AI 决策循环每拍；AI 引擎停用后为本地台区储能治理下发，沿用同一环）。
    Strategy,
}

/// 一条即时告警（**纯内存、不下发、不落库**——落库仍由 `storage.events` 负责）。
///
/// ⚠️ **不是** `display_proto::AlarmItem`（F7 告警条目，`{ts_ms, level, message}` 三字段、
/// 由 `display_host::AlarmSource` 产出）；本类型是 [`AlertFeed`] 环内的投递条目，
/// **不参与组帧**。命名订正理由见 [`FeedOrigin`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedItem {
    /// UTC 毫秒时间戳（与既有 `SystemEvent` / 审计条目同口径）。
    pub ts_ms: u64,
    /// 来源分类。
    pub source: FeedOrigin,
    /// 来源内的子类型：`System` 取级别（`info` / `warning` / **`major`** —— `major` 由
    /// 03 设计 §9.3 缺口 1 的健康巡检（`storage_health::DROP_ALERT_LEVEL`）投递）；
    /// `Interlock` 取事件名（`triggered` / `cleared` / `stop_failed` / `ack_m1`）；
    /// `Strategy` 为空串。
    ///
    /// **为什么保留它**：迁出前的 `SsePushService::push_interlock(event, msg)` 把事件名放在
    /// payload 里；本结构不消费它，但**不静默丢字段**——将来接 F7 增强时才不必回头改生产侧。
    pub subtype: String,
    /// 人可读单行文案（与迁出前的 SSE `message` **逐字一致**，不再改写）。
    pub message: String,
}

/// 告警即时投递环（有界 `broadcast`；见模块头「处置口径」）。
pub struct AlertFeed {
    tx: broadcast::Sender<FeedItem>,
}

impl Default for AlertFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl AlertFeed {
    /// 生产构造：容量 = [`ALERT_FEED_CAPACITY`]。
    pub fn new() -> Self {
        Self::with_capacity(ALERT_FEED_CAPACITY)
    }

    /// 指定容量构造（**仅供测试**构造小环以复现「落后即 `Lagged`」；生产走 [`Self::new`]）。
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    /// 订阅投递流（本期生产侧**无**订阅者；仅供测试与将来的最小屏推送增强）。
    ///
    /// `#[allow(dead_code)]` 是**如实登记**而非掩盖：本 crate 是二进制，生产构建里从 `main`
    /// 出发**没有**调用点（本期确实无人订阅——这正是 §4.7「可选增强、非 F7 真源」的含义）。
    /// **不得**为了让本告警消失而删掉它（那会断掉 R-07 若裁"提升为一路源"时的入口），也**不得**
    /// 为了让本告警消失而在生产侧随便找个地方调一下（那是为编译而造的假订阅）。
    #[allow(dead_code)]
    pub fn subscribe(&self) -> broadcast::Receiver<FeedItem> {
        self.tx.subscribe()
    }

    /// 投递一条告警，返回**当时**的订阅者数。
    ///
    /// 无订阅者 ⇒ `Err(SendError)` ⇒ 返回 **0**。这是**正常态**（本期确实无人订阅），
    /// 故**不**返回 `Result`、**不**记 warn、**不** panic——把它做成错误会让生产每拍刷一条
    /// 无意义的告警日志。
    pub fn push(&self, item: FeedItem) -> usize {
        self.tx.send(item).unwrap_or(0)
    }

    /// 系统告警（南向站 offline/online 等）——迁出前 `SsePushService::push_system_alert` 的落点。
    pub fn push_system_alert(&self, level: &str, message: &str) -> usize {
        self.push(FeedItem {
            ts_ms: crate::config_service::now_ms(),
            source: FeedOrigin::System,
            subtype: level.to_string(),
            message: message.to_string(),
        })
    }

    /// 联锁事件——迁出前 `SsePushService::push_interlock` 的落点（**语义保持**：`event` 原样带走）。
    pub fn push_interlock(&self, event: &str, message: &str) -> usize {
        self.push(FeedItem {
            ts_ms: crate::config_service::now_ms(),
            source: FeedOrigin::Interlock,
            subtype: event.to_string(),
            message: message.to_string(),
        })
    }

    /// 策略下发——迁出前 `SsePushService::push_ai_decision` 的落点（文案不变）。
    pub fn push_strategy_dispatch(&self, summary: &str) -> usize {
        self.push(FeedItem {
            ts_ms: crate::config_service::now_ms(),
            source: FeedOrigin::Strategy,
            subtype: String::new(),
            message: summary.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小形态的**硬形状**：容量就是 64。
    ///
    /// ⚠️ **订正 S-2（如实标注本断言的强度）**：`assert_eq!(ALERT_FEED_CAPACITY, 64)` 是
    /// **定义式断言**——常量在 `:27` 就写成 `= 64`，本断言只能守住"没人把常量改掉"，**证不了
    /// 运行期行为**（后者由 `feed_is_bounded_and_lagging_subscriber_is_told_so` 用 `Lagged`
    /// 覆盖）。它**真正守的是契约侧的常量**：设计 §4.7 原文「`tokio::sync::broadcast`（容量 64）」。
    /// ⇒ **改这里的 64 必须同改设计 §4.7**，否则两者不一致。
    #[test]
    fn minimal_form_capacity_is_64() {
        assert_eq!(ALERT_FEED_CAPACITY, 64);
    }

    /// **投递链路（任务书要求 ③）**：`SouthSink` 写系统事件 ⇒ 同刻投递 ⇒ 订阅者收到，
    /// 且来源/子类型/文案逐字段如实（不吞字段、不改写文案）。
    #[tokio::test]
    async fn south_sink_style_system_event_is_delivered_to_subscriber() {
        let feed = AlertFeed::new();
        let mut rx = feed.subscribe();
        let n = feed.push_system_alert("warning", "站 st-1 role=Grid 离线（采集失败）");
        assert_eq!(n, 1, "有一个订阅者 ⇒ 投递到 1 个");

        let got = rx.recv().await.expect("订阅者必须收到");
        assert_eq!(got.source, FeedOrigin::System);
        assert_eq!(got.subtype, "warning", "level 如实带走");
        assert_eq!(
            got.message, "站 st-1 role=Grid 离线（采集失败）",
            "文案逐字不改写"
        );
        assert!(got.ts_ms > 0, "时间戳为投递时刻");
    }

    /// 联锁 / 策略两条既有推送点的**语义未被简化**（`event` / 文案都带走）。
    #[tokio::test]
    async fn interlock_and_strategy_sources_keep_their_subtype_and_text() {
        let feed = AlertFeed::new();
        let mut rx = feed.subscribe();
        feed.push_interlock("stop_failed", "PCS 停机未确认");
        feed.push_strategy_dispatch("策略下发完成");

        let a = rx.recv().await.unwrap();
        assert_eq!(a.source, FeedOrigin::Interlock);
        assert_eq!(a.subtype, "stop_failed");
        assert_eq!(a.message, "PCS 停机未确认");

        let b = rx.recv().await.unwrap();
        assert_eq!(b.source, FeedOrigin::Strategy);
        // S-9 补：`FeedItem.subtype` 的文档承诺「`Strategy` 为空串」（`alert_feed.rs` 字段注释
        // / 设计 §4.7 的字段语义）——原先本条**只断言了 message**，承诺无人守。
        // **改什么会让这条断言红**：把 `push_strategy_dispatch` 的 `subtype` 从 `String::new()`
        // 改成任何非空值（如塞进 summary 或 source 名）。
        assert_eq!(b.subtype, "", "Strategy 子类型按契约恒为空串");
        assert_eq!(b.message, "策略下发完成");
    }

    /// **无订阅者是正常态**（本期生产真实形态）：投递返回 0，不 panic、不阻塞。
    #[test]
    fn delivery_without_subscriber_returns_zero_and_does_not_panic() {
        let feed = AlertFeed::new();
        assert_eq!(feed.push_system_alert("warning", "无人订阅"), 0);
    }

    /// **有界**：环容量满后，落后订阅者收 `Lagged` 而**生产者照常投递**（不阻塞、不无界增长）。
    ///
    /// **改什么会让本条变红**：把 `broadcast` 换成无界 `Vec`/`mpsc::unbounded` ⇒ `recv()`
    /// 不再产生 `Lagged`，第 1 条断言红。
    #[tokio::test]
    async fn feed_is_bounded_and_lagging_subscriber_is_told_so() {
        let feed = AlertFeed::with_capacity(4);
        let mut rx = feed.subscribe();
        for i in 0..10 {
            assert_eq!(feed.push_system_alert("warning", &format!("事件 {i}")), 1);
        }
        match rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                assert!(skipped > 0, "落后必须被告知，实 skipped={skipped}");
            }
            other => panic!("有界环落后应回 Lagged，实得 {other:?}"),
        }
        // 生产者未被阻塞：继续投递仍返回当前订阅者数
        assert_eq!(feed.push_system_alert("info", "环满后仍可投递"), 1);
    }
}
