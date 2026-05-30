# MUPC 南向通信模块 产品需求文档（PRD）

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.1 | 2026-05-29 | 需求分析师 | **[REVIEWED: PASS]** |

---

## 1. 产品概述

### 1.1 背景与定位

MUPC 微电网特种调控装置通信管理模块是"异构双核心模块主控架构"中的**非实时处理核心**（大脑）。Phase 1 已完成北向通信网关（IEC 104）和核心架构设计，建立了与调度主站的通信能力。Phase 2+ 需要扩展南向通信能力，支持与台区设备（TTU、光伏逆变器、充电桩、柔性负荷、消防控制等）的直接通信。

**南向通信模块职责：**
- 建立统一的南向设备抽象层，支持多协议插件化接入
- 通过 RS485 总线与台区设备通信
- 通过 HPLC（高速电力线载波）与台区设备通信（预留）
- 通过动态插件系统支持协议扩展和运行时加载

### 1.2 目标平台

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux (openEuler 22.03+) |
| 硬件 | RK3588 |
| 编程语言 | Rust |
| Rust 版本 | >= 1.75 |
| 异步运行时 | Tokio |

### 1.3 涉及 Crate

| Crate | 类型 | 说明 |
|-------|------|------|
| **device-trait** | NEW | 南向设备抽象定义（SouthDevice、ProtocolHandler、HplcDriver、Plugin） |
| **rs485-plugin** | ENHANCE | RS485 串口通信驱动，支持协议处理器注入 |
| **hplc-plugin** | NEW | HPLC 宽带电力线载波通信驱动（Mock 实现 + 芯片 SDK 预留） |
| **plugin-loader** | ENHANCE | 动态插件加载器（FFI 绑定、生命周期管理） |

### 1.4 支持设备清单

| 设备类型 | 通信方式 | 协议 | 处理器 |
|----------|----------|------|--------|
| **TTU**（配变终端） | RS485 | 电力行业规约 | TtuHandler |
| **光伏逆变器** | RS485 / 以太网 | 厂商私有协议 | InverterHandler |
| **充电桩** | RS485 / 以太网 | GB/T 27930 / OBC | ChargerHandler |
| **柔性负荷控制装置** | RS485 | Modbus RTU / 私有协议 | ModbusHandler / 自定义 |
| **消防控制系统** | RS485 | Modbus RTU / 火灾报警协议 | ModbusHandler / 自定义（专用 FireAlarmHandler 延后至 Phase 2+） |

---

## 2. SouthDevice 统一设备抽象

### 2.1 SouthDevice Trait

所有南向设备（RS485 / HPLC / 其他）统一实现 `SouthDevice` trait。此 trait 定义在 `device-trait` crate 中。

```rust
use device_trait::{DataFrame, DeviceError, DeviceStatus};

/// 南向设备统一接口
pub trait SouthDevice: Send + Sync {
    /// 获取设备ID
    fn device_id(&self) -> &str;

    /// 获取设备类型
    fn device_type(&self) -> &str;

    /// 获取设备状态
    fn status(&self) -> Result<DeviceStatus, DeviceError>;

    /// 连接设备
    fn connect(&self) -> Result<(), DeviceError>;

    /// 断开连接
    fn disconnect(&self) -> Result<(), DeviceError>;

    /// 读取数据
    fn read(&self) -> Result<DataFrame, DeviceError>;

    /// 批量读取（支持 >= 1Hz 遥测数据上送）
    fn read_batch(&self, count: usize) -> Result<Vec<DataFrame>, DeviceError>;

    /// 写入数据
    fn write(&self, data: &[u8]) -> Result<(), DeviceError>;

    /// 健康检查/心跳
    fn health_check(&self) -> Result<bool, DeviceError>;
}
```

### 2.2 数据结构

```rust
/// 设备状态
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceStatus {
    Online,
    Offline,
    Error(String),
}

/// 设备数据帧
#[derive(Debug, Clone)]
pub struct DataFrame {
    pub device_id: String,
    pub timestamp: u64,
    pub data: Vec<u8>,
    pub quality: DataQuality,
}

/// 数据质量
#[derive(Debug, Clone, PartialEq)]
pub enum DataQuality {
    Good,
    Invalid,
    Reserved,
}
```

