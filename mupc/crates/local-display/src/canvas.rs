//! 画布抽象 + 离屏实现（OffscreenCanvas）+ Linux 帧缓冲后端（FbCanvas）。
//!
//! 对齐 `[DESIGN_APPROVED]` 设计 §B1 / §5.2 canvas.rs / §6：
//! - 颜色：32-bit 不透明 `0x00RRGGBB`（alpha 留 0——屏面无合成，仅字形覆盖做 alpha 混合）。
//! - `Canvas` trait：宽高 / `set_px` / `fill_rect` / `clear`（绘制原语，layout 只面向 trait）。
//! - OffscreenCanvas：内存 `Vec<Color>` 缓冲，测试可对像素/区域断言；供 layout 离屏单测与
//!   `--backend offscreen` 全链路（可出 PNG——本期不引 png 依赖，后续 dev feature）。
//! - FbCanvas（仅 `target_os = "linux"`，`/dev/fb0` mmap）：Windows 本机不编译该 cfg 块；
//!   真机像素格式(bpp/order)/DRM 后端属设计 §13 前置项 1，实现按「32bpp XRGB 映射」，
//!   首验不符时切 `--fbdev-path` 或补 DRM dumb-buffer。

/// 颜色 = `0x00RRGGBB`（不透明）。
pub type Color = u32;

/// 便捷：由 RGB 通道构造颜色（0xRRGGBB）。
pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// 便捷：由 `#RRGGBB` 字面量常量构造（编译期）。
pub const fn hex(h: u32) -> Color {
    h
}

/// 把前景不透明色按 coverage 混合到背景上（字形反锯齿）。cov>=1 时直接取前景。
pub fn blend_over(bg: Color, fg: Color, cov: f32) -> Color {
    let cov = cov.clamp(0.0, 1.0);
    if cov >= 1.0 {
        return fg;
    }
    if cov <= 0.0 {
        return bg;
    }
    let bf = |b: u8, f: u8| {
        let bb = b as f32;
        let ff = f as f32;
        (bb + (ff - bb) * cov).round() as u32
    };
    let r = bf((bg >> 16) as u8, (fg >> 16) as u8);
    let g = bf((bg >> 8) as u8, (fg >> 8) as u8);
    let bl = bf(bg as u8, fg as u8);
    (r << 16) | (g << 8) | bl
}

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

/// 绘制画布抽象。layout 只依赖本 trait（离屏与帧缓冲共用同一套绘制命令）。
pub trait Canvas {
    fn width(&self) -> u32;
    fn height(&self) -> u32;

    /// 画单像素（越界自动忽略）。
    fn set_px(&mut self, x: i32, y: i32, c: Color);

    /// 读单像素（字形反锯齿混合需要背景色；越界/不支持读 → None）。默认 None。
    fn pixel(&self, x: i32, y: i32) -> Option<Color> {
        let _ = (x, y);
        None
    }

    /// 清屏。
    fn clear(&mut self, c: Color);

    /// 填充矩形（自动裁剪）。
    fn fill_rect(&mut self, r: &Rect, c: Color);

    /// 以 (x0,y0) 为左上，blit 一个不透明小位图（行宽 `stride`，仅低 `width` 字节有效）。
    /// 默认逐像素 `set_px`；连续缓冲实现可覆写为整行 memcpy。offset 按像素。
    fn blit_pixels(&mut self, x: i32, y: i32, width: u32, height: u32, src: &[Color]);
}

