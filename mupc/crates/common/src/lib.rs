//! MUPC 公共库
//!
//! 提供日志、错误类型、宏定义等公共功能

pub mod error;
pub mod logging;
pub mod macros;

pub use error::{ErrorCode, MupcError};
pub use logging::init_logging;