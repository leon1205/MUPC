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
//!    init` 之后，上一世代残留的 [`display::Display`] / [`indev::Indev`] / [`obj::Obj`] /
//!    [`style::Style`] 句柄的底层对象（或堆）已被 `lv_deinit()` 释放，句柄据世代失配而
//!    **拒绝**再操作（防 double free / 悬垂调用）。
//!
//! # 模块与工作单元
//!
//! - **A1「核心桥」**：`mod.rs` / [`event`] / [`display`] / [`indev`]；
//! - **A2「薄安全层·对象与样式」**：[`obj`]（`Obj` 包装，把 [`event::on`] /
//!   [`event::CallbackHandle::detach`] 的 `unsafe` 前置条件收进所有权不变量）、
//!   [`style`]（样式机制 + 类型化 setter，**不含** UI 规格值）、[`font`]（10 档位图字体取用）；
//! - **A3「薄安全层·控件」**：[`widgets`]（控件构造与 setter，设计 §5.6 控件映射表；
//!   含滚动容器、长按入口、输入组焦点）；
//! - **B4a「薄层能力补齐」**：[`obj`] 补 G1 样式读回（`bg_opa` / `text_color` / `text_font`）+
//!   G2 `ObjFlag::EVENT_BUBBLE` + G3 滚动位置（`scroll_to_y` / `scroll_y`）、[`display`] 补屏旋转
//!   （`set_rotation` / `rotation`）、`mod.rs` 补定容池余量（[`mem_monitor`]）。
//!
//! `unsafe` 始终只在本目录内（设计 §1.1.1.2 纪律 1）。
//!
//! # 薄层能力缺口登记（`ui/**` 提出的**具名需求**）
//!
//! 本层是 `ui/**` **唯一**的 LVGL 访问面（纪律 1）⇒ 本层没封装的能力，`ui/**` 就**做不到**。
//! 下面逐条登记"界面上层提出的具名能力需求"，每条给出：**缺什么 / 影响 / 候选改进**。
//! 登记处的意义：把"做不到"从散落的注释变成**一份清单** —— 补能力属「薄层收口批」；
//! `src/lvgl/**` 与 `lvgl-sys/allowlist.txt` 是另一个受审表面。
//!
//! **【状态变更（B4a，2026-09-16）】G1 / G2（枚举那一半）/ G3 / G4 四条已补齐** ——
//! 原表"当前**不做**"的口径已作废：四条各自有了薄层 API、薄层单测（`tests_b4.rs`）
//! 与最小消费者；下列表格保留**缺口原文**（便于追溯），状态列写实。
//!
//! | # | 缺口（本层原本未提供的符号） | 影响（`ui/**` 侧的后果） | **B4a 状态** |
//! |---|--------------------------|--------------------------|--------------|
//! | **G1** | **无样式读回**：`bg_opa` / `text_font` / `text_color` 三个 `lv_obj_get_style_*` 均未封装（`Obj` 只暴露 `size` / `coords` / `is_hidden` / `has_flag` 这类**几何与旗标**读回） | **"用错常量的视觉退化"在对象层不可判** —— 例：把整屏降级遮罩的不透明度由 20 % 偷换成模态的 62 %、把中央大字由 64 px 换成 24 px、或把 `#FFB020` 换成 `DANGER` 红，**对象级断言全部读不回来**（对象层只剩 `size` 可读，而"字号"若没同步喂进 `set_size` 连尺寸都不变）。`ui/**` 的既有对策是**应用标记**（`shell.rs::TabBg` / `p5_audit.rs::immutable_bg`：记下"本层送进 setter 的那个值"）+ **单一绑定**（建对象与设尺寸同源，`shell.rs::OVERLAY_TITLE_SLOT`）。**残余**：应用标记只证明"本层送出了哪一档"，不证明"LVGL 真的按它渲染"；**字色 / 真实字体**至今无任何网 | **✅ 已补齐**：[`obj::Obj::bg_opa`] / [`obj::Obj::text_color`] / [`obj::Obj::text_font`]。⚠️ 三个 C 侧 getter 是**头文件 `static inline`**（不入绑定）⇒ 按其**同一实现**复刻：`lv_obj_get_style_prop` + `LV_STYLE_*`（先例：`style.rs::Style::set_pad_all`；理由见 `obj.rs` 该节说明）。消费者：`ui/tests.rs::shell_chain` 的 EDGE-03 段把遮罩档位 / 大字字色 / 大字字体由"**常量相等**"升级为"**对象实际值相等**"（`ui/shell.rs` 的 G1 残余登记已同步订正为"**已可判**"）。**剩余边界（如实标注；B4a 整改「建议 3.1」按**实测**收窄）**：默认构建未启用 `noto-font` ⇒ 各字号槽都降级到同一 fallback 指针；而 LVGL 默认主题给裸 `lv_obj` 挂的 `LV_FONT_DEFAULT` 又**正是同一个** `lv_font_montserrat_14` ⇒ **字体**读回此时**连"设过"都证不了**（探针实测：把标签样式换成不带字体的那条，`shell_chain` 的字体断言**仍绿**）；只有启用 `noto-font` 构建（各槽拿到不同指针）后才有"换错槽 / 漏设字体即红"的判别力。**`bg_opa` / `text_color` 两项不受此限**（值域与主题默认不同，任何构建下都成网） |
//! | **G2** | `ObjFlag` **未镜像** `LV_OBJ_FLAG_EVENT_BUBBLE`；`Indev` **未**暴露 `lv_indev_add_event_cb`（该符号**在** `allowlist.txt` 里，只是没封装） | `ui/shell.rs` 的「任何触摸事件重置空闲计时」（UI §4.3）只覆盖**3 个控件**：LVGL 默认不上冒 + `lv_indev_search_obj` 取"命中的最深可点对象" ⇒ 页内按压**到不了**外壳根。详见 `ui/shell.rs` 偏差表 **SH5** | **◑ 一半**：[`obj::ObjFlag::EVENT_BUBBLE`] **已补**（镜像枚举值，**无需新 C 符号**）+ 薄层单测；**但 `ui/**` 的行为未动** —— SH5 的完整语义（外壳装配后**递归**给整棵子树置位，并把 `shell_chain` 的"页内按压**不**重置"现状断言改写为"也重置"）属 **B4b**。另一条候选路（给 [`indev::Indev`] 加 `on(EventCode, F)` —— 一处挂钩覆盖全屏、不碰 `pages/**`）**仍在桌上**，由 B4b 二选一 |
//! | **G3** | `Obj` 无"滚到指定位置"封装（`set/get_scroll_dir` / `set/get_scrollbar_mode` 有，`lv_obj_scroll_to_y` 未封装、**也**未进 `allowlist.txt`） | UI §4.3「超时回归 P1 始终从顶部开始」**做不到** ⇒ 回归后 P1 保留上次滚动位置。详见 `ui/shell.rs` 偏差表 **SH12** | **✅ 已补齐**：[`obj::Obj::scroll_to_y`] / [`obj::Obj::scroll_y`]（+ `allowlist.txt` 放行两个符号；`LV_EVENT_SCROLL` 早已随 `lv_event_code_t` 生成，无需新增）。**消费者 = B4b 的外壳**（`Core::select` 切回 P1 时调一次）；本单元**只补能力 + 薄层单测**，不动 `ui/**` 行为 |
//! | **G4** | **无内存余量读回**：`lv_mem_monitor` 未封装 ⇒ 设计 §10 的内存预算与 §14 风险 **R-24**「**1 MB 定容池余量未量化**」只能靠估算 | 定容池（`LV_MEM_SIZE` = 1 MiB）被打满时表现为 `lv_*_create` 返回 NULL（[`LvglError::OutOfMemory`]），**没有观测口** ⇒ 真机现场无法回答"离打满还有多远"、`--smoke` 也无法量化 | **✅ 已补齐**：[`mem_monitor`]（[`MemStats`]）。消费者：`--smoke` 逐行打印 `mem_total/mem_free/mem_max_used/mem_used_pct/mem_frag_pct/mem_headroom=…`，并作为**第五条判定口** `FAIL_MEM_TIGHT`（峰值 ≥ 可分配总量 95 %、或**根本没读到**池 ⇒ 失败）。⚠️ 读的是**定容池**（对象树/样式/定时器），**不含绘制缓冲**（后者走系统堆，见 [`MemStats`]） |
//! | **G5** | **无屏旋转通道**：`lv_display_set_rotation` 未封装、`allowlist.txt` 亦无该符号 ⇒ `config.rs` 对 `--rotate` 非 0 取值只能**启动即报错退出**（工作单元 C 评审 C-③ 的口径） | `--rotate` 的四个取值里三个用不了（面板横竖装无法适配） | **✅ 已补齐**：[`display::Display::set_rotation`] / [`display::Display::rotation`]（+ `lv_display_rotation_t`）。消费者：`app.rs::rotation_of` 映射后在建对象**之前**施加。⚠️ **B4a 整改「重要 2」把 `config.rs` 侧退回旧口径**：`0\|90\|180\|270` **不再全放行**，**非 0 在解析期即硬错误**（薄层能力仍在、接线仍在，只是 CLI 不放行）。**理由**：`lv_display_set_rotation` 只做**逻辑分辨率互换**；像素的物理旋转是**驱动/sink 的职责**（官方 fbdev 驱动在 flush 内 `lv_draw_sw_rotate`），本项目 `screen::Blitter` **未实现** ⇒ 非 0 取值下**像素面确已画错**（评审实测 `--rotate 90 --smoke`：`result=OK`、P4 `active_px` 524189 → 406880，五道自检门禁全绿）—— 这是**主动画错版式**而非 no-op，故 fail-fast；**sink 侧实现后摘除该 guard 即可** |
//!
//! ⚠️ **本清单不含"规格未要求、实现也未做"的项**（那是 `ui/**` 的偏差登记表管的）；
//! 只登记"**规格要求了、而本层没给能力** ⇒ 上层结构上做不到"的项。

