//! MQTT Topic 定义

// 本地 mosquitto Topic (进程间通信)
pub const LOCAL_TELEMETRY: &str = "mupc/local/telemetry";
pub const LOCAL_STRATEGY_COMMAND: &str = "mupc/local/strategy/command";
pub const LOCAL_AI_READY: &str = "mupc/local/ai/ready";

// 北向 emqx Topic (云端通信)
pub const NORTH_TELEMETRY: &str = "mupc/north/telemetry";
pub const NORTH_FAULT: &str = "mupc/north/fault";
pub const NORTH_STRATEGY_COMMAND: &str = "mupc/north/strategy/command";
pub const NORTH_STATUS: &str = "mupc/north/status";
