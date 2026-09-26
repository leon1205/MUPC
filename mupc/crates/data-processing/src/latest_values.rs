//! 外设遥测「最新值快照 + 变更通知」入口（LV-1～LV-6）。
//!
//! **设计真源**：[01 设计 §9.1](../../../../docs/superpowers/plans/modules/01-MUPC-通信网关-设计文档.md)
//! （01 / 03 / 12 三份设计的**共用件**，唯一真源在该章；03 设计 §9.5 只做归属确认，
//! 12 设计 §15 按消费契约引用）。需求口径 = 01 PRD §8.5（LV-1～LV-6）、03 PRD §11.6.2
//! （R-11.6-D1～D5）、12 PRD §3.9.0（RQ-9.0-1～5）。
//!
//! **归属与边界**（§9.1.1 / §9.1.7）：
//! - 本模块在 **`mupc-data-processing`**（依赖图上 `southd → data-processing`、
//!   `core-bin → data-processing` 均已存在 ⇒ **零新增依赖边、零反向依赖**）；
//! - **写入方 = core-bin 的 `SouthSink`**（装配层，三个 `StationSink` 回调）；
//!   `mupc-southd` 的 `StationSink` trait **不改**；
//! - **读取方** = IEC104 上送器 / MQTT 发布器（后续任务）、`display_host`、策略引擎；
//! - **不替代**任何既有数据面：`AiIntegrator::latest_data` / `set_battery_soc`（策略 phase 输入）、
//!   `WriteBuffer`/`telemetry`（**历史**通道）、`display_host` 的帧派生视图 —— 本快照是
//!   **实时值**通道，与历史表"不得互相替代"（§9.1.7）。
//!
//! **过期判据单一真源**：`stale_timeout_s` **注入**（来源 = `SouthStationsConfig::stale_timeout_s`，
//! 02 PRD §9.7.1 同源），[`LatestValues::is_fresh`] 是**唯一**实现；调用点**禁止**传字面量 5，
//! 亦**禁止**在消费方另写 `now - ts > X` 形式的第二套门限。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;

/// 快照常驻容量预留（§9.1.6 内存上界）：**639 点** = MQTT 全量 624 ∪ IEC104 聚合 15（PCS 启用；
/// 未启用 567）。点表规模在配置期即唯一确定 ⇒ 构造期预留后**无动态增长，无 OOM 面**。
/// 上界内存 ≈ 75–95 KB（`HashMap` 负载因子开销后 ≤ 128 KB）。
const SNAPSHOT_CAPACITY: usize = 639;

/// 变更广播容量（§9.1.3）：**满即丢最旧**，落后订阅方收到 [`tokio::sync::broadcast::error::RecvError::Lagged`]。
/// 丢帧是**允许**的（"允许丢帧、不允许静默传输损坏数值"）；消费方契约 = 全量重读 `all()` 重建基线。
/// 12 设计的"外设页 ≤ 2 s 上屏"即建立在此前提上：**丢失由兜底 tick 收敛**（12 设计 §15.6.1）。
const CHANGE_CHANNEL_CAPACITY: usize = 64;

/// 点位质量（**与 01 PRD §8.3.3 载荷的 `q` 枚举逐字一致**，单一枚举来源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointQuality {
    /// 有有效值且未过期
    Ok,
    /// 有值但已超过新鲜度门限
    Stale,
    /// 采集失败 / 站点离线（值可保留为最后一次有效值原值）
    Invalid,
    /// 站点未启用 / 点未配置（**从未采集过**）
    Unconfigured,
}

/// 单点最新值。
#[derive(Debug, Clone, PartialEq)]
pub struct PointValue {
    /// 工程值；`None` = **不可得**（严禁以 0.0 顶替，LV-6）。
    pub value: Option<f64>,
    /// **采集时刻** UTC 毫秒（不得用写入时刻 / 发送时刻顶替）。语义分两类
    /// （**这是位点与标点的唯一差别**，§9.1.2）：
    /// - **标量点**：每轮都读回 ⇒ `ts_ms` = **本轮轮询成功时刻**；
    /// - **位点**：southd 只按"变化沿"交付 ⇒ `ts_ms` = **该位最后一次变化的时刻**
    ///   （"该状态自何时起有效"）。位点的可得性判据**不是**逐点 5 s 新鲜度，而是
    ///   **站级轮询活性 ∧ 点位质量**，见 [`LatestValues::is_fresh`]。
    pub ts_ms: u64,
    /// 质量。
    pub quality: PointQuality,
}

