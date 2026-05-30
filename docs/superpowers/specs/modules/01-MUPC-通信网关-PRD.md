# MUPC 通信网关模块产品需求文档（PRD）

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.1 | 2026-05-29 | 需求分析师 | **[REVIEWED: PASS]** |

> 本文档为通信网关模块的权威需求文档。历史来源文档已在 v1.0 文档体系重构中合并，不再单独维护。

---

## 1. 产品概述

### 1.1 产品定位

通信网关是 MUPC 微电网特种调控装置"异构双核心模块主控架构"中**非实时处理核心**（大脑）的北向通信子系统。它负责与调度主站、配电自动化主站、物联平台等外部系统建立和维护通信连接，完成数据的上报和指令的接收转发。

**核心职责：**
- 北向 IEC 104 协议通信：与调度主站互通（遥测/遥信/遥控）
- 北向 IEC 61850 MMS 协议通信：与配电自动化 IED 设备互通
- 北向 MQTT 协议通信：与物联平台 / 虚拟电厂（VPP）互通
- 消息总线：进程间通信（本地）与北向消息路由（云端）
- 协议转换与数据格式统一
- 连接管理、心跳保活、自动重连
- 通过核间通信监控实时控制模块运行状态（系统级看门狗），详见模块10-PRD 第4章
- OTA 远程升级（模型 + 固件），详见模块07-PRD

### 1.2 目标平台

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux (openEuler 22.03+) |
| 硬件 | RK3588 |
| 编程语言 | Rust (>= 1.75) |
| 异步运行时 | Tokio |
| 网络框架 | Tower + tokio-net |

### 1.3 涵盖模块

| Crate | 说明 | 状态 |
|-------|------|------|
| `gateway` | 北向 IEC 104 协议网关 | Phase 1 ✅ [REVIEWED: PASS] |
| `iec61850-plugin` | IEC 61850 MMS 客户端插件 | Phase 2+ ✅ [REVIEWED: PASS] |
| `mqtt-plugin` | MQTT 协议客户端插件（北向扩展） | Phase 2+ ✅ [REVIEWED: PASS] |
| `mqtt-bridge` | MQTT 网桥（本地+北向分层架构） | Phase 3B ✅ [DESIGN_APPROVED] |
| `device-trait` | 核心 trait 定义（Plugin、MessageBus 等） | Phase 2+ ✅ [REVIEWED: PASS] |

### 1.4 用户角色

| 角色 | 描述 | 通信协议 | 权限范围 | 优先级 |
|------|------|----------|----------|--------|
| **调度主站运维人员** | 通过 IEC 104 协议远程监控和控制 MUPC 装置 | IEC 104 | 只读监控，可下发调度指令但需经本地策略引擎校验 | 低（次优先） |
| **配电自动化操作员** | 通过 IEC 61850 协议管理配电自动化 IED 设备 | IEC 61850 MMS | 设备状态监控、遥控操作 | 低 |
| **物联平台管理员** | 通过 MQTT 协议接入虚拟电厂或物联平台 | MQTT | 数据上报、指令接收 | 低 |
| **本地运维人员** | 通过 Web UI、星闪/Wi-Fi/蓝牙进行本地有线及无线运维 | HTTP / 星闪 / Wi-Fi / 蓝牙 | 最高权限，可覆盖调度指令 | 高（最高优先） |
| **系统管理员** | 系统整体运维和 OTA 升级 | 所有协议 | 所有配置权限 | 高 |

**权限优先级规则：**
- 权限冲突时，本地运维人员指令优先于调度主站 / 配电自动化 / 物联平台指令
- 调度主站 / 配电自动化 / 物联平台下发的指令需经本地策略引擎校验后才转发

---

## 2. IEC 104 功能需求

> 本节内容来源：主 PRD Section 3.2 — **[REVIEWED: PASS]**

### 2.1 IEC 104 协议通信

**User Story：**
> 作为调度主站运维人员，我需要通过 IEC 104 协议与 MUPC 装置通信，以便实现远程监控和控制。

**验收标准：**
- 全面兼容 IEC 60870-5-104 协议
- 支持 TCP 连接建立、保持（心跳）
- 支持 U 帧：STARTDT、STOPDT、TESTFR
- 支持 S 帧：确认收到的 I 帧
- 支持 I 帧：发送/接收应用数据
- 默认端口：2404

### 2.2 连接管理

**User Story：**
> 作为系统，我需要管理 IEC 104 连接状态，以便在连接异常时自动重连。

