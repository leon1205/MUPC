//! S3b-2 T3 —— **AC-1**（配置解析与校验）验收用例（PRD §9.9.1 / 设计 §11.11.2）。
//!
//! 输入 = `tests/fixtures/south_stations_s3b2.yaml`（= PRD §9.4.1 的 6 站参考配置，逐字照录）。
//!
//! 三级断言（AC-1 ①/②/③）：
//! - **②** 完整 6 站 YAML 解析通过 + 除 §9.4.3 明列条件外无拒绝（`validate` → `Ok`）；
//! - **①** 剥离本轮新增字段/取值后的**既有字段子集**仍解析通过，且既有 `validate()` 的
//!   拒绝条件逐条不触发；
//! - **③** §9.4.3 的 17 条 + 设计补落点的 2 条（规则 18/19）逐条各一个最小坏配置 → `Err`，
//!   并含 §11.11.2 点名的边界用例（规则 15 的 6 种形态 / 规则 6 无行放行 / 规则 18 的
//!   499·500 边界 / 规则 19 的 count 3·4·discrete）。

use mupc_data_processing::meter_regs::RegFormat;
use mupc_southd::config::{RegFunc, Role, SouthStationsConfig, StationParity};
use serde::Deserialize;

#[derive(Deserialize)]
struct Wrapper {
    south_stations: SouthStationsConfig,
}

fn parse(yaml: &str) -> SouthStationsConfig {
    serde_yaml::from_str::<Wrapper>(yaml)
        .expect("解析失败")
        .south_stations
}

/// PRD §9.4.1 的 6 站参考配置（AC-1 的验收输入）
const REF: &str = include_str!("fixtures/south_stations_s3b2.yaml");

/// 单站 YAML 构造（`body` 为 6 空格缩进的附加行，通常是 [`regs_of`] 的产物）
fn one_station(role: &str, body: &str) -> SouthStationsConfig {
    parse(&format!(
        "south_stations:\n  stations:\n    - id: s1\n      role: {role}\n      port: ttyS1\n      slave: 1\n      interval_ms: 1000\n{body}"
    ))
}

/// 把块行（无缩进）包成 `regs:` 段
fn regs_of(body: &str) -> String {
    let mut s = String::from("      regs:\n");
    for line in body.lines() {
        s.push_str("        ");
        s.push_str(line);
        s.push('\n');
    }
    s
}

/// 一段纯 `points` 块行（16 位，声明 `n` 个连续点，恰好覆盖 count）
fn pts_block(name: &str, addr: u16, count: u16, n: u16) -> String {
    format!(
        "- {{ name: {name}, addr: {addr}, count: {count}, format: uint16, scale: 1.0, points: [{{ at: 1, count: {n} }}] }}"
    )
}

/// 一段无 `points` 块行（16 位整窗口）
fn plain_block(name: &str, addr: u16, count: u16) -> String {
    format!("- {{ name: {name}, addr: {addr}, count: {count}, format: uint16, scale: 1.0 }}")
}

/// 流式单块 `regs`（供流式站行内插；等价于 [`regs_of`] + [`plain_block`]）
const FLOW_REGS: &str = "regs: [{ name: z, addr: 10, count: 2, format: uint16, scale: 1.0 }]";

fn assert_err_contains(cfg: &SouthStationsConfig, needle: &str, what: &str) {
    let err = cfg.validate().expect_err(&format!("{what} 应被拒"));
    assert!(err.contains(needle), "{what}：Err 应含 {needle:?}，实际 {err}");
}

fn assert_ok(cfg: &SouthStationsConfig, what: &str) {
    assert!(cfg.validate().is_ok(), "{what} 应通过：{:?}", cfg.validate());
}

// ═══════════════════════════ AC-1 ② 完整 6 站 ═══════════════════════════

