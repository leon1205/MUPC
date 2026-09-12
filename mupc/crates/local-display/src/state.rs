//! 帧状态模型 + 三态归一（纯逻辑，可单测）。
//!
//! 对齐 `[DESIGN_APPROVED]` 设计 §3.4/§5.3 与 UI §7：
//! - 通道态派生：`ChannelInit`（未首成功 GET）/ `Connected` / `ChannelDown`（无成功 >3s → 整屏态）。
//! - 新鲜度：`now − frame.ts_ms > stale_ms` → `Stale`（打「数据过期」标，保留最近值，不冒充实时）。
//! - 点级三态归一：`NumView::Value(v)` ↔ 正常展示；`NumView::Dash(flag)` ↔ 该字段显 `--`+对应角标
//!   （禁 0/陈旧值，PRD 不造假值）。SOC 双源皆失 → `SocView::Lost`（禁沿用旧值）。
//!
//! 本文件不含任何绘图/IO——`UiSnapshot` 为渲染层（layout）唯一消费的归一化视图。

use mupc_display_proto::{DisplayFrame, Field, FieldFlag, RunState, SocSource};

/// 通道断判定阈值：无成功 GET 超过该时长 → 切「与主进程数据通道断开」整屏态（UI §7.5 / PRD 6.3）。
pub const CHANNEL_DOWN_MS: u64 = 3000;

/// 通道状态（设计 §5.3 四种态里与主进程连通性相关的三种；新鲜度另由 [`Freshness`] 表达）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelStatus {
    /// 尚未首次成功 GET（渲染可先于 mupcd 启动 → 显示「初始化中」）。
    Init,
    /// 最近一次成功 GET 距今 ≤ 通道断阈值，链路通。
    Connected,
    /// 无成功 GET > `CHANNEL_DOWN_MS` → 整屏「与主进程数据通道断开」。
    Down,
}

/// 单帧数据新鲜度（PRD F5.3：now − ts_ms > stale_ms → 过期）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Stale,
}

/// 整屏展示模式（layout 据此选覆盖层文案/灰化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenMode {
    /// 通道断（>3s）→ 中央「与主进程数据通道断开」覆盖，底层冻结。
    ChannelDown,
    /// 尚未首成功 → 中央「正在连接数据通道…」。
    Init,
    /// 有实时/过期数据帧正常展示（过期仅在字段加「数据过期」标，非整屏覆盖）。
    Live,
}

/// 数值字段三态归一：正常展示值 / 该字段显 `--` + 角标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumView {
    Value(f64),
    /// 点级降级：`--` + [`FieldFlag`] 对应角标（NotRead=未取数 / Offline=源离线 / RangeError=数据异常）。
    Dash(FieldFlag),
}

impl NumView {
    /// 由帧字段归一（设计：`flag == Valid` 且 `v` 有值 → 正常；否则 Dash）。
    pub fn from_field(f: &Field) -> Self {
        if f.flag == FieldFlag::Valid {
            match f.v {
                Some(v) => NumView::Value(v),
                // Valid 但无值 = 生产方异常；按数据异常降级，绝不补 0。
                None => NumView::Dash(FieldFlag::RangeError),
            }
        } else {
            NumView::Dash(f.flag)
        }
    }

    /// 是否降级（Dash）。
    pub fn is_degraded(&self) -> bool {
        matches!(self, NumView::Dash(_))
    }

    pub fn value(&self) -> Option<f64> {
        match self {
            NumView::Value(v) => Some(*v),
            NumView::Dash(_) => None,
        }
    }
}

/// 角标文案集中定义（与 font.rs 码表 / UI §6.6 同步）。
pub fn dash_badge(flag: FieldFlag) -> &'static str {
    match flag {
        FieldFlag::NotRead => "未取数",
        FieldFlag::Offline => "源离线",
        FieldFlag::RangeError => "数据异常",
        FieldFlag::Valid => "",
    }
}

/// SOC 主区三态（UI §7.2）：有值正常展示 / 双源皆失 → `--` + 「SOC 源失效」，禁沿用旧值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SocView {
    Value(f64),
    Lost,
}

impl SocView {
    pub fn value(&self) -> Option<f64> {
        match self {
            SocView::Value(v) => Some(*v),
            SocView::Lost => None,
        }
    }
}

/// SOC 展示警示档（PRD F1.3：≤15 红 / 15–85 青 / ≥85 橙）——仅驱动 UI 断点，非控制硬限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocBand {
    Low,
    Mid,
    High,
}

