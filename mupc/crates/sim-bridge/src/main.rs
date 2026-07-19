mod action_server;
mod config;
mod error;
mod metrics;
mod mqtt;
mod py_engine;
mod scenario;

use action_server::{read_frame_with_timeout, ActionServer, ReadError, ACTION_READ_TIMEOUT};
use clap::Parser;
use config::{validate_environment, SimBridgeConfig};
use metrics::MetricsCollector;
use mqtt::MqttPublisher;
use py_engine::{PyEngine, SimRequest, SimResponse};
use scenario::validate_scenario;
use std::path::PathBuf;
use tokio::net::TcpStream;

#[derive(Parser)]
#[command(name = "mupc-sim-bridge")]
#[command(about = "MUPC HIL 仿真桥接代理")]
struct Cli {
    #[arg(short, long, default_value = "config/sim_config.yaml")]
    config: PathBuf,

    #[arg(short, long, help = "场景 MODE-01 ~ MODE-05")]
    scenario: Option<String>,

    #[arg(long, help = "启用 Grid2Op (默认使用 VoltageSimulator)")]
    grid2op: bool,

    #[arg(long, help = "MQTT Broker 地址 (覆盖配置文件)")]
    broker: Option<String>,

    #[arg(long, help = "动作监听地址 (覆盖配置文件)")]
    listen: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mupc_sim_bridge=info".into()),
        )
        .init();

    let cli = Cli::parse();

    // Load config
    let config_path = cli.config;
    let config_str = std::fs::read_to_string(&config_path).unwrap_or_else(|_| {
        tracing::warn!("配置文件 {} 不存在，使用默认配置", config_path.display());
        "{}".to_string()
    });
    let mut config: SimBridgeConfig =
        serde_yaml::from_str(&config_str).unwrap_or_else(|e| {
            tracing::warn!("配置解析失败: {}，使用默认值", e);
            serde_yaml::from_str("{}").unwrap()
        });

    // CLI overrides
    if let Some(ref s) = cli.scenario {
        config.scenario = s.clone();
    }
    if let Some(ref b) = cli.broker {
        config.mqtt_broker = b.clone();
    }
    if let Some(ref l) = cli.listen {
        config.action_listen_addr = l.clone();
    }

    // Validate
    if let Err(e) = validate_scenario(&config.scenario) {
        tracing::error!("{}", e);
        std::process::exit(1);
    }
    if let Err(e) = validate_environment(&config).await {
        tracing::error!("环境验证失败: {}", e);
        std::process::exit(1);
    }

    tracing::info!("场景: {}", config.scenario);
    tracing::info!("Grid2Op: {}", if cli.grid2op { "启用" } else { "禁用 (VoltageSimulator)" });

    // Initialize components
    let mut mqtt = MqttPublisher::connect(&config).await.unwrap_or_else(|e| {
        tracing::error!("MQTT 连接失败: {}", e);
        std::process::exit(1);
    });

    let action_srv = ActionServer::bind(&config.action_listen_addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("TCP 绑定失败: {}", e);
            std::process::exit(1);
        });

    let mut engine = PyEngine::spawn(&config).await.unwrap_or_else(|e| {
        tracing::error!("Python 引擎启动失败: {}", e);
        std::process::exit(1);
    });

    let mut metrics = MetricsCollector::new(&config.scenario);

    // Initial reset and publish
    let initial_obs = engine.send_reset(&config.scenario).await.unwrap_or_else(|e| {
        tracing::error!("初始 reset 失败: {}", e);
        std::process::exit(1);
    });
    let initial_data = match &initial_obs {
        SimResponse::Observation { data, .. } => data.clone(),
        _ => {
            tracing::error!("初始 reset 返回非 Observation: {:?}", initial_obs);
            std::process::exit(1);
        }
    };
    mqtt.publish_observation(&initial_data).await.unwrap_or_else(|e| {
        tracing::error!("初始 MQTT 发布失败: {}", e);
    });

    let mut current_obs = initial_data.clone();

    // Accept MUPC connection
    let (mut stream, addr) = action_srv.accept().await.unwrap_or_else(|e| {
        tracing::error!("等待 MUPC 连接失败: {}", e);
        std::process::exit(1);
    });
    tracing::info!("MUPC 已连接: {}", addr);

    // Main simulation loop
    loop {
        tokio::select! {
            result = read_frame_with_timeout(&mut stream, ACTION_READ_TIMEOUT) => {
                match result {
                    Ok(frame) => {
                        let t0 = std::time::Instant::now();

                        match engine.send_step(frame.p_ref, frame.k_droop).await {
                            Ok(SimResponse::Observation { data, reward, done, info }) => {
                                current_obs = data.clone();
                                let latency = t0.elapsed().as_millis() as u64;

                                // Publish with failure counting (PRD EH-01)
                                if let Err(e) = mqtt.publish_observation(&data).await {
                                    mqtt.record_failure();
                                    tracing::warn!("MQTT publish 失败: {}", e);
                                    if mqtt.should_exit() {
                                        tracing::error!("MQTT 连续3次失败，退出");
                                        break;
                                    }
                                }

                                metrics.record_step(latency, reward, &info);

                                if done {
                                    tracing::info!("Episode 完成, 重置中...");
                                    match engine.send_reset(&config.scenario).await {
                                        Ok(SimResponse::Observation { data, .. }) => {
                                            if let Err(e) = mqtt.publish_observation(&data).await {
                                                mqtt.record_failure();
                                                tracing::warn!("reset 后 MQTT publish 失败: {}", e);
                                                if mqtt.should_exit() {
                                                    tracing::error!("MQTT 连续3次失败，退出");
                                                    break;
                                                }
                                            }
                                            current_obs = data;
                                        }
                                        Ok(other) => {
                                            tracing::warn!("reset 返回非预期响应: {:?}", other);
                                        }
                                        Err(e) => {
                                            tracing::error!("reset 失败: {}", e);
                                        }
                                    }
                                    metrics.reset_episode(&config.scenario);
                                }

                                // Check MQTT health
                                if !mqtt.is_healthy() {
                                    tracing::error!("MQTT EventLoop 异常退出");
                                    break;
                                }
                            }
                            Ok(SimResponse::Error { msg }) => {
                                tracing::warn!("engine.py 返回错误: {}", msg);
                            }
                            Ok(_) => {
                                tracing::warn!("engine.py 返回非预期消息类型");
                            }
                            Err(e) => {
                                tracing::error!("send_step 失败: {}", e);
                                if let Err(re) = engine.restart(&config).await {
                                    tracing::error!("引擎重启失败: {}", re);
                                    break;
                                }
                            }
                        }
                    }
                    Err(ReadError::TimeoutElapsed) => {
                        tracing::warn!("等待 MUPC 动作超时 (>30s)，重新发布当前观测");
                        let _ = mqtt.publish_observation(&current_obs).await;
                    }
                    Err(ReadError::ConnectionLost) => {
                        tracing::warn!("MUPC 连接断开，等待重连...");
                        match action_srv.accept().await {
                            Ok((new_stream, new_addr)) => {
                                stream = new_stream;
                                tracing::info!("MUPC 重连: {}", new_addr);
                            }
                            Err(e) => {
                                tracing::error!("重连失败: {}", e);
                            }
                        }
                    }
                    Err(ReadError::CrcMismatch) => {
                        tracing::warn!("CRC 校验失败，丢弃帧");
                    }
                    Err(ReadError::Protocol(e)) => {
                        tracing::warn!("协议错误: {}", e);
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("收到 SIGINT，优雅退出...");
                if let Err(e) = engine.send_shutdown().await {
                    tracing::warn!("Python shutdown 失败: {}", e);
                }
                if let Err(e) = mqtt.shutdown().await {
                    tracing::warn!("MQTT shutdown 失败: {}", e);
                }
                if let Err(e) = metrics.export("sim_metrics.json") {
                    tracing::error!("指标导出失败: {}", e);
                }
                break;
            }
        }
    }

    tracing::info!("sim-bridge 已退出");
}
