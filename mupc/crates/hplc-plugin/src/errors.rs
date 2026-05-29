//! HPLC 驱动错误类型
//!
//! 定义 HPLC 驱动相关的错误类型

use thiserror::Error;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}