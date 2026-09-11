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
/// 主拍周期硬下界（设计 §4.9：`publish_ms >= 100`；`1..99` 会被打成高频通道，不得放行）。
pub const MIN_PUBLISH_MS: u64 = 100;
/// F7 告警列表最多展示条数（设计 §4.9 `display.alarm_page_size` 默认 10；§3.1「items ≤10」）。
pub const DEFAULT_ALARM_PAGE_SIZE: usize = 10;
/// 实时日志 ring 容量硬下界（设计 §4.9：`live_ring >= 100`）。
pub const MIN_LIVE_RING: usize = 100;
/// 控制通道回环绑定端点默认值（设计 §3.3 / §4.9：`display.control_bind_addr`）。
pub const DEFAULT_CONTROL_BIND: &str = "127.0.0.1:9811";
/// HMI 控制通道客户端默认基址（与 `DEFAULT_CONTROL_BIND` 同指一端点）。
pub const DEFAULT_CONTROL_BASE_URL: &str = "http://127.0.0.1:9811";
/// 慢拍变更唤醒组帧的合并窗口下限（设计 §3.1：合并窗口 ≥250 ms）。
pub const DEFAULT_MIN_PUBLISH_INTERVAL_MS: u64 = 250;
/// 装置状态慢拍周期默认值（设计 §3.1：3 s；端到端 ≤3.85 s）。
pub const DEFAULT_DEVICE_POLL_MS: u64 = 3000;
/// 告警慢拍周期默认值（设计 §3.1：0.5 s；端到端 ≤1.35 s）。
pub const DEFAULT_ALARM_POLL_MS: u64 = 500;
/// 联锁慢拍周期默认值（设计 §3.1：0.5 s；端到端 ≤1.35 s）。
pub const DEFAULT_INTERLOCK_POLL_MS: u64 = 500;
/// 慢拍周期硬上界（设计 §11.1：`alarm_poll_ms`/`interlock_poll_ms` ≤1000ms 为
/// F7.3 / F16.5「≤2 s 上屏」的算式前提，放开即可能静默破坏验收）。
pub const MAX_SLOW_POLL_MS: u64 = 1000;
/// 装置状态慢拍周期硬上界（设计 §11.1：`device_poll_ms` ≤4000ms）。
pub const MAX_DEVICE_POLL_MS: u64 = 4000;
/// 合并窗口下限（设计 §11.1：`min_publish_interval_ms ∈ [100, publish_ms]`）。
pub const MIN_MERGE_WINDOW_MS: u64 = 100;

/// 服务端限额（`log` 段；设计 §8.3「log 限额」/ §4.4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LogLimits {
    /// 实时 ring 容量（条）。serde 键名 = **`live_ring`**（设计 §4.9 的 `log.live_ring`），
    /// 不可写成 `ring_capacity`——现场按设计写 `live_ring:` 会被 serde 静默忽略，
    /// 限额不生效且无任何报错（属「静默失效」缺陷）。
    pub live_ring: usize,
    /// 单次日志检索最多扫描文件数。
    pub max_files: usize,
    /// 单次日志检索总行数上限。
    pub max_lines: usize,
    /// 日志单页返回条数上限。
    pub page_limit_max: usize,
    /// 审计分页大小。
    pub audit_page_size: usize,
}

