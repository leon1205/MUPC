//! 双缓冲区管理器
//!
//! 两个缓冲区交替工作，确保连续故障不丢失数据：
//!
//! ```text
//! 稳态采样 → RingBuffer A (活动) → 新数据覆盖旧数据
//!   故障触发 → RingBuffer A (冻结) → 等待读取 + 写入文件
//!              RingBuffer B (活动) → 继续采样（收集 post-trigger 数据）
//!   录制完成 → RingBuffer A (重置) → 恢复就绪状态
//! ```
//!
//! 当两个缓冲区皆满时第三故障发生：丢弃已保存完成的最旧录波，释放缓冲区复用。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// 双缓冲区管理器
///
/// 维护两个 `Vec<f32>` 缓冲区，交替用于连续采样和故障录波。
/// 写入操作使用原子标志位保护，适合单生产者场景。
pub struct DualBufferManager {
    /// 两个数据缓冲区
    buffers: [Vec<f32>; 2],
    /// 当前活动缓冲区索引 (0 或 1)
    active_idx: AtomicUsize,
    /// 缓冲区容量（采样点数）
    capacity: usize,
    /// 各缓冲区当前写入位置
    write_pos: [AtomicUsize; 2],
    /// 各缓冲区是否被冻结（正在录波）
    frozen: [AtomicBool; 2],
    /// 各缓冲区已写入总数
    total_written: [AtomicUsize; 2],
}

impl DualBufferManager {
    /// 创建新的双缓冲区管理器
    ///
    /// # 参数
    ///
    /// * `pre_trigger_samples` - 故障前采样点数
    /// * `post_trigger_samples` - 故障后采样点数
    ///
    /// 容量取两者最大值。
    pub fn new(pre_trigger_samples: usize, post_trigger_samples: usize) -> Self {
        let capacity = pre_trigger_samples.max(post_trigger_samples);
        Self {
            buffers: [
                vec![0.0f32; capacity],
                vec![0.0f32; capacity],
            ],
            active_idx: AtomicUsize::new(0),
            capacity,
            write_pos: [AtomicUsize::new(0), AtomicUsize::new(0)],
            frozen: [AtomicBool::new(false), AtomicBool::new(false)],
            total_written: [AtomicUsize::new(0), AtomicUsize::new(0)],
        }
    }

    /// 向当前活动缓冲区写入一个采样点
    ///
    /// 如果活动缓冲区被冻结（正在录波），自动切换到另一个缓冲区。
    ///
    /// # 参数
    ///
    /// * `sample` - 采样数据
    #[inline]
    pub fn write_sample(&self, sample: f32) {
        let idx = self.active_idx.load(Ordering::Acquire);

        // 如果当前缓冲区被冻结，尝试切换
        if self.frozen[idx].load(Ordering::Acquire) {
            let other = 1 - idx;
            if !self.frozen[other].load(Ordering::Acquire) {
                self.active_idx.store(other, Ordering::Release);
                self.write_to_buffer(other, sample);
                return;
            }
            // 两个缓冲区都冻结，丢弃数据
            tracing::warn!("双缓冲区皆满，丢弃采样点");
            return;
        }

        self.write_to_buffer(idx, sample);
    }

    /// 向指定缓冲区写入数据
    fn write_to_buffer(&self, buf_idx: usize, sample: f32) {
        let pos = self.write_pos[buf_idx].load(Ordering::Acquire);
        let idx = pos % self.capacity;

        // SAFETY: idx 始终在 [0, capacity) 范围内
        unsafe {
            let ptr = self.buffers[buf_idx].as_ptr().add(idx) as *mut f32;
            ptr.write(sample);
        }

        self.write_pos[buf_idx].store(pos.wrapping_add(1), Ordering::Release);
        self.total_written[buf_idx].fetch_add(1, Ordering::Release);
    }

    /// 交换活动缓冲区
    ///
    /// 通常在录波完成、释放缓冲区后调用，切换到另一个缓冲区继续采样。
    pub fn swap_buffer(&self) -> usize {
        let old = self.active_idx.load(Ordering::Acquire);
        let new = 1 - old;
        self.active_idx.store(new, Ordering::Release);
        new
    }

