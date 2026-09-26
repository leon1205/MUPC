//! **写操作幂等表**（设计 §3.3 管线第 3 步）——开发单元 **G-2**。
//!
//! 键 = `(op, request_id)`（契约 [`IdempotencyKey`]），三种结局：
//!
//! | 查表结果 | 回执 |
//! |----------|------|
//! | 未命中 | 占位为"处理中"，继续走管线 |
//! | 命中且**处理中** | `ControlCode::Busy`（**不排队、不重复执行**） |
//! | 命中且**已完成** | **首次的原始回执** + `duplicate=true`（`ok` 不变——首次失败的重放仍是失败） |
//!
//! # 有界性（契约常量，不是"差不多就行"）
//!
//! - 容量 [`IDEMPOTENCY_CAPACITY`]（256）：超限淘汰**最旧插入**的条目（LRU 的近似 = 插入序，
//!   因为"重复请求"在时间上必然靠近首次；真 LRU 要记录访问序，而本表的热点就是刚插入的那条）。
//! - TTL [`IDEMPOTENCY_TTL_MS`]（30 s）：超过 TTL 的条目**在查表时惰性清除**（不起后台任务——
//!   表最多 256 条，扫一遍是常数级；后台定时器反而多一个生命周期要管）。
//!
//! ⚠️ **"迟到 complete"**（评审阻塞 1）：收尾（[`IdempotencyTable::complete`]）可能晚于 TTL/
//! 容量淘汰——此刻该 `key` 已被判"不存在"。语义取**直接丢弃**（不复活），并由调用方记录；
//! 该不变式由单测 `late_complete_after_ttl_sweep_is_dropped_and_never_breaks_the_cap` 钉死。
//!
//! ⚠️ 契约把 `IDEMPOTENCY_TTL_MS` 与 `REPLAY_WINDOW_MS` 定成**同值但语义不同**的两个常量
//! （`display-proto/src/control.rs:24-30` 明写"实现者**不得**因二者同值而复用"）。本模块
//! 收的是**自带的 TTL 入参**，装配点从 `IDEMPOTENCY_TTL_MS` 注入 ⇒ 将来产品只调其中一个时，
//! 另一处不会被静默改掉。

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use mupc_display_proto::{IdempotencyKey, IDEMPOTENCY_CAPACITY, IDEMPOTENCY_TTL_MS};

/// 条目状态。
#[derive(Debug, Clone)]
enum Slot<T> {
    /// 占位：同键请求正在处理（第二个同键请求 ⇒ `Busy`）。
    InFlight,
    /// 已完成：存**首次的原始回执**（重复请求原样返回它）。
    Done(T),
}

/// 一次占位尝试的结局。
#[derive(Debug, Clone, PartialEq)]
pub enum Reserve<T> {
    /// 新占位成功 ⇒ 调用方继续走管线，并在结束时 [`IdempotencyTable::complete`]。
    Fresh,
    /// 同键请求仍在处理 ⇒ 回 `Busy`。
    InFlight,
    /// 同键请求已完成 ⇒ 回这份原始回执（`duplicate=true`）。
    Done(T),
}

/// 有界 / 带 TTL 的幂等表（`T` = 回执类型；本单元为 `ControlResponse<ConfigView>`）。
pub struct IdempotencyTable<T> {
    cap: usize,
    ttl_ms: u64,
    inner: Mutex<Inner<T>>,
}

struct Inner<T> {
    /// 插入序（容量淘汰的依据）。
    order: VecDeque<(IdempotencyKey, u64)>,
    map: HashMap<IdempotencyKey, Slot<T>>,
}

impl<T: Clone> IdempotencyTable<T> {
    /// 建表（容量 / TTL 由调用方给，见模块头"有界性"）。
    pub fn new(cap: usize, ttl_ms: u64) -> Self {
        Self {
            cap: cap.max(1),
            ttl_ms,
            inner: Mutex::new(Inner {
                order: VecDeque::new(),
                map: HashMap::new(),
            }),
        }
    }

