//! RS485 设备驱动实现
//!
//! 提供 RS485 设备通信能力，支持 Modbus RTU 协议

use crate::config::Config;
use crate::errors::Rs485Error;
use crate::protocol::Frame;
#[allow(unused_imports)]
use device_trait::Parity;
use device_trait::{
    CrcMode, DataFrame, Device, DeviceError, DeviceStatus, ProtocolHandler, SouthDevice,
};
use parking_lot::Mutex;
#[cfg(unix)]
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

/// RS485 方向控制
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rs485Dir {
    /// 接收模式
    Recv,
    /// 发送模式
    Send,
}

/// 测试交换缝的闭包类型（抽具名别名以避免 `clippy::type_complexity`；
/// 与 `strategy-engine/src/ai_integration.rs` 同款做法，调用处签名同样受益）。
#[cfg(test)]
type TestExchangeFn = Box<dyn Fn(&[u8]) -> Vec<u8> + Send + Sync>;

/// RS485 设备驱动
///
/// 实现南向 RS485 设备通信，支持 TTU、光伏逆变器、充电桩等设备
pub struct Rs485Device {
    /// 设备ID
    device_id: String,
    /// 设备类型
    device_type: String,
    /// 配置
    config: Config,
    /// 协议处理器（依赖注入）
    handler: Arc<dyn ProtocolHandler>,
    /// 串口文件描述符
    port_fd: Mutex<Option<RawFd>>,
    /// 状态
    status: Mutex<DeviceStatus>,
    /// 是否已打开
    opened: AtomicBool,
    /// 发送锁（保证事务原子性）
    tx_lock: StdMutex<()>,
    /// **测试专用**响应注入缝：非 `None` 时 [`Rs485Device::send_recv`] 直接返回该字节流，
    /// 使「请求帧 → 响应 → 帧级校验」整条链可在无真实串口（Windows/CI）下被断言。
    ///
    /// `#[cfg(test)]` ⇒ 产线构建**不含此字段**、`send_recv` 也不含对应分支，语义零影响。
    #[cfg(test)]
    test_response: Mutex<Option<Vec<u8>>>,
    /// **测试专用**交换缝（与 `test_response` 的差别见 `send_recv` 注释）：拿到**请求帧原文**，
    /// 返回**响应帧原文**。`#[cfg(test)]` ⇒ 产线构建不含此字段与分支。
    #[cfg(test)]
    test_exchange: Mutex<Option<TestExchangeFn>>,
}

/// 平台无关的文件描述符类型
#[cfg(unix)]
type RawFd = std::os::unix::io::RawFd;
#[cfg(windows)]
type RawFd = i32;

/// 构建 Modbus 读寄存器请求帧（FC03/FC04 等读功能码通用）。
///
/// 帧格式：[slave, func, addr_hi, addr_lo, count_hi, count_lo, crc_lo, crc_hi]
/// - `slave`: 显式从站地址（支持同口多从站：口内串行轮询各 slave）
/// - `func`: 功能码（0x03 保持寄存器 / 0x04 输入寄存器）
/// - `crc_mode`: 取自 `Config.crc_mode`，保证与原实现逐字节一致
///
/// 纯函数、无 IO，便于单元测试。
fn build_read_frame(slave: u8, func: u8, addr: u16, count: u16, crc_mode: CrcMode) -> Vec<u8> {
    let mut cmd = vec![
        slave,
        func,
        (addr >> 8) as u8,
        addr as u8,
        (count >> 8) as u8,
        count as u8,
    ];
    let crc = Frame::calculate_crc(slave, func, &cmd[2..], crc_mode);
    cmd.push(crc as u8);
    cmd.push((crc >> 8) as u8);
    cmd
}

/// 期望从站号 = **请求帧首字节**（审查 W1 的结构性保证）。
///
/// 请求帧由 [`build_read_frame`] 构造，其首字节恒为 `slave`（已有独立单测
/// `test_build_read_frame_*` 钉住）⇒ "请求谁 → 就校验谁" 由**构造关系**决定，
/// 而不是调用点"记得传对参数"：
/// 请求级读路径不再向 [`parse_regs_response`] / [`parse_bits_response`] 另传 `slave`，
/// 而是把请求帧本身交进来取值 —— 传错/漏传在语法上无从发生。
///
/// 用 `first()` 而非 `cmd[0]`：空帧返回 `Err` 而非 panic（同一提交内 O1 的口径）。
fn expected_slave_of(cmd: &[u8]) -> Result<u8, Rs485Error> {
    cmd.first()
        .copied()
        .ok_or_else(|| Rs485Error::ConfigFailed("请求帧为空，无法确定期望从站号".to_string()))
}

/// 帧级校验：请求级读路径（FC03/FC04/FC02）响应解析的**唯一校验入口**。
///
/// 口径对齐设计 §3.4.1「CRC 验证：**严格校验，地址+数据不匹配则拒绝**」与既有
/// [`crate::protocol::Frame::parse`] / `ModbusHandler::decode_response`：
/// 1. **最小帧长与 CRC16** —— 复用 [`Frame::parse`]（CRC 复用同一实现，不另写一套）；
/// 2. **响应从站号 == 请求从站号** —— 防同口多从站时他站帧被当本站数据（S3a 请求级
///    读路径的关键风险面：口内轮询多 slave，串扰帧在旧路径被无条件接受）；
/// 3. **Modbus 异常帧**（`func & 0x80 != 0`）—— 按 PRD §9.7.1 语义直接拒绝，异常码
///    入错误报文，**绝不静默当数据**。
///
/// 错误沿既有 `Result` 上抛；上层 southd 已按「单块读超时 / CRC 错 / 异常码 → 该站本轮
/// 失败 → `offline_count + 1`」（PRD §9.7.1）处理，本函数不触碰上层语义。
fn validate_read_response(response: &[u8], slave: u8, crc_mode: CrcMode) -> Result<(), Rs485Error> {
    // ① 长度（≥5）+ ② CRC16：Frame::parse 已含二者，且与写路径/旧 handler 路径同源。
    let frame = Frame::parse(response, crc_mode)?;

    // ③ 从站号：请求谁就只认谁（同口多从站语义的必要防线）。
    if frame.addr != slave {
        return Err(Rs485Error::ConfigFailed(format!(
            "响应从站号不匹配：请求 slave={slave}，响应 slave={}（他站帧不得当本站数据）",
            frame.addr
        )));
    }

    // ④ 异常帧：func 最高位置 1 ⇒ [slave, func|0x80, 异常码, crc_lo, crc_hi]。
    if frame.func_code & 0x80 != 0 {
        let code = response.get(2).copied().unwrap_or(0);
        return Err(Rs485Error::ConfigFailed(format!(
            "Modbus 异常响应：func=0x{:02X}，异常码=0x{code:02X}（{}）",
            frame.func_code,
            modbus_exception_desc(code)
        )));
    }

    Ok(())
}

/// Modbus 标准异常码 → 中文描述（仅用于错误报文可读性，**不参与判定**）。
fn modbus_exception_desc(code: u8) -> &'static str {
    match code {
        0x01 => "非法功能",
        0x02 => "非法数据地址",
        0x03 => "非法数据值",
        0x04 => "从站设备故障",
        0x05 => "确认（需要长时间处理）",
        0x06 => "从站设备忙",
        0x08 => "存储奇偶性差错",
        0x0A => "网关路径不可用",
        0x0B => "网关目标设备响应失败",
        _ => "未知异常码",
    }
}

/// 解析 Modbus 读寄存器响应（FC03/FC04 通用）。
///
/// 响应格式：[slave, func, byte_count, reg(大端 2 字节)..., crc_lo, crc_hi]
/// 先经 [`validate_read_response`] 做帧级严格校验（长度/CRC/从站号/异常帧），
/// 再做长度校验与寄存器拆解（大端 u16）。
/// 纯函数、无 IO，便于单元测试。
fn parse_regs_response(
    response: &[u8],
    slave: u8,
    crc_mode: CrcMode,
) -> Result<Vec<u16>, Rs485Error> {
    validate_read_response(response, slave, crc_mode)?;

    // 安全取值（审查 O1）：不裸索引 `response[2]` —— 其安全性此前完全依赖 `validate_read_response`
    // 先保 `len ≥ 5`，一旦该前置被短路/重排即 panic；而南向采集 task 内 panic 会**静默终止整口
    // 采集**（不是干净 Err）。此处显式取 `get(2)` ⇒ 失败形态统一为 Err。
    let byte_count = response.get(2).copied().ok_or_else(|| {
        Rs485Error::ConfigFailed("响应帧不足 3 字节，缺少 byte_count 字段".to_string())
    })? as usize;
    if response.len() < 3 + byte_count + 2 {
        return Err(Rs485Error::ConfigFailed("响应数据不完整".to_string()));
    }

    let mut registers = Vec::new();
    // 确保 byte_count 为偶数（每个寄存器 2 字节），并检查边界
    let register_count = byte_count / 2;
    for i in 0..register_count {
        let idx = 3 + i * 2;
        if idx + 1 >= response.len() {
            return Err(Rs485Error::ConfigFailed("响应数据不完整".to_string()));
        }
        let value = ((response[idx] as u16) << 8) | (response[idx + 1] as u16);
        registers.push(value);
    }

    Ok(registers)
}

/// Modbus FC02 响应字节 → 位向量（PRD §9.7.4 的唯一解包公式）。
///
/// 位 `k`（`0 ≤ k < count`）取自 `bytes[k / 8]` 的第 `(k % 8)` 位（**bit0 = LSB**），
/// 即协议规定的"较低地址的寄存器存储在一个字节的较低位上"。返回长度**恒为 `count`**。
///
/// - `count` 非 8 倍数时，**末字节高位为无关位**，既不参与解包也不污染前 `count` 位
///   （设计 §11.2.3 选型 C1 —— 这正是"复用 `Vec<u16>`"被否掉的原因）。
/// - 字节不足时按 0 补齐（长度契约优先于静默截断）；"响应字节数不足"由帧层
///   [`parse_bits_response`] 拒绝，本纯函数保持全定义：不 panic、不越界。
pub fn unpack_bits(bytes: &[u8], count: u16) -> Vec<bool> {
    (0..count as usize)
        .map(|k| {
            let byte = bytes.get(k / 8).copied().unwrap_or(0);
            (byte >> (k % 8)) & 1 == 1
        })
        .collect()
}

