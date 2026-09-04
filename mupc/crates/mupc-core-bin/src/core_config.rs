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
    /// 串口设备，默认 /dev/ttyS1
    #[serde(default = "default_serial_port")]
    pub serial_port: String,
    /// 波特率，默认 9600
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

/// 台区总表分相数据源配置（U-26）
///
/// 总表以 RS485 Modbus 保持寄存器暴露分相量；各量寄存器块定义见
/// [`MasterMeterRegMap`]。寄存器地址为现场点表占位，默认值仅示例。
#[derive(Debug, Clone, Deserialize)]
pub struct MasterMeterConfig {
    /// 是否启用总表采集（默认关；启用须配真点表）
    #[serde(default)]
    pub enabled: bool,
    /// 串口设备，默认 /dev/ttyUSB0
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

fn default_meter_serial() -> String {
    "/dev/ttyUSB0".to_string()
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

fn default_serial_port() -> String {
    "/dev/ttyS1".to_string()
}

fn default_baud_rate() -> u32 {
    9600
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
        // P1-4/P2-2: 台区总表启用时校验现场前提（独立串口/从站）与寄存器映射有效性
        if self.master_meter.enabled {
            self.validate_master_meter()?;
        }
        Ok(())
    }

    /// P1-4/P2-2: 校验台区总表配置（enabled 时）：
    /// serial_port 非空、slave_addr∈1..=247、与南向 RS485 默认串口分离（总线仲裁未实现）、
    /// reg_map 各量地址非 0 且 6 寄存器区间互不重叠。
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
        // 南向 RS485 串口当前不可配、默认 /dev/ttyUSB0；总表须独立串口或需总线仲裁（未实现）
        if mm.serial_port == "/dev/ttyUSB0" {
            return Err(
                "master_meter.serial_port 与南向 RS485 默认串口 /dev/ttyUSB0 相同——台区总表须独立于南向 RS485 串口或需总线仲裁（未实现）"
                    .to_string(),
            );
        }
        Self::validate_reg_map(&mm.reg_map)
    }

    /// P2-2: 分相量块 p/q/pf/u/i 起始地址非 0 且三相连续 6 寄存器区间互不重叠。
    fn validate_reg_map(reg_map: &MasterMeterRegMap) -> Result<(), String> {
        let blocks: [(&str, &MeterRegBlock); 5] = [
            ("p", &reg_map.p),
            ("q", &reg_map.q),
            ("pf", &reg_map.pf),
            ("u", &reg_map.u),
            ("i", &reg_map.i),
        ];
        for (name, b) in blocks.iter() {
            if b.addr == 0 {
                return Err(format!("master_meter.reg_map.{} addr 不能为 0", name));
            }
        }
        for (i, (name_i, b_i)) in blocks.iter().enumerate() {
            for (name_j, b_j) in blocks.iter().skip(i + 1) {
                let a0 = b_i.addr as u32;
                let b0 = b_j.addr as u32;
                // 半开区间 [addr, addr+6)：三相各占 2 寄存器
                if a0 < b0 + 6 && b0 < a0 + 6 {
                    return Err(format!(
                        "master_meter.reg_map.{} 与 {} 寄存器区间重叠（各量三相连续 6 寄存器不得交叠）",
                        name_i, name_j
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
        assert_eq!(config.intercore.modbus_rtu.serial_port, "/dev/ttyS1");
        assert_eq!(config.intercore.modbus_rtu.baud_rate, 9600);
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
            master_meter: MasterMeterConfig::default(),
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
}
