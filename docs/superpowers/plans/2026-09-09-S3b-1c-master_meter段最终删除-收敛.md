# S3b-1c · master_meter 段最终删除 — 收敛到 south_stations.meter_grid 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 删除迁移期 legacy `master_meter` 配置段与硬编码总表 task（startup A 分支 / read_master_meter 等函数 / MasterMeterConfig struct 与校验），收敛总表 phase 真源**唯一**走 `south_stations.meter_grid`（southd），并把 production yaml 实际迁移到 meter_grid。现场已确认收敛稳定（2026-09-09）。

**Architecture:** 收敛是 S3a/S3b-1a/1b 之后的清理收尾——grid 单写 AiIntegrator（M-4）已由 southd scheduler 承担，legacy A 分支与 `master_meter` 段是迁移期冗余。删除后 core-bin validate 的 R-H 排他 / master_meter 占口互斥 / modbus_rtu 同串口互斥随之移除（south_stations 已含 PCS 互斥与 meter_grid interval 校验）；startup grid_on 语义收敛为「south_stations 含 meter_grid 即 true」。production yaml 删 master_meter 段并启用 south_stations.meter_grid（regs 填真点表，与 legacy reg_map 同址）。

**Tech Stack:** Rust、serde_yaml、mupc-southd、core-bin core_config/startup。

**设计合同：** 02 §10.3 R-H（迁移期排他 → 收敛目标删除 master_meter 段）+ 02 §10.9（总表回归闸门 grid_convergence 锚，独立 canned 不依赖 legacy，删除零破坏）。范围：master_meter 删除 + production 迁移。SOC 实时回落（R-C）独立 S3b-1d。

---

## 文件结构

| 文件 | 动作 | 职责 |
|---|---|---|
| `mupc/crates/mupc-core-bin/src/core_config.rs` | 修改 | 删 `CoreConfig.master_meter` 字段 / `MasterMeterConfig`/`MasterMeterRegMap`/`MeterRegBlock` struct / 默认函数 / validate 相关（modbus_rtu 同串口互斥、validate_master_meter、validate_reg_map、R-H 排他、master_meter 占口）；~12 测试改写 |
| `mupc/crates/mupc-core-bin/src/startup.rs` | 修改 | 删 A 分支（legacy task）/ create_master_meter_device / read_meter_phases / read_master_meter / import；grid_on 收敛为「grid_station().is_some()」；装配简化为 B/C 两态 |
| `mupc/crates/mupc-southd/src/mapper.rs` | 修改（注释） | "移植 startup read_master_meter" 措辞 → 独立语义 |
| `mupc/crates/mupc-southd/tests/grid_convergence.rs` | 修改（注释） | legacy 参照注释清理 |
| `mupc/deploy/config/mupc_core_config.yaml` | 修改 | 删 master_meter 段 + south_stations 去注释（可启用 meter_grid 示例） |
| `mupc/deploy/config/mupc_core_config.production.yaml` | 修改 | **删 master_meter 段（enabled:true 现役）+ 启用 south_stations.meter_grid（真点表）** |
| `docs/deploy.md`（若引用 master_meter 段） | 修改 | 迁移/接线表清理 |
| `mupc/crates/mupc-core-bin/src/main.rs` | 不变 | 无引用 |

**范围外**：SOC 实时回落（S3b-1d）；BECG 厂方语义点表（S3b-2）；master_meter 历史数据迁移（config 无状态）。

