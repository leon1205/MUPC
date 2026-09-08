#[cfg(test)]
mod pcs_profile_test {
    use crate::config::TaiStorageConfig;
    use crate::pcs_profile::load_tai_storage_config;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 进程级自增序号，避免 SystemTime 墙钟节拍粗时的临时文件命名碰撞。
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn test_file_none_returns_default() {
        let cfg = load_tai_storage_config(None, None).unwrap();
        assert_eq!(cfg.i_rated, 110.0);
        assert_eq!(cfg.s_rated, 60.0);
        assert_eq!(cfg.dp_max, 25.0);
        assert_eq!(cfg.q_i_max, 25.0);
        assert_eq!(cfg.p_cap, 60.0);
    }

    #[test]
    fn test_file_empty_string_returns_default() {
        let cfg = load_tai_storage_config(Some("   "), Some("")).unwrap();
        assert_eq!(cfg.i_rated, 110.0);
        assert_eq!(cfg.s_rated, 60.0);
    }

    /// 三档 fixture：pcs60_dual（顶行，无中线，无 tuning）、pcs80_kva（无中线，
    /// 供 CLI key 覆盖）、pcs125_kva（has_neutral=true → 应 Err，Task 3 用）。
    const YAML_3_PROFILE: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "60kW 两级式 PCS（默认档，无 tuning）"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
  pcs80_kva:
    desc: "80kVA 无中线档（CLI key 覆盖用）"
    has_neutral: false
    phase_p_limit_kw: 26.7
    phase_q_limit_kvar: 26.7
    i_rated_a: 133
    s_rated_kva: 80
  pcs125_kva:
    desc: "125kVA（has_neutral=true → 应 Err）"
    has_neutral: true
    phase_p_limit_kw: 41.7
    phase_q_limit_kvar: 41.7
    i_rated_a: 190
    s_rated_kva: 125
"#;

    /// 带 tuning 的 60 档：收紧 dp_max + 覆盖若干 L3 字段
    const YAML_TUNED: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "60kW 双级式（tuning 收紧 dp_max + 覆盖 L3）"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      dp_max: 20
      q_i_max: 18
      p_abs_trig: 1.5
      soc_cap_day: 0.8
      window_size: 7
      s3_margin_limit: false
"#;

    /// 顶行选中合并 fixture：capacity_profile=pcs80_kva（无 CLI key → 取顶行 key 档位）。
    const YAML_TOP_PCS80: &str = r#"
capacity_profile: "pcs80_kva"
pcs_profiles:
  pcs80_kva:
    desc: "80kVA 无中线（顶行选中合并路径，值区别于默认 60 档）"
    has_neutral: false
    phase_p_limit_kw: 26.7
    phase_q_limit_kvar: 26.7
    i_rated_a: 133
    s_rated_kva: 80
"#;

    /// 写临时档位 YAML，返回路径
    fn write_tmp(content: &str) -> std::path::PathBuf {
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("tai_cap_profile_{}_{}.yaml", std::process::id(), seq));
        std::fs::write(&path, content).unwrap();
        path
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn test_top_profile_key_selected_and_merged() {
        // 顶行 capacity_profile=pcs80_kva 被选中合并（无 CLI key）→ 80 档值。
        // 数值与代码默认(110/60/25/25)可区分，能抓住"有文件却落默认档"的回归。
        let path = write_tmp(YAML_TOP_PCS80);
        let cfg = load_tai_storage_config(Some(path.to_str().unwrap()), None).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(approx(cfg.i_rated, 133.0), "i_rated={} expect 133", cfg.i_rated);
        assert!(approx(cfg.s_rated, 80.0), "s_rated={} expect 80", cfg.s_rated);
        assert!(approx(cfg.dp_max, 26.7), "dp_max={} expect 26.7", cfg.dp_max);
        assert!(approx(cfg.q_i_max, 26.7), "q_i_max={} expect 26.7", cfg.q_i_max);
    }

    #[test]
    fn test_profile_key_cli_overrides_file_top() {
        // 文件顶行 pcs60_dual；CLI key=pcs80_kva 优先生效 → 80 档值
        let path = write_tmp(YAML_3_PROFILE);
        let cfg =
            load_tai_storage_config(Some(path.to_str().unwrap()), Some("pcs80_kva")).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(approx(cfg.i_rated, 133.0), "i_rated={} expect 133", cfg.i_rated);
        assert!(approx(cfg.s_rated, 80.0), "s_rated={} expect 80", cfg.s_rated);
        assert!(approx(cfg.dp_max, 26.7), "dp_max={} expect 26.7", cfg.dp_max);
        assert!(approx(cfg.q_i_max, 26.7), "q_i_max={} expect 26.7", cfg.q_i_max);
    }