/// 解析 Modbus FC02 离散输入响应。
///
/// 响应格式：[slave, func, byte_count, data..., crc_lo, crc_hi]
/// 与 [`parse_regs_response`] 同口径：先经 [`validate_read_response`] 帧级严格校验
/// （长度/CRC/从站号/异常帧），再做长度校验与解包。
///
/// **刻意不复用 `parse_regs_response`**：后者按 `byte_count / 2` 拆寄存器，
/// 会丢掉非偶数字节的末字节（位宽 1..8 时响应只有 1 个数据字节）。
fn parse_bits_response(
    response: &[u8],
    slave: u8,
    count: u16,
    crc_mode: CrcMode,
) -> Result<Vec<bool>, Rs485Error> {
    validate_read_response(response, slave, crc_mode)?;

    // 同 [`parse_regs_response`]：`get(2)` 安全取值，杜绝"前置被短路 ⇒ 口内 panic"（审查 O1）。
    let byte_count = response.get(2).copied().ok_or_else(|| {
        Rs485Error::ConfigFailed("响应帧不足 3 字节，缺少 byte_count 字段".to_string())
    })? as usize;
    if response.len() < 3 + byte_count + 2 {
        return Err(Rs485Error::ConfigFailed("响应数据不完整".to_string()));
    }

    let needed = (count as usize).div_ceil(8);
    if byte_count < needed {
        return Err(Rs485Error::ConfigFailed(format!(
            "位块响应字节数不足：count={count} 需 {needed} 字节，实得 {byte_count}"
        )));
    }

    Ok(unpack_bits(&response[3..3 + byte_count], count))
}

impl Rs485Device {
    /// 创建新的 RS485 设备
    pub fn new(
        device_id: String,
        device_type: String,
        config: Config,
        handler: Arc<dyn ProtocolHandler>,
    ) -> Self {
        Self {
            device_id,
            device_type,
            config,
            handler,
            port_fd: Mutex::new(None),
            status: Mutex::new(DeviceStatus::Offline),
            opened: AtomicBool::new(false),
            tx_lock: StdMutex::new(()),
            #[cfg(test)]
            test_response: Mutex::new(None),
            #[cfg(test)]
            test_exchange: Mutex::new(None),
        }
    }

    /// 打开串口
    ///
    /// # Returns
    /// - `Ok(())`: 打开成功
    /// - `Err(Rs485Error)`: 打开失败
    pub fn open(&self) -> Result<(), Rs485Error> {
        #[cfg(unix)]
        {
            use std::ffi::CString;

            let port_path = self.config.port.clone();
            let c_path = CString::new(port_path.as_str())
                .map_err(|_| Rs485Error::config_failed("无效的端口路径"))?;

            // 打开串口
            let fd = unsafe {
                libc::open(
                    c_path.as_ptr(),
                    libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK,
                )
            };

            if fd < 0 {
                return Err(Rs485Error::open_failed(&self.config.port));
            }

            // 配置串口参数；失败时关闭 fd 防止泄漏
            if let Err(e) = self.configure_port(fd) {
                unsafe {
                    libc::close(fd);
                }
                return Err(e);
            }

            // 清除非阻塞标志（P0-1 根因，2026-09-09 项目审查）：
            // open 用 O_NONBLOCK 仅防 open 本身因载波等待阻塞；若保持非阻塞，则
            // recv_frame 的 VMIN/VTIME 阻塞弱超时读不生效——read 无数据即返回 EAGAIN，
            // 任何 Modbus 真从站请求-响应恒超时（南向真机采集失败根因）。
            // configure_port 已置 CLOCAL|CREAD，read 回到阻塞语义后由 termios
            // VMIN=0/VTIME 控制读超时。
            let fl = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if fl >= 0 {
                let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, fl & !libc::O_NONBLOCK) };
            }

            *self.port_fd.lock() = Some(fd);
            self.opened.store(true, Ordering::SeqCst);
            *self.status.lock() = DeviceStatus::Online;

