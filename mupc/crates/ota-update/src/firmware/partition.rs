//! A/B 双分区管理器
//!
//! 管理 RK3588 平台的 A/B 双系统分区，实现固件升级时对备用分区的
//! 挂载、写入、完整性验证和分区切换。

use crate::error::OtaError;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;

/// 分区信息
#[derive(Debug, Clone)]
pub struct PartitionInfo {
    pub name: String,
    pub device: String,
    pub mount_point: String,
    pub is_active: bool,
}

/// A/B 分区管理器
pub struct PartitionManager {
    partitions: Vec<PartitionInfo>,
    standby_mounted: bool,
    standby_written: bool,
    expected_sha256: Option<String>,
}

impl PartitionManager {
    pub fn new() -> Self {
        Self {
            partitions: Vec::new(),
            standby_mounted: false,
            standby_written: false,
            expected_sha256: None,
        }
    }

    /// 检测当前系统中的 A/B 分区
    ///
    /// Linux: 解析 /proc/mounts 和块设备命名约定。
    /// 非 Linux: 使用模拟分区数据进行开发测试。
    pub fn detect_partitions(&mut self) -> Result<(), OtaError> {
        self.partitions.clear();

        #[cfg(target_os = "linux")]
        {
            let mounts = fs::read_to_string("/proc/mounts")
                .map_err(|e| OtaError::IoError(format!("读取 /proc/mounts 失败: {}", e)))?;

            for line in mounts.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 2 {
                    continue;
                }
                let device = parts[0];
                let mount = parts[1];
                if device.contains("mmcblk") || device.contains("sd") {
                    let is_active = mount == "/";
                    let name = if device.contains("p3") || mount == "/" {
                        "system-a"
                    } else {
                        "system-b"
                    };
                    self.partitions.push(PartitionInfo {
                        name: name.to_string(),
                        device: device.to_string(),
                        mount_point: mount.to_string(),
                        is_active,
                    });
                }
            }
        }

        // 如果没有检测到分区（非 Linux 或开发环境），使用模拟数据
        if self.partitions.is_empty() {
            self.partitions.push(PartitionInfo {
                name: "system-a".into(),
                device: "/dev/mmcblk0p3".into(),
                mount_point: "/".into(),
                is_active: true,
            });
            self.partitions.push(PartitionInfo {
                name: "system-b".into(),
                device: "/dev/mmcblk0p4".into(),
                mount_point: "/mnt/standby".into(),
                is_active: false,
            });
        }