**验收标准：**
- 连接状态机：断开、连接中、已连接、等待激活
- 心跳间隔：**默认 10 秒，支持运行时配置**，配置范围：1 秒 ~ 60 秒
- 连接超时：30 秒无响应则判定超时
- 自动重连机制：断连后 5 秒开始重连，最多重试 10 次
- 重试失败后，每 1 分钟尝试一次
- 支持最多 5 个并发连接
- 心跳间隔配置方式：通过 Web UI 配置

### 2.3 数据收发

**User Story：**
> 作为调度主站运维人员，我需要通过 IEC 104 协议收发遥测、遥信、遥控数据。

**验收标准：**

**类型标识（TypeID）支持：**
- M_SP_TA_1（单点遥信，带时标）
- M_DP_TA_1（双点遥信，带时标）
- M_ME_TA_1（测量值，归一化值，带时标）
- M_ME_TD_1（测量值，归一化值，带时标）
- C_SC_TA_1（单点遥控，带时标）
- C_DC_TA_1（双点遥控，带时标）
- C_SE_TA_1（调节命令，带时标）

**数据上行（MUPC → 调度主站）：**
- 周期性发送遥测数据，周期可配置（默认 1 秒）
- 告警时立即上送
- 时标采用 UTC 时间

**数据下行（调度主站 → MUPC）：**
- 接收并确认收到的数据
- 遥控命令需经过本地策略引擎校验后才转发

### 2.4 调度指令接收与转发

**User Story：**
> 作为调度主站，我需要下发有功/无功设定值、一次调频参数、远程启停命令给 MUPC。

**验收标准：**
- 接收调度主站下发的控制指令
- 指令类型：
  - 有功设定值（P_set）：范围 -1000kW ~ 1000kW
  - 无功设定值（Q_set）：范围 -1000kVar ~ 1000kVar
  - 电池启停命令：启动/停止
  - 一次调频参数：K 值、Deadband
- 指令校验：检查数据范围、格式有效性
- **权限分级：**
  - 调度主站下发的指令需经过本地策略引擎校验后才转发
  - 如本地运维人员下发冲突指令，本地运维人员指令优先
- 校验通过后，通过 intercore 转发给实时控制模块
- 响应时间：从接收到转发 ≤ 100ms

### 2.5 IEC 104 连接异常流程

```
连接断开
    ↓
等待 5 秒后自动重连
    ↓
最多重试 10 次
    ↓
重试失败后，每 1 分钟尝试一次
    ↓
恢复后自动建立连接
```

---

## 3. IEC 61850 功能需求

> 本节内容综合来源：
> - Phase 2 规格文档 Section 4.2.2 — **[REVIEWED: PASS]**
> - IEC61850 MMS 实现设计文档 — **DRAFT**

### 3.1 概述

IEC 61850 插件提供 MMS（制造报文规范）协议客户端能力，支持连接符合 IEC 61850 标准的 IED 设备，实现数据对象读写、GOOSE 消息订阅等功能。

> **IEC 61850-7-420 DER 逻辑节点说明：** 完整的 IEC 61850-7-420 DER 逻辑节点模型（如 Photovoltaic、Storage、ElectricVehicle 等）延后至 Phase 2+ 实现。Phase 2 实现 MMS 传输层基础读写能力（7-2/7-3/8-1），支持标准 7-4 逻辑节点。

目标对接设备：
- 配电自动化 IED
- 支持 IEC 61850-7-420 的光伏逆变器
- 支持 IEC 61850 的消防控制系统

### 3.2 技术选型

| 特性 | 说明 |
|------|------|
| **协议栈实现** | libIEC61850 C 库 + Rust FFI 绑定 |
| **标准** | IEC 61850-7-2/7-3/8-1（MMS） |
| **平台** | Linux（RK3588） |
| **许可** | GPL-2.0 / 商业许可 |
| **依赖** | `iec61850-sys` (0.3, features: ["tls"])、`rustls` (0.23)、`webpki-roots` (0.26) |

### 3.3 MMS 客户端功能

**User Story：**
> 作为配电自动化操作员，我需要通过 IEC 61850 MMS 协议读写 IED 设备的数据对象。

**验收标准：**

**连接管理：**
- 支持 TCP 连接建立与断开
- 短连接模式：每次请求建立新连接
- 默认端口：102
- 连接超时可配置（默认 5000ms）
- 读取超时可配置（默认 3000ms）

