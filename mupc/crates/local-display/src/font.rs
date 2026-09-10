//! 文本/字形渲染：ab_glyph（真实字体）栅格化 + 内置 ASCII 最小字体回退（无字库时的容错路径）。
//!
//! ## 字体资源结论（本仓库当前不随代码提交中文字体文件）
//!
//! 对齐 `[DESIGN_APPROVED]` 设计 §5.4/§9/§13 前置项 8 + UI §6.6：
//! - **加载顺序**：
//!   1. 启动参数 `--font <path>` 指向外部/系统字库（含 `/usr/share/fonts/...`）——设计 §7.2；
//!   2. cargo feature `bundled-font`：`include_bytes!("../fonts/NotoSansSC-subset.otf")`
//!      **编译进二进制**（设计 D2b：运行期零文件依赖、测试可复现）。仓库未入库该 .otf，
//!      启用 feature 前须按下方「子集化命令」生成，否则该 feature 下编译失败（属预期）。
//!   3. 以上皆不可得（开发/无字库环境）→ **内置 ASCII 最小 5x7 位图回退**：数字/`.-%: /`/
//!      大写拉丁字母能画；其余码位（含中文）画 **空心占位盒** 并 `eprintln` 明示「无字形」。
//!      → 缺失字库 **不会 panic**，布局照常推进（占位盒可度量、可离屏断言区域被绘制）。
//! - **中文字体部署前需生成的子集文件**：以 OFL 中文黑体（文泉驿微米黑 / Noto Sans SC，宿主已装
//!   `pyftsubset`/fonttools）对下述「全屏用字」+ ASCII 子集化后，产物放
//!   `crates/local-display/fonts/NotoSansSC-subset.otf`（预期 100KB~500KB）。
//!
//! ## 子集化命令（开发直接执行；码表 = UI §6.6 全屏用字）
//! ```text
//! # 用字常量见下方 REQ_TEXT（中文全列表）与 REQ_ASCII（拉丁/符号）；按需取并集后：
//! pyftsubset  <源字体>  \
//!   --output-file=crates/local-display/fonts/NotoSansSC-subset.otf \
//!   --text="$(REQ_TEXT 拼接)" \
//!   --layout-features='*' --glyph-names --symbol-cmap --legacy-cmap \
//!   --notdef-glyph --notdef-outline --recommended-glyphs
//! ```
//! 校验：渲染进程以 `--font fonts/NotoSansSC-subset.otf` 离屏跑一遍，遍历 §6.6 文案断言无「占位盒」。

use ab_glyph::{Font as _, FontArc, Glyph, PxScale, ScaleFont as _, point};
use crate::canvas::{Canvas, Rect, blend_over};

/// 渲染端捆绑子集字库相对路径（feature `bundled-font` 编译期 include_bytes）。
/// 未启用该 feature 时本常量不被引用（正常路径走 `--font` 外部字库或 ASCII 回退）。
#[cfg_attr(not(feature = "bundled-font"), allow(dead_code))]
const BUNDLED_FONT_REL: &str = "../fonts/NotoSansSC-subset.otf";

/// **全屏用字表**（中文+标点，UI §6.6；子集化码表 + 文案走查基线，需与 layout 使用文案保持同步）。
pub const REQ_TEXT: &str = "\
台区储能装置运行状态  实时 已连接 正在连接数据通道 与主进程数据通道断开 冻结 \
储能电池 双源一致 BMS PCS 源失效 双源皆失效 数据过期 过期 \
运行状态 充电 放电 停机 待机 离线 状态未知 方向一致 方向不一致 与状态一致 \
三相输出 相 总 单位 数值如实读 方向随 停更 有功功率 电流 总有功 设备 \
未取数 源离线 数据异常 显示进程异常 初始化中";

/// **非中文字形纳入子集**（UI §6.6）：拉丁 + 数字/符号 + 几何/箭头。本常量供命令拼码表用。
pub const REQ_ASCII: &str = "MUPCSOCRAGEWB0123456789.:–·/%Σ!?×▲▼●○■❚✕-+ ";

// ---------------------------------------------------------------------------
// TextKit：字体对象（真实 ab_glyph 或内置 ASCII 回退）
// ---------------------------------------------------------------------------

