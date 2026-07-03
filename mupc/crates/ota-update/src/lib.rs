//! 固件与模型 OTA 升级引擎
//!
//! NOTE: 根据 ADR-007，此 crate 对外逻辑名称为 "update-engine"，
//! 目录名保持 ota-update 以兼容现有引用。
//!
//! # 实现状态（v3.1）
//!
//! | 子系统 | 状态 | 说明 |
//! |--------|:--:|------|
//! | 模型 OTA | ✅ 可用 | 下载/校验/调度/回滚完整，~200 测试 |
//! | 固件 OTA | ❌ stub | A/B 分区切换、bsdiff 应用、bootloader 环境变量均为占位 |
//! | RKNN 模型热加载 | ❌ Phase 4 | `ModelApplicator` 热加载待 RKNN Runtime API 支持 |
//!
//! ## 固件 OTA 阻塞条件
//!
//! | 阻塞项 | 需要的支持 |
//! |--------|-----------|
//! | A/B 分区切换 | Linux bootloader (U-Boot) `boot_partition` 环境变量读写 |
//! | bsdiff 差分包应用 | 系统 `bspatch` 命令或嵌入式 bsdiff 库 |
//! | 固件包签名验证 | security crate SM2 验签集成 |
//! | OTA 服务器通信 | HTTP/HTTPS 固件元数据 API 端点 |
//!
//! 当前固件 OTA 路径的 partition 切换仅打印日志，
//! `bsdiff_applier` 为 shell-out 占位，`mupc_package` 签名验证为跳过。

pub mod applicator;
pub mod config;
pub mod downloader;
pub mod error;
pub mod firmware;
pub mod manager;
pub mod rollback;
pub mod scheduler;
pub mod types;
pub mod verifier;

pub use applicator::ModelApplicator;
pub use config::OtaConfig;
pub use downloader::{compute_file_hash, DownloadResult, Downloader, ProgressCallback};
pub use error::OtaError;
pub use manager::{OtaManager, OtaManagerImpl, UpdateStatus};
pub use rollback::RollbackManager;
pub use scheduler::{OtaManager as SchedulerOtaManager, OtaScheduler, SchedulerCommand};
pub use types::*;
pub use verifier::{SignatureAlgorithm, Verifier};
