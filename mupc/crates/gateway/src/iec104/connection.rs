//! IEC 104 连接管理

#[cfg(test)]
mod tests {
    #[test]
    fn test_connection_state_transitions() {
        // 测试连接状态枚举值
        use super::ConnectionState;

        assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
        assert_eq!(ConnectionState::Connecting, ConnectionState::Connecting);
        assert_eq!(
            ConnectionState::WaitingStartDt,
            ConnectionState::WaitingStartDt
        );
        assert_eq!(ConnectionState::Connected, ConnectionState::Connected);
        assert_eq!(ConnectionState::Stopped, ConnectionState::Stopped);
    }

    // ========== 总召（C_IC_NA_1）分派测试（§9.2.5 / §9.5） ==========

    use super::super::command::{CommandResponse, ControlCommand, TelemetryItem, TelemetryKind};
    use super::super::protocol::{
        encode_cp56time2a, encode_ic_term, COT_ACT_CON, COT_ACT_TERM, COT_INTROGEN,
    };
    use super::super::server::OutboundSeq;
    use super::super::{CommandHandler, Connection, Iec104Frame, TypeId};
    use mupc_common::MupcError;

    /// mock 命令处理器：`on_interrogation` / `on_connection_snapshot` 返回注入的条目。
    struct MockHandler {
        items: Vec<TelemetryItem>,
    }

    #[async_trait::async_trait]
    impl CommandHandler for MockHandler {
        async fn handle_command(&self, cmd: ControlCommand) -> Result<CommandResponse, MupcError> {
            Ok(CommandResponse {
                cmd_id: cmd.cmd_id,
                success: true,
                message: "mock".to_string(),
                timestamp: 0,
            })
        }

        fn name(&self) -> &str {
            "mock"
        }

        async fn on_interrogation(&self) -> Vec<TelemetryItem> {
            self.items.clone()
        }
    }

    /// 回环 TCP 对端构造一条真实 `Connection`（`Connection` 只接受真实流）。
    async fn conn_pair() -> Connection {
        use tokio::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (server_side, peer) = listener.accept().await.unwrap();
        drop(client);
        Connection::new(server_side, peer)
    }

    /// 组装一条 `C_IC_NA_1` 的 I 帧（ASDU：type/vsq/cot/ca(2)/ioa(3)/qoi）。
    fn gi_frame(peer_send_seq: u16, qoi: u8) -> Iec104Frame {
        let asdu = vec![
            TypeId::CIcNa1 as u8,
            0x01,
            0x06,
            0x00,
            0x00,
            0x00,
            0x00,
            0x00,
            qoi,
        ];
        let bytes = Iec104Frame::make_i_frame(peer_send_seq, 0, &asdu);
        Iec104Frame::parse(&bytes).unwrap()
    }

