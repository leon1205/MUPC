//! **北向上送链路计数的出口**（U-74 审查 A-4 + A-5 的最小落地）。
//!
//! # 为什么需要这个模块（两个"有计数、无出口"的缺口）
//!
//! | 编号 | 设计/PRD 要求 | 改造前的事实 |
//! |------|--------------|--------------|
//! | **A-4** | 01 设计 §9.2.2 的 `iec104_dropped_total` 指标 | `Iec104Server::dropped_total()` **只有同 crate 用例在调**；生产装配零调用 ⇒ A/B 档背压丢弃只进 `tracing::warn!`，**无累计出口** |
//! | **A-5** | 01 PRD §8.6.5 **BF-6**「累计失败/丢弃计数须**可查询**」 | `MqttUplinkPublisher::stats()` 带 `#[allow(dead_code)]`，注释自认"无生产消费方" ⇒ 断线缓存淘汰/发布失败只有告警事件，**无计数面** |
//!
//! # 口径（本模块只做"读出来 + 打一行"，不新增计数、不改语义）
//!
//! - 计数**真源**仍是各自的原子量（IEC104 = `Iec104Server`；MQTT = `MqttUplinkPublisher`），
//!   本模块**不持有、不重算**任何计数（那是"两处维护同一事实"的开端）。
//! - **不可用 ≠ 0**：MQTT 未启用时 [`BothCounters::mqtt`] 为 `None`，日志行里打 `n/a`，
//!   **不得**按 0 打（0 会被读成"启用着且一切正常"，与"根本没启用"不可区分）。
//! - 出口形态 = **周期日志 + 启动首拍**（§9.3.3 留证③ 的"供后续管理面/屏显示"是后续项，
//!   本模块不臆造管理面）。日志字段名与设计/PRD **同名同拼写**，便于现场 grep 与日志采集。
//!
//! # 与既有任务的关系
//!
//! 自起一条 60 s 任务，**不入** `storage_health` / `grid_agg` 等协作名单（本任务不写数据、
//! 无在途未落盘批次 ⇒ 与网关/指标采集同范式入 **abort 名单** `guard`）。

use std::sync::Arc;
use std::time::Duration;

use mupc_gateway::iec104::server::Iec104Server;

use crate::uplink::MqttUplinkPublisher;

/// 链路计数上报周期（ms）。
///
/// 取 60 s：与步骤 12 的系统指标采集**同节拍**（排障时两类行时间轴对齐），且远高于 A/B 档
/// 周期 ⇒ 日志量可忽略（每 60 s 一行，与"逐拍打"相差 20–300×）。
pub const LINK_COUNTER_TICK_MS: u64 = 60_000;

/// 链路计数快照（**纯数据**；字段名 = 设计/PRD 里的指标名）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LinkCounters {
    /// 01 设计 §9.2.2：A/B 档背压丢弃的 **ASDU 条数**（累计）。
    pub iec104_dropped_total: u64,
    /// 01 设计 §9.2.6：无连接时的投递次数（累计）。
    pub iec104_no_subscriber_total: u64,
    /// PRD BF-6：上送**成功**发布数（消息数）。
    pub mqtt_published_total: u64,
    /// PRD BF-6：上送**失败**累计（不得静默）。
    pub mqtt_failed_total: u64,
    /// PRD BF-6：因缓存上限**淘汰**的条数（累计）。
    pub mqtt_dropped_total: u64,
    /// MQTT 侧当前**离线缓存条数**（瞬时值，非累计）。
    pub mqtt_cached_len: u64,
    /// MQTT 通道是否启用（`false` ⇒ 上表 4 个 MQTT 字段无意义，日志打 `n/a`）。
    pub mqtt_enabled: bool,
}

