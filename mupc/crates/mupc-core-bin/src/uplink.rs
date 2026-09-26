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

    // **原子落盘（T13 独立评审 B2）**：先写同目录临时文件再 rename（Windows 的
    // `std::fs::rename` 带 MOVEFILE_REPLACE_EXISTING ⇒ 可覆盖）。这样任何一步失败都
    // **不会留下半截 JSON**；且失败时**一并删除旧目标文件**——旧基线冒充本次基线
    // 对点人员比对**比"没有文件"更误导**（该 JSON 是派生产物，每次启动重生成）。
    let tmp = path.with_extension("json.tmp");
    let write_then_rename = async {
        tokio::fs::write(&tmp, text).await?;
        tokio::fs::rename(&tmp, path).await
    };
    if let Err(e) = write_then_rename.await {
        let _ = tokio::fs::remove_file(&tmp).await;
        let _ = tokio::fs::remove_file(path).await;
        return Err(e);
    }
    Ok(())
}

// ═══════════════════════════ 01 设计 §9.3 MQTT 上送（U-71） ═══════════════════════════
//
// 本文件同放**两个上送器**（§9.3.3 落点要求）：`Iec104UplinkDriver`（上一节）与
// `MqttUplinkPublisher`（本节），二者**共用同一份点表**（`Arc<Vec<UplinkPoint>>`）与
// 过滤口径（`channels.has(..)`），只是通道掩码不同（C-16：IEC104 234 / MQTT 624）。
//
// **禁轮询 DB（LV-2 / CNS-01）**：本节的取数只有两个入口——
// ① `LatestValues`（内存快照）；② `LatestValues::subscribe()` 的**变更批**。
// 任何"读 telemetry 表"的写法都是缺陷（PRD LV-2 明文禁止）。
//
// **加载顺序（§9.6 第 6/7/8 步）**：配置（core_config）→ mqtt-bridge 能力（主题函数/证书）
// → 本节的接线。故本节的 `unwrap` 一律不存在：失败路径全部显式上报。

use crate::core_config::MqttBridgeConfig;
use mupc_data_processing::latest_values::PointQuality;
use mupc_mqtt_bridge::{LocalMqttClient, LocalMqttConfig, NorthMqttClient};
use mupc_storage::{EventRepository, SystemEvent};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

/// 缺省 A 档周期（ms）——与 `core_config::PeriodsCfg::default()` **同源值**（§9.3.2）。
pub const DEFAULT_MQTT_A_MS: u64 = 1000;
/// 缺省 B 档周期（ms）。
pub const DEFAULT_MQTT_B_MS: u64 = 5000;
/// 缺省 COS 合并窗（ms）。
pub const DEFAULT_MQTT_COS_MERGE_MS: u64 = 200;
/// MQTT 通道点数（PCS 未启用）/（PCS 启用）—— AC-U74-02 的机械判据（§9.3.3）。
pub const MQTT_POINTS_WITHOUT_PCS: usize = 552;
/// 见 [`MQTT_POINTS_WITHOUT_PCS`]。
pub const MQTT_POINTS_WITH_PCS: usize = 624;
/// 连续发布失败告警门限（§9.3.6：≥10 条 ⇒ 一条 major 事件）。
pub const MQTT_FAILURE_ALERT_THRESHOLD: u64 = 10;
/// 连续失败告警的**最小间隔**（§9.3.6：之后每 5 min 最多一条，不风暴）。
pub const MQTT_FAILURE_ALERT_MIN_INTERVAL_MS: u64 = 300_000;
/// 缓存淘汰 WARN 的聚合窗（§9.3.3：按 1 min 聚合，不风暴）。
pub const MQTT_CACHE_WARN_INTERVAL_MS: u64 = 60_000;
/// 证书到期检查周期（§9.3.5 TLS-3：启动期 + 每日）。
pub const CERT_CHECK_INTERVAL: Duration = Duration::from_secs(24 * 3600);

// ───────────────────────────── 配置映射（C-12：逐字段） ─────────────────────────────

/// YAML(`core_config.mqtt_bridge.north`) → `mqtt-bridge::NorthMqttConfig`（§9.3.2 / C-12）。
///
/// **逐字段映射函数**（设计明文要求"写成本函数 + 单测"）：`enabled` 之外还搬 broker /
/// client_id / username / password / 三件证书 / `allow_plaintext`；`keepalive_secs` 与
/// `reconnect` 取 crate 缺省（YAML 层未建模这两项——**不发明**配置键）。
pub(crate) fn north_client_config(
    cfg: &crate::core_config::NorthCfg,
) -> mupc_mqtt_bridge::NorthMqttConfig {
    mupc_mqtt_bridge::NorthMqttConfig {
        enabled: cfg.enabled,
        broker_addr: cfg.broker.clone(),
        client_id: cfg.client_id.clone(),
        username: cfg.username.clone(),
        password: cfg.password.clone(),
        allow_plaintext: cfg.tls.allow_plaintext,
        keepalive_secs: 60,
        tls: mupc_mqtt_bridge::TlsConfig {
            ca_cert: cfg.tls.ca_cert.clone().into(),
            client_cert: cfg.tls.client_cert.clone().into(),
            client_key: cfg.tls.client_key.clone().into(),
        },
        reconnect: mupc_mqtt_bridge::ReconnectConfig::default(),
    }
}

/// YAML(`mqtt_bridge.local`) → `mqtt-bridge::LocalMqttConfig`（§9.3.2）。
pub(crate) fn local_client_config(cfg: &crate::core_config::LocalCfg) -> LocalMqttConfig {
    LocalMqttConfig {
        enabled: cfg.enabled,
        broker_addr: cfg.broker.clone(),
        client_id: cfg.client_id.clone(),
        ..LocalMqttConfig::default()
    }
}

/// 装配决策（**纯函数**）：`enabled=false` ⇒ 两个 `Option` 全 `None`。
///
/// ⚠️ 这是"**零连接尝试**"（CFG-2）的**机理**：装配点的一切建连代码都在
/// `if let Some(..) = launch.north` / `launch.local` 之内 ⇒ 未启用时**一行连接代码都不执行**
/// （既不 `new` 也不 `spawn`）。`mqtt_bridge.enabled=false` 与"分向全 false"都落 `None`。
pub(crate) struct MqttLaunch {
    pub north: Option<mupc_mqtt_bridge::NorthMqttConfig>,
    pub local: Option<LocalMqttConfig>,
}

impl MqttLaunch {
    /// 是否需要**任何** MQTT 动作（false ⇒ 装配层可整段跳过）。
    pub fn is_empty(&self) -> bool {
        self.north.is_none() && self.local.is_none()
    }
}

/// 见 [`MqttLaunch`]。
pub(crate) fn plan_mqtt_launch(cfg: &MqttBridgeConfig) -> MqttLaunch {
    // 总开关关闭 ⇒ 分向开关**一律**不再看（防"enabled=false 但 north.enabled=true 时仍连"）
    if !cfg.enabled {
        return MqttLaunch {
            north: None,
            local: None,
        };
    }
    MqttLaunch {
        north: cfg.north.enabled.then(|| north_client_config(&cfg.north)),
        local: cfg.local.enabled.then(|| local_client_config(&cfg.local)),
    }
}

// ───────────────────────────── 装配结果与探针 ─────────────────────────────

/// MQTT 服务注册状态（装配层据此 `coord.register_service`，§9.4 序 7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MqttServiceStatus {
    /// 未启用 ⇒ **不注册动作、零连接尝试**（`Stopped`）。
    Disabled,
    /// 至少一个客户端已装配并起了事件循环。
    Running,
    /// 启用但建连对象构造失败 ⇒ `Failed`（**不重试明文**，TLS-2）。
    Failed,
}

/// 装配结果（§9.4 序 7）。`clients_constructed` 是**零连接尝试**的机械探针：
/// 未启用时必为 0（用例 `disabled_config_never_constructs_a_client_or_connects`）。
pub struct MqttAssemblyOutcome {
    /// 现场构造的客户端对象数（North + Local）。**0 = 零连接尝试的结构性上界**
    /// （没有任何客户端对象 ⇒ 不存在任何连接路径）。
    pub clients_constructed: usize,
    /// 起的后台任务（装配层入 abort 名单/协作名单）。未启用 ⇒ 空。
    pub tasks: Vec<(&'static str, tokio::task::JoinHandle<()>)>,
    /// 服务状态。
    pub status: MqttServiceStatus,
    /// 失败原因（`Failed` 时非空；**不得含凭据**）。
    pub detail: Option<String>,
}

/// MQTT 装配（**唯一**的 `NorthMqttClient`/`LocalMqttClient` 构造点，§9.4 序 7）。
///
/// 抽成独立函数的理由与 `stop_producers` 同（"把裁决点抽出来单测"）：真环境
/// `initialize_all` 起不来（需 DB/串口/sysfs），而"**未配置 ⇒ 零连接尝试**"这条
/// 回归防护（CFG-2）必须**可机械验证** ⇒ 本函数是可被用例直接调用的接缝。
///
/// 返回的句柄**由调用方持有**（本函数不 spawn 到自己的名单）：启用时按 §9.3.3
/// 起 2 条 A/B 定时任务 + 1 条 COS/健康/补送任务 + 1 条事件循环任务。
pub async fn assemble_mqtt_bridge(
    cfg: &MqttBridgeConfig,
    latest: Arc<LatestValues>,
    points: Arc<Vec<UplinkPoint>>,
    roles: Arc<HashMap<String, String>>,
    dev: Option<String>,
    events: Arc<dyn EventRepository>,
) -> MqttAssemblyOutcome {
    let launch = plan_mqtt_launch(cfg);
    let mut out = MqttAssemblyOutcome {
        clients_constructed: 0,
        tasks: Vec::new(),
        status: MqttServiceStatus::Disabled,
        detail: None,
    };
    if launch.is_empty() {
        // **零连接尝试**：没有客户端对象、没有任务、没有连接（CFG-2）
        tracing::debug!(
            "mqtt_bridge 未启用（enabled/north/local 均未开）⇒ 零连接尝试：不构造客户端、不 spawn 事件循环"
        );
        return out;
    }

    // ── 本地 mosquitto（进程间）──
    if let Some(local_cfg) = launch.local.as_ref() {
        match LocalMqttClient::new(local_cfg) {
            Ok(c) => {
                out.clients_constructed += 1;
                let c = Arc::new(c);
                out.tasks.push((
                    "mqtt_bridge_local",
                    tokio::spawn(async move {
                        let _ = c.run().await;
                    }),
                ));
                tracing::info!(
                    broker = %local_cfg.broker_addr,
                    "本地 MQTT 客户端已装配（事件循环已起）"
                );
            }
            Err(e) => {
                tracing::error!("本地 MQTT 客户端初始化失败: {e}");
                out.detail = Some(format!("local: {e}"));
            }
        }
    }

    // ── 北向（物联平台）──
    if let Some(north_cfg) = launch.north.as_ref() {
        match NorthMqttClient::new(north_cfg) {
            Ok(c) => {
                out.clients_constructed += 1;
                let c = Arc::new(c);
                // TLS-4：明文例外必须**响亮**（ERROR 日志 + major 事件），不得静默
                if c.is_plaintext() {
                    tracing::error!(
                        broker = %north_cfg.broker_addr,
                        "⚠️ 北向 MQTT 以**明文**运行（mqtt_bridge.north.tls.allow_plaintext=true）\
                         ——仅允许在非生产环境（Q9/TLS-4）"
                    );
                    let ev = SystemEvent {
                        id: None,
                        timestamp: chrono::Utc::now(),
                        event_type: "mqtt_plaintext_enabled".to_string(),
                        source: "mqtt_bridge".to_string(),
                        message: format!(
                            "北向 MQTT 明文传输已启用（broker={}）；生产环境禁止",
                            north_cfg.broker_addr
                        ),
                    };
                    if let Err(e) = events.insert(&ev).await {
                        tracing::warn!("mqtt_plaintext_enabled 事件落库失败: {e}");
                    }
                }
                // 证书到期（TLS-3）：启动期先查一次（每日重复由发布任务负责）
                check_cert_expiry_once(&c, &events).await;

                let publisher = Arc::new(
                    match MqttUplinkPublisher::new(
                        latest,
                        points,
                        c.clone(),
                        roles,
                        dev,
                        cfg.north.clone(),
                        events,
                    ) {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!("MQTT 上送器装配失败: {e}");
                            out.detail = Some(format!("publisher: {e}"));
                            out.status = MqttServiceStatus::Failed;
                            return out;
                        }
                    },
                );
                // 事件循环（连接/重连的真值来源）
                out.tasks.push((
                    "mqtt_bridge_north",
                    tokio::spawn({
                        let c = c.clone();
                        async move {
                            let _ = c.run().await;
                        }
                    }),
                ));
                for (label, h) in publisher.spawn() {
                    out.tasks.push((label, h));
                }
                tracing::info!(
                    broker = %north_cfg.broker_addr,
                    client_id = %north_cfg.client_id,
                    "北向 MQTT 上送器已装配（A/B 定时 + C 档 COS/健康/补送）"
                );
            }
            Err(e) => {
                // **fail-closed**：证书读不到 / broker 空 ⇒ 记 error + 服务 Failed，**不重试明文**
                tracing::error!("北向 MQTT 客户端初始化失败（不回落明文）: {e}");
                out.detail = Some(format!("north: {e}"));
                out.status = MqttServiceStatus::Failed;
                return out;
            }
        }
    }

