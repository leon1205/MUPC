//! 遥测缓冲丢弃的**健康巡检** —— 03 设计 §9.3 **缺口 1 / 缺口 2**（PRD R-11.5-A3② / A4，
//! 用例 **FLS-03②**）。
//!
//! # 为什么在 core-bin 而不是 `storage`（依赖方向的硬理由）
//!
//! 「丢弃必须产一条 `major` 级告警/事件」（S-3②）**不能**由 `storage` 自己满足：它没有、
//! 也**不应**有告警/事件通道 —— `AlertFeed` 在 core-bin，让 `storage` 依赖 core-bin 是倒向边。
//! ⇒ 落点只能是装配层：本任务**读** `WriteBuffer` 的既有观测面
//! （`dropped_points()` / `dropped_batches()`，`storage` 侧早已实现并单测），在**丢弃事件的
//! 边沿**投一条 `major` 告警。
//!
//! # 告警口径 = **边沿触发**（按 PRD R-11.5-A4 字面收口，替换原"增量 > 0 即发"的误读）
//!
//! 两个计数器都是**只增不减的累计值**（`AtomicU64`）。
//!
//! - 若每拍**无条件**上报累计值 ⇒ 「连续失败 10 个周期」产 **10 条**完全相同的告警
//!   （告警风暴，R-11.5-A4 禁止）；
//! - 但**"只在增量 > 0 时发"也不够**：它在「**每个周期都在丢**」时照样每周期发一条
//!   （10 周期 = 10 条），**字面违反** R-11.5-A4 与 FLS-03（连续失败 10 个周期不得产生
//!   ≥10 条告警）。那个口径只做到"无丢弃时不发"，**不等于**"按周期聚合" —— 设计 §9.3
//!   缺口 2 原表述"自然按周期聚合"即为误读（已随本批更正）。
//!
//! 正确落地是**边沿触发**（[`StorageHealthWatch`] 的状态机）：
//!
//! | 本拍增量 | 上一拍状态 | 动作 |
//! |---|---|---|
//! | > 0 | 静默（尚未进入） | **进入边沿** ⇒ 投**恰好 1 条** `major`（文案含本拍增量 + 累计值） |
//! | > 0 | 丢弃中 | **持续态** ⇒ **一条都不投**（增量并入本段） |
//! | = 0 | 丢弃中 | **退出边沿** ⇒ **只记一行 `info` 日志**（含本段总计 + 累计值）；**不发告警**（理由见下） |
//! | = 0 | 静默 | 静默 |
//!
//! ⇒ **连续丢弃 10 个周期 = 恰好 1 条告警**（满足 FLS-03 的"不得 ≥10 条"）；无丢弃 = 0 条；
//! 恢复后**再次进入** = 再 1 条（边沿可重现）。
//!
//! **"同一时段内的连续失败合并为一条并递增计数"的落法**：段内每一拍的增量都累加进
//! `episode_points` / `episode_batches`，退出边沿时如实打进日志 —— 即**同一条告警承载整段
//! 丢弃事件**，计数由**日志 / 可观测计数**承担，而不是每周期刷一条新告警。
//!
//! # 恢复为什么不发告警（本批裁定：二选一里的"只记日志"侧）
//!
//! `AlertFeed::push_system_alert(level, ..)` 是**只有入、没有 ack/清除面**的投递环，消费侧按
//! `FeedItem::subtype` 取级。一条"已恢复"若同样以 `major` 投进去，对任何按级过滤的消费者
//! **与"新发生一次 `major` 告警"无法区分**（正是响应 ② 的 notifier 语义冲突）；换一个自造级别
//! （如 `major-recovered`）又等于**造 PRD/设计词表里没有的级别**，按 `major` 过滤的消费者依旧看不见。
//! 两难 ⇒ 取**"环里每一条 `major` = 一次新的丢弃事件开始"**这条干净不变量；恢复只记日志
//! （`tracing::info!`），并由 FLS-03③ 的**可观测计数**（增量回落到 0）让现场判定。
//!
//! ⚠️ **口径边界（如实登记）**：R-11.5-A4 的字面场景是"**持续失败**"，本状态机对"**每个周期
//! 都在丢**"（缓冲被持续顶穿）同样只发 1 条 ⇒ 字面已满足。
//!
//! # 与既有任务的关系（退出编排）
//!
//! 与 `flush_timer` / `grid_agg_timer` **同范式、同名单**：spawn 后句柄交 `producers`
//! （协作退出名单），收到停机信号即退出。本任务**不持有任何数据、不碰 `WriteBuffer` 本体**
//! （只读两个原子计数），故收工分支无需额外 flush —— 收工时刻也不会产生"落在最后一次 flush
//! 之后"的写者。
//!
//! **为什么必须走协作名单而不是 abort 名单（措辞订正，评审 W-2）**：真差别**不是**"abort 名单
//! 会在停机瞬间仍投出告警" —— 两个名单都在 `stop_tx.send(true)` **之后**处理，且 `select!` 的
//! tick 臂与 stop 臂本就随机竞争，"停机瞬间恰好多投一条"在**两个名单下都可能**。真差别是：
//! 协作名单会 **join、确认它已收工**（`stop_producers` 有上限地等每个任务回执）⇒ 只有它能
//! 保证 **`shutdown()` 返回后没有本任务在跑**；abort 名单只发 abort 请求、不 join ⇒ 无法给出
//! 这一保证（本任务虽无数据要落盘，但"退出后仍在跑"本身即该名单要排除的形态）。