### 2.3 设备错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("设备离线: {0}")]
    Offline(String),

    #[error("通信超时: {0}")]
    Timeout(String),

    #[error("数据校验失败: {0}")]
    ChecksumFailed(String),

    #[error("协议错误: {0}")]
    ProtocolError(String),

    #[error("设备忙: {0}")]
    Busy(String),

    #[error("其他错误: {0}")]
    Other(String),
}
```

### 2.4 设备类型枚举

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceType {
    Ttu,
    Inverter,
    Charger,
    FlexibleLoad,
    FireAlarm,
    Hplc,
    Unknown,
}
```

### 2.5 设备注册表

提供统一的设备注册、注销、查询机制。

```rust
/// 设备注册表接口
pub trait DeviceRegistry: Send + Sync {
    fn register(&self, device: Arc<dyn SouthDevice>) -> Result<(), RegistryError>;
    fn unregister(&self, device_id: &str) -> Result<(), RegistryError>;
    fn get(&self, device_id: &str) -> Option<Arc<dyn SouthDevice>>;
    fn query_by_type(&self, device_type: &str) -> Vec<Arc<dyn SouthDevice>>;
    fn list_all(&self) -> Vec<String>;
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("设备已存在: {0}")]
    AlreadyExists(String),

    #[error("设备不存在: {0}")]
    NotFound(String),

    #[error("注册失败: {0}")]
    RegisterFailed(String),
}
```

### 2.6 设备 ID 命名规范

```
格式: {设备类型}_{厂商}_{型号}_{序号}
示例: ttu_huawei_osu_001, inverter_sungrow_sg100_001
```

---

## 3. RS485 设备与协议处理器

### 3.1 架构

RS485 南向通信采用"统一设备抽象 + 协议处理器注入"模式。`Rs485Device` 结构体实现 `SouthDevice` trait，通过依赖注入 `ProtocolHandler` 支持多种设备协议。

```
rs485-plugin/
├── lib.rs                      # 插件入口
├── device.rs                   # Rs485Device 实现
├── config.rs                   # 配置定义
├── errors.rs                   # RS485 错误类型
├── protocol.rs                 # 帧解析 + CRC校验
└── handlers/
    ├── mod.rs                  # 协议处理器注册表
    ├── modbus_handler.rs       # Modbus RTU
    ├── ttu_handler.rs          # TTU 专用协议
    ├── inverter_handler.rs     # 光伏逆变器私有协议
    └── charger_handler.rs      # GB/T 27930 充电桩协议
```

### 3.2 Rs485Device 结构体

```rust
/// RS485 设备（支持协议注入）
pub struct Rs485Device {
    device_id: String,
    device_type: String,
    config: Config,
    port_fd: Mutex<Option<RawFd>>,
    status: Mutex<DeviceStatus>,
    handler: Arc<dyn ProtocolHandler>,  // 注入的协议处理器
}
```

### 3.3 ProtocolHandler Trait

```rust
/// RS485 协议处理器
///
/// 通过依赖注入支持多种协议
pub trait ProtocolHandler: Send + Sync {
    /// 编码请求数据
    fn encode_request(&self, device_id: &str, data: &[u8]) -> Vec<u8>;

    /// 解码响应数据
    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError>;

    /// 获取协议名称
    fn name(&self) -> &'static str;
}
```

### 3.4 协议处理器实现

| 处理器 | 实现协议 | 支持设备 | 优先级 |
|--------|----------|----------|--------|
| `ModbusHandler` | Modbus RTU | 通用 Modbus 设备 | 高 |
| `TtuHandler` | TTU 专用协议（电力行业规约） | 配变终端 | 高 |
| `InverterHandler` | 厂商私有协议 | 光伏逆变器 | 中 |
| `ChargerHandler` | GB/T 27930 | 充电桩 | 中 |
| `FireAlarmHandler` | 火灾报警协议（预留） | 消防控制系统 | Phase 2+ |

