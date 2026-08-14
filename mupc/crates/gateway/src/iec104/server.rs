//! IEC 104 服务器

use mupc_common::{ErrorCode, MupcError};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tracing::{error, info, warn};

use super::command::CommandHandler;
use super::{Connection, ConnectionState, Iec104Frame};

/// IEC 104 服务器配置
#[derive(Debug, Clone)]
pub struct Iec104Config {
    /// 监听地址
    pub listen_addr: String,
    /// 监听端口
    pub listen_port: u16,
    /// 心跳间隔（秒）
    pub heartbeat_interval_secs: u64,
    /// 连接超时（毫秒）
    pub connection_timeout_ms: u64,
    /// 最大连接数
    pub max_connections: usize,
}

impl Default for Iec104Config {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0".to_string(),
            listen_port: 2404,
            heartbeat_interval_secs: 10,
            connection_timeout_ms: 30000,
            max_connections: 5,
        }
    }
}

/// IEC 104 服务器
pub struct Iec104Server {
    config: Iec104Config,
    connections: Arc<RwLock<Vec<Arc<RwLock<Connection>>>>>,
    shutdown_tx: broadcast::Sender<()>,
    telemetry_txs: Arc<Mutex<Vec<mpsc::Sender<Vec<u8>>>>>,
}

impl Iec104Server {
    /// 创建 IEC 104 服务器
    pub fn new(config: Iec104Config) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            config,
            connections: Arc::new(RwLock::new(Vec::new())),
            shutdown_tx,
            telemetry_txs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 启动服务器
    pub async fn start(&self, command_handler: Arc<dyn CommandHandler>) -> Result<(), MupcError> {
        let addr = format!("{}:{}", self.config.listen_addr, self.config.listen_port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            MupcError::new(
                ErrorCode::ConnectionFailed,
                format!("Failed to bind {}: {}", addr, e),
                "gateway",
            )
        })?;

        info!("IEC 104 server listening on {}", addr);

        let connections = self.connections.clone();
        let shutdown_rx = self.shutdown_tx.subscribe();
        let max_connections = self.config.max_connections;
        let timeout_ms = self.config.connection_timeout_ms;
        let telemetry_txs = self.telemetry_txs.clone();

        // 接受连接任务
        tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;

            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                let conn_count = {
                                    let conns = connections.read().await;
                                    conns.len()
                                };

                                if conn_count >= max_connections {
                                    warn!("Max connections reached, rejecting {}", addr);
                                    drop(stream);
                                    continue;
                                }

                                info!("New connection from {}", addr);
                                let conn = Arc::new(RwLock::new(Connection::new(stream, addr)));
                                connections.write().await.push(conn.clone());