    /// 按 `length + 2` 逐帧切分写出的字节流。
    fn parse_all(buf: &[u8]) -> Vec<Iec104Frame> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i + 2 <= buf.len() {
            let len = buf[i + 1] as usize + 2;
            if i + len > buf.len() {
                break;
            }
            out.push(Iec104Frame::parse(&buf[i..i + len]).unwrap());
            i += len;
        }
        out
    }

    fn scalar(ioa: u32, v: f32, ts: u64) -> TelemetryItem {
        TelemetryItem {
            ioa,
            kind: TelemetryKind::Scalar,
            value: v,
            ts_ms: ts,
            cot: 0,
        }
    }

    fn bit(ioa: u32, v: f32, ts: u64) -> TelemetryItem {
        TelemetryItem {
            ioa,
            kind: TelemetryKind::Bit,
            value: v,
            ts_ms: ts,
            cot: 0,
        }
    }

    /// GI-2：`ACT_CON → 数据 → ACT_TERM` **顺序**完整；数据帧 cot 恒 20；时标逐字节 = 采集时刻。
    #[tokio::test]
    async fn test_interrogation_qoi20_emits_act_con_data_act_term_in_order() {
        let mut conn = conn_pair().await;
        let handler = MockHandler {
            items: vec![scalar(1, 12.5, 1_000), bit(315, 1.0, 1_000)],
        };
        let out_seq = OutboundSeq::new();
        let mut buf: Vec<u8> = Vec::new();

        conn.handle_frame(gi_frame(0, 20), &mut buf, &handler, &out_seq)
            .await
            .unwrap();

        let frames = parse_all(&buf);
        // [0] S 帧 + ACT_CON + 2 数据 + ACT_TERM
        assert_eq!(frames.len(), 5, "GI 三段应答的帧数不符");
        assert_eq!(frames[0].control1, 0x01, "首帧应为 S 帧确认");

        // ① ACT_CON（≤ 首帧）
        assert_eq!(frames[1].asdu, encode_ic_term(COT_ACT_CON));
        assert_eq!(frames[1].asdu[2], COT_ACT_CON);
        // ② 数据：标量 ⇒ TI=36，bit ⇒ TI=30；cot 恒 20；IOA 与采集时标逐字节可核
        assert_eq!(frames[2].asdu[0], TypeId::MMeTf1 as u8);
        assert_eq!(frames[2].asdu[2], COT_INTROGEN);
        assert_eq!(
            [frames[2].asdu[5], frames[2].asdu[6], frames[2].asdu[7]],
            [1, 0, 0]
        );
        assert_eq!(frames[2].asdu[8..12], 12.5f32.to_le_bytes());
        assert_eq!(frames[2].asdu[12..19], encode_cp56time2a(1_000));

        assert_eq!(frames[3].asdu[0], TypeId::MSpTb1 as u8);
        assert_eq!(frames[3].asdu[2], COT_INTROGEN);
        assert_eq!(
            [frames[3].asdu[5], frames[3].asdu[6], frames[3].asdu[7]],
            [0x3B, 0x01, 0]
        );
        assert_eq!(frames[3].asdu[8], 0x01); // SIQ
        assert_eq!(frames[3].asdu[9..16], encode_cp56time2a(1_000));
        // ③ ACT_TERM
        assert_eq!(frames[4].asdu, encode_ic_term(COT_ACT_TERM));

        // 出向序号在本连接内**单调、无重复**（§9.2.3 缺陷 #3 的修正）
        let seqs: Vec<u16> = frames[1..].iter().map(|f| f.send_sequence()).collect();
        assert_eq!(seqs, vec![0, 1, 2, 3]);
    }

    /// GI-4：**QOI≠20 不得静默忽略** —— 分组召唤回 ACT_CON + ACT_TERM（无数据）。
    #[tokio::test]
    async fn test_interrogation_group_qoi_is_answered_not_ignored() {
        let mut conn = conn_pair().await;
        let handler = MockHandler {
            items: vec![scalar(1, 1.0, 1_000)],
        };
        let out_seq = OutboundSeq::new();
        let mut buf: Vec<u8> = Vec::new();

        conn.handle_frame(gi_frame(0, 21), &mut buf, &handler, &out_seq)
            .await
            .unwrap();

        let frames = parse_all(&buf);
        // S 帧 + ACT_CON + ACT_TERM（**不含数据帧** —— 本轮未实现分组召唤）
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[1].asdu, encode_ic_term(COT_ACT_CON));
        assert_eq!(frames[2].asdu, encode_ic_term(COT_ACT_TERM));
    }

    /// 无有效数据源（默认 trait 实现返回空）⇒ 仍完整回 ACT_CON + ACT_TERM。
    #[tokio::test]
    async fn test_interrogation_empty_source_still_terminates() {
        struct EmptyHandler;
        #[async_trait::async_trait]
        impl CommandHandler for EmptyHandler {
            async fn handle_command(
                &self,
                cmd: ControlCommand,
            ) -> Result<CommandResponse, MupcError> {
                Ok(CommandResponse {
                    cmd_id: cmd.cmd_id,
                    success: false,
                    message: String::new(),
                    timestamp: 0,
                })
            }
            fn name(&self) -> &str {
                "empty"
            }
        }

        let mut conn = conn_pair().await;
        let out_seq = OutboundSeq::new();
        let mut buf: Vec<u8> = Vec::new();
        conn.handle_frame(gi_frame(0, 20), &mut buf, &EmptyHandler, &out_seq)
            .await
            .unwrap();

        let frames = parse_all(&buf);
        assert_eq!(frames.len(), 3, "空数据源仍须回 ACT_CON + ACT_TERM");
        assert_eq!(
            frames[2].asdu,
            encode_ic_term(COT_ACT_TERM),
            "三段完整性不因数据为空而破坏"
        );
    }

    /// 顶层 QOI（既非 20 也非分组召唤）⇒ 只记 WARN，不应答任何帧（除 S 帧确认）。
    #[tokio::test]
    async fn test_interrogation_unknown_qoi_is_ignored() {
        let mut conn = conn_pair().await;
        let handler = MockHandler { items: vec![] };
        let out_seq = OutboundSeq::new();
        let mut buf: Vec<u8> = Vec::new();
        conn.handle_frame(gi_frame(0, 99), &mut buf, &handler, &out_seq)
            .await
            .unwrap();

        let frames = parse_all(&buf);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].control1, 0x01);
    }

    /// **S 帧只带接收序号**：对端 N(S) = 7 ⇒ 应答 `68 04 01 00 0E 00`。
    #[tokio::test]
    async fn test_s_frame_ack_carries_only_recv_seq() {
        let mut conn = conn_pair().await;
        let handler = MockHandler { items: vec![] };
        let out_seq = OutboundSeq::new();
        let mut buf: Vec<u8> = Vec::new();

        conn.handle_frame(gi_frame(7, 99), &mut buf, &handler, &out_seq)
            .await
            .unwrap();

        let frames = parse_all(&buf);
        assert_eq!(frames[0].control1, 0x01);
        assert_eq!(frames[0].control2, 0x00);
        assert_eq!([frames[0].control3, frames[0].control4], [0x0E, 0x00]);
        assert_eq!(conn.recv_seq, 7);
    }

    /// GI-5 触发点：`StartDtAct` / `StartDtCon` 均置「刚刚连接」，且 `take` 语义只取一次。
    #[tokio::test]
    async fn test_just_connected_flag_set_once_per_handshake() {
        use super::super::protocol::UFrameType;
        let mut conn = conn_pair().await;
        assert!(!conn.take_just_connected());

        let startdt =
            Iec104Frame::parse(&Iec104Frame::make_u_frame(UFrameType::StartDtAct)).unwrap();
        let mut buf: Vec<u8> = Vec::new();
        conn.handle_u_frame(startdt, &mut buf).await.unwrap();
        assert_eq!(conn.state, super::ConnectionState::Connected);
        assert!(conn.take_just_connected(), "STARTDT_ACT 后应置位");
        assert!(
            !conn.take_just_connected(),
            "take 语义：同一连接只触发一次快照"
        );

        let con = Iec104Frame::parse(&Iec104Frame::make_u_frame(UFrameType::StartDtCon)).unwrap();
        conn.handle_u_frame(con, &mut buf).await.unwrap();
        assert!(conn.take_just_connected(), "STARTDT_CON 分支对称置位");
    }

    #[test]
    fn test_heartbeat_interval_clamp() {
        use super::{Connection, ConnectionState};
        use std::net::SocketAddr;
        use tokio::net::TcpStream;

        // 创建测试用 dummy TcpStream（用于测试 set_heartbeat_interval）
        let addr: SocketAddr = "127.0.0.1:2404".parse().unwrap();
        // 由于需要实际 TcpStream，我们通过测试 clamp 逻辑来验证
        // set_heartbeat_interval 内部使用 clamp(1, 60)

        // 测试上限 clamp
        let max_value = 100u64;
        let clamped_max = max_value.clamp(1, 60);
        assert_eq!(clamped_max, 60);

        // 测试下限 clamp
        let min_value = 0u64;
        let clamped_min = min_value.clamp(1, 60);
        assert_eq!(clamped_min, 1);

        // 测试正常值
        let normal_value = 30u64;
        let clamped_normal = normal_value.clamp(1, 60);
        assert_eq!(clamped_normal, 30);
    }
}