#### ModbusHandler 示例

```rust
pub struct ModbusHandler {
    device_addr: u8,
    crc_mode: CrcMode,
}

impl ProtocolHandler for ModbusHandler {
    fn encode_request(&self, _device_id: &str, data: &[u8]) -> Vec<u8> {
        // data: [func_code, addr_hi, addr_lo, ...]
        let mut frame = vec![self.device_addr];
        frame.extend_from_slice(data);
        let crc = Frame::calculate_crc(self.device_addr, data[0], &data[1..], self.crc_mode);
        frame.push((crc >> 8) as u8);
        frame.push(crc as u8);
        frame
    }

    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError> {
        if frame.len() < 5 {
            return Err(DeviceError::protocol_error("响应太短"));
        }
        Ok(DataFrame::new(format!("modbus_{}", self.device_addr), frame.to_vec()))
    }

    fn name(&self) -> &'static str {
        "ModbusRTU"
    }
}
```

### 3.5 协议处理器注册表

```rust
pub struct ProtocolHandlerRegistry;

impl ProtocolHandlerRegistry {
    /// 根据名称获取协议处理器实例
    pub fn get(name: &str, config: &Config) -> Option<Arc<dyn ProtocolHandler>> {
        match name {
            "modbus" => Some(Arc::new(ModbusHandler::new(config.device_addr, config.crc_mode))),
            "ttu" => Some(Arc::new(TtuHandler::new(config.device_addr))),
            "inverter" => Some(Arc::new(InverterHandler::new(config.device_addr))),
            "charger" => Some(Arc::new(ChargerHandler::new(config.device_addr))),
            _ => None,
        }
    }
}
```

### 3.6 RS485 半双工控制（DE/RE GPIO）

RS485 为半双工通信，需要通过 GPIO 控制发送使能（DE）和接收使能（RE）。

```rust
pub enum Rs485Dir {
    Recv,  // 接收模式
    Send,  // 发送模式
}

/// 设置 RS485 方向
fn set_dir(&self, dir: Rs485Dir) -> Result<(), Rs485Error> {
    if let Some(gpio_num) = match dir {
        Rs485Dir::Send => self.config.de_gpio,
        Rs485Dir::Recv => self.config.re_gpio,
    } {
        gpio_set_value(gpio_num, dir == Rs485Dir::Send)?;
    }
    Ok(())
}
```

### 3.7 读-写-读事务（原子操作）

```rust
impl SouthDevice for Rs485Device {
    fn transaction(&self, request: &[u8], recv_timeout_ms: u64) -> Result<DataFrame, DeviceError> {
        let _guard = self.tx_lock.lock();  // 全局锁保证原子性

        // 1. 切换到发送模式
        self.set_dir(Rs485Dir::Send)?;

        // 2. 发送请求
        self.send_frame(request)?;

        // 3. 切换到接收模式
        self.set_dir(Rs485Dir::Recv)?;

        // 4. 接收响应
        self.recv_frame(recv_timeout_ms)
            .map_err(|e| DeviceError::Other(e.to_string()))
    }
}
```

### 3.8 RS485 通信参数

| 设备类型 | 波特率 | 数据位 | 停止位 | 校验 | 典型轮询周期 |
|----------|--------|--------|--------|------|-------------|
| TTU | 9600 | 8 | 1 | 偶校验 | 1s |
| 光伏逆变器 | 9600 / 19200 | 8 | 1 | 无 | 5s |
| 充电桩 | 19200 | 8 | 1 | 偶校验 | 10s |
| 柔性负荷 | 9600 | 8 | 1 | 无 | 1s |
| 消防控制 | 9600 | 8 | 1 | 无 | 1s |

> **说明**：以太网通信方式（TCP/IP）延后至 Phase 2+ 实现，当前 Phase 2 仅实现 RS485。

