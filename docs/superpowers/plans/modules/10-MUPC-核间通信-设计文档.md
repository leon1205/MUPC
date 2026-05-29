# MUPC 核间通信模块设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-29 | 架构师 | 初稿 |

**来源文档：**
- `docs/superpowers/specs/modules/10-MUPC-核间通信-PRD.md` v1.0
- `docs/superpowers/plans/2026-05-27-MUPC-通信管理模块-技术设计.md` v1.1（提取 intercore 部分）
- `mupc/crates/intercore/src/` 代码库实际实现

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

核间通信（Intercore Communication）模块是 MUPC "异构双核心模块主控架构"中**非实时处理核心（大脑）**与**实时控制核心模块（小脑）**之间的通信桥梁。

```
通信管理模块 (大脑) ──── TCP Socket (RJ45) ──── 实时控制模块 (小脑)
    - gateway                         - 硬实时控制
    - strategy-engine                  - 10kHz 电流环
    - ai-engine                        - 功率变换
    - intercore  ←── 本模块 ──→        - 保护逻辑
```

**核心职责：**
- 建立并维护通信管理模块与实时控制模块之间的 TCP Socket 长连接
- 将经过策略校验的指令下发至实时控制模块执行
- 从实时控制模块读取实时电气量、电池数据、设备状态
- 通过心跳和看门狗机制监控实时控制模块的运行健康状态
- 在通信异常时触发告警和可选的复位机制

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

- **通信管理模块：** TCP 服务端，监听连接
- **实时控制模块：** TCP 客户端，主动发起连接
- **物理层：** RJ45 以太网直连

### 2.2 端口规划

| 角色 | 方向 | 默认端口 | 说明 |
|------|------|---------|------|
| 通信管理模块（服务端） | 监听 | **2500** | 等待实时控制模块连接 |
| 实时控制模块（客户端） | 连接对端 | **2501** | 主动连接通信管理模块 |

> **说明：** 通信管理模块监听端口 2500，接受实时控制模块的连接。实时控制模块作为客户端不需要暴露端口，2501 仅作为对端标识参考。

### 2.3 连接生命周期

```
[建立]
  实时控制模块 ──TCP 连接──→ 通信管理模块
       │                           │
       └────── 发送 Connect 帧 ────→   ← 连接建立完成
       
[保持]
  实时控制模块 ←── HeartbeatReq ──→ 通信管理模块 (1s 周期)
       │                           │
       └── StatusReport / DataUpload → 
       
[断开]
  实时控制模块  ──TCP 断开──→ 通信管理模块
       │                           │
       │                            → 标记连接断开
       │                            → 等待重连
       
[重连]
  实时控制模块  ──TCP 重连──→ 通信管理模块 (自动重连)
```

**连接管理规则：**
1. 通信管理模块作为 TCP 服务端，被动接受连接
2. 连接建立后，通信管理模块自动发送 Connect 帧（`0x0001`）完成注册
3. 连接丢失后，实时控制模块负责自动重连（通信管理模块被动等待）
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
| CRC16 | 62 | 2 字节 | 大端 | MODBUS CRC16 校验 |

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
| `0x0030` | DataUpload | 实时控制模块 → 通信管理模块 | 数据上送（周期遥测） |

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

Payload 采用 JSON 编码：

