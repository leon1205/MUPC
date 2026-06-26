# 上游训练管线（MUPC-AI2）改造要求 — 逆强化学习奖励函数吸收

> **来源**：李嘉庚《基于深度强化学习的微电网能量管理策略研究》(2025) — 太原理工大学硕士论文
> **吸收级别**：R2 推荐
> **MUPC 推理侧影响**：零改动（仅训练阶段奖励函数标定方式变化，推理时 ONNX 模型不变）
> **日期**：2026-06-25

---

## 1. 吸收方法

RE-IRL（Relative Entropy Inverse RL）从优化求解器（Gurobi）生成的专家调度方案中逆向学习奖励函数权重，替代当前人工反复调试 `SafetyOverride` 权重的过程。

```
当前:  人工经验 → 设定奖励权重 w → 训练 RL → 效果不理想 → 重新调 w → ...
                                          ↑_____________________________|

新方案: Gurobi求解MILP → 专家演示轨迹 → RE-IRL逆向学习 → 数据驱动奖励函数 → 训练 RL
```

### 1.1 RE-IRL 原理

**核心思想**：将奖励函数学习转化为**轨迹概率分布匹配**问题。

目标：最小化策略产生的轨迹分布 `P(τ)` 与基分布 `Q(τ)`（均匀分布）之间的相对熵，同时约束特征期望与专家匹配：

```
min_{P}  Σ P(τ_i) · ln( P(τ_i) / Q(τ_i) )
s.t.     Σ P(τ_i) · f(τ_i) = f_expert     (特征期望匹配)
         Σ P(τ_i) = 1                       (概率归一化)
```

拉格朗日对偶求解后，奖励函数形式：

```
R_θ(s, a) = θ^T · f(s, a)

其中 f(s, a) 为特征向量（各成本分量），θ 为 RE-IRL 学习的权重参数
```

**最优轨迹概率**：

```
P(τ | θ) = Q(τ) · exp( θ^T · f(τ) ) / Z(θ)
```

**关键创新**：通过重要性采样估计对偶梯度，**无需已知环境转移概率**（适用于无模型 RL 场景）：

```
∂g/∂θ ≈ f_expert - (1/M) Σ_{m=1}^{M} [ P(τ_m|θ) / Q(τ_m) · f(τ_m) ]
```

### 1.2 对 MUPC 的具体价值

MUPC 当前奖励函数有 6 个分量（safety/econ/grid/shock/voltage/battery），权重人工设定：

```python
# 当前：人工调权（reward_calculator.rs）
r = w_safety * r_safety + w_econ * r_econ + w_grid * r_grid \
  + w_shock * r_shock + w_voltage * r_voltage + w_battery * r_battery
```

RE-IRL 目标：从 Gurobi 求解的日前最优调度中学习这 6 个权重的最优组合。

### 1.3 特征向量定义（MUPC 适配）

```python
f(s, a) = [
    f1 = 变压器负载率偏离惩罚,  # 对应 safety
    f2 = 分时电价 × 电网功率,    # 对应 econ
    f3 = 并网点功率波动,         # 对应 grid
    f4 = 冲击性负荷未满足率,     # 对应 shock
    f5 = 电压偏差惩罚,           # 对应 voltage
    f6 = 电池衰减成本,           # 对应 battery (若不使用PCRL模型，用简化SOC惩罚)
]
```

---

## 2. 训练脚本改造

### 2.1 专家策略生成（Gurobi MILP）

在 `expert_demo.py` 中新增日前优化求解器：

