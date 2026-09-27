//! **服务级健康巡检** —— 07 PRD §4.2.1 / 07 设计 §4.7（U-164 裁定「服务级健康要真的实现」）。
//!
//! # 三处边界（读代码前先读这里）
//!
//! 1. **进程级不在这里**。`mupcd` / `mupc-local-display` 的存活与重启由 **systemd** 承担
//!    （`deploy/systemd/` 的 `Restart` / `RestartSec` / `StartLimitBurst` / `MemoryMax`）。
//!    本模块**不**做、也**不应**做进程存活检测 —— `system-monitor` 在 `mupcd` **内部**，
//!    监控/重启 `mupcd` 自己会随之一同消失（U-164 的改判根因）。
//!
//! 2. **判据 = 「子系统挂了」，不是「外部设备/对端不在线」**。这个区分是本模块的**核心**，
//!    弄错会造出比"不实现"更坏的形态（**恒告警**）：
//!
//!    | 看起来像"不健康"的读数 | 为什么**不能**当 `Failed` |
//!    |---|---|
//!    | `Iec104Server::connection_count() == 0` | **主站没连是常态**（现场未必总在线） |
//!    | display 通道未连接 | **屏可以关**，关了不该报服务故障 |
//!    | `LatestValues::station_is_active(..) == false` | 反映**现场设备**离线 —— 是**外部事件**（已有独立告警通道），不是 `mupcd` 子系统故障 |
//!
//!    ⇒ 只采信**真正表示"进程内这个子系统不转了"**的信号（见 [`unhealthy_services`]）。
//!
//! 3. **`Stopped` 不是 `Failed`**。`security` / `ai_engine` / `ota_update` / `wireless`
//!    注册时状态即 `Stopped`（2026-09-09 平台目标调整：国密只留框架、AI 引擎停用），
//!    而健康判据**只把 `Failed` 计为不健康** ⇒ `Stopped` **天然不产生告警**。
//!    **不得**把 `Stopped` 当 `Failed` —— 那会让这四个服务**恒告警**。
//!
//! # 告警口径 = **边沿触发**（与 `storage_health.rs` 同款，理由同）
//!
//! `AlertFeed::push_system_alert(level, ..)` 是**只有入、没有 ack/清除面**的投递环 ⇒
//! 「已恢复」若也投一条 `major`，对按级过滤的消费者**与"又发生一次"无法区分**。
//!
//! | 本拍 | 上一拍 | 动作 |
//! |---|---|---|
//! | 不健康 | 健康（尚未进入） | **进入边沿** ⇒ 投**恰好 1 条**（含服务名与判据） |
//! | 不健康 | 不健康 | **持续态** ⇒ **一条都不投** |
//! | 健康 | 不健康 | **退出边沿** ⇒ **只记一行 `info` 日志**；**不发告警** |
//! | 健康 | 健康 | 静默 |
//!
//! ⇒ 某服务连续 10 个巡检周期不健康 = **恰好 1 条**告警。
//!
//! # 状态写者唯一
//!
//! 本任务是 `ServiceCoordinator` 服务状态的**唯一运行期写者**（`update_service_status`），
//! 覆盖装配期 `register_service` 登记的初值。
//!
//! # 退出契约
//!
//! 句柄入 **`cooperative_tasks`**（协作退出名单），与 `storage_health_timer` / `flush_timer` /
//! `grid_agg_timer` **同名单**；**不得**放 `background_tasks`（abort 名单）—— 协作名单会 join
//! 并确认收工，abort 名单只发请求不等确认。

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use mupc_core::service_coord::ServiceStatus;
use mupc_core::service_coord_impl::ServiceCoordinatorImpl;

use crate::alert_feed::AlertFeed;

/// 巡检周期。
///
/// 取 **15 s**：07 PRD §4.2.1 原文即写「系统守护进程每 **15 秒**检查」，这是**需求数值**。
/// （与 `storage_health.rs` 的 1 s **刻意不同** —— 那是 03 号的需求数值，两者各自对齐自己的 PRD。）
pub const SERVICE_HEALTH_TICK_MS: u64 = 15_000;

