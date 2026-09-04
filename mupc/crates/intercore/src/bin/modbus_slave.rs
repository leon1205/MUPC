//! Modbus RTU Slave 参考实现——模拟实时控制模块（设计 §11.4/§11.6）
//!
//! 以 tokio-modbus **server**（从站）绑定串口，暴露 §11.4 保持寄存器区
//! （控制区 0x0000 / 执行确认区 0x0030-0x0033 / 状态心跳区 0x0100-0x0105）：
//!
//! - 收到 `cmd_valid` 上升沿（`WriteMultipleRegisters` FC16 或 `WriteSingleRegister`
//!   FC06 写 `REG_CMD_CTRL`，Task 5 Master 实际走 FC16）→ ① 校验
//!   `REG_PROTOCOL_VERSION`==1 → ② 回写 `exec_seq`（int32，0x0031=seq）/ `exec_status`
//!   （2 成功 / 3 失败）→ ③ 清 `cmd_valid`（写回 `v & !0x0001`）；
//! - 心跳任务每 100ms 递增 `REG_HEARTBEAT`（0x0100），Master 轮询判在线/超时。
//!
//! 用途：本地联调验证寄存器映射与执行确认（虚拟串口对，如 com0com / socat），
//! 并作为实时模块固件的寄存器协议参照。
//!
//! 用法：`modbus_slave <serial_port> [baud] [slave_addr]`
//! 例：`cargo run -p mupc-intercore --bin modbus_slave -- COM1 9600 1`
use mupc_intercore::modbus_rtu::{
    cmd_seq_of, cmd_valid_of, i32_to_regs, EXEC_FAILED, EXEC_SUCCESS, PROTOCOL_VERSION,
    REG_CMD_CTRL, REG_DEVICE_STATUS, REG_EXEC_SEQ, REG_EXEC_STATUS, REG_HEARTBEAT,
    REG_PROTOCOL_VERSION,
};
use std::pin::Pin;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_modbus::prelude::*;
use tokio_modbus::server::Service;
use tokio_serial::SerialStream;

/// 寄存器区大小：地址 0x0000..=0x0105，共 0x0106 = 262 个
const REG_COUNT: usize = 0x0106;

/// 心跳递增周期
const HEARTBEAT_PERIOD_MS: u64 = 100;

/// 从站服务：暴露保持寄存器区，实现 tokio-modbus `server::Service`
struct SlaveService {
    regs: Arc<Vec<AtomicU16>>,
    heartbeat: Arc<AtomicU16>,
}

impl SlaveService {
    /// 读单寄存器；`REG_HEARTBEAT` 特殊读心跳原子计数（镜像区不追写）
    fn read_reg(&self, addr: u16) -> u16 {
        if addr == REG_HEARTBEAT {
            self.heartbeat.load(Ordering::Relaxed)
        } else {
            self.regs[addr as usize].load(Ordering::Relaxed)
        }
    }

    /// 写单寄存器（写入镜像区）
    fn write_reg(&self, addr: u16, v: u16) {
        self.regs[addr as usize].store(v, Ordering::Relaxed);
    }

    /// cmd_valid 上升沿采纳：版本校验 → 回写 exec_seq/exec_status → 清 valid
    ///
    /// §11.4 语义：`exec_seq` 为 int32（大端，2 寄存器），seq 为 u8 故高 16 位（0x0030）=0、
    /// 低 16 位（0x0031）=seq（对齐 Master `regs_to_i32` 重组与 `exec_seq == cmd_seq` 比对）。
    /// 版本不符同样回写（exec_status=3 失败），Master 轮询读取后按失败处理。
    fn accept_cmd(&self, v: u16) {
        let seq = cmd_seq_of(v);
        let version_ok =
            self.regs[REG_PROTOCOL_VERSION as usize].load(Ordering::Relaxed) == PROTOCOL_VERSION;
        let status = if version_ok {
            EXEC_SUCCESS
        } else {
            EXEC_FAILED
        };
        let [hi, lo] = i32_to_regs(seq as i32);
        self.regs[REG_EXEC_SEQ as usize].store(hi, Ordering::Relaxed);
        self.regs[(REG_EXEC_SEQ + 1) as usize].store(lo, Ordering::Relaxed);
        self.regs[REG_EXEC_STATUS as usize].store(status, Ordering::Relaxed);
        // 清 cmd_valid：同值不二次触发，就绪下一指令（设计流程第 4 步）
        self.regs[REG_CMD_CTRL as usize].store(v & !0x0001, Ordering::Relaxed);
        println!("[modbus-slave] cmd accepted seq={} exec={}", seq, status);
    }
}

