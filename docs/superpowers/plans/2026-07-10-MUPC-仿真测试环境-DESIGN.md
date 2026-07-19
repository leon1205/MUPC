# MUPC 仿真测试环境 — 技术设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.3 | 2026-07-10 | 架构师 | `[DESIGN_APPROVED]` — 四轮审查全通过 |

> **关联 PRD**：`docs/superpowers/specs/modules/11-MUPC-仿真测试环境-PRD.md` `[REVIEWED: PASS]`

---

## 1. 技术选型

| 组件 | 选型 | 理由 |
|------|------|------|
| 仿真代理语言 | Rust (Tokio async) | 与 MUPC workspace 统一, 零成本 FFI |
| MQTT Client | `rumqttc` 0.24 | 纯 Rust, Tokio 兼容, 无 C 依赖 |
| TCP Server | `tokio::net::TcpListener` | 标准库, 异步非阻塞 |
| Python 子进程通信 | stdin/stdout JSONL + `tokio::process` | 零网络开销, 单机通信最简方案 |
| 电网仿真引擎 | Grid2Op + Pandapower (从 MUPC-AI2 复用) | 已有三相潮流代码, 无需重写 |
| 配置格式 | YAML (`serde_yaml`) | 与 MUPC 现有配置一致 |
| 日志 | `tracing` + `tracing-subscriber` | 与 MUPC 统一 |
| CLI | `clap` 4.0 derive | 标准 Rust CLI |

**未选用方案**：
- HTTP/gRPC 替代 JSONL pipe：增加网络栈，单机通信无需
- `paho-mqtt` C client：需要 C 编译依赖，交叉编译复杂
- 纯 Python 代理：无法复用 Rust 的 TCP 帧解析

---

## 2. 模块划分

### 2.1 总览

```
mupc/crates/sim-bridge/
├── Cargo.toml
└── src/
    ├── main.rs          # 入口: CLI解析 → 组件初始化 → 主循环
    ├── config.rs        # YAML 配置结构体 (SimBridgeConfig)
    ├── mqtt.rs          # MQTT 发布器 (rumqttc async)
    ├── action_server.rs # TCP 动作服务器 (tokio TcpListener)
    ├── py_engine.rs     # Python 子进程管理 + JSONL 编解码
    ├── scenario.rs      # 场景管理: 5 场景参数映射
    ├── metrics.rs       # Episode 指标收集 + JSON 导出
    └── error.rs         # 统一错误类型

sim-env/
├── engine.py           # JSONL stdin/stdout 主循环
├── mupc_env/           # 从 MUPC-AI2 复制核心文件
│   ├── core.py         # MupcEnv 主类
│   ├── observation.py  # 78维观测构建
│   ├── rewards.py      # 5 场景奖励
│   ├── constants.py    # 物理常数 + 归一化
│   ├── voltage_sim.py  # VoltageSimulator 降级
│   └── grid2op/        # Grid2Op 引擎封装
└── requirements.txt
```

### 2.2 模块职责

| 模块 | 职责 | 依赖 |
|------|------|------|
| `main.rs` | CLI 解析, 组件装配, select! 主循环, 信号处理 | 全部 |
| `config.rs` | 加载 `sim_config.yaml` → `SimBridgeConfig`, 命令行覆盖 | serde_yaml, clap |
| `mqtt.rs` | 异步连接 Broker, QoS 0 publish 78维观测 | rumqttc |
| `action_server.rs` | 绑定 TCP 9100, 接收 + 解析 intercore ControlCommand 帧 | tokio |
| `py_engine.rs` | spawn Python 子进程, JSONL 编解码, 超时保护, 崩溃重试 | tokio::process |
| `scenario.rs` | 5 场景参数常量, mode_id 映射, scenario→MupcEnv 初始化参数 | — |
| `metrics.rs` | 每步记录 (延迟/奖励/违规), episode 结束导出 JSON | serde_json |
| `error.rs` | `SimBridgeError` 枚举: Mqtt/Tcp/PyEngine/Config/Protocol | thiserror |

---

## 3. 核心数据结构

