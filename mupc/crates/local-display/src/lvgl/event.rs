//! C 回调 → Rust 闭包的桥（工作单元 A1；设计 §1.1.1.2 纪律 3 / §5.2 不变量 5）。
//!
//! # 为什么要一层桥
//!
//! LVGL 的事件回调是 C 函数指针 `void (*)(lv_event_t *)`，而页面逻辑是 Rust 闭包。
//! 把闭包带过 FFI 需要一份 **`user_data` 所有权契约**：
//!
//! - 挂载：`Box::into_raw(Box::new(Ctx { f }))` → 指针交给 LVGL 事件项；
//! - 回收：宿主被 LVGL 删除（`LV_EVENT_DELETE`）→ `Box::from_raw` 回收到 Rust 并 drop。
//!
//! 本模块是该契约的**唯一实现点**（设计 §1.1.1.2 纪律 3：「`user_data` 生命周期由
//! `event.rs` 统一管理」），因此不泄漏、不 double free。
//!
//! # 延迟回收（**本模块最关键的不变量**）
//!
//! LVGL 明确支持「**在自己的事件回调里删除自己**」：`obj_delete_core()`
//! （`vendor/lvgl/src/core/lv_obj_tree.c`）的 `is_deleting` 只挡"DELETE 内再删同对象"，
//! 挡不住"**非 DELETE 回调内删除宿主**"；此时 LVGL 会**立即**在当前回调栈帧之内派发
//! `LV_EVENT_DELETE`。若此处当场 `Box::from_raw`，就会把**正在执行的那个闭包连同其
//! 捕获状态**一并释放 —— 回调随后继续使用已释放内存（UAF）。A2/A3 的 `Obj::delete()`
//! 一旦落地，这就是**常态路径**。
//!
//! 因此：**回调执行期间到达的回收请求（DELETE / [`CallbackHandle::detach`]）只登记到
//! 待回收表，待最外层回调退出（回调嵌套深度归零）再统一 `from_raw`**。要点：
//!
//! - **覆盖重入**：用 `depth` 计数而非 bool，一次回调内触发的多层嵌套事件都能正确收敛；
//! - **恰好一次**：每个指针只在"深度归零"那一次被移出待回收表 ⇒ 不漏放、不 double free；
//! - **同闭包重入**：记录"执行中"的 `Ctx` 指针栈；若 DELETE 正是由该 `Ctx` 自己触发
//!   （记录命中），则跳过对它的再次派发 —— 否则会对同一个 `Ctx::f` 形成重入的可变
//!   别名（UB）。本条同时兜住 `filter = ALL` 的病态自删场景。
//!
//! # 两条硬约束
//!
//! 1. **回调内绝不 panic**：跨 FFI 展开（unwinding into C）是 UB。C 侧蹦床把所有执行体
//!    包进 [`catch_unwind`]，panic 只被记录、不传播。
//! 2. **不提供跨线程 API**：LVGL 非线程安全（`LV_USE_OS = LV_OS_NONE`），全部调用必须在
//!    事件循环线程内（设计 §5.2 不变量 4）。回收状态用 `thread_local` 表达这一前提。

use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};

use lvgl_sys as sys;

/// 事件码（薄层对 C 侧 `lv_event_code_t` 的无损镜像）。
///
/// 用 newtype 而非 `enum`：LVGL 的事件码是开放集合，未知码也要能原样带回 Rust 侧
/// 比较 / 打印，不应被强行折叠。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventCode(i32);

impl EventCode {
    /// 全部事件（`LV_EVENT_ALL`）—— 用作过滤器即"不过滤"。
    pub const ALL: Self = Self(sys::LV_EVENT_ALL);
    /// 按下。
    pub const PRESSED: Self = Self(sys::LV_EVENT_PRESSED);
    /// 抬起（无论是否转化为点击）。
    pub const RELEASED: Self = Self(sys::LV_EVENT_RELEASED);
    /// 按下后滑出控件。
    pub const PRESS_LOST: Self = Self(sys::LV_EVENT_PRESS_LOST);
    /// 点击（未转化为滚动的按下+抬起）。
    pub const CLICKED: Self = Self(sys::LV_EVENT_CLICKED);
    /// 长按达到 `long_press_time`（设计 §5.6：确认按钮 1.0 s）。
    pub const LONG_PRESSED: Self = Self(sys::LV_EVENT_LONG_PRESSED);
    /// 长按重复。
    pub const LONG_PRESSED_REPEAT: Self = Self(sys::LV_EVENT_LONG_PRESSED_REPEAT);
    /// 控件值变化（下拉/开关/滑块…）。
    pub const VALUE_CHANGED: Self = Self(sys::LV_EVENT_VALUE_CHANGED);
    /// 弹层 / 流程确认。
    pub const READY: Self = Self(sys::LV_EVENT_READY);
    /// 弹层 / 流程取消。
    pub const CANCEL: Self = Self(sys::LV_EVENT_CANCEL);
    /// 宿主即将被删除 —— **本桥的 `user_data` 回收点**。
    pub const DELETE: Self = Self(sys::LV_EVENT_DELETE);

