//! Mock HPLC 驱动实现
//!
//! 提供 MockHplcDriver 用于开发和测试

use crate::config::HplcConfig;
use crate::errors::HplcError;
use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Mock HPLC 驱动（用于开发测试）
///
/// 支持模拟数据注入，用于验证数据流通路
pub struct MockHplcDriver {
    connected: AtomicBool,
    mock_queue: parking_lot::Mutex<Vec<Vec<u8>>>,
    mock_delay_ms: AtomicU64,
}

impl MockHplcDriver {
    /// 创建新的 Mock HPLC 驱动
    pub fn new() -> Self {
        Self {
            connected: AtomicBool::new(false),
            mock_queue: parking_lot::Mutex::new(Vec::new()),
            mock_delay_ms: AtomicU64::new(10),
        }
    }

    /// 注入模拟数据
    ///
    /// 用于测试时注入预期接收的数据
    ///
    /// # Arguments
    /// - `data`: 要注入的数据
    pub fn inject_data(&self, data: Vec<u8>) {
        self.mock_queue.lock().push(data);
    }

    /// 设置模拟延迟（毫秒）
    pub fn set_mock_delay_ms(&self, delay_ms: u64) {
        self.mock_delay_ms.store(delay_ms, Ordering::SeqCst);
    }
}

impl Default for MockHplcDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl super::HplcDriver for MockHplcDriver {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn init(&self, _config: HplcConfig) -> Result<(), HplcError> {
        self.connected
            .store(true, std::sync::atomic::Ordering::SeqCst);
        tracing::debug!("MockHplcDriver 初始化成功");
        Ok(())
    }

    fn send(&self, data: &[u8]) -> Result<(), HplcError> {
        if !self.is_connected() {
            return Err(HplcError::disconnected("MockHplcDriver 未连接"));
        }
        tracing::debug!("MockHplcDriver 发送 {} 字节: {:?}", data.len(), data);
        Ok(())
    }

    fn recv(&self, _timeout_ms: u64) -> Result<Vec<u8>, HplcError> {
        if !self.is_connected() {
            return Err(HplcError::disconnected("MockHplcDriver 未连接"));
        }

        // 模拟延迟：使用 std::thread::sleep 而非 tokio::block_on
        // 注意：这是 Mock 实现的特点，用于模拟 I/O 延迟。生产环境的真实驱动
        // 应使用异步非阻塞方式实现 recv。
        let delay_ms = self.mock_delay_ms.load(Ordering::SeqCst);
        if delay_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        }

        // 从队列取出模拟数据
        let mut queue = self.mock_queue.lock();
        if let Some(data) = queue.pop() {
            tracing::debug!("MockHplcDriver 收到模拟数据 {} 字节", data.len());
            return Ok(data);
        }

        // 无模拟数据时返回空
        tracing::debug!("MockHplcDriver 队列为空，返回空数据");
        Ok(Vec::new())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn driver_name(&self) -> &'static str {
        "MockHplcDriver"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HplcDriver;

    #[test]
    fn test_mock_hplc_driver_init() {
        let driver = MockHplcDriver::new();
        let config = HplcConfig::new("/dev/ttyUSB0", 115200);
        assert!(driver.init(config).is_ok());
        assert!(driver.is_connected());
    }

    #[test]
    fn test_mock_hplc_driver_send() {
        let driver = MockHplcDriver::new();
        let config = HplcConfig::new("/dev/ttyUSB0", 115200);
        driver.init(config).unwrap();

        assert!(driver.send(&[0x01, 0x02, 0x03]).is_ok());
    }

    #[test]
    fn test_mock_hplc_driver_recv() {
        let driver = MockHplcDriver::new();
        let config = HplcConfig::new("/dev/ttyUSB0", 115200);
        driver.init(config).unwrap();

        // 注入测试数据
        driver.inject_data(vec![0xAA, 0xBB, 0xCC]);

        // 接收数据
        let data = driver.recv(100).unwrap();
        assert_eq!(data, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_mock_hplc_driver_recv_empty() {
        let driver = MockHplcDriver::new();
        let config = HplcConfig::new("/dev/ttyUSB0", 115200);
        driver.init(config).unwrap();

        // 不注入数据，直接接收
        let data = driver.recv(100).unwrap();
        assert_eq!(data, Vec::<u8>::new());
    }

    #[test]
    fn test_mock_hplc_driver_multiple_recv() {
        let driver = MockHplcDriver::new();
        let config = HplcConfig::new("/dev/ttyUSB0", 115200);
        driver.init(config).unwrap();

        // 注入多条数据（recv 以 LIFO 顺序返回：后进先出）
        driver.inject_data(vec![0x01]);
        driver.inject_data(vec![0x02]);
        driver.inject_data(vec![0x03]);

        assert_eq!(driver.recv(100).unwrap(), vec![0x03]);
        assert_eq!(driver.recv(100).unwrap(), vec![0x02]);
        assert_eq!(driver.recv(100).unwrap(), vec![0x01]);
    }

    #[test]
    fn test_mock_hplc_driver_not_connected() {
        let driver = MockHplcDriver::new();
        assert!(!driver.is_connected());

        let config = HplcConfig::new("/dev/ttyUSB0", 115200);
        let result = driver.init(config);
        assert!(result.is_ok());
        assert!(driver.is_connected());
    }

    #[test]
    fn test_mock_hplc_driver_name() {
        let driver = MockHplcDriver::new();
        assert_eq!(driver.driver_name(), "MockHplcDriver");
    }
}
