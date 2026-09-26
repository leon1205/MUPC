//! S3b-2 T3 —— **AC-1**（配置解析与校验）验收用例（PRD §9.9.1 / 设计 §11.11.2）。
//!
//! 输入 = `tests/fixtures/south_stations_s3b2.yaml`（= PRD §9.4.1 的参考配置，逐字照录；
//! **Task 6 起站级段为 5 站** —— PCS 已按 ADR-016 迁至 `tests/fixtures/south_pcs_s3b2.yaml`，
//! 局部门禁见 [`REF_PCS`] 与 `ac1_rule18_pcs_interval_lower_bound`）。
//!
//! 三级断言（AC-1 ①/②/③）：
//! - **②** 完整参考 YAML 解析通过 + 除 §9.4.3 明列条件外无拒绝（`validate` → `Ok`）；
//! - **①** 剥离本轮新增字段/取值后的**既有字段子集**仍解析通过，且既有 `validate()` 的
//!   拒绝条件逐条不触发；
//! - **③** §9.4.3 的 17 条 + 设计补落点的 2 条（规则 18/19）逐条各一个最小坏配置 → `Err`，
//!   并含 §11.11.2 点名的边界用例（规则 15 的 6 种形态 / 规则 6 无行放行 / 规则 18 的
//!   499·500 边界（Task 6 起在 `south_pcs` 段）/ 规则 19 的 count 3·4·discrete）；
//!   另加 Task 6 的新增拒绝：**规则 P-3**（站级段不收 `role: pcs`，文案指向 `south_pcs`）。

use mupc_data_processing::meter_regs::RegFormat;
use mupc_southd::config::{RegFunc, Role, SouthPcsConfig, SouthStationsConfig, StationParity};
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

/// `south_pcs` 段解析（裸结构，无外层键——见该 fixture 头部注释）
fn parse_pcs(yaml: &str) -> SouthPcsConfig {
    serde_yaml::from_str::<SouthPcsConfig>(yaml).expect("south_pcs 解析失败")
}

/// PRD §9.4.1 的参考配置（AC-1 的验收输入；Task 6 起站级段为 5 站，PCS 见 [`REF_PCS`]）
const REF: &str = include_str!("fixtures/south_stations_s3b2.yaml");

/// PCS 独立顶层段（Task 6 / ADR-016）
const REF_PCS: &str = include_str!("fixtures/south_pcs_s3b2.yaml");

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
    assert!(
        err.contains(needle),
        "{what}：Err 应含 {needle:?}，实际 {err}"
    );
}

fn assert_ok(cfg: &SouthStationsConfig, what: &str) {
    assert!(
        cfg.validate().is_ok(),
        "{what} 应通过：{:?}",
        cfg.validate()
    );
}

// ═══════════════════════════ AC-1 ② 完整 5 站 ═══════════════════════════

#[test]
fn ac1_full_reference_config_parses_and_passes() {
    let cfg = parse(REF);
    // Task 6（ADR-016 / 设计 §13.7）：站级段由 6 站变为 5 站 —— PCS 已迁至顶层段
    // `south_pcs`（下方单独断言），站级段**不再接受** `role: pcs`（规则 P-3）。
    assert_eq!(
        cfg.stations.len(),
        5,
        "Task 6 起站级段为 5 站（PCS 已迁出）"
    );
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
    assert_eq!(
        i116.format,
        Some(RegFormat::Uint16),
        "原文 UNIT → UINT 的推断订正"
    );
    let soc_pt = bms_io
        .points
        .iter()
        .find(|p| p.name.as_deref() == Some("soc"))
        .unwrap();
    assert_eq!(soc_pt.at, 19, "寄存器 118 = addr 100 + (19−1)");

    // PCS 段（独立顶层段）：`pcs_3zone` 的字节低-高互换 + 4 个 `lo_hi` 电量点
    let pcs = parse_pcs(REF_PCS);
    assert!(pcs.enabled, "参考 PCS 段须 enabled（否则段内校验整体短路）");
    assert_eq!(pcs.validate(), Ok(()), "参考 PCS 段须通过段内校验");
    assert!(pcs.regs[0].byte_swap, "pcs_3zone 字节低-高互换");
    assert_eq!(pcs.regs[0].points.len(), 31);
    assert!(pcs.regs[0]
        .points
        .iter()
        .any(|p| p.word_order == mupc_data_processing::meter_regs::WordOrder::LoHi));

    let fire = cfg.stations.iter().find(|s| s.id == "fire").unwrap();
    assert!(fire.regs[0]
        .points
        .iter()
        .any(|p| p.name.as_deref() == Some("fire_det_count")));

    let hvac = cfg.stations.iter().find(|s| s.id == "hvac").unwrap();
    assert_eq!(hvac.parity, StationParity::Even, "空调出厂偶校验");
    assert_eq!(hvac.regs[1].func, RegFunc::Discrete, "hvac_di = FC02 位块");

    // 除 §9.4.3 明列条件外不触发任何既有/新增拒绝（含规则 15 的"五站必然通过"）
    assert_ok(&cfg, "完整 5 站参考配置");
}

