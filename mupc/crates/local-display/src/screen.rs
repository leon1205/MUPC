//! `flush_cb` 的**像素 sink**（工作单元 C；设计 §1.1.1.1 **P-1** / §1.3 / §8.3）。
//!
//! # 职责（唯一：像素搬运）
//!
//! LVGL 在 [`crate::lvgl::timer_handler`] 内把脏区渲染进 PARTIAL 缓冲，然后回调
//! [`crate::lvgl::display::Display::set_flush_cb`] 注册的 sink：`sink(area, px)`。
//! 本模块把该脏区的 **LVGL 32bpp 像素字节**转成 [`crate::canvas::Color`]（`u32`）后写入
//! 目标（[`PixelSink`]）：
//!
//! | 后端 | 目标 | 说明 |
//! |------|------|------|
//! | `--backend fb0`（真机） | [`crate::canvas::fbdev::FbCanvas`] | 复用 v1.0 的 `/dev/fb0` mmap + 格式探测（设计 §8.1/§8.3「保留」，升为 flush 的 sink） |
//! | `--backend offscreen`（本机/CI） | [`MemorySink`] | 内存 `Vec<u32>`，可回读断言 / 导出 PPM —— **与真机共用同一条 flush 路径**（设计 §1.3） |
//!
//! # 纪律（评审逐条查）
//!
//! - [`Blitter::blit`] 由 flush 回调内调用 ⇒ **只做像素搬运**：不得阻塞、不得做通道 I/O、
//!   不得再调 LVGL（设计 §5.2 不变量 3）。
//! - **不得 panic**（跨 FFI 展开为 UB，设计 §5.2 不变量 5）：越界/尺寸不符一律**提前返回**，
//!   宁可少画一行也不越界读写（`px` 长度、`area` 与目标尺寸全部经显式校验）。
//! - 本模块**不引用 `lvgl_sys`**（unsafe 边界纪律 1）：只用薄安全层的公开值类型
//!   [`Area`] 与 [`crate::canvas::Canvas`]。
//!
//! # 像素格式契约（真机首验项）
//!
//! `lv_conf.h` 固定 `LV_COLOR_DEPTH 32` ⇒ LVGL 缓冲为 4 字节/像素、内存序 `B,G,R,X`
//! （`lv_color32_t`）。本模块按 `u32::from_ne_bytes` 逐像素装箱后交给
//! [`crate::canvas::Color`]（与 `canvas.rs` 的 `0x00RRGGBB` 打包一致）—— 小端机上即
//! **字节恒等搬运**（`B,G,R,X` 原样落 fb）。`/dev/fb0` 若为 16bpp 或跨距含 padding，
//! 需按 `fb_var_screeninfo` 校准：属设计 §13 前置项 1 / R-03（真机首验，本机无法验证）。
//!
//! # 已留痕偏离：PPM 替代 PNG（评审 C-④）
//!
//! 设计 §12.3 的「可选导出」写的是 **PNG**；本实现导出 **PPM(P6)**（[`MemorySink::write_ppm`]）。
//! 原因：PNG 需引入编码 crate（违背设计 §5.1「依赖刻意保持最小」），PPM 是**零依赖、无损**、
//! Linux 图像工具可直接查看的等价替代。评审判定**可接受**。**如需 PNG，须引入依赖或自写编码器**
//! （本轮不引依赖；需先走依赖评审）。

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

use crate::canvas::Color;
use crate::lvgl::display::{Area, BYTES_PER_PIXEL};

/// flush sink 的下游目标：一块可写的 `w × h` 像素面。
///
/// 实现者只需保证「按行写入时不越界」；[`Blitter`] 已把区域裁剪到目标范围内，
/// 但实现侧仍应自行裁剪（防御重复裁剪，成本是一次比较）。
pub trait PixelSink {
    /// 目标宽度（像素）。
    fn width(&self) -> u32;
    /// 目标高度（像素）。
    fn height(&self) -> u32;
    /// 写入一行（或一块）像素：`px` 长度 = `w * h`，起点 `(x, y)`。越界部分应被忽略。
    fn write_pixels(&mut self, x: i32, y: i32, w: u32, h: u32, px: &[Color]);
}