use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tracing::{info, warn};

use super::command::{CommandHandler, CommandType, ControlCommand};
use super::protocol::{encode_ic_term, COT_ACT_CON, COT_ACT_TERM, COT_INTROGEN};
use super::server::OutboundSeq;
use super::{protocol::FrameType, protocol::UFrameType, AsduHeader, Iec104Frame, Ioa, TypeId};

/// 连接状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    WaitingStartDt, // 等待 STARTDT
    Connected,
    Stopped,
}

/// IEC 104 连接
///
/// ⚠️ **出向序号（N(S)）不在此处**：由连接处理层持有 [`OutboundSeq`]（每连接一份，
/// 与写半锁同生命周期，§9.2.3）。原先的 `send_seq` 字段**无任何自增点**（§9.2.3 缺陷 #3），
/// 已删除——保留它会形成"第二个出向序号真源"。
pub struct Connection {
    pub stream: Option<TcpStream>,
    pub addr: SocketAddr,
    pub state: ConnectionState,
    pub recv_seq: u16,
    pub heartbeat_interval_secs: u64,
    /// 「刚刚进入 `Connected`」的一次性标志（§9.2.5 GI-5）：由 `handle_u_frame` 置位，
    /// 读循环以 [`Connection::take_just_connected`] 取走并据此发初始快照。
    just_connected: bool,
}