impl LinkCounters {
    /// 一行定长文本（周期日志的唯一格式；字段名与设计/PRD 同名）。
    ///
    /// MQTT 未启用 ⇒ 4 个 MQTT 计数打 `n/a`（**不打 0**：见模块头的「不可用 ≠ 0」）。
    pub fn log_line(&self) -> String {
        let mqtt = |v: u64| -> String {
            if self.mqtt_enabled {
                v.to_string()
            } else {
                "n/a".to_string()
            }
        };
        format!(
            "iec104_dropped_total={} iec104_no_subscriber_total={} \
             mqtt_published_total={} mqtt_failed_total={} mqtt_dropped_total={} \
             mqtt_cached_len={}",
            self.iec104_dropped_total,
            self.iec104_no_subscriber_total,
            mqtt(self.mqtt_published_total),
            mqtt(self.mqtt_failed_total),
            mqtt(self.mqtt_dropped_total),
            mqtt(self.mqtt_cached_len),
        )
    }
}

/// 计数来源（生产 = 两个既有实例；用例 = 可注入的探针，让"读到的是**当拍**值"可被钉住）。
pub trait LinkCounterSource: Send + Sync + 'static {
    /// 读一次当前计数（**只读、无锁等待**：两边都是原子量/短路锁）。
    fn snapshot(&self) -> LinkCounters;
}

/// 生产来源：IEC104 服务器（必需）+ MQTT 上送器（`None` = 未启用）。
pub struct BothCounters {
    /// IEC104 服务器实例（网关恒装配 ⇒ 非 `Option`）。
    pub iec104: Arc<Iec104Server>,
    /// MQTT 上送器（`mqtt_bridge` 未启用 / 装配失败 ⇒ `None`，不得伪造成"启用但全 0"）。
    pub mqtt: Option<Arc<MqttUplinkPublisher>>,
}

impl LinkCounterSource for BothCounters {
    fn snapshot(&self) -> LinkCounters {
        let iec104_dropped_total = self.iec104.dropped_total();
        let iec104_no_subscriber_total = self.iec104.no_subscriber_total();
        match self.mqtt.as_ref() {
            None => LinkCounters {
                iec104_dropped_total,
                iec104_no_subscriber_total,
                mqtt_enabled: false,
                ..Default::default()
            },
            Some(p) => {
                let s = p.stats();
                LinkCounters {
                    iec104_dropped_total,
                    iec104_no_subscriber_total,
                    mqtt_published_total: s.published_total,
                    mqtt_failed_total: s.failed_total,
                    mqtt_dropped_total: s.dropped_total,
                    mqtt_cached_len: s.cached_len,
                    mqtt_enabled: true,
                }
            }
        }
    }
}

/// 单次上报：读快照 → 打一行 `info` → **返回该快照**（返回值供用例/后续管理面取用，
/// 是"这次出口真的读到了什么"的可断言凭据）。
pub fn report_once(src: &dyn LinkCounterSource) -> LinkCounters {
    let c = src.snapshot();
    tracing::info!(counters = %c.log_line(), "北向上送链路计数（A-4 iec104_dropped_total / A-5 BF-6）");
    c
}

