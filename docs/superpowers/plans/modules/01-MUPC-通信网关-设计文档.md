# MUPC 通信网关模块 — 综合技术设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.1 | 2026-05-29 | 架构师 | **[DESIGN_APPROVED]** |

**来源文档：**
- `docs/superpowers/plans/2026-05-28-IEC61850-MMS-客户端真正实现-实施计划.md` — IEC 61850 MMS 实施计划
- `docs/superpowers/plans/2026-05-28-MUPC-Phase3B-实施计划.md` — Phase 3B 实施计划
- `docs/superpowers/specs/modules/01-MUPC-通信网关-PRD.md` — 通信网关 PRD

**涵盖 Crate：** `gateway`, `iec61850-plugin`, `mqtt-plugin`, `mqtt-bridge`, `device-trait`

---

## 1. 模块架构

### 1.1 模块定位

通信网关是 MUPC 微电网特种调控装置"异构双核心模块主控架构"中**非实时处理核心**（大脑）的北向通信子系统。负责与调度主站、配电自动化主站、物联平台等外部系统建立和维护通信连接，完成数据的上报和指令的接收转发。

**核心职责：**
- 北向 IEC 104 协议通信：与调度主站互通（遥测/遥信/遥控）
- 北向 IEC 61850 MMS 协议通信：与配电自动化 IED 设备互通
- 北向 MQTT 协议通信：与物联平台 / 虚拟电厂（VPP）互通
- 消息总线：进程间通信（本地）与北向消息路由（云端）
- 协议转换与数据格式统一
- 连接管理、心跳保活、自动重连
- 通过核间通信监控实时控制模块运行状态（系统级看门狗），详见模块10-PRD 第4章
- OTA 远程升级（模型 + 固件），详见模块07-PRD
- 纵向加密认证合规（发改委14号令），详见模块06-PRD 第5章

### 1.2 整体架构

```
调度主站 (IEC 104)      配电自动化 (IEC 61850)      物联平台 (MQTT)
        │                        │                       │
        ▼                        ▼                       ▼
┌───────────────────────────────────────────────────────────────┐
│                      gateway crate                             │
│  ┌─────────────────┐  ┌──────────────────┐  ┌──────────────┐  │
│  │  iec104 server  │  │ iec61850-plugin  │  │ mqtt-plugin  │  │
│  │  (服务端:2404)  │  │ (MMS 客户端)     │  │ (北向客户端)  │  │
│  └────────┬────────┘  └────────┬─────────┘  └──────┬───────┘  │
│           │                    │                    │          │
│           └────────────────────┴────────────────────┘          │
│                              │                                 │
│                        ┌─────▼──────┐                          │
│                        │ MessageBus │                          │
│                        │  (trait)   │                          │
│                        └─────┬──────┘                          │
└──────────────────────────────┼─────────────────────────────────┘
                               │
          ┌────────────────────┼────────────────────┐
          ▼                    ▼                    ▼
   ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
   │data-processing│    │strategy-engine│    │  mqtt-bridge  │
   │  (遥测采集)   │    │  (策略引擎)   │    │(本地mosquitto)│
   └──────────────┘    └──────────────┘    └──────────────┘
          │                    │                    │
          └────────────────────┼────────────────────┘
                               ▼
                        ┌──────────────┐
                        │  intercore   │
                        │  (TCP/RJ45)  │
                        └──────┬───────┘
                               │
                               ▼
                        ┌──────────────┐
                        │ 实时控制模块  │
                        └──────────────┘
```

### 1.3 Crate 职责与状态

| Crate | 职责 | 状态 |
|-------|------|------|
| `gateway` | IEC 104 服务端，接收调度主站连接 | Phase 1 ✅ |
| `iec61850-plugin` | IEC 61850 MMS 客户端，连接 IED 设备 | Phase 2+ ✅ |
| `mqtt-plugin` | MQTT 北向客户端，连接物联平台 emqx | Phase 2+ ✅ |
| `mqtt-bridge` | 分层 MQTT 网桥（本地 mosquitto + 北向 emqx） | Phase 3B ✅ **[DESIGN_APPROVED]** |
| `device-trait` | 核心 trait 定义（MessageBus、Plugin、Device 等） | Phase 2+ ✅ |

### 1.4 目标平台

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux (openEuler 22.03+) |
| 硬件 | RK3588 |
| 编程语言 | Rust (>= 1.75) |
| 异步运行时 | Tokio 1.x |
| 网络框架 | Tower 0.5 + tokio-net |
| 内存限制 | < 50MB |
| 冷启动时间 | < 3s |

### 1.5 用户角色与权限

| 角色 | 协议 | 权限范围 | 优先级 |
|------|------|----------|--------|
| 调度主站运维人员 | IEC 104 | 只读监控+调度指令（经策略引擎校验） | 中 |
| 配电自动化操作员 | IEC 61850 MMS | 设备状态监控、遥控操作 | 中 |
| 物联平台管理员 | MQTT | 数据上报、指令接收 | 中 |
| 本地运维人员 | HTTP (Web UI) / 星闪 / Wi-Fi / 蓝牙 | 最高权限，可覆盖调度指令 | **最高** |
| 系统管理员 | 所有协议 | 所有配置权限 | 高 |

**权限冲突规则：** 本地运维人员指令优先于所有北向指令。北向指令需经本地方略引擎校验后才转发。

> 本地无线运维通道（星闪 / Wi-Fi / 蓝牙）由模块09负责实现，详见模块09-PRD。

---

## 2. IEC 104 协议实现设计

> 来源：主技术设计 v1.1 Section 2.1/3.3/4.1，PRD Section 2
> 状态：Phase 1 ✅ 已实现

### 2.1 架构设计

