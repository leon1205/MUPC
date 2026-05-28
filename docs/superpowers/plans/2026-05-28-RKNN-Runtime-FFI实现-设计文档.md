# MUPC Phase 3C.2 技术设计文档 - RKNN Runtime FFI 实现

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-28 | 架构师 | ✅ 已批准 |

---

[DESIGN_APPROVED]

---

## 1. 概述

### 1.1 设计目标

本设计文档描述 RKNN Runtime FFI 实现的技术方案，将 Rust 异步运行时与 `librknnrt.so` C 库进行桥接，实现在 RK3588 NPU 上的 AI 模型推理能力。

### 1.2 架构位置

```
┌─────────────────────────────────────────────────────────────┐
│                     MUPC AI Engine                          │
├─────────────────────────────────────────────────────────────┤
│  rknn_runtime.rs (高层接口)                                  │
│       │                                                     │
│       ▼                                                     │
│  rknn_runtime_sys.rs (FFI 绑定)  ←─────────────────────────│
│       │                        librknnrt.so (C 库)           │
└───────┼─────────────────────────────────────────────────────┘
        │
        ▼
┌──────────────────┐
│   RK3588 NPU     │
└──────────────────┘
```

---

## 2. 模块结构

```
mupc/crates/ai-engine/src/
├── rknn_runtime.rs      # 高层接口（已存在，需修改）
├── rknn_runtime_sys.rs  # C API 绑定（新建）
├── rknn_types.rs        # 类型定义（新建）
└── error.rs             # 错误类型（已存在）
```

---

## 3. 核心模块设计

### 3.1 rknn_runtime_sys.rs - C API 绑定

**职责**：声明 `librknnrt.so` 的 C 函数接口

```rust
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

    pub fn rknn_inputs_set(
        ctx: u64,
        n: u32,
        inputs: *mut rknn_input,
    ) -> c_int;

    pub fn rknn_run(
        ctx: u64,
        reserved: *mut u64,
    ) -> c_int;

    pub fn rknn_outputs_get(
        ctx: u64,
        n: u32,
        outputs: *mut rknn_output,
    ) -> c_int;

    pub fn rknn_destroy(ctx: u64) -> c_int;

    pub fn rknn_query(ctx: u64, cmd: c_int, info: *mut c_void, size: u32) -> c_int;
}
```

### 3.2 rknn_types.rs - 类型定义

**职责**：定义 Rust 原生类型，封装 C 结构的内存布局

```rust
use std::os::raw::c_void;

/// RKNN 输入张量
#[derive(Debug, Clone)]
pub struct RknnInput {
    pub index: u32,
    pub buf: Vec<u8>,
    pub pass_timestamp: c_int,  // 使用 c_int 与 C API 保持一致
}

impl RknnInput {
    pub fn new(index: u32, buf: Vec<u8>) -> Self {
        Self {
            index,
            buf,
            pass_timestamp: 0,  // 默认不传递时间戳
        }
    }
}

/// RKNN 输出张量
#[derive(Debug)]
pub struct RknnOutput {
    pub buf: Vec<u8>,
}

impl RknnOutput {
    /// 安全地将输出缓冲区转换为 f32 数组
    /// 使用 align_to 处理 Vec<u8> 可能不满足 4 字节对齐的情况
    pub fn as_f32(&self) -> Vec<f32> {
        let (prefix, aligned, suffix) = self.buf.align_to::<f32>();
        let mut result = Vec::with_capacity(
            prefix.len() / 4 + aligned.len() + suffix.len() / 4
        );
        // 处理前缀中 4 字节对齐的部分
        for chunk in prefix.chunks_exact(4) {
            result.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        // 直接拷贝已对齐的数据
        result.extend(aligned.iter());
        // 处理后缀中 4 字节对齐的部分
        for chunk in suffix.chunks_exact(4) {
            result.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        result
    }
}
```

### 3.3 rknn_runtime.rs - 高层接口

**职责**：提供线程安全、异步封装的 RKNN Runtime 接口

