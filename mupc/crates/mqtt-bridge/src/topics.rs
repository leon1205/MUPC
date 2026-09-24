//! MQTT Topic 定义（01 设计 §9.3.4 函数式主题 / C-9）
//!
//! # 为什么北向遥测/事件改为**函数**（§9.3.4 / PRD §8.3.3）
//!
//! PRD §8.3.3 **强制按站分片**：`mupc/north/telemetry/{station_id}`、
//! `mupc/north/event/{station_id}`，且"**不得**把多站数据合并进一条消息"。
//! 旧的常量形态（`NORTH_TELEMETRY = "mupc/north/telemetry"`）**无法表达站 id** ⇒
//! 任何调用方都只能发到不分片的顶层主题（现状缺陷：物联平台无法按站路由）。
//! 常量无法参数化，故本文件把这两个主题**函数化**；`NORTH_STATUS` / `NORTH_FAULT`
//! 不带站维度，**保持常量不变**（§9.3.4 表：本行表为准）。
//!
//! # 兼容性（C-9）
//!
//! 旧常量 `NORTH_TELEMETRY` **保留**（`#[deprecated]`，不删名以免大规模改名），
//! 但它**不得**再被生产路径使用——分片要求下它只是"订阅通配 `mupc/north/telemetry/#`
//! 仍能收到全部站"这一兼容性要求的文本锚点。
//!
//! 订阅方兼容性：订阅 `mupc/north/telemetry/#` 仍可收到全部站（PRD §8.3.3 强制）；
//! **每条消息只含一个站**。

/// 北向分片遥测主题（§9.3.4）：`mupc/north/telemetry/{station_id}`，QoS1。
///
/// `station_id` = 02 PRD §9.3.3 的站 `id`
/// （`grid_meter` / `bms` / `pcs` / `meter_batt` / `fire` / `hvac`）。
pub fn north_telemetry(station_id: &str) -> String {
    format!("mupc/north/telemetry/{station_id}")
}

/// 北向分片事件主题（§9.3.4）：`mupc/north/event/{station_id}`，QoS1（故障类 2）。
///
/// 事件类：变位 / 站 offline/online / SOC 越界 / 消防登记数不一致。
pub fn north_event(station_id: &str) -> String {
    format!("mupc/north/event/{station_id}")
}

/// 北向装置状态（`mupc/north/status`，QoS0，双向）——**不变**（§9.3.4 表）。
pub const NORTH_STATUS: &str = "mupc/north/status";

/// 北向故障事件（`mupc/north/fault`，QoS2）——**不变**（§9.3.4 表）。
pub const NORTH_FAULT: &str = "mupc/north/fault";

/// 北向遥测**顶层**主题（**已废弃**，§9.3.4 / C-9）。
///
/// ⚠️ 它不是任何消息的发布目标（发布一律走 [`north_telemetry`] 的分片形态）；
/// 保留仅为兼容既有引用与"通配订阅仍可收到全部站"的文本锚点。
#[deprecated(
    note = "§9.3.4：北向遥测改为按站分片，请用 north_telemetry(station_id)；本常量不得用于发布"
)]
pub const NORTH_TELEMETRY: &str = "mupc/north/telemetry";

/// 北向策略指令主题（本增量不改，保留常量）。
pub const NORTH_STRATEGY_COMMAND: &str = "mupc/north/strategy/command";

// 本地 mosquitto Topic (进程间通信)
/// 本地遥测（进程间）。
pub const LOCAL_TELEMETRY: &str = "mupc/local/telemetry";
/// 本地策略指令（进程间）。
pub const LOCAL_STRATEGY_COMMAND: &str = "mupc/local/strategy/command";
/// 本地 AI 就绪信号（进程间）。
pub const LOCAL_AI_READY: &str = "mupc/local/ai/ready";

#[cfg(test)]
mod tests {
    use super::*;

    /// §9.3.4 主题**逐字**：分片主题带站 id，状态/故障主题不变。
    #[test]
    fn sharded_topics_are_verbatim() {
        assert_eq!(
            north_telemetry("bms"),
            "mupc/north/telemetry/bms",
            "§9.3.4：north_telemetry(id) = mupc/north/telemetry/{{station_id}}"
        );
        assert_eq!(
            north_event("bms"),
            "mupc/north/event/bms",
            "§9.3.4：north_event(id) = mupc/north/event/{{station_id}}"
        );
        assert_eq!(NORTH_STATUS, "mupc/north/status");
        assert_eq!(NORTH_FAULT, "mupc/north/fault");
        // 六个站 id 都可用（PRD §8.3.3 的 station_id 取值域）
        for s in [
            "grid_meter",
            "bms",
            "pcs",
            "meter_batt",
            "fire",
            "hvac",
        ] {
            assert_eq!(north_telemetry(s), format!("mupc/north/telemetry/{s}"));
        }
    }

    /// 通配订阅兼容性：任一分片主题都以 `mupc/north/telemetry/` 为前缀
    /// ⇒ 订阅 `mupc/north/telemetry/#` 仍能收到全部站（PRD §8.3.3 强制）。
    #[test]
    fn wildcard_subscription_still_covers_all_stations() {
        for s in ["grid_meter", "bms", "pcs", "meter_batt", "fire", "hvac"] {
            assert!(
                north_telemetry(s).starts_with("mupc/north/telemetry/"),
                "分片主题必须落在 mupc/north/telemetry/# 之下"
            );
            assert!(
                north_event(s).starts_with("mupc/north/event/"),
                "事件主题必须落在 mupc/north/event/# 之下"
            );
        }
    }
}
