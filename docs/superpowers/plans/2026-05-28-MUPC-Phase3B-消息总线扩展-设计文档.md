[DESIGN_APPROVED]

# MUPC Phase 3B 消息总线扩展 - 技术设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-28 | 架构师 | ✅ 已批准 |

---

## 1. 需求概述

### 1.1 项目背景

Phase 3A 已实现 data-processing 和 strategy-engine 模块，采用 `tokio::sync::mpsc` 进行进程内通信。Phase 3B 需要扩展消息总线，支持：
- 进程间通信（data-processing ↔ strategy-engine）
- 北向通信（与物联平台、配电自动化主站）

### 1.2 目标

1. 构建分层 MQTT 架构
2. 实现进程间通过本地 mosquitto 通信
3. 实现北向通过 emqx 企业级 MQTT Broker 通信
4. 支持 TLS 双向证书认证
5. 支持消息持久化和断线重连

---

## 2. 架构设计

### 2.1 分层 MQTT 架构

```
┌─────────────────────────────────────────────────────────────┐
│                      emqx (北向/云端)                         │
│            物联平台 + 配电自动化主站                          │
└─────────────────────────┬───────────────────────────────────┘
                          │ MQTT + TLS + 证书认证
                          │ Port: 8883
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                      本地 mosquitto                          │
│                    (进程间通信)                               │
│                    Port: 1883                                │
└───────┬─────────────────┬─────────────────┬─────────────────┘
        │                 │                 │
        ▼                 ▼                 ▼
┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│data-processing│  │strategy-engine│  │   其他模块   │
│   (发布)      │  │   (订阅)      │  │             │
└──────────────┘  └──────────────┘  └──────────────┘
```

### 2.2 MQTT Topic 设计

#### 北向 Topic（emqx）

| Topic | 方向 | QoS | 说明 |
|-------|------|-----|------|
| `mupc/north/telemetry` | → 物联平台 | 1 | 高频遥测数据 |
| `mupc/north/fault` | → 物联平台 | 2 | 故障事件 |
| `mupc/north/strategy/command` | ← 物联平台 | 2 | 下行指令 |
| `mupc/north/status` | ↔ 双方 | 0 | 设备状态 |

#### 进程间 Topic（mosquitto）

| Topic | 方向 | QoS | 说明 |
|-------|------|-----|------|
| `mupc/local/telemetry` | → | 0 | 遥测数据 |
| `mupc/local/strategy/command` | → | 1 | 策略指令 |
| `mupc/local/ai/ready` | → | 0 | AI 就绪状态 |

---

## 3. 模块设计

### 3.1 新增模块：`mqtt-bridge`

```
mupc/
├── crates/
│   ├── mqtt-bridge/          # 新增：MQTT 网桥模块
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── client.rs     # MQTT 客户端封装
│   │   │   ├── local_client.rs  # 本地 mosquitto 客户端
│   │   │   ├── north_client.rs  # 北向 emqx 客户端
│   │   │   ├── topics.rs     # Topic 定义
│   │   │   ├── config.rs     # 配置结构
│   │   │   └── error.rs      # 错误类型
│   │   ├── Cargo.toml
│   │   └── tests/
│   └── data-processing/      # 修改：移除 mpsc，使用 mqtt-bridge
└── docker/
    └── mosquitto/            # 本地 MQTT Broker 配置
```

### 3.2 核心组件

#### MqttBridge trait

```rust
/// MQTT 网桥 trait
#[async_trait]
pub trait MqttBridge: Send + Sync {
    /// 发布消息
    async fn publish(&self, topic: &str, payload: &[u8], qos: u8) -> Result<(), MqttError>;

    /// 订阅主题
    async fn subscribe(&self, topic: &str, qos: u8) -> Result<(), MqttError>;

    /// 获取连接状态
    fn is_connected(&self) -> bool;
}
```

#### LocalMqttClient

- 连接本地 mosquitto (127.0.0.1:1883)
- 用于进程间通信
- QoS 0/1
- 无 TLS

#### NorthMqttClient

- 连接 emqx (可配置地址:8883)
- 用于北向通信
- QoS 1/2
- TLS + 双向证书认证

### 3.3 配置结构

