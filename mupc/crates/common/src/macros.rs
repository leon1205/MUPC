//! 公共宏定义

/// 定义模块错误
#[macro_export]
macro_rules! impl_module_error {
    ($module:ident, $err_mod:ident) => {
        pub mod $err_mod {
            use super::super::error::{define_error, ErrorCode, MupcError};

            // 模块特定错误
            define_error!(module_not_found, ErrorCode::ModuleNotFound, module_name!());
            define_error!(invalid_param, ErrorCode::InvalidParam, module_name!());
            define_error!(timeout, ErrorCode::Timeout, module_name!());
            define_error!(
                connection_failed,
                ErrorCode::ConnectionFailed,
                module_name!()
            );
        }
    };
}

/// 定义设备错误
#[macro_export]
macro_rules! impl_device_error {
    ($module:ident) => {
        pub mod device_errors {
            use super::super::error::{define_error, ErrorCode, MupcError};

            define_error!(device_offline, ErrorCode::DeviceOffline, $module);
            define_error!(device_busy, ErrorCode::DeviceBusy, $module);
            define_error!(write_failure, ErrorCode::WriteFailure, $module);
            define_error!(read_failure, ErrorCode::ReadFailure, $module);
        }
    };
}

/// 便捷的异步日志宏
#[macro_export]
macro_rules! log_error {
    ($err:expr) => {
        tracing::error!("[{}] {}", module_path!(), $err);
    };
    ($fmt:expr, $($arg:tt)*) => {
        tracing::error!(concat!("[{}] ", $fmt), module_path!(), $($arg)*);
    };
}

#[macro_export]
macro_rules! log_warn {
    ($fmt:expr, $($arg:tt)*) => {
        tracing::warn!(concat!("[{}] ", $fmt), module_path!(), $($arg)*);
    };
}

#[macro_export]
macro_rules! log_info {
    ($fmt:expr, $($arg:tt)*) => {
        tracing::info!(concat!("[{}] ", $fmt), module_path!(), $($arg)*);
    };
}

#[macro_export]
macro_rules! log_debug {
    ($fmt:expr, $($arg:tt)*) => {
        tracing::debug!(concat!("[{}] ", $fmt), module_path!(), $($arg)*);
    };
}
