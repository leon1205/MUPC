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
}
