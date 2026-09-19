//! 显示后端（工作单元 A1；设计 §1.1.1.1 **P-1**：自研 `lv_display` + PARTIAL 双缓冲 + 自研 flush 桥）。
//!
//! # 路径选择（为什么不用 LVGL 官方 fbdev 驱动）
//!
//! P-1 让**像素格式与双缓冲策略完全自控**，且离屏测试与真机**共用同一条 flush 路径**
//! （只有 sink 不同：`Vec<u8>` vs `/dev/fb0`）—— 这是本模块最强的可测性支点（设计 §11.2）。
//!
//! # flush 桥的纪律（评审检查项）
//!
//! [`Display::set_flush_cb`] 传入的闭包由 LVGL 在 `lv_timer_handler()` 内回调，
//! **只做像素搬运**：不得阻塞、不得做通道 I/O、不得再调 LVGL API
//! （设计 §1.1.1.1 / §5.2 不变量 3）。需要的外部状态（fb 句柄、内存缓冲）由闭包捕获。
//!
//! # tick 与渲染时机
//!
//! tick 由 [`super::init`] 挂上；渲染只由 [`super::timer_handler`] 驱动。
//! `lv_refr_now()` 是**测试专用强制渲染**，生产路径禁用（设计 §5.2 不变量 2）。

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};

use lvgl_sys as sys;

use super::event::{self, EventCode};
use super::LvglError;

/// 每像素字节数。`lv_conf.h` 固定 `LV_COLOR_DEPTH 32` → XRGB8888，内存序 `B, G, R, X`。
pub const BYTES_PER_PIXEL: usize = 4;

/// PARTIAL 单块缓冲的高度 = 屏高 / 该除数（设计 §10：2 × 1/10 屏 ≈ 2 × 314 KB @1024×768）。
///
/// **单一真源**：测试断言"PARTIAL 缓冲行数 = 屏高 / 该除数"时引用本常量，
/// 不得另行硬编码 `10`（否则改常量会静默放过测试）。
pub(crate) const PARTIAL_ROWS_DIVISOR: u32 = 10;

/// 绘制缓冲起始地址对齐。
///
/// LVGL 在 `lv_display_set_buffers()` 里断言 `buf == lv_draw_buf_align(buf)`，其对齐值
/// `LV_DRAW_BUF_ALIGN` 默认为 4（本项目 `lv_conf.h` 未覆盖）。这里按 64 字节上界分配，
/// 以覆盖日后为 cache line 调大该宏的情形。
const BUF_ALIGN: usize = 64;

/// 屏幕脏区矩形（LVGL `lv_area_t` 的安全镜像；**闭区间**，与 LVGL 一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Area {
    /// 左（含）。
    pub x1: i32,
    /// 上（含）。
    pub y1: i32,
    /// 右（含）。
    pub x2: i32,
    /// 下（含）。
    pub y2: i32,
}

impl Area {
    /// **空区域**（0×0）：`x2 < x1` 且 `y2 < y1` ⇒ [`Area::width`] / [`Area::height`] 均为 0。
    ///
    /// 作为"句柄已失效"的失败返回值（见 [`super::obj::Obj::coords`]）—— 注意**不是**
    /// `Default`（全零的 `lv_area_t` 是 1×1，会与 [`super::obj::Obj::size`] 的 `(0, 0)` 打架）。
    pub(crate) const EMPTY: Self = Self {
        x1: 0,
        y1: 0,
        x2: -1,
        y2: -1,
    };

    /// 区域宽度（像素）。
    ///
    /// 用 `i64` 计算：`x2 - x1 + 1` 在 `i32` 下**可溢出**（debug 构建 panic、release 回绕），
    /// 而本函数是 [`Area::pixel_bytes`] 的输入、后者在 flush 回调内被调用 ——
    /// **回调内的任何 panic 都会跨 FFI 展开**（UB）⇒ 上游也必须一并做无溢出运算
    /// （否则下游的 `checked_mul` 被上游的溢出抵消）。
    pub fn width(&self) -> u32 {
        let w = (self.x2 as i64) - (self.x1 as i64) + 1;
        w.clamp(0, u32::MAX as i64) as u32
    }

    /// 区域高度（像素）。溢出口径同 [`Area::width`]。
    pub fn height(&self) -> u32 {
        let h = (self.y2 as i64) - (self.y1 as i64) + 1;
        h.clamp(0, u32::MAX as i64) as u32
    }

