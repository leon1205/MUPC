use crate::config::SimBridgeConfig;
use crate::error::SimBridgeError;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum SimRequest {
    #[serde(rename = "reset")]
    Reset { scenario: String },
    #[serde(rename = "step")]
    Step { p_ref: f64, k_droop: f64 },
    #[serde(rename = "shutdown")]
    Shutdown,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum SimResponse {
    #[serde(rename = "obs")]
    Observation {
        data: Vec<f32>,
        reward: f64,
        done: bool,
        #[serde(default)]
        info: serde_json::Value,
    },
    #[serde(rename = "shutdown_ack")]
    ShutdownAck,
    #[serde(rename = "error")]
    Error { msg: String },
}

pub struct PyEngine {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout_lines: tokio::io::Lines<BufReader<ChildStdout>>,
    restart_count: u32,
    max_restarts: u32,
}

impl PyEngine {
    pub async fn spawn(config: &SimBridgeConfig) -> Result<Self, SimBridgeError> {
        let mut child = Command::new(&config.python_cmd)
            .arg(&config.engine_script)
            .arg("--voltage-sim")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| SimBridgeError::PyEngine(format!("spawn 失败: {}", e)))?;

        let stdin = BufWriter::new(
            child.stdin.take()
                .ok_or_else(|| SimBridgeError::PyEngine("stdin 未 piped".into()))?
        );
        let stdout = BufReader::new(
            child.stdout.take()
                .ok_or_else(|| SimBridgeError::PyEngine("stdout 未 piped".into()))?
        );
        let stdout_lines = stdout.lines();

        tracing::info!("Python 引擎已启动: PID={}", child.id().unwrap_or(0));
        Ok(Self {
            child,
            stdin,
            stdout_lines,
            restart_count: 0,
            max_restarts: 3,
        })
    }

    async fn send_request(&mut self, req: &SimRequest) -> Result<SimResponse, SimBridgeError> {
        let line = serde_json::to_string(req)? + "\n";
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| SimBridgeError::PyEngine(format!("stdin write: {}", e)))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| SimBridgeError::PyEngine(format!("stdin flush: {}", e)))?;

        let response_line = timeout(
            Duration::from_secs(5),
            self.stdout_lines.next_line(),
        )
        .await
        .map_err(|_| SimBridgeError::PyEngineTimeout)?
        .map_err(|e| SimBridgeError::PyEngine(format!("stdout read: {}", e)))?
        .ok_or(SimBridgeError::PyEngineEof)?;

        serde_json::from_str(&response_line).map_err(|e| {
            SimBridgeError::Protocol(format!("JSONL 解析失败: {} (raw: {})", e, response_line))
        })
    }

    pub async fn send_step(
        &mut self,
        p_ref: f64,
        k_droop: f64,
    ) -> Result<SimResponse, SimBridgeError> {
        self.send_request(&SimRequest::Step { p_ref, k_droop }).await
    }

    pub async fn send_reset(&mut self, scenario: &str) -> Result<SimResponse, SimBridgeError> {
        self.send_request(&SimRequest::Reset {
            scenario: scenario.to_string(),
        })
        .await
    }

    pub async fn send_shutdown(&mut self) -> Result<(), SimBridgeError> {
        if let Err(e) = self.send_request(&SimRequest::Shutdown).await {
            tracing::warn!("Python shutdown 请求发送失败: {}", e);
        }
        // 带超时等待子进程退出，超时后强制 kill
        let _ = timeout(Duration::from_secs(5), self.child.wait()).await;
        if self.child.try_wait().ok().flatten().is_none() {
            tracing::warn!("Python 引擎未响应 shutdown, 强制 kill");
            let _ = self.child.kill().await;
        }
        Ok(())
    }

    pub async fn restart(&mut self, config: &SimBridgeConfig) -> Result<(), SimBridgeError> {
        if self.restart_count >= self.max_restarts {
            return Err(SimBridgeError::PyEngineMaxRestarts(self.max_restarts));
        }
        tracing::warn!(
            "Python 引擎崩溃，重启中 ({}/{})",
            self.restart_count + 1,
            self.max_restarts
        );
        let _ = self.child.kill().await;
        let new = Self::spawn(config).await?;
        self.child = new.child;
        self.stdin = new.stdin;
        self.stdout_lines = new.stdout_lines;
        self.restart_count += 1;
        Ok(())
    }
}