use std::sync::Arc;
use std::time::Duration;

use crate::alert_feed::AlertFeed;

/// 巡检周期（03 设计 §9.3 缺口 1：「新增，周期 1 s 或复用既有 tick」）。
///
/// 取 **1 s**（而不是复用 `grid_agg_timer` 的 tick）：两个理由 —— ① 单一职责，聚合任务的
/// 收工契约（drain 通道 + 关闭未闭合周期 + 入队）与"读告警计数"毫无关系，混在一起会让它的
/// 停机语义变模糊；② 本周期与两个既有 1 s 任务**同节拍**，故"契合既有架构"体现在**同名单、
/// 同周期**，而不是把两件事塞进一个 `select!`。
pub const HEALTH_TICK_MS: u64 = 1_000;

/// 丢弃告警的级别（设计原文写死 `"major"`；落到 `AlertFeed::push_system_alert(level, ..)`
/// 的 `level` 字段 ⇒ 订阅侧 `FeedItem.subtype == "major"`）。
pub const DROP_ALERT_LEVEL: &str = "major";

/// 巡检的**本拍输出**（三条互斥出路；见模块头口径表）。
///
/// **为什么是 enum 而不是 `Option<String>`**：判据有**三种**结果（静默 / 进入丢弃态 / 退出丢弃态），
/// `Option` 只能表达两种 ⇒ 生产循环里的 `if let Some(..)` 会把"恢复"与"静默"混为一谈，让
/// **"恢复只记日志、不发告警"这条裁定无法在类型上表达**（改成发告警也不必改任何 `match` 臂，
/// 编译不会拦）。三态 enum + 穷举 `match` 让该裁定在**类型层**立住。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthSignal {
    /// **进入边沿**：本拍有增量、上一拍静默 ⇒ 投**恰好一条** `major` 告警（文案见 [`StorageHealthWatch::poll`]）。
    DropStarted(String),
    /// **退出边沿**：本拍无增量、上一拍在丢弃中 ⇒ 该段丢弃事件结束 ⇒ **只记日志、不发告警**。
    DropRecovered {
        /// 本段（本次丢弃事件）累计新增点数。
        episode_points: u64,
        /// 本段累计"丢最旧"次数。
        episode_batches: u64,
        /// 只增不减的累计点数（含此前各段）。
        total_points: u64,
        /// 只增不减的累计次数（含此前各段）。
        total_batches: u64,
    },
    /// 无变化，或处于**持续丢弃态** ⇒ **什么都不投**（"连续丢弃 10 周期只 1 条告警"的落点）。
    Silent,
}

/// 丢弃计数的**跨周期视图**（上一拍的读数 + **边沿状态机**）。
///
/// 单拎成一个纯结构是刻意的（同 `stop_producers` / `console_write_paths` 的手法）：巡检的
/// **判据**在这里、"什么时候读""读到谁"在任务里 ⇒ 判据可以脱离真实时钟与真实 `WriteBuffer`
/// 被确定性单测（本模块 `tests`）。
///
/// 状态 = ① 上一拍两个累计读数（用于差分）② 上一拍**是否处于丢弃中**（边沿记忆）
/// ③ 本段丢弃事件的**累计增量**（"递增计数"的载体，退出边沿时交给日志）。
#[derive(Default)]
pub struct StorageHealthWatch {
    last_points: u64,
    last_batches: u64,
    /// 上一拍是否处于"丢弃中"（本拍增量 > 0）。**唯一**决定"发/不发"的边沿位。
    in_drop: bool,
    /// 本段丢弃事件（自进入边沿起）累计的点增量。
    episode_points: u64,
    /// 本段丢弃事件累计的批次增量。
    episode_batches: u64,
}

