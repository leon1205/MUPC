//! 口级采集调度器（S3a Task 5；§10.2 口调度预算 / §10.7 站超时隔离）。
//!
//! 架构：**每 port 一条采集 task**（口间并发），口内多从站按到期（`next_due`）**串行**
//! 轮询（口单 poller 天然串行；Rs485PortBus 内另有 per-port async Mutex 双保险，Task 3）。
//! 站失败（任一寄存器块读 Err / mapper 语义 Failed）→ 站级 offline 隔离，不阻断同口其它站。
//!
//! §10.2 M-11 口调度预算：到期判定（`next_due`）+ 角色优先级排序（grid/battery 先于
//! hvac/fire，见 [`role_priority`]）+ **offline 慢站指数退避降频**（poll 失败后
//! [`DueCalc::delay_group`] 把 next_due 按 `interval << min(offline_count-1, 5)` 后移，
//! 封顶 32×interval——失败站降频不拖累同口关键站 cadence；恢复即正常 cadence）。
//!
//! **S3b-3（T9）调度粒度由"站"细化为"读组"**（设计 §12）：站内**有效周期相同**的块合成一个
//! [`ReadGroup`]，各自独立到期（[`GroupPoll`]）；`offline`/`online`、`offline_count` 与站级
//! 退避**只由站级承载组承载**（C8），块级（快采）组失败**不升级为站级 offline**（§12.5）。
//! **单组站（无块声明 `interval_ms`）与空 `regs` 站均与改造前逐字等价**（§12.3 V-5）。
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

/// 本轮应采的**一个读组**（替代本模块此前**按站粒度**的那套类型 —— 本轮已整体删除，含其
/// 跨类型等值实现；设计 §12.2.2：`GroupPoll` **替代**站级 `Poll`，**不保留兼容类型**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupPoll {
    /// state Vec 全局下标（站）
    pub station_index: usize,
    /// 组锚块下标（[`ReadGroup::anchor_blk`]；空 `regs` 站的退化组 = [`EMPTY_GROUP_ANCHOR`]）
    pub anchor_blk: usize,
    /// 该组是否为**站级承载组**（PRD §10.3.2 **C8**：站内周期**最大**的组；并列取锚最小者）
    pub is_carrier: bool,
    /// 该组**上一轮是否失败**（决定本轮成功后是否重建变化沿基线；§12.5）
    pub was_failing: bool,
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

// ═══════════ S3b-3（T8）：块级采集周期的分组纯函数与内部结构（§12.2.2 / §12.4.1）═══════════
//
// 本节的 5 个定义（`EMPTY_GROUP_ANCHOR` / `ReadGroup` / `GroupKey` / [`read_groups_of`] /
// [`carrier_group`]）是 S3b-3 的**分组契约**：配置期校验（`config.rs::validate_block_intervals`
// 的规则 20/23/24）与调度期构造（T9 的 `DueCalc::from_group`）**共用同一实现**，防两处漂移。
// **T8 只新增**（既有调度流程——`DueCalc` / `PortRunner` / `run_port_round` / `poll_group` /
// `round_signals_group` / `EdgeTracker`——**一字未动**）；`GroupPoll` 与 `DueEntry` 的改造属 T9。

/// 空块集（`regs` 为空）退化组的**哨兵锚**。取 `usize::MAX`：任何真实块下标（`< regs.len()`）
/// 都不可能与之相等 ⇒ 「锚 → 组」仍是**单射**，`(station_index, anchor_blk)` 仍可作稳定
/// `HashMap` 键（设计 §12.4.1 的注；§12.10.1 第 7 项）。
pub const EMPTY_GROUP_ANCHOR: usize = usize::MAX;

/// 站内一个「读组」：**有效周期相同**的块合为一组；一组一次轮询读齐组内全部块。
///
/// 字段对 T9 与配置期校验均可见（`pub`；设计 §12.2.2 的字段定义，仅可见性放宽、语义不变）。
#[derive(Debug, Clone)]
pub struct ReadGroup {
    /// 组内块在 `StationConf::regs` 中的下标（**升序 = regs 书写序**）
    pub blk_indices: Vec<usize>,
    /// 组周期 = 组内块的有效周期（构造期由 [`read_groups_of`] 保证同组同值）
    pub interval_ms: u64,
    /// 组锚 = `blk_indices[0]`（块 → 组是 1:1，故锚**唯一**，可直接作稳定键）；
    /// **空块集（`regs` 为空的退化组）取哨兵 [`EMPTY_GROUP_ANCHOR`]**
    /// —— 无真实块下标可与之相等，故锚仍**单射**（§12.4.1）。
    pub anchor_blk: usize,
}

/// 组键：`(站下标, 组锚块下标)` —— 稳定、与 cfg 序绑定、可作 `HashMap` 键（`Hash + Eq`）。
///
/// T9 用它替换 `PortRunner.trackers` 的 `usize`（站）键（§12.4.4：不改则组间互相清空基线）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupKey {
    pub station_index: usize,
    pub anchor_blk: usize,
}

/// 站 → 读组划分。**唯一分组实现**（配置期校验与调度期构造都调它，防两处漂移）。
/// 返回按**组周期升序**（`BTreeMap` 序）。
///
/// **不变量：对任何 `StationConf`（含 `regs` 为空）至少返回一个组。**
/// - 有块且无块声明 `interval_ms` ⇒ 唯一桶（= 全部块、周期 = 站周期）—— V-5(a)；
/// - **`regs` 为空 ⇒ 一个退化组**（空块集、周期 = 站周期、锚 = [`EMPTY_GROUP_ANCHOR`]）—— V-5(b)。
///   **不得**返回 0 个组：那会使该站**永不进入 `DueCalc`**（= 被静默移出调度），与既有
///   "空 `regs` 站照常被轮询、读集为空、成功/失败记账照旧"的行为不符 —— 既有单测
///   `battery_station_without_soc_block_does_not_push` 内联构造 `regs: vec![]` 直接
///   `tick_once`、**不经 `validate`** ⇒ 该输入**可达**（设计 §12.4.1 / §12.10.3 R-12）。
pub fn read_groups_of(c: &StationConf) -> Vec<ReadGroup> {
    let mut by_period: std::collections::BTreeMap<u64, Vec<usize>> = Default::default();
    for (i, b) in c.regs.iter().enumerate() {
        by_period
            .entry(b.effective_interval_ms(c.interval_ms))
            .or_default()
            .push(i);
    }
    if by_period.is_empty() {
        by_period.insert(c.interval_ms, Vec::new()); // 退化组：空块集、周期 = 站周期
    }
    by_period
        .into_iter()
        .map(|(interval_ms, blk_indices)| ReadGroup {
            // 键：块 → 组 1:1 ⇒ 锚唯一；空块集取哨兵（**不索引 `blk_indices[0]`**）
            anchor_blk: blk_indices.first().copied().unwrap_or(EMPTY_GROUP_ANCHOR),
            blk_indices,
            interval_ms,
        })
        .collect()
}

/// 站级承载组（PRD §10.3.2 **C8**）：**周期最大**的组；并列取 `anchor_blk` 最小者。
/// 语义：`offline`/`online` 状态事件、`offline_count` 与站级退避都由它承载
/// ⇒ **站离线判定时延与现状一致**（不被快组影响）。
///
/// **返回 `Option<ReadGroup>`（设计 §12.4.1 的 S-1 注：绝不在该可达输入上 panic）**：
/// [`read_groups_of`] 的"至少一组"不变量保证实际恒 `Some`（含空 `regs` 站的退化组），
/// 但**即便如此也不得用 `expect`/`unwrap`** —— 空 `regs` 是**可达输入**（配置期只对
/// `Role::Pcs` 拒空；调度器单测直接内联构造），一旦将来 `read_groups_of` 的不变量被改坏，
/// `expect` 会把"配置错误"变成"进程 panic"。
pub fn carrier_group(c: &StationConf) -> Option<ReadGroup> {
    read_groups_of(c)
        .into_iter()
        .max_by_key(|g| (g.interval_ms, std::cmp::Reverse(g.anchor_blk)))
}

/// 到期计算条目（纯逻辑）：**一组一条**（S3b-3 T9 起；此前一站一条）。
struct DueEntry {
    /// 组键（站下标 + 组锚）
    key: GroupKey,
    role: Role,
    /// **组周期**（= 组内块的有效周期；非退化配置下承载组 ≡ 站周期）
    interval_ms: u64,
    /// 下次到期时刻（uptime 毫秒；由调用方 now_ms 单调驱动，独立于真时钟）
    next_due: u64,
    /// 本组是否站级承载组（C8；排序键用，§12.4.2）
    is_carrier: bool,
    /// **组级**连续失败计数（仅块级组自增；承载组的失败由站级 `offline_count` 记账，§12.5）
    group_fail_count: u32,
}

/// 口内到期排程（纯逻辑，无 IO、无真时钟依赖，可单测）。
///
/// 每口一个实例（口间独立 cadence：spawn 时各口 task 各自持有；见 [`SouthScheduler`]）。
/// 到期判定：`now_ms >= next_due`；推进：`next_due += interval`，落后一轮以上（
/// `now_ms >= next_due + interval`）钳制为 `now_ms + interval`（防追跳补采）。同一
/// `now_ms` 重复调用已到期组不再返回（next_due 已推过）——保证每轮每组至多采一次。
pub struct DueCalc {
    entries: Vec<DueEntry>,
}

impl DueCalc {
    /// 由本口站组（`(state 下标, conf)`，下标为 state Vec 全局序）构造。
    /// **每站产 [`read_groups_of`]`.len()` 个条目，且恒 ≥ 1**（单组站与**空 `regs` 站**各 1 个，
    /// 同既有；§12.4.1 的"至少一组"不变量）。
    /// 决议：初 `next_due = 0` → 首轮全部立即到期（启动即采一次），此后按**组周期**排程。
    fn from_group(group: &[(usize, &StationConf)]) -> Self {
        let mut entries = Vec::new();
        for (idx, c) in group {
            // `carrier_group` 返回 `Option`（S-1）：不变量保证 `Some`；`unwrap_or` 仅作
            // **无 panic 兜底** —— 真取到 `None` 时 `read_groups_of` 亦为空 ⇒ 下面的循环不执行，
            // 该值不被使用（§12.4.2 的伪码注）。
            let carrier = carrier_group(c)
                .map(|g| g.anchor_blk)
                .unwrap_or(EMPTY_GROUP_ANCHOR);
            for g in read_groups_of(c) {
                entries.push(DueEntry {
                    key: GroupKey {
                        station_index: *idx,
                        anchor_blk: g.anchor_blk,
                    },
                    role: c.role,
                    interval_ms: g.interval_ms,
                    next_due: 0,
                    is_carrier: g.anchor_blk == carrier,
                    group_fail_count: 0,
                });
            }
        }
        Self { entries }
    }

    /// 给定 now_ms（uptime 单调毫秒），返回本口「已到期的读组」，并推进到期组 next_due。
    /// 返回序：`(role 优先级, 站序, **!is_carrier**, 组锚)` 稳定排序。
    /// 单组站（含空 `regs` 站）每站恰 1 条目、且恒为该站承载组 ⇒ 键退化为
    /// `(role 优先级, 站序)`，与改造前的 `(role 优先级, 条目序)` **逐项等价**（§12.3 V-5）。
    pub fn due_round(&mut self, now_ms: u64) -> Vec<GroupPoll> {
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
        // 排序键的第三项 `!is_carrier` ⇒ **站内承载组恒排最前**（S-3 修订）：使"站恢复当轮的
        // 全组基线重建"发生在本站其它组的本轮产出**之前**；否则排在其后的快组会先用**离线前**
        // 的基线产出一屏事件，且下一轮还会多一次全量快照（论证见 §12.4.2；该细化与 PRD §10.6
        // 第 3 条字面元组的差异已登记为 Δ-14，**只定同 tick 先后、不引入并发**）。
        // 组锚为末位键：块 → 组 1:1 ⇒ 站内组序总被完全决定（`sort_by_key` 稳定，无并列歧义）。
        due.sort_by_key(|&i| {
            let e = &self.entries[i];
            (
                role_priority(e.role),
                e.key.station_index,
                !e.is_carrier,
                e.key.anchor_blk,
            )
        });
        due.into_iter()
            .map(|i| {
                let e = &self.entries[i];
                GroupPoll {
                    station_index: e.key.station_index,
                    anchor_blk: e.key.anchor_blk,
                    is_carrier: e.is_carrier,
                    was_failing: e.group_fail_count > 0,
                }
            })
            .collect()
    }

