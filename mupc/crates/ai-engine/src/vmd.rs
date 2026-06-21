//! VMD (Variational Mode Decomposition) 变分模态分解
//!
//! 纯 Rust 实现，基于 ADMM 迭代求解 K 个本征模态函数 (IMF)。
//!
//! **依赖：** `rustfft`（FFT/IFFT）+ `nalgebra`（矩阵运算）
//!
//! **参考论文：** Dragomiretskiy & Zosso (2014), "Variational Mode Decomposition"
//!
//! **性能约束：** 单次 VMD 分解（N=24, K<=8）<= 50ms

use crate::error::AiEngineError;
use rustfft::{num_complex::Complex64, FftPlanner};

/// VMD tau 默认值（与 `pipeline_config::default_vmd_tau` 保持一致）
pub(crate) const DEFAULT_VMD_TAU: f64 = 0.0;

// ============================================================================
// VmdConfig -- VMD 分解参数
// ============================================================================

/// VMD 分解配置
#[derive(Debug, Clone)]
pub struct VmdConfig {
    /// 模态数 K
    pub k: usize,
    /// 惩罚因子（带宽约束），典型值 2000
    pub alpha: f64,
    /// 噪声容忍度（Lagrangian 更新步长），0.0 表示不做双升更新
    pub tau: f64,
    /// 收敛容差
    pub tol: f64,
    /// 最大迭代次数
    pub max_iter: usize,
}

impl VmdConfig {
    /// 创建光伏 VMD 的默认配置
    pub fn default_pv() -> Self {
        Self {
            k: 5,
            alpha: 2000.0,
            tau: DEFAULT_VMD_TAU,
            tol: 1.0e-6,
            max_iter: 500,
        }
    }

    /// 创建负荷 VMD 的默认配置
    pub fn default_load() -> Self {
        Self {
            k: 6,
            alpha: 2000.0,
            tau: DEFAULT_VMD_TAU,
            tol: 1.0e-6,
            max_iter: 500,
        }
    }
}

// ============================================================================
// VmdResult -- VMD 分解结果
// ============================================================================

/// 单次 VMD 分解结果
#[derive(Debug, Clone)]
pub struct VmdResult {
    /// K 个子模态，每个长度 = 输入信号长度
    pub imfs: Vec<Vec<f32>>,
    /// 重构序列（所有 IMF 求和，时域）
    pub reconstructed: Vec<f32>,
    /// 重构误差 (RMSE)
    pub reconstruction_error: f64,
    /// 实际迭代次数
    pub iterations: usize,
    /// 是否收敛
    pub converged: bool,
}

// ============================================================================
// VmdDecomposer -- VMD 分解器
// ============================================================================

/// VMD 分解器
///
/// 持有固定的分解参数，可安全并发复用（内部无状态）。
///
/// # 使用示例
///
/// ```ignore
/// let config = VmdConfig::default_pv();
/// let decomposer = VmdDecomposer::new(config);
/// let result = decomposer.decompose(&pv_signal)?;
/// ```
pub struct VmdDecomposer {
    config: VmdConfig,
}

impl VmdDecomposer {
    /// 创建 VMD 分解器
    pub fn new(config: VmdConfig) -> Self {
        Self { config }
    }

