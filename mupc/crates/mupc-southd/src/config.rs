//! `south_stations:` 配置段类型（core_config 嵌入用；§10.3）。
//!
//! 定义站级南向统一调度的配置结构：轮询周期、新鲜度门限、站（role/port/slave）
//! 与每站寄存器块。校验仅限段内（跨段/互斥在 core-bin validate——Task 6）。

use mupc_data_processing::meter_regs::RegFormat;
use serde::Deserialize;

pub const DEFAULT_POLL_MS: u64 = 1000;
pub const DEFAULT_STALE_TIMEOUT_S: u64 = 5;
pub const DEFAULT_INTERVAL_MS: u64 = 1000;
/// 策略 5s 数据新鲜度共享常量落点（M-6）：单一真源在 data-processing
/// （`mupc_data_processing::DATA_FRESHNESS_MS`），此处别名引用避免双定义漂移。
pub const DATA_FRESHNESS_MS: u64 = mupc_data_processing::DATA_FRESHNESS_MS;

/// 站类型角色（南向调度语义划分，YAML 用 snake_case）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// 并网点主表（策略 phase 真源）
    MeterGrid,
    /// 电池侧电表（能量流核算）
    MeterBatt,
    /// 电池簇 BMS 采集
    Battery,
    /// 温控/空调（消防/热管理联动）
    Hvac,
    /// 消防子系统（联动/联锁）
    Fire,
}

/// 单站配置（role + 端口 + 从站地址 + 采集间隔 + 寄存器块）
#[derive(Debug, Clone, Deserialize)]
pub struct StationConf {
    pub id: String,
    pub role: Role,
    pub port: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default)]
    pub slave: u8,
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
    #[serde(default)]
    pub regs: Vec<RegBlockConf>,
}

/// 寄存器块配置（一段起始地址 + 数值格式 + 块长度）
#[derive(Debug, Clone, Deserialize)]
pub struct RegBlockConf {
    pub name: String,
    pub addr: u16,
    /// **注意：RegFormat 无 Default**，故用 `#[serde(default = "default_reg_format")]`
    /// （serde(default) 需要 Default 实现，此处不可用）。
    #[serde(default = "default_reg_format")]
    pub format: RegFormat,
    #[serde(default)]
    pub scale: f64,
    #[serde(default = "default_reg_count")]
    pub count: u16,
}

/// 顶层配置段：轮询周期、数据过期门限与站表
#[derive(Debug, Clone, Deserialize)]
pub struct SouthStationsConfig {
    #[serde(default = "default_poll_ms")]
    pub poll_ms: u64,
    #[serde(default = "default_stale_timeout_s")]
    pub stale_timeout_s: u64,
    #[serde(default)]
    pub stations: Vec<StationConf>,
}

impl Default for SouthStationsConfig {
    fn default() -> Self {
        Self {
            poll_ms: DEFAULT_POLL_MS,
            stale_timeout_s: DEFAULT_STALE_TIMEOUT_S,
            stations: Vec::new(),
        }
    }
}

impl SouthStationsConfig {
    /// 段内校验（跨段/互斥在 core-bin validate——Task 6）：id 唯一非空；
    /// meter_grid 至多一站（AiIntegrator 单写方约束，grid_station 取唯一）；
    /// port 非空；slave 1..=247；interval_ms>0；meter_grid interval_ms < DATA_FRESHNESS_MS。
    pub fn validate(&self) -> Result<(), String> {
        let mut ids: Vec<&str> = Vec::new();
        let mut grid_seen = false;
        for s in &self.stations {
            if s.id.trim().is_empty() {
                return Err("south_stations: station id 为空".into());
            }
            if ids.contains(&s.id.as_str()) {
                return Err(format!("south_stations: 站 id 重复: {}", s.id));
            }
            ids.push(s.id.as_str());
            if s.role == Role::MeterGrid {
                if grid_seen {
                    return Err("south_stations: 至多一个 meter_grid 站（AiIntegrator 单写方约束）".into());
                }
                grid_seen = true;
            }
            if s.port.trim().is_empty() {
                return Err(format!("south_stations: 站 {} port 为空", s.id));
            }
            if !(1..=247).contains(&s.slave) {
                return Err(format!(
                    "south_stations: 站 {} slave 越界: {}",
                    s.id, s.slave
                ));
            }
            if s.interval_ms == 0 {
                return Err(format!(
                    "south_stations: 站 {} interval_ms 须 > 0",
                    s.id
                ));
            }
            if s.role == Role::MeterGrid && s.interval_ms >= DATA_FRESHNESS_MS {
                return Err(format!(
                    "south_stations: meter_grid 站 {} interval_ms 须 < {}ms",
                    s.id, DATA_FRESHNESS_MS
                ));
            }
        }
        Ok(())
    }

