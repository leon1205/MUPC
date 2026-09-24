//! 主配置文件 `mupc_core_config.yaml` 结构定义
//!
//! 定义 mupcd 守护进程的完整配置结构，包括系统参数、
//! 核间通信、AI 引擎、插件、网关、IO/联锁、站级南向与本地显示终端配置。
//! （原「Web API」段已随 `mupc-web-api` crate 删除——单元 K；现场遗留的 `web_api:` 段按
//! **未建模段**容忍并逐字保留，见 [`CoreConfig`] 顶部说明。）

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use mupc_display_proto::DisplayConfig;

/// 主配置文件顶层结构
///
/// ⚠️ `Serialize` 的**唯一用途**是设计 §4.3.2.1 的**回退路径**（保留式编辑无法定位目标键时的
/// 整体序列化回写）与**往返单测**；它**不是**正常保存路径（正常路径是文本行级替换，见
/// `yaml_edit.rs`）。**不得**据此推断"配置回写 = 序列化整棵树"——那会丢注释与未建模键。
///
/// 未新增 `deny_unknown_fields`（设计 §4.3.2.1「`Serialize` 的边界」）：现场 yaml 里仍有
/// **本结构未建模**的段/键（运维手写的 `legacy_top:` 一类）必须继续可加载，否则升级即启动失败。
///
/// ⚠️ **单元 K 订正（2026-09）：`web_api:` 已从「已建模段」退为「未建模段」**。本结构原有一个
/// `pub web_api: WebApiConfig` 字段，随 `mupc-web-api` crate 整体删除（设计 §7.2 Step 4）。
/// 结论有二，**两条都必须成立**：
/// - **能读**：现场既有的带 `web_api:` 段的 yaml **仍可正常加载**（未设 `deny_unknown_fields`
///   ⇒ 该段被**忽略**而非报错），不强制运维立即改文件（设计 §7.3 兼容性主张前半）。
///   由 `legacy_web_api_section_still_loads_and_is_ignored` 钉死。
/// - **写了不丢**：正常保存路径是**保留式编辑**（文本行级替换，见 `yaml_edit.rs`），`web_api:`
///   段连同其注释**逐字保留**。只有在**整体回写**（保留式编辑不可定位时的回退路径）下它才会
///   整段消失——而现在它与 `legacy_top:` 属**同一类**（未建模段），"整体回写会丢未建模段"
///   这条已登记的边界**同样覆盖它**（`WriteMode::FullRewrite` 在回执/审计里可见，EDGE-23）。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoreConfig {
    /// 配置版本号（用于兼容性校验）
    pub version: String,
    /// 系统级配置
    pub system: SystemConfig,
    /// 核间通信配置
    pub intercore: InterCoreConfig,
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
    /// IEC 104 网关配置（S2 §12.3 gateway 段；缺省 0.0.0.0:2404——未配置 gateway 段
    /// 部署行为不变，仅端口不再硬编码 2404、可经 config 指定。审查 R2-A2）
    #[serde(default)]
    pub gateway: GatewayConfig,
    /// MQTT 桥接开关（审查 R2-B5 §mqtt_bridge 段；缺省双 false——未启用不 spawn，不再用
    /// Default（mqtt.example.com:8883 + dummy 证书）无条件真连假域名。端点/证书细节仍走
    /// mupc_mqtt_bridge crate Default）
    #[serde(default)]
    pub mqtt_bridge: MqttBridgeConfig,
    /// 本地显示终端发布侧配置（12-本地显示终端 设计 §7：mupcd 解析 yaml `display:` 段 →
    /// `display-proto::DisplayConfig`（单一真源，非重复定义）；缺省 disabled——未配置 display
    /// 段部署行为不变，不启屏）
    #[serde(default)]
    pub display: DisplayConfig,
    /// 存储运行参数（03 设计 §9.2 / PRD §11.3，U-67）。**整段缺省 ⇒ 取 `Default`
    /// （= 现实现常量 1000 / 5000 / 60000）⇒ 零行为变化**（PRD R-11.3-C / STG-01）。
    /// **只含三键**（PRD R-11.3-A 明文）：不含保留期字段、不含 `max_retained_points`。
    #[serde(default)]
    pub storage: StorageSectionConfig,
}

/// 03 设计 §9.2.1（U-67）：`storage:` 段的三个键 —— **进 YAML、不进 DB**，**重启生效**
/// （不引入 DB 覆写层，不做运行时热更新）。
///
/// ```yaml
/// storage:
///   batch_capacity: 1000              # 遥测写缓冲批量提交容量（条）
///   flush_interval_ms: 5000           # 遥测写缓冲提交间隔（ms）
///   grid_aggregate_period_ms: 60000   # 总表电气量聚合落库周期（ms）
/// ```
///
/// **两个量不可互推（PRD R-11.3-D / STG-05）**：`batch_capacity` + `flush_interval_ms` 是
/// **flush 窗口**（缓冲区**提交节拍**，只影响落库延迟与事务粒度）；`grid_aggregate_period_ms`
/// 是**存储周期**（记录**时间粒度**）。前者由 `mupc_storage::WriteBuffer` 解释、后者由
/// `mupc_storage::GridAggregator` 解释，**两个类型互不引用** ⇒ 正交由结构保证。
///
/// `Default` 的取值 = **变更前的硬编码**（`startup.rs` 的 `WriteBuffer::new(1000, 5000, …)`
/// 与 §4.1.1 的「默认 1 分钟」）⇒ 段缺省时运行行为**逐条一致**。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)] // **整段缺省 ⇒ 取 Default（= 现实现常量）**
pub struct StorageSectionConfig {
    #[serde(default = "default_batch_capacity")]
    pub batch_capacity: u64,
    #[serde(default = "default_flush_interval_ms")]
    pub flush_interval_ms: u64,
    #[serde(default = "default_grid_aggregate_period_ms")]
    pub grid_aggregate_period_ms: u64,
}

impl Default for StorageSectionConfig {
    /// **1000 / 5000 / 60000** —— 与变更前的现实现**逐字相同**（零行为变化，PRD R-11.3-C）。
    fn default() -> Self {
        Self {
            batch_capacity: default_batch_capacity(),
            flush_interval_ms: default_flush_interval_ms(),
            grid_aggregate_period_ms: default_grid_aggregate_period_ms(),
        }
    }
}