    out.status = MqttServiceStatus::Running;
    out
}

/// 证书到期检查（§9.3.5 TLS-3）：剩余 < 30 天 ⇒ 一条 `major` 事件（`mqtt_cert_expiring`）。
///
/// **解析失败 ⇒ WARN + 继续**（明文或不可解析都不阻断连接）；**每次都查**（启动 + 每日）。
pub(crate) async fn check_cert_expiry_once(
    client: &NorthMqttClient,
    events: &Arc<dyn EventRepository>,
) {
    let now = chrono::Utc::now().timestamp();
    match client.cert_expiry(now) {
        Ok(None) => {} // 明文模式：无证书，不适用
        Ok(Some(exp)) => {
            if exp.should_warn() {
                let msg = format!(
                    "北向 MQTT 客户端证书将于 {} 到期（剩余 {} 天，notAfter={}）——请提前更换",
                    exp.not_after_unix, exp.remaining_days, exp.not_after_unix
                );
                tracing::error!(remaining_days = exp.remaining_days, "{msg}");
                let ev = SystemEvent {
                    id: None,
                    timestamp: chrono::Utc::now(),
                    event_type: "mqtt_cert_expiring".to_string(),
                    source: "mqtt_bridge".to_string(),
                    message: msg,
                };
                if let Err(e) = events.insert(&ev).await {
                    tracing::warn!("mqtt_cert_expiring 事件落库失败: {e}");
                }
            }
        }
        Err(e) => tracing::warn!("客户端证书到期解析失败（不阻断连接，TLS-3）: {e}"),
    }
}

// ───────────────────────────── 发布配置与统计 ─────────────────────────────

/// 生产者侧发布配置（由 §9.3.2 的 `north` 段派生）。
#[derive(Debug, Clone)]
pub struct MqttPublishCfg {
    /// 主题前缀（缺省 `mupc/north`）；与 §9.3.4 的 `north_telemetry()` 同源拼接。
    pub topic_prefix: String,
    /// 缺省 QoS（0..=2）。
    pub qos: u8,
    /// A 档周期。
    pub a_interval: Duration,
    /// B 档周期。
    pub b_interval: Duration,
    /// COS 合并窗（ms）。
    pub cos_merge_ms: u64,
    /// 离线缓存时间窗（s）。
    pub cache_max_age_s: u64,
    /// 离线缓存条数上限。
    pub cache_max_messages: usize,
}

impl From<&crate::core_config::NorthCfg> for MqttPublishCfg {
    fn from(c: &crate::core_config::NorthCfg) -> Self {
        Self {
            topic_prefix: c.topic_prefix.clone(),
            qos: c.qos,
            a_interval: Duration::from_millis(c.periods.a_ms),
            b_interval: Duration::from_millis(c.periods.b_ms),
            cos_merge_ms: c.periods.cos_merge_ms,
            cache_max_age_s: c.cache.max_age_s,
            cache_max_messages: c.cache.max_messages,
        }
    }
}

/// 上送统计快照（§9.3.6 / BF-6：**不得静默丢弃**）。
///
/// `#[allow(dead_code)]` 是**如实登记**而非掩盖：§9.3.3 留证③ 明写该计数"供后续管理面/屏
/// 显示"——本轮**没有**生产消费方（只被单测当尺子），故按 `alert_feed.rs` 同一范式登记。
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MqttUplinkStatsSnapshot {
    /// 累计成功发布条数（消息数，不是点数）。
    pub published_total: u64,
    /// 累计发布失败次数。
    pub failed_total: u64,
    /// 当前离线缓存条数。
    pub cached_len: u64,
    /// 累计因上限淘汰的条数。
    pub dropped_total: u64,
    /// 最近一次错误文案（**不含凭据**）。
    pub last_error: Option<String>,
}

/// 统计累加器（`Arc` 共享，原子量 ⇒ 不阻塞发布路径）。
#[derive(Debug, Default)]
struct MqttUplinkCounters {
    published_total: AtomicU64,
    failed_total: AtomicU64,
    dropped_total: AtomicU64,
    /// 连续失败计数（§9.3.6：≥10 ⇒ 一条 major 事件）
    consecutive_failures: AtomicU64,
    /// 上一次失败告警时刻（ms；限频 5 min）
    last_failure_alert_ms: AtomicU64,
    /// 缓存淘汰 WARN 的上一次发射时刻（ms；按 1 min 聚合）
    last_cache_warn_ms: AtomicU64,
}

// ───────────────────────────── 离线缓存（§9.3.3） ─────────────────────────────

/// 待补送的一条消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pending {
    pub topic: String,
    pub payload: Vec<u8>,
    pub qos: u8,
    /// 该站的轮次序号（补送顺序判据；§9.3.3 "按 seq 单调顺序"）。
    pub seq: u64,
    /// **采集时刻**（原样保留，重连补送不得改写，§9.3.3 BF-2）。
    pub ts_ms: u64,
}

/// 内存离线缓存（**不落盘**，PRD Q7 建议①；§9.3.3）。
///
/// 上限 = `max_age_s`（按 `ts_ms`）**与** `max_messages` **先到先淘汰**，
/// 丢**最旧**；每次淘汰累计 `dropped` 并记录 `[first_ts, last_ts]` 范围供留证。
#[derive(Debug, Default)]
pub(crate) struct OfflineCache {
    q: VecDeque<Pending>,
    /// 累计淘汰条数（对外暴露 `dropped_total`）。
    dropped: u64,
    /// **未上报**的淘汰条数（跨 `push` 累计；`take_drop_evidence` 取走即清零）。
    ///
    /// 为什么需要它：留证**按 1 min 聚合、不风暴**（§9.3.3），而淘汰是逐条发生的
    /// ⇒ 被限频压掉的那几次必须**继续累计**，下一窗一次性上报，否则条数静默丢失（BF-6）。
    pending_dropped: u64,
    /// **未上报**淘汰的采集时刻范围 `[first_ts, last_ts]`（跨 `push` 累计，
    /// 取走即清零）：留证必须覆盖**本窗内全部**被丢条目（BF-4 / EX-9）。
    ///
    /// ⚠️ 不得写成"每次 `push` 覆盖"——那会让范围只剩**最后一条**被丢条目的时刻，
    /// 丢掉本窗最早的那一条（`last_drop_range` 是**累计**量，不是"最近一次淘汰的量"）。
    last_drop_range: Option<(u64, u64)>,
}

impl OfflineCache {
    pub(crate) fn len(&self) -> usize {
        self.q.len()
    }

    /// 累计淘汰条数（**留证口径**：生产侧读 `stats().dropped_total`；本口供单测）。
    #[allow(dead_code)]
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }

    /// **未上报**淘汰的累计时间范围（留证：BF-4 / EX-9；生产侧经 `take_drop_evidence` 取走）。
    #[allow(dead_code)]
    pub(crate) fn last_drop_range(&self) -> Option<(u64, u64)> {
        self.last_drop_range
    }

    /// **取走**未上报的淘汰留证（条数 + 时间范围）并清零 —— 只在"允许上报"（1 min 聚合窗
    /// 到点）时调用；被限频压掉的轮次**不取走** ⇒ 继续累计，下一窗一并上报（不风暴、不静默）。
    pub(crate) fn take_drop_evidence(&mut self) -> (u64, Option<(u64, u64)>) {
        let dropped = std::mem::take(&mut self.pending_dropped);
        (dropped, self.last_drop_range.take())
    }

    /// 入缓存（**先淘汰后入队**：保证"上限是硬上界"而不是"入队后再超"）。
    ///
    /// 返回本拍的 `(淘汰条数, 淘汰时间范围)`（供调用方累计 `dropped_total`）；**留证**用的
    /// "未上报"计数与范围由本函数**跨拍累计**保存，经 [`Self::take_drop_evidence`] 在允许
    /// 上报时一次取走（§9.3.3：按 1 min 聚合、不风暴、也不静默）。
    pub(crate) fn push(
        &mut self,
        p: Pending,
        max_age_s: u64,
        max_messages: usize,
    ) -> (u64, Option<(u64, u64)>) {
        // ① 时间窗：`ts_ms` 早于 `now − max_age_s` 的条目一律不可补送（补送过期数据
        //    只会污染云端统计）⇒ 先按窗口清一次
        let now_ms = now_millis();
        let cutoff = now_ms.saturating_sub(max_age_s.saturating_mul(1000));
        let mut dropped_now = 0u64;
        let mut range: Option<(u64, u64)> = None;
        let note_drop = |ts: u64, range: &mut Option<(u64, u64)>, dropped: &mut u64| {
            *dropped += 1;
            *range = Some(match *range {
                None => (ts, ts),
                Some((lo, _hi)) => (lo.min(ts), ts),
            });
        };
        while let Some(front) = self.q.front() {
            if front.ts_ms < cutoff {
                let f = self.q.pop_front().expect("front 存在");
                note_drop(f.ts_ms, &mut range, &mut dropped_now);
            } else {
                break;
            }
        }
        // ② 条数上限：丢最旧
        self.q.push_back(p);
        while self.q.len() > max_messages {
            let f = self.q.pop_front().expect("队首存在");
            note_drop(f.ts_ms, &mut range, &mut dropped_now);
        }
        self.dropped += dropped_now;
        if let Some((lo, hi)) = range {
            // **累计**（不覆盖）：留证范围 = 本聚合窗内全部被丢条目的 `[最早, 最晚]`
            self.pending_dropped += dropped_now;
            self.last_drop_range = Some(match self.last_drop_range {
                None => (lo, hi),
                Some((prev_lo, prev_hi)) => (prev_lo.min(lo), prev_hi.max(hi)),
            });
        }
        (dropped_now, range)
    }

    /// 取出一条待补送（FIFO ⇒ **全局 seq 单调**由入队顺序保证：入队即发布尝试顺序）。
    pub(crate) fn pop_front(&mut self) -> Option<Pending> {
        self.q.pop_front()
    }

    /// 补送失败 ⇒ **回填到队首**（保持顺序，§9.3.3）。
    pub(crate) fn push_front(&mut self, p: Pending) {
        self.q.push_front(p);
    }
}

// ───────────────────────────── 点表分站索引 ─────────────────────────────

/// 单站的发布计划（装配期一次算好：站 → 档 → 点下标）。
///
/// **为什么预计算**：① 避免每拍重扫 639 点；② 让"全量一轮的点集并集"成为**可断言**的
/// 结构量（AC-U74-02 的 624/552 机械判据）。
#[derive(Debug, Clone)]
pub struct StationPlan {
    /// 站 id（= 主题分片与载荷 `station`）。
    pub id: String,
    /// 角色字符串（载荷 `role`，§9.3.4）。
    pub role: String,
    /// A/B/C 档的 `points` 下标。
    pub a: Vec<usize>,
    pub b: Vec<usize>,
    pub c: Vec<usize>,
}