/// 点位主键。`metric` 与南向点名**逐字一致**（02 PRD §9.4.2.2）；**不得**二次改名（LV-5）。
/// ⚠️ `station` 仅用于**唯一性**（同名点跨站），**不构成改名**：上云载荷的 `n` 字段只写 `metric`。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PointId {
    pub station: String,
    pub metric: String,
}

/// 读结果视图（**永远返回**：不可得也返回显式态，不返回 `None`）。
#[derive(Debug, Clone, PartialEq)]
pub struct PointView {
    pub id: PointId,
    pub value: PointValue,
}

/// 一次变更批（**天然合并**：同一 `apply` 调用内的全部点打包为一批，满足 LV-4 的"允许合并"）。
#[derive(Debug, Clone)]
pub struct ChangeBatch {
    /// 单调递增版本号（批序号），自 1 起。
    pub seq: u64,
    /// 本批变更点，**按 `station` 分组**（便于分片发布）；批内同点只出现一次。
    pub changed: Vec<PointId>,
}

/// 外设最新值快照 + 变更通知（进程内唯一实例，`Arc` 共享）。
pub struct LatestValues {
    /// 上界 = 站数 × 站内点数（见 §9.1.6），构造期**预留容量**。
    map: RwLock<HashMap<PointId, PointValue>>,
    /// **站级轮询活性**：站 id → 最近一次"本轮轮询成功"的时刻（ms）。
    /// 位点的可得性据此判定（见 [`LatestValues::is_fresh`]）；标量点另有逐点 `ts_ms`。
    station_poll_ms: RwLock<HashMap<String, u64>>,
    /// 新鲜度门限（**注入，不另立常量**；单位 s）。
    stale_timeout_s: u64,
    /// 变更广播。
    change_tx: tokio::sync::broadcast::Sender<ChangeBatch>,
    /// 批序号。
    seq: AtomicU64,
}

/// 不可得点的**显式**取值（LV-6）：`value: None` + `Unconfigured` + 时标 0（无采集事实）。
/// 消费者据此可区分"从未采集过"与"值为 0"。
fn unavailable() -> PointValue {
    PointValue {
        value: None,
        ts_ms: 0,
        quality: PointQuality::Unconfigured,
    }
}

/// COS 比较用「值是否相同」：按**位模式**比较，而非 `f64` 数值相等。
///
/// 理由：① `NaN != NaN` 会让含 NaN 的点**每轮都判为变更** ⇒ 通知/带宽被打满；
/// ② 位比较下只有 `-0.0 / +0.0` 这类数值相等但位不同的组合会被误判为"变更"，
/// 而**误报变更**（多一次通知）远轻于**漏报变更**（消费方静默丢失变位）。
fn value_eq(a: &Option<f64>, b: &Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.to_bits() == y.to_bits(),
        _ => false,
    }
}

impl LatestValues {
    /// `stale_timeout_s` 来自 `SouthStationsConfig::stale_timeout_s`（唯一真源，02 PRD §9.7.1 同源）。
    /// **禁止**在调用点传字面量 5；本结构体是判据的唯一持有者。
    pub fn new(stale_timeout_s: u64) -> Self {
        // 丢弃初始 Receiver：订阅一律经 `subscribe()` 建立（Sender 不因无订阅者失效）。
        let (change_tx, _) = tokio::sync::broadcast::channel(CHANGE_CHANNEL_CAPACITY);
        Self {
            map: RwLock::new(HashMap::with_capacity(SNAPSHOT_CAPACITY)),
            station_poll_ms: RwLock::new(HashMap::new()),
            stale_timeout_s,
            change_tx,
            seq: AtomicU64::new(0),
        }
    }

    /// 订阅变更（广播容量 64；落后丢帧 ⇒ 消费方必须全量重读，见 §9.1.5）。
    ///
    /// 消费方标准形态：`Ok(batch)` 只处理 `batch.changed`；`Err(Lagged(_))` ⇒
    /// **必须** `all()` 全量重读重建基线；`Err(Closed)` ⇒ 退出循环。
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ChangeBatch> {
        self.change_tx.subscribe()
    }