fn default_batch_capacity() -> u64 {
    1000
}

fn default_flush_interval_ms() -> u64 {
    5000
}

fn default_grid_aggregate_period_ms() -> u64 {
    60_000
}

/// 数字 IO / 安全联锁配置（S2 §12.4 io: 段；缺省 disabled——未配置 io 段部署行为不变）
///
/// enabled=true 时按 DI 触发源（急停/水浸/消防 → pcs_stop；门禁 → event）驱动
/// 安全联锁，DO 输出运行/故障灯。
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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
#[derive(Debug, Clone, Deserialize, Serialize)]
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

/// AI 引擎配置
#[derive(Debug, Clone, Deserialize, Serialize)]
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
    /// 本地策略优先开关（平台目标调整 2026-09-09：AI 引擎暂停，本地策略为唯一默认下发
    /// 引擎）。AI 暂停期恒 true，观测空间维度重构前禁止置 false——false 分支代码保留为
    /// 框架（ai_integration.rs dispatch_ai_decision），不参与生产。
    #[serde(default = "default_local_priority")]
    pub local_priority: bool,
}

/// 插件配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginsConfig {
    /// 插件搜索路径
    #[serde(default = "default_plugin_search_paths")]
    pub search_paths: Vec<PathBuf>,
    /// 自动加载的插件名列表
    #[serde(default = "default_auto_load")]
    pub auto_load: Vec<String>,
}

/// 策略引擎配置（v2.24 容量档位 §2.10.2）
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct StrategyConfig {
    /// 台区储能档位 YAML 路径；空 = 默认档 pcs60_dual（唯一向后兼容分支）。
    /// 换 PCS 规格只改此路径指向的档位 key / YAML 加档，不改代码。
    #[serde(default)]
    pub tai_config_file: String,
}

/// IEC 104 网关配置（S2 §12.3 gateway 段；审查 R2-A2：北向监听地址/端口读 config，
/// 不再于 startup 硬编码 2404）。手动实现 `Default`（不走 derive），使 `#[serde(default)]`
/// 缺省整段配置时落到下方默认函数（0.0.0.0:2404，与历史硬编码一致）。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayConfig {
    /// IEC 104 监听地址，默认 0.0.0.0
    #[serde(default = "default_gateway_addr")]
    pub listen_addr: String,
    /// IEC 104 监听端口，默认 2404
    #[serde(default = "default_gateway_port")]
    pub listen_port: u16,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            listen_addr: default_gateway_addr(),
            listen_port: default_gateway_port(),
        }
    }
}

