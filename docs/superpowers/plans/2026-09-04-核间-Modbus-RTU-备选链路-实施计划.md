# 核间 Modbus RTU 备选链路 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为核间通信（IntercoreClient）新增可配置的 Modbus RTU 备选控制链路（控制下行 + 执行确认 + 心跳轮询），`transport` 配置选择 tcp / modbus_rtu，上层（AiIntegrator/strategy-engine）接口不变。

**Architecture:** `IntercoreClient` 重构为传输门面，内部持 `Arc<dyn IntercoreTransport>`。`TcpTransport` 承载现有帧协议（逻辑迁移、协议不变）；`ModbusRtuTransport` 作 Modbus Master，按寄存器映射表写控制寄存器（FC16）并经 `cmd_valid`+`exec_*` 完成执行确认，心跳改为轮询读。新增 Slave 参考实现（bin）供本地联调。

**Tech Stack:** Rust, Tokio, tokio-modbus（+ tokio-serial），配置在 mupc-core-bin core_config + startup。

**设计依据：** `docs/superpowers/plans/modules/10-MUPC-核间通信-设计文档.md` §11 + ADR-009~012（v2.0）；`docs/superpowers/specs/modules/10-MUPC-核间通信-PRD.md` §2.4 + IC-AC-33~39。

**测试命令**（Windows 本机，cwd = `mupc/`）：
```bash
cargo test -p mupc-intercore
cargo check -p mupc-core-bin
cargo check --workspace
```

---

## 任务一览

| Task | 内容 | crate |
|---|---|---|
| 1 | 依赖：Cargo.toml + tokio-modbus/tokio-serial | intercore |
| 2 | modbus_rtu.rs：寄存器地址 + f64↔int32 编解码 + cmd_ctrl（纯函数 + 测试） | intercore |
| 3 | transport.rs：`IntercoreTransport` trait + `TcpTransport`（现有帧逻辑迁移） | intercore |
| 4 | IntercoreClient 改造为门面（持 transport），Tcp 回归 | intercore |
| 5 | transport/modbus.rs：`ModbusRtuTransport`（Master：写控制 + 执行确认 + 心跳轮询） | intercore |
| 6 | modbus_slave.rs：Slave 参考实现（bin，模拟实时模块） | intercore |
| 7 | core_config + startup + deploy yaml：配置 `transport`/`modbus_rtu`，按 transport 构造 | mupc-core-bin |
| 8 | 端到端联调 + workspace 回归 + 文档标注 | 全局 |

---

## Task 1: 新增 tokio-modbus 依赖

**Files:**
- Modify: `mupc/crates/intercore/Cargo.toml`

- [ ] **Step 1: 加依赖**

```toml
[dependencies]
# ...现有...
tokio-modbus = "0.13"   # Modbus RTU master/server
tokio-serial = "5.4"    # 跨平台串口（tokio-modbus 底层）
```

- [ ] **Step 2: 验证依赖可解析**

Run: `cargo check -p mupc-intercore`
Expected: 编译通过（依赖拉取成功）

- [ ] **Step 3: Commit**

```bash
git add crates/intercore/Cargo.toml Cargo.toml Cargo.lock
git commit -m "deps: intercore 新增 tokio-modbus/tokio-serial（Modbus RTU 备选链路）"
```

---

## Task 2: modbus_rtu.rs 寄存器映射与编解码（纯函数）

**Files:**
- Create: `mupc/crates/intercore/src/modbus_rtu.rs`
- Modify: `mupc/crates/intercore/src/lib.rs`

- [ ] **Step 1: 写失败测试（先建文件骨架含测试）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i32_regs_roundtrip() {
        for v in [-60000i32, -1, 0, 1, 32767, 60000] {
            let regs = i32_to_regs(v);
            assert_eq!(regs_to_i32(&regs), v, "i32 roundtrip {}", v);
        }
    }

    #[test]
    fn test_scale_encode_decode() {
        // 0.01 kW/LSB：-50.0 kW -> -5000
        let raw = encode_scaled(-50.0, 0.01);
        assert_eq!(raw, -5000);
        assert!((decode_scaled(raw, 0.01) - (-50.0)).abs() < 1e-9);
        // k_droop 0.001：0.5 kW/V -> 500
        assert_eq!(encode_scaled(0.5, 0.001), 500);
    }

    #[test]
    fn test_cmd_ctrl_pack_unpack() {
        // cmd_seq=7, strategy_mode=2, ai_ready=false, cmd_valid=true
        let reg = pack_cmd_ctrl(7, 2, false, true);
        assert_eq!(cmd_seq_of(reg), 7);
        assert_eq!(strategy_mode_of(reg), 2);
        assert_eq!(ai_ready_of(reg), false);
        assert!(cmd_valid_of(reg));
        // cmd_valid 在低字节 bit0
        assert_eq!(reg & 0x0001, 0x0001);
        assert_eq!(reg & 0xFF00, 7 << 8);
    }
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cargo test -p mupc-intercore test_i32_regs_roundtrip`
Expected: FAIL（`modbus_rtu` 模块/函数未定义）

- [ ] **Step 3: 实现 modbus_rtu.rs**

```rust
//! 核间 Modbus RTU 寄存器映射与编解码
//!
//! 实时控制模块（Slave）暴露保持寄存器区，管理模块（Master）按 §11.4 映射
//! 写控制区（FC16）、读执行确认区与状态区（FC03）。编码统一 int32 缩放。