impl Service for SlaveService {
    type Request = Request<'static>;
    type Future = Pin<Box<dyn std::future::Future<Output = Result<Response, Exception>> + Send>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        // 复制共享状态到 async 块（&self 无法被 'static Future 借用）
        let svc = SlaveService {
            regs: Arc::clone(&self.regs),
            heartbeat: Arc::clone(&self.heartbeat),
        };
        Box::pin(async move {
            match req {
                Request::ReadHoldingRegisters(addr, cnt) => {
                    if addr as usize + cnt as usize > REG_COUNT {
                        return Err(Exception::IllegalDataAddress);
                    }
                    let mut out = Vec::with_capacity(cnt as usize);
                    for i in 0..cnt {
                        // 已校验 addr+cnt <= REG_COUNT，不会回绕
                        out.push(svc.read_reg(addr.wrapping_add(i)));
                    }
                    Ok(Response::ReadHoldingRegisters(out))
                }
                Request::WriteMultipleRegisters(addr, words) => {
                    // Task 5 Master 以 FC16（write_multiple_registers，长度 1）写 REG_CMD_CTRL
                    if addr as usize + words.len() > REG_COUNT {
                        return Err(Exception::IllegalDataAddress);
                    }
                    let mut cmd_ctrl: Option<u16> = None;
                    for (i, w) in words.iter().copied().enumerate() {
                        let a = addr.wrapping_add(i as u16);
                        svc.write_reg(a, w);
                        if a == REG_CMD_CTRL && cmd_valid_of(w) {
                            cmd_ctrl = Some(w);
                        }
                    }
                    if let Some(w) = cmd_ctrl {
                        svc.accept_cmd(w);
                    }
                    Ok(Response::WriteMultipleRegisters(addr, words.len() as u16))
                }
                Request::WriteSingleRegister(addr, v) => {
                    if addr as usize >= REG_COUNT {
                        return Err(Exception::IllegalDataAddress);
                    }
                    svc.write_reg(addr, v);
                    if addr == REG_CMD_CTRL && cmd_valid_of(v) {
                        svc.accept_cmd(v);
                    }
                    Ok(Response::WriteSingleRegister(addr, v))
                }
                // 其余功能码（线圈/离散量/自定义等）未暴露
                _ => Err(Exception::IllegalFunction),
            }
        })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1).cloned().unwrap_or_else(|| "COM1".into());
    let baud: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(9600);
    let addr: u8 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let stream = SerialStream::open(&tokio_serial::new(&port, baud))?;
        println!(
            "[modbus-slave] listening on {}@{} slave={}",
            port, baud, addr
        );

        // 保持寄存器镜像区（0x0000..=0x0105），除 REG_HEARTBEAT 外的读写落点
        let regs: Arc<Vec<AtomicU16>> =
            Arc::new((0..REG_COUNT).map(|_| AtomicU16::new(0)).collect());
        // §11.4 状态区 device_status：bit0 运行
        regs[REG_DEVICE_STATUS as usize].store(0x0001, Ordering::Relaxed);

        // 心跳计数（REG_HEARTBEAT 源，任务每 100ms 递增）
        let heartbeat = Arc::new(AtomicU16::new(0));
        {
            let hb = Arc::clone(&heartbeat);
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(HEARTBEAT_PERIOD_MS)).await;
                    hb.fetch_add(1, Ordering::Relaxed);
                }
            });
        }

        let service = SlaveService { regs, heartbeat };
        tokio_modbus::server::rtu::Server::new(stream)
            .serve_forever(service)
            .await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
