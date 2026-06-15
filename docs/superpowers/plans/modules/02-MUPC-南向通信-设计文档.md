# MUPC 南向通信模块 设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-29 | 架构师 | 定稿 |

> **文档定位：** 本文档记录实现级设计决策。需求级内容（功能描述、验收标准、性能指标）请参考 [02-MUPC-南向通信-PRD](../specs/modules/02-MUPC-南向通信-PRD.md)。

## 目录

1. [模块架构](#1-模块架构)
2. [RS485 设备设计](#2-rs485-设备设计)
3. [协议处理器设计](#3-协议处理器设计)
4. [HPLC 驱动设计](#4-hplc-驱动设计)
5. [动态插件系统设计](#5-动态插件系统设计)
6. [接口定义](#6-接口定义)
7. [文件结构](#7-文件结构)
8. [配置格式](#8-配置格式)
9. [技术决策记录](#9-技术决策记录)

---

## 1. 模块架构

### 1.1 架构概览

南向通信模块采用**分层+插件化**架构，自底向上分为四层：

```
┌──────────────────────────────────────────────────────────────────┐
│                         上层使用者                                 │
│            strategy-engine / data-processing / gateway            │
└──────────────────────────────┬───────────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────┐
│                   统一设备抽象层 (device-trait)                     │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  SouthDevice trait  │  DeviceRegistry trait  │  MessageBus │  │
│  │  ProtocolHandler    │  HplcDriver trait     │  Plugin      │  │
│  │  DataFrame / DeviceError / DeviceStatus     │  PluginLoader│  │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────┬───────────────────────────────────┘
                               │
          ┌────────────────────┼────────────────────┐
          ▼                    ▼                    ▼
┌───────────────────┐ ┌──────────────────┐ ┌───────────────────┐
│   rs485-plugin    │ │   hplc-plugin    │ │   其他插件         │
│  ┌─────────────┐  │ │ ┌──────────────┐ │ │  (未来扩展)       │
│  │ Rs485Device │  │ │ │ HplcDevice   │ │ │                   │
│  │ Modbus      │  │ │ │ MockDriver   │ │ │                   │
│  │ TTU         │  │ │ │ SdkDriver(预留)│ │                   │
│  │ Inverter    │  │ │ └──────────────┘ │ │                   │
│  │ Charger     │  │ └──────────────────┘ │                   │
│  └─────────────┘  │                      │                   │
└───────────────────┘                      └───────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────┐
│                     物理层 (Physical Layer)                       │
│     RS485 总线 (DE/RE GPIO)   /   HPLC 电力线载波 (FFI)          │
└──────────────────────────────────────────────────────────────────┘
```

### 1.2 核心概念

| 概念 | 说明 |
|------|------|
| **SouthDevice** | 所有南向设备的统一接口 abstraction |
| **ProtocolHandler** | RS485 协议处理器，通过依赖注入支持多种协议 |
| **HplcDriver** | HPLC 芯片驱动抽象，支持 Mock 和 SDK 接入 |
| **Plugin** | 动态插件接口，所有南向插件必须实现 |
| **PluginLoader** | 动态插件加载器，管理插件生命周期 |
| **DeviceRegistry** | 设备注册表，管理设备注册/注销/查询 |
| **MessageBus** | 消息总线，设备数据发布/订阅 |

### 1.3 数据流

```
策略引擎 → SouthDevice::write() → Rs485Device/HplcDevice → 物理层
物理层 → SouthDevice::read() → DataFrame → MessageBus → 策略引擎/数据处理
```

### 1.4 设备类型支持

| 设备类型 | 通信方式 | 协议 | 处理器 | 状态 |
|----------|----------|------|--------|------|
| **TTU**（配变终端） | RS485 | 电力行业规约 | `TtuHandler` | 已实现 |
| **光伏逆变器** | RS485 | 厂商私有协议 | `InverterHandler` | 已实现 |
| **充电桩** | RS485 | GB/T 27930 | `ChargerHandler` | 已实现 |
| **柔性负荷控制装置** | RS485 | Modbus RTU | `ModbusHandler` | 已实现 |
| **消防控制系统** | RS485 | Modbus RTU | `ModbusHandler` | 已实现 |
| **HPLC 设备** | 电力线载波 | 芯片 SDK | `MockHplcDriver`(开发) / `SdkHplcDriver`(预留) | 已实现(Mock) |

### 1.5 依赖关系

```
device-trait (无外部依赖)
    ↓
plugin-loader → device-trait
    ↓
rs485-plugin → device-trait (编译为 cdylib 供动态加载)
hplc-plugin  → device-trait (编译为 cdylib 供动态加载)
```

---

## 2. RS485 设备设计

### 2.1 总体设计

RS485 南向通信采用**统一设备抽象 + 协议处理器注入**模式。`Rs485Device` 结构体实现 `SouthDevice` trait，通过依赖注入 `ProtocolHandler` 支持多种设备协议。

```
rs485-plugin/
├── lib.rs                      # 插件入口
├── device.rs                   # Rs485Device 实现（串口操作、DE/RE、事务）
├── config.rs                   # 配置定义 + 验证
├── errors.rs                   # RS485 错误类型
├── protocol.rs                 # 帧解析 + CRC 校验 + 数据单元解析
└── handlers/
    ├── mod.rs                  # 协议处理器注册表 + 本地 CRC
    ├── modbus_handler.rs       # Modbus RTU
    ├── ttu_handler.rs          # TTU 专用协议
    ├── inverter_handler.rs     # 光伏逆变器私有协议
    └── charger_handler.rs      # GB/T 27930 充电桩协议
```

### 2.2 Rs485Device 结构体

```rust
/// RS485 设备驱动
pub struct Rs485Device {
    /// 设备唯一标识
    device_id: String,
    /// 设备类型
    device_type: String,
    /// 配置
    config: Config,
    /// 串口文件描述符
    port_fd: Mutex<Option<RawFd>>,
    /// 设备状态
    status: Mutex<DeviceStatus>,
    /// 是否已打开
    opened: AtomicBool,
    /// 发送锁（保证事务原子性）
    tx_lock: StdMutex<()>,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new(device_id, device_type, config)` | 创建 RS485 设备实例 |
| `open()` | 打开串口并配置参数（Unix: libc termios） |
| `close()` | 关闭串口 |
| `send_frame(frame)` | 发送原始数据帧 |
| `recv_frame(timeout_ms)` | 接收原始数据帧 |
| `transaction(request, timeout)` | 原子读-写-读事务 |
| `send_recv(frame, timeout)` | 发送并接收 |
| `read_holding_registers(addr, count)` | Modbus 功能码 0x03 |
| `write_single_register(addr, value)` | Modbus 功能码 0x06 |

### 2.3 串口配置

使用 Unix `libc` termios 直接操作串口，避免第三方库依赖：

```rust
fn configure_port(&self, fd: RawFd) -> Result<(), Rs485Error> {
    // 1. 获取当前终端属性 (tcgetattr)
    // 2. 设置波特率 (cfsetispeed / cfsetospeed)
    // 3. 设置数据位 (CS5/CS6/CS7/CS8)
    // 4. 设置校验位 (PARENB / PARODD)
    // 5. 设置停止位 (CSTOPB)
    // 6. 启用 CLOCAL | CREAD
    // 7. 设置超时 (VTIME / VMIN)
    // 8. 应用设置 (tcsetattr TCSANOW)
    // 9. 刷新缓冲区 (tcflush)
}
```

### 2.4 DE/RE GPIO 控制

RS485 为半双工通信，需要通过 GPIO 控制发送使能（DE）和接收使能（RE）。

```rust
pub enum Rs485Dir {
    Recv,  // 接收模式
    Send,  // 发送模式
}

impl Rs485Device {
    fn set_dir(&self, dir: Rs485Dir) -> Result<(), Rs485Error> {
        let gpio_num = match dir {
            Rs485Dir::Send => self.config.de_gpio,
            Rs485Dir::Recv => self.config.re_gpio,
        };
        if let Some(gpio) = gpio_num {
            gpio_set_value(gpio, dir == Rs485Dir::Send)?;
        }
        Ok(())
    }
}
```

`gpio_set_value` 实现（跨平台）：

| 平台 | 实现方式 |
|------|----------|
| Linux | sysfs GPIO: `/sys/class/gpio/gpio{num}/value` |
| Windows | 模拟实现（实际需要 platform-specific 驱动） |
| 其他 | debug 日志模拟 |

**验证规则：** DE 和 RE 引脚不能相同。

### 2.5 事务原子操作

```rust
pub fn transaction(&self, request: &[u8], recv_timeout_ms: u64) -> Result<DataFrame, Rs485Error> {
    let _guard = self.tx_lock.lock();  // 全局锁保证原子性

    // 1. 切换到发送模式
    self.set_dir(Rs485Dir::Send)?;

    // 2. 发送请求
    self.send_frame(request)?;

    // 3. 切换到接收模式
    self.set_dir(Rs485Dir::Recv)?;

    // 4. 接收响应
    let data = self.recv_frame(recv_timeout_ms)?;
    Ok(DataFrame::new(self.device_id.clone(), data))
}
```

**设计要点：**
- 使用 `StdMutex<()>` 作为全局锁，跨异步任务保证设备独占访问
- 发送前切换到发送模式，发送后立刻切换回接收模式
- 超时由 termios `VTIME` 控制，避免阻塞

### 2.6 RS485 通信参数

| 设备类型 | 波特率 | 数据位 | 停止位 | 校验 | 典型轮询周期 |
|----------|--------|--------|--------|------|-------------|
| TTU | 9600 | 8 | 1 | 偶校验 | 1s |
| 光伏逆变器 | 9600 / 19200 | 8 | 1 | 无 | 5s |
| 充电桩 | 19200 | 8 | 1 | 偶校验 | 10s |
| 柔性负荷 | 9600 | 8 | 1 | 无 | 1s |
| 消防控制 | 9600 | 8 | 1 | 无 | 1s |

### 2.7 RS485 错误类型

```rust
#[derive(Debug, Error)]
pub enum Rs485Error {
    OpenFailed(String),    // 串口打开失败
    ConfigFailed(String),  // 串口配置失败
    SendFailed(String),    // 数据发送失败
    RecvFailed(String),    // 数据接收失败
    Timeout,               // 串口读写超时
    CrcFailed(String),     // CRC 校验失败
    NotConnected(String),  // 设备未连接
    GpioError(String),     // GPIO 控制错误
    IoError(#[from] std::io::Error),  // IO 错误
}
```

---

### 2.7 南向控制指令分发（SouthCommandSender）

> **来源**：策略引擎模块通过 `SouthCommandSender` trait 向南向设备分发控制指令

**设计目标：**

策略引擎输出的两类南向控制指令通过 `SouthCommandSender` trait 发送到对应设备，与核间通信的 `p_ref`/`k_droop` 双参数指令分离：

```
┌──────────────────────────────────────────────────────────────┐
│                    策略引擎 (strategy-engine)                  │
├──────────────────────────────────────────────────────────────┤
│  p_ref + k_droop  →  IntercoreClient  →  实时控制模块        │  ← 核间通信
│  pv_limit         →  SouthCommandSender  →  光伏逆变器      │  ← 南向通信
│  load_shedding    →  SouthCommandSender  →  负荷控制装置    │  ← 南向通信
└──────────────────────────────────────────────────────────────┘
```

**Trait 定义（定义于 `strategy-engine/src/south_command_sender.rs`）：**

```rust
#[async_trait]
pub trait SouthCommandSender: Send + Sync {
    async fn send_pv_limit(&self, cmd: PvLimitCommand) -> SouthSendResult;
    async fn send_load_shedding(&self, cmd: LoadSheddingCommand) -> SouthSendResult;
}

pub struct PvLimitCommand {
    pub device_id: String,
    pub limit_ratio: f64,      // [0.0, 1.0]
    pub priority: u8,
}

pub struct LoadSheddingCommand {
    pub device_id: String,
    pub power_kw: f64,
    pub priority: u8,
}
```

**实现类：**

| 实现 | 文件 | 说明 |
|------|------|------|
| `MockSouthCommandSender` | `south_command_sender.rs` | 开发/测试用模拟实现 |
| `Rs485SouthCommandSender` | Phase 2+ 实现 | 真实 RS485 通信 |
| `HplcSouthCommandSender` | Phase 2+ 实现 | 真实 HPLC 通信 |

**与核间通信的分工：**

| 指令 | 发送路径 | 目标 |
|------|----------|------|
| `p_ref` (有功基准点) | 核间通信 → 实时控制模块 | 下垂闭环控制 |
| `k_droop` (下垂系数) | 核间通信 → 实时控制模块 | 下垂闭环控制 |
| `pv_limit` (限功率) | 南向通信 → 光伏逆变器 | 防逆流/功率限制 |
| `load_shedding` (切负荷) | 南向通信 → 负荷控制装置 | 需量控制 |

---

## 3. 协议处理器设计

### 3.1 设计模式

采用**策略模式**：`ProtocolHandler` trait 定义编码/解码接口，由具体的处理器实现不同协议。

```
Rs485Device (上下文)
    │
    ├── handler: Arc<dyn ProtocolHandler>  (注入的策略)
    │
    └── transaction() 时调用:
        handler.encode_request(device_id, data) → 编码请求
        handler.decode_response(frame)         → 解码响应
```

### 3.2 ProtocolHandler Trait

```rust
pub trait ProtocolHandler: Send + Sync {
    /// 编码请求数据
    fn encode_request(&self, device_id: &str, data: &[u8]) -> Vec<u8>;

    /// 解码响应数据
    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError>;

    /// 获取协议名称
    fn name(&self) -> &'static str;
}
```

### 3.3 协议处理器注册表

```rust
pub struct ProtocolHandlerRegistry;

impl ProtocolHandlerRegistry {
    pub fn get(name: &str, config: &Config) -> Option<Arc<dyn ProtocolHandler>> {
        match name {
            "modbus"   => Some(Arc::new(ModbusHandler::new(config.device_addr, config.crc_mode))),
            "ttu"      => Some(Arc::new(TtuHandler::new(config.device_addr))),
            "inverter" => Some(Arc::new(InverterHandler::new(config.device_addr))),
            "charger"  => Some(Arc::new(ChargerHandler::new(config.device_addr))),
            _ => None,
        }
    }
}
```

### 3.4 各处理器详情

#### 3.4.1 ModbusHandler

| 属性 | 值 |
|------|-----|
| 协议 | Modbus RTU |
| 帧格式 | `[设备地址][功能码][数据][CRC16高][CRC16低]` |
| 最小帧长 | 5 字节 |
| CRC 验证 | 严格校验，地址+数据不匹配则拒绝 |
| 支持功能码 | 0x01(读线圈)、0x03(读保持寄存器)、0x04(读输入寄存器)、0x05(写线圈)、0x06(写寄存器)、0x10(写多寄存器) |

```rust
impl ProtocolHandler for ModbusHandler {
    fn encode_request(&self, _device_id: &str, data: &[u8]) -> Vec<u8> {
        let mut frame = vec![self.device_addr];
        frame.extend_from_slice(data);
        let crc = crc16_modbus(&frame);
        frame.push((crc >> 8) as u8);
        frame.push(crc as u8);
        frame
    }

    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError> {
        // 验证最小长度、设备地址、CRC
        if frame.len() < 5 { return Err(...); }
        if frame[0] != self.device_addr { return Err(...); }
        // CRC 校验
        // ...
        Ok(DataFrame::new(format!("modbus_{}", self.device_addr), frame.to_vec()))
    }
}
```

#### 3.4.2 TtuHandler

| 属性 | 值 |
|------|-----|
| 协议 | 电力行业规约（类 101 规约简化版） |
| 帧格式 | `[0x68][版本][数据长度][数据载荷][校验和][0x16]` |
| 校验方式 | 累加和校验 |
| 数据长度 | 单字节，最大 255 |

```rust
impl ProtocolHandler for TtuHandler {
    fn encode_request(&self, _device_id: &str, data: &[u8]) -> Vec<u8> {
        let mut frame = vec![0x68];  // 起始符
        frame.push(self.protocol_version);  // 版本
        frame.push(data.len() as u8);  // 数据长度
        frame.extend_from_slice(data);  // 数据载荷
        let checksum: u8 = frame[1..].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        frame.push(checksum);  // 校验和
        frame.push(0x16);  // 结束符
        frame
    }
}
```

#### 3.4.3 InverterHandler

| 属性 | 值 |
|------|-----|
| 协议 | 光伏逆变器厂商私有协议 |
| 帧格式 | `[0x01][数据长度高][数据长度低][数据载荷]` |
| 数据长度 | 2 字节大端序 |

#### 3.4.4 ChargerHandler

| 属性 | 值 |
|------|-----|
| 协议 | GB/T 27930（电动汽车充电通信） |
| 帧格式 | `[0xFF][0xFE][协议版本][数据载荷][校验和]` |
| 校验方式 | 累加和（从协议版本开始） |

#### 3.4.5 消防控制（预留扩展）

消防控制系统使用 Modbus RTU 协议，通过 `ModbusHandler` 实现。如需求定制协议，可新增 `FireAlarmHandler`。

### 3.5 协议解析层

`protocol.rs` 提供底层帧解析和 CRC 计算：

```rust
pub struct Frame {
    pub addr: u8,        // 设备地址
    pub func_code: u8,   // 功能码
    pub data: Vec<u8>,   // 数据载荷
    pub crc: u16,        // CRC 校验码
}
```

**支持的 CRC 模式：**
- `Crc16Modbus` — Modbus CRC16 (x^16 + x^15 + x^2 + 1)
- `Crc16Xmodem` — XMODEM CRC16 (x^16 + x^12 + x^5 + 1)
- `None` — 无校验

**DataUnitParser** 提供数据单元解析工具：

| 方法 | 说明 |
|------|------|
| `parse_i16(data)` | 解析 16 位有符号整数 |
| `parse_u16(data)` | 解析 16 位无符号整数 |
| `parse_f32(data)` | 解析 32 位浮点数 |
| `pack_u16(value)` | 打包 16 位无符号整数 |
| `pack_i16(value)` | 打包 16 位有符号整数 |
| `pack_f32(value)` | 打包 32 位浮点数 |

---

## 4. HPLC 驱动设计

### 4.1 总体设计

HPLC（高速电力线载波）模块采用**通用驱动抽象 + 芯片 SDK 后续集成**策略。

- **Phase 2**：实现 Mock 驱动用于开发和验证数据通路
- **Phase 3**：芯片 SDK 绑定（预留接口）

```
hplc-plugin/
├── lib.rs              # 插件入口 + FFI 导出
├── driver.rs           # HplcDriver trait
├── device.rs           # HplcDevice（实现 SouthDevice）
├── mock.rs             # MockHplcDriver（开发/测试用）
├── errors.rs           # HplcError
└── config.rs           # 配置定义
```

### 4.2 HplcDriver Trait

```rust
pub trait HplcDriver: Send + Sync {
    /// 转换为 Any，用于 downcasting（获取实际类型引用）
    fn as_any(&self) -> &dyn Any;

    /// 初始化驱动
    fn init(&self, config: HplcConfig) -> Result<(), HplcError>;

    /// 发送数据
    fn send(&self, data: &[u8]) -> Result<(), HplcError>;

    /// 接收数据（阻塞，超时返回空）
    fn recv(&self, timeout_ms: u64) -> Result<Vec<u8>, HplcError>;

    /// 检查连接状态
    fn is_connected(&self) -> bool;

    /// 获取驱动名称
    fn driver_name(&self) -> &'static str;
}
```

### 4.3 HplcConfig

```rust
pub struct HplcConfig {
    pub port: String,              // 串口路径（Linux=/dev/ttyUSB0, Windows=COM3）
    pub baud_rate: u32,            // 波特率
    pub chip_type: Option<String>, // 芯片型号（FFI 预留）
    pub channel: Option<u8>,       // 通道号
}
```

**JSON 别名支持：** `serial_port`、`com_port` 均可作为 `port` 的别名。

### 4.4 HplcDevice

```rust
pub struct HplcDevice {
    device_id: String,
    device_type: String,
    config: HplcConfig,
    driver: Arc<dyn HplcDriver>,
    status: Mutex<DeviceStatus>,
}
```

`HplcDevice` 实现 `SouthDevice` trait：
- `connect()` 调用 `driver.init()`
- `read()` 调用 `driver.recv()`
- `write()` 调用 `driver.send()`
- `health_check()` 调用 `driver.is_connected()`

### 4.5 MockHplcDriver

```rust
pub struct MockHplcDriver {
    connected: AtomicBool,
    mock_queue: Mutex<Vec<Vec<u8>>>,  // 模拟数据队列
    mock_delay_ms: AtomicU64,         // 模拟延迟
}
```

**能力：**
- `inject_data(data)` — 注入模拟数据到接收队列
- `set_mock_delay_ms(ms)` — 设置模拟延迟
- 支持多次连续注入和接收（FIFO 队列）

**用途：** 开发和测试阶段使用，不依赖实际硬件。

### 4.6 SdkHplcDriver（预留，Phase 3）

```rust
pub struct SdkHplcDriver {
    handle: *mut c_void,  // FFI 句柄
}

impl HplcDriver for SdkHplcDriver {
    fn init(&self, config: HplcConfig) -> Result<(), HplcError> {
        // 调用 libhplc.so 中的 hplc_init()
        unsafe { hplc_init(config.port.as_ptr(), config.baud_rate) }
    }
    fn send(&self, data: &[u8]) -> Result<(), HplcError> {
        unsafe { hplc_send(self.handle, data.as_ptr(), data.len() as u32) }
    }
    fn recv(&self, timeout_ms: u64) -> Result<Vec<u8>, HplcError> {
        unsafe { hplc_recv(self.handle, buf.as_mut_ptr(), buf.len() as u32, timeout_ms as i32) }
    }
}
```

### 4.7 HPLC 技术参数（预留）

| 参数 | 规格 |
|------|------|
| 调制方式 | OFDM（BPSK/QPSK/16QAM/64QAM 自适应） |
| 通信频段 | 0.7 MHz - 3 MHz |
| 物理层速率 | 2 Mbps - 10 Mbps（自适应） |
| 最大帧长 | 1500 字节 |
| 典型应用 | 台区全覆盖，替代 RS485 布线困难区域 |

### 4.8 HPLC 错误类型

```rust
#[derive(Debug, Error)]
pub enum HplcError {
    InitFailed(String),    // 驱动初始化失败
    SendFailed(String),    // 发送失败
    RecvFailed(String),    // 接收失败
    Disconnected(String),  // 连接断开
    SdkError(String),      // SDK 错误
}
```

---

## 5. 动态插件系统设计

### 5.1 架构

动态插件系统通过 FFI 绑定实现运行时加载/卸载 `.so` / `.dll` / `.dylib` 动态库。

```
PluginLoader (plugin-loader crate)
    ↓
libloading (动态加载 .so/.dll)
    ↓
FFI 导出函数: create_plugin() + plugin_meta()
    ↓
Plugin trait 实例 (dyn Plugin trait object)
```

### 5.2 Plugin Trait

```rust
pub trait Plugin: Send + Sync {
    fn meta(&self) -> PluginMeta;
    fn init(&self, config: serde_json::Value) -> Result<(), PluginError>;
    fn start(&self) -> Result<(), PluginError>;
    fn stop(&self) -> Result<(), PluginError>;
    fn shutdown(self: Box<Self>) -> Result<(), PluginError>;
}
```

### 5.3 PluginMeta

```rust
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}
```

### 5.4 插件生命周期

```
Load ──→ Init ──→ Start ──→ Stop ──→ Unload
```

| 阶段 | 操作 | 状态 |
|------|------|------|
| **Load** | 使用 `libloading` 加载 `.so/.dll`，调用 `create_plugin()` 获取实例，调用 `plugin_meta()` 获取元信息 | `Loaded` |
| **Init** | 调用 `plugin.init(config)` 传入 JSON 配置 | `Initialized` |
| **Start** | 调用 `plugin.start()` 启动业务逻辑 | `Running` |
| **Stop** | 调用 `plugin.stop()` 停止业务逻辑 | `Stopped` |
| **Unload** | 调用 `plugin.shutdown()`，从注册表移除，卸载动态库 | `Unloaded` |

### 5.5 必需 FFI 导出符号

每个动态插件必须导出以下两个 `extern "C"` 函数：

```rust
#[no_mangle]
pub unsafe extern "C" fn create_plugin() -> *mut dyn Plugin {
    Box::into_raw(Box::new(MyPlugin::new())) as *mut dyn Plugin
}

#[no_mangle]
pub unsafe extern "C" fn plugin_meta() -> PluginMeta {
    MyPlugin::new().meta()
}
```

### 5.6 PluginLoader Trait

```rust
pub trait PluginLoader: Send + Sync {
    fn load(&self, plugin_path: &str, config: Value) -> Result<(), PluginError>;
    fn unload(&self, plugin_name: &str) -> Result<(), PluginError>;
    fn list(&self) -> Vec<PluginMeta>;
    fn get(&self, plugin_name: &str) -> Option<Arc<dyn Plugin>>;
    fn is_loaded(&self, plugin_name: &str) -> bool;
    fn plugin_count(&self) -> usize;
    fn unload_all(&self) -> Result<(), PluginError>;
}
```

### 5.7 PluginLoaderImpl

核心实现（使用 `libloading`）：

```rust
pub struct PluginLoaderImpl {
    plugins: RwLock<HashMap<String, PluginHandle>>,
    search_paths: RwLock<Vec<String>>,
}
```

**加载流程：**
1. 检查插件是否已加载（防止重复加载）
2. 使用 `libloading::Library::new()` 加载动态库
3. 通过 `library.get(b"create_plugin")` 获取工厂函数
4. 通过 `library.get(b"plugin_meta")` 获取元信息
5. 调用 `create_fn()` 创建插件实例
6. 存储到 `HashMap<String, PluginHandle>`

**卸载流程：**
1. 从 `HashMap` 中移除 `PluginHandle`
2. `PluginHandle` 被 `drop`，自动释放 `Library`（卸载 `.so`）

### 5.8 插件注册表 (PluginRegistry)

管理插件的元信息和生命周期状态，与 PluginLoader 配合使用：

```rust
pub struct PluginRegistry {
    entries: RwLock<HashMap<String, PluginEntry>>,
}
```

**能力：**
- `register` / `unregister` — 注册/注销插件
- `get` / `names` — 查询插件
- `query_by_state(state)` — 按状态查询
- `update_state` — 更新插件状态

### 5.9 编译要求

```toml
[lib]
crate-type = ["cdylib"]  # 必须编译为动态库
```

| 平台 | 输出 |
|------|------|
| Linux | `target/release/libmy_plugin.so` |
| Windows | `target/release/my_plugin.dll` |
| macOS | `target/release/libmy_plugin.dylib` |

### 5.10 插件错误类型

```rust
#[derive(Debug, Error)]
pub enum PluginError {
    LoadFailed(String),    // 插件加载失败
    InitFailed(String),    // 插件初始化失败
    StartFailed(String),   // 插件启动失败
    StopFailed(String),    // 插件停止失败
    NotFound(String),      // 插件不存在
    MetaError(String),     // 元信息错误
    Other(String),         // 其他错误
}
```

---

## 6. 接口定义

### 6.1 SouthDevice Trait

```rust
pub trait SouthDevice: Send + Sync {
    fn device_id(&self) -> &str;
    fn device_type(&self) -> &str;
    fn status(&self) -> Result<DeviceStatus, DeviceError>;
    fn connect(&self) -> Result<(), DeviceError>;
    fn disconnect(&self) -> Result<(), DeviceError>;
    fn read(&self) -> Result<DataFrame, DeviceError>;
    fn read_batch(&self, count: usize) -> Result<Vec<DataFrame>, DeviceError>;
    fn write(&self, data: &[u8]) -> Result<(), DeviceError>;
    fn health_check(&self) -> Result<bool, DeviceError>;
}
```

### 6.2 Device Trait（早期抽象，SouthDevice 的前身）

```rust
pub trait Device: Send + Sync {
    fn read(&self) -> Result<DataFrame, DeviceError>;
    fn write(&self, data: &[u8]) -> Result<(), DeviceError>;
    fn status(&self) -> Result<DeviceStatus, DeviceError>;
    fn device_id(&self) -> &str;
    fn device_type(&self) -> &str;
}
```

> **说明：** `Device` 是 Phase 1 的早期抽象，`SouthDevice` 是增强版本（增加了 `connect/disconnect/read_batch/health_check`）。`Rs485Device` 同时实现了 `Device` 和 `SouthDevice` trait。

### 6.3 DeviceRegistry Trait

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

**设备查询条件：**

```rust
pub struct DeviceQuery {
    pub device_type: Option<DeviceType>,
    pub status_online: Option<bool>,
    pub tags: Option<Vec<String>>,
}
```

### 6.4 MessageBus Trait

```rust
pub trait MessageBus: Send + Sync {
    fn publish(&self, topic: &Topic, msg: Message) -> Result<(), BusError>;
    fn subscribe(&self, topic: &Topic, handler: Arc<dyn MessageHandler>) -> Result<(), BusError>;
    fn unsubscribe(&self, topic: &Topic, handler_id: &str) -> Result<(), BusError>;
}

pub trait MessageHandler: Send + Sync {
    fn handle(&self, message: &Message) -> Result<(), BusError>;
}
```

### 6.5 核心数据类型

**设备状态：**

```rust
pub enum DeviceStatus {
    Online,
    Offline,
    Error(String),   // 设备故障
}
```

**数据帧：**

```rust
pub struct DataFrame {
    pub device_id: String,  // 设备唯一标识
    pub timestamp: u64,     // 时间戳（毫秒）
    pub data: Vec<u8>,      // 数据载荷
    pub quality: DataQuality, // 数据质量
}
```

**数据质量：**

```rust
pub enum DataQuality {
    Good,      // 数据有效
    Invalid,   // 数据无效
    Reserved,  // 保留
}
```

**设备类型枚举：**

```rust
pub enum DeviceType {
    Ttu,           // 配变终端
    Inverter,      // 光伏逆变器
    Charger,       // 充电桩
    FlexibleLoad,  // 柔性负荷
    FireAlarm,     // 消防控制
    Unknown,       // 未知类型
}
```

**设备 ID 命名规范：**
```
格式: {设备类型}_{厂商}_{型号}_{序号}
示例: ttu_huawei_osu_001, inverter_sungrow_sg100_001
```

**消息系统：**

```rust
pub struct Topic(String);

pub struct Message {
    pub topic: Topic,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}
```

**测量值：**

```rust
pub struct Measurement {
    pub name: String,       // 测量点名称
    pub value: f64,         // 测量值
    pub unit: Option<String>,  // 单位
}
```

### 6.6 错误类型定义

**DeviceError（统一设备错误）：**

```rust
pub enum DeviceError {
    Offline(String),        // 设备离线
    Timeout(String),        // 通信超时
    ChecksumFailed(String), // 数据校验失败
    ProtocolError(String),  // 协议错误
    Busy(String),           // 设备忙
    IoError(std::io::Error),// IO 错误
    Other(String),          // 其他错误
}
```

**RegistryError（注册表错误）：**

```rust
pub enum RegistryError {
    AlreadyExists(String),   // 设备已存在
    NotFound(String),        // 设备不存在
    RegisterFailed(String),  // 注册失败
    UnregisterFailed(String),// 注销失败
    Other(String),           // 其他错误
}
```

**BusError（总线错误）：**

```rust
pub enum BusError {
    TopicNotFound(String),   // 主题不存在
    PublishFailed(String),   // 发布失败
    SubscribeFailed(String), // 订阅失败
    UnsubscribeFailed(String),// 取消订阅失败
    Other(String),           // 其他错误
}
```

---

## 7. 文件结构

### 7.1 完整文件树

```
mupc/crates/
│
├── device-trait/                          # 设备抽象层
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                         # 模块导出 + re-export
│       ├── device.rs                      # Device trait (早期抽象)
│       ├── south_device.rs                # SouthDevice trait + ProtocolHandler + HplcDriver + 处理器实现
│       ├── registry.rs                    # DeviceRegistry trait + DeviceQuery
│       ├── message_bus.rs                 # MessageBus trait + MessageHandler
│       ├── plugin.rs                      # Plugin trait + PluginState + NoOpPlugin
│       ├── plugin_loader.rs               # PluginLoader trait
│       ├── types.rs                       # DataFrame, DeviceStatus, DeviceType, Topic, Message, CrcMode, Rs485Config, Parity, 等
│       └── errors.rs                      # DeviceError, PluginError, BusError, RegistryError
│
├── rs485-plugin/                          # RS485 驱动插件
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                         # 插件入口 + re-export
│       ├── device.rs                      # Rs485Device (串口操作, DE/RE GPIO, 事务)
│       ├── config.rs                      # Config (串口参数, de_gpio, re_gpio, 验证)
│       ├── errors.rs                      # Rs485Error
│       ├── protocol.rs                    # Frame 解析, CRC 计算, DataUnitParser
│       └── handlers/
│           ├── mod.rs                     # ProtocolHandlerRegistry + 本地 CRC
│           ├── modbus_handler.rs           # Modbus RTU
│           ├── ttu_handler.rs              # TTU 配变终端
│           ├── inverter_handler.rs         # 光伏逆变器
│           └── charger_handler.rs          # GB/T 27930 充电桩
│
├── hplc-plugin/                           # HPLC 驱动插件
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                         # 插件入口 + FFI 导出
│       ├── config.rs                      # HplcConfig
│       ├── driver.rs                      # HplcDriver trait
│       ├── device.rs                      # HplcDevice (SouthDevice 实现)
│       ├── mock.rs                        # MockHplcDriver
│       └── errors.rs                      # HplcError
│
└── plugin-loader/                         # 动态插件加载器
    ├── Cargo.toml
    └── src/
        ├── lib.rs                          # 模块导出 + re-export
        ├── loader.rs                       # PluginLoaderImpl (libloading 实现)
        ├── registry.rs                     # PluginRegistry + PluginEntry + PluginState
        └── errors.rs                       # LoaderError
```

### 7.2 device-trait Cargo.toml

```toml
[package]
name = "device-trait"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
```

### 7.3 rs485-plugin Cargo.toml

```toml
[package]
name = "rs485-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["lib", "cdylib"]

[dependencies]
device-trait = { path = "../device-trait" }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
parking_lot = { workspace = true }

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[dev-dependencies]
tempfile = "3"
```

### 7.4 hplc-plugin Cargo.toml

```toml
[package]
name = "mupc-hplc-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["lib", "cdylib"]

[dependencies]
device-trait = { path = "../device-trait" }
thiserror = { workspace = true }
parking_lot = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }

[features]
ffi = []
```

### 7.5 plugin-loader Cargo.toml

```toml
[package]
name = "plugin-loader"
version = "0.1.0"
edition = "2021"

[dependencies]
device-trait = { path = "../device-trait" }
libloading = "0.8"
parking_lot = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
```

---

## 8. 配置格式

### 8.1 RS485 设备配置

```json
{
  "rs485_devices": [
    {
      "device_id": "ttu_001",
      "device_type": "ttu",
      "port": "/dev/ttyUSB0",
      "baud_rate": 9600,
      "data_bits": 8,
      "stop_bits": 1,
      "parity": "even",
      "timeout_ms": 1000,
      "device_addr": 0x01,
      "handler": "ttu",
      "de_gpio": 17,
      "re_gpio": 27
    },
    {
      "device_id": "inverter_001",
      "device_type": "inverter",
      "port": "/dev/ttyUSB1",
      "baud_rate": 19200,
      "handler": "inverter",
      "de_gpio": 18,
      "re_gpio": 22
    }
  ]
}
```

**字段说明：**

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | 是 | — | 设备唯一标识 |
| `device_type` | string | 是 | — | 设备类型 |
| `port` | string | 是 | — | 串口路径 |
| `baud_rate` | int | 是 | — | 波特率 |
| `data_bits` | int | 否 | 8 | 数据位 |
| `stop_bits` | int | 否 | 1 | 停止位 |
| `parity` | string | 否 | "none" | 校验位 |
| `timeout_ms` | int | 否 | 1000 | 通信超时 |
| `device_addr` | int | 否 | 0x01 | 设备地址 |
| `handler` | string | 是 | — | 协议处理器名称 |
| `de_gpio` | int | 否 | null | DE 引脚编号 |
| `re_gpio` | int | 否 | null | RE 引脚编号 |

### 8.2 HPLC 设备配置

```json
{
  "hplc_devices": [
    {
      "device_id": "hplc_001",
      "device_type": "hplc",
      "driver": "mock",
      "config": {
        "serial_port": "/dev/ttyUSB2",
        "baud_rate": 115200,
        "chip_type": null,
        "channel": null
      }
    }
  ]
}
```

### 8.3 插件通用配置

```json
{
  "device_path": "/dev/ttyUSB0",
  "timeout_ms": 5000,
  "baud_rate": 9600
}
```

插件配置通过 `serde_json::Value` 传入 `plugin.init(config)` 方法。

---

## 9. 技术决策记录

### 9.1 架构决策

| 决策 | 选择 | 替代方案 | 理由 |
|------|------|----------|------|
| 设备抽象层次 | `SouthDevice` + `Device` 并存 | 统一为单个 trait | 向后兼容 Phase 1 的 `Device` 接口，同时提供增强的 `SouthDevice` |
| 协议扩展方式 | 策略模式 (ProtocolHandler 注入) | 继承/泛型参数 | 运行时可选，配置驱动，无需重新编译 |
| 串口操作 | 直接使用 `libc` termios | `serial` crate | 减少外部依赖，更细粒度控制 |
| 插件隔离 | 同一进程加载（trait object） | 子进程隔离 | 子进程 IPC 开销大，Rust 类型系统可保证安全 |
| 插件 FFI | `unsafe extern "C" fn` | C ABI struct | 简化绑定，`libloading` 原生支持 |

### 9.2 技术债清单

| 项目 | 说明 | 优先级 |
|------|------|--------|
| 串口抗干扰重试 | RS485 通信不稳定时自动重试 | 中 |
| 热插拔配置重载 | 运行时添加/移除设备配置 | 中 |
| 插件签名验证 | 加载插件前验证数字签名 | 低 |
| `SdkHplcDriver` | 对接实际 HPLC 芯片 SDK | 低（Phase 3） |
| `FireAlarmHandler` | 消防专用协议处理器 | 低（按需实现） |
| Windows 串口支持 | 实现 Windows 平台串口操作 | 低 |
| 异步 recv | HPLC driver 异步非阻塞接收 | 低（Mock 使用同步 sleep） |
| 设备心跳自动检测 | 定时检查设备在线状态 | 中 |

### 9.3 风险与对策

| 风险 | 等级 | 对策 |
|------|------|------|
| RS485 电气特性导致通信不稳定 | 低 | 增加重试机制和超时控制 |
| 插件隔离不足导致崩溃影响主进程 | 中 | 使用 Rust 的 Safe Trait 约束，`catch_unwind` |
| `libloading` 在 Windows 平台兼容性 | 低 | 测试阶段覆盖 Windows 环境 |
| 多线程竞争串口访问 | 低 | `StdMutex<()>` 保证事务原子性 |

### 9.4 验收标准

> 验收标准（功能验收、质量验收）详见 [02-MUPC-南向通信-PRD](../specs/modules/02-MUPC-南向通信-PRD.md) 第 7 章。

---

## 附录 A：术语表

| 术语 | 说明 |
|------|------|
| MUPC | 微电网特种调控装置 |
| TTU | 台区智能融合终端（配电变压器终端单元） |
| HPLC | 高速电力线载波通信 (High-speed Power Line Carrier) |
| RS485 | 串行通信总线标准（半双工，差分信号） |
| DE/RE | RS485 半双工使能引脚（Driver Enable / Receiver Enable） |
| GPIO | 通用输入输出引脚 |
| FFI | 外部函数接口 (Foreign Function Interface) |
| cdylib | C 动态库格式（Rust crate-type） |
| Modbus RTU | 串行通信协议，RS485 物理层上的常用工业协议 |
| GB/T 27930 | 电动汽车非车载传导式充电机与 BMS 通信协议 |
| OFDM | 正交频分复用 (Orthogonal Frequency Division Multiplexing) |
| termios | Unix 终端 I/O 控制系统（串口配置标准接口） |
| CRC | 循环冗余校验 (Cyclic Redundancy Check) |
| RKNN | Rockchip Neural Network（RK3588 NPU 推理框架） |

## 附录 B：来源文档

| 文档 | 说明 |
|------|------|
| `docs/superpowers/specs/modules/02-MUPC-南向通信-PRD.md` | 产品需求文档 |
| `docs/superpowers/plans/2026-05-27-MUPC-Phase2A-南向通信-实施计划.md` | Phase2A 实施计划 |
| `docs/superpowers/plans/2026-05-29-MUPC-南向通信扩展-实施计划.md` | 南向通信扩展实施计划 |

---

**文档状态**: 定稿
**维护者**: MUPC Team

---

## 10. Phase 2A 实现笔记

> **来源**: `docs/superpowers/reports/2026-05-27-MUPC-Phase2A-南向通信-实施计划.md`（已归档）
> **状态**: 核心实现已完成，4 个单元测试待修复
> **团队**: 团队A（2人）

### 10.1 实施任务分解

所有 Task 采用测试优先（TDD）策略：先编写测试，再实现功能。

| Task | 内容 | 提交信息 |
|------|------|----------|
| Task 1 | device-trait 设备抽象层（types.rs -> errors.rs -> device.rs -> registry.rs -> message_bus.rs -> lib.rs） | `feat(device-trait): 实现设备抽象层 trait 定义` |
| Task 2 | rs485-plugin RS485 驱动（配置解析 -> 设备驱动 -> 协议解析） | `feat(rs485-plugin): 实现 RS485 驱动插件` |
| Task 3 | plugin-loader 动态插件加载（加载器 -> 生命周期管理） | `feat(plugin-loader): 实现动态插件加载器` |
| Task 4 | 集成与测试（workspace 注册 + 集成测试 + clippy） | `feat(integration): 集成南向通信模块` |

### 10.2 技术栈

| 组件 | 技术选型 |
|------|----------|
| 插件系统 | `libloading` + trait object |
| 串口通信 | `serial` crate |
| 序列化 | `serde` + `serde_json` |
| 错误处理 | `thiserror` |

### 10.3 里程碑

| 里程碑 | 内容 | 交付物 |
|--------|------|--------|
| M2.1 | 核心 trait 定义 | device-trait crate |
| M2.2 | RS485 插件 | rs485-plugin crate |
| M2.3 | 插件加载器 | plugin-loader crate |
| M2.4 | 集成测试 | 完整南向通信模块 |

### 10.4 实施要点

- 串口操作使用 Unix `libc` termios 直接操作，避免第三方 crate 依赖
- 事务原子性通过 `StdMutex<()>` 全局锁保证，跨异步任务保证设备独占访问
- 插件通过 `libloading` 加载 `.so/.dll`，FFI 导出 `create_plugin()` + `plugin_meta()`
- 依赖顺序：device-trait 先行，plugin-loader 和 rs485-plugin 并行依赖 device-trait
