//! 契约 v3 外设段集成断言（U-73 / 设计 §15.2.2 + §15.2.4；**只用公开 API + 字面量 JSON**）。
//!
//! 对应设计 §15.8.1 的 `display-proto` 行 **T-1 / T-2 / T-3 / T-4 / T-4b / T-6（key 部分）**：
//! - T-1：外设段 JSON 往返（含 `v=None` / 空段 / `renames` / `truncated`）、`Default` 落在
//!   `available=false`、未知键容忍（F26.6 回归）；
//! - T-2：版本协商（`version=2` 与 `4` 均拒）；既有版本用例见 `contract_v2.rs`；
//! - T-3：字段缺失 / `v=null` / `flag != valid` **绝不补 0**（EX-30）；
//! - T-4：帧尺寸（n=20 / n=100；典型 / 上界 / f64 极值三口径）；编解码同源同值；
//! - T-4b：守卫常量自洽（含 `k_max` 复算与「PRD 上限内不裁」的机械保证）；
//! - T-6：`PeripheralBlock::key` 位置式 + `renames` 覆盖。
//!
//! **不在本文件**（属后续增量，设计 §15.3 catalog / §15.5.2 白名单）：T-6 的
//! `decimals_from_scale` 四值覆盖、T-7 短标签表覆盖性。

use mupc_display_proto::*;

/// 设计 §15.2.2 的外设段字面量（含 `renames` / `v=null` / 站级三种 `cylinder_configured` /
/// 未知键）。
const PERIPH_SECTION_JSON: &str = r#"{
    "ts_ms": 1757412000000,
    "available": true,
    "catalog_rev": 7,
    "truncated": [],
    "future_section_key": 1,
    "stations": [
        {
            "id": "fire",
            "role": "fire",
            "online": true,
            "last_ok_ms": 1757411999000,
            "cylinder_configured": false,
            "future_station_key": "x",
            "blocks": [
                {
                    "name": "fire_det",
                    "ts_ms": 1757411999000,
                    "renames": [[7, "fire_det_count"]],
                    "future_block_key": [],
                    "values": [
                        {"at": 1, "v": null, "flag": "not_read"},
                        {"at": 7, "v": 3.0, "flag": "valid"}
                    ]
                }
            ]
        },
        {
            "id": "hvac",
            "role": "hvac",
            "online": false,
            "last_ok_ms": 0,
            "cylinder_configured": null,
            "blocks": [
                {
                    "name": "hvac_in",
                    "ts_ms": 0,
                    "renames": [],
                    "values": [
                        {"at": 1, "v": null, "flag": "offline"},
                        {"at": 3, "v": null, "flag": "range_error"}
                    ]
                }
            ]
        }
    ]
}"#;

fn minimal_frame_json(version: u8) -> String {
    format!(
        r#"{{"version":{version},"seq":1,"ts_ms":1,"soc":null,"soc_source":"lost",
        "soc_flag":"offline","run_state":null,"pcs_online":false,
        "p_phase":[{{"v":null,"flag":"not_read"}},{{"v":null,"flag":"not_read"}},{{"v":null,"flag":"not_read"}}],
        "p_total":{{"v":null,"flag":"not_read"}},
        "i_phase":[{{"v":null,"flag":"not_read"}},{{"v":null,"flag":"not_read"}},{{"v":null,"flag":"not_read"}}],
        "inconsistency":false,"peripherals":{PERIPH_SECTION_JSON}}}"#
    )
}

/// **真实旧帧形态**（v1 / v2）：**无 `peripherals` 段**——用于 §15.2.1 的拒帧与类型层缺省断言。
fn legacy_frame_json(version: u8) -> String {
    let with_section = minimal_frame_json(version);
    with_section.replace(&format!(",\"peripherals\":{PERIPH_SECTION_JSON}"), "")
}

// ───────────────────────── T-1：往返 / 缺省 / 未知键 ─────────────────────────