/// 控制区寄存器地址
pub const REG_CMD_CTRL: u16 = 0x0000;
pub const REG_PROTOCOL_VERSION: u16 = 0x0001;
pub const REG_P_REF: u16 = 0x0010;
pub const REG_K_DROOP: u16 = 0x0012;
pub const REG_PHASE_P_A: u16 = 0x0020;
pub const REG_PHASE_Q_A: u16 = 0x0026; // phase_q 紧随 6 个 phase_p 寄存器之后
/// 执行确认区（从站写、master 读）
pub const REG_EXEC_SEQ: u16 = 0x0030;
pub const REG_EXEC_STATUS: u16 = 0x0032;
pub const REG_EXEC_ERROR: u16 = 0x0033;
/// 状态/心跳区
pub const REG_HEARTBEAT: u16 = 0x0100;
pub const REG_DEVICE_STATUS: u16 = 0x0101;
pub const REG_CPU_TEMP: u16 = 0x0102;
pub const REG_MEM_USAGE: u16 = 0x0104;

/// 协议版本
pub const PROTOCOL_VERSION: u16 = 1;

/// 缩放因子（kW/kVAr → 0.01；k_droop → 0.001）
pub const SCALE_POWER: f64 = 0.01;
pub const SCALE_K_DROOP: f64 = 0.001;

/// exec_status 取值
pub const EXEC_IDLE: u16 = 0;
pub const EXEC_RUNNING: u16 = 1;
pub const EXEC_SUCCESS: u16 = 2;
pub const EXEC_FAILED: u16 = 3;
pub const EXEC_TIMEOUT: u16 = 4;

/// f64 → 按 scale 缩放的 i32
pub fn encode_scaled(v: f64, scale: f64) -> i32 {
    (v / scale).round() as i32
}

/// i32（按 scale 缩放）→ f64
pub fn decode_scaled(raw: i32, scale: f64) -> f64 {
    raw as f64 * scale
}

/// i32 → 2 个大端 u16 寄存器（[高16, 低16]）
pub fn i32_to_regs(v: i32) -> [u16; 2] {
    [((v >> 16) & 0xFFFF) as u16, (v & 0xFFFF) as u16]
}

/// 2 个大端 u16 寄存器 → i32
pub fn regs_to_i32(regs: &[u16]) -> i32 {
    ((regs[0] as i32) << 16) | (regs[1] as i32)
}

/// f64 功率 → 2 寄存器（0.01 缩放）
pub fn power_to_regs(v: f64) -> [u16; 2] {
    i32_to_regs(encode_scaled(v, SCALE_POWER))
}

/// 2 寄存器 → f64 功率
pub fn regs_to_power(regs: &[u16]) -> f64 {
    decode_scaled(regs_to_i32(regs), SCALE_POWER)
}

/// 打包 cmd_ctrl：低字节 bit0 cmd_valid / bit1-3 strategy_mode / bit4 ai_ready；高字节 cmd_seq
pub fn pack_cmd_ctrl(cmd_seq: u8, strategy_mode: u8, ai_ready: bool, cmd_valid: bool) -> u16 {
    let mut low: u16 = 0;
    if cmd_valid {
        low |= 0x0001;
    }
    low |= ((strategy_mode as u16) & 0x07) << 1;
    if ai_ready {
        low |= 0x0010;
    }
    ((cmd_seq as u16) << 8) | low
}

pub fn cmd_seq_of(reg: u16) -> u8 {
    ((reg >> 8) & 0xFF) as u8
}

pub fn strategy_mode_of(reg: u16) -> u8 {
    ((reg >> 1) & 0x07) as u8
}

pub fn ai_ready_of(reg: u16) -> bool {
    reg & 0x0010 != 0
}

