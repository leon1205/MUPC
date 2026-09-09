//! `south_stations:` 配置段类型（core_config 嵌入用；§10.3）。
//!
//! 定义站级南向统一调度的配置结构：轮询周期、新鲜度门限、站（role/port/slave）
//! 与每站寄存器块。校验仅限段内（跨段/互斥在 core-bin validate——Task 6）。

use mupc_data_processing::meter_regs::RegFormat;
use serde::Deserialize;

pub const DEFAULT_POLL_MS: u64 = 1000;
pub const DEFAULT_STALE_TIMEOUT_S: u64 = 5;
pub const DEFAULT_INTERVAL_MS: u64 = 1000;
pub const DEFAULT_BAUD_RATE: u32 = 9600;
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
    /// 口波特率（同口各站必须一致——物理共享口波特率；缺省 9600）
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
    #[serde(default)]
    pub regs: Vec<RegBlockConf>,
}

/// 寄存器块读取功能码（YAML: `holding` / `input`）。默认 FC03 保持寄存器；
/// FC04 输入寄存器供厂方点表用 input regs 的设备（解码同构，读回同格式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegFunc {
    /// 保持寄存器（FC0x03，默认）
    Holding,
    /// 输入寄存器（FC0x04）
    Input,
}

/// 寄存器块配置（一段起始地址 + 数值格式 + 块长度）
#[derive(Debug, Clone, Deserialize)]
pub struct RegBlockConf {
    pub name: String,
    pub addr: u16,
    /// 功能码（FC03 保持 / FC04 输入；缺省 FC03）
    #[serde(default = "default_reg_func")]
    pub func: RegFunc,
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
    /// battery 至多一站（BMS SOC 单源约束，AiIntegrator bms_soc 单槽——多站抢写最后写入者胜）；
    /// port 非空；slave 1..=247；interval_ms>0；baud_rate 1..=4000000；
    /// meter_grid/battery interval_ms < DATA_FRESHNESS_MS（BMS SOC fresh 窗口 5s）；
    /// meter_grid regs 完整性（缺相量块 p/q/pf/u/i、addr>0、区间不重叠、count≥6、块名唯一）；
    /// 同口 baud 一致（见下）。
    pub fn validate(&self) -> Result<(), String> {
        let mut ids: Vec<&str> = Vec::new();
        let mut grid_seen = false;
        let mut battery_seen = false;
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
            if s.role == Role::Battery {
                if battery_seen {
                    return Err(
                        "south_stations: 至多一个 battery 站（BMS SOC 单源约束，AiIntegrator bms_soc 单槽）"
                            .into(),
                    );
                }
                battery_seen = true;
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
            // 口波特率下界/上界：0 会静默穿透同口一致性检查（整口皆 0 时放行），
            // 到 open 才报错；>4_000_000 亦为异常值，一并在此拒。
            if s.baud_rate == 0 || s.baud_rate > 4_000_000 {
                return Err(format!(
                    "south_stations: 站 {} baud_rate 越界: {}（须 1..=4000000）",
                    s.id, s.baud_rate
                ));
            }
            if s.role == Role::MeterGrid && s.interval_ms >= DATA_FRESHNESS_MS {
                return Err(format!(
                    "south_stations: meter_grid 站 {} interval_ms 须 < {}ms",
                    s.id, DATA_FRESHNESS_MS
                ));
            }
            if s.role == Role::Battery && s.interval_ms >= DATA_FRESHNESS_MS {
                return Err(format!(
                    "south_stations: battery 站 {} interval_ms 须 < {}ms（BMS SOC 新鲜度窗口 5s，防源周期翻转）",
                    s.id, DATA_FRESHNESS_MS
                ));
            }
            // S3b-1c（承接已删 core_config validate_reg_map P2-2/N2）：meter_grid 是总表 phase
            // 真源——regs 块须完整（相量块 p/q/pf/u/i 各须存在）、addr>0、半开区间不重叠，
            // 防配置 typo（addr 重叠/addr=0）静默读到错寄存器喂策略。
            if s.role == Role::MeterGrid {
                // 相量块语义（mapper 按 name 找块，缺失/读失败 → Failed→offline 是运行期；
                // 此处配置期拦截缺失与地址错误）。空 regs 亦落入缺 p 分支被拒。
                for required in ["p", "q", "pf", "u", "i"] {
                    if !s.regs.iter().any(|b| b.name == required) {
                        return Err(format!(
                            "south_stations: meter_grid 站 {} regs 缺相量块 {}（总表 phase 真源须 p/q/pf/u/i）",
                            s.id, required
                        ));
                    }
                }
                // count 语义：相量块须 count>=6（3 相×2 寄存器三相连续，mapper decode_phase_block
                // 硬性 ≥6）；p_total（可选）count>=2。count 配错（漏配→default 2，或 4）时
                // name/addr/重叠都过、启动绿灯，但 scheduler 每轮只读 blk.count → 读回 <6 →
                // decode None → meter_grid 永久 offline（phase 断供），只在运行期暴露；配置期须拦截。
                for b in &s.regs {
                    let is_phase = ["p", "q", "pf", "u", "i"].contains(&b.name.as_str());
                    if is_phase && b.count < 6 {
                        return Err(format!(
                            "south_stations: meter_grid 站 {} 相量块 {} count={} 须 ≥ 6（3 相×2 寄存器）",
                            s.id, b.name, b.count
                        ));
                    }
                    if b.name == "p_total" && b.count < 2 {
                        return Err(format!(
                            "south_stations: meter_grid 站 {} p_total 块 count={} 须 ≥ 2",
                            s.id, b.count
                        ));
                    }
                }
                // 块名唯一：mapper 按 name 取首块（`.find`），异 addr 同名不重叠时后者静默死配置。
                let mut names: Vec<&str> = Vec::new();
                for b in &s.regs {
                    if names.contains(&b.name.as_str()) {
                        return Err(format!(
                            "south_stations: meter_grid 站 {} regs 块名重复: {}（mapper 按 name 取首块，后者静默失效）",
                            s.id, b.name
                        ));
                    }
                    names.push(&b.name);
                }
                // addr>0 + 区间不重叠（含 p_total；width 取块 count，至少 1）
                let mut seen: Vec<(&str, u16, u32)> = Vec::new();
                for b in &s.regs {
                    if b.addr == 0 {
                        return Err(format!(
                            "south_stations: meter_grid 站 {} regs 块 {} addr 不能为 0",
                            s.id, b.name
                        ));
                    }
                    let width = (b.count as u32).max(1);
                    for (name, addr, w) in &seen {
                        let ai = b.addr as u32;
                        let aj = *addr as u32;
                        if ai < aj + w && aj < ai + width {
                            return Err(format!(
                                "south_stations: meter_grid 站 {} regs 块 {} 与 {} 寄存器区间重叠（{}@{:#x} 与 {}@{:#x}）",
                                s.id, b.name, name, b.name, b.addr, name, addr
                            ));
                        }
                    }
                    seen.push((&b.name, b.addr, width));
                }
            }
        }
        // 同口 baud 一致性：物理共享口波特率（Rs485Device 无动态切波特，同口只能一个波特率）。
        // 同 port 的站 baud_rate 必须相同，否则 Err（startup 每口用首站 conf open，异 baud 会被静默忽略）。
        let mut port_bauds: Vec<(&str, &str, u32)> = Vec::new();
        for s in &self.stations {
            if let Some((_, first_id, first_baud)) = port_bauds
                .iter()
                .find(|(p, _, _)| *p == s.port.as_str())
            {
                if *first_baud != s.baud_rate {
                    return Err(format!(
                        "south_stations: 站 {} port {} baud_rate={} 与同口首站 {}（baud_rate={}）不一致——同口共享物理波特率，须统一",
                        s.id, s.port, s.baud_rate, first_id, first_baud
                    ));
                }
            } else {
                port_bauds.push((s.port.as_str(), s.id.as_str(), s.baud_rate));
            }
        }
        Ok(())
    }