impl StationPlan {
    /// 本站在某档的点数（**只被用例当尺子**：逐档点数断言，§9.3.3 / §9.5）。
    #[allow(dead_code)]
    pub fn len_of(&self, class: SouthDataClass) -> usize {
        match class {
            SouthDataClass::A => self.a.len(),
            SouthDataClass::B => self.b.len(),
            SouthDataClass::C => self.c.len(),
        }
    }
}

/// 站号 → 角色字符串（YAML 的 `role`，snake_case；与 `Role` 的 serde 名逐字一致）。
///
/// 取 `mupc_southd::config::Role` 的 serde 表示作**唯一真源**（不手写第二份映射表）：
/// `serde_json::to_value(Role::Battery)` = `"battery"`。
pub(crate) fn station_roles(
    cfg: &mupc_southd::config::SouthStationsConfig,
) -> HashMap<String, String> {
    cfg.stations
        .iter()
        .map(|s| {
            let role = serde_json::to_value(s.role)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            (s.id.clone(), role)
        })
        .collect()
}

/// 按站/档建立发布计划（**只取 `channels.has(MQTT)`**，§9.2.1.0 / C-16）。
pub fn plan_stations(points: &[UplinkPoint], roles: &HashMap<String, String>) -> Vec<StationPlan> {
    let mut plan: HashMap<String, StationPlan> = HashMap::new();
    for (i, p) in points.iter().enumerate() {
        if !p.channels.has(ChannelMask::MQTT) {
            continue;
        }
        let e = plan
            .entry(p.station.clone())
            .or_insert_with(|| StationPlan {
                id: p.station.clone(),
                role: roles.get(&p.station).cloned().unwrap_or_default(),
                a: Vec::new(),
                b: Vec::new(),
                c: Vec::new(),
            });
        match p.class {
            SouthDataClass::A => e.a.push(i),
            SouthDataClass::B => e.b.push(i),
            SouthDataClass::C => e.c.push(i),
        }
    }
    let mut v: Vec<StationPlan> = plan.into_values().collect();
    // 确定性顺序（HashMap 迭代序不定；发布顺序与 seq 编号须稳定可复现）
    v.sort_by(|x, y| x.id.cmp(&y.id));
    v
}

/// 全量一轮（A+B+C）的 MQTT 点数（AC-U74-02 判据：== 624 / 552）。
pub fn mqtt_point_count(plan: &[StationPlan]) -> usize {
    plan.iter().map(|s| s.a.len() + s.b.len() + s.c.len()).sum()
}

// ───────────────────────────── 载荷（§9.3.4） ─────────────────────────────

/// 单位解析（§9.3.4 的 `u`）。
///
/// **口径**：位点恒为 `"bool"`；标量点取点表 `label` 的**尾 token 白名单**（如
/// `"簇累计充电电量 kWh"` ⇒ `kWh`）。**未识别 ⇒ 空串**（如实表达"点表未登记单位"，
/// **不臆造**）。登记为已知边界：02 PRD §9.7.5 是**设备级**单位表、无点级机读表，
/// 故机械来源只能是 label 尾 token；观测量级的覆盖率见单测。
pub fn unit_of(kind: UplinkKind, label: &str) -> &'static str {
    if matches!(kind, UplinkKind::Bit) {
        return "bool";
    }
    unit_from_label(label).unwrap_or("")
}

/// 已知单位白名单（**02 PRD §9.7.5 出现的全部单位**；大小写敏感）。
const KNOWN_UNITS: &[&str] = &[
    "V", "A", "kW", "kvar", "kVA", "kWh", "kvarh", "Ah", "Hz", "%", "℃", "kPa", "ppm", "dB/M",
];

/// 取 label 的尾 token 并在白名单内匹配。
fn unit_from_label(label: &str) -> Option<&'static str> {
    let tail = label.split_whitespace().last()?;
    KNOWN_UNITS.iter().copied().find(|u| *u == tail)
}

/// grid 6 点（§9.7 C-17 ② 的派生名）——**label 不含单位** ⇒ 显式单位表
/// （依据 02 PRD §9.7.5：ADL400/meter_grid 电压 V、电流 A、功率 kW/kvar、PF 无量纲、频率 Hz）。
fn grid_unit(metric: &str) -> Option<&'static str> {
    match metric {
        "active_power" => Some("kW"),
        "reactive_power" => Some("kvar"),
        "voltage" => Some("V"),
        "current" => Some("A"),
        // PF 无量纲：按 02 PRD §9.7.5 的分辨率口径（0.001）不带单位 ⇒ 空串，不臆造 "1"
        "cos_phi" => Some(""),
        "frequency" => Some("Hz"),
        _ => None,
    }
}

/// 载荷点位（字段名/顺序 = §9.3.4 逐字）。
#[derive(Serialize)]
struct PayloadPoint {
    /// 点名（**南向点名逐字**，LV-5）
    n: String,
    /// 工程值；不可得 ⇒ `null`（**严禁 0 顶替**）
    v: Option<f64>,
    /// 工程单位（`bool` 用于位点）
    u: String,
    /// `ok` / `stale` / `invalid` / `unconfigured`
    q: String,
}

/// 遥测载荷（字段名/顺序 = §9.3.4 逐字：`ts`/`dev`/`station`/`role`/`seq`/`points`）。
#[derive(Serialize)]
struct TelemetryPayload {
    /// **采集时刻** UTC ISO-8601 毫秒字符串（`YYYY-MM-DDThh:mm:ss.sssZ`）
    ts: String,
    /// 装置标识；来源未定（PRD Q10）⇒ 无值写 `null`，**不臆造**
    dev: Option<String>,
    station: String,
    role: String,
    /// 单调递增轮次（每站独立）
    seq: u64,
    points: Vec<PayloadPoint>,
}

/// 事件载荷（站 offline/online；§9.3.4 的 `event/{station}`）。
#[derive(Serialize)]
struct EventPayload {
    ts: String,
    dev: Option<String>,
    station: String,
    role: String,
    seq: u64,
    /// `"offline"` / `"online"`
    event: String,
}

/// UTC 毫秒 → ISO-8601 毫秒字符串（§9.3.4：线序 = 字符串，非整数 epoch；Q-E 默认 (a)）。
pub fn iso8601_ms(ts_ms: u64) -> String {
    match chrono::DateTime::from_timestamp_millis(ts_ms.min(i64::MAX as u64) as i64) {
        Some(dt) => dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        // 极端越界值（时钟异常）⇒ 不 panic，如实降级为 epoch 0 的字符串
        None => "1970-01-01T00:00:00.000Z".to_string(),
    }
}

/// `PointQuality` → 载荷 `q`（**一一对应**，§9.1.2 → §9.3.4）。
pub fn quality_str(q: PointQuality) -> &'static str {
    match q {
        PointQuality::Ok => "ok",
        PointQuality::Stale => "stale",
        PointQuality::Invalid => "invalid",
        PointQuality::Unconfigured => "unconfigured",
    }
}

// ───────────────────────────── 发布器（§9.3.3） ─────────────────────────────

/// 北向上送发布器（§9.3.3）：**订阅内存快照变更批（禁轮询 DB）** + A/B 周期 + 离线缓存。
pub struct MqttUplinkPublisher {
    latest: Arc<LatestValues>,
    points: Arc<Vec<UplinkPoint>>,
    client: Arc<NorthMqttClient>,
    /// 站 → 角色串（载荷 `role`）。
    plan: Arc<Vec<StationPlan>>,
    /// 装置标识（§9.3.4：来源未定 ⇒ `None` 时载荷写 `null`）。
    dev: Option<String>,
    cfg: MqttPublishCfg,
    cache: Arc<Mutex<OfflineCache>>,
    /// 每站独立的轮次序号（§9.3.4：`seq` 每站单调递增）。
    seq: Arc<Mutex<HashMap<String, u64>>>,
    counters: Arc<MqttUplinkCounters>,
    events: Arc<dyn EventRepository>,
    /// 补送互斥（防重连抖动叠加多条补送任务，§9.3.3 BF-3）。
    draining: Arc<AtomicBool>,
    /// 最近一次错误文案（**只存错误字面**，绝不含凭据；§9.3.5）。
    last_error_buf: Arc<Mutex<Option<String>>>,
}