### 3.1 配置 — `SimBridgeConfig`

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct SimBridgeConfig {
    pub scenario: String,              // MODE-01 ~ MODE-05
    pub mqtt_broker: String,           // 仿真 Broker: 192.168.3.118:1884 (独立端口)
    pub mqtt_topic: String,            // "mupc/sim/observation"
    #[serde(default = "default_client_id")]
    pub mqtt_client_id: String,        // "mupc-sim-bridge"
    pub action_listen_addr: String,    // "0.0.0.0:9100"
    pub python_cmd: String,            // "sim-env/venv/bin/python3"
    pub engine_script: String,         // "sim-env/engine.py"
    #[serde(default = "default_step_interval_ms")]
    pub step_interval_ms: u64,         // 200
    #[serde(default = "default_max_episode_steps")]
    pub max_episode_steps: u32,        // 96
}

/// 启动时验证关键路径和连接
pub async fn validate_environment(config: &SimBridgeConfig) -> Result<()> {
    // 1. 验证 Python 解释器路径
    let output = Command::new(&config.python_cmd).arg("--version").output().await
        .map_err(|e| SimBridgeError::Config(format!("Python 解释器不可用 ({}): {}", config.python_cmd, e)))?;
    tracing::info!("Python: {}", String::from_utf8_lossy(&output.stdout).trim());

    // 2. 验证 engine.py 存在
    let script = Path::new(&config.engine_script);
    if !script.exists() {
        return Err(SimBridgeError::Config(format!("engine.py 不存在: {}", config.engine_script)));
    }

    // 3. Broker 安全确认
    tracing::warn!("══════════════════════════════════════════════");
    tracing::warn!("  MQTT Broker: {}", config.mqtt_broker);
    tracing::warn!("  Topic: {}", config.mqtt_topic);
    tracing::warn!("  请确认以上地址为仿真环境 Broker (非生产)");
    tracing::warn!("══════════════════════════════════════════════");

    Ok(())
}
```

### 3.2 消息 — `SimRequest` / `SimResponse`

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum SimRequest {
    #[serde(rename = "reset")]
    Reset { scenario: String },
    #[serde(rename = "step")]
    Step { p_ref: f64, k_droop: f64 },
    #[serde(rename = "shutdown")]
    Shutdown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum SimResponse {
    #[serde(rename = "obs")]
    Observation {
        data: Vec<f32>,       // 78 elements
        reward: f64,
        done: bool,
        info: serde_json::Value,
    },
    #[serde(rename = "shutdown_ack")]
    ShutdownAck,
}
```

### 3.3 动作帧 — `ActionFrame` (TCP 二进制)

```rust
pub const ACTION_FRAME_LEN: usize = 26;

#[derive(Debug, Clone)]
pub struct ActionFrame {
    pub frame_id: u32,        // bytes 0..4, BE
    pub cmd_type: u8,         // byte 4, 0x01 = control
    pub reserved: u8,         // byte 5, 0x00
    pub payload_len: u16,     // bytes 6..8, BE, =16
    pub p_ref: f64,           // bytes 8..16, BE, IEEE754
    pub k_droop: f64,         // bytes 16..24, BE, IEEE754
    pub crc16: u16,           // bytes 24..26, BE, CRC-16/MODBUS
}
```

### 3.4 Episode 指标 — `EpisodeMetrics`

```rust
#[derive(Debug, Clone, Serialize)]
pub struct EpisodeMetrics {
    pub scenario: String,
    pub start_time: String,            // ISO 8601
    pub end_time: String,
    pub total_steps: u64,
    pub total_reward: f64,
    pub avg_step_latency_ms: f64,
    pub min_step_latency_ms: f64,
    pub max_step_latency_ms: f64,
    pub p99_step_latency_ms: f64,
    pub safety_override_count: u32,
    pub soc_violations: u32,
    pub voltage_violations: u32,
}
```

---

## 4. 模块详细设计

### 4.1 `main.rs` — 主循环

