//! 故障录波接口
//!
//! 定义故障录波器的核心 trait，支持故障记录、查询、波形获取和导出。
//!
//! # 扩展方法（P1-16）
//!
//! 新增以下方法以支持完整的故障录波工作流：
//! - `query_events` — 高级事件查询（过滤+分页）
//! - `query_events_by_type` — 按故障类型过滤
//! - `query_events_by_time` — 按时间范围过滤
//! - `get_waveform_by_id` — 按事件 ID 获取波形
//! - `get_waveform_summary` — 获取波形统计概要
//! - `export_comtrade` — 导出 COMTRADE 格式
//! - `export_csv` — 导出 CSV 格式

use async_trait::async_trait;
use mupc_common::MupcError;
use std::path::Path;

use super::telemetry::{FaultCondition, WaveformData};

/// 分页事件查询结果
#[derive(Debug, Clone)]
pub struct PaginatedEvents {
    /// 事件列表
    pub events: Vec<crate::fault_recorder_impl::FaultRecord>,
    /// 总记录数
    pub total: u64,
    /// 当前页码
    pub page: u32,
    /// 每页大小
    pub page_size: u32,
}

/// 故障事件过滤器
#[derive(Debug, Clone, Default)]
pub struct FaultEventFilter {
    /// 起始时间（微秒）
    pub start_time: Option<i64>,
    /// 结束时间（微秒）
    pub end_time: Option<i64>,
    /// 故障类型（可选）
    pub fault_type: Option<String>,
    /// 是否仅查询有波形文件的事件
    pub has_waveform: Option<bool>,
    /// 页码（1-based）
    pub page: Option<u32>,
    /// 每页大小（默认 20，最大 100）
    pub page_size: Option<u32>,
}

/// 通道统计信息
#[derive(Debug, Clone)]
pub struct ChannelStats {
    /// 通道名称
    pub channel_name: String,
    /// 最大值
    pub max: f64,
    /// 最小值
    pub min: f64,
    /// 平均值
    pub avg: f64,
    /// 有效值
    pub rms: f64,
    /// 谐波畸变率（电压通道特有）
    pub thd: Option<f64>,
}

/// 波形统计概要
#[derive(Debug, Clone)]
pub struct WaveformSummary {
    /// 事件 ID
    pub event_id: i64,
    /// 故障前通道统计
    pub pre_trigger_stats: Vec<ChannelStats>,
    /// 故障后通道统计
    pub post_trigger_stats: Vec<ChannelStats>,
    /// 触发点信息
    pub trigger_type: String,
    pub trigger_value: f64,
    pub trigger_timestamp: i64,
}

/// 导出结果
#[derive(Debug, Clone)]
pub struct ExportResult {
    /// 导出文件路径列表
    pub files: Vec<std::path::PathBuf>,
    /// 导出格式
    pub format: String,
}

/// 故障录波器 trait
#[async_trait]
pub trait FaultRecorder: Send + Sync {
    /// 记录故障事件
    async fn record(&self, event: &FaultCondition) -> Result<(), MupcError>;

    /// 查询故障记录（指定时间范围）
    async fn query(&self, start: i64, end: i64) -> Result<Vec<super::fault_recorder_impl::FaultRecord>, MupcError>;

    /// 获取波形数据
    async fn get_waveform(&self) -> Result<WaveformData, MupcError>;

    /// 检查是否正在录波
    fn is_recording(&self) -> bool;

    // === P1-16 新增方法 ===

    /// 高级事件查询（过滤+分页）
    ///
    /// 支持按故障类型、时间范围、波形存在性等多维度过滤，
    /// 返回分页结果。
    async fn query_events(&self, filter: &FaultEventFilter) -> Result<PaginatedEvents, MupcError>;

    /// 按故障类型查询事件
    async fn query_events_by_type(&self, fault_type: &str) -> Result<Vec<super::fault_recorder_impl::FaultRecord>, MupcError>;

    /// 按时间范围查询事件
    async fn query_events_by_time(&self, start: i64, end: i64) -> Result<Vec<super::fault_recorder_impl::FaultRecord>, MupcError>;

    /// 按事件 ID 获取波形数据
    async fn get_waveform_by_id(&self, event_id: i64) -> Result<WaveformData, MupcError>;

    /// 获取波形统计概要
    async fn get_waveform_summary(&self, event_id: i64) -> Result<WaveformSummary, MupcError>;

    /// 导出 COMTRADE 格式
    async fn export_comtrade(&self, event_id: i64, output_dir: &Path) -> Result<ExportResult, MupcError>;

    /// 导出 CSV 格式
    async fn export_csv(&self, event_id: i64, output_dir: &Path) -> Result<ExportResult, MupcError>;
}