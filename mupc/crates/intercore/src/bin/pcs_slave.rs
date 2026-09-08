//! PCS V1.3 协议 Modbus 从站仿真——模拟两级式 PCS（=实时控制模块）
//!
//! 以 tokio-modbus **server**（从站）绑定串口，实现协议 V1.3 语义：
//! - 4 区保持寄存器（FC03 读 / FC06 单写 / FC16 多写）：启停 500、有功模式 1000、
//!   恒功率 1001/1002、分相 P/Q 1006-1011；
//! - 3 区输入寄存器（FC04 读）：SOC=1010（固定 66%）、运行状态 1013（按启停+有功
//!   方向推演 0 停/1 待机/2 充电/3 放电，正放负充）、告警/BMS/故障/输出（恒 0）。
//!
//! ⚠️ 字节互换：收/发均经 `from_pcs_reg`/`to_pcs_reg`（PCS 端序），与生产 Master
//! （`ModbusRtuTransport` / crate::pcs）线格式一致——可软件验证寄存器映射、字节互换、
//! 模式/启停写序列、clamp ±25 与心跳在线判定（读 1013）。
//!
//! 用途：无 PCS 实机时经虚拟串口对（Linux socat / Windows com0com）与
//! `ModbusRtuTransport`（transport=modbus_rtu）做软件端到端联调（§11.11 测试缺口）。
//!
//! 用法：`pcs_slave <serial_port> [baud] [slave_addr]`
//! 例：`cargo run -p mupc-intercore --bin pcs_slave -- /dev/ttyS0 19200 1`
use mupc_intercore::pcs::{
    from_pcs_reg, to_pcs_reg, MODE_CONST_POWER, MODE_PHASE_SPLIT, REG_CONST_P_SET, REG_CONST_Q_SET,
    REG_MODE, REG_PHASE_P_A, REG_PHASE_Q_A, REG_RUN_STATE, REG_SOC, REG_START_STOP,
};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio_modbus::prelude::*;
use tokio_modbus::server::Service;
use tokio_serial::SerialStream;

/// 仿真固定 BMS SOC（%）
const SIM_SOC: f64 = 66.0;

/// 从站状态：4 区保持寄存器真实值镜像（模式/启停为原值、功率为 *1kW 有符号）
struct PcsSlave {
    hold: Arc<Mutex<HashMap<u16, f64>>>,
}

impl PcsSlave {
    /// 按启停 + 有功方向推演 3 区运行状态：正放负充，P>0 放电(3)/P<0 充电(2)/P=0 待机(1)
    fn run_state(map: &HashMap<u16, f64>) -> f64 {
        if map.get(&REG_START_STOP).copied().unwrap_or(0.0) as u16 != 1 {
            return 0.0; // 停机
        }
        let mode = map.get(&REG_MODE).copied().unwrap_or(MODE_CONST_POWER as f64) as u16;
        let p_total: f64 = if mode == MODE_PHASE_SPLIT {
            (0..3).map(|i| map.get(&(REG_PHASE_P_A + i)).copied().unwrap_or(0.0)).sum()
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
    fn write(hold: &Arc<Mutex<HashMap<u16, f64>>>, addr: u16, raw: u16) {
        let real = from_pcs_reg(raw);
        hold.lock().unwrap().insert(addr, real);
        let name = if addr == REG_START_STOP {
            "启停 500".to_string()
        } else if addr == REG_MODE {
            "模式 1000".to_string()
        } else if addr == REG_CONST_P_SET {
            "恒功率P 1001".to_string()
        } else if addr == REG_CONST_Q_SET {
            "恒功率Q 1002".to_string()
        } else if (REG_PHASE_P_A..=REG_PHASE_P_A + 2).contains(&addr) {
            format!("相{} 有功 {addr}", ((addr - REG_PHASE_P_A) as u8 + b'A') as char)
        } else if (REG_PHASE_Q_A..=REG_PHASE_Q_A + 2).contains(&addr) {
            format!("相{} 无功 {addr}", ((addr - REG_PHASE_Q_A) as u8 + b'A') as char)
        } else {
            format!("保持 {addr}")
        };
        println!("[pcs-slave] 写 {name} = {real} (raw={raw:#06x})");
    }
}

impl Service for PcsSlave {
    type Request = Request<'static>;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Response, Exception>> + Send>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        let hold = Arc::clone(&self.hold);
        Box::pin(async move {
            match req {
                Request::ReadHoldingRegisters(addr, cnt) => {
                    let map = hold.lock().unwrap();
                    let out: Vec<u16> = (0..cnt)
                        .map(|i| to_pcs_reg(map.get(&addr.wrapping_add(i)).copied().unwrap_or(0.0)))
                        .collect();
                    Ok(Response::ReadHoldingRegisters(out))
                }
                Request::ReadInputRegisters(addr, cnt) => {
                    let map = hold.lock().unwrap();
                    let out: Vec<u16> = (0..cnt)
                        .map(|i| match addr.wrapping_add(i) {
                            REG_SOC => to_pcs_reg(SIM_SOC),
                            REG_RUN_STATE => to_pcs_reg(PcsSlave::run_state(&map)),
                            // 告警 1000-1004 / BMS 1005 / 故障 1014 / 输出 1029-1036：恒 0
                            _ => to_pcs_reg(0.0),
                        })
                        .collect();
                    Ok(Response::ReadInputRegisters(out))
                }
                Request::WriteSingleRegister(addr, value) => {
                    PcsSlave::write(&hold, addr, value);
                    Ok(Response::WriteSingleRegister(addr, value))
                }
                Request::WriteMultipleRegisters(addr, words) => {
                    for (i, w) in words.iter().copied().enumerate() {
                        PcsSlave::write(&hold, addr.wrapping_add(i as u16), w);
                    }
                    Ok(Response::WriteMultipleRegisters(addr, words.len() as u16))
                }
                _ => Err(Exception::IllegalFunction),
            }
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1).cloned().unwrap_or_else(|| "COM1".into());
    let baud: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(19200);
    let addr: u8 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let stream = SerialStream::open(&tokio_serial::new(&port, baud))?;
        println!("[pcs-slave] listening on {}@{} slave={}", port, baud, addr);
        let service = PcsSlave { hold: Arc::new(Mutex::new(HashMap::new())) };
        tokio_modbus::server::rtu::Server::new(stream).serve_forever(service).await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
