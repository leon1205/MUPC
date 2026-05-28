# MUPC Phase 3C.2 规格文档 - RKNN Runtime FFI 实现

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-28 | 需求分析师 | ✅ 已评审 |

---

[REVIEWED: PASS]

---

## 1. 概述

### 1.1 项目背景

MUPC Phase 3C 实现 AI 优化引擎，包括 LSTM 预测和 MADDPG/PPO 决策模型。模型通过 RKNN Runtime 在 RK3588 NPU 上执行推理。

当前状态：
- `mupc/crates/ai-engine/src/rknn_runtime.rs` 是 FFI stub 占位实现
- Phase 3C.2 OTA 的 `warmup_model` 依赖于 RknnRuntime
- 需要实现真实的 FFI 调用到 `librknnrt.so`

### 1.2 RKNN Runtime 简介

RKNN Runtime 是 Rockchip 提供的 NPU 推理引擎，用于在 RK3588 NPU 上高效执行量化神经网络推理。

**核心特性**：
- 支持 INT8/FP16 量化推理
- 专用于 RK3588 NPU 硬件加速
- C API 接口 (`librknnrt.so`)

### 1.3 librknnrt.so 的作用

`librknnrt.so` 是 RKNN Runtime 的共享库，提供以下功能：
- 模型加载与初始化
- 输入/输出 tensor 管理
- 推理执行
- 资源释放

### 1.4 FFI 边界

```
Rust (mupc/ai-engine)  ←→  C API (librknnrt.so)
           ↓
    Tower + Tokio (异步封装)
```

**关键约束**：
- FFI 调用必须在后台线程执行（不阻塞 async runtime）
- 必须实现 `Send + Sync`
- 使用 RAII 模式管理内存

---

## 2. 功能需求

### 2.1 模型加载 (rknn_init)

**C API 参考**：
```c
int rknn_init(rknn_context* ctx, const char* model_path, int type, int flag);
```

**参数说明**：
| 参数 | 类型 | 说明 |
|------|------|------|
| ctx | rknn_context* | 输出：上下文句柄 |
| model_path | const char* | 模型文件路径 (.rknn) |
| type | int | 模型类型 (0=RKNN_MODEL_TYPE_UNKNOWN) |
| flag | int | 初始化标志 |

**返回值**：0 表示成功，非零表示失败

**Rust FFI 声明**：
```rust
#[link(name = "rknnrt")]
extern "C" {
    fn rknn_init(ctx: *mut u64, model_path: *const c_char, model_type: c_int, flag: c_int) -> c_int;
}
```

**错误处理**：
- 返回非零 → `AiEngineError::ModelLoadFailed`

---

### 2.2 输入设置 (rknn_inputs_set)

**C API 参考**：
```c
int rknn_inputs_set(rknn_context ctx, uint32_t n, rknn_input* inputs);
```

**参数说明**：
| 参数 | 类型 | 说明 |
|------|------|------|
| ctx | rknn_context | 上下文句柄 |
| n | uint32_t | 输入tensor数量 |
| inputs | rknn_input* | 输入tensor数组 |

**rknn_input 结构**：
```c
typedef struct {
    uint32_t index;       // 输入索引
    void* buf;            // 输入数据缓冲区
    uint32_t size;       // 数据大小
    int pass_timestamp;  // 时间戳标记
} rknn_input;
```

**Rust FFI 声明**：
```rust
#[repr(C)]
pub struct RknnInput {
    pub index: u32,
    pub buf: *mut c_void,
    pub size: u32,
    pub pass_timestamp: c_int,
}

extern "C" {
    fn rknn_inputs_set(ctx: u64, n: u32, inputs: *mut RknnInput) -> c_int;
}
```

---

### 2.3 推理执行 (rknn_run)

**C API 参考**：
```c
int rknn_run(rknn_context ctx, rknn_context* reserved);
```

**参数说明**：
| 参数 | 类型 | 说明 |
|------|------|------|
| ctx | rknn_context | 上下文句柄 |
| reserved | rknn_context* | 保留参数（传 nullptr） |