    /// 原始 C 事件码。
    pub const fn raw(self) -> i32 {
        self.0
    }
}

impl std::fmt::Display for EventCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lv_event_code({})", self.0)
    }
}

/// 派发给 Rust 闭包的事件（值类型，避开 FFI 生命周期）。
///
/// 目前只携带事件码；后续若需要"哪个对象 / `param`"，在此扩展即可，闭包签名不变。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    code: EventCode,
}

impl Event {
    /// 本次派发的事件码。
    pub const fn code(self) -> EventCode {
        self.code
    }
}

/// 挂到 LVGL 事件项上的 `user_data`：唯一所有者是 `Box<Ctx>`。
struct Ctx {
    /// 调用方声明的过滤器（原始码；`LV_EVENT_ALL` = 不过滤）。
    filter: i32,
    /// 闭包。`None` 仅出现于"已 `take` 出来准备 drop"的瞬间。
    f: Option<Box<dyn FnMut(Event)>>,
}

/// 延迟回收状态（LVGL 单线程 ⇒ `thread_local` 即"全局"）。
#[derive(Default)]
struct CbState {
    /// 当前回调嵌套深度（`0` = 不在任何回调执行体内）。
    depth: u32,
    /// 正在执行中的 `Ctx` 指针栈（识别"回调内删除宿主"的同闭包重入）。
    active: Vec<*mut Ctx>,
    /// 待回收 `Ctx`：回调执行期间到达的 DELETE / detach 落在此，外层回调退出后统一释放。
    pending: Vec<*mut Ctx>,
}

thread_local! {
    static CB_STATE: RefCell<CbState> = RefCell::new(CbState::default());
}

/// 进入一次 C 蹦床：`depth += 1`；退出（drop）时 `depth -= 1`，一旦归零即
/// **统一释放待回收表** —— 这正是"延迟回收"的落地时机。
struct ReentryGuard;

impl ReentryGuard {
    fn enter() -> Self {
        CB_STATE.with(|s| s.borrow_mut().depth += 1);
        ReentryGuard
    }
}

impl Drop for ReentryGuard {
    fn drop(&mut self) {
        let drained: Vec<*mut Ctx> = CB_STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.depth -= 1;
            if st.depth == 0 {
                std::mem::take(&mut st.pending)
            } else {
                Vec::new()
            }
        });
        for p in drained {
            // SAFETY: `p` 来自 `on()` 的 `Box::into_raw`；每个指针仅在"深度归零"这一次
            // 被移出 `pending`（`mem::take` 保证不重复取出）⇒ 恰好释放一次。
            drop(unsafe { Box::from_raw(p) });
        }
    }
}

/// 回收一个 `Ctx`：回调执行期间（`depth > 0`）**延迟**，否则立即 `from_raw`。
///
/// # Safety
///
/// `p` 必须来自 [`on`] 的 `Box::into_raw`，且此前未被回收。
unsafe fn reclaim(p: *mut Ctx) {
    let deferred = CB_STATE.with(|s| {
        let mut st = s.borrow_mut();
        if st.depth > 0 {
            st.pending.push(p);
            true
        } else {
            false
        }
    });
    if !deferred {
        // SAFETY: 由调用方保证 `p` 有效且尚未回收；`depth == 0` ⇒ 无回调正在使用它。
        drop(unsafe { Box::from_raw(p) });
    }
}