### 3.9 RS485 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum Rs485Error {
    #[error("串口打开失败: {0}")]
    OpenFailed(String),

    #[error("串口配置失败: {0}")]
    ConfigFailed(String),

    #[error("数据发送失败: {0}")]
    SendFailed(String),

    #[error("数据接收失败: {0}")]
    RecvFailed(String),

    #[error("串口读写超时")]
    Timeout,
}
```

---

## 4. HPLC 驱动

### 4.1 架构

HPLC（高速电力线载波）模块采用"通用驱动抽象 + 芯片 SDK 后续集成"策略。Phase 2 实现 Mock 驱动用于开发和验证数据通路；芯片 SDK 绑定预留接口，延后至 Phase 3 集成。

```
hplc-plugin/
├── lib.rs              # 插件入口
├── driver.rs           # HplcDriver trait
├── device.rs           # HplcDevice（实现 SouthDevice）
├── mock.rs             # MockHplcDriver（开发/测试用）
├── errors.rs           # HplcError
└── config.rs           # 配置定义
```

### 4.2 HplcDriver Trait

```rust
use crate::errors::HplcError;

/// HPLC 驱动接口
///
/// 抽象不同芯片厂商的 PLC SDK
pub trait HplcDriver: Send + Sync {
    /// 初始化驱动
    fn init(&self, config: HplcConfig) -> Result<(), HplcError>;

    /// 发送数据
    fn send(&self, data: &[u8]) -> Result<(), HplcError>;

    /// 接收数据
    fn recv(&self, timeout_ms: u64) -> Result<Vec<u8>, HplcError>;

    /// 检查连接状态
    fn is_connected(&self) -> bool;

    /// 获取驱动名称
    fn driver_name(&self) -> &'static str;
}
```

### 4.3 HplcConfig

```rust
#[derive(Debug, Clone)]
pub struct HplcConfig {
    /// 串口路径（跨平台：Linux=/dev/ttyUSB0, Windows=COM3）
    #[serde(alias = "serial_port", alias = "com_port")]
    pub port: String,
    /// 波特率
    pub baud_rate: u32,
    /// 芯片型号（FFI 预留）
    pub chip_type: Option<String>,
    /// 通道号
    pub channel: Option<u8>,
}
```

### 4.4 HplcDevice

```rust
/// HPLC 设备
pub struct HplcDevice {
    device_id: String,
    device_type: String,
    config: HplcConfig,
    driver: Arc<dyn HplcDriver>,
    status: Mutex<DeviceStatus>,
}
```

`HplcDevice` 实现 `SouthDevice` trait，通过 `HplcDriver` trait 与具体硬件解耦。

### 4.5 MockHplcDriver（开发/测试用）

```rust
/// Mock HPLC 驱动（用于开发测试）
///
/// 支持模拟数据注入，用于验证数据流通路
pub struct MockHplcDriver {
    connected: AtomicBool,
    mock_queue: Mutex<Vec<Vec<u8>>>,   // 模拟数据队列
    mock_delay_ms: u64,                // 模拟延迟
}

impl MockHplcDriver {
    pub fn new() -> Self { ... }

    /// 注入模拟数据
    pub fn inject_data(&self, data: Vec<u8>) {
        self.mock_queue.lock().unwrap().push(data);
    }
}

impl HplcDriver for MockHplcDriver {
    fn init(&self, _config: HplcConfig) -> Result<(), HplcError> {
        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn send(&self, data: &[u8]) -> Result<(), HplcError> {
        tracing::debug!("MockHplcDriver 发送 {} 字节", data.len());
        Ok(())
    }

    fn recv(&self, _timeout_ms: u64) -> Result<Vec<u8>, HplcError> {
        let mut queue = self.mock_queue.lock().unwrap();
        if let Some(data) = queue.pop() {
            return Ok(data);
        }
        Ok(Vec::new())  // 无模拟数据返回空
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn driver_name(&self) -> &'static str {
        "MockHplcDriver"
    }
}
```

### 4.6 芯片 SDK FFI 绑定（预留，Phase 3）

> **说明**：芯片 SDK FFI 绑定延后至 Phase 3 实现，Phase 2 使用 MockHplcDriver 进行开发验证。

```rust
/// 芯片 SDK FFI 绑定（预留接口）
pub struct SdkHplcDriver {
    handle: *mut std::os::raw::c_void,  // FFI 句柄
}

impl HplcDriver for SdkHplcDriver {
    fn init(&self, config: HplcConfig) -> Result<(), HplcError> {
        // 调用 libhplc.so 中的 hplc_init()
        let ret = unsafe { hplc_init(config.port.as_ptr(), config.baud_rate) };
        if ret != 0 {
            return Err(HplcError::InitFailed(format!("hplc_init failed: {}", ret)));
        }
        Ok(())
    }

