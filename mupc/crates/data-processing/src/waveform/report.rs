//! 北向上报适配器
//!
//! 将故障录波事件和波形数据通过 IEC 104 和 MQTT 通道上报至北向主站。
//! 当前为 stub 实现，预留接口待后续集成 gateway 和 MQTT 模块。
//!
//! # 上报通道
//!
//! - **IEC 104**：通过文件传输（TI=122）和服务帧上报故障摘要
//! - **MQTT**：通过 QOS 1 主题 `mupc/north/fault/event` 和 `mupc/north/fault/file` 上报

use std::path::PathBuf;

/// 北向上报错误
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    #[error("网络错误: {0}")]
    NetworkError(String),
    #[error("重试耗尽: {0}")]
    RetryExhausted(String),
    #[error("文件不存在: {0}")]
    FileNotFound(String),
    #[error("协议错误: {0}")]
    ProtocolError(String),
}

/// 故障事件概要（上报用）
#[derive(Debug, Clone)]
pub struct FaultEventSummary {
    /// 事件 ID
    pub event_id: i64,
    /// 故障类型
    pub fault_type: String,
    /// 触发时间（微秒时间戳）
    pub trigger_time: i64,
    /// 触发值
    pub trigger_value: f64,
    /// 采样率
    pub sample_rate: u32,
    /// 录波时长 (ms)
    pub duration_ms: u32,
    /// 通道数量
    pub channel_count: u16,
    /// 波形文件路径
    pub waveform_path: Option<PathBuf>,
}

/// 波形波形上报器
///
/// 提供 IEC 104 和 MQTT 两种北向通道的上报接口。
/// 当前为 stub 实现（TODO 标记），待 gateway 和 MQTT 模块完成集成后对接。
pub struct WaveformReporter {
    /// 最大重试次数
    max_retries: u32,
    /// 重试间隔 (ms)
    retry_interval_ms: u64,
    /// 已上报事件 ID 集合（幂等性保证）
    reported_events: parking_lot::Mutex<Vec<i64>>,
}

impl WaveformReporter {
    /// 创建新的波形上报器
    pub fn new() -> Self {
        Self {
            max_retries: 3,
            retry_interval_ms: 30_000,
            reported_events: parking_lot::Mutex::new(Vec::new()),
        }
    }

    /// 通过 IEC 104 通道上报故障事件
    ///
    /// # 参数
    ///
    /// * `event_id` - 事件 ID
    /// * `waveform_path` - 波形文件路径（可选）
    ///
    /// # 说明
    ///
    /// TODO: 待 gateway IEC 104 模块完成文件传输（TI=122）功能后对接实现。
    /// 当前为 stub，直接返回 Ok。
    pub async fn report_via_iec104(
        &self,
        event_id: i64,
        waveform_path: Option<PathBuf>,
    ) -> Result<(), ReportError> {
        // TODO: 对接 gateway IEC 104 文件传输
        // 1. 构建故障事件 ASDU (TI=130 FaultEventReport)
        // 2. 构建故障概要 ASDU (TI=131 FaultSummaryReport)
        // 3. 如需要文件传输，发起 TI=122 文件传输流程
        //    - C_FILE_CALL → F_FILE_READY → F_FILE_SEGMENT × N → F_FILE_FINISH
        let _ = event_id;
        let _ = waveform_path;

        tracing::debug!("[IEC104 stub] 上报故障事件 event_id={}", event_id);
        self.mark_reported(event_id);
        Ok(())
    }

    /// 通过 MQTT 通道上报故障事件
    ///
    /// # 参数
    ///
    /// * `event_id` - 事件 ID
    /// * `waveform_path` - 波形文件路径（可选）
    ///
    /// # 说明
    ///
    /// TODO: 待 MQTT 模块完成 `mupc/north/fault/event` 和 `mupc/north/fault/file`
    /// 主题发布功能后对接实现。当前为 stub，直接返回 Ok。
    pub async fn report_via_mqtt(
        &self,
        event_id: i64,
        waveform_path: Option<PathBuf>,
    ) -> Result<(), ReportError> {
        // TODO: 对接 MQTT bridge 模块
        // 1. 发布事件告警到 mupc/north/fault/event (QOS 1)
        //    JSON body: { event_id, fault_type, trigger_time, trigger_value, ... }
        // 2. 如需要文件传输，发布文件分块到 mupc/north/fault/file
        //    JSON body: { event_id, file_name, total_chunks, chunk_index, data(base64), checksum_sha256 }
        let _ = event_id;
        let _ = waveform_path;

        tracing::debug!("[MQTT stub] 上报故障事件 event_id={}", event_id);
        self.mark_reported(event_id);
        Ok(())
    }

    /// 检查事件是否已上报（幂等性检查）
    pub fn is_reported(&self, event_id: i64) -> bool {
        let reported = self.reported_events.lock();
        reported.contains(&event_id)
    }

    /// 标记事件为已上报
    fn mark_reported(&self, event_id: i64) {
        let mut reported = self.reported_events.lock();
        if !reported.contains(&event_id) {
            reported.push(event_id);
            // 限制已上报集合大小，保留最近 10000 条
            if reported.len() > 10000 {
                reported.drain(0..reported.len() - 5000);
            }
        }
    }
}

impl Default for WaveformReporter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_report_via_iec104_stub() {
        let reporter = WaveformReporter::new();
        let result = reporter.report_via_iec104(1, None).await;
        assert!(result.is_ok());
        assert!(reporter.is_reported(1));
    }

    #[tokio::test]
    async fn test_report_via_mqtt_stub() {
        let reporter = WaveformReporter::new();
        let result = reporter.report_via_mqtt(2, None).await;
        assert!(result.is_ok());
        assert!(reporter.is_reported(2));
    }

    #[tokio::test]
    async fn test_idempotency() {
        let reporter = WaveformReporter::new();
        reporter.report_via_iec104(1, None).await.unwrap();
        reporter.report_via_iec104(1, None).await.unwrap();
        // 应该只记录一次
        let count = reporter.reported_events.lock().len();
        assert_eq!(count, 1);
    }
}
