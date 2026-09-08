//! 主配置文件 `mupc_core_config.yaml` 结构定义
//!
//! 定义 mupcd 守护进程的完整配置结构，包括系统参数、
//! 核间通信、Web API、AI 引擎和插件配置。

use serde::Deserialize;
use std::path::PathBuf;

/// 主配置文件顶层结构
#[derive(Debug, Clone, Deserialize)]
pub struct CoreConfig {
    /// 配置版本号（用于兼容性校验）
    pub version: String,
    /// 系统级配置
    pub system: SystemConfig,
    /// 核间通信配置
    pub intercore: InterCoreConfig,
    /// Web API 配置
    pub web_api: WebApiConfig,
    /// AI 引擎配置
    pub ai_engine: AiEngineConfig,
    /// 插件配置
    pub plugins: PluginsConfig,
    /// 台区总表分相数据源（U-26：台区储能策略 phase 输入）
    #[serde(default)]
    pub master_meter: MasterMeterConfig,
    /// 策略引擎配置（v2.24：容量档位 YAML 路径）
    #[serde(default)]
    pub strategy: StrategyConfig,
    /// 数字 IO / 安全联锁配置（S2 §12.4 io: 段；缺省 disabled，未配置 io 段部署行为不变）
    #[serde(default)]
    pub io: IoConfig,
}

/// 数字 IO / 安全联锁配置（S2 §12.4 io: 段；缺省 disabled——未配置 io 段部署行为不变）
///
/// enabled=true 时按 DI 触发源（急停/水浸/消防 → pcs_stop；门禁 → event）驱动
/// 安全联锁，DO 输出运行/故障灯。
#[derive(Debug, Clone, Deserialize)]
pub struct IoConfig {
    /// 是否启用联锁控制器（配置 io 段即启用；缺省 false）
    #[serde(default)]
    pub enabled: bool,
    /// DI 轮询周期 ms，默认 100
    #[serde(default = "default_io_poll_ms")]
    pub poll_ms: u64,
    /// 触发源回安全态后是否自动清除联锁（true 不推荐，默认 false=须人工确认）
    #[serde(default)]
    pub auto_release: bool,
    /// 触发源回安全态须保持时长 s（人工解除前置校验），默认 0
    #[serde(default)]
    pub release_hold_secs: u64,
    /// 停机确认窗口 ms（停机指令 1013 须转 0；超时 → stop_failed + 周期重试），默认 5000
    #[serde(default = "default_stop_confirm_ms")]
    pub stop_confirm_ms: u64,
    /// DI 输入表（pcs_stop：急停/水浸/消防；event：门禁）
    #[serde(default)]
    pub di: Vec<DiConf>,
    /// DO 输出表（运行/故障灯）；YAML 键为 `do`（do 为 Rust 关键字 → 字段 do_out）
    #[serde(default, rename = "do")]
    pub do_out: Vec<DoConf>,
}

impl Default for IoConfig {
    /// 缺省 disabled：enabled=false、di/do 空、poll/stop_confirm 落字段默认函数值
    fn default() -> Self {
        Self {
            enabled: false,
            poll_ms: default_io_poll_ms(),
            auto_release: false,
            release_hold_secs: 0,
            stop_confirm_ms: default_stop_confirm_ms(),
            di: Vec::new(),
            do_out: Vec::new(),
        }
    }
}

/// DI 数字输入通道配置
#[derive(Debug, Clone, Deserialize)]
pub struct DiConf {
    /// 通道名（诊断/查重定位用，须非空且表内唯一）
    #[serde(default)]
    pub name: String,
    /// GPIO 引脚号（必填，须 > 0）
    pub gpio: u32,
    /// 低电平有效（默认 true：常闭安全回路，断线/急停按下视为触发）
    #[serde(default = "default_true")]
    pub active_low: bool,
    /// 消抖次数（默认 3，须 >= 1）
    #[serde(default = "default_debounce")]
    pub debounce: u32,
    /// 触发动作：pcs_stop（急停/水浸/消防）| event（门禁）
    #[serde(default)]
    pub action: String,
}

/// DO 数字输出通道配置
#[derive(Debug, Clone, Deserialize)]
pub struct DoConf {
    /// 通道名（诊断/查重定位用，须非空且表内唯一）
    #[serde(default)]
    pub name: String,
    /// GPIO 引脚号（必填，须 > 0）
    pub gpio: u32,
    /// 高电平有效（默认 true；false = 低电平点亮）
    #[serde(default = "default_true")]
    pub active_high: bool,
}

/// 系统级配置
#[derive(Debug, Clone, Deserialize)]
pub struct SystemConfig {
    /// 日志级别: "info" / "debug" / "warn" / "error"
    #[serde(default = "default_log_level")]
    pub log_level: String,
    /// 日志输出目录
    #[serde(default = "default_log_dir")]
    pub log_dir: PathBuf,
    /// 持久化数据目录
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
    /// 插件搜索目录
    #[serde(default = "default_plugin_dir")]
    pub plugin_dir: PathBuf,
    /// TLS 证书目录
    #[serde(default = "default_cert_dir")]
    pub cert_dir: PathBuf,
    /// 优雅退出超时（秒），默认 30
    #[serde(default = "default_shutdown_timeout_sec")]
    pub shutdown_timeout_sec: u64,
}

