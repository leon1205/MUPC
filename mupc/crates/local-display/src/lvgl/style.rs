//! 样式的**机制层**（工作单元 A2；设计 §5.1 / §5.6 控件映射表）。
//!
//! # 本模块的边界（**硬性**，评审检查项）
//!
//! 这里只有**机制**，共三件事：
//!
//! 1. 类型化取值：[`Color`] / [`Opa`] / [`Part`] / [`State`] / [`BorderSide`] /
//!    [`StyleSelector`]（把 C 侧的开放整数集收成有名字的类型）；
//! 2. 样式构造：[`Style`] = 一个 `lv_style_t` 的所有者 + `lv_style_set_*` 的类型化 setter；
//! 3. 挂载：[`super::obj::Obj::add_style`] / [`super::obj::Obj::remove_style`]。
//!
//! **不含本项目的任何 UI 规格值**（色板 HEX / 字号档 / 圆角 / 内边距 / 间距）——那些是
//! `ui/theme.rs` 的**单一真源**（设计 §5.1：「控件外观必须来自 `ui/theme.rs`，页面内**不得**
//! 硬编码裸色值/裸尺寸」）。因此本模块**刻意没有** `defaults` 一类的"内置色板"：
//! [`Style::new`] 返回**空样式**（不设任何属性），外观 100% 由调用方决定。
//! 唯一的例外是 [`Color::hex`] —— 它只是"写色值"的**便利构造**，不预设任何色值。
//!
//! # 状态色怎么表达（LVGL v9 与 v8 的关键差异）
//!
//! v9 的 `lv_style_set_*` **没有 selector 参数**：一个 `lv_style_t` 只描述"**一组属性**在
//! **某一组 (part, state)** 下的取值"。UI §5.2 的「状态 × 色值矩阵」因此表达为
//! **多个 `Style` + 挂载时的选择器**：
//!
//! ```
//! # use std::rc::Rc;
//! # use mupc_local_display::lvgl::style::{Color, Opa, State, Style, StyleSelector};
//! // 色值/圆角/内边距一律由 ui/theme.rs 给定 —— 下面是**占位示例**，不是规格值。
//! let mut pressed = Style::new();          // 先在 `Rc` 之前配置（共享后即冻结）
//! pressed.set_bg_color(Color::rgb(0x12, 0x34, 0x56));
//! pressed.set_bg_opa(Opa::COVER);
//! let pressed = Rc::new(pressed);
//! // obj.add_style(&pressed, StyleSelector::state_of(State::PRESSED));
//! ```
//!
//! `danger` 不是 LVGL 状态（UI §5.2 已注明），故它是**另一套** `Style`（主题里另建），
//! 与本模块的机制无关。
//!
//! # 生命周期纪律（与 A1 的句柄纪律一致）
//!
//! `lv_style_t` 的属性表存在 **LVGL 堆**上（首次 `lv_style_set_*` 即 `lv_malloc`），故
//! [`Style`] 是**所有者**：
//!
//! - `Drop` → `lv_style_reset()` 释放属性表；`lv_style_t` 本体在 Rust 堆（`Box`）；
//! - `init → deinit → init` 之后靠**世代令牌**（[`super::generation`]）识别"底层堆已随
//!   `lv_deinit()` 释放（`lv_mem_deinit`）"，此时**不得**再 reset/gcc set（否则触已释放内存）；
//! - 样式被 [`super::obj::Obj::add_style`] 挂到对象上时，LVGL 只存**裸指针**
//!   （`vendor/lvgl/src/core/lv_obj_style.c`：`obj->styles[i].style = style;`）。
//!   本层用**两条**机制把"样式必须活过引用它的对象"从调用方义务变成不变量：
//!   1. **地址稳定**：`lv_style_t` 装在 `Box` 里 ⇒ 移动 [`Style`]（含 `fn theme() -> Style`
//!      返回即移动）不改变 LVGL 存下的那个地址；
//!   2. **共享所有权**：挂载入口是 [`super::obj::Obj::add_style`]`(&Rc<Style>, ..)`，
//!      `Obj`（准确地说是锚在**底层 LVGL 对象**事件项上的 keeper）持一份 `Rc` 克隆 ⇒
//!      样式至少活到**底层对象**被删除之后；调用方即使提前 drop 句柄也不会悬垂。
//!
//! 因此 `Style` 一旦经由 `Rc` 共享即**不可再变**（`&mut` 仅存在于 `Rc::new` 之前）——
//! 这正是"挂上即冻结"的语义（见 `ui/theme.rs` 的构造方式）。
//!
//! 与 A1 同口径：未 `init()` 或处于上一世代时，全部 setter 是 **no-op**（不 panic、不触
//! 已释放内存），而不是返回错误。

