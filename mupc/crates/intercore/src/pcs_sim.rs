//! PCS V1.3 协议 Modbus 从站**仿真库**——模拟两级式 PCS（=实时控制模块）
//!
//! 原 `bin/pcs_slave.rs` 的从站服务与状态沉到本模块（T-L0）：bin 变薄壳（解析参数 →
//! 开串口 → [`serve_rtu`]），进程内 e2e 测试（`transport::modbus::e2e_pcs_sim`）经
//! `#[cfg(test)]` 测试缝以 `tokio::io::DuplexStream` 对接同一服务——**无需任何串口
//! 硬件/驱动**即可回归 PCS 链路。纯仿真：无 unsafe、无文件/网络副作用。
//!
//! 协议语义（与协议 V1.3 / `crate::pcs` 一致）：
//! - 4 区保持寄存器（FC03 读 / FC06 单写 / FC16 多写）：启停 500、有功模式 1000、
//!   恒功率 1001/1002、分相 P/Q 1006-1011；
//! - 3 区输入寄存器（FC04 读）：SOC=1010（固定 66%）、运行状态 1013（按启停+有功
//!   方向推演 0 停/1 待机/2 充电/3 放电，正放负充）、**告警字 1000-1004（可经
//!   [`PcsSimState::set_alarm`] 置位**——R2 边界消除：急停故障位 = 告警1 bit2 可仿真）、
//!   BMS/故障/输出（恒 0）。
//! - ⚠️ 字节互换：收/发均经 `from_pcs_reg`/`to_pcs_reg`（PCS 端序），与生产 Master
//!   （`ModbusRtuTransport` / `crate::pcs`）线格式一致。
//!
//! [`serve_rtu`] 为自实现 RTU 成帧（tokio-modbus 0.13.1 的 `server::rtu::Server`
//! 硬编码 `SerialStream`，无法服务进程内流）：帧提取（地址+PDU+CRC16，CRC 低字节在
//! 前，线序与 tokio-modbus codec 核对一致）、坏帧静默丢弃逐字节重同步、不过滤从站
//! 地址（应答回显请求地址，与 tokio-modbus server 行为一致）；支持 FC03/04/06/16，
//! 其余功能码解码为 `Request::Custom` ⇒ 服务层回 `IllegalFunction` 异常（与原 bin
//! 的 `_ =>` 分支语义一致）。
use crate::pcs::{
    from_pcs_reg, to_pcs_reg, MODE_CONST_POWER, MODE_PHASE_SPLIT, REG_CONST_P_SET, REG_CONST_Q_SET,
    REG_MODE, REG_PHASE_P_A, REG_PHASE_Q_A, REG_RUN_STATE, REG_SOC, REG_START_STOP,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_modbus::prelude::*;
use tokio_modbus::server::Service;

/// 仿真固定 BMS SOC（%）
const SIM_SOC: f64 = 66.0;

/// 3 区 告警字基址（1000-1004 共 5 字；告警1=1000，bit2=急停，协议 p6 序号 3）
pub const REG_ALARM_BASE: u16 = 1000;
/// 3 区 告警字末址（1004）
const REG_ALARM_LAST: u16 = REG_ALARM_BASE + ALARM_WORD_COUNT as u16 - 1;
/// 告警字数量（5）
pub const ALARM_WORD_COUNT: usize = 5;

/// Modbus RTU 最大帧长（规范 V1.02 p13：256 字节）
const RTU_MAX_FRAME: usize = 256;

/// 从站仿真状态：4 区保持寄存器真实值镜像（模式/启停为原值、功率为 *1kW 有符号）
/// + 3 区告警字（R2 消除：可置位，急停故障位可仿真）。bin 与 e2e 测试共享。
pub struct PcsSimState {
    /// 4 区保持寄存器镜像：addr → 真实值（已经 `from_pcs_reg` 回解字节互换）
    pub hold: Mutex<HashMap<u16, f64>>,
    /// 3 区 1000-1004 五个告警字（原始位域，上线经字内字节互换）。`AtomicU16`：
    /// `set_alarm` 可在服务运行期从测试/上层线程无锁置位。
    pub alarms: [AtomicU16; ALARM_WORD_COUNT],
}

impl Default for PcsSimState {
    fn default() -> Self {
        Self::new()
    }
}

impl PcsSimState {
    pub fn new() -> Self {
        Self {
            hold: Mutex::new(HashMap::new()),
            alarms: std::array::from_fn(|_| AtomicU16::new(0)),
        }
    }

    /// 置位告警字（**OR 累积**）：`word_idx` 0..5 ↔ 3 区 1000..1004（0=告警1）。
    /// 例：`set_alarm(0, 1 << 2)` = 告警1 bit2 急停（协议 p6 序号 3）。越界下标静默忽略。
    pub fn set_alarm(&self, word_idx: usize, bits: u16) {
        if let Some(a) = self.alarms.get(word_idx) {
            a.fetch_or(bits, Ordering::Relaxed);
        }
    }

    /// 3 区告警字 addr(1000..=1004) → 线值。⚠️ 直接 `swap_bytes` 而非 `to_pcs_reg(f64)`：
    /// 后者经 i16 会饱和截断 u16 高半区位域（bit15）；字内字节互换语义与 `to_pcs_reg`
    /// 在 i16 值域内完全一致（协议全设备高 8/低 8 互换）。
    fn alarm_wire(&self, addr: u16) -> u16 {
        match self.alarms.get(usize::from(addr - REG_ALARM_BASE)) {
            Some(a) => a.load(Ordering::Relaxed).swap_bytes(),
            None => 0,
        }
    }

    /// 按启停 + 有功方向推演 3 区运行状态：正放负充，P>0 放电(3)/P<0 充电(2)/P=0 待机(1)
    pub fn run_state(map: &HashMap<u16, f64>) -> f64 {
        if map.get(&REG_START_STOP).copied().unwrap_or(0.0) as u16 != 1 {
            return 0.0; // 停机
        }
        let mode = map
            .get(&REG_MODE)
            .copied()
            .unwrap_or(MODE_CONST_POWER as f64) as u16;
        let p_total: f64 = if mode == MODE_PHASE_SPLIT {
            (0..3)
                .map(|i| map.get(&(REG_PHASE_P_A + i)).copied().unwrap_or(0.0))
                .sum()
        } else {
            map.get(&REG_CONST_P_SET).copied().unwrap_or(0.0)
        };
        if p_total > 0.0 {
            3.0
        } else if p_total < 0.0 {
            2.0
        } else {
            1.0
        }
    }

    /// 写 4 区保持：线上字 → PCS 真实值（字节互换回解）后落镜像并打印诊断
    fn write_hold(&self, addr: u16, raw: u16) {
        let real = from_pcs_reg(raw);
        self.hold
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(addr, real);
        let name = if addr == REG_START_STOP {
            "启停 500".to_string()
        } else if addr == REG_MODE {
            "模式 1000".to_string()
        } else if addr == REG_CONST_P_SET {
            "恒功率P 1001".to_string()
        } else if addr == REG_CONST_Q_SET {
            "恒功率Q 1002".to_string()
        } else if (REG_PHASE_P_A..=REG_PHASE_P_A + 2).contains(&addr) {
            format!(
                "相{} 有功 {addr}",
                ((addr - REG_PHASE_P_A) as u8 + b'A') as char
            )
        } else if (REG_PHASE_Q_A..=REG_PHASE_Q_A + 2).contains(&addr) {
            format!(
                "相{} 无功 {addr}",
                ((addr - REG_PHASE_Q_A) as u8 + b'A') as char
            )
        } else {
            format!("保持 {addr}")
        };
        println!("[pcs-slave] 写 {name} = {real} (raw={raw:#06x})");
    }
}

/// PCS 从站服务（语义与原 `bin/pcs_slave.rs` 一致；3 区告警字 1000-1004 改读
/// [`PcsSimState::alarms`]，其余告警/BMS/故障/输出仍恒 0）
pub struct PcsSlaveService {
    /// 共享仿真状态（bin 单实例；e2e harness 与断言方共享同一 Arc）
    pub state: Arc<PcsSimState>,
}

impl PcsSlaveService {
    pub fn new(state: Arc<PcsSimState>) -> Self {
        Self { state }
    }
}

impl Service for PcsSlaveService {
    type Request = Request<'static>;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Exception>> + Send>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            match req {
                Request::ReadHoldingRegisters(addr, cnt) => {
                    let map = state.hold.lock().unwrap_or_else(|e| e.into_inner());
                    let out: Vec<u16> = (0..cnt)
                        .map(|i| to_pcs_reg(map.get(&addr.wrapping_add(i)).copied().unwrap_or(0.0)))
                        .collect();
                    Ok(Response::ReadHoldingRegisters(out))
                }
                Request::ReadInputRegisters(addr, cnt) => {
                    let map = state.hold.lock().unwrap_or_else(|e| e.into_inner());
                    let out: Vec<u16> = (0..cnt)
                        .map(|i| match addr.wrapping_add(i) {
                            REG_SOC => to_pcs_reg(SIM_SOC),
                            REG_RUN_STATE => to_pcs_reg(PcsSimState::run_state(&map)),
                            // 告警 1000-1004：读可置位告警字（R2 消除，急停位可仿真）
                            r @ REG_ALARM_BASE..=REG_ALARM_LAST => state.alarm_wire(r),
                            // BMS 1005 / 故障 1014 / 输出 1029-1036：恒 0
                            _ => to_pcs_reg(0.0),
                        })
                        .collect();
                    Ok(Response::ReadInputRegisters(out))
                }
                Request::WriteSingleRegister(addr, value) => {
                    state.write_hold(addr, value);
                    Ok(Response::WriteSingleRegister(addr, value))
                }
                Request::WriteMultipleRegisters(addr, words) => {
                    for (i, w) in words.iter().copied().enumerate() {
                        state.write_hold(addr.wrapping_add(i as u16), w);
                    }
                    Ok(Response::WriteMultipleRegisters(addr, words.len() as u16))
                }
                _ => Err(Exception::IllegalFunction),
            }
        })
    }
}

