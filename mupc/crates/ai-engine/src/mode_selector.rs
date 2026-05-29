//! 运行场景模式选择器
//!
//! 5 种预设运行场景，互斥选择，支持远程控制（IEC 104/61850）
//! 和本地选择（Web UI/配置文件）。同一时刻仅 1 种模式生效。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

use crate::error::AiEngineError;

/// 预设运行场景（互斥，无 Default 兜底）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum RunningMode {
    /// MODE-01：农网灌溉模式 — 最大化光伏消纳 + 电压治理
    AgriculturalIrrigation = 1,
    /// MODE-02：自主套利模式 — 最大化峰谷电价差收益
    CommercialArbitrage = 2,
    /// MODE-03：需量控制模式 — 减免需量罚金
    DemandControl = 3,
    /// MODE-04：虚拟电厂模式 — 最大化辅助服务收益
    VirtualPowerPlant = 4,
    /// MODE-05：极致绿色模式 — 最大化绿电消纳
    UltraGreen = 5,
}

impl RunningMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::AgriculturalIrrigation),
            2 => Some(Self::CommercialArbitrage),
            3 => Some(Self::DemandControl),
            4 => Some(Self::VirtualPowerPlant),
            5 => Some(Self::UltraGreen),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AgriculturalIrrigation => "农网灌溉模式",
            Self::CommercialArbitrage => "自主套利模式",
            Self::DemandControl => "需量控制模式",
            Self::VirtualPowerPlant => "虚拟电厂模式",
            Self::UltraGreen => "极致绿色模式",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::AgriculturalIrrigation => "最大化光伏消纳 + 电压治理 + 防止变压器过载",
            Self::CommercialArbitrage => "最大化峰谷电价差收益 + 最小化电池损耗",
            Self::DemandControl => "减免需量罚金 + 最小化舒适度损失",
            Self::VirtualPowerPlant => "最大化辅助服务收益 + 响应精度",
            Self::UltraGreen => "最大化绿电消纳比例 + 最小化碳排放",
        }
    }

    pub fn all() -> &'static [RunningMode] {
        &[
            Self::AgriculturalIrrigation,
            Self::CommercialArbitrage,
            Self::DemandControl,
            Self::VirtualPowerPlant,
            Self::UltraGreen,
        ]
    }
}

impl std::fmt::Display for RunningMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// 切换来源
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SwitchSource {
    /// 调度主站远程切换（IEC 104 / IEC 61850）
    RemoteDispatch {
        protocol: String,
        address: String,
    },
    /// 本地 Web UI 切换
    LocalWeb {
        username: String,
    },
    /// 配置文件加载（系统启动时）
    LocalConfig,
}

impl std::fmt::Display for SwitchSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RemoteDispatch { protocol, .. } => write!(f, "远程调度({})", protocol),
            Self::LocalWeb { username } => write!(f, "本地Web({})", username),
            Self::LocalConfig => write!(f, "配置文件"),
        }
    }
}

/// 模式切换事件
#[derive(Debug, Clone, Serialize)]
pub struct ModeSwitchEvent {
    pub previous: RunningMode,
    pub current: RunningMode,
    pub source: SwitchSource,
    pub timestamp: i64,
}

/// 模式选择器（线程安全，互斥保证）
pub struct ModeSelector {
    current_mode: Arc<Mutex<RunningMode>>,
    switch_tx: broadcast::Sender<ModeSwitchEvent>,
    persist_path: Option<PathBuf>,
}

impl ModeSelector {
    pub fn new(initial: RunningMode, persist_path: Option<PathBuf>) -> Self {
        let (switch_tx, _) = broadcast::channel(64);
        Self {
            current_mode: Arc::new(Mutex::new(initial)),
            switch_tx,
            persist_path,
        }
    }

    /// 获取当前运行模式（非阻塞读）
    ///
    /// 使用 try_lock 实现零开销读取。写操作极低频（< 1次/分钟），
    /// 几乎无锁竞争。仅在恰好有写操作进行中时短暂自旋等待。
    pub fn current(&self) -> RunningMode {
        match self.current_mode.try_lock() {
            Ok(guard) => *guard,
            Err(_) => {
                let guard = tokio::task::block_in_place(|| self.current_mode.blocking_lock());
                *guard
            }
        }
    }