impl Default for LogLimits {
    fn default() -> Self {
        Self {
            live_ring: crate::log::LOG_RING_CAPACITY,
            max_files: crate::log::LOG_SCAN_MAX_FILES,
            max_lines: crate::log::LOG_SCAN_MAX_LINES,
            page_limit_max: crate::log::LOG_PAGE_LIMIT_MAX,
            audit_page_size: crate::audit::AUDIT_PAGE_SIZE,
        }
    }
}

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
///
/// v2.0 扩展（设计 §8.3）：新增控制通道绑定、合并窗口与慢拍节拍、日志限额。
/// 全部 `serde(default)` → 现场既有 yaml（只写 `display.enabled`）仍可正常加载。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// true = mupcd 起 DisplayDataProvider + 回环发布（默认缺省 disabled，行为不变）。
    pub enabled: bool,
    /// 读通道回环发布端点（默认 `DEFAULT_BIND`）。
    pub bind_addr: String,
    /// 控制通道回环绑定端点（默认 `DEFAULT_CONTROL_BIND`；设计 §3.3 / §4.9）。
    pub control_bind_addr: String,
    /// mupcd 采集/组帧/发布周期(ms)（默认 `DEFAULT_PUBLISH_MS`）。
    pub publish_ms: u64,
    /// 慢拍变更唤醒组帧的最小合并窗口(ms)（默认 250；须 ∈ [`MIN_MERGE_WINDOW_MS`, publish_ms]）。
    pub min_publish_interval_ms: u64,
    /// 装置状态慢拍周期(ms)（默认 3000；≤ [`MAX_DEVICE_POLL_MS`]）。
    pub device_poll_ms: u64,
    /// 告警慢拍周期(ms)（默认 500；≤ [`MAX_SLOW_POLL_MS`]）。
    pub alarm_poll_ms: u64,
    /// 联锁慢拍周期(ms)（默认 500；≤ [`MAX_SLOW_POLL_MS`]）。
    pub interlock_poll_ms: u64,
    /// F7 告警列表最多展示条数（默认 [`DEFAULT_ALARM_PAGE_SIZE`] = 10；
    /// 设计 §4.9 `display.alarm_page_size`，落地 §3.1 的「items ≤10」）。
    pub alarm_page_size: usize,
    /// 服务端限额（日志 / 审计）。
    pub log: LogLimits,
    /// 域值化量程（mupcd 消费；PRD §6.5 越界判 RangeError）。
    pub range: DisplayRange,
}

