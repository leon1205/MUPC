//! Bootloader 环境变量操作
//!
//! 提供对 U-Boot 环境变量的读写操作，用于控制 A/B 分区启动选择。
//!
//! 开发阶段使用 JSON 文件模拟 env 存储，生产环境通过 fw_printenv/fw_setenv 命令操作。

use crate::error::OtaError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

const DEFAULT_ENV_CONFIG_PATH: &str = "/etc/fw_env.config";
const DEV_ENV_FILE: &str = "/tmp/mupc_bootloader_env.json";

pub const KEY_BOOT_PARTITION: &str = "boot_partition";
pub const KEY_BOOT_ATTEMPTS: &str = "boot_attempts";
pub const KEY_MAX_BOOT_ATTEMPTS: &str = "max_boot_attempts";
pub const KEY_OTA_STATUS: &str = "ota_status";

pub const OTA_STATUS_IDLE: &str = "idle";
pub const OTA_STATUS_UPDATED: &str = "updated";
pub const OTA_STATUS_ROLLBACK: &str = "rollback";
pub const OTA_STATUS_SAFE: &str = "safe";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EnvData {
    vars: HashMap<String, String>,
}

pub struct BootloaderEnv {
    env_path: String,
    cache: Mutex<HashMap<String, String>>,
}

impl BootloaderEnv {
    pub fn new() -> Self {
        let env = Self {
            env_path: DEFAULT_ENV_CONFIG_PATH.to_string(),
            cache: Mutex::new(HashMap::new()),
        };
        env.init_cache();
        env
    }

    pub fn with_config(env_path: impl Into<String>) -> Self {
        let env = Self {
            env_path: env_path.into(),
            cache: Mutex::new(HashMap::new()),
        };
        env.init_cache();
        env
    }