    /// 执行 VMD 分解
    ///
    /// # 参数
    /// - `signal`: 输入时间序列 x(t)，长度 >= 4
    ///
    /// # 返回
    /// - `VmdResult`: K 个子模态 + 重构序列 + 元信息
    ///
    /// # 错误
    /// - `VmdFailed`：信号长度不足、含 NaN/Inf
    /// - `VmdNotConverged`：达到最大迭代次数仍未收敛
    ///
    /// 注意：此函数为 CPU 密集型，调用方应使用 `tokio::task::spawn_blocking` 包装。
    pub fn decompose(&self, signal: &[f32]) -> Result<VmdResult, AiEngineError> {
        // --- 输入校验 ---
        let n = signal.len();
        if n < 4 {
            return Err(AiEngineError::VmdFailed(format!(
                "信号长度 {} < 4，无法进行 VMD 分解",
                n
            )));
        }
        if self.config.k < 1 || self.config.k > n {
            return Err(AiEngineError::VmdFailed(format!(
                "模态数 K={} 不合法（要求 1 <= K <= {}）",
                self.config.k, n
            )));
        }
        for (i, &v) in signal.iter().enumerate() {
            if v.is_nan() || v.is_infinite() {
                return Err(AiEngineError::VmdFailed(format!(
                    "信号在索引 {} 处含 NaN/Inf",
                    i
                )));
            }
        }

        // --- 转换为 f64 进行内部计算 ---
        let signal_f64: Vec<f64> = signal.iter().map(|&x| x as f64).collect();

        // --- Step 1: FFT ---
        let f_hat = real_fft(&signal_f64);

        // --- Step 2: 初始化 ---
        let h = f_hat.len(); // N/2 + 1 (half-spectrum)
        let k = self.config.k;

        // u_hat[k][freq] -- K 个模态在频域的表示（半频谱）
        let f_hat_div_k: Vec<Complex64> = f_hat.iter().map(|&x| x / (k as f64)).collect();
        let mut u_hat: Vec<Vec<Complex64>> = (0..k).map(|_| f_hat_div_k.clone()).collect();

        // omega_k -- 各模态的中心频率（角频率，rad/sample，范围 [0, pi]）
        let pi = std::f64::consts::PI;
        let mut omega_k: Vec<f64> = if k == 1 {
            vec![pi / 2.0]
        } else {
            (0..k)
                .map(|i| 0.1 * pi + 0.8 * pi * (i as f64) / ((k - 1) as f64))
                .collect()
        };

        // lambda_hat -- Lagrangian 乘子（半频谱）
        let mut lambda_hat: Vec<Complex64> = vec![Complex64::new(0.0, 0.0); h];

        // 频率轴（角频率 rad/sample，0 到 pi，只包含半频谱）
        let omega_axis: Vec<f64> = (0..h).map(|i| 2.0 * pi * (i as f64) / (n as f64)).collect();

        let alpha = self.config.alpha;
        let tau = self.config.tau;
        let tol = self.config.tol;
        let max_iter = self.config.max_iter;

        // --- Step 3: ADMM 迭代 ---
        let mut converged = false;
        let mut final_iter = max_iter;

        // H-03: tau=0 时保存上一轮 u_hat 用于真实收敛判定
        let mut u_hat_prev: Vec<Vec<Complex64>> = vec![];

        for iter in 0..max_iter {
            // 保存上一轮 u_hat（仅 tau=0 时需要用于收敛检查）
            if tau == 0.0 {
                u_hat_prev = u_hat.iter().map(|u| u.clone()).collect();
            }

            for k_idx in 0..k {
                // --- 更新 u_k（Wiener 滤波，频域） ---
                for i in 0..h {
                    // 残差 = f_hat + lambda/2 - sum_{j != k} u_j
                    let mut residual = f_hat[i] + lambda_hat[i] * 0.5;
                    for j in 0..k {
                        if j != k_idx {
                            residual -= u_hat[j][i];
                        }
                    }

                    // Wiener 滤波分母：1 + 2*alpha*(omega - omega_k)^2
                    let delta = omega_axis[i] - omega_k[k_idx];
                    let denom = 1.0 + 2.0 * alpha * delta * delta;
                    u_hat[k_idx][i] = residual / denom;
                }

                // --- 更新 omega_k（重心法） ---
                let mut num = 0.0_f64;
                let mut den = 0.0_f64;
                for i in 0..h {
                    let weight = u_hat[k_idx][i].norm_sqr();
                    // DC 和 Nyquist 频率权重折半（标准处理）
                    let freq_weight = if i == 0 || i == h - 1 {
                        weight * 0.5
                    } else {
                        weight
                    };
                    num += omega_axis[i] * freq_weight;
                    den += freq_weight;
                }
                if den > 1e-12 {
                    omega_k[k_idx] = num / den;
                }
                // else: 保持上一轮 omega_k
            }

            // --- 更新 lambda（双升，频域） ---
            if tau > 0.0 {
                for i in 0..h {
                    let mut sum_u = Complex64::new(0.0, 0.0);
                    for k_idx in 0..k {
                        sum_u += u_hat[k_idx][i];
                    }
                    lambda_hat[i] += tau * (f_hat[i] - sum_u);
                }
            }

            // --- 检查收敛 ---
            if tau > 0.0 {
                // 使用 lambda 更新幅度作为收敛判据
                let lambda_norm: f64 = lambda_hat.iter().map(|x| x.norm_sqr()).sum();
                if lambda_norm < tol * (n as f64) && iter > 10 {
                    converged = true;
                    final_iter = iter + 1;
                    break;
                }
            } else if !u_hat_prev.is_empty() {
                // H-03: tau=0 时使用 u_hat 相邻迭代相对变化判定真实收敛
                // N=24 时计算量可忽略，避免伪收敛导致 VMD-06 诊断标志失效
                let mut total_change = 0.0_f64;
                let mut total_norm = 0.0_f64;
                for k_idx in 0..k {
                    for i in 0..h {
                        let diff = u_hat[k_idx][i] - u_hat_prev[k_idx][i];
                        total_change += diff.norm_sqr();
                        total_norm += u_hat_prev[k_idx][i].norm_sqr();
                    }
                }
                if total_norm > 1e-12 {
                    let relative_change = (total_change / total_norm).sqrt();
                    if relative_change < tol {
                        converged = true;
                        final_iter = iter + 1;
                        break;
                    }
                }
            }

            final_iter = iter + 1;
        }

        // --- Step 4: IFFT 重构时域模态 ---
        let mut imfs: Vec<Vec<f32>> = Vec::with_capacity(k);
        let mut reconstructed: Vec<f32> = vec![0.0_f32; n];

        for k_idx in 0..k {
            let full_spectrum = half_to_full_spectrum(&u_hat[k_idx], n);
            let imf_complex = ifft(&full_spectrum);
            // L-01: 实信号 IFFT 虚部应接近零（共轭对称性保证）
            debug_assert!(
                imf_complex
                    .iter()
                    .all(|c| c.im.abs() < 1e-10 * (c.re.abs() + 1e-15)),
                "IFFT 虚部异常: K={}, max|im|={}",
                k_idx,
                imf_complex
                    .iter()
                    .map(|c| c.im.abs())
                    .fold(0.0_f64, f64::max)
            );
            // IFFT 后除以 N（rustfft 不做归一化）
            let imf: Vec<f32> = imf_complex
                .iter()
                .map(|c| (c.re / (n as f64)) as f32)
                .collect();

            // 累加到重构序列
            for i in 0..n {
                reconstructed[i] += imf[i];
            }
            imfs.push(imf);
        }

        // --- Step 5: 计算重构误差 (RMSE) ---
        let mse: f64 = signal
            .iter()
            .zip(reconstructed.iter())
            .map(|(&orig, &rec)| {
                let diff = (orig - rec) as f64;
                diff * diff
            })
            .sum::<f64>()
            / (n as f64);
        let reconstruction_error = mse.sqrt();

        // --- Step 6: 收敛失败告警（但不拒绝结果） ---
        if !converged {
            tracing::warn!(
                "VMD 分解未收敛 (max_iter={}, n={}, K={}, alpha={})",
                max_iter,
                n,
                k,
                alpha
            );
        }

        Ok(VmdResult {
            imfs,
            reconstructed,
            reconstruction_error,
            iterations: final_iter,
            converged,
        })
    }