#[test]
fn ac1_full_reference_config_parses_and_passes() {
    let cfg = parse(REF);
    assert_eq!(cfg.stations.len(), 6, "§9.4.1 为 6 站");
    assert_eq!(cfg.poll_ms, 1000);
    assert_eq!(cfg.stale_timeout_s, 5);

    // 本轮新增字段/取值确实被解析（否则 AC-1 ② 的"完整 YAML"名不副实）
    let bms = cfg.stations.iter().find(|s| s.id == "bms").unwrap();
    assert_eq!(bms.role, Role::Battery);
    assert_eq!(bms.parity, StationParity::None);
    let bms_io = bms.regs.iter().find(|b| b.name == "bms_io").unwrap();
    assert_eq!(bms_io.points.len(), 19, "bms_io 逐点声明 19 条");
    let i116 = bms_io.points.iter().find(|p| p.at == 17).unwrap();
    assert_eq!(i116.offset, Some(-1600.0), "116（at 17）簇组电流零点平移");
    assert_eq!(i116.format, Some(RegFormat::Uint16), "原文 UNIT → UINT 的推断订正");
    let soc_pt = bms_io.points.iter().find(|p| p.name.as_deref() == Some("soc")).unwrap();
    assert_eq!(soc_pt.at, 19, "寄存器 118 = addr 100 + (19−1)");

    let pcs = cfg.stations.iter().find(|s| s.id == "pcs").unwrap();
    assert_eq!(pcs.role, Role::Pcs);
    assert!(pcs.regs[0].byte_swap, "pcs_3zone 字节低-高互换");
    assert_eq!(pcs.regs[0].points.len(), 31);
    assert!(pcs.regs[0].points.iter().any(|p| p.word_order
        == mupc_data_processing::meter_regs::WordOrder::LoHi));

    let fire = cfg.stations.iter().find(|s| s.id == "fire").unwrap();
    assert!(fire.regs[0].points.iter().any(|p| p.name.as_deref() == Some("fire_det_count")));

    let hvac = cfg.stations.iter().find(|s| s.id == "hvac").unwrap();
    assert_eq!(hvac.parity, StationParity::Even, "空调出厂偶校验");
    assert_eq!(hvac.regs[1].func, RegFunc::Discrete, "hvac_di = FC02 位块");

    // 除 §9.4.3 明列条件外不触发任何既有/新增拒绝（含规则 15 的"六站必然通过"）
    assert_ok(&cfg, "完整 6 站参考配置");
}

/// 仅 `grid_meter` 六相量块（既有形态，无 `points`）→ Ok
/// （AC-1 ② 中"仅含 grid_meter 六相量块的配置均 Ok"的可执行断言）
#[test]
fn ac1_grid_meter_only_passes() {
    let mut cfg = parse(REF);
    cfg.stations.retain(|s| s.role == Role::MeterGrid);
    assert_eq!(cfg.stations.len(), 1);
    assert_eq!(cfg.stations[0].regs.len(), 6);
    assert_ok(&cfg, "仅 grid_meter 六相量块（6 块地址严格相邻但均无 points）");
}

// ═══════════════════════════ AC-1 ① 既有字段子集 ═══════════════════════════