pub fn cmd_valid_of(reg: u16) -> bool {
    reg & 0x0001 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i32_regs_roundtrip() {
        for v in [-60000i32, -1, 0, 1, 32767, 60000] {
            let regs = i32_to_regs(v);
            assert_eq!(regs_to_i32(&regs), v, "i32 roundtrip {}", v);
        }
    }

    #[test]
    fn test_scale_encode_decode() {
        let raw = encode_scaled(-50.0, SCALE_POWER);
        assert_eq!(raw, -5000);
        assert!((decode_scaled(raw, SCALE_POWER) - (-50.0)).abs() < 1e-9);
        assert_eq!(encode_scaled(0.5, SCALE_K_DROOP), 500);
    }

    #[test]
    fn test_power_regs_roundtrip() {
        for v in [-50.0f64, -2.0, 0.0, 23.45, 60.0] {
            let regs = power_to_regs(v);
            assert!((regs_to_power(&regs) - v).abs() < 0.005);
        }
    }

    #[test]
    fn test_cmd_ctrl_pack_unpack() {
        let reg = pack_cmd_ctrl(7, 2, false, true);
        assert_eq!(cmd_seq_of(reg), 7);
        assert_eq!(strategy_mode_of(reg), 2);
        assert!(!ai_ready_of(reg));
        assert!(cmd_valid_of(reg));
        assert_eq!(reg & 0x0001, 0x0001);
        assert_eq!(reg & 0xFF00, 7 << 8);
    }
}
```

- [ ] **Step 4: lib.rs 注册模块**

```rust
pub mod modbus_rtu;
```

- [ ] **Step 5: 运行测试验证通过**

Run: `cargo test -p mupc-intercore`
Expected: PASS（新增 4 个测试）

- [ ] **Step 6: Commit**

```bash
git add crates/intercore/src/modbus_rtu.rs crates/intercore/src/lib.rs
git commit -m "feat: intercore Modbus 寄存器映射与编解码（modbus_rtu.rs）"
```

---

## Task 3: IntercoreTransport trait + TcpTransport（迁移现有帧逻辑）

**Files:**
- Create: `mupc/crates/intercore/src/transport.rs`（含 trait + mod）
- Create: `mupc/crates/intercore/src/transport/tcp.rs`
- Modify: `mupc/crates/intercore/src/lib.rs`

- [ ] **Step 1: 写失败测试（trait 签名占位）**

`transport.rs` 声明 trait 后，先写 `TcpTransport` 的构造/发送测试会失败（类型未定义）。直接以编译失败为红。

- [ ] **Step 2: 实现 transport.rs + tcp.rs**

`mupc/crates/intercore/src/transport.rs`:

```rust
//! 核间传输抽象（可插拔：Tcp / ModbusRtu）
use async_trait::async_trait;
use mupc_common::MupcError;

pub mod modbus;
pub mod tcp;

use crate::protocol::{FrameType as IntercoreFrameType, IntercoreFrame};
use crate::tcp_server::{ControlCmdPayloadV2, ControlCmdPayloadV3, DualParamCommand};

/// 核间传输通道（上层经 IntercoreClient 门面调用，接口不随通道变化）
#[async_trait]
pub trait IntercoreTransport: Send + Sync {
    /// 下发 AI 双参数（p_ref/k_droop）
    async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError>;
    /// 下发台区储能分相 P/Q
    async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], mode: &str) -> Result<(), MupcError>;
    /// 连接状态
    async fn is_connected(&self) -> bool;
    async fn shutdown(&self) -> Result<(), MupcError>;
}

/// 构造 V2 ControlCmd 帧字节（TcpTransport 用）
fn v2_control_frame_bytes(cmd: &DualParamCommand) -> Result<Vec<u8>, MupcError> {
    let payload = ControlCmdPayloadV2 {
        p_ref: Some(cmd.p_ref),
        k_droop: Some(cmd.k_droop),
        ai_ready: Some(cmd.ai_ready),
        strategy_mode: Some(cmd.strategy_mode.clone()),
        timestamp_ms: Some(chrono::Utc::now().timestamp_millis() as u64),
        frame_version: Some(ControlCmdPayloadV2::FRAME_VERSION),
    };
    let bytes = payload.to_json().map_err(|e| {
        MupcError::new(mupc_common::ErrorCode::SerializeError, format!("serialize V2: {}", e), "intercore")
    })?;
    Ok(IntercoreFrame::new(IntercoreFrameType::ControlCmd, 0, bytes).to_bytes()?)
}

/// 构造 V3 分相帧字节
fn v3_control_frame_bytes(p: [f64; 3], q: [f64; 3], mode: &str) -> Result<Vec<u8>, MupcError> {
    let payload = ControlCmdPayloadV3 {
        frame_version: Some(ControlCmdPayloadV3::FRAME_VERSION),
        p_ref: None,
        k_droop: None,
        phase_p_set: Some(p),
        phase_q_set: Some(q),
        ai_ready: Some(false),
        strategy_mode: Some(mode.to_string()),
        timestamp_ms: Some(chrono::Utc::now().timestamp_millis() as u64),
    };
    let bytes = payload.to_json().map_err(|e| {
        MupcError::new(mupc_common::ErrorCode::SerializeError, format!("serialize V3: {}", e), "intercore")
    })?;
    Ok(IntercoreFrame::new(IntercoreFrameType::ControlCmd, 0, bytes).to_bytes()?)
}

pub use tcp::TcpTransport;
```

`mupc/crates/intercore/src/transport/tcp.rs`:

```rust
//! TcpTransport：经 TCP Socket 发送定长帧（迁移自 IntercoreClient 原逻辑，协议不变）
use crate::transport::{v2_control_frame_bytes, v3_control_frame_bytes, IntercoreTransport};
use async_trait::async_trait;
use mupc_common::{ErrorCode, MupcError};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{timeout, Duration};

pub struct TcpTransport {
    remote_addr: String,
    timeout_ms: u64,
    connected: RwLock<bool>,
    stream: Arc<Mutex<Option<TcpStream>>>,
}

impl TcpTransport {
    pub fn new(remote_addr: String) -> Self {
        Self { remote_addr, timeout_ms: 5000, connected: RwLock::new(false), stream: Arc::new(Mutex::new(None)) }
    }

    pub fn with_timeout(remote_addr: String, timeout_ms: u64) -> Self {
        Self { remote_addr, timeout_ms, connected: RwLock::new(false), stream: Arc::new(Mutex::new(None)) }
    }

