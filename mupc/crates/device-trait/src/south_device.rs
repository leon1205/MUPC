//! 南向设备统一抽象
//!
//! 提供 SouthDevice、ProtocolHandler、HplcDriver 等核心 trait 定义
//! 用于统一抽象 RS485、HPLC 等南向通信设备

use crate::errors::DeviceError;
use crate::types::{crc16_modbus, CrcMode, DataFrame, DeviceStatus};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::sync::Arc;
use thiserror::Error;

// ============================================================================
// SouthDevice trait - 南向设备统一接口
// ============================================================================

/// 南向设备统一接口
///
/// 所有南向设备（RS485/HPLC/其他）都实现此 trait
pub trait SouthDevice: Send + Sync {
    /// 获取设备ID
    fn device_id(&self) -> &str;

    /// 获取设备类型
    fn device_type(&self) -> &str;

    /// 获取设备状态
    fn status(&self) -> Result<DeviceStatus, DeviceError>;

    /// 连接设备
    fn connect(&self) -> Result<(), DeviceError>;

    /// 断开连接
    fn disconnect(&self) -> Result<(), DeviceError>;

    /// 读取数据
    fn read(&self) -> Result<DataFrame, DeviceError>;

    /// 批量读取（支持 ≥1Hz 遥测数据上送）
    fn read_batch(&self, count: usize) -> Result<Vec<DataFrame>, DeviceError>;

    /// 写入数据
    fn write(&self, data: &[u8]) -> Result<(), DeviceError>;

    /// 健康检查/心跳
    fn health_check(&self) -> Result<bool, DeviceError>;
}

// ============================================================================
// ProtocolHandler trait - RS485 协议处理器
// ============================================================================

/// RS485 协议处理器
///
/// 通过依赖注入支持多种协议（Modbus、TTU、逆变器、充电桩等）
pub trait ProtocolHandler: Send + Sync {
    /// 编码请求数据
    ///
    /// # Arguments
    /// - `device_id`: 目标设备ID
    /// - `data`: 原始数据载荷
    ///
    /// # Returns
    /// 编码后的字节帧
    fn encode_request(&self, device_id: &str, data: &[u8]) -> Vec<u8>;

    /// 解码响应数据
    ///
    /// # Arguments
    /// - `frame`: 原始响应帧
    ///
    /// # Returns
    /// 解码后的数据帧
    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError>;

    /// 获取协议名称
    fn name(&self) -> &'static str;
}

// ============================================================================
// HPLC 错误类型
// ============================================================================

/// HPLC 驱动错误
#[derive(Debug, Error)]
pub enum HplcError {
    /// 驱动初始化失败
    #[error("驱动初始化失败: {0}")]
    InitFailed(String),

    /// 发送失败
    #[error("发送失败: {0}")]
    SendFailed(String),

    /// 接收失败
    #[error("接收失败: {0}")]
    RecvFailed(String),

    /// 连接断开
    #[error("连接断开: {0}")]
    Disconnected(String),

    /// SDK 错误
    #[error("SDK 错误: {0}")]
    SdkError(String),
}

impl HplcError {
    /// 创建初始化失败错误
    pub fn init_failed(msg: impl Into<String>) -> Self {
        Self::InitFailed(msg.into())
    }

    /// 创建发送失败错误
    pub fn send_failed(msg: impl Into<String>) -> Self {
        Self::SendFailed(msg.into())
    }

    /// 创建接收失败错误
    pub fn recv_failed(msg: impl Into<String>) -> Self {
        Self::RecvFailed(msg.into())
    }

    /// 创建连接断开错误
    pub fn disconnected(msg: impl Into<String>) -> Self {
        Self::Disconnected(msg.into())
    }

    /// 创建 SDK 错误
    pub fn sdk_error(msg: impl Into<String>) -> Self {
        Self::SdkError(msg.into())
    }
}

// ============================================================================
// HPLC 配置
// ============================================================================

/// HPLC 设备配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HplcConfig {
    /// 串口路径（支持跨平台：Linux=/dev/ttyUSB0, Windows=COM3）
    #[serde(alias = "serial_port", alias = "com_port")]
    pub port: String,

    /// 波特率
    pub baud_rate: u32,

    /// 芯片型号（FFI 预留）
    pub chip_type: Option<String>,

    /// 通道号
    pub channel: Option<u8>,
}

