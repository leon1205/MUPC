# PCS 对接：ModbusRtuTransport 重构为 PCS 真实协议 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `intercore` 的 `ModbusRtuTransport`（transport=modbus_rtu）从早期假设点表重构为**两级式 PCS 真实协议 V1.3** 驱动：分相下行（模式2 + 单相 P/Q FC06 逐写，clamp ±25）、恒功率下行（模式0 + 1001/1002）、SOC 读 3 区 1010、心跳/在线读 3 区 1013、int16 缩放 + 高 8/低 8 字节互换。上层接口不变。

**Architecture:** PCS = 实时控制模块（用户定论）。`ModbusRtuTransport` 作 EMS/Modbus Master 直连 PCS（Slave 拨码默认 1，19200 N-8-1）。移除假设表 cmd_valid/exec 确认区语义（FC06 写响应即确认）；新增 PCS 点表/编解码模块（int16 + 字节 swap）；`modbus_rtu.rs`（假设 int32 编解码）与 `bin/modbus_slave.rs`（假设表 slave）标注旧路径/仿真专用，不再被新实现引用。

**Tech Stack:** Rust, tokio-modbus（FC04 读输入寄存器 / FC06 写单寄存器）, tokio。

**设计依据：** 10 核间设计文档 §11.9（v2.2）+ ADR-013；PCS 协议 V1.3 PDF（`60kW 双级式PCS产品资料包/5.通讯协议/`）。

**测试命令**（Windows Git-bash，cwd = `mupc/`，cargo 全路径 `/c/Users/Administrator/.cargo/bin/cargo.exe` + `--manifest-path /e/MUPC2/mupc/Cargo.toml`，前台勿后台）：
```bash
cargo test -p mupc-intercore --lib
cargo check --workspace
```

---

## Task 1: PCS 点表与编解码模块（pcs.rs）

**Files:**
- Create: `mupc/crates/intercore/src/pcs.rs`
- Modify: `mupc/crates/intercore/src/lib.rs`

- [ ] **Step 1: 写失败测试**（先建模块含测试，编译红）

`mupc/crates/intercore/src/pcs.rs` 内容（模块 + 测试）先落地，随后跑测试。测试覆盖：
- int16 缩放 roundtrip（含负值、±25 clamp）
- 字节 swap（`0x1234` ↔ `0x3412`）
- 点表常量值

- [ ] **Step 2: 运行测试验证**

Run: `cargo test -p mupc-intercore --lib pcs`
Expected: FAIL（`pcs` 模块未在 lib.rs 声明）

- [ ] **Step 3: 实现 pcs.rs**

```rust
//! 两级式 PCS 设备 Modbus 点表与编解码（协议 V1.3，v2.2）
//!
//! PCS = 实时控制模块，MUPC 作 EMS/Modbus Master 直连（transport=modbus_rtu）。
//! 4 区保持寄存器（FC03 读 / FC06 写单）；3 区输入寄存器（FC04 读）。
//! ⚠️ 高 8 位/低 8 位互换：寄存器 u16 收发须 swap_bytes。

/// 4 区 模块启停（0 停机 / 1 运行）
pub const REG_START_STOP: u16 = 500;
/// 4 区 有功模式
pub const REG_MODE: u16 = 1000;
pub const MODE_CONST_POWER: u16 = 0; // 交流恒功率
pub const MODE_PHASE_SPLIT: u16 = 2; // 交流分相
/// 4 区 恒功率有功/无功设置（Int16 *1kW，正放负充）
pub const REG_CONST_P_SET: u16 = 1001;
pub const REG_CONST_Q_SET: u16 = 1002;
/// 4 区 单 A/B/C 有功（1006-1008）与无功（1009-1011），Int16 *1kW，单相 ±25
pub const REG_PHASE_P_A: u16 = 1006;
pub const REG_PHASE_Q_A: u16 = 1009;
/// 3 区 BMS 系统 SOC（*1%）
pub const REG_SOC: u16 = 1010;
/// 3 区 模块运行状态（0 停机 / 1 待机 / 2 充电 / 3 放电）
pub const REG_RUN_STATE: u16 = 1013;
/// 单相功率限幅（kW）
pub const PHASE_LIMIT_KW: f64 = 25.0;

/// PCS 端 int16 值 → u16（含字节互换：高 8/低 8 位互换）
pub fn to_pcs_reg(v: f64) -> u16 {
    (v.round() as i16 as u16).swap_bytes()
}

/// PCS 端 u16 → f64（含字节互换回解）
pub fn from_pcs_reg(r: u16) -> f64 {
    (r.swap_bytes() as i16) as f64
}

/// 分相 P/Q 值 clamp 到 ±25 kW/kVar
pub fn clamp_phase(v: f64) -> f64 {
    v.clamp(-PHASE_LIMIT_KW, PHASE_LIMIT_KW)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_swap() {
        assert_eq!(to_pcs_reg(4660.0).swap_bytes() as i16, 4660); // 4660=0x1234
    }

    #[test]
    fn test_int16_roundtrip() {
        for v in [-25.0, -1.0, 0.0, 23.5, 25.0] {
            let r = to_pcs_reg(v);
            assert!((from_pcs_reg(r) - v).abs() < 1.0);
        }
    }

    #[test]
    fn test_clamp_phase() {
        assert_eq!(clamp_phase(30.0), 25.0);
        assert_eq!(clamp_phase(-30.0), -25.0);
        assert_eq!(clamp_phase(10.0), 10.0);
    }

    #[test]
    fn test_point_constants() {
        assert_eq!(REG_MODE, 1000);
        assert_eq!(REG_PHASE_P_A, 1006);
        assert_eq!(REG_SOC, 1010);
        assert_eq!(REG_RUN_STATE, 1013);
        assert_eq!(MODE_PHASE_SPLIT, 2);
    }
}
```