            Ok(())
        }

        #[cfg(not(unix))]
        {
            // Windows 平台预留实现
            Err(Rs485Error::config_failed("Windows 平台暂不支持串口打开"))
        }
    }

    /// 配置串口参数
    #[cfg(unix)]
    fn configure_port(&self, fd: RawFd) -> Result<(), Rs485Error> {
        // 获取终端属性
        let mut termios: MaybeUninit<libc::termios> = MaybeUninit::uninit();
        let mut termios = unsafe {
            if libc::tcgetattr(fd, termios.as_mut_ptr()) < 0 {
                return Err(Rs485Error::config_failed("获取终端属性失败"));
            }
            termios.assume_init()
        };

        // 设置波特率
        let baud_rate = self.config.baud_rate;
        let speed = match baud_rate {
            9600 => libc::B9600,
            19200 => libc::B19200,
            38400 => libc::B38400,
            115200 => libc::B115200,
            _ => libc::B9600,
        };

        unsafe { libc::cfsetispeed(&mut termios, speed) };
        unsafe { libc::cfsetospeed(&mut termios, speed) };

        // 设置数据位
        termios.c_cflag &= !libc::CSIZE;
        match self.config.data_bits {
            5 => termios.c_cflag |= libc::CS5,
            6 => termios.c_cflag |= libc::CS6,
            7 => termios.c_cflag |= libc::CS7,
            _ => termios.c_cflag |= libc::CS8,
        }

        // 设置校验位
        match self.config.parity {
            Parity::None => {
                termios.c_cflag &= !libc::PARENB;
            }
            Parity::Even => {
                termios.c_cflag |= libc::PARENB;
                termios.c_cflag &= !libc::PARODD;
            }
            Parity::Odd => {
                termios.c_cflag |= libc::PARENB;
                termios.c_cflag |= libc::PARODD;
            }
        }

        // 设置停止位
        match self.config.stop_bits {
            2 => termios.c_cflag |= libc::CSTOPB,
            _ => termios.c_cflag &= !libc::CSTOPB,
        }

        // 启用接收和本地模式
        termios.c_cflag |= libc::CLOCAL | libc::CREAD;

        // 设置超时
        termios.c_cc[libc::VTIME] = (self.config.timeout_ms / 100) as u8;
        termios.c_cc[libc::VMIN] = 0;

        // 应用设置
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) } < 0 {
            return Err(Rs485Error::config_failed("设置终端属性失败"));
        }

        // 刷新缓冲区
        unsafe { libc::tcflush(fd, libc::TCIFLUSH) };

        Ok(())
    }

    /// 关闭串口
    pub fn close(&self) {
        let mut port_guard = self.port_fd.lock();
        if let Some(fd) = port_guard.take() {
            #[cfg(unix)]
            {
                if unsafe { libc::close(fd) } < 0 {
                    tracing::error!(
                        "关闭串口文件描述符 {} 失败: {}",
                        fd,
                        std::io::Error::last_os_error()
                    );
                }
            }
        }
        self.opened.store(false, Ordering::SeqCst);
        *self.status.lock() = DeviceStatus::Offline;
    }

    /// 检查串口是否已打开
    pub fn is_open(&self) -> bool {
        self.opened.load(Ordering::SeqCst)
    }

    /// 发送原始数据帧
    ///
    /// # Arguments
    /// - `frame`: 要发送的数据帧
    ///
    /// # Returns
    /// - `Ok(())`: 发送成功
    /// - `Err(Rs485Error)`: 发送失败
    pub fn send_frame(&self, frame: &[u8]) -> Result<(), Rs485Error> {
        let port_guard = self.port_fd.lock();
        let fd = port_guard.ok_or_else(|| Rs485Error::NotConnected(self.device_id.clone()))?;

        #[cfg(unix)]
        {
            // 清空收发缓冲区（TCIOFLUSH），防止残留数据破坏帧结构
            unsafe { libc::tcflush(fd, libc::TCIOFLUSH) };

            let mut written = 0usize;
            while written < frame.len() {
                let result = unsafe {
                    libc::write(
                        fd,
                        frame.as_ptr().add(written) as *const libc::c_void,
                        frame.len() - written,
                    )
                };
                if result < 0 {
                    return Err(Rs485Error::send_failed("发送失败"));
                }
                written += result as usize;
            }
            Ok(())
        }

        #[cfg(not(unix))]
        {
            Err(Rs485Error::send_failed("Windows 平台暂不支持"))
        }
    }

    /// 接收原始数据帧
    ///
    /// # Arguments
    /// - `timeout_ms`: 超时时间（毫秒）
    ///
    /// # Returns
    /// - `Ok(Vec<u8>)`: 接收到的数据
    /// - `Err(Rs485Error)`: 接收失败
    pub fn recv_frame(&self, timeout_ms: u64) -> Result<Vec<u8>, Rs485Error> {
        let port_guard = self.port_fd.lock();
        let fd = port_guard.ok_or_else(|| Rs485Error::NotConnected(self.device_id.clone()))?;

        #[cfg(unix)]
        {
            let mut termios: MaybeUninit<libc::termios> = MaybeUninit::uninit();
            let mut termios = unsafe {
                if libc::tcgetattr(fd, termios.as_mut_ptr()) < 0 {
                    return Err(Rs485Error::recv_failed("获取终端属性失败"));
                }
                termios.assume_init()
            };
            let original_termios = termios;

            // 设置读取超时：VTIME 为十分之一秒
            termios.c_cc[libc::VTIME] = (timeout_ms / 100) as u8;
            termios.c_cc[libc::VMIN] = 0;

            if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) } < 0 {
                return Err(Rs485Error::recv_failed("设置终端属性失败"));
            }

            let mut buffer = vec![0u8; 1024];
            let n =
                unsafe { libc::read(fd, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len()) };

            // 恢复原始终端属性
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &original_termios) };

            if n < 0 {
                return Err(Rs485Error::recv_failed("接收失败或超时"));
            }

            Ok(buffer[..n as usize].to_vec())
        }

        #[cfg(not(unix))]
        {
            Err(Rs485Error::recv_failed("Windows 平台暂不支持"))
        }
    }

    /// 设置 RS485 方向
    ///
    /// # Arguments
    /// - `dir`: 方向（发送/接收）
    fn set_dir(&self, dir: Rs485Dir) -> Result<(), Rs485Error> {
        let gpio_num = match dir {
            Rs485Dir::Send => self.config.de_gpio,
            Rs485Dir::Recv => self.config.re_gpio,
        };

        if let Some(gpio) = gpio_num {
            gpio_set_value(gpio, dir == Rs485Dir::Send)?;
        }
        Ok(())
    }

    /// 事务：发送并接收（原子操作）
    ///
    /// 使用锁保证读-写-读的原子性
    ///
    /// # Arguments
    /// - `request`: 请求数据
    /// - `recv_timeout_ms`: 接收超时（毫秒）
    ///
    /// # Returns
    /// - `Ok(DataFrame)`: 接收到的数据帧
    /// - `Err(Rs485Error)`: 操作失败
    pub fn transaction(
        &self,
        request: &[u8],
        recv_timeout_ms: u64,
    ) -> Result<DataFrame, Rs485Error> {
        let _guard = self.tx_lock.lock();

        // 1. 切换到发送模式
        self.set_dir(Rs485Dir::Send)?;

        // 2. 发送请求
        self.send_frame(request)?;

        // 3. 切换到接收模式
        self.set_dir(Rs485Dir::Recv)?;

        // 4. 接收响应
        let data = self.recv_frame(recv_timeout_ms)?;
        Ok(DataFrame::new(self.device_id.clone(), data))
    }

    /// 使用协议处理器的编码/解码事务
    ///
    /// 将协议编码/解码委托给注入的 ProtocolHandler，支持任意协议类型
    ///
    /// # Arguments
    /// - `data`: 原始数据载荷
    /// - `recv_timeout_ms`: 接收超时（毫秒）
    ///
    /// # Returns
    /// - `Ok(DataFrame)`: 解码后的数据帧
    /// - `Err(Rs485Error)`: 操作失败
    pub fn transaction_with_handler(
        &self,
        data: &[u8],
        recv_timeout_ms: u64,
    ) -> Result<DataFrame, Rs485Error> {
        let _guard = self.tx_lock.lock();

        // 1. 使用 handler 编码请求
        let frame = self.handler.encode_request(&self.device_id, data);

        // 2. 切换到发送模式
        self.set_dir(Rs485Dir::Send)?;

        // 3. 发送请求
        self.send_frame(&frame)?;

        // 4. 切换到接收模式
        self.set_dir(Rs485Dir::Recv)?;

        // 5. 接收响应
        let response = self.recv_frame(recv_timeout_ms)?;

        // 6. 使用 handler 解码响应
        self.handler
            .decode_response(&response)
            .map_err(|e| Rs485Error::ConfigFailed(e.to_string()))
    }

    /// 获取当前协议处理器名称
    pub fn handler_name(&self) -> &'static str {
        self.handler.name()
    }

    /// 装测试交换缝（仅测试构建存在）。设置后 [`Self::send_recv`] 走本缝，**优先于**
    /// [`Self::test_response`]；`clear_test_exchange` 可撤销（每条 e2e 结束必须清，防串扰）。
    #[cfg(test)]
    pub fn set_test_exchange(&self, f: TestExchangeFn) {
        *self.test_exchange.lock() = Some(f);
    }

    /// 清测试交换缝。
    #[cfg(test)]
    pub fn clear_test_exchange(&self) {
        *self.test_exchange.lock() = None;
    }

    /// 发送并接收数据。
    ///
    /// 测试构建下两条零 IO 捷径（产线构建均不存在）：
    /// 1. `test_exchange`：拿请求帧原文、返回响应帧原文 —— 使「成帧 → 线路 → 解析」整链
    ///    在无串口环境可被断言（PCS 帧级 e2e 用，设计 §13.6 R-3）。
    /// 2. `test_response`：直接返回整段响应字节（既有缝，帧级校验用例用）。
    ///
    /// 顺序固定为 1 → 2 → 真 IO：缝未设置时行为与改动前**逐字节相同**。
    pub fn send_recv(&self, frame: &[u8], recv_timeout_ms: u64) -> Result<Vec<u8>, Rs485Error> {
        #[cfg(test)]
        {
            if let Some(f) = self.test_exchange.lock().as_ref() {
                return Ok(f(frame));
            }
            if let Some(injected) = self.test_response.lock().clone() {
                return Ok(injected);
            }
        }
        self.send_frame(frame)?;
        self.recv_frame(recv_timeout_ms)
    }

    /// 发送光伏限功率命令
    ///
    /// # Arguments
    /// - `limit_ratio`: 限功率比例 [0.0, 1.0]，0.0=完全限功率，1.0=不限功率
    /// - `recv_timeout_ms`: 接收超时
    ///
    /// # Returns
    /// - `Ok(response)`: 设备响应数据
    /// - `Err`: 发送失败
    pub fn send_pv_limit(
        &self,
        limit_ratio: f64,
        recv_timeout_ms: u64,
    ) -> Result<Vec<u8>, Rs485Error> {
        // 编码限功率命令数据
        // 格式: [功能码=0x10, 功率高字节, 功率低字节]
        // 注意：实际协议格式取决于具体逆变器厂商
        let limit_ratio = limit_ratio.clamp(0.0, 1.0);
        let power_value = (limit_ratio * 1000.0) as u16; // 缩放到 0-1000 范围
        let data = vec![0x10, (power_value >> 8) as u8, power_value as u8];

        tracing::info!(
            "发送光伏限功率: device_id={}, limit_ratio={:.2} (raw={})",
            self.device_id,
            limit_ratio,
            power_value
        );

        let frame = self.handler.encode_request(&self.device_id, &data);
        self.send_recv(&frame, recv_timeout_ms)
    }

    /// 发送负荷切除命令
    ///
    /// # Arguments
    /// - `power_kw`: 切除功率 (kW)
    /// - `recv_timeout_ms`: 接收超时
    ///
    /// # Returns
    /// - `Ok(response)`: 设备响应数据
    /// - `Err`: 发送失败
    pub fn send_load_shedding(
        &self,
        power_kw: f64,
        recv_timeout_ms: u64,
    ) -> Result<Vec<u8>, Rs485Error> {
        // 编码负荷切除命令数据
        // 格式: [功能码=0x11, 功率高字节, 功率低字节]
        // 注意：实际协议格式取决于具体负荷控制装置厂商
        let power_kw = power_kw.max(0.0);
        let power_value = (power_kw * 10.0) as u16; // 缩放到 0.1kW 分辨率
        let data = vec![0x11, (power_value >> 8) as u8, power_value as u8];

        tracing::info!(
            "发送负荷切除: device_id={}, power_kw={:.1}",
            self.device_id,
            power_kw
        );

        let frame = self.handler.encode_request(&self.device_id, &data);
        self.send_recv(&frame, recv_timeout_ms)
    }

    /// 读取保持寄存器（Modbus 功能码 0x03），从站地址取 `config.device_addr`。
    ///
    /// # Arguments
    /// - `addr`: 起始寄存器地址
    /// - `count`: 寄存器数量
    ///
    /// # Returns
    /// - `Ok(Vec<u16>)`: 寄存器值列表
    pub fn read_holding_registers(&self, addr: u16, count: u16) -> Result<Vec<u16>, Rs485Error> {
        self.read_holding_registers_from(self.config.device_addr, addr, count)
    }

    /// 读取保持寄存器（Modbus 0x03），显式从站地址（同口多从站，口内串行轮询）。
    pub fn read_holding_registers_from(
        &self,
        slave: u8,
        addr: u16,
        count: u16,
    ) -> Result<Vec<u16>, Rs485Error> {
        self.read_regs(build_read_frame(
            slave,
            0x03,
            addr,
            count,
            self.config.crc_mode,
        ))
    }

    /// 读取输入寄存器（Modbus 0x04），显式从站地址（同口多从站，口内串行轮询）。
    pub fn read_input_registers_from(
        &self,
        slave: u8,
        addr: u16,
        count: u16,
    ) -> Result<Vec<u16>, Rs485Error> {
        self.read_regs(build_read_frame(
            slave,
            0x04,
            addr,
            count,
            self.config.crc_mode,
        ))
    }

    /// 读取离散输入（Modbus **FC02**），显式从站地址（同口多从站，口内串行轮询）。
    ///
    /// `count` = **位数**（非寄存器数）；返回长度 = `count` 的位向量。
    /// 与 [`Self::read_holding_registers_from`] / [`Self::read_input_registers_from`] 同构：
    /// 复用 [`build_read_frame`] 与同一条 `send_recv` 事务路径，仅功能码与响应解析不同。
    pub fn read_discrete_inputs_from(
        &self,
        slave: u8,
        addr: u16,
        count: u16,
    ) -> Result<Vec<bool>, Rs485Error> {
        let cmd = build_read_frame(slave, 0x02, addr, count, self.config.crc_mode);
        let response = self.send_recv(&cmd, self.config.timeout_ms)?;
        parse_bits_response(
            &response,
            expected_slave_of(&cmd)?,
            count,
            self.config.crc_mode,
        )
    }

    /// 私有：发送读请求帧并解析响应寄存器。
    ///
    /// 期望从站号**取自请求帧首字节**（[`expected_slave_of`]），不再由调用方另传 ——
    /// "请求谁就校验谁" 是构造关系而非调用点约定（审查 W1）。
    fn read_regs(&self, cmd: Vec<u8>) -> Result<Vec<u16>, Rs485Error> {
        let response = self.send_recv(&cmd, self.config.timeout_ms)?;
        parse_regs_response(&response, expected_slave_of(&cmd)?, self.config.crc_mode)
    }

    /// 写入单个寄存器（Modbus 功能码 0x06），用 `config.device_addr` 作从站。
    pub fn write_single_register(&self, addr: u16, value: u16) -> Result<(), Rs485Error> {
        self.write_single_register_from(self.config.device_addr, addr, value)
    }

    /// 写入单个寄存器（FC06），**显式从站地址**（同口多从站；与读侧 `_from` 家族对称）。
    ///
    /// 与旧实现（只判 `len < 8`）的差别：本函数**校验响应回显** —— 从站号、功能码、
    /// 地址、值四项必须与请求逐字一致。停机写 `REG_START_STOP=0` 属**安全动作**，
    /// "发出去了但被别的从站/错帧应答"必须能被检出。
    /// 回显不符按 `Rs485Error::ConfigFailed` 报出，报文中含两侧从站号便于定位。
    pub fn write_single_register_from(
        &self,
        slave: u8,
        addr: u16,
        value: u16,
    ) -> Result<(), Rs485Error> {
        const FUNC: u8 = 0x06;
        let mut cmd = vec![
            slave,
            FUNC,
            (addr >> 8) as u8,
            addr as u8,
            (value >> 8) as u8,
            value as u8,
        ];
        let crc = Frame::calculate_crc(slave, FUNC, &cmd[2..], self.config.crc_mode);
        cmd.push(crc as u8);
        cmd.push((crc >> 8) as u8);

        let response = self.send_recv(&cmd, self.config.timeout_ms)?;

        // 期望从站号取自**请求帧首字节**（与读侧 `expected_slave_of` 同一取向：
        // "请求谁就校验谁"是构造关系，不是调用点约定）。
        let expected_slave = expected_slave_of(&cmd)?;

        // Modbus 异常响应必须**先于长度检查**：标准 FC06 异常帧仅 **5 字节**
        // （slave + func|0x80 + 异常码 + CRC16[2]）。若先判 `len < 8`，真实异常会被
        // 误报成"响应过短"，异常码永远看不到 —— 运维会去查线缆/成帧而不是"从站为何拒绝"。
        // 读侧 `validate_read_response` 走 `Frame::parse`（接受 ≥5 字节）能正确报异常，
        // 本处与读侧对齐。
        // 取值一律走 `get()`（本文件 device.rs:126/170/227 已登记的"不裸索引"约定）：
        // 旧写法把"长度 ≥3"与"下标 1/0/2 的顺序"耦合在一起，重排/短路即 panic（本 Task 已咬过一次）。
        // 异常帧成立的**最小**条件是"func|0x80 与异常码两字节都在"（标准异常帧 5 字节）：
        // 只判 func 位会把 2 字节残帧/噪声凭空说成"被从站拒绝，异常码=0x00（未知异常码）"——
        // 把"没收到成帧"误诊成"从站拒绝"（安全动作 500=0 的现场诊断方向完全不同）。
        // 故用两个 `get()` 同时表达"字段在不在"与"值对不对"，不足则落下方长度检查报"过短"。
        if matches!(
            (response.get(1), response.get(2)),
            (Some(&f), Some(_)) if f == (FUNC | 0x80)
        ) {
            let code = response.get(2).copied().unwrap_or(0);
            return Err(Rs485Error::ConfigFailed(format!(
                "写 reg {addr:#06x}（请求 slave={expected_slave}）被从站 {} 拒绝，异常码={code:#04x}（{}）",
                response.first().copied().unwrap_or(0),
                modbus_exception_desc(code)
            )));
        }
        if response.len() < 8 {
            return Err(Rs485Error::ConfigFailed(format!(
                "写响应过短：{} 字节（FC06 回显应为 8）",
                response.len()
            )));
        }
        if response[0] != expected_slave {
            return Err(Rs485Error::ConfigFailed(format!(
                "写 reg {addr:#06x}：响应从站号 slave={} 与请求 slave={expected_slave} 不符（他站帧）",
                response[0]
            )));
        }
        if response[1] != FUNC {
            return Err(Rs485Error::ConfigFailed(format!(
                "写 reg {addr:#06x}：响应功能码 {:#04x} 与请求 {FUNC:#04x} 不符",
                response[1]
            )));
        }
        let echo_addr = u16::from_be_bytes([response[2], response[3]]);
        let echo_value = u16::from_be_bytes([response[4], response[5]]);
        if echo_addr != addr || echo_value != value {
            return Err(Rs485Error::ConfigFailed(format!(
                "写 reg {addr:#06x}={value:#06x}：回显为 {echo_addr:#06x}={echo_value:#06x}，与请求不符"
            )));
        }
        Ok(())
    }
}

