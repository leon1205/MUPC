//! SSE (Server-Sent Events) 实时推送服务
//!
//! 向 Web 前端推送 AI 决策、场景切换、系统告警、遥测数据等实时事件。
//! 基于 Tokio broadcast channel 实现多客户端事件广播。

#![allow(clippy::result_large_err)]

use axum::response::sse::{Event, Sse};
use futures::stream::{Stream, StreamExt};
use mupc_ai_engine::RunningMode;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

/// SSE 事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SseEventType {
    /// AI 决策事件
    AiDecision { summary: String },
    /// 预测更新事件
    PredictionUpdate { prediction_type: String },
    /// 场景切换事件
    SceneChange { scene_name: String },
    /// 系统告警事件
    SystemAlert { level: String, message: String },
    /// 遥测数据更新事件
    TelemetryUpdate { telemetry_type: String },
}

/// SSE 事件消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SseEvent {
    pub event_id: String,
    pub event_type: SseEventType,
    pub timestamp: i64,
    pub payload: serde_json::Value,
}

/// SSE 推送服务
///
/// 基于 Tokio broadcast channel 实现多客户端事件广播。
pub struct SsePushService {
    tx: broadcast::Sender<SseEvent>,
    capacity: usize,
}

impl SsePushService {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx, capacity }
    }

    /// 订阅事件流
    pub fn subscribe(&self) -> broadcast::Receiver<SseEvent> {
        self.tx.subscribe()
    }

    /// 推送事件
    pub fn push(&self, event: SseEvent) -> Result<usize, broadcast::error::SendError<SseEvent>> {
        self.tx.send(event)
    }

    /// 推送模式切换事件
    pub fn push_mode_switch(
        &self,
        previous: RunningMode,
        current: RunningMode,
    ) -> Result<usize, broadcast::error::SendError<SseEvent>> {
        let event = SseEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: SseEventType::SceneChange {
                scene_name: current.display_name().to_string(),
            },
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: serde_json::json!({
                "event": "mode_switch",
                "previous": format!("{:?}", previous),
                "current": format!("{:?}", current),
                "display_name": current.display_name(),
            }),
        };
        self.tx.send(event)
    }

    /// 推送 AI 决策事件
    pub fn push_ai_decision(
        &self,
        summary: &str,
    ) -> Result<usize, broadcast::error::SendError<SseEvent>> {
        let event = SseEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: SseEventType::AiDecision {
                summary: summary.to_string(),
            },
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: serde_json::json!({
                "event": "ai_decision",
                "summary": summary
            }),
        };
        self.tx.send(event)
    }

    /// 推送预测更新事件
    pub fn push_prediction_update(
        &self,
        prediction_type: &str,
    ) -> Result<usize, broadcast::error::SendError<SseEvent>> {
        let event = SseEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: SseEventType::PredictionUpdate {
                prediction_type: prediction_type.to_string(),
            },
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: serde_json::json!({
                "event": "prediction_update",
                "type": prediction_type
            }),
        };
        self.tx.send(event)
    }

    /// 推送系统告警事件
    pub fn push_system_alert(
        &self,
        level: &str,
        message: &str,
    ) -> Result<usize, broadcast::error::SendError<SseEvent>> {
        let event = SseEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            event_type: SseEventType::SystemAlert {
                level: level.to_string(),
                message: message.to_string(),
            },
            timestamp: chrono::Utc::now().timestamp_millis(),
            payload: serde_json::json!({ "level": level, "message": message }),
        };
        self.tx.send(event)
    }

    /// 获取当前订阅者数量
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }

    /// 获取通道容量
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 启动后台定时推送任务
    ///
    /// Phase 2+ 集成 AiIntegrator 后，启动以下定时任务:
    /// - status: 每 5 秒推送 AI 引擎状态
    /// - predictions: 每 60 秒推送预测更新
    /// - heartbeat: 每 30 秒保活
    pub fn start_background_tasks(self: &Arc<Self>) {
        let this = Arc::clone(self);

        // 定时推送 status (每 5 秒)
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                let event = SseEvent {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    event_type: SseEventType::TelemetryUpdate {
                        telemetry_type: "heartbeat".to_string(),
                    },
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    payload: serde_json::json!({ "type": "heartbeat" }),
                };
                let _ = this.tx.send(event);
            }
        });
    }
}

/// SSE 事件流端点处理器
///
/// GET /api/v1/ai/stream
///
/// 将 SsePushService 的 broadcast 订阅转换为 Axum SSE 事件流。
/// 支持 query 参数 `types` 选择性订阅（逗号分隔：status,decision,predictions,rewards,finetuning）。
pub async fn sse_handler(
    axum::extract::State(state): axum::extract::State<Arc<crate::AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.sse_push.subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx).map(|result| {
        match result {
            Ok(event) => {
                let event_name = match &event.event_type {
                    SseEventType::SceneChange { .. } => "mode_switch",
                    SseEventType::AiDecision { .. } => "decision",
                    SseEventType::PredictionUpdate { .. } => "predictions",
                    SseEventType::SystemAlert { .. } => "alert",
                    SseEventType::TelemetryUpdate { .. } => "telemetry",
                };
                let data = serde_json::to_string(&event.payload).unwrap_or_default();
                Ok(Event::default().event(event_name).data(data))
            }
            Err(_) => {
                // broadcast 通道 lag 导致的丢弃事件，发送空注释保活
                Ok(Event::default().comment("keepalive"))
            }
        }
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("ping"),
    )
}
