//! IEC 61850 设备接口实现
//!
//! 实现 Iec61850Device trait，支持与 IEC 61850 IED 设备通信

use crate::config::GooseConfig;
use crate::config::Iec61850Config;
use crate::errors::Iec61850Error;
use crate::goose::GooseSubscriber;
use crate::mms_client::MmsClient;
use crate::Iec61850Status;
use async_trait::async_trait;
use device_trait::{DataFrame, DataQuality, Device, DeviceError, DeviceStatus};
use parking_lot::RwLock;
use std::sync::Arc;

/// IEC 61850 设备接口
#[async_trait]
pub trait Iec61850Device: Device {
    /// 读取数据对象（DataObject）
    async fn read_do(
        &self,
        ln: &str,
        do_name: &str,
    ) -> std::result::Result<DataFrame, Iec61850Error>;

    /// 写入数据对象
    async fn write_do(
        &self,
        ln: &str,
        do_name: &str,
        value: &[u8],
    ) -> std::result::Result<(), Iec61850Error>;

    /// 订阅 GOOSE 消息
    fn subscribe_goose(&self, go_id: &str) -> Arc<GooseSubscriber>;
}

/// IEC 61850 设备实现
pub struct Iec61850DeviceImpl {
    device_id: String,
    #[allow(dead_code)]
    config: Iec61850Config,
    mms_client: Arc<MmsClient>,
    status: Arc<RwLock<Iec61850Status>>,
    goose_subscribers: RwLock<Vec<Arc<GooseSubscriber>>>,
}

impl Iec61850DeviceImpl {
    /// 创建 IEC 61850 设备实例
    pub fn new(device_id: String, config: Iec61850Config) -> Self {
        let mms_config = crate::config::MmsConfig {
            local_ip: config.local_ip.clone(),
            local_port: config.local_port,
            remote_ip: config.remote_ip.clone(),
            remote_port: config.remote_port,
            max_connections: 10,
            connect_timeout_ms: 5000,
            read_timeout_ms: 3000,
            tls: None,
        };

        Self {
            device_id,
            config,
            mms_client: Arc::new(MmsClient::new(mms_config)),
            status: Arc::new(RwLock::new(Iec61850Status::Disconnected)),
            goose_subscribers: RwLock::new(Vec::new()),
        }
    }

    /// 连接到 IED
    pub async fn connect(&self) -> std::result::Result<(), Iec61850Error> {
        self.mms_client.connect().await?;

        let mut status = self.status.write();
        *status = Iec61850Status::Connected;

        Ok(())
    }

    /// 断开连接
    pub async fn disconnect(&self) -> std::result::Result<(), Iec61850Error> {
        self.mms_client.disconnect();

        let mut status = self.status.write();
        *status = Iec61850Status::Disconnected;

        Ok(())
    }

    /// 获取设备状态
    pub async fn get_status(&self) -> Iec61850Status {
        self.status.read().clone()
    }

    /// 获取 MMS 客户端
    pub fn mms_client(&self) -> Arc<MmsClient> {
        self.mms_client.clone()
    }
}

#[async_trait]
impl Device for Iec61850DeviceImpl {
    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn device_type(&self) -> &str {
        "IEC61850"
    }

    fn status(&self) -> std::result::Result<DeviceStatus, DeviceError> {
        // 转换状态
        let status = self.status.read();
        match &*status {
            Iec61850Status::Connected => Ok(DeviceStatus::Online),
            Iec61850Status::Disconnected => Ok(DeviceStatus::Offline),
            Iec61850Status::Error(s) => Ok(DeviceStatus::Error(s.clone())),
        }
    }

    fn read(&self) -> std::result::Result<DataFrame, DeviceError> {
        Err(DeviceError::ProtocolError(
            "IEC 61850 设备请使用 read_do 方法读取特定数据对象".to_string(),
        ))
    }

    fn write(&self, _data: &[u8]) -> std::result::Result<(), DeviceError> {
        Err(DeviceError::ProtocolError(
            "IEC 61850 设备请使用 write_do 方法写入特定数据对象".to_string(),
        ))
    }
}

#[async_trait]
impl Iec61850Device for Iec61850DeviceImpl {
    async fn read_do(
        &self,
        ln: &str,
        do_name: &str,
    ) -> std::result::Result<DataFrame, Iec61850Error> {
        let data = self
            .mms_client
            .read_do(ln, do_name)
            .await
            .map_err(|e| Iec61850Error::ProtocolError(e.to_string()))?;

        Ok(DataFrame {
            device_id: self.device_id.clone(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            data,
            quality: DataQuality::Good,
        })
    }

    async fn write_do(
        &self,
        ln: &str,
        do_name: &str,
        value: &[u8],
    ) -> std::result::Result<(), Iec61850Error> {
        self.mms_client
            .write_do(ln, do_name, value)
            .await
            .map_err(|e| Iec61850Error::ProtocolError(e.to_string()))
    }

    fn subscribe_goose(&self, go_id: &str) -> Arc<GooseSubscriber> {
        let config = GooseConfig {
            app_id: 0,
            go_id: go_id.to_string(),
            dat_set: "DataSet1".to_string(),
        };
        let (subscriber, _tx) = GooseSubscriber::new(config);

        let subscriber = Arc::new(subscriber);

        // 保存订阅者
        let mut subs = self.goose_subscribers.write();
        subs.push(subscriber.clone());

        subscriber
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_iec61850_device_creation() {
        let config = Iec61850Config::default();
        let device = Iec61850DeviceImpl::new("test_device".to_string(), config);
        assert_eq!(device.device_id(), "test_device");
        assert_eq!(device.device_type(), "IEC61850");
    }

    #[tokio::test]
    async fn test_iec61850_device_status() {
        let config = Iec61850Config::default();
        let device = Iec61850DeviceImpl::new("test_device".to_string(), config);
        let status = device.get_status().await;
        assert_eq!(status, Iec61850Status::Disconnected);
    }

    #[test]
    fn test_device_read_error() {
        let config = Iec61850Config::default();
        let device = Iec61850DeviceImpl::new("test_device".to_string(), config);
        let result = device.read();
        assert!(result.is_err());
    }

    #[test]
    fn test_device_write_error() {
        let config = Iec61850Config::default();
        let device = Iec61850DeviceImpl::new("test_device".to_string(), config);
        let result = device.write(b"test");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_goose_subscription() {
        let config = Iec61850Config::default();
        let device = Iec61850DeviceImpl::new("test_device".to_string(), config);
        let subscriber = device.subscribe_goose("GOOSE1");
        assert!(!subscriber.config().go_id.is_empty());
    }
}
