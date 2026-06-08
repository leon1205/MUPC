//! 日志模块
//!
//! 基于 tracing 实现，支持文件滚动

use std::path::Path;
use tracing::Level;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

/// 日志配置
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// 日志级别
    pub level: Level,
    /// 日志目录
    pub directory: String,
    /// 单文件大小上限（字节）
    pub file_size_limit: usize,
    /// 保留文件数
    pub max_files: usize,
    /// 是否输出到标准输出
    pub console: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        let directory = if cfg!(windows) {
            "./logs/mupc".to_string()
        } else {
            "/var/log/mupc".to_string()
        };
        Self {
            level: Level::INFO,
            directory,
            file_size_limit: 10 * 1024 * 1024, // 10MB
            max_files: 10,
            console: true,
        }
    }
}

/// 初始化日志系统
pub fn init_logging(config: LogConfig) -> Result<(), Box<dyn std::error::Error>> {
    // 创建日志目录
    let log_dir = Path::new(&config.directory);
    if !log_dir.exists() {
        std::fs::create_dir_all(log_dir)?;
    }

    // 创建文件滚动器
    let file_appender = RollingFileAppender::new(Rotation::DAILY, &config.directory, "mupc.log");

    // 创建环境过滤器
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("{}", config.level)));

    // 构建订阅者
    let subscriber = tracing_subscriber::registry();

    // 文件输出层
    let file_layer = fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_span_events(FmtSpan::CLOSE);

    // 收集所有层
    let subscriber = subscriber.with(env_filter).with(file_layer);
    if config.console {
        let console_layer = fmt::layer()
            .with_target(true)
            .with_thread_ids(false)
            .with_span_events(FmtSpan::CLOSE);
        subscriber.with(console_layer).init();
    } else {
        subscriber.init();
    }

    tracing::info!("日志系统初始化完成，日志目录: {}", config.directory);

    Ok(())
}

/// 初始化日志系统（使用默认配置）
pub fn init_default_logging() -> Result<(), Box<dyn std::error::Error>> {
    init_logging(LogConfig::default())
}
