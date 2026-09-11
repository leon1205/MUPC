//! LVGL v9.5.0 原始（unsafe）绑定。
//!
//! 内容全部来自 bindgen，按 `allowlist.txt` 精确生成（禁 `lv_*` 通配）。
//! **本 crate 不得被 `pages` / `state` / `channel` 等模块直接引用**——
//! 唯一合法用点是 `local-display/src/lvgl/` 薄安全层（设计 §1.1.1.2 unsafe 边界纪律 #1）。
//!
//! `build.rs` 已经 `cargo:rustc-link-lib=static=lvgl`，调用方无需再声明链接。

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

/// 记录一次构建的 LVGL 版本，供薄层/日志断言用。
pub const LVGL_VERSION: &str = "9.5.0";