- [ ] **Step 4: lib.rs 注册**

```rust
pub mod pcs;
```

- [ ] **Step 5: 运行测试验证通过**

Run: `cargo test -p mupc-intercore --lib pcs`
Expected: 4 PASS

- [ ] **Step 6: Commit**

```bash
git add crates/intercore/src/pcs.rs crates/intercore/src/lib.rs
git commit -m "feat: PCS 点表与 int16+字节互换编解码（pcs.rs，v2.2 真实协议）"
```

---

## Task 2: ModbusRtuTransport 重构为 PCS 驱动

**Files:**
- Modify: `mupc/crates/intercore/src/transport/modbus.rs`

- [ ] **Step 1: Read 现状，理解假设表逻辑移除点**

先 Read `mupc/crates/intercore/src/transport/modbus.rs` 与 `mupc/crates/intercore/src/modbus_rtu.rs`（假设表将不被新实现引用）。确认要替换的方法：`send_tai_command`/`send_dual_param`（现走假设 cmd_ctrl/issue/exec）、`latest_soc`（现 None）、心跳读（现 `REG_HEARTBEAT`=0x0100 假设）。`modbus_rtu.rs` 假设编解码在重构后不再被 transport 引用。

- [ ] **Step 2: 加 PCS 读写原语**

`transport/modbus.rs` 顶部 `use crate::pcs::*;`。新增 FC06 单写与 FC04 读（tokio-modbus `Reader::read_input_registers` / `Writer::write_single_register`）：
- 现 `write_regs`（FC16 多写）替换/新增 `write_reg(addr, value)`（FC06 `ctx.write_single_register(addr, value)`）
- 新增 `read_input(addr, len)`（FC04 `ctx.read_input_registers(addr, len)`），`read_regs`（FC03 读保持）保留给...PCS 4 区也用 FC03 读（心跳走 3 区 FC04）。保留 `read_regs`（FC03）供未来 4 区回读；PCS 心跳/SOC 走 FC04。

```rust
/// FC06 写单寄存器（PCS 4 区）
async fn write_reg(&self, addr: u16, value: u16) -> Result<(), MupcError> {
    let mut ctx = open_ctx(&self.settings).await?;
    timeout(Duration::from_millis(self.settings.response_timeout_ms), ctx.write_single_register(addr, value)).await
        .map_err(|_| MupcError::new(ErrorCode::IntercoreTimeout, "modbus write timeout", "intercore"))?
        .map_err(|e| MupcError::new(ErrorCode::SendFailed, format!("write: {}", e), "intercore"))
}

/// FC04 读输入寄存器（PCS 3 区）
async fn read_input(&self, addr: u16, len: u16) -> Result<Vec<u16>, MupcError> {
    let mut ctx = open_ctx(&self.settings).await?;
    timeout(Duration::from_millis(self.settings.response_timeout_ms), ctx.read_input_registers(addr, len)).await
        .map_err(|_| MupcError::new(ErrorCode::IntercoreTimeout, "modbus read timeout", "intercore"))?
        .map_err(|e| MupcError::new(ErrorCode::SendFailed, format!("read: {}", e), "intercore"))
}
```