    /// 该区域像素字节数 —— 即 flush 闭包收到的 `&[u8]` 长度。
    ///
    /// **返回 `Option` 而非裸 `usize`**：`w * h * 4` 是**未检查**的 `usize` 乘法，
    /// 回绕既会给出错误长度，更会让 `from_raw_parts` 造出超出真实分配的切片（UB）。
    /// 溢出时返回 `None`，由调用方按「本脏区不画」处理 —— 与 `screen.rs` 的
    /// `required_pixel_bytes`（同为 checked 口径）一致；**本函数不得改回裸乘法**。
    pub fn pixel_bytes(&self) -> Option<usize> {
        (self.width() as usize)
            .checked_mul(self.height() as usize)?
            .checked_mul(BYTES_PER_PIXEL)
    }

    /// 从 C 侧 `lv_area_t` 读取（薄层内部用：flush 桥、`obj.rs` 的 `Obj::coords`）。
    pub(crate) fn read(a: &sys::lv_area_t) -> Self {
        Self {
            x1: a.x1,
            y1: a.y1,
            x2: a.x2,
            y2: a.y2,
        }
    }
}

/// 屏旋转（`lv_display_rotation_t` 的镜像；B4a 新增）。
///
/// 与 `config::Rotate`（CLI 层）**分开定义**：`config.rs` 是 lib 里 `lvgl/**` **之外**的模块，
/// 按 unsafe 边界纪律（`mod.rs` 纪律 1）不得引用 `lvgl_sys` ⇒ 两边各有一个枚举，由
/// `app.rs` 做一次显式转换。这样"CLI 取值集合"与"LVGL 取值集合"的耦合点收在**一处**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Rotation {
    /// 不旋转。
    #[default]
    Deg0,
    /// 顺时针 90°（**逻辑宽高互换**：`hor_res ↔ ver_res`）。
    Deg90,
    /// 180°。
    Deg180,
    /// 顺时针 270°（同样互换宽高）。
    Deg270,
}

impl Rotation {
    /// 转 C 侧枚举（`lv_display_rotation_t`，成员顺序与 [`Rotation`] 逐一对应）。
    pub(crate) const fn to_sys(self) -> sys::lv_display_rotation_t {
        match self {
            Rotation::Deg0 => sys::LV_DISPLAY_ROTATION_0,
            Rotation::Deg90 => sys::LV_DISPLAY_ROTATION_90,
            Rotation::Deg180 => sys::LV_DISPLAY_ROTATION_180,
            Rotation::Deg270 => sys::LV_DISPLAY_ROTATION_270,
        }
    }

    /// 由 C 侧枚举值还原（读回用；未知取值按 [`Rotation::Deg0`] 收敛 —— 不会出现，
    /// 因为写入端只走 [`Rotation::to_sys`]，此处只是防御性全匹配）。
    pub(crate) fn from_sys(v: sys::lv_display_rotation_t) -> Self {
        // `lv_display_rotation_t` 在绑定里是 `c_uint`（enum），逐值比对。
        if v == sys::LV_DISPLAY_ROTATION_90 {
            Rotation::Deg90
        } else if v == sys::LV_DISPLAY_ROTATION_180 {
            Rotation::Deg180
        } else if v == sys::LV_DISPLAY_ROTATION_270 {
            Rotation::Deg270
        } else {
            Rotation::Deg0
        }
    }

    /// 是否互换逻辑宽高（90°/270°）。
    pub const fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Deg90 | Rotation::Deg270)
    }
}

/// LVGL 绘制缓冲：地址按 [`BUF_ALIGN`] 对齐、零初始化、随 [`Display`] 存活。
///
/// 为什么不用 `Vec<u8>`：`Vec<u8>` 只保证 1 字节对齐，而 LVGL 会对缓冲起始地址做对齐
/// 断言；显式对齐分配把这条隐式契约变成显式的。
struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
}

impl AlignedBuf {
    fn new(len: usize) -> Option<Self> {
        if len == 0 {
            return None;
        }
        let layout = std::alloc::Layout::from_size_align(len, BUF_ALIGN).ok()?;
        // SAFETY: `len != 0`，layout 合法（对齐为 2 的幂且非零）。
        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            None
        } else {
            Some(Self { ptr, len })
        }
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        // SAFETY: `ptr` 由 `alloc_zeroed` 以同一 layout 分配，且只在此处释放一次。
        let layout =
            std::alloc::Layout::from_size_align(self.len, BUF_ALIGN).expect("BUF_ALIGN 是合法对齐");
        unsafe { std::alloc::dealloc(self.ptr, layout) };
    }
}

