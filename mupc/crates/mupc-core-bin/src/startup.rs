//! 子系统启动编排器
//!
//! 按照依赖关系顺序初始化 14 个子系统。
//! 初始化顺序由 Section 3.3 设计文档定义。
//!
//! Phase 2 实现: 06/08/09/10 子系统已完成基础初始化。
//! Phase 2+ 实现: 11 OTA 管理器已完成初始化。
//! 剩余 6 个 TODO 见代码注释。

use device_trait::plugin_loader::PluginLoader;
use device_trait::Device;
use mupc_common::{ErrorCode, MupcError};
use mupc_core::service_coord::ServiceStatus;
use mupc_core::service_coord_impl::ServiceCoordinatorImpl;
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
    /// 后台任务句柄（Phase 6 优雅退出时 abort）
    pub background_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl StartupContext {
    /// 优雅退出：abort 所有后台任务
    pub async fn shutdown(&self) {
        tracing::info!("优雅退出：abort {} 个后台任务", self.background_tasks.len());
        for handle in &self.background_tasks {
            handle.abort();
        }
    }
}

/// IEC 104 命令处理器：转发主站控制命令到实时控制模块
struct StrategyCommandHandler {
    intercore: Arc<mupc_intercore::IntercoreClient>,
    /// 安全联锁控制器（io.enabled 时注入；latch 期间抑制主站下发，避免绕过联锁启停 PCS）
    interlock: Option<Arc<crate::interlock::InterlockController>>,
    /// 额定有功上限 (kW)：IEC104 主站外部指令 p_set clamp 用（审查 R1-A1，2026-09-09），
    /// 来源台区储能容量档 p_cap（电池功率上限，YAML 真值）。
    p_max_kw: f64,
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
                    let dual = mupc_intercore::DualParamCommand::new(
                        p_set,
                        cmd.k_value.unwrap_or(0.0),
                        true,
                        "intelligent",
                    );
                    self.intercore.send_dual_param(&dual).await.map_err(|e| {
                        MupcError::new(
                            ErrorCode::Unknown,
                            format!("命令下发失败: {}", e),
                            "gateway",
                        )
                    })?;
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
        timestamp: (frame.timestamp / 1000) as u64,
    }
}

/// DataPackage → 遥测点列表（FIXME: 指标映射根据点表确定）
fn datapackage_to_telemetry_points(
    pkg: &mupc_data_processing::DataPackage,
    device_id: &str,
) -> Vec<mupc_storage::TelemetryPoint> {
    let ts = chrono::DateTime::from_timestamp(pkg.timestamp as i64, 0)
        .unwrap_or_else(chrono::Utc::now);
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
                value: v,
                quality: 0,
            })
        })
        .collect()
}

/// S3 §10.3：southd 采集结果 sink（core-bin 装配侧实现，startup 是 core-bin 唯一装配者）。
///
/// 分流语义（southd scheduler 已按 role 分流，单写方口径见 southd 模块头）：
/// - 含 meter_grid 站时，grid 遥测 → `AiIntegrator::set_latest_data`（策略 phase 唯一写方；
///   master_meter 段已删除收敛，south_stations.meter_grid 是唯一 grid 源，绝不并存第二写方）。
///   仅配非 grid 站（B2）时本 sink 不触发 on_grid_package，AiIntegrator 由 pv/load 南向模拟兜底。
/// - 非 grid 遥测点（is_event=false）→ WriteBuffer 落库（telemetry）。
/// - offline/online 状态事件（is_event=true，metric=offline/online）→ storage.events 落库
///   + SSE system alert。
struct SouthSink {
    ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
    write_buffer: Arc<mupc_storage::WriteBuffer>,
    events: Arc<dyn mupc_storage::EventRepository>,
    sse: Arc<mupc_web_api::SsePushService>,
    /// IEC104 服务器（审查 R2-A2：meter_grid 真值上送北向）。SouthSink 是 core-bin 类型，
    /// mupc-southd 仅定义 StationSink trait——不引入 southd→gateway 反向依赖。
    iec104: Arc<mupc_gateway::iec104::server::Iec104Server>,
    /// grid 上送节流：meter_grid 最后广播时刻（1Hz 上界，见 broadcast_grid_iec104 注释）
    grid_bcast_at: std::sync::Mutex<Option<std::time::Instant>>,
}

