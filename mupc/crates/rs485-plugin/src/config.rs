//! RS485 配置解析

use device_trait::{CrcMode, Parity, Rs485Config};
use serde::{Deserialize, Serialize};

/// RS485 设备配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 串口设备路径
    pub port: String,
    /// 波特率
    pub baud_rate: u32,
    /// 数据位
    pub data_bits: u8,
    /// 停止位
    pub stop_bits: u8,
    /// 校验位
    pub parity: Parity,
    /// 通信超时（毫秒）
    pub timeout_ms: u64,
    /// 设备地址
    pub device_addr: u8,
    /// CRC 校验模式
    pub crc_mode: CrcMode,
    /// DE 引脚 (Driver Enable) - 控制发送使能
    pub de_gpio: Option<u32>,
    /// RE 引脚 (Receiver Enable) - 控制接收使能
    pub re_gpio: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: "/dev/ttyUSB0".to_string(),
            baud_rate: 9600,
            data_bits: 8,
            stop_bits: 1,
            parity: Parity::None,
            timeout_ms: 1000,
            device_addr: 0x01,
            crc_mode: CrcMode::Crc16Modbus,
            de_gpio: None,
            re_gpio: None,
        }
    }
}

impl Config {
    /// 从文件加载配置
    pub fn from_file(path: &std::path::Path) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(config)
    }

    /// 从 JSON 字符串解析配置
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// 转换为 Rs485Config
    pub fn to_rs485_config(&self) -> Rs485Config {
        Rs485Config {
            port: self.port.clone(),
            baud_rate: self.baud_rate,
            data_bits: self.data_bits,
            stop_bits: self.stop_bits,
            parity: self.parity,
            timeout_ms: self.timeout_ms,
            device_addr: self.device_addr,
            crc_mode: self.crc_mode,
            de_gpio: self.de_gpio,
            re_gpio: self.re_gpio,
        }
    }
}

/// 配置验证
impl Config {
    /// 验证配置有效性
    pub fn validate(&self) -> Result<(), String> {
        if self.baud_rate == 0 {
            return Err("波特率不能为 0".to_string());
        }
        if self.data_bits < 5 || self.data_bits > 8 {
            return Err("数据位必须在 5-8 之间".to_string());
        }
        if self.stop_bits < 1 || self.stop_bits > 2 {
            return Err("停止位必须在 1-2 之间".to_string());
        }
        // DE 和 RE 引脚不能相同
        if let (Some(de), Some(re)) = (self.de_gpio, self.re_gpio) {
            if de == re {
                return Err("DE 和 RE 引脚不能相同".to_string());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.baud_rate, 9600);
        assert_eq!(config.data_bits, 8);
        assert_eq!(config.device_addr, 0x01);
        assert_eq!(config.de_gpio, None);
        assert_eq!(config.re_gpio, None);
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        assert!(config.validate().is_ok());

        config.baud_rate = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_from_json() {
        let json = r#"{
            "port": "/dev/ttyUSB0",
            "baud_rate": 19200,
            "data_bits": 8,
            "stop_bits": 1,
            "parity": "even",
            "timeout_ms": 2000,
            "device_addr": 2,
            "crc_mode": "crc16_modbus",
            "de_gpio": 17,
            "re_gpio": 27
        }"#;
        let config = Config::from_json(json).unwrap();
        assert_eq!(config.baud_rate, 19200);
        assert_eq!(config.device_addr, 2);
        assert_eq!(config.de_gpio, Some(17));
        assert_eq!(config.re_gpio, Some(27));
    }

    #[test]
    fn test_config_with_gpio() {
        let json = r#"{
            "port": "/dev/ttyUSB1",
            "baud_rate": 115200,
            "de_gpio": 18,
            "re_gpio": 22
        }"#;
        let config = Config::from_json(json).unwrap();
        assert_eq!(config.de_gpio, Some(18));
        assert_eq!(config.re_gpio, Some(22));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validate_same_gpio() {
        let mut config = Config::default();
        config.de_gpio = Some(17);
        config.re_gpio = Some(17);
        let result = config.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "DE 和 RE 引脚不能相同");
    }
}