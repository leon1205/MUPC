//! 口级采集调度器（S3a Task 5；§10.2 口调度预算 / §10.7 站超时隔离）。
//!
//! 架构：**每 port 一条采集 task**（口间并发），口内多从站按到期（`next_due`）**串行**
//! 轮询（口单 poller 天然串行；Rs485PortBus 内另有 per-port async Mutex 双保险，Task 3）。
//! 站失败（任一寄存器块读 Err / mapper 语义 Failed）→ 站级 offline 隔离，不阻断同口其它站。
//!
//! 结果按 role 分发到 [`StationSink`]（core-bin 实现，Task 7；southd 不依赖
//! strategy/ai-integration，只定义 trait 边界）：
//! - `MeterGrid` → [`StationSink::on_grid_package`]（策略 phase 唯一写方，含分相）；
//! - 一切非 grid 站 → [`StationSink::on_station_telemetry`]（telemetry 落库 + 状态事件）。
//!   单写方口径：同一站数据绝不走两个通道（battery soc 经 telemetry 全量落库，暂不单独
//!   推 pkg.battery.soc——SOC 融合留 S3b §2.11）。
//!
//! 观测契约：每口 task 由其采集循环常驻，任一口 task 内 panic 会**静默终止**该口采集
//! （无自动重 spawn）。调用方必须持有并观测 [`SouthScheduler::spawn`] 返回的每个
//! `JoinHandle`（详见其文档；core-bin Task 7 接线时落实）。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;

use crate::config::{Role, SouthStationsConfig, StationConf};
use crate::mapper::{self, BlockReads, PollResult};
use crate::port_runtime::StationBus;
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
}

/// 本轮应采的一站。`station_index` = 调度 state Vec 全局下标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StationPoll {
    pub station_index: usize,
}

/// 角色优先级（口调度预算 §10.2：grid/battery 关键量优先于 hvac/fire；慢站降频不拖累关键站 cadence）。
fn role_priority(r: Role) -> u8 {
    match r {
        Role::MeterGrid | Role::Battery => 0,
        Role::MeterBatt => 1,
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
}

/// 口运行时：bus（open 失败 → None，该口全站 offline）+ 本口独立 DueCalc。
struct PortRunner {
    bus: Option<Arc<dyn StationBus>>,
    calc: std::sync::Mutex<DueCalc>,
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

    /// 跑一口的一个 tick（now_ms 驱动 due → 逐到期站 poll）。
    async fn run_port_round(&self, port_i: usize, now_ms: u64) {
        let runner = &self.runners[port_i];
        let due = runner.calc.lock().unwrap().due_round(now_ms);
        if due.is_empty() {
            return;
        }
        for poll in due {
            self.poll_station(poll.station_index, runner.bus.clone()).await;
        }
    }

    /// 单站一轮采集：逐 regs 块读 → mapper 判定 → 分发 + 调度态更新。
    ///
    /// 任一块读 Err（物理层）或 mapper `PollResult::Failed`（语义层）→ 站失败（offline 记账 +
    /// 事件；§10.7 对两层失败隔离语义一致——该站本轮无有效数据）。全块 Ok 且 mapper Data
    /// → 站成功（恢复事件 + 按 role 分发）。同口串行由口 task 单 poller 保证（本方法不并发）。
    async fn poll_station(&self, station_index: usize, bus: Option<Arc<dyn StationBus>>) {
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
                match b.read_holding(slave, blk.addr, blk.count).await {
                    Ok(reg) => reads.push((blk.clone(), Ok(reg))),
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
            return;
        }

        // 全块读 Ok → mapper 语义判定（PollResult::Failed = 语义层失败，同 offline 处理）。
        match mapper::poll_to_result(role, &reads) {
            PollResult::Failed(msg) => self.handle_failure(station_index, &msg).await,
            PollResult::Data(pkg) => {
                self.mark_success(station_index).await;
                if role == Role::MeterGrid {
                    self.sink.on_grid_package(pkg).await;
                } else {
                    // 非 grid（battery/hvac/fire/meter_batt）的 DataPackage 载荷本 S3a 不消费
                    // ——pkg 在此仅用于 match 到 Data 分支确认站「活着」；battery SOC 融合留
                    // S3b §2.11（模块头单写方口径）。`pkg` 被 grid 分支消费故无 unused 告警，
                    // 此处不再引用。遥测值以 telemetry_points(&reads) 二次 decode 落库。
                    let pts: Vec<(String, f64, bool)> = mapper::telemetry_points(&reads)
                        .into_iter()
                        .map(|(m, v)| (m, v, false))
                        .collect();
                    if !pts.is_empty() {
                        self.sink.on_station_telemetry(&station_id, role, pts).await;
                    }
                }
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
            self.sink
                .on_station_telemetry(&id, role, vec![("offline".to_string(), 1.0, true)])
                .await;
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
    use crate::config::RegBlockConf;
    use crate::port_runtime::MockBus;
    use mupc_data_processing::meter_regs::RegFormat;

    // ---------- 测试构件 ----------

    fn blk(name: &str, addr: u16, count: u16) -> RegBlockConf {
        RegBlockConf {
            name: name.into(),
            addr,
            format: RegFormat::Float32,
            scale: 0.0,
            count,
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

    fn hvac_conf(id: &str, port: &str, slave: u8, interval_ms: u64) -> StationConf {
        StationConf {
            id: id.into(),
            role: Role::Hvac,
            port: port.into(),
            protocol: "modbus".into(),
            slave,
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

    /// 假 Sink：记录 on_grid_package / on_station_telemetry（事件与普通遥测以 is_event 区分）。
    #[derive(Default)]
    struct FakeSink {
        grid_pkgs: std::sync::Mutex<Vec<mupc_data_processing::DataPackage>>,
        msgs: std::sync::Mutex<Vec<(String, Role, Vec<(String, f64, bool)>)>>,
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
        assert_eq!(sink.telemetry_of("hvac"), vec![("temp".to_string(), 23.5)]);
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
        assert_eq!(sink.telemetry_of("hvac"), vec![("temp".to_string(), 23.5)]);
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
        assert_eq!(sink.telemetry_of("hvac"), vec![("temp".to_string(), 23.5)]);
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

    /// offline 事件 stale_timeout_s 窗口防刷屏：持续失败多轮只告警一次（offline_count 仍逐轮累加）。
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
        // offline_count 仍逐轮累加（调度态独立于事件去抖）
        {
            let st = sched.state.read().unwrap();
            assert_eq!(st[0].offline_count, 4);
        }
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
