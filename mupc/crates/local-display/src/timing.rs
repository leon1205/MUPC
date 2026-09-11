//! 事件循环骨架（工作单元 C；设计 §5.2「LVGL 集成与事件循环」）。
//!
//! # 形状（与设计 §5.2 骨架逐行对应）
//!
//! ```text
//! loop {
//!     lv_next = ticker.timer_handler();                  // ① 渲染/定时器（LVGL 自决何时重绘）
//!     timeout = min(lv_next, host.next_deadline_ms()-now, cap≤500ms)
//!     poller.poll(timeout);                              // ② 唯一阻塞点（空闲即让出 CPU）
//!     host.pump();                                       // ③ evdev → 快照
//!     host.read_indev();                                 // ④ lv_indev_read（命中/派发）
//!     host.on_lv_events();                               // ⑤ LVGL 事件 → 业务动作
//!     host.tick(clock.now_ms());                         // ⑥ 通道推进 + 状态刷新（非阻塞）
//!     ticker.timer_handler();                            // ⑦ 把本拍 lv_obj 变更落到脏区
//! }
//! ```
//!
//! # 为什么是「trait + 注入」（可测性，设计 §11.1/§11.2）
//!
//! 事件循环本身**不碰 LVGL**：`ticker` 是抽象（生产用 [`LvglTicker`] 转发
//! [`crate::lvgl::timer_handler`]），`clock` / `poller` / `host` 同样注入。于是
//! 「超时计算 / 停止语义 / 调用次序 / 不忙等」可在**本机 Windows**上用**假时钟 + 假 poller**
//! 断言，**不真 sleep、不初始化 LVGL**（后者受设计 §11.1「LVGL 非线程安全 ⇒ 用例串行化」约束）。
//!
//! # §5.2 五条不变量的落点
//!
//! | 不变量 | 落点 |
//! |--------|------|
//! | 1 唯一阻塞点是 `poll`，超时 = `min(lv_timer_handler 返回值, 通道截止, ≤500 ms)` | [`compute_timeout_ms`] + [`run`] 的 ② 步；**生产实现** [`FdPoller`]（Linux `poll(2)`；无触摸设备时 `poll(NULL,0,t)`，**仍是唯一阻塞点**）；**无 `loop {}` 自旋、无 `sleep`/`poll` 混用** |
//! | 2 渲染只在 `lv_timer_handler()` 内 | 循环只经 `ticker.timer_handler()`（①/⑦）驱动；本模块**不出现** `lv_refr_now` |
//! | 3 阻塞 I/O 不得在回调内 | `host.tick`（⑥）是唯一「通道推进」位置，且由设计 §5.5 钉死为非阻塞状态机 |
//! | 4 所有 LVGL 调用在事件循环线程内 | 本模块不提供任何跨线程 API；`ticker`/`host` 均为 `&mut` 单线程借用 |
//! | 5 回调内不 panic | 本模块不注册回调；[`run`] 内全部为可失败之外的纯控制流（无 `unwrap`） |
//!
//! ## 超时公式的唯一例外：防忙等硬钳制 + 退避阶梯（评审 C-② / Important 4 整改）
//!
//! 设计字面公式就是 [`compute_timeout_ms`]（**不改**）。其外层另有一个**显式、注释清楚的
//! 例外**：[`apply_zero_timeout_clamp`] —— 连续 `> ` [`ZERO_TIMEOUT_BURST_LIMIT`] 拍算得 0 时，
//! 按下限 [`ZERO_CLAMP_LADDER_MS`] **逐级抬升**（1→2→4→…→500 ms，到顶饱和）。
//! 理由：LVGL 若持续返回 0，`poll(0)` 会**立即返回**，循环退化为等价的 `loop {}` 自旋，
//! 违背 PRD §4.1.1 / §5.2 不变量 1 的立意（空闲让出 CPU）。
//!
//! **为什么是阶梯而不是恒定 1 ms**：宿主若**长期**给出已过期的截止时刻，恒定 1 ms 会让
//! 循环**永久 1000 拍/s 空转**；阶梯让下限随病态持续时长增长，最终饱和到 [`MAX_POLL_TIMEOUT_MS`]
//! （即回到正常空闲节拍）。诊断口径见 [`LoopStats::max_zero_clamp_streak`]
//! （**偶发**钳制 ≤ [`ZERO_TIMEOUT_BURST_LIMIT`]；**持续**钳制则该值单调增长）。
//!
//! ## 生产 `Poller`（评审 C-① 整改）
//!
//! [`FdPoller`]（`#[cfg(target_os = "linux")]`）是 [`Poller`] 的**生产实现**：把
//! [`crate::touch::TouchDevice::fd`] 交给 `poll(2)`。其可跨平台单测的部分（超时换算 /
//! 无 fd 的 `poll(NULL,0,t)` 分支 / `EINTR` 重试判定 / 绝对截止）已抽成纯函数
//! （[`timeout_ms_to_c_int`] / [`poll_wait_target`] / [`remaining_ms`] / [`wait_with_retry`]），
//! 本机 Windows 可断言。
//!
//! ## `poll` 失败：不紧循环、不刷屏、有界终止（Critical 2 整改）
//!
//! `poll(2)` 的失败**不再**被静默当成「本拍无事件」：[`PollOutcome::Failed`] 把 errno 上抛给
//! [`run`]，由后者维护**连续失败计数**：
//!
//! | 连续失败 | 行为 | 理由 |
//! |---------|------|------|
//! | `1` | 打一条日志 | 可诊断，不刷屏 |
//! | `== `[`POLL_FAIL_FALLBACK_AFTER`] | 再打一条「转兜底」日志 | 一次性提示 |
//! | `> `[`POLL_FAIL_FALLBACK_AFTER`] | 改调 [`Poller::poll_fallback`]（`poll(NULL,0,t)` 语义）| **仍真阻塞** ⇒ 不忙等；期间**抑制日志** |
//! | `>= `[`POLL_FAIL_ABORT_AFTER`] | [`run`] 返回 [`PollFailure`] | poll 已不可用，必须让调用方知道（不无限降级） |
//!
//! ## `EINTR`：绝对截止时刻语义（Important 3 整改）
//!
//! [`wait_with_retry`] 首次等待**完整 timeout**，**重试按剩余时间**（绝对截止）—— 信号风暴下
//! 不再「每次重置完整 timeout」（那会使 `poll` 永不超时 ⇒ `iterations` 停滞、stop 永不检查）。
//! 剩余 ≤ 0 即按 [`PollOutcome::Timeout`] 返回（POSIX 正确语义）。

use std::time::Instant;

/// `poll` 超时硬上界（设计 §5.2 不变量 1：`≤500 ms`；与读通道轮询节拍一致）。
pub const MAX_POLL_TIMEOUT_MS: u64 = 500;

/// 单调毫秒时钟（注入用）。
pub trait Clock {
    /// 自时基原点起的毫秒数（单调，与 [`crate::lvgl::tick_ms`] 同口径）。
    fn now_ms(&self) -> u64;
}

/// 生产时钟（`Instant` 单调时基）。
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// 以「现在」为时基原点。
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }
}

/// 一次 `poll` 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollOutcome {
    /// 有可读事件（evdev fd 可读）。
    Ready,
    /// 超时（无事件）—— 正常的空闲路径。
    Timeout,
    /// `poll(2)` **失败**（errno）—— 与「无事件」**必须区分**（Critical 2）：
    /// 静默当超时会让「持续失败」退化成每拍立即返回的**紧循环 + stderr 刷屏**。
    Failed(i32),
}

/// 多路复用等待（事件循环的**唯一阻塞点**）。
pub trait Poller {
    /// 阻塞至多 `timeout_ms` 毫秒。实现必须**真的阻塞**（`poll(2)`/等价机制），
    /// **不得**退化为忙等自旋（PRD §4.1.1 / NF-02）。
    ///
    /// 失败以 [`PollOutcome::Failed`] 返回（**不得**静默当超时）；由 [`run`] 统一计数/兜底/终止。
    fn poll(&mut self, timeout_ms: u64) -> PollOutcome;

    /// **兜底阻塞**（Critical 2）：`poll(NULL, 0, t)` 语义 —— 不监听任何 fd，本拍交给内核
    /// 阻塞 `timeout_ms` 毫秒。[`run`] 只在「`poll` 连续失败超 [`POLL_FAIL_FALLBACK_AFTER`]」
    /// 后改调本方法，以维持「唯一阻塞点不得退化为忙等」不变量。
    ///
    /// **刻意不提供默认实现**：默认实现只能退化为「立即返回」，那正是本条要根治的忙等；
    /// 强制每个实现显式表态（生产实现 = `poll(NULL,0,t)`）。
    ///
    /// **兜底是「粘性」的**：[`run`] 一旦转入兜底就**不再探测** `poll`（无恢复探针）——
    /// 语义是「poll 已不可用 ⇒ 在 [`POLL_FAIL_ABORT_AFTER`] 内真阻塞让出 CPU，随后有界终止」。
    /// 需要恢复能力的调用方应在收到 [`PollFailure`] 后**重建 poller 并重启循环**。
    fn poll_fallback(&mut self, timeout_ms: u64);
}

