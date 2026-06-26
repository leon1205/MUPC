# 上游训练管线（MUPC-AI2）改造要求 — PPO 场景规划不确定性建模吸收

> **来源**：刘向杰《基于近端策略优化的智能微电网能量管理策略》(2026) — 发表于《中国科学:信息科学》
> **吸收级别**：R3 推荐（增强 MADDPG/PPO 对光伏/负荷不确定性的鲁棒性）
> **MUPC 推理侧影响**：零改动（场景规划为训练阶段技术，推理时策略网络输入不变）
> **日期**：2026-06-25

---

## 1. 吸收方法

LHS（拉丁超立方采样）+ SBR（同步回代削减）场景规划法嵌入 MADDPG/PPO 训练管线，将确定性的单一预测输入升级为概率场景加权决策。

```
当前架构:
  光伏/负荷点预测 → MADDPG/PPO → 确定性动作 a

新架构:
  光伏/负荷点预测 + 预测误差分布
    ↓
  LHS 场景生成 (K=500)
    ↓
  SBR 场景削减 (M=10)
    ↓
  10 个场景分别输入策略网络 → 10 个动作 a^m
    ↓
  场景概率加权: a = Σ p^m · a^m → 鲁棒动作
```

### 1.1 为什么场景规划对 MUPC 有价值

| 问题 | 当前状态 | 场景规划改善 |
|------|----------|-------------|
| 光伏预测在阴雨天 MAPE 可达 20% | 确定性输入，策略对此无能为力 | 场景覆盖±30%误差，策略可预先考虑最坏情况 |
| 8 级降级机制是被动应对 | 精度退化后才降级 | 场景规划主动量化不确定性，减少降级触发 |
| 电池寿命受充放电模式影响 | SafetyRLWrapper 仅设硬约束 | 场景加权动作天然倾向于保守策略（论文证实延长寿命 65%） |

### 1.2 LHS 场景生成

**输入**：
- `P_pv_hat[0..23]`：24h 光伏点预测
- `P_load_hat[0..23]`：24h 负荷点预测
- 预测误差分布：`ε ~ N(0, σ²)`（每个时段独立采样）

**参数**：

| 参数 | 论文值 | MUPC 适配值 | 说明 |
|------|--------|------------|------|
| 场景数 K | 500 | 500 | 覆盖不确定性全空间 |
| 维度 | 24 | 96 | MUPC 使用 15min 分辨率（96 点/天） |
| 误差分布 | N(0, σ²) | N(0, σ²(t)) | σ(t) 可按时段差异化（白天光伏误差大，夜间小） |

**采样步骤**：

```python
def lhs_sample(pv_forecast, load_forecast, sigma_pv, sigma_load, K=500, T=96):
    """
    pv_forecast: (T,) 光伏日前点预测
    load_forecast: (T,) 负荷日前点预测
    sigma_pv: (T,) 各时段光伏预测误差标准差
    sigma_load: (T,) 各时段负荷预测误差标准差
    K: 场景数
    T: 时间步数
    """
    scenarios = []
    for k in range(K):
        # 每个时段在累积概率等分区间的随机位置采样
        prob = np.zeros(T)
        for t in range(T):
            # 第 k 个场景第 t 时段的累积概率
            gamma = np.random.uniform(0, 1)
            prob[t] = (k + gamma) / K

        # 逆正态 CDF 反演预测误差
        pv_error = norm.ppf(prob, loc=0, scale=sigma_pv)
        load_error = norm.ppf(prob, loc=0, scale=sigma_load)

        # 生成场景
        scenario_pv = pv_forecast + pv_error
        scenario_load = load_forecast + load_error
        scenarios.append({
            'pv': np.clip(scenario_pv, 0, None),        # 光伏 ≥ 0
            'load': np.clip(scenario_load, 0, None),    # 负荷 ≥ 0
        })
    return scenarios  # List[Dict], len=K
```

**MUPC 差异化 σ 设计**：

```python
# σ 按天气类型和时段差异化
def get_sigma_profile(weather_type, T=96):
    if weather_type == 'sunny':
        sigma_pv = np.full(T, 0.05)      # 晴天光伏误差 5%
    elif weather_type == 'cloudy':
        sigma_pv = np.linspace(0.1, 0.25, T)  # 多云波动大
    else:  # rainy
        sigma_pv = np.full(T, 0.30)      # 阴雨天误差 30%

    sigma_load = np.full(T, 0.10)        # 负荷误差相对稳定 10%
    # 凌晨时段负荷误差略低
    sigma_load[0:24] *= 0.7  # 00:00-06:00
    return sigma_pv, sigma_load
```

