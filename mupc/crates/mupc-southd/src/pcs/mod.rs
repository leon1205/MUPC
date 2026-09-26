//! PCS（两级式 PCS = 实时控制模块）通信与控制（设计 §13）。
//!
//! 本模块是 PCS 的**完整所有者**：采集循环、控制序列、状态机与联锁 latch 同在
//! `PcsHandle` 内，**采集与控制共用同一把锁**（§13.4）—— 这是 §11.8② 要求的
//! "写与读共用同一总线仲裁"的更强形式。PCS **独占一路 RS485**（`south_pcs` 段）。

pub mod regs;
pub mod sim;

pub use regs::*;

use crate::config::SouthPcsConfig;
use crate::port_runtime::{BusError, StationBus};
use crate::scheduler::StationSink;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{Mutex, RwLock};

/// PCS 控制面错误。
#[derive(Debug, thiserror::Error)]
pub enum PcsError {
    #[error("PCS 总线错误: {0}")]
    Bus(#[from] BusError),
    /// 联锁锁存中（禁自动启动）
    #[error("联锁锁存：{0}")]
    Latched(String),
    /// M1 守卫：`RUN_STATE=0` 停机稳态下拒绝自动启动
    #[error("M1 守卫：{0}")]
    StoppedGuard(String),
}

/// AI 双参数下发（`p_ref` + `k_droop`）。
///
/// **为什么在南向独立定义**：`intercore::DualParamCommand` 同时被 intercore 的 TCP
/// 帧编码器（`v2_control_frame_bytes`）使用。把类型搬到任一侧都会造成**反向依赖**
/// （核间↔南向）。故 PCS 侧自有类型，消费者按目标门面构造（设计 §13.5.1）。
#[derive(Debug, Clone, PartialEq)]
pub struct PcsDualParam {
    pub p_ref: f64,
    pub k_droop: f64,
    pub ai_ready: bool,
    pub strategy_mode: String,
}

impl PcsDualParam {
    pub fn new(p_ref: f64, k_droop: f64, ai_ready: bool, strategy_mode: &str) -> Self {
        Self {
            p_ref,
            k_droop,
            ai_ready,
            strategy_mode: strategy_mode.to_string(),
        }
    }
}

/// 三相展示读数（PCS 3 区 1022-1024 电流 / 1029-1031 有功 / 1032 总有功）。
///
/// 由 `intercore::transport::ThreePhaseRead` 迁入；**字段与 `Option` 语义不变**
/// （12 号设计 §3.4 F5.5 的点级降级消费侧落点照旧）。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ThreePhaseRead {
    /// 三相输出电流 [A, B, C]，单位 A
    pub i_phase: Option<[f64; 3]>,
    /// 三相输出有功 [A, B, C]，单位 kW（正放负充）
    pub p_phase: Option<[f64; 3]>,
    /// 设备总有功，单位 kW（正放负充）
    pub p_total: Option<f64>,
}

pub struct PcsInner {
    cfg: SouthPcsConfig,
    bus: Arc<dyn StationBus>,
    /// **采集与控制共用的唯一一把锁**（设计 §13.3/§13.4）。入口持锁使整条控制序列
    /// （切模式 → 启停 → 写功率）在物理线路上原子，同时把采集读挡在序列之外 ——
    /// 这是 §11.8② "写必须与读共用同一总线仲裁"的**更强形式**（两者同属一个所有者）。
    lock: Mutex<()>,
    /// 链路在线态（由采集循环维护；连续 3 拍失败判离线 —— 对齐既有心跳口径，设计 Δ-16）
    #[allow(dead_code)] // Task 8（采集循环）读写；本 Task 只备字段，防 clippy -D warnings
    connected: RwLock<bool>,
    /// 已下发的有功模式字（0=恒功率 / 2=分相）。初值 0xFF 哨兵 ⇒ 首条指令必写 `REG_MODE`
    /// （PCS 上电默认模式未知，不能省首次写）。
    mode: AtomicU8,
    /// 是否已下发运行指令（`REG_START_STOP=1`）。初值 false ⇒ 首条指令必写启停。
    started: RwLock<bool>,
    /// 联锁锁存（运行期兜底档；C-1 双 latch 之一，**权威源仍在 storage/DB**）。
    /// **只由 `restore_interlock_latched` 置/清**。
    stopped_latched: RwLock<bool>,
    /// 人工授权重启位（M1，**单次**）：授权后放行 `ensure_started` 的 S-4 守卫一次，消费即弃。
    restart_authorized: AtomicBool,
    /// 采集快照（`last_run_state` 为**同步** getter ⇒ 用 std RwLock —— tokio 的
    /// `blocking_read` 在异步执行上下文内会 panic，与迁移前同一取向）。
    snapshot: StdRwLock<PcsSnapshot>,
    /// 采集出口
    #[allow(dead_code)] // Task 8（采集循环投递遥测）消费
    sink: Arc<dyn StationSink>,
    /// 采集连续失败计数（累计 `BAD_LIMIT` 判离线）—— Task 8 用
    #[allow(dead_code)] // Task 8（采集循环坏拍计数）消费
    bad: AtomicU32,
}

