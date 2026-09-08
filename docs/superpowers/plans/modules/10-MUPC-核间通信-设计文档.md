# MUPC 核间通信模块设计文档

---

## 目录

1. [模块架构](#1-模块架构)
2. [TCP 连接管理设计](#2-tcp-连接管理设计)
3. [帧协议设计](#3-帧协议设计)
4. [指令下发与数据读取设计](#4-指令下发与数据读取设计)
5. [关键信号设计](#5-关键信号设计)
6. [心跳与看门狗设计](#6-心跳与看门狗设计)
7. [状态监控与异常处理设计](#7-状态监控与异常处理设计)
8. [接口定义](#8-接口定义)
9. [文件结构](#9-文件结构)
10. [技术决策记录](#10-技术决策记录)
11. [传输通道抽象与 Modbus RTU 备选链路](#11-传输通道抽象与-modbus-rtu-备选链路)
12. [BECG-3568 现场接线契约与安全联锁](#12-becg-3568-现场接线契约与安全联锁)

---

## 1. 模块架构

### 1.1 模块定位

> 功能描述与核心职责详见 [PRD 第 1 章](../specs/modules/10-MUPC-核间通信-PRD.md#1-产品概述)。

核间通信模块采用 Tokio 异步 TCP 客户端架构，是异构双核心之间的通信桥梁。

```
通信管理模块 (大脑) ──── TCP Socket (RJ45) ──── 实时控制模块 (小脑)
    - gateway                         - 硬实时控制
    - strategy-engine                  - 10kHz 电流环
    - ai-engine                        - 功率变换
    - intercore  ←── 本模块 ──→        - 保护逻辑
```

### 1.2 边界说明

| 本模块负责 | 本模块不负责 |
|-----------|-------------|
| TCP 连接管理（建立、保持、重连） | 硬实时控制（如 10kHz 电流环） |
| 帧协议编解码（定长 64 字节二进制帧） | 指令内容的策略校验（由 strategy-engine 完成） |
| 数据收发与确认（序列号匹配、CRC 校验） | 调度指令的接收解析（由 gateway 完成） |
| 心跳检测与看门狗超时监控 | AI 指令的安全校验（由 strategy-engine 完成） |
| 连接状态报告（对外暴露状态查询接口） | 设备层协议转换（由南向插件完成） |

### 1.3 依赖关系

| 依赖组件 | 关系说明 |
|---------|---------|
| mupc-common | 错误类型（MupcError、ErrorCode）、日志（tracing） |
| mupc-core | 核心基础设施（可选的 ServiceCoordinator 集成） |
| strategy-engine（调用方） | 通过 intercore 下发控制指令 |
| data-processing（消费方） | 通过 intercore 读取的实时数据 |
| byteorder | 大端/小端字节序编解码 |
| chrono | 时间戳处理（心跳管理） |
| serde / serde_json | Payload JSON 编解码（控制指令、状态报告） |

### 1.4 架构集成

```
gateway (调度指令)
    │ 经 strategy-engine 校验后
    ▼
strategy-engine ──→ intercore ──→ 实时控制模块
                            │
                            ▼
                      data-processing (数据汇聚)
                            │
                    ┌───────┴───────┐
                    ▼               ▼
                gateway (IEC 104)   Web UI (状态展示)
```

---

## 2. TCP 连接管理设计

### 2.1 网络拓扑

- **通信管理模块：** TCP 客户端，主动发起连接
- **实时控制模块：** TCP 服务端，监听连接
- **物理层：** RJ45 以太网直连

### 2.2 端口规划

| 角色 | 方向 | 默认端口 | 说明 |
|------|------|---------|------|
| 实时控制模块（服务端） | 监听 | **9100** | 等待通信管理模块连接 |
| 通信管理模块（客户端） | 连接对端 | **9100** | 主动连接实时控制模块 |

> **说明：** 通信管理模块作为客户端主动连接实时控制模块的 9100 端口；实时控制模块作为服务端监听 9100。通信管理模块作为客户端不暴露监听端口。

### 2.3 连接生命周期

```
[建立]
  通信管理模块 ──TCP 连接──→ 实时控制模块
       │                           │
       └────── 发送 Connect 帧 ────→   ← 连接建立完成
       
[保持]
  通信管理模块 ──HeartbeatReq──→ 实时控制模块 (1s 周期)
       │                           │
       └────── StatusReport / DataUpload ←────┘
       
[断开]
  通信管理模块  ──TCP 断开──→ 实时控制模块
       │                           │
       │                            → 标记连接断开
       │                            → 等待重连
       
[重连]
  通信管理模块  ──TCP 重连──→ 实时控制模块 (自动重连)
```

**连接管理规则：**
1. 通信管理模块作为 TCP 客户端，主动发起连接
2. 连接建立后，通信管理模块自动发送 Connect 帧（`0x0001`）完成注册
3. 连接丢失后，通信管理模块负责自动重连
4. 支持长连接模式，连接建立后不主动断开
5. 通信参数（端口、地址、心跳间隔、看门狗超时）支持运行时配置

### 2.4 线程/任务模型

```
Tokio Runtime
    │
    ├── listener task: acceptor loop
    │      └── accept() → spawn per-connection handler
    │
    ├── heartbeat manager task: periodic check loop (1s interval)
    │
    └── watchdog task: timeout detection (10s threshold)
```

- 使用 Tokio 多线程运行时
- `IntercoreServer::start()` 启动后：
  1. 创建 `TcpListener` 绑定端口
  2. 进入 accept 循环，每个连接 spawn 独立处理任务
  3. 启动心跳管理器循环任务
  4. 返回 `HeartbeatManager` 的共享引用供外部访问



---

## 3. 帧协议设计

### 3.1 协议概述

采用自定义二进制协议，定长 64 字节，包含帧头、有效数据和 CRC16 校验。

**帧结构：**

```
+----------------+----------------+----------------+----------------+----------------+----------------+
|    Magic(2B)   |   Length(2B)   |   Type(2B)     |   SeqNo(2B)    |   Payload(NB)  |   CRC16(2B)    |
|    0xAA 0x55   |  帧总长度       |  帧类型标识     |  序列号         |  有效数据        |  校验和         |
+----------------+----------------+----------------+----------------+----------------+----------------+
```

### 3.2 字段定义

| 字段 | 偏移 | 长度 | 字节序 | 说明 |
|------|------|------|--------|------|
| Magic | 0 | 2 字节 | 大端 | 帧头标识，固定 `0xAA 0x55` |
| Length | 2 | 2 字节 | 大端 | 帧总长度（含帧头、数据、CRC16），固定为 64 |
| FrameType | 4 | 2 字节 | 大端 | 帧类型编码 |
| SeqNo | 6 | 2 字节 | 大端 | 序列号，用于请求-应答匹配 |
| Payload | 8 | N 字节 | — | 有效数据，N = 54 字节（扣除帧头 8 和 CRC 2） |
| CRC16 | 62 | 2 字节 | 大端（网络字节序） | MODBUS CRC16 校验，帧内以大端存储 |

**帧总长度约束：**
- 单帧长度固定为 **64 字节**
- 帧头固定 8 字节
- CRC16 占 2 字节
- 有效数据最大 54 字节
- 不足部分以 `0x00` 填充至 64 字节

### 3.3 帧类型定义

| 类型编码 | 名称 | 方向 | 说明 |
|---------|------|------|------|
| `0x0001` | Connect | 双向 | 连接注册帧，连接建立后发送 |
| `0x0002` | HeartbeatReq | 通信管理模块 → 实时控制模块 | 心跳请求 |
| `0x0003` | HeartbeatRsp | 实时控制模块 → 通信管理模块 | 心跳响应 |
| `0x0010` | ControlCmd | 通信管理模块 → 实时控制模块 | 控制指令下发 |
| `0x0011` | ControlRsp | 实时控制模块 → 通信管理模块 | 控制指令应答 |
| `0x0020` | StatusReport | 实时控制模块 → 通信管理模块 | 状态报告（含电气量等） |
| `0x0030` | DataUpload | 实时控制模块 → 通信管理模块 | 数据上送（周期遥测，含 q_realtime_margin） |
| `0x0040` | SafetyOverride | 实时控制模块 → 通信管理模块 | 安全覆盖触发 |

### 3.4 帧格式详述

#### 3.4.1 连接帧（Connect, 0x0001）

- 方向：双向（连接建立后通信管理模块主动发送）
- 用途：握手注册
- Payload：空（仅帧头 + CRC + padding）

```
Bytes:  0xAA 0x55 | 0x00 0x40 | 0x00 0x01 | 0x00 0x00 | (52 padding) | CRC16
         Magic       Length=64   Type=Connect  SeqNo=0
```

#### 3.4.2 心跳帧（HeartbeatReq, 0x0002 / HeartbeatRsp, 0x0003）

**HeartbeatReq Payload：**

| 偏移 | 长度 | 类型 | 字段 | 说明 |
|------|------|------|------|------|
| 0 | 1 | u8 | status | 状态码（0=正常，1=警告，2=故障） |
| 1 | 8 | f64（小端） | cpu_temp | CPU 温度 |
| 9 | 8 | f64（小端） | memory_usage | 内存使用率（0.0~1.0） |

**HeartbeatRsp：** Payload 为空（仅帧头 + CRC + padding）。

#### 3.4.3 控制指令帧（ControlCmd, 0x0010）

> **双参数模式**：帧格式从多指令类型简化为双参数（`p_ref` + `k_droop`），实现下垂控制。

Payload 采用 JSON 编码（v2.0）：

```json
{
    "p_ref": 10.5,
    "k_droop": 15.0,
    "ai_ready": true,
    "strategy_mode": "Smart",
    "timestamp_ms": 1712345678123,
    "frame_version": 2
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| p_ref | f64 | 有功基准点 (kW)，实时控制模块用于下垂控制 |
| k_droop | f64 | 电压-有功下垂系数 (kW/V) |
| ai_ready | bool | AI 引擎就绪状态 |
| strategy_mode | string | 当前策略模式上下文 |
| timestamp_ms | u64 | UTC 时标（毫秒） |
| frame_version | u8 | 帧版本号，v2.0 为 `2` |

> **注意**：`load_shedding` 和 `pv_limit` **不通过此帧发送**，而是通过 SouthCommandDispatcher 发送到南向设备（光伏逆变器、负荷控制装置），避免核间通信负载过大。

**支持指令类型：**

| 指令 | cmd_type | 说明 | 数据范围 |
|------|---------|------|---------|
| 系统复位 | Sys_reset | 触发实时控制模块复位 | — |

#### 3.4.4 控制响应帧（ControlRsp, 0x0011）

Payload 采用 JSON 编码：

```json
{
    "cmd_type": "P_batt_set",
    "seq_no": 42,
    "result": "success",
    "error_msg": "",
    "timestamp": 1712345679
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| cmd_type | string | 对应指令类型 |
| seq_no | u16 | 对应指令的序列号 |
| result | string | 执行结果：`success` / `failure` / `timeout` |
| error_msg | string | 失败时的错误描述 |
| timestamp | i64 | 执行完成时间戳 |

#### 3.4.5 状态报告帧（StatusReport, 0x0020）

实时控制模块定期上报状态信息。

Payload 采用 JSON 编码：

```json
{
    "U_a": 220.5,
    "U_b": 221.0,
    "U_c": 219.8,
    "I_a": 15.2,
    "I_b": 14.8,
    "I_c": 15.5,
    "P": 10.2,
    "Q": 0.5,
    "cos_phi": 0.95,
    "freq": 50.02,
    "soc": 75.5,
    "soh": 98.0,
    "batt_temp": 35.2,
    "inv_status": "running",
    "pv_power": 5.0,
    "load_power": 8.0,
    "charger_power": 2.5,
    "ai_ready": true,
    "strategy_mode": "Smart",
    "timestamp": 1712345678
}
```

#### 3.4.6 数据上送帧（DataUpload, 0x0030）

> DataUpload 在 StatusReport 基础上扩展，新增 `q_realtime_margin` 字段。

Payload 采用 JSON 编码（v2.10）：

```json
{
    "frame_version": 1,
    "timestamp_ms": 1712345678123,
    "q_realtime_margin": 0.65,
    "battery_soc": 75.5,
    "voltage_phase_a": 220.5,
    "voltage_phase_b": 221.0,
    "voltage_phase_c": 219.8,
    "battery_power": 10.2
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| frame_version | u8 | 帧版本号，v2.10 为 `1` |
| timestamp_ms | u64 | UTC 时标（毫秒） |
| q_realtime_margin | f64 | 实时模块剩余无功容量比例 [0.0, 1.0]，0=无功打满，1=完全空闲 |
| battery_soc | f64 | 电池荷电状态 (%) |
| voltage_phase_a/b/c | f64 | 三相电压标幺值 (p.u.) |
| battery_power | f64 | 电池当前功率 (kW) |

#### 3.4.7 安全覆盖帧（SafetyOverride, 0x0040）

> 当实时控制模块检测到电压越限且无功耗尽时，临时覆盖 AI 有功指令的紧急事件帧。

Payload 采用 JSON 编码（v2.10）：

```json
{
    "frame_version": 1,
    "timestamp_ms": 1712345678123,
    "trigger_reason": "voltage_sag",
    "voltage_phase_a": 0.85,
    "voltage_phase_b": 0.86,
    "voltage_phase_c": 0.84,
    "q_realtime_margin": 0.02,
    "override_p_ref": -30.0,
    "override_duration_ms": 5000,
    "recovery_condition": "timer_expired"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| frame_version | u8 | 帧版本号，v2.10 为 `1` |
| timestamp_ms | u64 | UTC 时标（毫秒） |
| trigger_reason | string | 触发原因 |
| voltage_phase_a/b/c | f64 | 三相电压标幺值 |
| q_realtime_margin | f64 | 实时模块剩余无功容量 |
| override_p_ref | f64 | 强制放电功率 (kW)，负值表示放电 |
| override_duration_ms | u64 | 覆盖持续时间（ms），不超过 10000ms |
| recovery_condition | string | 恢复条件 |

**频率限制**：1 分钟内最多触发 3 次，超限后丢弃。

### 3.5 CRC16 算法

采用 MODBUS CRC16 算法：

```rust
fn calculate_crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for byte in data {
        crc ^= *byte as u16;
        for _ in 0..8 {
            if crc & 0x0001 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}
```

- 初始值：`0xFFFF`
- 多项式：`0xA001`（反转形式）
- 输出：小端字节序（CRC 低字节在前）— 此为 Modbus CRC16 算法原始输出格式；存入核间通信帧时转换为大端字节序（网络字节序）

### 3.6 代码实现（关键 Rust 结构）

```rust
/// 帧头
pub struct FrameHeader {
    pub magic: u16,          // 固定 0xAA55
    pub length: u16,         // 帧总长度
    pub frame_type: FrameType, // 帧类型
    pub seq_no: u16,         // 序列号
}

/// 核间通信帧
pub struct IntercoreFrame {
    pub header: FrameHeader,
    pub data: Vec<u8>,       // 有效数据（不包括填充字节）
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `IntercoreFrame::new(type, seq_no, data)` | 创建通用帧 |
| `IntercoreFrame::new_connect()` | 创建连接帧 |
| `IntercoreFrame::new_heartbeat_req(status, cpu_temp, memory_usage)` | 创建心跳请求帧 |
| `IntercoreFrame::new_heartbeat_rsp()` | 创建心跳响应帧 |
| `IntercoreFrame::to_bytes()` | 序列化为定长 64 字节 |
| `IntercoreFrame::from_bytes(data)` | 反序列化，含 CRC 校验 |
| `FrameHeader::from_bytes(data)` | 从字节流解析帧头 |

---

## 4. 指令下发与数据读取设计

### 4.1 指令下发流程

```
strategy-engine / gateway
    │
    │ send_command(ControlCommand)
    ▼
intercore
    │
    │ 1. 分配序列号 seq_no
    │ 2. 构建 ControlCmd 帧（JSON Payload）
    │ 3. 写入 TCP 连接
    │ 4. 启动 5 秒超时定时器
    ▼
实时控制模块
    │
    │ 1. 解析 ControlCmd 帧
    │ 2. 执行指令
    ▼
    │ 3. 回复 ControlRsp 帧（含 seq_no + 执行结果）
    ▼
intercore
    │
    │ 匹配 seq_no
    │ 返回 CommandResponse
    ▼
调用方收到结果
```

**指令下发约束：**

| 指标 | 要求 |
|------|------|
| 指令下发延迟 | ≤ 50ms（从收到下发请求到帧发送完成） |
| 指令确认超时 | 5 秒 |
| 超时重试次数 | 最多 2 次 |
| 序列号匹配 | 必须，防止乱序 |

### 4.2 数据读取流程

```
实时控制模块
    │
    │ 周期上送（≥ 1Hz）
    ▼
StatusReport / DataUpload 帧
    │
    │ intercore 解析
    ▼
data-processing (数据汇聚)
    │
    ├──→ gateway (IEC 104 上送调度主站)
    ├──→ strategy-engine (供策略决策)
    └──→ Web UI (状态展示)
```

**数据读取数据类型：**

| 数据类别 | 具体数据项 | 说明 |
|---------|-----------|------|
| 电气量 | U（三相电压） | 单位：V |
| 电气量 | I（三相电流） | 单位：A |
| 电气量 | P（有功功率） | 单位：kW |
| 电气量 | Q（无功功率） | 单位：kVar |
| 电气量 | cosφ（功率因数） | — |
| 电气量 | f（频率） | 单位：Hz |
| 电池数据 | SOC（荷电状态） | 0% ~ 100% |
| 电池数据 | SOH（健康状态） | 0% ~ 100% |
| 电池数据 | 电池温度 | 单位：℃ |
| 逆变器状态 | 运行/停机/故障 | 状态枚举 |
| 功率数据 | 光伏出力 | 单位：kW |
| 功率数据 | 负荷功率 | 单位：kW |
| 功率数据 | 充电桩功率 | 单位：kW |

> 验收标准详见 [PRD 3.2 数据读取](../specs/modules/10-MUPC-核间通信-PRD.md#32-数据读取)。

---

## 5. 关键信号设计

### 5.1 信号定义

以下关键信号通过核间通信通道传输，用于表达通信管理模块与实时控制模块之间的协同状态：

| 信号 | 方向 | 类型 | 说明 |
|------|------|------|------|
| `ai_ready` | 通信管理模块 → 实时控制模块 | 布尔 | AI 优化引擎可用状态。`true` 表示 AI 引擎正常，可下发优化指令；`false` 表示 AI 失效，系统以兜底策略运行 |
| `strategy_mode` | 通信管理模块 → 实时控制模块 | 枚举 | 当前策略模式。取值：`Basic`（基础模式）、`Smart`（AI 智能模式）、`Fallback`（兜底模式） |
| `control_cmd` | 通信管理模块 → 实时控制模块 | 复合 | 下发给实时控制模块的具体控制指令，包含指令类型、目标值、时间戳。由 strategy-engine 或 gateway 经校验后发起 |

### 5.2 信号传输方式

关键信号通过以下方式传输：

1. **ai_ready 和 strategy_mode**：封装在 ControlCmd 帧的 JSON Payload 中，随指令下发
2. **控制指令**：使用 ControlCmd 帧（`0x0010`），通过序列号匹配应答

### 5.3 信号管理要求

- `ai_ready` 状态变更时立即通过 ControlCmd 帧同步给实时控制模块
- `strategy_mode` 在模式切换时立即同步
- `control_cmd` 每次下发均需携带当前 `strategy_mode` 上下文（在 JSON Payload 中）
- 实时控制模块通过 StatusReport 帧回显当前感知到的 `ai_ready` 和 `strategy_mode`，用于校验通信一致性

---

## 6. 心跳与看门狗设计

### 6.1 心跳机制

**心跳流程：**

```
通信管理模块                      实时控制模块
    │                                  │
    │── HeartbeatReq (0x0002) ────────→│  (status, cpu_temp, memory_usage)
    │                                  │
    │←─ HeartbeatRsp (0x0003) ─────────│  (空 Payload)
    │                                  │
    │ (收到响应 → 更新 last_heartbeat)   │
```

**心跳参数：**

| 参数 | 默认值 | 说明 |
|------|--------|------|
| 心跳周期 | 1 秒 | 通信管理模块每秒发送一次 HeartbeatReq |
| 状态码 | status: u8 | 0=正常，1=警告，2=故障 |
| CPU 温度 | cpu_temp: f64 | 对端 CPU 温度 |
| 内存使用率 | memory_usage: f64 | 对端内存使用率（0.0~1.0） |

**心跳帧 HeartbeatReq Payload 格式：**

```
Offset  Type    Field
0       u8      status      (状态码)
1       f64     cpu_temp    (CPU 温度，小端)
9       f64     memory_usage (内存使用率，小端)
17      ...     (padding 至 54 字节)
```

**心跳管理器（HeartbeatManager）职责：**
- 维护连接与心跳状态的映射（`HashMap<SocketAddr, HeartbeatStatus>`）
- 注册/注销连接
- 接收心跳时更新 `last_heartbeat` 时间戳
- 运行心跳检测循环（1 秒周期），检查连接是否超时

**HeartbeatStatus 结构：**

```rust
pub struct HeartbeatStatus {
    pub online: bool,           // 是否在线
    pub last_heartbeat: u64,    // 最后心跳时间戳（Unix 时间戳，秒）
    pub status: u8,             // 状态码
    pub cpu_temp: f64,          // CPU 温度
    pub memory_usage: f64,      // 内存使用率
}
```

### 6.2 看门狗超时检测

**看门狗配置：**

| 配置项 | 默认值 | 可配置范围 | 说明 |
|-------|--------|-----------|------|
| 超时时间（WatchdogConfig.timeout_ms） | 10000ms | 5000ms ~ 30000ms | 判定超时的阈值 |
| 最大连续丢失心跳（WatchdogConfig.max_missed_heartbeats） | 3 次 | 1 ~ 10 次 | 达到阈值后触发告警 |

**超时判定流程：**

```
HeartbeatManager::run() 每秒 tick
    │
    ▼
遍历所有连接，计算 elapsed = now - last_heartbeat
    │
    ├── elapsed × 1000 > watchdog_timeout_ms (10s)？
    │   ├── 是 → 标记 online = false（首次超时才触发 warn 日志）
    │   └── 否 → 不做处理
    │
Watchdog::check_timeout() 独立检测
    │
    ├── 所有连接均 offline？
    │   ├── 是 → 连续丢失心跳计数 +1
    │   │        ├── ≥ max_missed_heartbeats (3)？
    │   │        │   ├── 是 → state = Timeout，触发告警
    │   │        │   └── 否 → 继续监控
    │   │
    │   └── 否 → 重置连续丢失计数，state = Active
```

**WatchdogState 枚举：**

```rust
pub enum WatchdogState {
    Active,   // 正常
    Timeout,  // 超时
    Reset,    // 已复位
}
```

### 6.3 看门狗超时处理

```
实时控制模块无响应（看门狗超时）
    ↓
标记连接状态为 offline
    ↓
触发系统告警（IC-002）
    ↓
记录故障日志（含时间戳、丢失心跳次数）
    ↓
可选：发送系统复位指令（Sys_reset）触发实时控制模块复位
    ↓
持续监控，直至心跳恢复
    ↓
恢复后标记连接为 online，清除告警
```

**降级机制：**
- 看门狗超时后，通信管理模块应通知 strategy-engine 进入"实时控制模块离线"模式
- 此模式下，实时控制指令缓存至本地队列，等待连接恢复后补发
- 持续告警直至连接恢复

---

## 7. 状态监控与异常处理设计

### 7.1 连接状态监控

核间通信模块需要对外暴露连接状态，供 Web UI 和 system-monitoring 展示。

**状态信息：**
- 连接状态（已连接 / 已断开 / 连接中）
- 对端地址（IP:Port）
- 连接建立时间
- 最后一次数据收发时间
- 当前心跳状态（在线 / 超时）
- 累积丢失心跳次数
- 对端状态码、CPU 温度、内存使用率

> **⚠️ PCS 形态说明（transport=modbus_rtu，v2.2+）**：上表对端 IP/CPU 温度/内存等字段
> 源自 TCP 仿真通道的 StatusReport——**PCS 生产通道无此上送**（PCS 仅 RS485，无 TCP 状态帧）。
> PCS 通道连接/健康由 `ModbusRtuTransport` 心跳轮询 3 区 1013 驱动（`connected` 标志 +
> 停机观测 M1），经 `IntercoreClient::is_connected()` 对上层可见；**1013 运行状态/告警字到
> Web UI / system-monitoring 状态展示的具体字段映射尚未设计**（M12，属 §11.11 健康可选
> 接入的对外呈现，待 Web API 对接 PCS 健康项时补）。

**对外查询接口：**

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `get_connection_status(addr)` | `Option<HeartbeatStatus>` | 查询指定连接状态 |
| `get_all_status()` | `HashMap<SocketAddr, HeartbeatStatus>` | 查询所有连接状态 |
| `is_connection_timeout(addr)` | `bool` | 检查连接是否超时 |
| `state()` | `WatchdogState` | 获取看门狗状态 |
| `missed_heartbeats()` | `u32` | 获取连续丢失心跳次数 |

### 7.2 异常场景与处理

| 异常场景 | 检测方式 | 处理措施 | 告警编码 |
|---------|---------|---------|---------|
| 连接断开 | TCP 连接关闭/错误 | 标记连接断开，持续等待重连 | IC-001 |
| 心跳超时 | 看门狗超时检测 | 触发告警，可选复位，持续监控 | IC-002 |
| CRC 校验失败 | 帧解析时 CRC 验证 | 丢弃该帧，记录错误日志，不中断连接 | IC-003 |
| 帧格式错误 | 帧头解析失败（Magic/Length/Type） | 丢弃该帧，记录错误日志，不中断连接 | IC-003 |
| 指令超时无应答 | 5 秒超时计时 | 标记指令失败，可选重试 | IC-004 |
| 对端 CPU 温度异常 | 心跳帧中解析的 cpu_temp | 温度超过阈值（默认 85℃）触发告警 | IC-005 |
| 对端状态异常 | 心跳帧中 status 非 0 | 记录警告日志 | IC-006 |

### 7.3 告警定义

| 告警编码 | 告警名称 | 级别 | 触发条件 |
|---------|---------|------|---------|
| IC-001 | 核间连接断开 | 严重 | TCP 连接断开 |
| IC-002 | 核间心跳超时 | 警告 | 看门狗超时触发 |
| IC-003 | 核间帧校验失败 | 警告 | CRC 或帧格式错误 |
| IC-004 | 指令下发超时 | 警告 | ControlCmd 5 秒无应答 |
| IC-005 | 对端 CPU 温度异常 | 警告 | 对端上报温度超过阈值（默认 85℃） |
| IC-006 | 对端状态异常 | 警告 | 对端状态码非 0 |

### 7.4 错误码映射

核间通信相关错误码定义在 `mupc-common` 的 `ErrorCode` 枚举中（范围 0x0200~0x02FF）：

| 错误码 | 值 | 描述 |
|--------|------|------|
| `IntercoreTimeout` | 0x0200 | 核间通信超时 |
| `HeartbeatMissed` | 0x0201 | 心跳丢失 |
| `FrameChecksumError` | 0x0202 | 帧校验和错误 |
| `InvalidFrame` | 0x0203 | 无效帧 |
| `SendFailed` | 0x0204 | 发送失败 |

其他通用错误码：`ConnectionFailed`（0x0005）、`Timeout`（0x0004）、`FrameParseError`（0x0101）、`SerializeError`（0x0008）。

### 7.5 异常帧处理策略

- **CRC 校验失败**：静默丢弃该帧，记录 tracing::error! 日志，不中断连接
- **帧格式错误（Magic 不匹配）**：静默丢弃该帧，记录 tracing::error! 日志，不中断连接
- **未知帧类型**：记录 tracing::warn! 日志，继续处理后续帧
- **连接读取返回 0 字节**：对端关闭连接，标记连接断开

---

## 8. 接口定义

### 8.1 IntercoreServer

```rust
/// 核间通信配置
pub struct IntercoreConfig {
    pub connect_addr: String,            // 连接地址（实时控制模块），默认 "127.0.0.1"
    pub connect_port: u16,               // 连接端口（实时控制模块监听），默认 9100
    pub heartbeat_interval_ms: u64,      // 心跳间隔，默认 1000ms
    pub watchdog_timeout_ms: u64,        // 看门狗超时，默认 10000ms
}

impl Default for IntercoreConfig {
    fn default() -> Self {
        Self {
            connect_addr: "127.0.0.1".to_string(),
            connect_port: 9100,
            heartbeat_interval_ms: 1000,
            watchdog_timeout_ms: 10000,
        }
    }
}

/// 核间通信服务器
pub struct IntercoreServer {
    config: IntercoreConfig,
    shutdown_tx: broadcast::Sender<()>,
}
```

**对外接口：**

```rust
impl IntercoreServer {
    /// 创建服务器实例
    pub fn new(config: IntercoreConfig) -> Self;

    /// 启动服务器
    /// 返回 HeartbeatManager 的共享引用，供查询连接状态
    pub async fn start(&self) -> Result<Arc<RwLock<HeartbeatManager>>, MupcError>;

    /// 停止服务器
    pub async fn shutdown(&self) -> Result<(), MupcError>;
}
```

### 8.2 HeartbeatManager

```rust
/// 心跳状态
pub struct HeartbeatStatus {
    pub online: bool,           // 是否在线
    pub last_heartbeat: u64,    // 最后心跳时间戳（秒）
    pub status: u8,             // 状态码
    pub cpu_temp: f64,          // CPU 温度
    pub memory_usage: f64,      // 内存使用率
}

/// 心跳管理器
pub struct HeartbeatManager {
    heartbeat_interval_ms: u64,
    watchdog_timeout_ms: u64,
    connections: Arc<RwLock<HashMap<SocketAddr, HeartbeatStatus>>>,
}
```

**对外接口：**

```rust
impl HeartbeatManager {
    /// 创建心跳管理器
    pub fn new(heartbeat_interval_ms: u64, watchdog_timeout_ms: u64) -> Self;

    /// 注册连接
    pub fn register_connection(&self, addr: SocketAddr);

    /// 注销连接
    pub fn unregister_connection(&self, addr: SocketAddr);

    /// 接收心跳（更新最后心跳时间戳）
    pub async fn receive_heartbeat(&self, addr: SocketAddr);

    /// 查询指定连接状态
    pub async fn get_connection_status(&self, addr: &SocketAddr) -> Option<HeartbeatStatus>;

    /// 查询所有连接状态
    pub async fn get_all_status(&self) -> HashMap<SocketAddr, HeartbeatStatus>;

    /// 检查连接是否超时
    pub async fn is_connection_timeout(&self, addr: &SocketAddr) -> bool;

    /// 运行心跳检测循环（内部任务，每秒 tick）
    pub async fn run(&self);
}
```

### 8.3 Watchdog

```rust
/// 看门狗配置
pub struct WatchdogConfig {
    pub timeout_ms: u64,                  // 超时时间，默认 10000ms
    pub max_missed_heartbeats: u32,      // 连续超时次数阈值，默认 3
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 10000,
            max_missed_heartbeats: 3,
        }
    }
}

/// 看门狗状态
pub enum WatchdogState {
    Active,     // 正常
    Timeout,    // 超时
    Reset,      // 已复位
}

/// 看门狗
pub struct Watchdog {
    config: WatchdogConfig,
    heartbeat_manager: Arc<RwLock<HeartbeatManager>>,
    missed_heartbeats: u32,
    state: WatchdogState,
}
```

**对外接口：**

```rust
impl Watchdog {
    /// 创建看门狗
    pub fn new(config: WatchdogConfig, heartbeat_manager: Arc<RwLock<HeartbeatManager>>) -> Self;

    /// 获取看门狗状态
    pub fn state(&self) -> WatchdogState;

    /// 获取连续丢失心跳次数
    pub fn missed_heartbeats(&self) -> u32;

    /// 检查是否超时
    pub async fn check_timeout(&mut self) -> bool;

    /// 重置看门狗
    pub fn reset(&mut self);

    /// 触发复位（发送 Sys_reset 指令）
    pub async fn trigger_reset(&self) -> Result<(), MupcError>;
}
```

### 8.4 IntercoreFrame / FrameHeader

```rust
/// 帧类型
pub enum FrameType {
    Connect = 0x0001,
    HeartbeatReq = 0x0002,
    HeartbeatRsp = 0x0003,
    ControlCmd = 0x0010,
    ControlRsp = 0x0011,
    StatusReport = 0x0020,
    DataUpload = 0x0030,
    SafetyOverride = 0x0040,
    Unknown = 0xFFFF,
}

impl FrameType {
    pub fn from_u16(val: u16) -> Self;
}

/// 帧头
pub struct FrameHeader {
    pub magic: u16,
    pub length: u16,
    pub frame_type: FrameType,
    pub seq_no: u16,
}

impl FrameHeader {
    pub const MAGIC: u16 = 0xAA55;
    pub const FIXED_LENGTH: usize = 8;

    pub fn from_bytes(data: &[u8]) -> Result<Self, MupcError>;
}

/// 核间通信帧
pub struct IntercoreFrame {
    pub header: FrameHeader,
    pub data: Vec<u8>,
}

impl IntercoreFrame {
    pub const FRAME_FIXED_LENGTH: usize = 64;

    pub fn new(frame_type: FrameType, seq_no: u16, data: Vec<u8>) -> Self;
    pub fn new_connect() -> Self;
    pub fn new_heartbeat_req(status: u8, cpu_temp: f64, memory_usage: f64) -> Self;
    pub fn new_heartbeat_rsp() -> Self;
    pub fn to_bytes(&self) -> Result<Vec<u8>, MupcError>;
    pub fn from_bytes(data: &[u8]) -> Result<Self, MupcError>;
}
```

### 8.5 配置接口

配置文件（`mupc.toml`）中的 intercore 配置段：

```toml
[intercore]
connect_addr = "127.0.0.1"
connect_port = 9100
heartbeat_interval_ms = 1000
watchdog_timeout_ms = 10000
```

所有通信参数支持通过 Web UI 运行时配置，无需重启服务。

---

## 9. 文件结构

### 9.1 当前实现文件结构

```
mupc/crates/intercore/
├── Cargo.toml                  # crate 配置（name = "mupc-intercore"）
└── src/
    ├── lib.rs                  # 模块导出入口
    ├── protocol.rs             # 帧协议编解码（Magic + Length + Type + SeqNo + Payload + CRC16）
    ├── tcp_server.rs           # TCP 连接管理（IntercoreServer、IntercoreConfig）
    ├── heartbeat.rs            # 心跳管理（HeartbeatManager、HeartbeatStatus）
    └── watchdog.rs             # 看门狗（Watchdog、WatchdogConfig、WatchdogState）
```

> **⚠️ v2.2 文件结构变更**：上表为 §2-§9 TCP 仿真栈快照。§11 Modbus/PCS 驱动新增
> `src/transport.rs`（IntercoreTransport trait + V2/V3 帧字节）+ `src/transport/modbus.rs`
> （PCS 驱动）+ `src/transport/tcp.rs`（TcpTransport）+ `src/pcs.rs`（PCS 点表/编解码）；
> 假设表 `src/modbus_rtu.rs` 与 `src/bin/modbus_slave.rs` 标注旧路径/仿真专用；PCS 协议
> 从站仿真为 `src/bin/pcs_slave.rs`。`tcp_server.rs`/`heartbeat.rs`/`watchdog.rs` 属 TCP
> server 栈（IntercoreServer），PCS 生产通道由 `IntercoreClient` + transport 承载。

### 9.2 文件职责说明

| 文件 | 职责 | 关键导出 |
|------|------|---------|
| `lib.rs` | 模块入口，重导出公共类型 | `IntercoreServer`, `IntercoreFrame`, `FrameType`, `FrameHeader`, `HeartbeatManager`, `Watchdog` |
| `protocol.rs` | 帧协议定义、序列化/反序列化、CRC16 计算、单元测试 | `FrameType`, `FrameHeader`, `IntercoreFrame`, `FRAME_FIXED_LENGTH` |
| `tcp_server.rs` | TCP 服务器监听、连接接受、帧分发处理 | `IntercoreConfig`, `IntercoreServer` |
| `heartbeat.rs` | 连接心跳状态管理、周期性超时检测 | `HeartbeatStatus`, `HeartbeatManager` |
| `watchdog.rs` | 看门狗超时检测、复位触发 | `WatchdogConfig`, `WatchdogState`, `Watchdog` |

### 9.3 依赖关系

```toml
[dependencies]
tokio.workspace = true          # 异步运行时、TCP、定时器
tracing.workspace = true        # 结构化日志
serde.workspace = true          # 序列化
serde_json.workspace = true     # JSON Payload 编解码
byteorder = "1.5"               # 大端/小端字节序
chrono = { workspace = true }   # UTC 时间戳
mupc-common = { path = "../common" }  # 错误类型、ErrorCode
mupc-core = { path = "../core" }      # 核心基础设施

[dev-dependencies]
tokio-test = "0.4"              # Tokio 测试工具
```

---

## 10. 技术决策记录

### 10.1 决策日志

| 序号 | 决策 | 选项 | 结论 | 理由 |
|------|------|------|------|------|
| ADR-001 | 帧格式：带 Magic 的标准二进制帧 vs 无 Magic 的固定偏移帧 | ① Magic + Length + Type + SeqNo + Payload + CRC16（标准二进制帧，PRD/代码实现）；② Type + Length + SeqNo + ai_ready + strategy_mode + Reserved（早期技术设计） | **采用方案①** | 与 PRD 规范和实际代码实现一致；Magic 字段（0xAA55）提供帧同步能力，提高异常恢复鲁棒性 |
| ADR-002 | 帧定长策略 | ① 固定 64 字节；② 变长帧 | **固定 64 字节** | 简化嵌入式实时控制模块的缓冲区管理；避免变长帧的解析复杂度；有效数据 54 字节足够容纳单帧数据 |
| ADR-003 | 有效数据编码格式 | ① JSON 字符串（字节流）；② 纯二进制 | **JSON 编码** | 可扩展性强，便于调试和后续扩展指令类型；嵌入式端 JSON 解析开销经评估可接受 |
| ADR-004 | 网络拓扑：主控为服务端 vs 主控为客户端 | ① 主控服务端，实时控制模块客户端主动连入；② 主控客户端，主动连接实时控制模块 | **主控客户端** | 主控主动连接实时控制模块（默认 9100），由主控掌握连接生命周期与重连节奏，便于统一管理与仿真环境（sim-bridge 作服务端）对齐 |
| ADR-005 | 字节序选择 | ① 帧头字段统一大端（network byte order）；② 混合字节序 | **帧头大步端，Payload 中 f64 小端** | 帧头使用大端符合网络字节序惯例；f64 使用小端与 ARM 架构（RK3588）原生字节序一致，避免不必要的字节序转换 |
| ADR-006 | 序列号匹配策略 | ① u16 循环递增；② u32 递增；③ UUID | **u16 循环递增** | 与帧头长度匹配（2 字节），足够用于请求-应答匹配（65535 个并发指令远超出实际需求） |
| ADR-007 | 心跳与看门狗分离设计 | ① 心跳管理器 + 看门狗两个独立组件；② 合并为一个组件 | **分离设计** | 心跳管理器负责连接状态跟踪和健康检测循环；看门狗独立负责超时判定和告警/复位逻辑；职责分离，便于单组件测试和替换 |
| ADR-008 | CRC 算法选择 | ① MODBUS CRC16；② CRC32；③ Adler-32 | **MODBUS CRC16** | 2 字节校验满足帧传输错误检测需求；CRC16 计算开销低（每帧计算量小）；工业协议广泛采用 |
| ADR-009 | 传输通道抽象层级（新增 Modbus RTU 备选） | ① intercore 内部 `IntercoreTransport` trait，IntercoreClient 作门面；② 上层双客户端（AiIntegrator 按配置选）；③ 独立 transport crate | **intercore 内部 trait（方案①）** | 改动集中在 intercore 内部，上层（AiIntegrator/strategy-engine/web-api）接口不变、零改动；最符合"通信选择"定位（对控制逻辑透明） |
| ADR-010 | Modbus 寄存器数值编码 | ① int32 有符号缩放（2 寄存器/值）；② IEEE754 f64（4 寄存器/值） | **int32 缩放（方案①）** | 工业 Modbus 惯例、无端序歧义、寄存器占用减半；功率 ±60kW 精度 0.01kW 足够；`k_droop` 用 0.001 缩放 |
| ADR-011 | Modbus RTU 栈选型 | ① tokio-modbus（async master+server）；② 复用 rs485-plugin；③ serialport+自写帧 | **tokio-modbus（方案①）** | 纯 Rust async、同时提供 master 与 server（slave）、支持 FC03/06/16，与项目 tokio 栈契合；rs485-plugin 语义偏南向且缺 FC16 |
| ADR-012 | Modbus 通道数据面边界 | ① 控制备选（控制下行+执行确认+心跳，遥测/SafetyOverride 仍走 TCP）；② 全量对等承载 | **控制备选（方案①）** | 本系统遥测主数据流来自南向采集，RS485 带宽有限不适合大块遥测轮询；SafetyOverride 为安全即时事件，Modbus 轮询无法保证及时性；边界明确后控制链路可经 Modbus 独立承载。**⚠️ v2.2 PCS 架构修正**：PCS=实时模块仅 RS485、无 TCP 上送，遥测真实源转台区总表 master_meter（U-26）；SafetyOverride 概念废弃，由 PCS 内部保护 + AiValidator 承接；边界更新为 **Modbus 承载控制+SOC+健康，遥测转总表**（详见 ADR-013 / §11.11） |
| ADR-013 | Modbus 通道真实协议 | ① 自定义假设点表（cmd_valid/exec 确认区，早期实现）；② **PCS 真实协议 V1.3**（FC06 写即生效、分相模式、int16 缩放+字节互换） | **PCS 真实协议（方案②，v2.2）** | 实时控制模块=两级式 PCS，经现场协议资料确认点表；假设表无法对接真实设备，PCS 为标准 Modbus 从站无自建确认区 |

> **⚠️ ADR-010 修订注（2026-09-08，M11）**：ADR-010 的「int32 有符号缩放（2 寄存器/值，
> 0.01kW）」编码结论适用于已作废的假设表（§11.4-11.6 / `modbus_rtu.rs` 旧路径仿真）。
> **PCS 真实协议（ADR-013）下寄存器编码由设备协议 V1.3 固定为 int16 单寄存器 *1kW +
> 高 8/低 8 字节互换**，非本系统可选——ADR-010 不再适用于 PCS 通道，读者勿据此误采
> int32/0.01kW。ADR-011（tokio-modbus 栈选型）仍有效。

### 10.2 待澄清问题

| 序号 | 问题 | 优先级 | 状态 |
|------|------|--------|------|
| 1 | 看门狗超时触发实时控制模块复位的策略是否启用（PRD 标记为"可选"） | 低 | 待确认 |
| 2 | StatusReport 和 DataUpload 的上送周期是否相同，是否需要差异化配置 | 低 | 待确认 |
| 3 | 是否需要实现指令队列/缓存机制（连接断开时暂存指令） | 中 | 待确认 |
| 4 | 是否需要支持多个实时控制模块同时连接（当前实现已支持 HashMap 存储多连接） | 中 | 待确认 |

---

## 11. 传输通道抽象与 Modbus RTU 备选链路

### 11.1 背景与目标

部分现场以太网（TCP/RJ45）布线不可行或距离受限，需在现有 TCP 核间链路之外，提供一条 **Modbus RTU（RS485）备选链路**。要求：

- **通道可选择**：通过配置 `intercore.transport` 选择走以太网（`tcp`）或 Modbus RTU（`modbus_rtu`），部署时二选一，非运行时热备；
- **上层透明**：控制指令下发（AI 双参数 / 台区储能分相 P/Q）、状态查询接口不变，策略引擎、Web API 零改动；
- **Slave 参考实现**：本 repo 同时提供 Modbus RTU Slave 参考实现（模拟实时控制模块），便于无外部固件时本地联调验证寄存器映射。

**数据面边界（ADR-012）**：Modbus 通道承载**控制下行 + 执行确认 + 心跳/健康状态上行**；**遥测上送（StatusReport/DataUpload）与 SafetyOverride 事件仍走 TCP 以太网链路**。依据：本系统遥测主数据流来自南向采集（非核间实时模块上送），RS485 带宽有限不适合大块遥测轮询；SafetyOverride 为安全关键即时事件，Modbus 轮询模式无法保证及时性。走 `modbus_rtu` 时遥测/SafetyOverride 依赖 TCP 存在——若现场完全无以太网，须另行评估遥测路径（不在本次范围）。

> **⚠️ v2.2 PCS 架构取代注**：上述 ADR-012 边界中「遥测/SafetyOverride 仍走 TCP」基于「自定义小脑实时模块 + TCP 上送」假设。**PCS 架构下（ADR-013 / §11.11）**：实时模块 = 两级式 PCS，**仅 RS485（Modbus 从站），无 TCP 上送**；台区电气遥测**真实源 = 台区总表 master_meter（U-26 独立 RS485）**，非 PCS 核间上送；原 SafetyOverride 概念**废弃**，由 **PCS 内部保护 + AiValidator/策略校验**承接；PCS 3 区 1029-1036 输出功率仅可作健康/校验，不并作策略遥测。故 ADR-012 数据面边界更新为：**Modbus 承载 控制 + SOC + 健康**，**遥测转总表**（不再依赖 TCP 承载遥测/安全事件，原「无以太网现场遥测路径未覆盖」风险消解）。`transport=tcp` 帧协议仅用于仿真/联调（sim-bridge 作 TCP 服务端）。

### 11.2 可行性评估

| 维度 | 评估 | 结论 |
|---|---|---|
| 数据量 | 核间控制为秒级下发，单帧负载小（AI 双参数 2 个 f64；台区分相 6 个 f64） | ✅ 保持寄存器（16bit）足以承载 |
| 实时性 | Modbus 写/读周期 10~100ms，核间控制周期 1s 级 | ✅ 满足 |
| 带宽 | RS485 常用波特率 9600~115200，控制数据量小 | ✅ 充足 |
| 校验 | Modbus RTU 自带 CRC16 | ✅ 帧校验完备 |
| 基础设施 | rs485-plugin 已有 Modbus CRC16 / 寄存器读写实现可参考 | ✅ 无需从零写帧 |
| 栈选型 | tokio-modbus 提供 async master + server（slave） | ✅ 与 tokio 栈契合 |
| **主要风险** | ① 点表须与 **PCS 固件 V1.3** 对齐（寄存器/字节互换/符号约定，§11.11 契约清单；假设表 Slave 参考仅仿真）；② 串口物理层（RS485 接线/终端电阻/DE/RE 方向控制）需现场验证；③ **v2.2 修正**：遥测源=台区总表 master_meter（U-26），不依赖 PCS/TCP（原 ADR-012"遥测走 TCP"在 PCS=实时模块架构下消解，见 §11.1/§11.11） | ⚠️ 依赖 PCS 固件确认 |

### 11.3 Transport 抽象（intercore 内部，上层零改动）

`IntercoreClient` 由「TCP 客户端」重构为「传输门面」：内部持 `Arc<dyn IntercoreTransport>`，按配置选 Tcp 或 ModbusRtu。**上层接口（`send_dual_param`/`send_tai_command`/`is_connected`）签名不变**。

```rust
// intercore/src/transport.rs
#[async_trait]
pub trait IntercoreTransport: Send + Sync {
    /// 下发 AI 双参数（p_ref/k_droop）
    async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError>;
    /// 下发台区储能分相 P/Q（核间 V3）
    async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], mode: &str) -> Result<(), MupcError>;
    /// 连接状态（心跳/看门狗检测用）
    async fn is_connected(&self) -> bool;
    async fn shutdown(&self) -> Result<(), MupcError>;
}
```

- **TcpTransport**：现有 TCP 帧封装逻辑（`send_frame` + 持久 TcpStream + V1/V2/V3 JSON payload）原样迁移，协议不变；
- **ModbusRtuTransport**：Master，不传帧字节，按 §11.4 寄存器映射写控制寄存器 + 轮询读状态；
- `IntercoreClient` 保留 `connected`/`last_p_ref`/`last_k_droop` 门面状态，委托给 transport。

### 11.4 Modbus 寄存器映射与编码

> ⚠️ **v2.2 注**：本节自定义假设点表（控制/执行确认/状态三区、`cmd_valid`/`exec_seq`/int32 缩放）**已被 §11.11 PCS 真实协议 V1.3 取代**——`transport=modbus_rtu` 实际对接两级式 PCS（FC06 写即生效，无自建确认区，int16 缩放 + 高 8/低 8 互换）。本节保留作通用 Modbus 传输框架参考（transport 抽象/读写/心跳轮询思想），点表以实现 §11.11 为准。

实时控制模块（Slave）持有一块保持寄存器区，分三区：**控制区**（Master 写，FC16）、**执行确认区**（从站写、Master 读，FC03）、**状态/心跳区**（从站写、Master 读）。从站地址可配（默认 1）。

| 分区 | 地址 | 内容 | 方向 | 编码 |
|---|---|---|---|---|
| 控制区 | **0x0000** | `cmd_ctrl` 命令控制字 | 写 | 低字节：bit0 `cmd_valid`（上升沿触发从站采样生效）、bit1-3 `strategy_mode`、bit4 `ai_ready`；**高字节：`cmd_seq`（Master 每次下发递增 u8，用于执行确认匹配）** |
| 控制区 | **0x0001** | `protocol_version` | 写 | u16，Master 写期望协议版本（当前 1）；从站校验不符则拒采纳并置 `exec_status` 失败 |
| 控制区 | **0x0010-0x0011** | `p_ref` | 写 | int32 有符号，0.01 kW/LSB |
| 控制区 | **0x0012-0x0013** | `k_droop` | 写 | int32 有符号，0.001 kW/V·LSB |
| 控制区 | **0x0020-0x0021** | `phase_p[A]` | 写 | int32，0.01 kW/LSB |
| 控制区 | **0x0022-0x0023** | `phase_p[B]` | 写 | 同上 |
| 控制区 | **0x0024-0x0025** | `phase_p[C]` | 写 | 同上 |
| 控制区 | **0x0026-0x0027** | `phase_q[A]` | 写 | int32，0.01 kVAr/LSB |
| 控制区 | **0x0028-0x0029** | `phase_q[B]` | 写 | 同上 |
| 控制区 | **0x002A-0x002B** | `phase_q[C]` | 写 | 同上 |
| 执行确认区 | **0x0030-0x0031** | `exec_seq` | 读 | int32，从站回写本次采纳的指令序号（对应 ControlRsp.seq_no） |
| 执行确认区 | **0x0032** | `exec_status` | 读 | u16：0 空闲 / 1 执行中 / 2 执行成功 / 3 执行失败 / 4 超时（对应 ControlRsp.result） |
| 执行确认区 | **0x0033** | `exec_error` | 读 | u16 错误码（对应 ControlRsp.error_msg，映射表从站实现） |
| 状态区 | **0x0100** | `heartbeat_counter` | 读 | u16，实时模块周期递增（master 轮询判在线/超时） |
| 状态区 | **0x0101** | `device_status` | 读 | u16 状态字（bit0 运行/bit1 故障…） |
| 状态区 | **0x0102-0x0103** | `cpu_temp` | 读 | int32，0.01 ℃/LSB |
| 状态区 | **0x0104-0x0105** | `memory_usage` | 读 | int32，0.01 %/LSB |

**编码选择**：int32 缩放（2 寄存器/值）而非 IEEE754（4 寄存器/值）——工业 Modbus 惯例、无端序歧义、寄存器占用减半；功率 ±60kW 精度 0.01kW 足够。`k_droop` 数量级小，用 0.001 缩放。

**写生效 + 执行确认流程（对齐 PRD §3.1 ControlCmd/ControlRsp 语义）**：
1. Master 递增 `cmd_seq`，FC16 写整块数据寄存器（含 0x0001 版本，首次）；
2. 写 `cmd_ctrl`（低字节 `cmd_valid=1` + strategy_mode/ai_ready；**高字节 = 当前 `cmd_seq`**）→ 从站 `cmd_valid` 上升沿采样整块，防半写采纳；
3. 从站校验版本/采纳 → 写 `exec_seq = cmd_seq` + `exec_status`（2 成功 / 3 失败 + `exec_error`）；
4. Master 轮询读 0x0030-0x0033：`exec_seq == cmd_seq` 且 `exec_status==2` → 指令确认；`==3` → 失败（读错误码）；**超时（5s，对齐 PRD）→ 标记失败，可选重试最多 2 次**；
5. **从站采纳后自清 `cmd_valid`**（本实现约定：Master 不清 valid，每次新指令由从站自清后形成新的 0→1 上升沿）；Master 读到 `exec_seq` 匹配即完成本次下发确认，无需额外清位。

**心跳与离线判定**：Master 定时 FC03 读 `0x0100`，计数递增 → 实时模块在线；**连续 N 次（默认 3，对齐 PRD §4.2 丢失阈值）读失败或计数无变化 → 判离线**（替代 TCP 心跳帧/看门狗语义），恢复后判在线。

### 11.5 Modbus RTU 栈选型与实现拆分

选 **tokio-modbus**（async master + server），底层 tokio-serial（serialport，跨平台串口）。

```
intercore/
├── transport.rs          # IntercoreTransport trait
├── transport/tcp.rs      # TcpTransport（现有 TCP 逻辑迁移）
├── transport/modbus.rs   # ModbusRtuTransport（Master）
├── modbus_rtu.rs         # 寄存器映射表 + f64↔int32 编解码 + cmd_ctrl 触发
└── bin/modbus_slave.rs   # Slave 参考实现（模拟实时控制模块）
```

### 11.6 Slave 参考实现（modbus_slave.rs）

独立二进制，模拟实时控制模块：tokio-modbus server 绑定串口，暴露寄存器区；内部维护控制区副本，收到 `cmd_valid` 上升沿时——① 校验 `protocol_version`，② 采样整块数据寄存器更新"生效指令"，③ 回写 `exec_seq`/`exec_status`（模拟采纳执行结果，含可注入失败以测重试路径），④ 清 `cmd_valid`；周期递增心跳计数。用途：本地联调验证映射与执行确认（虚拟串口对），并作为实时模块固件的寄存器协议参照。

### 11.7 配置结构（core_config.rs）

```yaml
intercore:
  transport: "tcp"              # "tcp" | "modbus_rtu"（生产主链路=modbus_rtu→PCS；tcp 仅供仿真/联调，sim-bridge 作 TCP 服务端；示例默认 tcp 保现有行为）
  host: "192.168.1.2"           # TCP 参数（transport=tcp 仿真/联调用）
  port: 9100
  heartbeat_interval_sec: 5
  reconnect_interval_sec: 3
  modbus_rtu:                   # PCS 通道参数（transport=modbus_rtu 用，生产）
    serial_port: "/dev/ttyS0"   # PCS 主链路默认（§12.1）：BECG-3568 板载 COM1（无 ttyS1）；Linux 例，Windows 用 COM3
    baud_rate: 19200            # PCS 默认 19200 N-8-1
    data_bits: 8
    stop_bits: 1
    parity: "none"
    slave_addr: 1               # PCS 从站地址（拨码，默认 1）
    response_timeout_ms: 200
    heartbeat_poll_ms: 1000
```

`InterCoreConfig` 新增 `transport: String`（默认 `"tcp"`）+ `modbus_rtu: ModbusRtuConfig`。startup 按 `transport` 构造 transport 实例注入 `IntercoreClient`。

### 11.8 改动文件清单

| 文件 | 改动 |
|---|---|
| intercore/src/transport.rs + transport/{tcp,modbus}.rs | IntercoreTransport trait + 两实现（新） |
| intercore/src/modbus_rtu.rs | 寄存器表 + 编解码 + cmd_ctrl（新） |
| intercore/src/bin/modbus_slave.rs | Slave 参考实现（新） |
| intercore/src/tcp_server.rs | IntercoreClient 改持 `Arc<dyn IntercoreTransport>`，TCP 逻辑抽出为 TcpTransport |
| intercore/Cargo.toml | + tokio-modbus / tokio-serial |
| mupc-core-bin core_config.rs / startup.rs | 配置扩展 + 按 transport 构造 |
| deploy/config/mupc_core_config.yaml | intercore 段加 transport/modbus_rtu |
| intercore 测试 | 编解码 roundtrip / cmd_valid 触发 / 心跳在线 |

**上层零改动**：AiIntegrator、strategy-engine、web-api 接口不变。

### 11.9 验证方式

1. **单元**：f64↔int32 编解码 roundtrip、cmd_ctrl 位操作、寄存器地址表；
2. **端到端联调**：虚拟串口对（Windows com0com / Linux socat）→ Master ↔ Slave 参考 → 下发分相 P/Q 生效 + 心跳在线检测；
3. **回归**：`transport: "tcp"` 下现有 AI/本地优先下发全跑通（TcpTransport 不改变协议）。

**验证状态（2026-09-04）**：transport 抽象 + Tcp/Modbus 双实现 + Master/Slave + 配置已实现，`mupc-intercore` lib 22 测试全绿（含寄存器编解码 roundtrip、cmd_ctrl、心跳帧），`cargo check --workspace` 通过（上层调用方编译不变）。端到端 Modbus 联调（虚拟串口对下 Master↔Slave 下发生效）待具备串口环境（com0com/socat 或现场 RS485）执行；`transport: "tcp"` 回归不受影响。

### 11.10 依赖与风险确认（实现前）

1. ~~**寄存器映射表须与实时控制模块固件对齐**（地址/缩放/`cmd_valid` 触发/`exec_status` 语义；关键契约 I-2：从站采纳后自清 `cmd_valid`，否则自第 2 条指令起无上升沿）~~：**⚠️ v2.2 本节假设点表契约已被 §11.11 PCS 真实协议 V1.3 取代**——PCS 为标准 Modbus 从站，FC06 写响应即确认，无 `cmd_valid`/`exec` 确认区；契约改为与 **PCS 固件/协议 V1.3 点表对齐**（高 8/低 8 互换、符号约定、模式切换时序），见 §11.11 及「待厂方确认清单」；
2. RS485 物理层：接线极性、终端电阻、DE/RE 方向控制（半双工）需现场核验；
3. Modbus RTU 点对点（1 Master : 1 Slave），不支持现有 TCP 的多连接场景（§10.2 待澄清问题 4 仅适用 TCP）；
4. **下发延迟预算**（对齐 PRD §7.1 ≤50ms）：PCS 一次分相下发 = 模式字 1000 + 最多 6 个数据寄存器（1006-1011）逐 FC06 写（每帧单寄存器），19200bps 下单帧往返约 2~4ms、7 帧串行约 15~30ms，需以实测确认满足 50ms（半双工 RS485 往返与从站响应超时计入）；
5. ~~**遥测/SafetyOverride 依赖 TCP**（ADR-012 边界）：`transport=modbus_rtu` 时须保证 TCP 链路仍承载遥测与安全事件~~：**⚠️ v2.2 PCS 架构已取代本项**——PCS=实时模块仅 RS485、无 TCP 上送，遥测真实源转台区总表 master_meter（U-26），SafetyOverride 概念废弃（由 PCS 内部保护 + AiValidator 承接）；Modbus 只承载 控制+SOC+健康，不再依赖 TCP 承载遥测/安全事件（原「无以太网现场遥测路径未覆盖」风险消解）；
6. **配置热切偏离**（对齐 PRD §7.4）：`transport` 为部署配置二选一，需重启生效；Web UI 运行时切换不实现，记为此处对 PRD 可维护性需求的授权偏离；
7. **日志脱敏**（对齐 PRD §7.3）：PCS 寄存器写值（控制数值）不入日志，仅记录指令类型/结果/寄存器地址（PCS 无 `cmd_seq`）。
8. **写确认超时/重试授权偏离（2026-09-08 记，M4）**：PRD IC-AC-36「写响应超时 5s→失败→可选重试≤2」源于假设表时代的指令级确认语义（§11.4 已作废）。PCS 真实协议 FC06 单帧写响应由串口层 `response_timeout_ms=200` 兜底（帧级超时），**未实现指令级 5s 超时与显式重试**——以 1s 控制周期下周期重发整序列作隐式重试（FC06 幂等）。与 PRD 的量化差异记为授权偏离；实机若暴露单帧响应 >200ms 再上调帧级超时并评估显式重试。

---

### 11.11 PCS 真实协议 V1.3（v2.2，取代 §11.4~11.6 假设点表）

**架构确认（2026-09-04）**：实时控制模块 = **两级式 PCS 设备**（小脑集成于 PCS）。MUPC 作 EMS/主机（Modbus Master）经 RS485 直连 PCS（从站），`transport: "modbus_rtu"` 即此真实通道。**§11.4~11.6 的自定义假设点表（cmd_ctrl/exec 确认区）作废**——PCS 为标准 Modbus 从站，FC06 写响应即确认。依据：PCS 设备通讯协议 V1.3（`60kW 双级式PCS产品资料包/5.通讯协议/`）。

**`transport=tcp` 角色**：生产主链路默认 **`modbus_rtu` → PCS**（上述真实通道）；`tcp`（TCP 帧协议 + V1/V2/V3 JSON）仅供**仿真/联调**——sim-bridge 作 TCP 服务端。v2.1「TCP 回读 DataUpload SOC」路径标注**仿真专用**，**生产 SOC 一律读 PCS 3 区 1010**（读失败按 AiIntegrator 5s 新鲜度判过期），不依赖 TCP 回读。

**物理层**：RS485 Modbus，默认 **19200 N-8-1**，从站地址拨码（默认 1），EMS 接 A2/B2。**⚠️ 高 8 位/低 8 位互换**——寄存器 16bit 收发须字节交换。

**点表映射**（地址列 = Modbus 寄存器地址；3 区 FC04 读、4 区 FC03 读/FC06 写单）：

| PCS 点表 | 地址 | 类型/缩放 | 承载 |
|---|---|---|---|
| 4区 模块启停 | 500 | UInt16 0/1 | 启动序列 |
| 4区 有功模式 | 1000 | UInt16 0恒功率/1恒流/**2分相**/5离网 | 通道模式 |
| 4区 恒功率有功/无功 | 1001/1002 | Int16 *1kW(正放负充) | `send_dual_param` p_ref/q |
| 4区 单A/B/C 有功 | 1006-1008 | Int16 *1kW ±25 | `send_tai_command` phase_p |
| 4区 单A/B/C 无功 | 1009-1011 | Int16 *1kVar ±25 | `send_tai_command` phase_q |
| 3区 BMS 系统 SOC | 1010 | UInt16 *1% | `latest_soc`（生产 SOC 源） |
| 3区 模块详细告警1-5 | 1000-1004 | UInt16 位映射（DSP 告警码 1-68） | 故障字（**可选**健康接入） |
| 3区 BMS 工作状态 | 1005 | UInt16（状态编码随所配 BMS 协议） | 健康（**可选**接入） |
| 3区 模块故障状态 | 1014 | UInt16 0无故障/1故障 | 保护跳闸联读（**可选**） |
| 3区 模块运行状态 | 1013 | UInt16 0停机/1待机/2充电/3放电 | 心跳/在线判定 |
| 3区 输出有功/无功分相 | 1029-1036 | Int16 *0.1 | 健康/校验（**可选**；不并作策略遥测） |

**下行执行序列**：
- `send_tai_command`（台区储能分相）：若当前模式≠2 则先 `FC06 写 1000=2`；**每次分相 P/Q 单相 clamp ±25kW**（PCS 单相功率器件独立，不可跨相补）后逐个 `FC06` 写 1006-1011（协议只支持一次设一参）；启停（500=1）首次下发带。**⚠️ 已知局限：超限静默裁剪、无回读告警**（PCS 分相无执行值回读，无法回读确认裁剪后实际下发值）
- `send_dual_param`（AI 恒功率）：模式≠0 则写 `1000=0`；写 1001=p_ref、1002=q；**`k_droop` PCS 无下垂接口——忽略**。**⚠️ 语义偏离与安全兜底**：PCS 恒功率模式**无下垂闭环**，`k_droop` 表达的电压支撑能力实际**下降**（语义偏离 AI 下垂控制目标）；AI 恒功率模式下发前**须经 AiValidator 范围校验**（越限拒绝或回退）；投产以**本地优先分相**为主、恒功率为辅助
- 模式状态缓存于 transport（AtomicU8），一致时每周期只写数据寄存器

**上行/健康**：
- `latest_soc`：FC04 读 3区1010（swap），成功存 `(soc, Instant)`；读失败返回 None（AiIntegrator 5s 新鲜度已判过期）。**生产 SOC 源 = 3 区 1010**；v2.1「TCP 回读 DataUpload SOC」路径仅**仿真/联调**用（不用于生产）
- 心跳/在线：周期 FC04 读 3区1013（运行状态）成功即在线，连续失败判离线（替换假设表心跳计数器）；原 `modbus_slave` 假设表参考仅测旧路径，PCS 以实机联调为准
- 故障字（**可选**健康接入，非实时必需）：读 3 区 1000-1004 模块详细告警位 + 1005 BMS 工作状态 + 1014 模块故障状态，**区分运行停机（1013=0 且无告警）与保护跳闸（1014=1 或告警字非 0）**联读；PCS 内部保护为第一道安全防线，MUPC 侧仅旁路观测/告警，不并作策略遥测

**上电初始化/冷启动与模式缓存重同步（2026-09-08 补，M5；框架高危项 S001）**：
- **冷启动时序**（transport 构造后、首个下行周期前）：① 装配方 spawn `run_heartbeat_loop`（startup.rs 已将其句柄入后台任务 guard）；② 心跳任务首拍 FC04 读 3 区 1013 判链路在/离线并建立 online 基线；③ SOC 由首个 `latest_soc`（FC04 读 1010）注入，AiIntegrator 自该时刻起算 5s 新鲜度；④ 首个下行指令前 transport 的 `mode=0xFF` 哨兵 + `started=false` 保证顺序 **先写 REG_MODE → 再 REG_START_STOP=1 → 后写功率**。冷启动无独立"初始判定"模块——在线基线、SOC 时间戳、模式/启停首写均落在心跳首拍与首个 send 序列，不依赖运行时周期之外的特殊分支。
- **模式/启停缓存重同步缺口**：`mode`（AtomicU8）/`started`（RwLock）仅写侧缓存，**无 4 区 1000/500 回读校验**。链路断线 → W1 已令 `mark_offline` 清缓存（0xFF 哨兵/false）→ 下次指令强制重写，此路径已闭环。**链路在线但 PCS 被第三方（HMI/外部）切换模式/停机时缓存失步**：每周期 `ensure_mode` 仅当缓存≠目标才写，失步会致模式未重写而功率照写（PCS 按错误模式执行）。缓解：① 待厂方确认 4 区 1000 是否可 FC03 读回，可读则周期读回比对；② 不可读则 send 前无条件写模式字（额外 1 帧）——均列契约待确认项。

**停机观测与状态字校验（M1/M9a，2026-09-08 代码落地）**：
- 心跳每拍**解码 1013 值**（M9a）：仅 ∈0..3（0 停/1 待机/2 充电/3 放电）视为健康读数判在线；乱码/错位帧（可通过 Modbus CRC 的罕见坏帧）按坏读数计数，不判在线。
- **停机观测（M1）**：心跳读到 1013=0（停机）且 transport 此前已下发启动（`started=true`）→ 判定远端停机（保护跳闸/人工停机），`tracing::warn` 告警一次（去抖）。**策略=保守不自动重启**：不动 `started` 缓存、不重发 500=1（PCS 启停 500 电平/边沿语义待厂方确认，自动重启可能造成保护跳闸-重启振荡）；恢复动作留给上层/运维决策。该观测打破「链路在、PCS 已停、MUPC 静默继续写功率」盲区（上一轮 W1 仅覆盖链路断线场景）。

**配置**：复用 `core_config ModbusRtuConfig`（serial_port/baud/slave_addr/超时），§11.7 配置示例波特率已为 **19200**（PCS 默认）；点表地址/字节序为代码常量映射（`addr_base` 可配供现场校准）。

**容量对齐**：策略 `arbitrate`（i_rated 190A≈41.8kVA/相）高于 PCS 分相限（±25kW≈110A/相）——投产时按 PCS 容量调策略容量参数（投产项），PCS 侧 clamp 为硬限兜底。

**范围与投运前提**：
- **多台 PCS 并机不在本轮**：当前为点对点（1 Master : 1 Slave）；并机扩容另行设计；
- **型号基线**：以 **60kW 双级式 PCS V1.3** 协议为基线；**125kVA 型号点表待厂方确认**后方可对接（地址/缩放可能不同）；
- **符号约定核相（本通道投运前提）**：出厂/点表符号约定**正放负充**，投运前须现场**核相**——Q 阶跃（无功注入方向）+ 分相注流，确认 PCS 各相实际充放方向与相位对应后再启用闭环控制。

**PCS 契约待厂方确认清单**（协议 V1.3 未明示或需固件行为确认，投产前逐项与 PCS 厂商对齐）：
- **模式热切换**：1000 有功模式（恒功率/分相/恒流/离网）间切换是否需停机或重启？切换瞬时行为是否安全？
- **启停 500 时序**：启动序列（先 500=1 再写功率，抑或先写功率再启动？）；停机是否有渐变斜坡、是否需回零后再停？
- **4 区 502/503 充放使能语义**：与模式字、功率设定值的联动（写功率前是否必须先使能？使能=0 是否复位功率？）；
- **符号约定**：恒功率/分相正放负充的固件最终确认（正放负充为设计默认，须厂方背书）；
- **1018 有功功率变化率联动**：PCS 内部变化率限值与 MUPC 策略 ramp 的协调，避免两侧限值叠加导致响应滞后或超调。

**测试**：PCS int16 缩放/字节 swap 编解码 roundtrip、单相 clamp、模式切换缓存、写序列组装；**软件端到端**（M10）：`src/bin/pcs_slave.rs`（PCS V1.3 协议从站仿真，按启停+有功方向推演 1013 运行状态、1010 SOC 恒 66%）经虚拟串口对（Linux socat / Windows com0com）与 `ModbusRtuTransport` 对打，验证寄存器映射/字节互换/写序列/心跳判定；**最终端到端以真实 PCS RS485 联调**（填点表 / 核相）。

**验证状态（2026-09-04）**：pcs.rs 编解码 + `ModbusRtuTransport` PCS 驱动重构完成，`mupc-intercore` lib 31 测试全绿（含 PCS 编解码 roundtrip / SOC 3 区 1010 校验 / 心跳 REG_RUN_STATE(1013) 判定），`cargo check --workspace` 通过（上层调用方零改动）。端到端 PCS 实机 RS485 联调待 PCS 硬件（填点表 / 核相 / 并机基线）；PCS 契约待确认清单（模式热切换 / 启停 500 时序 / 4 区 502-503 / 符号约定）仍未获厂方答复。

**验证状态补记（2026-09-08，code-reviewer W1-W3 + 项目级审查 Action 修复）**：
- W1 离线清缓存 / W2 波特率默认 19200 / W3 总线事务互斥（`bus: Mutex<()>` 入口持锁）落地；`cargo check --workspace` 0 error，intercore lib 测试通过。
- 项目级审查修复：M1 停机观测（1013=0 告警、不自动重启）、M9a 运行状态值校验（∈0..3）、M6 删只写不读的 soc 缓存、M3 startup transport 显式 match（未知值启动报错）、M8 心跳句柄入后台任务 guard、M7 config.validate 校验 modbus_rtu 配置合法性及与总表串口互斥。
- 部署/测试配套：`mupc/deploy/config/mupc_core_config.production.yaml`（transport=modbus_rtu 生产模板，与仿真 tcp 默认配置分离）；`src/bin/pcs_slave.rs` PCS 协议从站仿真（见上测试）。
- 文档补记：ADR-010 取代注（M11）、§11.10 授权偏离第 8 条（写超时/重试 M4）、冷启动/缓存重同步与停机观测（M5/M1）、§7.1 PCS 形态健康映射说明（M12）。

## 12. BECG-3568 现场接线契约与安全联锁（v2.3/v2.4）`[DESIGN_APPROVED: 2026-09-08]`

> **目标平台变更**：MUPC 运行硬件为 **BECG-3568 BOX**（瑞芯微 RK3568 四核 A55 @2.0GHz、NPU 1TOPS、板载 8 路隔离 RS485 / 16 路隔离 DI / 6 路继电器 DO / 2 路 CAN / 4 路 ADC / 4×千兆网口）；后续换 **RK3588 型号接口完全一致**（仅 NPU/OTA 侧按 3588 SDK 变化，见 05 AI 引擎与 OTA 模块）。
> **板载串口节点**：COM1-8 ↔ `ttyS0` / `ttyS2` / `ttyS3` / `ttyS4` / `ttyS5` / `ttyS6` / `ttyS7` / `ttyS8`（**无 ttyS1**，A0→ttyS0、A2→ttyS2 … A8→ttyS8）；无「USB 转 485」概念（历史 `/dev/ttyUSB0` 假设在 BECG 上不成立）。
> **DI/DO GPIO 编号**（按规格书 V1.1）：DI1=124 / DI2=125 / DI3=102 / DI4=103 / DI5=104 / DI6=66 / DI7=63 / DI8=64 / DI9=65 / DI10=88 / DI11=89 / DI12=90 / DI13=91 / DI14=148 / DI15=154 / DI16=23；DO1=97 / DO2=107 / DO3=19 / DO4=108 / DO5=109 / DO6=110。编号以板端导出后实际 `gpioN` 校准（配置化容忍 chip 偏移）。

### 12.1 PCS 主链路物理接线契约（S1，v2.3）

**PCS 主链路**：**RS485-1 / COM1 / `/dev/ttyS0` ↔ PCS A2/B2，19200 N-8-1**（V1.3 线格式）。`intercore.modbus_rtu.serial_port` 默认 `/dev/ttyS1 → /dev/ttyS0`（YAML 可覆盖，现场以接线为准）；实施须**同步更新 core_config 默认常量与单测断言**，并建议 validate 在 `transport=modbus_rtu` 时启动即探测串口存在性（fail-fast，不等首帧超时）。总表等站级 485 节点完整分配见 **02 南向 §10 统一调度** 与 **deploy/deploy.md 现场接线章**。

台区储能现场接线总表（BECG-3568 作 MUPC/EMS 主控）：

| RS485 口 | 端子/节点 | 设备 | 数据归属 |
|---|---|---|---|
| RS485-1 | COM1/`ttyS0` | PCS 储能变流器（A2/B2，19200 N-8-1） | 本模块 `modbus_rtu`（§11.11 V1.3） |
| RS485-2 | COM2/`ttyS2` | BMS | 02 §10（role=battery，SOC 融合见 §12.6 交叉注） |
| RS485-3 | COM3/`ttyS3` | 空调 | 02 §10（role=hvac，本版遥测） |
| RS485-4 | COM4/`ttyS4` | 关口表/台区总表 | `master_meter` → 02 §10（role=meter_grid，策略 phase 源） |
| RS485-5 | COM5/`ttyS5` | 储能表（第二表计） | 02 §10（role=meter_batt） |
| RS485-6 | COM6/`ttyS6` | 消防状态 | 02 §10（role=fire） |

DI/DO 分配（联锁输入/状态输出，接线见 deploy.md）：DI1 急停(124)、DI2 水浸(125)、DI3 消防报警(102)、DI4 门禁(103)、DO1 运行灯(97)、DO2 故障灯(107)。

### 12.2 PCS 停机原语与联锁锁存（S2，v2.4）

**背景（Why）**：V1.3 驱动**只有启动无停机原语**——每次 `send_tai_command`/`send_dual_param` 前置 `ensure_started` 自动写 `REG_START_STOP(500)=1`；M1 停机观测（1013=0）仅告警不动作；心跳恢复后下一次下发即自动重启。安全联锁（急停/水浸/消防报警）要求**可靠停机且禁止自动重启**，故须新增显式停机原语 + 锁存停机态，堵住「自动重启」路径。

**双 latch 模型（C-1，安全关键）**：两个 latch 语义一致、必须同步：① transport `stopped_latched`（运行期挡启动兜底）② storage/interlock DB latch（持久化，Web/CLI/重启读回）。**介入时刻 = 联锁触发沿（DI 有效且去抖通过）即无条件置两个 latch——不依赖 `stop()` 写 500=0 成功**（写失败/链路离线期间 transport 兜底恒闭，恢复后亦不开洞）；`stop()` 仅负责「写 500=0 + 复位 `started`/`mode` 缓存」，**不设/不清 latch**。清/置唯一入口：`clear_interlock_latch()`（release 前置校验通过后）与 `restore_interlock_latched(bool)`——**运行时联锁触发沿由 interlock 以 `restore_interlock_latched(true)` 置 transport latch（运行时置位 / 启动 DB 读回两用）**，DB latch 同步置位；`stopped_latched` 不由 `stop()`/`ensure_started` 变更。§12.5 测试补：注入写失败 → stopped_latched 仍 true → `ensure_started`/`send_*` 拒绝 → 链路恢复重试成功且 release 后才可重启。

**方案（How）**：
1. **停机原语**：`ModbusRtuTransport` 新增 `stop()`——写 `REG_START_STOP(500)=0`（V1.3：停=0/启=1），**不设/不清 latch（置位由触发沿经 restore 完成，见上方 C-1）**，仅复位 `started=false`（否则人工 release 后 `ensure_started` 见 `started==true` 会跳过写 500=1 → 释放后无法重启，静默失效）**并复位 `mode` 缓存至 0xFF 哨兵**（否则 release 后 `ensure_mode` 见缓存==目标跳过 REG_MODE=1000 重写，可能在错误模式下直接写功率——自命令停机可低成本规避；测试加「stop → release → 首条 send 必含 REG_MODE 重写」断言，I-1）。直接写 500=0（不含先降功率的渐变序列；PCS 侧执行停机，深停机/渐变语义待厂方点表答复后如需再扩展）。**停机确认（R-I）**：stop() 后由 core-bin interlock 经 `last_run_state()` 周期读确认 1013 转 0（窗口 `stop_confirm_ms` 可配，默认 5000ms）；写失败/超时未确认 → latch 仍置位、状态升级「stop_failed（停机未确认）」，interlock **周期重试 `stop()`** 直至 1013=0 确认（与 confirmed stopped 区分，见 12.3）；**链路离线期暂停重试**（退避至 `stop_confirm_ms` 级，防反复 open 串口，L-3）。PCS 深停机/渐变停机时长待厂方（挂 §11.11 待确认清单「启停 500 时序」项）。
2. **锁存挡启动（底层兜底）**：`ensure_started` 与所有写启动序列在 `stopped_latched == true` 时**拒绝写 500=1 并 warn**（即便上层误发也不重启）。**接口入 trait**：`IntercoreTransport` 增 `async fn stop()` / `async fn is_interlock_stopped() -> bool` / `fn last_run_state() -> Option<u16>`（1013 最新解码值，心跳循环维护、供上层与 DO 驱动消费）；`IntercoreClient` 门面转发。`transport=tcp`（仿真）stop 为降级 no-op+记录（无 PCS 启停概念）。**M1 自命令豁免**：心跳停机观测改为仅 `st==0 && started && !stopped_latched` 才告警——自命令停机（latch）静默，避免把软停误报成异常跳闸。
**latch 期间整条下行中止**：`send_tai_command`/`send_dual_param`（ensure_mode→ensure_started→功率/模式写）在 latch 前置检查处直接返回错误、**不写任何寄存器**——避免 stop_failed（PCS 仍运行）时后续周期按设定继续出力（设计评审 2 轮 R-B）。**清 latch/恢复接口**：trait 增 `async fn clear_interlock_latch()`（Web release / 本地 CLI 旁路调用，随后 ensure_started 重新允许启动）与 `async fn restore_interlock_latched(bool)`（mupcd 启动 DB 读回在首个策略/下发行前调用，防启动窗口抢先写 500=1）（R-A）。
3. **持久化**：联锁触发/latch 沿事件记录写 SQLite（storage `faults`/`events` 复用），mupcd 重启读回仍禁启（见 12.4）。
4. **上层联动**：ai_integration / dispatch 决策前查 latch（锁存期间不产生新的启动/功率指令），transport 层再兜底——双层防重启。
5. **链路恢复前置校验（M1 守卫升级）**：链路断线恢复后、首个写启动序列**前**须先以一次 1013 读校验 PCS 非停机（与 `started` 缓存一致）才允许 `ensure_started` 写 500=1；读到 1013=0 按 M1 处理（不重启、告警、交上层）——堵住「离线窗口内保护跳闸 → 链路恢复自动重启跳闸机」路径。
6. **M1 保护跳闸态运维恢复（I-1）**：保护跳闸（非 latch、`!stopped_latched`）不置 latch；未确认前 MUPC 不自行补发启动（现状保证）。提供显式运维确认入口（如 `POST /api/v1/interlock/ack_m1`，或复用 release 语义授权重发 500=1）：人工确认后复位 `started` 并允许重发启动，避免恢复只能靠重启 mupcd。DO2 故障灯数据源 = **持久条件** `st==0 && started && !stopped_latched`（非一次性告警事件）。

### 12.3 DI/DO 安全联锁控制器

**组件边界**：
- **GPIO 抽象层（新轻量 crate `mupc-io`）**：`DigitalIn`/`DigitalOut` trait（read/write/direction），实现①**sysfs**（`/sys/class/gpio` export+`gpioN/{value,direction}`，与 rs485-plugin DE/RE 先例一致，**先落地**）②libgpiod 桩（接口预留切换点，双实现）。IO 编号配置化。
- **联锁控制器（core-bin 新模块 `interlock`，独立 ~100ms 轮询 task）**：DI 采样 → 去抖（每 DI 去抖计数）→ 状态沿检测 → 按 `action` 处置；DO 按状态驱动；释放状态机。

**DI→动作映射**（action 配置化）：急停/水浸/消防报警（active_low 按现场可配，急停默认 NC 断线触发）触发 **pcs_stop**；门禁仅事件。

**联锁触发动作（顺序，C-1）**：① 置 latch（transport `restore_interlock_latched(true)` + DB 持久化）→ ② 调 `client.stop()`（12.2）→ ③ `events` 落库 / SSE 告警 / DO2 故障灯亮。先置 latch 后 stop()，防 stop() 串口阻塞延迟 DB 落库（L-1 窗口）。
**fail-safe 与失效策略**：GPIO 初始化失败或 DI 读失败一律按**触发（latch+告警）**处理，禁止按未触发继续运行；startup 增加 GPIO 自检步骤，**自检同步采样各联锁 DI——处于触发态则直接重新 latch**（DB 非重启禁启唯一依据，兜底触发→落库间的掉电/崩溃窗口，L-1）。**去抖按 DI 细分**（`debounce_count` 移入每 DI 配置），急停/水浸/消防默认 1 次采样即触发（近即时），普通 DI 保留去抖（poll_ms=100）。**投运前提（S-3）**：联锁软停经 Modbus 500=0 传达，急停/水浸/消防的**硬停机须 PCS 侧独立干接点急停回路兜底**（现场核对 PCS 是否自带硬急停端子），MUPC 软停为第一层；stop_failed 时 DO2/告警升级提示停机未确认。
**消防双源语义（R-G）**：停机触发**仅以 DI3（干接点，高完整性主判）为准**；RS485 fire 站（02 §10 role=fire）作确认/校核、不独立触发停机；DI3 与 RS485 状态不一致（一触发一正常）产生「消防双源不一致」告警事件（融合规则 02 §10.4）。**跨源交互归属（I-2）**：southd 以 `fire_state: Arc<RwLock<…>>` 或事件流二选一（实现定）暴露消防站状态供 core-bin interlock 读取比对；不一致告警可做纯事件侧（两源各自成事件、事件消费者比对），不阻塞释放前置。

**释放状态机**（默认须人工，`auto_release=false`）：清 latch 需 ① 全部触发源 DI 已回安全态且保持 ≥ `release_hold_secs`（前置校验）② Web API `POST /api/v1/interlock/release` 手动确认；`auto_release: true` 时 ① 满足且 **停机已确认（非 `stop_failed`）** 即自动清（不推荐现场）。⚠️ `stop_failed`（停机确认超时，PCS 未真正停机）期间禁止自动解 latch——否则源复位即自动复位、联锁静默失效；仅人工 Web release（操作员明确放行）可容忍 stop_failed 态清 latch，且装配应对人工放行 stop_failed 补审计事件留痕。

**DO 驱动语义**：
- DO1 运行灯 = PCS `RUN_STATE(1013) ∈ {1,2,3}`（0 停 / 1 待机 / 2 充电 / 3 放电；非停机即上电）**且** 无联锁 latch（驱动数据源取 `last_run_state()`；`None` 链路未知 → 灯灭保守，首心跳前由 DO2=offline 语义覆盖，L-4；`mark_offline` 同步清 `last_run_state=None`，防离线期 DO1/DO2 同亮矛盾，B3）；
- DO2 故障灯 = 联锁 latch 触发 **或** PCS transport offline / M1 停机观测告警。

### 12.4 配置与持久化（core_config `io:` 段示例）

```yaml
io:
  poll_ms: 100            # DI 轮询周期
  auto_release: false     # false = 须 Web 人工解除；true = 触发源复位+保持时长后自动清
  release_hold_secs: 5    # 触发源回安全态需保持时长（人工解除前置校验）
  stop_confirm_ms: 5000   # 停机确认窗口（1013 须转 0）；超时 → stop_failed + interlock 周期重试 stop
  di:
    di1: { name: 急停,     gpio: 124, active_low: true,  debounce: 1, action: pcs_stop }  # NC 断线触发，近即时
    di2: { name: 水浸,     gpio: 125, active_low: false, debounce: 1, action: pcs_stop }
    di3: { name: 消防报警, gpio: 102, active_low: false, debounce: 1, action: pcs_stop }
    di4: { name: 门禁,     gpio: 103, active_low: false, debounce: 3, action: event }     # 仅事件
  do:
    do1: { name: 运行灯, gpio: 97,  active_high: true }
    do2: { name: 故障灯, gpio: 107, active_high: true }
```

持久化：联锁 latch 沿 `storage` 记录（`faults`/`events` 表，事件写库本轮接线；DB 行带 `cleared_at` 状态列）；**读回时机 = mupcd 启动、首个策略/下发行前**同步置入 transport（避免启动窗口内首个 send 抢先写 500=1）。Web API `GET /api/v1/interlock/status` + `POST /api/v1/interlock/release`（role 校验沿用占位 RBAC，08 模块）；**本地受控释放旁路**（mupcd CLI 子命令 + 本地日志审计）供现场无网/Web 不可达时人工恢复；RBAC 落地前 Web release 限内网访问并标注「占位 RBAC 仅开发期可用」为授权偏离。SSE 告警复用（startup SSE 推送）。

### 12.5 文件结构与测试

- **intercore**：`ModbusRtuTransport::stop` / `stopped_latched` / `ensure_started` 挡启动 / `is_interlock_stopped` / `clear_interlock_latch` / `restore_interlock_latched` / `last_run_state`（心跳维护 1013 最新值）；`stop()` 在 latch 期间**仍允许写 500=0**（仅挡 500=1 启动写，供 interlock 周期重试停机）；`pcs_slave.rs` 支持写 500=0 → RUN_STATE=0 停机仿真。
- **`mupc-io`**（新）：`DigitalIn`/`DigitalOut` trait + sysfs impl + gpiod 桩 + mock。
- **core-bin**：`interlock.rs`（采样/去抖/状态机/DO 驱动/事件）+ config `io:` 段解析与校验。
- **storage**：faults/events 运行时写入接线（联锁事件）。
- **web-api**：interlock status/release 两端点。
- 测试：GPIO mock 读写、联锁状态机纯函数（触发/去抖/释放/auto_release）、`stop()` 写 500=0（slave 仿真对打）、config `io:` 解析与非法值校验、latch 持久化读回、上层下发抑制。

### 12.6 验证状态

- S1/S2 设计与实施按 writing-plans 分批（S1 平台落地 → S2 联锁）；PCS 契约待确认清单（启停 500 时序/符号）仍待厂方，停机原语以 500=0 为当前契约。
- 交叉注：BMS 站（02 §10 role=battery）在线时其 SOC 优先于 `latest_soc`（intercore 回读）注入策略；见 **04 策略引擎 §2.11** SOC 源优先级补注。

### 12.7 架构审查修订（2026-09-08，dispatch_architect）`[DESIGN_APPROVED: 2026-09-08]`

**就地修订（已并入正文）**：

| 编号 | 内容 | 落点 |
|---|---|---|
| S-1 | `stop()` 成功须复位 `started=false`（否则 release 后 ensure_started 跳过启动 → 释放无法重启）；M1 停机观测加 `!stopped_latched` 自命令豁免 | §12.2 第 1 条 / §12.2 第 2 条尾 |
| S-2 | stop/is_interlock_stopped/last_run_state 入 `IntercoreTransport` trait（上层多态可调 + DO 数据源）；`tcp` 仿真 stop 降级 no-op | §12.2 第 2 条 |
| S-3 | PCS 侧独立硬急停回路兜底为投运前提（软停第一层）；stop 后 ≤T 确认 1013 转 0，未确认升级 stop_failed 态 | §12.3 联锁触发动作补 |
| S-4 | 链路恢复后首个写启动前先 1013 读校验（M1 守卫升级为 ensure_started 前置） | §12.2 第 5 条 |
| M-1 | GPIO 初始化/DI 读失败一律按触发（fail-safe latch + 告警），startup GPIO 自检 | §12.3 |
| M-2 | 去抖按 DI 细分（debounce_count 入每 DI），急停/消防 1 次采样即触发 | §12.3 |
| M-3 | 本地受控释放旁路（CLI + 审计）供无网恢复；Web release 占位 RBAC 标注授权偏离 + 限内网 | §12.4 |
| M-5 | 默认值变更同步 core_config 常量/单测；validate 启动探测串口 fail-fast | §12.1 |
| M-7 | DO1 运行灯 = 1013 ∈ {1,2,3}（非停机即上电）且无 latch（修正 =1） | §12.3 DO 语义 |

**跨模块采纳（落 02/04）**：M-4（south_stations 单写方 + 跨段「不与 modbus_rtu 共口」校验）、M-6（5s 新鲜度升共享常量）、M-8（fire 与 DI3 融合规则）、M-10（BMS 多包/多块扩展点）、M-11（口调度预算优先 meter_grid/battery）→ 02 §10；M-9（SOC 源切换滞回）→ 04 §2.11.1。**轻量项**：L-1 目录补 §11/§12（本节）、L-3 §11.7 示例串口改 ttyS0（下方实施修订）、L-2 deploy 章号与硬件行、L-4/L-5/L-6 → 02 §10 修订。

**放行标准**：S/M 级全部采纳，L 顺手；仍待厂方/联调确认项（PCS 启停 500 时序与符号、BMS 多包/私有点表、空调协议、消防融合语义）留实施/联调阶段闭环，不阻塞设计门禁。

**三轮修订（2026-09-08，design-reviewer 正式门禁 REJECTED → 修订）**：C-1 双 latch 模型（latch 介入不依赖 500=0 写成功、stop() 仅写与缓存复位、清/置唯一入口 clear/restore）→ §12.2；C-2 推包与新鲜度闸门（set_latest_data 闸门仅 meter_grid 推进）→ 02 §10.5/04 §2.11.1；I-1 M1 运维确认入口 + DO2 持久条件 → §12.2 bullet6；I-2 SOC 状态机测试清单 → 04 §2.11.1；B1/B2 → deploy §九 9.3；B3 → §12.3 DO；B4-B6 记为实施期项。

**二轮修订（2026-09-08，design-reviewer REJECTED → 修订）**：R-A（`clear_interlock_latch()`/`restore_interlock_latched(bool)` 清/恢复接口与调用时序，§12.2）；R-B（latch 期间整条下行中止、不写任何寄存器，§12.2）；R-E（去抖 `debounce` 移入每 DI、`stop_confirm_ms` 入 io schema，§12.4）；R-G（消防停机仅以 DI3 为准、RS485 作确认/校核 + 双源不一致告警，§12.3）；R-D（§11.7 示例串口 ttyS0）；R-I（停机确认循环归属 interlock + 挂 §11.11 待确认）；R-C（SOC 双源落点）→ 04 §2.11.1；R-F/R-H（02 §10 deploy 章号与 master_meter 排他）；R-J（BECG 485 方向控制确认项）→ 02 §10.8。



---

## 附录 A：性能指标参考

> 性能与可靠性指标详见 [PRD 第 7 章 非功能性需求](../specs/modules/10-MUPC-核间通信-PRD.md#7-非功能性需求)。

---

## 附录：版本演进

> 正文已整合全部历史补丁，本表仅作演进追溯。

| 版本 | 主要变更 |
|------|----------|
| v1.0 | 从 PRD v1.0、技术设计 v1.1 和代码库 intercore 实现合并整理 |
| v2.0 | 传输通道抽象（IntercoreTransport trait，IntercoreClient 作门面）新增 Modbus RTU 备选链路：Master + Slave 参考实现，控制备选数据面边界（遥测/SafetyOverride 仍走 TCP），含执行确认寄存器区，配置 transport 选择 tcp/modbus_rtu |
| v2.1 | TCP 回读 SOC（N3，U-26 延伸）：TcpTransport 加回读接收循环（独立连接读实时模块 DataUpload 帧 → battery_soc），`IntercoreTransport.latest_soc()` 查询，AiIntegrator 在总表模式（battery 无 SOC）时以核间 SOC 注入；Modbus 备选不承载（None） |
| v2.2 | PCS 真实协议 V1.3 取代 §11.4~11.6 假设点表：实时控制模块=两级式 PCS，`transport=modbus_rtu` 直连 PCS（RS485 19200 N-8-1，高 8/低 8 互换）；分相下行→PCS 模式2+单相 P/Q(±25 裁剪)，恒功率下行→模式0+1001/1002(k_droop 忽略)；SOC/心跳读 3 区 1010/1013 |
| v2.3 | BECG-3568 现场接线契约（S1）：PCS 主链路默认节点 /dev/ttyS1→/dev/ttyS0，站级 485 全口分配表（RS485-1..6 ↔ ttyS0/S2-S6 ↔ 设备），DI/DO 编号与接线落 deploy.md 现场接线章 |
| v2.4 | DI/DO 安全联锁（S2）：PCS 停机原语（500=0）+ stopped_latched 挡自动重启（双层：transport 兜底 + 上层抑制）；mupc-io GPIO 抽象（sysfs 先落地/gpiod 桩）；core-bin interlock 联锁控制器（急停/水浸/消防→pcs_stop，门禁仅事件；DB 持久化锁存 + Web release）；DO 运行/故障灯驱动；core_config io: 段 |
| v2.5 | S2 实施细化（评审闭环）：release 语义补 `stop_failed` 门控（§12.3 释放状态机：auto 释放须停机已确认，stop_failed 期间仅人工 Web release 放行 + 审计事件）；intercore `authorize_restart` 单次授权旁路 S-4 停机守卫（restore(true)/S-4 消费即清位）；interlock 状态机去抖归装配（§12.3 debounce 入每 DI 采样层） |
