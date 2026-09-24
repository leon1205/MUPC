//! IEC 104 服务器

use mupc_common::{ErrorCode, MupcError};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tracing::{error, info, warn};

use super::command::CommandHandler;
use super::protocol::COT_INTROGEN;
use super::{Connection, ConnectionState, Iec104Frame};

/// 每连接遥测队列容量（§9.2.2「队列容量」）。
///
/// 依据：**最坏一阵同刻到齐** = 总召/初始快照的 **234** 帧（234 × 25 B ≈ 5.7 KB）
/// + 5 连接的并发写 ⇒ 2048 帧 ≈ **8 轮**余量（2048 ÷ 234 = 8.75）。
///
/// **容量只在此一处定义**（既有值 100 在 234 点突发的首轮即被背压打满）。
const TELEMETRY_CHANNEL_CAPACITY: usize = 2048;

/// 上送档位（**协议层类型**，§9.2.2）。
///
/// 决定 [`Iec104Server::publish_asdus`] 的**背压策略**：
/// - [`DataClass::A`] / [`DataClass::B`]（周期遥测）：`try_send`，通道满 ⇒ **丢弃该批并计数**
///   （下一周期自然重发，lossy OK）；
/// - [`DataClass::C`]（变位/状态）：`send().await`（**阻塞等待，绝不静默丢遥信变位**，PRD EX-2）。
///
/// ⚠️ 与 `mupc-southd::uplink::DataClass` **同名不同物**：01 设计 §9.0 明确禁止新增
/// `gateway → mupc-southd` 依赖边，故本枚举是 gateway 侧的协议层镜像；
/// 装配层（core-bin，T13）负责逐值映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataClass {
    A,
    B,
    C,
}

/// [`Iec104Server::publish_asdus`] 的投递结果（§9.2.2 / §9.2.6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishOutcome {
    /// 已**整批**投递给 `subscribers` 个连接（A/B：`try_send` 全部成功；C：`send().await` 完成）。
    Delivered { subscribers: usize },
    /// A/B 档背压：该连接通道满 ⇒ 该批被丢弃（`dropped` = 被丢弃的 ASDU 条数）。
    Dropped { subscribers: usize, dropped: usize },
    /// **无任何已连接主站** ⇒ 不缓存、不入队，直接返回本值并计数（§9.2.6；
    /// 变位由连接后的初始快照补齐，口径同 PRD EX-2）。
    NoSubscriber,
}

/// **每连接的出向序号**（N(S)，15 位），与写半锁同生命周期（§9.2.3）。
///
/// ⚠️ **契约：必须在持有写半锁时调用 [`OutboundSeq::next`]**——否则"取号"与"写序"可能被
/// 其它写方交错，主站将看到乱序的 N(S)。本模块内两个调用点（写任务、总召数据帧）都在
/// `Mutex<WriteHalf>` 的临界区内取号。
pub struct OutboundSeq(AtomicU16);

impl OutboundSeq {
    pub fn new() -> Self {
        Self(AtomicU16::new(0))
    }

    /// 取下一个出向序号并自增（15 位回绕：`& 0x7FFF`）。
    ///
    /// **调用方必须持有写半锁**（见类型文档）。
    pub fn next(&self) -> u16 {
        self.0.fetch_add(1, Ordering::SeqCst) & 0x7FFF
    }

    /// 当前值（只读，供测试断言；不消耗序号）。
    pub fn current(&self) -> u16 {
        self.0.load(Ordering::SeqCst) & 0x7FFF
    }
}