/// 通用矩形填充（复用默认实现，供各实现 `fill_rect` 兜底）。
pub fn fill_rect_default<C: Canvas + ?Sized>(c: &mut C, r: &Rect, color: Color) {
    if let Some(r) = r.clipped_to(c.width(), c.height()) {
        for yy in r.y0..r.y1 {
            for xx in r.x0..r.x1 {
                c.set_px(xx, yy, color);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// OffscreenCanvas —— 内存缓冲（离屏测试 + offscreen 后端）
// ---------------------------------------------------------------------------

/// 离屏内存画布（1024x768x4 ≈ 3MB）。`pixel`/`buffer` 供测试断言与未来 PNG 导出。
pub struct OffscreenCanvas {
    w: u32,
    h: u32,
    buf: Vec<Color>,
}

impl OffscreenCanvas {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            buf: vec![0; (w * h) as usize],
        }
    }

    /// 设计默认分辨率：1024x768（8 寸屏）。
    pub fn new_default() -> Self {
        Self::new(1024, 768)
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    pub fn buffer(&self) -> &[Color] {
        &self.buf
    }

    /// 统计 `rect` 内等于 `c` 的像素数（裁剪越界）。
    pub fn count_color(&self, r: &Rect, c: Color) -> usize {
        let mut n = 0;
        for yy in r.y0.max(0)..r.y1.min(self.h as i32) {
            for xx in r.x0.max(0)..r.x1.min(self.w as i32) {
                if self.buf[(yy as u32 * self.w + xx as u32) as usize] == c {
                    n += 1;
                }
            }
        }
        n
    }

    /// `rect` 内是否存在非背景色像素（用于断言某区域被绘制过）。
    pub fn has_non_background(&self, r: &Rect, bg: Color) -> bool {
        for yy in r.y0.max(0)..r.y1.min(self.h as i32) {
            for xx in r.x0.max(0)..r.x1.min(self.w as i32) {
                if self.buf[(yy as u32 * self.w + xx as u32) as usize] != bg {
                    return true;
                }
            }
        }
        false
    }
}

impl Canvas for OffscreenCanvas {
    fn width(&self) -> u32 {
        self.w
    }
    fn height(&self) -> u32 {
        self.h
    }
    fn set_px(&mut self, x: i32, y: i32, c: Color) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = (y as u32 * self.w + x as u32) as usize;
        self.buf[i] = c;
    }
    fn pixel(&self, x: i32, y: i32) -> Option<Color> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            None
        } else {
            Some(self.buf[(y as u32 * self.w + x as u32) as usize])
        }
    }
    fn clear(&mut self, c: Color) {
        self.buf.fill(c);
    }
    fn fill_rect(&mut self, r: &Rect, c: Color) {
        if let Some(r) = r.clipped_to(self.w, self.h) {
            let w = self.w as usize;
            let s = (r.y0 as usize) * w + (r.x0 as usize);
            let n = (r.x1 - r.x0) as usize;
            for yy in 0..(r.y1 - r.y0) as usize {
                let start = s + yy * w;
                self.buf[start..start + n].fill(c);
            }
        }
    }
    fn blit_pixels(&mut self, x: i32, y: i32, width: u32, height: u32, src: &[Color]) {
        // O5：`src` 短于声明尺寸 → 直接返回（否则下方切片越界 panic）。
        if src.len() < (width as usize).saturating_mul(height as usize) {
            return;
        }
        let r = Rect::new(x, y, x + width as i32, y + height as i32);
        if let Some(clip) = r.clipped_to(self.w, self.h) {
            let src_w = width as usize;
            for yy in clip.y0..clip.y1 {
                let sy = (yy - y) as usize;
                let sx0 = (clip.x0 - x) as usize;
                let sx1 = (clip.x1 - x) as usize;
                let dst0 = (yy as u32 * self.w + clip.x0 as u32) as usize;
                let dst1 = (yy as u32 * self.w + clip.x1 as u32) as usize;
                self.buf[dst0..dst1]
                    .copy_from_slice(&src[sy * src_w + sx0..sy * src_w + sx1]);
            }
        }
    }
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

    // mmap 区指针在多线程只经 &mut self 使用；标记 Send/Sync 以便 run 循环持有。
    unsafe impl Send for FbCanvas {}
    unsafe impl Sync for FbCanvas {}

    impl FbCanvas {
        /// 打开帧缓冲设备并按 `w×h×4`（32bpp）mmap。返回前会清零整个显存。
        pub fn open(path: &str, w: u32, h: u32) -> crate::Result<Self> {
            let cpath = std::ffi::CString::new(path)
                .map_err(|_| crate::Error::Backend("fb path contains NUL".into()))?;
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
                    let _ = unsafe { libc::close(fd) };
                    crate::Error::Backend("fb size overflow".into())
                })?;
            // O3：mmap 前校验显存实际长度（`smem_len`）≥ 请求长度——不足即报错，
            // 杜绝「mmap 成功但越界写 SIGBUS」。（ioctl 只填结构体，不触碰显存。）
            let mut fix: FbFixScreenInfo = unsafe { std::mem::zeroed() };
            let rc = unsafe { libc::ioctl(fd, FBIOGET_FSCREENINFO as _, &mut fix as *mut FbFixScreenInfo) };
            if rc < 0 {
                let e = std::io::Error::last_os_error();
                unsafe { libc::close(fd) };
                return Err(crate::Error::Backend(format!(
                    "FBIOGET_FSCREENINFO {path} failed: {e}——无法校验显存长度，拒绝映射"
                )));
            }
            let smem_len = fix.smem_len as usize;
            if smem_len < len {
                unsafe { libc::close(fd) };
                return Err(crate::Error::Backend(format!(
                    "{path} 显存 {smem_len} 字节 < 需要 {len} 字节（{w}x{h} 32bpp）——拒绝越界映射"
                )));
            }
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
                unsafe { libc::close(fd) };
                return Err(crate::Error::Backend(format!("mmap {path} failed: {e}")));
            }
            // 清零显存（首帧前避免残影）。
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
        fn set_px(&mut self, x: i32, y: i32, c: Color) {
            if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
                return;
            }
            let idx = y as usize * self.w as usize + x as usize;
            unsafe {
                *(self.map as *mut Color).add(idx) = c;
            }
        }
        fn pixel(&self, x: i32, y: i32) -> Option<Color> {
            if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
                return None;
            }
            let idx = y as usize * self.w as usize + x as usize;
            unsafe { Some(*(self.map as *const Color).add(idx)) }
        }
        fn clear(&mut self, c: Color) {
            unsafe {
                let p = self.map as *mut Color;
                for i in 0..(self.w as usize * self.h as usize) {
                    *p.add(i) = c;
                }
            }
        }
        fn fill_rect(&mut self, r: &Rect, c: Color) {
            fill_rect_default(self, r, c);
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

    const RED: Color = 0x00_FF_00_00;

    #[test]
    fn rgb_packs_rrggbb() {
        assert_eq!(rgb(0xFF, 0x00, 0x00), 0xFF0000);
        assert_eq!(rgb(0x0B, 0x12, 0x20), 0x0B1220);
    }

    #[test]
    fn blend_over_half_coverage() {
        // 黑底(0) 上白字(0xFFFFFF) 50% → 约 0x808080
        let c = blend_over(0x000000, 0xFFFFFF, 0.5);
        assert!(c > 0x7F7F7F && c < 0x818181);
        assert_eq!(blend_over(0x000000, 0xFFFFFF, 1.0), 0xFFFFFF);
        assert_eq!(blend_over(0x000000, 0xFFFFFF, 0.0), 0x000000);
    }

    #[test]
    fn offscreen_rect_fill_and_clip() {
        let mut cv = OffscreenCanvas::new(64, 64);
        cv.clear(rgb(0, 0, 0));
        cv.fill_rect(&Rect::new(10, 10, 20, 20), RED);
        assert_eq!(cv.pixel(10, 10), Some(RED));
        assert_eq!(cv.pixel(19, 19), Some(RED));
        assert_eq!(cv.pixel(20, 20), Some(0));
        // 越界矩形裁剪
        cv.fill_rect(&Rect::new(0, 0, 70, 70), 0xFFFFFF);
        assert_eq!(cv.pixel(63, 63), Some(0xFFFFFF));
        assert_eq!(cv.count_color(&Rect::new(0, 0, 64, 64), 0xFFFFFF), 64 * 64);
    }

    #[test]
    fn offscreen_blit_pixels() {
        let mut cv = OffscreenCanvas::new(32, 32);
        cv.clear(0);
        let src = vec![RED, RED, RED, RED]; // 2x2
        cv.blit_pixels(1, 1, 2, 2, &src);
        assert_eq!(cv.pixel(2, 2), Some(RED));
        assert_eq!(cv.pixel(0, 0), Some(0));
    }
}
