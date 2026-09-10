//! 渲染进程 CLI 参数（设计 §7.2「渲染进程启动参数」）：`--channel/--interval/--stale-ms/
//! --backend/--fbdev-path/--width/--height/--font`。
//!
//! 设计约束（§7.2 KISS）：
//! - **不读 `mupc_core_config.yaml`**（渲染端零核心配置依赖，不引 serde_yaml）；
//!   默认值全部取自 display-proto 共享常量或本文件常量。
//! - 参数在此**一次性校验**（含数值范围、后端平台可用性），非法即明确报错退出（不静默降级）。
//! - 手写参数解析（不引 clap）：选项少、无常量、可确定性单测，依赖面保持最小。
//!
//! ⚠️ 本文件仅**解析**，不构造后端/字库——后端的平台可用性错误在 [`crate::run`]/bin 层
//! 以明确错误返回（`--backend drm` 未实现，见 [`Backend::Drm`]）。

use std::path::PathBuf;

use crate::channel::ChannelEndpoint;

/// 轮询周期默认值（设计 §3.1/§5.3：500ms；1Hz 展示取最新帧快照，允许丢中间帧）。
pub const DEFAULT_INTERVAL_MS: u64 = 500;
/// 轮询周期下限（防止误填 0 造成忙等/自旋，PRD 4.1.3 要求无忙等）。
pub const MIN_INTERVAL_MS: u64 = 50;
/// 轮询周期上限（10s；再长则「收到→上屏 ≤500ms」语义失去意义）。
pub const MAX_INTERVAL_MS: u64 = 10_000;
/// 过期阈值下限。
pub const MIN_STALE_MS: u64 = 100;
/// 过期阈值上限。
pub const MAX_STALE_MS: u64 = 600_000;
/// 分辨率下限（低于此不构成可用屏面）。
pub const MIN_DIM: u32 = 64;
/// 分辨率上限（防御性，避免按超大尺寸分配离屏缓冲）。
pub const MAX_DIM: u32 = 8192;
/// framebuffer 设备默认路径（设计 §11.2 unit 示例）。
pub const DEFAULT_FBDEV_PATH: &str = "/dev/fb0";

/// 屏幕后端（设计 §7.2 `--backend fbdev|drm|offscreen`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// 离屏内存画布（无真屏可测；Windows/x86 CI 全链路，设计 §10.1）。
    Offscreen,
    /// Linux `/dev/fb0` mmap 直写（真机默认；仅 Linux，设计 §B1）。
    Fbdev,
    /// DRM dumb-buffer 主平面。**本期未实现**（设计 §13 前置项 1：待真机校验 fb 像素格式
    /// 后再决定是否需要 DRM 后端）；解析接受该值，构造后端时给明确错误（不静默回退）。
    Drm,
}

impl Backend {
    /// 命令行取值名。
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Offscreen => "offscreen",
            Backend::Fbdev => "fbdev",
            Backend::Drm => "drm",
        }
    }

    /// 解析取值名（大小写不敏感）。
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "offscreen" => Some(Backend::Offscreen),
            "fbdev" => Some(Backend::Fbdev),
            "drm" => Some(Backend::Drm),
            _ => None,
        }
    }

    /// 是否可在当前平台构建（`fbdev` 需 Linux）。
    pub fn available_on_this_platform(self) -> bool {
        match self {
            Backend::Fbdev | Backend::Drm => cfg!(target_os = "linux"),
            Backend::Offscreen => true,
        }
    }
}

/// 平台默认后端：Linux 真机取 `fbdev`（设计 §11.2 unit），其余（Windows 开发/CI）取
/// `offscreen`（设计 §10.1 无真屏路径）。
pub fn default_backend() -> Backend {
    if cfg!(target_os = "linux") {
        Backend::Fbdev
    } else {
        Backend::Offscreen
    }
}