/// PCS 采集快照（设计 §13.4）。**只承载"按需读"的项**；采集失败 ⇒ `valid = false`
/// ⇒ 三个按需读一律返回 `None`（与迁移前"读失败即 None"逐字等价）。
// `Default` 派生 == 逐字段的空值初值（`valid: false` / 其余 `None`）—— 与设计 §13.4 的
// 初值口径一致（clippy `derivable_impls`：手写 impl 与派生等价，取派生免去两份真源）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PcsSnapshot {
    pub valid: bool,
    /// 本拍采集时刻（`latest_soc` 以它作时间戳 —— 设计 Δ-18）
    pub ts: Option<std::time::Instant>,
    pub soc: Option<f64>,
    pub run_state: Option<u16>,
    pub i_phase: Option<[f64; 3]>,
    pub p_phase: Option<[f64; 3]>,
    pub p_total: Option<f64>,
}

/// PCS 门面（PCS 的完整所有者）。各消费方持 `Arc<PcsHandle>`。
pub struct PcsHandle {
    inner: Arc<PcsInner>,
}

impl PcsHandle {
    pub fn new(
        cfg: SouthPcsConfig,
        bus: Arc<dyn StationBus>,
        sink: Arc<dyn StationSink>,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: Arc::new(PcsInner {
                cfg,
                bus,
                lock: Mutex::new(()),
                connected: RwLock::new(false),
                mode: AtomicU8::new(0xFF),
                started: RwLock::new(false),
                stopped_latched: RwLock::new(false),
                restart_authorized: AtomicBool::new(false),
                snapshot: StdRwLock::new(PcsSnapshot::default()),
                sink,
                bad: AtomicU32::new(0),
            }),
        })
    }

    fn slave(&self) -> u8 {
        self.inner.cfg.slave
    }

    // ── 测试观测点（仅测试构建；生产零痕迹）──────────────────────────
    #[cfg(test)]
    pub(crate) async fn debug_started(&self) -> bool {
        *self.inner.started.read().await
    }
    /// 测试用造态：直接置 `started`（原文 `test_authorize_restart_gated` 的
    /// `*tr.started.write().await = true` 同等动作）。
    #[cfg(test)]
    pub(crate) async fn debug_set_started(&self, v: bool) {
        *self.inner.started.write().await = v;
    }
    #[cfg(test)]
    pub(crate) fn debug_mode(&self) -> u8 {
        self.inner.mode.load(Ordering::Relaxed)
    }
    #[cfg(test)]
    pub(crate) fn debug_restart_authorized(&self) -> bool {
        self.inner.restart_authorized.load(Ordering::Relaxed)
    }
}

