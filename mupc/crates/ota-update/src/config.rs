//! OTA 更新配置
//!
//! Phase 3C.2 OTA 模型自动更新模块的配置管理

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// OTA 配置错误类型
#[derive(Debug, Error)]
pub enum OtaConfigError {
    #[error("无效的时间格式: {0}, 期望 HH:MM")]
    InvalidTimeFormat(String),

    #[error("无效的小时: {0}")]
    InvalidHour(String),

    #[error("小时超出范围: {0}")]
    HourOutOfRange(u32),

    #[error("分钟超出范围: {0}")]
    MinuteOutOfRange(u32),

    #[error("服务器地址不能为空")]
    EmptyServerUrl,

    #[error("检查间隔必须大于 0")]
    InvalidCheckInterval,

    #[error("下载超时必须大于 0")]
    InvalidDownloadTimeout,

    #[error("重试次数不能超过 10")]
    RetryCountExceeded,

    #[error("回滚次数上限不能超过 10")]
    MaxRollbackCountExceeded,

    #[error("公钥路径不能为空")]
    EmptyPublicKeyPath,

    #[error("模型存储路径不能为空")]
    EmptyModelStoragePath,
}

/// OTA 更新配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtaConfig {
    /// OTA 服务器地址
    pub server_url: String,

    /// 检查更新间隔（秒）
    pub check_interval: u64,

    /// 下载窗口开始时间 (HH:MM 格式)
    pub download_window_start: String,

    /// 下载窗口结束时间 (HH:MM 格式)
    pub download_window_end: String,

    /// 自动下载新版本
    pub auto_download: bool,

    /// 自动应用已下载的更新
    pub auto_apply: bool,

    /// 下载超时时间（秒）
    pub download_timeout: u64,

    /// 下载重试次数
    pub retry_count: u32,

    /// 最大回滚次数
    pub max_rollback_count: u32,

    /// 签名公钥路径
    pub public_key_path: String,

    /// 模型存储路径
    pub model_storage_path: String,
}

impl Default for OtaConfig {
    fn default() -> Self {
        Self {
            server_url: "https://ota.example.com".to_string(),
            check_interval: 3600,
            download_window_start: "02:00".to_string(),
            download_window_end: "05:00".to_string(),
            auto_download: true,
            auto_apply: true,
            download_timeout: 300,
            retry_count: 3,
            max_rollback_count: 3,
            public_key_path: "/etc/mupc/ota_public_key.pem".to_string(),
            model_storage_path: "/models".to_string(),
        }
    }
}

impl OtaConfig {
    /// 解析 HH:MM 格式为 (小时, 分钟)
    fn parse_hhmm(time_str: &str) -> Result<(u32, u32), OtaConfigError> {
        let parts: Vec<&str> = time_str.split(':').collect();
        if parts.len() != 2 {
            return Err(OtaConfigError::InvalidTimeFormat(time_str.to_string()));
        }

        let hour: u32 = parts[0]
            .parse()
            .map_err(|_| OtaConfigError::InvalidHour(parts[0].to_string()))?;
        let minute: u32 = parts[1]
            .parse()
            .map_err(|_| OtaConfigError::InvalidTimeFormat(time_str.to_string()))?;

        if hour > 23 {
            return Err(OtaConfigError::HourOutOfRange(hour));
        }

        if minute > 59 {
            return Err(OtaConfigError::MinuteOutOfRange(minute));
        }

        Ok((hour, minute))
    }

    /// 验证下载窗口时间格式
    pub fn validate_time_format(time_str: &str) -> Result<(), OtaConfigError> {
        Self::parse_hhmm(time_str).map(|_| ())
    }