/// §9.4.1 的 6 站**剥离本轮新增字段/取值**后的既有形制：
/// 去掉 `parity`、`byte_swap`、`offset`、`points`、`word_order`、`func: discrete` 块
/// 与所有站级新增取值；`grid_meter` 站保持生效配置取值**逐字不变**。
const LEGACY_SUBSET: &str = r#"
south_stations:
  poll_ms: 1000
  stale_timeout_s: 5
  stations:
    - id: bms
      role: battery
      port: "/dev/ttyS2"
      protocol: modbus
      slave: 1
      baud_rate: 9600
      interval_ms: 1000
      regs:
        - { name: bms_io,     func: input, addr: 100,  count: 31, format: uint16,       scale: 1.0 }
        - { name: bms_energy, func: input, addr: 139,  count: 19, format: int32_scaled, scale: 0.1 }
        - { name: bms_meta,   func: input, addr: 181,  count: 9,  format: uint16,       scale: 1.0 }
        - { name: bms_term,   func: input, addr: 2991, count: 4,  format: uint16,       scale: 1.0 }
        - { name: bms_cap,    func: input, addr: 4000, count: 6,  format: uint16,       scale: 1.0 }
    - id: pcs
      role: pcs
      port: "/dev/ttyS7"
      protocol: modbus
      slave: 1
      baud_rate: 19200
      interval_ms: 1000
      regs:
        - { name: pcs_3zone, func: input, addr: 1000, count: 76, format: uint16, scale: 1.0 }
    - id: meter_batt
      role: meter_batt
      port: "/dev/ttyS5"
      protocol: modbus
      slave: 1
      baud_rate: 9600
      interval_ms: 1000
      regs:
        - { name: mb_e_act_comb, func: holding, addr: 0x0000, count: 2, format: int32_scaled, scale: 0.01 }
        - { name: mb_e_act_fwd,  func: holding, addr: 0x000A, count: 2, format: int32_scaled, scale: 0.01 }
        - { name: mb_e_act_rev,  func: holding, addr: 0x0014, count: 2, format: int32_scaled, scale: 0.01 }
        - { name: mb_e_rea_comb, func: holding, addr: 0x001E, count: 2, format: int32_scaled, scale: 0.01 }
        - { name: mb_e_rea_fwd,  func: holding, addr: 0x0028, count: 2, format: int32_scaled, scale: 0.01 }
        - { name: mb_e_rea_rev,  func: holding, addr: 0x0032, count: 2, format: int32_scaled, scale: 0.01 }
        - { name: mb_ui,        func: holding, addr: 0x0061, count: 6,  format: uint16,       scale: 0.1 }
        - { name: mb_freq_line, func: holding, addr: 0x0077, count: 4,  format: uint16,       scale: 0.1 }
        - { name: mb_phase,     func: holding, addr: 0x0087, count: 14, format: int32_scaled, scale: 0.01 }
        - { name: mb_power,     func: holding, addr: 0x0164, count: 28, format: int32_scaled, scale: 0.001 }
    - id: fire
      role: fire
      port: "/dev/ttyS6"
      protocol: modbus
      slave: 1
      baud_rate: 9600
      interval_ms: 1000
      regs:
        - { name: fire_sys, func: holding, addr: 4,  count: 13,  format: uint16, scale: 1.0 }
        - { name: fire_det, func: holding, addr: 17, count: 114, format: uint16, scale: 1.0 }
    - id: hvac
      role: hvac
      port: "/dev/ttyS3"
      protocol: modbus
      slave: 1
      baud_rate: 9600
      interval_ms: 5000
      regs:
        - { name: hvac_in, func: input, addr: 0, count: 4, format: int16, scale: 0.1 }
    - id: grid_meter
      role: meter_grid
      port: "/dev/ttyS4"
      baud_rate: 9600
      protocol: modbus
      slave: 3
      interval_ms: 1000
      regs:
        - { name: p,       addr: 0x1000, format: int32_scaled, scale: 0.01,  count: 6 }
        - { name: q,       addr: 0x1006, format: int32_scaled, scale: 0.01,  count: 6 }
        - { name: pf,      addr: 0x100C, format: int32_scaled, scale: 0.001, count: 6 }
        - { name: u,       addr: 0x1012, format: float32,       scale: 1.0,   count: 6 }
        - { name: i,       addr: 0x1018, format: int32_scaled, scale: 0.01,  count: 6 }
        - { name: p_total, addr: 0x101E, format: int32_scaled, scale: 0.01,  count: 2 }
"#;