impl PcsHandle {
    // ── 联锁面（供 `InterlockPort` 适配）────────────────────────────
    /// 停机原语：写 `REG_START_STOP=0`。**不设/不清 latch**（C-1：置位由触发沿经
    /// `restore_interlock_latched(true)` 完成）。成功后复位 `started=false` + `mode=0xFF`
    /// （否则 release 后 `ensure_started`/`ensure_mode` 见缓存命中而跳过重写 → 静默失效）。
    /// 写 500=0 在 latch 期间**仍允许**（供联锁周期重试停机）。
    pub async fn stop(&self) -> Result<(), String> {
        let _g = self.inner.lock.lock().await;
        match self
            .inner
            .bus
            .write_single(self.slave(), regs::REG_START_STOP, regs::to_pcs_reg(0.0))
            .await
        {
            Ok(()) => {
                *self.inner.started.write().await = false;
                self.inner.mode.store(0xFF, Ordering::Relaxed);
                Ok(())
            }
            Err(e) => {
                // M-3：中性措辞 —— 不预设调用方、不臆断 latch 必然已置位；只陈述写失败事实
                // 与后续动作（停机未确认 ⇒ stop_failed，交由联锁流程按 latch 状态周期重试）。
                tracing::error!(
                    "stop 写 REG_START_STOP=0 失败：{}——PCS 停机未确认（stop_failed），\
                     交由联锁流程周期重试",
                    e
                );
                Err(format!("PCS 停机写 500=0 失败: {e}"))
            }
        }
    }

    /// 置/清联锁 latch（C-1 **唯一**入口：触发沿 `true`；release / DB 读回 `false`）。
    /// 不取总线锁（纯内存状态，不涉及物理线路）。
    pub async fn restore_interlock_latched(&self, latched: bool) -> Result<(), String> {
        *self.inner.stopped_latched.write().await = latched;
        // 新触发沿（restore(true) = 一次**新的**安全停机事件）作废任何**未消费**的人工重启
        // 授权：授权后、`send_*` 消费前若又发生一次联锁触发 + release，不允许凭**触发前**的
        // 陈旧授权在 release 后自动重启（须重新 `authorize_restart`，人工确认针对的是最新一次
        // 事件）。此语义自 `ModbusRtuTransport::restore_interlock_latched` 逐字迁入。
        if latched {
            self.inner
                .restart_authorized
                .store(false, Ordering::Relaxed);
        }
        Ok(())
    }

    /// latch 查询。
    pub async fn is_interlock_stopped(&self) -> bool {
        *self.inner.stopped_latched.read().await
    }

    /// M1 保护跳闸/停机**人工授权重启**（单次）：`!latch` 时复位 `started` **并置授权**，
    /// 放行下次 `ensure_started` 在 `RUN_STATE=0` 停机稳态下重写 500=1 一次。
    /// latch 期间 `Err`（须先 release）。
    pub async fn authorize_restart(&self) -> Result<(), String> {
        if *self.inner.stopped_latched.read().await {
            return Err("联锁锁存中：须先 release 才能授权重启".to_string());
        }
        *self.inner.started.write().await = false;
        self.inner.restart_authorized.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// 最新 `RUN_STATE(1013)`（**同步** getter，读采集快照）。
    pub fn last_run_state(&self) -> Option<u16> {
        let s = self
            .inner
            .snapshot
            .read()
            .unwrap_or_else(|e| e.into_inner());
        if s.valid {
            s.run_state
        } else {
            None
        }
    }
}

impl PcsHandle {
    /// 下发 AI 双参数（恒功率）：写 `REG_MODE=0` → `ensure_started` → 写 1001/1002。
    pub async fn send_dual_param(&self, cmd: &PcsDualParam) -> Result<(), PcsError> {
        // C-1 下行中止：latch 期间**任何总线 IO 前**拒绝。检查置于总线锁**之前** ——
        // latch 期间连锁都不取（不必先排在在途写序列之后），停机路径得以更快拿到锁。
        self.check_latched().await?;
        let _g = self.inner.lock.lock().await;
        self.ensure_mode(regs::MODE_CONST_POWER).await?;
        self.ensure_started().await?;
        self.write_reg(regs::REG_CONST_P_SET, regs::to_pcs_reg(cmd.p_ref))
            .await?;
        self.write_reg(regs::REG_CONST_Q_SET, regs::to_pcs_reg(0.0))
            .await?;
        Ok(())
    }