```rust
use std::ffi::CString;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::task;

use crate::error::AiEngineError;
use crate::rknn_runtime_sys::{self, rknn_input, rknn_output};

/// RKNN 上下文（RAII 资源管理）
struct RknnContext {
    ctx: u64,
    input_count: u32,
    output_count: u32,
}

impl Drop for RknnContext {
    fn drop(&mut self) {
        unsafe { rknn_runtime_sys::rknn_destroy(self.ctx) }
    }
}

/// RKNN Runtime 推理器
pub struct RknnRuntime {
    model_path: std::path::PathBuf,
    ctx: Arc<RwLock<Option<RknnContext>>>,
}

impl RknnRuntime {
    /// 创建推理器
    pub fn new(model_path: &Path) -> Result<Self, AiEngineError> {
        Ok(Self {
            model_path: model_path.to_path_buf(),
            ctx: Arc::new(RwLock::new(None)),
        })
    }

    /// 加载模型（异步）
    pub async fn load(&self) -> Result<(), AiEngineError> {
        let model_path = self.model_path.clone();
        let ctx = self.ctx.clone();

        task::spawn_blocking(move || {
            let mut ctx_handle: u64 = 0;

            // 使用 CString 确保 null-terminated 字符串
            let c_path = CString::new(model_path.to_string_lossy().as_bytes())?;
            let ret = unsafe {
                rknn_runtime_sys::rknn_init(
                    &mut ctx_handle,
                    c_path.as_ptr(),
                    0,  // RKNN_MODEL_TYPE_UNKNOWN
                    0,  // flag
                )
            };

            map_rknn_error(ret)?;

            // 使用 rknn_query 查询输入输出数量
            #[repr(C)]
            struct RknnQueryInOut {
                n_input: u32,
                n_output: u32,
            }
            let mut query_info = RknnQueryInOut { n_input: 0, n_output: 0 };
            let ret = unsafe {
                rknn_runtime_sys::rknn_query(
                    ctx_handle,
                    0,  // RKNN_CMD_QUERY_INPUT_OUTPUT_NUM
                    &mut query_info as *mut _ as *mut c_void,
                    std::mem::size_of::<RknnQueryInOut>() as u32,
                )
            };
            // 查询失败时使用默认值 1，仅支持单输入单输出场景
            let (input_count, output_count) = if ret == 0 {
                (query_info.n_input, query_info.n_output)
            } else {
                (1, 1)
            };

            let context = RknnContext {
                ctx: ctx_handle,
                input_count,
                output_count,
            };

            *ctx.write().unwrap() = Some(context);
            Ok(())
        })
        .await
        .map_err(|e| AiEngineError::InferenceFailed(e.to_string()))?
    }

    /// 执行推理（异步）
    pub async fn run(&self, input: &[f32]) -> Result<Vec<f32>, AiEngineError> {
        let ctx_guard = self.ctx.read().unwrap();
        let context = ctx_guard.as_ref().ok_or(AiEngineError::ModelNotLoaded)?;

        let input_len = input.len() * std::mem::size_of::<f32>();
        let mut input_buf = input.to_vec();

        let ret = task::spawn_blocking(move || {
            let mut rknn_in = rknn_input {
                index: 0,
                buf: input_buf.as_mut_ptr() as *mut c_void,
                size: input_len as u32,
                pass_timestamp: 0,
            };

            let ret = unsafe {
                rknn_runtime_sys::rknn_inputs_set(context.ctx, 1, &mut rknn_in)
            };
            map_rknn_error(ret)?;

            let ret = unsafe {
                rknn_runtime_sys::rknn_run(context.ctx, std::ptr::null_mut())
            };
            map_rknn_error(ret)?;

            let mut rknn_out = rknn_output {
                buf: std::ptr::null_mut(),
                size: 0,
                is_preallocated: 0,
            };

            let ret = unsafe {
                rknn_runtime_sys::rknn_outputs_get(context.ctx, 1, &mut rknn_out)
            };
            map_rknn_error(ret)?;

            let output_slice = unsafe {
                std::slice::from_raw_parts(rknn_out.buf as *const f32, rknn_out.size as usize / 4)
            };
            Ok(output_slice.to_vec())
        })
        .await
        .map_err(|e| AiEngineError::InferenceFailed(e.to_string()))?
    }

    /// 释放资源（异步）
    pub async fn destroy(&self) -> Result<(), AiEngineError> {
        let ctx = self.ctx.clone();

        task::spawn_blocking(move || {
            *ctx.write().unwrap() = None;
            Ok(())
        })
        .await
        .map_err(|e| AiEngineError::InferenceFailed(e.to_string()))?
    }
}

/// 错误码映射（覆盖 rknn_init、rknn_inputs_set、rknn_run、rknn_outputs_get）
fn map_rknn_error(code: c_int) -> Result<(), AiEngineError> {
    match code {
        0 => Ok(()),
        // rknn_init 错误码
        -1 => Err(AiEngineError::ModelLoadFailed("初始化失败".into())),
        -2 => Err(AiEngineError::ModelLoadFailed("模型格式错误".into())),
        -3 => Err(AiEngineError::ModelLoadFailed("模型不符合框架要求".into())),
        -4 => Err(AiEngineError::ModelLoadFailed("SDK 版本不匹配".into())),
        // rknn_inputs_set / rknn_run / rknn_outputs_get 错误码
        -5 => Err(AiEngineError::InferenceFailed("输入数量不匹配".into())),
        -6 => Err(AiEngineError::InferenceFailed("输出数量不匹配".into())),
        -7 => Err(AiEngineError::InferenceFailed("输入格式错误".into())),
        -8 => Err(AiEngineError::InferenceFailed("输出格式错误".into())),
        -9 => Err(AiEngineError::InferenceFailed("推理超时".into())),
        -10 => Err(AiEngineError::InferenceFailed("上下文无效".into())),
        _ => Err(AiEngineError::InferenceFailed(format!("未知错误: {}", code))),
    }
}

// Safety: RknnRuntime 通过 Arc<RwLock> 提供内部可变性
unsafe impl Send for RknnRuntime {}
unsafe impl Sync for RknnRuntime {}
```