use lvgl_sys as sys;

/// RGB 颜色（LVGL `lv_color_t` 的无损镜像）。
///
/// 构造是**纯 safe**（`ui/theme.rs` 不需要 `unsafe`）；字段按 UI 设计文档的 `#RRGGBB` 次序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    /// 红（0–255）。
    pub r: u8,
    /// 绿（0–255）。
    pub g: u8,
    /// 蓝（0–255）。
    pub b: u8,
}

impl Color {
    /// 由分量构造。
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// 由 `0xRRGGBB` 构造 —— 与 UI 设计文档色板的 `#RRGGBB` 写法**逐位对应**，
    /// 便于 `ui/theme.rs` 逐条抄录（本函数不预设任何色值）。
    pub const fn hex(rgb: u32) -> Self {
        Self {
            r: (rgb >> 16) as u8,
            g: (rgb >> 8) as u8,
            b: rgb as u8,
        }
    }

    /// 转 C 侧结构（内存序 `B, G, R` —— `LV_COLOR_DEPTH 32` 的 XRGB8888）。
    ///
    /// `pub(crate)`：`widgets.rs` 里直接吃 `lv_color_t` 的 C 接口（`lv_led_set_color`）
    /// 需要它 —— 让"内存序"这条知识仍只在本文件里一份。
    pub(crate) fn to_sys(self) -> sys::lv_color_t {
        sys::lv_color_t {
            blue: self.b,
            green: self.g,
            red: self.r,
        }
    }
}

/// 不透明度（`lv_opa_t` 的镜像）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Opa(u8);

impl Opa {
    /// 全透明。
    pub const TRANSPARENT: Self = Self(0);
    /// 不透明（`LV_OPA_COVER`）。
    pub const COVER: Self = Self(255);

    /// 由原始 0–255 构造。
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// 由百分比构造（`>100` 收敛到 `100`）。
    pub const fn percent(percent: u8) -> Self {
        let p = if percent > 100 { 100u32 } else { percent as u32 };
        Self((p * 255 / 100) as u8)
    }

    /// 原始 0–255。
    pub const fn raw(self) -> u8 {
        self.0
    }
}

/// 样式部件（`LV_PART_*` 的镜像；与 [`State`] 一起构成 [`StyleSelector`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Part(u32);

impl Part {
    /// 主体（背景/文字所在部件）。
    pub const MAIN: Self = Self(sys::LV_PART_MAIN as u32);
    /// 滚动条部件（UI §5.6 的"纯指示"滚动条宽度即 8 px 挂此部件）。
    pub const SCROLLBAR: Self = Self(sys::LV_PART_SCROLLBAR as u32);
    /// 指示器部件（`lv_bar` / `lv_switch` 的"已选"段）。
    pub const INDICATOR: Self = Self(sys::LV_PART_INDICATOR as u32);
    /// 旋钮部件（`lv_switch` 的滑块）。取值须与 C 端 `LV_PART_KNOB` 一致（`0x030000`）。
    pub const KNOB: Self = Self(sys::LV_PART_KNOB as u32);
    /// 选中项部件（`lv_dropdown` 的当前选项 / `lv_buttonmatrix` 的选中段）。
    /// 取值须与 C 端 `LV_PART_SELECTED` 一致（`0x040000`）。
    pub const SELECTED: Self = Self(sys::LV_PART_SELECTED as u32);
    /// 子项部件（`lv_buttonmatrix` 的段）。
    pub const ITEMS: Self = Self(sys::LV_PART_ITEMS as u32);
    /// 通配（仅用于 [`super::obj::Obj::remove_style`] 一类"匹配全部"的场合）。
    pub const ANY: Self = Self(sys::LV_PART_ANY as u32);

