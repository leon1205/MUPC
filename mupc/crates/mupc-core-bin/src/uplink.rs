//! IEC 104 上送装配（T13 / 01 设计 §9.2.2 / §9.2.5 / §9.4 序 5+6）。
//!
//! **本模块是"快照 → A/B/C 三档上送 → 总召/初始快照"的落点**（不承载协议编码——
//! 编码在 `gateway::iec104`，点表在 `mupc-southd::uplink`，快照在
//! `mupc-data-processing::latest_values`；本模块只做**装配与档位调度**，符合 §9.0
//! 的依赖方向：core-bin 是唯一装配者）。
//!
//! # 三档任务（§9.2.2）
//!
//! | 档 | 触发 | 内容 | 背压 |
//! |----|------|------|------|
//! | A | `interval`（缺省 1000 ms，可配） | `points` 中 `class==A && channels.has(IEC104)` 且 `is_fresh` | `publish_asdus(.., A)`：`try_send` 满丢该批 + 计数 + 限频 WARN（**计数/WARN 由 gateway 侧 `Iec104Server` 持有**，T12 已实现，本模块不重复造第二套） |
//! | B | `interval`（缺省 5000 ms，可配） | 同上，`class==B` | 同上（B 档） |
//! | C | 订阅 `LatestValues` 变更批 | 变更点中 `class==C && channels.has(IEC104)` 且 `is_fresh` | `publish_asdus(.., C)`：`send().await` **不丢**（PRD EX-2） |
//!
//! **档位过滤是表驱动**：直接读 `UplinkPoint.class`（T11 在 `mupc-southd::uplink` 里
//! 已按 §9.2.2 表标注），**本模块不复制第二份点名清单**（任务约束）。
//!
//! # 总召 / 初始快照（§9.2.5）
//!
//! [`interrogation_items`] 是**一个实现两处调用**的取数函数：`StrategyCommandHandler`
//! 的 `on_interrogation`（总召 `C_IC_NA_1`）与 `on_connection_snapshot`（连接初始快照，
//! 默认转发到 `on_interrogation`）共用它。从 `LatestValues::all()` 过滤
//! `channels.has(IEC104) && is_fresh(..)`（位点走站级活性），**无有效值的点不出现**
//! （PRD GI-3：不得以 0 或旧值顶替），`cot = COT_INTROGEN(20)`。
//!
//! # 时标口径（§9.2.4，方案 A）
//!
//! 全部 IEC104 点（含既有总表 6 点）统一走 `TelemetryItem::encode_asdu`：标量 ⇒
//! `M_ME_TF_1`(TI=36) 带 CP56Time2a、位 ⇒ `M_SP_TB_1`(TI=30) 带 CP56Time2a。
//! 既有总表 6 点因此**由 TI=13（无时标）改为 TI=36（带时标）**（§9.2.4「默认按方案 A
//! 落设计」/ §9.8 Q2 默认 (a)），IOA 1–6 不变。旧的 `encode_telemetry_asdu`(TI=13) 路径
//! 随 `broadcast_grid_iec104` 删除（§9.4 序 4）。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use mupc_data_processing::latest_values::{LatestValues, PointView};
use mupc_gateway::iec104::command::{TelemetryItem, TelemetryKind};
use mupc_gateway::iec104::protocol::{COT_CYCLIC, COT_INTROGEN, COT_SPONT};
use mupc_gateway::iec104::server::{DataClass as GwDataClass, Iec104Server};
use mupc_southd::uplink::{ChannelMask, DataClass as SouthDataClass, UplinkKind, UplinkPoint};
use serde::Serialize;

/// A 档缺省周期（§9.2.2：1000 ms，「周期须可配置」⇒ 经 [`Iec104UplinkDriver::new`] 注入）。
pub const DEFAULT_CLASS_A_INTERVAL: Duration = Duration::from_millis(1000);
/// B 档缺省周期（§9.2.2：5000 ms，可配）。
pub const DEFAULT_CLASS_B_INTERVAL: Duration = Duration::from_millis(5000);

/// IEC 104 上送驱动器（§9.4 序 5）：持快照 + 点表 + 服务器句柄，spawn A/B/C 三条任务。
///
/// 构造**无 I/O**；[`Iec104UplinkDriver::spawn`] 才起任务。三任务均无停机钩子（不写存储、
/// 在途无未落盘批次）⇒ 入装配层的 **abort 名单**（`TaskGuard`），与网关/指标采集同范式。
pub struct Iec104UplinkDriver {
    latest: Arc<LatestValues>,
    points: Arc<Vec<UplinkPoint>>,
    server: Arc<Iec104Server>,
    class_a_interval: Duration,
    class_b_interval: Duration,
}

