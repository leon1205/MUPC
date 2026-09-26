//! 遥测缓冲丢弃的**健康巡检** —— 03 设计 §9.3 **缺口 1 / 缺口 2**（PRD R-11.5-A3② / A4，
//! 用例 **FLS-03②**）。
//!
//! # 为什么在 core-bin 而不是 `storage`（依赖方向的硬理由）
//!
//! 「丢弃必须产一条 `major` 级告警/事件」（S-3②）**不能**由 `storage` 自己满足：它没有、
//! 也**不应**有告警/事件通道 —— `AlertFeed` 在 core-bin，让 `storage` 依赖 core-bin 是倒向边。
//! ⇒ 落点只能是装配层：本任务**读** `WriteBuffer` 的既有观测面
//! （`dropped_points()` / `dropped_batches()`，`storage` 侧早已实现并单测），增量 > 0 时
//! 投一条 `major` 告警。
//!
//! # 为什么必须读**增量**（这一条同时闭合缺口 2）
//!
//! 两个计数器都是**只增不减的累计值**（`AtomicU64`）。若每拍无条件上报累计值，则
//! 「连续失败 10 个周期」会产 **10 条**完全相同的告警（告警风暴，PRD R-11.5-A4 禁止）。
//! 本任务持有**上一拍的读数**，只在**增量 > 0** 时投一条 ⇒ 告警天然按周期聚合：
//!
//! - 落库持续失败但**没有丢点**（点都回填了）⇒ 增量为 0 ⇒ **0 条告警**（这正是 R-11.5-A4
//!   要的那个"不风暴"）；
//! - 真的发生了"丢最旧"⇒ 增量 > 0 ⇒ 该周期**恰好 1 条**（文案内带增量与累计两个数字，
//!   现场既能看出"本周期又丢了多少"，也能看出"累计丢了多少"）。
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

/// 丢弃计数的**跨周期视图**（上一拍的读数 + 差分判定）。
///
/// 单拎成一个纯结构是刻意的（同 `stop_producers` / `console_write_paths` 的手法）：巡检的
/// **判据**在这里、"什么时候读""读到谁"在任务里 ⇒ 判据可以脱离真实时钟与真实 `WriteBuffer`
/// 被确定性单测（本模块 `tests` 两组用例）。
pub struct StorageHealthWatch {
    last_points: u64,
    last_batches: u64,
}

impl Default for StorageHealthWatch {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageHealthWatch {
    pub fn new() -> Self {
        Self {
            last_points: 0,
            last_batches: 0,
        }
    }