/// flush 桥持有的 Rust 侧像素 sink 类型（闭包，捕获 fb 句柄或内存缓冲）。
type FlushSink = Box<dyn FnMut(Area, &[u8])>;

/// 一个 LVGL display：PARTIAL 双缓冲 + 自研 flush 桥。
///
/// 含裸指针 ⇒ 自动 `!Send` / `!Sync`：编译期即禁止跨线程使用（设计 §5.2 不变量 4）。
pub struct Display {
    raw: *mut sys::lv_display_t,
    // 绘制缓冲必须活过 display 的全部渲染周期（LVGL 只存裸指针）。
    _buf1: AlignedBuf,
    _buf2: AlignedBuf,
    width: u32,
    height: u32,
    /// 创建时的 LVGL 世代（见 [`super::generation`]）：`init → deinit → init` 后
    /// 世代失配即表明底层 display 已被 `lv_deinit()` 释放。
    generation: u64,
}

impl Display {
    /// 建屏：`lv_display_create(width, height)` + PARTIAL 双缓冲（2 × 屏高/10 行），
    /// 并设为默认 display（本模块单屏假设，也满足 `lv_indev_create` 取默认屏的行为）。
    pub fn create(width: u32, height: u32) -> Result<Self, LvglError> {
        if !super::is_initialized() {
            return Err(LvglError::NotInitialized);
        }
        if width == 0 || height == 0 {
            return Err(LvglError::InvalidArgument("display 尺寸不得为 0"));
        }

        let rows = buffer_rows(height);
        // checked 乘法：`buf_bytes` 直接决定 `AlignedBuf` 的分配尺寸，回绕会让缓冲
        // **小于** LVGL 的写入量（后续 flush 越界写）。溢出即拒绝建屏，不静默缩小。
        let buf_bytes = (rows as usize)
            .checked_mul(width as usize)
            .and_then(|n| n.checked_mul(BYTES_PER_PIXEL))
            .ok_or(LvglError::InvalidArgument("display 缓冲尺寸溢出（width×rows×4）"))?;
        // 下传给 LVGL 的是 `u32`（`lv_display_set_buffers` 的形参）⇒ 必须显式校验能装下，
        // 否则 `as u32` 会**静默截断**（与上一句"不静默缩小"的承诺自相矛盾）。
        let buf_bytes_u32 = u32::try_from(buf_bytes)
            .map_err(|_| LvglError::InvalidArgument("display 缓冲尺寸超出 u32"))?;
        let mut buf1 = AlignedBuf::new(buf_bytes).ok_or(LvglError::OutOfMemory("display buf1"))?;
        let mut buf2 = AlignedBuf::new(buf_bytes).ok_or(LvglError::OutOfMemory("display buf2"))?;

        // SAFETY: 已 `init()`；入参为合法正尺寸。
        let raw = unsafe { sys::lv_display_create(width as i32, height as i32) };
        if raw.is_null() {
            return Err(LvglError::OutOfMemory("lv_display_create"));
        }
        // SAFETY: `raw` 刚创建；两个缓冲在 `self` 存活期间保持有效且地址对齐。
        unsafe {
            sys::lv_display_set_default(raw);
            sys::lv_display_set_buffers(
                raw,
                buf1.as_mut_ptr() as *mut c_void,
                buf2.as_mut_ptr() as *mut c_void,
                buf_bytes_u32,
                sys::LV_DISPLAY_RENDER_MODE_PARTIAL,
            );
        }

        Ok(Self {
            raw,
            _buf1: buf1,
            _buf2: buf2,
            width,
            height,
            generation: super::generation(),
        })
    }

    /// 本句柄的底层 display 是否**仍存活**（LVGL 已初始化 **且** 世代未变）。
    ///
    /// `init → deinit → init` 之后，旧句柄的 `raw` 已被 `lv_deinit()` 释放 —— 此时
    /// [`super::is_initialized`] 为 `true`，必须靠世代失配才能识别（否则 double free）。
    pub(crate) fn is_live(&self) -> bool {
        super::is_initialized() && self.generation == super::generation()
    }

    /// 屏宽（像素）。
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 屏高（像素）。
    pub fn height(&self) -> u32 {
        self.height
    }

    /// PARTIAL 单块绘制缓冲的行数（预算核算 / 测试断言用）。
    pub fn buffer_rows(&self) -> u32 {
        buffer_rows(self.height)
    }