impl DisplayConfig {
    /// 校验节拍约束（设计 §11.1 时延拆解回归的落地约束）+ 量程段有限正数（PRD §6.5）。
    ///
    /// 这些值是 F6.3 / F7.3 / F16.5「上屏 ≤5 s / ≤2 s」算式的**输入**：
    /// 放开即可能**静默**破坏验收（如退化回纯 1 Hz 主拍会使 F7.3 变为 2.1 s 超差），
    /// 故启动即拒，不在运行期容忍。
    ///
    /// 返回 [`crate::Error::InvalidConfig`]（含 `field` / `reason`），调用方可 `match` 定位；
    /// **不得**靠字符串解析。
    pub fn validate(&self) -> crate::Result<()> {
        let invalid = |field: &str, reason: String| -> crate::Result<()> {
            Err(crate::Error::InvalidConfig {
                field: field.to_string(),
                reason,
            })
        };

        if self.publish_ms < MIN_PUBLISH_MS {
            return invalid(
                "display.publish_ms",
                format!(
                    "={} 越界，须 ≥ {MIN_PUBLISH_MS}（主拍下界；1..99 会打成高频通道）",
                    self.publish_ms
                ),
            );
        }
        if self.alarm_poll_ms == 0 || self.alarm_poll_ms > MAX_SLOW_POLL_MS {
            return invalid(
                "display.alarm_poll_ms",
                format!(
                    "={} 越界，须 ∈ [1, {MAX_SLOW_POLL_MS}]（F7.3 上屏 ≤2 s 前提）",
                    self.alarm_poll_ms
                ),
            );
        }
        if self.interlock_poll_ms == 0 || self.interlock_poll_ms > MAX_SLOW_POLL_MS {
            return invalid(
                "display.interlock_poll_ms",
                format!(
                    "={} 越界，须 ∈ [1, {MAX_SLOW_POLL_MS}]（F16.5 上屏 ≤2 s 前提）",
                    self.interlock_poll_ms
                ),
            );
        }
        if self.device_poll_ms == 0 || self.device_poll_ms > MAX_DEVICE_POLL_MS {
            return invalid(
                "display.device_poll_ms",
                format!(
                    "={} 越界，须 ∈ [1, {MAX_DEVICE_POLL_MS}]（F6.3 ≤5 s 前提）",
                    self.device_poll_ms
                ),
            );
        }
        if self.min_publish_interval_ms < MIN_MERGE_WINDOW_MS
            || self.min_publish_interval_ms > self.publish_ms
        {
            return invalid(
                "display.min_publish_interval_ms",
                format!(
                    "={} 越界，须 ∈ [{MIN_MERGE_WINDOW_MS}, publish_ms={}]",
                    self.min_publish_interval_ms, self.publish_ms
                ),
            );
        }
        // F7 最多展示条数：0 条 = 告警页永远空，且与「源不可用」混淆（EDGE-09）→ 启动即拒
        if self.alarm_page_size == 0 {
            return invalid(
                "display.alarm_page_size",
                "不得为 0（否则 F7 永远无条目）".to_string(),
            );
        }
        // 实时日志 ring 容量下界（设计 §4.9：`live_ring >= 100`）
        if self.log.live_ring < MIN_LIVE_RING {
            return invalid(
                "display.log.live_ring",
                format!(
                    "={} 越界，须 ≥ {MIN_LIVE_RING}（太小则上线即被冲掉，检索无意义）",
                    self.log.live_ring
                ),
            );
        }

        // ── 量程段：每个数值字段须为**有限正数** ────────────────────────────────
        //
        // 这是「静默坏值」最集中的一处：`CoreConfig` 走 serde_yaml，而 YAML 支持
        // `.inf` / `.nan` 字面量，会原样落入 `f64`。一旦放行：
        // - `current_max_a = .inf` → 下游 `v.abs() <= max` **恒真** → 量程守卫静默失效，
        //   越界值以 `Valid` 上屏（违反 PRD §6.5）；
        // - `*_max_kw = 0` → 全部读数恒 `RangeError`，有效数据被永久丢弃；
        // - `inconsistency_threshold_kw = -1` → 方向不一致角标恒亮。
        // 故此处逐字段 `is_finite() && > 0.0`，**启动即拒**。
        for (field, value) in [
            ("display.range.current_max_a", self.range.current_max_a),
            ("display.range.phase_power_max_kw", self.range.phase_power_max_kw),
            ("display.range.total_power_max_kw", self.range.total_power_max_kw),
            ("display.range.pcs_total_rated_kw", self.range.pcs_total_rated_kw),
            (
                "display.range.inconsistency_threshold_kw",
                self.range.inconsistency_threshold_kw,
            ),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return invalid(
                    field,
                    format!(
                        "={value} 越界，须为有限正数（禁 NaN / ±Inf / 0 / 负数；否则量程守卫静默失效）"
                    ),
                );
            }
        }
        // 单相量程不得超过总有功量程（否则单相读数恒 RangeError，有效数据被永久丢弃）
        if self.range.phase_power_max_kw > self.range.total_power_max_kw {
            return invalid(
                "display.range.total_power_max_kw",
                format!(
                    "={} 小于 phase_power_max_kw={}（单相量程不得大于总有功量程）",
                    self.range.total_power_max_kw, self.range.phase_power_max_kw
                ),
            );
        }
        // 一致性阈值须为「超量程 5%」量级（PRD F2 验收3），不得超过 PCS 总额定：
        // 超过则方向不一致角标永不触发（恒灭），同样属静默失效。
        if self.range.inconsistency_threshold_kw > self.range.pcs_total_rated_kw {
            return invalid(
                "display.range.inconsistency_threshold_kw",
                format!(
                    "={} 超过 pcs_total_rated_kw={}（阈值须为额定的小比例，否则角标恒灭）",
                    self.range.inconsistency_threshold_kw, self.range.pcs_total_rated_kw
                ),
            );
        }

