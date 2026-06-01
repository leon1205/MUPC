//! 心跳管理
//!
//! 1秒周期心跳检测

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::warn;

/// 心跳状态
#[derive(Debug, Clone)]
pub struct HeartbeatStatus {
    /// 是否在线
    pub online: bool,
    /// 最后心跳时间戳
    pub last_heartbeat: u64,
    /// 状态码
    pub status: u8,
    /// CPU 温度
    pub cpu_temp: f64,
    /// 内存使用率
    pub memory_usage: f64,
}

/// 心跳管理器
pub struct HeartbeatManager {
    /// 心跳间隔（毫秒）
    heartbeat_interval_ms: u64,
    /// 看门狗超时（毫秒）
    watchdog_timeout_ms: u64,
    /// 连接状态
    connections: Arc<RwLock<HashMap<SocketAddr, HeartbeatStatus>>>,
}

impl HeartbeatManager {
    /// 创建心跳管理器
    pub fn new(heartbeat_interval_ms: u64, watchdog_timeout_ms: u64) -> Self {
        Self {
            heartbeat_interval_ms,
            watchdog_timeout_ms,
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册连接
    pub fn register_connection(&self, addr: SocketAddr) {
        let status = HeartbeatStatus {
            online: true,
            last_heartbeat: chrono::Utc::now().timestamp() as u64,
            status: 0,
            cpu_temp: 0.0,
            memory_usage: 0.0,
        };

        let connections = self.connections.clone();
        tokio::spawn(async move {
            connections.write().await.insert(addr, status);
        });
    }

    /// 注销连接
    pub fn unregister_connection(&self, addr: SocketAddr) {
        let connections = self.connections.clone();
        tokio::spawn(async move {
            connections.write().await.remove(&addr);
        });
    }

    /// 接收心跳
    pub async fn receive_heartbeat(&self, addr: SocketAddr) {
        let mut connections = self.connections.write().await;
        if let Some(status) = connections.get_mut(&addr) {
            status.last_heartbeat = chrono::Utc::now().timestamp() as u64;
            status.online = true;
        }
    }

    /// 获取连接状态
    pub async fn get_connection_status(&self, addr: &SocketAddr) -> Option<HeartbeatStatus> {
        self.connections.read().await.get(addr).cloned()
    }

    /// 获取所有连接状态
    pub async fn get_all_status(&self) -> HashMap<SocketAddr, HeartbeatStatus> {
        self.connections.read().await.clone()
    }

    /// 检查连接是否超时
    pub async fn is_connection_timeout(&self, addr: &SocketAddr) -> bool {
        if let Some(status) = self.connections.read().await.get(addr) {
            let now = chrono::Utc::now().timestamp() as u64;
            let elapsed = now - status.last_heartbeat;
            elapsed * 1000 > self.watchdog_timeout_ms
        } else {
            true
        }
    }

    /// 运行心跳检测循环
    pub async fn run(&self) {
        let mut ticker = interval(Duration::from_millis(self.heartbeat_interval_ms));

        loop {
            ticker.tick().await;

            let now = chrono::Utc::now().timestamp() as u64;
            let mut connections = self.connections.write().await;

            for (addr, status) in connections.iter_mut() {
                let elapsed = now - status.last_heartbeat;
                if elapsed * 1000 > self.watchdog_timeout_ms {
                    if status.online {
                        warn!("Heartbeat timeout for {}: {} seconds", addr, elapsed);
                        status.online = false;
                    }
                }
            }
        }
    }
}