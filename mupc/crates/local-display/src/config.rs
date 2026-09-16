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
//! - **未实现的能力 = 响亮失败，绝不静默 no-op**：
//!   `--rotate` **非 0 取值仍启动即硬错误**（B4a 整改「重要 2」恢复的口径；越界取值同）：
//!   薄层 `Display::set_rotation` **已具备**，但 **sink 侧像素旋转未实现** ⇒ 非 0 取值只换
//!   逻辑分辨率、写进像素面的仍是未旋转的那一份，即**主动画错版式**（比静默 no-op 更坏）
//!   ⇒ 解析期即拒绝，错误文案点名"像素级旋转尚未实现"。见 [`Rotate`] 的状态说明。
//! - 手写参数解析（不引 clap）：选项少、无常量、可确定性单测，依赖面保持最小。
//!
//! ⚠️ 本文件仅**解析**，不构造后端/字库/触摸设备——后端的平台可用性错误在 **bin 层**
//! （`main.rs::run_process`）以明确错误返回（`--backend drm` 未实现，见 [`Backend::Drm`]）。
//! （v1.0 的 `crate::run` 模块已随 LVGL 链路删除，见 `lib.rs`「已废弃的 v1.0 模块」。）

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
/// # 状态（**B4a 整改「重要 2」，2026-09-16：非 0 仍硬错误**）
///
/// **现口径**：**只有 `0` 被放行**；`90|180|270` 在 `CliConfig::parse` 里**启动即硬错误**
/// （exit 2，错误文案点名"像素级旋转（sink 侧）尚未实现"）；**越界取值**（`45` / `x` / 空串）
/// 另有一层：`Rotate::parse` 返回 `None` ⇒ 拒于"取值须为 0|90|180|270"。
///
/// **沿革（读起来要连着看）**：
/// 1. **原口径（工作单元 C 评审 C-③）**：非 0 硬错误 —— 当时薄层未暴露旋转通道、
///    `allowlist.txt` 也无 `lv_display_set_rotation` ⇒"解析了但无人消费"的静默 no-op
///    必须响亮拒绝。
/// 2. **B4a 一度改为"四档全放行 + 启动告警"**：薄层已补
///    [`crate::lvgl::display::Display::set_rotation`]（+ 读回
///    [`crate::lvgl::display::Display::rotation`]），由 `app.rs` 在 `Display` 建立后施加。
/// 3. **本口径（B4a 整改，主控采纳评审意见）**：**退回硬错误**。评审实测
///    `--rotate 90 --smoke` ⇒ `result=OK` / exit 0，而 P4 的 `active_px` 由 **524189**
///    变为 **406880** ⇒ **像素面确已画错**、而自检五道门禁**一道都抓不到**。
///    ①"逻辑分辨率互换 + 像素不旋转"**不是 no-op**，而是**主动画错版式**，比静默 no-op 更坏；
///    ② 与第 1 条同一拓扑下的处置自相一致（"重启循环"论据在两处都成立或不成立，不能只在这处失效）；
///    ③ 配置类错误 fail-fast 在 journal 里**可见、可诊断**。
///
/// ⚠️ **能力边界（如实标注，不夸大）**：`lv_display_set_rotation` 只做**逻辑分辨率互换**
/// （`lv_display.c`：写 `disp->rotation` + `update_resolution()`）；**像素的物理旋转由驱动/
/// sink 负责**（官方 fbdev 驱动在 flush 里 `lv_display_rotate_area` + `lv_draw_sw_rotate`
/// 后写 fb）。本项目 `screen::Blitter` **未**实现该变换。
///
/// **本类型的四个取值与其薄层接线全部保留**（`Rotate` / `lvgl::display::Rotation` /
/// `Display::set_rotation` / `Display::rotation` / `app::rotation_of`）——
/// **sink 侧像素旋转实现后摘除本 guard 即可**，无需重建链路。
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
    /// `--font` 的取值（**v2.0 已废弃：该值被忽略**）。
    ///
    /// 字库在**构建期**由 `lv_font_conv` 产物绑定（`lvgl-sys` 的 `noto-font` feature；
    /// 生成脚本 `crates/local-display/fonts/gen_fonts.sh`），运行期不再加载外部字库
    /// ⇒ 本字段只解析、只用于**启动期响亮告警**（见 `main.rs::run_process`）。
    /// v1.0 的 `bundled-font` feature 与 `font.rs` 已删除。
    pub font: Option<PathBuf>,
    /// 触摸设备节点（None = 自动发现；生产 unit 固定 `/dev/mupc-touch`，设计 §12.2）。
    pub touch_device: Option<PathBuf>,
    /// 触摸校准与轴变换覆盖（`--touch-calib/--touch-swap-xy/--touch-invert-{x,y}`，设计 §1.2）。
    pub touch: TouchOverrides,
    /// 屏幕旋转（B4a 整改后：**只有 `0` 可解析通过**；非 0 与越界取值都在解析期硬错误 ⇒
    /// 本字段**在解析成功的路径上恒为 [`Rotate::Deg0`]**）。
    ///
    /// 薄层旋转通道（[`crate::lvgl::display::Display::set_rotation`]）与 `app.rs::rotation_of`
    /// 的接线**保留** —— sink 侧像素旋转实现后摘除 [`CliConfig::parse`] 里那道 guard 即可。
    pub rotate: Rotate,
    /// 空闲回归秒数（TT-12；默认 60，`0` = 禁用）。
    pub idle_timeout_secs: u64,
    /// `--smoke`：**一键自检**（设计 §9「HMI：`--backend offscreen --smoke`」）——
    /// 起事件循环跑有限拍 → 逐页渲染 6 页并打印逐页像素统计 → 退出（自检主体见
    /// [`crate::app::App::smoke`]；六页任一为空 ⇒ **退出码 3**，常量在 bin
    /// `main.rs::EXIT_SMOKE` —— 本文件是 lib，读不到 bin 常量，故此处只写语义）。
    pub smoke: bool,
    /// `--smoke-out <PATH>`：自检导出 **PPM(P6)** 的路径（**仅与 `--smoke` 联用**；
    /// 缺省 = 不导出）。PPM 而 PNG 的理由见 [`crate::screen::MemorySink::write_ppm`]
    /// （零依赖；本 crate 不引图像编码依赖）。
    pub smoke_out: Option<PathBuf>,
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
            smoke: false,
            smoke_out: None,
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

    // 【B3-2b-1 已删除】`CliConfig::idle_timer(now_ms) -> timing::IdleTimer`
    //
    // 该方法是 `--idle-timeout-secs` → 计时器的便捷构造口，**生产装配从未消费**（`App` 只把
    // 秒数经 `Shell::set_idle_timeout` 注入；`ui/shell.rs` 的 `Core::tick` 才是 TT-12 状态机
    // 的唯一真源）。B3-2a 质量评审 建议 I-5 登记"留到 B3-2b 删"，本单元执行。
    // 见 `timing.rs` 顶部「【B3-2b-1 已删除】`IdleTimer`」块的**边界核对表**。

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
                    // **越界值仍须响亮报错**（评审 C-③ 的口径不变）：只放行 0|90|180|270，
                    // 其余（含 `45` / `x` / 空串）照旧启动即退出，不静默取默认值。
                    let r = Rotate::parse(&v).ok_or_else(|| {
                        ConfigError::Invalid(flag.into(), v.clone(), "取值须为 0|90|180|270".into())
                    })?;
                    // **B4a 整改（重要 2）：非 0 取值退回「启动即硬错误」。**
                    // 评审实测：`--rotate 90 --smoke` 曾 `result=OK` / exit 0，而 P4 的
                    // `active_px` 由 524189 变 406880 ⇒ **像素面确已画错**、自检门禁完全抓不到。
                    // 理由：①「逻辑分辨率互换 + 像素不旋转」不是 no-op，而是**主动画错版式**，
                    // 比静默 no-op 更坏；② 与 B4a 之前同一拓扑下的处置（也是硬错误）自相一致；
                    // ③ 配置类错误 fail-fast 在 journal 里可见、可诊断。
                    // 薄层能力（`Rotation` / `Display::set_rotation` / `rotation()`）与
                    // `app.rs::rotation_of` 的全部接线**保留不动** —— 待 sink 侧像素旋转落地后，
                    // 摘除本 guard 即可（见 [`Rotate`] 的状态说明）。
                    if r != Rotate::Deg0 {
                        return Err(ConfigError::Invalid(
                            flag.into(),
                            v,
                            "像素级旋转（sink 侧）尚未实现：非 0 取值会主动画错版式，故启动即拒绝；\
                             待 sink 侧实现像素旋转后摘除本 guard"
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
                "--smoke" => cfg.smoke = parse_switch(flag, &inline)?,
                "--smoke-out" => {
                    let v = take(flag)?;
                    if v.is_empty() {
                        return Err(ConfigError::Invalid(flag.into(), v, "路径不能为空".into()));
                    }
                    cfg.smoke_out = Some(PathBuf::from(v));
                }
                other => return Err(ConfigError::UnknownArg(other.to_string())),
            }
        }
        // 组合校验（**响亮失败，不静默忽略**）：`--smoke-out` 只被 `--smoke` 消费；
        // 单给它在当前实现里就是「解析了但无人消费」的静默 no-op（本项目最忌讳的一类）。
        if let Some(p) = &cfg.smoke_out {
            if !cfg.smoke {
                return Err(ConfigError::Invalid(
                    "--smoke-out".into(),
                    p.display().to_string(),
                    "仅与 --smoke 联用（否则该路径不会被写入）".into(),
                ));
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
  --control-channel <URL>  [B3-2b-2 已接线] 控制通道基址（回环；GET 查询 + POST 写受控接口）
                          默认 {default_ctl}
                          ⚠️ 该地址非法 ⇒ 启动即报错退出（不静默降级成只读屏）；
                          启动时会回显实际取值。写操作**只**在页面确认完成后发起，
                          未确认 = 零网络动作（设计 §6.2 T-3 闭环）
  --poll-ms <MS>           轮询节拍（--interval 同义），{min_i}..={max_i}，默认 {interval}
                           ⚠️ >{max_i} 直接报错（设计 §5.5：影响端到端 ≤2s 验收）
  --interval <MS>          --poll-ms 的 v1.0 别名（同区间）
  --stale-ms <MS>          数据过期阈值，{min_s}..={max_s}，默认 {stale}
  --backend <NAME>         offscreen|fb0|drm（fb0 = fbdev），默认 {backend}
                           （fb0/drm 仅 Linux；Windows 开发用 offscreen）
  --fbdev-path <PATH>      framebuffer 设备，默认 {fbdev}
  --width <PX>             渲染宽度，{min_d}..={max_d}，默认 {w}
  --height <PX>            渲染高度，{min_d}..={max_d}，默认 {h}
  --rotate <DEG>           默认 0。**当前只有 0 可用**：非 0（90/180/270）与越界取值
                           一律启动即报错退出（退出码 2）
                           ⚠️ 原因：薄层旋转通道已具备，但 sink 侧**像素级旋转未实现**
                           ⇒ 非 0 只换逻辑分辨率、像素面仍按未旋转那份画（主动画错版式）
  --font <PATH>            [v2.0 已废弃：字库在构建期由 lv_font_conv 产物绑定
                           （--features noto-font，见 fonts/gen_fonts.sh），
                           本参数被忽略；启动时会打印响亮告警]
  --touch-device <PATH>    触摸设备节点；缺省 = 自动发现（多候选即报错并列出）
                           生产固定 /dev/mupc-touch（udev 符号链接，设计 §12.2）
  --touch-calib <V>        原始量程 xmin,xmax,ymin,ymax（覆盖设备 EVIOCGABS 自报值）
  --touch-swap-xy[=BOOL]   交换 X/Y 轴（面板旋转 90/270°）
  --touch-invert-x[=BOOL]  水平翻转
  --touch-invert-y[=BOOL]  垂直翻转
  --idle-timeout-secs <S>  无触摸回归主状态页秒数，0..={max_idle}，默认 {idle}（0=禁用）
  --smoke                  一键自检：跑有限拍 → 逐页渲染 6 页 → 打印逐页像素统计 → 退出
                           （六页任一为空 ⇒ 返回码 3，常量 `EXIT_SMOKE` 见 bin main.rs；
                             配合 --backend offscreen 本机可跑）
  --smoke-out <PATH>       自检导出 PPM 的路径（**仅与 --smoke 联用**，缺省不导出）
  -h, --help               显示本帮助
  -V, --version            显示版本

说明：
  * 本进程只读数据通道（GET 最新帧），不下发任何指令；不读 mupc_core_config.yaml（设计 §7.2）。
  * 生产 unit 的 --channel 须与 mupcd 配置 display.bind_addr 对齐，
    --control-channel 须与 display.control_bind_addr 对齐（两者均硬绑 127.0.0.1）。
  * 触摸设备打开失败不影响数据刷新（EDGE-13）：仅 warn + 屏幕角标「触摸不可用」。
  * 字库（中文上屏）在**构建期**由 lv_font_conv 产物绑定：先跑
    crates/local-display/fonts/gen_fonts.sh，再以 --features noto-font 构建（设计 §1.1.2）。
    v1.0 的 font.rs 与 --features bundled-font 已删除。
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

// ═══════════════════════════════════════════════════════════════════════════
// 启动期**响亮告警**文案（B3-2a 规格评审 建议 4/5）
// ═══════════════════════════════════════════════════════════════════════════
//
// 为什么文案要落在 lib（而不是 bin 里的 `eprintln!` 字面量）：本 crate 的可测逻辑一律
// 不得留在 bin（`main.rs` 文件头：bin 不进 lib、集成测试够不着）—— 文案本身也是行为的一部分
// （"参数被忽略"**必须可见**），所以它要被用例锁住；bin 只负责打印。

/// `--font` 的启动期告警文案。
///
/// # 为什么是**告警**而不是硬错误（B3-2a 规格评审 建议 4 的主控裁定）
/// 本进程由 systemd `Restart=always` 托管：把它降级成硬错误 ⇒ 现场带 `--font` 的 unit
/// 会**起不来并无限重启**（服务不可用且无人值守），而它只是"这个兼容参数没用了"。
/// 但"参数被忽略"这件事**必须可见** ⇒ 保留本告警 + `--help` 如实标注已废弃
/// （[`help_text`] 的 `--font` 行），绝不留下一条"看起来可用"的骗人 help。
pub fn font_ignored_warning(path: &std::path::Path) -> String {
    format!(
        "⚠️ --font {} 在 v2.0 不生效（参数被忽略）：中文上屏改由**构建期**绑定的 LVGL 位图字库\
         承担（lvgl-sys 的 `noto-font` feature，见 fonts/gen_fonts.sh 与 lvgl/font.rs）；\
         该参数仅为 CLI 兼容保留，可安全去掉。",
        path.display()
    )
}

// ── `rotate_pixel_gap_warning`（B4a 引入、B4a 整改「重要 2」**删除**）────────────
//
// 它曾是 `--rotate` 非 0 时的启动期"能力边界告警"（理由与 [`font_ignored_warning`] 同款：
// systemd `Restart=always` ⇒ 硬错误会造成重启循环）。B4a 整改把非 0 退回**解析期硬错误**
// 后，进程**永远到不了"带着非 0 旋转启动"**这一步 ⇒ 该告警**不再有可达调用点**。
// 保留它 = 死代码 + 一条恒为 `None` 的分支（本项目明令禁止）⇒ **连同 `main.rs` 的调用点
// 一并删除**；同一事实现由两处**可见**承担：① 解析期错误文案（点名"像素级旋转（sink 侧）
// 尚未实现"）；② `--help` 的 `--rotate` 行。能力边界本身记在 [`Rotate`] 的文档里。

/// `--control-channel` 的启动期**生效回显**（B3-2b-2：该参数已接线）。
///
/// # 沿革（B3-2a 的"待接线告警"已按设计删除）
///
/// B3-2a 时的形态是 `control_channel_pending_warning`（**该函数已在 B3-2b-2 整体删除**；
/// 此处写**纯文本**而非 intra-doc 链接 —— 指向已删符号的链接是**悬空链接**，会让
/// `cargo doc` 报 `unresolved link`）：「⚠️ 本参数当前**不影响行为**，
/// `ConsoleClient` 将在 B3-2b 接线」。B3-2b-2 把 `ConsoleClient` 接进 `app.rs` 之后该文案
/// **已经失真**（参数此刻**真的**影响行为）⇒ **整体删除**（连同 `main.rs` 的调用点与 help 行），
/// 换成这条**如实回显**：说明它已生效、地址非法会启动失败、以及 T-3 的写操作门禁。
///
/// # 为什么仍然打印（而不是静默生效）
///
/// 现场排障要能一眼看到「本进程到底在连哪个控制通道」（`--channel` 的回显是同一取向）；
/// 且 T-3 的"未确认 = 零网络动作"是本模块最容易被误读的一条 —— 写在启动行上，
/// 部署与评审都有据可查。
pub fn control_channel_notice(url: &str) -> String {
    format!(
        "控制通道 --control-channel {url}（B3-2b-2 已接线：GET 查询 + POST 写受控接口；\
         写操作**只**在页面确认完成后发起，未确认 = 零网络动作）"
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

    /// `--smoke` / `--smoke-out`（设计 §9 一键自检）。
    #[test]
    fn smoke_flags_parse_and_are_off_by_default() {
        let c = CliConfig::default();
        assert!(!c.smoke, "默认必须关闭（自检不得在生产 unit 上误触发）");
        assert!(c.smoke_out.is_none());

        let c = CliConfig::parse(&args(&["--smoke"])).unwrap();
        assert!(c.smoke);
        assert!(c.smoke_out.is_none(), "不导出时无需路径");

        let c = CliConfig::parse(&args(&["--smoke", "--smoke-out", "out.ppm"])).unwrap();
        assert!(c.smoke);
        assert_eq!(c.smoke_out, Some(PathBuf::from("out.ppm")));

        // `--smoke=false` 显式关闭（部署脚本拼参数时不易出错，与其它开关同口径）
        assert!(!CliConfig::parse(&args(&["--smoke=false"])).unwrap().smoke);
    }

    /// **`--smoke-out` 单独出现必须报错**（否则就是"解析了但无人消费"的静默 no-op）。
    #[test]
    fn smoke_out_without_smoke_is_rejected() {
        let e = CliConfig::parse(&args(&["--smoke-out", "out.ppm"])).unwrap_err();
        match e {
            ConfigError::Invalid(f, v, why) => {
                assert_eq!(f, "--smoke-out");
                assert_eq!(v, "out.ppm");
                assert!(why.contains("--smoke"), "错误须指出正确用法：{why}");
            }
            other => panic!("应判非法，实得 {other:?}"),
        }
        // 空路径同样响亮失败
        assert!(CliConfig::parse(&args(&["--smoke", "--smoke-out", ""])).is_err());
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
        assert_eq!(c.rotate, Rotate::Deg0);
        assert_eq!(c.idle_timeout_secs, 0, "0 = 禁用空闲回归");
    }

    /// **B4a 整改「重要 2」**：`--rotate 90|180|270`（含大小写 / `deg` 前缀 / 空白变体）
    /// **一律启动即硬错误**；`--rotate 0` 正常通过。
    ///
    /// **为什么退回硬错误**：评审实测 `--rotate 90 --smoke` 曾 `result=OK` / exit 0，
    /// 而 P4 的 `active_px` 由 `524189` 变 `406880` ⇒ **像素面确已画错**、自检门禁**完全抓不到**。
    /// "逻辑分辨率互换 + 像素不旋转"**不是 no-op**，是**主动画错版式** ⇒ 必须 fail-fast。
    ///
    /// **改什么会让本条变红**：把解析期的非 0 guard 摘掉（下面任一 `unwrap_err` 即红）。
    #[test]
    fn non_zero_rotate_is_rejected_at_parse_after_b4a_rework() {
        assert_eq!(
            CliConfig::parse(&args(&["--rotate", "0"]))
                .expect("--rotate 0（默认值）必须放行")
                .rotate,
            Rotate::Deg0
        );
        assert_eq!(
            CliConfig::parse(&args(&["--rotate=deg0"])).unwrap().rotate,
            Rotate::Deg0
        );
        assert_eq!(
            CliConfig::default().rotate,
            Rotate::Deg0,
            "默认仍是不旋转"
        );
        for v in ["90", "180", "270", "deg90", "DEG270", " 90 "] {
            assert!(
                CliConfig::parse(&args(&["--rotate", v])).is_err(),
                "--rotate {v} 应被拒绝（sink 侧像素旋转未实现）"
            );
        }
    }

    /// **B4a 整改「重要 2」的文案口径**：非 0 的拒绝必须**点名"像素级旋转"**且**说明是 sink 侧**
    /// —— 现场排障要能从 journal 一行看出"该参数不是拼错了、而是功能没做完"。
    ///
    /// **改什么会让本条变红**：把 guard 去掉（`unwrap_err` 变 `Ok` ⇒ panic），
    /// 或把错误文案换成泛泛的"非法取值"（下面的 `contains` 即红）。
    #[test]
    fn non_zero_rotate_error_names_the_pixel_rotation_gap() {
        for v in ["90", "180", "270"] {
            match CliConfig::parse(&args(&["--rotate", v])).unwrap_err() {
                ConfigError::Invalid(f, val, why) => {
                    assert_eq!(f, "--rotate");
                    assert_eq!(val, v);
                    assert!(
                        why.contains("像素级旋转"),
                        "--rotate {v} 的错误文案须点名「像素级旋转」：{why}"
                    );
                    assert!(
                        why.contains("sink"),
                        "--rotate {v} 的错误文案须指明是 **sink 侧**未实现（而非参数写错）：{why}"
                    );
                    assert!(
                        why.contains("尚未实现") || why.contains("未实现"),
                        "--rotate {v} 的错误文案须说明「未实现」：{why}"
                    );
                }
                other => panic!("--rotate {v} 应响亮失败，实得 {other:?}"),
            }
        }
    }

    /// **越界取值仍须响亮报错**（量程自工作单元 C 评审 C-③ 起未变 —— 只是"四档里三个"
    /// 也从放行退回拒绝，见上一条）。
    ///
    /// **改什么会让本条变红**：把 `Rotate::parse` 的兜底写成"未知值取 `Deg0`"（静默取默认）。
    #[test]
    fn out_of_range_rotate_still_fails_loudly() {
        for v in ["45", "x", "", "-90", "360"] {
            let e = CliConfig::parse(&args(&["--rotate", v])).unwrap_err();
            match e {
                ConfigError::Invalid(f, val, why) => {
                    assert_eq!(f, "--rotate");
                    assert_eq!(val, v);
                    assert!(
                        why.contains("0|90|180|270"),
                        "须点名合法取值集合：{why}"
                    );
                }
                other => panic!("--rotate {v:?} 应响亮失败，实得 {other:?}"),
            }
        }
    }

    /// `--help` 须**如实**标注 `--rotate` 当前状态：**只有 0 可用**；非 0 会被拒绝，
    /// 且**原因**（sink 侧像素旋转未实现）必须写在 help 里。
    ///
    /// **改什么会让本条变红**：help 行回退成"0|90|180|270 已放行"（与解析行为矛盾 ——
    /// 这正是 B4a 整改前的骗人 help），或抹掉"像素级旋转未实现"的原因标注。
    #[test]
    fn help_text_states_rotate_capability_and_its_gap() {
        let h = help_text();
        assert!(h.contains("--rotate"), "帮助仍须列该参数（防部署脚本踩空）");
        assert!(
            h.contains("像素级旋转") && h.contains("未实现"),
            "help 须保留「sink 侧像素级旋转未实现」的原因标注（不得把半成品写成完全可用）：\n{h}"
        );
        assert!(
            h.contains("启动即报错退出"),
            "help 须如实写「非 0 启动即报错退出」（与解析行为一致）：\n{h}"
        );
        assert!(
            !h.contains("0|90|180|270（默认 0）"),
            "旧的「0|90|180|270（默认 0）」（读起来像四档都放行）与解析行为矛盾：\n{h}"
        );
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

    // ---- B3-2a 规格评审整改：help 文案不得说谎（建议 3/4/5） ----

    /// `--help` 的 `--smoke` 行**必须以代码为准**（返回码 3 = bin `EXIT_SMOKE`），
    /// 且**不得**再出现旧的"返回码 1"（曾与 `deploy/local-display.md` 互相矛盾）。
    ///
    /// **改什么会让本条变红**：把 help 里的 `返回码 3` 写回 `返回码 1` ⇒ 第二条断言红。
    #[test]
    fn help_smoke_exit_code_matches_bin_constant() {
        let h = help_text();
        let smoke_line = h
            .lines()
            .find(|l| l.contains("六页任一为空"))
            .unwrap_or_else(|| panic!("help 里应有一行说明 --smoke 的失败返回码：\n{h}"));
        assert!(
            smoke_line.contains("返回码 3") && smoke_line.contains("EXIT_SMOKE"),
            "help 的 --smoke 行须写实返回码 3 并点名 bin 常量：{smoke_line}"
        );
        assert!(
            !smoke_line.contains("返回码 1"),
            "旧的「返回码 1」与 main.rs::EXIT_SMOKE=3 矛盾（部署文档据此照抄会误判）：{smoke_line}"
        );
    }

    /// `--font` 的 help 行须**如实标注已废弃**（不能再说"空=捆绑子集"——`bundled-font` 已删）。
    ///
    /// **改什么会让本条变红**：把该行改回 v1.0 文案「空=捆绑子集或内置 ASCII 回退」⇒ 红。
    #[test]
    fn help_marks_font_as_deprecated() {
        let h = help_text();
        // 该参数的 help 段 = `--font` 行 + 其续行（缩进对齐的后续行，直到下一个选项/空行）。
        let mut lines = h.lines().skip_while(|l| !l.trim_start().starts_with("--font"));
        let head = lines
            .next()
            .unwrap_or_else(|| panic!("help 仍须列 --font（防部署脚本踩空）：\n{h}"));
        let seg = std::iter::once(head)
            .chain(lines.take_while(|l| !l.starts_with("  --") && !l.trim().is_empty()))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(head.contains("已废弃"), "须如实标注已废弃：{head}");
        assert!(seg.contains("被忽略"), "须说明参数被忽略：\n{seg}");
        assert!(
            !h.contains("捆绑子集"),
            "`bundled-font` feature 已删除 ⇒ 不得再出现「捆绑子集」说法"
        );
        // 文案本身也要一致（启动告警与 help 讲同一件事）
        let w = font_ignored_warning(std::path::Path::new("/nope/x.otf"));
        assert!(w.contains("/nope/x.otf"), "告警须回显实际取值：{w}");
        assert!(w.contains("不生效") && w.contains("忽略"), "告警须说明不生效：{w}");
        assert!(
            w.contains("noto-font"),
            "告警须指向真正生效的路径（构建期字库）：{w}"
        );
    }

    /// `--control-channel` 须标注 `[B3-2b-2 已接线]`，且启动行**如实回显**（不再是"待接线"告警）。
    ///
    /// **改什么会让本条变红**：把该参数从 help 里删掉、去掉 `B3-2b-2` 标注、或把启动行改回
    /// "当前不影响行为"（B3-2b-2 之后那句已失真）⇒ 红。
    #[test]
    fn help_and_notice_register_control_channel_as_live() {
        let h = help_text();
        let line = h
            .lines()
            .find(|l| l.trim_start().starts_with("--control-channel"))
            .unwrap_or_else(|| panic!("help 仍须列 --control-channel：\n{h}"));
        assert!(line.contains("B3-2b-2"), "须标注接线单元：{line}");
        assert!(line.contains("已接线"), "须如实标注已接线：{line}");
        let n = control_channel_notice("http://127.0.0.1:9811");
        assert!(n.contains("B3-2b-2"), "启动行须给出接线单元：{n}");
        assert!(n.contains("9811"), "启动行须回显实际取值：{n}");
        assert!(
            n.contains("未确认 = 零网络动作"),
            "启动行须写明 T-3 门禁（写操作只在确认后发起）：{n}"
        );
        assert!(
            !n.contains("不影响行为"),
            "已接线后不得再声称「不影响行为」（该表述在 B3-2b-2 之后失真）：{n}"
        );
    }

    /// help 里**不得再引用已删除的 `font.rs`**（B3-2a 已删该模块 ⇒ 照抄者找不到文件）。
    #[test]
    fn help_does_not_point_at_deleted_font_rs() {
        let h = help_text();
        assert!(
            !h.contains("src/font.rs"),
            "help 指向已删除的 font.rs（应指向 fonts/gen_fonts.sh）：\n{h}"
        );
        assert!(
            h.contains("gen_fonts.sh"),
            "help 须给出真正在用的字库生成脚本：\n{h}"
        );
    }

    /// B3-2b-1 订正：原名 `config_builds_touch_and_idle_handles` —— `idle_timer()` 已随
    /// `timing::IdleTimer` 删除，故本用例只留触摸那一半，`--idle-timeout-secs` 的**解析**
    /// 仍在此断言（不丢覆盖）。对应的「0 = 禁用」**行为**断言已搬到
    /// `ui/tests.rs::shell_chain`（那里才是该语义的落点，见偏差 SH17）。
    #[test]
    fn config_builds_touch_handles() {
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
        assert_eq!(c.idle_timeout_secs, 30, "`--idle-timeout-secs` 仍被解析（注入 `Shell::set_idle_timeout`）");
    }
}