        // 读 / 控制通道强制回环（安全红线 PL-4，EDGE-24）。
        // 必须**真正解析 host:port**：`starts_with("127.0.0.1:")` 会放行
        // `127.0.0.1:0`（非法端口）与 `127.0.0.1:notaport`（非数字端口）。
        for (k, addr) in [
            ("display.bind_addr", self.bind_addr.as_str()),
            ("display.control_bind_addr", self.control_bind_addr.as_str()),
        ] {
            match addr.parse::<std::net::SocketAddr>() {
                Ok(sa) if sa.ip().is_loopback() && sa.port() != 0 => {}
                _ => {
                    return invalid(
                        k,
                        format!(
                            "=`{addr}` 非合法回环 host:port（仅允许 127.0.0.1/::1 且端口 ∈ [1, 65535]；PL-4 安全红线）"
                        ),
                    )
                }
            }
        }
        // 两地址不得相同（设计 §4.9）：同址会让读 / 控制两条通道互相抢占，后绑者启动失败。
        // 按**解析后**的端点比较，避免 `127.0.0.1:9810` 与 `127.0.0.1:09810` 这类同端异写漏判。
        let read_addr = self.bind_addr.parse::<std::net::SocketAddr>().ok();
        let ctrl_addr = self.control_bind_addr.parse::<std::net::SocketAddr>().ok();
        if read_addr.is_some() && read_addr == ctrl_addr {
            return invalid(
                "display.control_bind_addr",
                format!(
                    "与 display.bind_addr 不得相同（均为 `{}`）：读 / 控制须各占一端点",
                    self.bind_addr
                ),
            );
        }
        Ok(())
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: DEFAULT_BIND.to_string(),
            control_bind_addr: DEFAULT_CONTROL_BIND.to_string(),
            publish_ms: DEFAULT_PUBLISH_MS,
            min_publish_interval_ms: DEFAULT_MIN_PUBLISH_INTERVAL_MS,
            device_poll_ms: DEFAULT_DEVICE_POLL_MS,
            alarm_poll_ms: DEFAULT_ALARM_POLL_MS,
            interlock_poll_ms: DEFAULT_INTERLOCK_POLL_MS,
            alarm_page_size: DEFAULT_ALARM_PAGE_SIZE,
            log: LogLimits::default(),
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

