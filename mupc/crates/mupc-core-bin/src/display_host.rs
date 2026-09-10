//! 本地显示终端数据提供层（mupcd 侧，12-本地显示终端 设计 §4.2/§5，core-bin 内模块）。
//!
//! 组件（命名对齐设计 §3.2）：
//! - [`DisplayDataProvider`]：采集 + 组帧 + 发布。每 `publish_ms`（默认 1s，`display-proto`
//!   `DEFAULT_PUBLISH_MS`）组一帧 [`DisplayFrame`]——SOC 取 AiIntegrator 裁决快照（§4.3 唯一
//!   裁决入口）、run_state/pcs_online/三相取 intercore（`read_three_phase`/`last_run_state`/
//!   `is_connected`），原子写入共享 `latest`（`Arc<Mutex<Option<DisplayFrame>>>`，§3.5）。
//! - [`LoopbackHttpPublisher`]：127.0.0.1 回环 HTTP 短轮询端点 `GET /v1/display/latest`
//!   （§3.1/§5 决策 A1），返回最新帧 JSON；未就绪返回 503（渲染端视同无新帧重试）。
//!
//! 「不造假值」总原则（§8）：所有数值展示仅当对应 [`FieldFlag`] == `Valid`；源不可得一律显式
//! 打标（`Offline`=PCS 离线/核间读失败 / `NotRead`=transport 不支持 / `RangeError`=量程越界），
//! 值置 `None`——不补 0、不沿用陈旧值冒充实时。SOC 双源皆失时冻结值仅在控制内部、不送上屏。

use mupc_display_proto::{
    DisplayConfig, DisplayFrame, DisplayRange, Field, FieldFlag, RunState, SocSource,
    PROTO_VERSION,
};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 帧共享存储：provider 每 tick 原子更新；publisher 每请求 clone 返回。
/// 设计 §3.5：HTTP 路径不做任何 modbus 读（采集在专用 1s task，避免并发总线抖动）。
pub type SharedLatest = Arc<Mutex<Option<DisplayFrame>>>;

/// 最新帧端点路径（设计 §3.1，与 `display-proto` `DEFAULT_CHANNEL_URL` 尾部一致）。
pub const LATEST_PATH: &str = "/v1/display/latest";

/// 回环 publisher 单连接请求头读超时（O3：连接后不发数据的对端不得长期占用 task）。
pub const HEAD_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// 采集+组帧+发布组件（设计 §4.2 DisplayDataProvider）。
pub struct DisplayDataProvider {
    /// AiIntegrator（SOC 唯一裁决入口，只读快照，不参与控制态）。
    ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
    /// IntercoreClient（三相展示读 + run_state/连接态查询）。
    intercore: Arc<mupc_intercore::IntercoreClient>,
    /// 域值化量程（设计 §4.4：越界 → RangeError）。
    range: DisplayRange,
    /// 采集/组帧/发布周期 ms（设计 §3.5，标称 1Hz）。
    publish_ms: u64,
    /// transport==modbus_rtu 时三相缺段读 = `Offline`（PCS 离线/读失败）；否则（tcp/sim
    /// 无 PCS 3 区点表）transport 不支持 = `NotRead`（设计 §4.1/§4.4）。
    modbus_transport: bool,
    /// 单调发布序号（重启清零；渲染端判连续/重排，§3.3）。
    seq: u64,
    /// 最新帧共享存储（与 LoopbackHttpPublisher 共享同一 Arc）。
    latest: SharedLatest,
}

impl DisplayDataProvider {
    /// 创建提供层（`modbus_transport` = `config.intercore.transport=="modbus_rtu"`，
    /// 启动侧从 core_config 判定传入，用于 Offline/NotRead 区分）。
    pub fn new(
        ai_integrator: Arc<mupc_strategy_engine::AiIntegrator>,
        intercore: Arc<mupc_intercore::IntercoreClient>,
        cfg: &DisplayConfig,
        modbus_transport: bool,
        latest: SharedLatest,
    ) -> Self {
        Self {
            ai_integrator,
            intercore,
            range: cfg.range.clone(),
            publish_ms: cfg.publish_ms.max(50), // 防 0/极小周期空耗（KISS 下限 50ms）
            modbus_transport,
            seq: 0,
            latest,
        }
    }