    /// 挂 flush 桥：`sink(area, px)` 由 LVGL 在 [`super::timer_handler`] 内回调。
    ///
    /// `px` 是**该脏区**的像素（长度 = `area.width() * area.height() * BYTES_PER_PIXEL`，
    /// 连续、行间无 padding），格式见 [`BYTES_PER_PIXEL`]。
    ///
    /// 重复调用以最后一次为准：先前闭包仍会在 display 删除时统一回收（各自独立注册），
    /// **每次调用的 sink 都恰好 drop 一次** —— 不泄漏、不 double free（见 `tests.rs`
    /// 对"两次 `set_flush_cb` ⇒ 两个 sink 各 drop 1 次"的断言）。
    ///
    /// 已 [`super::deinit`] 或处于上一世代时是 **no-op**：那时 display 已随
    /// `lv_deinit()` 销毁，`self.raw` 悬垂（防 UB）。
    pub fn set_flush_cb<F>(&mut self, sink: F)
    where
        F: FnMut(Area, &[u8]) + 'static,
    {
        if !self.is_live() {
            return;
        }
        let boxed: *mut FlushSink = Box::into_raw(Box::new(Box::new(sink) as FlushSink));
        // SAFETY: `self.raw` 存活；`boxed` 的所有权交给下方 DELETE 回调。
        unsafe {
            sys::lv_display_set_user_data(self.raw, boxed as *mut c_void);
            // 每个 sink 各自注册一个 DELETE 回收回调 → 一一对应，无重复回收。
            // SAFETY: `self.raw` 是刚刚校验过存活的 display 指针（`is_live()`）。
            let _ = event::on(self.raw, EventCode::DELETE, move |_e| {
                // SAFETY: `boxed` 由上面的 `Box::into_raw` 产生，且只在此回收一次。
                drop(Box::from_raw(boxed));
            });
            sys::lv_display_set_flush_cb(self.raw, Some(flush_trampoline));
        }
    }

    /// **测试专用**：强制渲染一趟（`lv_refr_now`），使 `set_size` / `set_pos` 之后的
    /// `coords` 立即落定 —— 否则 [`Obj::size`](super::obj::Obj::size) 读到上一趟的值。
    ///
    /// ⚠️ **生产路径禁用**（设计 §5.2 不变量 2：渲染只发生在 `lv_timer_handler` 内；
    /// 生产调 `lv_refr_now` 会退化为全帧重绘、CPU 预算失控）。故本入口**只在
    /// `cfg(test)` 下存在** —— 生产构建里根本没有它。
    ///
    /// 已 [`super::deinit`] 或处于上一世代时是 **no-op**（[`Self::is_live`] 守卫）：
    /// 那时 display 已随 `lv_deinit()` 销毁，`self.raw` 悬垂（防 UB）。
    #[cfg(test)]
    pub fn refr_now_for_test(&self) {
        if !self.is_live() {
            return;
        }
        // SAFETY: `self.raw` 存活（`is_live()` 已校验：LVGL 已初始化且世代一致）。
        unsafe { sys::lv_refr_now(self.raw) };
    }

    /// 施加屏旋转（`lv_display_set_rotation`；B4a 新增）。
    ///
    /// # 语义（**只换逻辑分辨率**，像素级旋转是驱动的事）
    ///
    /// LVGL v9 的 `lv_display_set_rotation` 只做两件事：写 `disp->rotation` +
    /// `update_resolution()`（把 screen / `top_layer` / `sys_layer` / `bottom_layer` 的
    /// 矩形换成**互换后**的逻辑宽高）。**像素的物理旋转由驱动负责**——
    /// 官方 fbdev 驱动在 `flush_cb` 里调 `lv_display_rotate_area` + `lv_draw_sw_rotate`
    /// 后写进 fb（`vendor/lvgl/src/drivers/display/fb/lv_linux_fbdev.c`）。
    ///
    /// ⚠️ **本单元的接线边界（如实标注）**：本层只把旋转交给 LVGL（逻辑分辨率互换），
    /// **未**实现 sink 侧的像素旋转（`screen::Blitter` / `Canvas` 不做 `sw_rotate`）。
    /// `--rotate 90|270` 时逻辑布局已互换，但写进像素面的仍是未物理旋转的那一份 ⇒
    /// `main.rs` 启动期对非 0 取值**响亮告警**（不静默）。
    /// 绘制缓冲**不受影响**：`lv_display_set_buffers` 用的是
    /// `lv_display_get_original_horizontal_resolution`（**未旋转**的原始尺寸），
    /// 故在 `create` 之后调本方法不必重建缓冲。
    ///
    /// 句柄已失效时 no-op。
    pub fn set_rotation(&self, rotation: Rotation) {
        if !self.is_live() {
            return;
        }
        // SAFETY: 刚校验存活（已初始化且世代一致）⇒ `self.raw` 未被 `lv_deinit` 释放。
        unsafe { sys::lv_display_set_rotation(self.raw, rotation.to_sys()) };
    }