/// 仅 `grid_meter` 六相量块（既有形态，无 `points`）→ Ok
/// （AC-1 ② 中"仅含 grid_meter 六相量块的配置均 Ok"的可执行断言）
#[test]
fn ac1_grid_meter_only_passes() {
    let mut cfg = parse(REF);
    cfg.stations.retain(|s| s.role == Role::MeterGrid);
    assert_eq!(cfg.stations.len(), 1);
    assert_eq!(cfg.stations[0].regs.len(), 6);
    assert_ok(
        &cfg,
        "仅 grid_meter 六相量块（6 块地址严格相邻但均无 points）",
    );
}

// ═══════════════════════════ AC-1 ① 既有字段子集 ═══════════════════════════

/// §9.4.1 的**剥离本轮新增字段/取值**后的既有形制（Task 6 起为 5 站 —— PCS 站已按
/// ADR-016 迁出站级段）：去掉 `parity`、`byte_swap`、`offset`、`points`、`word_order`、
/// `func: discrete` 块与所有站级新增取值；`grid_meter` 站保持生效配置取值**逐字不变**。
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
    assert_eq!(
        cfg.stations.len(),
        5,
        "Task 6 起既有形制为 5 站（PCS 已迁出）"
    );

    // ① 剥离生效：既有形制里不含任何新增字段/取值
    for s in &cfg.stations {
        assert_eq!(
            s.parity,
            StationParity::None,
            "站 {} 的 parity 已剥离",
            s.id
        );
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
    assert_eq!(
        g.id, "grid_meter",
        "站 id 是 grid_meter（meter_grid 只是 role 名）"
    );
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
        assert_eq!(
            (b.addr, b.format, b.scale, b.count),
            (addr, format, scale, count)
        );
    }

    // ③ 除 battery 站以外的 4 站：既有拒绝条件逐条不触发
    let mut without_battery = parse(LEGACY_SUBSET);
    without_battery.stations.retain(|s| s.role != Role::Battery);
    assert_eq!(without_battery.stations.len(), 4);
    assert_ok(
        &without_battery,
        "既有字段子集（grid_meter/meter_batt/fire/hvac）",
    );

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
    let yaml =
        "south_stations:\n  stations:\n    - { id: s1, role: nonsense, port: ttyS1, slave: 1 }";
    assert!(
        serde_yaml::from_str::<Wrapper>(yaml).is_err(),
        "未知名 role 应在反序列化期被拒（无静默 fallback）"
    );
    let cfg = one_station("pcs", &regs_of(&plain_block("z", 10, 2)));
    assert_eq!(cfg.stations[0].role, Role::Pcs, "role: pcs 应解析通过");
}

