//! S3b-2 **AC-2（L2 总线注入层）** —— 端到端解码：**canned 寄存器 → 点位值**（设计 §11.11.1
//! L2 / §11.11.2 AC-2 / §11.6 的换算表）。
//!
//! **为什么必须有它**（与 L1 的分工）：L1（`meter_regs` 单测）只钉住"一个小算式"，
//! 而真实链路上还有三层会被漏掉的东西 —— ① 配置（块 `addr`/`format`/`scale`/`offset`/
//! `byte_swap`/`word_order`/点级覆盖）经 `points::expand` 折算成 `RegDecode` 的那一步；
//! ② 块内**偏移**（`at` 是 1 基、偏移是 0 基）与**绝对寄存器地址**的对齐；
//! ③ 块读结果的**形态**（FC03/FC04 → `BlockData::Regs`、FC02 → `BlockData::Bits`）。
//! 本文件把三者一起走通：**用参考配置的真实块划分**（`fixtures/south_stations_s3b2.yaml`
//! = PRD §9.4.1 的 6 站）经 [`MockBus`] 注入 canned 寄存器，再断言点位值 == §11.6 的期望值。
//!
//! **期望值的真源 = 设计 §11.6 的换算表**（全部已回厂方原文复算），逐设备抽样。其中两条是
//! **判别性**用例（"必须不等"）：BMS 116 `uint16` raw=65535 → **+4953.5 A**（若误配 `int16`
//! 得 −1600.1，二者必须不同）；PCS 32 位 `lo_hi` → **6563.6 kWh**（若误用 `hi_lo` 得 655360.1）。
//!
//! **本用例真能抓到的错**（举例）：`at` 抄成 0 基（整体错位一个寄存器 ⇒ 值变成邻点）；
//! 点级 `scale` 覆盖块级却写在了块级（值差 10 倍）；`byte_swap`/`word_order` 漏配或写反；
//! FC02 位块按 `Regs` 读（形态不符 ⇒ 点位全缺）；块 `addr` 改基准后**偏移仍能对齐**（若实现
//! 用"点绝对地址 == 块 addr + at"会红）。

use mupc_data_processing::meter_regs::WordOrder;
use mupc_southd::config::{RegFunc, Role, SouthStationsConfig, StationConf};
use mupc_southd::mapper::{self, BlockData, SampleKind, TelemetrySample};
use mupc_southd::port_runtime::{MockBus, StationBus};
use serde::Deserialize;

#[derive(Deserialize)]
struct Wrapper {
    south_stations: SouthStationsConfig,
}

/// PRD §9.4.1 的 6 站参考配置（与 `s3b2_config.rs` / `point_table_vs_reference_config.rs`
/// 同一份 fixture —— **同一份配置**才谈得上"配置 → 解码"的端到端）。
const REF: &str = include_str!("fixtures/south_stations_s3b2.yaml");

fn station(id: &str) -> StationConf {
    serde_yaml::from_str::<Wrapper>(REF)
        .expect("参考配置解析失败")
        .south_stations
        .stations
        .into_iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("参考配置里没有站 {id}"))
}

/// 某块的 `(块内偏移 0 基)` —— 由**绝对寄存器地址**反推（`at`/点位名都不可靠时用它定位）。
/// 仅本文件（测试）使用；生产侧不按硬编码地址取数（PRD G-5 禁设备特判）。
fn offset_of(conf: &StationConf, block: &str, abs_addr: u16) -> usize {
    let b = conf
        .regs
        .iter()
        .find(|b| b.name == block)
        .unwrap_or_else(|| panic!("站 {} 没有块 {block}", conf.id));
    assert!(
        abs_addr >= b.addr && abs_addr < b.addr + b.count,
        "寄存器 {abs_addr} 不在块 {block}（{:#06x}+{}）窗口内",
        b.addr,
        b.count
    );
    usize::from(abs_addr - b.addr)
}