    /// 批写入（**唯一写入口**）。`samples` 内每项 `(PointId, PointValue)`。
    ///
    /// 语义（§9.1.5）：
    /// - **无条件 upsert** `ts_ms` / `quality`（"不广播"**不等于**"不刷新时标"）；
    /// - 逐点比较 **`value`**：全部相同 ⇒ 返回 `None`（**不广播**，COS "值不变不发"）；
    /// - 其余情况打包为一批广播（**天然合并**），返回批序号。
    ///
    /// **锁边界**：只做 upsert + 差异收集，`broadcast::send` 在**释放写锁之后**调用
    /// （不持锁跨 await / 不阻塞采集）。
    pub fn apply(&self, samples: Vec<(PointId, PointValue)>) -> Option<u64> {
        let changed = {
            let mut map = self.map.write();
            let mut changed: Vec<PointId> = Vec::new();
            for (id, v) in samples {
                // 未登记过 ⇒ 首次登记即为变更；已登记 ⇒ 逐点比较 value
                let is_change = match map.get(&id) {
                    Some(old) => !value_eq(&old.value, &v.value),
                    None => true,
                };
                if is_change && !changed.contains(&id) {
                    changed.push(id.clone());
                }
                map.insert(id, v);
            }
            // 按 station 分组（同站相邻），便于消费方分片发布
            changed.sort_by(|a, b| {
                (a.station.as_str(), a.metric.as_str())
                    .cmp(&(b.station.as_str(), b.metric.as_str()))
            });
            changed
        }; // ← 写锁在此释放

        if changed.is_empty() {
            return None;
        }
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        // 无订阅者时 `send` 返回 `Err`（正常态，不是错误）：批已产生但无人接收。
        // ⚠️ 因此**不得**把"通知必达"当作任何语义的前提（§9.1.5）。
        let _ = self.change_tx.send(ChangeBatch { seq, changed });
        Some(seq)
    }

    /// 单点读（不可得 ⇒ `value: None` + 显式 `quality`，**不返回 `Option`**）。
    pub fn get(&self, id: &PointId) -> PointView {
        let value = self.map.read().get(id).cloned().unwrap_or_else(unavailable);
        PointView {
            id: id.clone(),
            value,
        }
    }

    /// 按站取全部点（MQTT 分片发布 / 聚合求值用）：返回该站**全部已登记点**（含不可得态）。
    /// **单次持读锁克隆**（聚合求值须一次拿到全部位，不得逐点 [`Self::get`] —— 那会反复取锁）。
    /// 未登记站返回**空集**（消费方按"点缺 = NotRead"处理，不臆造）。
    pub fn station_snapshot(&self, station: &str) -> Vec<PointView> {
        let map = self.map.read();
        map.iter()
            .filter(|(id, _)| id.station == station)
            .map(|(id, v)| PointView {
                id: id.clone(),
                value: v.clone(),
            })
            .collect()
    }

    /// 全量取（快照 / 总召 / 首轮 COS / `Lagged` 后重建基线用）。
    pub fn all(&self) -> Vec<PointView> {
        let map = self.map.read();
        map.iter()
            .map(|(id, v)| PointView {
                id: id.clone(),
                value: v.clone(),
            })
            .collect()
    }

    /// **本站在采集活性窗口内**（`now - station_poll_ms[station] <= stale_timeout_s*1000`）。
    /// 站未知（从未轮询成功）⇒ `false`。
    pub fn station_is_active(&self, station: &str, now_ms: u64) -> bool {
        match self.station_poll_ms.read().get(station) {
            Some(t) => now_ms.saturating_sub(*t) <= self.stale_timeout_s.saturating_mul(1000),
            None => false,
        }
    }

    /// **站级最后成功时刻**（= `station_poll_ms[station]` 的公开读口，**接口要求 R-38**，
    /// 12 设计 §15.1.2 / §15.9）。
    ///
    /// `None` = **从未轮询成功**（含已被 [`Self::mark_station_offline`] 清除者）
    /// —— 消费方须显示不可得（如 `--`），**不得**臆造、**不得**自维护第二份真源。
    ///
    /// 该 getter **不新增任何判据**：过期判据仍唯一由 [`Self::is_fresh`] /
    /// [`Self::station_is_active`] 持有（真源仍是注入的 `stale_timeout_s`）。
    pub fn station_last_poll_ms(&self, station: &str) -> Option<u64> {
        self.station_poll_ms.read().get(station).copied()
    }