/// **规则 P-3**（ADR-016 / 设计 §13.7）：站级段**不再接受** `role: pcs`，拒并指向
/// `south_pcs` 段。
///
/// **本条合并了原 `ac1_rule2_multiple_pcs_rejected` 与 `ac1_rule3_pcs_empty_regs_rejected`**
/// （2026-09-26 规格评审处置）：P-3 落地后，两条原用例都只断言 `Err` 含 `south_pcs`，
/// 而**实测**把 rule2 缩成**一个** pcs 站、把 rule3 的 `regs` **填满**，两条**仍全绿**
/// ⇒ 对"至多一个 pcs 站"与"空 regs"**零判别力**（名不副实），且与
/// `config::south_pcs_tests::south_stations_rejects_pcs_role` 完全重复。
/// 故按 P-3 语义合并重命名为本条：钉住"**只要**站级出现 `role: pcs`（不论几台、
/// 不论 `regs` 空否）即被拒，且文案含站 id + 指向 `south_pcs`"。
///
/// 原两条规则的新落点（见 `SouthStationsConfig::validate` 注释）：
/// - 规则 2「至多一个 pcs 站」⇒ 由 P-3 **单段化**天然满足（`south_pcs` 是单值段）；
/// - 规则 3「pcs `regs` 非空」⇒ 迁入 [`SouthPcsConfig::validate`] 的 `regs 为空` 分支，
///   单测覆盖见 `config::south_pcs_tests::south_pcs_validate_rejects_dead_or_illegal_configs`。
#[test]
fn ac1_rule_p3_station_level_pcs_rejected_points_to_south_pcs() {
    // ① 单台、`regs` 非空：仍拒（原 rule2 的真实判据是"至多一个"，与台数无关）
    let one = one_station("pcs", &regs_of(&plain_block("z", 10, 2)));
    let err = one.validate().unwrap_err();
    assert!(
        err.contains("south_pcs") && err.contains("role: pcs") && err.contains("s1"),
        "P-3 文案须指向 south_pcs + 含站名，实际: {err}"
    );
    // ② 单台、`regs` **空**：同样拒，且拒因是 **P-3 文案**（正向标记与 ① 同源：
    //    `role: pcs`）—— 证明"空点表"判据已不在站级路径（它随 P-3 单段化迁至
    //    `south_pcs` 段的 `regs 为空` 分支）。
    //    ⚠️ 此处原为负向断言 `!err.contains("regs 为空")`（2026-09-26 质量评审 Minor #4）：
    //    站级路径已无该文案，它实际只证明"P-3 文案里不含这 4 个字"，一旦为帮配置者把 P-3
    //    文案补上"（regs 为空也会被拒）"就会**误红** ⇒ 改用与 ① 同源的正向标记。
    let empty = one_station("pcs", "");
    let err = empty.validate().unwrap_err();
    assert!(
        err.contains("south_pcs") && err.contains("role: pcs"),
        "空 regs 的 pcs 站须先被 P-3 拒（正向标记：P-3 文案含 `role: pcs`），实际: {err}"
    );
    // ③ 两台：首台即被 P-3 拒（"至多一个 pcs 站"结构性不可达）
    let cfg = parse(&format!(
        "south_stations:\n  stations:\n    - {{ id: a, role: pcs, port: t1, slave: 1, interval_ms: 1000, {FLOW_REGS} }}\n    - {{ id: b, role: pcs, port: t2, slave: 1, interval_ms: 1000, {FLOW_REGS} }}"
    ));
    assert_err_contains(&cfg, "south_pcs", "两个 pcs 站（首个即被 P-3 拒）");
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
        &one_station(
            "hvac",
            &regs_of("- { name: z, addr: 10, count: 4, format: float32 }"),
        ),
        "float32 块缺 scale（既有语义：float32 不乘 scale）",
    );
    assert_ok(
        &one_station(
            "hvac",
            &regs_of("- { name: d, func: discrete, addr: 10, count: 8 }"),
        ),
        "discrete 块不适用 scale 规则（位值恒 0/1）",
    );
}

/// 规则 6 ③（负向）：`lookup` 查不到行 → **放行**（RC-3 现场改 `addr` 基准的合法配置
/// 不得被拒）。
#[test]
fn ac1_rule6_unknown_registry_row_passes_through() {
    let cfg = one_station(
        "hvac",
        &regs_of("- { name: z, addr: 5000, count: 2, format: uint16, scale: 1.0, points: [{ at: 1, offset: -40.0 }, { at: 2, offset: -40.0 }] }"),
    );
    assert_ok(&cfg, "uint16 + 非零 offset 但点表无行（addr 基准已改）");
}

