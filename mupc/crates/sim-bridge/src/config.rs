use crate::error::SimBridgeError;
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, Deserialize)]
pub struct SimBridgeConfig {
    #[serde(default = "default_scenario")]
    pub scenario: String,

    #[serde(default = "default_mqtt_broker")]
    pub mqtt_broker: String,

    #[serde(default = "default_mqtt_topic")]
    pub mqtt_topic: String,

    #[serde(default = "default_mqtt_client_id")]
    pub mqtt_client_id: String,

    #[serde(default = "default_action_listen_addr")]
    pub action_listen_addr: String,

    #[serde(default = "default_python_cmd")]
    pub python_cmd: String,

    #[serde(default = "default_engine_script")]
    pub engine_script: String,

    #[serde(default = "default_step_interval_ms")]
    pub step_interval_ms: u64,

    #[serde(default = "default_max_episode_steps")]
    pub max_episode_steps: u32,
}

fn default_scenario() -> String { "MODE-01".into() }
fn default_mqtt_broker() -> String { "192.168.3.118:1884".into() }
fn default_mqtt_topic() -> String { "mupc/sim/observation".into() }
fn default_mqtt_client_id() -> String { "mupc-sim-bridge".into() }
fn default_action_listen_addr() -> String { "0.0.0.0:9100".into() }
fn default_python_cmd() -> String { "sim-env/venv/bin/python3".into() }
fn default_engine_script() -> String { "sim-env/engine.py".into() }
fn default_step_interval_ms() -> u64 { 200 }
fn default_max_episode_steps() -> u32 { 96 }

/// Parse "host:port" from broker address string.
pub fn parse_broker_addr(addr: &str) -> Result<(&str, u16), SimBridgeError> {
    let (host, port_str) = addr.rsplit_once(':')
        .ok_or_else(|| SimBridgeError::Config(format!("Broker 地址格式错误 (需 host:port): {}", addr)))?;
    let port: u16 = port_str.parse()
        .map_err(|_| SimBridgeError::Config(format!("Broker 端口无效: {}", port_str)))?;
    Ok((host, port))
}

/// Validate environment before starting the simulation loop.
pub async fn validate_environment(config: &SimBridgeConfig) -> Result<(), SimBridgeError> {
    let output = Command::new(&config.python_cmd)
        .arg("--version")
        .output()
        .await
        .map_err(|e| SimBridgeError::Config(format!("Python 不可用 ({}): {}", config.python_cmd, e)))?;
    tracing::info!("Python: {}", String::from_utf8_lossy(&output.stdout).trim());

    let script = Path::new(&config.engine_script);
    if !script.exists() {
        return Err(SimBridgeError::Config(format!("engine.py 不存在: {}", config.engine_script)));
    }

    tracing::warn!("══════════════════════════════════════════════");
    tracing::warn!("  MQTT Broker: {}", config.mqtt_broker);
    tracing::warn!("  Topic: {}", config.mqtt_topic);
    tracing::warn!("  请确认以上地址为仿真环境 Broker (非生产)");
    tracing::warn!("══════════════════════════════════════════════");

    Ok(())
}
