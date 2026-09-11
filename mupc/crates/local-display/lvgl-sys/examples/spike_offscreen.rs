//! S-2 离屏渲染 proof（+ S-3 字形判别）：把 LVGL 整条链路在本机跑通并做像素断言。
//!
//! 链路（对齐设计 §1.1.1.1 P-1）：`lv_init` → tick → `lv_display_create`
//! → PARTIAL 双缓冲 → `flush_cb`（本 spike 的 sink 是内存 `Vec<u8>`，生产是 `/dev/fb0`）
//! → 建对象/label → 渲染 → 断言缓冲里出现非背景像素。
//!
//! 运行：
//!   export LIBCLANG_PATH='C:\Program Files\LLVM\bin'
//!   cargo run -p lvgl-sys --example spike_offscreen -j 2
//!   cargo run -p lvgl-sys --example spike_offscreen --features noto-font -j 2   # S-3
//!
//! ⚠️ 本文件用了 `lv_refr_now()` —— 设计 §5.2 不变量 2 明确「**生产路径禁用**，
//!    仅测试用」。此处是 spike 测试，故允许；生产 HMI 的渲染只由 `lv_timer_handler()`
//!    驱动。

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use lvgl_sys::*;

const W: i32 = 1024;
const H: i32 = 768;
const BPP: usize = 4; // LV_COLOR_DEPTH 32 → XRGB8888（内存序 B,G,R,X）

/// 设计 §10：PARTIAL 双缓冲建议 2 × 1/10 屏 ≈ 2 × 314 KB（1024×768×4 / 10）。
const BUF_PIXELS: usize = (W as usize * H as usize) / 10;

/// 屏幕背景色（小端内存序 B,G,R）。
const BG: [u8; 4] = [0x18, 0x10, 0x10, 0x00];
/// 卡片背景色。
const CARD_BG: [u8; 4] = [0x3D, 0x2B, 0x22, 0x00];

const LABEL_TEXT: &str = "储能电池 SOC 92%";

static TICK0: OnceLock<Instant> = OnceLock::new();
static SINK: OnceLock<Mutex<Vec<u8>>> = OnceLock::new();
static FLUSH_CALLS: AtomicU32 = AtomicU32::new(0);

/// Rust 单调毫秒 → LVGL tick（设计 §1.1.1.1「主循环与 tick 契约」）。
unsafe extern "C" fn tick_get_cb() -> u32 {
    TICK0.get_or_init(Instant::now).elapsed().as_millis() as u32
}

/// flush_cb：把 LVGL 渲染好的脏区**就地搬进内存 sink**。
/// 生产版把 `sink` 换成 `FbCanvas::blit(area, px_map)`（写 /dev/fb0），逻辑同构。
unsafe extern "C" fn flush_cb(disp: *mut lv_display_t, area: *const lv_area_t, px_map: *mut u8) {
    let area = &*area;
    let w = (area.x2 - area.x1 + 1) as usize;
    let h = (area.y2 - area.y1 + 1) as usize;
    let mut sink = SINK.get().unwrap().lock().unwrap();
    let src = std::slice::from_raw_parts(px_map, w * h * BPP);
    for row in 0..h {
        let dy = area.y1 as usize + row;
        let dx = area.x1 as usize;
        let dst_off = (dy * W as usize + dx) * BPP;
        sink[dst_off..dst_off + w * BPP].copy_from_slice(&src[row * w * BPP..(row + 1) * w * BPP]);
    }
    FLUSH_CALLS.fetch_add(1, Ordering::Relaxed);
    lv_display_flush_ready(disp);
}

