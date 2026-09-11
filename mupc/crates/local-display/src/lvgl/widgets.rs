//! 控件的**类型化构造与属性 setter**（工作单元 A3；设计 §5.1「控件的类型化构造与属性 setter
//! （§5.6 控件映射表）」/ §5.6 控件映射表）。
//!
//! # 本模块的边界（**硬性**，评审检查项）
//!
//! 1. **只搬运机制，不含任何 UI 规格值**：尺寸 / 色值 / 字号 / 圆角 / 间距**一律由调用方传入**
//!    （单一真源是 `ui/theme.rs`）。本模块**不提供**任何"看起来像 UI 规格"的默认值
//!    （不出现 48 / 64 / 1024 / 1000 这类量）；需要初始尺寸的控件由调用方 `set_size()`
//!    （经 `Deref` 到 [`Obj`] 直接可用）。
//! 2. **结构性不暴露文本输入控件**：设计 §5.6 F12 行要求"步进器 / IPv4 / 日期时间一律
//!    `lv_btn` + `lv_label` 组合"，三个文本输入控件在 `lv_conf.h` 中置 0（编译期即不存在）。
//!    本文件在**结构上**不含它们的构造 / 包装 / setter —— `tests_a3.rs` 用源码扫描钉死。
//! 3. **复用 A1/A2，不另起一套**：所有控件句柄都是 [`Obj`] 的类型化包装（`Deref` 转发全部
//!    通用能力：`set_pos` / `set_size` / `add_style` / `on` / `add_flag` …），样式一律
//!    [`super::style::Style`]，事件一律 [`Obj::on`]（A1 的 `catch_unwind` 桥 + 延迟回收）。
//!    本文件**不新建**任何 `user_data` / 回调生命周期 / 存活判定机制。
//! 4. **不提供跨线程 API**：句柄经 [`Obj`] 间接含裸指针 ⇒ 自动 `!Send` / `!Sync`；全部调用
//!    必须在事件循环线程内（设计 §5.2 不变量 4）。
//! 5. **构造失败返回 [`LvglError`]**（父对象已失效 / LVGL 未初始化 / 内存不足）；**setter
//!    沿用 A2 口径：句柄失效即 no-op**（不 panic、不触已释放内存）。
//!
//! # 控件映射（设计 §5.6 / §5.7）
//!
//! | 控件 | 本模块类型 | 说明 |
//! |------|-----------|------|
//! | `lv_button` | [`Button`] / [`TextButton`] | 通用按钮；**步进器 / IPv4 / 日期时间一律用 `Button` + [`Label`] 组合**（`TextButton` 即该组合的底座） |
//! | `lv_label` | [`Label`] | 文本；长文本用 [`LongMode::WRAP`] |
//! | `lv_list` | [`List`] | 日志 / 审计 / 告警列表容器 |
//! | `lv_table` | [`Table`] | 配置前后值、点表 |
//! | `lv_bar` | [`Bar`] | 进度 / 长按进度、数值条 |
//! | `lv_led` | [`Led`] | 状态灯；**F14 三重冗余**：灯只是"灯"通道，`.text` 必须并列一个 [`Label`]（见 [`Led::create_with_text`]） |
//! | `lv_dropdown` | [`Dropdown`] | 选项式选择（日志筛选等） |
//! | `lv_switch` | [`Switch`] | 布尔开关 |
//! | `lv_msgbox` | [`MsgBox`] | 确认弹层底座（[`MsgBox::modal`] 自带全屏遮罩） |
//! | `lv_buttonmatrix` | [`ButtonMatrix`] | 键矩阵（选项式选择 / 数字输入替代品） |
//! | `lv_checkbox` | [`Checkbox`] | 复选（多选筛选） |
//! | `lv_tabview` | [`TabView`] | 6 页导航候选（另一候选见 `ScrollContainer` + 自建导航栏） |
//! | 滚动容器 | [`ScrollContainer`] | `SCROLLABLE` + [`Dir::VER`]（**结构性禁横滚**）+ 滚动条纯指示 |
//! | 输入组 | [`Group`] | TT-09「取消」默认焦点（[`Group::focus`]） |
//!
//! # 滚动条口径（设计 §5.6-A，**已按 v9.5.0 源码核实**）
//!
//! `LV_PART_SCROLLBAR` 是**样式部件**而非 `lv_obj`（`src/core/lv_obj_style.h`），输入侧
//! `src/indev/lv_indev_scroll.c` **全文 0 处 `scrollbar` 引用** ⇒ 框架本就**不可拖**。
//! 本模块因此只提供"模式 + 方向"的设置与读取（[`set_scrollbar_mode`] / [`ScrollMode::AUTO`]），
//! **不实现**任何"拖 thumb / 点轨道"逻辑（框架亦无处可挂）。滚动条宽度/配色属 `ui/theme.rs`。
//!
//! # 长按（设计 §5.6）——**入口在 [`super::indev::Indev::set_long_press_time`]**
//!
//! 事件仍走 A1 的 [`super::event::EventCode::LONG_PRESSED`] / `PRESSED` / `RELEASED` /
//! `PRESS_LOST`，用 [`Obj::on`] 注册（无需 `unsafe`）。**逐对象**的长按阈值 API 是 v8 的
//! 产物、v9.5.0 不存在（详见 [`super::indev::Indev::set_long_press_time`] 的文档）。
//!
//! # LVGL "缓存 / 单例对象"的处置纪律（A3 质量复审，**已按 v9.5.0 源码逐项核对**）
//!
//! LVGL 有两类接口返回的对象**不是**调用方新建的：
//!
//! - **会话级单例**：`lv_screen_active()` / `lv_layer_top()`（全进程各一份）；
//! - **控件字段缓存的单例**：`lv_msgbox_add_title`（只建一次，之后恒返回 `mbox->title`）、
//!   `lv_tabview_get_content` / `get_tab_bar`、`lv_msgbox_get_content` 等。
//!
//! 处置规则：**凡返回非新建对象的，一律 [`Obj::adopt_borrowed`]（非拥有）**，绝不
//! `adopt_created` —— 后者 `drop` 会删掉对象，而 LVGL 侧仍持有该指针 ⇒ UAF。逐项核对结果：
//!
//! | C 接口 | 语义 | 本层处置 |
//! |--------|------|----------|
//! | `lv_msgbox_add_title` | **缓存单例**（`msgbox.c:163-172`） | `add_title` → `adopt_borrowed`（Critical 修正） |
//! | `lv_msgbox_add_text` | 每次 `lv_label_create` **新建** | `add_text` → 拥有 |
//! | `lv_msgbox_add_footer_button` | 每次新建 button **新建** | `add_footer_button` → 拥有 |
//! | `lv_msgbox_get_content` | 缓存（`mbox->content`） | `content` → `adopt_borrowed` |
//! | `lv_tabview_get_content` / `_get_tab_bar` | 缓存 | `content` / `tab_bar` → `adopt_borrowed` |
//! | `lv_tabview_add_tab` | 每次 `lv_obj_create` **新建**页 | `add_tab` → 拥有 |
//! | `lv_list_add_text` | 每次新建 | `List::add_text` → 拥有 |
//! | `lv_layer_top` / `lv_screen_active` | 会话级单例 | `layer_top` / `Obj::screen` → `adopt_borrowed` + **探针种子缓存**（防无界增长） |
//!
//! `lv_list_get_*` / `lv_dropdown_get_list` / `lv_msgbox_get_footer` / `_get_title` 本层**未封装**
//! （不提供入口即无风险）；若日后新增，按上表同法处置。

