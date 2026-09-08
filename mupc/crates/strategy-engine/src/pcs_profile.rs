//! 台区储能容量档位配置（capacity_profile，v2.24，设计文档 §2.10.2）
//!
//! TaiStorageConfig 本体不引入 serde；此处仅以薄结构（Deserialize）承接档位
//! YAML，启动时按当前 PCS 档位派生 L1 器件级参数 + L2 策略上限 + 可选 L3
//! tuning 覆盖。加载失败（缺文件/损坏/未知 key/has_neutral=true 等）= 启动
//! 中止（fail-fast），绝不静默落默认档进闭环。
//!
//! 优先级：`tuning > L1/L2 派生 > L3 代码 Default`。

use std::collections::HashMap;

use serde::Deserialize;

use crate::config::TaiStorageConfig;

/// 档位 YAML 顶层结构（deny_unknown_fields：防 YAML 键拼错被静默忽略）
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaiConfigFile {
    /// 当前部署档位 key（文件顶行；可被 CLI --capacity-profile 覆盖）
    capacity_profile: String,
    #[serde(default)]
    pcs_profiles: HashMap<String, PcsProfile>,
}

/// 单个 PCS 档位（L1 器件级参数）
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PcsProfile {
    desc: Option<String>,
    /// 有无中线：true = 带中线（当前仲裁已删中线判据 → 放行需仲裁扩展 + 驱动
    /// 点表确认双就绪，见 §2.10.2 M-1）
    has_neutral: bool,
    /// 单相有功硬限 (kW)：仅用于派生积分钳 dp_max（arbitrate 单相硬限由
    /// i_rated×U 电流钳 + s_rated 落实，不直接 clamp）
    phase_p_limit_kw: f64,
    /// 单相无功硬限 (kVAr)：用于派生积分钳 q_i_max
    phase_q_limit_kvar: f64,
    /// 单相电流限 (A)
    i_rated_a: f64,
    /// 总视在额定 (kVA)
    s_rated_kva: f64,
    /// 可选 L3 覆盖（None = 保持代码默认）
    #[serde(default)]
    tuning: Option<TuningOverrides>,
}

/// L3 控制标定覆盖（全 Option；None = 保持代码默认）
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TuningOverrides {
    dp_max: Option<f64>,  // L2 收紧（validate 须 ≤ phase_p_limit_kw）
    q_i_max: Option<f64>, // L2 收紧（validate 须 ≤ phase_q_limit_kvar）
    p_abs_trig: Option<f64>,
    p_dis_trig: Option<f64>,
    s1_exit: Option<f64>,
    p_tgt_s1: Option<f64>,
    p_tgt_s3: Option<f64>,
    p_cap: Option<f64>,
    slope: Option<f64>,
    kp: Option<f64>,
    k_diff: Option<f64>,
    k_q: Option<f64>,
    s_q_sign: Option<f64>,
    soc_cap_day: Option<f64>,
    soc_hys: Option<f64>,
    t_release_secs: Option<f64>,
    t_clear_start_secs: Option<f64>,
    t_clear_end_secs: Option<f64>,
    s4_limit_margin_kw: Option<f64>,
    s3_margin_limit: Option<bool>,
    s1_ff_step_kw: Option<f64>,
    window_size: Option<u32>,
    battery_capacity_kwh: Option<f64>,
}

/// 加载台区储能配置（设计 §2.10.2 加载 8 步）。
pub fn load_tai_storage_config(
    file: Option<&str>,
    profile_key: Option<&str>,
) -> Result<TaiStorageConfig, String> {
    let mut cfg = TaiStorageConfig::default();
    let path = file.map(str::trim).filter(|s| !s.is_empty());
    let Some(path) = path else {
        return Ok(cfg); // ② 未配置 = 默认档
    };

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("读取档位文件 {path} 失败: {e}"))?; // ③
    let f: TaiConfigFile = serde_yaml::from_str(&content)
        .map_err(|e| format!("解析档位文件 {path} 失败: {e}"))?; // ③（含缺字段）

    // ④ 选档：CLI key 优先于文件顶行
    let key = profile_key
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(f.capacity_profile.as_str());
    let p = f.pcs_profiles.get(key).ok_or_else(|| {
        let mut keys: Vec<&str> = f.pcs_profiles.keys().map(|s| s.as_str()).collect();
        keys.sort_unstable();
        format!("档位文件 {path}: 未知档位 key '{key}'；可用: [{}]", keys.join(", "))
    })?;

    // ⑤ 中线档防呆：当前仲裁无中线判据
    if p.has_neutral {
        return Err(format!(
            "档位 {key}（{path}）has_neutral=true：当前仲裁已删中线判据，须仲裁扩展 + PCS 驱动点表确认双就绪后方可放行（§2.10.2 M-1）"
        ));
    }

    // ⑥ L1/L2 合并：i_rated/s_rated 恒取 L1；dp_max/q_i_max 默认 = 对应单相限，tuning 可收紧
    cfg.i_rated = p.i_rated_a;
    cfg.s_rated = p.s_rated_kva;
    cfg.dp_max = p
        .tuning
        .as_ref()
        .and_then(|t| t.dp_max)
        .unwrap_or(p.phase_p_limit_kw);
    cfg.q_i_max = p
        .tuning
        .as_ref()
        .and_then(|t| t.q_i_max)
        .unwrap_or(p.phase_q_limit_kvar);

    // ⑦ tuning 覆盖其余 L3（dp_max/q_i_max 已在 ⑥ 处理，apply 跳过）
    if let Some(t) = &p.tuning {
        t.apply_l3(&mut cfg);
    }

    validate(key, &p, &cfg)?; // ⑧
    Ok(cfg)
}

