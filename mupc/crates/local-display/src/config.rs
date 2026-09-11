//! 渲染进程 CLI 参数（设计 §5.1/§5.5/§7.2 表「渲染进程启动参数」）：
//!
//! v1.0 项：`--channel/--interval/--stale-ms/--backend/--fbdev-path/--width/--height/--font`；
//! v2.0 新增（v2.0-r4）：`--poll-ms` / `--control-channel` / `--touch-device` / `--touch-calib` /
//! `--touch-swap-xy` / `--touch-invert-x` / `--touch-invert-y` / `--rotate` / `--idle-timeout-secs`。
//!
//! 设计约束（§7.2 KISS）：
//! - **不读 `mupc_core_config.yaml`**（渲染端零核心配置依赖，不引 serde_yaml）；
//!   默认值全部取自 display-proto 共享常量或本文件常量。
//! - 参数在此**一次性校验**（含数值范围、后端平台可用性、校准量程合法性），非法即明确报错退出
//!   （不静默降级、不静默取默认值 —— 设计 §1.2 校准行）。
//! - **未实现的能力 = 响亮失败**：`--rotate` 非 0 取值直接报错退出（见 [`Rotate`]），
//!   不允许「解析了但无人消费」的静默 no-op。
//! - 手写参数解析（不引 clap）：选项少、无常量、可确定性单测，依赖面保持最小。
//!
//! ⚠️ 本文件仅**解析**，不构造后端/字库/触摸设备——后端的平台可用性错误在 [`crate::run`]/bin 层
//! 以明确错误返回（`--backend drm` 未实现，见 [`Backend::Drm`]）。

use std::path::PathBuf;

use crate::channel::ChannelEndpoint;
use crate::touch::{CalibBounds, TouchOverrides};

/// 轮询周期默认值（设计 §3.1/§5.3：500ms；1Hz 展示取最新帧快照，允许丢中间帧）。
pub const DEFAULT_INTERVAL_MS: u64 = 500;
/// 轮询周期下限（设计 §5.5：`--poll-ms ∈ [100, 500]`；<100ms 无收益且徒增 CPU，
/// 且该值直接进入 F7.3/F16.5 的端到端时延算式）。
pub const MIN_INTERVAL_MS: u64 = 100;
/// 轮询周期上限（**设计 §5.5 红线**：`> 500` 直接报错退出 —— 放开即可能**静默破坏
/// 「告警/联锁变化上屏 ≤2 s」验收**；见设计 §4.2.1 时延拆解）。
pub const MAX_INTERVAL_MS: u64 = 500;
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
/// 空闲回归默认秒数（设计 §5.6 TT-12：无触摸 60 s 回归主状态页）。
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 60;
/// 空闲回归上限（1 h；再长无实际意义，防误填天文数字）。
pub const MAX_IDLE_TIMEOUT_SECS: u64 = 3600;

/// 屏幕旋转（设计 §5.1「`+ --rotate`」；UI 设计 §3.5 栅格）。
///
/// ⚠️ **本轮未实现 —— 响亮失败，不做静默 no-op**（工作单元 C 评审 C-③ 整改）。
/// 真正施加旋转需要薄层 `lv_display_set_rotation(disp, deg)`（或在 flush 时按角度变换像素），
/// 而当前 `src/lvgl/display.rs` **未暴露该通道**、`lvgl-sys/allowlist.txt` 亦无该符号
/// （属薄层能力缺口，见待办）。因此 [`CliConfig::parse`] 对**非 0** 取值**启动即报错退出**
/// （错误文本指明缺口与处置），只有 `--rotate 0`（默认）可用；薄层补齐后由工作单元 B 接线。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotate {
    /// 不旋转（默认）。
    #[default]
    Deg0,
    /// 顺时针 90°（横屏面板竖装）。
    Deg90,
    /// 180°。
    Deg180,
    /// 顺时针 270°。
    Deg270,
}