/// SOC 展示档**下限**阈值（%）：`≤` 此值 → [`SocBand::Low`]（PRD F1.3；UI §6.1 量程条 0–15 % 红段）。
///
/// **单一真源**（B2a 规格评审 Minor ⑤）：P1 的量程条分段几何（`ui/pages/p1_status.rs`）曾另抄一份
/// `SOC_LOW_PCT = 15`，与本文件 `soc_band` 的阈值字面量构成**双份真源**。阈值统一收在此处，
/// 两处共用 —— 改阈值只改这里。
pub const SOC_LOW_PCT: i32 = 15;
/// SOC 展示档**上限**阈值（%）：`≥` 此值 → [`SocBand::High`]（PRD F1.3；UI §6.1 量程条 85–100 % 橙段）。
pub const SOC_HIGH_PCT: i32 = 85;

/// SOC 值 → 展示档。SOC 值超出 0..100 亦 clamp 到端点档（生产方已域值化，此处兜底）。
///
/// 阈值取自 [`SOC_LOW_PCT`] / [`SOC_HIGH_PCT`]（单一真源，见其文档）。
pub fn soc_band(v: f64) -> SocBand {
    if v <= SOC_LOW_PCT as f64 {
        SocBand::Low
    } else if v >= SOC_HIGH_PCT as f64 {
        SocBand::High
    } else {
        SocBand::Mid
    }
}

/// 逐字段/逐区新鲜度点语义（UI §5.3 状态点：● 实时 / ○ 停更 / 琥珀 过期）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveDot {
    /// 字段有效且帧新鲜 → 实心青 ●
    Live,
    /// 字段降级（未取数/离线/异常）或通道不实时 → 空心 ○
    Paused,
    /// 帧过期（值保留）→ 琥珀点。
    Expired,
}

/// 由「字段归一值 + 整帧新鲜度」推导该字段状态点（渲染侧三态语法，纯逻辑）。
pub fn live_dot_for(nv: &NumView, fresh: Freshness) -> LiveDot {
    match nv {
        NumView::Value(_) if fresh == Freshness::Fresh => LiveDot::Live,
        NumView::Value(_) => LiveDot::Expired,
        NumView::Dash(_) => LiveDot::Paused,
    }
}

// ---------------------------------------------------------------------------
// 显示状态容器（DisplayState）：由 run 主循环喂「拉帧结果 + 时钟」，向外派生通道态/新鲜度
// ---------------------------------------------------------------------------

