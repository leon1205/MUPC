//! MUPC 核间通信模块
//!
//! 通过 TCP Socket 与实时控制模块通信

pub mod heartbeat;
pub mod protocol;
pub mod tcp_server;
pub mod watchdog;

pub use heartbeat::HeartbeatManager;
pub use protocol::{FrameHeader, FrameType as IntercoreFrameType, IntercoreFrame};
pub use tcp_server::{
    CommandConfig, CommandQueue, ControlCmdPayload, IntercoreConfig, IntercoreServer,
};
pub use watchdog::Watchdog;