impl MqttUplinkPublisher {
    /// 构造。**无 I/O**（客户端由调用方传入 ⇒ 建连时机的唯一决定点是装配层）。
    ///
    /// 装配期**点数断言**（AC-U74-02 的机械判据）：发布计划的全量并集必须等于点表中
    /// `channels.has(MQTT)` 的条数，且 ∈ {552, 624}（PCS 未启用/启用）。不等 ⇒ `Err`
    /// （装配层记 error 并注册 `Failed`，不静默带病运行）。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        latest: Arc<LatestValues>,
        points: Arc<Vec<UplinkPoint>>,
        client: Arc<NorthMqttClient>,
        roles: Arc<HashMap<String, String>>,
        dev: Option<String>,
        north_cfg: crate::core_config::NorthCfg,
        events: Arc<dyn EventRepository>,
    ) -> Result<Self, String> {
        let plan = plan_stations(&points, &roles);
        let planned = mqtt_point_count(&plan);
        let actual = points
            .iter()
            .filter(|p| p.channels.has(ChannelMask::MQTT))
            .count();
        if planned != actual {
            return Err(format!(
                "MQTT 发布计划漏点/重点：计划 {planned} 条，点表 MQTT 子集 {actual} 条（§9.3.3 点数断言）"
            ));
        }
        // PCS 是否启用由**点表自身**判定（存在 pcs 站的 MQTT 点即启用），不依赖配置文件之外的假设
        let has_pcs = points
            .iter()
            .any(|p| p.station == "pcs" && p.channels.has(ChannelMask::MQTT));
        let expected = if has_pcs {
            MQTT_POINTS_WITH_PCS
        } else {
            MQTT_POINTS_WITHOUT_PCS
        };
        if planned != expected {
            return Err(format!(
                "MQTT 全量点数 = {planned}，期望 {expected}（PCS {}）—— AC-U74-02 判据",
                if has_pcs { "已启用" } else { "未启用" }
            ));
        }
        let cfg = MqttPublishCfg::from(&north_cfg);
        Ok(Self {
            latest,
            points,
            client,
            plan: Arc::new(plan),
            dev,
            cfg,
            cache: Arc::new(Mutex::new(OfflineCache::default())),
            seq: Arc::new(Mutex::new(HashMap::new())),
            counters: Arc::new(MqttUplinkCounters::default()),
            events,
            draining: Arc::new(AtomicBool::new(false)),
            last_error_buf: Arc::new(Mutex::new(None)),
        })
    }

    /// 统计快照（§9.3.6：BF-6 **不得静默丢弃**）。
    ///
    /// 无生产消费方（§9.3.3 留证③："供后续管理面/屏显示"）⇒ `#[allow(dead_code)]` 如实登记。
    #[allow(dead_code)]
    pub fn stats(&self) -> MqttUplinkStatsSnapshot {
        MqttUplinkStatsSnapshot {
            published_total: self.counters.published_total.load(Ordering::Relaxed),
            failed_total: self.counters.failed_total.load(Ordering::Relaxed),
            cached_len: self.cache.lock().map(|c| c.len() as u64).unwrap_or(0),
            dropped_total: self.counters.dropped_total.load(Ordering::Relaxed),
            last_error: self.last_error(),
        }
    }

    #[allow(dead_code)] // 只被 `stats()`（同为无生产消费方的留证口）取用
    fn last_error(&self) -> Option<String> {
        self.last_error_buf
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 是否已与 broker 握手（复用 `NorthMqttClient::is_connected`，§9.3.6）。
    pub fn is_connected(&self) -> bool {
        use device_trait::MqttBridge;
        self.client.is_connected()
    }

    /// 发布计划（**只被用例当尺子**：断言 A/B/C 档点数与 624/552，§9.3.3）。
    #[allow(dead_code)]
    pub fn plan(&self) -> &[StationPlan] {
        &self.plan
    }

    /// 起 3 条任务（§9.3.3：A 档、B 档、C 档 + 合并窗 + 连接边沿 + 健康；§9.3.7 MQTT 新增任务数）。
    pub fn spawn(self: &Arc<Self>) -> Vec<(&'static str, tokio::task::JoinHandle<()>)> {
        vec![
            (
                "mqtt_uplink_a",
                self.spawn_class_periodic(SouthDataClass::A, self.cfg.a_interval),
            ),
            (
                "mqtt_uplink_b",
                self.spawn_class_periodic(SouthDataClass::B, self.cfg.b_interval),
            ),
            ("mqtt_uplink_cos", self.spawn_cos_and_health()),
        ]
    }

    /// A/B 档：单拍定时任务（逐站一条消息；**站间不合并**，§9.3.4 分片强制）。
    fn spawn_class_periodic(
        self: &Arc<Self>,
        class: SouthDataClass,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                me.publish_round(class, None).await;
            }
        })
    }

    /// C 档 + 连接边沿 + 健康（一条任务、`cos_merge_ms` 节拍）：
    ///
    /// - **合并窗**：`cos_merge_ms` 内的变更点合并为一批（§9.3.3 / COS ≤1 s 硬约束）；
    /// - **连接边沿**：`false → true` ⇒ ① C 档立即全量一次 ② 解除离线缓存并补送；
    /// - **健康**：站 offline/online 边沿 ⇒ `event/{station}`（QoS2）；证书到期每日复查。
    fn spawn_cos_and_health(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            let mut rx = me.latest.subscribe();
            let mut pending: HashSet<(String, String)> = HashSet::new();
            let mut ticker =
                tokio::time::interval(Duration::from_millis(me.cfg.cos_merge_ms.max(1)));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut was_connected = false;
            let mut station_state: HashMap<String, bool> = HashMap::new();
            let mut last_cert_check_ms = 0u64;
            loop {
                tokio::select! {
                    r = rx.recv() => match r {
                        Ok(batch) => {
                            // §9.1.5：变更批天然合并；同一窗内的多变位再合并一次
                            for id in batch.changed {
                                pending.insert((id.station, id.metric));
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            // 落后丢帧 ⇒ 全量重读重建基线（§9.1.5 消费方契约）
                            tracing::debug!(lagged = n, "MQTT C 档订阅落后 ⇒ 全量重读重发");
                            for p in me.points.iter() {
                                if p.channels.has(ChannelMask::MQTT)
                                    && p.class == SouthDataClass::C
                                {
                                    pending.insert((p.station.clone(), p.metric.clone()));
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    },
                    _ = ticker.tick() => {
                        // ① 连接边沿（含启动首拍：was_connected=false ⇒ 若已连上则补首轮）
                        let connected = me.is_connected();
                        if connected && !was_connected {
                            tracing::info!("MQTT 北向已连接 ⇒ C 档首轮全量 + 解缓存补送（§9.3.3）");
                            me.publish_round(SouthDataClass::C, None).await;
                            me.spawn_cache_drain();
                        }
                        was_connected = connected;

                        // ② 合并窗到点 ⇒ 发本窗变更（按站分条）
                        if !pending.is_empty() {
                            // 取走本窗变更集（等价于 drain + collect，`pending` 复用为下一窗的容器）
                            let changed: HashSet<(String, String)> = std::mem::take(&mut pending);
                            me.publish_round(SouthDataClass::C, Some(&changed)).await;
                        }

                        // ③ 站 offline/online 边沿 ⇒ event/{station}（QoS2）
                        me.publish_station_edges(&mut station_state).await;

                        // ④ 证书到期：启动 + 每日一次（TLS-3）
                        let now = now_millis();
                        if now.saturating_sub(last_cert_check_ms)
                            >= CERT_CHECK_INTERVAL.as_millis() as u64
                            || last_cert_check_ms == 0
                        {
                            last_cert_check_ms = now;
                            check_cert_expiry_once(&me.client, &me.events).await;
                        }
                    }
                }
            }
        })
    }

    /// 一轮发布：逐站组一条消息（**站间不合并**）。
    ///
    /// `only_changed = Some(..)` ⇒ 只发变更点（C 档 COS）；`None` ⇒ 发本档**全部**点
    /// （A/B 档周期、C 档首轮全量）。
    ///
    /// **点数口径（与 §9.3.7 的字节表一致）**：消息点集 = 本站本档的**点表全量**
    /// （`channels.has(MQTT)`，与"当前有无值"无关）⇒ 全量一轮的并集恒为 624/552
    /// （AC-U74-02）。**有值性由 `q`/`v` 表达**（§9.3.4：`q != ok` 保留原值、不可得写
    /// `null`；§9.5 AC-U74-05 要求失效点**仍出现在载荷里**且 `q=invalid`，故不得按
    /// "quality == Ok" 丢点——那会让"站失败"与"该站无此点"在云端不可区分）。
    ///
    /// **空批不发**：本站本档**全部点都从未采集**（`Unconfigured`）⇒ 不发（未投产的站
    /// 不产生空转报文）。
    async fn publish_round(
        &self,
        class: SouthDataClass,
        only_changed: Option<&HashSet<(String, String)>>,
    ) {
        let now_ms = now_millis();
        for st in self.plan.iter() {
            let idx = match class {
                SouthDataClass::A => &st.a,
                SouthDataClass::B => &st.b,
                SouthDataClass::C => &st.c,
            };
            if idx.is_empty() {
                continue;
            }
            let selected: Vec<usize> = match only_changed {
                None => idx.clone(),
                Some(changed) => idx
                    .iter()
                    .copied()
                    .filter(|i| {
                        let p = &self.points[*i];
                        changed.contains(&(p.station.clone(), p.metric.clone()))
                    })
                    .collect(),
            };
            if selected.is_empty() {
                continue;
            }
            let msg = self.build_message(st, &selected, now_ms);
            let Some((topic, payload, ts_ms, seq)) = msg else {
                continue;
            };
            self.publish_or_cache(topic, payload, ts_ms, seq).await;
        }
    }

    /// 组装一条站内消息；**全点 `Unconfigured` ⇒ `None`**（不发空转报文）。
    ///
    /// 返回 `(topic, payload, ts_ms, seq)`。
    fn build_message(
        &self,
        st: &StationPlan,
        selected: &[usize],
        now_ms: u64,
    ) -> Option<(String, Vec<u8>, u64, u64)> {
        // 单次站内读（一次读锁；避免逐点 `get` 反复取锁，§9.1.6）
        let views: HashMap<String, mupc_data_processing::latest_values::PointView> = self
            .latest
            .station_snapshot(&st.id)
            .into_iter()
            .map(|pv| (pv.id.metric.clone(), pv))
            .collect();

        let mut pts: Vec<PayloadPoint> = Vec::with_capacity(selected.len());
        let mut ts_max = 0u64;
        let mut any_configured = false;
        for i in selected {
            let p = &self.points[*i];
            let pv = views.get(&p.metric);
            let (value, q, ts_ms) = match pv {
                None => (None, PointQuality::Unconfigured, 0u64),
                Some(pv) => {
                    let is_bit = matches!(p.kind, UplinkKind::Bit);
                    // 新鲜度门控（§9.1.2）：质量 Ok 但已过期 ⇒ `stale`（判据唯一真源在快照）
                    let q = if pv.value.quality == PointQuality::Ok
                        && !self.latest.is_fresh(&pv.id, &pv.value, is_bit, now_ms)
                    {
                        PointQuality::Stale
                    } else {
                        pv.value.quality
                    };
                    (pv.value.value, q, pv.value.ts_ms)
                }
            };
            if q != PointQuality::Unconfigured {
                any_configured = true;
            }
            ts_max = ts_max.max(ts_ms);
            // grid 6 点用**显式单位表**（它们的 label 是"总有功功率（来源 …）"形态、无尾 token）；
            // 其余站取点表 label 的尾 token（未识别 ⇒ 空串，不臆造）
            let unit = if p.station == "grid_meter" {
                grid_unit(&p.metric).unwrap_or_else(|| unit_of(p.kind, p.label))
            } else {
                unit_of(p.kind, p.label)
            };
            pts.push(PayloadPoint {
                n: p.metric.clone(),
                v: value,
                u: unit.to_string(),
                q: quality_str(q).to_string(),
            });
        }
        if !any_configured {
            // 本站本档所有点从未采集 ⇒ 不产生报文（**不是**丢点：没有采集事实）
            return None;
        }
        let seq = {
            let mut g = self.seq.lock().unwrap_or_else(|e| e.into_inner());
            let e = g.entry(st.id.clone()).or_insert(0);
            *e += 1;
            *e
        };
        let payload = TelemetryPayload {
            // 消息级 `ts` = 批内**最大采集时刻**（同一轮的点共用一个时标，§9.3.4）
            ts: iso8601_ms(if ts_max == 0 { now_ms } else { ts_max }),
            dev: self.dev.clone(),
            station: st.id.clone(),
            role: st.role.clone(),
            seq,
            points: pts,
        };
        let body = serde_json::to_vec(&payload).ok()?;
        Some((
            format!("{}/telemetry/{}", self.cfg.topic_prefix, st.id),
            body,
            ts_max,
            seq,
        ))
    }

    /// 发布（QoS1）；**未连接或发布失败 ⇒ 入离线缓存**（§9.3.3）。
    async fn publish_or_cache(&self, topic: String, payload: Vec<u8>, ts_ms: u64, seq: u64) {
        if self.is_connected() {
            match self.publish_raw(&topic, &payload, self.cfg.qos).await {
                Ok(()) => {
                    self.counters
                        .published_total
                        .fetch_add(1, Ordering::Relaxed);
                    self.counters
                        .consecutive_failures
                        .store(0, Ordering::Relaxed);
                    return;
                }
                Err(e) => {
                    self.note_failure(&e).await;
                }
            }
        }
        let p = Pending {
            topic,
            payload,
            qos: self.cfg.qos,
            seq,
            ts_ms,
        };
        let (dropped, _range) = {
            let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            c.push(p, self.cfg.cache_max_age_s, self.cfg.cache_max_messages)
        };
        if dropped > 0 {
            self.counters
                .dropped_total
                .fetch_add(dropped, Ordering::Relaxed);
            // 留证取"本聚合窗累计"的条数与范围（不是本拍增量）⇒ 限频压掉的轮次不丢证据
            self.warn_cache_drop().await;
        }
    }

    /// 发布失败记账（§9.3.6：**不得静默**；连续 ≥10 ⇒ 一条 major 事件 + 5 min 限频）。
    async fn note_failure(&self, err: &str) {
        let total = self.counters.failed_total.fetch_add(1, Ordering::Relaxed) + 1;
        let consecutive = self
            .counters
            .consecutive_failures
            .fetch_add(1, Ordering::Relaxed)
            + 1;
        self.set_last_error(err);
        tracing::warn!(failed_total = total, consecutive, "MQTT 发布失败: {err}");
        if consecutive < MQTT_FAILURE_ALERT_THRESHOLD {
            return;
        }
        let now = now_millis();
        let last = self.counters.last_failure_alert_ms.load(Ordering::Relaxed);
        if last != 0 && now.saturating_sub(last) < MQTT_FAILURE_ALERT_MIN_INTERVAL_MS {
            return;
        }
        self.counters
            .last_failure_alert_ms
            .store(now, Ordering::Relaxed);
        let ev = SystemEvent {
            id: None,
            timestamp: chrono::Utc::now(),
            event_type: "mqtt_publish_failing".to_string(),
            source: "mqtt_bridge".to_string(),
            message: format!(
                "北向 MQTT 连续发布失败 {consecutive} 次（累计 {total}）；最近错误: {err}"
            ),
        };
        if let Err(e) = self.events.insert(&ev).await {
            tracing::warn!("mqtt_publish_failing 事件落库失败: {e}");
        }
    }

    /// 缓存淘汰留证（§9.3.3：① WARN 日志**按 1 min 聚合**不风暴；② 一条 `major` 事件）。
    ///
    /// **聚合口径**：本窗内（限频间隔）被压掉的轮次**继续累计**在 `OfflineCache` 里
    /// （`take_drop_evidence`），到点一次性上报 ⇒ 文案里的条数与时间范围是**本窗累计**，
    /// 不是"最后一拍"。上限（`cache.max_messages`）一并写出：现场据此判断是条数先到还是
    /// 时间窗先到（§9.3.7 的缓存上界算法）。
    async fn warn_cache_drop(&self) {
        let now = now_millis();
        let last = self.counters.last_cache_warn_ms.load(Ordering::Relaxed);
        if last != 0 && now.saturating_sub(last) < MQTT_CACHE_WARN_INTERVAL_MS {
            // 限频（不风暴）：**不取走**证据 ⇒ 本轮淘汰留待下一窗一并上报（BF-6 不静默）
            return;
        }
        self.counters
            .last_cache_warn_ms
            .store(now, Ordering::Relaxed);
        let cap = self.cfg.cache_max_messages;
        let (dropped, range, cached_len) = {
            let mut c = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            let (dropped, range) = c.take_drop_evidence();
            (dropped, range, c.len() as u64)
        };
        let rng = range
            .map(|(lo, hi)| format!("{} ~ {}", iso8601_ms(lo), iso8601_ms(hi)))
            .unwrap_or_else(|| "未知".to_string());
        tracing::warn!(
            dropped,
            cap,
            ts_range = %rng,
            cached_len,
            "MQTT 离线缓存超上限 ⇒ 丢弃最旧（BF-4：不得静默）"
        );
        let ev = SystemEvent {
            id: None,
            timestamp: chrono::Utc::now(),
            event_type: "mqtt_cache_overflow".to_string(),
            source: "mqtt_bridge".to_string(),
            message: format!(
                "北向 MQTT 离线缓存溢出：上限 {cap} 条已满，本窗丢弃最旧 {dropped} 条\
                 （时间范围 {rng}）"
            ),
        };
        if let Err(e) = self.events.insert(&ev).await {
            tracing::warn!("mqtt_cache_overflow 事件落库失败: {e}");
        }
    }

    fn set_last_error(&self, err: &str) {
        let mut g = self
            .last_error_buf
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *g = Some(err.to_string());
    }

    /// 底层发布（QoS 0/1/2；`device_trait::MqttBridge::publish`）。
    async fn publish_raw(&self, topic: &str, payload: &[u8], qos: u8) -> Result<(), String> {
        use device_trait::MqttBridge;
        self.client
            .publish(topic, payload, qos)
            .await
            .map_err(|e| e.to_string())
    }

    /// 补送（§9.3.3 BF-2/BF-3）：独立任务、按入队顺序逐条补送，失败**回填队首**。
    ///
    /// **与 A/B 实时发布并发**：A/B 是独立任务，不被本任务阻塞（BF-3）；本任务自身
    /// 与"连接边沿"解耦（`draining` 互斥防叠加）。
    fn spawn_cache_drain(self: &Arc<Self>) {
        if self
            .draining
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return; // 已有补送在跑
        }
        let me = self.clone();
        tokio::spawn(async move {
            let mut sent = 0usize;
            loop {
                if !me.is_connected() {
                    break;
                }
                let next = {
                    let mut c = me.cache.lock().unwrap_or_else(|e| e.into_inner());
                    c.pop_front()
                };
                let Some(p) = next else { break };
                match me.publish_raw(&p.topic, &p.payload, p.qos).await {
                    Ok(()) => {
                        me.counters.published_total.fetch_add(1, Ordering::Relaxed);
                        me.counters.consecutive_failures.store(0, Ordering::Relaxed);
                        sent += 1;
                    }
                    Err(e) => {
                        // 失败 ⇒ 回填队首（保持 seq 顺序）并结束本轮（连接已不可用）
                        me.cache
                            .lock()
                            .unwrap_or_else(|err| err.into_inner())
                            .push_front(p);
                        me.note_failure(&e).await;
                        break;
                    }
                }
            }
            me.draining.store(false, Ordering::SeqCst);
            if sent > 0 {
                tracing::info!(
                    sent,
                    remaining = me.cache.lock().map(|c| c.len()).unwrap_or(0),
                    "MQTT 断线缓存补送完成"
                );
            }
        });
    }

    /// 站 offline/online 边沿 ⇒ `event/{station}`（QoS2，§9.3.4 的故障类事件）。
    ///
    /// **边沿语义**：初始为 `Unknown`（不产生事件）⇒ 只在"曾活跃 → 失活"或"失活 → 复活"
    /// 的**真实迁移**上发事件；从未采集过的站在云端不会被误报为 offline。
    async fn publish_station_edges(&self, state: &mut HashMap<String, bool>) {
        let now = now_millis();
        for st in self.plan.iter() {
            let active = self.latest.station_is_active(&st.id, now);
            match state.get(&st.id) {
                // 首次观测：登记现状、不发事件（避免启动瞬间的假 offline 风暴）
                None => {
                    state.insert(st.id.clone(), active);
                }
                Some(prev) if *prev == active => {}
                Some(_) => {
                    let event = if active { "online" } else { "offline" };
                    tracing::warn!(station = %st.id, event, "MQTT 站级状态迁移事件");
                    state.insert(st.id.clone(), active);
                    let seq = {
                        let mut g = self.seq.lock().unwrap_or_else(|e| e.into_inner());
                        let e = g.entry(st.id.clone()).or_insert(0);
                        *e += 1;
                        *e
                    };
                    let payload = EventPayload {
                        ts: iso8601_ms(now),
                        dev: self.dev.clone(),
                        station: st.id.clone(),
                        role: st.role.clone(),
                        seq,
                        event: event.to_string(),
                    };
                    let body = match serde_json::to_vec(&payload) {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::warn!("事件载荷序列化失败: {e}");
                            continue;
                        }
                    };
                    let topic = format!("{}/event/{}", self.cfg.topic_prefix, st.id);
                    // 故障类事件走 QoS2（§9.3.4 表）
                    self.publish_or_cache(topic, body, now, seq).await;
                }
            }
        }
    }
}