impl Device for Rs485Device {
    fn read(&self) -> Result<DataFrame, DeviceError> {
        if !self.is_open() {
            return Err(DeviceError::offline(&self.device_id));
        }

        // 根据协议处理器类型选择读路径
        match self.handler.name() {
            "ModbusRTU" => {
                // 读取设备数据（示例：读取前 10 个保持寄存器）
                let registers = self
                    .read_holding_registers(0, 10)
                    .map_err(|e| DeviceError::Other(e.to_string()))?;

                let data = registers
                    .iter()
                    .flat_map(|r| vec![(*r >> 8) as u8, *r as u8])
                    .collect();

                Ok(DataFrame::new(self.device_id.clone(), data))
            }
            _ => {
                // 使用 handler 进行协议编解码
                self.transaction_with_handler(&[], self.config.timeout_ms)
                    .map_err(|e| DeviceError::Other(e.to_string()))
            }
        }
    }

    fn write(&self, data: &[u8]) -> Result<(), DeviceError> {
        if !self.is_open() {
            return Err(DeviceError::offline(&self.device_id));
        }

        let mut cmd = vec![self.config.device_addr];
        cmd.extend_from_slice(data);

        let func_code = cmd[1];
        let crc = Frame::calculate_crc(
            self.config.device_addr,
            func_code,
            &cmd[2..],
            self.config.crc_mode,
        );
        cmd.push(crc as u8);
        cmd.push((crc >> 8) as u8);

        self.send_frame(&cmd)
            .map_err(|e| DeviceError::Other(e.to_string()))
    }

    fn status(&self) -> Result<DeviceStatus, DeviceError> {
        Ok(self.status.lock().clone())
    }

    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn device_type(&self) -> &str {
        &self.device_type
    }
}

impl SouthDevice for Rs485Device {
    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn device_type(&self) -> &str {
        &self.device_type
    }

    fn status(&self) -> Result<DeviceStatus, DeviceError> {
        Ok(self.status.lock().clone())
    }

    fn connect(&self) -> Result<(), DeviceError> {
        self.open().map_err(|e| DeviceError::Other(e.to_string()))
    }

    fn disconnect(&self) -> Result<(), DeviceError> {
        self.close();
        Ok(())
    }

    fn read(&self) -> Result<DataFrame, DeviceError> {
        <Self as Device>::read(self)
    }

    fn read_batch(&self, count: usize) -> Result<Vec<DataFrame>, DeviceError> {
        let mut results = Vec::with_capacity(count);
        for _ in 0..count {
            results.push(Device::read(self)?);
        }
        Ok(results)
    }

    fn write(&self, data: &[u8]) -> Result<(), DeviceError> {
        <Self as Device>::write(self, data)
    }

    fn health_check(&self) -> Result<bool, DeviceError> {
        Ok(self.is_open())
    }
}

impl Drop for Rs485Device {
    fn drop(&mut self) {
        self.close();
    }
}