impl HplcConfig {
    /// 创建新的 HPLC 配置
    pub fn new(port: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            port: port.into(),
            baud_rate,
            chip_type: None,
            channel: None,
        }
    }

    /// 设置芯片型号
    pub fn with_chip_type(mut self, chip_type: impl Into<String>) -> Self {
        self.chip_type = Some(chip_type.into());
        self
    }

    /// 设置通道号
    pub fn with_channel(mut self, channel: u8) -> Self {
        self.channel = Some(channel);
        self
    }
}

// ============================================================================
// HplcDriver trait - HPLC 驱动接口
// ============================================================================

/// HPLC 驱动接口
///
/// 抽象不同芯片厂商的 PLC SDK
pub trait HplcDriver: Send + Sync {
    /// 转换为 Any 类型，用于 downcasting
    fn as_any(&self) -> &dyn Any;

    /// 初始化驱动
    fn init(&self, config: HplcConfig) -> Result<(), HplcError>;

    /// 发送数据
    fn send(&self, data: &[u8]) -> Result<(), HplcError>;

    /// 接收数据
    fn recv(&self, timeout_ms: u64) -> Result<Vec<u8>, HplcError>;

    /// 检查连接状态
    fn is_connected(&self) -> bool;

    /// 获取驱动名称
    fn driver_name(&self) -> &'static str;
}

// ============================================================================
// 协议处理器注册表
// ============================================================================

/// 协议处理器注册表
///
/// 用于根据名称获取对应的协议处理器实例
pub struct ProtocolHandlerRegistry;

impl ProtocolHandlerRegistry {
    /// 根据名称获取协议处理器实例
    ///
    /// # Arguments
    /// - `name`: 协议处理器名称（modbus/ttu/inverter/charger）
    /// - `config`: 设备配置（包含 device_addr 等参数）
    ///
    /// # Returns
    /// 协议处理器实例，如果名称不匹配则返回 None
    pub fn get(name: &str, config: &crate::types::Rs485Config) -> Option<Arc<dyn ProtocolHandler>> {
        match name {
            "modbus" => Some(Arc::new(ModbusHandler::new(
                config.device_addr,
                config.crc_mode,
            ))),
            "ttu" => Some(Arc::new(TtuHandler::new(config.device_addr))),
            "inverter" => Some(Arc::new(InverterHandler::new(config.device_addr))),
            "charger" => Some(Arc::new(ChargerHandler::new(config.device_addr))),
            _ => None,
        }
    }
}

// ============================================================================
// 内置协议处理器实现
// ============================================================================

/// Modbus RTU 协议处理器
#[allow(dead_code)]
pub struct ModbusHandler {
    device_addr: u8,
    crc_mode: CrcMode,
}

impl ModbusHandler {
    /// 创建新的 Modbus 处理器
    pub fn new(device_addr: u8, crc_mode: CrcMode) -> Self {
        Self {
            device_addr,
            crc_mode,
        }
    }

    /// 设置设备地址
    pub fn with_device_addr(mut self, addr: u8) -> Self {
        self.device_addr = addr;
        self
    }
}

impl ProtocolHandler for ModbusHandler {
    fn encode_request(&self, device_id: &str, data: &[u8]) -> Vec<u8> {
        let mut frame = vec![self.device_addr];
        frame.extend_from_slice(data);
        let crc = crc16_modbus(&frame);
        // Modbus RTU wire format: CRC low byte first (little-endian)
        frame.push(crc as u8);
        frame.push((crc >> 8) as u8);
        tracing::debug!(
            "ModbusHandler 编码请求 device_id={} addr={} data={:?}",
            device_id,
            self.device_addr,
            data
        );
        frame
    }

    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError> {
        if frame.len() < 5 {
            return Err(DeviceError::protocol_error("Modbus 响应太短"));
        }
        // 简单验证：检查地址匹配和 CRC
        if frame[0] != self.device_addr {
            return Err(DeviceError::protocol_error("Modbus 地址不匹配"));
        }
        // Modbus RTU wire format: CRC low byte first (little-endian)
        let frame_crc = ((frame[frame.len() - 1] as u16) << 8) | (frame[frame.len() - 2] as u16);
        let calc_crc = crc16_modbus(&frame[..frame.len() - 2]);
        if frame_crc != calc_crc {
            return Err(DeviceError::checksum_failed("Modbus CRC 校验失败"));
        }
        Ok(DataFrame::new(format!("modbus_{}", self.device_addr), frame.to_vec()))
    }

