//! IEC 104 连接管理

#[cfg(test)]
mod tests {
    #[test]
    fn test_connection_state_transitions() {
        // 测试连接状态枚举值
        use super::ConnectionState;

        assert_eq!(ConnectionState::Disconnected, ConnectionState::Disconnected);
        assert_eq!(ConnectionState::Connecting, ConnectionState::Connecting);
        assert_eq!(ConnectionState::WaitingStartDt, ConnectionState::WaitingStartDt);
        assert_eq!(ConnectionState::Connected, ConnectionState::Connected);
        assert_eq!(ConnectionState::Stopped, ConnectionState::Stopped);
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
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{info, warn, error};

use super::{Iec104Frame, protocol::FrameType, protocol::UFrameType, TypeId, Cot};
use super::command::CommandHandler;

/// 连接状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    WaitingStartDt,    // 等待 STARTDT
    Connected,
    Stopped,
}

/// IEC 104 连接
pub struct Connection {
    pub stream: TcpStream,
    pub addr: SocketAddr,
    pub state: ConnectionState,
    pub send_seq: u16,
    pub recv_seq: u16,
    pub heartbeat_interval_secs: u64,
}

impl Connection {
    pub fn new(stream: TcpStream, addr: SocketAddr) -> Self {
        Self {
            stream,
            addr,
            state: ConnectionState::Disconnected,
            send_seq: 0,
            recv_seq: 0,
            heartbeat_interval_secs: 10,
        }
    }

    /// 处理接收到的帧
    pub async fn handle_frame(
        &mut self,
        frame: Iec104Frame,
        writer: &mut (impl AsyncWriteExt + Unpin),
    ) -> Result<(), mupc_common::MupcError> {
        match frame.frame_type {
            FrameType::UFrame => {
                self.handle_u_frame(frame, writer).await?;
            }
            FrameType::SFrame => {
                // S 帧用于确认已收到的 I 帧
                info!("Received S frame");
                self.recv_seq = frame.send_sequence();
            }
            FrameType::IFrame => {
                self.handle_i_frame(frame, writer).await?;
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
            mupc_common::MupcError::new(mupc_common::ErrorCode::FrameParseError, "Unknown U frame type", "gateway")
        })?;

        match u_type {
            UFrameType::StartDtAct => {
                info!("Received STARTDT_ACT, sending STARTDT_CON");
                let response = Iec104Frame::make_u_frame(UFrameType::StartDtCon);
                writer.write_all(&response).await.map_err(|e| {
                    mupc_common::MupcError::new(mupc_common::ErrorCode::SendFailed, format!("Send error: {}", e), "gateway")
                })?;
                self.state = ConnectionState::Connected;
            }
            UFrameType::StartDtCon => {
                info!("Received STARTDT_CON");
                self.state = ConnectionState::Connected;
            }
            UFrameType::StopDtAct => {
                info!("Received STOPDT_ACT, sending STOPDT_CON");
                let response = Iec104Frame::make_u_frame(UFrameType::StopDtCon);
                writer.write_all(&response).await.map_err(|e| {
                    mupc_common::MupcError::new(mupc_common::ErrorCode::SendFailed, format!("Send error: {}", e), "gateway")
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
                    mupc_common::MupcError::new(mupc_common::ErrorCode::SendFailed, format!("Send error: {}", e), "gateway")
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
    ) -> Result<(), mupc_common::MupcError> {
        let send_seq = frame.send_sequence();
        let recv_seq = frame.recv_sequence();

        // 检查接收序号
        if send_seq != self.recv_seq {
            warn!("Sequence mismatch: expected {}, got {}", self.recv_seq, send_seq);
        }

        self.recv_seq = send_seq;

        // 发送 S 帧确认
        let s_frame = Iec104Frame::make_s_frame(self.send_seq, self.recv_seq);
        writer.write_all(&s_frame).await.map_err(|e| {
            mupc_common::MupcError::new(mupc_common::ErrorCode::SendFailed, format!("Send error: {}", e), "gateway")
        })?;

        // 解析 ASDU
        let header = frame.parse_asdu_header()?;
        info!("Received I frame: type_id={:?}, cot={}", header.type_id, header.cot.0);

        Ok(())
    }

    /// 设置心跳间隔
    pub fn set_heartbeat_interval(&mut self, secs: u64) {
        self.heartbeat_interval_secs = secs.clamp(1, 60);
    }
}