**MMS 服务支持：**
- Read 服务：读取数据对象（DataObject）
- Write 服务：写入数据对象
- DefineVariableAccess（预留）
- GetDataAccessAttributes（预留）

**数据对象：**
- 逻辑节点（LN）：如 LLN0、MMXU1、XCBR1 等
- 数据对象名（DO）：如 ST$Pos、MX$Meas 等
- 支持标准 IEC 61850-7-4 数据对象模型

**TLS 支持（可选）：**
- MMS over TLS 1.2+
- 双向证书认证
- 支持 CA 证书、客户端证书、客户端私钥配置
- 可配置是否验证对端证书

**验收标准（MMS 专用）：**

| ID | 标准 | 验证方法 |
|----|------|----------|
| MMS-01 | MMS Read 服务正常工作 | 读取 IED 数据对象 |
| MMS-02 | MMS Write 服务正常工作 | 写入 IED 数据对象 |
| MMS-03 | TLS 连接成功 | MMS over TLS 连接测试 |
| MMS-04 | 超时处理正确 | 模拟超时场景 |
| MMS-05 | 错误处理正确 | 模拟错误响应 |
| MMS-06 | 短连接模式正确 | 验证每次请求建立新连接 |

### 3.4 GOOSE 消息订阅

**User Story：**
> 作为系统，我需要订阅 IED 设备发出的 GOOSE 消息，以便获取实时事件。

**验收标准（[REVIEWED: PASS]）：**
- 支持订阅 GOOSE 消息
- 通过 GOOSE ID（`go_id`）识别消息源
- 消息通过 MessageHandler 回调处理
- 支持配置本地 IP、本地端口、远端 IP、远端端口

### 3.5 错误类型

| 错误 | 说明 |
|------|------|
| MmsConnectFailed | MMS 连接失败（含 TLS 连接失败） |
| MmsTimeout | MMS 请求超时（连接/读取） |
| MmsProtocolError | MMS 协议错误 |
| DataObjectNotFound | 数据对象不存在 |
| WriteFailed | 写操作失败 |
| MmsInvalidResponse | 无效响应 |
| TlsConnectFailed | TLS 连接失败 |
| CertVerifyFailed | 证书验证失败 |
| Asn1EncodeFailed | ASN.1 编码失败 |
| Asn1DecodeFailed | ASN.1 解码失败 |

### 3.6 配置定义

```rust
/// MMS 配置
pub struct MmsConfig {
    pub local_ip: String,            // 本地 IP
    pub local_port: u16,             // 本地端口
    pub remote_ip: String,           // 远端 IP（IED 设备）
    pub remote_port: u16,            // 远端端口（MMS 标准端口 102）
    pub tls: Option<MmsTlsConfig>,   // TLS 配置
    pub connect_timeout_ms: u64,     // 连接超时（毫秒），默认 5000
    pub read_timeout_ms: u64,        // 读取超时（毫秒），默认 3000
}

/// MMS TLS 配置
pub struct MmsTlsConfig {
    pub enabled: bool,               // 是否启用 TLS
    pub ca_cert_path: String,        // CA 证书路径
    pub client_cert_path: String,    // 客户端证书路径
    pub client_key_path: String,     // 客户端私钥路径
    pub verify_peer: bool,           // 是否验证对端证书，默认 true
}
```

### 3.7 文件结构

```
mupc/crates/iec61850-plugin/src/
├── lib.rs              # 模块导出
├── mms_client.rs       # MMS 客户端封装（libIEC61850）
├── mms_types.rs        # MMS 数据类型定义
├── asn1_utils.rs       # ASN.1 编码/解码工具
├── config.rs           # MmsConfig, MmsTlsConfig
├── device.rs           # Iec61850Device trait
├── goose.rs            # GOOSE 消息订阅
└── errors.rs           # 错误类型定义
```

### 3.8 性能要求

| 指标 | 要求 |
|------|------|
| MMS 连接建立时间 | ≤ 5s |
| Read 请求响应时间 | ≤ 3s |
| Write 请求响应时间 | ≤ 3s |
| 操作成功率 | ≥ 99% |
| 并发连接 | ≥ 1 个 IED（可扩展） |

---

## 4. MQTT 功能需求

> 本节内容综合来源：
> - Phase 2 规格文档 Section 4.2.3 — **[REVIEWED: PASS]**
> - Phase 3B 消息总线设计文档 — **[DESIGN_APPROVED]**

### 4.1 概述

MQTT 功能分为两个层次：