/// 规则 6 ②（**T4 补：点表已落 510 行 ⇒ 配置级用例可达**）：`lookup` 命中而行内 `offset`
/// 与配置不等（含"漏配 → 缺省 0 ≠ −1600/−40"）→ Err；相等 → 放行。
///
/// 期望值取自 `point_table::POINT_REGS` 的 BMS 行（116 = −1600.0、117 = −40.0，PRD §9.5.1）。
#[test]
fn ac1_rule6_registry_offset_drift_rejected() {
    // 正例：与点表登记值逐点一致（+ `soc` 点满足规则 4）
    let ok = one_station(
        "battery",
        &regs_of(
            "- { name: bms_io, addr: 100, count: 19, format: uint16, scale: 1.0, points: [ \
               { at: 1, count: 16 }, \
               { at: 17, offset: -1600.0 }, \
               { at: 18, offset: -40.0 }, \
               { at: 19, name: soc } ] }",
        ),
    );
    assert_ok(&ok, "116/117/118 与点表登记值一致");
    assert_eq!(
        mupc_southd::point_table::lookup(Role::Battery, 116)
            .unwrap()
            .offset,
        -1600.0
    );

    // 反例 1：**漏配 offset**（缺省 0 ≠ −1600，这正是"现场抄点表时漏了零点平移"的形态）
    assert_err_contains(
        &one_station(
            "battery",
            &regs_of(
                "- { name: bms_io, addr: 100, count: 19, format: uint16, scale: 1.0, points: [ \
                   { at: 1, count: 18 }, { at: 19, name: soc } ] }",
            ),
        ),
        "与点表登记值",
        "116 漏配 offset（0 ≠ −1600）",
    );

    // 反例 2：**配错值**（−1500 ≠ −1600）
    assert_err_contains(
        &one_station(
            "battery",
            &regs_of(
                "- { name: bms_io, addr: 100, count: 19, format: uint16, scale: 1.0, points: [ \
                   { at: 1, count: 16 }, { at: 17, offset: -1500.0 }, { at: 19, name: soc } ] }",
            ),
        ),
        "与点表登记值",
        "116 配了 −1500（≠ 登记的 −1600）",
    );

    // 反例 3：117 的 −40 抄成 −50
    assert_err_contains(
        &one_station(
            "battery",
            &regs_of(
                "- { name: bms_io, addr: 100, count: 19, format: uint16, scale: 1.0, points: [ \
                   { at: 1, count: 16 }, { at: 17, offset: -1600.0 }, { at: 18, offset: -50.0 }, \
                   { at: 19, name: soc } ] }",
            ),
        ),
        "与点表登记值",
        "117 配了 −50（≠ 登记的 −40）",
    );
}

