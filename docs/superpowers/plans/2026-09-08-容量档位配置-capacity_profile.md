# 容量档位配置（capacity_profile v2.24）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让台区储能策略参数不再与某台 PCS 容量（60kW 双级式）绑定为单一硬编码默认——按当前 PCS 档位在启动时动态派生整套硬件相关参数，换 PCS 规格只改档位 key / YAML 加档、不改代码。

**Architecture:** 在 `strategy-engine` 新增容量档位加载模块 `pcs_profile.rs`：用薄结构（Deserialize，`TaiStorageConfig` 本体不引入 serde）承接档位 YAML，把 L1 器件级参数（`i_rated`/`s_rated`/单相限）+ L2 派生上限（`dp_max`/`q_i_max`）+ 可选 L3 tuning 覆盖合并进 `TaiStorageConfig`；`core_config` 新增 `strategy.tai_config_file` 指向档位 YAML，启动装配用 `load_tai_storage_config` 加载（失败 = 启动中止 fail-fast，绝不静默落默认档进闭环）。

**Tech Stack:** Rust、serde + `serde_yaml = "0.9"`（与 mupc-core-bin 同版本）；变更覆盖 `mupc-strategy-engine`、`mupc-core-bin`、`deploy/config` 两模板、`tai_replay` bin。

**设计合同：** [04-MUPC-策略引擎-设计文档.md §2.10.2 `[DESIGN_APPROVED: 2026-09-08]`](../../docs/superpowers/plans/modules/04-MUPC-策略引擎-设计文档.md)

---

## 文件结构

| 文件 | 动作 | 职责 |
|---|---|---|
| `mupc/crates/strategy-engine/Cargo.toml` | 修改 | 新增正式依赖 `serde_yaml = "0.9"` |
| `mupc/crates/strategy-engine/src/pcs_profile.rs` | 新建 | 档位薄结构 `TaiConfigFile`/`PcsProfile`/`TuningOverrides` + `load_tai_storage_config` + validate |
| `mupc/crates/strategy-engine/src/pcs_profile_test.rs` | 新建 | 解析/合并/优先级/校验/防呆单元测试 |
| `mupc/crates/strategy-engine/src/lib.rs` | 修改 | `pub mod pcs_profile` + `pub use load_tai_storage_config` + `#[cfg(test)] mod pcs_profile_test` |
| `mupc/crates/mupc-core-bin/src/core_config.rs` | 修改 | 新增 `StrategyConfig`（`tai_config_file`）+ `CoreConfig.strategy` 段 + validate 预留装配位注释 + 2 处测试手动构造点补字段 |
| `mupc/crates/mupc-core-bin/src/startup.rs` | 修改 | TaiStorageStrategy 装配：`TaiStorageConfig::default()` → `load_tai_storage_config`（fail-fast） |
| `mupc/deploy/config/mupc_core_config.yaml` | 修改 | 并入 `strategy.tai_config_file` 示例（含 M-1 装配一致性警告注释） |
| `mupc/deploy/config/mupc_core_config.production.yaml` | 修改 | 同上（生产模板） |
| `mupc/crates/strategy-engine/src/bin/tai_replay.rs` | 修改 | `--config-file <path>` + `--capacity-profile <key>`，覆盖顺序 = 默认 → 档位 → tuning → CLI 位置参数 |

**现状锚点（工程师须知）：**
- 主配置装载：`mupc-core-bin/src/main.rs` → `CoreConfig::load` + `config.validate()` → `startup::initialize_all(&config, &coord)`。`startup.rs` 顶部已 `use mupc_common::{ErrorCode, MupcError};` 与 `use std::sync::Arc;`。
- TaiStorageStrategy 装配点：`startup.rs` 第 497-501 行（`ai_integrator.set_tai_storage_strategy(Arc::new(TaiStorageStrategy::new(TaiStorageConfig::default())))`），函数签名在 339-341 行带 `config: &CoreConfig`（同函数 503 行已用 `config.ai_engine.local_priority`，可访问新段）。
- `TaiStorageConfig`（`strategy-engine/src/config.rs`）：`#[derive(Debug, Clone)]`，`Default` 即 60kW 双级式档（dp_max/q_i_max=25、i_rated=110、s_rated=60），**保持不改**。
- 测试组织惯例：`tai_storage_test.rs` = 文件顶部 `#[cfg(test)] mod tai_storage_test { ... }` 包裹，lib.rs 内 `#[cfg(test)] mod tai_storage_test;` 声明。测试内用 `use crate::pcs_profile::...`。
- cargo 一律在 `mupc/` 目录执行；Windows 低内存建议 `cargo test -p <crate> -j 2`。git 在仓库根 `/e/MUPC2`，commit message 用单行。

---

### Task 1: pcs_profile 模块落地 + 完整加载实现（红→绿）

**Files:**
- Modify: `mupc/crates/strategy-engine/Cargo.toml`
- Modify: `mupc/crates/strategy-engine/src/lib.rs`
- Create: `mupc/crates/strategy-engine/src/pcs_profile.rs`
- Create: `mupc/crates/strategy-engine/src/pcs_profile_test.rs`（先放 2 条最基础测试 → 红 → 实现后绿）

- [ ] **Step 1: Cargo.toml 加 serde_yaml 依赖**

在 `[dependencies]` 块 `serde_json.workspace = true` 之后追加一行：

```toml
serde_yaml = "0.9"
```

- [ ] **Step 2: lib.rs 接线（新增三行）**

在 `pub mod ai_validator;` 附近的 pub mod 区追加 `pub mod pcs_profile;`，在其下 `pub mod tai_storage;` 之后加导出与测试模块声明：

```rust
pub mod pcs_profile;
...
pub use pcs_profile::load_tai_storage_config;

#[cfg(test)]
mod pcs_profile_test;
```