1. **MQTT 北向客户端（mqtt-plugin）**：与物联平台 / 虚拟电厂（VPP）通信，基于 emqx 企业级 MQTT Broker，支持 TLS 双向证书认证
2. **MQTT 本地网桥（mqtt-bridge）**：进程间通信，基于本地 mosquitto Broker，用于 data-processing 与 strategy-engine 等模块的消息传递

### 4.2 分层 MQTT 架构

```
┌─────────────────────────────────────────────────────────────┐
│                   emqx（北向/云端）                             │
│         物联平台 + 配电自动化主站                              │
└─────────────────────────┬───────────────────────────────────┘
                          │ MQTT + TLS + 证书认证
                          │ Port: 8883
                          ▼
┌─────────────────────────────────────────────────────────────┐
│                    本地 mosquitto                             │
│                   （进程间通信）                                │
│                   Port: 1883                                 │
└───────┬─────────────────┬─────────────────┬─────────────────┘
        │                 │                 │
        ▼                 ▼                 ▼
┌──────────────┐  ┌──────────────┐  ┌──────────────┐
│data-processing│  │strategy-engine│  │   其他模块   │
│   （发布）     │  │   （订阅）     │  │             │
└──────────────┘  └──────────────┘  └──────────────┘
```

### 4.3 MQTT 北向客户端功能

**User Story：**
> 作为物联平台管理员，我需要通过 MQTT 协议与 MUPC 装置安全通信，以便接收遥测数据和下发指令。

**验收标准（[REVIEWED: PASS]）：**

**连接管理：**
- 连接可配置的 MQTT Broker（emqx）
- 支持客户端 ID 配置
- 支持用户名/密码认证（可选）
- 支持 Keep Alive 保活机制
- 默认 QoS 级别可配置
- 断线重连：初始间隔 1 秒，指数退避（2 倍），最大间隔 60 秒，无重连次数上限

**TLS 安全（[REVIEWED: PASS]）：**
- 支持 MQTT over TLS 1.2+（默认端口 8883）
- 支持双向证书认证（CA 证书 + 客户端证书 + 客户端私钥）
- 证书格式：X.509 v3（PEM）

**QoS 支持（[REVIEWED: PASS]）：**
- QoS 0：至多一次（AtMostOnce）
- QoS 1：至少一次（AtLeastOnce）
- QoS 2：恰好一次（ExactlyOnce）

**订阅/发布：**
- 支持按主题（Topic）订阅
- 支持按主题发布消息
- 消息带 QoS 级别
- 订阅的消息通过回调处理

**错误类型：**
- ConnectFailed：连接失败
- AuthFailed：认证失败
- SubscribeFailed：订阅失败
- PublishFailed：发布失败
- Disconnected：连接已断开

### 4.4 MQTT 本地网桥功能

**User Story：**
> 作为系统，我需要通过本地 MQTT 网桥实现模块间松耦合通信，并路由消息至北向 MQTT。

**验收标准（[DESIGN_APPROVED]）：**

**MqttBridge trait：**

```rust
/// MQTT 网桥 trait
pub trait MqttBridge: Send + Sync {
    async fn publish(&self, topic: &str, payload: &[u8], qos: u8) -> Result<(), MqttBridgeError>;
    async fn subscribe(&self, topic: &str, qos: u8) -> Result<(), MqttBridgeError>;
    fn is_connected(&self) -> bool;
}
```

**本地客户端（LocalMqttClient）：**
- 连接本地 mosquitto（`127.0.0.1:1883`）
- 用于进程间通信
- QoS 0/1
- 无 TLS

**北向客户端（NorthMqttClient）：**
- 连接 emqx（可配置地址 `:8883`）
- 用于北向通信
- QoS 1/2
- TLS + 双向证书认证

### 4.5 Topic 设计

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

### 4.6 消息持久化策略

| 消息类型 | QoS | 持久化 | 说明 |
|---------|-----|--------|------|
| 遥测数据 | 1 | 否 | 仅实时展示 |
| 故障事件 | 2 | 是 | 需要事后分析 |
| 策略指令 | 2 | 是 | 断线重连恢复 |
| 设备状态 | 0 | 否 | 周期性刷新 |

### 4.7 数据流设计

**遥测数据流：**

```
intercore → DataCollector → LocalMqttClient → mosquitto → NorthMqttClient → emqx → 物联平台
```

**策略指令流：**

```
物联平台 → emqx → NorthMqttClient → mosquitto → AiCommandValidator → intercore
```

### 4.8 配置定义