/// 本拍 `poll(2)` 的等待目标（纯逻辑，跨平台可测）。
///
/// 存在的理由：**没有触摸设备时也必须有一个阻塞点**（EDGE-13 降级运行），否则「唯一阻塞点」
/// 在生产路径上为空 —— 那是 [`run`] 的不变量 1 的直接落空。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollWait {
    /// 监听该 fd（触摸设备可读即返回）。
    Fd(i32),
    /// 无 fd 可监听：仍**阻塞** `timeout` 毫秒（`poll(NULL, 0, timeout)`）——
    /// **仍然是唯一阻塞点**（等价 sleep），**不是**忙等。
    Sleep,
}

/// 由「触摸 fd」决定本拍等待目标（纯逻辑）。`None`（无触摸设备 / 降级运行）⇒ [`PollWait::Sleep`]。
pub fn poll_wait_target(fd: Option<i32>) -> PollWait {
    match fd {
        Some(fd) => PollWait::Fd(fd),
        None => PollWait::Sleep,
    }
}

/// 毫秒 → `poll(2)` 的超时入参（纯逻辑，跨平台可测；Linux 侧 `libc::c_int == i32`）。
///
/// - 入参是 `u64` ⇒ 结构上**不可能为负**：`poll(2)` 里负超时（尤其 `-1`）语义是**无限阻塞**，
///   一旦静默出现，停止标志就再也不会被检查到（`≤500 ms` 上界形同虚设）；
/// - 超 `i32::MAX`（约 24.8 天）钳到 `i32::MAX`，**绝不 `as i32` 静默截断**（截断可能得负值）。
pub fn timeout_ms_to_c_int(timeout_ms: u64) -> i32 {
    timeout_ms.min(i32::MAX as u64) as i32
}

/// 绝对截止时刻的**剩余时间**（纯逻辑）：`deadline − now`，已过则 0（绝不下溢成巨值）。
pub fn remaining_ms(deadline_ms: u64, now_ms: u64) -> u64 {
    deadline_ms.saturating_sub(now_ms)
}

/// 一次底层等待的**原始结果**（`poll(2)` 返回值归类；注入式单测用，不真 poll）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawPollResult {
    /// `ret > 0`：至少一个 fd 可读。
    Ready,
    /// `ret == 0`：超时（正常的空闲路径）。
    Timeout,
    /// `ret < 0`：失败，携带 errno。
    Failed(i32),
}

/// `EINTR` 感知的等待循环 —— **绝对截止时刻**语义（Important 3 整改；纯逻辑，跨平台可测）。
///
/// `raw_wait(remaining_ms)` 由调用方注入真实 `poll(2)`（或假实现）；`deadline_ms` 是本次等待的
/// 绝对截止（由 [`Clock::now_ms`] 同一时基给出）。
///
/// - `Ready` / `Timeout` ⇒ 直接返回；
/// - `EINTR`（被信号打断）⇒ **按剩余时间重试**；剩余已耗尽 ⇒ 按超时返回（不当作错误）；
/// - 其它 errno ⇒ `Err(errno)`，由调用方决定诊断/兜底/退出，**不静默当超时**。
///
/// **为什么必须是绝对截止**：若重试时重置完整 timeout，则「未设 `SA_RESTART` 的高频信号」
/// （如 unit B 可能用的 200 ms 看门狗）会让 `poll(500)` 被反复重置 ⇒ **永不超时** ⇒
/// 事件循环的 `iterations` 停滞、stop 标志永不检查，只能 SIGKILL。
///
/// 重试不会忙等：每次 `raw_wait()` 至少阻塞到「下一个信号」或剩余时间耗尽。
pub fn wait_with_retry<C, F>(
    deadline_ms: u64,
    clock: &C,
    mut raw_wait: F,
    eintr: i32,
) -> Result<PollOutcome, i32>
where
    C: Clock,
    F: FnMut(u64) -> RawPollResult,
{
    loop {
        let remain = remaining_ms(deadline_ms, clock.now_ms());
        match raw_wait(remain) {
            RawPollResult::Ready => return Ok(PollOutcome::Ready),
            RawPollResult::Timeout => return Ok(PollOutcome::Timeout),
            RawPollResult::Failed(e) if e == eintr => {
                // 剩余时间已耗尽 ⇒ 按超时返回（**不得**重置完整 timeout，否则信号风暴下永不超时）。
                if remaining_ms(deadline_ms, clock.now_ms()) == 0 {
                    return Ok(PollOutcome::Timeout);
                }
            }
            RawPollResult::Failed(e) => return Err(e),
        }
    }
}

/// 生产 [`Poller`]（Linux）：`poll(2)` 包住触摸 evdev fd（设计 §5.2 骨架 `poll(&touch.fd, t)`）。
///
/// - 有触摸设备 ⇒ `pollfd { fd, events: POLLIN, revents: 0 }`（触摸 fd 在 `open` 时已设非阻塞，
///   可读即返回，实际读事件由 `Host::pump` 完成）；
/// - **无触摸设备**（EDGE-13 降级）⇒ `poll(NULL, 0, t)`：**仍是唯一阻塞点**，不是忙等；
/// - `EINTR` ⇒ 按**剩余时间**重试（绝对截止，Important 3）；其它 errno ⇒ [`FdPoller::poll_checked`]
///   返回 `Err(errno)`，而 [`Poller::poll`] 侧上抛为 [`PollOutcome::Failed`]（**不 panic、不静默**，
///   由 [`run`] 计数/兜底/终止 —— Critical 2）。
///
/// ⚠️ 持 fd ⇒ **不实现 `Send`/`Sync`**（与 [`crate::touch::TouchDevice`] 同纪律，
/// 设计 §5.2 不变量 4：全部调用在事件循环线程内）。
#[cfg(target_os = "linux")]
pub struct FdPoller {
    fd: Option<std::os::fd::RawFd>,
    /// 绝对截止时刻的时基（[`wait_with_retry`] 的剩余时间来源）。
    clock: SystemClock,
}

#[cfg(target_os = "linux")]
impl FdPoller {
    /// 以触摸设备 fd 构造。
    pub fn new(fd: std::os::fd::RawFd) -> Self {
        Self {
            fd: Some(fd),
            clock: SystemClock::new(),
        }
    }

    /// 无触摸设备的构造（降级只读展示：仍以 `poll(NULL, 0, t)` 阻塞让出 CPU）。
    pub fn without_fd() -> Self {
        Self {
            fd: None,
            clock: SystemClock::new(),
        }
    }

    /// 可失败版本：`EINTR` 已按剩余时间自动重试；返回 `Err(errno)` 时由调用方决定诊断/退出。
    pub fn poll_checked(&mut self, timeout_ms: u64) -> Result<PollOutcome, i32> {
        let deadline = self.clock.now_ms().saturating_add(timeout_ms);
        let fd = self.fd;
        wait_with_retry(
            deadline,
            &self.clock,
            |remain| {
                let t = timeout_ms_to_c_int(remain);
                let ret = match poll_wait_target(fd) {
                    PollWait::Fd(fd) => {
                        let mut pfd = libc::pollfd {
                            fd,
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        // SAFETY: `pfd` 是本作用域内存活且已完全初始化的 `pollfd`；`nfds = 1`
                        // 与之一致；`t >= 0`（见 `timeout_ms_to_c_int`）故只做**有限**阻塞，
                        // 不存在「-1 无限阻塞」。poll 只读该结构体（revents 由内核写回），
                        // 借用期间无别名。
                        unsafe { libc::poll(&mut pfd, 1, t) }
                    }
                    // SAFETY: `fds` 为 NUL 指针且 `nfds = 0` ⇒ 内核不解引用该指针，仅做超时
                    // 等待（等价 `sleep`）；`t >= 0` ⇒ 有限阻塞。
                    PollWait::Sleep => unsafe { libc::poll(std::ptr::null_mut(), 0, t) },
                };
                if ret > 0 {
                    RawPollResult::Ready
                } else if ret == 0 {
                    RawPollResult::Timeout
                } else {
                    RawPollResult::Failed(
                        std::io::Error::last_os_error().raw_os_error().unwrap_or(-1),
                    )
                }
            },
            libc::EINTR,
        )
    }
}

#[cfg(target_os = "linux")]
impl Poller for FdPoller {
    fn poll(&mut self, timeout_ms: u64) -> PollOutcome {
        match self.poll_checked(timeout_ms) {
            Ok(o) => o,
            // 不 panic、不静默：把 errno 上抛给 `run` 统一计数 / 兜底阻塞 / 有界终止（Critical 2）。
            Err(errno) => PollOutcome::Failed(errno),
        }
    }