    /// 验证配置的有效性
    pub fn validate(&self) -> Result<(), OtaConfigError> {
        // 验证服务器地址
        if self.server_url.is_empty() {
            return Err(OtaConfigError::EmptyServerUrl);
        }

        // 验证检查间隔
        if self.check_interval == 0 {
            return Err(OtaConfigError::InvalidCheckInterval);
        }

        // 验证下载窗口时间格式
        Self::validate_time_format(&self.download_window_start)?;
        Self::validate_time_format(&self.download_window_end)?;

        // 验证下载超时
        if self.download_timeout == 0 {
            return Err(OtaConfigError::InvalidDownloadTimeout);
        }

        // 验证重试次数
        if self.retry_count > 10 {
            return Err(OtaConfigError::RetryCountExceeded);
        }

        // 验证回滚次数上限
        if self.max_rollback_count > 10 {
            return Err(OtaConfigError::MaxRollbackCountExceeded);
        }

        // 验证公钥路径
        if self.public_key_path.is_empty() {
            return Err(OtaConfigError::EmptyPublicKeyPath);
        }

        // 验证模型存储路径
        if self.model_storage_path.is_empty() {
            return Err(OtaConfigError::EmptyModelStoragePath);
        }

        Ok(())
    }

    /// 解析下载窗口开始时间为分钟数
    pub fn parse_window_start_minutes(&self) -> Result<u32, OtaConfigError> {
        let (hour, minute) = Self::parse_hhmm(&self.download_window_start)?;
        Ok(hour * 60 + minute)
    }

    /// 解析下载窗口结束时间为分钟数
    pub fn parse_window_end_minutes(&self) -> Result<u32, OtaConfigError> {
        let (hour, minute) = Self::parse_hhmm(&self.download_window_end)?;
        Ok(hour * 60 + minute)
    }

    /// 将 HH:MM 格式转换为从午夜开始的分钟数
    fn time_to_minutes(time_str: &str) -> Result<u32, OtaConfigError> {
        let (hour, minute) = Self::parse_hhmm(time_str)?;
        Ok(hour * 60 + minute)
    }

