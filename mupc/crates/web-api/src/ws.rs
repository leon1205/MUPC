//! WebSocket 日志推送

use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::Response,
    routing::get,
    Router,
};
use tokio::sync::broadcast;
use tracing::{info, error};

/// 日志消息
#[derive(Debug, Clone)]
pub struct LogMessage {
    pub level: String,
    pub timestamp: String,
    pub module: String,
    pub message: String,
}

/// WebSocket 日志流管理器
#[derive(Clone)]
pub struct WsLogStreamer {
    tx: broadcast::Sender<LogMessage>,
}

impl WsLogStreamer {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self { tx }
    }

    /// 发送日志消息
    pub fn send(&self, msg: LogMessage) {
        let _ = self.tx.send(msg);
    }

    /// 订阅日志消息
    pub fn subscribe(&self) -> broadcast::Receiver<LogMessage> {
        self.tx.subscribe()
    }
}

impl Default for WsLogStreamer {
    fn default() -> Self {
        Self::new()
    }
}

/// GET /ws/logs - WebSocket 日志流
async fn ws_logs(
    ws: WebSocketUpgrade,
    State(streamer): State<WsLogStreamer>,
) -> Response {
    ws.on_upgrade(|socket| async move {
        let mut rx = streamer.subscribe();

        let (mut sender, mut receiver) = socket.split();

        // 接收消息
        let _ = receiver.recv().await;

        // 发送日志
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let json = serde_json::json!({
                        "level": msg.level,
                        "timestamp": msg.timestamp,
                        "module": msg.module,
                        "message": msg.message,
                    });

                    if sender.send(Message::Text(json.to_string())).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    // Channel closed
                    break;
                }
            }
        }

        info!("WebSocket log stream closed");
    })
}

/// 创建 WebSocket 路由
pub fn create_router(streamer: WsLogStreamer) -> Router {
    Router::new()
        .route("/ws/logs", get(ws_logs))
        .with_state(streamer)
}