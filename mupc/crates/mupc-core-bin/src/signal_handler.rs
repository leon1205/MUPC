//! 信号处理模块
//!
//! 监听操作系统信号 (SIGTERM/SIGINT)，触发优雅退出流程。
//! 仅在 Unix 平台 (Linux) 上编译 SIGTERM 路径。

/// 等待操作系统终止信号
///
/// 在收到 SIGINT (Ctrl+C) 或 SIGTERM (systemd stop) 后返回。
/// 调用方应在返回后进入 Phase 6 优雅退出流程。
pub async fn wait_for_shutdown() {
    #[cfg(unix)]
    let sigterm = {
        let signal = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
        async {
            match signal {
                Ok(mut s) => {
                    s.recv().await;
                    tracing::info!("收到 SIGTERM，开始优雅退出...");
                }
                Err(e) => {
                    tracing::warn!("无法注册 SIGTERM 处理器: {}，仅监听 SIGINT", e);
                    std::future::pending::<()>().await;
                }
            }
        }
    };

    #[cfg(not(unix))]
    let sigterm = std::future::pending::<()>();

    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("无法注册 SIGINT 处理器");
        tracing::info!("收到 SIGINT (Ctrl+C)，开始优雅退出...");
    };

    tokio::select! {
        _ = ctrl_c => {},
        _ = sigterm => {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wait_for_shutdown_can_be_called() {
        // 验证函数可被调用且不会立即返回（会一直等待信号）
        // 使用 timeout 确保测试不会永远挂起
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            wait_for_shutdown(),
        )
        .await;
        assert!(result.is_err()); // timeout 表示函数正在等待信号
    }
}
