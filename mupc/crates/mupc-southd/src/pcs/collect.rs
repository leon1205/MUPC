//! PCS 采集面：采集循环、快照与四个按需读 getter（设计 §13.3/§13.4、Δ-16/Δ-17/Δ-18）。
//!
//! **拆分依据（2026-09-26 Task 8 质量评审）**：`pcs/mod.rs` 总行数越 1200 行的拆分触发线
//! （生产码未越 700），且 Task 9 还要往 [`collection_tests`] 增补用例 ⇒ 先拆再增补。
//!
//! **机制**：本模块是 `pcs` 的**子模块**，Rust 隐私规则下子模块可见祖先模块的私有项 ⇒
//! 可直接访问 [`PcsHandle::inner`]（私有字段）与 `PcsInner` 的私有字段，
//! **无需任何 `pub(crate)` 扩权、无 API 变化**。类型、控制面、`decode_*` 仍留在 `pcs/mod.rs`。
//!
//! [`PcsHandle::inner`]: super::PcsHandle

use super::*;

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
/// ⚠️ **可见性（拆分时唯一被迫的偏离，2026-09-26 登记）**：本函数随采集面沉入 `collect`，
/// 而它的**判别力用例** `control_tests::stopped_warn_is_edge_triggered_not_per_tick` 仍在
/// `control_tests`（控制面，不在本次搬迁清单内）。兄弟模块之间**互相看不到私有项**
/// （隐私规则只让子模块看祖先），故此处从 `fn` 放宽为 `pub(super)`（= `pub(in crate::pcs)`）。
/// 这只是模块内可见性、**不是对外 API**：`pcs` 之外不可见，`mupc-southd` 的对外契约不变。
pub(super) fn warn_stopped_once(
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
        let mut c = crate::pcs::control_tests::cfg_for_tests();
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
