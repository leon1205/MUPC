//! 安全联锁状态机（纯逻辑，便于单测）。
//!
//! 输入 = 各 DI **去抖后有效态** + PCS run_state/在线；输出 = latch 决策 + DO1/DO2
//! 目标电平（active 高低电平由装配换算，本模块只输出逻辑电平）。
//!
//! 语义出处：核间通信设计文档 §12.3（DI/DO 安全联锁 BECG-3568）/ §12.4（`io:` 配置段）。
//!
//! - pcs_stop 源（急停 Estop / 水浸 Flood / 消防 Fire）**有效沿** → 触发 latch
//!   （返回 `Action::TriggerLatch`，装配负责 PCS 停机指令 + DB 持久化 restore）。
//! - 释放须：触发源全部复位 && 保持 >= `release_hold_secs`；`auto_release=false` 时还须外部
//!   `request_release`（`web_release_pending`），`auto_release=true` 则保持后自动释放——
//!   **但自动释放要求停机已确认（`!stop_failed`）**，stop() 未确认（PCS 仍运行）时只许人工
//!   web release 放行，防联锁静默失效。
//! - 门禁 Door 仅上报事件，不计入 pcs_stop 源集合（装配发事件，不参与 latch）。
//! - DO1 运行灯 = run_state∈{1,2,3} 且 !latch；DO2 故障灯 = 持久条件
//!   （latch || stop_failed || !pcs_online || M1 停机）。
//! - fail-safe：GPIO 读失败由装配按"触发态"处理（本模块只吃 bool 有效态 tripped）。
//!
//! 去抖：raw+active_low → tripped 换算及简单去抖由装配完成（`DiConf.debounce`），本机
//! 内部仅对 pcs_stop 源做**二次边沿判定**（`prev` 字段跨 tick 保持上拍生效集）。
//! 职责划分：本机 tick 直接更新 `InterlockState` 的源/安全记时字段，命令
//! （Trigger/Release）返回给装配执行 transport/DB 动作——状态与动作分离，测试只断言
//! `s` + `Action`。

/// 数字输入触发源。门禁仅产生事件，不计入 pcs_stop 源集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterlockSource {
    /// 急停（pcs_stop）
    Estop,
    /// 水浸（pcs_stop）
    Flood,
    /// 消防（pcs_stop）
    Fire,
    /// 门禁（event，仅上报，不触发 latch）
    Door,
}

/// 故障灯原因（诊断/事件上报用，DO2 输出目标另见 `tick` 返回值 fault_lamp）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultReason {
    /// 无故障
    None,
    /// 联锁触发（latch）或停机失败（stop_failed）
    Interlock,
    /// PCS 链路离线
    PcsOffline,
    /// M1：PCS 在线但 run_state 非 {1,2,3}（未 latch 时）
    M1Stopped,
}

/// tick 一次后需要装配执行的命令（transport/DB 动作）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 无需动作
    None,
    /// 触发联锁：装配发 PCS 停机指令 + DB 持久化 latch（restore true）
    TriggerLatch,
    /// 释放联锁：装配清 DB latch（restore false）等
    ReleaseLatch,
}

/// 共享联锁状态（跨 tick 持久，测试可直接构造/断言）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterlockState {
    /// 是否处于联锁（禁启）锁存态
    pub latched: bool,
    /// PCS 停机确认失败（由装配置位；停机确认成功由装配清除——本机 auto 释放要求 !stop_failed，
    /// 仅人工 web release 可放行 stop_failed 态；新触发沿复位重试）
    pub stop_failed: bool,
    /// 本拍仍处触发态的 pcs_stop 源位图（Estop=1 / Flood=2 / Fire=4）
    pub active_pcs_stop_sources: u32,
    /// 全部触发源复位后的 tick（秒）；复位前为 None
    pub source_safe_since: Option<u64>,
    /// 本拍故障灯原因（DO2 诊断）
    pub last_fault_lamp_reason: FaultReason,
}

impl Default for InterlockState {
    fn default() -> Self {
        Self {
            latched: false,
            stop_failed: false,
            active_pcs_stop_sources: 0,
            source_safe_since: None,
            last_fault_lamp_reason: FaultReason::None,
        }
    }
}

/// pcs_stop 源位图常量
const BIT_ESTOP: u32 = 1;
const BIT_FLOOD: u32 = 2;
const BIT_FIRE: u32 = 4;

/// 源 → 位图；门禁无位（不计入 pcs_stop 集合）。
fn source_bit(src: InterlockSource) -> Option<u32> {
    match src {
        InterlockSource::Estop => Some(BIT_ESTOP),
        InterlockSource::Flood => Some(BIT_FLOOD),
        InterlockSource::Fire => Some(BIT_FIRE),
        InterlockSource::Door => None,
    }
}