    /// role=grid 站（策略 phase 真源；迁移期与 master_meter 互斥）。
    pub fn grid_station(&self) -> Option<&StationConf> {
        self.stations
            .iter()
            .find(|s| s.role == Role::MeterGrid)
    }
}

fn default_poll_ms() -> u64 {
    DEFAULT_POLL_MS
}
fn default_stale_timeout_s() -> u64 {
    DEFAULT_STALE_TIMEOUT_S
}
fn default_protocol() -> String {
    "modbus".into()
}
fn default_interval_ms() -> u64 {
    DEFAULT_INTERVAL_MS
}
fn default_reg_count() -> u16 {
    2
}
fn default_reg_format() -> RegFormat {
    RegFormat::Float32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// YAML 反序列化外层键容器（模拟 core_config 嵌入为结构体字段）
    #[derive(Deserialize)]
    struct Wrapper {
        south_stations: SouthStationsConfig,
    }

    /// 合法 5 站示例 YAML（role 走 snake_case）
    const VALID_5_STATION_YAML: &str = r#"
south_stations:
  poll_ms: 1000
  stale_timeout_s: 5
  stations:
    - id: meter_grid
      role: meter_grid
      port: /dev/ttyS1
      slave: 1
      interval_ms: 1000
      regs:
        - { name: p, addr: 0, format: float32, count: 6 }
        - { name: p_total, addr: 6, format: float32, count: 2 }
        - { name: q, addr: 8, format: float32, count: 6 }
        - { name: pf, addr: 14, format: float32, count: 6 }
        - { name: u, addr: 20, format: float32, count: 6 }
        - { name: i, addr: 26, format: float32, count: 6 }
    - id: meter_batt
      role: meter_batt
      port: /dev/ttyS1
      slave: 2
      interval_ms: 1000
      regs: [{ name: p, addr: 0, format: float32, count: 2 }]
    - id: battery_1
      role: battery
      port: /dev/ttyS2
      slave: 1
      interval_ms: 1000
      regs: [{ name: soc, addr: 100, format: int32_scaled, scale: 0.1, count: 2 }]
    - id: hvac_1
      role: hvac
      port: /dev/ttyS3
      slave: 3
      interval_ms: 2000
      regs: [{ name: temp, addr: 0, format: int32_scaled, scale: 0.01, count: 2 }]
    - id: fire_1
      role: fire
      port: /dev/ttyS4
      slave: 1
      interval_ms: 2000
      regs: [{ name: alarm, addr: 0, format: int32_scaled, scale: 1.0, count: 2 }]
"#;

    #[test]
    fn parses_valid_5_station_yaml() {
        let w: Wrapper = serde_yaml::from_str(VALID_5_STATION_YAML).expect("解析失败");
        let cfg = w.south_stations;
        assert_eq!(cfg.poll_ms, 1000);
        assert_eq!(cfg.stale_timeout_s, 5);
        assert_eq!(cfg.stations.len(), 5);

        let grid = &cfg.stations[0];
        assert_eq!(grid.id, "meter_grid");
        assert_eq!(grid.role, Role::MeterGrid);
        assert_eq!(grid.slave, 1);
        assert_eq!(grid.interval_ms, 1000);
        assert_eq!(grid.port, "/dev/ttyS1");

        assert_eq!(cfg.stations[1].role, Role::MeterBatt);
        assert_eq!(cfg.stations[1].slave, 2);
        assert_eq!(cfg.stations[2].role, Role::Battery);
        assert_eq!(cfg.stations[3].role, Role::Hvac);
        assert_eq!(cfg.stations[3].interval_ms, 2000);
        assert_eq!(cfg.stations[4].role, Role::Fire);
    }

    #[test]
    fn meter_grid_regs_contain_pq_pfu_i_p_total() {
        let w: Wrapper = serde_yaml::from_str(VALID_5_STATION_YAML).expect("解析失败");
        let grid = &w.south_stations.stations[0];

        let names: Vec<&str> = grid.regs.iter().map(|b| b.name.as_str()).collect();
        for expect in ["p", "q", "pf", "u", "i", "p_total"] {
            assert!(names.contains(&expect), "缺寄存器块: {expect}");
        }

        // 相量块 count=6（float32），标量 p_total count=2
        let p = grid.regs.iter().find(|b| b.name == "p").unwrap();
        assert_eq!(p.format, RegFormat::Float32);
        assert_eq!(p.count, 6);
        let p_total = grid.regs.iter().find(|b| b.name == "p_total").unwrap();
        assert_eq!(p_total.format, RegFormat::Float32);
        assert_eq!(p_total.count, 2);
    }

    #[test]
    fn validate_passes_for_valid_config() {
        let w: Wrapper = serde_yaml::from_str(VALID_5_STATION_YAML).expect("解析失败");
        assert!(w.south_stations.validate().is_ok());
        assert!(w.south_stations.grid_station().is_some());
        assert_eq!(w.south_stations.grid_station().unwrap().id, "meter_grid");
    }

    #[test]
    fn validate_rejects_duplicate_id() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 1 }
    - { id: a, role: hvac, port: t2, slave: 2 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn validate_rejects_multiple_meter_grid() {
        let yaml = r#"
south_stations:
  stations:
    - { id: mg1, role: meter_grid, port: t1, slave: 1, interval_ms: 1000 }
    - { id: mg2, role: meter_grid, port: t2, slave: 2, interval_ms: 1000 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(
            w.south_stations.validate().is_err(),
            "两个 meter_grid 站应被拒绝（AiIntegrator 单写方）"
        );
    }

    #[test]
    fn validate_rejects_empty_id() {
        let yaml = r#"
south_stations:
  stations:
    - { id: "", role: battery, port: t1, slave: 1 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty_port() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: "", slave: 1 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn validate_accepts_max_slave() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 247 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(
            w.south_stations.validate().is_ok(),
            "slave=247 为合法上界，应通过"
        );
    }

    #[test]
    fn validate_meter_grid_interval_boundary() {
        // 钉死 >= 边界语义：4999(<5000) 合法，5000(==DATA_FRESHNESS_MS) 拒绝
        for (iv, ok) in [(4999u64, true), (5000u64, false)] {
            let yaml = format!(
                "south_stations:\n  stations:\n    - {{ id: mg, role: meter_grid, port: t1, slave: 1, interval_ms: {iv} }}"
            );
            let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
            assert_eq!(
                w.south_stations.validate().is_ok(),
                ok,
                "meter_grid interval_ms={iv} 期望 ok={ok}"
            );
        }
    }

    #[test]
    fn validate_rejects_slave_out_of_range() {
        for bad in [0u16, 248u16] {
            let yaml = format!(
                "south_stations:\n  stations:\n    - {{ id: a, role: battery, port: t1, slave: {bad} }}"
            );
            let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
            assert!(
                w.south_stations.validate().is_err(),
                "slave={bad} 应被拒绝"
            );
        }
    }

    #[test]
    fn validate_rejects_zero_interval() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 1, interval_ms: 0 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn validate_rejects_meter_grid_slow_poll() {
        let yaml = r#"
south_stations:
  stations:
    - { id: mg, role: meter_grid, port: t1, slave: 1, interval_ms: 6000 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn default_field_fallbacks_apply() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        let st = &w.south_stations.stations[0];
        // 缺省项：protocol/slave/interval/regs/reg format
        assert_eq!(st.protocol, "modbus");
        assert_eq!(st.slave, 0);
        assert_eq!(st.interval_ms, DEFAULT_INTERVAL_MS);
        assert!(st.regs.is_empty());
    }

    #[test]
    fn default_reg_format_used_when_omitted() {
        let yaml = r#"
south_stations:
  stations:
    - id: a
      role: battery
      port: t1
      slave: 1
      regs:
        - { name: soc, addr: 100 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        let blk = &w.south_stations.stations[0].regs[0];
        assert_eq!(blk.format, RegFormat::Float32);
        assert_eq!(blk.scale, 0.0);
        assert_eq!(blk.count, 2);
    }
}
