//! 口级总线抽象：每 port 一个 master，请求级 slave 读（§10.2/§10.7）。
//!
//! 同口多从站不能各建 Rs485Device（各持独立 tx_lock 互不排斥、double-open 冲突），故
//! `scheduler`（Task 5）不直接持 Rs485Device，而是经 [`StationBus`] 读：
//!
//! - 真实实现 [`Rs485PortBus`]：每口一个 `Rs485Device`（open 一次），内带 per-port
//!   async `bus_lock` 强制"口内串行"（读路径本身无 tx_lock 保证，见 [`Rs485PortBus`]）；
//!   阻塞 libc IO 用 `spawn_blocking` 承载。
//! - 测试实现 [`MockBus`]：脚本化串口（按 (slave,addr) 预置寄存器 / 注入超时），
//!   供本模块单测与 Task 5 scheduler 集成测使用。

use async_trait::async_trait;

/// 口级读错误（[`StationBus::read_holding`] 返回）。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BusError {
    /// 口打开失败：该口全站 offline（不阻断启动，§10.7）。
    #[error("口 {0} 打开失败: {1}")]
    Open(String, String),
    /// 读一段寄存器失败（超时/CRC/未预置）。
    #[error("站 slave={slave} 读 {addr:#06x}x{count} 失败: {reason}")]
    Read {
        slave: u8,
        addr: u16,
        count: u16,
        reason: String,
    },
}

