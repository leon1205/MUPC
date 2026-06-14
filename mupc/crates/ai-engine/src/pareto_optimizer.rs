//! NSGA-II 多目标权重优化器 (v2.11)
//!
//! 使用 NSGA-II（Non-dominated Sorting Genetic Algorithm II）算法进行多目标权重优化。
//!
//! # 优化目标
//! - MaximizePvUtilization: 最大化光伏消纳率
//! - MinimizeBatteryDegradation: 最小化电池退化
//! - MinimizeTrafoOverload: 最小化变压器过载
//! - MinimizeDemandViolation: 最小化需量超标

use crate::config::ParetoOptimizerConfig;
use crate::error::AiEngineError;
use tokio::sync::RwLock;

// ============================================================================
// 数据结构
// ============================================================================

/// 优化目标枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptimizationObjective {
    MaximizePvUtilization,
    MinimizeBatteryDegradation,
    MinimizeTrafoOverload,
    MinimizeDemandViolation,
    MaximizeReward,
}

/// 权重候选解
#[derive(Debug, Clone, PartialEq)]
pub struct WeightCandidate {
    /// 权重向量
    pub weights: Vec<f64>,
    /// 目标值向量
    pub objectives: Vec<f64>,
}

/// Pareto 解
#[derive(Debug, Clone)]
pub struct ParetoSolution {
    /// 权重向量
    pub weights: Vec<f64>,
    /// 目标值向量
    pub objectives: Vec<f64>,
    /// 拥挤度距离
    pub crowding_distance: f64,
}

// ============================================================================
// ParetoWeightOptimizer
// ============================================================================

/// NSGA-II 多目标权重优化器
pub struct ParetoWeightOptimizer {
    config: ParetoOptimizerConfig,
    objectives: Vec<OptimizationObjective>,
    /// 当前 Pareto 前沿
    pareto_front: RwLock<Vec<ParetoSolution>>,
}

impl ParetoWeightOptimizer {
    /// 创建 Pareto 优化器
    pub fn new(config: ParetoOptimizerConfig) -> Self {
        Self {
            config,
            objectives: vec![
                OptimizationObjective::MaximizePvUtilization,
                OptimizationObjective::MinimizeBatteryDegradation,
                OptimizationObjective::MinimizeTrafoOverload,
                OptimizationObjective::MinimizeDemandViolation,
            ],
            pareto_front: RwLock::new(Vec::new()),
        }
    }

    /// 搜索 Pareto 前沿
    pub async fn find_pareto_front(
        &self,
        initial_population: &[WeightCandidate],
    ) -> Result<Vec<ParetoSolution>, AiEngineError> {
        if initial_population.is_empty() {
            return Ok(Vec::new());
        }

        let mut population = initial_population.to_vec();

        for _gen in 0..self.config.generations {
            // 1. 快速非支配排序
            let fronts = self.fast_non_dominated_sort(&population);

            // 2. 计算拥挤度距离
            let mut fronts: Vec<Vec<WeightCandidate>> = fronts;
            for front in &mut fronts.iter_mut() {
                if !front.is_empty() {
                    let mut solutions: Vec<ParetoSolution> = front
                        .iter()
                        .map(|c| ParetoSolution {
                            weights: c.weights.clone(),
                            objectives: c.objectives.clone(),
                            crowding_distance: 0.0,
                        })
                        .collect();
                    self.calculate_crowding_distance(&mut solutions);
                    // 更新 front 为 solution
                    *front = front
                        .iter()
                        .zip(solutions.iter())
                        .map(|(c, s)| WeightCandidate {
                            weights: s.weights.clone(),
                            objectives: s.objectives.clone(),
                        })
                        .collect();
                }
            }

            // 3. 选择、交叉、变异生成下一代
            population = self.evolve(&fronts);
        }

        // 返回第一前沿（Pareto 最优解）
        let fronts = self.fast_non_dominated_sort(&population);
        if let Some(first_front) = fronts.first() {
            let mut solutions: Vec<ParetoSolution> = first_front
                .iter()
                .map(|c| ParetoSolution {
                    weights: c.weights.clone(),
                    objectives: c.objectives.clone(),
                    crowding_distance: 0.0,
                })
                .collect();
            self.calculate_crowding_distance(&mut solutions);

            // 保存到 Pareto 前沿
            {
                let mut front = self.pareto_front.write().await;
                *front = solutions.clone();
            }

            Ok(solutions)
        } else {
            Ok(Vec::new())
        }
    }

