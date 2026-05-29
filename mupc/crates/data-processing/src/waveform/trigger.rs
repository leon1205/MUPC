//! 触发判定引擎
//!
//! 在每次采样点到达时同步执行触发判定，支持过压/欠压/过流/短路/频率越限/零序过流
//! 六种触发条件，每种条件独立状态机管理，包含防抖和回差逻辑。
//!
//! # 触发判定流程
//!
//! 1. 冷却检查 → 2. 逐条件判定（防抖 + 回差） → 3. 返回触发结果

use std::sync::atomic::{AtomicI64, Ordering};

/// 触发条件配置
///
/// 包含 15 个字段，覆盖六种故障触发条件及其参数。
#[derive(Debug, Clone)]
pub struct TriggerConfig {
    /// 过压阈值 (V)，默认 420.0
    pub over_voltage_threshold: f64,
    /// 欠压阈值 (V)，默认 200.0
    pub under_voltage_threshold: f64,
    /// 过流阈值 (A)，默认 150.0
    pub over_current_threshold: f64,
    /// 短路阈值 (A，瞬时值)，默认 500.0
    pub short_circuit_threshold: f64,
    /// 频率上限 (Hz)，默认 50.5
    pub frequency_high: f64,
    /// 频率下限 (Hz)，默认 49.5
    pub frequency_low: f64,
    /// 零序过流阈值 (A)，默认 20.0
    pub zero_seq_threshold: f64,
    /// 回差百分比（%），默认 5.0（即阈值的 5%）
    pub hysteresis_pct: f64,
    /// 防抖确认窗口（连续采样点数），默认 3
    pub debounce_samples: u32,
    /// 采样率 (Hz)，默认 4000
    pub sample_rate: u32,
    /// 故障前记录时长 (ms)，默认 200
    pub pre_trigger_ms: u32,
    /// 故障后记录时长 (ms)，默认 1000
    pub post_trigger_ms: u32,
    /// 通道启用掩码
    pub channel_mask: u16,
    /// 冷却时间 (ms)，默认 5000
    pub cool_down_ms: u32,
    /// 是否启用触发引擎
    pub enabled: bool,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            over_voltage_threshold: 420.0,
            under_voltage_threshold: 200.0,
            over_current_threshold: 150.0,
            short_circuit_threshold: 500.0,
            frequency_high: 50.5,
            frequency_low: 49.5,
            zero_seq_threshold: 20.0,
            hysteresis_pct: 5.0,
            debounce_samples: 3,
            sample_rate: 4000,
            pre_trigger_ms: 200,
            post_trigger_ms: 1000,
            channel_mask: ChannelMask::ALL,
            cool_down_ms: 5000,
            enabled: true,
        }
    }
}

/// 通道掩码常量
pub struct ChannelMask;
impl ChannelMask {
    /// 三相电压通道
    pub const VOLTAGE_3PHASE: u16 = 0b0000_0000_0111;
    /// 三相电流通道
    pub const CURRENT_3PHASE: u16 = 0b0000_0011_1000;
    /// 零序通道
    pub const ZERO_SEQUENCE: u16 = 0b0000_1100_0000;
    /// 功率通道
    pub const POWER: u16 = 0b0011_0000_0000;
    /// 全部通道
    pub const ALL: u16 = 0b0011_1111_1111;
}

/// 触发结果枚举
///
/// 表示当前采样点的触发判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerResult {
    /// 无触发
    None,
    /// 过压触发
    OverVoltage,
    /// 欠压触发
    UnderVoltage,
    /// 过流触发
    OverCurrent,
    /// 短路触发
    ShortCircuit,
    /// 频率过高触发
    FrequencyHigh,
    /// 频率过低触发
    FrequencyLow,
    /// 零序过流触发
    ZeroSeqOverCurrent,
}

impl TriggerResult {
    /// 判断是否触发了故障
    pub fn is_triggered(&self) -> bool {
        !matches!(self, TriggerResult::None)
    }

    /// 获取触发类型名称
    pub fn name(&self) -> &'static str {
        match self {
            TriggerResult::None => "NONE",
            TriggerResult::OverVoltage => "OVER_VOLTAGE",
            TriggerResult::UnderVoltage => "UNDER_VOLTAGE",
            TriggerResult::OverCurrent => "OVER_CURRENT",
            TriggerResult::ShortCircuit => "SHORT_CIRCUIT",
            TriggerResult::FrequencyHigh => "FREQUENCY_HIGH",
            TriggerResult::FrequencyLow => "FREQUENCY_LOW",
            TriggerResult::ZeroSeqOverCurrent => "ZERO_SEQ_OVER_CURRENT",
        }
    }
}

/// 单个触发条件的状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConditionState {
    /// 正常状态
    Normal,
    /// 已触发（等待回差恢复）
    Triggered,
    /// 回差等待中
    HysteresisWaiting,
}