/// 设置 GPIO 引脚值（跨平台）
///
/// # Arguments
/// - `gpio_num`: GPIO 引脚编号
/// - `value`: true = 高电平（发送使能）, false = 低电平（接收使能）
fn gpio_set_value(gpio_num: u32, value: bool) -> Result<(), Rs485Error> {
    #[cfg(target_os = "linux")]
    {
        // Linux: 使用 sysfs GPIO
        let path = format!("/sys/class/gpio/gpio{}/value", gpio_num);
        std::fs::write(&path, if value { "1" } else { "0" })
            .map_err(|e| Rs485Error::GpioError(format!("Failed to set GPIO {}: {}", gpio_num, e)))
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: 模拟实现（实际需要 platform-specific 驱动）
        tracing::debug!("GPIO {} set to {}", gpio_num, if value { "1" } else { "0" });
        Ok(())
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        // 其他平台：模拟实现
        tracing::debug!(
            "GPIO {} set to {} (mock)",
            gpio_num,
            if value { "1" } else { "0" }
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn create_test_device() -> Rs485Device {
        let config = Config {
            port: "/dev/ttyUSB0".to_string(),
            baud_rate: 9600,
            data_bits: 8,
            stop_bits: 1,
            parity: Parity::None,
            timeout_ms: 1000,
            device_addr: 0x01,
            crc_mode: device_trait::CrcMode::Crc16Modbus,
            de_gpio: Some(17),
            re_gpio: Some(27),
        };
        let handler = Arc::new(device_trait::ModbusHandler::new(
            0x01,
            device_trait::CrcMode::Crc16Modbus,
        ));
        Rs485Device::new(
            "test_ttu_001".to_string(),
            "ttu".to_string(),
            config,
            handler,
        )
    }

    #[test]
    fn test_device_creation() {
        let device = create_test_device();
        assert_eq!(device_trait::Device::device_id(&device), "test_ttu_001");
        assert_eq!(device_trait::Device::device_type(&device), "ttu");
    }

    #[test]
    fn test_device_status_offline() {
        let device = create_test_device();
        let status = device_trait::Device::status(&device).unwrap();
        assert_eq!(status, DeviceStatus::Offline);
    }

    #[test]
    fn test_device_is_open() {
        let device = create_test_device();
        assert!(!device.is_open());
    }

    #[test]
    fn test_config_with_gpio() {
        let device = create_test_device();
        assert_eq!(device.config.de_gpio, Some(17));
        assert_eq!(device.config.re_gpio, Some(27));
    }

    #[test]
    fn test_rs485_dir() {
        assert_eq!(Rs485Dir::Send, Rs485Dir::Send);
        assert_eq!(Rs485Dir::Recv, Rs485Dir::Recv);
    }

    #[test]
    fn test_config_validation_same_gpio() {
        let mut config = Config::default();
        config.de_gpio = Some(17);
        config.re_gpio = Some(17); // Same as de_gpio
        assert!(config.validate().is_err());
        assert_eq!(config.validate().unwrap_err(), "DE 和 RE 引脚不能相同");
    }

    #[test]
    fn test_build_read_frame_fc03_slave_param() {
        // slave=2（非默认1）、FC03、addr=0x0100、count=2
        let frame = build_read_frame(2, 0x03, 0x0100, 2, CrcMode::Crc16Modbus);
        assert_eq!(frame[0], 2, "帧首字节应为显式 slave=2");
        assert_eq!(frame[1], 0x03);
        assert_eq!(&frame[2..6], &[0x01, 0x00, 0x00, 0x02]);
        assert_eq!(frame.len(), 8);

        // CRC 低位在前，符合同参 Frame::calculate_crc 结果
        let crc = Frame::calculate_crc(2, 0x03, &frame[2..6], CrcMode::Crc16Modbus);
        assert_eq!((frame[6] as u16) | ((frame[7] as u16) << 8), crc);

        // 独立已知向量（参考实现 CRC16-Modbus 计算）：02 03 01 00 00 02 C5 C4
        assert_eq!(frame, vec![0x02, 0x03, 0x01, 0x00, 0x00, 0x02, 0xC5, 0xC4]);
    }

    #[test]
    fn test_build_read_frame_fc04_slave_param() {
        // slave=0x2A、FC04、addr=0x0000、count=4
        let frame = build_read_frame(0x2A, 0x04, 0x0000, 4, CrcMode::Crc16Modbus);
        assert_eq!(frame[0], 0x2A);
        assert_eq!(frame[1], 0x04);
        assert_eq!(frame.len(), 8);
        // 独立已知向量：2A 04 00 00 00 04 F7 D2
        assert_eq!(frame, vec![0x2A, 0x04, 0x00, 0x00, 0x00, 0x04, 0xF7, 0xD2]);
    }

    #[test]
    fn test_build_read_frame_differing_slave_first_byte() {
        // 同一配置（device_addr=1）与显式 slave=2 的帧首必须不同 —— 同口多从站语义
        let cfg_frame = build_read_frame(0x01, 0x03, 0x0100, 2, CrcMode::Crc16Modbus);
        let explicit_frame = build_read_frame(0x02, 0x03, 0x0100, 2, CrcMode::Crc16Modbus);
        assert_eq!(cfg_frame[0], 0x01);
        assert_eq!(explicit_frame[0], 0x02);
        assert_ne!(cfg_frame, explicit_frame);
    }

    #[test]
    fn test_parse_regs_response_ok() {
        // [slave, func, byte_count, reg1_hi, reg1_lo, reg2_hi, reg2_lo, crc_lo, crc_hi]
        // 原用例 CRC 位是请求帧 [02 03 01 00 00 02] 的 CRC（C5 C4），非本响应帧的真实 CRC；
        // 请求级读路径补 CRC 校验后必须换成真值：独立复算 CRC(lo,hi)=A9 7F。
        let response = vec![0x02, 0x03, 0x04, 0x01, 0x02, 0xFF, 0xFE, 0xA9, 0x7F];
        let regs = parse_regs_response(&response, 0x02, CrcMode::Crc16Modbus).unwrap();
        assert_eq!(regs, vec![0x0102, 0xFFFE]);
    }

    #[test]
    fn test_parse_regs_response_too_short() {
        assert!(parse_regs_response(&[], 0x02, CrcMode::Crc16Modbus).is_err());
        assert!(
            parse_regs_response(&[0x02, 0x03, 0x04, 0x01], 0x02, CrcMode::Crc16Modbus).is_err()
        );
    }

    #[test]
    fn test_parse_regs_response_incomplete() {
        // byte_count 声称 4，实际只有 3 个数据字节：CRC 覆盖实际字节（自洽 ⇒ 校验通过），
        // 应由 byte_count/长度一致性检查判"不完整"——保持本用例原有判别意图。
        let mut response = vec![0x02, 0x03, 0x04, 0x01, 0x02, 0xFF];
        let crc = Frame::calculate_crc(0x02, 0x03, &response[2..], CrcMode::Crc16Modbus);
        response.push(crc as u8);
        response.push((crc >> 8) as u8);
        assert!(parse_regs_response(&response, 0x02, CrcMode::Crc16Modbus).is_err());
    }

    #[test]
    fn test_read_holding_registers_delegates_config_device_addr() {
        // 未打开串口：委托链（read_holding_registers -> *_from -> read_regs -> send_recv）
        // 在触碰真实串口前即返回 NotConnected。证明原签名委托路径存活、零 IO、零破坏。
        let device = create_test_device(); // config.device_addr = 0x01
        let err = device.read_holding_registers(0x0100, 2).unwrap_err();
        assert!(
            matches!(err, Rs485Error::NotConnected(_)),
            "应返回 NotConnected（串口未打开），实际: {:?}",
            err
        );
    }

    #[test]
    fn test_read_from_methods_alive_no_io() {
        // 新方法 read_holding_registers_from / read_input_registers_from 存在且走同一条
        // send_recv 委托路径（未打开 -> NotConnected），串口未打开前无真实 IO。
        let device = create_test_device();
        let err1 = device
            .read_holding_registers_from(2, 0x0100, 2)
            .unwrap_err();
        assert!(matches!(err1, Rs485Error::NotConnected(_)));
        let err2 = device
            .read_input_registers_from(0x2A, 0x0000, 4)
            .unwrap_err();
        assert!(matches!(err2, Rs485Error::NotConnected(_)));
    }
}

/// 审查 P0-2：请求级读路径（FC03/FC04/FC02）的帧级严格校验。
///
/// 期望值全部来自**独立 CRC 向量**（Python 复算，非本仓实现产出），
/// 见各用例注释中的 "CRC(lo,hi)=…"。
#[cfg(test)]
mod frame_validation_tests {
    use super::tests::create_test_device;
    use super::*;

    // ── parse_regs_response（FC03/FC04） ────────────────────────────────

    #[test]
    fn regs_valid_frame_passes() {
        // 请求 slave=2 FC03；响应 02 03 04 01 02 FF FE → CRC(lo,hi)=A9 7F
        let resp = vec![0x02, 0x03, 0x04, 0x01, 0x02, 0xFF, 0xFE, 0xA9, 0x7F];
        let regs = parse_regs_response(&resp, 2, CrcMode::Crc16Modbus).unwrap();
        assert_eq!(regs, vec![0x0102, 0xFFFE], "CRC 与从站号均正确 ⇒ 必须放行");
    }

    #[test]
    fn regs_tampered_crc_err() {
        // 同上帧，末 CRC 字节 0x7F 篡改为 0x7E（数据改动而 CRC 未跟改的典型现场）
        let resp = vec![0x02, 0x03, 0x04, 0x01, 0x02, 0xFF, 0xFE, 0xA9, 0x7E];
        let err = parse_regs_response(&resp, 2, CrcMode::Crc16Modbus).unwrap_err();
        assert!(
            matches!(err, Rs485Error::CrcFailed(_)),
            "坏 CRC 必须返回 CrcFailed（设计 §2.7），实际: {err:?}"
        );
    }

    #[test]
    fn regs_tampered_payload_err() {
        // 数据字节篡改（0x0102 → 0x0103）而 CRC 不变 ⇒ 同样必须拒
        let resp = vec![0x02, 0x03, 0x04, 0x01, 0x03, 0xFF, 0xFE, 0xA9, 0x7F];
        let err = parse_regs_response(&resp, 2, CrcMode::Crc16Modbus).unwrap_err();
        assert!(matches!(err, Rs485Error::CrcFailed(_)), "实际: {err:?}");
    }

    #[test]
    fn regs_foreign_slave_err() {
        // 请求 slave=2，总线回的是 slave=3 的**格式完全合法**帧（同口多从站串扰）
        // 03 03 04 01 02 FF FE → CRC(lo,hi)=B9 BF
        let resp = vec![0x03, 0x03, 0x04, 0x01, 0x02, 0xFF, 0xFE, 0xB9, 0xBF];
        let err = parse_regs_response(&resp, 2, CrcMode::Crc16Modbus).unwrap_err();
        assert!(
            matches!(err, Rs485Error::ConfigFailed(_)),
            "他站帧必须拒，实际: {err:?}"
        );
        assert!(
            err.to_string().contains("slave=2") && err.to_string().contains("slave=3"),
            "错误报文须含请求/响应双方从站号，实际: {err}"
        );
    }

    #[test]
    fn regs_exception_frame_err() {
        // 异常帧 [slave=1, 0x03|0x80, 异常码 0x02(非法数据地址)] → CRC(lo,hi)=C0 F1
        let resp = vec![0x01, 0x83, 0x02, 0xC0, 0xF1];
        let err = parse_regs_response(&resp, 1, CrcMode::Crc16Modbus).unwrap_err();
        assert!(matches!(err, Rs485Error::ConfigFailed(_)), "实际: {err:?}");
        assert!(
            err.to_string().contains("异常") && err.to_string().contains("02"),
            "错误报文须带异常码，实际: {err}"
        );
    }

    #[test]
    fn regs_exception_frame_not_silently_parsed_as_data() {
        // 反证：异常码恰为 0x00 时旧实现的长度检查会放行（3+0+2=5）⇒ 返回空寄存器"成功"。
        // 现必须按异常帧拒。
        let mut resp = vec![0x01, 0x83, 0x00];
        let crc = Frame::calculate_crc(0x01, 0x83, &resp[2..], CrcMode::Crc16Modbus);
        resp.push(crc as u8);
        resp.push((crc >> 8) as u8);
        let err = parse_regs_response(&resp, 1, CrcMode::Crc16Modbus).unwrap_err();
        assert!(err.to_string().contains("异常"), "实际: {err}");
    }

    // ── parse_bits_response（FC02） ────────────────────────────────────

    #[test]
    fn bits_valid_frame_passes() {
        // 请求 slave=1 FC02 addr0 count8；响应 01 02 01 3B → CRC(lo,hi)=E0 5B
        let resp = vec![0x01, 0x02, 0x01, 0x3B, 0xE0, 0x5B];
        let bits = parse_bits_response(&resp, 1, 8, CrcMode::Crc16Modbus).unwrap();
        assert_eq!(
            bits,
            vec![true, true, false, true, true, true, false, false],
            "PRD §9.7.4 位序（bit0=LSB）不得因新增校验而改变"
        );
    }

    #[test]
    fn bits_tampered_crc_err() {
        // 末 CRC 字节 0x5B 篡改为 0x5A
        let resp = vec![0x01, 0x02, 0x01, 0x3B, 0xE0, 0x5A];
        let err = parse_bits_response(&resp, 1, 8, CrcMode::Crc16Modbus).unwrap_err();
        assert!(
            matches!(err, Rs485Error::CrcFailed(_)),
            "坏 CRC 必须返回 CrcFailed，实际: {err:?}"
        );
    }

    #[test]
    fn bits_foreign_slave_err() {
        // 请求 slave=1，回的是 slave=3 的合法帧 03 02 04 3B C5 6E 93 → CRC(lo,hi)=A9 36
        let resp = vec![0x03, 0x02, 0x04, 0x3B, 0xC5, 0x6E, 0x93, 0xA9, 0x36];
        let err = parse_bits_response(&resp, 1, 31, CrcMode::Crc16Modbus).unwrap_err();
        assert!(
            err.to_string().contains("slave=1") && err.to_string().contains("slave=3"),
            "他站帧必须拒且报文含双方从站号，实际: {err}"
        );
    }

    #[test]
    fn bits_exception_frame_err() {
        // 异常帧 [slave=1, 0x02|0x80, 异常码 0x02] → CRC(lo,hi)=C1 61
        let resp = vec![0x01, 0x82, 0x02, 0xC1, 0x61];
        let err = parse_bits_response(&resp, 1, 8, CrcMode::Crc16Modbus).unwrap_err();
        assert!(
            err.to_string().contains("异常"),
            "异常帧不得当数据解包，实际: {err}"
        );
    }

    // ── 读路径委托链：slave 透传（防"校验参数接错线"） ───────────────────

    #[test]
    fn read_from_methods_forward_requested_slave() {
        // 关键接线：*_from 的 `slave` 必须一路透传到响应校验（read_regs/parse_* 的
        // 第二参数）。串口未打开时委托链在 send_recv 处即返回 NotConnected —— 该用例
        // 钉的是"编译期接线 + 不 panic + 零 IO"，真机帧校验由上面各用例覆盖。
        let device = create_test_device(); // config.device_addr = 0x01
        for err in [
            device
                .read_holding_registers_from(2, 0x0100, 2)
                .unwrap_err(),
            device.read_input_registers_from(3, 0x0000, 4).unwrap_err(),
            device.read_discrete_inputs_from(4, 0x0000, 31).unwrap_err(),
        ] {
            assert!(matches!(err, Rs485Error::NotConnected(_)), "实际: {err:?}");
        }
    }

    #[test]
    fn read_path_validates_the_slave_actually_requested_not_config_addr() {
        // 审查 W1：探针③证实"把透传的 slave 换成 config.device_addr"后 59+10 条全绿 ⇒
        // 该接线接错也无人发现。本用例经 `send_recv` 测试注入口（零 IO）把
        // 「请求帧 → 响应 → 帧级校验」整链跑到底，并刻意让 **请求 slave=2 ≠ config.device_addr=1**：
        //   ① 回 slave=2（= 请求帧首字节）的合法帧 ⇒ 必须放行（期望值若误取 config.device_addr 必红）；
        //   ② 回 slave=1（= config.device_addr）的合法帧 ⇒ 必须拒（且报文含双方从站号）。
        // ①② 一正一反互为对照：既钉"必须按请求校验"，也钉"不是碰巧什么都放行"。
        let device = create_test_device(); // config.device_addr = 0x01
        assert_eq!(
            device.config.device_addr, 0x01,
            "前提：config 的从站号必须与请求从站号不同，否则本用例失去判别力"
        );

        // ① FC03：请求 slave=2，响应 slave=2 的合法帧（独立复算 CRC(lo,hi)=A9 7F）
        *device.test_response.lock() =
            Some(vec![0x02, 0x03, 0x04, 0x01, 0x02, 0xFF, 0xFE, 0xA9, 0x7F]);
        assert_eq!(
            device.read_holding_registers_from(2, 0x0100, 2).unwrap(),
            vec![0x0102, 0xFFFE],
            "请求 slave=2 且响应 slave=2 ⇒ 必须放行；期望值只能来自请求帧首字节"
        );

        // ② FC03：请求 slave=2，回的是 slave=1（= config.device_addr）合法帧（CRC=9A 7F）⇒ 必拒
        *device.test_response.lock() =
            Some(vec![0x01, 0x03, 0x04, 0x01, 0x02, 0xFF, 0xFE, 0x9A, 0x7F]);
        let err = device
            .read_holding_registers_from(2, 0x0100, 2)
            .unwrap_err();
        assert!(
            err.to_string().contains("slave=2") && err.to_string().contains("slave=1"),
            "回 config.device_addr 的帧对请求 slave=2 而言仍是**他站帧**，必须拒，实际: {err}"
        );

        // ③ FC04 走同一条 read_regs（同一接线），正反各一次（响应帧亦为 FC04，CRC=A8 C8 / 9B C8）
        *device.test_response.lock() =
            Some(vec![0x02, 0x04, 0x04, 0x01, 0x02, 0xFF, 0xFE, 0xA8, 0xC8]);
        assert!(device.read_input_registers_from(2, 0x0100, 2).is_ok());
        *device.test_response.lock() =
            Some(vec![0x01, 0x04, 0x04, 0x01, 0x02, 0xFF, 0xFE, 0x9B, 0xC8]);
        assert!(device.read_input_registers_from(2, 0x0100, 2).is_err());

        // ④ FC02 独立接线（不经 read_regs）：正反各一次
        *device.test_response.lock() = Some(vec![0x02, 0x02, 0x01, 0x3B, 0xE0, 0x1F]);
        assert_eq!(
            device.read_discrete_inputs_from(2, 0, 8).unwrap(),
            vec![true, true, false, true, true, true, false, false],
            "FC02 请求 slave=2 且响应 slave=2 ⇒ 必须放行"
        );
        *device.test_response.lock() = Some(vec![0x01, 0x02, 0x01, 0x3B, 0xE0, 0x5B]);
        let err = device.read_discrete_inputs_from(2, 0, 8).unwrap_err();
        assert!(
            err.to_string().contains("slave=2") && err.to_string().contains("slave=1"),
            "FC02 回 config.device_addr 的帧必须拒，实际: {err}"
        );
    }

    // ── 写路径（FC06）回显校验：与读侧同一取向 ─────────────────────────

    #[test]
    fn write_single_register_from_rejects_wrong_slave_echo() {
        // 请求 slave=2；回帧从站号 = 1（= config.device_addr）⇒ 必须拒，且报文两侧从站号都要出现。
        let device = create_test_device(); // config.device_addr = 0x01
        let bad = {
            let mut v = vec![0x01, 0x06, 0x01, 0xF4, 0x00, 0x00];
            let crc = Frame::calculate_crc(0x01, 0x06, &v[2..], CrcMode::Crc16Modbus);
            v.push(crc as u8);
            v.push((crc >> 8) as u8);
            v
        };
        *device.test_response.lock() = Some(bad);
        let err = device
            .write_single_register_from(2, 0x01F4, 0x0000)
            .unwrap_err();
        // 断言与读侧三个同源用例同款（`&&` 双向）：报文必须**同时**出现请求从站与响应从站。
        // 旧写法 `contains("slave=2") || contains("从站")` 把后半句（"两侧从站号都要出现"）
        // 掏空 —— 只要报文里出现"从站"二字即绿，断言名承诺的判别力并不存在。
        let msg = err.to_string();
        assert!(
            msg.contains("slave=2") && msg.contains("slave=1"),
            "报文必须同时点明请求从站与响应从站（两侧都要出现），实际: {msg}"
        );
    }

    #[test]
    fn write_single_register_from_rejects_value_echo_mismatch() {
        // 从站号对、功能码对，但回显值不同 ⇒ 必须拒。
        let device = create_test_device();
        let bad = {
            let mut v = vec![0x02, 0x06, 0x01, 0xF4, 0x00, 0x01]; // 回显 1，请求 0
            let crc = Frame::calculate_crc(0x02, 0x06, &v[2..], CrcMode::Crc16Modbus);
            v.push(crc as u8);
            v.push((crc >> 8) as u8);
            v
        };
        *device.test_response.lock() = Some(bad);
        assert!(device
            .write_single_register_from(2, 0x01F4, 0x0000)
            .is_err());
    }

    #[test]
    fn write_single_register_from_accepts_correct_echo() {
        // 正对照：从站号、功能码、地址、值全对 ⇒ 放行（防"改坏成恒 Err"式的假绿）。
        let device = create_test_device();
        // 前提断言（与读侧同源用例 device.rs:1298 同款）：请求从站号 2 必须 != config.device_addr，
        // 否则"期望从站取自请求帧首字节"与"误取 config.device_addr"两种实现皆绿，
        // 本用例对该缺陷失去判别力 —— 而 config 默认值一变即会静默发生。
        assert_eq!(
            device.config.device_addr, 0x01,
            "前提：config 从站号必须与请求从站号（2）不同，否则本用例对'误把 config.device_addr 当期望从站'失去判别力"
        );
        let ok = {
            let mut v = vec![0x02, 0x06, 0x01, 0xF4, 0x00, 0x00];
            let crc = Frame::calculate_crc(0x02, 0x06, &v[2..], CrcMode::Crc16Modbus);
            v.push(crc as u8);
            v.push((crc >> 8) as u8);
            v
        };
        *device.test_response.lock() = Some(ok);
        assert!(device.write_single_register_from(2, 0x01F4, 0x0000).is_ok());
    }

    #[test]
    fn write_single_register_from_reports_exception_code_not_short_frame() {
        // 标准 FC06 异常帧 = **5 字节**（slave=2, func=0x86, 异常码=0x03, CRC16）。
        // 必须报"异常码 0x03"，**不得**误报成"响应过短" —— 后者会把运维的诊断方向
        // 从"从站为什么拒绝"带偏到"线缆/成帧有没有问题"。
        let device = create_test_device();
        let exc = {
            let mut v = vec![0x02, 0x86, 0x03];
            let crc = Frame::calculate_crc(0x02, 0x86, &v[2..], CrcMode::Crc16Modbus);
            v.push(crc as u8);
            v.push((crc >> 8) as u8);
            v
        };
        assert_eq!(exc.len(), 5, "前提：标准 Modbus 异常帧长度为 5");
        *device.test_response.lock() = Some(exc);
        let err = device
            .write_single_register_from(2, 0x01F4, 0x0000)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("异常码") && msg.contains("0x03"),
            "须报出异常码，实际: {msg}"
        );
        assert!(!msg.contains("过短"), "不得误报成响应过短，实际: {msg}");
    }

    #[test]
    fn write_single_register_from_reports_short_frame_without_panic() {
        // 零字节响应 = 串口读超时（VMIN=0/VTIME 语义下 recv_frame 返回 Ok(vec![])）。
        // 用途有二：① 钉住"不裸索引"取值（无守卫则 response[1] 直接 panic，而南向采集
        // task 内 panic 会静默终止整口采集）；② 钉住"超时"这条运维最常见的失败形态
        // 能给出可读报文（而非 panic 或空错误）。
        let device = create_test_device();
        *device.test_response.lock() = Some(Vec::new());
        let err = device
            .write_single_register_from(2, 0x01F4, 0x0000)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("过短") || msg.contains("0 字节"),
            "空响应须给出可读的'过短/0 字节'报文而非 panic，实际: {msg}"
        );
    }

    #[test]
    fn write_single_register_from_does_not_panic_on_two_byte_response() {
        // 2 字节响应（噪声/残帧）：`get()` 取值必须挡住 response[2] 越界，且**不得**把它
        // 当异常帧（凭空报"被从站拒绝，异常码=0x00"）—— 它是"没收到成帧"，不是"从站拒绝"。
        // 判别力：把取值写成 `response.len() >= 2 && response[1] == (FUNC | 0x80)` 再读
        // `response[2]` ⇒ 本用例 panic；只判 func 位不判异常码字段在不在 ⇒ 本用例断言红。
        let device = create_test_device();
        *device.test_response.lock() = Some(vec![0x02, 0x86]);
        let err = device
            .write_single_register_from(2, 0x01F4, 0x0000)
            .unwrap_err();
        assert!(err.to_string().contains("过短"), "实际: {err}");
    }

    // ── 交换缝（`send_recv` 层：请求原文 → 响应原文）─────────────────────

    #[test]
    fn test_exchange_seam_sees_request_and_returns_response() {
        // 缝拿到的必须是**请求帧原文**（含 CRC），不是解析后的结构：
        // 用 FC03 请求 slave=2 addr=0x0100 count=2，断言首字节/功能码/地址/CRC 自洽。
        let device = create_test_device();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let seen2 = seen.clone();
        device.set_test_exchange(Box::new(move |req: &[u8]| {
            *seen2.lock().unwrap() = req.to_vec();
            // 回一个合法响应：slave=2, FC03, 字节数 4, 值 0x0102/0xFFFE
            let mut v = vec![0x02, 0x03, 0x04, 0x01, 0x02, 0xFF, 0xFE];
            let crc = Frame::calculate_crc(0x02, 0x03, &v[2..], CrcMode::Crc16Modbus);
            v.push(crc as u8);
            v.push((crc >> 8) as u8);
            v
        }));
        let out = device.read_holding_registers_from(2, 0x0100, 2).unwrap();
        assert_eq!(out, vec![0x0102, 0xFFFE], "缝合法的响应必须被正常解析");
        let req = seen.lock().unwrap().clone();
        assert_eq!(req[0], 0x02, "请求帧首字节 = 目标从站");
        assert_eq!(req[1], 0x03, "功能码 FC03");
        assert_eq!(&req[2..4], &[0x01, 0x00], "起始地址 0x0100");
        assert_eq!(&req[4..6], &[0x00, 0x02], "寄存器数 2");
        let crc = Frame::calculate_crc(0x02, 0x03, &req[2..6], CrcMode::Crc16Modbus);
        assert_eq!(&req[6..8], &[crc as u8, (crc >> 8) as u8], "CRC 低字节在前");
    }

    #[test]
    fn test_exchange_seam_takes_precedence_and_is_clearable() {
        // 缝设置后**优先于** test_response；clear 之后回到 test_response（默认 None → NotConnected）。
        let device = create_test_device();
        *device.test_response.lock() = Some(vec![0xEE; 8]); // 会被缝遮蔽
        device.set_test_exchange(Box::new(|_req: &[u8]| {
            let mut v = vec![0x02, 0x03, 0x02, 0x00, 0x2A];
            let crc = Frame::calculate_crc(0x02, 0x03, &v[2..], CrcMode::Crc16Modbus);
            v.push(crc as u8);
            v.push((crc >> 8) as u8);
            v
        }));
        assert_eq!(device.read_holding_registers_from(2, 0, 1).unwrap(), vec![0x2A]);

        device.clear_test_exchange();
        *device.test_response.lock() = None;
        assert!(
            matches!(
                device.read_holding_registers_from(2, 0, 1),
                Err(Rs485Error::NotConnected(_))
            ),
            "清缝且无 test_response ⇒ 回到未打开的 NotConnected（不改变既有语义）"
        );
    }
}

