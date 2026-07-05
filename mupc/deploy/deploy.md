# MUPC 部署指南

目标硬件：RK3588 ARM64，操作系统：Ubuntu 20.04+

---

## 一、快速部署

### 1.1 一键脚本（推荐）

```bash
# 编译 + 部署 + 重启（开发机 → 目标板）
./deploy/scripts/deploy.sh 192.168.3.118 --full

# 仅部署已编译产物
./deploy/scripts/deploy.sh 192.168.3.118 --restart

# 指定用户（密码交互式输入，避免命令行暴露）
./deploy/scripts/deploy.sh 192.168.3.118 --user pi --full
# 非交互场景使用 SSHPASS 环境变量
SSHPASS=xxx ./deploy/scripts/deploy.sh 192.168.3.118 --full
```

前置条件：`sudo apt install sshpass`

### 1.2 手动部署

```bash
# === 目标板上执行 ===

# 1. 创建目录结构
sudo mkdir -p /opt/mupc/{bin,lib,config,models,logs,data,certs}

# 2. 创建 mupc 系统用户
sudo useradd -r -s /bin/false -d /opt/mupc -M mupc

# 3. 复制文件（从开发机 scp 后执行）
sudo cp mupcd /opt/mupc/bin/
sudo cp *.so /opt/mupc/lib/
sudo cp *.yaml /opt/mupc/config/
sudo cp librknnrt.so /opt/mupc/lib/          # 如有 NPU
sudo cp *.rknn /opt/mupc/models/             # 如有 AI 模型

# 4. 设置权限
sudo chown -R mupc:mupc /opt/mupc
sudo chmod +x /opt/mupc/bin/mupcd

# 5. 安装 systemd 服务
sudo cp deploy/systemd/mupcd.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable mupcd

# 6. 启动
sudo systemctl start mupcd
```

---

## 二、目录结构

```
/opt/mupc/
├── bin/
│   └── mupcd                    # 主控可执行文件
├── lib/
│   ├── *.so                     # 插件（rs485/hplc/mqtt 等）
│   └── librknnrt.so             # RKNN NPU 运行时库（可选）
├── config/
│   ├── mupc_core_config.yaml    # 主配置文件
│   └── mupc_env_config.yaml     # AI 引擎环境配置
├── models/
│   ├── lstm.rknn                 # LSTM 预测模型（可选）
│   ├── rl.rknn                   # RL 决策模型（可选）
│   └── manifest.json             # 模型清单
├── logs/
│   └── mupc.log.2026-07-XX       # 滚动日志
├── data/
│   └── mupc.db                   # SQLite 数据库（自动创建）
└── certs/                        # 安全证书（Phase 2+）
```

---

## 三、配置文件

### 3.1 主配置 — `mupc_core_config.yaml`

```yaml
version: "1.0"

system:
  log_level: "info"              # debug / info / warn / error
  log_dir: "/opt/mupc/logs"
  data_dir: "/opt/mupc/data"
  plugin_dir: "/opt/mupc/lib/plugins"
  cert_dir: "/opt/mupc/certs"
  shutdown_timeout_sec: 30

intercore:
  host: "192.168.1.2"           # 实时核心模块 IP（按实际修改）
  port: 9100
  heartbeat_interval_sec: 5
  reconnect_interval_sec: 3

web_api:
  listen_addr: "0.0.0.0:8080"
  enable_https: false

ai_engine:
  model_dir: "/opt/mupc/models"
  config_file: "/opt/mupc/config/mupc_env_config.yaml"
  enable_npu: true               # 是否启用 RK3588 NPU
  inference_timeout_ms: 500

plugins:
  search_paths:
    - "/opt/mupc/lib/plugins"
  auto_load:
    - "rs485_plugin"
    - "hplc_plugin"
    - "mqtt_plugin"
```

### 3.2 AI 引擎配置 — `mupc_env_config.yaml`