pub mod display;
pub mod event;
pub mod font;
pub mod indev;
pub mod obj;
pub mod style;
pub mod widgets;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_a2;
#[cfg(test)]
mod tests_a3;
#[cfg(test)]
mod tests_b4;

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

/// LVGL **定容池**的余量快照（`lv_mem_monitor_t` 的安全镜像；B4a 新增）。
///
/// # 这一块内存到底是什么（**防误读**，务必照抄到任何引用处）
///
/// `lvgl-sys/lv_conf.h` 设 `LV_USE_STDLIB_MALLOC = LV_STDLIB_BUILTIN` +
/// `LV_MEM_SIZE = 1024 * 1024` ⇒ LVGL 用**自己的**一块 **1 MB 定容堆**，
/// `lv_mem_monitor()` 报的是**这一块池**的余量 —— 它装的是**对象树 / 样式属性表 /
/// 定时器 / 事件项 / 字体以外的内部结构**。
///
/// ⚠️ 它**不包含绘制缓冲**：两个 PARTIAL 缓冲由 [`display::Display`] 用 Rust 的
/// `std::alloc` 分配（`display.rs::AlignedBuf`），走**系统堆**，与本池无关
/// （`--smoke` 的 2 × 屏高/10 ≈ 2 × 314 KB 不在 `total_size` 里）。
///
/// # 为什么需要它
///
/// 设计 §10 的内存预算与 §14 风险 **R-24** 一直挂着一句"**1 MB 定容池余量未量化**"
/// （`mod.rs` 的缺口表 G4 行）。本读回把该残余**在离屏自检里量化出来**：`--smoke` 逐行打印
/// [`MemStats`]，真机（fbdev）同样可读 —— 判据一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemStats {
    /// **可分配字节总量**（= 池面积扣除块头开销），**不逐字节等于** `LV_MEM_SIZE`。
    ///
    /// # 它为什么会变（**别把它当常量**）
    ///
    /// LVGL 把本字段由 `lv_tlsf_walk_pool` **逐块累加** `block_size` 得到
    /// （`lv_mem_core_builtin.c::lv_mem_walker`），而每个块的大小字段**不含块头**
    /// （`lv_tlsf.c`：`block_header_overhead`）⇒ "总量" = 池面积 − 块数 × 块头开销。
    /// **块数随分配 / 合并形态变化**（同一进程里前后两次读数就可能不同），故：
    ///
    /// - 实测：单元测试链路（少量对象）≈ **1044384**；`--smoke` 六页走查后 ≈ **986632**；
    /// - 判据请用"**≤ `LV_MEM_SIZE` 且同一量级**"，**不要**写 `== 1 MiB`。
    ///
    /// "还剩多少"看 [`MemStats::free_size`]，"高水位"看 [`MemStats::max_used`]。
    pub total_size: usize,
    /// **当前**空闲字节数。
    pub free_size: usize,
    /// 空闲块个数。
    pub free_cnt: usize,
    /// **最大**的单块空闲字节数（判"还能不能塞下一个大对象"）。
    pub free_biggest_size: usize,
    /// 已分配块个数。
    pub used_cnt: usize,
    /// **历史峰值**已用字节数（`max_used`；跑完一遍全链路后即"高水位"）。
    pub max_used: usize,
    /// 当前使用率（%，C 侧算好，0–100）。
    pub used_pct: u8,
    /// 碎片率（%，C 侧算好，0–100）。
    pub frag_pct: u8,
}