    /// 获取配置引用
    pub fn config(&self) -> &VmdConfig {
        &self.config
    }
}

// ============================================================================
// 内部辅助函数
// ============================================================================

/// 实数信号的前向 FFT，返回半频谱（前 N/2+1 个复数）
fn real_fft(signal: &[f64]) -> Vec<Complex64> {
    let n = signal.len();
    let mut buffer: Vec<Complex64> = signal.iter().map(|&x| Complex64::new(x, 0.0)).collect();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n);
    fft.process(&mut buffer);

    // 提取半频谱：前 N/2 + 1 个频率分量
    let h = n / 2 + 1;
    buffer.into_iter().take(h).collect()
}

/// 半频谱扩展为全频谱（利用共轭对称性）
fn half_to_full_spectrum(half: &[Complex64], n: usize) -> Vec<Complex64> {
    let h = half.len();
    let mut full = vec![Complex64::new(0.0, 0.0); n];
    // 复制前半部分（正频率 + DC + Nyquist）
    full[..h].copy_from_slice(half);
    // 利用共轭对称性填充负频率部分
    // full[N - i] = conj(half[i])，i = 1..h-1（不含 DC 和 Nyquist）
    // 注意：当 N 为偶数时，h-1 = N/2 即为 Nyquist 频率，其共轭等于自身
    for (i, half_val) in half.iter().enumerate().take(h).skip(1) {
        let neg_idx = n - i;
        if neg_idx > 0 && neg_idx < n {
            full[neg_idx] = half_val.conj();
        }
    }
    full
}