/// 核间通信配置（与实时核心 TCP 连接）
#[derive(Debug, Clone, Deserialize)]
pub struct InterCoreConfig {
    /// 实时核心 IP 地址
    #[serde(default = "default_intercore_host")]
    pub host: String,
    /// 实时核心端口，默认 9100
    #[serde(default = "default_intercore_port")]
    pub port: u16,
    /// 心跳间隔（秒），默认 5
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_sec: u64,
    /// 重连间隔（秒），默认 3
    #[serde(default = "default_reconnect_interval")]
    pub reconnect_interval_sec: u64,
    /// 传输通道：tcp | modbus_rtu（部署二选一）
    #[serde(default = "default_intercore_transport")]
    pub transport: String,
    /// Modbus RTU 通道参数（transport=modbus_rtu 时生效）
    #[serde(default)]
    pub modbus_rtu: ModbusRtuConfig,
}

/// Modbus RTU 核间传输配置（transport=modbus_rtu 时生效）
///
/// 注意：手动实现 `Default`（不走 derive），使 `#[serde(default)]` 缺省整段
/// 配置时也落到下方默认函数，而非空/零值。
#[derive(Debug, Clone, Deserialize)]
pub struct ModbusRtuConfig {
    /// 串口设备，默认 /dev/ttyS0（BECG-3568 板载 COM1 ↔ PCS，19200 N-8-1；无 ttyS1）
    #[serde(default = "default_serial_port")]
    pub serial_port: String,
    /// 波特率，默认 19200（PCS 线格式 V1.3：N-8-1 @19200）
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    /// 数据位，默认 8
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    /// 停止位，默认 1
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    /// 校验位: none/even/odd，默认 none
    #[serde(default = "default_parity")]
    pub parity: String,
    /// 从站地址（有效 1..=247），默认 1
    #[serde(default = "default_slave_addr")]
    pub slave_addr: u8,
    /// 响应超时（毫秒），默认 200
    #[serde(default = "default_response_timeout_ms")]
    pub response_timeout_ms: u64,
    /// 心跳轮询间隔（毫秒），默认 1000
    #[serde(default = "default_heartbeat_poll_ms")]
    pub heartbeat_poll_ms: u64,
}

impl Default for ModbusRtuConfig {
    fn default() -> Self {
        Self {
            serial_port: default_serial_port(),
            baud_rate: default_baud_rate(),
            data_bits: default_data_bits(),
            stop_bits: default_stop_bits(),
            parity: default_parity(),
            slave_addr: default_slave_addr(),
            response_timeout_ms: default_response_timeout_ms(),
            heartbeat_poll_ms: default_heartbeat_poll_ms(),
        }
    }
}

/// Web API 配置
#[derive(Debug, Clone, Deserialize)]
pub struct WebApiConfig {
    /// 监听地址，如 "0.0.0.0:8080"
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    /// 是否启用 HTTPS（Phase 2+）
    #[serde(default = "default_enable_https")]
    pub enable_https: bool,
    /// TLS 证书路径
    pub tls_cert: Option<PathBuf>,
    /// TLS 私钥路径
    pub tls_key: Option<PathBuf>,
}

/// AI 引擎配置
#[derive(Debug, Clone, Deserialize)]
pub struct AiEngineConfig {
    /// 模型文件目录
    #[serde(default = "default_model_dir")]
    pub model_dir: PathBuf,
    /// AI 引擎配置文件路径（mupc_env_config.yaml）
    #[serde(default = "default_env_config_file")]
    pub config_file: PathBuf,
    /// 是否启用 NPU
    #[serde(default = "default_enable_npu")]
    pub enable_npu: bool,
    /// 推理超时（毫秒），默认 500
    #[serde(default = "default_inference_timeout_ms")]
    pub inference_timeout_ms: u64,
    /// 本地策略优先模式（默认 true = 部署默认本地台区储能治理策略优先，AI 旁路；false = AI 优先）
    #[serde(default = "default_local_priority")]
    pub local_priority: bool,
}

/// 插件配置
#[derive(Debug, Clone, Deserialize)]
pub struct PluginsConfig {
    /// 插件搜索路径
    #[serde(default = "default_plugin_search_paths")]
    pub search_paths: Vec<PathBuf>,
    /// 自动加载的插件名列表
    #[serde(default = "default_auto_load")]
    pub auto_load: Vec<String>,
}

/// 策略引擎配置（v2.24 容量档位 §2.10.2）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct StrategyConfig {
    /// 台区储能档位 YAML 路径；空 = 默认档 pcs60_dual（唯一向后兼容分支）。
    /// 换 PCS 规格只改此路径指向的档位 key / YAML 加档，不改代码。
    #[serde(default)]
    pub tai_config_file: String,
}