（若 tokio-modbus 0.13.1 的 `read_input_registers`/`write_single_register` 在 prelude/Reader/Writer trait 上，以实际签名适配——先 Read tokio-modbus 源码确认。）

- [ ] **Step 3: 模式与启停管理**

加字段 `mode: std::sync::atomic::AtomicU8`（0 未设），`started: RwLock<bool>`：

```rust
async fn ensure_mode(&self, mode: u16) -> Result<(), MupcError> {
    let cur = self.mode.load(Ordering::Relaxed) as u16;
    if cur != mode {
        self.write_reg(REG_MODE, mode as u16).await?; // PCS 寄存器非字节互换（模式字）——按 PCS 端序；若互换则 to_pcs_reg
        self.mode.store(mode as u8, Ordering::Relaxed);
    }
    Ok(())
}

async fn ensure_started(&self) -> Result<(), MupcError> {
    if !*self.started.read().await {
        self.write_reg(REG_START_STOP, to_pcs_reg(1.0)).await?;
        *self.started.write().await = true;
    }
    Ok(())
}
```

> 注：模式字 500/1000 是否也受"高 8/低 8 互换"影响——协议标注全设备寄存器互换，故统一 `to_pcs_reg`/`from_pcs_reg`；若实机不符（如控制字不互换）调为直写并记录（PCS 契约待确认项）。

- [ ] **Step 4: 重写 send_tai_command / send_dual_param**

```rust
async fn send_tai_command(&self, p: [f64; 3], q: [f64; 3], _mode: &str) -> Result<(), MupcError> {
    self.ensure_mode(MODE_PHASE_SPLIT).await?;
    self.ensure_started().await?;
    // 单相 clamp ±25（PCS 分相功率器件独立，超限静默裁剪、无回读告警——已知局限）
    for (i, reg) in [REG_PHASE_P_A, REG_PHASE_P_A + 1, REG_PHASE_P_A + 2].iter().enumerate() {
        self.write_reg(*reg, to_pcs_reg(clamp_phase(p[i]))).await?;
    }
    for (i, reg) in [REG_PHASE_Q_A, REG_PHASE_Q_A + 1, REG_PHASE_Q_A + 2].iter().enumerate() {
        self.write_reg(*reg, to_pcs_reg(clamp_phase(q[i]))).await?;
    }
    tracing::debug!("PCS 分相下发: p={:?} q={:?}", p, q);
    Ok(())
}

async fn send_dual_param(&self, cmd: &DualParamCommand) -> Result<(), MupcError> {
    // PCS 恒功率无下垂接口：k_droop 忽略（v2.2 语义偏离，AI 恒功率下发前须经 AiValidator 范围校验）
    self.ensure_mode(MODE_CONST_POWER).await?;
    self.ensure_started().await?;
    self.write_reg(REG_CONST_P_SET, to_pcs_reg(cmd.p_ref)).await?;
    self.write_reg(REG_CONST_Q_SET, to_pcs_reg(0.0)).await?;
    tracing::debug!("PCS 恒功率下发: p_ref={}（k_droop 忽略）", cmd.p_ref);
    Ok(())
}
```

- [ ] **Step 5: latest_soc 读 3 区 1010**

```rust
async fn latest_soc(&self) -> Option<(f64, Instant)> {
    match self.read_input(REG_SOC, 1).await {
        Ok(r) if !r.is_empty() => {
            let soc = from_pcs_reg(r[0]);
            if soc.is_finite() && (0.0..=100.0).contains(&soc) {
                *self.soc.write().await = Some((soc, Instant::now()));
                self.soc.write().await.map(|g| g.clone()).ok().flatten()
            } else {
                None
            }
        }
        _ => None,
    }
}
```

（若 `self.soc` 字段已存在（N3 通用）则复用；无则新增 `soc: RwLock<Option<(f64, Instant)>>`。）

- [ ] **Step 6: 心跳/在线改读 3 区 1013**

现 `probe_heartbeat`/`run_heartbeat_loop`（假设表读 0x0100）改为读 `REG_RUN_STATE`（FC04）——成功即在线（run_state ∈ 0..=3），连续失败判离线。若现实现按 0x0100 心跳计数则替换为"读 1013 成功判在线"。删除假设表 heartbeat_counter 引用。

- [ ] **Step 7: 移除假设表逻辑**

删除 `issue()`/exec 确认轮询、`cmd_ctrl`/`protocol_version`/假设 `REG_*` 写路径（原 `use crate::modbus_rtu::*` 若不再用则移除）。`probe_heartbeat`/`run_heartbeat_loop` 保留（改读 1013）。删除后 `cargo check` 清未用 import/死代码。