    fn name(&self) -> &'static str {
        "ModbusRTU"
    }
}

/// TTU 协议处理器
#[allow(dead_code)]
pub struct TtuHandler {
    device_addr: u8,
    protocol_version: u8,
}

impl TtuHandler {
    /// 创建新的 TTU 处理器
    pub fn new(device_addr: u8) -> Self {
        Self {
            device_addr,
            protocol_version: 0x10,
        }
    }
}

impl ProtocolHandler for TtuHandler {
    fn encode_request(&self, device_id: &str, data: &[u8]) -> Vec<u8> {
        let mut frame = vec![0x68]; // 起始符
        frame.push(self.protocol_version);
        frame.push(data.len() as u8);
        frame.extend_from_slice(data);
        let checksum: u8 = frame[1..].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        frame.push(checksum);
        frame.push(0x16); // 结束符
        tracing::debug!("TtuHandler 编码请求 device_id={}", device_id);
        frame
    }

    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError> {
        if frame.is_empty() || frame[0] != 0x68 {
            return Err(DeviceError::protocol_error("TTU 帧起始符错误"));
        }
        if frame.len() < 4 {
            return Err(DeviceError::protocol_error("TTU 响应太短"));
        }
        let data_len = frame[2] as usize;
        // 验证 data_len + 3 不越界（data_len + 3 为 payload + checksum 长度）
        let end_idx = data_len.checked_add(3).ok_or_else(|| {
            DeviceError::protocol_error("TTU 数据长度计算溢出")
        })?;
        if frame.len() < end_idx {
            return Err(DeviceError::protocol_error("TTU 数据长度不匹配"));
        }
        let payload = frame[1..end_idx].to_vec();
        Ok(DataFrame::new("ttu".to_string(), payload))
    }

    fn name(&self) -> &'static str {
        "TTU"
    }
}

impl Default for TtuHandler {
    fn default() -> Self {
        Self::new(0x01)
    }
}

/// 光伏逆变器协议处理器
#[allow(dead_code)]
pub struct InverterHandler {
    device_addr: u8,
    manufacturer_code: u16,
}

impl InverterHandler {
    /// 创建新的逆变器处理器
    pub fn new(device_addr: u8) -> Self {
        Self {
            device_addr,
            manufacturer_code: 0x0000,
        }
    }
}

impl ProtocolHandler for InverterHandler {
    fn encode_request(&self, device_id: &str, data: &[u8]) -> Vec<u8> {
        let mut frame = vec![0x01]; // 帧头
        frame.push((data.len() >> 8) as u8);
        frame.push(data.len() as u8);
        frame.extend_from_slice(data);
        tracing::debug!("InverterHandler 编码请求 device_id={}", device_id);
        frame
    }

    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError> {
        if frame.len() < 4 {
            return Err(DeviceError::protocol_error("逆变器响应太短"));
        }
        let data_len = ((frame[1] as usize) << 8) | (frame[2] as usize);
        if frame.len() < data_len + 4 {
            return Err(DeviceError::protocol_error("逆变器数据长度不匹配"));
        }
        Ok(DataFrame::new("inverter".to_string(), frame.to_vec()))
    }

    fn name(&self) -> &'static str {
        "Inverter"
    }
}

impl Default for InverterHandler {
    fn default() -> Self {
        Self::new(0x01)
    }
}

/// 充电桩协议处理器（GB/T 27930）
#[allow(dead_code)]
pub struct ChargerHandler {
    device_addr: u8,
    protocol_version: u8,
}

impl ChargerHandler {
    /// 创建新的充电桩处理器
    pub fn new(device_addr: u8) -> Self {
        Self {
            device_addr,
            protocol_version: 0x01,
        }
    }
}

impl ProtocolHandler for ChargerHandler {
    fn encode_request(&self, device_id: &str, data: &[u8]) -> Vec<u8> {
        let mut frame = vec![0xFF, 0xFE]; // 起始标志
        frame.push(self.protocol_version);
        frame.extend_from_slice(data);
        let checksum: u8 = frame[2..].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        frame.push(checksum);
        tracing::debug!("ChargerHandler 编码请求 device_id={}", device_id);
        frame
    }

    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError> {
        if frame.len() < 5 {
            return Err(DeviceError::protocol_error("充电桩响应太短"));
        }
        if frame[0] != 0xFF || frame[1] != 0xFE {
            return Err(DeviceError::protocol_error("充电桩起始标志错误"));
        }
        Ok(DataFrame::new("charger".to_string(), frame.to_vec()))
    }