/// 台区总表分相数据源配置（U-26）
///
/// 总表以 RS485 Modbus 保持寄存器暴露分相量；各量寄存器块定义见
/// [`MasterMeterRegMap`]。寄存器地址为现场点表占位，默认值仅示例。
#[derive(Debug, Clone, Deserialize)]
pub struct MasterMeterConfig {
    /// 是否启用总表采集（默认关；启用须配真点表）
    #[serde(default)]
    pub enabled: bool,
    /// 串口设备，默认 /dev/ttyS4（BECG-3568 板载 COM4 ↔ 关口表/台区总表）
    #[serde(default = "default_meter_serial")]
    pub serial_port: String,
    /// 波特率
    #[serde(default = "default_meter_baud")]
    pub baud_rate: u32,
    /// 总表从站地址
    #[serde(default = "default_meter_slave")]
    pub slave_addr: u8,
    /// 采集周期（毫秒）
    #[serde(default = "default_meter_interval")]
    pub read_interval_ms: u64,
    /// 分相量寄存器映射（各量三相连续，Int32/Float32 均 2 寄存器/相）
    #[serde(default)]
    pub reg_map: MasterMeterRegMap,
}

/// 分相量寄存器映射（各块起始地址；三相连续读 3×2 寄存器）
#[derive(Debug, Clone, Deserialize)]
pub struct MasterMeterRegMap {
    #[serde(default)]
    pub p: MeterRegBlock,
    #[serde(default)]
    pub q: MeterRegBlock,
    #[serde(default)]
    pub pf: MeterRegBlock,
    #[serde(default)]
    pub u: MeterRegBlock,
    #[serde(default)]
    pub i: MeterRegBlock,
    /// 总有功（可选，None 时由分相聚合）
    pub p_total: Option<MeterRegBlock>,
}

/// 单个量寄存器块定义
#[derive(Debug, Clone, Deserialize)]
pub struct MeterRegBlock {
    /// 起始寄存器地址（A 相）
    pub addr: u16,
    /// 数值格式（float32 / int32_scaled）
    #[serde(default = "default_reg_format")]
    pub format: mupc_data_processing::meter_regs::RegFormat,
    /// int32 缩放因子（format=int32_scaled 用）
    #[serde(default = "default_reg_scale")]
    pub scale: f64,
}

// BECG-3568 板载 RS485 COM4(ttyS4) ↔ 关口表/台区总表（核间 10 §12.1 / deploy §九）
fn default_meter_serial() -> String {
    "/dev/ttyS4".to_string()
}
fn default_meter_baud() -> u32 {
    9600
}
fn default_meter_slave() -> u8 {
    3
}
fn default_meter_interval() -> u64 {
    1000
}
fn default_reg_format() -> mupc_data_processing::meter_regs::RegFormat {
    mupc_data_processing::meter_regs::RegFormat::Float32
}
fn default_reg_scale() -> f64 {
    0.01
}

impl Default for MeterRegBlock {
    fn default() -> Self {
        Self {
            addr: 0,
            format: default_reg_format(),
            scale: default_reg_scale(),
        }
    }
}

impl Default for MasterMeterRegMap {
    fn default() -> Self {
        Self {
            p: MeterRegBlock::default(),
            q: MeterRegBlock::default(),
            pf: MeterRegBlock::default(),
            u: MeterRegBlock::default(),
            i: MeterRegBlock::default(),
            p_total: None,
        }
    }
}

impl Default for MasterMeterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            serial_port: default_meter_serial(),
            baud_rate: default_meter_baud(),
            slave_addr: default_meter_slave(),
            read_interval_ms: default_meter_interval(),
            reg_map: MasterMeterRegMap::default(),
        }
    }
}

// ── 默认值函数 ──

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_dir() -> PathBuf {
    PathBuf::from("/opt/mupc/logs")
}

fn default_data_dir() -> PathBuf {
    PathBuf::from("/opt/mupc/data")
}

fn default_plugin_dir() -> PathBuf {
    PathBuf::from("/opt/mupc/lib/plugins")
}

fn default_cert_dir() -> PathBuf {
    PathBuf::from("/opt/mupc/certs")
}

fn default_shutdown_timeout_sec() -> u64 {
    30
}

fn default_intercore_host() -> String {
    "127.0.0.1".to_string()
}

fn default_intercore_port() -> u16 {
    9100
}

fn default_heartbeat_interval() -> u64 {
    5
}

fn default_reconnect_interval() -> u64 {
    3
}

fn default_intercore_transport() -> String {
    "tcp".to_string()
}

// BECG-3568 板载隔离 RS485 COM1(ttyS0) ↔ PCS（核间 10 §12.1）；无 ttyS1
fn default_serial_port() -> String {
    "/dev/ttyS0".to_string()
}

fn default_baud_rate() -> u32 {
    19200
}

fn default_data_bits() -> u8 {
    8
}

fn default_stop_bits() -> u8 {
    1
}

fn default_parity() -> String {
    "none".to_string()
}

fn default_slave_addr() -> u8 {
    1
}

fn default_response_timeout_ms() -> u64 {
    200
}

fn default_heartbeat_poll_ms() -> u64 {
    1000
}