// ───────────────────────────── 单测（01 设计 §9.5「core-bin」行 + 任务交付物 3） ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_config::CoreConfig;
    use mupc_data_processing::latest_values::{PointId, PointQuality, PointValue};
    use mupc_southd::config::{SouthPcsConfig, SouthStationsConfig};
    use mupc_southd::uplink::build_uplink_points;

    /// 参考配置（含 PCS）——**复用 T11 的同一份 fixture**（不新建第二份点表真源）。
    /// **Task 6（ADR-016）起站级段为 5 站**（546 点），PCS 在独立顶层段 [`REF_PCS`]（72 点）。
    const REF_STATIONS: &str =
        include_str!("../../mupc-southd/tests/fixtures/south_stations_s3b2.yaml");

    /// PCS 独立顶层段（Task 6）。
    const REF_PCS: &str = include_str!("../../mupc-southd/tests/fixtures/south_pcs_s3b2.yaml");

    #[derive(serde::Deserialize)]
    struct Wrapper {
        south_stations: SouthStationsConfig,
    }

    /// 参考配置 = 站级段 5 站 **+ 由 `south_pcs` 段合成的 `Role::Pcs` 站**
    /// （合成理由与 `mupc-southd::uplink::tests::cfg_ref` 同源：本组用例钉的是"**PCS 启用**"
    /// 的通道/档位口径，而 `build_uplink_points` 的入参形态仍是站表；§13.9 契约零变化）。
    ///
    /// **合成走共用函数** `SouthPcsConfig::station_shell`（设计 §13.9 末要求③：生产与测试
    /// 共用同一函数）—— 此前本处手写，与 `mupc-southd` 两处各一份、会与生产漂移。
    fn cfg() -> SouthStationsConfig {
        let mut stations: SouthStationsConfig = serde_yaml::from_str::<Wrapper>(REF_STATIONS)
            .expect("参考配置解析失败")
            .south_stations;
        let pcs: SouthPcsConfig =
            serde_yaml::from_str(REF_PCS).expect("south_pcs 参考配置解析失败");
        assert!(pcs.enabled, "参考 PCS 段须 enabled");
        stations.stations.push(pcs.station_shell());
        stations
    }

    fn points() -> Vec<UplinkPoint> {
        build_uplink_points(&cfg())
            .expect("build_uplink_points 必须成功（参考配置：站级 5 站 + 合成 PCS 段）")
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

    /// **GI-3 守卫的隔离用例（T13 独立评审 B1）**：`value: None` **且** `quality: Ok`
    /// **且**时标新鲜 ⇒ `is_fresh` 为真 ⇒ **只有** `value.is_some()` 这一道守卫能把它排除。
    ///
    /// 既有 `interrogation_excludes_stale_invalid_and_none` 的 None 点写成 `Invalid`
    /// （被 `is_fresh` 先滤掉）⇒ 该守卫**当时零覆盖**（评审实测：删掉守卫，8 条用例全绿）。
    /// 本用例把它单独钉住——否则将来出现 `None + Ok` 的写方会**静默上送 0.0** 且无网。
    #[test]
    fn gi3_none_value_with_ok_quality_is_excluded_by_guard() {
        let pts = points();
        let latest = LatestValues::new(5);
        let now = 1_000_000u64;
        let ap = id("grid_meter", "active_power");
        latest.apply(vec![(
            ap.clone(),
            PointValue {
                value: None,
                ts_ms: now,
                quality: PointQuality::Ok,
            },
        )]);

        // 前置断言（防假绿）：该点确实新鲜 ⇒ 唯一能排除它的就是 `value.is_some()` 守卫
        let pv = latest.get(&ap);
        assert!(
            latest.is_fresh(&pv.id, &pv.value, false, now),
            "前提：None+Ok+新鲜 ⇒ is_fresh 为真，故排除只能来自 value.is_some() 守卫"
        );

        let items = interrogation_items(&latest, &pts, now);
        assert!(
            items.iter().all(|i| i.ioa != 1),
            "GI-3：value=None 的点不得出现（即便质量 Ok、时标新鲜）——不得以 0 顶替"
        );
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

    // ═══════════════ MQTT 上送（01 设计 §9.3 / §9.4 序 7 / §9.5 core-bin 行） ═══════════════

    /// 记账事件仓储（与 `startup.rs` 用例同范式；只用到 `insert`）。
    #[derive(Default)]
    struct RecordingEvents(std::sync::Mutex<Vec<SystemEvent>>);

    #[async_trait::async_trait]
    impl mupc_storage::EventRepository for RecordingEvents {
        async fn insert(&self, event: &SystemEvent) -> Result<i64, mupc_storage::StorageError> {
            self.0.lock().unwrap().push(event.clone());
            Ok(1)
        }
        async fn query_range(
            &self,
            _s: chrono::DateTime<chrono::Utc>,
            _e: chrono::DateTime<chrono::Utc>,
        ) -> Result<Vec<SystemEvent>, mupc_storage::StorageError> {
            Ok(Vec::new())
        }
        async fn purge_older_than(
            &self,
            _b: chrono::DateTime<chrono::Utc>,
        ) -> Result<usize, mupc_storage::StorageError> {
            Ok(0)
        }
        async fn latest_by_type(
            &self,
            _t: &str,
        ) -> Result<Option<SystemEvent>, mupc_storage::StorageError> {
            Ok(None)
        }
    }

    fn roles_of(cfg: &SouthStationsConfig) -> Arc<HashMap<String, String>> {
        Arc::new(station_roles(cfg))
    }

    /// 北向配置助手（**显式给全字段**，不复用 `::default()` 兜底——C-11 的同一口径）。
    fn north_cfg_with(
        f: impl FnOnce(&mut crate::core_config::NorthCfg),
    ) -> crate::core_config::NorthCfg {
        let mut c = crate::core_config::NorthCfg {
            enabled: true,
            broker: "127.0.0.1:1".into(), // 无监听者：`is_connected()` 恒 false ⇒ 走缓存路径
            client_id: "mupc-test".into(),
            tls: crate::core_config::TlsCfg {
                allow_plaintext: true,
                ..Default::default()
            },
            ..Default::default()
        };
        f(&mut c);
        c
    }

    /// 明文测试客户端（不跑事件循环 ⇒ 永不 connected；仅提供 publish 通道）。
    fn test_client() -> Arc<NorthMqttClient> {
        let c = mupc_mqtt_bridge::NorthMqttConfig {
            enabled: true,
            broker_addr: "127.0.0.1:1".into(),
            client_id: "mupc-test".into(),
            allow_plaintext: true,
            ..Default::default()
        };
        Arc::new(NorthMqttClient::new(&c).expect("明文客户端可构造"))
    }

    fn test_publisher(
        latest: Arc<LatestValues>,
        pts: Arc<Vec<UplinkPoint>>,
        north: crate::core_config::NorthCfg,
        events: Arc<dyn EventRepository>,
    ) -> MqttUplinkPublisher {
        MqttUplinkPublisher::new(
            latest,
            pts,
            test_client(),
            roles_of(&cfg()),
            Some("MUPC-0001".to_string()),
            north,
            events,
        )
        .expect("上送器装配（点数断言通过）")
    }

    /// 静态字符串助手（把 `p.plan()[..]` 里某站某档的下标取出来）。
    fn indices_of(plan: &[StationPlan], station: &str, class: SouthDataClass) -> Vec<usize> {
        let st = plan.iter().find(|s| s.id == station).expect("站存在");
        match class {
            SouthDataClass::A => st.a.clone(),
            SouthDataClass::B => st.b.clone(),
            SouthDataClass::C => st.c.clone(),
        }
    }

    /// 把所有 **MQTT** 点都注入为新鲜 Ok 值（含位点的站级活性）——
    /// "首轮全量"类用例的前置。
    fn seed_all_mqtt_values(latest: &LatestValues, pts: &[UplinkPoint], now: u64) {
        let mut samples: Vec<(PointId, PointValue)> = Vec::new();
        let mut stations: std::collections::HashSet<String> = std::collections::HashSet::new();
        for p in pts.iter().filter(|p| p.channels.has(ChannelMask::MQTT)) {
            samples.push((
                PointId {
                    station: p.station.clone(),
                    metric: p.metric.clone(),
                },
                ok_val(1.0, now),
            ));
            stations.insert(p.station.clone());
        }
        for s in stations {
            latest.mark_station_polled(&s, now);
        }
        latest.apply(samples);
    }

    fn pop_cached(p: &MqttUplinkPublisher) -> Vec<Pending> {
        let mut out = Vec::new();
        let mut c = p.cache.lock().unwrap_or_else(|e| e.into_inner());
        while let Some(x) = c.pop_front() {
            out.push(x);
        }
        out
    }

    // ── ① 点数断言（AC-U74-02 的机械判据） ──

    /// **MQTT 全量点数 = 624（PCS 启用）/ 552（未启用）**，且 = 点表 MQTT 子集条数。
    #[test]
    fn mqtt_plan_point_count_is_624_with_pcs_and_552_without() {
        let cfg6 = cfg();
        let pts = build_uplink_points(&cfg6).expect("点表");
        let plan = plan_stations(&pts, &station_roles(&cfg6));
        assert_eq!(
            mqtt_point_count(&plan),
            MQTT_POINTS_WITH_PCS,
            "PCS 启用 ⇒ MQTT 全量 624（§9.3.3 / AC-U74-02）"
        );
        assert_eq!(
            plan.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            vec!["bms", "fire", "grid_meter", "hvac", "meter_batt", "pcs"],
            "站顺序确定（HashMap 迭代序不得泄漏到发布顺序）"
        );
        // 逐站逐档点数（与 §9.3.7 的字节表同源）
        let pick = |s: &str, c: SouthDataClass| {
            let st = plan.iter().find(|x| x.id == s).unwrap();
            st.len_of(c)
        };
        assert_eq!(pick("grid_meter", SouthDataClass::A), 6);
        assert_eq!(pick("meter_batt", SouthDataClass::A), 9);
        assert_eq!(pick("meter_batt", SouthDataClass::B), 31);
        assert_eq!(pick("bms", SouthDataClass::A), 4);
        assert_eq!(pick("bms", SouthDataClass::B), 53);
        assert_eq!(pick("bms", SouthDataClass::C), 288);
        assert_eq!(pick("fire", SouthDataClass::B), 114);
        assert_eq!(pick("fire", SouthDataClass::C), 13);
        assert_eq!(pick("hvac", SouthDataClass::B), 3);
        assert_eq!(pick("hvac", SouthDataClass::C), 31);
        assert_eq!(pick("pcs", SouthDataClass::A), 3);
        assert_eq!(
            pick("pcs", SouthDataClass::B),
            69,
            "PCS B 档实测（§9.3.7 表的 65 为按站估读，实测以点表为准）"
        );
        assert_eq!(pick("pcs", SouthDataClass::C), 0);

        // 去掉 pcs 站 ⇒ 552（PCS 未启用）
        let mut cfg5 = cfg();
        cfg5.stations
            .retain(|s| s.role != mupc_southd::config::Role::Pcs);
        let pts5 = build_uplink_points(&cfg5).expect("点表（无 PCS）");
        let plan5 = plan_stations(&pts5, &station_roles(&cfg5));
        assert_eq!(
            mqtt_point_count(&plan5),
            MQTT_POINTS_WITHOUT_PCS,
            "PCS 未启用 ⇒ MQTT 全量 552"
        );
        assert_eq!(
            pts5.iter()
                .filter(|p| p.channels.has(ChannelMask::MQTT))
                .count(),
            MQTT_POINTS_WITHOUT_PCS
        );
    }

    /// **15 个 BMS 聚合 = IEC104-only**：MQTT 子集里一个都不许有（C-16）。
    #[test]
    fn mqtt_channel_excludes_iec104_only_bms_aggregates() {
        let pts = points();
        let n = pts
            .iter()
            .filter(|p| p.channels.has(ChannelMask::MQTT) && p.metric.starts_with("bms_aggr_"))
            .count();
        assert_eq!(
            n, 0,
            "15 个 BMS 聚合是 IEC104-only（物联平台订原始 288 位）"
        );
    }

    // ── ② 主题与载荷逐字（§9.3.4） ──

    /// **主题逐字**：`{topic_prefix}/telemetry/{station}` 与 §9.3.4 的 `north_telemetry()`
    /// 在缺省前缀下**同一字面**（配置驱动与函数式主题不得两说）。
    #[test]
    fn telemetry_topic_matches_design_function() {
        let prefix = default_topic_prefix();
        for s in ["grid_meter", "bms", "pcs", "meter_batt", "fire", "hvac"] {
            assert_eq!(
                format!("{prefix}/telemetry/{s}"),
                mupc_mqtt_bridge::topics::north_telemetry(s),
                "缺省 topic_prefix 下两处主题必须逐字一致"
            );
        }
    }

    fn default_topic_prefix() -> String {
        crate::core_config::NorthCfg::default().topic_prefix
    }

    /// **载荷逐字**（§9.3.4 表）：字段名/取值口径逐项断言（`ts` ISO-8601 毫秒串、
    /// `dev`/`station`/`role`/`seq`/`points`，点内 `n`/`v`/`u`/`q`）。
    #[test]
    fn telemetry_payload_fields_are_verbatim() {
        let pts = points();
        let latest = LatestValues::new(5);
        let now = 1_758_697_600_123u64; // 2025-09-24T07:06:40.123Z（毫秒精度可判）
        seed_all_mqtt_values(&latest, &pts, now);
        let pubr = test_publisher(
            Arc::new(latest),
            Arc::new(pts.clone()),
            north_cfg_with(|_| {}),
            Arc::new(RecordingEvents::default()),
        );

        let st = pubr
            .plan()
            .iter()
            .find(|s| s.id == "grid_meter")
            .unwrap()
            .clone();
        let idx = indices_of(pubr.plan(), "grid_meter", SouthDataClass::A);
        let (topic, body, ts_ms, seq) = pubr
            .build_message(&st, &idx, now)
            .expect("grid A 档有值 ⇒ 必产消息");
        assert_eq!(topic, "mupc/north/telemetry/grid_meter", "§9.3.4 主题");
        assert_eq!(seq, 1, "首轮 seq 从 1 起");
        assert_eq!(ts_ms, now, "消息时标 = 采集时刻（不是发送时刻）");

        let v: serde_json::Value = serde_json::from_slice(&body).expect("合法 JSON");
        assert_eq!(
            v["ts"], "2025-09-24T07:06:40.123Z",
            "ts 线序 = ISO-8601 毫秒字符串（Q-E 默认 (a)）"
        );
        assert_eq!(v["dev"], "MUPC-0001", "装置标识来自装配入参");
        assert_eq!(v["station"], "grid_meter");
        assert_eq!(
            v["role"], "meter_grid",
            "role 取 YAML role 的 snake_case 表示"
        );
        assert_eq!(v["seq"], 1);
        let points = v["points"].as_array().expect("points 数组");
        assert_eq!(points.len(), 6, "grid A 档 6 点（不含分相 15 点）");
        // 字段名逐字 + 单位表（grid 派生名的显式单位）
        let by_name = |n: &str| {
            points
                .iter()
                .find(|p| p["n"] == n)
                .unwrap_or_else(|| panic!("缺少点名 {n}（LV-5：南向点名逐字）"))
        };
        assert_eq!(by_name("active_power")["u"], "kW");
        assert_eq!(by_name("reactive_power")["u"], "kvar");
        assert_eq!(by_name("voltage")["u"], "V");
        assert_eq!(by_name("current")["u"], "A");
        assert_eq!(by_name("frequency")["u"], "Hz");
        assert_eq!(by_name("cos_phi")["u"], "");
        assert_eq!(by_name("active_power")["v"], 1.0);
        assert_eq!(by_name("active_power")["q"], "ok");
        // 点内字段**只有** n/v/u/q（§9.3.4 逐字；不得夹带内部字段）
        for p in points {
            let keys: Vec<&String> = p.as_object().unwrap().keys().collect();
            assert_eq!(keys, vec!["n", "v", "u", "q"], "点位字段名/顺序逐字");
        }
        // 位点单位 = "bool"
        let st_b = pubr.plan().iter().find(|s| s.id == "bms").unwrap().clone();
        let idx_b = indices_of(pubr.plan(), "bms", SouthDataClass::C);
        let (_, body_b, _, _) = pubr.build_message(&st_b, &idx_b, now).unwrap();
        let vb: serde_json::Value = serde_json::from_slice(&body_b).unwrap();
        assert!(
            vb["points"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p["u"] == "bool"),
            "位点单位恒为 bool（含 BMS 288 位）"
        );
        assert_eq!(vb["role"], "battery");
        assert_eq!(vb["station"], "bms");
    }

    /// **`q != ok` 与 `v` 的口径**（§9.3.4 / AC-U74-05）：不可得 ⇒ `v=null`、
    /// `q=unconfigured`；采集失败（`Invalid`）⇒ **保留原值**且 `q=invalid`
    /// （**严禁 0 顶替**，且"合法 0 采样"与"失效"在载荷上可区分）。
    #[test]
    fn payload_keeps_original_value_and_marks_quality() {
        let pts = points();
        let latest = Arc::new(LatestValues::new(5));
        let now = 1_758_604_800_123u64;
        let pubr = test_publisher(
            latest.clone(),
            Arc::new(pts.clone()),
            north_cfg_with(|_| {}),
            Arc::new(RecordingEvents::default()),
        );
        let st = pubr
            .plan()
            .iter()
            .find(|s| s.id == "grid_meter")
            .unwrap()
            .clone();
        let idx = indices_of(pubr.plan(), "grid_meter", SouthDataClass::A);

        // ① 合法 0 采样 ⇒ q=ok, v=0
        latest.mark_station_polled("grid_meter", now);
        latest.apply(vec![
            (id("grid_meter", "active_power"), ok_val(0.0, now)),
            (
                id("grid_meter", "voltage"),
                PointValue {
                    value: Some(220.0),
                    ts_ms: now,
                    quality: PointQuality::Invalid,
                },
            ),
        ]);
        let (_, body, _, _) = pubr.build_message(&st, &idx, now).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let find = |n: &str| {
            v["points"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["n"] == n)
                .unwrap()
                .clone()
        };
        let ap = find("active_power");
        assert_eq!(ap["v"], 0.0, "合法 0 采样必须原样上送");
        assert_eq!(ap["q"], "ok");
        let volt = find("voltage");
        assert_eq!(volt["v"], 220.0, "Invalid ⇒ **保留原值**（不得写 0）");
        assert_eq!(volt["q"], "invalid");
        // ② 从未采集的点 ⇒ v=null 且 q=unconfigured（与"值为 0 的合法采样"可区分）
        let cur = find("current");
        assert!(cur["v"].is_null(), "不可得 ⇒ null（严禁 0 顶替）");
        assert_eq!(cur["q"], "unconfigured");
    }

    /// **过期（stale）**：质量 `Ok` 但已过 `stale_timeout_s` ⇒ 载荷 `q = stale`
    /// （判据唯一真源 = `LatestValues::is_fresh`；标量按时标、位点按站级活性）。
    #[test]
    fn stale_scalar_is_marked_stale() {
        let pts = points();
        let latest = Arc::new(LatestValues::new(5));
        let now = 1_758_604_800_123u64;
        let pubr = test_publisher(
            latest.clone(),
            Arc::new(pts.clone()),
            north_cfg_with(|_| {}),
            Arc::new(RecordingEvents::default()),
        );
        let st = pubr
            .plan()
            .iter()
            .find(|s| s.id == "grid_meter")
            .unwrap()
            .clone();
        let idx = indices_of(pubr.plan(), "grid_meter", SouthDataClass::A);
        latest.mark_station_polled("grid_meter", now);
        latest.apply(vec![(
            id("grid_meter", "active_power"),
            ok_val(5.0, now - 10_000), // 10 s 前采集 ⇒ 超 5 s 门限
        )]);
        let (_, body, _, _) = pubr.build_message(&st, &idx, now).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let ap = v["points"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["n"] == "active_power")
            .unwrap();
        assert_eq!(ap["q"], "stale", "过期标量 ⇒ q=stale（不得谎报 ok）");
        assert_eq!(ap["v"], 5.0, "过期仍保留原值（不得置 0/null）");
    }

    /// **凭据不得进载荷**（§9.3.5 ③）：密码/用户名不进 JSON（载荷字段是白名单结构）。
    #[test]
    fn payload_never_contains_credentials() {
        let pts = points();
        let latest = LatestValues::new(5);
        let now = 1_758_604_800_123u64;
        seed_all_mqtt_values(&latest, &pts, now);
        let pubr = test_publisher(
            Arc::new(latest),
            Arc::new(pts.clone()),
            north_cfg_with(|c| {
                c.username = Some("mupc-user".into());
                c.password = Some("s3cr3t-p@ss".into());
            }),
            Arc::new(RecordingEvents::default()),
        );
        for st in pubr.plan().iter() {
            for class in [SouthDataClass::A, SouthDataClass::B, SouthDataClass::C] {
                let idx = match class {
                    SouthDataClass::A => st.a.clone(),
                    SouthDataClass::B => st.b.clone(),
                    SouthDataClass::C => st.c.clone(),
                };
                if idx.is_empty() {
                    continue;
                }
                let (_, body, _, _) = pubr.build_message(st, &idx, now).unwrap();
                let text = String::from_utf8(body).unwrap();
                assert!(
                    !text.contains("s3cr3t-p@ss") && !text.contains("mupc-user"),
                    "载荷绝不得含凭据（{} / {:?}）",
                    st.id,
                    class
                );
            }
        }
    }

    // ── ③ 分片 / 首轮全量 / COS 合并窗 ──

    /// **首轮全量 + 站间不合并 + 分片主题**：一次 A 档轮 = 每站**一条**消息（4 站 4 条），
    /// 主题按站分片、载荷 `station` 与主题后缀一致、点集 = 该站该档点表全量。
    #[tokio::test]
    async fn first_round_is_sharded_per_station_and_not_merged() {
        let pts = points();
        let latest = Arc::new(LatestValues::new(5));
        let now = now_millis();
        seed_all_mqtt_values(&latest, &pts, now);
        let pubr = test_publisher(
            latest,
            Arc::new(pts.clone()),
            north_cfg_with(|_| {}),
            Arc::new(RecordingEvents::default()),
        );
        // 未连接 ⇒ 发布全部走离线缓存（本用例的"抓包"位置）
        pubr.publish_round(SouthDataClass::A, None).await;
        let got = pop_cached(&pubr);
        assert_eq!(
            got.len(),
            4,
            "A 档 = grid_meter/meter_batt/bms/pcs 各一条（不合并）"
        );
        let mut seen: Vec<String> = Vec::new();
        for m in &got {
            let v: serde_json::Value = serde_json::from_slice(&m.payload).unwrap();
            let station = v["station"].as_str().unwrap().to_string();
            assert_eq!(
                m.topic,
                format!("mupc/north/telemetry/{station}"),
                "主题分片必须与载荷 station 一致"
            );
            let expect = indices_of(pubr.plan(), &station, SouthDataClass::A).len();
            assert_eq!(
                v["points"].as_array().unwrap().len(),
                expect,
                "{station} A 档点集 = 点表全量（{expect} 点）"
            );
            seen.push(station);
        }
        seen.sort();
        assert_eq!(seen, vec!["bms", "grid_meter", "meter_batt", "pcs"]);
        // seq 每站独立、自 1 起
        assert!(got.iter().all(|m| m.seq == 1), "每站 seq 独立计数");
    }

    /// **COS 只发变更点**（§9.3.3 C 档）：变更集只含一点 ⇒ 该站只发这一点，
    /// 且**未变位站不发**（"值不变不发"）。
    #[tokio::test]
    async fn cos_round_sends_only_changed_points_and_skips_untouched_stations() {
        let pts = points();
        let latest = Arc::new(LatestValues::new(5));
        let now = now_millis();
        seed_all_mqtt_values(&latest, &pts, now);
        let pubr = test_publisher(
            latest,
            Arc::new(pts.clone()),
            north_cfg_with(|_| {}),
            Arc::new(RecordingEvents::default()),
        );
        // 变更点必须是 **MQTT 通道里真实存在**的点：`bms_alarm_26`（位地址 225，§9.3.4 的
        // 逐字示例，属 288 位块 = MQTT-only）。**不得用 `bms_aggr_*`**——15 个 BMS 聚合是
        // **IEC104-only**（§9.2.1.0 / C-16），拿它当变更点则 MQTT 侧根本无此点 ⇒ 断言无意义。
        let changed: HashSet<(String, String)> = [("bms".to_string(), "bms_alarm_26".to_string())]
            .into_iter()
            .collect();
        pubr.publish_round(SouthDataClass::C, Some(&changed)).await;
        let got = pop_cached(&pubr);
        assert_eq!(got.len(), 1, "只有 bms 变位 ⇒ 只发 1 条（其余站不发）");
        let v: serde_json::Value = serde_json::from_slice(&got[0].payload).unwrap();
        assert_eq!(v["station"], "bms");
        let points = v["points"].as_array().unwrap();
        assert_eq!(points.len(), 1, "只含变更点");
        assert_eq!(points[0]["n"], "bms_alarm_26");
        // C 档也走遥测主题、QoS1（故障类事件才走 event/{station} QoS2）
        assert_eq!(got[0].topic, "mupc/north/telemetry/bms");
        assert_eq!(got[0].qos, 1);
    }

    /// **空批不发**：本站本档全部点从未采集（`Unconfigured`）⇒ 不产生报文。
    #[test]
    fn unconfigured_station_produces_no_message() {
        let pts = points();
        let latest = LatestValues::new(5);
        let pubr = test_publisher(
            Arc::new(latest),
            Arc::new(pts.clone()),
            north_cfg_with(|_| {}),
            Arc::new(RecordingEvents::default()),
        );
        let st = pubr.plan().iter().find(|s| s.id == "bms").unwrap().clone();
        let idx = indices_of(pubr.plan(), "bms", SouthDataClass::C);
        assert!(
            pubr.build_message(&st, &idx, now_millis()).is_none(),
            "全点未采集 ⇒ 不发空转报文"
        );
    }

    // ── ④ 离线缓存（§9.3.3：先到先淘汰 + 计数 + 留证） ──

    fn pending(ts: u64, tag: u8) -> Pending {
        Pending {
            topic: format!("t/{tag}"),
            payload: vec![tag],
            qos: 1,
            seq: tag as u64,
            ts_ms: ts,
        }
    }

    /// 条数上限：**丢最旧**，`dropped` 累计，留证范围 = 被丢条目的 `[first_ts, last_ts]`。
    #[test]
    fn offline_cache_evicts_oldest_by_count_and_records_range() {
        let mut c = OfflineCache::default();
        let now = now_millis();
        for i in 0..5u8 {
            let (dropped, range) = c.push(pending(now - 1000 + i as u64, i), 3600, 3);
            if i < 3 {
                assert_eq!(dropped, 0, "未超上限 ⇒ 不丢");
                assert!(range.is_none());
            }
        }
        assert_eq!(c.len(), 3, "上限 3 条");
        assert_eq!(c.dropped(), 2, "丢最旧 2 条");
        assert_eq!(
            c.last_drop_range(),
            Some((now - 1000, now - 999)),
            "留证范围 = 被丢条目的最早/最晚采集时刻（BF-4/EX-9）"
        );
        // 队内保留的是**最新**的 2/3/4 号
        let kept: Vec<u8> = (0..3).map(|_| c.pop_front().unwrap().payload[0]).collect();
        assert_eq!(kept, vec![2, 3, 4], "丢最旧、保最新");
    }

    /// 时间窗：早于 `now − max_age_s` 的条目在**入队时**即被淘汰（补送过期数据无意义）。
    #[test]
    fn offline_cache_evicts_by_age_window() {
        let mut c = OfflineCache::default();
        let now = now_millis();
        // 窗口 1 s：一条 10 s 前的旧消息先入队（队列为空 ⇒ 不淘汰自己）
        let (d0, _) = c.push(pending(now - 10_000, 0), 1, 100);
        assert_eq!(d0, 0);
        // 再入一条新消息 ⇒ 旧条目落在窗外，被淘汰
        let (d1, range) = c.push(pending(now, 1), 1, 100);
        assert_eq!(d1, 1, "窗外条目必须被淘汰");
        assert_eq!(range, Some((now - 10_000, now - 10_000)));
        assert_eq!(c.len(), 1);
        assert_eq!(c.dropped(), 1);
    }

    /// 补送失败**回填队首**（保序）：pop → push_front ⇒ 顺序不变。
    #[test]
    fn offline_cache_requeue_front_keeps_order() {
        let mut c = OfflineCache::default();
        let now = now_millis();
        for i in 0..3u8 {
            c.push(pending(now, i), 3600, 100);
        }
        let first = c.pop_front().unwrap();
        assert_eq!(first.seq, 0);
        c.push_front(first);
        let order: Vec<u64> = (0..3).map(|_| c.pop_front().unwrap().seq).collect();
        assert_eq!(order, vec![0, 1, 2], "回填队首后仍按 seq 单调");
    }

    /// **发布器级**：未连接 ⇒ 全部入缓存并计入 `stats().cached_len`；
    /// 超上限 ⇒ `dropped_total` 与一条 `mqtt_cache_overflow` major 事件（限频 1 min）。
    #[tokio::test]
    async fn publisher_caches_when_disconnected_and_reports_overflow() {
        let pts = points();
        let latest = Arc::new(LatestValues::new(5));
        let now = now_millis();
        seed_all_mqtt_values(&latest, &pts, now);
        let events = Arc::new(RecordingEvents::default());
        let pubr = test_publisher(
            latest,
            Arc::new(pts.clone()),
            north_cfg_with(|c| {
                c.cache.max_messages = 100;
                c.cache.max_age_s = 3600;
            }),
            events.clone(),
        );
        assert!(!pubr.is_connected(), "测试客户端不跑事件循环 ⇒ 恒未连接");
        for _ in 0..130 {
            pubr.publish_or_cache("mupc/north/telemetry/bms".into(), b"{}".to_vec(), now, 1)
                .await;
        }
        let s = pubr.stats();
        assert_eq!(s.cached_len, 100, "上限 100 条（先到先淘汰）");
        assert_eq!(s.dropped_total, 30, "丢弃计数不得静默（BF-6）");
        assert!(s.last_error.is_none(), "未连接不算发布错误（未尝试发布）");
        let evs = events.0.lock().unwrap().clone();
        let overflow: Vec<_> = evs
            .iter()
            .filter(|e| e.event_type == "mqtt_cache_overflow")
            .collect();
        assert!(!overflow.is_empty(), "缓存溢出必须留证（一条 major 事件）");
        assert!(
            overflow.len() <= 1,
            "WARN/事件按 1 min 聚合（不风暴）：{:?}",
            overflow.len()
        );
        assert!(overflow[0].message.contains("100") || overflow[0].message.contains("30"));
    }

    // ── ⑤ 装配缝：零连接尝试 / fail-closed（§9.4 序 7 / CFG-2 / TLS-2） ──

    /// 缺省 `mqtt_bridge:` 段（整段缺省 ⇒ 全 false / 空 ⇒ **零行为变化**，§9.3.2）。
    fn disabled_cfg() -> MqttBridgeConfig {
        MqttBridgeConfig::default()
    }

    /// **`enabled=false` ⇒ 零连接尝试**（CFG-2 / §9.4 序 7）。
    ///
    /// **探针**：本机起一个 TCP listener，把它当作北向/本地 broker 地址——
    /// 若装配层执行了任何连接代码，listener 必然 `accept` 到一条连接。
    /// 断言：① `clients_constructed == 0`（无客户端对象 ⇒ 结构上无连接路径）；
    /// ② `tasks` 为空（无后台任务）；③ 状态 `Disabled`；④ listener **超时未收到连接**。
    #[tokio::test]
    async fn disabled_config_makes_zero_connection_attempts() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let pts = points();
        let cfg_mqtt = MqttBridgeConfig {
            enabled: false,
            north: crate::core_config::NorthCfg {
                enabled: true, // 故意把分向打开：总开关 false 必须**压过**它
                broker: format!("127.0.0.1:{port}"),
                client_id: "probe".into(),
                tls: crate::core_config::TlsCfg {
                    allow_plaintext: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            local: crate::core_config::LocalCfg {
                enabled: true,
                broker: format!("127.0.0.1:{port}"),
                ..Default::default()
            },
        };
        let out = assemble_mqtt_bridge(
            &cfg_mqtt,
            Arc::new(LatestValues::new(5)),
            Arc::new(pts),
            roles_of(&cfg()),
            None,
            Arc::new(RecordingEvents::default()),
        )
        .await;

        assert_eq!(out.clients_constructed, 0, "零连接尝试：不得构造任何客户端");
        assert!(out.tasks.is_empty(), "零连接尝试：不得起任何后台任务");
        assert_eq!(out.status, MqttServiceStatus::Disabled);
        assert!(out.detail.is_none());
        // 探针：300 ms 内 listener 不得收到任何连接
        let got = tokio::time::timeout(Duration::from_millis(300), listener.accept()).await;
        assert!(
            got.is_err(),
            "enabled=false 时**不得**产生任何连接尝试（listener 却 accept 到了连接）"
        );
    }

    /// **正向对照（防假绿）**：同样的探针在"启用 + 明文"下**必须**测到连接
    /// ⇒ 证明上一条的"没测到"是真的没有连接，而不是探针失灵。
    #[tokio::test]
    async fn probe_detects_connection_when_enabled() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let pts = points();
        let cfg_mqtt = MqttBridgeConfig {
            enabled: true,
            north: crate::core_config::NorthCfg {
                enabled: true,
                broker: format!("127.0.0.1:{port}"),
                client_id: "probe".into(),
                tls: crate::core_config::TlsCfg {
                    allow_plaintext: true, // debug 构建 + 显式开关（Q9）
                    ..Default::default()
                },
                ..Default::default()
            },
            local: crate::core_config::LocalCfg::default(),
        };
        let out = assemble_mqtt_bridge(
            &cfg_mqtt,
            Arc::new(LatestValues::new(5)),
            Arc::new(pts),
            roles_of(&cfg()),
            None,
            Arc::new(RecordingEvents::default()),
        )
        .await;
        assert_eq!(out.clients_constructed, 1, "启用 ⇒ 构造 1 个北向客户端");
        assert!(!out.tasks.is_empty(), "启用 ⇒ 事件循环 + 上送任务已起");
        assert_eq!(out.status, MqttServiceStatus::Running);

        let accepted = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await;
        assert!(
            accepted.is_ok(),
            "启用后事件循环必须真的尝试连接 broker（探针未测到连接 ⇒ 探针失灵）"
        );
        for (_, h) in out.tasks {
            h.abort();
        }
    }

    /// **TLS fail-closed（TLS-2）**：`allow_plaintext=false` 且证书不可读 ⇒
    /// ① 客户端构造失败（`Failed`）；② **不重试明文**（探针：listener 收不到连接）；
    /// ③ 无任何后台任务（不会"失败后偷偷连"）。
    #[tokio::test]
    async fn tls_without_readable_certs_fails_closed_without_plaintext_retry() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let pts = points();
        let cfg_mqtt = MqttBridgeConfig {
            enabled: true,
            north: crate::core_config::NorthCfg {
                enabled: true,
                broker: format!("127.0.0.1:{port}"),
                client_id: "probe".into(),
                tls: crate::core_config::TlsCfg {
                    ca_cert: "Z:/nope/ca.crt".into(),
                    client_cert: "Z:/nope/client.crt".into(),
                    client_key: "Z:/nope/client.key".into(),
                    allow_plaintext: false,
                },
                ..Default::default()
            },
            local: crate::core_config::LocalCfg::default(),
        };
        let out = assemble_mqtt_bridge(
            &cfg_mqtt,
            Arc::new(LatestValues::new(5)),
            Arc::new(pts),
            roles_of(&cfg()),
            None,
            Arc::new(RecordingEvents::default()),
        )
        .await;
        assert_eq!(out.clients_constructed, 0, "证书不可读 ⇒ 不得构造客户端");
        assert_eq!(out.status, MqttServiceStatus::Failed);
        assert!(out.tasks.is_empty(), "失败后不得留下任何后台任务");
        assert!(
            out.detail.as_deref().unwrap_or("").contains("north:"),
            "失败原因须点名 north（不重试明文，TLS-2）"
        );
        let got = tokio::time::timeout(Duration::from_millis(300), listener.accept()).await;
        assert!(
            got.is_err(),
            "TLS 建连失败后**不得**回落明文去连 broker（fail-closed）"
        );
    }

    /// **装配决策纯函数**：未启用 / 分向未开 ⇒ 全 `None`；启用 + north ⇒ 仅 north。
    #[test]
    fn launch_plan_gates_on_switches() {
        let mut m = disabled_cfg();
        assert!(plan_mqtt_launch(&m).is_empty(), "缺省整段 ⇒ 什么都不做");

        m.enabled = false;
        m.north.enabled = true;
        assert!(plan_mqtt_launch(&m).is_empty(), "总开关优先于分向开关");

        m.enabled = true;
        // `allow_plaintext` 的**缺省必须是 false**（§9.3.2：「仅非生产构建 + 显式 true 允许明文
        // （Q9，默认 false）」）⇒ 要断言"逐字段搬运"（而不是断言一个恒假常量），必须**先显式
        // 打开**再比对；直接对缺省段断言 true 与合同相反。
        m.north.tls.allow_plaintext = true;
        let p = plan_mqtt_launch(&m);
        assert!(
            p.north.is_some() && p.local.is_none(),
            "只开 north ⇒ 只装 north"
        );
        let n = p.north.unwrap();
        assert_eq!(n.broker_addr, m.north.broker);
        assert!(n.allow_plaintext, "allow_plaintext 逐字段搬运");
        assert!(n.password.is_none());
    }

    /// **C-12 逐字段映射**：YAML 层 north → crate 层 `NorthMqttConfig`（不漏字段、不发明字段）。
    #[test]
    fn north_config_mapping_is_field_by_field() {
        let c = crate::core_config::NorthCfg {
            enabled: true,
            broker: "broker.local:8883".into(),
            client_id: "cid-1".into(),
            username: Some("u".into()),
            password: Some("pw-should-not-leak".into()),
            tls: crate::core_config::TlsCfg {
                ca_cert: "/ca".into(),
                client_cert: "/cc".into(),
                client_key: "/ck".into(),
                allow_plaintext: false,
            },
            topic_prefix: "mupc/x".into(),
            qos: 2,
            periods: crate::core_config::PeriodsCfg {
                a_ms: 2000,
                b_ms: 9000,
                cos_merge_ms: 500,
            },
            cache: crate::core_config::CacheCfg {
                max_age_s: 600,
                max_messages: 2000,
            },
        };
        let n = north_client_config(&c);
        assert_eq!(n.broker_addr, "broker.local:8883");
        assert_eq!(n.client_id, "cid-1");
        assert_eq!(n.username.as_deref(), Some("u"));
        assert_eq!(n.password.as_deref(), Some("pw-should-not-leak"));
        assert!(!n.allow_plaintext);
        assert_eq!(n.tls.ca_cert, std::path::PathBuf::from("/ca"));
        assert_eq!(n.tls.client_cert, std::path::PathBuf::from("/cc"));
        assert_eq!(n.tls.client_key, std::path::PathBuf::from("/ck"));
        assert!(n.enabled);
        // `Debug` 掩码（密码不入日志；§9.3.5 ①）
        let dbg = format!("{n:?}");
        assert!(
            !dbg.contains("pw-should-not-leak"),
            "映射后的客户端配置 Debug 不得泄漏密码：{dbg}"
        );
        assert!(dbg.contains("***"));

        // 生产者侧配置（档位/缓存，由发布器解释）
        let p = MqttPublishCfg::from(&c);
        assert_eq!(p.topic_prefix, "mupc/x");
        assert_eq!(p.qos, 2);
        assert_eq!(p.a_interval, Duration::from_millis(2000));
        assert_eq!(p.b_interval, Duration::from_millis(9000));
        assert_eq!(p.cos_merge_ms, 500);
        assert_eq!(p.cache_max_age_s, 600);
        assert_eq!(p.cache_max_messages, 2000);

        // YAML 缺省与 §9.3.2 表一致
        let d = MqttPublishCfg::from(&crate::core_config::NorthCfg::default());
        assert_eq!(d.a_interval, Duration::from_millis(DEFAULT_MQTT_A_MS));
        assert_eq!(d.b_interval, Duration::from_millis(DEFAULT_MQTT_B_MS));
        assert_eq!(d.cos_merge_ms, DEFAULT_MQTT_COS_MERGE_MS);
    }

    /// **两份 deploy YAML**（§9.4 序 9 / §9.5 静态结构断言）：
    /// ① `auto_load` 不含 `mqtt_plugin`（唯一有效的下架动作）；② 含 `mqtt_bridge:` 段且
    /// `enabled: false`；③ 生产模板**不得**出现占位域名/dummy 证书路径（CFG-3）。
    #[test]
    fn deploy_yamls_drop_mqtt_plugin_and_ship_disabled_mqtt_bridge() {
        for (name, path) in [
            (
                "开发模板",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../deploy/config/mupc_core_config.yaml"
                ),
            ),
            (
                "生产模板",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../deploy/config/mupc_core_config.production.yaml"
                ),
            ),
        ] {
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("{name} 必须可读（{path}）: {e}"));
            assert!(
                !text.contains("mqtt_plugin"),
                "{name}：auto_load 不得再含 mqtt_plugin（§9.3.1 下架；插件按名加载 ⇒ 只有配置层可断）"
            );
            assert!(
                text.contains("mqtt_bridge:"),
                "{name}：必须含 mqtt_bridge 段（CFG-3；保留式编辑也要求该段存在）"
            );
            // 解析后确认缺省 disabled + broker 空串（不残留任何假地址）
            let cfg: CoreConfig = serde_yaml::from_str(&text)
                .unwrap_or_else(|e| panic!("{name} 必须可被 CoreConfig 解析: {e}"));
            assert!(
                !cfg.mqtt_bridge.enabled,
                "{name}：缺省必须 disabled（CFG-2）"
            );
            assert!(
                cfg.mqtt_bridge.north.broker.is_empty(),
                "{name}：broker 不得留占位地址（CFG-3）"
            );
            assert!(
                cfg.mqtt_bridge.north.tls.ca_cert.is_empty(),
                "{name}：不得留 dummy 证书路径（CFG-3）"
            );
            // 全文件级：不得出现 example 域名
            assert!(!text.contains("example.com"), "{name}：不得含占位域名");
        }
    }
}