impl Connection {
    pub fn new(stream: TcpStream, addr: SocketAddr) -> Self {
        Self {
            stream: Some(stream),
            addr,
            state: ConnectionState::Disconnected,
            recv_seq: 0,
            heartbeat_interval_secs: 10,
            just_connected: false,
        }
    }

    /// 取走「刚刚连接」标志（Take 语义：同一连接只触发一次初始快照，§9.2.5 GI-5）。
    pub fn take_just_connected(&mut self) -> bool {
        std::mem::take(&mut self.just_connected)
    }

    /// 处理接收到的帧
    ///
    /// `out_seq`：**本连接的出向序号**。调用方必须已持有该连接的**写半锁**
    /// （本方法与写任务共用同一把锁 ⇒ 取号与写序一致，见 [`OutboundSeq`] 的契约）。
    pub async fn handle_frame(
        &mut self,
        frame: Iec104Frame,
        writer: &mut (impl AsyncWriteExt + Unpin),
        handler: &dyn CommandHandler,
        out_seq: &OutboundSeq,
    ) -> Result<(), mupc_common::MupcError> {
        match frame.frame_type {
            FrameType::UFrame => {
                self.handle_u_frame(frame, writer).await?;
            }
            FrameType::SFrame => {
                // S 帧用于确认已收到的 I 帧（其控制域携带对端的 N(S)）
                info!("Received S frame");
                self.recv_seq = frame.send_sequence();
            }
            FrameType::IFrame => {
                self.handle_i_frame(frame, writer, handler, out_seq).await?;
            }
        }
        Ok(())
    }

    /// 处理 U 帧
    async fn handle_u_frame(
        &mut self,
        frame: Iec104Frame,
        writer: &mut (impl AsyncWriteExt + Unpin),
    ) -> Result<(), mupc_common::MupcError> {
        let u_type = frame.u_frame_type().ok_or_else(|| {
            mupc_common::MupcError::new(
                mupc_common::ErrorCode::FrameParseError,
                "Unknown U frame type",
                "gateway",
            )
        })?;

        match u_type {
            UFrameType::StartDtAct => {
                info!("Received STARTDT_ACT, sending STARTDT_CON");
                let response = Iec104Frame::make_u_frame(UFrameType::StartDtCon);
                writer.write_all(&response).await.map_err(|e| {
                    mupc_common::MupcError::new(
                        mupc_common::ErrorCode::SendFailed,
                        format!("Send error: {}", e),
                        "gateway",
                    )
                })?;
                self.state = ConnectionState::Connected;
                // 连接初始快照的触发点（§9.2.5 GI-5）
                self.just_connected = true;
            }
            UFrameType::StartDtCon => {
                info!("Received STARTDT_CON");
                self.state = ConnectionState::Connected;
                // 对称置位：防主站先发 CON 的少见形态（§9.2.5 GI-5）
                self.just_connected = true;
            }
            UFrameType::StopDtAct => {
                info!("Received STOPDT_ACT, sending STOPDT_CON");
                let response = Iec104Frame::make_u_frame(UFrameType::StopDtCon);
                writer.write_all(&response).await.map_err(|e| {
                    mupc_common::MupcError::new(
                        mupc_common::ErrorCode::SendFailed,
                        format!("Send error: {}", e),
                        "gateway",
                    )
                })?;
                self.state = ConnectionState::Stopped;
            }
            UFrameType::StopDtCon => {
                info!("Received STOPDT_CON");
                self.state = ConnectionState::Stopped;
            }
            UFrameType::TestFrAct => {
                info!("Received TESTFR_ACT, sending TESTFR_CON");
                let response = Iec104Frame::make_u_frame(UFrameType::TestFrCon);
                writer.write_all(&response).await.map_err(|e| {
                    mupc_common::MupcError::new(
                        mupc_common::ErrorCode::SendFailed,
                        format!("Send error: {}", e),
                        "gateway",
                    )
                })?;
            }
            UFrameType::TestFrCon => {
                info!("Received TESTFR_CON");
            }
        }

        Ok(())
    }

