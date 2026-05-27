//! 设备注册表接口
//!
//! 管理南向设备的注册和查询

use crate::errors::RegistryError;
use crate::types::DeviceType;
use std::sync::Arc;

/// 设备注册表接口
///
/// 提供南向设备的统一注册、注销、查询能力
pub trait DeviceRegistry: Send + Sync {
    /// 注册设备
    ///
    /// # Arguments
    /// - `device`: 设备实例
    ///
    /// # Returns
    /// - `Ok(())`: 注册成功
    /// - `Err(RegistryError)`: 注册失败（设备已存在等）
    fn register(&self, device: Arc<dyn crate::device::Device>) -> Result<(), RegistryError>;

    /// 注销设备
    ///
    /// # Arguments
    /// - `device_id`: 设备ID
    ///
    /// # Returns
    /// - `Ok(())`: 注销成功
    /// - `Err(RegistryError)`: 注销失败（设备不存在等）
    fn unregister(&self, device_id: &str) -> Result<(), RegistryError>;

    /// 获取设备
    ///
    /// # Arguments
    /// - `device_id`: 设备ID
    ///
    /// # Returns
    /// - `Some(Arc<dyn Device>)`: 设备存在
    /// - `None`: 设备不存在
    fn get(&self, device_id: &str) -> Option<Arc<dyn crate::device::Device>>;

    /// 按类型查询设备
    ///
    /// # Arguments
    /// - `device_type`: 设备类型
    ///
    /// # Returns
    /// 符合类型的设备列表
    fn query_by_type(&self, device_type: &str) -> Vec<Arc<dyn crate::device::Device>>;

    /// 列出所有设备ID
    ///
    /// # Returns
    /// 所有已注册设备的ID列表
    fn list_all(&self) -> Vec<String>;

    /// 获取设备总数
    fn count(&self) -> usize;

    /// 清除所有设备
    fn clear(&self) -> Result<(), RegistryError>;
}

/// 设备查询条件
#[derive(Debug, Clone)]
pub struct DeviceQuery {
    /// 设备类型（可选）
    pub device_type: Option<DeviceType>,
    /// 设备状态（可选）
    pub status_online: Option<bool>,
    /// 标签过滤（可选）
    pub tags: Option<Vec<String>>,
}

impl DeviceQuery {
    /// 创建新的查询条件
    pub fn new() -> Self {
        Self {
            device_type: None,
            status_online: None,
            tags: None,
        }
    }

    /// 设置设备类型
    pub fn with_device_type(mut self, device_type: DeviceType) -> Self {
        self.device_type = Some(device_type);
        self
    }

    /// 设置在线状态过滤
    pub fn with_status_online(mut self, online: bool) -> Self {
        self.status_online = Some(online);
        self
    }

    /// 设置标签过滤
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }
}

impl Default for DeviceQuery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_query_builder() {
        let query = DeviceQuery::new()
            .with_device_type(DeviceType::Ttu)
            .with_status_online(true);

        assert!(query.device_type.is_some());
        assert_eq!(query.device_type.unwrap(), DeviceType::Ttu);
        assert!(query.status_online.is_some());
        assert!(query.status_online.unwrap());
    }
}