/// MQTT 桥接配置（审查 R2-B5：north_enabled/local_enabled 缺省双 false——未启用不 spawn，
/// 不再用 Default 真连 mqtt.example.com 假域名）。`derive(Default)`（两字段皆 `bool` ⇒
/// 缺省全 false）与 `#[serde(default)]` 配合，缺省整段配置时同样落到 false，与历史
/// （无条件 spawn）行为变更对齐。
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MqttBridgeConfig {
    /// 北向 emqx 桥接是否启用（缺省 false）
    #[serde(default)]
    pub north_enabled: bool,
    /// 本地 mosquitto 桥接是否启用（缺省 false）
    #[serde(default)]
    pub local_enabled: bool,
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

// IEC 104 网关段（S2 §12.3 gateway）默认值
fn default_gateway_addr() -> String {
    "0.0.0.0".to_string()
}

fn default_gateway_port() -> u16 {
    2404
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
    // 部署默认：本地台区储能治理策略优先（AI 旁路）；需 AI 控制时改 `ai_engine.local_priority`
    // 后重启（单元 K 后**无**运行时切换端点——原 Web API 出口已随 crate 删除）
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
        // 03 设计 §9.2.2（U-67）：storage 段校验（**无 enabled 门控** —— PRD R-11.1-A 明文
        // 「总表落库不设关闭开关」⇒ 本段任何取值都必须合法，不存在"整段跳过"）。
        self.validate_storage()?;
        // 12-本地显示终端 §7.3：display 段校验（enabled 时 bind_addr 强制仅回环 127.0.0.1；
        // disabled 整段跳过——未启用用户不打扰）。非 modbus transport 的 warn 在 startup
        // 装配处发射（main Phase 1 validate 早于 tracing 初始化，此处 warn 不可达）。
        self.validate_display()?;
        Ok(())
    }

    /// 03 设计 §9.2.2（U-67 / PRD R-11.3-E）：`storage:` 段合法性 —— **违规即拒启动**
    /// （fail-fast，错误信息**点名违规键**）。
    ///
    /// 为什么拒启动而不是"告警 + 降级取默认"：三项均**仅在启动期读取一次**（重启生效，
    /// 不进 DB、无热更新），静默降级会造成「以为配了其实没配」的失真；且与既有各段
    /// （`intercore` / `gateway` / `io` / `display`）「违规即 `Err`」的惯例一致。
    ///
    /// 本函数**不读文件、不做跨段校验**（与 `validate_io` 的 `enabled` 门控不同：本段
    /// **无开关**，PRD R-11.1-A）。两参数的**正交性**（`batch_capacity`/`flush_interval_ms`
    /// = 提交节拍 vs `grid_aggregate_period_ms` = 记录时间粒度，PRD R-11.3-D）**不在此断言** ——
    /// 由结构保证（分属 `WriteBuffer` / `GridAggregator` 两个类型，互不引用，STG-05）。
    fn validate_storage(&self) -> Result<(), String> {
        let s = &self.storage;
        if !(1..=100_000).contains(&s.batch_capacity) {
            return Err(format!(
                "storage.batch_capacity={} 须在 1..=100000（遥测写缓冲批量提交容量，条）",
                s.batch_capacity
            ));
        }
        if !(100..=600_000).contains(&s.flush_interval_ms) {
            return Err(format!(
                "storage.flush_interval_ms={} 须在 100..=600000（禁 0；遥测写缓冲提交间隔，ms）",
                s.flush_interval_ms
            ));
        }
        if !(10_000..=3_600_000).contains(&s.grid_aggregate_period_ms) {
            return Err(format!(
                "storage.grid_aggregate_period_ms={} 须在 10000..=3600000（总表聚合落库周期，ms）",
                s.grid_aggregate_period_ms
            ));
        }
        if s.grid_aggregate_period_ms % 1000 != 0 {
            return Err(format!(
                "storage.grid_aggregate_period_ms={} 须为 1000 的整数倍（时间戳按整秒对齐）",
                s.grid_aggregate_period_ms
            ));
        }
        Ok(())
    }

    /// 12-本地显示终端 §7.3：`display.enabled` 时**两条**地址（`bind_addr` = 读通道 /
    /// `control_bind_addr` = 控制通道）强制仅回环——本地数据通道禁止暴露到外网/北向网口；
    /// 非回环地址启动即报错。disabled 整段跳过。
    ///
    /// **口径（唯一真源 = 契约）**：只认**字面量**回环 —— host 必须是 `127.0.0.1` 或 `::1`
    /// （`"[::1]:9811"` 亦可，因按 `SocketAddr` 解析），端口须 ∈ [1, 65535]（**端口 ≠ 0**），
    /// 且**两条地址不得相同**（同址会让读/控制两条通道互相抢占）。判定与错误文案**全部**由契约
    /// `DisplayConfig::validate()`（`display-proto/src/config.rs`）给出，本函数只做转发。
    ///
    /// **口径收紧的沿革与理由**：本函数原先是**手写**校验，把 `localhost:9810` / `localhost`
    /// 也识别为回环（按 host 部分归一、缺端口单独报「缺少端口」）——**该口径已废弃**。名字可经
    /// hosts（或 DNS）重映射到非回环地址，安全红线（PL-4）上不接受名字；渲染端 `console.rs`
    /// 同样只收字面量，两侧口径由此一致。「缺端口」（`"127.0.0.1"`）现按契约统一归为**非法**
    /// （报「非合法回环 host:port」，不再是误导性的「缺少端口」）。
    ///
    /// ⚠️ **全集校验（不止地址）**：转发的 `DisplayConfig::validate()` 是**全集**校验，启动期
    /// 一并门禁下列**非地址**不变量（fail-fast，任一不合规则 `mupcd` **启动失败**）：
    /// ⚠️ **数值逐字以契约为准**（本函数只转发，不自持口径）：`publish_ms ∈ [100, 4000]`
    /// （**B3-2d 补上界**：上界 = 契约 `MAX_PUBLISH_MS` = **4000**，PM 裁定，依据设计 §4.2.1 的
    /// 上屏 ≤2 s 验收——主拍慢于最慢的一段采集（`device_poll_ms` ≤4000）时该拆解表失效，同时
    /// `min = publish = 60_000` 这类退化组合（端到端 ≈61 s）会被这条**挡在启动期**）；
    /// `min_publish_interval_ms ∈ [250, publish_ms]`（下界 = 契约 `MIN_MERGE_WINDOW_MS` = **250**，
    /// 与 `deploy/deploy.md` §10.2 核对表同口径。**订正（K 收尾 Q-1）**：本行原写 `[200, …]`，比契约严、
    /// 且与 deploy 文档的同一句话孪生不一致，已按契约订正；**再订正（A-2，独立评审）**：下界由
    /// `100` 抬到 `250`，对齐设计 §4.2.1 **约束 2**「`min_publish_interval_ms ≥ 250 ms`」——
    /// 契约是唯一真源，设计 §4.9/§11.1 的 `[100, …]` 已就地标注「以契约 250 为准」。
    /// ⚠️ **连带效果（如实登记）**：`min ≥ 250 ∧ min ≤ publish_ms` ⇒ `publish_ms < 250` 的现场 yaml
    /// 会被**这条**拒绝（报 `min_publish_interval_ms`）——主拍**实际**须 ≥250）；`alarm_poll_ms` / `interlock_poll_ms` /
    /// `device_poll_ms` 上界；`alarm_page_size != 0`；`log.live_ring >= 100`；
    /// `range.*` 须为有限正数（禁 NaN/±Inf/0/负数），且 `phase_power_max_kw <= total_power_max_kw`、
    /// `inconsistency_threshold_kw <= pcs_total_rated_kw`。原手写实现**只查地址**，这些一律放行
    /// ⇒ 升级后现场 yaml 若不合规会**首次启动即失败**，迁移核对清单见 `deploy/deploy.md` §9.4。
    fn validate_display(&self) -> Result<(), String> {
        let d = &self.display;
        if !d.enabled {
            return Ok(());
        }
        // **唯一真源 = 契约的 `DisplayConfig::validate()`**（`display-proto/src/config.rs`）。
        //
        // ⚠️ **这条修复堵住一个 P0**（以下是**修复前**的状态，留档说明本函数为何改为转发）：
        // 原实现是**本文件手写**的 host/port 校验、且**只查 `bind_addr`（读通道）**——
        // `control_bind_addr`（**控制通道** = T-3 无登录的写通道）**从不校验**；而契约里
        // 两条都查的 `DisplayConfig::validate()` **当时全仓无调用点**（现由本函数调用，即下一行）。
        // 后果：把 `display.control_bind_addr` 配成 `0.0.0.0:9811` 时进程照样启动，并把
        // 写通道暴露到全网（违 PL-4 安全红线）。
        //
        // ⚠️ **口径收紧（同一修复的一部分）**：契约只认**字面量**回环 `127.0.0.1` / `::1`，
        // 而原实现把 `localhost` 也当回环 —— 名字可经 hosts 重映射到非回环地址，安全红线上
        // 不接受名字（渲染端 `console.rs` 同样只收字面量，两侧口径由此一致）。契约另有两条
        // 本函数原本没有的检查：**端口 ≠ 0**、**两条地址不得相同**。
        d.validate().map_err(|e| e.to_string())
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
  port: 9101
# ── 现场 legacy 段（单元 K：`CoreConfig` 已无 `web_api` 字段）──
# 它不是任何被建模的段，此处**故意保留**：证明"带 `web_api:` 的现场 yaml 仍可加载"
# （设计 §7.3 兼容性主张前半）。专项回归见 `legacy_web_api_section_still_loads_and_is_ignored`。
web_api:
  listen_addr: "0.0.0.0:9000"
ai_engine: {}
plugins: {}
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.version, "1.0");
        assert_eq!(config.system.log_level, "debug");
        assert_eq!(config.intercore.host, "192.168.1.1");
        // 单元 K：原断言是 `config.web_api.listen_addr == "0.0.0.0:9000"`（该字段已随 crate 删除）。
        // **替代断言**：改断 `intercore.port`——它替代的是"解析确实生效"，不是"某个 web-api 字段"。
        //
        // ⚠️ **本断言必须取非默认值**（第一轮整改 I-1）：`default_intercore_port()` 返回 9100，
        // 若此处夹具写 `port: 9100` 并断言 9100，则**解析完全失效时断言依然为真**（恒真网）。
        // 故夹具取 9101（≠ 默认 9100）⇒ 删掉夹具那一行 `port: 9101`（或把断言值改回 9100）
        // 本行即红，**可自行复现**（第二轮整改 ②：原文写"实测记录见本用例的破坏性验证"，
        // 而本文件里并无这样一份记录 ⇒ 指向不存在之物，改为可复现的操作说明）。
        assert_eq!(config.intercore.port, 9101);
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
        // 未配置 gateway 段时缺省 0.0.0.0:2404（审查 R2-A2：端口读 config 且向后兼容）
        assert_eq!(config.gateway.listen_addr, "0.0.0.0");
        assert_eq!(config.gateway.listen_port, 2404);
        // 未配置 mqtt_bridge 段时缺省双 false（审查 R2-B5：不 spawn，不再真连假域名）
        assert!(!config.mqtt_bridge.north_enabled);
        assert!(!config.mqtt_bridge.local_enabled);
    }

    /// **配置向后兼容 ①「能读」**（设计 §7.2 Step 4 末段 / §7.3 兼容性主张前半，单元 K）：
    /// 现场既有的、**带完整 `web_api:` 段**（含注释、含 `tls_cert`/`tls_key`）的 yaml
    /// **仍必须能加载并通过 `validate()`**——不强制运维在升级时立即改文件。
    ///
    /// 依据：`CoreConfig` **未**设 `deny_unknown_fields` ⇒ `web_api` 退为**未建模段**后被
    /// **忽略**（不是报错）。同时钉住"忽略 ≠ 影响其它段"：同一个 yaml 里的已建模段照常生效。
    ///
    /// **改什么会让本条变红**：给 `CoreConfig` 加 `#[serde(deny_unknown_fields)]`
    /// （现场 yaml 立刻启动失败）⇒ 第 1 条断言红。
    #[test]
    fn legacy_web_api_section_still_loads_and_is_ignored() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "192.168.1.1"   # 已建模段照常生效
  port: 9100