    /// 后台 1s 采集/组帧/发布主循环（startup 装配 `config.display.enabled` 时 spawn）。
    pub async fn run(mut self) {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(self.publish_ms)).await;
            self.sample_once().await;
        }
    }

    /// 采一帧并发布（帧 seq 递增、ts=now、原子写 latest），返回该帧（供测试单拍断言）。
    pub async fn sample_once(&mut self) -> DisplayFrame {
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        let mut frame = self.build_frame(now_ms).await;
        frame.seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        // O2：毒化不 panic——仓库既有风格取回内部值（采集路径不得因一次 panic 永久失效）。
        let mut g = self.latest.lock().unwrap_or_else(|e| e.into_inner());
        *g = Some(frame.clone());
        frame
    }

    /// 组帧（域值化 + 逐字段打 flag + 6.6 一致性）。seq 由 sample_once 写入。
    async fn build_frame(&self, now_ms: u64) -> DisplayFrame {
        // ── F1 SOC：AiIntegrator 唯一裁决入口（不重判源；双失/无 fresh → Lost 不上冻结值）──
        let snap = self.ai_integrator.soc_display_snapshot().await;
        let (soc, soc_source, soc_flag) = if snap.dual_lost || snap.value_pct.is_none() {
            (None, SocSource::Lost, FieldFlag::Offline)
        } else {
            let source = match snap.source {
                mupc_strategy_engine::ai_integration::SocSourceKind::Bms => SocSource::Bms,
                mupc_strategy_engine::ai_integration::SocSourceKind::PcsReg1010 => {
                    SocSource::PcsReg1010
                }
                mupc_strategy_engine::ai_integration::SocSourceKind::None => SocSource::Lost,
            };
            // 裁决值由 resolve 天然规避非有限/越 0..100（§4.4）→ 能展示即 Valid
            (snap.value_pct, source, FieldFlag::Valid)
        };

        // ── F2 run_state（1013 心跳维护）+ 核间链路在线 ──
        let run_state = self.intercore.last_run_state().and_then(RunState::from_raw);
        let online = self.intercore.is_connected().await;
        let pcs_online = online && run_state.is_some();

        // ── F3/F4 三相（1022-1032 已 ×0.1 工程值，intercore 侧解码；量程校验在本层）──
        let three = self.intercore.read_three_phase().await;
        // transport 缺 PCS 3 区点表 → NotRead；modbus 读失败 → Offline（§4.4）
        let missing = if self.modbus_transport {
            FieldFlag::Offline
        } else {
            FieldFlag::NotRead
        };
        let (p_phase, p_total, i_phase) = match three {
            Some(tr) => {
                let p = Self::phase_fields(tr.p_phase, self.range.phase_power_max_kw, missing);
                let i = Self::phase_fields(tr.i_phase, self.range.current_max_a, missing);
                let total = match tr.p_total {
                    None => Field { v: None, flag: missing },
                    Some(v) => Self::scalar_field(v, self.range.total_power_max_kw),
                };
                (p, total, i)
            }
            None => (
                [Field { v: None, flag: missing }; 3],
                Field { v: None, flag: missing },
                [Field { v: None, flag: missing }; 3],
            ),
        };

        // ── 6.6 一致性：run∈{充/放} 且 Σp_phase 与预期方向显著反向 → true（仅佐证，主状态仍 1013）──
        let inconsistency =
            Self::check_inconsistency(run_state, &p_phase, self.range.inconsistency_threshold_kw);

        DisplayFrame {
            version: PROTO_VERSION,
            seq: 0, // sample_once 写入真实 seq
            ts_ms: now_ms,
            soc,
            soc_source,
            soc_flag,
            run_state,
            pcs_online,
            p_phase,
            p_total,
            i_phase,
            inconsistency,
        }
    }

    /// 一段三相读数 → [Field;3]：段缺失全打 missing（Offline/NotRead）；元素量程/有限性校验
    /// 越界 → RangeError（值 None，不补 0）。
    fn phase_fields(raw: Option<[f64; 3]>, max: f64, missing: FieldFlag) -> [Field; 3] {
        match raw {
            None => [Field { v: None, flag: missing }; 3],
            Some(arr) => {
                let mut out = [Field { v: None, flag: missing }; 3];
                for (i, v) in arr.iter().enumerate() {
                    out[i] = Self::scalar_field(*v, max);
                }
                out
            }
        }
    }

    /// 单值域值化：有限且 |v| ≤ 量程 → Valid；否则 RangeError（值 None）。
    fn scalar_field(v: f64, max: f64) -> Field {
        if v.is_finite() && v.abs() <= max {
            Field {
                v: Some(v),
                flag: FieldFlag::Valid,
            }
        } else {
            Field {
                v: None,
                flag: FieldFlag::RangeError,
            }
        }
    }

    /// 6.6 佐证一致性（纯逻辑，可单测）：三相有功均有效才求 Σ；run=充(2) 时 Σ 显著为正、
    /// run=放(3) 时 Σ 显著为负 → 方向不一致 true；其余 false（含任一相缺失 → 无佐证输入）。
    fn check_inconsistency(run: Option<RunState>, p: &[Field; 3], threshold: f64) -> bool {
        let mut sum = 0.0;
        for f in p.iter() {
            match f.v {
                Some(v) => sum += v,
                None => return false, // 任一相无效 → 无可靠 Σ 佐证，不妄断方向
            }
        }
        match run {
            Some(RunState::Charge) => sum > threshold,
            Some(RunState::Discharge) => sum < -threshold,
            _ => false,
        }
    }
}

