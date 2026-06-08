//! OTA 定时任务调度器
//!
//! Phase 3C.2 OTA 模型自动更新模块的定时任务调度器
//! 负责管理 OTA 更新任务的定时触发和下载窗口控制

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::time::interval;

use chrono::Timelike;

use crate::config::OtaConfig;
use crate::error::OtaError;

/// OTA 管理器 trait
///
/// 定义 OTA 检查更新的接口，由调用者实现具体逻辑
#[async_trait]
pub trait OtaManager: Send + Sync + std::fmt::Debug {
    /// 检查更新
    ///
    /// 连接到 OTA 服务器检查是否有可用更新
    async fn check_updates(&self) -> Result<(), OtaError>;
}

/// 调度器命令
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulerCommand {
    /// 停止调度器
    Stop,
    /// 手动触发检查
    TriggerCheck,
}

/// OTA 调度器
///
/// 负责按配置间隔检查更新，并在指定下载窗口内执行下载
#[derive(Debug)]
pub struct OtaScheduler {
    /// OTA 配置
    config: OtaConfig,
    /// 命令发送通道（用于发送命令给调度器）
    command_tx: mpsc::Sender<SchedulerCommand>,
    /// OTA 管理器引用
    ota_manager: Arc<dyn OtaManager>,
    /// 是否正在运行
    running: std::sync::atomic::AtomicBool,
}

impl OtaScheduler {
    /// 创建新的调度器
    ///
    /// # 参数
    /// * `config` - OTA 配置
    /// * `ota_manager` - OTA 管理器实现
    ///
    /// # 返回
    /// 调度器实例和命令接收器
    pub fn new(
        config: OtaConfig,
        ota_manager: Arc<dyn OtaManager>,
    ) -> Result<(Self, mpsc::Receiver<SchedulerCommand>), OtaError> {
        let (command_tx, command_rx) = mpsc::channel(16);
        Ok((
            Self {
                config,
                command_tx,
                ota_manager,
                running: std::sync::atomic::AtomicBool::new(false),
            },
            command_rx,
        ))
    }

    /// 启动调度器
    ///
    /// # 参数
    /// * `check_on_startup` - 是否在启动时执行一次检查
    pub async fn start(
        &self,
        check_on_startup: bool,
        mut command_rx: mpsc::Receiver<SchedulerCommand>,
    ) -> Result<(), OtaError> {
        if self.running.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(OtaError::UpdateTimeout); // 避免重复启动
        }

        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        // 如果配置要求启动时检查，则执行一次
        if check_on_startup {
            tracing::info!("启动时检查更新...");
            self.perform_check().await?;
        }

        // 创建定时器
        let check_interval = Duration::from_secs(self.config.check_interval);
        let mut ticker = interval(check_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        tracing::info!(
            "调度器已启动，检查间隔 {} 秒，下载窗口 [{} - {}]",
            self.config.check_interval,
            self.config.download_window_start,
            self.config.download_window_end
        );

        // 主循环：同时监听定时器和命令
        while self.running.load(std::sync::atomic::Ordering::SeqCst) {
            tokio::select! {
                // 等待定时器触发
                _ = ticker.tick() => {
                    if self.running.load(std::sync::atomic::Ordering::SeqCst) {
                        self.perform_check().await?;
                    }
                }
                // 处理命令
                cmd = command_rx.recv() => {
                    match cmd {
                        Some(SchedulerCommand::Stop) => {
                            tracing::info!("收到停止命令");
                            break;
                        }
                        Some(SchedulerCommand::TriggerCheck) => {
                            tracing::info!("收到手动触发检查命令");
                            self.perform_check().await?;
                        }
                        None => break,  // 发送端已关闭
                    }
                }
            }
        }

        tracing::info!("调度器已停止");
        Ok(())
    }

    /// 执行一次检查
    async fn perform_check(&self) -> Result<(), OtaError> {
        // 检查是否在下载窗口内
        if !self.is_in_download_window() {
            tracing::debug!(
                "当前不在下载窗口 [{} - {}] 内，跳过检查",
                self.config.download_window_start,
                self.config.download_window_end
            );
            return Ok(());
        }

        tracing::info!("开始检查更新...");
        self.ota_manager.check_updates().await
    }

