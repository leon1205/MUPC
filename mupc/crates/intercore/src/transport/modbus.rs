//! ModbusRtuTransport：经 Modbus RTU 写控制寄存器（FC16）+ cmd_valid 触发 + 执行确认轮询
//!
//! tokio-modbus 0.13 的异步 RTU 客户端通过 `rtu::attach_slave(stream, Slave)` 构造
//! `client::Context`（无 `connect_slave`/`SlaveAddr`，串口打开由调用方完成）。
//! 其 `tokio_modbus::Result<T>` 为双层 Result：外层为传输/IO 错误、内层为协议异常，
//! 本模块以 [`fold_tm`] 折叠为单一 `MupcError`。
use crate::modbus_rtu::*;
use crate::tcp_server::DualParamCommand;
use crate::transport::IntercoreTransport;
use async_trait::async_trait;
use mupc_common::{ErrorCode, MupcError};
use std::sync::atomic::{AtomicU8, Ordering};
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};
use tokio_modbus::client::Context;
use tokio_modbus::prelude::*;
use tokio_serial::SerialStream;

/// Modbus RTU 传输配置（对应 core_config ModbusRtuConfig，Task 7 填充）
#[derive(Debug, Clone)]
pub struct ModbusRtuSettings {
    pub serial_port: String,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub parity: String, // none/even/odd
    pub slave_addr: u8,
    pub response_timeout_ms: u64,
    pub heartbeat_poll_ms: u64,
}

pub struct ModbusRtuTransport {
    settings: ModbusRtuSettings,
    connected: RwLock<bool>,
    cmd_seq: AtomicU8,
}

/// 折叠 tokio-modbus 双层 Result（外层传输/IO 错误 + 内层协议异常）→ `Result<_, MupcError>`
fn fold_tm<T>(tag: &str, r: tokio_modbus::Result<T>) -> Result<T, MupcError> {
    let inner = r
        .map_err(|e| MupcError::new(ErrorCode::SendFailed, format!("{tag} transport: {e}"), "intercore"))?;
    inner.map_err(|e| MupcError::new(ErrorCode::SendFailed, format!("{tag} exception: {e}"), "intercore"))
}

/// 打开串口并 attach 到指定从站（每次请求独立连接，天然规避半双工总线残留帧）
async fn open_ctx(s: &ModbusRtuSettings) -> Result<Context, MupcError> {
    let stream = SerialStream::open(&tokio_serial::new(&s.serial_port, s.baud_rate)).map_err(|e| {
        MupcError::new(ErrorCode::ConnectionFailed, format!("open {}: {}", s.serial_port, e), "intercore")
    })?;
    // Modbus RTU 单从站有效地址 1..=247（0 为广播，主从应答式控制不适用）
    if s.slave_addr == 0 || s.slave_addr > 247 {
        return Err(MupcError::new(
            ErrorCode::ConfigError,
            format!("slave_addr {} 超出有效范围 1..=247", s.slave_addr),
            "intercore",
        ));
    }
    Ok(rtu::attach_slave(stream, Slave::from(s.slave_addr)))
}

impl ModbusRtuTransport {
    pub fn new(settings: ModbusRtuSettings) -> Self {
        Self { settings, connected: RwLock::new(false), cmd_seq: AtomicU8::new(0) }
    }

    async fn write_regs(&self, addr: u16, regs: &[u16]) -> Result<(), MupcError> {
        let mut ctx = open_ctx(&self.settings).await?;
        let r = timeout(
            Duration::from_millis(self.settings.response_timeout_ms),
            ctx.write_multiple_registers(addr, regs),
        )
        .await
        .map_err(|_| MupcError::new(ErrorCode::IntercoreTimeout, "modbus write timeout", "intercore"))?;
        fold_tm("write_multiple_registers", r)
    }

    async fn read_regs(&self, addr: u16, len: u16) -> Result<Vec<u16>, MupcError> {
        let mut ctx = open_ctx(&self.settings).await?;
        let r = timeout(
            Duration::from_millis(self.settings.response_timeout_ms),
            ctx.read_holding_registers(addr, len),
        )
        .await
        .map_err(|_| MupcError::new(ErrorCode::IntercoreTimeout, "modbus read timeout", "intercore"))?;
        fold_tm("read_holding_registers", r)
    }

    fn next_seq(&self) -> u8 {
        self.cmd_seq.fetch_add(1, Ordering::Relaxed)
    }

    /// 通用下发：写数据区 + cmd_valid 触发 + 轮询执行确认（对齐 5s 超时）
    async fn issue(
        &self,
        data_addr: u16,
        data_regs: &[u16],
        strategy_mode: u8,
        ai_ready: bool,
    ) -> Result<(), MupcError> {
        let seq = self.next_seq();
        self.write_regs(REG_PROTOCOL_VERSION, &[PROTOCOL_VERSION]).await?; // 版本（每次带，幂等）
        self.write_regs(data_addr, data_regs).await?;
        let ctrl = pack_cmd_ctrl(seq, strategy_mode, ai_ready, true);
        self.write_regs(REG_CMD_CTRL, &[ctrl]).await?;
        // 轮询 exec 确认
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let st = self.read_regs(REG_EXEC_SEQ, 2).await?;
            let exec_seq = regs_to_i32(&st[..2]);
            if exec_seq == seq as i32 {
                let status = self.read_regs(REG_EXEC_STATUS, 1).await?[0];
                match status {
                    EXEC_SUCCESS => {
                        *self.connected.write().await = true;
                        return Ok(());
                    }
                    EXEC_FAILED => {
                        return Err(MupcError::new(ErrorCode::SendFailed, "从站执行失败", "intercore"))
                    }
                    _ => {}
                }
            }
            if tokio::time::Instant::now() > deadline {
                return Err(MupcError::new(ErrorCode::IntercoreTimeout, "指令确认超时 5s", "intercore"));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[async_trait]
impl IntercoreTransport for ModbusRtuTransport {
    async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], _mode: &str) -> Result<(), MupcError> {
        let mut regs = Vec::with_capacity(12);
        for &v in p.iter().chain(q.iter()) {
            regs.extend_from_slice(&power_to_regs(v));
        }
        self.issue(REG_PHASE_P_A, &regs, 2, false).await
    }

    async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError> {
        let mut regs = Vec::with_capacity(4);
        regs.extend_from_slice(&power_to_regs(cmd.p_ref));
        regs.extend_from_slice(&i32_to_regs(encode_scaled(cmd.k_droop, SCALE_K_DROOP)));
        self.issue(REG_P_REF, &regs, 1, cmd.ai_ready).await
    }

    async fn is_connected(&self) -> bool {
        *self.connected.read().await
    }

    async fn shutdown(&self) -> Result<(), MupcError> {
        *self.connected.write().await = false;
        Ok(())
    }
}