（替换/插入到既有对应区域：`pub mod` 列表加一行；`pub use config::TaiStorageConfig;` 附近加 `pub use pcs_profile::load_tai_storage_config;`；底部 `#[cfg(test)] mod tai_storage_test;` 旁加 `#[cfg(test)] mod pcs_profile_test;`）

- [ ] **Step 3: 写最基础测试（先行，此时应红）**

创建 `pcs_profile_test.rs`：

```rust
#[cfg(test)]
mod pcs_profile_test {
    use crate::pcs_profile::load_tai_storage_config;

    #[test]
    fn test_file_none_returns_default() {
        // file=None：唯一向后兼容分支 → 默认 60 档
        let cfg = load_tai_storage_config(None, None).unwrap();
        assert_eq!(cfg.i_rated, 110.0);
        assert_eq!(cfg.s_rated, 60.0);
        assert_eq!(cfg.dp_max, 25.0);
        assert_eq!(cfg.q_i_max, 25.0);
        assert_eq!(cfg.p_cap, 60.0);
    }

    #[test]
    fn test_file_empty_string_returns_default() {
        // 空串与 None 等价（startup 归一化 trim 后转 None 之外的双保险）
        let cfg = load_tai_storage_config(Some("   "), Some("")).unwrap();
        assert_eq!(cfg.i_rated, 110.0);
        assert_eq!(cfg.s_rated, 60.0);
    }
}
```

- [ ] **Step 4: 运行确认红（模块不存在 → 编译失败）**

```bash
cd /e/MUPC2/mupc && cargo test -p mupc-strategy-engine -j 2 --no-run 2>&1 | tail -20
```
Expected: `error[E0432]: unresolved import crate::pcs_profile`（pcs_profile.rs / pcs_profile_test.rs 尚不存在）。

- [ ] **Step 5: 写完整实现 `pcs_profile.rs`**

完整覆盖设计 8 步加载流程（默认分支 / 读+解析 / 选档 / has_neutral 闸 / L1+L2 合并 / L3 tuning / validate）：

