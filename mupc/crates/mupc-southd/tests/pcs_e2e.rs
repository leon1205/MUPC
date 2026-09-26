//! PCS 帧级 e2e（T-L0 的 E1–E6 搬迁 + E7，设计 §13.6 R-3）。
//!
//! 驱动方式：`Rs485Device`（rs485-plugin） + `#[cfg(any(test, feature = "test-seam"))]`
//! 同步交换缝（`set_test_exchange`） + PCS 从站仿真（`mupc_southd::pcs::sim`）。
//! 成帧、CRC、字节互换、异常语义全程在环。
//!
//! **与迁移前的差异（如实登记）**：原缝用 `tokio::io::DuplexStream`，可模拟**分片到达**；
//! 本缝是同步"请求原文 → 响应原文"，**不模拟分片**。故本文件覆盖字节契约与状态机，
//! 不覆盖分片时序（该项由 rs485-plugin 自己的帧级单测承担）。
//!
//! 对应关系（迁移前 `intercore/src/transport/modbus.rs` 的 `mod e2e_pcs_sim`）：
//! | 本文件 | 迁移前用例 | 覆盖 |
//! |---|---|---|
//! | E1 | `e1_link_probe_and_heartbeat_run_state` | 链路/在线态 + 停机运行态可读 |
//! | E2 | `e2_input_area_read_and_wire_byte_order` | 3 区读 + 字内字节互换 |
//! | E3 | `e3_holding_write_readback_signed` | 4 区写-回读 + 负值符号性 |
//! | E4 | `e4_start_stop_direction_state_machine` | 启停 + 功率方向状态机 |
//! | E5 | `e5_alarm_estop_bit_readable` | 告警字可置位且被采集读到 |
//! | E6 | `e6_concurrent_write_read_no_crosstalk` | 并发控制/采集不串线 |
//! | E7 | （新增，无对应） | 采集定时循环契约（Task 8 评审查出零覆盖） |
//!
//! **E7 的登记**：`PcsHandle::spawn_collection_loop` 此前**全仓零覆盖、且无 Task 认领**
//! （Task 8 质量评审查出）—— 本文件是它唯一的落点。
use async_trait::async_trait;
use mupc_southd::config::{Role, SouthPcsConfig};
use mupc_southd::pcs::regs::{from_pcs_reg, to_pcs_reg, REG_CONST_P_SET, REG_SOC, REG_START_STOP};
// `REG_ALARM_BASE`（3 区告警字基址）定义在从站仿真侧（`sim.rs`）而非 regs.rs。
use mupc_southd::pcs::sim::{PcsSimState, PcsSlaveService, REG_ALARM_BASE};
use mupc_southd::pcs::{PcsDualParam, PcsHandle};
use mupc_southd::port_runtime::{BusError, StationBus};
use mupc_southd::scheduler::StationSink;
use rs485_plugin::{handlers::ModbusHandler, Config, CrcMode, Parity, Rs485Device};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 帧级 bus：把 `StationBus` 的寄存器读写落到 `Rs485Device` 的 `_from` 家族
/// （后者经同步交换缝转入从站仿真）。**不调用 `open()`** —— 缝已短路真实 IO。
struct SeamBus {
    dev: Rs485Device,
    /// E7：FC04 采集读的**尝试**次数（失败拍也计 —— 不退避/不降频才可辨）。
    input_calls: Arc<AtomicUsize>,
    /// E7：置位后 FC04 一律回 `Err`（验证失败拍仍按原周期推进）。
    fail_input: AtomicBool,
}

impl SeamBus {
    /// 建从站仿真 + 交换缝。返回 (bus, 仿真状态) —— 状态供测试断言从站侧。
    fn new() -> (Arc<Self>, Arc<PcsSimState>) {
        let state = Arc::new(PcsSimState::new());
        let svc = PcsSlaveService::new(Arc::clone(&state));
        let dev = Rs485Device::new(
            "pcs_e2e".into(),
            "modbus".into(),
            Config {
                port: "SEAM-INPROCESS".into(), // 缝已设 ⇒ 不会被真正打开
                baud_rate: 19200,
                data_bits: 8,
                stop_bits: 1,
                parity: Parity::None,
                timeout_ms: 1000,
                device_addr: 1,
                crc_mode: CrcMode::Crc16Modbus,
                de_gpio: None,
                re_gpio: None,
            },
            Arc::new(ModbusHandler::new(1, CrcMode::Crc16Modbus)),
        );
        // 缝：请求帧原文 → 从站仿真求值 → 响应帧原文（同步，零 IO）。
        // 注意 `Arc`（Task 2 收口把缝类型定为 `Arc<dyn Fn>`，非 `Box`）。
        dev.set_test_exchange(Arc::new(move |req: &[u8]| svc.serve_frame_sync(req)));
        (
            Arc::new(SeamBus {
                dev,
                input_calls: Arc::new(AtomicUsize::new(0)),
                fail_input: AtomicBool::new(false),
            }),
            state,
        )
    }
}