/// **AC-1 ①**：既有字段子集解析通过，且既有 `validate()` 的拒绝条件**逐条不触发**。
///
/// 判法：① 断言剥离确实生效（不含任何新增字段/取值）；② `grid_meter` 站与生效配置取值
/// **逐字一致**（AC-1 ① 明写的"必然通过"锚）；③ 对**除 battery 站以外**的 5 站调用
/// `validate()` → `Ok`（既有条件全部内联在 `validate()` 里，且新规则对它们亦不触发）；
/// ④ battery 站单独看，**唯一**被触发的拒绝是本轮新增的规则 4（`soc` 点契约——`points`
/// 被剥离后不再有点名 `soc` 的点），即**没有任何既有拒绝条件被触发**。
#[test]
fn ac1_legacy_field_subset_parses_and_legacy_conditions_do_not_fire() {
    let cfg = parse(LEGACY_SUBSET);
    assert_eq!(cfg.stations.len(), 6);

    // ① 剥离生效：既有形制里不含任何新增字段/取值
    for s in &cfg.stations {
        assert_eq!(s.parity, StationParity::None, "站 {} 的 parity 已剥离", s.id);
        for b in &s.regs {
            assert!(b.points.is_empty(), "块 {} 的 points 已剥离", b.name);
            assert_eq!(b.offset, 0.0);
            assert!(!b.byte_swap);
            assert!(!b.read_slice);
            assert_ne!(b.func, RegFunc::Discrete, "func: discrete 块已剥离");
        }
    }

    // ② grid_meter 站与生效配置取值逐字一致（`deploy/config/mupc_core_config.yaml`）
    let g = cfg.grid_station().expect("meter_grid 站在");
    assert_eq!(g.id, "grid_meter", "站 id 是 grid_meter（meter_grid 只是 role 名）");
    assert_eq!(g.slave, 3);
    assert_eq!(g.interval_ms, 1000);
    let want = [
        ("p", 0x1000u16, RegFormat::Int32Scaled, 0.01, 6u16),
        ("q", 0x1006, RegFormat::Int32Scaled, 0.01, 6),
        ("pf", 0x100C, RegFormat::Int32Scaled, 0.001, 6),
        ("u", 0x1012, RegFormat::Float32, 1.0, 6),
        ("i", 0x1018, RegFormat::Int32Scaled, 0.01, 6),
        ("p_total", 0x101E, RegFormat::Int32Scaled, 0.01, 2),
    ];
    for (name, addr, format, scale, count) in want {
        let b = g.regs.iter().find(|b| b.name == name).expect("相量块齐备");
        assert_eq!((b.addr, b.format, b.scale, b.count), (addr, format, scale, count));
    }

    // ③ 除 battery 站以外的 5 站：既有拒绝条件逐条不触发
    let mut without_battery = parse(LEGACY_SUBSET);
    without_battery.stations.retain(|s| s.role != Role::Battery);
    assert_eq!(without_battery.stations.len(), 5);
    assert_ok(&without_battery, "既有字段子集（grid_meter/pcs/meter_batt/fire/hvac）");

    // ④ battery 站：唯一被触发的拒绝是**本轮新增**的规则 4（`soc` 点契约）或规则 19
    //   （无 `points` 的 32 位块宽度护栏——`bms_energy` 的 `count: 19` 在 `points` 被剥离后
    //   不再是 32 位宽度 2 的整数倍）。二者**都是本轮新增的拒绝条件**，不在 AC-1 ① 的
    //   "既有拒绝条件"清单内 ⇒ 既有条件逐条不触发成立。
    let mut battery_only = parse(LEGACY_SUBSET);
    battery_only.stations.retain(|s| s.role == Role::Battery);
    let err = battery_only.validate().unwrap_err();
    assert!(
        err.contains("soc") || err.contains("整数倍"),
        "既有字段子集只允许被本轮新增的规则拒（规则 4 / 规则 19），实际: {err}"
    );
}

// ═══════════════════════════ AC-1 ③ 逐条拒绝 ═══════════════════════════

/// 规则 1（新 role 合法性）：未知取值由 serde 拒；`pcs` 解析通过。
#[test]
fn ac1_rule1_unknown_role_rejected_by_serde() {
    let yaml = "south_stations:\n  stations:\n    - { id: s1, role: nonsense, port: ttyS1, slave: 1 }";
    assert!(
        serde_yaml::from_str::<Wrapper>(yaml).is_err(),
        "未知名 role 应在反序列化期被拒（无静默 fallback）"
    );
    let cfg = one_station(
        "pcs",
        &regs_of(&plain_block("z", 10, 2)),
    );
    assert_eq!(cfg.stations[0].role, Role::Pcs, "role: pcs 应解析通过");
}