/// [`Rc`] 包装的 sink：与调用方**共享**同一块像素面（Minor 3 整改）。
///
/// 存在的理由：[`Blitter::into_flush_closure`] 会把 sink **move** 进闭包，接线之后
/// 离屏验证（回读单像素 / 导出 PPM）就再也够不着它 —— 而那恰是 [`MemorySink`] 的主要用途。
///
/// **回调内绝不 panic**（跨 FFI 展开 = UB，设计 §5.2 不变量 5）：目标被外部借走时
/// `width()/height()` 返回 0（[`Blitter::blit`] 据此判为「目标无效」而跳过本拍），
/// `write_pixels` 静默跳过 —— 宁可少画一帧，也不炸在回调里。
impl<S: PixelSink> PixelSink for Rc<RefCell<S>> {
    fn width(&self) -> u32 {
        self.try_borrow().map(|s| s.width()).unwrap_or(0)
    }

    fn height(&self) -> u32 {
        self.try_borrow().map(|s| s.height()).unwrap_or(0)
    }

    fn write_pixels(&mut self, x: i32, y: i32, w: u32, h: u32, px: &[Color]) {
        if let Ok(mut s) = self.try_borrow_mut() {
            s.write_pixels(x, y, w, h, px);
        }
    }
}

/// 一块 `w × h` 脏区需要的**像素字节数**（`w * h * 4`；**checked**，溢出返回 `None`）。
///
/// Minor 2 整改：原先直接 `area.pixel_bytes()`（内部 `w*h*4` 的 usize 乘法）做防御比较 ——
/// 乘法本身可溢出（32 位目标必然，64 位在极端量程下也可能），而本函数由 flush 回调内调用，
/// **回调内 panic = 跨 FFI 展开 = UB**（设计 §5.2 不变量 5）。溢出时按「宁可不画」处理。
fn required_pixel_bytes(w: u32, h: u32) -> Option<usize> {
    (w as usize)
        .checked_mul(h as usize)
        .and_then(|n| n.checked_mul(BYTES_PER_PIXEL))
}

/// 脏区搬运器：把 LVGL 的像素字节按行搬进 [`PixelSink`]。
///
/// 复用一块**单行**临时缓冲（`scratch`）：flush 回调每帧被调多次（按脏区），
/// 逐帧分配会带来无谓的堆抖动（设计 §10 的 CPU/内存预算）。
pub struct Blitter<T: PixelSink> {
    target: T,
    scratch: Vec<Color>,
    /// 累计搬运的脏区数（测试断言「每帧只搬有效脏区」用）。
    blits: u64,
    /// 因尺寸/长度不符被丢弃的脏区数（异常的观测哨；正常恒为 0）。
    dropped: u64,
}

impl<T: PixelSink> Blitter<T> {
    /// 以 `target` 为下游建搬运器（`scratch` 按需增长，初始空）。
    pub fn new(target: T) -> Self {
        Self {
            target,
            scratch: Vec::new(),
            blits: 0,
            dropped: 0,
        }
    }

    /// 目标只读访问（回读断言/统计用）。
    pub fn target(&self) -> &T {
        &self.target
    }

    /// 目标可变访问（测试构造场景用；**不得**在 flush 回调内额外触达 LVGL）。
    pub fn target_mut(&mut self) -> &mut T {
        &mut self.target
    }

    /// 搬运的脏区数。
    pub fn blits(&self) -> u64 {
        self.blits
    }

