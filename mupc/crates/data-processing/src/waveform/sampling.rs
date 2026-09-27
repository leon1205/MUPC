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
//!
//! # 安全口径（U-74 审查 C-1）
//!
//! 两个 `Vec<f32>` 槽位原经**裸指针**改写（`as_ptr` + `add` + 指针写）—— 那条 `// SAFETY` 只论证
//! "`idx` 在界内"，**未论证别名/并发**（裸写要求独占，而 `get_pre/post_trigger_data`
//! 经 `&Vec<f32>` 同时直读同一槽位 ⇒ 跨线程并发命中即形式上 UB）。改为 **`RwLock` 的写/读锁**：
//! 写者独占槽位、读者持读锁拷出 ⇒ **零 `unsafe`**、无撕裂读。该路径生产零触发（`FaultRecorderImpl`
//! 未接线），性能不是约束。
//!
//! # `active_idx` 的**不可越界**不变式
//!
//! `active_idx` 是 `AtomicUsize`，其"取值 ∈ {0,1}"是**本模块的内部不变式**（写入侧只经
//! [`DualBufferManager::swap_buffer`] 与 `write_sample` 内的切换赋值）。为避免"将来某处直接
//! store 越界值 ⇒ `1 - idx` 下溢 / `buffers[idx]` 越界"这类**整进程 panic**，读取侧统一走
//! [`DualBufferManager::active`]（对 2 取模）—— 越界值最多落到"另一个缓冲区"，**不可能 panic**。

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// 双缓冲区管理器
///
/// 维护两个 `Vec<f32>` 缓冲区（`RwLock` 保护），交替用于连续采样和故障录波。
/// 游标/冻结位用原子量无锁读，**槽位本体**加锁访问（见模块头的安全口径）。
pub struct DualBufferManager {
    /// 两个数据缓冲区（槽位本体经 `RwLock`；见模块头）
    buffers: [RwLock<Vec<f32>>; 2],
    /// 当前活动缓冲区索引（内部不变式 ∈ {0,1}；经 [`Self::active`] 读取以杜绝越界）
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
                RwLock::new(vec![0.0f32; capacity]),
                RwLock::new(vec![0.0f32; capacity]),
            ],
            active_idx: AtomicUsize::new(0),
            capacity,
            write_pos: [AtomicUsize::new(0), AtomicUsize::new(0)],
            frozen: [AtomicBool::new(false), AtomicBool::new(false)],
            total_written: [AtomicUsize::new(0), AtomicUsize::new(0)],
        }
    }

    /// **活动缓冲区索引（恒 ∈ {0,1}）** —— `active_idx` 的**唯一读法**。
    ///
    /// 对 2 取模：即便将来有人把越界值写进 `active_idx`，也只落到"另一个缓冲区"，
    /// **不会**造成 `1 - idx` 下溢或 `buffers[idx]` 越界（两者都是整进程 panic）。
    #[inline]
    fn active(&self) -> usize {
        self.active_idx.load(Ordering::Acquire) % 2
    }

    /// 取某缓冲区槽位的写锁（`buf_idx` 由 [`Self::active`] 保证 ∈ {0,1}）。
    fn buf_write(&self, buf_idx: usize) -> RwLockWriteGuard<'_, Vec<f32>> {
        self.buffers[buf_idx]
            .write()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// 取某缓冲区槽位的读锁（同上）。
    fn buf_read(&self, buf_idx: usize) -> RwLockReadGuard<'_, Vec<f32>> {
        self.buffers[buf_idx]
            .read()
            .unwrap_or_else(|e| e.into_inner())
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
        let idx = self.active();

        // 如果当前缓冲区被冻结，尝试切换
        if self.frozen[idx].load(Ordering::Acquire) {
            let other = 1 - idx; // `idx ∈ {0,1}` ⇒ `other ∈ {0,1}`
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

    /// 向指定缓冲区写入数据（`buf_idx` 由调用方保证 ∈ {0,1}）
    fn write_to_buffer(&self, buf_idx: usize, sample: f32) {
        let pos = self.write_pos[buf_idx].load(Ordering::Acquire);
        let idx = pos % self.capacity;
        {
            // `idx ∈ [0, capacity)` 由 `% capacity` 保证（`capacity > 0` 由 `new` 语义保证）
            let mut buf = self.buf_write(buf_idx);
            buf[idx] = sample;
        }
        self.write_pos[buf_idx].store(pos.wrapping_add(1), Ordering::Release);
        self.total_written[buf_idx].fetch_add(1, Ordering::Release);
    }

    /// 交换活动缓冲区
    ///
    /// 通常在录波完成、释放缓冲区后调用，切换到另一个缓冲区继续采样。
    pub fn swap_buffer(&self) -> usize {
        let old = self.active();
        let new = 1 - old; // `old ∈ {0,1}` ⇒ `new ∈ {0,1}`
        self.active_idx.store(new, Ordering::Release);
        new
    }

    /// 获取故障前触发数据
    ///
    /// 从指定缓冲区中提取触发时刻之前的数据。
    ///
    /// `buf_idx` 越界（≠ 0 / 1）⇒ **空结果 + `warn`**（**不 panic、不悄悄折到另一个缓冲区**：
    /// 后者会让调用方拿着错缓冲区的数据当对的用）。
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
        let (Some(total), Some(write_pos)) = (
            self.total_written.get(buf_idx),
            self.write_pos.get(buf_idx),
        ) else {
            tracing::warn!(buf_idx, "get_pre_trigger_data: buf_idx 越界（须为 0/1）⇒ 空结果");
            return Vec::new();
        };
        let total = total.load(Ordering::Acquire);
        let write_pos = write_pos.load(Ordering::Acquire);

        let count = pre_samples.min(total);
        let mut result = Vec::with_capacity(count);
        // 全程持读锁：同一把锁内的逐点拷贝是一致的快照（不再"跨点重入"）
        let buf = self.buf_read(buf_idx);

        if total <= self.capacity {
            // 缓冲区未回绕
            let start = write_pos.saturating_sub(count);
            for i in 0..count {
                result.push(buf[(start + i) % self.capacity]);
            }
        } else {
            // 缓冲区已回绕，从当前写位置向前追溯
            for i in 0..count {
                let idx = (write_pos + self.capacity - count + i) % self.capacity;
                result.push(buf[idx]);
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
        let (Some(total), Some(write_pos)) = (
            self.total_written.get(buf_idx),
            self.write_pos.get(buf_idx),
        ) else {
            tracing::warn!(buf_idx, "get_post_trigger_data: buf_idx 越界（须为 0/1）⇒ 空结果");
            return Vec::new();
        };
        let total = total.load(Ordering::Acquire);
        let write_pos = write_pos.load(Ordering::Acquire);

        let count = post_samples.min(total);
        let mut result = Vec::with_capacity(count);

        let start = write_pos.saturating_sub(count);
        let buf = self.buf_read(buf_idx);
        for i in 0..count {
            let idx = (start + i) % self.capacity;
            result.push(buf[idx]);
        }

        result
    }

    /// 冻结指定缓冲区（用于录波）
    ///
    /// 冻结后该缓冲区不再接受新数据写入。`buf_idx` 越界 ⇒ **不生效 + `warn`**（不 panic、
    /// 也不悄悄冻另一个缓冲区）。
    pub fn freeze(&self, buf_idx: usize) {
        match self.frozen.get(buf_idx) {
            Some(f) => f.store(true, Ordering::Release),
            None => tracing::warn!(buf_idx, "freeze: buf_idx 越界（须为 0/1）⇒ 忽略"),
        }
    }

    /// 释放指定缓冲区（录波完成后调用）
    ///
    /// 释放后该缓冲区恢复可用状态，写入位置归零。`buf_idx` 越界 ⇒ 不生效 + `warn`。
    pub fn release(&self, buf_idx: usize) {
        match (
            self.write_pos.get(buf_idx),
            self.total_written.get(buf_idx),
            self.frozen.get(buf_idx),
        ) {
            (Some(w), Some(t), Some(f)) => {
                w.store(0, Ordering::Release);
                t.store(0, Ordering::Release);
                f.store(false, Ordering::Release);
            }
            _ => tracing::warn!(buf_idx, "release: buf_idx 越界（须为 0/1）⇒ 忽略"),
        }
    }

    /// 获取当前活动缓冲区索引（**恒 ∈ {0,1}**：经 [`Self::active`] 取模，见模块头不变式）
    pub fn active_index(&self) -> usize {
        self.active()
    }

    /// 获取指定缓冲区的有效数据长度（`buf_idx` 越界 ⇒ 0 + `warn`）
    pub fn buffer_len(&self, buf_idx: usize) -> usize {
        match self.total_written.get(buf_idx) {
            Some(t) => t.load(Ordering::Acquire).min(self.capacity),
            None => {
                tracing::warn!(buf_idx, "buffer_len: buf_idx 越界（须为 0/1）⇒ 0");
                0
            }
        }
    }

    /// 获取缓冲区容量
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 检查指定缓冲区是否被冻结（`buf_idx` 越界 ⇒ `false` + `warn`）
    pub fn is_frozen(&self, buf_idx: usize) -> bool {
        match self.frozen.get(buf_idx) {
            Some(f) => f.load(Ordering::Acquire),
            None => {
                tracing::warn!(buf_idx, "is_frozen: buf_idx 越界（须为 0/1）⇒ false");
                false
            }
        }
    }

    /// **仅用例**：把 `active_idx` 直接写成任意值，验证读取侧对越界值的**不可 panic** 不变式
    /// （C-1 的次判据；见模块头「`active_idx` 的不可越界不变式」）。
    #[cfg(test)]
    pub(crate) fn force_active_index_for_test(&self, v: usize) {
        self.active_idx.store(v, Ordering::Release);
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

    // ── C-1（U-74 审查）：越界不变式与 `unsafe` 消除 ──

    /// **判别力（C-1 主判据）**：生产段不得出现 `unsafe` / 裸指针改写。
    ///
    /// **改坏实现即红**：把 `buf[idx] = sample` 改回 `as_ptr().add(idx) as *mut f32` + `ptr.write`
    /// ⇒ 两条断言各自红。切段口径同 `ring_buffer.rs`（只考核 `#[cfg(test)]` 之前的生产段）。
    #[test]
    fn production_source_has_no_unsafe() {
        let src = include_str!("sampling.rs").replace("\r\n", "\n");
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
                "双缓冲区管理生产段不得出现 `{bad}`（{why}）—— C-1 要求改 RwLock 安全实现"
            );
        }
    }

    /// **C-1 判据②：`active_idx` 越界值不得造成 panic**（原实现 `1 - idx` 在 `idx > 1` 时
    /// 整进程 panic / `buffers[idx]` 越界）。
    ///
    /// **改坏实现即红**：把 `active()` 的 `% 2` 去掉（回到直接 `load()`）⇒ `1 - 99` 下溢
    /// panic ⇒ 本用例红（debug 构建下 `usize` 减法溢出即 panic）。
    #[test]
    fn out_of_range_active_index_does_not_panic() {
        let m = DualBufferManager::new(8, 8);
        m.force_active_index_for_test(99);
        // 不得 panic（越界值经 `% 2` 落到缓冲区 1）
        m.write_sample(7.5);
        assert_eq!(m.active_index(), 1, "99 % 2 == 1 ⇒ 落到缓冲区 1");
        assert_eq!(m.buffer_len(1), 1);
        assert_eq!(m.get_pre_trigger_data(1, 1), vec![7.5]);
        // 冻结缓冲区 1 后再写 ⇒ 切到缓冲区 0（`1 - 1 = 0`，仍在界内）
        m.freeze(1);
        m.write_sample(8.5);
        assert_eq!(m.active_index(), 0);
        assert_eq!(m.get_pre_trigger_data(0, 1), vec![8.5]);
    }

    /// **C-1 判据③：公开访问器的 `buf_idx` 越界 ⇒ 空结果 / 不生效，而非 panic**
    /// （原实现对调用方传入的 `buf_idx` 直接索引 ⇒ 越界 panic）。
    #[test]
    fn out_of_range_buf_idx_degrades_instead_of_panicking() {
        let m = DualBufferManager::new(8, 8);
        m.write_sample(1.0);
        assert!(m.get_pre_trigger_data(2, 4).is_empty());
        assert!(m.get_post_trigger_data(9, 4).is_empty());
        assert_eq!(m.buffer_len(2), 0);
        assert!(!m.is_frozen(2));
        m.freeze(2);
        m.release(2); // 不 panic、不改动任何真实缓冲区
        assert_eq!(m.buffer_len(0), 1, "越界 freeze/release 不得影响真实缓冲区");
        assert!(!m.is_frozen(0));
    }
}
