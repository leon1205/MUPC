# MUPC 南向通信模块 设计文档

> ✅ **`[DESIGN_APPROVED: 2026-09-21, 设计评审员]`** —— §11「站级南向设备语义点表集成（S3b-2）」（v1.4）经三轮设计评审通过（§10「S3a」沿用既有批准，见 S3a 计划合同）。

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
10. [站级多从站统一调度框架](#10-站级多从站统一调度框架)
11. [站级南向设备语义点表集成（S3b-2）](#11-站级南向设备语义点表集成s3b-2)

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

### 2.8 南向控制指令分发（SouthCommandSender）

**来源**：策略引擎模块通过 `SouthCommandSender` trait 向南向设备分发控制指令

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
| `Rs485SouthSender` | `south_command_sender.rs` | 真实 RS485 通信 |
| `HplcSouthCommandSender` | 预留（未实现） | 真实 HPLC 通信 |

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

### 9.2 风险与对策

| 风险 | 等级 | 对策 |
|------|------|------|
| RS485 电气特性导致通信不稳定 | 低 | 增加重试机制和超时控制 |
| 插件隔离不足导致崩溃影响主进程 | 中 | 使用 Rust 的 Safe Trait 约束，`catch_unwind` |
| `libloading` 在 Windows 平台兼容性 | 低 | 测试阶段覆盖 Windows 环境 |
| 多线程竞争串口访问 | 低 | `StdMutex<()>` 保证事务原子性 |

**重试机制与故障隔离（实现级）：**

- **事务超时**：单次 Modbus 请求-响应超时，默认 1000ms（配置项 `timeout_ms`，见 §8.1 配置表）
- **重试**：超时后按可配置次数重试（对齐 PRD §7.2「可配置重试次数和超时时间」）
- **故障隔离**：单设备通信故障不影响其他设备（对齐 PRD §7.2）；故障设备跳过本轮轮询，独立标记离线并告警，恢复后自动重新上线
- **重试边界**：仅对超时/无响应/CRC 错重试；地址非法、数据非法等确定性错误直接返回错误、不重试

### 9.3 验收标准

> 验收标准（功能验收、质量验收）详见 [02-MUPC-南向通信-PRD](../specs/modules/02-MUPC-南向通信-PRD.md) 第 7 章。

## 10. 站级多从站统一调度框架

> **目标平台**：BECG-3568（RK3568，后续 RK3588 接口一致），板载 **8 路隔离 RS485（每路独立 Modbus master）**。接线分配见 **核间 10 §12.1** 与 deploy/deploy.md §九（现场接线与配置核对）。

### 10.1 背景与目标（Why）

现南向采集是 **core-bin 逐个硬编码 task**（台区总表一个、南向 pv/load 一个），`DeviceRegistry`/`MessageBus` trait 存在但未接线；AiIntegrator 仅 `latest_data` 单输入。新增站级从站（BMS/空调/储能表/消防状态）若沿用硬编码会失控。目标：把「南向站级采集」收敛为**配置驱动的统一多端口多从站调度器**（新 crate `mupc-southd`），复用既有 rs485-plugin 串口/handler 与 meter_regs 解码；每口独立 master 并行、口内多从站串行轮询；站级故障隔离。

**范围**：BMS(RS485-2/ttyS2)、空调(RS485-3/ttyS3)、关口表/台区总表(RS485-4/ttyS4)、储能表(RS485-5/ttyS5)、消防状态(RS485-6/ttyS6)。PCS(RS485-1/ttyS0) 走 intercore `modbus_rtu`（核间 10），**不在**本调度器内。**首版边界**：严格「采集 + meter_grid phase 输入 / battery SOC 输入 + 事件上送」，**不承载控制写侧**（空调只读遥测，温控/启停写指令与 pv/load 命令下发属未来负载管理，另设计）。**与 §9.2 重试条款关系**：§9.2 基于「单总线多设备」假设；本框架 BECG 每口独立 master，站超时隔离同口其它站，确定性错误不重试，语义见 §10.7。

### 10.2 架构

```
mupc-southd（新 crate）
├─ SouthScheduler         # 口级调度：每 port 一条采集 task
│   ├─ PortRuntime        # 串口 fd + tx 锁 + 口状态（BECG 每口独立 master）
│   │   └─ StationLoop    # 口内从站串行轮询（同口多从站），站超时→该站 offline
│   └─ Station            # 配置驱动：port + protocol + slave + interval + regs + role
├─ role 映射（Station→DataPackage）：meter_grid/meter_batt/battery/hvac/fire
└─ 上送：telemetry 落库 + role 分发（grid_meter → AiIntegrator.latest_data）
依赖复用：rs485-plugin 串口/ModbusRTU handler、data-processing meter_regs 解码、storage telemetry/events
```

- **线程承载**：rs485-plugin 串口为同步阻塞 fd，每口采集 task 内以 `spawn_blocking`/独立线程承载阻塞读写（避免多口阻塞占满 async worker）。
- 每站独立 `port` → 不同口并行（BECG 隔离 485 各自 master）；同 `port` 多站 → 口内串行轮询（现有 Rs485Device 事务 tx 锁机制扩展，单请求 slave 参数化——当前 Rs485Device 固定 device_addr，框架内新增「请求级 slave 覆盖」）。**口调度预算**：站按 `interval_ms` 计算 next_due；同口慢从站超时阻塞轮询时，优先保 `role=meter_grid/battery` 的站（防超 5s stale），慢站按 offline 计并降频，不拖累关键站 cadence。
- **故障隔离**：站级请求超时/CRC 错 → 标记该站 offline 并告警，跳过本轮；不牵连同口其它站；恢复后自动上线。

### 10.3 stations 配置（core_config `south_stations:` 段）

```yaml
south_stations:
  poll_ms: 1000            # 缺省轮询周期
  stale_timeout_s: 5       # 上送数据过期阈值（对齐 AiIntegrator 5s）
  stations:
    - { id: grid_meter,    role: meter_grid, port: ttyS4, protocol: modbus, slave: 1, interval_ms: 1000, regs: <分相 p/q/pf/u/i 点表> }
    - { id: meter_storage, role: meter_batt,  port: ttyS5, protocol: modbus, slave: 1, interval_ms: 1000, regs: <储能表点表> }
    - { id: bms,           role: battery,     port: ttyS2, protocol: modbus, slave: 1, interval_ms: 1000, regs: <BMS 点表> }
    - { id: ac_unit,       role: hvac,        port: ttyS3, protocol: modbus, slave: 1, interval_ms: 5000, regs: <空调点表> }
    - { id: fire_host,     role: fire,        port: ttyS6, protocol: modbus, slave: 1, interval_ms: 1000, regs: <消防状态点表> }
```

- 点表 `regs`：配置化寄存器映射，复用 meter_regs `RegFormat`（float32/int32_scaled）解码。
- **master_meter 收敛**：现有 `master_meter` 配置段与硬编码总表 task **收敛为 `role: meter_grid` 站**（行为回归等价，策略 phase 输入链路不变）；`master_meter.enabled/serial_port` 语义保留为 meter_grid 入口（兼容别名，实施后统一单写方 = southd mapper **唯一** `set_latest_data`，移除硬编码总表 task，避免双写 AiIntegrator）。**迁移期排他**：`master_meter` 段与 `south_stations.meter_grid` 站**二选一启用**（validate 互斥，禁双 master 同总线）；收敛目标形态 = 删除 `master_meter` 段、总表统一走 `south_stations`（分批：先单写守卫断言 → 后移除 alias）。
- **跨段校验（收敛后迁移）**：`south_stations.port` 与 `intercore.modbus_rtu.serial_port`（PCS ttyS0）**不得重复**（双 master 共总线禁止）；`meter_grid` 站 `interval_ms < 5000`（对齐策略数据新鲜度）；原 `master_meter` 段 `/dev/ttyUSB0` 特判随收敛移除（BECG 无 USB 概念），校验迁至 `south_stations` 段。
- **新鲜度共享常量**：策略 5s 数据新鲜度与各站 `interval_ms` 边界统一引用共享常量 `data_freshness_ms`（避免三处硬编码漂移）。
- **BMS 多包/多块（扩展点）**：真实 BMS 若多从站包或多寄存器块，以「同口多从站各包一站」或扩展 `Station.regs` 为多块聚合处理；SOC 聚合规则待厂方点表确认后落地。
- **协议**：第一版按通用 Modbus RTU + 配置点表；BMS/空调/消防主机若厂家私有帧 → 追加 `ProtocolHandler` 实现（registry 已有扩展点），私有点表待厂方提供后落地。

### 10.4 role → DataPackage 语义映射

| role | 产出字段 | 去向 |
|---|---|---|
| meter_grid | `electrical.phase`（分相 p/q/pf/u/i，总表） | `AiIntegrator.latest_data`（策略 phase 源，语义不变）+ telemetry + IEC104 |
| meter_batt | `electrical`（储能表总能/总无功） | telemetry + 展示/校核 |
| battery | `battery.soc/soh/temperature` + 告警字 | telemetry + 事件 + SOC 融合（见 10.5） |
| hvac | `device_status` 环境量/状态字 | telemetry + 事件 |
| fire | `device_status` 消防状态字 | 事件（与 DI3 消防报警融合规则见下） |

> **fire(RS485) 与 DI3 消防报警融合**：同一消防信号可能双源接入。规则：DI 干接点为**高完整性主判据**（触发即报，RS485 作确认/校核）；触发取 **OR**（任一源触发即触发）——OR 仅限 fire 状态/告警事件；**停机仍仅以 DI3 触发**（见核间 10 §12.3）；恢复需**双方复位**；`events` 以「消防+源」去重键防双报。详细联锁语义见核间 10 §12.3。

### 10.5 与策略的接口（SOC 源融合）

BMS 站在线时其 SOC **优先**于 intercore `latest_soc`（核间回读）注入策略 `TaiStorageStrategy`；BMS 掉线回落核间 SOC（可配优先级）。生效点：04 策略引擎 §2.11 AiIntegrator 数据注入。本版仅采集 + SOC 输入，不做其它策略融合。

**推包与新鲜度闸门**：`set_latest_data` **仅由 `meter_grid`（phase 真源）更新触发**推进 AiIntegrator 控制闸门时间戳（`last_data_ts`）；其余 role（battery/hvac/fire/meter_batt）更新**不推进**闸门（只落库/事件 + 各自逐源时间戳）——避免总表掉线而 BMS 活性时，整体 5s 闸门被活性站掩盖、策略以陈旧 phase 驱动。phase/SOC 逐源过期标记同构（04 §2.11.1）。

### 10.6 上行、事件与存储

每站采集结果 DataPackage → storage `telemetry`（device_id 维度，已有）批量落库；role=fire/hvac 状态变化与告警字 → storage `events`/`faults` 落库 + SSE（复用 startup SSE 推送）；`DeviceRegistry`/`MessageBus` trait 本次接线（承载 device 注册与事件路由）。

### 10.7 错误与故障隔离

站超时/CRC/地址错 → 该站 offline 计数 + `events` 告警 + role 数据置 stale；同口其它站照常；恢复探测后自动上线。确定性错误（非法地址/数据）不重试。口 open 失败 → 该口全部站 offline，启动告警不阻断（PCS 主链路不在本模块）。

### 10.8 文件结构与测试

> **依赖形态**：`mupc-southd` 以**静态库**依赖 rs485-plugin 的复用 API（串口、`ProtocolHandler`/`ModbusRTU` handler、`meter_regs` 解码），避免与 cdylib 插件加载产生双实例；`Rs485Device` 增加请求级 slave 参数化（现 `encode_request` 用固定 `device_addr`，签名按口内多从站需要扩展）。
> **方向控制确认（板端待核）**：现有 rs485-plugin DE/RE GPIO 源自外接 USB-485 适配器假设；BECG 板载隔离 485 若为自动方向收发器则无需 DE/RE（配置缺省关闭），若仍需方向控制须按板端实际 GPIO 提供；实现前以板端核对为准。

- **`mupc-southd`**（新）：`config`（stations 反序列化+validate）、`station.rs`（模型/role）、`scheduler.rs`（口级 task + 口内轮询）、`port_runtime.rs`、`mapper.rs`（role→DataPackage）。
- **core-bin**：移除硬编码总表/pv-load 采集 task 的 `set_latest_data` 段，改装配 southd；`south_stations` 配置段。
- **device-trait/rs485-plugin**：`Rs485Device` 请求级 slave 覆盖（同口多从站需要）、`DeviceRegistry` 接线。
- **strategy-engine**：SOC 源优先级小改（04 §2.11）。
- 测试：调度器多站并发/同口串行、站超时隔离、role→DataPackage 映射、配置解析/校验、SOC 优先级切换、总表回归（phase 链路与既有策略测试零破坏）。

### 10.9 验证状态

设计按 writing-plans 分批实施；master_meter 收敛以总表回归测试（策略 phase 输入等价）为闸门。设备点表（BMS/空调/消防）待厂方提供后填配置。

---

## 11. 站级南向设备语义点表集成（S3b-2）

> **状态**：`[DESIGN_APPROVED: 2026-09-21]` —— 已按二轮设计评审意见修订（v1.4），**三轮设计评审通过**（本章不自行改标记，标记由设计评审员于 2026-09-21 补注）。
>
> **本版（v1.4）修订范围（回应二轮评审的「3 处须补 + 8 条建议」；小范围收尾，不推翻 v1.3 已获认可内容）**：
> - **须补 1（事实错误订正）**：§11.5.2(2) 的 `grid_meter` 行原写 `p→p_total→q→pf→u→i`，实为**单测 fixture（`config.rs:324-329`）的形态**；按**生效配置的真值**（`deploy/config/*.yaml` 与 PRD §9.4.1 第 6 站）重写为 `p(0x1000,6)→q(0x1006,6)→pf(0x100C,6)→u(0x1012,6)→i(0x1018,6)→p_total(0x101E,2)`，并**分别标注"生效配置真值 / 单测 fixture 形态"及其差异**；同时逐站复核 §11.5.2(2) 其余 5 站（结论：仅 `grid_meter` 一行有误，其余取值与厂方/PRD 一致）；
> - **须补 2（顺序冲突钉死）**：C2 的 `validate_rejects_meter_grid_duplicate_block_name` 与**新规则 10（点名唯一）**冲突 ⇒ §11.5.1「顺序」与 §11.5.3.4 C2 明确**实现判定顺序约束**（既有 `meter_grid` 校验整组先于 `validate_station_regs`），保住 C2 断言（**不订正 C2**，理由见该处第 4 点）；
> - **须补 3（PRD 差异登记）**：PRD §9.8.4「PCS 32 位电量比对未通过（若配置）→ 事件」**无运行期落点**（比对是 RC-2 的现场人工比对，且 §9.7.2 明令本轮不做量程自校验）⇒ 取「RC-2 未通过则不配点」替代口径，在 §11.12.2 **登记 Δ-9**（标注需需求侧追认或订正 PRD），并在 §11.4.7 事件表 ⑦ 给出**一眼可见的处置**；
> - **建议（一并改净）**：§11.7.2 的 `bms_alarm_225` 示例取值订正（位 424 = 簇一级告警，非"簇 SOC 低·轻"）；`RegBlockConf` 字段数「9 → 13」订正为 **6 → 10**；A14「三个 crate」订正为 **1 个（`mupc-southd`）**；A1 的 `fire_1` 行号 `:345` → **`:348`**；§11.5.3.5 两个需补 `regs` 的用例**移入 ①（A15/A16）**并给该节改号；删除无定义的 `emit_falling_edges(EmitBidi)`；补 `Rs485PortBus::open` 的 **`parity` 透传**落点；补 **RC-12**（消防/PCS 设备软件版本核对，§9.7.2 第 3 条 + §9.8.4）。
>
> **上一版（v1.3）修订范围（回应首轮设计评审的 4 个 P0 + 3 处完整性遗漏 + 4 条建议）**：
> - **P0-1**：§11.5.1 第 15 条（极大性）**重写落点**（限定适用域 + 改判据），§11.5.2 给出「§9.4.1 六站配置必然通过」的逐站证明，§11.2.1/§11.13 的 T3/T7 口径同步订正；
> - **P0-2**：§11.4.4 取「**无行不拒**」，删除 ① 中"无行"半句，全表自洽；
> - **P0-3**：新增**消防事件落点**（§11.4.7 事件模型推广为「信号」+ §11.7.2 消防信号清单 + §11.10 与 G-6 的边界澄清）；
> - **P0-4**：§11.5.3 连锁订正清单**扩至「用例名 + 断言」级**，并区分「fixture 订正 / 断言订正 / 断言不得改」；
> - **完整性遗漏**：新增 §11.5.1 规则 18（`pcs` 站 `interval_ms ≥ 500ms` 下界）、规则 19（无 `points` 块的 `count` 宽度护栏）、消防事件（同 P0-3）、§11.11.3 补 RC-9/RC-10/RC-11（钢瓶气压口径 / Q-9 / Q-19）；
> - **建议**：`WordOrder` 单一定义（§11.4.2，§11.4.1 改为复用）、Battery 底块读失败语义（§11.4.6）、无 `points` 的 32 位块静默少产点护栏（规则 19）。
>
> **输入契约**：`specs/modules/02-MUPC-南向通信-PRD.md` §9（`[REVIEWED: PASS: 2026-09-21]`，**v1.7 补登 + v1.8 订正**）。§9 已定的块/点模型、位置式点名、类型来源三分类、G-1…G-6 范围、Q-1/Q-20 登记与 AC/RC 划分是**需求约束**，本章只落地、不推翻。
>
> **与 PRD v1.8 / v1.7 的对齐声明（本设计已按同一表述对齐，不另起炉灶）**：
> ① **第 15 条**：PRD v1.8 已落定 ——「**仅作用于声明了 `points` 的块**」「未声明 `points` 的块（含既有 `meter_grid` 形态、`discrete` 位块）**不参与**合并判定」「拒绝条件 = ① `func`/`byte_swap` 同 ② 地址连续 **③ 合并后空洞 ≤ 4** **④ 合并后 `count ≤ 120`**」「`read_slice: true` 豁免，且只在本条适用域内有对象」。本设计的 §11.5.1 #15 / §11.5.2 **四条全实现**（不是只取 ④），并额外给出：**③ 在适用域内由规则 11 的首尾锚定恒成立**（⇒ 实现 ③ 不引入额外拒绝，与 PRD 行为等价）、**§9.4.1 六站必然通过本条**的逐站证明、以及**建议的实现判定顺序**（先 ④ 再 ③）。**本设计未推翻：`grid_meter` 形态、`discrete` 位块、既有站行为一律不参与判定。**
> ② **Δ-1**（`format` 缺省 = `float32`）与 **Δ-4**（`fire_det_count` 定名）已由 PRD v1.7 正式补登，本设计**不再是差异**（§11.12.2 已标注"已闭合"）；③ **Δ-3**（块级 `read_slice`）已由 PRD v1.7 正式补登为块级字段（§9.4.2.4），本设计的 §11.4.1/§11.5.2 改为**引用 PRD 定义**而非"设计新增"。
>
> **与 §10 的关系**：§10（S3a，已批准并实施）的**调度架构、故障隔离语义、role→DataPackage 分发骨架全部不变**。本章只做两件事：① 补齐点表**表达与读取所必需**的数据面能力（PRD §9.1.1 的 G-1…G-5）；② 接入**一个新 role**（`Role::Pcs`）并落实 5 份厂方点表。所有扩展都落在 §10 已预留的接缝上（见 §11.3 的"扩展点"列）。

### 11.1 背景与现状差距（Why）

**现状（代码事实核对，2026-09-21）**：`mupc-southd` 的 role 分发骨架已就位，但数据面只能表达"每块 2 寄存器 1 值"：

| 现状代码事实 | 位置 | 后果 |
|--------------|------|------|
| `Role` 无 `Pcs`；`Role::{MeterBatt,Hvac,Fire}` 在 mapper 返回 `empty_package()` | `config.rs:20` / `mapper.rs:199` | 三站只作"站在线"信号 |
| `RegFormat` 仅 `Float32`/`Int32Scaled`，**两者都固定占 2 寄存器**；`decode_regs` 对 `len < 2` 返回 0.0 | `meter_regs.rs:16,39` | 16 位点表（绝大多数）无法表达 |
| 只有 `scale`，无 `offset` / `byte_swap` / `word_order` | `config.rs:65` | 温度类（`raw−40`）与 PCS 字节序不可表达 |
| `StationBus` 只有 `read_holding`(FC03) / `read_input`(FC04) | `port_runtime.rs:35` | FC02 位块（BMS 288 位 + 空调 31 位）读不到 |
| `mapper::telemetry_points` 每块只取**前 2 寄存器解 1 标量**、metric = 块名 | `mapper.rs:207` | 单点块（`count:1`）**不产任何点** |
| 无逐点换算参数 | `mapper.rs` 全篇 | 同一读窗口内混排换算（0.1V 与 0.01A、`raw−40` 与纯计数）无法表达 |

**本轮目标**：把上表 6 项能力补齐（PRD G-1…G-5），并让 5 份厂方点表（§9.5 共 **618 点**：`battery` 345 + `pcs` 72 + `meter_batt` 40 + `fire` 127 + `hvac` 34）**逐点可配置、可解码、可校验、可消费**。G-6（寄存器内字节拆分）PRD 已裁定**本轮不做**，替代口径见 §11.10。

**G-1…G-6 的设计落点（一眼定位）**：

| 差距 | 设计落点 |
|------|----------|
| G-1（16 位格式） | §11.4.2 `RegFormat::{Uint16,Int16}` + `RegDecode::width()` |
| G-2（`offset`） | §11.4.1 块/点级 `offset`；§11.4.2 换算 `值 = raw×scale + offset` |
| G-3（FC02） | §11.4.1 `RegFunc::Discrete`；§11.4.5 `StationBus::read_discrete` + `unpack_bits` |
| G-4（单寄存器块 / 多值块 / 逐点换算） | §11.4.1 `points[]`；§11.4.3 `points::expand()`（校验与运行期**同一函数**） |
| G-5（字节序/字序） | §11.4.1 `byte_swap` / `word_order`；§11.4.2 解码顺序（先逐寄存器 swap，再按字序拼） |
| G-6（字节拆分） | **不做**；§11.10 整字采集 + 展示层拆解 |
| **消防事件（`fire → events`）**（PRD §9.6.1 明文要求，**不属 G-1…G-6**，v1.3 补） | **§11.2.6 选型（F3）→ §11.4.7.1 统一事件模型（`SignalSpec` + `EdgeTracker`）→ §11.7.2 信号清单 → §11.10 与 G-6 的三层边界表** |

### 11.2 方案探索与选型决策（先探索，后设计）

#### 11.2.1 配置模型：块=事务 / 点=换算口径

| 路线 | 形态 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| **A1（选定）块内嵌 `points[]`** | `RegBlockConf` 原地扩字段：传输口径（`func/addr/count/byte_swap`）+ 缺省换算（`format/scale/offset`）+ 可选 `points[]` | 与 PRD §9.4.2.4 字段表**逐字对应**；向后兼容仅靠 `#[serde(default)]`；单一解析路径、单一校验器 | `RegBlockConf` 字段变多（**6 → 10**：既有 `name`/`addr`/`func`/`format`/`scale`/`count`，新增 `offset`/`byte_swap`/`points`/`read_slice`；v1.4 订正字段数 —— 原写"9 → 13"多算） | **选 A1** |
| A2 双形态枚举 | `RegsConf = Legacy(Block) \| Pointed(Block+points)`（untagged） | "老写法/新写法"概念分离清晰 | YAML 两套写法长期并存 → 文档歧义 + 校验分支翻倍；`untagged` 报错信息极差 | 否 |
| A3 双数组并行 | 保留 `regs` 旧语义 + 新增 `blocks` 段，旧站走 `regs`、新站走 `blocks` | 旧站零风险 | 同一语义两处配置（漂移）；`meter_grid` 站要同时兼容两段；违背 PRD"唯一落地规则" | 否 |

**不破坏既有 `meter_grid` 写法的机制（本设计的关键约束）**：新增字段**全部**带 `#[serde(default)]`，且**缺省值 = 既有行为**：`offset=0.0`（等价无偏移）、`byte_swap=false`、`points=[]`（空）、`read_slice=false`。因此既有 `- { name: p, addr: 0x1000, format: int32_scaled, scale: 0.01, count: 6 }` 一行的解析结果与改动前**逐字段相同**；`meter_grid` 的运行路径（mapper 按**块名** `p/q/pf/u/i/p_total` 查找 + `decode_phase_block`）**完全不经过** `points[]` 与新的逐点展开（§11.4.6），回归锚 = `tests/grid_convergence.rs` 的**全部断言**（`config.rs` 的既有用例中，**只有 meter_grid-only 系列与解析类**同属回归锚 —— 非 grid 站的用例含 9 处因 metric 更名而必须订正的期望值，见 §11.5.3.3/§11.5.3.4）。

> **一处刻意的行为变更（须登记）**：`telemetry_points` 对**未声明 `points` 的块**由"取前 2 寄存器 1 点、metric=块名"改为"**每个值槽 1 点、metric=`<块名>_<序号>`**"（PRD §9.4.2.1 第 4 条）。该函数**只被非 grid 站调用**（grid 走 `on_grid_package`），而当前生效配置中只有 `grid_meter` 一个站 → **线上零影响**；但任何"非 grid 站靠块名做 metric"的旧配置（`deploy` 里的注释占位）会改名，须随本轮一并对齐 §9.4.1（§11.7.4 迁移清单）。
>
> **该变更的测试级影响（上轮评审 P0-4 的要点，清单见 §11.5.3）**：**线上零影响 ≠ 测试零影响** —— 单元测试里大量以"非 grid 站 + 块名即 metric"构造期望值，metric 更名后这些**断言期望值必须订正**（属"断言订正"，不是回归）。因此 §11.2.1 的"回归锚"口径随之收窄为：**`tests/grid_convergence.rs` 的断言零改动**（grid 路径不经 `telemetry_points`），`config.rs`/`core_config.rs`/`scheduler.rs`/`mapper.rs` 的既有用例按 §11.5.3 的**分类清单**处理（fixture 订正 / 断言订正 / 不得改）。

#### 11.2.2 `RegFormat` 的扩展方式

| 路线 | 形态 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| **B1（选定）扩枚举 + 引入解码规格** | `RegFormat` 增 `Uint16/Int16` 并给 `reg_width()`；新增 `RegDecode{format,scale,offset,word_order,byte_swap}` 承载解码；`decode_regs` 保留为薄包装 | 宽度/字节序/字序**集中在一处**（"代码只提供通用解码原语"，PRD B-1 裁定）；既有调用点**零改动**；4 种格式用 `match` 足够 | `decode_regs` 与 `RegDecode::decode` 两个入口（须靠测试钉住等价） | **选 B1** |
| B2 trait 化 codec | `trait PointCodec { fn width(); fn decode() }` + 各格式实现 | 未来加格式不改 match | 4 个格式无扩展压力（YAGNI）；动态分派/trait object 徒增复杂度 | 否 |
| B3 双枚举并存 | 块级仍 `RegFormat`，点级新 `PointFormat` | 不动既有类型 | 双枚举的转换/校验/文档三处重复，`format` 一个概念两个名字 | 否 |

宽度口径**单点定义**：`RegFormat::reg_width() -> usize`（`Uint16`/`Int16` → 1，`Float32`/`Int32Scaled` → 2）。PRD 的"32 位点对齐/占 2 寄存器"等规则全部引用该函数，不再散落魔数。

#### 11.2.3 FC02 离散输入的接入点

| 路线 | 形态 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| **C1（选定）`read_discrete → Vec<bool>`** | bus 层完成"响应字节 → 位向量"解包（`unpack_bits` 纯函数在 rs485-plugin） | 语义最清晰（长度 = 位数）；解包公式只有一份；MockBus canned 注入即"位向量"，AC-3 逐位比对**直接可测** | `StationBus` 增 1 个方法（MockBus/未来实现都要补） | **选 C1** |
| C2 bus 返回原始字节 | `read_discrete → Vec<u8>`，解包在 southd | 帧层最小改动 | 帧格式知识泄漏到 southd（MockBus 也要造字节）；解包公式与 §9.7.4 的对应关系跨 crate 断裂 | 否 |
| C3 复用 `Vec<u16>` 签名 | 把位图塞进 u16 数组，复用现有 `Result<Vec<u16>>` | diff 最小 | 位数非 16 倍数时尾部含无关位（AC-3 明确要求"末字节高 1 位不得污染前 31 位"）→ 需另一套文档化的掩码约定，最易错 | 否 |

#### 11.2.4 位块点的落库策略（PRD §9.8.1 末条要求设计定口径）

| 路线 | 形态 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| D1 全量落库（现状口径） | 每轮 319 个位点全落 telemetry | 实现最简、时序最完整 | BMS 位块 **288 行/s ≈ 2490 万行/天**；`storage` 逐行 `INSERT ... VALUES`（事务批内）→ BECG-3568 上写入压力与 `retention` 清理量级均不可接受；而稳态下 99.9% 的行是重复的 0 | 否 |
| **D2（选定）变化沿落库 + 告警上升沿事件** | 位点**仅在与上一轮不同时**落 telemetry；`alarm` 类的 0→1 跳变另产**事件**；模拟量点照旧全量落库 | 稳态写量≈0；跳变有记录、可回溯；与 PRD §9.6.1"告警位→events/SSE"一致；**内存现势值**仍每轮可得（供即时展示） | 需每站一份位图记忆（288 位 ≈ 36 字节/块，可忽略）；"某位长时间未变"时最后一条 telemetry 时间戳陈旧（须按 §11.7.3 展示口径标 stale） | **选 D2** |
| D3 位块不入 telemetry，只产事件 | 位点只走 events | 写量最小 | 丢失"位现势值"，无法回答"此刻该位是 0 还是 1"（只能靠事件重建）；AC-6 ① 的点产出与落库被割裂 | 否 |

> **落库口径与 AC-6 的分工**：`mapper::telemetry_points` 仍**返回全部点**（含 288 位，AC-6 ① 在 mapper 层断言"点产出"）；**变化沿过滤发生在 scheduler**（§11.4.7），即"点产出"与"落库节流"是两层，互不混淆。
>
> **保留策略**：沿用 `storage` 既有 `telemetry_retention_days` / `event_retention_days`（`RetentionManager`），本轮**不新增**清理逻辑；模拟量点新增约 299 行/轮（618 − 319 位点 = 299），5 站合计 ≈ 300 行/s，与既有 `meter_grid` 量级同阶，retention 天数须在投运前按盘容量复核（§11.12 待决项）。

#### 11.2.5 Q-1（BMS 主从方向）：口径裁定与可切换设计

**这是 PRD 标为【设计阶段阻塞】的项，本节给出显式处理（详见 §11.9）**。三条路线的比较：

| 路线 | 形态 | 是否可行 | 结论 |
|------|------|----------|------|
| E1 按 PRD §2.2 单一口径实现（MUPC 主动轮询） | 无切换代码 | 可行 | 采纳为**基线**，但不单独使用（见 E3） |
| E2 实现"主站轮询 + 被动接收"双模式可配置切换 | southd 增从站模式 | **不可行（伪选项）** —— Modbus 从机语义下，"BMS 做主机"意味着 BMS 来**读 MUPC 的寄存器**，链路上**不存在** BMS 数据的下行通道；MUPC 拿不到数据不是"southd 缺一个模式"，而是**该采集链不存在** | 否（诚实说明，见 §11.9.2） |
| **E3（选定）基线轮询 + 消费侧与发起方解耦 + 第二方案影响面评估** | 按 E1 实现；同时把 SOC 消费契约锚定在"**点名 `soc`**"而非"battery 站轮询"（§11.9.3），并给出相反口径下的替代链路（BMS LAN/Modbus-TCP 客户端）与切换判据/代价/新鲜度影响 | 可行且满足 PRD ③"给两套可切换方案"的实质 | **选 E3** |

选 E3 的关键权衡：E2 看起来"更灵活"，但它把"链路是否存在"错当成"配置项"——真正能切换的不是初动方向，而是**接入路径**（RTU 主站轮询 / TCP 客户端）。E3 把**唯一会随口径变化的接缝**（SOC 供给源）从调度器里剥离出来，使两种口径下的消费侧（AiIntegrator/策略/telemetry）**零改动**。

#### 11.2.6 消防事件（`fire → events`）的落点（v1.3 新增，P0-3 的方案探索）

**问题**：PRD §9.6.1 要求 `fire` 的系统状态位/探测器/火警状态进 events，但消防**没有任何 `discrete` 块**（全部状态量在保持寄存器整字内）⇒ §11.4.7 原事件模型（只认离散位）**一条都产不出来**。

| 路线 | 形态 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| F1 把消防状态位改配为 `func: discrete` 块 | 用 FC02 读 addr 4/6/7/8/9/12 | 直接复用既有 `BitClass::Alarm` 通路 | **物理上不可行**：这些量在**保持寄存器**（FC03）里，FC02 读的是另一套离散输入空间（消防协议亦未提供对应位区） | 否 |
| F2 在展示层/Web 侧检测 | 由前端或 core-bin 旁路读 telemetry 后判跃迁 | southd 零改动 | **落点错**：事件必须进 `storage.events` + `AlertFeed` + SSE，且要与 §10.4 联锁融合 —— 该通道只由 southd 持有；展示层检测会**错过实时性**，也让 `fire` 行的"events"名不副实 | 否 |
| F2′ 不加语义，把整字当 16 个位逐位建信号 | 用离散位的 `Bit` 机制套整字 | 复用现有 `Bit` 通路、零新概念 | **语义噪声**：20+ 寄存器的每个位都成信号（含保留位、备电电量的 8 位、预留的 bit2）⇒ 事件量爆炸且**违背 PRD §9.7.6"预留位不产事件"**；还会把"0 = 离线"的极性位误判成"1 = 异常" | 否 |
| **F3（选定）信号层：`SignalSpec` + 统一 `EdgeTracker`** | `point_table` 登记**逐信号**的 `SignalPick`（`WordBit{mask}` / `WordEnum{active}`），**只登记产事件的信号**；scheduler 用**同一个** `EdgeTracker` 同时处理离散位与字级信号 | ① 语义显式且可逐条对表 PRD §9.5.4；② 与既有位点**共用**跃迁记忆与事件产出路径（"统一"而非"并存"）；③ 天然支持"预留位不产事件"（不入 mask）、"极性反转位不猜"（不入表）、"未登记 = 只落 telemetry"；④ 消防取**双向**事件，正好补上 §10.4"恢复需双方复位"缺的输入 | 需新增一个内部枚举与一张信号表（规模：系统状态 ~6 + 火警 4 + 3 组触发 + 探测器 2×n） | **选 F3**，落点见 §11.4.7.1 |

> **与 G-6 的关系**：F3 **不**触碰字节拆解（只用 `mask`/`active`），故与 §11.10 的"整字采集 + 展示层拆解"并存不冲突，边界见 §11.10 的三层表。

### 11.3 模块划分与文件结构

**依赖方向不变**（§10.8/§10 依赖声明）：`mupc-southd` 仍**不依赖** strategy-engine / storage；回传由 core-bin 的 `SouthSink` 回调注入；`mupc-southd` 仍以静态库形态依赖 `rs485-plugin` 与 `mupc-data-processing`。

| 文件 | 动作 | 职责 | 扩展点来源 |
|------|------|------|------------|
| `crates/data-processing/src/meter_regs.rs` | 修改 | `RegFormat` 增 2 变体 + `reg_width()`；新增 `WordOrder`/`RegDecode`；`decode_regs` 保留为薄包装 | §10 已定"复用 meter_regs 解码" |
| `crates/rs485-plugin/src/device.rs` | 修改 | `read_discrete_inputs_from` + `parse_bits_response` + `unpack_bits`（纯函数） | §10.8 "Rs485Device 按口内多从站需要扩展" |
| `crates/rs485-plugin/src/lib.rs` | 修改 | 导出 `unpack_bits` / `read_discrete_inputs_from` 相关符号（+ `pub use device_trait::Parity`，供 southd 具名使用） | 同上 |
| `crates/mupc-southd/src/config.rs` | 修改 | `Role::Pcs`、`RegFunc::Discrete`、`StationParity`、`RegBlockConf` 新字段、`PointConf`；`validate()` 扩展（§11.5） | §10.3 stations 配置段 |
| `crates/mupc-southd/src/points.rs` | **新增** | 块 → 点展开（`expand`/`footprint`）、命名规则、位解包适配、**字级信号的活跃判据（`SignalSpec` → `SignalState`）** | §10 未涉及（新能力）；信号表见 §11.4.7.1 |
| `crates/mupc-southd/src/point_table.rs` | **新增** | 点表登记注册表（§11.4.4）：`(role, addr) → {format, scale, offset, sym_src, kind, label, signals}`（探测器区为 **6 条组内模板**） | PRD §9.4.3 要求的"校验器内点表登记值常量表"；`signals` 承载消防事件源 |
| `crates/mupc-southd/src/mapper.rs` | 修改 | `telemetry_points` 重构（逐点）、`Battery` 按**点名** `soc` 查找、`Role::Pcs` 臂、SOC 域检查、消防登记数比对 | §10.5 role 映射 |
| `crates/mupc-southd/src/port_runtime.rs` | 修改 | `StationBus::read_discrete`；`Rs485PortBus` 实现（`bus_lock` + `spawn_blocking` 同构）；`MockBus` 位注入；**`Rs485PortBus::open` 的 `parity` 透传（v1.4 补）** | §10.2/§10.7 口级总线 |
| `crates/mupc-southd/src/scheduler.rs` | 修改 | `RegFunc::Discrete` 分发、位点变化沿过滤、`role_priority(Pcs)=1`、SOC 越界/消防登记数事件 | §10.2 口调度预算 |
| `crates/mupc-southd/src/station.rs` | 修改（可选） | 若把位变化沿记忆放 `Station`（本设计放 `PortRunner`，故**不改**） | — |
| `crates/mupc-core-bin/src/startup.rs` | 修改（小） | `SouthSink`：`is_event=true` 且非 offline/online 时把 **value 写进事件 message**（SOC 越界原始值落证）；事件 message 可选查 `point_table::label` 补中文名 | §10.6 事件落库 |
| `crates/mupc-core-bin/src/core_config.rs` | 修改（测试数据） | 既有 fixture 订正（§11.5.3） | §10.3 跨段校验 |
| `mupc/deploy/config/mupc_core_config.yaml`(+`.production`) | 修改 | `south_stations` 段换为 §9.4.1 的 6 站（PCS 站默认**保持注释**，待 Q-15 裁定） | §10.3 |

**不新增 crate、不新增独立设计文档**（项目 CLAUDE.md 文档原则）。

### 11.4 数据结构与接口定义

#### 11.4.1 配置结构（`mupc-southd::config`）

```rust
/// 站级校验位（YAML: none 缺省 / even / odd）——PRD §9.4.1 站级 `parity`（D-1 裁定 ① 的落地形态）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StationParity { #[default] None, Even, Odd }

/// 站类型角色（新增 Pcs；YAML 值 `pcs`）
pub enum Role { MeterGrid, MeterBatt, Battery, Hvac, Fire, Pcs }

/// 读功能码（新增 Discrete = FC02）
pub enum RegFunc { Holding, Input, Discrete }

// ── 32 位值的字序（YAML: hi_lo 缺省 / lo_hi）：**单一定义（v1.3 订正）** ──
// 类型**只在 `mupc-data-processing::meter_regs` 定义一次**（§11.4.2 —— 它与 `RegDecode` 同居，
// 解码原语与它的字序参数不可分离；该 crate 已依赖 serde，`RegFormat` 即同一先例）。
// 本 crate **复用**同一类型、不再重定义（v1.2 在 §11.4.1/§11.4.2 各定义一次 = §11.2.2 B3 的双枚举缺陷）：
pub use mupc_data_processing::meter_regs::WordOrder;

pub struct StationConf {
    // 既有字段全部保留（id/role/port/protocol/slave/baud_rate/interval_ms/regs）
    #[serde(default)] pub parity: StationParity,          // 新增
}

pub struct RegBlockConf {
    pub name: String,
    pub addr: u16,
    #[serde(default = "default_reg_func")]    pub func: RegFunc,
    #[serde(default = "default_reg_format")]  pub format: RegFormat,
    #[serde(default)]                         pub scale: f64,
    #[serde(default = "default_reg_count")]   pub count: u16,
    // ── S3b-2 新增：全部 #[serde(default)]，缺省 = 既有行为 ──
    #[serde(default, skip_serializing_if = "is_zero_f64")] pub offset: f64,        // G-2
    #[serde(default, skip_serializing_if = "is_false")]    pub byte_swap: bool,    // G-5
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub points: Vec<PointConf>, // G-4
    #[serde(default, skip_serializing_if = "is_false")]    pub read_slice: bool,   // PRD §9.4.2.4 块级字段（v1.7 补登），语义见 §11.5.2
}

/// 点级换算口径（PRD §9.4.2.4 点级字段表）
pub struct PointConf {
    pub at: u16,                                   // 块内寄存器/位偏移 + 1（32 位点填低地址寄存器序号）
    #[serde(default = "default_point_count")] pub count: u16,     // 缺省 1
    #[serde(default, skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub format: Option<RegFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub offset: Option<f64>,
    #[serde(default, skip_serializing_if = "is_default_word_order")] pub word_order: WordOrder,
}
```

**为什么点级用 `Option<T>` 而不是"缺省值语义"**：`scale`/`offset` 的"未声明"与"显式 0"是**两种不同事实**——前者须继承块级，后者是配置错误（`scale == 0` 拒，PRD §9.4.3）。用 `Option` 让"继承"与"显式 0"在类型上可区分，校验器才能既拒 `scale: 0` 又不误伤"继承块级 `scale: 0.01`"的点。

**`read_slice`（PRD 已正式补登，本设计只做落地）**：语义 = "本块是按**设备单次读上限**主动分片的结果，**仅**豁免 §11.5 的块落地极大性检查（第 15 条）"；缺省 `false`。字段定义、适用场景与四条"不得用于逃避合并"的约束以 **PRD §9.4.2.4 块级字段表**为准（v1.7 补登），落地口径见 §11.5.2。

**兼容性论证（"既有 `meter_grid` 写法不得破坏"）**：

| 既有写法 | 解析结果 | 运行行为 |
|----------|----------|----------|
| `{ name: p, addr: 0x1000, format: int32_scaled, scale: 0.01, count: 6 }` | 与改动前逐字段相同（新字段取缺省） | 同（mapper 按块名找 `p` → `decode_phase_block` → `decode_regs`） |
| 站级无 `parity` | `StationParity::None` | `Rs485PortBus::open` 传 `Parity::None`（与 `Config::default()` 一致） |
| 无 `points` 的块 | `points = []` | **非 grid 站的 metric 命名变化**（§11.2.1 已登记；grid 不受影响） |

#### 11.4.2 解码原语（`mupc-data-processing::meter_regs`）

```rust
pub enum RegFormat { Float32, Int32Scaled, Uint16, Int16 }   // 新增 2 变体

impl RegFormat {
    /// 单值占用的寄存器数（16 位 = 1，32 位 = 2）——"宽度"的唯一定义
    pub fn reg_width(self) -> usize { match self { Uint16 | Int16 => 1, Float32 | Int32Scaled => 2 } }
}

/// 32 位值的字序 —— **全项目唯一一处定义（v1.3 订正）**：`hi_lo` = 高字在低地址（缺省，
/// 与既有 `regs_to_u32_be` 一致）/ `lo_hi`（PCS 32 位电量）。serde 形态与同文件的
/// `RegFormat` 完全同构（`rename_all = "snake_case"` + 全链 `Serialize` 供配置回写）；
/// `mupc-southd::config` 以 `pub use` 复用（§11.4.1），**不得**在别处再定义一份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WordOrder { #[default] HiLo, LoHi }

/// 解码规格：格式 + 换算 + 字节序/字序（"通用解码原语"的载体）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegDecode {
    pub format: RegFormat,
    pub scale: f64,        // Float32 忽略（沿用既有语义：float32 不乘 scale）
    pub offset: f64,       // Float32 忽略（offset 是"整数零点平移"的语义，浮点原值无零点平移概念）
    pub word_order: WordOrder,   // 仅 32 位格式生效
    pub byte_swap: bool,         // 仅按寄存器生效（逐寄存器 u16::swap_bytes）
}
impl RegDecode {
    pub fn width(&self) -> usize { self.format.reg_width() }
    /// 解码 1 个值；寄存器数不足 → 0.0；非有限值 → 0.0（沿用既有守卫）
    pub fn decode(&self, regs: &[u16]) -> f64;
}
/// 既有入口保留（薄包装，语义不变：hi_lo + 无 swap + offset 0）
pub fn decode_regs(r: &[u16], format: RegFormat, scale: f64) -> f64;
```

**解码算法（唯一、有序）**：

```
1) 长度守卫：regs.len() < width() → 0.0
2) 字节序还原：对参与本次解码的每个寄存器做 swap_bytes()（byte_swap=true 时）
3) 位模式组装：
   - width == 1：v = regs[0]
     - Uint16 → raw_u = v as u64
     - Int16  → raw_i = v as i16 as i64
   - width == 2：依 word_order
     - HiLo（缺省）：u32 = (regs[0] << 16) | regs[1]        ← 与既有 regs_to_u32_be 逐位等价
     - LoHi         ：u32 = (regs[1] << 16) | regs[0]
4) 物理解算：
   - Float32      → f32::from_bits(u32) as f64（忽略 scale/offset）
   - Int32Scaled  → (u32 as i32) as f64 * scale + offset
   - Uint16/Int16 → raw as f64 * scale + offset
5) 非有限守卫 → 0.0
```

> **算法顺序的判据（PCS 场景）**：AC-2 的期望值**唯一地**钉住了顺序 —— `reg[1042]=0x6400, reg[1043]=0x0100`，`byte_swap=true`、`word_order=lo_hi` ⇒ 先逐寄存器 swap 得 `[0x0064, 0x0001]`，再按 `lo_hi` 拼得 `0x00010064 = 65636`，`×0.1 = 6563.6 kWh`（若**先拼后 swap** 会得不同值；若误用 `hi_lo` 得 655360.1，**必须不一致**）。故"先 swap 再拼字"不是风格选择，而是被验收用例固定的语义。

#### 11.4.3 点展开与命名（`mupc-southd::points`，新模块）

```rust
/// 一个可产出的遥测点（16 位/32 位标量 或 1 个位）
pub struct PointSpec {
    pub metric: String,      // 点位名（PRD §9.4.2.2 唯一命名规则）
    pub kind: PointKind,     // Scalar { offset: u16, decode: RegDecode } | Bit { offset: u16 }
}
pub enum PointKind { Scalar { offset: u16, decode: RegDecode }, Bit { offset: u16 } }

/// 块 → 完整点清单。**校验期与运行期调用同一函数**（防"校验通过但运行期展开不同"）
pub fn expand(block: &RegBlockConf) -> Result<Vec<PointSpec>, String>;

/// 某块点清单的寄存器占位（校验空洞/极大性用）：返回 [(起始偏移, 长度)]
pub fn footprint(block: &RegBlockConf) -> Result<Vec<(u16, u16)>, String>;

/// **字级信号**：把某块的整字读数按 `point_table::SignalSpec` 求值为"活跃/非活跃"（§11.4.7.1）。
/// 返回 (信号键全名 `<点名>@<key>`, 是否活跃)；`scheduler` 用它喂 `EdgeTracker`。
/// 注意：**只读整字、不改写遥测值**；无信号的块 → 空 Vec。
pub fn signals_of_block(role: Role, block: &RegBlockConf, regs: &[u16]) -> Vec<(String, bool)>;
```

**展开规则（与 PRD §9.4.2.1 第 4 条一一对应）**：

| 情形 | 产出 |
|------|------|
| `func: discrete` + 无 `points` | `count` 位各 1 点，`metric = <块名>_<k+1>`（k = 位偏移，0 起） |
| `func: discrete` + 有 `points` | 仅列出的位（`at` 与点级 `count` 展开），同上命名 |
| 标量块 + 无 `points` | 窗口内**每个值槽** 1 点：按 `format.reg_width()` 步进；`metric = <块名>_<序号>`，序号 = **低地址寄存器的块内偏移 + 1** |
| 标量块 + 有 `points` | 仅列出的点；点级 `format/scale/offset/word_order` 覆盖块级缺省（`Option::None` = 继承）；点级 `count: N` 在 16 位格式下产出连续 N 点（序号 `at, at+1, …`），**32 位格式必须 `count = 1`** |
| 点级 `name` | 覆盖位置命名；**仅允许 `count == 1`**（多值点无"命名序列"定义，PRD 未定义 → 取保守口径**配置期拒**，见 §11.5.2 设计补充） |

**护栏（v1.3 新增，落规则 19）——无 `points` 的块 `count` 必须是宽度的整数倍**：无 `points` 的标量块按 `format.reg_width()` 步进产点，若 `count % width != 0`（例如 32 位格式 `count: 3`），步进的最后一格**装不下一个完整值** ⇒ 该尾槽**静默不产点**（`3 / 2 = 1` 点，而非 2 点），配置者却看不出少了一个点。这条**不产生运行时错误、只产生"少产点"的静默失真**，正是 PRD §9.4.3 要防的形态，故在**配置期直接拒**（规则 19，§11.5.1）：`func != Discrete && points.is_empty() && count % format.reg_width() != 0 → Err`。`discrete` 块（`count` = 位数，无"宽度"概念）与**声明了 `points` 的块**（`count` 由规则 11 的"首尾锚定"约束，见 §11.5.1 #11）均不适用。

**位解包（`unpack_bits`，rs485-plugin 提供纯函数）**：

```rust
/// Modbus FC02 响应字节 → 位向量（PRD §9.7.4 公式，唯一）
/// bit k = bytes[k/8] 的第 (k % 8) 位（bit0 = LSB）；返回长度 = count
pub fn unpack_bits(bytes: &[u8], count: u16) -> Vec<bool>;
```
- 响应字节数 = `ceil(count / 8)`；`count` 非 8 倍数时**末字节高位为无关位，不得影响前 `count` 位**（AC-3 空调 31 位的断言点）。
- 示例（PRD §9.7.4 引用 BMS 协议原文）：连续 16 位 `1,1,0,1,1,1,0,0,…` → 首字节 `00111011B = 0x3B`；随后 `1,1,0,1,1,1,0,1` → `10111011B = 0xBB`。

#### 11.4.4 点表登记注册表（`mupc-southd::point_table`，新模块）

**为什么需要它**：PRD §9.4.3 明确要求"符号性声明一致性"的第 ② 条的可判定性**以设计落成"点表登记值常量表"为前提**（`(role, addr) → {format, scale, offset, 来源}`，由设计阶段按 §9.5 逐点转写）。

```rust
/// 符号性来源（PRD §9.5 前言的三分类）
pub enum SymSrc { Vendor,       // 厂方逐点明写（PCS、空调、ADL400 的 4 字节功率类/PF）
                  VendorTypo,   // 厂方标注 + 推断订正（BMS 的 UNIT→UINT）
                  Engineer }    // 工程判断（消防、ADL400 2 字节点）

/// 位点分类（决定是否产出告警事件，见 §11.7.2）
pub enum BitClass { Alarm, State, Reserved }

pub struct PointReg {
    pub role: Role,
    pub addr: u16,               // 绝对寄存器地址；discrete 块为**位地址**
    pub kind: RegPointKind,      // Scalar(RegFormat) | Bit(BitClass)
    pub scale: f64,
    pub offset: f64,
    pub sym_src: Option<SymSrc>, // Bit 行 / 无偏移点可为 None
    pub label: &'static str,     // 中文名（事件/展示/RC-1 核对清单用）
    pub signals: &'static [SignalSpec],  // v1.3 新增：该格子内的**字级信号**（消防专用，见 §11.4.7）
}

/// 字级信号：一个整字（或一位）上的**可跃迁量**，**入表即产事件**。仅消防站使用（其状态位/枚举
/// 全在保持寄存器整字内，无 `discrete` 块）—— 详见 §11.4.7.1 的事件模型与 §11.7.2 的信号清单。
///
/// **口径（避免"登记了却不产事件"的歧义）**：`signals` **只登记产事件的信号**；
/// 未登记的位/枚举**一律只落 telemetry**（等价于离散位块的 `State`/`Reserved` 语义）。
/// 故**不设 `class` 字段**（本轮无"登记为 State"的形态；将来若需要，再以新变体扩展）。
pub struct SignalSpec {
    pub key: &'static str,       // 信号键（事件名后缀），表内唯一
    pub pick: SignalPick,        // 活跃判据
}
pub enum SignalPick {
    /// 位图：整字 & `mask` ≠ 0 视为活跃。`mask` 应为**单一或一组同类位**，
    /// 不得跨位图混装（每 bit 一个独立 key，便于事件定位）。
    /// **极性反转位不建信号**（如消防探测器 bit15「1 = 在线 / 0 = 离线」）—— 本设计不为其造判据。
    WordBit { mask: u16 },
    /// 枚举：整字值 ∈ `active` 视为活跃（活跃值之间跃迁亦产事件：如 1 一级报警 → 2 二级火警）。
    WordEnum { active: &'static [(u16, &'static str)] },
}

pub const POINT_REGS: &[PointReg] = &[ /* 展开后 618 行（n=20），由 §9.5 逐点转写 */ ];
pub fn lookup(role: Role, addr: u16) -> Option<&'static PointReg>;
pub fn label(role: Role, metric: &str) -> Option<&'static str>;
/// 某 (role, addr) 上的信号（无信号 → 空切片）
pub fn signals_of(role: Role, addr: u16) -> &'static [SignalSpec];
```

> **探测器区的登记形态（v1.3 补充，防"618 行"与可变长度区矛盾）**：`fire_det` 区的行数**随现场登记数变化**（n=20 时 114 行；n=100 时 594 行），无法逐地址静态登记。故该区在 `POINT_REGS` 中登记为 **6 条"组内语义模板"**（`+0 地址` / `+1 状态` / `+2 数据 1` / `+3 CO` / `+4 VOC` / `+5 H2`），运行期按 `(addr − 17) % 6` 取模板；**信号挂在前两条模板上**（`+1 状态` 的 bit12/bit14）。因此 §11.4.4 末的断言口径改为：**"按 n=20 的参考配置展开后 == 618 行"**（而不是"静态数组 618 行"）。

**覆盖范围与强制口径（本设计的取舍，须评审确认）**：

| 用途 | 口径 |
|------|------|
| 覆盖 | **全 618 点**（按 n=20 参考配置展开；探测器区为模板 + 运行期展开）逐点转写（PRD §9.4.3 明文要求）——它同时是 RC-1 的**机读核对清单**与事件/展示的中文名来源 |
| **强制**（`Err`） | ① `format ∈ {uint16,int16}` 且 `offset ≠ 0`，而 `lookup(role, addr)` **命中一行**、但该行 `sym_src` 为空 → 拒<br>② `lookup` 命中且 `offset` 与登记值不等（**含"漏配 → 缺省 0 ≠ −40"**）→ 拒（PRD ② 明文，比较字段 = `offset`） |
| **不强制**（测试期） | `format` / `scale` 与登记值不一致**不拒** —— 这是刻意的：Q-4（PCS 地址基准）、Q-16（4000/4005 的 16/32 位）、Q-20（116/186 改 `int16`）三项**现场裁定会合法改变** `addr`/`format`/`scale`，若强校验会把现场裁定后的正确配置**拒在启动期**。这些字段的一致性改由 `tests/point_table_vs_reference_config.rs`（§11.4.4 末）与 RC-1 逐点比对保证 |
| **查不到行** | **不拒**（v1.3 裁定，取代 v1.2 的"① 无行 → 拒"） |

> **为什么「查不到行 → 不拒」（P0-2 的裁定理由，必须写清）**：**现场 RC-3 会合法改 `addr` 基准** —— PCS 3 区首点到底是 0 基还是 1000 基**文档自相矛盾**（PRD §9.5.2 / Q-4），现场实测后**回写 `addr` 就是本次裁定的产物**；此时 `POINT_REGS` 按 1000 基登记的键**自然全部失配**。若"无行即拒"，则**合法的现场校准配置会把站拒在启动期**（而且是 PCS 站，恰好是 Q-4 唯一要校准的站）——这是"把不确定性误判为错误"。故 v1.2 的 ① 「无行或 `sym_src` 空 → 拒」**删去"无行"半句**，只保留"**命中而行内 `sym_src` 为空** → 拒"（该形态是**表自身的转录错误**：一条 `offset ≠ 0` 的现行偏偏没登记来源），全表据此自洽（§11.5.1 #6 同步）。**代价与补偿**：查不到行时，`offset` 的**可追溯性由 ② 与 RC-1 逐点比对兜底**（登记为 §11.12.3 R-3）。

**漂移防护（表 vs 配置的双向核对，实现期必做）**：`tests/fixtures/south_stations_s3b2.yaml`（= PRD §9.4.1 的 6 站参考配置）与 `POINT_REGS` 逐行交叉核对：凡表内 `offset ≠ 0` 的行，配置展开后必须命中同名同值；凡配置中 `offset ≠ 0` 的点，表内必须命中。测试同时断言**展开后行数 == 618**（= PRD §9.8.3 的点数；探测器区模板按 n=20 展开，见上）。

#### 11.4.5 总线与帧层扩展

```rust
#[async_trait]
pub trait StationBus: Send + Sync {
    async fn read_holding (&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError>;
    async fn read_input   (&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError>;
    /// FC02 读离散输入：`count` = **位数**（非寄存器数）；返回长度 = count 的位向量
    async fn read_discrete(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<bool>, BusError>;
}
```
- `Rs485PortBus::read_discrete`：与既有两方法**同构**（先取 per-port `bus_lock` 强制口内串行 → `spawn_blocking` 内 `dev.read_discrete_inputs_from(slave, addr, count)`）。
- `rs485-plugin`：`read_discrete_inputs_from(slave, addr, count)` 复用 `build_read_frame(slave, 0x02, addr, count, crc)`；响应解析**不能**复用 `parse_regs_response`（它按 `byte_count / 2` 拆寄存器，**会丢掉非偶数字节的末字节**）→ 新增 `parse_bits_response(response, count) -> Result<Vec<bool>, Rs485Error>`，内部调 `unpack_bits`。
- `MockBus`：新增 `put_bits(slave, addr, Vec<bool>)` / `fail_bits_once(slave, addr)` / `bit_calls`（与 holding/input 三套独立键，FC03/04/02 是不同寄存器空间）。
- **`Rs485PortBus::open` 的 `parity` 透传（v1.4 补，落点见 §11.3 表）**：现有实现只透传 `baud_rate`/`device_addr`，其余走 `rs485_plugin::config::Config::default()` ⇒ **校验位恒 `Parity::None`**，站级 `parity` 会被**静默忽略**（配置写了 `even` 却按 `none` 通信 ⇒ 空调站全站通信失败，且现象是"offline"而非"配置错"）。本轮须补一行映射：`parity: match conf.parity { StationParity::None => device_trait::Parity::None, Even => Parity::Even, Odd => Parity::Odd }`（`device_trait::Parity` 由 `rs485-plugin` 的 `pub use` 具名引入，避免在本 crate 再依赖 `device-trait`）；`timeout_ms`/`crc_mode`/8N1/DE-RE 仍取 `Config::default()`（**本轮不改**）。该透传是 **D-1 空调校验位裁定（RC-6）的代码侧前置**：校验位一旦由现场裁定为 `even`，**只改配置即可生效、无需改代码**；测试须在 `port_runtime.rs` 的 `open` 失败路径用例族中**新增一条**（构造 `StationParity::Even` 的 `StationConf`，断言送进 `Config` 的 `parity` 为 `Parity::Even` —— 可借 `Rs485PortBus::open` 拆出的纯函数或对 `Config` 构造的可测接缝，避免触及真串口）。

#### 11.4.6 mapper 扩展（`mupc-southd::mapper`）

```rust
/// 遥测点样本（比 (String, f64) 多一个类别标志，供 scheduler 做变化沿过滤）
pub struct TelemetrySample { pub metric: String, pub value: f64, pub kind: SampleKind }
pub enum SampleKind { Scalar, Bit }   // v1.3：bool → 枚举（后续若加"字级信号"不破签名）

/// 全量点位（含 288 位）——逐点展开，读失败的块整块跳过
pub fn telemetry_points(role: Role, reads: &BlockReads) -> Vec<TelemetrySample>;

/// battery 的 SOC 域检查（PRD §9.6.3）：0 ≤ v ≤ 100 且有限 ⇒ Some(v)，否则 None
pub fn soc_in_domain(v: f64) -> bool;

/// battery 分支的 SOC 取值结果（v1.3 新增：把"底块读失败"等四种情形显式化，见下）
pub enum SocOutcome { Value(f64), NoSuchPoint, BlockFailed(String) }
pub fn battery_soc(reads: &BlockReads) -> SocOutcome;

/// 消防探测器登记数交叉校验（PRD §9.5.4"强制"）：返回 (读回的登记数, 配置容量) 不一致时的读回值
pub fn fire_detector_mismatch(role: Role, reads: &BlockReads) -> Option<f64>;

/// 消防探测器**地址升序**交叉校验（PRD §9.10 Q-9）：读回各探测器组 `+0` 寄存器（地址号），
/// 非严格升序或重复 → Some((首个违规组的序号, 该组地址值))。**Q-9 的设计落点。**
pub fn fire_detector_addr_order_violation(role: Role, reads: &BlockReads) -> Option<(usize, u16)>;

/// 钢瓶气压「是否配置」（PRD §9.7.6）：`ever_nonzero` = 本站生命周期内该点是否出现过非 0 值。
/// 从未非 0 ⇒ false（展示层标"未配置"而非 "0 kPa"），一旦出现过 ⇒ true（此后恒 0 按真实 0 展示）。
pub fn cylinder_pressure_configured(reads: &BlockReads, ever_nonzero: bool) -> bool;
```

- **按点名查找（本轮新语义）**：`Battery` 分支由"找 `b.name == "soc"` 的块"改为"**展开各块点清单，找 `metric == "soc"` 的点**"（PRD §9.4.3 的 `soc` 点契约）。解码用该点的 `RegDecode`。
- **Battery 分支的四种情形（v1.3 新增 —— 上轮评审指出"底块读失败"语义未写）**：`SocOutcome` 把语义钉死，**逐条实现、逐条测**：

  | 情形 | 判据 | `pkg.battery.soc` | `PollResult` | 说明 |
  |------|------|-------------------|--------------|------|
  | ① 底块读成功且含 `soc` 点 | 该块 `Ok(_)`，展开后 `metric == "soc"` | `Some(值)`（**域外则 `None`**，见下条） | `Data` | 正常路径 |
  | ② **底块读失败**（承载 `soc` 的那一块） | 该块 `Err(e)` | `None` | **`Failed(e)`** | **整轮无数据**：沿用 §10.7"任一块失败 = 整站本轮失败、无部分交付"的既有语义；退避/offline 记账同既有。**不得**退化为"只丢 SOC、其余照常"（那会造成"站在线但 SOC 静默缺失"） |
  | ③ 全站无任何块含 `soc` 点 | 展开后无 `metric == "soc"` | `None` | `Data`（空占位） | 防御性分支：**该形态在配置期已被规则 4 拒**（§11.5.1），此处的 `Data` 只为"调度器单测可绕过 validate"而保留，**不是**允许的配置形态 |
  | ④ 非承载 `soc` 的其它块读失败 | 任一其它块 `Err(e)` | `None` | **`Failed(e)`** | 与 ② 同：整站失败（§10.7），避免"半个站" |

  > ②③④ 的差别是刻意的：**②④ 属"通信/链路"故障 ⇒ 整站失败（可退避自愈）**；**③ 属"配置"错误 ⇒ 配置期拒**。二者混为一谈会让"配置错"在运行期表现为"站离线"（PRD §9.7.2 第 5 条明确禁止：配置错误一律由配置期校验拦截，不在运行期兜底）。
- **`Role::Pcs`**：与 `MeterBatt/Hvac/Fire` 同臂 → `empty_package()`（"站活着"信号）；其全部点只走 `telemetry_points`/`on_station_telemetry`，**不触发** `on_battery_soc`（N-1）与 `on_grid_package`（N-2）——这两条由 scheduler 的 role 判断**结构性保证**（只有 `Role::Battery` 才调 `on_battery_soc`；只有 `Role::MeterGrid` 才调 `on_grid_package`）。
- **SOC 域检查落点**：`Battery` 分支解出 `soc` 后，**域外则把 `pkg.battery.soc` 置 `None`**（控制链拿不到坏值），而 `telemetry_points` 仍按**原值**产出 `soc`（`§9.6.3` ③"telemetry 保留证据"）。越界的**告警事件**由 scheduler 发（§11.4.7）。
- **消防登记数**：`fire_detector_mismatch` 读"点名 `fire_det_count` 的点"（PRD §9.4.1 参考配置已在 `fire_sys` 的 `at: 7`（寄存器 10）上声明 `name: fire_det_count`，v1.7 定名），容量 = 全部以 `fire_det` 为前缀的块 `count` 之和 / 6（单块即退化为此块 count/6）。两者不等 → 返回读回值。
- **消防地址升序（Q-9）**：`fire_detector_addr_order_violation` 逐组读 `+0`（地址号），**必须严格升序**（PRD §9.5.4"探测器按地址号从小到大顺序排列"）；违规 → 产事件（§11.4.7 ④），并把该轮探测器区的点**标记为不可信**（telemetry 仍落原值，判据层拒用）——含义：**"第 n 只"只能等于"地址升序的第 n 只"**，顺序异常时点位与物理探测器**不再一一对应**。

#### 11.4.7 scheduler 扩展（`mupc-southd::scheduler`）

| 改动 | 内容 |
|------|------|
| 块读分发 | `match blk.func { Holding => read_holding, Input => read_input, Discrete => read_discrete }`（`read_discrete` 的 `BlockReads` 需要承载 `Vec<bool>`：`BlockReads` 元素类型扩为 `BlockData::Regs(Vec<u16>) \| BlockData::Bits(Vec<bool>)`，**或**引入平行的 `BitReads` 通道——本设计选**前者**：`BlockReads = Vec<(RegBlockConf, Result<BlockData, String>)>`，避免两条 mapper 入口） |
| 变化沿过滤 | `PortRunner` 增 `edge_tracker: Mutex<HashMap<usize /*站下标*/, EdgeTracker>>`（**v1.3 把 `BitEdgeTracker` 推广为 `EdgeTracker`**，同时承载离散位与字级信号，见下"统一事件模型"）；每轮取"与上轮不同的位/信号 + 首次/复位后全量"；**站从 offline 恢复时先 `reset()`**（恢复后现势值连续性不可假设，须重发一次全量快照） |
| 事件产出 | ① `Battery` 且 SOC 越界 → `on_station_telemetry(id, role, vec![("soc_out_of_range", 原始值, true)])`（**不调** `on_battery_soc`）② 位点 `BitClass::Alarm` 的 **0→1** 跳变 → `(点位名, 1.0, true)` ③ 消防登记数不一致 → `("fire_detector_count_mismatch", 读回值, true)` ④ **消防字级信号**（系统状态位 / 火警状态枚举 / 探测器总状态位）进入与退出活跃 → `("<点名>@<信号键>", 1.0/0.0, true)` ⑤ **消防探测器地址升序违规**（Q-9）→ `("fire_detector_addr_order_invalid", 违规组序号, true)` ⑥ 消防钢瓶气压"未配置"**不产事件**（仅展示层标注，PRD §9.7.6）⑦ **PCS 32 位电量比对未通过**（PRD §9.8.4）：**本设计不产该事件** —— 该要求由 RC-2「未通过则不配 4 个 32 位电量点」替代（§11.7.4），**处置与理由见 §11.12.2 Δ-9（须需求侧追认）**；故事件表**不含**此项，实现时**不得**自行造判据（PRD §9.7.2 第 2 条禁本轮做量程自校验） |
| role 优先级 | `role_priority`：`MeterGrid\|Battery => 0`、**`Pcs\|MeterBatt => 1`**、`Hvac\|Fire => 2`（PRD §9.3.2.3） |
| 闸门不变 | 只有 `MeterGrid` 的 `on_grid_package` 推进 AiIntegrator 闸门；`Pcs`/`Battery`/`MeterBatt`/`Hvac`/`Fire` 一律不推进（§10.5 语义**零改动**） |

#### 11.4.7.1 统一事件模型（v1.3 新增 —— P0-3 的落点）

**问题（上轮评审 P0-3）**：PRD §9.6.1 要求 `fire` 的「**系统状态位 / 探测器 / 火警状态**」进 `events` + SSE，但 §11.4.7 的事件模型只认**离散位点**（`func: discrete` 块的 `BitClass::Alarm`）—— 而**消防全部状态量都在保持寄存器整字里**（`fire_sys`：addr 4 系统状态位图、addr 6/7/8 烟/温/可燃状态位图、addr 9 火警状态枚举；探测器 addr 12 状态位图），**本站没有任何 `discrete` 块** ⇒ 消防一个事件都产不出来，§11.7.1 的 `fire` 行「events」与 §11.8「维持 §10.4 融合」**缺一半输入**。

**解法：把"事件源"从"离散位"推广为"信号（Signal）"** —— 四类信号共用**同一套跃迁记忆与同一条事件产出路径**，唯一的差别是**活跃判据函数**：

| 信号形态 | 载体 | 取值 | 活跃判据 | 事件产出 |
|----------|------|------|----------|----------|
| `Bit`（**既有**：BMS 288 位、空调 31 位） | `func: discrete` 块 | 位向量第 k 位 | 位 == 1 | 仅 **0→1 上升沿**（`BitClass::Alarm` 行；`State`/`Reserved` 只落 telemetry） |
| `WordBit`（**新增**：消防系统状态 / 烟温可燃 / 探测器总状态） | 保持寄存器**整字** | 整字值 | `字 & mask ≠ 0`（mask 取自 `point_table::signals`） | **进入/退出活跃均**产事件（见下"为何双向"）。**入表即产事件**，未入表的位一律只落 telemetry |
| `WordEnum`（**新增**：消防火警状态 addr 9） | 保持寄存器**整字** | 整字值 | 值 ∈ `active` 集合 | 同上；**活跃值之间跃迁亦产事件**（1 一级报警 → 2 二级火警） |
| `WordScalar`（阈值型模拟量） | — | — | — | **本轮不引入**（PRD 明令不做通用量程自校验；消防无模拟量消费方 —— 这同时是 G-6 延后的保护边界，见 §11.10） |

**统一机制（同一 `EdgeTracker`）**：每个信号（含每个离散位）在 `EdgeTracker` 里占一格"上轮活跃态"，每轮算出 `(上轮, 本轮)` 二元组：
- `false → true`：**进入活跃** ⇒ 产事件（`value = 1.0`）；
- `true → false`：**退出活跃** ⇒ 产事件（`value = 0.0`）；
- 其余（含首次采样的"未知 → X"）：**不产事件**（首轮只建立基线；站恢复后 `reset()` 同理，避免"恢复即刷一屏事件"）。

```rust
/// 某站的变化沿记忆（scheduler 内，按站下标索引）
pub struct EdgeTracker { last: HashMap<String /*信号全名或点位名*/, bool>, primed: bool }
impl EdgeTracker {
    /// 首轮/站恢复时调用：清空记忆并把本轮的活跃态记为基线（不产事件）
    pub fn prime(&mut self, now: &[(String, bool)]);
    /// 产事件：返回 (metric, value, is_event=true) —— 含 进入(1.0)/退出(0.0) 两类跳变
    pub fn edges(&mut self, now: &[(String, bool)]) -> Vec<(String, f64, bool)>;
}
```

> **双向/单向的落地形态（v1.4 订正 —— 删除无定义的 `EmitBidi`/`emit_falling_edges`）**：v1.3 曾在上面列出一个 `pub fn emit_falling_edges(class: EmitBidi) -> bool;`，但 **`EmitBidi` 在本设计中从未定义**（悬空符号）。本版**删去该行**：双向性是**信号形态自身的属性**，不需要额外开关 —— `WordBit`/`WordEnum`（消防字级信号）进入与退出**都产事件**；`Bit`（离散位块）**仅 0→1 上升沿**（`BitClass::Alarm` 行）。该差别落在 `scheduler` 调 `EdgeTracker::edges()` 后的**一次过滤**上（离散位只取 `false→true` 的项），**不是**一个公开 API，也不引入新类型。若将来需要开关，再按"新变体扩展"（与 `SignalSpec` 同一取向）。

事件名（**事件命名空间，非遥测命名空间**）：`<遥测点名>@<信号键>`。例：`fire_sys_1@main_power_fault`（系统状态 bit14 主电故障）、`fire_sys_6@level2`（火警状态 = 2 二级火警）、`fire_sys_8@alarm`（探测器 1 报警总状态）、`fire_det_2@fault`（探测器 2 故障总状态）。事件最终进 `storage.events` 的键仍是既有的 `south_station.<站id>.<metric>`，`metric` 即上述带 `@` 的名字（`SouthSink` 侧查 `point_table::label` 补中文名，查不到用原名）。

**为什么消防取"双向"，而 BMS/空调位块仍只取上升沿（不对称是刻意的，须登记）**：
1. 消防是**联锁判据源**（PRD §9.6.1：`fire` 行"是（仅事件/联锁判据，非控制量）"）；设计 §10.4 的融合规则含「**恢复需双方复位**」—— 联锁要能收敛，就必须知道**消防侧已解除**；只有上升沿会让"消防侧复位"这一事实**永远不可观测**。
2. BMS 288 位 / 空调 31 位的告警位**没有停机或联锁消费方**（PRD §9.6.1：BMS 告警位不触发停机），下降沿无消费方；对 288 位产双向事件只会制造事件噪声（去重键与保留策略压力）。
3. 消防信号总数受登记数约束、跃迁稀疏；BMS 位块相反（288 位/轮）。
> 该不对称登记为设计决策（§11.12.1 项 6），待评审追认。若评审要求统一，退路是"全部双向 + 事件节流"，代价见 §11.12.1。

**消防信号清单（哪些产事件、哪些只落 telemetry）**：

| 寄存器 | 点名 | 登记的信号键（**入表 = 产事件**） | 形态 | 依据 |
|--------|------|-----------------------------------|------|------|
| 4 系统状态 | `fire_sys_1` | `main_power_fault`（bit14）/ `backup_power_fault`（bit13）/ `drive_circuit_fault`（bit11）/ `pressure_sensor_fault`（bit10） | `WordBit{mask: 1<<n}` | PRD §9.5.4 位定义表；均为「1 = 故障」 |
| 4 | `fire_sys_1` | `spray_fired`（bit8，1 = 已喷）/ `valve_open`（bit9，1 = 开启） | `WordBit{mask: 1<<n}` | 灭火动作必须留痕（PRD 明确要求列举"喷洒标记"）；语义由 `label` 文案区分为"动作"而非"故障" |
| 6 / 7 / 8 | `fire_sys_3/4/5` | `smoke_trigger` / `temp_trigger` / `combustible_trigger` | `WordBit{mask: 0b11}` | bit1 复合探测器触发、bit0 干接点触发；**bit2（点型，预留未启用）不纳入 mask** —— PRD §9.7.6 明文"采集成点但不产出告警事件" |
| 9 火警状态 | `fire_sys_6` | `level1`（值 1）/ `level2`（值 2）/ `emg_start`（值 4）/ `emg_stop`（值 5） | `WordEnum{active: …}` | 值 3 预留（不纳入 `active`）；值 0 工作正常 = 非活跃（`active` 只列 1/2/4/5，故"任何活跃值 → 0"即退出活跃） |
| 12 探测器状态 | `fire_sys_9`（探测器 1）/ `fire_det_<序号>`（探测器 2..n） | `alarm`(bit12 报警总状态) / `fault`(bit14 故障总状态) | `WordBit{mask: 1<<n}` | PRD §9.6.1 明确要求"**探测器**"进 events；**探测器级只取这两条总状态**，bit0–4 的传感器细分**不产独立事件**（取舍：n≤100 ⇒ 逐传感器产事件会让事件源数 ×5，且告警类别可从该探测器原始整字回溯；登记为设计取舍 §11.12.1 项 6） |

**不登记信号（只落 telemetry，与上表同等重要 —— 防止"顺手多产"）**：

| 寄存器 / 位 | 为什么不登记 |
|-------------|--------------|
| 系统状态 bit15 工作模式 / bit12 充电状态 / bit7–0 备电电量 | 普通状态量与连续量；备电量是"值"不是"跃迁"（§11.7.2 第 7 条） |
| 探测器状态 bit15 通信状态（**0 = 离线，与其余位极性相反**）/ bit13 电磁阀 / bit10–11 反馈输入 | **极性反转位与反馈位无明确告警语义 ⇒ 本设计不猜、不造判据**（`SignalPick::WordBit` 的注释已把该口径写成约束） |
| 探测器状态 bit0–4（烟雾/温度/CO/H2/VOC 报警与传感器故障）| 逐传感器事件会让事件源数 ×5（n≤100），且 bit12 报警总状态已覆盖"该探测器报警"；细分可从整字原值回溯 |
| 5 钢瓶气压（整字） | 恒 0 不得判"气压异常"（PRD §9.7.6）；展示层按"未配置"呈现（§11.7.3），**永不产事件** |
| 11 探测器地址 / 13 数据 1 / 14–16 CO·VOC·H2 | 数据 1 的字节拆解属 G-6 展示层（§11.10），**事件层不触碰**；CO/VOC/H2 无阈值口径（文档未给），不得自行造判据 |

**事件检测**发生在**哪一层**：**`mupc-southd` 侧的 scheduler/tracker 层**（南向的"事件层"），**不**在下游展示层 —— 因为事件只能经 `on_station_telemetry(is_event=true)` 进 `storage.events` + `AlertFeed`，而**该通道只由 southd 持有**；若把检测放到 Web/展示层，火警将无法及时进 events/SSE，也接不上 §10.4 的联锁融合。

**与 §11.10 的 G-6 口径是否冲突：不冲突，边界如下（三层职责互不重叠）**：
1. **telemetry 值**：消防状态量一律**整字原值**落库（G-6 延后，`fire_sys_1/6/9` 等点的 `value` 就是整字 0–65535）——**不变**；
2. **事件层**：只做 **`字 & mask` / `字 ∈ 枚举集`** 的**位/枚举判定**（不涉跨字节组合的物理量、**不修改 telemetry 值**）—— 这是 PRD §9.7.6 明文授权的范围（"展示/**事件层**按字节语义拆解"）；
3. **展示层**：仍负责 §11.10 的**字节拆解**（唯一涉及跨字节的是"探测器数据 1"：高字节烟雾/低字节温度）—— 该点 **不产事件**（无模拟量消费方，PRD D-2 重启条件 ②），故与事件层**零重叠**。

> 一句话：**G-6 延后的是"把整字拆成两个遥测点"；消防事件用的是位/枚举语义 —— 二者不是同一件事，也不共用同一份拆解代码**（事件层只有 `mask/active` 常量，没有"字节切片"逻辑）。


### 11.5 配置期校验（PRD §9.4.3 十七条逐条落点 + 2 条补落点）

#### 11.5.1 落点表

| # | PRD 规则 | 落点函数 | 判定依据 | 说明 |
|---|----------|----------|----------|------|
| 1 | 新 role 合法性 | serde（`Role`/`RegFunc`/`RegFormat`/`WordOrder`/`StationParity` 枚举） | 未知 YAML 取值 → 反序列化 `Err`（启动期 config load 即失败） | 无需新代码，**补单测**（未知 role 字符串 → Err） |
| 2 | 单站约束 | `SouthStationsConfig::validate` | `Role::Pcs` 计数 > 1 → Err | 与既有 `meter_grid`/`battery` 单站约束同形态 |
| 3 | 必填点表 | 同上 | `role == Pcs && regs.is_empty()` → Err | 空 regs = 站永久 offline 的静默死配 |
| 4 | `soc` 点契约 | `validate_station_regs` | `role == Battery` 且 `points::expand()` 展开后**无 `metric == "soc"`** → Err | 消费方按点名查找，缺名即静默不推 SOC |
| 5 | 格式与标度 | `validate_station_regs` | `int32_scaled/int16/uint16` 的**块级或点级** `scale == 0`（点级 `None` = 继承块级，只判一次）→ Err；`discrete` 块**不适用** | 防"raw×0 整块解 0" |
| 6 | 符号性一致性 | `validate_symbolicity` | ① `format∈{u16,i16} && offset≠0` 且 `lookup(role,addr)` **命中一行**、而该行 `sym_src` 为空 → Err（**v1.3 删去 v1.2 的"无行"半句**，理由见 §11.4.4）② `lookup` 命中且 `offset` 与登记值不等（含漏配 → 0）→ Err | 期望值来自 §11.4.4 的 `POINT_REGS`；`format`/`scale` 不强制（§11.4.4 口径）；**查不到行 → 放行** |
| 7 | 点位越界 | `points::expand` | 点覆盖 `[at−1, at−1+width)` 超出 `[0, count)` → Err | 返回 Err 含块名/点名/`at` |
| 8 | 点位重叠 | `points::expand` | 同块内两点覆盖同一寄存器/位 → Err | 32 位点占 2 寄存器，与相邻 16 位点重叠也算 |
| 9 | 32 位点对齐 | `points::expand` | `width == 2 && at − 1 + 2 > count` → Err | "半个 32 位值" |
| 10 | 点名唯一 | `validate_station_regs` | 展开后站内 `metric` 去重（含自动点名与显式 `name` 相撞）→ Err | 遥测键冲突 |
| 11 | 空洞上限 | `validate_station_regs` + `footprint` | 声明了 `points` 的标量块**窗口首尾以声明点锚定**：首个声明寄存器必须落在块内偏移 **0**（不得含前导未声明寄存器），最后一个声明寄存器**末端必须恰好等于 `count`**（末尾未声明寄存器不计入 `count`）；块内未声明寄存器的**连续空洞 > 4** → Err。`discrete` 块**不受此限** | 空洞判据来自 PRD §9.4.2.1 第 3 条（空读 ≤ 一次请求帧 8 字节） |
| 12 | 位块上限 | `validate_station_regs` | `func == Discrete && count > 2000` → Err | BMS ≤ 2000 位；`count == 0` 亦拒 |
| 13 | 地址有效性 | `validate_station_regs` | `addr == 0` **仅允许** `role ∈ {MeterBatt, Hvac}`；其余 role（含既有 `MeterGrid`、新增 `Battery`/`Pcs`/`Fire`）要求 `addr > 0` | 既有 `MeterGrid` 的 `addr > 0` 检查**被吸收进**本规则（行为不变） |
| 14 | 区间与重叠 | `validate_station_regs` | 同站**按功能码空间**（holding / input / discrete 三套地址空间）分别判半开区间重叠 → Err | 覆盖既有"仅 meter_grid 做重叠检查"的形态（PCS 3 区/4 区同址不同 func 由此天然放行） |
| **15** | **块落地极大性（v1.3 重写）** | `validate_maximality` | **适用域（先行过滤）**：**仅当被考察的两个块都声明了 `points`（非空点清单）、且均非 `discrete` 块时才参与判定**；任一块未声明 `points` → **跳过、不判**（既有 `meter_grid` 六相量块、`bms_alarm`/`hvac_di` 两个位块均属此列）。<br>**判据（同时满足才拒）**：两块 `func` 相同、`byte_swap` 相同、地址**严格相邻**（`b.addr == a.addr + b.count` 即 `b.addr == a.addr + a.count`）、**合并后 `count = a.count + b.count ≤ MAX_SINGLE_READ_REGS (120)`**、**合并窗口内连续空洞 ≤ 4**、且**两块均未标 `read_slice: true`** → **Err**（提示合并）。<br>**豁免**：相邻块中**任一块**标 `read_slice: true` → 不拒（PRD §9.4.2.4 v1.7 补登；四条"不得用于逃避合并"的边界以 PRD 为准） | **重写要点**：① **限定适用域**（只作用于声明了 `points` 的标量块）—— 否则会拒掉既有 `grid_meter`（六块地址严格相邻）⇒ 现场启动 fail-fast；② **判据与 PRD v1.8 逐条对齐**：`count ≤ 120`（④）+ 空洞 ≤ 4（③）**都实现**，其中 **③ 在适用域内由规则 11 的首尾锚定恒成立**（证明见 §11.5.2(1) 表后"③ 的等价性"），故实现 ③ 只是与 PRD 逐字对齐、不引入额外拒绝。**逐站证明见 §11.5.2(2)** |
| **18** | **`pcs` 站周期下界**（v1.3 补落点；PRD §9.3.2.2(2) + §9.8.1 末条） | `SouthStationsConfig::validate`（站级基础校验，**与既有 `meter_grid`/`battery` 的 `< 5000` 拦截同址**） | `role == Pcs && interval_ms < 500` → Err | 防"误配的超短周期打满总线"；`pcs` **无** `< 5000` 上界约束（不参与控制决策，PRD §9.3.2.2(2)）。**该条不在 PRD §9.4.3 表内，由 PRD 另两条明文要求**（§9.3.2.2(2)、§9.8.1 末条）——AC-1 ③ 一并断言（§11.11.2） |
| **19** | **无 `points` 块的宽度护栏**（v1.3 补落点；设计补充） | `validate_station_regs` | `func != Discrete && points.is_empty() && count % format.reg_width() != 0` → Err | 防"32 位格式的尾槽静默不产点"（如 `count: 3` 只产 1 点、少 1 点而无人知）。`discrete` 块与声明了 `points` 的块不适用（理由见 §11.4.3 护栏段）。登记为设计补充（§11.12.1 项 3b） |
| 16 | 同口一致性 | `validate_port_consistency` | 同 `port` 各站 `baud_rate` **与 `parity`** 必须一致 → Err | 既有 baud 循环扩展 parity（缺省 `none` 参与比较：空调站配 `even` 而同口另站未写 → Err，防静默忽略） |
| 17 | 跨段互斥 | `core_config::validate_south_stations` | `port` 与 `intercore.modbus_rtu.serial_port` 同节点 → Err | **既有实现，沿用，零改动** |

**顺序（v1.4 钉住，含一条保住既有断言的实现约束）**：`validate()` 内先做站级基础校验（id/port/slave/interval/baud，**含规则 18 的 `pcs` 下界**）→ 再 `validate_station_regs`（含展开，**含规则 10/13/14/19**）→ 最后跨站（单站约束计数、同口一致性、极大性）。**逐站短路返回首个 Err**（沿用既有风格：错误消息即定位信息，含站 id / 块名 / 点名 / 期望值）。

> **⚠️ 实现约束（必须遵守，理由见 §11.5.3.4.1）**：既有的 **`meter_grid` 站内完整性校验整组**（缺相量块 `p/q/pf/u/i` / `int32_scaled` 块 `scale > 0` / 相量块 `count ≥ 6` / `p_total count ≥ 2` / **块名唯一** / `addr > 0` / 半开区间不重叠）**保持在 `validate_station_regs` 之前、原地不动**（即仍在逐站循环的"站级基础校验"阶段），**不得后移、不得被通用规则 10/13/14/19 取代**。原因：这组校验对同一份坏配置会给出**与通用规则不同但更具体**的文案（`块名重复` / `addr 不能为 0` / `寄存器区间重叠` / `count 须 ≥ 6` / `须显式 scale>0`），而 §11.5.3.4 **C2**（S3b-1c 校验语义的回归锚）逐条断言了这些文案；顺序一换，C2 即失配（其中"两块同名 `p`"这一例**新规则 10 同样成立**，最易被误判为"文案可改"）。

> **规则 18/19 的编号说明**：PRD §9.4.3 的表是 **17 条**；18/19 是设计侧补的两条落点（18 由 PRD §9.3.2.2(2)+§9.8.1 明文要求但**未进 §9.4.3 表**；19 为设计补充的护栏）。二者均上报需求侧（§11.12.2），**AC-1 ③ 一并断言**但标注"非 §9.4.3 表内条件"。

#### 11.5.2 第 15 条的完整口径与「§9.4.1 六站必然通过」的证明（P0-1）

**（1）第 15 条的完整口径（v1.3 按 PRD v1.8 逐条对齐重写）**

| 要素 | v1.2（旧） | **v1.3（本版）** | 为什么必须改 |
|------|-----------|------------------|--------------|
| **适用域** | 无（对所有块生效） | **仅当被考察的两个块都声明了 `points`（非空点清单）、且均非 `discrete` 块时才参与判定**；不满足者（未声明 `points` / 位块）**一律不参与**极大性合并判定 | 旧口径会把 `grid_meter` 的 6 个相量块（地址严格相邻）判成"可合并"→ `Err` → **现场启动 fail-fast**，与 PRD §9.4.1 第 6 站"逐字不动"、§11.2.1 的兼容性承诺、以及 T3/T7（v1.2 口径）的"既有 20 例零破坏"互相矛盾（v1.3 已把 T3/T7 与回归闸门的措辞一并订正，见 §11.13） |
| **判据** | 合并后**空洞 ≤ 4 寄存器**（PRD v1.6 的表述，只有这一条） | **PRD v1.8 的四条全实现**：①`func`/`byte_swap` 相同 ②地址连续 ③合并后空洞 ≤ 4 ④**合并后 `count ≤ MAX_SINGLE_READ_REGS (120)`**。其中 **③ 在适用域内恒成立**（证明见下），**④ 是真正起作用的新判据** | v1.6 少了 ④ ⇒ 仅凭"空洞 0"就会把 `fire_sys`(13)+`fire_det`(114) 判成"可合并"→ `Err`，而合并体 127 寄存器**本就超过设备单次读上限**（PRD §9.5.4 要求 ≤120 分片）⇒ 结论方向反了。补 ④（"合并后能否一次读回"）后，`fire` 这类"本就该分片"的形态**不会被要求合并** |
| **豁免** | 块级 `read_slice`（v1.2 设计自创，PRD 未列） | 相邻块中**任一块**标 **`read_slice: true`** → 不拒（**PRD §9.4.2.4 已正式补登**，v1.7；四条边界以 PRD 为准）；**且 `read_slice` 只在适用域内有对象**（未声明 `points` 的块本就不参与，标它是冗余、不构成错误配置 —— PRD v1.8 明文） | 现场按实测上限分片是**合法配置**，不能误拒；但豁免必须**显式、可审计**（注释登记理由，属 RC-1/RC-5 目视项），**不得**按 `role` 隐式特判（违反 G-5） |

`MAX_SINGLE_READ_REGS = 120`：取 **PRD 自己的分片口径**（BMS ≤120 寄存器、消防每片 ≤120 寄存器）——比 Modbus 标准上限 125 留 5 寄存器余量，是"保守的设备单次读上限"。也正因如此，**`pcs_3zone`（76 寄存器）与 `fire_det`（114 寄存器）自身单块 ≤120**，不需要 `read_slice`；只有当它们在现场被拆成多片时，才由配置者给分片块标 `read_slice: true`。

> **为什么必须"两块都声明 `points`"才判（强化理由，不只是"legacy 豁免"）**：未声明 `points` 的块，其点清单由**窗口宽度隐式决定**（每值槽 1 点），合并会同时改变**点的数量与默认点位名** —— 例如把 `fire_det` 并入 `fire_sys` 后，原 `fire_det_1` 会变成 `fire_sys_17`，而 PRD §9.4.2.2 明确"**改名 = 破坏历史数据可比性**、须按变更流程登记与评审"。而 `grid_meter` 的块名 `p/q/pf/u/i/p_total` **本身就是 mapper 的查找键**（PRD §9.4.2.4 明列为契约），合并它们等于**销毁契约**。因此对"含未声明 `points` 块"的相邻对做合并提示，是把**改名/契约破坏风险**伪装成"碎块风险"，**收益为负**。

> **③（合并后空洞 ≤ 4）在适用域内恒成立 —— 等价性证明（v1.3，与 PRD v1.8 逐条对齐的关键）**：PRD v1.8 的拒绝条件是 ① `func`/`byte_swap` 相同、② 地址连续、③ 合并窗口内连续空洞 ≤ 4、④ 合并后 `count ≤ 120`。但在**适用域**（两块**都声明了 `points` 的标量块**）内，③ **必然成立**，理由：
> - 由 §11.5.1 **规则 11 的首尾锚定**，任一 `points` 块"**首个声明寄存器落在块内偏移 0**、**最后一个声明寄存器的末端恰好等于 `count`**" ⇒ A 块的**最后一个寄存器必被声明**、B 块的**第一个寄存器（= A 的末寄存器 +1）必被声明** ⇒ **接缝处空洞 = 0**；
> - 合并窗口内的连续空洞**只能来自 A、B 各块的内部空洞**（接缝已无空洞），而各块内部空洞**已被规则 11 单独限制为 ≤ 4**（否则该块自身早被拒）⇒ 合并后的每个连续空洞 ≤ 4 ⇒ **③ 成立**。
>
> **结论**：实现 ③（与 PRD 逐字对齐）**不引入额外拒绝**；同时 ④ 是本条真正起作用的新判据（"合并后能否被设备一次读回"）。**故本设计的判据与 PRD v1.8 行为等价**，且"六站必然通过"的结论不受 ③ 影响。实现顺序建议：先判 ④（`count > 120` → **不拒**，直接返回），再判 ③，最后判 ① ② 与 `read_slice` —— 这样 `fire` 的 `fire_sys+fire_det`（若未来 `fire_det` 也声明 `points`）会被 ④ 提前放行，**不会**因错误顺序被 ③ 之外的条件误拒。（注：`fire_det` 当前**未声明 `points`**，故 `fire` 站在适用域外，与实现顺序无关。）
>
> **一处需求文本的两种读法 —— 本设计取更保守者，并说明其本轮不可达（须评审备案）**：PRD v1.8 的本条正文以"**未声明 `points` 的块**（…亦含 `discrete` 位块）不参与"为例，但同条 ② / ③ 又出现"`discrete` 按位地址同理""`discrete` 位块不受 ③ 限制" —— 字面上可读成"**声明了 `points` 的 `discrete` 位块仍参与本条**"。**本设计取保守读法：`discrete` 块一律不参与**（与"本体不改变既有行为"的立意一致，也避免为位块引入"合并后位数折算成寄存器数"的口径）。**该分歧在本轮不可达**：§9.4.1 的两个位块（`bms_alarm` 288 位、`hvac_di` 31 位）**都未声明 `points`** ⇒ 两种读法对本轮配置**判定结果完全相同**。登记为需求侧备案项（§11.12.2 Δ-8）。

**（2）§9.4.1 参考配置（6 站）必然通过第 15 条 —— 逐站证明**

先枚举"同 `func` 且地址严格相邻（`b.addr == a.addr + a.count`）"的块对，再按适用域过滤。下表每一格的 `addr`/`count` **逐字取自 PRD §9.4.1 的 YAML**（可逐行复核，无需推断）。

> **取值真源与「不得取证于单测 fixture」的口径（v1.4 订正 —— 二轮评审判定 §11.5.2(2) 的 `grid_meter` 行事实错误）**：
> - **生效配置的真值（唯一权威）**：`mupc/deploy/config/mupc_core_config.yaml:143-148` 与 `mupc/deploy/config/mupc_core_config.production.yaml:148-153` —— 两份文件该站的 6 行 `regs` **取值逐字相同**（仅注释措辞不同），且与 **PRD §9.4.1 第 6 站（PRD:1391-1396）逐字一致**：
>   `p`@`0x1000`(6) → `q`@`0x1006`(6) → `pf`@`0x100C`(6) → `u`@`0x1012`(6) → `i`@`0x1018`(6) → `p_total`@`0x101E`(2)。
> - **单测 fixture 的形态（仅测试自用，**不得**作为核对待迁移配置的依据）**：`mupc/crates/mupc-southd/src/config.rs:324-329` 的 `VALID_5_STATION_YAML` 里是另一种写法：`p`@`0x1000`(6) → **`p_total`@`0x1006`(2) → `q`@`0x1008`(6) → `pf`@`0x100E`(6) → `u`@`0x1014`(6) → `i`@`0x101A`(6)`，且 `format` **全部** `float32`（生效配置是 `int32_scaled`/`float32` 混排）、站 `id` 为 `meter_grid`（生效配置是 `grid_meter`）。
> - **两者确实不同，这是允许的**（单测自有构造数据、不调 `validate` 的解析类用例更不要求与现场配置同形），但**必须分别标注**：§11.5.3.4 C3 已把该 fixture 定性为"legacy 写法解析逐字段不变"的实证锚、**不得**被"顺手改成新写法"；而**本节及 §11.7.4 的迁移结论一律以"生效配置的真值"为准**。v1.3 的 `grid_meter` 行恰恰把 fixture 形态当成了真值（并误称"逐字取自 PRD §9.4.1"）—— 该错误据二轮评审订正，**结论不变**（见该行"不拒 ✓"）。
> - **其余 5 站的逐站复核结论**：`bms` / `pcs` / `meter_batt` / `fire` / `hvac` 五行的 `addr`/`count`/`func` **逐格与 PRD §9.4.1 的 5 个新增站一致**（这五站在生效配置中仍是注释占位，真值即 PRD §9.4.1，不存在"fixture 与真值两张皮"的问题）⇒ **仅 `grid_meter` 一行有此类取证错误，已订正**。

| 站 | 同 `func` 且严格相邻的块对（穷举） | 是否参与判定 | 结论 |
|----|-----------------------------------|--------------|------|
| **`grid_meter`（既有站，PRD §9.4.1 第 6 站 = 生效配置真值）** | `p`(0x1000,6) → `q`(0x1006,6) → `pf`(0x100C,6) → `u`(0x1012,6) → `i`(0x1018,6) → `p_total`(0x101E,2)：**5 个相邻对全部成立**（0x1006=0x1000+6、0x100C=0x1006+6、0x1012=0x100C+6、0x1018=0x1012+6、0x101E=0x1018+6，`func` 均 `holding`、`byte_swap` 均缺省 false）。**注**：`p_total` 是**末块**（不是第二块）—— 单测 fixture（`config.rs:324-329`）把 `p_total` 放第二位且 `format` 全为 `float32`，**与生效配置不同**，勿据其核对待迁移配置 | **否 —— 6 块均未声明 `points`** | **不拒 ✓**（这正是旧口径误拒、必须重写的那一处） |
| `bms` | `bms_io`(100,31) 止于 130；`bms_energy`(139,19) 止于 157；`bms_meta`(181,9) 止于 189；`bms_term`(2991,4)；`bms_cap`(4000,6)；`bms_alarm` 为 `discrete`。逐对检查：139−131=**8**、181−158=**23**、2991−190 巨大、4000−2995 巨大 ⇒ **无相邻对** | 无 | **不拒 ✓** |
| `pcs` | 单块 `pcs_3zone`(1000,76) | 无相邻对 | **不拒 ✓** |
| `meter_batt` | `mb_e_*` 六块起点 0x0000/0x000A/0x0014/0x001E/0x0028/0x0032，`count` 均 2 ⇒ 0x0000+2=0x0002 ≠ 0x000A（**相隔 8 寄存器**，PRD 注释亦写明）；`mb_ui`(0x0061,6) 止于 0x0067 ≠ 0x0077；`mb_freq_line`(0x0077,4) 止于 0x007B ≠ 0x0087；`mb_phase`(0x0087,14) 止于 0x0095 ≠ 0x0164 ⇒ **无相邻对** | 无 | **不拒 ✓** |
| `fire` | `fire_sys`(4,13) 与 `fire_det`(17,114)：4+13 = **17**，同 `holding`、同 `byte_swap` ⇒ **1 个相邻对成立** | **否 —— `fire_det` 未声明 `points`**（`fire_sys` 有）⇒ 不参与 | **不拒 ✓**。**双重保险**：即便参与判定，合并后 `count = 13 + 114 = 127 > 120` ⇒ 判据也不触发（与现场"探测器区必须分片"的事实一致） |
| `hvac` | `hvac_in`(0,4,`input`) 与 `hvac_di`(0,31,`discrete`)：`func` **不同** ⇒ 非相邻；块内无其它块 | 无 | **不拒 ✓** |

**结论**：**§9.4.1 的 6 站参考配置（含既有 `grid_meter` 形态）必然通过第 15 条**。依据两条、各自充分：
1. **`bms`/`pcs`/`meter_batt`/`hvac` 四站**：穷举后**不存在任何"同 `func` 且严格相邻"的块对** ⇒ 第 15 条**无从触发**；
2. **`grid_meter` 与 `fire` 两站**：相邻对确实存在（前者 5 对、后者 1 对），但**都因"至少一方未声明 `points`"而不参与判定** ⇒ **不拒**；其中 `fire` 还有第二重保险（合并体 127 > 120，即便参与也不触发）。

故 **AC-1 ② 的断言（"除 §9.4.3 明列条件外不触发任何既有/新增拒绝"）对第 15 条成立** —— 现场启动不会因本条 fail-fast；且 §11.11.2 的 AC-1 ③ 已把第 15 条的 6 种形态**全部做成可执行用例**（`Ok`：六站 YAML / 仅 grid_meter / 一方无 `points` / 双方无 `points` / 合并后 >120 / 带 `read_slice`；`Err`：双方有 `points` + 合并 ≤120 + 无 `read_slice`），"必然通过"这句话本身也有断言钉住。

**（3）`read_slice` 真正的适用场景（避免被当成"逃避合并的开关"）**：只有**两个都声明了 `points` 的块**、合并后 `count ≤ 120`（说明"设备本来能一次读回"）、却因现场实测上限更低而必须分片时，才标 `read_slice: true` —— 典型是 **PCS 3 区按实测拆成 38+38**（两块都是 `points` 块，合并 76 ≤ 120 ⇒ 不标就会拒）。PRD §9.4.2.4 的四条约束（合并后仍可一次读回者必须合并 / 只豁免本条 / 块级逐块判定 / 须注释登记理由）在此**逐条适用**。

**（4）另一处设计补充的保守口径（PRD 未定义）**：点级 `name` 与 `count > 1` 同时出现 → **配置期拒**。理由：PRD §9.4.2.2 只定义了"点名 ↔ 序号"的单点形态，未定义"命名序列"；禁用比发明安全。该条**不在 PRD §9.4.3 清单内**，登记为设计补充（§11.12.1 项 3）。

#### 11.5.3 既有测试的连锁订正清单（**v1.3 扩至「用例名 + 断言」级**；上轮评审 P0-4）

上轮清单只到"文件 + fixture"级，且漏报了**断言级失配**与两处用例（`core_config::test_south_stations_grid_only_passes`、`test_south_stations_same_port_same_spelling_passes`），与 §11.13「仅 §11.5.3 列出的 fixture 订正」口径不符。本版按**三类**重建清单（已逐处 grep 核实到 `file:行`）：

- **① fixture 订正**：构造数据要改，**断言不改**；
- **② 断言订正**：期望值要改（**因 metric 更名而来，不是回归**）；
- **③ 断言不得改**：回归锚 —— 改了就是**掩盖回归**，须在评审中说明理由。

##### 11.5.3.1 订正规模（逐个数核实，取代 v1.2 的"≈13 处"）

| 受影响的构造点 | 实测处数 | 位置（`file:行`） |
|----------------|----------|-------------------|
| `RegBlockConf` 字面量 | **10** | `mupc-southd/src/mapper.rs:226`、`mupc-southd/src/scheduler.rs:430`、`746`、`754`、`794`、`802`、`810`、`853`、`861`、`tests/grid_convergence.rs:28` |
| `StationConf` 字面量 | **8** | `mupc-southd/src/port_runtime.rs:289`、`mupc-southd/src/scheduler.rs:452`、`474`、`487`、`737`、`785`、`844`、`918` |
| 需改构造数据的 YAML fixture / 内联 YAML | 见 11.5.3.2 | （下同） |
| 需改期望值的断言 | 见 11.5.3.3 | （下同） |

> 这 18 处**全部**要补新字段（`RegBlockConf` 补 `offset/byte_swap/points/read_slice`；`StationConf` 补 `parity`），属**纯机械补字段**（Rust 结构体字面量无 `..Default::default()`，故逐处补），**不改变任何测值**。

##### 11.5.3.2 【① fixture 订正】（构造数据要改，断言不改）

| # | 文件 | 用例 / fixture（`file:行`） | 订正内容 | 订正后断言 |
|---|------|---------------------------|----------|-----------|
| A1 | `mupc-southd/src/config.rs` | 静态 fixture `VALID_5_STATION_YAML`（`:336` `battery_1`、`:348` `fire_1` —— v1.4 订正行号，原写 `:345`；该 fixture 的 meter_grid 段见 `:324-329`，其取值与生效配置**不同**，理由见 §11.5.2(2) 的"取值真源"注） | ① `battery_1` 的块 `{ name: soc, addr: 100, format: int32_scaled, scale: 0.1, count: 2 }` → 改为**点名式**：`{ name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }`（否则规则 4 拒）；② `fire_1` 的 `addr: 0` → `addr: 4`（否则规则 13 拒） | 使用该 fixture 的 3 个用例：`parses_valid_5_station_yaml`（只解析，**不受影响**）、`meter_grid_regs_contain_pq_pfu_i_p_total`（只查 grid 站，**不受影响**）、`validate_passes_for_valid_config`（`is_ok()`，**依赖本订正**） |
| A2 | 同上 | `validate_battery_interval_boundary`（`:502`，内联 YAML `{ id: bat, role: battery, … interval_ms: {iv} }`） | 补含 `soc` 点的 `regs` | `assert_eq!(validate().is_ok(), ok)`（2 组 iv）**不变** |
| A3 | 同上 | `validate_accepts_max_slave`（`:471`，`slave: 247`） | 同上 | `is_ok()` **不变** |
| A4 | 同上 | `validate_accepts_same_port_same_baud`（`:791`） | battery 侧补 `regs` | `is_ok()` **不变** |
| A5 | 同上 | `validate_accepts_diff_ports_same_baud`（`:817`） | 同上 | `is_ok()` **不变** |
| A6 | `mupc-core-bin/src/core_config.rs` | `test_south_stations_valid_5_station_passes`（`:1744`） | `battery_1` 补含 `soc` 点的 `regs` | `is_ok()` **不变** |
| **A7** | 同上 | **`test_south_stations_grid_only_passes`（`:1856`）** —— **上轮漏报** | 同上 | `is_ok()` **不变** |
| **A8** | 同上 | **`test_south_stations_same_port_same_spelling_passes`（`:1973`）** —— **上轮漏报** | 同上 | `is_ok()` **不变** |
| A9 | 同上 | `test_south_stations_shared_serial_with_pcs_short_rejected`（`:1786`） | `battery_1` 补 `regs` —— **必须补**：否则规则 4 的 `soc` 错误会**先于**跨段校验返回，`err.contains("重复") && err.contains("仲裁")` 直接失配（这正属"断言级"影响，旧清单只说"补 regs"未说清**为什么**） | 断言**不变** |
| A10 | 同上 | `test_south_stations_shared_serial_with_pcs_fullpath_rejected`（`:1816`） | 同上 | 断言**不变** |
| A11 | 同上 | `test_south_stations_same_port_alias_spelling_rejected`（`:1941`） | `battery_1` 补 `regs` | `err.contains("别名")或("同节点")` + `contains("hvac_1") && contains("battery_1")` **不变** |
| A12 | `mupc-southd/src/mapper.rs` | `poll_to_result_battery_soc_block_maps_soc`（`:369`）的 `let ok = vec![sblock("soc", 65.5), sblock("temp", 25.0)];` | 承载 `soc` 的条目改为**点名式块**（`RegBlockConf{ name: "bms_io", addr: 100, func: Holding, format: Float32, scale: 1.0, count: 2, points: [PointConf{at:1, name: Some("soc")}] , …}`）—— 否则新语义（按**点名**查找）找不到 `soc` 点 | `pkg.battery.soc == Some(65.5)`、`temperature == None`、`inverter_status == Running`、`Err → Failed`、`空 reads → Data + soc None` **全部不变** |
| A13 | `mupc-southd/src/scheduler.rs` | `battery_conf()`（`:473`，块名 `soc`） | 同 A12 改为点名式（保持 float32 + 65.5 量值，使 `soc_of == Some(65.5)` 成立） | 见 11.5.3.3 **第 9 条**（v1.4 订正交叉引用：原写"第 8 条"，该条讲的是 `telemetry_points` 的值槽数，与本例无关） |
| A14 | **同一 crate（`mupc-southd`）**的 18 处字面量（11.5.3.1；v1.4 订正 —— 原写"三个 crate"有误，这 18 处全部落在 `mupc-southd`：`src/mapper.rs`、`src/scheduler.rs`、`src/port_runtime.rs`、`tests/grid_convergence.rs`） | 机械补新字段 | 无 | 无 |
| **A15** | `mupc-southd/src/config.rs` | **`validate_rejects_slave_out_of_range`（`:520`）** —— **v1.4 由 §11.5.3.5 移入本类** | 补合法 `regs`（含 `soc` 点） | `is_err()` **不变**（见下方"语义漂移"注） |
| **A16** | 同上 | **`validate_rejects_zero_interval`（`:534`）** —— **同上移入** | 同上 | `is_err()` **不变** |

##### 11.5.3.3 【② 断言订正】（metric 更名导致期望值要改 —— **不是**回归锚）

根因：`telemetry_points` 对**未声明 `points` 的块**由"取首值、metric = 块名"改为"**每值槽 1 点、metric = `<块名>_<序号>`**"（§11.2.1 已登记的行为变更）。**受影响范围已逐处核实**：

| # | 用例（`file:行`） | 原断言 | 订正为 | 依据 |
|---|-------------------|--------|--------|------|
| 1–5 | `scheduler.rs`：`two_stations_same_port_schedules_by_due`（`:632`）、`grid_offline_isolated_hvac_continues`（`:662`）、`station_recovers_after_failure`（`:685`）、`station_recovers_after_extended_backoff`（`:719`）、`non_battery_role_never_triggers_soc_channel`（`:951`）—— **共 5 处 `assert_eq!(telemetry_of("hvac"), …)`** | `vec![("temp".to_string(), 23.5)]` | **`vec![("temp_1".to_string(), 23.5)]`** | 块 `temp`：`addr 100`、`count 2`、`format float32` ⇒ 宽度 2 ⇒ 1 个值槽 ⇒ 序号 = 0+1 = 1 |
| 6 | `scheduler.rs::station_mixed_holding_and_input_blocks`（`:768-770`） | `m == "temp"`、`m == "alarm_in"` | `m == "temp_1"`、`m == "alarm_in_1"` | 同上（两块各 `count 2` + `float32`） |
| 7 | `scheduler.rs::pure_input_blocks_station_collects_via_fc04`（`:877-879`） | `m == "alarm_in" && v == 0.5`、`m == "status_in" && v == 1.5` | `m == "alarm_in_1"`、`m == "status_in_1"`（**值 0.5/1.5 不变**） | 同上 |
| 8 | `mapper.rs::telemetry_points_first_value_per_block`（`:392`） | `assert_eq!(pts, vec![("p",1.0),("u",220.0)])` | **`vec![("p_1",1.0),("p_3",2.0),("p_5",3.0),("u_1",220.0),("u_3",221.0),("u_5",222.0)]`**（**测试名同步改为 `telemetry_points_every_value_slot`**） | 单块 6 寄存器 / `float32` ⇒ 3 个值槽；**点产出数量本身也变了**（1 点 → 3 点），这是 AC-6 ①"每值槽 1 点"的钉子。注：`blk()` 的 `scale = 0.0` 不影响 `float32`（`scale` 仅对整数格式生效，规则 5 亦不适用于 `Float32`），故值仍为 1.0/2.0/3.0 |
| 9 | `scheduler.rs::battery_station_soc_pushed_via_dedicated_channel`（`:896`） | `m == "soc"` | **不变**（`battery_conf` 按 A13 改为点名式后，点位名就是显式 `soc`） | PRD §9.4.2.2"显式 `name` 优先" |

> **为什么这些不属"断言不得改"**：它们断言的是**遥测键名**，而键名的变化是 PRD §9.4.2.1 第 4 条**规定的**（"未声明 `points` ⇒ 每值槽 1 点 + 位置式命名"）；#8 甚至断言的是**点数**，也由同一条规定改变。把它们当回归锚会**永久锁死 PRD 的规则**。反之（见 11.5.3.4）`grid_convergence.rs` 的断言与 grid 站的 `decode_phase_block` 语义**不经过**这条路径 —— 那才是回归锚。

##### 11.5.3.4 【③ 断言不得改】（回归锚，逐项说明"为什么它是锚"）

| # | 用例 / 断言 | 为什么不得改 |
|---|-------------|--------------|
| C1 | **`mupc-southd/tests/grid_convergence.rs` 全部 4 个用例的全部断言**（`meter_grid_phase_matches_legacy_semantics_canned` / `meter_grid_p_total_raw_when_present` / `meter_grid_missing_phase_block_returns_failed` / `meter_grid_negative_p_direction_signs_current`） | **评审员特别强调**：这是 S3a 收敛闸门（§10.9）。grid 路径走 `poll_to_result(MeterGrid)` → `decode_phase_block`（**按块名**查找），**完全不经过** `points[]`/`telemetry_points` ⇒ 本轮所有更名对它**零影响**。**只允许改 `block()` helper 的构造**（补 4 个新字段），**任何断言值的改动都视为掩盖回归** |
| C2 | `mupc-southd/src/config.rs` 的 **meter_grid-only 系列**：`meter_grid_regs_contain_pq_pfu_i_p_total`、`validate_meter_grid_interval_boundary`、`validate_rejects_meter_grid_missing_phase_block`/`_empty_regs`/`_zero_addr`/`_overlapping_regs`/`_phase_block_count_lt_6`/`_int32_scaled_zero_scale`/`_duplicate_block_name`、`validate_accepts_meter_grid_complete_regs`/`_without_p_total`、`meter_grid_full_regs_yaml`/`meter_grid_only_yaml` | 它们是 S3b-1c 的既有校验语义（相量块齐备、`count ≥ 6`、`addr > 0`、区间不重叠、块名唯一、`int32_scaled` 的 `scale > 0`）的**唯一钉子**；且这 6 个相量块**未声明 `points`** ⇒ 新的规则 15/19 与逐点展开**都不触碰它们**（这正是 §11.5.2 的适用域限定要保住的）。**⚠️ 与规则 10 的冲突及钉法见下方 §11.5.3.4.1** |
| C3 | `mupc-southd/src/config.rs` 的**解析类**：`default_reg_format_used_when_omitted`（`blk.format == Float32`、`scale == 0.0`、`count == 2`）、`reg_block_func_parses_holding_input`（`func` 缺省 = `Holding`）、`default_field_fallbacks_apply`、`default_baud_and_func_apply_when_omitted`、`parses_valid_5_station_yaml` | 它们钉住"**legacy 写法（块名 `soc`、无 `points`、缺省 `format`）解析逐字段不变**" = §11.2.1 兼容性论证的实证。**其 fixture 仍保留 `name: soc` 的旧写法**（parse-only，不调 `validate`），**不得**被"顺手改成新写法" |
| C4 | `mupc-core-bin/src/core_config.rs` 的 6 个 southern-stations 用例的**断言**（含 `err.contains("重复") && err.contains("仲裁")`、`err.contains("别名")`、`err.contains("interval_ms") && err.contains("south_stations")`、两处 `is_ok()`） | 跨段校验（port 与 intercore 互斥、同口别名、interval 传播）是 §10.3 的既有契约；**只改 fixture（A6–A11），断言一个字节都不动** |
| C5 | `scheduler.rs` 的 `soc_of` / `grid_count` / `event_count` / `bus.call_count` / `input_call_count` / `state[..].offline_count` 类断言（`:632` 的 `call_count`、`:640`、`:668`、`:693`、`:778`、`:839`、`:886`、`:901`、`:1015`、`:1039`、`:1066` 等） | 与 metric 命名无关：断言的是**调度节拍、退避、隔离、SOC 通道 gating**（§10.2/§10.5 语义）。若它们变红，就是**回归** |
| C6 | `scheduler.rs::battery_station_without_soc_block_does_not_push`（`:914`，`regs: vec![]`）**的全部断言**（无 offline/online 事件、`soc_of` 为 `None`） | 该配置形态在新规则下**已被配置期拒**（规则 4），但本用例**不调 `validate`**（直接构造 `StationConf` 交给调度器）⇒ 仍绿，且它是**调度器层的纵深防御锚**："无 `soc` 点 ⇒ 绝不误推 `on_battery_soc`"（PRD §9.4.3 规则 4 的运行期对偶）。故：**断言保留不动**，只加注释说明"该形态在配置期已被规则 4 拒，本测保的是调度器侧不依赖配置校验的负向 gating"；**同时**在 `config.rs` **新增** `validate_rejects_battery_without_soc_point` 覆盖配置期侧（两层各有其测） |
| C7 | `mupc-southd/src/mapper.rs` 的 `meter_grid_*`（`:286`/`:314`/`:331`/`:338`/`:347`）与 `poll_to_result_other_role_returns_minimal_data`（`:404`）断言 | 前者是 mapper 层的总表语义锚，后者钉住"非 grid/battery role ⇒ 空 `DataPackage`"；本轮只给 `Pcs` 加同臂，**语义零改动** |

> **`SouthScheduler::new` 不得新增 `validate()` 调用**（决议）：否则 C6 会因"构造了非法配置"而变红，且会把"配置期一次校验"变成"每次装配都校验"。配置校验的**唯一入口**仍是 `core_config` 的装配期（§10.3）。

##### 11.5.3.4.1 【C2 与规则 10 的冲突：实现判定顺序约束】（v1.4 新增 —— 二轮评审「须补 2」）

**冲突事实**：C2 里的 `validate_rejects_meter_grid_duplicate_block_name`（`config.rs:681-694`）构造的是**两个同名 `p` 块**（`p`@0x1000(6) 与 `p`@0x1006(6)，均无 `points`、`format: float32`）：
- **既有规则**（`meter_grid` 块名唯一，`config.rs:213-223`）命中 ⇒ 报 `south_stations: meter_grid 站 {id} regs 块名重复: {name}（mapper 按 name 取首块，后者静默失效）`，断言 `err.contains("块名重复") && err.contains("p")`；
- **但新规则 10（点名唯一）同样成立**：两个块展开后各产 `p_1`/`p_3`/`p_5`（`float32` 宽度 2、`count 6` ⇒ 3 个值槽）⇒ 站内 `metric` 重复 ⇒ 规则 10 也可判 `Err`（文案是"点名重复"一类）。

**若实现顺序变化，报错文案随之改变，而 C2 又把它列为"断言不得改"** ⇒ 开发无从猜测。**本设计按下述方式钉死（不必猜）**：

1. **判定顺序固定为**：站级基础校验（含既有 `meter_grid` 整组校验，**块名唯一在其中**）→ `validate_station_regs`（含展开 + 规则 10/13/14/19）→ 跨站（单站计数 / 同口一致性 / 极大性）。即 §11.5.1「顺序」给出的**实现约束**：既有 `meter_grid` 整组**原地不动、先于展开校验**。
2. **该用例命中的规则与文案**：命中**既有 `meter_grid` 块名唯一规则**，文案**逐字不变**（上引原文）⇒ **C2 断言不动**（"断言不得改"类别维持）。
3. **规则 10 不得因此删除，且有它独有的覆盖面**：块名唯一是 **`meter_grid` 专有**（因 mapper 按 `name` 用 `.find` 取首块）；其余 role 的同名块，以及"**块名不同、展开后 metric 却相同**"（如两块各有一个 `name: soc` 的显式点名、或显式 `name` 与另一块的自动位置点名相撞）**只有规则 10 能拒** ⇒ 须**新增用例**覆盖（已并入 §11.11.2 AC-1 ③ 的规则 10 一条）。
4. **为什么不订正 C2（对上一轮清单的处置说明）**：上一轮把 C2 列为"断言不得改"是**正确的，本轮不作订正** —— ① `块名重复` 是**根因**（mapper 按 `name` 取首块 ⇒ 后者静默死配），`点名重复` 只是同一配置的**派生症状**，报根因对运维可操作性更强；② C2 是 S3b-1c 校验语义的**唯一钉子**，改文案等于削弱回归锚；③ 本约束**零实现成本**（既有检查原地不动即可），不存在"为了保测试而扭曲设计"的代价。

##### 11.5.3.5 【④ 期望 `Err` 的用例：fixture 与断言均无需改动】

> **分类归属订正（v1.4，二轮评审建议 5）**：本类原本被标为"③"（与 §11.5.3.4 的"断言不得改"重号），且其中**两个用例实为 ①（fixture 订正）**。现：本类改号为 **④**；`validate_rejects_slave_out_of_range` 与 `validate_rejects_zero_interval` **移入 §11.5.3.2 的 A15/A16**（见该表与下方"语义漂移"注）。

`validate_rejects_duplicate_id`（`:406`）、`validate_rejects_multiple_meter_grid`（`:419`）、`validate_rejects_multiple_battery`（`:434`）、`validate_rejects_empty_id`（`:449`）、`validate_rejects_empty_port`（`:460`）、`validate_rejects_meter_grid_slow_poll`（`:545`）、`validate_rejects_same_port_mixed_baud`（`:779`）、`validate_rejects_same_port_mixed_baud_3_station`（`:803`）、`validate_rejects_zero_baud_rate`（`:830`）、`validate_rejects_zero_baud_same_port_all_zero`（`:842`）—— 断言均为 `is_err()`，fixture 亦无需改，**整例无需改动**。（原列表中的 `validate_rejects_slave_out_of_range`、`validate_rejects_zero_interval` 已移出本类，见上。）

> **须登记的一处语义漂移（不是缺陷，但要写下来；v1.4 已将其从"口头要求"落成 ① 的 A15/A16）**：`validate_rejects_slave_out_of_range`（`:520`，battery、无 `regs`）与 `validate_rejects_zero_interval`（`:534`，battery、无 `regs`）在实现新规则后**会先被规则 4（`soc` 点契约）拒**，而**不再由原本要测的那条规则拒**（`is_err()` 仍成立 ⇒ 用例仍绿，但已测不到 slave 下界 / interval 下界）。这不是缺陷，但属"测试通过却没测到"的隐患 ⇒ 实现期按 **A15/A16 补上合法 `regs`** 使二者真正测到目标规则；另**新增**两个用例（`validate_rejects_battery_without_soc_point` 已含在 C6；`pcs` 下界 `validate_rejects_pcs_interval_below_500` 对应规则 18）。**"补 regs"属 ① fixture 订正，断言仍不得改**。

### 11.6 字节序 / 字序 / 特殊编码（含 AC-2 算式复算）

| 场景 | 配置 | 解码链 | AC-2 期望值（逐位复算） |
|------|------|--------|--------------------------|
| BMS `soc`（118） | 块 `uint16`/`scale 1.0` + 点 `name: soc` | 1 寄存器 → u16 → ×1.0 | raw `0x0041`(65) → **65.0 %** |
| BMS 簇组电压（115） | `uint16`/`0.1` | 1 寄存器 → u16 → ×0.1 | raw `0x1403`(5123) → **512.3 V** |
| BMS 簇组模块温度（117） | `uint16`/`1.0`/`offset −40.0` | 1 寄存器 → u16 → ×1.0 + (−40) | raw 65 → **25.0 ℃** |
| BMS 116 簇组电流 | `uint16`/`0.1`/`offset −1600.0` | 1 寄存器 → u16 → ×0.1 − 1600 | raw 16000 → **0.0 A**；raw 15000 → **−100.0 A**；raw 0 → **−1600.0 A**；raw 65535 → **+4953.5 A**（判别点，与 `int16` 的 −1600.1 **必须不同**） |
| PCS `soc` 转述（1010） | 块 `byte_swap: true` | swap(0x4100) = 0x0041 → u16 → ×1.0 | 注入 `0x4100` → **65.0 %** |
| PCS 交流累计充电电量（1042–1043） | `int32_scaled`/`0.1`/`byte_swap`/`word_order: lo_hi` | swap 后 `[0x0064, 0x0001]` → lo_hi 拼 → 65636 → ×0.1 | 注入 `[0x6400, 0x0100]` → **6563.6 kWh**（误用 `hi_lo` 得 655360.1 → 必须不等） |
| ADL400 A 相电流（0x0064） | `uint16`/`0.01` | 1 寄存器 → u16 → ×0.01 | raw 946 → **9.46 A** |
| ADL400 组合有功总电能（0x0000） | `int32_scaled`/`0.01`（无 swap/字序） | hi_lo → (0×65536+12326) → ×0.01 | `[0x0000, 0x3026]` → **123.26 kWh** |
| 消防火警状态（9） | `uint16`/`1.0` | 1 寄存器 → u16 | 2 → **二级火警**（枚举语义，非位） |
| 消防探测器数据 1（13） | `uint16`/`1.0` | 1 寄存器 → u16（**整字**，G-6 延后） | `0x4150` → telemetry 落 **16720**；展示层拆解得烟雾 6.5 dB/M、温度 25 ℃（§11.10） |
| 空调 30001 / 30004 | `int16`/`0.1`；`uint16`/`0.1` | 1 寄存器 → i16/u16 → ×0.1 | 258 → **25.8 ℃**；602 → **60.2 %** |
| 位点（`hvac_di_1`） | `discrete`,`addr 0`,`count 31` | `unpack_bits(bytes, 31)[0]` | 首字节 `0x3B` → 位 0/1/3/4/5 = 1、位 2/6/7 = 0 |

> **换算的符号性口径（PRD v1.5 重裁定，设计不得回退）**：`offset` 只表示**零点平移**，与"raw 如何解释为整数"**无关**；`uint16`/`int16` 与任意 `offset` 的组合**一律放行**（BMS 的 116/117/122/127/129/155/157/186/2991–2994 全部是"无符号编码 + 负偏移"）。校验器**只校验"可追溯"与"与点表一致"**，不校验"符号性对不对"（后者靠 §9.5 的"类型来源"栏 + Q-20 现场判别 + RC-1）。

### 11.7 消费链路

#### 11.7.1 分流总表（对齐 PRD §9.6.1 / §10.5，**零语义变更**）

| 站 | 数据 | 通道 | 是否推进 5s 控制闸门 |
|----|------|------|----------------------|
| `battery` | 点名 `soc`（118） | `on_battery_soc` → `AiIntegrator::set_battery_soc`（BMS 优先源，超期回落核间，04 §2.11.1） | **否** |
| `battery` | 其余 344 点（含 288 位） | `on_station_telemetry`(is_event=false) → storage `telemetry`（device_id = 站 id） | 否 |
| `battery` | 告警族位的 0→1 跳变 | `on_station_telemetry`(is_event=true) → storage `events` + `AlertFeed` | 否 |
| `pcs` | 3 区 72 点 | `on_station_telemetry` → telemetry + 健康/校核展示 | 否 |
| `meter_batt` | 40 点 | telemetry + 能量流核算 | 否 |
| `fire` | 127 点 + **字级信号**（§11.7.2）+ 登记数不一致 + 探测器地址升序违规 | telemetry（整字原值）+ **events**（信号进入/退出活跃 + 登记数/地址序异常）；火警/联锁融合维持 §10.4（**停机仅以 DI3 触发**） | 否（**仅事件/联锁判据，非控制量**，PRD §9.6.1） |
| `hvac` | 34 点 | telemetry + events（告警位） | 否 |
| `grid_meter`（既有） | 6 点 | `on_grid_package` → AiIntegrator + IEC104 | **是（唯一）** |

#### 11.7.2 位点落库与事件（D2 口径的落地 + v1.3 的消防信号落点）

1. **每轮**在内存中形成完整点位（`mapper::telemetry_points` 返回全部 288+31 位），供即时展示/判据使用；
2. **落库**：位点仅在与上轮不同时经 `on_station_telemetry(is_event=false)` 落 `telemetry`（稳态≈0 行/s）；
3. **事件**：`BitClass::Alarm` 的 **0→1** 跳变 → events（metric = 点位名，如 `bms_alarm_225`）；`Reserved` 位（如 364–423、484–599）**只产 telemetry、不产事件**（PRD §9.7.6）；`State` 位同理只落 telemetry；
4. **事件可读性**：`SouthSink` 组装 `SystemEvent.message` 时可选查 `mupc_southd::point_table::label(role, metric)` 补中文名（如 **`bms_alarm_225` → "簇一级告警"** —— 按 §9.7.4/§9.5.1B，点名序号 225 = 位偏移 224 = **位地址 424 = 簇一级告警**；**勿**与位地址 225（"簇 SOC 低·轻"）混淆 —— 位地址 225 对应的**点名**是 `bms_alarm_26`，v1.3 此处示例把两者串了、v1.4 订正），查不到则用原名（**不引入新的事件类型枚举**，沿用 `south_station.<站id>.<metric>`）；
5. **SOC 越界事件**（PRD §9.6.3）：metric `soc_out_of_range`，`value` = **原始寄存器解码值**（`SouthSink` 侧把 `is_event` 且非 offline/online 的点的 `value` 写进 message，作为"原始值落证"）。

**消防字级信号（v1.3 新增，P0-3 的落地；机制见 §11.4.7.1）**：

6. **信号来源与检测层**：消防全部状态量在**保持寄存器整字**内（`fire_sys_1/3/4/5/6/9` 等）⇒ 由 `scheduler` 每轮在**整字原值**上做 **`字 & mask`（`WordBit`）/ `字 ∈ active`（`WordEnum`）** 判定，与离散位共用同一个 `EdgeTracker` 与同一条事件产出路径。**telemetry 仍落整字原值**（值不变、可回溯），事件**不改写**任何 telemetry 值；
7. **产事件的信号清单**（逐条对表 PRD §9.5.4 的位定义表）：
   - **系统状态（addr 4）**：`main_power_fault`(bit14) / `backup_power_fault`(bit13) / `drive_circuit_fault`(bit11) / `pressure_sensor_fault`(bit10) / **`spray_fired`(bit8)** / `valve_open`(bit9) —— 前 4 条为故障类；**`spray_fired`/`valve_open` 为"灭火动作已发生"的留痕**（PRD 明确要求列举"喷洒标记"），文案由 `label` 区分（"喷洒标记置位"而非"故障"）；`work_mode_manual`(bit15)、`charging`(bit12)、备电电量 bit7–0 只落 telemetry（普通状态量，无事件消费方）；
   - **烟/温/可燃状态（addr 6/7/8）**：各产 `smoke_trigger` / `temp_trigger` / `combustible_trigger`，`mask = 0b11`（bit1 复合探测器触发 + bit0 干接点触发）—— **bit2 点型探测器"预留未启用"不纳入 mask**（PRD §9.7.6 明文"不产出告警事件"）；
   - **火警状态（addr 9，枚举）**：`level1`(值 1) / `level2`(值 2) / `emg_start`(值 4) / `emg_stop`(值 5)；值 3 预留不入 `active`；**值 0 工作正常 = 非活跃** ⇒ 任何活跃值回到 0 即"退出活跃"事件（联锁恢复的可观测点，§10.4"恢复需双方复位"）；
   - **探测器状态（addr 12）**：每只探测器产 `alarm`(bit12 报警总状态) 与 `fault`(bit14 故障总状态)；bit0–4 传感器细分**不产独立事件**（取舍见 §11.12.1 项 6）；
   - **不产事件的消防点**：钢瓶气压（addr 5，PRD §9.7.6）、探测器地址/数据 1/CO/VOC/H2（无阈值口径）、复合探测器 `comm_offline`(bit15) 与反馈位（极性相反，不猜 —— 仅展示）；
8. **事件名**：`<遥测点名>@<信号键>`（例 `fire_sys_1@spray_fired`、`fire_sys_6@level2`、`fire_sys_9@alarm`）—— **属事件命名空间，不新增遥测点、不占用 telemetry 命名**（§11.12.1 项 5）；
9. **探测器地址升序违规（Q-9）**：`fire_detector_addr_order_violation` 命中 → `("fire_detector_addr_order_invalid", 违规组序号, true)`，且**该轮探测器区点位标记为不可信**（telemetry 照落原值）。

#### 11.7.3 展示与存储口径

- 位点"长时间未变" ⇒ 最后一条 telemetry 时间戳陈旧，展示层按"最后变更时刻 + 现势内存值"呈现，**不得**把陈旧时间戳当作"该位当前为 0/1 的证据"（与 §10/§9.7.6"保留上一有效值并标记 stale"同口径）。
- 新增模拟量点 ≈ 299 行/轮（5 站合计 ≈ 300 行/s），与既有 `meter_grid` 同阶；retention 天数复用既有配置，投运前按盘容量复核（§11.12.3 待决项）。
- **消防钢瓶气压"未配置"口径（v1.3 新增，PRD §9.7.6 的设计落点）**：`fire_sys_2`（addr 5）在**本站生命周期内从未出现过非 0 值** ⇒ 展示层标 **"未配置"**，**不得**显示 "0 kPa"、**不得**据此判"气压异常/泄漏"（`cylinder_pressure_configured`，§11.4.6）。该"从未非 0"事实由 scheduler 每站一个 `bool` 记忆维护（初值 false，任何一轮读到非 0 即置 true 且此后不再回退 —— 钢瓶气压不会在业务上"变回未配置"）。**注意**：该点**不产任何事件**（PRD §9.7.6 明令），故它不是"阈值判据"，只是**展示口径**。
- **消防探测器点位的"地址序有效前提"（v1.3 新增，Q-9 的展示侧落点）**：`fire_detector_addr_order_violation` 命中的那一轮，展示层对探测器区（`fire_det_*`）**不得**按"第 n 只探测器"呈现点位与物理编号的对应关系（升序前提被破坏 ⇒ 位置式点名与实物不再一一对应），应同时展示地址序异常告警；原始值仍可查（telemetry 照落）。

#### 11.7.4 配置迁移清单（生效配置从"1 站"到"6 站"）

| 项 | 动作 |
|----|------|
| `grid_meter` 站 | **逐字不动**（PRD §9.4.1 第 6 站与生效配置取值一致） |
| `bms`/`pcs`/`meter_batt`/`fire`/`hvac` | 按 PRD §9.4.1 的 6 站 YAML 替换注释占位；`pcs` 站**默认保持注释**，待 Q-15（A1/B1 是否隔离）现场裁定后启用（RC-8） |
| 旧 `battery` 站写法（`name: soc` 的**块**） | 必须改成"点名 `soc` 的点"（§11.4.6）；`deploy` 中的注释行按 §9.4.1 重写 |
| `fire` 站的两处"非默认"写法 | ① `fire_sys` 的 `at: 7` 声明 `name: fire_det_count`（PRD §9.4.1 参考配置已给，直接照录）；② 探测器区若现场需分片，分片块加 `read_slice: true` **并在注释登记理由**（PRD §9.4.2.4 第 ④ 条，RC-5 目视项） |
| `pcs` 站若按 RC-2 不配 4 个 32 位电量点 | 从 `pcs_3zone` 的 `points` 中删去 `at: 43/45/73/75` 四条（**删点不删块**：块窗口与其余点的序号锚定绝对地址，故删点**不构成改名**；但须在变更流程中登记）—— 与 §9.7.3"比对通过前 32 位电量视为不可信"一致。**⚠️ 本行同时是 PRD §9.8.4「比对未通过 → 事件」的替代口径（用"不配点"替代运行期事件），差异登记见 §11.12.2 Δ-9** |

### 11.8 写操作独立 Task 的设计边界与 S2 联锁分工

**本轮结构性保证"只读"**：`StationBus` trait **没有任何写方法**（§11.4.5 只加 `read_discrete`），scheduler/mapper 均无写路径 ⇒ PRD N-4（`pcs` 不写）、N-5（消防 1999 不被调用）、N-6（不改写设备参数寄存器）**由类型系统保证**，不依赖约定。PCS 4 区（1000–1057 与 500–503）与 BMS 1000+ 阈值、消防 1999、空调 40001–40059 **一律不配块**（不进 `regs`），避免"设定值被当实测值"（§9.5.1C）。

**写 Task 立项时须先结清的 4 件事（本设计只给边界，不给实现）**：

1. **PCS 写归属**：4 区 **500 = 模块启停**已被 S2 DI/DO 安全联锁用作**停机原语**（`intercore::ModbusRtuTransport` 持有，触发即 `500=0` 并**锁存禁启**）。若 `southd` 也写 4 区，将出现**两个写方争用同一停机原语**。故"并入 intercore 还是下放 southd"须**单独评审**后再立项；归属未定前 `southd` 对 PCS **一律只读**。
2. **写路径的仲裁**：写必须与读共用同一个 `Rs485PortBus`（同一 `bus_lock` + 同一 `tx_lock` 语义），否则口内请求会交错。
3. **控制动作的准入**：写指令必须经策略/AiValidator 链路（PRD N-5），不得由采集站直写；写操作须有"谁/何时/写了什么/回读确认"审计。
4. **与 S2 的分工（不得重叠）**：`southd` 负责**采集面**（`fire` 的状态位、`hvac` 的只读告警/温湿度、`pcs` 的只读 3 区）；S2 负责**安全联锁面**（DI 触发 → 停机原语 → 锁存）。两面的接口 = 既有事件通道（storage `events` + `AlertFeed`），**不新增直连调用**。消防的融合规则维持 §10.4（OR 取触发、停机仅 DI3、恢复需双方复位、去重键"消防+源"）。**v1.3 补一处联接**：§10.4 的"**恢复需双方复位**"需要知道**消防侧是否已解除** —— 这正是 §11.4.7.1 给消防信号取**双向**（进入/退出活跃）的原因；`fire` 侧的"退出"事件（`fire_sys_6@level2` 等，`value = 0.0`）是融合判据**唯一**可观测的消防侧复位信号，故该不对称决策**不是风格选择，而是 §10.4 融合的必要输入**。

### 11.9 Q-1（BMS 主从方向）的处理【PRD 标记为设计阶段阻塞项】

#### 11.9.1 采用的口径（书面结论）

**本设计采用 PRD §2.2 口径：MUPC（EMS）为 Modbus 主机，BMS 主控模块为从机，由 `southd` 主动轮询**（`port: /dev/ttyS2`、`protocol: modbus`(RTU)、`slave: 1` = 簇号）。依据：① 与 `southd`"每口一 master、主动轮询"的调度模型一致（无需新增从站模式）；② 与 `slave` 字段、`interval_ms` 排程、退避降频语义自洽；③ 该口径是 PRD §9.5.1 点表映射与 §9.6.1 SOC 消费链的既定基线。

**本设计不擅自修改 PRD**：若厂方书面确认为相反口径，按 PRD Q-1 ④-b **须由需求侧重新走需求评审**（点表映射与 SOC 消费链），设计不代庖。

#### 11.9.2 相反口径的影响面（为何"双模式"不可行，以及真正会变的是什么）

若厂方确认"**BMS 主控做主机、EMS 做从机**"：

- **RTU 链路的数据面消失**：Modbus 从机（MUPC）只会被 BMS **读取**自己的寄存器，链路上**不存在** BMS → MUPC 的数据下行。因此① `battery` 站的采集路径**不存在**（不是"参数取几"）；② `on_battery_soc` 的控制链时序、5s 新鲜度窗口、退避降频语义**失去意义**；③ 由此可得结论：**"主站/从站双模式可切换"是伪选项**——真正可切换的不是初动方向，而是**接入路径**。
- **唯一可行的替代路径**：BMS 协议 §1.1 载明同时支持 **LAN**，§3.1 载明 TCP 形态（"主控模块做 TCP 服务器，后台监控主动连接"）。即相反口径下的替代方案 = **MUPC 作为 Modbus-TCP 客户端**连到 BMS 的 TCP 服务器（数据由 MUPC 主动读，与"MUPC 轮询"的消费语义相同，只是传输换成 TCP）。
- **影响面清单（切换代价）**：
  | 维度 | 影响 |
  |------|------|
  | 传输层 | 新增 `protocol: modbus_tcp` 站点类型（`StationConf.protocol` 字段已在，但 `StationBus` 需新增 TCP 客户端实现：连接管理 + 重连 + MBAP 头 + 与串口的超时/退避语义对齐）→ **≈1 个新模块 + 一套连接生命周期测试** |
  | 采集面 | 点表、块划分、点位名、解码**完全复用**（本设计的 `points`/`RegDecode`/`point_table` 与传输无关）→ **零改动** |
  | SOC 链 | 若采用 TCP 客户端，消费侧**零改动**（见 §11.9.3）；若不采用（无 LAN 施工条件），则 BMS 站的 SOC **不可得**，SOC 只能回落核间（PCS 侧转述值 1010 **不得**作为控制源——N-1；若要启用，须重新评审） |
  | 新鲜度 | TCP 轮询语义与 RTU 相同（仍是轮询），5s 窗口/`interval_ms < 5000` 约束不变；若退化为"BMS 主动上报"，则须重新定义新鲜度与丢报判据（**属需求变更，非设计可裁**） |
  | `slave` 字段 | RTU 从机地址语义失效（TCP 用 Unit ID，缺省 1），配置字段保留、语义转为 Unit ID |
- **切换判据与时点**：判据 = **RC-4**（按 §2.2 口径实测，连续 ≥1h 无离线）；时点 = **PCS 站启用前**（Q-15 同理）。判据不成立 → 立即转厂方书面确认，按 PRD Q-1 ④ 分支处置。

#### 11.9.3 让口径切换不牵动消费侧的设计（E3 的落点）

把"SOC 供给"从"某站必须轮询"解耦为"**站内有点名为 `soc` 的点**"：

- 消费契约（PRD §9.4.3）锚定在**点名**而非块名/站类型；
- scheduler 只在 `role == Battery` 时推 `on_battery_soc`（当前唯一供给方）；
- 若将来供给方换成 TCP 客户端站，只需该站仍声明 `soc` 点名，`scheduler`/`SouthSink`/`AiIntegrator`/策略**均不需改**。

> **结论**：设计**不阻塞于** Q-1 的书面答复即可开工实现（按 §11.9.1 基线），但 **RC-4 与 PCS 站启用前必须结清**；本设计已按要求给出可切换方案（E3）与切换代价（§11.9.2）。

### 11.10 G-6（寄存器内字节拆分）延后口径与展示层拆解边界

**本轮不做**（PRD §9.4.2.3，配置契约**不含 `slice` 字段**），替代口径 = **整字采集原值 + 展示/事件层拆解**：

| 点 | 采集 | 展示层拆解（**仅展示，不作判据**） |
|----|------|-----------------------------------|
| BMS 位置编号 124/126/128/130（`bms_io_25/27/29/31`） | 整字 u16 原值 | **不拆**——原文 Bit15–8 与 Bit7–0 都标"PACK 编号"，低字节语义自相矛盾（Q-18）⇒ 拆解结果**不得**作为判据、也不建议展示"哪个单体" |
| 消防探测器数据 1（`fire_sys_10` / `fire_det_*`） | 整字 u16 原值 | 可拆：烟雾 = `(raw >> 8) × 0.1` dB/M；温度 = `(raw & 0xFF) − 55` ℃（原文语义无歧义） |

**跨模块一致性靠什么保证（必须有落点）**：拆解公式**不在南向代码里**，而是由 ① PRD §9.5.1/§9.5.4 明文（唯一真源）+ ② `point_table` 的 `label` 与 §11.10 表格（展示侧可直接引用本节的公式与示例值 `0x4150 → 6.5 dB/M / 25 ℃`）+ ③ 展示层单测复算 AC-2 的同一个例子共同保证。**南向侧只保证"原值不丢、不改、可回溯"**。

**与"消防事件层"的边界（v1.3 新增，回应 P0-3 的"是否与 G-6 冲突"）**：不冲突，因为两件事不同、代码也不同 ——

| 层 | 处理对象 | 手法 | 产物 | 本轮是否做 |
|----|----------|------|------|-----------|
| **遥测层**（southd `telemetry_points`） | 整字 | **不拆** | telemetry 落整字原值（`fire_sys_1/6`、`fire_det_*` 的 `value` = 0–65535） | 做（口径不变） |
| **事件层**（southd `scheduler` + `EdgeTracker`，§11.4.7.1） | 整字内的**位/枚举** | `字 & mask` / `字 ∈ active` —— **只有常量掩码与活跃集，没有"字节切片"逻辑** | 事件（`点名@信号键`），**不产出新遥测点、不改写原值** | 做（本轮新增） |
| **展示层** | 整字内的**字节**（唯一：探测器数据 1） | `raw >> 8` / `raw & 0xFF` − 55 | 界面/报表的"烟雾 dB/M、温度 ℃" | 做（不在南向仓库） |

> 即：**G-6 延后的是"把整字拆成两个遥测点"**（避免配置/带宽爆炸与语义不可信），而**事件层用的是位/枚举语义** —— 位定义表（PRD §9.5.4）是厂方**无歧义**给出的，与 G-6 的两个"不可可靠拆"候选点无关。**唯一涉及字节拆解的点（探测器数据 1）在事件层被显式排除**（它不产事件），故三层零重叠。

### 11.11 测试策略（AC / RC 在设计层的落点）

#### 11.11.1 可测接缝（三层，逐层可独立断言）

| 层 | 接缝 | 能断言什么 |
|----|------|------------|
| L1 纯函数 | `RegDecode::decode` / `unpack_bits` / `points::expand` / `validate_*` / `soc_in_domain` | AC-2 的**全部数值**、AC-3 位序、AC-1 ③ 逐条拒绝、AC-6 点产出/命名 |
| L2 总线注入 | `MockBus::{put, put_input, put_bits}` + `fail_*_once` + `calls` | AC-2 的**端到端**解码（canned 寄存器 → 点位值）、AC-4 隔离/退避、AC-5 分发 |
| L3 装配 | `SouthScheduler::new(cfg, buses, FakeSink)` + `tick_once`（既有测试驱动） | 站级分发（`on_battery_soc`/`on_grid_package` 是否被调）、位点/信号变化沿（含**消防字级信号进入与退出活跃**、预留位零事件、恢复后不刷事件）、SOC 越界事件、消防登记数与探测器地址序事件 |

#### 11.11.2 AC 逐条落点

| AC | 用例落点 | 要点 |
|----|----------|------|
| **AC-1** | `mupc-southd/tests/s3b2_config.rs` + `tests/fixtures/south_stations_s3b2.yaml`（= PRD §9.4.1 六站 YAML） | ① **既有字段子集**（剥离 `parity`/`discrete`/`byte_swap`/`points`/`offset`/`word_order` 后）解析通过 + 逐条核对既有拒绝条件不触发（其中 `grid_meter` 逐字一致 ⇒ 必过）② 完整 YAML 解析通过且除 §9.4.3 明列条件外无拒绝 ③ **§9.4.3 的 17 条 + 设计补落点的 2 条（规则 18/19）逐条**各一个最小坏配置 → `Err`。**必含的边界用例**（上轮评审打回的直接对应项）：<br>• 规则 15 —— **正向**：`tests/fixtures/south_stations_s3b2.yaml`（六站）与**仅含 `grid_meter` 六相量块**的配置均 `Ok`（**"六站必然通过第 15 条"的可执行断言**，见 §11.5.2(2)）；**反向**：两个**都声明 `points`** 的相邻块、合并后 `count ≤ 120` 且无 `read_slice` → `Err`；同一对块任一方标 `read_slice: true` → `Ok`；两块合并后 `count > 120` → `Ok`（fire 形态：127）；**相邻的一方未声明 `points`** → `Ok`（grid_meter / fire 形态）；**两块都未声明 `points` 且严格相邻**（grid_meter 形态）→ `Ok`；**`discrete` 块相邻** → `Ok`（保守读法，Δ-8）<br>• 规则 15 的**判据 ③（合并后空洞 ≤ 4）**单列一个用例即可（**核心断言 = "③ 由规则 11 恒成立"**：构造两个各自合法、地址连续的 `points` 块，断言 `Ok`；该用例同时是 §11.5.2(1) 等价性证明的钉子）<br>• 规则 6 —— ① 命中行而 `sym_src` 空 → `Err`；② `offset` 与登记值不等 → `Err`；**③ 负向：`addr` 改基准（RC-3 形态）使 `lookup` 无行、且 `offset ≠ 0` → `Ok`（"无行不拒"的可执行断言）**<br>• 规则 18 —— `pcs` 站 `interval_ms: 499` → `Err`、`500` → `Ok`；规则 19 —— 32 位块 `count: 3` 且无 `points` → `Err`、`count: 4` → `Ok`、`discrete` 块 `count: 31` → `Ok`<br>• **规则 10（点名唯一）—— v1.4 补一个"只有它能拒"的形态**：两个**块名不同**、展开后 `metric` 相同的配置（如两块各声明 `points: [{ at: 1, name: soc }]`，或一块的显式 `name` 与另一块的自动位置点名相撞）→ `Err` 含"点名重复"。**注意与 `meter_grid` 块名唯一的分工**：同名块（两块都叫 `p`）由**既有块名唯一规则**先拒（文案"块名重复"，见 §11.5.3.4.1），本用例**不得**用它退化替换 |
| **AC-2** | L1 `meter_regs`/`points` 表驱动 + L2 `tests/s3b2_decode_e2e.rs` | 逐设备抽样，期望值取 §11.6 表（全部已回原文复算）；**必含** BMS 116 raw=65535 → +4953.5（与 `int16` 的 −1600.1 不等）、PCS 32 位 `lo_hi` 6563.6（与误用 `hi_lo` 不等） |
| **AC-3** | L1 `rs485-plugin`（`unpack_bits` 单测）+ L2 点位名映射 | 288 位逐位比对（含跨字节边界、非 8 倍数尾部）、31 位（末字节高位不污染）、`bms_alarm_225` = 位 424 = 簇一级告警、`hvac_di_1` = 位 0 |
| **AC-4** | L3 scheduler | `pcs` 站超时 → 该站 offline + 事件、同口其它站不受影响；退避 `interval << min(n−1,5)` 封顶；恢复 online 一次；`role_priority(Pcs) = 1`（不与 grid/battery 同档） |
| **AC-5** | L3 scheduler（`FakeSink` 记录调用） | battery 注入合法 SOC → `on_battery_soc` 被调且为百分数；注入越界（65535）→ **不调** + 事件 + telemetry 保留原值；`pcs` 站任何输入 → 不调 `on_battery_soc`/`on_grid_package`；`meter_grid` 之外不推进闸门（core-bin 侧单测断言 `last_data_ts` 不被刷新）。<br>**v1.3 事件侧扩展（承接 PRD §9.6.1 的 `fire` 行"events + SSE"，PRD 的 AC 表未单列，属需求承接、一并上报）**：<br>• **消防字级信号**（§11.4.7.1）—— 注入 `fire_sys_1 = 0x4400`（bit14 主电故障 + bit10 压力传感器故障）→ 恰产 **2** 个事件（`fire_sys_1@main_power_fault`、`fire_sys_1@pressure_sensor_fault`），**不产**其它位的事件；下一轮注入 `0x0000` → 恰产 **2** 个"退出活跃"事件（`value = 0.0`）；**首轮只建基线、不产事件**；站从 offline 恢复后首轮同样**不产**事件（`reset()` 生效）<br>• **枚举跃迁**：`fire_sys_6`（火警状态）= 0 → 1 → 2 逐轮注入 → 产 `level1`（进入）与 `level2`（1→2 的**活跃值间跃迁**）；2 → 0 → 产 `level2` 退出<br>• **预留位不产事件**：`fire_sys_3 = 0x0004`（bit2 点型，预留未启用）→ **零事件**（PRD §9.7.6），但 telemetry 仍落 4<br>• **探测器总状态**：`fire_sys_9`（探测器 1 状态）bit12 → `fire_sys_9@alarm`；`fire_det_2`（探测器 2 状态，整字）= 0x4000（bit14）→ `fire_det_2@fault`<br>• **不产事件**：`fire_sys_2`（钢瓶气压）任何取值（含恒 0）→ 零事件；探测器 bit0–4 传感器位 → 零事件<br>• **地址序（Q-9）**：探测器 `+0` 地址寄存器注入非升序（如 3,2,4）→ 产 `fire_detector_addr_order_invalid`；升序 → 不产<br>• **位块仍只上升沿**：BMS `bms_alarm_*` 注入 1 → 0 → **不产**"退出"事件（与消防双向形成对照，钉住 §11.4.7.1 的不对称决策）|
| **AC-6** | L1 `points::expand` 表驱动 | ① 无 `points` 块按"每值槽 1 点"（`bms_term` → 4 点、`bms_alarm` → 288 点）② 有 `points` 块只产列出点（`bms_meta` 8 点，188/190 不产）③ 点级 `count: N` 连续产出（`bms_io` `at:8,count:8` → `bms_io_8..15`）④ 点级 `name` 覆盖（`soc`、**`fire_det_count`**）⑤ 32 位点占 2 寄存器产 1 点（`pcs_3zone` 76 → **72 点**，`pcs_3zone_43` = 1042–1043）⑥ 单寄存器点必产 1 点 ⑦ **无 `points` 的 32 位块产 `count/2` 点**（`count: 6` → 3 点，即 §11.5.3.3 第 8 条的 mapper 用例）|
| **AC-7** | 工程门禁 | `cargo build --release` 无警告 / `cargo clippy` 无 Error / `cargo test` 全绿 / `cargo fmt` |

#### 11.11.3 RC 的代码/配置准备（设计侧已就位，现场只做裁定与回写）

| RC | 现场动作 | 回写项 | 设计侧准备 |
|----|----------|--------|------------|
| RC-1 逐点比对 | 每站 ≥10 点与设备显示/厂方工具比对（**必须点名** BMS 116/186，另加 ADL400 0x0092、BMS 2991–2994） | 不一致 → 改配置（**禁止**代码加设备特判） | `POINT_REGS` + `label` 即核对清单；`tests/point_table_vs_reference_config.rs` 保证表↔配置不漂移 |
| RC-2 PCS 字节/字序 | 1010/1018 判据裁定；32 位电量与显示比对 | `byte_swap` / `word_order` 结论；未通过则不配 4 个 32 位电量点 | `byte_swap`/`word_order` 已是显式字段（无隐式特判） |
| RC-3 PCS 地址基准 | FC04 读 `addr=1000` 与 `addr=0` 各一次 | PCS 站 `addr` | 表按 (role, addr) 查，基准变更后退化为"无证据"（不误拒） |
| RC-4 BMS 主从方向 | 按 §2.2 口径实测 ≥1h | Q-1 降级/升级 | §11.9（基线 + 切换方案 + 代价） |
| RC-5 消防全量探测器 | 按实际登记数读全量、记单轮耗时 | `fire_det` 块 `count`（分片时加 `read_slice: true`） | `fire_detector_mismatch` 自动告警登记数不一致 |
| RC-6 空调校验位 | 按 D-1 结论落地并实测 | 站级 `parity`（建议 `even`） | `parity` 字段 + 同口一致性校验 + **`Rs485PortBus::open` 的 `parity` 透传**（§11.4.5 末条，v1.4 补 —— 否则现场改配置不生效） |
| RC-7 连续运行 | 全站 ≥1h、丢包 < 0.1% | — | 站级 offline/online 事件 + 退避封顶 |
| RC-8 总线不冲突 | 确认 `ttyS7` 与 intercore 的 `ttyS0` 物理独立 | 若同总线 → **PCS 站不启用**（保持注释） | 跨段互斥校验（沿用） |
| **RC-9 消防钢瓶气压功能有无**（v1.3 补，PRD §9.7.6） | 现场确认该机型**是否有钢瓶气压功能**（读 `fire_sys_2` 是否恒 0）；有 → 用厂方工具核对量值 | 无 → **展示层标"未配置"**（不回写配置；该点**不产事件**） | `cylinder_pressure_configured`（§11.4.6）+ §11.7.3 展示口径已就位 |
| **RC-10 Q-9 探测器地址序**（v1.3 补，PRD §9.10 Q-9） | 读回各探测器组 `+0` 地址寄存器，确认**严格升序且唯一**；核对"第 n 只"= **地址升序第 n 只** | 若顺序/编号与预期不符 → 调整探测器实际地址（或按实际序核对点位名） | `fire_detector_addr_order_violation` 自动产事件（§11.4.6/§11.7.2 第 9 条），现场只需确认 |
| **RC-11 Q-19 消防/空调总线归属**（v1.3 补，PRD §9.10 Q-19） | 现场核对消防主机、空调是否**并接在储能侧 BMS 的 485-2 总线**上（旁证：BMS 位 482「485-2 通讯失联」告警） | 若并接 → **该站不得启用**（§9.3.1"禁双 master 共总线"），改由储能侧转发或重新布线 | 与 RC-8 同构（跨段互斥校验只覆盖 `intercore` 串口，**不覆盖此情形 ⇒ 只能现场核对**，故必列） |
| **RC-12 消防/PCS 设备软件版本核对**（v1.4 补，PRD **§9.7.2 第 3 条** + **§9.8.4**） | 核对① 消防主机软件版本 **≥ V1.72**（消防协议 V1.3.1 的前置要求）、② PCS 协议为 **V1.3**（点表对应该版本）；**两者均无版本寄存器**（只有 BMS 有：输入寄存器 **181** 主控程序版本号，已由 `bms_meta` 的 `at: 1` 点采集）⇒ 只能凭**设备铭牌 / 本机显示屏 / 厂方调试工具**核对 | 版本不符 → **重新取点表**，**不得**按现表凑读（§9.7.2 第 3 条）；核对结论记入投运记录（与 RC-1 同批） | **刻意无代码落点**：系统内**不得**声称"读到消防/PCS 版本号"（§9.8.4 明文；PCS 1017 是**故障告警代码**不是版本号）；BMS 侧的可读依据由 `bms_meta` 的 `at: 1`（181）点提供（§11.4.4 登记） |

> **RC-9…RC-12 的来源与编号说明（v1.4 扩至 4 条）**：PRD §9.9.2 的 RC 表只到 **RC-8**，但这四项分别由 **PRD §9.7.6**（钢瓶气压）、**§9.10 Q-9**（探测器地址序）、**§9.10 Q-19**（消防/空调总线归属）、**§9.7.2 第 3 条 + §9.8.4**（消防/PCS 版本核对，v1.4 补）明文要求（前两项还需**展示层口径**与**交叉校验**落地，RC-12 明确**不得**在系统内落代码）。本设计**续号补充**并**上报需求侧回写 PRD §9.9.2**（§11.12.2 Δ-6），不擅自改动 PRD 的 RC 编号体系。

### 11.12 风险、待决项与 PRD 差异上报

#### 11.12.1 本设计新增/变更的字段与契约（须评审追认）

| 项 | 层级 | 来源 | 用途 | 若不追认的退路 |
|----|------|------|------|----------------|
| 1 `read_slice` | 块级（缺省 false） | **PRD §9.4.2.4 已正式补登（v1.7，Δ-3）** —— 本设计改为"落地 PRD 定义"，不再是设计新增 | 豁免第 15 条（设备单次读上限导致的分片，Q-6/Q-7 现场才知） | 不适用（PRD 已定）；若需求侧撤回该字段，退路 = 第 15 条改为"仅警告不拒"（弱化 PRD） |
| 2 点名 `fire_det_count` | 点级（消防 `fire_sys` 的 `at: 7`） | **PRD §9.4.1/§9.5.4 已正式定名（v1.7，Δ-4）** —— 不再是设计补充 | 消防登记数交叉校验（PRD §9.5.4"强制"） | 不适用（PRD 已定）；撤回则退化为"硬编码消防地址 10 / 块名 `fire_det`"（**违背** PRD 反设备特判取向）或不做该校验 |
| 3 `name` 与 `count > 1` 并存 → 拒 | 校验规则 | **PRD 未定义** | 保守口径（禁用而非发明） | 允许并取"首点命名、其余位置命名"（语义模糊） |
| **3b** 无 `points` 块的 `count % width != 0` → 拒（规则 19） | 校验规则 | **PRD 未定义**（设计补充，护栏） | 防"尾槽静默不产点"（§11.4.3） | 改为加载期告警（不拒），或允许静默（**不建议**：与本 PRD 反复整治的"静默失真"取向相反） |
| 4 `MAX_SINGLE_READ_REGS = 120` | 常量 | PRD §9.5.4 的分片口径 | 第 15 条判据 + 分片口径统一 | 取标准 125（与 PRD 分片口径不一致） |
| 5 **`TelemetrySample.kind` / `BlockData::{Regs,Bits}` / `SocOutcome`** | 内部类型 | 设计选择 | 位点变化沿过滤 + "点产出/落库节流"分层 + Battery 四种情形显式化（§11.4.6） | 两条 mapper 入口（前者）；Battery 语义继续隐式（后者，**不建议**，上轮评审正因语义未写而打回） |
| **6 消防字级信号（`SignalSpec` / `@信号键` 事件名 / 双向事件 / "入表即产事件"）** | 内部类型 + 事件命名契约 | **PRD 只要求"消防 → events"（§9.6.1）与"告警位 → events"，未定义信号的表达形态、事件名语法、单向/双向与"哪些位入表"** | 补齐消防事件落点（P0-3） | ① 若评审不接受 `@` 语法 → 改用 `<点名>.<信号键>`（换分隔符，语义不变）；② 若评审要求**位块也双向** → 统一双向 + 事件节流（代价：BMS 288 位在"全 1 复位"时刷新 288 条恢复事件，须加去抖窗口与事件保留策略复核）；③ 若不接受"探测器只取 bit12/bit14" → 展开到 bit0–4（代价：信号源数 ×5）；④ 若不接受"`signals` 只登记产事件者（无 `class` 字段）" → 恢复 `SignalClass{Alarm,State}` 并在表中登记 State 行（代价：出现"登记了却不产事件"的歧义，且需为极性反转位补判据语义） |
| **7 规则 18（`pcs` ≥ 500ms）** | 校验规则 | **PRD §9.3.2.2(2) + §9.8.1 末条明文要求，但未进 §9.4.3 表** | 设计侧补落点 | 不适用（PRD 已要求）；建议需求侧把该条**补进 §9.4.3 表**（现表 17 条 → 18 条），使 AC-1 ③ 有明确出处 |

#### 11.12.2 PRD 差异上报（**需求侧待订正，设计与实现按下列口径执行**）

| 编号 | 差异 | 事实 | 本设计执行口径 |
|------|------|------|----------------|
| **Δ-1**（**已闭合**） | PRD §9.4.2.4 点级 `format` 行注"⚠️ 缺省是 `int32_scaled`" | **代码事实是 `Float32`**（`config.rs::default_reg_format`，且既有单测 `default_reg_format_used_when_omitted` 钉住） | **PRD v1.7 已订正为 `float32` 并补登"本轮新增块一律显式声明 `format`"护栏** ⇒ 差异闭合。本设计**维持 `Float32` 缺省**（改缺省会改既有 S3a 行为、破坏既有单测与 §9.4.1 第 6 站语义），并把该单测列入"**断言不得改**"（§11.5.3.4 C3） |
| **Δ-2**（**部分闭合**） | PRD §9.4.3 把符号性规则 ① 标为"本轮不可机械校验、不纳入 AC-1" | 有了 §11.4.4 的 `POINT_REGS`（含 `sym_src`），① 变为**可机械判定** —— 但**只对"命中的行"成立**：v1.3 按 P0-2 裁定，**查不到行 → 不拒**（现场 RC-3 合法改 `addr` 基准），故 ① 的形态为"**命中而 `sym_src` 空 → 拒**" | **实现 ①（收窄形态）**（不改变 AC-1 清单，故不违反 §9.4.1 的 YAML ⇒ AC-1 ② 仍成立）。是否把 ① 纳入 AC-1 ③ **属需求侧决定**，本设计不擅自改 AC；**请需求侧注意**："无行不拒"是 ① 的**能力边界**（可追溯性由 ② + RC-1 兜底），PRD §9.4.3 的表述宜同步说明 |
| **Δ-3**（**已闭合**） | PRD 第 15 条"块落地极大性 → 拒"与 §9.5.2/§9.5.4 的"现场分片"张力；以及第 15 条**作用域**未限定（会误拒 `grid_meter`） | PCS 单次读上限（Q-6）、消防上限（Q-7）**均"文档未明确"** ⇒ 分片与否现场才定；且既有 `grid_meter` 六块严格相邻 | **PRD v1.7 已补登 `read_slice`（Δ-3 落地）；v1.8 按项目经理裁定补明第 15 条的适用域（仅作用于声明了 `points` 的块）与判据（合并后 `count ≤ 120`）** ⇒ 差异闭合。本设计 §11.5.1 #15 / §11.5.2 与之一致，并给出"六站必然通过"的证明 |
| **Δ-4**（**已闭合**） | PRD §9.5.4 登记数交叉校验为"强制"，但参考配置未给该点的可识别形态 | `fire_sys` 的点均无 `name` | **PRD v1.7 已正式定名 `fire_det_count` 并在 §9.4.1 的 `at: 7` 声明该名** ⇒ 差异闭合。本设计 §11.4.6/§11.4.7 按**点名**查找（禁按块名 + 硬编码地址） |
| **Δ-5**（**新，v1.3**） | **规则 18（`pcs` 站 `interval_ms ≥ 500ms`）在 PRD 有明文要求（§9.3.2.2(2)、§9.8.1 末条），但未进 §9.4.3 的 17 条表** | AC-1 ③ 的措辞是"逐条触发 §9.4.3 的拒绝条件" ⇒ 该条**没有 AC 出处** | 本设计**补落点并纳入 AC-1 ③**（§11.5.1 #18、§11.11.2），标注"非 §9.4.3 表内条件"。**建议需求侧把该条补进 §9.4.3 表**（17 → 18 条） |
| **Δ-6**（**新，v1.3；v1.4 扩列**） | PRD §9.6.1 要求 `fire` 的数据进 **events + SSE**，但 **PRD §9.9.1 的 AC 表没有对应验收项**；同理 **§9.7.6 的钢瓶气压展示口径、§9.10 的 Q-9/Q-19、§9.7.2 第 3 条 + §9.8.4 的消防/PCS 版本核对，均未进 §9.9.2 的 RC 表** | 需求侧确有要求（上述各条均为明文），但**验收侧无落点** | 本设计在 §11.11.2（AC-5 事件侧扩展）与 §11.11.3（**续号 RC-9/RC-10/RC-11/RC-12**）补齐落点并上报；**建议需求侧回写 PRD 的 AC/RC 表**（本设计不擅自改 PRD 编号体系） |
| **Δ-7**（**新，v1.3；已闭合，仅登记过程**） | PRD §9.4.3 第 15 条判据若只按 **v1.6 原文**（"合并后空洞 ≤ 4"）实现，会把 `fire_sys`+`fire_det`（空洞 0、合并 127 > 120）判为"应合并"⇒ 与 §9.5.4"必须分片"直接冲突 | 单一判据下"空洞小"并不等于"合并不超设备上限"，结论方向反了 | **PRD v1.8 已补 ④"合并后 `count ≤ 120`"并保留 ③，同时限定适用域** ⇒ 差异闭合。本设计**四条判据全实现**（§11.5.1 #15），并证明 **③ 在适用域内由规则 11 恒成立**（§11.5.2(1)）⇒ 与 PRD **行为等价**；实现顺序建议先判 ④（提前放行"本就该分片"的形态） |
| **Δ-8**（**新，v1.3；备案项，非阻塞**） | PRD v1.8 第 15 条的"`discrete` 位块是否参与合并判定"**字面有两种读法**（正文把位块列为"不参与"的例子，但 ②/③ 又出现"`discrete` 按位地址同理""位块不受 ③ 限制"） | 本设计取**保守读法**（`discrete` 一律不参与）；两种读法在本轮**判定结果完全相同**（§9.4.1 的两个位块均未声明 `points`） | 按保守读法实现；**请需求侧在 §9.4.3 第 15 条补一句消歧**（例如"声明了 `points` 的 `discrete` 位块是否参与：不参与/参与并如何折算位数为寄存器数"）。本设计不擅自改 PRD |
| **Δ-9**（**新，v1.4；须需求侧追认**） | PRD **§9.8.4「事件」栏要求「PCS 32 位电量比对未通过（若配置）」产事件** | 该"比对"是 **RC-2 的现场人工比对**（把 1042–1045/1072–1075 的累计电量与设备显示/大屏读数比对，§9.7.3），**系统内没有参考量可判**；而 PRD §9.7.2 第 2 条又明令"本轮**不做自动量程校验**（除 SOC 外）" ⇒ 运行期**不存在可落地的判据**（既不能自动比对、又不允许量程自校验） | **本设计以 RC-2「未通过则不配 4 个 32 位电量点」替代**（落地口径见 §11.7.4 迁移清单末行"删点不删块"；现场动作见 §11.11.3 RC-2）——即**用投运前的显式裁定替代运行期事件**：比对不通过 ⇒ 这些点根本不进配置 ⇒ 系统内不存在"不可信数据"，也就无需"比对未通过"事件（这正是 PRD §9.7.3"该比对通过前 32 位电量视为不可信"的形态）。**本轮实现口径 = 不产该事件**（§11.4.7 事件产出表 ⑦）。**登记为与 PRD 的差异，需需求侧追认或订正 PRD §9.8.4**：二者择一 —— a. **接受本口径**，把该事件从句中删除（或改写为"RC-2 未通过 ⇒ 不配点"，与 §9.7.3 表述对齐）；b. **坚持要事件**，则须由需求侧给出**系统内可判的判据**（例如"该 4 点已配置且值域/单调性校核失败"），本设计再据以补落点 |

#### 11.12.3 实现风险（开发期须注意）

| 风险 | 说明 | 缓解 |
|------|------|------|
| R-1 既有测试/构造点连锁订正 | ① **15 处** YAML fixture / 内联 YAML 需改构造数据（含 A1–A13 + **A15/A16**，后者是 v1.4 从"期望 `Err` 类"移入 ① 的两例；其中多数是"battery 补含 `soc` 点的 `regs`"、1 处 `fire` `addr` 改非 0、2 处 mapper/scheduler fixture 改点名式）；② **`RegBlockConf` 字面量 10 处 + `StationConf` 字面量 8 处**（逐个核实，非 v1.2 的"≈13 处"）需机械补新字段；③ `BlockReads` 改为承载 `BlockData::{Regs,Bits}` 后，`grid_convergence.rs` 与 mapper/scheduler 单测的 canned 构造点需同步；④ `telemetry_points` 签名/返回类型变更（`(String,f64)` → `TelemetrySample`），其单测**断言与测试名**需订正（§11.5.3.3 第 8 条）；⑤ **9 处 metric 更名导致的断言订正**（§11.5.3.3） | **完整分类清单见 §11.5.3**（fixture 订正 / 断言订正 / 断言不得改，逐项到 `file:行`）。执行顺序：先做类型改造 + 机械补字面量 + 改 fixture（**在一个提交内完成**），再按 11.5.3.3 订正断言期望值；`§11.5.3.4`（C1–C7）列出的断言**一个都不许改** |
| R-2 表↔配置漂移 | `POINT_REGS`（展开后 618 行，探测器区为模板）与配置双份数据 | `tests/point_table_vs_reference_config.rs` 双向核对（§11.4.4）+ 行数断言 |
| R-3 需求未定项的现场回写 | Q-4/Q-16/Q-20/RC-2/RC-3 会合法改 `addr`/`format`/`word_order`；**RC-3 改 `addr` 基准后 `POINT_REGS` 的键会整体失配** | 这些字段**不做**强校验（只作用于 `offset`），且 **`lookup` 查不到行 → 放行**（v1.3/P0-2 裁定）⇒ 现场校准**不会**被启动期拒；漂移由 `tests/point_table_vs_reference_config.rs`（参考配置侧）与 RC-1 逐点比对兜底 |
| R-4 位错 1 位 ⇒ 全表语义偏移 | FC02 位序（bit0 = LSB） | `unpack_bits` 单测（PRD 原文示例 0x3B/0xBB）+ AC-3 逐位 + RC-1 抽点 |
| R-5 618 点落库量 | 模拟量 ≈300 行/s；位点按 D2 稳态≈0 | retention 天数投运前复核；位点稳态零写 |
| **R-6 消防信号误报/漏报**（v1.3 新增） | 信号 mask/活跃集写错 → 火警事件误报（虚假停机判据）或漏报；`bit15 = 0 表示离线` 的**极性反转**位最易写错 | ① 信号清单逐条对表 PRD §9.5.4（§11.4.7.1 表）；② AC-5 事件侧用例**逐信号钉住**（含"预留 bit2 零事件""bit15 不入事件"的负向断言）；③ `EdgeTracker` 首轮/恢复后只建基线不产事件（防"上线即刷事件"）；④ 探测器地址序异常时点位标记不可信（防空地址乱序下的误判） |

### 11.13 实施顺序（建议 Task 拆分，供项目经理排期）

| # | Task | 内容 | 闸门 |
|---|------|------|------|
| T1 | 解码原语 | `RegFormat` 扩展 + `RegDecode` + **`WordOrder`（唯一定义）**；`decode_regs` 薄包装 | `meter_regs` 单测全绿（含既有 6 例不变）+ 新增 AC-2 L1 向量 |
| T2 | 帧层 FC02 | `unpack_bits` + `parse_bits_response` + `read_discrete_inputs_from` | `unpack_bits` 单测（PRD 示例 + 31 位/288 位） |
| T3 | 配置与校验 | §11.4.1 结构 + §11.5 **十七条 + 规则 18/19** + `read_slice`；**含 ① fixture 订正（§11.5.3.2）与 ② 断言订正（§11.5.3.3）**；`validate_maximality` 按 §11.5.2(1) 的四条判据实现（**建议判定顺序：先 ④ `count > 120` → 放行；再 ③ 空洞；再 ① ② 与 `read_slice`**，使"本就该分片"的形态提前放行） | **AC-1 ①②③ 全绿**（含 §11.11.2 AC-1 ③ 的规则 15/6/18/19 边界用例）；**既有用例经 §11.5.3 分类订正后全绿**——注意 T3 的"DoD"不是"既有用例一字不改"（v1.2 的该措辞不准确，见下注） |
| T4 | 点展开与点表 | `points.rs` + `point_table.rs`（探测器区**模板 + 运行期展开**，n=20 展开 618 行）+ 消防 `SignalSpec` 表 | AC-6 全绿；表↔配置交叉核对绿；**消防信号清单与 PRD §9.5.4 位定义逐条对表** |
| T5 | 总线与调度 | `StationBus::read_discrete`/MockBus；scheduler 分发 + **统一 `EdgeTracker`（位点 + 字级信号）** + `role_priority(Pcs)` | AC-3/AC-4 绿；**消防信号正向（进入/退出/枚举跃迁/地址序）与负向（预留位零事件、气压零事件、位块无退出事件）用例见 AC-5 事件侧** |
| T6 | mapper 与消费 | `telemetry_points` 重构（逐点/每值槽）、**`soc` 点名 + `SocOutcome` 四情形**、`Pcs` 臂、SOC 域检查、消防登记数 + 地址序 + 钢瓶气压口径；core-bin `SouthSink` 事件 message | AC-2 L2 / AC-5 绿（含事件侧扩展） |
| T7 | 配置迁移与文档 | `deploy/config/*.yaml` 换 §9.4.1 的 6 站（PCS 保持注释）；确认本设计的落地状态（**不新增构建参数，`build.md` 不改**） | AC-7 + `grid_convergence.rs` 回归锚（**断言零改动**） |

> **T3 闸门措辞订正（v1.3，上轮评审 P0-4 的直接后果）**：v1.2 写的是"既有用例全绿"＋回归闸门写"三者零破坏（**仅** §11.5.3 列出的 fixture 数据订正）"——该口径**不成立**：metric 更名（`temp` → `temp_1`）会让 `scheduler.rs` / `mapper.rs` 的**断言期望值**必须改（§11.5.3.3，共 9 处），这不是"fixture 订正"。本版统一为：**"经 §11.5.3 的三类分类处理（fixture 订正 / 断言订正 / 断言不得改）后全绿"**，并在 §11.5.3.4 明确"不得改"的回归锚清单。

**回归闸门（v1.3 精确化）**：
1. **`mupc-southd/tests/grid_convergence.rs`** —— meter_grid 语义逐字段等价，**只改 `block()` 构造的字段补齐，断言一个字节都不动**（§11.5.3.4 C1）；
2. **`config.rs` 的 meter_grid-only 系列 + 解析类系列**（§11.5.3.4 C2/C3）—— 断言不动；
3. **`core_config.rs` 的 6 个 southern-stations 用例** —— 断言不动（只改 fixture，§11.5.3.4 C4）；
4. **`scheduler.rs` 的调度/退避/隔离/SOC gating 断言**（§11.5.3.4 C5）—— 不动。

> 即：**"零破坏"的对象是"断言语义"，不是"逐字不动"** —— 因 PRD §9.4.2.1 第 4 条（每值槽 1 点 + 位置式命名）而必须改的期望值属**规则变更的传导**（§11.5.3.3 已逐处列明），其余任何断言变红都按**回归**处理。

### 11.14 一致性声明

| 关联 | 关系 |
|------|------|
| PRD §9（**v1.8**，`[REVIEWED: PASS: 2026-09-21]`；第 15 条已按项目经理裁定订正适用域与判据） | 本章是 §9 的实现级落地；§9.4.3 的 **17 条 + 设计补落点的 2 条（规则 18/19）**逐条落点见 §11.5（**第 15 条四条判据的完整口径、"③ 恒成立"的等价性证明与"六站必然通过"的逐站证明**见 §11.5.2）；G-1…G-6 落点见 §11.1（含**消防事件**这一非 G-项落点）；Q-1 处理见 §11.9；**G-6 替代口径与"消防事件层"三层边界**见 §11.10；**消防事件统一模型（信号/`EdgeTracker`）**见 §11.4.7.1；既有测试连锁订正的**三类分类清单（fixture / 断言 / 不得改）**见 §11.5.3；PRD 差异上报（**Δ-1…Δ-9**）见 §11.12.2（Δ-1/Δ-3/Δ-4/Δ-7 已闭合；**Δ-8 为待需求侧消歧的备案项**；**Δ-9 为 v1.4 新增、须需求侧追认**：§9.8.4 的"PCS 32 位电量比对未通过 → 事件"由 RC-2「未通过则不配点」替代，本轮不产该事件） |
| 本文档 §10（S3a） | 调度架构/故障隔离/role 分发骨架**不变**；本章只在 §10 预留接缝上扩展（§11.3 扩展点列），并对 §10.6"role 映射"与 §10.8"文件结构"作**增量补充**（不回改原条款） |
| PRD §9.11 ① | 已明确要求设计文档至少覆盖：`RegBlockConf.points` 结构与校验（§11.4.1/§11.5）、`StationConf.parity`（§11.4.1，D-1 裁定 ①）、位块点落库口径（§11.7.2）——**三项均已落实** |
| 核间 10 §2.4 / `2026-09-08-S2-DI-DO安全联锁.md` | PCS 只读 + 4 区 500 停机原语归属见 §11.8；数据面边界 ADR-012 维持 |
| 04 策略引擎 §2.11.1 | SOC 双源（BMS 优先/超期回落核间）链路不变（§11.7.1、§11.9.3） |
| 项目 CLAUDE.md | 无硬编码密钥；无新增 `unsafe`；错误类型实现 `std::error::Error`；不新增独立设计文档 |

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
| FC02 / FC03 / FC04 | Modbus 功能码：读离散输入 / 读保持寄存器 / 读输入寄存器（§11.4.5） |
| 读窗口（块） | 一次 Modbus 事务覆盖的连续地址区间，只承载**传输口径**（`func`/`addr`/`count`/`byte_swap`）与缺省换算（§11.2.1） |
| 点位（point） | 一个物理量的**换算口径**（`format`/`scale`/`offset`/`word_order`/`name`）+ 其遥测键（metric）；块内 `points[]` 逐点声明（§11.4.1/§11.4.3） |
| 位块 | `func: discrete` 的离散输入区间，逐位产出 0/1 点（§11.4.3/§11.4.5） |
| 点表登记注册表 | `mupc-southd::point_table::POINT_REGS`：`(role, addr) → {format, scale, offset, 符号性来源, 分类, 中文名, signals}`，按 n=20 参考配置**展开后 618 行**（探测器区为 6 条组内模板 + 运行期展开）由 PRD §9.5 逐点转写；用作配置期一致性校验的期望值来源、RC-1 核对清单与消防事件源（§11.4.4/§11.4.7.1） |
| 变化沿落库 | 位点仅在与上一轮取值不同时写 telemetry（稳态零写），`Alarm` 位另产上升沿事件（§11.2.4 D2 / §11.7.2） |
| **信号（Signal）** | 一个**可跃迁量**：离散位（`Bit`）或保持寄存器整字内的**位/枚举**（`WordBit`/`WordEnum`）。四类信号共用一套跃迁记忆与事件产出路径，差别仅在"活跃判据函数"（§11.4.7.1） |
| **`EdgeTracker`** | scheduler 内按站维护的"上轮活跃态"记忆表，同时承载离散位与字级信号；站恢复后 `reset()`（首轮只建基线、不产事件）（§11.4.7） |
| **`read_slice`** | 块级字段（PRD §9.4.2.4，v1.7 补登）：该块是"按设备单次读上限主动分片"的结果，**仅**豁免第 15 条极大性检查（§11.5.2） |

---

## 附录：版本演进

> 正文已整合全部历史补丁，本表仅作演进追溯。

| 版本 | 主要变更 |
|------|----------|
| v1.0 | 初版：定义南向通信模块实现级设计（RS485/HPLC/插件系统） |
| v1.1 | BECG-3568 站级多从站统一调度（S3，§10）：`mupc-southd` 调度器、`south_stations` 配置、role（meter_grid/meter_batt/battery/hvac/fire）映射、master_meter 收敛 meter_grid、口内多从站轮询与站级故障隔离、DeviceRegistry 接线、SOC 源融合接口（04 §2.11 注） |
| **v1.4（2026-09-21，按二轮设计评审意见修订，三轮设计评审通过 —— 本章当前版本）** | **只改 §11；小范围收尾，不推翻 v1.3 已获认可内容（P0-1 的 #15 重写与"六站必然通过"结论、P0-2 的"无行不拒"、P0-3 的统一事件模型与消防信号清单、P0-4 的三级清单结构、规则 18/19、`SocOutcome`、`WordOrder` 单一定义、RC-9/10/11 一律保留）；不自行加 `[DESIGN_APPROVED]` 标记。** 修订内容：① **事实错误订正** —— §11.5.2(2) 的 `grid_meter` 行原按**单测 fixture（`config.rs:324-329`）**写成 `p→p_total→q→pf→u→i` @0x1000/0x1006/0x1008/0x100E/0x1014/0x101A（并误称"逐字取自 PRD §9.4.1"），现按**生效配置真值**（`deploy/config/mupc_core_config.yaml:143-148` 与 `.production.yaml:148-153`，二者逐字相同，且与 PRD §9.4.1 第 6 站一致）重写为 `p→q→pf→u→i→p_total` @0x1000/0x1006/0x100C/0x1012/0x1018/0x101E，并新增"**取值真源 / 不得取证于 fixture**"注、逐站复核其余 5 站（无同类错误）；② **C2 与规则 10 的顺序冲突钉死** —— 新增 §11.5.3.4.1 给出**实现判定顺序约束**（既有 `meter_grid` 校验整组先于 `validate_station_regs`）与文案，**C2 维持"断言不得改"（不订正）**，并说明规则 10 独有的覆盖面（补 AC-1 ③ 用例）；§11.5.1「顺序」同步加约束；③ **Δ-9 登记** —— PRD §9.8.4「PCS 32 位电量比对未通过 → 事件」无运行期落点（比对是 RC-2 现场人工比对，且 §9.7.2 禁本轮量程自校验）⇒ 以 RC-2「未通过则不配点」替代，§11.12.2 登记 Δ-9 并标注**须需求侧追认或订正 PRD**，§11.4.7 事件表 ⑦ 与 §11.7.4 给出**一眼可见的处置**；④ **建议一并改净** —— `bms_alarm_225` 示例订正为"簇一级告警"（位 424；原写"簇 SOC 低·轻"混淆位地址与点名）、`RegBlockConf` 字段数 9→13 订正为 **6→10**、A14"三个 crate"订正为 **1 个（`mupc-southd`）**、A1 的 `fire_1` 行号 `:345`→**`:348`**、§11.5.3.5 两个需补 `regs` 的用例**移入 ①（A15/A16）**且该节改号 **④**、**删除悬空符号 `emit_falling_edges(EmitBidi)`**（改述双向性的落地形态）、补 **`Rs485PortBus::open` 的 `parity` 透传**（§11.3/§11.4.5/RC-6）、补 **RC-12**（消防/PCS 设备软件版本核对，§9.7.2 第 3 条 + §9.8.4）并更新 Δ-6。 |
| **v1.3（2026-09-21，按首轮设计评审意见修订，已由 v1.4 收尾）** | **只改 §11，不动 §10/§11 已获批准的部分；不自行加 `[DESIGN_APPROVED]` 标记。** 修订内容：① **P0-1** —— §11.5.1 第 15 条**重写落点**（适用域限定为"两块都声明了 `points`" + 判据改为"合并后 `count ≤ 120`" + `read_slice` 豁免），§11.5.2 新增**逐站穷举证明**"§9.4.1 六站（含既有 `grid_meter`）必然通过第 15 条"，并给出"为什么必须限定适用域"的**改名/契约破坏**论证；② **P0-2** —— §11.4.4 取「**查不到行不拒**」（删去 ① 中"无行"半句），写明理由（RC-3 会合法改 `addr` 基准，无行即拒会让合法校准配置启动失败），§11.5.1 #6 同步；③ **P0-3** —— 新增 **§11.4.7.1 统一事件模型**（把"事件源"从离散位推广为四类**信号**，共用 `EdgeTracker`；消防取**双向**事件并说明与 BMS/空调只取上升沿的**刻意不对称**）、**消防信号清单**（系统状态位/火警枚举/烟温可燃/探测器总状态，含"预留 bit2 零事件""钢瓶气压零事件"的负向边界）、§11.7.2 消防事件落点、§11.10 **与 G-6 的三层边界表**（遥测层不拆 / 事件层只做位与枚举判定 / 展示层做字节拆解）；④ **P0-4** —— §11.5.3 **重建为「用例名 + 断言」级**并分**三类**（fixture 订正 / 断言订正 / **断言不得改**），实测规模订正为 `RegBlockConf` **10 处**、`StationConf` **8 处**，点名 9 处 metric 更名断言订正与 C1–C7 回归锚，补报上轮漏掉的两个 `core_config` 用例；⑤ **完整性遗漏** —— 补**规则 18**（`pcs` `interval_ms ≥ 500ms`，落站级基础校验）、**规则 19**（无 `points` 块 `count % width == 0` 护栏）、§11.11.3 补 **RC-9/RC-10/RC-11**（钢瓶气压 / Q-9 / Q-19）并说明续号与上报；⑥ **建议** —— `WordOrder` **单一定义**（落 `meter_regs`，`config` 复用）、**Battery 底块读失败等四情形**（`SocOutcome` 表）、无 `points` 的 32 位块静默少产点护栏；§11.12.2 差异表更新（Δ-1/Δ-3/Δ-4 **已由 PRD v1.7 采纳闭合**，新增 Δ-5/Δ-6/Δ-7）、§11.13 的 T3/T7 与回归闸门口径**订正**。 |
| **v1.2（2026-09-21，设计评审【不通过】，已由 v1.3 修订）** | 新增 §11「站级南向设备语义点表集成（S3b-2）」，落实 PRD §9（v1.6）：① **方案探索**（配置模型 A1/A2/A3、`RegFormat` 扩展 B1/B2/B3、FC02 接入 C1/C2/C3、位块落库 D1/D2/D3、Q-1 口径 E1/E2/E3，逐项给选型与权衡）；② **能力补齐**：`RegFormat::{Uint16,Int16}` + `RegDecode`（G-1/G-2/G-5）、`RegFunc::Discrete` + `StationBus::read_discrete` + `unpack_bits`（G-3）、块内 `points[]` 逐点换算 + `points::expand`（G-4）、G-6 延后口径（§11.10）；③ **新角色** `Role::Pcs`（只读 3 区）与 `role_priority` 中档；④ **17 条配置期规则**逐条落点（§11.5）+ 既有 fixture 订正清单；⑤ **`POINT_REGS` 点表登记注册表**（618 行，PRD §9.4.3 的"校验器内常量表"，兼作 RC-1 机读核对清单与事件中文名来源）；⑥ **位块点落库口径**（变化沿 telemetry + 告警上升沿事件，替代全量 288 行/s）；⑦ Q-1 的书面口径确认、影响面与可切换设计（§11.9）；⑧ 写操作独立 Task 边界与 S2 联锁分工（§11.8）；⑨ AC/RC 三层可测接缝与逐条落点（§11.11）；⑩ 新增字段/契约追认清单与 **4 项 PRD 差异上报**（含 §9.4.2.4"缺省 `int32_scaled`"与代码事实 `float32` 不符，Δ-1）。 |