- [ ] **Step 8: 编译 + 测试**

Run: `cargo check -p mupc-intercore` → 修编译错；`cargo test -p mupc-intercore --lib`
Expected: 全绿（保留 protocol/tcp 测试 + pcs 4 测试）

- [ ] **Step 9: Commit**

```bash
git add crates/intercore/src/transport/modbus.rs
git commit -m "refactor: ModbusRtuTransport 重构为 PCS 真实协议驱动（FC06 分相/恒功率 + 3区 SOC/心跳）"
```

---

## Task 3: 旧路径标注（假设表不参与新实现）

**Files:**
- Modify: `mupc/crates/intercore/src/modbus_rtu.rs`
- Modify: `mupc/crates/intercore/src/bin/modbus_slave.rs`
- Modify: `mupc/crates/intercore/src/lib.rs`（若导出变化）

- [ ] **Step 1: 标注假设模块**

若 `modbus_rtu.rs`（假设 int32/cmd_ctrl 编解码）与 `bin/modbus_slave.rs`（假设表 slave）在新 transport 重构后不再被生产路径引用，则在其文件头加注：
```
//! ⚠️ 早期假设点表（自定义 cmd_ctrl/exec 确认）——已被 PCS 真实协议（§11.9/pcs.rs）取代。
//! 保留仅供旧路径仿真/历史参考；生产 transport=modbus_rtu 走 pcs.rs 驱动。
```
若 `lib.rs` 仍导出其类型但无生产引用，确认无 dead-code 门禁冲突（保留导出但注释说明即可）。

- [ ] **Step 2: 编译 + 测试确认无回归**

Run: `cargo check --workspace`（确认重构后无 crate 引用假设表被破坏）；`cargo test -p mupc-intercore --lib`（全绿）

- [ ] **Step 3: Commit**

```bash
git add crates/intercore/src/modbus_rtu.rs crates/intercore/src/bin/modbus_slave.rs crates/intercore/src/lib.rs
git commit -m "docs: PCS 重构后标注假设点表为旧路径/仿真专用"
```

---

## Task 4: 文档与端到端验证准备

**Files:**
- Modify: `mupc/deploy/config/mupc_core_config.yaml`（intercore 段注释已是 19200，核验）
- Modify: `E:/MUPC2/docs/superpowers/plans/modules/10-MUPC-核间通信-设计文档.md`（§11.9 验证状态补记）

- [ ] **Step 1: 核验配置示例**

Read `mupc/deploy/config/mupc_core_config.yaml` intercore/modbus_rtu 段——`baud_rate` 应为 19200（v2.2 已改，核验保留）；注释标注生产 PCS 主链路 / tcp 仿真。

- [ ] **Step 2: 文档验证状态**

10 设计文档 §11.9 后补「验证状态（2026-09-04）」：pcs.rs 编解码单测 + ModbusRtuTransport 重构编译/单测通过；端到端 PCS 实机 RS485 联调待具备 PCS 硬件（填点表/核相后）。标注 PCS 契约待确认清单仍未厂方答复。

- [ ] **Step 3: workspace 回归**

Run: `cargo check --workspace`（0 error）；`cargo test -p mupc-intercore --lib`（全绿）

- [ ] **Step 4: Commit**

```bash
git add deploy/config/mupc_core_config.yaml docs/superpowers/plans/modules/10-MUPC-核间通信-设计文档.md
git commit -m "docs: PCS 重构验证状态与配置核验（v2.2）"
```

---

## 自审记录

- **Spec 覆盖（10 设计 §11.9 + ADR-013）**：点表/字节互换/编解码 → Task 1；分相/恒功率下行 + 单相 clamp + 模式/启停管理 → Task 2；SOC 3 区 1010 + 心跳 1013 → Task 2；k_droop 忽略 + AiValidator 兜底（代码注释）→ Task 2；旧假设表标注仿真 → Task 3；配置/文档 → Task 4。
- **占位符扫描**：无 TBD；关键代码完整。tokio-modbus FC04/FC06 方法名以实际源码为准（各 Task 注明先 Read）。
- **类型一致性**：`to_pcs_reg`/`from_pcs_reg`/`clamp_phase` 在 Task 1 定义、Task 2 使用一致；`REG_*`/`MODE_*` 常量名贯穿。
- **遗留（非本计划）**：PCS 契约待厂方确认清单（模式热切/启停时序/502-503 使能/符号核相）；125kVA 型号点表；多台并机——均在文档 §11.9 标注，不在本轮实现。