/// 经 [`MockBus`] 把该站的**每个配置块**读回来（按 `func` 分发 FC03/FC04/FC02），
/// 构成 mapper 的 `BlockReads` —— 即"总线注入 → 解码"的端到端链路（设计 §11.11.1 L2）。
///
/// 测试须先把**每个块覆盖的地址**都用 `put`/`put_input`/`put_bits` 预置好（未预置 = 读 Err，
/// 正是"该块读失败"的真实形态）。
async fn read_station(bus: &MockBus, conf: &StationConf) -> mapper::BlockReads {
    let mut reads: mapper::BlockReads = Vec::new();
    for blk in &conf.regs {
        let res = match blk.func {
            RegFunc::Holding => bus
                .read_holding(conf.slave, blk.addr, blk.count)
                .await
                .map(BlockData::Regs),
            RegFunc::Input => bus
                .read_input(conf.slave, blk.addr, blk.count)
                .await
                .map(BlockData::Regs),
            RegFunc::Discrete => bus
                .read_discrete(conf.slave, blk.addr, blk.count)
                .await
                .map(BlockData::Bits),
        };
        reads.push((blk.clone(), res.map_err(|e| e.to_string())));
    }
    reads
}

/// 点位值（点不存在 → panic 并列出实际点位，便于定位"错位/漏产"）。
fn value_of(pts: &[TelemetrySample], metric: &str) -> f64 {
    pts.iter()
        .find(|p| p.metric == metric)
        .unwrap_or_else(|| {
            panic!(
                "没有点位 {metric}；实际点位：{:?}",
                pts.iter().map(|p| p.metric.as_str()).collect::<Vec<_>>()
            )
        })
        .value
}

/// 浮点比较（**缩放解算的固有误差**：如 `5123 × 0.1 = 512.3000000000001`，
/// 期望值取自 §11.6 的十进制真值 ⇒ 用 1e-9 容差而非逐位相等；整数解（raw 值）仍用 `assert_eq!`）。
fn assert_close(actual: f64, expect: f64, msg: &str) {
    assert!(
        (actual - expect).abs() < 1e-9,
        "{msg}: 实际 {actual}，期望 {expect}"
    );
}

/// 预置一个寄存器块的窗口（`regs[offset] = 值`，其余 0）——按**绝对地址**定位块的槽位。
fn window(conf: &StationConf, block: &str, entries: &[(u16, u16)]) -> Vec<u16> {
    let b = conf.regs.iter().find(|b| b.name == block).expect("块存在");
    let mut regs = vec![0u16; usize::from(b.count)];
    for (abs_addr, v) in entries {
        regs[offset_of(conf, block, *abs_addr)] = *v;
    }
    regs
}

/// 把该站的**所有**块预置为 0（按 `func` 选 `put`/`put_input`/`put_bits`）——用例随后覆盖
/// 关心的块。**未预置的块在 MockBus 上 = 读 Err**（正是"该块读失败"的真实形态，别拿它当 0）。
fn preload_zero(bus: &MockBus, conf: &StationConf) {
    for blk in &conf.regs {
        match blk.func {
            RegFunc::Holding => bus.put(conf.slave, blk.addr, vec![0u16; usize::from(blk.count)]),
            RegFunc::Input => {
                bus.put_input(conf.slave, blk.addr, vec![0u16; usize::from(blk.count)])
            }
            RegFunc::Discrete => {
                bus.put_bits(conf.slave, blk.addr, vec![false; usize::from(blk.count)])
            }
        }
    }
}

