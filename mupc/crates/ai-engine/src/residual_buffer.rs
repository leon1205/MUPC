//! 残差滑动窗口缓冲
//!
//! 存储最近 T 步历史残差（actual - predicted），供误差修正 BiLSTM 使用。
//!
//! ## 设计要点
//!
//! - 每个预测对象（PV、Load）各维护一个独立的 `ResidualBuffer`
//! - 冷启动时若缓冲未满：`zero_init=true` 返回零向量（跳过修正），
//!   `zero_init=false` 返回错误（保守模式）
//! - 残差序列含 NaN/Inf 时替换为零值并记录 WARN

use std::collections::VecDeque;

/// 残差滑动窗口缓冲
///
/// 容量 = `residual_window_steps`（默认 24），FIFO 循环缓冲。
///
/// # 使用示例
///
/// ```ignore
/// let mut buf = ResidualBuffer::new(24, true);
/// buf.push(100.0, 95.0);  // 残差 = +5.0
/// buf.push(200.0, 210.0); // 残差 = -10.0
/// if buf.is_ready(24) {
///     let window = buf.get_window(24);
///     // 送入误差修正模型 ...
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ResidualBuffer {
    /// 容量 = residual_window_steps（默认 24）
    capacity: usize,
    /// 循环缓冲（FIFO）
    buffer: VecDeque<f32>,
    /// 冷启动零填充标志
    zero_init: bool,
    /// 累计推入次数（用于判断 is_ready）
    total_pushed: usize,
}

impl ResidualBuffer {
    /// 创建残差缓冲
    ///
    /// # 参数
    ///
    /// - `capacity`: 最大容量（= residual_window_steps）
    /// - `zero_init`: 冷启动时是否零向量填充
    pub fn new(capacity: usize, zero_init: bool) -> Self {
        assert!(capacity > 0, "ResidualBuffer capacity must be > 0");
        Self {
            capacity,
            buffer: VecDeque::with_capacity(capacity),
            zero_init,
            total_pushed: 0,
        }
    }