    /// 按契约常量建表（生产装配点用）。
    pub fn with_contract_bounds() -> Self {
        Self::new(IDEMPOTENCY_CAPACITY, IDEMPOTENCY_TTL_MS)
    }

    /// 查表 + 占位（管线第 3 步）。
    pub fn reserve(&self, key: IdempotencyKey, now_ms: u64) -> Reserve<T> {
        let mut g = self.lock();
        g.sweep(now_ms, self.ttl_ms);
        match g.map.get(&key) {
            Some(Slot::InFlight) => Reserve::InFlight,
            Some(Slot::Done(v)) => Reserve::Done(v.clone()),
            None => {
                g.evict_to_capacity(self.cap.saturating_sub(1));
                g.map.insert(key.clone(), Slot::InFlight);
                g.order.push_back((key, now_ms));
                Reserve::Fresh
            }
        }
    }

    /// 标记完成（存**首次的原始回执**）。
    ///
    /// 返回 `false` ⇒ **迟到 `complete`**：该 `key` 已被 `sweep`（TTL）或 `evict`（容量）判为
    /// 不存在 ⇒ 占位在逻辑上**已经消失**，本次完成**直接丢弃**（调用方负责记录，见
    /// `ConfigService::apply` 的 `warn`）。
    ///
    /// # 为什么是"丢弃"而不是"补 `order.push_back` 救回来"（评审阻塞 1 的两个候选）
    ///
    /// 修复前：`complete` **只** `map.insert`（想当然认为"占位项已在 `order` 里"）。
    /// 一旦收尾晚于 TTL，该条目就落在 `map` 里而**不在** `order` 里 ⇒ ① `sweep` 扫不到它
    /// （**永不 TTL**）② `evict_to_capacity` 淘不到它 ③ 却仍计入 `map.len()` 逼走别人。
    /// 实测 `cap=1` 时 `len()==2`，且隔 100 s 仍是 2 —— 容量契约被打破。
    ///
    /// 两种修法都要求"`map` 与 `order` 重新一致"，但**语义**不同：补 `order` 等于让一个
    /// **已被判定过期**的请求在收尾时复活（它的 `now_ms` 锚点已失效：沿用旧时间戳 ⇒ 下一拍又被
    /// sweep 删回不一致态；写新时间戳 ⇒ 过期请求重新获得 30 s 窗口）。而"迟到 = 该 id 已被判
    /// 过期"本来就说明**它的占位逻辑上不存在**，其回执也没有再被认可的理由（同键的新请求在
    /// sweep 那一刻起就被允许作为**新操作**重新执行，若此刻再把旧回执塞回去，两条语义冲突）。
    /// ⇒ 取**丢弃**，并把"发生了丢弃"变成**可观测事实**（返回值 + 调用方 `warn`），不静默。
    pub fn complete(&self, key: &IdempotencyKey, done: T) -> bool {
        let mut g = self.lock();
        if !g.map.contains_key(key) {
            return false;
        }
        g.map.insert(key.clone(), Slot::Done(done));
        // 占位项已在 `order` 里（reserve 时插入）；这里不重复插入，避免 TTL 被刷新。
        true
    }

    // ⚠️ **没有 `abandon`**（曾经写过，已删）：它存在的理由是"管线中途放弃占位，避免留下永不
    // 完成的 `InFlight` 把同键请求永久判成 Busy"。但本实现的**每一条路径**都必然走到
    // `complete`（`execute` 用 `Result` 而非 `?` 提前返回，**不会** panic/短路跳过收尾），
    // 且 `InFlight` 也会被 TTL 清掉 ⇒ 该"保险"是**不可达的死代码**。留着它只会让读者以为
    // "有路径可能不 complete"（那是对实现的错误描述）。

