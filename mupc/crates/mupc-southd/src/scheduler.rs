//! 口级采集调度器（S3a Task 5；§10.2 口调度预算 / §10.7 站超时隔离）。
//!
//! 架构：**每 port 一条采集 task**（口间并发），口内多从站按到期（`next_due`）**串行**
//! 轮询（口单 poller 天然串行；Rs485PortBus 内另有 per-port async Mutex 双保险，Task 3）。
//! 站失败（任一寄存器块读 Err / mapper 语义 Failed）→ 站级 offline 隔离，不阻断同口其它站。
//!
//! §10.2 M-11 口调度预算：到期判定（`next_due`）+ 角色优先级排序（grid/battery 先于
//! hvac/fire，见 [`role_priority`]）+ **offline 慢站指数退避降频**（poll 失败后
//! [`DueCalc::delay_station`] 把 next_due 按 `interval << min(offline_count-1, 5)` 后移，
//! 封顶 32×interval——失败站降频不拖累同口关键站 cadence；恢复即正常 cadence）。
//!
//! 结果按 role 分发到 [`StationSink`]（core-bin 实现，Task 7；southd 不依赖
//! strategy/ai-integration，只定义 trait 边界）：
//! - `MeterGrid` → [`StationSink::on_grid_package`]（策略 phase 唯一写方，含分相）；
//! - 一切非 grid 站 → [`StationSink::on_station_telemetry`]（telemetry 落库 + 状态事件）。
//!   单写方口径：同一用途数据只走一条通道——battery 站 soc 经 telemetry 全量落库（旁路
//!   记录照旧），**同时**经 [`StationSink::on_battery_soc`] 独立通道推 AiIntegrator
//!   （SOC 双源裁决，BMS 优先/掉线回落核间，04 §2.11.1；不替代 on_station_telemetry）。
//!
//! 观测契约：每口 task 由其采集循环常驻，任一口 task 内 panic 会**静默终止**该口采集
//! （无自动重 spawn）。调用方必须持有并观测 [`SouthScheduler::spawn`] 返回的每个
//! `JoinHandle`（详见其文档；core-bin Task 7 接线时落实）。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::config::{RegFunc, Role, SouthStationsConfig, StationConf};
use crate::mapper::{self, BlockData, BlockReads, PollResult};
use crate::point_table::{self, BitClass, RegPointKind};
use crate::points::{self, PointKind};
use crate::port_runtime::{BusError, StationBus};
use crate::station::Station;

/// 采集结果上送回调（core-bin 实现；southd 不依赖 strategy/ai-integration）。
///
/// 消费方注意：grid 之外一切站（含 battery）的遥测值都经
/// [`StationSink::on_station_telemetry`]；`is_event=true` 的合成点（metric=`offline`/
/// `online`，value=1）为**状态事件**（离线告警/上线恢复），由 station_id+role 定位，
/// 非普通遥测。普通遥测点 `is_event=false`。
#[async_trait]
pub trait StationSink: Send + Sync {
    /// meter_grid 完整 pkg（含 phase）——单写方：AiIntegrator.set_latest_data。
    async fn on_grid_package(&self, pkg: mupc_data_processing::DataPackage);
    /// 非 grid 站遥测点（telemetry 落库 + 事件）——battery/hvac/fire/meter_batt。
    /// points 三元组 `(metric, value, is_event)`；is_event=true 表示状态事件。
    async fn on_station_telemetry(
        &self,
        station_id: &str,
        role: Role,
        points: Vec<(String, f64, bool)>,
    );

    /// battery 站（role=Battery）本轮 SOC（已由 mapper 解码进 pkg.battery.soc；本轮采集
    /// 刚成功即新鲜）。独立通道——AiIntegrator SOC 双源裁决用（BMS 优先/掉线回落核间，04 §2.11.1）。
    /// 不替代 on_station_telemetry（遥测全量落库照旧）；soc 值语义为 0-100 百分数。
    async fn on_battery_soc(&self, station_id: &str, soc: f64);

    /// 站失败（offline）事件 —— 把失败 `reason` 交给消费层（S3b-2 T5 新增接缝）。
    ///
    /// **为什么需要它**：PRD §9.7.2 第 1 条要求"站进入 offline，事件 `reason` 含
    /// `slave/addr/count` → 运维据 `events` 定位到具体块"，而 [`Self::on_station_telemetry`]
    /// 的三元组 `(metric, value, is_event)` **没有承载文案的字段**（`reason` 是字符串）。
    ///
    /// **默认实现 = 既有行为**：合成 `metric = "offline"` 的状态事件（与 S3a 口径逐字一致 ⇒
    /// 既有 `event_count(.., "offline")` 类断言不受影响）。消费方（core-bin `SouthSink`）可
    /// **覆写**本方法把 `reason` 写进 `SystemEvent.message` —— 事件文案的组装属消费层
    /// （设计 §11.3 的 core-bin 行），故 southd 侧只负责"把 reason 交出去"。
    async fn on_station_offline(&self, station_id: &str, role: Role, reason: &str) {
        let _ = reason;
        self.on_station_telemetry(station_id, role, vec![("offline".to_string(), 1.0, true)])
            .await;
    }
}

/// 本轮应采的一站。`station_index` = 调度 state Vec 全局下标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StationPoll {
    pub station_index: usize,
}

/// M-11 退避封顶：extra = interval << min(offline_count-1, MAX_BACKOFF_SHIFT)。
/// 5 → 封顶 32×interval（1s interval → 32s 退避上限，防永久停采）。
const MAX_BACKOFF_SHIFT: u32 = 5;

/// M-11 退避额外延时：offline_count=1 → 1×interval；每多一次失败 ×2，封顶 32×interval。
/// 独立纯函数便于直接单测封顶/边界（防永久停采，仍可探测恢复）。
fn backoff_extra(interval_ms: u64, offline_count: u32) -> u64 {
    let shift = offline_count.saturating_sub(1).min(MAX_BACKOFF_SHIFT);
    interval_ms.saturating_mul(1u64 << shift)
}

/// 角色优先级（口调度预算 §10.2：grid/battery 关键量优先于 hvac/fire；慢站降频不拖累关键站 cadence）。
/// S3b-2（PRD §9.3.2.3）：`pcs` 与 `meter_batt` 同为 1 档——**不与 grid/battery 同档**
/// （PCS 不参与控制决策，不得抢占总表 phase 的调度预算）。
fn role_priority(r: Role) -> u8 {
    match r {
        Role::MeterGrid | Role::Battery => 0,
        Role::MeterBatt | Role::Pcs => 1,
        Role::Hvac | Role::Fire => 2,
    }
}

/// 到期计算条目（纯逻辑）。
struct DueEntry {
    /// state Vec 全局下标
    station_index: usize,
    role: Role,
    interval_ms: u64,
    /// 下次到期时刻（uptime 毫秒；由调用方 now_ms 单调驱动，独立于真时钟）
    next_due: u64,
}

/// 口内到期排程（纯逻辑，无 IO、无真时钟依赖，可单测）。
///
/// 每口一个实例（口间独立 cadence：spawn 时各口 task 各自持有；见 [`SouthScheduler`]）。
/// 到期判定：`now_ms >= next_due`；推进：`next_due += interval`，落后一轮以上（
/// `now_ms >= next_due + interval`）钳制为 `now_ms + interval`（防追跳补采）。同一
/// `now_ms` 重复调用已到期站不再返回（next_due 已推过）——保证每轮每站至多采一次。
pub struct DueCalc {
    entries: Vec<DueEntry>,
}

impl DueCalc {
    /// 由本口站组（`(state 下标, conf)`，下标为 state Vec 全局序）构造。
    /// 决议：初 `next_due = 0` → 首轮全部立即到期（启动即采一次），此后按 interval 排程。
    fn from_group(group: &[(usize, &StationConf)]) -> Self {
        let entries = group
            .iter()
            .map(|(idx, c)| DueEntry {
                station_index: *idx,
                role: c.role,
                interval_ms: c.interval_ms,
                next_due: 0,
            })
            .collect();
        Self { entries }
    }

    /// 给定 now_ms（uptime 单调毫秒），返回本口「已到期应采的站」，并推进到期站 next_due。
    /// 返回序：口内按 `(role 优先级, 原序)` 稳定排序（grid/battery 优先于 hvac/fire）。
    pub fn due_round(&mut self, now_ms: u64) -> Vec<StationPoll> {
        let mut due: Vec<usize> = Vec::new();
        for (i, e) in self.entries.iter_mut().enumerate() {
            if now_ms >= e.next_due {
                if now_ms >= e.next_due + e.interval_ms {
                    e.next_due = now_ms + e.interval_ms; // 落后一轮以上 → 防追跳补采
                } else {
                    e.next_due += e.interval_ms;
                }
                due.push(i);
            }
        }
        // 原序为次键，保证优先级内稳定（同优先级保持 state 序）
        due.sort_by_key(|&i| (role_priority(self.entries[i].role), i));
        due.into_iter()
            .map(|i| StationPoll {
                station_index: self.entries[i].station_index,
            })
            .collect()
    }

    /// 退避：把站 next_due 后移到 `now_ms + extra_ms`（若现 next_due 已更晚则不动）。
    /// scheduler 在站 poll 失败后调用（offline 慢站降频，§10.2 M-11）。
    pub fn delay_station(&mut self, station_index: usize, now_ms: u64, extra_ms: u64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.station_index == station_index) {
            let target = now_ms.saturating_add(extra_ms);
            if e.next_due < target {
                e.next_due = target;
            }
        }
    }
}

/// 第 5 类信号：**站级派生布尔量**（`StationFlag`，v1.7 §11.4.7.1 末行 / §11.4.7.2 C）——
/// 判据是 `mapper` 的交叉校验 / 域检查的**布尔返回**（无寄存器、非跃迁量）：
/// 地址序违规（`fire_detector_addr_order_violation`）/ SOC 域检查 / 登记数交叉校验。
///
/// 它与前四类（`Bit`/`WordBit`/`WordEnum`）**共用同一个 [`EdgeTracker`] 与同一条事件产出
/// 路径**，区别只在①**产出频次口径**（状态翻转制：进入 1 条 / `@recovered` 1 条 / 未变不产，
/// 且**首次观测即产**）与②进入事件的 `value` 承载**诊断量**（见 [`station_flag_events`]）。
struct StationFlag {
    /// 事件名（**进入**用它；**退出**追加 `@recovered`）
    metric: &'static str,
    /// 本轮判据值（true = 异常成立）—— 喂 [`EdgeTracker`] 的那一格
    active: bool,
    /// 进入事件承载的**诊断量**（⑤ = 首个违规组的 1 基序号；③ = 读回登记数；
    /// ① = 越界原始值）。**退出**事件的 `value` 恒 `0.0`（哨兵），不取此字段。
    diag: f64,
}

/// 消防探测器地址序违规的事件名（§11.4.7 事件 ⑤）。
const ADDR_ORDER_INVALID: &str = "fire_detector_addr_order_invalid";

/// 某站的**变化沿记忆**（S3b-2 §11.4.7.1「统一事件模型」的实现 —— `PortRunner` 按站下标各持一个）。
///
/// **统一口径**：一个"信号"在 tracker 里占一格"上轮活跃态"，无论它是
/// ① 离散位点（`Bit`：`func: discrete` 块的位向量第 k 位）、
/// ② 字级信号（`WordBit`：整字 `& mask ≠ 0`；`WordEnum`：整字 ∈ `active` 值集），还是
/// ③ 第 5 类站级派生布尔量（`StationFlag`，见 [`EdgeTracker::mark_station_flags`]）。
/// 每轮算出 `(上轮, 本轮)` 二元组：`false→true` = 进入活跃（`value = 1.0`）、
/// `true→false` = 退出活跃（`value = 0.0`），**两者都返回**（是否采纳由调用方过滤，
/// 见 [`SouthScheduler::poll_station`] 的不对称过滤）。
///
/// **首次采样 / 站恢复后**：只建立基线、**不产事件**（`primed = false` 时 `edges()` 只
/// `prime()` 并返回空；`reset()` 使其回到该状态）—— 避免"启动即刷一屏事件""恢复即刷一屏"。
/// **唯一例外** = 已登记的 `StationFlag`（见其方法文档）。
#[derive(Default)]
pub struct EdgeTracker {
    /// 信号名 → 上轮活跃态（信号名 = 位点名 / `<点名>@<信号键>` / `StationFlag` 的事件名）
    last: HashMap<String, bool>,
    /// 是否已有基线（false = 本轮只建基线，不产事件）
    primed: bool,
    /// **分类**（非记忆）：第 5 类信号的事件名 —— 这些信号首轮/复位后首轮**仍产进入事件**。
    /// `prime()`/`reset()` 都不清它（信号分类由配置与 role 决定，不随轮次变化）。
    station_flags: HashSet<String>,
}

