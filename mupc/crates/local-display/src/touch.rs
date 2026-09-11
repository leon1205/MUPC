//! 触摸输入栈（工作单元 C；设计 §1.2 关键决策 B / §5.3 `touch.rs`，**纯 Rust evdev**）。
//!
//! # 分工与拆分（为什么这样切）
//!
//! | 部分 | 平台 | 内容 |
//! |------|------|------|
//! | **本文件上半** | **全平台** | 设计 §1.2 的**纯逻辑**：设备发现语义（能力判定 → 候选 → 多候选报错）、MT 协议 B 的**原始状态机**（[`RawState`]）、非阻塞读的**错误归类**（[`classify_fetch_error`] / [`pump_with`]）、校准线性映射（min/max + swap/invert + 边界钳制）、覆盖项合并、致命/可降级判定。**Windows 本机可测**（重评整改：原先把状态机与读路径整段关在 `cfg(linux)` 内 ⇒ 本机零测试，而它们恰是最易错的部分） |
//! | **本文件下半**（[`TouchDevice`]，`#[cfg(target_os = "linux")]`） | Linux | `evdev` crate 打开/枚举设备、`InputEvent` → [`RawEvent`] 降维、`EVIOCGABS` 量程、`poll(2)` 用 fd |
//!
//! # 事件投递终点（与 `src/lvgl/indev.rs` 的接口）
//!
//! 本模块**只**产出 [`crate::lvgl::indev::TouchSnapshot`]（`pressed` + **已校准到屏幕坐标**的
//! `x/y`）；命中测试 / z-order / 弹层拦截 / 滚动判定 / `LV_EVENT_*` 派发**全部交给 LVGL**
//! （设计 §5.3）。快照经 `Indev::feed()` 写入薄层，再由 `Indev::read()` 主动投递。
//!
//! # 重评整改要点
//!
//! - **Critical 1（空闲拍假错误）**：[`pump_with`] 把「非阻塞 fd 无事件」（`EWOULDBLOCK`/`EAGAIN`，
//!   即 [`std::io::ErrorKind::WouldBlock`]）与「真错误」分开 —— 前者是**正常的空闲拍**
//!   （契约：无事件 no-op），**绝不**上抛 [`TouchError::Io`]。否则事件循环每 500 ms 产生一条
//!   错误、unit B 据此降级会把触摸**永久判死**并造成 2 行/s 日志洪泛。
//! - **Important 5（启动即拒绝）**：[`detect_source_from_caps`] 收紧候选判定 —— 除「有绝对轴」
//!   外还要求**像触摸屏**的信号（`INPUT_PROP_DIRECT` 或 `BTN_TOUCH`），并**显式排除**
//!   `INPUT_PROP_ACCELEROMETER`。否则 RK3588 板载加速度计（`EV_ABS` + `ABS_X/Y`）会与触摸屏
//!   并列 ⇒ [`select_index`] 报 [`TouchError::Ambiguous`] ⇒ [`TouchError::is_fatal`] ⇒ 真机启动即拒绝。
//!
//! # 纪律
//!
//! - **不引入 libevdev**：`lv_conf.h` 的 `LV_USE_EVDEV = 0`（设计 §1.2 B-1/B-2）。
//! - **不提供跨线程 API**（设计 §5.2 不变量 4）：[`TouchDevice`] 绑定单线程事件循环。
//! - 本模块**不引用 `lvgl_sys`**（unsafe 边界纪律 1）；`evdev` 只在 Linux target 依赖中。
//! - **触摸失效不影响数据刷新**（EDGE-13）：[`TouchError::is_fatal`] 把「可降级」与
//!   「必须启动即报错」分开，调用方按此决定「warn 后继续只读展示」还是「退出」。

use std::path::PathBuf;

/// 触摸设备的坐标来源（协议判别结果；平台无关 —— 便于在无 evdev 的机器上测判定逻辑）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchSource {
    /// **多点协议 B**（首选）：`ABS_MT_POSITION_X/Y` + `ABS_MT_TRACKING_ID`（首活动 slot）。
    MultiTouch,
    /// **单点**（退化）：`ABS_X/ABS_Y` + `BTN_TOUCH`。
    Single,
}

impl TouchSource {
    /// 日志用短名。
    pub fn as_str(self) -> &'static str {
        match self {
            TouchSource::MultiTouch => "mt-protocol-b",
            TouchSource::Single => "single(abs)",
        }
    }
}

/// 设备能力摘要（**平台无关**；Linux 侧由 `evdev::Device` 的 `EVIOCGBIT` 结果填充）。
///
/// 存在的理由：把「什么算触摸屏」这条**判定**从 `cfg(linux)` 里解放出来 —— 它是启动期
/// 最致命的一处误判（Important 5），却完全不需要真设备即可测。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeviceCaps {
    /// 支持 `EV_ABS`（绝对坐标事件类）。
    pub has_abs: bool,
    /// 同时支持 `ABS_MT_POSITION_X` 与 `ABS_MT_POSITION_Y`（MT 协议 B 的坐标轴）。
    pub has_mt_xy: bool,
    /// 同时支持 `ABS_X` 与 `ABS_Y`（单点坐标轴；加速度计/摇杆也常有）。
    pub has_abs_xy: bool,
    /// 支持 `BTN_TOUCH`（触摸按下的按键码；加速度计/摇杆没有）。
    pub has_btn_touch: bool,
    /// 置位 `INPUT_PROP_DIRECT`（「直接输入设备」：触摸屏/绘图板把坐标直接映射到屏；加速度计没有）。
    pub direct: bool,
    /// 置位 `INPUT_PROP_ACCELEROMETER`（内核**明确**标为加速度计 ⇒ 一票否决，见
    /// [`detect_source_from_caps`]）。
    pub accelerometer: bool,
}

impl DeviceCaps {
    /// 「像触摸屏」的弱信号：`INPUT_PROP_DIRECT` **或** `BTN_TOUCH`。
    ///
    /// 取「或」而非「与」：任一信号在真机上都足以排除加速度计/摇杆，而要求「与」会误拒
    /// 部分合法面板（见 [`detect_source_from_caps`] 的宽松逃生门说明）。
    pub fn looks_like_touch(self) -> bool {
        self.direct || self.has_btn_touch
    }
}

/// 由设备能力判定坐标来源（**纯逻辑**；`cfg(linux)` 外可测）。
///
/// **用于设备发现（严格）** —— 这是「触摸屏 vs 板载加速度计并列」的闸门（Important 5）：
///
/// 1. 无 `EV_ABS` ⇒ 不是（`None`）；
/// 2. `INPUT_PROP_ACCELEROMETER` ⇒ **一票否决**（内核已明说它是加速度计）；
/// 3. 有 MT 轴 **且** [`DeviceCaps::looks_like_touch`] ⇒ [`TouchSource::MultiTouch`]；
/// 4. 有 `ABS_X/Y` **且** 有 `BTN_TOUCH` ⇒ [`TouchSource::Single`]（单点协议靠 `BTN_TOUCH`
///    判定按下，缺它就永远报不出「按下」—— 那种设备是加速度计/摇杆，不是触摸屏）；
/// 5. 其余 ⇒ `None`。
///
/// **误拒的代价是安全的**：自动发现返回 `None` ⇒ 候选为空 ⇒ [`TouchError::NoDevice`]，
/// 而它是**可降级**错误（EDGE-13：warn + 只读展示），**不会**导致启动即拒绝；
/// 若真机面板确实两个信号都缺，用 `--touch-device` 显式指定仍可启用（见
/// [`detect_source_lenient`]）。
pub fn detect_source_from_caps(caps: DeviceCaps) -> Option<TouchSource> {
    if !caps.has_abs || caps.accelerometer {
        return None;
    }
    if caps.has_mt_xy && caps.looks_like_touch() {
        return Some(TouchSource::MultiTouch);
    }
    if caps.has_abs_xy && caps.has_btn_touch {
        return Some(TouchSource::Single);
    }
    None
}

/// 由设备能力判定坐标来源（**宽松版**；仅用于 `--touch-device` **显式指定**的逃生门）。
///
/// 与 [`detect_source_from_caps`] 的唯一差别：**不要求**「像触摸屏」信号，只看坐标轴。
/// 理由：用户已经**明确**点名了设备节点，此时再因缺少 `INPUT_PROP_DIRECT`/`BTN_TOUCH` 而
/// 拒绝，会把一台**本来能用**的面板彻底关在门外（真机适配的最后一根救命稻草）。
/// 自动发现**不**用本函数 —— 那里必须严格，否则加速度计会与触摸屏并列而报 `Ambiguous`。
pub fn detect_source_lenient(caps: DeviceCaps) -> Option<TouchSource> {
    if !caps.has_abs {
        return None;
    }
    if caps.has_mt_xy {
        return Some(TouchSource::MultiTouch);
    }
    if caps.has_abs_xy {
        return Some(TouchSource::Single);
    }
    None
}

/// 设备候选（发现阶段的纯数据；不含设备句柄，便于注入式单测）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 设备节点路径（`/dev/input/eventN` 或 udev 稳定名 `/dev/mupc-touch`）。
    pub path: PathBuf,
    /// 内核上报设备名（`EVIOCGNAME`；多候选报错时**必须**列出，设计 §1.2）。
    pub name: Option<String>,
    /// 坐标来源。
    pub source: TouchSource,
}