```rust
//! 台区储能容量档位配置（capacity_profile，v2.24，设计文档 §2.10.2）
//!
//! TaiStorageConfig 本体不引入 serde；此处仅以薄结构（Deserialize）承接档位
//! YAML，启动时按当前 PCS 档位派生 L1 器件级参数 + L2 策略上限 + 可选 L3
//! tuning 覆盖。加载失败（缺文件/损坏/未知 key/has_neutral=true 等）= 启动
//! 中止（fail-fast），绝不静默落默认档进闭环。
//!
//! 优先级：`tuning > L1/L2 派生 > L3 代码 Default`。

use std::collections::HashMap;

use serde::Deserialize;

use crate::config::TaiStorageConfig;

/// 档位 YAML 顶层结构（deny_unknown_fields：防 YAML 键拼错被静默忽略）
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaiConfigFile {
    /// 当前部署档位 key（文件顶行；可被 CLI --capacity-profile 覆盖）
    capacity_profile: String,
    #[serde(default)]
    pcs_profiles: HashMap<String, PcsProfile>,
}

/// 单个 PCS 档位（L1 器件级参数）
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PcsProfile {
    desc: Option<String>,
    /// 有无中线：true = 带中线（当前仲裁已删中线判据 → 放行需仲裁扩展 + 驱动
    /// 点表确认双就绪，见 §2.10.2 M-1）
    has_neutral: bool,
    /// 单相有功硬限 (kW)：仅用于派生积分钳 dp_max（arbitrate 单相硬限由
    /// i_rated×U 电流钳 + s_rated 落实，不直接 clamp）
    phase_p_limit_kw: f64,
    /// 单相无功硬限 (kVAr)：用于派生积分钳 q_i_max
    phase_q_limit_kvar: f64,
    /// 单相电流限 (A)
    i_rated_a: f64,
    /// 总视在额定 (kVA)
    s_rated_kva: f64,
    /// 可选 L3 覆盖（None = 保持代码默认）
    #[serde(default)]
    tuning: Option<TuningOverrides>,
}

/// L3 控制标定覆盖（全 Option；None = 保持代码默认）
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TuningOverrides {
    dp_max: Option<f64>,  // L2 收紧（validate 须 ≤ phase_p_limit_kw）
    q_i_max: Option<f64>, // L2 收紧（validate 须 ≤ phase_q_limit_kvar）
    p_abs_trig: Option<f64>,
    p_dis_trig: Option<f64>,
    s1_exit: Option<f64>,
    p_tgt_s1: Option<f64>,
    p_tgt_s3: Option<f64>,
    p_cap: Option<f64>,
    slope: Option<f64>,
    kp: Option<f64>,
    k_diff: Option<f64>,
    k_q: Option<f64>,
    s_q_sign: Option<f64>,
    soc_cap_day: Option<f64>,
    soc_hys: Option<f64>,
    t_release_secs: Option<f64>,
    t_clear_start_secs: Option<f64>,
    t_clear_end_secs: Option<f64>,
    s4_limit_margin_kw: Option<f64>,
    s3_margin_limit: Option<bool>,
    s1_ff_step_kw: Option<f64>,
    window_size: Option<u32>,
    battery_capacity_kwh: Option<f64>,
}

/// 加载台区储能配置（设计 §2.10.2 加载 8 步）：
/// ① 默认（60 档内联）；② file=None/空 → Ok(默认)（唯一向后兼容分支）；
/// ③ Some(path) 读+解析失败 → Err（fail-fast）；④ 选档 key = profile_key(CLI)
/// 或 capacity_profile(文件顶行)，未知 → Err 附可用键；⑤ has_neutral=true → Err；
/// ⑥ 合并 L1/L2（i_rated/s_rated 恒取 L1；dp_max/q_i_max = tuning 或单相限）；
/// ⑦ 应用 tuning 覆盖其余 L3；⑧ validate → Err 含档名。
pub fn load_tai_storage_config(
    file: Option<&str>,
    profile_key: Option<&str>,
) -> Result<TaiStorageConfig, String> {
    let mut cfg = TaiStorageConfig::default();
    let path = file.map(str::trim).filter(|s| !s.is_empty());
    let Some(path) = path else {
        return Ok(cfg); // ② 未配置 = 默认档
    };

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("读取档位文件 {path} 失败: {e}"))?; // ③
    let f: TaiConfigFile = serde_yaml::from_str(&content)
        .map_err(|e| format!("解析档位文件 {path} 失败: {e}"))?; // ③（含缺字段）

    // ④ 选档：CLI key 优先于文件顶行
    let key = profile_key
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(f.capacity_profile.as_str());
    let p = f.pcs_profiles.get(key).ok_or_else(|| {
        let mut keys: Vec<&str> = f.pcs_profiles.keys().map(|s| s.as_str()).collect();
        keys.sort_unstable();
        format!("档位文件 {path}: 未知档位 key '{key}'；可用: [{}]", keys.join(", "))
    })?;

    // ⑤ 中线档防呆：当前仲裁无中线判据，解除需仲裁扩展 + 驱动点表确认双就绪
    if p.has_neutral {
        return Err(format!(
            "档位 {key}（{path}）has_neutral=true：当前仲裁已删中线判据，须仲裁扩展 + PCS 驱动点表确认双就绪后方可放行（§2.10.2 M-1）"
        ));
    }

    // ⑥ L1/L2 合并：i_rated/s_rated 恒取 L1；dp_max/q_i_max 默认 = 对应单相限，tuning 可收紧
    cfg.i_rated = p.i_rated_a;
    cfg.s_rated = p.s_rated_kva;
    cfg.dp_max = p
        .tuning
        .as_ref()
        .and_then(|t| t.dp_max)
        .unwrap_or(p.phase_p_limit_kw);
    cfg.q_i_max = p
        .tuning
        .as_ref()
        .and_then(|t| t.q_i_max)
        .unwrap_or(p.phase_q_limit_kvar);

    // ⑦ tuning 覆盖其余 L3（dp_max/q_i_max 已在 ⑥ 处理，apply 跳过）
    if let Some(t) = &p.tuning {
        t.apply_l3(&mut cfg);
    }

    validate(key, &p, &cfg)?; // ⑧
    Ok(cfg)
}

impl TuningOverrides {
    fn apply_l3(&self, cfg: &mut TaiStorageConfig) {
        if let Some(v) = self.p_abs_trig {
            cfg.p_abs_trig = v;
        }
        if let Some(v) = self.p_dis_trig {
            cfg.p_dis_trig = v;
        }
        if let Some(v) = self.s1_exit {
            cfg.s1_exit = v;
        }
        if let Some(v) = self.p_tgt_s1 {
            cfg.p_tgt_s1 = v;
        }
        if let Some(v) = self.p_tgt_s3 {
            cfg.p_tgt_s3 = v;
        }
        if let Some(v) = self.p_cap {
            cfg.p_cap = v;
        }
        if let Some(v) = self.slope {
            cfg.slope = v;
        }
        if let Some(v) = self.kp {
            cfg.kp = v;
        }
        if let Some(v) = self.k_diff {
            cfg.k_diff = v;
        }
        if let Some(v) = self.k_q {
            cfg.k_q = v;
        }
        if let Some(v) = self.s_q_sign {
            cfg.s_q_sign = v;
        }
        if let Some(v) = self.soc_cap_day {
            cfg.soc_cap_day = v;
        }
        if let Some(v) = self.soc_hys {
            cfg.soc_hys = v;
        }
        if let Some(v) = self.t_release_secs {
            cfg.t_release_secs = v;
        }
        if let Some(v) = self.t_clear_start_secs {
            cfg.t_clear_start_secs = v;
        }
        if let Some(v) = self.t_clear_end_secs {
            cfg.t_clear_end_secs = v;
        }
        if let Some(v) = self.s4_limit_margin_kw {
            cfg.s4_limit_margin_kw = v;
        }
        if let Some(v) = self.s3_margin_limit {
            cfg.s3_margin_limit = v;
        }
        if let Some(v) = self.s1_ff_step_kw {
            cfg.s1_ff_step_kw = v;
        }
        if let Some(v) = self.window_size {
            cfg.window_size = v;
        }
        if let Some(v) = self.battery_capacity_kwh {
            cfg.battery_capacity_kwh = v;
        }
    }
}

/// 校验合并结果（§2.10.2 validate 规则）：器件级 >0；dp_max/q_i_max ≤ 对应
/// 单相限；p_cap>0；soc_cap_day/soc_hys ∈ [0,1]；window_size ≥ 1。
/// p 供单相限上界，cfg 为合并后配置。
fn validate(key: &str, p: &PcsProfile, cfg: &TaiStorageConfig) -> Result<(), String> {
    for (field, v) in [
        ("phase_p_limit_kw", p.phase_p_limit_kw),
        ("phase_q_limit_kvar", p.phase_q_limit_kvar),
        ("i_rated_a", p.i_rated_a),
        ("s_rated_kva", p.s_rated_kva),
    ] {
        if v <= 0.0 {
            return Err(format!("档位 {key}: {field}={v} 须 > 0"));
        }
    }
    if !(cfg.dp_max > 0.0 && cfg.dp_max <= p.phase_p_limit_kw) {
        return Err(format!(
            "档位 {key}: dp_max={} 须满足 0 < dp_max ≤ phase_p_limit_kw={}",
            cfg.dp_max, p.phase_p_limit_kw
        ));
    }
    if !(cfg.q_i_max > 0.0 && cfg.q_i_max <= p.phase_q_limit_kvar) {
        return Err(format!(
            "档位 {key}: q_i_max={} 须满足 0 < q_i_max ≤ phase_q_limit_kvar={}",
            cfg.q_i_max, p.phase_q_limit_kvar
        ));
    }
    if cfg.p_cap <= 0.0 {
        return Err(format!("档位 {key}: p_cap={} 须 > 0", cfg.p_cap));
    }
    if !(0.0..=1.0).contains(&cfg.soc_cap_day) {
        return Err(format!("档位 {key}: soc_cap_day={} 须在 [0,1]", cfg.soc_cap_day));
    }
    if !(0.0..=1.0).contains(&cfg.soc_hys) {
        return Err(format!("档位 {key}: soc_hys={} 须在 [0,1]", cfg.soc_hys));
    }
    if cfg.window_size < 1 {
        return Err(format!("档位 {key}: window_size={} 须 ≥ 1", cfg.window_size));
    }
    Ok(())
}
```