web_api:                  # 现场 legacy 段（单元 K 后不再是模型的一部分）
  listen_addr: "0.0.0.0:8080"
  enable_https: false
  tls_cert: null
  tls_key: null
ai_engine: {}
plugins: {}
"#;
        let cfg: CoreConfig = serde_yaml::from_str(yaml)
            .expect("带 `web_api:` 段的现场 yaml 必须仍可解析（未设 deny_unknown_fields）");
        cfg.validate()
            .expect("该 yaml 必须仍过 validate()（否则现场升级即启动失败）");
        // 已建模段照常生效（"忽略整段"不等于"忽略整个文件"）
        assert_eq!(cfg.intercore.port, 9100);
        assert_eq!(cfg.system.log_level, "info");
        // 边界登记（**事实**，不是主张）：该段确实**不在**模型里 ⇒ 整体序列化回写会丢它。
        // 正常保存路径不受影响（保留式编辑按文本行替换，见 `yaml_edit.rs`），
        // 端到端往返由 `config_service` / `yaml_edit` 的字节级用例钉死。
        let round = serde_yaml::to_string(&cfg).unwrap();
        // ⚠️ **订正 S-3（点明本断言证的边界）**：这条证的是**模型边界**——`web_api` 不是
        // `CoreConfig` 的字段 ⇒ **整体序列化回写**这座桥必然丢它。它**不**证、也**证不了**
        // "现场文件不被抹掉"：那取决于**保存路径**是否走保留式编辑。
        // "写了不丢"由 `config_service::legacy_web_api_section_survives_a_real_save_verbatim`
        // 负责（真实保存动作的**逐字节**往返）。两条断言合起来才是完整的兼容性主张。
        assert!(
            !round.contains("web_api"),
            "`web_api` 已不在模型内 ⇒ **整体回写**必然丢它（模型边界；「写了不丢」见 \
             config_service::legacy_web_api_section_survives_a_real_save_verbatim）"
        );
    }

    /// R2-B5: mqtt_bridge 段显式配置可解析（north/local_enabled 生效）
    #[test]
    fn test_mqtt_bridge_enabled_config() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
ai_engine: {}
plugins: {}
mqtt_bridge:
  north_enabled: true
  local_enabled: false
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.mqtt_bridge.north_enabled);
        assert!(!config.mqtt_bridge.local_enabled);
        assert!(config.validate().is_ok());
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
            gateway: GatewayConfig::default(),
            mqtt_bridge: MqttBridgeConfig::default(),
            display: DisplayConfig::default(),
            storage: StorageSectionConfig::default(),
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
            gateway: GatewayConfig::default(),
            mqtt_bridge: MqttBridgeConfig::default(),
            display: DisplayConfig::default(),
            storage: StorageSectionConfig::default(),
        };
        assert!(config.validate().is_err());
    }

    /// 12-显示终端 §7.3: display 段缺省 disabled（未配置 display 段部署行为不变，不启屏）
    #[test]
    fn test_display_disabled_by_default() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
ai_engine: {}
plugins: {}
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!config.display.enabled, "缺省 display 段 → disabled");
        assert_eq!(config.display.bind_addr, "127.0.0.1:9810");
        assert_eq!(config.display.publish_ms, 1000);
        assert!(config.validate().is_ok());
    }

    /// 12-显示终端 §7.3: display.enabled=true 显式可解析（bind_addr 回环）→ validate 通过
    #[test]
    fn test_display_enabled_loopback_passes() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