/// **BMS 站**（FC04）：§11.6 表的 4 条 —— `soc`（65 %）、簇组电压（512.3 V）、模块温度
/// （offset −40 ⇒ 25 ℃）、簇组电流（`uint16` + 负偏移的四组 raw：−1600.0/0.0/−100.0/+4953.5）。
#[tokio::test]
async fn ac2_bms_uint16_scale_and_negative_offset_decode_end_to_end() {
    let bms = station("bms");
    let bus = MockBus::new();
    // `bms_io`（addr 100，count 31）：一次注入本表全部测点
    let regs = window(
        &bms,
        "bms_io",
        &[
            (118, 0x0041), // soc：raw 65 → 65.0 %
            (115, 0x1403), // 簇组电压：raw 5123 × 0.1 → 512.3 V
            (117, 65),     // 模块温度：raw 65 × 1.0 − 40 → 25.0 ℃
            (116, 16000),  // 簇组电流：raw 16000 × 0.1 − 1600 → 0.0 A
        ],
    );
    preload_zero(&bus, &bms);
    bus.put_input(bms.slave, 100, regs);

    let reads = read_station(&bus, &bms).await;
    let pts = mapper::telemetry_points(Role::Battery, &reads);

    assert_close(value_of(&pts, "soc"), 65.0, "BMS 118 raw 0x0041 → 65.0 %");
    assert_close(
        value_of(&pts, "bms_io_16"),
        512.3,
        "BMS 115 raw 0x1403 → 512.3 V",
    );
    assert_close(
        value_of(&pts, "bms_io_18"),
        25.0,
        "BMS 117 raw 65 − 40 → 25.0 ℃",
    );
    assert_close(
        value_of(&pts, "bms_io_17"),
        0.0,
        "BMS 116 raw 16000 → 0.0 A",
    );

    // 同一 `soc` 点也经 mapper 的 Battery 分支进 pkg（控制链口径：域内 → Some）
    let pkg = match mapper::poll_to_result(Role::Battery, &reads) {
        mapper::PollResult::Data(p) => p,
        mapper::PollResult::Failed(e) => panic!("bms 应成功: {e}"),
    };
    assert_eq!(pkg.battery.soc, Some(65.0));
}

/// **BMS 116 的判别性取值**（§11.6 明文"必含"）：`uint16` + `offset: -1600`
/// ⇒ raw 0 → **−1600.0**、raw 15000 → **−100.0**、raw 65535 → **+4953.5**；
/// **与误配 `int16` 的 −1600.1 必须不同**（这正是 Q-20 现场判别的落点）。
#[tokio::test]
async fn ac2_bms_current_uint16_beats_int16_at_high_raw() {
    let bms = station("bms");
    for (raw, expect) in [(0u16, -1600.0), (15000, -100.0), (65535, 4953.5)] {
        let bus = MockBus::new();
        preload_zero(&bus, &bms);
        bus.put_input(bms.slave, 100, window(&bms, "bms_io", &[(116, raw)]));
        let reads = read_station(&bus, &bms).await;
        let pts = mapper::telemetry_points(Role::Battery, &reads);
        assert_close(
            value_of(&pts, "bms_io_17"),
            expect,
            &format!("BMS 116 raw={raw}（uint16 ×0.1 − 1600）"),
        );
        // int16 口径下的同 raw 值（= −1600.1）必须**不等**：证明该点确实按 uint16 解
        assert_ne!(
            value_of(&pts, "bms_io_17"),
            -1600.1,
            "raw=65535 若按 int16 解得 −1600.1 ⇒ 现场判别项 Q-20 的判据"
        );
    }
}

/// **PCS 站**（FC04 + `byte_swap`）：§11.6 表的 2 条 —— `soc` 转述 1010（注入 `0x4100`
/// → swap 后 65.0）与**32 位电量 1042–1043**（`int32_scaled`/`0.1`/`byte_swap`/`lo_hi`）。
#[tokio::test]
async fn ac2_pcs_byte_swap_and_32bit_lo_hi_decode_end_to_end() {
    let pcs = station("pcs");
    let bus = MockBus::new();
    // `pcs_3zone`（addr 1000，count 76，byte_swap: true）
    let regs = window(
        &pcs,
        "pcs_3zone",
        &[
            (1010, 0x4100), // BMS 系统 SOC 转述：swap(0x4100) = 0x0041 → 65.0 %
            (1042, 0x6400), // 交流累计充电电量低字
            (1043, 0x0100), // 同点高字（lo_hi）
        ],
    );
    bus.put_input(pcs.slave, 1000, regs);

    let reads = read_station(&bus, &pcs).await;
    let pts = mapper::telemetry_points(Role::Pcs, &reads);

    assert_close(
        value_of(&pts, "pcs_3zone_11"),
        65.0,
        "PCS 1010 注入 0x4100 → 逐寄存器 swap → 0x0041 → 65.0 %",
    );
    assert_close(
        value_of(&pts, "pcs_3zone_43"),
        6563.6,
        "PCS 1042–1043：swap 后 [0x0064, 0x0001] → lo_hi 拼 65636 × 0.1 → 6563.6 kWh",
    );
    assert_ne!(
        value_of(&pts, "pcs_3zone_43"),
        655_360.1,
        "若误用 hi_lo ⇒ 655360.1；两者必须不同（AC-2 的判别点）"
    );
    // 32 位点占 2 寄存器、产 **1** 点（64 位下 76 寄存器 − 4 个 32 位点 = 72 点）
    assert_eq!(
        pts.iter().filter(|p| p.kind == SampleKind::Scalar).count(),
        72,
        "PCS 3 区逐点展开 = 72 点（AC-6 ⑤）"
    );
}