// S2 §12.4: io 段默认值
fn default_io_poll_ms() -> u64 {
    100
}

fn default_stop_confirm_ms() -> u64 {
    5000
}

fn default_true() -> bool {
    true
}

fn default_debounce() -> u32 {
    3
}

fn default_listen_addr() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_enable_https() -> bool {
    false
}

fn default_model_dir() -> PathBuf {
    PathBuf::from("/opt/mupc/models")
}

fn default_env_config_file() -> PathBuf {
    PathBuf::from("/opt/mupc/config/mupc_env_config.yaml")
}

fn default_enable_npu() -> bool {
    true
}

fn default_inference_timeout_ms() -> u64 {
    500
}

fn default_local_priority() -> bool {
    // 部署默认：本地台区储能治理策略优先（AI 旁路）；需 AI 控制时经配置或 Web API 切换
    true
}

fn default_plugin_search_paths() -> Vec<PathBuf> {
    vec![PathBuf::from("/opt/mupc/lib/plugins")]
}

fn default_auto_load() -> Vec<String> {
    vec![
        "rs485_plugin".to_string(),
        "hplc_plugin".to_string(),
        "mqtt_plugin".to_string(),
    ]
}

impl CoreConfig {
    /// 从 YAML 文件加载配置
    pub fn load(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: CoreConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }

    /// 校验配置完整性
    pub fn validate(&self) -> Result<(), String> {
        if self.version.is_empty() {
            return Err("version 字段不能为空".to_string());
        }
        if self.system.log_level.is_empty() {
            return Err("system.log_level 不能为空".to_string());
        }
        if self.intercore.host.is_empty() {
            return Err("intercore.host 不能为空".to_string());
        }
        if self.intercore.port == 0 {
            return Err("intercore.port 不能为 0".to_string());
        }
        if self.web_api.listen_addr.is_empty() {
            return Err("web_api.listen_addr 不能为空".to_string());
        }
        // M7/生产安全：transport=modbus_rtu（PCS 主链路）时，串口/从站/波特率须合法，
        // 且在总表启用时不得复用同一串口（RS485 总线仲裁未实现）。非法值启动即报错，
        // 避免运行时 open_ctx 才暴露。
        if self.intercore.transport == "modbus_rtu" {
            let mb = &self.intercore.modbus_rtu;
            if mb.serial_port.trim().is_empty() {
                return Err("intercore.modbus_rtu.serial_port 不能为空（transport=modbus_rtu）".to_string());
            }
            if !(1..=247).contains(&mb.slave_addr) {
                return Err(format!(
                    "intercore.modbus_rtu.slave_addr={} 须在 1..=247（transport=modbus_rtu）",
                    mb.slave_addr
                ));
            }
            if mb.baud_rate == 0 {
                return Err("intercore.modbus_rtu.baud_rate 不能为 0（transport=modbus_rtu）".to_string());
            }
            if self.master_meter.enabled && self.master_meter.serial_port == mb.serial_port {
                return Err(format!(
                    "transport=modbus_rtu 时 master_meter.serial_port={} 不得与 intercore.modbus_rtu.serial_port 相同（RS485 总线仲裁未实现）",
                    mb.serial_port
                ));
            }
        }
        // TODO(v2.24 M-1)：v2.24 §2.10.2 M-1 预留装配期校验位：策略档位（i_rated/s_rated/dp_max/
        // q_i_max）与 intercore transport 驱动点表型号不自动联动——放行任一非
        // 60kW 无中线档时须与驱动点表同批变更并在此核对（当前 60kW 档与
        // modbus_rtu V1.3 驱动天然匹配；has_neutral=true 档已在档位加载侧拦截）。
        // 注：档位 YAML 的实际加载/校验发生在 startup 装配（fail-fast），此处仅
        // 保留位注释，不读文件、不加逻辑。
        // 实际档位加载/校验在 startup.rs 装配（load_tai_storage_config）处执行（Task 5 落点）。
        // P1-4/P2-2: 台区总表启用时校验现场前提（独立串口/从站）与寄存器映射有效性
        if self.master_meter.enabled {
            self.validate_master_meter()?;
        }
        // S2 §12.4: io.enabled 时校验数字 IO/安全联锁配置（disabled 整段跳过，不打扰未启用用户）
        self.validate_io()?;
        Ok(())
    }

