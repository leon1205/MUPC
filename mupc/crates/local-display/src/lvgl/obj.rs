//! 基础对象的**安全包装**（工作单元 A2；设计 §5.1「Obj 包装（创建/父子/坐标/可见性/样式引用）」）。
//!
//! # 为什么需要它（把 `unsafe` 关进所有权不变量里）
//!
//! A1 的 [`super::event::on`] / [`super::event::CallbackHandle::detach`] 是 `unsafe fn`：
//! 其 `# Safety` 要求"宿主裸指针有效且未被删除"。这是**类型系统挡住非法裸指针**的措施，
//! 但用起来很难受，且**由调用方手工保证**等于把 unsafe 面的纪律散到全项目。
//!
//! [`Obj`] 正是这条 unsafe 边界的**密封句柄**：它由本层自己创建、内部持有裸指针、`Drop`
//! 时删除底层对象。于是"宿主有效"从**调用方义务**变成 [`Obj`] 的**所有权不变量**：
//!
//! - [`Obj::on`] / [`Obj::on_clicked`] 是**安全** API（内部走 `unsafe`，前置条件由 `&self` 保证）；
//! - [`EventSub::detach`] 也是**安全**的：它自己先判"宿主是否仍存活"，再决定是否摘除。
//!
//! # `Drop` 与回调回收的顺序（**评审关注项**）
//!
//! [`Obj::drop`] **不摘回调**，直接 `lv_obj_delete(raw)`：LVGL 删除对象时会对其**每个事件项**
//! 派发 `LV_EVENT_DELETE`，[`super::event`] 桥在该事件上回收 `user_data`（闭包 Box）——
//! 于是"对象没了 ⇒ 挂在它上面的闭包恰好被 drop 一次"，由桥**统一**保证（A1 已有测试证明）。
//!
//! **为什么不是"先摘回调、再删对象"**：
//!
//! 1. `Obj` 自己**不持有**任何 [`CallbackHandle`]（[`Obj::on`] 把闭包所有权直接交给 LVGL
//!    事件项，句柄只存在于用户拿到的 [`EventSub`] 里）—— 换句话说"要先摘"的那份清单
//!    **根本不存在**，`Obj` 无从摘起；要造这份清单，就得把 A1 的 `user_data` 生命周期契约
//!    复制成第二份真源（必然漂移）；
//! 2. 子树里的对象（以及 LVGL 内部建的子对象）上的回调，只有"删对象"这条路能保证被回收，
//!    摘除清单管不到它们；
//! 3. 回调内删自己时，"摘"与"删"落在同一个时机问题上：桥已把它收敛成**延迟回收**
//!    （见 [`EventSub::detach`] 与 A1 的场景 ⑦/⑧），先删后摘只会把同一件事做两遍。
//!
//! 结论：**先删对象（DELETE 路径统一回收）**，需要提前摘除时由用户对 [`EventSub::detach`]
//! 显式为之 —— 两条路径下闭包都恰好 drop 一次（`tests_a2.rs` 场景 ⑭ 断言）。
//!
//! # 两个删除路径（含"回调内删自己"）
//!
//! - [`Obj::delete`]：显式删除（消费 `self`，`Drop` 不再删第二次）；
//! - `Drop`：作用域结束时删除。
//!
//! **在自己的回调里删宿主是安全且受支持的**（A1 的延迟回收，设计 §1.1.1.2 纪律 3）：
//! `lv_obj_delete` 会在**当前回调栈帧内**立即派发 DELETE，桥把该闭包的释放推迟到最外层
//! 回调退出（否则正在执行的闭包连同捕获状态会被释放 = UAF）。本层不额外禁用这种用法。
//!
//! # 存活判定（为什么不用 `lv_obj_is_valid`）
//!
//! 裸指针失效有三条路径：① 显式删除；② **父对象被删导致级联删除**；③ `lv_deinit()`。
//! 三者都需要"这个对象还在吗"的**权威答案**（②尤其：句柄由 Rust 持有、删除却发生在别处）；
//! 另有与 A1 同法的**世代令牌**（[`super::generation`]）作 O(1) 兜底。
//!
//! 本层**不用** LVGL 自带的 `lv_obj_is_valid()`：它只把入参与对象树里的指针**比对**，
//! 因此无法排除**假阳性** —— "对象已删、其内存块被**新对象**复用" 时它会回答"有效"，
//! 于是失效句柄的 `Drop` 会去删一个**活的**对象（LVGL 的内存池对这种同尺寸块复用极快，
//! 删一个建一个正是 UI 重建控件的常态）。这类错误比 double free 更隐蔽：不崩、但界面被删掉。
//!
//! 改用**删除事件置位**的存活标志：每个 [`Obj`] 在创建时挂一个只做
//! `alive.set(false)` 的 `LV_EVENT_DELETE` 回调，[`Obj::is_alive`] 即读该标志（外加世代令牌）。
//! 这条路的依据与 A1 **完全同一个契约** —— "LVGL 删除对象（含级联 / `lv_deinit`）必派发
//! `LV_EVENT_DELETE`"（正是 A1 `user_data` 回收赖以成立的那条）；它**不引入**"地址不会被复用"
//! 这个额外假设。（每个对象多一个事件项 + 一个 `Rc<Cell<bool>>`，代价见设计 §10 内存预算。）
//!
//! # 线程
//!
//! 含裸指针与 `Rc` ⇒ 自动 `!Send` / `!Sync`（设计 §5.2 不变量 4：全部 LVGL 调用在事件循环
//! 线程内）。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use lvgl_sys as sys;