    fn send(&self, data: &[u8]) -> Result<(), HplcError> {
        let ret = unsafe { hplc_send(self.handle, data.as_ptr(), data.len() as u32) };
        if ret != 0 {
            return Err(HplcError::SendFailed(format!("hplc_send failed: {}", ret)));
        }
        Ok(())
    }

    fn recv(&self, timeout_ms: u64) -> Result<Vec<u8>, HplcError> {
        let mut buf = vec![0u8; 1024];
        let len = unsafe {
            hplc_recv(self.handle, buf.as_mut_ptr(), buf.len() as u32, timeout_ms as i32)
        };
        if len < 0 {
            return Err(HplcError::RecvFailed(format!("hplc_recv failed: {}", len)));
        }
        Ok(buf[..len as usize].to_vec())
    }

    fn is_connected(&self) -> bool {
        !self.handle.is_null()
    }

    fn driver_name(&self) -> &'static str {
        "SdkHplcDriver"
    }
}
```

### 4.7 HPLC 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum HplcError {
    #[error("驱动初始化失败: {0}")]
    InitFailed(String),

    #[error("发送失败: {0}")]
    SendFailed(String),

    #[error("接收失败: {0}")]
    RecvFailed(String),

    #[error("连接断开: {0}")]
    Disconnected(String),

    #[error("SDK 错误: {0}")]
    SdkError(String),
}
```

### 4.8 HPLC 技术参数（预留）

| 参数 | 规格 |
|------|------|
| 调制方式 | OFDM（BPSK / QPSK / 16QAM / 64QAM 自适应） |
| 通信频段 | 0.7 MHz - 3 MHz |
| 物理层速率 | 2 Mbps - 10 Mbps（自适应） |
| 最大帧长 | 1500 字节 |
| 典型应用 | 台区全覆盖场景，替代 RS485 布线困难区域 |

---

## 5. 动态插件系统

### 5.1 架构

动态插件系统通过 FFI 绑定实现运行时加载/卸载 `.so` / `.dll` / `.dylib` 动态库。所有南向通信插件（rs485-plugin、hplc-plugin）需遵循此规范。

```
PluginLoader (plugin-loader crate)
    ↓
libloading (动态加载 .so/.dll)
    ↓
FFI 导出函数: create_plugin() + plugin_meta()
    ↓
Plugin trait 实例
```

### 5.2 Plugin Trait

```rust
/// 插件元信息
#[derive(Debug, Clone)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}

/// 插件接口
pub trait Plugin: Send + Sync {
    fn meta(&self) -> PluginMeta;
    fn init(&self, config: serde_json::Value) -> Result<(), PluginError>;
    fn start(&self) -> Result<(), PluginError>;
    fn stop(&self) -> Result<(), PluginError>;
    fn shutdown(self: Box<Self>) -> Result<(), PluginError>;
}
```

### 5.3 必需 FFI 导出符号

每个动态插件必须导出以下两个 `extern "C"` 函数：

| 符号 | 类型 | 说明 |
|------|------|------|
| `create_plugin` | `unsafe extern "C" fn() -> *mut dyn Plugin` | 插件工厂函数 |
| `plugin_meta` | `unsafe extern "C" fn() -> PluginMeta` | 获取插件元信息 |

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

### 5.4 插件生命周期

