//! 环形缓冲区
//!
//! 固定大小预分配 `Vec` + 原子写入游标 + **`RwLock` 保护的槽位访问**。
//!
//! # 为什么用 `RwLock` 而不是裸指针写（U-74 审查 C-1 的口径变更）
//!
//! 原实现用**裸指针**改写槽位（`Vec` 的 `as_ptr` + `add` + 指针写），那条 `// SAFETY`
//! **只论证了 `idx` 在界内**，完全没有论证**别名与并发**：裸写要求对该位置**独占**，
//! 而 `read_all` / `read_range` 同时经 `&Vec<T>` 直接读同一槽位 ⇒ 只要写者与读者跨线程
//! 并发命中同一槽位，就是**形式上 UB**（`f64` 这类多字类型会撕裂读）。签名 `write(&self)`
//! 更是把"多读者 + 1 写者"当成了合法用法 —— 不是"当前没人这么用"，而是**签名允许**。
//!
//! 该路径在**生产零触发**（`FaultRecorderImpl` 未接线）⇒ **性能不是约束**，故取**安全实现**：
//! 写入持 `RwLock` 的**写**锁（独占槽位，读者不可能看到半写值），读取持**读**锁。锁竞争与
//! `T: Copy` 的拷贝成本在此量级下无关紧要；换来的是**零 `unsafe`**、无须再论证任何别名前提。
//!
//! # 容量计算
//!
//! 默认配置：4000 Hz × max(200 ms, 1000 ms) = 4000 采样点/通道
//! 总内存：10 通道 × 4000 点 × 8B + 4000 × 8B ≈ 352 KB

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// 环形缓冲区（多读者 + 单写者，**无 `unsafe`**）
///
/// 写入游标是原子量（读游标无锁）；**槽位本体**由 `RwLock` 保护：
/// 写者持写锁改一个槽位，读者持读锁拷出，二者互斥 ⇒ **不存在撕裂读**。
///
/// # 类型参数
///
/// * `T` - 采样数据类型，需实现 `Default + Copy`
pub struct RingBuffer<T: Default + Copy> {
    /// 存储缓冲区（预分配全容量），读写均经 `RwLock`（见模块头）
    buffer: RwLock<Vec<T>>,
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
            buffer: RwLock::new(vec![T::default(); capacity]),
            capacity,
            write_pos: AtomicUsize::new(0),
        }
    }

    /// 取读锁（**中毒不传播**：持锁期间不做任何可 panic 的事 ⇒ 中毒态仍可安全读；
    /// 与 `alert_feed` / `uplink` 的 `unwrap_or_else(|e| e.into_inner())` 同范式）。
    fn read_guard(&self) -> RwLockReadGuard<'_, Vec<T>> {
        self.buffer.read().unwrap_or_else(|e| e.into_inner())
    }

    /// 取写锁（同上）。
    fn write_guard(&self) -> RwLockWriteGuard<'_, Vec<T>> {
        self.buffer.write().unwrap_or_else(|e| e.into_inner())
    }

    /// 写入一个采样点
    ///
    /// 写入游标经 `Acquire`/`Release` 配对发布（读者据此判断可见范围）；
    /// **槽位本体**在写锁内改写（读者持读锁 ⇒ 不可能读到半写值）。
    ///
    /// # 参数
    ///
    /// * `sample` - 待写入的采样数据
    #[inline]
    pub fn write(&self, sample: T) {
        let pos = self.write_pos.load(Ordering::Acquire);
        let idx = pos % self.capacity;
        {
            // `idx ∈ [0, capacity)`：`capacity > 0` 由 `new` 断言，`% capacity` 保证上界
            let mut buf = self.write_guard();
            buf[idx] = sample;
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
        // 全程持读锁：本函数返回**拷贝**，逐点在同一把锁内取（不重新引入"跨锁重读"）
        let buf = self.read_guard();

        if total <= self.capacity {
            // 缓冲区尚未回绕，直接返回 [0..total)
            let mut result = Vec::with_capacity(total);
            for i in 0..total {
                result.push(buf[i]);
            }
            result
        } else {
            // 缓冲区已回绕，拼接 [write_pos%cap .. cap) + [0 .. write_pos%cap)
            let start = write_pos % self.capacity;
            let mut result = Vec::with_capacity(self.capacity);
            for i in start..self.capacity {
                result.push(buf[i]);
            }
            for i in 0..start {
                result.push(buf[i]);
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

        // 全程持读锁（同 `read_all`：同一把锁内逐点拷贝，返回的是一致的快照）
        let buf = self.read_guard();
        while remaining > 0 && idx < self.capacity {
            result.push(buf[idx]);
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

    // ── C-1（U-74 审查）：`unsafe` 消除的**类型层面**判据 ──

    /// **判别力（C-1 主判据）**：生产段**不得出现 `unsafe`**。
    ///
    /// 该路径生产零触发（`FaultRecorderImpl` 未接线）⇒ 性能不是约束 ⇒ 要求在**类型层面**
    /// 消除 `unsafe`，而不是把 `// SAFETY` 的论证补齐。**改回 `as_ptr` + `ptr.write`（或任何
    /// `unsafe` 块）⇒ 本用例红**（这正是 C-1 允许的判据：撕裂读难以在测试里稳定复现）。
    ///
    /// 切段口径与 `startup.rs` 的 `production_src` 同款：只考核 `#[cfg(test)]` 之前的生产段
    /// （断言自身的字面量不得自指）。
    #[test]
    fn production_source_has_no_unsafe() {
        let src = include_str!("ring_buffer.rs").replace("\r\n", "\n");
        let (production, _) = src
            .split_once("\n#[cfg(test)]\nmod tests {")
            .expect("`#[cfg(test)] mod tests` 分段锚点必须存在");
        assert!(
            production.len() > 1_000,
            "分段锚点须真的切出生产段，实得 {} 字节",
            production.len()
        );
        // 只匹配**代码构造**（不匹配文档里"零 unsafe"这类叙述）
        for (bad, why) in [
            ("unsafe {", "unsafe 块"),
            ("unsafe fn", "unsafe fn"),
            ("unsafe impl", "unsafe impl"),
            (".as_ptr()", "裸指针改写槽位"),
        ] {
            assert!(
                !production.contains(bad),
                "环形缓冲区生产段不得出现 `{bad}`（{why}）—— C-1 要求改 RwLock 安全实现"
            );
        }
    }

    /// **C-1 判据②：并发读写下读到的是「整帧」**（写者写入哨兵值，读者只应看到哨兵之一，
    /// 不得出现两值拼合的撕裂值）。
    ///
    /// ⚠️ **如实的局限**：撕裂读是**偶发**的，本用例在旧的不安全实现下**未必稳定变红**
    /// （故 C-1 的主判据是上一条"类型层面消除 unsafe"）。它在这里的作用是把"整帧性"这条
    /// 不变量写成**可执行**的，并在今后任何"为了性能改回裸指针"的尝试中提供第二道网。
    #[test]
    fn concurrent_read_write_never_tears_a_sample() {
        use std::sync::Arc;
        // 哨兵值：任取两个 f64，任何"高低字拼合"的撕裂值都不会等于其中之一
        const A: f64 = 1.0;
        const B: f64 = f64::from_bits(0x4000_0000_0000_0001); // 1.0000000000000002
        let buf = Arc::new(RingBuffer::<f64>::new(64));

        let writers: Vec<_> = (0..2)
            .map(|w| {
                let b = buf.clone();
                std::thread::spawn(move || {
                    for i in 0..20_000u64 {
                        b.write(if (i + w) % 2 == 0 { A } else { B });
                    }
                })
            })
            .collect();
        let readers: Vec<_> = (0..3)
            .map(|_| {
                let b = buf.clone();
                std::thread::spawn(move || {
                    let mut seen = 0usize;
                    for _ in 0..20_000 {
                        for v in b.read_all() {
                            assert!(
                                v == A || v == B,
                                "读到撕裂值 {v}（既不是 A 也不是 B）—— 说明槽位被并发半写"
                            );
                            seen += 1;
                        }
                    }
                    seen
                })
            })
            .collect();
        for h in writers {
            h.join().expect("写线程不得 panic");
        }
        let mut total = 0usize;
        for h in readers {
            total += h.join().expect("读线程不得 panic（读到撕裂值会在此暴露）");
        }
        assert!(total > 0, "读者必须真的读到过数据（否则本用例是空网）");
    }
}