**依赖事实（探得，2026-09-09）**：
- core_config.rs `CoreConfig.master_meter` :26（`#[serde(default)]`）；`MasterMeterConfig` :268-287、`MasterMeterRegMap` :291-304（p_total Option :303）、`MeterRegBlock` :308-317；默认函数 :320-337；`Default` impl :339-373。
- validate() 引用：modbus_rtu 同串口互斥 :558-563（保留 serial/slave/baud 校验 :542-557）；`if enabled { validate_master_meter() }` :573-575；validate_master_meter fn :589-618；validate_reg_map :785-844；validate_south_stations 内 R-H :736-742 / master_meter 占口 :771-778（③）。
- **CoreConfig 及全 struct 无 deny_unknown_fields**（derive 仅 Deserialize）→ 删字段后多余 yaml 键静默忽略。
- 测试引用（core_config.rs tests，起点 :855）：默认断言 :895-898；struct 字面量 :938/:983；master_meter 专属 validate 测 7 个 :993/:1047/:1078/:1101/:1131/:1164/:1193；跨段 ③ :1793/:1832；R-H ④ :1881；master-only :1919；south_stations 正常测带 `master_meter: enabled:false` yaml（:1704/:1966/:2021/:2051/:2087）删字段后键静默忽略不破但建议清理。
- startup.rs import :19；create_master_meter_device :156-177；read_meter_phases :180-193；read_master_meter :197-262；A/B/C 装配 :721-839（A :736-768 混合配置 warn :740-745/open :747/grid_on=master_meter.is_some() :748/spawn legacy :749-764）；pv/load :842-887（grid_on 捕获 :848、`if !grid_on` :861-863 兜底仍成立）；SouthSink on_grid_package→set_latest_data :390-392。
- grid_convergence.rs 独立 canned 不 import core-bin；mapper.rs :5/:88 注释提 legacy。
- production yaml master_meter 段 :84-100（enabled:true :89，reg_map :94-100）；south_stations 全注释 :102-149（grid_meter 示例 :127-140）。
- strategy-engine 不引用 MasterMeterConfig；main.rs 无引用。
- deploy.md :356-359/:382-385 引用 master_meter（接线表）。

**cargo 一律 `cd /e/MUPC2/mupc`；Windows 低内存 `-j 2`；commit 单行、精确 `git add <path>`、禁 push/amend/`-A/-u`、勿跑 `cargo fmt --all`、勿碰工作区用户 dirty（intercore/transport/modbus.rs）。**

---

### Task 1: core_config.rs — 删 master_meter 类型与字段（编译破 → 测试字面量同步）

**Files:**
- Modify: `mupc/crates/mupc-core-bin/src/core_config.rs`

**背景**：先删类型/字段/默认函数，让编译器暴露所有引用点（含测试字面量），逐处清。validate 逻辑删除放 Task 2（避免一次 diff 过大难审）。

- [ ] **Step 1: 删 struct 定义 + CoreConfig 字段 + 默认函数**

删：`CoreConfig.master_meter` 字段（:25-26 含 serde default）；`MasterMeterConfig`（:268-287）/`MasterMeterRegMap`（:291-304）/`MeterRegBlock`（:308-317）；默认函数 :320-337；`Default` impl :339-373。**若 MeterRegBlock/RegFormat 只被 master_meter 用**（grep 确认——RegFormat 来自 data-processing，south_stations RegBlockConf 用它，故 RegFormat import 保留；MeterRegBlock 是 core_config 本地 struct 只被 MasterMeterConfig 用则删）。

- [ ] **Step 2: 删测试字面量与默认断言（编译修复）**

- 默认值断言 :895-898（test_core_config_deserialize_minimal 里 master_meter 字段）删。
- struct 字面量 :938（test_core_config_validate_success）/ :983（test_core_config_validate_empty_version）删 `master_meter: ...` 字段。
- south_stations 正常测带 `master_meter: enabled: false` yaml（:1704/:1966/:2021/:2051/:2087）——删字段后键静默忽略不破，**但本 task 一并清理**（去掉 yaml 里 master_meter 键，保持测试纯净）。

- [ ] **Step 3: 编译确认（预期 validate 引用仍破——validate_master_meter 等未删）**

```bash
cd /e/MUPC2/mupc && cargo check -p mupc-core-bin -j 2 2>&1 | grep -E "^error" | head -20
```
Expected：剩余 error 全在 validate()（:558-563/:573-575 与 fn :589-618/:785-844 引用已删字段）——Task 2 处理。本 task 提交前须 Task 2 一并绿（见下依赖注）。