impl EdgeTracker {
    /// 登记第 5 类信号（`StationFlag`）的事件名（幂等；每轮调用无副作用）。
    ///
    /// 它们**首次观测即产**（进入）——这是"首轮/站恢复后首轮只建基线、不产事件"的
    /// **唯一例外**（§11.4.7.2 C）：① 至多 1 条/站，不构成风暴；② 若不产，
    /// "上线时地址序就已经错了""SOC 一直在域外"这类**无翻转点**的状态将**永久静默**。
    fn mark_station_flags(&mut self, flags: &[StationFlag]) {
        self.station_flags
            .extend(flags.iter().map(|f| f.metric.to_string()));
    }

    /// 建基线：把本轮的活跃态记为基线并**清空旧记忆**（首轮、站恢复后调用）。
    pub fn prime(&mut self, now: &[(String, bool)]) {
        self.last.clear();
        for (k, v) in now {
            self.last.insert(k.clone(), *v);
        }
        self.primed = true;
    }

    /// 丢弃基线（下一个 `edges()` 退化为"只建基线、不产事件"）。
    pub fn reset(&mut self) {
        self.last.clear();
        self.primed = false;
    }

    /// 返回本轮的变化沿事件 `(metric, value, is_event=true)`；首轮/复位后首轮 → 空（只建基线）。
    ///
    /// 未见过的新信号（上轮无记忆）**不产事件**（"未知 → X" 不是跃迁），但会记入基线。
    /// **例外**：已登记的 `StationFlag` 活跃时首轮即产（见 [`Self::mark_station_flags`]）。
    pub fn edges(&mut self, now: &[(String, bool)]) -> Vec<(String, f64, bool)> {
        if !self.primed {
            // 首轮 / 站恢复后首轮：只建基线、不产事件 —— 第 5 类信号除外（"首次观测即产"）
            let first: Vec<(String, f64, bool)> = now
                .iter()
                .filter(|(k, v)| *v && self.station_flags.contains(k))
                .map(|(k, _)| (k.clone(), 1.0, true))
                .collect();
            self.prime(now);
            return first;
        }
        let mut out = Vec::new();
        for (k, v) in now {
            match self.last.get(k) {
                Some(prev) if prev != v => {
                    out.push((k.clone(), if *v { 1.0 } else { 0.0 }, true));
                }
                _ => {}
            }
        }
        self.prime(now); // 本轮活跃态成为下一轮基线
        out
    }
}

/// 口运行时：bus（open 失败 → None，该口全站 offline）+ 本口独立 DueCalc + 各站变化沿记忆。
struct PortRunner {
    bus: Option<Arc<dyn StationBus>>,
    calc: std::sync::Mutex<DueCalc>,
    /// 站下标 → 该站的变化沿记忆（离散位 + 字级信号共用，§11.4.7.1）
    trackers: std::sync::Mutex<HashMap<usize, EdgeTracker>>,
}

/// 本轮参与变化沿检测的信号全集 + 分类（判据见 §11.4.7.1 的信号形态表）。
#[derive(Default)]
struct RoundSignals {
    /// 全部信号 `(信号名, 是否活跃)` —— 喂 [`EdgeTracker`]
    all: Vec<(String, bool)>,
    /// 其中"离散位点"的信号名集合（这些信号**只取 0→1 上升沿**）
    bits: HashSet<String>,
    /// 其中"离散告警位"（`BitClass::Alarm`）的信号名集合（`State`/`Reserved` 只落 telemetry）
    alarm_bits: HashSet<String>,
    /// 其中"第 5 类站级派生布尔量"（`StationFlag`，`all` 里对应条目的诊断量来源）
    station_flags: Vec<StationFlag>,
}

/// 汇总一站本轮的信号：离散位点（FC02 位向量逐点）+ 字级信号（消防整字 `字 & mask` / `字 ∈ active`）
/// + 第 5 类站级派生布尔量（`StationFlag`：判据是 mapper 的交叉校验布尔返回，无寄存器）。
///
/// **三类信号共用同一条产出路径**（`EdgeTracker`），差别只在活跃判据的求值处与
/// （`StationFlag` 独有）**产出频次口径**：位点的活跃 = 位向量第 k 位；字级信号的活跃 =
/// `SignalPick::is_active(整字)`；站级量的活跃 = 判据函数 `is_some()`。
fn round_signals(role: Role, reads: &BlockReads) -> RoundSignals {
    let mut rs = RoundSignals::default();
    for (blk, res) in reads {
        let Ok(data) = res else { continue };
        match data {
            BlockData::Bits(bits) => {
                let Ok(pts) = points::expand(blk) else {
                    continue;
                };
                for p in pts {
                    let PointKind::Bit { offset } = p.kind else {
                        continue;
                    };
                    // 位地址 = 块位地址 + 块内位偏移（**位空间**，与寄存器空间各自编址）
                    let class = match point_table::lookup_bit(role, blk.addr.wrapping_add(offset)) {
                        Some(row) => match row.kind {
                            RegPointKind::Bit(c) => Some(c),
                            RegPointKind::Scalar(_) => None,
                        },
                        None => None, // 查不到登记行：不产事件（只落 telemetry）
                    };
                    if class == Some(BitClass::Alarm) {
                        rs.alarm_bits.insert(p.metric.clone());
                    }
                    rs.bits.insert(p.metric.clone());
                    let active = bits.get(offset as usize).copied().unwrap_or(false);
                    rs.all.push((p.metric, active));
                }
            }
            BlockData::Regs(regs) => {
                rs.all.extend(points::signals_of_block(role, blk, regs));
            }
        }
    }
    // ── 第 5 类信号 `StationFlag`（§11.4.7.1 末行 / §11.4.7.2 C）──
    // 消防探测器地址升序违规（Q-9）：判据是 mapper 的交叉校验返回，**判据本身不是跃迁量**
    // ⇒ 按其布尔态喂进同一个 EdgeTracker（产出频次 = 状态翻转，见 `station_flag_events`）。
    // 非 fire 站无此判据（`mapper` 对非 fire 直接返回 None）⇒ 不喂信号、不占记忆格。
    if role == Role::Fire {
        let violation = mapper::fire_detector_addr_order_violation(role, reads);
        rs.station_flags.push(StationFlag {
            metric: ADDR_ORDER_INVALID,
            active: violation.is_some(),
            diag: violation.map(|(group, _addr)| group as f64).unwrap_or(0.0),
        });
    }
    // 站级量与前四类**同栏**喂进 tracker（"喂进同一个 `RoundSignals.all`"，§11.4.7.2 C）
    rs.all.extend(
        rs.station_flags
            .iter()
            .map(|f| (f.metric.to_string(), f.active)),
    );
    rs
}

/// 变化沿 → 事件（**不对称过滤**，§11.4.7.1 的刻意设计）：
/// - 离散位点（`Bit`）：**仅 0→1 上升沿**，且**仅 `BitClass::Alarm`**（`State`/`Reserved` 只落 telemetry）；
/// - 字级信号（`WordBit`/`WordEnum`，消防）：**进入与退出都产事件** —— 消防是联锁判据源
///   （§10.4"恢复需双方复位"），只有上升沿会让"消防侧已解除"永远不可观测。
fn edges_to_events(rs: &RoundSignals, edges: Vec<(String, f64, bool)>) -> Vec<(String, f64, bool)> {
    edges
        .into_iter()
        .filter(|(metric, value, _)| {
            if rs.bits.contains(metric) {
                // 位点：只上升沿 + 只告警位（其余类别只落 telemetry，不进 events）
                *value == 1.0 && rs.alarm_bits.contains(metric)
            } else {
                true // 字级信号：双向
            }
        })
        .collect()
}

/// 第 5 类信号（`StationFlag`）事件的**后处理**（§11.4.7.2 C 的 `value` 约定，进入/退出正交）：
/// - **进入**（变化沿 `value = 1.0`）⇒ `value` 替换为**诊断量**（⑤ = 首个违规组的 1 基序号）
///   —— 诊断量只能由本轮的判据给出（[`StationFlag::diag`]），[`EdgeTracker`] 只知布尔；
/// - **退出**（`value = 0.0`）⇒ metric 追加 **`@recovered`**、`value` 恒 **`0.0`**（哨兵）。
///
/// **为什么把"状态"放在事件名而不是 `value`**：诊断量本身可以取 0（如③读回 0 只）⇒ 单靠
/// `value` 无法同时承载"诊断量"与"进入/退出"两维；`@recovered` 后缀让两者正交、消费方零
/// 歧义（它仍在事件命名空间内，`metric` 不对应任何遥测点 —— §11.4.7.2 C）。
///
/// **安全红线**（§11.4.7.2 E）：本类事件是**数据可信性告警**，**不得并入 §10.4 的联锁
/// OR 触发**（否则"配错地址"会升级成消防停机）。本函数只改事件流的名字与 `value`，
/// 不触碰任何判据/联锁/telemetry 路径；§11.7.2 的"每轮点位不可信"标记**照旧逐轮生效**。
fn station_flag_events(
    flags: &[StationFlag],
    edges: Vec<(String, f64, bool)>,
) -> Vec<(String, f64, bool)> {
    edges
        .into_iter()
        .map(|(metric, value, is_event)| {
            match flags.iter().find(|f| f.metric == metric) {
                // 进入活跃：`value` 换成诊断量（原值 1.0 只是 EdgeTracker 的布尔占位）
                Some(f) if value != 0.0 => (metric, f.diag, is_event),
                // 恢复：改名 `@recovered` + 哨兵 0.0
                Some(_) => (format!("{metric}@recovered"), 0.0, is_event),
                None => (metric, value, is_event), // 前四类信号：原样
            }
        })
        .collect()
}

/// 口级调度器：每 port 一条采集 task（间隔 `poll_ms` tick），口内多站按 next_due 串行轮询。
///
/// `state` = 全站调度态（`Arc<RwLock<Vec<Station>>>`，state Vec 序即 cfg.stations 序）；
/// 站调度态在 poll 完成后用单锁快进快出更新（勿持锁跨 await）。DueCalc 纯逻辑不碰 state。
pub struct SouthScheduler {
    cfg: SouthStationsConfig,
    state: Arc<std::sync::RwLock<Vec<Station>>>,
    runners: Vec<PortRunner>,
    sink: Arc<dyn StationSink>,
}

impl SouthScheduler {
    /// 构造：buses 按 port 注入（真实场景只含成功 open 的口；Rs485PortBus::open 失败的口
    /// 不入 map——该口全站 offline，§10.7）。cfg 中出现的口都会建 runner；口不在 buses 中
    /// → runner.bus=None，其站每次 tick 走 offline 路径（首轮告警一次，窗口内防刷屏）。
    /// 该站 poll 恒 false，offline_count 随轮累加 → M-11 退避同样生效随退避降频（与真失败站
    /// 同策，§10.2：只告警/探测不刷流量——bus 缺失多因口硬件未接，慢探测亦无害）。
    pub fn new(
        cfg: SouthStationsConfig,
        buses: HashMap<String, Arc<dyn StationBus>>,
        sink: Arc<dyn StationSink>,
    ) -> Arc<Self> {
        let state = Arc::new(std::sync::RwLock::new(
            cfg.stations.iter().cloned().map(Station::from_conf).collect(),
        ));
        // 按 port 分组（保留 cfg 首现序；state 下标 = cfg 序）
        let mut port_order: Vec<String> = Vec::new();
        let mut groups: HashMap<String, Vec<(usize, &StationConf)>> = HashMap::new();
        for (i, c) in cfg.stations.iter().enumerate() {
            let g = groups.entry(c.port.clone()).or_insert_with(|| {
                port_order.push(c.port.clone());
                Vec::new()
            });
            g.push((i, c));
        }
        let mut runners = Vec::with_capacity(port_order.len());
        for port in &port_order {
            let group = groups.remove(port).expect("port_order/group 键应一致");
            runners.push(PortRunner {
                bus: buses.get(port).cloned(),
                calc: std::sync::Mutex::new(DueCalc::from_group(&group)),
                trackers: std::sync::Mutex::new(HashMap::new()),
            });
        }
        Arc::new(Self {
            cfg,
            state,
            runners,
            sink,
        })
    }

    /// 启动：每口 spawn 一条采集 task（poll_ms tick 循环，自 now 起算 uptime 驱动 due），
    /// 直至返回的 JoinHandle 被 abort。口间并发；口内串行。
    ///
    /// 返回的每个 `JoinHandle` **调用方必须持有并观测**：task 内任何 panic 都会静默终止
    /// 该口采集（无自动重 spawn）。观测方式：定期查 `is_finished()`，或 `await` 返回 `Err`
    /// 时记 error / 重建该口 task。core-bin（Task 7）接线时落实实际观测与重建。
    pub fn spawn(self: &Arc<Self>) -> Vec<tokio::task::JoinHandle<()>> {
        let poll_ms = self.cfg.poll_ms.max(1);
        let mut handles = Vec::with_capacity(self.runners.len());
        for port_i in 0..self.runners.len() {
            let me = Arc::clone(self);
            handles.push(tokio::spawn(async move {
                let origin = std::time::Instant::now();
                loop {
                    let now = origin.elapsed().as_millis() as u64;
                    me.run_port_round(port_i, now).await;
                    tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;
                }
            }));
        }
        handles
    }