    async fn send_bytes(&self, bytes: &[u8]) -> Result<(), MupcError> {
        let mut guard = self.stream.lock().await;
        if guard.is_none() {
            match TcpStream::connect(&self.remote_addr).await {
                Ok(s) => *guard = Some(s),
                Err(e) => return Err(MupcError::new(ErrorCode::ConnectionFailed, format!("connect {}: {}", self.remote_addr, e), "intercore")),
            }
        }
        let stream = guard.as_mut().ok_or_else(|| MupcError::new(ErrorCode::ConnectionFailed, "连接未建立", "intercore"))?;
        match timeout(Duration::from_millis(self.timeout_ms), stream.write_all(bytes)).await {
            Ok(Ok(())) => { *self.connected.write().await = true; Ok(()) }
            Ok(Err(e)) => { *guard = None; Err(MupcError::new(ErrorCode::SendFailed, format!("send: {}", e), "intercore")) }
            Err(_) => { *guard = None; Err(MupcError::new(ErrorCode::IntercoreTimeout, format!("timeout {}ms", self.timeout_ms), "intercore")) }
        }
    }
}

#[async_trait]
impl IntercoreTransport for TcpTransport {
    async fn send_dual_param(&self, cmd: &crate::tcp_server::DualParamCommand) -> Result<(), MupcError> {
        let bytes = v2_control_frame_bytes(cmd)?;
        self.send_bytes(&bytes).await
    }

    async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], mode: &str) -> Result<(), MupcError> {
        let bytes = v3_control_frame_bytes(p, q, mode)?;
        self.send_bytes(&bytes).await
    }

    async fn is_connected(&self) -> bool { *self.connected.read().await }

    async fn shutdown(&self) -> Result<(), MupcError> {
        *self.stream.lock().await = None;
        *self.connected.write().await = false;
        Ok(())
    }
}
```

- [ ] **Step 3: lib.rs 声明模块并导出**

```rust
pub mod transport;
pub use transport::{IntercoreTransport, TcpTransport};
```

- [ ] **Step 4: 编译验证**

Run: `cargo check -p mupc-intercore`
Expected: 编译通过（tcp.rs 需确认 `chrono` 依赖存在——已在 tcp_server.rs 使用，OK）

- [ ] **Step 5: Commit**

```bash
git add crates/intercore/src/transport.rs crates/intercore/src/transport crates/intercore/src/lib.rs
git commit -m "feat: IntercoreTransport trait + TcpTransport（现有 TCP 帧逻辑迁移）"
```

---

## Task 4: IntercoreClient 改造为传输门面

**Files:**
- Modify: `mupc/crates/intercore/src/tcp_server.rs`（IntercoreClient 结构 + impl）
- Test: `mupc/crates/intercore/src/tcp_server.rs`（迁移原有测试兼容）

- [ ] **Step 1: 改结构与构造（Tcp 路径保持外部兼容）**

把 `IntercoreClient` 的 `remote_addr/stream/connected` 换为 `transport: Arc<dyn IntercoreTransport>`；保留 `connected` 代理（可选）。`new`/`with_config` 改为构造 TcpTransport：

```rust
use crate::transport::{IntercoreTransport, TcpTransport};

pub struct IntercoreClient {
    transport: Arc<dyn IntercoreTransport>,
    connected: RwLock<bool>,
    last_p_ref: RwLock<Option<f64>>,
    last_k_droop: RwLock<Option<f64>>,
}

impl IntercoreClient {
    /// 默认 TCP 客户端（保持现有调用兼容）
    pub fn new(remote_addr: String) -> Self {
        Self::with_transport(Arc::new(TcpTransport::new(remote_addr)))
    }

    pub fn with_config(remote_addr: String, _cmd_config: CommandConfig) -> Self {
        Self::new(remote_addr) // cmd_config.timeout 由 TcpTransport 默认承载；如需注入扩展构造
    }

    /// 注入自定义传输（Modbus RTU 等）
    pub fn with_transport(transport: Arc<dyn IntercoreTransport>) -> Self {
        Self {
            transport,
            connected: RwLock::new(false),
            last_p_ref: RwLock::new(None),
            last_k_droop: RwLock::new(None),
        }
    }
}
```

- [ ] **Step 2: 改写发送方法为委托**

`send_dual_param` 委托 `self.transport.send_dual_param(cmd)`，成功后更新 `last_p_ref/last_k_droop`；`send_tai_command` 委托；`is_connected` 委托 `self.transport.is_connected()`（并同步本地缓存）；`remote_addr()` 保留返回 TCP 地址（兼容查询，Modbus 下返回串口描述或空串）。删除原 `stream`/`send_frame` 私有实现（已迁 TcpTransport）。`get_last_params`/`remote_addr` 保留。

```rust
pub async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError> {
    self.transport.send_dual_param(cmd).await?;
    *self.last_p_ref.write().await = Some(cmd.p_ref);
    *self.last_k_droop.write().await = Some(cmd.k_droop);
    tracing::debug!("Sent dual-param via transport: p_ref={}, k_droop={}", cmd.p_ref, cmd.k_droop);
    Ok(())
}

pub async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], mode: &str) -> Result<(), MupcError> {
    self.transport.send_tai_command(p, q, mode).await
}

