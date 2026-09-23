//! IEC 104 协议模块

pub mod command;
pub mod connection;
pub mod protocol;
pub mod server;

pub use command::{CommandHandler, ControlCommand, TelemetryItem, TelemetryKind};
pub use connection::{Connection, ConnectionState};
pub use protocol::{
    decode_cp56time2a, encode_cp56time2a, encode_ic_term, encode_me_tf1, encode_sp_tb1, AsduHeader,
    Cot, FrameType, Iec104Frame, Ioa, Quality, TypeId, UFrameType, Value, COT_ACT, COT_ACT_CON,
    COT_ACT_TERM, COT_CYCLIC, COT_INTROGEN, COT_REQ, COT_SPONT,
};
pub use server::{DataClass, Iec104Config, Iec104Server, LinkState, OutboundSeq, PublishOutcome};
