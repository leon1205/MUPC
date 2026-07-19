use crate::error::SimBridgeError;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};

pub const ACTION_FRAME_LEN: usize = 26;
pub const ACTION_READ_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct ActionFrame {
    pub p_ref: f64,
    pub k_droop: f64,
}

pub struct ActionServer {
    listener: TcpListener,
}

#[derive(Debug)]
pub enum ReadError {
    TimeoutElapsed,
    ConnectionLost,
    CrcMismatch,
    Protocol(SimBridgeError),
}

impl From<SimBridgeError> for ReadError {
    fn from(e: SimBridgeError) -> Self {
        ReadError::Protocol(e)
    }
}

impl ActionServer {
    pub async fn bind(addr: &str) -> Result<Self, SimBridgeError> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!("ActionServer 监听 {}", addr);
        Ok(Self { listener })
    }

    pub async fn accept(&self) -> Result<(TcpStream, SocketAddr), SimBridgeError> {
        let (stream, addr) = self.listener.accept().await?;
        tracing::info!("MUPC 已连接: {}", addr);
        Ok((stream, addr))
    }
}

/// Read one action frame from an established TCP connection, with timeout.
pub async fn read_frame_with_timeout(
    stream: &mut TcpStream,
    timeout: Duration,
) -> Result<ActionFrame, ReadError> {
    let mut buf = [0u8; ACTION_FRAME_LEN];
    match tokio::time::timeout(timeout, stream.read_exact(&mut buf)).await {
        Ok(Ok(_n)) => {
            let frame = parse_frame(&buf)?;
            tracing::debug!(
                "动作: p_ref={:.2}, k_droop={:.4}",
                frame.p_ref,
                frame.k_droop
            );
            Ok(frame)
        }
        Ok(Err(e)) => {
            tracing::warn!("TCP 读取错误: {}", e);
            Err(ReadError::ConnectionLost)
        }
        Err(_) => Err(ReadError::TimeoutElapsed),
    }
}

fn parse_frame(buf: &[u8; ACTION_FRAME_LEN]) -> Result<ActionFrame, ReadError> {
    let p_ref = f64::from_be_bytes(buf[8..16].try_into().unwrap());
    let k_droop = f64::from_be_bytes(buf[16..24].try_into().unwrap());

    // CRC-16/MODBUS verification
    let crc_actual = u16::from_be_bytes([buf[24], buf[25]]);
    let crc_expected = crc16_modbus(&buf[..24]);
    if crc_actual != crc_expected {
        tracing::warn!(
            "CRC 校验失败: expected={:#06x}, actual={:#06x}",
            crc_expected,
            crc_actual
        );
        return Err(ReadError::CrcMismatch);
    }

    // Clamp to physical constraints
    let p_ref = p_ref.clamp(-50.0, 50.0);
    let k_droop = k_droop.clamp(0.0, 30.0);

    Ok(ActionFrame { p_ref, k_droop })
}

fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}