impl Candidate {
    /// 一行可读描述（报错信息用）：`/dev/input/event3（Goodix TouchScreen, mt-protocol-b）`。
    pub fn describe(&self) -> String {
        match &self.name {
            Some(n) => format!("{}（{n}, {}）", self.path.display(), self.source.as_str()),
            None => format!("{}（{}）", self.path.display(), self.source.as_str()),
        }
    }
}

/// 由「路径 + 名 + 能力」构造候选（**发现语义的唯一入口**，纯逻辑、跨平台可测）。
///
/// 能力不像触摸屏 ⇒ `None`（跳过该设备，**不参与**多候选判定 —— Important 5 的落点）。
pub fn candidate_from_caps(
    path: impl Into<PathBuf>,
    name: Option<&str>,
    caps: DeviceCaps,
) -> Option<Candidate> {
    let source = detect_source_from_caps(caps)?;
    Some(Candidate {
        path: path.into(),
        name: name.map(str::to_string),
        source,
    })
}

/// 原始坐标边界（`--touch-calib xmin,xmax,ymin,ymax`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalibBounds {
    /// 原始 X 最小值（含）。
    pub x_min: i32,
    /// 原始 X 最大值（含）。
    pub x_max: i32,
    /// 原始 Y 最小值（含）。
    pub y_min: i32,
    /// 原始 Y 最大值（含）。
    pub y_max: i32,
}

impl CalibBounds {
    /// 解析 `xmin,xmax,ymin,ymax`（4 个整数，逗号分隔）。`max <= min` 即视为非法
    /// （设计 §1.2：「max<=min → 启动即报错，不静默用默认值」）。
    pub fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split(',').map(|p| p.trim()).collect();
        if parts.len() != 4 {
            return Err(format!("须为 `xmin,xmax,ymin,ymax` 四个整数，实得 {} 段", parts.len()));
        }
        let mut v = [0i32; 4];
        for (i, p) in parts.iter().enumerate() {
            v[i] = p
                .parse::<i32>()
                .map_err(|_| format!("第 {} 段 `{p}` 不是整数", i + 1))?;
        }
        let b = Self {
            x_min: v[0],
            x_max: v[1],
            y_min: v[2],
            y_max: v[3],
        };
        b.validate()?;
        Ok(b)
    }

    /// 校验：两轴的 `max > min`（跨度 > 0）。
    pub fn validate(&self) -> Result<(), String> {
        if self.x_max <= self.x_min {
            return Err(format!(
                "X 轴 max({}) <= min({})：无法线性映射，请用 --touch-calib 显式给出正确量程",
                self.x_max, self.x_min
            ));
        }
        if self.y_max <= self.y_min {
            return Err(format!(
                "Y 轴 max({}) <= min({})：无法线性映射，请用 --touch-calib 显式给出正确量程",
                self.y_max, self.y_min
            ));
        }
        Ok(())
    }

    /// 是否与另一组量程相同（用于「CLI 覆盖 vs 设备自报」的日志提示）。
    pub fn same_as(&self, other: &Self) -> bool {
        self == other
    }
}

/// 触摸相关 CLI 覆盖项（设计 §1.2「校准」行的三个开关 + `--touch-calib`）。
///
/// 全部为「现场适配」参数，**不写进 core 配置**（设计 §1.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TouchOverrides {
    /// `--touch-calib xmin,xmax,ymin,ymax`（None = 用设备 `EVIOCGABS` 自报量程）。
    pub calib: Option<CalibBounds>,
    /// `--touch-swap-xy`（面板旋转 90°/270° 时交换两轴）。
    pub swap_xy: bool,
    /// `--touch-invert-x`。
    pub invert_x: bool,
    /// `--touch-invert-y`。
    pub invert_y: bool,
}

/// 触摸配置（平台无关；供 `config.rs` 组装、Linux 侧消费）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TouchConfig {
    /// `--touch-device`：显式设备节点（None = 自动发现；生产固定 `/dev/mupc-touch`，§12.2）。
    pub device: Option<PathBuf>,
    /// CLI 覆盖项。
    pub overrides: TouchOverrides,
    /// 屏幕宽（校准目标）。
    pub width: u32,
    /// 屏幕高。
    pub height: u32,
}

impl TouchConfig {
    /// 便捷构造（测试/默认发现用）。
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            device: None,
            overrides: TouchOverrides::default(),
            width,
            height,
        }
    }
}

/// 校准表：原始坐标 → 屏幕坐标（设计 §1.2 校准行）。
///
/// 公式：`(raw − min) / (max − min) → [0,1]`，再按 `swap_xy` / `invert_x` / `invert_y`
/// 变换，最后线性缩放到 `[0, width-1] × [0, height-1]`（**闭区间**，`max` 落在最后一行/列）。
/// 原始值越界按 `[0,1]` 钳制（不产生屏外坐标）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Calibration {
    /// 原始 X 量程。
    pub bounds: CalibBounds,
    /// 交换两轴（在归一化之后、翻转之前施加）。
    pub swap_xy: bool,
    /// 水平翻转（在归一化之后施加）。
    pub invert_x: bool,
    /// 垂直翻转。
    pub invert_y: bool,
    /// 屏幕宽（像素）。
    pub screen_w: u32,
    /// 屏幕高。
    pub screen_h: u32,
}

impl Calibration {
    /// 构造并校验（`max<=min` 或屏幕尺寸为 0 → [`TouchError::InvalidCalibration`]）。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        bounds: CalibBounds,
        swap_xy: bool,
        invert_x: bool,
        invert_y: bool,
        screen_w: u32,
        screen_h: u32,
    ) -> Result<Self, TouchError> {
        bounds.validate().map_err(TouchError::InvalidCalibration)?;
        if screen_w == 0 || screen_h == 0 {
            return Err(TouchError::InvalidCalibration(format!(
                "屏幕尺寸不得为 0（{screen_w}x{screen_h}）"
            )));
        }
        Ok(Self {
            bounds,
            swap_xy,
            invert_x,
            invert_y,
            screen_w,
            screen_h,
        })
    }

    /// 原始坐标 → 屏幕坐标（越界钳制）。
    pub fn map(&self, raw_x: i32, raw_y: i32) -> (i32, i32) {
        let nx = normalize(raw_x, self.bounds.x_min, self.bounds.x_max);
        let ny = normalize(raw_y, self.bounds.y_min, self.bounds.y_max);
        let (mut fx, mut fy) = if self.swap_xy { (ny, nx) } else { (nx, ny) };
        if self.invert_x {
            fx = 1.0 - fx;
        }
        if self.invert_y {
            fy = 1.0 - fy;
        }
        (scale_axis(fx, self.screen_w), scale_axis(fy, self.screen_h))
    }
}

/// 归一化到 `[0,1]`（跨度 <= 0 视为 0；越界钳制）。
fn normalize(v: i32, lo: i32, hi: i32) -> f64 {
    let span = hi as f64 - lo as f64;
    if span <= 0.0 {
        return 0.0;
    }
    ((v as f64 - lo as f64) / span).clamp(0.0, 1.0)
}

/// 归一化值 → 像素坐标（闭区间 `[0, dim-1]`）。
fn scale_axis(f: f64, dim: u32) -> i32 {
    if dim == 0 {
        return 0;
    }
    let max = dim as f64 - 1.0;
    (f * max).round().clamp(0.0, max) as i32
}

/// 由「设备自报量程或 CLI 覆盖」+ 开关构造校准表（**纯逻辑**，Linux 侧与单测共用）。
///
/// 语义（设计 §1.2）：`--touch-calib` 给出时**以它为准**（覆盖 `EVIOCGABS` 自报值）；
/// 否则用设备自报量程。两种情况都经 [`Calibration::new`] 校验 —— 非法即报错，
/// **不静默用默认值**。
pub fn build_calibration(
    device_bounds: CalibBounds,
    ov: &TouchOverrides,
    screen_w: u32,
    screen_h: u32,
) -> Result<Calibration, TouchError> {
    let bounds = ov.calib.unwrap_or(device_bounds);
    Calibration::new(
        bounds,
        ov.swap_xy,
        ov.invert_x,
        ov.invert_y,
        screen_w,
        screen_h,
    )
}

/// 候选选择（**纯逻辑**，设计 §1.2「设备发现」行）：
///
/// - 0 个候选 → [`TouchError::NoDevice`]（**可降级**：EDGE-13，只读展示照常）；
/// - 1 个候选 → 选中；
/// - ≥2 个候选 → [`TouchError::Ambiguous`]（**启动报错并列出全部候选**，不猜 —— 避免在柜面选错设备）。
///
/// ⚠️ 传入的候选**必须**已经过能力过滤（[`candidate_from_caps`]）—— 否则加速计一类
/// 「有绝对轴但不是触摸屏」的设备会在此制造假 `Ambiguous`（Important 5）。
pub fn select_index(candidates: &[Candidate]) -> Result<usize, TouchError> {
    match candidates.len() {
        0 => Err(TouchError::NoDevice),
        1 => Ok(0),
        _ => Err(TouchError::Ambiguous(candidates.to_vec())),
    }
}