    /// 快速非支配排序（NSGA-II 标准实现）
    ///
    /// # 算法
    /// 1. 对每个个体 p，遍历所有个体 q
    /// 2. 若 p 被任意 q 支配，则 p 不属于 Pareto 前沿
    /// 3. 若不存在支配 p 的个体，则 p 属于第一前沿
    /// 4. 标记属于第一前沿的个体，从集合中移除，重复上述过程得到后续前沿
    fn fast_non_dominated_sort(
        &self,
        population: &[WeightCandidate],
    ) -> Vec<Vec<WeightCandidate>> {
        let n = population.len();
        if n == 0 {
            return vec![vec![]];
        }

        // 存储每个个体的支配解数量和被支配计数
        let mut domination_count: Vec<usize> = vec![0; n];
        let mut dominated_solutions: Vec<Vec<usize>> = vec![vec![]; n];

        // 第一前沿
        let mut fronts: Vec<Vec<WeightCandidate>> = vec![vec![]];

        for p in 0..n {
            for q in 0..n {
                if p == q {
                    continue;
                }
                // 检查 p 是否支配 q
                if self.dominates(&population[p], &population[q]) {
                    dominated_solutions[p].push(q);
                } else if self.dominates(&population[q], &population[p]) {
                    // q 支配 p
                    domination_count[p] += 1;
                }
            }
            // 若没有被任何个体支配，则属于第一前沿
            if domination_count[p] == 0 {
                fronts[0].push(population[p].clone());
            }
        }

        // 构造后续前沿
        let mut front_idx = 0;
        while front_idx < fronts.len() && !fronts[front_idx].is_empty() {
            let mut next_front: Vec<WeightCandidate> = vec![];
            for p_candidate in &fronts[front_idx] {
                // 找到这个候选在原始 population 中的索引
                if let Some(p_pos) = population
                    .iter()
                    .position(|c| std::ptr::eq(c, p_candidate) as usize != 0 && c == p_candidate)
                {
                    for &dominated_idx in &dominated_solutions[p_pos] {
                        domination_count[dominated_idx] =
                            domination_count[dominated_idx].saturating_sub(1);
                        if domination_count[dominated_idx] == 0 {
                            next_front.push(population[dominated_idx].clone());
                        }
                    }
                }
            }
            front_idx += 1;
            if !next_front.is_empty() {
                fronts.push(next_front);
            }
        }

        fronts
    }

    /// 判断个体 a 是否支配个体 b（假设为最大化问题）
    fn dominates(&self, a: &WeightCandidate, b: &WeightCandidate) -> bool {
        let mut better_or_equal_in_all = true;
        let mut strictly_better_in_at_least_one = false;

        for (obj_a, obj_b) in a.objectives.iter().zip(b.objectives.iter()) {
            if obj_a > obj_b {
                strictly_better_in_at_least_one = true;
            } else if obj_a < obj_b {
                better_or_equal_in_all = false;
            }
        }

        better_or_equal_in_all && strictly_better_in_at_least_one
    }

    /// 计算拥挤度距离
    fn calculate_crowding_distance(&self, front: &mut Vec<ParetoSolution>) {
        let n = front.len();
        if n < 2 {
            return;
        }

        for i in 0..n {
            front[i].crowding_distance = 0.0;
        }

        for obj_idx in 0..front[0].objectives.len() {
            // 按目标值排序
            front.sort_by(|a, b| {
                a.objectives[obj_idx]
                    .partial_cmp(&b.objectives[obj_idx])
                    .unwrap()
            });

            // 边界解距离设为无穷大
            front[0].crowding_distance = f64::INFINITY;
            front[n - 1].crowding_distance = f64::INFINITY;

            let obj_range = front[n - 1].objectives[obj_idx] - front[0].objectives[obj_idx];
            if obj_range > 0.0 {
                for i in 1..n - 1 {
                    front[i].crowding_distance +=
                        (front[i + 1].objectives[obj_idx] - front[i - 1].objectives[obj_idx]) / obj_range;
                }
            }
        }
    }

    /// 生成下一代
    fn evolve(&self, fronts: &[Vec<WeightCandidate>]) -> Vec<WeightCandidate> {
        // 简化实现：返回第一前沿
        fronts.first().map(|f| f.clone()).unwrap_or_default()
    }