    /// P1-4/P2-2/N1/N2: 校验台区总表配置（enabled 时）：
    /// serial_port 非空、slave_addr∈1..=247、采集周期须小于策略数据新鲜度阈值（5s）、
    /// 与南向 RS485 默认串口分离（总线仲裁未实现）、reg_map 各量地址非 0 且区间互不重叠。
    fn validate_master_meter(&self) -> Result<(), String> {
        let mm = &self.master_meter;
        if mm.serial_port.trim().is_empty() {
            return Err("master_meter.serial_port 不能为空".to_string());
        }
        if !(1..=247).contains(&mm.slave_addr) {
            return Err(format!(
                "master_meter.slave_addr={} 须在 1..=247",
                mm.slave_addr
            ));
        }
        // N1: AiIntegrator 数据新鲜度阈值为 5s——采集周期 ≥5s 会恒判 stale 导致兜底停发
        if mm.read_interval_ms >= 5000 {
            return Err(format!(
                "master_meter.read_interval_ms={} 须 < 5000（AiIntegrator 数据新鲜度阈值 5s，采集须持续更新）",
                mm.read_interval_ms
            ));
        }
        // 南向 RS485 默认 /dev/ttyUSB0（历史 USB-485/跨平台防御；BECG-3568 无 ttyUSB0，
        // 总表默认已迁 /dev/ttyS4，本分支仅对显式写该值或 USB-485 平台生效，勿误删）
        if mm.serial_port == "/dev/ttyUSB0" {
            return Err(
                "master_meter.serial_port 与南向 RS485 默认串口 /dev/ttyUSB0 相同——台区总表须独立于南向 RS485 串口或需总线仲裁（未实现）"
                    .to_string(),
            );
        }
        Self::validate_reg_map(&mm.reg_map)
    }

    /// S2 §12.4: io.enabled 时校验数字 IO/安全联锁配置：
    /// poll_ms>0；各 DI/DO gpio>0、debounce>=1、action∈{pcs_stop,event}；
    /// DI/DO 间 gpio 跨表唯一，di 内与 do 内 name 非空且唯一。
    fn validate_io(&self) -> Result<(), String> {
        let io = &self.io;
        if !io.enabled {
            return Ok(());
        }
        if io.poll_ms == 0 {
            return Err("io.poll_ms 不能为 0（io.enabled 时）".to_string());
        }
        for (i, d) in io.di.iter().enumerate() {
            if d.gpio == 0 {
                return Err(format!("io.di[{}].name={:?} gpio 不能为 0", i, d.name));
            }
            if d.debounce < 1 {
                return Err(format!(
                    "io.di[{}].name={:?} debounce={} 须 >= 1",
                    i, d.name, d.debounce
                ));
            }
            if d.action != "pcs_stop" && d.action != "event" {
                return Err(format!(
                    "io.di[{}].name={:?} action={:?} 须为 pcs_stop 或 event",
                    i, d.name, d.action
                ));
            }
        }
        for (i, d) in io.do_out.iter().enumerate() {
            if d.gpio == 0 {
                return Err(format!("io.do[{}].name={:?} gpio 不能为 0", i, d.name));
            }
        }
        // DI/DO 间 gpio 跨表唯一（防止 DO 复用 DI 引脚 / 引脚冲突）
        let mut gpios: std::collections::HashSet<u32> = Default::default();
        let all_gpio = io
            .di
            .iter()
            .map(|d| d.gpio)
            .chain(io.do_out.iter().map(|d| d.gpio));
        for gpio in all_gpio {
            if !gpios.insert(gpio) {
                return Err(format!(
                    "io 段 DI/DO gpio={} 重复（di/do 表间须唯一）",
                    gpio
                ));
            }
        }
        // di 内 name 非空且唯一
        let mut di_names: std::collections::HashSet<&str> = Default::default();
        for (i, d) in io.di.iter().enumerate() {
            if d.name.is_empty() {
                return Err(format!("io.di[{}] name 不能为空（io.enabled 时）", i));
            }
            if !di_names.insert(d.name.as_str()) {
                return Err(format!(
                    "io.di 内 name={:?} 重复（di 通道名须唯一）",
                    d.name
                ));
            }
        }
        // do 内 name 非空且唯一
        let mut do_names: std::collections::HashSet<&str> = Default::default();
        for (i, d) in io.do_out.iter().enumerate() {
            if d.name.is_empty() {
                return Err(format!("io.do[{}] name 不能为空（io.enabled 时）", i));
            }
            if !do_names.insert(d.name.as_str()) {
                return Err(format!(
                    "io.do 内 name={:?} 重复（do 通道名须唯一）",
                    d.name
                ));
            }
        }
        Ok(())
    }