use super::display::Area;
use super::event::{self, CallbackHandle, Event, EventCode};
use super::style::{Style, StyleSelector};
use super::LvglError;

/// **测试专用**：`from_raw` 挂探针的累计次数（观察"单例句柄是否每调用一次就挂一条"）。
///
/// 生产构建不可见。用途见 `tests_a3.rs` 的「会话级单例不得无界增长」断言：
/// [`Obj::screen`] / [`super::widgets::layer_top`] 这类**会话级单例**若每次调用都走
/// `from_raw`，就会对**同一个底层对象**反复挂 DELETE 探针 + `Ctx` Box（只在对象删除时
/// 回收）⇒ 每次弹层 / 每帧调用都会无界累积。本计数器让"复用探针"可被**确定性**断言。
#[cfg(test)]
pub(crate) static PROBE_MOUNTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// 会话级单例（`lv_screen_active` / `lv_layer_top`）的**探针种子**容器。
///
/// # 为什么需要它（防无界增长）
///
/// 单例在一个世代内是**同一底层对象**，却可能被每帧 / 每次弹层取用。若每次都
/// [`Obj::from_raw`]，就会对同一对象反复挂 DELETE 探针 + `Ctx` Box（只在对象删除时回收）
/// ⇒ **无界累积**。本容器让每个单例**每世代只挂一条探针**：调用方从种子
/// [`Obj::share_borrowed`] 出借用句柄（复用同一 `alive` / `styles`，不新增探针）。
///
/// 失效重建的两条路径经 [`Obj::is_alive`] 检出：① 对象被删（`alive` 置假）；② 世代变化。
///
/// # 为什么走 `Default` 初始化
///
/// `thread_local!` 的初始化式为 `RefCell::new(None)` 会被 clippy 要求升为 `const {...}`，
/// 而 `RefCell::new` 的 const 稳定化（Rust 1.83）高于本项目声明的本机下限（1.75）；
/// `Default` 初始化与 `event.rs` 的 `CbState::default()` 同法，且不抬高 MSRV。
#[derive(Default)]
pub(crate) struct SingletonSeed(RefCell<Option<Obj>>);

impl SingletonSeed {
    /// 取一个共享探针的**非拥有**句柄；种子与"当前单例"不一致时重建。
    ///
    /// `current_raw` **每次**调用（都是 O(1) 的取指针访问器），其返回值同时用于：
    /// ① 判空（NULL ⇒ 未就绪，归为 [`LvglError::NotInitialized`]）；
    /// ② **识别单例身份变化** —— 如 `lv_display_set_default` 切换默认屏后
    ///    `lv_screen_active()` 会指向另一个（仍存活的）屏幕，此时旧种子虽 `is_alive` 也已过期。
    ///
    /// 重建判据：`raw != 当前指针` 或 `!is_alive()`（对象被删 / 世代变化）。指向同一对象时
    /// 复用现有探针 —— 这正是"每世代至多一条探针"的来源（不随调用次数增长）。
    pub(crate) fn get_or_refresh(
        &self,
        current_raw: impl FnOnce() -> *mut sys::lv_obj_t,
    ) -> Result<Obj, LvglError> {
        let raw_now = current_raw();
        if raw_now.is_null() {
            return Err(LvglError::NotInitialized);
        }
        let mut slot = self.0.borrow_mut();
        if !slot.as_ref().is_some_and(|o| o.is_alive() && o.raw == raw_now) {
            *slot = Some(Obj::adopt_borrowed(raw_now)?);
        }
        Ok(slot
            .as_ref()
            .expect("上方刚确保已填充")
            .share_borrowed())
    }
}

