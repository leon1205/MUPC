//! BLE（低功耗蓝牙）无线通信驱动
//!
//! 提供标准 BLE 设备发现、连接管理、服务订阅和数据交互接口。
//!
//! 在 MUPC 中的主要用途：
//! - 台区低功耗传感器（温度、湿度、振动等）
//! - 手持巡检终端
//! - 低带宽遥测数据采集
//!
//! # Phase 2+ 规划
//!
//! 当前提供完整的 trait 定义和配置结构体框架。
//! 实际硬件集成需要：
//! - BlueZ D-Bus API 集成（Linux）
//! - GATT 服务发现与特征值操作
//! - BLE 配对与绑定管理
//! - MTU 协商

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::errors::WirelessError;

/// BLE 设备扫描结果
///
/// 包含一次设备发现过程中找到的 BLE 设备信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BleScanResult {
    /// 设备名称（广播名称）
    pub name: String,
    /// 设备 MAC 地址（格式：XX:XX:XX:XX:XX:XX）
    pub address: String,
    /// 接收信号强度指示（dBm）
    pub rssi: i16,
}

/// BLE 通信配置参数
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BleConfig {
    /// 目标设备名称（用于扫描过滤）
    pub device_name: String,
    /// 目标设备 MAC 地址（可选，用于直连）
    pub mac_address: Option<String>,
    /// GATT 服务 UUID
    pub service_uuid: String,
    /// GATT 特征值 UUID（用于数据读写）
    pub char_uuid: String,
    /// 最大传输单元（字节），默认 23，可协商至 247
    pub mtu: u16,
}

impl Default for BleConfig {
    fn default() -> Self {
        Self {
            device_name: String::new(),
            mac_address: None,
            service_uuid: String::new(),
            char_uuid: String::new(),
            mtu: 23,
        }
    }
}

/// BLE 驱动抽象 trait
///
/// 定义 BLE 无线通信的标准接口，上层策略引擎通过此 trait 与台区低功耗设备交互。
///
/// # 注意事项
///
/// - 当前为 Phase 1 接口定义阶段，方法返回 `Err(WirelessError::UnsupportedDevice(...))`
/// - Phase 2+ 将对接实际 BlueZ D-Bus API，实现真正的 BLE 通信
#[async_trait]
pub trait BleDriver: Send + Sync {
    /// 连接到指定的 BLE 设备
    ///
    /// # 参数
    /// - `config`: BLE 设备配置（设备名、服务 UUID、特征值 UUID 等）
    async fn connect(&mut self, config: &BleConfig) -> Result<(), WirelessError>;

    /// 断开当前 BLE 连接
    async fn disconnect(&mut self) -> Result<(), WirelessError>;

    /// 发现 GATT 服务及特征值
    ///
    /// # 返回
    /// - 可用的服务 UUID 列表
    async fn discover_services(&self) -> Result<Vec<String>, WirelessError>;

    /// 订阅 GATT 特征值通知
    ///
    /// 使能指定特征值的 notify/indicate 属性，
    /// 当远端设备更新特征值时自动接收通知。
    async fn subscribe(&self, char_uuid: &str) -> Result<(), WirelessError>;

    /// 写入 GATT 特征值
    ///
    /// # 参数
    /// - `char_uuid`: 目标特征值 UUID
    /// - `data`: 待写入的数据
    async fn write_characteristic(&self, char_uuid: &str, data: &[u8]) -> Result<(), WirelessError>;

    /// 扫描周围的 BLE 设备
    ///
    /// # 参数
    /// - `duration_ms`: 扫描持续时间（毫秒）
    ///
    /// # 返回
    /// - 发现的 BLE 设备列表，按 RSSI 降序排列
    async fn scan(&self, duration_ms: u32) -> Result<Vec<BleScanResult>, WirelessError>;

    /// 检查当前是否处于连接状态
    fn is_connected(&self) -> bool;
}

/// 默认的空实现（用于不需要 BLE 功能的场景）
///
/// 所有操作返回 `UnsupportedDevice` 错误，表示 BLE 功能尚未启用。
#[derive(Debug, Clone, Copy)]
pub struct NoOpBleDriver;

#[async_trait]
impl BleDriver for NoOpBleDriver {
    async fn connect(&mut self, _config: &BleConfig) -> Result<(), WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "BLE 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn disconnect(&mut self) -> Result<(), WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "BLE 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn discover_services(&self) -> Result<Vec<String>, WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "BLE 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn subscribe(&self, _char_uuid: &str) -> Result<(), WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "BLE 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn write_characteristic(&self, _char_uuid: &str, _data: &[u8]) -> Result<(), WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "BLE 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    async fn scan(&self, _duration_ms: u32) -> Result<Vec<BleScanResult>, WirelessError> {
        Err(WirelessError::UnsupportedDevice(
            "BLE 功能尚未启用（Phase 2+ 实现）".into(),
        ))
    }

    fn is_connected(&self) -> bool {
        false
    }
}