use std::cell::RefCell;
use std::ffi::{c_char, CStr, CString};

use lvgl_sys as sys;

use super::obj::{Obj, ObjFlag, SingletonSeed};
use super::style::{Color, State};
use super::LvglError;

// Obj 的通用能力（set_pos/set_size/add_style/on/…）经下面各类型的 `Deref` 转发。

// ═══════════════════════════════════════════════════════════════════════════
// 类型化取值（C 侧开放整数集 → 有名字的类型；与 style.rs 的 Part/State 同法）
// ═══════════════════════════════════════════════════════════════════════════

/// 方向（`lv_dir_t` 的镜像）。
///
/// 用途：滚动容器的**滚动方向**（[`Dir::VER`] = 结构性禁横滚）、`lv_dropdown` 的展开方向、
/// `lv_tabview` 的标签栏位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Dir(u32);

impl Dir {
    /// 无方向（`lv_dropdown` 可用来固定只朝某侧展开前的默认值）。
    pub const NONE: Self = Self(sys::LV_DIR_NONE as u32);
    /// 左。
    pub const LEFT: Self = Self(sys::LV_DIR_LEFT as u32);
    /// 右。
    pub const RIGHT: Self = Self(sys::LV_DIR_RIGHT as u32);
    /// 上。
    pub const TOP: Self = Self(sys::LV_DIR_TOP as u32);
    /// 下。
    pub const BOTTOM: Self = Self(sys::LV_DIR_BOTTOM as u32);
    /// 水平（左 + 右）。
    pub const HOR: Self = Self(sys::LV_DIR_HOR as u32);
    /// 垂直（上 + 下）—— 滚动容器只给这一项。
    pub const VER: Self = Self(sys::LV_DIR_VER as u32);
    /// 全部。
    pub const ALL: Self = Self(sys::LV_DIR_ALL as u32);

    /// 原始 C 取值。
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// 滚动条显示模式（`lv_scrollbar_mode_t` 的镜像）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollMode(u32);

impl ScrollMode {
    /// 从不显示。
    pub const OFF: Self = Self(sys::LV_SCROLLBAR_MODE_OFF as u32);
    /// 常显。
    pub const ON: Self = Self(sys::LV_SCROLLBAR_MODE_ON as u32);
    /// 滚动时显示。
    pub const ACTIVE: Self = Self(sys::LV_SCROLLBAR_MODE_ACTIVE as u32);
    /// 内容超出视口才显示（**本设计的默认口径**，设计 §5.6-A 方案 A）。
    pub const AUTO: Self = Self(sys::LV_SCROLLBAR_MODE_AUTO as u32);

    /// 原始 C 取值。
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// 文本长模式（`lv_label_long_mode_t` 的镜像）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LongMode(u32);

impl LongMode {
    /// 定宽换行、高度自适应（**长文本用这一档**，设计 §1.1.2「中文断行由 LVGL 处理」）。
    pub const WRAP: Self = Self(sys::LV_LABEL_LONG_MODE_WRAP as u32);
    /// 定尺寸，超出部分以 `…` 收尾。
    pub const DOTS: Self = Self(sys::LV_LABEL_LONG_MODE_DOTS as u32);
    /// 定尺寸，来回滚动。
    pub const SCROLL: Self = Self(sys::LV_LABEL_LONG_MODE_SCROLL as u32);
    /// 定尺寸，循环滚动。
    pub const SCROLL_CIRCULAR: Self = Self(sys::LV_LABEL_LONG_MODE_SCROLL_CIRCULAR as u32);
    /// 定尺寸，直接裁剪。
    pub const CLIP: Self = Self(sys::LV_LABEL_LONG_MODE_CLIP as u32);

    /// 原始 C 取值。
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// 进度条模式（`lv_bar_mode_t` 的镜像）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BarMode(u32);

impl BarMode {
    /// 从 min 起单向填充。
    pub const NORMAL: Self = Self(sys::LV_BAR_MODE_NORMAL as u32);
    /// 以 0 为中点双向填充（充电/放电这类双向量）。
    pub const SYMMETRICAL: Self = Self(sys::LV_BAR_MODE_SYMMETRICAL as u32);
    /// 用 start/end 两点画区间（SOC 15 %/85 % 警示档即用它）。
    pub const RANGE: Self = Self(sys::LV_BAR_MODE_RANGE as u32);

    /// 原始 C 取值。
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// 值变更是否走 LVGL 内建动画（`lv_anim_enable_t` 的镜像）。
///
/// **显式传参、不设默认值**：设计 §5.6「动效纪律」只允许状态切换类动效，值变更要不要
/// 动画必须由调用方决定（本层不替它选）。
///
/// 注意 C 侧本体是 **`typedef bool lv_anim_enable_t`**（v9.5.0 `src/misc/lv_anim.h:87`），
/// `LV_ANIM_OFF` / `LV_ANIM_ON` 是 **`#define false/true`**（不经 bindgen 生成）——
/// 故此处镜像为 `bool` 而不是整数枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Anim(bool);

impl Anim {
    /// 立即生效（无动画）。
    pub const OFF: Self = Self(false);
    /// LVGL 内建缓动。
    pub const ON: Self = Self(true);