**返回值**：0 表示成功，非零表示失败

**Rust FFI 声明**：
```rust
extern "C" {
    fn rknn_run(ctx: u64, reserved: *mut u64) -> c_int;
}
```

**错误处理**：
- 返回非零 → `AiEngineError::InferenceFailed`

---

### 2.4 输出获取 (rknn_outputs_get)

**C API 参考**：
```c
int rknn_outputs_get(rknn_context ctx, uint32_t n, rknn_output* outputs);
```

**rknn_output 结构**：
```c
typedef struct {
    void* buf;            // 输出数据缓冲区
    uint32_t size;       // 数据大小
    int is_preallocated; // 是否预分配内存
} rknn_output;
```

**Rust FFI 声明**：
```rust
#[repr(C)]
pub struct RknnOutput {
    pub buf: *mut c_void,
    pub size: u32,
    pub is_preallocated: c_int,
}

extern "C" {
    fn rknn_outputs_get(ctx: u64, n: u32, outputs: *mut RknnOutput) -> c_int;
}
```

---

### 2.5 资源释放 (rknn_destroy)

**C API 参考**：
```c
int rknn_destroy(rknn_context ctx);
```

**Rust FFI 声明**：
```rust
extern "C" {
    fn rknn_destroy(ctx: u64) -> c_int;
}
```

**RAII 设计**：
- `RknnContext` 实现 `Drop` trait 自动调用 `rknn_destroy`
- 确保资源正确释放

---

## 3. 接口设计

### 3.1 核心数据结构

```rust
/// RKNN Runtime 上下文
///
/// 实际为 FFI 到 librknnrt.so 的句柄
pub struct RknnContext {
    ctx: u64,              // rknn_context 实际上是 u64
    input_shape: Vec<i32>,
    output_shape: Vec<i32>,
}

/// RKNN Runtime 推理器
///
/// 支持 RK3588 NPU INT8 量化推理
pub struct RknnRuntime {
    model_path: PathBuf,
    ctx: RwLock<Option<RknnContext>>,
}
```

### 3.2 异步封装设计

**关键原则**：
- FFI 调用使用 `tokio::task::spawn_blocking` 在后台线程执行
- 不阻塞 Tokio async runtime

```rust
impl RknnRuntime {
    /// 创建推理器（同步，因为 rknn_init 可能耗时）
    pub fn new(model_path: &Path) -> Result<Self, AiEngineError>;

    /// 加载模型（异步封装 spawn_blocking）
    pub async fn load(&self) -> Result<(), AiEngineError> {
        let model_path = self.model_path.clone();
        let ctx = self.ctx.clone();

        tokio::task::spawn_blocking(move || {
            // 调用 rknn_init
            // ...
        }).await??;
    }

    /// 执行推理（异步封装 spawn_blocking）
    pub async fn run(&self, input: &[f32]) -> Result<Vec<f32>, AiEngineError> {
        // spawn_blocking 中调用 rknn_inputs_set + rknn_run + rknn_outputs_get
    }

    /// 释放资源（异步封装 spawn_blocking）
    pub async fn destroy(&self) -> Result<(), AiEngineError>;
}
```

### 3.3 线程安全

```rust
unsafe impl Send for RknnRuntime {}
unsafe impl Sync for RknnRuntime {}
```

**设计理由**：
- `RwLock<Option<RknnContext>>` 提供内部可变性
- 多个模型实例可并行推理
- 同一实例内部串行执行（通过 RwLock）

---

## 4. 错误处理

### 4.1 错误类型映射

| C API 返回值 | Rust 错误类型 | 说明 |
|-------------|--------------|------|
| != 0 (rknn_init) | `AiEngineError::ModelLoadFailed` | 模型加载失败 |
| != 0 (rknn_run) | `AiEngineError::InferenceFailed` | 推理执行失败 |
| 上下文为 None | `AiEngineError::ModelNotLoaded` | 模型未加载 |
| 输入形状不匹配 | `AiEngineError::InputShapeMismatch` | 输入形状错误 |

### 4.2 AiEngineError 枚举（扩展）

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