    fn init_cache(&self) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.insert(KEY_BOOT_PARTITION.to_string(), "a".to_string());
        cache.insert(KEY_BOOT_ATTEMPTS.to_string(), "0".to_string());
        cache.insert(KEY_MAX_BOOT_ATTEMPTS.to_string(), "3".to_string());
        cache.insert(KEY_OTA_STATUS.to_string(), OTA_STATUS_IDLE.to_string());
    }

    pub fn config_path(&self) -> &str {
        &self.env_path
    }

    /// 读取环境变量
    ///
    /// Linux 生产环境：调用 fw_printenv。
    /// 开发环境：从内存缓存读取。
    pub fn read(&self, key: &str) -> Result<String, OtaError> {
        #[cfg(target_os = "linux")]
        {
            // 尝试使用 fw_printenv 读取
            if let Ok(output) = std::process::Command::new("fw_printenv").arg(key).output() {
                if output.status.success() {
                    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !value.is_empty() {
                        return Ok(value);
                    }
                }
            }
        }

        // 回退到缓存读取
        let cache = self
            .cache
            .lock()
            .map_err(|e| OtaError::RollbackFailed(format!("锁获取失败: {}", e)))?;
        cache
            .get(key)
            .cloned()
            .ok_or_else(|| OtaError::VerificationFailed(format!("环境变量 {} 不存在", key)))
    }

    /// 写入环境变量
    pub fn write(&self, key: &str, value: &str) -> Result<(), OtaError> {
        #[cfg(target_os = "linux")]
        {
            if let Ok(output) = std::process::Command::new("fw_setenv")
                .arg(key)
                .arg(value)
                .output()
            {
                if output.status.success() {
                    // 同步更新缓存
                    let mut cache = self
                        .cache
                        .lock()
                        .map_err(|e| OtaError::RollbackFailed(format!("锁获取失败: {}", e)))?;
                    cache.insert(key.to_string(), value.to_string());
                    return Ok(());
                }
            }
        }

        // 回退到缓存写入
        let mut cache = self
            .cache
            .lock()
            .map_err(|e| OtaError::RollbackFailed(format!("锁获取失败: {}", e)))?;
        cache.insert(key.to_string(), value.to_string());
        self.persist_cache(&cache)?;
        Ok(())
    }

    /// 批量写入环境变量
    pub fn batch_write(&self, pairs: &[(&str, &str)]) -> Result<(), OtaError> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|e| OtaError::RollbackFailed(format!("锁获取失败: {}", e)))?;
        for (key, value) in pairs {
            cache.insert(key.to_string(), value.to_string());
        }
        self.persist_cache(&cache)?;
        Ok(())
    }

    fn persist_cache(&self, cache: &HashMap<String, String>) -> Result<(), OtaError> {
        let data = EnvData {
            vars: cache.clone(),
        };
        let json = serde_json::to_string(&data)
            .map_err(|e| OtaError::RollbackFailed(format!("序列化环境变量失败: {}", e)))?;
        fs::write(DEV_ENV_FILE, json)
            .map_err(|e| OtaError::RollbackFailed(format!("持久化环境变量失败: {}", e)))?;
        Ok(())
    }

    pub fn current_boot_partition(&self) -> Result<String, OtaError> {
        self.read(KEY_BOOT_PARTITION)
    }

    pub fn set_boot_partition(&self, partition: &str) -> Result<(), OtaError> {
        if partition != "a" && partition != "b" {
            return Err(OtaError::VerificationFailed(format!(
                "无效的分区标识: {}（期望 a 或 b）",
                partition
            )));
        }
        self.write(KEY_BOOT_PARTITION, partition)
    }

    pub fn ota_status(&self) -> Result<String, OtaError> {
        self.read(KEY_OTA_STATUS)
    }

    pub fn set_ota_status(&self, status: &str) -> Result<(), OtaError> {
        self.write(KEY_OTA_STATUS, status)
    }

    pub fn boot_attempts(&self) -> Result<String, OtaError> {
        self.read(KEY_BOOT_ATTEMPTS)
    }

    pub fn max_boot_attempts(&self) -> Result<String, OtaError> {
        self.read(KEY_MAX_BOOT_ATTEMPTS)
    }

    /// 检查 Bootloader 环境是否可访问
    pub fn is_accessible(&self) -> Result<bool, OtaError> {
        #[cfg(target_os = "linux")]
        {
            if Path::new(&self.env_path).exists() {
                return Ok(true);
            }
        }
        Ok(!self
            .cache
            .lock()
            .map_err(|e| OtaError::RollbackFailed(format!("锁获取失败: {}", e)))?
            .is_empty())
    }
}

impl Default for BootloaderEnv {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootloader_env_new() {
        let env = BootloaderEnv::new();
        assert_eq!(env.config_path(), DEFAULT_ENV_CONFIG_PATH);
        assert!(env.is_accessible().unwrap());
    }

    #[test]
    fn test_read_write_cached() {
        let env = BootloaderEnv::new();
        env.write("test_key", "test_value").unwrap();
        assert_eq!(env.read("test_key").unwrap(), "test_value");
    }

    #[test]
    fn test_set_boot_partition() {
        let env = BootloaderEnv::new();
        env.set_boot_partition("b").unwrap();
        assert_eq!(env.current_boot_partition().unwrap(), "b");
    }

    #[test]
    fn test_invalid_partition() {
        let env = BootloaderEnv::new();
        assert!(env.set_boot_partition("c").is_err());
    }

    #[test]
    fn test_batch_write() {
        let env = BootloaderEnv::new();
        env.batch_write(&[
            (KEY_BOOT_PARTITION, "b"),
            (KEY_OTA_STATUS, OTA_STATUS_UPDATED),
        ])
        .unwrap();
        assert_eq!(env.current_boot_partition().unwrap(), "b");
        assert_eq!(env.ota_status().unwrap(), OTA_STATUS_UPDATED);
    }

    #[test]
    fn test_constants() {
        assert_eq!(KEY_BOOT_PARTITION, "boot_partition");
        assert_eq!(OTA_STATUS_IDLE, "idle");
        assert_eq!(OTA_STATUS_UPDATED, "updated");
    }
}