- [ ] **Step 4: 提交（与 Task 2 合并或紧随——controller 编排：Task 1/2 同文件强耦合，可合并一 commit 或 Task 2 紧随）**

---

### Task 2: core_config.rs — 删 validate 逻辑 + 专属测试

**Files:**
- Modify: `mupc/crates/mupc-core-bin/src/core_config.rs`

**背景**：validate 的 master_meter 分支与跨段互斥在删字段后冗余/失效。删后 south_stations 承担收敛（meter_grid interval 段内验 + PCS 互斥 + 别名互斥）。

- [ ] **Step 1: validate() 精简**

- `if self.master_meter.enabled { validate_master_meter() }`（:573-575）删。
- modbus_rtu 块 master_meter 同串口互斥子判 :558-563 删（保留 serial/slave/baud 校验 :542-557）。
- 注释 :578-579（总表启用前提）清理。

- [ ] **Step 2: 删 fn + R-H/占口**

- `validate_master_meter` fn（:589-618）删。
- `validate_reg_map` fn（:785-844）删（收敛由 southd config 段内 interval 验 + mapper 块语义承担——grid_convergence 锚已锁）。
- `validate_south_stations`：④ R-H 排他 :736-742 删（不再有 master_meter 可排他）；③ master_meter 占口 :771-778 删；fn 头注释 :716-731 清理 ③/④ 描述（保留 ①段内/②PCS 同口/⑤别名）。

- [ ] **Step 3: 删/改专属测试**

删 7 个 master_meter validate 测：test_validate_master_meter_shared_serial_rejected :993 / test_validate_modbus_rtu_shared_serial_rejected :1047（**此测是 modbus_rtu vs master_meter 跨判——master_meter 删后无意义，但 modbus_rtu 自身 serial/slave/baud 校验仍由其他测覆盖？核实保留一个纯 modbus_rtu 校验测**）/ test_validate_master_meter_slave_addr_range :1078 / test_validate_master_meter_reg_overlap_rejected :1101 / test_validate_master_meter_valid_ok :1131 / test_validate_master_meter_read_interval_too_long :1164 / test_validate_master_meter_p_total_overlap_rejected :1193。
删跨段：test_south_stations_port_conflicts_master_meter :1793 / test_south_stations_nonconflicting_with_master_meter_passes :1832（③占口场景消失）/ test_south_stations_meter_grid_exclusive_with_master_meter_rejected :1881（R-H ④）/ test_master_meter_only_without_south_stations_passes :1919。
**保留**：PCS 互斥（south_stations vs modbus_rtu）、别名互斥、段内各校验测（删 yaml 里 master_meter 键即可）。

- [ ] **Step 4: 全量 core_config 测试**

```bash
cd /e/MUPC2/mupc && cargo test -p mupc-core-bin -j 2 core_config 2>&1 | tail -3
```
Expected：绿（删后核心 south_stations 校验不破；R-H 测删除后需确认无其它依赖 master_meter 存在假设）。

---

### Task 3: startup.rs — 删 A 分支与 legacy 函数，grid_on 收敛

**Files:**
- Modify: `mupc/crates/mupc-core-bin/src/startup.rs`

**背景**：删除迁移 A 分支后 south_stations 是唯一 grid 源。grid_on 语义收敛。

- [ ] **Step 1: 删 import + 3 legacy 函数**

- import :19 `MasterMeterConfig, MeterRegBlock` 删。
- `create_master_meter_device`（:156-177）/`read_meter_phases`（:180-193）/`read_master_meter`（:197-262）全删。
- 若 mupc-data-processing meter_regs import 只被 read_master_meter 用——**southd 已独立引用，startup 侧若不再用则清理 import**（grep 确认）。

- [ ] **Step 2: 装配简化为 B/C 两态**