/// 安全联锁状态机（纯逻辑，可 `std::thread` 单测，不依赖 tokio/transport）。
pub struct StateMachine {
    /// 触发源回安全态并保持 ≥ release_hold_secs 后是否自动释放（false=须人工 web 确认）
    pub auto_release: bool,
    /// 触发源全复位后须保持的时长（秒），人工解除前置校验
    pub release_hold_secs: u64,
    /// 上拍 pcs_stop 生效源位图（二次边沿判定，跨 tick 保持）
    prev: u32,
}

impl StateMachine {
    pub fn new(auto_release: bool, release_hold_secs: u64) -> Self {
        Self {
            auto_release,
            release_hold_secs,
            prev: 0,
        }
    }

    /// tick 一次。
    ///
    /// 入参：
    /// - `s`：可变共享状态（本机直接记账 latch/源集/安全计时）
    /// - `tripped`：各源**去抖后有效态**（tripped=true 表示该源处触发态）
    /// - `web_release_pending`：人工 release 请求是否待处理（HTTP 置位，装配消费）
    /// - `run_state_ok`：PCS run_state ∈ {1,2,3}（装配由 last_run_state 换算）
    /// - `pcs_online`：PCS 链路在线
    /// - `now`：当前秒级时间
    ///
    /// 返回 `(Action, run_lamp, fault_lamp)`——DO1/DO2 两灯各自目标电平。
    pub fn tick(
        &mut self,
        s: &mut InterlockState,
        tripped: &[(InterlockSource, bool)],
        web_release_pending: bool,
        run_state_ok: bool,
        pcs_online: bool,
        now: u64,
    ) -> (Action, bool, bool) {
        // ── 1. pcs_stop 源本拍生效集 + 触发沿（门禁不入集）──
        let mut cur: u32 = 0;
        for (src, tr) in tripped {
            if let Some(b) = source_bit(*src) {
                if *tr {
                    cur |= b;
                }
            }
        }
        // 触发沿 = 本拍新增位（上拍 prev 未含）
        let new_bits = cur & !self.prev;
        self.prev = cur;
        s.active_pcs_stop_sources = cur;

        let mut action = Action::None;

        if !s.latched && new_bits != 0 {
            // pcs_stop 源首次有效（有效沿）→ 触发 latch；装配执行 PCS 停机 + DB 持久化
            s.latched = true;
            s.stop_failed = false;
            s.source_safe_since = None;
            action = Action::TriggerLatch;
        } else if s.latched {
            if cur == 0 {
                // 触发源全复位：记录安全起点
                if s.source_safe_since.is_none() {
                    s.source_safe_since = Some(now);
                }
                let held = now.saturating_sub(s.source_safe_since.unwrap_or(now))
                    >= self.release_hold_secs;
                // 释放门槛：人工 web 确认（操作员明确放行，可容忍 stop_failed 态），或
                // auto_release **且停机已确认**（!stop_failed）——stop() 从未确认（PCS 仍运行、
                // run_state 未转 0）时禁止自动解 latch 清 stop_failed：否则联锁静默失效
                // （源复位即自动复位，故障灯熄灭但 PCS 从未真正停机）。
                let manual_ok = web_release_pending;
                let auto_ok = self.auto_release && !s.stop_failed;
                if held && (manual_ok || auto_ok) {
                    s.latched = false;
                    s.stop_failed = false;
                    s.source_safe_since = None;
                    action = Action::ReleaseLatch;
                }
                // else：维持 latch（等待 hold 到达 / 人工确认 / 停机确认）
            } else {
                // 仍有触发源 active（含 latched 期间重新触发）→ 重置安全计时，保持 latch
                s.source_safe_since = None;
            }
        }
        // !s.latched 且 new_bits==0：无 pcs_stop 源新触发，无事可做（保持释放态）

        // ── 2. DO1/DO2 决策 ──
        // M1：在线但 run_state 非 {1,2,3} 且未 latch（离线不计 M1，因离线已由 PcsOffline 亮故障）
        let m1 = pcs_online && !run_state_ok && !s.latched;
        let fault = s.latched || s.stop_failed || !pcs_online || m1;

        s.last_fault_lamp_reason = if s.latched || s.stop_failed {
            FaultReason::Interlock
        } else if !pcs_online {
            FaultReason::PcsOffline
        } else if m1 {
            FaultReason::M1Stopped
        } else {
            FaultReason::None
        };

        let run_lamp = run_state_ok && !s.latched;
        let fault_lamp = fault;

        (action, run_lamp, fault_lamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const E: InterlockSource = InterlockSource::Estop;
    const FLOOD: InterlockSource = InterlockSource::Flood;
    const FIRE: InterlockSource = InterlockSource::Fire;
    const DOOR: InterlockSource = InterlockSource::Door;

    fn pcs_ok() -> (bool, bool) {
        (true, true) // (run_state_ok, pcs_online)
    }

    #[test]
    fn test_trigger_edge_latches_and_lights_fault() {
        let mut m = StateMachine::new(false, 5);
        let mut s = InterlockState::default();
        let (act, run_lamp, fault_lamp) =
            m.tick(&mut s, &[(E, true)], false, true, true, 1000);

        assert_eq!(act, Action::TriggerLatch);
        assert!(s.latched);
        assert_eq!(s.active_pcs_stop_sources, BIT_ESTOP);
        assert!(s.source_safe_since.is_none());
        assert!(!run_lamp);
        assert!(fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::Interlock);
    }

    #[test]
    fn test_continuous_trip_does_not_repeat_trigger() {
        let mut m = StateMachine::new(false, 5);
        let mut s = InterlockState::default();
        let (a1, _, _) = m.tick(&mut s, &[(E, true)], false, true, true, 1000);
        assert_eq!(a1, Action::TriggerLatch);

        // 持续 tripped：不再重复触发
        let (a2, _, fault_lamp) = m.tick(&mut s, &[(E, true)], false, true, true, 1001);
        assert_eq!(a2, Action::None);
        assert!(s.latched);
        assert!(fault_lamp);
    }

    #[test]
    fn test_auto_release_after_hold_and_lamp_recovers() {
        let mut m = StateMachine::new(true, 3);
        let mut s = InterlockState::default();
        let (run_ok, online) = pcs_ok();

        let (a0, _, _) = m.tick(&mut s, &[(E, true)], false, run_ok, online, 0);
        assert_eq!(a0, Action::TriggerLatch);

        // t=1 源复位 → 记录 safe_since=1，hold 未满
        let (a1, _, fl1) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 1);
        assert_eq!(a1, Action::None);
        assert!(s.latched);
        assert!(fl1);
        assert_eq!(s.source_safe_since, Some(1));

        // t=4 hold=3 满 + auto → 释放
        let (a2, run_lamp, fault_lamp) =
            m.tick(&mut s, &[(E, false)], false, run_ok, online, 4);
        assert_eq!(a2, Action::ReleaseLatch);
        assert!(!s.latched);
        assert!(s.source_safe_since.is_none());
        assert!(run_lamp);
        assert!(!fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::None);
    }

    #[test]
    fn test_auto_release_blocked_when_stop_unconfirmed() {
        // Important（质量评审）：stop() 从未确认（stop_failed=true，PCS 仍运行）时，即使
        // auto_release=true + 源复位 + hold 满，**也不得**自动解 latch 清 stop_failed——否则
        // 联锁静默失效（故障灯熄、run 灯亮，但 PCS 从未真正停机）。仅人工 web release 可放行。
        let mut m = StateMachine::new(true, 1);
        let mut s = InterlockState::default();
        let (run_ok, online) = pcs_ok();
        let (a0, _, _) = m.tick(&mut s, &[(E, true)], false, run_ok, online, 0);
        assert_eq!(a0, Action::TriggerLatch);
        s.stop_failed = true; // 装配：停机确认超时（PCS 未转 0）

        // t=1 源复位、t=3 hold=1 满 + auto → 仍维持 latch（禁止静默失效）
        let (a1, _, _) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 1);
        assert_eq!(a1, Action::None);
        let (a2, _, fault_lamp) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 3);
        assert_eq!(a2, Action::None);
        assert!(s.latched);
        assert!(s.stop_failed);
        assert!(fault_lamp);