/// 触发引擎
///
/// 每个采样点到达时调用 `detect()` 方法进行同步判定。
/// 内部维护每个触发条件的独立状态机和防抖计数器。
pub struct TriggerEngine {
    /// 触发配置
    config: TriggerConfig,
    /// 每个触发条件的状态（按 TriggerResult 枚举顺序，排除 None）
    states: [ConditionState; 7],
    /// 防抖计数器
    debounce_counters: [u32; 7],
    /// 冷却结束时间戳（微秒）
    cooldown_until: AtomicI64,
}

impl TriggerEngine {
    /// 创建新的触发引擎
    ///
    /// # 参数
    ///
    /// * `config` - 触发条件配置
    pub fn new(config: TriggerConfig) -> Self {
        Self {
            config,
            states: [ConditionState::Normal; 7],
            debounce_counters: [0; 7],
            cooldown_until: AtomicI64::new(0),
        }
    }

    /// 使用默认配置创建触发引擎
    pub fn default() -> Self {
        Self::new(TriggerConfig::default())
    }

    /// 执行触发判定
    ///
    /// 对每个采样点同步调用，检查所有启用的触发条件。
    ///
    /// # 参数
    ///
    /// * `ua` - A相电压瞬时值 (V)
    /// * `ub` - B相电压瞬时值 (V)
    /// * `uc` - C相电压瞬时值 (V)
    /// * `ia` - A相电流瞬时值 (A)
    /// * `ib` - B相电流瞬时值 (A)
    /// * `ic` - C相电流瞬时值 (A)
    /// * `u0` - 零序电压瞬时值 (V)
    /// * `i0` - 零序电流瞬时值 (A)
    /// * `freq` - 频率 (Hz)
    /// * `timestamp_us` - 当前采样点时间戳（微秒）
    ///
    /// # 返回
    ///
    /// 触发结果，`TriggerResult::None` 表示无触发
    pub fn detect(
        &mut self,
        ua: f64,
        ub: f64,
        uc: f64,
        ia: f64,
        ib: f64,
        ic: f64,
        u0: f64,
        i0: f64,
        freq: f64,
        timestamp_us: i64,
    ) -> TriggerResult {
        if !self.config.enabled {
            return TriggerResult::None;
        }

        // 冷却检查
        let cooldown = self.cooldown_until.load(Ordering::Acquire);
        if timestamp_us < cooldown {
            return TriggerResult::None;
        }

        // 获取电压/电流最大值（三相）
        let max_voltage = ua.abs().max(ub.abs()).max(uc.abs());
        let max_current = ia.abs().max(ib.abs()).max(ic.abs());

        // 依次检查每种触发条件（按优先级）
        // 短路优先级最高
        if self.check_condition(
            0,
            max_current > self.config.short_circuit_threshold,
            timestamp_us,
        ) {
            return TriggerResult::ShortCircuit;
        }

        // 过压
        if self.check_condition(
            1,
            max_voltage > self.config.over_voltage_threshold,
            timestamp_us,
        ) {
            return TriggerResult::OverVoltage;
        }

        // 欠压
        if self.check_condition(
            2,
            max_voltage < self.config.under_voltage_threshold && max_voltage > 0.0,
            timestamp_us,
        ) {
            return TriggerResult::UnderVoltage;
        }

        // 过流
        if self.check_condition(
            3,
            max_current > self.config.over_current_threshold,
            timestamp_us,
        ) {
            return TriggerResult::OverCurrent;
        }

        // 频率过高
        if self.check_condition(4, freq > self.config.frequency_high, timestamp_us) {
            return TriggerResult::FrequencyHigh;
        }

        // 频率过低
        if self.check_condition(
            5,
            freq < self.config.frequency_low && freq > 0.0,
            timestamp_us,
        ) {
            return TriggerResult::FrequencyLow;
        }

        // 零序过流
        if self.check_condition(
            6,
            i0.abs() > self.config.zero_seq_threshold,
            timestamp_us,
        ) {
            return TriggerResult::ZeroSeqOverCurrent;
        }

        TriggerResult::None
    }