    /// 下发台区储能分相 P/Q：写 `REG_MODE=2` → `ensure_started` → 写 1006-1011
    /// （逐相 `clamp ±25`，与迁移前 `clamp_phase` 同源）。
    pub async fn send_tai_command(
        &self,
        p: [f64; 3],
        q: [f64; 3],
        _strategy_mode: &str,
    ) -> Result<(), PcsError> {
        self.check_latched().await?;
        let _g = self.inner.lock.lock().await;
        self.ensure_mode(regs::MODE_PHASE_SPLIT).await?;
        self.ensure_started().await?;
        for i in 0..3u16 {
            self.write_reg(
                regs::REG_PHASE_P_A + i,
                regs::to_pcs_reg(regs::clamp_phase(p[i as usize])),
            )
            .await?;
            self.write_reg(
                regs::REG_PHASE_Q_A + i,
                regs::to_pcs_reg(regs::clamp_phase(q[i as usize])),
            )
            .await?;
        }
        Ok(())
    }

    // ── 内部原语（**不取锁**，由入口持锁；与迁移前纪律一致，防嵌套死锁）──
    async fn check_latched(&self) -> Result<(), PcsError> {
        if *self.inner.stopped_latched.read().await {
            // 逐字迁回原文两处入口的日志措辞（`send_tai_command` / `send_dual_param` 各有一条
            // 同文 `tracing::warn!`；本函数为两入口共用 ⇒ 一条即可）。
            tracing::warn!("interlock stopped：拒绝下行（含启动/功率写）");
            return Err(PcsError::Latched("联锁锁存禁止下发".into()));
        }
        Ok(())
    }

    /// FC06 写单寄存器。**不动在线态**（在线/离线统一由采集循环判定 —— 设计 Δ-16）。
    async fn write_reg(&self, addr: u16, value: u16) -> Result<(), PcsError> {
        self.inner
            .bus
            .write_single(self.slave(), addr, value)
            .await?;
        Ok(())
    }

    /// FC04 读输入寄存器。**不含在线态副作用**（同 `write_reg` 的理由）。
    async fn read_input(&self, addr: u16, len: u16) -> Result<Vec<u16>, PcsError> {
        Ok(self.inner.bus.read_input(self.slave(), addr, len).await?)
    }

    /// 确保处于指定有功模式（缓存命中则跳过；否则写 `REG_MODE` 并更新缓存）。
    async fn ensure_mode(&self, mode: u16) -> Result<(), PcsError> {
        if self.inner.mode.load(Ordering::Relaxed) as u16 == mode {
            return Ok(());
        }
        self.write_reg(regs::REG_MODE, regs::to_pcs_reg(mode as f64))
            .await?;
        self.inner.mode.store(mode as u8, Ordering::Relaxed);
        Ok(())
    }

