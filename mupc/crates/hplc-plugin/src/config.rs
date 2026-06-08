//! HPLC 设备配置
//!
//! 定义 HPLC 设备的配置参数

use serde::{Deserialize, Serialize};

/// HPLC 设备配置
///
/// 用于配置 HPLC 驱动的串口参数和芯片选项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HplcConfig {
    /// 串口路径（支持跨平台：Linux=/dev/ttyUSB0, Windows=COM3）
    #[serde(alias = "serial_port", alias = "com_port")]
    pub port: String,

    /// 波特率
    pub baud_rate: u32,

    /// 芯片型号（FFI 预留）
    pub chip_type: Option<String>,

    /// 通道号
    pub channel: Option<u8>,
}

impl HplcConfig {
    /// 创建新的 HPLC 配置
    ///
    /// # Arguments
    /// - `port`: 串口路径
    /// - `baud_rate`: 波特率
    ///
    /// # Example
    /// ```
    /// let config = HplcConfig::new("/dev/ttyUSB0", 115200);
    /// ```
    pub fn new(port: impl Into<String>, baud_rate: u32) -> Self {
        Self {
            port: port.into(),
            baud_rate,
            chip_type: None,
            channel: None,
        }
    }

    /// 设置芯片型号
    ///
    /// # Arguments
    /// - `chip_type`: 芯片型号（如 "G3", "GDM" 等）
    pub fn with_chip_type(mut self, chip_type: impl Into<String>) -> Self {
        self.chip_type = Some(chip_type.into());
        self
    }

    /// 设置通道号
    ///
    /// # Arguments
    /// - `channel`: 通道号（0-255）
    pub fn with_channel(mut self, channel: u8) -> Self {
        self.channel = Some(channel);
        self
    }
}

impl Default for HplcConfig {
    fn default() -> Self {
        Self {
            port: "/dev/ttyUSB0".to_string(),
            baud_rate: 115200,
            chip_type: None,
            channel: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hplc_config_new() {
        let config = HplcConfig::new("/dev/ttyUSB0", 115200);
        assert_eq!(config.port, "/dev/ttyUSB0");
        assert_eq!(config.baud_rate, 115200);
        assert_eq!(config.chip_type, None);
        assert_eq!(config.channel, None);
    }

    #[test]
    fn test_hplc_config_with_options() {
        let config = HplcConfig::new("COM3", 9600)
            .with_chip_type("G3")
            .with_channel(1);
        assert_eq!(config.port, "COM3");
        assert_eq!(config.chip_type, Some("G3".to_string()));
        assert_eq!(config.channel, Some(1));
    }

    #[test]
    fn test_hplc_config_default() {
        let config = HplcConfig::default();
        assert_eq!(config.port, "/dev/ttyUSB0");
        assert_eq!(config.baud_rate, 115200);
    }

    #[test]
    fn test_hplc_config_serde() {
        let config = HplcConfig::new("/dev/ttyUSB0", 115200).with_chip_type("GDM");

        let json = serde_json::to_string(&config).unwrap();
        let parsed: HplcConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.port, "/dev/ttyUSB0");
        assert_eq!(parsed.chip_type, Some("GDM".to_string()));
    }
}