IEC 104 网关以**服务端模式**运行，监听 TCP 端口（默认 2404），接受调度主站连接。采用 `Iec104Server` + `Connection` + `Iec104Frame` 三层结构：

```
Iec104Server (TcpListener)
      │
      │ accept()
      ▼
Connection state machine
      │
      ▼
Iec104Frame (protocol parse/encode)
      │
      ├── UFrame: STARTDT/STOPDT/TESTFR
      ├── SFrame: I-frame ACK
      └── IFrame: ASDU data (telemetry/command)
```

### 2.2 数据结构定义

#### 帧格式

IEC 104 帧结构（最小 6 字节）：

```
┌──────┬────────┬────────┬────────┬────────┬──────────┐
│ 0x68 │ Length │ Control│ Control│ Control│ Control  │
│      │        │  1     │  2     │  3     │  4       │
├──────┼────────┼────────┼────────┼────────┼──────────┤
│ u8   │ u8     │ u8     │ u8     │ u8     │ u8       │
└──────┴────────┴────────┴────────┴────────┴──────────┘
```

#### 帧类型枚举 (protocol.rs)

```rust
pub enum FrameType {
    IFrame,  // 编号的信息传输帧
    SFrame,  // 确认帧
    UFrame,  // 控制帧
}

pub enum UFrameType {
    StartDtAct,   // 启动数据传输激活
    StartDtCon,   // 启动数据传输确认
    StopDtAct,    // 停止数据传输激活
    StopDtCon,    // 停止数据传输确认
    TestFrAct,    // 测试帧激活
    TestFrCon,    // 测试帧确认
}
```

#### 类型标识 (TypeId)

支持的 TypeID（代码位置：`gateway/src/iec104/protocol.rs` line 29-46）：

| 方向 | TypeId | 值 | 说明 |
|------|--------|-----|------|
| 监视 | `MSpNa1` | 1 | 单点遥信 (M_SP_NA_1) |
| 监视 | `MDpNa1` | 3 | 双点遥信 (M_DP_NA_1) |
| 监视 | `MMeNa1` | 9 | 测量值，归一化值 (M_ME_NA_1) |
| 监视 | `MMeNc1` | 13 | 测量值，短浮点数 (M_ME_NC_1) |
| 监视 | `MSpTa1` | 30 | 单点遥信带时标 (M_SP_TA_1) |
| 监视 | `MDpTa1` | 31 | 双点遥信带时标 (M_DP_TA_1) |
| 监视 | `MMeTa1` | 34 | 测量值带时标，归一化值 (M_ME_TA_1) |
| 监视 | `MMeTd1` | 35 | 测量值带时标，归一化值 (M_ME_TD_1) |
| 控制 | `CScNa1` | 45 | 单点遥控 (C_SC_NA_1) |
| 控制 | `CDcNa1` | 46 | 双点遥控 (C_DC_NA_1) |
| 控制 | `CSeNa1` | 48 | 调节命令 (C_SE_NA_1) |
| 控制 | `CScTa1` | 58 | 单点遥控带时标 (C_SC_TA_1) |
| 控制 | `CDcTa1` | 59 | 双点遥控带时标 (C_DC_TA_1) |
| 控制 | `CSeTa1` | 61 | 调节命令带时标 (C_SE_TA_1) |

#### 数据值

```rust
pub enum Value {
    SinglePoint(bool),      // 单点 (开/关)
    DoublePoint(u8),        // 双点 (00=中间,01=开,10=关,11=无效)
    Normalized(f64),        // 归一化值 (-1.0 ~ 1.0)
    Scaled(i16),            // 标度化值
    Float(f64),             // 短浮点数
}
```

#### ASDU 头

```rust
pub struct AsduHeader {
    pub type_id: TypeId,
    pub sq_num: u8,
    pub cot: Cot,          // 传输原因
    pub orig_addr: u16,    // 源站地址
}
```

#### 时标规范

所有带时标的 TypeID（M_SP_TA_1、M_DP_TA_1、M_ME_TA_1、M_ME_TD_1、C_SC_TA_1、C_DC_TA_1、C_SE_TA_1）使用时标字段遵循以下规范：

- **时间基准：** 所有时标字段使用 UTC 时间
- **精度：** 毫秒级（milliseconds since epoch）
- **编码：** 按 IEC 60870-5-4 标准的 CP56Time2a 格式编码（7 字节）

#### 连接状态机

```rust
pub enum ConnectionState {
    Disconnected,
    Connecting,
    WaitingStartDt,    // 等待 STARTDT
    Connected,
    Stopped,
}
```

### 2.3 服务器实现 (server.rs)

`Iec104Server` 结构：

```rust
pub struct Iec104Server {
    config: Iec104Config,
    connections: Arc<RwLock<Vec<Arc<RwLock<Connection>>>>>,
    shutdown_tx: broadcast::Sender<()>,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new(config)` | 创建服务器实例 |
| `start(command_handler)` | 启动 TCP 监听，接受连接 |
| `shutdown()` | 停止服务器，清理所有连接 |
| `connection_count()` | 获取当前连接数 |

**连接处理流程：**
1. `accept()` 接受新 TCP 连接
2. 检查并发连接数（最大 5 个）
3. 创建 `Connection` 实例，添加到连接池
4. 启动异步任务 `handle_connection()` 处理帧
5. 帧解析 → 状态机处理 → 响应

### 2.4 连接管理 (connection.rs)

`Connection` 结构：

```rust
pub struct Connection {
    pub stream: TcpStream,
    pub addr: SocketAddr,
    pub state: ConnectionState,
    pub send_seq: u16,
    pub recv_seq: u16,
    pub heartbeat_interval_secs: u64,
}
```

**U 帧处理：**
- `STARTDT_ACT` → `STARTDT_CON`，状态迁移到 `Connected`
- `STOPDT_ACT` → `STOPDT_CON`，状态迁移到 `Stopped`
- `TESTFR_ACT` → `TESTFR_CON`