fn main() {
    let t0 = Instant::now();
    TICK0.set(Instant::now()).ok();
    SINK.set(Mutex::new(fill_screen(BG))).ok();

    let (label_area, using_noto);
    // 绘制缓冲必须在 LVGL 可能触碰它们的整个周期内存活。
    let mut buf1 = vec![0u8; BUF_PIXELS * BPP];
    let mut buf2 = vec![0u8; BUF_PIXELS * BPP];
    unsafe {
        // ── 1. 初始化 ─────────────────────────────────────────────
        lv_init();
        lv_tick_set_cb(Some(tick_get_cb));

        // ── 2. display：PARTIAL 双缓冲 + flush 桥 ─────────────────
        let disp = lv_display_create(W, H);
        assert!(!disp.is_null(), "lv_display_create returned NULL");
        lv_display_set_default(disp);

        lv_display_set_buffers(
            disp,
            buf1.as_mut_ptr() as *mut _,
            buf2.as_mut_ptr() as *mut _,
            (BUF_PIXELS * BPP) as u32,
            LV_DISPLAY_RENDER_MODE_PARTIAL,
        );
        lv_display_set_flush_cb(disp, Some(flush_cb));

        // ── 3. 控件树：屏幕（已知背景）→ 容器 → 中文 label ─────────
        let screen = lv_screen_active();
        set_bg(screen, BG);

        let card = lv_obj_create(screen);
        lv_obj_set_size(card, 900, 400);
        set_bg(card, CARD_BG);
        lv_obj_center(card);

        let label = lv_label_create(card);
        let text = std::ffi::CString::new(LABEL_TEXT).unwrap();
        lv_label_set_text(label, text.as_ptr());
        lv_label_set_long_mode(label, LV_LABEL_LONG_MODE_WRAP);
        lv_obj_set_style_text_color(
            label,
            lv_color_t {
                blue: 0xE8,
                green: 0xEF,
                red: 0xF4,
            },
            0,
        );

        using_noto = cfg!(feature = "noto-font");
        if using_noto {
            // S-3：挂上 lv_font_conv 生成的 CJK 子集字体
            lv_obj_set_style_text_font(
                label,
                std::ptr::addr_of!(lv_font_noto_sc_32) as *const lv_font_t,
                0,
            );
        } else {
            // 默认字体（ASCII）——中文将走 LV_USE_FONT_PLACEHOLDER 画方框
            lv_obj_set_style_text_font(
                label,
                std::ptr::addr_of!(lv_font_montserrat_14) as *const lv_font_t,
                0,
            );
        }
        lv_obj_center(label);

        // ── 4. 渲染 ───────────────────────────────────────────────
        // 正规路径：lv_timer_handler() 返回"距下次需要处理的时间(ms)"，
        // 生产事件循环把它作为 poll 超时上界（设计 §5.2 不变量 1）。
        let next_ms = lv_timer_handler();
        println!("lv_timer_handler() 返回: {next_ms} ms");
        // 测试专用强制整屏渲染 —— 生产路径禁用（设计 §5.2 不变量 2）。
        lv_refr_now(disp);

        let mut a = lv_area_t {
            x1: 0,
            y1: 0,
            x2: 0,
            y2: 0,
        };
        lv_obj_get_coords(label, &mut a);
        label_area = a;
    }
    std::hint::black_box(&buf1);
    std::hint::black_box(&buf2);

    let elapsed = t0.elapsed();
    let sink = SINK.get().unwrap().lock().unwrap().clone();
    let total = W as usize * H as usize;
    let non_bg = count_non_bg(&sink, BG) as usize;
    let distinct = distinct_colors(&sink);
    let blobs_screen = count_blobs(&sink, Region::full(), 0, BG);

    let text_n = label_text_pixels(LABEL_TEXT) as usize;
    let (tw, th, blobs_text, ink_text) = text_region_stats(&sink, label_area);

    println!("── S-2 离屏渲染 proof ───────────────────────────────");
    println!("画布 {W}x{H} / PARTIAL 双缓冲 2 x {BUF_PIXELS} px ({BUF_PIXELS}*{BPP} B)");
    println!("字体: {}", if using_noto { "lv_font_noto_sc_32 (lv_font_conv 子集)" } else { "默认 lv_font_montserrat_14（ASCII，中文将是缺字占位）" });
    println!("flush_cb 调用次数: {}", FLUSH_CALLS.load(Ordering::Relaxed));
    println!("耗时: {elapsed:?}");
    println!("非背景像素: {non_bg} / {total}");
    println!("不同颜色数: {distinct}");
    println!("全屏连通域数: {blobs_screen}");
    println!("label 文本区: {}x{} @ ({},{})", tw, th, label_area.x1, label_area.y1);
    println!("文本区前景像素: {ink_text}");
    println!("文本区连通域数: {blobs_text}（期望 >= 字数 {text_n}）");
    dump_art(&sink, label_area);

    let mut fail = Vec::new();
    if non_bg == 0 {
        fail.push("缓冲里没有任何非背景像素 —— 渲染链路没通");
    }
    if ink_text == 0 {
        fail.push("label 文本区一个前景像素都没有 —— 控件画了但文字没画");
    }
    if using_noto {
        // 设计 §1.1.3：字形连通区域数量 ≈ 字数（豆腐块会明显偏离）
        if blobs_text < text_n {
            fail.push("文本区连通域数 < 字数 —— 疑似豆腐块/缺字");
        }
    }
    if !fail.is_empty() {
        eprintln!("FAIL:");
        for f in &fail {
            eprintln!("  - {f}");
        }
        std::process::exit(1);
    }
    println!("PASS: 内存 sink 里画出了东西（非背景像素 {non_bg}，文本区连通域 {blobs_text}）");
}

/// `&str` 的字符数（用于期望连通域数）。
fn label_text_pixels(s: &str) -> u32 {
    s.chars().count() as u32
}

fn fill_screen(c: [u8; 4]) -> Vec<u8> {
    let mut v = vec![0u8; W as usize * H as usize * BPP];
    for px in v.chunks_exact_mut(BPP) {
        px.copy_from_slice(&c);
    }
    v
}

fn set_bg(obj: *mut lv_obj_t, c: [u8; 4]) {
    unsafe {
        lv_obj_set_style_bg_color(
            obj,
            lv_color_t {
                blue: c[0],
                green: c[1],
                red: c[2],
            },
            0,
        );
        lv_obj_set_style_bg_opa(obj, 255, 0);
    }
}

