//! ModbusRtuTransport：PCS 真实协议驱动（协议 V1.3，v2.2）
//!
//! 与实时控制模块（PCS）直连：FC06 逐写 4 区保持寄存器（模式字 1000 / 启停 500 /
//! 恒功率 1001-1002 / 分相 1006-1011），FC04 读 3 区输入寄存器（SOC=1010 / 运行状态
//! 1013）。数据 int16 *1kW，寄存器收发高 8/低 8 字节互换（见 [`crate::pcs`]）。
//! 自 v2.2 移除假设表驱动：cmd_ctrl/exec 确认轮询、int32 假设点表（REG_* 0x0000 区）。
//! 假设表编解码保留于 `modbus_rtu.rs`（Task 3 标注废弃）。
//!
//! tokio-modbus 0.13 的异步 RTU 客户端通过 `rtu::attach_slave(stream, Slave)` 构造
//! `client::Context`（无 `connect_slave`/`SlaveAddr`，串口打开由调用方完成）。
//! 其 `tokio_modbus::Result<T>` 为双层 Result：外层为传输/IO 错误、内层为协议异常，
//! 本模块以 [`fold_tm`] 折叠为单一 `MupcError`。
use crate::pcs::*;
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
    /// 3 区 REG_RUN_STATE(1013) 判在线/离线。本模块不自动启动该任务（Modbus 未实联验证），
    /// 由装配方在构造 `Arc<Self>` 后 `tokio::spawn`；取 0 时该任务回退 1000ms。
    pub heartbeat_poll_ms: u64,
}

