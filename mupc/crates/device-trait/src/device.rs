//! 设备抽象接口
//!
//! 定义南向设备的统一抽象

use crate::errors::DeviceError;
use crate::types::{DataFrame, DeviceStatus};

/// 设备抽象接口
///
/// 所有南向设备（TTU、光伏逆变器、充电桩等）都需要实现此 trait
pub trait Device: Send + Sync {
    /// 读取设备数据
    ///
    /// # Returns
    /// - `Ok(DataFrame)`: 成功读取数据
    /// - `Err(DeviceError)`: 读取失败
    fn read(&self) -> Result<DataFrame, DeviceError>;

    /// 写入数据到设备
    ///
    /// # Arguments
    /// - `data`: 要写入的数据
    ///
    /// # Returns
    /// - `Ok(())`: 写入成功
    /// - `Err(DeviceError)`: 写入失败
    fn write(&self, data: &[u8]) -> Result<(), DeviceError>;

    /// 获取设备状态
    ///
    /// # Returns
    /// - `Ok(DeviceStatus)`: 成功获取状态
    /// - `Err(DeviceError)`: 获取失败
    fn status(&self) -> Result<DeviceStatus, DeviceError>;

    /// 获取设备唯一标识
    ///
    /// # Returns
    /// 设备ID字符串
    fn device_id(&self) -> &str;

    /// 获取设备类型
    ///
    /// # Returns
    /// 设备类型字符串
    fn device_type(&self) -> &str;
}

/// 设备命令
#[derive(Debug, Clone)]
pub enum DeviceCommand {
    /// 读取数据
    Read,
    /// 写入数据
    Write(Vec<u8>),
    /// 获取状态
    Status,
    /// 重置设备
    Reset,
    /// 自检
    SelfTest,
}

impl DeviceCommand {
    /// 判断是否为只读命令
    pub fn is_read_only(&self) -> bool {
        matches!(
            self,
            DeviceCommand::Read | DeviceCommand::Status | DeviceCommand::SelfTest
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_command_is_read_only() {
        assert!(DeviceCommand::Read.is_read_only());
        assert!(DeviceCommand::Status.is_read_only());
        assert!(DeviceCommand::SelfTest.is_read_only());
        assert!(!DeviceCommand::Write(vec![]).is_read_only());
        assert!(!DeviceCommand::Reset.is_read_only());
    }
}