    /// 原始 C 取值。
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// 组合两个部件（C 侧本就是位域，`|` 即"并集"）。
    pub const fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for Part {
    type Output = Part;
    fn bitor(self, rhs: Part) -> Part {
        self.or(rhs)
    }
}

/// 样式状态（`LV_STATE_*` 的镜像）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct State(u32);

impl State {
    /// 默认态（**不是**"任何态"）。
    pub const DEFAULT: Self = Self(sys::LV_STATE_DEFAULT as u32);
    /// 按下（UI §5.2「pressed ≤100 ms 反馈」）。
    pub const PRESSED: Self = Self(sys::LV_STATE_PRESSED as u32);
    /// 选中（`LV_OBJ_FLAG_CHECKABLE` 控件自动维护）。
    pub const CHECKED: Self = Self(sys::LV_STATE_CHECKED as u32);
    /// 聚焦（确认弹层的"默认焦点「取消」"）。
    pub const FOCUSED: Self = Self(sys::LV_STATE_FOCUSED as u32);
    /// 禁用。
    pub const DISABLED: Self = Self(sys::LV_STATE_DISABLED as u32);
    /// 滚动中（滚动条 thumb 变色用）。
    pub const SCROLLED: Self = Self(sys::LV_STATE_SCROLLED as u32);
    /// 通配（仅用于"匹配全部态"的场合）。
    pub const ANY: Self = Self(sys::LV_STATE_ANY as u32);

    /// 原始 C 取值。
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// 组合两个状态（C 侧是位域，`|` 即"这些态下都生效"）。
    pub const fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for State {
    type Output = State;
    fn bitor(self, rhs: State) -> Self {
        self.or(rhs)
    }
}

/// 描边侧（`LV_BORDER_SIDE_*` 的镜像）——UI §3.5 的"卡顶 / 卡左缘 3 px 警示描边"即单侧描边。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BorderSide(u32);

impl BorderSide {
    /// 不描边。
    pub const NONE: Self = Self(sys::LV_BORDER_SIDE_NONE as u32);
    /// 上缘。
    pub const TOP: Self = Self(sys::LV_BORDER_SIDE_TOP as u32);
    /// 下缘。
    pub const BOTTOM: Self = Self(sys::LV_BORDER_SIDE_BOTTOM as u32);
    /// 左缘。
    pub const LEFT: Self = Self(sys::LV_BORDER_SIDE_LEFT as u32);
    /// 右缘。
    pub const RIGHT: Self = Self(sys::LV_BORDER_SIDE_RIGHT as u32);
    /// 四边（默认）。
    pub const FULL: Self = Self(sys::LV_BORDER_SIDE_FULL as u32);

    /// 原始 C 取值。
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// 样式挂载选择器（`lv_style_selector_t` = 部件 | 状态）。
///
/// 单一真源是"部件 + 状态"两个**命名**取值，`|` 的组合关系由本类型掌握，
/// 调用方不再手写位运算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleSelector {
    part: Part,
    state: State,
}

impl StyleSelector {
    /// 主部件 + 默认态（C 侧 `LV_PART_MAIN | LV_STATE_DEFAULT` = 0）—— 最常用。
    pub const fn main() -> Self {
        Self {
            part: Part::MAIN,
            state: State::DEFAULT,
        }
    }

    /// 主部件 + 指定态。
    pub const fn state_of(state: State) -> Self {
        Self {
            part: Part::MAIN,
            state,
        }
    }

    /// 指定部件 + 默认态（如滚动条部件 `LV_PART_SCROLLBAR`）。
    pub const fn part_of(part: Part) -> Self {
        Self {
            part,
            state: State::DEFAULT,
        }
    }

    /// 指定部件 + 指定态。
    pub const fn new(part: Part, state: State) -> Self {
        Self { part, state }
    }

    /// 换部件（保留状态）。
    pub const fn part(self, part: Part) -> Self {
        Self { part, ..self }
    }

    /// 换状态（保留部件）。
    pub const fn state(self, state: State) -> Self {
        Self { state, ..self }
    }

