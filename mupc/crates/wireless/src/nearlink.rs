//! 星闪（NearLink）无线通信驱动
//!
//! NearLink 是中国自主短距无线通信标准，具备以下核心优势：
//! - 极低时延：<20us（优于 WiFi 的 ms 级、BLE 的百 us 级）
//! - 高可靠：支持实时控制场景的确定性调度
//! - 抗干扰：适用于台区复杂的电磁环境
//!
//! 在 MUPC 中的主要用途：
//! - 台区设备实时控制（需 <1ms 响应）
//! - 与 HPLC 配合作为互补无线通道
//!
//! # Phase 2+ 规划
//!
//! 当前提供完整的 trait 定义和配置结构体框架。
//! 实际硬件集成需要：
//! - 星闪芯片 SDK 对接（如 Hi2821）
//! - FFI 接口封装（C/C++ SDK → Rust）
//! - GPIO 中断处理
//! - DMA 数据传输

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::errors::WirelessError;

/// 星闪通信配置参数
///
/// 包含设备标识、信道选择、功率控制和接收增益等参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NearLinkConfig {
    /// 星闪设备节点 ID（唯一标识）
    pub device_node_id: String,
    /// 工作信道（0-39，对应 2.4GHz/5GHz 频段）
    pub channel: u8,
    /// 最大发射功率（dBm），范围 -20 ~ 20
    pub max_power_dbm: i8,
    /// 接收增益（0-7，对应不同增益档位）
    pub rx_gain: u8,
}

impl Default for NearLinkConfig {
    fn default() -> Self {
        Self {
            device_node_id: String::new(),
            channel: 0,
            max_power_dbm: 0,
            rx_gain: 3,
        }
    }
}

/// 星闪驱动抽象 trait
///
/// 定义星闪无线通信的标准接口，上层策略引擎通过此 trait 与台区设备交互。
///
/// # 注意事项
///
/// - 当前为 Phase 1 接口定义阶段，方法返回 `Err(WirelessError::UnsupportedDevice(...))`
/// - Phase 2+ 将对接实际星闪硬件 SDK，实现真正的无线通信
#[async_trait]
pub trait NearLinkDriver: Send + Sync {
    /// 与指定节点建立星闪连接
    ///
    /// # 参数
    /// - `config`: 星闪通信配置
    ///
    /// # 返回
    /// - `Ok(())` 连接成功
    /// - `Err(WirelessError)` 连接失败
    async fn connect(&mut self, config: &NearLinkConfig) -> Result<(), WirelessError>;

    /// 断开当前星闪连接
    async fn disconnect(&mut self) -> Result<(), WirelessError>;

    /// 发送数据到已连接的对端设备
    ///
    /// # 参数
    /// - `data`: 待发送的字节数据
    async fn send(&self, data: &[u8]) -> Result<(), WirelessError>;

    /// 从已连接的对端设备接收数据
    ///
    /// # 参数
    /// - `buf`: 接收缓冲区
    ///
    /// # 返回
    /// - `Ok(usize)` 实际接收的字节数
    async fn recv(&self, buf: &mut [u8]) -> Result<usize, WirelessError>;

    /// 扫描周围的星闪设备
    ///
    /// # 返回
    /// - 发现的设备节点 ID 列表
    async fn scan(&self) -> Result<Vec<String>, WirelessError>;

    /// 检查当前是否处于连接状态
    fn is_connected(&self) -> bool;

    /// 获取当前连接的节点 ID
    fn node_id(&self) -> Option<&str>;
}

/// 默认的空实现（用于不需要星闪功能的场景）
///
/// 所有操作返回 `UnsupportedDevice` 错误，表示星闪功能尚未启用。
#[derive(Debug, Clone, Copy)]
pub struct NoOpNearLinkDriver;

#[async_trait]
impl NearLinkDriver for NoOpNearLinkDriver {
    async fn connect(&mut self, _config: &NearLinkConfig) -> Result<(), WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "星闪功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn disconnect(&mut self) -> Result<(), WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "星闪功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn send(&self, _data: &[u8]) -> Result<(), WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "星闪功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn recv(&self, _buf: &mut [u8]) -> Result<usize, WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "星闪功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn scan(&self) -> Result<Vec<String>, WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "星闪功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    fn is_connected(&self) -> bool {
        false
    }

    fn node_id(&self) -> Option<&str> {
        None
    }
}
