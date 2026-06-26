# 上游训练管线（MUPC-AI2）改造要求 — 在线微调闭环对接

> **来源**：MUPC 推理侧 `online_updater.rs` 集成审计（2026-06-26）
> **MUPC-AI2 当前状态**：仅有完整训练（train from scratch），无增量训练/在线微调能力
> **MUPC 推理侧状态**：PER 数据收集 ✅、安全更新管线 ✅、热切换 ✅；阻塞于权重来源（RKNN 私有格式）
> **日期**：2026-06-26

---

## 1. 背景

MUPC 推理侧（Rust/NPU）的在线微调模块（`online_updater.rs`）已完成全部组件实现：

- PER 优先经验回放缓冲区（加权随机采样 + 重要性采样权重）
- KL 散度正则化（高斯 KL 解析 + 自适应 β）
- 影子模型验证 + 安全约束检查 + 性能对比
- 渐进式权重切换（step-by-step 插值）
- 模型热切换（`ModelRegistry.hot_swap_current`）

**阻塞点**：Rust 侧不做梯度计算（架构设计如此），RKNN NPU 不暴露内部权重。在线微调要闭环，**必须由 MUPC-AI2（Python 训练管线）承担梯度计算角色**。

## 2. 闭环架构

```
MUPC (Rust/NPU)                          MUPC-AI2 (Python)
━━━━━━━━━━━━━━━━━━                        ━━━━━━━━━━━━━━━━━━
full_decision_cycle()
  └─ add_sample() → PER 缓冲
       │
       │  (定期导出 PER 样本)
       ▼
  per_samples.json ──────────────────►  [新] per_injection.py
                                        加载 PER 样本 → 构建增量数据集
                                              │
                                              ▼
                                        [新] incremental_train.py
                                        load_weights() → 增量 PPO 更新 → save_weights()
                                              │
                                              ▼
                                        export_rl_policy() → .onnx
                                        export_to_rknn() → .rknn
                                              │
       ◄─────────────────────────────────── OTA 推送
       │
  ModelRegistry.hot_swap_current()
```

## 3. MUPC-AI2 改造清单

### 3.1 新增：PER 样本注入模块

**文件**：`e:\MUPC-AI2\per_injection.py`（新文件）

将 MUPC 导出的 PER 样本转换为训练可用的数据格式：

```python
# per_injection.py

import json
import numpy as np
from pathlib import Path

def load_per_samples(path: str | Path) -> list[dict]:
    """
    加载 MUPC 导出的 PER 样本 JSON

    预期 JSON 格式:
    [
      {
        "timestamp": 1719400000,
        "state": [78 floats],     # FusedSystemState.to_input_vector()
        "action": [p_ref, k_droop],
        "reward": 0.85,
        "next_state": [78 floats],
        "td_error": 0.32,
        "priority": 0.32,
        "scene": "SeasonalLoadManagement"
      },
      ...
    ]
    """
    with open(path) as f:
        samples = json.load(f)
    return samples


def per_samples_to_data_dict(samples: list[dict]) -> dict[str, np.ndarray]:
    """
    将 PER 样本转换为 MupcEnv 可用的 data dict 格式

    从 78 维 FusedSystemState 中解包出各字段，
    构造与 data_loader 输出兼容的 dict。
    """
    n = len(samples)
    data = {
        "pv_power": np.zeros(n),
        "load_power": np.zeros(n),
        "solar_irradiance": np.zeros(n),
        "temperature": np.zeros(n),
        # ... 从 state[0..77] 解包各 D1-D10 字段
        "n_steps": n,
        "norm_params": {},  # 已归一化，无需再归一化
    }

    for i, s in enumerate(samples):
        state = s["state"]
        # D1: 实时数据 [0..8]
        data["pv_power"][i] = state[1]   # D1 index 1
        data["load_power"][i] = state[2]  # D1 index 2
        data["solar_irradiance"][i] = state[13]  # D2 index
        data["temperature"][i] = state[14]        # D2 index
        # ... 完整字段映射见 data_fusion.rs D1-D10 布局

    return data
```

### 3.2 新增：增量训练模式

**文件**：`e:\MUPC-AI2\incremental_train.py`（新文件）

基于已有模型权重，用新数据做有限步增量更新：