/// 规则 2（单站约束）：`pcs` 站 > 1 → Err。
#[test]
fn ac1_rule2_multiple_pcs_rejected() {
    let cfg = parse(&format!(
        "south_stations:\n  stations:\n    - {{ id: a, role: pcs, port: t1, slave: 1, interval_ms: 1000, {FLOW_REGS} }}\n    - {{ id: b, role: pcs, port: t2, slave: 1, interval_ms: 1000, {FLOW_REGS} }}"
    ));
    assert_err_contains(&cfg, "至多一个 pcs", "两个 pcs 站");
}

/// 规则 3（必填点表）：`pcs` 站 regs 为空 → Err。
#[test]
fn ac1_rule3_pcs_empty_regs_rejected() {
    assert_err_contains(&one_station("pcs", ""), "regs 为空", "pcs 站空 regs");
}

/// 规则 4（`soc` 点契约）：battery 站点名集合无 `soc` → Err；
/// **块名为 `soc` 已不再是契约**（v1.2 的"块名为 soc"随换算口径下沉到点而修订）。
#[test]
fn ac1_rule4_battery_soc_point_contract() {
    assert_err_contains(
        &one_station("battery", &regs_of(&plain_block("bms_io", 118, 1))),
        "soc",
        "battery 站无点名 soc 的点",
    );
    assert_err_contains(
        &one_station("battery", &regs_of(&plain_block("soc", 118, 1))),
        "soc",
        "块名为 soc 但无名点（契约锚定点名，不是块名）",
    );
    let ok = one_station(
        "battery",
        &regs_of("- { name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }"),
    );
    assert_ok(&ok, "battery 站点名 soc 的点存在");
}

/// 规则 5（格式与标度）：`int32_scaled`/`uint16`/`int16` 的块级或点级 `scale == 0` → Err；
/// `float32` 与 `discrete` 块不适用。
#[test]
fn ac1_rule5_zero_scale_rejected() {
    for body in [
        "- { name: z, addr: 10, count: 1, format: uint16 }",
        "- { name: z, addr: 10, count: 1, format: int16 }",
        "- { name: z, addr: 10, count: 2, format: int32_scaled }",
        // 点级 `None` = 继承块级（0.0）→ 只判一次，仍拒
        "- { name: z, addr: 10, count: 4, format: float32, points: [{ at: 1, format: uint16 }] }",
        // 点级显式 0
        "- { name: z, addr: 10, count: 4, format: uint16, scale: 1.0, points: [{ at: 1, format: uint16, scale: 0.0 }] }",
    ] {
        assert_err_contains(&one_station("hvac", &regs_of(body)), "scale", body);
    }
    assert_ok(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 4, format: float32 }")),
        "float32 块缺 scale（既有语义：float32 不乘 scale）",
    );
    assert_ok(
        &one_station("hvac", &regs_of("- { name: d, func: discrete, addr: 10, count: 8 }")),
        "discrete 块不适用 scale 规则（位值恒 0/1）",
    );
}

/// 规则 6 ③（负向）：`lookup` 查不到行 → **放行**（RC-3 现场改 `addr` 基准的合法配置
/// 不得被拒）。规则 6 的 ①/② 判定核心由 `config.rs` 的 `check_symbolicity_row` 单测钉住
/// （点表 618 行转写属 T4，届时另补配置级用例）。
#[test]
fn ac1_rule6_unknown_registry_row_passes_through() {
    let cfg = one_station(
        "hvac",
        &regs_of("- { name: z, addr: 5000, count: 2, format: uint16, scale: 1.0, points: [{ at: 1, offset: -40.0 }, { at: 2, offset: -40.0 }] }"),
    );
    assert_ok(&cfg, "uint16 + 非零 offset 但点表无行（addr 基准已改）");
}

/// 规则 7（点位越界）：点覆盖区间超出 `[0, count)` → Err。
#[test]
fn ac1_rule7_point_out_of_window_rejected() {
    assert_err_contains(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 4, format: uint16, scale: 1.0, points: [{ at: 1 }, { at: 5 }] }")),
        "越界",
        "点 at=5 超出 count=4",
    );
}