    /// Critical 1：`range` 段每个数值字段都要**正例放行 + 反例拒绝**。
    ///
    /// 反例强制覆盖 `NaN` / `+Inf` / `-Inf` / `0.0` / 负数——YAML 的 `.inf` / `.nan`
    /// 字面量会原样落入 `f64`，未校验即让下游量程守卫静默失效（PRD §6.5）。
    #[test]
    fn display_range_every_numeric_field_rejects_non_finite_or_non_positive() {
        // 各字段的 (名, getter, setter)：逐字段构造反例 / 正例
        type RangeField = (&'static str, fn(&DisplayRange) -> f64, fn(&mut DisplayRange, f64));
        let fields: [RangeField; 5] = [
            (
                "current_max_a",
                |r| r.current_max_a,
                |r, v| r.current_max_a = v,
            ),
            (
                "phase_power_max_kw",
                |r| r.phase_power_max_kw,
                |r, v| r.phase_power_max_kw = v,
            ),
            (
                "total_power_max_kw",
                |r| r.total_power_max_kw,
                |r, v| r.total_power_max_kw = v,
            ),
            (
                "pcs_total_rated_kw",
                |r| r.pcs_total_rated_kw,
                |r, v| r.pcs_total_rated_kw = v,
            ),
            (
                "inconsistency_threshold_kw",
                |r| r.inconsistency_threshold_kw,
                |r, v| r.inconsistency_threshold_kw = v,
            ),
        ];
        // 先确认基线默认配置自洽（否则下面的「单字段改动」归因不成立）
        assert!(DisplayConfig::default().validate().is_ok());

        for (field, get, set) in fields {
            // 反例：NaN / ±Inf / 0 / -0 / 负数 一律拒绝（不得静默放行）
            for bad in [
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
                0.0,
                -0.0,
                -1.0,
                -300.0,
            ] {
                let mut cfg = DisplayConfig::default();
                set(&mut cfg.range, bad);
                let err = cfg.validate().unwrap_err().to_string();
                assert!(
                    err.contains("display.range.") && err.contains(field),
                    "range.{field}={bad} 必须拒绝（不得静默放行），实际: {err}"
                );
                assert!(
                    err.contains("有限正数"),
                    "拒绝原因须指出「有限正数」，实际: {err}"
                );
            }
            // 正例：该字段取默认值的 2 倍（有限正数且与其它字段仍自洽）→ 放行
            let mut ok = DisplayConfig::default();
            set(&mut ok.range, get(&DisplayRange::default()) * 2.0);
            assert!(
                ok.validate().is_ok(),
                "range.{field} 取默认值 2 倍时须放行: {:?}",
                ok.validate()
            );
        }
    }

    /// 评审员实测的三个具体反例（回归锚点）：`.inf` / `0` / `-1`。
    #[test]
    fn display_range_reviewer_probe_cases_are_rejected() {
        let probe = |f: fn(&mut DisplayRange)| {
            let mut c = DisplayConfig::default();
            f(&mut c.range);
            c.validate().unwrap_err().to_string()
        };
        // ① current_max_a = .inf → 下游 `v.abs() <= max` 恒真（量程守卫静默失效）
        let e = probe(|r| r.current_max_a = f64::INFINITY);
        assert!(e.contains("current_max_a") && e.contains("有限正数"), "{e}");
        // ② phase_power_max_kw = 0 → 全部读数恒 RangeError（有效数据被永久丢弃）
        let e = probe(|r| r.phase_power_max_kw = 0.0);
        assert!(e.contains("phase_power_max_kw"), "{e}");
        // ③ inconsistency_threshold_kw = -1 → 方向不一致角标恒亮
        let e = probe(|r| r.inconsistency_threshold_kw = -1.0);
        assert!(e.contains("inconsistency_threshold_kw"), "{e}");
        // 另有：阈值 > 额定 → 角标恒灭（同样静默失效）
        let e = probe(|r| r.inconsistency_threshold_kw = 999.0);
        assert!(e.contains("pcs_total_rated_kw"), "{e}");
        // 另有：单相量程 > 总有功量程 → 单相读数恒 RangeError
        let e = probe(|r| r.phase_power_max_kw = 301.0);
        assert!(e.contains("total_power_max_kw"), "{e}");
    }

    /// Minor 7：`validate()` 返回结构化 [`crate::Error::InvalidConfig`]，调用方可 `match`。
    #[test]
    fn validate_returns_matchable_invalid_config_variant() {
        let bad = DisplayConfig {
            publish_ms: 1,
            min_publish_interval_ms: 1,
            ..Default::default()
        };
        match bad.validate() {
            Err(crate::Error::InvalidConfig { field, reason }) => {
                assert_eq!(field, "display.publish_ms");
                assert!(reason.contains("越界"));
            }
            other => panic!("须为 InvalidConfig，实际: {other:?}"),
        }
        // Display 文案含 field，便于现场定位（既有日志口径不回归）
        let msg = bad.validate().unwrap_err().to_string();
        assert!(msg.contains("display.publish_ms") && msg.contains("invalid config"));
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

    #[test]
    fn display_config_v2_defaults_match_design() {
        let c = DisplayConfig::default();
        assert_eq!(c.control_bind_addr, "127.0.0.1:9811");
        assert_eq!(c.min_publish_interval_ms, 250, "合并窗口 ≥250 ms（设计 §3.1）");
        assert_eq!(c.device_poll_ms, 3000, "慢拍 A = 3 s");
        assert_eq!(c.alarm_poll_ms, 500, "慢拍 B = 0.5 s");
        assert_eq!(c.interlock_poll_ms, 500, "慢拍 C = 0.5 s");
        assert_eq!(c.alarm_page_size, 10, "F7 最多展示 10 条（设计 §4.9）");
        assert_eq!(c.log, LogLimits::default());
        assert_eq!(c.log.live_ring, 2000);
        assert_eq!(c.log.max_files, 5);
        assert_eq!(c.log.max_lines, 50_000);
        assert_eq!(c.log.page_limit_max, 200);
        assert_eq!(c.log.audit_page_size, 20);
        // 默认配置自洽（逐条落设计 §11.1 的约束）
        assert!(c.validate().is_ok(), "默认配置须通过校验: {:?}", c.validate());
        // 读通道 URL 路径与本 crate 的 LATEST_PATH 常量同源
        assert!(DEFAULT_CHANNEL_URL.ends_with(crate::frame::LATEST_PATH));
        assert!(DEFAULT_CONTROL_BASE_URL.ends_with(":9811"));
    }

    #[test]
    fn display_config_timing_bounds_rejected_not_silently_accepted() {
        let err = |c: DisplayConfig| c.validate().unwrap_err().to_string();
        // 慢拍退化（如 2000 ms）会让 F7.3 变成 3.5 s 超差 → 启动即拒
        assert!(err(DisplayConfig {
            alarm_poll_ms: 2000,
            ..Default::default()
        })
        .contains("alarm_poll_ms"));
        assert!(err(DisplayConfig {
            interlock_poll_ms: 1001,
            ..Default::default()
        })
        .contains("interlock_poll_ms"));
        assert!(err(DisplayConfig {
            device_poll_ms: 4001,
            ..Default::default()
        })
        .contains("device_poll_ms"));
        // 合并窗口越界（< 100 或 > publish_ms）
        assert!(err(DisplayConfig {
            min_publish_interval_ms: 99,
            ..Default::default()
        })
        .contains("min_publish_interval_ms"));
        assert!(err(DisplayConfig {
            min_publish_interval_ms: 1001,
            ..Default::default()
        })
        .contains("min_publish_interval_ms"));
        // 边界值放行
        let at_lower = DisplayConfig {
            min_publish_interval_ms: 100,
            ..Default::default()
        };
        assert!(at_lower.validate().is_ok());
        let at_upper = DisplayConfig {
            min_publish_interval_ms: DisplayConfig::default().publish_ms,
            ..Default::default()
        };
        assert!(at_upper.validate().is_ok());
        let poll_at_upper = DisplayConfig {
            alarm_poll_ms: 1000,
            interlock_poll_ms: 1000,
            device_poll_ms: 4000,
            ..Default::default()
        };
        assert!(poll_at_upper.validate().is_ok(), "上界取值须放行");
    }

    #[test]
    fn display_config_rejects_non_loopback_bind() {
        // PL-4 安全红线：读 / 控制通道强制回环
        let bad_read = DisplayConfig {
            bind_addr: "0.0.0.0:9810".to_string(),
            ..Default::default()
        };
        assert!(bad_read.validate().unwrap_err().to_string().contains("回环"));
        let bad_control = DisplayConfig {
            control_bind_addr: "0.0.0.0:9811".to_string(),
            ..Default::default()
        };
        assert!(bad_control.validate().unwrap_err().to_string().contains("回环"));
    }

    /// PL-4 回环判据必须**真正解析 host:port**：旧的 `starts_with("127.0.0.1:")`
    /// 会放行 `127.0.0.1:0` 与 `127.0.0.1:notaport`（评审员探针实测 `Ok`）。
    #[test]
    fn loopback_check_parses_host_and_port_not_prefix_match() {
        let err = |c: DisplayConfig| c.validate().unwrap_err().to_string();
        // 反例：前缀对但端口非法
        for bad in ["127.0.0.1:0", "127.0.0.1:notaport", "127.0.0.1:", "127.0.0.1:65536"] {
            let c = DisplayConfig {
                bind_addr: bad.to_string(),
                ..Default::default()
            };
            assert!(
                err(c).contains("回环"),
                "bind_addr=`{bad}` 端口非法必须拒绝（不得靠前缀放行）"
            );
        }
        // 反例：前缀对但根本不是地址
        let c = DisplayConfig {
            control_bind_addr: "127.0.0.1:9811/garbage".to_string(),
            ..Default::default()
        };
        assert!(err(c).contains("回环"));
        // 正例：合法回环（IPv4 / IPv6）放行
        for good in ["127.0.0.1:9810", "[::1]:9810", "127.0.0.5:9810"] {
            let c = DisplayConfig {
                bind_addr: good.to_string(),
                ..Default::default()
            };
            assert!(c.validate().is_ok(), "合法回环 `{good}` 须放行: {:?}", c.validate());
        }
        // 同端点不同写法（前导 0）必须判为相同
        let c = DisplayConfig {
            control_bind_addr: "127.0.0.1:09810".to_string(),
            ..Default::default()
        };
        assert!(
            err(c).contains("不得相同"),
            "同端异写须按解析后端点判等（否则两通道抢同一端口）"
        );
    }

    #[test]
    fn log_limits_partial_deserialize_uses_defaults() {
        // 现场 yaml 仅写 log.live_ring → 其余取默认
        let json = r#"{"enabled":true,"control_bind_addr":"127.0.0.1:9811","log":{"live_ring":512}}"#;
        let c: DisplayConfig = serde_json::from_str(json).unwrap();
        assert!(c.enabled);
        assert_eq!(c.log.live_ring, 512);
        assert_eq!(c.log.max_files, 5);
        assert_eq!(c.device_poll_ms, 3000);
    }

    /// 键名回归（真实兼容性缺陷）：yaml/JSON **必须**按设计 §4.9 写 `log.live_ring`。
    ///
    /// 若沿用旧名 `ring_capacity`，现场按设计写 `live_ring:` 会被 serde 静默忽略
    /// （未知键默认容忍）→ 限额不生效且无任何报错。此断言把线格式钉死。
    #[test]
    fn log_live_ring_key_name_is_pinned_by_literal_json() {
        // 正例：设计键名 `live_ring` 能生效
        let good = r#"{"log":{"live_ring":321,"max_files":3,"max_lines":1000}}"#;
        let c: DisplayConfig = serde_json::from_str(good).unwrap();
        assert_eq!(c.log.live_ring, 321, "设计键名 live_ring 必须被识别");
        assert_eq!(c.log.max_files, 3);
        assert_eq!(c.log.max_lines, 1000);
        // 序列化输出同样用设计键名（不能被改回 ring_capacity）
        let out = serde_json::to_string(&c).unwrap();
        assert!(out.contains("\"live_ring\":321"), "编码须用 live_ring：{out}");
        assert!(!out.contains("ring_capacity"), "旧键名不得再出现在线格式：{out}");
        // 旧键名 `ring_capacity` 属未知键 → 被忽略，取默认 2000（正是「静默失效」的机理，
        // 此处显式固化该机理，防止有人"顺手"加回 alias 而掩盖设计键名）
        let legacy = r#"{"log":{"ring_capacity":321}}"#;
        let c2: DisplayConfig = serde_json::from_str(legacy).unwrap();
        assert_eq!(c2.log.live_ring, 2000, "旧键名不得生效（否则双键名混淆真源）");
    }

    /// 设计 §4.9 的 `alarm_page_size` 键名与默认值（F7「items ≤10」的配置源）。
    #[test]
    fn alarm_page_size_key_and_default() {
        assert_eq!(DEFAULT_ALARM_PAGE_SIZE, 10);
        assert_eq!(DisplayConfig::default().alarm_page_size, 10);
        let c: DisplayConfig = serde_json::from_str(r#"{"alarm_page_size":7}"#).unwrap();
        assert_eq!(c.alarm_page_size, 7);
        // 0 条 → 启动即拒（否则 F7 永远空，且与「源不可用」混淆）
        let bad = DisplayConfig {
            alarm_page_size: 0,
            ..Default::default()
        };
        assert!(bad.validate().unwrap_err().to_string().contains("alarm_page_size"));
    }

    /// validate 的每一项：正例放行 + 越界反例拒绝（设计 §4.9 新增项逐条）。
    #[test]
    fn validate_new_invariants_each_have_reject_case() {
        let err = |c: DisplayConfig| c.validate().unwrap_err().to_string();

        // ① 两地址不得相同
        let same = DisplayConfig {
            control_bind_addr: DEFAULT_BIND.to_string(),
            ..Default::default()
        };
        assert!(
            err(same).contains("不得相同"),
            "同址必须拒绝（否则读/控制抢同一端口）"
        );
        // 反例边界：仅端口不同 → 放行
        assert!(DisplayConfig::default().validate().is_ok());

        // ② publish_ms >= 100（旧实现只判 ==0，1..99 被错误放行）。
        // 用**全区间** 1..MIN_PUBLISH_MS 覆盖：原缺陷恰恰出在 1..99 这段，
        // 只抽 3 个样本会漏掉未抽到的值。
        for bad_ms in 1..MIN_PUBLISH_MS {
            let c = DisplayConfig {
                publish_ms: bad_ms,
                min_publish_interval_ms: MIN_MERGE_WINDOW_MS,
                ..Default::default()
            };
            assert!(
                err(c).contains("publish_ms"),
                "publish_ms={bad_ms} 必须拒绝（< {MIN_PUBLISH_MS}）"
            );
        }
        // 0 同样拒绝（全区间起点之外的显式边界）
        assert!(err(DisplayConfig {
            publish_ms: 0,
            min_publish_interval_ms: 0,
            ..Default::default()
        })
        .contains("publish_ms"));
        // 正例：恰好 100 放行
        let at_min = DisplayConfig {
            publish_ms: MIN_PUBLISH_MS,
            min_publish_interval_ms: MIN_MERGE_WINDOW_MS,
            ..Default::default()
        };
        assert!(at_min.validate().is_ok(), "publish_ms={MIN_PUBLISH_MS} 须放行");

        // ③ live_ring >= 100（旧实现完全缺失该约束）
        let mut small_ring = DisplayConfig::default();
        small_ring.log.live_ring = 99;
        assert!(err(small_ring).contains("live_ring"), "live_ring=99 必须拒绝");
        let mut ok_ring = DisplayConfig::default();
        ok_ring.log.live_ring = MIN_LIVE_RING;
        assert!(ok_ring.validate().is_ok(), "live_ring=100 须放行");

        // ④ 时延红线（设计 §4.2.1 约束 1/2/5）：逐项反例 + 上界正例
        assert!(err(DisplayConfig {
            alarm_poll_ms: 1001,
            ..Default::default()
        })
        .contains("alarm_poll_ms"));
        assert!(err(DisplayConfig {
            interlock_poll_ms: 1001,
            ..Default::default()
        })
        .contains("interlock_poll_ms"));
        assert!(err(DisplayConfig {
            device_poll_ms: 4001,
            ..Default::default()
        })
        .contains("device_poll_ms"));
        assert!(err(DisplayConfig {
            min_publish_interval_ms: 99,
            ..Default::default()
        })
        .contains("min_publish_interval_ms"));
    }

    /// mupc_core_config.yaml 的 `display:` 段字面量（设计 §4.9）必须整体可加载且自洽。
    #[test]
    fn design_section_4_9_display_yaml_literal_loads_and_validates() {
        // 与设计 §4.9 逐键一致（yaml 为 JSON 兼容子集，键名/语义同源）
        let json = r#"{
            "enabled": true,
            "bind_addr": "127.0.0.1:9810",
            "control_bind_addr": "127.0.0.1:9811",
            "publish_ms": 1000,
            "min_publish_interval_ms": 250,
            "device_poll_ms": 3000,
            "alarm_poll_ms": 500,
            "interlock_poll_ms": 500,
            "alarm_page_size": 10,
            "log": { "max_files": 5, "max_lines": 50000, "live_ring": 2000 }
        }"#;
        let c: DisplayConfig = serde_json::from_str(json).unwrap();
        assert!(c.enabled);
        assert_eq!(c.control_bind_addr, "127.0.0.1:9811");
        assert_eq!(c.min_publish_interval_ms, 250);
        assert_eq!(c.device_poll_ms, 3000);
        assert_eq!(c.alarm_poll_ms, 500);
        assert_eq!(c.interlock_poll_ms, 500);
        assert_eq!(c.alarm_page_size, 10);
        assert_eq!(c.log.max_files, 5);
        assert_eq!(c.log.max_lines, 50_000);
        assert_eq!(c.log.live_ring, 2000);
        assert!(c.validate().is_ok(), "设计 §4.9 样例配置须自洽: {:?}", c.validate());
    }
}