**I 帧处理：**
- 序列号校验（`send_seq` / `recv_seq`）
- 发送 S 帧确认
- ASDU 解析

### 2.5 命令处理 (command.rs)

```rust
pub struct ControlCommand {
    pub cmd_id: u16,
    pub cmd_type: CommandType,  // SwitchControl | PowerRegulation | ChargeDischarge
    pub p_set: Option<f64>,     // 有功设定值 (kW)
    pub q_set: Option<f64>,     // 无功设定值 (kVar)
    pub switch_state: Option<bool>,
    pub k_value: Option<f64>,   // 一次调频 K 值
    pub deadband: Option<f64>,  // 一次调频死区 (Hz)
    pub priority: u8,
}

#[async_trait]
pub trait CommandHandler: Send + Sync {
    async fn handle_command(&self, cmd: ControlCommand) -> Result<CommandResponse, MupcError>;
    fn name(&self) -> &str;
}
```

### 2.6 配置定义

```rust
pub struct Iec104Config {
    pub listen_addr: String,               // "0.0.0.0"
    pub listen_port: u16,                  // 2404
    pub heartbeat_interval_secs: u64,      // 默认 10s
    pub connection_timeout_ms: u64,        // 默认 30000ms
    pub max_connections: usize,            // 默认 5
}
```

### 2.7 性能参数

| 指标 | 要求 |
|------|------|
| 数据上报周期 | ≥ 1Hz（可配置） |
| 调度指令处理延迟（接收→转发） | ≤ 100ms |
| 并发连接数 | ≤ 5 |
| 心跳间隔 | 默认 10s，可配置 1s~60s |
| 连接超时判定 | 30 秒无响应 |
| 自动重连 | 断连 5s 后开始，最多 10 次，之后每 1 分钟尝试 |

---

## 3. IEC 61850 MMS 客户端设计

> 来源：IEC61850 MMS 实施计划 Tasks 1-8，PRD Section 3
> 状态：Phase 2+ ✅ 已实现

### 3.1 架构概述

采用 **libIEC61850 C 库 + Rust FFI 绑定** 实现真正的 MMS 协议栈。采用**短连接模式**（每次请求建立新连接），支持 MMS over TLS。

> **IEC 61850-7-420 DER 逻辑节点说明：** 完整的 IEC 61850-7-420 DER 逻辑节点模型（如 Photovoltaic、Storage、ElectricVehicle 等）延后至 Phase 2+ 实现。Phase 2 实现 MMS 传输层基础读写能力（7-2/7-3/8-1），支持标准 7-4 逻辑节点。

```
┌─────────────────────────────────────────┐
│            mms_client.rs                 │
│  ┌─────────────────────────────────┐    │
│  │       MmsClient                 │    │
│  │  - connect() / disconnect()     │    │
│  │  - read_do(ln, do_name)         │    │
│  │  - write_do(ln, do_name, value) │    │
│  └──────────┬──────────────────────┘    │
│             │                            │
│  ┌──────────▼──────────────────────┐    │
│  │      asn1_utils.rs              │    │
│  │  - encode_mms_request()         │    │
│  │  - decode_mms_response()        │    │
│  └─────────────────────────────────┘    │
└─────────────────────────────────────────┘
```

### 3.2 技术选型

| 特性 | 说明 |
|------|------|
| 协议栈 | libIEC61850 C 库 + iec61850-sys FFI 绑定 |
| 标准 | IEC 61850-7-2/7-3/8-1 (MMS) |
| 连接模式 | 短连接（每次请求建立新连接） |
| 默认端口 | 102 |
| TLS | MMS over TLS 1.2+（可选） |
| 依赖 | `iec61850-sys` (0.3, features: ["tls"]) |
| 证书 | `rustls` (0.23)、`webpki-roots` (0.26) |

### 3.3 连接状态机

```rust
pub enum MmsClientState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}
```

短连接模式：每次 `read_do()` / `write_do()` 调用先建立 TCP 连接，请求完成后断开。

### 3.4 MMS 数据类型 (mms_types.rs)

```rust
/// MMS 数据对象
pub struct DataObject {
    pub ln: String,        // 逻辑节点名（如 "LLN0", "MMXU1"）
    pub do_name: String,   // 数据对象名（如 "ST$Pos", "MX$Meas"）
}

/// MMS 服务类型
pub enum MmsService {
    Read,
    Write,
    DefineVariableAccess,   // 预留
    GetDataAccessAttributes, // 预留
}

/// MMS 请求
pub struct MmsRequest {
    pub service: MmsService,
    pub object: DataObject,
    pub payload: Vec<u8>,
}

/// MMS 响应
pub struct MmsResponse {
    pub success: bool,
    pub data: Vec<u8>,
    pub error: Option<String>,
}
```

便捷构造方法：
- `MmsRequest::read(ln, do_name)` — 创建 Read 请求
- `MmsRequest::write(ln, do_name, value)` — 创建 Write 请求
- `MmsResponse::success(data)` / `MmsResponse::error(msg)` — 创建响应

### 3.5 ASN.1 编码/解码 (asn1_utils.rs)

```rust
/// 编码 MMS 请求为 ASN.1 BER 格式
pub fn encode_mms_request(request: &MmsRequest) -> Result<Vec<u8>>;

/// 解码 ASN.1 BER 响应
pub fn decode_mms_response(data: &[u8]) -> Result<MmsResponse>;
```

**MMS PDU 标签：**

| 标签 | 说明 |
|------|------|
| `CONFIRMED_REQUEST_PDU` (0x01) | 确认请求 PDU |
| `CONFIRMED_RESPONSE_PDU` (0x02) | 确认响应 PDU |
| `CONFIRMED_ERROR_PDU` (0x03) | 确认错误 PDU |
| `UNCONFIRMED_PDU` (0x04) | 非确认 PDU |
| `REJECTED_PDU` (0x05) | 拒绝 PDU |