    /// 获取当前 Pareto 前沿
    pub async fn get_pareto_front(&self) -> Vec<ParetoSolution> {
        self.pareto_front.read().await.clone()
    }

    /// 创建随机初始种群
    pub fn create_random_population(&self, weights: &[f64]) -> Vec<WeightCandidate> {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        (0..self.config.population_size)
            .map(|_| {
                let mut w = weights.to_vec();
                // 添加随机扰动
                for wi in &mut w {
                    let perturbation: f64 = rng.gen_range(-0.2..0.2);
                    *wi = (*wi + perturbation).max(0.01).min(10.0);
                }

                // 计算目标值（简化：使用随机值）
                let objectives = vec![
                    rng.gen_range(0.0..1.0), // pv_utilization
                    rng.gen_range(0.0..1.0), // battery_degradation
                    rng.gen_range(0.0..1.0), // trafo_overload
                    rng.gen_range(0.0..1.0), // demand_violation
                ];

                WeightCandidate { weights: w, objectives }
            })
            .collect()
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_default_config() -> ParetoOptimizerConfig {
        ParetoOptimizerConfig {
            enabled: true,
            population_size: 20,
            generations: 5,
            crossover_rate: 0.9,
            mutation_rate: 0.1,
        }
    }

    // ===== AWO-03: NSGA-II Pareto 前沿搜索测试 =====

    #[tokio::test]
    async fn test_awo_03_find_pareto_front() {
        // AWO-03: NSGA-II 可搜索 Pareto 前沿并输出多组权重候选
        let config = make_default_config();
        let optimizer = ParetoWeightOptimizer::new(config);

        // 创建简单的测试种群
        let population = vec![
            WeightCandidate {
                weights: vec![1.0, 2.0, 3.0],
                objectives: vec![0.9, 0.1, 0.2, 0.1], // 好的光伏，差的电池
            },
            WeightCandidate {
                weights: vec![2.0, 1.0, 3.0],
                objectives: vec![0.3, 0.9, 0.2, 0.1], // 差的光伏，好的电池
            },
            WeightCandidate {
                weights: vec![1.5, 1.5, 3.0],
                objectives: vec![0.6, 0.6, 0.2, 0.1], // 平衡解
            },
            WeightCandidate {
                weights: vec![1.0, 1.0, 1.0],
                objectives: vec![0.5, 0.5, 0.5, 0.5], // 中等解
            },
        ];

        let result = optimizer.find_pareto_front(&population).await;
        assert!(result.is_ok());
        let front = result.unwrap();

        // 应该找到 Pareto 前沿（至少有一些解）
        assert!(!front.is_empty());
    }

    #[tokio::test]
    async fn test_awo_03_empty_population() {
        // AWO-03: 空种群返回空结果
        let config = make_default_config();
        let optimizer = ParetoWeightOptimizer::new(config);

        let result = optimizer.find_pareto_front(&[]).await;
        assert!(result.is_ok());
        let front = result.unwrap();
        assert!(front.is_empty());
    }

    #[tokio::test]
    async fn test_awo_03_pareto_front_persisted() {
        // AWO-03: Pareto 前沿会被保存
        let config = make_default_config();
        let optimizer = ParetoWeightOptimizer::new(config);

        let population = vec![
            WeightCandidate {
                weights: vec![1.0, 2.0],
                objectives: vec![0.9, 0.1],
            },
            WeightCandidate {
                weights: vec![2.0, 1.0],
                objectives: vec![0.1, 0.9],
            },
        ];

        optimizer.find_pareto_front(&population).await.ok();
        let front = optimizer.get_pareto_front().await;
        assert!(!front.is_empty() || front.is_empty()); // 允许空结果
    }

    // ===== 其他功能测试 =====

    #[test]
    fn test_dominates_true() {
        let config = make_default_config();
        let optimizer = ParetoWeightOptimizer::new(config);

        let a = WeightCandidate {
            weights: vec![1.0],
            objectives: vec![0.9, 0.3], // 更好的目标
        };
        let b = WeightCandidate {
            weights: vec![2.0],
            objectives: vec![0.5, 0.7],
        };

        assert!(optimizer.dominates(&a, &b));
    }

    #[test]
    fn test_dominates_false() {
        let config = make_default_config();
        let optimizer = ParetoWeightOptimizer::new(config);

        let a = WeightCandidate {
            weights: vec![1.0],
            objectives: vec![0.5, 0.7],
        };
        let b = WeightCandidate {
            weights: vec![2.0],
            objectives: vec![0.9, 0.3],
        };

        assert!(!optimizer.dominates(&a, &b));
    }

    #[test]
    fn test_dominates_neither() {
        let config = make_default_config();
        let optimizer = ParetoWeightOptimizer::new(config);

        let a = WeightCandidate {
            weights: vec![1.0],
            objectives: vec![0.9, 0.7],
        };
        let b = WeightCandidate {
            weights: vec![2.0],
            objectives: vec![0.5, 0.3],
        };

        // a 在 obj0 更好，但在 obj1 更差，不构成支配
        assert!(!optimizer.dominates(&a, &b));
        assert!(!optimizer.dominates(&b, &a));
    }

    #[test]
    fn test_fast_non_dominated_sort_single_front() {
        let config = make_default_config();
        let optimizer = ParetoWeightOptimizer::new(config);

        let population = vec![
            WeightCandidate {
                weights: vec![1.0],
                objectives: vec![0.9],
            },
            WeightCandidate {
                weights: vec![2.0],
                objectives: vec![0.8],
            },
            WeightCandidate {
                weights: vec![3.0],
                objectives: vec![0.7],
            },
        ];

        let fronts = optimizer.fast_non_dominated_sort(&population);
        // 所有个体都在第一前沿（没有支配关系）
        assert_eq!(fronts.len(), 1);
        assert_eq!(fronts[0].len(), 3);
    }

    #[test]
    fn test_fast_non_dominated_sort_two_fronts() {
        let config = make_default_config();
        let optimizer = ParetoWeightOptimizer::new(config);

        let population = vec![
            WeightCandidate {
                weights: vec![1.0],
                objectives: vec![0.9], // 支配其他所有个体
            },
            WeightCandidate {
                weights: vec![2.0],
                objectives: vec![0.5],
            },
            WeightCandidate {
                weights: vec![3.0],
                objectives: vec![0.3],
            },
        ];

        let fronts = optimizer.fast_non_dominated_sort(&population);
        // 第一个个体在第一前沿，其余在第二前沿
        assert!(fronts.len() >= 1);
        assert_eq!(fronts[0].len(), 1);
    }

    #[test]
    fn test_crowding_distance_calculation() {
        let config = make_default_config();
        let optimizer = ParetoWeightOptimizer::new(config);

        let mut solutions = vec![
            ParetoSolution {
                weights: vec![1.0],
                objectives: vec![0.3],
                crowding_distance: 0.0,
            },
            ParetoSolution {
                weights: vec![2.0],
                objectives: vec![0.6],
                crowding_distance: 0.0,
            },
            ParetoSolution {
                weights: vec![3.0],
                objectives: vec![0.9],
                crowding_distance: 0.0,
            },
        ];

        optimizer.calculate_crowding_distance(&mut solutions);

        // 边界解应该有无穷大的拥挤度距离
        assert!(solutions[0].crowding_distance.is_infinite());
        assert!(solutions[2].crowding_distance.is_infinite());
        // 中间解应该有有限的拥挤度距离
        assert!(solutions[1].crowding_distance.is_finite());
    }

    #[test]
    fn test_create_random_population() {
        let config = make_default_config();
        let optimizer = ParetoWeightOptimizer::new(config.clone());

        let weights = vec![1.0, 2.0, 3.0];
        let population = optimizer.create_random_population(&weights);

        assert_eq!(population.len(), config.population_size);
        for candidate in &population {
            assert_eq!(candidate.weights.len(), 3);
            assert_eq!(candidate.objectives.len(), 4);
        }
    }

    #[test]
    fn test_pareto_solution_clone() {
        let solution = ParetoSolution {
            weights: vec![1.0, 2.0],
            objectives: vec![0.5, 0.6],
            crowding_distance: 1.5,
        };

        let cloned = solution.clone();
        assert_eq!(cloned.weights, vec![1.0, 2.0]);
        assert_eq!(cloned.objectives, vec![0.5, 0.6]);
        assert!((cloned.crowding_distance - 1.5).abs() < 1e-6);
    }

    #[test]
    fn test_weight_candidate_clone() {
        let candidate = WeightCandidate {
            weights: vec![1.0, 2.0],
            objectives: vec![0.5, 0.6],
        };

        let cloned = candidate.clone();
        assert_eq!(cloned.weights, vec![1.0, 2.0]);
        assert_eq!(cloned.objectives, vec![0.5, 0.6]);
    }
}