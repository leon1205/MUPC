//! MUPC 核间通信模块
//!
//! 通过 TCP Socket 与实时控制模块通信

pub mod tcp_server;
pub mod protocol;
pub mod heartbeat;
pub mod watchdog;

pub use tcp_server::{IntercoreServer, IntercoreConfig, ControlCmdPayload, CommandConfig, CommandQueue};
pub use protocol::{IntercoreFrame, FrameType as IntercoreFrameType, FrameHeader};
pub use heartbeat::HeartbeatManager;
pub use watchdog::Watchdog;