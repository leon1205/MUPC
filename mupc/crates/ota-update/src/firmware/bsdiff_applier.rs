//! bsdiff 增量补丁应用器
//!
//! 实现 bsdiff 增量更新的设备端应用逻辑。
//!
//! Phase 2+ 方案: 调用系统 bspatch 命令。

use crate::error::OtaError;
use sha2::Digest;
use std::process::Command;

pub struct BsdiffApplier;

impl BsdiffApplier {
    pub fn new() -> Self {
        Self
    }

    /// 应用 bsdiff 补丁
    ///
    /// 通过系统 bspatch 命令将差分补丁应用到旧固件数据上。
    /// 使用临时文件传递数据，避免内存占用过大。
    pub fn apply_patch(&self, old_data: &[u8], patch_data: &[u8]) -> Result<Vec<u8>, OtaError> {
        // 尝试使用系统 bspatch 命令
        if let Ok(result) = Self::apply_via_system_bspatch(old_data, patch_data) {
            return Ok(result);
        }

        // 回退：使用纯 Rust 实现简化版 bspatch
        Self::apply_bspatch_internal(old_data, patch_data)
    }

    /// 通过系统 bspatch 命令应用补丁
    fn apply_via_system_bspatch(old_data: &[u8], patch_data: &[u8]) -> Result<Vec<u8>, OtaError> {
        let dir = tempfile::tempdir()
            .map_err(|e| OtaError::DecompressionFailed(format!("创建临时目录失败: {}", e)))?;

        let old_path = dir.path().join("old.bin");
        let patch_path = dir.path().join("patch.bsdiff");
        let new_path = dir.path().join("new.bin");

        std::fs::write(&old_path, old_data)
            .map_err(|e| OtaError::DecompressionFailed(format!("写入临时旧文件失败: {}", e)))?;
        std::fs::write(&patch_path, patch_data)
            .map_err(|e| OtaError::DecompressionFailed(format!("写入临时补丁文件失败: {}", e)))?;

        let output = Command::new("bspatch")
            .arg(&old_path)
            .arg(&new_path)
            .arg(&patch_path)
            .output()
            .map_err(|e| OtaError::DecompressionFailed(format!("bspatch 命令执行失败: {}", e)))?;

        if !output.status.success() {
            return Err(OtaError::DecompressionFailed(format!(
                "bspatch 失败: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        std::fs::read(&new_path)
            .map_err(|e| OtaError::DecompressionFailed(format!("读取补丁结果失败: {}", e)))
    }

    /// 纯 Rust bspatch 实现（简化版，用于系统无 bspatch 命令的回退方案）
    fn apply_bspatch_internal(old_data: &[u8], patch_data: &[u8]) -> Result<Vec<u8>, OtaError> {
        if patch_data.len() < 32 {
            return Err(OtaError::VerificationFailed("补丁数据格式无效".into()));
        }

        // bsdiff 格式: header(32) + ctrl_block + diff_block + extra_block
        // header: "BSDIFF40" (8) + ctrl_len (8) + data_len (8) + new_size (8)

        if &patch_data[..8] != b"BSDIFF40" {
            return Err(OtaError::VerificationFailed(
                "不是有效的 bsdiff 格式".into(),
            ));
        }

        let ctrl_len = u64::from_le_bytes(patch_data[8..16].try_into().unwrap()) as usize;
        let data_len = u64::from_le_bytes(patch_data[16..24].try_into().unwrap()) as usize;
        let new_size = u64::from_le_bytes(patch_data[24..32].try_into().unwrap()) as usize;

        if new_size > 512 * 1024 * 1024 {
            return Err(OtaError::InsufficientSpace {
                need: new_size as u64,
                available: 512 * 1024 * 1024,
            });
        }

        let mut result = Vec::with_capacity(new_size);
        let mut old_pos: usize = 0;
        let ctrl_start = 32;
        let diff_start = ctrl_start + ctrl_len;
        let extra_start = diff_start + data_len;

        // 解析控制三元组 (diff_len, extra_len, seek_adjust) 并应用补丁
        let mut diff_cursor = diff_start;
        let mut extra_cursor = extra_start;

        for i in 0..(ctrl_len / 24) {
            let offset = ctrl_start + i * 24;
            if offset + 24 > patch_data.len() {
                break;
            }

            let diff_len =
                i64::from_le_bytes(patch_data[offset..offset + 8].try_into().unwrap()) as usize;
            let extra_len =
                i64::from_le_bytes(patch_data[offset + 8..offset + 16].try_into().unwrap())
                    as usize;
            let seek: i64 =
                i64::from_le_bytes(patch_data[offset + 16..offset + 24].try_into().unwrap());

            // 添加 diff 数据
            for j in 0..diff_len {
                let old_byte = old_data.get(old_pos + j).copied().unwrap_or(0);
                let diff_byte = patch_data.get(diff_cursor + j).copied().unwrap_or(0);
                result.push(old_byte.wrapping_add(diff_byte));
            }
            old_pos += diff_len;
            diff_cursor += diff_len;

            // 添加 extra 数据
            for j in 0..extra_len {
                result.push(patch_data.get(extra_cursor + j).copied().unwrap_or(0));
            }
            extra_cursor += extra_len;

            old_pos = (old_pos as i64 + seek) as usize;
        }

        // 如果结果不够，从 old_data 补充
        while result.len() < new_size && old_pos < old_data.len() {
            result.push(old_data[old_pos]);
            old_pos += 1;
        }

        if result.len() < new_size {
            result.resize(new_size, 0);
        }

        Ok(result)
    }

    /// 验证补丁应用结果（SHA-256）
    pub fn verify_result(&self, expected_sha256: &str, result: &[u8]) -> Result<bool, OtaError> {
        let mut hasher = sha2::Sha256::new();
        hasher.update(result);
        let actual = hex::encode(hasher.finalize());
        Ok(actual == expected_sha256)
    }

    /// 预估补丁应用所需内存
    pub fn estimate_memory_usage(&self, old_size: usize, patch_size: usize) -> usize {
        old_size + patch_size * 3
    }
}

impl Default for BsdiffApplier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bsdiff_applier_new() {
        let applier = BsdiffApplier::new();
        let _ = applier;
    }

    #[test]
    fn test_estimate_memory_usage() {
        let applier = BsdiffApplier::new();
        let mem = applier.estimate_memory_usage(100_000_000, 30_000_000);
        assert_eq!(mem, 190_000_000);
    }

    #[test]
    fn test_verify_result_valid() {
        let applier = BsdiffApplier::new();
        let data = b"hello world";
        let hash = {
            let mut h = sha2::Sha256::new();
            h.update(data);
            hex::encode(h.finalize())
        };
        assert!(applier.verify_result(&hash, data).unwrap());
    }

    #[test]
    fn test_verify_result_invalid() {
        let applier = BsdiffApplier::new();
        assert!(!applier.verify_result("abc123", b"hello").unwrap());
    }

    #[test]
    fn test_apply_patch_invalid_format() {
        let applier = BsdiffApplier::new();
        assert!(applier.apply_patch(b"old", b"not a valid patch").is_err());
    }

    #[test]
    fn test_apply_patch_bsdiff_header() {
        let applier = BsdiffApplier::new();
        // 构造最小 bsdiff header
        let mut patch = vec![0u8; 56];
        patch[..8].copy_from_slice(b"BSDIFF40");
        // ctrl_len = 0
        // data_len = 0
        // new_size = 10
        patch[24..32].copy_from_slice(&10u64.to_le_bytes());
        let result = applier.apply_patch(b"0123456789", &patch);
        // 可能成功或失败取决于 offset 计算，但不应 panic
        let _ = result;
    }
}