A 分支（:736-768，含 :721-722 R-H 注释、混合配置 warn :740-745、open :747、grid_on :748、spawn legacy :749-764）整段删。装配收敛：
```rust
    // ── S3 §10.3：策略 phase 源装配（master_meter 段已删除收敛，2026-09-09 S3b-1c）──
    //   B. south_stations.stations 非空 → southd scheduler：grid 单写 AiIntegrator。
    //      B1. 含 meter_grid → grid_on=true（SouthSink.on_grid_package 单写；offline 由事件 +
    //          AiIntegrator 5s 闸门判断，pv/load 不兜底）。
    //      B2. 仅非 grid 站 → grid_on=false → pv/load 南向模拟兜底测量（无 grid 源不断供）。
    //   C. 无 stations → grid_on=false → pv/load 南向模拟兜底。
    let mut grid_on = false;
    if !config.south_stations.stations.is_empty() {
        if config.south_stations.grid_station().is_none() {
            tracing::warn!(...B2 无 meter_grid warn...);
        }
        let sink = Arc::new(SouthSink::new(...));
        ...buses open（现 :786-804 逻辑不变）...
        let scheduler = SouthScheduler::new(config.south_stations.clone(), buses, sink);
        let handles = scheduler.spawn();
        grid_on = config.south_stations.grid_station().is_some();
        for h in handles { guard.0.push(h); }
        tracing::info!(...);
    } else {
        tracing::info!("无 south_stations 站：策略 phase 由 pv/load 南向模拟兜底");
    }
```
（现 B 分支 :769-834 主体搬为唯一分支，删 :736-768 A 与 :772-777 B2 warn 保留。C :835-839 并入 else。）

- [ ] **Step 3: 编译 + 既有测试**

```bash
cd /e/MUPC2/mupc && cargo check -p mupc-core-bin -j 2 2>&1 | tail -3
cd /e/MUPC2/mupc && cargo test -p mupc-core-bin -j 2 2>&1 | tail -3
```
Expected：0 errors；既有测试绿（grid_on 语义经 Task 2 删 R-H 后无 master_meter 分支——确认无 startup 测试引用 master_meter）。

---

### Task 4: 注释/语义清理（mapper.rs + grid_convergence.rs + deploy.md）

**Files:**
- Modify: `mupc/crates/mupc-southd/src/mapper.rs`（注释 :5/:88）
- Modify: `mupc/crates/mupc-southd/tests/grid_convergence.rs`（注释 :2-3/:61）
- Modify: `docs/deploy.md`（:356-359/:382-385，若引用 master_meter 段——接线表改 south_stations）

**背景**：删 legacy 后注释引用失效。

- [ ] **Step 1: mapper.rs 注释清理**

"完整移植 startup.rs read_master_meter"（:5/:88）→ "meter_grid 语义（与已删除的 legacy read_master_meter 等价，收敛锚 grid_convergence.rs）"。

- [ ] **Step 2: grid_convergence.rs 注释清理**

:2-3 "legacy read_master_meter 语义" → "总表收敛后唯一语义（master_meter 已删）"；:61 同。

- [ ] **Step 3: deploy.md 接线表清理**

:356-359/:382-385 master_meter 引用 → south_stations.meter_grid（或删旧表项）。

- [ ] **Step 4: 构建**

```bash
cd /e/MUPC2/mupc && cargo test -p mupc-southd -j 2 2>&1 | tail -3
cd /e/MUPC2/mupc && cargo test -p mupc-southd --test grid_convergence -j 2 2>&1 | tail -3
```
Expected：注释改动零行为；grid_convergence 4 passed（独立 canned 不依赖 legacy 已确认）。

---

### Task 5: deploy yaml — dev + production 迁移

**Files:**
- Modify: `mupc/deploy/config/mupc_core_config.yaml`
- Modify: `mupc/deploy/config/mupc_core_config.production.yaml`

**背景**：**production 现 master_meter.enabled:true 在用 legacy**（现场已确认可迁）。删代码字段后 serde 静默忽略多余键——若不迁 production，grid 源消失落 pv/load 兜底（危险）。须同 PR 迁移。