/// 规则 8（点位重叠）：同块内两点覆盖同一寄存器 → Err。
#[test]
fn ac1_rule8_overlapping_points_rejected() {
    assert_err_contains(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 4, format: uint16, scale: 1.0, points: [{ at: 1 }, { at: 1 }] }")),
        "点位重叠",
        "两点同占寄存器 0",
    );
}

/// 规则 9（32 位点对齐）：`at + 1 > count`（半个 32 位值）→ Err。
#[test]
fn ac1_rule9_32bit_point_alignment_rejected() {
    assert_err_contains(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 4, format: uint16, scale: 1.0, points: [{ at: 1 }, { at: 4, format: int32_scaled, scale: 1.0 }] }")),
        "越界",
        "32 位点跨窗口末尾",
    );
}

/// 规则 10（点名唯一）：**块名不同、展开后 metric 相同**（只有本规则能拒的形态）
/// → Err 含"点名重复"。
///
/// 与既有 `meter_grid` 块名唯一规则的分工：两块同名（都叫 `p`）由**既有块名唯一规则**先拒
/// （文案"块名重复"，见 `config.rs` 的 C2 用例），本用例**不得**用它退化替换（§11.5.3.4.1）。
#[test]
fn ac1_rule10_duplicate_metric_rejected() {
    let body = format!(
        "{}\n{}",
        "- { name: a_blk, addr: 10, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: dup }] }",
        "- { name: b_blk, addr: 20, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: dup }] }"
    );
    assert_err_contains(&one_station("hvac", &regs_of(&body)), "点名重复", "两块各自点名 dup");
}

/// 规则 11（空洞上限 + 窗口首尾锚定，仅作用于声明了 `points` 的标量块）。
#[test]
fn ac1_rule11_hole_cap_and_anchoring() {
    // 连续空洞 6 > 4
    assert_err_contains(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 8, format: uint16, scale: 1.0, points: [{ at: 1 }, { at: 8 }] }")),
        "空洞",
        "块内空洞 6 寄存器",
    );
    // 前导未声明寄存器（首点未锚定偏移 0）
    assert_err_contains(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 4, format: uint16, scale: 1.0, points: [{ at: 2 }] }")),
        "首尾锚定",
        "首点落在偏移 1",
    );
    // 末尾未声明寄存器（末点末端 ≠ count）
    assert_err_contains(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 4, format: uint16, scale: 1.0, points: [{ at: 1 }] }")),
        "首尾锚定",
        "末点末端 1 ≠ count 4",
    );
}

/// 规则 12（位块上限）：`discrete` 块 `count > 2000` 或 `count == 0` → Err。
#[test]
fn ac1_rule12_discrete_bit_cap() {
    assert_err_contains(
        &one_station("hvac", &regs_of("- { name: d, func: discrete, addr: 10, count: 2001 }")),
        "位块上限",
        "discrete count=2001",
    );
    assert_err_contains(
        &one_station("hvac", &regs_of("- { name: d, func: discrete, addr: 10, count: 0 }")),
        "count 须 > 0",
        "discrete count=0",
    );
    assert_ok(
        &one_station("hvac", &regs_of("- { name: d, func: discrete, addr: 10, count: 2000 }")),
        "discrete count=2000（上界内）",
    );
}

/// 规则 13（地址有效性）：`addr == 0` 仅 `meter_batt`/`hvac` 合法。
#[test]
fn ac1_rule13_addr_zero_by_role() {
    for role in ["fire", "pcs", "battery"] {
        let extra = if role == "battery" {
            "- { name: z, addr: 0, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }"
        } else {
            "- { name: z, addr: 0, count: 1, format: uint16, scale: 1.0 }"
        };
        assert_err_contains(&one_station(role, &regs_of(extra)), "addr 不能为 0", role);
    }
    assert_ok(
        &one_station("meter_batt", &regs_of("- { name: z, addr: 0, count: 2, format: int32_scaled, scale: 0.01 }")),
        "ADL400 电能块首址即 0",
    );
    assert_ok(
        &one_station("hvac", &regs_of("- { name: z, addr: 0, count: 2, format: int16, scale: 0.1 }")),
        "空调 FC04 首址亦为 0",
    );
}

