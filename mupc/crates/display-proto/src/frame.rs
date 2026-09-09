//! 帧数据模型（跨进程契约）——与设计文档 §3.3 逐字段对齐（`[DESIGN_APPROVED]`）。
//!
//! 语义要点（摘自设计）：
//! - 本文件是 mupcd（DisplayDataProvider 发布方）与 mupc-local-display（渲染订阅方）
//!   以及测试桩共享的**帧契约单一真源**。
//! - 所有数值展示仅当对应 `FieldFlag == Valid`；源不可得一律显式打标，禁止补 0 / 沿用
//!   陈旧值冒充实时（PRD "不造假值"）。
//! - `run_state` 在帧内值域保证 0..=3（`RunState` 枚举 + `from_raw` 越界滤除），渲染端
//!   `Option<RunState>` match 穷尽四态 + None 即可，无枚举外值分支。

/// 帧协议版本。
pub const PROTO_VERSION: u8 = 1;

/// 渲染端判「数据过期」阈值（PRD F5.3：当前时间 − 帧时间戳 > 2s 判过期）。
pub const DEFAULT_STALE_MS: u64 = 2000;

/// 点级字段有效/降级标志。渲染端据 flag 决定显示数值或 `--` + 对应角标。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldFlag {
    /// 正常展示。
    Valid,
    /// "未取数"（点表/采集未覆盖，PRD 6.4）。
    NotRead,
    /// "源离线"（PCS 离线/核间读失败，PRD 6.1）。
    Offline,
    /// "数据异常"（域值化量程/有限性校验不过，PRD 6.5）。
    RangeError,
}

/// 单数值字段（工程值 + 点级 valid 标志）。
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Field {
    /// 工程值；`flag != Valid` 时通常 `None`（不补 0，PRD 不造假值）。
    pub v: Option<f64>,
    /// 点级有效/降级标志。
    pub flag: FieldFlag,
}

/// 运行状态（F2，主判据 REG1013）。枚举化保证值域 0..=3：
/// **越界态在帧内不可达**——采集侧心跳已将 1013 越界读数按坏读数滤除（改判离线）。
/// JSON 以判别数 u8 传输（如 `"run_state": 2`），serde 以手写 u8 映射实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RunState {
    /// 停机。
    Stop = 0,
    /// 待机。
    Standby = 1,
    /// 充电。
    Charge = 2,
    /// 放电。
    Discharge = 3,
}

impl RunState {
    /// 由原始寄存器读数（REG1013 UInt16）构造；越界（>3）返回 `None`（采集侧滤除语义）。
    pub fn from_raw(raw: u16) -> Option<Self> {
        match raw {
            0 => Some(Self::Stop),
            1 => Some(Self::Standby),
            2 => Some(Self::Charge),
            3 => Some(Self::Discharge),
            _ => None,
        }
    }

    /// UI 展示名（F2 文字态；色板由渲染 layout 决定，此处仅给文字）。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Stop => "停机",
            Self::Standby => "待机",
            Self::Charge => "充电",
            Self::Discharge => "放电",
        }
    }
}

impl std::fmt::Display for RunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

impl serde::Serialize for RunState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // JSON 以判别数 u8 传输（设计 §3.3，如 `"run_state": 2`）。
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> serde::Deserialize<'de> for RunState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = u16::deserialize(deserializer)?;
        Self::from_raw(raw)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid RunState raw value: {raw}")))
    }
}

/// SOC 展示源标注（三态，对齐 UI 源标签）。不含「双源一致」态——SOC 源裁决是
/// 「优先级+回落」的**单源化**（Bms fresh → Bms；否则活读核间 REG1010 → PcsReg1010；
/// 双失 → Lost），任一时该值实际取值源唯一（设计 §3.3 末落地解释）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SocSource {
    /// BMS 站 SOC（优先源）。
    Bms,
    /// 核间活读 PCS REG1010 SOC（回落源）。
    PcsReg1010,
    /// 双源皆失/无 fresh 源（"SOC 源失效"，警示胶囊）。
    Lost,
}

impl SocSource {
    /// UI 源标签展示名（设计 §6.3 F1 行：源胶囊 BMS / PCS / 失效）。
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Bms => "BMS",
            Self::PcsReg1010 => "PCS(REG1010)",
            Self::Lost => "SOC 源失效",
        }
    }
}

impl std::fmt::Display for SocSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