- [ ] **Step 6: 运行确认绿**

```bash
cd /e/MUPC2/mupc && cargo test -p mupc-strategy-engine -j 2 pcs_profile 2>&1 | tail -15
```
Expected: `test result: ok. 2 passed`（pcs_profile_test 两条绿；其余 strategy-engine 测试零回归）。

- [ ] **Step 7: 提交**

```bash
cd /e/MUPC2 && git add mupc/crates/strategy-engine/Cargo.toml mupc/crates/strategy-engine/src/lib.rs mupc/crates/strategy-engine/src/pcs_profile.rs mupc/crates/strategy-engine/src/pcs_profile_test.rs
git commit -m "feat: pcs_profile 容量档位加载器（§2.10.2 v2.24）：默认/读解析/选档/has_neutral 闸/L1L2 合并/L3 tuning/validate"
```

---

### Task 2: 成功路径测试锁定（档位解析/合并/tuning/CLI key 优先）

实现已在 Task 1 完整交付；本批测试为**行为锁定**（应直接绿，无新增实现）。若任何用例红，说明 Task 1 实现与设计有偏差，先修复再提交。

**Files:**
- Modify: `mupc/crates/strategy-engine/src/pcs_profile_test.rs`

- [ ] **Step 1: 追加 fixture 常量与临时文件 helper + 4 个成功路径测试**

在 `pcs_profile_test` 模块内、两个既有测试之后追加：

```rust
    use crate::config::TaiStorageConfig;

    /// 三档 fixture：pcs60_dual（顶行，无中线，无 tuning）、pcs80_kva（无中线，
    /// 供 CLI key 覆盖）、pcs125_kva（has_neutral=true → 应 Err）。
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

    /// 写临时档位 YAML，返回路径（测试结束尽力清理）
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
        // 类型契约：返回值即策略引擎使用的 TaiStorageConfig（Default 系列零回归锚点）
        let cfg = load_tai_storage_config(None, None).unwrap();
        let _: TaiStorageConfig = cfg;
    }
```

- [ ] **Step 2: 运行确认绿**

```bash
cd /e/MUPC2/mupc && cargo test -p mupc-strategy-engine -j 2 pcs_profile 2>&1 | tail -15
```
Expected: `test result: ok. 6 passed`。

- [ ] **Step 3: 提交**

```bash
cd /e/MUPC2 && git add mupc/crates/strategy-engine/src/pcs_profile_test.rs
git commit -m "test: pcs_profile 成功路径（双档合并/tuning L3+收紧/CLI key 覆盖/类型契约）"
```

---

### Task 3: 错误路径与防呆测试锁定（fail-fast / has_neutral / 越界 / deny）

**Files:**
- Modify: `mupc/crates/strategy-engine/src/pcs_profile_test.rs`

- [ ] **Step 1: 追加错误/防呆 fixture 与 8 个测试**

错误簇 fixture（各自独立最小 YAML）：

