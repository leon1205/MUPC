//! 数字 IO 抽象（BECG-3568 DI/DO，sysfs 先落地；gpiod 后端接口预留）。
//! sysfs 路径 /sys/class/gpio/gpio{num}/{direction,value}；DI 读原始电平（1/0 硬件态，
//! active_low 语义由上层按配置反相），DO 写原始电平（active_high 由上层决定 0/1）。

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("GPIO 初始化失败 {0}: {1}")]
    Init(String, String),
    #[error("GPIO 读写失败 {0}: {1}")]
    Io(String, String),
}

/// 数字输入（光耦 DI）
pub trait DigitalIn: Send + Sync {
    fn read_level(&self) -> Result<bool, IoError>; // true = 硬件高电平
}

/// 数字输出（继电器 DO）
pub trait DigitalOut: Send + Sync {
    fn set_level(&self, high: bool) -> Result<(), IoError>;
}

/// sysfs 输入：export + direction in + 读 value
pub struct SysfsIn { path: PathBuf }
impl SysfsIn {
    pub fn new(gpio: u32) -> Result<Self, IoError> {
        let gpio_dir = PathBuf::from(format!("/sys/class/gpio/gpio{gpio}"));
        if !gpio_dir.exists() {
            std::fs::write("/sys/class/gpio/export", gpio.to_string())
                .map_err(|e| IoError::Init(format!("export gpio{gpio}"), e.to_string()))?;
            // export 后等待 udev 建目录
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::fs::write(gpio_dir.join("direction"), "in")
            .map_err(|e| IoError::Init(format!("dir gpio{gpio}"), e.to_string()))?;
        Ok(Self { path: gpio_dir.join("value") })
    }
}
impl DigitalIn for SysfsIn {
    fn read_level(&self) -> Result<bool, IoError> {
        let v = std::fs::read_to_string(&self.path)
            .map_err(|e| IoError::Io(self.path.display().to_string(), e.to_string()))?;
        Ok(v.trim() == "1")
    }
}

/// sysfs 输出：export + direction out + 写 value（active 电平由上层换算）
pub struct SysfsOut { path: PathBuf }
impl SysfsOut {
    pub fn new(gpio: u32) -> Result<Self, IoError> {
        let gpio_dir = PathBuf::from(format!("/sys/class/gpio/gpio{gpio}"));
        if !gpio_dir.exists() {
            std::fs::write("/sys/class/gpio/export", gpio.to_string())
                .map_err(|e| IoError::Init(format!("export gpio{gpio}"), e.to_string()))?;
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::fs::write(gpio_dir.join("direction"), "out")
            .map_err(|e| IoError::Init(format!("dir gpio{gpio}"), e.to_string()))?;
        Ok(Self { path: gpio_dir.join("value") })
    }
}
impl DigitalOut for SysfsOut {
    fn set_level(&self, high: bool) -> Result<(), IoError> {
        std::fs::write(&self.path, if high { "1" } else { "0" })
            .map_err(|e| IoError::Io(self.path.display().to_string(), e.to_string()))
    }
}

/// 测试 mock（内存电平）
pub struct MockIn { level: std::sync::RwLock<bool> }
impl MockIn {
    pub fn new() -> Self { Self { level: std::sync::RwLock::new(false) } }
    pub fn set(&self, high: bool) { *self.level.write().unwrap() = high; }
}
impl DigitalIn for MockIn {
    fn read_level(&self) -> Result<bool, IoError> { Ok(*self.level.read().unwrap()) }
}
pub struct MockOut { level: std::sync::RwLock<bool> }
impl MockOut {
    pub fn new() -> Self { Self { level: std::sync::RwLock::new(false) } }
    pub fn get(&self) -> bool { *self.level.read().unwrap() }
}
impl DigitalOut for MockOut {
    fn set_level(&self, high: bool) -> Result<(), IoError> {
        *self.level.write().unwrap() = high; Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mock_in_out_roundtrip() {
        let o = MockOut::new(); o.set_level(true).unwrap(); assert!(o.get());
        o.set_level(false).unwrap(); assert!(!o.get());
        let i = MockIn::new(); assert!(!i.read_level().unwrap()); i.set(true); assert!(i.read_level().unwrap());
    }
}