/// 渲染进程显示状态（单线程使用，非 Sync）。
#[derive(Debug)]
pub struct DisplayState {
    frame: Option<DisplayFrame>,
    /// 最近一次成功 GET 的单调时钟 ms（虚拟时钟：run 注入 `now_ms`）。
    last_ok_ms: Option<u64>,
    /// 首次尝试拉帧时刻（用于 Init → Down 的超时判定）。
    first_attempt_ms: Option<u64>,
    /// 连续失败计数（诊断/日志；本身不直接驱动通道态——驱动靠 last_ok 时间）。
    fail_streak: u32,
    /// 乱序（回退）丢弃计数（W3：设计 §3.3「渲染端判连续/重排」——丢旧帧不做展示回退）。
    reorder_dropped: u64,
    /// 过期阈值（设计：默认取 display-proto `DEFAULT_STALE_MS=2000`，可 `--stale-ms` 覆盖）。
    stale_ms: u64,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayState {
    pub fn new() -> Self {
        Self {
            frame: None,
            last_ok_ms: None,
            first_attempt_ms: None,
            fail_streak: 0,
            reorder_dropped: 0,
            stale_ms: mupc_display_proto::DEFAULT_STALE_MS,
        }
    }

    pub fn set_stale_ms(&mut self, ms: u64) {
        self.stale_ms = ms;
    }

    pub fn stale_ms(&self) -> u64 {
        self.stale_ms
    }

    /// 记录一次拉帧成功（更新最新帧 + 成功时刻）。
    ///
    /// **乱序保护（W3，设计 §3.3「渲染端判连续/重排」）**：`seq` 与 `ts_ms` **双双**回退
    /// （即确定更旧的帧）时丢弃，不做展示回退——但通道仍记成功（链路是通的）。用双条件而非
    /// 仅比 `seq`：mupcd 重启后 `seq` 清零（设计 §3.3），此时 `ts_ms` 更新，须接受新发布序号
    /// 周期的帧；仅比 `seq` 会让屏面冻结至 `seq` 追平旧值（最坏数十小时）。
    pub fn record_success(&mut self, frame: DisplayFrame, now_ms: u64) {
        self.first_attempt_ms.get_or_insert(now_ms);
        self.last_ok_ms = Some(now_ms);
        self.fail_streak = 0;
        if let Some(prev) = &self.frame {
            if frame.seq < prev.seq && frame.ts_ms <= prev.ts_ms {
                self.reorder_dropped = self.reorder_dropped.saturating_add(1);
                return; // 旧帧/乱序：保留较新帧，不把屏面回退到旧值
            }
        }
        self.frame = Some(frame);
    }

    /// 记录一次拉帧失败（不丢帧——保留最近有效帧供冻结展示，设计 §6.3）。
    pub fn record_fail(&mut self, now_ms: u64) {
        self.first_attempt_ms.get_or_insert(now_ms);
        self.fail_streak = self.fail_streak.saturating_add(1);
    }

    /// 通用入口：`Ok(frame)` → success，`Err` → fail。
    pub fn update(&mut self, res: Result<DisplayFrame, crate::Error>, now_ms: u64) {
        match res {
            Ok(f) => self.record_success(f, now_ms),
            Err(_) => self.record_fail(now_ms),
        }
    }

    pub fn frame(&self) -> Option<&DisplayFrame> {
        self.frame.as_ref()
    }

    pub fn last_ok_ms(&self) -> Option<u64> {
        self.last_ok_ms
    }

    pub fn fail_streak(&self) -> u32 {
        self.fail_streak
    }

    /// 乱序（seq+ts 双双回退）被丢弃的帧数（诊断用；W3）。
    pub fn reorder_dropped(&self) -> u64 {
        self.reorder_dropped
    }

    /// 通道态派生（纯逻辑）。`now_ms` 为注入时钟。
    pub fn channel_status(&self, now_ms: u64) -> ChannelStatus {
        match self.last_ok_ms {
            Some(t) if now_ms.saturating_sub(t) < CHANNEL_DOWN_MS => ChannelStatus::Connected,
            Some(_) => ChannelStatus::Down,
            None => match self.first_attempt_ms {
                None => ChannelStatus::Init,
                Some(t) if now_ms.saturating_sub(t) >= CHANNEL_DOWN_MS => ChannelStatus::Down,
                Some(_) => ChannelStatus::Init,
            },
        }
    }

    /// 有帧时的单帧新鲜度（无帧 → Fresh 占位，layout 用 ScreenMode=Init 不会消费数值）。
    pub fn freshness(&self, now_ms: u64) -> Freshness {
        match &self.frame {
            Some(f) if now_ms.saturating_sub(f.ts_ms) > self.stale_ms => Freshness::Stale,
            _ => Freshness::Fresh,
        }
    }

    /// 整屏展示模式（layout 据此选覆盖层/灰化）。
    pub fn screen_mode(&self, now_ms: u64) -> ScreenMode {
        match self.channel_status(now_ms) {
            ChannelStatus::Down => ScreenMode::ChannelDown,
            ChannelStatus::Init => ScreenMode::Init,
            ChannelStatus::Connected => ScreenMode::Live,
        }
    }

    /// 渲染层唯一消费的归一化视图快照。
    pub fn snapshot(&self, now_ms: u64) -> UiSnapshot {
        let f = self.frame.as_ref();
        let mode = self.screen_mode(now_ms);
        let fresh = self.freshness(now_ms);
        UiSnapshot {
            mode,
            fresh,
            soc: match f {
                Some(f) if f.soc.is_some() && f.soc_source != SocSource::Lost => {
                    SocView::Value(f.soc.unwrap())
                }
                _ => SocView::Lost,
            },
            soc_source: f.map(|f| f.soc_source).unwrap_or(SocSource::Lost),
            run_state: f.and_then(|f| f.run_state),
            pcs_online: f.map(|f| f.pcs_online).unwrap_or(false),
            inconsistency: f.map(|f| f.inconsistency).unwrap_or(false),
            p_phase: f.map(|f| norm3(&f.p_phase)).unwrap_or(DASH3),
            p_total: f
                .map(|f| NumView::from_field(&f.p_total))
                .unwrap_or(NumView::Dash(FieldFlag::NotRead)),
            i_phase: f.map(|f| norm3(&f.i_phase)).unwrap_or(DASH3),
            clock_text: String::new(),
        }
    }
}

/// 无帧/字段缺失时的三相占位：全 `--` + 「未取数」角标。
const DASH3: [NumView; 3] = [NumView::Dash(FieldFlag::NotRead); 3];

/// `[Field; 3]` → `[NumView; 3]` 逐点归一（借位遍历，避免 `[Field;3]` 非 Copy 的移动）。
fn norm3(a: &[Field; 3]) -> [NumView; 3] {
    [
        NumView::from_field(&a[0]),
        NumView::from_field(&a[1]),
        NumView::from_field(&a[2]),
    ]
}

/// 渲染视图快照：layout 只依赖本结构，与底层帧/通道解耦（KISS + 可测）。
#[derive(Debug, Clone)]
pub struct UiSnapshot {
    pub mode: ScreenMode,
    pub fresh: Freshness,
    /// SOC 数值（Lost = 双源皆失 → `--` + 源失效警示）。
    pub soc: SocView,
    pub soc_source: SocSource,
    /// PCS 状态（None = 离线/无有效态 → 「PCS 离线」）。
    pub run_state: Option<RunState>,
    pub pcs_online: bool,
    pub inconsistency: bool,
    /// 三相有功（已 ×0.1 kW）。
    pub p_phase: [NumView; 3],
    pub p_total: NumView,
    /// 三相电流（已 ×0.1 A）。
    pub i_phase: [NumView; 3],
    /// 页眉时钟文本（ASCII HH:MM:SS，渲染端 run/bin 层负责生成；空则 layout 不画）。
    pub clock_text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use mupc_display_proto::{DisplayFrame, Field};