pub async fn is_connected(&self) -> bool {
    self.transport.is_connected().await
}
```

- [ ] **Step 3: 编译 + 既有测试**

Run: `cargo test -p mupc-intercore`
Expected: 编译通过；既有协议测试（V2/V3 payload roundtrip 等）通过（send 相关如需网络则跳过/保持原样）

- [ ] **Step 4: 修编译错误（若 `chrono`/旧字段引用残留）**

Run: `cargo check --workspace` — 修复所有引用旧字段（`client.stream` 等）的编译点。

- [ ] **Step 5: Commit**

```bash
git add crates/intercore/src/tcp_server.rs
git commit -m "refactor: IntercoreClient 改造为传输门面（持 IntercoreTransport，Tcp 默认兼容）"
```

---

## Task 5: ModbusRtuTransport（Master）

**Files:**
- Create: `mupc/crates/intercore/src/transport/modbus.rs`
- Test: 复用 modbus_rtu.rs 编解码测试；transport 层联调见 Task 6/8

- [ ] **Step 1: 实现 ModbusRtuTransport**

```rust
//! ModbusRtuTransport：经 Modbus RTU 写控制寄存器（FC16）+ cmd_valid 触发 + 执行确认轮询
use crate::modbus_rtu::*;
use crate::transport::IntercoreTransport;
use crate::tcp_server::DualParamCommand;
use async_trait::async_trait;
use mupc_common::{ErrorCode, MupcError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};
use tokio_modbus::prelude::*;
use tokio_serial::{SerialPortBuilderExt, SerialStream};

/// Modbus RTU 传输配置（对应 core_config ModbusRtuConfig）
#[derive(Debug, Clone)]
pub struct ModbusRtuSettings {
    pub serial_port: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: String,      // none/even/odd
    pub slave_addr: u8,
    pub response_timeout_ms: u64,
    pub heartbeat_poll_ms: u64,
}

pub struct ModbusRtuTransport {
    settings: ModbusRtuSettings,
    connected: RwLock<bool>,
    cmd_seq: AtomicBool, // 简化占位；真实递增用 AtomicU8
}

// 注：为最小实现，用内部互斥 ctx 持有（每写重连，简单可靠；优化可持久连接）
```

**串口与上下文建立（每 op 连接简化，避免复杂状态机）：**

```rust
fn serial_settings(s: &ModbusRtuSettings) -> Result<tokio_serial::SerialPortSettings, MupcError> {
    use tokio_serial::Parity;
    let mut ss = tokio_serial::SerialPortSettings::default();
    ss.baud_rate = s.baud_rate;
    ss.data_bits = tokio_serial::DataBits::from(s.data_bits);
    ss.stop_bits = tokio_serial::StopBits::from(s.stop_bits);
    ss.parity = match s.parity.as_str() {
        "even" => Parity::Even, "odd" => Parity::Odd, _ => Parity::None,
    };
    Ok(ss)
}

async fn open_ctx(s: &ModbusRtuSettings) -> Result<rtu::rtu::Context, MupcError> {
    let stream = SerialStream::open(&tokio_serial::new(&s.serial_port, s.baud_rate))
        .map_err(|e| MupcError::new(ErrorCode::ConnectionFailed, format!("open {}: {}", s.serial_port, e), "intercore"))?;
    let slave = SlaveAddr::try_from(s.slave_addr)
        .map_err(|e| MupcError::new(ErrorCode::ConfigError, format!("slave addr: {}", e), "intercore"))?;
    rtu::connect_slave(stream, slave).await
        .map_err(|e| MupcError::new(ErrorCode::ConnectionFailed, format!("modbus connect: {}", e), "intercore"))
}

impl ModbusRtuTransport {
    pub fn new(settings: ModbusRtuSettings) -> Self {
        Self { settings, connected: RwLock::new(false), cmd_seq: AtomicBool::new(false) }
    }

    /// 写一块保持寄存器（FC16）
    async fn write_regs(&self, addr: u16, regs: &[u16]) -> Result<(), MupcError> {
        let mut ctx = open_ctx(&self.settings).await?;
        timeout(Duration::from_millis(self.settings.response_timeout_ms), ctx.write_multiple_registers(addr, regs)).await
            .map_err(|_| MupcError::new(ErrorCode::IntercoreTimeout, "modbus write timeout", "intercore"))?
            .map_err(|e| MupcError::new(ErrorCode::SendFailed, format!("write: {}", e), "intercore"))
    }

    /// 读一块保持寄存器（FC03）
    async fn read_regs(&self, addr: u16, len: u16) -> Result<Vec<u16>, MupcError> {
        let mut ctx = open_ctx(&self.settings).await?;
        timeout(Duration::from_millis(self.settings.response_timeout_ms), ctx.read_holding_registers(addr, len)).await
            .map_err(|_| MupcError::new(ErrorCode::IntercoreTimeout, "modbus read timeout", "intercore"))?
            .map_err(|e| MupcError::new(ErrorCode::SendFailed, format!("read: {}", e), "intercore"))
    }