    /// 丢弃（尺寸不符）的脏区数。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// **flush 路径本体**：`area` 是 LVGL 的闭区间脏区，`px` 是其连续像素字节
    /// （长度 = `area.pixel_bytes()`，行间无 padding）。
    ///
    /// 语义：`(area.x1, area.y1)` 对应 `px[0..4]`；行跨距 = `area.width() * 4`。
    /// 越界（含目标外的部分脏区）按行裁剪；长度不足则整块丢弃并计数（不读越界）。
    pub fn blit(&mut self, area: Area, px: &[u8]) {
        let (aw, ah) = (area.width(), area.height());
        if aw == 0 || ah == 0 {
            return;
        }
        // 防御 1：LVGL 保证 `px.len() == area.width() * area.height() * 4`；不满足时宁可不画。
        // 用 `required_pixel_bytes`（checked）而非 `area.pixel_bytes()`：后者是**未检查**的
        // usize 乘法，溢出会 panic —— 而这里是 flush 回调内（panic 跨 FFI = UB）。
        match required_pixel_bytes(aw, ah) {
            Some(need) if px.len() >= need => {}
            _ => {
                self.dropped += 1;
                return;
            }
        }
        let tw = self.target.width() as i64;
        let th = self.target.height() as i64;
        if tw <= 0 || th <= 0 {
            self.dropped += 1;
            return;
        }
        // 防御 2：把脏区与目标求交（LVGL 通常会自行裁剪到屏内，此处不假设）。
        let x0 = (area.x1 as i64).max(0);
        let y0 = (area.y1 as i64).max(0);
        let x1 = (area.x2 as i64).min(tw - 1);
        let y1 = (area.y2 as i64).min(th - 1);
        if x0 > x1 || y0 > y1 {
            return; // 完全在屏外：无像素可搬（不计 dropped，属正常裁剪）
        }

        let row_w = (x1 - x0 + 1) as usize;
        if self.scratch.len() < row_w {
            self.scratch.resize(row_w, 0);
        }
        let stride = aw as usize;
        let sx = (x0 - area.x1 as i64) as usize;
        for y in y0..=y1 {
            let sy = (y - area.y1 as i64) as usize;
            let base = (sy * stride + sx) * BYTES_PER_PIXEL;
            // 逐像素装箱：`from_ne_bytes` 是 safe 且不要求 4 字节对齐
            // （LVGL 缓冲虽已 64 字节对齐，但裁剪后的行首偏移不保证对齐）。
            for i in 0..row_w {
                let b = &px[base + i * BYTES_PER_PIXEL..base + (i + 1) * BYTES_PER_PIXEL];
                self.scratch[i] = Color::from_ne_bytes([b[0], b[1], b[2], b[3]]);
            }
            self.target
                .write_pixels(x0 as i32, y as i32, row_w as u32, 1, &self.scratch[..row_w]);
        }
        self.blits += 1;
    }

    /// 生成 flush 闭包（交给 `Display::set_flush_cb`）。
    ///
    /// ⚠️ 闭包按 `FnMut` 调用、捕获 `self`（含 `scratch` 与目标句柄），
    /// 因此 `Blitter` 必须**与 display 同寿**（LVGL 在 display 删除时会 drop 该闭包）。
    ///
    /// ⚠️ 目标 `T` 被 **move** 进闭包 ⇒ 接线后再也回读不到它。离屏验证（回读 / 导出 PPM）
    /// 请用 [`Blitter::into_shared_flush_closure`]。
    pub fn into_flush_closure(mut self) -> impl FnMut(Area, &[u8]) + 'static
    where
        T: 'static,
    {
        move |area, px| self.blit(area, px)
    }

    /// 生成 flush 闭包**并同时返回共享句柄**（Minor 3 整改）—— 交给 `set_flush_cb`
    /// 之后仍能回读单像素 / 导出 PPM（离屏验证的主要用途）。
    ///
    /// 与 [`Blitter::into_flush_closure`] 的唯一差别：目标不再被 move 走，而是以
    /// [`Rc<RefCell<T>>`](Rc) 共享（单线程纪律不变：**本模块不提供跨线程 API**，
    /// 设计 §5.2 不变量 4）。
    ///
    /// 纪律不变：闭包仍**只搬像素、不阻塞、不调 LVGL**；目标被外部借用时静默跳过本拍
    /// （见 `Rc<RefCell<S>>` 的 [`PixelSink`] 实现），**不会**在 flush 回调里 panic。
    pub fn into_shared_flush_closure(
        self,
    ) -> (impl FnMut(Area, &[u8]) + 'static, Rc<RefCell<T>>)
    where
        T: PixelSink + 'static,
    {
        let Blitter { target, .. } = self;
        let shared = Rc::new(RefCell::new(target));
        let mut inner = Blitter::new(Rc::clone(&shared));
        (
            move |area: Area, px: &[u8]| inner.blit(area, px),
            shared,
        )
    }
}

/// 内存 sink（`--backend offscreen`；设计 §1.3/§12.3）：`w × h` 的 `Vec<Color>`。
///
/// 用途：本机/CI 无 `/dev/fb0` 时走**与真机同一条 flush 路径**做离屏断言
/// （可回读单像素、可导出 PPM 供人工核对）；真机路径仅把 sink 换成 `FbCanvas`。
pub struct MemorySink {
    w: u32,
    h: u32,
    buf: Vec<Color>,
    default_fill: Color,
}

impl MemorySink {
    /// 建 `w × h` 内存屏（零初始化 ⇒ 全黑）。
    pub fn new(w: u32, h: u32) -> Self {
        Self::with_fill(w, h, 0)
    }