    /// 原始 C 取值（`lv_anim_enable_t` = `bool`）。
    pub const fn raw(self) -> bool {
        self.0
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 薄层内部工具
// ═══════════════════════════════════════════════════════════════════════════

/// Rust `&str`（UTF-8）→ C 字符串。
///
/// LVGL 的文本接口一律是 **C 字符串**，无法表达内嵌 NUL。此处**截断到首个 NUL 之前**
/// （不 panic —— 薄层纪律；NUL 是单字节，截断不会切断多字节 UTF-8 序列）。
///
/// `expect` 分支**可证不可达**（非"大概到不了"）：`end` 取 `position(|b| *b == 0)` 或
/// `bytes.len()`，故 `&bytes[..end]` **按构造**不含任何 `0` 字节 —— `CString::new`
/// 只在入参含内嵌 NUL 时返回 `Err`，此处置条件恒不成立。（评审已判不可达，此处把证明写死。）
fn to_cstring(text: &str) -> CString {
    let bytes = text.as_bytes();
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    CString::new(&bytes[..end]).expect("已按 NUL 截断，不可能再含 NUL")
}

/// `lv_*_create(parent)` 的公共收尾：**父对象存活前置** + 判空 + 收成拥有句柄。
fn create_child(
    parent: &Obj,
    what: &'static str,
    create: unsafe extern "C" fn(*mut sys::lv_obj_t) -> *mut sys::lv_obj_t,
) -> Result<Obj, LvglError> {
    if !parent.is_alive() {
        return Err(LvglError::NotInitialized);
    }
    // SAFETY: 父对象刚校验存活 ⇒ 满足各 `lv_*_create` 的父指针前置条件。
    let raw = unsafe { create(parent.raw()) };
    Obj::adopt_created(raw, what)
}

/// 置位 / 清位对象**状态**（`LV_STATE_*`）。
///
/// [`Obj`] 封装的是**对象标志**（[`ObjFlag`]）；状态是同一层的另一套位域，且是主题
/// 状态色 / 禁用 / 勾选 / 聚焦的驱动力（TT-10 的按钮禁用反馈、§6.2 步进器越界禁用、
/// [`Switch`] / [`Checkbox`] 的勾选态、TT-09 的默认焦点）。因此这**三个**函数是 A2
/// 未覆盖、而 B 必然要用的通用能力（对应设计 §5.6 直接写的 `lv_obj_add_state(btn, LV_STATE_DISABLED)`）。
pub fn set_state(obj: &Obj, state: State, on: bool) {
    if !obj.is_alive() {
        return;
    }
    // SAFETY: 刚校验存活；`state.raw()` 是 `LV_STATE_*` 位域（与 C 侧同型）。
    unsafe {
        if on {
            sys::lv_obj_add_state(obj.raw(), state.raw() as sys::lv_state_t);
        } else {
            sys::lv_obj_remove_state(obj.raw(), state.raw() as sys::lv_state_t);
        }
    }
}

/// 是否处于该状态（句柄失效时为 `false`）。
pub fn has_state(obj: &Obj, state: State) -> bool {
    if !obj.is_alive() {
        return false;
    }
    // SAFETY: 刚校验存活。
    unsafe { sys::lv_obj_has_state(obj.raw(), state.raw() as sys::lv_state_t) }
}

thread_local! {
    /// `lv_layer_top()` 的**探针种子**（会话级单例）：每世代至多挂一条 DELETE 探针，见
    /// [`layer_top`] 的「会话级单例」说明（与 [`Obj::screen`] 共用 [`SingletonSeed`]）。
    static LAYER_TOP_SEED: SingletonSeed = SingletonSeed::default();
}

/// 顶层图层（`lv_layer_top()`）—— 模态容器 / Toast 的宿主（设计 §5.7 确认对话框行）。
///
/// **非拥有**句柄：图层归 LVGL 所有（同 [`Obj::screen`]），`Drop` 不删除它。
/// 典型的模态容器 = 在本图层上 [`Obj::create`] 一个全屏对象 + [`ObjFlag::CLICKABLE`]
/// 拦截穿透（设计 §5.7）。[`MsgBox::modal`] 已把这条路径封装好。
///
/// # 会话级单例：探针只挂一次（防无界增长）
///
/// 图层在一个世代内是**同一对象**，但本函数可能每次弹层 / 每帧都被调用。若每次都
/// `from_raw`，就会对同一对象反复挂 DELETE 探针 + `Ctx`（只在对象删除时回收）⇒ 无界
/// 累积。故按 **世代 + 存活** 缓存"探针种子"，每次返回共享同一 `alive` / `styles` 的
/// **非拥有**借用句柄（[`Obj::share_borrowed`]，与 A2 的 [`Obj::screen`] 同法、同不变量）。
/// 种子失效（图层被删 / `lv_deinit` 世代变化 / **默认屏切换**导致 `lv_layer_top()` 指向
/// 另一个仍存活的图层）时经 [`SingletonSeed::get_or_refresh`] 检出并重建，绝不返回过期图层。
pub fn layer_top() -> Result<Obj, LvglError> {
    if !super::is_initialized() {
        return Err(LvglError::NotInitialized);
    }
    LAYER_TOP_SEED.with(|seed| {
        // SAFETY: 已 `init()`；`lv_layer_top()` 返回 LVGL 自己的图层对象（可能 NULL，容器判空）。
        seed.get_or_refresh(|| unsafe { sys::lv_layer_top() })
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// 滚动容器辅助（设计 §5.6「滚动与滚动指示」/ §5.6-A）
// ═══════════════════════════════════════════════════════════════════════════

/// 设滚动方向（[`Dir::VER`] = **结构性禁止横滚**：LVGL 的滚动轴由该方向位决定，
/// 横轴根本不参与拖拽判定）。
pub fn set_scroll_dir(obj: &Obj, dir: Dir) {
    if !obj.is_alive() {
        return;
    }
    // SAFETY: 刚校验存活。
    unsafe { sys::lv_obj_set_scroll_dir(obj.raw(), dir.raw() as sys::lv_dir_t) };
}

/// 读回滚动方向（设置生效的离屏断言口径）。
pub fn scroll_dir(obj: &Obj) -> Dir {
    if !obj.is_alive() {
        return Dir::NONE;
    }
    // SAFETY: 刚校验存活。
    Dir(unsafe { sys::lv_obj_get_scroll_dir(obj.raw()) } as u32)
}

/// 设滚动条显示模式（[`ScrollMode::AUTO`] = 内容超出视口才出现）。
///
/// **只影响"画不画"**，不影响可交互性 —— 滚动条本就不可拖（见模块文档 §滚动条口径）。
pub fn set_scrollbar_mode(obj: &Obj, mode: ScrollMode) {
    if !obj.is_alive() {
        return;
    }
    // SAFETY: 刚校验存活。
    unsafe { sys::lv_obj_set_scrollbar_mode(obj.raw(), mode.raw() as sys::lv_scrollbar_mode_t) };
}

/// 读回滚动条模式。
pub fn scrollbar_mode(obj: &Obj) -> ScrollMode {
    if !obj.is_alive() {
        return ScrollMode::OFF;
    }
    // SAFETY: 刚校验存活。
    ScrollMode(unsafe { sys::lv_obj_get_scrollbar_mode(obj.raw()) } as u32)
}

// ═══════════════════════════════════════════════════════════════════════════
// 控件句柄（薄包装：`Deref` 到 `Obj` ⇒ 通用能力全部可用）
// ═══════════════════════════════════════════════════════════════════════════

/// 生成一个控件句柄类型：`pub fn create(&Obj)` + `Deref/DerefMut<Target = Obj>`。
///
/// 这样"控件特有 setter"与"通用对象能力"（坐标 / 尺寸 / 样式 / 事件 / 标志）在同一处
/// 可用，且**不存在第二条生命周期真源** —— 存活判定、样式共享所有权、回调回收全部是
/// [`Obj`] 那一套（模块文档第 3 条）。
macro_rules! widget_handle {
    ($(#[$doc:meta])* $name:ident, $create:ident, $what:literal) => {
        $(#[$doc])*
        pub struct $name {
            obj: Obj,
        }

        impl $name {
            /// 在 `parent` 下创建（失败见 [`LvglError`]）。**尺寸 / 外观由调用方设定**
            /// （`set_size` / `add_style` 经 `Deref` 直接可用）。
            pub fn create(parent: &Obj) -> Result<Self, LvglError> {
                Ok(Self {
                    obj: create_child(parent, $what, sys::$create)?,
                })
            }

            /// 底层对象（只读）。
            pub fn obj(&self) -> &Obj {
                &self.obj
            }

            /// 取出底层 [`Obj`]（组合 / 转移所有权场景）。
            pub fn into_obj(self) -> Obj {
                self.obj
            }

            /// 显式删除底层对象（等价于 `drop`）。
            pub fn delete(self) {
                self.obj.delete();
            }
        }

        impl std::ops::Deref for $name {
            type Target = Obj;
            fn deref(&self) -> &Obj {
                &self.obj
            }
        }

        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Obj {
                &mut self.obj
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($name), "({:?})"), self.obj)
            }
        }
    };
}

widget_handle!(
    /// 按钮（`lv_button`）—— 通用可点控件。
    ///
    /// 带文字的按钮用 [`TextButton`]（= 本类型 + [`Label`] 的组合）；**步进器 / IPv4 /
    /// 日期时间**按设计 §5.6 F12 行一律由该组合搭建（不引入任何文本输入控件）。
    Button,
    lv_button_create,
    "lv_button_create"
);

widget_handle!(
    /// 文本（`lv_label`）。长文本用 [`Label::set_long_mode`]`(`[`LongMode::WRAP`]`)`。
    Label,
    lv_label_create,
    "lv_label_create"
);

widget_handle!(
    /// 列表容器（`lv_list`）—— 日志 / 审计 / 告警列表。
    ///
    /// ⚠️ 长列表的**窗口化**（只保留可视行 ×1.5 的对象）是页面层的事（设计 §5.7 / R-22）。
    List,
    lv_list_create,
    "lv_list_create"
);

widget_handle!(
    /// 表格（`lv_table`）—— 配置前后值、点表。
    Table,
    lv_table_create,
    "lv_table_create"
);

widget_handle!(
    /// 进度 / 数值条（`lv_bar`）。长按 1.0 s 的进度反馈也用它（值驱动，不用 `lv_anim`）。
    Bar,
    lv_bar_create,
    "lv_bar_create"
);

widget_handle!(
    /// 状态灯（`lv_led`）。
    ///
    /// **F14 语义三重冗余**：本控件只是"灯"这一个通道，`.text` 必须**并列**一个
    /// [`Label`]（[`Led::create_with_text`] 把这一对一次给出，避免漏配）。
    Led,
    lv_led_create,
    "lv_led_create"
);

widget_handle!(
    /// 选项式选择（`lv_dropdown`）—— 日志筛选 / 枚举配置项。
    Dropdown,
    lv_dropdown_create,
    "lv_dropdown_create"
);

widget_handle!(
    /// 布尔开关（`lv_switch`）。勾选态即 `LV_STATE_CHECKED`（见 [`Switch::set_checked`]）。
    Switch,
    lv_switch_create,
    "lv_switch_create"
);

widget_handle!(
    /// 复选（`lv_checkbox`）—— 多选筛选。
    Checkbox,
    lv_checkbox_create,
    "lv_checkbox_create"
);

widget_handle!(
    /// 6 页导航候选之一（`lv_tabview`）。
    ///
    /// **隐藏内置标签栏**（设计 §5.4 的 `LV_TAB_POS_NONE`）在 v9.5.0 **不存在该常量** ——
    /// 正确做法是取到标签栏对象后挂 [`ObjFlag::HIDDEN`]：`tabview.tab_bar()?.set_hidden(true)`。
    TabView,
    lv_tabview_create,
    "lv_tabview_create"
);

// ── Label ────────────────────────────────────────────────────────────────

impl Label {
    /// 建标签并设文本（等价 `create` + `set_text`）。
    pub fn create_with_text(parent: &Obj, text: &str) -> Result<Self, LvglError> {
        let label = Self::create(parent)?;
        label.set_text(text);
        Ok(label)
    }

    /// 设文本（LVGL 会**复制**该字符串，故临时 `CString` 即可）。
    pub fn set_text(&self, text: &str) {
        if !self.is_alive() {
            return;
        }
        let c = to_cstring(text);
        // SAFETY: 刚校验存活；`c` 的生命周期覆盖本次调用（LVGL 内部复制）。
        unsafe { sys::lv_label_set_text(self.raw(), c.as_ptr()) };
    }

    /// 读回文本（离屏断言用；句柄失效时为 `None`）。
    pub fn text(&self) -> Option<String> {
        if !self.is_alive() {
            return None;
        }
        // SAFETY: 刚校验存活；LVGL 返回其内部 `'\0'` 结尾的缓冲区（在本对象存活期内有效）。
        let p = unsafe { sys::lv_label_get_text(self.raw()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: `p` 非空且指向 LVGL 自己维护的 NUL 结尾字符串。
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    /// 设长模式（长文本 / 中文换行用 [`LongMode::WRAP`]，设计 §1.1.2）。
    pub fn set_long_mode(&self, mode: LongMode) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_label_set_long_mode(self.raw(), mode.raw() as sys::lv_label_long_mode_t) };
    }
}

// ── List ─────────────────────────────────────────────────────────────────

impl List {
    /// 追加一行纯文本（返回该行的 [`Label`] 句柄；`drop` 它即删掉该行）。
    pub fn add_text(&self, text: &str) -> Result<Label, LvglError> {
        if !self.is_alive() {
            return Err(LvglError::NotInitialized);
        }
        let c = to_cstring(text);
        // SAFETY: 刚校验存活；`c` 生命周期覆盖本次调用（`lv_list_add_text` 内部复制文本）。
        let raw = unsafe { sys::lv_list_add_text(self.raw(), c.as_ptr()) };
        Ok(Label {
            obj: Obj::adopt_created(raw, "lv_list_add_text")?,
        })
    }
}

// ── Table ────────────────────────────────────────────────────────────────

impl Table {
    /// 设列数（`lv_table` 的行列是"逻辑尺寸"，与控件像素尺寸无关）。
    pub fn set_column_count(&self, count: u32) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_table_set_column_count(self.raw(), count) };
    }

    /// 设行数。
    pub fn set_row_count(&self, count: u32) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_table_set_row_count(self.raw(), count) };
    }

    /// 当前列数。
    pub fn column_count(&self) -> u32 {
        if !self.is_alive() {
            return 0;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_table_get_column_count(self.raw()) }
    }

    /// 当前行数。
    pub fn row_count(&self) -> u32 {
        if !self.is_alive() {
            return 0;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_table_get_row_count(self.raw()) }
    }

    /// 设单元格文本（LVGL 会复制）。
    pub fn set_cell(&self, row: u32, col: u32, text: &str) {
        if !self.is_alive() {
            return;
        }
        let c = to_cstring(text);
        // SAFETY: 刚校验存活；`c` 生命周期覆盖本次调用。
        unsafe { sys::lv_table_set_cell_value(self.raw(), row, col, c.as_ptr()) };
    }

    /// 读单元格文本（离屏断言用）。
    pub fn cell(&self, row: u32, col: u32) -> Option<String> {
        if !self.is_alive() {
            return None;
        }
        // SAFETY: 刚校验存活；返回 LVGL 内部 NUL 结尾缓冲（越界入参由 LVGL 自身处理）。
        let p = unsafe { sys::lv_table_get_cell_value(self.raw(), row, col) };
        if p.is_null() {
            return None;
        }
        // SAFETY: `p` 非空且 NUL 结尾。
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    /// 设列宽（像素；宽度值属 `ui/theme.rs`）。
    pub fn set_column_width(&self, col: u32, width: i32) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_table_set_column_width(self.raw(), col, width) };
    }
}

// ── Bar ──────────────────────────────────────────────────────────────────

impl Bar {
    /// 设量程（`min` / `max` 的取法属 `ui/theme.rs`；本层只搬运）。
    pub fn set_range(&self, min: i32, max: i32) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_bar_set_range(self.raw(), min, max) };
    }