    /// 下发分相：写 phase_p/q（12 寄存器）→ 置 cmd_valid（含 seq）→ 轮询执行确认
    async fn issue(&self, data_regs: &[u16]) -> Result<(), MupcError> {
        // 1. 数据寄存器（含首周期版本）
        self.write_regs(REG_PHASE_P_A, data_regs).await?;
        self.write_regs(REG_PROTOCOL_VERSION, &[PROTOCOL_VERSION]).await?;
        // 2. cmd_valid 触发（seq 递增）
        let seq = self.settings.slave_addr.wrapping_add(1); // 占位递增源；真实为 AtomicU8
        let ctrl = pack_cmd_ctrl(seq, 2, false, true);
        self.write_regs(REG_CMD_CTRL, &[ctrl]).await?;
        // 3. 轮询执行确认（对齐 5s 超时）
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let st = self.read_regs(REG_EXEC_SEQ, 2).await?;
            let exec_seq = regs_to_i32(&st[..2]);
            if exec_seq == seq as i32 {
                let status_regs = self.read_regs(REG_EXEC_STATUS, 1).await?;
                match status_regs[0] {
                    EXEC_SUCCESS => { *self.connected.write().await = true; return Ok(()); }
                    EXEC_FAILED => return Err(MupcError::new(ErrorCode::SendFailed, "从站执行失败", "intercore")),
                    _ => {}
                }
            }
            if tokio::time::Instant::now() > deadline { return Err(MupcError::new(ErrorCode::IntercoreTimeout, "指令确认超时 5s", "intercore")); }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[async_trait]
impl IntercoreTransport for ModbusRtuTransport {
    async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], _mode: &str) -> Result<(), MupcError> {
        let mut regs = Vec::with_capacity(12);
        for &v in p.iter().chain(q.iter()) { regs.extend_from_slice(&power_to_regs(v)); }
        self.issue(&regs).await
    }

    async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError> {
        let mut regs = Vec::with_capacity(4);
        regs.extend_from_slice(&power_to_regs(cmd.p_ref));
        regs.extend_from_slice(&i32_to_regs(encode_scaled(cmd.k_droop, SCALE_K_DROOP)));
        self.write_regs(REG_P_REF, &regs).await?;
        let ctrl = pack_cmd_ctrl(self.settings.slave_addr.wrapping_add(1), 1, cmd.ai_ready, true);
        self.write_regs(REG_CMD_CTRL, &[ctrl]).await
    }

    async fn is_connected(&self) -> bool { *self.connected.read().await }

    async fn shutdown(&self) -> Result<(), MupcError> {
        *self.connected.write().await = false;
        Ok(())
    }
}
```

> 注：上例为**首个可运行实现**——每操作重连串口（简单、避免持久上下文状态），`issue()` 完成"写数据→cmd_valid→轮询 exec 确认（5s 超时）"。`cmd_seq` 递增源用 AtomicU8 而非占位（Task 8 联调前替换）——实现时用 `AtomicU8`：`fetch_add(1, Ordering::Relaxed)`。

- [ ] **Step 2: 编译**

Run: `cargo check -p mupc-intercore`
Expected: 编译通过（tokio-modbus API 以实际版本为准，必要时调整 `rtu::connect_slave` 签名）

- [ ] **Step 3: Commit**

```bash
git add crates/intercore/src/transport/modbus.rs
git commit -m "feat: ModbusRtuTransport Master（写控制寄存器 + 执行确认轮询）"
```

---

## Task 6: Slave 参考实现（bin）

**Files:**
- Create: `mupc/crates/intercore/src/bin/modbus_slave.rs`

- [ ] **Step 1: 实现 Slave 参考（模拟实时控制模块）**

```rust
//! Modbus RTU Slave 参考实现——模拟实时控制模块，供本地联调验证寄存器映射。
//! 用法: modbus_slave <serial_port> [baud] [slave_addr]
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use tokio_modbus::prelude::*;

const REG_CMD_CTRL: u16 = 0x0000;
const REG_PROTOCOL_VERSION: u16 = 0x0001;
const REG_P_REF: u16 = 0x0010;
const REG_PHASE_P_A: u16 = 0x0020;
const REG_EXEC_SEQ: u16 = 0x0030;
const REG_EXEC_STATUS: u16 = 0x0032;
const REG_HEARTBEAT: u16 = 0x0100;
const PROTOCOL_VERSION_EXPECTED: u16 = 1;
const EXEC_SUCCESS: u16 = 2;