    /// 指定「未写入区」的填充色后清零（人工核对时便于与背景区分）。
    pub fn new_filled(w: u32, h: u32, fill: Color) -> Self {
        Self::with_fill(w, h, fill)
    }

    /// 分配内部缓冲：`w * h` 用 **checked** 乘法（Minor 2 同源理由）—— 溢出时退化为
    /// **空缓冲**，而不是分配一块「比声明尺寸小」的缓冲（后者会让下标访问越界）。
    /// 空缓冲下所有访问都走 `.get()/.get_mut()` 与裁剪路径，不 panic、不越界写。
    fn with_fill(w: u32, h: u32, fill: Color) -> Self {
        let buf = match (w as usize).checked_mul(h as usize) {
            Some(n) => vec![fill; n],
            None => Vec::new(),
        };
        Self {
            w,
            h,
            buf,
            default_fill: fill,
        }
    }

    /// 未写入区的填充色。
    pub fn default_fill(&self) -> Color {
        self.default_fill
    }

    /// 回读单像素（越界返回 `None`，不 panic）。
    pub fn pixel(&self, x: i32, y: i32) -> Option<Color> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return None;
        }
        self.buf.get(y as usize * self.w as usize + x as usize).copied()
    }

    /// 全区像素（行主序，长度 = `w * h`）。
    pub fn pixels(&self) -> &[Color] {
        &self.buf
    }

    /// 区域内某颜色的像素数（离屏断言用；区域自动裁剪）。
    pub fn count_color(&self, area: Area, c: Color) -> usize {
        let mut n = 0;
        for y in area.y1.max(0)..=area.y2.min(self.h as i32 - 1) {
            for x in area.x1.max(0)..=area.x2.min(self.w as i32 - 1) {
                if self.pixel(x, y) == Some(c) {
                    n += 1;
                }
            }
        }
        n
    }

    /// 区域内是否存在非 `bg` 像素（「该区域已绘制」断言；区域自动裁剪）。
    pub fn has_non_background(&self, area: Area, bg: Color) -> bool {
        for y in area.y1.max(0)..=area.y2.min(self.h as i32 - 1) {
            for x in area.x1.max(0)..=area.x2.min(self.w as i32 - 1) {
                match self.pixel(x, y) {
                    Some(p) if p != bg => return true,
                    _ => {}
                }
            }
        }
        false
    }

    /// 导出 **PPM(P6)** 供人工核对（设计 §12.3 的「可选导出」）。
    ///
    /// 为什么不是 PNG（**已留痕偏离**，评审 C-④ 判可接受，详见模块文档）：PNG 需引入编码
    /// crate（依赖面纪律，设计 §5.1「依赖刻意保持最小」），PPM 是零依赖、无损、Linux 图像
    /// 工具可直接查看的等价替代。**如需 PNG 须引入依赖或自写编码器**（本轮不引依赖）。
    pub fn write_ppm(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
        write!(f, "P6\n{} {}\n255\n", self.w, self.h)?;
        let mut row = Vec::with_capacity(self.w as usize * 3);
        for y in 0..self.h as usize {
            row.clear();
            for x in 0..self.w as usize {
                // 缓冲可能因乘法溢出而为空（见 `with_fill`）⇒ 用 `.get()` 兜底成黑色，
                // 绝不直接下标（那会在导出路径上 panic）。
                let p = self
                    .buf
                    .get(y.saturating_mul(self.w as usize).saturating_add(x))
                    .copied()
                    .unwrap_or(0);
                row.push(((p >> 16) & 0xFF) as u8);
                row.push(((p >> 8) & 0xFF) as u8);
                row.push((p & 0xFF) as u8);
            }
            f.write_all(&row)?;
        }
        f.flush()
    }
}

impl PixelSink for MemorySink {
    fn width(&self) -> u32 {
        self.w
    }

    fn height(&self) -> u32 {
        self.h
    }

