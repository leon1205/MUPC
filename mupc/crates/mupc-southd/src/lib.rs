//! 站级南向统一调度（S3，02 §10）：配置驱动的多端口多从站采集调度。
//!
//! 本 crate 提供 `south_stations` 配置段类型（Role/StationConf/RegBlockConf，
//! core_config 嵌入用）与站运行时模型（配置 → 调度状态）。实际采集调度
//! （mapper / scheduler / port_runtime）由后续 Task 建立后追加声明。
pub mod config;
pub mod station;