```python
import gurobipy as gp

def generate_expert_trajectory(day_data, price_profile):
    """
    日前 24h MILP 优化，生成一条专家调度轨迹
    
    输出: τ = [(s_1, a_1), (s_2, a_2), ..., (s_T, a_T)]
    状态 s_t: 78维 FusedSystemState
    动作 a_t: [p_ref, k_droop] (2维)
    """
    T = 96  # 15min × 96 点
    model = gp.Model("MUPC_day_ahead")

    # 决策变量
    p_batt = model.addVars(T, lb=-50, ub=50, name="p_batt")      # 电池功率
    p_grid = model.addVars(T, lb=-200, ub=200, name="p_grid")     # 电网交换
    p_load_shed = model.addVars(T, lb=0, ub=60, name="shed")      # 切负荷
    p_pv_curtail = model.addVars(T, lb=0, ub=50, name="curtail")  # 弃光
    soc = model.addVars(T, lb=0.1, ub=0.9, name="soc")

    # 目标函数
    obj = gp.quicksum(
        price[t] * p_grid[t]                          # 购电成本
        + 0.0832 * abs(p_batt[t])                     # 电池运维
        + 500 * p_load_shed[t]                        # 切负荷惩罚
        + 300 * p_pv_curtail[t]                       # 弃光惩罚
        + 100 * abs(soc[t] - 0.5)                     # SOC 偏离中点
        for t in range(T)
    )
    model.setObjective(obj, gp.GRB.MINIMIZE)

    # 约束
    for t in range(T):
        # 功率平衡: p_pv + p_grid + p_batt = p_load - p_shed - p_curtail
        model.addConstr(
            pv_profile[t] + p_grid[t] + p_batt[t]
            == load_profile[t] - p_load_shed[t] - p_pv_curtail[t]
        )
        # SOC 递推: soc[t] = soc[t-1] + p_batt[t] · Δt / E_batt
        if t == 0:
            model.addConstr(soc[t] == soc0 + p_batt[t] * 0.25 / 100.0)
        else:
            model.addConstr(soc[t] == soc[t-1] + p_batt[t] * 0.25 / 100.0)

    model.optimize()

    # 提取轨迹
    trajectory = []
    for t in range(T):
        # 从决策变量反算动作空间
        p_ref_t = p_grid[t].X + p_batt[t].X  # 简化映射
        k_droop_t = 1.0  # MILP 不输出 k_droop，用默认值
        # 构造 78 维状态向量（从 day_data 获取）
        state = build_fused_state(day_data, t, soc[t].X)
        trajectory.append((state, np.array([p_ref_t, k_droop_t])))
    return trajectory
```

### 2.2 RE-IRL 算法实现

```python
class REIRL:
    """相对熵逆强化学习 —— 学习奖励函数权重 θ"""
    
    def __init__(self, feature_dim=6, lr=0.001, max_iter=30):
        self.theta = torch.randn(feature_dim) * 0.01
        self.lr = lr
        self.max_iter = max_iter
    
    def learn(self, expert_trajectories, env, ddpg_trainer):
        """
        expert_trajectories: List[List[(s,a)]], K=30 条
        env: MOMDP 环境
        ddpg_trainer: DDPG 训练器（在每轮 RE-IRL 中重新训练）
        """
        # Step 1: 计算专家特征期望
        f_expert = torch.zeros(self.theta.shape[0])
        for tau in expert_trajectories:
            for s, a in tau:
                f_expert += self._feature(s, a)
        f_expert /= len(expert_trajectories)
        
        # Step 2: 迭代学习
        for epoch in range(self.max_iter):
            # a. 用当前奖励训练 DDPG 策略
            reward_fn = lambda s, a: (self.theta * self._feature(s, a)).sum()
            policy = ddpg_trainer.train(env, reward_fn=reward_fn, epochs=500)
            
            # b. 采样轨迹
            sampled_trajs = [rollout(env, policy) for _ in range(10)]
            
            # c. 计算重要性加权特征期望
            f_sampled = torch.zeros_like(f_expert)
            for tau in sampled_trajs:
                weight = self._importance_weight(tau)
                for s, a in tau:
                    f_sampled += weight * self._feature(s, a)
            f_sampled /= len(sampled_trajs)
            
            # d. 对偶梯度更新
            grad = f_expert - f_sampled
            self.theta += self.lr * grad
            
            if grad.norm() < 1e-4:
                break
        
        return self.theta
    
    def _feature(self, s, a):
        """特征映射 f(s,a): S×A → R⁶"""
        p_ref, k_droop = a[0], a[1]
        return torch.tensor([
            abs(s['transformer_load']) - 0.85,           # f1: 过载惩罚
            s['grid_price'] * s['grid_power'],            # f2: 购电成本
            abs(s['grid_power'] - s.get('p_grid_prev', 0)), # f3: 波动
            max(0, s['load_power'] - s['pv_power'] - s['grid_power']),  # f4: 未满足
            abs(s['voltage_phase_a'] - 1.0),              # f5: 电压偏差
            abs(s['battery_power']) / s['battery_capacity'], # f6: C-rate
        ])
    
    def _importance_weight(self, trajectory):
        """重要性权重 P(τ|θ)/Q(τ)"""
        log_p = sum((self.theta * self._feature(s, a)).sum() for s, a in trajectory)
        log_q = 0.0  # 均匀基分布
        return torch.exp(log_p - log_q)
```