```rust
    /// 顶行 key 未知 + 缺 pcs_profiles 映射以外，另造一个"档内未知键"场景
    const YAML_UNKNOWN_KEY: &str = r#"
capacity_profile: "nope"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
"#;

    const YAML_PHASE_LIMIT_ZERO: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 0
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
"#;

    const YAML_I_RATED_ZERO: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 0
    s_rated_kva: 60
"#;

    const YAML_TUNING_DP_EXCEED: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      dp_max: 30
"#;

    const YAML_UNKNOWN_PROFILE_FIELD: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limt_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
"#;

    const YAML_UNKNOWN_TUNING_FIELD: &str = r#"
capacity_profile: "pcs60_dual"
pcs_profiles:
  pcs60_dual:
    desc: "d"
    has_neutral: false
    phase_p_limit_kw: 25
    phase_q_limit_kvar: 25
    i_rated_a: 110
    s_rated_kva: 60
    tuning:
      soc_cap_dayy: 0.8
"#;

    const YAML_MALFORMED: &str = "capacity_profile: [unclosed\npcs_profiles: {";

    fn load_tmp(content: &str) -> Result<TaiStorageConfig, String> {
        let path = write_tmp(content);
        let r = load_tai_storage_config(Some(path.to_str().unwrap()), None);
        let _ = std::fs::remove_file(&path);
        r
    }

    #[test]
    fn test_missing_file_err() {
        // fail-fast：文件不存在 → Err（绝不静默落默认档）
        let e = load_tai_storage_config(Some("/nonexistent/tai_profiles.yaml"), None)
            .unwrap_err();
        assert!(e.contains("读取档位文件"), "实际: {e}");
    }

    #[test]
    fn test_malformed_yaml_err() {
        let e = load_tmp(YAML_MALFORMED).unwrap_err();
        assert!(e.contains("解析档位文件"), "实际: {e}");
    }

    #[test]
    fn test_unknown_profile_key_err_lists_keys() {
        // 未知 key（文件顶行 nope）→ Err 且附可用键列表
        let e = load_tmp(YAML_UNKNOWN_KEY).unwrap_err();
        assert!(e.contains("未知档位 key 'nope'"), "实际: {e}");
        assert!(e.contains("pcs60_dual"), "应附可用键列表: {e}");
    }

    #[test]
    fn test_unknown_cli_key_err_lists_keys() {
        // CLI key 未知 → 同 Err（用 YAML_3_PROFILE 覆盖顶行）
        let path = write_tmp(YAML_3_PROFILE);
        let e = load_tai_storage_config(Some(path.to_str().unwrap()), Some("pcs200_kva"))
            .unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(e.contains("未知档位 key 'pcs200_kva'"), "实际: {e}");
        assert!(e.contains("pcs60_dual") && e.contains("pcs125_kva"), "应附全部可用键: {e}");
    }

    #[test]
    fn test_has_neutral_true_err() {
        // has_neutral=true → Err（当前仲裁无中线判据）
        let path = write_tmp(YAML_3_PROFILE);
        let e = load_tai_storage_config(Some(path.to_str().unwrap()), Some("pcs125_kva"))
            .unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(e.contains("has_neutral=true"), "实际: {e}");
    }

    #[test]
    fn test_phase_limit_zero_err() {
        let e = load_tmp(YAML_PHASE_LIMIT_ZERO).unwrap_err();
        assert!(e.contains("phase_p_limit_kw") && e.contains("须 > 0"), "实际: {e}");
    }

    #[test]
    fn test_i_rated_zero_err() {
        let e = load_tmp(YAML_I_RATED_ZERO).unwrap_err();
        assert!(e.contains("i_rated_a") && e.contains("须 > 0"), "实际: {e}");
    }

    #[test]
    fn test_tuning_dp_max_exceed_phase_err() {
        // tuning 越界：dp_max=30 > phase_p_limit_kw=25 → Err
        let e = load_tmp(YAML_TUNING_DP_EXCEED).unwrap_err();
        assert!(e.contains("dp_max") && e.contains("≤ phase_p_limit_kw=25"), "实际: {e}");
    }

    #[test]
    fn test_deny_unknown_profile_field_err() {
        // 档内未知键（拼错 phase_p_limit_kw）→ deny_unknown_fields Err
        let e = load_tmp(YAML_UNKNOWN_PROFILE_FIELD).unwrap_err();
        assert!(e.contains("解析档位文件"), "实际: {e}");
        assert!(e.contains("phase_p_limt_kw"), "应提示未知键: {e}");
    }

    #[test]
    fn test_deny_unknown_tuning_field_err() {
        // tuning 内未知键（拼错 soc_cap_day）→ Err
        let e = load_tmp(YAML_UNKNOWN_TUNING_FIELD).unwrap_err();
        assert!(e.contains("解析档位文件"), "实际: {e}");
        assert!(e.contains("soc_cap_dayy"), "应提示未知键: {e}");
    }
```

- [ ] **Step 2: 运行确认绿（预期 6+10=16 passed，其中 Task1/2 的 6 条不回归）**

```bash
cd /e/MUPC2/mupc && cargo test -p mupc-strategy-engine -j 2 pcs_profile 2>&1 | tail -15
```
Expected: `test result: ok. 16 passed`。

- [ ] **Step 3: 全 crate 回归**

```bash
cd /e/MUPC2/mupc && cargo test -p mupc-strategy-engine -j 2 2>&1 | tail -8
```
Expected: 全部绿（tai_storage_test / ai_validator_test 等 default 系列零回归——`Default` 仍是 60 档、不依赖任何 YAML）。

- [ ] **Step 4: 提交**

```bash
cd /e/MUPC2 && git add mupc/crates/strategy-engine/src/pcs_profile_test.rs
git commit -m "test: pcs_profile 错误路径（fail-fast/has_neutral/越界/deny_unknown_fields）"
```

---

### Task 4: core_config 新增 strategy 段

**Files:**
- Modify: `mupc/crates/mupc-core-bin/src/core_config.rs`

- [ ] **Step 1: 新增 StrategyConfig 结构（放在 PluginsConfig 定义之后、MasterMeterConfig 注释之前）**

```rust
/// 策略引擎配置（v2.24 容量档位 §2.10.2）
#[derive(Debug, Clone, Deserialize)]
pub struct StrategyConfig {
    /// 台区储能档位 YAML 路径；空 = 默认档 pcs60_dual（唯一向后兼容分支）。
    /// 换 PCS 规格只改此路径指向的档位 key / YAML 加档，不改代码。
    #[serde(default)]
    pub tai_config_file: String,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            tai_config_file: String::new(),
        }
    }
}
```

- [ ] **Step 2: CoreConfig 增加字段（`master_meter` 字段声明之后）**

```rust
    /// 台区总表分相数据源（U-26：台区储能策略 phase 输入）
    #[serde(default)]
    pub master_meter: MasterMeterConfig,
    /// 策略引擎配置（v2.24：容量档位 YAML 路径）
    #[serde(default)]
    pub strategy: StrategyConfig,
```

- [ ] **Step 3: validate() 预留装配期一致性核对注释位**

在 `CoreConfig::validate` 里 `if self.master_meter.enabled { ... }` 之前追加：

```rust
        // v2.24 §2.10.2 M-1 预留装配期校验位：策略档位（i_rated/s_rated/dp_max/
        // q_i_max）与 intercore transport 驱动点表型号不自动联动——放行任一非
        // 60kW 无中线档时须与驱动点表同批变更并在此核对（当前 60kW 档与
        // modbus_rtu V1.3 驱动天然匹配；has_neutral=true 档已在档位加载侧拦截）。
        // 注：档位 YAML 的实际加载/校验发生在 startup 装配（fail-fast），此处仅
        // 保留位注释，不读文件、不加逻辑。
```

- [ ] **Step 4: 两处手动 CoreConfig 构造补 strategy 字段（tests 内 568、610 行附近）**