    fn poll_fallback(&mut self, timeout_ms: u64) {
        // SAFETY: `fds` 为 NUL 且 `nfds = 0` ⇒ 内核不解引用该指针，仅做有限超时等待
        // （等价 `sleep`）。这正是「poll 不可用但仍不得忙等」的兜底。
        unsafe { libc::poll(std::ptr::null_mut(), 0, timeout_ms_to_c_int(timeout_ms)) };
    }
}

/// LVGL 定时器驱动（`lv_timer_handler` 的抽象；返回值 = 距下次需要处理的毫秒数）。
pub trait Ticker {
    /// 驱动 LVGL 一次，返回距下次需要处理的毫秒数（直接作为 `poll` 超时上界）。
    fn timer_handler(&mut self) -> u32;
}

/// 生产实现：转发 [`crate::lvgl::timer_handler`]。
///
/// ⚠️ 含 LVGL 调用 ⇒ 只能在**事件循环线程**内使用（设计 §5.2 不变量 4）。
#[derive(Debug, Default)]
pub struct LvglTicker;

impl Ticker for LvglTicker {
    fn timer_handler(&mut self) -> u32 {
        crate::lvgl::timer_handler()
    }
}

/// 事件循环宿主（由工作单元 B 的 `ui::App` 实现：6 页控件树 + 通道客户端）。
///
/// 五个方法按设计 §5.2 骨架的 ③–⑥ 步顺序被调用；**全部在事件循环线程内**。
pub trait Host {
    /// ③ 读 evdev 事件 → 更新触摸快照（无事件则 no-op；**不得阻塞**）。
    ///
    /// 「无事件」**必须**是 no-op：非阻塞 fd 的 `EAGAIN`/`EWOULDBLOCK` 是**正常的空闲拍**，
    /// 不得上抛为错误（评审 Critical 1；契约落点在 [`crate::touch::pump_with`]）。
    fn pump(&mut self);
    /// ④ `lv_indev_read()` 主动投递 → LVGL 命中 / z-order / 滚动判定 / `LV_EVENT_*` 派发。
    fn read_indev(&mut self);
    /// ⑤ 取 LVGL 事件队列中的 UI 动作 → 业务（切页 / 提交 / 防抖）。
    fn on_lv_events(&mut self);
    /// 本拍到**下一次必须唤醒**的时刻（单调毫秒）：读通道轮询截止、
    /// 空闲回归截止（[`IdleTimer`]）、控制通道超时截止……的**最小值**。
    /// 返回 `None` ⇒ 本拍无时间驱动任务（只由 LVGL 与输入唤醒）。
    fn next_deadline_ms(&self) -> Option<u64>;
    /// ⑥ 推进通道状态机（**非阻塞**，设计 §5.5）+ 刷新受影响的 `lv_obj`。
    ///
    /// 注意：`lv_obj` 变更只是标脏，真正的重绘发生在 ⑦ 的 `timer_handler` 内。
    fn tick(&mut self, now_ms: u64);
}

/// 停止条件（信号处理/测试注入）。
pub trait Stop {
    /// 是否请求退出（每迭代查询一次）。
    fn stop_requested(&self) -> bool;
}

impl Stop for std::sync::atomic::AtomicBool {
    fn stop_requested(&self) -> bool {
        self.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Stop for std::sync::Arc<std::sync::atomic::AtomicBool> {
    fn stop_requested(&self) -> bool {
        self.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// 事件循环配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopConfig {
    /// `poll` 超时上界（毫秒）。**上界**：不得大于 [`MAX_POLL_TIMEOUT_MS`]（否则停机会变慢）。
    pub poll_cap_ms: u64,
}

impl LoopConfig {
    /// 构造并夹紧到 (0, [`MAX_POLL_TIMEOUT_MS`]]。
    pub fn new(poll_cap_ms: u64) -> Self {
        Self {
            poll_cap_ms: poll_cap_ms.clamp(1, MAX_POLL_TIMEOUT_MS),
        }
    }
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            poll_cap_ms: MAX_POLL_TIMEOUT_MS,
        }
    }
}

/// 事件循环统计（退出报告 / 测试断言 / 单元 B 诊断）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoopStats {
    /// 完成的迭代数。
    pub iterations: u64,
    /// 阻塞调用次数（`poll` **或**兜底 `poll_fallback`；**恒等于 `iterations`**
    /// ⇒ 每迭代恰好阻塞一次，无自旋）。
    pub poll_calls: u64,
    /// 其中「有事件」的次数。
    pub ready_events: u64,
    /// 超时算得 0 的迭代数（诊断哨：持续非零意味着 LVGL 或宿主有即时任务积压）。
    pub zero_timeout_iters: u64,
    /// 触发**防忙等硬钳制**的迭代数（连续 0 超时超阈值 ⇒ 抬到阶梯下限；正常恒为 0）。
    ///
    /// 真机上该值持续增长 ⇒ 「LVGL 一直返回 0」（异常/积压）。
    pub zero_timeout_clamps: u64,
    /// **最长连续钳制拍数**（诊断：区分「偶发钳制」`<=` [`ZERO_TIMEOUT_BURST_LIMIT`] 与
    /// 「持续钳制」——后者单调增长，且伴随着本次钳制下限沿 [`ZERO_CLAMP_LADDER_MS`] 升级）。
    pub max_zero_clamp_streak: u32,
    /// `poll` 失败（[`PollOutcome::Failed`]）的**总**次数。
    pub poll_failures: u64,
    /// 退出时的**连续** `poll` 失败拍数（任一次成功/超时即归零）。
    pub consecutive_poll_failures: u64,
    /// 因 `poll` 连续失败而改用 [`Poller::poll_fallback`] 的迭代数（兜底阻塞拍数）。
    pub poll_fallback_iters: u64,
    /// 最后一次的超时值（毫秒）。
    pub last_timeout_ms: u64,
    /// 最后一次 LVGL 给出的「距下次处理」值（毫秒）。
    pub lv_next_ms: u32,
}

/// **超时计算**（纯函数，设计 §5.2 不变量 1）：
/// `min(lv_timer_handler 返回值, 通道截止 − now, 上界 ≤500ms)`。
///
/// 截止已过（`deadline <= now`）⇒ 0（立即唤醒处理，不阻塞）。
pub fn compute_timeout_ms(
    lv_next_ms: u32,
    deadline_ms: Option<u64>,
    now_ms: u64,
    cap_ms: u64,
) -> u64 {
    let mut t = (lv_next_ms as u64).min(cap_ms);
    if let Some(d) = deadline_ms {
        t = t.min(d.saturating_sub(now_ms));
    }
    t
}

/// 连续「算得 0 ms」的迭代数**超过**该阈值后，才启用防忙等钳制（评审 C-②）。
///
/// 设计字面公式 [`compute_timeout_ms`] **不含**这一条；本常量是「§5.2 不变量 1 立意
/// （无自旋）」下的**显式防忙等下限**。取 2：允许 LVGL 偶尔连给两拍 0（正常抖动），
/// 但第三拍起必须真阻塞——否则 `poll(0)` 立即返回即等价 `loop {}`。
pub const ZERO_TIMEOUT_BURST_LIMIT: u32 = 2;

/// 防忙等钳制的**退避阶梯**（毫秒；Important 4 整改）。
///
/// 第 `k` 拍被钳制（`k` 从 1 起，仅计连续钳制拍）取下限 `ladder[k-1]`，到顶饱和。
/// 阶梯末端 = [`MAX_POLL_TIMEOUT_MS`]：宿主若**长期**给出过期截止，循环最终回到
/// 正常空闲节拍（而不是永久 1000 拍/s 空转）。
pub const ZERO_CLAMP_LADDER_MS: [u64; 10] = [1, 2, 4, 8, 16, 32, 64, 128, 256, 500];

/// **防忙等硬钳制 + 退避阶梯**（设计字面之外的显式例外；纯逻辑，跨平台可测）。
///
/// - 设计字面 = [`compute_timeout_ms`] 的 `min(lv_next, 截止, cap)`（**不改**）；
/// - 例外 = 连续 `> ` [`ZERO_TIMEOUT_BURST_LIMIT`] 拍算得 0 时，取下限
///   [`ZERO_CLAMP_LADDER_MS`]（按超阈值后的连续钳制次数逐级抬升、到顶饱和）。
///
/// `consecutive_zero` 是**含本拍在内**的连续 0 计数（从 1 起）。非 0 的 `computed_ms`
/// 一律原样返回 —— 钳制只动「持续 0」这一种病态，超时公式的可解释性不受影响。
pub fn apply_zero_timeout_clamp(computed_ms: u64, consecutive_zero: u32) -> u64 {
    if computed_ms != 0 || consecutive_zero <= ZERO_TIMEOUT_BURST_LIMIT {
        return computed_ms;
    }
    // 第 1 拍钳制（consecutive_zero = 阈值+1）取 ladder[0]。
    let step = (consecutive_zero - ZERO_TIMEOUT_BURST_LIMIT - 1) as usize;
    ZERO_CLAMP_LADDER_MS[step.min(ZERO_CLAMP_LADDER_MS.len() - 1)]
}

/// `poll` 连续失败多少拍后改用 [`Poller::poll_fallback`] 兜底阻塞（Critical 2）。
pub const POLL_FAIL_FALLBACK_AFTER: u32 = 3;

/// `poll` 连续失败多少拍后**终止** [`run`]（Critical 2；必须 > [`POLL_FAIL_FALLBACK_AFTER`]）。
///
/// 上限内是「给 poll 一点恢复机会 + 期间仍真阻塞（不忙等、不刷屏）」，
/// 超限就是「poll 已不可用」——必须让调用方知道，而不是永远降级下去。
pub const POLL_FAIL_ABORT_AFTER: u32 = 100;

// 常量关系在**编译期**钉住（改错即编译失败，不留到运行期/测试期）。
const _: () = assert!(POLL_FAIL_FALLBACK_AFTER >= 1);
const _: () = assert!(POLL_FAIL_ABORT_AFTER > POLL_FAIL_FALLBACK_AFTER);

/// [`run`] 因 `poll` 连续失败超 [`POLL_FAIL_ABORT_AFTER`] 而终止（Critical 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PollFailure {
    /// 最后一次 `poll(2)` 的 errno。
    pub errno: i32,
    /// 连续失败拍数（终止时）。
    pub consecutive: u64,
    /// 已完成的迭代数（诊断用）。
    pub iterations: u64,
}