### 3.6 MMS 客户端实现 (mms_client.rs)

```rust
pub struct MmsClient {
    config: MmsConfig,
    state: Arc<parking_lot::RwLock<MmsClientState>>,
}
```

**核心方法：**

| 方法 | 说明 |
|------|------|
| `new(config)` | 创建 MMS 客户端 |
| `connect()` | 连接到 IED（短连接模式） |
| `disconnect()` | 断开连接 |
| `read_do(ln, do_name)` | 读取数据对象 |
| `write_do(ln, do_name, value)` | 写入数据对象 |
| `get_state()` | 获取客户端状态 |

**请求流程：**
1. 建立 TCP 连接到 IED（支持超时配置）
2. 使用 ASN.1 编码请求
3. 发送请求（`write_all`）
4. 读取响应（支持超时配置）
5. 使用 ASN.1 解码响应
6. 断开连接

### 3.7 MMS Trait (mms_client.rs)

```rust
#[async_trait]
pub trait MmsClientTrait: Send + Sync {
    async fn connect(&self) -> Result<()>;
    fn disconnect(&self);
    fn get_state(&self) -> MmsClientState;
    async fn read_do(&self, ln: &str, do_name: &str) -> Result<Vec<u8>>;
    async fn write_do(&self, ln: &str, do_name: &str, value: &[u8]) -> Result<()>;
}
```

### 3.8 GOOSE 消息订阅 (goose.rs)

| 功能 | 说明 |
|------|------|
| 订阅 GOOSE 消息 | 通过 GOOSE ID (`go_id`) 识别消息源 |
| 回调处理 | 消息通过 MessageHandler 回调 |
| 配置参数 | 本地 IP、本地端口、远端 IP、远端端口 |

### 3.9 配置定义

```rust
/// MMS 配置
pub struct MmsConfig {
    pub local_ip: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,              // 默认 102
    pub max_connections: u32,
    pub connect_timeout_ms: u64,       // 默认 5000
    pub read_timeout_ms: u64,          // 默认 3000
    pub tls: Option<MmsTlsConfig>,
}

/// MMS TLS 配置
pub struct MmsTlsConfig {
    pub enabled: bool,
    pub ca_cert_path: String,
    pub client_cert_path: String,
    pub client_key_path: String,
    pub verify_peer: bool,
}
```

### 3.10 错误类型

| 错误 | 说明 |
|------|------|
| `MmsConnectFailed` | MMS 连接失败（含 TLS 连接失败） |
| `MmsTimeout` | MMS 请求超时（连接/读取） |
| `MmsProtocolError` | MMS 协议错误 |
| `MmsInvalidResponse` | 无效响应 |
| `DataObjectNotFound` | 数据对象不存在 |
| `WriteFailed` | 写操作失败 |
| `TlsConnectFailed` | TLS 连接失败 |
| `CertVerifyFailed` | 证书验证失败 |
| `Asn1EncodeFailed` | ASN.1 编码失败 |
| `Asn1DecodeFailed` | ASN.1 解码失败 |
| `LibIec61850Error` | libIEC61850 底层错误 |

### 3.11 性能要求

| 指标 | 要求 |
|------|------|
| MMS 连接建立时间 | ≤ 5s |
| Read 请求响应时间 | ≤ 3s |
| Write 请求响应时间 | ≤ 3s |
| 操作成功率 | ≥ 99% |
| 并发连接 | ≥ 1 个 IED（可扩展） |

---

## 4. MQTT 通信设计

> 来源：Phase 3B 消息总线设计文档 Section 3-6（[DESIGN_APPROVED]），
>       mqtt-plugin 现有代码（Phase 2+ ✅），
>       PRD Section 4
> 状态：Phase 2+（mqtt-plugin）✅ + Phase 3B（mqtt-bridge）✅ **[DESIGN_APPROVED]**

### 4.1 分层 MQTT 架构

MQTT 功能分为两个层次：

1. **MQTT 北向客户端（mqtt-plugin）**：与物联平台 / VPP 通信，基于 emqx 企业级 MQTT Broker
2. **MQTT 本地网桥（mqtt-bridge）**：进程间通信，基于本地 mosquitto Broker

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

### 4.2 MQTT 北向客户端 (mqtt-plugin)

> 状态：Phase 2+ ✅ 已实现

#### 核心结构

```rust
pub struct MqttClient {
    config: MqttConfig,
    inner: AsyncClient,                    // rumqttc
    state: Arc<RwLock<MqttClientState>>,
    event_tx: broadcast::Sender<Event>,
}
```

#### 连接状态

```rust
pub enum MqttClientState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}
```

#### 核心方法

| 方法 | 说明 |
|------|------|
| `new(config)` | 创建客户端（自动启动事件循环） |
| `connect()` | 连接到 MQTT Broker |
| `disconnect()` | 断开连接 |
| `subscribe(topic, qos)` | 订阅主题 |
| `publish(topic, payload, qos, retain)` | 发布消息 |
| `get_state()` | 获取客户端状态 |

#### QoS 枚举

```rust
pub enum MqttQos {
    AtMostOnce = 0,   // QoS 0
    AtLeastOnce = 1,  // QoS 1
    ExactlyOnce = 2,  // QoS 2
}
```

#### 配置

```rust
pub struct MqttConfig {
    pub broker_addr: String,
    pub client_id: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub use_tls: bool,
    pub ca_cert: Option<String>,
    pub client_cert: Option<String>,
    pub client_key: Option<String>,
    pub qos: MqttQos,
    pub keepalive_secs: u16,
    pub clean_session: bool,
}
```