    /// P2-2/N2: 分相量块 p/q/pf/u/i 起始地址非 0 且三相连续 6 寄存器区间互不重叠；
    /// 可选 p_total（单值 2 寄存器）同样校验且不与其它块重叠。
    fn validate_reg_map(reg_map: &MasterMeterRegMap) -> Result<(), String> {
        struct Block {
            name: &'static str,
            addr: u16,
            width: u32, // 分相量三相连续 6 寄存器；p_total 单值 2
        }
        let mut blocks = vec![
            Block { name: "p", addr: reg_map.p.addr, width: 6 },
            Block { name: "q", addr: reg_map.q.addr, width: 6 },
            Block { name: "pf", addr: reg_map.pf.addr, width: 6 },
            Block { name: "u", addr: reg_map.u.addr, width: 6 },
            Block { name: "i", addr: reg_map.i.addr, width: 6 },
        ];
        if let Some(pt) = &reg_map.p_total {
            blocks.push(Block { name: "p_total", addr: pt.addr, width: 2 });
        }
        for b in &blocks {
            if b.addr == 0 {
                return Err(format!("master_meter.reg_map.{} addr 不能为 0", b.name));
            }
        }
        for (i, bi) in blocks.iter().enumerate() {
            for bj in blocks.iter().skip(i + 1) {
                let ai = bi.addr as u32;
                let aj = bj.addr as u32;
                // 半开区间 [addr, addr+width) 重叠判定
                if ai < aj + bj.width && aj < ai + bi.width {
                    return Err(format!(
                        "master_meter.reg_map.{} 与 {} 寄存器区间重叠（{}+{} 与 {}+{} 不得交叠）",
                        bi.name, bj.name, bi.name, bi.width, bj.name, bj.width
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_config_deserialize_minimal() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "debug"
intercore:
  host: "192.168.1.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:9000"
ai_engine: {}
plugins: {}
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.version, "1.0");
        assert_eq!(config.system.log_level, "debug");
        assert_eq!(config.intercore.host, "192.168.1.1");
        assert_eq!(config.web_api.listen_addr, "0.0.0.0:9000");
        // 默认值校验
        assert_eq!(config.system.shutdown_timeout_sec, 30);
        assert_eq!(config.ai_engine.model_dir, PathBuf::from("/opt/mupc/models"));
        assert_eq!(config.intercore.heartbeat_interval_sec, 5);
        // 未配置 intercore.transport 时默认 tcp
        assert_eq!(config.intercore.transport, "tcp");
        // 未配置 intercore.modbus_rtu 时默认参数
        assert_eq!(config.intercore.modbus_rtu.serial_port, "/dev/ttyS0");
        assert_eq!(config.intercore.modbus_rtu.baud_rate, 19200);
        assert_eq!(config.intercore.modbus_rtu.data_bits, 8);
        assert_eq!(config.intercore.modbus_rtu.stop_bits, 1);
        assert_eq!(config.intercore.modbus_rtu.parity, "none");
        assert_eq!(config.intercore.modbus_rtu.slave_addr, 1);
        assert_eq!(config.intercore.modbus_rtu.response_timeout_ms, 200);
        assert_eq!(config.intercore.modbus_rtu.heartbeat_poll_ms, 1000);
        assert!(config.ai_engine.local_priority, "本地优先应为部署默认");
        // 未配置 master_meter 时默认参数（默认关，serial_port 落默认 /dev/ttyS4）
        assert_eq!(config.master_meter.serial_port, "/dev/ttyS4");
        assert_eq!(config.master_meter.baud_rate, 9600);
        assert_eq!(config.master_meter.slave_addr, 3);
    }

    #[test]
    fn test_core_config_validate_success() {
        let config = CoreConfig {
            version: "1.0".into(),
            system: SystemConfig {
                log_level: "info".into(),
                log_dir: PathBuf::from("/tmp/logs"),
                data_dir: PathBuf::from("/tmp/data"),
                plugin_dir: PathBuf::from("/tmp/plugins"),
                cert_dir: PathBuf::from("/tmp/certs"),
                shutdown_timeout_sec: 30,
            },
            intercore: InterCoreConfig {
                host: "127.0.0.1".into(),
                port: 9100,
                heartbeat_interval_sec: 5,
                reconnect_interval_sec: 3,
                transport: "tcp".into(),
                modbus_rtu: ModbusRtuConfig::default(),
            },
            web_api: WebApiConfig {
                listen_addr: "0.0.0.0:8080".into(),
                enable_https: false,
                tls_cert: None,
                tls_key: None,
            },
            ai_engine: AiEngineConfig {
                model_dir: PathBuf::from("/tmp/models"),
                config_file: PathBuf::from("/tmp/config.yaml"),
                enable_npu: true,
                inference_timeout_ms: 500,
                local_priority: false,
            },
            plugins: PluginsConfig {
                search_paths: vec![PathBuf::from("/tmp/plugins")],
                auto_load: vec!["rs485_plugin".into()],
            },
            master_meter: MasterMeterConfig::default(),
            strategy: StrategyConfig::default(),
            io: IoConfig::default(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_core_config_validate_empty_version() {
        let config = CoreConfig {
            version: "".into(),
            system: SystemConfig {
                log_level: "info".into(),
                log_dir: PathBuf::from("/tmp"),
                data_dir: PathBuf::from("/tmp"),
                plugin_dir: PathBuf::from("/tmp"),
                cert_dir: PathBuf::from("/tmp"),
                shutdown_timeout_sec: 30,
            },
            intercore: InterCoreConfig {
                host: "127.0.0.1".into(),
                port: 9100,
                heartbeat_interval_sec: 5,
                reconnect_interval_sec: 3,
                transport: "tcp".into(),
                modbus_rtu: ModbusRtuConfig::default(),
            },
            web_api: WebApiConfig {
                listen_addr: "0.0.0.0:8080".into(),
                enable_https: false,
                tls_cert: None,
                tls_key: None,
            },
            ai_engine: AiEngineConfig {
                model_dir: PathBuf::from("/tmp"),
                config_file: PathBuf::from("/tmp"),
                enable_npu: false,
                inference_timeout_ms: 500,
                local_priority: false,
            },
            plugins: PluginsConfig {
                search_paths: vec![],
                auto_load: vec![],
            },
            master_meter: MasterMeterConfig::default(),
            strategy: StrategyConfig::default(),
            io: IoConfig::default(),
        };
        assert!(config.validate().is_err());
    }

    /// P1-4: 总表启用但 serial_port 与南向默认 /dev/ttyUSB0 相同 → validate Err
    #[test]
    fn test_validate_master_meter_shared_serial_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
master_meter:
  enabled: true
  serial_port: "/dev/ttyUSB0"
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("仲裁") || err.contains("独立"),
            "期望提示串口冲突/总线仲裁，实际: {}",
            err
        );
    }

    /// M7: transport=modbus_rtu（PCS 生产链路）时 slave_addr 越界 → validate Err
    #[test]
    fn test_validate_modbus_rtu_slave_addr_range() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
  transport: "modbus_rtu"
  modbus_rtu:
    slave_addr: 0
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("slave_addr"),
            "期望提示从站地址越界（transport=modbus_rtu），实际: {}",
            err
        );
    }

    /// M7: transport=modbus_rtu 与总表同串口 → validate Err（RS485 总线仲裁未实现）
    #[test]
    fn test_validate_modbus_rtu_shared_serial_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
  transport: "modbus_rtu"
  modbus_rtu:
    serial_port: "/dev/ttyS1"
    slave_addr: 1
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
master_meter:
  enabled: true
  serial_port: "/dev/ttyS1"
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("不得与 intercore") || err.contains("仲裁"),
            "期望提示与 intercore.modbus_rtu 串口冲突，实际: {}",
            err
        );
    }

    /// P1-4: 总表启用但 slave_addr 越界 → validate Err
    #[test]
    fn test_validate_master_meter_slave_addr_range() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
master_meter:
  enabled: true
  serial_port: "/dev/ttyS2"
  slave_addr: 0
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.validate().is_err());
    }

    /// P2-2: 总表 reg_map 各量地址重叠 → validate Err
    #[test]
    fn test_validate_master_meter_reg_overlap_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