    /// 切换运行场景（原子操作，互斥保护）
    ///
    /// 幂等：如果 new_mode == current，不触发切换事件，直接返回 Ok。
    pub async fn switch(
        &self,
        new_mode: RunningMode,
        source: SwitchSource,
    ) -> Result<RunningMode, AiEngineError> {
        let mut current = self.current_mode.lock().await;
        let previous = *current;

        if previous == new_mode {
            return Ok(previous);
        }

        *current = new_mode;
        drop(current);

        if let Some(ref path) = self.persist_path {
            if let Err(e) = self.persist_mode(new_mode, path).await {
                tracing::error!("模式持久化失败: {}", e);
            }
        }

        let event = ModeSwitchEvent {
            previous,
            current: new_mode,
            source,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let _ = self.switch_tx.send(event);

        tracing::info!(
            "运行场景切换: {} → {}",
            previous.display_name(),
            new_mode.display_name()
        );

        Ok(previous)
    }

    /// 订阅模式切换事件
    pub fn subscribe(&self) -> broadcast::Receiver<ModeSwitchEvent> {
        self.switch_tx.subscribe()
    }

    async fn persist_mode(&self, mode: RunningMode, path: &PathBuf) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, format!("{}", mode as u8)).await
    }

    /// 从持久化文件恢复模式
    pub async fn restore_from_file(path: &std::path::Path) -> Result<RunningMode, AiEngineError> {
        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                let v: u8 = content.trim().parse().unwrap_or(0);
                RunningMode::from_u8(v).ok_or_else(|| {
                    AiEngineError::ModeSwitchFailed(format!("持久化文件中无效模式值: {}", v))
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RunningMode::AgriculturalIrrigation),
            Err(e) => Err(AiEngineError::ModeSwitchFailed(format!("读取持久化文件失败: {}", e))),
        }
    }
}

/// 解析模式名称（支持多种格式）
pub fn parse_mode_name(s: &str) -> Option<RunningMode> {
    match s.to_lowercase().as_str() {
        "agriculturalirrigation" | "agricultural_irrigation" | "mode-01" | "1" => {
            Some(RunningMode::AgriculturalIrrigation)
        }
        "commercialarbitrage" | "commercial_arbitrage" | "mode-02" | "2" => {
            Some(RunningMode::CommercialArbitrage)
        }
        "demandcontrol" | "demand_control" | "mode-03" | "3" => {
            Some(RunningMode::DemandControl)
        }
        "virtualpowerplant" | "virtual_power_plant" | "mode-04" | "4" => {
            Some(RunningMode::VirtualPowerPlant)
        }
        "ultragreen" | "ultra_green" | "mode-05" | "5" => {
            Some(RunningMode::UltraGreen)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_running_mode_from_u8() {
        assert_eq!(RunningMode::from_u8(1), Some(RunningMode::AgriculturalIrrigation));
        assert_eq!(RunningMode::from_u8(5), Some(RunningMode::UltraGreen));
        assert_eq!(RunningMode::from_u8(0), None);
        assert_eq!(RunningMode::from_u8(6), None);
    }

    #[test]
    fn test_running_mode_display_name() {
        assert_eq!(RunningMode::AgriculturalIrrigation.display_name(), "农网灌溉模式");
        assert_eq!(RunningMode::CommercialArbitrage.display_name(), "自主套利模式");
    }

    #[test]
    fn test_running_mode_all_count() {
        assert_eq!(RunningMode::all().len(), 5);
    }

    #[test]
    fn test_parse_mode_name() {
        assert_eq!(parse_mode_name("agriculturalirrigation"), Some(RunningMode::AgriculturalIrrigation));
        assert_eq!(parse_mode_name("AgriculturalIrrigation"), Some(RunningMode::AgriculturalIrrigation));
        assert_eq!(parse_mode_name("1"), Some(RunningMode::AgriculturalIrrigation));
        assert_eq!(parse_mode_name("mode-01"), Some(RunningMode::AgriculturalIrrigation));
        assert_eq!(parse_mode_name("ultragreen"), Some(RunningMode::UltraGreen));
        assert_eq!(parse_mode_name("invalid"), None);
        assert_eq!(parse_mode_name("6"), None);
    }

    #[tokio::test]
    async fn test_mode_selector_initial_current() {
        let selector = ModeSelector::new(RunningMode::AgriculturalIrrigation, None);
        assert_eq!(selector.current(), RunningMode::AgriculturalIrrigation);
    }

    #[tokio::test]
    async fn test_mode_selector_switch() {
        let selector = ModeSelector::new(RunningMode::AgriculturalIrrigation, None);
        let prev = selector.switch(RunningMode::CommercialArbitrage, SwitchSource::LocalConfig).await;
        assert_eq!(prev, Ok(RunningMode::AgriculturalIrrigation));
        assert_eq!(selector.current(), RunningMode::CommercialArbitrage);
    }

    #[tokio::test]
    async fn test_mode_selector_switch_idempotent() {
        let selector = ModeSelector::new(RunningMode::AgriculturalIrrigation, None);
        let prev = selector.switch(RunningMode::AgriculturalIrrigation, SwitchSource::LocalConfig).await;
        assert_eq!(prev, Ok(RunningMode::AgriculturalIrrigation));
    }

    #[tokio::test]
    async fn test_mode_selector_subscribe() {
        let selector = ModeSelector::new(RunningMode::AgriculturalIrrigation, None);
        let mut rx = selector.subscribe();
        let _ = selector.switch(RunningMode::DemandControl, SwitchSource::LocalConfig).await;
        let event = rx.recv().await.unwrap();
        assert_eq!(event.previous, RunningMode::AgriculturalIrrigation);
        assert_eq!(event.current, RunningMode::DemandControl);
    }
}
