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
use std::sync::Arc;
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
    /// 心跳轮询周期：驱动 [`ModbusRtuTransport::run_heartbeat_loop`] 后台任务按此周期读
    /// REG_HEARTBEAT 判在线/离线。本模块不自动启动该任务（Modbus 未实联验证），由装配方
    /// 在构造 `Arc<Self>` 后 `tokio::spawn`；取 0 时该任务回退 1000ms。
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
    // Modbus RTU 单从站有效地址 1..=247（0 为广播，主从应答式控制不适用）。
    // 提前到打开串口之前校验，避免无效地址仍先打开串口、迟至 attach 阶段才报错
    if s.slave_addr == 0 || s.slave_addr > 247 {
        return Err(MupcError::new(
            ErrorCode::ConfigError,
            format!("slave_addr {} 超出有效范围 1..=247", s.slave_addr),
            "intercore",
        ));
    }
    // 线格式参数映射（data_bits/stop_bits/parity 全部实际应用到 builder）
    let data_bits = tokio_serial::DataBits::try_from(s.data_bits).map_err(|_| {
        MupcError::new(
            ErrorCode::ConfigError,
            format!("data_bits {} 无效（支持 5/6/7/8）", s.data_bits),
            "intercore",
        )
    })?;
    let stop_bits = tokio_serial::StopBits::try_from(s.stop_bits).map_err(|_| {
        MupcError::new(
            ErrorCode::ConfigError,
            format!("stop_bits {} 无效（支持 1/2）", s.stop_bits),
            "intercore",
        )
    })?;
    let parity = match s.parity.to_ascii_lowercase().as_str() {
        "none" => tokio_serial::Parity::None,
        "even" => tokio_serial::Parity::Even,
        "odd" => tokio_serial::Parity::Odd,
        _ => {
            return Err(MupcError::new(
                ErrorCode::ConfigError,
                format!("parity {} 无效（none/even/odd）", s.parity),
                "intercore",
            ))
        }
    };
    let stream = SerialStream::open(
        &tokio_serial::new(&s.serial_port, s.baud_rate)
            .data_bits(data_bits)
            .stop_bits(stop_bits)
            .parity(parity),
    )
    .map_err(|e| {
        MupcError::new(ErrorCode::ConnectionFailed, format!("open {}: {}", s.serial_port, e), "intercore")
    })?;
    Ok(rtu::attach_slave(stream, Slave::from(s.slave_addr)))
}

impl ModbusRtuTransport {
    pub fn new(settings: ModbusRtuSettings) -> Self {
        // cmd_seq 从 1 起而非 0：0 与从站 exec_seq 初始"空闲/未采纳"态（0）重合，
        // seq=0 会导致首条指令与从站空闲态无法区分
        Self { settings, connected: RwLock::new(false), cmd_seq: AtomicU8::new(1) }
    }

    /// 在线标记：任一读/写事务成功即视为链路在线
    async fn mark_online(&self) {
        *self.connected.write().await = true;
    }

    /// 离线复位：任一读/写事务失败（串口打开失败/超时/协议异常）即复位，
    /// 避免断线后 is_connected 恒 true 的语义失真
    async fn mark_offline(&self) {
        *self.connected.write().await = false;
    }

    async fn write_regs(&self, addr: u16, regs: &[u16]) -> Result<(), MupcError> {
        let result = self.write_regs_once(addr, regs).await;
        if result.is_ok() {
            self.mark_online().await;
        } else {
            self.mark_offline().await;
        }
        result
    }

    /// 写事务本体（不含 connected 状态更新，供 [`Self::write_regs`] 包装）
    async fn write_regs_once(&self, addr: u16, regs: &[u16]) -> Result<(), MupcError> {
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
        let result = self.read_regs_once(addr, len).await;
        if result.is_ok() {
            self.mark_online().await;
        } else {
            self.mark_offline().await;
        }
        result
    }