    /// 设当前值（`anim` 由调用方显式选择，本层不替它决定 —— 见 [`Anim`]）。
    pub fn set_value(&self, value: i32, anim: Anim) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_bar_set_value(self.raw(), value, anim.raw()) };
    }

    /// 设区间起点（配合 [`BarMode::RANGE`] 画 SOC 15 %/85 % 这类警示档）。
    pub fn set_start_value(&self, value: i32, anim: Anim) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_bar_set_start_value(self.raw(), value, anim.raw()) };
    }

    /// 设模式。
    pub fn set_mode(&self, mode: BarMode) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_bar_set_mode(self.raw(), mode.raw() as sys::lv_bar_mode_t) };
    }

    /// 当前值。
    pub fn value(&self) -> i32 {
        if !self.is_alive() {
            return 0;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_bar_get_value(self.raw()) }
    }
}

// ── Led ──────────────────────────────────────────────────────────────────

impl Led {
    /// 建"灯 + 文本"一对（**F14 三重冗余**：颜色走 [`Led::set_color`]、文本走 [`Label`]，
    /// 再叠上图标/形状即三通道；本函数把"灯与文本必须并列"从文档约定变成一处构造）。
    ///
    /// 布局（并排/上下、间距）由调用方给 —— 本层不预设任何坐标。
    pub fn create_with_text(parent: &Obj, text: &str) -> Result<(Self, Label), LvglError> {
        let led = Self::create(parent)?;
        let label = Label::create_with_text(parent, text)?;
        Ok((led, label))
    }