#### 错误类型

| 错误 | 说明 |
|------|------|
| `ConnectFailed` | 连接失败 |
| `AuthFailed` | 认证失败 |
| `SubscribeFailed` | 订阅失败 |
| `PublishFailed` | 发布失败 |
| `Disconnected` | 连接已断开 |
| `TlsConfigError` | TLS 配置错误 |
| `QosNotSupported` | QoS 不支持 |
| `ProtocolError` | 协议错误 |

#### TLS 配置

支持 MQTT over TLS 1.2+，双向证书认证：
- CA 证书（`ca_cert`）
- 客户端证书（`client_cert`）
- 客户端私钥（`client_key`）

#### 断线重连策略

| 参数 | 值 |
|------|-----|
| 初始间隔 | 1 秒 |
| 指数退避 | 2 倍 |
| 最大间隔 | 60 秒 |
| 重连次数 | 无上限 |

### 4.3 MQTT 本地网桥 (mqtt-bridge)

> 状态：Phase 3B ✅ **[DESIGN_APPROVED]**

#### MqttBridge Trait

```rust
#[async_trait]
pub trait MqttBridge: Send + Sync {
    async fn publish(&self, topic: &str, payload: &[u8], qos: u8) -> Result<(), MqttBridgeError>;
    async fn subscribe(&self, topic: &str, qos: u8) -> Result<(), MqttBridgeError>;
    fn is_connected(&self) -> bool;
}
```

#### LocalMqttClient

- 连接本地 mosquitto（`127.0.0.1:1883`）
- 用于进程间通信
- QoS 0/1
- 无 TLS

```rust
pub struct LocalMqttClient {
    client: AsyncClient,
    eventloop: Arc<Mutex<EventLoop>>,
    connected: Arc<Mutex<bool>>,
}
```

#### NorthMqttClient

- 连接 emqx（可配置地址 `:8883`）
- 用于北向通信
- QoS 1/2
- TLS + 双向证书认证

```rust
pub struct NorthMqttClient {
    client: AsyncClient,
    eventloop: Arc<Mutex<EventLoop>>,
    connected: Arc<Mutex<bool>>,
}
```

#### mqtt-bridge 配置

```rust
pub struct MqttConfig {
    pub local: LocalMqttConfig,
    pub north: NorthMqttConfig,
}

pub struct LocalMqttConfig {
    pub broker_addr: String,       // "127.0.0.1:1883"
    pub client_id: String,
    pub clean_session: bool,
    pub keepalive_secs: u64,
    pub reconnect: ReconnectConfig,
}

pub struct NorthMqttConfig {
    pub broker_addr: String,       // "mqtt.example.com:8883"
    pub client_id: String,
    pub keepalive_secs: u64,
    pub tls: TlsConfig,
    pub reconnect: ReconnectConfig,
}

pub struct TlsConfig {
    pub ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}

pub struct ReconnectConfig {
    pub initial_interval_secs: u64,   // 默认 1s
    pub max_interval_secs: u64,       // 默认 60s
    pub backoff_multiplier: f64,      // 默认 2.0
}
```

#### mqtt-bridge 错误类型

```rust
pub enum MqttBridgeError {
    ConnectionFailed(String),
    SubscribeFailed(String),
    PublishFailed(String),
    TlsError(String),
    CertificateError(String),
    Disconnected(String),
    MaxReconnectAttemptsReached(usize),
    Timeout(String),
}
```

---

## 5. 消息总线设计

> 来源：Phase 3B 消息总线设计文档 Section 2（[DESIGN_APPROVED]），
>       device-trait/src/message_bus.rs（已实现），
>       PRD Section 5
> 状态：Phase 1（mpsc）✅ + Phase 3B（MQTT）✅ **[DESIGN_APPROVED]**

### 5.1 MessageBus Trait

定义于 `device-trait/src/message_bus.rs`：

```rust
/// 消息总线接口
pub trait MessageBus: Send + Sync {
    fn publish(&self, msg: Message) -> Result<(), BusError>;
    fn subscribe(&self, topic: Topic, handler: Arc<dyn MessageHandler>) -> Result<(), BusError>;
    fn unsubscribe(&self, topic: &Topic) -> Result<(), BusError>;
    fn subscriber_count(&self, topic: &Topic) -> usize;
    fn subscribed_topics(&self) -> Vec<Topic>;
}
```

### 5.2 消息处理器

```rust
pub trait MessageHandler: Send + Sync {
    fn handle(&self, msg: Message);
}
```

### 5.3 数据类型

```rust
/// 消息主题
pub struct Topic(String);

/// 消息封装
pub struct Message {
    pub topic: Topic,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}
```

### 5.4 错误类型

```rust
pub enum BusError {
    TopicNotFound(String),
    PublishFailed(String),
    SubscribeFailed(String),
    UnsubscribeFailed(String),
    Other(String),
}
```

### 5.5 Topic 定义

#### 北向 Topic（emqx）

> 来源：Phase 3B 设计文档 Section 2.2 **[DESIGN_APPROVED]**

| 常量 | Topic | 方向 | QoS | 说明 |
|------|-------|------|-----|------|
| `NORTH_TELEMETRY` | `mupc/north/telemetry` | → 物联平台 | 1 | 高频遥测数据 |
| `NORTH_FAULT` | `mupc/north/fault` | → 物联平台 | 2 | 故障事件 |
| `NORTH_STRATEGY_COMMAND` | `mupc/north/strategy/command` | ← 物联平台 | 2 | 下行指令 |
| `NORTH_STATUS` | `mupc/north/status` | ↔ 双方 | 0 | 设备状态 |

#### 进程间 Topic（mosquitto）

> 来源：Phase 3B 设计文档 Section 2.2 **[DESIGN_APPROVED]**

