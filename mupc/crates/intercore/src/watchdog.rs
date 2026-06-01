//! 看门狗
//!
//! 10秒超时检测

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error};

use super::HeartbeatManager;

/// 看门狗配置
#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    /// 超时时间（毫秒）
    pub timeout_ms: u64,
    /// 连续超时次数阈值
    pub max_missed_heartbeats: u32,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 10000,  // 10秒
            max_missed_heartbeats: 3,
        }
    }
}

/// 看门狗状态
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WatchdogState {
    Active,
    Timeout,
    Reset,
}

/// 看门狗
pub struct Watchdog {
    config: WatchdogConfig,
    heartbeat_manager: Arc<RwLock<HeartbeatManager>>,
    missed_heartbeats: u32,
    state: WatchdogState,
}

impl Watchdog {
    /// 创建看门狗
    pub fn new(config: WatchdogConfig, heartbeat_manager: Arc<RwLock<HeartbeatManager>>) -> Self {
        Self {
            config,
            heartbeat_manager,
            missed_heartbeats: 0,
            state: WatchdogState::Active,
        }
    }

    /// 获取看门狗状态
    pub fn state(&self) -> WatchdogState {
        self.state
    }

    /// 获取连续丢失心跳次数
    pub fn missed_heartbeats(&self) -> u32 {
        self.missed_heartbeats
    }

    /// 检查是否超时
    pub async fn check_timeout(&mut self) -> bool {
        let all_status = self.heartbeat_manager.read().await.get_all_status().await;

        let any_online = all_status.values().any(|s| s.online);

        if !any_online {
            self.missed_heartbeats += 1;
            if self.missed_heartbeats >= self.config.max_missed_heartbeats {
                self.state = WatchdogState::Timeout;
                warn!("Watchdog timeout triggered: {} consecutive missed heartbeats",
                    self.missed_heartbeats);
                return true;
            }
        } else {
            self.missed_heartbeats = 0;
            self.state = WatchdogState::Active;
        }

        false
    }

    /// 重置看门狗
    pub fn reset(&mut self) {
        self.missed_heartbeats = 0;
        self.state = WatchdogState::Reset;
        info!("Watchdog reset");
    }

    /// 触发复位
    pub async fn trigger_reset(&self) -> Result<(), mupc_common::MupcError> {
        error!("Watchdog triggering system reset");

        // 这里应该触发实时控制模块复位
        // 实际实现需要通过 intercore 发送复位命令

        Ok(())
    }
}