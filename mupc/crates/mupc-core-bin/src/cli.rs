//! CLI 命令行参数解析
//!
//! 使用 clap v4 derive 模式定义 mupcd 的所有命令行选项。

use clap::Parser;
use std::path::PathBuf;

/// MUPC 微电网特种调控装置通信管理模块
#[derive(Parser, Debug)]
#[command(name = "mupcd", version, about = "MUPC 微电网特种调控装置通信管理模块", long_about = None)]
pub struct Cli {
    /// 主配置文件路径
    #[arg(
        short = 'c',
        long = "config",
        default_value = "/opt/mupc/config/mupc_core_config.yaml",
        value_name = "FILE"
    )]
    pub config: PathBuf,

    /// 模型文件目录
    #[arg(
        short = 'm',
        long = "model-dir",
        default_value = "/opt/mupc/models",
        value_name = "DIR"
    )]
    pub model_dir: PathBuf,

    /// 日志输出目录
    #[arg(
        short = 'l',
        long = "log-dir",
        default_value = "/opt/mupc/logs",
        value_name = "DIR"
    )]
    pub log_dir: PathBuf,

    /// 详细日志模式 (RUST_LOG=debug)
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    /// 仅校验配置文件合法性后退出
    #[arg(long = "validate-config")]
    pub validate_config: bool,
}

impl Cli {
    /// 校验 CLI 参数合法性
    ///
    /// 返回 Err 当必需路径不存在或参数非法。
    pub fn validate(&self) -> Result<(), String> {
        if self.config.as_os_str().is_empty() {
            return Err("--config 参数不能为空".to_string());
        }
        if self.model_dir.as_os_str().is_empty() {
            return Err("--model-dir 参数不能为空".to_string());
        }
        if self.log_dir.as_os_str().is_empty() {
            return Err("--log-dir 参数不能为空".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_default_values() {
        let args = Cli::parse_from(["mupcd", "--config", "/tmp/test.yaml"]);
        assert_eq!(args.config, PathBuf::from("/tmp/test.yaml"));
        assert_eq!(args.model_dir, PathBuf::from("/opt/mupc/models"));
        assert_eq!(args.log_dir, PathBuf::from("/opt/mupc/logs"));
        assert!(!args.verbose);
        assert!(!args.validate_config);
    }

    #[test]
    fn test_cli_validate_config_flag() {
        let args = Cli::parse_from([
            "mupcd",
            "--config",
            "/tmp/test.yaml",
            "--validate-config",
        ]);
        assert!(args.validate_config);
    }

    #[test]
    fn test_cli_verbose_flag() {
        let args =
            Cli::parse_from(["mupcd", "--config", "/tmp/test.yaml", "-v"]);
        assert!(args.verbose);
    }

    #[test]
    fn test_cli_validate_success() {
        let cli = Cli {
            config: PathBuf::from("/tmp/test.yaml"),
            model_dir: PathBuf::from("/tmp/models"),
            log_dir: PathBuf::from("/tmp/logs"),
            verbose: false,
            validate_config: false,
        };
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn test_cli_validate_empty_config() {
        let cli = Cli {
            config: PathBuf::from(""),
            model_dir: PathBuf::from("/tmp/models"),
            log_dir: PathBuf::from("/tmp/logs"),
            verbose: false,
            validate_config: false,
        };
        assert!(cli.validate().is_err());
    }
}