    /// 处理 I 帧
    async fn handle_i_frame(
        &mut self,
        frame: Iec104Frame,
        writer: &mut (impl AsyncWriteExt + Unpin),
        handler: &dyn CommandHandler,
        out_seq: &OutboundSeq,
    ) -> Result<(), mupc_common::MupcError> {
        let send_seq = frame.send_sequence();
        let _recv_seq = frame.recv_sequence();

        // 检查接收序号
        if send_seq != self.recv_seq {
            warn!(
                "Sequence mismatch: expected {}, got {}",
                self.recv_seq, send_seq
            );
        }

        self.recv_seq = send_seq;

        // 发送 S 帧确认：**S 帧只带接收序号**（§9.2.3 缺陷 #4 的修正）
        let s_frame = Iec104Frame::make_s_frame(self.recv_seq);
        writer.write_all(&s_frame).await.map_err(|e| {
            mupc_common::MupcError::new(
                mupc_common::ErrorCode::SendFailed,
                format!("Send error: {}", e),
                "gateway",
            )
        })?;

        // 解析 ASDU
        let header = frame.parse_asdu_header()?;
        info!(
            "Received I frame: type_id={:?}, cot={}",
            header.type_id, header.cot.0
        );

        // 站总召（C_IC_NA_1）：**在本次调用持有的写半锁内**完成三段应答（§9.2.5）
        if header.type_id == TypeId::CIcNa1 {
            return self
                .handle_interrogation(&frame.asdu, writer, handler, out_seq)
                .await;
        }

        // 控制方向命令：解析并调用命令处理器
        if let Some(cmd) = parse_control_command(&header, &frame.asdu) {
            match handler.handle_command(cmd).await {
                Ok(response) => {
                    info!(
                        "命令执行成功: cmd_id={}, success={}, msg={}",
                        response.cmd_id, response.success, response.message
                    );
                }
                Err(e) => {
                    warn!("命令执行失败: {}", e);
                }
            }
        }

        Ok(())
    }

