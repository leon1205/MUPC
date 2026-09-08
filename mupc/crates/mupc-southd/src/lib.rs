//! 站级南向统一调度（S3，02 §10）：配置驱动的多端口多从站采集调度。
//!
//! 本 crate 提供 `south_stations` 配置段类型（Role/StationConf/RegBlockConf，
//! core_config 嵌入用）、站运行时模型（配置 → 调度状态）与口级总线抽象
//! （StationBus：真实 Rs485PortBus + 测试 MockBus）。采集调度逻辑
//! （mapper / scheduler）由后续 Task 建立后追加声明。
pub mod config;
pub mod port_runtime;
pub mod station;