| 常量 | Topic | 方向 | QoS | 说明 |
|------|-------|------|-----|------|
| `LOCAL_TELEMETRY` | `mupc/local/telemetry` | → | 0 | 遥测数据 |
| `LOCAL_STRATEGY_COMMAND` | `mupc/local/strategy/command` | → | 1 | 策略指令 |
| `LOCAL_AI_READY` | `mupc/local/ai/ready` | → | 0 | AI 就绪状态 |

常量定义位置：`mqtt-bridge/src/topics.rs`

### 5.6 实现策略演进

| Phase | 实现方式 | 说明 |
|-------|----------|------|
| Phase 1 | `tokio::sync::mpsc` | 进程内通信，最简实现 |
| Phase 2+ | device-trait::MessageBus trait | 可替换为 AMQP/MQTT |
| Phase 3B | mosquitto（进程间）+ emqx（北向） | 分层 MQTT 总线架构 |

### 5.7 数据流设计

#### 遥测数据流

```
intercore → DataCollector → LocalMqttClient → mosquitto (本地)
                                                   ↓
                                           NorthMqttClient
                                                   ↓
                                            emqx (云端)
                                                   ↓
                                            物联平台
```

#### 策略指令流

```
物联平台 → emqx → NorthMqttClient → LocalMqttClient → mosquitto
                                                            ↓
                                                  AiCommandValidator
                                                            ↓
                                                  intercore → 实时控制模块
```

### 5.8 消息持久化策略

> 来源：Phase 3B 设计文档 Section 4.3 **[DESIGN_APPROVED]**

| 消息类型 | QoS | 持久化 | 说明 |
|---------|-----|--------|------|
| 遥测数据 | 1 | 否 | 仅实时展示 |
| 故障事件 | 2 | 是 | 需要事后分析 |
| 策略指令 | 2 | 是 | 断线重连恢复 |
| 设备状态 | 0 | 否 | 周期性刷新 |

### 5.9 性能要求

| 指标 | Phase 1 (mpsc) | Phase 3B (MQTT) |
|------|----------------|------------------|
| 消息总线吞吐量 | ≥ 1000 msg/s | ≥ 10000 msg/s |
| 进程间消息延迟 | N/A | < 100ms |

---

## 6. 接口定义

### 6.1 插件接口 (Plugin)

定义于 `device-trait/src/plugin.rs`：

```rust
pub trait Plugin: Send + Sync {
    fn meta(&self) -> PluginMeta;
    fn init(&self, config: serde_json::Value) -> Result<(), PluginError>;
    fn start(&self) -> Result<(), PluginError>;
    fn stop(&self) -> Result<(), PluginError>;
    fn shutdown(self: Box<Self>) -> Result<(), PluginError>;
}

pub enum PluginState {
    Loaded,
    Initialized,
    Running,
    Stopped,
    Unloaded,
}
```

### 6.2 插件加载器 (PluginLoader)

定义于 `device-trait/src/plugin_loader.rs`：

```rust
pub trait PluginLoader: Send + Sync {
    fn load(&self, plugin_path: &str, config: serde_json::Value) -> Result<(), PluginError>;
    fn unload(&self, plugin_name: &str) -> Result<(), PluginError>;
    fn list(&self) -> Vec<PluginMeta>;
    fn get(&self, plugin_name: &str) -> Option<Arc<dyn Plugin>>;
    fn is_loaded(&self, plugin_name: &str) -> bool;
    fn plugin_count(&self) -> usize;
    fn unload_all(&self) -> Result<(), PluginError>;
}
```

### 6.3 插件元信息

```rust
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}
```

### 6.4 FFI 导出规范

动态插件（.so/.dll）需导出以下函数：

```rust
// 插件工厂
#[no_mangle]
pub extern "C" fn create_plugin() -> *mut dyn Plugin;

// 插件元信息
#[no_mangle]
pub extern "C" fn plugin_meta() -> PluginMeta;
```

**插件生命周期：** Load → Init → Start → Stop → Unload

### 6.5 设备注册表 (DeviceRegistry)

定义于 `device-trait/src/registry.rs`：

```rust
pub trait DeviceRegistry: Send + Sync {
    fn register(&self, device: Arc<dyn Device>) -> Result<(), RegistryError>;
    fn unregister(&self, device_id: &str) -> Result<(), RegistryError>;
    fn get(&self, device_id: &str) -> Option<Arc<dyn Device>>;
    fn query_by_type(&self, device_type: &str) -> Vec<Arc<dyn Device>>;
    fn list_all(&self) -> Vec<String>;
    fn count(&self) -> usize;
    fn clear(&self) -> Result<(), RegistryError>;
}
```

### 6.6 MessageBus 接口

