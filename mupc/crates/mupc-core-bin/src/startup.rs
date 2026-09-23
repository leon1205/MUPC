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
    /// 遥测写缓冲（P0-1：**退出前必须落盘**的最后一批数据在这里）。
    ///
    /// 持有方式是 `Arc`（**多持有者共享同一缓冲区**，不是各持一份拷贝）：`SouthSink`、
    /// pv/load 南向模拟循环、定时 flush 任务各持一个 `Arc` 克隆，缓冲本体（`Mutex<Vec<…>>`）
    /// 只有一份 ⇒ 退出路径拿到的是**同一个**缓冲，`flush()` 排空的就是生产者刚写进去的那批。
    pub write_buffer: Arc<mupc_storage::WriteBuffer>,
    /// 后台任务句柄（Phase 6 优雅退出时 abort）
    pub background_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl StartupContext {
    /// 优雅退出：**先 abort 后台任务（含遥测定时 flush 任务）→ 再落盘剩余遥测缓冲**。
    ///
    /// **为什么顺序是"先 abort 再 flush"**（P0-1）：生产者（southd 采集 task、pv/load 南向模拟
    /// 循环）本身就在 `background_tasks` 里 —— 若反过来先 flush 再 abort，两次调用之间生产者
    /// 仍可能写入新点，那批新点就**落在 flush 之后**、随进程退出滞留内存（正是本缺陷要消灭的形态）。
    /// 先 abort 把生产者停下，flush 才真正是"最后一批"。
    ///
    /// **为什么放在这里而不是 `main.rs`**：本方法是**唯一**的优雅退出入口（`main.rs` 的
    /// `graceful_shutdown` 调用）。放在这里 ⇒ "忘了 flush"在结构上不可能发生，而不是靠调用方
    /// 记得多打一行；`main.rs` 也就无需知道 `WriteBuffer` 的存在。
    ///
    /// 残留竞态（如实登记，不夸大）：`abort()` 需等到 task 的下一个 await 点才生效，
    /// 毫秒级窗口内仍可能有一次并发 `buffer_telemetry`；但 `flush()` 与它争的是同一把
    /// `buffer` 互斥锁，谁先拿到谁生效、不会漏掉已完成 push 的点。
    pub async fn shutdown(&self) {
        tracing::info!("优雅退出：abort {} 个后台任务", self.background_tasks.len());
        for handle in &self.background_tasks {
            handle.abort();
        }
        self.flush_telemetry_buffer().await;
    }

    /// 落盘遥测缓冲中**剩余的全部**数据（含不足一批的），供退出路径使用。
    pub async fn flush_telemetry_buffer(&self) {
        match self.write_buffer.flush().await {
            Ok(0) => tracing::info!("优雅退出：遥测缓冲已空，无需落盘"),
            Ok(n) => tracing::info!(points = n, "优雅退出：最后一批遥测已落盘"),
            // 失败语义与运行期同源（整批丢弃、不重试）；此处**必须响亮** —— 这是进程最后一次
            // 落盘机会，静默会让"退出丢数据"不可观测。
            Err(e) => tracing::error!(
                error = %e,
                "优雅退出：最后一批遥测落盘失败（本批已丢弃；设计无重试/背压条款，本轮不重试）"
            ),
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
///   + `AlertFeed` 即时投递（单元 K：原 web-api 的 SSE 推送服务已随 crate 删除）。
struct SouthSink {
    ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
    write_buffer: Arc<mupc_storage::WriteBuffer>,
    events: Arc<dyn mupc_storage::EventRepository>,
    /// **即时投递环**（设计 §4.7）。⚠️ **不是 F7 真源**——F7 真源仍是 `storage.events`；
    /// 本字段只做「未落库也能上屏」的可选增强（是否并入 F7 由待裁项 R-07 决定，本轮不裁）。
    alert_feed: Arc<crate::alert_feed::AlertFeed>,
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
        alert_feed: Arc<crate::alert_feed::AlertFeed>,
        iec104: Arc<mupc_gateway::iec104::server::Iec104Server>,
    ) -> Self {
        Self {
            ai_integrator,
            write_buffer,
            events,
            alert_feed,
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
    // ⚠️ 容量口径（P0-1 审查核对项，**未改**）：设计 03:1321 为"100ms 或积累 **100** 条"，
    // 而这里是 `capacity=1000` / `flush_interval_ms=5000`。容量影响吞吐与事务频率，**须设计确认
    // 后再动**（本轮只补"时间触发"这一半，见下方定时任务）；此处就地登记差异，避免下次又被当成
    // "已对齐"。
    let write_buffer = Arc::new(mupc_storage::WriteBuffer::new(
        1000,
        5000,
        storage.pool().clone(),
    ));
    // P0-1：`flush_interval_ms` 的**读取方**（此前无任何读取方 ⇒ 不满一批的数据永久滞留内存）。
    // 句柄入 guard：装配中途失败即随 TaskGuard::drop 一并 abort；成功则随
    // `StartupContext.background_tasks` 移交，退出时由 `shutdown()` abort。
    guard.0.push(write_buffer.clone().spawn_flush_timer());
    coord.register_service("storage", ServiceStatus::Running);

    // ── 4. 核间通信 ──
    tracing::info!("[04/14] 初始化核间通信...");
    // 传输通道由 intercore.transport 决定：modbus_rtu=生产主链路(PCS 真实协议)，
    // tcp=仿真/联调（sim-bridge 作 TCP 服务端）。未知值启动即报错（M3），避免
    // 配置手误静默落到仿真通道、生产 PCS 空转不被控。
    let intercore: Arc<mupc_intercore::IntercoreClient> = match config.intercore.transport.as_str()
    {
        "modbus_rtu" => {
            let mb = &config.intercore.modbus_rtu;
            tracing::info!(
                "intercore transport = modbus_rtu: {} @{}",
                mb.serial_port,
                mb.baud_rate
            );
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
            guard
                .0
                .push(tokio::spawn(transport.clone().run_heartbeat_loop()));
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

    // ── S2 §12.4 / Task7：安全联锁控制器（io.enabled 时装配）──
    // 依赖：intercore(步骤 4) + storage(步骤 3) + alert_feed 均已就绪。GPIO(sysfs) 打开失败由
    // InterlockController::new 内部 fail-safe（预置 latch，绝不静默无 latch 运行）。disabled 时
    // 不装配 → 读通道走 `InterlockWiring::Disabled`（未启用部署行为不变）。
    let interlock_ctl: Option<Arc<crate::interlock::InterlockController>> = if config.io.enabled {
        let il = Arc::new(crate::interlock::InterlockController::new(
            config.io.clone(),
            Box::new(intercore.clone()), // Arc<IntercoreClient> → InterlockPort
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
    // 快照、三相/run_state 取 intercore），再起 LoopbackHttpPublisher（127.0.0.1 GET 最新帧）。
    // 两 handle 都入 guard（优雅退出随其它后台任务 abort）；主进程不 spawn/不管理渲染子进程
    // （渲染生命周期归 systemd，§4.2/§11）。disabled 不装配（warn）。
    // IEC 104 服务器**实例提前构造**（步骤 9 只做 `start()`）：HMI 的装置状态源要读它的
    // 链路状态（`Iec104Server::link_state()`，U-59 / L-5），而 HMI 装配在步骤 8 末尾、
    // 早于步骤 9。实例构造**无 I/O**（只建连接表/通道），提前无副作用。
    let iec104_server = Arc::new(mupc_gateway::iec104::server::Iec104Server::new(
        mupc_gateway::iec104::server::Iec104Config {
            listen_addr: config.gateway.listen_addr.clone(),
            listen_port: config.gateway.listen_port,
            ..Default::default()
        },
    ));

    if config.display.enabled {
        // 设计 §4.9 字面稿的「初始化本地 HMI 后端」日志行（第一轮整改 S-3：原先只存在于设计里，
        // 实现无对应日志 ⇒ 现场无法从启动日志确认 HMI 后端是否真的在装配）。
        // ⚠️ **不带步号**：设计字面稿写的是 `[10/14]`，而实现里 HMI 装配**并入步骤 8 之后**
        // （`[10/14]` 现为 OTA 管理器）⇒ 带号会与现行 14 步编号体系冲突（登记见
        // `docs/technical-debt.md` U-38）。
        tracing::info!("初始化本地 HMI 后端（读通道 + 控制通道）...");
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
            interlock_backend.clone(),
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
                // IEC 104 链路真源（U-59 / L-5）：实例已在下方提前构造（**尚未** start()，
                // 故此刻读得「未配置」，start() 后自动转「断开/连接中/已连接」）。
                Some(iec104_server.clone()),
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
            alert_feed.clone(),
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
        let scheduler =
            mupc_southd::scheduler::SouthScheduler::new(config.south_stations.clone(), buses, sink);
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
                                    let asdu =
                                        mupc_gateway::iec104::protocol::encode_telemetry_asdu(
                                            ioa_seq, v as f32, 1,
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

    // ── 13. MQTT 桥接 ──
    tracing::info!("[13/14] 初始化 MQTT 桥接...");
    // 审查 R2-B5：由 config.mqtt_bridge.*_enabled 门控。缺省双 false——不再用 Default
    // (mqtt.example.com:8883 + dummy 证书) 无条件构造并 spawn 假域名；启用走原连接逻辑。
    let mut mqtt_spawned = false;
    if config.mqtt_bridge.local_enabled {
        // 用 `match` 而非 `…inspect_err(..).ok()`：后者所需的 `inspect_err` 稳定于
        // Rust 1.76，高于本仓声明的 MSRV（1.75）⇒ 保持 MSRV 干净。
        match mupc_mqtt_bridge::LocalMqttClient::new(&mupc_mqtt_bridge::LocalMqttConfig::default())
        {
            Ok(local) => {
                let local = Arc::new(local);
                guard.0.push(tokio::spawn(async move {
                    let _ = local.run().await;
                }));
                mqtt_spawned = true;
            }
            Err(e) => tracing::warn!("本地 MQTT 客户端初始化失败: {}", e),
        }
    } else {
        tracing::debug!("本地 MQTT 未启用（config.mqtt_bridge.local_enabled=false），跳过");
    }
    if config.mqtt_bridge.north_enabled {
        // 同上：避开 `inspect_err`（1.76 稳定 vs MSRV 1.75）。
        match mupc_mqtt_bridge::NorthMqttClient::new(&mupc_mqtt_bridge::NorthMqttConfig::default())
        {
            Ok(north) => {
                let north = Arc::new(north);
                guard.0.push(tokio::spawn(async move {
                    let _ = north.run().await;
                }));
                mqtt_spawned = true;
            }
            Err(e) => tracing::warn!("北向 MQTT 客户端初始化失败: {}", e),
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
        write_buffer,
        background_tasks: bg_tasks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    #[test]
    fn telemetry_buffer_timer_and_shutdown_flush_are_wired() {
        let production = production_src();
        assert!(
            production.contains("spawn_flush_timer()"),
            "装配点必须起定时 flush 任务（否则 `flush_interval_ms` 又变成无读取方的死字段）"
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
}
