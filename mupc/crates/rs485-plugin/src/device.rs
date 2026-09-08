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

/// 解析 Modbus 读寄存器响应（FC03/FC04 通用）。
///
/// 响应格式：[slave, func, byte_count, reg(大端 2 字节)..., crc_lo, crc_hi]
/// 本函数只做长度校验与寄存器拆解（大端 u16），不校验 CRC（与原实现逐行为一致）。
/// 纯函数、无 IO，便于单元测试。
fn parse_regs_response(response: &[u8]) -> Result<Vec<u16>, Rs485Error> {
    if response.len() < 5 {
        return Err(Rs485Error::ConfigFailed("响应数据太短".to_string()));
    }

    let byte_count = response[2] as usize;
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
            use std::os::unix::io::FromRawFd;

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

    /// 发送并接收数据
    pub fn send_recv(&self, frame: &[u8], recv_timeout_ms: u64) -> Result<Vec<u8>, Rs485Error> {
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

    /// 私有：发送读请求帧并解析响应寄存器。
    fn read_regs(&self, cmd: Vec<u8>) -> Result<Vec<u16>, Rs485Error> {
        let response = self.send_recv(&cmd, self.config.timeout_ms)?;
        parse_regs_response(&response)
    }

    /// 写入单个寄存器（Modbus 功能码 0x06）
    pub fn write_single_register(&self, addr: u16, value: u16) -> Result<(), Rs485Error> {
        let func_code: u8 = 0x06;
        let mut cmd = vec![
            self.config.device_addr,
            func_code,
            (addr >> 8) as u8,
            addr as u8,
            (value >> 8) as u8,
            value as u8,
        ];

        let crc = Frame::calculate_crc(
            self.config.device_addr,
            func_code,
            &cmd[2..],
            self.config.crc_mode,
        );
        cmd.push(crc as u8);
        cmd.push((crc >> 8) as u8);

        let response = self.send_recv(&cmd, self.config.timeout_ms)?;

        if response.len() < 8 {
            return Err(Rs485Error::ConfigFailed("响应数据太短".to_string()));
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

    fn create_test_device() -> Rs485Device {
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
        let response = vec![0x02, 0x03, 0x04, 0x01, 0x02, 0xFF, 0xFE, 0xC5, 0xC4];
        let regs = parse_regs_response(&response).unwrap();
        assert_eq!(regs, vec![0x0102, 0xFFFE]);
    }

    #[test]
    fn test_parse_regs_response_too_short() {
        assert!(parse_regs_response(&[]).is_err());
        assert!(parse_regs_response(&[0x02, 0x03, 0x04, 0x01]).is_err());
    }

    #[test]
    fn test_parse_regs_response_incomplete() {
        // byte_count=4，但 len=6 < 3+4+2=9，应判不完整
        let response = vec![0x02, 0x03, 0x04, 0x01, 0x02, 0xFF];
        assert!(parse_regs_response(&response).is_err());
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
