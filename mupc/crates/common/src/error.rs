//! 统一错误类型
//!
//! 实现 std::error::Error trait，支持错误链

use thiserror::Error;

/// 错误码定义
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u16)]
pub enum ErrorCode {
    // 通用错误 (0x0000-0x00FF)
    Ok = 0x0000,
    Unknown = 0x0001,
    InvalidParam = 0x0002,
    NotFound = 0x0003,
    Timeout = 0x0004,
    ConnectionFailed = 0x0005,
    IoError = 0x0006,
    ParseError = 0x0007,
    SerializeError = 0x0008,

    // 网关错误 (0x0100-0x01FF)
    ModuleNotFound = 0x0100,
    FrameParseError = 0x0101,
    AsduTypeMismatch = 0x0102,
    ProtocolError = 0x0103,
    ConnectionClosed = 0x0104,

    // 核间通信错误 (0x0200-0x02FF)
    IntercoreTimeout = 0x0200,
    HeartbeatMissed = 0x0201,
    FrameChecksumError = 0x0202,
    InvalidFrame = 0x0203,
    SendFailed = 0x0204,

    // 设备错误 (0x0300-0x03FF)
    DeviceOffline = 0x0300,
    DeviceBusy = 0x0301,
    WriteFailure = 0x0302,
    ReadFailure = 0x0303,

    // Web API 错误 (0x0400-0x04FF)
    AuthFailed = 0x0400,
    InvalidSession = 0x0401,
    ConfigError = 0x0402,

    // 策略引擎错误 (0x0500-0x05FF)
    StrategyError = 0x0500,
    ValidationFailed = 0x0501,
}

impl ErrorCode {
    /// 从 u16 值创建 ErrorCode
    pub fn from_u16(val: u16) -> Option<Self> {
        match val {
            0x0000 => Some(ErrorCode::Ok),
            0x0001 => Some(ErrorCode::Unknown),
            0x0002 => Some(ErrorCode::InvalidParam),
            0x0003 => Some(ErrorCode::NotFound),
            0x0004 => Some(ErrorCode::Timeout),
            0x0005 => Some(ErrorCode::ConnectionFailed),
            0x0006 => Some(ErrorCode::IoError),
            0x0007 => Some(ErrorCode::ParseError),
            0x0008 => Some(ErrorCode::SerializeError),
            0x0100 => Some(ErrorCode::ModuleNotFound),
            0x0101 => Some(ErrorCode::FrameParseError),
            0x0102 => Some(ErrorCode::AsduTypeMismatch),
            0x0103 => Some(ErrorCode::ProtocolError),
            0x0104 => Some(ErrorCode::ConnectionClosed),
            0x0200 => Some(ErrorCode::IntercoreTimeout),
            0x0201 => Some(ErrorCode::HeartbeatMissed),
            0x0202 => Some(ErrorCode::FrameChecksumError),
            0x0203 => Some(ErrorCode::InvalidFrame),
            0x0204 => Some(ErrorCode::SendFailed),
            0x0300 => Some(ErrorCode::DeviceOffline),
            0x0301 => Some(ErrorCode::DeviceBusy),
            0x0302 => Some(ErrorCode::WriteFailure),
            0x0303 => Some(ErrorCode::ReadFailure),
            0x0400 => Some(ErrorCode::AuthFailed),
            0x0401 => Some(ErrorCode::InvalidSession),
            0x0402 => Some(ErrorCode::ConfigError),
            0x0500 => Some(ErrorCode::StrategyError),
            0x0501 => Some(ErrorCode::ValidationFailed),
            _ => None,
        }
    }

    /// 获取错误码的描述
    pub fn description(&self) -> &'static str {
        match self {
            ErrorCode::Ok => "Success",
            ErrorCode::Unknown => "Unknown error",
            ErrorCode::InvalidParam => "Invalid parameter",
            ErrorCode::NotFound => "Resource not found",
            ErrorCode::Timeout => "Operation timeout",
            ErrorCode::ConnectionFailed => "Connection failed",
            ErrorCode::IoError => "I/O error",
            ErrorCode::ParseError => "Parse error",
            ErrorCode::SerializeError => "Serialize error",
            ErrorCode::ModuleNotFound => "Module not found",
            ErrorCode::FrameParseError => "Frame parse error",
            ErrorCode::AsduTypeMismatch => "ASDU type mismatch",
            ErrorCode::ProtocolError => "Protocol error",
            ErrorCode::ConnectionClosed => "Connection closed",
            ErrorCode::IntercoreTimeout => "Intercore communication timeout",
            ErrorCode::HeartbeatMissed => "Heartbeat missed",
            ErrorCode::FrameChecksumError => "Frame checksum error",
            ErrorCode::InvalidFrame => "Invalid frame",
            ErrorCode::SendFailed => "Send failed",
            ErrorCode::DeviceOffline => "Device offline",
            ErrorCode::DeviceBusy => "Device busy",
            ErrorCode::WriteFailure => "Write failure",
            ErrorCode::ReadFailure => "Read failure",
            ErrorCode::AuthFailed => "Authentication failed",
            ErrorCode::InvalidSession => "Invalid session",
            ErrorCode::ConfigError => "Configuration error",
            ErrorCode::StrategyError => "Strategy error",
            ErrorCode::ValidationFailed => "Validation failed",
        }
    }
}