```
main():
  1. parse_cli() → (config_path, scenario_override)
  2. load_config(config_path) → SimBridgeConfig
  3. validate_python_path(&config).await?
  4. verify_broker_is_simulation(&config)?  // 打印 Broker 地址确认
  5. mqtt = MqttPublisher::connect(&config).await?
  6. action_srv = ActionServer::bind(&config.action_listen_addr).await?
  7. engine = PyEngine::spawn(&config).await?
  8. engine.send_reset(&config.scenario).await? → initial_obs
  9. mqtt.publish_observation(&initial_obs).await?


  11. let mut current_obs = initial_obs;  // 跟踪当前观测状态

  10. action_srv.accept() → (stream, addr)   // 接受 MUPC 连接
      tracing::info!("MUPC 已连接: {}", addr);

  11. loop {
        select! {
            result = read_frame_with_timeout(&mut stream, 30s) => {
                match result {
                    Ok(frame) => {
                        let t0 = Instant::now();
                        let resp = engine.send_step(frame.p_ref, frame.k_droop).await?;
                        current_obs = resp.data.clone();  // 更新当前观测
                        // 发布观测，失败时累计计数 (PRD EH-01)
                        if let Err(e) = mqtt.publish_observation(&current_obs).await {
                            mqtt.record_failure();
                            tracing::warn!("MQTT publish 失败: {}", e);
                            if mqtt.should_exit() {
                                tracing::error!("MQTT 连续3次失败，退出");
                                break;
                            }
                        }
                        let latency = t0.elapsed().as_millis();
                        metrics.record_step(latency, resp.reward, &resp.info);

                            // PRD SB-06: 重置后立即发布新 episode 初始观测
                            let new_obs = engine.send_reset(&config.scenario).await?;
                            if let SimResponse::Observation { data, .. } = new_obs {
                                mqtt.publish_observation(&data).await?;
                                current_obs = data;
                            }
                            metrics.reset_episode();
                        }
                    }
                    Err(TimeoutElapsed) => {
                        tracing::warn!("等待 MUPC 动作超时 (>30s)，跳过本步");
                        mqtt.publish_observation(&current_obs).await?; // 重新发布当前观测
                    }
                    Err(ConnectionLost) => {
                        tracing::warn!("MUPC 连接断开，等待重连");
                        action_srv.accept().await? → (stream, addr);  // 等待新连接
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                engine.send_shutdown().await?;
                mqtt.shutdown().await?;
                metrics.export("sim_metrics.json")?;
                break;
            }
        }
    }
```

**关键设计决策**：
- `read_frame_with_timeout()` 在连接上循环读取帧，内嵌 30s 超时（满足 PRD EH-04）
- 连接断开自动等待新连接（满足 PRD EH-03）
- `select!` 仅两个分支：读动作 + Ctrl+C
- `publish_observation` 异步非阻塞
- `validate_python_path` 启动时检查 Python 解释器路径有效性（解决 venv 相对路径问题）

### 4.2 `mqtt.rs` — MQTT 发布器

```rust
pub struct MqttPublisher {
    client: AsyncClient,
    event_loop_handle: JoinHandle<()>,
    topic: String,
    consecutive_failures: u32,
}

impl MqttPublisher {
    pub async fn connect(config: &SimBridgeConfig) -> Result<Self> {
        // 解析 host:port (mqtt_broker 格式: "192.168.3.118:1884")
        let (host, port) = parse_broker_addr(&config.mqtt_broker)?;
        let mut mqtt_opts = MqttOptions::new(
            &config.mqtt_client_id,
            host,
            port,
        );
        mqtt_opts.set_keep_alive(Duration::from_secs(5));

        let (client, event_loop) = AsyncClient::new(mqtt_opts, 256);
        // spawn event_loop 并监控退出状态
        let event_loop_handle = tokio::spawn(async move {
            event_loop.await;
        });

        Ok(Self {
            client,
            event_loop_handle,
            topic: config.mqtt_topic.clone(),
            consecutive_failures: 0,
        })
    }

    /// 健康检查：EventLoop 是否仍在运行
    pub fn is_healthy(&self) -> bool {
        !self.event_loop_handle.is_finished()
    }

    pub async fn publish_observation(&mut self, obs: &[f32]) -> Result<()> {
        if !self.is_healthy() {
            return Err(SimBridgeError::MqttEventLoopLost);
        }
        let payload = serde_json::to_string(obs)?;
        self.client
            .publish(&self.topic, QoS::AtMostOnce, false, payload)
            .await?;
        self.consecutive_failures = 0;
        Ok(())
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        tracing::warn!("MQTT publish 失败 ({}/{})", self.consecutive_failures, 3);
    }

    pub fn should_exit(&self) -> bool {
        self.consecutive_failures >= 3  // PRD EH-01: 连续3次 → exit
    }

    pub async fn shutdown(self) -> Result<()> {
        self.client.disconnect().await?;
        self.event_loop_handle.abort();
        Ok(())
    }
}
```

