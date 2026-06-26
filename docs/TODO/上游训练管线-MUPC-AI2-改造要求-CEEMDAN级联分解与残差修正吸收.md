# 上游训练管线（MUPC-AI2）改造要求 — CEEMDAN 级联分解与 BiLSTM 残差修正吸收

> **来源**：王林《基于光伏功率与用电负荷预测的光储微电网优化调度研究》(2025)
> **吸收级别**：P1 推荐（预测管线升级）
> **MUPC 推理侧影响**：中等（ONNX 计算图增加级联分量预测 + 残差修正模块）
> **日期**：2026-06-25

---

## 1. 吸收方法

将当前单一的 VMD 分解升级为 **CEEMDAN 级联二次分解 + 样本熵自适应重构**，并在预测管线末端增加 **BiLSTM 二阶残差修正**。

```
当前架构:
  VMD分解 → LSTM → Attention → 6头Linear → 预测值

新架构:
  CEEMDAN一次分解(11 IMF) → SE重构(3分量)
    → 高频分量 CEEMDAN二次分解 → SE重构(阈值0.5, 2子分量)
    → 最终4个重构分量
    → FTCN-MOSE多任务预测(可选，先做基础版)
    → 分量求和 → 一阶预测值
    → BiLSTM残差修正(二阶) → 最终预测值
```

### 1.1 CEEMDAN-SE 级联分解规格

| 参数 | 值 | 说明 |
|------|-----|------|
| 一次分解算法 | CEEMDAN | 替代当前 VMD（无需预设 K 值，自适应） |
| 一次分解 IMF 数 | 11 | 经验值，由数据自适应确定 |
| 样本熵 m | 2 | 嵌入维数 |
| 样本熵 r | 0.2 × Std | 相似容限 |
| 一次重构阈值 | 相邻 IMF 的 SE 值相近度 | 将 11 IMF 重构为 3 分量（高频/中频/低频） |
| 二次分解目标 | 高频分量1（SE 最高的重构组） | 对非平稳最强的分量再次 CEEMDAN |
| 二次重构阈值 | SE = 0.5 | SE > 0.5 为一组(高频子分量)，< 0.5 为另一组 |
| 最终分量数 | **4 个** | 中频1 + 低频1 + 高-高频1 + 高-低频1 |
| 三次分解 | **不做** | 论文实验证实三次分解导致 R² 下降、MAPE 上升（过分解） |

### 1.2 级联分解流程

```
原始负荷序列 (T 个时间步)
    │
    ▼
[一次 CEEMDAN]
    │ 输出 IMF1~IMF11 + 残差
    ▼
[对每个 IMF 计算样本熵 SE]
  (m=2, r=0.2·Std)
    │
    ▼
[SE 值聚类 → 3 个重构分量]
    分量1 (高频): IMF1+IMF2 (SE 最高，非平稳最强)
    分量2 (中频): IMF3 (SE 居中)
    分量3 (低频): IMF4-IMF11 (SE 最低，趋势性)
    │
    ├──────────────────┬──────────────────┐
    ▼                                     ▼
  中频分量2 (不分解)               低频分量3 (不分解)
                                           │
    ▼                                      │
[高频分量1 → 二次 CEEMDAN]                  │
    │ 输出子IMF1~子IMFk                      │
    ▼                                      │
[子IMF SE 计算 → 阈值 0.5 聚类]              │
    │                                      │
    ├── SE > 0.5 → 子分量A (高-高频)         │
    └── SE ≤ 0.5 → 子分量B (高-低频)         │
         │                │                │
         ▼                ▼                ▼
    最终 4 分量:  高-高频   高-低频   中频   低频
```

### 1.3 FTCN-MOSE 多任务预测（可选增强，Phase 2）

对 4 个分量分别预测，再求和：

