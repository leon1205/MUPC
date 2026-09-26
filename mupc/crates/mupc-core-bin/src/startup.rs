//! 子系统启动编排器
//!
//! 按照依赖关系顺序初始化 14 个子系统。
//! 初始化顺序由 Section 3.3 设计文档定义。
//!
//! Phase 2 实现: 06/08/09/10 子系统已完成基础初始化。
//! Phase 2+ 实现: 11 OTA 管理器已完成初始化。
//! 剩余 6 个 TODO 见代码注释。

// ── 协作式停机（U-64，2026-09-23）─────────────────────────────────────────────────
//
// 背景（审查警告 W3）：`abort()` **丢弃 future**、且要到该任务的下一个 await 点才生效 ⇒
// "先 abort 全部后台任务、再 flush"会在两个地方丢点：① 被 abort 的任务若正卡在
// `commit_batch` 的 await 上，已 drain 出缓冲的那一批随 future 消失（连 `Err` 分支都走不到）；
// ② 生产者在 flush 之后仍可能推新点。本节的次序把这两条都堵住。

/// 协作式停机信号（生产者侧只读端）。
///
/// 用 tokio 的 `watch` 通道（**不新增依赖**：tokio 已在依赖表内且 `features=["full"]`）。
/// 生产者把它 `select!` 进自己的调度循环，收到即**收工**（在途工作做完再返回）。
#[derive(Clone)]
pub struct StopSignal {
    rx: tokio::sync::watch::Receiver<bool>,
}

impl StopSignal {
    /// 供生产者 `select!`：停机信号已发出（或发送端已 drop）即就绪。
    ///
    /// 先查当前值再等 `changed()`：信号若在生产者进入 `select!` **之前**就已发出，
    /// 只等 `changed()` 会永远等不到"下一次变化"（这是 watch 的语义），必须补这一步。
    pub async fn stopped(&mut self) {
        if *self.rx.borrow() {
            return;
        }
        let _ = self.rx.changed().await;
    }
}

/// 协作任务退出等待上限。取值理由：协作生产者是 pv/load 南向模拟环（每轮 `sleep(1s)` + 两次
/// 串口读，一轮 ≤ 约 2 s）与定时 flush 任务（提交在毫秒级）⇒ 3 s 覆盖"一整轮 + 一次提交"，
/// 又远小于 `main.rs` 对整段优雅退出的 30 s 总闸（`system.shutdown_timeout_sec`）。
pub(crate) const COOPERATIVE_EXIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// **优雅停机的第一步：停生产者**（U-64）——从 `StartupContext::shutdown` 抽出的**可测接缝**
/// （同 `console_write_paths` 的手法与理由：真环境起不来 ⇒ 把裁决点抽出来单测）。
///
/// 次序（**不得调换**）：
/// 1. `stop_tx.send(true)`：通知协作生产者收工 —— 它们 `select!` 到信号即退出，**在途的
///    最后一次写入得以完成**（这正是"先 abort 再 flush"丢掉的那一步）；
/// 2. abort **非协作**任务：southd 采集口 / 网关 / MQTT / 指标采集等**没有退出钩子**
///    （`mupc-southd::scheduler::spawn` 是独立 crate 的常驻 poll 循环，本轮不跨 crate 加停机
///    协议）⇒ 只能 abort。它们的在途批次不会因此消失：`storage` 的 `BatchGuard` 在 future
///    被丢弃时把"已 drain、未提交"的点回填缓冲（见 `WriteBuffer::flush_batch` 的三层保护）；
/// 3. **有上限地**等协作任务**确认退出**：超时即如实告警并 abort 该任务（不无限 hang）；
/// 4. 返回 —— 调用方随后做**最后一次 flush**（此刻最后一个写者已停，这才是真正的"最后一批"）。
pub(crate) async fn stop_producers(
    stop_tx: &tokio::sync::watch::Sender<bool>,
    abort_tasks: &[tokio::task::JoinHandle<()>],
    cooperative_tasks: Vec<(&'static str, tokio::task::JoinHandle<()>)>,
    cooperative_timeout: std::time::Duration,
) {
    // 1) 通知收工
    if stop_tx.send(true).is_err() {
        // 接收端全 drop 了 ⇒ 没有协作生产者在场（信号已无意义），照常往下走。
        tracing::debug!("停机信号无接收者（协作生产者均已退出）");
    }
    // 2) 非协作任务：abort 生效点在下一次 await；其未提交批次由 BatchGuard 回填兜底
    tracing::info!(
        "优雅退出：abort {} 个无停机钩子的后台任务",
        abort_tasks.len()
    );
    for handle in abort_tasks {
        handle.abort();
    }
    // 3) 等协作任务确认退出（总预算 cooperative_timeout）
    let deadline = tokio::time::Instant::now() + cooperative_timeout;
    for (label, handle) in cooperative_tasks {
        // 先取 abort 句柄：`timeout_at` 会**消费** handle，超时分支就必须靠它收尾。
        let abort = handle.abort_handle();
        match tokio::time::timeout_at(deadline, handle).await {
            Ok(Ok(())) => tracing::info!(task = label, "协作任务已确认收工"),
            Ok(Err(e)) if e.is_panic() => tracing::error!(
                task = label,
                "协作任务 panic 退出（其未提交批次由 BatchGuard 回填，仍在缓冲里待落盘）"
            ),
            Ok(Err(_)) => tracing::debug!(task = label, "协作任务已被取消"),
            Err(_) => {
                // 超时：不能死等（用户/运维等着进程退出），如实告警后 abort，继续进 flush。
                abort.abort();
                tracing::warn!(
                    task = label,
                    timeout_ms = cooperative_timeout.as_millis(),
                    "协作任务未在上限内确认收工 ⇒ 已 abort；其已 drain 未提交的批次由 BatchGuard 回填，\
                     仍会随随后那次 flush 落盘（超时属异常路径，如实记录）"
                );
            }
        }
    }
}

use device_trait::plugin_loader::PluginLoader;
use device_trait::Device;
use mupc_common::{ErrorCode, MupcError};
use mupc_core::service_coord::ServiceStatus;
use mupc_core::service_coord_impl::ServiceCoordinatorImpl;
use mupc_data_processing::latest_values::{LatestValues, PointId, PointQuality, PointValue};
use mupc_system_monitor::MetricCollector;
use std::collections::HashMap;
use std::sync::Arc;

use crate::core_config::CoreConfig;

/// 启动上下文：持有所有已初始化的子系统句柄
///
/// 各子系统以 Arc 形式存放。
/// 在 Phase 6 优雅退出时用于逆序清理。
#[allow(dead_code)]
pub struct StartupContext {
    pub message_bus: Arc<mupc_core::TokioMessageBus>,
    pub storage: Arc<mupc_storage::StorageService>,
    pub intercore: Arc<mupc_intercore::IntercoreClient>,
    pub plugin_loader: Arc<plugin_loader::PluginLoaderImpl>,
    pub ai_engine: Arc<mupc_ai_engine::ModelManager>,
    pub ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
    pub ota_manager: Arc<dyn mupc_ota_update::OtaManager>,
    /// 故障录波器（保留实例供录波使用）
    pub fault_recorder: Arc<mupc_data_processing::FaultRecorderImpl>,
    /// 遥测写缓冲（P0-1：**退出前必须落盘**的最后一批数据在这里）。
    ///
    /// 持有方式是 `Arc`（**多持有者共享同一缓冲区**，不是各持一份拷贝）：`SouthSink`、
    /// pv/load 南向模拟循环、定时 flush 任务各持一个 `Arc` 克隆，缓冲本体（`Mutex<Vec<…>>`）
    /// 只有一份 ⇒ 退出路径拿到的是**同一个**缓冲，`flush()` 排空的就是生产者刚写进去的那批。
    pub write_buffer: Arc<mupc_storage::WriteBuffer>,
    /// 后台任务句柄（Phase 6 优雅退出时 abort）
    pub background_tasks: Vec<tokio::task::JoinHandle<()>>,
    /// **协作式退出**的生产者句柄（U-64）：`shutdown()` **先通知它们收工并等确认**，
    /// 而不是 abort —— 定时 flush 任务与 pv/load 南向模拟环都在这里。
    ///
    /// 与 `background_tasks` 分开登记是**刻意的**：混在一起就退化成"全部 abort"（U-64 缺陷本身）。
    pub cooperative_tasks: Vec<(&'static str, tokio::task::JoinHandle<()>)>,
    /// 协作式停机信号的**发送端**（U-64）：`shutdown()` 第一步置 true。
    stop_tx: tokio::sync::watch::Sender<bool>,
}

impl StartupContext {
    /// 优雅退出（U-64 重做，2026-09-23）：**通知生产者收工 → 等它们确认退出（有上限）→
    /// 最后 flush**。
    ///
    /// **为什么不再是"先 abort 再 flush"**（原 P0-1 的次序，其问题已被审查 W3 定位）：
    /// `abort()` 是**丢弃 future**，要到任务的下一个 await 点才生效 ⇒ 任务若恰好停在
    /// `commit_batch` 的 await 上，它**已经 drain 出缓冲的那一批**随 future 一起消失，
    /// 连 `flush_batch` 的 `Err` 分支都走不到（无日志、无计数）。改成"通知 → 等确认"后，
    /// 生产者是在**自己的循环里**看到信号才退出的，在途工作（含那次提交）会正常走完。
    ///
    /// **为什么还必须 flush 在最后**（P0-1 的原始理由仍然成立）：若先 flush 再停生产者，
    /// 两次调用之间生产者仍可能写入新点，那批新点就落在 flush 之后、随进程退出滞留内存。
    ///
    /// **为什么放在这里而不是 `main.rs`**：本方法是**唯一**的优雅退出入口（`main.rs` 的
    /// `graceful_shutdown` 调用）。放在这里 ⇒ "忘了 flush"在结构上不可能发生，而不是靠调用方
    /// 记得多打一行；`main.rs` 也就无需知道 `WriteBuffer` 的存在。
    ///
    /// **残留窗口（如实登记，不夸大）**：无停机钩子的任务（southd 采集口等）仍靠 `abort`，
    /// 其 abort 生效点在下一次 await ⇒ 它们可能在本方法返回后才真正停下；若它们在最后一次
    /// flush **之后**又 push 点位，那批点仍会滞留内存。与修复前的区别是：① 已 drain 未提交的
    /// 批次**不再丢**（`BatchGuard` 回填）；② 这个窗口从"每个任务都可能踩"缩小到"仅无钩子任务
    /// 的极端时序"。协作任务（定时 flush、pv/load 环）**已无此窗口**。
    pub async fn shutdown(&mut self) {
        tracing::info!(
            "优雅退出：{} 个协作任务（通知收工并等确认）+ {} 个无钩子任务（abort）",
            self.cooperative_tasks.len(),
            self.background_tasks.len()
        );
        stop_producers(
            &self.stop_tx,
            &self.background_tasks,
            std::mem::take(&mut self.cooperative_tasks),
            COOPERATIVE_EXIT_TIMEOUT,
        )
        .await;
        self.flush_telemetry_buffer().await;
    }

    /// 落盘遥测缓冲中**剩余的全部**数据（含不足一批的），供退出路径使用。
    pub async fn flush_telemetry_buffer(&self) {
        match self.write_buffer.flush().await {
            Ok(0) => tracing::info!("优雅退出：遥测缓冲已空，无需落盘"),
            Ok(n) => tracing::info!(points = n, "优雅退出：最后一批遥测已落盘"),
            // 失败语义与运行期同源（U-68③：**回填**缓冲、不丢弃）；此处**必须响亮** —— 这是
            // 进程最后一次落盘机会。⚠️ 如实说清后果：数据还在内存里，但进程随即退出 ⇒ 实为丢失
            // （不得说成"已丢弃"——那会掩盖"若下一拍能重试就能救回"的事实；也不必说成"已保全"）。
            Err(e) => tracing::error!(
                error = %e,
                dropped_total = self.write_buffer.dropped_points(),
                "优雅退出：最后一批遥测落盘失败（批次已回填内存缓冲，但进程即将退出 ⇒ 该批数据仍会丢失；\
                 丢弃计数见 dropped_total）"
            ),
        }
    }
}

/// IEC 104 命令处理器：转发主站控制命令到实时控制模块（PCS）
struct StrategyCommandHandler {
    /// PCS 通道（`south_pcs.enabled=false` ⇒ `None` ⇒ 主站指令**明确不下发**并告警，
    /// 不静默吞掉）。
    pcs: Option<Arc<mupc_southd::pcs::PcsHandle>>,
    /// 安全联锁控制器（io.enabled 时注入；latch 期间抑制主站下发，避免绕过联锁启停 PCS）
    interlock: Option<Arc<crate::interlock::InterlockController>>,
    /// 额定有功上限 (kW)：IEC104 主站外部指令 p_set clamp 用（审查 R1-A1，2026-09-09），
    /// 来源台区储能容量档 p_cap（电池功率上限，YAML 真值）。
    p_max_kw: f64,
    /// **总召 / 连接初始快照数据源**（01 设计 §9.4 序 6）：最新值快照 + 上送点表。
    /// `on_interrogation` 从 `latest.all()` 过滤 `channels.has(IEC104) && is_fresh(..)` 转
    /// `TelemetryItem`（无有效值的点不出现，GI-3）；`on_connection_snapshot` 走 trait 默认
    /// 实现转发到 `on_interrogation`（**一个实现两处调用**，§9.2.5）。
    latest: Arc<LatestValues>,
    points: Arc<Vec<mupc_southd::uplink::UplinkPoint>>,
}

impl StrategyCommandHandler {
    /// 联锁锁存时是否抑制本次控制下发（读共享 state，同步）
    fn interlock_blocks(&self) -> bool {
        self.interlock.as_ref().is_some_and(|c| c.is_latched_now())
    }

    fn name(&self) -> &str {
        "strategy-command-handler"
    }
}

#[async_trait::async_trait]
impl mupc_gateway::iec104::command::CommandHandler for StrategyCommandHandler {
    fn name(&self) -> &str {
        StrategyCommandHandler::name(self)
    }

    async fn handle_command(
        &self,
        cmd: mupc_gateway::iec104::command::ControlCommand,
    ) -> Result<mupc_gateway::iec104::command::CommandResponse, MupcError> {
        match cmd.cmd_type {
            mupc_gateway::iec104::command::CommandType::PowerRegulation
            | mupc_gateway::iec104::command::CommandType::ChargeDischarge => {
                // Task7：联锁锁存期间抑制主站功率/启停下发（双参下发路径同样挡，防绕过联锁启停）
                if self.interlock_blocks() {
                    tracing::warn!(
                        "IEC104 命令被安全联锁抑制（latch 中）: cmd_id={}",
                        cmd.cmd_id
                    );
                    return Ok(mupc_gateway::iec104::command::CommandResponse {
                        cmd_id: cmd.cmd_id,
                        success: false,
                        message: "安全联锁锁存中，禁止下发".into(),
                        timestamp: chrono::Utc::now().timestamp() as u64,
                    });
                }
                // p_set → 下发到实时控制模块（DualParamCommand: p_ref + k_droop）
                if let Some(p_set) = cmd.p_set {
                    // 外部指令防护（审查 R1-A1，2026-09-09）：p_set 有限性 + 额定限幅——外部主站
                    // 指令不再无界直通 PCS 1001（原越界仅靠 i16 as 饱和截断静默失真）。
                    let p_raw = p_set;
                    if !p_raw.is_finite() {
                        tracing::warn!("IEC104 p_set 非有限，拒绝下发: {}", p_raw);
                        return Ok(mupc_gateway::iec104::command::CommandResponse {
                            cmd_id: cmd.cmd_id,
                            success: false,
                            message: "p_set 非有限，拒绝下发".into(),
                            timestamp: chrono::Utc::now().timestamp() as u64,
                        });
                    }
                    let p_set = p_raw.clamp(-self.p_max_kw, self.p_max_kw);
                    if (p_set - p_raw).abs() > 1e-6 {
                        tracing::warn!(
                            "IEC104 p_set 超出额定限幅，clamp 至 {}（原 {}）",
                            p_set,
                            p_raw
                        );
                    }
                    if let Some(pcs) = &self.pcs {
                        let dual = mupc_southd::pcs::PcsDualParam::new(
                            p_set,
                            cmd.k_value.unwrap_or(0.0),
                            true,
                            "intelligent",
                        );
                        pcs.send_dual_param(&dual).await.map_err(|e| {
                            MupcError::new(
                                ErrorCode::SendFailed,
                                format!("PCS 下发失败: {e}"),
                                "startup",
                            )
                        })?;
                    } else {
                        // 无 PCS 通道 ⇒ 指令**不可能**到达执行端：如实回失败，不谎报"命令已下发"
                        tracing::warn!(
                            "south_pcs.enabled=false ⇒ IEC104 p_set={p_set} 无法下发（无 PCS 通道）"
                        );
                        return Ok(mupc_gateway::iec104::command::CommandResponse {
                            cmd_id: cmd.cmd_id,
                            success: false,
                            message: "无 PCS 通道（south_pcs.enabled=false），指令未下发".into(),
                            timestamp: chrono::Utc::now().timestamp() as u64,
                        });
                    }
                }
            }
            mupc_gateway::iec104::command::CommandType::SwitchControl => {
                // 开关控制：记录（南向开关下发路径 Phase 2+）
                tracing::info!(
                    "开关控制命令: cmd_id={}, switch_state={:?}",
                    cmd.cmd_id,
                    cmd.switch_state
                );
            }
        }
        Ok(mupc_gateway::iec104::command::CommandResponse {
            cmd_id: cmd.cmd_id,
            success: true,
            message: "命令已下发".into(),
            timestamp: chrono::Utc::now().timestamp() as u64,
        })
    }

    /// **站召唤（总召 `C_IC_NA_1`）数据源**（01 设计 §9.2.5 / §9.4 序 6）。
    ///
    /// 从最新值快照取全部 `channels.has(IEC104) && is_fresh(..)` 的点转 `TelemetryItem`
    /// （`cot = COT_INTROGEN(20)`）；**无有效值的点不出现**（GI-3：不得以 0 或旧值顶替）。
    /// 连接初始快照 `on_connection_snapshot` 走 trait 默认实现**转发到本方法**
    /// （一个实现两处调用，防两套口径）。取数/编码在 `crate::uplink::interrogation_items`。
    async fn on_interrogation(&self) -> Vec<mupc_gateway::iec104::command::TelemetryItem> {
        let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        crate::uplink::interrogation_items(&self.latest, &self.points, now_ms)
    }
}

/// `south_pcs` 段 → 口打开所需的最小 `StationConf`（**仅**给 `Rs485PortBus::open` 用）。
///
/// **为什么不直接复用 [`mupc_southd::config::SouthPcsConfig::station_shell`]**：那个函数是
/// **上云站壳**（`regs` 必须随壳一起走，供点表生成用）；而 `open` 只看**口层**字段
/// （port/baud_rate/parity），带上 `regs` 只会让"配置 × 上云"两条路径在读取点表时被误接。
/// 故此处把 `regs` 置空、role 记 `Pcs`（只为日志可读），**且本函数不参与任何配置校验路径**
/// （P-2/P-3/P-4/P-5 全在 `CoreConfig::validate` 与 `SouthPcsConfig::validate`）。
///
/// **口层控制面三项（超时/数据位/停止位）不在这里**：`StationConf` 无对应字段，故单列
/// [`south_pcs_port_params`] 交给 `open_with_port_params`（Task 10 评审项 2 —— 这三项曾因
/// 只有站壳路径而**全部不生效**）。
fn south_pcs_bus_conf(
    cfg: &mupc_southd::config::SouthPcsConfig,
) -> mupc_southd::config::StationConf {
    mupc_southd::config::StationConf {
        id: "pcs".into(),
        role: mupc_southd::config::Role::Pcs,
        port: cfg.port.clone(),
        protocol: cfg.protocol.clone(),
        slave: cfg.slave,
        baud_rate: cfg.baud_rate,
        parity: cfg.parity,
        interval_ms: cfg.interval_ms,
        regs: Vec::new(),
    }
}

/// `south_pcs` 段的**口层控制面参数** → [`mupc_southd::port_runtime::PortParams`]
/// （**生产调用形态的唯一收敛点**）。
///
/// 三项的落点：`response_timeout_ms` → `Rs485Device` 的 `Config.timeout_ms`
/// （迁移前由 `intercore.modbus_rtu.response_timeout_ms` 提供，缺省 200ms ⇒ 若不接线就
/// 静默变 1000ms，`stop()` 这类安全动作的失败检测随之变慢）；`data_bits`/`stop_bits`
/// → 同结构体的对应字段。
///
/// 回归网：`task10_south_pcs_port_params_read_all_three_control_fields`
/// （把本函数改成 `PortParams::default()` ⇒ 该用例红）。
fn south_pcs_port_params(
    cfg: &mupc_southd::config::SouthPcsConfig,
) -> mupc_southd::port_runtime::PortParams {
    mupc_southd::port_runtime::PortParams {
        timeout_ms: cfg.response_timeout_ms,
        data_bits: cfg.data_bits,
        stop_bits: cfg.stop_bits,
    }
}

/// MQTT 上送的角色表（**Task 10 第 7 接线点的唯一收敛处**，设计 §9.3 / §13.9）。
///
/// 站级段（`south_stations`，5 站）**不含** PCS 站 —— PCS 已按 ADR-016 迁到顶层段
/// `south_pcs`，其站壳由 [`mupc_southd::config::SouthPcsConfig::station_shell`] 合成
/// ⇒ 此处**必须**传 `Some(&config.south_pcs)`：退化为 `None` 时
/// `uplink::plan_stations` 查不到 `"pcs"`，MQTT 遥测/事件载荷的 `role` 会落**空串**
/// （`roles.get(..).unwrap_or_default()`）——**静默降级**，无日志、无告警。
///
/// 回归网：`task10_station_roles_wiring_carries_pcs_role_into_publish_plan`
/// （把本函数的 `Some` 改成 `None` ⇒ 该用例红；改写前把它改回 `None` 则全绿，
/// 因为既有用例都是自己带上 `Some` 直接调 `uplink::station_roles`，绕开了本生产调用点）。
fn mqtt_station_roles(
    config: &CoreConfig,
) -> std::sync::Arc<std::collections::HashMap<String, String>> {
    std::sync::Arc::new(crate::uplink::station_roles(
        &config.south_stations,
        Some(&config.south_pcs),
    ))
}

/// 创建并打开一个 RS485 南向设备；无硬件（串口不存在）时返回 None
fn create_rs485_device(
    handler_name: &str,
    device_addr: u8,
    device_id: &str,
) -> Option<Arc<rs485_plugin::device::Rs485Device>> {
    let config = rs485_plugin::config::Config {
        device_addr,
        ..Default::default()
    };
    let handler = rs485_plugin::handlers::ProtocolHandlerRegistry::get(handler_name, &config)?;
    let device = rs485_plugin::device::Rs485Device::new(
        device_id.to_string(),
        handler_name.to_string(),
        config,
        handler,
    );
    match device.open() {
        Ok(()) => Some(Arc::new(device)),
        Err(e) => {
            tracing::warn!("南向设备 {} 串口打开失败: {}", device_id, e);
            None
        }
    }
}

/// DataFrame → DataPackage（FIXME: 固定值代替 Modbus 寄存器映射，实际应解析 frame.data）
fn dataframe_to_datapackage(frame: &device_trait::DataFrame) -> mupc_data_processing::DataPackage {
    mupc_data_processing::DataPackage {
        electrical: mupc_data_processing::ElectricalData {
            voltage: Some(380.0),
            current: Some(100.0),
            active_power: Some(50.0),
            reactive_power: Some(10.0),
            cos_phi: Some(0.98),
            frequency: Some(50.0),
            phase: None,
        },
        battery: mupc_data_processing::BatteryData {
            soc: Some(75.0),
            soh: Some(95.0),
            temperature: Some(35.0),
        },
        device_status: mupc_data_processing::DeviceStatus {
            inverter_status: mupc_data_processing::InverterStatus::Running,
            pv_power: Some(30.0),
            load_power: Some(40.0),
            ev_charger_power: Some(10.0),
        },
        timestamp: frame.timestamp / 1000,
    }
}

/// DataPackage → 遥测点列表（FIXME: 指标映射根据点表确定）
fn datapackage_to_telemetry_points(
    pkg: &mupc_data_processing::DataPackage,
    device_id: &str,
) -> Vec<mupc_storage::TelemetryPoint> {
    let ts =
        chrono::DateTime::from_timestamp(pkg.timestamp as i64, 0).unwrap_or_else(chrono::Utc::now);
    let metrics: Vec<(&str, Option<f64>)> = vec![
        ("voltage", pkg.electrical.voltage),
        ("current", pkg.electrical.current),
        ("active_power", pkg.electrical.active_power),
        ("reactive_power", pkg.electrical.reactive_power),
        ("cos_phi", pkg.electrical.cos_phi),
        ("frequency", pkg.electrical.frequency),
        ("battery_soc", pkg.battery.soc),
        ("battery_soh", pkg.battery.soh),
        ("battery_temperature", pkg.battery.temperature),
        ("pv_power", pkg.device_status.pv_power),
        ("load_power", pkg.device_status.load_power),
        ("ev_charger_power", pkg.device_status.ev_charger_power),
    ];
    metrics
        .into_iter()
        .filter_map(|(name, value)| {
            value.map(|v| mupc_storage::TelemetryPoint {
                id: None,
                device_id: device_id.to_string(),
                timestamp: ts,
                metric_name: name.to_string(),
                // `value` 自 03 设计 §9.1.4 起可空：本路径**只在有值**时建点（`Option::map`）
                // ⇒ 恒为 `Some`；缺测字段的既有行为不变（不建点、不落库）。
                value: Some(v),
                quality: 0,
            })
        })
        .collect()
}

/// 从 `DataPackage` 抽取一个 [`mupc_storage::GridSample`]（**唯一抽取点**，03 设计 §9.6 末段）。
///
/// # 为什么在装配层（而不是 `storage`）
///
/// 设计 §9.6 末段写「映射函数落 `storage`」，但 §9.8 D-8 的**依赖边裁定**（v1.3 新增，专门为
/// 同类问题立的规矩）明确：`storage` **不依赖** `data-processing`，把不认识 `DataPackage` 的
/// 转换塞进去会**新增 `storage → data-processing` 依赖边**。两处冲突时以 D-8 为准（依赖边是
/// 硬约束，落点只是代码组织），故抽取同 `quality_map.rs` 一起落装配层 —— `core-bin` 同时依赖
/// 两者，**零新增边**。
///
/// 语义（§9.1.3 / §9.6 末段）：
/// - `electrical.phase` **缺块** ⇒ 15 个分相通道（u/i/p/q/pf）全 `None`，其中 `pf_total` 是
///   A 相值（`pf[0]`，与 mapper 的 `electrical.cos_phi = pf[0]` 同源）⇒ 一并 `None`；
/// - `electrical.active_power` / `reactive_power` **缺块** ⇒ `p_total` / `q_total` 为 `None`；
/// - **`frequency` 不抽取**（点表无频率寄存器、mapper 恒 50.0 常量，常量入库会污染统计，
///   §9.1.3 明文「不落」）；视在功率 / 电能同属"无点表来源"，**不抽取、不造数**。
fn grid_sample_from_package(pkg: &mupc_data_processing::DataPackage) -> mupc_storage::GridSample {
    let el = &pkg.electrical;
    let mut s = mupc_storage::GridSample {
        p_total: el.active_power,
        q_total: el.reactive_power,
        ..Default::default()
    };
    // 缺相量块 ⇒ 分相通道全 `None`（产 `NoData` 行），**不臆造**。
    if let Some(ph) = el.phase.as_ref() {
        s.u = ph.voltage;
        s.i = ph.current;
        s.p = ph.active_power;
        s.q = ph.reactive_power;
        s.pf = ph.cos_phi;
    }
    s
}

/// 总表聚合的定时 tick 任务（03 设计 §9.1.5 / §9.6 序 6）。
///
/// 周期 **1000 ms**（聚合周期最小 10 s ⇒ 1 s 粒度足够把「已走完的周期」及时闭合）。
/// 收工契约（U-64 的**协作生产者**）：收到 `stop` ⇒ `flush(now)` 入队 ⇒ 退出；剩余缓冲由
/// 既有退出序列的最后一次 flush 落盘（**本任务不自己提交**，只往 `WriteBuffer` 入队）。
///
/// 落库失败语义沿用既有路径：`buffer_telemetry` 返回 `Err` 只 `warn`（与南向遥测、事件落库
/// 同范式：丢点可观测、不 panic、不停采集）。
/// ⚠️ **FLS-04 订正（2026-09-26）**：`buffer_telemetry` 自本批起**不再可能返回 `Err`**
/// （容量触发只投一次非阻塞唤醒，不再在调用栈内提交）⇒ 本任务那几个 `if let Err(..)` 分支
/// 退化为永不触发（保留不删：签名兼容）。真正的落库失败仍由 `flush_batch` 响亮化 + 计数，
/// 并由 `storage_health_timer` 转成 `major` 告警。
/// 聚合行的**生产者 → 入队者**通道（T15/T16 遗留③ 的修法，2026-09-24）。
///
/// # 为什么是通道而不是 `tokio::spawn`（竞态消除的**结构性**理由）
///
/// **修复前**：`SouthSink::forward_grid_sample` 每产出一次聚合行就 `tokio::spawn` 一个
/// **游离任务**去入队。该 spawn **不登记**在 `producers`/`bg_tasks` 任何名单里 ⇒ 退出序列
/// 对它**没有任何可见性**：它可能在 `stop_producers` 返回**之后**才被创建，其
/// `buffer_telemetry`（容量触发路径会 `await` 提交）就可能落在**最后一次 flush 之后**
/// ⇒ 这一批（≤22 行）随进程退出滞留内存而丢失。这正是 U-64 之后残留的那条窄竞态。
///
/// **修复后**：聚合行**不再有任何游离任务**——`forward_grid_sample` 只做一次**同步内存
/// 投递**（同一调用栈内，无 await、无 spawn），由**已注册**的 `grid_agg_timer` 任务
/// 统一入队。于是：
/// 1. 聚合行通往 `WriteBuffer` 的路径**唯一**，且该路径的终点是一个
///    `producers` 名单里的任务 ⇒ `stop_producers` 会**等它确认收工**（有上限）之后
///    才做最后 flush ⇒ "入队落在最后 flush 之后"在结构上不再可能（不再存在
///    退出序列看不见的写者）；
/// 2. 投递本身是**同一调用栈内的同步 send** ⇒ 任务收工时刻不可能还有"在半空中的
///    入队工作"：要么行已在通道里（收工分支会 drain），要么行的生产者回调**尚未执行**；
/// 3. 收工分支先 drain 通道、再 `flush(now)` 关闭未闭合周期、最后二次 drain 才入队
///    ⇒ 最后一段周期由**退出序列等待的那个任务**亲自闭合与入队（修复前这一步只发生在
///    与退出竞跑的 tick 里）。
pub(crate) type AggregateRowSender =
    tokio::sync::mpsc::UnboundedSender<Vec<mupc_storage::AggregateRow>>;

/// 聚合行通道的接收端。
pub(crate) type AggregateRowReceiver =
    tokio::sync::mpsc::UnboundedReceiver<Vec<mupc_storage::AggregateRow>>;

/// 建通道（无界：投递侧处**同步回调**内，不能 await；有界通道的 `try_send` 会在满时丢行
/// ——丢弃聚合行是设计不允许的静默损失）。
pub(crate) fn aggregate_row_channel() -> (AggregateRowSender, AggregateRowReceiver) {
    tokio::sync::mpsc::unbounded_channel()
}

/// 把通道里已投递的批次**全部**取出（保持投递顺序）。
fn drain_aggregate_rows(rx: &mut AggregateRowReceiver) -> Vec<mupc_storage::AggregateRow> {
    let mut out = Vec::new();
    while let Ok(mut batch) = rx.try_recv() {
        out.append(&mut batch);
    }
    out
}

/// 总表聚合的**唯一**入队者（§9.2.3 末 / §9.6 序 6；U-64 协作退出名单成员）。
///
/// 见 [`AggregateRowSender`] 的"结构性理由"：本任务同时是 ① 周期闭合者（`tick`）与
/// ② 聚合行入队者（drain 通道），收工时 ③ drain + `flush(now)` 关闭最后一段 + 再 drain。
fn spawn_grid_aggregate_timer(
    aggregator: Arc<parking_lot::Mutex<mupc_storage::GridAggregator>>,
    write_buffer: Arc<mupc_storage::WriteBuffer>,
    device_id: String,
    mut stop: tokio::sync::watch::Receiver<bool>,
    mut agg_rx: AggregateRowReceiver,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(1_000));
        // 落后时顺延（不追赶补打）：补打只会连产空批，节拍按原样继续即可。
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = stop.changed() => {
                    // ① 先 drain 通道里**已投递**的行（按投递顺序，早于下面的 flush 产出）
                    let mut rows = drain_aggregate_rows(&mut agg_rx);
                    // ② 再关闭**未闭合周期**（最后一段不丢）
                    let now_ms = now_ms();
                    rows.extend(aggregator.lock().flush(now_ms));
                    // ③ 让出一次调度：无停机钩子的南向采集口只是被 `abort`（生效点在它下一次
                    //    await），其**在途回调**仍可能刚投递完；再 drain 一次把这批也带走。
                    tokio::task::yield_now().await;
                    rows.extend(drain_aggregate_rows(&mut agg_rx));
                    tracing::debug!(rows = rows.len(), "停机信号：总表聚合任务收工（drain 通道 + flush 未闭合周期）");
                    enqueue_aggregate_rows(&write_buffer, &device_id, rows).await;
                    break;
                }
                _ = ticker.tick() => {
                    // 投递顺序 = 采集时序：先取 handoff 的行，再取本轮 `tick` 闭合的行
                    let mut rows = drain_aggregate_rows(&mut agg_rx);
                    let now_ms = now_ms();
                    rows.extend(aggregator.lock().tick(now_ms));
                    enqueue_aggregate_rows(&write_buffer, &device_id, rows).await;
                }
            }
        }
    })
}