impl Rotate {
    /// 命令行取值名。
    pub fn as_str(self) -> &'static str {
        match self {
            Rotate::Deg0 => "0",
            Rotate::Deg90 => "90",
            Rotate::Deg180 => "180",
            Rotate::Deg270 => "270",
        }
    }

    /// 角度值。
    pub fn degrees(self) -> u16 {
        match self {
            Rotate::Deg0 => 0,
            Rotate::Deg90 => 90,
            Rotate::Deg180 => 180,
            Rotate::Deg270 => 270,
        }
    }

    /// 解析 `0|90|180|270`（大小写不敏感；也接受 `deg90` 形式）。
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().trim_start_matches("deg") {
            "0" => Some(Rotate::Deg0),
            "90" => Some(Rotate::Deg90),
            "180" => Some(Rotate::Deg180),
            "270" => Some(Rotate::Deg270),
            _ => None,
        }
    }

    /// 是否交换了宽高（90°/270° 时屏幕逻辑尺寸互换 —— `screen.rs` 的 sink 与 `--width/--height`
    /// 的语义在此处对齐）。
    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotate::Deg90 | Rotate::Deg270)
    }
}

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

    /// 解析取值名（大小写不敏感）。`fb0` 是 `fbdev` 的别名（设计 §1.1.1.1 以 P-1 的
    /// `/dev/fb0` 指代该后端；两个名字都接受，避免既有部署脚本回归）。
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "offscreen" => Some(Backend::Offscreen),
            "fbdev" | "fb0" => Some(Backend::Fbdev),
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

/// 渲染进程运行参数（CLI 解析结果；设计 §5.1/§7.2 表「渲染进程启动参数」）。
#[derive(Debug, Clone, PartialEq)]
pub struct CliConfig {
    /// 数据通道 URL（默认 display-proto `DEFAULT_CHANNEL_URL`，与 mupcd `display.bind_addr` 同端点）。
    pub channel: String,
    /// 控制通道基址（默认 display-proto `DEFAULT_CONTROL_BASE_URL`，与 mupcd
    /// `display.control_bind_addr` 同端点；**仅回环**，设计 §1.4 PL-8）。
    pub control_channel: String,
    /// 轮询周期(ms)（`--interval` / `--poll-ms` 同义；设计 §5.5 硬区间 [100, 500]）。
    pub interval_ms: u64,
    /// 数据过期阈值(ms)（默认 display-proto `DEFAULT_STALE_MS=2000`）。
    pub stale_ms: u64,
    /// 屏幕后端。
    pub backend: Backend,
    /// framebuffer 设备路径（仅 `--backend fbdev` 使用）。
    pub fbdev_path: String,
    /// 渲染宽度(px)（逻辑尺寸；`--rotate 90|270` 时由上层决定面板物理宽高）。
    pub width: u32,
    /// 渲染高度(px)。
    pub height: u32,
    /// 外部/系统字库路径（None = 捆绑子集 feature 或内置 ASCII 回退，设计 §5.4）。
    pub font: Option<PathBuf>,
    /// 触摸设备节点（None = 自动发现；生产 unit 固定 `/dev/mupc-touch`，设计 §12.2）。
    pub touch_device: Option<PathBuf>,
    /// 触摸校准与轴变换覆盖（`--touch-calib/--touch-swap-xy/--touch-invert-{x,y}`，设计 §1.2）。
    pub touch: TouchOverrides,
    /// 屏幕旋转（**恒为** [`Rotate::Deg0`]：非 0 取值在解析期即报错退出，见 [`Rotate`] 文档）。
    /// 薄层补齐 `lv_display_set_rotation` 后，由工作单元 B 放开校验并接线。
    pub rotate: Rotate,
    /// 空闲回归秒数（TT-12；默认 60，`0` = 禁用）。
    pub idle_timeout_secs: u64,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            channel: mupc_display_proto::DEFAULT_CHANNEL_URL.to_string(),
            control_channel: mupc_display_proto::DEFAULT_CONTROL_BASE_URL.to_string(),
            interval_ms: DEFAULT_INTERVAL_MS,
            stale_ms: mupc_display_proto::DEFAULT_STALE_MS,
            backend: default_backend(),
            fbdev_path: DEFAULT_FBDEV_PATH.to_string(),
            width: crate::SCREEN_W,
            height: crate::SCREEN_H,
            font: None,
            touch_device: None,
            touch: TouchOverrides::default(),
            rotate: Rotate::Deg0,
            idle_timeout_secs: DEFAULT_IDLE_TIMEOUT_SECS,
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