pub struct ModbusRtuTransport {
    settings: ModbusRtuSettings,
    connected: RwLock<bool>,
    /// 已下发的有功模式字（0=恒功率 / 2=分相，u8 缓存）。初值 0xFF 哨兵：与任何合法
    /// 模式不等，保证首条指令必写 REG_MODE（PCS 上电默认模式未知，不能省首次写）。
    mode: AtomicU8,
    /// 是否已下发运行指令（REG_START_STOP=1）。初值 false：首条指令必写启停。
    started: RwLock<bool>,
    /// 最近一次读到的 BMS SOC（%，含读取时刻；N3 来源，FCP04 读 3 区 1010）
    soc: RwLock<Option<(f64, std::time::Instant)>>,
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

/// 解码 PCS 3 区 SOC 字 → 百分比（0..=100）；非有限或越界返回 None（视为无效读数）
fn decode_soc(word: u16) -> Option<f64> {
    let soc = from_pcs_reg(word);
    if soc.is_finite() && (0.0..=100.0).contains(&soc) {
        Some(soc)
    } else {
        None
    }
}

impl ModbusRtuTransport {
    pub fn new(settings: ModbusRtuSettings) -> Self {
        Self {
            settings,
            connected: RwLock::new(false),
            // 0xFF 哨兵：首条指令强制写模式字（PCS 上电默认模式未知）
            mode: AtomicU8::new(0xFF),
            started: RwLock::new(false),
            soc: RwLock::new(None),
        }
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

    /// FC06 写单寄存器（PCS 逐写：一次一寄存器，无批量写）。成功→在线，失败→离线。
    async fn write_reg(&self, addr: u16, value: u16) -> Result<(), MupcError> {
        let result = self.write_reg_once(addr, value).await;
        if result.is_ok() {
            self.mark_online().await;
        } else {
            self.mark_offline().await;
        }
        result
    }

    /// 写事务本体（不含 connected 状态副作用，供 [`Self::write_reg`] 包装）
    async fn write_reg_once(&self, addr: u16, value: u16) -> Result<(), MupcError> {
        let mut ctx = open_ctx(&self.settings).await?;
        let r = timeout(
            Duration::from_millis(self.settings.response_timeout_ms),
            ctx.write_single_register(addr, value),
        )
        .await
        .map_err(|_| MupcError::new(ErrorCode::IntercoreTimeout, "modbus write timeout", "intercore"))?;
        fold_tm("write_single_register", r)
    }

    /// FC04 读输入寄存器（PCS 3 区 SOC/运行状态）。成功→在线，失败→离线。
    async fn read_input(&self, addr: u16, len: u16) -> Result<Vec<u16>, MupcError> {
        let result = self.read_input_once(addr, len).await;
        if result.is_ok() {
            self.mark_online().await;
        } else {
            self.mark_offline().await;
        }
        result
    }

    /// 读事务本体（不含 connected 状态副作用，供 [`Self::read_input`] 包装）
    async fn read_input_once(&self, addr: u16, len: u16) -> Result<Vec<u16>, MupcError> {
        let mut ctx = open_ctx(&self.settings).await?;
        let r = timeout(
            Duration::from_millis(self.settings.response_timeout_ms),
            ctx.read_input_registers(addr, len),
        )
        .await
        .map_err(|_| MupcError::new(ErrorCode::IntercoreTimeout, "modbus read timeout", "intercore"))?;
        fold_tm("read_input_registers", r)
    }

    /// 确保 PCS 处于指定有功模式：缓存模式与目标一致则跳过写（FC06 幂等、省总线往返）。
    /// 模式字也经 `to_pcs_reg` 字节互换（协议全设备高 8/低 8 互换）；⚠️ 若实机模式/启停
    /// 控制字不互换需在此调整——PCS 契约待确认项。
    async fn ensure_mode(&self, mode: u16) -> Result<(), MupcError> {
        let cur = self.mode.load(Ordering::Relaxed) as u16;
        if cur != mode {
            self.write_reg(REG_MODE, to_pcs_reg(mode as f64)).await?;
            self.mode.store(mode as u8, Ordering::Relaxed);
        }
        Ok(())
    }

    /// 确保 PCS 已运行（REG_START_STOP=1）：已下发过运行则跳过（启停是边沿性设置，
    /// 重复写 1 幂等但省一次总线往返）。下发失败不改缓存，下次调用重试。
    async fn ensure_started(&self) -> Result<(), MupcError> {
        if !*self.started.read().await {
            self.write_reg(REG_START_STOP, to_pcs_reg(1.0)).await?;
            *self.started.write().await = true;
        }
        Ok(())
    }

    /// 心跳探测：离线状态被查询时主动读一次 3 区 REG_RUN_STATE 判定在/离线
    async fn probe_link(&self) -> bool {
        self.read_input(REG_RUN_STATE, 1).await.map(|_| true).unwrap_or(false)
    }

    /// 后台心跳轮询：按 [`ModbusRtuSettings::heartbeat_poll_ms`] 周期读 3 区 REG_RUN_STATE
    /// （FC04）判在线/离线（补偿 is_connected 仅在事务触发时才判线的被动性）。
    ///
    /// 判定规则：读到即在线 `connected=true`、失败计数清零；连续 3 次读取失败 → 离线
    /// `connected=false`。PCS 运行状态字合法值本就不必每拍变化，故不采用假设表时代
    /// "计数停滞判离线"的判据。心跳自身错误静默降级（`tracing::debug`），不 panic；
    /// 从站恢复后下一次成功读数自动回在线。
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
        let mut bad = 0u32;
        const BAD_LIMIT: u32 = 3;
        loop {
            ticker.tick().await;
            // 用 read_input_once（不含状态副作用）：在线/离线由本任务统一判定
            match self.read_input_once(REG_RUN_STATE, 1).await {
                Ok(_) => {
                    bad = 0;
                    self.mark_online().await;
                }
                Err(e) => {
                    bad += 1;
                    if bad >= BAD_LIMIT {
                        self.mark_offline().await;
                        tracing::debug!("modbus run-state read error after {bad} polls (silent): {e}");
                    }
                }
            }
        }
    }
}

#[async_trait]
impl IntercoreTransport for ModbusRtuTransport {
    /// 台区储能分相 P/Q 下发（PCS 交流分相模式）：逐相 FC06 写 P(1006-1008)/Q(1009-1011)。
    /// 首条指令前置写模式字（REG_MODE=2 分相）与启停（REG_START_STOP=1）。
    /// mode 字符串不参与编码：PCS 无"基础/智能/兜底"三态，分相即恒功率曲线由 AiValidator
    /// 校验后下发（上层 ai_integration 传 "fallback" 仅为语义占位，不映射）。
    async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], _mode: &str) -> Result<(), MupcError> {
        self.ensure_mode(MODE_PHASE_SPLIT).await?;
        self.ensure_started().await?;
        for (i, reg) in [REG_PHASE_P_A, REG_PHASE_P_A + 1, REG_PHASE_P_A + 2].iter().enumerate() {
            self.write_reg(*reg, to_pcs_reg(clamp_phase(p[i]))).await?;
        }
        for (i, reg) in [REG_PHASE_Q_A, REG_PHASE_Q_A + 1, REG_PHASE_Q_A + 2].iter().enumerate() {
            self.write_reg(*reg, to_pcs_reg(clamp_phase(q[i]))).await?;
        }
        Ok(())
    }

    /// 双参数下发（PCS 交流恒功率模式）：写 REG_CONST_P_SET=p_ref / REG_CONST_Q_SET=0。
    /// PCS 恒功率无下垂：k_droop 忽略（v2.2 语义偏离，AI 恒功率下发前须经 AiValidator
    /// 范围校验）；cmd.ai_ready/strategy_mode 字段 PCS 点表无对应寄存器，不落盘。
    async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError> {
        self.ensure_mode(MODE_CONST_POWER).await?;
        self.ensure_started().await?;
        self.write_reg(REG_CONST_P_SET, to_pcs_reg(cmd.p_ref)).await?;
        self.write_reg(REG_CONST_Q_SET, to_pcs_reg(0.0)).await?;
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        // connected 由读/写事务成败驱动；离线状态下被查询时主动探测一次
        // REG_RUN_STATE，避免冷启动/断线后仅因尚无写操作而一直误报离线
        if *self.connected.read().await {
            return true;
        }
        self.probe_link().await
    }

    async fn shutdown(&self) -> Result<(), MupcError> {
        // 复位 connected 及模式/启停缓存：下次调用（如重连后）需重新初始化 PCS
        *self.connected.write().await = false;
        *self.started.write().await = false;
        self.mode.store(0xFF, Ordering::Relaxed);
        *self.soc.write().await = None;
        Ok(())
    }

    /// 实时读 3 区 REG_SOC(1010)（FC04）。读成功且 SOC∈[0,100] 则更新缓存并返回
    /// （含读取时刻）；读失败或越界返回 None（保留旧缓存值，不因一次坏读数清空）。
    async fn latest_soc(&self) -> Option<(f64, std::time::Instant)> {
        match self.read_input(REG_SOC, 1).await {
            Ok(r) if !r.is_empty() => {
                let now = std::time::Instant::now();
                if let Some(soc) = decode_soc(r[0]) {
                    *self.soc.write().await = Some((soc, now));
                    Some((soc, now))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> ModbusRtuSettings {
        ModbusRtuSettings {
            serial_port: "/dev/ttyS1".to_string(),
            baud_rate: 9600,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".to_string(),
            slave_addr: 1,
            response_timeout_ms: 200,
            heartbeat_poll_ms: 1000,
        }
    }

    #[test]
    fn test_initial_state() {
        let t = ModbusRtuTransport::new(test_settings());
        // 0xFF 哨兵：首条指令必写模式字；started=false 首条必写启停
        assert_eq!(t.mode.load(Ordering::Relaxed), 0xFF);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert!(!rt.block_on(async { *t.started.read().await }));
        assert!(!rt.block_on(async { *t.connected.read().await }));
        assert!(rt.block_on(async { t.soc.read().await.is_none() }));
    }

    #[test]
    fn test_decode_soc_valid() {
        // 66% → to_pcs_reg(66) 字节互换，回解须还原 66
        assert_eq!(decode_soc(to_pcs_reg(66.0)), Some(66.0));
    }

    #[test]
    fn test_decode_soc_rejects_out_of_range() {
        // 负数与 >100 均视为无效读数
        assert_eq!(decode_soc(to_pcs_reg(-1.0)), None);
        assert_eq!(decode_soc(to_pcs_reg(101.0)), None);
    }
}