/// 对任意字节流 serve Modbus RTU 从站协议（服务到对端 EOF 为止）。bin（串口）与
/// 进程内 e2e（DuplexStream）共用。成帧/CRC/异常语义见模块头注释。
pub async fn serve_rtu<S>(stream: S, svc: PcsSlaveService) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut rd, mut wr) = tokio::io::split(stream);
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; RTU_MAX_FRAME];
    loop {
        // 先排干缓冲区内所有完整帧（一次 read 可能带多帧），再等下一段字节
        while let Some(frame) = take_rtu_frame(&mut buf) {
            let rsp = handle_frame(&svc, &frame).await;
            wr.write_all(&rsp).await?;
        }
        match rd.read(&mut tmp).await {
            Ok(0) => return Ok(()), // EOF：对端半关闭（e2e 每事务独立开合流；串口拔线）
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(e) => return Err(e),
        }
    }
}

/// CRC16-MODBUS（初值 0xFFFF、反射多项式 0xA001）。结果按 Modbus RTU 约定
/// **低字节在前**附于帧尾（与 tokio-modbus codec 线序核对一致）。
fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for b in data {
        crc ^= u16::from(*b);
        for _ in 0..8 {
            let odd = (crc & 0x0001) != 0;
            crc >>= 1;
            if odd {
                crc ^= 0xA001;
            }
        }
    }
    crc
}