    /// 读事务本体（不含 connected 状态更新，供 [`Self::read_regs`] 包装）
    async fn read_regs_once(&self, addr: u16, len: u16) -> Result<Vec<u16>, MupcError> {
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

    /// 心跳探测：离线状态被查询时主动读一次 REG_HEARTBEAT 判定在/离线
    async fn probe_heartbeat(&self) -> bool {
        self.read_regs(REG_HEARTBEAT, 1).await.map(|_| true).unwrap_or(false)
    }

    /// 后台心跳轮询：按 [`ModbusRtuSettings::heartbeat_poll_ms`] 周期读 REG_HEARTBEAT
    /// 计数，据其递增与否判定链路在/离线（补偿 is_connected 仅在事务触发时才判线的被动性）。
    ///
    /// 判定规则：读到且计数较上次变化（首读到视为变化）→ 在线 `connected=true`；
    /// 连续 3 次读取失败或计数停滞（无变化）→ 离线 `connected=false`。心跳自身错误
    /// 静默降级（`tracing::debug`），不 panic；从站恢复后下一次成功且变化的读数自动回在线。
    ///
    /// 由装配方在构造 `Arc<Self>` 后调用 `tokio::spawn(arc.clone().run_heartbeat_loop())`
    /// 启动（本模块不自动 spawn——Modbus 未实联验证，避免无串口环境误跑后台任务）。
    pub async fn run_heartbeat_loop(self: Arc<Self>) {
        // heartbeat_poll_ms=0 回退 1s，避免 tokio::interval 零周期 panic
        let poll_ms = if self.settings.heartbeat_poll_ms == 0 {
            1000
        } else {
            self.settings.heartbeat_poll_ms
        };
        let mut ticker = tokio::time::interval(Duration::from_millis(poll_ms));
        // 上次心跳计数；None=首拍（视为活跃）；坏连续计数达 BAD_LIMIT 判离线
        let mut last: Option<u16> = None;
        let mut bad = 0u32;
        const BAD_LIMIT: u32 = 3;
        loop {
            ticker.tick().await;
            // 用 read_regs_once（不含状态副作用）：在线/离线由本任务统一判定
            match self.read_regs_once(REG_HEARTBEAT, 1).await {
                Ok(v) => {
                    let cur = v[0];
                    let changed = match last {
                        None => true,
                        Some(p) => p != cur,
                    };
                    last = Some(cur);
                    if changed {
                        bad = 0;
                        self.mark_online().await;
                    } else {
                        // 计数停滞：从站不再刷新心跳，连续 N 次判离线
                        bad += 1;
                        if bad >= BAD_LIMIT {
                            self.mark_offline().await;
                            tracing::debug!("modbus heartbeat stale after {bad} polls");
                        }
                    }
                }
                Err(e) => {
                    bad += 1;
                    if bad >= BAD_LIMIT {
                        self.mark_offline().await;
                        tracing::debug!("modbus heartbeat read error (silent): {e}");
                    }
                }
            }
        }
    }

    /// 通用下发：写数据区 + cmd_valid 触发 + 轮询执行确认（对齐 5s 超时）
    ///
    /// connected 语义：读/写事务成功置 online、失败（串口打开/超时/协议异常）复位
    /// offline，exec 确认 5s 超时保守复位；装配方亦可启动 [`Self::run_heartbeat_loop`]
    /// 周期判定在/离线，离线时 is_connected() 还会主动探测一次 REG_HEARTBEAT。
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
        // 轮询 exec 确认：单次读回 REG_EXEC_SEQ..REG_EXEC_STATUS 共 3 寄存器
        // （seq 高/低 + status），消除分两次读 seq/status 的窗口竞态；轮询内瞬时
        // 读错误不中止命令（read_regs 已在该失败路径复位 connected），在 5s deadline
        // 内继续重试，仅 deadline 到仍未确认才判 IntercoreTimeout 并保守复位 offline
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if tokio::time::Instant::now() > deadline {
                self.mark_offline().await;
                return Err(MupcError::new(
                    ErrorCode::IntercoreTimeout,
                    "指令确认超时 5s",
                    "intercore",
                ));
            }
            match self.read_regs(REG_EXEC_SEQ, 3).await {
                Ok(st) => {
                    let exec_seq = regs_to_i32(&st[..2]);
                    if exec_seq == seq as i32 {
                        match st[2] {
                            EXEC_SUCCESS => return Ok(()),
                            EXEC_FAILED => {
                                return Err(MupcError::new(
                                    ErrorCode::SendFailed,
                                    "从站执行失败",
                                    "intercore",
                                ))
                            }
                            // EXEC_IDLE/EXEC_RUNNING/EXEC_TIMEOUT：未采纳或执行中，继续轮询
                            _ => {}
                        }
                    }
                }
                Err(e) => {
                    // 瞬时读错误：本轮跳过、deadline 内继续重试
                    tracing::debug!("exec 确认轮询读失败，deadline 内继续重试: {e}");
                }
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
        // 3-bit strategy_mode 编码（0 基础 / 1 智能 / 2 兜底）为协议基线，最终映射待实时
        // 固件确认（§11.4/§11.10）；台区储能分相下发当前固定 2=兜底(fallback)，上层 _mode
        // 字符串未参与编码，不做实际映射函数（避免过度设计）
        self.issue(REG_PHASE_P_A, &regs, 2, false).await
    }

    async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError> {
        let mut regs = Vec::with_capacity(4);
        regs.extend_from_slice(&power_to_regs(cmd.p_ref));
        regs.extend_from_slice(&i32_to_regs(encode_scaled(cmd.k_droop, SCALE_K_DROOP)));
        // 3-bit strategy_mode 编码（0 基础 / 1 智能 / 2 兜底）为协议基线，最终映射待实时
        // 固件确认（§11.4/§11.10）；双参数下发当前固定 1=智能(intelligent)，cmd.strategy_mode
        // 字符串未参与编码，不做实际映射函数（避免过度设计）
        self.issue(REG_P_REF, &regs, 1, cmd.ai_ready).await
    }

    async fn is_connected(&self) -> bool {
        // connected 由读/写事务成败驱动；离线状态下被查询时主动探测一次
        // REG_HEARTBEAT，避免冷启动/断线后仅因尚无写操作而一直误报离线
        if *self.connected.read().await {
            return true;
        }
        self.probe_heartbeat().await
    }

    async fn shutdown(&self) -> Result<(), MupcError> {
        *self.connected.write().await = false;
        Ok(())
    }
}