### 1.3 SBR 场景削减

从 K=500 削减至 **M=10**（论文通过消融实验确定的最优数：5→10 安全成本降 44.7%，10→15 仅再降 2.5% 但计算时间 +63.5%）。

```python
def sbr_reduce(scenarios, target_size=10):
    """
    同步回代削减 (Synchronous Backward Reduction)

    Args:
        scenarios: List[Dict], K 个场景, 每个含 'pv' 和 'load'
        target_size: 目标场景数 M
    Returns:
        reduced: List[Dict], M 个场景
        probs: (M,) 各场景概率
    """
    K = len(scenarios)
    # 初始概率均等
    probs = np.ones(K) / K
    # 场景合并为向量 (K, 2T)
    vectors = np.array([np.concatenate([s['pv'], s['load']]) for s in scenarios])

    active = set(range(K))

    while len(active) > target_size:
        # 计算所有场景对距离
        min_pd = float('inf')
        to_delete = None
        to_keep = None

        for n in active:
            # 找离 n 最近的场景 c
            distances = {m: np.linalg.norm(vectors[n] - vectors[m])
                        for m in active if m != n}
            c = min(distances, key=distances.get)
            pd = probs[n] * distances[c]  # Kantorovich 距离
            if pd < min_pd:
                min_pd = pd
                to_delete = n
                to_keep = c

        # 删除场景，概率累加到最近场景
        probs[to_keep] += probs[to_delete]
        probs[to_delete] = 0
        active.remove(to_delete)

    # 返回剩余场景
    reduced = [scenarios[i] for i in sorted(active)]
    final_probs = probs[sorted(active)]
    # 重新归一化
    final_probs /= final_probs.sum()
    return reduced, final_probs
```

### 1.4 场景加权训练集成

```python
# train_with_scenarios.py

def scenario_weighted_training(scenarios, probs, policy, env, optimizer):
    """
    每个场景独立前向，加权聚合策略更新
    """
    batch_states = sample_batch(replay_buffer)
    
    total_loss = 0.0
    for i, scenario in enumerate(scenarios):
        # 用场景 i 的光伏/负荷覆盖环境状态
        augmented_states = inject_scenario(batch_states, scenario)
        
        # 正常 PPO/MADDPG 前向（每个场景独立推理）
        actions_i = policy(augmented_states)
        log_probs_i = policy.log_prob(augmented_states, actions_i)
        
        # 场景概率加权
        total_loss += probs[i] * compute_ppo_loss(log_probs_i, advantages)
    
    optimizer.zero_grad()
    total_loss.backward()
    optimizer.step()
```

**最终动作输出**：

```python
def scenario_weighted_action(scenarios, probs, policy, current_state):
    """
    执行阶段：各场景动作的概率加权平均
    a_final = Σ_{m=1}^{M} p^m · a^m
    """
    actions = []
    for scenario in scenarios:
        augmented_state = inject_single_scenario(current_state, scenario)
        a = policy(augmented_state).detach()
        actions.append(a)
    
    # 概率加权
    a_final = sum(p * a for p, a in zip(probs, actions))
    return a_final
```

---

## 2. 训练脚本改造

### 2.1 场景生成与削减模块

```python
# scenario_planner.py  — 新增文件

import numpy as np
from scipy.stats import norm

class ScenarioPlanner:
    def __init__(self, n_original=500, n_reduced=10):
        self.n_original = n_original
        self.n_reduced = n_reduced

    def generate(self, pv_forecast, load_forecast, pv_sigma, load_sigma):
        scenarios = lhs_sample(pv_forecast, load_forecast,
                               pv_sigma, load_sigma,
                               K=self.n_original)
        reduced, probs = sbr_reduce(scenarios, target_size=self.n_reduced)
        return reduced, probs
```

### 2.2 训练循环改造

```python
# 原训练循环
for episode in range(n_episodes):
    state = env.reset()  # 使用默认点预测
    action = policy(state)
    # ...

# 新训练循环
planner = ScenarioPlanner(n_original=500, n_reduced=10)

for episode in range(n_episodes):
    # 每天开始前生成场景
    pv_hat = pv_predictor.predict(today_features)
    load_hat = load_predictor.predict(today_features)
    sigma_pv, sigma_load = get_sigma_profile(weather_type)

    scenarios, probs = planner.generate(pv_hat, load_hat, sigma_pv, sigma_load)

    # 场景加权训练
    for t in range(T):
        state = env.get_state()
        # 场景加权动作
        a = scenario_weighted_action(scenarios, probs, policy, state)
        next_state, reward, done = env.step(a)
        replay_buffer.push(state, a, reward, next_state, done)

        # 批量更新（场景加权损失）
        if ready_to_update():
            scenario_weighted_training(scenarios, probs, policy, env, optimizer)
```