    fn frame(seq: u64, ts_ms: u64) -> DisplayFrame {
        DisplayFrame {
            version: mupc_display_proto::PROTO_VERSION,
            seq,
            ts_ms,
            soc: Some(65.0),
            soc_source: SocSource::Bms,
            soc_flag: FieldFlag::Valid,
            run_state: Some(RunState::Charge),
            pcs_online: true,
            p_phase: [Field { v: Some(12.3), flag: FieldFlag::Valid }; 3],
            p_total: Field { v: Some(36.1), flag: FieldFlag::Valid },
            i_phase: [Field { v: Some(22.5), flag: FieldFlag::Valid }; 3],
            inconsistency: false,
            // v2 契约新增的四段：本测试桩只关心 v1 字段，四段一律取契约缺省
            // （`DeviceSection` 等均 `#[serde(default)]` + `Default`，语义 = 「未提供」）。
            device: Default::default(),
            alarms: Default::default(),
            info: Default::default(),
            interlock: Default::default(),
        }
    }

    // ---- 三态归一 ----
    #[test]
    fn num_view_valid_value() {
        let f = Field { v: Some(12.3), flag: FieldFlag::Valid };
        assert_eq!(NumView::from_field(&f), NumView::Value(12.3));
    }

    #[test]
    fn num_view_degraded_flags_map_to_dash() {
        for flag in [
            FieldFlag::NotRead,
            FieldFlag::Offline,
            FieldFlag::RangeError,
        ] {
            let f = Field { v: None, flag };
            assert_eq!(NumView::from_field(&f), NumView::Dash(flag));
        }
        // Valid 但无值 → RangeError 兜底（不补 0）
        let f = Field { v: None, flag: FieldFlag::Valid };
        assert_eq!(NumView::from_field(&f), NumView::Dash(FieldFlag::RangeError));
    }

    #[test]
    fn dash_badge_words_match_ui() {
        assert_eq!(dash_badge(FieldFlag::NotRead), "未取数");
        assert_eq!(dash_badge(FieldFlag::Offline), "源离线");
        assert_eq!(dash_badge(FieldFlag::RangeError), "数据异常");
        assert_eq!(dash_badge(FieldFlag::Valid), "");
    }

    #[test]
    fn soc_band_thresholds() {
        assert_eq!(soc_band(0.0), SocBand::Low);
        assert_eq!(soc_band(15.0), SocBand::Low);
        assert_eq!(soc_band(50.0), SocBand::Mid);
        assert_eq!(soc_band(85.0), SocBand::High);
        assert_eq!(soc_band(100.0), SocBand::High);
    }

