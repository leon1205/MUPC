/**
 * @file lv_conf.h
 * MUPC 本地显示终端（12 号模块）的 LVGL v9.5.0 配置。
 *
 * 对齐设计文档 §1.1.1.2（关键项初值）/ §10（预算口径说明）。
 * 本文件由 `cc` 通过 `-DLV_CONF_INCLUDE_SIMPLE` 被 LVGL 源码与 bindgen 共同引用，
 * 因此【编译产物层面的红线】在这里落地：
 *   LV_USE_TEXTAREA 0 / LV_USE_KEYBOARD 0 / LV_USE_SPINBOX 0
 *   —— 三者必须同时为 0（lv_spinbox 以 lv_textarea 为基类，启用即不可能关掉 textarea）。
 * 未在此显式定义的宏，由 vendor/lvgl/src/lv_conf_internal.h 提供默认值。
 */

#ifndef LV_CONF_H
#define LV_CONF_H

/*====================
   COLOR SETTINGS
 *====================*/
/* 设计 §1.1.1.2：XRGB8888。 */
#define LV_COLOR_DEPTH 32

/*=========================
   STDLIB WRAPPER SETTINGS
 *=========================*/
/* 用 LVGL 内置 malloc（内存池由 LV_MEM_SIZE 定容，便于预算核算）。 */
#define LV_USE_STDLIB_MALLOC    LV_STDLIB_BUILTIN
#define LV_USE_STDLIB_STRING    LV_STDLIB_BUILTIN
#define LV_USE_STDLIB_SPRINTF   LV_STDLIB_BUILTIN

/* 设计 §1.1.1.2 / §10：256 KB 起，实测后定稿。 */
#define LV_MEM_SIZE (256 * 1024U)
#define LV_MEM_POOL_EXPAND_SIZE 0

/*====================
   HAL SETTINGS
 *====================*/
#define LV_DEF_REFR_PERIOD  33      /* [ms] */
#define LV_DPI_DEF 130

/*=================
 * OPERATING SYSTEM
 *=================*/
/* 设计 §5.2 不变量 4：单线程，所有 LVGL 调用在事件循环线程内。 */
#define LV_USE_OS   LV_OS_NONE

/*=====================
 *  LOG
 *====================*/
/* 设计 §1.1.1.2：LV_USE_LOG 1（后续在薄层里把 log 回调转发到 Rust tracing）。 */
#define LV_USE_LOG 1
#if LV_USE_LOG
    #define LV_LOG_LEVEL LV_LOG_LEVEL_WARN
    #define LV_LOG_PRINTF 1
#endif

/*========================
 * RENDERING / DRAW
 *========================*/
/* 仅软件光栅化（RK3588 上不使用 GPU/NEMA/矢量加速后端）。 */
#define LV_USE_DRAW_SW 1
#define LV_DRAW_SW_SUPPORT_RGB888 1
#define LV_DRAW_SW_SUPPORT_XRGB8888 1

/*==================
 * FONT
 *==================*/
/* 缺字时画方框（"豆腐块"）—— S-2 用它对照，S-3 用真字形覆盖。 */
#define LV_USE_FONT_PLACEHOLDER 1

/*==============================
 * WIDGETS / THEMES（本模块用点）
 *==============================*/
#define LV_USE_LABEL 1
#define LV_USE_OBJ_ID 0

/* ── 硬性红线：不提供任何文本输入（T-1 裁定，设计 §10 预算口径说明）── */
#define LV_USE_TEXTAREA 0
#define LV_USE_KEYBOARD 0
#define LV_USE_SPINBOX 0

/* 默认主题仅作控件行为基线，最终外观一律由 ui/theme.rs 的 lv_style 覆盖（设计 §5.1）。 */
#define LV_USE_THEME_DEFAULT 1

/*==============================
 * DEVICES / BACKENDS（全关）
 *==============================*/
/* 设计 §1.1.1.1：显示走 P-1（自研 lv_display + flush_cb 直写 fb0），
 * 触摸走自研 indev（Rust evdev），因此 LVGL 自带的设备驱动全部关闭。 */
#define LV_USE_LINUX_FBDEV 0
#define LV_USE_LINUX_DRM 0
#define LV_USE_EVDEV 0
#define LV_USE_SDL 0
#define LV_USE_GLFW 0
#define LV_USE_X11 0
#define LV_USE_WAYLAND 0
#define LV_USE_WINDOWS 0
#define LV_USE_NUTTX 0
#define LV_USE_LINUX 0

#endif /* LV_CONF_H */
