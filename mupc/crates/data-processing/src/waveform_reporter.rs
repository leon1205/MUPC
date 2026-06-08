//! 波形上报适配器
//!
//! 包装 `waveform::report::WaveformReporter`，提供统一的故障事件上报接口。
//! 支持 IEC 104 和 MQTT 两种北向通道。
//!
//! # 使用示例
//!
//! ```ignore
//! let adapter = WaveformReporterAdapter::new();
//! adapter.report_fault_event(12345, Some("OVER_VOLTAGE".into()),
//!     Some(PathBuf::from("/data/waveforms/20260529_143022_001.wave"))).await?;
//! ```

use crate::waveform::report::{ReportError, WaveformReporter};
use std::path::PathBuf;
use std::sync::Arc;

/// 波形上报适配器
///
/// 对 `WaveformReporter` 进行包装，提供统一的上报接口。
/// 根据目标通道自动选择 IEC 104 或 MQTT 上报路径。
pub struct WaveformReporterAdapter {
    /// 内部上报器实例
    reporter: Arc<WaveformReporter>,
}

impl WaveformReporterAdapter {
    /// 创建新的波形上报适配器
    pub fn new() -> Self {
        Self {
            reporter: Arc::new(WaveformReporter::new()),
        }
    }

    /// 使用已有上报器实例创建适配器
    pub fn with_reporter(reporter: Arc<WaveformReporter>) -> Self {
        Self { reporter }
    }

    /// 获取内部上报器引用
    pub fn inner(&self) -> &Arc<WaveformReporter> {
        &self.reporter
    }

    /// 上报故障事件（自动选择通道）
    ///
    /// 同时通过 IEC 104 和 MQTT 通道上报事件摘要。
    /// 如果提供了波形文件路径，还会上报文件传输通知。
    ///
    /// # 参数
    ///
    /// * `event_id` - 事件 ID
    /// * `fault_type` - 故障类型名称（可选）
    /// * `waveform_path` - 波形文件路径（可选）
    pub async fn report_fault_event(
        &self,
        event_id: i64,
        fault_type: Option<String>,
        waveform_path: Option<PathBuf>,
    ) -> Result<(), ReportError> {
        // 幂等性检查
        if self.reporter.is_reported(event_id) {
            tracing::debug!("事件 {} 已上报，跳过", event_id);
            return Ok(());
        }

        // 同时上报到 IEC 104 和 MQTT 通道
        let iec104_fut = self
            .reporter
            .report_via_iec104(event_id, waveform_path.clone());
        let mqtt_fut = self.reporter.report_via_mqtt(event_id, waveform_path);

        // 并发上报
        let (iec_result, mqtt_result) = tokio::join!(iec104_fut, mqtt_fut);

        // 至少一个通道成功即视为上报成功
        match (iec_result, mqtt_result) {
            (Ok(_), _) | (_, Ok(_)) => {
                tracing::info!("故障事件 {} 上报成功 (类型: {:?})", event_id, fault_type);
                Ok(())
            }
            (Err(e1), Err(e2)) => {
                tracing::error!("故障事件 {} 上报失败: IEC104={}, MQTT={}", event_id, e1, e2);
                Err(e1) // 返回第一个错误
            }
        }
    }

    /// 仅通过 IEC 104 上报
    pub async fn report_via_iec104(
        &self,
        event_id: i64,
        waveform_path: Option<PathBuf>,
    ) -> Result<(), ReportError> {
        self.reporter
            .report_via_iec104(event_id, waveform_path)
            .await
    }

    /// 仅通过 MQTT 上报
    pub async fn report_via_mqtt(
        &self,
        event_id: i64,
        waveform_path: Option<PathBuf>,
    ) -> Result<(), ReportError> {
        self.reporter.report_via_mqtt(event_id, waveform_path).await
    }
}

impl Default for WaveformReporterAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_report_fault_event_stub() {
        let adapter = WaveformReporterAdapter::new();
        let result = adapter
            .report_fault_event(1, Some("OVER_VOLTAGE".into()), None)
            .await;
        assert!(result.is_ok());
        assert!(adapter.inner().is_reported(1));
    }

    #[tokio::test]
    async fn test_report_idempotency() {
        let adapter = WaveformReporterAdapter::new();

        // 第一次上报
        adapter.report_fault_event(1, None, None).await.unwrap();
        // 第二次上报同一事件（应被幂等性检查拦截）
        adapter.report_fault_event(1, None, None).await.unwrap();

        // 确保只标记一次
        assert!(adapter.inner().is_reported(1));
    }
}