| 组件 | 参数 | 说明 |
|------|------|------|
| FTCN 头数 | 4 | 每个分量一个预测头 |
| 膨胀系数 | [1, 2, 4, 8] | 覆盖 15 步感受野 |
| 卷积核 | 3 | 1D 因果卷积 |
| 特征融合 | Multi-Head Self-Attention | 对 4 头 TCN 输出加权 |
| 专家网络 | 4 个 LSTM(16) | 每个分量一个专家 |
| 门控 | Softmax 软共享 | 允许跨分量知识迁移 |
| Dropout | 0.05 | |
| 学习率 | 0.001 | Adam |

**基础版（先落地）**：4 个独立 BiLSTM 分别预测 4 个分量（去掉 FTCN 和 MOSE，降低复杂度）。

### 1.4 BiLSTM 二阶残差修正

**残差定义**：
```
r(t) = y_true(t) - y_pred_first_order(t)
```
其中 `y_pred_first_order` 为 4 分量求和后的预测值。

**二阶修正网络**：

| 参数 | 值 | 说明 |
|------|-----|------|
| 层数 | 2 层 BiLSTM | |
| 隐藏单元 | L1=64, L2=32 | |
| 输入特征 | 时间, 负荷, 温度, 湿度, 风速, 节假日, 星期几 | 7 维外部特征 |
| 目标 | 残差序列 r(t) | |
| Dropout | 0.1/层 | |
| 学习率 | 0.001 | |
| Batch size | 128 | |
| 训练轮数 | 100 | |
| 损失函数 | MSE | |

**最终预测**：
```
y_final(t) = y_first_order(t) + r_hat(t)
```

---

## 2. 训练脚本改造

### 2.1 CEEMDAN-SE 分解模块

```python
# ceemdan_decomposer.py
from PyEMD import CEEMDAN
from sklearn.preprocessing import StandardScaler
import numpy as np

class CascadeDecomposer:
    """CEEMDAN 级联二次分解器"""

    def __init__(self, m=2, r_ratio=0.2, se_threshold=0.5):
        self.m = m
        self.r_ratio = r_ratio
        self.se_threshold = se_threshold

    def decompose(self, series):
        """
        Args:
            series: (T,) 原始负荷序列
        Returns:
            components: List[np.ndarray] 最终 4 个重构分量, 每个 (T,)
        """
        # === 一次 CEEMDAN ===
        ceemdan1 = CEEMDAN()
        imfs1 = ceemdan1(series)  # (n_imfs, T), 含残差

        # === 样本熵计算 ===
        se_values = [self._sample_entropy(imf) for imf in imfs1[:-1]]

        # === 一次 SE 重构（3 分量）===
        # 对 SE 值聚类，相似的分在一组
        comp1_high, comp2_mid, comp3_low = self._cluster_by_se(imfs1[:-1], se_values)

        # === 二次 CEEMDAN（仅高频分量）===
        ceemdan2 = CEEMDAN()
        sub_imfs = ceemdan2(comp1_high)  # 二次分解

        # === 二次 SE 重构（阈值 0.5）===
        sub_se = [self._sample_entropy(s) for s in sub_imfs[:-1]]
        comp_a = np.zeros_like(comp1_high)  # 高-高频
        comp_b = np.zeros_like(comp1_high)  # 高-低频
        for i, (s_imf, s_se) in enumerate(zip(sub_imfs[:-1], sub_se)):
            if s_se > self.se_threshold:
                comp_a += s_imf
            else:
                comp_b += s_imf

        return [comp_a, comp_b, comp2_mid, comp3_low]  # 4 个最终分量

    def _sample_entropy(self, x):
        """计算样本熵 (m=2, r=0.2*std)"""
        std = np.std(x)
        if std < 1e-10:
            return 0.0
        r = self.r_ratio * std
        N = len(x)

        def count_matches(m):
            templates = np.array([x[i:i+m] for i in range(N - m)])
            count = 0
            for i in range(len(templates)):
                dist = np.max(np.abs(templates - templates[i]), axis=1)
                count += np.sum(dist < r) - 1
            return count

        B = max(count_matches(self.m), 1)
        A = max(count_matches(self.m + 1), 1)
        return -np.log(A / B)

    def _cluster_by_se(self, imfs, se_values):
        """按 SE 值分为 3 组：高频(SE最大)/中频/低频(SE最小)"""
        # 排序 IMF 按 SE 降序
        indices = np.argsort(se_values)[::-1]
        # 简化：SE 相邻差距 < 阈值则合并
        groups = [[indices[0]]]
        for i in range(1, len(indices)):
            if abs(se_values[indices[i]] - se_values[groups[-1][-1]]) < 0.1:
                groups[-1].append(indices[i])
            else:
                if len(groups) >= 3:
                    # 合并到最后一组
                    groups[-1].append(indices[i])
                else:
                    groups.append([indices[i]])

        comp1 = np.sum([imfs[i] for i in groups[0]], axis=0)
        comp2 = np.sum([imfs[i] for i in groups[1]], axis=0) if len(groups) > 1 else np.zeros_like(comp1)
        comp3 = np.sum([imfs[i] for i in groups[2]], axis=0) if len(groups) > 2 else np.zeros_like(comp1)
        return comp1, comp2, comp3
```