impl std::fmt::Display for PollFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "poll 连续失败 {} 次（errno={}，已完成 {} 拍）：事件循环终止",
            self.consecutive, self.errno, self.iterations
        )
    }
}

impl std::error::Error for PollFailure {}

/// 跑事件循环直到 [`Stop`] 置位（设计 §5.2 骨架）。
///
/// 返回统计（`Ok`）或 [`PollFailure`]（`poll` 连续失败超上限 ⇒ 终止，Critical 2）。
/// **唯一阻塞点是 [`Poller::poll`]**（或其兜底 [`Poller::poll_fallback`]）；其余步骤全部为
/// 非阻塞调用（`pump`/`read_indev`/`on_lv_events`/`tick` 由 [`Host`] 保证非阻塞）。
///
/// 超时值 = 设计字面公式 [`compute_timeout_ms`] **叠加**一个显式例外
/// （[`apply_zero_timeout_clamp`]：连续多拍 0 ⇒ 按退避阶梯抬升，防 `poll(0)` 自旋）。
pub fn run<C, P, T, H, S>(
    clock: &C,
    poller: &mut P,
    ticker: &mut T,
    host: &mut H,
    stop: &S,
    cfg: &LoopConfig,
) -> Result<LoopStats, PollFailure>
where
    C: Clock,
    P: Poller,
    T: Ticker,
    H: Host,
    S: Stop,
{
    let mut st = LoopStats::default();
    // 连续「算得 0 ms」的迭代数（含本拍；非 0 拍归零）——防忙等钳制的输入。
    let mut consecutive_zero: u32 = 0;
    // 连续「被钳制」的迭代数（非钳制拍归零）——退避阶梯的输入 / 诊断。
    let mut clamp_streak: u32 = 0;
    // 连续 `poll` 失败拍数（成功/超时归零）——Critical 2 的输入。
    let mut consecutive_fail: u32 = 0;
    let mut last_errno: i32 = 0;

    while !stop.stop_requested() {
        // ① 渲染 / 定时器 / 动画：LVGL 自决何时重绘脏区（不变量 2）。
        let lv_next = ticker.timer_handler();
        st.lv_next_ms = lv_next;

        // ② 唯一阻塞点：超时 = min(LVGL 下次处理时刻, 宿主截止, ≤500ms)。
        let now = clock.now_ms();
        // 设计字面公式（不改）；`computed` 即 §5.2 不变量 1 的可解释值。
        let computed = compute_timeout_ms(lv_next, host.next_deadline_ms(), now, cfg.poll_cap_ms);
        if computed == 0 {
            st.zero_timeout_iters += 1;
            consecutive_zero = consecutive_zero.saturating_add(1);
        } else {
            consecutive_zero = 0;
        }
        // 设计字面之外的**显式例外**（防忙等下限 + 退避阶梯）：阶梯结果仍不得超本拍上界。
        let timeout = apply_zero_timeout_clamp(computed, consecutive_zero).min(cfg.poll_cap_ms);
        if timeout != computed {
            st.zero_timeout_clamps += 1;
            clamp_streak = clamp_streak.saturating_add(1);
            st.max_zero_clamp_streak = st.max_zero_clamp_streak.max(clamp_streak);
        } else {
            clamp_streak = 0;
        }
        st.last_timeout_ms = timeout;

        if consecutive_fail >= POLL_FAIL_FALLBACK_AFTER {
            // 兜底：不再触达可能已损坏的 fd，但**仍真阻塞**（不忙等）；日志已抑制（不刷屏）。
            poller.poll_fallback(timeout);
            st.poll_fallback_iters += 1;
            consecutive_fail = consecutive_fail.saturating_add(1);
        } else {
            match poller.poll(timeout) {
                PollOutcome::Ready => {
                    st.ready_events += 1;
                    consecutive_fail = 0;
                }
                PollOutcome::Timeout => {
                    consecutive_fail = 0;
                }
                PollOutcome::Failed(errno) => {
                    last_errno = errno;
                    consecutive_fail = consecutive_fail.saturating_add(1);
                    st.poll_failures += 1;
                    // 只在「首次失败」与「转入兜底」各打一条 ⇒ 不刷屏（Critical 2）。
                    if consecutive_fail == 1 {
                        eprintln!(
                            "[local-display] poll(2) 失败（errno={errno}）：按无事件继续；\
                             连续失败将转入兜底阻塞"
                        );
                    } else if consecutive_fail == POLL_FAIL_FALLBACK_AFTER {
                        eprintln!(
                            "[local-display] poll(2) 连续 {consecutive_fail} 次失败（errno={errno}）：\
                             改用 poll(NULL,0,t) 兜底阻塞并抑制后续日志"
                        );
                    }
                }
            }
        }
        st.poll_calls += 1;

        // ③④⑤⑥ 输入 → 投递 → 业务 → 通道/状态刷新（均不得阻塞，不变量 3）。
        host.pump();
        host.read_indev();
        host.on_lv_events();
        host.tick(clock.now_ms());

        // ⑦ 再驱动一次：把本拍 `lv_obj` 变更立刻落到脏区重绘（仍是 LVGL 内部渲染）。
        ticker.timer_handler();

        st.iterations += 1;
        st.consecutive_poll_failures = consecutive_fail as u64;

        if consecutive_fail >= POLL_FAIL_ABORT_AFTER {
            // poll 已不可用：**有界终止**（不再无限降级），把 errno 交给调用方处置（Critical 2）。
            return Err(PollFailure {
                errno: last_errno,
                consecutive: consecutive_fail as u64,
                iterations: st.iterations,
            });
        }
    }
    st.consecutive_poll_failures = consecutive_fail as u64;
    Ok(st)
}

/// 空闲回归计时器（TT-12 / 设计 §5.6）。
///
/// 语义：`--idle-timeout-secs` 内无触摸则「回归主状态页」。本类型只负责**计时**；
/// 「`dirty=true` 时不强制切页、改为顶部提示条」与「确认弹层打开期间不计时（TT-13）」
/// 属 UI 策略，由工作单元 B 在拿到 [`IdleTimer::is_expired`] 后决定（本模块不越界）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleTimer {
    timeout_secs: u64,
    deadline_ms: Option<u64>,
}