#[test]
fn t1_peripherals_section_wire_shape_roundtrips() {
    let sec: PeripheralsSection = serde_json::from_str(PERIPH_SECTION_JSON).unwrap();
    assert!(sec.available);
    assert_eq!(sec.ts_ms, 1_757_412_000_000);
    assert_eq!(sec.catalog_rev, 7);
    assert!(sec.truncated.is_empty(), "常态为空（PRD 上限内不裁）");
    assert_eq!(sec.stations.len(), 2, "顺序 = 站配置顺序（不跳位）");

    // 站级：role 字面量 / online / last_ok_ms（0 = 从未成功 ⇒ 屏显 `--`，不臆造时刻）
    let fire = &sec.stations[0];
    assert_eq!(fire.id, "fire");
    assert_eq!(fire.role, PeriphRole::Fire);
    assert!(fire.online);
    assert_eq!(fire.cylinder_configured, Some(false), "EDGE-23：未配置");
    let hvac = &sec.stations[1];
    assert_eq!(hvac.role, PeriphRole::Hvac);
    assert!(!hvac.online, "站离线（EDGE-18）");
    assert_eq!(hvac.last_ok_ms, 0, "从未成功 ⇒ 0（屏显 --，不臆造）");
    assert_eq!(hvac.cylinder_configured, None, "非消防站 ⇒ 不可得");

    // 点级：v=None 与四态；白名单内每一点都在（行数与 catalog 恒等）
    let det = &fire.blocks[0];
    assert_eq!(det.name, "fire_det");
    assert_eq!(det.values.len(), 2);
    assert_eq!(det.values[0].v, None);
    assert_eq!(det.values[0].flag, FieldFlag::NotRead);
    assert_eq!(det.values[1].v, Some(3.0));
    assert_eq!(det.values[1].flag, FieldFlag::Valid);
    let hvac_in = &hvac.blocks[0];
    assert_eq!(hvac_in.values[1].flag, FieldFlag::RangeError);
    // T-6：点键 = 唯一构造点（位置式 / renames 覆盖）
    assert_eq!(det.key(&det.values[0]), "fire_det_1");
    assert_eq!(det.key(&det.values[1]), "fire_det_count");
    assert_eq!(hvac_in.key(&hvac_in.values[0]), "hvac_in_1");

    // 再编码：字段名 / null 语义逐字保持（防实现侧改名）、未知键不回流
    let out = serde_json::to_string(&sec).unwrap();
    for needle in [
        "\"ts_ms\":1757412000000",
        "\"available\":true",
        "\"catalog_rev\":7",
        "\"truncated\":[]",
        "\"role\":\"fire\"",
        "\"last_ok_ms\":0",
        "\"cylinder_configured\":false",
        "\"renames\":[[7,\"fire_det_count\"]]",
        "{\"at\":1,\"v\":null,\"flag\":\"not_read\"}",
        "{\"at\":3,\"v\":null,\"flag\":\"range_error\"}",
    ] {
        assert!(out.contains(needle), "编码结果缺 `{needle}`：{out}");
    }
    // 未知键容忍（F26.6）≠ 未知键回流
    assert!(!out.contains("future_"), "未知键不得被回写：{out}");

    // 稳定往返
    let back: PeripheralsSection = serde_json::from_str(&out).unwrap();
    assert_eq!(back, sec);
}