### 2.2 分量预测模型（基础版：独立 BiLSTM）

```python
# component_predictor.py
class ComponentPredictor(nn.Module):
    """对单个分量做 BiLSTM 预测"""
    def __init__(self, input_dim=7, hidden=64, num_layers=2):
        super().__init__()
        self.bilstm = nn.LSTM(input_dim, hidden, num_layers,
                               bidirectional=True, batch_first=True)
        self.fc = nn.Linear(hidden * 2, 1)

    def forward(self, x):
        # x: (B, seq_len, input_dim)
        out, _ = self.bilstm(x)
        return self.fc(out[:, -1, :])  # 最后时间步 → 下一时刻值


class MultiComponentPredictor(nn.Module):
    """4 分量并行预测"""
    def __init__(self, input_dim=7, hidden=64):
        super().__init__()
        self.predictors = nn.ModuleList([
            ComponentPredictor(input_dim, hidden) for _ in range(4)
        ])

    def forward(self, x):
        preds = [p(x) for p in self.predictors]
        return sum(preds)  # 分量求和 = 一阶预测值
```

### 2.3 残差修正模块

```python
# residual_corrector.py
class ResidualCorrector(nn.Module):
    """BiLSTM 二阶残差修正"""
    def __init__(self, input_dim=7, hidden=[64, 32], dropout=0.1):
        super().__init__()
        self.bilstm = nn.LSTM(input_dim, hidden[0], 2,
                               bidirectional=True, batch_first=True,
                               dropout=dropout)
        self.fc = nn.Sequential(
            nn.Linear(hidden[0] * 2, hidden[1]),
            nn.ReLU(),
            nn.Dropout(dropout),
            nn.Linear(hidden[1], 1)
        )

    def forward(self, x):
        # x: (B, seq_len, 7)  — 时间、负荷、温度、湿度、风速、节假日、星期几
        out, _ = self.bilstm(x)
        return self.fc(out[:, -1, :])  # 残差修正值
```

### 2.4 完整训练流程

```python
# train_cascade.py

# Step 1: 级联分解（离线预处理）
decomposer = CascadeDecomposer()
components = decomposer.decompose(load_series)  # List[4] × (T,)

# Step 2: 训练分量预测模型
model = MultiComponentPredictor(input_dim=7, hidden=64)
optimizer = torch.optim.Adam(model.parameters(), lr=0.001)
for epoch in range(100):
    for batch in dataloader:
        x, y = batch  # x: 特征, y: 负荷真值
        y_hat = model(x)
        loss = F.mse_loss(y_hat, y)
        optimizer.zero_grad()
        loss.backward()
        optimizer.step()

# Step 3: 计算一阶残差
y_first_order = model.predict(test_features)
residuals = y_true - y_first_order

# Step 4: 训练残差修正模型
corrector = ResidualCorrector()
optimizer_c = torch.optim.Adam(corrector.parameters(), lr=0.001)
for epoch in range(100):
    r_hat = corrector(test_features)
    loss = F.mse_loss(r_hat, residuals)
    # ...

# Step 5: 最终预测
y_final = y_first_order + corrector(test_features)
```