    /// 当前旋转（读回；未设置过 ⇒ [`Rotation::Deg0`]）。
    ///
    /// 存在的理由：`set_rotation` 在离屏链路上**没有别的可观测口**（逻辑分辨率要另引
    /// `lv_display_get_horizontal_resolution`），补上读回它才**可断言**
    /// ——"调用点被删/写错"当场红（见 `tests_b4.rs`）。
    /// 句柄已失效时返回 [`Rotation::Deg0`]。
    pub fn rotation(&self) -> Rotation {
        if !self.is_live() {
            return Rotation::Deg0;
        }
        // SAFETY: 刚校验存活 ⇒ `self.raw` 指向活 display；传 NULL 时 C 侧会用默认屏，
        // 此处传的是本句柄自己的指针。
        Rotation::from_sys(unsafe { sys::lv_display_get_rotation(self.raw) })
    }

    /// 原始 display 指针（薄层内部：`indev.rs` 绑屏、`tests.rs` 强制渲染）。
    pub(crate) fn raw(&self) -> *mut sys::lv_display_t {
        self.raw
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        // `super::deinit()` 已经把全部 display 删掉（并回收 flush sink）——不能再删一次。
        // 用世代令牌而非仅 `is_initialized()`：`init → deinit → init` 之后该标志又为
        // `true`，但本句柄的 display 早已随上一世代 `lv_deinit()` 释放（否则 double free）。
        if self.is_live() {
            // SAFETY: 仍存活（已初始化且世代未变）⇒ 该 display 尚未被删除。
            unsafe { sys::lv_display_delete(self.raw) };
        }
    }
}

/// PARTIAL 单块缓冲的行数（至少 1 行）。
fn buffer_rows(height: u32) -> u32 {
    (height / PARTIAL_ROWS_DIVISOR).max(1)
}

/// C 侧 flush 蹦床：LVGL 渲染好的脏区 → Rust sink。
///
/// **唯一职责是像素搬运**。无论 sink 是否 panic，都必须调用
/// `lv_display_flush_ready()`，否则 LVGL 的刷新队列会停摆（双缓冲不再轮转）。
unsafe extern "C" fn flush_trampoline(
    disp: *mut sys::lv_display_t,
    area: *const sys::lv_area_t,
    px_map: *mut u8,
) {
    let r = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `disp` 由 LVGL 传入且存活；`user_data` 只由 `set_flush_cb` 写入。
        let p = unsafe { sys::lv_display_get_user_data(disp) } as *mut FlushSink;
        if p.is_null() || area.is_null() || px_map.is_null() {
            return;
        }
        // SAFETY: `area` 由 LVGL 传入且指向有效（闭区间）矩形。
        let a = Area::read(unsafe { &*area });
        // `pixel_bytes` 是 checked 的（见其文档）：溢出 ⇒ 本脏区不画，绝不 panic、
        // 也不造出超长切片。这是 flush 回调内，panic 跨 FFI 展开 = UB。
        let Some(n) = a.pixel_bytes() else {
            return;
        };
        if n == 0 {
            return;
        }
        // SAFETY: LVGL 保证 `px_map` 指向 `area` 的 `n` 个连续像素字节。
        let px = unsafe { std::slice::from_raw_parts(px_map, n) };
        // SAFETY: `p` 非空且指向 `set_flush_cb` 放入的闭包；单线程调用。
        let sink: &mut FlushSink = unsafe { &mut *p };
        sink(a, px);
    }));
    if r.is_err() {
        // 走 `diag` 而非 `eprintln!`：此处仍在 `catch_unwind` **之外**、栈上是 C 帧，
        // 而 `eprintln!` 在 stderr 写失败时自身会 panic ⇒ 跨 FFI 展开 = UB。
        super::diag(format_args!(
            "[lvgl] flush 回调内 panic 已被拦截；本次脏区丢弃（不阻塞刷新队列）"
        ));
    }
    // SAFETY: `disp` 由 LVGL 传入且存活。
    unsafe { sys::lv_display_flush_ready(disp) };
}