ai_engine: {}
plugins: {}
display:
  enabled: true
  bind_addr: "127.0.0.1:9810"
  publish_ms: 500
  range:
    current_max_a: 400
    phase_power_max_kw: 100
    total_power_max_kw: 300
    pcs_total_rated_kw: 60
    inconsistency_threshold_kw: 3.0
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.display.enabled);
        assert_eq!(config.display.publish_ms, 500);
        assert_eq!(config.display.range.current_max_a, 400.0);
        assert_eq!(config.display.range.total_power_max_kw, 300.0);
        assert!(
            config.validate().is_ok(),
            "回环 display 配置应通过: {:?}",
            config.validate()
        );
    }

    /// **I-2 回归（第一轮整改）：两份 deploy yaml 的 `display:` 段必须整体合规**。
    ///
    /// 依据设计 §7.3 的两行：`mupc/deploy/config/mupc_core_config.yaml` 与 `.production.yaml`
    /// 「删 `web_api:` 段；加 `display:` 的 `enabled/control_bind_addr/*_poll_ms/log:`」，
    /// 且「`.production.yaml` 同上 + **开启 `display.enabled: true`**」。
    /// 背景：`DisplayConfig.enabled` 缺省 **false** ⇒ 两份 yaml 原先都**没有** `display:` 段时，
    /// 直接按仓库 yaml 部署 = 读通道 / 控制通道 / `hmi_backend` 注册**全部不启动**（静默无 HMI）。
    ///
    /// 以**真文件**（`include_str!` ⇒ 与现场逐字节同源）为输入，逐份断言：
    /// ① 可解析为 `CoreConfig`；② 过 `CoreConfig::validate()`；③ **直接**过契约
    /// `DisplayConfig::validate()`；④ `enabled` 取值（仅 production 为 true）。
    ///
    /// ⚠️ **③ 是必需的第二道网**：非生产那份 `enabled: false` ⇒ `validate_display()` 会
    /// **整段早退跳过**，只靠 ② 就**验不到**它的非地址不变量（时延 / ring / 量程）。故此处
    /// 对两份都**真跑**契约校验，不看 `enabled`。
    ///
    /// **改什么会让本条变红**：把任一 `bind_addr` / `control_bind_addr` 改成 `"localhost:9810"`
    /// （名字不是字面量回环）、端口改成 `0`、两址写成同一个、`publish_ms` 压到 99、
    /// `log.live_ring` 压到 99、`alarm_page_size: 0`、`range.*` 写成 `.inf` /
    /// `phase_power_max_kw > total_power_max_kw` ⇒ ③ 红（实测记录：`localhost` 与端口 0 各红一次）；
    /// 把 `.production.yaml` 的 `enabled` 改回 `false` ⇒ ④ 红。
    #[test]
    fn deploy_configs_display_sections_are_contract_valid() {
        for (name, text, expect_enabled) in [
            (
                "mupc_core_config.yaml",
                include_str!("../../../deploy/config/mupc_core_config.yaml"),
                false,
            ),
            (
                "mupc_core_config.production.yaml",
                include_str!("../../../deploy/config/mupc_core_config.production.yaml"),
                true,
            ),
        ] {
            let cfg: CoreConfig = serde_yaml::from_str(text)
                .unwrap_or_else(|e| panic!("`{name}` 必须能解析为 CoreConfig: {e}"));
            assert!(
                cfg.validate().is_ok(),
                "`{name}` 必须过 CoreConfig::validate(): {:?}",
                cfg.validate()
            );
            assert_eq!(
                cfg.display.enabled, expect_enabled,
                "`{name}` 的 display.enabled（设计 §7.3：仅 production 开启）"
            );
            // ③ 契约**全集**校验：两份都真跑（不因 enabled=false 早退而漏验）
            assert!(
                cfg.display.validate().is_ok(),
                "`{name}` 的 display 段必须过契约 DisplayConfig::validate(): {:?}",
                cfg.display.validate()
            );
            // 地址取值本身（防"复制粘贴换名"式错配：两址须各占一端点、均为字面量回环）
            assert_eq!(cfg.display.bind_addr, "127.0.0.1:9810", "`{name}` 读通道端点");
            assert_eq!(
                cfg.display.control_bind_addr, "127.0.0.1:9811",
                "`{name}` 控制通道端点"
            );
        }
    }

    /// 12-显示终端 §7.3：回环**只认字面量** —— `127.0.0.1` / `::1`（含 `[::1]:port` 写法）
    /// 通过；`localhost`（名字）与缺端口（`"127.0.0.1"`）按契约**一律拒**。
    ///
    /// **沿革**：本用例原名 `..._host_forms_and_missing_port`，断言"缺端口单独报「缺少端口」、
    /// 各种写法都识别为回环"——那是**已废弃的旧口径**（O6 时期按 host 部分归一）。P0 修复后
    /// 校验真源收敛为契约 `DisplayConfig::validate()`：名字可经 hosts 重映射到非回环地址，
    /// 安全红线上不接受名字；缺端口也不再单独分类，统一报「非合法回环 host:port」。
    #[test]
    fn test_display_loopback_is_literal_only() {
        let with_addr = |addr: &str| {
            format!(
                r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
ai_engine: {{}}
plugins: {{}}
display:
  enabled: true
  bind_addr: "{addr}"
"#
            )
        };
        // 回环**字面量**写法 → 通过（`localhost` 已按契约收紧，见下）
        for ok in ["127.0.0.1:9810", "[::1]:9810"] {
            let config: CoreConfig = serde_yaml::from_str(&with_addr(ok)).unwrap();
            assert!(
                config.validate().is_ok(),
                "{ok} 应判回环通过，实际: {:?}",
                config.validate()
            );
        }
        // 非回环 / 非法 / **非字面量**写法 → 一律拒（错误文案来自契约的
        // `DisplayConfig::validate`，含 `PL-4` 安全红线说明）。
        //
        // ⚠️ `localhost` 与 `localhost:9810` 在此**由通过改为拒绝**（P0 修复的一部分）：
        // 原实现把它当回环，而契约只认**字面量** `127.0.0.1`/`::1` —— 名字可经 hosts
        // 重映射到非回环地址，安全红线上不该接受名字。渲染端 `console.rs` 同样只收字面量。
        for bad in [
            "localhost:9810",
            "localhost",
            "127.0.0.1",
            "127.0.0.1:abc",
            "127.0.0.1:0",
            "0.0.0.0:9810",
            "192.168.1.5:9810",
        ] {
            let config: CoreConfig = serde_yaml::from_str(&with_addr(bad)).unwrap();
            let err = config.validate().unwrap_err();
            assert!(
                err.contains("非合法回环") && err.contains("PL-4"),
                "{bad} 必须按契约判非法回环，实际: {err}"
            );
        }
    }

    /// **P0 网**：`display.control_bind_addr`（**控制通道** = 无登录的写通道）同样强制回环。
    ///
    /// **修复前**（以下为历史状态，现 `validate_display` 已转发契约校验）：`validate_display`
    /// **只查 `bind_addr`**（读通道），而契约里两条都查的 `DisplayConfig::validate()`
    /// **那时全仓无调用点** ⇒ 把控制面绑到 `0.0.0.0` 也照样启动，
    /// 等于把写通道（T-3 无鉴权）暴露到全网。
    ///
    /// **改什么会让本条变红**：把 `validate_display` 改回"只查 `bind_addr`"⇒
    /// 第 1 条（`0.0.0.0:9811`）与第 3 条（两址相同）都会被放行。
    #[test]
    fn display_control_bind_addr_is_forced_loopback_too() {
        let with_both = |read: &str, ctrl: &str| {
            format!(
                r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
ai_engine: {{}}
plugins: {{}}
display:
  enabled: true
  bind_addr: "{read}"
  control_bind_addr: "{ctrl}"
"#
            )
        };
        // ① 控制通道非回环 ⇒ 拒（就是这条修复的主要动因）
        let c: CoreConfig = serde_yaml::from_str(&with_both("127.0.0.1:9810", "0.0.0.0:9811")).unwrap();
        let e = c.validate().unwrap_err();
        assert!(
            e.contains("display.control_bind_addr") && e.contains("PL-4"),
            "控制通道非回环必须拒且点名该键，实际: {e}"
        );
        // ② 两条都回环但**同址** ⇒ 拒（同址会让后绑者启动失败）
        let c: CoreConfig =
            serde_yaml::from_str(&with_both("127.0.0.1:9810", "127.0.0.1:9810")).unwrap();
        let e = c.validate().unwrap_err();
        assert!(e.contains("不得相同"), "两址相同必须拒，实际: {e}");
        // ③ 两条都回环且不同 ⇒ 通过（防"一刀切拒绝"）
        let c: CoreConfig =
            serde_yaml::from_str(&with_both("127.0.0.1:9810", "127.0.0.1:9811")).unwrap();
        assert!(c.validate().is_ok(), "两条均回环且不同应通过: {:?}", c.validate());
    }

    /// **重要-4 网**：转发的 `DisplayConfig::validate()` 是**全集**校验 ⇒ 原先"只查地址"时
    /// 一律放行的**非地址**不变量，现在**同样门禁 `mupcd` 启动**（fail-fast）。
    ///
    /// 每条各取一个族代表：时延（`publish_ms`，**下界 50 与上界 4001 两侧都取**，B3-2d）、
    /// 环容量（`log.live_ring`）、告警页（`alarm_page_size`）、量程（`range.current_max_a`）、
    /// 量程交叉（`phase_power_max_kw > total_power_max_kw`）。**地址一律合法**
    /// （`127.0.0.1:9810/9811`），唯一非法项就是被测的那个字段 ⇒ 报错必须点名该键，
    /// 排除"其实是被地址判死的"。
    ///
    /// **改什么会让本条变红**：把 `validate_display` 换成"只查地址"的实现（P0 修复前的写法）
    /// ⇒ 下面循环里 6 条断言的 `unwrap_err()` 全部 panic（B3-2d 新增 `publish_ms: 4001`
    /// 一条后由 5 条增至 6 条）。
    #[test]
    fn display_non_address_invariants_now_gate_startup() {
        // 全部合规的基准（address 合法 + 各非地址不变量合规）
        let with_display = |extra: &str| {
            format!(
                r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
ai_engine: {{}}
plugins: {{}}
display:
  enabled: true
  bind_addr: "127.0.0.1:9810"
  control_bind_addr: "127.0.0.1:9811"
{extra}
"#
            )
        };
        // 基准自证：不带 extra 时必须通过（否则下面的红分不清是"改坏了"还是"本来就不合规"）
        let base: CoreConfig = serde_yaml::from_str(&with_display("")).unwrap();
        assert!(base.validate().is_ok(), "基准配置应通过: {:?}", base.validate());

        for (key, extra) in [
            ("display.publish_ms", "  publish_ms: 50"),
            // **B3-2d**：同键的**上界**一侧（4001 > 契约 `MAX_PUBLISH_MS`）——转发网须同样门禁
            // 启动。`min_publish_interval_ms` 同步抬到 4001 以免被合并窗口那条先拒（否则本行
            // 就成了"其实是被 min 判死的"假绿）。
            (
                "display.publish_ms",
                "  publish_ms: 4001\n  min_publish_interval_ms: 4001",
            ),
            (
                "display.log.live_ring",
                "  log:\n    live_ring: 50",
            ),
            ("display.alarm_page_size", "  alarm_page_size: 0"),
            (
                "display.range.current_max_a",
                "  range:\n    current_max_a: 0\n    phase_power_max_kw: 100\n    \
                 total_power_max_kw: 300\n    pcs_total_rated_kw: 60\n    \
                 inconsistency_threshold_kw: 3.0",
            ),
            (
                "display.range.total_power_max_kw",
                "  range:\n    current_max_a: 300\n    phase_power_max_kw: 100\n    \
                 total_power_max_kw: 50\n    pcs_total_rated_kw: 60\n    \
                 inconsistency_threshold_kw: 3.0",
            ),
        ] {
            let c: CoreConfig = serde_yaml::from_str(&with_display(extra)).unwrap();
            let e = c.validate().unwrap_err();
            assert!(
                e.contains(key),
                "非地址不变量 `{key}` 必须在启动期被门禁且点名该键，实际: {e}"
            );
        }
    }

    /// 12-显示终端 §7.3: display.enabled 时 bind_addr 非回环 → validate Err（强制仅 127.0.0.1）
    #[test]
    fn test_display_enabled_non_loopback_rejected() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
