//! 口级总线抽象：每 port 一个 master，请求级 slave 读写（§10.2/§10.7）。
//!
//! 同口多从站不能各建 Rs485Device（各持独立 tx_lock 互不排斥、double-open 冲突），故
//! `scheduler`（Task 5）不直接持 Rs485Device，而是经 [`StationBus`] 读写：
//!
//! - 真实实现 [`Rs485PortBus`]：每口一个 `Rs485Device`（open 一次），内带 per-port
//!   async `bus_lock` 强制"口内串行"（读路径本身无 tx_lock 保证，见 [`Rs485PortBus`]）；
//!   阻塞 libc IO 用 `spawn_blocking` 承载。
//! - 测试实现 [`MockBus`]：脚本化串口（按 (slave,addr) 预置寄存器 / 注入超时、记录写调用），
//!   供本模块单测与 Task 5 scheduler 集成测使用。

use async_trait::async_trait;

/// 口级总线错误（[`StationBus`] 的读/写方法返回）。
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
    /// 写单个寄存器失败（超时/CRC/回显不符/异常响应）。
    #[error("站 slave={slave} 写 {addr:#06x}={value:#06x} 失败: {reason}")]
    Write {
        slave: u8,
        addr: u16,
        value: u16,
        reason: String,
    },
}

/// 口层**控制面参数**：`StationConf` **无落点**、而 `rs485_plugin::config::Config` 需要三项
/// （单次事务超时 / 数据位 / 停止位）。
///
/// **为什么单列而不给 `StationConf` 加字段**：站级段的语义是"一口多站的口/从站/点表"，
/// 不含控制面；只有 `south_pcs`（PCS 独占口的单值段）带这三项 ⇒ 由调用方显式传入，
/// **不为 PCS 改站级结构语义**。
///
/// **默认值 = `rs485_plugin::config::Config::default()` 的同三项**（超时 1000ms / 8N1）
/// ⇒ 站级路径经 [`Rs485PortBus::open`] 调用时与改动前**逐字段等价**（`bus_config` 原先直接
/// `..Config::default()` 兜底）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortParams {
    /// 单次读写事务超时（毫秒）。⚠️ 落进 `termios` 的 `VTIME = timeout_ms / 100`
    /// （**100ms 粒度**，见 `rs485-plugin/src/device.rs` 的 `configure_port`）。
    pub timeout_ms: u64,
    /// 数据位（5..=8；越界由 [`Rs485PortBus::open_with_port_params`] fail-closed 拒）。
    pub data_bits: u8,
    /// 停止位（1..=2；越界同上）。
    pub stop_bits: u8,
}

impl Default for PortParams {
    fn default() -> Self {
        // 单一真源：不手写 1000/8/1 三个字面量（rs485 侧改默认值时本处必须同向跟随）
        let d = rs485_plugin::config::Config::default();
        Self {
            timeout_ms: d.timeout_ms,
            data_bits: d.data_bits,
            stop_bits: d.stop_bits,
        }
    }
}