```rust
/// MQTT 总配置
pub struct MqttConfig {
    pub local: LocalMqttConfig,   // 本地 mosquitto 配置
    pub north: NorthMqttConfig,   // 北向 emqx 配置
}

/// 本地 MQTT 配置
pub struct LocalMqttConfig {
    pub broker_addr: String,       // "127.0.0.1:1883"
    pub client_id: String,
    pub clean_session: bool,
    pub keepalive_secs: u64,
}

/// 北向 MQTT 配置
pub struct NorthMqttConfig {
    pub broker_addr: String,       // "mqtt.example.com:8883"
    pub client_id: String,
    pub keepalive_secs: u64,
    pub tls: TlsConfig,
    pub credentials: Credentials,
}

/// TLS 配置
pub struct TlsConfig {
    pub ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}
```

---

## 5. 消息总线功能需求

> 本节内容综合来源：
> - 主 PRD Section 3.7.3 — **[REVIEWED: PASS]**
> - Phase 3B 消息总线设计文档 — **[DESIGN_APPROVED]**

### 5.1 概述

消息总线是模块间松耦合通信的基础设施，支持 publish/subscribe 模式。Phase 1 使用 `tokio::sync::mpsc` 实现进程内通信，Phase 3B 扩展为基于 MQTT 的分层消息架构，同时支持进程内和进程间通信。

### 5.2 MessageBus Trait

**验收标准（[REVIEWED: PASS]）：**

```rust
/// 消息总线接口
pub trait MessageBus: Send + Sync {
    /// 发布消息
    fn publish(&self, msg: Message) -> Result<(), BusError>;
    /// 订阅主题
    fn subscribe(&self, topic: Topic, handler: Arc<dyn MessageHandler>) -> Result<(), BusError>;
    /// 取消订阅
    fn unsubscribe(&self, topic: &Topic) -> Result<(), BusError>;
}
```

**数据类型：**

```rust
/// 消息主题
pub struct Topic(String);

/// 消息封装
pub struct Message {
    pub topic: Topic,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

/// 消息订阅者
pub trait MessageHandler: Send + Sync {
    fn handle(&self, msg: Message);
}
```

### 5.3 错误类型

```rust
pub enum BusError {
    TopicNotFound(String),
    PublishFailed(String),
    SubscribeFailed(String),
}
```

### 5.4 实现策略

| Phase | 实现方式 | 说明 |
|-------|----------|------|
| Phase 1 | `tokio::sync::mpsc` | 进程内通信，最简实现 |
| Phase 2+ | plugin-loader + MessageBus trait | 可替换为 AMQP/MQTT |
| Phase 3B | mosquitto（进程间）+ emqx（北向） | 分层 MQTT 总线架构 |

### 5.5 性能要求

| 指标 | Phase 1 | Phase 3B |
|------|---------|----------|
| 消息总线吞吐量 | ≥ 1000 msg/s | ≥ 10000 msg/s |
| 进程间消息延迟 | N/A | < 100ms |

---

## 6. 非功能性需求

### 6.1 性能需求

| 指标 | 要求 | 来源 |
|------|------|------|
| IEC 104 数据上报周期 | ≥ 1Hz（可配置） | **[REVIEWED: PASS]** |
| 调度指令处理延迟（接收→转发） | ≤ 100ms | **[REVIEWED: PASS]** |
| IEC 104 并发连接数 | ≤ 5 个 | **[REVIEWED: PASS]** |
| MMS 连接建立时间 | ≤ 5s | DRAFT |
| MMS Read/Write 响应时间 | ≤ 3s | DRAFT |
| MMS 操作成功率 | ≥ 99% | **[REVIEWED: PASS]** |
| MQTT 消息延迟（QoS 0） | < 100ms | **[REVIEWED: PASS]** |
| MQTT 消息延迟（QoS 1/2） | < 500ms | **[REVIEWED: PASS]** |
| 进程间消息传递延迟 | < 100ms | **[DESIGN_APPROVED]** |
| Web UI 页面响应时间 | ≤ 2 秒 | **[REVIEWED: PASS]** |
| 消息总线吞吐量 | ≥ 10000 msg/s | **[REVIEWED: PASS]** |
| 消息总线吞吐量（Phase 1 mpsc） | ≥ 1000 msg/s（接口预留） | **[REVIEWED: PASS]** |
| MQTT 断线重连最大间隔 | 60 秒（指数退避） | **[DESIGN_APPROVED]** |

### 6.2 可靠性需求