/// 触摸栈错误（设计 §1.2/§5.3）。[`TouchError::is_fatal`] 区分「启动即报错」与「降级运行」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TouchError {
    /// 未发现任何触摸设备候选（**可降级**：EDGE-13「触摸失效不影响数据刷新」）。
    NoDevice,
    /// 发现多个候选：**启动报错并列出**（设计 §1.2，不猜）。
    Ambiguous(Vec<Candidate>),
    /// 显式指定/发现的设备打不开（**可降级**：EDGE-13）。
    Unavailable {
        /// 设备路径。
        path: String,
        /// 失败原因（errno 文本）。
        reason: String,
    },
    /// 设备存在但不是可用触摸设备（缺 `ABS_X/Y` 与 `ABS_MT_POSITION_X/Y`）——**可降级**。
    NotATouchDevice {
        /// 设备路径。
        path: String,
        /// 缺什么能力。
        reason: String,
    },
    /// 量程不可信（`max<=min` / `EVIOCGABS` 失败 / 屏幕尺寸为 0）——**启动报错，不静默取默认值**。
    InvalidCalibration(String),
    /// 读事件失败（设备被拔出等）——**可降级**。
    ///
    /// ⚠️ **只**用于真错误：非阻塞 fd 的「当前无事件」（`EWOULDBLOCK`/`EAGAIN`）**不得**
    /// 归到此项（Critical 1；判定见 [`classify_fetch_error`]）。
    Io(String),
}

impl TouchError {
    /// 是否**启动即报错退出**（设计 §1.2 明确要求的两类：多候选、校准不可信）。
    ///
    /// `false` 的一律按 EDGE-13 **降级运行**：打印 `warn` + 屏幕上角标「触摸不可用」，
    /// 只读展示与数据刷新不受影响（PRD §4.3 / EDGE-13）。
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            TouchError::Ambiguous(_) | TouchError::InvalidCalibration(_)
        )
    }

    /// 降级提示文案（EDGE-13 的屏幕角标/日志用）。
    pub fn degrade_hint(&self) -> &'static str {
        "触摸不可用（数据刷新不受影响）：请检查 --touch-device / 权限（input 组）/ udev 符号链接"
    }
}

impl std::fmt::Display for TouchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TouchError::NoDevice => write!(f, "未发现触摸设备候选（已遍历 /dev/input/event*）"),
            TouchError::Ambiguous(list) => {
                write!(
                    f,
                    "发现 {} 个触摸设备候选，无法确定使用哪一个（不猜）——请用 --touch-device 显式指定；候选：",
                    list.len()
                )?;
                for (i, c) in list.iter().enumerate() {
                    if i > 0 {
                        write!(f, "；")?;
                    }
                    write!(f, "{}", c.describe())?;
                }
                Ok(())
            }
            TouchError::Unavailable { path, reason } => {
                write!(f, "触摸设备 `{path}` 不可用：{reason}")
            }
            TouchError::NotATouchDevice { path, reason } => {
                write!(f, "设备 `{path}` 不是可用触摸设备：{reason}")
            }
            TouchError::InvalidCalibration(why) => write!(f, "触摸校准不可用：{why}"),
            TouchError::Io(why) => write!(f, "读取触摸事件失败：{why}"),
        }
    }
}

impl std::error::Error for TouchError {}

// ---------------------------------------------------------------------------
// 平台无关：读事件错误归类 + 原始状态机（重评整改；Windows 本机可测）
// ---------------------------------------------------------------------------

/// `fetch_events()` 失败的归类（**纯逻辑**，跨平台可测）—— 重评 Critical 1 的落点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchErrorKind {
    /// **无事件**（正常的空闲拍）：非阻塞 fd 的 `EWOULDBLOCK`/`EAGAIN`
    /// （`ErrorKind::WouldBlock`），或读被信号打断（`ErrorKind::Interrupted`）。
    /// 后者的数据仍留在内核缓冲 ⇒ 下一拍重试不丢事件。
    NoEvents,
    /// **真错误**（`EIO` / `ENODEV` …）：设备拔出/损坏，按 EDGE-13 降级。
    Failed,
}

/// 由 [`std::io::Error`] 判定「无事件」还是「真错误」（**纯逻辑**）。
///
/// # 为什么 `WouldBlock` 是对的判据（已核实 0.13.2 源码）
///
/// `evdev::Device::fetch_events` → `RawStream::fill_events` 只做**一次** `read(2)` 并经
/// `nix::errno::Errno::result(res)?` 直传 errno；nix 0.26.4 的
/// `impl From<Errno> for io::Error` 走 `io::Error::from_raw_os_error(err as i32)`，
/// 而 Rust std 在 Unix 上把 `EAGAIN`/`EWOULDBLOCK` 归一化为
/// [`std::io::ErrorKind::WouldBlock`]。因此本判定在真机上等价于 `errno == EAGAIN`。
pub fn classify_fetch_error(err: &std::io::Error) -> FetchErrorKind {
    match err.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted => FetchErrorKind::NoEvents,
        _ => FetchErrorKind::Failed,
    }
}

/// 本项目关心的绝对坐标轴（[`RawEvent`] 的轴码；平台无关，替代 `evdev::AbsoluteAxisCode`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsAxis {
    /// `ABS_MT_SLOT`。
    MtSlot,
    /// `ABS_MT_TRACKING_ID`（`>= 0` 表示该 slot 有触点；`-1` 为抬起）。
    MtTrackingId,
    /// `ABS_MT_POSITION_X`。
    MtPositionX,
    /// `ABS_MT_POSITION_Y`。
    MtPositionY,
    /// `ABS_X`（单点）。
    X,
    /// `ABS_Y`（单点）。
    Y,
}

/// 平台无关的 evdev 事件（Linux 侧由 `evdev::InputEvent` 降维而来）。
///
/// 存在的理由：状态机是触摸栈里**最易错**的一段（slot 跟踪 / `TRACKING_ID == -1` 抬起 /
/// `SYN_DROPPED`），把它从 `cfg(linux)` 里解放出来，才能在**本机 Windows** 上覆盖（Minor 4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawEvent {
    /// 绝对坐标轴事件（`EV_ABS`）。
    Abs(AbsAxis, i32),
    /// 触摸按下键（`BTN_TOUCH`）的真/假（仅单点模型使用）。
    Touch(bool),
    /// `SYN_REPORT`：一帧结束 ⇒ **原子提交**。
    SynReport,
    /// `SYN_DROPPED`：内核缓冲溢出 ⇒ 状态不可信，清空重来（不猜）。
    SynDropped,
    /// 其它事件（忽略）。
    Other,
}

/// 协议 A/B 的原始状态机（`SYN_REPORT` 时原子提交到 [`crate::lvgl::indev::TouchSnapshot`]）。
///
/// 单点语义（设计 §1.2「单点」行）：多点设备只取**首个活动 slot**
/// （`ABS_MT_TRACKING_ID >= 0` 的最小 slot 号）；单点设备用 `ABS_X/Y` + `BTN_TOUCH`。
///
/// **平台无关**（Minor 4 整改）：Linux 侧只需把 `evdev::InputEvent` 降维成 [`RawEvent`]。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RawState {
    /// 已提交快照（`read_cb` 读它）。
    snap: crate::lvgl::indev::TouchSnapshot,
    /// 当前 MT slot（`ABS_MT_SLOT`）。
    cur_slot: u16,
    /// 各 slot 的 `(tracking_id, raw_x, raw_y)`；`tracking_id >= 0` 为活动。
    slots: std::collections::BTreeMap<u16, (i32, i32, i32)>,
    /// 单点模型：帧内待提交的原始坐标 / 按下态。
    pend_x: Option<i32>,
    pend_y: Option<i32>,
    pend_pressed: Option<bool>,
}

impl RawState {
    /// 当前快照（`Host::pump` 成功后喂给 `Indev::feed`）。
    pub fn snapshot(&self) -> crate::lvgl::indev::TouchSnapshot {
        self.snap
    }

    /// 活动 slot 数（诊断/测试用）。
    pub fn active_slots(&self) -> usize {
        self.slots.iter().filter(|(_, (tid, _, _))| *tid >= 0).count()
    }

    /// 把帧内累计的原始状态换算并提交（`SYN_REPORT`）。返回快照是否有变化。
    ///
    /// `source` / `calib` 由调用方传入（本结构只持有原始状态，换算参数归设备所有）。
    pub fn commit(&mut self, source: TouchSource, calib: &Calibration) -> bool {
        let before = self.snap;
        match source {
            TouchSource::MultiTouch => {
                // 首个活动 slot（slot 号最小者，满足「多点只取第一个触点」）。
                if let Some((_, (_, rx, ry))) =
                    self.slots.iter().find(|(_, (tid, _, _))| *tid >= 0)
                {
                    let (x, y) = calib.map(*rx, *ry);
                    self.snap.pressed = true;
                    self.snap.x = x;
                    self.snap.y = y;
                } else {
                    // 全部抬手：保留最后坐标（避免抬起瞬间跳回 (0,0)）。
                    self.snap.pressed = false;
                }
                self.slots.retain(|_, (tid, _, _)| *tid >= 0);
            }
            TouchSource::Single => {
                let (rx, ry) = (
                    self.pend_x.unwrap_or(self.snap.x),
                    self.pend_y.unwrap_or(self.snap.y),
                );
                if let Some(p) = self.pend_pressed {
                    self.snap.pressed = p;
                }
                if self.snap.pressed {
                    let (x, y) = calib.map(rx, ry);
                    self.snap.x = x;
                    self.snap.y = y;
                }
            }
        }
        self.snap != before
    }