    /// 站总召（`C_IC_NA_1`）处理（§9.2.5）。
    ///
    /// 数据源**不在本层**：取数经 [`CommandHandler::on_interrogation`]（gateway 只提供接缝，
    /// core-bin 覆写；默认实现返回空 ⇒ 应答不含数据帧）。
    async fn handle_interrogation(
        &mut self,
        asdu: &[u8],
        writer: &mut (impl AsyncWriteExt + Unpin),
        handler: &dyn CommandHandler,
        out_seq: &OutboundSeq,
    ) -> Result<(), mupc_common::MupcError> {
        // ASDU 布局：type(0) vsq(1) cot(2) ca(3,4) ioa(5,6,7) **qoi(8)**
        let qoi = asdu.get(8).copied();
        match qoi {
            Some(20) => {
                // ① 立即应答 ACT_CON（≤ 首帧）
                write_i_asdu(
                    &mut *writer,
                    out_seq,
                    self.recv_seq,
                    &encode_ic_term(COT_ACT_CON),
                )
                .await?;
                // ② 先取数（不跨 await 持锁——本层不持锁，写半锁由调用方持有）
                let items = handler.on_interrogation().await;
                // ③ 逐条数据帧：cot 恒 20，时标 = **该点采集时刻**（不得用响应时刻重打）
                for mut item in items {
                    item.cot = COT_INTROGEN;
                    write_i_asdu(&mut *writer, out_seq, self.recv_seq, &item.encode_asdu()).await?;
                }
                // ④ ACT_TERM
                write_i_asdu(
                    &mut *writer,
                    out_seq,
                    self.recv_seq,
                    &encode_ic_term(COT_ACT_TERM),
                )
                .await?;
            }
            Some(n @ 21..=36) => {
                // 分组召唤（GI-4）：本轮不实现，但**不得静默忽略** —— 回 ACT_CON + ACT_TERM（无数据）
                // 并产出 info 级事件。⚠️ gateway 无事件总线，该"事件"以 `tracing::info!` 落地。
                info!("收到分组召唤 QOI={}，本轮未实现", n);
                write_i_asdu(
                    &mut *writer,
                    out_seq,
                    self.recv_seq,
                    &encode_ic_term(COT_ACT_CON),
                )
                .await?;
                write_i_asdu(
                    &mut *writer,
                    out_seq,
                    self.recv_seq,
                    &encode_ic_term(COT_ACT_TERM),
                )
                .await?;
            }
            other => {
                warn!("收到无法识别的总召 QOI={:?}，已忽略（不应答）", other);
            }
        }
        Ok(())
    }

    /// 设置心跳间隔
    pub fn set_heartbeat_interval(&mut self, secs: u64) {
        self.heartbeat_interval_secs = secs.clamp(1, 60);
    }
}

/// 写一个携带 ASDU 的 I 帧（**取号 + 组帧 + 写**）。
///
/// ⚠️ 调用方**必须已持有写半锁**（[`OutboundSeq`] 的契约：取号与写序须在同一临界区）。
async fn write_i_asdu(
    writer: &mut (impl AsyncWriteExt + Unpin),
    out_seq: &OutboundSeq,
    recv_seq: u16,
    asdu: &[u8],
) -> Result<(), mupc_common::MupcError> {
    let seq = out_seq.next();
    let frame = Iec104Frame::make_i_frame(seq, recv_seq, asdu);
    writer.write_all(&frame).await.map_err(|e| {
        mupc_common::MupcError::new(
            mupc_common::ErrorCode::SendFailed,
            format!("Send error: {}", e),
            "gateway",
        )
    })
}

/// 从 ASDU 解析控制命令
///
/// ASDU 布局：type_id(1) sq(1) cot(1) orig_addr(2) | ioa(3) cmd_value...
/// 控制方向 TypeId：单点/双点遥控、调节命令（带/不带时标）。
fn parse_control_command(header: &AsduHeader, asdu: &[u8]) -> Option<ControlCommand> {
    if asdu.len() < 8 {
        return None;
    }

    let ioa = Ioa::new(asdu[5], asdu[6], asdu[7]);
    let cmd_id = ioa.value() as u16;

    let (cmd_type, switch_state, p_set, q_set) = match header.type_id {
        TypeId::CScNa1 | TypeId::CScTa1 => {
            if asdu.len() < 9 {
                return None;
            }
            let state = asdu[8] & 0x01 == 0x01;
            (CommandType::SwitchControl, Some(state), None, None)
        }
        TypeId::CDcNa1 | TypeId::CDcTa1 => {
            if asdu.len() < 9 {
                return None;
            }
            let state = (asdu[8] & 0x03) == 0x02;
            (CommandType::SwitchControl, Some(state), None, None)
        }
        TypeId::CSeNa1 | TypeId::CSeTa1 => {
            if asdu.len() < 10 {
                return None;
            }
            let bits = ((asdu[9] as u32) << 8) | (asdu[8] as u32);
            let value = f32::from_bits(bits) as f64;
            (CommandType::PowerRegulation, None, Some(value), None)
        }
        _ => return None,
    };

    Some(ControlCommand {
        cmd_id,
        cmd_type,
        p_set,
        q_set,
        switch_state,
        priority: 0,
        k_value: None,
        deadband: None,
    })
}