```
Load → Init → Start → Stop → Unload
```

| 阶段 | 操作 | 状态 |
|------|------|------|
| **Load** | `dlopen` / `libloading` 加载 `.so` / `.dll`，调用 `create_plugin()` 获取实例，调用 `plugin_meta()` 获取元信息，注册到 PluginRegistry | Loaded |
| **Init** | 调用 `plugin.init(config)`，传入 JSON 配置 | Initialized |
| **Start** | 调用 `plugin.start()`，启动插件业务逻辑 | Running |
| **Stop** | 调用 `plugin.stop()`，停止插件业务逻辑 | Stopped |
| **Unload** | 调用 `plugin.shutdown()`，从注册表移除，卸载动态库 | Unloaded |

### 5.5 PluginLoader Trait

```rust
pub trait PluginLoader: Send + Sync {
    fn load(&self, plugin_path: &str, config: serde_json::Value) -> Result<(), PluginError>;
    fn unload(&self, plugin_name: &str) -> Result<(), PluginError>;
    fn list(&self) -> Vec<PluginMeta>;
    fn get(&self, plugin_name: &str) -> Option<Arc<dyn Plugin>>;
    fn init(&self, plugin_name: &str, config: serde_json::Value) -> Result<(), PluginError>;
    fn start(&self, plugin_name: &str) -> Result<(), PluginError>;
    fn stop(&self, plugin_name: &str) -> Result<(), PluginError>;
}
```

### 5.6 插件错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("插件加载失败: {0}")]
    LoadFailed(String),

    #[error("插件初始化失败: {0}")]
    InitFailed(String),

    #[error("插件启动失败: {0}")]
    StartFailed(String),

    #[error("插件停止失败: {0}")]
    StopFailed(String),

    #[error("插件不存在: {0}")]
    NotFound(String),

    #[error("元信息错误: {0}")]
    MetaError(String),

    #[error("其他错误: {0}")]
    Other(String),
}
```

### 5.7 插件状态枚举

| 状态 | 说明 |
|------|------|
| `Loaded` | 已加载 |
| `Initialized` | 已初始化 |
| `Running` | 运行中 |
| `Stopped` | 已停止 |
| `Unloaded` | 已卸载 |

### 5.8 编译要求

```toml
[lib]
crate-type = ["cdylib"]  # 必须编译为动态库
```

| 平台 | 输出 |
|------|------|
| Linux | `target/release/libmy_plugin.so` |
| Windows | `target/release/my_plugin.dll` |
| macOS | `target/release/libmy_plugin.dylib` |

### 5.9 插件依赖关系

```
device-trait (无依赖)
    ↓
plugin-loader → device-trait
    ↓
