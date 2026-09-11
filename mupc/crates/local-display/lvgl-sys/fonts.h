/**
 * @file fonts.h
 * `lv_font_conv` 生成的位图字体的符号声明（设计 §1.1.2 / §12.3）。
 *
 * 生成物本体在 `crates/local-display/fonts/`（由 `fonts/gen_fonts.sh` 复现），
 * 由 lvgl-sys/build.rs 一并编入静态库；这里只做声明，供 bindgen 导出符号。
 * 档位 = UI 设计 §3.3 字号阶梯 / 设计 §1.1.2 的 10 档
 * （24 26 28 32 48 56 64 96 112 148）。
 *
 * 全量声明 10 档：Rust 侧 `src/lvgl/font.rs` 的 `Font::of(档位)` 逐档取用（工作单元 A2）；
 * 生成物缺失时，启用 `noto-font` feature 会在 build.rs 得到**可读的编译期报错**，
 * 不退化为晦涩的链接错误（设计 §1.1.2）。
 */

#ifndef MUPC_LVGL_SYS_FONTS_H
#define MUPC_LVGL_SYS_FONTS_H

#include "lvgl.h"

extern const lv_font_t lv_font_noto_sc_24;
extern const lv_font_t lv_font_noto_sc_26;
extern const lv_font_t lv_font_noto_sc_28;
extern const lv_font_t lv_font_noto_sc_32;
extern const lv_font_t lv_font_noto_sc_48;
extern const lv_font_t lv_font_noto_sc_56;
extern const lv_font_t lv_font_noto_sc_64;
extern const lv_font_t lv_font_noto_sc_96;
extern const lv_font_t lv_font_noto_sc_112;
extern const lv_font_t lv_font_noto_sc_148;

#endif /* MUPC_LVGL_SYS_FONTS_H */