参见本文档 [第 5 章](#5-消息总线设计)。

### 6.7 MqttBridge 接口

参见本文档 [4.3 节](#43-mqtt-本地网桥-mqtt-bridge)。

### 6.8 IEC 104 命令处理器

```rust
#[async_trait]
pub trait CommandHandler: Send + Sync {
    async fn handle_command(&self, cmd: ControlCommand) -> Result<CommandResponse, MupcError>;
    fn name(&self) -> &str;
}

pub struct ControlCommand {
    pub cmd_id: u16,
    pub cmd_type: CommandType,
    pub p_set: Option<f64>,
    pub q_set: Option<f64>,
    pub switch_state: Option<bool>,
    pub k_value: Option<f64>,   // 一次调频 K 值
    pub deadband: Option<f64>,  // 一次调频死区 (Hz)
    pub priority: u8,
}
```

### 6.9 MMS Client Trait

```rust
#[async_trait]
pub trait MmsClientTrait: Send + Sync {
    async fn connect(&self) -> Result<()>;
    fn disconnect(&self);
    fn get_state(&self) -> MmsClientState;
    async fn read_do(&self, ln: &str, do_name: &str) -> Result<Vec<u8>>;
    async fn write_do(&self, ln: &str, do_name: &str, value: &[u8]) -> Result<()>;
}
```

---

## 7. 文件结构

### 7.1 gateway crate

```
mupc/crates/gateway/src/
├── lib.rs                  # 模块导出，pub use iec104::*
└── iec104/
    ├── mod.rs              # 子模块声明
    ├── protocol.rs         # Iec104Frame 帧解析/编码，TypeId，Cot，Value
    ├── server.rs           # Iec104Server TCP 服务器
    ├── connection.rs       # Connection 连接管理，状态机，帧处理
    └── command.rs          # CommandHandler trait，ControlCommand
```

### 7.2 iec61850-plugin crate

```
mupc/crates/iec61850-plugin/src/
├── lib.rs                  # 模块导出
├── mms_client.rs           # MMS 客户端封装（短连接模式）
├── mms_types.rs            # MMS 数据类型（DataObject, MmsRequest, MmsResponse）
├── asn1_utils.rs           # ASN.1 BER 编码/解码工具
├── config.rs               # MmsConfig, MmsTlsConfig
├── device.rs               # Iec61850Device trait
├── goose.rs                # GOOSE 消息订阅
└── errors.rs               # Iec61850Error 错误类型
```

### 7.3 mqtt-plugin crate

```
mupc/crates/mqtt-plugin/src/
├── lib.rs                  # 模块导出
├── client.rs               # MqttClient 北向 MQTT 客户端
├── config.rs               # MqttConfig, MqttQos, TlsConfig
└── errors.rs               # MqttError 错误类型
```

### 7.4 mqtt-bridge crate

```
mupc/crates/mqtt-bridge/src/
├── lib.rs                  # 模块导出
├── client.rs               # MqttBridge trait
├── local_client.rs         # LocalMqttClient（本地 mosquitto）
├── north_client.rs         # NorthMqttClient（北向 emqx）
├── config.rs               # MqttConfig, LocalMqttConfig, NorthMqttConfig, TlsConfig, ReconnectConfig
├── topics.rs               # Topic 常量定义
└── error.rs                # MqttBridgeError 错误类型
```

### 7.5 device-trait crate

```
mupc/crates/device-trait/src/
├── lib.rs                  # 模块导出
├── device.rs               # Device trait, DeviceCommand 枚举
├── south_device.rs         # SouthDevice trait, ProtocolHandler, HplcDriver
├── message_bus.rs          # MessageBus trait, MessageHandler
├── plugin.rs               # Plugin trait, PluginState
├── plugin_loader.rs        # PluginLoader trait
├── registry.rs             # DeviceRegistry trait, DeviceQuery
├── types.rs                # Topic, Message, DataFrame, PluginMeta, Rs485Config
└── errors.rs               # BusError, DeviceError, PluginError, RegistryError
```

### 7.6 Mosquitto Docker 配置

```
mupc/docker/mosquitto/
├── Dockerfile              # eclipse-mosquitto:2 镜像
└── config/
    └── mosquitto.conf      # 本地 MQTT Broker 配置
```

---

## 8. 技术决策记录

### 8.1 IEC 104 服务端模式

| 决策 | 选择 | 理由 |
|------|------|------|
| 通信模式 | **服务端** | MUPC 对调度主站而言是被控端，主站主动发起连接 |
| 帧解析 | **逐字节解析** | IEC 104 是定长/变长混合协议，无需额外序列化框架 |
| 并发模型 | **每连接一个 Task** | Tokio async 原生支持，最大 5 连接开销可控 |
| 状态管理 | **显式状态机** | IEC 104 STARTDT/STOPDT 协议要求严格状态转换 |

### 8.2 IEC 61850 MMS 短连接模式

| 决策 | 选择 | 理由 |
|------|------|------|
| 连接模式 | **短连接** | IED 设备资源有限，避免长期占用连接；简化连接管理 |
| 协议栈 | **libIEC61850 C + FFI** | 成熟的 C 实现，覆盖完整 MMS 协议栈；避免从零实现 ASN.1 |
| TLS | **可选**（MmsTlsConfig） | 配电自动化网络通常为专网，TLS 按需启用 |
| ASN.1 编解码 | **自实现 BER 编解码** | libIEC61850 处理核心 MMS PDU，辅助工具自行实现 |

### 8.3 MQTT 分层架构

| 决策 | 选择 | 理由 |
|------|------|------|
| 南向 Broker | **mosquitto** | 轻量开源，适合本地进程间通信 |
| 北向 Broker | **emqx** | 企业级 MQTT Broker，支持 TLS、集群、规则引擎 |
| TLS 要求 | **北向强制，本地不启用** | 北向经过公网，本地内网无需加密 |
| 客户端库 | **rumqttc** | Rust 原生异步 MQTT 客户端，社区活跃 |
| 连接管理 | **指数退避重连（无上限）** | 确保断线后最终恢复，适用于工业场景 |

### 8.4 消息总线演进

| 决策 | Phase 1 | Phase 3B |
|------|---------|----------|
| 实现 | `tokio::sync::mpsc` | mqtt-bridge (mosquitto + emqx) |
| 接口 | MessageBus trait（device-trait） | MqttBridge trait + MessageBus trait |
| 适用范围 | 进程内 | 进程间 + 进程内 |

### 8.5 插件体系

| 决策 | 选择 | 理由 |
|------|------|------|
| 插件 trait | **device-trait::Plugin** | 统一所有插件（IEC61850、MQTT、RS485 等） |
| 动态加载 | **libloading** | 跨平台 POSIX 支持 |
| 生命周期 | **Load → Init → Start → Stop → Unload** | 标准插件生命周期 |

### 8.6 错误处理策略

```
错误层次：
  ApplicationError  → 业务逻辑错误、策略执行失败
  ProtocolError     → IEC 104 帧错误、MQTT 协议错误
  IoError           → TCP 连接断开、超时
  DeviceError       → 设备离线、无响应
```

每层错误都实现 `std::error::Error`，支持 `error.source()` 错误链。

---

## 附录 A：验收标准参考

### IEC 104 验收标准

| ID | 功能点 | 来源 |
|----|--------|------|
| IEC104-01 | 协议兼容（U/S/I 帧） | **[REVIEWED: PASS]** |
| IEC104-02 | 连接管理（状态机、心跳、超时） | **[REVIEWED: PASS]** |
| IEC104-03 | 自动重连（5s/10次/1分钟） | **[REVIEWED: PASS]** |
| IEC104-04 | TypeID 支持（14 种） | **[REVIEWED: PASS]** |
| IEC104-05 | 数据上行（周期性 1Hz、告警即时） | **[REVIEWED: PASS]** |
| IEC104-06 | 数据下行（确认、策略引擎校验） | **[REVIEWED: PASS]** |
| IEC104-07 | 调度指令（P_set/Q_set、响应 ≤ 100ms） | **[REVIEWED: PASS]** |
| IEC104-08 | 并发连接（≤ 5） | **[REVIEWED: PASS]** |

### IEC 61850 / MMS 验收标准

| ID | 功能点 | 来源 |
|----|--------|------|
| MMS-01 | MMS Read 服务 | DRAFT |
| MMS-02 | MMS Write 服务 | DRAFT |
| MMS-03 | MMS over TLS | DRAFT |
| MMS-04 | 超时处理 | DRAFT |
| MMS-05 | 错误处理 | DRAFT |
| MMS-06 | 短连接模式 | DRAFT |
| MMS-07 | GOOSE 订阅 | **[REVIEWED: PASS]** |
| MMS-08 | IED 连接（≤ 5s，成功率 ≥ 99%） | **[REVIEWED: PASS]** |

### MQTT 验收标准

| ID | 功能点 | 来源 |
|----|--------|------|
| MQTT-01 | Broker 连接 | **[REVIEWED: PASS]** |
| MQTT-02 | QoS 0（< 100ms） | **[REVIEWED: PASS]** |
| MQTT-03 | QoS 1/2（< 500ms） | **[REVIEWED: PASS]** |
| MQTT-04 | MQTT over TLS | **[REVIEWED: PASS]** |
| MQTT-05 | 断线重连 | **[DESIGN_APPROVED]** |
| MQTT-06 | 消息持久化 | **[DESIGN_APPROVED]** |

### 消息总线验收标准

| ID | 功能点 | 来源 |
|----|--------|------|
| MB-01 | 本地 mosquitto 连接 | **[DESIGN_APPROVED]** |
| MB-02 | 北向 emqx 连接（TLS） | **[DESIGN_APPROVED]** |
| MB-03 | 进程间消息延迟 < 100ms | **[DESIGN_APPROVED]** |
| MB-04 | 断线重连自动恢复 | **[DESIGN_APPROVED]** |
| MB-05 | QoS 1 至少一次到达 | **[DESIGN_APPROVED]** |
| MB-06 | QoS 2 恰好一次到达 | **[DESIGN_APPROVED]** |
| MB-07 | 证书认证成功 | **[DESIGN_APPROVED]** |
| MB-08 | 消息持久化正确 | **[DESIGN_APPROVED]** |

---

## 附录 B：来源文档索引

| 文档 | 路径 | 状态 |
|------|------|------|
| 通信网关 PRD | `docs/superpowers/specs/modules/01-MUPC-通信网关-PRD.md` | 合并修订版 v1.0 |
| IEC61850 MMS 实施计划 | `docs/superpowers/plans/2026-05-28-IEC61850-MMS-客户端真正实现-实施计划.md` | — |
| Phase 3B 实施计划 | `docs/superpowers/plans/2026-05-28-MUPC-Phase3B-实施计划.md` | — |

---

**文档状态：** 合并版 v1.1
**合并说明：** 综合 5 份来源文档中与通信网关模块（gateway、iec61850-plugin、mqtt-plugin、mqtt-bridge、device-trait）相关的内容，与现有代码实现保持一致。所有 **[DESIGN_APPROVED]** 和 **[REVIEWED: PASS]** 标记已完整保留。

---

## 附录 C：v1.1 修订记录

| 编号 | 缺口 | 风险等级 | 修改位置 | 修改内容 |
|------|------|---------|---------|---------|
| 1 | ControlCommand 缺少一次调频参数 | 高 | 2.5 命令处理 + 6.8 IEC 104 命令处理器 | `ControlCommand` 结构体新增 `k_value: Option<f64>`（一次调频 K 值）和 `deadband: Option<f64>`（一次调频死区 Hz）字段 |
| 2 | UTC 时标未明确 | 中 | 2.2 数据结构定义 | 新增"时标规范"子章节，明确所有带时标 TypeID 使用时标字段使用 UTC 时间，精度到毫秒，按 CP56Time2a 格式编码 |
| 3 | 跨模块引用缺失 | 高 | 1.1 模块定位 + 1.5 用户角色 + 3.1 架构概述 | 补充跨模块引用：OTA→模块07、看门狗→模块10、安全合规→模块06、无线运维通道→模块09、DER 逻辑节点延后说明 |
| 4 | 版本号更新 | 低 | 文档头 + 文档尾 | 版本号从 v1.0 更新为 v1.1 |

**修订说明：** 本次修订（v1.0 → v1.1）同步 PRD v1.1 [REVIEWED: PASS] 的变更，修复设计覆盖度审查中 CONDITIONAL_PASS 的三项问题：ControlCommand 字段补全、UTC 时标规范明确、跨模块引用补充。