    /// **活性刷新（唯一调用点 = `SouthSink` 每次站轮成功）**：更新 `station_poll_ms[station] = now_ms`。
    /// 只刷新"站在采"这一事实，**不改任何点值/点位时标** ⇒ 不产生变更批、不影响 COS 语义。
    pub fn mark_station_polled(&self, station: &str, now_ms: u64) {
        self.station_poll_ms
            .write()
            .insert(station.to_string(), now_ms);
    }

    /// 有效点判定（**过期判据单一实现**）：`v.quality == Ok` **且**
    /// - 标量点（`is_bit == false`）：`now - v.ts_ms <= stale_timeout_s*1000`；
    /// - 位点（`is_bit == true`）：[`Self::station_is_active`]（位点时标语义 = 最后变化时刻）。
    ///
    /// 由调用方给出 `id.station` / 是否为位，避免本类型猜语义。
    pub fn is_fresh(&self, id: &PointId, v: &PointValue, is_bit: bool, now_ms: u64) -> bool {
        if v.quality != PointQuality::Ok {
            return false;
        }
        if is_bit {
            self.station_is_active(&id.station, now_ms)
        } else {
            now_ms.saturating_sub(v.ts_ms) <= self.stale_timeout_s.saturating_mul(1000)
        }
    }

    /// 站点离线：该站全部点 `quality = Invalid`（**保留原值与原始时标**，口径 = PRD EX-1）；
    /// 同时清 `station_poll_ms[station]`（恢复在线前 `station_is_active == false`）。
    ///
    /// **不产生变更批**：只改质量位、不改值（COS 判据在 `value` 上，§9.1.5）；离线事实
    /// 另由 `south_station.<id>.offline` 事件承担（§9.2.6），周期上送档会因 `quality != Ok`
    /// 自动过滤该站全部点（含聚合点）。
    pub fn mark_station_offline(&self, station: &str) {
        {
            let mut map = self.map.write();
            for (id, v) in map.iter_mut() {
                if id.station == station {
                    v.quality = PointQuality::Invalid;
                }
            }
        }
        self.station_poll_ms.write().remove(station);
    }
}

// ── 白盒用例 ────────────────────────────────────────────────────────────
//
// 集成测试（`tests/latest_values_tests.rs`）覆盖全部**公开面**语义；此处只放
// **公开面不可观测**的结构性断言（`map` / `station_poll_ms` 为私有字段）。
#[cfg(test)]
mod tests {
    use super::*;

    fn id(station: &str, metric: &str) -> PointId {
        PointId {
            station: station.to_string(),
            metric: metric.to_string(),
        }
    }

    /// §9.5 内存上界：**639 点**构造后 `HashMap` 容量不得低于点表规模上界
    /// （构造期预留 ⇒ `apply` 不以 rehash/扩容为常态，无动态增长 ⇒ 无 OOM 面）。
    #[test]
    fn snapshot_map_capacity_is_reserved_up_front() {
        let lv = LatestValues::new(5);
        assert!(
            lv.map.read().capacity() >= SNAPSHOT_CAPACITY,
            "构造期必须预留 {SNAPSHOT_CAPACITY} 点容量，实际 = {}",
            lv.map.read().capacity()
        );

        // 装到上界规模仍不发生扩容（容量不变 = 无动态增长）
        let before = lv.map.read().capacity();
        let samples = (0..SNAPSHOT_CAPACITY)
            .map(|i| {
                (
                    id("bulk", &format!("m{i}")),
                    PointValue {
                        value: Some(i as f64),
                        ts_ms: 1,
                        quality: PointQuality::Ok,
                    },
                )
            })
            .collect();
        lv.apply(samples);
        assert_eq!(lv.all().len(), SNAPSHOT_CAPACITY);
        assert_eq!(
            lv.map.read().capacity(),
            before,
            "{SNAPSHOT_CAPACITY} 点内不得扩容"
        );
    }

    /// 站活性表规模上界 = 站数（≤ 5 站 + 模拟站），且离线后条目被清除（不泄漏）。
    #[test]
    fn station_poll_map_is_bounded_and_cleared_on_offline() {
        let lv = LatestValues::new(5);
        for s in ["meter_grid", "battery", "hvac", "fire", "meter_batt"] {
            lv.mark_station_polled(s, 1_000);
        }
        assert_eq!(lv.station_poll_ms.read().len(), 5);

        lv.mark_station_offline("battery");
        assert_eq!(lv.station_poll_ms.read().len(), 4, "离线后条目不残留");
    }
}