    /// 转 C 侧选择器（`lv_style_selector_t`）。
    pub(crate) const fn to_sys(self) -> sys::lv_style_selector_t {
        self.part.raw() | self.state.raw()
    }
}

/// 一个 LVGL 样式（`lv_style_t` 的所有者）。
///
/// 见模块文档「生命周期纪律」：`lv_style_t` 本体在 **Rust 堆的 `Box`** 里（地址不随
/// [`Style`] 移动而变），属性表在 LVGL 堆；挂载走
/// [`super::obj::Obj::add_style`]`(&Rc<Style>, ..)`，由 `Obj` 持共享所有权。
/// 未 `init()` 或处于上一世代时全部 setter 为 no-op。
pub struct Style {
    /// **`Box` 是刻意的**：LVGL 长期保存 `&*raw` 这个地址（`obj->styles[i].style`），
    /// 若把 `lv_style_t` 内联在 `Style` 里，`let s2 = s;` / `fn theme() -> Style` 一移动
    /// 就会让 LVGL 读到旧地址（UB）。装箱后地址在样式存活期内恒定。
    raw: Box<sys::lv_style_t>,
    /// 创建时的 LVGL 世代（`init → deinit → init` 后底层堆已被 `lv_mem_deinit` 释放）。
    generation: u64,
}

impl Style {
    /// 建一个**空样式**（不设任何属性；外观由调用方决定）。
    ///
    /// 与 [`super::init`] 前后顺序无关：`lv_style_init()` 只做内存清零、不分配 LVGL 堆；
    /// 真正的堆分配发生在第一次 `lv_style_set_*`，那时若不在有效世代则整条设置链为 no-op。
    /// 反过来，**未 `init()` 时创建的样式其世代永不匹配**（见 [`Style::is_live`]）⇒ 它不会
    /// 被任何挂载入口接受（请先 `init()` 再建样式，`ui/theme.rs` 即此顺序）。
    pub fn new() -> Self {
        // SAFETY: `lv_style_t` 全零是合法位型（一个裸指针 + 两个整数），且 `lv_style_init()`
        // 紧接着按 LVGL 契约把它初始化（内部 `lv_memzero`）。
        let mut raw: Box<sys::lv_style_t> = Box::new(unsafe { std::mem::zeroed() });
        // SAFETY: `raw` 是本函数私有、按 `lv_style_t` 布局分配的堆上对象；`Box` 的地址在
        // `Style` 移动时不变，故此后 LVGL 存下的指针始终有效。
        unsafe { sys::lv_style_init(&mut *raw) };
        Self {
            raw,
            generation: super::generation(),
        }
    }

    /// 本样式是否仍可安全设值 / 被挂载 / 释放（LVGL 已初始化 **且** 世代未变）。
    ///
    /// `pub(crate)`：挂载入口 [`super::obj::Obj::add_style`] / `remove_style` 必须据此
    /// 拒绝"属性表已随 `lv_deinit` 释放"的旧世代样式（否则挂上去 = 悬垂属性表）。
    pub(crate) fn is_live(&self) -> bool {
        super::is_initialized() && self.generation == super::generation()
    }

    /// 原始样式指针（薄层内部：`obj.rs` 挂载样式用）。
    ///
    /// 指向 `Box` 内的 `lv_style_t` ⇒ **地址在 [`Style`] 存活期内恒定**（移动不改变它）。
    pub(crate) fn raw(&self) -> *const sys::lv_style_t {
        &*self.raw as *const sys::lv_style_t
    }

    /// 背景色。
    pub fn set_bg_color(&mut self, color: Color) {
        if !self.is_live() {
            return;
        }
        // SAFETY: `self.raw` 已 `lv_style_init` 且世代未变（LVGL 堆存活）。
        unsafe { sys::lv_style_set_bg_color(&mut *self.raw, color.to_sys()) };
    }

    /// 背景不透明度。
    pub fn set_bg_opa(&mut self, opa: Opa) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_bg_opa(&mut *self.raw, opa.raw()) };
    }