### 2.3 训练流程集成

```python
# train_with_reirl.py

# Phase 1: 生成专家演示（离线，30条轨迹）
expert_trajs = []
for day in training_days[:30]:
    traj = generate_expert_trajectory(day_data=load_day(day),
                                       price_profile=prices[day])
    expert_trajs.append(traj)

# Phase 2: RE-IRL 学习奖励权重
reirl = REIRL(feature_dim=6, lr=0.001, max_iter=30)
theta_learned = reirl.learn(expert_trajs, env, ddpg_trainer)

print(f"Learned reward weights: {theta_learned}")
# → θ = [2.3, 0.8, 1.5, 0.6, 1.2, 0.4]
# （对比人工设定值，验证合理性）

# Phase 3: 用学到的权重训练最终策略
reward_fn = lambda s, a: (theta_learned * f(s, a)).sum()
final_policy = madppg_trainer.train(env, reward_fn=reward_fn, epochs=5000)
```

---

## 3. ONNX 导出与推理侧影响

**零改动。** RE-IRL 仅改变训练阶段的奖励函数权重确定方式，RL 策略网络结构不变：

- Actor 输入：78 维（不变）
- Actor 输出：2 维（不变）
- ONNX 计算图：不变
- RKNN 部署：不变

唯一需要在 ONNX metadata_props 中追加：

| 键 | 值 | 说明 |
|----|-----|------|
| `mupc_reward_source` | `"reirl"` | 奖励函数由 RE-IRL 学习 |

---

## 4. 测试与验证

### 4.1 对比方案

| 方案 | 奖励函数来源 | 说明 |
|------|-------------|------|
| 基线 | 人工设定 | 当前 v2.15 权重 |
| RE-IRL (10 条专家) | 逆RL学习 | 验证小样本效果 |
| RE-IRL (30 条专家) | 逆RL学习 | 论文推荐的最优数量 |
| RE-IRL (50 条专家) | 逆RL学习 | 验证过拟合边界 |

### 4.2 验证指标

| 指标 | 目标 | 测量方法 |
|------|------|----------|
| 策略成本（vs 人工设计） | 降低 ≥ 3% | 测试集蒙特卡洛仿真 |
| 策略成本（vs MILP 最优） | 差距 ≤ 3% | 与 Gurobi 全局最优对比 |
| 收敛速度（vs 人工设计） | 提升 ≥ 2× | 训练 episodes 到收敛 |
| 奖励权重泛化性 | 不同月份学到的 θ 偏差 ≤ 20% | 交叉验证 |

### 4.3 专家轨迹质量验证

| 指标 | 要求 |
|------|------|
| MILP 求解时间（单日） | ≤ 60s |
| 专家轨迹数 | 30 条（论文验证最优） |
| 专家策略准确率 | ≥ 95%（与 Gurobi 最优解对比） |

---

## 5. 非目标

| 项 | 状态 | 理由 |
|----|------|------|
| 端侧部署 RE-IRL 推理 | 不做 | RE-IRL 仅训练阶段使用，推理时策略网络不变 |
| 日内实时逆RL在线学习 | 不做 | Gurobi 求解耗时，不适合实时 |
| 非线性奖励函数 | 不做 | 线性 R=θ^T·f 可解释性好，与 MUPC SafetyOverride 兼容 |
| MaxEnt IRL | 不做 | RE-IRL 的相对熵框架无需已知转移概率，适用无模型场景 |
| 行为克隆替代 | 不做 | 论文消融实验证实 BC 比 RE-IRL 差 8.9% |
| 气象分类场景自适应 | 暂不做 | 论文中 CNN-LSTM 气象分类的 3 类策略可作为 R3 补充，独立评估 |

---

**文档状态**：待 MUPC-AI2 训练管线团队评审