impl Default for OutboundSeq {
    fn default() -> Self {
        Self::new()
    }
}

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
    /// A/B 档背压丢弃的 ASDU 条数（§9.2.2 的 `iec104_dropped_total` 计数）。
    dropped_total: AtomicU64,
    /// 无连接时的投递次数（§9.2.6「无连接 ⇒ 返回 NoSubscriber **并计数**」）。
    no_subscriber_total: AtomicU64,
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
            dropped_total: AtomicU64::new(0),
            no_subscriber_total: AtomicU64::new(0),
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

                                // 处理连接（载荷 = **ASDU，不含 I 帧头**；序号由连接层赋予）
                                let (telemetry_tx, telemetry_rx) =
                                    mpsc::channel::<Vec<u8>>(TELEMETRY_CHANNEL_CAPACITY);
                                {
                                    let mut txs = telemetry_txs.lock().await;
                                    // 广播用副本（连接结束由 `publish_asdus` 的 is_closed 剪除）
                                    txs.push(telemetry_tx.clone());
                                }
                                let handler = command_handler.clone();
                                let cleanup_connections = connections.clone();
                                let conn_for_cleanup = conn.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = Self::handle_connection(
                                        conn,
                                        handler,
                                        timeout_ms,
                                        telemetry_tx,
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
        snapshot_tx: mpsc::Sender<Vec<u8>>,
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

        // 每连接一份出向序号（与写半锁同生命周期）与共享接收序号（§9.2.3）
        let outbound_seq = Arc::new(OutboundSeq::new());
        let shared_recv_seq = Arc::new(AtomicU16::new(0));

        // 遥测发送任务：从 channel 取 **ASDU**，在同一把写半锁内取序号 → 组 I 帧 → 写（§9.2.3）
        let telemetry_write = write_half.clone();
        let write_seq = outbound_seq.clone();
        let write_recv = shared_recv_seq.clone();
        let telemetry_handle = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            while let Some(asdu) = telemetry_rx.recv().await {
                let mut w = telemetry_write.lock().await;
                // ⚠️ 取号与写必须在**同一临界区**内（OutboundSeq 的契约）
                let seq = write_seq.next();
                let frame =
                    Iec104Frame::make_i_frame(seq, write_recv.load(Ordering::SeqCst), &asdu);
                if let Err(e) = w.write_all(&frame).await {
                    tracing::debug!("遥测发送失败: {}", e);
                    break;
                }
            }
        });

        // 读取循环
        let read_conn = conn.clone();
        let read_write = write_half.clone();
        let read_seq = outbound_seq.clone();
        let read_recv = shared_recv_seq.clone();
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
                                .handle_frame(frame, &mut *w, handler.as_ref(), &read_seq)
                                .await
                            {
                                error!("Frame handling error: {}", e);
                                break 'outer;
                            }
                            drop(w);

                            // 读循环维护共享的接收序号（写任务用它填 I 帧的 N(R)）
                            read_recv.store(conn_guard.recv_seq, Ordering::SeqCst);

                            // 连接初始快照（§9.2.5 GI-5）：STARTDT_CON 已在 handle_u_frame 内写出，
                            // 此处**先释放连接锁再取数**，随后入本连接的遥测通道（复用 C 档不丢语义），
                            // 由写任务按本连接序号发出。
                            let just_connected = conn_guard.take_just_connected();
                            let state = conn_guard.state;
                            drop(conn_guard);

                            if just_connected {
                                for mut item in handler.on_connection_snapshot().await {
                                    // 快照恒以 cot=20（响应站召唤）发出（§9.2.5 GI-5）
                                    item.cot = COT_INTROGEN;
                                    if snapshot_tx.send(item.encode_asdu()).await.is_err() {
                                        break;
                                    }
                                }
                            }

                            if state == ConnectionState::Disconnected {
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

    /// 批量投递 ASDU（**不含 I 帧头**）到所有已连接主站（北向遥测上送）。
    ///
    /// 连接层（写任务）负责：取**本连接**出向序号 → 组 I 帧 → 写。调用方**不得**自带序号
    /// （§9.2.3 缺陷 #2 的根因即"调用方恒塞 `seq = 0`"）。
    ///
    /// `class` 决定背压策略（§9.2.2）：A/B 档 `try_send`、通道满即**丢弃该批并计数**；
    /// C 档 `send().await`（阻塞等待，**绝不静默丢遥信变位**）。返回本次投递结果
    /// （[`PublishOutcome`]）；**无连接时不计入丢弃**，直接返回 `NoSubscriber`（§9.2.6）。
    /// ⚠️ **空批次不短路**：空批同样走一次订阅者判定，使三态语义对空批成立
    /// （无订阅者 ⇒ `NoSubscriber` 并计数；有订阅者 ⇒ `Delivered { n }`，`n` = 连接数）。
    /// 若让空批直接返回 `Delivered { subscribers: 0 }`，调用方误发空批就会**掩盖"无主站"**。
    pub async fn publish_asdus(&self, asdus: Vec<Vec<u8>>, class: DataClass) -> PublishOutcome {
        // C 档会 `await`（可能长时间阻塞）⇒ 先在锁内快照发送句柄、出锁后再 await
        // （否则一个慢主站会挡住其它发布方）。同时剪除已关闭的连接句柄，防序号表随重连无界增长。
        let senders: Vec<mpsc::Sender<Vec<u8>>> = {
            let mut txs = self.telemetry_txs.lock().await;
            txs.retain(|tx| !tx.is_closed());
            txs.clone()
        };

        if senders.is_empty() {
            self.no_subscriber_total.fetch_add(1, Ordering::Relaxed);
            return PublishOutcome::NoSubscriber;
        }

        match class {
            DataClass::A | DataClass::B => {
                let mut subscribers = 0usize;
                let mut dropped = 0usize;
                for tx in senders.iter() {
                    if tx.capacity() < asdus.len() {
                        // 通道满 ⇒ **整批丢弃**（不半推），下一周期自然重发
                        dropped += asdus.len();
                        continue;
                    }
                    let mut all_ok = true;
                    for asdu in asdus.iter() {
                        if tx.try_send(asdu.clone()).is_err() {
                            all_ok = false;
                            dropped += 1;
                        }
                    }
                    if all_ok {
                        subscribers += 1;
                    }
                }
                if dropped > 0 {
                    let total = self
                        .dropped_total
                        .fetch_add(dropped as u64, Ordering::Relaxed)
                        + dropped as u64;
                    // 限频 WARN：只报首条与每 100 条
                    if total == dropped as u64 || total % 100 == 0 {
                        warn!(
                            "IEC 104 A/B 档背压丢弃 {} 条 ASDU（累计 {}）——慢主站或队列已满",
                            dropped, total
                        );
                    }
                    PublishOutcome::Dropped {
                        subscribers,
                        dropped,
                    }
                } else {
                    PublishOutcome::Delivered { subscribers }
                }
            }
            DataClass::C => {
                let mut subscribers = 0usize;
                for tx in senders.iter() {
                    let mut all_ok = true;
                    for asdu in asdus.iter() {
                        if tx.send(asdu.clone()).await.is_err() {
                            all_ok = false;
                            break;
                        }
                    }
                    if all_ok {
                        subscribers += 1;
                    }
                }
                PublishOutcome::Delivered { subscribers }
            }
        }
    }

    /// A/B 档背压累计丢弃的 ASDU 条数（§9.2.2 的 `iec104_dropped_total`）。
    pub fn dropped_total(&self) -> u64 {
        self.dropped_total.load(Ordering::Relaxed)
    }

    /// 无连接时的投递次数（§9.2.6）。
    pub fn no_subscriber_total(&self) -> u64 {
        self.no_subscriber_total.load(Ordering::Relaxed)
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
    // `FrameType` 只被测试用到（删掉 T12 过渡壳后，生产路径不再解析入向帧）
    use crate::iec104::protocol::FrameType;
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

    // ========== 出向序号 / 投递 / 背压（§9.2.2 / §9.2.3） ==========

    use crate::iec104::protocol::{encode_me_tf1, COT_CYCLIC};

    /// `OutboundSeq`：跨 128 边界单调，32767 后回绕 0（15 位）。
    #[test]
    fn outbound_seq_is_monotonic_and_wraps_at_32767() {
        let seq = OutboundSeq::new();
        assert_eq!(seq.next(), 0);
        assert_eq!(seq.next(), 1);
        for _ in 0..126 {
            seq.next();
        }
        assert_eq!(
            seq.next(),
            128,
            "128 处**不得**回绕（§9.2.3 缺陷 #1 的断裂点）"
        );

        let s = OutboundSeq::new();
        for _ in 0..32767 {
            s.next();
        } // 已发出 0..=32766
        assert_eq!(s.next(), 32767, "满量程值本身必须能发出");
        assert_eq!(s.next(), 0, "15 位满量程之后回绕 0");
        assert_eq!(s.next(), 1);
    }

    /// 无连接 ⇒ `NoSubscriber` 并计数（§9.2.6，**不缓存不入队**）。
    #[tokio::test]
    async fn publish_asdus_returns_no_subscriber_without_connections() {
        let server = Iec104Server::new(cfg());
        let outcome = server.publish_asdus(vec![vec![0x0D]], DataClass::A).await;
        assert_eq!(outcome, PublishOutcome::NoSubscriber);
        assert_eq!(server.no_subscriber_total(), 1);
        assert_eq!(server.dropped_total(), 0);
    }

    /// **空批次不短路**：无订阅者时仍须判 `NoSubscriber` 并计数（三态语义对空批同样成立）。
    ///
    /// 旧实现空批直接返回 `Delivered { subscribers: 0 }` ⇒ 调用方误发空批会**掩盖"无主站"**。
    #[tokio::test]
    async fn publish_asdus_empty_batch_reports_no_subscriber_without_connections() {
        let server = Iec104Server::new(cfg());
        let outcome = server
            .publish_asdus(Vec::<Vec<u8>>::new(), DataClass::A)
            .await;
        assert_eq!(outcome, PublishOutcome::NoSubscriber);
        assert_eq!(server.no_subscriber_total(), 1);
        assert_eq!(server.dropped_total(), 0);
    }

    /// 空批次 + 有订阅者 ⇒ `Delivered { subscribers: 1 }`（判定走完、**不**走丢弃路径）。
    #[tokio::test]
    async fn publish_asdus_empty_batch_delivers_to_subscriber() {
        let server = Iec104Server::new(cfg());
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(2);
        server.telemetry_txs.lock().await.push(tx);

        let outcome = server
            .publish_asdus(Vec::<Vec<u8>>::new(), DataClass::C)
            .await;
        assert_eq!(outcome, PublishOutcome::Delivered { subscribers: 1 });
        assert_eq!(server.no_subscriber_total(), 0);
        assert_eq!(server.dropped_total(), 0);
        assert!(rx.try_recv().is_err(), "空批不得向通道写入任何载荷");
    }

    /// A/B 档：通道满 ⇒ **整批丢弃**并计数（§9.2.2 的 `iec104_dropped_total`）。
    #[tokio::test]
    async fn publish_asdus_ab_class_drops_when_channel_full_and_counts() {
        let server = Iec104Server::new(cfg());
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(2);
        server.telemetry_txs.lock().await.push(tx);

        // 通道容量 2 < 批次 3 ⇒ 整批丢弃
        let outcome = server
            .publish_asdus(vec![vec![1], vec![2], vec![3]], DataClass::A)
            .await;
        assert_eq!(
            outcome,
            PublishOutcome::Dropped {
                subscribers: 0,
                dropped: 3
            }
        );
        assert_eq!(server.dropped_total(), 3);
        assert!(rx.try_recv().is_err(), "整批丢弃：通道内不得留下半批");

        // 批次 1 放得下 ⇒ 投递成功且不计数
        let outcome = server.publish_asdus(vec![vec![7]], DataClass::B).await;
        assert_eq!(outcome, PublishOutcome::Delivered { subscribers: 1 });
        assert_eq!(rx.recv().await.unwrap(), vec![7]);
        assert_eq!(server.dropped_total(), 3);

        // 通道再次打满 ⇒ 单条亦被丢弃
        server
            .publish_asdus(vec![vec![8], vec![9]], DataClass::A)
            .await;
        let outcome = server.publish_asdus(vec![vec![10]], DataClass::A).await;
        assert_eq!(
            outcome,
            PublishOutcome::Dropped {
                subscribers: 0,
                dropped: 1
            }
        );
        assert_eq!(server.dropped_total(), 4);
    }

    /// C 档：容量不足时**阻塞等待**（`send().await`），**绝不静默丢遥信变位**（PRD EX-2）。
    #[tokio::test]
    async fn publish_asdus_c_class_blocks_and_never_drops() {
        let server = Arc::new(Iec104Server::new(cfg()));
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1);
        server.telemetry_txs.lock().await.push(tx);

        let producer = {
            let server = server.clone();
            tokio::spawn(async move {
                server
                    .publish_asdus(vec![vec![1], vec![2], vec![3]], DataClass::C)
                    .await
            })
        };

        let mut got = Vec::new();
        for _ in 0..3 {
            got.push(rx.recv().await.expect("C 档不得丢帧"));
        }
        assert_eq!(got, vec![vec![1], vec![2], vec![3]]);
        assert_eq!(
            producer.await.expect("join"),
            PublishOutcome::Delivered { subscribers: 1 }
        );
        assert_eq!(server.dropped_total(), 0, "C 档不计入丢弃");
    }

    /// **本增量硬前提的端到端回归**：234 点一轮突发（§9.2.2）的 I 帧序号**逐帧单调、
    /// 不在 128 处回绕**——旧实现（`i2` 恒 0）在第 129 帧起即回绕，主站必判序号错误。
    #[tokio::test]
    async fn write_task_assigns_monotonic_15bit_seq_for_234_frame_burst() {
        use tokio::io::AsyncReadExt;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let mut client = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (server_side, peer) = listener.accept().await.expect("accept");
        let conn = Arc::new(RwLock::new(Connection::new(server_side, peer)));
        let (tx, rx) = mpsc::channel::<Vec<u8>>(TELEMETRY_CHANNEL_CAPACITY);

        let handle = tokio::spawn(Iec104Server::handle_connection(
            conn,
            Arc::new(StubHandler),
            30_000,
            tx.clone(),
            rx,
        ));

        const N: usize = 234;
        let asdu = encode_me_tf1(1, 12.5, 1_000, COT_CYCLIC);
        let expect_len = N * (6 + asdu.len());
        let producer_asdu = asdu.clone();
        tokio::spawn(async move {
            for _ in 0..N {
                let _ = tx.send(producer_asdu.clone()).await;
            }
        });

        let mut buf = vec![0u8; 16 * 1024];
        let mut all: Vec<u8> = Vec::new();
        while all.len() < expect_len {
            let n = client.read(&mut buf).await.expect("read");
            assert!(n > 0, "连接提前关闭（已收 {} B）", all.len());
            all.extend_from_slice(&buf[..n]);
        }

        // 逐帧解析：序号必须是 0..233 的连续序列
        let frame_len = 6 + asdu.len();
        assert_eq!(frame_len, 25, "TI=36 的帧长须为 25 B（§9.2.2 帧长表）");
        let mut seqs = Vec::with_capacity(N);
        for i in 0..N {
            let f = Iec104Frame::parse(&all[i * frame_len..(i + 1) * frame_len]).expect("parse");
            assert_eq!(f.frame_type, FrameType::IFrame);
            assert_eq!(f.asdu, asdu);
            seqs.push(f.send_sequence());
        }
        assert_eq!(seqs[0], 0);
        assert_eq!(seqs[127], 127);
        assert_eq!(seqs[128], 128, "第 129 帧不得回绕（旧实现在此处回绕为 0）");
        assert_eq!(seqs[N - 1], (N - 1) as u16);
        assert!(
            seqs.windows(2).all(|w| w[1] == w[0] + 1),
            "序号必须逐帧连续递增"
        );

        handle.abort();
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