/// 规则 14（区间与重叠，按 `func` 空间分别判）：同 func 重叠 → Err；
/// 同址不同 func（PCS 3 区/4 区形态）→ Ok。
#[test]
fn ac1_rule14_overlap_by_func_space() {
    assert_err_contains(
        &one_station(
            "hvac",
            &regs_of(&format!("{}\n{}", plain_block("a", 10, 4), plain_block("b", 12, 4))),
        ),
        "区间重叠",
        "同 holding 空间 [10,14) 与 [12,16)",
    );
    let pcs = one_station(
        "pcs",
        &regs_of("- { name: a, func: holding, addr: 10, count: 4, format: uint16, scale: 1.0 }\n- { name: b, func: input, addr: 10, count: 4, format: uint16, scale: 1.0 }"),
    );
    assert_ok(&pcs, "同址不同 func（三套地址空间独立）");
}

/// **规则 15（块落地极大性）** —— AC-1 ③ 点名的 6 种形态 + 判据 ③ 的等价性钉子。
#[test]
fn ac1_rule15_maximality_forms() {
    // 反向：两块**均声明 points**、地址严格相邻、合并 4 ≤ 120、无 read_slice → Err
    let bad = one_station(
        "hvac",
        &regs_of(&format!("{}\n{}", pts_block("a", 10, 2, 2), pts_block("b", 12, 2, 2))),
    );
    assert_err_contains(&bad, "应合并", "两块均声明 points 且可一次读回");

    // ③ 等价性钉子（§11.5.2(1)）：两块**各自合法、各含 4 寄存器内部空洞**、地址连续
    //   ⇒ 接缝空洞 0 + 各块内部空洞 ≤ 4 ⇒ 合并后每段空洞仍 ≤ 4（③ 由规则 11 恒成立）
    //   ⇒ ③ **不引入额外拒绝**：判定由 ①②④ 给出（仍 Err）。
    //   ⚠️ §11.11.2 该条写"断言 Ok"，与 §11.5.1 #15 / §11.5.2 的合取判据（①②③④ 全成立 ⇒ 拒）
    //   矛盾；此处按 §11.5.1/§11.5.2（与 PRD v1.8 逐条对齐处）执行并上报（见 T3 交付报告）。
    let holey = |name: &str, addr: u16| {
        format!("- {{ name: {name}, addr: {addr}, count: 6, format: uint16, scale: 1.0, points: [{{ at: 1 }}, {{ at: 6 }}] }}")
    };
    let with_holes = one_station(
        "hvac",
        &regs_of(&format!("{}\n{}", holey("a", 10), holey("b", 16))),
    );
    assert_err_contains(&with_holes, "应合并", "③ 恒成立 ⇒ 判据仍由 ①②④ 给出 Err");

    // 豁免：任一块标 read_slice → Ok
    let sliced = one_station(
        "hvac",
        &regs_of(&format!(
            "{}\n- {{ name: b, addr: 12, count: 2, format: uint16, scale: 1.0, read_slice: true, points: [{{ at: 1, count: 2 }}] }}",
            pts_block("a", 10, 2, 2)
        )),
    );
    assert_ok(&sliced, "任一块 read_slice=true 豁免规则 15");

    // 合并后 count > 120（fire 形态 127）→ Ok（本就该分片）
    let big = one_station(
        "hvac",
        &regs_of(&format!(
            "{}\n{}",
            pts_block("a", 10, 61, 61),
            pts_block("b", 71, 66, 66)
        )),
    );
    assert_ok(&big, "合并后 127 > 120（fire 形态）");

    // 相邻的一方未声明 points（grid_meter / fire 形态）→ Ok
    let one_side = one_station(
        "hvac",
        &regs_of(&format!("{}\n{}", pts_block("a", 10, 2, 2), plain_block("b", 12, 2))),
    );
    assert_ok(&one_side, "相邻一方未声明 points ⇒ 不参与判定");

    // 两块均未声明 points 且严格相邻（grid_meter 形态）→ Ok
    let legacy_pair = one_station(
        "hvac",
        &regs_of(&format!("{}\n{}", plain_block("a", 10, 6), plain_block("b", 16, 6))),
    );
    assert_ok(&legacy_pair, "两块均未声明 points 且严格相邻（既有 meter_grid 形态）");

    // `discrete` 块相邻（保守读法 Δ-8：一律不参与）→ Ok
    let bits = one_station(
        "hvac",
        &regs_of("- { name: d1, func: discrete, addr: 200, count: 4, points: [{ at: 1, count: 4 }] }\n- { name: d2, func: discrete, addr: 204, count: 4, points: [{ at: 1, count: 4 }] }"),
    );
    assert_ok(&bits, "discrete 块相邻（保守读法：discrete 一律不参与）");
}