ai_engine: {}
plugins: {}
display:
  enabled: true
  bind_addr: "0.0.0.0:9810"
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        let err = config.validate().unwrap_err();
        assert!(
            err.contains("回环"),
            "非回环 bind_addr 应报错（强制仅 127.0.0.1），实际: {}",
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
ai_engine: {}
plugins: {}
south_stations:
  poll_ms: 1000
  stale_timeout_s: 5
  stations:
    - id: meter_grid
      role: meter_grid
      port: /dev/ttyS1
      slave: 1
      interval_ms: 1000
      regs:                    # S3b-1c: meter_grid 须配完整相量块（p/q/pf/u/i），addr 非 0 不重叠
        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }
    - { id: meter_batt, role: meter_batt, port: /dev/ttyS1, slave: 2, interval_ms: 1000 }
    - { id: battery_1, role: battery, port: /dev/ttyS2, slave: 1, interval_ms: 1000, regs: [{ name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }] }
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
ai_engine: {}
plugins: {}
south_stations:
  stations:
    - { id: battery_1, role: battery, port: "ttyS0", slave: 1, interval_ms: 1000, regs: [{ name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }] }
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
ai_engine: {}
plugins: {}
south_stations:
  stations:
    - { id: battery_1, role: battery, port: "/dev/ttyS0", slave: 1, interval_ms: 1000, regs: [{ name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }] }
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
ai_engine: {}
plugins: {}
south_stations:
  stations:
    - id: meter_grid
      role: meter_grid
      port: /dev/ttyS1
      slave: 1
      interval_ms: 1000
      regs:                    # S3b-1c: meter_grid 须配完整相量块（p/q/pf/u/i），addr 非 0 不重叠
        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }
    - { id: battery_1, role: battery, port: /dev/ttyS2, slave: 1, interval_ms: 1000, regs: [{ name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }] }
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
ai_engine: {}
plugins: {}
south_stations:
  stations:
    - { id: hvac_1, role: hvac, port: "ttyS3", slave: 3, interval_ms: 2000 }
    - { id: battery_1, role: battery, port: "/dev/ttyS3", slave: 1, interval_ms: 1000, regs: [{ name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }] }
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
ai_engine: {}
plugins: {}
south_stations:
  stations:
    - { id: hvac_1, role: hvac, port: "ttyS3", slave: 3, interval_ms: 2000 }
    - { id: battery_1, role: battery, port: "ttyS3", slave: 1, interval_ms: 1000, regs: [{ name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }] }
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(
            config.validate().is_ok(),
            "同物理口同拼写（合法同口多从站）应通过: {:?}",
            config.validate()
        );
    }

    // ── 03 设计 §9.2 / PRD §11.3（U-67）：`storage:` 段 ──

    /// **STG-01**：`storage:` 段**整段缺省** ⇒ 三值 = 变更前的现实现常量（零行为变化）。
    ///
    /// 改什么会让本条变红：把 `Default` 的任一值改掉（如 `batch_capacity` 默认改 2000）⇒ ① 红。
    #[test]
    fn stg01_storage_section_defaults_equal_pre_change_constants() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
