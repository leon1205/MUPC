//! 南向命令发送器（模拟实现）
//!
//! 负责将 pv_limit 和 load_shedding 命令发送到南向设备（光伏逆变器、柔性负荷装置）
//!
//! Phase 2+ 将替换为真实的 RS485/HPLC 通信实现

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 南向命令类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SouthCommandType {
    /// 光伏限功率命令
    PvLimit,
    /// 负荷切除命令
    LoadShedding,
    /// 组合命令（同时包含两者）
    Combined,
}

/// 光伏限功率命令
#[derive(Debug, Clone)]
pub struct PvLimitCommand {
    /// 目标设备ID
    pub device_id: String,
    /// 限功率比例 [0.0, 1.0]
    pub limit_ratio: f64,
    /// 命令优先级
    pub priority: u8,
}

/// 负荷切除命令
#[derive(Debug, Clone)]
pub struct LoadSheddingCommand {
    /// 目标设备ID
    pub device_id: String,
    /// 切除功率 (kW)
    pub power_kw: f64,
    /// 命令优先级
    pub priority: u8,
}

/// 南向命令发送结果
#[derive(Debug, Clone)]
pub struct SouthSendResult {
    /// 是否成功
    pub success: bool,
    /// 设备ID
    pub device_id: String,
    /// 命令类型
    pub command_type: SouthCommandType,
    /// 错误消息（如有）
    pub error_message: Option<String>,
}

/// 南向命令发送器 trait
#[async_trait]
pub trait SouthCommandSender: Send + Sync {
    /// 发送光伏限功率命令
    async fn send_pv_limit(&self, cmd: PvLimitCommand) -> SouthSendResult;

    /// 发送负荷切除命令
    async fn send_load_shedding(&self, cmd: LoadSheddingCommand) -> SouthSendResult;
}

/// 模拟南向命令发送器
///
/// 用于开发/测试阶段，将命令打印到日志而不真正发送到设备
pub struct MockSouthCommandSender {
    /// 已发送命令计数
    pv_limit_count: std::sync::atomic::AtomicU64,
    load_shedding_count: std::sync::atomic::AtomicU64,
}

