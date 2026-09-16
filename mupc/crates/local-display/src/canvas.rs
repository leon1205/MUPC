//! 画布色彩类型 + Linux 帧缓冲后端（FbCanvas）。
//!
//! 对齐 `[DESIGN_APPROVED]` 设计 §B1 / §5.2 canvas.rs / §6：
//! - 颜色：32-bit 不透明 `0x00RRGGBB`（alpha 留 0——屏面无合成，仅字形覆盖做 alpha 混合）。
//! - `Canvas` trait：**像素面的最小接口**（宽 / 高 / `blit_pixels`）—— 由 [`fbdev::FbCanvas`]
//!   实现，并被 `screen.rs` 的 [`crate::screen::PixelSink`] 复用为真机 sink。
//! - FbCanvas（仅 `target_os = "linux"`，`/dev/fb0` mmap）：Windows 本机不编译该 cfg 块；
//!   真机像素格式(bpp/order)/DRM 后端属设计 §13 前置项 1，实现按「32bpp XRGB 映射」，
//!   首验不符时切 `--fbdev-path` 或补 DRM dumb-buffer。
//!
//! # 死代码清理（开发单元 **B3-2b-1**，主控逐项裁定）
//!
//! 本文件曾同时承担 **v1.0 的自绘链路**（v2.0 已整体废弃：`font.rs` / `layout.rs` / `run.rs`
//! 与 `tests/full_chain.rs` 在 B3-2a 删除，见 `lib.rs` 模块文档）。残留的 v1.0 绘制原语
//! **无任何生产消费者**，本轮删除：
//!
//! | 已删符号 | 原用途（v1.0） | 语义现由谁承担 |
//! |----------|----------------|----------------|
//! | `OffscreenCanvas` | 内存像素面（自绘链路的离屏断言） | [`crate::screen::MemorySink`]（v2.0 的离屏出口，**与真机共用同一条 flush 路径**） |
//! | `blend_over` | 字形反锯齿混合（自绘链路自己光栅化文本时用） | **无**（文本光栅化已由 LVGL 承担，`ui/**` 不再自绘字形）；**真需要时点名**：字体混合属薄层 `crate::lvgl::font` 的职责，不在本文件 |
//! | `Canvas::{set_px, pixel, clear, fill_rect}` + `fill_rect_default` | 逐像素 / 矩形绘制原语（自绘链路） | **无**（LVGL 负责绘制；`PixelSink` 只需要"写一块像素"，即保留的 `blit_pixels`）。⚠️ 注意 `pixel`（**读**单像素）在 v2.0 的回读需求由 `screen::MemorySink::pixel` 满足（**同一个名字，不同的类型**） |
//! | `hex` / `rgb` 便捷构造 | 自绘链路里写色值 | `ui/theme.rs` 的 `Palette`（命名常量；`Color::hex` 是 `lvgl::style` 的**另一个** `hex`） |
//! | `lib.rs::new_offscreen_canvas()` | 快捷构造离屏画布 | [`crate::screen::MemorySink::new`] |
//!
//! **保留**：`Color`（`screen.rs` / `app.rs` 的像素类型）、`Rect`（`FbCanvas::blit_pixels`
//! 的裁剪用）、[`Canvas`] 的 `width` / `height` / `blit_pixels`（`screen.rs` 的 `PixelSink`
//! impl 直接调它们）、[`fbdev::FbCanvas`]（设计 §8.3 保留资产 ⇒ 真机 flush sink）。
//!
//! **残留（如实登记）**：`Rect::width` / `Rect::height` / `Rect::contains` 三个便捷方法
//! **在生产路径上没有调用点**（`FbCanvas::blit_pixels` 只用 `Rect::new` + 内部的
//! `clipped_to`；三者目前只由本模块的 `rect_clips_to_canvas_half_open` 用例覆盖）。
//! 它们**不是死代码**（`pub` + 模块 `pub` ⇒ 仍在 crate 的公开面上，`lib.rs` 也仍 re-export
//! `Rect`），保留理由 = 它们是 `Rect` 这个值类型的**语义完整性**一部分（删掉后调用者只能
//! 自己算 `x1 - x0`）。**若 PM 要求砍到最小公开面**：这三条 + `lib.rs` 的 `Rect` 一并删，
//! 此时 `clipped_to` 的 `dead_code` 许可也要跟着重新评估。
//!
//! # 具名豁免：`FbCanvas` 的 `unsafe` 与 §1.1.1.2 纪律 1（评审 C-⑤ 留痕）
//!
//! 设计 §1.1.1.2 纪律 1 规定「`unsafe` 只允许出现在 `lvgl-sys` 与 `src/lvgl/*`」。本模块的
//! [`fbdev::FbCanvas`] **是 v1.0 既有资产**，按设计 §8.3 **保留**，v2.0 升为 `flush_cb` 的
//! 像素 sink（`screen.rs` 里为它实现了 `PixelSink`），**不引用 `lvgl_sys`**。其 `unsafe`
//! （libc `open`/`ioctl`/`mmap`/`munmap`）**不涉及任何 LVGL 绑定**，与纪律 1 的立意
//! （「绑定层的 unsafe 只用点」）不冲突。**该豁免将补记进设计文档**（设计侧由主控统一修订，
//! 本文件只做代码侧留痕）。
//!
//! ## 刻意不实现 `Send`/`Sync`（同评审 C-⑤）
//!
//! `FbCanvas` 含 mmap 裸指针 ⇒ 自动 `!Send`/`!Sync`，**本模块刻意不再手写 `unsafe impl`**：
//! fb 映射一旦被跨线程写，就与 LVGL 的单线程约束（设计 §5.2 不变量 4）叠加成不可审计的数据
//! 竞争。当前唯一调用点是 `main.rs` 在 `current_thread` 运行时内以 `&mut Renderer<FbCanvas>`
//! 独占驱动（无 `spawn`、无 `Send` 约束），故**编译期锁死单线程**是最低成本的正确选择；
//! 将来若真要跨线程，必须先给出显式的所有权/同步设计，而不是靠一行 `unsafe impl` 放开。

