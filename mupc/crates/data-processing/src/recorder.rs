//! 故障录波接口
//!
//! Phase 1 仅定义接口

use async_trait::async_trait;
use mupc_common::MupcError;

use super::telemetry::{FaultCondition, WaveformData};

/// 故障录波器 trait
#[async_trait]
pub trait FaultRecorder: Send + Sync {
    /// 触发录波
    async fn trigger(&self, condition: &FaultCondition) -> Result<(), MupcError>;

    /// 获取波形数据
    async fn get_waveform(&self) -> Result<WaveformData, MupcError>;

    /// 检查是否正在录波
    fn is_recording(&self) -> bool;
}