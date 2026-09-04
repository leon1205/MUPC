//! TcpTransport：经 TCP Socket 发送定长帧（迁移自 IntercoreClient 原逻辑，协议不变）
//!
//! 支持回读：`spawn_receive` 启动后台循环读实时模块上送帧（DataUpload/StatusReport），
//! 提取 `battery_soc` 供上层（N3 SOC 数据源，U-26 延伸）。

use crate::protocol::{FrameType as IntercoreFrameType, IntercoreFrame};
use crate::tcp_server::{DataUploadPayload, DualParamCommand};
use crate::transport::{v2_control_frame_bytes, v3_control_frame_bytes, IntercoreTransport};
use async_trait::async_trait;
use mupc_common::{ErrorCode, MupcError};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{timeout, Duration};

pub struct TcpTransport {
    remote_addr: String,
    timeout_ms: u64,
    connected: RwLock<bool>,
    stream: Arc<Mutex<Option<TcpStream>>>,
    /// 实时模块上送的最远 SOC（%，含上送时刻；N3）
    soc: RwLock<Option<(f64, Instant)>>,
}

impl TcpTransport {
    pub fn new(remote_addr: String) -> Self {
        Self {
            remote_addr,
            timeout_ms: 5000,
            connected: RwLock::new(false),
            stream: Arc::new(Mutex::new(None)),
            soc: RwLock::new(None),
        }
    }

    /// 启动回读接收循环（后台读 DataUpload 帧提取 SOC）。由装配方在 transport=tcp 时调用。
    pub fn spawn_receive(self: &Arc<Self>) {
        let s = self.clone();
        tokio::spawn(async move { s.receive_forever().await });
    }

    /// 后台循环：用**独立接收连接**读实时模块周期上送帧（DataUpload）。不共享发送连接
    /// 的 TcpStream（tokio TcpStream 不提供共享读写视图）；实时模块 Server 支持多连接，
    /// 向已连 client 上送状态帧。读超时/断开 → 重连。soc 断连后保留旧值——新鲜度由
    /// 上层按上送时刻（Instant）判定过期（>5s 弃用），故无需在此清除。
    async fn receive_forever(self: Arc<Self>) {
        let addr = self.remote_addr.clone();
        loop {
            match TcpStream::connect(&addr).await {
                Ok(mut stream) => {
                    let mut buf = [0u8; 64];
                    loop {
                        match timeout(Duration::from_secs(2), stream.read_exact(&mut buf)).await {
                            Ok(Ok(_)) => {
                                if let Ok(frame) = IntercoreFrame::from_bytes(&buf) {
                                    self.handle_frame(&frame).await;
                                }
                            }
                            _ => break, // 读失败/超时：断开重连
                        }
                    }
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        }
    }

    /// 处理实时模块上送帧：DataUpload → battery_soc
    async fn handle_frame(&self, frame: &IntercoreFrame) {
        if frame.header.frame_type != IntercoreFrameType::DataUpload {
            return;
        }
        match DataUploadPayload::from_json(&frame.data) {
            Ok(payload) => {
                if let Some(soc) = payload.battery_soc {
                    if soc.is_finite() {
                        *self.soc.write().await = Some((soc, Instant::now()));
                    }
                }
            }
            Err(e) => tracing::debug!("DataUpload 帧解析失败: {}", e),
        }
    }

    async fn send_bytes(&self, bytes: &[u8]) -> Result<(), MupcError> {
        let mut guard = self.stream.lock().await;
        if guard.is_none() {
            match TcpStream::connect(&self.remote_addr).await {
                Ok(s) => *guard = Some(s),
                Err(e) => return Err(MupcError::new(ErrorCode::ConnectionFailed, format!("connect {}: {}", self.remote_addr, e), "intercore")),
            }
        }
        let stream = guard.as_mut().ok_or_else(|| MupcError::new(ErrorCode::ConnectionFailed, "连接未建立", "intercore"))?;
        match timeout(Duration::from_millis(self.timeout_ms), stream.write_all(bytes)).await {
            Ok(Ok(())) => { *self.connected.write().await = true; Ok(()) }
            Ok(Err(e)) => { *guard = None; Err(MupcError::new(ErrorCode::SendFailed, format!("send: {}", e), "intercore")) }
            Err(_) => { *guard = None; Err(MupcError::new(ErrorCode::IntercoreTimeout, format!("timeout {}ms", self.timeout_ms), "intercore")) }
        }
    }
}

#[async_trait]
impl IntercoreTransport for TcpTransport {
    async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError> {
        let bytes = v2_control_frame_bytes(cmd)?;
        self.send_bytes(&bytes).await
    }

    async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], mode: &str) -> Result<(), MupcError> {
        let bytes = v3_control_frame_bytes(p, q, mode)?;
        self.send_bytes(&bytes).await
    }

    async fn is_connected(&self) -> bool { *self.connected.read().await }

    async fn shutdown(&self) -> Result<(), MupcError> {
        *self.stream.lock().await = None;
        *self.connected.write().await = false;
        *self.soc.write().await = None;
        Ok(())
    }

    async fn latest_soc(&self) -> Option<(f64, Instant)> {
        *self.soc.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_upload_soc_parsed_and_stored() {
        // DataUploadPayload 仅 from_json；用原始 JSON 字节构造帧 → handle_frame → soc store
        let json = br#"{"frame_version":1,"timestamp_ms":1700000000000,"q_realtime_margin":0.65,"battery_soc":75.5,"battery_power":10.0}"#;
        let frame = IntercoreFrame::new(IntercoreFrameType::DataUpload, 0, json.to_vec());
        let transport = TcpTransport::new("127.0.0.1:1".into());
        tokio_test::block_on(transport.handle_frame(&frame));
        let soc = tokio_test::block_on(transport.latest_soc());
        assert!(soc.is_some(), "DataUpload battery_soc 应被存入");
        assert!((soc.unwrap().0 - 75.5).abs() < 1e-9);
    }

    #[test]
    fn test_non_data_upload_ignored() {
        let frame = IntercoreFrame::new(IntercoreFrameType::HeartbeatReq, 0, vec![0]);
        let transport = TcpTransport::new("127.0.0.1:1".into());
        tokio_test::block_on(transport.handle_frame(&frame));
        assert!(tokio_test::block_on(transport.latest_soc()).is_none());
    }
}