| 指标 | 要求 | 来源 |
|------|------|------|
| 系统 MTBF | ≥ 50,000 小时 | **[REVIEWED: PASS]** |
| IEC 104 心跳间隔 | 默认 10 秒，可配置（1秒~60秒） | **[REVIEWED: PASS]** |
| IEC 104 连接超时 | 30 秒 | **[REVIEWED: PASS]** |
| IEC 104 自动重连 | 5 秒后开始，最多 10 次，之后每 1 分钟尝试 | **[REVIEWED: PASS]** |
| MQTT 断线重连 | 初始 1 秒，指数退避（2 倍），最大 60 秒，不限次数 | **[DESIGN_APPROVED]** |
| 消息总线 QoS | 支持 0/1/2 | **[DESIGN_APPROVED]** |
| 系统级看门狗 | 通过核间通信监控实时控制模块运行状态，异常时告警并尝试恢复 | 详见模块10-PRD 第4章 |

### 6.3 安全需求

| 需求 | 说明 | 来源 |
|------|------|------|
| **MQTT over TLS** | MQTT 北向连接强制使用 TLS 1.2+，双向证书认证 | **[REVIEWED: PASS]** / **[DESIGN_APPROVED]** |
| **MMS over TLS** | MMS 连接可选使用 TLS 1.2+，双向证书认证 | DRAFT |
| **国密算法（SM2/SM4）** | 数字签名、密钥交换、对称加密（Phase 2+） | **[REVIEWED: PASS]** |
| **证书管理** | X.509 v3 格式，支持 SM2 签名算法，PEM 文件存储 | **[REVIEWED: PASS]** |
| **密钥管理** | SM2 私钥仅本地存储禁止网络传输，SM4 密钥定期轮换（默认 30 天），文件权限 600 | **[REVIEWED: PASS]** |
| **Web UI 安全** | Session 登录认证（`POST /api/auth/login`），默认用户 admin | **[REVIEWED: PASS]** |
| **配置安全** | 敏感配置不得明文存储 | **[REVIEWED: PASS]** |
| **日志安全** | 日志中不得记录明文密码、密钥 | **[REVIEWED: PASS]** |
| **Topic 权限** | 进程间 Topic 仅允许本地服务订阅，北向 Topic 按角色分配读写权限 | **[DESIGN_APPROVED]** |
| **纵向加密认证合规** | 纵向加密认证需符合电力监控系统安全防护规定（发改委14号令），详见模块06-PRD 第5章 | **[REVIEWED: PASS]** |

### 6.4 兼容性需求

| 项目 | 要求 |
|------|------|
| 操作系统 | openEuler 22.03+ |
| 硬件平台 | RK3588 |
| 协议 | IEC 60870-5-104、IEC 61850-7-2/7-3/8-1、MQTT 3.1.1 |
| Web 浏览器 | Chrome 90+、Firefox 88+、Edge 90+ |
| 本地无线运维通道 | 星闪 / Wi-Fi / 蓝牙本地运维通信，详见模块09-PRD |

### 6.5 依赖关系

| Crate | 版本 | 用途 | 来源 |
|-------|------|------|------|
| tokio | 1.x | 异步运行时 | 通用 |
| serde | 1.x | 序列化 | 通用 |
| serde_json | 1.x | JSON 解析 | 通用 |
| thiserror | 1.x | 错误类型 | 通用 |
| libloading | 0.8 | 动态库加载 | Phase 2 |
| rumqttc | 0.20+ | MQTT 客户端 | Phase 2 / Phase 3B |
| iec61850-sys | 0.3 | MMS 协议栈 FFI 绑定 | Phase 2 |
| rustls | 0.23 | TLS 实现 | Phase 3B |
| webpki-roots | 0.26 | TLS 根证书 | Phase 3B |
| serial | 0.4 | 串口通信（RS485 依赖） | Phase 2 |
| GmSSL | (绑定) | 国密算法 | Phase 2+ |

---

## 7. 验收标准汇总

### 7.1 IEC 104 验收标准