    /// 确保 PCS 已运行（`REG_START_STOP=1`）。
    ///
    /// ① `stopped_latched` ⇒ `Err`（联锁禁启，底层兜底）；
    /// ② `started==true` 缓存命中 ⇒ `Ok`（跳过总线往返）；
    /// ③ 否则 **S-4 前置读**：先 FC04 读 `REG_RUN_STATE(1013)` 校验 PCS 非停机 ——
    ///    堵住「离线窗口内保护跳闸 → 链路恢复自动重启跳闸机」。读到 0（停机）时：
    ///    已 `restart_authorized` ⇒ **消费授权**（清位）并放行重写 500=1（M1 单次旁路）；
    ///    否则 `Err` 交上层（运维走 `authorize_restart`）。非 0 或读数无效 ⇒ 清授权并正常写 500=1。
    /// ④ **I-2 复查**：读后、写 500=1 前再查一次 latch（`restore` 不取总线锁，可随时置位）。
    async fn ensure_started(&self) -> Result<(), PcsError> {
        if *self.inner.stopped_latched.read().await {
            return Err(PcsError::Latched("联锁锁存禁止自动启动".into()));
        }
        if *self.inner.started.read().await {
            return Ok(());
        }
        // S-4：首个写启动前先读一次 RUN_STATE 校验非停机（M1 守卫升级）
        let words = self.read_input(regs::REG_RUN_STATE, 1).await?;
        let run = decode_run_state(words.first().copied().unwrap_or(u16::MAX));
        if run == Some(0) {
            if !self.inner.restart_authorized.load(Ordering::Relaxed) {
                tracing::warn!("M1 前置校验：RUN_STATE=0（停机/保护跳闸），不自动启动，交上层");
                return Err(PcsError::StoppedGuard(
                    "RUN_STATE=0 不允许自动启动（M1 守卫）".into(),
                ));
            }
            // 决策点即清位：即使随后写失败 / I-2 复查放弃，授权已消耗（须重授权），
            // 杜绝过期授权在下次停机后凭陈旧位自动重启（M1 单次语义）。
            self.inner
                .restart_authorized
                .store(false, Ordering::Relaxed);
            tracing::info!("M1 授权重启：RUN_STATE=0 且已授权，放行重发 500=1（单次，授权已消费）");
        } else {
            self.inner
                .restart_authorized
                .store(false, Ordering::Relaxed);
        }
        // I-2：读后、写 500=1 前再查一次 latch
        if *self.inner.stopped_latched.read().await {
            tracing::warn!("S-4 写前复查：联锁 latch 已在此窗口置位，放弃启动");
            return Err(PcsError::Latched("S-4 写前 latch 置位，放弃启动".into()));
        }
        self.write_reg(regs::REG_START_STOP, regs::to_pcs_reg(1.0))
            .await?;
        *self.inner.started.write().await = true;
        Ok(())
    }
}

/// 解码 PCS 3 区 SOC 字 → 百分比（0..=100）；非有限或越界返回 None（视为无效读数）
#[allow(dead_code)] // Task 8（采集循环 PcsSnapshot::soc）消费；本 Task 先原样迁入
fn decode_soc(word: u16) -> Option<f64> {
    let soc = from_pcs_reg(word);
    if soc.is_finite() && (0.0..=100.0).contains(&soc) {
        Some(soc)
    } else {
        None
    }
}

/// 解码 PCS 3 区运行状态字 → 0停机/1待机/2充电/3放电；越界返回 None（乱码/错位帧判无效读数）
fn decode_run_state(word: u16) -> Option<u16> {
    let st = from_pcs_reg(word);
    if st.is_finite() && (0.0..=3.0).contains(&st) {
        Some(st as u16)
    } else {
        None
    }
}

#[cfg(test)]
mod control_tests {
    use super::*;
    use crate::config::{RegBlockConf, RegFunc, StationParity};
    use crate::port_runtime::MockBus;
    use async_trait::async_trait;

    struct NullSink;
    #[async_trait]
    impl StationSink for NullSink {
        async fn on_grid_package(&self, _pkg: mupc_data_processing::DataPackage) {}
        async fn on_station_telemetry(
            &self,
            _id: &str,
            _role: crate::config::Role,
            _pts: Vec<(String, f64, bool)>,
        ) {
        }
        async fn on_battery_soc(&self, _id: &str, _soc: f64) {}
    }

    /// 供 `control_tests` 与后续 `collection_tests` 共用。
    pub(super) fn cfg_for_tests() -> SouthPcsConfig {
        SouthPcsConfig {
            enabled: true,
            port: "/dev/ttyS0".into(),
            protocol: "modbus".into(),
            slave: 1,
            baud_rate: 19200,
            data_bits: 8,
            stop_bits: 1,
            parity: StationParity::None,
            interval_ms: 1000,
            response_timeout_ms: 200,
            regs: vec![RegBlockConf {
                name: "pcs_3zone".into(),
                addr: 1000,
                func: RegFunc::Input,
                format: mupc_data_processing::meter_regs::RegFormat::Uint16,
                scale: 1.0,
                count: 76,
                offset: 0.0,
                byte_swap: true,
                points: Vec::new(),
                read_slice: false,
                interval_ms: None,
            }],
        }
    }

    fn handle(bus: Arc<dyn StationBus>) -> Arc<PcsHandle> {
        PcsHandle::new(cfg_for_tests(), bus, Arc::new(NullSink))
    }

    /// 读后置位 latch 的总线装饰器（**仅测试**）。
    ///
    /// **为什么需要它**：I-4 是"读 `RUN_STATE(1013)` 之后、写 `500=1` 之前 latch 被置位 ⇒
    /// 放弃启动"的**窄窗口**防御。`MockBus` 的 `read_input` 是被动返回，无法在窗口内造态；
    /// 给 `MockBus` 加钩子要动 `port_runtime.rs`（不在本 Task 授权范围）。故在测试模块内
    /// 用装饰器包一层 —— **生产代码零改动**（`MockBus` / `StationBus` 一字未动）。
    ///
    /// 钩子在 `inner.read_input` **返回之后**触发 —— 即"读已完成、写尚未发出"，正是 I-4
    /// 复查要拦的那个窗口；一次性（`take()`）避免多条读路径重复触发。
    struct LatchOnReadBus {
        inner: Arc<MockBus>,
        on_input: std::sync::Mutex<Option<Box<dyn FnOnce() -> PinnedFuture + Send>>>,
    }

