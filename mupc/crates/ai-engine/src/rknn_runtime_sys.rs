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

// ⚠️ 判据是构建脚本发出的 `rknn_real_rt`（见 build.rs：npu ∧ linux ∧ **aarch64**）。
// Rockchip 只发布 aarch64 的 `librknnrt.so`
// （项目 build.rs 的自动探测也只在 `rknpu2/runtime/Linux/librknn_api/aarch64/` 下找）。
// 少了这条，x86_64 宿主（本机开发 / CI 的 test+lint job）会去链接 aarch64 的 .so
// ⇒ `rust-lld: ... is incompatible with elf64-x86-64`，整条 CI 流水线结构性不可跑。
// x86_64 目标走下面的 stub（与 Windows 同口径：npu 语义保留，运行时返回 -1）。
#[cfg(rknn_real_rt)]
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

// ── 其余情形（非 Linux / 非 aarch64 / 未启用 npu）的 stub 实现 ──
//
// # Safety
//
// 以下各 stub 与真 FFI 的签名逐一对齐（便于调用方无差别编译），但**不触碰任何指针**：
// 一律直接返回 `-1`（"未启用 NPU"）。因此调用它们不会产生任何内存安全义务 —— 指
// 参即便悬垂/为空也仅被忽略。保留 `unsafe` 仅因签名必须与真实现一致。
/// stub：未启用 NPU（非 Linux / 非 aarch64 / feature 未开），恒返回 `-1`。
///
/// # Safety
/// 不产生任何内存安全义务 —— 指针参数被忽略（即便悬垂/为空）。保留 `unsafe` 仅为与真
/// FFI 的签名逐一对齐，使调用方无需按平台分叉。
#[cfg(not(rknn_real_rt))]
#[allow(non_snake_case)]
pub unsafe fn rknn_init(
    _ctx: *mut u64,
    _model_path: *const c_char,
    _model_type: c_int,
    _flag: c_int,
) -> c_int {
    -1 // 未启用 NPU
}

/// stub：未启用 NPU（非 Linux / 非 aarch64 / feature 未开），恒返回 `-1`。
///
/// # Safety
/// 不产生任何内存安全义务 —— 指针参数被忽略（即便悬垂/为空）。保留 `unsafe` 仅为与真
/// FFI 的签名逐一对齐，使调用方无需按平台分叉。
#[cfg(not(rknn_real_rt))]
#[allow(non_snake_case)]
pub unsafe fn rknn_inputs_set(_ctx: u64, _n: u32, _inputs: *mut rknn_input) -> c_int {
    -1
}

/// stub：未启用 NPU（非 Linux / 非 aarch64 / feature 未开），恒返回 `-1`。
///
/// # Safety
/// 不产生任何内存安全义务 —— 指针参数被忽略（即便悬垂/为空）。保留 `unsafe` 仅为与真
/// FFI 的签名逐一对齐，使调用方无需按平台分叉。
#[cfg(not(rknn_real_rt))]
#[allow(non_snake_case)]
pub unsafe fn rknn_run(_ctx: u64, _reserved: *mut u64) -> c_int {
    -1
}

/// stub：未启用 NPU（非 Linux / 非 aarch64 / feature 未开），恒返回 `-1`。
///
/// # Safety
/// 不产生任何内存安全义务 —— 指针参数被忽略（即便悬垂/为空）。保留 `unsafe` 仅为与真
/// FFI 的签名逐一对齐，使调用方无需按平台分叉。
#[cfg(not(rknn_real_rt))]
#[allow(non_snake_case)]
pub unsafe fn rknn_outputs_get(_ctx: u64, _n: u32, _outputs: *mut rknn_output) -> c_int {
    -1
}

/// stub：未启用 NPU（非 Linux / 非 aarch64 / feature 未开），恒返回 `-1`。
///
/// # Safety
/// 不产生任何内存安全义务 —— 指针参数被忽略（即便悬垂/为空）。保留 `unsafe` 仅为与真
/// FFI 的签名逐一对齐，使调用方无需按平台分叉。
#[cfg(not(rknn_real_rt))]
#[allow(non_snake_case)]
pub unsafe fn rknn_destroy(_ctx: u64) -> c_int {
    -1
}

/// stub：未启用 NPU（非 Linux / 非 aarch64 / feature 未开），恒返回 `-1`。
///
/// # Safety
/// 不产生任何内存安全义务 —— 指针参数被忽略（即便悬垂/为空）。保留 `unsafe` 仅为与真
/// FFI 的签名逐一对齐，使调用方无需按平台分叉。
#[cfg(not(rknn_real_rt))]
#[allow(non_snake_case)]
pub unsafe fn rknn_query(_ctx: u64, _cmd: c_int, _info: *mut c_void, _size: u32) -> c_int {
    -1
}

/// 本二进制**是否编入真实 NPU 推理** —— 这是**构建期事实**，不是运行期配置。
///
/// 仅当 `npu` feature 开启 **且** 目标是 linux+aarch64 时为 `true`（此时才链接真实
/// `librknnrt.so`）；其余情形编译的是 stub（`rknn_*` 一律返回 -1）。
///
/// 用途：启动期与配置项 `.ai_engine.enable_npu` 对账并显式告警 —— NPU 自 2026-09-19 起是
/// **构建期开关**，配置文件里写 `enable_npu: true` 并不会让 stub 二进制长出推理能力。
pub const NPU_BUILD_ENABLED: bool = cfg!(all(
    feature = "npu",
    target_os = "linux",
    target_arch = "aarch64"
));
