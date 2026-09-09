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
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::{Mutex, RwLock};
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
    /// RS485 半双工总线事务互斥（W3）。锁在**入口**获取（send_tai_command / send_dual_param
    /// 整条"模式+启停+功率"序列、`stop()` 停机写、latest_soc / probe_link / 心跳每拍各一次读），
    /// 保证并发指令不在物理线路上交错（模式切换与寄存器写须原子，避免两条控制序列互插；
    /// interlock stop() 与在途下行序列串行）。
    /// 内部 helper（ensure_*/write_reg/read_input/_once 系）**不**获取本锁，由入口持有；
    /// 取锁入口须成对，防止嵌套死锁。
    bus: Mutex<()>,
    /// 已下发的有功模式字（0=恒功率 / 2=分相，u8 缓存）。初值 0xFF 哨兵：与任何合法
    /// 模式不等，保证首条指令必写 REG_MODE（PCS 上电默认模式未知，不能省首次写）。
    mode: AtomicU8,
    /// 是否已下发运行指令（REG_START_STOP=1）。初值 false：首条指令必写启停。
    started: RwLock<bool>,
    /// 联锁锁存（transport 运行期挡启动兜底；C-1 双 latch 之一，与 storage/DB latch 语义同步）。
    /// **只由 restore_interlock_latched 置/清**；stop()/ensure_started 均不改（stop() 写 500=0 前
    /// 触发沿已 restore(true)，写失败时 latch 已挡启动）。锁存期间 send_* 入口与 ensure_started 拒写。
    stopped_latched: RwLock<bool>,
    /// 人工授权重启位（I-1，**单次**）：`authorize_restart()`（ack_m1/release 后）置 true，放行
    /// ensure_started 的 S-4 停机守卫（RUN_STATE=0 停机稳态下重写 500=1 一次）；由 S-4 消费分支
    /// 或正常启动路径清 false（单次授权、经消费即弃）。仅 `!stopped_latched` 时可授权（latch 须先
    /// release）。TCP 通道无 PCS 500 语义、无此字段。
    restart_authorized: AtomicBool,
    /// 心跳维护的最新 RUN_STATE(1013) 解码值（0..=3；合法读数才更新）；mark_offline 清 None（B3，
    /// 防离线期 DO1/DO2 同亮）。供上层/DO 驱动同步读取。
    /// ⚠️ 用 std RwLock 而非 tokio RwLock：trait `last_run_state()` 为**同步** getter，需在异步
    /// 上下文外安全读取；tokio RwLock 无同步读取（blocking_read 在异步执行上下文内会 panic）。
    last_run_state: StdRwLock<Option<u16>>,
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

/// 解码 PCS 3 区运行状态字 → 0停机/1待机/2充电/3放电；越界返回 None（乱码/错位帧判无效读数）
fn decode_run_state(word: u16) -> Option<u16> {
    let st = from_pcs_reg(word);
    if st.is_finite() && (0.0..=3.0).contains(&st) {
        Some(st as u16)
    } else {
        None
    }
}

impl ModbusRtuTransport {
    pub fn new(settings: ModbusRtuSettings) -> Self {
        Self {
            settings,
            connected: RwLock::new(false),
            bus: Mutex::new(()),
            // 0xFF 哨兵：首条指令强制写模式字（PCS 上电默认模式未知）
            mode: AtomicU8::new(0xFF),
            started: RwLock::new(false),
            stopped_latched: RwLock::new(false),
            restart_authorized: AtomicBool::new(false),
            last_run_state: StdRwLock::new(None),
        }
    }

    /// 测试用：读取人工授权重启位（[`Self::restart_authorized`]）当前值。仅 `cfg(test)` 编译存在；
    /// 无 IO 单测断言 `authorize_restart()` 的置位效果；S-4 消费/写 500=1 分支需总线 IO，
    /// 由 Task8 E2E（pcs_slave 停机语义）覆盖。
    #[cfg(test)]
    pub(crate) async fn is_restart_authorized(&self) -> bool {
        self.restart_authorized.load(Ordering::Relaxed)
    }

    /// 在线标记：任一读/写事务成功即视为链路在线
    async fn mark_online(&self) {
        *self.connected.write().await = true;
    }

