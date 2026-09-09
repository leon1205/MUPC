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
    /// 策略引擎配置（v2.24：容量档位 YAML 路径）
    #[serde(default)]
    pub strategy: StrategyConfig,
    /// 数字 IO / 安全联锁配置（S2 §12.4 io: 段；缺省 disabled，未配置 io 段部署行为不变）
    #[serde(default)]
    pub io: IoConfig,
    /// 站级南向统一调度配置（S3 §10.3 south_stations 段；缺省空——未配置站时
    /// 策略 phase 由 pv/load 南向模拟兜底，部署行为不变。master_meter 段已删除收敛，S3b-1c）
    #[serde(default)]
    pub south_stations: mupc_southd::config::SouthStationsConfig,
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
        // M7/生产安全：transport=modbus_rtu（PCS 主链路）时，串口/从站/波特率须合法。
        // 非法值启动即报错，避免运行时 open_ctx 才暴露。
        if self.intercore.transport == "modbus_rtu" {
            let mb = &self.intercore.modbus_rtu;
            if mb.serial_port.trim().is_empty() {
                return Err(
                    "intercore.modbus_rtu.serial_port 不能为空（transport=modbus_rtu）".to_string(),
                );
            }
            if !(1..=247).contains(&mb.slave_addr) {
                return Err(format!(
                    "intercore.modbus_rtu.slave_addr={} 须在 1..=247（transport=modbus_rtu）",
                    mb.slave_addr
                ));
            }
            if mb.baud_rate == 0 {
                return Err(
                    "intercore.modbus_rtu.baud_rate 不能为 0（transport=modbus_rtu）".to_string(),
                );
            }
        }
        // TODO(v2.24 M-1)：v2.24 §2.10.2 M-1 预留装配期校验位：策略档位（i_rated/s_rated/dp_max/
        // q_i_max）与 intercore transport 驱动点表型号不自动联动——放行任一非
        // 60kW 无中线档时须与驱动点表同批变更并在此核对（当前 60kW 档与
        // modbus_rtu V1.3 驱动天然匹配；has_neutral=true 档已在档位加载侧拦截）。
        // 注：档位 YAML 的实际加载/校验发生在 startup 装配（fail-fast），此处仅
        // 保留位注释，不读文件、不加逻辑。
        // 实际档位加载/校验在 startup.rs 装配（load_tai_storage_config）处执行（Task 5 落点）。
        // S2 §12.4: io.enabled 时校验数字 IO/安全联锁配置（disabled 整段跳过，不打扰未启用用户）
        self.validate_io()?;
        // S3 §10.3: south_stations 段校验（段内 validate + 跨段：与 PCS 主链路串口互斥、
        // 站内同节点别名互斥）。stations 空（未配置站）整段跳过——部署行为不变。
        if !self.south_stations.stations.is_empty() {
            self.validate_south_stations()?;
        }
        Ok(())
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
        // S2 Task7 Important：transport=modbus_rtu（PCS 主链路）时 io.stop_confirm_ms 须 ≥ 2×心跳
        // 轮询周期。原因：interlock 无主动读路径，last_run_state 由心跳缓存刷新（modbus.rs
        // run_heartbeat_loop 按 heartbeat_poll_ms 周期更新）；若停机确认窗口 < 心跳周期，PCS 已停但
        // 缓存要下个心跳才报 0 → 确认自旋/帧检查会误判超时置**假 stop_failed**。取 2× 给缓存刷新留
        // 余量（含坏读/丢拍）。heartbeat_poll_ms=0 回退 1000ms（与 modbus.rs run_heartbeat_loop 的
        // 退避常量一致）。io.enabled 已在上方早退保证非空。
        if self.intercore.transport == "modbus_rtu" {
            let hb = self.intercore.modbus_rtu.heartbeat_poll_ms;
            let hb_effective = if hb == 0 { 1000 } else { hb };
            if io.stop_confirm_ms < 2 * hb_effective {
                return Err(format!(
                    "io.stop_confirm_ms={} 须 >= 2×intercore.modbus_rtu.heartbeat_poll_ms={} \
                     （heartbeat_poll_ms=0 回退 1000；transport=modbus_rtu）——停机确认窗口须覆盖≥2个心跳\
                     周期，否则 PCS 已停而心跳缓存未刷新时误判停机超时（假 stop_failed）",
                    io.stop_confirm_ms,
                    2 * hb_effective
                ));
            }
        }
        Ok(())
    }

    /// S3 §10.3 跨段校验（south_stations.stations 非空时由 validate() 调用）：
    /// ① south_stations.validate()（段内，mupc-southd 实现：id 唯一非空、meter_grid 至多一站、
    ///    port 非空、slave 1..=247、interval_ms>0、meter_grid interval_ms < DATA_FRESHNESS_MS）失败传播；
    /// ② transport=="modbus_rtu"（PCS ttyS0 主链路）时任一 station.port 与 modbus_rtu.serial_port
    ///    同串口 → Err（RS485 总线仲裁未实现，禁双 master 共总线；串口节点名归一比较）。
    ///    总表收敛 south_stations.meter_grid 后，master_meter 段删除（S3b-1c），无 legacy 占口可排他；
    /// ③ 站内同节点别名端口互斥：两站 port_node 相同（同物理口）但原始 port 字符串不同
    ///    （"ttyS4" vs "/dev/ttyS4"）→ Err。原因见 Rs485PortBus::normalize_port 双写法支持——
    ///    startup seen_ports 与 scheduler runner 分组均按**原始串**去重/分口，别名拼写会让同物理口
    ///    open 两次并分属两 runner → 同总线并发双 master 帧交错。原始串完全一致（node+raw 都同）
    ///    的合法同口多从站不受影响。
    fn validate_south_stations(&self) -> Result<(), String> {
        // ① 段内校验（含 meter_grid interval_ms < DATA_FRESHNESS_MS 新鲜度边界）
        self.south_stations.validate()?;
        let ss = &self.south_stations;
        // ③ 站内同节点别名端口互斥：遍历已见 (节点名, 原始 port, id)，新站 node 与已见 node
        // 相同但原始 raw 不同 → Err（同物理口别名双拼写）；node+raw 都同（合法同口多从站）→ 跳过。
        let mut seen: Vec<(String, String, String)> = Vec::new();
        for s in &ss.stations {
            let node = port_node(&s.port).to_string();
            if let Some((_prev_node, prev_raw, prev_id)) = seen
                .iter()
                .find(|(n, raw, _)| *n == node && *raw != s.port)
            {
                return Err(format!(
                    "south_stations 站 {} port {} 与站 {} port {} 为同节点 {} 的别名端口——同物理口禁双拼写并站（统一写 /dev/ttyX 或 ttyX），否则该口被 open 两次 / 双 runner 并发双 master 帧交错",
                    s.id, s.port, prev_id, prev_raw, node
                ));
            }
            seen.push((node, s.port.clone(), s.id.clone()));
        }
        // ② transport=modbus_rtu（PCS 主链路 ttyS0）时站串口不得与其同总线（tcp 部署时不生效）。
        for s in &ss.stations {
            if self.intercore.transport == "modbus_rtu"
                && port_node(&s.port) == port_node(&self.intercore.modbus_rtu.serial_port)
            {
                return Err(format!(
                    "south_stations 站 {} port {} 与 intercore.modbus_rtu.serial_port {} 重复（PCS 主链路 RS485 总线仲裁未实现，禁双 master 共总线）",
                    s.id, s.port, self.intercore.modbus_rtu.serial_port
                ));
            }
        }
        Ok(())
    }

}