    /// 宽度（像素；UI §5.6-A 的滚动条 8 px 宽度即挂 [`Part::SCROLLBAR`] 用本 setter 设）。
    ///
    /// 对应 `lv_style_set_width`。对对象本身而言，本属性参与**布局趟**的尺寸解算：
    /// 挂在 `MAIN` 上会决定对象宽度（不再取内容宽度），挂在 `SCROLLBAR` 上决定滚动条厚度。
    pub fn set_width(&mut self, width: i32) {
        if !self.is_live() {
            return;
        }
        // SAFETY: `self.raw` 已 `lv_style_init` 且世代未变（LVGL 堆存活）。
        unsafe { sys::lv_style_set_width(&mut *self.raw, width) };
    }

    /// 圆角（UI §3.5：卡片 8 / 控件 6 / 胶囊用极大值）。
    pub fn set_radius(&mut self, radius: i32) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_radius(&mut *self.raw, radius) };
    }

    /// 四向内边距。
    ///
    /// C 侧的 `lv_style_set_pad_all()` 是**头文件里的 `static inline`**（不入绑定），
    /// 故此处按四个方向展开 —— 语义与之一致。
    pub fn set_pad_all(&mut self, pad: i32) {
        self.set_pad_top(pad);
        self.set_pad_bottom(pad);
        self.set_pad_left(pad);
        self.set_pad_right(pad);
    }

    /// 上内边距。
    pub fn set_pad_top(&mut self, pad: i32) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_pad_top(&mut *self.raw, pad) };
    }

    /// 下内边距。
    pub fn set_pad_bottom(&mut self, pad: i32) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_pad_bottom(&mut *self.raw, pad) };
    }

    /// 左内边距。
    pub fn set_pad_left(&mut self, pad: i32) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_pad_left(&mut *self.raw, pad) };
    }

    /// 右内边距。
    pub fn set_pad_right(&mut self, pad: i32) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_pad_right(&mut *self.raw, pad) };
    }

    /// 描边宽度（UI §3.5：卡片 1 px / 值越界或危险操作 3 px）。
    pub fn set_border_width(&mut self, width: i32) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_border_width(&mut *self.raw, width) };
    }

    /// 描边色。
    pub fn set_border_color(&mut self, color: Color) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_border_color(&mut *self.raw, color.to_sys()) };
    }

    /// 描边不透明度。
    pub fn set_border_opa(&mut self, opa: Opa) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_border_opa(&mut *self.raw, opa.raw()) };
    }

    /// 描边侧（默认四边；单侧用于"卡顶/卡左缘"警示描边）。
    pub fn set_border_side(&mut self, side: BorderSide) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_border_side(&mut *self.raw, side.raw() as sys::lv_border_side_t) };
    }

    /// 文字色。
    pub fn set_text_color(&mut self, color: Color) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上。
        unsafe { sys::lv_style_set_text_color(&mut *self.raw, color.to_sys()) };
    }

    /// 文字字体（档位取自 [`super::font::Font`]，**不得**由页面硬编码字体引用）。
    pub fn set_text_font(&mut self, font: &super::font::Font) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 同上；`font.raw()` 指向 LVGL 拥有的静态字体数据（生命周期 = 进程）。
        unsafe { sys::lv_style_set_text_font(&mut *self.raw, font.raw()) };
    }
}

impl Default for Style {
    /// 等价于 [`Style::new`]（空样式）。
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Style {
    fn drop(&mut self) {
        // 未 `init()` 或处于上一世代 ⇒ 属性表所在堆已随 `lv_deinit()`（`lv_mem_deinit`）
        // 释放，不能再 `lv_style_reset`（否则触已释放内存）。
        //
        // `Drop` 只在本样式**最后一份 `Rc`** 被释放时运行 —— 而挂载方（`Obj` 的事件项
        // keeper）持有一份 `Rc` 直到**底层 LVGL 对象**被删除 ⇒ "仍被引用的样式先 drop"
        // 这条路在安全 API 下不可达（`tests_a2.rs` 场景 ⑱ 断言）。
        if self.is_live() {
            // SAFETY: 仍存活（已初始化且世代未变）⇒ `self.raw` 已 init、属性表未被释放。
            // 解引用释放的是 LVGL 堆上的属性表；`Box` 本体随后由 Rust 释放（不泄漏）。
            unsafe { sys::lv_style_reset(&mut *self.raw) };
        }
    }
}

impl std::fmt::Debug for Style {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Style").field("alive", &self.is_live()).finish()
    }
}
