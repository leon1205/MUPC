//! HPLC 驱动接口
//!
//! 定义 HplcDriver trait，抽象不同芯片厂商的 PLC SDK

use crate::config::HplcConfig;
use crate::errors::HplcError;
use std::any::Any;

/// HPLC 驱动接口
///
/// 抽象不同芯片厂商的 PLC SDK，支持高速电力线载波通信
///
/// # Example
/// ```ignore
/// use hplc_plugin::{HplcDriver, MockHplcDriver};
///
/// let driver = MockHplcDriver::new();
/// driver.init(HplcConfig::new("/dev/ttyUSB0", 115200)).unwrap();
///
/// // 发送数据
/// driver.send(&[0x01, 0x02, 0x03]).unwrap();
///
/// // 接收数据
/// let data = driver.recv(1000).unwrap();
/// ```
pub trait HplcDriver: Send + Sync {
    /// 转换为 Any 类型，用于 downcasting
    fn as_any(&self) -> &dyn Any;

    /// 初始化驱动
    ///
    /// # Arguments
    /// - `config`: HPLC 配置参数
    ///
    /// # Returns
    /// - `Ok(())`: 初始化成功
    /// - `Err(HplcError)`: 初始化失败
    fn init(&self, config: HplcConfig) -> Result<(), HplcError>;

    /// 发送数据
    ///
    /// # Arguments
    /// - `data`: 要发送的数据字节
    ///
    /// # Returns
    /// - `Ok(())`: 发送成功
    /// - `Err(HplcError)`: 发送失败
    fn send(&self, data: &[u8]) -> Result<(), HplcError>;

    /// 接收数据
    ///
    /// # Arguments
    /// - `timeout_ms`: 超时时间（毫秒）
    ///
    /// # Returns
    /// - `Ok(Vec<u8>)`: 接收到的数据
    /// - `Err(HplcError)`: 接收失败
    fn recv(&self, timeout_ms: u64) -> Result<Vec<u8>, HplcError>;

    /// 检查连接状态
    ///
    /// # Returns
    /// - `true`: 已连接
    /// - `false`: 未连接
    fn is_connected(&self) -> bool;

    /// 获取驱动名称
    ///
    /// # Returns
    /// 驱动名称字符串
    fn driver_name(&self) -> &'static str;
}