impl MemStats {
    /// 池余量是否**健康**：已初始化过（`total_size > 0`）且历史峰值没有打满池
    /// （留有余量 ⇒ 不会因多建一个对象就分配失败）。
    ///
    /// `--smoke` 会把它打印成 `headroom=ok|tight`（见 `main.rs::run_smoke`）：**它必须能失败** ——
    /// 判据写成恒真就只是"打印"。阈值取"峰值 < 可分配总量的 95 %"，给"运行期新增对象"留 5 %。
    ///
    /// 口径说明：`max_used`（峰值**载荷**）与 `total_size`（载荷总量，见该字段的"它为什么会变"）
    /// 同为**载荷**口径 ⇒ 相除有意义；块头开销不在两者之内。
    pub fn has_headroom(&self) -> bool {
        self.total_size > 0 && self.max_used * 100 < self.total_size * 95
    }
}

/// 读一次 LVGL 定容池余量（`lv_mem_monitor`；未 `init()` 时返回全 0）。
///
/// 字段含义与"**不含绘制缓冲**"的边界见 [`MemStats`]。调用线程必须是事件循环线程
/// （设计 §5.2 不变量 4）。
pub fn mem_monitor() -> MemStats {
    if !is_initialized() {
        return MemStats::default();
    }
    // `lv_mem_monitor_t` 全零是合法位型（几个 `size_t` + 两个 `uint8_t`）；
    // `lv_mem_monitor` 按契约整体填充它。
    let mut m: sys::lv_mem_monitor_t = unsafe { std::mem::zeroed() };
    // SAFETY: 已 `init()` ⇒ 定容池存在；`m` 是本函数私有、按 C 侧布局分配的栈对象。
    unsafe { sys::lv_mem_monitor(&mut m) };
    MemStats {
        total_size: m.total_size,
        free_size: m.free_size,
        free_cnt: m.free_cnt,
        free_biggest_size: m.free_biggest_size,
        used_cnt: m.used_cnt,
        max_used: m.max_used,
        used_pct: m.used_pct,
        frag_pct: m.frag_pct,
    }
}