---

## 4. 生命周期管理

### 4.1 RAII 模式

```rust
impl Drop for RknnContext {
    fn drop(&mut self) {
        unsafe { rknn_runtime_sys::rknn_destroy(self.ctx) }
    }
}
```

- `RknnContext` 在 `RwLock` 中管理
- `drop()` 时自动调用 `rknn_destroy`
- 防止资源泄漏

### 4.2 线程安全

```rust
unsafe impl Send for RknnRuntime {}
unsafe impl Sync for RknnRuntime {}
```

- 使用 `Arc<RwLock<Option<RknnContext>>>` 实现内部可变性
- 支持多实例并行推理
- 单实例内串行执行

---

## 5. 异步封装策略

### 5.1 spawn_blocking 使用

```rust
pub async fn load(&self) -> Result<(), AiEngineError> {
    let model_path = self.model_path.clone();
    let ctx = self.ctx.clone();

    task::spawn_blocking(move || {
        // FFI 调用
    }).await.map_err(|e| AiEngineError::InferenceFailed(e.to_string()))?
}
```

- FFI 调用不阻塞 Tokio async runtime
- `spawn_blocking` 将阻塞调用移到专用线程池

---

## 6. 错误处理

### 6.1 错误类型定义

```rust
#[derive(Debug, Error)]
pub enum AiEngineError {
    #[error("模型加载失败: {0}")]
    ModelLoadFailed(String),

    #[error("推理执行失败: {0}")]
    InferenceFailed(String),

    #[error("模型未加载")]
    ModelNotLoaded,

    #[error("输入形状不匹配: expected {expected:?}, actual {actual:?}")]
    InputShapeMismatch { expected: Vec<i32>, actual: Vec<i32> },
}
```

### 6.2 错误码映射表

| C API 返回值 | Rust 错误类型 | 说明 |
|-------------|--------------|------|
| 0 | Ok(()) | 成功 |
| -1 | ModelLoadFailed | 初始化失败 |
| -2 | ModelLoadFailed | 模型格式错误 |
| -3 | ModelLoadFailed | 模型不符合框架要求 |
| -4 | ModelLoadFailed | SDK 版本不匹配 |
| 其他 | InferenceFailed | 推理执行失败 |

---

## 7. 测试策略

### 7.1 静态分析

- 使用 `rust-bindgen` 生成 C header 绑定
- 验证类型布局一致性

### 7.2 单元测试

- Mock C API（当 `librknnrt.so` 不可用时）
- 验证错误处理路径
- 验证资源释放

### 7.3 集成测试

- 需要真实 RK3588 硬件或模拟器
- 验证端到端推理流程

---

## 8. 依赖项

```toml
[dependencies]
tokio = { version = "1", features = ["rt-multi-thread"] }
thiserror = "1"

[build-dependencies]
bindgen = "0.68"  # 可选：用于从 C header 生成绑定
```

---

## 9. 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| RK-01 | rknn_init 成功加载 .rknn 模型 | 单元测试：使用真实模型文件 |
| RK-02 | rknn_run 正确执行推理 | 单元测试：比对已知输入输出 |
| RK-03 | 输入形状验证正确 | 单元测试：验证 shape mismatch 错误 |
| RK-04 | 输出形状正确 | 单元测试：验证输出维度 |
| RK-05 | 资源正确释放 (rknn_destroy) | 单元测试：验证资源泄漏检测 |
| RK-06 | 错误处理正确 | 单元测试：验证各错误场景 |
| RK-07 | 异步封装不阻塞 runtime | 集成测试：验证并发执行 |
| RK-08 | Send + Sync 实现正确 | 编译测试 |

---

## 10. 实现计划

| 阶段 | 任务 | 优先级 |
|------|------|--------|
| 1 | 创建 rknn_runtime_sys.rs (FFI 声明) | P0 |
| 2 | 创建 rknn_types.rs (类型定义) | P0 |
| 3 | 修改 rknn_runtime.rs (异步封装) | P0 |
| 4 | 编写单元测试 | P1 |
| 5 | 集成测试（硬件环境） | P2 |

---

## 11. 术语表

| 术语 | 说明 |
|------|------|
| FFI | Foreign Function Interface，跨语言函数调用 |
| RKNN | Rockchip Neural Network，Rockchip NPU 推理框架 |
| librknnrt.so | RKNN Runtime C 库 |
| NPU | Neural Processing Unit，神经网络处理器 |
| RAII | Resource Acquisition Is Initialization，资源获取即初始化 |
| spawn_blocking | Tokio 异步运行时提供的阻塞任务封装 |