/// C 侧事件蹦床：**所有**挂载都指向它（`filter` 一律注册 `LV_EVENT_ALL`）。
///
/// 为什么一律注册 `LV_EVENT_ALL`：`LV_EVENT_DELETE` 必须能到达本桥，否则 `user_data`
/// 没有回收点（泄漏）。用户的过滤器改在 Rust 侧比较（代价是一次整数比较）。
unsafe extern "C" fn trampoline(e: *mut sys::lv_event_t) {
    // 进入即加深嵌套；无论走哪条分支返回，退出时都会尝试"深度归零 ⇒ 统一回收"。
    let _guard = ReentryGuard::enter();

    // SAFETY: `e` 由 LVGL 保证非空且指向有效事件对象；`user_data` 只由本模块写入。
    let p = unsafe { sys::lv_event_get_user_data(e) } as *mut Ctx;
    if p.is_null() {
        return;
    }
    let raw = unsafe { sys::lv_event_get_code(e) };

    // 本 Ctx 是否正处于"执行中"（即本次事件是由它自己的回调触发）。命中时**绝不**
    // 再碰 `(*p).f` —— 否则对正在执行的可变借用形成重入别名（UB）。
    let is_executing = CB_STATE.with(|s| s.borrow().active.contains(&p));

    if raw == sys::LV_EVENT_DELETE {
        // ── 唯一回收点：宿主被 LVGL 删除 ──────────────────────────────
        // 两件事在此发生，**互不耦合**：
        //   (a) 回收：无条件执行（否则 `user_data` 泄漏），但**回调执行期间只登记**，
        //       真正的 `from_raw` 推迟到最外层回调退出 —— 这样"回调内删除宿主"不会
        //       把正在执行的闭包连同其捕获状态释放（UAF）；
        //   (b) 派发：是否把 DELETE 送给闭包，**照常受 `ctx.filter` 约束**（注册了
        //       `DELETE` 或 `ALL` 才收到），与其它事件码语义一致。同闭包重入时跳过。
        if !is_executing {
            // SAFETY: `p` 有效（回收只发生在本分支，且此刻它不是执行中的那个 Ctx，
            // 故不存在与回调栈帧并存的可变别名）。
            let ctx = unsafe { &mut *p };
            if ctx.filter == sys::LV_EVENT_ALL || ctx.filter == raw {
                if let Some(mut f) = ctx.f.take() {
                    // 同非 DELETE 分支：回调内绝不 panic。
                    let _ = catch_unwind(AssertUnwindSafe(|| {
                        f(Event {
                            code: EventCode(raw),
                        })
                    }));
                }
            }
        }
        // SAFETY: `p` 由 `on()` 的 `Box::into_raw` 产生；每个事件项只在删除时派发一次
        // DELETE，且 `reclaim` 保证恰好回收一次（立即或延迟）。
        unsafe { reclaim(p) };
        return;
    }

    if is_executing {
        // 同一闭包被重入（如 `filter = ALL` 且闭包内又向宿主重发事件）：直接返回，
        // 避免对正在执行的 `Ctx::f` 形成可变别名（UB），也避免无限递归。
        return;
    }

    // SAFETY: 非 DELETE 分支下 `p` 仍有效（回收只发生在 DELETE / detach，且此 Ctx
    // 不在执行中）。
    let ctx = unsafe { &mut *p };
    if ctx.filter != sys::LV_EVENT_ALL && ctx.filter != raw {
        return;
    }
    let Some(f) = ctx.f.as_mut() else {
        return;
    };

    // 登记"执行中"，供嵌套到达的 DELETE 识别同闭包重入。
    CB_STATE.with(|s| s.borrow_mut().active.push(p));
    // 纪律：回调内绝不 panic（跨 FFI 展开 = UB）—— 统一在此拦截并记录。
    let r = catch_unwind(AssertUnwindSafe(|| {
        f(Event {
            code: EventCode(raw),
        })
    }));
    CB_STATE.with(|s| {
        let mut st = s.borrow_mut();
        if let Some(i) = st.active.iter().rposition(|q| *q == p) {
            st.active.remove(i);
        }
    });
    if r.is_err() {
        eprintln!("[lvgl] 事件回调内 panic 已被拦截（code={raw}）；该次事件作废，事件循环继续");
    }
}

mod sealed {
    /// 密封：只有本模块可以为 LVGL 宿主类型实现 [`super::EventHost`]。
    pub trait Sealed {}
}

/// 可挂 LVGL 事件回调的宿主裸指针（`lv_obj` / `lv_display` / `lv_indev`）。
///
/// 密封 trait：A2/A3 的 `Obj` / `Widget` 包装只需把自己的 `raw()` 指针传给 [`on`]，
/// **不需要**（也不能）自行实现本 trait。
pub trait EventHost: sealed::Sealed + Copy {
    /// 注册 C 事件回调（内部用）。
    #[doc(hidden)]
    unsafe fn add_event_cb(self, filter: EventCode, user_data: *mut c_void);
    /// 按 `user_data` 摘除回调，返回摘除数量（内部用）。
    #[doc(hidden)]
    unsafe fn remove_event_cb(self, user_data: *mut c_void) -> u32;
}

impl sealed::Sealed for *mut sys::lv_obj_t {}
impl EventHost for *mut sys::lv_obj_t {
    unsafe fn add_event_cb(self, filter: EventCode, user_data: *mut c_void) {
        sys::lv_obj_add_event_cb(self, Some(trampoline), filter.raw(), user_data);
    }
    unsafe fn remove_event_cb(self, user_data: *mut c_void) -> u32 {
        sys::lv_obj_remove_event_cb_with_user_data(self, Some(trampoline), user_data)
    }
}

impl sealed::Sealed for *mut sys::lv_display_t {}
impl EventHost for *mut sys::lv_display_t {
    unsafe fn add_event_cb(self, filter: EventCode, user_data: *mut c_void) {
        sys::lv_display_add_event_cb(self, Some(trampoline), filter.raw(), user_data);
    }
    unsafe fn remove_event_cb(self, user_data: *mut c_void) -> u32 {
        sys::lv_display_remove_event_cb_with_user_data(self, Some(trampoline), user_data)
    }
}