/// 文本渲染器。`font: Some` → ab_glyph 真字形；`None` → 内置 ASCII 位图回退 + 占位盒。
pub struct TextKit {
    font: Option<FontArc>,
}

impl Default for TextKit {
    fn default() -> Self {
        Self::fallback()
    }
}

impl std::fmt::Debug for TextKit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextKit")
            .field("real_font", &self.font.is_some())
            .finish()
    }
}

impl TextKit {
    /// 内置 ASCII 回退渲染器（无任何真实字库文件依赖）。
    pub fn fallback() -> Self {
        Self { font: None }
    }

    /// 按「外部路径 → bundled-font feature → 回退」加载。加载失败不 panic，回退并返回说明。
    pub fn load(font_path: Option<&std::path::Path>) -> Self {
        if let Some(p) = font_path {
            return Self::load_from_path(p);
        }
        #[cfg(feature = "bundled-font")]
        {
            let data: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/", BUNDLED_FONT_REL));
            match FontArc::try_from_vec(data.to_vec()) {
                Ok(f) => {
                    eprintln!("[local-display] 捆绑子集字库已加载（{} bytes）", data.len());
                    return Self { font: Some(f) };
                }
                Err(e) => {
                    eprintln!(
                        "[local-display] 捆绑子集字库非法，回退内置 ASCII：{}",
                        e
                    );
                }
            }
        }
        eprintln!(
            "[local-display] 无可用中文字库（未指定 --font 且未启用 bundled-font feature）\
             —— 中文将显示占位盒，数字/拉丁用内置 ASCII 回退。部署需按 font.rs 子集化命令生成字库。"
        );
        Self::fallback()
    }

    fn load_from_path(p: &std::path::Path) -> Self {
        match std::fs::read(p) {
            Err(e) => {
                eprintln!("[local-display] 字体文件 {p:?} 读取失败({e})，回退内置 ASCII");
                Self::fallback()
            }
            Ok(data) => match FontArc::try_from_vec(data) {
                Ok(f) => {
                    eprintln!("[local-display] 外部字库 {p:?} 已加载");
                    Self { font: Some(f) }
                }
                Err(e) => {
                    eprintln!("[local-display] 字体 {p:?} 解析失败({e})，回退内置 ASCII");
                    Self::fallback()
                }
            },
        }
    }

    pub fn is_real(&self) -> bool {
        self.font.is_some()
    }

    /// 是否有可用真实字形（影响 layout 是否渲染某字形 / 是否提示缺字库）。
    pub fn has_glyph(&self) -> bool {
        self.font.is_some()
    }

    // -- 度量 -----------------------------------------------------------------

    /// ascent（px）——由顶到基线的正数高度；`baseline = top + ascent`。
    pub fn ascent(&self, size_px: f32) -> f32 {
        match &self.font {
            Some(f) => f.as_scaled(PxScale::from(size_px)).ascent(),
            None => fallback_cell(size_px) * 7.0,
        }
    }

    /// descent（px，负向下为正数的小数）。
    pub fn descent(&self, size_px: f32) -> f32 {
        match &self.font {
            Some(f) => -f.as_scaled(PxScale::from(size_px)).descent(),
            None => fallback_cell(size_px),
        }
    }

    /// 由文本块 top 求 baseline。
    pub fn baseline_for_top(&self, top: f32, size_px: f32) -> f32 {
        top + self.ascent(size_px)
    }

    /// 横向 advance 宽度（px）——用于水平居中。
    pub fn measure(&self, text: &str, size_px: f32) -> f32 {
        match &self.font {
            Some(f) => {
                let sf = f.as_scaled(PxScale::from(size_px));
                let mut w = 0.0f32;
                for ch in text.chars() {
                    w += sf.h_advance(f.glyph_id(ch));
                }
                w
            }
            None => text.chars().count() as f32 * fallback_advance(size_px),
        }
    }

    /// 以 `(x, baseline_y)` 为起笔横向绘制整串文本。
    pub fn draw(
        &self,
        c: &mut dyn Canvas,
        text: &str,
        x: f32,
        baseline_y: f32,
        size_px: f32,
        color: crate::canvas::Color,
    ) {
        match &self.font {
            Some(f) => self.draw_real(f, c, text, x, baseline_y, size_px, color),
            None => self.draw_fallback(c, text, x, baseline_y, size_px, color),
        }
    }