thread_local! {
    /// [`Obj::screen`] 的探针种子（每世代至多一条探针）。
    static SCREEN_SEED: SingletonSeed = SingletonSeed::default();
}

/// 对象标志（`LV_OBJ_FLAG_*` 的镜像）。
///
/// 只列本模块**直接用到**的可见性标志，以及设计 §5.6 控件映射表明确点名、A3 一定会用的
/// 三个标志（`CHECKABLE` 页签/多选 Chip、`CLICKABLE` 模态拦截、`SCROLLABLE` 滚动容器）——
/// 这样 A3 不必回头改本文件。需要更多标志时按同一形式增列即可。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjFlag(u32);

impl ObjFlag {
    /// 隐藏（"如同不存在"：不参与布局与命中）。
    pub const HIDDEN: Self = Self(sys::LV_OBJ_FLAG_HIDDEN as u32);
    /// 可点（命中测试/事件派发的对象）。
    pub const CLICKABLE: Self = Self(sys::LV_OBJ_FLAG_CLICKABLE as u32);
    /// 可勾选（点击后自动切换 `LV_STATE_CHECKED`）。
    pub const CHECKABLE: Self = Self(sys::LV_OBJ_FLAG_CHECKABLE as u32);
    /// 可滚动。
    pub const SCROLLABLE: Self = Self(sys::LV_OBJ_FLAG_SCROLLABLE as u32);

    /// 原始 C 取值。
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// 基础对象（`lv_obj_t` 的所有者句柄）。
///
/// **拥有** `owns = true` 的对象（[`Obj::create`]）在 `Drop` 时删除底层对象；
/// 屏幕句柄（[`Obj::screen`]，`owns = false`）不删 —— 屏幕归 LVGL 所有
/// （删掉活动屏 LVGL 会自动补一个，语义上无意义）。
pub struct Obj {
    raw: *mut sys::lv_obj_t,
    /// 是否由本句柄负责删除（见类型文档）。
    ///
    /// ⚠️ **待 A3 定夺**（A2 评审）：`owns = false` 使 `Obj::screen().delete()` 静默
    /// no-op（评审判无 UB，但语义易误用）—— 是否拆成 `Obj` / `ObjRef` 两型（后者只读、
    /// 无 `delete`）由 A3 前的设计一并决定，本轮不动。
    owns: bool,
    /// 创建时的 LVGL 世代（识别 `init → deinit → init` 之后随 `lv_deinit()` 释放的对象）。
    generation: u64,
    /// 「底层对象仍存在」的**权威标志**：由挂在对象上的 `LV_EVENT_DELETE` 回调置 `false`
    /// （见模块文档「存活判定」）。与 [`EventSub`] 共享同一个 `Rc`。
    alive: Rc<Cell<bool>>,
    /// 已挂样式 [`Style`] 的**共享所有权表**（与 DELETE 探针闭包共享同一个 `Rc`）。
    ///
    /// **不变量**：只要**底层 LVGL 对象**还活着，其上挂的每个 `Style` 就至少有一份 `Rc`
    /// 存活。锚点是挂在对象上的 DELETE 事件项（所有权在 LVGL，对象删除时才回收），
    /// **不是**本 Rust 句柄 —— 故 `Obj::screen()` 这类非拥有句柄先 drop 也不会让样式悬垂。
    ///
    /// 只增不删：`remove_style` **不**提前释放（同一个 `Rc` 可能还挂在别的选择器上，
    /// 提前 reset 会让 LVGL 读到已释放的属性表）。代价是"挂过的样式活到对象死"，
    /// 与目标不变量一致。
    styles: Rc<RefCell<Vec<Rc<Style>>>>,
}

impl Obj {
    /// 取当前活动屏幕（**非拥有**句柄：`Drop` 不删除底层对象）。
    ///
    /// 需要已 `init()` 且已有一个 display 作为默认屏（[`super::display::Display::create`]
    /// 会设为默认）；否则返回 [`LvglError::NotInitialized`]（无默认 display 时 `lv_screen_active()`
    /// 返回 NULL，此处也按此归类）。
    ///
    /// # 会话级单例：探针只挂一次（防无界增长）
    ///
    /// 屏幕在一个世代内是**同一对象**，但本方法可能被每帧 / 每次弹层调用。若每次都
    /// [`Obj::from_raw`]，就会对同一对象反复挂 DELETE 探针 + `Ctx` Box（只在对象删除时
    /// 回收）⇒ 无界累积。故本层按**世代 + 存活**缓存一个"探针种子"，每次返回的都是
    /// 共享同一 `alive` / `styles` 的借用句柄（[`Obj::share_borrowed`]），**不**新增探针。
    ///
    /// 缓存失效重建的**三条**路径：① 屏被删除（含 `lv_deinit` / display 删除）⇒ 种子的
    /// `alive` 置假；② 世代变化 ⇒ 种子的世代判据失配；③ **默认屏被切换**（另一 display
    /// 成为默认，`lv_screen_active()` 指向另一个仍存活的屏）⇒ 当前指针与种子不一致。
    /// 三者都经 [`SingletonSeed::get_or_refresh`] 检出 ⇒ 重建，绝不返回过期屏。
    pub fn screen() -> Result<Self, LvglError> {
        if !super::is_initialized() {
            return Err(LvglError::NotInitialized);
        }
        SCREEN_SEED.with(|seed| {
            // SAFETY: 已 `init()`；`lv_screen_active()` 返回默认屏（可能为 NULL，容器判空）。
            seed.get_or_refresh(|| unsafe { sys::lv_screen_active() })
        })
    }

