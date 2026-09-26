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

/// 私有共享态（`PcsHandle` 的内部）。`pub(crate)` 而非 `pub`：本类型**可达但不可构造**
/// （无 `pub` 构造函数，`PcsHandle::inner` 字段私有），全仓仅在**本文件内**被引用
/// （`PcsHandle::inner` 字段类型 + `PcsHandle::new` 构造）；对外契约是 [`PcsHandle`]
/// 与 [`PcsSnapshot`]，不需要把内部态暴露到 crate 外。
pub(crate) struct PcsInner {
    cfg: SouthPcsConfig,
    bus: Arc<dyn StationBus>,
    /// **采集与控制共用的唯一一把锁**（设计 §13.3/§13.4）。入口持锁使整条控制序列
    /// （切模式 → 启停 → 写功率）在物理线路上原子，同时把采集读挡在序列之外 ——
    /// 这是 §11.8② "写必须与读共用同一总线仲裁"的**更强形式**（两者同属一个所有者）。
    lock: Mutex<()>,
    /// 链路在线态（由采集循环维护；连续 3 拍失败判离线 —— 对齐既有心跳口径，设计 Δ-16）
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
    /// M1 停机告警的**跨拍去抖**记忆（"非停机→停机"跃迁告警一次；恢复非停机态时复位）。
    /// 与 `intercore::ModbusRtuTransport` 的 `stopped_warned` 局部变量语义等价（此处提升为字段）。
    stopped_warned: AtomicBool,
    /// "空 `regs`"告警的一次性记忆（`tick_once` 的早退路径**只记一次** —— 否则每拍一条，
    /// `interval_ms` 周期无限刷屏；与 M1 告警同款理由，见 `warn_stopped_once`）。
    empty_cfg_warned: AtomicBool,
    /// 采集快照（`last_run_state` 为**同步** getter ⇒ 用 std RwLock —— tokio 的
    /// `blocking_read` 在异步执行上下文内会 panic，与迁移前同一取向）。
    snapshot: StdRwLock<PcsSnapshot>,
    /// 采集出口
    sink: Arc<dyn StationSink>,
    /// 采集连续失败计数（累计 `BAD_LIMIT` 判离线）
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
                stopped_warned: AtomicBool::new(false),
                empty_cfg_warned: AtomicBool::new(false),
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

    // ── 快照存取（`StdRwLock` 的 poison 惯用法**集中于此**）────────────────
    // `unwrap_or_else(|e| e.into_inner())` 的"取毒"取向全仓仅此两处：快照是**纯展示/
    // 按需读**数据，poison 时取最后一个完整值比 panic 掉采集线程更可取（与迁移前一致）。
    // 集中一处免得 5 个调用点各写一份、各自漂移。