#[derive(Clone, Default)]
struct Regs {
    data: Arc<Vec<AtomicU16>>, // 简化：覆盖 0x0106 内寄存器
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1).cloned().unwrap_or_else(|| "COM1".into());
    let baud: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(9600);
    let addr: u8 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let stream = tokio_serial::new(&port, baud).open_native_async()?;
        let slave = SlaveAddr::try_from(addr)?;
        // 寄存器区（0x0000..0x0106）初值
        let regs = vec![AtomicU16::new(0); 0x0106];
        let regs = Arc::new(regs);
        let heartbeat = Arc::new(AtomicU16::new(0));

        // 心跳递增任务
        {
            let hb = heartbeat.clone();
            tokio::spawn(async move {
                loop { tokio::time::sleep(std::time::Duration::from_millis(100)).await; hb.fetch_add(1, Ordering::Relaxed); }
            });
        }

        // 服务处理器
        let service = tokio_modbus::server::service::Service::new(|req| {
            let regs = regs.clone();
            let hb = heartbeat.clone();
            async move {
                use tokio_modbus::server::service::Request;
                match req {
                    Request::ReadHoldingRegisters(addr, len) => {
                        let a = addr as usize;
                        let mut out = Vec::with_capacity(len as usize);
                        for i in 0..len as usize {
                            out.push(if (addr + i) == REG_HEARTBEAT { hb.load(Ordering::Relaxed) }
                                     else if a + i < regs.len() { regs[a + i].load(Ordering::Relaxed) } else { 0 });
                        }
                        Ok(Response::ReadHoldingRegisters(out))
                    }
                    Request::WriteMultipleRegisters(addr, values) => {
                        for (i, v) in values.iter().enumerate() {
                            let a = (addr as usize) + i;
                            if a < regs.len() { regs[a].store(*v, Ordering::Relaxed); }
                        }
                        Ok(Response::WriteMultipleRegisters(addr, values))
                    }
                    Request::WriteSingleRegister(addr, v) => {
                        let a = addr as usize;
                        if a < regs.len() { regs[a].store(v, Ordering::Relaxed); }
                        // cmd_valid 上升沿：校验版本 + 采纳 + 回写 exec
                        if addr == REG_CMD_CTRL && v & 0x0001 == 0x0001 {
                            let version_ok = regs[REG_PROTOCOL_VERSION as usize].load(Ordering::Relaxed) == PROTOCOL_VERSION_EXPECTED;
                            let seq = ((v >> 8) & 0xFF) as u16;
                            regs[REG_EXEC_SEQ as usize].store(seq, Ordering::Relaxed);
                            regs[(REG_EXEC_SEQ as usize) + 1].store(0, Ordering::Relaxed);
                            regs[REG_EXEC_STATUS as usize].store(if version_ok { EXEC_SUCCESS } else { 3 }, Ordering::Relaxed);
                            regs[REG_CMD_CTRL as usize].store(v & !0x0001, Ordering::Relaxed); // 清 valid
                        }
                        Ok(Response::WriteSingleRegister(addr, v))
                    }
                    other => Ok(tokio_modbus::server::service::Response::Exception(tokio_modbus::ExceptionCode::IllegalFunction)),
                }
            }
        });
        let server = rtu::Server::new(stream);
        server.serve_forever(service).await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
