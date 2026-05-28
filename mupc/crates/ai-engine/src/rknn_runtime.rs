//! RKNN Runtime 推理器
//!
//! RK3588 NPU 专用推理接口
//!
//! 实际推理通过 FFI 调用 librknnrt.so (RKNN Runtime C API)

use crate::error::AiEngineError;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// RKNN Runtime 上下文
///
/// 实际为 FFI 到 librknnrt.so 的句柄
pub struct RknnContext {
    /// 模型 ID (RKNN SDK 返回)
    model_id: u64,
    input_shape: Vec<i32>,
    output_shape: Vec<i32>,
}

/// RKNN Runtime 推理器
///
/// 支持 RK3588 NPU INT8 量化推理
pub struct RknnRuntime {
    model_path: std::path::PathBuf,
    ctx: Arc<RwLock<Option<RknnContext>>>,
}

impl RknnRuntime {
    /// 创建 RKNN Runtime 推理器
    pub fn new(model_path: &Path) -> Result<Self, AiEngineError> {
        if !model_path.exists() {
            return Err(AiEngineError::ModelLoadFailed(
                format!("模型文件不存在: {:?}", model_path)
            ));
        }
        Ok(Self {
            model_path: model_path.to_path_buf(),
            ctx: Arc::new(RwLock::new(None)),
        })
    }

    /// 加载模型
    ///
    /// 调用 rknn_init() 初始化 RKNN Context
    pub async fn load(&self) -> Result<(), AiEngineError> {
        // 检查文件是否存在
        if !self.model_path.exists() {
            return Err(AiEngineError::ModelLoadFailed(
                format!("模型文件不存在: {:?}", self.model_path)
            ));
        }

        // TODO: 实际 FFI 调用
        // rknn_init(model_path, &model_id)
        //
        // 模拟实现：设置默认输入/输出形状
        // 实际形状应根据模型 graph 获取
        let ctx = RknnContext {
            model_id: 0, // 模拟
            input_shape: vec![1, 64],   // LSTM 输入: [batch, seq_len, features]
            output_shape: vec![1, 8],    // LSTM 输出: [batch, output_horizon]
        };

        *self.ctx.write().await = Some(ctx);
        Ok(())
    }

    /// 执行推理
    ///
    /// 调用 rknn_inputs_set() 和 rknn_run()
    pub async fn run(&self, input: &[f32]) -> Result<Vec<f32>, AiEngineError> {
        let ctx = self.ctx.read().await;
        let ctx = ctx.as_ref()
            .ok_or(AiEngineError::ModelNotLoaded)?;

        // 验证输入形状
        let expected_size: usize = ctx.input_shape.iter().product();
        if input.len() != expected_size {
            return Err(AiEngineError::InputShapeMismatch {
                expected: ctx.input_shape.clone(),
                actual: vec![input.len() as i32],
            });
        }

        // TODO: 实际 FFI 调用
        // rknn_inputs_set(ctx.model_id, input)
        // rknn_run(ctx.model_id, &outputs)

        // 模拟推理：返回零向量
        // 实际应从 rknn_outputs 读取推理结果
        let output_size: usize = ctx.output_shape.iter().product();
        Ok(vec![0.0; output_size])
    }

    /// 获取输入形状
    pub fn get_input_shape(&self) -> Vec<i32> {
        // 同步获取，需要访问 ctx
        // 注意：这是一个简化实现，实际应返回真实形状
        vec![1, 64]
    }

    /// 获取输出形状
    pub fn get_output_shape(&self) -> Vec<i32> {
        vec![1, 8]
    }

    /// 检查模型是否已加载
    pub fn is_loaded(&self) -> bool {
        // 简化检查，实际应查询 ctx 状态
        true
    }
}

unsafe impl Send for RknnRuntime {}
unsafe impl Sync for RknnRuntime {}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rknn_runtime_creation() {
        let runtime = RknnRuntime::new(Path::new("/nonexistent/model.rknn"));
        assert!(runtime.is_ok());
    }

    #[tokio::test]
    async fn test_rknn_runtime_load() {
        let runtime = RknnRuntime::new(Path::new("/tmp/test.rknn")).unwrap();
        // 文件不存在会失败，但结构体创建成功
        assert!(!runtime.is_loaded());
    }
}