| ID | 功能点 | 验收条件 | 来源 |
|----|--------|----------|------|
| IEC104-01 | 协议兼容 | 全面兼容 IEC 60870-5-104，支持 U/S/I 帧 | **[REVIEWED: PASS]** |
| IEC104-02 | 连接管理 | 支持状态机、心跳（10s~60s 可配置）、超时判定（30s） | **[REVIEWED: PASS]** |
| IEC104-03 | 自动重连 | 断连 5s 重试，最多 10 次，之后每 1 分钟尝试 | **[REVIEWED: PASS]** |
| IEC104-04 | TypeID 支持 | 支持 M_SP_TA_1、M_DP_TA_1、M_ME_TA_1、M_ME_TD_1、C_SC_TA_1、C_DC_TA_1、C_SE_TA_1 | **[REVIEWED: PASS]** |
| IEC104-05 | 数据上行 | 周期性遥测（默认 1s），告警立即上送，UTC 时标 | **[REVIEWED: PASS]** |
| IEC104-06 | 数据下行 | 接收确认，遥控命令需经策略引擎校验 | **[REVIEWED: PASS]** |
| IEC104-07 | 调度指令 | 支持 P_set/Q_set、电池启停、一次调频参数，响应 ≤ 100ms | **[REVIEWED: PASS]** |
| IEC104-08 | 并发连接 | 支持最多 5 个并发连接 | **[REVIEWED: PASS]** |

### 7.2 IEC 61850 / MMS 验收标准

| ID | 功能点 | 验收条件 | 来源 |
|----|--------|----------|------|
| MMS-01 | MMS Read | MMS Read 服务正常工作，读取 IED 数据对象 | DRAFT |
| MMS-02 | MMS Write | MMS Write 服务正常工作，写入 IED 数据对象 | DRAFT |
| MMS-03 | MMS TLS | MMS over TLS 连接成功（可选） | DRAFT |
| MMS-04 | 超时处理 | 超时场景下正确处理 | DRAFT |
| MMS-05 | 错误处理 | 错误响应场景下正确处理 | DRAFT |
| MMS-06 | 短连接模式 | 每次请求建立新连接 | DRAFT |
| MMS-07 | GOOSE 订阅 | 支持订阅 GOOSE 消息，通过 MessageHandler 回调处理 | **[REVIEWED: PASS]** |
| MMS-08 | IED 连接 | 能连接 IEC 61850 IED 设备，读写数据对象；超时 < 5s，成功率 ≥ 99% | **[REVIEWED: PASS]** |

### 7.3 MQTT 验收标准

| ID | 功能点 | 验收条件 | 来源 |
|----|--------|----------|------|
| MQTT-01 | Broker 连接 | 能连接 MQTT Broker，支持订阅/发布 | **[REVIEWED: PASS]** |
| MQTT-02 | QoS 0 消息 | 至多一次送达，延迟 < 100ms | **[REVIEWED: PASS]** |
| MQTT-03 | QoS 1/2 消息 | 至少一次 / 恰好一次送达，延迟 < 500ms | **[REVIEWED: PASS]** |
| MQTT-04 | MQTT over TLS | MQTT over TLS 1.2+ 连接成功，双向证书认证 | **[REVIEWED: PASS]** |
| MQTT-05 | 断线重连 | 初始 1s，指数退避（2 倍），最大 60s，不限次数 | **[DESIGN_APPROVED]** |
| MQTT-06 | 消息持久化 | QoS 2 消息持久化正确 | **[DESIGN_APPROVED]** |

### 7.4 消息总线验收标准

| ID | 功能点 | 验收条件 | 来源 |
|----|--------|----------|------|
| MB-01 | 本地 mosquitto 连接 | 本地 MQTT 连接成功 | **[DESIGN_APPROVED]** |
| MB-02 | 北向 emqx 连接（TLS） | 北向 MQTT + TLS 连接成功 | **[DESIGN_APPROVED]** |
| MB-03 | 进程间消息延迟 | < 100ms | **[DESIGN_APPROVED]** |
| MB-04 | 断线重连自动恢复 | 网络中断后自动恢复 | **[DESIGN_APPROVED]** |
| MB-05 | QoS 1 消息至少一次到达 | 单元测试验证 | **[DESIGN_APPROVED]** |
| MB-06 | QoS 2 消息恰好一次到达 | 单元测试验证 | **[DESIGN_APPROVED]** |
| MB-07 | 证书认证成功 | 集成测试验证 | **[DESIGN_APPROVED]** |
| MB-08 | 消息持久化正确 | 集成测试验证 | **[DESIGN_APPROVED]** |

### 7.5 安全验收标准

