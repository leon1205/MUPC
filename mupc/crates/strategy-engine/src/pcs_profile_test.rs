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
}