```yaml
version:
  fingerprint: "v3.0-20260621"
  source: "mupc-ai2"

# LSTM 模型配置
lstm:
  input_features: 7              # 输入特征数（对齐训练管线）
  yesterday_offset_steps: 96     # 昨日 PV 偏移步数

# 物理常量（必须与实际硬件一致）
physical:
  transformer_kva: 200.0         # 变压器额定容量 (kVA)
  battery_capacity_kwh: 100.0    # 电池容量 (kWh)
  p_batt_max_kw: 50.0            # 最大充放电功率 (kW)
  load_shed_max_kw: 60.0         # 最大切负荷 (kW)

# 安全约束
safety:
  soc_min: 0.10                  # SOC 下限 (10%)
  soc_max: 0.90                  # SOC 上限 (90%)
  overload_threshold: 0.85       # 变压器过载阈值

# 操作参数（可被数据库覆盖）
operational:
  p_batt_ramp_limit_kw: 50.0
  q_batt_ramp_limit_kvar: 30.0
  pv_limit_min: 0.10

# 奖励阈值
reward_thresholds:
  q_margin_threshold: 0.10
  p_threshold_kw: 5.0
```

---

## 四、AI 模型部署

### 4.1 模型文件清单

| 文件 | 用途 | 必需 | 说明 |
|------|------|:--:|------|
| `lstm_attn.rknn` | LSTM + Attention 光伏/负荷预测 | 否 | ONNX→RKNN 转换，训练管线产出 |
| `rl_seasonal.rknn` | MODE-01 农网季节性负荷 RL | 否 | 5 场景模型之一 |
| `rl_arbitrage.rknn` | MODE-02 自主套利 RL | 否 | |
| `rl_demand.rknn` | MODE-03 需量控制 RL | 否 | |
| `rl_vpp.rknn` | MODE-04 虚拟电厂 RL | 否 | |
| `rl_green.rknn` | MODE-05 极致绿色 RL | 否 | |
| `error_correction.rknn` | 误差修正 BiLSTM | 否 | 预测增强可选 |
| `bilstm_attn.rknn` | BiLSTM + Attention | 否 | 预测增强可选 |
| `manifest.json` | 模型清单 | 是 | 记录各场景模型路径和 SHA256 |
| `librknnrt.so` | RKNN Runtime | 是 | 从 SDK `rknpu2/runtime/Linux/librknn_api/aarch64/` 复制 |

### 4.2 manifest.json 示例

```json
{
  "version": "1.0",
  "updated_at": "2026-07-05T00:00:00Z",
  "models": {
    "SeasonalLoadManagement": {
      "file_name": "rl_seasonal.rknn",
      "sha256": "",
      "file_size_bytes": 0,
      "version": "0.1.0"
    }
  }
}
```

### 4.3 模型部署步骤

```bash
# 1. 从训练管线获取 RKNN 模型文件
#    将 .rknn 文件复制到目标板

# 2. 放置模型
sudo mkdir -p /opt/mupc/models
sudo cp *.rknn /opt/mupc/models/
sudo cp manifest.json /opt/mupc/models/

# 3. 放置 RKNN Runtime
sudo cp librknnrt.so /opt/mupc/lib/

# 4. 设置权限
sudo chown -R mupc:mupc /opt/mupc/models /opt/mupc/lib

# 5. 重启
sudo systemctl restart mupcd
```

### 4.4 无模型运行

缺少任何模型文件时，mupcd 仍可正常启动：

- **无 LSTM 模型**：预测返回 0 向量，RL 基于实时数据决策
- **无 RL 模型**：`manifest.json` 不存在时自动创建默认清单，推理调用时返回错误
- **无 librknnrt.so**：npu feature 自动降级为 stub（FFI 函数返回 -1）

---

## 五、日常运维

### 5.1 服务管理

```bash
# 启动
sudo systemctl start mupcd

# 停止
sudo systemctl stop mupcd

# 重启
sudo systemctl restart mupcd

# 查看状态
sudo systemctl status mupcd

# 查看日志
sudo journalctl -u mupcd -f

# 或查看文件日志
tail -f /opt/mupc/logs/mupc.log.*
```

### 5.2 配置修改

```bash
# 编辑配置
sudo vim /opt/mupc/config/mupc_core_config.yaml

# 重启生效
sudo systemctl restart mupcd
```

### 5.3 数据库备份

