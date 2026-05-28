//! RKNN Runtime C API FFI 绑定
//!
//! 直接声明 librknnrt.so 的 C 函数，供 rknn_runtime.rs 高层接口调用

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