ai_engine: {}
plugins: {}
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.storage.batch_capacity, 1000, "= 变更前 startup.rs 的硬编码");
        assert_eq!(config.storage.flush_interval_ms, 5000);
        assert_eq!(config.storage.grid_aggregate_period_ms, 60_000, "§4.1.1 默认 1 分钟");
        assert!(config.validate().is_ok());

        // 显式空段 / 部分键：其余键同样落到默认（serde 容器级 default）
        let yaml_empty = "version: \"1.0\"\nsystem: {log_level: info}\nintercore: {host: h, port: 9100}\nai_engine: {}\nplugins: {}\nstorage: {}\n";
        let c2: CoreConfig = serde_yaml::from_str(yaml_empty).unwrap();
        assert_eq!(c2.storage.batch_capacity, 1000);
        assert_eq!(c2.storage.flush_interval_ms, 5000);
        assert_eq!(c2.storage.grid_aggregate_period_ms, 60_000);
        let yaml_partial = "version: \"1.0\"\nsystem: {log_level: info}\nintercore: {host: h, port: 9100}\nai_engine: {}\nplugins: {}\nstorage: {batch_capacity: 500}\n";
        let c3: CoreConfig = serde_yaml::from_str(yaml_partial).unwrap();
        assert_eq!(c3.storage.batch_capacity, 500);
        assert_eq!(c3.storage.flush_interval_ms, 5000, "未写的键取默认");
        assert_eq!(c3.storage.grid_aggregate_period_ms, 60_000);
        assert!(c3.validate().is_ok());
    }

    /// **STG-01（真文件）**：两份部署 yaml 的 `storage:` 段**显式写出**且取值 = 默认
    /// （§9.6 序 2：注释态或显式默认值等效，本仓选显式写出以便现场可见）。
    #[test]
    fn stg01_deploy_configs_storage_section_is_present_and_is_the_default() {
        for (name, text) in [
            (
                "mupc_core_config.yaml",
                include_str!("../../../deploy/config/mupc_core_config.yaml"),
            ),
            (
                "mupc_core_config.production.yaml",
                include_str!("../../../deploy/config/mupc_core_config.production.yaml"),
            ),
        ] {
            let cfg: CoreConfig = serde_yaml::from_str(text)
                .unwrap_or_else(|e| panic!("`{name}` 必须能解析为 CoreConfig: {e}"));
            assert_eq!(cfg.storage.batch_capacity, 1000, "`{name}` 的 storage.batch_capacity");
            assert_eq!(cfg.storage.flush_interval_ms, 5000, "`{name}` 的 storage.flush_interval_ms");
            assert_eq!(
                cfg.storage.grid_aggregate_period_ms, 60_000,
                "`{name}` 的 storage.grid_aggregate_period_ms"
            );
            assert!(
                text.contains("\nstorage:"),
                "`{name}` 必须**显式**写出 storage: 段（现场可见；§9.6 序 2）"
            );
            assert!(cfg.validate().is_ok(), "`{name}` 的 storage 段必须过 validate_storage");
        }
    }

    /// **STG-02 / STG-03**：显式配置生效（装配点是**启动期读一次** ⇒ 「重启生效」由
    /// 「值从 YAML 来、不来自硬编码」体现；热改 YAML 不生效属预期，见 §9.2.1「不进 DB」）。
    #[test]
    fn stg02_stg03_explicit_storage_values_are_taken() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