        // 人工 web release 可放行 stop_failed 态（操作员明确确认）
        let (a3, run_lamp, _) = m.tick(&mut s, &[(E, false)], true, run_ok, online, 3);
        assert_eq!(a3, Action::ReleaseLatch);
        assert!(!s.latched);
        assert!(!s.stop_failed);
        assert!(run_lamp);
    }

    #[test]
    fn test_non_auto_without_web_request_stays_latched() {
        let mut m = StateMachine::new(false, 2);
        let mut s = InterlockState::default();
        let (run_ok, online) = pcs_ok();

        let (a0, _, _) = m.tick(&mut s, &[(FLOOD, true)], false, run_ok, online, 0);
        assert_eq!(a0, Action::TriggerLatch);

        let (a1, _, _) = m.tick(&mut s, &[(FLOOD, false)], false, run_ok, online, 1);
        assert_eq!(a1, Action::None);
        assert_eq!(s.source_safe_since, Some(1));

        // t=3 hold=2 满，但 auto=false 且无 web 请求 → 维持 latch
        let (a2, _, fault_lamp) =
            m.tick(&mut s, &[(FLOOD, false)], false, run_ok, online, 3);
        assert_eq!(a2, Action::None);
        assert!(s.latched);
        assert!(fault_lamp);
    }

    #[test]
    fn test_web_release_pending_allows_release() {
        let mut m = StateMachine::new(false, 2);
        let mut s = InterlockState::default();
        let (run_ok, online) = pcs_ok();

        let (a0, _, _) = m.tick(&mut s, &[(FIRE, true)], false, run_ok, online, 0);
        assert_eq!(a0, Action::TriggerLatch);
        let (a1, _, _) = m.tick(&mut s, &[(FIRE, false)], false, run_ok, online, 1);
        assert_eq!(a1, Action::None);

        // hold 满且 web_release_pending=true → 释放
        let (a2, _, fault_lamp) = m.tick(&mut s, &[(FIRE, false)], true, run_ok, online, 3);
        assert_eq!(a2, Action::ReleaseLatch);
        assert!(!s.latched);
        assert!(!fault_lamp);
    }

    #[test]
    fn test_m1_online_but_run_state_bad_lights_fault() {
        let mut m = StateMachine::new(true, 0);
        let mut s = InterlockState::default();
        // pcs_online=true, run_state_ok=false, 未 latch
        let (act, run_lamp, fault_lamp) =
            m.tick(&mut s, &[], false, false, true, 1000);

        assert_eq!(act, Action::None);
        assert!(!run_lamp);
        assert!(fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::M1Stopped);
    }

    #[test]
    fn test_run_ok_and_not_latched_lights_run_only() {
        let mut m = StateMachine::new(true, 0);
        let mut s = InterlockState::default();
        let (act, run_lamp, fault_lamp) =
            m.tick(&mut s, &[], false, true, true, 1000);

        assert_eq!(act, Action::None);
        assert!(run_lamp);
        assert!(!fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::None);
    }

    #[test]
    fn test_stop_failed_lights_fault() {
        let mut m = StateMachine::new(true, 0);
        let mut s = InterlockState::default();
        s.stop_failed = true; // 装配在停机确认超时后置位
        let (_, _, fault_lamp) = m.tick(&mut s, &[], false, true, true, 1000);

        assert!(fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::Interlock);
    }

    #[test]
    fn test_pcs_offline_lights_fault_reason_pcs_offline() {
        let mut m = StateMachine::new(true, 0);
        let mut s = InterlockState::default();
        let (_, _, fault_lamp) = m.tick(&mut s, &[], false, true, false, 1000);

        assert!(fault_lamp);
        assert_eq!(s.last_fault_lamp_reason, FaultReason::PcsOffline);
    }

    #[test]
    fn test_door_is_event_only_never_latches() {
        let mut m = StateMachine::new(false, 5);
        let mut s = InterlockState::default();
        // 门禁 tripped 不计入 pcs_stop 源集合 → 不触发 latch
        let (act, _, fault_lamp) =
            m.tick(&mut s, &[(DOOR, true)], false, true, true, 1000);

        assert_eq!(act, Action::None);
        assert!(!s.latched);
        assert_eq!(s.active_pcs_stop_sources, 0);
        assert!(!fault_lamp);
    }

    #[test]
    fn test_relatch_after_release_requires_new_edge() {
        let mut m = StateMachine::new(true, 1);
        let mut s = InterlockState::default();
        let (run_ok, online) = pcs_ok();

        // 触发 → 复位 hold 1s → 自动释放
        let (a0, _, _) = m.tick(&mut s, &[(E, true)], false, run_ok, online, 0);
        assert_eq!(a0, Action::TriggerLatch);
        let (a1, _, _) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 1);
        assert_eq!(a1, Action::None);
        let (a2, _, _) = m.tick(&mut s, &[(E, false)], false, run_ok, online, 2);
        assert_eq!(a2, Action::ReleaseLatch);
        assert!(!s.latched);

        // 释放后源再次 tripped → 新沿 → 重新触发
        let (a3, _, fl) = m.tick(&mut s, &[(E, true)], false, run_ok, online, 3);
        assert_eq!(a3, Action::TriggerLatch);
        assert!(s.latched);
        assert!(fl);
    }
}