    fn write_pixels(&mut self, x: i32, y: i32, w: u32, h: u32, px: &[Color]) {
        if w == 0 || h == 0 {
            return;
        }
        // 防御：声明尺寸与切片长度不符 ⇒ 不写（不越界读 `px`）。
        if px.len() < w as usize * h as usize {
            return;
        }
        for dy in 0..h as i32 {
            let yy = y + dy;
            if yy < 0 || yy >= self.h as i32 {
                continue;
            }
            for dx in 0..w as i32 {
                let xx = x + dx;
                if xx < 0 || xx >= self.w as i32 {
                    continue;
                }
                let idx = yy as usize * self.w as usize + xx as usize;
                if let Some(dst) = self.buf.get_mut(idx) {
                    *dst = px[dy as usize * w as usize + dx as usize];
                }
            }
        }
    }
}

/// `/dev/fb0` sink：把 `canvas.rs::FbCanvas`（v1.0 保留资产，设计 §8.3）适配为 flush 下游。
#[cfg(target_os = "linux")]
impl PixelSink for crate::canvas::fbdev::FbCanvas {
    fn width(&self) -> u32 {
        crate::canvas::Canvas::width(self)
    }

    fn height(&self) -> u32 {
        crate::canvas::Canvas::height(self)
    }

    fn write_pixels(&mut self, x: i32, y: i32, w: u32, h: u32, px: &[Color]) {
        // `FbCanvas::blit_pixels` 自带 `src.len()` 校验与裁剪（见 canvas.rs O5 整改）。
        crate::canvas::Canvas::blit_pixels(self, x, y, w, h, px);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(x1: i32, y1: i32, x2: i32, y2: i32) -> Area {
        Area { x1, y1, x2, y2 }
    }

    /// 构造一块 2×2 脏区的 XRGB 字节：像素值 = 0xAARRGGBB 的 LE 字节序 `B,G,R,A`。
    fn px_bytes(colors: &[Color]) -> Vec<u8> {
        let mut v = Vec::with_capacity(colors.len() * 4);
        for c in colors {
            v.extend_from_slice(&c.to_ne_bytes());
        }
        v
    }

    const RED: Color = 0x00FF0000;
    const GREEN: Color = 0x0000FF00;
    const BLUE: Color = 0x000000FF;

    #[test]
    fn memsink_writes_area_pixels_1to1() {
        let mut b = Blitter::new(MemorySink::new(8, 4));
        // 2×2 脏区落在 (1,1)..(2,2)
        let px = px_bytes(&[RED, GREEN, BLUE, RED]);
        b.blit(area(1, 1, 2, 2), &px);
        let s = b.target();
        assert_eq!(s.pixel(1, 1), Some(RED));
        assert_eq!(s.pixel(2, 1), Some(GREEN));
        assert_eq!(s.pixel(1, 2), Some(BLUE));
        assert_eq!(s.pixel(2, 2), Some(RED));
        // 未触及的像素保持初值
        assert_eq!(s.pixel(0, 0), Some(0));
        assert_eq!(s.pixel(3, 3), Some(0));
        assert_eq!(b.blits(), 1);
        assert_eq!(b.dropped(), 0);
    }

    #[test]
    fn memsink_avg_pixel_row_stride_is_area_width() {
        // 3 宽 × 2 高脏区：第二行第 0 列取 px[3]（而非 px[6]）—— 行跨距 = area 宽度。
        let mut b = Blitter::new(MemorySink::new(16, 16));
        let px = px_bytes(&[RED, RED, RED, GREEN, GREEN, GREEN]);
        b.blit(area(5, 5, 7, 6), &px);
        let s = b.target();
        assert_eq!(s.pixel(5, 5), Some(RED));
        assert_eq!(s.pixel(7, 5), Some(RED));
        assert_eq!(s.pixel(5, 6), Some(GREEN));
        assert_eq!(s.pixel(7, 6), Some(GREEN));
        assert_eq!(s.pixel(4, 5), Some(0), "脏区左侧不得被写");
        assert_eq!(s.pixel(8, 6), Some(0), "脏区右侧不得被写");
    }

    /// 越界脏区（部分出屏）必须被裁剪且不 panic、不越界写。
    #[test]
    fn blit_clips_area_partially_outside_target() {
        let mut b = Blitter::new(MemorySink::new(4, 4));
        // 4×1 脏区，起始 x = 2 ⇒ 只有 x=2,3 可见
        let px = px_bytes(&[RED, GREEN, BLUE, 0x00FFFFFF]);
        b.blit(area(2, 0, 5, 0), &px);
        let s = b.target();
        assert_eq!(s.pixel(2, 0), Some(RED));
        assert_eq!(s.pixel(3, 0), Some(GREEN));
        assert_eq!(b.blits(), 1, "裁剪后仍有可见像素 ⇒ 计一次搬运");

        // 完全在屏外：无像素可搬，且不得 panic
        let before = b.blits();
        b.blit(area(10, 10, 12, 12), &px_bytes(&[RED; 9]));
        assert_eq!(b.blits(), before);
        // 负坐标脏区
        b.blit(area(-2, -2, -1, -1), &px_bytes(&[RED]));
        assert_eq!(b.target().pixel(0, 0), Some(0));
    }

    /// 像素字节数不足 / 空区域：丢弃且不读越界（不 panic）。
    #[test]
    fn blit_drops_short_or_empty_pixel_slice() {
        let mut b = Blitter::new(MemorySink::new(8, 8));
        b.blit(area(0, 0, 1, 1), &[0u8; 4]); // 需 16 字节，仅给 4
        assert_eq!(b.dropped(), 1);
        assert_eq!(b.blits(), 0);
        assert_eq!(b.target().pixel(0, 0), Some(0), "不足的脏区不得写入");

        // 空区域（x2 < x1 ⇒ width/height == 0）：**提前返回** —— 既不搬运、也不计 dropped，
        // 故 dropped 仍是上一次调用的 1（Minor 6：此前注释与断言口径自相矛盾）。
        b.blit(area(3, 3, 2, 2), &[]);
        assert_eq!(b.dropped(), 1, "空区域直接返回：dropped 不增（仍为上一行的 1）");
        assert_eq!(b.blits(), 0);
    }

    /// Minor 2 整改：脏区字节数用 **checked** 乘法（溢出 ⇒ `None` ⇒ 宁可不画，不 panic）。
    #[test]
    fn required_pixel_bytes_is_checked_not_wrapping() {
        assert_eq!(required_pixel_bytes(2, 2), Some(16));
        assert_eq!(required_pixel_bytes(0, 5), Some(0));
        assert_eq!(required_pixel_bytes(1, 1), Some(4));
        // 极端量程：`u32::MAX * u32::MAX * 4` 溢出 usize ⇒ None（旧口径会 panic/回绕）
        assert_eq!(required_pixel_bytes(u32::MAX, u32::MAX), None);
        assert_eq!(required_pixel_bytes(u32::MAX, u32::MAX / 2), None);
        // 不 panic 即达标（blit 侧据此走 dropped 分支）
        let mut b = Blitter::new(MemorySink::new(4, 4));
        b.blit(area(0, 0, 3, 3), &[0u8; 4]); // 需 64 字节，仅给 4
        assert_eq!(b.dropped(), 1);
        assert_eq!(b.blits(), 0);
    }

    /// Minor 3 整改：共享式 flush 闭包 ⇒ 接线后仍可回读 / 导出 PPM；借用冲突不 panic。
    #[test]
    fn shared_flush_closure_keeps_sink_readable_after_wiring() {
        let (mut f, handle) = Blitter::new(MemorySink::new(4, 2)).into_shared_flush_closure();
        f(area(1, 0, 2, 0), &px_bytes(&[RED, GREEN]));
        assert_eq!(handle.borrow().pixel(1, 0), Some(RED), "接线后仍可回读");
        assert_eq!(handle.borrow().pixel(2, 0), Some(GREEN));
        assert_eq!(handle.borrow().pixel(0, 0), Some(0));
        assert_eq!(handle.borrow().default_fill(), 0);

        // 导出 PPM（离屏验证的主要用途）在接线后依然可用
        let dir = std::env::temp_dir().join("mupc-local-display-screen-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("shared.ppm");
        handle.borrow().write_ppm(&p).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(&bytes[..11], b"P6\n4 2\n255\n");
        // 行主序：x=0 未写（黑）→ x=1 红 → x=2 绿 → x=3 未写
        assert_eq!(&bytes[11..14], &[0, 0, 0]);
        assert_eq!(&bytes[14..17], &[255, 0, 0]);
        assert_eq!(&bytes[17..20], &[0, 255, 0]);
        assert_eq!(&bytes[20..23], &[0, 0, 0]);
        let _ = std::fs::remove_file(&p);

        // 目标被外部借走时，flush 回调**静默跳过本拍**（不得 panic —— 回调内 panic 跨 FFI = UB）
        let guard = handle.borrow();
        f(area(0, 1, 0, 1), &px_bytes(&[BLUE]));
        drop(guard);
        assert_eq!(
            handle.borrow().pixel(0, 1),
            Some(0),
            "借用冲突时宁可不画（绝不 panic）"
        );
        // 借用释放后恢复正常搬运
        f(area(0, 1, 0, 1), &px_bytes(&[BLUE]));
        assert_eq!(handle.borrow().pixel(0, 1), Some(BLUE));
    }

    #[test]
    fn memsink_write_pixels_clips_and_validates() {
        let mut s = MemorySink::new(2, 2);
        // 声明 2×2 但只给 1 个像素 ⇒ 整块丢弃
        s.write_pixels(0, 0, 2, 2, &[RED]);
        assert_eq!(s.pixel(0, 0), Some(0));
        // 越界坐标被忽略
        s.write_pixels(1, 1, 3, 3, &[GREEN; 9]);
        assert_eq!(s.pixel(1, 1), Some(GREEN));
        assert_eq!(s.pixel(0, 0), Some(0));
    }

    #[test]
    fn memsink_helpers_count_and_probe_area() {
        let s = MemorySink::new_filled(4, 4, 0x00101010);
        assert_eq!(s.default_fill(), 0x00101010);
        let mut b = Blitter::new(s);
        b.blit(area(0, 0, 1, 1), &px_bytes(&[RED, RED, RED, RED]));
        let s = b.target();
        assert_eq!(s.count_color(area(0, 0, 3, 3), RED), 4);
        assert_eq!(s.count_color(area(0, 0, 0, 0), RED), 1);
        assert!(s.has_non_background(area(0, 0, 3, 3), 0x00101010));
        assert!(!s.has_non_background(area(2, 2, 3, 3), 0x00101010));
        assert_eq!(s.pixel(-1, 0), None);
        assert_eq!(s.pixel(0, 99), None);
        assert_eq!(s.pixels().len(), 16);
    }

    #[test]
    fn memsink_exports_ppm_header_and_body() {
        let mut s = MemorySink::new(2, 1);
        s.write_pixels(0, 0, 2, 1, &[0x00FF0000, 0x0000FF00]);
        let dir = std::env::temp_dir().join("mupc-local-display-screen-test");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("sink.ppm");
        s.write_ppm(&p).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(&bytes[..11], b"P6\n2 1\n255\n");
        // 行主序 RGB：红 → (255,0,0)；绿 → (0,255,0)
        assert_eq!(&bytes[11..], &[255, 0, 0, 0, 255, 0]);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn blitter_scratch_is_reused_across_blits() {
        let mut b = Blitter::new(MemorySink::new(64, 64));
        for i in 0..5 {
            let a = area(i, i, i + 9, i);
            b.blit(a, &px_bytes(&[RED; 10]));
        }
        assert_eq!(b.blits(), 5);
        assert_eq!(b.scratch.len(), 10, "scratch 只增长到最大行宽，不随调用次数增长");
    }

    /// 写入日志（`(x, y, w, h)` 逐条）。
    type WriteLog = std::rc::Rc<std::cell::RefCell<Vec<(i32, i32, u32, u32)>>>;

    /// 能写进下游的「探针 sink」：记录每次 `write_pixels` 的 (x,y,w,h)。
    struct ProbeSink {
        log: WriteLog,
        w: u32,
        h: u32,
    }

    impl PixelSink for ProbeSink {
        fn width(&self) -> u32 {
            self.w
        }
        fn height(&self) -> u32 {
            self.h
        }
        fn write_pixels(&mut self, x: i32, y: i32, w: u32, h: u32, _px: &[Color]) {
            self.log.borrow_mut().push((x, y, w, h));
        }
    }

    /// `into_flush_closure` 产出的闭包可直接作 `set_flush_cb` 的 sink：按行搬运、可反复调用。
    #[test]
    fn flush_closure_writes_rows_to_target() {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let probe = ProbeSink {
            log: std::rc::Rc::clone(&log),
            w: 64,
            h: 64,
        };
        let mut f = Blitter::new(probe).into_flush_closure();
        f(area(4, 7, 6, 8), &px_bytes(&[RED; 6])); // 3 宽 × 2 高
        f(area(0, 0, 0, 0), &px_bytes(&[BLUE]));
        assert_eq!(
            *log.borrow(),
            vec![(4, 7, 3, 1), (4, 8, 3, 1), (0, 0, 1, 1)],
            "闭包应按行搬运（行跨距 = 脏区宽度）"
        );
    }
}
