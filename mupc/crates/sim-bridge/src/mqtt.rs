use crate::config::{parse_broker_addr, SimBridgeConfig};
use crate::error::SimBridgeError;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use std::time::Duration;
use tokio::task::JoinHandle;

pub struct MqttPublisher {
    client: AsyncClient,
    event_loop_handle: JoinHandle<()>,
    topic: String,
    consecutive_failures: u32,
}

impl MqttPublisher {
    pub async fn connect(config: &SimBridgeConfig) -> Result<Self, SimBridgeError> {
        let (host, port) = parse_broker_addr(&config.mqtt_broker)?;
        let mut mqtt_opts = MqttOptions::new(&config.mqtt_client_id, host, port);
        mqtt_opts.set_keep_alive(Duration::from_secs(5));

        let (client, mut event_loop) = AsyncClient::new(mqtt_opts, 256);
        // rumqttc EventLoop is poll-based, spawn a polling task
        let event_loop_handle = tokio::spawn(async move {
            loop {
                match event_loop.poll().await {
                    Ok(rumqttc::Event::Incoming(_)) => {}
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!("MQTT EventLoop 错误: {}", e);
                        break;
                    }
                }
            }
        });

        tracing::info!("MQTT 已连接: {}:{}", host, port);
        Ok(Self {
            client,
            event_loop_handle,
            topic: config.mqtt_topic.clone(),
            consecutive_failures: 0,
        })
    }

    pub fn is_healthy(&self) -> bool {
        !self.event_loop_handle.is_finished()
    }

    pub async fn publish_observation(&mut self, obs: &[f32]) -> Result<(), SimBridgeError> {
        if !self.is_healthy() {
            return Err(SimBridgeError::MqttEventLoopLost);
        }
        let payload = serde_json::to_string(obs)?;
        self.client
            .publish(&self.topic, QoS::AtMostOnce, false, payload.as_bytes())
            .await
            .map_err(|e| SimBridgeError::Mqtt(e.to_string()))?;
        self.consecutive_failures = 0;
        Ok(())
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        tracing::warn!(
            "MQTT publish 失败 ({}/{})",
            self.consecutive_failures, 3
        );
    }

    pub fn should_exit(&self) -> bool {
        self.consecutive_failures >= 3
    }

    pub async fn shutdown(self) -> Result<(), SimBridgeError> {
        self.client
            .disconnect()
            .await
            .map_err(|e| SimBridgeError::Mqtt(e.to_string()))?;
        self.event_loop_handle.abort();
        Ok(())
    }
}

impl Drop for MqttPublisher {
    fn drop(&mut self) {
        // If shutdown() was not called, abort the event loop task.
        // Note: AsyncClient's Drop handles disconnect.
        self.event_loop_handle.abort();
    }
}