    /// 用**当前累计值**推进一拍。
    ///
    /// 返回 `Some(文案)` = 本周期应投**恰好一条**告警；`None` = 增量全为 0 ⇒ **不投**
    /// （这是缺口 2 的全部实现：无增量即静默，连续失败时段自然不会产告警风暴）。
    ///
    /// 文案**必须**同时含**增量条数**与**累计值**（设计原文）；`d_batches`（本周期发生
    /// "丢最旧"的次数）与累计次数一并带上，便于现场区分"一次丢一大批"与"多次各丢一点"。
    pub fn poll(&mut self, dropped_points: u64, dropped_batches: u64) -> Option<String> {
        // `saturating_sub`：计数器只增不减（`AtomicU64`），正常情况下不会倒退；万一读数倒退
        // （不可能：进程内单实例递增），按 0 处理 —— 宁可漏报一拍，也不伪造一个巨大的增量。
        let d_points = dropped_points.saturating_sub(self.last_points);
        let d_batches = dropped_batches.saturating_sub(self.last_batches);
        self.last_points = dropped_points;
        self.last_batches = dropped_batches;
        if d_points == 0 && d_batches == 0 {
            return None;
        }
        Some(format!(
            "遥测缓冲丢弃：本周期新增 {d_points} 点（{d_batches} 次超限丢弃），\
             累计 {dropped_points} 点（{dropped_batches} 次）"
        ))
    }
}

/// 巡检循环（**可注入读源与投递口** ⇒ 可确定性单测；生产接线见
/// [`spawn_storage_health_timer`]）。
///
/// - `read`：读当前**累计**丢弃计数（生产 = `WriteBuffer::dropped_points/dropped_batches`）；
/// - `emit(level, message)`：投递一条告警（生产 = `AlertFeed::push_system_alert`）；
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
        if let Some(message) = watch.poll(points, batches) {
            emit(DROP_ALERT_LEVEL, &message);
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
    use std::sync::atomic::{AtomicU64, Ordering};
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

    /// 级别常量就是 `"major"`（**定义式断言**，同 `alert_feed.rs` 的
    /// `minimal_form_capacity_is_64` 的强度口径：它守的是"没人把常量改掉"，即契约字面；
    /// 运行期行为由 `health_loop_emits_exactly_one_major_alert_per_changed_cycle` 用**字面量**
    /// 断言覆盖）。改这里的 `"major"` 必须同改 03 设计 §9.3 缺口 1 / PRD R-11.5-A3②。
    #[test]
    fn drop_alert_level_is_major() {
        assert_eq!(DROP_ALERT_LEVEL, "major");
    }

    /// **缺口 1 判据 ③（纯判据层）**：**无增量 ⇒ 不投**（不得每周期空发）。
    ///
    /// 改什么会让本条红：`poll` 去掉 `d_points == 0 && d_batches == 0` 的早退
    /// （改成无条件 `Some(..)`）。
    #[test]
    fn drop_watch_is_silent_when_there_is_no_increment() {
        let mut w = StorageHealthWatch::new();
        assert!(w.poll(0, 0).is_none(), "冷启动读数全 0 ⇒ 不得投告警");
        assert!(w.poll(0, 0).is_none(), "连续空周期 ⇒ 每次都不得投");

        let first = w.poll(7, 2).expect("首次出现丢弃 ⇒ 必须投一条");
        assert!(first.contains("本周期新增 7 点"), "文案含增量条数：{first}");
        assert!(first.contains("累计 7 点"), "文案含累计值：{first}");
        assert!(
            w.poll(7, 2).is_none(),
            "读数未变 ⇒ 不得重复投（缺口 2 聚合）"
        );

        // 只涨"丢弃次数"不涨"丢弃点数"（一句日志里丢 0 点但有 1 次超限）也算增量 ⇒ 必须投。
        let only_batches = w
            .poll(7, 3)
            .expect("批次计数增长也是增量 ⇒ 必须投（两个计数器任一增长都算）");
        assert!(
            only_batches.contains("本周期新增 0 点") && only_batches.contains("1 次超限丢弃"),
            "文案必须如实带两个增量：{only_batches}"
        );
        assert!(w.poll(7, 3).is_none(), "再次无增量 ⇒ 不得重复投");
    }

    /// **缺口 1 判据 ①②（任务层、真实时钟 + 真实任务，读源/投递口可注入）**：
    /// 增量 > 0 ⇒ 该周期**恰好 1 条**（含两个数字）；连续多个"有增量"的周期
    /// ⇒ **每周期 1 条**（不是 1 条/全部，也不是每周期多条）；无增量的周期**一条不发**。
    ///
    /// 判据强度靠"静默窗口内**至少又跑了 4 拍**"（`reads` 计数）来保证 —— 否则"没投"也可能
    /// 只是"任务已经死了"，那是假绿。
    ///
    /// 改什么会让本条红：把 `poll` 改成无条件投（静默窗口内会多出若干条）；
    /// 或把差分成"与首次读数比"（第二次变更的增量会算错）。
    #[tokio::test]
    async fn health_loop_emits_exactly_one_major_alert_per_changed_cycle() {
        let points = Arc::new(AtomicU64::new(0));
        let batches = Arc::new(AtomicU64::new(0));
        // 巡检实际跑过的拍数（证"静默窗口内任务确实在跑"，排除假绿）
        let reads = Arc::new(AtomicU64::new(0));
        let seen: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));

        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        let p = points.clone();
        let b = batches.clone();
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
                    (p.load(Ordering::SeqCst), b.load(Ordering::SeqCst))
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