    #[test]
    fn test_tuning_l3_and_tighten_override() {
        // tuning 收紧 dp_max=20/q_i_max=18（≤ 单相限 25），并覆盖 L3 字段
        let path = write_tmp(YAML_TUNED);
        let cfg = load_tai_storage_config(Some(path.to_str().unwrap()), None).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(approx(cfg.dp_max, 20.0), "dp_max={} expect 20", cfg.dp_max);
        assert!(approx(cfg.q_i_max, 18.0), "q_i_max={} expect 18", cfg.q_i_max);
        assert!(approx(cfg.p_abs_trig, 1.5), "p_abs_trig={} expect 1.5", cfg.p_abs_trig);
        assert!(approx(cfg.soc_cap_day, 0.8), "soc_cap_day={} expect 0.8", cfg.soc_cap_day);
        assert_eq!(cfg.window_size, 7);
        assert!(!cfg.s3_margin_limit);
        // L1 器件级不受 tuning 影响（恒取档位）
        assert!(approx(cfg.i_rated, 110.0), "i_rated={} expect 110", cfg.i_rated);
        assert!(approx(cfg.s_rated, 60.0), "s_rated={} expect 60", cfg.s_rated);
    }

    #[test]
    fn test_load_returns_tai_storage_config_type() {
        // 类型契约：返回值即策略引擎使用的 TaiStorageConfig
        let cfg = load_tai_storage_config(None, None).unwrap();
        let _: TaiStorageConfig = cfg;
    }

    const YAML_UNKNOWN_KEY: &str = r#"
capacity_profile: "nope"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
"#;

    const YAML_PHASE_LIMIT_ZERO: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 0
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
"#;

    const YAML_I_RATED_NAN: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: .nan
    s_rated_kva: 60
"#;

    const YAML_TUNING_DP_EXCEED: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      dp_max: 30
"#;

    const YAML_TUNING_Q_EXCEED: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      q_i_max: 30
"#;

    const YAML_TUNING_P_CAP_ZERO: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      p_cap: 0
"#;

    const YAML_TUNING_SOC_CAP_OUT: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      soc_cap_day: 1.5
"#;

    const YAML_TUNING_SOC_HYS_NEG: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      soc_hys: -0.1
"#;

    const YAML_TUNING_WINDOW_ZERO: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      window_size: 0
"#;

    const YAML_UNKNOWN_PROFILE_FIELD: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limt_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
"#;

    const YAML_UNKNOWN_TUNING_FIELD: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      soc_cap_dayy: 0.8
"#;

    const YAML_MALFORMED: &str = "capacity_profile: [unclosed\npcs_profiles: {";

    /// 写临时档位 → 加载（带 CLI key）→ 尽力清理 → 返回结果（失败路径也清理，
    /// 避免 unwrap_err panic 时漏删临时文件）
    fn load_tmp(content: &str) -> Result<TaiStorageConfig, String> {
        load_tmp_with_key(content, None)
    }

    fn load_tmp_with_key(content: &str, key: Option<&str>) -> Result<TaiStorageConfig, String> {
        let path = write_tmp(content);
        let r = load_tai_storage_config(Some(path.to_str().unwrap()), key);
        let _ = std::fs::remove_file(&path);
        r
    }

    #[test]
    fn test_missing_file_err() {
        // fail-fast：文件不存在 → Err（绝不静默落默认档）
        let e = load_tai_storage_config(Some("/nonexistent/tai_profiles.yaml"), None)
            .unwrap_err();
        assert!(e.contains("读取档位文件"), "实际: {e}");
        assert!(
            e.contains("/nonexistent/tai_profiles.yaml"),
            "read 阶段错误应含文件路径: {e}"
        );
    }

    #[test]
    fn test_malformed_yaml_err() {
        let e = load_tmp(YAML_MALFORMED).unwrap_err();
        assert!(e.contains("解析档位文件"), "实际: {e}");
    }

    #[test]
    fn test_unknown_top_profile_key_err_lists_keys() {
        // 文件顶行 key='nope' 未知 → Err 且附可用键列表
        let e = load_tmp(YAML_UNKNOWN_KEY).unwrap_err();
        assert!(e.contains("未知档位 key 'nope'"), "实际: {e}");
        assert!(e.contains("pcs60_dual"), "应附可用键列表: {e}");
    }

    #[test]
    fn test_unknown_cli_key_err_lists_keys() {
        // CLI key 未知 → 同 Err（用 YAML_3_PROFILE 覆盖顶行）
        let e = load_tmp_with_key(YAML_3_PROFILE, Some("pcs200_kva")).unwrap_err();
        assert!(e.contains("未知档位 key 'pcs200_kva'"), "实际: {e}");
        assert!(
            e.contains("pcs60_dual") && e.contains("pcs80_kva") && e.contains("pcs125_kva"),
            "应附全部可用键: {e}"
        );
    }

    #[test]
    fn test_has_neutral_true_err() {
        // has_neutral=true → Err（当前仲裁无中线判据）
        let e = load_tmp_with_key(YAML_3_PROFILE, Some("pcs125_kva")).unwrap_err();
        assert!(e.contains("has_neutral=true"), "实际: {e}");
    }

    #[test]
    fn test_phase_limit_zero_err() {
        // 器件级 <=0 → Err（文案为"须为有限正数"）
        let e = load_tmp(YAML_PHASE_LIMIT_ZERO).unwrap_err();
        assert!(e.contains("phase_p_limit_kw"), "实际: {e}");
        assert!(e.contains("须为有限正数"), "实际: {e}");
    }

