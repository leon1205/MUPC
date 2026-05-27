//! MUPC 北向通信网关（IEC 104）
//!
//! 实现与调度主站的 IEC 60870-5-104 协议通信

pub mod iec104;

pub use iec104::{Iec104Server, Iec104Config};