    /// 设灯色（F14 的色通道；`lv_led` 的颜色是**控件字段**而非样式，只能由此路径设定）。
    pub fn set_color(&self, color: Color) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活；`Color::to_sys` 是本层唯一的 XRGB8888 内存序真源。
        unsafe { sys::lv_led_set_color(self.raw(), color.to_sys()) };
    }

    /// 设亮度（0–255；`lv_led` 内部会按 `LV_LED_BRIGHT_MIN/MAX` 夹取）。
    pub fn set_brightness(&self, brightness: u8) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_led_set_brightness(self.raw(), brightness) };
    }

    /// 当前亮度。
    pub fn brightness(&self) -> u8 {
        if !self.is_alive() {
            return 0;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_led_get_brightness(self.raw()) }
    }

    /// 点亮（= 最大亮度）。
    pub fn on(&self) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_led_on(self.raw()) };
    }

    /// 熄灭。
    pub fn off(&self) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_led_off(self.raw()) };
    }

    /// 翻转亮/灭。
    pub fn toggle(&self) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_led_toggle(self.raw()) };
    }
}

// ── Dropdown ─────────────────────────────────────────────────────────────

impl Dropdown {
    /// 设选项（**以 `\n` 分隔**；LVGL 会复制该串）。
    ///
    /// ⚠️ 空串 / 末位 `\n` 在 LVGL 侧会得到空选项，属调用方数据问题（本层不猜测、不修正）。
    pub fn set_options(&self, options: &str) {
        if !self.is_alive() {
            return;
        }
        let c = to_cstring(options);
        // SAFETY: 刚校验存活；`c` 生命周期覆盖本次调用（内部 `lv_malloc` + `lv_strcpy`）。
        unsafe { sys::lv_dropdown_set_options(self.raw(), c.as_ptr()) };
    }

    /// 当前选中项下标（0 起）。
    pub fn selected(&self) -> u32 {
        if !self.is_alive() {
            return 0;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_dropdown_get_selected(self.raw()) }
    }

    /// 设选中项下标（越界由 LVGL 自行夹取）。
    pub fn set_selected(&self, index: u32) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_dropdown_set_selected(self.raw(), index) };
    }

    /// 设展开方向（弹层往哪侧展开；受屏幕边界的自适应仍由 LVGL 负责）。
    pub fn set_dir(&self, dir: Dir) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_dropdown_set_dir(self.raw(), dir.raw() as sys::lv_dir_t) };
    }

    /// 设"未选中时显示的引导文字"（LVGL 会复制）。
    pub fn set_text(&self, text: &str) {
        if !self.is_alive() {
            return;
        }
        let c = to_cstring(text);
        // SAFETY: 刚校验存活；`c` 生命周期覆盖本次调用（内部 `lv_strdup`）。
        unsafe { sys::lv_dropdown_set_text(self.raw(), c.as_ptr()) };
    }
}

// ── Switch / Checkbox ────────────────────────────────────────────────────

impl Switch {
    /// 勾选态 = `LV_STATE_CHECKED`（v9 的开关状态就是这一位，见 `src/widgets/switch/lv_switch.c`）。
    pub fn set_checked(&self, checked: bool) {
        set_state(self, State::CHECKED, checked);
    }

    /// 是否勾选。
    pub fn is_checked(&self) -> bool {
        has_state(self, State::CHECKED)
    }
}

impl Checkbox {
    /// 设标签文字（LVGL 会复制）。
    pub fn set_text(&self, text: &str) {
        if !self.is_alive() {
            return;
        }
        let c = to_cstring(text);
        // SAFETY: 刚校验存活；`c` 生命周期覆盖本次调用（内部复制）。
        unsafe { sys::lv_checkbox_set_text(self.raw(), c.as_ptr()) };
    }

    /// 勾选态 = `LV_STATE_CHECKED`。
    pub fn set_checked(&self, checked: bool) {
        set_state(self, State::CHECKED, checked);
    }

    /// 是否勾选。
    pub fn is_checked(&self) -> bool {
        has_state(self, State::CHECKED)
    }
}

// ── ButtonMatrix ─────────────────────────────────────────────────────────

/// 键矩阵（`lv_buttonmatrix`）—— 选项式选择 / 数字输入的替代品（**零文本输入**）。
///
/// # 为什么不能直接用宏生成
///
/// `lv_buttonmatrix_set_map()` 只把地图的**裸指针**存进控件（v9.5.0
/// `src/widgets/buttonmatrix/lv_buttonmatrix.c`：`btnm->map_p = map;`，**不复制**）。
/// 因此 Rust 侧必须**持有**这份地图直到控件死亡 —— 本类型用 `map` 字段锚住
/// `CString` 串与其指针数组（与 [`super::obj::Obj`] 的"样式共享所有权"同一思路：
/// 不留悬垂指针的缝）。
pub struct ButtonMatrix {
    obj: Obj,
    /// 锚住地图：`strings` 是串本体，`pointers` 是指向它们的 **NULL 结尾**指针数组。
    /// 只**整体替换**、不原地增删（LVGL 存的是 `pointers` 的堆地址）。
    map: RefCell<Option<MapStorage>>,
}

#[derive(Default)]
struct MapStorage {
    strings: Vec<CString>,
    pointers: Vec<*const c_char>,
}

/// `LV_BUTTONMATRIX_BUTTON_NONE` 的取值。
///
/// v9.5.0 里它是 **`#define …  0xFFFF`**（`src/widgets/buttonmatrix/lv_buttonmatrix.h:26`），
/// 不经 bindgen 生成（故也不进 allowlist —— allowlist 只登记真正生成的符号）。
/// 语义由 `tests_a3.rs` 的行为断言钉死（未选中 ⇒ `None`）。
const BUTTONMATRIX_BUTTON_NONE: u32 = 0xFFFF;

impl ButtonMatrix {
    /// 在 `parent` 下创建（地图由 [`ButtonMatrix::set_map`] 设定）。
    pub fn create(parent: &Obj) -> Result<Self, LvglError> {
        Ok(Self {
            obj: create_child(
                parent,
                "lv_buttonmatrix_create",
                sys::lv_buttonmatrix_create,
            )?,
            map: RefCell::new(None),
        })
    }

    /// 底层对象（只读）。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 设地图：每个元素是一个键的文本；元素 **`"\n"` 表示换行**（与 LVGL 语义一致）。
    ///
    /// 地图整体替换后立即交给 LVGL，并把两个 `Vec` 留在 `self` 里续命（见类型文档）。
    pub fn set_map(&self, buttons: &[&str]) {
        if !self.is_alive() {
            return;
        }
        let mut storage = MapStorage {
            strings: buttons.iter().map(|b| to_cstring(b)).collect(),
            pointers: Vec::new(),
        };
        storage.pointers = storage.strings.iter().map(|c| c.as_ptr()).collect();
        storage.pointers.push(std::ptr::null()); // LVGL 的终止条件：`map[i] == NULL`
                                                 // SAFETY: 地图已随 `self.map` 一起持有（与底层对象同寿命）；指针数组以 NULL 结尾；
                                                 // 数组堆地址在本次 set 之后不再变动（下次 set 会整体换新并立即重挂）。
        unsafe { sys::lv_buttonmatrix_set_map(self.obj.raw(), storage.pointers.as_ptr()) };
        *self.map.borrow_mut() = Some(storage);
    }