/// 服务不健康的告警级别。落到 `AlertFeed::push_system_alert(level, ..)` 的 `level` 字段
/// ⇒ 订阅侧 `FeedItem.subtype == "major"`。
pub const SERVICE_UNHEALTHY_LEVEL: &str = "major";

/// 本拍各探针的**取值**（纯数据，**不含 `Arc`** ⇒ 判据可纯函数化、可单测）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HealthInputs {
    /// `south_pcs.enabled` —— 未启用时 `pcs` 服务不存在，**不判定**。
    pub pcs_enabled: bool,
    /// `pcs` 采集 task 已结束（`TaskWatch::finished`）。
    pub pcs_collect_finished: bool,
    /// `pcs` 采集 task panic（`TaskWatch::panicked`）。
    pub pcs_collect_panicked: bool,
    /// `storage` 写闸门处于降级（`WriteGate::is_degraded`）—— DB 完整性失败或磁盘水位触发停写。
    pub storage_degraded: bool,
}

/// **判据（纯函数）**：本拍应处于 `Failed` 的服务及其判据文案。
///
/// 只列**有可靠失败信号**的服务；其余服务**保持注册时的状态**（见 [`NOT_PROBED`]）。
///
/// **改什么会让 `unhealthy_services_*` 系列的断言红**：把某个 `&&` 的条件去掉
/// （如 `pcs_collect_panicked || pcs_collect_finished` 改成只判 `panicked`）⇒ 相应用例红。
pub fn unhealthy_services(i: &HealthInputs) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();

    // `pcs`：采集 task 结束（正常返回或 panic 都算"不再采集"）。
    // ⚠️ **不用** `PcsHandle::is_connected()` —— 那是**设备在线态**（PCS 断电/断线是现场事件，
    // 已有独立告警通道），不是"采集循环是否在跑"。B-9 把采集 task 经 `observe_task` 包装
    // （`startup.rs`），其 `TaskWatch` 修好补上这个信号，注释原文即写"保留…供后续
    // supervisor/健康面上报"。
    if i.pcs_enabled && (i.pcs_collect_finished || i.pcs_collect_panicked) {
        out.push((
            "pcs",
            if i.pcs_collect_panicked {
                "PCS 采集 task 已 panic（PCS 从此不再采集；联锁 last_run_state 随之失去数据源）"
                    .to_string()
            } else {
                "PCS 采集 task 已结束（PCS 从此不再采集）".to_string()
            },
        ));
    }

    // `storage`：写闸门降级 ⇒ 时序数据停写。
    if i.storage_degraded {
        out.push((
            "storage",
            "存储已降级：DB 完整性失败或磁盘水位触发停写（见 WriteGate）".to_string(),
        ));
    }

    out
}

/// **本模块不设探针的服务**（记录用，防"以为是漏了"）。
///
/// 每条都给出**为什么没有可靠失败信号**；将来若要补，须先找到"子系统挂了"语义的真信号，
/// **不得**拿外部设备/对端的在线态顶替（见模块头第 2 条）。
pub const NOT_PROBED: &[(&str, &str)] = &[
    ("message_bus", "进程内广播通道，无独立失败面"),
    ("security", "`Stopped`（国密只留框架）；`Stopped` 不参与告警"),
    ("ai_engine", "`Stopped`（2026-09-09 起引擎停用）"),
    ("intercore", "核间 TCP 通道**生产路径无消费者**，无「可用/不可用」可言"),
    ("plugin_loader", "运行期加载 cdylib，其失败以插件加载错误呈现，无持续健康面"),
    ("data_processing", "无「子系统挂了」信号；`station_is_active` 反映**现场设备**离线（外部事件）"),
    ("strategy_engine", "进程内组件，无独立失败面"),
    ("hmi_backend", "display 通道未连是**常态**（屏可关），不能当服务故障"),
    ("gateway", "`connection_count()==0` 是**常态**（主站未连），不能当服务故障"),
    ("ota_update", "`Stopped`（未启用）"),
    ("system_monitor", "本巡检任务的宿主，自观测无意义"),
    ("wireless", "`Stopped`（硬件未到）"),
];