| ID | 安全点 | 验收条件 | 来源 |
|----|--------|----------|------|
| S01 | SM2 签名 | 能使用 SM2 签名和验签 | **[REVIEWED: PASS]** |
| S02 | SM4 加密 | 能使用 SM4 加密和解密 | **[REVIEWED: PASS]** |
| S03 | 证书验证 | 双向证书认证功能正常 | **[REVIEWED: PASS]** |
| S04 | MQTT TLS | MQTT over TLS 连接成功 | **[REVIEWED: PASS]** |
| S05 | MMS TLS | MMS over TLS 连接成功 | DRAFT |
| S06 | 无硬编码密钥 | 检查 SM2/SM4 密钥残留 | **[REVIEWED: PASS]** |
| S07 | 无新增 unsafe 块 | Code review 检查 | **[REVIEWED: PASS]** |
| S08 | 错误实现 std::error::Error | 所有错误类型符合标准 | **[REVIEWED: PASS]** |

### 7.6 质量验收标准

| ID | 质量点 | 验收条件 | 来源 |
|----|--------|----------|------|
| Q01 | 编译通过 | `cargo build --release` 无警告 | **[REVIEWED: PASS]** |
| Q02 | Clippy 检查 | `cargo clippy` 无 Error | **[REVIEWED: PASS]** |
| Q03 | 单元测试 | `cargo test` 覆盖率 ≥ 80% | **[REVIEWED: PASS]** |
| Q04 | 错误处理 | 所有错误实现 `std::error::Error` | **[REVIEWED: PASS]** |
| Q05 | 文档 | 公共 API 有 rustdoc 注释 | **[REVIEWED: PASS]** |
| Q06 | 格式化 | `cargo fmt` 格式化通过 | **[REVIEWED: PASS]** |

---

## 附录 A：术语表

| 术语 | 说明 |
|------|------|
| MUPC | 微电网特种调控装置 |
| IEC 104 | IEC 60870-5-104，远动协议 |
| IEC 61850 | 变电站通信协议 |
| MMS | 制造报文规范（Manufacturing Message Specification） |
| GOOSE | 面向通用对象的变电站事件（Generic Object Oriented Substation Event） |
| ASN.1 | 抽象语法标记（Abstract Syntax Notation One） |
| QoS | 服务质量（Quality of Service） |
| MQTT | 物联网消息队列协议 |
| TLS | 传输层安全协议 |
| IED | 智能电子设备（Intelligent Electronic Device） |
| VPP | 虚拟电厂 |
| emqx | 企业级 MQTT Broker |
| mosquitto | 轻量开源 MQTT Broker |
| TTU | 台区智能融合终端 |
| SOC | 电池荷电状态 |
| SOH | 电池健康状态 |

## 附录 B：来源文档索引

| 文档 | 路径 | 评审状态 |
|------|------|----------|
| 项目需求主文档 | `docs/superpowers/specs/PROJECT-MUPC-项目需求主文档.md` | **[REVIEWED: PASS]** |
| 技术债清单 | `docs/technical-debt.md` | — |

---

## 附录 C：v1.1 修订记录

| 编号 | 缺口 | 风险等级 | 修改位置 | 修改内容 |
|------|------|---------|---------|---------|
| 1 | 星闪/Wi-Fi/蓝牙本地运维通道 | 高 | 1.4 用户角色 + 6.4 兼容性需求 | 本地运维人员角色补充无线运维通道说明；6.4 新增"本地无线运维通道"行，引用模块09-PRD |
| 2 | 纵向加密认证合规 | 高 | 6.3 安全需求 | 新增"纵向加密认证合规"行，引用发改委14号令及模块06-PRD 第5章 |
| 3 | 看门狗监控实时控制模块 | 高 | 1.1 核心职责 + 6.2 可靠性需求 | 核心职责补充"通过核间通信监控实时控制模块运行状态"；6.2 新增"系统级看门狗"行，引用模块10-PRD 第4章 |
| 4 | IEC 61850-7-420 DER 逻辑节点 | 中 | 3.1 概述 | 补充说明：完整 7-420 DER 逻辑节点模型延后至 Phase 2+ 实现，Phase 2 实现 MMS 传输层基础读写能力 |
| 5 | OTA 远程升级引用 | 中 | 1.1 核心职责 | 新增"OTA 远程升级（模型 + 固件）"条目，引用模块07-PRD |

**修订说明：** 本次修订（v1.0 → v1.1）修复总报告中模块 01 PRD 的 5 项部分覆盖缺口，均采用"补充引用"方式，未修改已有内容逻辑。

---

**文档状态：** 合并修订版（v1.1）
**合并说明：** 合并四份来源文档中与通信网关模块（gateway、iec61850-plugin、mqtt-plugin、mqtt-bridge、device-trait）相关的需求，已去重。所有 **[REVIEWED: PASS]** 标记的功能需求已完整保留。