    /// role=grid 站（策略 phase 真源站；唯一 grid 形态——master_meter 段已删收敛，S3b-1c）。
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
fn default_baud_rate() -> u32 {
    DEFAULT_BAUD_RATE
}
fn default_reg_func() -> RegFunc {
    RegFunc::Holding
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
        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: p_total, addr: 0x1006, format: float32, count: 2 }
        - { name: q, addr: 0x1008, format: float32, count: 6 }
        - { name: pf, addr: 0x100E, format: float32, count: 6 }
        - { name: u, addr: 0x1014, format: float32, count: 6 }
        - { name: i, addr: 0x101A, format: float32, count: 6 }
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
    fn validate_rejects_multiple_battery() {
        let yaml = r#"
south_stations:
  stations:
    - { id: b1, role: battery, port: t1, slave: 1, interval_ms: 1000 }
    - { id: b2, role: battery, port: t2, slave: 2, interval_ms: 1000 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(
            w.south_stations.validate().is_err(),
            "两个 battery 站应被拒绝（BMS SOC 单源约束，AiIntegrator bms_soc 单槽）"
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
        // 钉死 >= 边界语义：4999(<5000) 合法，5000(==DATA_FRESHNESS_MS) 拒绝。
        // meter_grid 须配完整 regs（S3b-1c 校验），否则 4999 也会因缺相量块被拒——隔离 interval 语义。
        for (iv, ok) in [(4999u64, true), (5000u64, false)] {
            let yaml = format!(
                "south_stations:\n  stations:\n    - id: mg\n      role: meter_grid\n      port: t1\n      slave: 1\n      interval_ms: {iv}\n      regs:\n        - {{ name: p, addr: 0x1000, format: float32, count: 6 }}\n        - {{ name: q, addr: 0x1006, format: float32, count: 6 }}\n        - {{ name: pf, addr: 0x100C, format: float32, count: 6 }}\n        - {{ name: u, addr: 0x1012, format: float32, count: 6 }}\n        - {{ name: i, addr: 0x1018, format: float32, count: 6 }}"
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
    fn validate_battery_interval_boundary() {
        // Battery 站与 meter_grid 对称：interval 须 < DATA_FRESHNESS_MS——battery 是 BMS SOC
        // 真源，若采集间隔 > fresh 窗口 5s，BMS 每轮仅前 5s fresh、余下回落核间 → SOC 源周期
        // 翻转、soc_protect 剪带震荡。4999(<5000) 合法，6000(>=5000) 拒绝。
        for (iv, ok) in [(4999u64, true), (6000u64, false)] {
            let yaml = format!(
                "south_stations:\n  stations:\n    - {{ id: bat, role: battery, port: t1, slave: 1, interval_ms: {iv} }}"
            );
            let w: Wrapper = serde_yaml::from_str(&yaml).expect("解析失败");
            assert_eq!(
                w.south_stations.validate().is_ok(),
                ok,
                "battery interval_ms={iv} 期望 ok={ok}"
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

    /// 完整 meter_grid regs（addr 非 0、半开区间不重叠、p_total 可选含）
    fn meter_grid_full_regs_yaml() -> &'static str {
        // 注意：与上面注释同口径——相量块 count 6，p_total count 2
        r#"        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }
        - { name: p_total, addr: 0x101E, format: float32, count: 2 }"#
    }

    /// 单 meter_grid 站 yaml（regs_body 为已缩进 8 空格的列表行，原样内插）
    fn meter_grid_only_yaml(regs_body: &str) -> String {
        "south_stations:\n  stations:\n    - id: mg\n      role: meter_grid\n      port: t1\n      slave: 1\n      interval_ms: 1000\n      regs:\n".to_string()
            + regs_body
    }

    /// S3b-1c：meter_grid regs 缺相量块（这里缺 q）→ validate Err（总表 phase 真源须完整）
    #[test]
    fn validate_rejects_meter_grid_missing_phase_block() {
        let regs = r#"        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("缺相量块 q"),
            "缺 q 应报缺块（含 q 名），实际: {err}"
        );
    }

    /// S3b-1c：空 regs 的 meter_grid 亦被拒（落入缺 p 分支；空 regs = 站永久 offline、phase 断供）
    #[test]
    fn validate_rejects_meter_grid_empty_regs() {
        let regs = "";
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("缺相量块 p"),
            "空 regs 应报缺 p 块，实际: {err}"
        );
    }

    /// S3b-1c：meter_grid regs 某块 addr=0 → validate Err（承接 legacy validate_reg_map 拒 0）
    #[test]
    fn validate_rejects_meter_grid_zero_addr() {
        let regs = r#"        - { name: p, addr: 0, format: float32, count: 6 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("addr 不能为 0") && err.contains("p"),
            "p addr=0 应报 addr 不能为 0，实际: {err}"
        );
    }

    /// S3b-1c：meter_grid regs 两块半开区间重叠 → validate Err
    #[test]
    fn validate_rejects_meter_grid_overlapping_regs() {
        // q addr 0x1002 落入 p 块 [0x1000,0x1006) 区间内
        let regs = r#"        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: q, addr: 0x1002, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("区间重叠") && err.contains("p") && err.contains("q"),
            "p 与 q 区间重叠应报重叠（含两块名），实际: {err}"
        );
    }

    /// S3b-1c：完整 p/q/pf/u/i + p_total（addr 非 0 不重叠）→ validate Ok
    #[test]
    fn validate_accepts_meter_grid_complete_regs() {
        let w: Wrapper =
            serde_yaml::from_str(&meter_grid_only_yaml(meter_grid_full_regs_yaml()))
                .expect("解析失败");
        assert!(
            w.south_stations.validate().is_ok(),
            "完整 meter_grid regs 应通过: {:?}",
            w.south_stations.validate()
        );
    }

    /// S3b-1c：相量块 p count:2（<6，count 语义错）→ Err 含 count（name/addr/重叠均过，
    /// 仅 count 不够；mapper decode_phase_block 硬性 ≥6，漏配 default 2 → 读回 <6 → 永久 offline）
    #[test]
    fn validate_rejects_meter_grid_phase_block_count_lt_6() {
        let regs = r#"        - { name: p, addr: 0x1000, format: float32, count: 2 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("count") && err.contains("p"),
            "相量块 p count=2 应报 count 须 ≥6，实际: {err}"
        );
    }

    /// S3b-1c：同块名重复（异 addr 不重叠也死配）→ Err 含"块名重复"（mapper `.find` 取首块，后者静默失效）
    #[test]
    fn validate_rejects_meter_grid_duplicate_block_name() {
        let regs = r#"        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: p, addr: 0x1006, format: float32, count: 6 }
        - { name: q, addr: 0x100C, format: float32, count: 6 }
        - { name: pf, addr: 0x1012, format: float32, count: 6 }
        - { name: u, addr: 0x1018, format: float32, count: 6 }
        - { name: i, addr: 0x101E, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        let err = w.south_stations.validate().unwrap_err();
        assert!(
            err.contains("块名重复") && err.contains("p"),
            "两块同名 p 应报块名重复，实际: {err}"
        );
    }

    /// S3b-1c：完整 p/q/pf/u/i（无 p_total，count 各 6）→ Ok——p_total 可选正向
    #[test]
    fn validate_accepts_meter_grid_without_p_total() {
        let regs = r#"        - { name: p, addr: 0x1000, format: float32, count: 6 }
        - { name: q, addr: 0x1006, format: float32, count: 6 }
        - { name: pf, addr: 0x100C, format: float32, count: 6 }
        - { name: u, addr: 0x1012, format: float32, count: 6 }
        - { name: i, addr: 0x1018, format: float32, count: 6 }"#;
        let w: Wrapper = serde_yaml::from_str(&meter_grid_only_yaml(regs)).expect("解析失败");
        assert!(
            w.south_stations.validate().is_ok(),
            "无 p_total 的完整 meter_grid regs 应通过（p_total 可选）: {:?}",
            w.south_stations.validate()
        );
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

    #[test]
    fn default_baud_and_func_apply_when_omitted() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        let st = &w.south_stations.stations[0];
        assert_eq!(st.baud_rate, DEFAULT_BAUD_RATE);
    }

    #[test]
    fn reg_block_func_parses_holding_input() {
        let yaml = r#"
south_stations:
  stations:
    - id: a
      role: battery
      port: t1
      slave: 1
      regs:
        - { name: soc, addr: 100, func: input }
        - { name: temp, addr: 200 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        let regs = &w.south_stations.stations[0].regs;
        assert_eq!(regs[0].func, RegFunc::Input);
        assert_eq!(regs[1].func, RegFunc::Holding); // 缺省 FC03
    }

    #[test]
    fn validate_rejects_same_port_mixed_baud() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 1, baud_rate: 9600 }
    - { id: b, role: hvac,    port: t1, slave: 2, baud_rate: 19200 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn validate_accepts_same_port_same_baud() {
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 1, baud_rate: 9600 }
    - { id: b, role: hvac,    port: t1, slave: 2, baud_rate: 9600 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_ok());
    }

    #[test]
    fn validate_rejects_same_port_mixed_baud_3_station() {
        // 第 3 站异 baud → 应被拒（同口物理共享波特率）
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 1, baud_rate: 9600 }
    - { id: b, role: hvac,    port: t1, slave: 2, baud_rate: 9600 }
    - { id: c, role: fire,    port: t1, slave: 3, baud_rate: 19200 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn validate_accepts_diff_ports_same_baud() {
        // 不同口独立物理口，同 baud 无冲突 → Ok
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 1, baud_rate: 9600 }
    - { id: b, role: hvac,    port: t2, slave: 2, baud_rate: 9600 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_ok());
    }

    #[test]
    fn validate_rejects_zero_baud_rate() {
        // baud_rate=0 整口皆 0 时同口一致性检查放行，必须靠下界拦截
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 1, baud_rate: 0 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_baud_same_port_all_zero() {
        // 整口 3 站 baud_rate 全为 0：同口一致性检查放行，须被下界拦截
        let yaml = r#"
south_stations:
  stations:
    - { id: a, role: battery, port: t1, slave: 1, baud_rate: 0 }
    - { id: b, role: hvac,    port: t1, slave: 2, baud_rate: 0 }
    - { id: c, role: fire,    port: t1, slave: 3, baud_rate: 0 }
"#;
        let w: Wrapper = serde_yaml::from_str(yaml).expect("解析失败");
        assert!(w.south_stations.validate().is_err());
    }
}