#[async_trait]
impl StationBus for SeamBus {
    async fn read_holding(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError> {
        self.dev
            .read_holding_registers_from(slave, addr, count)
            .map_err(|e| BusError::Read {
                slave,
                addr,
                count,
                reason: e.to_string(),
            })
    }
    async fn read_input(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError> {
        // 计数先于失败判定：失败拍必须同样被数到（E7 判"不退避"的前提）。
        self.input_calls.fetch_add(1, Ordering::Relaxed);
        if self.fail_input.load(Ordering::Relaxed) {
            return Err(BusError::Read {
                slave,
                addr,
                count,
                reason: "E7 注入：读失败".to_string(),
            });
        }
        self.dev
            .read_input_registers_from(slave, addr, count)
            .map_err(|e| BusError::Read {
                slave,
                addr,
                count,
                reason: e.to_string(),
            })
    }
    async fn read_discrete(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<bool>, BusError> {
        self.dev
            .read_discrete_inputs_from(slave, addr, count)
            .map_err(|e| BusError::Read {
                slave,
                addr,
                count,
                reason: e.to_string(),
            })
    }
    async fn write_single(&self, slave: u8, addr: u16, value: u16) -> Result<(), BusError> {
        self.dev
            .write_single_register_from(slave, addr, value)
            .map_err(|e| BusError::Write {
                slave,
                addr,
                value,
                reason: e.to_string(),
            })
    }
}

/// 无副作用 sink（E1–E4/E6/E7 不关心投递）。
struct NullSink;
#[async_trait]
impl StationSink for NullSink {
    async fn on_grid_package(&self, _pkg: mupc_data_processing::DataPackage) {}
    async fn on_station_telemetry(&self, _id: &str, _role: Role, _pts: Vec<(String, f64, bool)>) {}
    async fn on_battery_soc(&self, _id: &str, _soc: f64) {}
}

/// 记录遥测批次的 sink（E5 用：证明告警字**被采集读到**而不只被裸读看到）。
#[derive(Default)]
struct RecSink {
    telemetry: std::sync::Mutex<Vec<Vec<(String, f64, bool)>>>,
}
#[async_trait]
impl StationSink for RecSink {
    async fn on_grid_package(&self, _pkg: mupc_data_processing::DataPackage) {}
    async fn on_station_telemetry(&self, _id: &str, _role: Role, pts: Vec<(String, f64, bool)>) {
        self.telemetry.lock().unwrap().push(pts);
    }
    async fn on_battery_soc(&self, _id: &str, _soc: f64) {}
}

/// 与 `tests/fixtures/south_pcs_s3b2.yaml` 的 `pcs_3zone` 同构（addr 1000 / count 76），
/// 直接复用生产 fixture（与 `config.rs` 的 `collection_tests::cfg_with_points` 同法）——
/// `points` 已锚定覆盖满窗口（规则 11），不会被 `SouthPcsConfig::validate` 拒。
fn pcs_cfg() -> SouthPcsConfig {
    serde_yaml::from_str(include_str!("fixtures/south_pcs_s3b2.yaml")).expect("fixture 必须可解析")
}

/// 造一帧从站请求 ADU 之外的"经真实栈"写：直接经 bus 写 4 区（绕过 `send_*` 的 M1 守卫，
/// 用于把从站**先**置于非停机态 —— 与迁移前 e2e 用 `locked_write(500=1)` 的取向一致）。
async fn write_run(bus: &Arc<SeamBus>) {
    bus.write_single(1, REG_START_STOP, to_pcs_reg(1.0))
        .await
        .expect("写 500=1 应成功");
}

/// E1 链路与在线态（对应迁移前 e1_link_probe_and_heartbeat_run_state）：
/// 一拍采集即建立在线态；3 区运行状态可读 —— 从站默认 500=0 ⇒ `RUN_STATE=0`（停机）。
#[tokio::test]
async fn e1_tick_once_brings_link_online_and_run_state_readable() {
    let (bus, _state) = SeamBus::new();
    let h = PcsHandle::new(pcs_cfg(), bus, Arc::new(NullSink));

    assert!(
        !h.is_connected().await,
        "前提：初值不在线（否则下面的在线断言恒真、零判别力）"
    );
    h.tick_once().await;
    assert!(h.is_connected().await, "成功一拍 ⇒ 在线");
    assert_eq!(
        h.last_run_state(),
        Some(0),
        "500 缺省 0 ⇒ RUN_STATE=0（停机）"
    );
}

/// E2 3 区读 + 字内字节互换（对应 e2_input_area_read_and_wire_byte_order）：
/// 从站恒 SOC=66% ⇒ 采集解出 66.0；**同一用例内另钉线值** = `to_pcs_reg(66.0)` = 0x4200。
///
/// ⚠️ **为什么必须同时钉线值**（零判别力陷阱）：66 = 0x0042 是个"巧合数"—— 若**收发两侧
/// 一起**漏掉 `swap_bytes`（全局一致的改动），解出的仍是 66 ⇒ 只断 `Some(66.0)` 抓不到它。
/// 线值断言（0x4200）把"协议线格式"本身钉死，两侧任一侧单独漏掉则解出 16896 → 域外 → `None`。
#[tokio::test]
async fn e2_input_area_read_and_wire_byte_order() {
    let (bus, _state) = SeamBus::new();
    let h = PcsHandle::new(pcs_cfg(), bus.clone(), Arc::new(NullSink));

    // 线值侧：66 = 0x0042 ⇒ 字内字节互换后线上应为 0x4200
    let wire = bus
        .read_input(1, REG_SOC, 1)
        .await
        .expect("FC04 读 SOC 应经缝成功")[0];
    assert_eq!(wire, to_pcs_reg(66.0), "线值须为实值的字节互换");
    assert_eq!(wire, 0x4200, "0x0042 互换 ⇒ 0x4200");
    assert_eq!(from_pcs_reg(wire), 66.0, "回解须还原实值 66");

    // 解码侧：采集快照解出的 SOC 必须是 66
    h.tick_once().await;
    assert_eq!(h.latest_soc().await.map(|(v, _)| v), Some(66.0));
}

/// E3 4 区写-回读 + 双向字节互换（对应 e3_holding_write_readback_signed）：
/// 经**控制面** `send_dual_param` 写恒功率 P = −12.0 ⇒ 从站镜像（回解后）须原样是 −12.0；
/// 再 FC03 回读线值 = `to_pcs_reg(−12.0)` = 0xF4FF、回解仍 −12。负值钉死符号位不坏。
#[tokio::test]
async fn e3_holding_write_readback_signed() {
    let (bus, state) = SeamBus::new();
    let h = PcsHandle::new(pcs_cfg(), bus.clone(), Arc::new(NullSink));
    write_run(&bus).await; // 前置：S-4 守卫要求先处于非停机态

    h.send_dual_param(&PcsDualParam::new(-12.0, 0.0, true, "fallback"))
        .await
        .expect("下发应成功");

    let hold = state.hold.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(
        hold.get(&REG_CONST_P_SET).copied(),
        Some(-12.0),
        "−12.0 必须原样落从站镜像（写侧 encode + 从站侧 from_pcs_reg 双向互换都对）"
    );
    drop(hold);

    let wire = bus
        .read_holding(1, REG_CONST_P_SET, 1)
        .await
        .expect("FC03 回读应成功")[0];
    assert_eq!(wire, to_pcs_reg(-12.0), "回读线值须为字节互换");
    assert_eq!(wire, 0xF4FF, "−12 = 0xFFF4 ⇒ 互换 0xF4FF");
    assert_eq!(from_pcs_reg(wire), -12.0, "回读回解须还原 −12");
}

/// E4 启停 + 功率方向推演运行状态机（对应 e4_start_stop_direction_state_machine）：
/// 500=1 ⇒ 待机(1)；P>0 ⇒ 放电(3)；P<0 ⇒ 充电(2)；`stop()`（写 500=0）⇒ 停机(0)。
/// **状态经 `tick_once` 刷新快照后才由 `last_run_state()` 读出**（快照口径，非现读）。
///
/// **启动走 M1 人工授权路径**（`authorize_restart` → `send_dual_param`）而非直接写 500=1：
/// 从站起点 500=0 ⇒ `ensure_started` 的 S-4 守卫会拒绝自动启动（`StoppedGuard`），授权正是
/// 放行它的**生产路径** ⇒ 本用例顺带钉住"M1 授权后经真实栈真的把 500=1 写到从站"
/// （删掉该写则下面 待机(1) 必红 —— 见 Task 9 的探针 2）。
#[tokio::test]
async fn e4_start_stop_direction_state_machine() {
    let (bus, _state) = SeamBus::new();
    let h = PcsHandle::new(pcs_cfg(), bus, Arc::new(NullSink));

    h.tick_once().await;
    assert_eq!(h.last_run_state(), Some(0), "起点：500 缺省 0 ⇒ 停机(0)");

    // M1：RUN_STATE=0 停机稳态下自动启动被守卫拒绝，须人工授权（单次）
    h.authorize_restart().await.expect("非 latch 下授权应成功");
    h.send_dual_param(&PcsDualParam::new(0.0, 0.0, true, "fallback"))
        .await
        .expect("授权后须放行一次启动");
    h.tick_once().await;
    assert_eq!(
        h.last_run_state(),
        Some(1),
        "500=1（授权写入）、P=0 ⇒ 待机(1)"
    );

    h.send_dual_param(&PcsDualParam::new(5.0, 0.0, true, "fallback"))
        .await
        .expect("写 P=+5 应成功");
    h.tick_once().await;
    assert_eq!(h.last_run_state(), Some(3), "P>0 ⇒ 放电(3)");

    h.send_dual_param(&PcsDualParam::new(-5.0, 0.0, true, "fallback"))
        .await
        .expect("写 P=−5 应成功");
    h.tick_once().await;
    assert_eq!(h.last_run_state(), Some(2), "P<0 ⇒ 充电(2)");

    h.stop().await.expect("stop() 写 500=0 应成功");
    h.tick_once().await;
    assert_eq!(h.last_run_state(), Some(0), "500=0 ⇒ 停机(0)");
}

/// E5 告警字可读（对应 e5_alarm_estop_bit_readable）：告警1（3 区 1000）bit2 = 急停。
/// ① 帧级：线值 = `0x0004` 的字节互换 `0x0400`，`from_pcs_reg` 回解后 bit2 置位；
/// ② 采集面：该字**进遥测点 `pcs_3zone_1`**（值 4.0）⇒ "被采集读到"，而不只被裸读看到。
#[tokio::test]
async fn e5_alarm_estop_bit_readable() {
    let (bus, state) = SeamBus::new();
    let sink = Arc::new(RecSink::default());
    let h = PcsHandle::new(pcs_cfg(), bus.clone(), sink.clone());
    state.set_alarm(0, 1 << 2); // 告警1 bit2 = 急停（协议 p6 序号 3）

    let wire = bus
        .read_input(1, REG_ALARM_BASE, 1)
        .await
        .expect("FC04 读告警1 应成功")[0];
    assert_eq!(
        wire,
        (1u16 << 2).swap_bytes(),
        "线值应为告警字的字节互换（字内 bit2 ⇔ 线上 bit10）"
    );
    assert_eq!(wire, 0x0400, "0x0004 互换 ⇒ 0x0400");
    assert!(
        (from_pcs_reg(wire) as u16) & (1 << 2) != 0,
        "解码后急停位 bit2 应置位"
    );

    // 采集面：告警1 是 pcs_3zone 窗口第 1 点（块内偏移 0 ⇒ 点名 pcs_3zone_1）
    h.tick_once().await;
    let batches = sink.telemetry.lock().unwrap();
    let pts = batches.first().expect("成功一拍须投一批遥测");
    assert_eq!(
        pts[0],
        ("pcs_3zone_1".to_string(), 4.0, false),
        "告警1 须被采集面带进遥测第 1 点（且经块级 byte_swap 回解为 4）"
    );
}

/// E6 并发不串线（对应 e6_concurrent_write_read_no_crosstalk）：
/// 写方 N 次 `send_dual_param` 与读方 N 次 `tick_once` 并发。判据：
/// ① 两侧都不得报错 —— FC06 的**回显校验**要求从站号/功能码/地址/值四项与请求逐字一致，
///    两帧若交错（请求配到别人的响应）必报错；
/// ② 所有写都真的到达从站：末次写（i=49 为奇 ⇒ −5）原样落在镜像 `hold[1001]`。
#[tokio::test]
async fn e6_concurrent_write_read_no_crosstalk() {
    const N: u16 = 50;
    let (bus, state) = SeamBus::new();
    let h = PcsHandle::new(pcs_cfg(), bus.clone(), Arc::new(NullSink));
    write_run(&bus).await;

    let hw = Arc::clone(&h);
    let writer = tokio::spawn(async move {
        for i in 0..N {
            let p = if i % 2 == 0 { 5.0 } else { -5.0 };
            hw.send_dual_param(&PcsDualParam::new(p, 0.0, true, "fallback"))
                .await
                .unwrap_or_else(|e| panic!("写 1001 第 {i} 次失败（疑似串帧/回显不符）: {e}"));
        }
    });
    let hr = Arc::clone(&h);
    let reader = tokio::spawn(async move {
        for i in 0..N {
            hr.tick_once().await;
            let st = hr
                .last_run_state()
                .unwrap_or_else(|| panic!("读 1013 第 {i} 拍得非法值（疑似串帧）"));
            assert!(matches!(st, 1..=3), "500=1 ⇒ run_state∈{{1,2,3}}，得 {st}");
        }
    });
    writer.await.expect("writer 任务不应 panic");
    reader.await.expect("reader 任务不应 panic");

    let hold = state.hold.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(
        hold.get(&REG_CONST_P_SET).copied(),
        Some(-5.0),
        "末次写（i=49，奇 ⇒ −5）必须原样落从站镜像 —— 写序列不得丢失/串线"
    );
}

/// 等到采集读次数达到 `want`（假时钟下的有界让步）。
///
/// **为什么是有界的**：时钟暂停时，采集循环除了"首次 tick"外只能靠 `advance` 推进 ⇒
/// 单次让步能多出的拍数上界已知（本用例逐周期推进时为 1）。故"让步到计数到位"既不
/// flaky（不会无限等）也不会掩盖问题（超时即 panic 报实得值）。
async fn wait_ticks(bus: &Arc<SeamBus>, want: usize) {
    for _ in 0..64 {
        if bus.input_calls.load(Ordering::Relaxed) >= want {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!(
        "假时钟下等待第 {want} 拍超时（实得 {}）—— 采集循环未按预期推进",
        bus.input_calls.load(Ordering::Relaxed)
    );
}

/// E7 定时循环契约 —— 每 `interval_ms` 一拍（tokio **假时钟**确定性驱动）、**Burst**
/// 语义（与迁移前 `run_heartbeat_loop` 一致）、首次 tick 立即完成、**不做退避**
/// （失败也不降频）、返回的 `JoinHandle` 可 abort。
///
/// Task 8 质量评审查出：`spawn_collection_loop` 此前**全仓零覆盖**（本用例是唯一落点）。
#[tokio::test(start_paused = true)]
async fn e7_collection_loop_ticks_at_interval_no_backoff_and_abort() {
    let (bus, _state) = SeamBus::new();
    let h = PcsHandle::new(pcs_cfg(), bus.clone(), Arc::new(NullSink));
    let period = Duration::from_millis(pcs_cfg().interval_ms);
    let count = || bus.input_calls.load(Ordering::Relaxed);

    let task = h.spawn_collection_loop();

    // ① 首次 tick 立即完成（裸 tokio interval 语义：不等一个周期）
    wait_ticks(&bus, 1).await;
    assert_eq!(count(), 1, "启动即采一拍（首次 tick 立即完成）");
    assert!(h.is_connected().await, "首拍成功 ⇒ 在线");

    // ② 每 interval_ms 恰一拍
    for want in [2usize, 3] {
        tokio::time::advance(period).await;
        wait_ticks(&bus, want).await;
        assert_eq!(count(), want, "推进一个周期应恰增一拍");
    }

    // ③ Burst：一次推进 3 个周期（任务被"饿住"）⇒ 连续补跑 3 拍。
    //    判别力：`MissedTickBehavior::Skip`/`Delay` 下此处只会补 1 拍（脚本会停在 4）⇒ 必红。
    tokio::time::advance(period * 3).await;
    wait_ticks(&bus, 6).await;
    assert_eq!(count(), 6, "Burst：落后 3 拍须连续补跑 3 拍");

    // ④ 不做退避：读全失败期间**仍按原周期**一拍一次（失败不降频、不停摆）
    bus.fail_input.store(true, Ordering::Relaxed);
    for want in [7usize, 8, 9] {
        tokio::time::advance(period).await;
        wait_ticks(&bus, want).await;
        assert_eq!(count(), want, "失败拍仍须按原周期推进（不做退避）");
    }
    assert!(
        !h.is_connected().await,
        "连续 3 拍失败 ⇒ 判离线（证明失败真的生效，上面的计数不是空转）"
    );

    // ⑤ abort 后不再采（含 Burst 补跑判定）
    task.abort();
    tokio::time::advance(period * 5).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert_eq!(count(), 9, "abort 后不得再采");
    assert!(task.is_finished(), "abort 后任务须已终止");
}