    /// 读快照。
    fn snapshot(&self) -> PcsSnapshot {
        *self
            .inner
            .snapshot
            .read()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// 写快照。
    fn set_snapshot(&self, s: PcsSnapshot) {
        *self
            .inner
            .snapshot
            .write()
            .unwrap_or_else(|e| e.into_inner()) = s;
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
                // 文案：`BusError::Write` 的 Display 已含 slave+地址+线上值+原因，再写"失败"
                // 会成"失败: …失败"三连；此处只给结论（停机未确认）+ 包装错误原文。
                Err(format!("PCS 停机写 500=0 未确认: {e}"))
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
    ///
    /// 口径：读**采集快照**（非现读）—— 快照由采集循环（Task 8）每 `interval_ms` 写入一次。
    /// `None` 有两义：**本拍读失败**（快照 `valid = false`，设计 §13.4 单拍语义）或**读数
    /// 域外**（`decode_run_state` 判乱码/错位帧）；域内但真停机 → `Some(0)`。消费者在
    /// Task 10 接线（经适配器供 DO1/联锁）。
    /// 保持 `pub`：Task 10 在**另一个 crate** 经适配器调用。
    pub fn last_run_state(&self) -> Option<u16> {
        let s = self.snapshot();
        if !s.valid {
            return None;
        }
        s.run_state
    }
}

/// 连续失败上限（判离线）—— 对齐迁移前心跳的 `BAD_LIMIT = 3`
/// （迁移前 `intercore::transport::modbus::run_heartbeat_loop` 的 `BAD_LIMIT`，该模块 T4 删除）。
const BAD_LIMIT: u32 = 3;
/// 采集循环的 **tick 非零兜底**（防 `interval(0)` panic）。
///
/// **命名与语义**：与配置层的 `PCS_MIN_INTERVAL_MS`（500ms，**策略下界**）刻意区分 ——
/// 二者是"错位近义"（都像周期下界）而语义**相反**：那个是"防超短周期打满总线"的**策略**
/// 下界（`SouthPcsConfig::validate` 强制），本常量只是**非零兜底**。`PcsHandle::new`
/// **不校验配置** ⇒ 绕过 validate 构造（或将来容错路径）时 `interval(0)` 会 panic，
/// 故此处再夹一层。
///
/// **取值理由**：tokio 仅在**周期为 0** 时 panic（非零任意值都合法）⇒ 本兜底只需"非零"；
/// 取 100 只是留余量（明显高于任何真实周期预算的探测下限，又远低于策略下界 500 ——
/// 一旦真有人靠这个值在跑，说明已绕过 validate，那是配置路径问题、不是本值该兜的）。
const PCS_TICK_FLOOR_MS: u64 = 100;

impl PcsHandle {
    /// 链路在线态（由采集循环维护；设计 §13.4 的"累积 3 拍"口径）。
    pub async fn is_connected(&self) -> bool {
        *self.inner.connected.read().await
    }

    /// SOC（%）。**读采集快照**（非现读）—— 时间戳为本拍采集时刻（设计 Δ-18）。
    pub async fn latest_soc(&self) -> Option<(f64, std::time::Instant)> {
        let s = self.snapshot();
        if !s.valid {
            return None;
        }
        s.soc.zip(s.ts)
    }

    /// 三相展示读数（读采集快照；设计 Δ-17：粒度为**单块**，块读失败即整体 `None`）。
    pub async fn read_three_phase(&self) -> Option<ThreePhaseRead> {
        let s = self.snapshot();
        if !s.valid {
            return None;
        }
        Some(ThreePhaseRead {
            i_phase: s.i_phase,
            p_phase: s.p_phase,
            p_total: s.p_total,
        })
    }

    /// **一拍采集**（测试可直接驱动，不含 sleep）：FC04 读一次 3 区块 → 解码 →
    /// 更新快照 → 投 sink → 记账。持**与采集/控制共用的同一把锁**（设计 §13.3）：
    /// 本函数运行期间控制序列无法插进物理线路。
    ///
    /// **读失败的两级语义**（设计 §13.4 的注，刻意不统一）：
    /// - **快照失效 = 单拍**：立即 `valid = false` ⇒ 三个按需读全 `None`
    ///   （与迁移前"读失败即 None"逐字等价）；
    /// - **在线态 = 累积 `BAD_LIMIT` 拍**：对齐迁移前心跳口径（`latest_soc` 走
    ///   mark_offline 的"单次失败即离线"属迁移前的不一致，本 Task 收敛掉）。
    pub async fn tick_once(&self) {
        let _g = self.inner.lock.lock().await;
        // 共享借用（非 `clone`）：`blk` 在 `.await` 上存活合法，借用检查无 clone 需求。
        // 下方 `BlockReads` 那处的 `blk.clone()` 则**必须保留**（`Vec<(RegBlockConf, ..)>`
        // 要所有权，`&RegBlockConf` 无法满足）。
        let blk = match self.inner.cfg.regs.first() {
            Some(b) => b,
            None => {
                // 空 `regs` 早退此前**完全静默** ⇒ 采集循环以 `connected=false` 空转、无线索。
                // 规则 P-5 后该形态在配置期已被拒，但 `PcsHandle::new` **不校验**配置 ⇒
                // 单测/未来路径仍可构造。
                //
                // **只记一次**（不是每拍）：本函数每 `interval_ms` 走一遍，逐拍记 ⇒
                // ≈86400 条/日刷屏 —— 与 M1 告警（`warn_stopped_once`）同款理由，故用
                // `empty_cfg_warned` 做一次性记忆（空配置不会自愈，一条足够定位）。
                if !self.inner.empty_cfg_warned.swap(true, Ordering::Relaxed) {
                    tracing::error!(
                        "south_pcs.regs 为空 —— 采集恒空转（PcsHandle::new 不校验配置）"
                    );
                }
                return;
            }
        };
        let res = self
            .inner
            .bus
            .read_input(self.slave(), blk.addr, blk.count)
            .await;

        match res {
            Ok(words) => {
                let ts = std::time::Instant::now();
                let snap = build_snapshot(&words, blk.addr, ts);
                self.set_snapshot(snap);
                self.inner.bad.store(0, Ordering::Relaxed);
                {
                    let mut c = self.inner.connected.write().await;
                    if !*c {
                        tracing::info!(station = "pcs", "PCS 链路恢复在线");
                    }
                    *c = true;
                }
                // ③ 停机观测（M1 告警**去抖**）：仅在"非停机 → 停机"跃迁时告警一次。
                //    去抖靠 `stopped_warned` 跨拍记忆（不是"每拍判一次"——见该函数的注释）。
                if warn_stopped_once(
                    &self.inner.stopped_warned,
                    snap.run_state,
                    *self.inner.started.read().await,
                    *self.inner.stopped_latched.read().await,
                ) {
                    tracing::warn!(
                        "PCS 运行状态=0(停机)但 MUPC 此前已下发启动——疑似保护跳闸/人工停机；\
                         链路在线，MUPC 不自动重启，请上层/运维确认后处理"
                    );
                }
                // 遥测点上送（块级：成功即全量，与站级调度器"标量每轮全量"口径一致）。
                //
                // **计划 Step 4 分叉的定论（实测，非推断）**：`mapper::telemetry_points`
                // **不按 `role` 过滤**（仅一条 `debug_assert_ne!(role, MeterGrid)` 只读断言），
                // 内部即 `points::expand` + 逐点 `RegDecode::decode` ⇒ `Role::Pcs` 经它产出
                // 与 `points::expand` **完全一致**的 72 点（`total == 72` 实测通过）。
                // 故**不**改走 `points::expand` 直连（备选分支不成立）；沿用本函数还与站级
                // scheduler 同一入口，展开口径天然统一。
                let reads: crate::mapper::BlockReads =
                    vec![(blk.clone(), Ok(crate::mapper::BlockData::Regs(words)))];
                let pts: Vec<(String, f64, bool)> =
                    crate::mapper::telemetry_points(crate::config::Role::Pcs, &reads)
                        .into_iter()
                        .map(|s| (s.metric, s.value, false))
                        .collect();
                if !pts.is_empty() {
                    self.inner
                        .sink
                        .on_station_telemetry("pcs", crate::config::Role::Pcs, pts)
                        .await;
                }
            }
            Err(e) => {
                // 快照**立即失效**（单拍语义）—— 与迁移前"读失败即 None"逐字等价（设计 §13.4）
                self.set_snapshot(PcsSnapshot::default());
                let bad = self.inner.bad.fetch_add(1, Ordering::Relaxed) + 1;
                tracing::debug!(error = %e, bad, "PCS 3 区采集失败");
                if bad >= BAD_LIMIT {
                    let was_online = {
                        let mut c = self.inner.connected.write().await;
                        let prev = *c;
                        *c = false;
                        prev
                    };
                    // ★★ 判离线时**必须同时复位控制侧缓存**（= 迁移前 mark_offline 的语义）★★
                    // 理由（W1：迁移前 `intercore::transport::modbus` 的 `mark_offline`，
                    // 该模块 T4 删除）：断线期间 PCS **可能掉电/复位** ⇒ started/mode
                    // 缓存**不可信**；不复位则恢复后
                    // ① ensure_mode 见缓存命中 ⇒ 跳过 REG_MODE 重写；
                    // ② ensure_started 见 started==true ⇒ 命中缓存直接 Ok（连 S-4 的 1013 读都不发生）
                    // ⇒ 在**未知实际模式**下直接写 1006-1011 功率寄存器。这是 fail-open，必须复位。
                    *self.inner.started.write().await = false;
                    self.inner.mode.store(0xFF, Ordering::Relaxed);
                    if was_online {
                        self.inner
                            .sink
                            .on_station_offline("pcs", crate::config::Role::Pcs, &e.to_string())
                            .await;
                    }
                }
            }
        }
    }

    /// 启动采集循环（每 `interval_ms` 一拍；**不做退避** —— 与迁移前心跳的固定周期一致）。
    ///
    /// **`MissedTickBehavior` 契约（未披露项，2026-09-26 质量评审补记）**：本函数用裸
    /// `tokio::time::interval` ⇒ 取 tokio 缺省的 **`Burst`**：某拍耗时超过周期时，**连续
    /// 补跑多拍**（直到追上进度），可能挤住总线与 `inner.lock`（调度器侧的 `SouthScheduler`
    /// 另有其自身的节奏纪律）。同时**首次 `tick()` 立即完成**（不等一个周期）⇒ 启动后
    /// 立刻采一拍。两点均与迁移前 `run_heartbeat_loop`（同样裸 `interval`）**行为一致**，
    /// **不是回归** —— 但此前未披露，故在此写明。
    ///
    /// **不改行为**（保持与迁移前一致；改 `Delay`/`Skip` 属独立议题）。这一点由计划 Task 9
    /// 的用例 **E7** 钉住（E7 是唯一落点）。
    ///
    /// 返回的 `JoinHandle` 调用方**必须持有并观测**（task 内 panic 会静默终止采集，
    /// 与 `SouthScheduler::spawn` 同一观测契约）。
    pub fn spawn_collection_loop(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = Arc::clone(self);
        let period = me.inner.cfg.interval_ms.max(PCS_TICK_FLOOR_MS);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_millis(period));
            loop {
                ticker.tick().await;
                me.tick_once().await;
            }
        })
    }
}