---

## 3. ONNX 导出改造

### 3.1 导出策略选择

| 方案 | 说明 | 复杂度 | 精度 |
|------|------|--------|------|
| **A: 嵌入 ONNX** | 分量预测+残差修正全部嵌入 ONNX 计算图 | 高 | 全精度 |
| **B: 仅分量预测** | 4 分量预测嵌入 ONNX，残差修正 Rust 侧实现 | 中 | 略低 |
| **C: 仅残差修正** | VMD 预测不变，仅加 BiLSTM 残差修正到 ONNX | 低 | 改善有限 |

**推荐方案 B**（初始落地）：4 分量预测 ONNX 导出，残差修正因需要外部特征输入（温度/湿度等），在 Rust 侧用轻量 BiLSTM 推理或简化为 MLP。

### 3.2 ONNX metadata_props

在现有基础上追加：

| 键 | 值 | 说明 |
|----|-----|------|
| `mupc_decomp_method` | `"ceemdan_cascade"` | 级联分解方式 |
| `mupc_n_components` | `"4"` | 最终重构分量数 |
| `mupc_has_residual_correction` | `"true"` | 是否含残差修正 |
| `mupc_se_threshold` | `"0.5"` | 二次重构 SE 阈值 |

---

## 4. 测试与验证

### 4.1 消融实验

| 配置 | 说明 |
|------|------|
| VMD+LSTM+Attention（基线） | 当前 R1 架构 |
| CEEMDAN 一次分解 + 3 分量 | 仅换分解算法 |
| CEEMDAN 级联二次分解 + 4 分量 | 本文核心创新 |
| 级联 + BiLSTM 残差修正 | 完整方案 |

### 4.2 精度目标

| 指标 | 目标 | 参考（论文） |
|------|------|-------------|
| 负荷 1h MAPE 改善（vs VMD 基线） | 降低 ≥ 2% | 论文从 9.84%→9.63%（一次→二次），+残差→7.09% |
| 负荷 15min MAPE | ≤ 5% | 论文 4.95% |
| 残差修正增量 | MAPE 额外降低 ≥ 1.5 pp | 论文 9.63%→7.09%（降低 2.54pp） |

### 4.3 性能验证

| 指标 | 要求 |
|------|------|
| CEEMDAN 分解耗时（单次 T=96） | ≤ 500ms（离线预处理，不进入推理路径） |
| 分量预测模型参数增量 | ≤ 200KB（4×BiLSTM 比单 LSTM 增加但可控） |
| 残差修正模块参数 | ≤ 100KB（BiLSTM 64+32） |
| 端到端推理延迟增加 | ≤ 50ms（vs 当前 VMD+LSTM+Attention） |

---

## 5. 与已有改造的协同

| 已有改造 | 与本方案关系 | 建议 |
|----------|-------------|------|
| TCN 前置特征提取 | TCN 可插入在分量预测 BiLSTM 之前 | 本方案先落地基础版（无 TCN），后续叠加 |
| BiLSTM 替换 LSTM | 本方案分量预测器即使用 BiLSTM | 方向一致，直接使用 BiLSTM |
| MSSA 超参优化 | 需对新增参数优化：SE 阈值、分量数、残差修正窗口 | 纳入 MSSA 搜索空间 |

---

## 6. 非目标

| 项 | 状态 | 理由 |
|----|------|------|
| FTCN-MOSE 多任务框架 | Phase 2 | 先落地基础版（独立 BiLSTM×4），验证级联分解收益后再叠加 |
| 双通道 CNN-BiGRU-Attention 光伏分型预测 | 独立评估 | 光伏分型（K-means++天气聚类）与负荷预测管线正交 |
| CEEMDAN 三次分解 | 不做 | 论文实验证实过分解导致精度退化 |
| NSGA-II 日前优化 | 不做 | 调度优化非 RL 路线，与 MUPC 技术栈不兼容 |

---

**文档状态**：待 MUPC-AI2 训练管线团队评审