/// 按功能码推断请求帧总长（含地址与 CRC）。FC16(0x10) 为变长帧，长度由第 7 字节
/// byte_count 决定（9 + byte_count），不足 7 字节时返回 None（还需继续读）；
/// FC03/04/06 定长 8 字节；其余功能码按定长 8 处理（解为 `Request::Custom` ⇒
/// 服务层回 `IllegalFunction`，本仿真不赌未知变长帧）。
fn request_frame_len(buf: &[u8]) -> Option<usize> {
    match buf[1] {
        0x10 => {
            if buf.len() >= 7 {
                Some(9 + usize::from(buf[6]))
            } else {
                None
            }
        }
        _ => Some(8),
    }
}

/// 从缓冲区提取一条完整且 CRC 正确的请求帧。返回 None = 字节不足需继续读；
/// CRC 校验失败按 RTU 语义静默丢弃——丢首字节逐字节重同步（与 tokio-modbus
/// `FrameDecoder::recover_on_error` 策略一致）。
fn take_rtu_frame(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        if buf.len() < 2 {
            return None;
        }
        // FC16 长度未定（byte_count 未到齐）→ 等待更多字节
        let need = request_frame_len(buf)?;
        if need > RTU_MAX_FRAME {
            // 超长"帧"必为失步垃圾：丢首字节重同步（规范上限 256）
            buf.remove(0);
            continue;
        }
        if buf.len() < need {
            return None;
        }
        let crc = crc16_modbus(&buf[..need - 2]);
        if buf[need - 2] == (crc & 0xFF) as u8 && buf[need - 1] == ((crc >> 8) & 0xFF) as u8 {
            return Some(buf.drain(..need).collect());
        }
        buf.remove(0);
    }
}