/// 由 3 区块读数构造快照（纯函数，便于单测）。
///
/// `base` = 块起始地址（`pcs_3zone` 为 1000）；寄存器按 `addr - base` 索引。
/// 各段**独立** `Option`：域外读数为 `None`（保留迁移前的域校验 —— SOC ∈ [0,100]、
/// `RUN_STATE` ∈ 0..=3）；三相段须 3 字齐备（缺任一字 ⇒ 该段 `None`）。
///
/// ⚠️ **与 `regs::REG_*` 绝对地址常量的隐式耦合（改动前必读）**：本函数用**块基址 +
/// 偏移**索引，而下方各字段一律按 `regs::REG_SOC` / `REG_RUN_STATE` / `REG_I_A` /
/// `REG_P_A` / `REG_P_TOTAL` 这组**绝对地址**常量取数（`at(REG_SOC)` 之类）。故 `base`
/// **必须**是 `regs::REG_MODE`（= 1000，3 区窗口基址）且窗口须覆盖到 `REG_P_TOTAL` ——
/// 否则 `at()` 因 `addr < base` 或越界返 `None` ⇒ **五字段全 `None` 而 `valid` 仍 `true`**，
/// 且总线读成功 ⇒ `bad=0`、`connected=true`、**无任何日志**（现象："PCS 在线、点表有值，
/// 但 SOC/三相/运行态恒空"）。该形态现由配置期规则 **P-5**
/// （[`SouthPcsConfig::validate`]）fail-closed 拦截；`PcsHandle::new` 不校验配置 ⇒
/// 绕过 validate 直接构造仍会落入上述静默形态（本函数不设防、也不该设：它是纯解码）。
///
/// [`SouthPcsConfig::validate`]: crate::config::SouthPcsConfig::validate
fn build_snapshot(words: &[u16], base: u16, ts: std::time::Instant) -> PcsSnapshot {
    let at = |addr: u16| -> Option<u16> {
        if addr < base {
            return None;
        }
        words.get((addr - base) as usize).copied()
    };
    let arr3 = |addr: u16| -> Option<[f64; 3]> {
        Some([
            regs::from_pcs_reg(at(addr)?) * regs::SCALE_3PH,
            regs::from_pcs_reg(at(addr + 1)?) * regs::SCALE_3PH,
            regs::from_pcs_reg(at(addr + 2)?) * regs::SCALE_3PH,
        ])
    };
    PcsSnapshot {
        valid: true,
        ts: Some(ts),
        soc: at(regs::REG_SOC).and_then(decode_soc),
        run_state: at(regs::REG_RUN_STATE).and_then(decode_run_state),
        i_phase: arr3(regs::REG_I_A),
        p_phase: arr3(regs::REG_P_A),
        p_total: at(regs::REG_P_TOTAL).map(|w| regs::from_pcs_reg(w) * regs::SCALE_3PH),
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
    ///
    /// ⚠️ **写序偏离原文（登记，经评审判定不回退）**：本实现逐相**交错**写
    /// `P_i → Q_i`（线上序 1006,1009,1007,1010,1008,1011）；原文
    /// `intercore::transport::modbus::send_tai_command`（该模块 T4 删除）是**分组**写
    /// （P 三相 1006-1008 → Q 三相 1009-1011）。正常完成时末态相同 ⇒ **非功能回归**；
    /// 差异只在**中断残留态** —— 6 次 FC06 是独立事务，中途超时/CRC/异常真实可发生：
    ///
    /// - 原文中断：三相都有**新 P**、仅 A 相有新 Q ⇒ B/C = 新有功 + 旧无功
    ///   （**非预期功率因数**）
    /// - 交错中断：A 相完整更新、B/C 保持**上一周期自洽的 (P,Q) 对**
    ///
    /// 交错残留态的相位自洽优先 ⇒ **保留交错**。此偏离此前既未登记也无覆盖（两个方向的
    /// 写序都无法被任何探针判红）⇒ 现用
    /// `send_tai_command_writes_phase_regs_clamped_and_interleaved` 的写序断言钉住。
    /// 若将来要改回分组写序，须同时改该用例并复核上面的残留态论证。
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

/// M1 停机告警的**去抖决策**（**不依赖 `&self`**，便于直接单测；**有副作用：写 `warned`**）：
/// 仅当"RUN_STATE=0 且已下发运行 且 非 latch"**且**本拍是**首次**进入该状态时返回 `true`
/// （即"非停机 → 停机"**跃迁**告警一次）；其余情形返回 `false`。
///
/// **名称里的 `once` 即副作用**（2026-09-26 质量评审订正）：本函数**不是纯函数** —— 它
/// `swap`/`store` 入参 `warned`。签名刻意收 `&AtomicBool`（而非 `&mut bool`）并把写入
/// 留在函数内，是为保住 `swap` 的**原子读-改-写**（改成外部先读再写会让"两拍并发都判首次"
/// 的竞态重新打开）。改名 `warn_stopped_once` 让副作用进入名字，取代原首句"（纯函数…）"
/// 与事实矛盾的自称。
///
/// `warned` 是**跨拍记忆**（调用方持的 `AtomicBool`）：
/// - 进入告警态：`swap(true)` ⇒ 首次返回 `true`，其后同一段停机内恒 `false`（**去抖**）
/// - 离开告警态（含 latch / 非停机 / 未下发运行 / 域外 `None`）：复位为 `false`
///   ⇒ 下次跃迁仍能告警
///
/// ⚠️ 为什么必须有跨拍记忆：M1 语义下 `started` **刻意不复位**（见 `ensure_started` 注释），
/// 故"停机"条件一旦成立就**长期为真**；无记忆则逐拍告警 ⇒ `interval_ms` 周期无限刷屏
///（≈86400 条/日），正是迁移前注释点名要防的"停机期间每秒刷屏"。
fn warn_stopped_once(
    warned: &AtomicBool,
    run_state: Option<u16>,
    started: bool,
    latched: bool,
) -> bool {
    if run_state == Some(0) && started && !latched {
        !warned.swap(true, Ordering::Relaxed)
    } else {
        warned.store(false, Ordering::Relaxed);
        false
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
    ///
    /// ⚠️ **使用约束（钩子内不得取锁）**：本装饰器的钩子在**调用方仍持有
    /// `PcsInner::lock` 期间**执行（`send_*` 持总线锁跑完整条序列，`read_input` 只是其中
    /// 一步）。当前成立的前提是钩子只调 `restore_interlock_latched`（**不取总线锁**的纯
    /// 内存状态写入）。**一旦钩子内走任何取锁路径**（尤其调用会取 `PcsInner::lock` 的
    /// `PcsHandle` 方法，如 `stop()` / `send_*` / `ensure_*`），`#[tokio::test]` 默认
    /// current-thread 且**无超时** ⇒ 自锁**挂死**（不是失败、不是 panic）—— 排障时别往
    /// 逻辑断言上找。
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

    /// 正向断言（质量评审 Important）：`send_dual_param` 的功率寄存器写此前**零正向覆盖**
    /// —— grep 实测 `1000`(REG_MODE)/`1001`/`1002` 在测试里一次都没出现过（只有生产代码
    /// 引用）⇒ 模式字写错、设定值写错、乃至整条写序消失都不会红。
    ///
    /// 本用例钉住**整条写序**（值 + 先后）。判据值一律**同源编码** `to_pcs_reg`（含高/低 8 位
    /// 互换）：`to_pcs_reg(0.0)` = 0x0000、`to_pcs_reg(10.0)` = 0x0A00 —— **不是字面量**。
    #[tokio::test]
    async fn send_dual_param_writes_power_regs_in_order() {
        let bus = Arc::new(MockBus::new());
        // 前置：S-4 守卫会先 FC04 读 RUN_STATE(1013)；造"待机"使守卫放行（否则落 StoppedGuard）
        bus.put_input(1, 1013, vec![to_pcs_reg(1.0)]);
        let h = handle(bus.clone());
        h.send_dual_param(&PcsDualParam::new(10.0, 0.5, true, "intelligent"))
            .await
            .unwrap();
        assert_eq!(
            bus.write_calls.lock().unwrap().clone(),
            vec![
                // ① 首条指令必写模式字（`mode` 初值 0xFF 哨兵 ⇒ 缓存不可能命中）⇒ 恒功率 0
                (1, regs::REG_MODE, to_pcs_reg(regs::MODE_CONST_POWER as f64)),
                // ② 首条指令必写启停 ⇒ 运行
                (1, regs::REG_START_STOP, to_pcs_reg(1.0)),
                // ③ 恒功率有功设定 = p_ref（`k_droop`/`ai_ready`/`strategy_mode` 不入 PCS 点表）
                (1, regs::REG_CONST_P_SET, to_pcs_reg(10.0)),
                // ④ 恒功率无功设定恒 0（PCS 恒功率无下垂）
                (1, regs::REG_CONST_Q_SET, to_pcs_reg(0.0)),
            ],
            "恒功率写序与线上字（全部同源编码 to_pcs_reg，含字节互换）"
        );
    }

    /// 正向断言（质量评审 Important）：`send_tai_command` 此前**连快乐路径都没有**
    /// （原文也只从 latch 拒绝用例里碰过一次）⇒ `REG_MODE` 的分相写、`1006-1011` 六次写、
    /// `clamp_phase` 接线**全部零覆盖**。本用例钉住三者 + **写序**。
    ///
    /// 越界构造：`p = [30, -30, 0]` ⇒ 1006/1007/1008 的**线上字**须为
    /// `to_pcs_reg(±25)` / `to_pcs_reg(0)`（钳位发生在编码前）。
    ///
    /// ⚠️ 附带作用是 §2 的**可判红锚点**：`send_tai_command` 的写序是逐相**交错**
    /// （`P_i → Q_i`），与原文（P 三相后 Q 三相）不同（理由见该方法的 doc 注释）——
    /// 改回分组写序 ⇒ 本用例的写序专锚必红（判据是"P/Q 逐相配对相邻"而非"P_i 在 Q_i 之前"，
    /// 后者**判不出**分组，见 ① 处注释；该锚排在值断言之前，正是为了能单独观测）。
    #[tokio::test]
    async fn send_tai_command_writes_phase_regs_clamped_and_interleaved() {
        let bus = Arc::new(MockBus::new());
        bus.put_input(1, 1013, vec![to_pcs_reg(1.0)]);
        let h = handle(bus.clone());
        h.send_tai_command([30.0, -30.0, 0.0], [1.0, -2.0, 3.0], "fallback")
            .await
            .unwrap();

        let writes = bus.write_calls.lock().unwrap().clone();
        // ① 写序专锚：逐相**配对且相邻**（每相的 P 紧跟其后就是同相 Q，A→B→C），而非原文的
        //    "P 三相后 Q 三相"。置于值断言**之前**，让两条断言各有**独立**判别力：值改坏
        //    （如 clamp 拿掉）由 ② 抓（① 不受影响），顺序改回分组由本条抓。
        //
        //    ⚠️ 判据**不能**写成 `pos(P_i) < pos(Q_i)`（评审原建议的"1006 出现在 1009 之前"）：
        //    分组写序（1006,1007,1008,1009,1010,1011）**同样**满足它（所有 P 都在所有 Q 之前）
        //    ⇒ 那条断言对"交错 vs 分组"**零判别力**。实测已证：改成分组写序时它仍绿，只有 ②
        //    报红。故此处改判**相邻性** `pos(Q_i) == pos(P_i) + 1` —— 分组写序下 Q_A 远在
        //    P_A 之后 3 位，必红。
        let pos = |addr: u16| {
            writes
                .iter()
                .position(|&(_, a, _)| a == addr)
                .unwrap_or_else(|| panic!("写序中缺寄存器 {addr}（写序断言前提不成立）"))
        };
        for (ph, p_addr, q_addr) in [
            ("A", regs::REG_PHASE_P_A, regs::REG_PHASE_Q_A),
            ("B", regs::REG_PHASE_P_A + 1, regs::REG_PHASE_Q_A + 1),
            ("C", regs::REG_PHASE_P_A + 2, regs::REG_PHASE_Q_A + 2),
        ] {
            assert_eq!(
                pos(q_addr),
                pos(p_addr) + 1,
                "{ph} 相须**逐相配对**：Q({q_addr}) 紧邻其 P({p_addr}) 之后 —— \
                 原文 intercore 为 P 三相后 Q 三相；偏离理由见 send_tai_command doc。\
                 （只用 pos(P_i)<pos(Q_i) 判不出分组，故此处判相邻）"
            );
        }
        // ② 整条写序的**值**：模式字（分相 2）→ 启停 → 逐相交错 P/Q（越界已 clamp ±25）
        assert_eq!(
            writes,
            vec![
                (1, regs::REG_MODE, to_pcs_reg(regs::MODE_PHASE_SPLIT as f64)),
                (1, regs::REG_START_STOP, to_pcs_reg(1.0)),
                (1, regs::REG_PHASE_P_A, to_pcs_reg(25.0)), // 30 → clamp 25
                (1, regs::REG_PHASE_Q_A, to_pcs_reg(1.0)),
                (1, regs::REG_PHASE_P_A + 1, to_pcs_reg(-25.0)), // -30 → clamp -25
                (1, regs::REG_PHASE_Q_A + 1, to_pcs_reg(-2.0)),
                (1, regs::REG_PHASE_P_A + 2, to_pcs_reg(0.0)), // 0 → 原样
                (1, regs::REG_PHASE_Q_A + 2, to_pcs_reg(3.0)),
            ],
            "分相写序（逐相交错）与 clamp 后线上字（同源编码 to_pcs_reg）"
        );
    }

    #[test]
    fn stopped_warn_is_edge_triggered_not_per_tick() {
        // 判据：M1 停机告警必须**跃迁触发**（每段停机恰一次），而非逐拍。
        // 判别力：把实现改成 `run_state == Some(0) && started && !latched`（无记忆）
        // ⇒ 第二次调用会返回 true ⇒ 本用例必红。
        //
        // ⚠️ **2026-09-26 质量评审补判别力**：原版把"latch 臂复位""未下发运行臂复位"
        // "`None`（域外）臂复位"三条塞在同一串断言里，而相邻的**非停机**断言（它会复位记忆）
        // 恰好把前提冲掉 ⇒ 这三条复位**零独立判别力**；`run_state == None` 分支更是完全未测。
        // 下面按"先造 warned=true → 再调目标臂 → **紧接着**断言下一个跃迁仍能告警"的
        // 三段式补齐（每段的自检前提都在其上一行刚建立，故删掉对应复位**必红**）。
        let w = AtomicBool::new(false);
        // 前提：未下发运行 ⇒ 不告警（进入时 warned 本就是 false ⇒ 此断言只测"返回值"，不测复位）
        assert!(
            !warn_stopped_once(&w, Some(0), false, false),
            "未下发运行不得告警"
        );
        // 跃迁：停机 + 已下发运行 + 非 latch ⇒ 首次告警（同时把 warned 置 true）
        assert!(
            warn_stopped_once(&w, Some(0), true, false),
            "跃迁必须告警一次"
        );
        // 同一段停机内（相邻拍）⇒ 必须**不再**告警（去抖）
        assert!(
            !warn_stopped_once(&w, Some(0), true, false),
            "同一段停机内不得重复告警"
        );
        assert!(
            !warn_stopped_once(&w, Some(0), true, false),
            "第三拍同样不得告警"
        );

        // ── latch 臂的复位（独立判别力）────────────────────────────────
        // 第 ⑤ 步调用时 warned=true；**紧接着**再调非 latch 停机态：
        // 若 latch 臂没复位记忆 ⇒ swap(true) 见旧 true ⇒ 返回 false ⇒ 本断言必红。
        assert!(
            !warn_stopped_once(&w, Some(0), true, true),
            "latch 期间不得告警"
        );
        assert!(
            warn_stopped_once(&w, Some(0), true, false),
            "latch 臂必须复位记忆（否则 latch 解除后的首次停机告警被吞）"
        );

        // ── 非停机臂的复位 ─────────────────────────────────────────────
        // 前提：上一行刚把 warned 置 true。非停机 ⇒ 复位；下一拍停机须能再告警。
        assert!(!warn_stopped_once(&w, Some(1), true, false), "非停机不告警");
        assert!(
            warn_stopped_once(&w, Some(0), true, false),
            "非停机臂必须复位记忆（否则「恢复后再次停机」不再告警）"
        );

        // ── 未下发运行臂的复位 ─────────────────────────────────────────
        // 前提：上一行刚把 warned 置 true。
        assert!(
            !warn_stopped_once(&w, Some(0), false, false),
            "未下发运行不告警"
        );
        assert!(
            warn_stopped_once(&w, Some(0), true, false),
            "未下发运行臂必须复位记忆（否则重启后的首次停机不再告警）"
        );

        // ── `None`（域外/乱码/错位帧）臂的复位 ─────────────────────────
        // 前提：上一行刚把 warned 置 true。域外读数既不是"停机"、也不该保留旧记忆 ——
        // 否则一段乱码之后再真停机，告警会被静默吞掉。
        assert!(
            !warn_stopped_once(&w, None, true, false),
            "域外运行态不得告警"
        );
        assert!(
            warn_stopped_once(&w, Some(0), true, false),
            "None 臂必须复位记忆（否则乱码后的首次停机不再告警）"
        );
    }
}

/// 采集循环与快照（Task 8；设计 §13.3/§13.4、Δ-16/Δ-17/Δ-18）。
#[cfg(test)]
mod collection_tests {
    use super::*;
    use crate::port_runtime::MockBus;
    use async_trait::async_trait;
    use std::sync::Mutex as StdMutex;

    /// 记录 sink 收到的遥测批次与离线事件
    #[derive(Default)]
    struct RecSink {
        pub telemetry: StdMutex<Vec<Vec<(String, f64, bool)>>>,
        pub offline: StdMutex<Vec<String>>,
    }

    #[async_trait]
    impl StationSink for RecSink {
        async fn on_grid_package(&self, _pkg: mupc_data_processing::DataPackage) {}
        async fn on_station_telemetry(
            &self,
            _id: &str,
            _role: crate::config::Role,
            pts: Vec<(String, f64, bool)>,
        ) {
            self.telemetry.lock().unwrap().push(pts);
        }
        async fn on_battery_soc(&self, _id: &str, _soc: f64) {}
        async fn on_station_offline(&self, _id: &str, _role: crate::config::Role, reason: &str) {
            self.offline.lock().unwrap().push(reason.to_string());
        }
    }

    /// 造一块 76 寄存器的 3 区读数（按 PCS 线格式：已经 `to_pcs_reg` 的字节互换）。
    ///
    /// ⚠️ **量纲（与计划原文不同，以代码为准）**：3 区三相量纲 `regs::SCALE_3PH = 0.1`，
    /// 寄存器里是 **raw = 物理值 / 0.1**（125 → 12.5 A）；SOC(1010) / 运行态(1013) 量纲为 1，
    /// 直接给物理值。`to_pcs_reg` = `round() as i16` + 字节互换（**无小数位**）⇒ 照计划原文
    /// 写 `put(1022, 12.5)` 会被舍成 13、再 ×0.1 得 **1.3 A**，与期望值差 10 倍。
    fn block_words() -> Vec<u16> {
        let mut w = vec![0u16; 76];
        let put = |w: &mut Vec<u16>, addr: u16, v: f64| {
            w[(addr - 1000) as usize] = to_pcs_reg(v);
        };
        put(&mut w, 1010, 66.0); // SOC 66 %
        put(&mut w, 1013, 1.0); // 待机
        put(&mut w, 1022, 125.0); // 12.5 A
        put(&mut w, 1023, 135.0); // 13.5 A
        put(&mut w, 1024, 145.0); // 14.5 A
        put(&mut w, 1029, 10.0); // 1.0 kW
        put(&mut w, 1030, 20.0); // 2.0 kW
        put(&mut w, 1031, 30.0); // 3.0 kW
        put(&mut w, 1032, 60.0); // 总 6.0 kW
        w
    }

    /// 生产形态块配置：`pcs_3zone`（76 寄存器 / **72 点** `points`）。
    ///
    /// **点表真源 = `tests/fixtures/south_pcs_s3b2.yaml`**（部署 `south_pcs` 段的逐字转载，
    /// PRD §9.8.3 的 72 点口径即出自它）—— 此处 `include_str!` 复用，**不二次转录**
    /// （转录两份必然漂移；`config.rs::south_pcs_tests::pcs_block` 的注释就是被这个坑咬过
    /// 的活体证据）。
    ///
    /// ⚠️ **为什么不照计划原文写 `points = [{at: 1, count: 6}]`**：`points::expand` 按点级
    /// `count` 展开 ⇒ 只产 **6** 点，且该形态在规则 11（首尾锚定）下是**必被拒的不真实形态**
    /// ⇒ 下面的 `total == 72` 断言无从成立（恒红）。故本 helper 取真实点表。
    fn cfg_with_points() -> SouthPcsConfig {
        let mut c = super::control_tests::cfg_for_tests();
        let prod: SouthPcsConfig =
            serde_yaml::from_str(include_str!("../../tests/fixtures/south_pcs_s3b2.yaml"))
                .expect("fixture south_pcs_s3b2.yaml 须可反序列化为 SouthPcsConfig");
        assert_eq!(prod.regs.len(), 1, "fixture 须恰含 pcs_3zone 一块");
        assert_eq!(prod.regs[0].count, 76, "pcs_3zone 须为 76 寄存器窗口");
        c.regs = prod.regs;
        c
    }

    #[tokio::test]
    async fn one_tick_reads_block_once_and_serves_all_derived_views() {
        // Δ-16/Δ-18 核心判据：**一次总线读**同时产出 SOC / 运行态 / 三相 / 遥测点。
        // 判别力：把采集循环改成"分三次读"（迁移前口径）⇒ 本断言必红。
        let bus = Arc::new(MockBus::new());
        bus.put_input(1, 1000, block_words());
        let sink = Arc::new(RecSink::default());
        let h = PcsHandle::new(cfg_with_points(), bus.clone(), sink.clone());

        h.tick_once().await;

        assert_eq!(bus.input_call_count(1, 1000), 1, "每拍只允许一次 3 区读");
        assert_eq!(
            bus.input_call_count(1, 1010),
            0,
            "SOC 不得单独读（已并入块）"
        );
        assert_eq!(
            bus.input_call_count(1, 1013),
            0,
            "运行态不得单独读（已并入块）"
        );
        assert_eq!(h.latest_soc().await.map(|(v, _)| v), Some(66.0));
        assert_eq!(h.last_run_state(), Some(1));
        let tp = h.read_three_phase().await.expect("三相必须有值");
        assert_eq!(tp.i_phase, Some([12.5, 13.5, 14.5]));
        assert_eq!(tp.p_phase, Some([1.0, 2.0, 3.0]));
        assert_eq!(tp.p_total, Some(6.0));
        let batches = sink.telemetry.lock().unwrap();
        let total: usize = batches.iter().map(|b| b.len()).sum();
        assert_eq!(total, 72, "pcs_3zone 展开应为 72 点（PRD §9.8.3）");
    }

    #[tokio::test]
    async fn failure_invalidates_snapshot_immediately_but_offline_needs_three() {
        // 设计 §13.4 的注：'快照失效'（单拍）与'在线态'（累积 3）是两个独立判据。
        //
        // ⚠️ **必须先跑一拍成功建立在线基线**（计划原文没有这一步）：`connected` 初值
        // `false` ⇒ 不在线的句柄上"1 拍失败仍在线"是**恒真**的空断言（原始版本实测红在
        // 此处，也正是这条断言把该缺陷揪出来的）。
        let bus = Arc::new(MockBus::new());
        bus.put_input(1, 1000, block_words());
        let sink = Arc::new(RecSink::default());
        let h = PcsHandle::new(cfg_with_points(), bus.clone(), sink.clone());
        h.tick_once().await;
        assert!(h.is_connected().await, "基线：成功一拍 ⇒ 在线");
        assert!(h.latest_soc().await.is_some(), "基线：快照有效");
        let telem_after_ok = sink.telemetry.lock().unwrap().len();
        assert_eq!(telem_after_ok, 1, "基线：成功一拍恰投 1 批遥测");

        bus.fail_input_once(1, 1000);
        h.tick_once().await;
        assert!(h.latest_soc().await.is_none(), "单拍失败 ⇒ 快照失效 ⇒ None");
        assert!(h.read_three_phase().await.is_none());
        assert_eq!(h.last_run_state(), None);
        assert!(
            h.is_connected().await,
            "1 拍失败不得判离线（对齐既有心跳 3 拍口径）"
        );
        assert!(sink.offline.lock().unwrap().is_empty());
        // 失败拍**不得投遥测**：`telemetry` 的长度须恒等于"成功拍数"。
        // 判别力：让 Err 臂也走一遍上送 ⇒ 下面两条断言必红（失败读数进遥测 = 假数据）。
        assert_eq!(
            sink.telemetry.lock().unwrap().len(),
            telem_after_ok,
            "失败拍不得投遥测（1 拍失败后仍应恰 {} 批）",
            telem_after_ok
        );

        bus.fail_input_once(1, 1000);
        h.tick_once().await;
        assert!(h.is_connected().await, "2 拍仍不得判离线");
        assert_eq!(
            sink.telemetry.lock().unwrap().len(),
            telem_after_ok,
            "失败拍不得投遥测（2 拍失败后仍应恰 {} 批）",
            telem_after_ok
        );

        bus.fail_input_once(1, 1000);
        h.tick_once().await;
        assert!(!h.is_connected().await, "连续 3 拍失败 ⇒ 离线");
        assert_eq!(sink.offline.lock().unwrap().len(), 1, "离线事件恰一次");
        assert_eq!(
            sink.telemetry.lock().unwrap().len(),
            telem_after_ok,
            "失败拍不得投遥测（3 拍失败后仍应恰 {} 批）",
            telem_after_ok
        );

        // 已离线后**续拍**再失败：离线事件**不得重复投**（判据是 `was_online` 跃迁，
        // 不是"每拍失败即投"）。原用例只测到第 3 拍 ⇒ 该契约零覆盖。
        for _ in 0..2 {
            bus.fail_input_once(1, 1000);
            h.tick_once().await;
        }
        assert!(!h.is_connected().await, "续拍失败仍离线");
        assert_eq!(
            sink.offline.lock().unwrap().len(),
            1,
            "离线后连续失败不得重复投离线事件"
        );
        assert_eq!(
            sink.telemetry.lock().unwrap().len(),
            telem_after_ok,
            "离线段同样不得投遥测"
        );
    }

    #[tokio::test]
    async fn recovery_after_failure_restores_snapshot() {
        // 判离线（3 拍失败）之后，读恢复 ⇒ **回在线** + 快照恢复。
        let bus = Arc::new(MockBus::new());
        bus.put_input(1, 1000, block_words());
        let sink = Arc::new(RecSink::default());
        let h = PcsHandle::new(cfg_with_points(), bus.clone(), sink);

        h.tick_once().await; // 基线：在线
        assert!(h.is_connected().await && h.latest_soc().await.is_some());

        for _ in 0..3 {
            bus.fail_input_once(1, 1000);
            h.tick_once().await;
        }
        assert!(!h.is_connected().await, "连续 3 拍失败 ⇒ 离线");
        assert!(h.latest_soc().await.is_none(), "离线期间快照失效");

        h.tick_once().await; // 下一拍成功（put_input 仍在）⇒ 回在线 + 快照恢复
        assert!(h.is_connected().await, "读成功即回在线");
        assert_eq!(h.latest_soc().await.map(|(v, _)| v), Some(66.0));
    }

    #[tokio::test]
    async fn offline_transition_resets_control_caches() {
        // 设计 Δ-16 落地评审的查出项（W1 语义）：判离线时**必须**复位 started + mode 哨兵。
        // 理由：断线期间 PCS 可能掉电/复位 ⇒ 缓存不可信；不复位则恢复后 ensure_mode 跳过
        // REG_MODE 重写、ensure_started 命中缓存直接 Ok（连 S-4 的 1013 读都不发生）
        // ⇒ 在未知实际模式下写功率寄存器（fail-open）。
        let bus = Arc::new(MockBus::new());
        bus.put_input(1, 1013, vec![to_pcs_reg(1.0)]);
        bus.put_input(1, 1000, block_words());
        let sink = Arc::new(RecSink::default());
        let h = PcsHandle::new(cfg_with_points(), bus.clone(), sink);
        // 先建立"已下发模式 + 已启动"
        h.send_tai_command([1.0, 2.0, 3.0], [0.0, 0.0, 0.0], "fallback")
            .await
            .unwrap();
        assert!(h.debug_started().await, "前提：已启动");
        assert_eq!(
            h.debug_mode(),
            regs::MODE_PHASE_SPLIT as u8,
            "前提：已下发分相模式"
        );
        // 再跑一拍成功 ⇒ 在线基线（否则"已判离线"前提同上恒真）
        h.tick_once().await;
        assert!(h.is_connected().await, "前提：在线");
        assert!(h.debug_started().await, "前提：成功拍不动控制侧缓存");
        // 连续 3 拍失败 ⇒ 判离线
        for _ in 0..3 {
            bus.fail_input_once(1, 1000);
            h.tick_once().await;
        }
        assert!(!h.is_connected().await, "前提：已判离线");
        assert!(
            !h.debug_started().await,
            "判离线必须复位 started（否则恢复后会跳过 S-4 读）"
        );
        assert_eq!(
            h.debug_mode(),
            0xFF,
            "判离线必须复位 mode 哨兵（否则会跳过 REG_MODE 重写）"
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