/// **ADL400（`meter_batt`，FC03）**：§11.6 表的 2 条 —— A 相电流 0x0064（raw 946 → 9.46 A，
/// 点级 `scale: 0.01` 覆盖块级 0.1）与组合有功总电能 0x0000（`[0x0000, 0x3026]` → 123.26 kWh，
/// `hi_lo` 缺省）。**点级覆盖块级**这一条最容易写错（写反 ⇒ 差 10 倍）。
#[tokio::test]
async fn ac2_meter_batt_point_level_scale_override_and_total_energy() {
    let mb = station("meter_batt");
    let bus = MockBus::new();
    preload_zero(&bus, &mb);
    bus.put(mb.slave, 0x0000, vec![0x0000, 0x3026]); // 组合有功总电能（hi_lo）
    bus.put(mb.slave, 0x0061, window(&mb, "mb_ui", &[(0x0064, 946)]));

    let reads = read_station(&bus, &mb).await;
    let pts = mapper::telemetry_points(Role::MeterBatt, &reads);

    assert_close(
        value_of(&pts, "mb_e_act_comb_1"),
        123.26,
        "ADL400 组合有功总电能 [0x0000, 0x3026] → 123.26 kWh",
    );
    assert_close(
        value_of(&pts, "mb_ui_4"),
        9.46,
        "ADL400 A 相电流 raw 946 × 0.01（点级 scale 覆盖块级 0.1）→ 9.46 A",
    );
}

/// **消防站**（FC03）：§11.6 表的 2 条 —— 火警状态（addr 9）枚举原值 2；探测器数据 1
/// （addr 13）**整字** 0x4150 → 16720（G-6：本轮整字采集，字节拆解在展示层，§11.10）。
#[tokio::test]
async fn ac2_fire_enum_and_packed_word_are_raw_values() {
    let fire = station("fire");
    let bus = MockBus::new();
    // `fire_sys`（addr 4，count 13）：偏移 5 = addr 9 = 火警状态；偏移 9 = addr 13 = 数据 1
    let regs = window(&fire, "fire_sys", &[(9, 2), (13, 0x4150)]);
    bus.put(fire.slave, 4, regs);
    bus.put(fire.slave, 17, vec![0u16; 6 * 19]); // fire_det（n=20 ⇒ 114 寄存器 = 19 组）

    let reads = read_station(&bus, &fire).await;
    let pts = mapper::telemetry_points(Role::Fire, &reads);

    assert_eq!(
        value_of(&pts, "fire_sys_6"),
        2.0,
        "火警状态 = 2（二级火警，枚举语义非位）"
    );
    assert_eq!(
        value_of(&pts, "fire_sys_10"),
        16720.0,
        "探测器数据 1 落**整字**原值 0x4150 = 16720（展示层才拆 6.5 dB/M / 25 ℃）"
    );
}