/// 口级总线：读保持寄存器（口内串行；调用方按站 slave 传参）。
///
/// 真实 = [`Rs485PortBus`]（包单 `Rs485Device` + per-port async Mutex + spawn_blocking）；
/// 测试 = [`MockBus`]（脚本化串口）。scheduler 只依赖本 trait，纯逻辑可 mock 测。
#[async_trait]
pub trait StationBus: Send + Sync {
    /// 读一段保持寄存器（FC03；口内串行由实现方强制，真实实现经 per-port async Mutex）。
    async fn read_holding(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError>;
    /// 读一段输入寄存器（FC04；与 FC03 解码同构，供厂方点表用 input regs 的设备）。
    async fn read_input(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError>;
}

/// 真机：每 port 单 `Rs485Device`。构造 open 失败 → Err（该口全站 offline，不阻断启动，§10.7）。
pub struct Rs485PortBus {
    device: std::sync::Arc<rs485_plugin::device::Rs485Device>,
    /// 归一后的串口节点（如 `/dev/ttyS4`）；日志/排障用。
    port: String,
    /// per-port async Mutex：强制"口内串行"。读路径 send_frame→recv_frame 分两次短暂持
    /// port_fd 锁、其间不持 tx_lock（tx_lock 仅 transaction/transaction_with_handler 持有），
    /// 同口并发读会 A.send/B.send 交错污染；此锁使每次 `read_holding` 整体原子。
    bus_lock: tokio::sync::Mutex<()>,
}

impl Rs485PortBus {
    /// 用站配置建 device 并 open。
    ///
    /// port 归一：不以 `/` 开头则补 `/dev/`（兼容 §10.3 两种写法：`ttyS4` / `/dev/ttyS4`）。
    /// 串口参数：baud_rate 透传 `conf.baud_rate`（per-station baud，同口一致性由段内
    /// validate 保证，Task 1），余 8N1/timeout1000/Crc16Modbus 用 rs485 `Config::default()`。
    /// device_addr = 该口首个站的 slave（仅作 handler/委托缺省；southd 读走 `*_from`
    /// 显式 slave，不受影响）。
    pub fn open(conf: &crate::config::StationConf) -> Result<Self, BusError> {
        let c = rs485_plugin::config::Config {
            port: normalize_port(&conf.port),
            baud_rate: conf.baud_rate, // per-station baud（同口一致性由段内 validate 保证）
            device_addr: conf.slave,
            ..rs485_plugin::config::Config::default()
        };
        let handler = rs485_plugin::handlers::ProtocolHandlerRegistry::get(&conf.protocol, &c)
            .ok_or_else(|| {
                BusError::Open(
                    conf.port.clone(),
                    format!("无 {} handler", conf.protocol),
                )
            })?;
        let device = rs485_plugin::device::Rs485Device::new(
            format!("port_{}", conf.port),
            "modbus".into(),
            c,
            handler,
        );
        device
            .open()
            .map_err(|e| BusError::Open(conf.port.clone(), e.to_string()))?;
        Ok(Self {
            device: std::sync::Arc::new(device),
            port: normalize_port(&conf.port),
            bus_lock: tokio::sync::Mutex::new(()),
        })
    }
}

#[async_trait]
impl StationBus for Rs485PortBus {
    async fn read_holding(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError> {
        // per-port async Mutex 强制口内串行：读路径（send_frame→recv_frame 分两次持
        // port_fd 锁）不持 tx_lock，同口并发读会 send/recv 交错污染，故先整体拿
        // bus_lock（tokio Mutex 专为持锁跨 await 设计，此处跨 spawn_blocking 的 await
        // 安全）使每次读原子，再做阻塞 IO。`_g` 须为具名绑定：`let _ =` 会立刻释放锁。
        let _g = self.bus_lock.lock().await;
        let dev = self.device.clone();
        let port = self.port.clone();
        tokio::task::spawn_blocking(move || {
            tracing::debug!(port = %port, slave, addr, count, "southd 口读保持寄存器");
            dev.read_holding_registers_from(slave, addr, count)
                .map_err(|e| BusError::Read { slave, addr, count, reason: e.to_string() })
        })
        .await
        .map_err(|e| BusError::Read { slave, addr, count, reason: e.to_string() })?
    }

    async fn read_input(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError> {
        // 同 read_holding：per-port bus_lock 强制口内串行后再做阻塞 IO（FC04 input 寄存器，
        // send_frame→recv_frame 分两次持 port_fd 锁，同上防同口并发交错污染）。
        let _g = self.bus_lock.lock().await;
        let dev = self.device.clone();
        let port = self.port.clone();
        tokio::task::spawn_blocking(move || {
            tracing::debug!(port = %port, slave, addr, count, "southd 口读输入寄存器");
            dev.read_input_registers_from(slave, addr, count)
                .map_err(|e| BusError::Read { slave, addr, count, reason: e.to_string() })
        })
        .await
        .map_err(|e| BusError::Read { slave, addr, count, reason: e.to_string() })?
    }
}

/// 归一串口节点：`ttyS4` → `/dev/ttyS4`；已带 `/`（`/dev/ttyS1`）原样返回。
fn normalize_port(p: &str) -> String {
    if p.starts_with('/') {
        p.to_string()
    } else {
        format!("/dev/{p}")
    }
}

/// 测试/集成测用 mock 串口：脚本化响应（每 (slave,addr) 预置寄存器/错误），记录读调用。
///
/// 说明：放在 lib 顶层（非 cfg(test)）供 Task 5 scheduler 集成测跨模块引用；
/// 不含 IO、仅内存脚本，生产构建无副作用。
pub struct MockBus {
    /// (slave, addr) → 预置寄存器响应
    responses: std::sync::Mutex<std::collections::HashMap<(u8, u16), Vec<u16>>>,
    /// 待抛错读（模拟超时/CRC），消费即清；多次调用排队逐次抛错。
    fail_next: std::sync::Mutex<Vec<(u8, u16)>>,
    /// 已发生读调用清单（(slave, addr, count)，测试断言调度序/次数）。
    pub calls: std::sync::Mutex<Vec<(u8, u16, u16)>>,
    /// input 读响应（FC04）：(slave, addr) → 寄存器。与 `responses`（FC03）独立键——同一
    /// (slave,addr) 可同时有 holding/input 两套（Modbus FC03/FC04 是不同寄存器空间）。
    input_responses: std::sync::Mutex<std::collections::HashMap<(u8, u16), Vec<u16>>>,
    /// input 读失败队列（模拟超时），消费即清；多次调用排队逐次抛错。
    input_fail_next: std::sync::Mutex<Vec<(u8, u16)>>,
    /// FC04 读调用清单（(slave, addr, count)，测试断言）。
    pub input_calls: std::sync::Mutex<Vec<(u8, u16, u16)>>,
}

impl MockBus {
    pub fn new() -> Self {
        Self {
            responses: std::sync::Mutex::new(std::collections::HashMap::new()),
            fail_next: std::sync::Mutex::new(Vec::new()),
            calls: std::sync::Mutex::new(Vec::new()),
            input_responses: std::sync::Mutex::new(std::collections::HashMap::new()),
            input_fail_next: std::sync::Mutex::new(Vec::new()),
            input_calls: std::sync::Mutex::new(Vec::new()),
        }
    }
    /// 预置 (slave, addr) → 返回寄存器。
    pub fn put(&self, slave: u8, addr: u16, regs: Vec<u16>) {
        self.responses.lock().unwrap().insert((slave, addr), regs);
    }

    /// 预置 FC04 input 读响应。
    pub fn put_input(&self, slave: u8, addr: u16, regs: Vec<u16>) {
        self.input_responses.lock().unwrap().insert((slave, addr), regs);
    }

    /// 下次该 (slave, addr) 读抛 Err（模拟超时/CRC）——多次调用排队逐次抛错。
    pub fn fail_once(&self, slave: u8, addr: u16) {
        self.fail_next.lock().unwrap().push((slave, addr));
    }

    /// FC04 读失败一次（队列；消费即清）。
    pub fn fail_input_once(&self, slave: u8, addr: u16) {
        self.input_fail_next.lock().unwrap().push((slave, addr));
    }

    /// 已发生读调用次数（按 slave+addr 计数）。
    pub fn call_count(&self, slave: u8, addr: u16) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|&&(s, a, _)| s == slave && a == addr)
            .count()
    }

    /// FC04 读调用次数。
    pub fn input_call_count(&self, slave: u8, addr: u16) -> usize {
        self.input_calls
            .lock()
            .unwrap()
            .iter()
            .filter(|&&(s, a, _)| s == slave && a == addr)
            .count()
    }
}

impl Default for MockBus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StationBus for MockBus {
    async fn read_holding(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError> {
        self.calls.lock().unwrap().push((slave, addr, count));
        // 先在同一 guard 内查 + 删（消费即清），guard 出块即释放，避免重入死锁。
        let to_fail = {
            let mut q = self.fail_next.lock().unwrap();
            if let Some(pos) = q.iter().position(|&(s, a)| s == slave && a == addr) {
                q.remove(pos);
                true
            } else {
                false
            }
        };
        if to_fail {
            return Err(BusError::Read {
                slave,
                addr,
                count,
                reason: "mock 超时".into(),
            });
        }
        self.responses
            .lock()
            .unwrap()
            .get(&(slave, addr))
            .cloned()
            .ok_or_else(|| BusError::Read {
                slave,
                addr,
                count,
                reason: "mock 未预置".into(),
            })
    }

    async fn read_input(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError> {
        self.input_calls.lock().unwrap().push((slave, addr, count));
        // 与 read_holding 同构：先在同一 guard 内查 + 删（消费即清），guard 出块即释放。
        let to_fail = {
            let mut q = self.input_fail_next.lock().unwrap();
            if let Some(pos) = q.iter().position(|&(s, a)| s == slave && a == addr) {
                q.remove(pos);
                true
            } else {
                false
            }
        };
        if to_fail {
            return Err(BusError::Read {
                slave,
                addr,
                count,
                reason: "mock 超时".into(),
            });
        }
        self.input_responses
            .lock()
            .unwrap()
            .get(&(slave, addr))
            .cloned()
            .ok_or_else(|| BusError::Read {
                slave,
                addr,
                count,
                reason: "mock input 未预置".into(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Role, StationConf};

    fn conf(port: &str, protocol: &str) -> StationConf {
        StationConf {
            id: "t1".into(),
            role: Role::Battery,
            port: port.into(),
            protocol: protocol.into(),
            slave: 1,
            baud_rate: 9600,
            interval_ms: 1000,
            regs: Vec::new(),
        }
    }

    // ---------- MockBus ----------

    #[tokio::test]
    async fn mock_put_then_read_returns_preset_and_records_calls() {
        let bus = MockBus::new();
        bus.put(2, 0x100, vec![1, 2, 3]);
        let r = bus.read_holding(2, 0x100, 3).await.expect("读应返回预置");
        assert_eq!(r, vec![1, 2, 3]);
        assert_eq!(bus.calls.lock().unwrap().as_slice(), &[(2, 0x100, 3)]);
        assert_eq!(bus.call_count(2, 0x100), 1);
        assert_eq!(bus.call_count(2, 0x101), 0);
    }

    #[tokio::test]
    async fn mock_read_unpreset_addr_errors() {
        let bus = MockBus::new();
        let r = bus.read_holding(9, 0x200, 2).await;
        assert!(
            matches!(r, Err(BusError::Read { slave: 9, addr: 0x200, count: 2, .. })),
            "未预置 addr 应报 Err，实际 {r:?}"
        );
        assert_eq!(bus.call_count(9, 0x200), 1);
    }

    #[tokio::test]
    async fn mock_fail_once_consumes_then_recovers() {
        let bus = MockBus::new();
        bus.put(2, 0x100, vec![1, 2, 3]);
        bus.fail_once(2, 0x100);
        // 首次读：命中 fail_once → 超时
        assert!(bus.read_holding(2, 0x100, 3).await.is_err());
        // 二次读：fail 已消费 → 恢复返回预置值
        let r = bus.read_holding(2, 0x100, 3).await.expect("恢复应返回预置");
        assert_eq!(r, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn mock_fail_once_queues_multiple() {
        let bus = MockBus::new();
        bus.put(2, 0x100, vec![5]);
        bus.fail_once(2, 0x100);
        bus.fail_once(2, 0x100);
        // 两次排队 → 前两次 Err，第三次恢复
        assert!(bus.read_holding(2, 0x100, 1).await.is_err());
        assert!(bus.read_holding(2, 0x100, 1).await.is_err());
        assert_eq!(bus.read_holding(2, 0x100, 1).await.expect("恢复"), vec![5]);
    }

    #[tokio::test]
    async fn mock_call_count_across_repeated_reads() {
        let bus = MockBus::new();
        bus.put(1, 0x10, vec![7]);
        let _ = bus.read_holding(1, 0x10, 1).await;
        let _ = bus.read_holding(1, 0x10, 1).await;
        let _ = bus.read_holding(1, 0x11, 2).await; // 不同键不计入
        assert_eq!(bus.call_count(1, 0x10), 2);
        assert_eq!(bus.calls.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn mock_put_input_then_read_input_returns_preset() {
        let bus = MockBus::new();
        bus.put_input(2, 0x100, vec![9, 8]);
        let r = bus.read_input(2, 0x100, 2).await.expect("input 读应返回预置");
        assert_eq!(r, vec![9, 8]);
        assert_eq!(bus.input_call_count(2, 0x100), 1);
        // 与 holding 键独立：同 (slave,addr) 的 holding 未预置仍 Err
        // （FC03/FC04 是不同寄存器空间，input 预置不影响 read_holding）。
        assert!(bus.read_holding(2, 0x100, 2).await.is_err());
    }

    #[tokio::test]
    async fn mock_input_fail_once_consumes_then_recovers() {
        let bus = MockBus::new();
        bus.put_input(2, 0x100, vec![5]);
        bus.fail_input_once(2, 0x100);
        // 首次 input 读：命中 fail → 超时
        assert!(bus.read_input(2, 0x100, 1).await.is_err());
        // 二次 input 读：fail 已消费 → 恢复返回预置值
        assert_eq!(bus.read_input(2, 0x100, 1).await.expect("恢复"), vec![5]);
        assert_eq!(bus.input_call_count(2, 0x100), 2);
    }

    // ---------- normalize_port ----------

    #[test]
    fn normalize_port_prepends_dev_prefix_only_when_missing() {
        assert_eq!(normalize_port("ttyS4"), "/dev/ttyS4");
        assert_eq!(normalize_port("/dev/ttyS1"), "/dev/ttyS1");
        assert_eq!(normalize_port("ttyS2"), "/dev/ttyS2");
    }

    // ---------- Rs485PortBus::open（全部走 open 失败路径，不触碰真串口）----------
    //
    // 故意用确定不存在的节点名（southd_ut_no_such_tty）：若误用真实存在的 ttyS4，
    // Linux 真机/CI 会真 open + tcsetattr 配成 9600/8N1（污染宿主串口）。失败路径：
    // Windows：Rs485Device::open 恒 Err（"Windows 平台暂不支持串口打开"）；
    // unix：归一为 /dev/southd_ut_no_such_tty 不存在节点 → open Err。故 is_err 跨平台成立。

    #[tokio::test]
    async fn open_no_dev_prefix_normalizes_and_reports_raw_port() {
        // conf.port = "southd_ut_no_such_tty"（无 /dev 前缀）→ 归一后仍走
        // /dev/xxx 打开失败路径（unix 节点不存在 / Windows 恒 Err），错误携带原始 port。
        let r = Rs485PortBus::open(&conf("southd_ut_no_such_tty", "modbus"));
        assert!(
            matches!(r, Err(BusError::Open(ref p, _)) if p == "southd_ut_no_such_tty"),
            "open 失败且错误携带原始 port"
        );
    }

    #[tokio::test]
    async fn open_nonexistent_dev_node_errors_without_panic() {
        let r = Rs485PortBus::open(&conf("/nonexistent", "modbus"));
        assert!(matches!(r, Err(BusError::Open(ref p, _)) if p == "/nonexistent"));
    }

    #[tokio::test]
    async fn open_unsupported_protocol_errors_early() {
        // 协议非 modbus（如 private）→ registry 无 handler → Open Err（不触设备 open）；
        // port 亦用不存在节点名，防未来该 handler 被注册后误碰真串口。
        let r = Rs485PortBus::open(&conf("southd_ut_no_such_tty", "private"));
        assert!(
            matches!(r, Err(BusError::Open(_, ref reason)) if reason.contains("private")),
            "无 handler 应报 Open Err"
        );
    }

    #[tokio::test]
    async fn open_modbus_hits_device_open_error_path() {
        // modbus handler 存在 → 进入 device.open() 失败路径（Windows 恒失败 / unix 不存在节点）
        let r = Rs485PortBus::open(&conf("southd_ut_no_such_tty", "modbus"));
        assert!(r.is_err());
    }
}
