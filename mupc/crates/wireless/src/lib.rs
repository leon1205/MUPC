//! MUPC 台区本地无线通信库
//!
//! 提供台区设备近场无线通信的驱动抽象，包括：
//! - **星闪（NearLink）**：中国自主短距无线标准，<20us 低时延，用于实时控制
//! - **WiFi**：标准 WiFi 通信，用于监控摄像头、环境传感器
//! - **BLE（低功耗蓝牙）**：低功耗传感器、手持巡检终端
//! - **ECDH 加密**：P-256 密钥交换 + AES-256-GCM 链路加密
//!
//! # 架构
//!
//! ```text
//! strategy-engine
//!       │
//!       ▼
//! wireless crate (本 crate)
//!   ├── NearLinkDriver  trait  ←→ 台区设备（星闪）
//!   ├── WiFiDriver       trait  ←→ 台区设备（WiFi）
//!   ├── BleDriver        trait  ←→ 台区设备（BLE）
//!   └── EcdhKeyPair            ←→ 链路加密
//! ```
//!
//! # 开发状态（v3.1）
//!
//! **当前阶段：Phase 1 接口骨架。全部硬件驱动为 NoOp 占位实现。**
//!
//! | 驱动 | 阻塞条件 | 目标 SDK | 预计 Phase |
//! |------|----------|----------|:--:|
//! | NearLink (星闪) | 需星闪芯片 SDK（海思 Hi2821 等）及硬件 Bring-up | 芯片厂商 SDK | Phase 2+ |
//! | WiFi | 需 Linux nl80211 / wpa_supplicant D-Bus API 接入 | nl80211 + wpa_supplicant | Phase 2+ |
//! | BLE | 需 Linux BlueZ D-Bus API 或 ESP32 BLE 协议栈 | BlueZ 5.x | Phase 2+ |
//! | ECDH P-256 | XOR 占位（非加密安全），待替换为 `p256::ecdh::diffie_hellman()` | p256 crate | Phase 2 |
//!
//! 上述驱动在无硬件环境下均返回 `WirelessError::UnsupportedDevice("功能尚未启用（Phase 2+ 实现）")`。`NoOp*` 实现设计为 graceful degradation——调用方应检查返回值并回退到有线通信路径。

pub mod ble;
pub mod ecdh;
pub mod errors;
pub mod nearlink;
pub mod wifi;

// 公开类型 re-export
pub use ble::{BleConfig, BleDriver, BleScanResult, NoOpBleDriver};
pub use ecdh::{derive_aes_key, EcdhKeyPair};
pub use errors::WirelessError;
pub use nearlink::{NearLinkConfig, NearLinkDriver, NoOpNearLinkDriver};
pub use wifi::{NoOpWiFiDriver, WiFiConfig, WiFiDriver, WiFiScanResult, WiFiSecurity};