    type PinnedFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

    impl LatchOnReadBus {
        fn new() -> Self {
            Self {
                inner: Arc::new(MockBus::new()),
                on_input: std::sync::Mutex::new(None),
            }
        }
        /// 登记一次性"读后"钩子。
        fn set_input_hook(&self, hook: impl FnOnce() -> PinnedFuture + Send + 'static) {
            *self.on_input.lock().unwrap() = Some(Box::new(hook));
        }
    }

    #[async_trait]
    impl StationBus for LatchOnReadBus {
        async fn read_holding(
            &self,
            slave: u8,
            addr: u16,
            count: u16,
        ) -> Result<Vec<u16>, BusError> {
            self.inner.read_holding(slave, addr, count).await
        }
        async fn read_input(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError> {
            let r = self.inner.read_input(slave, addr, count).await;
            // 先取走钩子再 await（不留 std MutexGuard 跨 await 点）。
            let hook = self.on_input.lock().unwrap().take();
            if let Some(hook) = hook {
                hook().await;
            }
            r
        }
        async fn read_discrete(
            &self,
            slave: u8,
            addr: u16,
            count: u16,
        ) -> Result<Vec<bool>, BusError> {
            self.inner.read_discrete(slave, addr, count).await
        }
        async fn write_single(&self, slave: u8, addr: u16, value: u16) -> Result<(), BusError> {
            self.inner.write_single(slave, addr, value).await
        }
    }

    #[tokio::test]
    async fn latched_rejects_send_without_any_io() {
        // I-1：latch 期间 send_* 必须拒写，且**一次总线调用都不发生**。
        let bus = Arc::new(MockBus::new());
        let h = handle(bus.clone());
        h.restore_interlock_latched(true).await.unwrap();
        let e = h
            .send_dual_param(&PcsDualParam::new(10.0, 0.5, true, "intelligent"))
            .await
            .unwrap_err();
        assert!(matches!(e, PcsError::Latched(_)), "实际: {e}");
        assert!(
            bus.write_calls.lock().unwrap().is_empty(),
            "latch 期间不得有任何写"
        );
        // I-1 要求"**一次总线调用都不发生**"（不只有写）。此断言此前缺失：评审探针 P2 实证
        // 在 `check_latched` 之前插一次读，本用例仍全绿 ⇒ 读半边零判别力。
        assert!(
            bus.input_calls.lock().unwrap().is_empty(),
            "latch 期间不得有任何读（I-1 要求一次总线调用都不发生）"
        );
    }

    #[tokio::test]
    async fn restore_clear_toggles_latch() {
        let bus = Arc::new(MockBus::new());
        let h = handle(bus);
        assert!(!h.is_interlock_stopped().await);
        h.restore_interlock_latched(true).await.unwrap();
        assert!(h.is_interlock_stopped().await);
        h.restore_interlock_latched(false).await.unwrap();
        assert!(!h.is_interlock_stopped().await);
    }

    /// 新触发沿作废**未消费**的人工授权 —— 迁入时**逐字保留**原
    /// `ModbusRtuTransport::restore_interlock_latched` 的 Minor-2 语义：
    /// 「authorize → 新一次联锁触发 → release」之后，`ensure_started` 不得凭**触发前**的
    /// 陈旧授权在 `RUN_STATE=0` 停机稳态下自动重写 500=1（那等于绕过 M1 人工确认）。
    #[tokio::test]
    async fn new_trip_invalidates_pending_authorization() {
        let bus = Arc::new(MockBus::new());
        let h = handle(bus);
        h.authorize_restart().await.unwrap();
        assert!(h.debug_restart_authorized(), "前提：授权已置位");
        h.restore_interlock_latched(true).await.unwrap();
        assert!(
            !h.debug_restart_authorized(),
            "新触发沿必须作废未消费的授权（否则 release 后凭陈旧授权自动重启）"
        );
        assert!(h.is_interlock_stopped().await, "新触发沿仍须置 latch");
    }