`test_core_config_validate_success` 与 `test_core_config_validate_empty_version` 的 `CoreConfig { ... }` 字面量中，`master_meter: MasterMeterConfig::default(),` 之后追加：

```rust
            strategy: StrategyConfig::default(),
```

- [ ] **Step 5: 追加 2 个配置测试（mod tests 末尾）**

```rust
    /// v2.24: strategy 段缺省 → 默认空路径（向后兼容）；显式配置可解析
    #[test]
    fn test_core_config_strategy_tai_config_file() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
strategy:
  tai_config_file: "/opt/mupc/config/tai_profiles.yaml"
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.strategy.tai_config_file, "/opt/mupc/config/tai_profiles.yaml");
        assert!(config.validate().is_ok(), "显式配置合法应通过: {:?}", config.validate());
    }

    /// v2.24: 未配 strategy 段 → 默认空（load 侧归一化为 None → 默认 60 档）
    #[test]
    fn test_core_config_strategy_default_empty() {
        let yaml = r#"
version: "1.0"
system:
  log_level: "info"
intercore:
  host: "127.0.0.1"
  port: 9100
web_api:
  listen_addr: "0.0.0.0:8080"
ai_engine: {}
plugins: {}
"#;
        let config: CoreConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.strategy.tai_config_file.is_empty());
    }
```

- [ ] **Step 6: 运行测试确认绿**

```bash
cd /e/MUPC2/mupc && cargo test -p mupc-core-bin -j 2 core_config 2>&1 | tail -15
```
Expected: `test result: ok.`（含新增 2 条 + 既有 core_config tests 全绿）。

- [ ] **Step 7: 提交**

```bash
cd /e/MUPC2 && git add mupc/crates/mupc-core-bin/src/core_config.rs
git commit -m "feat: core_config 新增 strategy.tai_config_file（§2.10.2 v2.24 档位路径）+ validate 预留装配位"
```

---

### Task 5: startup 装配 + 部署模板

**Files:**
- Modify: `mupc/crates/mupc-core-bin/src/startup.rs`
- Modify: `mupc/deploy/config/mupc_core_config.yaml`
- Modify: `mupc/deploy/config/mupc_core_config.production.yaml`

- [ ] **Step 1: startup.rs 装配替换（原 497-501 行 TaiStorageConfig::default()）**

原代码：

```rust
    // v2.16: 注入台区储能治理策略（AI 失效兜底，分相 P/Q 经核间下发）
    ai_integrator.set_tai_storage_strategy(Arc::new(
        mupc_strategy_engine::TaiStorageStrategy::new(
            mupc_strategy_engine::TaiStorageConfig::default(),
        ),
    ));
```

替换为：

```rust
    // v2.16: 注入台区储能治理策略（AI 失效兜底，分相 P/Q 经核间下发）
    // v2.24: 容量档位加载（strategy.tai_config_file → TaiStorageConfig）；档位
    // 加载失败 = 启动中止（fail-fast），绝不静默落默认档进闭环。
    let tai_cfg = {
        let path = config.strategy.tai_config_file.trim();
        let opt = if path.is_empty() { None } else { Some(path) };
        mupc_strategy_engine::load_tai_storage_config(opt, None).map_err(|e| {
            MupcError::new(
                ErrorCode::ConfigError,
                format!("tai 档位加载失败: {e}"),
                "startup",
            )
        })?
    };
    ai_integrator.set_tai_storage_strategy(Arc::new(
        mupc_strategy_engine::TaiStorageStrategy::new(tai_cfg),
    ));
```

（`MupcError`/`ErrorCode` 已在文件顶部 import；`config` 参数在 339 行签名可用。）

- [ ] **Step 2: 开发模板并入 strategy 段（`mupc_core_config.yaml` master_meter 段之前）**

```yaml
# 策略引擎配置（v2.24 容量档位 §2.10.2）
# tai_config_file 指向档位 YAML；空 = 默认档 pcs60_dual（60kW 双级式，唯一向后兼容分支）。
# 换 PCS 规格：改档位 YAML 顶行 capacity_profile key 或直接换文件，不需改代码。
strategy:
  tai_config_file: ""
  # ⚠️ 非 60kW 档（如 125kVA）放行须与 intercore transport 驱动点表型号同批变更
  #    并装配期核对（§2.10.2 M-1）；has_neutral=true 档当前在档位加载侧被拦截。
```

- [ ] **Step 3: 生产模板并入 strategy 段（`mupc_core_config.production.yaml` master_meter 段之前，注释强调生产 60kW 默认匹配 V1.3 驱动）**

```yaml
# 策略引擎配置（v2.24 容量档位 §2.10.2）
# 【生产】默认空 = pcs60_dual（60kW 双级式，与 modbus_rtu PCS V1.3 驱动点表匹配）；
# 换 PCS 型号时须同时核对 intercore transport 驱动点表（§2.10.2 M-1），
# 125kVA 等带中线档当前被档位加载校验拦截（待厂方点表 + 仲裁扩展）。
strategy:
  tai_config_file: ""
```

- [ ] **Step 4: 编译验证 + core-bin 全测试**

```bash
cd /e/MUPC2/mupc && cargo check -p mupc-core-bin -j 2 2>&1 | tail -5
cd /e/MUPC2/mupc && cargo test -p mupc-core-bin -j 2 2>&1 | tail -8
```
Expected: 0 errors；测试全绿。

- [ ] **Step 5: 工作区级编译核验**

```bash
cd /e/MUPC2/mupc && cargo check --workspace -j 2 --exclude mupc-iec61850-plugin --exclude rs485-plugin --exclude device-trait 2>&1 | tail -6
```
Expected: `Finished`，0 errors（CLAUDE.md 推送前 0 errors 门禁）。

- [ ] **Step 6: 提交**