    /// 追加新残差
    ///
    /// 自动维护 FIFO 窗口：超出容量时丢弃最旧残差。
    /// 若 actual 或 predicted 含 NaN/Inf，替换为零值并记录 WARN。
    pub fn push(&mut self, actual: f32, predicted: f32) {
        let residual = if actual.is_nan() || actual.is_infinite()
            || predicted.is_nan() || predicted.is_infinite()
        {
            tracing::warn!(
                "残差缓冲: 检测到 NaN/Inf (actual={}, predicted={})，替换为零值",
                actual,
                predicted
            );
            0.0_f32
        } else {
            actual - predicted
        };

        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
        }
        self.buffer.push_back(residual);
        self.total_pushed += 1;
    }

    /// 获取最近 `window_size` 步残差
    ///
    /// - 缓冲已满（>= window_size）：返回最近 window_size 步残差
    /// - 缓冲未满且 `zero_init=true`：返回全零向量 `[0.0; window_size]`
    /// - 缓冲未满且 `zero_init=false`：返回 `None`
    ///
    /// 返回的向量长度为 `window_size`，按时间顺序排列（最旧 → 最新）。
    pub fn get_window(&self, window_size: usize) -> Option<Vec<f32>> {
        let len = self.buffer.len();

        if len >= window_size {
            // 取最近 window_size 步
            let start = len - window_size;
            let window: Vec<f32> = self.buffer.iter().skip(start).take(window_size).copied().collect();
            Some(window)
        } else if self.zero_init {
            // 冷启动零填充：生成 window_size 长度的零向量
            // 前 (window_size - len) 位为零，后 len 位为已有残差
            let mut window = vec![0.0_f32; window_size];
            let offset = window_size - len;
            for (i, &val) in self.buffer.iter().enumerate() {
                window[offset + i] = val;
            }
            tracing::debug!(
                "残差缓冲未满 ({}/{})，零填充 (zero_init=true)",
                len,
                window_size
            );
            Some(window)
        } else {
            tracing::debug!(
                "残差缓冲不足 ({}/{})，zero_init=false 拒绝返回",
                len,
                window_size
            );
            None
        }
    }

    /// 检查缓冲是否已有足够数据
    ///
    /// - `zero_init=true`：永远返回 `true`（未满时零填充）
    /// - `zero_init=false`：仅当已填充 >= `window_size` 步时返回 `true`
    pub fn is_ready(&self, window_size: usize) -> bool {
        self.zero_init || self.buffer.len() >= window_size
    }

    /// 清空缓冲（模型重置或 OTA 升级后调用）
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.total_pushed = 0;
        tracing::debug!("残差缓冲已重置 (capacity={})", self.capacity);
    }

    /// 获取已填充的残差数量
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// 缓冲是否为空
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// 获取容量
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // RB-01: 缓冲已满时提取最近 T 步残差
    // ========================================================================

    #[test]
    fn test_buffer_full_extract_window() {
        let mut buf = ResidualBuffer::new(24, true);

        // 填充 30 步残差（actual=10, predicted=8 → residual=2）
        for _ in 0..30 {
            buf.push(10.0, 8.0);
        }

        assert_eq!(buf.len(), 24, "缓冲应截断到容量 24");
        assert!(buf.is_ready(24));

        let window = buf.get_window(24).unwrap();
        assert_eq!(window.len(), 24);
        // 所有值应为 2.0
        for &v in &window {
            assert!((v - 2.0).abs() < 1e-6, "残差应为 2.0，实际为 {}", v);
        }
    }

    // ========================================================================
    // RB-02: 缓冲未满 + zero_init=true 时返回零向量
    // ========================================================================

    #[test]
    fn test_buffer_not_full_zero_init() {
        let mut buf = ResidualBuffer::new(24, true);

        // 仅填充 5 步
        for i in 0..5 {
            buf.push(10.0 + i as f32, 10.0);
        }

        assert_eq!(buf.len(), 5);
        assert!(buf.is_ready(24), "zero_init=true 永远就绪");

        let window = buf.get_window(24).unwrap();
        assert_eq!(window.len(), 24);

        // 前 19 位为零，后 5 位为残差
        for (i, &v) in window.iter().enumerate() {
            if i < 19 {
                assert!((v - 0.0).abs() < 1e-6, "位置 {} 应为 0，实际 {}", i, v);
            } else {
                let expected = (i - 19) as f32;
                assert!(
                    (v - expected).abs() < 1e-6,
                    "位置 {} 应为 {}，实际 {}",
                    i,
                    expected,
                    v
                );
            }
        }
    }

    // ========================================================================
    // RB-03: 缓冲未满 + zero_init=false 时返回 None
    // ========================================================================

    #[test]
    fn test_buffer_not_full_no_zero_init() {
        let mut buf = ResidualBuffer::new(24, false);

        buf.push(10.0, 8.0);
        buf.push(12.0, 10.0);

        assert!(!buf.is_ready(24), "zero_init=false 且未满时不就绪");
        assert!(buf.get_window(24).is_none(), "应返回 None");
    }

    // ========================================================================
    // RB-04: reset 清空缓冲
    // ========================================================================

    #[test]
    fn test_buffer_reset() {
        let mut buf = ResidualBuffer::new(10, true);

        for _ in 0..8 {
            buf.push(5.0, 3.0);
        }
        assert_eq!(buf.len(), 8);

        buf.reset();
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
    }

    // ========================================================================
    // RB-05: push 含 NaN 时替换为零值
    // ========================================================================

    #[test]
    fn test_buffer_nan_handling() {
        let mut buf = ResidualBuffer::new(5, true);

        // 正常值
        buf.push(10.0, 8.0); // residual = 2.0
        // NaN
        buf.push(f32::NAN, 8.0); // residual = 0.0（替换）
        // 正常值
        buf.push(12.0, 10.0); // residual = 2.0

        let window = buf.get_window(5).unwrap();
        assert_eq!(window.len(), 5);

        // 后 3 个位置对应实际数据
        assert!((window[2] - 2.0).abs() < 1e-6);
        assert!((window[3] - 0.0).abs() < 1e-6, "NaN 应替换为 0");
        assert!((window[4] - 2.0).abs() < 1e-6);
    }

    // ========================================================================
    // RB-06: push 含 Inf 时替换为零值
    // ========================================================================

    #[test]
    fn test_buffer_inf_handling() {
        let mut buf = ResidualBuffer::new(5, true);

        buf.push(f32::INFINITY, 8.0);
        buf.push(10.0, f32::NEG_INFINITY);

        let window = buf.get_window(5).unwrap();
        assert!((window[3] - 0.0).abs() < 1e-6);
        assert!((window[4] - 0.0).abs() < 1e-6);
    }

    // ========================================================================
    // RB-07: FIFO 窗口滑动正确
    // ========================================================================

    #[test]
    fn test_buffer_fifo_window() {
        let mut buf = ResidualBuffer::new(3, true);

        buf.push(10.0, 9.0); // residual = 1.0
        buf.push(10.0, 8.0); // residual = 2.0
        buf.push(10.0, 7.0); // residual = 3.0
        buf.push(10.0, 6.0); // residual = 4.0，1.0 被挤出

        assert_eq!(buf.len(), 3);

        let window = buf.get_window(3).unwrap();
        assert!((window[0] - 2.0).abs() < 1e-6);
        assert!((window[1] - 3.0).abs() < 1e-6);
        assert!((window[2] - 4.0).abs() < 1e-6);
    }

    // ========================================================================
    // RB-08: 性能测试（<= 1ms / 100 次 push）
    // ========================================================================

    #[test]
    fn test_residual_buffer_update_latency() {
        let mut buf = ResidualBuffer::new(24, true);
        let start = std::time::Instant::now();
        for _ in 0..100 {
            buf.push(0.5, 0.3);
        }
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_millis() <= 1,
            "残差缓冲更新超时: {}ms（要求 <= 1ms）",
            elapsed.as_millis()
        );
    }
}