/// 颜色 = `0x00RRGGBB`（不透明）。
pub type Color = u32;

/// 轴对齐矩形（左闭右开：`[x0, x1) × [y0, y1)`）。超出画布的绘制由实现裁剪。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect {
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> i32 {
        self.y1 - self.y0
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }

    /// 与画布裁剪区求交（空矩形 → None）。
    ///
    /// ⚠️ **在非 Linux 构建里"看似无人用"**（`dead_code` 许可）：唯一的生产消费者是
    /// [`fbdev::FbCanvas::blit_pixels`]（`cfg(target_os = "linux")` 块内，本机 Windows
    /// **不编译**），另由本模块的 `rect_clips_to_canvas_half_open` 用例直接覆盖。
    /// 若日后把本方法删掉，**Linux 交叉编译才是判据**（本机 `cargo check` 验不到）。
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn clipped_to(&self, w: u32, h: u32) -> Option<Rect> {
        let x0 = self.x0.max(0);
        let y0 = self.y0.max(0);
        let x1 = self.x1.min(w as i32);
        let y1 = self.y1.min(h as i32);
        if x0 < x1 && y0 < y1 {
            Some(Rect { x0, y0, x1, y1 })
        } else {
            None
        }
    }
}

/// 像素面抽象（**v2.0 的最小面**：宽 / 高 / 写一块像素）。
///
/// 由 [`fbdev::FbCanvas`] 实现；`screen.rs` 的 [`crate::screen::PixelSink`] 为它写的适配层
/// 直接调用本 trait 的三个方法。**v1.0 的逐像素 / 矩形 / 清屏原语已删**（无生产消费者，
/// 见文件头「死代码清理」）。
pub trait Canvas {
    fn width(&self) -> u32;
    fn height(&self) -> u32;

    /// 以 (x0,y0) 为左上，blit 一个不透明小位图（行宽 `stride`，仅低 `width` 字节有效）。
    /// 连续缓冲实现可覆写为整行 memcpy。offset 按像素。
    fn blit_pixels(&mut self, x: i32, y: i32, width: u32, height: u32, src: &[Color]);
}

