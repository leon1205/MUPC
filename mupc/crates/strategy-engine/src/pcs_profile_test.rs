#[cfg(test)]
mod pcs_profile_test {
    use crate::pcs_profile::load_tai_storage_config;

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

    use crate::config::TaiStorageConfig;

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

    /// 写临时档位 YAML，返回路径
    fn write_tmp(content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tai_cap_profile_{}_{}.yaml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, content).unwrap();
        path
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn test_dual_profile_merge_ok() {
        // 顶行 key = pcs60_dual，无 tuning → L1/L2 落默认 60 档值
        let path = write_tmp(YAML_3_PROFILE);
        let cfg = load_tai_storage_config(Some(path.to_str().unwrap()), None).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(approx(cfg.i_rated, 110.0));
        assert!(approx(cfg.s_rated, 60.0));
        assert!(approx(cfg.dp_max, 25.0));
        assert!(approx(cfg.q_i_max, 25.0));
    }

    #[test]
    fn test_profile_key_cli_overrides_file_top() {
        // 文件顶行 pcs60_dual；CLI key=pcs80_kva 优先生效 → 80 档值
        let path = write_tmp(YAML_3_PROFILE);
        let cfg =
            load_tai_storage_config(Some(path.to_str().unwrap()), Some("pcs80_kva")).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(approx(cfg.i_rated, 133.0));
        assert!(approx(cfg.s_rated, 80.0));
        assert!(approx(cfg.dp_max, 26.7));
        assert!(approx(cfg.q_i_max, 26.7));
    }

    #[test]
    fn test_tuning_l3_and_tighten_override() {
        // tuning 收紧 dp_max=20/q_i_max=18（≤ 单相限 25），并覆盖 L3 字段
        let path = write_tmp(YAML_TUNED);
        let cfg = load_tai_storage_config(Some(path.to_str().unwrap()), None).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(approx(cfg.dp_max, 20.0));
        assert!(approx(cfg.q_i_max, 18.0));
        assert!(approx(cfg.p_abs_trig, 1.5));
        assert!(approx(cfg.soc_cap_day, 0.8));
        assert_eq!(cfg.window_size, 7);
        assert!(!cfg.s3_margin_limit);
        // L1 器件级不受 tuning 影响（恒取档位）
        assert!(approx(cfg.i_rated, 110.0));
        assert!(approx(cfg.s_rated, 60.0));
    }

    #[test]
    fn test_load_returns_tai_storage_config_type() {
        // 类型契约：返回值即策略引擎使用的 TaiStorageConfig
        let cfg = load_tai_storage_config(None, None).unwrap();
        let _: TaiStorageConfig = cfg;
    }
}