**序列化**：78 维 float 数组 → JSON 字符串 (如 `"[0.5, 75.0, ...]"`)，体积约 500 字节。

### 4.3 `action_server.rs` — TCP 动作服务器

```rust
pub struct ActionServer {
    listener: TcpListener,
}

impl ActionServer {
    pub async fn bind(addr: &str) -> Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!("ActionServer 监听 {}", addr);
        Ok(Self { listener })
    }

    /// 接受 MUPC 连接（阻塞直到连接建立）
    pub async fn accept(&self) -> Result<(TcpStream, SocketAddr)> {
        let (stream, addr) = self.listener.accept().await?;
        tracing::info!("MUPC 已连接: {}", addr);
        Ok((stream, addr))
    }
}

/// 从已建立的 TCP 连接读取一帧，带超时
pub async fn read_frame_with_timeout(
    stream: &mut TcpStream,
    timeout: Duration,
) -> Result<ActionFrame, ReadError> {
    let mut buf = [0u8; ACTION_FRAME_LEN];
    match tokio::time::timeout(timeout, stream.read_exact(&mut buf)).await {
        Ok(Ok(())) => {
            let frame = ActionFrame::parse(&buf)?;
            tracing::debug!("动作: p_ref={:.2}, k_droop={:.4}", frame.p_ref, frame.k_droop);
            Ok(frame)
        }
        Ok(Err(e)) => {
            tracing::warn!("TCP 读取错误: {}", e);
            Err(ReadError::ConnectionLost)
        }
        Err(_) => Err(ReadError::TimeoutElapsed),
    }
}

#[derive(Debug)]
pub enum ReadError {
    TimeoutElapsed,      // 30s 超时 (PRD EH-04)
    ConnectionLost,      // 连接断开 (PRD EH-03)
    CrcMismatch,         // CRC 校验失败
}

pub const ACTION_READ_TIMEOUT: Duration = Duration::from_secs(30);

impl ActionFrame {
    pub fn parse(buf: &[u8; ACTION_FRAME_LEN]) -> Result<Self> {
        let frame_id = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let cmd_type = buf[4];
        let reserved = buf[5];
        let payload_len = u16::from_be_bytes([buf[6], buf[7]]);
        let p_ref = f64::from_be_bytes(buf[8..16].try_into().unwrap());
        let k_droop = f64::from_be_bytes(buf[16..24].try_into().unwrap());
        let crc16 = u16::from_be_bytes([buf[24], buf[25]]);

        // CRC 校验
        let computed = crc16_modbus(&buf[..24]);
        if crc16 != computed {
            return Err(SimBridgeError::CrcMismatch { expected: computed, actual: crc16 });
        }
        // 物理约束 clamp (PRD §2.3)
        let p_ref = p_ref.clamp(-50.0, 50.0);
        let k_droop = k_droop.clamp(0.0, 30.0);

        Ok(Self { frame_id, cmd_type, reserved, payload_len, p_ref, k_droop, crc16 })
    }
}
```

### 4.4 `py_engine.rs` — Python 子进程管理

```rust
pub struct PyEngine {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
    restart_count: u32,
    max_restarts: u32,
}

impl PyEngine {
    pub async fn spawn(config: &SimBridgeConfig) -> Result<Self> {
        let mut child = Command::new(&config.python_cmd)
            .arg(&config.engine_script)
            .arg("--no-grid2op")  // 默认使用 VoltageSimulator, Phase 2 去掉此参数启用 Grid2Op
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;

        let stdin = BufWriter::new(child.stdin.take().unwrap());
        let stdout = BufReader::new(child.stdout.take().unwrap()).lines();

        Ok(Self { child, stdin, stdout, restart_count: 0, max_restarts: 3 })
    }

    pub async fn send_request(&mut self, req: &SimRequest) -> Result<SimResponse> {
        let line = serde_json::to_string(req)? + "\n";
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;

        // 带超时的读取
        let response_line = tokio::time::timeout(
            Duration::from_secs(5),
            self.stdout.next_line(),
        )
        .await
        .map_err(|_| SimBridgeError::PyEngineTimeout)??;

        let response_line = response_line
            .ok_or(SimBridgeError::PyEngineEof)?;

        serde_json::from_str(&response_line)
            .map_err(|e| SimBridgeError::Protocol(e.to_string()))
    }

    pub async fn send_step(&mut self, p_ref: f64, k_droop: f64) -> Result<SimResponse> {
        self.send_request(&SimRequest::Step { p_ref, k_droop }).await
    }

    pub async fn send_reset(&mut self, scenario: &str) -> Result<SimResponse> {
        self.send_request(&SimRequest::Reset { scenario: scenario.to_string() }).await
    }

    pub async fn send_shutdown(&mut self) -> Result<()> {
        let _ = self.send_request(&SimRequest::Shutdown).await;
        let _ = self.child.wait().await;
        Ok(())
    }

    /// 崩溃重启: stdout EOF 时调用
    pub async fn restart(&mut self, config: &SimBridgeConfig) -> Result<()> {
        if self.restart_count >= self.max_restarts {
            return Err(SimBridgeError::PyEngineMaxRestarts);
        }
        tracing::warn!("Python 引擎崩溃，重启中 ({}/{})", self.restart_count + 1, self.max_restarts);
        let _ = self.child.kill().await;
        let new = Self::spawn(config).await?;
        *self = Self { restart_count: self.restart_count + 1, ..new };
        Ok(())
    }
}
```

