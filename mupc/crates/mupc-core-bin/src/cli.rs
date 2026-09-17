//! CLI 命令行参数解析
//!
//! 使用 clap v4 derive 模式定义 mupcd 的所有命令行选项。

use clap::Parser;
use std::path::{Path, PathBuf};

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

    /// 日志输出目录（**可选覆盖** `config.system.log_dir`；见 [`Cli::effective_log_dir`]）
    ///
    /// ⚠️ **不再有默认值**（2026-09-17 整改）。整改前它默认 `/opt/mupc/logs`，而读者
    /// （`LogService`）取的是 `config.system.log_dir` —— **两个值**：写者按命令行、读者按配置，
    /// 二者可以不同且**无人知晓**。最坏后果不是 503，而是**静默失实**：`create_dir_all` 会把
    /// 读者那侧目录建出来 ⇒ 目录存在但为空 ⇒ `200 + entries=[]` ⇒ 屏上「当前筛选条件下无日志」，
    /// 而日志其实写在别处。现在省略即"用配置的值"，给了就覆盖（并**写回配置**，全进程一个值）。
    #[arg(short = 'l', long = "log-dir", value_name = "DIR")]
    pub log_dir: Option<PathBuf>,

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
        if let Some(d) = &self.log_dir {
            if d.as_os_str().is_empty() {
                return Err("--log-dir 给了空值（要覆盖就给真实目录，不改就省略）".to_string());
            }
        }
        Ok(())
    }

    /// **生效的**日志目录 —— 写者（`tracing_appender::rolling::daily`）与读者
    /// （`log_service::LogService`）**共用**的唯一真源。
    ///
    /// 规则：给了 `--log-dir` 就用它（现场 `deploy/scripts/start.sh` 与 systemd 单元本就显式传），
    /// 否则用配置的 `system.log_dir`。
    ///
    /// ⚠️ **调用方必须把结果写回 `config.system.log_dir`**（`main.rs` 在 Phase 1 之后就地覆盖）
    /// —— 只算不用等于把"两个值"从"命令行 vs 配置"换成"命令行 vs 配置的拷贝"，双双都是隐患。
    /// 写回后：appender 写它、`LogService` 读它、审计目录（`{log_dir}/audit`）在它下面，**同一个值**。
    pub fn effective_log_dir(&self, config_log_dir: &Path) -> PathBuf {
        self.log_dir
            .clone()
            .unwrap_or_else(|| config_log_dir.to_path_buf())
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
        // R2 整改：`--log-dir` **没有**默认值（省略 = 用 `config.system.log_dir`）
        assert_eq!(args.log_dir, None, "省略 --log-dir ⇒ 不得造出第二个值");
        assert_eq!(
            args.effective_log_dir(Path::new("/var/log/mupc")),
            PathBuf::from("/var/log/mupc"),
            "省略 ⇒ 生效值 = 配置值"
        );
        assert!(!args.verbose);
        assert!(!args.validate_config);
    }

    /// **R2**：`--log-dir` 是**可选覆盖**；生效值有且只有一个（给了用给的，没给用配置的）。
    #[test]
    fn log_dir_is_an_optional_override_with_exactly_one_effective_value() {
        let cfg = Path::new("/var/log/mupc");

        let given = Cli::parse_from(["mupcd", "--log-dir", "/opt/mupc/logs"]);
        assert_eq!(given.log_dir, Some(PathBuf::from("/opt/mupc/logs")));
        assert_eq!(
            given.effective_log_dir(cfg),
            PathBuf::from("/opt/mupc/logs"),
            "--log-dir 给了 ⇒ 以它为准（现场脚本/systemd 单元显式传此参数）"
        );

        let omitted = Cli::parse_from(["mupcd", "-c", "/tmp/x.yaml"]);
        assert_eq!(omitted.effective_log_dir(cfg), PathBuf::from("/var/log/mupc"));

        // 空值仍是**显式非法**（不得被当成"没给"而静默回落到配置值）
        let empty = Cli {
            config: PathBuf::from("/tmp/x.yaml"),
            model_dir: PathBuf::from("/tmp/models"),
            log_dir: Some(PathBuf::new()),
            verbose: false,
            validate_config: false,
        };
        assert!(empty.validate().is_err(), "空 --log-dir 必须被拒");
    }

    /// **单一真源网**（源码级，防回退）：`main.rs` 必须把 `--log-dir` 的生效值**写回**
    /// `config.system.log_dir`，且 tracing appender 与建目录用的必须是**那一个字段**
    /// （不得再直接吃 `cli.log_dir`）；读者侧（`startup.rs` 把 `config.system.log_dir` 交给
    /// `LogService`）因此与写者同值。
    ///
    /// 这条网抓的是"**两个值可以不一致且无人知晓**"这个结构 —— 它曾真实存在：
    /// appender 用 `cli.log_dir`（默认 `/opt/mupc/logs`）、`LogService` 用
    /// `config.system.log_dir`。改法可以变（比如换成一个参数一路传下去），但**两个值同时存在**
    /// 必须让本条变红（探针：把 appender 那行改回 `cli.log_dir` ⇒ 红，实测）。
    #[test]
    fn main_wires_one_effective_log_dir_into_writer_and_reader() {
        let main_src = include_str!("main.rs");
        assert!(
            main_src.contains("config.system.log_dir = cli.effective_log_dir"),
            "main.rs 必须把 --log-dir 的生效值**写回** config.system.log_dir（否则写者/读者各持一个值）"
        );
        assert!(
            main_src.contains("rolling::daily(&config.system.log_dir"),
            "tracing appender 必须用**写回后**的那一个字段"
        );
        // S-1：这里原本有一条 `assert!(!main_src.contains("rolling::daily(&cli.log_dir")))`。
        // 它是**恒真断言**（复核实测：真那么写会因 `E0277` **编译不过**——`cli.log_dir` 是
        // `Option<PathBuf>`，不满足 `rolling::daily` 要求的 `AsRef<Path>`）⇒ 它永远不会失败，
        // 留着会给人"它抓到了什么"的错觉。该回归已由**类型系统**兜住，故删断言、留此说明。
        // （下面两条才是真网：写回语句存在 + appender 吃写回后的字段 + 建目录同源。）
        assert!(
            main_src.contains("create_dir_all(&config.system.log_dir)"),
            "建日志目录也必须用**同一个**值（否则 mkdir 造出一个空目录冒充'无日志'）"
        );

        let startup_src = include_str!("startup.rs");
        assert!(
            startup_src.contains("config.system.log_dir.clone()"),
            "LogService 必须取 config.system.log_dir（= 写回后的生效值），不得另取一个源"
        );
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
            log_dir: Some(PathBuf::from("/tmp/logs")),
            verbose: false,
            validate_config: false,
        };
        assert!(cli.validate().is_ok());
        // 省略 `--log-dir` 也必须合法（= 用配置值）
        assert!(Cli { log_dir: None, ..cli }.validate().is_ok());
    }

    #[test]
    fn test_cli_validate_empty_config() {
        let cli = Cli {
            config: PathBuf::from(""),
            model_dir: PathBuf::from("/tmp/models"),
            log_dir: Some(PathBuf::from("/tmp/logs")),
            verbose: false,
            validate_config: false,
        };
        assert!(cli.validate().is_err());
    }
}