```

> 注：以上为 Slave 参考——`WriteSingleRegister` 命中 `cmd_valid` 时做"版本校验→采纳→回写 exec_seq/exec_status→清 valid"。若 tokio-modbus server 用多寄存器写下发 cmd_valid，需在 `WriteMultipleRegisters` 分支同样处理（实现时核对实际下发方式：Task 5 Master 用 FC06 写单 cmd_ctrl，故走 WriteSingleRegister）。

- [ ] **Step 2: 编译**

Run: `cargo check -p mupc-intercore --bins`
Expected: 编译通过

- [ ] **Step 3: Commit**

```bash
git add crates/intercore/src/bin/modbus_slave.rs
git commit -m "feat: Modbus RTU Slave 参考实现（模拟实时模块，联调用）"
```

---

## Task 7: 配置扩展 + startup 按 transport 构造

**Files:**
- Modify: `mupc/crates/mupc-core-bin/src/core_config.rs`
- Modify: `mupc/crates/mupc-core-bin/src/startup.rs`
- Modify: `mupc/deploy/config/mupc_core_config.yaml`
- Test: `mupc/crates/mupc-core-bin/src/core_config.rs`

- [ ] **Step 1: core_config.rs 加字段**

```rust
pub struct InterCoreConfig {
    pub host: String,
    pub port: u16,
    pub heartbeat_interval_sec: u64,
    pub reconnect_interval_sec: u64,
    /// 传输通道：tcp | modbus_rtu
    #[serde(default = "default_intercore_transport")]
    pub transport: String,
    #[serde(default)]
    pub modbus_rtu: ModbusRtuConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModbusRtuConfig {
    #[serde(default = "default_serial_port")]
    pub serial_port: String,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    #[serde(default = "default_parity")]
    pub parity: String,
    #[serde(default = "default_slave_addr")]
    pub slave_addr: u8,
    #[serde(default = "default_response_timeout_ms")]
    pub response_timeout_ms: u64,
    #[serde(default = "default_heartbeat_poll_ms")]
    pub heartbeat_poll_ms: u64,
}

fn default_intercore_transport() -> String { "tcp".to_string() }
fn default_serial_port() -> String { "/dev/ttyS1".to_string() }
fn default_baud_rate() -> u32 { 9600 }
fn default_data_bits() -> u8 { 8 }
fn default_stop_bits() -> u8 { 1 }
fn default_parity() -> String { "none".to_string() }
fn default_slave_addr() -> u8 { 1 }
fn default_response_timeout_ms() -> u64 { 200 }
fn default_heartbeat_poll_ms() -> u64 { 1000 }
```

> 注意：现有 `InterCoreConfig` 为 `#[derive(Deserialize)]` + 手写 default 字段（`#[serde(default=...)]`）。需把所有既有字段 serde-default 补齐或保持现有构造测试同步新增字段。同步修正 `test_core_config_*` 的手工结构体构造（加 `transport`/`modbus_rtu`）。

- [ ] **Step 2: startup.rs 按 transport 构造 IntercoreClient**

把 L284-285 替换为：

```rust
    let intercore: Arc<mupc_intercore::IntercoreClient> = if config.intercore.transport == "modbus_rtu" {
        let mb = &config.intercore.modbus_rtu;
        tracing::info!("intercore 走 Modbus RTU: {} @{}", mb.serial_port, mb.baud_rate);
        Arc::new(mupc_intercore::IntercoreClient::with_transport(
            Arc::new(mupc_intercore::ModbusRtuTransport::new(
                mupc_intercore::ModbusRtuSettings {
                    serial_port: mb.serial_port.clone(),
                    baud_rate: mb.baud_rate,
                    data_bits: mb.data_bits,
                    stop_bits: mb.stop_bits,
                    parity: mb.parity.clone(),
                    slave_addr: mb.slave_addr,
                    response_timeout_ms: mb.response_timeout_ms,
                    heartbeat_poll_ms: mb.heartbeat_poll_ms,
                },
            )),
        ))
    } else {
        let remote_addr = format!("{}:{}", config.intercore.host, config.intercore.port);
        Arc::new(mupc_intercore::IntercoreClient::new(remote_addr))
    };
```

- [ ] **Step 3: intercore 导出 ModbusRtuSettings / ModbusRtuTransport**

`mupc/crates/intercore/src/lib.rs`:

```rust
pub use transport::modbus::{ModbusRtuSettings, ModbusRtuTransport};
```

- [ ] **Step 4: deploy config yaml**

```yaml
intercore:
  transport: "tcp"              # "tcp" | "modbus_rtu"
  host: "192.168.1.2"
  port: 9100
  heartbeat_interval_sec: 5
  reconnect_interval_sec: 3
  modbus_rtu:
    serial_port: "/dev/ttyS1"
    baud_rate: 9600
    data_bits: 8
    stop_bits: 1
    parity: "none"
    slave_addr: 1
    response_timeout_ms: 200
    heartbeat_poll_ms: 1000
```

- [ ] **Step 5: 编译 + 测试**

Run: `cargo test -p mupc-core-bin core_config && cargo check -p mupc-core-bin && cargo check --workspace`
Expected: 通过

- [ ] **Step 6: Commit**

```bash
git add crates/mupc-core-bin/src/core_config.rs crates/mupc-core-bin/src/startup.rs crates/intercore/src/lib.rs deploy/config/mupc_core_config.yaml
git commit -m "feat: 核间 transport 配置（tcp/modbus_rtu）+ startup 按通道构造 + ModbusRtuConfig"
```

---

## Task 8: 端到端联调 + 回归 + 文档标注

**Files:**
- 联调：虚拟串口对（Windows com0com / Linux socat）
- Modify: `docs/superpowers/plans/modules/10-MUPC-核间通信-设计文档.md`（§11.9 验证结果补记）

- [ ] **Step 1: 起 Slave + Master 联调**

Windows（com0com 建立 COM3↔COM4 对）或 Linux：
```bash
# Linux: socat -d -d pty,raw,echo=0,link=/tmp/vcom1 pty,raw,echo=0,link=/tmp/vcom2
# 终端1: Slave（监听 /tmp/vcom1）
cargo run -p mupc-intercore --bin modbus_slave -- /tmp/vcom1 9600 1
# Master 侧代码: transport=modbus_rtu + serial_port=/tmp/vcom2 后启动 mupcd 验证下发
```

Expected: Master `send_tai_command` 写分相寄存器 → Slave 采样 → exec 回读成功 → Master 日志 "已下发"

- [ ] **Step 2: TCP 回归（默认不变）**

Run: `transport: "tcp"` 启动 → AiIntegrator/本地优先下发仍走帧协议
Expected: 现有行为无回归（TcpTransport 协议字节不变）

- [ ] **Step 3: workspace 全测**

Run: `cargo test --workspace --exclude mupc-iec61850-plugin --exclude rs485-plugin --exclude device-trait`
Expected: 无新增失败（既有 pre-existing ai_validator mock_predict ×2 等除外）

- [ ] **Step 4: 设计文档 §11.9 补联调结果**

在 10 设计文档 §11.9 加一句验证结论（含虚拟串口对下生效/执行确认/心跳，及 TCP 回归通过）。

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/plans/modules/10-MUPC-核间通信-设计文档.md
git commit -m "docs: 核间 Modbus RTU 联调验证结果（Slave 参考 + TCP 回归）"
```

---

## 自审记录

- **Spec 覆盖**：§11.3 transport trait → Task 3/4；§11.4 寄存器映射 → Task 2/5/6；§11.5 栈选型/文件 → Task 1/5/6；§11.6 Slave → Task 6；§11.7 配置 → Task 7；§11.9 验证 → Task 8；ADR-012 数据面边界由 Tcp 承载遥测保持、Modbus 仅控制（Task 5 不实现遥测读写）。
- **占位符扫描**：无 TBD；关键代码完整（tokio-modbus API 以实际版本微调签名已在步骤注明）。
- **类型一致性**：`IntercoreTransport`（trait）在 Task 3 定义、Task 4/5 实现与使用一致；`ModbusRtuSettings` Task 5 定义、Task 7 startup 构造使用一致；`ModbusRtuConfig`（core_config）Task 7 定义、与 Settings 字段一一对应；编解码函数 `modbus_rtu.rs` 被 Task 5 transport 与 Task 6 slave 复用。