    /// 跑一口的一个 tick（now_ms 驱动 due → 逐到期站 poll → 失败站退避）。
    async fn run_port_round(&self, port_i: usize, now_ms: u64) {
        let runner = &self.runners[port_i];
        let due = runner.calc.lock().unwrap().due_round(now_ms);
        if due.is_empty() {
            return;
        }
        let mut failed: Vec<usize> = Vec::new();
        for poll in due {
            let ok = self.poll_station(runner, poll.station_index).await;
            if !ok {
                failed.push(poll.station_index);
            }
        }
        // M-11：失败站退避（读 state 的 offline_count 已由 handle_failure +1）。
        // 先一次锁 state 快取失败站 (interval_ms, offline_count)，勿持 state 锁跨 calc 锁。
        if !failed.is_empty() {
            let stats: Vec<(usize, u64, u32)> = {
                let st = self.state.read().unwrap();
                failed
                    .iter()
                    .map(|&i| {
                        let s = &st[i];
                        (i, s.conf.interval_ms, s.offline_count)
                    })
                    .collect()
            };
            let mut calc = runner.calc.lock().unwrap();
            for (idx, interval, oc) in stats {
                // 退避公式抽离为 backoff_extra（纯函数，封顶/边界见 tests 单测）。
                // config::validate 已强校验 interval_ms>0 且此处 oc≥1 → extra 恒 > 0，
                // 原 `if extra > 0` 守卫冗余已去（delay_station 不空转）。
                calc.delay_station(idx, now_ms, backoff_extra(interval, oc));
            }
        }
    }

    /// 单站一轮采集：逐 regs 块读 → mapper 判定 → 分发 + 调度态更新。
    ///
    /// 任一块读 Err（物理层）或 mapper `PollResult::Failed`（语义层）→ 站失败（offline 记账 +
    /// 事件；§10.7 对两层失败隔离语义一致——该站本轮无有效数据）。全块 Ok 且 mapper Data
    /// → 站成功（恢复事件 + 按 role 分发）。同口串行由口 task 单 poller 保证（本方法不并发）。
    ///
    /// 返回本轮是否成功（站数据可用）。false = 失败（offline 记账 + 事件已由内部处理；
    /// 调用方据此退避该站，§10.2 M-11）。
    async fn poll_station(&self, runner: &PortRunner, station_index: usize) -> bool {
        let bus = runner.bus.clone();
        let (station_id, role, slave, regs) = {
            let st = self.state.read().unwrap();
            let s = &st[station_index];
            (
                s.conf.id.clone(),
                s.conf.role,
                s.conf.slave,
                s.conf.regs.clone(),
            )
        };

        // 逐 regs 块读（阻塞 IO 由 StationBus 内 spawn_blocking 承载——全 async 无阻塞）。
        let mut reads: BlockReads = Vec::with_capacity(regs.len());
        let mut io_error: Option<String> = None;
        if let Some(b) = &bus {
            for blk in &regs {
                // 按块 func 分发读方法：Holding → FC03 read_holding，Input → FC04 read_input，
                // Discrete → FC02 read_discrete（S3b-2 T5 接通 `StationBus::read_discrete`；
                // T5 之前该块读被显式判失败——配置了 discrete 块的站在运行期 offline 而不静默无数据）。
                // 读结果按 `BlockData` 统一承载（寄存器块 = Regs / 位块 = Bits），见 `mapper::BlockData`。
                let res: Result<mapper::BlockData, BusError> = match blk.func {
                    RegFunc::Holding => b
                        .read_holding(slave, blk.addr, blk.count)
                        .await
                        .map(mapper::BlockData::Regs),
                    RegFunc::Input => b
                        .read_input(slave, blk.addr, blk.count)
                        .await
                        .map(mapper::BlockData::Regs),
                    RegFunc::Discrete => b
                        .read_discrete(slave, blk.addr, blk.count)
                        .await
                        .map(mapper::BlockData::Bits),
                };
                match res {
                    Ok(data) => reads.push((blk.clone(), Ok(data))),
                    Err(e) => {
                        // 站失败语义（§10.7）：任一块读失败 → 整站 offline，本轮无有效数据，
                        // 不部分交付——已读 Ok 块随失败路径整体弃用（io_error 即返回，reads 丢弃）。
                        // 首块错误即 break → 钳制同口 cadence 受损上界（不为该站耗尽本轮预算，
                        // 尽快回到同口其它站）。故 `reads` 从不带 Err 条目进 mapper/telemetry_points
                        // ——mapper 里跳过 Err 块的分支为「多块站扩展时部分交付」预留，与模块头
                        // 「同站单写方、整站 offline 隔离」语义一致。
                        io_error = Some(format!("{} @ {:#06x} x{}", e, blk.addr, blk.count));
                        break;
                    }
                }
            }
        } else {
            io_error = Some("口未打开（open 失败）".into());
        }

        if let Some(reason) = io_error {
            self.handle_failure(station_index, &reason).await;
            return false;
        }

        // 全块读 Ok → mapper 语义判定（PollResult::Failed = 语义层失败，同 offline 处理）。
        match mapper::poll_to_result(role, &reads) {
            PollResult::Failed(msg) => {
                self.handle_failure(station_index, &msg).await;
                false
            }
            PollResult::Data(pkg) => {
                // 恢复判定须在 `mark_success` 清零 offline_count **之前**取：站恢复后变化沿基线
                // 必须重建（现势值连续性不可假设），否则"恢复即刷一屏事件"（§11.4.7）。
                let recovered = { self.state.read().unwrap()[station_index].offline_count > 0 };
                self.mark_success(station_index).await;
                if role == Role::MeterGrid {
                    self.sink.on_grid_package(pkg).await;
                } else {
                    // 非 grid（battery/hvac/fire/meter_batt/pcs）遥测全量落库（is_event=false）。
                    let pts: Vec<(String, f64, bool)> = mapper::telemetry_points(&reads)
                        .into_iter()
                        .map(|(m, v)| (m, v, false))
                        .collect();
                    if !pts.is_empty() {
                        self.sink.on_station_telemetry(&station_id, role, pts).await;
                    }
                    // ── 事件侧（§11.4.7.1 统一事件模型）：离散位 + 字级信号 + 第 5 类站级量
                    //（`StationFlag`）**共用同一 EdgeTracker 与同一条产出路径** ──
                    let signals = round_signals(role, &reads);
                    let events = {
                        let mut trackers = runner.trackers.lock().unwrap();
                        let tracker = trackers.entry(station_index).or_default();
                        if recovered {
                            tracker.reset(); // 恢复后首轮只重建基线（`StationFlag` 仍"首次观测即产"）
                        }
                        tracker.mark_station_flags(&signals.station_flags);
                        let edges = edges_to_events(&signals, tracker.edges(&signals.all));
                        // 站级量：进入事件的 value 换诊断量、退出事件改名 `@recovered`（§11.4.7.2 C）
                        station_flag_events(&signals.station_flags, edges)
                    }; // 锁在 await 前释放（勿持锁跨 await）
                    if !events.is_empty() {
                        self.sink
                            .on_station_telemetry(&station_id, role, events)
                            .await;
                    }
                    // SOC 双源通道（04 §2.11.1）：battery 站 pkg.battery.soc 由 mapper 解码；
                    // 本轮采集成功即新鲜 → 独立推给 AiIntegrator（BMS 优先源）。telemetry 落库照旧。
                    if role == Role::Battery {
                        if let Some(soc) = pkg.battery.soc {
                            self.sink.on_battery_soc(&station_id, soc).await;
                        }
                    }
                }
                true
            }
        }
    }

    /// 站失败记账 + 事件（锁内快进快出，勿持锁跨 await）。offline 事件按 stale_timeout_s
    /// 窗口去抖（防刷屏；首次失败立即告警一次）。返回前已释放锁；事件经 sink 异步上送。
    async fn handle_failure(&self, station_index: usize, reason: &str) {
        let (emit, id, role) = {
            let mut st = self.state.write().unwrap();
            let s = &mut st[station_index];
            // saturating：防 u32 极端回绕归零误判恢复（漏 online 事件，§10.7 状态机不破）
            s.offline_count = s.offline_count.saturating_add(1);
            let now = Utc::now();
            let emit = match s.last_offline_event {
                None => true, // 首次失败立即记一次
                Some(prev) => {
                    (now.signed_duration_since(prev).num_seconds())
                        >= self.cfg.stale_timeout_s as i64
                }
            };
            if emit {
                s.last_offline_event = Some(now);
            }
            (emit, s.conf.id.clone(), s.conf.role)
        };
        if emit {
            tracing::warn!(station = %id, ?role, reason, "southd 站采集失败（offline 隔离）");
            // reason 含 `slave/addr/count`（PRD §9.7.2 第 1 条）；经 `on_station_offline`
            // 交出去，使消费层可把它写进事件 message（默认实现 = 既有 `offline` 事件）。
            self.sink.on_station_offline(&id, role, reason).await;
        }
    }

    /// 站本轮成功：offline_count 清零、last_ok 刷新；此前在 offline（offline_count>0）
    /// → 恢复（online 事件一次），并复位 last_offline_event（下次 offline 重新即时告警）。
    async fn mark_success(&self, station_index: usize) {
        let (recovered, id, role) = {
            let mut st = self.state.write().unwrap();
            let s = &mut st[station_index];
            let recovered = s.offline_count > 0;
            s.offline_count = 0;
            s.last_ok = Some(Utc::now());
            s.last_offline_event = None;
            (recovered, s.conf.id.clone(), s.conf.role)
        };
        if recovered {
            tracing::info!(station = %id, ?role, "southd 站恢复上线");
            self.sink
                .on_station_telemetry(&id, role, vec![("online".to_string(), 1.0, true)])
                .await;
        }
    }

