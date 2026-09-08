//! 站运行时模型（配置 → 调度状态）。

use crate::config::{Role, StationConf};

/// 站调度态（scheduler 维护）
#[derive(Debug, Clone)]
pub struct Station {
    pub conf: StationConf,
    /// 距上次成功读的累计失败（站隔离 offline 计数）
    pub offline_count: u32,
    /// 上次成功采集时间（新鲜度/事件去抖用）
    pub last_ok: Option<chrono::DateTime<chrono::Utc>>,
    /// 上次触发 offline 事件时间（防每帧刷屏）
    pub last_offline_event: Option<chrono::DateTime<chrono::Utc>>,
}

impl Station {
    pub fn from_conf(c: StationConf) -> Self {
        Self {
            conf: c,
            offline_count: 0,
            last_ok: None,
            last_offline_event: None,
        }
    }

    pub fn role(&self) -> Role {
        self.conf.role
    }
}
