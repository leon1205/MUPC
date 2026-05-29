//! 无线通信错误类型定义
//!
//! 定义了 NearLink/WiFi/BLE 三种无线通信方式共享的错误类型。

use thiserror::Error;

/// 无线通信统一错误类型
///
/// 涵盖连接管理、数据传输、加密和配对等场景的错误。
/// 所有无线驱动 trait 方法均返回 `Result<_, WirelessError>`。
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum WirelessError {
    /// 连接建立失败，包含失败原因描述
    #[error("连接建立失败: {0}")]
    ConnectionFailed(String),

    /// 已建立的连接异常断开
    #[error("连接已断开: {0}")]
    Disconnected(String),

    /// 数据发送失败
    #[error("发送数据失败: {0}")]
    SendFailed(String),

    /// 数据接收失败
    #[error("接收数据失败: {0}")]
    RecvFailed(String),

    /// BLE 设备配对失败
    #[error("设备配对失败: {0}")]
    PairingFailed(String),

    /// ECDH 密钥交换或 AES 加解密错误
    #[error("加密错误: {0}")]
    EncryptionError(String),

    /// 不支持的设备类型或不兼容的协议版本
    #[error("不支持的设备: {0}")]
    UnsupportedDevice(String),
}

impl serde::Serialize for WirelessError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for WirelessError {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(WirelessError::ConnectionFailed(s))
    }
}