/// S3b-2 T2：FC02 离散输入帧层（设计 §11.4.3 / §11.4.5，PRD §9.7.4）。
#[cfg(test)]
mod fc02_tests {
    use super::tests::create_test_device;
    use super::*;

    // ── unpack_bits：PRD §9.7.4 的唯一解包公式（bit0 = LSB） ────────────

    #[test]
    fn test_unpack_bits_prd_original_example() {
        // PRD §9.7.4 引用 BMS 协议 §3.2.4 原文示例：
        // 连续 16 位 1,1,0,1,1,1,0,0,… → 首字节 00111011B = 0x3B
        // 随后      1,1,0,1,1,1,0,1   → 10111011B = 0xBB
        assert_eq!(
            unpack_bits(&[0x3B], 8),
            vec![true, true, false, true, true, true, false, false]
        );
        assert_eq!(
            unpack_bits(&[0x3B, 0xBB], 16),
            vec![
                true, true, false, true, true, true, false, false, // 0x3B
                true, true, false, true, true, true, false, true, // 0xBB
            ]
        );
    }

    #[test]
    fn test_unpack_bits_31_bits_tail_byte_does_not_pollute() {
        // 空调 hvac_di：addr 0 / count 31 ⇒ 响应 4 字节，末字节的 bit7 是无关位。
        // 设计 §11.2.3 选型 C1 明确否掉了"复用 Vec<u16>"：尾部无关位不得污染前 31 位。
        let tail_set = unpack_bits(&[0x3B, 0xBB, 0x00, 0xFF], 31);
        assert_eq!(tail_set.len(), 31, "长度必须 = count，不含无关位");
        assert!(
            tail_set[24..31].iter().all(|b| *b),
            "bit7=1 无关位，前 7 位仍为 1"
        );

        let tail_clear = unpack_bits(&[0x3B, 0xBB, 0x00, 0x80], 31);
        assert_eq!(tail_clear.len(), 31);
        assert!(
            tail_clear[24..31].iter().all(|b| !*b),
            "无关位置 1 也不得被读进来（bit 31 不是有效位）"
        );
        // 反证：把 count 提到 32，同一位就是有效位 ⇒ 必须为 true
        assert!(unpack_bits(&[0x3B, 0xBB, 0x00, 0x80], 32)[31]);
    }

