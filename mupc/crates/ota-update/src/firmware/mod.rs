//! 固件 OTA 升级子系统
//!
//! 包含固件升级状态机、A/B 分区管理、Bootloader 环境操作、
//! .mupc 固件包格式处理、bsdiff 增量升级等核心功能。
//!
//! # 模块结构
//!
//! - `fw_state` — 17 状态固件 OTA 状态机
//! - `partition` — A/B 双分区管理器
//! - `bootloader` — U-Boot 环境变量操作
//! - `mupc_package` — .mupc 固件包格式解析与签名验证
//! - `bsdiff_applier` — bsdiff 增量补丁应用器

pub mod fw_state;
pub mod partition;
pub mod bootloader;
pub mod mupc_package;
pub mod bsdiff_applier;