```python
# incremental_train.py

import numpy as np
from pathlib import Path
from _ppo_core import NumPyPPO, MLPPolicy, PPO_DEFAULTS
from mupc_env.core import MupcEnv


def incremental_fine_tune(
    pretrained_weights_path: str,   # 现有 .npz 权重文件
    new_samples_path: str,          # MUPC 导出的 PER 样本 JSON
    output_weights_path: str,       # 输出权重路径
    incremental_steps: int = 10_000,  # 增量更新步数（远小于完整训练的 1M+）
    kl_penalty_weight: float = 0.01,  # KL 正则化权重
    learning_rate: float = 1e-4,      # 更低的学习率
):
    """
    增量微调：加载已有权重 → 少量更新 → 保存

    关键约束：
    - 使用 KL 正则化防止灾难性遗忘
    - 学习率低于从头训练的 1/3
    - 更新步数远小于完整训练
    """
    # 1. 加载 PER 样本并构造环境
    from per_injection import load_per_samples, per_samples_to_data_dict
    samples = load_per_samples(new_samples_path)
    data = per_samples_to_data_dict(samples)
    env = MupcEnv(data, mode="SeasonalLoadManagement")

    # 2. 加载预训练权重（温启动）
    ppo = NumPyPPO(env, obs_dim=78)
    ppo.policy.load_weights(pretrained_weights_path)
    pretrained = ppo.policy.get_weights()  # 保存离线基线供 KL 正则化

    # 3. 增量训练（带 KL 正则化）
    total_steps = 0
    while total_steps < incremental_steps:
        # Rollout 收集
        obs_buf, act_buf, rew_buf, val_buf, done_buf, logp_buf = [], [], [], [], [], []
        obs = env.reset()
        for _ in range(2048):  # n_steps
            action, value, log_prob = ppo.policy.get_action(obs)
            next_obs, reward, done, _ = env.step(action)
            obs_buf.append(obs)
            act_buf.append(action)
            rew_buf.append(reward)
            val_buf.append(value)
            done_buf.append(done)
            logp_buf.append(log_prob)
            obs = next_obs
            total_steps += 1
            if total_steps >= incremental_steps:
                break

        # GAE 优势估计
        advantages = compute_gae(rew_buf, val_buf, done_buf,
                                 gamma=0.99, gae_lambda=0.95)
        returns = advantages + np.array(val_buf)

        # PPO 更新（带 KL 正则化 + 低学习率）
        for _ in range(3):  # 减少 epochs（完整训练用 10）
            for i in range(0, len(obs_buf), 64):
                batch_obs = np.array(obs_buf[i:i+64])
                batch_act = np.array(act_buf[i:i+64])
                batch_adv = advantages[i:i+64]
                batch_ret = returns[i:i+64]
                batch_logp = np.array(logp_buf[i:i+64])

                # 标准 PPO loss + KL 正则化
                loss = ppo._update_step(batch_obs, batch_act, batch_adv,
                                        batch_ret, batch_logp)
                # 追加 KL penalty
                current = ppo.policy.get_weights()
                kl_penalty = compute_kl_penalty(current, pretrained)
                loss += kl_penalty_weight * kl_penalty

    # 4. 保存增量更新后的权重
    ppo.save_weights(output_weights_path)
    return output_weights_path


def compute_kl_penalty(current_weights: dict, pretrained_weights: dict) -> float:
    """计算当前权重与预训练基线之间的 KL 散度惩罚"""
    # 简化：用权重差的 L2 范数近似
    diff = 0.0
    for key in current_weights:
        diff += np.sum((current_weights[key] - pretrained_weights[key]) ** 2)
    return 0.5 * diff
```

### 3.3 改造：train.py 增加温启动模式

**文件**：`e:\MUPC-AI2\train.py`

新增命令行参数和逻辑：

```python
# 新增参数
parser.add_argument("--resume", type=str, default=None,
                    help="温启动权重文件路径 (.npz 或 SB3 .zip)")
parser.add_argument("--incremental-steps", type=int, default=None,
                    help="增量训练步数（默认完整训练）")
parser.add_argument("--kl-beta", type=float, default=0.01,
                    help="KL 正则化权重（增量模式）")

# train() 函数中新增逻辑
def train(args):
    # ... 现有数据加载 ...

    if args.resume:
        # 温启动路径
        if args.algo == "ppo":
            ppo = NumPyPPO(env, obs_dim=78)
            if args.resume.endswith(".npz"):
                ppo.policy.load_weights(args.resume)
            elif args.resume.endswith(".zip"):
                from stable_baselines3 import PPO
                model = PPO.load(args.resume, env=env)
                # 提取 SB3 权重映射到 NumPyPPO
                _map_sb3_to_numpy(model, ppo.policy)

        total_steps = args.incremental_steps or args.total_timesteps
        ppo.learn(total_timesteps=total_steps, callback=callback)
    else:
        # 现有：完整训练
        ppo = NumPyPPO(env, obs_dim=78)
        ppo.learn(total_timesteps=args.total_timesteps, callback=callback)
```

### 3.4 新增：MUPC 侧 PER 样本导出

**文件**：`e:\MUPC2\mupc\crates\ai-engine\src\online_updater.rs`

新增 `PerBuffer.export_json()` 方法，将 PER 样本序列化：

```rust
impl PerBuffer {
    /// v3.1: 导出 PER 样本为 JSON 格式（供 MUPC-AI2 增量训练消费）
    pub fn export_json(&self) -> serde_json::Value {
        let samples: Vec<serde_json::Value> = self.samples
            .iter()
            .map(|s| {
                serde_json::json!({
                    "timestamp": s.data.timestamp,
                    "state": s.data.input,
                    "action": s.data.output,
                    "td_error": s.td_error,
                    "priority": s.priority,
                    "scene": s.data.scene.display_name(),
                })
            })
            .collect();
        serde_json::Value::Array(samples)
    }
}
```