impl SouthSink {
    fn new(
        ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
        write_buffer: Arc<mupc_storage::WriteBuffer>,
        events: Arc<dyn mupc_storage::EventRepository>,
        sse: Arc<mupc_web_api::SsePushService>,
        iec104: Arc<mupc_gateway::iec104::server::Iec104Server>,
    ) -> Self {
        Self {
            ai_integrator,
            write_buffer,
            events,
            sse,
            iec104,
            grid_bcast_at: std::sync::Mutex::new(None),
        }
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
    async fn broadcast_grid_iec104(&self, pkg: &mupc_data_processing::DataPackage) {
        let now = std::time::Instant::now();
        let allowed = {
            let mut last = self.grid_bcast_at.lock().unwrap();
            match *last {
                Some(t) if now.duration_since(t) < std::time::Duration::from_millis(1000) => false,
                _ => {
                    *last = Some(now);
                    true
                }
            }
        };
        if !allowed {
            return;
        }
        let el = &pkg.electrical;
        // 固定 IOA 分配：1=有功(kW) 2=无功(kVAr) 3=电压(V) 4=电流(A) 5=功率因数 6=频率(Hz)
        let points: [(&str, u32, Option<f64>); 6] = [
            ("active_power", 1, el.active_power),
            ("reactive_power", 2, el.reactive_power),
            ("voltage", 3, el.voltage),
            ("current", 4, el.current),
            ("cos_phi", 5, el.cos_phi),
            ("frequency", 6, el.frequency),
        ];
        for (name, ioa, val) in points {
            if let Some(v) = val {
                // 仿 pv/load 南向模拟上送循环：encode_telemetry_asdu(ioa, v as f32, cot=1)，
                // 单点单 ASDU 各自 make_i_frame 广播（pv/load 同范式，I 帧序号由连接层维护）。
                tracing::debug!(ioa, name, v, "IEC104 上送 meter_grid 真值");
                let asdu = mupc_gateway::iec104::protocol::encode_telemetry_asdu(ioa, v as f32, 1);
                let frame = mupc_gateway::iec104::Iec104Frame::make_i_frame(0, 0, &asdu);
                self.iec104.broadcast_telemetry(frame).await;
            }
        }
    }
}

#[async_trait::async_trait]
impl mupc_southd::scheduler::StationSink for SouthSink {
    async fn on_grid_package(&self, pkg: mupc_data_processing::DataPackage) {
        // 策略 phase 单写方（唯一 grid 源 south_stations.meter_grid，M-4 防双写方并存）。
        self.ai_integrator.set_latest_data(pkg.clone()).await;
        // 审查 R2-A2 (2026-09-09)：同包 meter_grid 真值上送 IEC104——调度主站不再只收
        // pv/load 南向模拟固定假遥测。数据流接线在 startup 层（SouthSink 为 core-bin 内联
        // 类型，见广播实现注释的依赖方向约束）。
        self.broadcast_grid_iec104(&pkg).await;
    }