/// 规则 6 的**适用边界**（三条，均由 T4 的真实点表决定）：
/// ① 查表按 `role` 隔离 —— 同址在别的 role 上无行 ⇒ 放行（不误伤）；
/// ② 非 16 位格式不参与本条（`int32_scaled`/`float32` ⇒ 放行，PRD §9.4.3 只要求 16 位可追溯）；
/// ③ `sym_src` 空 + `offset ≠ 0` 的 ① 形态在**真实点表下结构性不可达**（BMS 全部非零
///   offset 行都已登记来源），故 ① 由 `config.rs::symbolicity_rejects_untraceable_offset`
///   的行级单测与 `point_table_vs_reference_config.rs::registry_internal_invariants`
///   的全表不变量共同钉住（配置级无法构造出该形态 —— 这本身就是"表已自洽"的证明）。
#[test]
fn ac1_rule6_scope_boundaries() {
    // ① role 隔离：BMS 的 116 在**别的 role** 上无登记行
    //    （Task 6 起改用 `hvac`：站级段已不收 `role: pcs`，而"查表按 role 隔离"与具体 role 无关）
    assert_ok(
        &one_station(
            "hvac",
            &regs_of("- { name: hvac_x, addr: 100, count: 17, format: uint16, scale: 1.0, points: [ { at: 1, count: 16 }, { at: 17, offset: -1600.0 } ] }"),
        ),
        "Hvac/116 无登记行（查表按 role 隔离）",
    );
    // ② int32_scaled 覆盖 116 时整块不参与符号性判定
    assert_ok(
        &one_station(
            "hvac",
            &regs_of(
                "- { name: z, addr: 100, count: 18, format: int32_scaled, scale: 0.1, points: [ \
                   { at: 1 }, { at: 3 }, { at: 5 }, { at: 7 }, { at: 9 }, { at: 11 }, { at: 13 }, \
                   { at: 15 }, { at: 17 } ] }",
            ),
        ),
        "int32_scaled 点不参与规则 6（116 被 32 位点覆盖）",
    );
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
    assert_err_contains(
        &one_station("hvac", &regs_of(&body)),
        "点名重复",
        "两块各自点名 dup",
    );
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
        &one_station(
            "hvac",
            &regs_of("- { name: d, func: discrete, addr: 10, count: 2001 }"),
        ),
        "位块上限",
        "discrete count=2001",
    );
    assert_err_contains(
        &one_station(
            "hvac",
            &regs_of("- { name: d, func: discrete, addr: 10, count: 0 }"),
        ),
        "count 须 > 0",
        "discrete count=0",
    );
    assert_ok(
        &one_station(
            "hvac",
            &regs_of("- { name: d, func: discrete, addr: 10, count: 2000 }"),
        ),
        "discrete count=2000（上界内）",
    );
}

/// 规则 13（地址有效性）：`addr == 0` 仅 `meter_batt`/`hvac` 合法。
///
/// Task 6 起 `pcs` 不再列入被拒 role（站级段已不收 `role: pcs`，P-3 先命中）。
#[test]
fn ac1_rule13_addr_zero_by_role() {
    for role in ["fire", "battery"] {
        let extra = if role == "battery" {
            "- { name: z, addr: 0, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }"
        } else {
            "- { name: z, addr: 0, count: 1, format: uint16, scale: 1.0 }"
        };
        assert_err_contains(&one_station(role, &regs_of(extra)), "addr 不能为 0", role);
    }
    assert_ok(
        &one_station(
            "meter_batt",
            &regs_of("- { name: z, addr: 0, count: 2, format: int32_scaled, scale: 0.01 }"),
        ),
        "ADL400 电能块首址即 0",
    );
    assert_ok(
        &one_station(
            "hvac",
            &regs_of("- { name: z, addr: 0, count: 2, format: int16, scale: 0.1 }"),
        ),
        "空调 FC04 首址亦为 0",
    );
}

/// 规则 14（区间与重叠，按 `func` 空间分别判）：同 func 重叠 → Err；
/// 同址不同 func（PCS 3 区/4 区的形态）→ Ok。
///
/// Task 6 起"同址不同 func ⇒ 不重叠"改用 `hvac` 表达（站级段已不收 `role: pcs`）；
/// 该规则本身与 role 无关，断言效力不变。
#[test]
fn ac1_rule14_overlap_by_func_space() {
    assert_err_contains(
        &one_station(
            "hvac",
            &regs_of(&format!(
                "{}\n{}",
                plain_block("a", 10, 4),
                plain_block("b", 12, 4)
            )),
        ),
        "区间重叠",
        "同 holding 空间 [10,14) 与 [12,16)",
    );
    let same_addr_diff_func = one_station(
        "hvac",
        &regs_of("- { name: a, func: holding, addr: 10, count: 4, format: uint16, scale: 1.0 }\n- { name: b, func: input, addr: 10, count: 4, format: uint16, scale: 1.0 }"),
    );
    assert_ok(&same_addr_diff_func, "同址不同 func（三套地址空间独立）");
}