    /// 获取故障前触发数据
    ///
    /// 从指定缓冲区中提取触发时刻之前的数据。
    ///
    /// # 参数
    ///
    /// * `buf_idx` - 缓冲区索引 (0 或 1)
    /// * `pre_samples` - 需要提取的故障前采样点数
    ///
    /// # 返回
    ///
    /// 故障前采样数据切片
    pub fn get_pre_trigger_data(&self, buf_idx: usize, pre_samples: usize) -> Vec<f32> {
        let total = self.total_written[buf_idx].load(Ordering::Acquire);
        let write_pos = self.write_pos[buf_idx].load(Ordering::Acquire);

        let count = pre_samples.min(total);
        let mut result = Vec::with_capacity(count);

        if total <= self.capacity {
            // 缓冲区未回绕
            let start = write_pos.saturating_sub(count);
            for i in 0..count {
                result.push(self.buffers[buf_idx][(start + i) % self.capacity]);
            }
        } else {
            // 缓冲区已回绕，从当前写位置向前追溯
            for i in 0..count {
                let idx =
                    (write_pos + self.capacity - count + i) % self.capacity;
                result.push(self.buffers[buf_idx][idx]);
            }
        }

        result
    }

    /// 获取故障后触发数据
    ///
    /// 从指定缓冲区中提取触发时刻之后的数据。
    ///
    /// # 参数
    ///
    /// * `buf_idx` - 缓冲区索引 (0 或 1)
    /// * `post_samples` - 需要提取的故障后采样点数
    ///
    /// # 返回
    ///
    /// 故障后采样数据切片
    pub fn get_post_trigger_data(&self, buf_idx: usize, post_samples: usize) -> Vec<f32> {
        let total = self.total_written[buf_idx].load(Ordering::Acquire);
        let write_pos = self.write_pos[buf_idx].load(Ordering::Acquire);

        let count = post_samples.min(total);
        let mut result = Vec::with_capacity(count);

        let start = write_pos.saturating_sub(count);
        for i in 0..count {
            let idx = (start + i) % self.capacity;
            result.push(self.buffers[buf_idx][idx]);
        }

        result
    }

    /// 冻结指定缓冲区（用于录波）
    ///
    /// 冻结后该缓冲区不再接受新数据写入。
    pub fn freeze(&self, buf_idx: usize) {
        self.frozen[buf_idx].store(true, Ordering::Release);
    }

    /// 释放指定缓冲区（录波完成后调用）
    ///
    /// 释放后该缓冲区恢复可用状态，写入位置归零。
    pub fn release(&self, buf_idx: usize) {
        self.write_pos[buf_idx].store(0, Ordering::Release);
        self.total_written[buf_idx].store(0, Ordering::Release);
        self.frozen[buf_idx].store(false, Ordering::Release);
    }

    /// 获取当前活动缓冲区索引
    pub fn active_index(&self) -> usize {
        self.active_idx.load(Ordering::Acquire)
    }

    /// 获取指定缓冲区的有效数据长度
    pub fn buffer_len(&self, buf_idx: usize) -> usize {
        let total = self.total_written[buf_idx].load(Ordering::Acquire);
        total.min(self.capacity)
    }

    /// 获取缓冲区容量
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 检查指定缓冲区是否被冻结
    pub fn is_frozen(&self, buf_idx: usize) -> bool {
        self.frozen[buf_idx].load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_read() {
        let manager = DualBufferManager::new(100, 200);

        for i in 0..50 {
            manager.write_sample(i as f32);
        }

        let data = manager.get_pre_trigger_data(0, 50);
        assert_eq!(data.len(), 50);
        assert_eq!(data[0], 0.0);
        assert_eq!(data[49], 49.0);
    }

    #[test]
    fn test_swap_buffer() {
        let manager = DualBufferManager::new(100, 200);

        // 先写入 buffer 0
        manager.write_sample(1.0);
        assert_eq!(manager.active_index(), 0);

        // 交换
        let new_idx = manager.swap_buffer();
        assert_eq!(new_idx, 1);
        assert_eq!(manager.active_index(), 1);

        // 写入 buffer 1
        manager.write_sample(2.0);
        let data1 = manager.get_pre_trigger_data(1, 1);
        assert_eq!(data1[0], 2.0);
    }

    #[test]
    fn test_freeze_and_release() {
        let manager = DualBufferManager::new(100, 200);

        manager.write_sample(1.0);
        manager.freeze(0);
        assert!(manager.is_frozen(0));

        // 冻结后应自动切换到 buffer 1
        manager.write_sample(2.0);
        assert_eq!(manager.active_index(), 1);

        manager.release(0);
        assert!(!manager.is_frozen(0));
        assert_eq!(manager.buffer_len(0), 0); // 释放后清零
    }

    #[test]
    fn test_capacity() {
        let manager = DualBufferManager::new(100, 300);
        assert_eq!(manager.capacity(), 300); // 取最大值
    }
}
