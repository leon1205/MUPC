//! LVGL v9 薄安全层（12-MUPC 本地显示终端，设计 §1.1.1.2 / §5.2 工作单元 A1）。
//!
//! # 位置与纪律（设计 §1.1.1.2「unsafe 边界纪律」，评审逐条检查）
//!
//! 1. `unsafe` **只允许**出现在 `lvgl-sys` 生成物与**本目录**内；`lvgl-sys` 不得被
//!    `pages` / `state` / `channel` / `console` 等模块直接引用 —— 唯一合法用点就是这里。
//! 2. **不提供任何跨线程 API**：LVGL 非线程安全（`lv_conf.h` 的 `LV_USE_OS = LV_OS_NONE`），
//!    本层所有类型都含裸指针（故自动 `!Send` / `!Sync`），编译期即挡住跨线程使用；
//!    所有 `lv_*` 调用必须发生在事件循环线程内（设计 §5.2 不变量 4）。
//! 3. 回调内**绝不 panic**（跨 FFI 展开为 UB）：[`event`] 的桥统一 `catch_unwind` 收敛。
//! 4. [`display`] 的 flush 桥**只做像素搬运**，不得阻塞、不得做通道 I/O
//!    （设计 §1.1.1.1 P-1 / §5.2 不变量 3）。
//! 5. `user_data` 生命周期由 [`event`] **统一管理**（`Box::into_raw` / `from_raw` 配对，
//!    宿主被 LVGL 删除时 drop）：不泄漏、不 double free。回调执行期间到达的回收请求
//!    走 `event.rs` 的**延迟回收**（见该模块文档）。
//! 6. 句柄与底层对象的对应关系用**世代令牌**（[`generation`]）刻画：`init → deinit →
//!    init` 之后，上一世代残留的 [`display::Display`] / [`indev::Indev`] 句柄的底层对象
//!    已被 `lv_deinit()` 释放，句柄据世代失配而**拒绝**再操作（防 double free / 悬垂调用）。
//!
//! # 本轮范围（A1「核心桥」）
//!
//! `mod.rs` / [`event`] / [`display`] / [`indev`] 四个模块。
//! `obj.rs` / `style.rs` / `font.rs` / `widgets.rs` 留给 A2/A3（届时 `unsafe` 仍在同一目录内）。

pub mod display;
pub mod event;
pub mod indev;

#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use lvgl_sys as sys;

/// 薄层错误类型（`std::error::Error` 实现见下）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LvglError {
    /// 未调用 [`init`] 就使用了薄层 API。
    NotInitialized,
    /// `lv_*_create` 返回 NULL —— 多为 `LV_MEM_SIZE`（`lv_conf.h` 256 KB 起）耗尽。
    OutOfMemory(&'static str),
    /// 入参不合法（如 0×0 的屏）。
    InvalidArgument(&'static str),
}

impl std::fmt::Display for LvglError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LvglError::NotInitialized => write!(f, "LVGL 薄层未初始化：请先调用 lvgl::init()"),
            LvglError::OutOfMemory(what) => {
                write!(f, "LVGL 内存分配失败（LV_MEM_SIZE 不足？）：{what}")
            }
            LvglError::InvalidArgument(what) => write!(f, "LVGL 薄层入参不合法：{what}"),
        }
    }
}

impl std::error::Error for LvglError {}

/// 进程内 LVGL 是否已初始化。
///
/// 单线程访问（薄层不提供跨线程 API）；同时用作 [`display::Display`] /
/// [`indev::Indev`] 的 `Drop` 守卫，避免 [`deinit`] 之后再删一次（double free）。
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// 初始化**世代令牌**：每次成功的 [`init`] 递增一次。
///
/// 用途：`init → deinit → init` 之后，`INITIALIZED` 又变回 `true`，但**上一世代**
/// 残留的 [`display::Display`] / [`indev::Indev`] 句柄其底层对象已被 `lv_deinit()`
/// 释放。若 `Drop` / 方法只看 `INITIALIZED`，就会对这些已释放对象二次操作
/// （`lv_display_delete` 等 = double free）。句柄记录创建时的世代，只有
/// `世代一致 && is_initialized()` 才认为底层对象仍存活。
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// 当前世代号（[`display::Display`] / [`indev::Indev`] 内部记录并比对）。
pub(crate) fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

/// 单调时基原点（`lv_tick_set_cb` 契约：返回自时基原点起的毫秒数）。
static TICK0: OnceLock<Instant> = OnceLock::new();

/// LVGL 当前 tick（毫秒）。用 [`Instant`] 提供**单调**时基，不受系统时钟跳变影响
/// （设计 §1.1.1.1「主循环与 tick 契约」）。
pub fn tick_ms() -> u32 {
    let t0 = TICK0.get_or_init(Instant::now);
    // 32 位毫秒约 49.7 天回绕 —— 与 LVGL tick 语义一致（无符号差值比较）。
    t0.elapsed().as_millis() as u32
}

/// `lv_tick_set_cb` 的 C 侧回调（无 panic 可能：纯算术）。
unsafe extern "C" fn tick_cb() -> u32 {
    tick_ms()
}

/// 初始化 LVGL 并挂上 Rust 单调 tick。**幂等**。
///
/// 必须在事件循环线程调用，且早于本层任何其他 API（设计 §5.2 不变量 4）。
pub fn init() -> Result<(), LvglError> {
    if INITIALIZED.load(Ordering::SeqCst) {
        return Ok(());
    }
    let _ = TICK0.set(Instant::now());
    // SAFETY: `lv_init()` 只允许被调用一次（LVGL 自带 `lv_initialized` 守卫），
    // 且此处在事件循环线程内、早于任何 display/indev 创建。
    unsafe {
        sys::lv_init();
        sys::lv_tick_set_cb(Some(tick_cb));
    }
    // 先推进世代再置位：此后创建的句柄都归属新世代；上一世代残留句柄就此失效。
    GENERATION.fetch_add(1, Ordering::SeqCst);
    INITIALIZED.store(true, Ordering::SeqCst);
    Ok(())
}

/// LVGL 是否已初始化（[`init`] 之后、[`deinit`] 之前为 `true`）。
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::SeqCst)
}

/// 反初始化（进程退出前调用；幂等）。
///
/// LVGL 会在此删除**全部** display / indev，因而触发我们注册的 `LV_EVENT_DELETE`
/// 回调并回收 `user_data`（设计 §1.1.1.2 纪律 3）。因此**必须**在它之前先落旗标：
/// 之后 [`display::Display`] / [`indev::Indev`] 的 `Drop` 不得再删一次。
pub fn deinit() {
    if !INITIALIZED.swap(false, Ordering::SeqCst) {
        return;
    }
    // SAFETY: 已确认处于已初始化状态；调用线程 = 事件循环线程。
    unsafe { sys::lv_deinit() };
}

/// 驱动 LVGL 的定时器 / 动画 / 脏区重绘一次，返回**距下次需要处理的毫秒数**
/// —— 直接作为事件循环 `poll()` 的超时上界（设计 §1.1.1.1 / §5.2 不变量 1）。
///
/// ⚠️ 渲染只在这里（以及 LVGL 内部）发生；**不得**在生产路径调用 `lv_refr_now()`
/// （设计 §5.2 不变量 2：那是测试专用强制渲染）。
pub fn timer_handler() -> u32 {
    // SAFETY: 调用线程 = 事件循环线程；LVGL 未初始化时其内部自行处理（返回 0）。
    unsafe { sys::lv_timer_handler() }
}