impl sealed::Sealed for *mut sys::lv_indev_t {}
impl EventHost for *mut sys::lv_indev_t {
    unsafe fn add_event_cb(self, filter: EventCode, user_data: *mut c_void) {
        sys::lv_indev_add_event_cb(self, Some(trampoline), filter.raw(), user_data);
    }
    unsafe fn remove_event_cb(self, user_data: *mut c_void) -> u32 {
        sys::lv_indev_remove_event_cb_with_user_data(self, Some(trampoline), user_data)
    }
}

/// 把 Rust 闭包挂到宿主的指定事件上，返回可在宿主存活期间主动摘除的句柄。
///
/// - `filter`：关心的事件码；[`EventCode::ALL`] 表示全部。
/// - `user_data` 生命周期：见模块文档 —— 宿主被 LVGL 删除时自动回收并 drop；
///   若删除发生在**某个回调执行期间**，回归收延迟到该外层回调退出（防 UAF）。
/// - **单线程**：宿主与闭包都不得跨线程使用（设计 §5.2 不变量 4）。
///
/// 调用方通常可以忽略返回值（句柄无 `Drop`，丢弃即"挂上就不管"）；
/// 需要在宿主存活期间显式摘除时才保存它并调用 [`CallbackHandle::detach`]。
///
/// # Safety
///
/// `host` 必须是**指向该类 LVGL 活对象**的有效非空裸指针，且在其挂载的生命周期内
/// 有效（即：真正被 LVGL 删除之前，不得用于任何调用）。本函数是裸指针 FFI 边界，
/// 无法在类型系统层面校验 —— 调用方（薄层的 `Display` / `Indev`，以及 A2/A3 的
/// `Obj` / `Widget` 包装，它们持有创建时的有效指针）负责满足该前置条件。
/// `std::ptr::null_mut()` 或悬垂指针在本函数中即为 UB。
pub unsafe fn on<H, F>(host: H, filter: EventCode, f: F) -> CallbackHandle<H>
where
    H: EventHost,
    F: FnMut(Event) + 'static,
{
    let ctx = Box::new(Ctx {
        filter: filter.raw(),
        f: Some(Box::new(f)),
    });
    let user_data = Box::into_raw(ctx) as *mut c_void;
    // SAFETY: 由调用方的 `# Safety` 契约保证 `host` 有效；`user_data` 的所有权自此
    // 交给 LVGL 事件项，由 `trampoline` 在 DELETE 时回收（或延迟回收）。
    unsafe { host.add_event_cb(EventCode::ALL, user_data) };
    CallbackHandle { host, user_data }
}

/// [`on`] 的返回值：持有宿主与 `user_data` 指针，用于在宿主存活期间主动摘除。
///
/// ⚠️ **没有 `Drop`**：句柄本身不拥有 `user_data`（所有权在 LVGL 事件项上），
/// 丢弃句柄不会摘除回调。宿主被 LVGL 删除（`lv_obj_delete` / `lv_display_delete` /
/// `lv_indev_delete`）之后本句柄即失效，**不得**再调用 [`CallbackHandle::detach`]。
pub struct CallbackHandle<H: EventHost> {
    host: H,
    user_data: *mut c_void,
}

impl<H: EventHost> CallbackHandle<H> {
    /// 主动摘除回调并释放闭包（若正在某回调内调用，则释放延迟到该外层回调退出）。
    ///
    /// 回调内部调用本方法是**安全**的：LVGL 的事件派发对"派发期间摘除事件项"有专门
    /// 处理（`lv_event_remove_dsc` → `event_mark_deleting`，遍历中只标记、遍历后清理），
    /// 且本桥的释放走延迟回收，不会在闭包执行中释放该闭包自身。
    ///
    /// # Safety
    ///
    /// `host`（即挂载时传入的宿主）必须**尚未**被 LVGL 删除 —— 宿主已删则其裸指针悬垂，
    /// 与直接用裸指针调 LVGL 同一纪律（`remove_event_cb` 会解引用它）。宿主删除后请改用
    /// `Drop`/DELETE 自动回收路径，不要调用本方法。
    pub unsafe fn detach(self) {
        // 先摘事件项（此后不会再被派发），再回收 user_data。
        // SAFETY: 由调用方的 `# Safety` 契约保证 host 存活。
        unsafe {
            self.host.remove_event_cb(self.user_data);
        }
        // SAFETY: `user_data` 由 `on()` 分配且尚未回收（DELETE 未到达，detach 只调一次）。
        unsafe { reclaim(self.user_data as *mut Ctx) };
    }
}