/// 一帧展示数据（每 1s 由 mupcd 域值化后发布）。字段名/语义与设计 §3.3 逐字对齐。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DisplayFrame {
    /// = `PROTO_VERSION`。
    pub version: u8,
    /// 单调递增发布序号（重启清零；渲染端判连续/重排）。
    pub seq: u64,
    /// 域值化时刻（Unix 毫秒；新鲜度判据 F5.3）。
    pub ts_ms: u64,
    /// F1: 裁决后 SOC(%)。`None` ↔ `soc_source = Lost`（双源皆失）→ 屏显 `--` + "SOC 源失效"，禁沿用旧值。
    pub soc: Option<f64>,
    /// F1: 三态源标注；渲染据此画源胶囊（Lost → 警示色）。
    pub soc_source: SocSource,
    /// F1: 所用源点级异常（如 PCS 源 RangeError）辅助。
    pub soc_flag: FieldFlag,
    /// F2: 运行状态（1013 主判据；值域保证 0..=3）。`None` = PCS 离线/心跳无有效态（PRD 6.1）。
    pub run_state: Option<RunState>,
    /// 核间 modbus 链路在线（心跳维护；PCS 离线整体提示 6.1）。
    pub pcs_online: bool,
    /// F3: 三相有功(kW) + 设备总有功(kW)——已 ×0.1，渲染端不再换算。`[A, B, C]`。
    pub p_phase: [Field; 3],
    /// F3: 设备总有功(kW)。
    pub p_total: Field,
    /// F4: 三相电流(A)——已 ×0.1。`[A, B, C]`。
    pub i_phase: [Field; 3],
    /// 6.6 佐证一致性: 1013(充/放) 与 Σp_phase 方向显著反向 → true（渲染端加"方向不一致"角标，主状态仍以 1013 展示）。
    pub inconsistency: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- RunState：from_raw 越界 → None，合法值 → 对应变体 ----
    #[test]
    fn run_state_from_raw_valid_values() {
        assert_eq!(RunState::from_raw(0), Some(RunState::Stop));
        assert_eq!(RunState::from_raw(1), Some(RunState::Standby));
        assert_eq!(RunState::from_raw(2), Some(RunState::Charge));
        assert_eq!(RunState::from_raw(3), Some(RunState::Discharge));
    }

    #[test]
    fn run_state_from_raw_out_of_range_is_none() {
        assert_eq!(RunState::from_raw(4), None);
        assert_eq!(RunState::from_raw(0xFFFF), None);
        // u16 上限
        assert_eq!(RunState::from_raw(65535), None);
    }

    #[test]
    fn run_state_display_name_matches_prd_words() {
        assert_eq!(RunState::Stop.display_name(), "停机");
        assert_eq!(RunState::Standby.display_name(), "待机");
        assert_eq!(RunState::Charge.display_name(), "充电");
        assert_eq!(RunState::Discharge.display_name(), "放电");
    }

    // ---- RunState：JSON 以判别数 u8 传输（设计 §3.3）----
    #[test]
    fn run_state_json_roundtrip_as_number() {
        assert_eq!(serde_json::to_string(&RunState::Charge).unwrap(), "2");
        assert_eq!(serde_json::to_string(&RunState::Stop).unwrap(), "0");
        let back: RunState = serde_json::from_str("3").unwrap();
        assert_eq!(back, RunState::Discharge);
    }

    #[test]
    fn run_state_json_out_of_range_rejected() {
        let err = serde_json::from_str::<RunState>("7").unwrap_err();
        assert!(err.to_string().contains("invalid RunState"));
    }

    // ---- SocSource：JSON snake_case + UI 展示名 ----
    #[test]
    fn soc_source_json_snake_case() {
        assert_eq!(
            serde_json::to_string(&SocSource::PcsReg1010).unwrap(),
            "\"pcs_reg1010\""
        );
        assert_eq!(serde_json::to_string(&SocSource::Bms).unwrap(), "\"bms\"");
        assert_eq!(serde_json::to_string(&SocSource::Lost).unwrap(), "\"lost\"");
    }

    #[test]
    fn soc_source_display_name() {
        assert_eq!(SocSource::Bms.display_name(), "BMS");
        assert_eq!(SocSource::PcsReg1010.display_name(), "PCS(REG1010)");
        assert_eq!(SocSource::Lost.display_name(), "SOC 源失效");
    }

    // ---- FieldFlag：JSON snake_case ----
    #[test]
    fn field_flag_json_snake_case() {
        assert_eq!(serde_json::to_string(&FieldFlag::Valid).unwrap(), "\"valid\"");
        assert_eq!(
            serde_json::to_string(&FieldFlag::NotRead).unwrap(),
            "\"not_read\""
        );
        assert_eq!(
            serde_json::to_string(&FieldFlag::Offline).unwrap(),
            "\"offline\""
        );
        assert_eq!(
            serde_json::to_string(&FieldFlag::RangeError).unwrap(),
            "\"range_error\""
        );
    }

    // ---- Field：None v 序列化为 null ----
    #[test]
    fn field_with_none_v_serializes_null() {
        let f = Field {
            v: None,
            flag: FieldFlag::NotRead,
        };
        assert_eq!(
            serde_json::to_string(&f).unwrap(),
            r#"{"v":null,"flag":"not_read"}"#
        );
    }

    // ---- DisplayFrame：JSON 全往返（含 Option None）----
    /// 设计 §3.3 JSON 示例帧（run_state 为数值 2、soc 65.0）。
    const SAMPLE_JSON: &str = r#"{
        "version": 1,
        "seq": 123,
        "ts_ms": 1757412000000,
        "soc": 65.0,
        "soc_source": "pcs_reg1010",
        "soc_flag": "valid",
        "run_state": 2,
        "pcs_online": true,
        "p_phase": [ {"v":12.3,"flag":"valid"}, {"v":11.8,"flag":"valid"}, {"v":12.0,"flag":"valid"} ],
        "p_total": {"v":36.1,"flag":"valid"},
        "i_phase": [ {"v":22.5,"flag":"valid"}, {"v":22.1,"flag":"valid"}, {"v":22.3,"flag":"valid"} ],
        "inconsistency": false
    }"#;

    #[test]
    fn display_frame_roundtrip_sample() {
        let f: DisplayFrame = serde_json::from_str(SAMPLE_JSON).unwrap();
        // 字段语义断言
        assert_eq!(f.version, PROTO_VERSION);
        assert_eq!(f.seq, 123);
        assert_eq!(f.soc, Some(65.0));
        assert_eq!(f.soc_source, SocSource::PcsReg1010);
        assert_eq!(f.soc_flag, FieldFlag::Valid);
        assert_eq!(f.run_state, Some(RunState::Charge));
        assert!(f.pcs_online);
        assert!(!f.inconsistency);
        assert_eq!(f.p_phase[0].v, Some(12.3));
        assert_eq!(f.i_phase[2].v, Some(22.3));
        // 编码后回解码 == 原帧（稳定往返）
        let encoded = serde_json::to_string(&f).unwrap();
        let dec: DisplayFrame = serde_json::from_str(&encoded).unwrap();
        assert_eq!(dec, f);
    }

    #[test]
    fn display_frame_none_fields_roundtrip() {
        // 全降级态：soc=None/soc_source=lost、run_state=None、数值 v 全 None
        let f = DisplayFrame {
            version: PROTO_VERSION,
            seq: 0,
            ts_ms: 0,
            soc: None,
            soc_source: SocSource::Lost,
            soc_flag: FieldFlag::Offline,
            run_state: None,
            pcs_online: false,
            p_phase: [Field {
                v: None,
                flag: FieldFlag::Offline,
            }; 3],
            p_total: Field {
                v: None,
                flag: FieldFlag::NotRead,
            },
            i_phase: [Field {
                v: None,
                flag: FieldFlag::Offline,
            }; 3],
            inconsistency: false,
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"soc\":null"));
        assert!(json.contains("\"soc_source\":\"lost\""));
        assert!(json.contains("\"run_state\":null"));
        let back: DisplayFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn display_frame_unknown_fields_tolerated() {
        // 设计 §10.2：未知字段容忍（前向兼容），结构体默认忽略未知键。
        let json = r#"{"version":1,"seq":1,"ts_ms":1,"soc":null,"soc_source":"lost",
            "soc_flag":"offline","run_state":null,"pcs_online":false,
            "p_phase":[{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"}],
            "p_total":{"v":null,"flag":"not_read"},
            "i_phase":[{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"}],
            "inconsistency":false,"future_field":123}"#;
        let f: DisplayFrame = serde_json::from_str(json).unwrap();
        assert_eq!(f.soc_source, SocSource::Lost);
    }
}
