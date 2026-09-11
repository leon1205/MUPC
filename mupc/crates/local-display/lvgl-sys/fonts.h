/**
 * @file fonts.h
 * `lv_font_conv` 生成的位图字体的符号声明（设计 §1.1.2 / §12.3）。
 *
 * 生成物本体在 `crates/local-display/fonts/`（由 `fonts/gen_fonts.sh` 复现），
 * 由 lvgl-sys/build.rs 一并编入静态库；这里只做声明，供 bindgen 导出符号。
 * 本轮 spike 只生成 1 档（32 px）；正式编码期按 UI §3.3 补齐 24/48/64/96。
 */

#ifndef MUPC_LVGL_SYS_FONTS_H
#define MUPC_LVGL_SYS_FONTS_H

#include "lvgl.h"

extern const lv_font_t lv_font_noto_sc_32;

#endif /* MUPC_LVGL_SYS_FONTS_H */
