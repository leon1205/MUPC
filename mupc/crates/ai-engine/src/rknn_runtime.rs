//! RKNN Runtime 推理器
//!
//! RK3588 NPU 专用推理接口
//!
//! 实际推理通过 FFI 调用 librknnrt.so (RKNN Runtime C API)

use std::ffi::CString;
use std::os::raw::c_int;
use std::path::Path;
use std::sync::{Arc, RwLock};
use tokio::task;

use crate::error::AiEngineError;
use crate::rknn_runtime_sys::{rknn_input, rknn_output};

/// RKNN 上下文（RAII 资源管理）
#[allow(dead_code)]
struct RknnContext {
    ctx: u64,
    input_count: u32,
    output_count: u32,
}

impl Drop for RknnContext {
    fn drop(&mut self) {
        unsafe { crate::rknn_runtime_sys::rknn_destroy(self.ctx); }
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
            let c_path = CString::new(model_path.to_string_lossy().as_bytes())
                .map_err(|e| AiEngineError::ModelLoadFailed(format!("路径包含空字节: {}", e)))?;
            let ret = unsafe {
                crate::rknn_runtime_sys::rknn_init(
                    &mut ctx_handle,
                    c_path.as_ptr(),
                    0, // RKNN_MODEL_TYPE_UNKNOWN
                    0, // flag
                )
            };

            map_rknn_error(ret)?;

            // 使用 rknn_query 查询输入输出数量
            #[repr(C)]
            struct RknnQueryInOut {
                n_input: u32,
                n_output: u32,
            }
            let mut query_info = RknnQueryInOut {
                n_input: 0,
                n_output: 0,
            };
            let ret = unsafe {
                crate::rknn_runtime_sys::rknn_query(
                    ctx_handle,
                    0, // RKNN_CMD_QUERY_INPUT_OUTPUT_NUM
                    &mut query_info as *mut _ as *mut std::os::raw::c_void,
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

        let ctx = context.ctx;
        let input_count = context.input_count;

        task::spawn_blocking(move || {
            let mut rknn_in = rknn_input {
                index: 0,
                buf: input_buf.as_mut_ptr() as *mut std::os::raw::c_void,
                size: input_len as u32,
                pass_timestamp: 0,
            };

            let ret =
                unsafe { crate::rknn_runtime_sys::rknn_inputs_set(ctx, input_count, &mut rknn_in) };
            map_rknn_error(ret)?;

            let ret = unsafe { crate::rknn_runtime_sys::rknn_run(ctx, std::ptr::null_mut()) };
            map_rknn_error(ret)?;

            let mut rknn_out = rknn_output {
                buf: std::ptr::null_mut(),
                size: 0,
                is_preallocated: 0,
            };

            let ret = unsafe { crate::rknn_runtime_sys::rknn_outputs_get(ctx, 1, &mut rknn_out) };
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

    /// 检查模型是否已加载
    pub fn is_loaded(&self) -> bool {
        self.ctx.read().unwrap().is_some()
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
        _ => Err(AiEngineError::InferenceFailed(format!(
            "未知错误: {}",
            code
        ))),
    }
}

// Safety: RknnRuntime 通过 Arc<RwLock> 提供内部可变性
unsafe impl Send for RknnRuntime {}
unsafe impl Sync for RknnRuntime {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rknn_runtime_creation() {
        let runtime = RknnRuntime::new(Path::new("/nonexistent/model.rknn")).unwrap();
        assert!(!runtime.is_loaded());
    }

    #[tokio::test]
    async fn test_rknn_load_invalid_path() {
        let runtime = RknnRuntime::new(Path::new("/nonexistent/model.rknn")).unwrap();
        let result = runtime.load().await;
        // 加载不存在的文件会失败
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rknn_destroy_without_load() {
        let runtime = RknnRuntime::new(Path::new("/tmp/test.rknn")).unwrap();
        // 未加载时销毁应该成功（只是清理 None）
        let result = runtime.destroy().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_rknn_run_without_load() {
        let runtime = RknnRuntime::new(Path::new("/tmp/test.rknn")).unwrap();
        let result = runtime.run(&[1.0, 2.0, 3.0]).await;
        assert!(result.is_err());
    }
}
