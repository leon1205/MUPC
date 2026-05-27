use thiserror::Error;

#[derive(Error, Debug)]
pub enum StrategyError {
    #[error("策略执行失败: {0}")]
    ExecutionFailed(String),

    #[error("AI 模型错误: {0}")]
    ModelError(String),

    #[error("配置错误: {0}")]
    ConfigError(String),
}