    /// 在 `parent` 下建一个基础对象（**拥有**句柄：`Drop` / [`Obj::delete`] 删除它）。
    ///
    /// `parent` 必须仍存活（世代未变 **且** 未被删除），否则返回
    /// [`LvglError::NotInitialized`] —— 不会把悬垂的父指针交给 LVGL。
    pub fn create(parent: &Obj) -> Result<Self, LvglError> {
        if !parent.is_alive() {
            return Err(LvglError::NotInitialized);
        }
        // SAFETY: `parent.is_alive()` ⇒ LVGL 已 `init()` 且父对象指针有效。
        let raw = unsafe { sys::lv_obj_create(parent.raw()) };
        if raw.is_null() {
            return Err(LvglError::OutOfMemory("lv_obj_create（LV_MEM_SIZE 不足？）"));
        }
        Ok(Self::from_raw(raw, true))
    }

    /// 薄层内部（`widgets.rs`）：把 `lv_*_create` **刚返回**的裸指针收成**拥有**句柄。
    ///
    /// 与 [`Obj::create`] 的分工：那条路走 `lv_obj_create`（基类），这条走各控件的
    /// `lv_<widget>_create`（子类，带控件行为/绘制）——两者对象模型相同（都是
    /// `lv_obj_t*`），故收尾完全一致：判空 + 挂存活探针 + 交给 `Drop` 删除。
    /// `what` 只用于错误文案（指出是哪个控件创建失败）。
    ///
    /// **前提（`pub(crate)` 的信任边界）**：调用方仅限 `src/lvgl/**` 内部，且 `raw` 必为
    /// `lv_*_create` **刚返回**的裸指针 ⇒ 只需"已初始化 + 判空"，**不**额外验指针有效性
    /// （刚建的对象不可能已被删除）；跨模块的任意裸指针不得喂进来。
    pub(crate) fn adopt_created(
        raw: *mut sys::lv_obj_t,
        what: &'static str,
    ) -> Result<Self, LvglError> {
        if !super::is_initialized() {
            return Err(LvglError::NotInitialized);
        }
        if raw.is_null() {
            return Err(LvglError::OutOfMemory(what));
        }
        Ok(Self::from_raw(raw, true))
    }

    /// 薄层内部（`widgets.rs`）：把 LVGL **自己拥有**的既有对象（`lv_layer_top()`、
    /// `lv_tabview_get_content()` 这类"取一个已存在的子对象"）包成**非拥有**句柄 ——
    /// `Drop` 不删除它（与 [`Obj::screen`] 同口径）。
    pub(crate) fn adopt_borrowed(raw: *mut sys::lv_obj_t) -> Result<Self, LvglError> {
        if !super::is_initialized() {
            return Err(LvglError::NotInitialized);
        }
        if raw.is_null() {
            return Err(LvglError::NotInitialized);
        }
        Ok(Self::from_raw(raw, false))
    }

