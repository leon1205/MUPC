//! IEC 104 服务器

use mupc_common::{ErrorCode, MupcError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tracing::{error, info, warn};

use super::command::CommandHandler;
use super::{Connection, ConnectionState, Iec104Frame};

/// 链路状态（**对外聚合口径**，设计 §4.1 #1 —— 供本地显示终端 F6「IEC 104 连接状态」）。
///
/// 与 [`ConnectionState`] 的分工：后者是**单条连接**的内部状态机（含 `WaitingStartDt`
/// 这类 104 协议细节态），本枚举是**面向 HMI 的 4 态聚合**（与显示契约
/// `display-proto::LinkState` 的 `Connected/Connecting/Disconnected/NotConfigured`
/// 一一对应，映射在 `mupc-core-bin/src/display_host.rs`）。
///
/// ⚠️ 不设 `Unknown`：真源不可得的场景由**调用方**（未接线时）自行给出，本枚举只表达
/// 服务器自己**已知**的状态（PRD F6.5 的「未知」由显示侧兜底，不由本层臆造）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    /// 未配置 / 未启动（`start()` 从未成功绑定监听）。
    NotConfigured,
    /// 已启动，但当前无任何连接。
    Disconnected,
    /// 有连接，但均未进入 [`ConnectionState::Connected`]（连接中 / 等待 STARTDT / 已停止）。
    Connecting,
    /// 至少一条连接已 [`ConnectionState::Connected`]。
    Connected,
}

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
    /// `start()` 是否已成功绑定监听 —— [`Iec104Server::link_state`] 的
    /// 「未配置 / 未启动」判据（无此位则"从未启动"与"无连接"不可区分）。
    started: AtomicBool,
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
            started: AtomicBool::new(false),
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
        // 绑定成功即视为「已启动」（`link_state()` 的 NotConfigured 判据）——bind 失败时
        // 上面的 `?` 已提前返回，本行不执行 ⇒ 此时状态仍为「未启动」。
        self.started.store(true, Ordering::SeqCst);

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

    /// 链路状态聚合（设计 §4.1 #1 的对外口径；HMI F6「IEC 104 连接状态」的真源）。
    ///
    /// 判据（与设计逐字一致）：
    /// - 未启动（`start()` 未成功绑定） → [`LinkState::NotConfigured`]；
    /// - 已启动且无连接 → [`LinkState::Disconnected`]；
    /// - 有连接但均未 `Connected` → [`LinkState::Connecting`]；
    /// - 任一连 `Connected` → [`LinkState::Connected`]。
    ///
    /// ⚠️ 与 [`Self::connection_count`] 的分工：本方法是**状态**查询（HMI 3 s 慢拍），
    /// 计数只反映连接表长度；两者都不触碰协议逻辑。
    pub async fn link_state(&self) -> LinkState {
        if !self.started.load(Ordering::SeqCst) {
            return LinkState::NotConfigured;
        }
        let conns = self.connections.read().await;
        if conns.is_empty() {
            return LinkState::Disconnected;
        }
        for conn in conns.iter() {
            if conn.read().await.state == ConnectionState::Connected {
                return LinkState::Connected;
            }
        }
        LinkState::Connecting
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iec104::command::{CommandResponse, ControlCommand};

    struct StubHandler;

    #[async_trait::async_trait]
    impl CommandHandler for StubHandler {
        async fn handle_command(&self, cmd: ControlCommand) -> Result<CommandResponse, MupcError> {
            Ok(CommandResponse {
                cmd_id: cmd.cmd_id,
                success: false,
                message: "测试桩".to_string(),
                timestamp: 0,
            })
        }

        fn name(&self) -> &str {
            "stub"
        }
    }

    /// 回环 + 临时端口：测试不占固定端口（`listen_port = 0` 由内核分配）。
    fn cfg() -> Iec104Config {
        Iec104Config {
            listen_addr: "127.0.0.1".to_string(),
            listen_port: 0,
            ..Default::default()
        }
    }

    /// 向连接表注入一条**状态可控**的连接（同模块可见私有字段；用真实 TCP 对端避免
    /// `Connection` 内部裸状态被伪造）。
    async fn push_conn(server: &Iec104Server, state: ConnectionState) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let client = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (server_side, peer) = listener.accept().await.expect("accept");
        let mut conn = Connection::new(server_side, peer);
        conn.state = state;
        drop(client);
        server
            .connections
            .write()
            .await
            .push(Arc::new(RwLock::new(conn)));
    }

    /// 未 `start()` ⇒ `NotConfigured`（**即便连接表里已有连接**——"未启动"优先于一切）。
    #[tokio::test]
    async fn link_state_is_not_configured_before_start() {
        let server = Iec104Server::new(cfg());
        assert_eq!(server.link_state().await, LinkState::NotConfigured);
        push_conn(&server, ConnectionState::Connected).await;
        assert_eq!(
            server.link_state().await,
            LinkState::NotConfigured,
            "未启动优先于连接表内容"
        );
    }

    /// `start()` 成功 ⇒ 「已启动」；无连接 = `Disconnected`。
    #[tokio::test]
    async fn link_state_is_disconnected_after_start_without_connections() {
        let server = Iec104Server::new(cfg());
        server
            .start(Arc::new(StubHandler))
            .await
            .expect("bind 127.0.0.1:0");
        assert_eq!(server.link_state().await, LinkState::Disconnected);
    }

    /// 连接表聚合：有连接但均未 `Connected` → `Connecting`；任一连 `Connected` → `Connected`。
    #[tokio::test]
    async fn link_state_aggregates_connections() {
        let server = Iec104Server::new(cfg());
        server
            .start(Arc::new(StubHandler))
            .await
            .expect("bind 127.0.0.1:0");

        push_conn(&server, ConnectionState::Connecting).await;
        assert_eq!(server.link_state().await, LinkState::Connecting);

        push_conn(&server, ConnectionState::WaitingStartDt).await;
        assert_eq!(
            server.link_state().await,
            LinkState::Connecting,
            "WaitingStartDt 属「连接中」而非「已连接」"
        );

        push_conn(&server, ConnectionState::Connected).await;
        assert_eq!(
            server.link_state().await,
            LinkState::Connected,
            "任一连 Connected ⇒ 聚合为 Connected"
        );
    }
}