需要在 `Cargo.toml` 中已有 `serde_json` 依赖（已存在）。

### 3.5 改造：`data_loader.py` 增加内存注入支持

**文件**：`e:\MUPC-AI2\data_loader.py`

新增 `InMemoryLoader` 类：

```python
class InMemoryLoader:
    """从内存 dict 构造数据加载器（供 PER 样本注入使用）"""

    def __init__(self, data: dict[str, np.ndarray]):
        self._data = data
        self.n_steps = data.get("n_steps", len(data.get("pv_power", [])))

    def load(self) -> dict[str, np.ndarray]:
        return self._data

    def split(self, data: dict, ratio: float = 0.8):
        """80/20 按时间分割"""
        n = len(data["pv_power"])
        split_idx = int(n * ratio)
        train = {k: v[:split_idx] for k, v in data.items()
                 if isinstance(v, np.ndarray)}
        val = {k: v[split_idx:] for k, v in data.items()
               if isinstance(v, np.ndarray)}
        return train, val
```

---

## 4. 完整在线微调闭环流程

### 4.1 触发时机

| 触发条件 | 频率 |
|----------|------|
| PER 缓冲区满（≥ batch_size × 10） | 约每 2-4 小时 |
| 定时触发（cron） | 每日一次 |

### 4.2 端到端步骤

```
1. [MUPC] PerBuffer.export_json() → per_samples.json
           ↓ HTTP PUT / 文件传输
2. [AI2]   per_injection.load_per_samples("per_samples.json")
           per_injection.per_samples_to_data_dict(samples) → data dict
           ↓
3. [AI2]   incremental_train.incremental_fine_tune(
               pretrained_weights_path="last_model.npz",
               new_samples_path="per_samples.json",
               incremental_steps=10_000,
           ) → new_weights.npz
           ↓
4. [AI2]   export_onnx.export_rl_policy(
               checkpoint_path="new_weights.npz",
               obs_dim=78,
           ) → mupc_rl_policy.onnx
           ↓
5. [AI2]   export_onnx.export_to_rknn(
               onnx_path="mupc_rl_policy.onnx",
           ) → mupc_rl_policy.rknn
           ↓ OTA 推送
6. [MUPC]  ModelRegistry.hot_swap_current(
               new_file_name="mupc_rl_policy.rknn",
               expected_sha256="...",
           )
           ↓
7. [MUPC]  渐进式切换完成 → 新模型生效
```

### 4.3 新增文件清单

| 文件 | 项目 | 用途 |
|------|------|------|
| `per_injection.py` | MUPC-AI2 | PER 样本 JSON → data dict 转换 |
| `incremental_train.py` | MUPC-AI2 | 温启动 + KL 正则化增量训练 |
| `train.py` 改造 | MUPC-AI2 | 新增 `--resume` `--incremental-steps` 参数 |
| `data_loader.py` 改造 | MUPC-AI2 | 新增 `InMemoryLoader` |
| `online_updater.rs` 改造 | MUPC | `PerBuffer.export_json()` |

### 4.4 关键风险与约束

| 风险 | 缓解措施 |
|------|----------|
| PPO 是 on-policy 算法，PER 样本是 off-policy 的 | 增量训练时减少 epochs（3 vs 10），加 KL 正则化防止策略漂移过大 |
| PER 样本量少（~320 个）vs 正式训练（1M+ 步） | 仅做少量步更新（~10K），配合 KL 约束 |
| 增量训练后模型可能过拟合近期数据 | `DefaultSafetyChecker` + 性能对比门控，劣化则拒绝 |
| RKNN 导出需要 `rknn-toolkit2`（仅 Linux x86_64 开发环境） | 导出步骤在开发机或 CI 环境完成 |

---

## 5. 非目标

| 项 | 状态 | 理由 |
|----|------|------|
| 实时在线学习 | 不做 | 闭环周期为小时级（非毫秒级），非严格在线 |
| 联邦学习 | 不做 | 单台区场景，无需跨设备聚合 |
| 自动 OTA 推送 | 不做 | OTA 通道已有（`download_model`），触发逻辑属于运维调度系统 |
| DDPG-REIRL 温启动 | 暂不做 | R2 吸收方案的逆 RL 专家轨迹生成和增量训练是两个独立课题 |

### 5.1 关于 MADDPG

MADDPG 不需要也不应该在 Python 侧实现：

- MUPC 是**单配电台区、单智能体**场景，MADDPG 是多智能体（Multi-Agent）算法，核心价值在 CTDE 框架下多 Agent Critic 共享，MUPC 用不上
- MADDPG Actor 和 PPO Actor 的推理图结构完全一致：`MLP(78→128→128→2, Tanh)`。用 PPO 训练、导出 ONNX，NPU 推理侧零区别
- Rust 侧 `RlAlgorithm::MADDPG` 是**预留枚举变体**，仅用于 ONNX metadata 标记，不影响任何推理逻辑。统一使用 `RlAlgorithm::PPO` 即可

---

**文档状态**：待 MUPC-AI2 训练管线团队评审