impl StorageHealthWatch {
    pub fn new() -> Self {
        Self {
            last_points: 0,
            last_batches: 0,
            in_drop: false,
            episode_points: 0,
            episode_batches: 0,
        }
    }

    /// 用**当前累计值**推进一拍，返回本拍应执行的**边沿动作**（[`HealthSignal`]）。
    ///
    /// - **进入边沿** ⇒ `DropStarted(文案)`：文案**必须**同时含**本拍增量条数**与**累计值**
    ///   （设计原文）；`d_batches` 与累计次数一并带上，便于现场区分"一次丢一大批"与"多次各丢一点"。
    /// - **持续态** ⇒ `Silent`：本拍增量并入 `episode_*`，**不投**（这正是 R-11.5-A4 的聚合）。
    /// - **退出边沿** ⇒ `DropRecovered { .. }`：交还本段总计与累计值，由调用方**记日志**（不发告警）。
    /// - 其余 ⇒ `Silent`。
    pub fn poll(&mut self, dropped_points: u64, dropped_batches: u64) -> HealthSignal {
        // `saturating_sub`：计数器只增不减（`AtomicU64`），正常情况下不会倒退；万一读数倒退
        // （不可能：进程内单实例递增），按 0 处理 —— 宁可漏报一拍，也不伪造一个巨大的增量。
        let d_points = dropped_points.saturating_sub(self.last_points);
        let d_batches = dropped_batches.saturating_sub(self.last_batches);
        self.last_points = dropped_points;
        self.last_batches = dropped_batches;

        let dropping = d_points > 0 || d_batches > 0;
        match (self.in_drop, dropping) {
            // ── 进入边沿：投**恰好一条** ──
            (false, true) => {
                self.in_drop = true;
                self.episode_points = d_points;
                self.episode_batches = d_batches;
                HealthSignal::DropStarted(format!(
                    "遥测缓冲丢弃：本周期新增 {d_points} 点（{d_batches} 次超限丢弃），\
                     累计 {dropped_points} 点（{dropped_batches} 次）"
                ))
            }
            // ── 持续态：一条都不投，只把增量并入本段（saturating 与上面的差分同口径） ──
            (true, true) => {
                self.episode_points = self.episode_points.saturating_add(d_points);
                self.episode_batches = self.episode_batches.saturating_add(d_batches);
                HealthSignal::Silent
            }
            // ── 退出边沿：交还本段总计，清段，**不发告警**（调用方只记日志） ──
            (true, false) => {
                self.in_drop = false;
                let (episode_points, episode_batches) = (self.episode_points, self.episode_batches);
                self.episode_points = 0;
                self.episode_batches = 0;
                HealthSignal::DropRecovered {
                    episode_points,
                    episode_batches,
                    total_points: dropped_points,
                    total_batches: dropped_batches,
                }
            }
            // ── 静默 ──
            (false, false) => HealthSignal::Silent,
        }
    }
}

/// 巡检循环（**可注入读源与投递口** ⇒ 可确定性单测；生产接线见
/// [`spawn_storage_health_timer`]）。
///
/// - `read`：读当前**累计**丢弃计数（生产 = `WriteBuffer::dropped_points/dropped_batches`）；
/// - `emit(level, message)`：投递一条告警（生产 = `AlertFeed::push_system_alert`）。
///   ⚠️ **只在"进入边沿"被调用**（持续态/退出边沿都不调）—— 这是 FLS-03② 的核心判据
///   （持续丢弃 10 拍仍恰 1 条）；
/// - `stop`：协作停机信号（收到即退出；本任务无在途数据，无需 flush 收尾）。
pub async fn run_health_loop<R, E>(
    watch: &mut StorageHealthWatch,
    period: Duration,
    mut stop: tokio::sync::watch::Receiver<bool>,
    mut read: R,
    mut emit: E,
) where
    R: FnMut() -> (u64, u64),
    E: FnMut(&str, &str),
{
    let mut ticker = tokio::time::interval(period);
    // 落后时顺延（不追赶补打）：巡检是"看当前值"的操作，补打只会重复读同一个数（增量恒 0）。
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // 第一次 `tick()` 立即返回：吞掉它，避免启动瞬间多一次无意义的巡检。
    ticker.tick().await;
    loop {
        tokio::select! {
            _ = stop.changed() => {
                tracing::debug!("停机信号：遥测缓冲健康巡检收工（本任务不持有数据，无需 flush 收尾）");
                break;
            }
            _ = ticker.tick() => {}
        }
        let (points, batches) = read();
        match watch.poll(points, batches) {
            // 进入边沿 ⇒ 恰好一条 `major` 告警
            HealthSignal::DropStarted(message) => emit(DROP_ALERT_LEVEL, &message),
            // 退出边沿 ⇒ **只记日志**（裁定与理由见模块头「恢复为什么不发告警」）
            HealthSignal::DropRecovered {
                episode_points,
                episode_batches,
                total_points,
                total_batches,
            } => {
                tracing::info!(
                    "遥测缓冲丢弃已恢复：本段累计新增 {episode_points} 点（{episode_batches} 次超限丢弃），\
                     累计 {total_points} 点（{total_batches} 次）"
                );
            }
            // 持续态 / 静默 ⇒ 不投任何东西
            HealthSignal::Silent => {}
        }
    }
}

