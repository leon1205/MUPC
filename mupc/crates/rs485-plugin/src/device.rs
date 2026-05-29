//! RS485 设备驱动实现
//!
//! 提供 RS485 设备通信能力，支持 Modbus RTU 协议

use crate::config::Config;
use crate::errors::Rs485Error;
use crate::protocol::Frame;
use device_trait::{DataFrame, Device, DeviceError, DeviceStatus, Parity, ProtocolHandler, SouthDevice};
use parking_lot::Mutex;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

/// RS485 方向控制
#[derive(Debug, Clone, Copy)]
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

impl Rs485Device {
    /// 创建新的 RS485 设备
    pub fn new(device_id: String, device_type: String, config: Config, handler: Arc<dyn ProtocolHandler>) -> Self {
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
            let fd = unsafe { libc::open(
                c_path.as_ptr(),
                libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK,
            ) };

            if fd < 0 {
                return Err(Rs485Error::open_failed(&self.config.port));
            }

            // 配置串口参数；失败时关闭 fd 防止泄漏
            if let Err(e) = self.configure_port(fd) {
                unsafe { libc::close(fd); }
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
        let termios = unsafe {
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
        termios.c_cc[libc::VTIME] = (self.config.timeout_ms / 100) as i32;
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
                    tracing::error!("关闭串口文件描述符 {} 失败: {}", fd, std::io::Error::last_os_error());
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
            let result = unsafe { libc::write(fd, frame.as_ptr() as *const libc::c_void, frame.len()) };
            if result < 0 {
                return Err(Rs485Error::send_failed("发送失败"));
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
            // 设置串口超时使用 termios
            let mut termios: MaybeUninit<libc::termios> = MaybeUninit::uninit();
            let termios = unsafe {
                if libc::tcgetattr(fd, termios.as_mut_ptr()) < 0 {
                    return Err(Rs485Error::recv_failed("获取终端属性失败"));
                }
                termios.assume_init()
            };

            // 设置读取超时：VTIME 为十分之一秒
            termios.c_cc[libc::VTIME] = (timeout_ms / 100) as i32;
            termios.c_cc[libc::VMIN] = 0;

            if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) } < 0 {
                return Err(Rs485Error::recv_failed("设置终端属性失败"));
            }

            let mut buffer = vec![0u8; 1024];
            let n = unsafe { libc::read(fd, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len()) };

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
    pub fn transaction(&self, request: &[u8], recv_timeout_ms: u64) -> Result<DataFrame, Rs485Error> {
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
    pub fn transaction_with_handler(&self, data: &[u8], recv_timeout_ms: u64) -> Result<DataFrame, Rs485Error> {
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
        self.handler.decode_response(&response)
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

    /// 读取保持寄存器（Modbus 功能码 0x03）
    pub fn read_holding_registers(&self, addr: u16, count: u16) -> Result<Vec<u16>, Rs485Error> {
        let func_code: u8 = 0x03;
        let mut cmd = vec![
            self.config.device_addr,
            func_code,
            (addr >> 8) as u8,
            addr as u8,
            (count >> 8) as u8,
            count as u8,
        ];

        // 添加 CRC
        let crc = Frame::calculate_crc(self.config.device_addr, func_code, &cmd[2..], self.config.crc_mode);
        cmd.push((crc >> 8) as u8);
        cmd.push(crc as u8);

        let response = self.send_recv(&cmd, self.config.timeout_ms)?;

        // 解析响应
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

        let crc = Frame::calculate_crc(self.config.device_addr, func_code, &cmd[2..], self.config.crc_mode);
        cmd.push((crc >> 8) as u8);
        cmd.push(crc as u8);

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
                let registers = self.read_holding_registers(0, 10)
                    .map_err(|e| DeviceError::Other(e.to_string()))?;

                let data = registers.iter()
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
        let crc = Frame::calculate_crc(self.config.device_addr, func_code, &cmd[2..], self.config.crc_mode);
        cmd.push((crc >> 8) as u8);
        cmd.push(crc as u8);

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
            results.push(self.read()?);
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
        tracing::debug!("GPIO {} set to {} (mock)", gpio_num, if value { "1" } else { "0" });
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
        let handler = Arc::new(device_trait::ModbusHandler::new(0x01, device_trait::CrcMode::Crc16Modbus));
        Rs485Device::new("test_ttu_001".to_string(), "ttu".to_string(), config, handler)
    }

    #[test]
    fn test_device_creation() {
        let device = create_test_device();
        assert_eq!(device.device_id(), "test_ttu_001");
        assert_eq!(device.device_type(), "ttu");
    }

    #[test]
    fn test_device_status_offline() {
        let device = create_test_device();
        let status = device.status().unwrap();
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
}