### 2.3 预测误差 σ 的校准

```python
# sigma_calibration.py

def calibrate_sigma(predictor, val_dataset):
    """在校准集上计算各时段预测误差标准差"""
    errors = []
    for sample in val_dataset:
        y_hat = predictor(sample['features'])
        errors.append(sample['true'] - y_hat)

    errors = np.array(errors)  # (n_samples, T)
    sigma_t = np.std(errors, axis=0)  # 各时段标准差
    return sigma_t
```

---

## 3. ONNX 导出与推理侧影响

**零改动。** 场景规划仅改变训练阶段的输入处理方式，推理时策略网络输入（78 维 FusedSystemState）不变：

- Actor 输入：78 维（不变）
- Actor 输出：2 维（不变）
- ONNX 计算图：不变
- RKNN 部署：不变

唯一变化：训练产出的策略网络权重因训练方式改变而不同（更鲁棒）。

ONNX metadata_props 追加：

| 键 | 值 | 说明 |
|----|-----|------|
| `mupc_scenario_planning` | `"lhs_sbr"` | 训练时使用 LHS+SBR 场景规划 |
| `mupc_scenario_k` | `"500"` | 原始场景数 |
| `mupc_scenario_m` | `"10"` | 削减后场景数 |

---

## 4. 测试与验证

### 4.1 消融实验

| 配置 | 说明 |
|------|------|
| MADDPG + 点预测（基线） | 当前 R1 架构 |
| MADDPG + LHS 500（无削减） | 仅场景生成 |
| MADDPG + LHS 500 + SBR 10 | 完整场景规划 |
| MADDPG + LHS 500 + SBR 5 | 削减场景数对比 |
| MADDPG + LHS 500 + SBR 15 | 削减场景数对比 |

### 4.2 精度与鲁棒性目标

| 指标 | 目标 | 测量方法 |
|------|------|----------|
| 低成本日平均成本改善 | ≥ 3% | 仿真测试集 |
| 高误差日（光伏 MAPE > 15%）成本改善 | ≥ 10% | 选取恶劣天气日单独评估 |
| 电池等效循环寿命 | 延长 ≥ 30% | 循环计数模型 |
| 降级触发频率 | 降低 ≥ 20% | 8 级降级计数器 |
| 训练时间增加 | ≤ 50%（10 场景下前向 10× 但仍可并行） | GPU 计时 |

### 4.3 预测误差 σ 鲁棒性验证

| 测试条件 | 要求 |
|----------|------|
| σ 高估 50%（保守场景） | 成本不劣于基线（过于保守，但不至于更差） |
| σ 低估 50%（激进场景） | 约束违反率不高于基线的 120% |
| σ 按天气差异化 vs 固定 σ | 差异化的成本降低 ≥ 2%（多云/雨天更明显） |

---

## 5. 与其他改造的协同

| 改造 | 协同关系 | 建议 |
|------|----------|------|
| CEEMDAN 级联分解（P1） | 级联分解提升预测精度 → 缩小 σ → 场景更集中 | 先提升预测精度，再上场景规划 |
| PCRL 偏好可控 RL（R1） | 场景加权 + 偏好条件可叠加 | 场景规划和偏好条件作用于不同维度（不确定性 vs 多目标） |
| 逆 RL 奖励函数（R2） | 场景规划下奖励函数需重新学习 | 场景规划后的奖励期望 ≠ 单场景奖励，RE-IRL 专家轨迹也需场景化 |
| TCN 前置特征提取 | 独立，无冲突 | 可并行评估 |

**建议落地顺序**：预测升级（P1）→ PCRL（R1）→ 场景规划（R3）
理由：先提高预测精度缩小 σ，场景规划的效果会更集中；先完成奖励函数升级，场景规划的加权方式更清晰。

---

## 6. 非目标

| 项 | 状态 | 理由 |
|----|------|------|
| GRU 风电预测 | 不做 | MUPC 无风电，仅光伏+负荷 |
| PPO 替换 MADDPG | 不做 | 场景规划是框架级技术，与具体 RL 算法无关 |
| 实时场景在线更新 | 不做 | 场景生成在每天开始前离线完成（≤ 1s），不进入推理路径 |
| 柴油发电机建模 | 不做 | MUPC 无此设备 |
| 超过 10 个削减场景 | 不做 | 论文实验证实 10 为最优性价比 |

---

**文档状态**：待 MUPC-AI2 训练管线团队评审