```bash
cd /e/MUPC2 && git add mupc/crates/mupc-core-bin/src/startup.rs mupc/deploy/config/mupc_core_config.yaml mupc/deploy/config/mupc_core_config.production.yaml
git commit -m "feat: startup 装配 load_tai_storage_config（fail-fast）+ 部署模板 strategy.tai_config_file 示例"
```

---

### Task 6: tai_replay 支持 --config-file / --capacity-profile

**Files:**
- Modify: `mupc/crates/strategy-engine/src/bin/tai_replay.rs`

覆盖顺序契约：`代码默认 → 档位派生(L1/L2) → tuning(L3) → CLI 位置参数`，扫参可在任意档基础上叠加。

- [ ] **Step 1: 更新 import（第 17 行）**

```rust
use mupc_strategy_engine::{load_tai_storage_config, TaiStorageStrategy};
```

（原 `TaiStorageConfig` 不再直接引用——cfg 由 `load_tai_storage_config` 返回。）

- [ ] **Step 2: main() 顶部参数解析改造（替换 60-69 行原 env::args 段）**

原：

```rust
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).expect("用法: tai_replay <xlsx路径> [SOC初值]");
    let soc_init: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.50);
    if soc_init != soc_init.clamp(0.10, 0.90) {
        eprintln!(
            "警告: SOC 初值超出 [0.10, 0.90]，已裁剪为 {}",
            soc_init.clamp(0.10, 0.90)
        );
    }
    let soc_init = soc_init.clamp(0.10, 0.90);
```

替换为：

```rust
    // v2.24: --config-file <档位YAML> 与 --capacity-profile <key> 自参数剥离，
    // 其余保持既有位置参数顺序（xlsx, SOC初值, soc_cap_day, s4_limit_margin_kw,
    // s3_margin, p_abs_trig, p_tgt_s1, kp, slope）。覆盖顺序 = 代码默认 → 档位
    // 派生(L1/L2) → tuning(L3) → 下方 CLI 位置参数（扫参可在任意档叠加）。
    let mut config_file: Option<String> = None;
    let mut capacity_profile: Option<String> = None;
    let mut pos: Vec<String> = Vec::new();
    let raw: Vec<String> = env::args().collect();
    let mut i = 1;
    while i < raw.len() {
        match raw[i].as_str() {
            "--config-file" => {
                i += 1;
                if let Some(v) = raw.get(i) {
                    config_file = Some(v.clone());
                }
            }
            "--capacity-profile" => {
                i += 1;
                if let Some(v) = raw.get(i) {
                    capacity_profile = Some(v.clone());
                }
            }
            s if s.starts_with("--") => {
                eprintln!("未知选项: {s}");
                std::process::exit(2);
            }
            _ => pos.push(raw[i].clone()),
        }
        i += 1;
    }
    let path = pos
        .get(0)
        .expect("用法: tai_replay [--config-file <档位YAML>] [--capacity-profile <key>] <xlsx路径> [SOC初值]");
    let soc_init: f64 = pos.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.50);
    if soc_init != soc_init.clamp(0.10, 0.90) {
        eprintln!(
            "警告: SOC 初值超出 [0.10, 0.90]，已裁剪为 {}",
            soc_init.clamp(0.10, 0.90)
        );
    }
    let soc_init = soc_init.clamp(0.10, 0.90);
```

- [ ] **Step 3: cfg 来源替换（76-99 行覆盖参数段）**

原：

```rust
    let mut cfg = TaiStorageConfig::default();
    // 可选参数覆盖：args[3]=soc_cap_day，args[4]=s4_limit_margin_kw，args[5]=s3_margin(0|1)
    // 可选 S1 激进调参覆盖：args[6]=p_abs_trig，args[7]=p_tgt_s1，args[8]=kp，args[9]=slope
    if let Some(v) = args.get(3) {
        cfg.soc_cap_day = v.parse().unwrap_or(cfg.soc_cap_day);
    }
    if let Some(v) = args.get(4) {
        cfg.s4_limit_margin_kw = v.parse().unwrap_or(cfg.s4_limit_margin_kw);
    }
    if let Some(v) = args.get(5) {
        cfg.s3_margin_limit = v == "1";
    }
    if let Some(v) = args.get(6) {
        cfg.p_abs_trig = v.parse().unwrap_or(cfg.p_abs_trig);
    }
    if let Some(v) = args.get(7) {
        cfg.p_tgt_s1 = v.parse().unwrap_or(cfg.p_tgt_s1);
    }
    if let Some(v) = args.get(8) {
        cfg.kp = v.parse().unwrap_or(cfg.kp);
    }
    if let Some(v) = args.get(9) {
        cfg.slope = v.parse().unwrap_or(cfg.slope);
    }
```

替换为：

```rust
    // v2.24: 代码默认 → 档位派生(L1/L2) → tuning(L3) 由加载器完成；加载失败
    // fail-fast（与运行时启动一致，不静默落默认档）。
    let mut cfg = load_tai_storage_config(config_file.as_deref(), capacity_profile.as_deref())
        .unwrap_or_else(|e| {
            eprintln!("tai 档位加载失败: {e}");
            std::process::exit(2);
        });
    // 可选位置参数扫参（最外层覆盖）：pos[2]=soc_cap_day, pos[3]=s4_limit_margin_kw,
    // pos[4]=s3_margin(0|1), pos[5]=p_abs_trig, pos[6]=p_tgt_s1, pos[7]=kp, pos[8]=slope
    if let Some(v) = pos.get(2) {
        cfg.soc_cap_day = v.parse().unwrap_or(cfg.soc_cap_day);
    }
    if let Some(v) = pos.get(3) {
        cfg.s4_limit_margin_kw = v.parse().unwrap_or(cfg.s4_limit_margin_kw);
    }
    if let Some(v) = pos.get(4) {
        cfg.s3_margin_limit = v == "1";
    }
    if let Some(v) = pos.get(5) {
        cfg.p_abs_trig = v.parse().unwrap_or(cfg.p_abs_trig);
    }
    if let Some(v) = pos.get(6) {
        cfg.p_tgt_s1 = v.parse().unwrap_or(cfg.p_tgt_s1);
    }
    if let Some(v) = pos.get(7) {
        cfg.kp = v.parse().unwrap_or(cfg.kp);
    }
    if let Some(v) = pos.get(8) {
        cfg.slope = v.parse().unwrap_or(cfg.slope);
    }
```