    /// 某个键的文本（离屏断言用；键由地图顺序编号，`"\n"` 不占号）。
    pub fn button_text(&self, index: u32) -> Option<String> {
        if !self.is_alive() {
            return None;
        }
        // SAFETY: 刚校验存活；返回地图里那份 NUL 结尾串（地图由 `self.map` 锚住）。
        let p = unsafe { sys::lv_buttonmatrix_get_button_text(self.obj.raw(), index) };
        if p.is_null() {
            return None;
        }
        // SAFETY: `p` 非空且 NUL 结尾。
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    /// 当前选中键下标（无选中时为 `None` —— LVGL 用 `LV_BUTTONMATRIX_BUTTON_NONE` 表示）。
    pub fn selected(&self) -> Option<u32> {
        if !self.is_alive() {
            return None;
        }
        // SAFETY: 刚校验存活。
        let id = unsafe { sys::lv_buttonmatrix_get_selected_button(self.obj.raw()) };
        if id == BUTTONMATRIX_BUTTON_NONE {
            None
        } else {
            Some(id)
        }
    }

    /// 设选中键下标（配合 [`ButtonMatrix::set_one_checked`] 做单选）。
    pub fn set_selected(&self, index: u32) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_buttonmatrix_set_selected_button(self.obj.raw(), index) };
    }

    /// 是否"同组单选"（`true` 时选中项互斥）。
    pub fn set_one_checked(&self, one_checked: bool) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_buttonmatrix_set_one_checked(self.obj.raw(), one_checked) };
    }

    /// 显式删除（等价于 `drop`）。
    pub fn delete(self) {
        self.obj.delete();
    }
}

impl std::ops::Deref for ButtonMatrix {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self.obj
    }
}

impl std::ops::DerefMut for ButtonMatrix {
    fn deref_mut(&mut self) -> &mut Obj {
        &mut self.obj
    }
}

impl std::fmt::Debug for ButtonMatrix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ButtonMatrix({:?})", self.obj)
    }
}

// ── TabView ──────────────────────────────────────────────────────────────

impl TabView {
    /// 加一页并返回该页容器（**拥有**句柄：`drop` 它即删掉该页）。
    ///
    /// 页内容的"纵向滚动 + 禁横滚"由调用方在返回的容器上套
    /// [`ScrollContainer`]/[`set_scroll_dir`]（设计 §6.1：整页为纵向滚动容器）。
    pub fn add_tab(&self, name: &str) -> Result<Obj, LvglError> {
        if !self.is_alive() {
            return Err(LvglError::NotInitialized);
        }
        let c = to_cstring(name);
        // SAFETY: 刚校验存活；`c` 生命周期覆盖本次调用（LVGL 内部复制页名）。
        let raw = unsafe { sys::lv_tabview_add_tab(self.raw(), c.as_ptr()) };
        Obj::adopt_created(raw, "lv_tabview_add_tab")
    }

    /// 内容容器（**非拥有**句柄，归 tabview 所有）—— 若要走"自建导航栏"路线可在此挂页面。
    pub fn content(&self) -> Result<Obj, LvglError> {
        if !self.is_alive() {
            return Err(LvglError::NotInitialized);
        }
        // SAFETY: 刚校验存活。
        let raw = unsafe { sys::lv_tabview_get_content(self.raw()) };
        Obj::adopt_borrowed(raw)
    }

    /// 标签栏对象（**非拥有**句柄）—— 隐藏内置标签栏的口径：
    /// `tabview.tab_bar()?.set_hidden(true)`（v9.5.0 **不存在** `LV_TAB_POS_NONE`）。
    pub fn tab_bar(&self) -> Result<Obj, LvglError> {
        if !self.is_alive() {
            return Err(LvglError::NotInitialized);
        }
        // SAFETY: 刚校验存活。
        let raw = unsafe { sys::lv_tabview_get_tab_bar(self.raw()) };
        Obj::adopt_borrowed(raw)
    }

    /// 标签栏位置（[`Dir::TOP`] / [`Dir::BOTTOM`] / [`Dir::LEFT`] / [`Dir::RIGHT`]）。
    pub fn set_tab_bar_position(&self, dir: Dir) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_tabview_set_tab_bar_position(self.raw(), dir.raw() as sys::lv_dir_t) };
    }

    /// 标签栏厚度（像素；数值属 `ui/theme.rs`）。
    pub fn set_tab_bar_size(&self, size: i32) {
        if !self.is_alive() {
            return;
        }
        // SAFETY: 刚校验存活。
        unsafe { sys::lv_tabview_set_tab_bar_size(self.raw(), size) };
    }
}

// ── MsgBox ───────────────────────────────────────────────────────────────

/// 确认弹层底座（`lv_msgbox`，设计 §5.6 TT-09 / §5.7 确认对话框行）。
///
/// **`level` 分级、二次确认强度、明细/前后值列表、默认焦点策略都在 B 的
/// `ui/components.rs`**；本类型只给底座能力：标题 / 正文 / 底部按钮 / 内容区 / 关闭。
///
/// # 为什么手写（不用 `widget_handle!`）
///
/// 自动遮罩版（[`MsgBox::modal`]）的删除语义是 **`lv_msgbox_close`** 而不是
/// `lv_obj_delete`：`lv_msgbox_create(NULL)` 会额外建一个全屏 backdrop 作为父对象并置
/// `LV_MSGBOX_FLAG_AUTO_PARENT`，关闭时必须连带删掉 backdrop（v9.5.0
/// `src/widgets/msgbox/lv_msgbox.c`）。`Drop` 走正确路径即可（对象删除后 [`Obj`]
/// 的存活探针已置假 ⇒ 随后的 `Obj::drop` 自动 no-op，不 double free）。
pub struct MsgBox {
    obj: Obj,
}

impl MsgBox {
    /// 在指定父对象下建弹层（父对象常取 [`layer_top`]）。
    pub fn create(parent: &Obj) -> Result<Self, LvglError> {
        Ok(Self {
            obj: create_child(parent, "lv_msgbox_create", sys::lv_msgbox_create)?,
        })
    }

    /// 建**模态**弹层：`lv_msgbox_create(NULL)` —— LVGL 自动把它放在顶层图层上并铺一层
    /// 全屏 backdrop（遮罩感即来自该 backdrop，设计 §5.7「遮罩用 `lv_obj_set_style_bg_opa`」
    /// 的样式施加点也在它上面）。
    pub fn modal() -> Result<Self, LvglError> {
        if !super::is_initialized() {
            return Err(LvglError::NotInitialized);
        }
        // SAFETY: 已 `init()`；`parent = NULL` 是 LVGL 明确定义的"自动挂顶层"入口。
        let raw = unsafe { sys::lv_msgbox_create(std::ptr::null_mut()) };
        Ok(Self {
            obj: Obj::adopt_created(raw, "lv_msgbox_create(NULL)")?,
        })
    }

    /// 底层对象（只读）。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 弹层是否仍存活（`close` / `drop` / 父对象级联删除之后为 `false`）。
    pub fn is_alive(&self) -> bool {
        self.obj.is_alive()
    }