```bash
# 备份
sudo cp /opt/mupc/data/mupc.db /opt/mupc/data/mupc.db.bak.$(date +%Y%m%d)

# 恢复
sudo systemctl stop mupcd
sudo cp /opt/mupc/data/mupc.db.bak.20260705 /opt/mupc/data/mupc.db
sudo systemctl start mupcd
```

### 5.4 模型更新

```bash
# 替换模型文件并重启
sudo cp new_model.rknn /opt/mupc/models/
sudo systemctl restart mupcd
```

---

## 六、启动流程日志解读

正常启动输出 14 步初始化：

```
[01/14] 初始化消息总线...      — TokioMessageBus 全局消息总线
[02/14] 初始化安全模块...      — stub（Phase 2+ 接入 SM2/SM4）
[03/14] 初始化持久化存储...    — SQLite + 迁移
[04/14] 初始化核间通信...      — TCP Socket 连接实时核心
[05/14] 初始化插件加载器...    — 动态加载 .so 插件
[06/14] 初始化遥测数据采集...  — DataProcessing
[07/14] 初始化 AI 引擎...      — LSTM + RL + ModelManager
[08/14] 初始化策略引擎...      — 削峰填谷/需量控制/防逆流
[09/14] 初始化 IEC 104 网关... — 北向调度主站通信
[10/14] 初始化 Web API...      — Axum HTTP 服务 (0.0.0.0:8080)
[11/14] 初始化 OTA 管理器...   — 固件/模型远程升级
[12/14] 初始化系统资源监控...  — CPU/内存/磁盘
[13/14] 初始化 MQTT 桥接...    — 物联平台对接
[14/14] 初始化近场无线...      — WiFi/BLE/NearLink (stub)

所有子系统就绪，进入主循环
```

### 关键日志行

| 日志 | 说明 |
|------|------|
| `预测增强未配置，使用基线 LSTM 推理路径` | 正常，未启用 VMD+Attention |
| `预测增强管线已启用: 初始等级=VmdAttention` | 已启用 VMD 增强 |
| `安全模块初始化 (stub)` | 正常，国密 Phase 2+ 实现 |
| `Web API 配置: listen=0.0.0.0:8080` | Web 管理页面已启动 |
| `子系统初始化失败: [0x0005] 数据库连接失败` | 检查 `/opt/mupc/data/` 权限 |
| `子系统初始化失败: [0x0402] 数据库迁移失败` | 删除 `mupc.db` 重试 |

---

## 七、故障排查

| 问题 | 原因 | 解决 |
|------|------|------|
| `Permission denied` | 日志/数据目录权限 | `sudo chown -R mupc:mupc /opt/mupc` |
| `unable to open database file` | `/opt/mupc/data/` 不存在或无权限 | `sudo mkdir -p /opt/mupc/data && sudo chown mupc:mupc /opt/mupc/data` |
| `duplicate column name` | 迁移重复执行 | 删除 `mupc.db` 重建（开发阶段） |
| `file is not a database` | db 文件损坏 | 删除 `mupc.db` 重建 |
| `librknnrt.so: cannot open` | NPU 库缺失 | 从 SDK 复制或禁用 `enable_npu` |
| `cannot find -lssl` | OpenSSL 缺失 | `sudo apt install libssl-dev` |

---

## 八、部署检查清单

- [ ] 交叉编译工具链已安装：`aarch64-linux-gnu-gcc --version`
- [ ] Rust aarch64 target 已安装：`rustup target list --installed | grep aarch64`
- [ ] OpenSSL ARM64 已编译：`ls external/openssl-4.0.1/aarch64-install/lib/libssl.a`
- [ ] RKNN SDK 已获取（如需 NPU）：`ls rknn-toolkit2-2.3.2/rknpu2/runtime/Linux/librknn_api/aarch64/librknnrt.so`
- [ ] 目标板已连接：`ssh pi@192.168.3.118`
- [ ] 目标板目录已创建：`/opt/mupc/{bin,lib,config,models,logs,data,certs}`
- [ ] mupc 用户已创建：`id mupc`
- [ ] 配置文件已部署：`/opt/mupc/config/mupc_core_config.yaml`
- [ ] intercore 实时核心 IP 已配置（如需要）
- [ ] systemd 服务已安装：`systemctl status mupcd`
- [ ] 启动日志显示"所有子系统就绪，进入主循环"
