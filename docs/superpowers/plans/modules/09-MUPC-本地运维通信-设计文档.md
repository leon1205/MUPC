# MUPC 本地运维通信 - 模块设计文档

**对应模块：** `wireless` crate（新建）

**前置依赖：** OTA模块、Web UI、日志系统、security crate

---

## 目录

1. [模块架构](#1-模块架构)
2. [星闪（NearLink）驱动设计](#2-星闪-nearlink-驱动设计)
3. [Wi-Fi 通信设计](#3-wi-fi-通信设计)
4. [BLE 通信设计](#4-ble-通信设计)
5. [端到端加密设计](#5-端到端加密设计)
6. [通道管理设计](#6-通道管理设计)
7. [与OTA/日志/配置系统集成](#7-与ota日志配置系统集成)
8. [接口定义](#8-接口定义)
9. [文件结构](#9-文件结构)
10. [技术决策记录](#10-技术决策记录)

---

## 1. 模块架构

### 1.1 设计目标

MUPC 微电网特种调控装置在 Web UI 和北向远程通信两通道之外，新增 **现场本地无线运维通道**。本设计覆盖三条无线通道：

| 通道 | 协议 | 核心能力 | 速率要求 |
|------|------|----------|----------|
| 星闪 (NearLink) | SLE Profile | 配置读写、日志导出、固件升级 | >= 10 Mbps |
| Wi-Fi | 802.11 b/g/n/ac (2.4+5 GHz) | Web UI 访问、大量日志导出、大文件固件升级 | >= 20 Mbps |
| 蓝牙 | BLE 4.2+ / 5.0 | 状态读取、轻量配置、升级命令触发 | >= 50 Kbps |

### 1.2 整体架构

```
┌──────────────────────────────────────────────────────────────────┐
│                        wireless crate                              │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐  │
│  │                      WirelessManager                          │  │
│  │  ┌──────────────────────────────────────────────────────┐   │  │
│  │  │  状态管理 (state.rs)   配置管理 (config.rs)  审计日志  │   │  │
│  │  │                        (audit.rs)                      │   │  │
│  │  └──────────────────────────────────────────────────────┘   │  │
│  │                           │                                  │  │
│  │          ┌────────────────┼────────────────┐                │  │
│  │          ▼                ▼                ▼                │  │
│  │  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐        │  │
│  │  │ NearLinkCtrl  │ │  WiFiCtrl    │ │  BleCtrl     │        │  │
│  │  │ (FFI/SDK)    │ │ (hostapd/    │ │ (bluer/      │        │  │
│  │  │              │ │  wpa_suppl.) │ │  BlueZ D-Bus)│        │  │
│  │  └──────┬───────┘ └──────┬───────┘ └──────┬───────┘        │  │
│  │         │                │                │                  │  │
│  └─────────┼────────────────┼────────────────┼──────────────────┘  │
│            │                │                │                     │
│            ▼                ▼                ▼                     │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐              │
│  │  Hi2821 USB  │ │  Wi-Fi M.2  │ │  BLE (shares │              │
│  │   NearLink   │ │   Module    │ │  Wi-Fi RF)   │              │
│  │   /dev/hi*   │ │   wlan0     │ │   hci0       │              │
│  └──────────────┘ └──────────────┘ └──────────────┘              │
└──────────────────────────────────────────────────────────────────┘
         │                      │                      │
         └──────────────────────┼──────────────────────┘
                                │
                                ▼
┌──────────────────────────────────────────────────────────────────┐
│                         外部集成层                                 │
│                                                                   │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐           │
│  │   web-api    │  │  ota-update  │  │   security   │           │
│  │  (Axum 路由) │  │  (固件升级)  │  │  (认证/加密)  │           │
│  └──────────────┘  └──────────────┘  └──────────────┘           │
│                                                                   │
│  ┌──────────────┐  ┌──────────────────────────────────┐         │
│  │    core      │  │  /var/log/mupc/audit.log         │         │
│  │  (配置系统)   │  │  (审计日志文件)                    │         │
│  └──────────────┘  └──────────────────────────────────┘         │
└──────────────────────────────────────────────────────────────────┘
```

### 1.3 合并 Crate 策略

**决策：新增一个 `wireless` crate，内部按技术类型分 sub-module。**

```
crates/wireless/
├── Cargo.toml
└── src/
    ├── lib.rs                   # 模块导出 + WirelessManager 统一入口
    ├── config.rs                # 无线配置管理
    ├── error.rs                 # 错误类型
    ├── types.rs                 # 公共数据类型
    ├── audit.rs                 # 审计日志
    ├── auth.rs                  # 接入认证
    ├── manager.rs               # WirelessManager 核心
    ├── web_api.rs               # REST API 路由
    ├── scheduler.rs             # 定时开关、省电策略
    ├── near_link/
    │   ├── mod.rs
    │   ├── ffi.rs
    │   └── controller.rs
    ├── wifi/
    │   ├── mod.rs
    │   ├── hostapd.rs
    │   ├── supplicant.rs
    │   └── scanner.rs
    └── ble/
        ├── mod.rs
        └── gatt_server.rs
```

### 1.4 WirelessManager 核心接口

```rust
pub struct WirelessManager {
    state: Arc<RwLock<WirelessState>>,
    config: Arc<RwLock<WirelessConfig>>,
    near_link: NearLinkController,
    wifi: WiFiController,
    ble: BleController,
    audit_logger: AuditLogger,
    auth_manager: AuthManager,
}

impl WirelessManager {
    pub async fn initialize(&self, config: WirelessConfig) -> Result<()>;
    pub async fn start_channel(&self, channel: ChannelType) -> Result<()>;
    pub async fn stop_channel(&self, channel: ChannelType) -> Result<()>;
    pub async fn flight_mode(&self, enable: bool) -> Result<()>;
    pub async fn get_status(&self) -> Result<Vec<ChannelStatus>>;
    pub async fn get_stats(&self) -> Result<WirelessStats>;
}
```

### 1.5 关键约束

| 项目 | 要求 |
|------|------|
| 平台 | openEuler 22.03+, 内核 >= 5.10, RK3588 |
| 语言 | Rust 1.75+ |
| 异步运行时 | Tokio |
| 无线模组接口 | USB / SDIO / M.2 Key E（需硬件确认） |
| 客户端兼容 | Android 10+ / iOS 15+ 手机或平板，支持 BLE GATT 和 Wi-Fi |

---

## 2. 星闪 (NearLink) 驱动设计

### 2.1 模组选型

| 方案 | 制造商 | 接口 | Linux 驱动 | 优选级 |
|------|--------|------|-----------|--------|
| **HiSilicon Hi2821** | 海思 | USB 2.0 | 厂商提供 SDK（openEuler 已验证） | **推荐** |
| HiSilicon Hi3861V100 | 海思 | UART/SPI | 厂商提供 AT 固件，串口命令交互 | 备选 |
| 爱旗 IoT-NearLink | 爱旗科技 | USB | 厂商提供 Linux 驱动 | 候选 |

**推荐选中：海思 Hi2821 USB 模组**。理由：USB 接口即插即用，对 RK3588 兼容性最好；厂商已提供 openEuler 驱动；支持 SLE 1.0 标准；覆盖距离 >= 150 米（视距）。

### 2.2 软件栈设计

```
应用层 (Rust)
    ↓  FFI / IPC
NearLink SDK (C, 厂商提供)
    ↓
USB 字符设备 (/dev/hi2821_nearlink)
    ↓
内核驱动 (vendor module)
    ↓
Hi2821 USB 模组
```

### 2.3 FFI 绑定策略

| 方案 | 说明 | 复杂度 | 性能 | 维护性 |
|------|------|--------|------|--------|
| **A: FFI 绑定** | 使用 `bindgen` 生成厂商 SDK 的 Rust FFI 绑定 | 中 | 优 | SDK 升级需重新绑定 |
| **B: C 子进程通信** | 封装为独立 C 守护进程，通过 Unix Socket + JSON 通信 | 低 | 良 | 隔离性好，但增加延迟 |
| **C: 完全 Rust 实现** | 基于 USB raw device 从零实现 SLE 协议 | 高 | 优 | 完全不依赖厂商，但开发量极大 |

**决策：采用方案 A（FFI 绑定）+ 方案 B（C 守护进程降级）的双轨策略。** 首选基于厂商 SDK 做 FFI 绑定（方案 A）；若 SDK 质量或 license 有问题，降级为方案 B（C 守护进程 + Unix Socket）。无论是哪种方式，`near_link::controller` 的对外接口不变。

### 2.4 NearLink 应用层协议

```
┌──────────────────────────────────────────────────────────┐
│ NearLink 应用帧结构                                        │
├──────────┬──────────┬──────────┬──────────────────────────┤
│ FrameType│ SeqNo   │ Payload  │ Payload (JSON, UTF-8)    │
│ (1 byte) │ (2 byte)│ Length   │                          │
│          │         │ (2 byte) │                          │
├──────────┼──────────┼──────────┼──────────────────────────┤
│ 0x01     │ 0x0001   │ 0x003C   │ {"cmd":"get_config",...}│
└──────────┴──────────┴──────────┴──────────────────────────┘
```

**帧类型定义：**

| Type | 值 | 方向 | 说明 |
|------|----|------|------|
| `REQ` | 0x01 | Client → Device | 请求帧 |
| `RSP` | 0x02 | Device → Client | 响应帧 |
| `NOTIFY` | 0x03 | Device → Client | 通知帧（状态变化、进度）|
| `FILE_REQ` | 0x10 | Client → Device | 文件传输请求（日志导出）|
| `FILE_DATA` | 0x11 | Bidirectional | 文件数据分块 |
| `FILE_ACK` | 0x12 | Bidirectional | 文件传输确认 |
| `FW_REQ` | 0x20 | Client → Device | 固件上传请求 |
| `FW_DATA` | 0x21 | Client → Device | 固件数据分块 |
| `FW_ACK` | 0x22 | Device → Client | 固件数据确认 |

### 2.5 固件传输与断点续传

```rust
pub struct FirmwareTransfer {
    state: TransferState,
    bytes_received: u64,
    total_bytes: u64,
    checksum: Sha256,
    temp_file: PathBuf,
}

impl FirmwareTransfer {
    pub async fn write_chunk(&mut self, chunk: &[u8]) -> Result<ChunkAck>;
    pub fn get_resume_point(&self) -> ResumePoint;
}
```

### 2.6 Unsafe 代码范围

| unsafe 代码位置 | 行数 | 说明 |
|---|---|---|
| `near_link/ffi.rs` | 10-30 | FFI 调用厂商 SDK (bindgen) |
| **总计** | **~20** | 全部集中在 near_link::ffi 模块 |

---

## 3. Wi-Fi 通信设计

### 3.1 技术选型

| 方案 | 实现方式 | 优点 | 缺点 | 推荐度 |
|------|----------|------|------|--------|
| **A: `hostapd` + `wpa_supplicant` 子进程** | 通过 `std::process::Command` 调用系统工具 | 稳定成熟，与发行版集成好 | 需要系统预装工具，进程管理复杂 | **推荐** |
| **B: `nl80211` 直接控制（`netlink`）** | 通过 `neli` 或 `genetlink` crate 直接操作内核 netlink | 纯 Rust，无外部依赖 | 实现复杂，需深入理解 nl80211 协议 | 备选 |

**决策：采用方案 A（hostapd + wpa_supplicant 子进程）。** `hostapd` 和 `wpa_supplicant` 是 openEuler 标准包，可通过 `yum install` 安装；API 成熟，通过配置文件 + 信号即可控制；通过控制接口 socket（`/var/run/hostapd/*`）实现动态管理。

### 3.2 AP 模式设计

```rust
pub struct HostapdController {
    config_path: PathBuf,         // /etc/hostapd/hostapd.conf
    control_socket: PathBuf,      // /var/run/hostapd/<iface>
    process: Option<Child>,
}

impl HostapdController {
    async fn start_ap(&mut self, config: ApConfig) -> Result<()>;
    async fn stop_ap(&mut self) -> Result<()>;
    async fn get_clients(&self) -> Result<Vec<ClientInfo>>;
    async fn get_status(&self) -> Result<ApStatus>;
}
```

**默认配置：**
- SSID: `MUPC-AP-{序列号后6位}`
- 默认密码印刷在装置外壳标签上
- 频段: 5 GHz / 2.4 GHz 可配置
- DHCP 地址池: `192.168.4.2/24` ~ `192.168.4.254/24`
- 装置 AP IP: `192.168.4.1`
- 最大客户端数: 8
- 空闲超时: 600 秒无客户端连接后自动关闭

### 3.2.1 AP 模式零影响约束

AP 模式的运行不得影响其他模块的正常工作，具体要求如下：

| 约束项 | 要求 |
|--------|------|
| **网络接口隔离** | AP 模式仅在无线网卡（wlan0）上启动 SoftAP，不修改其他网络接口（eth0、eth1）配置 |
| **北向通信不受影响** | AP 模式不占用有线网卡（eth0），北向通信（IEC 104、IEC 61850、MQTT）正常运行 |
| **策略引擎不受影响** | AP 模式运行于独立线程，不阻塞 Tokio runtime 主线程 |
| **资源占用上限** | CPU 占用 < 5%，内存占用 < 50MB |
| **失败容错** | AP 启动失败时仅标记通道为 `unavailable`，不影响其他模块运行 |

### 3.3 Station 模式设计

```rust
pub struct WpaSupplicantController {
    config_path: PathBuf,         // /etc/wpa_supplicant/wpa_supplicant.conf
    control_socket: PathBuf,      // /var/run/wpa_supplicant/<iface>
    process: Option<Child>,
}

impl WpaSupplicantController {
    async fn connect(&mut self, config: StationConfig) -> Result<()>;
    async fn disconnect(&mut self) -> Result<()>;
    async fn scan_networks(&self) -> Result<Vec<NetworkInfo>>;
    async fn get_status(&self) -> Result<StationStatus>;
}
```

**支持特性：**
- WPA2-PSK / WPA3-SAE 认证
- DHCP 自动获取 IP / 手动配置静态 IP
- 保存最多 5 个 Wi-Fi 网络配置，按优先级自动连接
- 周期性扫描可用 Wi-Fi 网络（扫描间隔: 60 秒）

### 3.4 并发双 Wi-Fi 模式处理

Wi-Fi AP 和 Station 同时运行的能力取决于无线模组。分三种情况：

| 情况 | 描述 | 处理策略 |
|------|------|----------|
| **支持并发双模式** | 单个物理 Wi-Fi 芯片同时支持 AP + Station | 同一 `wlan0` 设备启用 `CONFIG_AP` + `CONFIG_STATION` |
| **仅支持单模式** | 同一芯片无法同时 AP + Station | 软件切换：用户配置时提示二选一 |
| **双物理设备** | 两个 Wi-Fi 芯片（如内置 + USB Wi-Fi dongle） | 分别绑定，完全并发 |

**设计决策：**
- 通过运行时检测确定当前硬件能力（尝试 `iw list` 中的 `combinations` 字段）
- 配置文件允许用户指定 `wifi.concurrency_mode = "auto" | "ap_only" | "station_only" | "concurrent"`
- API 层抽象差异：无论底层是何种并发模式，`WiFiController` 对外暴露统一接口

---

## 4. BLE 通信设计

### 4.1 技术选型

| 库 | 底层 | 异步支持 | 成熟度 | GATT 服务器 | 推荐度 |
|----|------|----------|--------|------------|--------|
| **`bluer`** | BlueZ D-Bus | 原生 async/await | 成熟 | **支持** | **推荐** |
| `btleplug` | BlueZ / CoreBluetooth | 通过 `futures` | 活跃 | 不支持（仅客户端） | 备选 |
| `zbus` 直接 D-Bus | BlueZ D-Bus | 原生 async/await | 成熟 | 支持 | 备选 |

**决策：采用 `bluer` crate。** 理由：直接基于 BlueZ D-Bus API，对 BlueZ GATT Server 封装良好；原生 `async/await` 支持，与 Tokio 运行时自然集成；openEuler 预装 BlueZ 5.66+；提供 GATT Server 示例。

### 4.2 GATT 服务定义

**服务 UUID（采用标准 Bluetooth Base UUID）：**

| 服务 | UUID | 类型 |
|------|------|------|
| MUPC 无线运维服务 | `0000FACE-0000-1000-8000-00805F9B34FB` | Primary Service |

### 4.3 Characteristic 定义

| 特征 | UUID | 属性 | 说明 |
|------|------|------|------|
| 密钥协商 | `0000FAC0-0000-1000-8000-00805F9B34FB` | Write / Indicate | ECDH 公钥交换、会话建立、密钥轮换 |
| 设备状态 | `0000FAC1-0000-1000-8000-00805F9B34FB` | Read / Notify | 序列号、固件版本、运行状态等 |
| 配置读写 | `0000FAC2-0000-1000-8000-00805F9B34FB` | Read / Write | 关键配置项读写 |
| 日志传输 | `0000FAC3-0000-1000-8000-00805F9B34FB` | Notify | 日志数据分块传输 |
| 固件传输 | `0000FAC4-0000-1000-8000-00805F9B34FB` | Write | 固件升级控制命令 |
| 控制命令 | `0000FAC5-0000-1000-8000-00805F9B34FB` | Write / Indicate | 升级触发、回滚等命令 |

### 4.4 UUID 合规性说明

上述 UUID 使用标准 Bluetooth Base UUID 格式：
- **Base**: `0000XXXX-0000-1000-8000-00805F9B34FB`（Bluetooth SIG 定义的 128-bit UUID 基值）
- **16-bit 部分**: `0xFACE`（服务）、`0xFAC0`–`0xFAC5`（特征）
- 这些 UUID 在 Bluetooth SIG 的 16-bit UUID 空间中属于 **自定义/供应商特定范围**（`0xFC00`–`0xFFFF` 为供应商专用，`0xFAC0`–`0xFACF` 位于标准未分配的自定义区间），可直接使用而无需向 SIG 申请。实际部署时可根据需要替换为分配的 16-bit UUID。

### 4.5 设备状态特征数据格式

```json
{
  "serial": "MUPC202605280001",
  "fw_version": "1.2.3",
  "uptime_seconds": 86400,
  "cpu_temp_celsius": 52.3,
  "mem_usage_percent": 34,
  "module_status": {
    "iec104": "online",
    "intercore": "online",
    "rs485": "online"
  },
  "alarm_count": 0,
  "wireless_status": {
    "nearlink": "connected",
    "wifi_ap": "idle",
    "wifi_sta": "disabled",
    "ble": "connected"
  }
}
```

### 4.6 控制命令协议

每个控制命令为 JSON 格式，最大 512 字节：

```json
// 发送端 → 装置
{ "cmd": "ota_check", "token": "eyJhbGci...", "params": {} }

// 装置 → 发送端（通过 Indicate 回复）
{ "cmd": "ota_check", "status": "ok", "message": "New firmware version 1.3.0 available" }
```

**支持的命令列表：**

| cmd | 说明 | 参数 |
|-----|------|------|
| `ota_check` | 检查 OTA 更新 | `{}` |
| `ota_trigger` | 触发 OTA 升级 | `{"version": "1.3.0"}` |
| `ota_status` | 查询升级状态 | `{}` |
| `rollback` | 触发回滚 | `{}` |
| `reboot` | 重启装置 | `{}` |
| `set_log_config` | 设置日志导出参数 | `{"minutes": 60, "level": "ERROR", "max_entries": 500}`<br>`minutes`: 时间范围(分钟); `level`: 日志级别过滤; `max_entries`: u16, 默认 500, 范围 100~2000, 单次日志导出最大条目数 |

### 4.7 BLE 连接不稳定处理

监测短暂断开重连模式，当 30 秒内断开重连超过 3 次时，主动延长蓝牙扫描间隔（100 ms → 500 ms），降低功耗并提高稳定性。

---

## 5. 端到端加密设计

### 5.1 加密方案总览

PRD 要求配置数据传输使用端到端应用层加密。本设计基于 **ECDH (X25519) + AES-256-GCM** 实现所有无线通道的传输加密。

| 参数 | 值 | 说明 |
|------|----|------|
| KEM | X25519 (Curve25519 ECDH) | 椭圆曲线密钥交换，RFC 7748 |
| KDF | HKDF-SHA256 | RFC 5869 密钥派生函数 |
| 加密算法 | AES-256-GCM | 认证加密 (AEAD)，NIST SP 800-38D |
| AES Key 长度 | 32 字节 (256-bit) | |
| Nonce 长度 | 12 字节 (96-bit) | 随机 per-message |
| Auth Tag 长度 | 16 字节 (128-bit) | GCM 认证标签 |
| 公钥长度 | 32 字节 (256-bit) | X25519 原始格式 |

### 5.2 密钥协商流程（ECDH X25519）

```
MUPC (Server)                          Client (App/Tool)
    │                                        │
    │  ── 广播自身公钥 PubKey_MUPC ────────►  │   (Advertising/Beacon中携带)
    │                                        │
    │  ◄── 客户端公钥 PubKey_Client ─────────  │   (连接建立后交换)
    │                                        │
    │  计算共享密钥:                          │   计算共享密钥:
    │  SharedSecret = X25519(               │   SharedSecret = X25519(
    │    PrivKey_MUPC, PubKey_Client         │     PrivKey_Client, PubKey_MUPC
    │  )                                     │   )
    │                                        │
    │  派生会话密钥 (HKDF-SHA256):            │   派生会话密钥 (HKDF-SHA256):
    │  salt = channel_id || session_id       │   salt = channel_id || session_id
    │  info = "mupc-wireless-aes-gcm"       │   info = "mupc-wireless-aes-gcm"
    │                                        │
    │  AES_Key = HKDF-Expand(salt, info, 32) │   (同上)
    │  AES_Nonce_Base = HKDF-Expand(...)     │   (同上)
    │                                        │
    │  ── Encrypted(测试帧) ────────────────►  │   解密测试帧成功
    │  ◄── Encrypted(测试帧确认) ─────────────  │
    │                                        │
    │  安全通道建立完成                        │   安全通道建立完成
```

### 5.3 AES-256-GCM 加密帧格式

每条应用层消息在发送前进行加密封装：

```
┌──────────────────────────────────────────────────────────────────┐
│ Encrypted Application Frame                                       │
├─────────────┬─────────────┬──────────────┬──────────────────────┤
│ Frame Header│ Nonce       │ Ciphertext   │ GCM Auth Tag         │
│ (plaintext) │ (random)    │ (encrypted)  │ (integrity check)    │
├─────────────┼─────────────┼──────────────┼──────────────────────┤
│ 4 bytes     │ 12 bytes    │ variable     │ 16 bytes             │
├─────────────┼─────────────┼──────────────┼──────────────────────┤
│ Version(1B) │ Per-message │ AES-256-GCM  │ GCM authentication   │
│ FrameType   │ random      │ output       │ tag                  │
│ (1B)        │ nonce       │              │                      │
│ Reserved(2B)│             │              │                      │
└─────────────┴─────────────┴──────────────┴──────────────────────┘
```

**加密流程（发送端）：**
1. 生成随机 Nonce (12 bytes)，使用加密安全随机数生成器
2. AES-256-GCM 加密: `(ciphertext, auth_tag) = AES-256-GCM-Encrypt(key, nonce, plaintext, aad)`，其中 `aad = FrameHeader (4 bytes 明文)`
3. 组装帧: `FrameHeader || Nonce || Ciphertext || AuthTag`

**解密流程（接收端）：**
1. 提取 FrameHeader, Nonce, Ciphertext, AuthTag
2. `plaintext = AES-256-GCM-Decrypt(key, nonce, ciphertext, auth_tag, aad)`，其中 `aad = FrameHeader (4 bytes)`
3. 验证 auth_tag: 若验证失败，丢弃该帧，记录告警并递增认证失败计数

**安全约束：**
- Nonce 必须随机生成，禁止使用计数器模式
- 同一会话密钥下最多加密 2^32 条消息后必须轮换密钥
- AAD 包含 FrameHeader，防止帧头被篡改

### 5.4 Wi-Fi 通道加密叠加

Wi-Fi 链路层已通过 WPA2/WPA3-SAE 提供链路加密，在此基础上叠加应用层加密实现"双保险"：

1. Client 连接 Wi-Fi AP（WPA2-PSK / WPA3-SAE 链路层认证）
2. Client 访问 Web UI，登录获取会话 Token
3. 首次访问受保护 API 时触发密钥协商（通过 `POST /api/v1/wireless/key-exchange`）
4. 后续 API 请求使用自定义 Header `X-MUPC-Encrypted` 传输加密帧

若部署 TLS 1.3 证书，HTTPS 本身已提供传输层加密。应用层 AES-256-GCM 加密作为**补充层**，用于：
- 无证书场景下的安全保障（自签名证书场景）
- 与 BLE 通道统一加密协议栈，减少代码分支
- 防御 TLS 中间人攻击（企业内部网场景）

### 5.5 BLE 通道加密

BLE 链路层加密（LESC/Passkey Entry）仅保护配对阶段，不保护配对完成后的 GATT 数据交换。所有 GATT Characteristic 的读写操作必须在应用层加密。

密钥协商在 `0000FAC0` 特征中完成：
- **Write**: 客户端写入 `session_id(16B UUID bytes) || pubkey(32B)`
- **Indicate**: 服务端回复 `session_id(16B) || pubkey(32B) || encrypted_test_frame`

密钥协商完成前所有特征返回操作拒绝；协商完成后特征值自动进入加密模式。

### 5.6 密钥轮换策略

| 轮换类型 | 触发条件 | 操作 | 影响 |
|---------|---------|------|------|
| **会话级** | 每次新连接建立 | 重新执行 ECDH 密钥协商 | 毫秒级重新握手 |
| **周期轮换** | 每 24 小时 或 每 1GB 加密数据量 | 重新密钥协商，更新会话密钥 | 现有连接中断 < 100ms，自动重连 |
| **事件驱动** | 检测到安全事件（非预期帧、认证失败过多） | 立即重新密钥协商 | 临时断开后自动恢复 |
| **客户端主动** | 客户端发起密钥更新请求 | 通过密钥协商特征重新写入新公钥 | 无中断（双缓冲切换） |

**密钥清除协议：**
1. 连接断开/会话超时：立即清除内存中的会话密钥和派生密钥
2. 主动销毁：通过控制命令 `cmd: "key_clear"` 清除所有会话状态
3. 恢复出厂：配合 security crate 的清空流程，移除所有长期密钥材料
4. 内存保护：会话密钥通过 `mlock()` 锁定在物理内存，禁止 swap 到磁盘

---

## 6. 通道管理设计

### 6.1 通道优先级规则

| 优先级 | 通道 | 说明 |
|--------|------|------|
| 1（最高） | NearLink | 最优先，安全且高速 |
| 2 | Wi-Fi Station 模式 | 已有网络连接 |
| 3 | Wi-Fi AP 模式 | 临时热点 |
| 4（最低） | Bluetooth | 低带宽，仅轻量操作 |

### 6.2 通道生命周期管理

```
                ┌─────────────┐
                │   DISABLED  │
                └──────┬──────┘
                       │ start_channel()
                       ▼
                ┌─────────────┐
                │  INIT       │ ← 加载驱动/初始化硬件
                └──────┬──────┘
                       │ init_ok
                       ▼
                ┌─────────────┐
         ┌──────┤   IDLE      ├──────┐
         │      └──────┬──────┘      │
         │             │             │
         │ client_     │ client_     │ timeout /
         │ connected   │ connected   │ user_stop
         ▼             ▼             ▼
   ┌──────────┐  ┌──────────┐  ┌──────────┐
   │CONNECTED │  │CONNECTED │  │ SLEEP    │
   │ (1 client)│  │ (N client)│  │(低功耗)  │
   └────┬─────┘  └────┬─────┘  └────┬─────┘
        │             │             │
        │ client_     │ all_client_ │ wakeup /
        │ disconnect  │ disconnect  │ start
        ▼             ▼             │
   ┌──────────┐  ┌──────────┐        │
   │   IDLE   │  │   IDLE   │        │
   └──────────┘  └──────────┘        │
        │             │              │
        └─────────────┼──────────────┘
                      │ stop_channel()
                      ▼
                ┌─────────────┐
                │  SHUTDOWN   │
                └─────────────┘
```

### 6.3 状态监控

通过 Web UI 无线状态页面展示：
- 各通道开关状态（开启/关闭）
- 各通道连接状态（已连接/等待连接/未启用）
- 已连接客户端数量
- 当前数据传输速率（Mbps）
- 累计传输数据量（MB）
- 信号强度（dBm，仅对已连接的客户端）

状态刷新周期：5 秒。

### 6.4 通道控制

- 通过 Web UI 可独立开启/关闭 NearLink、Wi-Fi AP、Wi-Fi Station、蓝牙
- 关闭操作立即生效（无线模块停止广播/扫描）
- 开启操作后 10 秒内服务恢复
- 支持定时开关（工作日 09:00-18:00 开启，其余时间关闭）
- 支持一键关闭所有无线通道（"飞行模式"）
- 通道状态变更记录审计日志
- 装置重启后各通道恢复为断电前状态

### 6.5 省电策略

- 配置无线通道空闲超时时间（默认 300 秒无客户端连接自动关闭）
- 支持配置定时开关计划
- 低功耗模式下，NearLink 和 Wi-Fi AP 停止广播，蓝牙保持可发现模式
- 低功耗模式下功耗降低 >= 80%（相对全功率状态）
- 有连接请求时（如客户端扫描/配对）自动唤醒

### 6.6 认证锁定策略

| 通道 | 认证方式 | 锁定策略 |
|------|----------|----------|
| **NearLink** | PIN 码配对（外壳标签印刷） + 信任设备列表 | 5 次失败锁定 5 分钟 |
| **Wi-Fi AP** | WPA2-PSK / WPA3-SAE + Web UI 登录 Token | Web UI 登录 5 次失败锁定账户 |
| **Wi-Fi Station** | WPA2-PSK / WPA3-SAE | 依赖外部 AP 的认证 |
| **BLE** | Passkey Entry 配对 + 配置写入需额外 Token | 5 次配对失败锁定 BLE 5 分钟 |

```rust
pub struct AuthManager {
    lock_state: Arc<RwLock<HashMap<String, LockEntry>>>,
    config: AuthConfig,
    // trusted_devices: Arc<RwLock<Vec<TrustedDevice>>>,
}

impl AuthManager {
    pub async fn record_auth_failure(&self, identity: &str, channel: ChannelType) -> Result<LockStatus>;
    // pub async fn manage_trusted_devices(&self, ...) -> Result<()>;
}
```

> **注意：信任设备列表管理（白名单，最多 20 个信任设备）延后实现。Phase 2 仅支持基于密钥的认证（PIN 码 / Passkey Entry / WPA2-PSK）。信任设备列表功能将通过 `AuthManager` 的 `trusted_devices` 字段实现，支持添加、移除、查询信任设备操作。**

---

## 7. 与 OTA/日志/配置系统集成

### 7.1 与 web-api 的集成

web-api 在启动时调用 `wireless::register_routes(router)` 注册路由：

```rust
// web-api/src/lib.rs
use axum::Router;
use wireless::WirelessManager;

pub async fn build_app(wireless_mgr: Arc<WirelessManager>) -> Router {
    Router::new()
        .nest("/api/v1/wireless", wireless::routes(wireless_mgr))
        .nest("/api/v1/config", config_routes())
        .nest("/api/v1/logs", logs_routes())
        .nest("/api/v1/ota", ota_routes())
}
```

### 7.2 与 ota-update 的集成

```
┌──────────────────┐     ┌──────────────────┐
│  wireless crate  │     │  ota-update crate│
│                  │     │                  │
│  Wi-Fi file      │────>│  verify_firmware │
│  upload handler  │     │  (SHA-256 + 签名) │
│  (分块 + 断点续传) │     │                  │
│                  │<────│  progress_callback│
│  NearLink file   │     │  (进度通知)        │
│  transfer handler│     │                  │
│                  │     │  apply_firmware   │
│  BLE command     │────>│  (升级状态机)      │
│  (cmd: ota_*)    │     │                  │
└──────────────────┘     └──────────────────┘
```

**集成点：**
1. **Wi-Fi 固件上传**：上传完成后调用 `ota_update::verifier::verify_update_package()`
2. **NearLink 固件传输**：传输完成后调用相同验证流程
3. **BLE 控制命令**：`"ota_check"`、`"ota_trigger"`、`"rollback"` 命令转发给 `ota_update::OtaManager`
4. **升级进度**：`ota_update` 通过回调或 channel 将进度推送到 `wireless`，再通过 BLE Notify 或 NearLink Notify 转发给客户端

**文件路径约定：**
```
/tmp/mupc_update/          ← 上传固件临时存储（无线通道写入）
/tmp/mupc_update/partial/  ← 分块上传的中间文件
```

### 7.3 与日志系统的集成

```
┌──────────────────┐
│  wireless crate  │
│                  │
│  NearLink 日志导出 │────> 读取 /var/log/mupc/*.log
│  Wi-Fi 日志导出   │────> 按条件过滤 + 分块传输
│  BLE 日志导出     │────> 按条件过滤 + 512 字节分块
│                  │
│  审计日志写入      │────> /var/log/mupc/audit.log (追加)
└──────────────────┘
```

- 日志读取复用 `mupc-common` 或 web-api 的日志查询函数
- `wireless::audit::AuditLogger` 封装文件追加写入逻辑
- 日志文件轮转（logrotate）由系统负责，wireless crate 不管理
- 审计日志文件权限 `0600`，使用 `chattr +a`（如果文件系统支持）

### 7.4 与配置系统的集成

- 启动时读取 `/etc/mupc/mupc.toml` 中的 `[wireless]` 配置节
- 运行时通过 REST API 热加载配置变更
- 配置存储在装置主配置文件中，所有通道共享同一份配置

```toml
[wireless]

[wireless.nearlink]
enabled = true
device_name = "MUPC-{serial_suffix}"
pin_code = "123456"
advertising_interval_ms = 100
idle_timeout_seconds = 300
auto_start = true

[wireless.wifi]
[wireless.wifi.ap]
enabled = true
ssid = "MUPC-AP-{serial_suffix}"
password = "********"
band = "5ghz"
channel = 36
max_clients = 8
idle_timeout_seconds = 600
auto_start = true

[wireless.wifi.station]
enabled = false
scan_interval_seconds = 60
auto_start = false

[[wireless.wifi.station.networks]]
ssid = "site-wifi"
auth = "wpa2-psk"
password = "********"
priority = 1

[wireless.ble]
enabled = true
device_name = "MUPC-BLE-{serial_suffix}"
advertising_interval_ms = 100
pairing_mode = "passkey_entry"
auto_start = true

[wireless.global]
flight_mode = false
scheduled_on = "09:00"
scheduled_off = "18:00"
auth_lock_threshold = 5
auth_lock_duration_minutes = 5
```

### 7.5 与告警系统的集成

| 事件 | 触发 | 告警级别 |
|------|------|----------|
| 无线模组故障/驱动异常 | 初始化失败或运行时丢失 | WARNING |
| 认证锁定事件 | 连续 5 次认证失败 | WARNING |
| 固件升级完成 | OTA 流程结束 | INFO |
| 所有无线通道不可用 | 初始化后无一通道可用 | ERROR |

---

## 8. 接口定义

### 8.1 REST API 定义

所有 API 路径以 `/api/v1/wireless/` 为前缀。

#### 8.1.1 通道管理

```
GET  /api/v1/wireless/status          → 获取所有无线通道状态
POST /api/v1/wireless/{channel}/start → 启动指定通道
POST /api/v1/wireless/{channel}/stop  → 关闭指定通道
POST /api/v1/wireless/flight-mode     → 飞行模式开关
```

**通道状态响应：**
```json
{
  "channels": [
    {
      "type": "nearlink",
      "enabled": true,
      "state": "connected",
      "clients": 1,
      "clients_max": 4,
      "signal_dbm": -65,
      "tx_rate_mbps": 8.5,
      "rx_rate_mbps": 12.3,
      "total_tx_mb": 1024,
      "total_rx_mb": 2048,
      "uptime_seconds": 3600
    },
    {
      "type": "wifi_ap",
      "enabled": true,
      "state": "idle",
      "clients": 0,
      "clients_max": 8,
      "ssid": "MUPC-AP-A1B2C3",
      "band": "5ghz",
      "channel": 36,
      "tx_rate_mbps": 0,
      "uptime_seconds": 1800
    },
    {
      "type": "wifi_sta",
      "enabled": false,
      "state": "disabled",
      "ssid": "",
      "ip_address": ""
    },
    {
      "type": "ble",
      "enabled": true,
      "state": "idle",
      "clients": 0,
      "clients_max": 1,
      "uptime_seconds": 3600
    }
  ],
  "flight_mode": false
}
```

#### 8.1.2 配置读写

```
GET  /api/v1/wireless/config              → 获取无线完整配置
PUT  /api/v1/wireless/config              → 更新无线配置（全量覆盖）
PATCH /api/v1/wireless/config             → 部分更新无线配置
```

#### 8.1.3 Wi-Fi 网络扫描

```
GET /api/v1/wireless/wifi/scan           → 扫描可用 Wi-Fi 网络
```

```json
{
  "networks": [
    {
      "ssid": "Office-WiFi",
      "bssid": "aa:bb:cc:dd:ee:ff",
      "band": "5ghz",
      "channel": 36,
      "signal_dbm": -45,
      "auth": "wpa3-sae",
      "encrypted": true
    }
  ],
  "scan_time_ms": 3200
}
```

#### 8.1.4 定时计划配置

```
PUT /api/v1/wireless/schedule  → 配置无线定时开关计划
GET /api/v1/wireless/schedule  → 查询定时开关计划
```

```json
{
  "enabled": true,
  "schedule": {
    "monday":    { "on": "09:00", "off": "18:00" },
    "tuesday":   { "on": "09:00", "off": "18:00" },
    "wednesday": { "on": "09:00", "off": "18:00" },
    "thursday":  { "on": "09:00", "off": "18:00" },
    "friday":    { "on": "09:00", "off": "18:00" },
    "saturday":  { "on": null, "off": null },
    "sunday":    { "on": null, "off": null }
  }
}
```

#### 8.1.5 密钥协商

```
POST /api/v1/wireless/key-exchange
```

```json
// 请求
{ "pubkey": "base64-encoded-x25519-public-key", "algorithm": "X25519", "session_id": "uuid" }

// 响应
{ "pubkey": "base64-encoded-x25519-public-key", "algorithm": "X25519", "session_id": "uuid", "encrypted_test": "base64-encrypted-test" }
```

### 8.2 BLE GATT 接口

详见第 4 节 GATT 服务定义。

### 8.3 审计日志格式

所有无线通道的操作写入 `/var/log/mupc/audit.log`，每条记录为单行 JSON：

```json
{"timestamp":"2026-05-29T10:00:00Z","op":"config_write","identity":"ble:aa:bb:cc:dd:ee:ff","target":"wifi.ap.ssid","result":"success","channel":"ble","detail":"SSID changed to MUPC-AP-NEW"}
```

| 字段 | 说明 | 示例 |
|------|------|------|
| `timestamp` | UTC 操作时间 | `2026-05-29T10:00:00Z` |
| `op` | 操作类型 | `connect / disconnect / config_write / log_export / fw_upgrade / rollback` |
| `identity` | 操作者标识 | `nearlink:ab:cd:ef:01:23:45 / wifi:192.168.4.10 / ble:aa:bb:cc:dd:ee:ff` |
| `target` | 操作对象 | `wifi.ap.ssid / logs/export / fw/v1.3.0.tar.gz` |
| `result` | 操作结果 | `success / failure` |
| `channel` | 通道类型 | `nearlink / wifi / bluetooth` |
| `detail` | 详情描述 | `SSID changed to MUPC-AP-NEW` |
| `failure_reason` | 失败原因（result=failure时） | `Invalid config value` |

---

## 9. 文件结构

### 9.1 新增 crate 文件清单

```
crates/wireless/
├── Cargo.toml                              # 依赖配置
├── src/
│   ├── lib.rs                              # 模块导出、WirelessManager 构造 (~80 行)
│   ├── error.rs                            # 统一错误类型 (~60 行)
│   ├── config.rs                           # 配置结构 + 加载/保存 (~180 行)
│   ├── types.rs                            # 公共数据类型/枚举 (~120 行)
│   ├── state.rs                            # 通道状态管理 (~150 行)
│   ├── audit.rs                            # 审计日志写入器 (~120 行)
│   ├── auth.rs                             # 接入认证、Token 管理、锁定逻辑 (~200 行)
│   ├── manager.rs                          # WirelessManager 核心实现 (~250 行)
│   ├── web_api.rs                          # REST API 路由注册 (~200 行)
│   ├── scheduler.rs                        # 定时开关、省电策略 (~100 行)
│   │
│   ├── near_link/
│   │   ├── mod.rs                          # NearLink 模块入口 (~20 行)
│   │   ├── ffi.rs                          # 厂商 SDK FFI 绑定 (~150 行)
│   │   ├── controller.rs                   # NearLink 控制器 (~250 行)
│   │   ├── file_transfer.rs                # 文件/日志导出传输 (~200 行)
│   │   └── fw_upgrade.rs                   # 固件上传 + 断点续传 (~180 行)
│   │
│   ├── wifi/
│   │   ├── mod.rs                          # Wi-Fi 模块入口 (~20 行)
│   │   ├── hostapd.rs                      # AP 模式 (hostapd 控制器) (~250 行)
│   │   ├── supplicant.rs                   # Station 模式 (wpa_supplicant 控制器) (~250 行)
│   │   ├── scanner.rs                      # 网络扫描 (~100 行)
│   │   └── concurrency.rs                  # 并发模式检测与管理 (~80 行)
│   │
│   └── ble/
│       ├── mod.rs                          # BLE 模块入口 (~20 行)
│       ├── gatt_server.rs                  # GATT 服务定义 + 特征处理 (~300 行)
│       ├── config_handler.rs               # 配置读写特征处理 (~100 行)
│       ├── log_export_handler.rs           # 日志导出特征处理 (~150 行)
│       └── fw_command_handler.rs           # 固件控制命令处理 (~100 行)
│
├── tests/
│   ├── integration_test.rs                 # 集成测试 (~200 行)
│   ├── mock_driver.rs                      # 驱动层 mock (~150 行)
│   └── ble_gatt_test.rs                    # BLE GATT 测试 (~120 行)
│
└── unsafe_sdk/                             # (可选) 厂商 SDK 头文件
    └── hi2821_nearlink.h                    # FFI 绑定源
```

### 9.2 预估代码行数

| 模块 | 文件数 | 预估行数 |
|------|--------|---------|
| 公共模块 (lib/config/error/types/state/audit/auth/manager/web_api/scheduler) | 10 | 1,310 |
| NearLink (ffi/controller/file_transfer/fw_upgrade) | 5 | 800 |
| Wi-Fi (hostapd/supplicant/scanner/concurrency) | 5 | 700 |
| BLE (gatt_server/config_handler/log_export_handler/fw_command_handler) | 5 | 670 |
| 测试 | 3 | 470 |
| 配置/构建 (Cargo.toml, unsafe_sdk) | 2 | 100 |
| **合计** | **30** | **~4,050** |

### 9.3 修改的现有文件

| 文件路径 | 变更类型 | 说明 |
|---------|---------|------|
| `crates/web-api/src/router.rs` | 修改 | 注册 `/api/v1/wireless/*` 路由 |
| `crates/web-api/Cargo.toml` | 修改 | 添加 `wireless` 依赖 |
| `mupc/Cargo.toml` | 修改 | 添加 `crates/wireless` 到 workspace members |
| `/etc/mupc/mupc.toml` | 新增配置节 | 添加 `[wireless]` 配置节 |

### 9.4 Cargo.toml 依赖

```toml
[package]
name = "mupc-wireless"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
# 基础
tokio.workspace = true
tracing.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
anyhow.workspace = true
chrono.workspace = true
uuid.workspace = true
async-trait.workspace = true
parking_lot.workspace = true
futures.workspace = true

# HTTP Web
axum.workspace = true

# BLE
bluer = "0.20"

# 子进程管理 (Wi-Fi) - 使用 tokio::process::Command 无需额外依赖

# 可选：FFI 绑定 (NearLink SDK)
bindgen = { version = "0.69", optional = true }

# workspace 内部依赖
mupc-common = { path = "../common" }
mupc-core = { path = "../core" }
mupc-ota-update = { path = "../ota-update" }
mupc-security = { path = "../security" }

[features]
default = []
nearlink = ["dep:bindgen"]   # 启用 NearLink SDK FFI 绑定
ble = []                     # 启用 BLE (默认启用，bluer 为必需)
test-mock = []               # 测试模式（mock 底层驱动）

[dev-dependencies]
tokio-test.workspace = true
tempfile.workspace = true
```

---

## 10. 技术决策记录

### 10.1 决策汇总

| 序号 | 决策点 | 方案 | 备选 | 理由 |
|------|--------|------|------|------|
| D-01 | crate 合并策略 | 合并为 `wireless` crate | 分拆为三个独立 crate | 共享公共逻辑（审计日志、配置、状态管理），避免重复；与 web-api 集成只需一次路由注册 |
| D-02 | NearLink SDK 绑定 | FFI 绑定（方案 A），C 守护进程（方案 B 降级） | 纯 Rust 实现 | 开发效率高，性能优；降级策略保证 SDK 不可用时的可替代性 |
| D-03 | Wi-Fi 控制方式 | `hostapd` + `wpa_supplicant` 子进程 | `nl80211` netlink | 稳定成熟，openEuler 标准包，控制接口 socket 支持动态管理 |
| D-04 | BLE 库选型 | `bluer` crate | `btleplug` / `zbus` | 原生 async/await，GATT Server 支持，openEuler 预装 BlueZ |
| D-05 | 端到端加密 | ECDH X25519 + AES-256-GCM | TLS 1.3 单独使用 | 统一各通道加密协议栈；无证书场景安全保障；防御 TLS MITM |
| D-06 | UUID 合规 | Bluetooth Base UUID 格式自定义区间 | 随机 UUID | 标准兼容，避免 SIG 冲突，客户端 SDK 友好 |
| D-07 | 密钥轮换 | 24h/1GB 周期轮换 + 事件驱动 + 客户端主动 | 仅连接建立时协商 | 满足安全余量，支持自动重连无缝切换 |
| D-08 | Wi-Fi 并发双模式 | 运行时检测硬件能力 + 配置文件策略 | 假设硬件支持 | 三种情形（并发/单模式/双物理）统一接口，不依赖特定芯片 |

### 10.2 与 PRD 阻塞性问题的对应关系

| PRD 阻塞问题 | 本方案的处理 |
|-------------|------------|
| Q1: 无线模组型号 | Wi-Fi/BLE 假定为 M.2 Key E 模组（AP6275P 类），NearLink 假定为 Hi2821 USB 模组。设计上通过 `detect_*_module()` 函数与具体硬件解耦，更换模组不影响上层逻辑。 |
| Q2: 星闪 Linux 驱动 | 采用双轨策略：首选 FFI 绑定厂商 SDK；若不可用降级为 C 守护进程 + Unix Socket。`near_link::controller` 对外接口不变。 |
| Q3: Wi-Fi AP+Station 并发 | 通过 `ConcurrencyDetector` 运行时检测硬件能力，配置 `concurrency_mode` 字段适应三种情形。即使硬件不支持并发，也保证 AP 和 Station 可单独运行。 |

### 10.3 实施优先级

| 阶段 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **Phase 1** | Wi-Fi AP 模式 + BLE 设备状态 | P0 | 先实现最成熟的两条通道，满足基本运维需求 |
| **Phase 2** | NearLink + Wi-Fi Station | P1 | 等待星闪模组和 SDK 就绪后实施 |
| **Phase 3** | BLE 配置读写 + 日志导出 + 固件升级命令 | P1 | BLE 通道功能完善 |
| **Phase 4** | 定时开关 + 省电策略 + 多通道协同 | P2 | 高级功能 |
| **Phase 5** | 远程锁机 + 信任设备列表管理（白名单） | P3 | Phase 2 仅支持基于密钥的认证，信任设备列表延后实现 |

### 10.4 边界条件处理

| 场景 | 处理策略 |
|------|----------|
| 无线模组不可用 | 初始化时检测各模组状态，不可用通道标记为 `unavailable`；所有通道不可用时发送告警，装置主功能不受影响 |
| 多通道并发配置冲突 | AP 和 Station 同频段时给出警告；推荐 AP 用 5 GHz、Station 用 2.4 GHz 或反之 |
| 固件升级中断 | 支持断点续传（NearLink/Wi-Fi）；不完整固件不触发升级；记录审计日志 |
| 同时接入多个运维通道 | 配置写操作以后写入覆盖先写入（按时间戳）；固件升级仅允许单一通道触发 |
| 认证锁定与攻击防护 | 连续 5 次认证失败锁定通道 5 分钟；记录安全审计日志；Web UI 显示安全告警 |
| BLE 连接频繁断开 | 30 秒内断开重连超过 3 次时延长广播间隔 (100ms → 500ms) |

### 10.5 待硬件团队确认事项

| 序号 | 事项 | 影响 |
|------|------|------|
| 1 | RK3588 开发板无线模组型号（影响 Wi-Fi 驱动 + BLE 共存） | Wi-Fi + BLE 实现 |
| 2 | 是否预留 M.2 Key E 接口 | Wi-Fi 模组选型 |
| 3 | 星闪模组型号和接口 | NearLink 实现 |
| 4 | 天线接口和增益规格 | 覆盖范围验证 |
| 5 | Wi-Fi 芯片是否支持 AP + Station 并发 | Wi-Fi AP/Station 设计 |

---

## 附录：版本演进

> 正文已整合全部历史补丁，本表仅作演进追溯。

| 版本 | 主要变更 |
|------|----------|
| v1.0 | 初始设计 |
| v1.1 | 新增 AP 零影响约束、BLE 日志导出 max_entries、信任设备列表延后标注 |