    /// 把一条事件并入状态机。返回是否需要提交（即该事件是 `SYN_REPORT`）。
    pub fn feed(&mut self, ev: RawEvent, source: TouchSource) -> bool {
        match ev {
            RawEvent::Abs(axis, v) => {
                match (source, axis) {
                    (TouchSource::MultiTouch, AbsAxis::MtSlot) => self.cur_slot = v.max(0) as u16,
                    (TouchSource::MultiTouch, AbsAxis::MtTrackingId) => {
                        if v >= 0 {
                            let e = self.slots.entry(self.cur_slot).or_insert((v, 0, 0));
                            e.0 = v;
                        } else {
                            self.slots.remove(&self.cur_slot);
                        }
                    }
                    (TouchSource::MultiTouch, AbsAxis::MtPositionX) => {
                        // 未收到 TRACKING_ID 前视为非活动（tid = -1），避免把
                        // 「只报了坐标」的 slot 误判为按下。
                        self.slots.entry(self.cur_slot).or_insert((-1, 0, 0)).1 = v;
                    }
                    (TouchSource::MultiTouch, AbsAxis::MtPositionY) => {
                        self.slots.entry(self.cur_slot).or_insert((-1, 0, 0)).2 = v;
                    }
                    (TouchSource::Single, AbsAxis::X) => self.pend_x = Some(v),
                    (TouchSource::Single, AbsAxis::Y) => self.pend_y = Some(v),
                    _ => {}
                }
                false
            }
            RawEvent::Touch(pressed) => {
                if source == TouchSource::Single {
                    self.pend_pressed = Some(pressed);
                }
                false
            }
            RawEvent::SynDropped => {
                // 内核缓冲溢出：设备状态不可信 → 清空并等待重新上报（不猜）。
                self.slots.clear();
                self.pend_x = None;
                self.pend_y = None;
                self.pend_pressed = None;
                self.snap.pressed = false;
                false
            }
            RawEvent::SynReport => true,
            RawEvent::Other => false,
        }
    }
}