/// 处理一条已校验帧：解析 → 派发服务 → 编码应答 ADU（回显从站地址）
async fn handle_frame(svc: &PcsSlaveService, frame: &[u8]) -> Vec<u8> {
    // frame 长度 ≥8（take_rtu_frame 保证）：[slave][fc][data..][crc_lo][crc_hi]
    let slave = frame[0];
    let fc = frame[1];
    let data = &frame[2..frame.len() - 2];
    let rsp = svc.call(parse_request(fc, data)).await;
    encode_adu(slave, fc, rsp)
}

/// 请求 PDU 数据段（功能码后、CRC 前）→ `Request`。仅支持 FC03/04/06/16；
/// 其余（含长度不符的畸形帧）解为 `Custom` ⇒ 服务层 `IllegalFunction`。
fn parse_request(fc: u8, data: &[u8]) -> Request<'static> {
    let be16 = |i: usize| u16::from_be_bytes([data[i], data[i + 1]]);
    match fc {
        0x03 if data.len() >= 4 => Request::ReadHoldingRegisters(be16(0), be16(2)),
        0x04 if data.len() >= 4 => Request::ReadInputRegisters(be16(0), be16(2)),
        0x06 if data.len() >= 4 => Request::WriteSingleRegister(be16(0), be16(2)),
        0x10 if data.len() >= 5 => {
            let addr = be16(0);
            let byte_count = usize::from(data[4]);
            // 数据段被 byte_count 截断的畸形帧：只取实际存在的整字（filter 防越界 panic）
            let words: Vec<u16> = (0..byte_count / 2)
                .filter(|i| data.len() >= 5 + 2 * (i + 1))
                .map(|i| u16::from_be_bytes([data[5 + 2 * i], data[6 + 2 * i]]))
                .collect();
            Request::WriteMultipleRegisters(addr, Cow::Owned(words))
        }
        other => Request::Custom(other, Cow::Owned(data.to_vec())),
    }
}