- [ ] **Step 1: dev yaml 删 master_meter 段 + south_stations 去注释（示例形态）**

`mupc_core_config.yaml`：删 master_meter 段 :80-94。south_stations 注释块（:96-140）去注释启用（grid_meter 全配示例 :118-131，含 baud_rate 9600/slave 3/regs :125-131）——**dev 模板展示收敛后形态**。文件头注释同步（"master_meter 段已删除，总表走 south_stations.meter_grid"）。

- [ ] **Step 2: production yaml 删 master_meter 段 + 启用 south_stations.meter_grid（真点表）**

`mupc_core_config.production.yaml`：
- 删 master_meter 段 :84-100（含 enabled:true :89、reg_map 占位 :94-100）。
- south_stations 注释块 :102-149 去注释启用：grid_meter 站 port `/dev/ttyS4`、baud_rate 9600、slave 3、interval_ms 1000、**regs 填真点表**（与 legacy reg_map 同址 0x0000-0x001E，块名 p/q/pf/u/i/p_total——从已删 master_meter.reg_map 值搬入）。
- 文件头/迁移注释清理。

- [ ] **Step 3: YAML 合法 + 配置加载验证**

```bash
cd /e/MUPC2/mupc && python -c "import yaml; yaml.safe_load(open('E:/MUPC2/mupc/deploy/config/mupc_core_config.yaml',encoding='utf-8')); yaml.safe_load(open('E:/MUPC2/mupc/deploy/config/mupc_core_config.production.yaml',encoding='utf-8')); print('YAML OK')"
cd /e/MUPC2/mupc && cargo test -p mupc-core-bin -j 2 core_config 2>&1 | tail -3
```
（若模板被 core_config 加载测试引用则确认；production yaml 手工/测试校验无 master_meter 键 + south_stations.meter_grid 生效。）

- [ ] **Step 4: 全量回归 + workspace check**

```bash
cd /e/MUPC2/mupc && cargo test -p mupc-southd -p mupc-core-bin -p mupc-strategy-engine -j 2 2>&1 | grep -E "test result" | head
cd /e/MUPC2/mupc && cargo check --workspace -j 2 --exclude mupc-iec61850-plugin --exclude rs485-plugin --exclude device-trait 2>&1 | tail -3
cd /e/MUPC2/mupc && cargo clippy -p mupc-southd -p mupc-core-bin -p mupc-strategy-engine -j 2 2>&1 | grep -E "^error" | head
```

- [ ] **Step 5: 提交**

```bash
cd /e/MUPC2 && git add mupc/deploy/config/mupc_core_config.yaml mupc/deploy/config/mupc_core_config.production.yaml
git commit -m "feat(deploy): 删 master_meter 段，总表收敛 south_stations.meter_grid（production 迁移真点表）"
```

---

## Self-Review 对照（02 §10.3 R-H 收敛目标 → S3b-1c）

| 设计要求 | 计划落点 |
|---|---|
| master_meter 段收敛删除（02 §10.3 目标形态） | Task 1/2/3（字段/struct/validate/A 分支/legacy 函数删） |
| 删除后 validate 简化（R-H/占口/同串口互斥移除） | Task 2（south_stations 承担收敛） |
| startup grid_on 收敛（south_stations 唯一源） | Task 3 |
| 总表回归闸门零破坏（grid_convergence 锚） | Task 4（独立 canned，注释清理） |
| production 实际迁移（删 legacy + 启用 meter_grid） | Task 5（现场已确认） |
| 模板/接线文档同步 | Task 4/5 |

**Placeholder/一致性**：无 TBD。删除面依赖探索精确行号（core_config :558/:573/:589/:736/:771/:785 等）。Task 1/2 同文件强耦合——controller 可合并执行或 Task 2 紧随 Task 1（Task 1 单独会 validate 编译破，故 Task 1+2 须在一轮内绿）。production 迁移是**生效配置改动**——现场已确认收敛，但提交前 controller 应向用户明示此变更（production 从 legacy 切 south_stations 路径）。