    /// 退避：把该组 next_due 后移到 `now_ms + extra_ms`（若现 next_due 已更晚则不动）。
    /// scheduler 在组 poll 失败后调用（承载组 = 站级退避 §10.2 M-11；块级组 = 组级退避 §12.5）。
    /// 签名与语义逐字沿用既有 `delay_station`（只把"站"换成"组"）。
    pub fn delay_group(&mut self, key: GroupKey, now_ms: u64, extra_ms: u64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.key == key) {
            let target = now_ms.saturating_add(extra_ms);
            if e.next_due < target {
                e.next_due = target;
            }
        }
    }

    /// 组级失败计数 +1，并**原子返回** `(组周期, 加一后的失败计数)`（= 退避公式入参）。
    /// **键不存在 ⇒ `None`（不 panic）** —— 使"计数 +1"与"取入参"合成一步，调用方无需二次
    /// 查表（那会引入一次 `unwrap`，与 §12.4.1 的 S-1 取向相悖；§12.4.2 的注）。
    pub fn bump_group_fail(&mut self, key: GroupKey) -> Option<(u64, u32)> {
        let e = self.entries.iter_mut().find(|e| e.key == key)?;
        e.group_fail_count = e.group_fail_count.saturating_add(1);
        Some((e.interval_ms, e.group_fail_count))
    }

    /// 组级失败计数清零（本组本轮成功；§12.4.3）。
    pub fn clear_group_fail(&mut self, key: GroupKey) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.key == key) {
            e.group_fail_count = 0;
        }
    }

    /// 读该组的 `(组周期, 组级失败计数)` —— 退避公式的入参（替代既有从 `state` 取
    /// `(interval_ms, offline_count)`）；**键不存在 ⇒ `None`（不 panic）**。
    pub fn group_backoff_input(&self, key: GroupKey) -> Option<(u64, u32)> {
        self.entries
            .iter()
            .find(|e| e.key == key)
            .map(|e| (e.interval_ms, e.group_fail_count))
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
/// battery 站 SOC 越界的事件名（§11.4.7 事件 ①；PRD §9.6.3）。
const SOC_OUT_OF_RANGE: &str = "soc_out_of_range";
/// 消防探测器登记数不一致的事件名（§11.4.7 事件 ③；PRD §9.5.4"登记数交叉校验"）。
const DET_COUNT_MISMATCH: &str = "fire_detector_count_mismatch";

/// 某站的**变化沿记忆**（S3b-2 §11.4.7.1「统一事件模型」的实现 —— `PortRunner` 按站下标各持一个）。
///
/// **统一口径**：一个"信号"在 tracker 里占一格"上轮活跃态"，无论它是
/// ① 离散位点（`Bit`：`func: discrete` 块的位向量第 k 位）、
/// ② 字级信号（`WordBit`：整字 `& mask ≠ 0`；`WordEnum`：整字 ∈ `active` 值集），还是
/// ③ 第 5 类站级派生布尔量（`StationFlag`，见 [`EdgeTracker::mark_station_flags`]）。
/// 每轮算出 `(上轮, 本轮)` 二元组：`false→true` = 进入活跃（`value = 1.0`）、
/// `true→false` = 退出活跃（`value = 0.0`），**两者都返回**（是否采纳由调用方过滤，
/// 见 [`SouthScheduler::poll_group`] 与 [`edges_to_events`] 的不对称过滤）。
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

/// 口运行时：bus（open 失败 → None，该口全站 offline）+ 本口独立 DueCalc + 各**组**变化沿记忆。
struct PortRunner {
    bus: Option<Arc<dyn StationBus>>,
    calc: std::sync::Mutex<DueCalc>,
    /// **变化沿记忆：键由站下标改为 [`GroupKey`]**（离散位 + 字级信号 + 站级量共用，§11.4.7.1）。
    ///
    /// **必须换键**（§12.4.4 的机制论证）：`EdgeTracker::prime` 的动作是 `last.clear()` 再插入
    /// 本轮观测 ⇒ 若仍按**站**存，快组与慢组会**互相清空记忆**（快组先 prime 清掉标量记忆、
    /// 慢组再 prime 清掉位记忆）⇒ 位块的真实 0→1 跳变被**静默吞掉**（每 5 s 复发一次）。
    trackers: std::sync::Mutex<HashMap<GroupKey, EdgeTracker>>,
    /// 站下标 → 该站的**全部组键**（**承载组**判定"站恢复"时须"重建全部组基线"，§12.4.4 连带项 a）。
    /// **只有承载组**会用到它 —— S-3 修订：非承载组不得触发站级全组重建。
    groups_of_station: HashMap<usize, Vec<GroupKey>>,
    /// 组键 → 组内块在 `StationConf::regs` 中的下标（构造期由 [`read_groups_of`] 一次算好，
    /// `poll_group` 直接查表 ⇒ 免每轮重新分组；也是"块 → 组"的唯一权威映射）。
    /// **空 `regs` 站的退化组映射到空块集** ⇒ `poll_group` 的读循环 **0 次**、
    /// `poll_to_result(role, &[])` 照常求值（与既有空 `regs` 站逐字等价，§12.3 V-5(b)）。
    group_of: HashMap<GroupKey, Vec<usize>>,
    /// **消防钢瓶气压"本站曾出现过非 0"的站下标集合**（§11.7.3 / PRD §9.7.6）：
    /// 一旦入集合**不再移除**（钢瓶气压不会在业务上"变回未配置"）。该语义是**站级**的
    /// ⇒ 键仍为站下标，不随"组"细化。改 `station.rs` 的方案被设计否决（§11.3 末行：
    /// "本设计放 `PortRunner`"）。
    cylinder_seen_nonzero: std::sync::Mutex<HashSet<usize>>,
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

/// 汇总**一个读组**本轮的**块内信号**：离散位点（FC02 位向量逐点）+ 字级信号
/// （消防整字 `字 & mask` / `字 ∈ active`）。
///
/// **三类信号共用同一条产出路径**（`EdgeTracker`），差别只在活跃判据的求值处与
/// （`StationFlag` 独有）**产出频次口径**：位点的活跃 = 位向量第 k 位；字级信号的活跃 =
/// `SignalPick::is_active(整字)`；站级量的活跃 = 判据函数 `is_some()`。
///
/// **S3b-3（T9）的机械拆分**：本函数是既有 `round_signals` 的**前半段**（只读本组读集、
/// **不跨块求判据**），对**任何**读组都安全 ⇒ 位/标量遥测与变化沿事件对**任意组恒产出**
/// （§12.4.3 的"三句话"第 3 条）。第 5 类站级量（跨块求值，缺块即假报）**不在本函数**，
/// 由 [`round_station_flags`] 单独产出、且**只在承载组**上求值。**判据零改动**。
fn round_signals_group(role: Role, reads: &BlockReads) -> RoundSignals {
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
    rs
}

/// 第 5 类信号 `StationFlag`（§11.4.7.1 末行 / §11.4.7.2 C）：判据是 `mapper` 的交叉校验 /
/// 域检查的**布尔返回**（无寄存器、非跃迁量）。**逐 role 求值且跨块** ⇒ 缺块即**假报**
/// ⇒ **只由承载组调用**，且调用点前置 [`judges_evaluable`] 守卫（§12.4.5）。
///
/// 本函数是既有 `round_signals` 的**后半段机械拆分**（判据一字未改）：① SOC 域检查
/// （PRD §9.6.3）+ ⑤ 消防地址升序违规（Q-9）与 ③ 登记数交叉校验（PRD §9.5.4）。返回值由
/// 调用方**同栏**喂进 `RoundSignals.all`（既有口径，"喂进同一个 `RoundSignals.all`"）。
fn round_station_flags(role: Role, reads: &BlockReads) -> Vec<StationFlag> {
    let mut flags: Vec<StationFlag> = Vec::new();
    // ① SOC 域检查（PRD §9.6.3 / §11.4.7 事件 ①）：判据是 `mapper::soc_in_domain` 的布尔
    // 返回，**判据本身不是跃迁量** ⇒ 按其布尔态喂进同一个 EdgeTracker（产出频次 = 状态
    // 翻转，见 `station_flag_events`）。进入事件的 `value` = **越界原值**（诊断量）。
    // 域检查与控制链取值**同源**（都来自 `battery_soc`），避免"两处各解一次"漂移。
    if role == Role::Battery {
        if let mapper::SocOutcome::Value(v) = mapper::battery_soc(reads) {
            flags.push(StationFlag {
                metric: SOC_OUT_OF_RANGE,
                active: !mapper::soc_in_domain(v),
                diag: v,
            });
        }
    }
    // ⑤ 消防探测器地址升序违规（Q-9）+ ③ 登记数交叉校验（PRD §9.5.4"强制"）：同①，
    // 判据是 mapper 的交叉校验返回，**判据本身不是跃迁量** ⇒ 同栏喂进同一个 EdgeTracker。
    // 非 fire 站无此判据（`mapper` 对非 fire 直接返回 None）⇒ 不喂信号、不占记忆格。
    if role == Role::Fire {
        let violation = mapper::fire_detector_addr_order_violation(role, reads);
        flags.push(StationFlag {
            metric: ADDR_ORDER_INVALID,
            active: violation.is_some(),
            diag: violation.map(|(group, _addr)| group as f64).unwrap_or(0.0),
        });
        let mismatch = mapper::fire_detector_mismatch(role, reads);
        flags.push(StationFlag {
            metric: DET_COUNT_MISMATCH,
            active: mismatch.is_some(),
            diag: mismatch.unwrap_or(0.0), // ③ 进入事件的 value = **读回登记数**
        });
    }
    flags
}

/// `fire` 判据的**链首可得性**：`reads` 中存在**读成功且覆盖寄存器 11**
/// （[`mapper::FIRE_DET1_ADDR_REG`]，探测器 1 的**地址寄存器**）的**寄存器块**。
///
/// **与 `mapper::fire_chain_head`（私有）同判**：`addr ≤ 11 < addr + count` 且 `res.regs()`
/// 可取（位块取不到寄存器 ⇒ 不能当链首）；**复用同一常量**（`mapper::FIRE_DET1_ADDR_REG`
/// ⇒ 单一真源）。**不得**改写成"块名 == `fire_sys`"（那属设备特判，违反 G-5；§12.4.5 的注）。
fn fire_head_present(reads: &BlockReads) -> bool {
    reads.iter().any(|(b, res)| {
        let det1 = usize::from(mapper::FIRE_DET1_ADDR_REG);
        let start = usize::from(b.addr);
        let covered = start <= det1 && det1 < start + usize::from(b.count);
        covered && res.as_ref().ok().and_then(|d| d.regs()).is_some()
    })
}

/// **判据完整性守卫**（运行期对偶，纵深防御；设计 §12.4.5）：
/// 组内是否齐备"站级判据所需的块"（= PRD §10.3.2 的 `R(role)`）。
///
/// **作用域（B-1 修订，唯一判据）**：**只**管「判据 / 站级量」路径（[`round_station_flags`]）；
/// **不**管位 / 标量遥测与事件（后者对任意读组都安全 —— §12.4.3 的"三句话"第 3 条）。
/// 调用点唯一：`if poll.is_carrier && judges_evaluable(role, &reads) { round_station_flags(..) }`。
/// ⚠️ **本返回值不得用于门控"位 / 标量遥测与事件"**：非承载组天然不含 `R(role)` 的块
/// （battery 的 `bms_alarm` 快组不含 `soc` 块），误扩作用域 ⇒ 该组全部位/标量遥测与事件被
/// **静默丢弃**（由 §12.8 的 `non_carrier_group_still_emits_its_bits` 钉住）。
///
/// **与配置侧同源**：本函数是 `config::criterion_block_indices`（T8，`R(role)` 的配置侧求值域）
/// 的**运行期求值域** —— 同一份 `R(role)` 定义，一侧判"哪些块下标在 R 内"、一侧判"这些块在
/// 本组读集里是否读成功"。**不得另立第二套口径**。
///
/// **总口径（各 role 的"可用性"按各自自然口径判定）**：本函数回答的是一个统一命题
/// —— "**`R(role)` 本轮可求值**"，但它**按 role 具体化**，不强行统一成同一字面谓词：
///
/// | role | "本轮可求值"的具体化 |
/// |------|----------------------|
/// | `MeterGrid` | `R` 的**五块**（`p/q/pf/u/i`，+ `p_total`）**存在且 `is_ok()`** |
/// | `Battery` | **`soc` 点本轮可解出**（`SocOutcome::Value` = 承载块在组内 ∧ 读成功 ∧ 长度足够） |
/// | `Fire` | 链首（覆盖寄存器 11 的寄存器块）+ 至少一个 `fire_det*` 块 |
///
/// ⚠️ **不得**为追求"字面统一"而把两侧改成同一谓词：`Battery` 若只看"承载块 `is_ok()`"，
/// 就会把**读回截断**（响应长度装不下 `soc` 点，`mapper` 判为 `BlockFailed`）误判为"可求值"
/// —— 那与 `MeterGrid` 侧"块 `is_ok()` 但相量长度不足"同样不该算可求值。**故
/// `runtime_guard_and_config_criterion_agree` 的"集合相等"比的是 `R` 的**块集合**
/// （配置侧 `criterion_block_indices` ⟷ 运行期"哪些块必需"），"可用性"则各按自然口径判。
///
/// **在承载组作用域内、配置合法时恒 `true`**（V-2 由配置期 C6 保证）；本守卫是纵深防御，
/// 为两种"配置期保证失效"的场合兜底：① 配置校验被绕过（直接构造 `StationConf` 的既有用法）；
/// ② 将来放开 C6/C7（PRD §10.9 **Q-22** 选项 B）。**守卫失败 = "跳过求值"**（本轮不产任何
/// 站级量事件；本组的位/标量遥测**照常产出**），**不是**"丢弃本组数据"。
fn judges_evaluable(role: Role, reads: &BlockReads) -> bool {
    match role {
        // `p/q/pf/u/i/p_total` 齐备 —— **与配置侧 `criterion_block_indices` 逐字同集**
        // （PRD §10.3.2 的 `R(role)` 定义表是**唯一权威口径**且含 `p_total`，其注写明
        // "`p_total` 供 `scalar_total` 降级求和"）。缺 `p_total` 时 `poll_to_result` 只**降级**
        // （不 `Failed`），但本守卫判的是"判据块是否齐备"而非"是否 `Failed`" ⇒ 两侧必须同集，
        // 不得各留一套口径（由 `runtime_guard_and_config_criterion_agree` 钉住）。
        Role::MeterGrid => ["p", "q", "pf", "u", "i", "p_total"]
            .iter()
            .all(|n| reads.iter().any(|(b, r)| b.name == *n && r.is_ok())),
        // 语义 = "**该 role 的站级判据本轮可求值**"（"`soc` 点本轮可解出"）—— **复用
        // `mapper::battery_soc` 的四情形**，不另造判据。
        // ★ 只认 `SocOutcome::Value`：`BlockFailed` 的**两种形态**（① 承载块 `Err(e)`；
        //   ② **读回长度装不下该点** = 响应被截断）**都必须返回 `false`** —— 截断与"块读
        //   失败"同属"本轮求不出该判据"，不得当成"站在线但 SOC 缺失"而放行求值。
        //   （旧写法 `!matches!(.., NoSuchPoint)` 会把 ② 误判为 `true`，与 `MeterGrid` 侧
        //   "块须 `is_ok()`"的语义不对称。）
        // ★ 可达性：运行期 `reads` **恒为全 `Ok`** —— `poll_group` 的读循环在组内任一块
        //   `Err` 时即 `break` 并**整组弃用**，`Err` 从不进 `reads` ⇒ `BlockFailed` 的 ① 形态
        //   在运行期不可达。可达的只有 ②，而 ② 若发生在承载组上，`poll_to_result` 已先将其
        //   拦成 `Failed`（早返回，见其调用点）⇒ 本支路是**纵深防御**（非承载组按 C6/C7 不含
        //   `soc` 块 ⇒ 亦不可达）。语义仍须精确：守卫就是"配置期保证失效"时的兜底。
        Role::Battery => matches!(mapper::battery_soc(reads), mapper::SocOutcome::Value(_)),
        // 链首（覆盖寄存器 11 的**寄存器块**）+ 至少一个 `fire_det*` 块（地址序与登记数两个
        // 判据**共用**这两类输入，mapper.rs 的 `fire_chain_head` / `fire_detector_mismatch`）
        Role::Fire => {
            fire_head_present(reads) && reads.iter().any(|(b, _)| b.name.starts_with("fire_det"))
        }
        // MeterBatt / Hvac / Pcs：`poll_to_result` 返回空包，无站级判据 ⇒ 恒真
        Role::MeterBatt | Role::Hvac | Role::Pcs => true,
    }
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
            // 分组表（组键 → 组内块下标；站 → 全部组键）**构造期一次算好**（§12.2.2）：
            // 与 `DueCalc::from_group` 共用同一个 `read_groups_of` ⇒ "块 → 组"与"组 → 块"
            // 两处不可能漂移（也是"免每轮重新分组"的落点）。
            let mut groups_of_station: HashMap<usize, Vec<GroupKey>> = HashMap::new();
            let mut group_of: HashMap<GroupKey, Vec<usize>> = HashMap::new();
            for (idx, c) in &group {
                let keys = groups_of_station.entry(*idx).or_default();
                for g in read_groups_of(c) {
                    let key = GroupKey {
                        station_index: *idx,
                        anchor_blk: g.anchor_blk,
                    };
                    keys.push(key);
                    group_of.insert(key, g.blk_indices);
                }
            }
            runners.push(PortRunner {
                bus: buses.get(port).cloned(),
                calc: std::sync::Mutex::new(DueCalc::from_group(&group)),
                trackers: std::sync::Mutex::new(HashMap::new()),
                groups_of_station,
                group_of,
                cylinder_seen_nonzero: std::sync::Mutex::new(HashSet::new()),
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

    /// 跑一口的一个 tick（now_ms 驱动 due → 逐**到期组** poll → 失败组退避）。
    /// **结构同既有**：`due` 空则直接返回（**零空转**）。
    async fn run_port_round(&self, port_i: usize, now_ms: u64) {
        let runner = &self.runners[port_i];
        let due = runner.calc.lock().unwrap().due_round(now_ms);
        if due.is_empty() {
            return;
        }
        let mut failed: Vec<GroupPoll> = Vec::new();
        for poll in due {
            let ok = self.poll_group(runner, poll).await;
            // 显式构造组键（§12.2.2 的优化 4 之一：不声明 `From<GroupPoll> for GroupKey`）
            let key = GroupKey {
                station_index: poll.station_index,
                anchor_blk: poll.anchor_blk,
            };
            let mut calc = runner.calc.lock().unwrap();
            if ok {
                calc.clear_group_fail(key);
            } else {
                failed.push(poll);
            }
        }
        if failed.is_empty() {
            return;
        }
        // M-11 退避（§12.4.3 的末段）：
        //  · **站级承载组** → 入参 = (**承载组自身组周期**, 站级 `offline_count`)；
        //    `offline_count` 已由 `poll_group` 内的 `handle_failure` +1。**非退化配置**下
        //    组周期 ≡ 站周期（C5+C7 ⇒ 承载组 = 站周期组）⇒ 与既有逐字等价；只在**退化配置**
        //    （站内全部块都声明了 `interval_ms`，PRD §10.6 第 8 条）下二者不同。
        //  · **块级组**     → 入参 = (组周期, **组级** fail_count)（`bump_group_fail` **原子**返回两者）。
        // 先一次锁 state 快取 `offline_count`，**勿持 state 锁跨 calc 锁**（既有约定）。
        //
        // ★ **按站下标直取**（评审 修 2）★ 快照 = `state` 的**全量** offline_count，下标即站下标
        //   ⇒ `station_oc.get(p.station_index)` 一步命中。**不得**退回"按 `failed` 拷成
        //   `(si, oc)` 再线性 `find(..).unwrap_or(0)`"的写法：那使**站缺失时 `oc` 静默退化为
        //   `0`**（既不报错也不告警），退避被悄悄压成 1×。快照长度 = `state.len()`，而
        //   `state` 与 `DueCalc` 的站条目**同源**（`SouthScheduler::new` 里都由 `cfg.stations`
        //   的下标构造）⇒ `p.station_index` 必在；仍**不以 panic 表达**（取向同 §12.4.1/§12.4.2
        //   的 `Option` 查表）。
        let station_oc: Vec<u32> = {
            let st = self.state.read().unwrap();
            st.iter().map(|s| s.offline_count).collect()
        };
        let mut calc = runner.calc.lock().unwrap();
        for p in failed {
            let key = GroupKey {
                station_index: p.station_index,
                anchor_blk: p.anchor_blk,
            };
            // `key` 必存在（来自本 tick 的 `due_round`，条目由 `from_group` 建成）⇒ 两方法恒
            // `Some`；但**一律 `if let`、不 `unwrap`**（§12.4.2 的注：查表失败不以 panic 表达）。
            if p.is_carrier {
                if let Some((iv, _)) = calc.group_backoff_input(key) {
                    // 承载组失败：站级 offline 记账已在 `poll_group` 内完成（handle_failure）
                    //
                    // 缺站下标 ⇒ **不静默**（评审 修 2）：`warn!` 一次并按 **`oc = 1`**（= 首次
                    // 失败口径 ⇒ `extra = 1 × 组周期`，即**最短**退避）处理。选最短退避而非更长：
                    // ① `oc` 的语义下界就是 1（计数自首次失败起算，**无 0 态**）；② 本条路径只在
                    // "快照与下标空间不一致"这一结构性故障下可达，按最短退避重试可**尽早重新观测**，
                    // 不会把故障放大成"更晚才发现"（也不给"永久停采"留机会 —— M-11 的取向）。
                    let oc = match station_oc.get(p.station_index) {
                        Some(oc) => *oc,
                        None => {
                            tracing::warn!(
                                station_index = p.station_index,
                                "southd 站级退避取 `offline_count` 失败（state 快照缺该站下标）⇒ 按首次失败口径（1 × 组周期）退避"
                            );
                            1
                        }
                    };
                    // 退避公式抽离为 backoff_extra（纯函数，封顶/边界见 tests 单测）。
                    // config::validate 已强校验 interval_ms>0 且此处 oc≥1 → extra 恒 > 0，
                    // 原 `if extra > 0` 守卫冗余已去（delay_group 不空转）。
                    calc.delay_group(key, now_ms, backoff_extra(iv, oc));
                }
            } else if let Some((iv, fc)) = calc.bump_group_fail(key) {
                // 块级组 → **组级**计数 +1 后按**组周期**指数退避（先 bump 再取，同一原子返回）
                calc.delay_group(key, now_ms, backoff_extra(iv, fc));
            }
        }
    }

    /// **单组一轮采集**：逐**组内块**读 → mapper 判定 → 分发 + 调度态更新。
    ///
    /// 读循环 / 失败处理 / 成功记账 / mapper 调用**与既有 `poll_station` 逐字相同**，只把
    /// "整站全部块"换成"组内块"（§12.4.3）。任一块读 Err（物理层）或 `PollResult::Failed`
    /// （语义层）→ 该组本轮失败。同口串行由口 task 单 poller 保证（本方法不并发）。
    ///
    /// 返回该组本轮是否成功。false = 失败（**承载组**：offline 记账 + 事件已由内部处理；
    /// **块级组**：只 `warn`，不产事件 —— 调用方据此按组退避，§12.5）。
    async fn poll_group(&self, runner: &PortRunner, poll: GroupPoll) -> bool {
        let si = poll.station_index;
        let key = GroupKey {
            station_index: si,
            anchor_blk: poll.anchor_blk,
        };
        let (station_id, role, slave, blk_indices) = {
            let st = self.state.read().unwrap();
            let s = &st[si];
            // 构造期算好的「组 → 组内块下标」表（免每轮重新分组；空 `regs` 站 ⇒ 空块集）。
            //
            // ★ **查表失败不 panic**（评审 修 1）★ 取向与本章对 `DueCalc` 的查表**一致**
            //   （§12.4.1 / §12.4.2：查表失败**不以 panic 表达**，恒用 `Option` 退化）：
            //   缺键 ⇒ 退化为**空块集**，与既有 `poll_to_result(role, &[])` 的退化语义**自洽**
            //   （读循环 0 次 = 空转；空 `regs` 站的退化组**本就**走这条路，§12.3 V-5(b)）。
            //   `group_of` 与 `DueCalc::from_group` **同源**（构造期共用同一个 `read_groups_of`）
            //   ⇒ 键必在；`debug_assert!` 使"两表漂移"在 **debug 构建下即刻可见**，
            //   而 **release 行为不变**（仍走空块集退化、不 panic）。
            debug_assert!(
                runner.group_of.contains_key(&key),
                "southd: `group_of` 缺组键（与 `DueCalc::from_group` 应同源共用 `read_groups_of`）"
            );
            let blk_indices = match runner.group_of.get(&key) {
                Some(v) => v.clone(),
                // 缺键 ⇒ 空块集（**不 panic**；debug 构建下上面的 `debug_assert!` 已先行暴露漂移）
                None => Vec::new(),
            };
            (s.conf.id.clone(), s.conf.role, s.conf.slave, blk_indices)
        };
        // **读前**的站离线态 —— 必须在 `mark_success` 清 `offline_count` **之前**取（既有约定）：
        // 站恢复后（该站**全部组**）变化沿基线必须重建，否则"恢复即刷一屏事件"（§12.4.4 连带项 a）。
        let station_was_offline = { self.state.read().unwrap()[si].offline_count > 0 };

        // 逐**组内块**读（阻塞 IO 由 StationBus 内 spawn_blocking 承载——全 async 无阻塞）。
        let mut reads: BlockReads = Vec::with_capacity(blk_indices.len());
        let mut io_error: Option<String> = None;
        if let Some(b) = &runner.bus {
            for &bi in &blk_indices {
                let blk = &self.cfg.stations[si].regs[bi];
                // 按块 func 分发读方法：Holding → FC03 read_holding，Input → FC04 read_input，
                // Discrete → FC02 read_discrete（读结果按 `BlockData` 统一承载）。
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
                        // 失败语义（§10.7）：组内任一块读失败 → 该组本轮无有效数据，不部分交付
                        // ——已读 Ok 块随失败路径整体弃用（io_error 即返回，reads 丢弃）。
                        // 首块错误即 break → 钳制同口 cadence 受损上界（不为该组耗尽本轮预算，
                        // 尽快回到同口其它组）。故 `reads` 从不带 Err 条目进 mapper/telemetry_points
                        // ——mapper 里跳过 Err 块的分支为「多块组扩展时部分交付」预留，与模块头
                        // 「组内整批、无部分交付」语义一致。
                        io_error = Some(format!("{} @ {:#06x} x{}", e, blk.addr, blk.count));
                        break;
                    }
                }
            }
        } else {
            io_error = Some("口未打开（open 失败）".into());
        }

        // ── 失败路径（PRD §10.6 第 4 条）──
        if let Some(reason) = io_error {
            if poll.is_carrier {
                self.handle_failure(si, &reason).await; // 站级 offline 记账 + 事件（既有，零改动）
            } else {
                // **块级（快采）组失败：不产 `offline`/`online`、`offline_count` 不自增**（§12.5）。
                // 两条硬理由：① 站被判 offline ⇒ 12 号 F25.4 令该站**全部**点显示"站离线"，
                // 会把**刚刚成功采到并上送的快采告警位**一并遮蔽；② `mark_success` 会重置
                // offline 去抖窗口 ⇒ 快/慢组交替成败时每轮"online+offline"对刷屏（≈3.5 万条/日）。
                // 字段按 §12.5 的语义表给全（`station/role/块名/reason`）——块名由**组锚 + 组内
                // 块下标**（`group_of` 表，见上方 `blk_indices` 的取法）查 `cfg` 得；空 `regs` 站
                // 的退化组无块可列 ⇒ 记 `<空块集>`（不省略字段，防水位却缺块的日志失真）。
                let blocks: String = if blk_indices.is_empty() {
                    "<空块集>".to_string()
                } else {
                    blk_indices
                        .iter()
                        .map(|&bi| self.cfg.stations[si].regs[bi].name.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                };
                tracing::warn!(station = %station_id, ?role, anchor_blk = poll.anchor_blk, blocks = %blocks, reason,
                    "southd 块组采集失败（组级退避；不升级为站级 offline）");
            }
            return false;
        }

        // ── 语义判定（**只在承载组上**）—— C6/C7 保证 `R(role) ⊆ 承载组`
        //    ⇒ 其读集与"改造前的整站一轮"在 R 相关块上**等价**（其余 role 返回空包）──
        //    对快组调用它会**恒 Failed ⇒ 假 offline**（`MeterGrid` 缺相量块 / `Battery` 缺
        //    `soc` 块），§12.4.3 的注。
        if poll.is_carrier {
            match mapper::poll_to_result(role, &reads) {
                PollResult::Failed(msg) => {
                    self.handle_failure(si, &msg).await;
                    return false;
                }
                PollResult::Data(pkg) => {
                    self.mark_success(si).await;
                    if role == Role::MeterGrid {
                        self.sink.on_grid_package(pkg).await;
                    } else if role == Role::Battery {
                        // SOC 双源通道（04 §2.11.1）：battery 站 pkg.battery.soc 由 mapper 解码；
                        // 本轮采集成功即新鲜 → 独立推给 AiIntegrator（BMS 优先源）。落库照旧。
                        if let Some(soc) = pkg.battery.soc {
                            self.sink.on_battery_soc(&station_id, soc).await;
                        }
                    }
                }
            }
        }

        // ── 遥测 / 事件（承载组与块级组**同一路径**；grid 站不走此路，同既有）──
        //
        // ★★ **守卫的作用域（B-1 修订）** ★★ `judges_evaluable` **只**门控「**判据 / 站级量**」
        //    这一条路径（`StationFlag`：SOC 域 / 消防地址升序 / 消防登记数交叉校验 —— 这些量
        //    **跨块**求值，缺块即**假报**，见 §12.4.5）；**位 / 标量遥测与事件产出不受它门控**。
        //    反例（旧写法为何错）：把守卫套在**整段**上 ⇒ 对**非承载组**（battery 的 `bms_alarm`
        //    快组**天然不含** `soc` 块）恒 `false` ⇒ 该组**全部位/标量遥测与事件被静默丢弃**
        //    （无日志、无事件），既与 §12.5「组内块只喂本组 tracker」自相矛盾，也使 PRD §10.3.2
        //    明文允许的 "`bms_alarm` 可提速" 失效 —— 由 `non_carrier_group_still_emits_its_bits` 钉住。
        if role != Role::MeterGrid {
            // ① 本组块**自身**的信号（离散位 + 字级信号）：只读本组读集、**不跨块求判据**
            //    ⇒ 对**任何**组都安全，**恒产出**（= "块级组照发遥测" 的落点）。
            let mut signals = round_signals_group(role, &reads);
            // ② 判据 / 站级量：**仅当**「本组 = 站级承载组」**且**「组内齐备 `R(role)`」时求值；
            //    否则**跳过求值**（不是"丢弃本组数据"，而是"本轮不产出站级量"）。
            //    非承载组不含 `R(role)` 的块是**正常形态**（battery 快组 / hvac 位组皆如此）。
            if poll.is_carrier && judges_evaluable(role, &reads) {
                signals.station_flags = round_station_flags(role, &reads);
                // 站级量与前四类**同栏**喂 tracker（既有口径，逐字沿用）
                signals.all.extend(
                    signals
                        .station_flags
                        .iter()
                        .map(|f| (f.metric.to_string(), f.active)),
                );
            }
            let (events, changed_bits) = {
                // ── 事件侧 + **位点落库节流**共用**本组**的 EdgeTracker（§11.4.7.1 + §12.4.4）──
                // 位点的"与上轮不同"就是 `edges()` 的 `prev != v`；首轮/重建后首轮取
                // **全量快照**（§11.4.7「每轮取与上轮不同的位/信号 + 首次/复位后全量」）。
                let mut trackers = runner.trackers.lock().unwrap();
                // ① **站级恢复** ⇒ **该站全部组**基线重建（§12.4.4 连带项 a）。
                //    ★★ 触发者**只能是承载组**（S-3 修订）★★：`station_was_offline` 单独**不足以**
                //    定触发 —— `offline_count` 只在**承载组**成功时被 `mark_success` 清零
                //    （`mark_success` 只在承载组分支调用），故**承载组持续失败**时该标志对非承载组
                //    **恒为真**；若据此触发"全组重建"，非承载组**每一轮**成功都会把自己的基线重置 ⇒
                //    (i) 该组 `primed` 恒 `false` ⇒ **0→1 变化沿事件永不产出**（与 §12.5 表
                //        "只受本组基线状态与组级失败重建影响"直接矛盾）；
                //    (ii) `full_snapshot` 每轮为真 ⇒ **每轮全量落位**（BMS 288 位/轮 ≈ 2.5×10⁷ 行/天，
                //         正是 PRD §9.8.1 末条明文告警的量级）。
                //    ⇒ 门控 = `is_carrier` **且** 读前 `offline_count > 0`（= 真正的"本轮恢复"）。
                //    ⇒ 承载组在站内**恒排最前**（§12.4.2 的排序键）⇒ 本段的重建**先于**该站其它组的
                //       本轮产出发生，不存在"先产出、后被重建"的半状态，也不会多一次全量快照。
                if poll.is_carrier && station_was_offline {
                    // ★ **查表失败不 panic**（评审 修 1）★ 取向同 §12.4.1/§12.4.2 的 `Option`
                    //   查表：缺站下标 ⇒ 退化为**空列表**（无组可重置），**不** panic ——
                    //   与上面 `group_of` 的"缺键 ⇒ 空块集"同款退化，两处风格由此一致。
                    //   `groups_of_station` 与 `group_of` **同源**（同一构造循环里一并建成）
                    //   ⇒ 键必在；`debug_assert!` 使漂移在 debug 构建下可见，**release 行为不变**。
                    debug_assert!(
                        runner.groups_of_station.contains_key(&si),
                        "southd: `groups_of_station` 缺站下标（应与 `group_of` 同源构造）"
                    );
                    if let Some(keys) = runner.groups_of_station.get(&si) {
                        for k in keys {
                            trackers.entry(*k).or_default().reset();
                        }
                    }
                }
                // ② **组级**：本组上一轮失败过 ⇒ 本轮只重建基线（不产事件，§12.5 的重建条件表）。
                //    ⚠️ 与 ① 是**两个独立来源**：非承载组自身的成功**不**触发站级全组重建。
                let tracker = trackers.entry(key).or_default();
                if poll.was_failing {
                    tracker.reset();
                }
                // `station_flags` 在非承载组上**恒为空**（上面 ② 分支已跳过求值）⇒ 本行对非承载组
                // 是幂等空操作；**不得**因它为空而短路整段（B-1 修订）。
                tracker.mark_station_flags(&signals.station_flags);
                // `edges()` 会 prime（此后 `primed = true`）⇒ 快照判定须在它之前取
                let full_snapshot = !tracker.primed;
                let raw_edges = tracker.edges(&signals.all);
                let changed: HashSet<String> =
                    raw_edges.iter().map(|(m, _, _)| m.clone()).collect();
                // 站级量：进入事件的 value 换诊断量、退出事件改名 `@recovered`（§11.4.7.2 C）
                let events = station_flag_events(
                    &signals.station_flags,
                    edges_to_events(&signals, raw_edges),
                );
                // 需落库的位点：全量快照轮 = **本组**全部位；其后 = 仅变化位（字级信号属**事件
                // 命名空间**，不是遥测点，故只取 `bits` 集合内的那些）。
                let bits: Vec<(String, f64)> = signals
                    .all
                    .iter()
                    .filter(|(m, _)| signals.bits.contains(m))
                    .filter(|(m, _)| full_snapshot || changed.contains(m))
                    .map(|(m, a)| (m.clone(), if *a { 1.0 } else { 0.0 }))
                    .collect();
                (events, bits)
            }; // 锁在 await 前释放（勿持锁跨 await）

            // 遥测落库（§11.7.2 第 1/2 条）：**标量每轮全量 + 位点仅变化沿**
            // （稳态位点写量 ≈ 0 —— D2 口径；"点产出"在 mapper，节流在本层）。
            // ★ **该段不受守卫门控** ⇒ **非承载组的位/标量遥测照常上送**（B-1 修订的落点：
            //   守卫只少产"站级量"，**不"静默丢弃整段"**）。
            let mut pts: Vec<(String, f64, bool)> = mapper::telemetry_points(role, &reads)
                .into_iter()
                .filter(|s| s.kind == mapper::SampleKind::Scalar)
                .map(|s| (s.metric, s.value, false))
                .collect();
            pts.extend(changed_bits.into_iter().map(|(m, v)| (m, v, false)));
            if !pts.is_empty() {
                self.sink.on_station_telemetry(&station_id, role, pts).await;
            }
            // 钢瓶气压"本站曾出现过非 0"记忆（§11.7.3 / PRD §9.7.6）：**只置位、不回退**
            // —— 展示层据此区分"未配置"与"真实 0 kPa"。该点**不产任何事件**。
            // **站级**语义 ⇒ 键仍为站下标（不随分组细化）。
            if role == Role::Fire {
                let mut seen = runner.cylinder_seen_nonzero.lock().unwrap();
                if !seen.contains(&si) && mapper::cylinder_pressure_configured(&reads, false) {
                    seen.insert(si);
                }
            }
            if !events.is_empty() {
                self.sink
                    .on_station_telemetry(&station_id, role, events)
                    .await;
            }
        }
        true
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

    /// **消防钢瓶气压"是否配置"的取数接缝**（§11.7.3 展示口径 / PRD §9.7.6 / §11.4.6）：
    /// 本站生命周期内该点（绝对寄存器 5）是否出现过非 0 值 ⇒ `false`（**未配置**）时
    /// 展示层须标"未配置"而非 "0 kPa"，且**不得**据此判"气压异常/泄漏"。
    ///
    /// **该点不产任何事件**（PRD §9.7.6 明令）⇒ 本接缝只服务展示侧，不参与事件/判据路径。
    /// `station_index` = `cfg.stations` 下标（与调度内部 state 下标同序）。
    pub fn cylinder_pressure_configured(&self, station_index: usize) -> bool {
        self.runners.iter().any(|r| {
            r.cylinder_seen_nonzero
                .lock()
                .unwrap()
                .contains(&station_index)
        })
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
            interval_ms: None,
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
                interval_ms: None,
            }],
        }
    }

    /// battery 站：点名式 `soc` 点落在 **FC04 输入寄存器 118**（PRD §9.4.1 参考配置的
    /// `bms_io` 形态）+ **`uint16`** —— SOC 域检查/越界事件的用例靠它注入 0–65535 原值
    /// （`battery_conf` 是 float32，只够测"域内"）。
    fn battery_soc_conf() -> StationConf {
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
                addr: 118,
                func: RegFunc::Input,
                format: RegFormat::Uint16,
                scale: 1.0,
                count: 1,
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
                interval_ms: None,
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
            interval_ms: None,
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
        /// 该站 on_battery_soc 的**推送次数**（"本轮是否推"只能靠增量断言，`soc_of` 是历次累计）
        fn soc_push_count(&self, station_id: &str) -> usize {
            self.battery_socs
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _)| id == station_id)
                .count()
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

        /// 该站**逐次**普通遥测上送（`is_event = false`）的点列表，按调用序 ——
        /// 供"第 N 次调用项数"/"某轮的项数"类断言（既有 `telemetry_of` 只给展平后的并集）。
        fn telemetry_calls_of(&self, station_id: &str) -> Vec<Vec<(String, f64)>> {
            self.msgs
                .lock()
                .unwrap()
                .iter()
                .filter(|(id, _, pts)| id == station_id && pts.iter().any(|&(_, _, ev)| !ev))
                .map(|(_, _, pts)| {
                    pts.iter()
                        .filter(|&&(_, _, ev)| !ev)
                        .map(|(m, v, _)| (m.clone(), *v))
                        .collect::<Vec<_>>()
                })
                .collect()
        }

        /// 该站普通遥测上送的**调用次数**（= `telemetry_calls_of` 的行数）
        fn telemetry_call_count(&self, station_id: &str) -> usize {
            self.telemetry_calls_of(station_id).len()
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
                    interval_ms: None,
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
                    interval_ms: None,
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
                    interval_ms: None,
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
                    interval_ms: None,
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
                    interval_ms: None,
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
                    interval_ms: None,
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
                    interval_ms: None,
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

    /// **① SOC 越界事件按"状态翻转"产出**（PRD §9.6.3 + 设计 §11.4.7 事件 ① + **§11.4.7.2 C
    /// 的 v1.7 口径**）：进入 1 条（`value` = **越界原值**）、稳态 0 条、恢复 1 条
    /// (`@recovered`, `value = 0.0`)、再次越界再产 1 条。**若沿用"命中即产"，同样的事件风暴
    /// 会原样重现**（越界持续 1h = 3600 条）—— 本用例是该口径的钉子。
    ///
    /// 同时钉住 PRD §9.6.3 的 ①/③：越界轮**不推** `on_battery_soc`（回落核间 SOC），
    /// 但 telemetry **仍按原值落库**（保留证据）。
    #[tokio::test]
    async fn soc_out_of_range_event_follows_state_flip() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![battery_soc_conf()], bus.clone(), sink.clone());
        let put = |v: u16| bus.put_input(2, 118, vec![v]);

        // 首轮：域内 65 ⇒ 基线，无事件、推 soc 一次
        put(65);
        sched.tick_once(0).await;
        assert!(sink.events_of("bms").is_empty(), "首轮只建基线");
        assert_eq!(sink.soc_of("bms"), Some(65.0));
        let pushes = sink.soc_push_count("bms");

        // 越界（65535）→ **首次观测即产 1 条**，value = 越界原值；且**不推** soc
        put(65535);
        sched.tick_once(1000).await;
        assert_eq!(
            sink.events_since("bms", 0),
            vec![("soc_out_of_range".to_string(), 65535.0)],
            "进入事件 value = 越界原值（诊断量）"
        );
        assert_eq!(
            sink.soc_push_count("bms"),
            pushes,
            "越界轮不得推 on_battery_soc（回落核间 SOC，防以坏值剪带）"
        );
        assert!(
            sink.telemetry_of("bms")
                .iter()
                .any(|(m, v)| m == "soc" && *v == 65535.0),
            "telemetry 仍按**原值**落库（§9.6.3 ③ 保留证据，不掩盖）"
        );

        // 判决式仍成立（连续轮）→ **一条都不产**（原"命中即产"= 3600 条/时）
        for t in 2..6u64 {
            put(65535);
            sched.tick_once(t * 1000).await;
        }
        assert_eq!(
            sink.events_of("bms").len(),
            1,
            "越界状态未变的轮次不重复产（状态翻转制）"
        );

        // 恢复域内 → 1 条 `@recovered`（value 恒 0.0），并恢复推 soc
        put(65);
        sched.tick_once(6000).await;
        assert_eq!(
            sink.events_since("bms", 1),
            vec![("soc_out_of_range@recovered".to_string(), 0.0)],
            "恢复可观测（否则事件日志分不清「仍越界」与「已修复」）"
        );
        assert_eq!(sink.soc_of("bms"), Some(65.0), "恢复域内即恢复推 soc");

        // 再次越界 → 再产 1 条进入事件（翻转是双向可重复的）
        put(1000);
        sched.tick_once(7000).await;
        assert_eq!(
            sink.events_since("bms", 2),
            vec![("soc_out_of_range".to_string(), 1000.0)],
            "再次越界再产 1 条（含新诊断量）"
        );
    }

    /// **① 首轮即越界 ⇒ 首次观测即产**（§11.4.7.2 C 对"首轮只建基线"的**唯一例外**：
    /// "上线时 SOC 就已越界"必须可见，且至多 1 条/站、不构成风暴）—— 与 ⑤/③ 同口径。
    #[tokio::test]
    async fn soc_out_of_range_first_observation_emits() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![battery_soc_conf()], bus.clone(), sink.clone());
        bus.put_input(2, 118, vec![65535]);
        sched.tick_once(0).await;
        assert_eq!(
            sink.events_of("bms"),
            vec![("soc_out_of_range".to_string(), 65535.0)],
            "首轮即越界 ⇒ 产 1 条进入事件"
        );
        assert_eq!(sink.soc_push_count("bms"), 0, "越界值不进控制链");
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

    /// **位点落 telemetry 按变化沿**（S3b-2 T6，§11.4.7 变化沿过滤行 + §11.7.2 第 1/2 条，
    /// D2 口径）：首轮/恢复后首轮**全量快照**一次，此后**稳态零写**，只有变化的位才再落
    /// ——"位点每轮全量落库"会形成 288 行/s（≈2490 万行/天）的写压，本用例把该形态封死。
    ///
    /// 同时钉住"**落库**与**事件**是两条路径"：`State` 位（位 0 内风机）**落 telemetry
    /// 但不产事件**（`BitClass::State`），而 `Alarm` 位（位 9）走事件（既有用例覆盖）。
    #[tokio::test]
    async fn bit_points_land_in_telemetry_only_on_change() {
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

        // 首轮（全 0）：31 位**全量快照**落库一次
        bus.put_bits(3, 0, vec![false; 31]);
        sched.tick_once(0).await;
        let n_first = sink.telemetry_of("hvac").len();
        assert_eq!(n_first, 31, "首轮 = 全量快照（位点基线）");

        // 状态未变 → 稳态零写
        sched.tick_once(1000).await;
        assert_eq!(
            sink.telemetry_of("hvac").len(),
            n_first,
            "位点未变 ⇒ 不落库（稳态写量 0）"
        );

        // 位 0（`State`：内风机）= 1 → **该位**补落一条（其余 30 位不落）
        let mut bits = vec![false; 31];
        bits[0] = true;
        bus.put_bits(3, 0, bits);
        sched.tick_once(2000).await;
        let tel = sink.telemetry_of("hvac");
        assert_eq!(tel.len(), n_first + 1, "只有变化的位补落一条");
        assert_eq!(
            tel.last(),
            Some(&("hvac_di_1".to_string(), 1.0)),
            "补落的是位 0（`hvac_di_1`）的新值"
        );
        assert!(
            sink.events_since("hvac", 0).is_empty(),
            "State 位不产事件（事件侧与落库侧分流，§11.7.2 第 3 条）"
        );

        // 位 0 回 0 → 再落一条（落库取**双向**变化沿，与事件侧的"只上升沿"刻意不同）
        bus.put_bits(3, 0, vec![false; 31]);
        sched.tick_once(3000).await;
        assert_eq!(
            sink.telemetry_of("hvac").len(),
            n_first + 2,
            "1→0 亦落一条（变化沿双向）"
        );
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
                    interval_ms: None,
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
                    interval_ms: None,
                },
            ],
        }
    }

    /// 预置 fire 站：`sys4` = 系统状态（addr 4）整字；`det` = 探测器区逐寄存器
    /// （`[地址, 状态, 数据1, CO, VOC, H2]` 每 6 个一组）。
    ///
    /// **`sys[6]` 的取值（S3b-2 T6 的 fixture 订正，断言不变）**：偏移 6 = 寄存器 10 =
    /// 契约点 `fire_det_count`（登记数）。T6 起 ③「登记数交叉校验」按 §11.4.7.2 的状态翻转
    /// 口径产事件 ⇒ 若 fixture 不填该寄存器（缺省 0），每轮都会命中不一致并插进既有用例的
    /// `events_since` 期望序列。故此处按**容量**填平，使「登记数」与「探测器容量」一致。
    ///
    /// **容量式（本轮订正）**：本 fixture 的 `fire_sys`（addr 4，`count: 13`）**覆盖寄存器
    /// 11** ⇒ **链首存在 ⇒ 容量 = `1 + det.len()/6`**（`1` = 探测器 1，它的 6 个寄存器在
    /// `fire_sys` 的 11–16 里，不在 `fire_det` 区）—— 漏掉这一项即"判据恒真、永久假告警"。
    /// 该文件其余断言**一字不改**。
    fn put_fire(bus: &MockBus, slave: u8, sys4: u16, det: &[u16]) {
        let mut sys = vec![0u16; 13];
        sys[0] = sys4; // 偏移 0 = addr 4 = 系统状态
        sys[6] = (det.len() / 6 + 1) as u16; // 偏移 6 = addr 10 = 登记数（= 容量，含探测器 1）
        bus.put(slave, 4, sys);
        bus.put(slave, 17, det.to_vec());
    }

    /// 预置 fire 站，并**显式给出探测器 1 的地址号（寄存器 11）**——`fire_sys` 块
    ///（addr 4）的偏移 7。地址升序链首的载体（v1.7 §11.4.6）。
    fn put_fire_det1(bus: &MockBus, slave: u8, sys4: u16, det1: u16, det: &[u16]) {
        let mut sys = vec![0u16; 13];
        sys[0] = sys4;
        sys[6] = (det.len() / 6 + 1) as u16; // 登记数 = 容量（见 `put_fire` 的 fixture 订正说明）
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
            sys[6] = 2; // 登记数 = 容量（1 + 6/6 = 2：含探测器 1；见 `put_fire` 的订正说明）
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
            sys[6] = 2; // 登记数 = 容量（1 + 6/6 = 2：含探测器 1；见 `put_fire` 的订正说明）
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
            sys[6] = 2; // 登记数 = 容量（1 + 6/6 = 2：含探测器 1；见 `put_fire` 的订正说明）
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
        sys[6] = 2; // 登记数 = 容量（1 + 6/6 = 2：含探测器 1；见 `put_fire` 的订正说明）
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

    /// **③ 消防登记数不一致事件按"状态翻转"产出**（PRD §9.5.4"登记数交叉校验（强制）" +
    /// §11.4.7 事件 ③ + §11.4.7.2 C 的 v1.7 口径，与 ⑤ 地址序**同口径**）：
    /// 进入 1 条（`value` = **读回登记数**）、稳态 0 条、恢复 1 条（`@recovered`, 0.0）。
    ///
    /// **判据的容量口径（本轮订正）**：本 fixture 的 `fire_sys`（addr 4，`count: 13`）覆盖
    /// 寄存器 11 ⇒ **链首存在 ⇒ 容量 = `1 + Σ(fire_det.count)/6`**（`1` = 探测器 1）。
    /// 漏掉 `+1` ⇒ 容量 3 ≠ 读回 4 ⇒ **判据恒真、永久假告警**（本用例的 `put(4)` 首轮即红）。
    #[tokio::test]
    async fn fire_detector_count_mismatch_follows_state_flip() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 3)], bus.clone(), sink.clone());
        let det = [1u16, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0];
        // 登记数（寄存器 10 = `fire_sys` 块内偏移 6）由用例显式给出
        let put = |count: u16| {
            let mut sys = vec![0u16; 13];
            sys[6] = count;
            bus.put(1, 4, sys);
            bus.put(1, 17, det.to_vec());
        };

        // 首轮：登记数 4 == 容量（1 + fire_det.count 18 ÷ 6 = 4）⇒ 一致，无事件
        put(4);
        sched.tick_once(0).await;
        assert!(
            sink.events_of("fire").is_empty(),
            "登记数与容量一致 ⇒ 零事件"
        );

        // 不一致（读回 5）→ **首次观测即产 1 条**，`value` = 读回登记数
        put(5);
        sched.tick_once(1000).await;
        assert_eq!(
            sink.events_since("fire", 0),
            vec![("fire_detector_count_mismatch".to_string(), 5.0)],
            "进入事件 value = 读回登记数（诊断量）"
        );

        // 判决式仍成立（连续 4 轮）→ 一条都不产（原"命中即产"= 8.6 万条/日）
        for t in 2..6u64 {
            put(5);
            sched.tick_once(t * 1000).await;
        }
        assert_eq!(
            sink.events_of("fire").len(),
            1,
            "不一致状态未变的轮次不重复产（状态翻转制）"
        );
        // 去重只作用于事件：telemetry 逐轮照落（该轮 `fire_sys_1` 已落 6 次）
        assert_eq!(
            sink.telemetry_of("fire")
                .iter()
                .filter(|(m, _)| m == "fire_sys_1")
                .count(),
            6,
            "去重不得扩散到非事件路径"
        );

        // 改回一致 → 恰 1 条 `@recovered`（value 恒 0.0）
        put(4);
        sched.tick_once(6000).await;
        assert_eq!(
            sink.events_since("fire", 1),
            vec![("fire_detector_count_mismatch@recovered".to_string(), 0.0)],
            "恢复一致 ⇒ 必产 1 条「已恢复」"
        );
    }

    /// **③ 首轮即不一致 ⇒ 首次观测即产**（"上线时登记数就已错"必须可见，§11.4.7.2 C）——
    /// 且该轮**不产** ⑤ 地址序事件（本 fixture 的 `+0` 严格升序），即两类判据互不串台。
    #[tokio::test]
    async fn fire_detector_count_mismatch_first_observation_emits() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 1)], bus.clone(), sink.clone());
        // 容量 = 1（探测器 1，`fire_sys` 覆盖寄存器 11）+ `fire_det`.count 6 ÷ 6 = **2**；
        // 寄存器 10 读回 **4** ⇒ 首轮即不一致
        let mut sys = vec![0u16; 13];
        sys[6] = 4;
        bus.put(1, 4, sys);
        bus.put(1, 17, vec![1, 0, 0, 0, 0, 0]);
        sched.tick_once(0).await;
        assert_eq!(
            sink.events_of("fire"),
            vec![("fire_detector_count_mismatch".to_string(), 4.0)],
            "首轮即不一致 ⇒ 首次观测即产 1 条（value = 读回登记数）"
        );
        // 两类判据互不串台：本 fixture 只有 1 只探测器（无回退）⇒ 不产 ⑤
        assert_eq!(
            sink.event_count("fire", "fire_detector_addr_order_invalid"),
            0,
            "登记数不一致不得顺带产地址序事件"
        );
    }

    /// **钢瓶气压"是否配置"口径**（PRD §9.7.6 / §11.7.3）：本站生命周期内**曾出现过非 0**
    /// ⇒ `true`（此后恒 0 按真实 0 展示）；**从未非 0** ⇒ `false`（展示层标"未配置"，
    /// 不得显示 "0 kPa"、不得判"气压异常/泄漏"）。该点**不产任何事件**（同文件既有用例钉住）。
    #[tokio::test]
    async fn fire_cylinder_pressure_configured_latches_on_nonzero() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![fire_conf("ttyS6", 1, 1)], bus.clone(), sink.clone());
        let det = [1u16, 0, 0, 0, 0, 0];

        // 恒 0 三轮 ⇒ 仍未配置
        for t in 0..3u64 {
            put_fire(&bus, 1, 0, &det);
            sched.tick_once(t * 1000).await;
        }
        assert!(
            !sched.cylinder_pressure_configured(0),
            "从未非 0 ⇒ 未配置（展示层标「未配置」而非 0 kPa）"
        );

        // 出现非 0（123 kPa）⇒ 置位
        let mut sys = vec![0u16; 13];
        sys[1] = 123; // 偏移 1 = addr 5 = 钢瓶气压
        sys[6] = 2; // 登记数 = 容量（1 + 6/6 = 2：含探测器 1；见 `put_fire` 的订正说明）
        bus.put(1, 4, sys);
        bus.put(1, 17, det.to_vec());
        sched.tick_once(3000).await;
        assert!(sched.cylinder_pressure_configured(0), "出现过非 0 ⇒ 已配置");

        // 此后恒 0 ⇒ **不回退**（钢瓶气压不会在业务上"变回未配置"）
        for t in 4..6u64 {
            put_fire(&bus, 1, 0, &det);
            sched.tick_once(t * 1000).await;
        }
        assert!(
            sched.cylinder_pressure_configured(0),
            "记忆只置位不回退（恒 0 按真实 0 展示）"
        );
        assert!(
            sink.events_of("fire").is_empty(),
            "钢瓶气压永不产事件（PRD §9.7.6 明令）"
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
            vec![
                GroupPoll {
                    station_index: 0,
                    anchor_blk: 0,
                    is_carrier: true,
                    was_failing: false,
                },
                GroupPoll {
                    station_index: 1,
                    anchor_blk: 0,
                    is_carrier: true,
                    was_failing: false,
                },
            ]
        );
        // 同一 now 二次调用不再返回（已推进 next_due）
        assert!(calc.due_round(0).is_empty());
        // now=1000：仅 grid 到期（hvac next_due 已推进到 5000）
        assert_eq!(
            calc.due_round(1000),
            vec![GroupPoll {
                station_index: 0,
                anchor_blk: 0,
                is_carrier: true,
                was_failing: false,
            }]
        );
        // now=5000：grid（1000 到期后 1000+1000=2000→5000 已落后一轮，钳到 6000）与 hvac 都到期
        assert_eq!(
            calc.due_round(5000),
            vec![
                GroupPoll {
                    station_index: 0,
                    anchor_blk: 0,
                    is_carrier: true,
                    was_failing: false,
                },
                GroupPoll {
                    station_index: 1,
                    anchor_blk: 0,
                    is_carrier: true,
                    was_failing: false,
                },
            ]
        );
    }

    /// delay_group：把 next_due 后移 now+extra；现 next_due 更晚则不动。
    /// （T9：既有 `delay_station` 的站级入口由 `delay_group` 取代 —— 单组站的组键 =
    /// `(站下标, 组锚 0)`，**本用例的断言一字未改**，只把调用换成组键。）
    /// 本例退避 `extra=2000` ⇒ `target = 0+2000 = 2000 > 现 next_due(1000)` ⇒ 后移到 2000。
    #[test]
    fn due_calc_delay_station_backs_off() {
        let stations = vec![hvac_conf("hvac", "ttyS1", 3, 1000)];
        let group: Vec<(usize, &StationConf)> = stations.iter().enumerate().collect();
        let mut calc = DueCalc::from_group(&group);
        assert_eq!(calc.due_round(0).len(), 1); // 首轮到期，next_due 推进到 1000
        calc.delay_group(
            GroupKey {
                station_index: 0,
                anchor_blk: 0,
            },
            0,
            2000,
        );
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

    // ══════ S3b-3（T8）：分组纯函数的不变量（设计 §12.4.1 / §12.3 的 V-5）══════

    /// `read_groups_of` / `carrier_group` 的**直接单测**（不依赖调度流程）：
    /// ① 无声明 ⇒ 唯一组（周期 = 站周期、全部块、锚 = 0）；
    /// ② 快慢两块 ⇒ 两组、按**周期升序**、锚正确、承载组 = 周期最大者（C8）；
    /// ③ **空 `regs`** ⇒ 恰好 1 组、空块集、锚 = [`EMPTY_GROUP_ANCHOR`]、周期 = 站周期；
    /// ④ 空 `regs` 时 `carrier_group` 返回 `Some` 且就是那唯一组（**不得 panic**）。
    #[test]
    fn read_groups_of_invariants() {
        // ① 全部块未声明 `interval_ms` ⇒ 唯一桶（V-5(a)；单组站 ⇒ 零行为变化的依据）
        let c = grid_conf("grid_meter", "ttyS4", 3, 1000);
        let gs = read_groups_of(&c);
        assert_eq!(gs.len(), 1, "无声明 ⇒ 唯一组");
        assert_eq!(gs[0].interval_ms, 1000);
        assert_eq!(
            gs[0].blk_indices,
            vec![0, 1, 2, 3, 4, 5],
            "组内块 = 全部块（书写序）"
        );
        assert_eq!(gs[0].anchor_blk, 0, "锚 = blk_indices[0] = 0");
        let cg = carrier_group(&c).expect("至少一组");
        assert_eq!((cg.interval_ms, cg.anchor_blk), (1000, 0));

        // ② 快慢两块 ⇒ 两组（周期升序）、锚 = 各组首块；承载组 = 周期最大者（PRD §10.3.2 C8）
        let mut c = hvac_conf("hvac", "ttyS3", 1, 5000);
        c.regs.push(dblk("hvac_di", 0, 31));
        assert!(
            c.regs[0].interval_ms.is_none(),
            "hvac_in 不声明 ⇒ 继承站周期"
        );
        c.regs[1].interval_ms = Some(1000);
        let gs = read_groups_of(&c);
        assert_eq!(gs.len(), 2);
        assert_eq!(
            (
                gs[0].interval_ms,
                gs[0].blk_indices.clone(),
                gs[0].anchor_blk
            ),
            (1000, vec![1], 1),
            "快组（锚 = 块下标 1）"
        );
        assert_eq!(
            (
                gs[1].interval_ms,
                gs[1].blk_indices.clone(),
                gs[1].anchor_blk
            ),
            (5000, vec![0], 0),
            "站周期组（锚 = 块下标 0）"
        );
        let cg = carrier_group(&c).expect("至少一组");
        assert_eq!(
            (cg.interval_ms, cg.anchor_blk),
            (5000, 0),
            "承载组 = 周期最大的组（快组不承载站级语义）"
        );

        // ③ 空 `regs` ⇒ 恰好 1 个**退化组**（空块集、哨兵锚、周期 = 站周期）—— V-5(b)。
        //    该输入**可达**（既有 `battery_station_without_soc_block_does_not_push` 内联构造
        //    空 `regs` 站、不经 `validate`）⇒ 不得返回 0 组、不得 panic。
        let mut c = hvac_conf("empty", "ttyS9", 1, 3000);
        c.regs = Vec::new();
        let gs = read_groups_of(&c);
        assert_eq!(gs.len(), 1, "空 regs 站须仍产 1 个组（否则被静默移出调度）");
        assert!(gs[0].blk_indices.is_empty());
        assert_eq!(gs[0].anchor_blk, EMPTY_GROUP_ANCHOR);
        assert_eq!(gs[0].interval_ms, 3000, "退化组周期 = 站周期");

        // ④ 空 `regs` 时 `carrier_group` 返回 `Some`（不 panic；设计 §12.4.1 的 S-1 注）
        let cg = carrier_group(&c).expect("空 regs 站也必有 1 组（退化组）");
        assert_eq!(
            (cg.interval_ms, cg.anchor_blk, cg.blk_indices.len()),
            (3000, EMPTY_GROUP_ANCHOR, 0)
        );

        // 组键（T9 的 `HashMap` 键）须可对退化组使用：哨兵锚仍是**单射**的合法键
        let mut keys: HashMap<GroupKey, usize> = HashMap::new();
        keys.insert(
            GroupKey {
                station_index: 0,
                anchor_blk: cg.anchor_blk,
            },
            1,
        );
        assert!(keys.contains_key(&GroupKey {
            station_index: 0,
            anchor_blk: EMPTY_GROUP_ANCHOR,
        }));
        assert!(!keys.contains_key(&GroupKey {
            station_index: 1,
            anchor_blk: EMPTY_GROUP_ANCHOR,
        }));
    }

    // ══════════ S3b-3（T9）：调度器改造的行为钉子（设计 §12.4 / §12.8）══════════

    /// §10.3.3 / §12.6 的**首例**（HVAC 位块快采）：站周期 5000；`hvac_in`（FC04
    /// 30001–30004，3 点，**不声明**）维持站周期 ⇒ 站级**承载组**；`hvac_di`（FC02 位 0–30，
    /// 31 位）按 `fast` 声明 `interval_ms: 1000` ⇒ 快组。
    /// `fast = false` ⇒ **单组站**（`eff` 全 = 5000；AC-8-3 的零回归锚）。
    fn hvac_first_case_conf(fast: bool) -> StationConf {
        StationConf {
            id: "hvac".into(),
            role: Role::Hvac,
            port: "ttyS3".into(),
            protocol: "modbus".into(),
            slave: 1,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::Even,
            interval_ms: 5000,
            regs: vec![
                RegBlockConf {
                    name: "hvac_in".into(),
                    addr: 0,
                    func: RegFunc::Input,
                    format: RegFormat::Int16,
                    scale: 0.1,
                    count: 4,
                    offset: 0.0,
                    byte_swap: false,
                    // 3 个标量点（AC-8-2 的 `hvac_in_1` / `hvac_in_3` / `hvac_in_4`）
                    points: vec![
                        PointConf {
                            at: 1,
                            count: 1,
                            name: None,
                            format: None,
                            scale: None,
                            offset: None,
                            word_order: WordOrder::HiLo,
                        },
                        PointConf {
                            at: 3,
                            count: 1,
                            name: None,
                            format: None,
                            scale: None,
                            offset: None,
                            word_order: WordOrder::HiLo,
                        },
                        PointConf {
                            at: 4,
                            count: 1,
                            name: None,
                            format: Some(RegFormat::Uint16),
                            scale: None,
                            offset: None,
                            word_order: WordOrder::HiLo,
                        },
                    ],
                    read_slice: false,
                    interval_ms: None,
                },
                RegBlockConf {
                    name: "hvac_di".into(),
                    addr: 0,
                    func: RegFunc::Discrete,
                    format: RegFormat::Uint16, // discrete 块的 format/scale 不参与
                    scale: 0.0,
                    count: 31,
                    offset: 0.0,
                    byte_swap: false,
                    points: Vec::new(),
                    read_slice: false,
                    interval_ms: fast.then_some(1000),
                },
            ],
        }
    }

    /// 首例的**块序颠倒**变体（`hvac_di` 写在配置**前面**）：快组锚 = 0、承载组锚 = 1
    /// —— 正是 §12.4.2 的 Δ-14 论证所举的形态（旧的升序键 `(.., anchor_blk)` 会把**承载组
    /// 排在快组之后**）。用于钉住"承载组在站内**恒排最前**"（S-3 修订的同 tick 组序）。
    fn hvac_fast_first_conf() -> StationConf {
        let mut c = hvac_first_case_conf(true);
        c.regs.swap(0, 1); // regs = [hvac_di(1000, 锚 0), hvac_in(5000, 锚 1 = 承载组)]
        c
    }

    /// **真交错**变体：快组周期取 **2000**（而非首例的 1000）。
    ///
    /// 2000 与站周期 5000 **互不整除** ⇒ 存在"承载组到期、快组**不**到期"的 tick（t=5000）；
    /// 这正是"按站存 tracker 会**静默吞掉**位跳变"的窗口。若快组周期整除站周期
    /// （如 1000 | 5000），承载组 prime 后**同一 tick 内**快组会立刻重新 prime ⇒ 机制被掩盖、
    /// 用例假绿（承载组恒排最前，见 `!is_carrier` 排序键）。
    /// C1–C5 全部满足：`2000 > 0` ✓ `≥ 500` ✓ `% poll_ms(1000) == 0` ✓ `≥ 1.5×22 ms` ✓ `≤ 5000` ✓。
    fn hvac_interleaved_conf() -> StationConf {
        let mut c = hvac_first_case_conf(true);
        c.regs[1].interval_ms = Some(2000); // regs[1] = hvac_di（快组）；regs[0] = hvac_in = 承载组
        c
    }

    /// **退化配置变体**（PRD §10.6 第 8 条 / 设计 §12.4.3 的注）：站内**全部**块都声明了
    /// `interval_ms` ⇒ 站级 `interval_ms`（5000）**完全不参与分组**，`read_groups_of` 只按
    /// 声明值分桶：`{1000 → [hvac_di], 2000 → [hvac_in]}`。**承载组 = 周期最大的组 = 2000**
    /// （`hvac_in`），与站周期 5000 **不等** ⇒ 正是"站级退避入参取承载组自身组周期"这一设计
    /// 选择可被观测的唯一形态（非退化配置下二者恒等 ⇒ 不可区分）。
    ///
    /// C1–C5 全部满足（依赖 `validate_block_intervals` 规则 20/21 的判据）：两块 `≥ 500` ✓、
    /// `% poll_ms(1000) == 0` ✓、`≤ 站周期 5000` ✓；`R(Role::Hvac) = ∅`（规则 22 不适用）。
    fn hvac_all_declared_conf() -> StationConf {
        let mut c = hvac_first_case_conf(true); // regs[0] = hvac_in（原不声明）, regs[1] = hvac_di(1000)
        c.regs[0].interval_ms = Some(2000); // 补上声明 ⇒ 站内全部块都已声明（站周期 5000 落空）
        c
    }

    /// 预置首例站点：`hvac_in` 4 寄存器（全 0 ⇒ 3 个标量点值 0.0）+ `hvac_di` 31 位（全 0）。
    fn put_hvac_first_case(bus: &MockBus) {
        bus.put_input(1, 0, vec![0u16; 4]);
        bus.put_bits(1, 0, vec![false; 31]);
    }

    /// AC-8-1 / AC-8-3 明文的**确定性 tick 序**：`t = 0,1000,…,9000`（10 tick）。
    const FIRST_CASE_TICKS: [u64; 10] = [0, 1000, 2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000];

    /// §12.8 的 **B-1 / S-3 判别锚**共用构造（**自建站、slave = 1** —— 与既有 `battery_*`
    /// fixture 的 `slave = 2` 无关）：
    /// - 站 `interval_ms: 2000`（02 PRD §9.5.1 已登记的 `battery` 回退周期档位；< 5000 ⇒ 合法）；
    /// - `bms_alarm`（FC02 `addr 200` `count 288`）声明 `interval_ms: 1000` ⇒ **非承载组**；
    /// - 含 `soc` 点的 `bms_io`（FC04 `addr 100` `count 31`）**不声明** ⇒ `eff = 2000`
    ///   = **承载组**（C8；同时也是 C6/C7 未被破坏的证明 —— `soc` 块恒在承载组）。
    fn battery_split_conf() -> StationConf {
        StationConf {
            id: "battery".into(),
            role: Role::Battery,
            port: "ttyS2".into(),
            protocol: "modbus".into(),
            slave: 1,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
            interval_ms: 2000,
            regs: vec![
                RegBlockConf {
                    name: "bms_alarm".into(),
                    addr: 200,
                    func: RegFunc::Discrete,
                    format: RegFormat::Uint16,
                    scale: 0.0,
                    count: 288,
                    offset: 0.0,
                    byte_swap: false,
                    points: Vec::new(),
                    read_slice: false,
                    interval_ms: Some(1000),
                },
                RegBlockConf {
                    name: "bms_io".into(),
                    addr: 100,
                    func: RegFunc::Input,
                    format: RegFormat::Uint16,
                    scale: 1.0,
                    count: 31,
                    offset: 0.0,
                    byte_swap: false,
                    points: vec![PointConf {
                        at: 1, // 寄存器偏移 0 = 块内首个寄存器
                        count: 1,
                        name: Some("soc".into()),
                        format: None,
                        scale: None,
                        offset: None,
                        word_order: WordOrder::HiLo,
                    }],
                    read_slice: false,
                    interval_ms: None,
                },
            ],
        }
    }

    /// 预置该站：`bms_io` 31 寄存器（首寄存器 = 65 ⇒ `soc` 域内）+ `bms_alarm` 288 位全 0。
    fn put_battery_split(bus: &MockBus) {
        let mut io = vec![0u16; 31];
        io[0] = 65;
        bus.put_input(1, 100, io);
        bus.put_bits(1, 200, vec![false; 288]);
    }

    /// `bms_alarm` 的 288 位向量，**位内偏移 1 = 位地址 201**（`point_table::lookup_bit(
    /// Role::Battery, 201) == BitClass::Alarm` ⇒ 点位名 `bms_alarm_2`，`point_table.rs:388`）。
    fn alarm_bits_201(active: bool) -> Vec<bool> {
        let mut b = vec![false; 288];
        b[1] = active;
        b
    }

    /// §12.8 用例①（**AC-8-1 的可观测判据 ①**）：块级周期覆盖生效 ——
    /// 同一 tick 序下快组每个 tick 到期、承载组只按站周期到期。
    #[tokio::test]
    async fn block_interval_overrides_station_period() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_first_case_conf(true)], bus.clone(), sink.clone());

        for t in FIRST_CASE_TICKS {
            sched.tick_once(t).await;
        }
        assert_eq!(
            bus.bit_call_count(1, 0),
            10,
            "位块（快组，1000 ms）⇒ 10 tick 各一次 FC02"
        );
        assert_eq!(
            bus.input_call_count(1, 0),
            2,
            "标量块（承载组，5000 ms）⇒ 仅 t=0/5000 各一次 FC04"
        );
        assert_eq!(sink.event_count("hvac", "offline"), 0);
    }

    /// §12.8 用例②（**AC-8-2 的判据 ②**）：标量仍按站周期 —— 3 个标量点各上送 **2** 次，
    /// **不受位块提速影响**（不得变成 10 次）。
    #[tokio::test]
    async fn scalar_still_follows_station_period() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_first_case_conf(true)], bus.clone(), sink.clone());

        for t in FIRST_CASE_TICKS {
            sched.tick_once(t).await;
        }
        let tel = sink.telemetry_of("hvac");
        for m in ["hvac_in_1", "hvac_in_3", "hvac_in_4"] {
            assert_eq!(
                tel.iter().filter(|(n, _)| n == m).count(),
                2,
                "{m} 应按站周期（5000 ms）上送 2 次，不受位块提速影响"
            );
        }
    }

    /// §12.8 用例③（**AC-8-3，零回归锚 / V-5**）：首例配置**去掉** `hvac_di.interval_ms`
    /// ⇒ 单组站（`eff` 全 = 5000）。四项断言与**改造前**逐条相同（"一个字节都不动"）。
    #[tokio::test]
    async fn no_block_interval_is_bit_identical_to_legacy() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_first_case_conf(false)], bus.clone(), sink.clone());

        for t in FIRST_CASE_TICKS {
            sched.tick_once(t).await;
        }
        // ① 单组每 5000 ms 到期一次 ⇒ 二块各 2 次读事务
        assert_eq!(bus.bit_call_count(1, 0), 2);
        assert_eq!(bus.input_call_count(1, 0), 2);
        // ② `on_station_telemetry` 调用次数 = 2（每轮 1 次；位块无变化 ⇒ 第 2 轮无位点）
        let calls = sink.telemetry_calls_of("hvac");
        assert_eq!(calls.len(), 2, "每轮 1 次普通遥测上送");
        // ③ 第 1 次项数 = 34（3 标量 + 31 位，**首轮全量快照**）、第 2 次 = 3（标量全量 + 无变化位）
        assert_eq!(calls[0].len(), 34, "首轮 = 3 标量 + 31 位全量快照");
        assert_eq!(calls[1].len(), 3, "第 2 轮 = 3 标量 + 0 变化位");
        // ④ 事件 0 条（首轮只建基线）
        assert!(sink.events_of("hvac").is_empty(), "首轮只建基线");
    }

    /// §12.8 用例④（**AC-8-4**）：位 10（`hvac_di_11` 柜内高温告警）由 0→1 后，在**下一次
    /// 位组到期轮**（t=1000 ⇒ ≤ 1000 ms）即产出；事件经 `on_station_telemetry(.., is_event=true)`
    /// 上送（`events_since` 只含 `is_event=true` 的项）且**沿用既有 metric**（无新增命名）。
    #[tokio::test]
    async fn bit_change_event_within_block_period() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_first_case_conf(true)], bus.clone(), sink.clone());

        sched.tick_once(0).await; // 首轮：只建基线
        assert!(sink.events_of("hvac").is_empty(), "首轮只建基线、不产事件");

        let mut bits = vec![false; 31];
        bits[10] = true; // 位 10 ⇒ 点位名 `hvac_di_11`
        bus.put_bits(1, 0, bits);
        sched.tick_once(1000).await;
        assert_eq!(
            sink.events_since("hvac", 0),
            vec![("hvac_di_11".to_string(), 1.0)],
            "位块周期（1000 ms）内即产进入事件（既有事件通道，无新增 metric）"
        );
    }

    /// §12.8 用例⑤（**§12.4.4 的机制钉子**）：**交错 tick** 下位块的 0→1 必须产事件 ——
    /// 若 tracker 键仍按**站**存，慢组（t=5000）的 `prime()` 会 `clear()` 掉快组的位记忆
    /// ⇒ t=6000 的跳变被**静默吞掉** ⇒ 本用例**必红**。
    #[tokio::test]
    async fn edge_memory_is_per_group() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus);
        let sink = Arc::new(FakeSink::default());
        // ⚠️ **必须用真交错变体**（快组 2000 / 承载组 5000，二者互不整除）：只有在"承载组到期、
        // 快组不到期"的 tick（t=5000）里，按站存才会用承载组的 3 个标量把 31 位记忆**清掉**；
        // 若快组周期整除站周期，承载组 prime 之后同一 tick 内快组立刻重新 prime ⇒ 机制被掩盖、
        // 用例变假绿（承载组恒排最前 —— 见 §12.4.2 的 `!is_carrier` 排序键）。
        let sched = build(vec![hvac_interleaved_conf()], bus.clone(), sink.clone());

        // 快组 t=0/2000/4000/6000；承载组 t=0/5000 ⇒ **t=5000 只有承载组**到期
        for t in [0u64, 2000, 4000, 5000] {
            sched.tick_once(t).await;
        }
        assert!(sink.events_of("hvac").is_empty(), "此前位未变 ⇒ 无事件");

        let mut bits = vec![false; 31];
        bits[10] = true;
        bus.put_bits(1, 0, bits);
        sched.tick_once(6000).await;
        assert_eq!(
            sink.events_since("hvac", 0),
            vec![("hvac_di_11".to_string(), 1.0)],
            "组级 tracker ⇒ 承载组的基线重建不得吞掉快组的真实跳变"
        );
    }

    /// §12.8 用例⑥（**B-1 回归锚**）：**非承载组**的位遥测与事件**照常产出**。
    /// 判别性断言 = ②：非承载组的读集 `reads = {bms_alarm}` 在
    /// `judges_evaluable(Role::Battery, ..)` 下**恒 `false`**（`battery_soc` 见不到 `soc` 点
    /// ⇒ `SocOutcome::NoSuchPoint`）—— 若把守卫误扩到**整段**遥测/事件（旧写法），该组全部
    /// 位/标量遥测与事件被**静默丢弃** ⇒ 位跳变永不产出 ⇒ 本用例**必红**。
    #[tokio::test]
    async fn non_carrier_group_still_emits_its_bits() {
        let bus = Arc::new(MockBus::new());
        put_battery_split(&bus);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![battery_split_conf()], bus.clone(), sink.clone());

        sched.tick_once(0).await; // t=0：两组建基线
        assert!(sink.events_of("battery").is_empty(), "首轮只建基线");

        // 位地址 201 由 0 置 1（在 t=1000 轮之前）；t=0 轮只建基线 ⇒ 边沿在**首次看到新值的
        // 那一轮** = t=1000 产出（t=2000 时该值已无变化 ⇒ 取不到 is_event = true）。
        bus.put_bits(1, 200, alarm_bits_201(true));
        sched.tick_once(1000).await;
        // ② 判别性断言：非承载组的**事件**照常产出
        assert_eq!(
            sink.events_since("battery", 0),
            vec![("bms_alarm_2".to_string(), 1.0)],
            "非承载组的位变化沿事件照常产出（守卫**不得**门控它）"
        );
        // ②′ 非承载组的**位遥测**照常上送（落库侧与事件侧分流，两边都要在）
        assert!(
            sink.telemetry_of("battery")
                .iter()
                .any(|(m, v)| m == "bms_alarm_2" && *v == 1.0),
            "非承载组的位遥测照常上送"
        );
        // ③ 该组若有标量块则按 D2 口径每轮全量 —— 本组的块全是位块，无标量点（形态如实登记）

        // ① 分组与计数：非承载组每 tick 到期（t=0…5000 共 6）；承载组 t=0/2000/4000（3）
        for t in [2000u64, 3000, 4000, 5000] {
            sched.tick_once(t).await;
        }
        assert_eq!(
            bus.bit_call_count(1, 200),
            6,
            "非承载组每 tick 到期（1 s 快采）"
        );
        assert_eq!(
            bus.input_call_count(1, 100),
            3,
            "承载组按站周期 2000 ms 到期（同时证明 `soc` 块恒在承载组：C6/C7 未被破坏）"
        );
    }

    /// §12.8 用例⑦（**守卫有效性负向锚**）：**绕过 `validate()`** 直接构造一个"判据跨组"的
    /// `fire` 站（`fire_det` 声明 1000、`fire_sys` 不声明、站 5000 ⇒ **违 C7**，配置期会拒）。
    /// ① **非承载组照常交付**（不得因守卫恒 `false` 而整段静默）；
    /// ② 承载组的 `StationFlag` **恒不求值** ⇒ 两类消防事件一条都不产 —— 若不设守卫/删守卫，
    ///    承载组读集 `{fire_sys}` 会让登记数判据**假报**（读回 20 vs 容量 `0 + 1 = 1`）。
    /// 该用例钉住"B-1 的修法是**收窄作用域**，不是**取消守卫**"。
    #[tokio::test]
    async fn station_flag_guard_still_scopes_to_carrier() {
        let bus = Arc::new(MockBus::new());
        let sink = Arc::new(FakeSink::default());
        let mut st = fire_conf("ttyS6", 1, 1); // regs = [fire_sys(idx 0), fire_det(idx 1)]
        st.interval_ms = 5000;
        st.regs[1].interval_ms = Some(1000); // fire_det 提速 ⇒ 非承载组 = {fire_det}
        let sched = build(vec![st], bus.clone(), sink.clone());

        let mut sys = vec![0u16; 13];
        sys[6] = 20; // 寄存器 10 = `fire_det_count` ⇒ 读回 20
        bus.put(1, 4, sys);
        bus.put(1, 17, vec![0u16; 6]); // 探测器区：1 组、全 0（不额外产字级信号事件）

        for t in [0u64, 1000, 2000, 3000, 4000, 5000] {
            sched.tick_once(t).await;
        }
        // ① 读集不含 `R(fire)` 的**非承载组**仍在正常交付（每 tick 一轮标量全量）。
        //    **精确值 = 8**（评审 修 5：原为软下界 `>= 6`）—— 确定性 tick 序下该计数**完全可算**：
        //    · 非承载组（`fire_det`，组周期 1000）：t=0/1000/2000/3000/4000/5000 共 **6** 轮各 1 次；
        //    · 承载组（`fire_sys`，组周期 5000）：t=0/5000 共 **2** 轮各 1 次（标量全量）。
        //    软下界 `>= 6` 会同时**放过**"承载组整段静默"（=6）与"某轮多采"两类回归 ⇒ 改精确值。
        assert_eq!(
            sink.telemetry_call_count("fire"),
            8,
            "非承载组 6 轮 + 承载组 2 轮 = 8 次；软下界会放过「承载组整段静默」这类回归"
        );
        // ② 站级判据守卫：承载组的 StationFlag 恒不求值 ⇒ 两类事件恒 0 条
        assert_eq!(
            sink.event_count("fire", "fire_detector_count_mismatch"),
            0,
            "承载组缺 `fire_det` ⇒ 须跳过求值（否则读回 20 vs 容量 1 ⇒ 假报警）"
        );
        assert_eq!(
            sink.event_count("fire", "fire_detector_addr_order_invalid"),
            0
        );
    }

    /// §12.8 用例⑧（**AC-8-7 ②** 之一）：**块级（快采）组失败不升级为站级 offline** ——
    /// `offline`/`online` 都不产、`offline_count` 不自增（§12.5 的两条硬理由）。
    #[tokio::test]
    async fn block_group_failure_no_station_offline() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_first_case_conf(true)], bus.clone(), sink.clone());

        bus.fail_bits_once(1, 0); // 非承载（位）组读失败一次
        sched.tick_once(0).await;
        assert_eq!(
            sink.event_count("hvac", "offline"),
            0,
            "块级组失败不产 offline"
        );
        assert_eq!(
            sink.event_count("hvac", "online"),
            0,
            "也不产 online（站从未被判离线）"
        );
        {
            let st = sched.state.read().unwrap();
            assert_eq!(st[0].offline_count, 0, "offline_count 不自增");
        }
    }

    /// §12.8 用例⑨（**AC-8-7 ②** 之二）：块级组退避按**组周期**指数增长。
    ///
    /// **为什么必须 fail 两轮**（评审建议 d）：单轮失败时 `backoff_extra(组周期, 1) == 组周期`
    /// 与 `due_round` 自身的 `next_due += interval` **数值相同** ⇒ 无法区分"退避生效"与
    /// "退避未生效"；`oc = 2` 时 `extra = 2 × 组周期 = 2000 ms` 才有判别力。
    #[tokio::test]
    async fn block_group_backs_off_by_group_period() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_first_case_conf(true)], bus.clone(), sink.clone());

        bus.fail_bits_once(1, 0);
        bus.fail_bits_once(1, 0);
        sched.tick_once(0).await; // 第 1 轮失败 ⇒ 组级计数 1（extra = 1×1000）
        sched.tick_once(1000).await; // 第 2 轮失败 ⇒ 组级计数 2（extra = 2×1000）
        assert_eq!(bus.bit_call_count(1, 0), 2, "两轮各试一次");
        sched.tick_once(2000).await;
        assert_eq!(
            bus.bit_call_count(1, 0),
            2,
            "oc=2 ⇒ 失败后 next_due 后移 2×组周期，t=2000 不应重试"
        );
        sched.tick_once(3000).await;
        assert_eq!(
            bus.bit_call_count(1, 0),
            3,
            "第 3 轮到期间隔 = 2 × 组周期 = 2000 ms（t=1000 + 2000）"
        );
        assert_eq!(
            sink.event_count("hvac", "offline"),
            0,
            "全程不升级为站级 offline"
        );
    }

    /// §12.8 用例⑩（**AC-8-7 ①**）：承载组（慢组）失败 ⇒ 站级 `offline` 一次 +
    /// `offline_count == 1` + 按**（承载组自身）组周期**退避（非退化配置 ≡ 站周期）；
    /// 同 tick 的位组不受影响，退避到期重试成功 ⇒ `online` 一次。
    #[tokio::test]
    async fn carrier_group_failure_emits_offline_once() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_first_case_conf(true)], bus.clone(), sink.clone());

        bus.fail_input_once(1, 0); // 承载组（hvac_in）读失败
        sched.tick_once(0).await;
        assert_eq!(
            sink.event_count("hvac", "offline"),
            1,
            "承载组失败 ⇒ 站级 offline 一次"
        );
        {
            let st = sched.state.read().unwrap();
            assert_eq!(st[0].offline_count, 1, "offline_count = 1");
        }
        assert_eq!(
            bus.bit_call_count(1, 0),
            1,
            "同站非承载组不受影响（同 tick 照常采）"
        );

        for t in [1000u64, 2000, 3000, 4000] {
            sched.tick_once(t).await;
        }
        assert_eq!(
            bus.input_call_count(1, 0),
            1,
            "按承载组组周期（= 站周期 5000）退避 ⇒ 未到 t=5000 不重试"
        );
        sched.tick_once(5000).await;
        assert_eq!(bus.input_call_count(1, 0), 2, "退避到期点重试");
        assert_eq!(
            sink.event_count("hvac", "online"),
            1,
            "重试成功 ⇒ online 一次"
        );
    }

    /// **退化配置下：承载组退避按「承载组自身组周期」而非「站周期」**（设计 §12.4.3 的注 +
    /// §12.5 的站级退避行；PRD §10.6 第 8 条允许"站内全部块都声明 `interval_ms`"）。
    ///
    /// **判别力从何而来**（既有 `carrier_group_failure_emits_offline_once` 钉不住这点）：
    /// 那条例用的是 `hvac_first_case_conf(true)` —— 站周期 5000、承载组即"周期最大的组"，
    /// 其组周期**恒等于**站周期 ⇒ 入参取"组周期"还是"站周期"**结果相同、不可区分**。本用例
    /// 改用 [`hvac_all_declared_conf`]：站 5000、块声明 **2000 / 1000** ⇒ 承载组 = 2000 组
    /// （`hvac_in`），与站周期 5000 **不等**，两条口径给出**不同的到期点**：
    ///  · 取**组周期 2000**（本实现）：t=0 首次失败（oc=1）后 `next_due = 0 + 2000 = 2000`
    ///    （与 `due_round` 自身推进**同值** ⇒ 单轮不可判）；t=2000 第 2 次失败（oc=2）后
    ///    `next_due = 2000 + 2 × 2000 = **6000**`；
    ///  · 取**站周期 5000**：t=0 失败后 `next_due = 0 + 1 × 5000 = 5000` ⇒ **t=2000 就不重试**
    ///    （**已实注入验证**：本用例在 t=2000 的 `input_call_count` 断言即红）；
    ///  · 取"无退避"退化：t=2000 时 `next_due = 4000`（仅 `due_round` 推进）⇒ **t=4000 会重试**
    ///    ⇒ 该轮计数断言红。
    /// ⇒ 三重判别：只有"组周期 2000"口径能同时满足 t=2000 采第 2 次、t=3000/4000/5000 **不**采、
    ///   t=6000 采第 3 次。
    ///
    /// **确定性 tick 序**：t = 0, 1000, 2000, 3000, 4000, 5000, 6000（步长 1000 = `poll_ms`）。
    /// **为什么承载组必须失败两轮**：单轮失败时 `backoff_extra(2000, 1) == 2000` 与
    /// `due_round` 自身的 `next_due += interval` **数值相同** ⇒ 无法区分"退避生效"与"未生效"
    /// （同用例⑨的注）；`oc = 2` 时 `extra = 4000` 才有判别力。
    #[tokio::test]
    async fn carrier_backoff_uses_group_period_when_all_blocks_declared() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus); // hvac_in（FC04 addr 0）4 寄存器 + hvac_di（FC02 addr 0）31 位
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_all_declared_conf()], bus.clone(), sink.clone());

        // t=0：承载组（2000 组）+ 快组（1000 组）都到期（承载组恒排最前）⇒ 承载组读失败
        bus.fail_input_once(1, 0);
        sched.tick_once(0).await; // oc=1 ⇒ extra = 1×2000 = 2000（与 due_round 的推进同值 ⇒ 不可判）
        assert_eq!(sink.event_count("hvac", "offline"), 1, "承载组失败 ⇒ offline 一次");
        assert_eq!(bus.input_call_count(1, 0), 1, "承载组 t=0 采一轮（失败）");

        // t=1000：只有快组到期（承载组 next_due = 2000）
        sched.tick_once(1000).await;
        assert_eq!(bus.input_call_count(1, 0), 1, "承载组未到期");

        // t=2000：承载组第 2 次到期 ⇒ 再失败（oc=2 ⇒ extra = 2×2000 = 4000）
        bus.fail_input_once(1, 0);
        sched.tick_once(2000).await;
        assert_eq!(bus.input_call_count(1, 0), 2, "承载组 t=2000 再采一轮（失败）");
        {
            let st = sched.state.read().unwrap();
            assert_eq!(st[0].offline_count, 2, "offline_count 逐失败轮累加 = 2");
        }

        // 退避后的下一次到期 = 2000 + 4000 = **6000**（组周期口径；站周期口径应为 12000）
        for t in [3000u64, 4000, 5000] {
            sched.tick_once(t).await;
            assert_eq!(
                bus.input_call_count(1, 0),
                2,
                "t={t} 承载组不应到期（退避到 6000 ⇒ 非站周期口径的 12000、也非无退避的 4000）"
            );
        }
        // t=4000 **必须**不重试：「无退避」的退化写法在 t=4000 就会重试 ⇒ 此处红（关键判别点）
        sched.tick_once(6000).await;
        assert_eq!(
            bus.input_call_count(1, 0),
            3,
            "退避到期点 = 2000 + 2 × 组周期(2000) = 6000（按站周期 5000 则为 12000 ⇒ 此处红）"
        );
        assert_eq!(
            sink.event_count("hvac", "online"),
            1,
            "重试成功 ⇒ online 一次"
        );

        // 快组（1000 组）cadence 全程不受承载组退避影响：t=0…6000 共 7 轮
        assert_eq!(
            bus.bit_call_count(1, 0),
            7,
            "快组每 tick 到期（0/1000/…/6000）"
        );
        assert_eq!(
            sink.event_count("hvac", "offline"),
            1,
            "退避窗口内不重复产 offline（stale_timeout_s = 5s 去抖 + 退避跳过）"
        );
    }

    /// §12.8 用例⑪（**AC-8-7 ③** + §12.4.4 连带项 a）：**站恢复 ⇒ 该站全部组基线一并重建**。
    ///
    /// 时点安排使三个机制同时被钉住：① 触发者**只能是承载组**（`is_carrier && 读前
    /// offline_count > 0`）；② 承载组在站内**恒排最前**（§12.4.2 的 `!is_carrier` 排序键）——
    /// 恢复当轮**先**重建全组基线、**再**轮到快组产出，故快组读到新值 1 也**不产事件**；
    /// ③ 若只重建"承载组自己的"基线（旧写法），快组会立刻刷一条位事件 ⇒ 本用例必红。
    #[tokio::test]
    async fn station_recovery_resets_all_group_baselines() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus);
        let sink = Arc::new(FakeSink::default());
        // 用**块序颠倒**变体（快组锚 0、承载组锚 1）⇒ 本用例**同时**钉住同 tick 组序：
        // 若排序键缺 `!is_carrier`，恢复当轮快组会**先**产出 ⇒ 位事件照发 ⇒ 断言变红。
        let sched = build(vec![hvac_fast_first_conf()], bus.clone(), sink.clone());

        sched.tick_once(0).await; // 两组建基线（位全 0）
        sched.tick_once(1000).await; // 位组照常一轮（位未变）

        // 站离线：承载组 t=5000 读失败 ⇒ offline 1 条、offline_count = 1
        bus.fail_input_once(1, 0);
        sched.tick_once(5000).await;
        assert_eq!(sink.event_count("hvac", "offline"), 1);
        {
            let st = sched.state.read().unwrap();
            assert_eq!(st[0].offline_count, 1);
        }

        // 恢复当轮：承载组（t=10000，退避后的到期点）成功 + 位在此期间由 0→1
        let mut bits = vec![false; 31];
        bits[10] = true;
        bus.put_bits(1, 0, bits);
        sched.tick_once(10000).await;

        assert_eq!(sink.event_count("hvac", "online"), 1, "恢复 ⇒ online 一次");
        assert_eq!(sink.event_count("hvac", "offline"), 1);
        assert_eq!(
            sink.event_count("hvac", "hvac_di_11"),
            0,
            "全组基线已重建 ⇒ 恢复当轮（承载组最先）不刷位事件"
        );
        assert_eq!(
            sink.events_since("hvac", 1),
            vec![("online".to_string(), 1.0)],
            "恢复当轮**只**产 online 一条"
        );
    }

    /// **站恢复当轮会重置"本轮未到期"的组**（§12.4.4 连带项 a；补齐既有用例⑪盖不住的缺口）。
    ///
    /// **既有用例⑪为何盖不住**：它用 `hvac_fast_first_conf()`（快组 1000，站 5000）且恢复设在
    /// **t=10000** —— 10000 既是承载组的退避到期点、**又是快组的到期点** ⇒ 快组在恢复当轮
    /// **本来就到期**，其基线被重建究竟是"全组重建"带来的、还是"本组 `was_failing` 重建"
    /// 带来的，**不可区分**（该用例的判别力只在"同 tick 组序"与"承载组先行"两点上）。
    /// 若把全组重建误改成"**只重建承载组自己**"，用例⑪仍**绿**（快组同轮到期、随后自身
    /// 也会重建……不 —— 快组 `was_failing = false`，它**不会**重建；⑪ 之所以绿是因为恢复当轮
    /// 快组还没产出就被重建了 —— 但"未到期组"这一路径⑪**从未覆盖**）。
    ///
    /// **本用例的构造（错开 tick 序列）**：`hvac_interleaved_conf()`（承载组 5000 / 快组 **2000**，
    /// 互不整除）⇒ 恢复点 **t=5000 是承载组到期、而快组不到期**的 tick（快组 next_due = 6000）。
    /// 离线期间把位 10 改成 1（快组**尚不知情** —— 它在 5000 **不被采**）；恢复后快组
    /// **首次到期**在 t=6000 ⇒ 该轮必须**只产基线、不产假事件**。
    ///
    /// **确定性 tick 序**：t = 0, 2000, 4000, **5000（恢复）**, 6000。
    /// **判别力**：把"全组重建"改成"只重建承载组自己"，则 t=6000 快组仍持**离线前**基线
    /// （位全 0）⇒ 位 10 的 0→1 被当成真事件 ⇒ ② 断言取到 1 条**假报**、③ 该轮落位仅 1 项
    /// （非 31 项全量快照）⇒ 本用例**必红**（已实注入验证）。
    #[tokio::test]
    async fn recovery_resets_even_groups_not_due_this_round() {
        let bus = Arc::new(MockBus::new());
        put_hvac_first_case(&bus);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![hvac_interleaved_conf()], bus.clone(), sink.clone());

        // t=0：承载组（5000）读失败 ⇒ 站离线；快组（2000）同 tick 首采 ⇒ 只建基线（31 位快照）
        bus.fail_input_once(1, 0);
        sched.tick_once(0).await;
        assert_eq!(sink.event_count("hvac", "offline"), 1, "承载组失败 ⇒ offline 一次");
        {
            let st = sched.state.read().unwrap();
            assert_eq!(st[0].offline_count, 1, "offline_count = 1（站离线）");
        }

        // t=2000 / t=4000：**只有快组**到期（承载组退避到 5000）；位未变 ⇒ 零事件、零位点
        sched.tick_once(2000).await;
        sched.tick_once(4000).await;
        assert_eq!(bus.bit_call_count(1, 0), 3, "快组 0/2000/4000 各一轮");
        assert_eq!(bus.input_call_count(1, 0), 1, "承载组仅 t=0 一轮（退避到 5000）");
        assert_eq!(
            sink.events_since("hvac", 1),
            Vec::<(String, f64)>::new(),
            "快组位未变 ⇒ 除 offline 外零事件"
        );
        assert_eq!(
            sink.telemetry_call_count("hvac"),
            1,
            "此前仅 t=0 快组的 31 位全量快照 1 次"
        );

        // **离线期间**位 10（`hvac_di_11`）由 0→1 —— 快组**尚未观测到**（它下次到期才采）
        let mut bits = vec![false; 31];
        bits[10] = true;
        bus.put_bits(1, 0, bits);

        // t=5000：**承载组**到期且成功 ⇒ 站恢复；快组本轮**不到期**（next_due = 6000）
        sched.tick_once(5000).await;
        assert_eq!(sink.event_count("hvac", "online"), 1, "恢复 ⇒ online 一次");
        assert_eq!(
            bus.bit_call_count(1, 0),
            3,
            "快组在恢复轮**不到期**（t=5000 不是 2000 的倍数）⇒ 本用例钉的正是「未到期组」"
        );

        // t=6000：快组**恢复后首次到期** ⇒ 必须只产基线、**不产假事件**
        sched.tick_once(6000).await;
        assert_eq!(bus.bit_call_count(1, 0), 4, "快组 t=6000 首次到期");
        assert_eq!(
            sink.event_count("hvac", "hvac_di_11"),
            0,
            "未到期组也已被恢复轮一并重建基线 ⇒ 恢复后首轮不得把 0→1 误报为真事件"
        );
        assert_eq!(
            sink.events_since("hvac", 1),
            vec![("online".to_string(), 1.0)],
            "全窗口**只**产 online 一条"
        );
        // 该轮为**全量快照**（基线刚重建）——非"仅 1 个变化位"（旧写法：1 项）
        let calls = sink.telemetry_calls_of("hvac");
        assert_eq!(
            calls.len(),
            3,
            "t=0 快组快照 / t=5000 承载组标量 / t=6000 快组快照"
        );
        assert_eq!(
            calls.last().map(|c| c.len()),
            Some(31),
            "t=6000 轮须为全量快照（31 位）；旧写法该轮只落 1 个「变化位」"
        );
    }

    /// §12.8 用例⑫（**S-3 判别锚**）：**承载组持续失败**期间，非承载组仍逐轮产变化沿事件、
    /// 且**不每轮全量落位**。
    ///
    /// 判别性：按旧写法（① 段只判 `station_was_offline`、不含 `is_carrier`）—— 承载组持续失败时
    /// `offline_count > 0` 对非承载组**恒真** ⇒ 非承载组**每轮**成功都把全组基线 `reset()` ⇒
    /// ② 取到 **0 条**事件（位跳变被静默吞掉）且 ③ 取到 **288** 项/轮（每轮全量落位，
    /// ≈2.5×10⁷ 行/天）⇒ 本用例必红。
    #[tokio::test]
    async fn non_carrier_group_events_survive_carrier_failure() {
        let bus = Arc::new(MockBus::new());
        put_battery_split(&bus);
        let sink = Arc::new(FakeSink::default());
        let sched = build(vec![battery_split_conf()], bus.clone(), sink.clone());

        sched.tick_once(0).await; // t=0：两组建基线
        sched.tick_once(1000).await;

        // 承载组在窗口内**持续失败、不恢复** ⇒ offline_count 自 t=2000 起恒 > 0
        bus.fail_input_once(1, 100);
        sched.tick_once(2000).await;
        assert_eq!(
            sink.event_count("battery", "offline"),
            1,
            "去抖后 offline 仍 1 条"
        );

        // t=3000：位 201 由 0→1 ⇒ **非承载组照常产变化沿事件**（第 1 条）
        bus.put_bits(1, 200, alarm_bits_201(true));
        sched.tick_once(3000).await;
        assert_eq!(
            sink.events_since("battery", 1),
            vec![("bms_alarm_2".to_string(), 1.0)],
            "非承载组在承载组失败期间仍逐轮产变化沿事件（t=3000 轮）"
        );

        bus.fail_input_once(1, 100);
        sched.tick_once(4000).await;
        bus.put_bits(1, 200, alarm_bits_201(false));
        sched.tick_once(5000).await; // 1→0 不产事件（位块只上升沿）
        assert_eq!(sink.event_count("battery", "bms_alarm_2"), 1);

        bus.fail_input_once(1, 100); // 见下注：承载组因 oc=2 的退避已不在 t=6000 到期
        bus.put_bits(1, 200, alarm_bits_201(true));
        sched.tick_once(6000).await; // 第 2 条上升沿
        assert_eq!(
            sink.events_since("battery", 2),
            vec![("bms_alarm_2".to_string(), 1.0)],
            "t=6000 轮再产 1 条（两次 0→1 各 1 条）"
        );

        // ① 非承载组每 tick 照常到期（t=0…6000 共 7 次），**不被**承载组失败影响
        assert_eq!(bus.bit_call_count(1, 200), 7);
        // ③ **不每轮全量落位**：只有首轮是 288 项的全量快照
        let calls = sink.telemetry_calls_of("battery");
        assert_eq!(
            calls.iter().filter(|c| c.len() == 288).count(),
            1,
            "仅首轮全量快照（旧写法：非承载组每轮被重置基线 ⇒ 每轮 288 项）"
        );
        let only_201 = calls
            .iter()
            .find(|c| c.iter().any(|(m, v)| m == "bms_alarm_2" && *v == 1.0))
            .expect("t=3000 轮应先落位位 201 的遥测");
        assert_eq!(
            only_201.len(),
            1,
            "t=3000 轮该组位遥测项数 = 1（仅变化的位 201）"
        );
        // ④ 站级语义不被非承载组污染：非承载组成功**不**清 offline_count、不产 online
        assert_eq!(sink.event_count("battery", "online"), 0);
        {
            let st = sched.state.read().unwrap();
            assert!(
                st[0].offline_count > 0,
                "承载组持续失败 ⇒ offline_count 恒 > 0"
            );
        }
    }

    /// §12.8 用例⑬（**V-5**）：单组站上 `DueCalc::due_round` 的返回序与既有
    /// `(role_priority, 站序)` **逐项一致**，且每站恒恰 1 个条目、恒为承载组
    /// （`!is_carrier = false` 为常量 ⇒ 新增的排序键退化为既有键）。
    #[test]
    fn single_group_ordering_unchanged() {
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
        // cfg 序 = [hvac(prio 2), pcs(prio 1), grid(prio 0)] ⇒ 到期序须为 grid → pcs → hvac
        let stations = [
            hvac_conf("hvac", "ttyS1", 3, 1000),
            pcs,
            grid_conf("grid", "ttyS1", 1, 1000),
        ];
        let group: Vec<(usize, &StationConf)> = stations.iter().enumerate().collect();
        let mut calc = DueCalc::from_group(&group);

        let round = calc.due_round(0);
        assert_eq!(
            round.iter().map(|p| p.station_index).collect::<Vec<_>>(),
            vec![2, 1, 0],
            "单组站：返回序 = 既有 (role 优先级, 站序)"
        );
        assert!(
            round.iter().all(|p| p.is_carrier),
            "单组站：唯一组即承载组（C8）"
        );
        assert!(round.iter().all(|p| !p.was_failing));
        assert_eq!(round.len(), 3, "单组站每站恒恰 1 个条目（V-5）");
        // 同一 now 二次调用不再返回（next_due 已推进）—— 既有口径不变
        assert!(calc.due_round(0).is_empty());
    }

    /// **交叉断言用例**（T8 评审建议、T9 落实）：**同一份配置下**，配置侧
    /// `config::criterion_block_indices` 给出的 `R(role)` 块下标集，与运行期 `judges_evaluable`
    /// 的判据**必须一致** —— 两者是**同一定义的两种求值域**（一侧"哪些块下标在 R 内"、
    /// 一侧"这些块在本组读集内是否读成功"）。本用例防两侧 `R(role)` 定义漂移。
    #[test]
    fn runtime_guard_and_config_criterion_agree() {
        // ── fire：**判据跨组**用例的同一份配置（`fire_det` 提速 ⇒ 判据块分属两组）──
        let mut fire = fire_conf("ttyS6", 1, 1);
        fire.interval_ms = 5000;
        fire.regs[1].interval_ms = Some(1000);
        assert_guard_matches_criterion(&fire);
        // ── battery：`soc` 用例（R = 承载 `soc` 点的那一块）──
        assert_guard_matches_criterion(&battery_soc_conf());

        // ── meter_grid：**两侧集合必须相等**（含 `p_total`）──
        // PRD §10.3.2 的 `R(role)` 定义表是唯一权威口径且含 `p_total` ⇒ 运行期守卫的硬要求集
        // 必须与配置侧 `criterion_block_indices` **逐字同集**（下方对全块集双向断言：R 内块
        // 去掉 ⇒ 守卫 false；R 外块去掉 ⇒ 守卫仍 true ⇒ 集合相等，非仅单向包含）。
        let grid = grid_conf("grid_meter", "ttyS4", 1, 1000);
        let r = crate::config::criterion_block_indices(&grid);
        assert_eq!(
            r,
            vec![0, 1, 2, 3, 4, 5],
            "配置侧 R = 6 块（含 p_total —— 供 scalar_total 降级求和）"
        );
        let all: Vec<usize> = (0..grid.regs.len()).collect();
        // ① R 内逐块去掉 ⇒ 守卫 false（缺任一判据块 ⇒ 判据不全）
        for &i in &r {
            let kept: Vec<usize> = all.iter().copied().filter(|&k| k != i).collect();
            assert!(
                !judges_evaluable(Role::MeterGrid, &ok_reads(&grid, &kept)),
                "grid 的判据块 {}（下标 {}）被去掉后守卫仍为 true ⇒ 两侧口径漂移",
                grid.regs[i].name,
                i
            );
        }
        // ② 全块齐备 ⇒ 守卫 true；R 外块（本配置为空集）去掉 ⇒ 守卫仍 true
        assert!(
            judges_evaluable(Role::MeterGrid, &ok_reads(&grid, &all)),
            "R 齐备时守卫应为 true"
        );
        for &i in all.iter().filter(|i| !r.contains(i)) {
            let kept: Vec<usize> = all.iter().copied().filter(|&k| k != i).collect();
            assert!(
                judges_evaluable(Role::MeterGrid, &ok_reads(&grid, &kept)),
                "非判据块 {} 不在 R 内 ⇒ 去掉它不得让守卫为 false（两侧集合须相等）",
                grid.regs[i].name
            );
        }
    }

    /// **守卫的 `Battery` 支路：`soc` 块读回**截断** ⇒ 必须 `false`（T9 评审遗留项）**。
    ///
    /// `SocOutcome::BlockFailed` 覆盖**两种**形态（见 [`judges_evaluable`] 的 `Battery` 支路注释）：
    /// ① 承载块 `Err(e)`；② **读回长度装不下 `soc` 点**（响应被截断）。旧写法
    /// `!matches!(.., NoSuchPoint)` 会把 ② 误判为"可求值"，而 `MeterGrid` 侧要求块 `is_ok()`
    /// ⇒ 两侧语义不对称。本用例**直接构造 `BlockReads`**（绕开 IO 与 `poll_group` 的"失败即
    /// 整组弃用"，故 ② 在此可达），钉住"② 同样必须 `false`"。
    ///
    /// **判别力**：把该支路改回 `!matches!(.., NoSuchPoint)` ⇒ ③ 截断断言**必红**
    /// （`BlockFailed ≠ NoSuchPoint` ⇒ 旧写法返回 `true`）。
    #[test]
    fn battery_guard_false_when_soc_block_truncated() {
        // 块：FC04 输入寄存器，**声明 `count = 2`**，`soc` 点落在**块内偏移 1**（`at = 2`、
        // `uint16` ⇒ 宽 1 寄存器 ⇒ 完整解码需 `len >= 1 + 1 = 2`）。
        let mut conf = battery_soc_conf();
        conf.regs[0].count = 2;
        conf.regs[0].points[0].at = 2;
        let b = conf.regs[0].clone();

        // ③ **截断**：实际只回 1 个寄存器（声明 2 > 载荷 1）⇒ 装不下偏移 1 处的 `soc` 点。
        let truncated: BlockReads = vec![(b.clone(), Ok(BlockData::Regs(vec![42])))];
        // 前置：确系**承载** `soc` 点的块（不是 `NoSuchPoint`——否则本用例会因"找不到点"
        // 而假绿，注入验证也真不出红），且落进 `BlockFailed` 的**② 截断**形态。
        assert!(
            matches!(
                mapper::battery_soc(&truncated),
                mapper::SocOutcome::BlockFailed(_)
            ),
            "构造必须落入 BlockFailed 的**截断**形态（而非 NoSuchPoint）"
        );
        assert!(
            !judges_evaluable(Role::Battery, &truncated),
            "读回截断 ⇒ 该 role 的站级判据本轮**不可求值** ⇒ 守卫必须 false"
        );

        // ④ **正常**：载荷长度足够（`len = 2 >= 2`）⇒ 可解出 ⇒ 可求值 ⇒ `true`。
        let ok: BlockReads = vec![(b, Ok(BlockData::Regs(vec![0, 42])))];
        assert!(
            matches!(mapper::battery_soc(&ok), mapper::SocOutcome::Value(_)),
            "构造必须可解出（否则 ④ 不是「正常」形态）"
        );
        assert!(
            judges_evaluable(Role::Battery, &ok),
            "soc 点本轮可解出 ⇒ 判据可求值 ⇒ 守卫 true"
        );
    }

    /// 断言"配置侧 `R(role)`"与"运行期守卫"对**同一块**的判定一致（**只**用于 R 非空的 role）：
    ///  ① R 的块全在且读成功（其余块也在 ⇒ 属"多余输入"）⇒ 守卫 `true`；
    ///  ② 逐个去掉 R 内的一块 ⇒ 守卫 `false`（配置说是判据块 ⟺ 运行期真的需要它）。
    fn assert_guard_matches_criterion(conf: &StationConf) {
        let r = crate::config::criterion_block_indices(conf);
        assert!(!r.is_empty(), "本辅助只用于 R 非空的 role");
        let all: Vec<usize> = (0..conf.regs.len()).collect();
        assert!(
            judges_evaluable(conf.role, &ok_reads(conf, &all)),
            "站 {} role={:?}：R={:?} 齐备时守卫应为 true",
            conf.id,
            conf.role,
            r
        );
        for &i in &r {
            let kept: Vec<usize> = all.iter().copied().filter(|&k| k != i).collect();
            assert!(
                !judges_evaluable(conf.role, &ok_reads(conf, &kept)),
                "站 {} 的判据块 {}（下标 {}）被去掉后守卫仍为 true ⇒ 配置侧 R 与运行期守卫口径漂移",
                conf.id,
                conf.regs[i].name,
                i
            );
        }
    }

    /// 由配置造一个"指定的块**全部读成功**"的读集（**不触 IO**，只服务运行期守卫的单测）：
    /// 寄存器块 → `Regs([0; count])`；位块 → `Bits([false; count])`。
    fn ok_reads(conf: &StationConf, indices: &[usize]) -> BlockReads {
        indices
            .iter()
            .map(|&i| {
                let b = conf.regs[i].clone();
                let data = if b.func == RegFunc::Discrete {
                    BlockData::Bits(vec![false; b.count as usize])
                } else {
                    BlockData::Regs(vec![0u16; b.count as usize])
                };
                (b, Ok(data))
            })
            .collect()
    }
}