/// 把聚合产出的行**原样**转成落库点并入队（唯一转换点是 `AggregateRow::to_telemetry_point`，
/// 缺测行在那里保持 `None` ⇒ 库内真 `NULL`，**不得在此 `unwrap_or(0.0)`**）。
async fn enqueue_aggregate_rows(
    write_buffer: &mupc_storage::WriteBuffer,
    device_id: &str,
    rows: Vec<mupc_storage::AggregateRow>,
) {
    for row in rows {
        let point = row.to_telemetry_point(device_id);
        if let Err(e) = write_buffer.buffer_telemetry(point).await {
            // 与南向遥测同一范式：落库失败 warn（`WriteBuffer` 内部已回填待重试 + 已 error 计数），
            // 不 panic、不停 tick。
            tracing::warn!("总表聚合落库失败 {}: {}", row.metric_name, e);
        }
    }
}

/// 当前 UTC 毫秒（`u64`；负数——时钟早于 epoch——钳到 0，**不 panic**）。
fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

/// S3 §10.3：southd 采集结果 sink（core-bin 装配侧实现，startup 是 core-bin 唯一装配者）。
///
/// 分流语义（southd scheduler 已按 role 分流，单写方口径见 southd 模块头）：
/// - 含 meter_grid 站时，grid 遥测 → `AiIntegrator::set_latest_data`（策略 phase 唯一写方；
///   master_meter 段已删除收敛，south_stations.meter_grid 是唯一 grid 源，绝不并存第二写方）。
///   仅配非 grid 站（B2）时本 sink 不触发 on_grid_package，AiIntegrator 由 pv/load 南向模拟兜底。
/// - 非 grid 遥测点（is_event=false）→ WriteBuffer 落库（telemetry）。
/// - offline/online 状态事件（is_event=true，metric=offline/online）→ storage.events 落库
///   + `AlertFeed` 即时投递（单元 K：原 web-api 的 SSE 推送服务已随 crate 删除）。
struct SouthSink {
    ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
    write_buffer: Arc<mupc_storage::WriteBuffer>,
    events: Arc<dyn mupc_storage::EventRepository>,
    /// **即时投递环**（设计 §4.7）。⚠️ **不是 F7 真源**——F7 真源仍是 `storage.events`；
    /// 本字段只做「未落库也能上屏」的可选增强（是否并入 F7 由待裁项 R-07 决定，本轮不裁）。
    alert_feed: Arc<crate::alert_feed::AlertFeed>,
    /// IEC104 服务器句柄。⚠️ **T13 后本字段不再被读取**：grid 真值上送已从本 sink 的
    /// `broadcast_grid_iec104`（旧 R2-A2 路径，TI=13 无时标）迁移到 `Iec104UplinkDriver`
    /// 的 A 档任务（01 设计 §9.4 序 4「删除，并入 A 档」）。保留字段与构造入参只为
    /// **不改既有 `SouthSink::new` 签名**（三处既有用例的装配零改动），故 `allow(dead_code)`。
    #[allow(dead_code)]
    iec104: Arc<mupc_gateway::iec104::server::Iec104Server>,
    /// **外设遥测最新值快照**（01 设计 §9.1；本 sink 是其**唯一写入方**，§9.1.4 / §9.4 序 3）。
    /// 与既有 `WriteBuffer`（**历史**通道）**并存不互替**：本支路只**新增**快照写入，
    /// 不改落库路径（§9.1.7 的边界：实时值与历史表"不是一回事、不得互相替代"）。
    latest: Arc<LatestValues>,
    /// grid 站 id。`on_grid_package(pkg)` 的入参**不含站 id**（既有 `StationSink` 契约
    /// **不改**，§9.1.1），故在装配期从 `south_stations.grid_station()` 解析一次。
    /// `None` = 未配 meter_grid ⇒ 该回调不写快照（**不臆造站 id**）。
    grid_station_id: Option<String>,
    /// **总表电气量聚合器**（U-69 / 03 设计 §9.1）。本 sink 只做「转发样本」（`observe`）；
    /// 聚合算法本身在 `mupc_storage::GridAggregator`（纯逻辑、可单测），**周期闭合由 tick 任务
    /// 驱动**（`spawn_grid_aggregate_timer`），二者共用同一个 `Arc`。
    grid_aggregator: Arc<parking_lot::Mutex<mupc_storage::GridAggregator>>,
    /// 聚合行**投递通道**（T15/T16 遗留③ 修法）：本 sink 只做**同步投递**，
    /// 入队由已注册的 `grid_agg_timer` 承担（理由见 [`AggregateRowSender`] 的文档）。
    ///
    /// ⚠️ **不得**改回 `tokio::spawn`：那会让聚合行的入队重新落进"退出序列看不见的
    /// 游离任务"，从而恢复"落在最后一次 flush 之后"的窄竞态（U-64 之后 T15/T16 的遗留项）。
    agg_tx: AggregateRowSender,
}

impl SouthSink {
    // 装配层构造器：8 个入参全是**依赖注入点**（各自都不可由其它入参推出）⇒ 按签名原样放行。
    #[allow(clippy::too_many_arguments)]
    fn new(
        ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
        write_buffer: Arc<mupc_storage::WriteBuffer>,
        events: Arc<dyn mupc_storage::EventRepository>,
        alert_feed: Arc<crate::alert_feed::AlertFeed>,
        iec104: Arc<mupc_gateway::iec104::server::Iec104Server>,
        latest: Arc<LatestValues>,
        grid_station_id: Option<String>,
        grid_aggregator: Arc<parking_lot::Mutex<mupc_storage::GridAggregator>>,
        agg_tx: AggregateRowSender,
    ) -> Self {
        Self {
            ai_integrator,
            write_buffer,
            events,
            alert_feed,
            iec104,
            latest,
            grid_station_id,
            grid_aggregator,
            agg_tx,
        }
    }

    /// U-69（03 设计 §9.1.4 / §9.6 序 5）：把 grid 包的电气量**样本转发**给聚合器，把**已闭合
    /// 周期**的行**投递**给聚合行通道。本函数**只做转发 + 投递**，不做任何聚合计算
    /// （周期边界/均值/极值全在 `mupc_storage::GridAggregator` 内，可单测）。
    ///
    /// 时序：在既有 `set_latest_data`（策略 phase 单写方）与 `apply_grid_snapshot`（最新值快照）
    /// **之后**调用 ⇒ 两条既有路径**逐字不动**（PRD GRD-07：不改变北向上送与策略输入）。
    ///
    /// **本函数不含 `tokio::spawn`**（T15/T16 遗留③ 修复，2026-09-24）：投递是**同步内存
    /// 动作**（同一调用栈、无 await），入队由已注册的 `grid_agg_timer` 任务承担 ⇒ 采集调用栈
    /// 仍**不被 DB 提交阻塞**（原 spawn 的这一目的保持成立），但聚合行不再存在"退出序列看不见
    /// 的在飞任务"（原竞态的成因）。理由详见 [`AggregateRowSender`]。
    fn forward_grid_sample(&self, pkg: &mupc_data_processing::DataPackage) {
        if self.grid_station_id.is_none() {
            // 未配 meter_grid ⇒ 不该走到这里（`on_grid_package` 只由 meter_grid 站触发）；
            // 真发生也**不臆造站 id**：只跳过聚合，既有两条路径不受影响。
            return;
        }
        let ts_ms = now_ms();
        let sample = grid_sample_from_package(pkg);
        // 临界区只有纯计算（无 await）⇒ 用 parking_lot，不跨 await 持锁。
        let rows = { self.grid_aggregator.lock().observe(ts_ms, &sample) };
        if rows.is_empty() {
            return;
        }
        if let Err(e) = self.agg_tx.send(rows) {
            // 通道已关 = `grid_agg_timer` 已收工（只可能发生在退出期：它是唯一接收方，
            // 其收工分支在 exit 序列内、且排在最后 flush 之前）。**如实记 ERROR 不静默**
            // ——若这条日志出现，说明南向采集回调在该任务收工之后仍在跑（即 `shutdown()`
            // 文档里已登记的"abort 名单任务的极端时序"窗口），需按该窗口的口径评估。
            let n = e.0.len();
            // 文案**避开**异步 spawn 的 API 字面（本函数有源文本结构断言：函数体内不得再出现
            // 该串，防回退成游离任务）——此处只描述"不得改回游离任务"的语义。
            tracing::error!(
                rows = n,
                "聚合行通道已关闭（grid_agg_timer 已收工）⇒ 本批 {n} 行未入队（退出期极端时序，\
                 已如实记录；不得改回游离任务——那会恢复更宽的窄竞态）"
            );
        }
    }

    /// 01 设计 §9.1.4 第 1 行：`DataPackage.electrical` 顶层 **6 个派生量** → 最新值快照。
    ///
    /// 点名沿用既有 IEC104 固定 IOA 表（§9.2.1 段 1）的**派生名** —— 这是 §9.7 C-17 ②
    /// 登记的**唯一点名例外**（grid 6 点是 `DataPackage.electrical` 的派生量，非 02 号点表
    /// 展开名；改名会同时破坏既有对点与 `AiIntegrator` 的键）。
    ///
    /// `ts_ms = Utc::now()`：与**同一次** `on_grid_package` 内的既有 grid 路径同刻
    /// （§9.1.4 时标语义）。
    ///
    /// **不可得**（字段 `None`）写 `value: None` + `Invalid`（**不补 0**，LV-6）。这与既有
    /// IEC104 支路"Some 才上送"**效果一致**：上送侧按 `quality == Ok` 过滤，`Invalid` 点同样不发。
    fn apply_grid_snapshot(&self, pkg: &mupc_data_processing::DataPackage) {
        let Some(station) = self.grid_station_id.as_deref() else {
            return;
        };
        let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        self.latest.mark_station_polled(station, now_ms);
        let el = &pkg.electrical;
        let samples: Vec<(PointId, PointValue)> = [
            ("active_power", el.active_power),
            ("reactive_power", el.reactive_power),
            ("voltage", el.voltage),
            ("current", el.current),
            ("cos_phi", el.cos_phi),
            ("frequency", el.frequency),
        ]
        .into_iter()
        .map(|(metric, v)| {
            (
                PointId {
                    station: station.to_string(),
                    metric: metric.to_string(),
                },
                PointValue {
                    value: v,
                    ts_ms: now_ms,
                    quality: if v.is_some() {
                        PointQuality::Ok
                    } else {
                        PointQuality::Invalid
                    },
                },
            )
        })
        .collect();
        self.latest.apply(samples);
    }

    /// 北向 IEC104 上送 meter_grid 遥测真值（审查 R2-A2，2026-09-09）。
    ///
    /// 上送量（固定 IOA 分配，**现场点表追认**）：1=active_power(kW)、2=reactive_power(kVAr)、
    /// 3=voltage(V)、4=current(A)、5=cos_phi、6=frequency(Hz)。DataPackage.electrical
    /// 字段逐个 `Option<f64>`，Some 才上送（meter_grid mapper 现全量填顶层量，防御保留）。
    ///
    /// 频率节流判断：southd scheduler 按站 interval_ms 排程，meter_grid 每轮 poll 成功即触发
    /// on_grid_package 一次。config::validate 仅约束 meter_grid interval_ms>0 且 <5000
    /// （DATA_FRESHNESS_MS），**未保证 >=1s**（现场可配如 500ms）→ 在此钳 1Hz 上界防
    /// broadcast flood。生产样例 interval_ms=1000 每收即上送，不受节流影响。
    /// 系统事件落库 + **即时投递**（设计 §4.7：「`SouthSink` 写入系统事件时同时投递」）。
    ///
    /// 两条入口（[`StationSink::on_station_telemetry`] 的状态事件分支与覆写的
    /// [`StationSink::on_station_offline`]）**共用本函数** ⇒ "落库文案 == 投递文案"由代码结构
    /// 保证（单元 K ③ 的断言即靠此）。落库失败仅 warn 不 panic（与 interlock 同范式）。
    async fn record_event(&self, event_type: &str, source: &str, message: &str, level: &str) {
        let ev = mupc_storage::SystemEvent {
            id: None,
            timestamp: chrono::Utc::now(),
            event_type: event_type.to_string(),
            source: source.to_string(),
            message: message.to_string(),
        };
        if let Err(e) = self.events.insert(&ev).await {
            tracing::warn!("南向站事件落库失败 {}: {}", ev.event_type, e);
        }
        // 无订阅者时投递返回 0，属正常态，不是错误。
        self.alert_feed.push_system_alert(level, message);
    }

    /// 状态事件的**中文名 + 原始值**文案（设计 §11.7.2 第 4/5 条）：
    /// - **中文名**：`point_table::label(role, metric)` 命中则用之（如 `bms_alarm_225`
    ///   → "簇一级告警"，RC-1 的机读核对清单同源）；未命中（`<点名>@<信号键>`、
    ///   `<原名>@recovered`、`soc_out_of_range`、`fire_detector_*` 等**事件命名空间**的名字）
    ///   **用原名** —— 不引入新的事件类型枚举，事件键仍是 `south_station.<站id>.<metric>`；
    /// - **原始值落证**：offline/online 是纯状态信号（value 恒 1.0，写进文案无信息量，且
    ///   既有 SSE 文案与断言逐字依赖），故这两者保持原文案；其余事件把 `value` 写进文案
    ///   （PRD §9.6.3 ② 要求"越界告警含原始寄存器值"——`soc_out_of_range` 的 value 即越界原值）。
    fn event_message(
        station_id: &str,
        role: mupc_southd::config::Role,
        metric: &str,
        value: f64,
    ) -> String {
        match metric {
            "offline" => format!("站 {station_id} role={role:?} 离线（采集失败）"),
            "online" => format!("站 {station_id} role={role:?} 恢复上线"),
            _ => {
                let name = mupc_southd::point_table::label(role, metric).unwrap_or(metric);
                format!("站 {station_id} role={role:?} {name}（值 {value}）")
            }
        }
    }

    /// 01 设计 §9.2.1.1 / §9.4 序 3：**写入侧**求值 15 组 BMS 聚合并写回快照（唯一求值点）。
    ///
    /// 触发：`on_station_telemetry` 内 `role == Battery` 且 `mark_station_polled` + 本批
    /// `apply` **之后**（看到的是本轮一致视图）。求值输入 = 快照里该站**全部 288 个位点**
    /// （一次 `station_snapshot` 取读锁，**不得逐点 `get`**——那会取 288 次锁）；位地址 `a`
    /// ↔ 点名 `bms_alarm_{a-199}`（`bms_alarm` 块 `addr:200` + `positional`，§9.2.1.1）。
    /// 结果以聚合点名经**唯一写入口** `apply` 写回（`ts_ms = 本轮`）⇒ C 档变更订阅 / 总召 /
    /// 初始快照天然可见，不存在第二写方。值不变 ⇒ `apply` 不广播（COS，§9.1.5）。
    fn apply_bms_aggregates(&self, station_id: &str, now_ms: u64) {
        // 单次取读锁拿该站全部点，构建 位地址 → (值, 质量) 查询表
        let snap = self.latest.station_snapshot(station_id);
        let mut bit_map: HashMap<u16, (f64, PointQuality)> = HashMap::new();
        for pv in &snap {
            if let Some(k) = pv
                .id
                .metric
                .strip_prefix("bms_alarm_")
                .and_then(|s| s.parse::<u16>().ok())
            {
                if let Some(v) = pv.value.value {
                    // 位地址 = 199 + 序号（`bms_alarm_k` ↔ 位地址 199+k）
                    bit_map.insert(199 + k, (v, pv.value.quality));
                }
            }
        }
        let lookup = |addr: u16| bit_map.get(&addr).copied();
        let aggrs = mupc_southd::uplink::evaluate_bms_aggregates(&lookup);
        let samples: Vec<(PointId, PointValue)> = aggrs
            .into_iter()
            .map(|(metric, value, quality)| {
                (
                    PointId {
                        station: station_id.to_string(),
                        metric: metric.to_string(),
                    },
                    PointValue {
                        value,
                        ts_ms: now_ms,
                        quality,
                    },
                )
            })
            .collect();
        self.latest.apply(samples);
    }
}