/// 渲染进程运行参数（CLI 解析结果；设计 §7.2 表「渲染进程启动参数」）。
#[derive(Debug, Clone, PartialEq)]
pub struct CliConfig {
    /// 数据通道 URL（默认 display-proto `DEFAULT_CHANNEL_URL`，与 mupcd `display.bind_addr` 同端点）。
    pub channel: String,
    /// 轮询周期(ms)。
    pub interval_ms: u64,
    /// 数据过期阈值(ms)（默认 display-proto `DEFAULT_STALE_MS=2000`）。
    pub stale_ms: u64,
    /// 屏幕后端。
    pub backend: Backend,
    /// framebuffer 设备路径（仅 `--backend fbdev` 使用）。
    pub fbdev_path: String,
    /// 渲染宽度(px)。
    pub width: u32,
    /// 渲染高度(px)。
    pub height: u32,
    /// 外部/系统字库路径（None = 捆绑子集 feature 或内置 ASCII 回退，设计 §5.4）。
    pub font: Option<PathBuf>,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            channel: mupc_display_proto::DEFAULT_CHANNEL_URL.to_string(),
            interval_ms: DEFAULT_INTERVAL_MS,
            stale_ms: mupc_display_proto::DEFAULT_STALE_MS,
            backend: default_backend(),
            fbdev_path: DEFAULT_FBDEV_PATH.to_string(),
            width: crate::SCREEN_W,
            height: crate::SCREEN_H,
            font: None,
        }
    }
}

impl CliConfig {
    /// 轮询周期（Duration）。
    pub fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.interval_ms)
    }

    /// 是否为离屏后端（bin 据此决定是否打印每帧调试行，设计 §10.1）。
    pub fn is_offscreen(&self) -> bool {
        self.backend == Backend::Offscreen
    }

    /// 解析命令行参数（**不含** argv[0]，由 bin 侧 `skip(1)` 后传入）。
    ///
    /// 支持 `--flag value` 与 `--flag=value` 两种写法；未知参数/非法取值一律返回
    /// [`ConfigError`]（调用方打印明确原因并非零退出，杜绝静默取默认值）。
    pub fn parse(args: &[String]) -> Result<Self, ConfigError> {
        let mut cfg = Self::default();
        let mut it = args.iter();
        while let Some(raw) = it.next() {
            // 允许 --k=v
            let (flag, inline) = match raw.split_once('=') {
                Some((f, v)) => (f, Some(v.to_string())),
                None => (raw.as_str(), None),
            };
            // 需要一个取值的参数：优先 inline，否则取下一个 argv
            let mut take = |flag: &str| -> Result<String, ConfigError> {
                match &inline {
                    Some(v) => Ok(v.clone()),
                    None => it.next().cloned().ok_or_else(|| ConfigError::MissingValue(flag.into())),
                }
            };
            match flag {
                "-h" | "--help" => return Err(ConfigError::Help),
                "-V" | "--version" => return Err(ConfigError::Version),
                "--channel" => {
                    let v = take(flag)?;
                    // 复用通道 URL 解析做前置校验（仅 http:// 回环，无 TLS）
                    ChannelEndpoint::parse(&v)
                        .map_err(|e| ConfigError::Invalid(flag.into(), v.clone(), e.to_string()))?;
                    cfg.channel = v;
                }
                "--interval" => {
                    let v = take(flag)?;
                    cfg.interval_ms = parse_u64_range(flag, &v, MIN_INTERVAL_MS, MAX_INTERVAL_MS)?;
                }
                "--stale-ms" => {
                    let v = take(flag)?;
                    cfg.stale_ms = parse_u64_range(flag, &v, MIN_STALE_MS, MAX_STALE_MS)?;
                }
                "--backend" => {
                    let v = take(flag)?;
                    let b = Backend::parse(&v)
                        .ok_or_else(|| ConfigError::Invalid(flag.into(), v.clone(), "取值须为 offscreen|fbdev|drm".into()))?;
                    if !b.available_on_this_platform() {
                        return Err(ConfigError::Invalid(
                            flag.into(),
                            v,
                            format!("后端 `{}` 仅 Linux 支持（本机请用 --backend offscreen）", b.as_str()),
                        ));
                    }
                    cfg.backend = b;
                }
                "--fbdev-path" => {
                    let v = take(flag)?;
                    if v.is_empty() {
                        return Err(ConfigError::Invalid(flag.into(), v, "路径不能为空".into()));
                    }
                    cfg.fbdev_path = v;
                }
                "--width" => {
                    let v = take(flag)?;
                    cfg.width = parse_u32_range(flag, &v, MIN_DIM, MAX_DIM)?;
                }
                "--height" => {
                    let v = take(flag)?;
                    cfg.height = parse_u32_range(flag, &v, MIN_DIM, MAX_DIM)?;
                }
                "--font" => {
                    let v = take(flag)?;
                    cfg.font = if v.is_empty() { None } else { Some(PathBuf::from(v)) };
                }
                other => return Err(ConfigError::UnknownArg(other.to_string())),
            }
        }
        Ok(cfg)
    }
}