/// 读一批事件并推进状态机（**平台无关的控制流**；`read` 注入真 `fetch_events` 或假读器）。
///
/// # Critical 1（唯一修法：`WouldBlock` **不是错误**）
///
/// `read` 返回 `Err` 时按 [`classify_fetch_error`] 归类：
/// - [`FetchErrorKind::NoEvents`]（`EWOULDBLOCK`/`EAGAIN`/`EINTR`）⇒ **`Ok(false)`**，
///   「本拍无事件」—— 这正是事件循环空闲拍的正常路径（[`crate::timing::Host::pump`] 的契约）；
/// - [`FetchErrorKind::Failed`] ⇒ `Err(`[`TouchError::Io`]`)`，由调用方按 EDGE-13 降级。
///
/// 返回 `Ok(changed)` = 本批事件是否**恰好提交**（`SYN_REPORT`）且快照有变化。
pub fn pump_with<I, E, R>(
    read: R,
    st: &mut RawState,
    source: TouchSource,
    calib: &Calibration,
) -> Result<bool, TouchError>
where
    R: FnOnce() -> std::io::Result<I>,
    I: IntoIterator<Item = E>,
    E: Into<RawEvent>,
{
    let events = match read() {
        Ok(evs) => evs,
        Err(e) => {
            return match classify_fetch_error(&e) {
                // 空闲拍：无事件，**绝不**报错（否则每 500 ms 一条错误 ⇒ 触摸被误判为死）。
                FetchErrorKind::NoEvents => Ok(false),
                FetchErrorKind::Failed => Err(TouchError::Io(e.to_string())),
            };
        }
    };
    let mut pending_commit = false;
    for ev in events {
        if st.feed(ev.into(), source) {
            pending_commit = true;
        }
    }
    if pending_commit {
        Ok(st.commit(source, calib))
    } else {
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// Linux：evdev 设备发现 / 读取 / 提交（Windows 本机不编译该块）
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
pub use linux::TouchDevice;

#[cfg(target_os = "linux")]
mod linux {
    use std::os::fd::RawFd;
    use std::path::{Path, PathBuf};

    use evdev::{
        AbsoluteAxisCode, Device, EventSummary, EventType, InputEvent, KeyCode, PropType,
        SynchronizationCode,
    };

    use super::{
        build_calibration, candidate_from_caps, detect_source_lenient, CalibBounds, Candidate,
        DeviceCaps, RawEvent, RawState, TouchConfig, TouchError, TouchSource,
    };
    use crate::lvgl::indev::TouchSnapshot;

    /// 一个已打开、已校准的触摸设备（设计 §5.3）。
    ///
    /// 含设备句柄 ⇒ **不提供跨线程 API**（设计 §5.2 不变量 4）：只在事件循环线程内使用。
    pub struct TouchDevice {
        dev: Device,
        path: PathBuf,
        source: TouchSource,
        calib: super::Calibration,
        st: RawState,
    }

    impl TouchDevice {
        /// 按配置打开触摸设备（显式指定优先，否则自动发现）。
        ///
        /// 失败语义见 [`TouchError::is_fatal`]：**多候选**与**校准不可信**为致命
        /// （设计 §1.2 要求启动报错）；其余（无设备 / 打不开 / 不是触摸设备）可降级，
        /// 由调用方按 EDGE-13 打印 `warn` 后继续只读展示。
        pub fn open(cfg: &TouchConfig) -> Result<Self, TouchError> {
            match &cfg.device {
                Some(p) => {
                    let dev = Device::open(p).map_err(|e| TouchError::Unavailable {
                        path: p.display().to_string(),
                        reason: e.to_string(),
                    })?;
                    Self::from_device(dev, p.clone(), None, cfg)
                }
                None => {
                    // 自动发现：**必须**走严格能力判定（[`candidate_from_caps`]），
                    // 否则 RK3588 板载加速度计会与触摸屏并列 ⇒ Ambiguous ⇒ 启动即拒绝。
                    let mut found: Vec<(Candidate, Device)> = Vec::new();
                    for (path, dev) in evdev::enumerate() {
                        if let Some(c) = candidate_of(&path, &dev) {
                            found.push((c, dev));
                        }
                    }
                    let cands: Vec<Candidate> = found.iter().map(|(c, _)| c.clone()).collect();
                    let idx = select_index(&cands)?;
                    let (c, dev) = found.swap_remove(idx);
                    Self::from_device(dev, c.path, Some(c.source), cfg)
                }
            }
        }

        /// 由已打开设备 + 可选已知来源构造（显式指定时现场再判一次能力）。
        ///
        /// 显式指定走**宽松**判定（[`detect_source_lenient`]）：用户已点名设备节点，
        /// 不因缺少 `INPUT_PROP_DIRECT`/`BTN_TOUCH` 而拒绝一台本来能用的面板
        /// （真机适配的逃生门，见 [`detect_source_lenient`] 文档）。
        fn from_device(
            dev: Device,
            path: PathBuf,
            source: Option<TouchSource>,
            cfg: &TouchConfig,
        ) -> Result<Self, TouchError> {
            let src = match source {
                Some(s) => s,
                None => {
                    let caps = caps_of(&dev);
                    detect_source_lenient(caps).ok_or_else(|| TouchError::NotATouchDevice {
                        path: path.display().to_string(),
                        reason: "缺少 ABS_MT_POSITION_X/Y 与 ABS_X/Y（EV_ABS 绝对坐标）".into(),
                    })?
                }
            };
            // 量程：MT 设备取 MT 轴，单点取 ABS_X/Y（设计 §1.2 校准行）。
            let (xc, yc) = axis_codes(src);
            let (x_min, x_max) = abs_range(&dev, xc)?;
            let (y_min, y_max) = abs_range(&dev, yc)?;
            let device_bounds = CalibBounds {
                x_min,
                x_max,
                y_min,
                y_max,
            };
            let calib = build_calibration(device_bounds, &cfg.overrides, cfg.width, cfg.height)?;
            // 非阻塞读取：事件循环在 `poll` 判定可读后才 `pump`（设计 §5.2 不变量 1）。
            dev.set_nonblocking(true).map_err(|e| TouchError::Io(e.to_string()))?;
            Ok(Self {
                dev,
                path,
                source: src,
                calib,
                st: RawState::default(),
            })
        }

        /// 设备节点路径。
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// 坐标来源（日志用）。
        pub fn source(&self) -> TouchSource {
            self.source
        }

        /// 校准表（日志/诊断用）。
        pub fn calibration(&self) -> &super::Calibration {
            &self.calib
        }

        /// 底层 fd —— 事件循环把它交给 `poll`（设计 §5.2 骨架 `poll(&touch.fd, timeout)`）。
        pub fn fd(&self) -> RawFd {
            use std::os::fd::AsRawFd;
            self.dev.as_raw_fd()
        }

        /// 读出当前可用的全部事件并并入状态机；返回**快照是否变化**（供上层决定是否
        /// 调 `lv_indev_read`，避免无谓投递）。
        ///
        /// 非阻塞（`open` 时已 `set_nonblocking(true)`）⇒ **不会**在事件循环内阻塞
        /// （设计 §5.2 不变量 1/3）；fd 当前无事件时返回 `Ok(false)` —— **不是错误**
        /// （Critical 1，判定见 [`super::classify_fetch_error`]）。
        pub fn pump(&mut self) -> Result<bool, TouchError> {
            let Self {
                dev,
                st,
                source,
                calib,
                ..
            } = self;
            super::pump_with(|| dev.fetch_events(), st, *source, calib)
        }

        /// 当前触摸快照（喂给 `Indev::feed`）。
        pub fn snapshot(&self) -> TouchSnapshot {
            self.st.snapshot()
        }
    }

    /// 由 `evdev::Device` 抽取能力摘要（`cfg(linux)` 内唯一需要真实设备的一步，纯读取）。
    pub(crate) fn caps_of(dev: &Device) -> DeviceCaps {
        let axes = dev.supported_absolute_axes();
        let keys = dev.supported_keys();
        let props = dev.properties();
        let has = |a: AbsoluteAxisCode| axes.map_or(false, |s| s.contains(a));
        DeviceCaps {
            has_abs: dev.supported_events().contains(EventType::ABSOLUTE),
            has_mt_xy: has(AbsoluteAxisCode::ABS_MT_POSITION_X)
                && has(AbsoluteAxisCode::ABS_MT_POSITION_Y),
            has_abs_xy: has(AbsoluteAxisCode::ABS_X) && has(AbsoluteAxisCode::ABS_Y),
            has_btn_touch: keys.map_or(false, |k| k.contains(KeyCode::BTN_TOUCH)),
            direct: props.contains(PropType::DIRECT),
            accelerometer: props.contains(PropType::ACCELEROMETER),
        }
    }

    /// 某来源对应的坐标轴码。
    fn axis_codes(s: TouchSource) -> (AbsoluteAxisCode, AbsoluteAxisCode) {
        match s {
            TouchSource::MultiTouch => (
                AbsoluteAxisCode::ABS_MT_POSITION_X,
                AbsoluteAxisCode::ABS_MT_POSITION_Y,
            ),
            TouchSource::Single => (AbsoluteAxisCode::ABS_X, AbsoluteAxisCode::ABS_Y),
        }
    }

    /// 候选（纯数据）—— 发现阶段的过滤：能力像触摸屏（[`super::candidate_from_caps`]）。
    pub(crate) fn candidate_of(path: &Path, dev: &Device) -> Option<Candidate> {
        candidate_from_caps(path, dev.name(), caps_of(dev))
    }

    /// `EVIOCGABS` 取某轴的 min/max（经 evdev 的 `get_absinfo`）——失败即
    /// [`TouchError::InvalidCalibration`]（设计 §1.2：「ioctl 失败 → 启动报错」）。
    fn abs_range(dev: &Device, code: AbsoluteAxisCode) -> Result<(i32, i32), TouchError> {
        let it = dev.get_absinfo().map_err(|e| {
            TouchError::InvalidCalibration(format!(
                "EVIOCGABS 失败（无法读取坐标轴量程，拒绝用默认值）：{e}"
            ))
        })?;
        for (c, info) in it {
            if c == code {
                return Ok((info.minimum(), info.maximum()));
            }
        }
        Err(TouchError::InvalidCalibration(format!(
            "设备未上报所需坐标轴（code 0x{:x}）——无法校准",
            code.0
        )))
    }

    /// `evdev::InputEvent` → 平台无关 [`RawEvent`]（唯一的降维点；本机不可测）。
    impl From<InputEvent> for RawEvent {
        fn from(ev: InputEvent) -> Self {
            match ev.destructure() {
                EventSummary::AbsoluteAxis(_, axis, v) => {
                    let a = match axis {
                        AbsoluteAxisCode::ABS_MT_SLOT => super::AbsAxis::MtSlot,
                        AbsoluteAxisCode::ABS_MT_TRACKING_ID => super::AbsAxis::MtTrackingId,
                        AbsoluteAxisCode::ABS_MT_POSITION_X => super::AbsAxis::MtPositionX,
                        AbsoluteAxisCode::ABS_MT_POSITION_Y => super::AbsAxis::MtPositionY,
                        AbsoluteAxisCode::ABS_X => super::AbsAxis::X,
                        AbsoluteAxisCode::ABS_Y => super::AbsAxis::Y,
                        _ => return RawEvent::Other,
                    };
                    RawEvent::Abs(a, v)
                }
                EventSummary::Key(_, code, v) => {
                    if code == KeyCode::BTN_TOUCH {
                        RawEvent::Touch(v != 0)
                    } else {
                        RawEvent::Other
                    }
                }
                EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                    RawEvent::SynReport
                }
                EventSummary::Synchronization(_, SynchronizationCode::SYN_DROPPED, _) => {
                    RawEvent::SynDropped
                }
                _ => RawEvent::Other,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lvgl::indev::TouchSnapshot;
    use std::io::{Error as IoError, ErrorKind};

    fn cand(path: &str, name: Option<&str>, source: TouchSource) -> Candidate {
        Candidate {
            path: PathBuf::from(path),
            name: name.map(|s| s.to_string()),
            source,
        }
    }

    fn calib(swap: bool, ix: bool, iy: bool) -> Calibration {
        Calibration::new(
            CalibBounds {
                x_min: 0,
                x_max: 100,
                y_min: 0,
                y_max: 100,
            },
            swap,
            ix,
            iy,
            1024,
            768,
        )
        .unwrap()
    }

    // ---- ① 校准映射 ----

    #[test]
    fn calibration_maps_corners_to_closed_pixel_range() {
        let c = calib(false, false, false);
        assert_eq!(c.map(0, 0), (0, 0));
        // 闭区间：raw max ⇒ 最后一行/列（不是 width/height —— 越界一像素）
        assert_eq!(c.map(100, 100), (1023, 767));
        assert_eq!(c.map(25, 50), (256, 384)); // 0.25*1023=255.75→256；0.5*767=383.5→384
    }

    #[test]
    fn calibration_clamps_out_of_range_raw() {
        let c = calib(false, false, false);
        assert_eq!(c.map(-50, -1), (0, 0));
        assert_eq!(c.map(10_000, 999), (1023, 767));
    }

    #[test]
    fn calibration_swap_and_invert_axes() {
        let c = calib(true, false, false);
        // 交换后：raw X 归一值成为屏幕 Y 分数
        assert_eq!(c.map(100, 0), (0, 767));
        assert_eq!(c.map(0, 100), (1023, 0));

        let c = calib(false, true, false);
        assert_eq!(c.map(0, 0), (1023, 0));
        assert_eq!(c.map(100, 100), (0, 767));

        let c = calib(false, false, true);
        assert_eq!(c.map(0, 0), (0, 767));
        assert_eq!(c.map(100, 100), (1023, 0));

        // 交换 + 双向翻转：raw(0,0) → 归一(0,0) → 交换(0,0) → 双翻(1,1)
        let c = calib(true, true, true);
        assert_eq!(c.map(0, 0), (1023, 767));
    }

    #[test]
    fn calibration_rejects_degenerate_bounds_and_zero_screen() {
        let bad = CalibBounds {
            x_min: 100,
            x_max: 100,
            y_min: 0,
            y_max: 100,
        };
        let e = Calibration::new(bad, false, false, false, 1024, 768).unwrap_err();
        assert!(
            matches!(&e, TouchError::InvalidCalibration(m) if m.contains("X 轴")),
            "max==min 应报错且指明 X 轴，实得 {e:?}"
        );
        assert!(e.is_fatal(), "校准不可信 = 启动即报错（不静默用默认值）");

        let bad = CalibBounds {
            x_min: 0,
            x_max: 100,
            y_min: 50,
            y_max: 10,
        };
        let e = Calibration::new(bad, false, false, false, 1024, 768).unwrap_err();
        assert!(matches!(&e, TouchError::InvalidCalibration(m) if m.contains("Y 轴")));

        let ok = CalibBounds {
            x_min: 0,
            x_max: 100,
            y_min: 0,
            y_max: 100,
        };
        let e = Calibration::new(ok, false, false, false, 0, 768).unwrap_err();
        assert!(e.is_fatal());
        assert!(Calibration::new(ok, false, false, false, 1024, 768).is_ok());
    }

    #[test]
    fn calibration_handles_negative_and_offset_raw_range() {
        // 真机常见：raw 量程为负数（如 -2048..2047）
        let c = Calibration::new(
            CalibBounds {
                x_min: -2048,
                x_max: 2047,
                y_min: -100,
                y_max: 900,
            },
            false,
            false,
            false,
            1024,
            768,
        )
        .unwrap();
        assert_eq!(c.map(-2048, -100), (0, 0));
        assert_eq!(c.map(2047, 900), (1023, 767));
    }

    #[test]
    fn calibration_single_pixel_screen_is_safe() {
        let c = Calibration::new(
            CalibBounds {
                x_min: 0,
                x_max: 10,
                y_min: 0,
                y_max: 10,
            },
            false,
            false,
            false,
            1,
            1,
        )
        .unwrap();
        assert_eq!(c.map(5, 5), (0, 0));
    }

    // ---- 校准量程的来源合并 ----

    #[test]
    fn cli_calib_overrides_device_reported_bounds() {
        let dev = CalibBounds {
            x_min: 0,
            x_max: 100,
            y_min: 0,
            y_max: 100,
        };
        let ov = TouchOverrides {
            calib: Some(CalibBounds {
                x_min: 0,
                x_max: 200,
                y_min: 0,
                y_max: 400,
            }),
            ..Default::default()
        };
        let c = build_calibration(dev, &ov, 1024, 768).unwrap();
        assert_eq!(c.bounds.x_max, 200, "CLI 覆盖须以 CLI 为准");
        assert_eq!(c.bounds.y_max, 400);
        assert_eq!(c.map(200, 400), (1023, 767));
    }

    #[test]
    fn device_bounds_used_when_no_cli_override() {
        let dev = CalibBounds {
            x_min: 10,
            x_max: 20,
            y_min: 30,
            y_max: 40,
        };
        let c = build_calibration(dev, &TouchOverrides::default(), 100, 100).unwrap();
        assert_eq!(c.bounds, dev);
        // 归一值 0.5 ⇒ 0.5 × (100-1) = 49.5 ⇒ f64::round 半值远离零 ⇒ 50
        assert_eq!(c.map(15, 35), (50, 50));
    }

    #[test]
    fn invalid_cli_calib_is_fatal() {
        let ov = TouchOverrides {
            calib: Some(CalibBounds {
                x_min: 5,
                x_max: 5,
                y_min: 0,
                y_max: 10,
            }),
            ..Default::default()
        };
        let dev = CalibBounds {
            x_min: 0,
            x_max: 100,
            y_min: 0,
            y_max: 100,
        };
        let e = build_calibration(dev, &ov, 1024, 768).unwrap_err();
        assert!(e.is_fatal());
        // 错误文本必须可读且指向处置手段
        let s = e.to_string();
        assert!(s.contains("--touch-calib"), "错误应给出处置手段：{s}");
    }

    // ---- ② 设备发现的多候选语义 ----

    #[test]
    fn discovery_requires_explicit_choice_when_multiple_candidates() {
        let list = vec![
            cand("/dev/input/event2", Some("Goodix TouchScreen"), TouchSource::MultiTouch),
            cand("/dev/input/event5", Some("USB Touchscreen"), TouchSource::Single),
        ];
        let e = select_index(&list).unwrap_err();
        assert!(e.is_fatal(), "多候选 = 启动报错并列出（不猜第一个）");
        let s = e.to_string();
        assert!(s.contains("/dev/input/event2"), "须列出候选路径：{s}");
        assert!(s.contains("Goodix TouchScreen"), "须列出候选设备名：{s}");
        assert!(s.contains("/dev/input/event5"));
        assert!(s.contains("--touch-device"), "须给出处置手段：{s}");
        match &e {
            TouchError::Ambiguous(c) => assert_eq!(c.len(), 2),
            other => panic!("应为 Ambiguous，实得 {other:?}"),
        }
    }

    #[test]
    fn discovery_selects_single_candidate_and_reports_none() {
        let one = vec![cand("/dev/mupc-touch", Some("MupcTouch"), TouchSource::MultiTouch)];
        assert_eq!(select_index(&one).unwrap(), 0);

        let e = select_index(&[]).unwrap_err();
        assert!(!e.is_fatal(), "无设备可降级运行（EDGE-13）");
        assert!(e.to_string().contains("未发现"));
        assert!(!e.degrade_hint().is_empty());
    }

    /// 致命/可降级分类（设计 §1.2 + EDGE-13 的落地口径）。
    #[test]
    fn error_fatality_classification() {
        assert!(TouchError::Ambiguous(vec![]).is_fatal());
        assert!(TouchError::InvalidCalibration("x".into()).is_fatal());
        for e in [
            TouchError::NoDevice,
            TouchError::Unavailable {
                path: "/dev/mupc-touch".into(),
                reason: "ENOENT".into(),
            },
            TouchError::NotATouchDevice {
                path: "/dev/input/event0".into(),
                reason: "缺 EV_ABS".into(),
            },
            TouchError::Io("EIO".into()),
        ] {
            assert!(!e.is_fatal(), "{e:?} 应可降级（触摸失效不影响数据刷新）");
            assert!(!e.to_string().is_empty());
        }
    }

    // ---- ②-b 能力判定与真机「触摸屏 + 加速度计并列」（Important 5 整改） ----

    /// 真触摸屏（MT 协议 B + `INPUT_PROP_DIRECT`）。
    const CAPS_TOUCH_MT: DeviceCaps = DeviceCaps {
        has_abs: true,
        has_mt_xy: true,
        has_abs_xy: true,
        has_btn_touch: true,
        direct: true,
        accelerometer: false,
    };

    /// RK3588 板载加速度计：`EV_ABS` + `ABS_X/Y`，**无** `BTN_TOUCH`、**无** `DIRECT`，
    /// 且内核置了 `INPUT_PROP_ACCELEROMETER`。
    const CAPS_ACCELEROMETER: DeviceCaps = DeviceCaps {
        has_abs: true,
        has_mt_xy: false,
        has_abs_xy: true,
        has_btn_touch: false,
        direct: false,
        accelerometer: true,
    };

    #[test]
    fn accelerometer_is_not_a_touch_candidate() {
        assert_eq!(detect_source_from_caps(CAPS_ACCELEROMETER), None);
        assert_eq!(candidate_from_caps("/dev/input/event0", Some("accel"), CAPS_ACCELEROMETER), None);
        // 即便它「碰巧」带 BTN_TOUCH，`ACCELEROMETER` 仍一票否决
        let weird = DeviceCaps {
            has_btn_touch: true,
            ..CAPS_ACCELEROMETER
        };
        assert_eq!(detect_source_from_caps(weird), None);
    }

    /// **Critical/Important 5 回归**：触摸屏与加速度计并列 ⇒ 唯一选中触摸屏（**不报 Ambiguous**）。
    /// 旧实现（只看「有绝对轴」）会把两者都收进候选 ⇒ `select_index` 报 `Ambiguous` ⇒
    /// `is_fatal` ⇒ **真机启动即拒绝**。
    #[test]
    fn touchscreen_plus_accelerometer_selects_the_touchscreen_uniquely() {
        let enumerations: [(&str, Option<&str>, DeviceCaps); 2] = [
            ("/dev/input/event1", Some("Goodix TouchScreen"), CAPS_TOUCH_MT),
            ("/dev/input/event0", Some("rk3588-accel"), CAPS_ACCELEROMETER),
        ];
        let cands: Vec<Candidate> = enumerations
            .iter()
            .filter_map(|(p, n, c)| candidate_from_caps(*p, *n, *c))
            .collect();
        assert_eq!(cands.len(), 1, "加速度计不得进入候选：{cands:?}");
        assert_eq!(cands[0].path, PathBuf::from("/dev/input/event1"));
        assert_eq!(select_index(&cands).unwrap(), 0);
        assert_eq!(cands[0].source, TouchSource::MultiTouch);

        // 反证：若按旧口径（只看坐标轴）两者都会入选 ⇒ 假 Ambiguous（这正是被修的 bug）。
        let legacy: Vec<Candidate> = enumerations
            .iter()
            .filter_map(|(p, n, c)| {
                detect_source_lenient(*c).map(|s| cand(p, *n, s))
            })
            .collect();
        assert_eq!(legacy.len(), 2);
        let e = select_index(&legacy).unwrap_err();
        assert!(e.is_fatal(), "旧口径会让真机启动即拒绝（本测试的反证基线）");
    }

    /// 两台**真**触摸屏并列 ⇒ 仍报 `Ambiguous`（不得退化为静默取第一个）。
    #[test]
    fn two_real_touchscreens_still_report_ambiguous() {
        let cands: Vec<Candidate> = [
            ("/dev/input/event2", Some("Goodix TouchScreen"), CAPS_TOUCH_MT),
            (
                "/dev/input/event5",
                Some("USB Touchscreen"),
                DeviceCaps {
                    has_mt_xy: false,
                    has_abs_xy: true,
                    has_btn_touch: true,
                    direct: true,
                    ..CAPS_TOUCH_MT
                },
            ),
        ]
        .iter()
        .filter_map(|(p, n, c)| candidate_from_caps(*p, *n, *c))
        .collect();
        assert_eq!(cands.len(), 2);
        let e = select_index(&cands).unwrap_err();
        assert!(e.is_fatal());
        let s = e.to_string();
        assert!(s.contains("/dev/input/event2") && s.contains("/dev/input/event5"));
    }

    /// 能力判定的其余分支（MT/Single/都不是）逐条钉住。
    #[test]
    fn source_detection_requires_touch_like_signals() {
        // MT：MT 轴 + DIRECT（无 BTN_TOUCH）⇒ MultiTouch
        assert_eq!(
            detect_source_from_caps(DeviceCaps {
                has_btn_touch: false,
                ..CAPS_TOUCH_MT
            }),
            Some(TouchSource::MultiTouch)
        );
        // MT：MT 轴 + BTN_TOUCH（无 DIRECT）⇒ MultiTouch
        assert_eq!(
            detect_source_from_caps(DeviceCaps {
                direct: false,
                ..CAPS_TOUCH_MT
            }),
            Some(TouchSource::MultiTouch)
        );
        // MT 轴但两个「像触摸屏」信号都缺 ⇒ 不是（板载加速度计/摇杆）
        assert_eq!(
            detect_source_from_caps(DeviceCaps {
                has_btn_touch: false,
                direct: false,
                ..CAPS_TOUCH_MT
            }),
            None
        );
        // 单点：ABS_X/Y + BTN_TOUCH ⇒ Single
        assert_eq!(
            detect_source_from_caps(DeviceCaps {
                has_abs: true,
                has_mt_xy: false,
                has_abs_xy: true,
                has_btn_touch: true,
                direct: false,
                accelerometer: false,
            }),
            Some(TouchSource::Single)
        );
        // 单点：有 ABS_X/Y 但无 BTN_TOUCH ⇒ 报不出按下 ⇒ 不是触摸屏
        assert_eq!(
            detect_source_from_caps(DeviceCaps {
                has_mt_xy: false,
                has_btn_touch: false,
                ..CAPS_TOUCH_MT
            }),
            None
        );
        // 无 EV_ABS ⇒ 不是
        assert_eq!(
            detect_source_from_caps(DeviceCaps {
                has_abs: false,
                ..CAPS_TOUCH_MT
            }),
            None
        );
        // 有 EV_ABS 但没有任何坐标轴 ⇒ 不是
        assert_eq!(
            detect_source_from_caps(DeviceCaps {
                has_mt_xy: false,
                has_abs_xy: false,
                ..CAPS_TOUCH_MT
            }),
            None
        );
    }

    /// 显式指定（`--touch-device`）走宽松判定：只缺「像触摸屏」信号的面板仍可启用（逃生门）。
    #[test]
    fn explicit_device_path_is_lenient_escape_hatch() {
        let bare_mt = DeviceCaps {
            has_btn_touch: false,
            direct: false,
            ..CAPS_TOUCH_MT
        };
        assert_eq!(detect_source_from_caps(bare_mt), None, "自动发现必须严格");
        assert_eq!(
            detect_source_lenient(bare_mt),
            Some(TouchSource::MultiTouch),
            "显式指定是用户明确意图 ⇒ 不得因缺信号而拒绝"
        );
        // 无 EV_ABS：宽松版也不认（连坐标都没有）
        assert_eq!(
            detect_source_lenient(DeviceCaps {
                has_abs: false,
                ..CAPS_TOUCH_MT
            }),
            None
        );
    }

    // ---- ③ `--touch-calib` 文本解析 ----

    #[test]
    fn calib_bounds_parse_accepts_valid_and_trims() {
        let b = CalibBounds::parse(" 0 , 4095 , 0 , 4095 ").unwrap();
        assert_eq!(
            b,
            CalibBounds {
                x_min: 0,
                x_max: 4095,
                y_min: 0,
                y_max: 4095
            }
        );
        let neg = CalibBounds::parse("-2048,2047,-100,900").unwrap();
        assert_eq!(neg.x_min, -2048);
        assert!(neg.same_as(&neg));
        assert!(!neg.same_as(&b));
    }

    #[test]
    fn calib_bounds_parse_rejects_bad_input() {
        for bad in ["", "0,100,0", "0,100,0,100,7", "a,100,0,100", "0,100,0,"] {
            assert!(CalibBounds::parse(bad).is_err(), "`{bad}` 应判非法");
        }
        // max<=min 在解析期即拒绝（设计 §1.2）
        let e = CalibBounds::parse("100,100,0,100").unwrap_err();
        assert!(e.contains("X 轴"), "{e}");
        let e = CalibBounds::parse("0,100,9,9").unwrap_err();
        assert!(e.contains("Y 轴"), "{e}");
    }

    #[test]
    fn touch_config_defaults_are_auto_discover() {
        let c = TouchConfig::new(1024, 768);
        assert!(c.device.is_none(), "默认自动发现；生产由 unit 固定 --touch-device");
        assert_eq!(c.overrides, TouchOverrides::default());
        assert!(!c.overrides.swap_xy);
    }

    // ---- ④ 非阻塞读的错误归类（Critical 1 整改；旧实现「空闲拍必然返回假错误」） ----

    #[test]
    fn would_block_and_eintr_are_no_events_not_errors() {
        let wb = IoError::new(ErrorKind::WouldBlock, "EAGAIN");
        assert_eq!(classify_fetch_error(&wb), FetchErrorKind::NoEvents);
        let intr = IoError::new(ErrorKind::Interrupted, "EINTR");
        assert_eq!(
            classify_fetch_error(&intr),
            FetchErrorKind::NoEvents,
            "读被信号打断：数据仍在内核缓冲 ⇒ 下一拍重试，不是设备故障"
        );
        // 真错误不得被当成空闲
        for k in [
            ErrorKind::Other,
            ErrorKind::NotFound,
            ErrorKind::PermissionDenied,
            ErrorKind::UnexpectedEof,
        ] {
            let e = IoError::new(k, "boom");
            assert_eq!(classify_fetch_error(&e), FetchErrorKind::Failed, "{k:?}");
        }
    }

    /// 真机链路的等价性（**仅 Linux**）：`EAGAIN`/`EWOULDBLOCK` 经
    /// `nix::errno::Errno::result` → `io::Error::from_raw_os_error` 后 `kind()` 必为
    /// `WouldBlock`（`libc` 只在 Linux target 依赖，故本用例无法在本机 Windows 运行）。
    #[cfg(target_os = "linux")]
    #[test]
    fn eagain_errno_maps_to_no_events_on_linux() {
        for e in [libc::EAGAIN, libc::EWOULDBLOCK] {
            let err = IoError::from_raw_os_error(e);
            assert_eq!(classify_fetch_error(&err), FetchErrorKind::NoEvents, "errno={e}");
        }
        let eio = IoError::from_raw_os_error(libc::EIO);
        assert_eq!(classify_fetch_error(&eio), FetchErrorKind::Failed);
    }

    /// **Critical 1 回归**：空闲拍（非阻塞 fd 当前无事件）**不得产生任何错误**。
    ///
    /// 旧实现：`fetch_events()` 的 `EWOULDBLOCK` 被 `?` 直传 ⇒ 每 500 ms 一条
    /// [`TouchError::Io`] ⇒ unit B 据此降级会把触摸永久判死 + 2 行/s 日志洪泛。
    #[test]
    fn idle_ticks_never_produce_errors() {
        let mut st = RawState::default();
        let c = calib(false, false, false);
        // 连续 5 个空闲拍（≈2.5 s），每拍都必须 Ok(false) —— 不 Err、不算「快照变化」。
        for tick in 0..5 {
            let r = pump_with(
                || -> std::io::Result<Vec<RawEvent>> {
                    Err(IoError::new(ErrorKind::WouldBlock, "Resource temporarily unavailable"))
                },
                &mut st,
                TouchSource::MultiTouch,
                &c,
            );
            assert_eq!(r, Ok(false), "空闲拍 #{tick} 必须是无事件 no-op，实得 {r:?}");
        }
        // 空批次（fd 可读但无完整事件）同样是 no-op
        let r: Result<bool, TouchError> =
            pump_with(|| Ok(Vec::<RawEvent>::new()), &mut st, TouchSource::MultiTouch, &c);
        assert_eq!(r, Ok(false));
        // 快照保持默认（未按下、坐标未被污染）
        assert_eq!(st.snapshot(), TouchSnapshot::default());
    }

    /// 真错误（如设备拔出 `ENODEV`）仍必须上抛，且是**可降级**错误（EDGE-13）。
    #[test]
    fn real_io_errors_still_surface_and_are_degradable() {
        let mut st = RawState::default();
        let c = calib(false, false, false);
        let r = pump_with(
            || -> std::io::Result<Vec<RawEvent>> { Err(IoError::other("ENODEV")) },
            &mut st,
            TouchSource::MultiTouch,
            &c,
        );
        let e = r.unwrap_err();
        assert!(matches!(&e, TouchError::Io(m) if m.contains("ENODEV")), "{e:?}");
        assert!(!e.is_fatal(), "读失败可降级（触摸失效不影响数据刷新）");
    }

    /// 一批事件 ⇒ 恰在 `SYN_REPORT` 提交，且快照变化被如实上报。
    #[test]
    fn events_commit_on_syn_report_only() {
        let mut st = RawState::default();
        let c = calib(false, false, false);
        let batch = vec![
            RawEvent::Abs(AbsAxis::MtSlot, 0),
            RawEvent::Abs(AbsAxis::MtTrackingId, 7),
            RawEvent::Abs(AbsAxis::MtPositionX, 100),
            RawEvent::Abs(AbsAxis::MtPositionY, 0),
        ];
        // 无 SYN_REPORT ⇒ 不提交、无变化
        assert_eq!(
            pump_with(
                || Ok(batch.clone()),
                &mut st,
                TouchSource::MultiTouch,
                &c
            ),
            Ok(false)
        );
        assert!(!st.snapshot().pressed);
        assert_eq!(st.active_slots(), 1, "slot 已被跟踪（只是尚未提交）");

        // 补上 SYN_REPORT ⇒ 提交并报「有变化」
        let mut with_syn = batch.clone();
        with_syn.push(RawEvent::SynReport);
        assert_eq!(pump_with(|| Ok(with_syn), &mut st, TouchSource::MultiTouch, &c), Ok(true));
        assert_eq!(
            st.snapshot(),
            TouchSnapshot {
                pressed: true,
                x: 1023,
                y: 0
            }
        );

        // 同一帧重复提交 ⇒ 快照无变化 ⇒ Ok(false)（不触发无谓的 lv_indev_read）
        let mut again = batch.clone();
        again.push(RawEvent::SynReport);
        assert_eq!(pump_with(|| Ok(again), &mut st, TouchSource::MultiTouch, &c), Ok(false));
    }

    // ---- ⑤ 原始状态机（Minor 4 整改：原先整段在 `cfg(linux)` 内 ⇒ 本机零测试） ----

    fn mt_frame(events: &[RawEvent], st: &mut RawState, c: &Calibration) {
        let mut v = events.to_vec();
        v.push(RawEvent::SynReport);
        st.feed_many(&v, TouchSource::MultiTouch);
        st.commit(TouchSource::MultiTouch, c);
    }

    /// 小工具：把一串事件喂进状态机（含提交）。
    impl RawState {
        fn feed_many(&mut self, evs: &[RawEvent], source: TouchSource) {
            for e in evs {
                self.feed(*e, source);
            }
        }
    }

    #[test]
    fn mt_state_machine_tracks_slots_and_takes_first_active() {
        let c = calib(false, false, false);
        let mut st = RawState::default();
        // 触点 1：slot 0
        mt_frame(
            &[
                RawEvent::Abs(AbsAxis::MtSlot, 0),
                RawEvent::Abs(AbsAxis::MtTrackingId, 100),
                RawEvent::Abs(AbsAxis::MtPositionX, 0),
                RawEvent::Abs(AbsAxis::MtPositionY, 100),
            ],
            &mut st,
            &c,
        );
        assert_eq!(st.snapshot(), TouchSnapshot { pressed: true, x: 0, y: 767 });

        // 再加触点 2（slot 1，坐标不同）⇒ 仍取**首个活动 slot**（slot 号最小）
        mt_frame(
            &[
                RawEvent::Abs(AbsAxis::MtSlot, 1),
                RawEvent::Abs(AbsAxis::MtTrackingId, 101),
                RawEvent::Abs(AbsAxis::MtPositionX, 100),
                RawEvent::Abs(AbsAxis::MtPositionY, 0),
            ],
            &mut st,
            &c,
        );
        assert_eq!(st.active_slots(), 2);
        assert_eq!(st.snapshot(), TouchSnapshot { pressed: true, x: 0, y: 767 });

        // slot 0 抬起（TRACKING_ID = -1）⇒ 回落到 slot 1
        mt_frame(
            &[
                RawEvent::Abs(AbsAxis::MtSlot, 0),
                RawEvent::Abs(AbsAxis::MtTrackingId, -1),
            ],
            &mut st,
            &c,
        );
        assert_eq!(st.active_slots(), 1);
        assert_eq!(st.snapshot(), TouchSnapshot { pressed: true, x: 1023, y: 0 });
    }

    /// 全部抬手 ⇒ `pressed=false` 但**保留最后坐标**（避免抬起瞬间跳回 (0,0)）。
    #[test]
    fn mt_release_keeps_last_coordinates() {
        let c = calib(false, false, false);
        let mut st = RawState::default();
        mt_frame(
            &[
                RawEvent::Abs(AbsAxis::MtSlot, 0),
                RawEvent::Abs(AbsAxis::MtTrackingId, 5),
                RawEvent::Abs(AbsAxis::MtPositionX, 50),
                RawEvent::Abs(AbsAxis::MtPositionY, 50),
            ],
            &mut st,
            &c,
        );
        let pressed_pos = st.snapshot();
        mt_frame(&[RawEvent::Abs(AbsAxis::MtTrackingId, -1)], &mut st, &c);
        assert_eq!(
            st.snapshot(),
            TouchSnapshot {
                pressed: false,
                ..pressed_pos
            }
        );
        assert_eq!(st.active_slots(), 0);
    }

    /// 只报坐标、未报 `TRACKING_ID` 的 slot **不得**被当成按下（tid 初值 -1）。
    #[test]
    fn mt_positions_without_tracking_id_are_not_pressed() {
        let c = calib(false, false, false);
        let mut st = RawState::default();
        mt_frame(
            &[
                RawEvent::Abs(AbsAxis::MtSlot, 2),
                RawEvent::Abs(AbsAxis::MtPositionX, 80),
                RawEvent::Abs(AbsAxis::MtPositionY, 20),
            ],
            &mut st,
            &c,
        );
        assert!(!st.snapshot().pressed);
        assert_eq!(st.active_slots(), 0);
    }

    /// `SYN_DROPPED`（内核缓冲溢出）⇒ 清空状态并置未按下（不猜），随后可重新建立。
    #[test]
    fn syn_dropped_clears_state_and_recovers() {
        let c = calib(false, false, false);
        let mut st = RawState::default();
        mt_frame(
            &[
                RawEvent::Abs(AbsAxis::MtSlot, 0),
                RawEvent::Abs(AbsAxis::MtTrackingId, 9),
                RawEvent::Abs(AbsAxis::MtPositionX, 30),
                RawEvent::Abs(AbsAxis::MtPositionY, 60),
            ],
            &mut st,
            &c,
        );
        assert!(st.snapshot().pressed);
        assert_eq!(st.active_slots(), 1);

        let dropped = RawEvent::SynDropped;
        assert!(!st.feed(dropped, TouchSource::MultiTouch), "SYN_DROPPED 不触提交");
        assert_eq!(st.active_slots(), 0, "状态不可信 ⇒ 清空 slot");
        assert!(!st.snapshot().pressed, "丢事件后必须视为未按下（不猜）");
        // 与 Linux `feed` 分支等价：单点模型的待提交量同样被清空
        st.feed(RawEvent::Abs(AbsAxis::X, 10), TouchSource::Single);
        st.feed(RawEvent::Touch(true), TouchSource::Single);
        st.feed(RawEvent::SynDropped, TouchSource::Single);
        st.feed(RawEvent::SynReport, TouchSource::Single);
        assert!(!st.snapshot().pressed, "SYN_DROPPED 已清掉 pend_pressed");

        // 重来一帧 ⇒ 恢复
        mt_frame(
            &[
                RawEvent::Abs(AbsAxis::MtSlot, 0),
                RawEvent::Abs(AbsAxis::MtTrackingId, 11),
                RawEvent::Abs(AbsAxis::MtPositionX, 0),
                RawEvent::Abs(AbsAxis::MtPositionY, 0),
            ],
            &mut st,
            &c,
        );
        assert_eq!(st.snapshot(), TouchSnapshot { pressed: true, x: 0, y: 0 });
    }

    /// 单点模型：`BTN_TOUCH` 决定按下；`ABS_X/Y` 决定坐标；抬手保留坐标。
    #[test]
    fn single_point_state_machine_uses_btn_touch() {
        let c = calib(false, false, false);
        let mut st = RawState::default();
        let frame = |st: &mut RawState, evs: &[RawEvent]| {
            st.feed_many(evs, TouchSource::Single);
            st.commit(TouchSource::Single, &c);
        };
        frame(
            &mut st,
            &[RawEvent::Abs(AbsAxis::X, 25), RawEvent::Abs(AbsAxis::Y, 75), RawEvent::Touch(true)],
        );
        // 0.25×1023 = 255.75 → 256；0.75×767 = 575.25 → 575
        assert_eq!(st.snapshot(), TouchSnapshot { pressed: true, x: 256, y: 575 });

        // 拖动（仍按下）⇒ 坐标跟随
        frame(&mut st, &[RawEvent::Abs(AbsAxis::X, 100)]);
        assert_eq!(st.snapshot(), TouchSnapshot { pressed: true, x: 1023, y: 575 });

        // 抬手 ⇒ 不再更新坐标，但保留最后位置
        frame(&mut st, &[RawEvent::Touch(false)]);
        assert_eq!(st.snapshot(), TouchSnapshot { pressed: false, x: 1023, y: 575 });

        // 抬手后继续移动 ⇒ 坐标不得被改写（未按下时不 map）
        frame(&mut st, &[RawEvent::Abs(AbsAxis::X, 0)]);
        assert_eq!(st.snapshot(), TouchSnapshot { pressed: false, x: 1023, y: 575 });

        // 单点模型**忽略** MT 轴；MT 模型忽略 BTN_TOUCH（源判别互不串台）
        st.feed(RawEvent::Abs(AbsAxis::MtTrackingId, 3), TouchSource::Single);
        st.feed(RawEvent::Touch(true), TouchSource::MultiTouch);
        st.commit(TouchSource::Single, &c);
        assert_eq!(st.active_slots(), 0);
    }

    /// `Other` 事件既不改变状态也不触发提交。
    #[test]
    fn unrelated_events_are_ignored() {
        let mut st = RawState::default();
        assert!(!st.feed(RawEvent::Other, TouchSource::MultiTouch));
        assert!(!st.feed(RawEvent::Other, TouchSource::Single));
        assert_eq!(st.snapshot(), TouchSnapshot::default());
        assert_eq!(st.active_slots(), 0);
    }
}