    /// 判断当前时间是否在下载窗口内
    pub fn is_in_download_window(&self, current_hour: u32, current_minute: u32) -> bool {
        let current_minutes = current_hour * 60 + current_minute;
        let start_minutes = Self::time_to_minutes(&self.download_window_start).unwrap_or(0);
        let end_minutes = Self::time_to_minutes(&self.download_window_end).unwrap_or(0);

        if start_minutes <= end_minutes {
            // 同一天内的窗口（如 02:00 - 05:00）
            current_minutes >= start_minutes && current_minutes <= end_minutes
        } else {
            // 跨天的窗口（如 22:00 - 06:00）
            current_minutes >= start_minutes || current_minutes <= end_minutes
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = OtaConfig::default();

        assert_eq!(config.server_url, "https://ota.example.com");
        assert_eq!(config.check_interval, 3600);
        assert_eq!(config.download_window_start, "02:00");
        assert_eq!(config.download_window_end, "05:00");
        assert!(config.auto_download);
        assert!(config.auto_apply);
        assert_eq!(config.download_timeout, 300);
        assert_eq!(config.retry_count, 3);
        assert_eq!(config.max_rollback_count, 3);
        assert_eq!(config.public_key_path, "/etc/mupc/ota_public_key.pem");
        assert_eq!(config.model_storage_path, "/models");
    }

    #[test]
    fn test_validate_time_format_valid() {
        assert!(OtaConfig::validate_time_format("00:00").is_ok());
        assert!(OtaConfig::validate_time_format("23:59").is_ok());
        assert!(OtaConfig::validate_time_format("02:00").is_ok());
        assert!(OtaConfig::validate_time_format("12:30").is_ok());
    }

    #[test]
    fn test_validate_time_format_invalid() {
        // 无效格式
        assert!(OtaConfig::validate_time_format("").is_err());
        assert!(OtaConfig::validate_time_format("2:00").is_err());
        assert!(OtaConfig::validate_time_format("02:0").is_err());
        assert!(OtaConfig::validate_time_format("02").is_err());
        assert!(OtaConfig::validate_time_format("24:00").is_err());
        assert!(OtaConfig::validate_time_format("12:60").is_err());
        assert!(OtaConfig::validate_time_format("abc").is_err());
        assert!(OtaConfig::validate_time_format("12:30:00").is_err());
    }

    #[test]
    fn test_validate_valid_config() {
        let config = OtaConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_empty_server_url() {
        let mut config = OtaConfig::default();
        config.server_url = "".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_zero_check_interval() {
        let mut config = OtaConfig::default();
        config.check_interval = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_download_timeout() {
        let mut config = OtaConfig::default();
        config.download_timeout = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_exceed_retry_count() {
        let mut config = OtaConfig::default();
        config.retry_count = 11;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_exceed_max_rollback_count() {
        let mut config = OtaConfig::default();
        config.max_rollback_count = 11;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_empty_public_key_path() {
        let mut config = OtaConfig::default();
        config.public_key_path = "".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_empty_model_storage_path() {
        let mut config = OtaConfig::default();
        config.model_storage_path = "".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_parse_window_times() {
        let config = OtaConfig::default();

        // 02:00 = 120 分钟
        assert_eq!(config.parse_window_start_minutes().unwrap(), 120);
        // 05:00 = 300 分钟
        assert_eq!(config.parse_window_end_minutes().unwrap(), 300);
    }

    #[test]
    fn test_is_in_download_window_same_day() {
        let config = OtaConfig::default();

        // 在窗口内
        assert!(config.is_in_download_window(2, 0)); // 02:00
        assert!(config.is_in_download_window(3, 30)); // 03:30
        assert!(config.is_in_download_window(5, 0)); // 05:00

        // 在窗口外
        assert!(!config.is_in_download_window(1, 0)); // 01:00
        assert!(!config.is_in_download_window(6, 0)); // 06:00
        assert!(!config.is_in_download_window(12, 0)); // 12:00
    }

    #[test]
    fn test_is_in_download_window_cross_midnight() {
        let mut config = OtaConfig::default();
        config.download_window_start = "22:00".to_string();
        config.download_window_end = "06:00".to_string();

        // 在窗口内 (跨天)
        assert!(config.is_in_download_window(22, 0)); // 22:00
        assert!(config.is_in_download_window(23, 30)); // 23:30
        assert!(config.is_in_download_window(0, 0)); // 00:00
        assert!(config.is_in_download_window(6, 0)); // 06:00

        // 在窗口外
        assert!(!config.is_in_download_window(7, 0)); // 07:00
        assert!(!config.is_in_download_window(12, 0)); // 12:00
        assert!(!config.is_in_download_window(21, 0)); // 21:00
    }

    #[test]
    fn test_config_serialization() {
        let config = OtaConfig::default();

        // 测试 JSON 序列化
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("https://ota.example.com"));
        assert!(json.contains("3600"));

        // 测试 JSON 反序列化
        let deserialized: OtaConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.server_url, config.server_url);
        assert_eq!(deserialized.check_interval, config.check_interval);
    }

    #[test]
    fn test_config_clone() {
        let config = OtaConfig::default();
        let cloned = config.clone();

        assert_eq!(cloned.server_url, config.server_url);
        assert_eq!(cloned.check_interval, config.check_interval);
        assert_eq!(cloned.download_window_start, config.download_window_start);
        assert_eq!(cloned.download_window_end, config.download_window_end);
        assert_eq!(cloned.auto_download, config.auto_download);
        assert_eq!(cloned.auto_apply, config.auto_apply);
        assert_eq!(cloned.download_timeout, config.download_timeout);
        assert_eq!(cloned.retry_count, config.retry_count);
        assert_eq!(cloned.max_rollback_count, config.max_rollback_count);
        assert_eq!(cloned.public_key_path, config.public_key_path);
        assert_eq!(cloned.model_storage_path, config.model_storage_path);
    }

    #[test]
    fn test_custom_config() {
        let config = OtaConfig {
            server_url: "https://custom-ota.example.com".to_string(),
            check_interval: 7200,
            download_window_start: "01:00".to_string(),
            download_window_end: "04:00".to_string(),
            auto_download: false,
            auto_apply: false,
            download_timeout: 600,
            retry_count: 5,
            max_rollback_count: 5,
            public_key_path: "/custom/path/key.pem".to_string(),
            model_storage_path: "/custom/models".to_string(),
        };

        assert!(config.validate().is_ok());
        assert_eq!(config.server_url, "https://custom-ota.example.com");
        assert_eq!(config.check_interval, 7200);
        assert!(!config.auto_download);
        assert!(!config.auto_apply);
    }
}
