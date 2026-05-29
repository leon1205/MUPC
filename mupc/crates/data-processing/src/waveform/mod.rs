//! 故障录波子模块
//!
//! 负责波形数据的采集、触发判定、存储、导出和北向上报。
//!
//! # 子模块
//!
//! - `ring_buffer` — 环形缓冲区（无锁单生产者单消费者）
//! - `trigger` — 触发判定引擎（过压/欠压/过流/短路/频率越限/零序过流）
//! - `sampling` — 双缓冲区管理器（保证连续故障不丢数据）
//! - `storage` — 波形文件读写（自定义 .wave 二进制格式）
//! - `export` — COMTRADE / CSV 导出
//! - `report` — 北向上报接口

pub mod ring_buffer;
pub mod trigger;
pub mod sampling;
pub mod storage;
pub mod export;
pub mod report;

// Re-export 常用类型
pub use ring_buffer::RingBuffer;
pub use trigger::{TriggerConfig, TriggerEngine, TriggerResult};
pub use sampling::DualBufferManager;
pub use storage::{WaveformMeta, WaveformReader, WaveformWriter};
pub use export::{ComtradeExporter, CsvExporter};
pub use report::WaveformReporter;