    /// 设 / 改标题文本，返回标题对象句柄。
    ///
    /// # ⚠️ 返回的是**非拥有**句柄（**Critical 修正**）
    ///
    /// `lv_msgbox_add_title` 的名字有误导性：标题对象**只在首次为 NULL 时创建，之后恒返回
    /// 同一个缓存对象**（v9.5.0 `src/widgets/msgbox/lv_msgbox.c:163-172`：
    /// `if(mbox->title == NULL) { … lv_label_create … }` 之后 `lv_label_set_text(mbox->title, …);
    /// return mbox->title;`）。因此**不能**把它收成拥有句柄：`drop` 会 `lv_obj_delete(title)`
    /// 删除该对象，而 `mbox->title` **仍指向已释放块** ⇒ 再次 `add_title` 即在悬垂指针上
    /// `lv_label_set_text` = **UAF**（评审探针实测：>60 s 挂死，`lv_obj_get_display: No screen found`）。
    ///
    /// 故本方法恒走 [`Obj::adopt_borrowed`]（与 [`Obj::screen`] / [`layer_top`] 同口径）：
    /// `drop` 不删除标题；重复调用返回**同一底层对象**（只更新文本）。标题归 msgbox 所有，
    /// 随 [`MsgBox::close`] / `drop` 一并销毁 —— 调用方无需（也不得）单独删它。
    ///
    /// 需要给标题挂样式（字号 / 颜色）或读回底层对象时用返回的借用句柄。
    pub fn add_title(&self, title: &str) -> Result<Obj, LvglError> {
        if !self.is_alive() {
            return Err(LvglError::NotInitialized);
        }
        let c = to_cstring(title);
        // SAFETY: 刚校验存活；`c` 生命周期覆盖本次调用（LVGL 内部复制文本）。
        let raw = unsafe { sys::lv_msgbox_add_title(self.obj.raw(), c.as_ptr()) };
        Obj::adopt_borrowed(raw)
    }

    /// 加正文（返回正文标签的**拥有**句柄；`drop` 它即删掉该段正文）。
    ///
    /// 与 [`MsgBox::add_title`] 不同：`lv_msgbox_add_text` **每次都新建**一个 label
    /// （v9.5.0 `lv_msgbox.c:199`：`lv_label_create(mbox->content)`），故返回拥有句柄正确 ——
    /// 每次调用确实产出一个新的、归调用方回收的对象。
    pub fn add_text(&self, text: &str) -> Result<Obj, LvglError> {
        self.add_child(text, sys::lv_msgbox_add_text, "lv_msgbox_add_text")
    }

    /// 在底部加一个按钮（返回的 [`Button`] 可直接挂 `on_clicked`；`drop` 它即删掉按钮）。
    pub fn add_footer_button(&self, text: &str) -> Result<Button, LvglError> {
        if !self.is_alive() {
            return Err(LvglError::NotInitialized);
        }
        let c = to_cstring(text);
        // SAFETY: 刚校验存活；`c` 生命周期覆盖本次调用（LVGL 内部复制按钮文字）。
        let raw = unsafe { sys::lv_msgbox_add_footer_button(self.obj.raw(), c.as_ptr()) };
        Ok(Button {
            obj: Obj::adopt_created(raw, "lv_msgbox_add_footer_button")?,
        })
    }

    /// 内容容器（**非拥有**句柄：标题/正文就挂在这里；给 B 需要自定义内容时用）。
    pub fn content(&self) -> Result<Obj, LvglError> {
        if !self.is_alive() {
            return Err(LvglError::NotInitialized);
        }
        // SAFETY: 刚校验存活。
        let raw = unsafe { sys::lv_msgbox_get_content(self.obj.raw()) };
        Obj::adopt_borrowed(raw)
    }

    /// 关闭并销毁弹层（含自动遮罩）。等价于 `drop`。
    pub fn close(self) {
        // 语义即 drop：`MsgBox::drop` 调 `lv_msgbox_close`（自动遮罩的父级一并删除）。
    }

    /// "每次新建对象"的 add_* 的公共形状（文本 + 返回**拥有**句柄）。
    ///
    /// **只准用于 C 侧"每次调用都新建对象"的入口**；缓存单例（如 `lv_msgbox_add_title`）
    /// 必须走 [`Obj::adopt_borrowed`] —— 否则 `drop` 删掉的对象仍被控件字段引用（UAF）。
    fn add_child(
        &self,
        text: &str,
        add: unsafe extern "C" fn(*mut sys::lv_obj_t, *const c_char) -> *mut sys::lv_obj_t,
        what: &'static str,
    ) -> Result<Obj, LvglError> {
        if !self.is_alive() {
            return Err(LvglError::NotInitialized);
        }
        let c = to_cstring(text);
        // SAFETY: 刚校验存活；`c` 生命周期覆盖本次调用（LVGL 内部复制文本）。
        let raw = unsafe { add(self.obj.raw(), c.as_ptr()) };
        Obj::adopt_created(raw, what)
    }
}

impl Drop for MsgBox {
    fn drop(&mut self) {
        if self.obj.is_alive() {
            // SAFETY: 刚校验存活。`lv_msgbox_close` 是**正确的销毁路径**（自动遮罩版会
            // 连同 backdrop 一起删；普通版只删自己）；删除后存活探针置假，随后
            // `Obj::drop`（字段析构）自动 no-op —— 不会 double free。
            unsafe { sys::lv_msgbox_close(self.obj.raw()) };
        }
    }
}

impl std::fmt::Debug for MsgBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MsgBox({:?})", self.obj)
    }
}

// ── TextButton（`lv_button` + `lv_label` 组合底座）────────────────────────

/// 带文字的按钮 = [`Button`] + 子 [`Label`]。
///
/// 这是设计 §5.6 F12 / §5.7 点名的**组合底座**：通用按钮、步进器（`−` / 值 / `＋`）、
/// IPv4 四段、日期时间五段全部由它搭（**不是**组件库里的成品组件 —— 步进器的越界禁用、
/// 防抖等语义仍在 B 的 `ui/components.rs`）。
///
/// `Deref` 到按钮的 [`Obj`] ⇒ `set_size` / `add_style` / `on_clicked` 直接可用。
pub struct TextButton {
    button: Button,
    label: Label,
}

impl TextButton {
    /// 建按钮并挂一个字标签。
    pub fn create(parent: &Obj, text: &str) -> Result<Self, LvglError> {
        let button = Button::create(parent)?;
        let label = Label::create_with_text(&button, text)?;
        Ok(Self { button, label })
    }

    /// 改文字。
    pub fn set_text(&self, text: &str) {
        self.label.set_text(text);
    }

    /// 当前文字。
    pub fn text(&self) -> Option<String> {
        self.label.text()
    }

    /// 按钮本体（**安全**版事件注册入口：`button.on_clicked(..)`）。
    pub fn button(&self) -> &Button {
        &self.button
    }

    /// 文字标签（单独调字号/颜色时用；外观仍应来自 `ui/theme.rs` 的 [`super::style::Style`]）。
    pub fn label(&self) -> &Label {
        &self.label
    }

    /// 设可选（`LV_STATE_CHECKED`）—— 多选 chip 用。
    pub fn set_checkable(&self, checkable: bool) {
        if checkable {
            self.button.add_flag(ObjFlag::CHECKABLE);
        } else {
            self.button.remove_flag(ObjFlag::CHECKABLE);
        }
    }

    /// 设勾选态（配合 [`TextButton::set_checkable`]）。
    pub fn set_checked(&self, checked: bool) {
        set_state(&self.button, State::CHECKED, checked);
    }

    /// 是否勾选。
    pub fn is_checked(&self) -> bool {
        has_state(&self.button, State::CHECKED)
    }