/// 起周期上报任务（**启动首拍就打**：`interval` 的第一拍立即到期 ⇒ 现场不必等 60 s）。
///
/// 句柄由调用方持有（装配层入 abort 名单 `guard`）；本任务**不写任何数据**、无停机钩子。
pub fn spawn_link_counter_reporter(
    src: Arc<dyn LinkCounterSource>,
    tick: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(tick.max(Duration::from_millis(1)));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            report_once(src.as_ref());
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 可注入探针：两个原子量在两次 `snapshot()` 之间被改动 ⇒ 证明出口**读的是当拍值**
    /// （而不是构造期抓的快照 / 常量）。**改坏实现即红**：把 `snapshot` 改成返回缓存值或
    /// 常量（如 `LinkCounters::default()`）⇒ 第二条断言红。
    struct Probe {
        a: AtomicU64,
        b: AtomicU64,
    }

    impl LinkCounterSource for Probe {
        fn snapshot(&self) -> LinkCounters {
            LinkCounters {
                iec104_dropped_total: self.a.load(Ordering::Relaxed),
                iec104_no_subscriber_total: self.b.load(Ordering::Relaxed),
                mqtt_enabled: false,
                ..Default::default()
            }
        }
    }

    /// A-4/A-5 判别力①：**出口读到的是实时值**，且每个字段各归各位。
    #[test]
    fn outlet_reads_live_values_and_maps_every_field() {
        let src = Probe {
            a: AtomicU64::new(0),
            b: AtomicU64::new(0),
        };
        let first = report_once(&src);
        assert_eq!(first.iec104_dropped_total, 0);
        assert_eq!(first.iec104_no_subscriber_total, 0);

        src.a.store(7, Ordering::Relaxed);
        src.b.store(3, Ordering::Relaxed);
        let second = report_once(&src);
        assert_eq!(
            second.iec104_dropped_total, 7,
            "读的是当拍值（非构造期快照）"
        );
        assert_eq!(
            second.iec104_no_subscriber_total, 3,
            "两个计数**不得互换**（dropped vs no_subscriber 语义不同）"
        );
    }

    /// A-4/A-5 判别力②：日志行**逐字段**可核对；MQTT 未启用时 4 个 MQTT 计数打 `n/a`
    /// —— **不得打 0**（0 会被读成"启用着且一切正常"）。
    #[test]
    fn log_line_carries_every_counter_and_marks_absent_channel_na() {
        let enabled = LinkCounters {
            iec104_dropped_total: 11,
            iec104_no_subscriber_total: 22,
            mqtt_published_total: 33,
            mqtt_failed_total: 44,
            mqtt_dropped_total: 55,
            mqtt_cached_len: 66,
            mqtt_enabled: true,
        };
        let line = enabled.log_line();
        for (name, val) in [
            ("iec104_dropped_total=11", ()),
            ("iec104_no_subscriber_total=22", ()),
            ("mqtt_published_total=33", ()),
            ("mqtt_failed_total=44", ()),
            ("mqtt_dropped_total=55", ()),
            ("mqtt_cached_len=66", ()),
        ] {
            assert!(line.contains(name), "日志行缺字段 {name}：{line}");
            let _ = val;
        }
        assert!(!line.contains("n/a"), "启用时不得出现 n/a：{line}");

        let disabled = LinkCounters {
            iec104_dropped_total: 11,
            mqtt_enabled: false,
            ..Default::default()
        };
        let line = disabled.log_line();
        assert!(line.contains("iec104_dropped_total=11"));
        assert_eq!(
            line.matches("n/a").count(),
            4,
            "MQTT 未启用 ⇒ 恰 4 个 MQTT 计数打 n/a（不打 0）：{line}"
        );
        assert!(
            !line.contains("mqtt_published_total=0"),
            "未启用不得打成 0（与'启用且正常'不可区分）：{line}"
        );
    }

    /// A-4 判别力③：**真 `Iec104Server` 实例**上的计数确实被出口读到
    /// （无连接 ⇒ `publish_asdus` 走 `NoSubscriber` 并计数）。
    #[tokio::test]
    async fn both_counters_reads_live_iec104_server() {
        let server = Arc::new(Iec104Server::new(
            mupc_gateway::iec104::server::Iec104Config::default(),
        ));
        let src = BothCounters {
            iec104: server.clone(),
            mqtt: None,
        };
        assert_eq!(src.snapshot().iec104_no_subscriber_total, 0);
        // 无连接 ⇒ NoSubscriber（§9.2.6）并计数
        let outcome = server
            .publish_asdus(vec![vec![0x0D]], mupc_gateway::iec104::DataClass::A)
            .await;
        assert_eq!(outcome, mupc_gateway::iec104::PublishOutcome::NoSubscriber);
        let snap = report_once(&src);
        assert_eq!(
            snap.iec104_no_subscriber_total, 1,
            "出口须读到真实服务器的当拍计数（A-4）"
        );
        assert_eq!(
            snap.iec104_no_subscriber_total,
            server.no_subscriber_total(),
            "出口值与访问器同源"
        );
        assert!(!snap.mqtt_enabled, "未传 MQTT 上送器 ⇒ mqtt_enabled=false");
        assert!(snap.log_line().contains("iec104_no_subscriber_total=1"));
        assert!(
            snap.log_line().contains("mqtt_cached_len=n/a"),
            "未启用侧须打 n/a"
        );
    }
}
