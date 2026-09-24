//! PCS V1.3 协议 Modbus 从站仿真——模拟两级式 PCS（=实时控制模块）
//!
//! **薄壳**（T-L0）：从站服务与状态已沉至 [`mupc_intercore::pcs_sim`]（`PcsSimState` /
//! `PcsSlaveService` / `serve_rtu`），本 bin 仅解析参数 → 开串口 → serve，行为与沉库前
//! 一致；进程内 e2e 回归（`cargo test -p mupc-intercore e2e_pcs_sim`）共用同一仿真库，
//! **无需串口硬件**即可验证协议语义（含 3 区告警字 1000-1004——急停故障位经
//! `PcsSimState::set_alarm` 置位，R2 边界消除）。
//!
//! 协议语义（4 区保持 500/1000/1001-1002/1006-1011、3 区输入 SOC=1010 / 运行状态 1013 /
//! 告警字、高 8/低 8 字节互换）详见 `mupc_intercore::pcs_sim` 模块文档。
//!
//! 用途：无 PCS 实机时经虚拟串口对（Linux socat / Windows com0com）或 USB-RS485 回环与
//! `ModbusRtuTransport`（transport=modbus_rtu）做**双进程**端到端联调（方案 §6.0 备选手段；
//! 主手段为上述进程内 e2e）。
//!
//! 用法：`pcs_slave <serial_port> [baud] [slave_addr]`
//! 例：`cargo run -p mupc-intercore --bin pcs_slave -- /dev/ttyS0 19200 1`
use mupc_intercore::pcs_sim::{serve_rtu, PcsSimState, PcsSlaveService};
use std::sync::Arc;
use tokio_serial::SerialStream;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let port = args.get(1).cloned().unwrap_or_else(|| "COM1".into());
    let baud: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(19200);
    let addr: u8 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let stream = SerialStream::open(&tokio_serial::new(&port, baud))?;
        println!("[pcs-slave] listening on {}@{} slave={}", port, baud, addr);
        let service = PcsSlaveService::new(Arc::new(PcsSimState::new()));
        serve_rtu(stream, service).await?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