    /// 触摸配置（平台无关；交给 `touch::TouchDevice::open`）。
    pub fn touch_config(&self) -> crate::touch::TouchConfig {
        crate::touch::TouchConfig {
            device: self.touch_device.clone(),
            overrides: self.touch,
            width: self.width,
            height: self.height,
        }
    }

    /// 空闲回归计时器（TT-12；`idle_timeout_secs == 0` ⇒ 禁用）。
    pub fn idle_timer(&self, now_ms: u64) -> crate::timing::IdleTimer {
        crate::timing::IdleTimer::new(self.idle_timeout_secs, now_ms)
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
                // `--poll-ms` 是设计 §5.5 的规范名；`--interval` 为 v1.0 兼容别名（同义同区间）。
                "--interval" | "--poll-ms" => {
                    let v = take(flag)?;
                    let ms = parse_u64_range(flag, &v, MIN_INTERVAL_MS, MAX_INTERVAL_MS).map_err(
                        |e| match e {
                            // 区间错误直接指向设计依据，便于现场诊断（§4.2.1 时延拆解）。
                            ConfigError::Invalid(f, val, _) => ConfigError::Invalid(
                                f,
                                val,
                                format!(
                                    "轮询节拍须在 {MIN_INTERVAL_MS}..={MAX_INTERVAL_MS} ms 之间\
                                     （设计 §5.5 红线：>500ms 会破坏「告警/联锁上屏 ≤2s」验收）"
                                ),
                            ),
                            other => other,
                        },
                    )?;
                    cfg.interval_ms = ms;
                }
                "--control-channel" => {
                    let v = take(flag)?;
                    // 与读通道同口径校验：仅 http:// 回环、无 TLS（设计 §1.4 PL-8）。
                    ChannelEndpoint::parse(&v)
                        .map_err(|e| ConfigError::Invalid(flag.into(), v.clone(), e.to_string()))?;
                    cfg.control_channel = v;
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
                "--touch-device" => {
                    let v = take(flag)?;
                    if v.is_empty() {
                        return Err(ConfigError::Invalid(flag.into(), v, "路径不能为空".into()));
                    }
                    cfg.touch_device = Some(PathBuf::from(v));
                }
                "--touch-calib" => {
                    let v = take(flag)?;
                    let b = CalibBounds::parse(&v)
                        .map_err(|why| ConfigError::Invalid(flag.into(), v.clone(), why))?;
                    cfg.touch.calib = Some(b);
                }
                "--touch-swap-xy" => cfg.touch.swap_xy = parse_switch(flag, &inline)?,
                "--touch-invert-x" => cfg.touch.invert_x = parse_switch(flag, &inline)?,
                "--touch-invert-y" => cfg.touch.invert_y = parse_switch(flag, &inline)?,
                "--rotate" => {
                    let v = take(flag)?;
                    let r = Rotate::parse(&v).ok_or_else(|| {
                        ConfigError::Invalid(flag.into(), v.clone(), "取值须为 0|90|180|270".into())
                    })?;
                    // 响亮失败（评审 C-③）：非 0 旋转**未实现**（薄层缺 `lv_display_set_rotation`），
                    // 必须启动即报错 —— 绝不解析后静默忽略（那是本项目最忌讳的「静默 no-op」）。
                    if r != Rotate::Deg0 {
                        return Err(ConfigError::Invalid(
                            flag.into(),
                            v,
                            "屏幕旋转暂未实现（需薄层 `lv_display_set_rotation` 能力，见待办）；\
                             请去掉 --rotate 或改用 --rotate 0"
                                .into(),
                        ));
                    }
                    cfg.rotate = r;
                }
                "--idle-timeout-secs" => {
                    let v = take(flag)?;
                    cfg.idle_timeout_secs =
                        parse_u64_range(flag, &v, 0, MAX_IDLE_TIMEOUT_SECS)?;
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

/// 解析布尔开关：`--flag`（无值 ⇒ `true`）或 `--flag=true|false|1|0|yes|no`。
///
/// 为什么允许 `=false`：部署脚本常以变量拼参数（`--touch-swap-xy=$SWAP`），
/// 显式 false 比"拼接时删掉整个参数"更不易出错；非法取值仍**明确报错**（不静默当 true）。
fn parse_switch(flag: &str, inline: &Option<String>) -> Result<bool, ConfigError> {
    match inline {
        None => Ok(true),
        Some(v) => match v.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Ok(true),
            "false" | "0" | "no" | "off" => Ok(false),
            other => Err(ConfigError::Invalid(
                flag.into(),
                other.to_string(),
                "布尔取值须为 true|false".into(),
            )),
        },
    }
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
  --channel <URL>         数据通道（回环 HTTP GET 最新帧）
                          默认 {default_url}
  --control-channel <URL>  控制通道基址（回环；仅 GET 查询 + POST 写受控接口）
                          默认 {default_ctl}
  --poll-ms <MS>           轮询节拍（--interval 同义），{min_i}..={max_i}，默认 {interval}
                           ⚠️ >{max_i} 直接报错（设计 §5.5：影响端到端 ≤2s 验收）
  --interval <MS>          --poll-ms 的 v1.0 别名（同区间）
  --stale-ms <MS>          数据过期阈值，{min_s}..={max_s}，默认 {stale}
  --backend <NAME>         offscreen|fb0|drm（fb0 = fbdev），默认 {backend}
                           （fb0/drm 仅 Linux；Windows 开发用 offscreen）
  --fbdev-path <PATH>      framebuffer 设备，默认 {fbdev}
  --width <PX>             渲染宽度，{min_d}..={max_d}，默认 {w}
  --height <PX>            渲染高度，{min_d}..={max_d}，默认 {h}
  --rotate <DEG>           [暂未实现] 仅接受 0（90|180|270 会启动即报错退出）
                           ⚠️ 施加旋转待薄层 lv_display_set_rotation 补齐（不做静默忽略）
  --font <PATH>            外部/系统字库(.otf/.ttf)；空=捆绑子集或内置 ASCII 回退
  --touch-device <PATH>    触摸设备节点；缺省 = 自动发现（多候选即报错并列出）
                           生产固定 /dev/mupc-touch（udev 符号链接，设计 §12.2）
  --touch-calib <V>        原始量程 xmin,xmax,ymin,ymax（覆盖设备 EVIOCGABS 自报值）
  --touch-swap-xy[=BOOL]   交换 X/Y 轴（面板旋转 90/270°）
  --touch-invert-x[=BOOL]  水平翻转
  --touch-invert-y[=BOOL]  垂直翻转
  --idle-timeout-secs <S>  无触摸回归主状态页秒数，0..={max_idle}，默认 {idle}（0=禁用）
  -h, --help               显示本帮助
  -V, --version            显示版本

说明：
  * 本进程只读数据通道（GET 最新帧），不下发任何指令；不读 mupc_core_config.yaml（设计 §7.2）。
  * 生产 unit 的 --channel 须与 mupcd 配置 display.bind_addr 对齐，
    --control-channel 须与 display.control_bind_addr 对齐（两者均硬绑 127.0.0.1）。
  * 触摸设备打开失败不影响数据刷新（EDGE-13）：仅 warn + 屏幕角标「触摸不可用」。
  * 字库子集化命令见 crates/local-display/src/font.rs 顶部注释。
",
        default_url = mupc_display_proto::DEFAULT_CHANNEL_URL,
        default_ctl = mupc_display_proto::DEFAULT_CONTROL_BASE_URL,
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
        max_idle = MAX_IDLE_TIMEOUT_SECS,
        idle = DEFAULT_IDLE_TIMEOUT_SECS,
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

    // ---- v2.0 新增 CLI（工作单元 C） ----

    #[test]
    fn v2_defaults_match_design() {
        let c = CliConfig::default();
        assert_eq!(c.control_channel, "http://127.0.0.1:9811");
        assert_eq!(c.touch_device, None, "缺省自动发现（生产 unit 固定 --touch-device）");
        assert_eq!(c.touch, crate::touch::TouchOverrides::default());
        assert_eq!(c.rotate, Rotate::Deg0);
        assert_eq!(c.idle_timeout_secs, 60, "TT-12 默认 60 s");
    }

    #[test]
    fn parse_v2_flags_both_forms() {
        let c = CliConfig::parse(&args(&[
            "--poll-ms",
            "250",
            "--control-channel=http://127.0.0.1:9811",
            "--touch-device",
            "/dev/mupc-touch",
            "--touch-calib=0,4095,0,4095",
            "--touch-swap-xy",
            "--touch-invert-x=true",
            "--touch-invert-y=false",
            "--rotate",
            "0",
            "--idle-timeout-secs=0",
        ]))
        .unwrap();
        assert_eq!(c.interval_ms, 250);
        assert_eq!(c.control_channel, "http://127.0.0.1:9811");
        assert_eq!(c.touch_device.as_deref(), Some(std::path::Path::new("/dev/mupc-touch")));
        assert_eq!(
            c.touch.calib,
            Some(crate::touch::CalibBounds {
                x_min: 0,
                x_max: 4095,
                y_min: 0,
                y_max: 4095
            })
        );
        assert!(c.touch.swap_xy);
        assert!(c.touch.invert_x);
        assert!(!c.touch.invert_y, "--touch-invert-y=false 须显式关闭");
        assert_eq!(c.rotate, Rotate::Deg0, "非 0 旋转暂未实现 ⇒ 仅 0 可解析通过");
        assert_eq!(c.idle_timeout_secs, 0, "0 = 禁用空闲回归");
    }

    /// 评审 C-③：非 0 `--rotate` **响亮失败**（本轮不实现旋转，也不静默忽略 = 静默 no-op）。
    #[test]
    fn non_zero_rotate_fails_loudly_until_implemented() {
        for v in ["90", "180", "270", "deg90", "DEG270"] {
            let e = CliConfig::parse(&args(&["--rotate", v])).unwrap_err();
            match e {
                ConfigError::Invalid(f, val, why) => {
                    assert_eq!(f, "--rotate");
                    assert_eq!(val, v);
                    assert!(why.contains("未实现"), "须说明「未实现」：{why}");
                    assert!(
                        why.contains("lv_display_set_rotation"),
                        "须指向薄层能力缺口：{why}"
                    );
                    assert!(why.contains("--rotate 0"), "须给出处置手段：{why}");
                }
                other => panic!("--rotate {v} 应响亮失败，实得 {other:?}"),
            }
        }
        // 显式 0 / deg0 仍可用（等价默认）
        assert_eq!(
            CliConfig::parse(&args(&["--rotate", "0"])).unwrap().rotate,
            Rotate::Deg0
        );
        assert_eq!(
            CliConfig::parse(&args(&["--rotate=deg0"])).unwrap().rotate,
            Rotate::Deg0
        );
        assert_eq!(CliConfig::default().rotate, Rotate::Deg0);
    }

    /// `--help` 须**如实**标注 `--rotate` 当前状态（未实现，不是「可选可用」）。
    #[test]
    fn help_text_marks_rotate_as_unimplemented() {
        let h = help_text();
        assert!(h.contains("--rotate"), "帮助仍须列该参数（防部署脚本踩空）");
        assert!(h.contains("未实现"), "帮助须如实标注未实现：\n{h}");
    }

    #[test]
    fn interval_and_poll_ms_are_the_same_knob() {
        let a = CliConfig::parse(&args(&["--interval", "300"])).unwrap();
        let b = CliConfig::parse(&args(&["--poll-ms", "300"])).unwrap();
        assert_eq!(a.interval_ms, b.interval_ms);
        // 设计 §5.5 红线：>500 报错（既有 --interval 一并收紧到同一区间）
        for (flag, val) in [("--poll-ms", "501"), ("--interval", "501"), ("--poll-ms", "99")] {
            let e = CliConfig::parse(&args(&[flag, val])).unwrap_err();
            match e {
                ConfigError::Invalid(f, v, why) => {
                    assert_eq!(f, flag);
                    assert_eq!(v, val);
                    assert!(why.contains("500") && why.contains("§5.5"), "须指向设计依据：{why}");
                }
                other => panic!("{flag} {val} 应判非法，实得 {other:?}"),
            }
        }
        assert_eq!(MIN_INTERVAL_MS, 100);
        assert_eq!(MAX_INTERVAL_MS, 500);
    }

    #[test]
    fn v2_invalid_values_rejected() {
        for (flag, val) in [
            ("--rotate", "45"),
            ("--rotate", "x"),
            ("--idle-timeout-secs", "3601"),
            ("--idle-timeout-secs", "-1"),
            ("--touch-calib", "0,100,0"),        // 段数不足
            ("--touch-calib", "100,100,0,100"),  // max<=min
            ("--touch-calib", "a,100,0,100"),
            ("--control-channel", "https://127.0.0.1:9811"), // 非回环 http
            ("--control-channel", "127.0.0.1:9811"),
            // 布尔开关的非法取值只能走 `=` 形式（`--flag value` 会把 value 当新参数）
            ("--touch-swap-xy=maybe", ""),
            ("--touch-device", ""),
        ] {
            let e = CliConfig::parse(&args(&[flag, val])).unwrap_err();
            assert!(
                matches!(e, ConfigError::Invalid(_, _, _)),
                "{flag} {val} 应判非法，实得 {e:?}"
            );
        }
    }

    #[test]
    fn backend_accepts_fb0_alias() {
        let c = CliConfig::parse(&args(&["--backend", "fb0"]));
        if cfg!(target_os = "linux") {
            assert_eq!(c.unwrap().backend, Backend::Fbdev);
        } else {
            // 非 Linux 上 fb0 与 fbdev 一样明确报错（不静默回退 offscreen）
            assert!(matches!(c.unwrap_err(), ConfigError::Invalid(f, _, _) if f == "--backend"));
        }
        assert_eq!(Backend::parse("fb0"), Some(Backend::Fbdev));
    }

    #[test]
    fn rotate_parse_and_axis_swap() {
        assert_eq!(Rotate::parse("0"), Some(Rotate::Deg0));
        assert_eq!(Rotate::parse(" 90 "), Some(Rotate::Deg90));
        assert_eq!(Rotate::parse("180"), Some(Rotate::Deg180));
        assert_eq!(Rotate::parse("DEG270"), Some(Rotate::Deg270));
        assert_eq!(Rotate::parse("91"), None);
        assert_eq!(Rotate::Deg0.degrees(), 0);
        assert_eq!(Rotate::Deg270.degrees(), 270);
        assert!(!Rotate::Deg0.swaps_axes());
        assert!(Rotate::Deg90.swaps_axes());
        assert!(!Rotate::Deg180.swaps_axes());
        assert!(Rotate::Deg270.swaps_axes());
        assert_eq!(Rotate::Deg90.as_str(), "90");
    }

    #[test]
    fn v2_help_text_lists_all_new_flags() {
        let h = help_text();
        for f in [
            "--poll-ms",
            "--control-channel",
            "--touch-device",
            "--touch-calib",
            "--touch-swap-xy",
            "--touch-invert-x",
            "--touch-invert-y",
            "--rotate",
            "--idle-timeout-secs",
            "fb0", // `--backend` 的取值别名（帮助行 `offscreen|fb0|drm`）
        ] {
            assert!(h.contains(f), "帮助文本缺 {f}");
        }
    }

    #[test]
    fn config_builds_touch_and_idle_handles() {
        let c = CliConfig::parse(&args(&[
            "--touch-device",
            "/dev/mupc-touch",
            "--touch-calib",
            "0,100,0,200",
            "--touch-swap-xy",
            "--width",
            "800",
            "--height",
            "600",
            "--idle-timeout-secs",
            "30",
        ]))
        .unwrap();
        let t = c.touch_config();
        assert_eq!(t.device.as_deref(), Some(std::path::Path::new("/dev/mupc-touch")));
        assert_eq!((t.width, t.height), (800, 600));
        assert!(t.overrides.swap_xy);

        let idle = c.idle_timer(1_000);
        assert_eq!(idle.timeout_secs(), 30);
        assert_eq!(idle.deadline_ms(), Some(31_000));
        assert!(!CliConfig::default().idle_timer(0).is_disabled());
    }
}
