//! IEC 61850-7-420 协议插件
//!
//! 实现 IEC 61850 协议客户端，支持 MMS 读写和 GOOSE 订阅

mod config;
mod device;
mod errors;
mod goose;
mod mms_client;
mod mms_types;
mod asn1_utils;

pub use config::{Iec61850Config, GooseConfig, MmsConfig, MmsTlsConfig};
pub use device::{Iec61850DeviceImpl, Iec61850Device};
pub use errors::{Iec61850Error, Result};
pub use goose::{GooseSubscriber, GooseMessage};
pub use mms_client::{MmsClient, MmsClientState, MmsClientTrait};
pub use mms_types::{MmsRequest, MmsResponse, MmsService, DataObject};

use device_trait::errors::PluginError;
use device_trait::plugin::Plugin;
use device_trait::types::PluginMeta;

/// IEC 61850 设备状态
#[derive(Debug, Clone, PartialEq)]
pub enum Iec61850Status {
    Connected,
    Disconnected,
    Error(String),
}

impl std::fmt::Display for Iec61850Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Iec61850Status::Connected => write!(f, "Connected"),
            Iec61850Status::Disconnected => write!(f, "Disconnected"),
            Iec61850Status::Error(s) => write!(f, "Error: {}", s),
        }
    }
}

// ============================================================================
// Plugin trait 实现
// ============================================================================

/// IEC 61850 插件
pub struct Iec61850Plugin {
    meta: PluginMeta,
}

impl Iec61850Plugin {
    /// 创建新的 IEC 61850 插件
    pub fn new() -> Self {
        Self {
            meta: PluginMeta::new(
                "iec61850-plugin",
                "0.1.0",
                "MUPC Team",
                "IEC 61850 MMS client plugin for substation communication",
            ),
        }
    }
}

impl Default for Iec61850Plugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for Iec61850Plugin {
    fn meta(&self) -> PluginMeta {
        self.meta.clone()
    }

    fn init(&self, _config: serde_json::Value) -> Result<(), PluginError> {
        Ok(())
    }

    fn start(&self) -> Result<(), PluginError> {
        Ok(())
    }

    fn stop(&self) -> Result<(), PluginError> {
        Ok(())
    }

    fn shutdown(self: Box<Self>) -> Result<(), PluginError> {
        Ok(())
    }
}

// ============================================================================
// FFI 入口点（用于动态加载）
// ============================================================================

/// 创建 IEC 61850 插件实例（FFI 入口点）
///
/// # Safety
/// - 必须通过 Box::from_raw 释放返回的指针
/// - 同一插件实例不能同时被多个线程使用
#[no_mangle]
pub unsafe extern "C" fn create_plugin() -> *mut dyn Plugin {
    let plugin = Iec61850Plugin::new();
    Box::into_raw(Box::new(plugin)) as *mut dyn Plugin
}

/// 获取插件元信息（FFI 入口点）
#[no_mangle]
pub unsafe extern "C" fn plugin_meta() -> PluginMeta {
    Iec61850Plugin::new().meta()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iec61850_status_display() {
        assert_eq!(Iec61850Status::Connected.to_string(), "Connected");
        assert_eq!(Iec61850Status::Disconnected.to_string(), "Disconnected");
        assert_eq!(Iec61850Status::Error("test".to_string()).to_string(), "Error: test");
    }

    #[test]
    fn test_mms_types_export() {
        let req = MmsRequest::read("LLN0", "ST$Pos");
        assert_eq!(req.service, MmsService::Read);
    }

    #[test]
    fn test_iec61850_plugin_meta() {
        let plugin = Iec61850Plugin::new();
        let meta = plugin.meta();
        assert_eq!(meta.name, "iec61850-plugin");
        assert_eq!(meta.version, "0.1.0");
    }
}