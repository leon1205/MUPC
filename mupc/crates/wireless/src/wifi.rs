//! WiFi 无线通信驱动
//!
//! 提供标准的 WiFi 连接管理、扫描和数据传输接口。
//!
//! 在 MUPC 中的主要用途：
//! - 台区监控摄像头（视频流传输）
//! - 环境传感器（温度、湿度、风速等）
//! - 作为有线通信的备用通道
//!
//! # Phase 2+ 规划
//!
//! 当前提供完整的 trait 定义和配置结构体框架。
//! 实际硬件集成需要：
//! - Linux nl80211/netlink API 集成
//! - wpa_supplicant 对接
//! - 信道扫描与切换
//! - 信号强度监测

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::errors::WirelessError;

/// WiFi 安全协议类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WiFiSecurity {
    /// 开放网络，无加密
    #[default]
    Open,
    /// WPA2-Personal（AES-CCMP）
    WPA2,
    /// WPA3-Personal（SAE / Dragonfly）
    WPA3,
}

/// WiFi 扫描结果
///
/// 包含一次扫描中发现的一个 WiFi 网络的信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WiFiScanResult {
    /// 网络 SSID
    pub ssid: String,
    /// BSSID（MAC 地址）
    pub bssid: String,
    /// 信号强度（dBm）
    pub signal_dbm: i16,
    /// 使用的信道
    pub channel: u8,
    /// 安全协议类型
    pub security: WiFiSecurity,
}

/// WiFi 通信配置参数
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct WiFiConfig {
    /// 网络 SSID
    pub ssid: String,
    /// 网络密码（Open 网络为 None）
    pub password: Option<String>,
    /// 安全协议类型
    pub security: WiFiSecurity,
    /// 指定信道（None 表示自动选择）
    pub channel: Option<u8>,
}

/// WiFi 驱动抽象 trait
///
/// 定义 WiFi 无线通信的标准接口，上层策略引擎通过此 trait 与台区设备交互。
///
/// # 注意事项
///
/// - 当前为 Phase 1 接口定义阶段，方法返回 `Err(WirelessError::UnsupportedDevice(...))`
/// - Phase 2+ 将对接实际 nl80211/netlink API，实现真正的 WiFi 通信和扫描
#[async_trait]
pub trait WiFiDriver: Send + Sync {
    /// 连接到指定的 WiFi 网络
    ///
    /// # 参数
    /// - `config`: WiFi 网络配置（SSID、密码、安全协议等）
    async fn connect(&mut self, config: &WiFiConfig) -> Result<(), WirelessError>;

    /// 断开当前 WiFi 连接
    async fn disconnect(&mut self) -> Result<(), WirelessError>;

    /// 扫描周围的 WiFi 网络
    ///
    /// # 返回
    /// - 发现的 WiFi 网络列表，按信号强度降序排列
    async fn scan(&self) -> Result<Vec<WiFiScanResult>, WirelessError>;

    /// 通过 WiFi 发送数据
    ///
    /// # 参数
    /// - `data`: 待发送的字节数据
    async fn send(&self, data: &[u8]) -> Result<(), WirelessError>;

    /// 通过 WiFi 接收数据
    ///
    /// # 参数
    /// - `buf`: 接收缓冲区
    ///
    /// # 返回
    /// - `Ok(usize)` 实际接收的字节数
    async fn recv(&self, buf: &mut [u8]) -> Result<usize, WirelessError>;

    /// 检查当前是否已连接到 WiFi 网络
    fn is_connected(&self) -> bool;

    /// 获取当前连接的信噪比/信号强度（dBm）
    ///
    /// # 返回
    /// - `Some(dbm)` 信号强度值
    /// - `None` 未连接或无法获取
    fn signal_strength_dbm(&self) -> Option<i16>;
}

/// 默认的空实现（用于不需要 WiFi 功能的场景）
///
/// 所有操作返回 `UnsupportedDevice` 错误，表示 WiFi 功能尚未启用。
#[derive(Debug, Clone, Copy)]
pub struct NoOpWiFiDriver;

#[async_trait]
impl WiFiDriver for NoOpWiFiDriver {
    async fn connect(&mut self, _config: &WiFiConfig) -> Result<(), WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "WiFi 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn disconnect(&mut self) -> Result<(), WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "WiFi 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn scan(&self) -> Result<Vec<WiFiScanResult>, WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "WiFi 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn send(&self, _data: &[u8]) -> Result<(), WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "WiFi 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn recv(&self, _buf: &mut [u8]) -> Result<usize, WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "WiFi 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    fn is_connected(&self) -> bool {
        false
    }

    fn signal_strength_dbm(&self) -> Option<i16> {
        None
    }
}
