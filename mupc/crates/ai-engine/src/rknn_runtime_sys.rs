//! RKNN Runtime C API FFI 绑定
//!
//! 直接声明 librknnrt.so 的 C 函数，供 rknn_runtime.rs 高层接口调用。
//!
//! ## Feature Gate
//!
//! 当 `npu` feature 未启用时，所有 FFI 函数替换为返回错误的 stub，
//! 避免链接时找不到 `librknnrt.so`。

use std::os::raw::{c_char, c_int, c_void};

#[repr(C)]
pub struct rknn_input {
    pub index: u32,
    pub buf: *mut c_void,
    pub size: u32,
    pub pass_timestamp: c_int,
}

#[repr(C)]
pub struct rknn_output {
    pub buf: *mut c_void,
    pub size: u32,
    pub is_preallocated: c_int,
}

#[cfg(feature = "npu")]
#[link(name = "rknnrt")]
extern "C" {
    pub fn rknn_init(
        ctx: *mut u64,
        model_path: *const c_char,
        model_type: c_int,
        flag: c_int,
    ) -> c_int;

    pub fn rknn_inputs_set(ctx: u64, n: u32, inputs: *mut rknn_input) -> c_int;

    pub fn rknn_run(ctx: u64, reserved: *mut u64) -> c_int;

    pub fn rknn_outputs_get(ctx: u64, n: u32, outputs: *mut rknn_output) -> c_int;

    pub fn rknn_destroy(ctx: u64) -> c_int;

    pub fn rknn_query(ctx: u64, cmd: c_int, info: *mut c_void, size: u32) -> c_int;
}

// ── 无 NPU feature 时的 stub 实现 ──
#[cfg(not(feature = "npu"))]
#[allow(non_snake_case)]
pub unsafe fn rknn_init(
    _ctx: *mut u64,
    _model_path: *const c_char,
    _model_type: c_int,
    _flag: c_int,
) -> c_int {
    -1 // 未启用 NPU
}

#[cfg(not(feature = "npu"))]
#[allow(non_snake_case)]
pub unsafe fn rknn_inputs_set(
    _ctx: u64, _n: u32, _inputs: *mut rknn_input,
) -> c_int { -1 }

#[cfg(not(feature = "npu"))]
#[allow(non_snake_case)]
pub unsafe fn rknn_run(_ctx: u64, _reserved: *mut u64) -> c_int { -1 }

#[cfg(not(feature = "npu"))]
#[allow(non_snake_case)]
pub unsafe fn rknn_outputs_get(
    _ctx: u64, _n: u32, _outputs: *mut rknn_output,
) -> c_int { -1 }

#[cfg(not(feature = "npu"))]
#[allow(non_snake_case)]
pub unsafe fn rknn_destroy(_ctx: u64) -> c_int { -1 }

#[cfg(not(feature = "npu"))]
#[allow(non_snake_case)]
pub unsafe fn rknn_query(
    _ctx: u64, _cmd: c_int, _info: *mut c_void, _size: u32,
) -> c_int { -1 }