impl IdleTimer {
    /// 构造（`timeout_secs == 0` ⇒ **禁用**空闲回归，`deadline` 恒为 `None`）。
    ///
    /// 秒→毫秒用 [`saturating_mul`](u64::saturating_mul)：`--idle-timeout-secs` 是 CLI 入参，
    /// 极大值不得在 debug 下 panic（溢出）或在 release 下静默 wrap 成**很小的**超时。
    pub fn new(timeout_secs: u64, now_ms: u64) -> Self {
        let mut t = Self {
            timeout_secs,
            deadline_ms: None,
        };
        if timeout_secs > 0 {
            t.deadline_ms = Some(now_ms.saturating_add(timeout_secs.saturating_mul(1000)));
        }
        t
    }

    /// 超时秒数（0 = 禁用）。
    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    /// 是否禁用。
    pub fn is_disabled(&self) -> bool {
        self.timeout_secs == 0
    }

    /// 触摸事件到达 → 重置计时（设计 §5.6 TT-12）。
    pub fn on_touch(&mut self, now_ms: u64) {
        if self.timeout_secs > 0 {
            self.deadline_ms = Some(now_ms.saturating_add(self.timeout_secs.saturating_mul(1000)));
        }
    }

    /// 到期时刻（`None` = 禁用）；应并入 [`Host::next_deadline_ms`] 的最小值里，
    /// 否则空闲回归会被 `poll` 的 500 ms 上界之外的其他截止掩盖。
    pub fn deadline_ms(&self) -> Option<u64> {
        self.deadline_ms
    }

    /// 是否已到期（禁用时恒为 `false`）。
    pub fn is_expired(&self, now_ms: u64) -> bool {
        matches!(self.deadline_ms, Some(d) if now_ms >= d)
    }