    /// 包一个刚取得的裸指针，并挂上"删除即置位"的存活探测回调。
    fn from_raw(raw: *mut sys::lv_obj_t, owns: bool) -> Self {
        #[cfg(test)]
        PROBE_MOUNTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let alive = Rc::new(Cell::new(true));
        let alive_w = alive.clone();
        let styles: Rc<RefCell<Vec<Rc<Style>>>> = Rc::new(RefCell::new(Vec::new()));
        // 探针闭包**一并锚住样式共享所有权**：事件项的 `Ctx` 恰在对象被删除时（任何路径：
        // 显式 / 父级联 / `lv_deinit`）由 `event.rs` 回收 ⇒ `styles` 里的每个 `Style`
        // 至少活到**底层 LVGL 对象**死亡（与 Rust 句柄是否拥有该对象无关）。
        let styles_anchor = styles.clone();
        // 过滤 `DELETE`：对象被删（含父级联删除 / `lv_deinit`）时置位；回调体只写一个
        // `Rc<Cell<bool>>` 与一次空引用，**不可能 panic**（薄层纪律 3）。
        // SAFETY: `raw` 是刚创建/刚取得的活对象 ⇒ 满足 `event::on` 的宿主有效性前置条件。
        // 返回的 `CallbackHandle` 在此丢弃 = "挂上就不管"：闭包所有权在 LVGL 事件项上，
        // 由 DELETE 回收（恰好一次；同 tests_a2.rs 场景 ⑭ 对"丢弃订阅不释放闭包"的断言）。
        let _sub = unsafe {
            event::on(raw, EventCode::DELETE, move |_e| {
                alive_w.set(false);
                // 仅为把 `styles_anchor` 移进闭包（所有权即锚点，不做任何读取）。
                let _anchor = &styles_anchor;
            })
        };
        Self {
            raw,
            owns,
            generation: super::generation(),
            alive,
            styles,
        }
    }

    /// 底层指针（薄层内部：`Obj::create` 取父指针、测试显式派发事件；
    /// **对外不暴露** —— 页面代码一律走本类型的安全方法，不得触碰裸指针）。
    pub(crate) fn raw(&self) -> *mut sys::lv_obj_t {
        self.raw
    }

    /// 以 `self` 为**探针种子**再给一个**非拥有**借用句柄：复用同一 `raw` / 世代 /
    /// `alive` 标志 / 样式共享表，**不**重复挂 DELETE 探针、**不**新增 `Ctx`。
    ///
    /// 仅两条**会话级单例**路径使用（[`Obj::screen`] 与 [`super::widgets::layer_top`]）：
    /// 它们可能被高频调用，而底层对象在一个世代内是同一个 ⇒ 每调用一次就 `from_raw`
    /// 会让探针无界累积（只在对象删除时回收）。句柄间共享 `alive` / `styles` 与
    /// [`Obj::stale_generation_handle`] 同法，符合既有所有权不变量（不新增机制）。
    ///
    /// `pub(crate)` 而非 `pub`：这是"探针种子"的内部复用点，不构成新的对外所有权语义。
    pub(crate) fn share_borrowed(&self) -> Self {
        Self {
            raw: self.raw,
            owns: false,
            generation: self.generation,
            alive: self.alive.clone(),
            styles: self.styles.clone(),
        }
    }

    /// 底层对象是否仍存活（**删除标志为真** 且 LVGL 已初始化 **且** 世代未变）。
    ///
    /// 三条失效路径（显式删除 / 父级联删除 / `lv_deinit`）见模块文档。已失效时：
    /// `is_alive() == false`，且本类型的全部方法（含 `Drop`）**自动 no-op**。
    pub fn is_alive(&self) -> bool {
        // 顺序即代价：先读本地标志（零 LVGL 调用），再两项 O(1) 判据。
        self.alive.get() && super::is_initialized() && self.generation == super::generation()
    }

    /// 删除底层对象（消费句柄；`Drop` 不再删第二次）。
    ///
    /// 对象上挂的回调由 [`super::event`] 桥在 DELETE 上**恰好回收一次**（见模块文档）。
    /// 已失效时是 no-op。
    pub fn delete(mut self) {
        if self.owns && self.is_alive() {
            // SAFETY: 仍存活 ⇒ 该对象尚未被删除，删除一次合法。
            unsafe { sys::lv_obj_delete(self.raw) };
        }
        // `Drop` 紧接着运行：置位后不得再删一次（防 double free）。
        self.owns = false;
    }