```json
{
    "cmd_type": "P_batt_set",
    "value": 100.0,
    "unit": "kW",
    "timestamp": 1712345678,
    "strategy_mode": "Smart",
    "seq_no": 42
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| cmd_type | string | 指令类型标识 |
| value | float | 指令数值 |
| unit | string | 单位 |
| timestamp | i64 | UTC 时标（Unix 时间戳） |
| strategy_mode | string | 当前策略模式上下文 |
| seq_no | u16 | 与帧头 SeqNo 一致的序列号 |

**支持指令类型：**

| 指令 | cmd_type | 说明 | 数据范围 |
|------|---------|------|---------|
| 电池有功设定 | P_batt_set | 电池有功功率设定值 | -1000kW ~ 1000kW |
| 电池无功设定 | Q_batt_set | 电池无功功率设定值 | -1000kVar ~ 1000kVar |
| 分相补偿系数 | Phase_comp | 三相分别补偿系数 | 0.0 ~ 1.0（每相） |
| 启停命令 | Start_Stop | 电池/逆变器启停控制 | 启动 / 停止 |
| 有功设定值 | P_set | 调度主站下发的有功设定 | -1000kW ~ 1000kW |
| 无功设定值 | Q_set | 调度主站下发的无功设定 | -1000kVar ~ 1000kVar |
| 一次调频参数 | Freq_reg | K 值、死区设置 | 按参数定义 |
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

与 StatusReport 结构类似的周期性遥测帧，Payload 格式同为 JSON，具体字段可按需扩展。备用帧类型用于区分定期状态报告与事件触发的数据上送。

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
- 输出：小端字节序（CRC 低字节在前）

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

**验收标准：**
- 数据由实时控制模块主动通过 StatusReport 或 DataUpload 帧上送
- 读取周期：≥ **1Hz**（默认每秒一次）
- 所有数据必须携带 **UTC 时标**
- 数据到达后通知 data-processing 模块进行汇聚处理

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
    pub listen_addr: String,             // 监听地址，默认 "0.0.0.0"
    pub listen_port: u16,                // 监听端口，默认 2500
    pub heartbeat_interval_ms: u64,      // 心跳间隔，默认 1000ms
    pub watchdog_timeout_ms: u64,        // 看门狗超时，默认 10000ms
}

impl Default for IntercoreConfig {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0".to_string(),
            listen_port: 2500,
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
listen_addr = "0.0.0.0"
listen_port = 2500
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
| ADR-004 | 网络拓扑：主控为服务端 vs 主控为客户端 | ① 主控服务端，实时控制模块客户端主动连入；② 主控客户端，主动连接实时控制模块 | **主控服务端** | 实时控制模块需要在上电后立即连入主控，由实时控制模块发起连接更符合启动时序；主控侧简化重连逻辑 |
| ADR-005 | 字节序选择 | ① 帧头字段统一大端（network byte order）；② 混合字节序 | **帧头大步端，Payload 中 f64 小端** | 帧头使用大端符合网络字节序惯例；f64 使用小端与 ARM 架构（RK3588）原生字节序一致，避免不必要的字节序转换 |
| ADR-006 | 序列号匹配策略 | ① u16 循环递增；② u32 递增；③ UUID | **u16 循环递增** | 与帧头长度匹配（2 字节），足够用于请求-应答匹配（65535 个并发指令远超出实际需求） |
| ADR-007 | 心跳与看门狗分离设计 | ① 心跳管理器 + 看门狗两个独立组件；② 合并为一个组件 | **分离设计** | 心跳管理器负责连接状态跟踪和健康检测循环；看门狗独立负责超时判定和告警/复位逻辑；职责分离，便于单组件测试和替换 |
| ADR-008 | CRC 算法选择 | ① MODBUS CRC16；② CRC32；③ Adler-32 | **MODBUS CRC16** | 2 字节校验满足帧传输错误检测需求；CRC16 计算开销低（每帧计算量小）；工业协议广泛采用 |

### 10.2 与实际代码实现的差异说明

| 项目 | 技术设计 v1.1 描述 | 实际代码实现 | 结论 |
|------|-------------------|-------------|------|
| 帧头 Magic | 无 Magic 字段 | 有 Magic 字段（0xAA55） | 以代码为准 |
| 帧头字段 | Type + Length + SeqNo + ai_ready + strategy_mode + Reserved | Magic + Length + Type + SeqNo + Payload + CRC16 | 以代码为准 |
| SeqNo 类型 | u32（4 字节） | u16（2 字节） | 以代码为准 |
| 关键信号嵌入方式 | 帧头固定偏移（ai_ready offset 8, strategy_mode offset 9） | 通过 ControlCmd 帧的 JSON Payload 传递 | 以代码为准 |
| CRC 放置位置 | 在帧头计算中位置未明确 | 帧尾 2 字节 | 以代码为准 |
| 连接处理 | 未详细说明 | 连接建立后主动发送 Connect 帧 | 以代码为准 |
| 心跳 Payload | 未详述 | status(1B) + cpu_temp(8B, f64) + memory_usage(8B, f64) | 以代码为准 |

### 10.3 待澄清问题

| 序号 | 问题 | 优先级 | 状态 |
|------|------|--------|------|
| 1 | 看门狗超时触发实时控制模块复位的策略是否启用（PRD 标记为"可选"） | 低 | 待确认 |
| 2 | StatusReport 和 DataUpload 的上送周期是否相同，是否需要差异化配置 | 低 | 待确认 |
| 3 | 是否需要实现指令队列/缓存机制（连接断开时暂存指令） | 中 | 待确认 |
| 4 | 是否需要支持多个实时控制模块同时连接（当前实现已支持 HashMap 存储多连接） | 中 | 待确认 |

---

## 附录 A：性能指标参考

| 指标 | 要求 | 说明 |
|------|------|------|
| 指令下发延迟 | ≤ 50ms | 从收到下发请求到帧发送完成 |
| 数据读取周期 | ≥ 1Hz | 实时控制模块每秒至少上送一次数据 |
| 心跳周期 | 1 秒 | 每秒发送一次心跳请求 |
| 指令超时 | 5 秒 | ControlCmd 等待应答超时 |
| 连接建立时间 | ≤ 1 秒 | TCP 握手 + 连接帧注册完成 |
| 看门狗超时 | 10 秒 | 连续无心跳响应超过 10 秒触发 |
| 帧定长 | 64 字节 | 含帧头 8 + 有效数据 54 + CRC 2 |
| 系统 MTBF | ≥ 50,000 小时 | 整机可靠性指标 |

## 附录 B：修改记录

| 版本 | 日期 | 修改说明 |
|------|------|---------|
| v1.0 | 2026-05-29 | 从 PRD v1.0、技术设计 v1.1 和代码库 intercore 实现合并整理 |

---

**文档结束**