/// **规则 15（块落地极大性）** —— AC-1 ③ 点名的 6 种形态 + 判据 ③ 的等价性钉子。
#[test]
fn ac1_rule15_maximality_forms() {
    // 反向：两块**均声明 points**、地址严格相邻、合并 4 ≤ 120、无 read_slice → Err
    let bad = one_station(
        "hvac",
        &regs_of(&format!(
            "{}\n{}",
            pts_block("a", 10, 2, 2),
            pts_block("b", 12, 2, 2)
        )),
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
        &regs_of(&format!(
            "{}\n{}",
            pts_block("a", 10, 2, 2),
            plain_block("b", 12, 2)
        )),
    );
    assert_ok(&one_side, "相邻一方未声明 points ⇒ 不参与判定");

    // 两块均未声明 points 且严格相邻（grid_meter 形态）→ Ok
    let legacy_pair = one_station(
        "hvac",
        &regs_of(&format!(
            "{}\n{}",
            plain_block("a", 10, 6),
            plain_block("b", 16, 6)
        )),
    );
    assert_ok(
        &legacy_pair,
        "两块均未声明 points 且严格相邻（既有 meter_grid 形态）",
    );

    // `discrete` 块相邻（保守读法 Δ-8：一律不参与）→ Ok
    let bits = one_station(
        "hvac",
        &regs_of("- { name: d1, func: discrete, addr: 200, count: 4, points: [{ at: 1, count: 4 }] }\n- { name: d2, func: discrete, addr: 204, count: 4, points: [{ at: 1, count: 4 }] }"),
    );
    assert_ok(&bits, "discrete 块相邻（保守读法：discrete 一律不参与）");
}