    #[test]
    fn test_unpack_bits_288_bits_bms_scale() {
        // BMS bms_alarm：288 位 = 36 字节，逐位与构造位图比对（含跨字节边界）
        let bytes: Vec<u8> = (0..36u32).map(|i| (i * 37 + 11) as u8).collect();
        let bits = unpack_bits(&bytes, 288);
        assert_eq!(bits.len(), 288);
        for k in 0..288usize {
            let expected = (bytes[k / 8] >> (k % 8)) & 1 == 1;
            assert_eq!(
                bits[k],
                expected,
                "位 {k} 不符（应取第 {} 字节的 bit {}）",
                k / 8,
                k % 8
            );
        }
    }

    #[test]
    fn test_unpack_bits_cross_byte_boundary_explicit() {
        // 显式边界断言：bit7 = 字节 0 的最高位；bit8 = 字节 1 的最低位（跨字节）
        let edge = unpack_bits(&[0x80, 0x01], 16);
        assert!(edge[..7].iter().all(|b| !*b), "bit0..6 应为 0");
        assert!(edge[7], "bit7 是字节 0 的 bit7");
        assert!(edge[8], "bit8 是字节 1 的 bit0（跨字节边界）");
        assert!(edge[9..].iter().all(|b| !*b), "bit9..15 应为 0");
    }

    #[test]
    fn test_unpack_bits_edges_and_non_multiples() {
        assert_eq!(unpack_bits(&[], 0), Vec::<bool>::new());
        assert_eq!(unpack_bits(&[0x01], 1), vec![true]);
        assert_eq!(
            unpack_bits(&[0x02], 1),
            vec![false],
            "count 之外的位不得取用"
        );
        assert_eq!(unpack_bits(&[0xFF], 3), vec![true, true, true]);
        let nine = unpack_bits(&[0x00, 0x01], 9);
        assert_eq!(nine.len(), 9);
        assert!(nine[8], "第 9 位取自字节 1 的 bit0");
        assert!(nine[..8].iter().all(|b| !*b));
    }

    #[test]
    fn test_unpack_bits_short_input_pads_false() {
        // 契约："返回长度 = count"（设计 §11.4.3）。字节不足时按 0 补齐（不 panic、不越界）；
        // "响应字节数不足"由帧层 parse_bits_response 拒（见下），纯函数保持全定义。
        let bits = unpack_bits(&[0x01], 16);
        assert_eq!(bits.len(), 16);
        assert!(bits[0]);
        assert!(bits[1..].iter().all(|b| !*b));
    }

    // ── parse_bits_response：FC02 响应 → 位向量 ────────────────────────

    #[test]
    fn test_parse_bits_response_31_bits() {
        // 保留 PRD §9.7.4 示例锚（首字节 0x3B，bit0=LSB ⇒ 1,1,0,1,1,1,0,0），但断言从
        // bits[..6] **扩到全部 31 位**，且期望值为**独立手写常量**（不经 unpack_bits 反读，
        // 否则与实现同义反复）。
        //
        // 载荷刻意避开 0x3B/0xBB 这类"低 6 位平移不变"的组合（两者低 6 位均 = 111011）：
        // 原用例只断言 bits[..6]，帧层数据段错位 1 字节时读到的是 0xBB，其低 6 位与原
        // 0x3B 恰好相同 ⇒ 对该缺陷**零判别力**（S3b-2 测试门禁报告 R3 / 探针 C 实证）。
        // 现相邻字节低 6 位两两不同 ⇒ 取数错位必然使本用例变红。
        let data = [0x3B, 0xC5, 0x6E, 0x93];
        let bits = parse_bits_response(
            &build_fc02_response(0x01, &data),
            0x01,
            31,
            CrcMode::Crc16Modbus,
        )
        .unwrap();
        assert_eq!(bits.len(), 31, "长度恒 = count，末字节 bit7 为无关位不入列");

        let expected: Vec<bool> = [
            // 0x3B = 0b0011_1011（PRD §9.7.4 示例字节）
            true, true, false, true, true, true, false, false, // 0xC5 = 0b1100_0101
            true, false, true, false, false, false, true, true, // 0x6E = 0b0110_1110
            false, true, true, true, false, true, true, false,
            // 0x93 = 0b1001_0011：末字节只取低 7 位（bit7 无关）
            true, true, false, false, true, false, false,
        ]
        .to_vec();
        assert_eq!(expected.len(), 31, "手写锚长度必须为 31");
        assert_eq!(bits, expected, "帧级 31 位须逐位等于独立手写锚");
    }

    #[test]
    fn test_parse_bits_response_288_bits() {
        // 原用例 CRC 用 0x00,0x00 占位（当时本函数不校验 CRC）；补校验后改用真实帧
        // （build_fc02_response 按产线同一 CRC 实现生成）。
        let data: Vec<u8> = (0..36u8).map(|i| i.wrapping_mul(7)).collect();
        let resp = build_fc02_response(0x01, &data);
        let bits = parse_bits_response(&resp, 0x01, 288, CrcMode::Crc16Modbus).unwrap();
        assert_eq!(bits, unpack_bits(&data, 288));
    }

    #[test]
    fn test_parse_bits_response_too_short() {
        assert!(parse_bits_response(&[], 0x01, 8, CrcMode::Crc16Modbus).is_err());
        assert!(parse_bits_response(&[0x01, 0x02, 0x01], 0x01, 8, CrcMode::Crc16Modbus).is_err());
    }

    #[test]
    fn test_parse_bits_response_incomplete() {
        // byte_count 声称 4 但报文只有 3 个数据字节：CRC 覆盖实际字节（自洽 ⇒ 帧级校验通过），
        // 应由 byte_count/长度一致性检查判"不完整"——保持本用例原有判别意图。
        let mut resp = vec![0x01, 0x02, 0x04, 0x3B, 0xBB, 0x00];
        let crc = Frame::calculate_crc(0x01, 0x02, &resp[2..], CrcMode::Crc16Modbus);
        resp.push(crc as u8);
        resp.push((crc >> 8) as u8);
        assert!(parse_bits_response(&resp, 0x01, 31, CrcMode::Crc16Modbus).is_err());
    }

    #[test]
    fn test_parse_bits_response_byte_count_insufficient_for_count() {
        // 288 位需 36 字节，响应只给 2 字节 ⇒ 必须拒（不得静默补 0 冒充"全部正常"）
        // 帧级校验（CRC/从站号）先行通过，本用例专钉 byte_count 不足这一层。
        let resp = build_fc02_response(0x01, &[0x00, 0x00]);
        let err = parse_bits_response(&resp, 0x01, 288, CrcMode::Crc16Modbus).unwrap_err();
        assert!(
            matches!(err, Rs485Error::ConfigFailed(_)),
            "应为配置/帧错误，实际: {err:?}"
        );
        assert!(
            err.to_string().contains("字节数不足"),
            "错误须指向字节数不足而非 CRC，实际: {err}"
        );
    }

