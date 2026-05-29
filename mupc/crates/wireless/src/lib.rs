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
//! # 开发状态
//!
//! - Phase 1：trait 接口定义 + 配置结构体框架（当前阶段）
//! - Phase 2+：实际硬件 SDK 集成（星闪芯片 / nl80211 / BlueZ）

pub mod errors;
pub mod nearlink;
pub mod wifi;
pub mod ble;
pub mod ecdh;

// 公开类型 re-export
pub use errors::WirelessError;
pub use nearlink::{NearLinkConfig, NearLinkDriver, NoOpNearLinkDriver};
pub use wifi::{WiFiConfig, WiFiDriver, WiFiScanResult, WiFiSecurity, NoOpWiFiDriver};
pub use ble::{BleConfig, BleDriver, BleScanResult, NoOpBleDriver};
pub use ecdh::{derive_aes_key, EcdhKeyPair};
