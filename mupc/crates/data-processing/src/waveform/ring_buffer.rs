//! 环形缓冲区
//!
//! 采用固定大小预分配 Vec + 原子写入游标实现，
//! 支持无锁单生产者单消费者模式，避免运行时动态内存分配。
//!
//! # 容量计算
//!
//! 默认配置：4000 Hz × max(200 ms, 1000 ms) = 4000 采样点/通道
//! 总内存：10 通道 × 4000 点 × 8B + 4000 × 8B ≈ 352 KB

use std::sync::atomic::{AtomicUsize, Ordering};

/// 环形缓冲区（无锁单生产者单消费者）
///
/// 使用原子操作实现写入游标，生产者写入时无需加锁，
/// 消费者读取时通过原子加载获取最新写入位置。
///
/// # 类型参数
///
/// * `T` - 采样数据类型，需实现 `Default + Copy`
pub struct RingBuffer<T: Default + Copy> {
    /// 存储缓冲区（预分配全容量）
    buffer: Vec<T>,
    /// 缓冲区容量
    capacity: usize,
    /// 原子写入游标（下一个写入位置，0..capacity 循环）
    write_pos: AtomicUsize,
}

impl<T: Default + Copy> RingBuffer<T> {
    /// 创建新的环形缓冲区
    ///
    /// # 参数
    ///
    /// * `capacity` - 缓冲区容量（最大存储元素个数）
    ///
    /// # 返回
    ///
    /// 预分配好内存的环形缓冲区实例
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "RingBuffer capacity must be greater than 0");
        Self {
            buffer: vec![T::default(); capacity],
            capacity,
            write_pos: AtomicUsize::new(0),
        }
    }

    /// 原子写入一个采样点
    ///
    /// 使用 `AcqRel` 顺序保证：写入前 Acquire 获取最新位置，
    /// 写入后 Release 使消费者可见。
    ///
    /// # 参数
    ///
    /// * `sample` - 待写入的采样数据
    #[inline]
    pub fn write(&self, sample: T) {
        let pos = self.write_pos.load(Ordering::Acquire);
        let idx = pos % self.capacity;

        // SAFETY: idx 始终在 [0, capacity) 范围内
        unsafe {
            let ptr = self.buffer.as_ptr().add(idx) as *mut T;
            ptr.write(sample);
        }

        self.write_pos.store(pos.wrapping_add(1), Ordering::Release);
    }

    /// 读取全部有效数据（按写入顺序）
    ///
    /// 从最旧的有效数据开始读取，直到最新的写入位置。
    /// 如果已写入数量未超过容量，返回全部已写入数据；
    /// 否则返回最近 `capacity` 个数据。
    ///
    /// # 返回
    ///
    /// 按写入时间顺序排列的数据副本
    pub fn read_all(&self) -> Vec<T> {
        let write_pos = self.write_pos.load(Ordering::Acquire);
        let total = write_pos;

        if total == 0 {
            return Vec::new();
        }

        if total <= self.capacity {
            // 缓冲区尚未回绕，直接返回 [0..total)
            let mut result = Vec::with_capacity(total);
            for i in 0..total {
                result.push(self.buffer[i]);
            }
            result
        } else {
            // 缓冲区已回绕，拼接 [write_pos%cap .. cap) + [0 .. write_pos%cap)
            let start = write_pos % self.capacity;
            let mut result = Vec::with_capacity(self.capacity);
            for i in start..self.capacity {
                result.push(self.buffer[i]);
            }
            for i in 0..start {
                result.push(self.buffer[i]);
            }
            result
        }
    }

    /// 读取指定偏移量开始的指定数量采样点
    ///
    /// # 参数
    ///
    /// * `offset` - 从最旧数据开始的偏移量（0 = 最旧）
    /// * `count` - 读取的采样点数量
    ///
    /// # 返回
    ///
    /// 按时间顺序排列的指定范围数据
    pub fn read_range(&self, offset: usize, count: usize) -> Vec<T> {
        let write_pos = self.write_pos.load(Ordering::Acquire);
        let total = write_pos.min(self.capacity + offset + count);
        let effective_total = total.min(write_pos);

        let mut result = Vec::with_capacity(count);
        let start_idx = if write_pos > self.capacity {
            (write_pos % self.capacity + offset) % self.capacity
        } else {
            offset.min(write_pos)
        };

        let mut remaining = count.min(effective_total.saturating_sub(offset));
        let mut idx = start_idx;

        while remaining > 0 && idx < self.capacity {
            result.push(self.buffer[idx]);
            idx = (idx + 1) % self.capacity;
            remaining -= 1;
        }

        result
    }

    /// 获取已写入的采样点数量
    ///
    /// 注意：此值单调递增，超过容量后继续增长（用于计算偏移量）。
    pub fn len(&self) -> usize {
        self.write_pos.load(Ordering::Acquire)
    }

    /// 检查缓冲区是否为空（从未写入过数据）
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 获取缓冲区容量
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 重置缓冲区（清零写入游标）
    ///
    /// 注意：不会清空已有数据，仅重置游标。
    /// 旧数据将被后续写入覆盖。
    pub fn reset(&self) {
        self.write_pos.store(0, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer() {
        let buf = RingBuffer::<f64>::new(100);
        assert_eq!(buf.capacity(), 100);
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_write_and_read() {
        let buf = RingBuffer::<f64>::new(4);
        buf.write(1.0);
        buf.write(2.0);
        buf.write(3.0);

        let data = buf.read_all();
        assert_eq!(data, vec![1.0, 2.0, 3.0]);
        assert_eq!(buf.len(), 3);
    }

    #[test]
    fn test_overwrite() {
        let buf = RingBuffer::<f64>::new(3);
        buf.write(1.0);
        buf.write(2.0);
        buf.write(3.0);
        buf.write(4.0); // 覆盖 1.0
        buf.write(5.0); // 覆盖 2.0

        let data = buf.read_all();
        assert_eq!(data.len(), 3);
        assert_eq!(data, vec![3.0, 4.0, 5.0]);
    }

    #[test]
    fn test_read_range() {
        let buf = RingBuffer::<f64>::new(5);
        for i in 0..10 {
            buf.write(i as f64);
        }
        // 缓冲区包含 [5,6,7,8,9]
        let range = buf.read_range(1, 3);
        assert_eq!(range.len(), 3);
        assert_eq!(range, vec![6.0, 7.0, 8.0]);
    }

    #[test]
    fn test_reset() {
        let buf = RingBuffer::<f64>::new(5);
        buf.write(1.0);
        buf.write(2.0);
        buf.reset();
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
    }
}