#[async_trait::async_trait]
impl mupc_southd::scheduler::StationSink for SouthSink {
    async fn on_grid_package(&self, pkg: mupc_data_processing::DataPackage) {
        // 策略 phase 单写方（唯一 grid 源 south_stations.meter_grid，M-4 防双写方并存）。
        self.ai_integrator.set_latest_data(pkg.clone()).await;
        // 01 设计 §9.1.4 第 1 行：同包派生 6 量写最新值快照。**IEC104 上送不再在此**——
        // 旧 `broadcast_grid_iec104`（R2-A2，TI=13 无时标）已删除（§9.4 序 4），grid 6 点
        // 由 `Iec104UplinkDriver` 的 A 档任务统一上送（TI=36 带时标，§9.2.4 方案 A）。
        self.apply_grid_snapshot(&pkg);
        // U-69（03 设计 §9.6 序 5）：同包电气量**转发样本**给聚合器（周期边界由 tick 驱动）。
        // 刻意放在两条既有路径**之后**：既有行为逐字不动（PRD GRD-07）。
        self.forward_grid_sample(&pkg);
    }

    async fn on_station_telemetry(
        &self,
        station_id: &str,
        role: mupc_southd::config::Role,
        points: Vec<(String, f64, bool)>,
    ) {
        // ① 01 设计 §9.1.4 第 2 行①：**先**刷站活性（与点数无关；`online` 合成事件的那次
        //    调用同样刷新）。只刷"站在采"这一事实 ⇒ 不产生变更批、不影响 COS 语义。
        let now_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        self.latest.mark_station_polled(station_id, now_ms);

        // ② 非事件点（**含位点**，LV-5 要求位块点同样有入口）→ 快照；③ 事件点**不写**快照
        //    （它们是站级/信号级事件，归 `events` 表，不属"点位最新值"）。
        let mut samples: Vec<(PointId, PointValue)> = Vec::with_capacity(points.len());
        for (metric, value, is_event) in points {
            if is_event {
                // 状态事件（offline/online 由 scheduler handle_failure/mark_success 合成，
                // 其余为位/信号/站级量事件）：DB 落库 + SSE system alert（先落库、后投递）。
                // 形如 south_station.<站id>.offline / .fire_sys_1@main_power_fault。
                let event_type = format!("south_station.{}.{}", station_id, metric);
                let message = Self::event_message(station_id, role, &metric, value);
                // online 恢复是状态正常化，用 info 级；offline/其它状态异常才告警级，
                // 避免站恢复上线时刷屏 warning。
                let level = if metric == "online" {
                    "info"
                } else {
                    "warning"
                };
                self.record_event(&event_type, station_id, &message, level)
                    .await;
            } else {
                // ② 非事件点（**含位点**）写最新值快照：`is_event == false` ⇒ 有采集事实
                //    ⇒ `value: Some(_)` + `Ok`；同一轮交付的全部点**共用同一 `ts_ms`**
                //    （southd mapper 逐点不携带时标，§9.1.4 时标语义）。
                samples.push((
                    PointId {
                        station: station_id.to_string(),
                        metric: metric.clone(),
                    },
                    PointValue {
                        value: Some(value),
                        ts_ms: now_ms,
                        quality: PointQuality::Ok,
                    },
                ));
                // 普通遥测点落库（**既有路径原样不动**：历史通道与实时快照并存不互替）
                let tp = mupc_storage::TelemetryPoint {
                    id: None,
                    device_id: station_id.to_string(),
                    timestamp: chrono::Utc::now(),
                    metric_name: metric,
                    // `Some(v)`：非事件点必**有采集事实**（`is_event == false`）⇒ 有值。
                    value: Some(value),
                    // 跨域转换走装配层唯一映射点（03 设计 §9.6 序 10 / D-8）：
                    // 本分支的点质量与上面快照同源（`PointQuality::Ok`）⇒ 落库值仍为 **0**
                    // （`Quality::Good`，零行为变化）。
                    quality: crate::quality_map::quality_from_point_quality(PointQuality::Ok),
                };
                if let Err(e) = self.write_buffer.buffer_telemetry(tp).await {
                    // 与事件落库路径一致用 warn：遥测丢点影响持久性可观测，不宜静默降 debug
                    tracing::warn!("南向遥测落库失败 {}: {}", station_id, e);
                }
            }
        }
        // 本批（含全部非事件点）一次 `apply` ⇒ 天然合并为一批（LV-4 允许合并）
        self.latest.apply(samples);
        // ④ 01 设计 §9.2.1.1 / §9.4 序 3：`role == Battery` ⇒ 本批 `apply` 之后立即求值
        //    15 组 BMS 聚合并写回快照（写入侧唯一求值点；输入 = 快照里该站全部 288 位点，
        //    一次取读锁，不逐点 get）。幂等：scheduler 每轮至多两次调用本回调（遥测 + 事件），
        //    值不变 ⇒ `apply` 不广播（COS）。
        if role == mupc_southd::config::Role::Battery {
            self.apply_bms_aggregates(station_id, now_ms);
        }
    }

    /// **覆写默认实现**（T5 留在 `StationSink` 上的接缝，T6 落地）：把站失败的 `reason`
    /// （含 `slave/addr/count`，由 scheduler 组装）写进事件 `message`，使 PRD §9.7.2 第 1 条
    /// "站进入 offline，事件 `reason` 含 `slave/addr/count` → 运维据 `events` 定位到具体块"
    /// **真正可用**。
    ///
    /// 与默认实现的**一致项**（刻意保持，防消费方被动受影响）：事件类型仍是
    /// `south_station.<站id>.offline`、等级仍是 warning、事件**去抖仍由 scheduler 负责**
    /// （本层不重复去抖，故不会多产 offline 事件）。**差异只有文案**：多一句 `reason`。
    async fn on_station_offline(
        &self,
        station_id: &str,
        role: mupc_southd::config::Role,
        reason: &str,
    ) {
        // 01 设计 §9.1.4 第 3 行：全站置 `Invalid`、清站活性、**保原值原时标**（EX-1）。
        // 该站全部点（含同站聚合点）因此被上送侧按 `quality == Ok` 过滤 ⇒ 与"站离线该点
        // 从周期上送与总召响应中消失"一致（§9.2.6），不靠本回调产出变更批。
        self.latest.mark_station_offline(station_id);

        let event_type = format!("south_station.{}.offline", station_id);
        let message = format!("站 {station_id} role={role:?} 离线（采集失败）：{reason}");
        self.record_event(&event_type, station_id, &message, "warning")
            .await;
    }

    async fn on_battery_soc(&self, station_id: &str, soc: f64) {
        // SOC 双源（04 §2.11.1）：battery 站（BMS）SOC 优先源 → AiIntegrator 双源裁决。
        // soc 由 AiIntegrator.set_battery_soc 就地校验（NaN/0-100 守卫），此处透明转发。
        tracing::debug!(station = %station_id, soc, "BMS 站 SOC 注入 AiIntegrator");
        self.ai_integrator.set_battery_soc(soc).await;
    }
}

/// 按依赖顺序初始化所有子系统
///
/// 14 步初始化流程，每步失败时级联清理已启动的服务。
/// `process_started_at` = **进程启动零点**（`main()` 入口最顶部取的 `Instant::now()`）。
/// 显示终端 F6 的 `device.uptime_secs` 用它作零点（设计 §4.1「以 `mupcd` 进程启动时刻为准」）
/// ——**不得**在本装配点另取 `Instant::now()`（本点已在 DB/intercore/gateway/AI/security 之后，
/// 会让屏上 uptime 系统性偏小）。
/// G-1 装配点：控制通道的配置读源 = **装配传入的同一个** `Arc<RwLock<CoreConfig>>`
/// （设计 D10「内存副本 + 原子落盘」的"进程内唯一权威读源"）。
///
/// **为什么单独成一个函数**：这条"同一性"是本模块最容易被**无声**破坏的契约——
/// `CoreConfig: Clone` 让 `Arc::new(RwLock::new(arc.read().await.clone()))` 这种**深拷贝成
/// 新 `Arc`** 的写法既能编译、也能通过全部既有用例，后果却是"装配写 A、控制通道读 B"
/// （G-2 的写入在屏上静默不生效）。收敛到一个函数后，装配点与单测
/// `console_host::tests::console_config_source_is_arc_identical_to_assembly_handle`
/// 走的是**同一段代码**——`Arc::ptr_eq` 才构成证据（测试里自造等价物只能证明测试自己）。
pub(crate) fn console_config_source(
    core_config: &std::sync::Arc<tokio::sync::RwLock<CoreConfig>>,
) -> crate::console_host::ConfigSource {
    crate::console_host::ConfigSource::Ready(core_config.clone())
}

/// 控制台**两条写路径**的装配（G-2 配置写 + **单元 J 联锁写**）：审计 sink 打开失败 ⇒
/// **两条写路径整体不可用**（fail-closed）。
///
/// **为什么抽成独立函数**（评审建议 6.1「`startup.rs` 该分支补一条单测」）：这是
/// **fail-closed 的唯一裁决点**，而 `initialize_all` 要跑完 DB/网络/串口才能走到这里 ⇒
/// 单测无法触达那条分支。抽出来后，`console_write_paths_fail_closed_when_audit_is_unusable`
/// 用**真实失败**（审计目录的父路径是普通文件）驱动它。
///
/// 口径：审计是 T-3 无登录后的**唯一操作凭据**（设计 §3.3）⇒ 宁可写路径整体不可用，
/// **也不**给一个"没有审计的写路径"。
///
/// # ⚠️ 为什么两条路径**必须**共用同一个 sink（而不是各开一个）
///
/// `FileAuditSink` 内含**全进程唯一**的哈希链 `AuditLogger`（`AuditLogger::new` 会读回既有链
/// 的最后一条哈希）。两个实例各持一条链 ⇒ 各自算 `sequence` ⇒ **链断**（`FileAuditSink` 的
/// `chain` 字段文档明写此约束）。故本函数**只 open 一次**，把同一个
/// `Arc<dyn ConsoleAuditSink>` 分别注入 `ConfigService` 与 `InterlockService`。
///
/// # 返回
///
/// `(配置写源, 联锁写源)`。审计失败时两条一起不可用（**不得**只降级其中一条：那会让
/// "审计坏了"在一条通道上表现为"操作被拒"、在另一条上表现为"照常执行"——同一件事两种后果）。
pub(crate) fn console_write_paths(
    audit_dir: &std::path::Path,
    config_path: &std::path::Path,
    core_config: &std::sync::Arc<tokio::sync::RwLock<CoreConfig>>,
    log_reload: Option<crate::hot_apply::LogReloadHandle>,
    interlock: Option<std::sync::Arc<dyn mupc_display_proto::InterlockApi>>,
) -> (
    crate::console_host::ApplySource,
    crate::console_host::InterlockOpsSource,
) {
    match crate::console_audit::FileAuditSink::open(audit_dir) {
        Ok(sink) => {
            let audit: std::sync::Arc<dyn crate::console_audit::ConsoleAuditSink> =
                std::sync::Arc::new(sink);
            let apply = crate::console_host::ApplySource::Ready(std::sync::Arc::new(
                crate::config_service::ConfigService::new(
                    config_path.to_path_buf(),
                    core_config.clone(),
                    audit.clone(),
                    crate::hot_apply::HotApply::new(log_reload),
                ),
            ));
            let ops = crate::console_host::InterlockOpsSource::Ready(std::sync::Arc::new(
                crate::interlock_ops::InterlockService::new(interlock, audit),
            ));
            (apply, ops)
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                "控制台审计不可用 ⇒ 配置写 / 联锁写两条路径整体不可用（fail-closed：写操作将被拒）"
            );
            const WHY: &str = "审计子系统不可用（fail-closed：审计是唯一操作凭据）";
            (
                crate::console_host::ApplySource::Unavailable(WHY),
                crate::console_host::InterlockOpsSource::AuditUnavailable(WHY),
            )
        }
    }
}

// ── U-73 §15.11 #7：消防钢瓶气压「是否配置」取数适配器（EDGE-23 / EX-11）──────────────────
//
// 包装 `SouthScheduler::cylinder_pressure_configured(station_index)`
// （`crates/mupc-southd/src/scheduler.rs`，真源 = 该站生命周期内绝对寄存器 5 是否出现过非 0，
// PRD §9.7.6），把 southd 的「**站下标**」口径翻成帧侧的「**站 id**」口径，并把
// §15.2.2 的三种状态**逐字**落地：
//   - `Some(false)` = 未配置 ⇒ 屏显「未配置」（忽略 `v`、不得含 `0 kPa`）；
//   - `Some(true)`  = 已配置 ⇒ 按值正常展示；
//   - `None`        = 不可得（**非消防站** / **接缝未接线**）⇒ 按值正常展示。
//
// # 为什么是「先建空壳、后 `attach`」而不是构造期入参
//
// `initialize_all` 内 HMI（`DisplayDataProvider` / `ConsoleDeps`）的装配点**早于** southd
// 调度器（顺序见该函数内两处注释）。要做成构造期依赖，就得把整段 HMI 装配搬到调度器之后
// —— 那会动到多条既有装配不变量（读通道 / 控制通道路由、`latest` 构造点、guard 顺序），
// 收益只是"少一个 `attach`"。故改为：装配期先建空壳（`attach` 前恒 `None`），
// `SouthScheduler::new` 之后立刻 `attach`。启动期那一瞬的 `None` **语义正确**
// （= 接缝尚未接线），不是造假。
//
// ⚠️ **不臆造**：本适配器**不**用"气压读数为 0"去猜 `Some(false)` —— "未配置"的判据是 southd
// 的 `cylinder_seen_nonzero` 记忆（**只置位、不回退**），本层只做口径转换与查表。
pub struct SouthCylinderPressureQuery {
    /// 站 id → `cfg.stations` 下标。**下标口径 = `SouthScheduler::new` 里 `state` 的构造序**
    /// （`cfg.stations.iter().enumerate()`）⇒ 两侧同序，本表是唯一转换点。
    index_of: std::collections::HashMap<String, usize>,
    /// 消防站 id 集合（`None` 成因之一"非消防站"的**唯一判定点**）。
    fire_ids: std::collections::HashSet<String>,
    /// 调度器句柄；`attach` 前 = `None` ⇒ 一律回 `None`（= 接缝未接线）。
    scheduler: std::sync::RwLock<Option<Arc<mupc_southd::scheduler::SouthScheduler>>>,
}

impl SouthCylinderPressureQuery {
    /// 从**站配置**（唯一真源）建"站 id → 下标"表与消防站集合。
    pub fn new(cfg: &mupc_southd::config::SouthStationsConfig) -> Self {
        let mut index_of = std::collections::HashMap::with_capacity(cfg.stations.len());
        let mut fire_ids = std::collections::HashSet::new();
        for (i, s) in cfg.stations.iter().enumerate() {
            index_of.insert(s.id.clone(), i);
            if s.role == mupc_southd::config::Role::Fire {
                fire_ids.insert(s.id.clone());
            }
        }
        Self {
            index_of,
            fire_ids,
            scheduler: std::sync::RwLock::new(None),
        }
    }

    /// 调度器建好后注入（装配期一次；`initialize_all` 内 `SouthScheduler::new` 之后）。
    /// 毒化不 panic（与仓内 `read_cache` 同口径：装配 / 采集路径不得因一次 panic 永久失效）。
    pub fn attach(&self, scheduler: Arc<mupc_southd::scheduler::SouthScheduler>) {
        *self.scheduler.write().unwrap_or_else(|e| e.into_inner()) = Some(scheduler);
    }
}

impl crate::display_host::CylinderPressureQuery for SouthCylinderPressureQuery {
    fn cylinder_configured(&self, station_id: &str) -> Option<bool> {
        // 入口守卫：**非消防站** ⇒ 不可得（§15.2.2 明文的 `None` 成因之一）。
        if !self.fire_ids.contains(station_id) {
            return None;
        }
        // 接缝未接线 ⇒ 不可得（另一成因）。**不得**回 `Some(false)`。
        let scheduler = self
            .scheduler
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()?;
        // 未登记站（配置里没有）⇒ 不可得（不臆造下标）。
        let idx = *self.index_of.get(station_id)?;
        Some(scheduler.cylinder_pressure_configured(idx))
    }
}

