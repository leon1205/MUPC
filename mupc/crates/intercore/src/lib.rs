//! MUPC 核间通信模块
//!
//! 通过 TCP Socket 与实时控制模块通信

pub mod heartbeat;
pub mod protocol;
pub mod tcp_server;
pub mod transport;
// modbus_rtu：⚠️ 早期假设点表（自定义 cmd_ctrl/exec 确认、int32 缩放），已被 pcs 真实协议取代。
// 保留导出仅因 modbus_slave bin（仿真）与历史/旧路径测试仍引用；生产 transport=modbus_rtu 走 pcs.rs。
pub mod modbus_rtu;
pub mod pcs;
// pcs_sim：PCS 从站仿真库（自 bin/pcs_slave.rs 沉入，T-L0）——bin 薄壳与进程内 e2e
// 测试（transport::modbus 测试缝）共用；纯仿真，无 unsafe/副作用。
pub mod pcs_sim;

pub use heartbeat::HeartbeatManager;
pub use protocol::{FrameHeader, FrameType as IntercoreFrameType, IntercoreFrame};
pub use tcp_server::{
    CommandConfig, CommandQueue, ControlCmdPayload, ControlCmdPayloadV2, ControlCmdPayloadV3,
    DualParamCommand, IntercoreClient, IntercoreConfig, IntercoreServer,
};
pub use transport::{IntercoreTransport, ModbusRtuSettings, ModbusRtuTransport, TcpTransport};