    #[test]
    fn test_parse_bits_response_extra_bytes_ignored() {
        // byte_count 大于所需时只取前 ceil(count/8) 字节（多余字节不参与解包）
        let resp = build_fc02_response(0x01, &[0x3B, 0xBB, 0x00, 0x00, 0xAA]);
        let bits = parse_bits_response(&resp, 0x01, 16, CrcMode::Crc16Modbus).unwrap();
        assert_eq!(bits, unpack_bits(&[0x3B, 0xBB], 16));
    }

    // ── G1 闭合（S3b-2 测试报告 §4.1）：帧级字节→位端到端断言 ───────────
    //
    // 上述 parse_bits_response 用例中，288 位一例以 unpack_bits 自身为期望值
    // （同义反复，位序改坏时两侧同变 ⇒ 无判别力）。本段全部使用**独立逐位锚**
    // （手写期望向量 / 独立位提取公式），并把 PRD §9.7.4 位序口径钉在帧层：
    // 即使 southd L2 经 MockBus 注入已解包位向量而绕过帧层（P3 实证），
    // 位序回归也必然在此变红。

    /// 手工构造完整 FC02 响应帧：[slave, 0x02, byte_count, data…, CRC16(lo,hi)]。
    /// CRC 用产线同一实现计算（审查 P0-2 起 `parse_bits_response` **严格校验 CRC**）。
    fn build_fc02_response(slave: u8, data: &[u8]) -> Vec<u8> {
        let mut frame = vec![slave, 0x02, data.len() as u8];
        frame.extend_from_slice(data);
        let crc = Frame::calculate_crc(slave, 0x02, &frame[2..], CrcMode::Crc16Modbus);
        frame.push(crc as u8);
        frame.push((crc >> 8) as u8);
        frame
    }

    #[test]
    fn fc02_frame_8_bits_prd_anchor() {
        // PRD §9.7.4 原文示例字节 0x3B 由**完整响应帧**驱动（1 字节最小正向帧）：
        // 0x3B = 0b0011_1011，bit0=LSB ⇒ 1,1,0,1,1,1,0,0
        let resp = build_fc02_response(0x01, &[0x3B]);
        let bits = parse_bits_response(&resp, 0x01, 8, CrcMode::Crc16Modbus).unwrap();
        assert_eq!(
            bits,
            vec![true, true, false, true, true, true, false, false],
            "帧级 8 位解包必须逐位等于 PRD 示例（bit0 = LSB）"
        );
    }

    #[test]
    fn fc02_frame_bit_order_is_lsb_first() {
        // 位序回归钉（G1/P3）：两帧**不对称**单字节，钉死 bit0/bit7 的归属。
        // 若 unpack_bits 被改成 MSB-first，本用例两帧全部翻转 ⇒ 必红，且红在帧层。
        let resp_a = build_fc02_response(0x01, &[0b0000_0001]);
        let bits_a = parse_bits_response(&resp_a, 0x01, 8, CrcMode::Crc16Modbus).unwrap();
        assert!(bits_a[0], "0x01 的 LSB ⇒ 位 0 必须为 true");
        assert!(
            bits_a[1..].iter().all(|b| !*b),
            "0x01 只有 bit0 置位，位 1..7 必须全 false"
        );

        let resp_b = build_fc02_response(0x01, &[0b1000_0000]);
        let bits_b = parse_bits_response(&resp_b, 0x01, 8, CrcMode::Crc16Modbus).unwrap();
        assert!(bits_b[7], "0x80 的 MSB ⇒ 位 7 必须为 true");
        assert!(
            bits_b[..7].iter().all(|b| !*b),
            "0x80 只有 bit7 置位，位 0..6 必须全 false"
        );
    }

    #[test]
    fn fc02_frame_31_bits_tail_irrelevant_bit_no_pollution() {
        // 空调 hvac_di 规格：count=31 ⇒ 4 数据字节，末字节仅 bit7 为无关位。
        // 无关位置 0（0x5A）/ 置 1（0xDA）两帧结果必须**逐位相同**，
        // 且等于独立手写期望向量（不经 unpack_bits 比对）。
        let expected: Vec<bool> = [
            // 0x3B = 0b0011_1011（bit0=LSB）
            true, true, false, true, true, true, false, false, // 0xBB = 0b1011_1011
            true, true, false, true, true, true, false, true, // 0xA5 = 0b1010_0101
            true, false, true, false, false, true, false, true,
            // 0x5A/0xDA 低 7 位 = 0b101_1010（bit7 无关）
            false, true, false, true, true, false, true,
        ]
        .to_vec();
        assert_eq!(expected.len(), 31);

        let tail_clear = parse_bits_response(
            &build_fc02_response(0x01, &[0x3B, 0xBB, 0xA5, 0x5A]),
            0x01,
            31,
            CrcMode::Crc16Modbus,
        )
        .unwrap();
        let tail_set = parse_bits_response(
            &build_fc02_response(0x01, &[0x3B, 0xBB, 0xA5, 0xDA]),
            0x01,
            31,
            CrcMode::Crc16Modbus,
        )
        .unwrap();
        assert_eq!(tail_clear, expected, "无关位=0 帧必须逐位等于独立锚");
        assert_eq!(tail_set, tail_clear, "末字节 bit7 置 1 不得改变前 31 位");
        assert_eq!(tail_set.len(), 31, "长度恒 = count，无关位不入列");
    }

    #[test]
    fn fc02_frame_288_bits_independent_anchor() {
        // BMS bms_alarm 规格：count=288 ⇒ 36 数据字节。期望值用**独立位提取公式**
        // （非 unpack_bits 调用）逐位比对，并硬编码跨字节边界与首末位抽样。
        let data: Vec<u8> = (0..36u32).map(|i| (i * 37 + 11) as u8).collect();
        let resp = build_fc02_response(0x01, &data);
        let bits = parse_bits_response(&resp, 0x01, 288, CrcMode::Crc16Modbus).unwrap();
        assert_eq!(bits.len(), 288);
        for k in 0..288usize {
            let expected = (data[k / 8] >> (k % 8)) & 1 == 1;
            assert_eq!(
                bits[k],
                expected,
                "帧级位 {k}（字节 {} 的 bit {}）不符",
                k / 8,
                k % 8
            );
        }
        // 硬编码抽样：首末位 + 字节 31/32 交界（防"公式与实现同错"的极端情形）
        assert!(bits[0], "data[0]=11=0b0000_1011 ⇒ 位 0 = true");
        assert!(
            !bits[287],
            "data[35]=26=0b0001_1010 ⇒ 位 287（bit7）= false"
        );
        assert!(bits[255], "data[31]=134=0b1000_0110 ⇒ 位 255（bit7）= true");
        assert!(
            bits[256],
            "data[32]=171=0b1010_1011 ⇒ 位 256（次字节 bit0）= true"
        );
    }

    #[test]
    fn fc02_frame_byte_count_one_below_needed_err() {
        // 精确边界：count=9 需 ceil(9/8)=2 字节，帧只给 1 字节 ⇒ Err（不得补 0 放行）
        let resp = build_fc02_response(0x01, &[0xFF]);
        let err = parse_bits_response(&resp, 0x01, 9, CrcMode::Crc16Modbus).unwrap_err();
        assert!(matches!(err, Rs485Error::ConfigFailed(_)), "实际: {err:?}");
    }

    #[test]
    fn fc02_frame_truncated_crc_err() {
        // 判别意图（审查 W2 恢复）：钉的是 **byte_count 与帧长不一致** 分支
        // （`parse_bits_response` 的 `len < 3 + byte_count + 2`），不是 CRC 分支。
        // 帧 [01 02 01 E0 A0]：byte_count=1，消息体只有它自己，CRC 覆盖 [01 02 01]
        // （独立复算 CRC(lo,hi)=E0 A0）⇒ **帧级校验通过**，但 3+1+2=6 > len=5
        // ⇒ 必须落"响应数据不完整"。**补真 CRC 之前该帧被 CRC 分支先拦**，
        // 该用例因此不再敏感于本分支（探针①实证）。断言错误报文，钉死落点。
        let resp = vec![0x01, 0x02, 0x01, 0xE0, 0xA0];
        let err = parse_bits_response(&resp, 0x01, 8, CrcMode::Crc16Modbus).unwrap_err();
        assert!(
            err.to_string().contains("不完整"),
            "CRC 有效而消息体截断 ⇒ 必须落长度一致性分支（而非 CRC 分支），实际: {err}"
        );
    }

    #[test]
    fn fc02_frame_byte_count_exceeds_buffer_err() {
        // 判别意图（审查 W2 恢复）：byte_count 声称 10 字节、缓冲只有 3 数据字节
        // （len=8 < 3+10+2）⇒ 必须落"响应数据不完整"（防按虚报长度越界读）。
        // CRC 覆盖实际字节 [01 02 0A 3B BB 00]（独立复算 CRC(lo,hi)=78 EF）⇒ 帧级校验
        // **通过**，否则又会被 CRC 分支先拦（同 W2 的问题）。
        let resp = vec![0x01, 0x02, 0x0A, 0x3B, 0xBB, 0x00, 0x78, 0xEF];
        let err = parse_bits_response(&resp, 0x01, 16, CrcMode::Crc16Modbus).unwrap_err();
        assert!(
            err.to_string().contains("不完整"),
            "byte_count 超出缓冲必须拒于长度一致性分支（防越界读），实际: {err}"
        );
    }

    // ── read_discrete_inputs_from：请求帧与委托路径 ─────────────────────

    #[test]
    fn test_build_read_frame_fc02_known_vectors() {
        // 独立已知向量（CRC16-Modbus 参考实现复算）
        // 空调 hvac_di：slave 1、FC02、addr 0、count 31
        assert_eq!(
            build_read_frame(0x01, 0x02, 0x0000, 31, CrcMode::Crc16Modbus),
            vec![0x01, 0x02, 0x00, 0x00, 0x00, 0x1F, 0x39, 0xC2]
        );
        // BMS bms_alarm：slave 1、FC02、addr 400(0x0190)、count 288(0x0120)
        assert_eq!(
            build_read_frame(0x01, 0x02, 400, 288, CrcMode::Crc16Modbus),
            vec![0x01, 0x02, 0x01, 0x90, 0x01, 0x20, 0x79, 0x93]
        );
    }

    #[test]
    fn test_read_discrete_inputs_from_no_io() {
        // 与既有 read_holding_registers_from / read_input_registers_from 同构：
        // 未打开串口 ⇒ 在触碰真实串口之前即返回 NotConnected。
        let device = create_test_device();
        let err = device.read_discrete_inputs_from(1, 0, 31).unwrap_err();
        assert!(
            matches!(err, Rs485Error::NotConnected(_)),
            "应返回 NotConnected（串口未打开），实际: {err:?}"
        );
    }
}
