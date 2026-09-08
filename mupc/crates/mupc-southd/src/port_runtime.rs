//! 口级总线抽象：每 port 一个 master，请求级 slave 读（§10.2/§10.7）。
//!
//! 同口多从站不能各建 Rs485Device（各持 tx_lock 不互斥、double-open 冲突），故
//! `scheduler`（Task 5）不直接持 Rs485Device，而是经 [`StationBus`] 读：
//!
//! - 真实实现 [`Rs485PortBus`]：每口一个 `Rs485Device`（open 一次），阻塞 libc IO
//!   走 `spawn_blocking`；设备内部 tx_lock 串行化同口请求。
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
/// 真实 = [`Rs485PortBus`]（包单 `Rs485Device` + spawn_blocking）；
/// 测试 = [`MockBus`]（脚本化串口）。scheduler 只依赖本 trait，纯逻辑可 mock 测。
#[async_trait]
pub trait StationBus: Send + Sync {
    /// 读一段保持寄存器（FC03；多从站同口由设备 tx_lock 串行）。
    async fn read_holding(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError>;
}

/// 真机：每 port 单 `Rs485Device`。构造 open 失败 → Err（该口全站 offline，不阻断启动，§10.7）。
pub struct Rs485PortBus {
    device: std::sync::Arc<rs485_plugin::device::Rs485Device>,
    /// 归一后的串口节点（如 `/dev/ttyS4`）；日志/排障用。
    port: String,
}

impl Rs485PortBus {
    /// 用站配置建 device 并 open。
    ///
    /// port 归一：不以 `/` 开头则补 `/dev/`（兼容 §10.3 两种写法：`ttyS4` / `/dev/ttyS4`）。
    /// 串口参数用 rs485 `Config::default()`（9600/8N1/timeout1000/Crc16Modbus；厂方各口
    /// 波特率差异由 S3b 再补覆盖字段）。device_addr = 该口首个站的 slave（仅作
    /// handler/委托缺省；southd 读走 `*_from` 显式 slave，不受影响）。
    pub fn open(conf: &crate::config::StationConf) -> Result<Self, BusError> {
        let c = rs485_plugin::config::Config {
            port: normalize_port(&conf.port),
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
        })
    }
}

#[async_trait]
impl StationBus for Rs485PortBus {
    async fn read_holding(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError> {
        // spawn_blocking 承载阻塞 libc 读；Rs485Device 内部 tx_lock 串行化同口请求。
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
}

impl MockBus {
    pub fn new() -> Self {
        Self {
            responses: std::sync::Mutex::new(std::collections::HashMap::new()),
            fail_next: std::sync::Mutex::new(Vec::new()),
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }
    /// 预置 (slave, addr) → 返回寄存器。
    pub fn put(&self, slave: u8, addr: u16, regs: Vec<u16>) {
        self.responses.lock().unwrap().insert((slave, addr), regs);
    }

    /// 下次该 (slave, addr) 读抛 Err（模拟超时/CRC）——多次调用排队逐次抛错。
    pub fn fail_once(&self, slave: u8, addr: u16) {
        self.fail_next.lock().unwrap().push((slave, addr));
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

    // ---------- normalize_port ----------

    #[test]
    fn normalize_port_prepends_dev_prefix_only_when_missing() {
        assert_eq!(normalize_port("ttyS4"), "/dev/ttyS4");
        assert_eq!(normalize_port("/dev/ttyS1"), "/dev/ttyS1");
        assert_eq!(normalize_port("ttyS2"), "/dev/ttyS2");
    }

    // ---------- Rs485PortBus::open（全部走 open 失败路径，不触碰真串口）----------
    //
    // Windows：Rs485Device::open 恒 Err（"Windows 平台暂不支持串口打开"）；
    // unix：port 归一为 /dev/xxx 不存在节点 → open Err。故 is_err 断言跨平台成立。

    #[tokio::test]
    async fn open_no_dev_prefix_normalizes_and_reports_raw_port() {
        // conf.port = "ttyS4"（无 /dev 前缀）→ 归一 /dev/ttyS4 → open 失败
        let r = Rs485PortBus::open(&conf("ttyS4", "modbus"));
        assert!(
            matches!(r, Err(BusError::Open(ref p, _)) if p == "ttyS4"),
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
        // 协议非 modbus（如 private）→ registry 无 handler → Open Err（不触设备 open）
        let r = Rs485PortBus::open(&conf("ttyS4", "private"));
        assert!(
            matches!(r, Err(BusError::Open(_, ref reason)) if reason.contains("private")),
            "无 handler 应报 Open Err"
        );
    }

    #[tokio::test]
    async fn open_modbus_hits_device_open_error_path() {
        // modbus handler 存在 → 进入 device.open() 失败路径（Windows 恒失败 / unix 无节点）
        let r = Rs485PortBus::open(&conf("ttyS4", "modbus"));
        assert!(r.is_err());
    }
}