fn parse_u64_range(flag: &str, v: &str, lo: u64, hi: u64) -> Result<u64, ConfigError> {
    let n: u64 = v
        .parse()
        .map_err(|_| ConfigError::Invalid(flag.into(), v.into(), "须为整数".into()))?;
    if n < lo || n > hi {
        return Err(ConfigError::Invalid(
            flag.into(),
            v.into(),
            format!("须在 {lo}..={hi} 之间"),
        ));
    }
    Ok(n)
}

fn parse_u32_range(flag: &str, v: &str, lo: u32, hi: u32) -> Result<u32, ConfigError> {
    let n: u32 = v
        .parse()
        .map_err(|_| ConfigError::Invalid(flag.into(), v.into(), "须为整数".into()))?;
    if n < lo || n > hi {
        return Err(ConfigError::Invalid(
            flag.into(),
            v.into(),
            format!("须在 {lo}..={hi} 之间"),
        ));
    }
    Ok(n)
}

/// CLI 解析错误（含 `--help` / `--version` 两个「非错误」的早退信号）。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    /// `--help` / `-h`：调用方打印用法后以 0 退出。
    #[error("显示用法帮助")]
    Help,
    /// `--version` / `-V`：调用方打印版本后以 0 退出。
    #[error("显示版本")]
    Version,
    /// 未知参数。
    #[error("未识别的参数 `{0}`")]
    UnknownArg(String),
    /// 参数缺少取值。
    #[error("参数 `{0}` 缺少取值")]
    MissingValue(String),
    /// 参数取值非法。
    #[error("参数 `{0}` 的取值 `{1}` 非法：{2}")]
    Invalid(String, String, String),
}