/// **规则 15 判据 ② 与「块书写顺序」无关**（S1 回归钉子 —— 首轮评审实测：同址集**逆序书写**
/// 曾被放行）。
///
/// 判据 ② 的语义是"**地址**严格相邻"，而 YAML 里块的**书写顺序**是自由的 —— 逆序书写时
/// 若按列表顺序两两配对，`b.addr == a.addr + a.count` 永不成立 ⇒ 该判形态被放行、极大性形同
/// 虚设。故配对应按**地址序**枚举。本用例把同一对块**两种书写顺序**各跑一遍，两者都必须 `Err`。
#[test]
fn ac1_rule15_maximality_independent_of_written_order() {
    // 正序（低地址块在前）——与既有 `ac1_rule15_maximality_forms` 的反向形态同形
    let asc = one_station(
        "hvac",
        &regs_of(&format!(
            "{}\n{}",
            pts_block("a", 10, 2, 2),
            pts_block("b", 12, 2, 2)
        )),
    );
    assert_err_contains(&asc, "应合并", "正序书写：a@10(2) → b@12(2)");

    // 逆序（高地址块在前）：同一对块、同一形态 ⇒ 必须同样 `Err`（书写顺序不得影响判定）
    let desc = one_station(
        "hvac",
        &regs_of(&format!(
            "{}\n{}",
            pts_block("b", 12, 2, 2),
            pts_block("a", 10, 2, 2)
        )),
    );
    assert_err_contains(
        &desc,
        "应合并",
        "逆序书写：b@12(2) → a@10(2)（同址集逆序仍应判为应合并）",
    );

    // 三块连续、**逆序**书写：非相邻对（a@10 与 c@20 间隔 8）不得误判，相邻对（a-b、b-c）仍拒
    let three = one_station(
        "hvac",
        &regs_of(&format!(
            "{}\n{}\n{}",
            pts_block("c", 20, 2, 2),
            pts_block("b", 12, 2, 2),
            pts_block("a", 10, 2, 2)
        )),
    );
    assert_err_contains(&three, "应合并", "三块逆序书写：地址相邻对仍应被拒");
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

/// 规则 18（`pcs` 周期下界，设计补落点）：`499` → Err、`500` → Ok；
/// **上界有意不设**（与 meter_grid/battery 的 `< 5000` 不同）。
///
/// **Task 6（ADR-016）起落点迁移**：站级 `role: pcs` 已被 P-3 拒，周期下界随段迁入
/// `SouthPcsConfig::validate`（设计 §13.7/§13.8）—— 本条改用 `south_pcs` 段入口，
/// 边界值（499/500/30000）与断言文案逐条不变。
///
/// **"无上界"的当前真实口径（2026-09-26 质量评审 Minor #5 改写 —— 原理由"pcs 不参与控制
/// 决策"在迁移后**已失效**）**：本段 `interval_ms` 自 Task 6 起是**采集兼心跳**周期，
/// SOC 由这一拍产出并**参与 SOC 双源裁决**（设计 §13.4 / Δ-18），不再有独立心跳路径。
/// 故 `interval_ms ≥ 5000`（= `DATA_FRESHNESS_MS`）时，PCS 侧 SOC 会被
/// `AiIntegrator::DATA_STALE_AFTER`（同取 5s）**判过期**、长期由 **BMS 侧 SOC 兜底**
/// （`resolve_soc_source` 的优先级本就 BMS 优先、PCS 侧只作回落）。
/// **明知情而保留无上界**，两条理由：① 无上界是原 `intercore.modbus_rtu.heartbeat_poll_ms`
/// 的既有行为（该字段同样只有 0→1000ms 回退、无上界），本轮**迁移不引入新语义**；
/// ② 加下界/上界会改变既有配置的**可接受域** ⇒ 属**产品裁定**，非代码自选。
/// **兜底依赖登记**：BMS 侧 SOC 优先 + 5s 过期判定（二者都在 `ai_integration.rs`）。
#[test]
fn ac1_rule18_pcs_interval_lower_bound() {
    let mk = |iv: u64| {
        parse_pcs(&format!(
            "enabled: true\nport: /dev/ttyS7\nslave: 1\ninterval_ms: {iv}\nregs:\n  - {{ name: pcs_3zone, func: input, addr: 1000, count: 76, format: uint16, scale: 1.0, byte_swap: true }}\n"
        ))
    };
    let err = mk(499).validate().unwrap_err();
    assert!(
        err.contains("须 ≥ 500ms"),
        "pcs interval=499 应被拒，实际: {err}"
    );
    assert_eq!(mk(500).validate(), Ok(()), "pcs interval=500（下界）");
    assert_eq!(
        mk(30000).validate(),
        Ok(()),
        "pcs 无 <5000 上界（有意保留：原 intercore 行为；PCS 侧 SOC ≥5s 即过期、由 BMS 侧兜底，见上）"
    );
}

/// 规则 19（无 `points` 块的宽度护栏，设计补落点）：32 位块 `count` 非宽度整数倍 → Err；
/// `count: 4` → Ok；`discrete` 块不适用。
#[test]
fn ac1_rule19_width_guard_without_points() {
    assert_err_contains(
        &one_station(
            "hvac",
            &regs_of("- { name: z, addr: 10, count: 3, format: int32_scaled, scale: 1.0 }"),
        ),
        "整数倍",
        "32 位块 count=3（尾槽静默少产点）",
    );
    assert_ok(
        &one_station(
            "hvac",
            &regs_of("- { name: z, addr: 10, count: 4, format: int32_scaled, scale: 1.0 }"),
        ),
        "32 位块 count=4",
    );
    assert_ok(
        &one_station(
            "hvac",
            &regs_of("- { name: d, func: discrete, addr: 10, count: 31 }"),
        ),
        "discrete 块 count=31（位数无宽度概念，31 非 8 倍数亦合法）",
    );
    // 声明了 `points` 的块不适用本条：count=3 非 32 位宽度整数倍，但逐点声明后由
    // 规则 11 的首尾锚定 + 逐点宽度保证无"尾槽装不下"（此块：2 个 16 位点恰好覆盖 3 寄存器）
    assert_ok(
        &one_station("hvac", &regs_of("- { name: z, addr: 10, count: 3, format: int32_scaled, scale: 1.0, points: [{ at: 1, format: uint16, scale: 1.0 }, { at: 3, format: uint16, scale: 1.0 }] }")),
        "声明了 points 的块不适用规则 19",
    );
}