#[test]
fn t1_default_section_is_unavailable_and_section_fields_are_individually_optional() {
    // `Default` 落在 `available=false`（EDGE-22）——绝不伪装成"空段正常"
    let d = PeripheralsSection::default();
    assert!(!d.available);
    assert_eq!(d.ts_ms, 0, "未采集 ⇒ 0（不臆造时刻）");
    assert_eq!(d.catalog_rev, 0);
    assert!(d.truncated.is_empty() && d.stations.is_empty());

    // 段级 `#[serde(default)]`：仅给部分键也能解出，缺的取缺省（与 v2 四段同范式）
    let partial: PeripheralsSection = serde_json::from_str(r#"{"available":true}"#).unwrap();
    assert!(partial.available);
    assert_eq!(partial.ts_ms, 0);
    assert!(partial.truncated.is_empty() && partial.stations.is_empty());

    // 段缺失（旧帧 / 未接线）⇒ 帧层取 Default ⇒ available=false
    let json = minimal_frame_json(PROTO_VERSION).replace(
        &format!("\"peripherals\":{PERIPH_SECTION_JSON}"),
        "\"peripherals\":{}",
    );
    let f: DisplayFrame = serde_json::from_str(&json).unwrap();
    assert!(!f.peripherals.available, "段缺失 ⇒ 「外设数据不可用」");
    assert!(f.check_version().is_ok());
}

// ───────────────────────── T-2：版本协商 ─────────────────────────

#[test]
fn t2_v3_pin_and_any_other_version_is_rejected() {
    assert_eq!(PROTO_VERSION, 3, "U-73 增量把版本单点改为 3（§15.2.1）");

    // 当前版本可解
    let ok = minimal_frame_json(PROTO_VERSION);
    let frame = DisplayFrame::from_json_slice(ok.as_bytes()).unwrap();
    assert_eq!(frame.version, 3);
    assert!(frame.peripherals.available);

    // 旧帧 v1 / v2（真实形态：**无外设段**）与未来 v4 一律拒帧，不得放宽
    for (version, expected_got) in [(2u8, 2u8), (1, 1)] {
        let json = legacy_frame_json(version);
        let err = DisplayFrame::from_json_slice(json.as_bytes()).unwrap_err();
        assert!(
            matches!(
                err,
                Error::ProtoVersionMismatch { got, expected }
                    if got == expected_got && expected == 3
            ),
            "version={version} 必须被拒绝，实际: {err:?}"
        );
    }
    let json = minimal_frame_json(4);
    let err = DisplayFrame::from_json_slice(json.as_bytes()).unwrap_err();
    assert!(
        matches!(err, Error::ProtoVersionMismatch { got: 4, expected: 3 }),
        "version=4 必须被拒绝，实际: {err:?}"
    );
}

// ───────────────────────── T-3：字段缺失 / 降级不补 0 ─────────────────────────

#[test]
fn t3_missing_or_degraded_values_are_never_filled_with_zero() {
    let sec: PeripheralsSection = serde_json::from_str(PERIPH_SECTION_JSON).unwrap();
    for station in &sec.stations {
        for block in &station.blocks {
            for pv in &block.values {
                if pv.flag != FieldFlag::Valid {
                    assert!(pv.v.is_none(), "flag != Valid ⇒ v 必须 None（EX-30）");
                }
            }
        }
    }
    let out = serde_json::to_string(&sec).unwrap();
    // 降级点必须是 null，绝不出现 `"v":0` / `"v":0.0`（不补 0、不沿用旧值）
    assert!(!out.contains("\"v\":0,"), "降级点不得补 0：{out}");
    assert!(!out.contains("\"v\":0.0,"), "降级点不得补 0：{out}");
    // 站级 `last_ok_ms = 0` 是"从未成功"的**真值**（屏显 --），与"补 0"是两回事
    assert!(out.contains("\"cylinder_configured\":null"));

    // 旧帧 v2（无外设段）：类型层取 Default，但**不是**"全 0 的有效外设数据"
    let v2: DisplayFrame = serde_json::from_str(&legacy_frame_json(2)).unwrap();
    assert!(!v2.peripherals.available);
    assert!(v2.peripherals.stations.is_empty(), "不得凭空造 0 值行");
    assert_eq!(v2.peripherals.ts_ms, 0);
    assert!(v2.check_version().is_err(), "类型层容忍 ≠ 版本校验放行");
}

// ───────────────────────── 合成帧（容量核算用）─────────────────────────

/// 白名单点数分布（设计 §15.2.4：非 `fire_det` 441 点与 n 无关；`fire_det` = 6×(n−1)）。
/// 块名取 §15.2.2 列出的真实块名；**块内拆分是合成**（§15.5.2 白名单逐条枚举属后续增量），
/// 容量结论只依赖点数。
const NON_FIRE_DET_BLOCKS: &[(&str, PeriphRole, usize)] = &[
    ("bms_io", PeriphRole::Battery, 17),
    ("bms_meta", PeriphRole::Battery, 8),
    ("bms_energy", PeriphRole::Battery, 9),
    ("bms_term", PeriphRole::Battery, 4),
    ("bms_cap", PeriphRole::Battery, 4),
    ("bms_alarm", PeriphRole::Battery, 288),
    ("mb_ui", PeriphRole::MeterBatt, 10),
    ("mb_freq_line", PeriphRole::MeterBatt, 4),
    ("mb_power", PeriphRole::MeterBatt, 8),
    ("mb_phase", PeriphRole::MeterBatt, 6),
    ("mb_e_act", PeriphRole::MeterBatt, 10),
    ("fire_sys", PeriphRole::Fire, 13),
    ("hvac_in", PeriphRole::Hvac, 3),
    ("hvac_di", PeriphRole::Hvac, 30),
    ("pcs_3zone", PeriphRole::Pcs, 27),
];

fn mk_block(name: &str, point_count: usize, v: f64, flag: FieldFlag) -> PeripheralBlock {
    PeripheralBlock {
        name: name.to_string(),
        ts_ms: 1_757_412_000_000,
        renames: vec![],
        values: (1..=point_count as u16)
            .map(|at| PointValue {
                at,
                v: Some(v),
                flag,
            })
            .collect(),
    }
}

/// 外设段合成帧：`n` = 每只探测器的寄存器数 ⇒ `fire_det` 点 = 6×(n−1)。
/// 既有段按设计基线的形态填满（10 条短告警 + 装置 / 信息 / 联锁段齐备）。
fn synth_frame(n: usize, v: f64, flag: FieldFlag) -> DisplayFrame {
    let mut by_role: Vec<(&str, PeriphRole, PeripheralBlock)> = NON_FIRE_DET_BLOCKS
        .iter()
        .map(|(name, role, cnt)| (*name, *role, mk_block(name, *cnt, v, flag)))
        .collect();
    let fire_det_points = 6 * (n - 1);
    let mk_station = |id: &str, role: PeriphRole, mut blocks: Vec<PeripheralBlock>| {
        blocks.sort_by(|a, b| a.name.cmp(&b.name));
        PeripheralStation {
            id: id.to_string(),
            role,
            online: true,
            last_ok_ms: 1_757_412_000_000,
            cylinder_configured: if role == PeriphRole::Fire {
                Some(true)
            } else {
                None
            },
            blocks,
        }
    };
    let take = |role: PeriphRole, by_role: &mut Vec<(&str, PeriphRole, PeripheralBlock)>| {
        let mut out = vec![];
        let mut i = 0;
        while i < by_role.len() {
            if by_role[i].1 == role {
                out.push(by_role.remove(i).2);
            } else {
                i += 1;
            }
        }
        out
    };
    let bms = mk_station("bms", PeriphRole::Battery, take(PeriphRole::Battery, &mut by_role));
    let mb = mk_station("meter_batt", PeriphRole::MeterBatt, take(PeriphRole::MeterBatt, &mut by_role));
    let mut fire_blocks = take(PeriphRole::Fire, &mut by_role);
    if fire_det_points > 0 {
        fire_blocks.push(mk_block("fire_det", fire_det_points, v, flag));
    }
    let fire = mk_station("fire", PeriphRole::Fire, fire_blocks);
    let hvac = mk_station("hvac", PeriphRole::Hvac, take(PeriphRole::Hvac, &mut by_role));
    let pcs = mk_station("pcs", PeriphRole::Pcs, take(PeriphRole::Pcs, &mut by_role));

    let ok_field = Field {
        v: Some(1.0),
        flag: FieldFlag::Valid,
    };
    DisplayFrame {
        version: PROTO_VERSION,
        seq: 42,
        ts_ms: 1_757_412_000_000,
        soc: Some(65.0),
        soc_source: SocSource::Bms,
        soc_flag: FieldFlag::Valid,
        run_state: Some(RunState::Charge),
        pcs_online: true,
        p_phase: [ok_field; 3],
        p_total: ok_field,
        i_phase: [ok_field; 3],
        inconsistency: false,
        device: DeviceSection {
            ts_ms: 1_757_412_000_000,
            uptime_secs: Some(90_000),
            cpu_temp_c: Some(48.4),
            mem_used_pct: Some(31.2),
            iec104: LinkState::Connected,
            intercore: LinkState::Connecting,
            hmi_channel: LinkState::Unknown,
            control_source: ControlSource::LocalStrategy,
        },
        alarms: AlarmsSection {
            ts_ms: 1_757_412_000_000,
            available: true,
            items: (0..10)
                .map(|i| AlarmItem {
                    ts_ms: 1_757_412_000_000 - i,
                    level: AlarmLevel::Warn,
                    message: format!("通道抖动 {i}"),
                })
                .collect(),
        },
        info: InfoSection {
            firmware_version: "0.1.0".to_string(),
            build_time: Some("2026-09-25T00:00:00Z".to_string()),
            model: Some("BECG-3568".to_string()),
            serial: Some("SN-0001".to_string()),
            service_scope: ServiceScope::LoopbackOnly,
            mgmt_ipv4: Some("192.168.3.118".to_string()),
        },
        interlock: InterlockSection {
            ts_ms: 1_757_412_000_000,
            available: true,
            enabled: true,
            latched: false,
            stop_failed: false,
            sources: vec![InterlockSourceItem {
                name: "estop".to_string(),
                tripped: false,
            }],
            fault_lamp: Some(false),
            run_lamp: Some(true),
            release_hold_secs: 30,
        },
        peripherals: PeripheralsSection {
            ts_ms: 1_757_412_000_000,
            available: true,
            catalog_rev: 7,
            truncated: vec![],
            stations: vec![bms, mb, fire, hvac, pcs],
        },
    }
}

fn peripherals_points(f: &DisplayFrame) -> usize {
    f.peripherals
        .stations
        .iter()
        .flat_map(|s| &s.blocks)
        .map(|b| b.values.len())
        .sum()
}

/// 既有 5 段 + v2 四段的 JSON 投影（**排除** `peripherals`），用于"既有段逐字段不变"断言。
fn existing_segments_projection(f: &DisplayFrame) -> serde_json::Value {
    let mut obj = serde_json::to_value(f).unwrap().as_object().unwrap().clone();
    obj.remove("peripherals");
    serde_json::Value::Object(obj)
}

// ───────────────────────── T-4：帧尺寸（三口径）─────────────────────────

#[test]
fn t4a_typical_encoding_n20_and_n100_stay_within_frame_limit() {
    // 典型口径：33 B/点（§15.2.4 表首行）
    let n20 = synth_frame(20, 1.0, FieldFlag::Valid);
    assert_eq!(peripherals_points(&n20), 555, "白名单 555 点（n=20）");
    let b20 = n20.to_json_slice().expect("n=20 必须可编码");
    let n100 = synth_frame(100, 1.0, FieldFlag::Valid);
    assert_eq!(peripherals_points(&n100), 1035, "白名单 1035 点（n=100）");
    let b100 = n100.to_json_slice().expect("n=100（PRD 上限）必须可编码且不触发裁剪");
    // 段级预算：典型口径下 段 ≤ 点数×33 + 固定开销（远在 56 KiB 之内 ⇒ 不触发裁剪）
    let sec_bytes = serde_json::to_vec(&n100.peripherals).unwrap().len();
    assert!(
        sec_bytes <= 1035 * POINT_JSON_BYTES_TYPICAL + PERIPH_FIXED_JSON_BYTES,
        "典型口径下外设段 {sec_bytes} B 超出 点数×33 + 4 KiB"
    );
    assert!(sec_bytes < MAX_PERIPH_BYTES);

    let pct20 = b20.len() * 100 / MAX_FRAME_BYTES;
    let pct100 = b100.len() * 100 / MAX_FRAME_BYTES;
    assert!(
        b100.len() <= MAX_FRAME_BYTES,
        "整帧 {} B 超上限（n=100 典型口径）",
        b100.len()
    );
    // 设计预测：n=20 ⇒ 22.2 KiB（35 %）；n=100 ⇒ 37.7 KiB（59 %）。留余量防回归（爆帧即失败）。
    assert!(pct20 <= 45, "n=20 实测占帧 {pct20} %，设计预测 35 %");
    assert!(pct100 <= 65, "n=100 实测占帧 {pct100} %，设计预测 59 %");
    assert!(pct100 > pct20, "点变多必须占帧变多");

    // 帧长随 n 线性（多出的 480 点 ≈ 30–33 B/点）：元数据**没有**入帧，否则差额会爆掉
    let diff = b100.len() - b20.len();
    assert!(
        (480 * 30..=480 * 34).contains(&diff),
        "n=20→100 的增量应为 480 点 × 约 31–33 B，实测 {diff} B"
    );
    // 解码侧同源同值
    let back = DisplayFrame::from_json_slice(&b100).expect("编码产物必须能过解码侧上限+版本校验");
    assert_eq!(back, n100);
}

#[test]
fn t4b_upper_encoding_n100_stays_within_peripherals_budget() {
    // 上界口径：48 B/点（`POINT_JSON_BYTES_UPPER`，守卫预检口径）
    let f = synth_frame(100, -214748364.7, FieldFlag::RangeError);
    let body = f
        .to_json_slice()
        .expect("上界口径下 n=100 也必须可编码（§15.2.4 结论 ②）");
    let sec_bytes = serde_json::to_vec(&f.peripherals).unwrap().len();
    // 守卫预检口径下的段长必须落在 56 KiB 之内 ⇒ **n=100 不触发裁剪**（机械保证，与
    // `K_MAX_FIRE_DET ≥ 99` 互为印证），且不超「点数×48 + 4 KiB」的预算式。
    assert!(
        sec_bytes <= 1035 * POINT_JSON_BYTES_UPPER + PERIPH_FIXED_JSON_BYTES,
        "上界口径下外设段 {sec_bytes} B 超出 点数×48 + 4 KiB"
    );
    assert!(
        sec_bytes <= MAX_PERIPH_BYTES,
        "上界口径下外设段 {sec_bytes} B 超 MAX_PERIPH_BYTES"
    );
    // 每点 ≥ 45 B（本合成帧 `at` 多为 1–3 位 ⇒ 可达形态 45–48 B），整帧仍在 64 KiB 内
    assert!(
        sec_bytes >= 1035 * 45,
        "上界口径下外设段 {sec_bytes} B 低于每点 45 B 的下界（口径失真）"
    );
    assert!(
        body.len() <= MAX_FRAME_BYTES,
        "整帧 {} B 超上限",
        body.len()
    );
    let pct = body.len() * 100 / MAX_FRAME_BYTES;
    assert!(pct <= 90, "n=100 上界口径实测占帧 {pct} %，设计预测 85 %");
    // 编 / 解码同源同值
    let back = DisplayFrame::from_json_slice(&body).unwrap();
    assert_eq!(back, f);
}

#[test]
fn t4c_f64_abs_max_form_triggers_exit_guard_and_frame_still_publishes() {
    // f64 理论极值形态（60 B/点，工程不可达）：即便在步骤 4 裁到 k_max(=111 只) 之后，
    // 整段仍是 (441 + 666) 点 × 60 B = 66,420 B ⇒ 触发步骤 5 的出口守卫（R-43 的失效模式）。
    let mut f = synth_frame(112, -1.7976931348623157e308, FieldFlag::RangeError);
    // fire 站 = `fire_sys` 13 + `fire_det` 6×111 = 666（n=112 ⇒ 111 只，已到 k_max 边界）
    assert_eq!(
        f.peripherals.stations[2]
            .blocks
            .iter()
            .map(|b| b.values.len())
            .sum::<usize>(),
        13 + 666,
        "n=112 ⇒ 111 只 ⇒ 已处于 k_max 边界（距 64 KiB 只剩理论口径）"
    );
    let before = existing_segments_projection(&f);
    let err = f.to_json_slice().unwrap_err();
    assert!(
        matches!(err, Error::FrameTooLarge { .. }),
        "极端形态必须先被既有唯一出口拒绝，实际: {err:?}"
    );

    // 出口守卫：既有段照发、不黑屏
    assert_eq!(enforce_exit_guard(&mut f), ExitGuardOutcome::PeripheralsDowngraded);
    assert!(
        !f.peripherals.available,
        "超预算 ⇒ 整段置不可用（不得静默发超限帧）"
    );
    assert!(
        f.peripherals.stations.is_empty() && f.peripherals.truncated.is_empty(),
        "载荷必须清空（仅翻 available 位不减字节，整帧仍会被拒）"
    );
    let body = f.to_json_slice().expect("降级后该帧必须照常发布");
    assert!(body.len() <= MAX_FRAME_BYTES);
    // 既有 5 段 + v2 四段**逐字段不变**（F26.7：新增不得拖垮既有）
    assert_eq!(existing_segments_projection(&f), before);
    // 对端可解且版本一致
    let back = DisplayFrame::from_json_slice(&body).unwrap();
    assert_eq!(back, f);
    // 再次调用是幂等的（已降级 ⇒ 不再改动，也不需要降级）
    assert_eq!(enforce_exit_guard(&mut f), ExitGuardOutcome::NoChange);
}

#[test]
fn t4d_budget_truncation_keeps_frame_publishable() {
    // n=120 ⇒ 119 只 > k_max=111 ⇒ 裁剪（§15.2.4「裁剪触发条件」：n ≥ 113）
    let mut f = synth_frame(120, -214748364.7, FieldFlag::RangeError);
    assert_eq!(peripherals_points(&f), 441 + 6 * 119);
    assert!(
        enforce_fire_det_budget(&mut f.peripherals),
        "119 只必须触发裁剪"
    );
    assert_eq!(f.peripherals.truncated, vec!["fire_det:119→111".to_string()]);
    assert_eq!(peripherals_points(&f), 441 + 666, "裁到前 111 只（前缀，地址升序）");
    assert!(f.peripherals.available, "裁剪 ≠ 段不可用（F21.4 显式提示而非静默）");
    let body = f.to_json_slice().expect("裁剪后必须 ≤ 帧上限");
    assert!(body.len() <= MAX_FRAME_BYTES);
    let back = DisplayFrame::from_json_slice(&body).unwrap();
    assert_eq!(back, f);

    // n=112（111 只 = k_max）⇒ **不裁**（边界上侧）
    let mut edge = synth_frame(112, 1.0, FieldFlag::Valid);
    assert!(!enforce_fire_det_budget(&mut edge.peripherals));
    assert!(edge.peripherals.truncated.is_empty());
}

// ───────────────────────── T-4b：常量自洽 ─────────────────────────

#[test]
fn t4b_guard_constants_are_self_consistent_and_recomputable() {
    // 派生关系（单一真源）
    assert_eq!(
        MAX_PERIPH_BYTES,
        MAX_FRAME_BYTES - EXISTING_SEGMENTS_RESERVE
    );
    assert_eq!(MAX_PERIPH_BYTES, 56 * 1024);
    assert_eq!(EXISTING_SEGMENTS_RESERVE, 8 * 1024);
    assert_eq!(
        POINT_JSON_BYTES_TYPICAL,
        33,
        "容量陈述口径（TYPICAL）"
    );
    assert_eq!(POINT_JSON_BYTES_UPPER, 48, "守卫预检口径（UPPER）");
    assert_eq!(
        POINT_JSON_BYTES_F64_ABS_MAX,
        60,
        "兜底说明口径（仅失效边界）"
    );
    let (typical, upper, abs_max) = (
        POINT_JSON_BYTES_TYPICAL,
        POINT_JSON_BYTES_UPPER,
        POINT_JSON_BYTES_F64_ABS_MAX,
    );
    assert!(typical <= upper, "TYPICAL ≤ UPPER（口径单调、不得互替）");
    assert!(upper <= abs_max, "UPPER ≤ F64_ABS_MAX");
    assert_eq!(PERIPH_FIXED_JSON_BYTES, 4 * 1024);
    assert_eq!(PERIPH_NON_FIRE_DET_POINTS, 441, "555 − 6×19（与 n 无关）");
    assert_eq!(PERIPH_NON_FIRE_DET_UPPER_BYTES, 441 * 48 + 4096);

    // k_max 独立复算（按 §15.2.4 算式，不引用常量自身）
    let recomputed = (56 * 1024 - (441 * 48 + 4 * 1024)) / (6 * 48);
    assert_eq!(recomputed, 111);
    assert_eq!(K_MAX_FIRE_DET, 111);
    // 「PRD 上限（n=100 ⇒ 99 只）不裁」的**机械保证**
    let k_max = K_MAX_FIRE_DET;
    assert!(k_max >= 99, "k_max({k_max}) 必须 ≥ 99 只");
    // 裁剪触发点：只数 = n−1 > k_max ⇒ n ≥ 113
    assert_eq!(FIRE_DET_TRUNCATE_MIN_N, 113);
    assert_eq!(fire_det_keep_units(99), 99, "n=100 ⇒ 99 只不裁（PRD 上限）");
    assert_eq!(fire_det_keep_units(110), 110, "n=111 ⇒ 110 只不裁");
    assert_eq!(fire_det_keep_units(111), 111, "n=112 ⇒ 111 只 = k_max，不裁");
    assert_eq!(fire_det_keep_units(112), 111, "n=113 ⇒ 112 只裁到 111（首个触发点）");
    assert_eq!(FIRE_DET_POINTS_PER_UNIT, 6);
    assert_eq!(FIRE_DET_BLOCK_NAME, "fire_det");
    assert_eq!(fire_det_truncated_note(119, 111), "fire_det:119→111");
}