/// **空调站**（FC04 整字 + FC02 位块）：§11.6 表的 2 条 —— 30001 `int16`/0.1 raw 258
/// → 25.8 ℃、30004 `uint16`/0.1 raw 602 → 60.2 %；位点 `hvac_di_1` 由首字节 `0x3B`
/// 解出位 0/1/3/4/5 = 1、位 2/6/7 = 0（bit0 = LSB）。
///
/// **同址不同地址空间**（v1.6 勘误的实证）：`hvac_in` 的寄存器 0/2/3 与 `hvac_di` 的位
/// 0/2/3 **真实同址**，二者由 `func` 分流（FC04 vs FC02）、点位名不同（`hvac_in_1` vs
/// `hvac_di_1`）—— 本用例同时钉住"两个空间互不串台"。
#[tokio::test]
async fn ac2_hvac_int16_uint16_and_fc02_bits() {
    let hvac = station("hvac");
    let bus = MockBus::new();
    bus.put_input(
        hvac.slave,
        0,
        window(&hvac, "hvac_in", &[(0, 258), (3, 602)]),
    );
    // 首字节 0x3B ⇒ 位 0/1/3/4/5 = 1、位 2/6/7 = 0（PRD §9.7.4 的原文示例）
    let first = 0x3Bu8;
    bus.put_bits(
        hvac.slave,
        0,
        (0..31).map(|k| (first >> (k % 8)) & 1 == 1).collect(),
    );

    let reads = read_station(&bus, &hvac).await;
    let pts = mapper::telemetry_points(Role::Hvac, &reads);

    assert_close(
        value_of(&pts, "hvac_in_1"),
        25.8,
        "30001 int16 raw 258 × 0.1 → 25.8 ℃",
    );
    assert_close(
        value_of(&pts, "hvac_in_4"),
        60.2,
        "30004 uint16 raw 602 × 0.1 → 60.2 %",
    );
    // 位空间：bit0 = LSB ⇒ 位 0/1/3/4/5 为 1
    for k_addr in [0usize, 1, 3, 4, 5] {
        assert_eq!(
            value_of(&pts, &format!("hvac_di_{}", k_addr + 1)),
            1.0,
            "位地址 {k_addr} 应为 1（0x3B 的 bit{k_addr}）"
        );
    }
    for k_addr in [2usize, 6, 7] {
        assert_eq!(
            value_of(&pts, &format!("hvac_di_{}", k_addr + 1)),
            0.0,
            "位地址 {k_addr} 应为 0（0x3B 的 bit{k_addr}）"
        );
    }
    assert_eq!(
        pts.iter().filter(|p| p.kind == SampleKind::Bit).count(),
        31,
        "31 位逐位产点（AC-6 ① 位块形态）"
    );
    // 两个地址空间的同名序号点**互不覆盖**：`hvac_in_1`（寄存器 0）与 `hvac_di_1`（位 0）
    assert_close(
        value_of(&pts, "hvac_in_1"),
        25.8,
        "同址不同空间：字符串键不同、取值各按自己的空间解",
    );
}

/// 参考配置的**每站**都经 FC 读通路能读到块（未预置即 Err）——保证上面的用例不是"只对某一站
/// 的块划分有效"；顺带钉住"6 站里除 `grid_meter` 外都有逐点产出"（grid 走分相语义，
/// 不经 `telemetry_points`，见 §11.4.6 的只读断言）。
#[tokio::test]
async fn ac2_reference_config_every_collected_station_produces_points() {
    let cfg: SouthStationsConfig = serde_yaml::from_str::<Wrapper>(REF).unwrap().south_stations;
    for s in &cfg.stations {
        let bus = MockBus::new();
        for blk in &s.regs {
            match blk.func {
                RegFunc::Discrete => {
                    bus.put_bits(s.slave, blk.addr, vec![false; usize::from(blk.count)]);
                }
                RegFunc::Holding => {
                    bus.put(s.slave, blk.addr, vec![0u16; usize::from(blk.count)]);
                }
                RegFunc::Input => {
                    bus.put_input(s.slave, blk.addr, vec![0u16; usize::from(blk.count)]);
                }
            }
        }
        let reads = read_station(&bus, s).await;
        assert!(
            reads.iter().all(|(_, r)| r.is_ok()),
            "站 {} 的块都应能经 MockBus 读回",
            s.id
        );
        if s.role == Role::MeterGrid {
            continue; // grid 走 poll_to_result(MeterGrid) 的分相语义（§11.5.3.4 C1 锚）
        }
        assert!(
            !mapper::telemetry_points(s.role, &reads).is_empty(),
            "站 {}（role={:?}）应产出点位",
            s.id,
            s.role
        );
    }
    // 静态断言：`WordOrder` 的缺省口径（`hi_lo`）由配置文件缺省继承而来（PCS 显式写 lo_hi）
    assert_eq!(WordOrder::default(), WordOrder::HiLo);
}