// ---------------------------------------------------------------------------
// FbCanvas —— Linux /dev/fb0 mmap（仅 target_os="linux" 编译；Windows 本机不编译）
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub mod fbdev {
    //! Linux 帧缓冲画布：mmap `/dev/fb0`（固定按 32bpp XRGB 映射，见模块注释）。
    //! 真机像素格式/DRM 后端属设计 §13 前置项 1；本实现为「逻辑正确 + 可编译桩」，
    //! 首验不符需在此处按实机 bpp/order 校准或补 DRM dumb-buffer 后端。
    //!
    //! ## ⚠️ 部署首验必读（本机 Windows 无法编译/运行该块，以下为待验证项）
    //!
    //! 1. **映射长度**：O3 评审整改后已用 `FBIOGET_FSCREENINFO` 校验 `smem_len` ≥ 请求长度
    //!    （`w*h*4`），不足**直接报错**、不 mmap/不越界写（原实现只按请求长度 mmap，显存不足时
    //!    mmap 成功但越界写会 SIGBUS）。仍属 §13 前置项 1：真机须确认 `/dev/fb0` 显存 ≥ 3MiB。
    //! 2. **像素格式**：假定 32bpp `XRGB8888` 且行跨距 = `w*4`。实机若为 16bpp 或
    //!    跨距含 padding，颜色/花屏会异常，需按 `fb_var_screeninfo` 校准。
    //! 3. 建议部署脚本以 `--backend offscreen` 先跑通全链路，再切实屏定位驱动层问题。
    //!
    //! ## ⚠️ 本块**只在本机 Linux 上参与编译**（Windows 走 `cfg` 分支不编译）
    //!
    //! ⇒ 任何对本块的改动，**本机 `cargo check` 都验不到**。B3-2b-1 删除 v1.0 绘制原语时
    //! 同步删掉了 `impl Canvas for FbCanvas` 里的 `set_px` / `pixel` / `clear` / `fill_rect`
    //! 四个方法（它们只被已删的 `OffscreenCanvas` 对称实现与自绘链路消费），**保留的
    //! `width` / `height` / `blit_pixels` 是 `screen.rs::PixelSink` 唯一的入口** —— 交叉编译
    //! （`./deploy/scripts/build-for-rk3588.sh --cross`）是这块的唯一编译判据。

    use super::*;

    /// `FBIOGET_FSCREENINFO`（`<linux/fb.h>`：0x46 是 'F'，该族 ioctl 为**未编码**常量，
    /// 各架构同值）。libc 未提供该常量与 `fb_fix_screeninfo` 绑定，故本地声明。
    const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;

    /// `struct fb_fix_screeninfo`（`<linux/fb.h>` 内核 ABI 逐字段对齐；`c_ulong` 保 32/64 位
    /// 布局一致）。本实现只消费 `smem_len`，其余字段仅供 ioctl 写满结构体。
    #[repr(C)]
    #[derive(Clone, Copy)]
    #[allow(dead_code)] // 内核 ABI 结构体：仅 smem_len 被消费，其余字段为布局占位
    struct FbFixScreenInfo {
        id: [libc::c_char; 16],
        smem_start: libc::c_ulong,
        smem_len: u32,
        type_: u32,
        type_aux: u32,
        visual: u32,
        xpanstep: u16,
        ypanstep: u16,
        ywrapstep: u16,
        line_length: u32,
        mmio_start: libc::c_ulong,
        mmio_len: u32,
        accel: u32,
        capabilities: u16,
        reserved: [u16; 2],
    }

    pub struct FbCanvas {
        w: u32,
        h: u32,
        fd: i32,
        map: *mut u8,
        len: usize,
    }

    // ⚠️ 刻意**不**实现 `Send`/`Sync`（评审 C-⑤ 裁定删除原手写 `unsafe impl`）：
    // `FbCanvas` 含 mmap 裸指针 ⇒ 自动 `!Send`/`!Sync`。理由与取舍见本模块文件头的
    // 「刻意不实现 Send/Sync」小节：编译期锁死单线程，防止 fb 映射被跨线程写而与
    // LVGL 单线程约束（设计 §5.2 不变量 4）叠加成不可审计的数据竞争。
    // 另需注意（Rust 模型之外的残留风险，属真机首验项，设计 §13 前置项 1）：`map` 指向
    // 内核/显示控制器共享的物理显存，若同机 fbcon/其他进程并发写 /dev/fb0，本进程无法用
    // 锁串行化外部写者；当前部署假设「渲染进程独占 fb0、fbcon 未占用该 fb」。

    impl FbCanvas {
        /// 打开帧缓冲设备并按 `w×h×4`（32bpp）mmap。返回前会清零整个显存。
        pub fn open(path: &str, w: u32, h: u32) -> crate::Result<Self> {
            let cpath = std::ffi::CString::new(path)
                .map_err(|_| crate::Error::Backend("fb path contains NUL".into()))?;
            // SAFETY: `cpath` 是本作用域内仍存活的 `CString`，`as_ptr()` 给出以 NUL 结尾、
            // 指向有效可读内存的 C 字符串，且在整个调用期间不会被移动或释放（借用自局部的
            // 不可变绑定）。`O_RDWR` 不含 `O_CREAT`，故 open 的变参列表无需第三个 mode 实参
            // （传了反而是未定义行为）。返回值是 fd 整数，下一行立即判 `< 0`；失败时无任何
            // 需要回收的资源（不产生半初始化对象）。后置条件：`fd >= 0` 时为一个尚未归还的
            // 有效 fd，其所有权由本函数后续路径唯一持有（成功则交给 FbCanvas，失败则 close）。
            let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDWR) };
            if fd < 0 {
                return Err(crate::Error::Backend(format!(
                    "open {path} failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            let len = (w as usize)
                .checked_mul(h as usize)
                .and_then(|n| n.checked_mul(4))
                .ok_or_else(|| {
                    // SAFETY: `fd` 由上方 open 成功返回（此处 `fd >= 0` 已判），且此分支尚未
                    // 构造 `FbCanvas`——即该 fd 目前无其他所有者；本处是它唯一一次 close，
                    // 关闭后不再被使用（`?` 立即把错误向上传播，函数返回）。故无 double-close、
                    // 无 use-after-close。`close` 的返回值（EINTR/EIO）在此忽略，因为无论如何
                    // 都必须放弃该 fd（无重试语义）。
                    let _ = unsafe { libc::close(fd) };
                    crate::Error::Backend("fb size overflow".into())
                })?;
            // O3：mmap 前校验显存实际长度（`smem_len`）≥ 请求长度——不足即报错，
            // 杜绝「mmap 成功但越界写 SIGBUS」。（ioctl 只填结构体，不触碰显存。）
            // SAFETY: `FbFixScreenInfo` 是 `#[repr(C)]` 纯整数结构体（`c_char`/`u32`/`u16`/
            // `c_ulong` 及其数组），这些类型的**任意位模式**都是合法值（无引用、无函数指针、
            // 无 `NonNull`/`bool`/枚举等对位模式有前提的字段），故全零初始化不会构造出无效值；
            // 内部可能存在的 padding 字节同样对整数类型无有效性约束。零初始化是必要的，因为
            // ioctl 只保证写入内核侧结构体长度内的字段（本实现只消费 `smem_len`，其余字段
            // 若为垃圾值也仅是占位，但仍以零值避免 trace 时出现未初始化内存）。
            let mut fix: FbFixScreenInfo = unsafe { std::mem::zeroed() };
            // SAFETY: 1) `fd` 为上方 open 成功返回的 O_RDWR fd（有效）；2) `FBIOGET_FSCREENINFO`
            //    （0x4602，`linux/fb.h` 中该族为**未编码**常量，各架构同值）是「取回」型请求，
            //    语义为内核把 `struct fb_fix_screeninfo` 拷贝**写入**第三个参数指向的结构体，
            //    不读入其中的旧内容（因此无需预先填充）；
            // 3) 第三个实参是栈上存活的 `fix` 的可变借用转裸指针，调用期间有效、对齐（`c_ulong`
            //    决定 8 字节对齐）、独占（`fix` 无其他并发引用）；
            // 4) 结构体字段顺序与宽度逐字段对应内核 `linux/fb.h` 的 `struct fb_fix_screeninfo`
            //    （`c_ulong` 保证 32/64 位布局一致），故内核按内核布局写入的字节数不超过
            //    `size_of::<FbFixScreenInfo>()`，不会越过 `fix` 所在栈帧 → 无缓冲区溢出。
            //    此「布局一致」是**人证而非机器校验**（libc 未绑定该结构体，无编译期断言）：
            //    若内核头文件改版，需重新比对（属设计 §13 前置项 1 真机首验范畴）。
            let rc = unsafe { libc::ioctl(fd, FBIOGET_FSCREENINFO as _, &mut fix as *mut FbFixScreenInfo) };
            if rc < 0 {
                let e = std::io::Error::last_os_error();
                // SAFETY: `fd` 有效，且此错误分支是它唯一的所有者/唯一一次 close，之后立即
                // return，不再使用该 fd（无 double-close / use-after-close）。
                unsafe { libc::close(fd) };
                return Err(crate::Error::Backend(format!(
                    "FBIOGET_FSCREENINFO {path} failed: {e}——无法校验显存长度，拒绝映射"
                )));
            }
            let smem_len = fix.smem_len as usize;
            if smem_len < len {
                // SAFETY: `fd` 有效且在此分支仍为唯一所有者；close 是该 fd 唯一一次关闭，
                // 随后立即 return（未映射任何内存，无需 munmap）。
                unsafe { libc::close(fd) };
                return Err(crate::Error::Backend(format!(
                    "{path} 显存 {smem_len} 字节 < 需要 {len} 字节（{w}x{h} 32bpp）——拒绝越界映射"
                )));
            }
            // SAFETY（前置条件均由上文建立）：
            // - `len` = w*h*4 经 `checked_mul` 防溢出，且已校验 `len <= fix.smem_len`
            //   （内核自报的该 fb 显存字节数）→ 请求的映射区间落在设备显存之内，杜绝
            //   「mmap 成功但写入越界 → SIGBUS」；
            // - `fd` 有效且以 O_RDWR 打开，与 `PROT_READ | PROT_WRITE` 权限匹配；
            //   `MAP_SHARED` 使写入对显示控制器可见（本用例必需，非 MAP_PRIVATE）；
            // - `offset = 0` 满足 mmap 的页对齐要求；`addr = NULL` 表示交由内核选址
            //   （未带 MAP_FIXED），不会覆盖/顶掉既有映射；
            // - 对齐：`len` 与 `map` 不必页对齐（内核按页处理并返回页对齐地址），因此
            //   本调用无「结构体/缓冲区对齐」类前提。
            // 后置条件：返回值已判 != `MAP_FAILED`（失败即 close + 返回，不构造 FbCanvas）；
            // 成功时 `map` 是恰好 `len` 字节的可读写映射首地址，由 `Drop` 以同一 `len`
            // 做 munmap 保证配对释放。
            let map = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    0,
                )
            };
            if map == libc::MAP_FAILED {
                let e = std::io::Error::last_os_error();
                // SAFETY: `fd` 有效且为唯一所有者；mmap 已失败（无映射需 undo），close 是
                // 该 fd 唯一一次关闭，随后立即 return。
                unsafe { libc::close(fd) };
                return Err(crate::Error::Backend(format!("mmap {path} failed: {e}")));
            }
            // 清零显存（首帧前避免残影）。
            // SAFETY: `map` 已确认 != `MAP_FAILED`，是上一步刚建立的、长度恰为 `len` 字节的
            // 可读写映射首地址（非空、页对齐）；写入长度就是 `len`，恰好等于映射长度 →
            // 完全在映射内、不越界。目标元素类型为 `u8`，无对齐要求（页对齐地址更满足）。
            // 写入值 0 对 `u8` 无有效性约束；按 32bpp 解读即透明黑，与后续 `clear` 的
            // 「全 0/全 c」语义一致。`len` 是局部不可变绑定，映射存续期间不会变化；写入后
            // `map`/`len` 原值成组移入 `FbCanvas`，两者配对的映射关系保持不变。
            unsafe { std::ptr::write_bytes(map as *mut u8, 0, len) };
            Ok(Self {
                w,
                h,
                fd,
                map: map as *mut u8,
                len,
            })
        }
    }

    impl Drop for FbCanvas {
        fn drop(&mut self) {
            // SAFETY: 逆序释放 `open()` 建立的资源，每个资源只释放一次、释放后不再访问
            // （`Drop` 之后对象即销毁）：
            // - `map`：仅在 `open` 成功路径被赋值（`MAP_FAILED` 在该路径被拦截并提前 return），
            //   故「非 null」等价于「确实是有效映射首地址」；`self.len` 正是当初 mmap 的长度，
            //   munmap 的长度与映射严格一致（长度不符是 UB，此处不存在该风险）。注：本处
            //   不判 `MAP_FAILED`（其值为 -1 非 null）——该值从未写入 `self.map`，故无需判。
            // - `fd`：`fd >= 0` 是 `open` 成功的判据，且失败路径已各自 close 并 return，
            //   绝不会留下 `fd >= 0` 的已关闭 fd 交给 `Drop` → 无 double-close；close 之后
            //   不再使用该 fd。忽略 close 返回值：析构无法重试，且进程生命周期内 fd 归还
            //   失败无补救手段（进程退出时由内核回收）。
            unsafe {
                if !self.map.is_null() {
                    libc::munmap(self.map as *mut libc::c_void, self.len);
                }
                if self.fd >= 0 {
                    libc::close(self.fd);
                }
            }
        }
    }

    impl Canvas for FbCanvas {
        fn width(&self) -> u32 {
            self.w
        }
        fn height(&self) -> u32 {
            self.h
        }
        fn blit_pixels(&mut self, x: i32, y: i32, width: u32, height: u32, src: &[Color]) {
            // O5：裸指针按声明尺寸读 `src`——过短即越界读；此处长度校验兜底（不越界写显存）。
            if src.len() < (width as usize).saturating_mul(height as usize) {
                return;
            }
            let r = Rect::new(x, y, x + width as i32, y + height as i32);
            if let Some(clip) = r.clipped_to(self.w, self.h) {
                let base = self.map as *mut Color;
                for yy in clip.y0..clip.y1 {
                    let sy = (yy - y) as usize;
                    let sx = (clip.x0 - x) as usize;
                    let n = (clip.x1 - clip.x0) as usize;
                    let dst = (yy as usize * self.w as usize + clip.x0 as usize) as isize;
                    // SAFETY（读 `src` 侧）：
                    // - 上方已校验 `src.len() >= width * height`（`saturating_mul` 防溢出；
                    //   不满足即提前 return，不进入本块），故 `src` 至少有 width*height 个元素；
                    // - `sy = yy - y`，其中 yy ∈ clip ⊆ [y, y+height) ⇒ sy ∈ [0, height)；
                    //   `sx = clip.x0 - x` 且 clip.x0 ≥ x ⇒ sx ∈ [0, width)；因 clip.x1 ≤ x+width
                    //   ⇒ n ≤ width - sx。因此最大读索引 = sy*width + sx + (n-1) ≤
                    //   (height-1)*width + width - 1 = height*width - 1 < src.len() ⇒
                    //   `src.as_ptr().add(...)` 与 `*s.add(k)` 全部落在该切片的分配内、
                    //   最远仅到「末尾前一个元素」（未越过 `add` 允许的一格越界红线）；
                    // - 对齐：`src: &[Color]` 本身按 4 字节对齐，偏移均以元素为单位 ⇒ 对齐保持。
                    // SAFETY（写 `base` 侧）：`dst` 与 `dst + k`（k < n）— 由 `clipped_to`
                    //   保证 yy ∈ [0, h)、clip.x0 + n = clip.x1 ≤ w ⇒ 最大写索引 ≤ w*h - 1，
                    //   与 `set_px` 同一映射长度不变量（idx < len/4，地址 4 字节对齐）；
                    //   `offset` 的 isize 偏移量远小于 isize::MAX（w*h ≤ u32::MAX 量级）；
                    // - 无别名：`&mut self` 保证映射在写期间无 Rust 侧别名；`src` 也不可能
                    //   与 `self.map` 指向同一内存——`FbCanvas` 的公开 API 从未暴露指向该映射
                    //   的 `&[Color]`（`pixel` 只按值返回），且 `map` 为私有裸指针，安全代码
                    //   无法据此构造切片（若未来源码新增此类出口，本前提即需重新评估）。
                    // - 读范围恰好用满 `src` 时（`src.len() == width*height`）最大读索引仍为
                    //   len-1，不会读到切片末尾之外。
                    unsafe {
                        let s = src.as_ptr().add(sy * width as usize + sx);
                        for k in 0..n {
                            *base.offset(dst + k as isize) = *s.add(k);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ⚠️ **B3-2b-1 删除的四条用例**（与其被测符号**专属**，故一并删；每条都点名落点）：
    //
    // | 已删用例 | 被测符号 | 语义现由谁覆盖 |
    // |----------|----------|----------------|
    // | `rgb_packs_rrggbb` | `rgb()`（已删） | `ui/tests.rs::theme_matches_ui_spec`（`Palette` 逐色值锚定 `#RRGGBB`） |
    // | `blend_over_half_coverage` | `blend_over()`（已删） | **无**：字形混合已归 LVGL 渲染器（v2.0 不自绘字形）⇒ 不再有需要断言的本地混合算术 |
    // | `offscreen_rect_fill_and_clip` / `offscreen_blit_pixels` | `OffscreenCanvas`（已删） | `screen.rs::tests` 的 `memsink_writes_area_pixels_1to1` / `blit_clips_area_partially_outside_target`（**同一个语义，v2.0 的出口 = `MemorySink` + `Blitter`，且走的是生产同一条 flush 路径**） |

    /// `Rect` 的**裁剪口径**（左闭右开 + 越界裁剪）—— `FbCanvas::blit_pixels` 唯一依赖的
    /// `Rect` 行为（**本文件唯一保留的纯逻辑**）。
    ///
    /// **改什么会让本条变红**：把 `clipped_to` 的两处判据（`x0 < x1`、`y0 < y1`）写成 `<=`
    /// —— **任一处单独改也会红**（下面**各有**一条"零宽 / 零高但另一维非空"的用例，
    /// 见 `Rect::new(3, 3, 3, 7)` 与 `Rect::new(3, 3, 7, 3)`；只留 `Rect::new(3,3,3,3)`
    /// 那种"两维同时退化"的用例时，两处判据**互相掩护** ⇒ 单改一处仍绿，评审已实测）；
    /// 以及把 `clipped_to` 的边界 `min(w)` 改成 `min(w - 1)`（右缘少写一列像素）。
    #[test]
    fn rect_clips_to_canvas_half_open() {
        let r = Rect::new(10, 10, 20, 20);
        assert_eq!((r.width(), r.height()), (10, 10));
        assert!(r.contains(10, 10) && !r.contains(20, 20), "左闭右开");
        assert_eq!(r.clipped_to(64, 64), Some(r), "完全在画布内 ⇒ 原样");
        assert_eq!(
            r.clipped_to(16, 16),
            Some(Rect::new(10, 10, 16, 16)),
            "越界部分按画布尺寸裁剪"
        );
        assert_eq!(r.clipped_to(5, 64), None, "完全在画布外 ⇒ None（不得返回空矩形）");
        assert_eq!(Rect::new(3, 3, 3, 3).clipped_to(64, 64), None, "零面积 ⇒ None");
        // **两维分别退化**（`x0 == x1` / `y0 == y1`，另一维非空）—— 这两条把两处判据
        // **各自**钉住：缺了它们，`x0 < x1` 单独写成 `<=` 会返回"零宽矩形"而不被察觉。
        assert_eq!(
            Rect::new(3, 3, 3, 7).clipped_to(64, 64),
            None,
            "零宽（`x0 == x1`）⇒ None —— 不得因 `y0 < y1` 成立就返回 0 宽矩形"
        );
        assert_eq!(
            Rect::new(3, 3, 7, 3).clipped_to(64, 64),
            None,
            "零高（`y0 == y1`）⇒ None —— 不得因 `x0 < x1` 成立就返回 0 高矩形"
        );
        assert_eq!(Rect::new(-5, -5, 5, 5).clipped_to(64, 64), Some(Rect::new(0, 0, 5, 5)));
    }
}