### 4.5 `engine.py` — Python 主循环（~50 行）

```python
#!/usr/bin/env python3
"""MUPC 仿真引擎 — JSONL stdin/stdout 通信。"""
import sys, json, argparse
import numpy as np
from mupc_env.core import MupcEnv

def main():
    p = argparse.ArgumentParser()
    p.add_argument("--no-grid2op", action="store_true")
    args = p.parse_args()

    env = MupcEnv(mode="MODE-01", use_grid2op=not args.no_grid2op)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            print(json.dumps({"type": "error", "msg": "invalid json"}), flush=True)
            continue

        msg_type = msg.get("type")
        if msg_type == "reset":
            obs, _ = env.reset(scenario=msg.get("scenario", "MODE-01"))
            respond({"type": "obs", "data": obs.tolist(), "reward": 0.0, "done": False, "info": {}})
        elif msg_type == "step":
            action = np.array([msg["p_ref"], msg["k_droop"]], dtype=np.float32)
            obs, reward, terminated, truncated, info = env.step(action)
            respond({"type": "obs", "data": obs.tolist(), "reward": float(reward),
                      "done": terminated or truncated, "info": info})
        elif msg_type == "shutdown":
            respond({"type": "shutdown_ack"})
            break

def respond(obj):
    print(json.dumps(obj), flush=True)

if __name__ == "__main__":
    main()
```

### 4.6 `metrics.rs` — 指标收集

```rust
impl MetricsCollector {
    pub fn new(scenario: &str) -> Self;
    pub fn record_step(&mut self, latency_ms: u64, reward: f64, info: &serde_json::Value);
    pub fn export(&self, path: &str) -> Result<()>;
    pub fn reset_episode(&mut self, scenario: &str);
}
```

- 内存中累计: `total_steps`, `total_reward`, `latency_samples: Vec<u64>`
- `export()` 序列化为 JSON + 计算 min/max/avg/p99 延迟
- `reset_episode()` 追加已完成 episode 到历史数组, 清零当前

---

## 5. 错误处理

| 错误类型 | 变体 | 处理策略 |
|---------|------|---------|
| MQTT 不可达 | `MqttConnectFailed` | 启动阶段 panic, 运行阶段连续3次 → exit(1) |
| TCP 绑定失败 | `TcpBindFailed` | panic 退出 |
| Python spawn 失败 | `PySpawnFailed` | panic 退出 |
| Python 子进程崩溃 | `PyEngineEof` | 重试 3 次 (wait 2s), 仍失败 exit(1) |
| Python 超时 (5s) | `PyEngineTimeout` | 返回错误, 主循环跳过本步 |
| JSONL 解析失败 | `Protocol` | 主循环 WARN + 跳过该响应 |
| CRC 校验失败 | `CrcMismatch` | WARN + 丢弃帧 |

---

## 6. 文件结构预估