ai_engine: {}
plugins: {}
storage:
  batch_capacity: 200
  flush_interval_ms: 2000
  grid_aggregate_period_ms: 120000
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.storage.batch_capacity, 200);
        assert_eq!(config.storage.flush_interval_ms, 2000);
        assert_eq!(config.storage.grid_aggregate_period_ms, 120_000);
        assert!(config.validate().is_ok());
        // 序列化往返：三键都在（供「保留式编辑不可定位」时的整体回退路径）
        let back: StorageSectionConfig =
            serde_yaml::from_str(&serde_yaml::to_string(&config.storage).unwrap()).unwrap();
        assert_eq!(back.batch_capacity, 200);
        assert_eq!(back.flush_interval_ms, 2000);
        assert_eq!(back.grid_aggregate_period_ms, 120_000);
    }

    /// **STG-04**：非法值**拒启动**且错误信息**点名违规键**（逐字段边界，含 `% 1000 != 0`）。
    #[test]
    fn stg04_storage_invalid_values_are_rejected_by_name() {
        fn with(apply: impl FnOnce(&mut StorageSectionConfig)) -> CoreConfig {
            let yaml = "version: \"1.0\"\nsystem: {log_level: info}\nintercore: {host: h, port: 9100}\nai_engine: {}\nplugins: {}\n";
            let mut c: CoreConfig = serde_yaml::from_str(yaml).unwrap();
            apply(&mut c.storage);
            c
        }
        // batch_capacity：0 / 100001 拒；1 / 100000 过
        let err = with(|s| s.batch_capacity = 0).validate().unwrap_err();
        assert!(err.contains("storage.batch_capacity"), "错误必须点名键: {err}");
        let err = with(|s| s.batch_capacity = 100_001).validate().unwrap_err();
        assert!(err.contains("storage.batch_capacity"));
        assert!(with(|s| s.batch_capacity = 1).validate().is_ok());
        assert!(with(|s| s.batch_capacity = 100_000).validate().is_ok());
        // flush_interval_ms：禁 0；99 / 600001 拒；100 / 600000 过
        let err = with(|s| s.flush_interval_ms = 0).validate().unwrap_err();
        assert!(err.contains("storage.flush_interval_ms"), "错误必须点名键: {err}");
        let err = with(|s| s.flush_interval_ms = 99).validate().unwrap_err();
        assert!(err.contains("storage.flush_interval_ms"));
        let err = with(|s| s.flush_interval_ms = 600_001).validate().unwrap_err();
        assert!(err.contains("storage.flush_interval_ms"));
        assert!(with(|s| s.flush_interval_ms = 100).validate().is_ok());
        assert!(with(|s| s.flush_interval_ms = 600_000).validate().is_ok());
        // grid_aggregate_period_ms：9999 / 3600001 拒；10000 / 3600000 过
        let err = with(|s| s.grid_aggregate_period_ms = 9_999).validate().unwrap_err();
        assert!(err.contains("storage.grid_aggregate_period_ms"));
        let err = with(|s| s.grid_aggregate_period_ms = 3_600_001)
            .validate()
            .unwrap_err();
        assert!(err.contains("storage.grid_aggregate_period_ms"));
        assert!(with(|s| s.grid_aggregate_period_ms = 10_000).validate().is_ok());
        assert!(with(|s| s.grid_aggregate_period_ms = 3_600_000).validate().is_ok());
        // 须为 1000 的整数倍（时间戳按整秒对齐）：60001 拒、61000 过
        let err = with(|s| s.grid_aggregate_period_ms = 60_001).validate().unwrap_err();
        assert!(
            err.contains("storage.grid_aggregate_period_ms") && err.contains("1000"),
            "整倍约束的错误文案须点名键 + 说明整倍: {err}"
        );
        let err = with(|s| s.grid_aggregate_period_ms = 60_500).validate().unwrap_err();
        assert!(err.contains("storage.grid_aggregate_period_ms"));
        assert!(with(|s| s.grid_aggregate_period_ms = 15_000).validate().is_ok());
    }

    /// **GRD-08**：`storage:` 段**无任何「关闭」语义**（PRD R-11.1-A / R-11.3-A：本段**只含三键**）。
    ///
    /// 两条判据：① 结构 —— 序列化只出这三个键（加不了 `enabled: false` 一类开关）；
    /// ② 行为 —— 现场 yaml 里硬塞 `enabled: false` 这类未建模键**既不报错也不改变三值**
    /// （`CoreConfig` 无 `deny_unknown_fields`）⇒ 不存在「配错即静默不落库」的合法路径。
    #[test]
    fn grd08_storage_section_has_no_disable_switch() {
        // ① 结构：恰好三键
        let yaml = serde_yaml::to_string(&StorageSectionConfig::default()).unwrap();
        let keys: Vec<&str> = yaml
            .lines()
            .filter_map(|l| l.split_once(':').map(|(k, _)| k.trim()))
            .collect();
        assert_eq!(
            keys,
            vec!["batch_capacity", "flush_interval_ms", "grid_aggregate_period_ms"],
            "storage 段只含三键（不含 enabled / 保留期 / max_retained_points）"
        );
        // ② 行为：塞未建模键不生效（三值仍默认、validate 仍过）
        let yaml_off = r#"
version: "1.0"
system: { log_level: "info" }
intercore: { host: "127.0.0.1", port: 9100 }
ai_engine: {}
plugins: {}
storage:
  enabled: false
  aggregate_enabled: false
"#;
        let c: CoreConfig = serde_yaml::from_str(yaml_off).unwrap();
        assert_eq!(c.storage.batch_capacity, 1000);
        assert_eq!(c.storage.flush_interval_ms, 5000);
        assert_eq!(c.storage.grid_aggregate_period_ms, 60_000);
        assert!(c.validate().is_ok(), "未建模键被忽略而非报错（与 CoreConfig 既有兼容性口径一致）");
    }
}
