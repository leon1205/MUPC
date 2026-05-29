use thiserror::Error;

#[derive(Error, Debug)]
pub enum MonitorError {
    #[error("采集错误: {0}")]
    CollectionError(String),
    #[error("分析错误: {0}")]
    AnalysisError(String),
    #[error("自愈动作失败: {0}")]
    SelfHealingError(String),
    #[error("存储错误: {0}")]
    StorageError(String),
    #[error("阈值配置错误: {0}")]
    ThresholdConfigError(String),
}