/// 口级总线：按站读写寄存器 / 位（口内串行；调用方按站 slave 传参）。
///
/// 真实 = [`Rs485PortBus`]（包单 `Rs485Device` + per-port async Mutex + spawn_blocking）；
/// 测试 = [`MockBus`]（脚本化串口）。scheduler 只依赖本 trait，纯逻辑可 mock 测。
#[async_trait]
pub trait StationBus: Send + Sync {
    /// 读一段保持寄存器（FC03；口内串行由实现方强制，真实实现经 per-port async Mutex）。
    async fn read_holding(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError>;
    /// 读一段输入寄存器（FC04；与 FC03 解码同构，供厂方点表用 input regs 的设备）。
    async fn read_input(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError>;
    /// 读离散输入（FC02）：`count` = **位数**（非寄存器数）；返回长度 = `count` 的位向量
    /// （`bit k` = 响应字节第 `k%8` 位，bit0 = LSB —— 见 `rs485_plugin::unpack_bits`）。
    /// S3b-2 T5（设计 §11.4.5）：位块（`RegFunc::Discrete`）的通路，与既有两方法同构。
    async fn read_discrete(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<bool>, BusError>;
    /// 写单个保持寄存器（FC06），显式从站地址。
    ///
    /// **为什么只加这一个写方法**（设计 §13.3/§13.4）：PCS 控制序列全部落在 4 区单寄存器写
    /// （模式 1000 / 启停 500 / 恒功率 1001-1002 / 分相 1006-1011），逐写即协议要求（FC06 无批量）。
    /// 不提供批量写 / 任意地址范围写 —— 写能力的**门只开一条缝**，把"能写什么"交给调用方
    /// `PcsHandle` 的 **4 个受限入口**（设计 §13.5.3：`send_dual_param` / `send_tai_command`
    /// / `stop` / `restore_interlock_latched`），而非把写权限摊开在 bus 层。
    async fn write_single(&self, slave: u8, addr: u16, value: u16) -> Result<(), BusError>;
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
    /// 用站配置建 device 并 open（**站级路径**：口层控制面参数取 [`PortParams::default()`]）。
    ///
    /// port 归一：不以 `/` 开头则补 `/dev/`（兼容 §10.3 两种写法：`ttyS4` / `/dev/ttyS4`）。
    /// 串口参数：baud_rate 透传 `conf.baud_rate`（per-station baud，同口一致性由段内
    /// validate 保证，Task 1），**parity 透传 `conf.parity`**（S3b-2 T5，见 [`bus_config`]）；
    /// **超时/数据位/停止位**取 [`PortParams::default()`]（= 改动前的
    /// `Config::default()`：timeout1000 / 8N1 ⇒ 站级行为逐字段不变）；
    /// Crc16Modbus/DE-RE 仍用 rs485 `Config::default()`。
    /// device_addr = 该口首个站的 slave（仅作 handler/委托缺省；southd 读走 `*_from`
    /// 显式 slave，不受影响）。
    pub fn open(conf: &crate::config::StationConf) -> Result<Self, BusError> {
        Self::open_with_port_params(conf, PortParams::default())
    }

    /// 与 [`Self::open`] 同，另收**口层控制面参数**（`StationConf` 无落点的三项；见 [`PortParams`]）。
    ///
    /// **当前唯一生产消费者**：`mupc-core-bin` 的 PCS 装配段（`south_pcs.response_timeout_ms`
    /// / `data_bits` / `stop_bits`）。迁移前这三项走 `intercore.modbus_rtu`；迁入南向后若没有
    /// 落点就是**死配置** —— 尤以 `response_timeout_ms` 有实害：yaml 写 200 而实际取
    /// `Config::default()` 的 1000ms，会拖慢 `stop()` 这类安全动作的失败检测（Task 10 评审项 2）。
    ///
    /// **fail-closed 边界**（与 `rs485_plugin::config::Config::validate()` 同源）：`data_bits`
    /// 须 5..=8、`stop_bits` 须 1..=2、`timeout_ms` 须 > 0 —— 越界即 `Err`。原因：rs485 侧
    /// `configure_port` 对越界值是 `_ =>` **静默**落到 8/1 ⇒ 不拦就是拿一种静默降级换另一种。
    /// 站级路径不受影响：`PortParams::default()` 恒在界内，永不触发本守卫。
    pub fn open_with_port_params(
        conf: &crate::config::StationConf,
        params: PortParams,
    ) -> Result<Self, BusError> {
        if !(5..=8).contains(&params.data_bits) {
            return Err(BusError::Open(
                conf.port.clone(),
                format!("data_bits={} 越界（须 5..=8）", params.data_bits),
            ));
        }
        if !(1..=2).contains(&params.stop_bits) {
            return Err(BusError::Open(
                conf.port.clone(),
                format!("stop_bits={} 越界（须 1..=2）", params.stop_bits),
            ));
        }
        if params.timeout_ms == 0 {
            return Err(BusError::Open(
                conf.port.clone(),
                "timeout_ms=0 非法（VTIME 会落 0 ⇒ 读恒即时返回）".to_string(),
            ));
        }
        let c = bus_config(conf, params);
        let handler = rs485_plugin::handlers::ProtocolHandlerRegistry::get(&conf.protocol, &c)
            .ok_or_else(|| {
                BusError::Open(conf.port.clone(), format!("无 {} handler", conf.protocol))
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
                .map_err(|e| BusError::Read {
                    slave,
                    addr,
                    count,
                    reason: e.to_string(),
                })
        })
        .await
        .map_err(|e| BusError::Read {
            slave,
            addr,
            count,
            reason: e.to_string(),
        })?
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
                .map_err(|e| BusError::Read {
                    slave,
                    addr,
                    count,
                    reason: e.to_string(),
                })
        })
        .await
        .map_err(|e| BusError::Read {
            slave,
            addr,
            count,
            reason: e.to_string(),
        })?
    }

    async fn read_discrete(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<bool>, BusError> {
        // 同 read_holding/read_input：per-port bus_lock 强制口内串行后再做阻塞 IO
        //（FC02 离散输入；count = 位数，返回长度 = count 的位向量）。
        let _g = self.bus_lock.lock().await;
        let dev = self.device.clone();
        let port = self.port.clone();
        tokio::task::spawn_blocking(move || {
            tracing::debug!(port = %port, slave, addr, count, "southd 口读离散输入");
            dev.read_discrete_inputs_from(slave, addr, count)
                .map_err(|e| BusError::Read {
                    slave,
                    addr,
                    count,
                    reason: e.to_string(),
                })
        })
        .await
        .map_err(|e| BusError::Read {
            slave,
            addr,
            count,
            reason: e.to_string(),
        })?
    }

    async fn write_single(&self, slave: u8, addr: u16, value: u16) -> Result<(), BusError> {
        // 与读路径同款：per-port `bus_lock` 强制口内串行（读与写在同一条物理总线上，
        // 交错即帧污染），阻塞 IO 交给 spawn_blocking。`_g` 须**具名绑定**：`let _ =` 会立刻放锁。
        let _g = self.bus_lock.lock().await;
        let dev = self.device.clone();
        let port = self.port.clone();
        tokio::task::spawn_blocking(move || {
            tracing::debug!(port = %port, slave, addr, value, "southd 口写单寄存器");
            dev.write_single_register_from(slave, addr, value)
                .map_err(|e| BusError::Write {
                    slave,
                    addr,
                    value,
                    reason: e.to_string(),
                })
        })
        .await
        // join 失败沿用读侧同款格式（`e.to_string()`），**有意为之**：
        // `JoinError` 自身 Display 已含 "task N panicked/was cancelled"，
        // 与设备侧 Rs485Error 文案（"写响应过短"/"被从站拒绝"）天然可分；
        // 四方法保持同构优于再加一层"join 失败"前缀。请勿"顺手修正"成不对称写法。
        .map_err(|e| BusError::Write {
            slave,
            addr,
            value,
            reason: e.to_string(),
        })?
    }
}

/// 参数 → rs485 口配置（纯函数，可测接缝：**不必触碰真串口**即可断言透传结果）。
///
/// **S3b-2 T5 新增 `parity` 透传（设计 §11.4.5 末条，v1.4 补）**：既有实现只透传
/// `baud_rate`/`device_addr`，其余走 `Config::default()` ⇒ 校验位**恒 `Parity::None`**，
/// 站级 `parity` 被**静默忽略**（配置写 `even` 却按 `none` 通信 ⇒ 空调站整站通信失败，
/// 且现象是 "offline" 而非"配置错"）。该透传是 **D-1 空调校验位裁定（RC-6）的代码侧前置**：
/// 现场一旦裁定为 `even`，**只改配置即可生效、无需改代码**。
///
/// **Task 10 评审项 2 新增 `PortParams` 透传**（超时/数据位/停止位）：站级段没有这三项，
/// 原先一律 `..Config::default()`（timeout1000 / 8N1）⇒ `south_pcs` 段写了也**不生效**
/// （YAML 写 200ms、实际 1000ms）。现由调用方经 [`PortParams`] 显式传入；站级路径传
/// `PortParams::default()` ⇒ 与改动前逐字段等价。
///
/// 其余参数（`crc_mode`/DE-RE）仍取 `Config::default()`。
fn bus_config(
    conf: &crate::config::StationConf,
    params: PortParams,
) -> rs485_plugin::config::Config {
    use crate::config::StationParity;
    rs485_plugin::config::Config {
        port: normalize_port(&conf.port),
        baud_rate: conf.baud_rate, // per-station baud（同口一致性由段内 validate 保证）
        device_addr: conf.slave,
        parity: match conf.parity {
            StationParity::None => rs485_plugin::Parity::None,
            StationParity::Even => rs485_plugin::Parity::Even,
            StationParity::Odd => rs485_plugin::Parity::Odd,
        },
        // 口层控制面三项（见 `PortParams`）：缺省即 rs485 默认 ⇒ 站级路径行为不变
        timeout_ms: params.timeout_ms,
        data_bits: params.data_bits,
        stop_bits: params.stop_bits,
        ..rs485_plugin::config::Config::default()
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
    /// FC02 位读响应（离散输入）：(slave, addr) → 位向量。与 holding/input 三套**独立键**
    ///（FC03/FC04/FC02 是三个不同地址空间）。
    bits_responses: std::sync::Mutex<std::collections::HashMap<(u8, u16), Vec<bool>>>,
    /// FC02 读失败队列（模拟超时），消费即清。
    bits_fail_next: std::sync::Mutex<Vec<(u8, u16)>>,
    /// FC02 读调用清单（(slave, addr, count)，测试断言）。
    pub bit_calls: std::sync::Mutex<Vec<(u8, u16, u16)>>,
    /// 待抛错写（模拟超时/回显不符），消费即清；多次调用排队逐次抛错。
    write_fail_next: std::sync::Mutex<Vec<(u8, u16)>>,
    /// 已发生写调用清单（(slave, addr, value)，测试断言）。
    pub write_calls: std::sync::Mutex<Vec<(u8, u16, u16)>>,
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
            bits_responses: std::sync::Mutex::new(std::collections::HashMap::new()),
            bits_fail_next: std::sync::Mutex::new(Vec::new()),
            bit_calls: std::sync::Mutex::new(Vec::new()),
            write_fail_next: std::sync::Mutex::new(Vec::new()),
            write_calls: std::sync::Mutex::new(Vec::new()),
        }
    }
    /// 预置 (slave, addr) → 返回寄存器。
    pub fn put(&self, slave: u8, addr: u16, regs: Vec<u16>) {
        self.responses.lock().unwrap().insert((slave, addr), regs);
    }

    /// 预置 FC04 input 读响应。
    pub fn put_input(&self, slave: u8, addr: u16, regs: Vec<u16>) {
        self.input_responses
            .lock()
            .unwrap()
            .insert((slave, addr), regs);
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

    /// 预置 FC02 离散输入位响应（`count` = 位数；返回长度由测试自行保证 = count）。
    pub fn put_bits(&self, slave: u8, addr: u16, bits: Vec<bool>) {
        self.bits_responses
            .lock()
            .unwrap()
            .insert((slave, addr), bits);
    }

    /// FC02 读失败一次（队列；消费即清）。
    pub fn fail_bits_once(&self, slave: u8, addr: u16) {
        self.bits_fail_next.lock().unwrap().push((slave, addr));
    }

    /// FC02 读调用次数。
    pub fn bit_call_count(&self, slave: u8, addr: u16) -> usize {
        self.bit_calls
            .lock()
            .unwrap()
            .iter()
            .filter(|&&(s, a, _)| s == slave && a == addr)
            .count()
    }

    /// 下次该 (slave, addr) 写抛 Err（消费即清）。
    pub fn fail_write_once(&self, slave: u8, addr: u16) {
        self.write_fail_next.lock().unwrap().push((slave, addr));
    }

    /// 某 (slave, addr) 的写调用次数。
    pub fn write_call_count(&self, slave: u8, addr: u16) -> usize {
        self.write_calls
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

    async fn read_discrete(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<bool>, BusError> {
        self.bit_calls.lock().unwrap().push((slave, addr, count));
        // 与 read_holding/read_input 同构：先在同一 guard 内查 + 删（消费即清），guard 出块即释放。
        let to_fail = {
            let mut q = self.bits_fail_next.lock().unwrap();
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
                reason: "mock 位读超时".into(),
            });
        }
        self.bits_responses
            .lock()
            .unwrap()
            .get(&(slave, addr))
            .cloned()
            .ok_or_else(|| BusError::Read {
                slave,
                addr,
                count,
                reason: "mock 位读未预置".into(),
            })
    }

    async fn write_single(&self, slave: u8, addr: u16, value: u16) -> Result<(), BusError> {
        self.write_calls.lock().unwrap().push((slave, addr, value));
        // 与读路径同构：同一 guard 内查 + 删（消费即清），guard 出块即释放，避免重入死锁。
        let to_fail = {
            let mut q = self.write_fail_next.lock().unwrap();
            if let Some(pos) = q.iter().position(|&(s, a)| s == slave && a == addr) {
                q.remove(pos);
                true
            } else {
                false
            }
        };
        if to_fail {
            return Err(BusError::Write {
                slave,
                addr,
                value,
                reason: "mock 写失败".into(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Role, StationConf, StationParity, DEFAULT_BAUD_RATE};

    fn conf(port: &str, protocol: &str) -> StationConf {
        StationConf {
            id: "t1".into(),
            role: Role::Battery,
            port: port.into(),
            protocol: protocol.into(),
            slave: 1,
            baud_rate: DEFAULT_BAUD_RATE,
            parity: StationParity::None,
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
            matches!(
                r,
                Err(BusError::Read {
                    slave: 9,
                    addr: 0x200,
                    count: 2,
                    ..
                })
            ),
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
        let r = bus
            .read_input(2, 0x100, 2)
            .await
            .expect("input 读应返回预置");
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

    // ---------- MockBus FC02（位读） ----------

    /// FC02 预置 → 位向量原样返回；调用记账；与 holding/input 键独立（三个地址空间）。
    #[tokio::test]
    async fn mock_put_bits_then_read_discrete_returns_preset() {
        let bus = MockBus::new();
        bus.put_bits(2, 200, vec![true, false, true]);
        let r = bus.read_discrete(2, 200, 3).await.expect("位读应返回预置");
        assert_eq!(r, vec![true, false, true]);
        assert_eq!(bus.bit_call_count(2, 200), 1);
        // 键独立：同 (slave,addr) 的 holding/input 未预置仍 Err（FC03/FC04/FC02 不同空间）
        assert!(bus.read_holding(2, 200, 3).await.is_err());
        assert!(bus.read_input(2, 200, 3).await.is_err());
        // 反向：holding 预置不影响位读（此处位读已成功，改为断言 calls 互不串台）
        assert_eq!(bus.call_count(2, 200), 1, "holding 记账只记自己那次");
    }

    /// FC02 未预置 → Err（显式失败，不静默给全 0 位）。
    #[tokio::test]
    async fn mock_read_discrete_unpreset_addr_errors() {
        let bus = MockBus::new();
        let r = bus.read_discrete(9, 0x200, 8).await;
        assert!(
            matches!(
                r,
                Err(BusError::Read {
                    slave: 9,
                    addr: 0x200,
                    count: 8,
                    ..
                })
            ),
            "未预置位地址应报 Err，实际 {r:?}"
        );
        assert_eq!(bus.bit_call_count(9, 0x200), 1);
    }

    /// FC02 失败一次后恢复（与 fail_once/fail_input_once 同构，队列消费即清）。
    #[tokio::test]
    async fn mock_bits_fail_once_consumes_then_recovers() {
        let bus = MockBus::new();
        bus.put_bits(2, 200, vec![true]);
        bus.fail_bits_once(2, 200);
        assert!(bus.read_discrete(2, 200, 1).await.is_err());
        assert_eq!(
            bus.read_discrete(2, 200, 1).await.expect("恢复"),
            vec![true]
        );
        assert_eq!(bus.bit_call_count(2, 200), 2);
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

    // ---------- bus_config：parity 透传（S3b-2 T5；纯函数接缝，不触真串口）----------

    /// 站级 `parity` 必须**透传**进 rs485 `Config`：三种取值逐一对映（RC-6 的代码侧前置
    /// —— 现场裁定 `even` 后只改配置即生效，无需改代码）。
    #[test]
    fn bus_config_passes_station_parity_through() {
        let mut c = conf("ttyS4", "modbus");
        assert_eq!(
            bus_config(&c, PortParams::default()).parity,
            rs485_plugin::Parity::None,
            "缺省 none"
        );
        c.parity = StationParity::Even;
        assert_eq!(
            bus_config(&c, PortParams::default()).parity,
            rs485_plugin::Parity::Even
        );
        c.parity = StationParity::Odd;
        assert_eq!(
            bus_config(&c, PortParams::default()).parity,
            rs485_plugin::Parity::Odd
        );
    }

    /// 透传的同时**不得**改动既有透传项（port 归一 / baud_rate / device_addr）。
    #[test]
    fn bus_config_keeps_existing_passthrough_fields() {
        let mut c = conf("ttyS4", "modbus");
        c.baud_rate = 19200;
        c.slave = 7;
        c.parity = StationParity::Even;
        let cfg = bus_config(&c, PortParams::default());
        assert_eq!(cfg.port, "/dev/ttyS4");
        assert_eq!(cfg.baud_rate, 19200);
        assert_eq!(cfg.device_addr, 7);
    }

    // ---------- bus_config：口层控制面三项透传（Task 10 评审项 2）----------

    /// **超时/数据位/停止位必须真透传**（此前是死配置：`south_pcs` 段写了不生效）。
    ///
    /// **改什么会让本条变红**：把 `bus_config` 里三项回填改成不读 `params`（走
    /// `..Config::default()` 兜底）⇒ 本条红（正是评审指出的形态：yaml 写 200ms 实际 1000ms）。
    ///
    /// 判别力：三项各取一个**非默认**值（200 / 7 / 2），任何一个漏透传都会被单独抓住。
    #[test]
    fn bus_config_passes_port_params_through() {
        let c = conf("ttyS4", "modbus");
        let cfg = bus_config(
            &c,
            PortParams {
                timeout_ms: 200,
                data_bits: 7,
                stop_bits: 2,
            },
        );
        assert_eq!(
            cfg.timeout_ms, 200,
            "response_timeout_ms 须落到 Config.timeout_ms"
        );
        assert_eq!(cfg.data_bits, 7, "data_bits 须落到 Config.data_bits");
        assert_eq!(cfg.stop_bits, 2, "stop_bits 须落到 Config.stop_bits");

        // 站级路径等价性锚：默认参数 == `rs485_plugin::config::Config::default()` 的同三项
        //（`open(conf)` 走的就是这一档 ⇒ 站级行为逐字段不变）
        let d = bus_config(&c, PortParams::default());
        let rd = rs485_plugin::config::Config::default();
        assert_eq!(
            (d.timeout_ms, d.data_bits, d.stop_bits),
            (rd.timeout_ms, rd.data_bits, rd.stop_bits),
            "PortParams::default() 必须等于 rs485 默认三项（否则站级路径语义被改）"
        );
    }

    /// `open_with_port_params` 对越界控制面参数 **fail-closed**。
    ///
    /// 理由：rs485 侧 `configure_port` 对越界 `data_bits`/`stop_bits` 是 `_ =>` 静默落到 8/1
    /// ⇒ 不拦就是"配错了却不报"。故三态各拦一条。
    #[test]
    fn open_with_port_params_rejects_out_of_range_bits() {
        let c = conf("southd_ut_no_such_tty", "modbus");
        let bad = [
            PortParams {
                timeout_ms: 200,
                data_bits: 9,
                stop_bits: 1,
            },
            PortParams {
                timeout_ms: 200,
                data_bits: 4,
                stop_bits: 1,
            },
            PortParams {
                timeout_ms: 200,
                data_bits: 8,
                stop_bits: 3,
            },
            PortParams {
                timeout_ms: 0,
                data_bits: 8,
                stop_bits: 1,
            },
        ];
        for pp in bad {
            assert!(
                matches!(
                    Rs485PortBus::open_with_port_params(&c, pp),
                    Err(BusError::Open(..))
                ),
                "越界控制面参数须 Open Err（fail-closed），实得 Ok：{pp:?}"
            );
        }
    }
}

#[cfg(test)]
mod write_single_tests {
    use super::*;

    #[tokio::test]
    async fn mock_bus_write_single_records_and_returns_ok() {
        // ⚠️ 必须包含一次 **非零 value**：只写 value=0 时"第三元是否真透传"不具判别力
        //（把 `value` 换成硬编码 `0` 仍会全绿）—— 而下游 PcsHandle 的测试要靠
        // `write_calls` 验证控制序列写了哪些**值**（如 500=0 停机、1001=P 设定）。
        let bus = MockBus::new();
        bus.write_single(3, 500, 0).await.unwrap();
        bus.write_single(3, 500, 7).await.unwrap();
        assert_eq!(
            bus.write_calls.lock().unwrap().clone(),
            vec![(3u8, 500u16, 0u16), (3u8, 500u16, 7u16)],
            "记录必须按序累积，且 value 必须真透传（非零值可判别）"
        );
    }

    #[tokio::test]
    async fn mock_bus_write_single_can_fail_once() {
        let bus = MockBus::new();
        bus.fail_write_once(3, 500);
        assert!(bus.write_single(3, 500, 0).await.is_err());
        // 消费即清：第二次成功
        assert!(bus.write_single(3, 500, 0).await.is_ok());
    }
}