impl MockSouthCommandSender {
    /// 创建新的模拟发送器
    pub fn new() -> Self {
        Self {
            pv_limit_count: std::sync::atomic::AtomicU64::new(0),
            load_shedding_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 获取已发送的 pv_limit 命令数量
    pub fn pv_limit_sent_count(&self) -> u64 {
        self.pv_limit_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 获取已发送的 load_shedding 命令数量
    pub fn load_shedding_sent_count(&self) -> u64 {
        self.load_shedding_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for MockSouthCommandSender {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SouthCommandSender for MockSouthCommandSender {
    async fn send_pv_limit(&self, cmd: PvLimitCommand) -> SouthSendResult {
        let count = self
            .pv_limit_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        tracing::info!(
            "[MockSouth] PV Limit #{}: device={}, limit_ratio={:.2}, priority={}",
            count + 1,
            cmd.device_id,
            cmd.limit_ratio,
            cmd.priority
        );

        SouthSendResult {
            success: true,
            device_id: cmd.device_id,
            command_type: SouthCommandType::PvLimit,
            error_message: None,
        }
    }

    async fn send_load_shedding(&self, cmd: LoadSheddingCommand) -> SouthSendResult {
        let count = self
            .load_shedding_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        tracing::info!(
            "[MockSouth] Load Shedding #{}: device={}, power_kw={:.2}, priority={}",
            count + 1,
            cmd.device_id,
            cmd.power_kw,
            cmd.priority
        );

        SouthSendResult {
            success: true,
            device_id: cmd.device_id,
            command_type: SouthCommandType::LoadShedding,
            error_message: None,
        }
    }
}

/// RS485 南向命令发送器
///
/// 通过真实的 Rs485Device 发送 pv_limit 和 load_shedding 命令
pub struct Rs485SouthSender {
    pv_device: std::sync::Arc<rs485_plugin::device::Rs485Device>,
    load_device: std::sync::Arc<rs485_plugin::device::Rs485Device>,
    /// 已发送计数
    pv_count: std::sync::atomic::AtomicU64,
    load_count: std::sync::atomic::AtomicU64,
}

impl Rs485SouthSender {
    /// 创建 RS485 南向发送器
    ///
    /// # Arguments
    /// - `pv_device`: 光伏逆变器设备
    /// - `load_device`: 负荷控制设备
    pub fn new(
        pv_device: std::sync::Arc<rs485_plugin::device::Rs485Device>,
        load_device: std::sync::Arc<rs485_plugin::device::Rs485Device>,
    ) -> Self {
        Self {
            pv_device,
            load_device,
            pv_count: std::sync::atomic::AtomicU64::new(0),
            load_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// 获取已发送的 pv_limit 命令数量
    pub fn pv_limit_sent_count(&self) -> u64 {
        self.pv_count.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 获取已发送的 load_shedding 命令数量
    pub fn load_shedding_sent_count(&self) -> u64 {
        self.load_count.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[async_trait]
impl SouthCommandSender for Rs485SouthSender {
    async fn send_pv_limit(&self, cmd: PvLimitCommand) -> SouthSendResult {
        let count = self
            .pv_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        tracing::info!(
            "[Rs485South] PV Limit #{}: device={}, limit_ratio={:.2}, priority={}",
            count + 1,
            cmd.device_id,
            cmd.limit_ratio,
            cmd.priority
        );

        // 同步调用 RS485 设备发送（Rs485Device 方法是同步的）
        // 注意：生产环境应使用 tokio::task::spawn_blocking 避免阻塞异步运行时
        match self.pv_device.send_pv_limit(cmd.limit_ratio, 1000) {
            Ok(_response) => SouthSendResult {
                success: true,
                device_id: cmd.device_id,
                command_type: SouthCommandType::PvLimit,
                error_message: None,
            },
            Err(e) => SouthSendResult {
                success: false,
                device_id: cmd.device_id,
                command_type: SouthCommandType::PvLimit,
                error_message: Some(e.to_string()),
            },
        }
    }

    async fn send_load_shedding(&self, cmd: LoadSheddingCommand) -> SouthSendResult {
        let count = self
            .load_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        tracing::info!(
            "[Rs485South] Load Shedding #{}: device={}, power_kw={:.1}, priority={}",
            count + 1,
            cmd.device_id,
            cmd.power_kw,
            cmd.priority
        );

        match self.load_device.send_load_shedding(cmd.power_kw, 1000) {
            Ok(_response) => SouthSendResult {
                success: true,
                device_id: cmd.device_id,
                command_type: SouthCommandType::LoadShedding,
                error_message: None,
            },
            Err(e) => SouthSendResult {
                success: false,
                device_id: cmd.device_id,
                command_type: SouthCommandType::LoadShedding,
                error_message: Some(e.to_string()),
            },
        }
    }
}

/// 南向命令分发器
///
/// 协调 pv_limit 和 load_shedding 命令的发送
pub struct SouthCommandDispatcher {
    sender: Arc<dyn SouthCommandSender>,
    /// 默认光伏设备ID
    default_pv_device_id: String,
    /// 默认负荷设备ID
    default_load_device_id: String,
}

impl SouthCommandDispatcher {
    /// 创建新的分发器
    pub fn new(
        sender: Arc<dyn SouthCommandSender>,
        default_pv_device_id: &str,
        default_load_device_id: &str,
    ) -> Self {
        Self {
            sender,
            default_pv_device_id: default_pv_device_id.to_string(),
            default_load_device_id: default_load_device_id.to_string(),
        }
    }

    /// 创建使用模拟发送器的分发器
    pub fn with_mock(default_pv_device_id: &str, default_load_device_id: &str) -> Self {
        Self::new(
            Arc::new(MockSouthCommandSender::new()),
            default_pv_device_id,
            default_load_device_id,
        )
    }

    /// 分发 pv_limit 命令
    pub async fn dispatch_pv_limit(&self, limit_ratio: f64, priority: u8) -> SouthSendResult {
        let cmd = PvLimitCommand {
            device_id: self.default_pv_device_id.clone(),
            limit_ratio,
            priority,
        };
        self.sender.send_pv_limit(cmd).await
    }

    /// 分发 load_shedding 命令
    pub async fn dispatch_load_shedding(&self, power_kw: f64, priority: u8) -> SouthSendResult {
        let cmd = LoadSheddingCommand {
            device_id: self.default_load_device_id.clone(),
            power_kw,
            priority,
        };
        self.sender.send_load_shedding(cmd).await
    }

    /// 获取发送器引用（用于测试）
    pub fn sender(&self) -> Arc<dyn SouthCommandSender> {
        self.sender.clone()
    }
}

/// 全局南向命令分发器（使用 RwLock 实现延迟初始化）
static DISPATCHER: std::sync::OnceLock<RwLock<Option<Arc<SouthCommandDispatcher>>>> =
    std::sync::OnceLock::new();

/// 获取全局分发器
pub fn get_dispatcher() -> Option<Arc<SouthCommandDispatcher>> {
    DISPATCHER.get().and_then(|rw| {
        let guard = rw.try_read().ok()?;
        guard.as_ref().map(|d| d.clone())
    })
}

/// 设置全局分发器
pub fn set_dispatcher(dispatcher: Arc<SouthCommandDispatcher>) {
    let _ = DISPATCHER.get_or_init(|| RwLock::new(Some(dispatcher)));
}

/// 清空全局分发器（用于测试）
#[cfg(test)]
pub fn clear_dispatcher() {
    if let Some(rw) = DISPATCHER.get() {
        if let Ok(mut guard) = rw.try_write() {
            *guard = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_sender_pv_limit() {
        let sender = MockSouthCommandSender::new();
        let cmd = PvLimitCommand {
            device_id: "pv_inverter_001".to_string(),
            limit_ratio: 0.8,
            priority: 1,
        };
        let result = sender.send_pv_limit(cmd).await;
        assert!(result.success);
        assert_eq!(result.device_id, "pv_inverter_001");
        assert_eq!(sender.pv_limit_sent_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_sender_load_shedding() {
        let sender = MockSouthCommandSender::new();
        let cmd = LoadSheddingCommand {
            device_id: "load_ctrl_001".to_string(),
            power_kw: 30.0,
            priority: 2,
        };
        let result = sender.send_load_shedding(cmd).await;
        assert!(result.success);
        assert_eq!(result.device_id, "load_ctrl_001");
        assert_eq!(sender.load_shedding_sent_count(), 1);
    }

    #[test]
    fn test_dispatcher_with_mock() {
        let dispatcher = SouthCommandDispatcher::with_mock("pv_001", "load_001");
        assert_eq!(dispatcher.default_pv_device_id, "pv_001");
        assert_eq!(dispatcher.default_load_device_id, "load_001");
    }
}
