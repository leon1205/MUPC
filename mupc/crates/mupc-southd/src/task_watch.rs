//! 后台 task 的**完成态观测**（B-9，2026-09-27 全项目审查 P3）。
//!
//! **要解决的问题**：采集类 task（`SouthScheduler::spawn` 的各口 task、`PcsHandle::spawn_collection_loop`）
//! 都是 `loop { … }` 的常驻任务，句柄此前只被"持有 + 退出时 abort"，**从不被观测**。
//! 于是 task 内任何 panic 只会以 `JoinError` 的形态**静静留在句柄里** —— 现象是
//! "该口/该通道从此不再采集，而进程、日志、服务状态一切正常"（静默停采）。
//!
//! **本模块只做观测，不改变任何 task 的语义**：
//! - **panic 语义不变**：task 仍因 panic 终止（本模块**不**捕获、不吞、不重启）；
//! - **abort 语义不变**：`observe_task` 返回的句柄被 abort 时，**被观测 task 一并 abort**
//!   （靠 [`AbortOnDrop`]：观测 task 的 future 被 drop ⇒ 其栈上哨兵 drop ⇒ abort 内层 task）。
//!   这是"句柄入 `TaskGuard` 后 abort 仍能真正停掉采集"的保证 —— 若只把内层 `JoinHandle`
//!   交给观测 task 而 `TaskGuard` 拿着观测句柄，内层 task 会**脱离**（drop 句柄 ≠ 停 task）。
//!
//! **边界（本轮不做，如实登记）**：不做 supervisor（自动重建 / 退避重试 / 告警去重 / 上报
//! `ServiceStatus`）。本轮只提供"panic 可被观测到"的最小能力：日志 + [`TaskWatch`] 标志位。
//! `SouthScheduler::spawn` 的 5 个口 task 尚未接本模块（其调用点注释已登记 supervisor 属
//! Task 7 决议、本轮不动）；**PCS 采集 task 已在 `startup.rs` 接入**（那是最关键的一条：
//! PCS 无采集 ⇒ 联锁的 `last_run_state` 陈旧 ⇒ 停机确认失效）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;

/// 一条被观测 task 的**完成态**（可由外部随时读，无锁）。
///
/// 两个标志互相独立：`finished` 覆盖"任何原因结束"（含被 abort）；`panicked` 只覆盖
/// panic（`JoinError::is_panic()`）—— 取消**不算** panic（正常关停路径会 abort，若把
/// 取消也记成 panic，关停时必然刷一条假 error）。
#[derive(Debug, Default)]
pub struct TaskWatch {
    finished: AtomicBool,
    panicked: AtomicBool,
}

impl TaskWatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// 该 task 是否已结束（panic / 正常返回 / 被取消）。
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    /// 该 task 是否**因 panic 结束**。
    pub fn panicked(&self) -> bool {
        self.panicked.load(Ordering::SeqCst)
    }
}