    #[test]
    fn test_i_rated_nan_rejected() {
        // 非有限值（YAML .nan）→ Err（Task 1 非有限值拦截）
        let e = load_tmp(YAML_I_RATED_NAN).unwrap_err();
        assert!(e.contains("i_rated_a"), "实际: {e}");
        assert!(e.contains("须为有限正数"), "实际: {e}");
    }

    #[test]
    fn test_tuning_dp_max_exceed_phase_err() {
        // tuning 越界：dp_max=30 > phase_p_limit_kw=25 → Err
        let e = load_tmp(YAML_TUNING_DP_EXCEED).unwrap_err();
        assert!(e.contains("dp_max"), "实际: {e}");
        assert!(e.contains("≤ phase_p_limit_kw=25"), "实际: {e}");
    }

    #[test]
    fn test_tuning_q_i_max_exceed_phase_err() {
        // tuning 越界：q_i_max=30 > phase_q_limit_kvar=25 → Err
        let e = load_tmp(YAML_TUNING_Q_EXCEED).unwrap_err();
        assert!(e.contains("q_i_max"), "实际: {e}");
        assert!(e.contains("≤ phase_q_limit_kvar=25"), "实际: {e}");
    }

    #[test]
    fn test_tuning_p_cap_zero_err() {
        // tuning 非有限正数：p_cap=0 → Err
        let e = load_tmp(YAML_TUNING_P_CAP_ZERO).unwrap_err();
        assert!(e.contains("p_cap"), "实际: {e}");
        assert!(e.contains("须为有限正数"), "实际: {e}");
    }

    #[test]
    fn test_tuning_soc_cap_day_out_of_range_err() {
        // tuning 越界：soc_cap_day=1.5 超出 [0,1] → Err
        let e = load_tmp(YAML_TUNING_SOC_CAP_OUT).unwrap_err();
        assert!(e.contains("soc_cap_day"), "实际: {e}");
        assert!(e.contains("须在 [0,1]"), "实际: {e}");
    }

    #[test]
    fn test_tuning_soc_hys_negative_err() {
        // tuning 越界：soc_hys=-0.1 落在 [0,1] 外 → Err
        let e = load_tmp(YAML_TUNING_SOC_HYS_NEG).unwrap_err();
        assert!(e.contains("soc_hys"), "实际: {e}");
        assert!(e.contains("须在 [0,1]"), "实际: {e}");
    }

    #[test]
    fn test_tuning_window_size_zero_err() {
        // tuning 非法：window_size=0 < 1 → Err
        let e = load_tmp(YAML_TUNING_WINDOW_ZERO).unwrap_err();
        assert!(e.contains("window_size"), "实际: {e}");
        assert!(e.contains("须 ≥ 1"), "实际: {e}");
    }

    #[test]
    fn test_deny_unknown_profile_field_err() {
        // 档内未知键（拼错 phase_p_limit_kw）→ deny_unknown_fields Err
        let e = load_tmp(YAML_UNKNOWN_PROFILE_FIELD).unwrap_err();
        assert!(e.contains("解析档位文件"), "实际: {e}");
        assert!(e.contains("phase_p_limt_kw"), "应提示未知键: {e}");
    }

    #[test]
    fn test_deny_unknown_tuning_field_err() {
        // tuning 内未知键（拼错 soc_cap_day）→ Err
        let e = load_tmp(YAML_UNKNOWN_TUNING_FIELD).unwrap_err();
        assert!(e.contains("解析档位文件"), "实际: {e}");
        assert!(e.contains("soc_cap_dayy"), "应提示未知键: {e}");
    }

    const YAML_TUNING_P_CAP_FOLLOW: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "tuning p_cap=80 未给 s1_ff_step_kw"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      p_cap: 80
"#;

    const YAML_TUNING_P_CAP_EXPLICIT_S1FF: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "tuning p_cap=80 且显式 s1_ff_step_kw=70"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      p_cap: 80
      s1_ff_step_kw: 70
"#;

    #[test]
    fn test_s1_ff_step_follows_p_cap_when_not_tuned() {
        // tuning 仅覆盖 p_cap → s1_ff_step_kw 自动跟随合并后 p_cap（文档不变量）
        let path = write_tmp(YAML_TUNING_P_CAP_FOLLOW);
        let cfg = load_tai_storage_config(Some(path.to_str().unwrap()), None).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.p_cap, 80.0);
        assert_eq!(cfg.s1_ff_step_kw, 80.0);
    }

    #[test]
    fn test_s1_ff_step_explicit_tuning_wins() {
        // tuning 显式给 s1_ff_step_kw → 保留 tuning 值（不跟随 p_cap）
        let path = write_tmp(YAML_TUNING_P_CAP_EXPLICIT_S1FF);
        let cfg = load_tai_storage_config(Some(path.to_str().unwrap()), None).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(cfg.p_cap, 80.0);
        assert_eq!(cfg.s1_ff_step_kw, 70.0);
    }
}
