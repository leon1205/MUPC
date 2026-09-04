//! TcpTransport：经 TCP Socket 发送定长帧（迁移自 IntercoreClient 原逻辑，协议不变）
use crate::transport::{v2_control_frame_bytes, v3_control_frame_bytes, IntercoreTransport};
use async_trait::async_trait;
use mupc_common::{ErrorCode, MupcError};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{timeout, Duration};

pub struct TcpTransport {
    remote_addr: String,
    timeout_ms: u64,
    connected: RwLock<bool>,
    stream: Arc<Mutex<Option<TcpStream>>>,
}

impl TcpTransport {
    pub fn new(remote_addr: String) -> Self {
        Self { remote_addr, timeout_ms: 5000, connected: RwLock::new(false), stream: Arc::new(Mutex::new(None)) }
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
    async fn send_dual_param(&self, cmd: &crate::tcp_server::DualParamCommand) -> Result<(), MupcError> {
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
        Ok(())
    }
}