    /// 设位置（相对父对象**内容区**的左上角）。
    pub fn set_pos(&self, x: i32, y: i32) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_obj_set_pos(self.raw, x, y) };
    }

    /// 设尺寸（像素）。
    pub fn set_size(&self, w: i32, h: i32) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_obj_set_size(self.raw, w, h) };
    }

    /// 在父对象内容区居中。
    pub fn center(&self) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_obj_center(self.raw) };
    }

    /// 当前尺寸 `(宽, 高)`（布局趟落定后的实际值）。
    pub fn size(&self) -> (i32, i32) {
        if !self.is_alive() {
            return (0, 0);
        }
        // SAFETY: 刚校验存活。
        unsafe { (sys::lv_obj_get_width(self.raw), sys::lv_obj_get_height(self.raw)) }
    }

    /// 当前**屏内绝对**坐标矩形（闭区间，与 LVGL 的 `lv_area_t` 一致）。
    ///
    /// 句柄已失效时返回 [`Area::EMPTY`]（**0×0**）—— 与 [`Obj::size`] 的 `(0, 0)` 同口径
    /// （此前返回全零 `lv_area_t`，其 `width()` = `0-0+1` = 1，与 `size()` 自相矛盾）。
    ///
    /// ⚠️ `coords` 由**布局趟**写入：`set_pos` / `set_size` 之后须等一次渲染
    /// （生产 = 每拍 `timer_handler()`；测试 = [`super::display`] 的强制渲染）才反映新值。
    pub fn coords(&self) -> Area {
        if !self.is_alive() {
            return Area::EMPTY;
        }
        let mut a = sys::lv_area_t {
            x1: 0,
            y1: 0,
            x2: 0,
            y2: 0,
        };
        // SAFETY: 刚校验存活；`a` 是本地有效可写对象。
        unsafe { sys::lv_obj_get_coords(self.raw, &mut a) };
        Area::read(&a)
    }

    /// 设可见性（隐藏时"如同不存在"：不参与布局、不接收命中）。
    pub fn set_hidden(&self, hidden: bool) {
        if hidden {
            self.add_flag(ObjFlag::HIDDEN);
        } else {
            self.remove_flag(ObjFlag::HIDDEN);
        }
    }

    /// 当前是否隐藏。
    pub fn is_hidden(&self) -> bool {
        self.has_flag(ObjFlag::HIDDEN)
    }

    /// 置位对象标志。
    pub fn add_flag(&self, flag: ObjFlag) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_obj_add_flag(self.raw, flag.raw() as sys::lv_obj_flag_t) };
    }

    /// 清位对象标志。
    pub fn remove_flag(&self, flag: ObjFlag) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_obj_remove_flag(self.raw, flag.raw() as sys::lv_obj_flag_t) };
    }

    /// 是否置位了该标志（句柄已失效时为 `false`）。
    pub fn has_flag(&self, flag: ObjFlag) -> bool {
        if !self.is_alive() {
            return false;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_obj_has_flag(self.raw, flag.raw() as sys::lv_obj_flag_t) }
    }

    /// 改挂到另一个父对象下（`parent` 必须存活；不合法时 no-op）。
    pub fn set_parent(&self, parent: &Obj) {
        if !self.is_alive() || !parent.is_alive() {
            return;
        }
        // SAFETY: 两个对象均存活。
        unsafe { sys::lv_obj_set_parent(self.raw, parent.raw) };
    }

    /// 直接子对象个数（句柄已失效时为 `0`）。
    pub fn child_count(&self) -> u32 {
        if !self.is_alive() {
            return 0;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_obj_get_child_count(self.raw) }
    }

    /// 挂一个样式（**共享所有权**：本层会克隆一份 `Rc` 并持住它）。
    ///
    /// # 为什么入参是 `&Rc<Style>`
    ///
    /// `lv_obj_add_style()` 只把 `lv_style_t*` **长期存进对象**
    /// （`vendor/lvgl/src/core/lv_obj_style.c`：`obj->styles[i].style = style;`），
    /// 渲染期再解引用它。因此 `Style` 必须活过所有引用它的对象 —— 这是原 `unsafe fn`
    /// 才敢写进 `# Safety` 的前置条件。本层把它**收成类型/所有权不变量**：
    ///
    /// - 调用方交出的 `Rc<Style>` 被克隆进对象的事件项 keeper（见 [`Obj`] 的 `styles`
    ///   字段），样式至少活到**底层 LVGL 对象被删除之后**；
    /// - [`Style`] 内的 `lv_style_t` 是 `Box` ⇒ 移动（`let s2 = s;` / `fn theme() -> Style`）
    ///   不改变 LVGL 存下的地址。
    ///
    /// 两条合起来使"样式先 drop / 样式被移动 → LVGL 读悬垂指针"在**安全代码**里不可达。
    /// 代价：共享后样式不可再变（`&mut` 只在 `Rc::new` 之前可取）—— 这正是"挂上即冻结"。
    ///
    /// 句柄已失效、或 `style` 属于上一世代（属性表已随 `lv_deinit` 释放）时 no-op。
    pub fn add_style(&self, style: &Rc<Style>, selector: StyleSelector) {
        if !self.is_alive() || !style.is_live() {
            return;
        }
        // SAFETY: 本对象存活；`style.raw()` 指向已 `lv_style_init`、世代未变、且地址在
        // 样式存活期内恒定的 `lv_style_t`（`Box`）；紧随其后登记共享所有权 ⇒ 它不会在
        // 本对象（及其事件项）被删除前被 `Drop`。
        unsafe { sys::lv_obj_add_style(self.raw, style.raw(), selector.to_sys()) };
        let mut styles = self.styles.borrow_mut();
        if !styles.iter().any(|s| Rc::ptr_eq(s, style)) {
            styles.push(style.clone());
        }
    }

    /// 摘掉一个样式（`selector` 用 [`super::style::Part::ANY`] / [`super::style::State::ANY`]
    /// 可放宽为"匹配任意部件/状态"）。句柄已失效、或样式已失效时 no-op。
    ///
    /// **不释放**该样式的共享所有权（见 [`Obj`] 的 `styles` 字段）：同一个 `Rc` 可能仍挂在
    /// 别的选择器上，提前 `lv_style_reset` 会让 LVGL 读到已释放的属性表。样式活到本对象死。
    pub fn remove_style(&self, style: &Rc<Style>, selector: StyleSelector) {
        if !self.is_alive() || !style.is_live() {
            return;
        }
        // SAFETY: 本对象存活；`style` 世代未变 ⇒ 其 `lv_style_t`（含属性表）仍有效。
        unsafe { sys::lv_obj_remove_style(self.raw, style.raw(), selector.to_sys()) };
    }

    /// 挂事件回调（**安全**版 [`event::on`]：宿主有效性由 `&self` + 内部存活校验保证）。
    ///
    /// 返回的 [`EventSub`] 可以
    /// ① 直接丢弃 —— 闭包仍挂在对象上，由 DELETE 回收（"挂上就不管"）；
    /// ② 调 [`EventSub::detach`] 主动摘除。
    ///
    /// 两种用法下闭包都**恰好 drop 一次**（不泄漏、不 double free，见 `tests_a2.rs`）。
    /// 句柄已失效时不注册，返回惰性订阅。
    pub fn on<F>(&self, filter: EventCode, f: F) -> EventSub
    where
        F: FnMut(Event) + 'static,
    {
        if !self.is_alive() {
            // 惰性订阅：不注册、`detach` 也是 no-op。
            return EventSub::inert(self.alive.clone(), self.generation);
        }
        // SAFETY: `self.raw` 是刚校验过的活对象 ⇒ 满足 `event::on` 的前置条件（宿主有效）。
        // 闭包的所有权交给 LVGL 事件项，由桥在 DELETE 上回收（回调内删除宿主则延迟回收）。
        let handle = unsafe { event::on(self.raw, filter, f) };
        EventSub {
            handle: Some(handle),
            alive: self.alive.clone(),
            generation: self.generation,
        }
    }

    /// 挂"点击"回调（[`EventCode::CLICKED`] 的常用别名）。
    pub fn on_clicked<F>(&self, f: F) -> EventSub
    where
        F: FnMut(Event) + 'static,
    {
        self.on(EventCode::CLICKED, f)
    }

    /// 向本对象**派发**一个事件（`lv_obj_send_event`）—— [`Obj::on`] 的对偶入口。
    ///
    /// # 用途
    ///
    /// 让 `ui/**` 在**离屏链路**里驱动交互状态机：不接 `indev` 也能把
    /// `PRESSED` / `RELEASED` / `PRESS_LOST` / `LONG_PRESSED` / `CLICKED` 送到对象上，
    /// 于是"松手取消""长按提交""点击防重"这些**只由状态机承载**的分支可被回归覆盖
    /// （此前 `ui` 层只有 `on`、没有派发口 ⇒ 那些分支从未被驱动过）。
    ///
    /// # 语义
    ///
    /// 与真实操作走**同一条** LVGL 派发路径：先送本对象的事件项（[`super::event`] 桥 →
    /// Rust 闭包，过滤器照常生效），再按 LVGL 的冒泡规则传给父链。`param` 传 `NULL`
    /// （LVGL 允许；本层闭包不读它 —— [`Event`] 只携带事件码）。
    ///
    /// **注意**：这是"直接派发"，不经过 `indev` 的命中/滚动/禁用判定 —— 已 `DISABLED` 的
    /// 对象照样能收到事件（正因如此，它才能用来验证回调**自身**的防重逻辑）。
    ///
    /// 句柄已失效时 no-op。
    pub fn send_event(&self, code: EventCode) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活（删除标志 + 已初始化 + 世代）⇒ `self.raw` 指向活对象；
        // `lv_obj_send_event` 只读取该对象及其事件项，`param = NULL` 是 C 侧允许的取值。
        unsafe { sys::lv_obj_send_event(self.raw, code.raw(), std::ptr::null_mut()) };
    }
}

