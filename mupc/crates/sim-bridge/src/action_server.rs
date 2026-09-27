use crate::error::SimBridgeError;
use mupc_common::ErrorCode;
use mupc_intercore::{ActionPayload, IntercoreFrame, IntercoreFrameType, FRAME_FIXED_LENGTH};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};

/// 动作帧长度 —— **与 intercore 定长帧同源**（11 号设计 §3.3：复用 `IntercoreFrame`）。
///
/// 历史：本常量原为 26，对应一套 sim-bridge 私有的 26 字节帧（`[8..16) p_ref / [16..24)
/// k_droop / [24..26) CRC`）。MUPC 侧 `IntercoreClient` 实际发出的是 64 字节 intercore
/// 定长帧 ⇒ 双方格式互斥、动作链路**永远解析不了**（审查 E-01）。现改为复用 intercore
/// 的编解码（`IntercoreFrame` + `ActionPayload`），本常量直接引用 `FRAME_FIXED_LENGTH`。
pub const ACTION_FRAME_LEN: usize = FRAME_FIXED_LENGTH;
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

impl From<mupc_common::MupcError> for ReadError {
    /// CRC 失败单独归类（供上层 WARN 口径 `ReadError::CrcMismatch`），其余归协议错误。
    fn from(e: mupc_common::MupcError) -> Self {
        if e.code == ErrorCode::FrameChecksumError {
            ReadError::CrcMismatch
        } else {
            ReadError::Protocol(SimBridgeError::Protocol(e.to_string()))
        }
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

/// 解析一个 intercore 定长帧（64 B）为动作。
///
/// 帧布局（PRD 10 §2.2，`IntercoreFrame` 同源实现）：
///
/// | 偏移 | 字段 | 大小 | 说明 |
/// |------|------|:--:|------|
/// | 0..2 | magic | 2B | 0xAA55 |
/// | 2..4 | length | 2B | 帧总长度（含帧头+载荷+CRC，不含 padding）|
/// | 4..6 | frame_type | 2B | 0x0010 = ControlCmd |
/// | 6..8 | seq_no | 2B | 序列号 |
/// | 8..24 | payload | 16B | `ActionPayload`：p_ref f64 BE ‖ k_droop f64 BE |
/// | 24..26 | crc16 | 2B | CRC-16/MODBUS（大端），覆盖 magic..payload |
/// | 26..64 | padding | — | 0x00 |
///
/// 原实现把 p_ref/k_droop 直接读在偏移 8/16 并自算 CRC —— 数值布局**恰好**与
/// `ActionPayload` 相同（所以迁移后没有零点偏移），但帧头/长度/CRC 位置全不一致。
fn parse_frame(buf: &[u8; ACTION_FRAME_LEN]) -> Result<ActionFrame, ReadError> {
    let frame = IntercoreFrame::from_bytes(buf)?;
    if frame.header.frame_type != IntercoreFrameType::ControlCmd {
        return Err(ReadError::Protocol(SimBridgeError::Protocol(format!(
            "非 ControlCmd 帧: {:?}",
            frame.header.frame_type
        ))));
    }

    let payload = ActionPayload::from_frame(&frame)?;

    // Clamp to physical constraints
    let p_ref = payload.p_ref.clamp(-50.0, 50.0);
    let k_droop = payload.k_droop.clamp(0.0, 30.0);

    Ok(ActionFrame { p_ref, k_droop })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// E-01 判别力测试：**用 intercore 的真实编码器产帧 → sim-bridge 必须能解析**。
    ///
    /// 改坏方式（必须变红）：把 `ACTION_FRAME_LEN` 改回 26、或把 `parse_frame` 换回
    /// sim-bridge 私有的 26 字节 [`crc16_modbus`](自算) 版本 ⇒ 64 B 帧在偏移 24..26
    /// 读到的是 padding 而非 CRC ⇒ `CrcMismatch`。
    #[test]
    fn test_intercore_encoded_action_frame_is_parsed() {
        let encoded = ActionPayload::new(-12.5, 3.25).to_frame(7).unwrap();
        assert_eq!(
            encoded.len(),
            ACTION_FRAME_LEN,
            "intercore 编码器必须产出 sim-bridge 期望的 {} 字节帧",
            ACTION_FRAME_LEN
        );

        let buf: [u8; ACTION_FRAME_LEN] = encoded.as_slice().try_into().unwrap();
        let parsed = parse_frame(&buf).expect("intercore 产出的 ControlCmd 帧必须可解析");
        assert!((parsed.p_ref - (-12.5)).abs() < 1e-9, "p_ref={}", parsed.p_ref);
        assert!((parsed.k_droop - 3.25).abs() < 1e-9, "k_droop={}", parsed.k_droop);
    }

    /// 旧的 26 字节私有帧（补零到 64 B）**不再**被接受：其字节 0..2 是 frame_id 高 16 位
    /// (=0x0000) 而非 magic 0xAA55 ⇒ magic 校验失败。改回 26 B 口径本条必须变红。
    #[test]
    fn test_legacy_26_byte_frame_is_rejected() {
        let mut legacy = [0u8; ACTION_FRAME_LEN];
        legacy[0..4].copy_from_slice(&7u32.to_be_bytes()); // frame_id
        legacy[4] = 0x01; // cmd_type
        legacy[6..8].copy_from_slice(&16u16.to_be_bytes()); // payload_len
        legacy[8..16].copy_from_slice(&12.5f64.to_be_bytes());
        legacy[16..24].copy_from_slice(&(-3.25f64).to_be_bytes());
        legacy[24..26].copy_from_slice(&0x1234u16.to_be_bytes());

        assert!(
            matches!(parse_frame(&legacy), Err(ReadError::Protocol(_))),
            "旧 26 字节私有帧必须被拒绝（magic 不匹配）"
        );
    }

    /// CRC 被篡改 ⇒ 归类为 `CrcMismatch`（上层 WARN + 丢帧，不断连）。
    #[test]
    fn test_tampered_crc_is_reported_as_crc_mismatch() {
        let mut bytes = ActionPayload::new(1.0, 2.0).to_frame(1).unwrap();
        bytes[8] ^= 0xFF; // 篡改载荷首字节
        let buf: [u8; ACTION_FRAME_LEN] = bytes.as_slice().try_into().unwrap();
        assert!(matches!(parse_frame(&buf), Err(ReadError::CrcMismatch)));
    }

    /// 非 ControlCmd 帧（如 HeartbeatRsp）不得被当成动作。
    #[test]
    fn test_non_control_cmd_frame_is_rejected() {
        let bytes = IntercoreFrame::new_heartbeat_rsp().to_bytes().unwrap();
        let buf: [u8; ACTION_FRAME_LEN] = bytes.as_slice().try_into().unwrap();
        assert!(matches!(parse_frame(&buf), Err(ReadError::Protocol(_))));
    }

    /// 物理限幅保留（p_ref ∈ [-50, 50]、k_droop ∈ [0, 30]）。
    #[test]
    fn test_action_frame_clamps_to_physical_limits() {
        let bytes = ActionPayload::new(999.0, -50.0).to_frame(0).unwrap();
        let buf: [u8; ACTION_FRAME_LEN] = bytes.as_slice().try_into().unwrap();
        let parsed = parse_frame(&buf).unwrap();
        assert_eq!(parsed.p_ref, 50.0);
        assert_eq!(parsed.k_droop, 0.0);
    }
}