rs485-plugin → device-trait (编译为 cdylib)
hplc-plugin → device-trait (编译为 cdylib)
```

---

## 6. 配置格式

### 6.1 RS485 设备配置

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
      "parity": "none",
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

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `device_id` | string | 是 | 设备唯一标识 |
| `device_type` | string | 是 | 设备类型（ttu / inverter / charger / flexible_load / fire_alarm） |
| `port` | string | 是 | 串口路径（Linux=/dev/ttyUSB0, Windows=COM3） |
| `baud_rate` | int | 是 | 波特率（9600 / 19200 / 115200 等） |
| `data_bits` | int | 否 | 数据位，默认 8 |
| `stop_bits` | int | 否 | 停止位，默认 1 |
| `parity` | string | 否 | 校验位（none / even / odd） |
| `timeout_ms` | int | 否 | 通信超时（毫秒），默认 1000 |
| `device_addr` | int | 否 | 设备 Modbus 地址 |
| `handler` | string | 是 | 协议处理器名称（modbus / ttu / inverter / charger） |
| `de_gpio` | int | 否 | DE (Driver Enable) GPIO 引脚编号 |
| `re_gpio` | int | 否 | RE (Receiver Enable) GPIO 引脚编号 |

### 6.2 HPLC 设备配置

```json
{
  "hplc_devices": [
    {
      "device_id": "hplc_001",
      "device_type": "hplc",
      "driver": "mock",
      "config": {
        "serial_port": "/dev/ttyUSB2",
        "baud_rate": 115200
      }
    }
  ]
}
```

### 6.3 插件通用配置

```json
{
  "device_path": "/dev/ttyUSB0",
  "timeout_ms": 5000,
  "baud_rate": 9600
}
```

插件配置通过 `serde_json::Value` 传入 `plugin.init(config)` 方法。

---

## 7. 非功能性需求

### 7.1 性能需求

| 指标 | 要求 |
|------|------|
| 南向设备轮询周期 | TTU/柔性负荷/消防：>= 1Hz；逆变器：>= 0.2Hz（5s）；充电桩：>= 0.1Hz（10s） |
| RS485 吞吐量 | 115200 波特率下无丢包 |
| 单设备读取延迟 | <= 500ms |
| 设备注册/注销操作 | <= 50ms |
| 插件加载时间 | 单个插件 <= 500ms |
| 并发设备数 | 支持 >= 100 个设备同时在线 |
| 消息总线吞吐量 | >= 10000 msg/s |

### 7.2 可靠性需求

| 指标 | 要求 |
|------|------|
| 通信丢包率 | < 0.1% |
| 误码率 | < 10^-6 |
| 超时重试机制 | 支持可配置重试次数和超时时间 |
| 故障隔离 | 单设备通信故障不影响其他设备 |

### 7.3 安全需求

| 需求 | 说明 |
|------|------|
| RS485 物理安全 | 物理层无加密，依赖物理访问控制 |
| 配置文件安全 | 设备地址和串口路径需要访问控制 |
| 插件验证 | 插件签名验证（可选） |
| 日志安全 | 日志中不得记录明文密码、密钥 |

### 7.4 可维护性需求

| 需求 | 说明 |
|------|------|
| 热插拔 | 支持运行时添加/移除设备配置（重载配置生效） |
| 动态加载 | 插件支持运行时加载/卸载 |
| 日志 | 基于 `tracing` 记录通信日志 |
| rustdoc | 公共 API 有 rustdoc 注释 |

---

## 8. 验收标准汇总

### 8.1 功能验收

| ID | 功能点 | 验收条件 |
|----|--------|----------|
| F01 | SouthDevice trait | 所有南向设备统一实现 SouthDevice trait |
| F02 | 设备注册表 | 支持注册、注销、按类型查询、列出所有设备 |
| F03 | RS485 Modbus 通信 | 能与通用 Modbus RTU 设备通信，编码/解码正确 |
| F04 | RS485 TTU 通信 | 能与 TTU 设备通信，遵循电力行业规约 |
| F05 | RS485 逆变器通信 | 能与光伏逆变器通信，支持厂商私有协议 |
| F06 | RS485 充电桩通信 | 能与充电桩通信，支持 GB/T 27930 |
| F07 | RS485 半双工控制 | DE/RE GPIO 方向切换正确，读-写-读事务原子性 |
| F08 | RS485 串口参数 | 支持配置波特率/数据位/停止位/校验位 |
| F09 | MockHplcDriver | 支持模拟数据注入，支持开发调试 |
| F10 | HplcDriver trait | 定义 init/send/recv/is_connected/driver_name |
| F11 | 插件加载 | `plugin-loader` 支持动态加载 `.so` / `.dll` |
| F12 | 插件卸载 | 支持动态卸载插件，释放资源 |
| F13 | 插件生命周期 | 严格遵循 Load -> Init -> Start -> Stop -> Unload |
| F14 | FFI 导出 | 插件必须导出 `create_plugin` 和 `plugin_meta` |
| F15 | 协议处理器注册 | 支持通过名称配置协议处理器，运行时注入 |

### 8.2 性能验收

| ID | 性能点 | 验收条件 |
|----|--------|----------|
| P01 | 插件加载时间 | 单个插件加载 < 500ms |
| P02 | RS485 吞吐量 | 115200 波特率下无丢包 |
| P03 | 设备注册/注销 | 操作耗时 < 50ms |
| P04 | 并发设备数 | 支持 >= 100 个设备同时在线 |
| P05 | 消息总线吞吐量 | >= 10000 msg/s |

### 8.3 质量验收

| ID | 质量点 | 验收条件 |
|----|--------|----------|
| Q01 | 编译通过 | `cargo build --release` 无警告 |
| Q02 | Clippy 检查 | `cargo clippy` 无 Error |
| Q03 | 单元测试 | `cargo test` 覆盖率 >= 80% |
| Q04 | 错误处理 | 所有错误实现 `std::error::Error` |
| Q05 | 无新增 unsafe | 除 FFI 导出外无新增不安全代码 |
| Q06 | rustdoc | 公共 API 有 rustdoc 注释 |
| Q07 | 格式化 | `cargo fmt` 通过 |

### 8.4 兼容性验收

| ID | 兼容性点 | 验收条件 |
|----|----------|----------|
| C01 | 操作系统 | openEuler 22.03+ |
| C02 | 硬件平台 | RK3588 |
| C03 | 编译器 | Rust >= 1.75 |

---

## 附录 A：依赖关系

### A.1 内部依赖

```
device-trait (无依赖)
    ↓
