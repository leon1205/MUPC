#[cfg(test)]
mod tai_storage_test {
    use crate::config::TaiStorageConfig;
    use crate::tai_storage::{control, move_toward, MeterData, TaiControllerState, TaiState};

    fn meter(p: f64, pi: [f64; 3], qi: [f64; 3], u: [f64; 3], pfi: [f64; 3]) -> MeterData {
        MeterData {
            p,
            q: qi.iter().sum(),
            pf: pfi,
            u,
            // 带符号电流近似（≈P/U，符号一致，幅值差异用于触发不平衡）
            i: [pi[0], pi[1], pi[2]],
            p_i: pi,
            q_i: qi,
        }
    }

    #[test]
    fn test_s2_flat_no_output_when_normal() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 平衡负载（三相一致）→ unbal=0 → 差模不动作 → 输出 [0,0,0]
        let m = meter(6.0, [2.0, 2.0, 2.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let (p, q) = control(&mut st, &cfg, &m, 0.5, 3600 * 10); // 10:00
        assert_eq!(st.st, TaiState::S2Flat);
        assert_eq!(p, [0.0; 3]);
        assert_eq!(q, [0.0; 3]);
    }

    #[test]
    fn test_s1_absorb_on_reverse() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 白天 12:00，总表返送 -30kW（三相各有返送）
        let m = meter(
            -30.0,
            [-10.0, -10.0, -10.0],
            [0.0; 3],
            [220.0; 3],
            [0.99; 3],
        );
        let (p, _) = control(&mut st, &cfg, &m, 0.5, 3600 * 12);
        assert_eq!(st.st, TaiState::S1PvAbsorb);
        // 共模应充电（p_st < 0，分相 P 之和 = p_st）
        let sum: f64 = p.iter().sum();
        assert!(sum < 0.0, "S1 应充电吸收返送，p 之和={}", sum);
    }

    #[test]
    fn test_s3_discharge_on_high_load() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        let m = meter(50.0, [20.0, 15.0, 15.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let (p, _) = control(&mut st, &cfg, &m, 0.5, 3600 * 16);
        assert_eq!(st.st, TaiState::S3Peak);
        let sum: f64 = p.iter().sum();
        assert!(sum > 0.0, "S3 应放电，p 之和={}", sum);
    }

    #[test]
    fn test_dp_zero_net_energy() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 不平衡：B 相电流大（制造 unbal >25%）
        let m = meter(10.0, [2.0, 6.0, 2.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        st.d_p_active = true;
        for _ in 0..5 {
            let (p, _) = control(&mut st, &cfg, &m, 0.5, 3600 * 10);
            let sum: f64 = p.iter().sum();
            assert!(
                (sum - st.p_st).abs() < 1e-6,
                "ΣΔP 应守恒: sum={} p_st={}",
                sum,
                st.p_st
            );
        }
        let dsum: f64 = st.d_p.iter().sum();
        assert!(dsum.abs() < 1e-6, "ΣΔP 应=0: {}", dsum);
    }

    #[test]
    fn test_s4_force_discharge_to_soc_floor() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        // 22:00 已过清空起点，SOC=0.5 → S4 强制放电
        let m = meter(5.0, [2.0, 1.0, 2.0], [0.0; 3], [220.0; 3], [0.99; 3]);
        let (p, _) = control(&mut st, &cfg, &m, 0.5, 3600 * 22);
        assert_eq!(st.st, TaiState::S4Clear);
        let sum: f64 = p.iter().sum();
        assert!(sum > 0.0, "S4 应强制放电: {}", sum);
    }

    #[test]
    fn test_failsafe_nan_regresses_to_zero() {
        let cfg = TaiStorageConfig::default();
        let mut st = TaiControllerState::default();
        st.p_st = -40.0; // 之前充电
        let m = meter(f64::NAN, [f64::NAN; 3], [0.0; 3], [220.0; 3], [0.99; 3]);
        let (p, _) = control(&mut st, &cfg, &m, 0.5, 3600 * 10);
        assert!(st.p_st.abs() < 40.0, "failsafe 应斜坡回归: {}", st.p_st);
        assert!(p.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn test_move_toward_behavior() {
        assert_eq!(move_toward(10.0, 0.0, 5.0), 5.0);
        assert_eq!(move_toward(2.0, 0.0, 5.0), 0.0);
        assert_eq!(move_toward(-10.0, 0.0, 3.0), -7.0);
    }
}