    /// 当前条目数（**测试用**：容量与 TTL 的可观测证据）。
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lock().map.len()
    }

    /// 是否为空（**测试用**）。
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner<T>> {
        // 锁只在**极短的临界区**内持有（无 await、无 IO）⇒ 毒化只可能来自同进程内的 panic，
        // 此时 `into_inner` 继续用（与本仓库 `display_host` 的 `SharedLatest` 同范式：
        // 一次 panic 不该让整个控制通道永久失效）。
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl<T> Inner<T> {
    /// 惰性清除过期条目（TTL；含 `InFlight`：卡死的占位也会到期，不当成永久 Busy）。
    fn sweep(&mut self, now_ms: u64, ttl_ms: u64) {
        let mut expired: Vec<IdempotencyKey> = Vec::new();
        for (k, ts) in &self.order {
            if now_ms.saturating_sub(*ts) >= ttl_ms {
                expired.push(k.clone());
            }
        }
        if expired.is_empty() {
            return;
        }
        for k in expired {
            self.map.remove(&k);
        }
        // 显式取字段引用：`self.order.retain(..)` 期间闭包只借 `self.map`（两字段互不相干）
        let map = &self.map;
        self.order.retain(|(k, _)| map.contains_key(k));
    }

    /// 淘汰到不超过 `cap` 条（最旧插入的先走）。
    fn evict_to_capacity(&mut self, cap: usize) {
        while self.map.len() > cap {
            match self.order.pop_front() {
                Some((k, _)) => {
                    self.map.remove(&k);
                }
                None => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(rid: &str) -> IdempotencyKey {
        IdempotencyKey::new("apply", rid)
    }

    /// 三态：未命中 ⇒ Fresh；占位中 ⇒ InFlight（**不重复执行**）；完成 ⇒ Done（原样回首次值）。
    #[test]
    fn fresh_then_inflight_then_done() {
        let t: IdempotencyTable<String> = IdempotencyTable::new(8, 30_000);
        assert_eq!(t.reserve(key("r1"), 1_000), Reserve::Fresh);
        // 占位中再来一次 ⇒ InFlight（这是 Busy 的判据，**不能**变成 Fresh 再执行一遍）
        assert_eq!(t.reserve(key("r1"), 1_001), Reserve::InFlight);
        t.complete(&key("r1"), "first".to_string());
        assert_eq!(
            t.reserve(key("r1"), 1_002),
            Reserve::Done("first".to_string()),
            "重复请求必须拿到**首次**的回执"
        );
        // 不同 request_id 互不影响
        assert_eq!(t.reserve(key("r2"), 1_003), Reserve::Fresh);
        // 同 request_id 不同 op 也互不影响（键是 (op, request_id)）
        assert_eq!(
            t.reserve(IdempotencyKey::new("release", "r1"), 1_004),
            Reserve::Fresh
        );
    }

    /// TTL：到期后同键**不再**命中（⇒ 允许作为新操作重新执行，符合"30 s 窗口"的语义）。
    #[test]
    fn entries_expire_after_ttl() {
        let t: IdempotencyTable<String> = IdempotencyTable::new(8, 30_000);
        assert_eq!(t.reserve(key("r1"), 0), Reserve::Fresh);
        t.complete(&key("r1"), "first".to_string());
        assert_eq!(t.reserve(key("r1"), 29_999), Reserve::Done("first".into()));
        // 恰在 TTL 边界（>= ttl）⇒ 过期
        assert_eq!(t.reserve(key("r1"), 30_000), Reserve::Fresh);
        // 过期项确实被清掉（不是"仍占着容量"）
        let t2: IdempotencyTable<String> = IdempotencyTable::new(8, 30_000);
        t2.reserve(key("a"), 0);
        t2.reserve(key("b"), 100);
        t2.reserve(key("c"), 40_000);
        assert_eq!(t2.len(), 1, "前两条已过期 ⇒ 表内只剩新占位");
    }

    /// 容量上界：插入 `cap + N` 条后不得超 `cap`，且**最旧的先被淘汰**。
    #[test]
    fn capacity_is_bounded_and_evicts_oldest() {
        let t: IdempotencyTable<String> = IdempotencyTable::new(4, 30_000);
        for i in 0..10u32 {
            assert_eq!(
                t.reserve(key(&format!("r{i}")), 1_000 + i as u64),
                Reserve::Fresh
            );
        }
        assert!(t.len() <= 4, "容量必须被界住，实得 {}", t.len());
        // 最旧的 r0 / r1 已被淘汰；最新的 r9 仍在
        assert_eq!(t.reserve(key("r9"), 1_100), Reserve::InFlight);
        assert_eq!(t.reserve(key("r0"), 1_100), Reserve::Fresh);
    }

    /// **阻塞 1 回归**：`complete` 晚于 `sweep`/`evict`（"迟到 complete"）⇒ 必须**丢弃**，
    /// 不得在 `map` 里留下一个"不在插入序"的条目。
    ///
    /// **改什么会让本条变红**：把 `complete` 改回修复前的"无条件 `map.insert`"⇒ 第 2 条
    /// （`len() <= 1`）当场红，且后续容量界也被撑破（评审实测 `cap=1` 时 `len()==2`）。
    #[test]
    fn late_complete_after_ttl_sweep_is_dropped_and_never_breaks_the_cap() {
        let t: IdempotencyTable<String> = IdempotencyTable::new(1, 30_000);
        assert_eq!(t.reserve(key("a"), 0), Reserve::Fresh);
        // 到 TTL：`reserve` 触发的惰性 sweep 把 `a` 清掉（迟到 complete 的前置）
        assert_eq!(t.reserve(key("b"), 30_000), Reserve::Fresh);
        assert_eq!(t.len(), 1, "`a` 已被 sweep 清掉");
        // 迟到的 complete（该 id 的占位在逻辑上已不存在）
        assert!(
            !t.complete(&key("a"), "late".to_string()),
            "迟到 complete 必须报 false（丢弃），不得复活"
        );
        assert!(t.len() <= 1, "迟到 complete 撑破容量，实得 {}", t.len());
        // 容量淘汰必须仍然成立（无游离条目 ⇒ 最旧的仍先走）
        assert_eq!(t.reserve(key("c"), 30_001), Reserve::Fresh);
        assert!(t.len() <= 1, "容量必须仍被界住，实得 {}", t.len());
        // 正对照：正常 complete 仍报 true（不是"恒 false"的假实现）
        assert!(t.complete(&key("c"), "ok".to_string()));
        assert_eq!(t.reserve(key("c"), 30_002), Reserve::Done("ok".to_string()));

        // 被**容量淘汰**的键同样算"不存在"（迟到 complete 的另一条来路，与 TTL 对称）
        let t2: IdempotencyTable<String> = IdempotencyTable::new(2, 30_000);
        for (i, k) in ["x", "y", "z"].iter().enumerate() {
            assert_eq!(t2.reserve(key(k), i as u64), Reserve::Fresh);
        }
        assert!(
            !t2.complete(&key("x"), "late".to_string()),
            "`x` 已被淘汰 ⇒ 丢弃"
        );
        assert!(t2.len() <= 2, "容量必须仍被界住，实得 {}", t2.len());
    }

    /// 卡死的 `InFlight`（假设某次执行**永远不结束**，例如线程被卡住）也会被 TTL 清掉 ——
    /// "占位"不是永久状态，这是本表**不需要** `abandon` 的旁证。
    #[test]
    fn a_stuck_inflight_placeholder_also_expires() {
        let t: IdempotencyTable<String> = IdempotencyTable::new(8, 30_000);
        assert_eq!(t.reserve(key("r1"), 0), Reserve::Fresh);
        assert_eq!(t.reserve(key("r1"), 1_000), Reserve::InFlight);
        assert!(!t.is_empty());
        // TTL 到期后同键请求可以重新执行（不会永久 Busy）
        assert_eq!(t.reserve(key("r1"), 30_000), Reserve::Fresh);
        assert_eq!(t.len(), 1);
    }
}