/// 服务结果 → 完整应答 ADU（[slave][pdu][crc_lo][crc_hi]）。服务层仅产生
/// 读字/写单/写多应答与 `IllegalFunction`；其余 Response 变体理论不可达，
/// 兜底编码为 ServerDeviceFailure 异常（不 panic）。
fn encode_adu(slave: u8, req_fc: u8, rsp: Result<Response, Exception>) -> Vec<u8> {
    let mut pdu: Vec<u8> = Vec::new();
    match rsp {
        Ok(Response::ReadHoldingRegisters(words)) => encode_read_registers(&mut pdu, 0x03, &words),
        Ok(Response::ReadInputRegisters(words)) => encode_read_registers(&mut pdu, 0x04, &words),
        Ok(Response::WriteSingleRegister(addr, word)) => {
            pdu.push(0x06);
            pdu.extend_from_slice(&addr.to_be_bytes());
            pdu.extend_from_slice(&word.to_be_bytes());
        }
        Ok(Response::WriteMultipleRegisters(addr, qty)) => {
            pdu.push(0x10);
            pdu.extend_from_slice(&addr.to_be_bytes());
            pdu.extend_from_slice(&qty.to_be_bytes());
        }
        Err(e) => {
            pdu.push(req_fc | 0x80);
            pdu.push(u8::from(e));
        }
        Ok(_) => {
            pdu.push(req_fc | 0x80);
            pdu.push(u8::from(Exception::ServerDeviceFailure));
        }
    }
    let mut adu = Vec::with_capacity(pdu.len() + 3);
    adu.push(slave);
    adu.extend_from_slice(&pdu);
    adu.extend_from_slice(&crc16_modbus(&adu).to_le_bytes());
    adu
}

