//! plugin-loader 错误类型

use thiserror::Error;

/// 插件加载器错误
#[derive(Debug, Error)]
pub enum LoaderError {
    /// 插件加载失败
    #[error("插件加载失败: {0}")]
    LoadFailed(String),

    /// 插件卸载失败
    #[error("插件卸载失败: {0}")]
    UnloadFailed(String),

    /// 插件不存在
    #[error("插件不存在: {0}")]
    NotFound(String),

    /// 插件初始化失败
    #[error("插件初始化失败: {0}")]
    InitFailed(String),

    /// 插件启动失败
    #[error("插件启动失败: {0}")]
    StartFailed(String),

    /// 插件停止失败
    #[error("插件停止失败: {0}")]
    StopFailed(String),

    /// 插件元信息错误
    #[error("插件元信息错误: {0}")]
    MetaError(String),

    /// 动态库加载错误
    #[error("动态库加载错误: {0}")]
   DlOpenError(String),

    /// 符号查找失败
    #[error("符号查找失败: {0}")]
    SymbolNotFound(String),

    /// 其他错误
    #[error("插件加载器错误: {0}")]
    Other(String),
}

impl LoaderError {
    /// 创建加载失败错误
    pub fn load_failed(msg: impl Into<String>) -> Self {
        Self::LoadFailed(msg.into())
    }

    /// 创建卸载失败错误
    pub fn unload_failed(msg: impl Into<String>) -> Self {
        Self::UnloadFailed(msg.into())
    }

    /// 创建不存在错误
    pub fn not_found(name: impl Into<String>) -> Self {
        Self::NotFound(name.into())
    }

    /// 创建初始化失败错误
    pub fn init_failed(msg: impl Into<String>) -> Self {
        Self::InitFailed(msg.into())
    }

    /// 创建启动失败错误
    pub fn start_failed(msg: impl Into<String>) -> Self {
        Self::StartFailed(msg.into())
    }

    /// 创建停止失败错误
    pub fn stop_failed(msg: impl Into<String>) -> Self {
        Self::StopFailed(msg.into())
    }
}