                                // 处理连接
                                let (telemetry_tx, telemetry_rx) = mpsc::channel::<Vec<u8>>(100);
                                {
                                    let mut txs = telemetry_txs.lock().await;
                                    txs.push(telemetry_tx);
                                }
                                let handler = command_handler.clone();
                                let cleanup_connections = connections.clone();
                                let conn_for_cleanup = conn.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(
                                        conn,
                                        handler,
                                        timeout_ms,
                                        telemetry_rx,
                                    )
                                    .await
                                    {
                                        error!("Connection error: {}", e);
                                    }
                                    // 连接结束（正常或异常），从列表移除，避免连接泄漏
                                    cleanup_connections
                                        .write()
                                        .await
                                        .retain(|c| !Arc::ptr_eq(c, &conn_for_cleanup));
                                });
                            }
                            Err(e) => {
                                error!("Accept error: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("IEC 104 server shutting down");
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    /// 处理单个连接
    async fn handle_connection(
        conn: Arc<RwLock<Connection>>,
        handler: Arc<dyn CommandHandler>,
        timeout_ms: u64,
        mut telemetry_rx: mpsc::Receiver<Vec<u8>>,
    ) -> Result<(), MupcError> {
        let stream = conn.write().await.stream.take().ok_or_else(|| {
            MupcError::new(
                ErrorCode::Unknown,
                "Stream already taken from connection",
                "gateway",
            )
        })?;
        let (read_half, write_half) = tokio::io::split(stream);
        let write_half = Arc::new(Mutex::new(write_half));

        // 遥测发送任务：从 channel 取遥测字节，持续发送（北向上送）
        let telemetry_write = write_half.clone();
        let telemetry_handle = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            while let Some(data) = telemetry_rx.recv().await {
                let mut w = telemetry_write.lock().await;
                if let Err(e) = w.write_all(&data).await {
                    tracing::debug!("遥测发送失败: {}", e);
                    break;
                }
            }
        });

        // 读取循环
        let read_conn = conn.clone();
        let read_write = write_half.clone();
        let read_handle = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;

            let mut buf = [0u8; 1024];
            let mut reader = tokio::io::BufReader::new(read_half);
            let mut pending: Vec<u8> = Vec::new();

            'outer: loop {
                let read_result = tokio::time::timeout(
                    std::time::Duration::from_millis(timeout_ms),
                    reader.read(&mut buf),
                )
                .await;

                match read_result {
                    Ok(Ok(0)) => {
                        info!("Connection closed");
                        read_conn.write().await.state = ConnectionState::Disconnected;
                        break;
                    }
                    Ok(Ok(n)) => {
                        pending.extend_from_slice(&buf[..n]);

                        // 按 IEC104 帧长（length 字段 + 2）循环提取完整帧（处理半包/粘包）
                        loop {
                            if pending.len() < 2 {
                                break;
                            }
                            let frame_len = (pending[1] as usize) + 2;
                            if pending.len() < frame_len {
                                break;
                            }

                            let frame_bytes: Vec<u8> = pending.drain(..frame_len).collect();
                            let frame = match Iec104Frame::parse(&frame_bytes) {
                                Ok(f) => f,
                                Err(e) => {
                                    error!("Frame parse error: {}", e);
                                    continue;
                                }
                            };

                            let mut conn_guard = read_conn.write().await;
                            let mut w = read_write.lock().await;
                            if let Err(e) = conn_guard
                                .handle_frame(frame, &mut *w, handler.as_ref())
                                .await
                            {
                                error!("Frame handling error: {}", e);
                                break 'outer;
                            }
                            drop(w);

                            if conn_guard.state == ConnectionState::Disconnected {
                                break 'outer;
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        error!("Read error: {}", e);
                        break;
                    }
                    Err(_elapsed) => {
                        warn!(
                            "Connection {} idle timeout after {}ms",
                            read_conn.read().await.addr, timeout_ms
                        );
                        read_conn.write().await.state = ConnectionState::Disconnected;
                        break;
                    }
                }
            }
        });

        read_handle.await.map_err(|e| {
            MupcError::new(
                ErrorCode::Unknown,
                format!("Task join error: {}", e),
                "gateway",
            )
        })?;

        telemetry_handle.abort();

        // 清理连接
        Ok(())
    }

    /// 广播遥测字节到所有已连接的主站（北向遥测上送）
    ///
    /// FIXME: 遥测字节的编码（ASDU + I 帧）由调用方负责，本方法仅广播
    pub async fn broadcast_telemetry(&self, data: Vec<u8>) {
        let txs = self.telemetry_txs.lock().await;
        for tx in txs.iter() {
            let _ = tx.send(data.clone()).await;
        }
    }

    /// 停止服务器
    pub async fn shutdown(&self) -> Result<(), MupcError> {
        let _ = self.shutdown_tx.send(());
        let mut conns = self.connections.write().await;
        for conn in conns.iter() {
            conn.write().await.state = ConnectionState::Disconnected;
        }
        conns.clear();
        Ok(())
    }

    /// 获取连接数
    pub async fn connection_count(&self) -> usize {
        self.connections.read().await.len()
    }
}