```rust
#[derive(Debug, Clone)]
pub struct MqttConfig {
    /// 本地 mosquitto 配置
    pub local: LocalMqttConfig,
    /// 北向 emqx 配置
    pub north: NorthMqttConfig,
}

#[derive(Debug, Clone)]
pub struct LocalMqttConfig {
    pub broker_addr: String,      // "127.0.0.1:1883"
    pub client_id: String,
    pub clean_session: bool,
    pub keepalive_secs: u64,
}

#[derive(Debug, Clone)]
pub struct NorthMqttConfig {
    pub broker_addr: String,      // "mqtt.example.com:8883"
    pub client_id: String,
    pub keepalive_secs: u64,
    pub tls: TlsConfig,
    pub credentials: Credentials,
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}
```

---

## 4. 数据流设计

### 4.1 遥测数据流

```
intercore → DataCollector → HighFreqTelemetry
                                      ↓
                              LocalMqttClient
                                      ↓
                              mosquitto (local)
                                      ↓
                              NorthMqttClient
                                      ↓
                              emqx (cloud)
                                      ↓
                              物联平台
```

### 4.2 策略指令流

```
物联平台 → emqx → NorthMqttClient → LocalMqttClient → mosquitto
                                                        ↓
                                              AiCommandValidator
                                                        ↓
                                              intercore
```

### 4.3 消息持久化策略

| 消息类型 | QoS | 持久化 | 说明 |
|---------|-----|--------|------|
| 遥测数据 | 1 | 否 | 仅实时展示 |
| 故障事件 | 2 | 是 | 需要事后分析 |
| 策略指令 | 2 | 是 | 断线重连恢复 |
| 设备状态 | 0 | 否 | 周期性刷新 |

---

## 5. 错误处理

### 5.1 连接错误

```rust
pub enum MqttBridgeError {
    #[error("连接失败: {0}")]
    ConnectionFailed(String),

    #[error("订阅失败: {0}")]
    SubscribeFailed(String),

    #[error("发布失败: {0}")]
    PublishFailed(String),

    #[error("TLS 错误: {0}")]
    TlsError(String),

    #[error("证书错误: {0}")]
    CertificateError(String),
}
```

### 5.2 重连策略

- 初始重连间隔：1秒
- 最大重连间隔：60秒
- 重连指数退避：2倍
- 最大重连次数：无限制（保持连接）

---

## 6. 安全设计

### 6.1 TLS 配置

- 北向连接强制使用 TLS 1.2+
- 双向证书认证（Client Certificate）

### 6.2 证书管理

- CA 证书：验证服务器证书
- 客户端证书：客户端身份认证
- 私钥：使用 RSA 2048-bit 或 ECDSA P-256

### 6.3 Topic 权限

- 进程间 Topic 仅允许本地服务订阅
- 北向 Topic 按角色分配读写权限

---

## 7. 技术选型

| 组件 | 选择 | 说明 |
|------|------|------|
| MQTT 客户端库 | rumqttc | 异步 Rust MQTT 客户端 |
| 本地 Broker | mosquitto | 轻量开源 MQTT Broker |
| 云端 Broker | emqx | 企业级 MQTT Broker |
| TLS 库 | rustls | Rust TLS 实现 |

---

## 8. 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| MB-01 | 本地 mosquitto 连接成功 | 单元测试 |
| MB-02 | 北向 emqx 连接成功（TLS） | 集成测试 |
| MB-03 | 进程间消息传递 < 100ms | 性能测试 |
| MB-04 | 断线重连自动恢复 | 单元测试 |
| MB-05 | QoS 1 消息至少一次到达 | 单元测试 |
| MB-06 | QoS 2 消息恰好一次到达 | 单元测试 |
| MB-07 | 证书认证成功 | 集成测试 |
| MB-08 | 消息持久化正确 | 集成测试 |

---

## 9. 未来扩展

| Phase | 内容 |
|-------|------|
| 3B.2 | 添加 MQTT WebSocket 支持（前端直连）|
| 3B.3 | 添加消息压缩（zstd）|
| 3C | AI 优化引擎（LSTM/TCN + MADDPG/PPO）|

---

**评审状态**：✅ 已批准
**批准人**：项目经理
**批准日期**：2026-05-28