    /// 到期后重排下一次（调用方在「已处理一次回归」后调用，避免反复触发）。
    pub fn rearm(&mut self, now_ms: u64) {
        self.on_touch(now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::rc::Rc;

    // ---- ④-a 超时计算 ----

    #[test]
    fn timeout_is_min_of_lvgl_deadline_and_cap() {
        // LVGL 更小
        assert_eq!(compute_timeout_ms(100, Some(1000), 0, 500), 100);
        // 通道截止更小
        assert_eq!(compute_timeout_ms(400, Some(120), 0, 500), 120);
        // 上界更小
        assert_eq!(compute_timeout_ms(4000, Some(9000), 0, 500), 500);
        // 无截止 ⇒ 只比 LVGL 与上界
        assert_eq!(compute_timeout_ms(30, None, 0, 500), 30);
        assert_eq!(compute_timeout_ms(0, None, 0, 500), 0);
        // 截止已过 ⇒ 0（立即唤醒处理，不阻塞）
        assert_eq!(compute_timeout_ms(500, Some(700), 700, 500), 0);
        assert_eq!(compute_timeout_ms(500, Some(700), 999, 500), 0);
        // 上界本身即 500（设计硬上界）
        assert_eq!(MAX_POLL_TIMEOUT_MS, 500);
    }

    #[test]
    fn loop_config_clamps_cap_to_hard_limit() {
        assert_eq!(LoopConfig::new(5000).poll_cap_ms, MAX_POLL_TIMEOUT_MS);
        assert_eq!(LoopConfig::new(0).poll_cap_ms, 1);
        assert_eq!(LoopConfig::new(200).poll_cap_ms, 200);
        assert_eq!(LoopConfig::default().poll_cap_ms, MAX_POLL_TIMEOUT_MS);
    }

    // ---- ④-b 事件循环：注入时钟 + 假 poller/ticker/host ----

    /// 假时钟（不真 sleep；测试手动推进，也可由假 poller 推进）。
    #[derive(Clone)]
    struct FakeClock(Rc<Cell<u64>>);
    impl FakeClock {
        fn new() -> Self {
            Self(Rc::new(Cell::new(0)))
        }
        fn set(&self, v: u64) {
            self.0.set(v);
        }
    }
    impl Clock for FakeClock {
        fn now_ms(&self) -> u64 {
            self.0.get()
        }
    }

    /// 阻塞调用日志（`poll` / `poll_fallback` 的次序）。
    type CallLog = Rc<RefCell<Vec<&'static str>>>;

    /// 记录超时并按**脚本**返回结果（脚本耗尽后恒 [`PollOutcome::Timeout`]）；**不真阻塞**。
    struct FakePoller {
        outcomes: Rc<RefCell<VecDeque<PollOutcome>>>,
        timeouts: Rc<RefCell<Vec<u64>>>,
        fallbacks: Rc<RefCell<Vec<u64>>>,
        log: CallLog,
    }
    impl Poller for FakePoller {
        fn poll(&mut self, timeout_ms: u64) -> PollOutcome {
            self.log.borrow_mut().push("poll");
            self.timeouts.borrow_mut().push(timeout_ms);
            self.outcomes
                .borrow_mut()
                .pop_front()
                .unwrap_or(PollOutcome::Timeout)
        }
        fn poll_fallback(&mut self, timeout_ms: u64) {
            self.log.borrow_mut().push("poll_fallback");
            self.fallbacks.borrow_mut().push(timeout_ms);
        }
    }

    /// 按脚本返回 `timer_handler` 的值（脚本耗尽后返回 500）。
    struct FakeTicker {
        script: Rc<RefCell<Vec<u32>>>,
        log: CallLog,
    }
    impl Ticker for FakeTicker {
        fn timer_handler(&mut self) -> u32 {
            self.log.borrow_mut().push("timer_handler");
            let mut s = self.script.borrow_mut();
            if s.is_empty() {
                500
            } else {
                s.remove(0)
            }
        }
    }

    struct FakeHost {
        deadline: Option<u64>,
        log: CallLog,
    }
    impl Host for FakeHost {
        fn pump(&mut self) {
            self.log.borrow_mut().push("pump");
        }
        fn read_indev(&mut self) {
            self.log.borrow_mut().push("read_indev");
        }
        fn on_lv_events(&mut self) {
            self.log.borrow_mut().push("on_lv_events");
        }
        fn next_deadline_ms(&self) -> Option<u64> {
            self.log.borrow_mut().push("next_deadline");
            self.deadline
        }
        fn tick(&mut self, _now_ms: u64) {
            self.log.borrow_mut().push("tick");
        }
    }

    /// 调 `stop_after` 次 `stop_requested` 后返回 `true`。
    struct FakeStop {
        seen: Cell<usize>,
        stop_after: usize,
    }
    impl Stop for FakeStop {
        fn stop_requested(&self) -> bool {
            let n = self.seen.get();
            self.seen.set(n + 1);
            n >= self.stop_after
        }
    }

    struct Harness {
        clock: FakeClock,
        poller: FakePoller,
        ticker: FakeTicker,
        host: FakeHost,
        stop: FakeStop,
        log: CallLog,
        timeouts: Rc<RefCell<Vec<u64>>>,
        fallbacks: Rc<RefCell<Vec<u64>>>,
        outcomes: Rc<RefCell<VecDeque<PollOutcome>>>,
    }

    impl Harness {
        /// 跑一轮（`LoopConfig::default()`）。
        fn run(&mut self) -> Result<LoopStats, PollFailure> {
            run(
                &self.clock,
                &mut self.poller,
                &mut self.ticker,
                &mut self.host,
                &self.stop,
                &LoopConfig::default(),
            )
        }
    }

    fn harness(script: Vec<u32>, deadline: Option<u64>, stop_after: usize) -> Harness {
        harness_with_outcomes(script, deadline, stop_after, Vec::new())
    }

    fn harness_with_outcomes(
        script: Vec<u32>,
        deadline: Option<u64>,
        stop_after: usize,
        outcomes: Vec<PollOutcome>,
    ) -> Harness {
        let log: CallLog = Rc::new(RefCell::new(Vec::new()));
        let timeouts = Rc::new(RefCell::new(Vec::new()));
        let fallbacks = Rc::new(RefCell::new(Vec::new()));
        let outcomes = Rc::new(RefCell::new(outcomes.into()));
        Harness {
            clock: FakeClock::new(),
            poller: FakePoller {
                outcomes: Rc::clone(&outcomes),
                timeouts: Rc::clone(&timeouts),
                fallbacks: Rc::clone(&fallbacks),
                log: Rc::clone(&log),
            },
            ticker: FakeTicker {
                script: Rc::new(RefCell::new(script)),
                log: Rc::clone(&log),
            },
            host: FakeHost {
                deadline,
                log: Rc::clone(&log),
            },
            stop: FakeStop {
                seen: Cell::new(0),
                stop_after,
            },
            log,
            timeouts,
            fallbacks,
            outcomes,
        }
    }

    /// 每迭代恰好一次 `poll`、恰好两次 `timer_handler`，且调用次序与设计 §5.2 骨架一致。
    #[test]
    fn loop_calls_in_designed_order_once_per_iteration() {
        let mut h = harness(vec![100, 60, 0], Some(400), 3);
        let st = h.run().unwrap();
        assert_eq!(st.iterations, 3);
        assert_eq!(st.poll_calls, 3, "每迭代恰好阻塞一次（无自旋）");
        assert!(h.fallbacks.borrow().is_empty(), "无失败 ⇒ 不触达兜底");
        assert_eq!(
            *h.log.borrow(),
            vec![
                "timer_handler",
                "next_deadline",
                "poll",
                "pump",
                "read_indev",
                "on_lv_events",
                "tick",
                "timer_handler",
                "timer_handler",
                "next_deadline",
                "poll",
                "pump",
                "read_indev",
                "on_lv_events",
                "tick",
                "timer_handler",
                "timer_handler",
                "next_deadline",
                "poll",
                "pump",
                "read_indev",
                "on_lv_events",
                "tick",
                "timer_handler",
            ]
        );
    }

    /// 超时值 = `min(lv_timer_handler 返回, 通道截止−now, ≤500)`，逐拍核对。
    #[test]
    fn loop_passes_computed_timeout_to_poll() {
        // now 恒为 0（FakeClock 不推进）；deadline = 400 ⇒ 前三拍取 min(lv_next, 400, 500)
        let mut h = harness(vec![100, 450, 900, 4000], Some(400), 4);
        let st = h.run().unwrap();
        assert_eq!(*h.timeouts.borrow(), vec![100, 400, 400, 400]);
        assert_eq!(st.last_timeout_ms, 400);
        assert_eq!(st.zero_timeout_iters, 0);
        assert_eq!(st.ready_events, 0, "假 poller 恒超时 ⇒ 无 ready");
    }

    /// **不得忙等**：只要 LVGL 给出正间隔，`poll` 就必须拿到正超时（阻塞那么久）。
    #[test]
    fn loop_never_spins_when_lvgl_has_future_work() {
        let mut h = harness(vec![500, 500, 500], None, 3);
        let st = h.run().unwrap();
        let ts = h.timeouts.borrow();
        assert_eq!(ts.len(), 3);
        assert!(
            ts.iter().all(|t| *t > 0),
            "LVGL 有未来任务时必须阻塞等它：{ts:?}"
        );
        assert_eq!(st.poll_calls, st.iterations);
        assert_eq!(st.zero_timeout_iters, 0);
    }

    /// 截止已过 ⇒ 超时 0（立即处理，不阻塞），且仍只 poll 一次（不空转）。
    #[test]
    fn expired_deadline_yields_zero_timeout_without_spinning() {
        let mut h = harness(vec![500, 500], Some(0), 2);
        let st = h.run().unwrap();
        assert_eq!(*h.timeouts.borrow(), vec![0, 0]);
        assert_eq!(st.zero_timeout_iters, 2);
        assert_eq!(st.poll_calls, 2, "0 超时也只 poll 一次/迭代（由宿主消除积压）");
        assert_eq!(st.zero_timeout_clamps, 0, "仅两拍连 0，未超阈值 ⇒ 不触发钳制");
    }

    /// 停止标志：置位后不再迭代（停机延迟 ≤ 1 个 poll 周期）。
    #[test]
    fn stop_flag_terminates_loop() {
        let mut h = harness(vec![500; 10], None, 0);
        let st = h.run().unwrap();
        assert_eq!(st.iterations, 0);
        assert_eq!(st.poll_calls, 0);
        assert!(h.log.borrow().is_empty(), "停止标志在首个 sleep 前即生效");

        let mut h = harness(vec![500; 10], None, 2);
        let st = h.run().unwrap();
        assert_eq!(st.iterations, 2);
    }

    /// Minor 5 整改：`ready_events` 分支（FakePoller 恒 Timeout 时无覆盖）必须有用例。
    #[test]
    fn ready_outcomes_are_counted_and_reset_failures() {
        let mut h = harness_with_outcomes(
            vec![500; 6],
            None,
            5,
            vec![
                PollOutcome::Ready,
                PollOutcome::Timeout,
                PollOutcome::Ready,
                PollOutcome::Ready,
                PollOutcome::Timeout,
            ],
        );
        let st = h.run().unwrap();
        assert_eq!(st.iterations, 5);
        assert_eq!(st.ready_events, 3, "Ready 分支计数（此前无覆盖）");
        assert_eq!(st.poll_calls, st.iterations);
        assert_eq!(st.poll_failures, 0);
        assert_eq!(st.consecutive_poll_failures, 0);
        assert!(h.fallbacks.borrow().is_empty());
    }

    #[test]
    fn system_clock_is_monotonic_and_loose() {
        let c = SystemClock::new();
        let a = c.now_ms();
        let b = c.now_ms();
        assert!(b >= a);
        assert!(a < 60_000, "刚构造的时基原点应接近 0");
        assert!(SystemClock::default().now_ms() < 60_000);
    }

    // ---- ⑥ 空闲回归计时器（TT-12） ----

    #[test]
    fn idle_timer_resets_on_touch_and_expires() {
        let mut t = IdleTimer::new(60, 1_000);
        assert_eq!(t.timeout_secs(), 60);
        assert!(!t.is_disabled());
        assert_eq!(t.deadline_ms(), Some(61_000));
        assert!(!t.is_expired(60_999));
        assert!(t.is_expired(61_000));
        assert!(t.is_expired(99_999));

        // 触摸重置（TT-12：触摸事件重置计时）
        t.on_touch(50_000);
        assert_eq!(t.deadline_ms(), Some(110_000));
        assert!(!t.is_expired(61_000), "重置后不得因旧截止而触发");

        // 到期后重排
        t.rearm(110_000);
        assert_eq!(t.deadline_ms(), Some(170_000));
    }

    /// Minor 1 整改：秒→毫秒必须**饱和**（debug 不 panic / release 不 wrap 成极小值）。
    #[test]
    fn idle_timer_seconds_to_ms_saturates() {
        let t = IdleTimer::new(u64::MAX, 1_000);
        assert_eq!(
            t.deadline_ms(),
            Some(u64::MAX),
            "极大秒数不得溢出成小 deadline"
        );
        let mut t = IdleTimer::new(u64::MAX, u64::MAX - 10);
        t.on_touch(u64::MAX - 10);
        assert_eq!(t.deadline_ms(), Some(u64::MAX));
        // 正常量级不受影响
        assert_eq!(IdleTimer::new(3, 40).deadline_ms(), Some(3_040));
    }

    #[test]
    fn idle_timer_zero_means_disabled() {
        let mut t = IdleTimer::new(0, 1_000);
        assert!(t.is_disabled());
        assert_eq!(t.deadline_ms(), None);
        assert!(!t.is_expired(u64::MAX));
        t.on_touch(u64::MAX - 1);
        assert_eq!(t.deadline_ms(), None, "禁用时触摸也不得排出截止");
    }

    // ---- ④-c 防忙等硬钳制 + 退避阶梯（评审 C-② / Important 4 整改） ----

    /// 纯逻辑：只有「连续超阈值」才钳制；下限沿阶梯升级并到顶饱和；非 0 值**完全不受影响**。
    #[test]
    fn zero_clamp_is_explicit_exception_with_backoff_ladder() {
        assert_eq!(ZERO_TIMEOUT_BURST_LIMIT, 2);
        // 连续 1、2 拍：设计字面 0 保持 0（正常抖动）
        assert_eq!(apply_zero_timeout_clamp(0, 1), 0);
        assert_eq!(apply_zero_timeout_clamp(0, 2), 0);
        // 第 N+1 = 3 拍起：按下限阶梯抬升（1 → 2 → 4 → 8 …）
        assert_eq!(apply_zero_timeout_clamp(0, 3), 1);
        assert_eq!(apply_zero_timeout_clamp(0, 4), 2);
        assert_eq!(apply_zero_timeout_clamp(0, 5), 4);
        assert_eq!(apply_zero_timeout_clamp(0, 6), 8);
        // 到顶饱和（永不小于 1、永不无限增长）
        let top = *ZERO_CLAMP_LADDER_MS.last().unwrap();
        assert_eq!(top, MAX_POLL_TIMEOUT_MS, "阶梯末端 = poll 硬上界");
        assert_eq!(apply_zero_timeout_clamp(0, 12), top);
        assert_eq!(apply_zero_timeout_clamp(0, u32::MAX), top);
        assert!(ZERO_CLAMP_LADDER_MS.windows(2).all(|w| w[0] < w[1]), "阶梯单调递增");
        // 非 0：一律原样（钳制不得改写 `min(...)` 的正常结果）
        for t in [1u64, 7, 500] {
            assert_eq!(apply_zero_timeout_clamp(t, 1), t);
            assert_eq!(apply_zero_timeout_clamp(t, 9), t);
        }
    }

    /// 循环级：LVGL 连给 0 ⇒ 前两拍 0，第三拍起 `poll` 拿到阶梯下限（不再可能是 `poll(0)` 自旋），
    /// 且**持续病态**时下限继续升级（Important 4：不是永久 1 ms 空转）。
    ///
    /// 注：`FakeTicker` 在每拍被调两次（骨架 ①/⑦），故脚本按 2 倍填写（每拍取前一个）。
    #[test]
    fn loop_clamps_consecutive_zero_timeouts_with_escalation() {
        let mut h = harness(vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 500, 500], None, 6);
        let st = h.run().unwrap();
        assert_eq!(
            *h.timeouts.borrow(),
            vec![0, 0, 1, 2, 4, 8],
            "连续 0 的下限沿阶梯升级"
        );
        assert_eq!(st.zero_timeout_iters, 6);
        assert_eq!(st.zero_timeout_clamps, 4, "第 3~6 拍被钳制");
        assert_eq!(st.max_zero_clamp_streak, 4, "诊断：最长连续钳制 4 拍（持续病态）");
        assert_eq!(st.poll_calls, st.iterations);
        assert_eq!(st.poll_fallback_iters, 0, "无 poll 失败 ⇒ 恒不走兜底");
    }

    /// 非 0 拍**重置**连续计数（偶发 0 抖动不累积成钳制 / 不升级）。
    #[test]
    fn zero_clamp_counter_resets_after_nonzero_timeout() {
        let mut h = harness(
            vec![0, 0, 0, 0, 500, 500, 0, 0, 0, 0, 0, 0, 0, 0],
            None,
            7,
        );
        let st = h.run().unwrap();
        assert_eq!(*h.timeouts.borrow(), vec![0, 0, 500, 0, 0, 1, 2]);
        assert_eq!(st.zero_timeout_clamps, 2, "非 0 拍把连续计数清零 ⇒ 只钳制后两拍");
        assert_eq!(st.zero_timeout_iters, 6);
        assert_eq!(st.max_zero_clamp_streak, 2, "偶发 0 抖动后的最长连续钳制");
    }

    /// 钳制结果仍受本拍上界约束（`poll_cap_ms` 更小时不得被阶梯顶出去）。
    #[test]
    fn zero_clamp_never_exceeds_configured_cap() {
        let log: CallLog = Rc::new(RefCell::new(Vec::new()));
        let timeouts = Rc::new(RefCell::new(Vec::new()));
        let fallbacks = Rc::new(RefCell::new(Vec::new()));
        let outcomes = Rc::new(RefCell::new(VecDeque::new()));
        let mut poller = FakePoller {
            outcomes,
            timeouts: Rc::clone(&timeouts),
            fallbacks,
            log,
        };
        let mut ticker = FakeTicker {
            script: Rc::new(RefCell::new(vec![0; 16])),
            log: Rc::new(RefCell::new(Vec::new())),
        };
        let mut host = FakeHost {
            deadline: None,
            log: Rc::new(RefCell::new(Vec::new())),
        };
        let stop = FakeStop {
            seen: Cell::new(0),
            stop_after: 6,
        };
        let st = run(
            &FakeClock::new(),
            &mut poller,
            &mut ticker,
            &mut host,
            &stop,
            &LoopConfig::new(4),
        )
        .unwrap();
        assert_eq!(st.last_timeout_ms, 4);
        assert!(
            timeouts.borrow().iter().all(|t| *t <= 4),
            "钳制不得越过本拍上界：{:?}",
            timeouts.borrow()
        );
    }

    // ---- ④-d 生产 Poller 的纯逻辑（评审 C-① 整改；注入式，不真 poll） ----

    /// 超时换算：`u64` 入参 ⇒ 永不产出负值（负值 = 无限阻塞，会吃掉停机语义）。
    #[test]
    fn timeout_ms_to_c_int_never_negative_and_clamped() {
        assert_eq!(timeout_ms_to_c_int(0), 0);
        assert_eq!(timeout_ms_to_c_int(500), 500);
        assert_eq!(timeout_ms_to_c_int(u32::MAX as u64), i32::MAX);
        assert_eq!(timeout_ms_to_c_int(u64::MAX), i32::MAX);
        // 全量域断言：任何取值都不得为负
        for t in [0u64, 1, 499, 500, 1 << 20, u64::MAX - 1] {
            assert!(timeout_ms_to_c_int(t) >= 0, "t={t}");
        }
    }

    /// 无 fd 分支：必须落到**阻塞式** `Sleep`（`poll(NULL,0,t)`），而不是「立即返回」。
    #[test]
    fn poll_wait_target_has_sleep_branch_without_fd() {
        assert_eq!(poll_wait_target(Some(7)), PollWait::Fd(7));
        assert_eq!(poll_wait_target(Some(0)), PollWait::Fd(0));
        assert_eq!(
            poll_wait_target(None),
            PollWait::Sleep,
            "无触摸设备也要有阻塞点（不变量 1 的生产落点）"
        );
    }

    /// 剩余时间：`deadline − now`，已过 ⇒ 0（不下溢）。
    #[test]
    fn remaining_time_never_underflows() {
        assert_eq!(remaining_ms(1_000, 400), 600);
        assert_eq!(remaining_ms(1_000, 1_000), 0);
        assert_eq!(remaining_ms(1_000, 5_000), 0, "已过 ⇒ 0（不得绕回成巨值）");
        assert_eq!(remaining_ms(0, 0), 0);
    }

    /// `EINTR` 重试 / 其它 errno 上抛 / 正常路径直返（**绝对截止**语义）。
    #[test]
    fn wait_with_retry_retries_eintr_and_propagates_other_errno() {
        // EINTR(4) 两拍后 Ready ⇒ 重试至成功（时钟不推进 ⇒ 剩余时间充足）
        let clock = FakeClock::new();
        let script = [
            RawPollResult::Failed(4),
            RawPollResult::Failed(4),
            RawPollResult::Ready,
        ];
        let mut calls = 0usize;
        let out = wait_with_retry(
            1_000,
            &clock,
            |remain| {
                let i = calls.min(script.len() - 1);
                calls += 1;
                assert_eq!(remain, 1_000, "时钟未推进 ⇒ 剩余时间不变");
                script[i]
            },
            4,
        )
        .unwrap();
        assert_eq!(out, PollOutcome::Ready);
        assert_eq!(calls, 3, "两次 EINTR 被重试");

        // 非 EINTR ⇒ 立即 Err(errno)，**不静默当超时、不重试**
        let mut n = 0usize;
        let e = wait_with_retry(
            1_000,
            &clock,
            |_| {
                n += 1;
                RawPollResult::Failed(9)
            },
            4,
        )
        .unwrap_err();
        assert_eq!(e, 9);
        assert_eq!(n, 1, "非 EINTR 不重试");

        // Timeout / Ready 直接返回（不消耗额外调用）
        let mut m = 0usize;
        assert_eq!(
            wait_with_retry(
                1_000,
                &clock,
                |_| {
                    m += 1;
                    RawPollResult::Timeout
                },
                4
            )
            .unwrap(),
            PollOutcome::Timeout
        );
        assert_eq!(m, 1);
        let mut k = 0usize;
        assert_eq!(
            wait_with_retry(
                1_000,
                &clock,
                |_| {
                    k += 1;
                    RawPollResult::Ready
                },
                4
            )
            .unwrap(),
            PollOutcome::Ready
        );
        assert_eq!(k, 1);
        // EINTR 再 Timeout：重试后按超时返回（信号打断不等于有事件）
        let mut j = 0usize;
        assert_eq!(
            wait_with_retry(
                1_000,
                &clock,
                |_| {
                    j += 1;
                    if j == 1 {
                        RawPollResult::Failed(4)
                    } else {
                        RawPollResult::Timeout
                    }
                },
                4
            )
            .unwrap(),
            PollOutcome::Timeout
        );
        assert_eq!(j, 2);
    }

    /// **Important 3 回归**：信号风暴（每次 `poll` 都被 `EINTR` 打断 + 200 ms 周期看门狗）
    /// 下，总等待受**原 timeout** 约束 —— 旧实现（每次重置完整 timeout）会无限重试。
    #[test]
    fn eintr_storm_is_bounded_by_absolute_deadline() {
        let clock = FakeClock::new();
        let mut calls = 0usize;
        let mut last_remain = u64::MAX;
        let out = wait_with_retry(
            500,
            &clock,
            |remain| {
                calls += 1;
                last_remain = remain;
                // 假 poller 上限：防「修复前」用例挂死（旧实现会无限重试）——
                // 超过上限即当作超时返回，让断言去判失败。
                if calls > 8 {
                    return RawPollResult::Timeout;
                }
                // 模拟「未设 SA_RESTART 的 200 ms 看门狗」：poll 被打断且时间前进 200 ms。
                clock.0.set(clock.0.get() + 200);
                RawPollResult::Failed(4)
            },
            4,
        )
        .unwrap();
        assert_eq!(out, PollOutcome::Timeout, "剩余耗尽 ⇒ 按超时返回（POSIX 语义）");
        assert!(
            calls <= 4,
            "重试次数受原 timeout 约束（实得 {calls} 次；无限重试即本条要根治的 bug）"
        );
        assert!(
            last_remain <= 500,
            "剩余时间不得超过原 timeout（实得 {last_remain}）"
        );
        assert!(
            clock.0.get() <= 800,
            "总等待时间受原 timeout 约束（信号周期 200ms；实得 {} ms）",
            clock.0.get()
        );
    }

    /// **Important 3（循环级）**：假 poller 内部走 [`wait_with_retry`] + 信号风暴 ⇒
    /// `poll` 仍会在原 timeout 内返回，因此 `iterations` 照常推进、**stop 每拍都被检查**。
    #[test]
    fn eintr_storm_still_lets_loop_check_stop() {
        /// 每次 `poll` 都被 EINTR 打断并推进注入时钟 200 ms 的假 poller。
        struct EintrStormPoller {
            clock: FakeClock,
            retries: Rc<Cell<u32>>,
        }
        impl Poller for EintrStormPoller {
            fn poll(&mut self, timeout_ms: u64) -> PollOutcome {
                let deadline = self.clock.now_ms().saturating_add(timeout_ms);
                let clock = self.clock.clone();
                let retries = Rc::clone(&self.retries);
                wait_with_retry(
                    deadline,
                    &self.clock,
                    |_| {
                        let n = retries.get();
                        retries.set(n + 1);
                        if n > 8 {
                            return RawPollResult::Timeout; // 防挂死（旧实现会无限重试）
                        }
                        clock.set(clock.now_ms() + 200);
                        RawPollResult::Failed(4)
                    },
                    4,
                )
                .unwrap()
            }
            fn poll_fallback(&mut self, _timeout_ms: u64) {}
        }

        let clock = FakeClock::new();
        let retries = Rc::new(Cell::new(0));
        let mut poller = EintrStormPoller {
            clock: clock.clone(),
            retries: Rc::clone(&retries),
        };
        let log: CallLog = Rc::new(RefCell::new(Vec::new()));
        let mut ticker = FakeTicker {
            script: Rc::new(RefCell::new(vec![500; 16])),
            log: Rc::clone(&log),
        };
        let mut host = FakeHost {
            deadline: None,
            log: Rc::clone(&log),
        };
        let stop = FakeStop {
            seen: Cell::new(0),
            stop_after: 3,
        };
        let st = run(
            &clock,
            &mut poller,
            &mut ticker,
            &mut host,
            &stop,
            &LoopConfig::default(),
        )
        .unwrap();
        assert_eq!(st.iterations, 3, "stop 每拍都被检查到（旧实现会卡死在 poll 内）");
        assert!(retries.get() <= 12, "重试次数有界（实得 {}）", retries.get());
    }

    // ---- ④-e `poll` 失败：不紧循环 / 有界终止（Critical 2 整改） ----

    #[test]
    fn poll_failure_constants_are_sane() {
        // 常量关系已由模块级编译期断言钉住（见 `const _: () = assert!(...)`）；
        // 此处只固定**对外契约的绝对值**（改小会让「兜底宽限期」短到没有意义）。
        assert_eq!(POLL_FAIL_FALLBACK_AFTER, 3);
        assert_eq!(POLL_FAIL_ABORT_AFTER, 100);
    }

    /// **Critical 2 回归**：`poll` 持续失败（`EBADF` 一类）时
    /// ① 不紧循环（真正调用 `poll` 的次数被阈值钳住，其余走**兜底阻塞**）、
    /// ② 最终**有界终止**并返回错误（不是每拍立即失败 → 立即重试的死循环）。
    #[test]
    fn persistent_poll_failure_does_not_spin_and_aborts() {
        // stop 永不置位 ⇒ 只能靠 abort 结束（另设 150 拍保险丝：若被误判为"正常退出"，
        // `unwrap_err` 会失败而不是挂死）。
        let mut h = harness_with_outcomes(
            vec![500; 400],
            None,
            150,
            vec![PollOutcome::Failed(9); 400],
        );
        let err = h.run().unwrap_err();
        assert_eq!(err.errno, 9);
        assert_eq!(err.consecutive, POLL_FAIL_ABORT_AFTER as u64);
        assert_eq!(err.iterations, POLL_FAIL_ABORT_AFTER as u64);

        // ① 不紧循环：`poll` 只被调用 POLL_FAIL_FALLBACK_AFTER 次，之后全部改为兜底阻塞。
        assert_eq!(
            h.timeouts.borrow().len(),
            POLL_FAIL_FALLBACK_AFTER as usize,
            "持续失败时不得每拍都调 poll（那正是紧循环）"
        );
        assert_eq!(
            h.fallbacks.borrow().len(),
            (POLL_FAIL_ABORT_AFTER - POLL_FAIL_FALLBACK_AFTER) as usize,
            "其余拍由兜底阻塞承担（仍真阻塞 ⇒ 不忙等）"
        );
        // 兜底阻塞拿到的仍是**正**超时（不是 poll(0) 自旋）。
        assert!(
            h.fallbacks.borrow().iter().all(|t| *t > 0),
            "兜底阻塞必须拿到正超时：{:?}",
            h.fallbacks.borrow()
        );
        // 调用次序：前 3 拍 poll，之后全 poll_fallback。
        let log = h.log.borrow();
        let polls = log.iter().filter(|e| **e == "poll").count();
        let fallbacks = log.iter().filter(|e| **e == "poll_fallback").count();
        assert_eq!(polls, POLL_FAIL_FALLBACK_AFTER as usize);
        assert_eq!(fallbacks, (POLL_FAIL_ABORT_AFTER - POLL_FAIL_FALLBACK_AFTER) as usize);
        assert_eq!(
            h.outcomes.borrow().len(),
            400 - POLL_FAIL_FALLBACK_AFTER as usize,
            "兜底拍不触达 poll（不消费脚本）⇒ 恰好消费 3 次"
        );
    }

    /// 失败后恢复：连续失败被**成功**打断即归零，不触达兜底、不终止。
    #[test]
    fn poll_failure_streak_resets_on_success() {
        let mut h = harness_with_outcomes(
            vec![500; 6],
            None,
            5,
            vec![
                PollOutcome::Failed(9),
                PollOutcome::Failed(9),
                PollOutcome::Ready, // 恢复
                PollOutcome::Failed(9),
                PollOutcome::Timeout, // 恢复
            ],
        );
        let st = h.run().unwrap();
        assert_eq!(st.iterations, 5);
        assert_eq!(st.poll_failures, 3, "总失败次数照实统计");
        assert_eq!(st.consecutive_poll_failures, 0, "退出时连续失败已归零");
        assert_eq!(st.poll_fallback_iters, 0);
        assert!(h.fallbacks.borrow().is_empty(), "未达阈值 ⇒ 不触达兜底");
        assert_eq!(st.ready_events, 1);
    }

    /// 兜底一旦进入**不再探测 `poll`**（刻意无恢复探针）：语义是「poll 已不可用 ⇒ 有界终止」；
    /// 需要恢复能力的调用方应在收到 [`PollFailure`] 后重建 poller 并重启循环。
    #[test]
    fn fallback_is_sticky_and_never_returns_to_poll() {
        let mut h = harness_with_outcomes(vec![500; 12], None, 6, vec![PollOutcome::Failed(7); 12]);
        let st = h.run().unwrap();
        assert_eq!(st.ready_events, 0);
        assert_eq!(st.poll_failures, POLL_FAIL_FALLBACK_AFTER as u64);
        assert_eq!(st.poll_fallback_iters, 3, "第 4~6 拍走兜底");
        assert_eq!(h.timeouts.borrow().len(), POLL_FAIL_FALLBACK_AFTER as usize);
        assert_eq!(h.fallbacks.borrow().len(), 3);
        assert_eq!(st.consecutive_poll_failures, 6, "退出时连续失败 = 6");
        assert_eq!(st.iterations, 6);
    }
}
