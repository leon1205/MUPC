//! 站级南向统一调度（S3，02 §10）：配置驱动的多端口多从站采集调度。
//!
//! 本 crate 提供 `south_stations` 配置段类型（Role/StationConf/RegBlockConf，
//! core_config 嵌入用）、站运行时模型（配置 → 调度状态）、口级总线抽象
//! （StationBus：真实 Rs485PortBus + 测试 MockBus）与口级采集调度器
//! （scheduler：每口一条 task、口内按 next_due 串行轮询、站超时隔离/恢复、结果按
//! role 分发到 StationSink——core-bin 实现）。
pub mod config;
pub mod mapper;
pub mod port_runtime;
pub mod scheduler;
pub mod station;