        // ① 增量 > 0 ⇒ 恰好 1 条，且含增量与累计两个数字
        points.store(3, Ordering::SeqCst);
        batches.store(1, Ordering::SeqCst);
        assert!(
            wait_until(|| !snapshot().is_empty()).await,
            "出现丢弃后必须投出告警（否则现场对丢数完全无感）"
        );
        let got = snapshot();
        assert_eq!(got.len(), 1, "一个周期只投一条，实得 {got:?}");
        // 断言**字面量**（不引常量）：否则"常量被改掉"这条恒绿 —— 级别是 PRD/设计的硬要求
        assert_eq!(got[0].0, "major", "级别必须是 major（PRD R-11.5-A3②）");
        assert!(
            got[0].1.contains("本周期新增 3 点"),
            "文案必须含增量条数：{}",
            got[0].1
        );
        assert!(
            got[0].1.contains("累计 3 点"),
            "文案必须含累计值：{}",
            got[0].1
        );

        // ③ 增量 = 0 ⇒ 一条不发（判据强度：先等到任务**真的又跑了 ≥4 拍**再断言 ——
        //    否则"没投"也可能只是任务已死/被饿死 = 假绿）。等拍数而非 `sleep` 固定时长：
        //    测试跑在 `-j 2` 的并行 harness 里，固定 sleep 在负载下只跑得到 2–3 拍（实测假红）。
        let reads_before = reads.load(Ordering::SeqCst);
        assert!(
            wait_until(|| reads.load(Ordering::SeqCst) - reads_before >= 4).await,
            "静默窗口内必须真的又巡检了 ≥4 拍（上限 3 s；实得 {} 拍）",
            reads.load(Ordering::SeqCst) - reads_before
        );
        assert_eq!(
            snapshot().len(),
            1,
            "读数未变 ⇒ 每个周期都不得投（这正是缺口 2 要的聚合，实得 {:?}）",
            snapshot()
        );

        // ② 连续多个"有增量"的周期 ⇒ 每周期恰好 1 条（总数 = 变更次数，不是 0、不是翻倍）
        points.store(5, Ordering::SeqCst);
        batches.store(2, Ordering::SeqCst);
        assert!(
            wait_until(|| snapshot().len() >= 2).await,
            "第二次丢弃（+2 点）必须再投一条"
        );
        points.store(9, Ordering::SeqCst);
        batches.store(3, Ordering::SeqCst);
        assert!(
            wait_until(|| snapshot().len() >= 3).await,
            "第三次丢弃（+4 点）必须再投一条"
        );
        // 三次变更之后安静下来：**再跑 ≥3 拍**仍不得冒第 4 条
        // （同 ③：等拍数、不用固定 `sleep` —— 并行 harness 下 sleep 不可靠）
        let reads_before = reads.load(Ordering::SeqCst);
        assert!(
            wait_until(|| reads.load(Ordering::SeqCst) - reads_before >= 3).await,
            "静默窗口内必须真的又巡检了 ≥3 拍（上限 3 s；实得 {} 拍）",
            reads.load(Ordering::SeqCst) - reads_before
        );
        let got = snapshot();
        assert_eq!(
            got.len(),
            3,
            "3 次变更 ⇒ 恰好 3 条（每周期 1 条、无空发、无重复），实得 {got:?}"
        );
        assert_eq!(got[1].0, "major");
        assert!(
            got[1].1.contains("本周期新增 2 点") && got[1].1.contains("累计 5 点"),
            "第 2 条必须报**本周期增量**与**累计**两个数：{}",
            got[1].1
        );
        assert_eq!(got[2].0, "major");
        assert!(
            got[2].1.contains("本周期新增 4 点") && got[2].1.contains("累计 9 点"),
            "第 3 条同理：{}",
            got[2].1
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