master_meter:
  enabled: true
  serial_port: "/dev/ttyS2"
  slave_addr: 3
  reg_map:
    p: { addr: 0x100 }
    q: { addr: 0x100 }
    pf: { addr: 0x110 }
    u: { addr: 0x116 }
    i: { addr: 0x11C }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.contains("重叠"), "期望提示寄存器重叠，实际: {}", err);
    }

    /// P1-4 + P2-2: 独立串口 + 合法且不重叠 reg_map → validate Ok
    #[test]
    fn test_validate_master_meter_valid_ok() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
master_meter:
  enabled: true
  serial_port: "/dev/ttyS2"
  slave_addr: 3
  reg_map:
    p: { addr: 0x100 }
    q: { addr: 0x106 }
    pf: { addr: 0x10C }
    u: { addr: 0x112 }
    i: { addr: 0x118 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.validate().is_ok(), "合法总表配置应通过: {:?}", config.validate());
    }

    /// N1: 采集周期 >= 数据新鲜度阈值（5s）→ validate Err（兜底会恒判 stale 停发）
    #[test]
    fn test_validate_master_meter_read_interval_too_long() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
master_meter:
  enabled: true
  serial_port: "/dev/ttyS2"
  slave_addr: 3
  read_interval_ms: 5000
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("read_interval_ms"),
            "期望提示采集周期与新鲜度阈值冲突，实际: {}",
            err
        );
    }

    /// N2: 可选 p_total 块与分相块重叠 → validate Err
    #[test]
    fn test_validate_master_meter_p_total_overlap_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
master_meter:
  enabled: true
  serial_port: "/dev/ttyS2"
  slave_addr: 3
  reg_map:
    p: { addr: 0x100 }
    q: { addr: 0x106 }
    pf: { addr: 0x10C }
    u: { addr: 0x112 }
    i: { addr: 0x118 }
    p_total: { addr: 0x100 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.contains("重叠"), "期望提示 p_total 与 p 重叠，实际: {}", err);
    }

    /// v2.24: strategy 段显式配置可解析；合法值 validate 通过
    #[test]
    fn test_core_config_strategy_tai_config_file() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