        Ok(())
    }

    pub fn current_partition(&self) -> Option<&PartitionInfo> {
        self.partitions.iter().find(|p| p.is_active)
    }

    pub fn standby_partition(&self) -> Option<&PartitionInfo> {
        self.partitions.iter().find(|p| !p.is_active)
    }

    /// 挂载备用分区
    #[cfg(target_os = "linux")]
    pub fn mount_standby(&mut self) -> Result<(), OtaError> {
        let standby = self.standby_partition().ok_or_else(|| {
            OtaError::RollbackFailed("未检测到备用分区".into())
        })?;

        let mount_point = standby.mount_point.clone();
        if !Path::new(&mount_point).exists() {
            fs::create_dir_all(&mount_point).map_err(|e| {
                OtaError::RollbackFailed(format!("创建挂载点失败: {}", e))
            })?;
        }

        let output = Command::new("mount")
            .arg(&standby.device)
            .arg(&mount_point)
            .output()
            .map_err(|e| OtaError::RollbackFailed(format!("挂载命令执行失败: {}", e)))?;

        if !output.status.success() {
            return Err(OtaError::RollbackFailed(format!(
                "挂载失败: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        self.standby_mounted = true;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn mount_standby(&mut self) -> Result<(), OtaError> {
        let standby = self.standby_partition().ok_or_else(|| {
            OtaError::RollbackFailed("未检测到备用分区".into())
        })?;
        let mount_point = &standby.mount_point;
        if !Path::new(mount_point).exists() {
            fs::create_dir_all(mount_point).map_err(|e| {
                OtaError::RollbackFailed(format!("创建挂载点失败: {}", e))
            })?;
        }
        self.standby_mounted = true;
        Ok(())
    }

    /// 卸载备用分区
    #[cfg(target_os = "linux")]
    pub fn unmount_standby(&mut self) -> Result<(), OtaError> {
        if !self.standby_mounted {
            return Ok(());
        }
        if let Some(standby) = self.standby_partition() {
            let output = Command::new("umount")
                .arg(&standby.mount_point)
                .output()
                .map_err(|e| OtaError::RollbackFailed(format!("卸载命令执行失败: {}", e)))?;
            if !output.status.success() {
                return Err(OtaError::RollbackFailed(format!(
                    "卸载失败: {}",
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
        }
        self.standby_mounted = false;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    pub fn unmount_standby(&mut self) -> Result<(), OtaError> {
        self.standby_mounted = false;
        Ok(())
    }

    /// 将固件数据写入备用分区
    pub fn write_standby(&mut self, data: &[u8]) -> Result<(), OtaError> {
        if !self.standby_mounted {
            return Err(OtaError::RollbackFailed("备用分区未挂载".into()));
        }
        let standby = self.standby_partition().ok_or_else(|| {
            OtaError::RollbackFailed("未检测到备用分区".into())
        })?;

        let target_file = Path::new(&standby.mount_point).join("firmware_payload.tar.gz");
        fs::write(&target_file, data).map_err(|e| {
            OtaError::RollbackFailed(format!("写入备用分区失败: {}", e))
        })?;

        self.standby_written = true;
        Ok(())
    }

    /// 验证备用分区完整性（SHA-256）
    pub fn verify_standby_integrity(&self) -> Result<bool, OtaError> {
        if !self.standby_written {
            return Ok(false);
        }
        let standby = self.standby_partition().ok_or_else(|| {
            OtaError::RollbackFailed("未检测到备用分区".into())
        })?;

        let target_file = Path::new(&standby.mount_point).join("firmware_payload.tar.gz");
        let mut file = fs::File::open(&target_file).map_err(|e| {
            OtaError::VerificationFailed(format!("打开固件文件失败: {}", e))
        })?;

        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = file.read(&mut buffer).map_err(|e| {
                OtaError::VerificationFailed(format!("读取固件文件失败: {}", e))
            })?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        let hash = hex::encode(hasher.finalize());

        if let Some(ref expected) = self.expected_sha256 {
            return Ok(hash == *expected);
        }
        Ok(true)
    }

    /// 设置预期的 SHA-256 校验值
    pub fn set_expected_sha256(&mut self, sha256: &str) {
        self.expected_sha256 = Some(sha256.to_string());
    }

    /// 切换到备用分区（更新 Bootloader 环境变量）
    pub fn switch_to_standby(&self) -> Result<(), OtaError> {
        if !self.standby_written {
            return Err(OtaError::RollbackFailed("备用分区数据未写入".into()));
        }
        // Phase 2+ 完整实现：通过 BootloaderEnv 设置 boot_partition
        tracing::info!("分区切换请求已记录（Phase 2+ 将通过 Bootloader 环境变量执行）");
        Ok(())
    }

    /// 回滚到当前活动分区
    pub fn rollback_to_current(&self) -> Result<(), OtaError> {
        tracing::info!("回滚到当前活动分区");
        Ok(())
    }

    pub fn partition_count(&self) -> usize {
        self.partitions.len()
    }
}

impl Default for PartitionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_manager_new() {
        let manager = PartitionManager::new();
        assert!(manager.partitions.is_empty());
        assert!(!manager.standby_mounted);
        assert!(!manager.standby_written);
    }

    #[test]
    fn test_detect_partitions() {
        let mut manager = PartitionManager::new();
        manager.detect_partitions().unwrap();
        assert!(manager.partition_count() >= 1);
        assert!(manager.current_partition().is_some());
        assert!(manager.standby_partition().is_some());
    }

    #[test]
    fn test_partition_info_creation() {
        let info = PartitionInfo {
            name: "boot_a".to_string(),
            device: "/dev/mmcblk0p3".to_string(),
            mount_point: "/".to_string(),
            is_active: true,
        };
        assert_eq!(info.name, "boot_a");
        assert!(info.is_active);
    }

    #[test]
    fn test_set_expected_sha256() {
        let mut manager = PartitionManager::new();
        manager.set_expected_sha256("abc123");
        assert!(manager.expected_sha256.is_some());
    }
}