    /// 检查单个触发条件
    ///
    /// 实现防抖逻辑：需要连续 `debounce_samples` 次满足条件才真正触发。
    /// 触发后进入冷却期。
    fn check_condition(&mut self, idx: usize, condition_met: bool, timestamp_us: i64) -> bool {
        match self.states[idx] {
            ConditionState::Normal => {
                if condition_met {
                    self.debounce_counters[idx] += 1;
                    if self.debounce_counters[idx] >= self.config.debounce_samples {
                        // 触发确认
                        self.states[idx] = ConditionState::Triggered;
                        self.debounce_counters[idx] = 0;

                        // 进入冷却期
                        let cooldown_us =
                            timestamp_us + (self.config.cool_down_ms as i64) * 1000;
                        self.cooldown_until
                            .store(cooldown_us, Ordering::Release);

                        return true;
                    }
                } else {
                    self.debounce_counters[idx] = 0;
                }
                false
            }
            ConditionState::Triggered => {
                if !condition_met {
                    self.states[idx] = ConditionState::HysteresisWaiting;
                    self.debounce_counters[idx] = 1;
                }
                false
            }
            ConditionState::HysteresisWaiting => {
                if condition_met {
                    // 重新满足 → 回到触发态
                    self.states[idx] = ConditionState::Triggered;
                    self.debounce_counters[idx] = 0;
                } else {
                    self.debounce_counters[idx] += 1;
                    if self.debounce_counters[idx] >= self.config.debounce_samples {
                        // 完全恢复
                        self.states[idx] = ConditionState::Normal;
                        self.debounce_counters[idx] = 0;
                    }
                }
                false
            }
        }
    }

    /// 更新触发配置
    pub fn update_config(&mut self, config: TriggerConfig) {
        self.config = config;
        // 重置所有状态
        self.states = [ConditionState::Normal; 7];
        self.debounce_counters = [0; 7];
    }

    /// 获取当前配置的只读引用
    pub fn config(&self) -> &TriggerConfig {
        &self.config
    }

    /// 重置引擎状态（不清除配置）
    pub fn reset(&mut self) {
        self.states = [ConditionState::Normal; 7];
        self.debounce_counters = [0; 7];
        self.cooldown_until.store(0, Ordering::Release);
    }

    /// 计算故障前采样点数
    pub fn pre_trigger_samples(&self) -> usize {
        (self.config.sample_rate as usize * self.config.pre_trigger_ms as usize) / 1000
    }

    /// 计算故障后采样点数
    pub fn post_trigger_samples(&self) -> usize {
        (self.config.sample_rate as usize * self.config.post_trigger_ms as usize) / 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_engine() -> TriggerEngine {
        let mut config = TriggerConfig::default();
        config.debounce_samples = 1; // 测试用，不防抖
        config.cool_down_ms = 100;
        config.enabled = true;
        TriggerEngine::new(config)
    }

    #[test]
    fn test_no_trigger_normal() {
        let mut engine = make_engine();
        let result = engine.detect(220.0, 220.0, 220.0, 10.0, 10.0, 10.0, 0.0, 0.0, 50.0, 1000);
        assert_eq!(result, TriggerResult::None);
    }

    #[test]
    fn test_over_voltage_trigger() {
        let mut engine = make_engine();
        let result =
            engine.detect(430.0, 430.0, 430.0, 10.0, 10.0, 10.0, 0.0, 0.0, 50.0, 1000);
        assert_eq!(result, TriggerResult::OverVoltage);
    }

    #[test]
    fn test_under_voltage_trigger() {
        let mut engine = make_engine();
        let result =
            engine.detect(180.0, 180.0, 180.0, 10.0, 10.0, 10.0, 0.0, 0.0, 50.0, 1000);
        assert_eq!(result, TriggerResult::UnderVoltage);
    }

    #[test]
    fn test_over_current_trigger() {
        let mut engine = make_engine();
        let result =
            engine.detect(220.0, 220.0, 220.0, 160.0, 160.0, 160.0, 0.0, 0.0, 50.0, 1000);
        assert_eq!(result, TriggerResult::OverCurrent);
    }

    #[test]
    fn test_short_circuit_trigger() {
        let mut engine = make_engine();
        let result =
            engine.detect(220.0, 220.0, 220.0, 600.0, 600.0, 600.0, 0.0, 0.0, 50.0, 1000);
        assert_eq!(result, TriggerResult::ShortCircuit);
    }

    #[test]
    fn test_cooldown() {
        let mut engine = make_engine();
        // 第一次触发
        let result =
            engine.detect(430.0, 430.0, 430.0, 10.0, 10.0, 10.0, 0.0, 0.0, 50.0, 1000);
        assert_eq!(result, TriggerResult::OverVoltage);

        // 冷却期内不触发
        let result =
            engine.detect(430.0, 430.0, 430.0, 10.0, 10.0, 10.0, 0.0, 0.0, 50.0, 50000);
        assert_eq!(result, TriggerResult::None);

        // 冷却期过后再次触发
        let result =
            engine.detect(430.0, 430.0, 430.0, 10.0, 10.0, 10.0, 0.0, 0.0, 50.0, 200000);
        assert_eq!(result, TriggerResult::OverVoltage);
    }

    #[test]
    fn test_disabled_engine() {
        let mut config = TriggerConfig::default();
        config.enabled = false;
        let mut engine = TriggerEngine::new(config);
        let result =
            engine.detect(430.0, 430.0, 430.0, 10.0, 10.0, 10.0, 0.0, 0.0, 50.0, 1000);
        assert_eq!(result, TriggerResult::None);
    }
}