/// 规则 16（同口一致性）：同 `port` 的 `parity` 必须一致（与 `baud_rate` 同形态）。
#[test]
fn ac1_rule16_same_port_parity_consistency() {
    let bad = parse(
        "south_stations:\n  stations:\n    - { id: h1, role: hvac, port: t1, slave: 1, interval_ms: 2000, parity: even, regs: [{ name: a, addr: 0, count: 2, format: uint16, scale: 1.0 }] }\n    - { id: h2, role: hvac, port: t1, slave: 2, interval_ms: 2000, parity: none, regs: [{ name: b, addr: 10, count: 2, format: uint16, scale: 1.0 }] }",
    );
    assert_err_contains(&bad, "parity", "同口 even 与 none");
    let ok = parse(
        "south_stations:\n  stations:\n    - { id: h1, role: hvac, port: t1, slave: 1, interval_ms: 2000, parity: even, regs: [{ name: a, addr: 0, count: 2, format: uint16, scale: 1.0 }] }\n    - { id: h2, role: hvac, port: t1, slave: 2, interval_ms: 2000, parity: even, regs: [{ name: b, addr: 10, count: 2, format: uint16, scale: 1.0 }] }",
    );
    assert_ok(&ok, "同口同 parity");
}

/// 规则 18（`pcs` 站周期下界，设计补落点）：`499` → Err、`500` → Ok；
/// `pcs` **无** `< 5000` 上界（与 meter_grid/battery 不同）。
#[test]
fn ac1_rule18_pcs_interval_lower_bound() {
    let mk = |iv: u64| {
        parse(&format!(
            "south_stations:\n  stations:\n    - id: s1\n      role: pcs\n      port: ttyS1\n      slave: 1\n      interval_ms: {iv}\n{}",
            regs_of(&plain_block("z", 10, 2))
        ))
    };
    assert_err_contains(&mk(499), "须 ≥ 500ms", "pcs interval=499");
    assert_ok(&mk(500), "pcs interval=500（下界）");
    assert_ok(&mk(30000), "pcs 无 <5000 上界（不参与控制决策）");
}

/// 规则 19（无 `points` 块的宽度护栏，设计补落点）：32 位块 `count` 非宽度整数倍 → Err；
/// `count: 4` → Ok；`discrete` 块不适用。
#[test]
fn ac1_rule19_width_guard_without_points() {
    assert_err_contains(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 3, format: int32_scaled, scale: 1.0 }")),
        "整数倍",
        "32 位块 count=3（尾槽静默少产点）",
    );
    assert_ok(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 4, format: int32_scaled, scale: 1.0 }")),
        "32 位块 count=4",
    );
    assert_ok(
        &one_station("hvac", &regs_of("- { name: d, func: discrete, addr: 10, count: 31 }")),
        "discrete 块 count=31（位数无宽度概念，31 非 8 倍数亦合法）",
    );
    // 声明了 `points` 的块不适用本条：count=3 非 32 位宽度整数倍，但逐点声明后由
    // 规则 11 的首尾锚定 + 逐点宽度保证无"尾槽装不下"（此块：2 个 16 位点恰好覆盖 3 寄存器）
    assert_ok(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 3, format: int32_scaled, scale: 1.0, points: [{ at: 1, format: uint16, scale: 1.0 }, { at: 3, format: uint16, scale: 1.0 }] }")),
        "声明了 points 的块不适用规则 19",
    );
}
