//! 跨 crate 测试缝可见性守卫。
//!
//! **为什么需要这个文件**：`rs485-plugin` 的帧级交换缝（`set_test_exchange`）若用
//! `#[cfg(test)]` 门控，则**只对 rs485-plugin 自身的 test 目标**可见；本文件位于
//! `mupc-southd` 的 `tests/`（外部集成测试），那里 `rs485-plugin` 是普通依赖 ⇒
//! 那些 API 不存在、本文件**编译不过**。
//! 故缝改由 Cargo feature `test-seam` 门控，并由本 crate 的 dev-dependencies 开启。
//!
//! 本文件是一道**编译期护栏**：任何人把门控改回 `#[cfg(test)]`、或从
//! dev-dependencies 撤掉 `test-seam`，本文件立即编译失败。
//!
//! 生效条件：`cargo test -p mupc-southd`（或带 `--all-targets` 的 check/clippy）。
//! ⚠️ 裸 `cargo check -p mupc-southd` **不编译** `tests/*.rs` ⇒ 它兜不住本护栏。
//!
//! 依赖面刻意收窄：`device-trait` **不在** mupc-southd 的依赖里，故此处一律经
//! `rs485-plugin` 的 re-export 取类型（`Config` / `Parity` / `CrcMode` /
//! `handlers::ModbusHandler`），不新引 `device-trait`。
use rs485_plugin::{handlers::ModbusHandler, Config, CrcMode, Parity, Rs485Device};

fn dev() -> Rs485Device {
    Rs485Device::new(
        "seam_visibility".into(),
        "modbus".into(),
        Config {
            port: "SEAM-INPROCESS".into(), // 缝已设 ⇒ 不会被真正打开
            baud_rate: 19200,
            data_bits: 8,
            stop_bits: 1,
            parity: Parity::None,
            timeout_ms: 1000,
            device_addr: 1,
            crc_mode: CrcMode::Crc16Modbus,
            de_gpio: None,
            re_gpio: None,
        },
        std::sync::Arc::new(ModbusHandler::new(1, CrcMode::Crc16Modbus)),
    )
}

#[test]
fn rs485_exchange_seam_is_visible_from_downstream_crate_test() {
    let d = dev();
    // 缝：拿请求帧原文，回一个合法的 FC03 响应（slave=2, 1 寄存器 = 0x002A）
    d.set_test_exchange(std::sync::Arc::new(|req: &[u8]| {
        assert_eq!(req[0], 2, "缝必须拿到请求帧原文（首字节 = 目标从站）");
        let mut v = vec![2u8, 0x03, 0x02, 0x00, 0x2A];
        let crc =
            rs485_plugin::protocol::Frame::calculate_crc(2, 0x03, &v[2..], CrcMode::Crc16Modbus);
        v.push(crc as u8);
        v.push((crc >> 8) as u8);
        v
    }));
    assert_eq!(
        d.read_holding_registers_from(2, 0x0100, 1).unwrap(),
        vec![0x2A],
        "缝的响应必须被真实成帧/解析链正确解出"
    );
    // 清缝后应回落到"未打开 ⇒ NotConnected"（不改变既有语义）
    d.clear_test_exchange();
    assert!(d.read_holding_registers_from(2, 0x0100, 1).is_err());
}