impl TuningOverrides {
    fn apply_l3(&self, cfg: &mut TaiStorageConfig) {
        if let Some(v) = self.p_abs_trig {
            cfg.p_abs_trig = v;
        }
        if let Some(v) = self.p_dis_trig {
            cfg.p_dis_trig = v;
        }
        if let Some(v) = self.s1_exit {
            cfg.s1_exit = v;
        }
        if let Some(v) = self.p_tgt_s1 {
            cfg.p_tgt_s1 = v;
        }
        if let Some(v) = self.p_tgt_s3 {
            cfg.p_tgt_s3 = v;
        }
        if let Some(v) = self.p_cap {
            cfg.p_cap = v;
        }
        if let Some(v) = self.slope {
            cfg.slope = v;
        }
        if let Some(v) = self.kp {
            cfg.kp = v;
        }
        if let Some(v) = self.k_diff {
            cfg.k_diff = v;
        }
        if let Some(v) = self.k_q {
            cfg.k_q = v;
        }
        if let Some(v) = self.s_q_sign {
            cfg.s_q_sign = v;
        }
        if let Some(v) = self.soc_cap_day {
            cfg.soc_cap_day = v;
        }
        if let Some(v) = self.soc_hys {
            cfg.soc_hys = v;
        }
        if let Some(v) = self.t_release_secs {
            cfg.t_release_secs = v;
        }
        if let Some(v) = self.t_clear_start_secs {
            cfg.t_clear_start_secs = v;
        }
        if let Some(v) = self.t_clear_end_secs {
            cfg.t_clear_end_secs = v;
        }
        if let Some(v) = self.s4_limit_margin_kw {
            cfg.s4_limit_margin_kw = v;
        }
        if let Some(v) = self.s3_margin_limit {
            cfg.s3_margin_limit = v;
        }
        if let Some(v) = self.s1_ff_step_kw {
            cfg.s1_ff_step_kw = v;
        }
        if let Some(v) = self.window_size {
            cfg.window_size = v;
        }
        if let Some(v) = self.battery_capacity_kwh {
            cfg.battery_capacity_kwh = v;
        }
    }
}

/// 校验合并结果（§2.10.2 validate 规则）。p 供单相限上界，cfg 为合并后配置。
fn validate(key: &str, p: &PcsProfile, cfg: &TaiStorageConfig) -> Result<(), String> {
    for (field, v) in [
        ("phase_p_limit_kw", p.phase_p_limit_kw),
        ("phase_q_limit_kvar", p.phase_q_limit_kvar),
        ("i_rated_a", p.i_rated_a),
        ("s_rated_kva", p.s_rated_kva),
    ] {
        if v <= 0.0 {
            return Err(format!("档位 {key}: {field}={v} 须 > 0"));
        }
    }
    if !(cfg.dp_max > 0.0 && cfg.dp_max <= p.phase_p_limit_kw) {
        return Err(format!(
            "档位 {key}: dp_max={} 须满足 0 < dp_max ≤ phase_p_limit_kw={}",
            cfg.dp_max, p.phase_p_limit_kw
        ));
    }
    if !(cfg.q_i_max > 0.0 && cfg.q_i_max <= p.phase_q_limit_kvar) {
        return Err(format!(
            "档位 {key}: q_i_max={} 须满足 0 < q_i_max ≤ phase_q_limit_kvar={}",
            cfg.q_i_max, p.phase_q_limit_kvar
        ));
    }
    if cfg.p_cap <= 0.0 {
        return Err(format!("档位 {key}: p_cap={} 须 > 0", cfg.p_cap));
    }
    if !(0.0..=1.0).contains(&cfg.soc_cap_day) {
        return Err(format!("档位 {key}: soc_cap_day={} 须在 [0,1]", cfg.soc_cap_day));
    }
    if !(0.0..=1.0).contains(&cfg.soc_hys) {
        return Err(format!("档位 {key}: soc_hys={} 须在 [0,1]", cfg.soc_hys));
    }
    if cfg.window_size < 1 {
        return Err(format!("档位 {key}: window_size={} 须 ≥ 1", cfg.window_size));
    }
    Ok(())
}
