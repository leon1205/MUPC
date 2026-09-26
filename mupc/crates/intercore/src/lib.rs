//! MUPC 核间通信模块
//!
//! 通过 TCP Socket 与实时控制模块通信。
//!
//! **PCS 语义面已于 2026-09-26 迁出**：PCS 的三相读数、SOC、启停/联锁/重启授权等原语
//! 现由 `mupc-southd::pcs::PcsHandle` 承担（02 号设计 §13 / ADR-014）；对应的
//! `pcs` / `pcs_sim` / `modbus_rtu` 模块与 `transport::modbus` 一并删除。本模块现仅保留
//! **核间 TCP 通道**（帧协议 + 服务端 + 传输门面）供后续演进；该通道在生产路径暂无
//! 消费者（见实施计划 §待裁定 P-2）。

pub mod heartbeat;
pub mod protocol;
pub mod tcp_server;
pub mod transport;

pub use heartbeat::HeartbeatManager;
pub use protocol::{FrameHeader, FrameType as IntercoreFrameType, IntercoreFrame};
pub use tcp_server::{
    CommandConfig, CommandQueue, ControlCmdPayload, ControlCmdPayloadV2, ControlCmdPayloadV3,
    DualParamCommand, IntercoreClient, IntercoreConfig, IntercoreServer,
};
pub use transport::{IntercoreTransport, TcpTransport};