    #[tokio::test]
    async fn stop_writes_500_zero_and_resets_caches() {
        // I-5：停机写 500=0，且成功后复位 started/mode 哨兵（否则 release 后无法重启）。
        let bus = Arc::new(MockBus::new());
        bus.put_input(1, 1013, vec![to_pcs_reg(1.0)]); // RUN_STATE=1 待机
        let h = handle(bus.clone());
        h.send_dual_param(&PcsDualParam::new(0.0, 0.0, true, "intelligent"))
            .await
            .unwrap();
        assert!(h.debug_started().await, "前提：启动成功");
        h.stop().await.unwrap();
        assert!(!h.debug_started().await, "停机必须复位 started");
        assert_eq!(h.debug_mode(), 0xFF, "停机必须复位 mode 哨兵");
        assert!(bus.write_call_count(1, 500) >= 1, "必须写过 500");
    }

    #[tokio::test]
    async fn s4_guard_refuses_autostart_when_run_state_zero() {
        // I-2（M1 守卫）：RUN_STATE=0 且未授权 ⇒ 拒绝自动启动，且**不得写 500=1**。
        let bus = Arc::new(MockBus::new());
        bus.put_input(1, 1013, vec![to_pcs_reg(0.0)]); // 停机稳态
        let h = handle(bus.clone());
        let e = h
            .send_dual_param(&PcsDualParam::new(10.0, 0.5, true, "intelligent"))
            .await
            .unwrap_err();
        assert!(matches!(e, PcsError::StoppedGuard(_)), "实际: {e}");
        // ⚠️ 判据值必须与代码**同源编码**：写 "500=1" 落在线上的是 `to_pcs_reg(1.0)` = 0x0100
        // （PCS 全设备高/低 8 位互换），**不是字面量 1**。计划原文的 `v == 1` 因此**恒假** ——
        // 该断言零判别力（已用注入探针实测：在守卫拒绝分支里插一次 500=1 写，用例仍全绿）。
        // 此处改为同源编码，并已用同一注入探针验证**改坏必红**。
        let run_word = to_pcs_reg(1.0);
        assert_eq!(
            bus.write_calls
                .lock()
                .unwrap()
                .iter()
                .filter(|&&(_, a, v)| a == 500 && v == run_word)
                .count(),
            0,
            "M1 守卫下不得写 500=1（线上字 {run_word:#06x}）"
        );
    }

    /// 原文 `intercore::transport::modbus` 的同名用例**逐字迁入**（断言未改，仅
    /// `*tr.started.write().await = true` → `debug_set_started(true)`、
    /// `*tr.started.read().await` → `debug_started()`）。此前**未被列入迁移清单** ⇒
    /// `authorize_restart` 的 latch 门禁零覆盖（评审探针 P10：删掉门禁，旧用例仍全绿）。
    #[tokio::test]
    async fn test_authorize_restart_gated() {
        // I-1/ack_m1：!stopped_latched 时 authorize 复位 started（下个 send 经 ensure_started
        // 重发 500=1）；stopped_latched 时拒绝（须先 release 清 latch）
        let bus = Arc::new(MockBus::new());
        let h = handle(bus);
        // !latch：复位 started
        h.debug_set_started(true).await;
        h.authorize_restart().await.unwrap();
        assert!(
            !h.debug_started().await,
            "authorize 应复位 started，允许下次 send 重发 500=1"
        );
        // latch：authorize 拒绝且不改 started
        h.restore_interlock_latched(true).await.unwrap();
        h.debug_set_started(true).await;
        assert!(h.authorize_restart().await.is_err());
        assert!(
            h.debug_started().await,
            "latch 期间 authorize 不得复位 started"
        );
    }