/// 串口节点名归一："/dev/ttyS0" 与 "ttyS0" 都取 "ttyS0"（跨段串口重复比较基准；
/// BECG ttySx/COMx，站 port 可能写短名，modbus_rtu.serial_port 写全路径）。
fn port_node(p: &str) -> &str {
    p.rsplit('/').next().unwrap_or(p)
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
        assert_eq!(
            config.ai_engine.model_dir,
            PathBuf::from("/opt/mupc/models")
        );
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
            strategy: StrategyConfig::default(),
            io: IoConfig::default(),
            south_stations: mupc_southd::config::SouthStationsConfig::default(),
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
            strategy: StrategyConfig::default(),
            io: IoConfig::default(),
            south_stations: mupc_southd::config::SouthStationsConfig::default(),
        };
        assert!(config.validate().is_err());
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

    /// M7: transport=modbus_rtu 时 serial_port 为空 → validate Err（master_meter 段删除后，
    /// 保留纯 modbus_rtu 串口自校验覆盖；原与总表同串口跨判随 master_meter 收敛删除）
    #[test]
    fn test_validate_modbus_rtu_serial_empty_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
  transport: "modbus_rtu"
  modbus_rtu:
    serial_port: ""
    slave_addr: 1
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("serial_port"),
            "期望提示 modbus_rtu.serial_port 不能为空，实际: {}",
            err
        );
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
        assert!(
            err.contains("重复") && err.contains("运行灯"),
            "实际: {}",
            err
        );
    }

    /// S2 Task7 Important: transport=modbus_rtu（PCS 主链路）时，stop_confirm_ms < 2×心跳 → Err
    /// （停机确认窗口须覆盖≥2个心跳周期，防 PCS 已停但心跳缓存未刷新时误判超时/假 stop_failed）
    #[test]
    fn test_io_modbus_rtu_stop_confirm_too_small_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
  transport: "modbus_rtu"
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: true
  stop_confirm_ms: 1000
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("stop_confirm_ms") && err.contains("heartbeat_poll_ms"),
            "期望提示 stop_confirm_ms 与心跳窗口交叉校验，实际: {}",
            err
        );
    }

    /// S2 Task7 Important: 边界 stop_confirm_ms == 2×心跳 → 通过
    #[test]
    fn test_io_modbus_rtu_stop_confirm_boundary_ok() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
  transport: "modbus_rtu"
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: true
  stop_confirm_ms: 2000
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(
            config.validate().is_ok(),
            "stop_confirm_ms==2×心跳 应通过: {:?}",
            config.validate()
        );
    }

    /// S2 Task7 Important: heartbeat_poll_ms=0 回退 1000ms（与 modbus.rs run_heartbeat_loop 一致）→
    /// stop_confirm_ms=1500（< 2×1000）仍 Err
    #[test]
    fn test_io_modbus_rtu_heartbeat_zero_fallback_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
  transport: "modbus_rtu"
  modbus_rtu:
    heartbeat_poll_ms: 0
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: true
  stop_confirm_ms: 1500
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("heartbeat_poll_ms") && err.contains("回退"),
            "期望按 heartbeat_poll_ms=0 回退 1000 判定，实际: {}",
            err
        );
    }

    /// S2 Task7 Important: transport=tcp 时不作交叉校验（stop_confirm_ms 小不误伤）
    #[test]
    fn test_io_tcp_does_not_cross_validate_stop_confirm() {
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
  stop_confirm_ms: 100
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(
            config.validate().is_ok(),
            "transport=tcp 不应做 modbus 心跳交叉校验: {:?}",
            config.validate()
        );
    }

    /// S2 Task7 Important: transport=modbus_rtu + io.enabled 且 stop_confirm_ms 走默认 5000 →
    /// 默认配置（5000 ≥ 2×1000=2000）仍合法，不得误伤既有合法默认
    #[test]
    fn test_io_modbus_rtu_default_stop_confirm_legal() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
  transport: "modbus_rtu"
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
io:
  enabled: true
  poll_ms: 200
  di:
    - { name: "急停", gpio: 1, action: "pcs_stop" }
  do:
    - { name: "运行灯", gpio: 8 }
    - { name: "故障灯", gpio: 9 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.io.stop_confirm_ms, 5000);
        assert_eq!(config.intercore.modbus_rtu.heartbeat_poll_ms, 1000);
        assert!(
            config.validate().is_ok(),
            "modbus_rtu 合法默认（stop_confirm_ms=5000 ≥ 2×1000）应通过: {:?}",
            config.validate()
        );
    }

    /// S3 §10.3: south_stations 5 站（含 meter_grid）YAML 解析 + validate 合法
    /// （transport=tcp 无 PCS 串口互斥；south_stations 段内自校验通过）
    #[test]
    fn test_south_stations_valid_5_station_passes() {
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
south_stations:
  poll_ms: 1000
  stale_timeout_s: 5
  stations:
    - { id: meter_grid, role: meter_grid, port: /dev/ttyS1, slave: 1, interval_ms: 1000 }
    - { id: meter_batt, role: meter_batt, port: /dev/ttyS1, slave: 2, interval_ms: 1000 }
    - { id: battery_1, role: battery, port: /dev/ttyS2, slave: 1, interval_ms: 1000 }
    - { id: hvac_1, role: hvac, port: /dev/ttyS3, slave: 3, interval_ms: 2000 }
    - { id: fire_1, role: fire, port: /dev/ttyS4, slave: 1, interval_ms: 2000 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.south_stations.stations.len(), 5);
        assert!(
            config.validate().is_ok(),
            "5 站 south_stations 应通过: {:?}",
            config.validate()
        );
    }

    /// S3 §10.3 跨段 ②: transport=modbus_rtu（PCS 主链路）时，站 port 与 modbus_rtu.serial_port
    /// 同串口（站写短名 "ttyS0"，归一后与 "/dev/ttyS0" 同）→ validate Err（禁双 master 共总线）
    #[test]
    fn test_south_stations_shared_serial_with_pcs_short_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
  transport: "modbus_rtu"
  modbus_rtu:
    serial_port: "/dev/ttyS0"
    slave_addr: 1
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
south_stations:
  stations:
    - { id: battery_1, role: battery, port: "ttyS0", slave: 1, interval_ms: 1000 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("重复") && err.contains("仲裁"),
            "期望提示站与 PCS 主链路串口重复（总线仲裁未实现），实际: {}",
            err
        );
    }

    /// S3 §10.3 跨段 ②: 站 port 与 modbus_rtu.serial_port 均写全路径 "/dev/ttyS0"
    /// → 节点名归一后仍重复 → Err
    #[test]
    fn test_south_stations_shared_serial_with_pcs_fullpath_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
  transport: "modbus_rtu"
  modbus_rtu:
    serial_port: "/dev/ttyS0"
    slave_addr: 1
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
south_stations:
  stations:
    - { id: battery_1, role: battery, port: "/dev/ttyS0", slave: 1, interval_ms: 1000 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("重复") && err.contains("仲裁"),
            "期望提示站与 PCS 主链路串口重复（全路径归一），实际: {}",
            err
        );
    }

    /// port_node 纯函数边界：空串 → ""；无斜杠短名 → 原样；全路径 → 末段节点名；
    /// Windows COMx → 原样
    #[test]
    fn test_port_node_boundaries() {
        assert_eq!(port_node(""), "");
        assert_eq!(port_node("ttyS0"), "ttyS0");
        assert_eq!(port_node("/dev/ttyS0"), "ttyS0");
        assert_eq!(port_node("/dev/ttyUSB0"), "ttyUSB0");
        assert_eq!(port_node("COM3"), "COM3");
    }

    /// S3 §10.3: 只配 south_stations meter_grid（总表收敛后唯一 grid 形态）→ 合法
    #[test]
    fn test_south_stations_grid_only_passes() {
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
south_stations:
  stations:
    - { id: meter_grid, role: meter_grid, port: /dev/ttyS1, slave: 1, interval_ms: 1000 }
    - { id: battery_1, role: battery, port: /dev/ttyS2, slave: 1, interval_ms: 1000 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(
            config.validate().is_ok(),
            "只配 south_stations meter_grid 应通过: {:?}",
            config.validate()
        );
    }

    /// S3 §10.3: yaml 无 south_stations 段 → serde(default) 空 → 合法（向后兼容）
    #[test]
    fn test_missing_south_stations_defaults_empty_passes() {
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
        assert!(config.south_stations.stations.is_empty());
        assert_eq!(config.south_stations.poll_ms, 1000);
        assert!(
            config.validate().is_ok(),
            "缺省 south_stations 应通过: {:?}",
            config.validate()
        );
    }

    /// S3 §10.3: meter_grid interval_ms=6000（>= DATA_FRESHNESS_MS）由段内 south_stations.validate()
    /// 拒绝并传播 → core validate Err
    #[test]
    fn test_south_stations_meter_grid_slow_interval_propagated() {
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
south_stations:
  stations:
    - { id: meter_grid, role: meter_grid, port: /dev/ttyS1, slave: 1, interval_ms: 6000 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("interval_ms") && err.contains("south_stations"),
            "meter_grid interval 过慢应段内拒绝并传播，实际: {}",
            err
        );
    }

    /// S3a Fix1: 两站同物理口但别名拼写（hvac 写短名 "ttyS3"、battery 写全路径 "/dev/ttyS3"）
    /// → validate Err（同节点别名端口禁并用——startup/scheduler 按原始串去重会令同口 open 两次）
    #[test]
    fn test_south_stations_same_port_alias_spelling_rejected() {
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
south_stations:
  stations:
    - { id: hvac_1, role: hvac, port: "ttyS3", slave: 3, interval_ms: 2000 }
    - { id: battery_1, role: battery, port: "/dev/ttyS3", slave: 1, interval_ms: 1000 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("别名") || err.contains("同节点"),
            "期望提示同节点别名端口互斥，实际: {}",
            err
        );
        assert!(
            err.contains("hvac_1") && err.contains("battery_1"),
            "Err 消息应含两站 id，实际: {}",
            err
        );
    }

    /// S3a Fix1: 两站同物理口且原始串完全一致（hvac + battery 都写 "ttyS3"，slave 不同）
    /// → Ok（合法同口多从站，startup seen_ports 按原始串去重判同、scheduler 同一 runner 串行）
    #[test]
    fn test_south_stations_same_port_same_spelling_passes() {
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
south_stations:
  stations:
    - { id: hvac_1, role: hvac, port: "ttyS3", slave: 3, interval_ms: 2000 }
    - { id: battery_1, role: battery, port: "ttyS3", slave: 1, interval_ms: 1000 }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(
            config.validate().is_ok(),
            "同物理口同拼写（合法同口多从站）应通过: {:?}",
            config.validate()
        );
    }
}
