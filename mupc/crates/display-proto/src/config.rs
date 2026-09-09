//! 本地显示终端配置结构（单一真源）——与设计 §7.1 逐字段对齐。
//!
//! 结构定义放 display-proto（而非各消费方各写一遍）：mupcd（core-bin，反序列化
//! `mupc_core_config.yaml` 的 `display:` 段）与 mupc-local-display（渲染 CLI，默认值取
//! 本文件共享常量）都依赖该形态。渲染分辨率/字体属渲染进程 CLI 参数，**不入**
//! `DisplayConfig`（设计 §7.2：渲染端不读 core 配置）。

/// 回环发布端点默认值（`bind_addr`）。
pub const DEFAULT_BIND: &str = "127.0.0.1:9810";
/// 渲染进程默认通道 URL（指向 mupcd 回环端点；与 `DEFAULT_BIND` 同指一端点）。
pub const DEFAULT_CHANNEL_URL: &str = "http://127.0.0.1:9810/v1/display/latest";
/// mupcd 采集/组帧/发布周期默认值（标称 1Hz）。
pub const DEFAULT_PUBLISH_MS: u64 = 1000;

/// 域值化量程/阈值（mupcd 消费；PRD §6.5 越界判 RangeError；§6.6 一致性阈值基准）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DisplayRange {
    /// 三相电流量程上限(A) 默认 300。
    pub current_max_a: f64,
    /// 单相有功量程上限(kW) 默认 100。
    pub phase_power_max_kw: f64,
    /// 设备总有功量程上限(kW) 默认 300。
    pub total_power_max_kw: f64,
    /// PCS 总额定(kW) 默认 60（6.6 一致性阈值基准）。
    pub pcs_total_rated_kw: f64,
    /// 6.6 一致性反向显著阈值(kW) 默认 3.0 = 60kW*5%（PRD F2 验收3）。
    pub inconsistency_threshold_kw: f64,
}

impl Default for DisplayRange {
    fn default() -> Self {
        Self {
            current_max_a: 300.0,
            phase_power_max_kw: 100.0,
            total_power_max_kw: 300.0,
            pcs_total_rated_kw: 60.0,
            inconsistency_threshold_kw: 3.0,
        }
    }
}

/// 本地显示终端发布侧配置。字段与 `mupc_core_config.yaml` 的 `display:` 段一一对应。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// true = mupcd 起 DisplayDataProvider + 回环发布（默认缺省 disabled，行为不变）。
    pub enabled: bool,
    /// 回环发布端点（默认 `DEFAULT_BIND`）。
    pub bind_addr: String,
    /// mupcd 采集/组帧/发布周期(ms)（默认 `DEFAULT_PUBLISH_MS`）。
    pub publish_ms: u64,
    /// 域值化量程（mupcd 消费；PRD §6.5 越界判 RangeError）。
    pub range: DisplayRange,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: DEFAULT_BIND.to_string(),
            publish_ms: DEFAULT_PUBLISH_MS,
            range: DisplayRange::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_config_default() {
        let c = DisplayConfig::default();
        assert!(!c.enabled, "默认整段缺省 = disabled（行为不变，设计 §7.3）");
        assert_eq!(c.bind_addr, DEFAULT_BIND);
        assert_eq!(c.bind_addr, "127.0.0.1:9810");
        assert_eq!(c.publish_ms, 1000);
        let r = &c.range;
        assert_eq!(r.current_max_a, 300.0);
        assert_eq!(r.phase_power_max_kw, 100.0);
        assert_eq!(r.total_power_max_kw, 300.0);
        assert_eq!(r.pcs_total_rated_kw, 60.0);
        assert_eq!(r.inconsistency_threshold_kw, 3.0);
    }

    #[test]
    fn display_range_default_matches_yaml_section() {
        // 设计 §7.3 yaml 值对齐
        let r = DisplayRange::default();
        assert_eq!(r.inconsistency_threshold_kw, 60.0 * 0.05);
    }

    #[test]
    fn display_config_deserialize_partial_with_defaults() {
        // 部分字段缺失 → serde(default) 填充默认（对应 core yaml 只写 display.enabled=true）
        let json = r#"{"enabled":true}"#;
        let c: DisplayConfig = serde_json::from_str(json).unwrap();
        assert!(c.enabled);
        assert_eq!(c.bind_addr, DEFAULT_BIND);
        assert_eq!(c.range, DisplayRange::default());
    }

    #[test]
    fn display_config_json_roundtrip() {
        let c = DisplayConfig::default();
        let json = serde_json::to_string(&c).unwrap();
        let back: DisplayConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }
}
