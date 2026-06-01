//! IEC 104 服务器

use mupc_common::{MupcError, ErrorCode};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn, error};

use super::{Connection, ConnectionState, Iec104Frame};
use super::command::CommandHandler;

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
}

impl Iec104Server {
    /// 创建 IEC 104 服务器
    pub fn new(config: Iec104Config) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            config,
            connections: Arc::new(RwLock::new(Vec::new())),
            shutdown_tx,
        }
    }

    /// 启动服务器
    pub async fn start(&self, command_handler: Arc<dyn CommandHandler>) -> Result<(), MupcError> {
        let addr = format!("{}:{}", self.config.listen_addr, self.config.listen_port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| MupcError::new(ErrorCode::ConnectionFailed, format!("Failed to bind {}: {}", addr, e), "gateway"))?;

        info!("IEC 104 server listening on {}", addr);

        let connections = self.connections.clone();
        let shutdown_rx = self.shutdown_tx.subscribe();
        let max_connections = self.config.max_connections;

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
                                let conn_for_task = conn;
                                let _handler = command_handler.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(conn_for_task, _handler).await {
                                        error!("Connection error: {}", e);
                                    }
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
        _handler: Arc<dyn CommandHandler>,
    ) -> Result<(), MupcError> {
        let stream = conn.write().await.stream.take()
            .ok_or_else(|| MupcError::new(ErrorCode::Unknown, "Stream already taken from connection", "gateway"))?;
        let (read_half, mut write_half) = tokio::io::split(stream);

        // 读取循环
        let read_conn = conn.clone();
        let read_handle = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;

            let mut buf = [0u8; 1024];
            let mut reader = tokio::io::BufReader::new(read_half);

            loop {
                match reader.read(&mut buf).await {
                    Ok(0) => {
                        info!("Connection closed");
                        read_conn.write().await.state = ConnectionState::Disconnected;
                        break;
                    }
                    Ok(n) => {
                        let frame = match Iec104Frame::parse(&buf[..n]) {
                            Ok(f) => f,
                            Err(e) => {
                                error!("Frame parse error: {}", e);
                                continue;
                            }
                        };

                        let mut conn_guard = read_conn.write().await;
                        if let Err(e) = conn_guard.handle_frame(frame, &mut write_half).await {
                            error!("Frame handling error: {}", e);
                            break;
                        }

                        if conn_guard.state == ConnectionState::Disconnected {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Read error: {}", e);
                        break;
                    }
                }
            }
        });

        read_handle.await.map_err(|e| MupcError::new(ErrorCode::Unknown, format!("Task join error: {}", e), "gateway"))?;

        // 清理连接
        Ok(())
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