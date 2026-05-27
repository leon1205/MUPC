//! IEC 61850-7-420 协议插件
//!
//! 实现 IEC 61850 协议客户端，支持 MMS 读写和 GOOSE 订阅

mod config;
mod device;
mod errors;
mod goose;
mod mms_client;

pub use config::{Iec61850Config, GooseConfig, MmsConfig};
pub use device::{Iec61850DeviceImpl, Iec61850Device};
pub use errors::{Iec61850Error, Result};
pub use goose::{GooseSubscriber, GooseMessage};
pub use mms_client::MmsClient;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iec61850_status_display() {
        assert_eq!(Iec61850Status::Connected.to_string(), "Connected");
        assert_eq!(Iec61850Status::Disconnected.to_string(), "Disconnected");
        assert_eq!(Iec61850Status::Error("test".to_string()).to_string(), "Error: test");
    }
}