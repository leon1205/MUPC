//! 核间通信 TCP 服务器

use mupc_common::{MupcError, ErrorCode};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn, error};

use super::{IntercoreFrame, IntercoreFrameType, HeartbeatManager, Watchdog, protocol::FrameHeader};

/// 核间通信配置
#[derive(Debug, Clone)]
pub struct IntercoreConfig {
    /// 监听地址
    pub listen_addr: String,
    /// 监听端口
    pub listen_port: u16,
    /// 心跳间隔（毫秒）
    pub heartbeat_interval_ms: u64,
    /// 看门狗超时（毫秒）
    pub watchdog_timeout_ms: u64,
}

impl Default for IntercoreConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0".to_string(),
            listen_port: 2500,
            heartbeat_interval_ms: 1000,
            watchdog_timeout_ms: 10000,
        }
    }
}

/// 核间通信服务器
pub struct IntercoreServer {
    config: IntercoreConfig,
    shutdown_tx: broadcast::Sender<()>,
}

impl IntercoreServer {
    /// 创建核间通信服务器
    pub fn new(config: IntercoreConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            config,
            shutdown_tx,
        }
    }

    /// 启动服务器
    pub async fn start(&self) -> Result<Arc<RwLock<HeartbeatManager>>, MupcError> {
        let addr = format!("{}:{}", self.config.listen_addr, self.config.listen_port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| MupcError::new(ErrorCode::ConnectionFailed, format!("Failed to bind {}: {}", addr, e), "intercore"))?;

        info!("Intercore server listening on {}", addr);

        let heartbeat_manager = Arc::new(RwLock::new(HeartbeatManager::new(
            self.config.heartbeat_interval_ms,
            self.config.watchdog_timeout_ms,
        )));

        let shutdown_rx = self.shutdown_tx.subscribe();

        // 接受连接任务
        let listener_handle = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;

            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                info!("New intercore connection from {}", addr);
                                let heartbeat = heartbeat_manager.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(stream, addr, heartbeat).await {
                                        error!("Connection error from {}: {}", addr, e);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Accept error: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Intercore server shutting down");
                        break;
                    }
                }
            }
        });

        // 启动心跳管理器
        let heartbeat = heartbeat_manager.clone();
        tokio::spawn(async move {
            heartbeat.read().await.run().await;
        });

        Ok(heartbeat_manager)
    }

    /// 处理连接
    async fn handle_connection(
        stream: TcpStream,
        addr: SocketAddr,
        heartbeat: Arc<RwLock<HeartbeatManager>>,
    ) -> Result<(), MupcError> {
        let (mut read_half, mut write_half) = tokio::io::split(stream);

        // 发送连接注册
        let connect_frame = IntercoreFrame::new_connect();
        let frame_data = connect_frame.to_bytes()?;
        write_half.write_all(&frame_data).await.map_err(|e| {
            MupcError::new(ErrorCode::SendFailed, format!("Send error: {}", e), "intercore")
        })?;

        heartbeat.read().await.register_connection(addr);

        // 读取循环
        let mut buf = [0u8; 64];
        let mut reader = tokio::io::BufReader::new(read_half);

        loop {
            match reader.read(&mut buf).await {
                Ok(0) => {
                    info!("Connection closed: {}", addr);
                    heartbeat.read().await.unregister_connection(addr);
                    break;
                }
                Ok(n) => {
                    match IntercoreFrame::from_bytes(&buf[..n]) {
                        Ok(frame) => {
                            match frame.frame_type {
                                IntercoreFrameType::HeartbeatReq | IntercoreFrameType::HeartbeatRsp => {
                                    heartbeat.read().await.receive_heartbeat(addr);
                                }
                                IntercoreFrameType::ControlCmd => {
                                    info!("Received control command from {}", addr);
                                }
                                IntercoreFrameType::ControlRsp => {
                                    info!("Received control response from {}", addr);
                                }
                                IntercoreFrameType::StatusReport => {
                                    info!("Received status report from {}", addr);
                                }
                                IntercoreFrameType::DataUpload => {
                                    info!("Received data upload from {}", addr);
                                }
                                IntercoreFrameType::Connect => {
                                    info!("Received connect from {}", addr);
                                }
                                IntercoreFrameType::Unknown => {
                                    warn!("Unknown frame type from {}", addr);
                                }
                            }

                            // 回复心跳响应
                            if frame.frame_type == IntercoreFrameType::HeartbeatReq {
                                let rsp = IntercoreFrame::new_heartbeat_rsp();
                                let rsp_data = rsp.to_bytes()?;
                                write_half.write_all(&rsp_data).await.map_err(|e| {
                                    MupcError::new(ErrorCode::SendFailed, format!("Send error: {}", e), "intercore")
                                })?;
                            }
                        }
                        Err(e) => {
                            error!("Frame parse error from {}: {}", addr, e);
                        }
                    }
                }
                Err(e) => {
                    error!("Read error from {}: {}", addr, e);
                    heartbeat.read().await.unregister_connection(addr);
                    break;
                }
            }
        }

        Ok(())
    }

    /// 停止服务器
    pub async fn shutdown(&self) -> Result<(), MupcError> {
        let _ = self.shutdown_tx.send(());
        Ok(())
    }
}