impl Iec104UplinkDriver {
    /// 构造。`class_a_interval` / `class_b_interval` 为 A/B 档周期（§9.2.2「周期须可配置」
    /// ⇒ 由调用方注入；装配层传 [`DEFAULT_CLASS_A_INTERVAL`] / [`DEFAULT_CLASS_B_INTERVAL`]，
    /// 后续可改读配置）。C 档为 COS（变更驱动），无周期。构造**无 I/O**。
    pub fn new(
        latest: Arc<LatestValues>,
        points: Arc<Vec<UplinkPoint>>,
        server: Arc<Iec104Server>,
        class_a_interval: Duration,
        class_b_interval: Duration,
    ) -> Self {
        Self {
            latest,
            points,
            server,
            class_a_interval,
            class_b_interval,
        }
    }

    /// spawn A/B/C 三条任务，返回句柄（装配层入 abort 名单）。
    pub fn spawn(self: &Arc<Self>) -> Vec<tokio::task::JoinHandle<()>> {
        vec![
            self.spawn_periodic(SouthDataClass::A, self.class_a_interval),
            self.spawn_periodic(SouthDataClass::B, self.class_b_interval),
            self.spawn_cos(),
        ]
    }

    /// A/B 档：单拍定时任务（§9.2.2「每档一个 interval 单拍任务，不逐点各起定时器」）。
    ///
    /// 到点从快照过滤本档点 → 编码 → `publish_asdus`。**空批不发**（无有效点时不制造
    /// `NoSubscriber` 噪声，§9.2.6「主站未连接不缓存、不入队」由 `publish_asdus` 兜底）。
    fn spawn_periodic(
        self: &Arc<Self>,
        class: SouthDataClass,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            // 慢拍不补偿（跳过错过的 tick），防停顿后一次性补发多批
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let now_ms = now_millis();
                let items = build_items(
                    &me.latest,
                    &me.points,
                    now_ms,
                    COT_CYCLIC,
                    Some(class),
                    None,
                );
                if items.is_empty() {
                    continue;
                }
                let asdus: Vec<Vec<u8>> = items.iter().map(TelemetryItem::encode_asdu).collect();
                // 背压/丢弃计数/限频 WARN 由 gateway 侧 publish_asdus 持有（T12）
                let _ = me.server.publish_asdus(asdus, gw_class(class)).await;
            }
        })
    }

    /// C 档：订阅变更批 → 过滤 C 档 IEC104 点 → 一次 `publish_asdus(.., C)` 批量入队（合并）。
    ///
    /// **值不变不发**由 `LatestValues::apply` 去重保证（§9.1.5）；`Lagged` ⇒ 全量重读
    /// 重发所有新鲜 C 档点（§9.1.5 消费方契约，"丢失由兜底收敛"）。C 档 `send().await`
    /// **绝不静默丢遥信变位**（PRD EX-2）。
    fn spawn_cos(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            let mut rx = me.latest.subscribe();
            loop {
                match rx.recv().await {
                    Ok(batch) => {
                        let changed: HashSet<(String, String)> = batch
                            .changed
                            .into_iter()
                            .map(|id| (id.station, id.metric))
                            .collect();
                        let items = build_items(
                            &me.latest,
                            &me.points,
                            now_millis(),
                            COT_SPONT,
                            Some(SouthDataClass::C),
                            Some(&changed),
                        );
                        if items.is_empty() {
                            continue;
                        }
                        let asdus: Vec<Vec<u8>> =
                            items.iter().map(TelemetryItem::encode_asdu).collect();
                        let _ = me.server.publish_asdus(asdus, GwDataClass::C).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        // 落后丢帧 ⇒ 全量重读重发 C 档（§9.1.5）；不静默吞掉
                        tracing::debug!(lagged = n, "IEC104 C 档订阅落后 ⇒ 全量重读重发");
                        let items = build_items(
                            &me.latest,
                            &me.points,
                            now_millis(),
                            COT_SPONT,
                            Some(SouthDataClass::C),
                            None,
                        );
                        if !items.is_empty() {
                            let asdus: Vec<Vec<u8>> =
                                items.iter().map(TelemetryItem::encode_asdu).collect();
                            let _ = me.server.publish_asdus(asdus, GwDataClass::C).await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        })
    }
}

/// southd 档位 → gateway 档位（§9.2.2 注：`DataClass` 在 gateway 侧镜像定义，装配层逐值映射）。
fn gw_class(c: SouthDataClass) -> GwDataClass {
    match c {
        SouthDataClass::A => GwDataClass::A,
        SouthDataClass::B => GwDataClass::B,
        SouthDataClass::C => GwDataClass::C,
    }
}

/// 采集时刻 UTC 毫秒（不得用发送时刻顶替，§8.4）。
fn now_millis() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

/// **核心过滤/编码**（纯函数，无 I/O ⇒ 可单测；A/B/C 档任务与总召共用）。
///
/// 从 `latest.all()` 建 `(station, metric) → PointView` 索引，遍历 `points`：
/// 1. 只取 `channels.has(IEC104)`（MQTT-only 点不进 IEC104，§9.2.1.0 / C-16）；
/// 2. `class` 为 `Some(c)` 时只取该档（总召传 `None` = 全档）；
/// 3. `only_changed` 为 `Some(set)` 时只取变更点（C 档 COS）；
/// 4. `is_fresh`（标量逐点时标 / 位点站级活性，§9.1.3）且 `value.is_some()`
///    ⇒ 产 [`TelemetryItem`]（**无有效值的点不出现**，GI-3）；否则跳过。
///
/// `cot` 由调用方给（周期=1 / 突发=3 / 总召=20）。**时标 = 该点采集时刻**（`pv.value.ts_ms`），
/// 不是响应/发送时刻（§8.4）。
pub(crate) fn build_items(
    latest: &LatestValues,
    points: &[UplinkPoint],
    now_ms: u64,
    cot: u8,
    class: Option<SouthDataClass>,
    only_changed: Option<&HashSet<(String, String)>>,
) -> Vec<TelemetryItem> {
    // 单次全量读 + 建索引（避免逐点 get 反复取锁，§9.1.6）
    let idx: HashMap<(String, String), PointView> = latest
        .all()
        .into_iter()
        .map(|pv| ((pv.id.station.clone(), pv.id.metric.clone()), pv))
        .collect();

    let mut out: Vec<TelemetryItem> = Vec::new();
    for p in points {
        if !p.channels.has(ChannelMask::IEC104) {
            continue;
        }
        if let Some(want) = class {
            if p.class != want {
                continue;
            }
        }
        let key = (p.station.clone(), p.metric.clone());
        if let Some(changed) = only_changed {
            if !changed.contains(&key) {
                continue;
            }
        }
        let Some(pv) = idx.get(&key) else {
            continue;
        };
        let is_bit = matches!(p.kind, UplinkKind::Bit);
        if !latest.is_fresh(&pv.id, &pv.value, is_bit, now_ms) {
            continue;
        }
        // GI-3：无有效值（None）不出现，不以 0 顶替
        let Some(v) = pv.value.value else {
            continue;
        };
        out.push(TelemetryItem {
            ioa: p.ioa,
            kind: match p.kind {
                UplinkKind::Scalar => TelemetryKind::Scalar,
                UplinkKind::Bit => TelemetryKind::Bit,
            },
            value: v as f32,
            ts_ms: pv.value.ts_ms,
            cot,
        });
    }
    out
}

/// **总召 / 连接初始快照取数**（§9.2.5，`cot = COT_INTROGEN(20)`，全档）。
///
/// `StrategyCommandHandler::on_interrogation` 与 `on_connection_snapshot` 共用本函数
/// （**一个实现两处调用**，防两套口径）。无有效值的点不出现（GI-3）。
pub(crate) fn interrogation_items(
    latest: &LatestValues,
    points: &[UplinkPoint],
    now_ms: u64,
) -> Vec<TelemetryItem> {
    build_items(latest, points, now_ms, COT_INTROGEN, None, None)
}

/// `uplink_points.json` 的可序列化镜像（`UplinkPoint` 未派生 `Serialize`，且**不得改
/// `mupc-southd`**（任务约束）⇒ 在装配层建镜像）。
#[derive(Serialize)]
struct UplinkPointJson {
    ioa: u32,
    station: String,
    metric: String,
    /// `"scalar"` / `"bit"`
    kind: &'static str,
    /// `"A"` / `"B"` / `"C"`
    class: &'static str,
    /// 通道掩码原值（`0b01`=IEC104 / `0b10`=MQTT / `0b11`=BOTH）
    channels: u8,
    label: String,
}

impl From<&UplinkPoint> for UplinkPointJson {
    fn from(p: &UplinkPoint) -> Self {
        Self {
            ioa: p.ioa,
            station: p.station.clone(),
            metric: p.metric.clone(),
            kind: match p.kind {
                UplinkKind::Scalar => "scalar",
                UplinkKind::Bit => "bit",
            },
            class: match p.class {
                SouthDataClass::A => "A",
                SouthDataClass::B => "B",
                SouthDataClass::C => "C",
            },
            channels: p.channels.0,
            label: p.label.to_string(),
        }
    }
}

/// 把点表落 `uplink_points.json`（§9.2.1「启动期产物」：供 RC-U74-02 与主站逐点对点）。
///
/// 先建父目录再写（`system.data_dir` 可能尚不存在）。**pretty JSON 数组**，逐点含
/// `ioa/station/metric/kind/class/channels/label`。写失败由调用方决定如何观测（本函数
/// 只如实返回 `io::Result`，不 panic、不静默）。
pub(crate) async fn write_uplink_points_json(
    path: &std::path::Path,
    points: &[UplinkPoint],
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let json: Vec<UplinkPointJson> = points.iter().map(UplinkPointJson::from).collect();
    let text = serde_json::to_string_pretty(&json)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    tokio::fs::write(path, text).await
}

// ───────────────────────────── 单测（01 设计 §9.5「core-bin」行 + 任务交付物 3） ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use mupc_data_processing::latest_values::{PointId, PointQuality, PointValue};
    use mupc_southd::config::SouthStationsConfig;
    use mupc_southd::uplink::build_uplink_points;

    /// 6 站参考配置（含 PCS）——**复用 T11 的同一份 fixture**（不新建第二份点表真源）。
    const REF_6: &str = include_str!("../../mupc-southd/tests/fixtures/south_stations_s3b2.yaml");

    #[derive(serde::Deserialize)]
    struct Wrapper {
        south_stations: SouthStationsConfig,
    }

    fn cfg() -> SouthStationsConfig {
        let w: Wrapper = serde_yaml::from_str(REF_6).expect("6 站参考配置解析失败");
        w.south_stations
    }

    fn points() -> Vec<UplinkPoint> {
        build_uplink_points(&cfg()).expect("build_uplink_points 必须成功（6 站参考配置）")
    }

    fn id(station: &str, metric: &str) -> PointId {
        PointId {
            station: station.to_string(),
            metric: metric.to_string(),
        }
    }

    fn ok_val(v: f64, ts: u64) -> PointValue {
        PointValue {
            value: Some(v),
            ts_ms: ts,
            quality: PointQuality::Ok,
        }
    }

    /// **A 档枚举（6+9+4+3=22，PCS 启用）**：grid 6 + meter_batt 9 + bms 4 + pcs 3。
    /// 表驱动（读 `UplinkPoint.class`），逐点可枚举（§9.2.2）。
    #[test]
    fn class_a_enumerates_22_points() {
        let pts = points();
        let a: Vec<&UplinkPoint> = pts
            .iter()
            .filter(|p| p.class == SouthDataClass::A && p.channels.has(ChannelMask::IEC104))
            .collect();
        assert_eq!(
            a.len(),
            22,
            "A 档（PCS 启用）= grid6 + meter_batt9 + bms4 + pcs3 = 22（§9.2.2）"
        );
        // 全部走 IEC104 通道
        assert!(a.iter().all(|p| p.channels.has(ChannelMask::IEC104)));

        let mut metrics: Vec<&str> = a.iter().map(|p| p.metric.as_str()).collect();
        metrics.sort_unstable();
        let mut expected = [
            // grid 6
            "active_power",
            "reactive_power",
            "voltage",
            "current",
            "cos_phi",
            "frequency",
            // meter_batt 9
            "mb_power_7",
            "mb_power_15",
            "mb_ui_1",
            "mb_ui_2",
            "mb_ui_3",
            "mb_ui_4",
            "mb_ui_5",
            "mb_ui_6",
            "mb_freq_line_1",
            // bms 4
            "soc",
            "bms_io_16",
            "bms_io_17",
            "bms_meta_6",
            // pcs 3
            "pcs_3zone_14",
            "pcs_3zone_33",
            "pcs_3zone_37",
        ];
        expected.sort_unstable();
        assert_eq!(metrics, expected, "A 档点名逐点枚举（§9.2.2 表）");

        // grid 6 点 IOA = 1..6（现场追认，不得改号，§9.2.1 段 1）
        let mut grid_ioas: Vec<u32> = a
            .iter()
            .filter(|p| {
                matches!(
                    p.metric.as_str(),
                    "active_power"
                        | "reactive_power"
                        | "voltage"
                        | "current"
                        | "cos_phi"
                        | "frequency"
                )
            })
            .map(|p| p.ioa)
            .collect();
        grid_ioas.sort_unstable();
        assert_eq!(grid_ioas, vec![1, 2, 3, 4, 5, 6], "grid IOA 1–6 不得改号");
    }

    /// **A 档 build_items**：全部 A 点新鲜 ⇒ 22 条 `TelemetryItem`，`cot=周期(1)`、IOA 正确。
    #[test]
    fn build_items_a_class_returns_fresh_points_with_cot_cyclic() {
        let pts = points();
        let latest = LatestValues::new(5);
        let now = 1_000_000u64;
        let samples: Vec<(PointId, PointValue)> = pts
            .iter()
            .filter(|p| p.class == SouthDataClass::A)
            .map(|p| (id(&p.station, &p.metric), ok_val(1.5, now)))
            .collect();
        latest.apply(samples);

        let items = build_items(
            &latest,
            &pts,
            now,
            COT_CYCLIC,
            Some(SouthDataClass::A),
            None,
        );
        assert_eq!(items.len(), 22, "A 档 22 点全部新鲜 ⇒ 22 条");
        assert!(items.iter().all(|it| it.cot == COT_CYCLIC), "周期 cot=1");
        assert!(
            items.iter().all(|it| (it.value - 1.5f32).abs() < 1e-6),
            "值透传"
        );
        assert!(items.iter().any(|it| it.ioa == 1), "含 grid IOA 1");
        // 标量 A 点 ⇒ TI=36。`encode_asdu` 只产 **ASDU**（不含 6 B APCI/I 帧头，§9.2.3
        // 「载荷改为 ASDU」）：类型1+VSQ1+COT1+CA2+IOA3+短浮点4+CP56Time2a7 = **19 B**
        // （整帧 = APCI 6 + 19 = 25 B，§9.2.2 帧长表）。
        let asdu = items[0].encode_asdu();
        assert_eq!(
            asdu.len(),
            19,
            "TI=36 ASDU 19 B（整帧 25 B，§9.2.2/§9.2.4）"
        );
    }

    /// **总召 GI-3（无有效值的点不出现）**：过期 / Invalid / None 三类均被排除，只发新鲜 Ok 点。
    #[test]
    fn interrogation_excludes_stale_invalid_and_none() {
        let pts = points();
        let latest = LatestValues::new(5);
        let now = 1_000_000u64;
        // 取 grid 4 个点（IOA 1–4），各置不同状态
        latest.apply(vec![
            // 新鲜 Ok ⇒ 出现
            (id("grid_meter", "active_power"), ok_val(10.0, now)),
            // 过期（ts 超 5 s）⇒ 不出现
            (
                id("grid_meter", "reactive_power"),
                ok_val(20.0, now - 10_000),
            ),
            // Invalid 质量 ⇒ 不出现
            (
                id("grid_meter", "voltage"),
                PointValue {
                    value: Some(30.0),
                    ts_ms: now,
                    quality: PointQuality::Invalid,
                },
            ),
            // 值 None（不可得）⇒ 不出现（不以 0 顶替）
            (
                id("grid_meter", "current"),
                PointValue {
                    value: None,
                    ts_ms: now,
                    quality: PointQuality::Invalid,
                },
            ),
        ]);

        let items = interrogation_items(&latest, &pts, now);
        assert_eq!(
            items.len(),
            1,
            "GI-3：只有新鲜 Ok 点出现（过期/Invalid/None 均排除）"
        );
        assert_eq!(items[0].ioa, 1);
        assert!((items[0].value - 10.0f32).abs() < 1e-6);
        assert_eq!(items[0].cot, COT_INTROGEN, "总召 cot=20");
        assert_eq!(items[0].ts_ms, now, "时标=采集时刻，不是响应时刻（§8.4）");
    }

    /// **C 档 COS：值不变不发 / 变位合批 / 时标=采集时刻**。
    ///
    /// 用真 `LatestValues` 的广播：① 同值 apply ⇒ 无变更批（`apply` 返回 None）；
    /// ② 两点同批变位 ⇒ **一个**变更批含两点 ⇒ C 档一次发两条（合并）。
    #[tokio::test]
    async fn c_class_cos_unchanged_silent_and_changed_batched() {
        let pts = points();
        let latest = LatestValues::new(5);
        let now = 1_000_000u64;
        // bms_aggr_cluster_voltage（IOA 301）/ bms_aggr_cell_temp（IOA 304）均为 C 档 Bit IEC104
        let cv = id("bms", "bms_aggr_cluster_voltage");
        let ct = id("bms", "bms_aggr_cell_temp");
        latest.mark_station_polled("bms", now); // 位点走站级活性

        // 首轮登记两点为 0（产生首批）
        latest.apply(vec![
            (cv.clone(), ok_val(0.0, now)),
            (ct.clone(), ok_val(0.0, now)),
        ]);

        // ① 值不变 ⇒ 不广播（COS「值不变不发」）
        let mut rx = latest.subscribe();
        let seq = latest.apply(vec![
            (cv.clone(), ok_val(0.0, now)),
            (ct.clone(), ok_val(0.0, now)),
        ]);
        assert!(seq.is_none(), "同值 apply ⇒ 无变更批（值不变不发）");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "无变更批 ⇒ 订阅方收不到（不触发 C 档上送）"
        );

        // ② 两点同批变位 ⇒ 一个变更批含两点 ⇒ C 档一次发两条（合批）
        let mut rx2 = latest.subscribe();
        let seq2 = latest.apply(vec![
            (cv.clone(), ok_val(1.0, now)),
            (ct.clone(), ok_val(1.0, now)),
        ]);
        assert!(seq2.is_some(), "变位 ⇒ 产生变更批");
        let batch = tokio::time::timeout(Duration::from_secs(1), rx2.recv())
            .await
            .expect("≤1 s 内收到变更批（COS 时延）")
            .expect("订阅方收到批");
        assert_eq!(batch.changed.len(), 2, "两点合为一批");

        let changed: HashSet<(String, String)> = batch
            .changed
            .into_iter()
            .map(|i| (i.station, i.metric))
            .collect();
        let items = build_items(
            &latest,
            &pts,
            now,
            COT_SPONT,
            Some(SouthDataClass::C),
            Some(&changed),
        );
        assert_eq!(items.len(), 2, "C 档一次发两条（合批）");
        assert!(items.iter().all(|it| it.cot == COT_SPONT), "突发 cot=3");
        assert!(items.iter().all(|it| it.kind == TelemetryKind::Bit));
        let mut ioas: Vec<u32> = items.iter().map(|it| it.ioa).collect();
        ioas.sort_unstable();
        assert_eq!(ioas, vec![301, 304], "聚合点 IOA 301/304");
        // 位点 ⇒ TI=30。ASDU = 类型1+VSQ1+COT1+CA2+IOA3+SIQ1+CP56Time2a7 = **16 B**
        // （整帧 = APCI 6 + 16 = 22 B，§9.2.2 帧长表）。
        assert_eq!(
            items[0].encode_asdu().len(),
            16,
            "TI=30 ASDU 16 B（整帧 22 B，§9.2.2/§9.2.4）"
        );
    }

    /// **C 档只发变更点**：变更集只含一点 ⇒ 只发该点（未变位点不重发）。
    #[tokio::test]
    async fn c_class_emits_only_changed_points() {
        let pts = points();
        let latest = LatestValues::new(5);
        let now = 1_000_000u64;
        let cv = id("bms", "bms_aggr_cluster_voltage");
        let ct = id("bms", "bms_aggr_cell_temp");
        latest.mark_station_polled("bms", now);
        latest.apply(vec![
            (cv.clone(), ok_val(0.0, now)),
            (ct.clone(), ok_val(0.0, now)),
        ]);

        // 只变 cv
        let mut rx = latest.subscribe();
        latest.apply(vec![
            (cv.clone(), ok_val(1.0, now)),
            (ct.clone(), ok_val(0.0, now)), // 不变
        ]);
        let batch = rx.recv().await.expect("批");
        assert_eq!(batch.changed.len(), 1, "只有 cv 变更");
        let changed: HashSet<(String, String)> = batch
            .changed
            .into_iter()
            .map(|i| (i.station, i.metric))
            .collect();
        let items = build_items(
            &latest,
            &pts,
            now,
            COT_SPONT,
            Some(SouthDataClass::C),
            Some(&changed),
        );
        assert_eq!(items.len(), 1, "只发变更点");
        assert_eq!(items[0].ioa, 301);
        assert!((items[0].value - 1.0f32).abs() < 1e-6);
    }

    /// **删除假遥测后 IOA 1–6 只来自 SouthSink（grid 派生 6 点）**：
    /// ① 静态断言——`startup.rs` 不再含 `ioa_seq` / `broadcast_telemetry` / `make_i_frame` /
    ///    `encode_telemetry_asdu`（§9.4 序 10 / §9.5「静态/结构」防回退）；
    /// ② 结构断言——grid IOA 1–6 由 `build_uplink_points` 的固定派生表产出（非手写常量）。
    #[test]
    fn fake_telemetry_path_removed_and_grid_ioa_from_generator() {
        // ① 静态断言：扫描 startup.rs 源文本（本测试在 uplink.rs，不扫自身）
        let src = include_str!("startup.rs");
        for token in [
            "ioa_seq",
            "broadcast_telemetry",
            "make_i_frame",
            "encode_telemetry_asdu",
        ] {
            assert!(
                !src.contains(token),
                "startup.rs 不得再含 `{token}`（T13 删除 south_sim_loop 假遥测支路 + \
                 broadcast_grid_iec104，§9.4 序 4/10、§9.5 静态断言防回退）"
            );
        }

        // ② grid IOA 1–6 由生成器固定表产出（唯一真源，非手写常量）
        let pts = points();
        let grid: Vec<&UplinkPoint> = pts
            .iter()
            .filter(|p| p.station == "grid_meter" && p.channels.has(ChannelMask::IEC104))
            .collect();
        assert_eq!(grid.len(), 6, "grid 段固定 6 点");
        let mut ioas: Vec<u32> = grid.iter().map(|p| p.ioa).collect();
        ioas.sort_unstable();
        assert_eq!(ioas, vec![1, 2, 3, 4, 5, 6]);
        // 6 点均 A 档标量（走 TI=36）
        assert!(grid.iter().all(|p| p.class == SouthDataClass::A));
        assert!(grid.iter().all(|p| p.kind == UplinkKind::Scalar));
    }

    /// **档位三档之和 = IEC104 通道点数**（§9.2.2 恒等式；PCS 启用 22+153+59=234）。
    #[test]
    fn abc_class_counts_sum_to_iec104_total() {
        let pts = points();
        let iec: Vec<&UplinkPoint> = pts
            .iter()
            .filter(|p| p.channels.has(ChannelMask::IEC104))
            .collect();
        let a = iec.iter().filter(|p| p.class == SouthDataClass::A).count();
        let b = iec.iter().filter(|p| p.class == SouthDataClass::B).count();
        let c = iec.iter().filter(|p| p.class == SouthDataClass::C).count();
        assert_eq!((a, b, c), (22, 153, 59), "PCS 启用 A/B/C = 22/153/59");
        assert_eq!(iec.len(), 234, "IEC104 通道 234 点（PCS 启用）");
        assert_eq!(a + b + c, iec.len(), "三档之和 = 通道点数");
    }

    /// **uplink_points.json 落盘**：写入后可读回、点数一致、含 IOA 字段。
    #[tokio::test]
    async fn write_uplink_points_json_roundtrip() {
        let pts = points();
        let t = crate::testutil::TempDir::new("uplink-json");
        let path = t.join("uplink_points.json");
        write_uplink_points_json(&path, &pts)
            .await
            .expect("落盘成功");
        let text = tokio::fs::read_to_string(&path).await.expect("可读回");
        let v: serde_json::Value = serde_json::from_str(&text).expect("合法 JSON");
        let arr = v.as_array().expect("数组");
        assert_eq!(arr.len(), pts.len(), "JSON 条数 = 点表条数");
        // 首条含 ioa/metric 字段
        assert!(arr[0].get("ioa").is_some());
        assert!(arr[0].get("metric").is_some());
        assert!(arr[0].get("channels").is_some());
    }
}