pub async fn initialize_all(
    core_config: &std::sync::Arc<tokio::sync::RwLock<CoreConfig>>,
    coord: &ServiceCoordinatorImpl,
    process_started_at: std::time::Instant,
    // ── G-2 新增两个入参（都是**数据**，不是全局单例：装配点与单测能注入不同的值）──────
    //
    // `config_path`：真源 yaml（`--config`）。**必须**是真源文件的真实路径——保留式编辑
    // 要读**原文本**才能保住现场注释（设计 §4.3.2.1）；传一个别的路径会静默改错文件。
    config_path: &std::path::Path,
    // `log_reload`：`tracing` 的 reload 句柄（`main.rs` 在 Phase 2 建）。`None` ⇒ 日志级别
    // **无法**热生效 ⇒ `hot_apply` 如实报"需重启"（**不谎报**生效）。
    log_reload: Option<crate::hot_apply::LogReloadHandle>,
) -> Result<StartupContext, MupcError> {
    // 装配期读一份**快照**（装配是一次性动作，各子系统的构造参数取自启动瞬间的配置）。
    // 内存副本本身（`core_config`）由控制通道宿主持续持有：G-2 写入后它才是权威读源，
    // 而装配过的子系统各自持有自己的生效机制（`watch` / reload handle，见设计 §4.3.3）。
    let config = core_config.read().await.clone();
    let config = &config;
    // 错误路径守卫: 初始化中途失败时 abort 所有已启动的后台任务
    // （后台任务**只**寄存在 `TaskGuard` 里：装配中途失败即全部 abort；成功路径在函数末尾
    //  `std::mem::take(&mut guard.0)` 移交给 `StartupContext`。
    //  S-5：此处原本另有一个 `let mut bg_tasks`（全函数无人使用），已删除。）
    struct TaskGuard(Vec<tokio::task::JoinHandle<()>>);
    impl Drop for TaskGuard {
        fn drop(&mut self) {
            for h in &self.0 {
                h.abort();
            }
        }
    }
    let mut guard = TaskGuard(Vec::new());

    /// 协作式退出任务的装配期哨兵（U-64）：与 [`TaskGuard`] 同为"装配中途失败即 abort"，
    /// 但登记的是**带标签的协作任务**（成功路径移交 `StartupContext::cooperative_tasks`，
    /// 退出时走"通知 + 等确认"而不是 abort）。
    struct ProducerGuard(Vec<(&'static str, tokio::task::JoinHandle<()>)>);
    impl Drop for ProducerGuard {
        fn drop(&mut self) {
            for (label, h) in &self.0 {
                tracing::debug!(task = label, "装配失败：abort 协作任务");
                h.abort();
            }
        }
    }
    let mut producers = ProducerGuard(Vec::new());

    // ── U-64：协作式停机信号（`tokio::sync::watch`，**不新增依赖**）──
    // **必须在生产者 spawn 之前建**：接收端要随 spawn 交到生产者手里，否则"通知收工"无从送达。
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);

    // ── 1. 消息总线 (无依赖) ──
    tracing::info!("[01/14] 初始化消息总线...");
    let message_bus = Arc::new(mupc_core::TokioMessageBus::new(256));
    coord.register_service("message_bus", ServiceStatus::Running);

    // ── 2. 安全模块 ──
    tracing::info!("[02/14] 初始化安全模块...");
    // TODO (Phase 2+): 加载 TLS 证书和 SM2/SM4 密钥
    // 当前: mupc-security 模块无 SecurityModule 类型
    tracing::info!(
        "安全模块初始化 (stub): cert_dir={}",
        config.system.cert_dir.display()
    );
    // 占位 stub：security 模块无 SecurityModule 实例，国密/TLS 未真正运行（Phase 2+）
    // ——不谎报 Running（审查 R2-B5）
    coord.register_service("security", ServiceStatus::Stopped);

    // ── 3. 持久化存储 ──
    tracing::info!("[03/14] 初始化持久化存储...");
    let data_dir = config.system.data_dir.clone();
    tokio::fs::create_dir_all(&data_dir).await.map_err(|e| {
        MupcError::new(
            ErrorCode::IoError,
            format!("创建数据目录失败: {}", e),
            "startup",
        )
    })?;
    let db_path = data_dir.join("mupc.db");
    if !db_path.exists() {
        tokio::fs::File::create(&db_path).await.map_err(|e| {
            MupcError::new(
                ErrorCode::IoError,
                format!("创建数据库文件失败: {}", e),
                "startup",
            )
        })?;
    }
    let db_str = db_path.to_str().expect("数据目录路径包含非法 UTF-8 字符");
    let pool = mupc_storage::init_pool(db_str).await.map_err(|e| {
        MupcError::new(
            ErrorCode::ConnectionFailed,
            format!("数据库连接失败: {}", e),
            "startup",
        )
    })?;
    mupc_storage::run_migrations(&pool).await.map_err(|e| {
        MupcError::new(
            ErrorCode::ConfigError,
            format!("数据库迁移失败: {}", e),
            "startup",
        )
    })?;
    let storage = Arc::new(mupc_storage::StorageService::new(Arc::new(pool)));
    // U-67（03 设计 §9.2.3）：`WriteBuffer::new` 的容量/间隔改读 `storage:` 段配置。
    // **缺省 = 现实现**（1000 / 5000，`StorageSectionConfig::default()`）⇒ 零行为变化
    // （PRD R-11.3-C / STG-01）；非法值已在 `CoreConfig::validate_storage` 拒启动（STG-04）。
    // ⚠️ 容量口径沿革：设计 03:1321 曾写"100ms 或积累 100 条"，现以 `storage.batch_capacity`
    // 为准（默认 1000 = 变更前的硬编码值）。
    let write_buffer = Arc::new(mupc_storage::WriteBuffer::new(
        config.storage.batch_capacity as usize,
        config.storage.flush_interval_ms,
        storage.pool().clone(),
    ));
    // ── U-69（03 设计 §9.1 / §9.2.3）：总表电气量 1 分钟聚合落库 ──
    // 聚合器实例**在此创建**（而非 `SouthSink` 构造处）：故障 tick 任务要在下面与 `flush_timer`
    // 同处 spawn（§9.6 序 6），而 tick 需要同一个 `Arc` ⇒ 先建、后两处共用。
    //
    // `parking_lot::Mutex`（`observe` 需 `&mut`）：临界区**不含 await**（只做纯计算 + 取行），
    // 故不会跨 await 持锁（也就不用 `tokio::sync::Mutex`）。
    let grid_aggregator = Arc::new(parking_lot::Mutex::new(mupc_storage::GridAggregator::new(
        config.storage.grid_aggregate_period_ms,
    )));
    // 聚合行通道（T15/T16 遗留③）：生产端 = `SouthSink`（同步投递），消费端 = 下方
    // `grid_agg_timer`（**已注册的协作生产者**，也就是"唯一入队者"）。
    let (agg_tx, agg_rx) = aggregate_row_channel();
    // 总表聚合记录的 `device_id`（§9.1.4：= 生效配置的 grid 站 id；未配 meter_grid 的部署
    // 用设计明文的常量 `grid_meter` 兜底 —— 与 `mupc_core_config.yaml` 的站 id 一致）。
    let grid_device_id: String = config
        .south_stations
        .grid_station()
        .map(|s| s.id.clone())
        .unwrap_or_else(|| "grid_meter".to_string());
    // P0-1：`flush_interval_ms` 的**读取方**（此前无任何读取方 ⇒ 不满一批的数据永久滞留内存）。
    // U-64：句柄入 `producers`（**协作退出名单**，不是 abort 名单）——它是运行期**唯一会 drain
    // 缓冲**的常驻者，必须"收到停机信号 → 确认收工"之后才轮到退出路径那次 flush；
    // 装配中途失败仍随 ProducerGuard::drop 一并 abort。
    producers.0.push((
        "flush_timer",
        write_buffer.clone().spawn_flush_timer(stop_rx.clone()),
    ));
    // U-69 的 tick 任务：**与 `flush_timer` 同范式、同名单**（§9.2.3 末 / §9.6 序 6）——
    // 它也是"会往 `WriteBuffer` 里写点"的协作生产者：收到停机信号后**先 `flush(now)` 入队**
    // （把当前未闭合周期闭合并落库，不丢最后一段）再收工；随后由**既有退出序列**最后 flush
    // 遥测缓冲（U-64 的顺序契约：通知生产者收工 → 等确认 → 最后 flush）。
    // **不得**绕开该名单自行在 `main.rs` 加 flush。
    producers.0.push((
        "grid_agg_timer",
        spawn_grid_aggregate_timer(
            grid_aggregator.clone(),
            write_buffer.clone(),
            grid_device_id.clone(),
            stop_rx.clone(),
            agg_rx,
        ),
    ));
    coord.register_service("storage", ServiceStatus::Running);

    // ── 4. 核间通信 ──
    tracing::info!("[04/14] 初始化核间通信...");
    // 传输通道由 intercore.transport 决定：**迁入南向后仅剩 `tcp`（仿真/联调，sim-bridge 作
    // TCP 服务端）**；PCS 主链路（原 modbus_rtu）已由顶层段 `south_pcs` 承担（下文 4′ 段）。
    // 未知值/`modbus_rtu` 一律启动即报错（M3），避免配置手误静默落到仿真通道、生产 PCS 空转不被控。
    //
    // ⚠️ 本变量在 PCS 迁入南向后**生产路径无消费者**（6 个注入点已全部改持 `PcsHandle`）；
    // 它只经 `StartupContext.intercore` 移交（该字段现无读取方）。保留 TCP 装配的理由：
    // ① 保留"核间通道"的演进起点（设计 ADR-014）；② tcp 本就是仿真/联调通道，删掉会让
    // sim-bridge 侧无从对接。
    // ⚠️ **本轮（2026-09-26）措辞订正**：原文写"**已登记为技术债**"，但技术债台账
    // `docs/technical-debt.md` **查无本条**（该说法当时只落在实施计划里）⇒ 现如实指向：
    // 详见实施计划 §待裁定 **P-2**（`intercore` TCP 装配保留但无消费者），**Task 12 统一
    // 回写技术债**。本条不是"已闭环"，也不是"已登记"。
    #[allow(unused_variables)]
    let intercore: Arc<mupc_intercore::IntercoreClient> = match config.intercore.transport.as_str()
    {
        "tcp" => {
            let remote_addr = format!("{}:{}", config.intercore.host, config.intercore.port);
            let transport = Arc::new(mupc_intercore::TcpTransport::new(remote_addr));
            // 原 N3 回读接收（`spawn_receive` → `battery_soc`）已随 intercore 的 PCS 面删除
            // （Task 11 / 02 设计 §13.9）：SOC 真源现为 `south_stations.battery`。
            Arc::new(mupc_intercore::IntercoreClient::with_transport(transport))
        }
        other => {
            return Err(MupcError::new(
                ErrorCode::ConfigError,
                format!(
                    "intercore.transport='{other}' 非法：仅支持 \"tcp\"（仿真/联调）；\
                     生产 PCS 主链路已迁至南向，见 south_pcs 段（02 设计 §13）"
                ),
                "startup",
            ));
        }
    };
    coord.register_service("intercore", ServiceStatus::Running);

    // ── 5. 插件加载器 ──
    tracing::info!("[05/14] 初始化插件加载器...");
    let plugin_loader = {
        let loader = plugin_loader::PluginLoaderImpl::new();
        for path in &config.plugins.search_paths {
            loader.add_search_path(path.to_string_lossy().to_string());
        }
        // 自动加载配置的插件（libloading FFI 加载 .so）
        for plugin_name in &config.plugins.auto_load {
            let so_name = format!("lib{}.so", plugin_name);
            tracing::info!("加载插件: {}", so_name);
            match loader.load(&so_name, serde_json::json!({})) {
                Ok(()) => tracing::info!("  {} 加载成功", plugin_name),
                Err(e) => {
                    tracing::warn!("  {} 加载失败 (预期内，如 .so 未编译): {}", plugin_name, e)
                }
            }
        }
        tracing::info!(
            "插件目录已配置: {} 个搜索路径, {} 个自动加载插件",
            config.plugins.search_paths.len(),
            config.plugins.auto_load.len()
        );
        Arc::new(loader)
    };
    coord.register_service("plugin_loader", ServiceStatus::Running);

    // ── 6. 遥测数据采集 ──
    tracing::info!("[06/14] 初始化遥测数据采集...");
    // DataCollectorImpl / HighFreqTelemetryImpl 为纯数据容器，延迟创建
    // FaultRecorderImpl 保留实例（供故障录波使用），避免创建后立即 drop 的反模式
    let fault_recorder = Arc::new(
        mupc_data_processing::FaultRecorderImpl::new(&db_path).map_err(|e| {
            MupcError::new(
                ErrorCode::Unknown,
                format!("故障录波器初始化失败: {}", e),
                "startup",
            )
        })?,
    );
    coord.register_service("data_processing", ServiceStatus::Running);

    // ── 7. AI 引擎 ──
    tracing::info!("[07/14] 初始化 AI 引擎...");
    let ai_config = mupc_ai_engine::AiEngineConfig::default();
    let ai_engine = mupc_ai_engine::ModelManager::new(ai_config);
    // AI 引擎暂停（平台目标调整 2026-09-09）：不加载模型，ModelStatus 保持 Unloaded，
    // 本地策略引擎为唯一下发引擎。观测空间数据维度重构后再接回（届时恢复 load_models
    // + dispatch AI 分支 + rt_source 观测注入）。
    // ModelManager 实例保留：engine_status / web 状态查询返回 unloaded（如实，不谎报）。
    // 构建期事实 vs 配置项对账：`enable_npu` 曾是运行期开关，2026-09-19 起 NPU 改为
    // **构建期** `--features npu`；该配置项现今**无任何读取方**，若与构建事实不符必须显式
    // 告警，否则运维会照旧以为"配置里 enable_npu: true 就等于有 NPU"（排障时会白折腾）。
    if config.ai_engine.enable_npu != mupc_ai_engine::NPU_BUILD_ENABLED {
        tracing::warn!(
            "配置 .ai_engine.enable_npu={} 与本二进制不一致：本产物{}真实 NPU 推理。\
             NPU 是**构建期**开关，部署请用 `cargo build --features npu`（配置项不改变二进制能力）",
            config.ai_engine.enable_npu,
            if mupc_ai_engine::NPU_BUILD_ENABLED {
                "已编入"
            } else {
                "未编入"
            }
        );
    }
    let ai_engine = Arc::new(ai_engine);
    coord.register_service("ai_engine", ServiceStatus::Running);

    // ── 8. 策略引擎 ──
    tracing::info!("[08/14] 初始化策略引擎...");
    let mut ai_integrator = mupc_strategy_engine::AiIntegrator::new();
    // ⚠️ PCS 通道的注入点**不在这里**：`PcsHandle` 的采集出口是站级同一个 `SouthSink`，
    // 而 sink 持 `Arc<AiIntegrator>` ⇒ 句柄必然晚于本结构体的 `Arc::new`（Task 10 装配顺序
    // 约束，详见 `AiIntegrator::set_pcs_client` 的文档）。注入发生在下文 **4′. PCS 装配段**。
    // 南向设备（上行遥测采集共享）
    let pv_device = create_rs485_device("inverter", 0x01, "pv_inverter_001");
    let load_device = create_rs485_device("modbus", 0x02, "load_ctrl_001");

    // v2.16: 注入台区储能治理策略（AI 失效兜底，分相 P/Q 经核间下发）
    // v2.24: 容量档位加载（strategy.tai_config_file → TaiStorageConfig）；档位
    // 加载失败 = 启动中止（fail-fast），绝不静默落默认档进闭环。
    let tai_cfg = {
        let path = config.strategy.tai_config_file.trim();
        let opt = if path.is_empty() { None } else { Some(path) };
        mupc_strategy_engine::load_tai_storage_config(opt, None).map_err(|e| {
            MupcError::new(
                ErrorCode::ConfigError,
                format!("tai 档位加载失败: {e}"),
                "startup",
            )
        })?
    };
    // 额定有功上限（审查 R1-A1，2026-09-09）：取台区储能容量档 p_cap（电池功率上限 kW，
    // YAML 真值）——IEC104 主站 PowerRegulation/ChargeDischarge 外部指令 clamp 用。
    let p_max_kw = tai_cfg.p_cap;
    ai_integrator.set_tai_storage_strategy(Arc::new(
        mupc_strategy_engine::TaiStorageStrategy::new(tai_cfg),
    ));
    // 本地策略优先模式（YAML 配置：ai_engine.local_priority；单元 K 后**无**运行时切换端点）
    ai_integrator
        .set_local_priority(config.ai_engine.local_priority)
        .await;

    // v2.23: 注入 AI 指令安全校验器（安全闸门，dispatch 前校验 AI 指令，不通过降级本地兜底）
    ai_integrator
        .set_validator(Arc::new(mupc_strategy_engine::AiCommandValidatorImpl::new()))
        .await;

    // 审查 R1-A3（2026-09-09）：本地策略决策落库 decisions 表（web /decisions 审计回放）。
    // 回调注入保持 strategy-engine 无 storage 依赖；同步闭包内 tokio::spawn 异步落库。
    // phase_p/phase_q 为三相 [f64;3]（send_tai_command 下发语义，见 strategies.rs phase_p_set）。
    {
        let decision_store = storage.clone();
        ai_integrator
            .set_decision_sink(Arc::new(move |phase_p, phase_q| {
                let store = decision_store.clone();
                tokio::spawn(async move {
                    // scene_type="local_tai"; action_json={phase_p_set,phase_q_set}; confidence=1.0;
                    // model_version="local"; ts=now（AiDecisionRecord 实际字段，见 storage models.rs）
                    let rec = mupc_storage::AiDecisionRecord {
                        id: None,
                        timestamp: chrono::Utc::now(),
                        scene_type: "local_tai".to_string(),
                        action_json: serde_json::json!({
                            "phase_p_set": phase_p,
                            "phase_q_set": phase_q,
                        })
                        .to_string(),
                        confidence: 1.0,
                        model_version: "local".to_string(),
                    };
                    if let Err(e) = store.decisions.insert(&rec).await {
                        tracing::debug!("本地策略决策落库失败: {}", e);
                    }
                });
            }))
            .await;
    }

    // AI 引擎暂停（平台目标调整 2026-09-09）：不注入 model_manager——set_model_manager 会把
    // AiIntegrator.status 置 Ready，模型未加载时会导致 engine_status 谎报已启用。当前
    // AiIntegrator.model_manager=None → engine_status 如实报 unloaded / ai_engine_enabled=false。
    // 观测空间维度重构 + 恢复 load_models 后，在此恢复 set_model_manager 注入（届时 Ready 语义才正确）。
    let ai_integrator = Arc::new(ai_integrator);
    coord.register_service("strategy_engine", ServiceStatus::Running);

    // 告警即时投递环（提前创建，供联锁 major 事件 / 南向站事件 / 策略下发三个生产者共用）。
    //
    // ⚠️ **不是 F7 真源**（设计 §4.7）：F7 真源仍是 `storage.events`；本环只是「未落库也能上屏」
    // 的**可选增强**，是否与 `storage.events` 合并成 F7 的一路源属待裁项 **R-07**（本轮不裁）。
    // 容量取 `ALERT_FEED_CAPACITY`(=64)，不再沿用迁出前那个 `new(256)` 的 256——
    // 设计 §4.7 对最小形态**写死 64**。
    let alert_feed = Arc::new(crate::alert_feed::AlertFeed::new());

    // ── 03 设计 §9.3 缺口 1/2（FLS-03②）：遥测缓冲丢弃的**健康巡检** ──
    // 依赖：`write_buffer`(步骤 3) + `alert_feed`(上一步) 均已就绪 ⇒ 只能在**这里**起（不能再早）。
    // 职责：每 1 s 读 `dropped_points()/dropped_batches()` 的**增量**，增量 > 0 才投一条 `major`
    // 告警（含增量条数与累计值）⇒ 无增量即静默（连续失败时段不产告警风暴，缺口 2 由同一判据闭合）。
    // 分工理由（为什么不是 storage 自己发）：`storage` 无告警通道且**不应**依赖 core-bin
    // （依赖方向），故落点是装配层的"读增量 → 投既有 `AlertFeed`"。
    // 退出契约：与 `flush_timer`/`grid_agg_timer` **同名单**（`producers` 协作退出）——
    // 收到停机信号即退出，收工时不会再往环里投告警。**不得**改放 abort 名单。
    producers.0.push((
        "storage_health_timer",
        crate::storage_health::spawn_storage_health_timer(
            write_buffer.clone(),
            alert_feed.clone(),
            stop_rx.clone(),
        ),
    ));

    // ── IEC 104 服务器**实例提前构造**（步骤 9 只做 `start()`；Task 10 起**提前到本处**）──
    // HMI 的装置状态源（`SystemDeviceSource`）与 **PCS 采集出口**（`SouthSink`）都要它，而两者
    // 都在本行之后装配。实例构造**无 I/O**（只建连接表/通道），提前无副作用（原注释同款理由）。
    let iec104_server = Arc::new(mupc_gateway::iec104::server::Iec104Server::new(
        mupc_gateway::iec104::server::Iec104Config {
            listen_addr: config.gateway.listen_addr.clone(),
            listen_port: config.gateway.listen_port,
            ..Default::default()
        },
    ));

    // ── 01 设计 §9.1.8：外设遥测最新值快照（**在此提前构造**；Task 10 起再上移到本处）──
    //   U-73（12 号设计 §15.1.1）把 `latest_values` 定为外设段（慢拍 D）的**唯一取数面**，
    //   而外设源必须随 `DisplayDataProvider` 一同装配 ⇒ 快照的构造点须早于 HMI 装配；
    //   Task 10 起还须早于 **PCS 装配段**（PCS 采集出口即它的写入方之一）。
    //   **语义不变**：本项仍是 §9.4 序 1 的"第一步构造"，早于其全部读取方
    //   （HMI 慢拍 D / IEC104 上送驱动器 / 总召 / `SouthSink` 写入方）。
    //   `stale_timeout_s` 由配置注入 ⇒ 过期判据单一真源（消费方不得另立门限，LV-3）。
    let latest = Arc::new(mupc_data_processing::latest_values::LatestValues::new(
        config.south_stations.stale_timeout_s,
    ));

    // ── 4′. PCS（南向，设计 §13 / ADR-016；PCS 的**完整所有者** = `southd::pcs::PcsHandle`）──
    //   ⚠️ **位置约束（两条须同时成立）**：① 在 `SouthSink` 构造**之后**（PCS 采集出口与站级
    //   同源 —— 故 sink 的构造上提到本段，站级调度器下文改为克隆同一 `Arc`）；② 在
    //   `display.enabled` 装配块**之前**（HMI 的 `DisplayDataProvider`/`SystemDeviceSource`
    //   要持同一个 `Arc<PcsHandle>`）。sink 无自身可变状态（全是下游依赖的 `Arc` 克隆）⇒
    //   "一个实例两处用"与"两处各建一个"行为等价，取前者以免下游被注册两次。
    let south_sink: Arc<SouthSink> = Arc::new(SouthSink::new(
        ai_integrator.clone(),
        write_buffer.clone(),
        storage.events.clone(),
        alert_feed.clone(),
        iec104_server.clone(),
        latest.clone(),
        // `on_grid_package(pkg)` 契约不含站 id ⇒ 装配期解析 grid 站 id（未配则 None）
        config.south_stations.grid_station().map(|s| s.id.clone()),
        grid_aggregator.clone(),
        agg_tx.clone(),
    ));
    let pcs: Option<Arc<mupc_southd::pcs::PcsHandle>> = if config.south_pcs.enabled {
        tracing::info!(
            "[04′] 初始化 PCS 通道: {} @{} slave={}（采集周期 {} ms）",
            config.south_pcs.port,
            config.south_pcs.baud_rate,
            config.south_pcs.slave,
            config.south_pcs.interval_ms
        );
        // 口打开：PCS **独占**该口（规则 P-2），故不走站级 `buses` 去重表。
        // ⚠️ 打开失败 = **拒启动**（与站级"失败口 offline 隔离、不阻断启动"不同）：PCS 是
        // 安全链的执行端，静默降级为"永远离线"会让联锁停机无原语、AI/策略下发无处可去。
        let bus: Arc<dyn mupc_southd::port_runtime::StationBus> = {
            let conf = south_pcs_bus_conf(&config.south_pcs);
            // 口层控制面三项（超时/数据位/停止位）经 `PortParams` 显式传给口层 —— 它们
            // **不在** `StationConf` 里（见 `south_pcs_port_params` 文档）；不接线就是死配置。
            let b = mupc_southd::port_runtime::Rs485PortBus::open_with_port_params(
                &conf,
                south_pcs_port_params(&config.south_pcs),
            )
            .map_err(|e| {
                MupcError::new(
                    ErrorCode::ConfigError,
                    format!("south_pcs 口 {} 打开失败: {e}", config.south_pcs.port),
                    "startup",
                )
            })?;
            Arc::new(b)
        };
        let h = mupc_southd::pcs::PcsHandle::new(config.south_pcs.clone(), bus, south_sink.clone());
        // 采集循环句柄入 guard（abort 名单；无停机钩子，与站级采集 task 同范式）
        guard.0.push(h.spawn_collection_loop());
        coord.register_service("pcs", ServiceStatus::Running);
        // 策略引擎持同一句柄（双参数 / 分相下发 / SOC 回落活读）
        ai_integrator.set_pcs_client(h.clone());
        Some(h)
    } else {
        tracing::info!(
            "[04′] south_pcs.enabled=false ⇒ 不启用 PCS 通道（行为与迁移前同：无 PCS 主链路）"
        );
        None
    };

    // ── S2 §12.4 / Task7：安全联锁控制器（io.enabled 时装配）──
    // 依赖：**PCS 通道(4′ 段)** + storage(步骤 3) + alert_feed 均已就绪（Task 10 起停机原语
    // 由 `intercore` 改经 `PcsHandle`）。GPIO(sysfs) 打开失败由 `InterlockController::new`
    // 内部 fail-safe（预置 latch，绝不静默无 latch 运行）。disabled 时不装配 → 读通道走
    // `InterlockWiring::Disabled`（未启用部署行为不变）。
    let interlock_ctl: Option<Arc<crate::interlock::InterlockController>> = if config.io.enabled {
        // ⚠️ fail-fast：io.enabled 的全部停机原语（stop / restore_latched / last_run_state /
        // authorize_restart）都经 `PcsHandle`。`south_pcs.enabled=false` ⇒ **无停机原语**，
        // 联锁会退化成"能触发却停不了机"的假安全形态 ⇒ 启动即报错，绝不放行。
        let port: Arc<mupc_southd::pcs::PcsHandle> = pcs.clone().ok_or_else(|| {
            MupcError::new(
                ErrorCode::ConfigError,
                "io.enabled 需要 south_pcs.enabled=true（联锁停机原语经 PcsHandle；无 PCS 通道时停机无执行端）",
                "startup",
            )
        })?;
        let il = Arc::new(crate::interlock::InterlockController::new(
            config.io.clone(),
            Box::new(port), // Arc<PcsHandle> → InterlockPort
            storage.events.clone(),
            alert_feed.clone(),
        ));
        tracing::info!("安全联锁已启用（io.enabled=true），装配联锁控制器");
        Some(il)
    } else {
        tracing::debug!("安全联锁未启用（io.enabled 缺省/false）");
        None
    };
    // DB 读回 + 常驻轮询（首 dispatch 前恢复 latch；restore 后即由 run_loop 维护）
    if let Some(il) = interlock_ctl.clone() {
        il.restore_from_db().await;
        guard.0.push(tokio::spawn(il.clone().run_loop()));
    }
    // 联锁后端（**唯一签名**：契约 `mupc_display_proto::InterlockApi`）。单元 K 删除了
    // 旧 web-api trait 与那个过渡适配器（单元 K 已删）⇒ 两个消费点（控制通道两条写端点、
    // 读通道 `display_host::interlock_wiring_for`）现在**吃同一个 `Arc<dyn>`**。
    let interlock_backend: Option<Arc<dyn mupc_display_proto::InterlockApi>> = interlock_ctl
        .clone()
        .map(|c| c as Arc<dyn mupc_display_proto::InterlockApi>);

    // AI 决策循环：周期执行决策并分发到核间/南向（RL 决策 <1s）
    // Task7：联锁 latch 期间抑制 dispatch（skip 本轮 warn；transport 层另有 stopped_latched 兜底）
    let decision_integrator = ai_integrator.clone();
    let decision_alert_feed = alert_feed.clone();
    let decision_interlock = interlock_ctl.clone();
    guard.0.push(tokio::spawn(async move {
        // 遗留待办 A（2026-09-09）：latch 释放边沿检测——上一拍联锁锁存中、本拍已释放时，
        // 需清 TaiStorage 节流缓存强制一拍重发（互锁抑制期 skip 后目标值未变也不会重发，PCS
        // 会保持停机态停等）。was_latched 为循环局部状态，不跨 await 持有。
        let mut was_latched = false;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            if decision_interlock
                .as_deref()
                .is_some_and(|c| c.is_latched_now())
            {
                tracing::warn!("安全联锁锁存中：跳过本周期 AI/策略下发（dispatch 抑制）");
                was_latched = true;
                continue;
            }
            if was_latched {
                // 联锁释放边沿：清 TaiStorage 节流缓存，本拍重发当前控制目标（PCS 恢复控制）
                tracing::info!("安全联锁解除：强制一拍重发当前控制目标");
                decision_integrator.reset_last_sent_tai().await;
                was_latched = false;
            }
            if let Err(e) = decision_integrator.dispatch_ai_decision().await {
                tracing::debug!("AI 决策周期失败: {}", e);
            } else {
                // 推送策略下发事件（`AlertFeed` 生产者；AI 引擎已停用 2026-09-09，每拍实际为本地
                // 台区储能治理 run_fallback_strategies 下发——沿用同一环，仅文案中性化）
                decision_alert_feed.push_strategy_dispatch("策略下发完成");
            }
        }
    }));

    // ── 12-本地显示终端数据提供层（12-显示终端 设计 §4.2 装配点：策略引擎(第8步)+决策循环之后）──
    // config.display.enabled 时：先起 DisplayDataProvider（1s 采集组帧，SOC 取 AiIntegrator 裁决
    // 快照、三相/run_state 取 **PCS 通道**），再起 LoopbackHttpPublisher（127.0.0.1 GET 最新帧）。
    // 两 handle 都入 guard（优雅退出随其它后台任务 abort）；主进程不 spawn/不管理渲染子进程
    // （渲染生命周期归 systemd，§4.2/§11）。disabled 不装配（warn）。
    // （IEC 104 服务器实例与 `latest` 快照已在**上文 PCS 装配段之前**构造 —— PCS 采集出口
    //   与 HMI 装置状态源都要用它们，见那两处的注释。）

    // ── U-73 §15.11 #7 / §15.2.2：消防钢瓶气压取数接缝（EDGE-23 / EX-11）──
    //   在本文件的**HMI 装配之前**建空壳（`attach` 前恒 `None` = "接缝未接线"），
    //   等下方 `SouthScheduler::new` 之后再 `attach`（原因见适配器的类型文档）。
    //   ⚠️ 这段与 `latest` 分开的原因：`latest` 是先构造后接线也**必须**的分叉点，
    //   而本接缝的"空壳 → attach"是**装配顺序**的产物，两者不是同一类约束。
    let cylinder_query = Arc::new(SouthCylinderPressureQuery::new(&config.south_stations));

    if config.display.enabled {
        // 设计 §4.9 字面稿的「初始化本地 HMI 后端」日志行（第一轮整改 S-3：原先只存在于设计里，
        // 实现无对应日志 ⇒ 现场无法从启动日志确认 HMI 后端是否真的在装配）。
        // ⚠️ **不带步号**：设计字面稿写的是 `[10/14]`，而实现里 HMI 装配**并入步骤 8 之后**
        // （`[10/14]` 现为 OTA 管理器）⇒ 带号会与现行 14 步编号体系冲突（登记见
        // `docs/technical-debt.md` U-38）。
        tracing::info!("初始化本地 HMI 后端（读通道 + 控制通道）...");
        // §7.3 warn：无 PCS 3 区通道（`south_pcs.enabled=false`）时可看 SOC/通道，三相
        // 1022-1032 将 NotRead。Task 10：判据真源由 `intercore.transport` 改为 `south_pcs.enabled`
        // （PCS 迁入南向后它才是"有无 PCS 3 区点表"的唯一真源）。
        if !config.south_pcs.enabled {
            tracing::warn!(
                "display.enabled=true 但 south_pcs.enabled=false（无 PCS 通道）：可看 SOC/通道，三相 1022-1032 将 NotRead（12-显示终端 §7.3）"
            );
        }
        // ⚠️ 命名：本变量是**帧共享存储**，与上面的 `latest`（`latest_values` 快照）**不同物**
        // （U-73 起两者在本函数内同时可见）⇒ 显式区分名，避免误用（设计 §15.2.2 的同款提醒）。
        let shared_frame: Arc<std::sync::Mutex<Option<mupc_display_proto::DisplayFrame>>> =
            Arc::new(std::sync::Mutex::new(None));
        // 慢拍四段源（设计 §4.2）：
        // - F6 装置状态：intercore 链路 + 控制源 + 本机温度/内存（uptime 零点取**进程启动时刻**，
        //   由 `main()` 入口最顶部注入，见 `process_started_at`——不用本装配点时刻）；
        // - F7 告警：storage.events（设计 §4.1 #3 裁决 D11，「最近 10 条、时间倒序」）；
        // - F16 联锁：三分支**互斥**、语义不得互替（判定抽到
        //   `display_host::interlock_wiring_for` 这一**纯函数**并由用例钉死）——
        //   已接线 / `io.enabled=true` 却没接上（**不可用**）/ `io.enabled=false` 的
        //   **已知状态**「功能未启用」。
        let interlock_wiring = crate::display_host::interlock_wiring_for(
            interlock_backend.clone(),
            config.io.enabled,
            config.io.release_hold_secs,
        );
        // U-73（12 号设计 §15.11 #6/#8）：外设段计划（配置 → 白名单投影）+ 点表目录 + 数据源。
        // - 计划：**装配期一次算**（纯函数 `peripheral_plan`），运行期只做内存读（帧路径零 I/O）；
        // - 目录：与帧内 `catalog_rev` **同源同值**（同一份 `Arc<PeripheralCatalog>` 既进
        //   控制通道三端点、也提供帧内 `catalog_rev`）——两处各建一份会让屏侧永远在重取；
        // - 源：`latest_values` 的**公开只读面**投影（禁 DB / 禁第二真源，§15.1.2 C-1/C-5）。
        let periph_plan = crate::display_host::peripheral_plan(&config.south_stations);
        let periph_catalog = Arc::new(crate::console_host::build_peripheral_catalog(
            &config.south_stations,
            &periph_plan,
            chrono::Utc::now().timestamp_millis().max(0) as u64,
        ));
        let periph_source: Arc<dyn crate::display_host::PeripheralSource> = Arc::new(
            crate::display_host::StationPeripheralSource::new(
                latest.clone(),
                periph_plan.clone(),
                periph_catalog.rev,
            )
            // §15.11 #7：钢瓶气压接缝（消防站的 EDGE-23「未配置」由此可达）
            .with_cylinder_query(cylinder_query.clone()),
        );
        tracing::info!(
            "外设段已接线：{} 站 / catalog {} 条（rev={}）/ 兜底 tick {} ms",
            periph_plan.len(),
            periph_catalog
                .stations
                .iter()
                .map(|s| s.blocks.iter().map(|b| b.points.len()).sum::<usize>())
                .sum::<usize>(),
            periph_catalog.rev,
            config.display.periph_poll_ms,
        );
        let provider = crate::display_host::DisplayDataProvider::new(
            ai_integrator.clone(),
            // 三相/run_state/连接态取数面 = PCS 通道（`south_pcs.enabled=false` ⇒ `None`）
            pcs.clone(),
            &config.display,
            // `modbus_transport`（三相缺段读 `Offline` 而非 `NotRead`）的真源 = 有无 PCS 通道
            config.south_pcs.enabled,
            shared_frame.clone(),
        )
        .with_slow_sources(
            Some(Arc::new(crate::display_host::SystemDeviceSource::new(
                pcs.clone(),
                ai_integrator.clone(),
                // IEC 104 链路真源（U-59 / L-5）：实例已在上文提前构造（**尚未** start()，
                // 故此刻读得「未配置」，start() 后自动转「断开/连接中/已连接」）。
                Some(iec104_server.clone()),
                process_started_at,
            ))),
            Some(Arc::new(crate::display_host::StorageAlarmSource::new(
                storage.events.clone(),
                config.display.alarm_page_size,
            ))),
            interlock_wiring,
        )
        .with_peripheral_source(periph_source.clone());
        guard.0.push(tokio::spawn(provider.run()));
        let publisher = crate::display_host::LoopbackHttpPublisher::new(shared_frame.clone());
        match tokio::net::TcpListener::bind(&config.display.bind_addr).await {
            Ok(listener) => {
                tracing::info!(
                    "本地显示终端数据通道已启动: http://{}{}（回环仅本机）",
                    config.display.bind_addr,
                    crate::display_host::LATEST_PATH
                );
                guard.0.push(tokio::spawn(publisher.serve(listener)));
            }
            Err(e) => {
                // bind 失败不阻断 mupcd 启动（回环端口占用属本机配置问题，留 trace 排查）
                tracing::error!(
                    "display 回环绑定 {} 失败: {}——跳过发布端（确认 bind_addr 未被占用）",
                    config.display.bind_addr,
                    e
                );
            }
        }

        // 10.2 控制通道（设计 §4.9；单元 G-1：**只实现** `GET /v1/console/config`，
        // 其余 7 条契约端点已登记路由但回 501——见 console_host.rs 模块头「未做的部分」）。
        // 与读通道**并行不冲突**：不同端口（9811 vs 9810）、不同 listener、不同 task。
        // 配置读源 = `core_config` 内存副本（设计 D10：内存副本 + 原子落盘），**不**在本点重读 yaml。
        // ⚠️ **必须**经 `console_config_source()` 取源——它保证"控制通道与装配共用**同一个** `Arc`"
        // （唯一真源）。**不得**在此内联 `Arc::new(RwLock::new(core_config.read().await.clone()))`
        // 之类的**深拷贝**：那样能编译、能过其余用例，却会让 G-2 的写入在屏上**静默失效**
        // （装配写 A、控制通道读 B）。该不变式由单测
        // `console_host::tests::console_config_source_is_arc_identical_to_assembly_handle` 钉死。
        // 10.2' 配置写路径（G-2）。**审计先建**：建不起来 ⇒ 写路径整体 `Unavailable`
        // （fail-closed：宁可写路径整体不可用，也不给一个"没有审计的写路径"——
        // 设计 §3.3：T-3 无登录后审计是唯一操作凭据）。
        // 装配走 [`console_write_paths`]（fail-closed 的唯一裁决点，独立成函数 ⇒ 可单测）。
        // **一次性**返回配置写源与联锁写源：两者**共用同一个 `FileAuditSink`**（哈希链唯一实例，
        // 见该函数的"为什么必须共用"）。
        let (apply, interlock_ops) = console_write_paths(
            &config.system.log_dir.join("audit"),
            config_path,
            core_config,
            log_reload,
            interlock_backend.clone(),
        );
        // 单元 H：日志源 = **日志目录扫描**（设计 §4.4）。目录取自 `config.system.log_dir`
        // （与迁出前的 `web-api::LogsHandler`、以及审计目录 `{log_dir}/audit` 同一个真源）。
        // ✅ 与 **写者**（`main.rs` 的 `tracing_appender::rolling::daily`）**同值**：`--log-dir` 是
        // 可选覆盖，`main.rs` 在 Phase 1 之后把它**写回** `config.system.log_dir` 再往下传
        // （单一真源，R2 整改；见 `log_service.rs` 模块头「日志目录的单一真源」）。
        // 限额取 `config.display.log`（设计 §8.3「log 限额」的真源）。
        let logs = crate::console_host::LogSource::Ready(Arc::new(
            crate::log_service::LogService::new(config.system.log_dir.clone(), config.display.log),
        ));
        // 单元 I：审计**查询**源（设计 §4.5 / F19）。
        // 目录与写侧 `FileAuditSink`（上面 `console_write_paths` 的 `audit_dir` 入参）**同一个值**
        // （`{system.log_dir}/audit`）——两侧不同值会让屏上查到的是**别的目录**（静默失实，
        // 与 R2 整改的日志目录同款风险）。构造**不做 I/O**⇒ 恒 `Ready`；"读不出来"在**请求期**
        // 以 `AuditPage{available:false}` 表达（EDGE-17），不必也不该在此把控制通道打挂。
        let audit = Arc::new(crate::console_audit::ConsoleAuditService::new(
            config.system.log_dir.join("audit"),
        ));
        let console = crate::console_host::ConsoleHost::new(crate::console_host::ConsoleDeps {
            config: console_config_source(core_config),
            apply,
            logs,
            interlock: interlock_ops,
            audit,
            // U-73 §15.3.2：外设三只读端点的数据源（**与帧内同一份段**）。
            peripherals: crate::console_host::PeripheralConsoleSource::Ready {
                source: periph_source,
                catalog: periph_catalog,
            },
            // U-73 §15.11 #4（评审 T20 (B) D5 的"静默空转"收口）：`fire_detectors` 的默认页
            // 大小取自**配置**——这是该键在全仓的**唯一消费点**（改键 ⇒ 端点的缺省分页真的变）。
            periph_page_size: config.display.periph_page_size,
        });
        match tokio::net::TcpListener::bind(&config.display.control_bind_addr).await {
            Ok(listener) => {
                // 回环裁决在 `ConsoleHost::serve` 内按**实际绑定结果**再判一次（PL-4 二次兜底）：
                // 配置层 `CoreConfig::validate_display` **已**调用契约的 `DisplayConfig::validate()`，
                // 两条地址（bind_addr / control_bind_addr）在启动期即强制字面量回环、端口≠0、两址不同；
                // 本处兜底防的是"配置校验被绕过 / 被新增调用路径跳过"。
                guard.0.push(tokio::spawn(async move {
                    if let Err(e) = console.serve(listener).await {
                        tracing::error!("控制通道 serve 异常退出（非回环地址会被拒绝）: {}", e);
                    }
                }));
            }
            Err(e) => {
                // 与读通道同口径：bind 失败不阻断 mupcd 启动（本机端口占用属配置问题）
                tracing::error!(
                    "控制通道绑定 {} 失败: {}——跳过控制通道（确认 control_bind_addr 未被占用）",
                    config.display.control_bind_addr,
                    e
                );
            }
        }
        // 单元 K：本服务名由迁出前那个 web 服务名改名而来（`web-api` crate 已整体删除）。
        // 注册位置随之从"步骤 10 的 Web API 装配"移到**本地 HMI 后端**的装配点——若仍留在原处，
        // 会在 `display.enabled=false` 时谎报"HMI 后端已运行"。
        coord.register_service("hmi_backend", ServiceStatus::Running);
    } else {
        tracing::debug!(
            "本地显示终端未启用（config.display.enabled=false），跳过 DisplayDataProvider/回环发布"
        );
    }

    // ── 01 设计 §9.1.8 / §9.4 序 1+2：外设遥测最新值快照 + 上送点表（机械生成）──
    //   `stale_timeout_s` 由配置注入 ⇒ 过期判据单一真源（消费方不得另立门限，LV-3）；
    //   构造点在网关 / 南向调度装配**之前**——写入方 = 下方 `SouthSink`，读取方 = IEC104
    //   上送驱动器（序 5）与 `StrategyCommandHandler` 总召/初始快照（序 6）。
    // `latest` 已在**本地 HMI 装配之前**构造（上文；U-73 慢拍 D 需要同一实例），此处不再重建。
    // §9.4 序 2：上送点表机械生成（`build_uplink_points`），失败 ⇒ **拒启动**（配置/点表
    // 漂移的 fail-fast，与 `validate_south_stations` 同范式，§9.2.1）。
    // ⚠️ Task 10：**显式传入 PCS 段**（`Some(&config.south_pcs)`）。PCS 迁出站级段后若不喂它，
    // 72 个 PCS IOA 会**静默**从 IEC104/MQTT 点表消失，且生成期自检不会响（`has_pcs == false`
    // 时期的期望值恰为 567 = 5 站并集，与"5 站本就无 PCS"的合法形态不可分）——签名扩成
    // 显式入参就是让**编译器**强制每个调用点表态（设计 §13.9 末要求①②）。
    let uplink_points = Arc::new(
        mupc_southd::uplink::build_uplink_points(&config.south_stations, Some(&config.south_pcs))
            .map_err(|e| {
            MupcError::new(
                ErrorCode::Unknown,
                format!("上送点表生成失败（拒启动）: {e}"),
                "startup",
            )
        })?,
    );
    // §9.2.1 启动期产物：点表 JSON 落 `system.data_dir/uplink_points.json`（供 RC-U74-02
    // 与主站逐点对点；**不得手写 IOA 常量**）。落盘失败**不**拒启动——内存点表才是上送真源，
    // JSON 只是离线对点产物，故只 error 观测（与 `validate` 的 fail-fast 区分）。
    {
        let json_path = config.system.data_dir.join("uplink_points.json");
        let total = uplink_points.len();
        let iec104_n = uplink_points
            .iter()
            .filter(|p| p.channels.has(mupc_southd::uplink::ChannelMask::IEC104))
            .count();
        match crate::uplink::write_uplink_points_json(&json_path, &uplink_points).await {
            Ok(()) => tracing::info!(
                "上送点表已生成：共 {} 条 / IEC104 子集 {} 条 → 落 {}",
                total,
                iec104_n,
                json_path.display()
            ),
            Err(e) => tracing::error!(
                "uplink_points.json 落盘失败（不阻断启动；内存点表为真源）{}: {}",
                json_path.display(),
                e
            ),
        }
    }

    // ── 9. IEC 104 网关 ──
    tracing::info!("[09/14] 初始化 IEC 104 网关...");
    // 审查 R2-A2 (2026-09-09)：北向监听地址/端口读 config.gateway 段（缺省 0.0.0.0:2404，
    // 见 core_config.rs GatewayConfig::default——不再硬编码 2404）。
    // 实例本身已在 HMI 装配点**提前构造**（那里要取 `link_state()` 句柄，见该处注释）；
    // 本步只做「告知监听地址 + 起 start()」。
    tracing::info!(
        "IEC 104 监听 {}:{}",
        config.gateway.listen_addr,
        config.gateway.listen_port
    );
    let cmd_handler = Arc::new(StrategyCommandHandler {
        pcs: pcs.clone(),
        interlock: interlock_ctl.clone(),
        p_max_kw,
        // §9.4 序 6：总召 / 连接初始快照数据源（与上送驱动器同一份快照 + 点表）
        latest: latest.clone(),
        points: uplink_points.clone(),
    });
    let server_clone = iec104_server.clone();
    guard.0.push(tokio::spawn(async move {
        if let Err(e) = server_clone.start(cmd_handler).await {
            tracing::error!("IEC 104 服务器异常退出: {}", e);
        }
    }));
    coord.register_service("gateway", ServiceStatus::Running);

    // ── 01 设计 §9.4 序 5：IEC 104 上送驱动器（A/B/C 三档任务；A/B 周期、C 变位 COS）──
    //   持同一份 `latest` + `uplink_points`；三任务无停机钩子（不写存储、在途无未落盘批次）
    //   ⇒ 入 abort 名单（`guard`），与网关/指标采集同范式。档位过滤表驱动（读 `UplinkPoint.class`）。
    let uplink_driver = Arc::new(crate::uplink::Iec104UplinkDriver::new(
        latest.clone(),
        uplink_points.clone(),
        iec104_server.clone(),
        // A/B 档周期（§9.2.2「周期须可配置」的注入点；当前用缺省 1000/5000 ms，
        // 后续可改读 config——本任务不动 core_config schema）。
        crate::uplink::DEFAULT_CLASS_A_INTERVAL,
        crate::uplink::DEFAULT_CLASS_B_INTERVAL,
    ));
    for h in uplink_driver.spawn() {
        guard.0.push(h);
    }

    // ── S3 §10.3：策略 phase 源装配（master_meter 段已删除收敛，2026-09-09 S3b-1c）──
    //   B. south_stations.stations 非空 → southd scheduler：grid 单写 AiIntegrator。
    //      B1. 含 meter_grid → grid_on=true（SouthSink.on_grid_package 单写；offline 由事件 +
    //          AiIntegrator 5s 闸门判断，pv/load 不兜底 set_latest_data——保持 "grid 单写方"）。
    //      B2. 仅非 grid 站（无 meter_grid）→ grid_on=false → pv/load 南向模拟兜底测量（无 grid
    //          源时 AiIntegrator 不断供）。
    //   C. 无 stations → grid_on=false → pv/load 南向模拟兜底。
    // grid_on = 策略 phase 源可用（决定下方 pv/load 南向模拟 task 是否 set_latest_data；
    // M-4 防双写方并存：grid 源在即南向模拟不覆盖；无 grid 源则南向模拟兜底测量）。
    // `latest`（最新值快照，01 设计 §9.1.8）已在步骤 9 之前构造（§9.4 序 1，上送驱动器与
    // 总召共用同一实例）——此处不再重建，下方 `SouthSink` 经 `latest.clone()` 注入为写入方。
    let mut grid_on = false;
    if !config.south_stations.stations.is_empty() {
        // B. southd 路径（stations 非空即装配——非 grid 站 battery/hvac/fire telemetry/状态
        //    事件也必须采集落库，不得因缺 grid 源整体不启）。
        if config.south_stations.grid_station().is_none() {
            tracing::warn!(
                "south_stations 配置了 {} 个站但无 meter_grid：非 grid 站 telemetry 将采集，策略 phase 由南向模拟/无 grid 源兜底（S3b 语义点表前）",
                config.south_stations.stations.len()
            );
        }
        // ── S3b-3（T8）C8 配置异味提示的**发射点**（PRD §10.6 第 8 条；设计 §12.7 末段）──
        //   判定是纯函数 `mupc_southd::config::block_interval_hints`（站内全部块都声明了
        //   `interval_ms` ⇒ 站级 `interval_ms` 已不再描述实际节奏；**不拒配置**）。
        //   为何在**此处**发射而非配置期：`CoreConfig::validate` 属 main **Phase 1**（`main.rs:105`），
        //   早于 `tracing_subscriber::try_init()`（**Phase 2**，`main.rs:164`）⇒ 配置期发射的
        //   日志**无订阅者、被直接丢弃**（同 `core_config.rs:511-512` 的成文约定：判定放配置期、
        //   发射放 startup 装配期）。本处是"南向配置 → 调度器装配"的入口，日志与装配上下文同批。
        for h in mupc_southd::config::block_interval_hints(&config.south_stations) {
            tracing::debug!("{}", h);
        }
        // sink 与 PCS 通道**共用同一实例**（构造已上提到 PCS 装配段；见那处的注释）
        let sink = south_sink.clone();
        // 每口 open 一次 Rs485PortBus：按 port 去重。open 失败口不入 map → 该口全站走
        // offline 事件隔离（§10.7 不阻断启动）。口单 poller、站级隔离由 scheduler 负责。
        let mut buses: HashMap<String, Arc<dyn mupc_southd::port_runtime::StationBus>> =
            HashMap::new();
        let mut seen_ports: Vec<String> = Vec::new();
        for st in &config.south_stations.stations {
            if seen_ports.contains(&st.port) {
                continue;
            }
            seen_ports.push(st.port.clone());
            match mupc_southd::port_runtime::Rs485PortBus::open(st) {
                Ok(bus) => {
                    buses.insert(st.port.clone(), Arc::new(bus));
                }
                Err(e) => tracing::warn!(
                    "southd 口 {} 打开失败（该口站 offline 隔离，不阻断启动）: {}",
                    st.port,
                    e
                ),
            }
        }
        let opened = buses.len();
        let cfg_ports = seen_ports.len();
        let station_count = config.south_stations.stations.len();
        let scheduler =
            mupc_southd::scheduler::SouthScheduler::new(config.south_stations.clone(), buses, sink);
        // §15.11 #7：调度器已建 ⇒ 立刻接上钢瓶气压接缝（此前一律 `None` = "接缝未接线"）。
        // 与 HMI 装配**解耦**：`display.enabled=false` 时本接缝无人消费，`attach` 仍执行
        // （幂等且零 I/O，不做条件分支可少一条"配置组合 × 接线状态"的分叉）。
        cylinder_query.attach(scheduler.clone());
        let handles = scheduler.spawn();
        let handle_count = handles.len();
        // grid_on 仅 grid 源配置存在才 true（B1）；B2（只非 grid 站）false → pv/load 兜底
        grid_on = config.south_stations.grid_station().is_some();
        // 观测：句柄全部入 TaskGuard（Phase 6 优雅退出 abort）。口 task panic 静默停采该口
        // 的完整观测/重建 supervisor 留 TODO（Task 7 决议：装配期不引入，仅持有句柄不裸丢）。
        for h in handles {
            guard.0.push(h);
        }
        tracing::info!(
            "southd 已装配：{} 站 / 去重配置 {} 口，open 成功 {}/{} 口（失败口站 offline 隔离、runner 以 bus=None 暂停采集）→ {} 条采集 task（{}）",
            station_count,
            cfg_ports,
            opened,
            cfg_ports,
            handle_count,
            if config.south_stations.grid_station().is_some() {
                "grid 源单写 AiIntegrator，非 grid 遥测/事件经 SouthSink 落库"
            } else {
                "无 meter_grid：仅采非 grid telemetry/事件，策略 phase 由 pv/load 南向模拟兜底"
            }
        );
    } else {
        tracing::warn!(
            "未装配 south_stations（无 grid 源）：策略 phase 由 pv/load 南向模拟兜底——若现场应有总表数据，请确认配置文件已收敛 south_stations.meter_grid（master_meter 段已删除，S3b-1c）"
        );
    }

    // 南向数据采集循环（上行）：读取 → 转换 → 持久化（+ 无 grid 源时兜底注入 AiIntegrator）。
    // ⚠️ 原本还有一条 IEC104 假遥测上送支路，已按 §9.4 序 10 删除（见循环内注释）。
    // AI 观测注入已停（平台目标调整 2026-09-09：观测维度重构前停采）。
    if let (Some(pv), Some(load)) = (pv_device.clone(), load_device.clone()) {
        let wb = write_buffer.clone();
        let ai_int = ai_integrator.clone();
        // U-64：本环是**遥测缓冲的生产者**之一（另一处是 `SouthSink`）⇒ 入协作退出名单：
        // 收到停机信号即结束本轮、把在途写入做完，而不是被 abort 在半路。
        let mut stop = StopSignal {
            rx: stop_rx.clone(),
        };
        producers.0.push((
            "south_sim_loop",
            tokio::spawn(async move {
                let grid_on = grid_on; // grid 策略源在时（southd 含 meter_grid）由它提供，南向模拟不覆盖
                loop {
                    // 停机信号与 1 s 节拍二选一：先到先执行（信号到了就收工，不再采新点）
                    tokio::select! {
                        _ = stop.stopped() => break,
                        _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {}
                    }
                    for (dev, name) in [(&pv, "pv_inverter_001"), (&load, "load_ctrl_001")] {
                        match dev.read() {
                            Ok(frame) => {
                                let pkg = dataframe_to_datapackage(&frame);
                                // 注入遥测到 AiIntegrator（grid_on：southd meter_grid 策略源在，
                                // 南向模拟不覆盖——M-4 防双写；无 grid 源时南向模拟兜底测量）
                                if !grid_on {
                                    ai_int.set_latest_data(pkg.clone()).await;
                                }
                                for point in datapackage_to_telemetry_points(&pkg, name) {
                                    if let Err(e) = wb.buffer_telemetry(point).await {
                                        tracing::debug!("遥测写入失败: {}", e);
                                    }
                                }
                                // 01 设计 §9.4 序 10 / §9.7 C-15：原本此处还有一条 IEC104
                                // 假遥测上送支路（以自增的点表外 IOA、固定序号组 I 帧广播），
                                // 已**删除**——它与段 1（总表 IOA 1–6）语义相撞、破坏 AC-U74-01。
                                // pv/load 是 `create_rs485_device` 造的模拟设备，**不在** `south_stations`
                                // ⇒ 无点表 IOA；北向真值上送统一由 `Iec104UplinkDriver`（读快照）承担。
                                // **保留** `set_latest_data` 注入与 `buffer_telemetry` 落库两条非 IEC104 职责。
                            }
                            Err(e) => tracing::debug!("南向采集 {} 失败: {}", name, e),
                        }
                    }
                }
            }),
        ));
    }

    // ══ 10. OTA 管理器（原「10. Web API」装配整块已随 `web-api` crate 删除，单元 K）══
    //
    // **处置口径（设计 §7.3 末段备注 / 待裁项 R-16，逐字照办）**：
    // 迁出前 `ota_manager` 的**唯一消费者**是 web-api 的 `AppState.ota_manager`；crate 删除后
    // 该实例失去消费者。本单元**保留 `ota_update` 服务注册与实例（不删除能力），但不启动任何
    // 服务面**——即在后续 OTA 需求里重新接线即可，不必重新发明实例构造。
    //
    // ⚠️ **绝不可**因此连带删除 `mupc-ota-update` crate：那是能力删除，不在本单元授权范围内。
    tracing::info!("[10/14] 初始化 OTA 管理器...");
    // ⚠️ 这里用 `OtaManagerImpl::new`（**不传策略引擎回调**）⇒ 自动回滚完成后策略引擎不会
    // 被告知加载旧模型（设计 §2.9.2 第 5 步"重启策略引擎加载旧模型"）。原因：策略引擎侧
    // 目前**没有**模型重载/通知入口（`AiIntegrator` 只有 `set_model_manager`），凭空接线会
    // 造出一个无消费者的空壳。**管理器侧的回调转发已修好并有用例钉住**（`with_callbacks`
    // 会同份转发给 applicator 与 rollback，见 `ota-update/src/manager.rs` 的
    // `rollback_notifies_strategy_engine_when_callback_supplied`）⇒ 待策略引擎补上重载 API
    // 后，这里改用 `with_callbacks(..., Some(cb))` 即可（缺口登记见 U-63）。
    let ota_manager: Arc<dyn mupc_ota_update::OtaManager> = Arc::new(
        mupc_ota_update::manager::OtaManagerImpl::new(
            mupc_ota_update::OtaConfig::default(),
            config.system.data_dir.join("ota"),
        )
        .map_err(|e| {
            MupcError::new(
                ErrorCode::Unknown,
                format!("OTA 管理器初始化失败: {}", e),
                "startup",
            )
        })?,
    );
    // 实例**仍然**随 `StartupContext.ota_manager`（本文件 `:33` 的字段）交回调用方——该字段本就
    // 在，非本单元新增。⚠️ 措辞订正（第二轮整改 ②）：交回方类型是 `StartupContext`，
    // **不存在** `InitializeResult` 这个类型。
    // 「保留实例」的可观测性来自**源文本静态断言**：单测
    // `ota_manager_is_still_constructed_and_registered` 只对生产段做 `contains`（构造调用 /
    // `StartupContext` 字段 / 字段交回 / 服务注册 四处）。⚠️ 该用例**不取实例、不核对数据目录**
    // ——`initialize_all` 需 DB/intercore/gateway/sysfs 全套真环境，本机单测起不来。
    // 这条断言挡住的是"用删掉构造的方式悄悄退化成不保留实例"。
    tracing::info!(
        "OTA 管理器已创建（数据目录 {}）——本期无服务面（原消费者 web-api 已删除，见设计 §7.3 / R-16）",
        config.system.data_dir.join("ota").display()
    );

    // ── 11. OTA 服务注册（实例已在步骤 10 中创建；**本期不启动任何服务面**，见步骤 10 口径）──
    // 状态取 **`Stopped`**（订正 S-5）：本单元只保留实例与注册，**没有任何服务面在跑**；
    // 注册成 `Running` 会与上一行的日志"本期无服务面"自相矛盾，属"谎报已运行"。同 crate 对
    // 无运行时面的服务有现成先例（步骤 14 `wireless` 即 `Stopped`）。R-16 要求的"保留注册"
    // 由本行的注册动作本身满足——**注册在册 ≠ 状态为 Running**。
    tracing::info!("[11/14] 注册 OTA 服务（实例已在步骤 10 创建，本期无服务面，状态=Stopped）...");
    coord.register_service("ota_update", ServiceStatus::Stopped);

    // ── 12. 系统资源监控 ──
    tracing::info!("[12/14] 初始化系统资源监控...");
    let metrics_store = Arc::new(mupc_system_monitor::MetricsStore::new(
        config
            .system
            .data_dir
            .join("metrics")
            .to_str()
            .unwrap_or("/tmp/mupc-metrics"),
        30,
    ));
    if let Err(e) = metrics_store.init().await {
        tracing::warn!("指标存储初始化失败: {}", e);
    }
    let metrics_collector = mupc_system_monitor::FullCollector::new(60_000);
    let interval_ms = metrics_collector.collection_interval_ms();
    let collector = Arc::new(metrics_collector);
    let healing_engine = Arc::new(tokio::sync::Mutex::new(
        mupc_system_monitor::SelfHealingEngine::new(3, 30),
    ));
    let threshold_analyzer = mupc_system_monitor::ThresholdAnalyzer::default();
    let metrics_bg = metrics_store.clone();
    guard.0.push(tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
            match collector.collect().await {
                Ok(snapshot) => {
                    tracing::debug!(
                        "系统指标: CPU={:.1}% MEM={:.1}% DISK={:.1}% TEMP={:.1}°C",
                        snapshot.cpu.usage_percent,
                        snapshot.memory.usage_percent,
                        snapshot.disk.usage_percent,
                        snapshot.temperature.cpu_temp_c,
                    );
                    if let Err(e) = metrics_bg.store(&snapshot).await {
                        tracing::warn!("保存系统指标失败: {}", e);
                    }
                    // 自愈：分析指标 + 执行自愈动作
                    if let Ok(analysis) = threshold_analyzer.analyze(&snapshot) {
                        if let Ok(Some(healing)) = healing_engine.lock().await.auto_heal(&analysis)
                        {
                            tracing::info!("自愈动作已执行: {:?}", healing.action);
                        }
                    }
                }
                Err(e) => tracing::warn!("系统指标采集失败: {}", e),
            }
        }
    }));
    coord.register_service("system_monitor", ServiceStatus::Running);

    // ── 13. MQTT 桥接（01 设计 §9.4 序 7 / §9.3）──
    tracing::info!("[13/14] 初始化 MQTT 桥接...");
    // 装配**整段**由 `assemble_mqtt_bridge` 承担（可测接缝：真环境起不来，而
    // "未配置 ⇒ 零连接尝试"这条 CFG-2 回归防护必须可机械验证 ⇒ 见该函数的用例）。
    //
    // ⚠️ 现状缺陷已消除（§9.3.2 的装配点替换）：原实现用
    // `NorthMqttClient::new(&NorthMqttConfig::default())` —— 其 `broker_addr` 是
    // `mqtt.example.com:8883` + dummy 证书路径 ⇒ 一旦分向开关被打开就**真连假域名**。
    // 现在：① 客户端配置**逐字段来自 `config.mqtt_bridge.north`**（C-12 分层映射）；
    // ② 缺省 `enabled=false` ⇒ `plan_mqtt_launch` 返回全 `None` ⇒ **一行连接代码都不执行**；
    // ③ `NorthMqttConfig::default()` 的假域名/dummy 证书已改空串（C-11）。
    // 角色表须同时含 `south_pcs` 段合成的 `pcs` 站（否则 MQTT 载荷 `role` 落空串）
    // ⇒ 收敛到 `mqtt_station_roles`（**唯一接线点**，回归网见该函数文档与
    // `task10_station_roles_wiring_carries_pcs_role_into_publish_plan`）。
    let mqtt_roles = mqtt_station_roles(config);
    let mqtt_outcome = crate::uplink::assemble_mqtt_bridge(
        &config.mqtt_bridge,
        latest.clone(),
        uplink_points.clone(),
        mqtt_roles,
        // 装置标识（§9.3.4：PRD Q10 来源未定 ⇒ 本仓无权威来源 ⇒ 载荷 `dev` 写 `null`，不臆造）。
        // 待产品裁定后：`client_id` 缺省时取该值，仍在配置层。
        None,
        storage.events.clone(),
    )
    .await;
    for (label, h) in mqtt_outcome.tasks {
        // MQTT 任务**无退出钩子**（事件循环/定时器；离线缓存不落盘、掉电即丢——§9.3.3
        // "不落盘"是设计明文）⇒ 入 abort 名单 `guard`，与网关/指标采集同范式。
        tracing::debug!(task = label, "MQTT 后台任务已登记（abort 名单）");
        guard.0.push(h);
    }
    if let Some(detail) = mqtt_outcome.detail.as_deref() {
        tracing::error!("MQTT 装配未完成：{detail}");
    }
    coord.register_service(
        "mqtt_bridge",
        match mqtt_outcome.status {
            crate::uplink::MqttServiceStatus::Running => ServiceStatus::Running,
            // 未启用 / 构造失败一律不得谎报 Running（CFG-4；审查 R2-B5 的同一口径）
            crate::uplink::MqttServiceStatus::Disabled => ServiceStatus::Stopped,
            crate::uplink::MqttServiceStatus::Failed => ServiceStatus::Failed,
        },
    );

    // ── 14. 近场无线 ──
    tracing::info!("[14/14] 初始化近场无线...");
    // TODO (Phase 2+): 实例化 NoOp 无线驱动
    // 占位 stub：无线驱动未实例化，ECDH/链路加密未真正运行（Phase 2+）
    // ——不谎报 Running（审查 R2-B5）
    coord.register_service("wireless", ServiceStatus::Stopped);

    tracing::info!("所有 14 个子系统初始化完成 ({} 个 TODO 待阶段补全)", 2);

    // 初始化成功，取出两份任务清单（防止 Drop abort）并移交 StartupContext：
    // `bg_tasks` 退出时 abort；`coop_tasks` 退出时**先通知收工、等确认**（U-64）。
    let bg_tasks = std::mem::take(&mut guard.0);
    let coop_tasks = std::mem::take(&mut producers.0);
    Ok(StartupContext {
        message_bus,
        storage,
        intercore,
        plugin_loader,
        ai_engine,
        ai_integrator,
        ota_manager,
        fault_recorder,
        write_buffer,
        background_tasks: bg_tasks,
        cooperative_tasks: coop_tasks,
        stop_tx,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 只投递、不收的聚合行通道（T15/T16 遗留③ 修法后 `SouthSink::new` 多出的入参）。
    ///
    /// 接收端**故意保活**：无界通道的接收端一旦 drop，`forward_grid_sample` 的投递就会走
    /// "通道已关"的 ERROR 分支（那是退出期语义，不该出现在用例里）；本组用例不落库、
    /// 不闭合周期，故"遗忘接收端"等价于"下游仍在"。需要断言聚合行的用例请**直接建通道**
    /// （见 `grd07_grid_package_forwards_sample_without_disturbing_snapshot_or_strategy`）。
    ///
    /// ⚠️ 本助手**不是**生产路径的一部分（`#[cfg(test)]`）——生产侧通道在 `initialize_all`
    /// 里建，接收端交给 `grid_agg_timer`。
    fn agg_tx_for_test() -> AggregateRowSender {
        let (tx, rx) = aggregate_row_channel();
        std::mem::forget(rx);
        tx
    }

    /// 写路径装配的最小可解析 yaml（只需 `CoreConfig` 里**没有 `#[serde(default)]`** 的段）。
    ///
    /// 单元 K：原样保留现场运维手写的 `legacy_top:` 段——它**不是**任何字段，用来钉住
    /// "未建模段不得让 `CoreConfig` 解析失败"（`CoreConfig` 未设 `deny_unknown_fields`）。
    /// （`web_api:` 段此前也充当同一角色；该段随 crate 删除从 CoreConfig 退为未建模段，
    /// 其"仍能加载"的兼容性断言改由本段的 `legacy_top:` 与 `core_config.rs` 的专项用例承担。）
    const MIN_YAML: &str = r#"
version: "1.0"
system: {}
intercore: {}
legacy_top:
  a: 1
ai_engine: {}
plugins: {}
"#;

    fn core_handle() -> std::sync::Arc<tokio::sync::RwLock<CoreConfig>> {
        let cfg: CoreConfig = serde_yaml::from_str(MIN_YAML).expect("min yaml 必须可解析");
        std::sync::Arc::new(tokio::sync::RwLock::new(cfg))
    }

    /// **建议 6.1 的网**（单元 J 扩到**两条**写路径）：审计 sink 建不起来 ⇒ 配置写与联锁写
    /// **都**必须整体不可用（fail-closed），而**不是**降级成"没有审计的写路径"。
    ///
    /// 注入方式是**真实失败**（审计目录的父路径是一个普通文件 ⇒ `create_dir_all` 必失败），
    /// 不用 mock —— 要证的是**真实装配代码**的裁决，而不是测试桩自己的行为。
    ///
    /// **改什么会让本条变红**：把 `console_write_paths` 的错误分支改成
    /// `ApplySource::Ready(..)` / `InterlockOpsSource::Ready(..)`（或改成 `expect`/`unwrap`
    /// 让它 panic）⇒ 对应的断言红。
    #[test]
    fn console_write_paths_fail_closed_when_audit_is_unusable() {
        let t = crate::testutil::TempDir::new("apply-assembly");
        let blocker = t.write("blocker", "i am a file, not a dir");
        let bad_audit_dir = blocker.join("audit"); // 父是文件 ⇒ 建不出审计目录
        let core = core_handle();
        let (apply, ops) = console_write_paths(
            &bad_audit_dir,
            &t.join("mupc_core_config.yaml"),
            &core,
            None,
            None,
        );
        match apply {
            crate::console_host::ApplySource::Unavailable(reason) => {
                assert!(reason.contains("审计"), "原因须点明审计不可用: {reason}");
                assert!(
                    reason.contains("fail-closed"),
                    "原因须点明 fail-closed 口径: {reason}"
                );
            }
            crate::console_host::ApplySource::Ready(_) => {
                panic!("审计建不起来 ⇒ 不得给出「没有审计的写路径」（fail-closed 被绕过）")
            }
        }
        // 联锁写路径**同一条裁决**：审计建不起来 ⇒ 一律不执行（EDGE-18）
        match ops {
            crate::console_host::InterlockOpsSource::AuditUnavailable(reason) => {
                assert!(reason.contains("审计"), "原因须点明审计不可用: {reason}");
            }
            crate::console_host::InterlockOpsSource::Ready(_) => {
                panic!("审计建不起来 ⇒ 联锁写路径也**不得**可用（否则同一件事两条通道两种后果）")
            }
        }

        // 正对照：审计目录可用 ⇒ 两条都 `Ready`（证明上面那条是"审计不可用"而非"函数恒失败"）
        let (ok_apply, ok_ops) = console_write_paths(
            &t.join("audit"),
            &t.join("mupc_core_config.yaml"),
            &core,
            None,
            None,
        );
        assert!(
            matches!(ok_apply, crate::console_host::ApplySource::Ready(_)),
            "审计目录可建 ⇒ 配置写路径必须 Ready"
        );
        assert!(
            matches!(ok_ops, crate::console_host::InterlockOpsSource::Ready(_)),
            "审计目录可建 ⇒ 联锁写路径必须 Ready"
        );
    }

    /// **本文件的生产段源码**（`#[cfg(test)] mod tests` 之前），且**行尾统一为 LF**。
    ///
    /// 两条硬要求，缺一条断言就会变成坏网：
    /// 1. **必须切掉测试段**：下面的被禁串清单里就有 `mupc_web_api` / `WebInterlockApi`
    ///    这类字面量（**断言自己**写出来的），若不切段，`src.contains(..)` 会因断言自身而恒真
    ///    / 恒假——那是自指坏网，不是回归网；
    /// 2. **归一化行尾**：`startup.rs` 是 **CRLF**（现场 Windows 编辑过），`\n#[cfg(test)]\n`
    ///    这样的锚点匹配不上 `\r\n#[cfg(test)]\r\n`。锚点匹配不上就 panic，**不静默退化成考核空串**。
    fn production_src() -> String {
        let src = include_str!("startup.rs").replace("\r\n", "\n");
        let (production, _) = src
            .split_once("\n#[cfg(test)]\nmod tests {")
            .expect("`#[cfg(test)] mod tests` 标记必须存在（生产段/测试段的分段锚点）");
        assert!(
            production.len() > 10_000,
            "分段锚点必须真的切出生产段，实得 {} 字节",
            production.len()
        );
        production.to_string()
    }

    /// **单元 K ①：`ota_manager` 仍被创建与注册（能力未删）**——设计 §7.2 Step 3 末 / §7.3 末段
    /// 备注 / 待裁项 R-16 的**逐字口径**：「保留 `ota_update` 服务注册与实例（不删除能力），
    /// 但不启动任何服务面」。
    ///
    /// 这里用**源文本静态断言**（与 `cli.rs` 对 `startup.rs` 的同款手法）：`initialize_all`
    /// 需要 DB / intercore / gateway / sysfs 全套真环境才能跑，本机单测起不来；而本单元要证的
    /// 恰恰是"装配源码里这两件事还在"，不是"运行时它返回了什么"。
    ///
    /// **改什么会让本条变红**：删掉 `OtaManagerImpl::new` 的构造、把它从 `StartupContext`
    /// 摘掉、删掉 `register_service("ota_update"`、或**把注册状态从 `Stopped` 改回 `Running`**
    /// （K 收尾 Q-3 补的那条网）⇒ 对应断言红。
    #[test]
    fn ota_manager_is_still_constructed_and_registered() {
        let production = production_src();
        assert!(
            production.contains("mupc_ota_update::manager::OtaManagerImpl::new("),
            "OTA 实例必须仍被构造（不得因失去 web-api 消费者就删掉能力，见 R-16）"
        );
        assert!(
            production.contains("pub ota_manager: Arc<dyn mupc_ota_update::OtaManager>"),
            "实例必须仍随 StartupContext 交回调用方（否则只是「构造完就丢」）"
        );
        assert!(
            production.contains("ota_manager,"),
            "实例必须真的被放进 StartupContext（构造了却不交回 = 静默退化）"
        );
        assert!(
            production.contains("register_service(\"ota_update\""),
            "`ota_update` 服务注册不得删（这是「能力未删」的在册证据）"
        );
        // **状态必须是 `Stopped`**（订正 S-5；K 收尾 Q-3 补网）：本单元**没有任何服务面在跑**，注册成
        // `Running` 会与步骤 10 的日志"本期无服务面"（`:1150`）以及步骤 11 的注释自相矛盾
        // ⇒ 属"谎报已运行"。⚠️ 这条断言**必须钉住状态本身**：只 `contains("register_service(\"ota_update\"")`
        // 时，把 `Stopped` 改回 `Running` 仍然全绿（正是 Q-3 指出的"实质改动无守护"）。
        assert!(
            production.contains("register_service(\"ota_update\", ServiceStatus::Stopped)"),
            "`ota_update` 必须以 `Stopped` 注册（订正 S-5；K 收尾 Q-3）：改回 `Running` = 谎报服务面在跑"
        );
        // 「实例用哪个数据目录构造」也钉在**源文本**上（`config.system.data_dir` 下的 `ota/`）：
        // 这是**不取实例**的前提下能给出的最强证据（第二轮整改 ② 的补强）。
        assert!(
            production.contains("config.system.data_dir.join(\"ota\")"),
            "OTA 实例的数据目录必须来自配置（`config.system.data_dir` / `ota`），不得硬编码"
        );
        // 反向网：`mupc-ota-update` crate 不得被连带删除（同一次断言里钉住"能力仍在"）
        assert!(
            production.contains("mupc_ota_update::OtaConfig::default()"),
            "OTA 配置构造在，能力未删"
        );
    }

    /// **P0-1 网：遥测缓冲的「时间触发」与「退出落盘」两条接线不得被静默摘除。**
    ///
    /// 同 `ota_manager_is_still_constructed_and_registered` 的手法（源文本静态断言）：
    /// 两处都在 `initialize_all` / 优雅退出里，本机单测起不来真环境（DB/intercore/串口全套），
    /// 而本节要证的恰恰是"装配源码里这两件事还在"。**行为级**证据在 storage 侧
    /// （`writebuffer_flush_timer_commits_without_full_capacity` /
    /// `writebuffer_flush_timer_is_periodic` / `writebuffer_manual_flush`），本网只负责
    /// "核心接线没被摘掉"，两者互补：storage 侧证机制、本节证装配。
    ///
    /// **改什么会让本条变红**：删掉定时任务的 spawn（⇒ `flush_interval_ms` 又成死字段）、
    /// 把 `write_buffer` 从 `StartupContext` 摘掉（⇒ 退出路径够不到缓冲）、删掉 `shutdown()`
    /// 里的 flush 调用、或 `main.rs` 不再调用 `ctx.shutdown()`。
    ///
    /// ⚠️ **U-64 订正**：`spawn_flush_timer()` 的字面量改为 `spawn_flush_timer(`——
    /// 该函数现在要求传入停机信号接收端（协作退出），断言随之改为同时钉住"传了信号"
    /// （只钉函数名会漏掉"定时任务收不到停机信号 ⇒ 退出时仍被 abort 在半路"）。
    #[test]
    fn telemetry_buffer_timer_and_shutdown_flush_are_wired() {
        let production = production_src();
        assert!(
            production.contains("spawn_flush_timer(stop_rx.clone())"),
            "装配点必须起定时 flush 任务并**把停机信号交给它**（否则 `flush_interval_ms` 又成死字段、\
             或定时任务无法协作退出）"
        );
        assert!(
            production.contains("pub write_buffer: Arc<mupc_storage::WriteBuffer>"),
            "缓冲必须随 StartupContext 交回调用方（否则退出路径够不到它、无法落盘）"
        );
        assert!(
            production.contains("write_buffer,\n"),
            "实例必须真的被放进 StartupContext（构造了却不交回 = 退出仍丢最后一批）"
        );
        assert!(
            production.contains("self.flush_telemetry_buffer().await;"),
            "优雅退出必须落盘剩余缓冲（含不足一批的数据）"
        );
        assert!(
            include_str!("main.rs").contains("ctx.shutdown().await;"),
            "main.rs 的关闭路径必须调用 StartupContext::shutdown（flush 在其内）"
        );
    }

    // ── U-64 优雅停机 ──────────────────────────────────────────────────────────────

    /// 真缓冲 + 真库（`StorageService` 供落库断言用；本 crate 不依赖 `sqlx` ⇒ 不出现 `sqlx` 类型名）。
    async fn u64_setup(
        tag: &str,
    ) -> (mupc_storage::StorageService, Arc<mupc_storage::WriteBuffer>) {
        let t = crate::testutil::TempDir::new(tag);
        let db = t.join("mupcd.db");
        std::fs::File::create(&db).unwrap();
        let pool = Arc::new(mupc_storage::init_pool(db.to_str().unwrap()).await.unwrap());
        mupc_storage::run_migrations(&pool).await.unwrap();
        // flush_interval_ms=60_000 ⇒ 定时任务在本用例期间不会自己提交：落盘只能来自退出路径那次 flush
        let wb = Arc::new(mupc_storage::WriteBuffer::new(1000, 60_000, pool.clone()));
        (mupc_storage::StorageService::new(pool), wb)
    }

    fn u64_point(i: f64) -> mupc_storage::TelemetryPoint {
        mupc_storage::TelemetryPoint {
            id: None,
            device_id: "dev-u64".to_string(),
            timestamp: chrono::Utc::now(),
            metric_name: "v".to_string(),
            value: Some(i),
            quality: 0,
        }
    }

    /// 测试用聚合器（周期 60 s：单次 `on_grid_package` 不会闭合周期 ⇒ 不产行、不 spawn）。
    fn grid_agg() -> Arc<parking_lot::Mutex<mupc_storage::GridAggregator>> {
        Arc::new(parking_lot::Mutex::new(mupc_storage::GridAggregator::new(
            60_000,
        )))
    }

    /// **U-64 主用例**：停机时**不丢已完成 push 的点** —— 生产者必须在**被 flush 之前**收到
    /// 停机信号、把在途的最后一次写入做完；随后退出路径的 flush 必须把全部点落盘。
    ///
    /// 生产者刻意在"收到信号之后"再写一点：这正是修复前 `abort` 会杀掉的那一步。
    /// 改什么会让本条红：把 `stop_producers` 改回"直接 abort 所有任务"（不看信号、不等确认）
    /// ⇒ 生产者死在 `stopped()` 上、第 4 点永远不产生（`pushed_after_stop` 为 false、库里只有 3 行）。
    #[tokio::test]
    async fn stop_producers_lets_producer_finish_its_last_push_before_flush() {
        let (svc, wb) = u64_setup("u64-stop").await;
        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);

        let pushed_after_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = pushed_after_stop.clone();
        let wb_p = wb.clone();
        let mut stop = StopSignal {
            rx: stop_rx.clone(),
        };
        let producer = tokio::spawn(async move {
            for i in 0..3 {
                wb_p.buffer_telemetry(u64_point(i as f64)).await.unwrap();
            }
            stop.stopped().await;
            wb_p.buffer_telemetry(u64_point(99.0)).await.unwrap();
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        // 真定时 flush 任务（与生产同款协作退出者）
        let timer = wb.clone().spawn_flush_timer(stop_rx.clone());

        // 让生产者先把前 3 点写进缓冲（此后它停在 `stopped()` 上等信号）
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        stop_producers(
            &stop_tx,
            &[],
            vec![("south_sim_loop", producer), ("flush_timer", timer)],
            std::time::Duration::from_secs(2),
        )
        .await;

        assert!(
            pushed_after_stop.load(std::sync::atomic::Ordering::SeqCst),
            "生产者必须在被 flush 之前确认收工并做完最后一次写入（abort-first 会把它杀死 ⇒ 静默丢点）"
        );

        // 退出路径的最后一次 flush（次序与 `StartupContext::shutdown` 一致：先停生产者、再 flush）
        assert_eq!(
            wb.flush().await.unwrap(),
            4,
            "停机路径的 flush 必须落盘全部 4 点（含收工前最后一批）"
        );
        let rows = svc
            .telemetry
            .query_range(
                "dev-u64",
                chrono::Utc::now() - chrono::Duration::minutes(1),
                chrono::Utc::now() + chrono::Duration::minutes(1),
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 4, "4 点全部入库");
        assert_eq!(wb.dropped_points(), 0, "正常停机路径不得丢点");
    }

    /// **U-64 超时保护**：生产者**不理会**停机信号时，等待必须有上限（不得 hang 住退出流程）。
    ///
    /// 改什么会让本条红：把 `timeout_at` 去掉（直接 `handle.await`）⇒ 本用例卡在 30 s 的
    /// 顽固任务上直到测试超时。
    #[tokio::test]
    async fn stop_producers_gives_up_on_uncooperative_task_within_timeout() {
        let (stop_tx, _stop_rx) = tokio::sync::watch::channel(false);
        let stubborn = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        });

        let started = std::time::Instant::now();
        stop_producers(
            &stop_tx,
            &[],
            vec![("stubborn", stubborn)],
            std::time::Duration::from_millis(100),
        )
        .await;
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "必须在有上限的时间内返回（实得 {:?}）",
            started.elapsed()
        );
    }

    /// **U-64 接线网（源文本静态断言）**：`shutdown()` 的次序必须是
    /// **通知/停生产者 → 等确认 → 最后 flush**，且协作任务与 abort 名单**分开登记**。
    ///
    /// 同 `telemetry_buffer_timer_and_shutdown_flush_are_wired` 的手法与理由：装配期起不来真环境，
    /// 而本条要证的恰恰是"装配源码里的次序与分组"。
    ///
    /// 改什么会让本条变红：`shutdown` 里把 `self.flush_telemetry_buffer().await;` 提到
    /// `stop_producers(..)` 之前（回到"先 flush 再停生产者"或"先 abort 后 flush"的次序）；
    /// 把协作任务又塞回 `background_tasks`（⇒ 定时 flush 任务会被 abort 在半路）。
    #[test]
    fn graceful_shutdown_stops_producers_before_final_flush() {
        let production = production_src();
        assert!(
            production.contains(
                "pub cooperative_tasks: Vec<(&'static str, tokio::task::JoinHandle<()>)>"
            ),
            "协作式退出任务必须单独登记（不得混进 abort 名单）"
        );
        assert!(
            production.contains("stop_tx.send(true)"),
            "停机第一步必须是**通知收工**（而不是直接 abort）"
        );
        assert!(
            production.contains("&self.stop_tx,"),
            "`shutdown` 必须把**停机信号发送端**交给 `stop_producers`（否则生产者收不到信号）"
        );
        // 取**最后一次**出现：第一次是函数定义本身，调用点在 `shutdown` 里（定义在前、调用在后）。
        let stop_at = production
            .rfind("stop_producers(")
            .expect("`shutdown` 必须经 `stop_producers` 接缝停生产者");
        let flush_at = production
            .find("self.flush_telemetry_buffer().await;")
            .expect("退出必须落盘剩余缓冲");
        assert!(
            stop_at < flush_at,
            "次序必须是「先停生产者、后 flush」（实得 stop={stop_at} flush={flush_at}）"
        );
    }

    /// 只读的事件仓储桩（`on_station_telemetry` 的事件分支只会 `insert`，其余方法用不到）。
    struct RecordingEvents(std::sync::Mutex<Vec<mupc_storage::SystemEvent>>);

    #[async_trait::async_trait]
    impl mupc_storage::EventRepository for RecordingEvents {
        async fn insert(
            &self,
            event: &mupc_storage::SystemEvent,
        ) -> Result<i64, mupc_storage::StorageError> {
            self.0.lock().unwrap().push(event.clone());
            Ok(1)
        }
        async fn query_range(
            &self,
            _start: chrono::DateTime<chrono::Utc>,
            _end: chrono::DateTime<chrono::Utc>,
        ) -> Result<Vec<mupc_storage::SystemEvent>, mupc_storage::StorageError> {
            Ok(Vec::new())
        }
        async fn purge_older_than(
            &self,
            _before: chrono::DateTime<chrono::Utc>,
        ) -> Result<usize, mupc_storage::StorageError> {
            Ok(0)
        }
        async fn latest_by_type(
            &self,
            _event_type: &str,
        ) -> Result<Option<mupc_storage::SystemEvent>, mupc_storage::StorageError> {
            Ok(None)
        }
    }

    /// **单元 K ③：投递链路端到端** —— `SouthSink` 收到状态事件 ⇒ **先落库、同刻投递**
    /// ⇒ `AlertFeed` 订阅者收到（设计 §4.7：「`SouthSink` 写入系统事件时同时投递」）。
    ///
    /// 走**真实装配类型**（真 `SouthSink` + 真 `WriteBuffer` + 真 `Iec104Server`），只把
    /// 事件仓储换成记账桩（`insert` 是否被调用本身就是"先落库"的证据）。仓储用真 SQLite
    /// 临时文件连池（`init_pool` 不跑迁移，本用例不触表）。
    ///
    /// **改什么会让本条变红**：删掉 `SouthSink` 里的 `push_system_alert(..)` 调用
    /// （投递断链）⇒ 第 2 条断言 5 s 超时红；把投递挪到落库**之前**且删掉落库 ⇒ 第 1 条红。
    #[tokio::test]
    async fn south_sink_event_lands_in_storage_and_is_delivered_to_alert_feed() {
        use mupc_southd::scheduler::StationSink as _;

        let t = crate::testutil::TempDir::new("south-sink-feed");
        let db = t.join("mupcd.db");
        std::fs::File::create(&db).unwrap();
        let pool = mupc_storage::init_pool(db.to_str().unwrap()).await.unwrap();
        let write_buffer = Arc::new(mupc_storage::WriteBuffer::new(1000, 5000, Arc::new(pool)));
        let events = Arc::new(RecordingEvents(std::sync::Mutex::new(Vec::new())));

        let feed = Arc::new(crate::alert_feed::AlertFeed::new());
        let mut rx = feed.subscribe();

        let sink = SouthSink::new(
            Arc::new(mupc_strategy_engine::AiIntegrator::new()),
            write_buffer,
            events.clone(),
            feed.clone(),
            Arc::new(mupc_gateway::iec104::server::Iec104Server::new(
                mupc_gateway::iec104::server::Iec104Config::default(),
            )),
            Arc::new(mupc_data_processing::latest_values::LatestValues::new(
                mupc_data_processing::DATA_FRESHNESS_MS / 1000,
            )),
            None,
            grid_agg(),
            agg_tx_for_test(),
        );

        // 站离线：`is_event=true` 的状态事件点
        sink.on_station_telemetry(
            "st-1",
            mupc_southd::config::Role::MeterGrid,
            vec![("offline".to_string(), 0.0, true)],
        )
        .await;

        // ① 先落库（`storage.events` 仍是 F7 真源）
        let logged = events.0.lock().unwrap().clone();
        assert_eq!(
            logged.len(),
            1,
            "事件必须落库（AlertFeed 不是真源，只是附加投递）"
        );
        assert_eq!(logged[0].event_type, "south_station.st-1.offline");

        // ② 同刻投递到 `AlertFeed`（有界即时环）
        let got = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("5 s 内必须收到投递（超时 = 投递断链）")
            .expect("订阅者必须收到事件");
        assert_eq!(got.source, crate::alert_feed::FeedOrigin::System);
        assert_eq!(
            got.subtype, "warning",
            "offline 走告警级（online 才是 info）"
        );
        assert!(
            got.message.contains("离线"),
            "文案逐字沿用落库那条: {}",
            got.message
        );
        assert_eq!(got.message, logged[0].message, "投递文案与落库文案必须同源");
    }

    /// **T1 写入侧接线**（01 设计 §9.1.4 / §9.4 序 3）：`SouthSink` 三个回调把采集结果写入
    /// `LatestValues` 快照，且**不动**既有落库路径（本用例用真 `WriteBuffer`，落库同时发生）。
    ///
    /// **改什么会让本条变红**：删掉 `on_station_telemetry` 里的 `mark_station_polled` /
    /// `apply`（快照恒空）、把 `is_event` 分支也写进快照（点数变 3）、
    /// 或把 `on_station_offline` 的 `mark_station_offline` 删掉（质量仍 `Ok`、站活性不消失）。
    #[tokio::test]
    async fn south_sink_writes_telemetry_and_grid_into_latest_values() {
        use mupc_southd::scheduler::StationSink as _;

        let t = crate::testutil::TempDir::new("south-sink-latest");
        let db = t.join("mupcd.db");
        std::fs::File::create(&db).unwrap();
        let pool = mupc_storage::init_pool(db.to_str().unwrap()).await.unwrap();
        let latest = Arc::new(mupc_data_processing::latest_values::LatestValues::new(
            mupc_data_processing::DATA_FRESHNESS_MS / 1000,
        ));
        let sink = SouthSink::new(
            Arc::new(mupc_strategy_engine::AiIntegrator::new()),
            Arc::new(mupc_storage::WriteBuffer::new(1000, 5000, Arc::new(pool))),
            Arc::new(RecordingEvents(std::sync::Mutex::new(Vec::new()))),
            Arc::new(crate::alert_feed::AlertFeed::new()),
            Arc::new(mupc_gateway::iec104::server::Iec104Server::new(
                mupc_gateway::iec104::server::Iec104Config::default(),
            )),
            latest.clone(),
            Some("meter_grid".to_string()),
            grid_agg(),
            agg_tx_for_test(),
        );

        let soc_id = PointId {
            station: "bms".to_string(),
            metric: "soc".to_string(),
        };

        // ① 非事件点 ⇒ 写快照；事件点**不**写（§9.1.4 第 2 行 ②③）
        sink.on_station_telemetry(
            "bms",
            mupc_southd::config::Role::Battery,
            vec![
                ("soc".to_string(), 55.0, false),
                ("bms_alarm_225".to_string(), 1.0, false),
                ("online".to_string(), 1.0, true),
            ],
        )
        .await;

        let soc = latest.get(&soc_id);
        assert_eq!(soc.value.value, Some(55.0), "非事件点必须进快照");
        assert_eq!(soc.value.quality, PointQuality::Ok);
        // T13（§9.2.1.1）：`role==Battery` ⇒ 本批 apply 后写回 **15 组聚合**（`bms_aggr_*`）。
        // 故 "bms" 快照 = 2 个非事件源点（soc + bms_alarm_225）+ 15 聚合。本断言的原判据是
        // 「**事件点不得进快照**」⇒ 过滤掉聚合点后计数（原 `.len()==2` 系聚合接线**前**的 T1
        // 口径，随 T13 按 §9.2.1.1 更新；意图不变，并新增聚合数断言钉住写回）。
        let bms_snap = latest.station_snapshot("bms");
        assert_eq!(
            bms_snap
                .iter()
                .filter(|pv| !pv.id.metric.starts_with("bms_aggr_"))
                .count(),
            2,
            "事件点不得进快照（非聚合仍只有 2 个非事件点）"
        );
        assert_eq!(
            bms_snap
                .iter()
                .filter(|pv| pv.id.metric.starts_with("bms_aggr_"))
                .count(),
            15,
            "role==Battery ⇒ 15 组 BMS 聚合写回快照（§9.2.1.1）"
        );
        assert_eq!(
            latest.station_last_poll_ms("bms"),
            Some(soc.value.ts_ms),
            "同一轮：点 ts_ms == 站最后成功时刻（R-38 的消费前提，12 设计 §15.1.2 C-4）"
        );
        assert!(latest.station_is_active("bms", soc.value.ts_ms));

        // ② 站离线 ⇒ **保原值原时标** + `Invalid` + 清站活性（EX-1）
        sink.on_station_offline("bms", mupc_southd::config::Role::Battery, "mock 超时")
            .await;
        let soc_off = latest.get(&soc_id);
        assert_eq!(soc_off.value.value, Some(55.0), "保原值，不补 0");
        assert_eq!(soc_off.value.ts_ms, soc.value.ts_ms, "保原始时标");
        assert_eq!(soc_off.value.quality, PointQuality::Invalid);
        assert_eq!(latest.station_last_poll_ms("bms"), None);
        assert!(!latest.station_is_active("bms", soc.value.ts_ms));

        // ③ grid 回调 ⇒ 6 个派生量；字段不可得 ⇒ `None` + `Invalid`（**禁补 0**，LV-6）
        sink.on_grid_package(mupc_data_processing::DataPackage {
            electrical: mupc_data_processing::ElectricalData {
                voltage: Some(220.0),
                current: Some(10.0),
                active_power: Some(50.0),
                reactive_power: Some(10.0),
                cos_phi: Some(0.98),
                frequency: None, // 本轮不可得
                phase: None,
            },
            battery: mupc_data_processing::BatteryData {
                soc: None,
                soh: None,
                temperature: None,
            },
            device_status: mupc_data_processing::DeviceStatus {
                inverter_status: mupc_data_processing::InverterStatus::Running,
                pv_power: None,
                load_power: None,
                ev_charger_power: None,
            },
            timestamp: 0,
        })
        .await;

        let grid = latest.station_snapshot("meter_grid");
        assert_eq!(grid.len(), 6, "grid 段固定 6 个派生量（C-17 ② 的点名例外）");
        let freq = latest.get(&PointId {
            station: "meter_grid".to_string(),
            metric: "frequency".to_string(),
        });
        assert_eq!(freq.value.value, None, "不可得字段严禁以 0 顶替");
        assert_eq!(freq.value.quality, PointQuality::Invalid);
        let volt = latest.get(&PointId {
            station: "meter_grid".to_string(),
            metric: "voltage".to_string(),
        });
        assert_eq!(volt.value.value, Some(220.0));
        assert_eq!(volt.value.quality, PointQuality::Ok);
    }

    /// **GRD-07**：总表聚合**不改变**北向上送与策略输入 —— 同一次 `on_grid_package` 里：
    /// ① 最新值快照（北向上送的取数源）逐字段照旧；② 策略 phase 的 `set_latest_data` 仍在；
    /// ③ 聚合只**转发样本**（单包不闭合周期 ⇒ 不往 `WriteBuffer` 里塞点）。
    ///
    /// **改什么会让本条变红**：删掉 `set_latest_data`（策略输入没了）；把 `forward_grid_sample`
    /// 改成在回调里自己算聚合/自己 `unwrap_or(0.0)`；把 `apply_grid_snapshot` 从回调里摘掉。
    #[tokio::test]
    async fn grd07_grid_package_forwards_sample_without_disturbing_snapshot_or_strategy() {
        use mupc_southd::scheduler::StationSink as _;

        let t = crate::testutil::TempDir::new("grid-forward");
        let db = t.join("mupcd.db");
        std::fs::File::create(&db).unwrap();
        let pool = mupc_storage::init_pool(db.to_str().unwrap()).await.unwrap();
        // 真 WriteBuffer（容量足够大 ⇒ 不会容量触发提交，便于断言"没多写点"）
        let wb = Arc::new(mupc_storage::WriteBuffer::new(1000, 60_000, Arc::new(pool)));
        let latest = Arc::new(mupc_data_processing::latest_values::LatestValues::new(
            mupc_data_processing::DATA_FRESHNESS_MS / 1000,
        ));
        // 聚合周期 60 s：单次包不闭合周期
        let agg = grid_agg();
        // 本用例**直接持有接收端**：① 断言"同包不产行"；② 反证聚合行只经该通道
        //（T15/T16 遗留③：`forward_grid_sample` 已无 `tokio::spawn`）
        let (agg_tx, mut agg_rx) = aggregate_row_channel();
        let sink = SouthSink::new(
            Arc::new(mupc_strategy_engine::AiIntegrator::new()),
            wb.clone(),
            Arc::new(RecordingEvents(std::sync::Mutex::new(Vec::new()))),
            Arc::new(crate::alert_feed::AlertFeed::new()),
            Arc::new(mupc_gateway::iec104::server::Iec104Server::new(
                mupc_gateway::iec104::server::Iec104Config::default(),
            )),
            latest.clone(),
            Some("meter_grid".to_string()),
            agg.clone(),
            agg_tx,
        );

        sink.on_grid_package(mupc_data_processing::DataPackage {
            electrical: mupc_data_processing::ElectricalData {
                voltage: Some(220.0),
                current: Some(10.0),
                active_power: Some(50.0),
                reactive_power: Some(10.0),
                cos_phi: Some(0.98),
                frequency: Some(50.0),
                phase: Some(mupc_data_processing::telemetry::PhaseElectricalData {
                    voltage: [Some(220.0), Some(221.0), Some(219.0)],
                    current: [Some(10.0), Some(11.0), Some(12.0)],
                    active_power: [Some(20.0), Some(15.0), Some(15.0)],
                    reactive_power: [Some(3.0), Some(3.0), Some(4.0)],
                    cos_phi: [Some(0.98), Some(0.97), Some(0.99)],
                }),
            },
            battery: mupc_data_processing::BatteryData {
                soc: None,
                soh: None,
                temperature: None,
            },
            device_status: mupc_data_processing::DeviceStatus {
                inverter_status: mupc_data_processing::InverterStatus::Running,
                pv_power: None,
                load_power: None,
                ev_charger_power: None,
            },
            timestamp: 0,
        })
        .await;

        // ① 北向上送源（最新值快照）逐字段照旧
        let volt = latest.get(&PointId {
            station: "meter_grid".to_string(),
            metric: "voltage".to_string(),
        });
        assert_eq!(volt.value.value, Some(220.0));
        assert_eq!(volt.value.quality, PointQuality::Ok);
        assert_eq!(
            latest.station_snapshot("meter_grid").len(),
            6,
            "grid 6 个派生量照旧"
        );
        // ② 样本**已转发**给聚合器（锚定到当前周期；跨周期才产行）
        assert!(
            agg.lock().current_start_ms().is_some(),
            "同包电气量必须已转发给聚合器（转发点未接线 ⇒ 红）"
        );
        // ③ 同包不产行 ⇒ 既有遥测落库路径没有被聚合多写点
        assert_eq!(
            wb.buffered_points(),
            0,
            "单包不闭合周期 ⇒ 不得往遥测缓冲塞点（聚合只在周期闭合时产行）"
        );
        // ④ T15/T16 遗留③：聚合行**只经投递通道**（本包未闭合周期 ⇒ 通道为空；
        //    且这一步能读到 rx 本身就证明"入队不再由游离 spawn 承担"）
        assert!(
            drain_aggregate_rows(&mut agg_rx).is_empty(),
            "单包不闭合周期 ⇒ 通道内不得有聚合行"
        );
        // ⑤ 结构断言：`forward_grid_sample` **不得**再出现 `tokio::spawn`
        //    （回归防护：改回 spawn 即恢复"退出序列看不见的写者"窄竞态）
        let src = production_src();
        let start = src
            .find("fn forward_grid_sample")
            .expect("forward_grid_sample 必须存在");
        let end = src[start..]
            .find("fn apply_grid_snapshot")
            .map(|o| start + o)
            .expect("apply_grid_snapshot 必须紧随其后（作为右锚点）");
        assert!(
            !src[start..end].contains("tokio::spawn"),
            "`forward_grid_sample` 不得含 `tokio::spawn`（T15/T16 遗留③：聚合行须经通道交给\
             已注册的 grid_agg_timer 入队）"
        );
        assert!(
            src[start..end].contains("agg_tx.send("),
            "`forward_grid_sample` 必须经 `agg_tx.send(..)` 投递聚合行"
        );
    }

    /// **STG-02 / STG-03 / §9.6 序 3/4/6**：`storage:` 段 → 装配点的接线（**源文本静态断言**）。
    ///
    /// 为什么用源文本：`initialize_all` 需要 DB / intercore / 串口全套真环境，本机单测起不来；
    /// 而本用例要证的恰恰是「装配源码里这两个构造取自 `config.storage.*`、tick 任务进了
    /// **协作生产者名单**」——不是「运行时它返回了什么」（与 `ota_manager_is_still_constructed_
    /// and_registered` 同一手法）。
    ///
    /// **改什么会让本条变红**：把 `WriteBuffer::new` 的参数写回硬编码 `1000, 5000`；
    /// 用常量而不是 `config.storage.grid_aggregate_period_ms` 建聚合器；
    /// 把 tick 任务从 `producers.0.push` 挪到 abort 名单（破坏 U-64 顺序契约）。
    #[test]
    fn stg02_stg03_assembly_reads_storage_section_and_registers_tick_as_producer() {
        let production = production_src();
        for needle in [
            "config.storage.batch_capacity",
            "config.storage.flush_interval_ms",
            "config.storage.grid_aggregate_period_ms",
        ] {
            assert!(
                production.contains(needle),
                "装配段必须从 `storage:` 段取 {needle}（缺 ⇒ 该项仍在硬编码）"
            );
        }
        assert!(
            !production.contains("WriteBuffer::new(\n        1000,"),
            "旧的硬编码 `WriteBuffer::new(1000, 5000, …)` 必须消失（口径改由配置承载）"
        );
        // tick 任务：协作生产者名单（不是 abort 名单）+ 收工前 flush
        assert!(
            production.contains("\"grid_agg_timer\""),
            "总表聚合 tick 任务必须登记（否则周期永远不闭合 ⇒ 无行落库）"
        );
        let tick_idx = production
            .find("\"grid_agg_timer\"")
            .expect("grid_agg_timer 必须存在");
        assert!(
            production[..tick_idx].contains("producers.0.push(("),
            "tick 必须走 `producers`（协作退出名单）—— 走 abort 名单会与退出 flush 抢时序（U-64）"
        );
        assert!(
            production.contains("aggregator.lock().flush(now_ms)"),
            "收工前必须 flush 未闭合周期（不丢最后一段）"
        );
        // 聚合算法不在装配闭包内：`on_grid_package` 只转发样本
        assert!(
            production.contains("fn forward_grid_sample"),
            "`on_grid_package` 的聚合入口必须是「转发样本」这一层（算法在 storage，可单测）"
        );
        // 落库入队必须经**唯一转换点** `to_telemetry_point`，且该段内**不得**出现 `unwrap_or`
        // （把缺测 `None` 兜成 0 就是 PRD R-11.2-E 的「写 0 冒充」；`AggregateRow` 侧的同名
        // 断言见 `storage/src/grid_aggregate.rs` 的 `to_telemetry_point_preserves_none_as_null_intent`）。
        let enq_start = production
            .find("async fn enqueue_aggregate_rows")
            .expect("落库入队函数必须存在");
        let enq_end = production
            .find("fn now_ms()")
            .expect("`fn now_ms()` 必须存在（作为入队段的右锚点）");
        let enqueue_src = &production[enq_start..enq_end];
        assert!(
            enqueue_src.contains("to_telemetry_point"),
            "聚合行落库必须经唯一转换点 `AggregateRow::to_telemetry_point`"
        );
        assert!(
            !enqueue_src.contains("unwrap_or"),
            "入队段严禁把缺测值兜成 0（`None` 必须原样落 NULL）"
        );
    }

    /// **§9.6 末段 + §9.8 D-8**：`DataPackage → GridSample` 的抽取语义（缺块 ⇒ `None`，不造数）。
    #[test]
    fn grid_sample_from_package_maps_blocks_and_keeps_missing_as_none() {
        let pkg = |phase: Option<mupc_data_processing::telemetry::PhaseElectricalData>,
                   p_total: Option<f64>,
                   q_total: Option<f64>| mupc_data_processing::DataPackage {
            electrical: mupc_data_processing::ElectricalData {
                voltage: Some(220.0),
                current: Some(10.0),
                active_power: p_total,
                reactive_power: q_total,
                cos_phi: Some(0.98),
                frequency: Some(50.0),
                phase,
            },
            battery: mupc_data_processing::BatteryData {
                soc: None,
                soh: None,
                temperature: None,
            },
            device_status: mupc_data_processing::DeviceStatus {
                inverter_status: mupc_data_processing::InverterStatus::Running,
                pv_power: None,
                load_power: None,
                ev_charger_power: None,
            },
            timestamp: 0,
        };
        let full_phase = mupc_data_processing::telemetry::PhaseElectricalData {
            voltage: [Some(220.0), None, Some(219.0)],
            current: [Some(-10.0), Some(11.0), Some(12.0)],
            active_power: [Some(20.0), Some(15.0), Some(15.0)],
            reactive_power: [Some(3.0), Some(3.0), Some(4.0)],
            cos_phi: [Some(0.98), Some(0.97), Some(0.99)],
        };
        let s = grid_sample_from_package(&pkg(Some(full_phase.clone()), Some(50.0), Some(10.0)));
        assert_eq!(
            s.u,
            [Some(220.0), None, Some(219.0)],
            "缺测相保持 None（不补 0）"
        );
        assert_eq!(s.i[0], Some(-10.0), "电流带符号原样透传");
        assert_eq!(s.p_total, Some(50.0), "顶层有功");
        assert_eq!(s.q_total, Some(10.0), "顶层无功");

        // 缺相量块 ⇒ 分相通道全 None（产 NoData 行），顶层量仍在
        let s2 = grid_sample_from_package(&pkg(None, Some(50.0), Some(10.0)));
        assert!(s2.u.iter().all(|v| v.is_none()));
        assert!(s2.i.iter().all(|v| v.is_none()));
        assert!(s2.pf.iter().all(|v| v.is_none()));
        assert_eq!(s2.p_total, Some(50.0), "顶层量不受相量块缺失影响");

        // 顶层缺块 ⇒ 该通道 None（`frequency` **一律不抽取**：点表无源，常量入库会污染统计）
        let s3 = grid_sample_from_package(&pkg(Some(full_phase), None, None));
        assert_eq!(s3.p_total, None);
        assert_eq!(s3.q_total, None);
    }

    /// **T13（§9.2.1.1）：BMS 15 组聚合写回快照** —— `on_station_telemetry(role==Battery)`
    /// 在本批 `apply` 之后，从快照读该站**全部位点**（一次取读锁，不逐点 get）求值 15 组聚合，
    /// 以聚合点名经同一 `apply` 写回。判据：① 15 组 `bms_aggr_*` 出现；② OR 语义（任一位 1
    /// ⇒ 聚合 1）；③ 全组不可得 ⇒ `None`（**严禁写 0**，EX-6，不得假报"无告警"）。
    ///
    /// **改什么会让本条变红**：删掉 `on_station_telemetry` 末尾的 `apply_bms_aggregates` 调用
    /// （聚合恒缺失 ⇒ ① 红）；用"本批交付位"而非全快照求值；或把不可得组写成 0（③ 红）。
    #[tokio::test]
    async fn south_sink_battery_writes_back_15_bms_aggregates() {
        use mupc_southd::scheduler::StationSink as _;

        let t = crate::testutil::TempDir::new("bms-aggr-writeback");
        let db = t.join("mupcd.db");
        std::fs::File::create(&db).unwrap();
        let pool = mupc_storage::init_pool(db.to_str().unwrap()).await.unwrap();
        let latest = Arc::new(mupc_data_processing::latest_values::LatestValues::new(
            mupc_data_processing::DATA_FRESHNESS_MS / 1000,
        ));
        let sink = SouthSink::new(
            Arc::new(mupc_strategy_engine::AiIntegrator::new()),
            Arc::new(mupc_storage::WriteBuffer::new(1000, 5000, Arc::new(pool))),
            Arc::new(RecordingEvents(std::sync::Mutex::new(Vec::new()))),
            Arc::new(crate::alert_feed::AlertFeed::new()),
            Arc::new(mupc_gateway::iec104::server::Iec104Server::new(
                mupc_gateway::iec104::server::Iec104Config::default(),
            )),
            latest.clone(),
            None,
            grid_agg(),
            agg_tx_for_test(),
        );

        // 组 1 cluster_voltage = 位地址 201–206 ↔ `bms_alarm_2..7`；置 bms_alarm_2=1、其余 0
        let bits: Vec<(String, f64, bool)> = (2..=7u16)
            .map(|k| {
                (
                    format!("bms_alarm_{k}"),
                    if k == 2 { 1.0 } else { 0.0 },
                    false,
                )
            })
            .collect();
        sink.on_station_telemetry("bms", mupc_southd::config::Role::Battery, bits)
            .await;

        // ① 15 组聚合写回
        let aggrs: Vec<_> = latest
            .station_snapshot("bms")
            .into_iter()
            .filter(|pv| pv.id.metric.starts_with("bms_aggr_"))
            .collect();
        assert_eq!(aggrs.len(), 15, "15 组 BMS 聚合必须写回快照");

        // ② OR 语义：cluster_voltage 组有位 201=1 ⇒ 聚合=1，且组内位齐 ⇒ Ok
        let cv = latest.get(&PointId {
            station: "bms".to_string(),
            metric: "bms_aggr_cluster_voltage".to_string(),
        });
        assert_eq!(cv.value.value, Some(1.0), "任一位 1 ⇒ 聚合 1（OR）");
        assert_eq!(cv.value.quality, PointQuality::Ok, "组内位齐 ⇒ Ok");
        assert!(cv.value.ts_ms > 0, "ts_ms = 本轮");

        // ③ 全组不可得（位未交付）⇒ None（严禁写 0，EX-6）
        let ct = latest.get(&PointId {
            station: "bms".to_string(),
            metric: "bms_aggr_cell_temp".to_string(),
        });
        assert_eq!(
            ct.value.value, None,
            "全组不可得 ⇒ None（严禁写 0；不得假报无告警）"
        );
    }

    /// 真 `SouthSink` + 记账仓储 + 真 `AlertFeed`（T6 的两个文案用例共用装配，避免三处复制）。
    async fn south_sink_for_test(
        tag: &str,
        events: Arc<RecordingEvents>,
        feed: Arc<crate::alert_feed::AlertFeed>,
    ) -> SouthSink {
        let t = crate::testutil::TempDir::new(tag);
        let db = t.join("mupcd.db");
        std::fs::File::create(&db).unwrap();
        let pool = mupc_storage::init_pool(db.to_str().unwrap()).await.unwrap();
        SouthSink::new(
            Arc::new(mupc_strategy_engine::AiIntegrator::new()),
            Arc::new(mupc_storage::WriteBuffer::new(1000, 5000, Arc::new(pool))),
            events,
            feed,
            Arc::new(mupc_gateway::iec104::server::Iec104Server::new(
                mupc_gateway::iec104::server::Iec104Config::default(),
            )),
            Arc::new(mupc_data_processing::latest_values::LatestValues::new(
                mupc_data_processing::DATA_FRESHNESS_MS / 1000,
            )),
            None,
            grid_agg(),
            agg_tx_for_test(),
        )
    }

    /// **T6 / PRD §9.7.2 第 1 条**：站 offline 的**块级 reason** 必须落到事件 `message`
    /// （运维据 `events` 定位到具体块）。覆写 `on_station_offline` 前，reason 被默认实现
    /// **丢弃**（默认只发 `("offline", 1.0, true)`）⇒ 本条会红。
    #[tokio::test]
    async fn south_sink_offline_event_carries_block_reason() {
        use mupc_southd::scheduler::StationSink as _;

        let events = Arc::new(RecordingEvents(std::sync::Mutex::new(Vec::new())));
        let feed = Arc::new(crate::alert_feed::AlertFeed::new());
        let mut rx = feed.subscribe();
        let sink = south_sink_for_test("south-offline-reason", events.clone(), feed).await;

        sink.on_station_offline(
            "grid_meter",
            mupc_southd::config::Role::MeterGrid,
            "站 slave=3 读 0x1000x6 失败: mock 超时",
        )
        .await;

        let logged = events.0.lock().unwrap().clone();
        assert_eq!(
            logged.len(),
            1,
            "覆写后仍是**一条** offline 事件（不多产，去抖仍归 scheduler）"
        );
        assert_eq!(
            logged[0].event_type, "south_station.grid_meter.offline",
            "事件类型与默认实现逐字一致（消费方零改动）"
        );
        assert!(
            logged[0].message.contains("slave=3") && logged[0].message.contains("0x1000"),
            "reason 必须进 message（运维据此定位到块）: {}",
            logged[0].message
        );
        assert!(logged[0].message.contains("离线"), "既有文案片段保留");
        let got = tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("5 s 内必须收到投递")
            .expect("订阅者必须收到事件");
        assert_eq!(got.subtype, "warning");
        assert_eq!(got.message, logged[0].message, "投递文案与落库文案同源");
    }

    /// **T6 / §11.7.2 第 4/5 条**：状态事件的 `message` = **中文名（点表 label）+ 原始值**。
    /// - `soc_out_of_range`（事件命名空间的名字，label 查不到）⇒ 用原名 + 越界原值（PRD
    ///   §9.6.3 ② 要求"soc 越界告警含原始寄存器值"，该值由 scheduler 作为进入事件 `value` 传出）；
    /// - `bms_alarm_225`（**遥测**点名）⇒ 查 `point_table::label` 得"簇一级告警"；
    /// - `<原名>@recovered`（§11.4.7.2 C 的哨兵 value = 0.0）⇒ 原名 + 值 0。
    #[tokio::test]
    async fn south_sink_event_message_carries_label_and_raw_value() {
        use mupc_southd::config::Role;
        use mupc_southd::scheduler::StationSink as _;

        let events = Arc::new(RecordingEvents(std::sync::Mutex::new(Vec::new())));
        let feed = Arc::new(crate::alert_feed::AlertFeed::new());
        let sink = south_sink_for_test("south-event-message", events.clone(), feed).await;

        sink.on_station_telemetry(
            "bms",
            Role::Battery,
            vec![
                ("soc_out_of_range".to_string(), 65535.0, true),
                ("bms_alarm_225".to_string(), 1.0, true),
                (
                    "fire_detector_addr_order_invalid@recovered".to_string(),
                    0.0,
                    true,
                ),
            ],
        )
        .await;

        let logged = events.0.lock().unwrap().clone();
        assert_eq!(
            logged[0].event_type, "south_station.bms.soc_out_of_range",
            "事件键仍是 south_station.<站id>.<metric>（不新造事件类型枚举）"
        );
        assert!(
            logged[0].message.contains("soc_out_of_range") && logged[0].message.contains("65535"),
            "越界原值必须落证: {}",
            logged[0].message
        );
        assert!(
            logged[1].message.contains("簇一级告警"),
            "点表 label 反查中文名（点名 225 ↔ 位地址 424）: {}",
            logged[1].message
        );
        assert!(
            logged[2].message.contains("@recovered") && logged[2].message.contains("值 0"),
            "`@recovered` 是事件命名空间的名字（label 查不到）⇒ 原名 + 哨兵 0: {}",
            logged[2].message
        );
    }

    /// **单元 K ②：`startup.rs` 生产段里不得再有任何 web-api 残引用**（设计 §7.2 Step 3 / §7.3）。
    ///
    /// 这是"删干净"的可编译版网：`mupc-web-api` 依赖一删，任何残留 `use` / 路径都会**编译失败**
    /// ——但**注释与字符串里的残留不会**（例如 `register_service("web_api")` 是字符串，
    /// 编译得过却让协调器里多一个幽灵服务）。故此处逐条钉死。
    ///
    /// ⚠️ **只考核 `#[cfg(test)] mod tests` 之前的生产段**：本条断言自身的**字面量**就含被禁串，
    /// 若把测试段也纳入考核，`src.contains("…")` 会因**断言自己**而成真/成假——那是自指坏网，
    /// 不是回归网。分段锚点缺失即 panic（不许静默退化成"考核空串"）。
    ///
    /// **改什么会让本条变红**：把服务名改回 `"web_api"`、重新引入 `WebInterlockApi` 适配器、
    /// 或把 `register_service("hmi_backend", …)` 搬出 `display` 门——**搬进 `} else {` 分支**
    /// 或**搬回步骤 10 一带**，两条均已于第二轮整改 ① 各实测一次：都红。
    /// **订正 K 收尾 Q-2（伪造锚点绕过）**：判据的 `gate` / `at` 已从**子串查找**改为**行首锚定**
    /// ——改前在注册点正上方插一行注释 `// if config.display.enabled {` 即可把锚点"拉"过来
    /// （复核员实测假通过）；改后注释行不满足行首条件，同样插入后**必红**（已实测）。
    #[test]
    fn startup_production_code_has_no_web_api_residue() {
        let production = production_src();
        assert!(
            !production.contains("mupc_web_api"),
            "不得再引用 `mupc_web_api`（crate 已删）"
        );
        assert!(
            !production.contains("WebInterlockApi"),
            "过渡适配器已删除，不得回流"
        );
        assert!(
            !production.contains("\"web_api\""),
            "服务名已改为 `hmi_backend`，不得再注册幽灵服务名"
        );
        assert!(
            !production.contains("SsePushService"),
            "`SsePushService` 已随 crate 删除"
        );
        assert!(
            production.contains("register_service(\"hmi_backend\""),
            "本地 HMI 后端必须在册"
        );
        // `hmi_backend` 的注册必须在 `display.enabled` 的 **`if` 分支体**内：若留在原处
        // （step 10），`display.enabled=false` 时会谎报"HMI 后端已运行"。
        //
        // ⚠️ 上界**必须**取 `} else {` 分界，**不能**取"步骤 9 注释行"（订正 I-1）：
        // `if { … } else { … }` 是一个视觉块，若用步骤 9 注释当上界，`else` 块体（其内也含
        // `tracing::debug!` 等语句）会被一并算进"门内区间" ⇒ 把注册点搬进 `else`
        // （`display.enabled=false` 时才注册——正是本断言要防的那件事）**也能通过**。
        // 判据因此是三元：注册点必须晚于「注册点**之前最近**的 `if config.display.enabled {`」
        // （S-4：从 `at` 往前找，防将来在装配点前再出现同样 `if` 时静默放宽），
        // 且早于该 `if` 之后的 `} else {`。
        //
        // ⚠️ **两处锚点都必须"行首锚定"**（订正 K 收尾 Q-2，复核员实测过伪造绕过）：原先用
        // `find` / `rfind` 做**子串**查找 ⇒ 只要在注册点正上方插一行注释
        // `// if config.display.enabled {`，`rfind` 就会把"门"拉到该注释处（`gate < at` 仍成立、
        // 后面 `find("} else {")` 仍找到真 `else`）⇒ 把注册点搬进 `else` 也能假通过。
        // 现要求锚点**独占行首**：注释行（行首为 `//`）与字符串天然不满足。
        // 上界 `} else {` 仍是子串查找——伪造只会把它**提前**（`find` 取首个匹配 ⇒ 判据更严，
        // 是红而非假绿），故不必动。
        let line_start = |i: usize| production[..i].rfind('\n').map_or(0, |n| n + 1);
        let at = production
            .match_indices("register_service(\"hmi_backend\"")
            .map(|(i, _)| i)
            .find(|&i| production[line_start(i)..i].trim() == "coord.")
            .expect(
                "生产段必须存在 `hmi_backend` 的注册点（须独占行首 `coord.register_service(`）",
            );
        let gate = production
            .match_indices("if config.display.enabled {")
            .map(|(i, _)| i)
            .filter(|&i| production[line_start(i)..i].trim().is_empty())
            .map(line_start)
            .filter(|&i| i < at)
            .last()
            .expect("注册点之前必须存在独占行首的 `if config.display.enabled {`（门）");
        let else_anchor = gate
            + production[gate..].find("} else {").expect(
                "display 块必须有 `} else {` 作为上界锚点（若改成无 else 的 if，请同步改本判据）",
            );
        assert!(
            gate < at && at < else_anchor,
            "`hmi_backend` 注册必须在 `config.display.enabled` 的 **`if` 分支体内**\
             （不得落到 `}} else {{` 分支、也不得在块外）（实得 gate={gate} at={at} else={else_anchor}）"
        );
    }

    /// **D2（§15.2.2 / §15.11 #7）钢瓶气压接缝适配器**：真对象、真接缝，无 mock。
    ///
    /// 三态与"接缝未接线"的可达性：
    /// - `attach` 前 ⇒ 消防站回 `None`（**接缝未接线**；**不得**回 `Some(false)`）；
    /// - 非消防站 ⇒ 恒 `None`（§15.2.2 的"非消防站"成因，**本适配器是唯一判定点**）；
    /// - 未登记站 ⇒ `None`（不臆造下标）；
    /// - `attach` 真 `SouthScheduler`（无口可开 ⇒ 从未采到非 0）⇒ **`Some(false)`**
    ///   —— 这正是 EDGE-23「未配置」/ EX-11 的生产可达态。
    ///
    /// **改什么会让本条变红**：把 `attach` 后的 `Some(..)` 改回 `None` ⇒ ⑤ 红；去掉"非消防站
    /// ⇒ `None`"的入口守卫 ⇒ ② 红（会把 `bms` 也判成「未配置」）。
    ///
    /// ⚠️ **本用例不判别"站 id → 站下标"映射的正确性**：无口可开时**任何**下标都回 `false`
    /// （要有真差别，需"某站采到非 0 而另一站没有"，而 `cylinder_seen_nonzero` 没有可注入句柄）。
    /// 该映射目前只有**结构性**保证（两侧都用 `cfg.stations.iter().enumerate()` 的同一序，
    /// 且调度器由**同一份** `config.south_stations` 构造）——如实登记于任务报告。
    #[test]
    fn cylinder_pressure_query_follows_attach_and_filters_non_fire() {
        use crate::display_host::CylinderPressureQuery as _;

        /// 调度器需要一个 sink；本用例不验采集投递 ⇒ 全空实现。
        struct NoopSink;
        #[async_trait::async_trait]
        impl mupc_southd::scheduler::StationSink for NoopSink {
            async fn on_grid_package(&self, _pkg: mupc_data_processing::DataPackage) {}
            async fn on_station_telemetry(
                &self,
                _station_id: &str,
                _role: mupc_southd::config::Role,
                _points: Vec<(String, f64, bool)>,
            ) {
            }
            async fn on_battery_soc(&self, _station_id: &str, _soc: f64) {}
        }

        // 站序刻意让**消防不是首站**（`bms` 在下标 0）
        let cfg: mupc_southd::config::SouthStationsConfig = serde_yaml::from_str(
            r#"
poll_ms: 1000
stale_timeout_s: 5
stations:
  - id: bms
    role: battery
    port: "/dev/ttyS2"
    interval_ms: 1000
    regs:
      - { name: bms_io, func: input, addr: 100, count: 31, format: uint16, scale: 1.0 }
  - id: fire
    role: fire
    port: "/dev/ttyS6"
    interval_ms: 1000
    regs:
      - { name: fire_sys, func: holding, addr: 4, count: 13, format: uint16, scale: 1.0 }
"#,
        )
        .expect("测试南向配置可解析");

        let q = SouthCylinderPressureQuery::new(&cfg);
        // ① `attach` 前 = 接缝未接线 ⇒ 消防站也不得回 `Some(false)`
        assert_eq!(
            q.cylinder_configured("fire"),
            None,
            "接缝未接线 ⇒ 不可得（**不得**伪装成「未配置」）"
        );
        // ② 非消防站 ⇒ 恒不可得（`None` 成因之一）
        assert_eq!(q.cylinder_configured("bms"), None, "非消防站 ⇒ 不可得");
        // ③ 未登记站 ⇒ 不可得（不臆造下标）
        assert_eq!(q.cylinder_configured("no-such-station"), None);

        // ④/⑤ 接**真**调度器：无口可开 ⇒ 该站从未采到非 0 ⇒ "未配置"
        let sched = mupc_southd::scheduler::SouthScheduler::new(
            cfg.clone(),
            std::collections::HashMap::new(),
            Arc::new(NoopSink),
        );
        assert!(
            !sched.cylinder_pressure_configured(0) && !sched.cylinder_pressure_configured(1),
            "前提：无口可开 ⇒ 两站都没有「曾出现过非 0」的记忆"
        );
        q.attach(sched);
        assert_eq!(
            q.cylinder_configured("fire"),
            Some(false),
            "消防站 + 已接线 ⇒ 权威结论 Some(false)（EDGE-23「未配置」的可达态）"
        );
        assert_eq!(q.cylinder_configured("bms"), None, "接线后非消防站仍不可得");
        assert_eq!(q.cylinder_configured("no-such-station"), None);
    }

    /// **Task 10 第 7 接线点的回归网**（规格评审 2026-09-26 指出：把 `mqtt_station_roles`
    /// 里的 `Some(&config.south_pcs)` 改回 `None`，**全部既有用例仍绿** —— 因为既有用例
    /// （`core-bin/src/uplink.rs` 的 `roles_of`）自己带上 `Some` 直接调
    /// `uplink::station_roles`，**绕开了生产调用点**）。
    ///
    /// 后果（不接线时）：MQTT 载荷里 pcs 站的 `role` 落**空串**，且无用例会响。
    ///
    /// 两条断言缺一不可：
    /// ① **生产形态**（`mqtt_station_roles(&cfg)` = `initialize_all` 的调用形态）下，`pcs` 站
    ///    在发布计划里的 `role == "pcs"` —— `StationPlan.role` 正是遥测载荷 `role` 的来源
    ///    （`uplink.rs` 的 `TelemetryPayload { role: st.role }`）；
    /// ② `station_roles(.., None)` 与 `(.., Some(&pcs))` **确实不同**、且退化后落空串
    ///    —— 钉住"传 None 会退化"这件事本身（这是防"接线点被改回 None 仍绿"的关键）。
    ///
    /// **改什么会让本条变红**：① `mqtt_station_roles` 的 `Some(..)` → `None` ⇒ 运行时断言红；
    /// ② 把生产段改成**绕过** `mqtt_station_roles` 直接调 `crate::uplink::station_roles`
    /// （哪怕硬编码 `Some(..)`）⇒ 源文本断言红（否则"换个地方退化成 None"会绕开 ①）。
    #[test]
    fn task10_station_roles_wiring_carries_pcs_role_into_publish_plan() {
        // 与 `core-bin/src/uplink.rs` 用例**同一份 fixture**（不新建第二份点表真源）
        const REF_STATIONS: &str =
            include_str!("../../mupc-southd/tests/fixtures/south_stations_s3b2.yaml");
        const REF_PCS: &str = include_str!("../../mupc-southd/tests/fixtures/south_pcs_s3b2.yaml");
        #[derive(serde::Deserialize)]
        struct Wrapper {
            south_stations: mupc_southd::config::SouthStationsConfig,
        }

        // ⓪ 生产段接线形状：角色表**只能**经 `mqtt_station_roles` 收敛（堵"绕过包装函数"的改法）
        let production = production_src();
        assert!(
            production.contains("let mqtt_roles = mqtt_station_roles(config);"),
            "生产段必须以 `mqtt_station_roles(config)` 收敛角色表（不得在装配点就地调 station_roles）"
        );
        assert_eq!(
            production.matches("crate::uplink::station_roles(").count(),
            1,
            "生产段只允许 `mqtt_station_roles` 内部调用一次 `station_roles` —— 在装配点**再就地**\
             调一次（比如退化成 None）会把 `Some/None` 的选择挪出被用例钉住的收敛点"
        );

        let mut cfg: CoreConfig = serde_yaml::from_str(MIN_YAML).expect("min yaml 必须可解析");
        cfg.south_stations = serde_yaml::from_str::<Wrapper>(REF_STATIONS)
            .expect("站级段解析失败")
            .south_stations;
        cfg.south_pcs = serde_yaml::from_str(REF_PCS).expect("south_pcs 段解析失败");
        assert!(cfg.south_pcs.enabled, "参考 PCS 段须 enabled");

        let points =
            mupc_southd::uplink::build_uplink_points(&cfg.south_stations, Some(&cfg.south_pcs))
                .expect("参考点表（5 站 + south_pcs）必须可生成");

        // ① 生产形态 → 发布计划里 pcs 站的 role（= 载荷 role 的来源）
        let plan = crate::uplink::plan_stations(&points, &mqtt_station_roles(&cfg));
        let pcs = plan
            .iter()
            .find(|s| s.id == "pcs")
            .expect("pcs 站必须在发布计划里（south_pcs.enabled=true）");
        assert_eq!(
            pcs.role, "pcs",
            "生产装配形态下 pcs 站载荷 role 必须是 \"pcs\"（空串 = 静默降级）"
        );

        // ② 传 None 会退化（证明"改回 None 仍绿"是**缺陷**，而非两种写法等价）
        let with_none = crate::uplink::station_roles(&cfg.south_stations, None);
        let with_pcs = crate::uplink::station_roles(&cfg.south_stations, Some(&cfg.south_pcs));
        assert_ne!(
            with_none, with_pcs,
            "传 None 必须与传 Some 不同（否则本条断言无判别力）"
        );
        assert!(
            !with_none.contains_key("pcs"),
            "传 None ⇒ 角色表里查无 pcs 站（`unwrap_or_default` 落空串的成因）"
        );
        let plan_none = crate::uplink::plan_stations(&points, &with_none);
        assert_eq!(
            plan_none
                .iter()
                .find(|s| s.id == "pcs")
                .expect("pcs 站仍在计划里，只是 role 查不到")
                .role,
            "",
            "传 None ⇒ pcs 站载荷 role 落空串（正是评审指出的静默退化形态）"
        );
    }

    /// **Task 10 评审项 2 的回归网**：`south_pcs` 的 `response_timeout_ms` / `data_bits` /
    /// `stop_bits` 是 `StationConf` **无落点**的控制面三项，`PcsHandle` 也不读它们
    /// ⇒ 若不显式接线到口层，就是**死配置**（YAML 写 `response_timeout_ms: 200`，
    /// 实际 `Rs485Device` 取 `Config::default()` 的 **1000ms**，`stop()` 这类安全动作的
    /// 失败检测随之变慢）。
    ///
    /// 本用例钉住**生产形态的收敛点** `south_pcs_port_params`（口层侧的透传由
    /// `mupc-southd` 的 `bus_config_passes_port_params_through` 钉住 ⇒ 两段合起来才是全链）。
    ///
    /// **改什么会让本条变红**：把 `south_pcs_port_params` 改成返回 `PortParams::default()`
    /// （或任一字段硬编码）⇒ 本条红；把装配点的调用改成绕过它（就地 `PortParams::default()`、
    /// 或退回 `Rs485PortBus::open`）⇒ 源文本断言红。
    #[test]
    fn task10_south_pcs_port_params_read_all_three_control_fields() {
        // ⓪ 生产段接线形状：PCS 口必须以 `open_with_port_params` + `south_pcs_port_params` 打开
        let production = production_src();
        assert!(
            production.contains("Rs485PortBus::open_with_port_params("),
            "PCS 口必须走 `open_with_port_params`（退回 `Rs485PortBus::open` ⇒ 三项又成死配置）"
        );
        assert!(
            production.contains("south_pcs_port_params(&config.south_pcs)"),
            "口层参数的唯一来源必须是 `south_pcs_port_params(&config.south_pcs)`（不得就地取默认）"
        );

        let pcs = mupc_southd::config::SouthPcsConfig::default();
        // 段缺省形态：`response_timeout_ms=200`（原 `intercore.modbus_rtu` 默认值）/
        // `data_bits=8` / `stop_bits=1` ⇒ 三项都取自本段（**不得**是口层默认 1000ms）
        let d = south_pcs_port_params(&pcs);
        assert_eq!(
            (d.timeout_ms, d.data_bits, d.stop_bits),
            (200, 8, 1),
            "段缺省必须取自 `south_pcs` 段（尤其 timeout：口层默认是 1000ms，取错即回落）"
        );
        assert_ne!(
            d.timeout_ms,
            mupc_southd::port_runtime::PortParams::default().timeout_ms,
            "判别力锚：`south_pcs` 段默认 200ms **不同于**口层默认 1000ms ⇒ 本条不可能是\"两边都取默认\"的巧合"
        );
        // 三个字段各改成一个**非默认**值（注意 timeout 取 350 而非 200：200 恰是段默认值，
        // 用它无法区分"真读字段"与"写死段默认"）⇒ 改配置后生效路径上的取值必须随动
        let mut pcs = pcs;
        pcs.response_timeout_ms = 350;
        pcs.data_bits = 7;
        pcs.stop_bits = 2;
        let pp = south_pcs_port_params(&pcs);
        assert_eq!(
            pp.timeout_ms, 350,
            "response_timeout_ms 必须透传（不得回落 `Config::default()` 的 1000ms）"
        );
        assert_eq!(pp.data_bits, 7, "data_bits 必须透传（不得静默用默认 8）");
        assert_eq!(pp.stop_bits, 2, "stop_bits 必须透传（不得静默用默认 1）");
    }
}