/// 回环 HTTP 发布组件（设计 §3.2 LoopbackHttpPublisher）：127.0.0.1 仅回环，GET 最新帧 JSON。
pub struct LoopbackHttpPublisher {
    latest: SharedLatest,
}

impl LoopbackHttpPublisher {
    pub fn new(latest: SharedLatest) -> Self {
        Self { latest }
    }

    /// 常驻 accept 循环（startup 装配时 spawn）。每连接独立 task（KISS，逐连接短读短写，
    /// 渲染端每轮新建连接，不依赖 keep-alive，§3.1）。
    pub async fn serve(self, listener: TcpListener) {
        loop {
            match listener.accept().await {
                Ok((stream, _peer)) => {
                    let latest = self.latest.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle(stream, latest).await {
                            tracing::debug!("display loopback 应答失败: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("display loopback accept 失败: {}（退避 100ms）", e);
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// 处理单连接：读至请求头结束 → 校验 `GET /v1/display/latest` → 200 + 最新帧 JSON /
    /// 503（未就绪）/ 404（其它路径）。`Connection: close`（每次连接即关，渲染端每轮新建连接亦可）。
    /// 读到 `\r\n\r\n`（头结束）再应答：避免收到缓冲残留未读数据时 close 触发 Windows RST，
    /// 保证客户端得到干净的 FIN/EOF。
    async fn handle(mut stream: TcpStream, latest: SharedLatest) -> std::io::Result<()> {
        // O3：整段请求头读取套**总时限**（非每字节各自计时）——慢速滴字节的对端同样无法长期
        // 占用本 task（每连接独立 task，accept 无并发上限，此超时是最廉价的兜底）。
        let head = match tokio::time::timeout(HEAD_READ_TIMEOUT, Self::read_head(&mut stream)).await {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Ok(()), // 超时：直接关闭连接（渲染端本就有 2s GET 超时）
        };
        let head_owned = String::from_utf8_lossy(&head);
        let mut parts = head_owned.split_whitespace();
        let method = parts.next().unwrap_or("");
        let raw_path = parts.next().unwrap_or("");
        let path = match raw_path.find('?') {
            Some(i) => &raw_path[..i],
            None => raw_path,
        };

        // 短锁 clone 后出锁再序列化（HTTP 路径不持锁做序列化/IO；设计 §3.5 不在 HTTP 读 modbus）
        let frame = latest.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let (status, body) = if method == "GET" && path == LATEST_PATH {
            match frame {
                // O4：序列化失败明确 500 + 空体（原实现 200 + 空体，对端会当成功帧却解不出）。
                Some(f) => match serde_json::to_vec(&f) {
                    Ok(b) => ("200 OK", b),
                    Err(e) => {
                        tracing::warn!("display 帧序列化失败: {e}（应答 500，不计为成功帧）");
                        ("500 Internal Server Error", Vec::new())
                    }
                },
                // 未就绪 → 503，渲染端视同无新帧重试（§3.1）
                None => ("503 Service Unavailable", Vec::new()),
            }
        } else {
            ("404 Not Found", Vec::new())
        };
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).await?;
        if !body.is_empty() {
            stream.write_all(&body).await?;
        }
        stream.flush().await?;
        Ok(())
    }

    /// 逐字节收至请求头结束（GET 无 body；最多 4096 字节防异常长头）。EOF → 返回已收内容。
    /// 由 [`Self::handle`] 套总时限（[`HEAD_READ_TIMEOUT`]）调用（O3）。
    async fn read_head(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
        let mut head = Vec::with_capacity(128);
        let mut b = [0u8; 1];
        loop {
            if stream.read(&mut b).await? == 0 {
                break;
            }
            head.push(b[0]);
            if head.ends_with(b"\r\n\r\n") || head.len() > 4096 {
                break;
            }
        }
        Ok(head)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mupc_display_proto::{DisplayConfig, DisplayRange, FieldFlag, SocSource};
    use std::time::Instant;

    // ── 测试桩：核间 transport（可控三相/run_state/连接态），下行接口返回默认 ──
    #[derive(Clone)]
    struct StubIntercore {
        soc: Option<(f64, Instant)>,
        run: Option<u16>,
        connected: bool,
        three: Option<mupc_intercore::transport::ThreePhaseRead>,
    }

    #[async_trait::async_trait]
    impl mupc_intercore::IntercoreTransport for StubIntercore {
        async fn send_dual_param(
            &self,
            _c: &mupc_intercore::DualParamCommand,
        ) -> Result<(), mupc_common::MupcError> {
            Ok(())
        }
        async fn send_tai_command(
            &self,
            _p: [f64; 3],
            _q: [f64; 3],
            _m: &str,
        ) -> Result<(), mupc_common::MupcError> {
            Ok(())
        }
        async fn is_connected(&self) -> bool {
            self.connected
        }
        async fn shutdown(&self) -> Result<(), mupc_common::MupcError> {
            Ok(())
        }
        async fn latest_soc(&self) -> Option<(f64, Instant)> {
            self.soc
        }
        async fn stop(&self) -> Result<(), String> {
            Ok(())
        }
        async fn is_interlock_stopped(&self) -> bool {
            false
        }
        async fn restore_interlock_latched(&self, _b: bool) -> Result<(), String> {
            Ok(())
        }
        fn last_run_state(&self) -> Option<u16> {
            self.run
        }
        async fn authorize_restart(&self) -> Result<(), String> {
            Ok(())
        }
        async fn read_three_phase(&self) -> Option<mupc_intercore::transport::ThreePhaseRead> {
            self.three
        }
    }

    fn stub_client(
        run: Option<u16>,
        connected: bool,
        three: Option<mupc_intercore::transport::ThreePhaseRead>,
    ) -> Arc<mupc_intercore::IntercoreClient> {
        Arc::new(mupc_intercore::IntercoreClient::with_transport(Arc::new(
            StubIntercore {
                soc: None,
                run,
                connected,
                three,
            },
        )))
    }

    fn cfg() -> DisplayConfig {
        let mut c = DisplayConfig::default();
        c.publish_ms = 50; // 测试单拍调用不依赖 run 循环；小周期仅供 run 冒烟
        c
    }

    fn valid_three() -> mupc_intercore::transport::ThreePhaseRead {
        mupc_intercore::transport::ThreePhaseRead {
            i_phase: Some([22.5, 22.1, 22.3]),
            p_phase: Some([12.3, 11.8, 12.0]),
            p_total: Some(36.1),
        }
    }

    /// BMS fresh SOC + PCS 在线(放 3) + 三相全有效 → 帧各字段 Valid、pcs_online、一致性 false
    #[tokio::test]
    async fn frame_all_valid_bms_soc_discharge_consistent() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        ai.set_battery_soc(65.5).await; // fresh BMS → snapshot=Bms
        let mut provider = DisplayDataProvider::new(
            ai.clone(),
            stub_client(Some(3), true, Some(valid_three())),
            &cfg(),
            true,
            Arc::new(Mutex::new(None)),
        );
        let f = provider.sample_once().await;
        assert_eq!(f.version, PROTO_VERSION);
        assert_eq!(f.seq, 0);
        assert_eq!(f.soc, Some(65.5));
        assert_eq!(f.soc_source, SocSource::Bms);
        assert_eq!(f.soc_flag, FieldFlag::Valid);
        assert_eq!(f.run_state, Some(RunState::Discharge));
        assert!(f.pcs_online);
        assert_eq!(f.p_phase[0], Field { v: Some(12.3), flag: FieldFlag::Valid });
        assert_eq!(f.p_total, Field { v: Some(36.1), flag: FieldFlag::Valid });
        assert_eq!(f.i_phase[2], Field { v: Some(22.3), flag: FieldFlag::Valid });
        // 放(3) + Σp 显著为正 → 方向一致
        assert!(!f.inconsistency);
    }

    /// 双源皆失 SOC + PCS 离线（run None/connected false/三相缺段, modbus）→
    /// soc=None/Lost、run None、pcs_online false、三相 Offline（值 None 不造假；冻结 SOC 不上屏）
    #[tokio::test]
    async fn frame_dual_lost_and_offline_flags_not_faked() {
        use mupc_data_processing::telemetry::{
            BatteryData, DataPackage, DeviceStatus, ElectricalData, InverterStatus,
        };
        // 复用同一 intercore 桩：作为 AiIntegrator 的活读 client（latest_soc=None → 无 fresh）
        // 与 provider 的三相/run_state/连接源（PCS 离线）。BMS 未注入 + existing 冻结 30 → 双源皆失
        let shared_client = stub_client(None, false, None);
        let mut ai = mupc_strategy_engine::AiIntegrator::new();
        ai.set_intercore_client(shared_client.clone());
        ai.set_latest_data(DataPackage {
            timestamp: 0,
            electrical: ElectricalData::default(),
            device_status: DeviceStatus {
                inverter_status: InverterStatus::Running,
                pv_power: None,
                load_power: None,
                ev_charger_power: None,
            },
            battery: BatteryData {
                soc: Some(30.0),
                soh: None,
                temperature: None,
            },
        })
        .await;

        let mut provider = DisplayDataProvider::new(
            Arc::new(ai),
            shared_client,
            &cfg(),
            true, // modbus_rtu
            Arc::new(Mutex::new(None)),
        );
        let f = provider.sample_once().await;
        assert_eq!(f.soc, None, "双源皆失 → soc=None（冻结值 30 不上屏）");
        assert_eq!(f.soc_source, SocSource::Lost);
        assert_eq!(f.run_state, None);
        assert!(!f.pcs_online);
        for ph in &f.p_phase {
            assert_eq!(ph.v, None);
            assert_eq!(ph.flag, FieldFlag::Offline);
        }
        assert_eq!(f.p_total.flag, FieldFlag::Offline);
        assert!(!f.inconsistency);
    }

    /// transport=tcp（无 PCS 3 区点表）：三相 read None → NotRead（值 None），SOC 仍可看（BMS）
    #[tokio::test]
    async fn frame_tcp_transport_three_phase_not_read() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        ai.set_battery_soc(50.0).await;
        let mut provider = DisplayDataProvider::new(
            ai.clone(),
            stub_client(None, true, None), // tcp 通道：无 run_state、无三相点表
            &cfg(),
            false, // 非 modbus → NotRead
            Arc::new(Mutex::new(None)),
        );
        let f = provider.sample_once().await;
        assert_eq!(f.soc, Some(50.0));
        assert_eq!(f.soc_source, SocSource::Bms);
        for ph in &f.p_phase {
            assert_eq!(ph.flag, FieldFlag::NotRead);
            assert_eq!(ph.v, None);
        }
        assert_eq!(f.p_total.flag, FieldFlag::NotRead);
    }

    /// 量程越界（p_phase 1000kW > phase_power_max 100）→ RangeError 值 None；未越界相仍 Valid
    #[tokio::test]
    async fn frame_range_error_on_out_of_range_phase() {
        let ai = Arc::new(mupc_strategy_engine::AiIntegrator::new());
        ai.set_battery_soc(50.0).await;
        let three = mupc_intercore::transport::ThreePhaseRead {
            i_phase: Some([500.0, 22.1, 22.3]), // 500A > current_max 300 → RangeError
            p_phase: Some([1000.0, 11.8, 12.0]), // 1000kW > phase_power_max 100 → RangeError
            p_total: Some(1000.0),               // 1000kW > total_power_max 300 → RangeError
        };
        let mut provider = DisplayDataProvider::new(
            ai.clone(),
            stub_client(Some(0), true, Some(three)),
            &cfg(),
            true,
            Arc::new(Mutex::new(None)),
        );
        let f = provider.sample_once().await;
        assert_eq!(f.p_phase[0], Field { v: None, flag: FieldFlag::RangeError });
        assert_eq!(f.p_phase[1], Field { v: Some(11.8), flag: FieldFlag::Valid });
        assert_eq!(f.i_phase[0], Field { v: None, flag: FieldFlag::RangeError });
        assert_eq!(f.i_phase[1], Field { v: Some(22.1), flag: FieldFlag::Valid });
        assert_eq!(f.p_total, Field { v: None, flag: FieldFlag::RangeError });
    }

    // ── 6.6 一致性纯函数（run 与 Σp 方向）──
    fn pf(v: f64) -> Field {
        Field { v: Some(v), flag: FieldFlag::Valid }
    }

    #[test]
    fn inconsistency_detects_direction_mismatch() {
        let charge = Some(RunState::Charge);
        let discharge = Some(RunState::Discharge);
        // 充(2) 却 Σp 显著为正（输出）→ 方向不一致
        assert!(DisplayDataProvider::check_inconsistency(
            charge,
            &[pf(2.0), pf(1.0), pf(1.0)],
            3.0
        ));
        // 放(3) 却 Σp 显著为负（吸收）→ 方向不一致
        assert!(DisplayDataProvider::check_inconsistency(
            discharge,
            &[pf(-2.0), pf(-1.0), pf(-1.0)],
            3.0
        ));
        // 阈值下（|Σ|=3.0 不 > 3.0）→ 一致
        assert!(!DisplayDataProvider::check_inconsistency(
            charge,
            &[pf(1.0), pf(1.0), pf(1.0)],
            3.0
        ));
        // 停机/待机 不判方向
        assert!(!DisplayDataProvider::check_inconsistency(
            Some(RunState::Stop),
            &[pf(5.0), pf(5.0), pf(5.0)],
            3.0
        ));
        // 任一相缺失 → 无佐证输入 → false（不妄断）
        let miss = Field { v: None, flag: FieldFlag::Offline };
        assert!(!DisplayDataProvider::check_inconsistency(
            charge,
            &[pf(2.0), pf(2.0), miss],
            3.0
        ));
    }

    // ── LoopbackHttpPublisher：GET 最新帧 / 未就绪 503 / 错误路径 404 ──

    fn sample_frame(soc: Option<f64>) -> DisplayFrame {
        let missing = Field { v: None, flag: FieldFlag::NotRead };
        DisplayFrame {
            version: PROTO_VERSION,
            seq: 7,
            ts_ms: 1_757_412_000_000,
            soc,
            soc_source: if soc.is_some() { SocSource::Bms } else { SocSource::Lost },
            soc_flag: if soc.is_some() { FieldFlag::Valid } else { FieldFlag::Offline },
            run_state: Some(RunState::Charge),
            pcs_online: true,
            p_phase: [missing; 3],
            p_total: missing,
            i_phase: [missing; 3],
            inconsistency: false,
        }
    }

    async fn connect_and_get(addr: std::net::SocketAddr, req: &str) -> Vec<u8> {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(req.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn http_get_returns_latest_frame_json() {
        let latest: SharedLatest = Arc::new(Mutex::new(Some(sample_frame(Some(65.5)))));
        let publisher = LoopbackHttpPublisher::new(latest.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serve = tokio::spawn(publisher.serve(listener));

        let req = format!("GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", LATEST_PATH);
        let resp = connect_and_get(addr, &req).await;
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 200 OK"), "响应应为 200: {text:?}");
        // 解析 body（header 与 body 以空行分隔）
        let body = text.split("\r\n\r\n").nth(1).unwrap_or("");
        let frame: DisplayFrame = serde_json::from_str(body).unwrap();
        assert_eq!(frame.soc, Some(65.5));
        assert_eq!(frame.seq, 7);

        serve.abort();
    }

    #[tokio::test]
    async fn http_get_not_ready_returns_503() {
        let latest: SharedLatest = Arc::new(Mutex::new(None)); // 未发布过帧
        let publisher = LoopbackHttpPublisher::new(latest.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serve = tokio::spawn(publisher.serve(listener));

        let req = format!("GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n", LATEST_PATH);
        let resp = connect_and_get(addr, &req).await;
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 503"), "未就绪应 503: {text:?}");

        serve.abort();
    }

    #[tokio::test]
    async fn http_wrong_path_returns_404() {
        let latest: SharedLatest = Arc::new(Mutex::new(Some(sample_frame(None))));
        let publisher = LoopbackHttpPublisher::new(latest.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let serve = tokio::spawn(publisher.serve(listener));

        let resp = connect_and_get(addr, "GET /nope HTTP/1.1\r\nHost: x\r\n\r\n").await;
        let text = String::from_utf8_lossy(&resp);
        assert!(text.starts_with("HTTP/1.1 404"), "错误路径应 404: {text:?}");

        serve.abort();
    }

    // DisplayRange/DisplayConfig serde(default) 与 mupcd yaml display 段对齐（KISS 冒烟）
    #[test]
    fn default_range_matches_display_proto() {
        let r = DisplayRange::default();
        assert_eq!(r.current_max_a, 300.0);
        assert_eq!(r.phase_power_max_kw, 100.0);
        assert_eq!(r.total_power_max_kw, 300.0);
        assert_eq!(r.pcs_total_rated_kw, 60.0);
        assert_eq!(r.inconsistency_threshold_kw, 3.0);
    }
}