```
新增文件 (10 个 Rust + 2 个 Python + 1 YAML):

mupc/crates/sim-bridge/Cargo.toml          # Rust crate 定义
mupc/crates/sim-bridge/src/main.rs         # ~120 行
mupc/crates/sim-bridge/src/config.rs       # ~50 行
mupc/crates/sim-bridge/src/mqtt.rs         # ~60 行
mupc/crates/sim-bridge/src/action_server.rs # ~80 行
mupc/crates/sim-bridge/src/py_engine.rs    # ~100 行
mupc/crates/sim-bridge/src/scenario.rs     # ~60 行
mupc/crates/sim-bridge/src/metrics.rs      # ~80 行
mupc/crates/sim-bridge/src/error.rs        # ~30 行

sim-env/engine.py                           # ~50 行
sim-env/requirements.txt                    # grid2op, pandapower, lightsim2grid, numpy
mupc/config/sim_config.yaml                 # ~20 行

修改文件 (1 个):
mupc/Cargo.toml                             # workspace members: + "crates/sim-bridge"

总代码量: Rust ~580 行, Python ~50 行
```

---

## 7. 技术决策记录

## 8. 测试策略

| 模块 | 测试类型 | Mock 方式 | 覆盖目标 |
|------|---------|---------|---------|
| `config.rs` | 单元 | 提供 valid/invalid YAML 文件 | 必填字段检测 / 默认值 / 路径解析 |
| `action_server.rs` | 单元 | 用 `tokio::net::TcpStream` 模拟客户端发送 26 字节帧 | 正常帧解析 / CRC 错误 / 连接断开 / 30s 超时 |
| `py_engine.rs` | 单元 | 用 `tokio::process::Command` 启动 mock Python 脚本（echo JSONL） | spawn 成功 / JSONL 解析 / 超时 / EOF 检测 / 质量崩溃重启 (最多3次) |
| `mqtt.rs` | 单元 | 用 `rumqttc` 连接本地 mosquitto (CI 中安装) | connect / publish / EventLoop 健康检查 / 连续失败计数 |
| `metrics.rs` | 单元 | 构造 Snapshot 数组 | min/max/avg/p99 计算 / JSON 导出 / reset_episode 清零 |
| `main.rs` | 集成 | 启动 mock engine.py + mock MQTT broker + mock TCP client | 完整主循环：reset → 3步 step → done → reset / Ctrl+C 退出 |
| 全链路 | 系统 | 真实 engine.py (VoltageSimulator) + 真实 mosquitto + 真实 TCP | 96 步 episode 完整闭环 |

**Mock Python 脚本示例** (`tests/mock_engine.py`)：
```python
import sys, json
print(json.dumps({"type":"obs","data":[0.5]*78,"reward":0.0,"done":False,"info":{}}))
sys.stdout.flush()
for line in sys.stdin:
    msg = json.loads(line)
    if msg["type"] == "step":
        print(json.dumps({"type":"obs","data":[0.5]*78,"reward":1.0,"done":False,"info":{}}))
        sys.stdout.flush()
    elif msg["type"] == "shutdown":
        print(json.dumps({"type":"shutdown_ack"}))
        break
```

---

### ADR-001: sim-bridge 与 MUPC workspace 其他 crate 零依赖

**决定**：sim-bridge 不依赖任何 MUPC crate (mqtt-plugin, intercore, common 等)。

**理由**：
1. sim-bridge 是**外部测试工具**，不是 MUPC 运行时组件
2. 避免循环依赖（MUPC 不依赖 sim-bridge）
3. 独立编译和发布，隔离故障域

### ADR-002: JSONL 替代 gRPC/HTTP

**决定**：sim-bridge ↔ engine.py 通信使用 stdin/stdout JSONL，不使用网络协议。

**理由**：
1. 单机通信，网络栈开销多余
2. JSONL 可人工阅读，调试友好
3. 零序列化依赖（serde_json 即可）
4. tokio::process 已提供完善的子进程管理

### ADR-003: Python 引擎默认使用 VoltageSimulator

**决定**：Phase 1 默认为 `--no-grid2op`，Phase 2 再启用 Grid2Op。

**理由**：
1. Grid2Op 对 Python 环境和 C 扩展 (lightsim2grid) 有额外依赖
2. VoltageSimulator 步进 < 2ms，适合快速调通 sim-bridge 核心流程
3. Phase 2 切换到 Grid2Op 只需去掉 `--no-grid2op` 参数

---

**文档状态**: 待评审

**关联文档**:
- PRD: `docs/superpowers/specs/modules/11-MUPC-仿真测试环境-PRD.md` `[REVIEWED: PASS]`
- MUPC-AI2: `mupc_env/` 训练环境源码