/// 生产装配：起巡检任务（周期 [`HEALTH_TICK_MS`]）。
///
/// 调用方**必须**把返回的句柄登记进 `producers`（协作退出名单，与 `flush_timer` 同处），
/// 由 `StartupContext::shutdown` 在最后那次 flush **之前**等它确认收工 —— 否则停机瞬间它
/// 仍可能往 `AlertFeed` 里投一条告警（环是有界的广播，投递本身无害，但"退出后还有生产者在跑"
/// 这件事必须消除）。
pub fn spawn_storage_health_timer(
    write_buffer: Arc<mupc_storage::WriteBuffer>,
    alert_feed: Arc<AlertFeed>,
    stop: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    let mut watch = StorageHealthWatch::new();
    tokio::spawn(async move {
        run_health_loop(
            &mut watch,
            Duration::from_millis(HEALTH_TICK_MS),
            stop,
            // 读源 = `storage` 的既有观测面（**不新增接口**、不碰缓冲本体）
            || {
                (
                    write_buffer.dropped_points(),
                    write_buffer.dropped_batches(),
                )
            },
            // 投递口 = 既有 `AlertFeed`（文案风格与 `SouthSink::record_event` 那条一致：
            // 单行人可读、全角标点、`push_system_alert(level, message)` 的既有用法）
            |level, message| {
                alert_feed.push_system_alert(level, message);
            },
        )
        .await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Mutex;

    /// 有上限地等 `cond` 成立（最多 **3 s**；每 10 ms 探一次）。返回是否成立。
    ///
    /// 上限取 3 s 而不是"够用就行"的几百 ms：本用例与另外 388 个用例在 `-j 2` 的同一 harness 里
    /// 抢 CPU，固定 `sleep` 推出的节拍数**会随负载漂移**（曾被饿到 200 ms 只跑 2 拍 ⇒ 假红）。
    async fn wait_until(mut cond: impl FnMut() -> bool) -> bool {
        for _ in 0..300 {
            if cond() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    /// 取出 `DropStarted` 的文案；其余变体一律 panic（**用例 ③ 靠它区分"进入"与"恢复"**：
    /// 恢复若被错做成"也投一条告警"，这里就会 panic）。
    fn started_message(signal: HealthSignal) -> String {
        match signal {
            HealthSignal::DropStarted(message) => message,
            other => panic!("期望进入边沿 `DropStarted`，实得 {other:?}"),
        }
    }

    /// 从文案里抠出「累计 {n} 点」的 `n`（断言累计值随段数**递增**，而不是恒为第一段的量）。
    fn parse_total_points(message: &str) -> u64 {
        message
            .split("累计 ")
            .nth(1)
            .and_then(|rest| rest.split(" 点").next())
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("文案里没有可解析的「累计 N 点」：{message}"))
    }

    /// 级别常量就是 `"major"`（**定义式断言**，同 `alert_feed.rs` 的
    /// `minimal_form_capacity_is_64` 的强度口径：它守的是"没人把常量改掉"，即契约字面；
    /// 运行期行为由 `edge_triggered_loop_emits_one_alert_per_episode_not_per_tick` 用**字面量**
    /// 断言覆盖）。改这里的 `"major"` 必须同改 03 设计 §9.3 缺口 1 / PRD R-11.5-A3②。
    #[test]
    fn drop_alert_level_is_major() {
        assert_eq!(DROP_ALERT_LEVEL, "major");
    }

    /// **用例 ⑤（纯判据层）**：**全程无丢弃 ⇒ 0 条告警**（读数恒 0 时状态机恒静默）。
    ///
    /// 改什么会让本条红：把 `poll` 的静默臂改成出文案（如无条件 `Some(..)`）。
    #[test]
    fn no_drop_means_no_alert_at_all() {
        let mut w = StorageHealthWatch::new();
        for _ in 0..10 {
            assert_eq!(
                w.poll(0, 0),
                HealthSignal::Silent,
                "读数恒 0 ⇒ 每一拍都不得投告警"
            );
        }
    }

    /// **用例 ①（纯判据层）**：**进入边沿 ⇒ 恰 1 条**，文案同时含"本周期增量"与"累计值"两个数字；
    /// 且**只有进入那一拍**出文案（读数未变的下一拍不得重复投）。
    ///
    /// 另覆盖"**只涨批次不涨点数**"的进入（一句日志里丢 0 点但有 1 次超限）⇒ 文案必须如实带两个增量。
    #[test]
    fn edge_enter_emits_exactly_one_alert_with_increment_and_total() {
        let mut w = StorageHealthWatch::new();
        let first = started_message(w.poll(7, 2));
        assert!(first.contains("本周期新增 7 点"), "文案含增量条数：{first}");
        assert!(first.contains("累计 7 点"), "文案含累计值：{first}");
        assert_eq!(parse_total_points(&first), 7);

        // 只涨"丢弃次数"不涨"丢弃点数"从**静默态**进入 ⇒ 也必须投（两个计数器任一增长都算增量）
        let mut w2 = StorageHealthWatch::new();
        let only_batches = started_message(w2.poll(0, 1));
        assert!(
            only_batches.contains("本周期新增 0 点") && only_batches.contains("1 次超限丢弃"),
            "文案必须如实带两个增量：{only_batches}"
        );
    }

    /// **用例 ②（纯判据层，R-11.5-A4 / FLS-03 的**核心判据**）**：
    /// **连续丢弃 10 拍 ⇒ 累计仍恰 1 条告警**，且**计数确实在涨**（不是任务停了）。
    ///
    /// 这正是"作废"的那条旧口径（"增量 > 0 即发"）会红的用例：它在 10 拍里发 10 条。
    ///
    /// 改什么会让本条红：`poll` 的持续态臂改成返回 `DropStarted`（每拍一条）；
    /// 或状态位 `in_drop` 被忽略/未保持。
    #[test]
    fn ten_consecutive_drop_ticks_still_yield_exactly_one_alert() {
        let mut w = StorageHealthWatch::new();
        let mut alerts = 0usize;
        // 第 1 拍：进入（增量 1 点）
        let msg = started_message(w.poll(1, 0));
        alerts += 1;
        assert_eq!(parse_total_points(&msg), 1, "进入时累计 = 1：{msg}");
        // 其后 9 拍**每拍都还在丢**（累计读数每拍 +1）⇒ 一条都不许再投
        for tick in 2..=10u64 {
            assert_eq!(
                w.poll(tick, 0),
                HealthSignal::Silent,
                "持续丢弃第 {tick} 拍不得再投告警（每周期各报一条 = 告警风暴）"
            );
        }
        assert_eq!(
            alerts, 1,
            "连续丢弃 10 拍 ⇒ 累计恰 1 条告警，实得 {alerts} 条"
        );
        // 第 11 拍读数**冻结**（本轮不再丢）⇒ 正是"恢复边沿"，且本段总计 = 10：
        // 10 拍的丢弃**全被一条告警 + 一条恢复日志承载**（"合并为一条并递增计数"）
        match w.poll(10, 0) {
            HealthSignal::DropRecovered {
                episode_points,
                total_points,
                ..
            } => {
                assert_eq!(episode_points, 10, "本段总计 = 逐拍累加的 10 点");
                assert_eq!(total_points, 10);
            }
            other => panic!("读数冻结 ⇒ 必须是恢复边沿，实得 {other:?}"),
        }
    }

    /// **用例 ③（纯判据层）**：**恢复（退出边沿）**被信号化为 `DropRecovered`（含本段总计 +
    /// 累计值），**不是** `DropStarted` ⇒ 调用方**不发告警、只记日志**（钉住本批裁定的那一侧；
    /// 若改成"恢复也投一条告警"，`started_message` 会 panic，任务层那条断言也会红）。
    ///
    /// 同时证"本段总计"如实累加了段内**每一拍**的增量：3 + 4 + 5 = 12 点、0 + 1 + 1 = 2 次。
    #[test]
    fn recovery_edge_is_signalled_as_log_only_not_as_a_new_alert() {
        let mut w = StorageHealthWatch::new();
        let _ = started_message(w.poll(3, 0)); // 进入
        assert_eq!(
            w.poll(7, 1),
            HealthSignal::Silent,
            "持续态：只并入本段，不投"
        );
        assert_eq!(w.poll(12, 2), HealthSignal::Silent, "同上");
        // 退出边沿：读数冻结 ⇒ 恢复
        match w.poll(12, 2) {
            HealthSignal::DropRecovered {
                episode_points,
                episode_batches,
                total_points,
                total_batches,
            } => {
                assert_eq!(episode_points, 12, "本段总计 = 3 + 4 + 5（每拍都累加进来）");
                assert_eq!(episode_batches, 2, "本段总计 = 0 + 1 + 1");
                assert_eq!(total_points, 12);
                assert_eq!(total_batches, 2, "累计次数 = 最后读数");
            }
            other => panic!("恢复必须是 `DropRecovered`（只记日志、不发告警），实得 {other:?}"),
        }
        // 恢复后仍静默
        assert_eq!(w.poll(12, 2), HealthSignal::Silent, "恢复后静默期不得再投");
    }

    /// **用例 ④（纯判据层）**：恢复后**再次进入** ⇒ 再 1 条（边沿可重现，两段共 2 条）；
    /// 第二段的 `episode_*` **从 0 起算**（不把上一段的量带进来），而累计值继续递增。
    #[test]
    fn edge_is_reproducible_after_recovery() {
        let mut w = StorageHealthWatch::new();
        // ── 第 1 段（进入 ⇒ 持续 ⇒ 恢复） ──
        let first = started_message(w.poll(2, 0)); // 进入：本拍 +2，累计 2
        assert_eq!(parse_total_points(&first), 2);
        assert_eq!(
            w.poll(4, 0),
            HealthSignal::Silent,
            "第 1 段持续一拍：+2 并入本段，不投"
        );
        assert!(
            matches!(w.poll(4, 0), HealthSignal::DropRecovered { .. }),
            "读数冻结 ⇒ 第 1 段恢复边沿"
        );

        // ── 第 2 段：再次进入 ⇒ **再 1 条**（边沿可重现） ──
        let second = started_message(w.poll(5, 0)); // 累计 4 → 5
        assert!(
            second.contains("本周期新增 1 点") && second.contains("累计 5 点"),
            "第二次进入必须报**本段进入那拍的增量 1**与**累计 5**：{second}"
        );
        assert_eq!(w.poll(7, 0), HealthSignal::Silent, "第 2 段持续第 1 拍");
        assert_eq!(w.poll(10, 0), HealthSignal::Silent, "第 2 段持续第 2 拍");
        // 第 2 段恢复 ⇒ 本段总计**只有第二段的量**（段界被正确清空），累计值跨段继续递增
        match w.poll(10, 0) {
            HealthSignal::DropRecovered {
                episode_points,
                total_points,
                ..
            } => {
                assert_eq!(
                    episode_points,
                    1 + 2 + 3,
                    "第二段总计 = 1+2+3（不含第一段的 2）"
                );
                assert_eq!(total_points, 10, "累计值跨段继续递增");
            }
            other => panic!("第二段恢复必须是 `DropRecovered`，实得 {other:?}"),
        }
    }

    /// **用例 ①②③④⑤（任务层：真实时钟 + 真实任务，读源/投递口可注入）**
    /// + **用例 ⑥（判据加强：静默/持续窗口内"真的又跑了 ≥N 拍"，防"任务已死 ⇒ 不发"的假绿）**。
    ///
    /// 判据强度靠 `reads` 计数（**等拍数，不用固定 `sleep`** —— 本用例与其余 ~388 条测试在
    /// `-j 2` 的同一 harness 里抢 CPU，固定 sleep 推出的节拍数会随负载漂移、实测假红）。
    ///
    /// 模拟口径：读闭包**每被调用一次就替缓冲"又丢 1 点"**（仅当 `dropping` 位为真）——
    /// 这正是"**每个周期都在丢**"（缓冲被持续顶穿）的形态；`dropping` 为假时读数冻结（无丢弃拍）。
    ///
    /// 改什么会让本条红：`poll` 持续态改回"增量 > 0 即发"（② 会在 10 拍里攒出 >1 条）；
    /// 恢复改成发告警（③ 的"恢复不产新告警"红）；去掉边沿位（④ 的第二段不再出第二条告警）。
    #[tokio::test]
    async fn edge_triggered_loop_emits_one_alert_per_episode_not_per_tick() {
        let points = Arc::new(AtomicU64::new(0));
        // true = 缓冲被持续顶穿：每拍都再丢 1 点
        let dropping = Arc::new(AtomicBool::new(false));
        // 巡检实际跑过的拍数（证"窗口内任务确实在跑"，排除假绿）
        let reads = Arc::new(AtomicU64::new(0));
        let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));

        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let p = points.clone();
        let d = dropping.clone();
        let r = reads.clone();
        let s = seen.clone();
        let handle = tokio::spawn(async move {
            let mut w = StorageHealthWatch::new();
            run_health_loop(
                &mut w,
                // 40 ms/拍：本用例要在秒级内跑过 ≥10 拍（生产是 1 s，见 HEALTH_TICK_MS）
                Duration::from_millis(40),
                stop_rx,
                || {
                    r.fetch_add(1, Ordering::SeqCst);
                    if d.load(Ordering::SeqCst) {
                        p.fetch_add(1, Ordering::SeqCst);
                    }
                    (p.load(Ordering::SeqCst), 0)
                },
                |level, message| {
                    s.lock()
                        .unwrap()
                        .push((level.to_string(), message.to_string()));
                },
            )
            .await;
        });

        let snapshot = || {
            let v = seen.lock().unwrap();
            v.iter()
                .map(|(l, m)| (l.clone(), m.clone()))
                .collect::<Vec<_>>()
        };
        // 等任务真的又跑了 ≥`n` 拍（用例 ⑥ 的加强：只等"没投"可能只是任务已死）
        async fn wait_ticks(reads: &AtomicU64, n: u64) {
            let before = reads.load(Ordering::SeqCst);
            assert!(
                wait_until(|| reads.load(Ordering::SeqCst) - before >= n).await,
                "窗口内必须真的又巡检了 ≥{n} 拍（上限 3 s；实得 {} 拍）",
                reads.load(Ordering::SeqCst) - before
            );
        }

        // ⑤ 全程无丢弃 ⇒ 0 条（先跑过 ≥3 拍；等拍数，不用固定 sleep）
        wait_ticks(&reads, 3).await;
        assert!(
            snapshot().is_empty(),
            "无丢弃 ⇒ 0 条告警，实得 {:?}",
            snapshot()
        );

        // ① 进入边沿 ⇒ 恰 1 条，且含增量与累计两个数字
        dropping.store(true, Ordering::SeqCst);
        assert!(
            wait_until(|| !snapshot().is_empty()).await,
            "出现丢弃后必须投出告警（否则现场对丢数完全无感）"
        );
        let got = snapshot();
        assert_eq!(got.len(), 1, "进入边沿只投一条，实得 {got:?}");
        // 断言**字面量**（不引常量）：否则"常量被改掉"这条恒绿 —— 级别是 PRD/设计的硬要求
        assert_eq!(got[0].0, "major", "级别必须是 major（PRD R-11.5-A3②）");
        assert!(
            got[0].1.contains("本周期新增 1 点"),
            "文案必须含增量条数：{}",
            got[0].1
        );
        assert_eq!(
            parse_total_points(&got[0].1),
            1,
            "文案必须含累计值：{}",
            got[0].1
        );

        // ② **核心判据**：持续丢弃 ≥10 拍（累计丢弃数同步 +≥10）⇒ 累计仍恰 1 条
        let points_before = points.load(Ordering::SeqCst);
        wait_ticks(&reads, 10).await;
        assert!(
            points.load(Ordering::SeqCst) - points_before >= 10,
            "持续窗口内累计丢弃必须确实在涨（+{} 点）—— 否则「只有 1 条」可能只是因为没再丢",
            points.load(Ordering::SeqCst) - points_before
        );
        assert_eq!(
            snapshot().len(),
            1,
            "持续丢弃 10 拍 ⇒ 累计仍恰 1 条（每周期各报一条 = 告警风暴，R-11.5-A4 禁止），实得 {:?}",
            snapshot()
        );

        // ③ 恢复 ⇒ **只记日志、不发告警**（钉住本批裁定那一侧）
        dropping.store(false, Ordering::SeqCst);
        wait_ticks(&reads, 3).await;
        assert_eq!(
            snapshot().len(),
            1,
            "恢复不发新告警（只记 info 日志 + 可观测计数回落），实得 {:?}",
            snapshot()
        );

        // ④ 恢复后再次进入 ⇒ 再 1 条（边沿可重现，两段共 2 条）
        dropping.store(true, Ordering::SeqCst);
        assert!(
            wait_until(|| snapshot().len() >= 2).await,
            "恢复后再次进入必须再投一条"
        );
        let got = snapshot();
        assert_eq!(got.len(), 2, "两段丢弃事件 ⇒ 恰好 2 条告警，实得 {got:?}");
        assert_eq!(got[1].0, "major");
        assert!(
            got[1].1.contains("本周期新增 1 点"),
            "第 2 条报的是**本段进入那拍**的增量：{}",
            got[1].1
        );
        assert!(
            parse_total_points(&got[1].1) > 1,
            "第 2 条的累计值必须含此前各段的量（不是从 1 重新数）：{}",
            got[1].1
        );

        // 协作收工：收到停机信号即有上限地退出（本任务无数据要落盘）
        stop_tx.send(true).unwrap();
        let joined = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(joined.is_ok(), "收到停机信号后必须有上限地收工");
    }

    /// **缺口 1 接线网（源文本静态断言）**：装配点必须真的起本任务、**登记进协作退出名单**，
    /// 且读的是 `storage` 的既有计数面、投的是 `major` 级的既有 `AlertFeed`。
    ///
    /// 同 `startup.rs` 里 `telemetry_buffer_timer_and_shutdown_flush_are_wired` 的手法与理由
    /// （装配期起不来真环境；这里要证的恰恰是"装配源码里这几件事还在"）。
    ///
    /// 改什么会让本条红：删掉 `spawn_storage_health_timer(`（任务不再起）、删掉标签字面量
    /// `"storage_health_timer"`（不再带标签登记）、或删掉 `startup.rs` 里对
    /// `dropped_points/dropped_batches` 的读取。
    ///
    /// ⚠️ **"把它塞进 `guard.0`（abort 名单）"不再列为红因（评审 W-8）**：该变异
    /// **不可编译** —— `guard` 是 `TaskGuard(Vec<JoinHandle<()>>)`、`producers` 是
    /// `ProducerGuard(Vec<(&'static str, JoinHandle<()>)>)`（`startup.rs:1127` / `:1140`）
    /// ⇒ 带标签的元组塞不进 `guard.0`（实测 `E0308`）。**该属性由类型系统保证**，
    /// 故原先那条"标签之前不得夹 `guard.0.push(`"的静态断言**不可达、无判别力**，已删除
    /// （详见函数体内注）。
    #[test]
    fn startup_wires_the_health_timer_into_the_cooperative_list() {
        let src = include_str!("startup.rs").replace("\r\n", "\n");
        let (production, _) = src
            .split_once("\n#[cfg(test)]\nmod tests {")
            .expect("`#[cfg(test)] mod tests` 标记必须存在（分段锚点）");
        assert!(
            production.contains("spawn_storage_health_timer("),
            "装配点必须起健康巡检任务"
        );
        assert!(
            production.contains("\"storage_health_timer\""),
            "必须带标签登记（否则退出期日志无法指认是谁）"
        );
        // ── **已删除：原先那条"标签之前不得夹 `guard.0.push(`"的静态断言（评审 W-8）** ──
        //
        // 它**不可达、无判别力**：`guard` = `TaskGuard(Vec<JoinHandle<()>>)`、
        // `producers` = `ProducerGuard(Vec<(&'static str, JoinHandle<()>)>)`
        // （`startup.rs:1127` / `:1140`）⇒ **把带标签的元组塞进 `guard.0` 是类型错误**，
        // 该变异**不可编译**（探针实测 `E0308`），故"混进 abort 名单"这一形态在本仓
        // **根本构造不出来** —— 该属性由**类型系统**保证，不需要（也不可能有）一条有牙的
        // 源文本断言。要证的自始至终只是"**登记还在**"：由本文件上方两条
        // （`spawn_storage_health_timer(` + 标签字面量）与下面的 `AlertFeed` 同一实例断言覆盖。
        //
        // 另：`producers.0.push((&'static str, JoinHandle<()>))` 是**唯一**能接收该元组的
        // 名单 ⇒ 标签所在的这次登记**只能是**它（`guard.0` 装不下）。原实现里那条
        // `rfind("producers.0.push(")` 的 `expect` 因此也只是同义反复，一并删除。
        assert!(
            production.contains("alert_feed.clone(),") || production.contains("alert_feed.clone()"),
            "巡检必须拿到同一个 `AlertFeed` 实例（新造一个环 = 无人订阅、告警凭空消失）"
        );
    }
}