#[cfg(test)]
impl Obj {
    /// **测试专用**（生产构建不可见）：以 `self` 指向的**活对象**为底，构造一个世代被人为
    /// 改小（= 上一世代）的**非拥有**句柄 —— `alive` 标志为真、裸指针健在，**只有世代失配**。
    ///
    /// 用途：**隔离**验证 [`Obj::is_alive`] 的世代判据。场景 ⑰ 走的是"父被删 → 级联删除 →
    /// 探针先置 `alive = false`"的真实路径，`alive` 与世代**同时**为假，无法隔离；本构造器
    /// 让世代成为**唯一**的失效原因（`tests_a2.rs` 场景 ⑰ 断言 `!forged.is_alive()`）。
    pub(crate) fn stale_generation_handle(&self) -> Self {
        Self {
            raw: self.raw,
            owns: false,
            // 测试期世代 ≥ 1，故不会回绕到"恰好等于当前世代"。
            generation: self.generation.wrapping_sub(1),
            alive: self.alive.clone(),
            styles: self.styles.clone(),
        }
    }
}

impl Drop for Obj {
    fn drop(&mut self) {
        // 对象已失效（显式删除后 / 父被删 / `lv_deinit` 之后）⇒ 不得再删（否则 double free）。
        if self.owns && self.is_alive() {
            // SAFETY: 仍存活 ⇒ 该对象尚未被删除。
            // 挂在它（及其子树）上的回调由 `event.rs` 桥在 DELETE 上回收 —— 见模块文档。
            unsafe { sys::lv_obj_delete(self.raw) };
        }
    }
}

