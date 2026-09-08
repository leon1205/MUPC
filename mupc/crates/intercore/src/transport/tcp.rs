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
    /// 联锁锁存（内存 latch，供联锁流程/测试表达；TCP 无 PCS 500 语义，stop 降级 no-op，
    /// 仅 latch 语义被 IntercoreClient 上层消费——仿真下仍挡启动与表达联锁状态）
    stopped_latched: RwLock<bool>,
}

impl TcpTransport {
    pub fn new(remote_addr: String) -> Self {
        Self {
            remote_addr,
            timeout_ms: 5000,
            connected: RwLock::new(false),
            stream: Arc::new(Mutex::new(None)),
            soc: RwLock::new(None),
            stopped_latched: RwLock::new(false),
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

    /// TCP 通道无 PCS 500 启停寄存器语义：stop 降级 no-op（仅记录，供仿真联锁流程表达）。
    /// 不设/清 stopped_latched（C-1 与 Modbus 一致：latch 只由 restore 管理）。
    /// ⚠️ M-4：下行 latch gate（send/stop 的 500 语义）仅 Modbus 实现；TCP 仅表达 latch 状态、
    /// send 是否抑制由上层（IntercoreClient/联锁流程）决定——本通道 send_* 不查 latch。
    async fn stop(&self) -> Result<(), String> {
        tracing::warn!("tcp 通道无 PCS 500 语义，stop 降级 no-op（仿真）");
        Ok(())
    }

    async fn is_interlock_stopped(&self) -> bool {
        *self.stopped_latched.read().await
    }

    async fn restore_interlock_latched(&self, latched: bool) -> Result<(), String> {
        // 内存 latch（联锁流程/测试可表达；纯状态不涉及网络 IO）
        *self.stopped_latched.write().await = latched;
        Ok(())
    }

    fn last_run_state(&self) -> Option<u16> {
        // TCP 通道无 REG_RUN_STATE(1013)；None（链路未知 → DO1 灭保守）
        None
    }

    async fn authorize_restart(&self) -> Result<(), String> {
        // TCP 无 started 缓存/S-4 概念：仅 !latch 才放行（联锁语义一致）。Modbus 侧 restart_
        // authorized 单次旁路（S-4 停机守卫）为本通道专属语义，TCP 不表达。
        if *self.stopped_latched.read().await {
            return Err("interlock stopped：联锁锁存中，须先 release 才能重启".to_string());
        }
        Ok(())
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

    #[tokio::test]
    async fn test_latch_toggle_gates_authorize() {
        // M-5：TCP 通道 latch 为内存表达（联锁流程/仿真语义；send 由上层抑制，M-4）。
        // restore 置/清驱动 is_interlock_stopped；latch 期间 authorize_restart 拒绝、清后放行。
        let tr = TcpTransport::new("127.0.0.1:1".into());
        assert!(!tr.is_interlock_stopped().await);
        assert!(tr.authorize_restart().await.is_ok(), "!latch 时 authorize 应放行");

        tr.restore_interlock_latched(true).await.unwrap();
        assert!(tr.is_interlock_stopped().await, "restore(true) 应置 latch");
        assert!(tr.authorize_restart().await.is_err(), "latch 期间 authorize 应拒绝");

        tr.restore_interlock_latched(false).await.unwrap();
        assert!(!tr.is_interlock_stopped().await, "restore(false) 应清 latch");
        assert!(tr.authorize_restart().await.is_ok(), "清 latch 后 authorize 应放行");
    }
}
