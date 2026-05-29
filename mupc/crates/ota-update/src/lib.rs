//! OTA 更新模块
//!
//! Phase 3C.2 OTA 模型自动更新模块
//! Phase 2+ 固件 OTA 升级子系统

pub mod config;
pub mod downloader;
pub mod error;
pub mod firmware;
pub mod manager;
pub mod rollback;
pub mod scheduler;
pub mod types;
pub mod verifier;
pub mod applicator;

pub use config::OtaConfig;
pub use downloader::{compute_file_hash, Downloader, DownloadResult, ProgressCallback};
pub use error::OtaError;
pub use manager::{OtaManager, OtaManagerImpl};
pub use rollback::RollbackManager;
pub use scheduler::{OtaScheduler, OtaManager as SchedulerOtaManager, SchedulerCommand};
pub use types::*;
pub use verifier::{SignatureAlgorithm, Verifier};
pub use applicator::ModelApplicator;