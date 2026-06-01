//! HPLC 设备实现
//!
//! 实现 SouthDevice trait，提供 HPLC 设备通信能力

use crate::config::HplcConfig;
use crate::driver::HplcDriver;
use device_trait::{DataFrame, DeviceError, DeviceStatus, SouthDevice};
use std::sync::Arc;

/// HPLC 设备
///
/// 实现南向 HPLC 设备通信，支持高速电力线载波通信
pub struct HplcDevice {
    /// 设备ID
    device_id: String,
    /// 设备类型
    device_type: String,
    /// 配置
    config: HplcConfig,
    /// HPLC 驱动
    driver: Arc<dyn HplcDriver>,
    /// 状态
    status: parking_lot::Mutex<DeviceStatus>,
}

impl HplcDevice {
    /// 创建新的 HPLC 设备
    ///
    /// # Arguments
    /// - `device_id`: 设备唯一标识
    /// - `device_type`: 设备类型（如 "hplc"）
    /// - `config`: HPLC 配置
    /// - `driver`: HPLC 驱动实例
    ///
    /// # Example
    /// ```ignore
    /// let driver = Arc::new(MockHplcDriver::new());
    /// let device = HplcDevice::new(
    ///     "hplc_001".to_string(),
    ///     "hplc".to_string(),
    ///     config,
    ///     driver,
    /// );
    /// ```
    pub fn new(
        device_id: String,
        device_type: String,
        config: HplcConfig,
        driver: Arc<dyn HplcDriver>,
    ) -> Self {
        Self {
            device_id,
            device_type,
            config,
            driver,
            status: parking_lot::Mutex::new(DeviceStatus::Offline),
        }
    }

    /// 获取设备配置引用
    pub fn config(&self) -> &HplcConfig {
        &self.config
    }

    /// 获取驱动引用
    pub fn driver(&self) -> &Arc<dyn HplcDriver> {
        &self.driver
    }
}

impl SouthDevice for HplcDevice {
    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn device_type(&self) -> &str {
        &self.device_type
    }

    fn status(&self) -> Result<DeviceStatus, DeviceError> {
        Ok(self.status.lock().clone())
    }

    fn connect(&self) -> Result<(), DeviceError> {
        self.driver
            .init(self.config.clone())
            .map_err(|e| DeviceError::Other(e.to_string()))?;

        *self.status.lock() = DeviceStatus::Online;
        tracing::info!(
            "HPLC 设备 {} 连接成功 (驱动: {})",
            self.device_id,
            self.driver.driver_name()
        );
        Ok(())
    }

    fn disconnect(&self) -> Result<(), DeviceError> {
        *self.status.lock() = DeviceStatus::Offline;
        tracing::info!("HPLC 设备 {} 断开连接", self.device_id);
        Ok(())
    }

    fn read(&self) -> Result<DataFrame, DeviceError> {
        // 检查连接状态
        if !self.driver.is_connected() {
            return Err(DeviceError::offline(&self.device_id));
        }

        // 接收数据
        let data = self
            .driver
            .recv(1000)
            .map_err(|e| DeviceError::Other(e.to_string()))?;

        if data.is_empty() {
            return Err(DeviceError::timeout("HPLC 接收超时"));
        }

        Ok(DataFrame::new(self.device_id.clone(), data))
    }

    fn read_batch(&self, count: usize) -> Result<Vec<DataFrame>, DeviceError> {
        // 检查连接状态
        if !self.driver.is_connected() {
            return Err(DeviceError::offline(&self.device_id));
        }

        let mut frames = Vec::with_capacity(count);

        for _ in 0..count {
            let data = self
                .driver
                .recv(1000)
                .map_err(|e| DeviceError::Other(e.to_string()))?;

            if !data.is_empty() {
                frames.push(DataFrame::new(self.device_id.clone(), data));
            }
        }

        Ok(frames)
    }

    fn write(&self, data: &[u8]) -> Result<(), DeviceError> {
        // 检查连接状态
        if !self.driver.is_connected() {
            return Err(DeviceError::offline(&self.device_id));
        }

        self.driver
            .send(data)
            .map_err(|e| DeviceError::Other(e.to_string()))?;

        Ok(())
    }

    fn health_check(&self) -> Result<bool, DeviceError> {
        let connected = self.driver.is_connected();
        Ok(connected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockHplcDriver;

    fn create_test_device() -> HplcDevice {
        let config = HplcConfig::new("/dev/ttyUSB0", 115200);
        let driver = Arc::new(MockHplcDriver::new());
        HplcDevice::new(
            "hplc_001".to_string(),
            "hplc".to_string(),
            config,
            driver,
        )
    }

    #[test]
    fn test_device_creation() {
        let device = create_test_device();
        assert_eq!(device.device_id(), "hplc_001");
        assert_eq!(device.device_type(), "hplc");
    }

    #[test]
    fn test_device_status_offline() {
        let device = create_test_device();
        let status = device.status().unwrap();
        assert_eq!(status, DeviceStatus::Offline);
    }

    #[test]
    fn test_device_connect() {
        let device = create_test_device();
        assert!(device.connect().is_ok());

        let status = device.status().unwrap();
        assert_eq!(status, DeviceStatus::Online);
    }

    #[test]
    fn test_device_disconnect() {
        let device = create_test_device();
        device.connect().unwrap();

        assert!(device.disconnect().is_ok());

        let status = device.status().unwrap();
        assert_eq!(status, DeviceStatus::Offline);
    }

    #[test]
    fn test_device_health_check() {
        let device = create_test_device();

        // 未连接时 health_check 应该返回 false
        assert!(!device.health_check().unwrap());

        // 连接后 health_check 应该返回 true
        device.connect().unwrap();
        assert!(device.health_check().unwrap());
    }

    #[test]
    fn test_device_write() {
        let device = create_test_device();
        device.connect().unwrap();

        // 写入数据应该成功
        assert!(device.write(&[0x01, 0x02, 0x03]).is_ok());
    }

    #[test]
    fn test_device_write_not_connected() {
        let device = create_test_device();

        // 未连接时写入应该失败
        let result = device.write(&[0x01, 0x02, 0x03]);
        assert!(result.is_err());
    }

    #[test]
    fn test_device_read_with_mock_data() {
        let device = create_test_device();
        device.connect().unwrap();

        // 获取 driver 并注入数据
        let driver = Arc::clone(device.driver());
        if let Some(mock) = driver.as_any().downcast_ref::<MockHplcDriver>() {
            mock.inject_data(vec![0xAA, 0xBB, 0xCC]);
        }

        // 读取数据
        let frame = device.read().unwrap();
        assert_eq!(frame.data, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_device_read_batch() {
        let device = create_test_device();
        device.connect().unwrap();

        // 获取 driver 并注入数据
        let driver = Arc::clone(device.driver());
        if let Some(mock) = driver.as_any().downcast_ref::<MockHplcDriver>() {
            mock.inject_data(vec![0x01]);
            mock.inject_data(vec![0x02]);
            mock.inject_data(vec![0x03]);
        }

        // 批量读取（recv 以 LIFO 顺序返回：后进先出）
        let frames = device.read_batch(3).unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].data, vec![0x03]);
        assert_eq!(frames[1].data, vec![0x02]);
        assert_eq!(frames[2].data, vec![0x01]);
    }
}