    fn name(&self) -> &'static str {
        "ChargerGB27930"
    }
}

impl Default for ChargerHandler {
    fn default() -> Self {
        Self::new(0x01)
    }
}

// ============================================================================
// 模块导出
// ============================================================================

/// SouthDevice 泛型实现标记
///
/// 用于标记实现了 SouthDevice trait 的类型
pub type SouthDeviceMarker = ();

/// ProtocolHandler 泛型实现标记
pub type ProtocolHandlerMarker = ();

/// HplcDriver 泛型实现标记
pub type HplcDriverMarker = ();

// Re-export all public types
pub use crate::errors::DeviceError as SouthDeviceError;

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modbus_handler_encode_decode() {
        let handler = ModbusHandler::new(0x01, CrcMode::Crc16Modbus);
        let data = vec![0x03, 0x00, 0x00, 0x00, 0x10]; // 读保持寄存器
        let frame = handler.encode_request("test_device", &data);
        assert!(frame.len() > data.len() + 3); // 地址 + 数据 + CRC
        assert_eq!(frame[0], 0x01); // 地址

        // 解码验证
        let result = handler.decode_response(&frame);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ttu_handler_encode_decode() {
        let handler = TtuHandler::new(0x01);
        let data = vec![0x01, 0x02, 0x03];
        let frame = handler.encode_request("ttu_001", &data);
        assert_eq!(frame[0], 0x68); // 起始符
        assert_eq!(frame[frame.len() - 1], 0x16); // 结束符

        let result = handler.decode_response(&frame);
        assert!(result.is_ok());
    }

    #[test]
    fn test_inverter_handler_encode_decode() {
        let handler = InverterHandler::new(0x01);
        let data = vec![0x10, 0x00];
        let frame = handler.encode_request("inv_001", &data);
        assert!(frame.len() >= 4);

        let result = handler.decode_response(&frame);
        assert!(result.is_ok());
    }

    #[test]
    fn test_charger_handler_encode_decode() {
        let handler = ChargerHandler::new(0x01);
        let data = vec![0x01, 0x00];
        let frame = handler.encode_request("charger_001", &data);
        assert_eq!(frame[0], 0xFF);
        assert_eq!(frame[1], 0xFE);

        let result = handler.decode_response(&frame);
        assert!(result.is_ok());
    }

    #[test]
    fn test_protocol_handler_registry() {
        let config = crate::types::Rs485Config::default();
        let handler = ProtocolHandlerRegistry::get("modbus", &config);
        assert!(handler.is_some());
        assert_eq!(handler.unwrap().name(), "ModbusRTU");

        let unknown = ProtocolHandlerRegistry::get("unknown", &config);
        assert!(unknown.is_none());
    }

    #[test]
    fn test_hplc_config_new() {
        let config = HplcConfig::new("/dev/ttyUSB0", 115200);
        assert_eq!(config.port, "/dev/ttyUSB0");
        assert_eq!(config.baud_rate, 115200);
    }

    #[test]
    fn test_hplc_config_with_options() {
        let config = HplcConfig::new("COM3", 9600)
            .with_chip_type("G3")
            .with_channel(1);
        assert_eq!(config.port, "COM3");
        assert_eq!(config.chip_type, Some("G3".to_string()));
        assert_eq!(config.channel, Some(1));
    }

    #[test]
    fn test_hplc_error_messages() {
        let err = HplcError::init_failed("test");
        assert_eq!(err.to_string(), "驱动初始化失败: test");

        let err = HplcError::send_failed("send error");
        assert_eq!(err.to_string(), "发送失败: send error");

        let err = HplcError::recv_failed("recv error");
        assert_eq!(err.to_string(), "接收失败: recv error");

        let err = HplcError::disconnected("link down");
        assert_eq!(err.to_string(), "连接断开: link down");

        let err = HplcError::sdk_error("sdk error");
        assert_eq!(err.to_string(), "SDK 错误: sdk error");
    }

    #[test]
    fn test_modbus_crc_calculation() {
        // 已知 CRC 测试向量
        let data = vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x10];
        let crc = crc16_modbus(&data);
        assert_eq!(crc, 0xC4); // 简化的 CRC 验证
    }
}