/// 统一错误类型
#[derive(Debug)]
pub struct MupcError {
    /// 错误码
    pub code: ErrorCode,
    /// 错误描述
    pub message: String,
    /// 错误来源模块
    pub module: &'static str,
    /// 错误来源（支持错误链）
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl MupcError {
    /// 创建新的错误
    pub fn new(code: ErrorCode, message: impl Into<String>, module: &'static str) -> Self {
        Self {
            code,
            message: message.into(),
            module,
            source: None,
        }
    }

    /// 从其他错误创建
    pub fn with_source<E>(mut self, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        self.source = Some(Box::new(source));
        self
    }

    /// 获取错误码值
    pub fn code_value(&self) -> u16 {
        self.code as u16
    }
}

impl std::fmt::Display for MupcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{:#06x}] {} (module: {})",
            self.code as u16, self.message, self.module
        )
    }
}

impl std::error::Error for MupcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e.as_ref() as _)
    }
}

// 便捷错误构造宏
#[macro_export]
macro_rules! define_error {
    ($name:ident, $code:expr, $module:expr) => {
        /// 创建错误的便捷函数
        pub fn $name(msg: impl Into<String>) -> MupcError {
            MupcError::new($code, msg, $module)
        }
    };
}

// 预定义错误构造函数
mod predefine {
    use super::*;

    // 通用错误
    define_error!(unknown_error, ErrorCode::Unknown, "common");
    define_error!(invalid_param, ErrorCode::InvalidParam, "common");
    define_error!(not_found, ErrorCode::NotFound, "common");
    define_error!(timeout_error, ErrorCode::Timeout, "common");
    define_error!(connection_failed, ErrorCode::ConnectionFailed, "common");
    define_error!(io_error, ErrorCode::IoError, "common");
    define_error!(parse_error, ErrorCode::ParseError, "common");
    define_error!(serialize_error, ErrorCode::SerializeError, "common");
}

pub use predefine::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_from_u16() {
        assert_eq!(ErrorCode::from_u16(0x0001), Some(ErrorCode::Unknown));
        assert_eq!(ErrorCode::from_u16(0x0101), Some(ErrorCode::FrameParseError));
        assert_eq!(ErrorCode::from_u16(0xFFFF), None);
    }

    #[test]
    fn test_error_code_description() {
        assert_eq!(ErrorCode::Unknown.description(), "Unknown error");
        assert_eq!(ErrorCode::FrameParseError.description(), "Frame parse error");
        assert_eq!(ErrorCode::AuthFailed.description(), "Authentication failed");
    }

    #[test]
    fn test_mupc_error_new() {
        let err = MupcError::new(ErrorCode::NotFound, "Resource not found", "test-module");
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(err.message, "Resource not found");
        assert_eq!(err.module, "test-module");
        assert_eq!(err.code_value(), 0x0003);
    }

    #[test]
    fn test_mupc_error_display() {
        let err = MupcError::new(ErrorCode::NotFound, "Resource not found", "test-module");
        let display = format!("{}", err);
        assert!(display.contains("0x0003"));
        assert!(display.contains("Resource not found"));
        assert!(display.contains("test-module"));
    }

    #[test]
    fn test_mupc_error_source() {
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "inner error");
        let err = MupcError::new(ErrorCode::IoError, "outer error", "test-module")
            .with_source(inner);
        let source = err.source();
        assert!(source.is_some());
    }

    #[test]
    fn test_mupc_error_error_trait() {
        let err = MupcError::new(ErrorCode::NotFound, "Resource not found", "test-module");
        // 验证实现了 std::error::Error trait
        let _dyn_err: Box<dyn std::error::Error> = Box::new(err);
    }

    #[test]
    fn test_error_chain() {
        let inner = std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout");
        let err = MupcError::new(ErrorCode::Timeout, "operation timed out", "test-module")
            .with_source(inner);

        // 验证错误链
        assert!(err.source().is_some());
        let mut current: Option<&(dyn std::error::Error + 'static)> = Some(&err);
        let mut found_inner = false;
        while let Some(e) = current {
            if e.to_string().contains("timeout") {
                found_inner = true;
            }
            current = e.source();
        }
        assert!(found_inner);
    }

    #[test]
    fn test_error_code_value() {
        assert_eq!(ErrorCode::Ok as u16, 0x0000);
        assert_eq!(ErrorCode::Unknown as u16, 0x0001);
        assert_eq!(ErrorCode::FrameParseError as u16, 0x0101);
        assert_eq!(ErrorCode::FrameChecksumError as u16, 0x0202);
        assert_eq!(ErrorCode::AuthFailed as u16, 0x0400);
    }

    #[test]
    fn test_predefined_errors() {
        let err = unknown_error("something went wrong");
        assert_eq!(err.code, ErrorCode::Unknown);
        assert_eq!(err.module, "common");

        let err = invalid_param("bad parameter");
        assert_eq!(err.code, ErrorCode::InvalidParam);
    }
}