/// 观测 task 被 abort 时**连带 abort 被观测 task**（见模块头"abort 语义不变"）。
struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// 包装一条常驻 task：返回的句柄交给 `TaskGuard`（abort 名单），内层 task 的完成态写入
/// `watch` 并记日志。
///
/// `name` 只用于日志字段（如 `"pcs_collect"` / `"southd_port"`）。
///
/// 三态日志：
/// - `Ok(())` ⇒ **error**（常驻循环正常返回 = 不应发生，等同停采）
/// - `Err(panicked)` ⇒ **error**（含 panic 摘要，一望可知"停采原因是 task 内 panic"）
/// - `Err(cancelled)` ⇒ **debug**（关停路径的正常形态，不打 error 噪声）
pub fn observe_task(
    name: &'static str,
    jh: JoinHandle<()>,
    watch: Arc<TaskWatch>,
) -> JoinHandle<()> {
    let abort = AbortOnDrop(jh.abort_handle());
    tokio::spawn(async move {
        // 哨兵随本 future 存活：本观测 task 被 abort ⇒ future 被 drop ⇒ 内部 task 一并 abort
        let _abort = abort;
        match jh.await {
            Ok(()) => {
                watch.finished.store(true, Ordering::SeqCst);
                tracing::error!(
                    task = name,
                    "常驻 task 正常返回（不应发生）——该通道采集/服务已停止，须重启进程或重建该 task"
                );
            }
            Err(e) if e.is_panic() => {
                watch.panicked.store(true, Ordering::SeqCst);
                watch.finished.store(true, Ordering::SeqCst);
                tracing::error!(
                    task = name,
                    error = %e,
                    "常驻 task 因 panic 终止——该通道采集/服务已静默停止，须重启进程或重建该 task"
                );
            }
            Err(e) => {
                watch.finished.store(true, Ordering::SeqCst);
                tracing::debug!(task = name, error = %e, "常驻 task 被取消（关停路径）");
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// panic 必须被**观测到**：`watch.panicked()` 置位 + 外层句柄**正常完成**（不把 panic
    /// 再抛给调用方，否则 `TaskGuard` 侧仍要额外处理）。
    ///
    /// 判别力：把 `observe_task` 换成"原样返回 `jh`"（= 改动前的"只持有不观测"）⇒
    /// `watch` 两个标志恒 false ⇒ 两条断言红。
    #[tokio::test]
    async fn observe_task_flags_panic_and_absorbs_it() {
        let watch = Arc::new(TaskWatch::new());
        let jh = tokio::spawn(async {
            panic!("模拟采集 task 内 panic");
        });
        let obs = observe_task("ut_panicking_task", jh, watch.clone());
        assert!(
            obs.await.is_ok(),
            "观测 task 自身必须正常结束（不把 panic 二次抛出）"
        );
        assert!(watch.finished(), "task 结束须置 finished");
        assert!(
            watch.panicked(),
            "panic 终止必须置 panicked（B-9 的核心判据）"
        );
    }

    /// 正常返回也要被观测（常驻循环返回 = 停采，与 panic 同样需要可见）。
    #[tokio::test]
    async fn observe_task_flags_normal_return() {
        let watch = Arc::new(TaskWatch::new());
        let obs = observe_task("ut_returning_task", tokio::spawn(async {}), watch.clone());
        obs.await.unwrap();
        assert!(watch.finished(), "正常返回须置 finished");
        assert!(
            !watch.panicked(),
            "正常返回**不得**被误报为 panic（否则关停/正常退出会刷假 error）"
        );
    }

    /// **abort 语义不变**：观测句柄被 abort ⇒ 内层 task 一并 abort（否则采集停不下来）。
    ///
    /// 判别力：去掉 [`AbortOnDrop`] 哨兵（只 `jh.await`）⇒ 内层 task 仍活着 ⇒ 本用例红。
    #[tokio::test]
    async fn aborting_observer_aborts_inner_task() {
        static DROPPED: AtomicUsize = AtomicUsize::new(0);

        /// 被 abort 时（future 被 drop）留下"我死了"的痕迹
        struct Marker;
        impl Drop for Marker {
            fn drop(&mut self) {
                DROPPED.fetch_add(1, Ordering::SeqCst);
            }
        }

        DROPPED.store(0, Ordering::SeqCst);
        let watch = Arc::new(TaskWatch::new());
        let inner = tokio::spawn(async {
            let _m = Marker;
            std::future::pending::<()>().await; // 永不自退
        });
        let obs = observe_task("ut_abort_task", inner, watch.clone());
        // 让内层 task 真正跑起来（否则 abort 打在"尚未轮询"的 future 上，Marker 未构造）
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        obs.abort();
        for _ in 0..50 {
            if DROPPED.load(Ordering::SeqCst) > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            DROPPED.load(Ordering::SeqCst),
            1,
            "观测句柄被 abort ⇒ 内层 task 必须一并 abort（Marker 被 drop 恰一次）"
        );
    }
}
