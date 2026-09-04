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
| ADR-012 | Modbus 通道数据面边界 | ① 控制备选（控制下行+执行确认+心跳，遥测/SafetyOverride 仍走 TCP）；② 全量对等承载 | **控制备选（方案①）** | 本系统遥测主数据流来自南向采集，RS485 带宽有限不适合大块遥测轮询；SafetyOverride 为安全即时事件，Modbus 轮询无法保证及时性；边界明确后控制链路可经 Modbus 独立承载 |

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

### 11.2 可行性评估

| 维度 | 评估 | 结论 |
|---|---|---|
| 数据量 | 核间控制为秒级下发，单帧负载小（AI 双参数 2 个 f64；台区分相 6 个 f64） | ✅ 保持寄存器（16bit）足以承载 |
| 实时性 | Modbus 写/读周期 10~100ms，核间控制周期 1s 级 | ✅ 满足 |
| 带宽 | RS485 常用波特率 9600~115200，控制数据量小 | ✅ 充足 |
| 校验 | Modbus RTU 自带 CRC16 | ✅ 帧校验完备 |
| 基础设施 | rs485-plugin 已有 Modbus CRC16 / 寄存器读写实现可参考 | ✅ 无需从零写帧 |
| 栈选型 | tokio-modbus 提供 async master + server（slave） | ✅ 与 tokio 栈契合 |
| **主要风险** | ① 寄存器映射表为协议基线，须与**实时控制模块固件**确认（Slave 侧实现参考版供对齐）；② 串口物理层（RS485 接线/终端电阻/DE/RE 方向控制）需现场验证；③ 数据面边界（ADR-012）依赖 TCP 存在——无以太网现场遥测路径未覆盖 | ⚠️ 依赖外部对齐 |

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
  transport: "tcp"              # "tcp" | "modbus_rtu"（通道选择，默认 tcp = 现有行为）
  host: "192.168.1.2"           # TCP 参数（transport=tcp 用）
  port: 9100
  heartbeat_interval_sec: 5
  reconnect_interval_sec: 3
  modbus_rtu:                   # Modbus RTU 参数（transport=modbus_rtu 用）
    serial_port: "/dev/ttyS1"   # Linux 例；Windows 用 COM3
    baud_rate: 9600
    data_bits: 8
    stop_bits: 1
    parity: "none"
    slave_addr: 1               # 实时控制模块从站地址
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

1. **寄存器映射表须与实时控制模块固件对齐**（地址/缩放/`cmd_valid` 触发/`exec_status` 语义）。**关键契约（I-2）**：`cmd_valid` 的清除责任为**从站采纳后自清**（Master 不清、每次新指令由从站自清形成上升沿）——实时固件必须同样自清，否则自第 2 条指令起无上升沿、`issue()` 全部超时；Slave 参考实现已按此约定；
2. RS485 物理层：接线极性、终端电阻、DE/RE 方向控制（半双工）需现场核验；
3. Modbus RTU 点对点（1 Master : 1 Slave），不支持现有 TCP 的多连接场景（§10.2 待澄清问题 4 仅适用 TCP）；
4. **下发延迟预算**（对齐 PRD §7.1 ≤50ms）：一次下发 = 数据寄存器 FC16 + cmd_ctrl FC06 两帧，9600bps 下约 20~30ms，需以实测确认满足 50ms（波特率不足时提高，如 19200）；
5. **遥测/SafetyOverride 依赖 TCP**（ADR-012 边界）：`transport=modbus_rtu` 时须保证 TCP 链路仍承载遥测与安全事件（否则另行评估，不在本次范围）；
6. **配置热切偏离**（对齐 PRD §7.4）：`transport` 为部署配置二选一，需重启生效；Web UI 运行时切换不实现，记为此处对 PRD 可维护性需求的授权偏离；
7. **日志脱敏**（对齐 PRD §7.3）：Modbus 寄存器写值（控制数值）不入日志，仅记录指令类型/结果/`cmd_seq`。

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