- [ ] **Step 4: 编译核验 + 用法语义 smoke**

```bash
cd /e/MUPC2/mupc && cargo build -p mupc-strategy-engine -j 2 --bin tai_replay 2>&1 | tail -5
```
Expected: `Finished`（bin 编译成功，证明 flags 解析编译通过）。

可选实机 smoke（若无 7-04 xlsx 则跳过，仅编译即验收）：`cargo run -p mupc-strategy-engine --bin tai_replay -- --config-file /path/tai_profiles.yaml "/e/MUPC2/数据/2026_07_04_data_rule.xlsx"`。

- [ ] **Step 5: 提交**

```bash
cd /e/MUPC2 && git add mupc/crates/strategy-engine/src/bin/tai_replay.rs
git commit -m "feat: tai_replay 支持 --config-file/--capacity-profile（档位覆盖 + 位置参数扫参并存）"
```

---

### Task 7: 全量回归与收尾

- [ ] **Step 1: 工作区测试（排除预存失败清单）**

```bash
cd /e/MUPC2/mupc && cargo test --workspace -j 2 --exclude mupc-iec61850-plugin --exclude rs485-plugin --exclude device-trait 2>&1 | tail -25
```
Expected: 各 crate 测试通过，失败清单仅限 CLAUDE.md 已知预存失败之外无新增。

- [ ] **Step 2: clippy 无新增告警**

```bash
cd /e/MUPC2/mupc && cargo clippy -p mupc-strategy-engine -p mupc-core-bin -j 2 2>&1 | tail -10
```
Expected: 无 error；如 pcs_profile 出现 `needless`/结构风格告警，按提示修复后重新提交（可追加至前一 commit 之后的独立小 commit）。

- [ ] **Step 3: 提交（若 Step 2 有修复）**

```bash
cd /e/MUPC2 && git add <具体修复文件>
git commit -m "style: pcs_profile 容量档位实现收尾（clippy 清理）"
```

- [ ] **Step 4: 向用户汇报**

- 设计 8 步加载/合并/校验已落地，`cargo test -p mupc-strategy-engine` 16 条档位测试绿 + default 系列零回归；
- `core_config.strategy.tai_config_file` + startup fail-fast 装配 + 两部署模板示例就绪；
- `tai_replay` 新增 `--config-file`/`--capacity-profile`（覆盖顺序：默认 → 档位 → tuning → CLI 扫参）；
- 换 PCS 规格 = 改档位 YAML（换 key 或加档），不改代码；125kVA 带中线档仍被 has_neutral 闸拦截（待厂方点表 + 仲裁扩展双就绪）。

---

## Self-Review 对照（设计 §2.10.2 → 计划）

| 设计要求 | 计划落点 |
|---|---|
| 加载 8 步（默认/读/解析/选档/has_neutral/合并/tuning/validate） | Task 1 Step 5 完整实现 |
| file=None/空 → 默认（唯一向后兼容） | Task 1 测试 + 实现 ② |
| fail-fast：读失败/损坏/缺字段/未知 key → Err 附可用键 | Task 1 实现 ③④、Task 3 测试锁定 |
| has_neutral=true → Err | Task 1 ⑤、Task 3 `test_has_neutral_true_err` |
| dp_max/q_i_max = tuning 或单相限；i_rated/s_rated 恒取 L1 | Task 1 ⑥、Task 2 `test_tuning_l3_and_tighten_override` |
| tuning 覆盖其余 L3 | Task 1 apply_l3 ⑦ |
| validate 规则（器件 >0、越界、p_cap/soc/window） | Task 1 validate ⑧、Task 3 越界测试 |
| deny_unknown_fields 档位/tuning | Task 1 struct、Task 3 拼错测试 |
| 优先级 tuning > L1/L2 > L3 default | Task 1 ⑥⑦ + 文件头注释 |
| core_config `strategy.tai_config_file`（String、serde 默认空） | Task 4 Step 1-2 |
| validate 预留装配期校验位（M-1） | Task 4 Step 3 |
| startup 装配 load + map_err fail-fast | Task 5 Step 1 |
| 部署模板并入示例（含 M-1 警告） | Task 5 Step 2-3 |
| tai_replay `--config-file`/`--capacity-profile`（CLI 优先 + 与位置参数扫参共存、覆盖顺序） | Task 6 |
| pcs_profile.rs / _test.rs、lib.rs pub mod + pub use | Task 1 Step 2 |
| serde_yaml 依赖 | Task 1 Step 1 |

**类型一致性检查：** 所有 Task 统一用 `load_tai_storage_config(file: Option<&str>, profile_key: Option<&str>) -> Result<TaiStorageConfig, String>`；fixture 档位字段名与结构体逐一对应（has_neutral/phase_p_limit_kw/phase_q_limit_kvar/i_rated_a/s_rated_kva/tuning）；validate 比较字段（cfg.dp_max/q_i_max vs p.phase_*_limit_*）与设计表一致。

**范围外（非阻塞残留，另行处理）：** §2.15 旧编号撞号、`--config-file` 与 startup `tai_config_file` 的命名差异（design reviewer 已认可非阻塞）、tuning 未含 `control_period_s` 与 `s_q_sign` 符号校验（沿用设计合同，不加戏）。
