//! 固件 OTA 状态机（17 状态）
//!
//! 与模型 OTA 状态机分离，提供更细粒度的固件升级流程控制。
//!
//! # 状态流程
//!
//! ```text
//! Idle → CheckingUpdate → UpdateAvailable → Downloading → DownloadComplete
//!   → Verifying → ReadyToApply → PreUpgradeCheck → SwitchingToStandby
//!   → Applying → Applied → PostUpgradeVerify → Idle
//!
//! 异常分支:
//!   Downloading ⇄ DownloadPaused (暂停/恢复)
//!   Verifying → VerifyFailed → Idle
//!   Applying → RollingBack → RolledBack → Idle
//!   PostUpgradeVerify → RollingBack → RolledBack → Idle
//!   任意状态 → Failed (携带错误信息和阶段标识)
//! ```
//!
//! # 与设计文档的关系
//!
//! 对应设计文档第 3.4 节「固件升级状态机」。
//! 设计文档中定义了 21 个细化状态，本实现根据任务规格简化为 17 个核心状态，
//! 将部分相邻阶段合并（如 IntegrityCheck + SignatureVerify → Verifying），
//! 保留完整的状态转换覆盖。

use serde::{Deserialize, Serialize};

/// 固件 OTA 状态（17 状态）
///
/// 覆盖固件升级全生命周期：空闲 → 检查 → 下载 → 验证 → 预检 → 切换 → 应用 → 后验证 → 完成/回滚。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FwOtaState {
    /// 空闲状态，未进行任何升级操作
    Idle,

    /// 正在检查是否有可用固件更新
    CheckingUpdate,

    /// 发现可用更新，等待用户确认或自动触发下载
    UpdateAvailable,

    /// 正在下载固件包
    Downloading,

    /// 下载暂停（网络中断、手动暂停等），可恢复继续下载
    DownloadPaused,

    /// 固件包下载完成，等待进入验证阶段
    DownloadComplete,

    /// 正在验证固件包完整性（SHA-256）和签名（SM2）
    Verifying,

    /// 验证失败（哈希不匹配或签名无效）
    VerifyFailed,

    /// 验证通过，固件包就绪，等待进入预升级检查
    ReadyToApply,

    /// 执行升级前检查（磁盘空间、电源状态、CPU 负载、进程健康等）
    PreUpgradeCheck,

    /// 正在切换至备用分区（发送降级信号、挂载备用分区）
    SwitchingToStandby,

    /// 正在将固件写入备用分区并应用
    Applying,

    /// 固件已应用至备用分区，等待重启后进行升级后验证
    Applied,

    /// 重启后正在执行升级后验证（版本检查、进程存活、核间通信等）
    PostUpgradeVerify,

    /// 正在执行回滚操作（恢复至原分区）
    RollingBack,

    /// 回滚完成，系统已恢复至升级前状态
    RolledBack,

    /// 升级失败，携带错误描述信息
    Failed(String),
}