/// 逆 FFT（全频谱 → 时域复数信号）
///
/// 注意：rustfft 的 IFFT 不做归一化，调用方须自行除以 N。
fn ifft(spectrum: &[Complex64]) -> Vec<Complex64> {
    let n = spectrum.len();
    let mut buffer = spectrum.to_vec();
    let mut planner = FftPlanner::new();
    let ifft = planner.plan_fft_inverse(n);
    ifft.process(&mut buffer);
    buffer
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成合成测试信号：sin + cos 混合（已知频域结构）
    fn synthetic_signal_24() -> Vec<f32> {
        (0..24)
            .map(|i| {
                let t = i as f32 / 24.0;
                let s1 = (2.0 * std::f32::consts::PI * 2.0 * t).sin(); // 2 Hz 分量
                let s2 = 0.5 * (2.0 * std::f32::consts::PI * 5.0 * t).cos(); // 5 Hz 分量
                s1 + s2
            })
            .collect()
    }

    // ========================================================================
    // VMD-01: IMF 长度 = 输入长度
    // ========================================================================

    #[test]
    fn test_vmd_imf_length_equals_input() {
        let signal = synthetic_signal_24();
        let config = VmdConfig {
            k: 4,
            alpha: 2000.0,
            tau: 0.0,
            tol: 1.0e-6,
            max_iter: 500,
        };
        let decomposer = VmdDecomposer::new(config);
        let result = decomposer.decompose(&signal).unwrap();

        assert_eq!(result.imfs.len(), 4, "应有 K=4 个 IMF");
        for (i, imf) in result.imfs.iter().enumerate() {
            assert_eq!(
                imf.len(),
                signal.len(),
                "IMF[{}] 长度应为 {}，实际为 {}",
                i,
                signal.len(),
                imf.len()
            );
        }
    }

    // ========================================================================
    // VMD-02: 重构保真度（所有 IMF 求和与原始信号 RMSE 可接受）
    // ========================================================================

    #[test]
    fn test_vmd_reconstruction_fidelity() {
        let signal = synthetic_signal_24();
        let config = VmdConfig {
            k: 4,
            alpha: 2000.0,
            tau: 0.0,
            tol: 1.0e-6,
            max_iter: 500,
        };
        let decomposer = VmdDecomposer::new(config);
        let result = decomposer.decompose(&signal).unwrap();

        // 验证重构序列长度
        assert_eq!(result.reconstructed.len(), signal.len());

        // 重构误差应小于信号幅度的合理比例
        let signal_amplitude: f32 = signal.iter().map(|x| x.abs()).fold(0.0_f32, f32::max);
        assert!(
            (result.reconstruction_error as f32) < signal_amplitude * 0.3,
            "重构误差 {} 应小于信号幅度 {} 的 30%",
            result.reconstruction_error,
            signal_amplitude
        );
    }

    // ========================================================================
    // VMD-03: 结果包含全部元信息字段
    // ========================================================================

    #[test]
    fn test_vmd_result_metadata() {
        let signal = synthetic_signal_24();
        let config = VmdConfig {
            k: 3,
            alpha: 2000.0,
            tau: 0.0,
            tol: 1.0e-6,
            max_iter: 300,
        };
        let decomposer = VmdDecomposer::new(config);
        let result = decomposer.decompose(&signal).unwrap();

        assert!(result.iterations > 0, "迭代次数应 > 0");
        assert!(result.iterations <= 300, "迭代次数应 <= max_iter");
        assert!(result.reconstruction_error >= 0.0, "RMSE 应非负");
        // converged 在 tau=0 时如果提前退出则为 true
        // 如果达到 max_iter，也可能为 false
    }

    // ========================================================================
    // VMD-04: 不同 K 值产生不同数量的 IMF
    // ========================================================================

    #[test]
    fn test_vmd_different_k_values() {
        let signal = synthetic_signal_24();

        for k in [2, 3, 5] {
            let config = VmdConfig {
                k,
                alpha: 2000.0,
                tau: 0.0,
                tol: 1.0e-6,
                max_iter: 500,
            };
            let decomposer = VmdDecomposer::new(config);
            let result = decomposer.decompose(&signal).unwrap();
            assert_eq!(result.imfs.len(), k, "K={} 应产生 {} 个 IMF", k, k);
        }
    }

    // ========================================================================
    // VMD-05: K=1 退化情况（单一模态）
    // ========================================================================

    #[test]
    fn test_vmd_single_mode() {
        let signal = synthetic_signal_24();
        let config = VmdConfig {
            k: 1,
            alpha: 2000.0,
            tau: 0.0,
            tol: 1.0e-6,
            max_iter: 500,
        };
        let decomposer = VmdDecomposer::new(config);
        let result = decomposer.decompose(&signal).unwrap();

        assert_eq!(result.imfs.len(), 1);
        assert_eq!(result.imfs[0].len(), signal.len());
    }

    // ========================================================================
    // VMD-06: max_iter=1 时返回结果但 converged=false（或正常运行）
    // ========================================================================

    #[test]
    fn test_vmd_few_iterations() {
        let signal = synthetic_signal_24();
        let config = VmdConfig {
            k: 4,
            alpha: 2000.0,
            tau: 0.0,
            tol: 1.0e-6,
            max_iter: 5,
        };
        let decomposer = VmdDecomposer::new(config);
        let result = decomposer.decompose(&signal).unwrap();

        // 即使迭代很少，也应返回结果（至少 IMF 长度正确）
        assert_eq!(result.imfs.len(), 4);
        // 迭代次数应 <= max_iter
        assert!(result.iterations <= 5);
    }

    // ========================================================================
    // VMD-07: 输入含 NaN 时返回错误
    // ========================================================================

    #[test]
    fn test_vmd_nan_input() {
        let mut signal = synthetic_signal_24();
        signal[5] = f32::NAN;

        let config = VmdConfig::default_pv();
        let decomposer = VmdDecomposer::new(config);
        let result = decomposer.decompose(&signal);

        assert!(result.is_err(), "NaN 输入应返回错误");
        let err = result.unwrap_err();
        match err {
            AiEngineError::VmdFailed(msg) => {
                assert!(msg.contains("NaN"), "错误消息应提到 NaN");
            }
            _ => panic!("应该返回 VmdFailed 错误"),
        }
    }

    // ========================================================================
    // VMD-08: 输入含 Inf 时返回错误
    // ========================================================================

    #[test]
    fn test_vmd_inf_input() {
        let mut signal = synthetic_signal_24();
        signal[10] = f32::INFINITY;

        let config = VmdConfig::default_pv();
        let decomposer = VmdDecomposer::new(config);
        let result = decomposer.decompose(&signal);

        assert!(result.is_err(), "Inf 输入应返回错误");
    }

    // ========================================================================
    // VMD-09: 信号长度不足
    // ========================================================================

    #[test]
    fn test_vmd_too_short_signal() {
        let signal = vec![1.0_f32, 2.0, 3.0]; // length = 3 < 4

        let config = VmdConfig::default_pv();
        let decomposer = VmdDecomposer::new(config);
        let result = decomposer.decompose(&signal);

        assert!(result.is_err(), "过短信号应返回错误");
    }

    // ========================================================================
    // VMD-10: K > 信号长度
    // ========================================================================

    #[test]
    fn test_vmd_k_too_large() {
        let signal = synthetic_signal_24();
        let config = VmdConfig {
            k: 100, // K > N
            alpha: 2000.0,
            tau: 0.0,
            tol: 1.0e-6,
            max_iter: 500,
        };
        let decomposer = VmdDecomposer::new(config);
        let result = decomposer.decompose(&signal);

        assert!(result.is_err(), "K > N 应返回错误");
    }

    // ========================================================================
    // VMD-11: 默认配置可用
    // ========================================================================

    #[test]
    fn test_vmd_default_configs() {
        let signal = synthetic_signal_24();

        let pv_config = VmdConfig::default_pv();
        let load_config = VmdConfig::default_load();

        let decomposer_pv = VmdDecomposer::new(pv_config);
        let result_pv = decomposer_pv.decompose(&signal).unwrap();
        assert_eq!(result_pv.imfs.len(), 5);

        let decomposer_load = VmdDecomposer::new(load_config);
        let result_load = decomposer_load.decompose(&signal).unwrap();
        assert_eq!(result_load.imfs.len(), 6);
    }

    // ========================================================================
    // VMD-12: 性能基准测试（<= 50ms）
    // ========================================================================

    #[test]
    fn test_vmd_performance_24_steps() {
        let signal = synthetic_signal_24();
        let config = VmdConfig::default_pv();
        let decomposer = VmdDecomposer::new(config);

        let start = std::time::Instant::now();
        let result = decomposer.decompose(&signal).unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() <= 50,
            "VMD 分解超时: {}ms（要求 <= 50ms）",
            elapsed.as_millis()
        );
        assert!(result.imfs.len() > 0);
    }

    // ========================================================================
    // VMD-13: tau > 0 时的噪声鲁棒模式
    // ========================================================================

    #[test]
    fn test_vmd_with_tau_nonzero() {
        let signal = synthetic_signal_24();
        let config = VmdConfig {
            k: 3,
            alpha: 2000.0,
            tau: 0.01,
            tol: 1.0e-6,
            max_iter: 500,
        };
        let decomposer = VmdDecomposer::new(config);
        let result = decomposer.decompose(&signal).unwrap();

        assert_eq!(result.imfs.len(), 3);
        for imf in &result.imfs {
            assert_eq!(imf.len(), signal.len());
        }
    }
}