    /// 设禁用视觉/交互（TT-10 防重、§6.2 步进器越界）—— 语义与
    /// [`set_state`]`(self, `[`State::DISABLED`]`, true)` 相同，此处给按钮一个直白入口。
    pub fn set_disabled(&self, disabled: bool) {
        set_state(&self.button, State::DISABLED, disabled);
    }
}

impl std::ops::Deref for TextButton {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        self.button.obj()
    }
}

impl std::ops::DerefMut for TextButton {
    fn deref_mut(&mut self) -> &mut Obj {
        // `Button` 只有 `DerefMut` 到 `Obj`（字段私有）；这里用同一实现取可变引用。
        &mut self.button
    }
}

impl std::fmt::Debug for TextButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextButton({:?})", self.button)
    }
}

// ── ScrollContainer ──────────────────────────────────────────────────────

/// 纵向滚动容器（设计 §5.6「滚动与滚动指示」/ §6.1「整页为 LVGL 纵向滚动容器」）。
///
/// 构造即落三条结构性约束：
/// 1. [`ObjFlag::SCROLLABLE`] —— 内容拖拽 + 惯性（阈值由 `lv_conf.h` 的
///    `LV_INDEV_DEF_SCROLL_LIMIT` / `SCROLL_THROW` 定）；
/// 2. [`set_scroll_dir`]`=`[`Dir::VER`]` —— **结构性禁止横滚**（横轴根本不参与拖拽）；
/// 3. [`set_scrollbar_mode`]`=`[`ScrollMode::AUTO`]` —— 滚动条**纯指示**，内容超出才出现
///    （不可拖，见模块文档 §滚动条口径）。
///
/// 尺寸 / 内边距 / 底色一律由调用方经 `Deref` 到 [`Obj`] 设定。
pub struct ScrollContainer {
    obj: Obj,
}

impl ScrollContainer {
    /// 建容器（在 `parent` 下）。
    pub fn create(parent: &Obj) -> Result<Self, LvglError> {
        let obj = Obj::create(parent)?;
        obj.add_flag(ObjFlag::SCROLLABLE);
        set_scroll_dir(&obj, Dir::VER);
        set_scrollbar_mode(&obj, ScrollMode::AUTO);
        Ok(Self { obj })
    }

    /// 底层对象（只读）。
    pub fn obj(&self) -> &Obj {
        &self.obj
    }

    /// 当前滚动方向（离屏断言口径）。
    pub fn scroll_dir(&self) -> Dir {
        scroll_dir(&self.obj)
    }

    /// 当前滚动条模式（离屏断言口径）。
    pub fn scrollbar_mode(&self) -> ScrollMode {
        scrollbar_mode(&self.obj)
    }
}

impl std::ops::Deref for ScrollContainer {
    type Target = Obj;
    fn deref(&self) -> &Obj {
        &self.obj
    }
}

impl std::ops::DerefMut for ScrollContainer {
    fn deref_mut(&mut self) -> &mut Obj {
        &mut self.obj
    }
}

impl std::fmt::Debug for ScrollContainer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ScrollContainer({:?})", self.obj)
    }
}

// ── Group（输入组 / 默认焦点）───────────────────────────────────────────

/// 输入组（`lv_group`）—— TT-09「把『取消』设为**默认聚焦对象**」的落点。
///
/// # 口径（已按 v9.5.0 源码核实）
///
/// - [`Group::focus`] 会把 `LV_STATE_FOCUSED` 加到该对象上（`LV_EVENT_FOCUSED` 的默认
///   处理，`src/core/lv_obj.c:989`），主题据此给"默认焦点"外观；对象**必须先
///   `Group::add` 进组**（`lv_group_focus_obj` 对不在组内的对象直接返回）。
/// - 本机是**指针类** indev（`LV_INDEV_TYPE_POINTER`），而
///   `lv_indev_set_group()` 在 v9.5.0 **只对 KEYPAD / ENCODER 生效**
///   （`src/indev/lv_indev.c`：`if(indev->type == LV_INDEV_TYPE_KEYPAD || ENCODER)`）；
///   故本层**不提供** indev 绑定（无键导航，绑了也是空操作），组的作用就是**焦点状态**。
/// - `Drop` → `lv_group_delete`：它会自行把焦点对象去焦、把自己从所有 indev 上摘掉
///   （v9.5.0 源码），故不存在"组先死、indev 仍引用"的悬垂。
pub struct Group {
    raw: *mut sys::lv_group_t,
    /// 创建时的 LVGL 世代（`lv_deinit()` 内部会 `lv_group_deinit()` 清空组链表 —— 上一世代
    /// 的组指针届时已失效，`Drop` 必须据此拒绝再删，防 double free）。
    generation: u64,
}

impl Group {
    /// 建组（失败见 [`LvglError`]）。
    pub fn create() -> Result<Self, LvglError> {
        if !super::is_initialized() {
            return Err(LvglError::NotInitialized);
        }
        // SAFETY: 已 `init()`（`lv_group_init` 亦由 `lv_init` 完成）。
        let raw = unsafe { sys::lv_group_create() };
        if raw.is_null() {
            return Err(LvglError::OutOfMemory("lv_group_create"));
        }
        Ok(Self {
            raw,
            generation: super::generation(),
        })
    }

    /// 本组是否仍存活（LVGL 已初始化 **且** 世代未变）。
    pub fn is_alive(&self) -> bool {
        super::is_initialized() && self.generation == super::generation()
    }

    /// 把对象加入本组（对象失效时 no-op）。
    pub fn add(&self, obj: &Obj) {
        if !self.is_alive() || !obj.is_alive() {
            return;
        }
        // SAFETY: 组与对象均存活。
        unsafe { sys::lv_group_add_obj(self.raw, obj.raw()) };
    }

    /// 把对象**从本组**移出（对象不属于本组时 no-op）。
    ///
    /// # 为什么先校验归属
    ///
    /// `lv_group_remove_obj(obj)` **只吃对象指针**：它按对象自查其所在组再摘除。若对象实际
    /// 属于**别的组**，直接调用会摘掉**它自己的组**——与"从本组移除"的语义不符（`&self`
    /// 承诺的是本组）。故先用 `lv_obj_get_group(obj)` 校验归属；一致才摘除。
    pub fn remove(&self, obj: &Obj) {
        if !self.is_alive() || !obj.is_alive() {
            return;
        }
        // SAFETY: 组与对象均存活。
        if unsafe { sys::lv_obj_get_group(obj.raw()) } != self.raw {
            return; // 不属于本组 ⇒ 不动它（含"属于别的组"的情形）
        }
        // SAFETY: 已确认对象确属本组，`lv_group_remove_obj` 会摘除本组持有的该项。
        unsafe { sys::lv_group_remove_obj(obj.raw()) };
    }

    /// 设为**默认聚焦对象**（TT-09：确认弹层打开时焦点落在「取消」上）。对象须已入组。
    pub fn focus(&self, obj: &Obj) {
        if !self.is_alive() || !obj.is_alive() {
            return;
        }
        // SAFETY: 组与对象均存活；不在组内时 LVGL 自行 no-op。
        unsafe { sys::lv_group_focus_obj(obj.raw()) };
    }
}

impl Drop for Group {
    fn drop(&mut self) {
        if self.is_alive() {
            // SAFETY: 仍存活（已初始化且世代未变）⇒ 该组尚未被 `lv_group_deinit` 清掉。
            unsafe { sys::lv_group_delete(self.raw) };
        }
    }
}

impl std::fmt::Debug for Group {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Group")
            .field("alive", &self.is_alive())
            .finish()
    }
}