fn near(a: u8, b: u8) -> bool {
    (a as i32 - b as i32).abs() <= 8
}

fn is_bg(px: &[u8], bg: [u8; 4]) -> bool {
    near(px[0], bg[0]) && near(px[1], bg[1]) && near(px[2], bg[2])
}

fn count_non_bg(sink: &[u8], bg: [u8; 4]) -> u32 {
    sink.chunks_exact(BPP).filter(|px| !is_bg(px, bg)).count() as u32
}

fn distinct_colors(sink: &[u8]) -> usize {
    let mut set = std::collections::HashSet::new();
    for px in sink.chunks_exact(BPP) {
        set.insert((px[0], px[1], px[2]));
    }
    set.len()
}

struct Region {
    x1: usize,
    y1: usize,
    x2: usize,
    y2: usize,
}

impl Region {
    fn full() -> Self {
        Region {
            x1: 0,
            y1: 0,
            x2: W as usize - 1,
            y2: H as usize - 1,
        }
    }
}

/// 4-邻域连通域计数（迭代 BFS，避免深递归爆栈）。
fn count_blobs(sink: &[u8], region: Region, min_ink: u32, bg: [u8; 4]) -> usize {
    let w = W as usize;
    let mut mask = vec![false; w * H as usize];
    for y in region.y1..=region.y2 {
        for x in region.x1..=region.x2 {
            let i = y * w + x;
            mask[i] = !is_bg(&sink[i * BPP..i * BPP + BPP], bg);
        }
    }
    let _ = min_ink;
    let mut seen = vec![false; mask.len()];
    let mut blobs = 0usize;
    let mut stack: Vec<usize> = Vec::with_capacity(4096);
    for y in region.y1..=region.y2 {
        for x in region.x1..=region.x2 {
            let start = y * w + x;
            if !mask[start] || seen[start] {
                continue;
            }
            blobs += 1;
            stack.push(start);
            seen[start] = true;
            while let Some(i) = stack.pop() {
                let cx = i % w;
                let cy = i / w;
                let push = |j: usize, seen: &mut Vec<bool>, stack: &mut Vec<usize>| {
                    if mask[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                };
                if cx > 0 {
                    push(i - 1, &mut seen, &mut stack);
                }
                if cx + 1 < w {
                    push(i + 1, &mut seen, &mut stack);
                }
                if cy > 0 {
                    push(i - w, &mut seen, &mut stack);
                }
                if cy + 1 < H as usize {
                    push(i + w, &mut seen, &mut stack);
                }
            }
        }
    }
    blobs
}

/// 文本区统计：前景像素数 + 连通域数（判别"真字形 vs 豆腐块"）。
fn text_region_stats(sink: &[u8], a: lv_area_t) -> (i32, i32, usize, usize) {
    let region = Region {
        x1: a.x1.max(0) as usize,
        y1: a.y1.max(0) as usize,
        x2: a.x2.clamp(0, W - 1) as usize,
        y2: a.y2.clamp(0, H - 1) as usize,
    };
    let ink = count_non_bg_in(sink, &region, CARD_BG);
    let blobs = count_blobs(sink, region, 0, CARD_BG);
    (
        a.x2 - a.x1 + 1,
        a.y2 - a.y1 + 1,
        blobs,
        ink as usize,
    )
}

fn count_non_bg_in(sink: &[u8], region: &Region, bg: [u8; 4]) -> u32 {
    let w = W as usize;
    let mut n = 0;
    for y in region.y1..=region.y2 {
        for x in region.x1..=region.x2 {
            let i = y * w + x;
            if !is_bg(&sink[i * BPP..i * BPP + BPP], bg) {
                n += 1;
            }
        }
    }
    n
}

/// 把文本区降采样打印成 ASCII 图 —— 给人眼确认「是不是真的汉字」。
fn dump_art(sink: &[u8], a: lv_area_t) {
    let x1 = a.x1.max(0) as usize;
    let y1 = a.y1.max(0) as usize;
    let x2 = a.x2.clamp(0, W - 1) as usize;
    let y2 = a.y2.clamp(0, H - 1) as usize;
    let step = 2usize; // 2x2 降采样
    println!("── 文本区 ASCII 预览（# = 有墨，. = 卡片底色）──");
    let mut y = y1;
    while y <= y2 {
        let mut line = String::new();
        let mut x = x1;
        while x <= x2 {
            let mut ink = 0;
            for dy in 0..step {
                for dx in 0..step {
                    let (yy, xx) = (y + dy, x + dx);
                    if yy <= y2 && xx <= x2 {
                        let i = yy * W as usize + xx;
                        if !is_bg(&sink[i * BPP..i * BPP + BPP], CARD_BG) {
                            ink += 1;
                        }
                    }
                }
            }
            line.push(if ink > 0 { '#' } else { '.' });
            x += step;
        }
        println!("{line}");
        y += step;
    }
}