/// 用法帮助文本（bin `--help` 打印）。
pub fn help_text() -> String {
    format!(
        "\
mupc-local-display —— MUPC 本地显示终端渲染进程（12-本地显示终端）

用法：
  mupc-local-display [OPTIONS]

选项：
  --channel <URL>       数据通道（回环 HTTP GET 最新帧）
                        默认 {default_url}
  --interval <MS>       轮询周期，{min_i}..={max_i}，默认 {interval}
  --stale-ms <MS>       数据过期阈值，{min_s}..={max_s}，默认 {stale}
  --backend <NAME>      offscreen|fbdev|drm，默认 {backend}
                        （fbdev/drm 仅 Linux；Windows 开发用 offscreen）
  --fbdev-path <PATH>   framebuffer 设备，默认 {fbdev}
  --width <PX>          渲染宽度，{min_d}..={max_d}，默认 {w}
  --height <PX>         渲染高度，{min_d}..={max_d}，默认 {h}
  --font <PATH>         外部/系统字库(.otf/.ttf)；空=捆绑子集或内置 ASCII 回退
  -h, --help            显示本帮助
  -V, --version         显示版本

说明：
  * 本进程只读数据通道，不下发任何指令；不读 mupc_core_config.yaml（设计 §7.2）。
  * 生产 unit 的 --channel 须与 mupcd 配置 display.bind_addr 对齐。
  * 字库子集化命令见 crates/local-display/src/font.rs 顶部注释。
",
        default_url = mupc_display_proto::DEFAULT_CHANNEL_URL,
        min_i = MIN_INTERVAL_MS,
        max_i = MAX_INTERVAL_MS,
        interval = DEFAULT_INTERVAL_MS,
        min_s = MIN_STALE_MS,
        max_s = MAX_STALE_MS,
        stale = mupc_display_proto::DEFAULT_STALE_MS,
        backend = default_backend().as_str(),
        fbdev = DEFAULT_FBDEV_PATH,
        min_d = MIN_DIM,
        max_d = MAX_DIM,
        w = crate::SCREEN_W,
        h = crate::SCREEN_H,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_match_design() {
        let c = CliConfig::default();
        assert_eq!(c.channel, "http://127.0.0.1:9810/v1/display/latest");
        assert_eq!(c.interval_ms, 500);
        assert_eq!(c.stale_ms, 2000);
        assert_eq!(c.width, 1024);
        assert_eq!(c.height, 768);
        assert_eq!(c.fbdev_path, "/dev/fb0");
        assert!(c.font.is_none());
        // 平台默认后端：Linux=fbdev（真机），其余=offscreen（无真屏路径）
        #[cfg(target_os = "linux")]
        assert_eq!(c.backend, Backend::Fbdev);
        #[cfg(not(target_os = "linux"))]
        assert_eq!(c.backend, Backend::Offscreen);
    }

    #[test]
    fn parse_all_flags_space_and_equals_form() {
        let c = CliConfig::parse(&args(&[
            "--channel",
            "http://127.0.0.1:9999/v1/display/latest",
            "--interval=200",
            "--stale-ms",
            "1500",
            "--backend",
            "offscreen",
            "--fbdev-path=/dev/fb1",
            "--width",
            "800",
            "--height=600",
            "--font",
            "fonts/x.otf",
        ]))
        .unwrap();
        assert_eq!(c.channel, "http://127.0.0.1:9999/v1/display/latest");
        assert_eq!(c.interval_ms, 200);
        assert_eq!(c.stale_ms, 1500);
        assert_eq!(c.backend, Backend::Offscreen);
        assert_eq!(c.fbdev_path, "/dev/fb1");
        assert_eq!(c.width, 800);
        assert_eq!(c.height, 600);
        assert_eq!(c.font, Some(PathBuf::from("fonts/x.otf")));
    }

    #[test]
    fn empty_font_means_bundled_or_fallback() {
        let c = CliConfig::parse(&args(&["--font", ""])).unwrap();
        assert!(c.font.is_none());
    }

    #[test]
    fn unknown_arg_rejected() {
        let e = CliConfig::parse(&args(&["--nope"])).unwrap_err();
        assert_eq!(e, ConfigError::UnknownArg("--nope".into()));
    }

    #[test]
    fn missing_value_rejected() {
        let e = CliConfig::parse(&args(&["--channel"])).unwrap_err();
        assert_eq!(e, ConfigError::MissingValue("--channel".into()));
    }

    #[test]
    fn out_of_range_values_rejected() {
        for (flag, val) in [
            ("--interval", "10"),       // < MIN
            ("--interval", "60000"),    // > MAX
            ("--interval", "abc"),
            ("--stale-ms", "0"),
            ("--stale-ms", "99999999"),
            ("--width", "8"),
            ("--height", "999999"),
        ] {
            let e = CliConfig::parse(&args(&[flag, val])).unwrap_err();
            assert!(
                matches!(e, ConfigError::Invalid(_, _, _)),
                "{flag} {val} 应判非法，实得 {e:?}"
            );
        }
    }

    #[test]
    fn bad_channel_url_rejected_early() {
        let e = CliConfig::parse(&args(&["--channel", "https://127.0.0.1:1/x"])).unwrap_err();
        assert!(matches!(e, ConfigError::Invalid(f, _, _) if f == "--channel"));
        let e = CliConfig::parse(&args(&["--channel", "127.0.0.1:9810"])).unwrap_err();
        assert!(matches!(e, ConfigError::Invalid(f, _, _) if f == "--channel"));
    }

    #[test]
    fn bad_backend_name_rejected() {
        let e = CliConfig::parse(&args(&["--backend", "sdl"])).unwrap_err();
        assert!(matches!(e, ConfigError::Invalid(f, _, _) if f == "--backend"));
    }

    /// 非 Linux 上 `--backend fbdev` 必须**明确报错**（不静默回退 offscreen）。
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn fbdev_rejected_on_non_linux() {
        let e = CliConfig::parse(&args(&["--backend", "fbdev"])).unwrap_err();
        match e {
            ConfigError::Invalid(f, v, why) => {
                assert_eq!(f, "--backend");
                assert_eq!(v, "fbdev");
                assert!(why.contains("仅 Linux"), "错误应可读且明确：{why}");
            }
            other => panic!("应判非法，实得 {other:?}"),
        }
    }

    #[test]
    fn help_and_version_are_early_signals() {
        assert_eq!(CliConfig::parse(&args(&["--help"])).unwrap_err(), ConfigError::Help);
        assert_eq!(CliConfig::parse(&args(&["-h"])).unwrap_err(), ConfigError::Help);
        assert_eq!(
            CliConfig::parse(&args(&["--version"])).unwrap_err(),
            ConfigError::Version
        );
        // 帮助文本含全部设计参数名（防漏项）
        let h = help_text();
        for f in [
            "--channel",
            "--interval",
            "--stale-ms",
            "--backend",
            "--fbdev-path",
            "--width",
            "--height",
            "--font",
        ] {
            assert!(h.contains(f), "帮助文本缺 {f}");
        }
    }

    #[test]
    fn backend_parse_is_case_insensitive() {
        assert_eq!(Backend::parse("OFFSCREEN"), Some(Backend::Offscreen));
        assert_eq!(Backend::parse("FbDev"), Some(Backend::Fbdev));
        assert_eq!(Backend::parse("drm"), Some(Backend::Drm));
        assert_eq!(Backend::parse("x"), None);
    }
}