/// 读寄存器应答 PDU：[fc][byte_count][数据大端字...]。Modbus 限单次读 ≤125 寄存器
/// （byte_count ≤250），`as u8` 不溢出。
fn encode_read_registers(pdu: &mut Vec<u8>, fc: u8, words: &[u16]) {
    pdu.push(fc);
    pdu.push((words.len() * 2) as u8);
    for w in words {
        pdu.extend_from_slice(&w.to_be_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// S2 Task8 停机语义（纯函数、无 IO）：intercore 停机确认依赖心跳读 1013（run_state）转 0。
    /// - 500=1 且无功率设定 → P_total=0 → 待机(1)（非停机，可被心跳判在线）
    /// - 500=0 → run_state 必须回 0（PCS 已停，停机确认窗口等待 1013→0）
    /// - 未写 500（缺省）→ 视为停机
    #[test]
    fn run_state_shutdown_semantics() {
        let mut map = HashMap::new();
        map.insert(REG_START_STOP, 1.0);
        assert_eq!(
            PcsSimState::run_state(&map),
            1.0,
            "500=1 且 P=0 应为待机(1)"
        );
        map.insert(REG_START_STOP, 0.0);
        assert_eq!(
            PcsSimState::run_state(&map),
            0.0,
            "500=0 应停机(run_state=0)"
        );
        let empty = HashMap::new();
        assert_eq!(
            PcsSimState::run_state(&empty),
            0.0,
            "未写 500 应视为停机(run_state=0)"
        );
    }

    /// 有功方向推演：500=1 时 P>0 放电(3)、P<0 充电(2)；且停机优先级最高——
    /// 即便带功率设定，500=0 仍强制回 0（不许带功率待机被误判在线）
    #[test]
    fn run_state_direction_and_stop_priority() {
        let mut map = HashMap::new();
        map.insert(REG_START_STOP, 1.0);
        map.insert(REG_MODE, MODE_CONST_POWER as f64);
        map.insert(REG_CONST_P_SET, 30.0);
        assert_eq!(PcsSimState::run_state(&map), 3.0, "P>0 应为放电(3)");
        map.insert(REG_CONST_P_SET, -30.0);
        assert_eq!(PcsSimState::run_state(&map), 2.0, "P<0 应为充电(2)");
        // 停机优先：带 30kW 放电设定仍须回 0
        map.insert(REG_START_STOP, 0.0);
        assert_eq!(
            PcsSimState::run_state(&map),
            0.0,
            "500=0 优先级最高，带功率设定也须停机"
        );
    }

    /// 告警字置位与线格式：set_alarm OR 累积；线值 = 字内字节互换（bit2 ⇔ 线上 bit10）
    #[test]
    fn alarm_set_and_wire_swap() {
        let st = PcsSimState::new();
        st.set_alarm(0, 1 << 2); // 告警1 bit2 急停
        st.set_alarm(0, 1 << 5); // OR 累积
        assert_eq!(
            st.alarm_wire(REG_ALARM_BASE),
            (1u16 << 2 | 1 << 5).swap_bytes()
        );
        assert_eq!(st.alarm_wire(REG_ALARM_BASE + 1), 0, "未置位告警字为 0");
        st.set_alarm(99, 0xFFFF); // 越界下标静默忽略（不 panic）
                                  // u16 高半区位域不丢失（to_pcs_reg(f64) 会饱和，swap_bytes 直通）
        st.set_alarm(1, 1 << 15);
        assert_eq!(st.alarm_wire(REG_ALARM_BASE + 1), (1u16 << 15).swap_bytes());
    }

    #[test]
    fn crc16_known_vector() {
        // CRC16/MODBUS 标准校验向量："123456789" ⇒ 0x4B37（线上低字节 0x37 在前）
        assert_eq!(crc16_modbus(b"123456789"), 0x4B37);
    }

    /// 帧提取：垃圾前缀经 CRC 失败逐字节重同步后仍能取出合法帧
    #[test]
    fn take_frame_resyncs_over_garbage() {
        // 合法 FC04 读 1013×1 请求帧：01 04 03 F5 00 01 + CRC
        let mut frame = vec![0x01, 0x04, 0x03, 0xF5, 0x00, 0x01];
        let crc = crc16_modbus(&frame);
        frame.extend_from_slice(&crc.to_le_bytes());
        let mut buf = vec![0xAA, 0xBB]; // 垃圾前缀
        buf.extend_from_slice(&frame);
        let got = take_rtu_frame(&mut buf).expect("重同步后应取出合法帧");
        assert_eq!(got, frame);
        assert!(buf.is_empty());
    }

    /// FC16 变长帧：byte_count 未到齐前不出帧；到齐后整帧提取
    #[test]
    fn take_frame_fc16_variable_len() {
        // 01 10 03E9(1001) 0002(2字) 04(字节数) 0005 000A + CRC
        let mut req = vec![
            0x01, 0x10, 0x03, 0xE9, 0x00, 0x02, 0x04, 0x00, 0x05, 0x00, 0x0A,
        ];
        let crc = crc16_modbus(&req);
        req.extend_from_slice(&crc.to_le_bytes());
        let mut head = req[..6].to_vec(); // 尚不知 byte_count
        assert!(
            take_rtu_frame(&mut head).is_none(),
            "长度未定应等待更多字节"
        );
        let mut buf = req.clone();
        assert_eq!(take_rtu_frame(&mut buf), Some(req));
    }

    /// 请求解析：FC03/04/06/16 映射对应 Request；未知功能码 → Custom（服务层回异常）
    #[test]
    fn parse_request_maps_supported_fcs() {
        assert_eq!(
            parse_request(0x04, &[0x03, 0xF5, 0x00, 0x01]),
            Request::ReadInputRegisters(1013, 1)
        );
        assert_eq!(
            parse_request(0x03, &[0x03, 0xE9, 0x00, 0x02]),
            Request::ReadHoldingRegisters(1001, 2)
        );
        assert_eq!(
            parse_request(0x06, &[0x01, 0xF4, 0x00, 0x01]),
            Request::WriteSingleRegister(500, 1)
        );
        assert_eq!(
            parse_request(
                0x10,
                &[0x03, 0xE9, 0x00, 0x02, 0x04, 0x00, 0x05, 0xFF, 0xF6]
            ),
            Request::WriteMultipleRegisters(1001, Cow::Owned(vec![5, 0xFFF6]))
        );
        assert_eq!(
            parse_request(0x07, &[]),
            Request::Custom(0x07, Cow::Owned(vec![]))
        );
    }
}