/// 巡检的**边沿信号**（纯数据；由 [`SvcHealthWatch::poll`] 产出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvcSignal {
    /// **进入边沿**：该服务本拍起不健康 ⇒ 投**恰好一条** `major` 告警。
    NewlyUnhealthy(&'static str, String),
    /// **退出边沿**：该服务已恢复 ⇒ **只记日志、不发告警**。
    Recovered(&'static str),
}

/// 服务级健康的**边沿状态机**（纯逻辑，可单测；生产循环只做"喂输入 + 投返回值"）。
#[derive(Debug, Default)]
pub struct SvcHealthWatch {
    /// 上一拍的**不健康服务集合**（进入/退出的判据）。
    prev_unhealthy: HashSet<&'static str>,
}

impl SvcHealthWatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入本拍的不健康服务集合，产出应执行的动作。
    pub fn poll(&mut self, unhealthy: &[(&'static str, String)]) -> Vec<SvcSignal> {
        let now: HashSet<&'static str> = unhealthy.iter().map(|(n, _)| *n).collect();
        let mut signals = Vec::new();

        // 进入边沿：本拍在、上一拍不在
        for (name, detail) in unhealthy {
            if !self.prev_unhealthy.contains(name) {
                signals.push(SvcSignal::NewlyUnhealthy(name, detail.clone()));
            }
        }
        // 退出边沿：上一拍在、本拍不在
        for name in &self.prev_unhealthy {
            if !now.contains(name) {
                signals.push(SvcSignal::Recovered(name));
            }
        }

        self.prev_unhealthy = now;
        signals
    }
}

/// 巡检任务持有的探针句柄（**全部可选**：未启用的子系统不注入）。
pub struct ServiceHealthProbes {
    /// `pcs` 采集 task 的观测（`south_pcs.enabled=false` ⇒ `None`）。
    pub pcs_collect_watch: Option<Arc<mupc_southd::task_watch::TaskWatch>>,
    /// 存储写闸门。
    pub write_gate: Arc<mupc_storage::WriteGate>,
}

impl ServiceHealthProbes {
    /// 读本拍取值（生产循环唯一碰句柄的地方）。
    pub fn read(&self) -> HealthInputs {
        HealthInputs {
            pcs_enabled: self.pcs_collect_watch.is_some(),
            pcs_collect_finished: self
                .pcs_collect_watch
                .as_ref()
                .is_some_and(|w| w.finished()),
            pcs_collect_panicked: self
                .pcs_collect_watch
                .as_ref()
                .is_some_and(|w| w.panicked()),
            storage_degraded: self.write_gate.is_degraded(),
        }
    }
}

/// 巡检循环（读源 / 投递口 / 状态回写均**可注入** ⇒ 任务层可测）。
///
/// 与 `storage_health.rs` 的 `run_health_loop` 同款：`MissedTickBehavior::Delay`（落后顺延、
/// 不追赶补打）+ 吞掉第一次立即返回的 `tick()`（避免启动瞬间多一次无意义巡检）。
pub async fn run_service_health_loop<P, E>(
    watch: &mut SvcHealthWatch,
    coord: &ServiceCoordinatorImpl,
    period: Duration,
    mut stop: tokio::sync::watch::Receiver<bool>,
    mut read: P,
    mut emit: E,
) where
    P: FnMut() -> HealthInputs,
    E: FnMut(&str, &str),
{
    let mut ticker = tokio::time::interval(period);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ticker.tick().await; // 吞掉立即返回的那一拍

    loop {
        tokio::select! {
            _ = stop.changed() => {
                tracing::debug!("停机信号：服务健康巡检收工（本任务不持有数据，无需收尾）");
                break;
            }
            _ = ticker.tick() => {}
        }

        let unhealthy = unhealthy_services(&read());

        // ① 状态回写（本任务是**唯一运行期写者**）：不健康 ⇒ `Failed`；已恢复 ⇒ `Running`
        for (name, _) in &unhealthy {
            coord.update_service_status(name, ServiceStatus::Failed);
        }
        for signal in watch.poll(&unhealthy) {
            match signal {
                SvcSignal::NewlyUnhealthy(name, detail) => {
                    coord.update_service_status(name, ServiceStatus::Failed);
                    emit(SERVICE_UNHEALTHY_LEVEL, &format!("服务不健康：{name} —— {detail}"));
                }
                SvcSignal::Recovered(name) => {
                    coord.update_service_status(name, ServiceStatus::Running);
                    // 退出边沿 ⇒ **只记日志**（理由见模块头「告警口径」）
                    tracing::info!(service = name, "服务已恢复（服务级巡检）");
                }
            }
        }
    }
}

/// 起服务级健康巡检任务（句柄须入 `cooperative_tasks` 协作退出名单，见模块头）。
pub fn spawn_service_health_timer(
    coord: Arc<ServiceCoordinatorImpl>,
    probes: ServiceHealthProbes,
    alert_feed: Arc<AlertFeed>,
    stop: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    // 一次性把"**哪些服务没设探针**"写进日志：现场看到某服务一直没告警时，能立刻分清
    // 是"它真的健康"还是"它根本没有探针"（这正是 [`NOT_PROBED`] 存在的意义）。
    let not_probed: Vec<&str> = NOT_PROBED.iter().map(|(n, _)| *n).collect();
    tracing::info!(
        tick_ms = SERVICE_HEALTH_TICK_MS,
        probed = ?["pcs", "storage"],
        ?not_probed,
        "服务级健康巡检启动（探针口径：只看\"子系统挂了\"，不看外部设备/对端在线态；\
         未设探针的服务的理由详见 crate::service_health::NOT_PROBED）"
    );

    tokio::spawn(async move {
        let mut watch = SvcHealthWatch::new();
        run_service_health_loop(
            &mut watch,
            &coord,
            Duration::from_millis(SERVICE_HEALTH_TICK_MS),
            stop,
            || probes.read(),
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
    // `service_status` 是 trait 方法（`ServiceCoordinatorImpl` 的固有方法里没有它）
    use mupc_core::service_coord::ServiceCoordinator;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn inputs(pcs_enabled: bool, finished: bool, panicked: bool, degraded: bool) -> HealthInputs {
        HealthInputs {
            pcs_enabled,
            pcs_collect_finished: finished,
            pcs_collect_panicked: panicked,
            storage_degraded: degraded,
        }
    }

    // ── 判据层（纯函数） ──

    /// **判据：健康 ⇒ 空**。改坏：让 `unhealthy_services` 无条件 push ⇒ 红。
    #[test]
    fn nothing_unhealthy_when_all_probes_healthy() {
        assert!(unhealthy_services(&inputs(true, false, false, false)).is_empty());
    }

    /// **判据：`pcs` 采集 task 结束（正常返回）⇒ 不健康**。
    ///
    /// 改坏：`pcs_collect_finished` 这一项从条件里去掉（只判 `panicked`）⇒ 本条红。
    #[test]
    fn pcs_collect_task_ending_is_unhealthy() {
        let got = unhealthy_services(&inputs(true, true, false, false));
        assert_eq!(got.len(), 1, "实得 {got:?}");
        assert_eq!(got[0].0, "pcs");
        assert!(got[0].1.contains("已结束"), "文案须写明判据：{}", got[0].1);
    }

    /// **判据：`pcs` 采集 task panic ⇒ 不健康**，且文案点明"panic"。
    #[test]
    fn pcs_collect_panic_is_unhealthy() {
        let got = unhealthy_services(&inputs(true, false, true, false));
        assert_eq!(got.len(), 1, "实得 {got:?}");
        assert!(got[0].1.contains("panic"), "文案须写明 panic：{}", got[0].1);
    }

    /// **判据：`south_pcs.enabled=false` ⇒ 不判 `pcs`**（未启用的子系统不该被报不健康）。
    ///
    /// 改坏：把 `i.pcs_enabled &&` 从条件里去掉 ⇒ 本条红。
    #[test]
    fn pcs_is_not_judged_when_disabled() {
        assert!(
            unhealthy_services(&inputs(false, true, true, false)).is_empty(),
            "未启用 PCS 时不得报 `pcs` 不健康"
        );
    }

    /// **判据：`storage` 降级 ⇒ 不健康**。
    #[test]
    fn storage_degraded_is_unhealthy() {
        let got = unhealthy_services(&inputs(true, false, false, true));
        assert_eq!(got.len(), 1, "实得 {got:?}");
        assert_eq!(got[0].0, "storage");
    }

    /// **判据：两个探针同时不健康 ⇒ 两条**（互不遮蔽）。
    #[test]
    fn both_probes_unhealthy_yields_two() {
        let got = unhealthy_services(&inputs(true, true, false, true));
        assert_eq!(got.len(), 2, "实得 {got:?}");
    }

    /// **硬约束回归网：`Stopped` 不是 `Failed`**。
    ///
    /// `security` / `ai_engine` / `ota_update` / `wireless` 注册时即 `Stopped`；
    /// 本模块的判据**只看"有没有可靠失败信号"**，不看注册状态 ⇒ 这四者**永远不进**不健康集合。
    ///
    /// 改坏：在 `unhealthy_services` 里加一条「`Stopped` 也算不健康」（例如读 `coord` 的
    /// 全量状态）⇒ 现场会**恒告警**四条。本用例通过"判据函数只接受 [`HealthInputs`]、拿不到
    /// 任何服务注册状态"这一**签名约束**把它钉住；`SERVICE_HEALTH_TICK_MS` 之外的
    /// 反向断言见 `not_probed_lists_the_four_stopped_services`。
    #[test]
    fn stopped_services_are_never_reported_unhealthy() {
        // 全健康输入 ⇒ 必然为空（判据里没有"注册状态"这一维，`Stopped` 无从进入）
        for enabled in [true, false] {
            for finished in [true, false] {
                for panicked in [true, false] {
                    for degraded in [true, false] {
                        let got = unhealthy_services(&inputs(enabled, finished, panicked, degraded));
                        for (name, _) in &got {
                            assert!(
                                *name == "pcs" || *name == "storage",
                                "判据只允许产出 pcs / storage；实得 {name} —— \
                                 若某停用服务出现在这里，现场会恒告警"
                            );
                        }
                    }
                }
            }
        }
    }

    /// **四个停用服务确实登记在 [`NOT_PROBED`] 里**（记录用；防"以为是漏了"）。
    #[test]
    fn not_probed_lists_the_four_stopped_services() {
        let names: Vec<&str> = NOT_PROBED.iter().map(|(n, _)| *n).collect();
        for n in ["security", "ai_engine", "ota_update", "wireless"] {
            assert!(names.contains(&n), "`{n}` 须在 NOT_PROBED 中并写明理由");
        }
        // `intercore` 也必须在内（生产路径无消费者）
        assert!(names.contains(&"intercore"));
    }

    // ── 边沿状态机（纯逻辑） ──

    /// **进入边沿 ⇒ 恰 1 条**；**持续态 ⇒ 0 条**。
    ///
    /// 改坏：`poll` 的进入判据改成"本拍不健康就报"（不看 `prev_unhealthy`）⇒ 持续那几拍各出 1 条 ⇒ 红。
    #[test]
    fn enter_edge_emits_exactly_once_then_silent() {
        let mut w = SvcHealthWatch::new();
        let u = [("pcs", "d".to_string())];

        let first = w.poll(&u);
        assert_eq!(first.len(), 1, "进入边沿须恰 1 条，实得 {first:?}");
        assert!(matches!(first[0], SvcSignal::NewlyUnhealthy("pcs", _)));

        // 其后 9 拍仍在持续不健康 ⇒ 一条都不投
        for tick in 2..=10 {
            assert!(
                w.poll(&u).is_empty(),
                "持续不健康第 {tick} 拍不得再投（每拍一条 = 告警风暴）"
            );
        }
    }

    /// **退出边沿 ⇒ `Recovered`（只记日志），不是 `NewlyUnhealthy`**。
    #[test]
    fn exit_edge_is_recovered_not_a_new_alert() {
        let mut w = SvcHealthWatch::new();
        let _ = w.poll(&[("pcs", "d".to_string())]);
        let got = w.poll(&[]);
        assert_eq!(got, vec![SvcSignal::Recovered("pcs")], "实得 {got:?}");
        // 恢复后静默
        assert!(w.poll(&[]).is_empty());
    }

    /// **边沿可重现**：恢复后再次不健康 ⇒ 再 1 条。
    #[test]
    fn edge_is_reproducible_after_recovery() {
        let mut w = SvcHealthWatch::new();
        let u = [("storage", "d".to_string())];
        assert_eq!(w.poll(&u).len(), 1);
        assert!(w.poll(&u).is_empty());
        assert_eq!(w.poll(&[]).len(), 1, "恢复边沿 1 条");
        assert_eq!(w.poll(&u).len(), 1, "再次进入 ⇒ 再 1 条");
    }

    /// **多服务互不遮蔽**：`pcs` 恢复不影响 `storage`（后者仍是持续态 ⇒ 不投）。
    #[test]
    fn services_are_tracked_independently() {
        let mut w = SvcHealthWatch::new();
        let both = [
            ("pcs", "a".to_string()),
            ("storage", "b".to_string()),
        ];
        assert_eq!(w.poll(&both).len(), 2, "两个新进入 ⇒ 2 条");

        // pcs 恢复、storage 仍在 ⇒ 恰 1 条 Recovered，且**不**为 storage 再投
        let got = w.poll(&[("storage", "b".to_string())]);
        assert_eq!(got, vec![SvcSignal::Recovered("pcs")], "实得 {got:?}");
    }

    // ── 任务层（真实时钟 + 可注入读源/投递口） ──

    /// **任务层：进入 ⇒ 恰 1 条；持续 ≥10 拍 ⇒ 累计仍恰 1 条；恢复 ⇒ 不投；再进入 ⇒ 再 1 条**。
    ///
    /// 读源用 `AtomicBool` 驱动（**等拍数**而非固定 `sleep` —— 本用例与其余数百条测试在同一
    /// harness 里抢 CPU，固定 sleep 推出的节拍数会随负载漂移、曾导致假红）。
    #[tokio::test]
    async fn loop_emits_one_alert_per_episode_and_writes_back_status() {
        let failing = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let seen: Arc<std::sync::Mutex<Vec<(String, String)>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

        let coord = Arc::new(ServiceCoordinatorImpl::new());
        coord.register_service("pcs", ServiceStatus::Running);
        // 停用服务同样注册（回归网：它不得被报不健康）
        coord.register_service("ai_engine", ServiceStatus::Stopped);

        let f = failing.clone();
        let r = reads.clone();
        let s = seen.clone();
        let c = coord.clone();
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(async move {
            let mut w = SvcHealthWatch::new();
            let stop = stop_rx;
            // 手工跑循环（不 spawn，便于在测试里直接 join）——与 `run_service_health_loop` 同形
            run_service_health_loop(
                &mut w,
                &c,
                Duration::from_millis(30),
                stop,
                || {
                    r.fetch_add(1, Ordering::SeqCst);
                    inputs(true, f.load(Ordering::SeqCst), false, false)
                },
                |level, message| {
                    s.lock().unwrap().push((level.to_string(), message.to_string()));
                },
            )
            .await;
        });

        let snapshot = || {
            let v = seen.lock().unwrap();
            v.iter().map(|(l, m)| (l.clone(), m.clone())).collect::<Vec<_>>()
        };
        async fn wait_ticks(reads: &std::sync::atomic::AtomicU64, n: u64) {
            let before = reads.load(Ordering::SeqCst);
            for _ in 0..300 {
                if reads.load(Ordering::SeqCst) - before >= n {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("窗口内必须真的又巡检了 ≥{n} 拍（上限 3 s）");
        }

        // 健康期 ⇒ 0 条
        wait_ticks(&reads, 3).await;
        assert!(snapshot().is_empty(), "全健康 ⇒ 0 条，实得 {:?}", snapshot());

        // 进入边沿 ⇒ 恰 1 条，级别字面量 major，文案含服务名
        failing.store(true, Ordering::SeqCst);
        for _ in 0..300 {
            if !snapshot().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let got = snapshot();
        assert_eq!(got.len(), 1, "进入边沿只投一条，实得 {got:?}");
        assert_eq!(got[0].0, "major", "级别必须是 major");
        assert!(got[0].1.contains("pcs"), "文案须点名服务：{}", got[0].1);

        // 状态回写：`pcs` 变 `Failed`
        assert_eq!(coord.service_status("pcs"), Some(ServiceStatus::Failed));
        // **停用服务不受影响**（回归网）
        assert_eq!(
            coord.service_status("ai_engine"),
            Some(ServiceStatus::Stopped),
            "停用服务不得被巡检改写，更不得告警"
        );

        // 持续 ≥10 拍 ⇒ 累计仍恰 1 条
        wait_ticks(&reads, 10).await;
        assert_eq!(
            snapshot().len(),
            1,
            "持续不健康 10 拍 ⇒ 累计恰 1 条，实得 {:?}",
            snapshot()
        );

        // 恢复 ⇒ 不投新告警，且状态回写为 Running
        failing.store(false, Ordering::SeqCst);
        wait_ticks(&reads, 3).await;
        assert_eq!(snapshot().len(), 1, "恢复不发新告警，实得 {:?}", snapshot());
        assert_eq!(coord.service_status("pcs"), Some(ServiceStatus::Running));

        // 再次进入 ⇒ 再 1 条
        failing.store(true, Ordering::SeqCst);
        for _ in 0..300 {
            if snapshot().len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(snapshot().len(), 2, "边沿可重现：两段共 2 条");

        // 协作收工
        stop_tx.send(true).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(2), handle).await.is_ok(),
            "收到停机信号后必须有上限地收工"
        );
    }

    /// **装配接线网（源文本静态断言）**：装配点必须真的起本任务、**登记进协作退出名单**、
    /// 并写入 `coord`。
    ///
    /// 同 `storage_health.rs` 的 `startup_wires_the_health_timer_into_the_cooperative_list`
    /// 的手法与理由（装配期起不来真环境；要证的恰恰是"装配源码里这几件事还在"）。
    #[test]
    fn startup_wires_the_service_health_timer_into_the_cooperative_list() {
        let src = include_str!("startup.rs").replace("\r\n", "\n");
        let (production, _) = src
            .split_once("\n#[cfg(test)]\nmod tests {")
            .expect("`#[cfg(test)] mod tests` 标记必须存在（分段锚点）");
        assert!(
            production.contains("spawn_service_health_timer("),
            "装配点必须起服务健康巡检任务"
        );
        assert!(
            production.contains("\"service_health_timer\""),
            "必须带标签登记（否则退出期日志无法指认是谁）"
        );
    }
}