    async fn on_station_telemetry(
        &self,
        station_id: &str,
        role: mupc_southd::config::Role,
        points: Vec<(String, f64, bool)>,
    ) {
        for (metric, value, is_event) in points {
            if is_event {
                // 状态事件（offline/online 由 scheduler handle_failure/mark_success 合成）：
                // DB 落库 + SSE system alert。落库失败仅 warn 不 panic（interlock 同范式）。
                let ev = mupc_storage::SystemEvent {
                    id: None,
                    timestamp: chrono::Utc::now(),
                    // 形如 south_station.<站id>.offline / .online
                    event_type: format!("south_station.{}.{}", station_id, metric),
                    source: station_id.to_string(),
                    message: format!(
                        "站 {} role={:?} {}",
                        station_id,
                        role,
                        if metric == "offline" {
                            "离线（采集失败）"
                        } else if metric == "online" {
                            "恢复上线"
                        } else {
                            metric.as_str()
                        }
                    ),
                };
                if let Err(e) = self.events.insert(&ev).await {
                    tracing::warn!("南向站事件落库失败 {}: {}", ev.event_type, e);
                }
                // online 恢复是状态正常化，用 info 级；offline/其它状态异常才告警级，
                // 避免站恢复上线时刷屏 warning。
                let level = if metric == "online" { "info" } else { "warning" };
                let _ = self.sse.push_system_alert(level, &ev.message);
            } else {
                // 普通遥测点落库
                let tp = mupc_storage::TelemetryPoint {
                    id: None,
                    device_id: station_id.to_string(),
                    timestamp: chrono::Utc::now(),
                    metric_name: metric,
                    value,
                    quality: 0,
                };
                if let Err(e) = self.write_buffer.buffer_telemetry(tp).await {
                    // 与事件落库路径一致用 warn：遥测丢点影响持久性可观测，不宜静默降 debug
                    tracing::warn!("南向遥测落库失败 {}: {}", station_id, e);
                }
            }
        }
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

/// web-api 旧联锁出口的**过渡适配器**（单元 **K** 的删除面）。
///
/// `mupc_web_api::app_state::InterlockApi` 是**迁移前**的旧契约（`Result<(), String>`）。
/// 单元 J 把 `InterlockController` 的实现迁到了 `mupc_display_proto::interlock`（结构化
/// `InterlockReject`），但 `AppState.interlock` 与读通道装配
/// （`display_host::interlock_wiring_for`）**仍按旧 trait 取用**，而 `web-api` crate 属 K 的
/// 删除面（J 禁改）。⇒ 本适配器把新契约**桥**回旧 trait，**不复制任何逻辑**：
/// 状态由 `InterlockApi::status()` 转出，错误取 `InterlockReject::user_message()`。
///
/// ⚠️ **如实登记**：这不是"两份实现"，而是**同一个实现的两种签名**——真源仍是
/// `InterlockController` 的 `do_request_release` / `ack_m1`。K 删掉 web-api 时，本适配器与
/// `mupc-web-api` 依赖一并删除（届时 `display_host` 也应改吃契约版视图，那属 K 的范围）。
pub(crate) struct WebInterlockApi(pub(crate) std::sync::Arc<crate::interlock::InterlockController>);

#[async_trait::async_trait]
impl mupc_web_api::app_state::InterlockApi for WebInterlockApi {
    async fn status(&self) -> mupc_web_api::app_state::InterlockStatus {
        use mupc_display_proto::InterlockApi as _;
        let v = self.0.status().await;
        mupc_web_api::app_state::InterlockStatus {
            enabled: v.enabled,
            latched: v.latched,
            stop_failed: v.stop_failed,
            sources: v
                .sources
                .into_iter()
                .map(|s| mupc_web_api::app_state::InterlockSourceStatus {
                    name: s.name,
                    tripped: s.tripped,
                })
                .collect(),
            // 旧 DTO 的两个灯位是 `bool`（**无**「未知」态），契约是 `Option<bool>` ⇒
            // `unwrap_or(false)`：**本适配路径不可表达** `None`（旧 DTO 根本没有那个槽，
            // **不是**"契约的 `None` 不重要"）。真源 `InterlockController::status()` 当前**恒**
            // 回 `Some(..)` ⇒ 该降级分支零后果；**单元 K 把读通道换到契约版真源时，必须一并
            // 迁移**（届时 `None` = 「灯未知」须如实上屏，不得再被这里吞成 `false`）。
            // 取 `false` 而非 `true`：宁可显"灯灭"，也绝不臆造"灯亮"。
            fault_lamp: v.fault_lamp.unwrap_or(false),
            run_lamp: v.run_lamp.unwrap_or(false),
        }
    }

    async fn request_release(&self) -> Result<(), String> {
        use mupc_display_proto::InterlockApi as _;
        self.0.request_release().await.map_err(|r| r.user_message())
    }

    async fn ack_m1(&self) -> Result<(), String> {
        use mupc_display_proto::InterlockApi as _;
        self.0.ack_m1().await.map_err(|r| r.user_message())
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

    // ── 1. 消息总线 (无依赖) ──
    tracing::info!("[01/14] 初始化消息总线...");
    let message_bus = Arc::new(mupc_core::TokioMessageBus::new(256));
    coord.register_service("message_bus", ServiceStatus::Running);

    // ── 2. 安全模块 ──
    tracing::info!("[02/14] 初始化安全模块...");
    // TODO (Phase 2+): 加载 TLS 证书和 SM2/SM4 密钥
    // 当前: mupc-security 模块无 SecurityModule 类型
    tracing::info!("安全模块初始化 (stub): cert_dir={}", config.system.cert_dir.display());
    // 占位 stub：security 模块无 SecurityModule 实例，国密/TLS 未真正运行（Phase 2+）
    // ——不谎报 Running（审查 R2-B5）
    coord.register_service("security", ServiceStatus::Stopped);

    // ── 3. 持久化存储 ──
    tracing::info!("[03/14] 初始化持久化存储...");
    let data_dir = config.system.data_dir.clone();
    tokio::fs::create_dir_all(&data_dir)
        .await
        .map_err(|e| MupcError::new(ErrorCode::IoError, format!("创建数据目录失败: {}", e), "startup"))?;
    let db_path = data_dir.join("mupc.db");
    if !db_path.exists() {
        tokio::fs::File::create(&db_path)
            .await
            .map_err(|e| MupcError::new(ErrorCode::IoError, format!("创建数据库文件失败: {}", e), "startup"))?;
    }
    let db_str = db_path.to_str()
        .expect("数据目录路径包含非法 UTF-8 字符");
    let pool = mupc_storage::init_pool(db_str)
        .await
        .map_err(|e| MupcError::new(ErrorCode::ConnectionFailed, format!("数据库连接失败: {}", e), "startup"))?;
    mupc_storage::run_migrations(&pool)
        .await
        .map_err(|e| MupcError::new(ErrorCode::ConfigError, format!("数据库迁移失败: {}", e), "startup"))?;
    let storage = Arc::new(mupc_storage::StorageService::new(Arc::new(pool)));
    let write_buffer = Arc::new(mupc_storage::WriteBuffer::new(1000, 5000, storage.pool().clone()));
    coord.register_service("storage", ServiceStatus::Running);

    // ── 4. 核间通信 ──
    tracing::info!("[04/14] 初始化核间通信...");
    // 传输通道由 intercore.transport 决定：modbus_rtu=生产主链路(PCS 真实协议)，
    // tcp=仿真/联调（sim-bridge 作 TCP 服务端）。未知值启动即报错（M3），避免
    // 配置手误静默落到仿真通道、生产 PCS 空转不被控。
    let intercore: Arc<mupc_intercore::IntercoreClient> = match config.intercore.transport.as_str() {
        "modbus_rtu" => {
            let mb = &config.intercore.modbus_rtu;
            tracing::info!("intercore transport = modbus_rtu: {} @{}", mb.serial_port, mb.baud_rate);
            let transport = Arc::new(mupc_intercore::ModbusRtuTransport::new(
                mupc_intercore::ModbusRtuSettings {
                    serial_port: mb.serial_port.clone(),
                    baud_rate: mb.baud_rate,
                    data_bits: mb.data_bits,
                    stop_bits: mb.stop_bits,
                    parity: mb.parity.clone(),
                    slave_addr: mb.slave_addr,
                    response_timeout_ms: mb.response_timeout_ms,
                    heartbeat_poll_ms: mb.heartbeat_poll_ms,
                },
            ));
            // Modbus 无主动心跳：后台轮询读 PCS 3 区 REG_RUN_STATE(1013) 判在线/离线。
            // 句柄入 guard（M8）：优雅退出时随其它后台任务一并 abort，而非只靠 runtime drop
            guard.0.push(tokio::spawn(transport.clone().run_heartbeat_loop()));
            Arc::new(mupc_intercore::IntercoreClient::with_transport(transport))
        }
        "tcp" => {
            let remote_addr = format!("{}:{}", config.intercore.host, config.intercore.port);
            let transport = Arc::new(mupc_intercore::TcpTransport::new(remote_addr));
            // N3: 启动回读接收（实时模块 DataUpload 上送 battery_soc → SOC 数据源）
            transport.spawn_receive();
            Arc::new(mupc_intercore::IntercoreClient::with_transport(transport))
        }
        other => {
            return Err(MupcError::new(
                ErrorCode::ConfigError,
                format!(
                    "intercore.transport='{other}' 非法：仅支持 \"tcp\"（仿真/联调）或 \"modbus_rtu\"（生产 PCS 主链路）"
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
                Err(e) => tracing::warn!("  {} 加载失败 (预期内，如 .so 未编译): {}", plugin_name, e),
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
    let ai_engine = Arc::new(ai_engine);
    coord.register_service("ai_engine", ServiceStatus::Running);

    // ── 8. 策略引擎 ──
    tracing::info!("[08/14] 初始化策略引擎...");
    let mut ai_integrator = mupc_strategy_engine::AiIntegrator::new();
    ai_integrator.set_intercore_client(intercore.clone());
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
    // 本地策略优先模式（YAML 配置：ai_engine.local_priority；Web API 可运行时切换）
    ai_integrator.set_local_priority(config.ai_engine.local_priority).await;

    // v2.23: 注入 AI 指令安全校验器（安全闸门，dispatch 前校验 AI 指令，不通过降级本地兜底）
    ai_integrator
        .set_validator(Arc::new(
            mupc_strategy_engine::AiCommandValidatorImpl::new(),
        ))
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

    // SSE 推送服务（提前创建，供 AI 决策循环推送决策事件）
    let sse_push = Arc::new(mupc_web_api::SsePushService::new(256));

    // ── S2 §12.4 / Task7：安全联锁控制器（io.enabled 时装配）──
    // 依赖：intercore(步骤 4) + storage(步骤 3) + sse_push 均已就绪。GPIO(sysfs) 打开失败由
    // InterlockController::new 内部 fail-safe（预置 latch，绝不静默无 latch 运行）。disabled 时
    // 不装配 → AppState.interlock=None（未启用部署行为不变）。
    let interlock_ctl: Option<Arc<crate::interlock::InterlockController>> = if config.io.enabled {
        let il = Arc::new(crate::interlock::InterlockController::new(
            config.io.clone(),
            Box::new(intercore.clone()), // Arc<IntercoreClient> → InterlockPort
            storage.events.clone(),
            sse_push.clone(),
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
    // 联锁后端两种签名（**同一实现**，单元 J）：
    // - `interlock_backend` = 契约版（`display-proto`）⇒ 本地屏控制通道的两条写端点；
    // - `interlock_api` = 旧 web-api trait，经 `WebInterlockApi` 过渡适配器（K 的删除面）
    //   ⇒ `AppState.interlock`（web 出口）与读通道 `display_host::interlock_wiring_for`。
    let interlock_backend: Option<Arc<dyn mupc_display_proto::InterlockApi>> = interlock_ctl
        .clone()
        .map(|c| c as Arc<dyn mupc_display_proto::InterlockApi>);
    let interlock_api: Option<Arc<dyn mupc_web_api::app_state::InterlockApi>> = interlock_ctl
        .clone()
        .map(|c| Arc::new(WebInterlockApi(c)) as Arc<dyn mupc_web_api::app_state::InterlockApi>);

    // AI 决策循环：周期执行决策并分发到核间/南向（RL 决策 <1s）
    // Task7：联锁 latch 期间抑制 dispatch（skip 本轮 warn；transport 层另有 stopped_latched 兜底）
    let decision_integrator = ai_integrator.clone();
    let decision_sse = sse_push.clone();
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
                // 推送策略下发事件（SSE 生产者；AI 引擎已停用 2026-09-09，每拍实际为本地台区
                // 储能治理 run_fallback_strategies 下发——沿用 AiDecision 通道，仅文案中性化）
                let _ = decision_sse.push_ai_decision("策略下发完成");
            }
        }
    }));

    // ── 12-本地显示终端数据提供层（12-显示终端 设计 §4.2 装配点：策略引擎(第8步)+决策循环之后）──
    // config.display.enabled 时：先起 DisplayDataProvider（1s 采集组帧，SOC 取 AiIntegrator 裁决
    // 快照、三相/run_state 取 intercore），再起 LoopbackHttpPublisher（127.0.0.1 GET 最新帧）。
    // 两 handle 都入 guard（优雅退出随其它后台任务 abort）；主进程不 spawn/不管理渲染子进程
    // （渲染生命周期归 systemd，§4.2/§11）。disabled 不装配（warn）。
    if config.display.enabled {
        // §7.3 warn：非 modbus_rtu（tcp 仿真/联调）可看 SOC/通道，三相 1022-1032 将 NotRead
        if config.intercore.transport != "modbus_rtu" {
            tracing::warn!(
                "display.enabled=true 但 intercore.transport={}（非 modbus_rtu）：仿真/联调可看 SOC/通道，三相 1022-1032 将 NotRead（12-显示终端 §7.3）",
                config.intercore.transport
            );
        }
        let latest: Arc<std::sync::Mutex<Option<mupc_display_proto::DisplayFrame>>> =
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
            interlock_api.clone(),
            config.io.enabled,
            config.io.release_hold_secs,
        );
        let provider = crate::display_host::DisplayDataProvider::new(
            ai_integrator.clone(),
            intercore.clone(),
            &config.display,
            config.intercore.transport == "modbus_rtu",
            latest.clone(),
        )
        .with_slow_sources(
            Some(Arc::new(crate::display_host::SystemDeviceSource::new(
                intercore.clone(),
                ai_integrator.clone(),
                process_started_at,
            ))),
            Some(Arc::new(crate::display_host::StorageAlarmSource::new(
                storage.events.clone(),
                config.display.alarm_page_size,
            ))),
            interlock_wiring,
        );
        guard.0.push(tokio::spawn(provider.run()));
        let publisher = crate::display_host::LoopbackHttpPublisher::new(latest.clone());
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
            crate::log_service::LogService::new(
                config.system.log_dir.clone(),
                config.display.log,
            ),
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
        });
        match tokio::net::TcpListener::bind(&config.display.control_bind_addr).await {
            Ok(listener) => {
                // 回环裁决在 `ConsoleHost::serve` 内按**实际绑定结果**再判一次（PL-4 二次兜底）：
                // 配置层 `CoreConfig::validate_display` **已**调用契约的 `DisplayConfig::validate()`，
                // 两条地址（bind_addr / control_bind_addr）在启动期即强制字面量回环、端口≠0、两址不同；
                // 本处兜底防的是"配置校验被绕过 / 被新增调用路径跳过"。
                guard.0.push(tokio::spawn(async move {
                    if let Err(e) = console.serve(listener).await {
                        tracing::error!(
                            "控制通道 serve 异常退出（非回环地址会被拒绝）: {}",
                            e
                        );
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
    } else {
        tracing::debug!(
            "本地显示终端未启用（config.display.enabled=false），跳过 DisplayDataProvider/回环发布"
        );
    }

    // ── 9. IEC 104 网关 ──
    tracing::info!("[09/14] 初始化 IEC 104 网关...");
    // 审查 R2-A2 (2026-09-09)：北向监听地址/端口读 config.gateway 段（缺省 0.0.0.0:2404，
    // 见 core_config.rs GatewayConfig::default——不再硬编码 2404）。
    let iec104_config = mupc_gateway::iec104::server::Iec104Config {
        listen_addr: config.gateway.listen_addr.clone(),
        listen_port: config.gateway.listen_port,
        ..Default::default()
    };
    tracing::info!(
        "IEC 104 监听 {}:{}",
        config.gateway.listen_addr,
        config.gateway.listen_port
    );
    let iec104_server = Arc::new(mupc_gateway::iec104::server::Iec104Server::new(iec104_config));
    let cmd_handler = Arc::new(StrategyCommandHandler {
        intercore: intercore.clone(),
        interlock: interlock_ctl.clone(),
        p_max_kw,
    });
    let server_clone = iec104_server.clone();
    guard.0.push(tokio::spawn(async move {
        if let Err(e) = server_clone.start(cmd_handler).await {
            tracing::error!("IEC 104 服务器异常退出: {}", e);
        }
    }));
    coord.register_service("gateway", ServiceStatus::Running);

    // ── S3 §10.3：策略 phase 源装配（master_meter 段已删除收敛，2026-09-09 S3b-1c）──
    //   B. south_stations.stations 非空 → southd scheduler：grid 单写 AiIntegrator。
    //      B1. 含 meter_grid → grid_on=true（SouthSink.on_grid_package 单写；offline 由事件 +
    //          AiIntegrator 5s 闸门判断，pv/load 不兜底 set_latest_data——保持 "grid 单写方"）。
    //      B2. 仅非 grid 站（无 meter_grid）→ grid_on=false → pv/load 南向模拟兜底测量（无 grid
    //          源时 AiIntegrator 不断供）。
    //   C. 无 stations → grid_on=false → pv/load 南向模拟兜底。
    // grid_on = 策略 phase 源可用（决定下方 pv/load 南向模拟 task 是否 set_latest_data；
    // M-4 防双写方并存：grid 源在即南向模拟不覆盖；无 grid 源则南向模拟兜底测量）。
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
        let sink = Arc::new(SouthSink::new(
            ai_integrator.clone(),
            write_buffer.clone(),
            storage.events.clone(),
            sse_push.clone(),
            // 审查 R2-A2：meter_grid 真值上送 IEC104 的接收句柄（已在步骤 9 创建）
            iec104_server.clone(),
        ));
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
        let scheduler = mupc_southd::scheduler::SouthScheduler::new(
            config.south_stations.clone(),
            buses,
            sink,
        );
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

    // 南向数据采集循环（上行）：读取 → 转换 → 持久化 + 北向 gateway 上送。
    // AI 观测注入已停（平台目标调整 2026-09-09：观测维度重构前停采）。
    if let (Some(pv), Some(load)) = (pv_device.clone(), load_device.clone()) {
        let wb = write_buffer.clone();
        let g = iec104_server.clone();
        let ai_int = ai_integrator.clone();
        guard.0.push(tokio::spawn(async move {
            let grid_on = grid_on; // grid 策略源在时（southd 含 meter_grid）由它提供，南向模拟不覆盖
            // FIXME: IOA 分配和发送序号按连接维护，这里用固定值
            let mut ioa_seq = 0u32;
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
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
                            // 北向上送（仅无 grid 源兜底路径；审查 R2-A2）：grid_on 时 meter_grid
                            // 真值已由 SouthSink 以固定 IOA 1..6 上送，此 pv/load 假遥测 ioa_seq
                            // 亦自 1 递增 → 若仍上送会与真值 IOA 相撞（M-4 同款单写方语义，
                            // grid 源在即南向模拟不覆盖北向）。取有功功率作为示例（FIXME: 完整点表映射）
                            if !grid_on {
                                if let Some(v) = pkg.electrical.active_power {
                                    ioa_seq = ioa_seq.wrapping_add(1);
                                    let asdu = mupc_gateway::iec104::protocol::encode_telemetry_asdu(
                                        ioa_seq,
                                        v as f32,
                                        1,
                                    );
                                    let frame = mupc_gateway::iec104::Iec104Frame::make_i_frame(
                                        0, 0, &asdu,
                                    );
                                    g.broadcast_telemetry(frame).await;
                                }
                            }
                        }
                        Err(e) => tracing::debug!("南向采集 {} 失败: {}", name, e),
                    }
                }
            }
        }));
    }

    // ── 10. Web API ──
    // AppState 所需依赖在步骤 07/08/11 中已初始化
    tracing::info!("[10/14] 初始化 Web API...");
    let ota_manager: Arc<dyn mupc_ota_update::OtaManager> =
        Arc::new(mupc_ota_update::manager::OtaManagerImpl::new(
            mupc_ota_update::OtaConfig::default(),
            config.system.data_dir.join("ota"),
        )
        .map_err(|e| MupcError::new(ErrorCode::Unknown, format!("OTA 管理器初始化失败: {}", e), "startup"))?);

    let web_config = Arc::new(tokio::sync::RwLock::new(
        mupc_web_api::routes::config::AppConfig {
            gateway: Default::default(),
            intercore: Default::default(),
            system: Default::default(),
        },
    ));
    // Phase 2+ TODO: 从配置读取管理员用户名，当前硬编码
    let session_manager = mupc_web_api::SessionManager::new("admin".to_string());
    let status_handler = mupc_web_api::routes::StatusHandler::new();
    let logs_handler = mupc_web_api::routes::LogsHandler::new(config.system.log_dir.clone());
    let ws_streamer = mupc_web_api::WsLogStreamer::new();
    let audit_logger = Arc::new(
        mupc_web_api::AuditLogger::new(
            config.system.log_dir.join("audit").to_str().unwrap_or("/opt/mupc/logs/audit"),
        )
        .unwrap_or_else(|e| {
            tracing::warn!("审计日志初始化失败，降级使用 /tmp: {}", e);
            mupc_web_api::AuditLogger::new("/tmp/mupc-audit")
                .expect("审计日志初始化致命失败 — 磁盘满或 /tmp 不可写")
        }),
    );
    // 使用 ModelManager 内部的 online_updater（与 AI 引擎共享同一实例，避免两实例不连通）
    let online_updater = ai_engine.online_updater().clone();
    let ab_test_manager = Arc::new(mupc_web_api::routes::ai::ab_test_manager::AbTestManager::new());
    let mode_selector = ai_engine.mode_selector_arc();

    let app_state = Arc::new(mupc_web_api::AppState {
        config: web_config,
        ai_integrator: ai_integrator.clone(),
        mode_selector,
        sse_push,
        audit_logger,
        session_manager,
        status_handler,
        logs_handler,
        ws_streamer,
        storage: storage.clone(),
        ota_manager: ota_manager.clone(),
        online_updater,
        ab_test_manager,
        // Task7：io.enabled 时注入真实联锁 controller；disabled → None（路由返回 503 语义）
        interlock: interlock_api,
    });

    // 组装 Router 并启动 HTTP 服务
    let app_router = axum::Router::new()
        .merge(mupc_web_api::routes::mode::create_router())
        .merge(mupc_web_api::routes::strategy_mode::create_router())
        .merge(mupc_web_api::routes::ai::ai_routes())
        .merge(mupc_web_api::routes::ai::sse_route())
        .merge(mupc_web_api::routes::status::create_router())
        .merge(mupc_web_api::routes::config::create_router())
        .merge(mupc_web_api::routes::logs::create_router())
        .merge(mupc_web_api::routes::interlock::create_router())
        .merge(mupc_web_api::ws::create_router())
        .merge(mupc_web_api::auth::create_router())
        .with_state(app_state.clone());

    let listen_addr = config.web_api.listen_addr.clone();
    guard.0.push(tokio::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(&listen_addr).await {
            Ok(l) => {
                tracing::info!("Web API 已启动: http://{}", listen_addr);
                l
            }
            Err(e) => {
                tracing::error!("Web API 绑定 {} 失败: {}", listen_addr, e);
                return;
            }
        };
        axum::serve(listener, app_router)
            .await
            .unwrap_or_else(|e| tracing::error!("Web API 服务器异常退出: {}", e));
    }));
    tracing::info!(
        "Web API 配置: listen={}, https={}",
        config.web_api.listen_addr,
        config.web_api.enable_https
    );
    coord.register_service("web_api", ServiceStatus::Running);

    // ── 11. OTA 管理器 (实例已在步骤 10 中创建) ──
    tracing::info!("[11/14] 初始化 OTA 管理器...");
    coord.register_service("ota_update", ServiceStatus::Running);

    // ── 12. 系统资源监控 ──
    tracing::info!("[12/14] 初始化系统资源监控...");
    let metrics_store = Arc::new(mupc_system_monitor::MetricsStore::new(
        config.system.data_dir.join("metrics").to_str().unwrap_or("/tmp/mupc-metrics"),
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
                        if let Ok(Some(healing)) =
                            healing_engine.lock().await.auto_heal(&analysis)
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

    // ── 13. MQTT 桥接 ──
    tracing::info!("[13/14] 初始化 MQTT 桥接...");
    // 审查 R2-B5：由 config.mqtt_bridge.*_enabled 门控。缺省双 false——不再用 Default
    // (mqtt.example.com:8883 + dummy 证书) 无条件构造并 spawn 假域名；启用走原连接逻辑。
    let mut mqtt_spawned = false;
    if config.mqtt_bridge.local_enabled {
        if let Some(local) = mupc_mqtt_bridge::LocalMqttClient::new(
            &mupc_mqtt_bridge::LocalMqttConfig::default(),
        )
        .map(Arc::new)
        .inspect_err(|e| tracing::warn!("本地 MQTT 客户端初始化失败: {}", e))
        .ok()
        {
            guard.0.push(tokio::spawn(async move {
                let _ = local.run().await;
            }));
            mqtt_spawned = true;
        }
    } else {
        tracing::debug!("本地 MQTT 未启用（config.mqtt_bridge.local_enabled=false），跳过");
    }
    if config.mqtt_bridge.north_enabled {
        if let Some(north) = mupc_mqtt_bridge::NorthMqttClient::new(
            &mupc_mqtt_bridge::NorthMqttConfig::default(),
        )
        .map(Arc::new)
        .inspect_err(|e| tracing::warn!("北向 MQTT 客户端初始化失败: {}", e))
        .ok()
        {
            guard.0.push(tokio::spawn(async move {
                let _ = north.run().await;
            }));
            mqtt_spawned = true;
        }
    } else {
        tracing::warn!(
            "mqtt-bridge 北向未启用（config.mqtt_bridge.north_enabled=false），跳过——不再默认连 mqtt.example.com 假域名"
        );
    }
    coord.register_service(
        "mqtt_bridge",
        if mqtt_spawned {
            ServiceStatus::Running
        } else {
            ServiceStatus::Stopped
        },
    );

    // ── 14. 近场无线 ──
    tracing::info!("[14/14] 初始化近场无线...");
    // TODO (Phase 2+): 实例化 NoOp 无线驱动
    // 占位 stub：无线驱动未实例化，ECDH/链路加密未真正运行（Phase 2+）
    // ——不谎报 Running（审查 R2-B5）
    coord.register_service("wireless", ServiceStatus::Stopped);

    tracing::info!("所有 14 个子系统初始化完成 ({} 个 TODO 待阶段补全)", 2);

    // 初始化成功，取出 bg_tasks（防止 Drop abort）并移交 StartupContext
    let bg_tasks = std::mem::take(&mut guard.0);
    Ok(StartupContext {
        message_bus,
        storage,
        intercore,
        plugin_loader,
        ai_engine,
        ai_integrator,
        ota_manager,
        fault_recorder,
        background_tasks: bg_tasks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 写路径装配的最小可解析 yaml（只需 `CoreConfig` 里**没有 `#[serde(default)]`** 的段）。
    const MIN_YAML: &str = r#"
version: "1.0"
system: {}
intercore: {}
web_api:
  tls_cert: null
  tls_key: null
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
                assert!(reason.contains("fail-closed"), "原因须点明 fail-closed 口径: {reason}");
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
            crate::console_host::InterlockOpsSource::Ready(_) => panic!(
                "审计建不起来 ⇒ 联锁写路径也**不得**可用（否则同一件事两条通道两种后果）"
            ),
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
}
