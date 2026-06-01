//! RKNN Runtime 类型定义
//!
//! 封装 RKNN C API 的数据结构，用于 FFI 边界

use std::os::raw::c_int;

/// RKNN 输入结构
///
/// 对应 librknnrt.so 的 rknn_input 结构
#[derive(Debug, Clone)]
pub struct RknnInput {
    /// 输入索引
    pub index: u32,
    /// 输入数据缓冲区
    pub buf: Vec<u8>,
    /// 时间戳传递标志
    ///
    /// 0: 不传递时间戳（默认）
    /// 1: 传递时间戳
    pub pass_timestamp: c_int,
}

impl RknnInput {
    /// 创建新的 RKNN 输入
    pub fn new(index: u32, buf: Vec<u8>) -> Self {
        Self {
            index,
            buf,
            pass_timestamp: 0, // 默认不传递时间戳
        }
    }
}

/// RKNN 输出结构
///
/// 对应 librknnrt.so 的 rknn_output 结构
#[derive(Debug)]
pub struct RknnOutput {
    /// 输出数据缓冲区
    pub buf: Vec<u8>,
}

impl RknnOutput {
    /// 安全地将输出缓冲区转换为 f32 数组
    ///
    /// 使用 `align_to` 处理 Vec<u8> 可能不满足 4 字节对齐的情况
    pub fn as_f32(&self) -> Vec<f32> {
        let (prefix, aligned, suffix) = unsafe { self.buf.align_to::<f32>() };
        let mut result = Vec::with_capacity(prefix.len() / 4 + aligned.len() + suffix.len() / 4);
        // 处理前缀中 4 字节对齐的部分
        for chunk in prefix.chunks_exact(4) {
            result.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        // 直接拷贝已对齐的数据
        result.extend(aligned.iter());
        // 处理后缀中 4 字节对齐的部分
        for chunk in suffix.chunks_exact(4) {
            result.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rknn_input_new() {
        let input = RknnInput::new(0, vec![1, 2, 3, 4]);
        assert_eq!(input.index, 0);
        assert_eq!(input.buf, vec![1, 2, 3, 4]);
        assert_eq!(input.pass_timestamp, 0);
    }

    #[test]
    fn test_rknn_output_as_f32_aligned() {
        // 4 字节对齐的数据
        let buf = vec![0u8; 16];
        let output = RknnOutput { buf };
        let floats = output.as_f32();
        assert_eq!(floats.len(), 4);
    }

    #[test]
    fn test_rknn_output_as_f32_unaligned() {
        // 非 4 字节对齐的数据（模拟 Realtek RKNN SDK 输出）
        let buf = vec![0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let output = RknnOutput { buf };
        let floats = output.as_f32();
        assert_eq!(floats.len(), 3);
    }

    #[test]
    fn test_rknn_output_as_f32_exact_alignment() {
        // 正好 4 字节对齐的情况
        let buf: Vec<u8> = (0..12).collect();
        let output = RknnOutput { buf };
        let floats = output.as_f32();
        assert_eq!(floats.len(), 3);
        // 验证数据正确性
        assert_eq!(floats[0], f32::from_le_bytes([0, 1, 2, 3]));
        assert_eq!(floats[1], f32::from_le_bytes([4, 5, 6, 7]));
        assert_eq!(floats[2], f32::from_le_bytes([8, 9, 10, 11]));
    }
}