impl std::fmt::Debug for Obj {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Obj")
            .field("owns", &self.owns)
            .field("alive", &self.is_alive())
            .finish()
    }
}

/// [`Obj::on`] 的返回值：**可安全摘除**的回调订阅。
///
/// **无 `Drop` 副作用**：丢弃本值 = "挂上就不管"——闭包由 LVGL 事件项持有，在对象被删除时
/// 由 [`event`] 桥**恰好回收一次**（与 A1 的 [`CallbackHandle`] 同契约，但**不泄漏**、
/// 也**不需要** `unsafe` 才能摘除）。
pub struct EventSub {
    /// 摘除用句柄；`None` = 未注册（宿主当时已失效）。
    handle: Option<CallbackHandle<*mut sys::lv_obj_t>>,
    /// 宿主存活标志（与 [`Obj`] 共享同一个 `Rc<Cell<bool>>`）。
    alive: Rc<Cell<bool>>,
    /// 宿主创建时的世代。
    generation: u64,
}

impl EventSub {
    /// 未注册的惰性订阅（宿主当时已失效）。
    fn inert(alive: Rc<Cell<bool>>, generation: u64) -> Self {
        Self {
            handle: None,
            alive,
            generation,
        }
    }

    /// 主动摘除回调并释放闭包（在回调内调用则释放延迟到该外层回调退出）。
    ///
    /// 宿主已删除（或已 `lv_deinit` / 处于上一世代）时是 **no-op** —— 这正好把
    /// [`CallbackHandle::detach`] 的 `# Safety` 前置条件（宿主必须存活）变成本类型的
    /// **自动**校验，调用方不再需要 `unsafe`。
    pub fn detach(mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        // 与 `Obj::is_alive` 同一判据（删除标志 / 已初始化 / 世代）：三者齐备才说明宿主裸指针
        // 仍有效，`remove_event_cb` 解引用它才是安全的。
        let host_is_live =
            self.alive.get() && super::is_initialized() && self.generation == super::generation();
        if host_is_live {
            // SAFETY: 宿主仍存活 ⇒ 满足 `detach` 的前置条件（`remove_event_cb` 会解引用宿主）。
            unsafe { handle.detach() };
        }
        // 不存活时 `handle` 在此 drop：`CallbackHandle` 无 `Drop`，且其 `Ctx` 已由 DELETE
        // 路径回收 ⇒ 无泄漏、无 double free。
    }
}

impl std::fmt::Debug for EventSub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventSub")
            .field("mounted", &self.handle.is_some())
            .finish()
    }
}