    /// 测试驱动：以同一 now_ms 扫过所有口的 due 并逐 poll（真实 spawn 的每口 loop 内也调
    /// run_port_round；本方法仅测试用，避免触真时钟/无限 loop）。
    #[cfg(test)]
    async fn tick_once(&self, now_ms: u64) {
        for port_i in 0..self.runners.len() {
            self.run_port_round(port_i, now_ms).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PointConf, StationParity, DEFAULT_BAUD_RATE, RegBlockConf, RegFunc};
    use crate::port_runtime::MockBus;
    use mupc_data_processing::meter_regs::{RegFormat, WordOrder};

    // ---------- 测试构件 ----------

    fn blk(name: &str, addr: u16, count: u16) -> RegBlockConf {
        RegBlockConf {
            name: name.into(),
            addr,
            func: RegFunc::Holding,
            format: RegFormat::Float32,
            scale: 0.0,
            count,
            offset: 0.0,
            byte_swap: false,
            points: Vec::new(),
            read_slice: false,
        }
    }

    /// f32 → 大端 u16 寄存器对（高字在前；与 meter_regs regs_to_f32_be 一致）
    fn f32_regs(v: f32) -> Vec<u16> {
        let b = v.to_bits();
        vec![(b >> 16) as u16, b as u16]
    }

    /// 三相 float32 → 6 寄存器
    fn phase_regs(a: f32, b: f32, c: f32) -> Vec<u16> {
        [f32_regs(a), f32_regs(b), f32_regs(c)].concat()
    }

    fn grid_conf(id: &str, port: &str, slave: u8, interval_ms: u64) -> StationConf {
        StationConf {
            id: id.into(),
            role: Role::MeterGrid,
            port: port.into(),
            protocol: "modbus".into(),
            slave,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms,
            regs: vec![
                blk("p", 0, 6),
                blk("p_total", 6, 2),
                blk("q", 8, 6),
                blk("pf", 14, 6),
                blk("u", 20, 6),
                blk("i", 26, 6),
            ],
        }
    }

    /// battery 站：**点名式** `soc` 点（float32，slave 2，地址 100）——mapper Battery 分支
    /// 按**点名**查找并 decode 进 pkg.battery.soc；telemetry 与独立 SOC 通道都用它。
    /// 单测成功路径值 65.5（f32 精确）。
    fn battery_conf() -> StationConf {
        StationConf {
            id: "bms".into(),
            role: Role::Battery,
            port: "ttyS2".into(),
            protocol: "modbus".into(),
            slave: 2,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![RegBlockConf {
                name: "bms_io".into(),
                addr: 100,
                func: RegFunc::Holding,
                format: RegFormat::Float32,
                scale: 1.0,
                count: 2,
                offset: 0.0,
                byte_swap: false,
                points: vec![PointConf {
                    at: 1,
                    count: 1,
                    name: Some("soc".into()),
                    format: None,
                    scale: None,
                    offset: None,
                    word_order: WordOrder::HiLo,
                }],
                read_slice: false,
            }],
        }
    }

    /// 位块（FC02）构造：`addr` = **位地址**，`count` = **位数**。
    fn dblk(name: &str, addr: u16, count: u16) -> RegBlockConf {
        RegBlockConf {
            name: name.into(),
            addr,
            func: RegFunc::Discrete,
            format: RegFormat::Uint16, // discrete 块的 format/scale 不参与（位恒 0/1）
            scale: 0.0,
            count,
            offset: 0.0,
            byte_swap: false,
            points: Vec::new(),
            read_slice: false,
        }
    }

    fn hvac_conf(id: &str, port: &str, slave: u8, interval_ms: u64) -> StationConf {
        StationConf {
            id: id.into(),
            role: Role::Hvac,
            port: port.into(),
            protocol: "modbus".into(),
            slave,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms,
            regs: vec![blk("temp", 100, 2)],
        }
    }

    fn cfg(stations: Vec<StationConf>, stale_timeout_s: u64) -> SouthStationsConfig {
        SouthStationsConfig {
            poll_ms: 1000,
            stale_timeout_s,
            stations,
        }
    }

    /// 预置 meter_grid 完整分相寄存器（值选 f32 可精确表示；测值同 mapper 回归锚）。
    fn put_grid(bus: &MockBus, slave: u8) {
        bus.put(slave, 0, phase_regs(1.0, 2.0, 3.0));
        bus.put(slave, 6, f32_regs(5.5));
        bus.put(slave, 8, phase_regs(0.5, 0.25, 0.125));
        bus.put(slave, 14, phase_regs(0.75, 0.75, 0.75));
        bus.put(slave, 20, phase_regs(220.0, 221.0, 222.0));
        bus.put(slave, 26, phase_regs(10.0, 11.0, 12.0));
    }

    /// 假 Sink：记录 on_grid_package / on_station_telemetry（事件与普通遥测以 is_event 区分）
    /// 与 on_battery_soc（独立 SOC 通道）。
    #[derive(Default)]
    struct FakeSink {
        grid_pkgs: std::sync::Mutex<Vec<mupc_data_processing::DataPackage>>,
        msgs: std::sync::Mutex<Vec<(String, Role, Vec<(String, f64, bool)>)>>,
        battery_socs: std::sync::Mutex<Vec<(String, f64)>>,
        /// 覆写 `on_station_offline` 收到的 `reason`（PRD §9.7.2 第 1 条的落证）
        offline_reasons: std::sync::Mutex<Vec<(String, String)>>,
    }

    impl FakeSink {
        fn grid_count(&self) -> usize {
            self.grid_pkgs.lock().unwrap().len()
        }
        fn grid_pkg(&self) -> mupc_data_processing::DataPackage {
            self.grid_pkgs.lock().unwrap()[0].clone()
        }
        /// station 的普通遥测点（is_event=false）
        fn telemetry_of(&self, station_id: &str) -> Vec<(String, f64)> {
            self.msgs
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _, pts)| id == station_id && pts.iter().any(|&(_, _, ev)| !ev))
                .flat_map(|(_, _, pts)| pts.iter().filter(|&&(_, _, ev)| !ev).map(|(m, v, _)| (m.clone(), *v)).collect::<Vec<_>>())
                .collect()
        }
        /// battery 站最近一次 on_battery_soc 推的 soc（无推送 → None）
        fn soc_of(&self, station_id: &str) -> Option<f64> {
            self.battery_socs
                .lock()
                .unwrap()
                .iter()
                .rev()
                .find(|(id, _)| id == station_id)
                .map(|(_, v)| *v)
        }
        /// station 的全部事件点（`is_event=true`）按发生序 —— 含 `offline`/`online` 状态事件
        /// 与位/信号变化沿事件；`(metric, value)`。
        fn events_of(&self, station_id: &str) -> Vec<(String, f64)> {
            self.msgs
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _, _)| id == station_id)
                .flat_map(|(_, _, pts)| {
                    pts.iter()
                        .filter(|&&(_, _, ev)| ev)
                        .map(|(m, v, _)| (m.clone(), *v))
                        .collect::<Vec<_>>()
                })
                .collect()
        }

        /// 自 `from`（= 上一次 `events_of(..).len()`）起的新增事件 —— 按轮切片的断言用。
        fn events_since(&self, station_id: &str, from: usize) -> Vec<(String, f64)> {
            self.events_of(station_id)[from..].to_vec()
        }

        /// station 的状态事件计数（metric ∈ offline/online，is_event=true）
        fn event_count(&self, station_id: &str, metric: &str) -> usize {
            self.msgs
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _, _pts)| id == station_id)
                .flat_map(|(_, _, pts)| pts.iter())
                .filter(|&&(ref m, _, ev)| ev && m == metric)
                .count()
        }
    }

    #[async_trait]
    impl StationSink for FakeSink {
        async fn on_grid_package(&self, pkg: mupc_data_processing::DataPackage) {
            self.grid_pkgs.lock().unwrap().push(pkg);
        }
        async fn on_station_telemetry(
            &self,
            station_id: &str,
            role: Role,
            points: Vec<(String, f64, bool)>,
        ) {
            self.msgs
                .lock()
                .unwrap()
                .push((station_id.to_string(), role, points));
        }
        async fn on_battery_soc(&self, station_id: &str, soc: f64) {
            self.battery_socs
                .lock()
                .unwrap()
                .push((station_id.to_string(), soc));
        }
        /// 覆写默认实现：记下 `reason`，并**保持默认实现的既有事件**（两条都要有，
        /// 否则本用例无法同时验证"事件不变"与"reason 已交出"）。
        async fn on_station_offline(&self, station_id: &str, role: Role, reason: &str) {
            self.offline_reasons
                .lock()
                .unwrap()
                .push((station_id.to_string(), reason.to_string()));
            self.on_station_telemetry(station_id, role, vec![("offline".to_string(), 1.0, true)])
                .await;
        }
    }

    fn build(
        stations: Vec<StationConf>,
        bus: Arc<MockBus>,
        sink: Arc<FakeSink>,
    ) -> Arc<SouthScheduler> {
        let mut buses: HashMap<String, Arc<dyn StationBus>> = HashMap::new();
        // 本批站若同口，注入口为 bus（cfg 组测多口场景时可扩展，此处单口够用）
        let port = stations[0].port.clone();
        buses.insert(port, bus as Arc<dyn StationBus>);
        SouthScheduler::new(cfg(stations, 5), buses, sink)
    }

    // ---------- 测例 ----------

    /// 同口两站（grid 快 interval=1000 / hvac 慢 interval=5000）按 due 排程：
    /// grid 每轮（0/1000/2000）都采；hvac 仅首轮（next_due=0）采，此后 5000 才到期。
    #[tokio::test]
    async fn two_stations_same_port_schedules_by_due() {
        let bus = Arc::new(MockBus::new());
        put_grid(&bus, 1);
        bus.put(3, 100, f32_regs(23.5));
        let sink = Arc::new(FakeSink::default());
        let sched = build(
            vec![
                grid_conf("grid", "ttyS1", 1, 1000),
                hvac_conf("hvac", "ttyS1", 3, 5000),
            ],
            bus.clone(),
            sink.clone(),
        );

        sched.tick_once(0).await;
        sched.tick_once(1000).await;
        sched.tick_once(2000).await;

        // grid：每轮 due → p 块读到 3 次
        assert_eq!(bus.call_count(1, 0), 3, "grid 应每轮到期");
        assert_eq!(bus.call_count(1, 26), 3);
        // hvac：仅首轮（next_due=0 启动即采）；1000/2000 未到期
        assert_eq!(bus.call_count(3, 100), 1, "hvac interval=5000 首轮后应隔 5000 才到期");
        // sink：grid 每轮 on_grid_package；hvac 一次 telemetry（非事件）
        assert_eq!(sink.grid_count(), 3);
        assert_eq!(sink.telemetry_of("hvac"), vec![("temp_1".to_string(), 23.5)]);
        assert_eq!(sink.event_count("grid", "offline"), 0);
        assert_eq!(sink.event_count("hvac", "offline"), 0);
        assert_eq!(sink.event_count("grid", "online"), 0);
    }

    /// 同口隔离：grid 读失败（offline 事件一次，不上送 pkg）→ hvac 本轮照常采（telemetry 非事件）。
    #[tokio::test]
    async fn grid_offline_isolated_hvac_continues() {
        let bus = Arc::new(MockBus::new());
        put_grid(&bus, 1);
        bus.put(3, 100, f32_regs(23.5));
        bus.fail_once(1, 0); // grid 的 p 块一次超时
        let sink = Arc::new(FakeSink::default());
        let sched = build(
            vec![
                grid_conf("grid", "ttyS1", 1, 1000),
                hvac_conf("hvac", "ttyS1", 3, 5000),
            ],
            bus.clone(),
            sink.clone(),
        );

        sched.tick_once(0).await;

        // grid offline 事件一次；无 pkg 上送（沿用旧数据）
        assert_eq!(sink.event_count("grid", "offline"), 1);
        assert_eq!(sink.grid_count(), 0);
        // 同口 hvac 不受隔离影响：仍读到并上送普通遥测
        assert_eq!(bus.call_count(3, 100), 1);
        assert_eq!(sink.telemetry_of("hvac"), vec![("temp_1".to_string(), 23.5)]);
        assert_eq!(sink.event_count("hvac", "offline"), 0);
    }

    /// 失败后恢复：tick(0) 读失败 → offline 事件；tick(1000) 读恢复 → online 事件 + 正常遥测。
    #[tokio::test]
    async fn station_recovers_after_failure() {
        let bus = Arc::new(MockBus::new());
        bus.put(3, 100, f32_regs(23.5));
        bus.fail_once(3, 100);
        let sink = Arc::new(FakeSink::default());
        let sched = build(
            vec![hvac_conf("hvac", "ttyS1", 3, 1000)],
            bus.clone(),
            sink.clone(),
        );

        sched.tick_once(0).await; // 失败
        assert_eq!(sink.event_count("hvac", "offline"), 1);
        assert!(sink.telemetry_of("hvac").is_empty());

        sched.tick_once(1000).await; // 恢复（fail 已消费）
        assert_eq!(sink.event_count("hvac", "online"), 1);
        assert_eq!(sink.telemetry_of("hvac"), vec![("temp_1".to_string(), 23.5)]);
    }

    /// 多轮退避（oc≥3，next_due 已推远）后恢复：probe 在退避到期点成功 → oc 归零、
    /// online 事件 1、cadence 回到 interval 正常节奏。补 `station_recovers_after_failure`
    /// （仅 oc=1，退避未推远，恢复与正常 cadence 无异）未覆盖的「持续失败退避 → 恢复 →
    /// 归零 + 正常 cadence」模块头核心承诺。
    #[tokio::test]
    async fn station_recovers_after_extended_backoff() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_conf("hvac", "ttyS1", 3, 1000)], bus.clone(), sink.clone());
        // 三轮失败：0 → oc=1 next_due 1000；1000 → oc=2 next_due 3000；3000 → oc=3 next_due 7000
        sched.tick_once(0).await;
        sched.tick_once(1000).await;
        sched.tick_once(3000).await;
        {
            let st = sched.state.read().unwrap();
            assert_eq!(st[0].offline_count, 3);
        }
        // 2000 被退避跳过验证（已由 offline_station_backs_off 覆盖，此处不重复）
        // 恢复前基线：三轮失败尝试读（0/1000/3000）→ call_count 累积含失败尝试（MockBus
        // 每次读都记账，与 offline_station_backs_off 断言口径一致）。以基线增量断言恢复后
        // cadence：7000 恢复读 + 8000 正常到期读 = 2；若退避残留把 next_due 推过 8000，
        // 则 8000 轮不读 → 增量仅 1，断言区分成立。
        let baseline = bus.call_count(3, 100); // = 3（三轮失败尝试）
        // 恢复：put 预置 → 7000 到期 probe 成功
        bus.put(3, 100, f32_regs(23.5));
        sched.tick_once(7000).await;
        {
            let st = sched.state.read().unwrap();
            assert_eq!(st[0].offline_count, 0, "恢复后 oc 归零");
            assert_eq!(sink.event_count("hvac", "online"), 1);
        }
        assert_eq!(sink.telemetry_of("hvac"), vec![("temp_1".to_string(), 23.5)]);
        // 恢复后 cadence 正常：next_due=8000，8000 到期再采一次（不因退避残留再跳）
        sched.tick_once(8000).await;
        assert_eq!(
            bus.call_count(3, 100) - baseline,
            2,
            "恢复后按 interval 正常 cadence（7000 恢复读 + 8000 正常到期）"
        );
    }

    /// 单站混合 FC03+FC04 块：按 func 分发读（Holding→read_holding、Input→read_input），
    /// telemetry 全量落库（两 metric），无 offline 事件。
    #[tokio::test]
    async fn station_mixed_holding_and_input_blocks() {
        let bus = Arc::new(MockBus::new());
        bus.put(3, 100, f32_regs(23.5)); // holding: temp
        bus.put_input(3, 200, f32_regs(0.0)); // input: alarm_in（值 0.0）
        let sink = Arc::new(FakeSink::default());
        let st = StationConf {
            id: "mix".into(),
            role: Role::Hvac,
            port: "ttyS1".into(),
            protocol: "modbus".into(),
            slave: 3,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![
                RegBlockConf {
                    name: "temp".into(),
                    addr: 100,
                    func: RegFunc::Holding,
                    format: RegFormat::Float32,
                    scale: 0.0,
                    count: 2,
                    offset: 0.0,
                    byte_swap: false,
                    points: Vec::new(),
                    read_slice: false,
                },
                RegBlockConf {
                    name: "alarm_in".into(),
                    addr: 200,
                    func: RegFunc::Input,
                    format: RegFormat::Float32,
                    scale: 0.0,
                    count: 2,
                    offset: 0.0,
                    byte_swap: false,
                    points: Vec::new(),
                    read_slice: false,
                },
            ],
        };
        let sched = build(vec![st], bus.clone(), sink.clone());
        sched.tick_once(0).await;
        assert_eq!(bus.call_count(3, 100), 1, "holding 块应被 FC03 读");
        assert_eq!(bus.input_call_count(3, 200), 1, "input 块应被 FC04 读");
        let tel = sink.telemetry_of("mix");
        assert!(tel.iter().any(|(m, _)| m == "temp_1"));
        assert!(tel.iter().any(|(m, _)| m == "alarm_in_1"));
        assert_eq!(sink.event_count("mix", "offline"), 0);
    }

    /// 混块站 input 块（FC04）读失败 → 整站 offline + 部分弃用（§10.7 两层失败隔离语义一致）：
    /// 读序 holding temp(100) Ok → input alarm_in(200) Err → 已读 Ok 块整体弃用（无部分交付，
    /// telemetry 空），且 early-break 不再读其后的第三块 temp2(300)。
    #[tokio::test]
    async fn mixed_blocks_input_failure_isolates_station_no_partial_delivery() {
        let bus = Arc::new(MockBus::new());
        bus.put(3, 100, f32_regs(23.5)); // holding: temp（首个块，会先读到）
        bus.put_input(3, 200, f32_regs(1.0)); // input: alarm_in（第二块，命中失败）
        bus.put(3, 300, f32_regs(45.0)); // holding: temp2（第三块，early-break 后不应被读）
        bus.fail_input_once(3, 200); // input 块一次超时
        let sink = Arc::new(FakeSink::default());
        let st = StationConf {
            id: "mix".into(),
            role: Role::Hvac,
            port: "ttyS1".into(),
            protocol: "modbus".into(),
            slave: 3,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![
                RegBlockConf {
                    name: "temp".into(),
                    addr: 100,
                    func: RegFunc::Holding,
                    format: RegFormat::Float32,
                    scale: 0.0,
                    count: 2,
                    offset: 0.0,
                    byte_swap: false,
                    points: Vec::new(),
                    read_slice: false,
                },
                RegBlockConf {
                    name: "alarm_in".into(),
                    addr: 200,
                    func: RegFunc::Input,
                    format: RegFormat::Float32,
                    scale: 0.0,
                    count: 2,
                    offset: 0.0,
                    byte_swap: false,
                    points: Vec::new(),
                    read_slice: false,
                },
                RegBlockConf {
                    name: "temp2".into(),
                    addr: 300,
                    func: RegFunc::Holding,
                    format: RegFormat::Float32,
                    scale: 0.0,
                    count: 2,
                    offset: 0.0,
                    byte_swap: false,
                    points: Vec::new(),
                    read_slice: false,
                },
            ],
        };
        let sched = build(vec![st], bus.clone(), sink.clone());
        sched.tick_once(0).await;

        // 读序：holding temp 先读到（FC03）→ input alarm_in 读 1 次即失败（FC04）
        assert_eq!(bus.call_count(3, 100), 1, "holding 块应先被 FC03 读");
        assert_eq!(bus.input_call_count(3, 200), 1, "input 块应被 FC04 读 1 次后失败");
        // 任一块读失败 → 整站 offline（事件一次）；已读 Ok 块整体弃用——无部分交付
        assert_eq!(sink.event_count("mix", "offline"), 1, "input 块失败应隔离整站");
        assert!(
            sink.telemetry_of("mix").is_empty(),
            "holding 已读但不部分交付——本轮无 telemetry"
        );
        // early-break：input 块失败即 break → 其后的第三块 temp2(300) 不应被读
        assert_eq!(bus.call_count(3, 300), 0, "early-break 应钳制读序，不再读后续块");
    }

    /// 纯 FC04 站（全 input 块、无 holding）：读走 read_input 可正常采集（telemetry 非事件）。
    /// 与混块测互补：覆盖「站 regs 全部 input 块」的采集路径（无 holding 兜底）。
    #[tokio::test]
    async fn pure_input_blocks_station_collects_via_fc04() {
        let bus = Arc::new(MockBus::new());
        bus.put_input(3, 200, f32_regs(0.5)); // input: alarm_in
        bus.put_input(3, 202, f32_regs(1.5)); // input: status_in
        let sink = Arc::new(FakeSink::default());
        let st = StationConf {
            id: "pure_in".into(),
            role: Role::Hvac,
            port: "ttyS1".into(),
            protocol: "modbus".into(),
            slave: 3,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![
                RegBlockConf {
                    name: "alarm_in".into(),
                    addr: 200,
                    func: RegFunc::Input,
                    format: RegFormat::Float32,
                    scale: 0.0,
                    count: 2,
                    offset: 0.0,
                    byte_swap: false,
                    points: Vec::new(),
                    read_slice: false,
                },
                RegBlockConf {
                    name: "status_in".into(),
                    addr: 202,
                    func: RegFunc::Input,
                    format: RegFormat::Float32,
                    scale: 0.0,
                    count: 2,
                    offset: 0.0,
                    byte_swap: false,
                    points: Vec::new(),
                    read_slice: false,
                },
            ],
        };
        let sched = build(vec![st], bus.clone(), sink.clone());
        sched.tick_once(0).await;

        assert_eq!(bus.input_call_count(3, 200), 1, "全 input 块站应走 FC04");
        assert_eq!(bus.input_call_count(3, 202), 1);
        assert_eq!(bus.call_count(3, 200), 0, "纯 input 块站不应发 FC03 holding 读");
        let tel = sink.telemetry_of("pure_in");
        assert!(tel.iter().any(|(m, v)| m == "alarm_in_1" && *v == 0.5));
        assert!(tel.iter().any(|(m, v)| m == "status_in_1" && *v == 1.5));
        assert_eq!(sink.event_count("pure_in", "offline"), 0);
    }

    /// battery 站采集成功 → on_battery_soc 收到 mapper 解码的 soc（65.5，f32 精确）；
    /// telemetry 落库照旧（soc 点仍进 telemetry，独立通道不替代落库）；无 offline 事件。
    #[tokio::test]
    async fn battery_station_soc_pushed_via_dedicated_channel() {
        let bus = Arc::new(MockBus::new());
        // battery 站 soc 块（float32）：两寄存器 65.5 → mapper decode 65.5
        bus.put(2, 100, f32_regs(65.5));
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![battery_conf()], bus.clone(), sink.clone());
        sched.tick_once(0).await;
        assert_eq!(sink.soc_of("bms"), Some(65.5), "battery 站 soc 应经 on_battery_soc 推送");
        assert_eq!(sink.event_count("bms", "offline"), 0);
        // telemetry 落库照旧（soc 点仍进 telemetry）
        assert!(sink.telemetry_of("bms").iter().any(|(m, _)| m == "soc"));
    }

    /// battery 站读失败（未预置 → 读 Err）→ offline 事件一次，不推 soc（沿用旧数据语义）。
    #[tokio::test]
    async fn battery_station_failure_no_soc_push() {
        let bus = Arc::new(MockBus::new()); // 未预置 → 读 Err
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![battery_conf()], bus.clone(), sink.clone());
        sched.tick_once(0).await;
        assert_eq!(sink.event_count("bms", "offline"), 1);
        assert!(sink.soc_of("bms").is_none(), "失败轮不推 soc");
    }

    /// 负面 gating：battery 站 regs **无 soc 块**（role Battery，但 mapper Battery 分支仅空占位
    /// soc=None）→ 采集成功（无 offline 事件）但**不误推** on_battery_soc——`pkg.battery.soc`
    /// 为 None 时 `if let` 不触发，核心保证（无 soc 数据不产生 soc 通道噪声）。
    ///
    /// **C6（设计 §11.5.3.4）**：该配置形态（battery 站无 `soc` 点）**在配置期已被规则 4 拒**
    /// （`SouthStationsConfig::validate`），但本用例**不调 `validate`**（直接构造 `StationConf`
    /// 交给调度器）⇒ 仍绿。它保的是**调度器层不依赖配置校验的负向 gating**（PRD §9.4.3 规则 4
    /// 的运行期对偶：无 `soc` 点 ⇒ 绝不误推 `on_battery_soc`）；配置期侧由
    /// `config.rs::validate_rejects_battery_without_soc_point` 覆盖（两层各有其测）。
    /// **全部断言保留不动**。
    #[tokio::test]
    async fn battery_station_without_soc_block_does_not_push() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        // 决议：内联构造 StationConf（不动 battery_conf 签名）——role Battery 但 regs 空（无 soc 块）
        let st = StationConf {
            id: "bms".into(),
            role: Role::Battery,
            port: "ttyS2".into(),
            protocol: "modbus".into(),
            slave: 2,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![],
        };
        let sched = build(vec![st], bus.clone(), sink.clone());
        sched.tick_once(0).await;
        // 空 regs → 无块读、poll 成功（Data 占位、soc=None）：无 offline/online 状态事件
        assert_eq!(sink.event_count("bms", "offline"), 0);
        assert_eq!(sink.event_count("bms", "online"), 0);
        // 关键断言：无 soc 块 → battery.soc None → 不误触 on_battery_soc
        assert!(sink.soc_of("bms").is_none(), "无 soc 块不应误推 on_battery_soc");
    }

    /// 负面 gating：非 battery role（hvac）站正常采遥测 → 不触发 battery soc 分支（role
    /// gating `role == Role::Battery` 保证只有 Battery 站才走 on_battery_soc，其它站零噪声）。
    #[tokio::test]
    async fn non_battery_role_never_triggers_soc_channel() {
        let bus = Arc::new(MockBus::new());
        bus.put(3, 100, f32_regs(23.5)); // hvac temp 块
        let sink = Arc::new(FakeSink::default());
        let sched = build(
            vec![hvac_conf("hvac", "ttyS1", 3, 1000)],
            bus.clone(),
            sink.clone(),
        );
        sched.tick_once(0).await;
        // 正常采遥测（temp 点落库，无 offline 事件）——证明站本身健康
        assert_eq!(sink.telemetry_of("hvac"), vec![("temp_1".to_string(), 23.5)]);
        assert_eq!(sink.event_count("hvac", "offline"), 0);
        // 关键断言：hvac（非 Battery）不走 on_battery_soc 通道
        assert!(sink.soc_of("hvac").is_none(), "非 battery role 不应触发 soc 推送");
    }

    /// S3b-2 T5：`func: discrete` 块按 **FC02** 读（`StationBus::read_discrete`），
    /// 即 `read_discrete` 通路真正接通（T5 之前该块读被显式判失败 ⇒ 整站 offline）。
    #[tokio::test]
    async fn discrete_block_dispatches_to_fc02_read() {
        let bus = Arc::new(MockBus::new());
        bus.put_bits(3, 0, vec![false; 31]);
        let sink = Arc::new(FakeSink::default());
        let st = StationConf {
            id: "hvac".into(),
            role: Role::Hvac,
            port: "ttyS1".into(),
            protocol: "modbus".into(),
            slave: 3,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![dblk("hvac_di", 0, 31)],
        };
        let sched = build(vec![st], bus.clone(), sink.clone());
        sched.tick_once(0).await;

        assert_eq!(
            bus.bit_call_count(3, 0),
            1,
            "discrete 块应走 FC02（read_discrete）"
        );
        assert_eq!(bus.call_count(3, 0), 0, "不应发 FC03 holding 读");
        assert_eq!(bus.input_call_count(3, 0), 0, "不应发 FC04 input 读");
        assert_eq!(
            sink.event_count("hvac", "offline"),
            0,
            "FC02 读成功 ⇒ 站不 offline"
        );
    }

    /// 位块读失败（FC02 超时）→ 与寄存器块同策：整站 offline（§10.7 两层失败语义一致）。
    #[tokio::test]
    async fn discrete_block_read_failure_isolates_station() {
        let bus = Arc::new(MockBus::new()); // 未预置位 → 读 Err
        let sink = Arc::new(FakeSink::default());
        let st = StationConf {
            id: "hvac".into(),
            role: Role::Hvac,
            port: "ttyS1".into(),
            protocol: "modbus".into(),
            slave: 3,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![dblk("hvac_di", 0, 31)],
        };
        let sched = build(vec![st], bus.clone(), sink.clone());
        sched.tick_once(0).await;
        assert_eq!(sink.event_count("hvac", "offline"), 1);
    }

    // ---------- EdgeTracker（§11.4.7.1 统一事件模型，纯逻辑） ----------

    fn sig(k: &str, v: bool) -> (String, bool) {
        (k.to_string(), v)
    }

    /// 首轮只建基线（不产事件）；此后按 `(上轮, 本轮)` 二元组产"进入 1.0 / 退出 0.0"。
    #[test]
    fn edge_tracker_first_round_primes_then_reports_both_directions() {
        let mut t = EdgeTracker::default();
        assert!(
            t.edges(&[sig("a", false), sig("b", true)]).is_empty(),
            "首轮只建基线"
        );
        // b 由 true→false（退出）、a 由 false→true（进入），两者都返回（过滤在调用方）
        assert_eq!(
            t.edges(&[sig("a", true), sig("b", false)]),
            vec![("a".to_string(), 1.0, true), ("b".to_string(), 0.0, true)]
        );
        // 无变化 → 空
        assert!(t.edges(&[sig("a", true), sig("b", false)]).is_empty());
    }

    /// `reset()`（站从 offline 恢复时调用）后下一个 `edges()` 只重建基线、**不产事件**。
    #[test]
    fn edge_tracker_reset_suppresses_burst_after_recovery() {
        let mut t = EdgeTracker::default();
        t.edges(&[sig("a", false)]);
        assert_eq!(
            t.edges(&[sig("a", true)]),
            vec![("a".to_string(), 1.0, true)]
        );
        t.reset();
        assert!(t.edges(&[sig("a", true)]).is_empty(), "恢复后首轮不刷事件");
        // 恢复基线已建立：之后的变化照常产事件
        assert_eq!(
            t.edges(&[sig("a", false)]),
            vec![("a".to_string(), 0.0, true)]
        );
    }

    /// 上轮无记忆的新信号（未见过）不算跃迁：只记入基线、不产事件。
    #[test]
    fn edge_tracker_unknown_signal_is_not_an_edge() {
        let mut t = EdgeTracker::default();
        t.edges(&[sig("a", false)]);
        assert!(t.edges(&[sig("a", false), sig("new", true)]).is_empty());
        assert_eq!(
            t.edges(&[sig("a", false), sig("new", false)]),
            vec![("new".to_string(), 0.0, true)]
        );
    }

    // ---------- 消防字级信号 / 位块事件（AC-5 事件侧，§11.4.7.1）----------

    /// 消防站：`fire_sys`（addr 4，count 13，逐点声明，`at: 7` = 契约点 `fire_det_count`）
    /// 与 `fire_det` 探测器区（addr 17，`6×det_groups` 个寄存器 = 探测器 2..n，整字无 `points`）。
    /// 形态照录 PRD §9.4.1 的 fire 站（比例缩小到 1..det_groups 只探测器）。
    fn fire_conf(port: &str, slave: u8, det_groups: u16) -> StationConf {
        let mut pts: Vec<PointConf> = Vec::new();
        for k in 1..=13u16 {
            pts.push(PointConf {
                at: k,
                count: 1,
                name: (k == 7).then(|| "fire_det_count".to_string()),
                format: None,
                scale: None,
                offset: None,
                word_order: WordOrder::HiLo,
            });
        }
        StationConf {
            id: "fire".into(),
            role: Role::Fire,
            port: port.into(),
            protocol: "modbus".into(),
            slave,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![
                RegBlockConf {
                    name: "fire_sys".into(),
                    addr: 4,
                    func: RegFunc::Holding,
                    format: RegFormat::Uint16,
                    scale: 1.0,
                    count: 13,
                    offset: 0.0,
                    byte_swap: false,
                    points: pts,
                    read_slice: false,
                },
                RegBlockConf {
                    name: "fire_det".into(),
                    addr: 17,
                    func: RegFunc::Holding,
                    format: RegFormat::Uint16,
                    scale: 1.0,
                    count: 6 * det_groups,
                    offset: 0.0,
                    byte_swap: false,
                    points: Vec::new(),
                    read_slice: false,
                },
            ],
        }
    }

    /// 预置 fire 站：`sys4` = 系统状态（addr 4）整字；`det` = 探测器区逐寄存器
    /// （`[地址, 状态, 数据1, CO, VOC, H2]` 每 6 个一组）。
    fn put_fire(bus: &MockBus, slave: u8, sys4: u16, det: &[u16]) {
        let mut sys = vec![0u16; 13];
        sys[0] = sys4; // 偏移 0 = addr 4 = 系统状态
        bus.put(slave, 4, sys);
        bus.put(slave, 17, det.to_vec());
    }

    /// 预置 fire 站，并**显式给出探测器 1 的地址号（寄存器 11）**——`fire_sys` 块
    ///（addr 4）的偏移 7。地址升序链首的载体（v1.7 §11.4.6）。
    fn put_fire_det1(bus: &MockBus, slave: u8, sys4: u16, det1: u16, det: &[u16]) {
        let mut sys = vec![0u16; 13];
        sys[0] = sys4;
        sys[7] = det1;
        bus.put(slave, 4, sys);
        bus.put(slave, 17, det.to_vec());
    }

    /// 正/负向（AC-5 事件侧）：`WordBit` 进入**与退出**都产事件，`0x4400` 恰产 2 个。
    /// 首轮只建基线；telemetry 仍落整字原值（事件不改写遥测）。
    #[tokio::test]
    async fn fire_wordbit_signals_emit_enter_and_exit_events() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 1)], bus.clone(), sink.clone());

        put_fire(&bus, 1, 0x0000, &[1, 0, 0, 0, 0, 0]);
        sched.tick_once(0).await;
        assert!(sink.events_of("fire").is_empty(), "首轮只建基线、不产事件");

        // 0x4400 = bit14 主电故障 | bit10 压力传感器故障 → 恰 2 个进入事件（不多产其它位）
        put_fire(&bus, 1, 0x4400, &[1, 0, 0, 0, 0, 0]);
        sched.tick_once(1000).await;
        assert_eq!(
            sink.events_since("fire", 0),
            vec![
                ("fire_sys_1@main_power_fault".to_string(), 1.0),
                ("fire_sys_1@pressure_sensor_fault".to_string(), 1.0),
            ],
            "进入活跃：仅登记的两位产事件"
        );

        // 归零 → 恰 2 个"退出活跃"事件（联锁恢复的可观测点，§10.4）
        put_fire(&bus, 1, 0x0000, &[1, 0, 0, 0, 0, 0]);
        sched.tick_once(2000).await;
        assert_eq!(
            sink.events_since("fire", 2),
            vec![
                ("fire_sys_1@main_power_fault".to_string(), 0.0),
                ("fire_sys_1@pressure_sensor_fault".to_string(), 0.0),
            ],
            "退出活跃：消防双向（与位块只上升沿刻意不对称）"
        );
        // telemetry 仍落整字原值（事件不改写遥测值）
        assert!(sink
            .telemetry_of("fire")
            .iter()
            .any(|(m, v)| m == "fire_sys_1" && *v == 0.0));
    }

    /// 枚举跃迁（`WordEnum`）：0 → 1 → 2 → 0；活跃值之间（1→2）亦产事件。
    #[tokio::test]
    async fn fire_wordenum_emits_transition_events_between_active_values() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 1)], bus.clone(), sink.clone());
        // 火警状态 = `fire_sys_6`（addr 9，`fire_sys` 块内偏移 5）
        let det = [1u16, 0, 0, 0, 0, 0];
        let with_level = |lv: u16| {
            let mut sys = vec![0u16; 13];
            sys[5] = lv;
            bus.put(1, 4, sys);
            bus.put(1, 17, det.to_vec());
        };

        with_level(0);
        sched.tick_once(0).await;
        assert!(sink.events_of("fire").is_empty(), "首轮只建基线");

        with_level(1);
        sched.tick_once(1000).await;
        assert_eq!(
            sink.events_since("fire", 0),
            vec![("fire_sys_6@level1".to_string(), 1.0)],
            "0 → 1：一级报警进入"
        );

        with_level(2);
        sched.tick_once(2000).await;
        assert_eq!(
            sink.events_since("fire", 1),
            vec![
                ("fire_sys_6@level1".to_string(), 0.0),
                ("fire_sys_6@level2".to_string(), 1.0),
            ],
            "1 → 2：活跃值之间跃迁（level1 退出 + level2 进入）"
        );

        with_level(0);
        sched.tick_once(3000).await;
        assert_eq!(
            sink.events_since("fire", 3),
            vec![("fire_sys_6@level2".to_string(), 0.0)],
            "2 → 0：任何活跃值回到 0 即退出活跃"
        );
    }

    /// 负向：**预留位（bit2 点型，未启用）零事件**（PRD §9.7.6），但 telemetry 仍落整字原值。
    #[tokio::test]
    async fn fire_reserved_bit_yields_no_event_but_keeps_telemetry() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 1)], bus.clone(), sink.clone());
        let det = [1u16, 0, 0, 0, 0, 0];
        let with_smoke = |v: u16| {
            let mut sys = vec![0u16; 13];
            sys[2] = v; // 偏移 2 = addr 6 = 烟感状态
            bus.put(1, 4, sys);
            bus.put(1, 17, det.to_vec());
        };

        with_smoke(0);
        sched.tick_once(0).await;
        with_smoke(0x0004); // bit2 = 点型探测器（预留未启用，不在 mask 内）
        sched.tick_once(1000).await;
        assert!(
            sink.events_since("fire", 0).is_empty(),
            "预留位不产事件（mask = 0b11 不含 bit2）"
        );
        assert!(
            sink.telemetry_of("fire")
                .iter()
                .any(|(m, v)| m == "fire_sys_3" && *v == 4.0),
            "telemetry 仍落整字原值 4"
        );
        // 对照：同一寄存器 bit0（干接点触发）在 mask 内 → 产事件
        with_smoke(0x0001);
        sched.tick_once(2000).await;
        assert_eq!(
            sink.events_since("fire", 0),
            vec![("fire_sys_3@smoke_trigger".to_string(), 1.0)],
            "mask 内的位照常产事件（证明上一条的零事件源于「未登记」而非「未检测」）"
        );
    }

    /// 负向：**钢瓶气压零事件**（PRD §9.7.6，任何取值含恒 0 —— 不为其造判据）。
    #[tokio::test]
    async fn fire_cylinder_pressure_never_emits_event() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 1)], bus.clone(), sink.clone());
        let det = [1u16, 0, 0, 0, 0, 0];
        let with_pressure = |p: u16| {
            let mut sys = vec![0u16; 13];
            sys[1] = p; // 偏移 1 = addr 5 = 钢瓶气压
            bus.put(1, 4, sys);
            bus.put(1, 17, det.to_vec());
        };

        with_pressure(0);
        sched.tick_once(0).await;
        with_pressure(0);
        sched.tick_once(1000).await;
        with_pressure(123); // 非 0 亦不产事件（该点未登记任何信号）
        sched.tick_once(2000).await;
        assert!(
            sink.events_since("fire", 0).is_empty(),
            "钢瓶气压不产任何事件（恒 0 不得判「气压异常」）"
        );
        assert!(sink
            .telemetry_of("fire")
            .iter()
            .any(|(m, v)| m == "fire_sys_2" && *v == 123.0));
    }

    /// 探测器总状态（addr 12 / 探测器区 `+1`）：只 `alarm`(bit12) / `fault`(bit14) 产事件；
    /// bit0–4 的传感器细分**不产独立事件**（设计取舍，§11.12.1 项 6）。
    #[tokio::test]
    async fn fire_detector_state_emits_alarm_and_fault_only() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        // 探测器区 1 组（探测器 2）：offset 1 = 状态（addr 18 → 模板 +1）
        let sched = build(vec![fire_conf("ttyS6", 1, 1)], bus.clone(), sink.clone());

        put_fire(&bus, 1, 0, &[2, 0x0000, 0, 0, 0, 0]);
        sched.tick_once(0).await;
        // 探测器 1 状态（addr 12 = `fire_sys_9`）bit12 → 报警总状态
        let mut sys = vec![0u16; 13];
        sys[8] = 1 << 12;
        bus.put(1, 4, sys);
        // 探测器 2 状态（addr 18 = `fire_det_2`）bit14 → 故障总状态 + bit0–4 细分位
        bus.put(1, 17, vec![2, (1 << 14) | 0x001F, 0, 0, 0, 0]);
        sched.tick_once(1000).await;
        assert_eq!(
            sink.events_since("fire", 0),
            vec![
                ("fire_sys_9@alarm".to_string(), 1.0),
                ("fire_det_2@fault".to_string(), 1.0),
            ],
            "只取两条总状态；bit0–4 传感器细分不产独立事件"
        );
    }

    /// 负向：**消防地址序违规（Q-9）** —— 探测器 `+0` 地址非严格升序 → 产事件；
    /// **首次观测即产**（§11.4.7.2 C 对"首轮只建基线"的唯一例外：上线时就已违规
    /// 必须可见），但**状态未变的轮次不重复产**（状态翻转制，原"命中即产"= 8.6 万条/日）。
    ///
    /// 组序号含**链首 = 寄存器 11（探测器 1）**（v1.7 §11.4.6 订正）：故"第 2 只探测器"
    /// 回退时组序号是 **3**（探测器 1 / 探测器 2 / 探测器 3 的 1 基序号）。
    #[tokio::test]
    async fn fire_detector_addr_order_violation_emits_event() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 3)], bus.clone(), sink.clone());

        // 链首（探测器 1 地址 0）+ 3 组探测器：+0 地址 3 / 2 / 4 → 第 3 组回退 ⇒ 违规
        let violation = [3u16, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0];
        put_fire(&bus, 1, 0, &violation);
        sched.tick_once(0).await;
        assert_eq!(
            sink.events_of("fire"),
            vec![("fire_detector_addr_order_invalid".to_string(), 3.0)],
            "首轮即违规 ⇒ 首次观测即产 1 条，value = 首个违规组的 1 基序号（含链首探测器 1）"
        );

        // 违规状态**未变**：连续多轮（含第 5 轮）**一条都不产**（去抖 = 状态翻转，非时间窗）
        for t in 1..5u64 {
            put_fire(&bus, 1, 0, &violation);
            sched.tick_once(t * 1000).await;
        }
        assert_eq!(
            sink.events_of("fire").len(),
            1,
            "判据仍成立的稳态轮次不重复产事件（翻转为零才产）"
        );
        // 去重**只作用于事件**：telemetry 逐轮照落（§11.4.7.2 E —— 5 轮 × `fire_sys_1`）
        assert_eq!(
            sink.telemetry_of("fire")
                .iter()
                .filter(|(m, _)| m == "fire_sys_1")
                .count(),
            5,
            "去重不得扩散到非事件路径：telemetry 每轮都落"
        );
    }

    /// **恢复必产"已恢复"事件**（§11.4.7.2 C）：升序恢复 ⇒ 恰 1 条
    /// `fire_detector_addr_order_invalid@recovered`（`value = 0.0`，哨兵不承载诊断量）
    /// —— 否则事件日志分不清"仍违规"与"已修复"。恢复后再保持升序 → 不再产。
    #[tokio::test]
    async fn fire_addr_order_flag_recovery_emits_recovered_event() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 3)], bus.clone(), sink.clone());

        put_fire(
            &bus,
            1,
            0,
            &[3, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0],
        );
        sched.tick_once(0).await; // 进入（首次观测即产）

        // 升序（2/3/4）→ 状态翻转 false：产 1 条 `@recovered`
        put_fire(
            &bus,
            1,
            0,
            &[2, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0],
        );
        sched.tick_once(1000).await;
        assert_eq!(
            sink.events_since("fire", 1),
            vec![(
                "fire_detector_addr_order_invalid@recovered".to_string(),
                0.0
            )],
            "恢复升序 ⇒ 必产 1 条「已恢复」（value 恒 0.0）"
        );

        // 保持升序 → 稳态零事件
        let before = sink.events_of("fire").len();
        sched.tick_once(2000).await;
        sched.tick_once(3000).await;
        assert!(
            sink.events_since("fire", before).is_empty(),
            "恢复后的稳态轮次一条都不产"
        );
    }

    /// "首次观测即产"的例外对**站恢复后的首轮**同样成立（§11.4.7.2 C 的 offline→恢复行）：
    /// 恢复后首轮**仍违规** ⇒ 再产 1 条进入事件（`reset()` 重建基线后仍可见），
    /// 不为它单开"跨离线保持"的第二套记忆。
    #[tokio::test]
    async fn fire_addr_order_flag_reemits_after_station_recovery() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 3)], bus.clone(), sink.clone());
        let violation = [3u16, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0];

        put_fire(&bus, 1, 0, &violation);
        sched.tick_once(0).await;
        assert_eq!(sink.events_of("fire").len(), 1, "首轮即违规 ⇒ 产 1 条");

        bus.fail_once(1, 4); // 系统状态块读失败 → 整站 offline
        sched.tick_once(1000).await;
        assert_eq!(sink.event_count("fire", "offline"), 1);
        let before = sink.events_of("fire").len();

        put_fire(&bus, 1, 0, &violation); // 恢复后首轮**仍违规**
        sched.tick_once(2000).await;
        assert_eq!(
            sink.events_since("fire", before),
            vec![
                ("online".to_string(), 1.0),
                ("fire_detector_addr_order_invalid".to_string(), 3.0),
            ],
            "恢复首轮仍违规 ⇒ 按「首次观测即产」再产 1 条（基线已重建）"
        );
    }

    /// 【补改 2 / v1.7 §11.4.6】**探测器 1（寄存器 11）已纳入升序链首**：
    /// 探测器 1 的地址大于探测器 2 ⇒ 第 2 组违规（旧实现只看 `fire_det*` 区，此形态漏检）。
    #[tokio::test]
    async fn fire_addr_order_chain_head_checks_detector_one() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 1)], bus.clone(), sink.clone());

        // 对照：探测器 1 地址 1 < 探测器 2 地址 3 ⇒ 严格升序，零事件
        put_fire_det1(&bus, 1, 0, 1, &[3, 0, 0, 0, 0, 0]);
        sched.tick_once(0).await;
        assert!(
            sink.events_of("fire").is_empty(),
            "探测器 1 地址 1 < 探测器 2 地址 3 ⇒ 升序成立、无违规"
        );

        // 探测器 1（寄存器 11）地址 9 > 探测器 2 地址 3 ⇒ 违规（组序号 2 = 探测器 2）
        put_fire_det1(&bus, 1, 0, 9, &[3, 0, 0, 0, 0, 0]);
        sched.tick_once(1000).await;
        assert_eq!(
            sink.events_since("fire", 0),
            vec![("fire_detector_addr_order_invalid".to_string(), 2.0)],
            "链首参与比较：探测器 1 地址 9 之后出现 3 ⇒ 第 2 只违规"
        );
    }

    /// 负向：**位块只上升沿**（与消防双向刻意不对称，§11.4.7.1）——
    /// BMS `bms_alarm_*` 注入 1（进入）→ 产事件；回 0（退出）→ **不产**。
    /// 同时钉住 AC-3 的点位名映射：点位名序号 225 = 位内偏移 224 = **位地址 424**。
    #[tokio::test]
    async fn alarm_bit_block_emits_rising_edge_only() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        // 位块 `bms_alarm`（位地址 200..487，`_k` ↔ 位地址 199+k）
        let st = StationConf {
            id: "bms".into(),
            role: Role::Battery,
            port: "ttyS2".into(),
            protocol: "modbus".into(),
            slave: 2,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![dblk("bms_alarm", 200, 288)],
        };
        let sched = build(vec![st], bus.clone(), sink.clone());
        let with_bit = |k: usize| {
            let mut bits = vec![false; 288];
            bits[k] = true;
            bus.put_bits(2, 200, bits);
        };

        with_bit(0);
        sched.tick_once(0).await; // 首轮：只建基线
        assert!(sink.events_of("bms").is_empty());

        with_bit(224); // 位地址 424 = 簇一级告警（点名为 `bms_alarm_225`）
        sched.tick_once(1000).await;
        assert_eq!(
            sink.events_since("bms", 0),
            vec![("bms_alarm_225".to_string(), 1.0)],
            "告警位 0→1 产事件；点位名序号 225 ↔ 位地址 424"
        );

        with_bit(0); // 该位回 0
        sched.tick_once(2000).await;
        assert!(
            sink.events_since("bms", 1).is_empty(),
            "位块只上升沿：0→1 之后的 1→0 不产退出事件"
        );
    }

    /// 位块的非告警类别（`State`/`Reserved`）只落 telemetry、不产事件：
    /// 空调 `hvac_di` 位 0（内风机 = `State`）与保留位 25 置位 → 零事件；
    /// 同类里位 9（`Alarm` 柜内温感故障）置位 → 产事件（`hvac_di_10`）。
    #[tokio::test]
    async fn state_and_reserved_bits_yield_no_event_alarm_bit_does() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let st = StationConf {
            id: "hvac".into(),
            role: Role::Hvac,
            port: "ttyS1".into(),
            protocol: "modbus".into(),
            slave: 3,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 1000,
            regs: vec![dblk("hvac_di", 0, 31)],
        };
        let sched = build(vec![st], bus.clone(), sink.clone());
        let with_bits = |ks: &[usize]| {
            let mut bits = vec![false; 31];
            for &k in ks {
                bits[k] = true;
            }
            bus.put_bits(3, 0, bits);
        };

        with_bits(&[]);
        sched.tick_once(0).await;
        with_bits(&[0, 25]); // 位 0 = 内风机（State）、位 25 = 保留
        sched.tick_once(1000).await;
        assert!(
            sink.events_since("hvac", 0).is_empty(),
            "State/Reserved 位不产事件"
        );

        with_bits(&[9]); // 位 9 = 柜内温感故障（Alarm）→ 点名 `hvac_di_10`
        sched.tick_once(2000).await;
        assert_eq!(
            sink.events_since("hvac", 0),
            vec![("hvac_di_10".to_string(), 1.0)],
            "同类中 Alarm 位产事件（点名序号 = 位偏移 + 1）"
        );
    }

    /// 站从 offline 恢复后首轮**不产事件**（`reset()` 生效：现势值连续性不可假设）。
    #[tokio::test]
    async fn tracker_reset_after_recovery_suppresses_event_burst() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 1)], bus.clone(), sink.clone());

        put_fire(&bus, 1, 0, &[1, 0, 0, 0, 0, 0]);
        sched.tick_once(0).await; // 基线（全 0）
        assert_eq!(sink.event_count("fire", "offline"), 0);

        // 读失败一轮 → offline
        bus.fail_once(1, 4);
        sched.tick_once(1000).await;
        assert_eq!(sink.event_count("fire", "offline"), 1);
        let before = sink.events_of("fire").len();

        // 恢复首轮：系统状态已置位（若基线未重建，会立刻误产"进入"事件）
        put_fire(&bus, 1, 0x4400, &[1, 0, 0, 0, 0, 0]);
        sched.tick_once(2000).await;
        let after_recovery = sink.events_of("fire");
        assert_eq!(
            sink.events_since("fire", before),
            vec![("online".to_string(), 1.0)],
            "恢复首轮只发 online 状态事件，不刷信号事件"
        );
        assert_eq!(after_recovery.len(), 2, "offline + online 两条状态事件");

        // 恢复之后的变化照常产事件（基线已重建为恢复轮的 0x4400：两位皆活跃 ⇒ 清 bit14 即退出）
        put_fire(&bus, 1, 0x0400, &[1, 0, 0, 0, 0, 0]); // 仅剩 bit10 压力传感器故障
        sched.tick_once(3000).await;
        assert_eq!(
            sink.events_since("fire", after_recovery.len()),
            vec![("fire_sys_1@main_power_fault".to_string(), 0.0)],
            "恢复后的基线已重建：其后变化照常产事件（此轮为 bit14 退出）"
        );
    }

    // ---------- AC-4：pcs 站调度预算与隔离 ----------

    /// AC-4：`role_priority(Pcs) = 1` —— 排在 grid/battery(0) 之后、hvac/fire(2) 之前
    /// （PCS 不参与控制决策，不得抢占总表 phase 的调度预算，PRD §9.3.2.3）。
    #[tokio::test]
    async fn pcs_priority_is_between_grid_and_hvac() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let pcs = StationConf {
            id: "pcs".into(),
            role: Role::Pcs,
            port: "ttyS1".into(),
            protocol: "modbus".into(),
            slave: 5,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 500,
            regs: vec![blk("pcs_3zone", 0, 6)],
        };
        put_grid(&bus, 1);
        bus.put(5, 0, phase_regs(1.0, 2.0, 3.0));
        bus.put(3, 100, f32_regs(23.5));
        let sched = build(
            vec![
                hvac_conf("hvac", "ttyS1", 3, 1000),
                pcs,
                grid_conf("grid", "ttyS1", 1, 1000),
            ],
            bus.clone(),
            sink.clone(),
        );
        sched.tick_once(0).await;

        // 同轮到期时的读序（口内串行）= 优先级序：grid(slave 1，多块) → pcs(5) → hvac(3)。
        // 按"站首现序"归并（grid 站有 6 个块 ⇒ 连续多次 slave=1）。
        let mut order: Vec<u8> = Vec::new();
        for &(slave, _, _) in bus.calls.lock().unwrap().iter() {
            if order.last() != Some(&slave) {
                order.push(slave);
            }
        }
        assert_eq!(
            order,
            vec![1, 5, 3],
            "读序应为 grid → pcs → hvac（prio 0/1/2）"
        );
    }

    /// AC-4：`pcs` 站超时 → 该站 offline + 事件、同口其它站不受影响；恢复 online **一次**。
    #[tokio::test]
    async fn pcs_station_isolated_then_recovers_online_once() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let pcs = StationConf {
            id: "pcs".into(),
            role: Role::Pcs,
            port: "ttyS1".into(),
            protocol: "modbus".into(),
            slave: 5,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 500,
            regs: vec![blk("pcs_3zone", 0, 6)],
        };
        bus.put(3, 100, f32_regs(23.5));
        let sched = build(
            vec![hvac_conf("hvac", "ttyS1", 3, 1000), pcs],
            bus.clone(),
            sink.clone(),
        );

        sched.tick_once(0).await; // pcs 未预置 → 超时
        assert_eq!(
            sink.event_count("pcs", "offline"),
            1,
            "pcs 超时应 offline 告警一次"
        );
        assert_eq!(
            sink.telemetry_of("hvac"),
            vec![("temp_1".to_string(), 23.5)],
            "同口 hvac 不受 pcs 隔离影响"
        );

        sched.tick_once(500).await; // pcs interval=500 到期（退避后 oc=2 → next_due=1500）
        assert_eq!(sink.event_count("pcs", "offline"), 1, "窗口内防刷屏仍一次");

        bus.put(5, 0, phase_regs(1.0, 2.0, 3.0));
        sched.tick_once(1500).await; // 退避到期点 → 恢复
        assert_eq!(sink.event_count("pcs", "online"), 1, "恢复 online 一次");
        {
            let st = sched.state.read().unwrap();
            assert_eq!(st[1].offline_count, 0, "恢复后 offline_count 归零");
        }
        assert_eq!(sink.event_count("hvac", "offline"), 0, "hvac 全程健康");
    }

    /// PRD §9.7.2 第 1 条：站 offline 的**事件 reason 含 `slave/addr/count`**（运维据 events
    /// 定位到具体块）。southd 侧经 `StationSink::on_station_offline` 交出 reason；事件文案的
    /// 组装属消费层（core-bin，T6）。
    #[tokio::test]
    async fn offline_reason_contains_slave_addr_count() {
        let bus = Arc::new(MockBus::new()); // 未预置 → 读 Err
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_conf("hvac", "ttyS1", 3, 1000)], bus, sink.clone());
        sched.tick_once(0).await;

        let reasons = sink.offline_reasons.lock().unwrap();
        assert_eq!(reasons.len(), 1, "offline 一次即交出一次 reason");
        let (id, reason) = &reasons[0];
        assert_eq!(id, "hvac");
        assert!(
            reason.contains("slave=3") && reason.contains("0x0064") && reason.contains("x2"),
            "reason 须含 slave/addr/count（块 temp @ 0x0064 x2），实际: {reason}"
        );
        // 默认实现的事件形态不变（C5 锚：`event_count(.., "offline")` 语义不动）
        assert_eq!(sink.event_count("hvac", "offline"), 1);
    }

    /// AC-5：`pcs` 站**只读** —— 正常采集也不触发 `on_grid_package`（不推进 5s 控制闸门）
    /// 与 `on_battery_soc`（不参与 SOC 双源）；其点位只走 `on_station_telemetry`。
    #[tokio::test]
    async fn pcs_station_never_triggers_grid_or_soc_channels() {
        let bus = Arc::new(MockBus::new());
        bus.put(5, 0, phase_regs(1.0, 2.0, 3.0));
        let sink = Arc::new(FakeSink::default());
        let pcs = StationConf {
            id: "pcs".into(),
            role: Role::Pcs,
            port: "ttyS1".into(),
            protocol: "modbus".into(),
            slave: 5,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 500,
            regs: vec![blk("pcs_3zone", 0, 6)],
        };
        let sched = build(vec![pcs], bus.clone(), sink.clone());
        sched.tick_once(0).await;

        assert_eq!(
            sink.grid_count(),
            0,
            "pcs 站不得走 on_grid_package（不推进控制闸门）"
        );
        assert!(sink.soc_of("pcs").is_none(), "pcs 站不得走 on_battery_soc");
        assert_eq!(sink.event_count("pcs", "offline"), 0);
        assert!(
            sink.telemetry_of("pcs")
                .iter()
                .any(|(m, _)| m == "pcs_3zone_1"),
            "点位仍经 on_station_telemetry 落库"
        );
    }

    /// 纯 DueCalc：到期/间隔/优先级/同 now 去重/落后钳制。
    #[test]
    fn due_calc_respects_intervals_and_priority() {
        let stations = vec![
            grid_conf("grid", "ttyS1", 1, 1000),
            hvac_conf("hvac", "ttyS1", 3, 5000),
        ];
        let group: Vec<(usize, &StationConf)> = stations.iter().enumerate().collect();
        let mut calc = DueCalc::from_group(&group);

        // 首轮两站都到期（next_due=0 启动即采），grid(prio0) 在 hvac(prio2) 前
        assert_eq!(
            calc.due_round(0),
            vec![StationPoll { station_index: 0 }, StationPoll { station_index: 1 }]
        );
        // 同一 now 二次调用不再返回（已推进 next_due）
        assert!(calc.due_round(0).is_empty());
        // now=1000：仅 grid 到期（hvac next_due 已推进到 5000）
        assert_eq!(calc.due_round(1000), vec![StationPoll { station_index: 0 }]);
        // now=5000：grid（1000 到期后 1000+1000=2000→5000 已落后一轮，钳到 6000）与 hvac 都到期
        assert_eq!(
            calc.due_round(5000),
            vec![StationPoll { station_index: 0 }, StationPoll { station_index: 1 }]
        );
    }

    /// delay_station：把 next_due 后移 now+extra；现 next_due 更晚则不动。
    #[test]
    fn due_calc_delay_station_backs_off() {
        let stations = vec![hvac_conf("hvac", "ttyS1", 3, 1000)];
        let group: Vec<(usize, &StationConf)> = stations.iter().enumerate().collect();
        let mut calc = DueCalc::from_group(&group);
        assert_eq!(calc.due_round(0).len(), 1); // 首轮到期，next_due 推进到 1000
        // 退避 extra=2000 → target=0+2000；现 next_due=1000 < 2000 → 后移到 2000
        calc.delay_station(0, 0, 2000);
        // now=1000 不再到期（next_due=2000）
        assert!(calc.due_round(1000).is_empty());
        // now=2000 到期
        assert_eq!(calc.due_round(2000).len(), 1);
    }

    /// backoff_extra：1→1×, 2→2×, 3→4×, 6→32×(封顶), u32::MAX→32×(saturating)
    #[test]
    fn backoff_extra_caps_at_max_shift() {
        assert_eq!(backoff_extra(1000, 1), 1000);
        assert_eq!(backoff_extra(1000, 2), 2000);
        assert_eq!(backoff_extra(1000, 3), 4000);
        assert_eq!(backoff_extra(1000, 5), 16000);
        assert_eq!(backoff_extra(1000, 6), 32000, "oc=6 封顶 32×");
        assert_eq!(backoff_extra(1000, u32::MAX), 32000, "极端 oc saturating 后封顶");
        assert_eq!(backoff_extra(60000, 6), 1_920_000); // 60s×32
    }

    /// offline 事件 stale_timeout_s 窗口防刷屏：持续失败多轮只告警一次（offline_count 仍逐轮
    /// 累加）。注意退避语义（§10.2 M-11）：失败轮 next_due 指数后移 → tick 0/1000 失败后
    /// 下一到期点是 3000（2000 轮被退避跳过），故 4 个 tick 实际只采 3 次 → offline_count=3
    /// （非 4）；事件去抖独立于退避，3600s 窗口内仍只 1 次 offline。
    #[tokio::test]
    async fn offline_event_throttled_by_stale_timeout() {
        let bus = Arc::new(MockBus::new()); // 未预置 → 每次读 Err
        let sink = Arc::new(FakeSink::default());
        // stale_timeout 取大值（3600s）：多轮 tick 都在同一窗口内
        let sched = build_with_timeout(
            vec![hvac_conf("hvac", "ttyS1", 3, 1000)],
            bus,
            sink.clone(),
            3600,
        );

        for now in [0u64, 1000, 2000, 3000] {
            sched.tick_once(now).await;
        }
        assert_eq!(sink.event_count("hvac", "offline"), 1, "窗口内防刷屏只应告警一次");
        // offline_count 逐失败轮累加（0/1000/3000 三失败轮；2000 轮退避跳过未试，调度态独立于事件去抖）
        {
            let st = sched.state.read().unwrap();
            assert_eq!(st[0].offline_count, 3);
        }
    }

    /// offline 慢站降频：持续失败站 poll 次数随退避减少，同口关键站（grid）cadence 不损。
    #[tokio::test]
    async fn offline_station_backs_off_reducing_poll_frequency() {
        let bus = Arc::new(MockBus::new()); // 未预置 → 恒 Err
        put_grid(&bus, 1); // grid 完整预置（成功路径，验证其 cadence 不因同口 hvac 退避受损）
        let sink = Arc::new(FakeSink::default());
        let sched = build(
            vec![
                grid_conf("grid", "ttyS1", 1, 1000),
                hvac_conf("hvac", "ttyS1", 3, 1000),
            ],
            bus.clone(),
            sink.clone(),
        );
        // grid 正常每轮采；hvac 恒失败退避（offline_count=1 时 extra=1000）
        sched.tick_once(0).await; // 两站到期；hvac 失败 oc=1 → extra=1000 → next_due=1000
        sched.tick_once(1000).await; // hvac 到期再失败 oc=2 → extra=2000 → next_due=3000
        sched.tick_once(2000).await; // hvac next_due=3000 未到期 → 不试
        sched.tick_once(3000).await; // hvac 到期再失败 oc=3 → extra=4000 → next_due=7000
        // grid：0/1000/2000/3000 全采（4 次）；hvac：0/1000/3000 失败试 3 次（2000 被退避跳过）
        assert_eq!(bus.call_count(1, 0), 4, "grid 不应被 hvac 退避拖累");
        assert_eq!(bus.call_count(3, 100), 3, "hvac 2000 轮应被退避跳过");
        // 事件：stale_timeout_s=5（build 默认 cfg(stations,5)），3000-0=3s<5s → 一次 offline
        assert_eq!(sink.event_count("hvac", "offline"), 1);
        assert_eq!(sink.event_count("hvac", "online"), 0);
    }

    /// meter_grid 正常 → on_grid_package 收 pkg 且含分相（phase.is_some）与顶层量。
    #[tokio::test]
    async fn meter_grid_pkg_delivered_to_sink() {
        let bus = Arc::new(MockBus::new());
        put_grid(&bus, 1);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![grid_conf("grid", "ttyS1", 1, 1000)], bus, sink.clone());

        sched.tick_once(0).await;

        assert_eq!(sink.grid_count(), 1);
        assert_eq!(sink.event_count("grid", "offline"), 0);
        assert_eq!(sink.event_count("grid", "online"), 0);
        let pkg = sink.grid_pkg();
        assert!(pkg.electrical.phase.is_some(), "grid pkg 应含分相");
        assert_eq!(pkg.electrical.active_power, Some(5.5)); // p_total 独立块原值
        assert_eq!(pkg.electrical.voltage, Some(220.0));
    }

    /// build 的带 timeout 变体
    fn build_with_timeout(
        stations: Vec<StationConf>,
        bus: Arc<MockBus>,
        sink: Arc<FakeSink>,
        stale_timeout_s: u64,
    ) -> Arc<SouthScheduler> {
        let mut buses: HashMap<String, Arc<dyn StationBus>> = HashMap::new();
        buses.insert(stations[0].port.clone(), bus as Arc<dyn StationBus>);
        SouthScheduler::new(cfg(stations, stale_timeout_s), buses, sink)
    }
}