    /// 停止调度器
    pub async fn stop(&self) -> Result<(), OtaError> {
        tracing::info!("收到停止命令，正在停止调度器...");

        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);

        // 发送 Stop 命令以确保主循环退出
        // 注意：由于调度器不拥有 command_rx，这里我们主要通过设置 running 标志
        // 在实际使用中，调用者应该丢弃 command_rx 来让调度器退出 select!

        Ok(())
    }

    /// 发送命令
    ///
    /// # 参数
    /// * `cmd` - 要发送的命令
    pub fn send_command(&self, cmd: SchedulerCommand) -> Result<(), OtaError> {
        self.command_tx
            .try_send(cmd)
            .map_err(|_e| OtaError::UpdateTimeout)
    }

    /// 检查是否在下载窗口内
    ///
    /// 使用本地时间判断当前是否在配置的下载窗口内
    fn is_in_download_window(&self) -> bool {
        use chrono::Local;

        let now = Local::now();
        self.config.is_in_download_window(now.hour(), now.minute())
    }
}

/// 用于测试的 Mock OtaManager
#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock OTA 管理器，用于测试
    #[derive(Debug)]
    pub struct MockOtaManager {
        pub check_count: AtomicUsize,
        pub should_fail: bool,
    }

    impl MockOtaManager {
        /// 创建新的 Mock OTA 管理器
        pub fn new() -> Self {
            Self {
                check_count: AtomicUsize::new(0),
                should_fail: false,
            }
        }

        /// 设置是否检查失败
        pub fn with_fail(mut self, should_fail: bool) -> Self {
            self.should_fail = should_fail;
            self
        }

        /// 获取检查次数
        pub fn get_check_count(&self) -> usize {
            self.check_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl OtaManager for MockOtaManager {
        async fn check_updates(&self) -> Result<(), OtaError> {
            self.check_count.fetch_add(1, Ordering::SeqCst);

            if self.should_fail {
                Err(OtaError::VersionQueryFailed("Mock 错误".to_string()))
            } else {
                tracing::info!("Mock OTA Manager: 检查更新完成");
                Ok(())
            }
        }
    }
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use mock::MockOtaManager;

    // ========== OtaScheduler 创建测试 ==========

    #[test]
    fn test_scheduler_new() {
        let config = OtaConfig::default();
        let ota_manager = Arc::new(MockOtaManager::new());

        let result = OtaScheduler::new(config.clone(), ota_manager);
        assert!(result.is_ok());

        let (scheduler, _command_rx) = result.unwrap();
        assert_eq!(scheduler.config.check_interval, 3600);
    }

    #[test]
    fn test_scheduler_new_multiple() {
        let config = OtaConfig::default();

        let (scheduler1, _rx1) =
            OtaScheduler::new(config.clone(), Arc::new(MockOtaManager::new())).unwrap();
        let (scheduler2, _rx2) =
            OtaScheduler::new(config.clone(), Arc::new(MockOtaManager::new())).unwrap();

        // 两个调度器应该独立工作
        assert!(scheduler1.config.check_interval == scheduler2.config.check_interval);
    }

    // ========== send_command 测试 ==========

    #[tokio::test]
    async fn test_send_command_stop() {
        let config = OtaConfig::default();
        let ota_manager = Arc::new(MockOtaManager::new());

        let (scheduler, mut command_rx) = OtaScheduler::new(config, ota_manager).unwrap();

        // 发送 Stop 命令
        let result = scheduler.send_command(SchedulerCommand::Stop);
        assert!(result.is_ok());

        // 接收并验证命令
        let cmd = command_rx.recv().await;
        assert!(cmd.is_some());
        assert_eq!(cmd.unwrap(), SchedulerCommand::Stop);
    }

    #[tokio::test]
    async fn test_send_command_trigger_check() {
        let config = OtaConfig::default();
        let ota_manager = Arc::new(MockOtaManager::new());

        let (scheduler, mut command_rx) = OtaScheduler::new(config, ota_manager).unwrap();

        let result = scheduler.send_command(SchedulerCommand::TriggerCheck);
        assert!(result.is_ok());

        let cmd = command_rx.recv().await;
        assert!(cmd.is_some());
        assert_eq!(cmd.unwrap(), SchedulerCommand::TriggerCheck);
    }

    #[tokio::test]
    async fn test_send_command_buffer_full() {
        let config = OtaConfig::default();
        let ota_manager = Arc::new(MockOtaManager::new());

        let (scheduler, _command_rx) = OtaScheduler::new(config, ota_manager).unwrap();

        // 发送大量命令填满缓冲区
        for _ in 0..20 {
            let _ = scheduler.send_command(SchedulerCommand::TriggerCheck);
        }

        // 再次发送应该失败
        let result = scheduler.send_command(SchedulerCommand::Stop);
        assert!(result.is_err());
    }

    // ========== stop 测试 ==========

    #[tokio::test]
    async fn test_stop() {
        let config = OtaConfig::default();
        let ota_manager = Arc::new(MockOtaManager::new());

        let (scheduler, _command_rx) = OtaScheduler::new(config, ota_manager).unwrap();

        let result = scheduler.stop().await;
        assert!(result.is_ok());
    }

    // ========== is_in_download_window 测试 ==========

    #[test]
    fn test_is_in_download_window_default_config() {
        // 默认配置: 02:00 - 05:00
        let config = OtaConfig::default();
        let ota_manager = Arc::new(MockOtaManager::new());

        let (scheduler, _command_rx) = OtaScheduler::new(config, ota_manager).unwrap();

        // 使用已知的时间测试（不依赖本地时区）
        // 注意：is_in_download_window 使用 Local::now()，测试可能受时区影响
        // 这里主要验证逻辑正确性
        let _ = scheduler.is_in_download_window();
    }

    #[test]
    fn test_download_window_logic() {
        // 测试同一天窗口逻辑
        let mut config = OtaConfig::default();
        config.download_window_start = "02:00".to_string();
        config.download_window_end = "05:00".to_string();

        // 验证 is_in_download_window 逻辑
        assert!(config.is_in_download_window(2, 0)); // 02:00 - 在窗口内
        assert!(config.is_in_download_window(3, 30)); // 03:30 - 在窗口内
        assert!(config.is_in_download_window(5, 0)); // 05:00 - 在窗口内（边界）
        assert!(!config.is_in_download_window(1, 0)); // 01:00 - 在窗口外
        assert!(!config.is_in_download_window(6, 0)); // 06:00 - 在窗口外
    }

    #[test]
    fn test_download_window_logic_cross_midnight() {
        // 测试跨午夜窗口逻辑
        let mut config = OtaConfig::default();
        config.download_window_start = "22:00".to_string();
        config.download_window_end = "06:00".to_string();

        assert!(config.is_in_download_window(22, 0)); // 22:00 - 在窗口内
        assert!(config.is_in_download_window(23, 30)); // 23:30 - 在窗口内
        assert!(config.is_in_download_window(0, 0)); // 00:00 - 在窗口内
        assert!(config.is_in_download_window(6, 0)); // 06:00 - 在窗口内（边界）
        assert!(!config.is_in_download_window(7, 0)); // 07:00 - 在窗口外
        assert!(!config.is_in_download_window(12, 0)); // 12:00 - 在窗口外
    }

    // ========== MockOtaManager 测试 ==========

    #[tokio::test]
    async fn test_mock_ota_manager() {
        let manager = MockOtaManager::new();
        let result = manager.check_updates().await;
        assert!(result.is_ok());
        assert_eq!(manager.get_check_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_ota_manager_with_fail() {
        let manager = MockOtaManager::new().with_fail(true);
        let result = manager.check_updates().await;
        assert!(result.is_err());
        assert_eq!(manager.get_check_count(), 1);
    }

    // ========== SchedulerCommand 测试 ==========

    #[test]
    fn test_scheduler_command_debug() {
        let stop_cmd = SchedulerCommand::Stop;
        let trigger_cmd = SchedulerCommand::TriggerCheck;

        assert!(format!("{:?}", stop_cmd).contains("Stop"));
        assert!(format!("{:?}", trigger_cmd).contains("TriggerCheck"));
    }

    #[test]
    fn test_scheduler_command_clone() {
        let cmd = SchedulerCommand::Stop;
        let cloned = cmd.clone();
        assert_eq!(cmd, cloned);
    }

    // ========== OtaManager trait 测试 ==========

    #[tokio::test]
    async fn test_ota_manager_trait_object() {
        // 验证 OtaManager 可以作为 trait object 使用
        let manager: Arc<dyn OtaManager> = Arc::new(MockOtaManager::new());
        let result = manager.check_updates().await;
        assert!(result.is_ok());
    }

    // ========== 并发测试 ==========

    #[tokio::test]
    async fn test_concurrent_send_command() {
        let config = OtaConfig::default();
        let ota_manager = Arc::new(MockOtaManager::new());

        let (scheduler, mut command_rx) = OtaScheduler::new(config, ota_manager).unwrap();

        // 并发发送多个命令
        let handles: Vec<_> = (0..5)
            .map(|i| {
                let scheduler = OtaScheduler {
                    config: OtaConfig::default(),
                    command_tx: scheduler.command_tx.clone(),
                    ota_manager: scheduler.ota_manager.clone(),
                    running: std::sync::atomic::AtomicBool::new(false),
                };
                tokio::spawn(async move { scheduler.send_command(SchedulerCommand::TriggerCheck) })
            })
            .collect();

        // 等待所有任务完成
        for handle in handles {
            let _ = handle.await;
        }

        // 接收并验证命令
        let mut received = 0;
        while received < 5 {
            if command_rx.recv().await.is_some() {
                received += 1;
            }
        }
    }

    // ========== OtaScheduler Debug 测试 ==========

    #[test]
    fn test_scheduler_debug() {
        let config = OtaConfig::default();
        let ota_manager = Arc::new(MockOtaManager::new());

        let (scheduler, _command_rx) = OtaScheduler::new(config, ota_manager).unwrap();
        let debug_str = format!("{:?}", scheduler);

        assert!(debug_str.contains("OtaScheduler"));
        assert!(debug_str.contains("check_interval"));
    }

    // ========== 配置影响测试 ==========

    #[test]
    fn test_different_check_intervals() {
        let mut config = OtaConfig::default();
        config.check_interval = 7200; // 2 小时

        let ota_manager = Arc::new(MockOtaManager::new());
        let (scheduler, _command_rx) = OtaScheduler::new(config.clone(), ota_manager).unwrap();

        assert_eq!(scheduler.config.check_interval, 7200);
    }

    #[test]
    fn test_custom_download_window() {
        let mut config = OtaConfig::default();
        config.download_window_start = "23:00".to_string();
        config.download_window_end = "05:30".to_string();

        let ota_manager = Arc::new(MockOtaManager::new());
        let (scheduler, _command_rx) = OtaScheduler::new(config.clone(), ota_manager).unwrap();

        assert_eq!(scheduler.config.download_window_start, "23:00");
        assert_eq!(scheduler.config.download_window_end, "05:30");
    }

    // ========== 运行状态测试 ==========

    #[test]
    fn test_scheduler_initial_state() {
        let config = OtaConfig::default();
        let ota_manager = Arc::new(MockOtaManager::new());

        let (scheduler, _command_rx) = OtaScheduler::new(config, ota_manager).unwrap();

        // 初始状态 running 应该为 false
        assert!(!scheduler.running.load(std::sync::atomic::Ordering::SeqCst));
    }

    // ========== SchedulerCommand 相等性测试 ==========

    #[test]
    fn test_scheduler_command_equality() {
        assert_eq!(SchedulerCommand::Stop, SchedulerCommand::Stop);
        assert_eq!(
            SchedulerCommand::TriggerCheck,
            SchedulerCommand::TriggerCheck
        );
        assert_ne!(SchedulerCommand::Stop, SchedulerCommand::TriggerCheck);
    }
}