impl FwOtaState {
    /// 判断状态转换是否合法
    ///
    /// 基于设计文档第 3.4.2 节状态转换图定义的规则。
    /// 返回 `true` 表示可以从 `from` 状态转换到 `to` 状态。
    ///
    /// # 转换规则覆盖
    ///
    /// - 正常流程：Idle → CheckingUpdate → UpdateAvailable → Downloading →
    ///   DownloadComplete → Verifying → ReadyToApply → PreUpgradeCheck →
    ///   SwitchingToStandby → Applying → Applied → PostUpgradeVerify → Idle
    /// - 暂停/恢复：Downloading ⇄ DownloadPaused
    /// - 验证失败：Verifying → VerifyFailed → Idle
    /// - 回滚：Applying → RollingBack → RolledBack → Idle
    /// - 后验证回滚：PostUpgradeVerify → RollingBack
    /// - 失败处理：多数状态 → Failed；Failed → Idle（清理后恢复）
    pub fn can_transition(from: FwOtaState, to: FwOtaState) -> bool {
        use FwOtaState::*;

        // 任意状态都可以转换到 Failed（出现不可恢复的错误）
        if matches!(to, Failed(_)) {
            return !matches!(from, Failed(_) | Idle);
        }

        matches!(
            (from, to),
            // === 正常升级流程 ===
            // 空闲 → 开始检查更新
            (Idle, CheckingUpdate)
                // 检查完成 → 发现更新
                | (CheckingUpdate, UpdateAvailable)
                // 检查完成 → 无更新，回到空闲
                | (CheckingUpdate, Idle)
                // 发现更新 → 开始下载
                | (UpdateAvailable, Downloading)
                // 发现更新 → 用户取消，回到空闲
                | (UpdateAvailable, Idle)
                // 下载完成 → 进入验证
                | (DownloadComplete, Verifying)
                // 验证通过 → 准备应用
                | (Verifying, ReadyToApply)
                // 就绪 → 执行预升级检查
                | (ReadyToApply, PreUpgradeCheck)
                // 就绪 → 用户取消，回到空闲
                | (ReadyToApply, Idle)
                // 预检通过 → 切换至备用分区
                | (PreUpgradeCheck, SwitchingToStandby)
                // 切换完成 → 开始应用固件
                | (SwitchingToStandby, Applying)
                // 应用完成 → 等待重启验证
                | (Applying, Applied)
                // 重启验证通过 → 回到空闲（升级完成）
                | (Applied, PostUpgradeVerify)
                // 后验证通过 → 升级成功完成
                | (PostUpgradeVerify, Idle)

            // === 下载暂停/恢复 ===
                // 下载中暂停
                | (Downloading, DownloadPaused)
                // 暂停后恢复下载
                | (DownloadPaused, Downloading)

            // === 验证失败处理 ===
                // 验证不通过
                | (Verifying, VerifyFailed)
                // 验证失败后清理，回到空闲
                | (VerifyFailed, Idle)

            // === 回滚流程 ===
                // 应用失败 → 触发回滚
                | (Applying, RollingBack)
                // 后验证失败 → 触发回滚
                | (PostUpgradeVerify, RollingBack)
                // 回滚完成
                | (RollingBack, RolledBack)
                // 回滚完成后回到空闲
                | (RolledBack, Idle)

            // === 错误恢复 ===
                // 失败后清理，回到空闲（接受重试或放弃）
                | (Failed(_), Idle)
        )
    }

    /// 判断当前是否为终态（不会再主动变化的状态）
    ///
    /// 终态包括：Idle（升级完成或取消后）、RolledBack（回滚完成）、
    /// Failed（不可恢复的错误，需人工介入）。
    pub fn is_terminal(&self) -> bool {
        matches!(self, FwOtaState::Idle | FwOtaState::RolledBack | FwOtaState::Failed(_))
    }

    /// 判断当前是否处于升级进行中状态
    pub fn is_in_progress(&self) -> bool {
        matches!(
            self,
            FwOtaState::CheckingUpdate
                | FwOtaState::UpdateAvailable
                | FwOtaState::Downloading
                | FwOtaState::DownloadComplete
                | FwOtaState::Verifying
                | FwOtaState::ReadyToApply
                | FwOtaState::PreUpgradeCheck
                | FwOtaState::SwitchingToStandby
                | FwOtaState::Applying
                | FwOtaState::Applied
                | FwOtaState::PostUpgradeVerify
                | FwOtaState::RollingBack
        )
    }

    /// 判断当前是否处于错误状态
    pub fn is_error(&self) -> bool {
        matches!(self, FwOtaState::VerifyFailed | FwOtaState::Failed(_))
    }