plugin-loader → device-trait
    ↓
rs485-plugin → device-trait
hplc-plugin → device-trait
```

### A.2 外部依赖

| Crate | 版本 | 用途 |
|-------|------|------|
| tokio | 1.x | 异步运行时 |
| serde | 1.x | 序列化 |
| serde_json | 1.x | JSON 解析 |
| thiserror | 1.x | 错误类型 |
| libloading | 0.8 | 动态库加载 |
| serial | 0.4 | 串口通信 |
| tracing | 1.x | 日志 |

## 附录 B：术语表

| 术语 | 说明 |
|------|------|
| MUPC | 微电网特种调控装置 |
| TTU | 台区智能融合终端 |
| HPLC | 高速电力线载波通信 |
| RS485 | 串行通信总线标准 |
| DE/RE | RS485 半双工使能引脚（Driver Enable / Receiver Enable） |
| GPIO | 通用输入输出引脚 |
| FFI | 外部函数接口（Foreign Function Interface） |
| cdylib | C 动态库格式（Rust crate-type） |
| Modbus RTU | 串行通信协议，RS485 物理层上的常用协议 |
| GB/T 27930 | 电动汽车非车载传导式充电机与电池管理系统通信协议 |

## 附录 C：来源文档

| 文档 | 说明 |
|------|------|
| `docs/superpowers/specs/2026-05-29-MUPC-动态插件-FFI绑定规范.md` | 动态插件 FFI 绑定规范 |

---

**文档状态**: 已评审通过 (REVIEWED: PASS)
**维护者**: MUPC Team

---

## v1.1 修订记录

本次修订基于 [MUPC 全量需求与设计覆盖度审查总报告](../reports/2026-05-29-MUPC-全量需求与设计覆盖度审查-总报告.md) 中识别出的模块 02 部分覆盖缺口（P-06、P-07、P-08），全部为低风险项。

| 编号 | 缺口 | 修改内容 | 修改位置 |
|------|------|----------|----------|
| P-06 | 以太网连接方式 | 光伏逆变器、充电桩的通信方式从"RS485"改为"RS485 / 以太网"；在 3.8 通信参数表下方增加以太网延后至 Phase 2+ 的说明 | 1.4 表格、3.8 节末尾 |
| P-07 | 消防控制系统详细协议 | 消防控制系统的处理器列标注"专用 FireAlarmHandler 延后至 Phase 2+"；在 3.4 协议处理器实现表中新增 FireAlarmHandler 行 | 1.4 表格、3.4 表格 |
| P-08 | HPLC 芯片 SDK 实际集成延后 | 在 4.6 章节标题后增加明确状态说明：Phase 2 使用 MockHplcDriver，芯片 SDK FFI 延后至 Phase 3 | 4.6 节 |