    #[test]
    fn snapshot_soc_lost_when_none_or_source_lost() {
        let mut st = DisplayState::new();
        let mut f = frame(1, 1000);
        f.soc = None;
        f.soc_source = SocSource::Lost;
        st.record_success(f, 1000);
        let snap = st.snapshot(1000);
        assert_eq!(snap.soc, SocView::Lost);
        assert_eq!(snap.soc_source, SocSource::Lost);
    }

    // ---- 通道态 / 新鲜度 ----
    #[test]
    fn channel_init_then_connected() {
        let st = DisplayState::new();
        assert_eq!(st.channel_status(0), ChannelStatus::Init);
        let mut st = st;
        st.record_fail(0);
        assert_eq!(st.channel_status(500), ChannelStatus::Init);
        st.record_success(frame(1, 500), 500);
        assert_eq!(st.channel_status(2500), ChannelStatus::Connected);
        // 超过 3s 无成功 → Down（即使最后一次是成功的，只要没再来成功）
        assert_eq!(st.channel_status(500 + CHANNEL_DOWN_MS), ChannelStatus::Down);
    }

    #[test]
    fn init_timeout_becomes_down() {
        let mut st = DisplayState::new();
        st.record_fail(0);
        // 尚未 3s → 仍 Init
        assert_eq!(st.channel_status(1000), ChannelStatus::Init);
        // ≥3s 仍无成功 → Down
        assert_eq!(st.channel_status(CHANNEL_DOWN_MS), ChannelStatus::Down);
    }

    #[test]
    fn reconnect_after_down_recovers_on_next_success() {
        let mut st = DisplayState::new();
        st.record_success(frame(1, 0), 0);
        assert_eq!(st.channel_status(10000), ChannelStatus::Down);
        st.record_success(frame(2, 10000), 10000);
        assert_eq!(st.channel_status(10500), ChannelStatus::Connected);
    }

    #[test]
    fn freshness_stale_after_threshold() {
        let mut st = DisplayState::new();
        st.set_stale_ms(2000);
        st.record_success(frame(1, 1000), 1000);
        assert_eq!(st.freshness(2000), Freshness::Fresh);
        // now - ts = 2500 > 2000
        assert_eq!(st.freshness(1000 + 2500), Freshness::Stale);
    }

    /// W3：旧帧（seq 与 ts 双双回退）丢弃——屏面不回退到旧值，但通道仍记成功。
    #[test]
    fn out_of_order_older_frame_is_dropped_but_channel_ok() {
        let mut st = DisplayState::new();
        st.record_success(frame(10, 10_000), 10_000);
        // 乱序旧帧：seq 9 + ts 更旧 → 丢弃
        st.record_success(frame(9, 9_000), 10_100);
        assert_eq!(st.frame().unwrap().seq, 10, "更旧的帧不得覆盖较新帧");
        assert_eq!(st.reorder_dropped(), 1);
        assert_eq!(st.fail_streak(), 0, "通道是通的，仍记成功");
        assert_eq!(st.last_ok_ms(), Some(10_100));
        // 同 seq 重取（轮询同一帧）→ 幂等接受，不计乱序
        st.record_success(frame(10, 10_000), 10_200);
        assert_eq!(st.reorder_dropped(), 1);
    }

    /// W3：mupcd 重启 → seq 清零但 ts 更新 → 必须接受（否则屏面冻结到 seq 追平旧值）。
    #[test]
    fn seq_restart_with_newer_ts_is_accepted() {
        let mut st = DisplayState::new();
        st.record_success(frame(86_400, 10_000), 10_000);
        st.record_success(frame(0, 10_100), 10_100); // seq 回退但 ts 更新 = 新发布周期
        assert_eq!(st.frame().unwrap().seq, 0);
        assert_eq!(st.reorder_dropped(), 0, "重启清零不得被当作乱序丢弃");
    }

    #[test]
    fn fail_streak_preserves_last_frame() {
        let mut st = DisplayState::new();
        st.record_success(frame(5, 0), 0);
        st.record_fail(10);
        st.record_fail(510);
        st.record_fail(1010);
        assert_eq!(st.fail_streak(), 3);
        // 帧保留（冻结展示用），通道断但不清数值
        assert_eq!(st.frame().unwrap().seq, 5);
        assert_eq!(st.screen_mode(1010 + CHANNEL_DOWN_MS), ScreenMode::ChannelDown);
    }
}