    /// 以块顶对齐绘制（`top = 文本顶`），等价于 `baseline = top + ascent`。
    pub fn draw_top(
        &self,
        c: &mut dyn Canvas,
        text: &str,
        x: f32,
        top: f32,
        size_px: f32,
        color: crate::canvas::Color,
    ) {
        let baseline = self.baseline_for_top(top, size_px);
        self.draw(c, text, x, baseline, size_px, color);
    }

    /// 水平居中绘制于 `[cx]` 轴（以块顶 `top` 对齐）。
    pub fn draw_centered_top(
        &self,
        c: &mut dyn Canvas,
        text: &str,
        cx: f32,
        top: f32,
        size_px: f32,
        color: crate::canvas::Color,
    ) {
        let w = self.measure(text, size_px);
        self.draw_top(c, text, cx - w / 2.0, top, size_px, color);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_real(
        &self,
        f: &FontArc,
        c: &mut dyn Canvas,
        text: &str,
        x: f32,
        baseline_y: f32,
        size_px: f32,
        color: crate::canvas::Color,
    ) {
        let scale = PxScale::from(size_px);
        let sf = f.as_scaled(scale);
        let mut pen_x = x;
        for ch in text.chars() {
            let gid = f.glyph_id(ch);
            if let Some(outline) = f.outline_glyph(Glyph {
                id: gid,
                scale,
                position: point(pen_x, baseline_y),
            }) {
                let b = outline.px_bounds();
                let (bw, bh) = (b.width() as i32, b.height() as i32);
                outline.draw(|gx, gy, cov| {
                    if cov <= 0.0 {
                        return;
                    }
                    let px = b.min.x as i32 + gx as i32;
                    let py = b.min.y as i32 + gy as i32;
                    if px < 0 || py < 0 || px >= c.width() as i32 || py >= c.height() as i32 {
                        return;
                    }
                    if cov >= 1.0 {
                        c.set_px(px, py, color);
                    } else if let Some(bg) = c.pixel(px, py) {
                        c.set_px(px, py, blend_over(bg, color, cov));
                    }
                    let _ = (bw, bh);
                });
            }
            pen_x += sf.h_advance(gid);
        }
    }

    fn draw_fallback(
        &self,
        c: &mut dyn Canvas,
        text: &str,
        x: f32,
        baseline_y: f32,
        size_px: f32,
        color: crate::canvas::Color,
    ) {
        let cell = fallback_cell(size_px);
        let adv = fallback_advance(size_px);
        let top = baseline_y - cell * 7.0;
        let cx = cell.ceil() as i32;
        let mut pen = x;
        for ch in text.chars() {
            match fallback_bitmap(ch) {
                Some(rows) => {
                    for (r, row) in rows.iter().enumerate() {
                        for col in 0..5 {
                            if (row >> (4 - col)) & 1 == 1 {
                                let x0 = pen.floor() as i32 + col * cx;
                                let y0 = top.floor() as i32 + r as i32 * cx;
                                // 画该单元块（小字号时 cx=1 → 单像素）
                                for dy in 0..cx {
                                    for dx in 0..cx {
                                        c.set_px(x0 + dx, y0 + dy, color);
                                    }
                                }
                            }
                        }
                    }
                }
                None => {
                    // 无字形：画空心占位盒（可度量、非 panic）
                    let x0 = pen.floor() as i32;
                    let y0 = top.floor() as i32;
                    let x1 = x0 + 5 * cx;
                    let y1 = y0 + 7 * cx;
                    c.fill_rect(&Rect::new(x0, y0, x1, y0 + cx.max(1)), color); // 顶
                    c.fill_rect(&Rect::new(x0, y1 - cx.max(1), x1, y1), color); // 底
                    c.fill_rect(&Rect::new(x0, y0, x0 + cx.max(1), y1), color); // 左
                    c.fill_rect(&Rect::new(x1 - cx.max(1), y0, x1, y1), color); // 右
                }
            }
            pen += adv;
        }
    }
}

// -- 内置 ASCII 5x7 回退字形 --------------------------------------------------

/// 回退网格：以字号为整盒高（8 行格、7 行字），像素块宽 = ceil(size/8)。
fn fallback_cell(size_px: f32) -> f32 {
    (size_px / 8.0).max(1.0)
}

/// 每字符 advance = 5 列宽 + 1 间距格。
fn fallback_advance(size_px: f32) -> f32 {
    fallback_cell(size_px) * 6.0
}

/// 返回某字符的 7 行 × 5 列位图（位 4..0 = 列 左..右）。`None` → 占位盒。
fn fallback_bitmap(ch: char) -> Option<&'static [u8; 7]> {
    let t: &'static [u8; 7] = match ch {
        '0' => &[0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => &[0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => &[0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => &[0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => &[0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => &[0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => &[0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => &[0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => &[0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => &[0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        '.' => &[0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        '-' => &[0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '%' => &[0b11001, 0b11001, 0b00010, 0b00100, 0b01000, 0b10011, 0b10011],
        ':' => &[0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000],
        '/' => &[0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b00000, 0b00000],
        ' ' => &[0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        '+' => &[0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        'A' => &[0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => &[0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => &[0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => &[0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => &[0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => &[0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => &[0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => &[0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => &[0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => &[0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => &[0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => &[0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => &[0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => &[0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => &[0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => &[0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => &[0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => &[0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => &[0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => &[0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => &[0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => &[0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => &[0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => &[0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => &[0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => &[0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        _ => return None,
    };
    Some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{OffscreenCanvas, rgb};

    #[test]
    fn fallback_never_panics_and_draws_digits() {
        let kit = TextKit::fallback();
        let mut cv = OffscreenCanvas::new(400, 200);
        cv.clear(rgb(0, 0, 0));
        // 无字库：数字照画、中文/未知 → 占位盒，不 panic
        kit.draw_top(&mut cv, "65 充电", 10.0, 10.0, 48.0, 0xFFFFFF);
        let used = Rect::new(0, 0, 400, 200);
        assert!(cv.has_non_background(&used, 0));
    }

    #[test]
    fn fallback_measure_matches_draw_advance() {
        let kit = TextKit::fallback();
        let w = kit.measure("1234", 64.0);
        // 4 字符 × advance(ceil(64/8)*6 = 8*6=48) = 192
        assert!((w - 192.0).abs() < 0.001);
    }

    #[test]
    fn ascent_is_positive() {
        let kit = TextKit::fallback();
        assert!(kit.ascent(64.0) > 0.0);
        assert!(kit.ascent(148.0) > 0.0);
    }

    #[test]
    fn real_font_optional_load_from_system_path() {
        // 仅当本机存在真实 ASCII 字库时校验 ab_glyph 全路径（可复现，缺文件则跳过不失败）。
        // Windows 用系统 arial.ttf；Linux 常见 Noto/Sans 路径。与中文无关（只测 ASCII 栅格化）。
        let candidates = if cfg!(windows) {
            ["C:\\Windows\\Fonts\\arial.ttf", "C:\\Windows\\Fonts\\Arial.ttf"]
        } else {
            [
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            ]
        };
        let Some(existing) = candidates.iter().find(|p| std::path::Path::new(p).exists()) else {
            eprintln!("[font-test] 无系统字库，跳过真实 ab_glyph 路径校验（本机非问题）");
            return;
        };
        let kit = TextKit::load(Some(std::path::Path::new(existing)));
        assert!(kit.is_real(), "应能加载 {existing}");
        let mut cv = OffscreenCanvas::new(400, 200);
        cv.clear(rgb(0, 0, 0));
        kit.draw_top(&mut cv, "65.0%", 10.0, 10.0, 64.0, 0xFFFFFF);
        assert!(
            cv.has_non_background(&Rect::new(0, 0, 400, 200), 0),
            "ab_glyph 路径应能画出 ASCII"
        );
    }

    #[test]
    fn load_nonexistent_path_falls_back_without_panic() {
        let kit = TextKit::load(Some(std::path::Path::new("no/such/font.otf")));
        assert!(!kit.is_real());
        // 回退可正常度量与绘制
        assert!(kit.measure("12", 32.0) > 0.0);
        let mut cv = OffscreenCanvas::new(100, 100);
        cv.clear(0);
        kit.draw_top(&mut cv, "12", 0.0, 0.0, 32.0, 0xFFFFFF);
    }
}