strategy:
  tai_config_file: "/opt/mupc/config/tai_profiles.yaml"
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.strategy.tai_config_file,
            "/opt/mupc/config/tai_profiles.yaml"
        );
        assert!(
            config.validate().is_ok(),
            "显式配置合法应通过: {:?}",
            config.validate()
        );
    }

    /// v2.24: 未配 strategy 段 → 默认空（load 侧归一化为 None → 默认 60 档）
    #[test]
    fn test_core_config_strategy_default_empty() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.strategy.tai_config_file.is_empty());
    }

    /// S2 §12.4: 未配置 io 段 → 缺省 disabled（enabled=false、di/do 空、poll/stop_confirm 落默认）
    #[test]
    fn test_io_disabled_default() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let io = &config.io;
        assert!(!io.enabled);
        assert!(io.di.is_empty());
        assert!(io.do_out.is_empty());
        assert_eq!(io.poll_ms, 100);
        assert_eq!(io.stop_confirm_ms, 5000);
        assert!(!io.auto_release);
        assert_eq!(io.release_hold_secs, 0);
        // disabled 时 validate 不拦截
        assert!(config.validate().is_ok());
    }

    /// S2 §12.4: 合法 io 段（di 2×pcs_stop + 1×event、do 2）解析正确且 validate 通过
    #[test]
    fn test_io_validate_valid_ok() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: true
  poll_ms: 200
  di:
    - { name: "急停", gpio: 1, action: "pcs_stop" }
    - { name: "水浸", gpio: 2, action: "pcs_stop", active_low: false, debounce: 5 }
    - { name: "门禁", gpio: 3, action: "event" }
  do:
    - { name: "运行灯", gpio: 8 }
    - { name: "故障灯", gpio: 9, active_high: false }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let io = &config.io;
        assert!(io.enabled);
        assert_eq!(io.poll_ms, 200);
        assert_eq!(io.di.len(), 3);
        // di 缺省：active_low 默认 true、debounce 默认 3；显式覆盖生效
        assert!(io.di[0].active_low);
        assert_eq!(io.di[0].debounce, 3);
        assert_eq!(io.di[0].action, "pcs_stop");
        assert!(!io.di[1].active_low);
        assert_eq!(io.di[1].debounce, 5);
        assert_eq!(io.di[2].action, "event");
        // YAML do 键 → do_out rename；active_high 默认 true
        assert_eq!(io.do_out.len(), 2);
        assert!(io.do_out[0].active_high);
        assert!(!io.do_out[1].active_high);
        // stop_confirm_ms / auto_release 缺省
        assert_eq!(io.stop_confirm_ms, 5000);
        assert!(!io.auto_release);
        assert!(
            config.validate().is_ok(),
            "合法 io 配置应通过: {:?}",
            config.validate()
        );
    }

    /// S2 §12.4: action 非法 → validate Err（须为 pcs_stop/event）
    #[test]
    fn test_io_validate_bad_action_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: true
  di:
    - { name: "未知源", gpio: 1, action: "bogus" }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("action") && err.contains("bogus"),
            "期望提示非法 action，实际: {}",
            err
        );
    }

    /// S2 §12.4: DI gpio=0 → validate Err（gpio 必填）
    #[test]
    fn test_io_validate_zero_gpio_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: true
  di:
    - { name: "急停", gpio: 0, action: "pcs_stop" }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("gpio"),
            "期望提示 gpio 不能为 0，实际: {}",
            err
        );
    }

    /// S2 §12.4: di 与 do 间 gpio 重复 → validate Err（DI/DO 引脚跨表须唯一）
    #[test]
    fn test_io_validate_gpio_duplicate_di_do_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: true
  di:
    - { name: "急停", gpio: 3, action: "pcs_stop" }
  do:
    - { name: "运行灯", gpio: 3 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("重复") && err.contains("3"),
            "期望提示 gpio 重复，实际: {}",
            err
        );
    }

    /// S2 §12.4: di 内 name 重复 → validate Err（DI 通道名须唯一）
    #[test]
    fn test_io_validate_duplicate_di_name_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: true
  di:
    - { name: "急停", gpio: 1, action: "pcs_stop" }
    - { name: "急停", gpio: 2, action: "pcs_stop" }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("重复") && err.contains("急停"),
            "期望提示 di name 重复，实际: {}",
            err
        );
    }

    /// S2 §12.4: io.enabled=false 时 di/do 含非法内容（bad action/gpio=0）仍放行——
    /// disabled 整段跳过校验的行为契约（未启用联锁的部署不被误拦）
    #[test]
    fn test_io_disabled_bypasses_validation() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: false
  di:
    - { name: "急停", gpio: 0, action: "bogus" }
  do:
    - { name: "故障灯", gpio: 0 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!config.io.enabled);
        assert!(
            config.validate().is_ok(),
            "enabled=false 应跳过 io 校验: {:?}",
            config.validate()
        );
    }

    /// S2 §12.4: do 表专属拒绝路径——gpio=0 / name 空 / name 重复各 Err（与 di 路径对称）
    #[test]
    fn test_io_validate_do_rejections() {
        let base = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: true
  do:
"#;
        // gpio=0 → Err
        let yaml = format!("{}\n    - {{ name: \"运行灯\", gpio: 0 }}\n", base);
        let config: CoreConfig = serde_yaml::from_str(&yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.contains("do") && err.contains("gpio"), "实际: {}", err);
        // name 空 → Err
        let yaml = format!("{}\n    - {{ name: \"\", gpio: 7 }}\n", base);
        let config: CoreConfig = serde_yaml::from_str(&yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.contains("do") && err.contains("name"), "实际: {}", err);
        // name 重复 → Err
        let yaml = format!(
            "{}\n    - {{ name: \"运行灯\", gpio: 7 }}\n    - {{ name: \"运行灯\", gpio: 8 }}\n",
            base
        );
        let config: CoreConfig = serde_yaml::from_str(&yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(err.contains("重复") && err.contains("运行灯"), "实际: {}", err);
    }
}