    /// 离线复位：任一读/写事务失败（串口打开失败/超时/协议异常）即复位，并**清空
    /// 模式/启停缓存**（W1）——断线期间 PCS 侧可能掉电/复位到默认模式，缓存不再
    /// 可信；下次指令的 ensure_mode/ensure_started 看到 0xFF 哨兵/false 会强制重写
    /// REG_MODE/REG_START_STOP，恢复 PCS 到预期状态。避免 is_connected 恒 true 的
    /// 语义失真与重连后缓存与实机不符。SOC 非控制缓存（每周期实时读、无缓存），不受此影响。
    async fn mark_offline(&self) {
        *self.connected.write().await = false;
        *self.started.write().await = false;
        self.mode.store(0xFF, Ordering::Relaxed);
        // B3：离线清 last_run_state（None → DO1 灭），防离线期 DO1/DO2 同亮矛盾
        *self.last_run_state
            .write()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// FC06 写单寄存器（PCS 逐写：一次一寄存器，无批量写）。成功→在线，失败→离线。
    /// 由持锁入口（send_*/ensure_* 链路，bus 锁已持有）调用，本函数**不**获取 bus 锁。
    async fn write_reg(&self, addr: u16, value: u16) -> Result<(), MupcError> {
        let result = self.write_reg_once(addr, value).await;
        if result.is_ok() {
            self.mark_online().await;
        } else {
            self.mark_offline().await;
        }
        result
    }

    /// 写事务本体（不含 connected 状态副作用，供 [`Self::write_reg`] 包装）。
    /// 错误消息携带寄存器地址：分相 6 寄存器逐写中某地址持续失败时（M9b 诊断），
    /// 日志可定位退化寄存器（如 1011 Q_C 地址错）。
    async fn write_reg_once(&self, addr: u16, value: u16) -> Result<(), MupcError> {
        let mut ctx = open_ctx(&self.settings).await?;
        let r = timeout(
            Duration::from_millis(self.settings.response_timeout_ms),
            ctx.write_single_register(addr, value),
        )
        .await
        .map_err(|_| {
            MupcError::new(ErrorCode::IntercoreTimeout, format!("modbus write timeout reg@{addr}"), "intercore")
        })?;
        fold_tm(&format!("write_single_register@{addr}"), r)
    }

    /// FC04 读输入寄存器（PCS 3 区 SOC/运行状态）。成功→在线，失败→离线。
    /// 由持锁入口（latest_soc/probe_link，bus 锁已持有）调用，本函数**不**获取 bus 锁。
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
        .map_err(|_| {
            MupcError::new(ErrorCode::IntercoreTimeout, format!("modbus read timeout reg@{addr}"), "intercore")
        })?;
        fold_tm(&format!("read_input_registers@{addr}"), r)
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

    /// 确保 PCS 已运行（REG_START_STOP=1）：
    /// ① `stopped_latched` 时直接 Err（联锁禁启，即使上层误发也不重启——底层兜底）；
    /// ② `started==true` 缓存命中 Ok（跳过总线往返）；
    /// ③ 否则 **S-4 前置读**：先 FC04 读 REG_RUN_STATE(1013) 校验 PCS 非停机（M1 守卫——
    /// 堵住「离线窗口内保护跳闸 → 链路恢复自动重启跳闸机」路径）。读到 0（停机）→ 若
    /// `restart_authorized`（人工已经 ack_m1/authorize_restart 确认）→ **消费授权**（清位）并放行
    /// 重写 500=1（I-1 单次旁路：停机稳态也能重启一次）；否则 Err 交上层（运维恢复走
    /// authorize_restart）。非 0（待机/充/放电）或读数无效（乱码按非停机处理）→ 清授权并正常
    /// 写 500=1 置 started=true（正常启动路径同样消费/清授权，防过期授权滞留）。
    /// **I-2**：读后、写 500=1 前再查一次 `stopped_latched`——restore 不取 bus 锁，读与写间若触发
    /// 沿介入置位则放弃启动（窗口有界：复查 read-guard 释放后、500=1 写上位前，restore(true) 仍可
    /// 介入——最坏产生「start 脉冲后 stop() 随 bus 序立即写 500=0」，末态仍停且 latch 挡启，可接受）。
    /// 读写用 *_once 原语（不带 connected 副作用）：链路在线状态由入口级 write_reg/心跳维护。
    ///
    /// ⚠️ 语义挂起（§11.11 待确认）：PCS 停机后 run_state=0 稳态下重启 = 人工授权后 S-4 放行一次；
    /// 500 电平/边沿时序最终以厂方答复为准，本实现按「写 500=1 触发启动」处理。
    async fn ensure_started(&self) -> Result<(), MupcError> {
        if *self.stopped_latched.read().await {
            return Err(MupcError::new(
                ErrorCode::SendFailed,
                "interlock stopped：联锁锁存禁止自动启动",
                "intercore",
            ));
        }
        if *self.started.read().await {
            return Ok(()); // 已下发运行缓存命中，跳过
        }
        // S-4：首个写启动前先读一次 RUN_STATE 校验非停机（M1 守卫升级）
        let words = self.read_input_once(REG_RUN_STATE, 1).await?;
        let run = decode_run_state(words.first().copied().unwrap_or(u16::MAX));
        if run == Some(0) {
            if !self.restart_authorized.load(Ordering::Relaxed) {
                tracing::warn!("M1 前置校验：RUN_STATE=0（停机/保护跳闸），不自动启动，交上层");
                return Err(MupcError::new(
                    ErrorCode::SendFailed,
                    "RUN_STATE=0 不允许自动启动（M1 守卫）",
                    "intercore",
                ));
            }
            // I-1：人工已确认（restart_authorized）→ 在决策点**消费授权**（单次）并放行重写 500=1。
            // PCS 停机后 run_state=0 稳态下重启 = 人工授权后 S-4 放行一次；500 电平/边沿时序以厂方
            // 答复为准（§11.11「启停 500 时序」待确认）。决策点即清位：即使随后写失败/I-2 复查放弃，
            // 授权已消耗（须重授权），杜绝过期授权在下次停机后凭陈旧位自动重启（M1 单次语义）。
            self.restart_authorized.store(false, Ordering::Relaxed);
            tracing::info!("M1 授权重启：RUN_STATE=0 且 restart_authorized，放行重发 500=1（单次，授权已消费）");
        } else {
            // 非 0（待机/充/放电）或读数无效（乱码按非停机处理）：正常启动路径，清授权
            self.restart_authorized.store(false, Ordering::Relaxed);
        }
        // I-2：读后、写 500=1 前再查一次 latch——读与写间触发沿可能已置 stopped_latched（restore 不取
        // bus 锁、可随时置位；stop() 随 bus 序在本序列之后才写 500=0）。此刻置位则放弃启动。
        if *self.stopped_latched.read().await {
            tracing::warn!("S-4 写前复查：联锁 latch 已在此窗口置位，放弃启动");
            return Err(MupcError::new(
                ErrorCode::SendFailed,
                "S-4 写前联锁 latch 置位，放弃启动",
                "intercore",
            ));
        }
        self.write_reg_once(REG_START_STOP, to_pcs_reg(1.0)).await?;
        *self.started.write().await = true;
        Ok(())
    }

    /// 停机写本体（写 REG_START_STOP=0；**不设/清 stopped_latched**——C-1：触发沿已经
    /// restore_interlock_latched(true) 置 latch，本函数失败时 latch 仍挡启动）。成功复位
    /// `started=false`（否则人工 release 后 ensure_started 见 started==true 跳过写 500=1 →
    /// 释放后无法重启，静默失效）并复位 `mode` 0xFF 哨兵（否则 release 后 ensure_mode 见缓存==
    /// 目标跳过 REG_MODE=1000 重写，可能在错误模式下直接写功率）。由持锁入口（trait `stop()`）
    /// 调用，本函数**不**获取 bus 锁。写 500=0 在 latch 期间仍允许（仅挡 500=1 启动写，供
    /// interlock 周期重试停机）。
    async fn do_stop(&self) -> Result<(), MupcError> {
        self.write_reg_once(REG_START_STOP, 0).await?;
        *self.started.write().await = false;
        self.mode.store(0xFF, Ordering::Relaxed);
        Ok(())
    }

    /// 心跳探测：离线状态被查询时主动读一次 3 区 REG_RUN_STATE 判定在/离线。
    /// 单次读事务持有 [`Self::bus`]，不与并发的下行写序列交错（W3）。
    async fn probe_link(&self) -> bool {
        let _bus_guard = self.bus.lock().await;
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
        // M1 停机告警去抖：仅在"非停机→停机"跃迁时告警一次，避免停机期间每秒刷屏
        let mut stopped_warned = false;
        loop {
            ticker.tick().await;
            // 用 read_input_once（不含状态副作用）：在线/离线由本任务统一判定
            // 每拍单次读持有 bus 锁（W3），避免与下行写序列在物理线路上交错
            let _bus_guard = self.bus.lock().await;
            match self.read_input_once(REG_RUN_STATE, 1).await {
                Ok(r) if !r.is_empty() => {
                    // M9a: 运行状态字仅 0..=3 为合法读数；乱码/错位帧按坏读数计，不判在线
                    match decode_run_state(r[0]) {
                        Some(st) => {
                            bad = 0;
                            self.mark_online().await;
                            // 维护 last_run_state：读到合法 0..=3 后更新（DO1 运行灯数据源；
                            // 越界/坏读数按 M9a 判无效、不更新）。std RwLock 写不跨 await，安全。
                            *self.last_run_state
                                .write()
                                .unwrap_or_else(|e| e.into_inner()) = Some(st);
                            // M1 停机观测：PCS 停机（1013=0）而 MUPC 此前已下发运行 →
                            // 远端停机（保护跳闸/人工）。保守策略：**不动 started 缓存、不自动
                            // 重发 500=1**（PCS 启停 500 电平/边沿语义待厂方确认，自动重启可能造成
                            // 保护跳闸-重启振荡）；仅告警一次供上层/运维感知，恢复由上层决策。
                            // PCS 未下发运行即停机属正常冷态，无需告警。
                            // S-1 豁免：stopped_latched（自命令软停/联锁停机）时不告警——避免把
                            // 联锁停机误报成异常跳闸。
                            if st == 0
                                && *self.started.read().await
                                && !*self.stopped_latched.read().await
                            {
                                if !stopped_warned {
                                    stopped_warned = true;
                                    tracing::warn!(
                                        "PCS 运行状态=0(停机)但 MUPC 此前已下发启动——疑似保护跳闸/人工停机；\
                                         链路在线，MUPC 不自动重启，请上层/运维确认后处理"
                                    );
                                }
                            } else {
                                stopped_warned = false;
                            }
                        }
                        None => {
                            bad += 1;
                            tracing::debug!("modbus run-state 值非法({:#06x})，视为坏读数", r[0]);
                        }
                    }
                }
                // 空读数（read_input_once 成功但零字，理论不可达）按坏读数计，不判在线
                Ok(_) => {
                    bad += 1;
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
    ///
    /// ⚠️ 整条序列持有 [`Self::bus`]：模式切换 + 启停 + 6 寄存器写须原子，避免与并发的
    /// 恒功率下发/读事务在物理线路上互插（W3）。内部 ensure_*/write_reg 不取锁。
    async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], _mode: &str) -> Result<(), MupcError> {
        // C-1 下行中止（R-B）：联锁 latch 期间整条下行（模式/启停/功率写）在**任何总线 IO 前**
        // 拒绝——避免 stop_failed（PCS 仍运行）时后续周期按设定继续出力。检查置于 bus 锁前，
        // latch 期间连锁都不取（省去等待在途写序列）。latch 检查不 open 串口（测试可离线验证）。
        if *self.stopped_latched.read().await {
            tracing::warn!("interlock stopped：拒绝下行（含启动/功率写）");
            return Err(MupcError::new(
                ErrorCode::SendFailed,
                "interlock stopped：联锁锁存期间禁止下发",
                "intercore",
            ));
        }
        let _bus_guard = self.bus.lock().await;
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
        // C-1 下行中止（R-B）：同 send_tai_command，latch 期间任何总线 IO 前拒发。
        if *self.stopped_latched.read().await {
            tracing::warn!("interlock stopped：拒绝下行（含启动/功率写）");
            return Err(MupcError::new(
                ErrorCode::SendFailed,
                "interlock stopped：联锁锁存期间禁止下发",
                "intercore",
            ));
        }
        let _bus_guard = self.bus.lock().await;
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
        Ok(())
    }

    /// 实时读 3 区 REG_SOC(1010)（FC04）。读成功且 SOC∈[0,100] 返回（含读取时刻）；
    /// 读失败（链路异常，经 read_input 已置离线）或 SOC 越界/非有限返回 None。
    /// 无 SOC 缓存：每周期实时读，瞬时坏读数由上层（AiIntegrator 5s 新鲜度）判过期。
    async fn latest_soc(&self) -> Option<(f64, std::time::Instant)> {
        let _bus_guard = self.bus.lock().await;
        match self.read_input(REG_SOC, 1).await {
            Ok(r) if !r.is_empty() => decode_soc(r[0]).map(|soc| (soc, std::time::Instant::now())),
            _ => None,
        }
    }

    /// 停机原语：写 REG_START_STOP=0（PCS 停机）。持有 bus 锁与并发下行序列在物理线路上串行
    /// （W3 半双工互斥）；成功后复位 started=false + mode=0xFF（release 后 ensure_mode/ensure_
    /// started 强制重写 REG_MODE/500=1）。**不设/不清 stopped_latched**（C-1：置位由触发沿经
    /// restore_interlock_latched(true) 完成；本函数失败时 latch 仍挡启动，交由 interlock 周期重试）。
    async fn stop(&self) -> Result<(), String> {
        let _bus_guard = self.bus.lock().await;
        if let Err(e) = self.do_stop().await {
            // M-3：中性措辞——不预设调用方/不臆断 latch 必然已置位；只陈述写失败事实与后续动作
            // （停机未确认即处于 stop_failed 态，交由联锁流程按 latch 状态周期重试/升级）。
            tracing::error!(
                "stop 写 REG_START_STOP=0 失败：{}——PCS 停机未确认（stop_failed），交由联锁流程周期重试",
                e
            );
            return Err(e.to_string());
        }
        Ok(())
    }

    async fn is_interlock_stopped(&self) -> bool {
        *self.stopped_latched.read().await
    }

    async fn restore_interlock_latched(&self, latched: bool) -> Result<(), String> {
        // C-1：stopped_latched 仅由此置/清（运行时触发沿 restore(true)；release/启动 DB 读回
        // restore(false)）。纯状态、不写设备——即使 stop() 写失败，latch 已挡启动。
        *self.stopped_latched.write().await = latched;
        // Minor-2：新触发沿（restore(true) = 一次新的安全停机事件）作废任何**未消费**的人工重启
        // 授权——授权后、send 消费前若发生新一次联锁触发+release，不允许凭「触发前」的陈旧授权
        // 在 release 后自动重启（须经 ack_m1 重新授权，人工确认针对的是最新一次事件）。
        if latched {
            self.restart_authorized.store(false, Ordering::Relaxed);
        }
        Ok(())
    }

    fn last_run_state(&self) -> Option<u16> {
        // std RwLock：同步 getter 供上层/DO 驱动在异步上下文外安全读取
        *self.last_run_state
            .read()
            .unwrap_or_else(|e| e.into_inner())
    }

    async fn authorize_restart(&self) -> Result<(), String> {
        // I-1/ack_m1（人工授权重启，**单次**）：!stopped_latched 时复位 started=false **并置
        // restart_authorized=true**——放行下个 send 的 ensure_started 在 RUN_STATE=0 停机稳态下
        // 重发 500=1 一次（S-4 停机守卫旁路；否则停机态 500=0 后 run_state 恒 0——如 pcs_slave
        // 建模——release/ack 后永远无法重启，与设计「人工确认后允许重发 500=1」矛盾）。
        // 授权为单次：ensure_started 的 S-4 消费分支或正常启动路径清位。PCS 停机后 run_state=0
        // 稳态下重启 = 人工授权后 S-4 放行一次；500 电平/边沿时序以厂方答复为准（§11.11 待确认）。
        // stopped_latched 时 Err（须先 release 清 latch）。
        if *self.stopped_latched.read().await {
            return Err("interlock stopped：联锁锁存中，须先 release 才能重启".to_string());
        }
        *self.started.write().await = false;
        self.restart_authorized.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> ModbusRtuSettings {
        ModbusRtuSettings {
            serial_port: "/dev/ttyS0".to_string(), // BECG COM1 ↔ PCS（§12.1）
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
    }

    #[test]
    fn test_decode_run_state() {
        // 0 停机 / 1 待机 / 2 充电 / 3 放电 合法；越界（如乱码/错位帧）判无效
        for st in [0.0, 1.0, 2.0, 3.0] {
            assert_eq!(decode_run_state(to_pcs_reg(st)), Some(st as u16));
        }
        assert_eq!(decode_run_state(to_pcs_reg(-1.0)), None);
        assert_eq!(decode_run_state(to_pcs_reg(4.0)), None);
        // 0xFFFF 原样（未解互换）解码 → -1 → 无效
        assert_eq!(decode_run_state(0xFFFF), None);
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

    #[tokio::test]
    async fn test_latched_rejects_send_without_io() {
        // C-1 双 latch：restore(true) 置 transport latch → 下行入口（任何总线 IO 前）即拒发。
        // 测试不 open 串口——Err 在 latch 检查处返回（断言错误消息含 interlock 以区别于
        // open_ctx 失败），不触碰设备。
        let tr = ModbusRtuTransport::new(test_settings());
        tr.restore_interlock_latched(true).await.unwrap();
        let e = tr.send_tai_command([0.0; 3], [0.0; 3], "fallback").await;
        assert!(e.is_err(), "latch 期间应拒发（不发总线写）");
        assert!(
            e.unwrap_err().to_string().contains("interlock"),
            "应为 latch 拦截（非 open_ctx 失败）"
        );
        assert!(tr.is_interlock_stopped().await);
    }

    #[tokio::test]
    async fn test_restore_clear_toggles() {
        // restore 是 stopped_latched 置/清唯一入口（C-1）：true 挡启 → false 恢复
        let tr = ModbusRtuTransport::new(test_settings());
        assert!(!tr.is_interlock_stopped().await);
        tr.restore_interlock_latched(true).await.unwrap();
        assert!(tr.is_interlock_stopped().await);
        tr.restore_interlock_latched(false).await.unwrap();
        assert!(!tr.is_interlock_stopped().await);
    }

    #[tokio::test]
    async fn test_authorize_restart_gated() {
        // I-1/ack_m1：!stopped_latched 时 authorize 复位 started（下个 send 经 ensure_started
        // 重发 500=1）；stopped_latched 时拒绝（须先 release 清 latch）
        let tr = ModbusRtuTransport::new(test_settings());
        // !latch：复位 started
        *tr.started.write().await = true;
        tr.authorize_restart().await.unwrap();
        assert!(!*tr.started.read().await, "authorize 应复位 started，允许下次 send 重发 500=1");
        // latch：authorize 拒绝且不改 started
        tr.restore_interlock_latched(true).await.unwrap();
        *tr.started.write().await = true;
        assert!(tr.authorize_restart().await.is_err());
        assert!(*tr.started.read().await, "latch 期间 authorize 不得复位 started");
    }

    #[tokio::test]
    async fn test_authorize_restart_grants_single_shot() {
        // I-1 补丁（Task1 质量评审闭环）：authorize_restart 除复位 started 外，**置单次授权位
        // restart_authorized**——放行 ensure_started 在 RUN_STATE=0 停机稳态（500=0 后 run_state
        // 恒 0，如 pcs_slave 建模）下重写 500=1（S-4 守卫旁路，否则 release/ack 后永远无法重启）。
        // 无 IO 单测仅断言授权位即可；ensure_started 的 S-4 消费/写 500=1 分支需总线 IO，
        // 由 Task8 E2E（pcs_slave 停机语义）覆盖并注明。
        let tr = ModbusRtuTransport::new(test_settings());
        assert!(!tr.is_restart_authorized().await, "new() 初值：无授权");
        // !latch authorize → 复位 started + 置单次授权位
        *tr.started.write().await = true;
        tr.authorize_restart().await.unwrap();
        assert!(!*tr.started.read().await, "authorize 应复位 started");
        assert!(
            tr.is_restart_authorized().await,
            "authorize 应置单次授权位（放行 S-4 停机守卫）"
        );
        // 新触发沿 restore(true) 作废**未消费**的授权（Minor-2：新联锁事件须重新 ack，防「触发前」
        // 陈旧授权在 release 后自动重启）——先清 latch 前验证作废发生在置 latch 的同时。
        tr.restore_interlock_latched(true).await.unwrap();
        assert!(
            !tr.is_restart_authorized().await,
            "restore(true)（新触发沿）应作废未消费授权"
        );
        // latch 期间 authorize 拒绝（须先 release；latch 下 S-4 首检即挡启）
        assert!(tr.authorize_restart().await.is_err());
    }

    #[test]
    fn test_mark_offline_resets_caches() {
        // W1：离线复位须清模式/启停缓存（0xFF 哨兵/false），下次指令强制重写
        // REG_MODE/REG_START_STOP，避免缓存与实机不符
        let t = ModbusRtuTransport::new(test_settings());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            // 模拟已缓存运行态（此前成功下发过）
            t.mode.store(MODE_PHASE_SPLIT as u8, Ordering::Relaxed);
            *t.started.write().await = true;
            *t.connected.write().await = true;

            t.mark_offline().await;

            assert_eq!(t.mode.load(Ordering::Relaxed), 0xFF);
            assert!(!*t.started.read().await);
            assert!(!*t.connected.read().await);
        });
    }
}