    #[tokio::test]
    async fn authorize_restart_grants_single_shot() {
        // I-3：人工授权**单次** —— 授权后放行一次启动，授权即被消费。
        let bus = Arc::new(MockBus::new());
        bus.put_input(1, 1013, vec![to_pcs_reg(0.0)]);
        let h = handle(bus.clone());
        h.authorize_restart().await.unwrap();
        h.send_dual_param(&PcsDualParam::new(10.0, 0.5, true, "intelligent"))
            .await
            .expect("授权后必须放行一次");
        assert!(
            !h.debug_restart_authorized(),
            "授权必须已被消费（单次语义）"
        );
        // 授权不只是"返回 Ok + 消费授权位"，线上必须**真的**写出 500=1 —— 否则 M1 人工
        // 确认后 PCS 永远不会重启。判据值必须**同源编码** `to_pcs_reg(1.0)`（线上字 0x0100，
        // 不是字面量 1）；评审探针 P3 实证：删掉 ensure_started 正路的这次写，旧用例仍全绿。
        assert!(
            bus.write_calls
                .lock()
                .unwrap()
                .iter()
                .any(|&(_, a, v)| a == 500 && v == to_pcs_reg(1.0)),
            "授权后必须真的写了 500=1（线上字 {:#06x}）",
            to_pcs_reg(1.0)
        );
    }

    /// I-4：S-4 前置读 `RUN_STATE` 之后、写 `500=1` 之前 latch 被置位 ⇒ **放弃启动**。
    ///
    /// 这条窄窗口防御此前**零覆盖**（评审注入探针 P5：删掉 `ensure_started` 写前复查，
    /// 旧用例仍全绿）。构造：`LatchOnReadBus` 的读后钩子恰好落在该窗口内（读已返回、
    /// 写未发出），钩子把一个**新的**联锁停机事件置位。
    #[tokio::test]
    async fn latch_set_between_s4_read_and_start_write_aborts() {
        let bus = Arc::new(LatchOnReadBus::new());
        // RUN_STATE=1 待机 ⇒ 本可正常通过 S-4（非 0 不触发 M1 守卫）
        bus.inner.put_input(1, 1013, vec![to_pcs_reg(1.0)]);
        let h = handle(bus.clone());
        let h_hook = h.clone();
        bus.set_input_hook(move || {
            Box::pin(async move {
                h_hook.restore_interlock_latched(true).await.unwrap();
            })
        });
        let e = h
            .send_dual_param(&PcsDualParam::new(10.0, 0.5, true, "intelligent"))
            .await
            .unwrap_err();
        assert!(
            h.is_interlock_stopped().await,
            "前提：钩子确实在窗口内置位了 latch（否则本用例空转）"
        );
        assert_eq!(
            bus.inner.input_call_count(1, 1013),
            1,
            "证明确实走到了 S-4 前置读（否则根本没进入 I-4 的窗口）"
        );
        assert!(matches!(e, PcsError::Latched(_)), "实际: {e}");
        assert_eq!(
            bus.inner.write_call_count(1, 500),
            0,
            "I-4：写前复查发现 latch 置位必须放弃启动（不得写 500）"
        );
    }
}

/// PCS 点表解码（SOC / RUN_STATE）域校验 —— 由 `intercore::transport::modbus`
/// 同名用例**逐字迁入**（断言未改，仅把 `ModbusRtuTransport` 方法调用换成模块内
/// 私有 fn `decode_soc` / `decode_run_state`）。
#[cfg(test)]
mod decode_tests {
    use super::*;

    #[test]
    fn test_decode_run_state() {
        // 0 停机 / 1 待机 / 2 充电 / 3 放电 合法；越界（如乱码/错位帧）判无效
        for st in [0.0, 1.0, 2.0, 3.0] {
            assert_eq!(decode_run_state(to_pcs_reg(st)), Some(st as u16));
        }
        assert_eq!(decode_run_state(to_pcs_reg(-1.0)), None);
        assert_eq!(decode_run_state(to_pcs_reg(4.0)), None);
        // 0xFFFF 原样（未解互换）解码 → -1 → 无效
        assert_eq!(decode_run_state(0xFFFF), None);
    }

    #[test]
    fn test_decode_soc_valid() {
        // 66% → to_pcs_reg(66) 字节互换，回解须还原 66
        assert_eq!(decode_soc(to_pcs_reg(66.0)), Some(66.0));
    }

    #[test]
    fn test_decode_soc_rejects_out_of_range() {
        // 负数与 >100 均视为无效读数
        assert_eq!(decode_soc(to_pcs_reg(-1.0)), None);
        assert_eq!(decode_soc(to_pcs_reg(101.0)), None);
    }
}