---

## 5. 验收标准

| ID | 标准 | 验证方法 |
|----|------|----------|
| RK-01 | rknn_init 成功加载 .rknn 模型 | 单元测试：使用真实模型文件 |
| RK-02 | rknn_run 正确执行推理 | 单元测试：比对已知输入输出 |
| RK-03 | 输入形状验证正确 | 单元测试：验证 shape mismatch 错误 |
| RK-04 | 输出形状正确 | 单元测试：验证输出维度 |
| RK-05 | 资源正确释放 (rknn_destroy) | 单元测试：验证资源泄漏检测 |
| RK-06 | 错误处理正确 | 单元测试：验证各错误场景 |
| RK-07 | 异步封装不阻塞 runtime | 集成测试：验证并发执行 |
| RK-08 | Send + Sync 实现正确 | 编译测试：`impl Send for RknnRuntime` |

---

## 6. 技术约束

### 6.1 链接方式

**方式一：静态链接（首选）**
```rust
#[link(name = "rknnrt")]
extern "C" { }
```

**方式二：动态加载（libloading）**
```rust
use libloading::{Library, Symbol};

let lib = Library::new("librknnrt.so")?;
let init: Symbol<unsafe extern "C" fn(...) -> c_int> = lib.get(b"rknn_init")?;
```

**约束**：静态链接优先，满足条件时使用

### 6.2 异步封装

```rust
// 错误示例：阻塞 async runtime
pub async fn run(&self, input: &[f32]) -> Result<Vec<f32>, AiEngineError> {
    unsafe { rknn_run(self.ctx, std::ptr::null_mut()) }  // 阻塞调用
}

// 正确示例：spawn_blocking
pub async fn run(&self, input: &[f32]) -> Result<Vec<f32>, AiEngineError> {
    tokio::task::spawn_blocking(move || {
        unsafe { rknn_run(ctx, std::ptr::null_mut()) }
    }).await??;
}
```

### 6.3 内存管理

**RAII 模式**：
```rust
impl Drop for RknnContext {
    fn drop(&mut self) {
        unsafe { rknn_destroy(self.ctx) }
    }
}
```

**约束**：
- 所有 FFI 调用使用 `unsafe`
- 输入/输出缓冲区使用 `Box::into_raw` / `Box::from_raw`
- 确保不出现 use-after-free

### 6.4 依赖项

```toml
[dependencies]
libloading = "0.8"  # 动态加载 librknnrt.so
tokio = { version = "1", features = ["rt-multi-thread"] }
```

---

## 7. 实现计划

### 7.1 第一阶段：FFI 声明

- [ ] 定义 C struct 对应的 Rust repr(C) 结构
- [ ] 声明 `#[link(name = "rknnrt")] extern "C"` 函数
- [ ] 验证链接器可找到 librknnrt.so

### 7.2 第二阶段：同步实现

- [ ] 实现 `RknnRuntime::new()`
- [ ] 实现 `RknnContext` 结构（含 FFI 调用）
- [ ] 实现 `RknnRuntime::load()` 同步版本

### 7.3 第三阶段：异步封装

- [ ] 使用 `spawn_blocking` 封装 FFI 调用
- [ ] 实现 `run()` 异步推理
- [ ] 实现 `destroy()` 异步资源释放

### 7.4 第四阶段：错误处理与测试

- [ ] 完善错误类型映射
- [ ] 实现输入/输出形状验证
- [ ] 编写单元测试覆盖验收标准

---

## 8. 参考资料

- Rockchip RKNN Runtime API 文档
- MUPC Phase 3C 规格文档
- Rust FFI 指南：https://rust-lang.github.io/rust-bindgen/

---

## 9. 术语表

| 术语 | 说明 |
|------|------|
| FFI | Foreign Function Interface，跨语言函数调用 |
| RKNN | Rockchip Neural Network，Rockchip NPU 推理框架 |
| librknnrt.so | RKNN Runtime C 库 |
| NPU | Neural Processing Unit，神经网络处理器 |
| INT8 | 8位整数量化，一种模型压缩格式 |