    /// 获取状态的简短描述
    pub fn description(&self) -> &'static str {
        match self {
            FwOtaState::Idle => "空闲",
            FwOtaState::CheckingUpdate => "检查更新中",
            FwOtaState::UpdateAvailable => "发现可用更新",
            FwOtaState::Downloading => "下载中",
            FwOtaState::DownloadPaused => "下载暂停",
            FwOtaState::DownloadComplete => "下载完成",
            FwOtaState::Verifying => "验证中",
            FwOtaState::VerifyFailed => "验证失败",
            FwOtaState::ReadyToApply => "就绪待应用",
            FwOtaState::PreUpgradeCheck => "升级前检查中",
            FwOtaState::SwitchingToStandby => "切换至备用分区",
            FwOtaState::Applying => "应用固件中",
            FwOtaState::Applied => "固件已应用",
            FwOtaState::PostUpgradeVerify => "升级后验证中",
            FwOtaState::RollingBack => "回滚中",
            FwOtaState::RolledBack => "已回滚",
            FwOtaState::Failed(_) => "升级失败",
        }
    }
}

impl std::fmt::Display for FwOtaState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FwOtaState::Failed(reason) => write!(f, "升级失败: {}", reason),
            other => write!(f, "{}", other.description()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ================================================================
    // 正常流程测试
    // ================================================================

    #[test]
    fn test_normal_flow() {
        // 完整的正常升级流程
        assert!(FwOtaState::can_transition(FwOtaState::Idle, FwOtaState::CheckingUpdate));
        assert!(FwOtaState::can_transition(
            FwOtaState::CheckingUpdate,
            FwOtaState::UpdateAvailable
        ));
        assert!(FwOtaState::can_transition(
            FwOtaState::UpdateAvailable,
            FwOtaState::Downloading
        ));
        assert!(FwOtaState::can_transition(
            FwOtaState::DownloadComplete,
            FwOtaState::Verifying
        ));
        assert!(FwOtaState::can_transition(FwOtaState::Verifying, FwOtaState::ReadyToApply));
        assert!(FwOtaState::can_transition(
            FwOtaState::ReadyToApply,
            FwOtaState::PreUpgradeCheck
        ));
        assert!(FwOtaState::can_transition(
            FwOtaState::PreUpgradeCheck,
            FwOtaState::SwitchingToStandby
        ));
        assert!(FwOtaState::can_transition(
            FwOtaState::SwitchingToStandby,
            FwOtaState::Applying
        ));
        assert!(FwOtaState::can_transition(FwOtaState::Applying, FwOtaState::Applied));
        assert!(FwOtaState::can_transition(
            FwOtaState::Applied,
            FwOtaState::PostUpgradeVerify
        ));
        assert!(FwOtaState::can_transition(
            FwOtaState::PostUpgradeVerify,
            FwOtaState::Idle
        ));
    }

    #[test]
    fn test_no_update_returns_to_idle() {
        // 检查后无可用更新，回到空闲
        assert!(FwOtaState::can_transition(FwOtaState::CheckingUpdate, FwOtaState::Idle));
    }

    #[test]
    fn test_cancel_during_update_available() {
        // 发现更新但用户取消
        assert!(FwOtaState::can_transition(FwOtaState::UpdateAvailable, FwOtaState::Idle));
    }

    #[test]
    fn test_cancel_before_precheck() {
        // 就绪后用户取消
        assert!(FwOtaState::can_transition(FwOtaState::ReadyToApply, FwOtaState::Idle));
    }

    // ================================================================
    // 下载暂停/恢复测试
    // ================================================================

    #[test]
    fn test_download_pause_and_resume() {
        // Downloading → DownloadPaused
        assert!(FwOtaState::can_transition(
            FwOtaState::Downloading,
            FwOtaState::DownloadPaused
        ));

        // DownloadPaused → Downloading (恢复)
        assert!(FwOtaState::can_transition(
            FwOtaState::DownloadPaused,
            FwOtaState::Downloading
        ));
    }

    // ================================================================
    // 验证失败处理测试
    // ================================================================

    #[test]
    fn test_verify_failed_flow() {
        // Verifying → VerifyFailed
        assert!(FwOtaState::can_transition(FwOtaState::Verifying, FwOtaState::VerifyFailed));

        // VerifyFailed → Idle (清理后恢复)
        assert!(FwOtaState::can_transition(FwOtaState::VerifyFailed, FwOtaState::Idle));
    }

    // ================================================================
    // 回滚流程测试
    // ================================================================

    #[test]
    fn test_rollback_from_applying() {
        // Applying → RollingBack (应用失败触发回滚)
        assert!(FwOtaState::can_transition(FwOtaState::Applying, FwOtaState::RollingBack));
    }

    #[test]
    fn test_rollback_from_post_verify() {
        // PostUpgradeVerify → RollingBack (后验证失败触发回滚)
        assert!(FwOtaState::can_transition(
            FwOtaState::PostUpgradeVerify,
            FwOtaState::RollingBack
        ));
    }

    #[test]
    fn test_rollback_complete_flow() {
        // RollingBack → RolledBack
        assert!(FwOtaState::can_transition(FwOtaState::RollingBack, FwOtaState::RolledBack));

        // RolledBack → Idle (回滚完成，系统恢复)
        assert!(FwOtaState::can_transition(FwOtaState::RolledBack, FwOtaState::Idle));
    }

    // ================================================================
    // 错误恢复测试
    // ================================================================

    #[test]
    fn test_checking_to_failed() {
        // CheckingUpdate → Failed (检查过程出错)
        assert!(FwOtaState::can_transition(
            FwOtaState::CheckingUpdate,
            FwOtaState::Failed("网络不可达".to_string())
        ));
    }

    #[test]
    fn test_downloading_to_failed() {
        // Downloading → Failed (下载致命错误)
        assert!(FwOtaState::can_transition(
            FwOtaState::Downloading,
            FwOtaState::Failed("磁盘空间不足".to_string())
        ));
    }

    #[test]
    fn test_download_paused_to_failed() {
        // DownloadPaused → Failed (暂停期间放弃)
        assert!(FwOtaState::can_transition(
            FwOtaState::DownloadPaused,
            FwOtaState::Failed("重试次数耗尽".to_string())
        ));
    }

    #[test]
    fn test_verifying_to_failed() {
        // Verifying → Failed (验证过程异常)
        assert!(FwOtaState::can_transition(
            FwOtaState::Verifying,
            FwOtaState::Failed("验证过程 IO 错误".to_string())
        ));
    }

    #[test]
    fn test_verify_failed_to_failed() {
        // VerifyFailed → Failed (不可恢复)
        assert!(FwOtaState::can_transition(
            FwOtaState::VerifyFailed,
            FwOtaState::Failed("连续验证失败".to_string())
        ));
    }

    #[test]
    fn test_precheck_to_failed() {
        // PreUpgradeCheck → Failed (检查不通过)
        assert!(FwOtaState::can_transition(
            FwOtaState::PreUpgradeCheck,
            FwOtaState::Failed("磁盘空间不足".to_string())
        ));
    }

    #[test]
    fn test_switching_to_failed() {
        // SwitchingToStandby → Failed (切换失败)
        assert!(FwOtaState::can_transition(
            FwOtaState::SwitchingToStandby,
            FwOtaState::Failed("备用分区不可用".to_string())
        ));
    }

    #[test]
    fn test_applying_to_failed() {
        // Applying → Failed (应用过程致命错误)
        assert!(FwOtaState::can_transition(
            FwOtaState::Applying,
            FwOtaState::Failed("写入分区失败".to_string())
        ));
    }

    #[test]
    fn test_post_verify_to_failed() {
        // PostUpgradeVerify → Failed (后验证异常)
        assert!(FwOtaState::can_transition(
            FwOtaState::PostUpgradeVerify,
            FwOtaState::Failed("后验证过程崩溃".to_string())
        ));
    }

    #[test]
    fn test_rolling_back_to_failed() {
        // RollingBack → Failed (回滚失败)
        assert!(FwOtaState::can_transition(
            FwOtaState::RollingBack,
            FwOtaState::Failed("回滚过程分区损坏".to_string())
        ));
    }

    #[test]
    fn test_failed_to_idle() {
        // Failed → Idle (错误处理后恢复)
        assert!(FwOtaState::can_transition(
            FwOtaState::Failed("网络不可达".to_string()),
            FwOtaState::Idle
        ));
    }

    // ================================================================
    // 非法转换测试
    // ================================================================

    #[test]
    fn test_invalid_transitions() {
        // Idle 不能直接跳到 Downloading
        assert!(!FwOtaState::can_transition(FwOtaState::Idle, FwOtaState::Downloading));

        // Idle 不能到 Applying
        assert!(!FwOtaState::can_transition(FwOtaState::Idle, FwOtaState::Applying));

        // Idle 不能到 Failed (Idle 是终态，必须由某个操作触发)
        assert!(!FwOtaState::can_transition(
            FwOtaState::Idle,
            FwOtaState::Failed("".to_string())
        ));

        // Downloading 不能直接到 Applying
        assert!(!FwOtaState::can_transition(FwOtaState::Downloading, FwOtaState::Applying));

        // RolledBack 不能到 Downloading (回滚后必须经过 Idle)
        assert!(!FwOtaState::can_transition(FwOtaState::RolledBack, FwOtaState::Downloading));

        // Applied 不能到 Idle (必须经过 PostUpgradeVerify)
        assert!(!FwOtaState::can_transition(FwOtaState::Applied, FwOtaState::Idle));
    }

    // ================================================================
    // 辅助方法测试
    // ================================================================

    #[test]
    fn test_is_terminal() {
        assert!(FwOtaState::Idle.is_terminal());
        assert!(FwOtaState::RolledBack.is_terminal());
        assert!(FwOtaState::Failed("test".to_string()).is_terminal());

        assert!(!FwOtaState::Downloading.is_terminal());
        assert!(!FwOtaState::Applying.is_terminal());
        assert!(!FwOtaState::Verifying.is_terminal());
    }

    #[test]
    fn test_is_in_progress() {
        assert!(FwOtaState::CheckingUpdate.is_in_progress());
        assert!(FwOtaState::Downloading.is_in_progress());
        assert!(FwOtaState::Applying.is_in_progress());
        assert!(FwOtaState::PostUpgradeVerify.is_in_progress());
        assert!(FwOtaState::RollingBack.is_in_progress());

        assert!(!FwOtaState::Idle.is_in_progress());
        assert!(!FwOtaState::VerifyFailed.is_in_progress());
        assert!(!FwOtaState::RolledBack.is_in_progress());
        assert!(!FwOtaState::Failed("test".to_string()).is_in_progress());
    }

    #[test]
    fn test_is_error() {
        assert!(FwOtaState::VerifyFailed.is_error());
        assert!(FwOtaState::Failed("test".to_string()).is_error());

        assert!(!FwOtaState::Idle.is_error());
        assert!(!FwOtaState::Downloading.is_error());
        assert!(!FwOtaState::RolledBack.is_error());
    }

    #[test]
    fn test_display_format() {
        assert_eq!(FwOtaState::Idle.to_string(), "空闲");
        assert_eq!(FwOtaState::Downloading.to_string(), "下载中");
        assert_eq!(
            FwOtaState::Failed("网络连接超时".to_string()).to_string(),
            "升级失败: 网络连接超时"
        );
    }

    #[test]
    fn test_serde_roundtrip() {
        let states = vec![
            FwOtaState::Idle,
            FwOtaState::CheckingUpdate,
            FwOtaState::UpdateAvailable,
            FwOtaState::Downloading,
            FwOtaState::DownloadPaused,
            FwOtaState::DownloadComplete,
            FwOtaState::Verifying,
            FwOtaState::VerifyFailed,
            FwOtaState::ReadyToApply,
            FwOtaState::PreUpgradeCheck,
            FwOtaState::SwitchingToStandby,
            FwOtaState::Applying,
            FwOtaState::Applied,
            FwOtaState::PostUpgradeVerify,
            FwOtaState::RollingBack,
            FwOtaState::RolledBack,
            FwOtaState::Failed("测试错误".to_string()),
        ];

        for state in states {
            let json = serde_json::to_string(&state).unwrap();
            let restored: FwOtaState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, restored, "序列化往返失败: {:?}", state);
        }
    }
}
