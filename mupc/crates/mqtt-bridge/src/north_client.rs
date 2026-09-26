//! 北向 emqx MQTT 客户端
//!
//! 连接 emqx (可配置地址:8883)，用于北向通信
//! 支持 TLS + 双向证书认证（**fail-closed**，§9.3.5）
//!
//! # 建连前置（01 设计 §9.3.5 TLS-2）
//!
//! 1. **broker 必须非空且可解析为 `host:port`**（缺省空串 ⇒ `ConnectionFailed`）；
//! 2. **明文只走 `allow_plaintext=true` 这一条分支**：`false` 时读三件证书，任一件
//!    读不到 ⇒ `CertificateError`（**不重试明文**）；除 `allow_plaintext` 之外，
//!    本文件**不存在**任何明文分支；
//! 3. 凭据（username/password）经 `set_credentials` 交 rumqttc，**只进协议栈、不进日志**
//!    （`NorthMqttConfig::Debug` 已掩码，本节不再打印 `MqttOptions`）。

use crate::config::NorthMqttConfig;
use crate::error::MqttBridgeError;
use async_trait::async_trait;
use device_trait::errors::PluginError;
use device_trait::MqttBridge;
use rumqttc::{AsyncClient, Event, EventLoop, MqttOptions, Packet, QoS, Transport};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

/// 证书到期信息的**告警门限**（天）：§9.3.5 TLS-3 / §9.8 Q8（默认 30 天）。
pub const CERT_EXPIRY_WARN_DAYS: i64 = 30;

/// `notAfter` 与当前时刻的差距（TLS-3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CertExpiry {
    /// 证书 `notAfter`（Unix 秒）。
    pub not_after_unix: i64,
    /// 剩余天数（**向下取整**；已过期 ⇒ 负值）。
    pub remaining_days: i64,
}

impl CertExpiry {
    /// 是否应发"证书将到期"告警（剩余 < 30 天，含已过期）。
    pub fn should_warn(&self) -> bool {
        self.remaining_days < CERT_EXPIRY_WARN_DAYS
    }
}

/// 北向 MQTT 客户端实现
pub struct NorthMqttClient {
    client: AsyncClient,
    eventloop: Arc<Mutex<EventLoop>>,
    connected: Arc<AtomicBool>,
    reconnect_config: crate::config::ReconnectConfig,
    /// 客户端证书路径（TLS-3 到期监控用）。明文模式（`allow_plaintext=true`）⇒ `None`。
    client_cert_path: Option<PathBuf>,
    /// 明文模式标记（供装配层发"响亮告警"，§9.3.5 TLS-4）。
    allow_plaintext: bool,
}

impl NorthMqttClient {
    /// 创建新的 NorthMqttClient（**不发起任何网络连接**：`AsyncClient::new` 只建客户端与
    /// 事件循环，握手发生在 [`Self::run`] / [`Self::process_events`] 的 `poll` 中）。
    pub fn new(config: &NorthMqttConfig) -> Result<Self, MqttBridgeError> {
        // ① broker 必须已配置（空串 = `::default()` 的缺陷形态 ⇒ 明确拒绝，不猜测地址）
        if !config.broker_is_configured() {
            return Err(MqttBridgeError::ConnectionFailed(
                "broker_addr 为空（mqtt_bridge.north.broker 未配置）——拒绝建连".to_string(),
            ));
        }
        let (host, port) = parse_broker(&config.broker_addr).ok_or_else(|| {
            MqttBridgeError::ConnectionFailed(format!(
                "broker_addr 非法（须为 host:port，port ∈ 1..=65535）: {}",
                config.broker_addr
            ))
        })?;

        let mut mqtt_options = MqttOptions::new(config.client_id.clone(), host, port);
        mqtt_options.set_keep_alive(std::time::Duration::from_secs(config.keepalive_secs));
        // 北向通信需要持久化会话，支持断线重连后恢复
        mqtt_options.set_clean_session(false);
        // 凭据：只进协议栈。**不**打印（`MqttOptions` 的 Debug 含明文凭据）。
        if let Some(user) = config.username.as_deref() {
            let pass = config.password.clone().unwrap_or_default();
            mqtt_options.set_credentials(user, pass);
        }

        // ② 传输层：**只有** `allow_plaintext=true` 才允许明文（TLS-2/TLS-4/Q9）
        let client_cert_path = if config.allow_plaintext {
            mqtt_options.set_transport(Transport::tcp());
            None
        } else {
            let ca = read_cert(&config.tls.ca_cert, "CA 证书")?;
            let client_cert = read_cert(&config.tls.client_cert, "客户端证书")?;
            let client_key = read_cert(&config.tls.client_key, "客户端密钥")?;
            mqtt_options.set_transport(Transport::tls(ca, Some((client_cert, client_key)), None));
            Some(config.tls.client_cert.clone())
        };

        let (client, eventloop) = AsyncClient::new(mqtt_options, 100);

        Ok(Self {
            client,
            eventloop: Arc::new(Mutex::new(eventloop)),
            // 与 `LocalMqttClient` 对齐：**未完成握手前不得自称已连接**（2026-09-20 评审修复）。
            connected: Arc::new(AtomicBool::new(false)),
            reconnect_config: config.reconnect.clone(),
            client_cert_path,
            allow_plaintext: config.allow_plaintext,
        })
    }

    /// 是否处于明文模式（§9.3.5 TLS-4：装配层据此打 ERROR 日志 + 产 major 事件）。
    pub fn is_plaintext(&self) -> bool {
        self.allow_plaintext
    }

    /// 客户端证书到期信息（§9.3.5 TLS-3）。
    ///
    /// - `Ok(None)`：明文模式（无证书，不适用）；
    /// - `Ok(Some(x))`：解析成功（`remaining_days` 已算好，供门限判定）；
    /// - `Err`：读文件/解析失败 —— **调用方 WARN 后继续**（解析失败**不阻断连接**）。
    pub fn cert_expiry(&self, now_unix: i64) -> Result<Option<CertExpiry>, String> {
        let Some(path) = self.client_cert_path.as_ref() else {
            return Ok(None);
        };
        let pem = std::fs::read(path)
            .map_err(|e| format!("读取客户端证书失败({}): {e}", path.display()))?;
        let not_after_unix = parse_cert_not_after(&pem)?;
        Ok(Some(CertExpiry {
            not_after_unix,
            // 向下取整到"整天"：剩余 29.4 天 ⇒ 29（门限判定取保守侧）
            remaining_days: (not_after_unix - now_unix).div_euclid(86_400),
        }))
    }

    /// 处理事件循环，更新连接状态
    pub async fn process_events(&self) -> Result<(), MqttBridgeError> {
        let mut eventloop = self.eventloop.lock().await;
        match eventloop.poll().await {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                self.connected.store(true, Ordering::SeqCst);
                Ok(())
            }
            Ok(Event::Incoming(Packet::Disconnect)) => {
                self.connected.store(false, Ordering::SeqCst);
                Err(MqttBridgeError::Disconnected("连接断开".to_string()))
            }
            Ok(_) => Ok(()),
            Err(e) => {
                self.connected.store(false, Ordering::SeqCst);
                Err(MqttBridgeError::ConnectionFailed(e.to_string()))
            }
        }
    }

    /// 运行事件循环，支持自动重连
    /// 按照 reconnect_config 的策略进行指数退避重连
    pub async fn run(&self) -> Result<(), MqttBridgeError> {
        let mut interval_secs = self.reconnect_config.initial_interval_secs;

        loop {
            match self.process_events().await {
                Ok(_) => {
                    interval_secs = self.reconnect_config.initial_interval_secs;
                }
                Err(e) => {
                    tracing::warn!("MQTT 北向连接错误: {}, 等待 {} 秒后重连", e, interval_secs);
                    sleep(Duration::from_secs(interval_secs)).await;

                    let max_interval = self.reconnect_config.max_interval_secs;
                    interval_secs = ((interval_secs as f64)
                        * self.reconnect_config.backoff_multiplier)
                        .min(max_interval as f64) as u64;
                    interval_secs = interval_secs.max(1);
                }
            }
        }
    }

    /// 启动后台任务处理事件（不阻塞）
    pub fn start_event_loop(&self) {
        let eventloop = Arc::clone(&self.eventloop);
        let connected = Arc::clone(&self.connected);

        tokio::spawn(async move {
            let mut _interval_secs = 1u64;

            loop {
                let mut el = eventloop.lock().await;
                match el.poll().await {
                    Ok(Event::Incoming(Packet::ConnAck(_))) => {
                        connected.store(true, Ordering::SeqCst);
                        _interval_secs = 1;
                    }
                    Ok(Event::Incoming(Packet::Disconnect)) => {
                        connected.store(false, Ordering::SeqCst);
                    }
                    Ok(_) => {}
                    Err(_) => {
                        connected.store(false, Ordering::SeqCst);
                    }
                }
            }
        });
    }
}

#[async_trait]
impl MqttBridge for NorthMqttClient {
    async fn connect(&mut self) -> Result<(), PluginError> {
        // 同 `LocalMqttClient::connect`：不置位 `connected`（真值由事件循环在 ConnAck/断开
        // 时维护）。此前直接置 true 会让"broker 不可达"被报成在线（2026-09-20 评审修复）。
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), PluginError> {
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }

    async fn publish(&self, topic: &str, payload: &[u8], qos: u8) -> Result<(), PluginError> {
        let qos = match qos {
            0 => QoS::AtMostOnce,
            1 => QoS::AtLeastOnce,
            _ => QoS::ExactlyOnce,
        };
        self.client
            .publish(topic, qos, false, payload)
            .await
            .map_err(|e| PluginError::Other(format!("发布失败: {}", e)))
    }

    async fn subscribe(&self, topic: &str, qos: u8) -> Result<(), PluginError> {
        let qos = match qos {
            0 => QoS::AtMostOnce,
            1 => QoS::AtLeastOnce,
            _ => QoS::ExactlyOnce,
        };
        self.client
            .subscribe(topic, qos)
            .await
            .map_err(|e| PluginError::Other(format!("订阅失败: {}", e)))
    }

    /// 是否已与 broker 完成握手。
    ///
    /// ⚠️ 用 `AtomicBool` 而非 `tokio::sync::Mutex<bool>`：后者只能 `blocking_lock()`（在任何
    /// 运行时线程内调用即 panic「Cannot block the current thread from within a runtime」），
    /// 而本方法是**同步 trait 方法**、必须可被任意上下文调用（2026-09-20 评审修复）。
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn name(&self) -> &'static str {
        "NorthMqttClient"
    }
}

/// `"host:port"` 解析（§9.3.2 的同一条口径：port ∈ 1..=65535）。
///
/// IPv6 字面量（`[::1]:1883`）按"最后一个 `:` 分隔"处理，故 `[::1]:1883` 可用；
/// 无端口 / 端口非数字 / 端口 0 / host 为空 ⇒ `None`（调用方拒建连）。
pub fn parse_broker(broker_addr: &str) -> Option<(String, u16)> {
    let (host, port) = broker_addr.trim().rsplit_once(':')?;
    let host = host.trim();
    if host.is_empty() {
        return None;
    }
    let port: u16 = port.trim().parse().ok()?;
    if port == 0 {
        return None;
    }
    Some((host.to_string(), port))
}

/// 读证书文件（**fail-closed 的读侧**：读不到即 `CertificateError`，调用方不回落明文）。
fn read_cert(path: &std::path::Path, what: &str) -> Result<Vec<u8>, MqttBridgeError> {
    if path.as_os_str().is_empty() {
        return Err(MqttBridgeError::CertificateError(format!(
            "{what} 路径为空（allow_plaintext=false 时三件证书必须齐备）"
        )));
    }
    std::fs::read(path).map_err(|e| {
        MqttBridgeError::CertificateError(format!("读取{what}失败({}): {e}", path.display()))
    })
}

/// 解析 PEM 证书的 `notAfter`（Unix 秒，§9.3.5 TLS-3）。
///
/// 依赖 `x509-parser`（workspace 既有 **0.16**，与 `security` 同版本，避免重复版本树）。
/// 解析失败 ⇒ `Err(String)`（**调用方 WARN 后继续**，不阻断连接）。
pub fn parse_cert_not_after(pem_bytes: &[u8]) -> Result<i64, String> {
    let (_, pem) =
        x509_parser::pem::parse_x509_pem(pem_bytes).map_err(|e| format!("PEM 解析失败: {e:?}"))?;
    let cert = pem
        .parse_x509()
        .map_err(|e| format!("X.509 解析失败: {e:?}"))?;
    Ok(cert.validity().not_after.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TlsConfig;
    use std::path::PathBuf;

    /// 自签测试证书（**10 年有效期**，2026-09-24 生成；仅用于解析/到期判定单测，
    /// **仓库内不含其私钥**）。`notBefore` = `2026-09-24T12:48:19Z`、
    /// `notAfter` = **`2036-09-21T12:48:19Z`**（epoch **2105614099**）。
    const TEST_CERT_PEM: &str = include_str!("../tests/fixtures/test_cert.pem");

    #[test]
    fn test_reconnect_config_default() {
        let config = crate::config::ReconnectConfig::default();
        assert_eq!(config.initial_interval_secs, 1);
        assert_eq!(config.max_interval_secs, 60);
        assert_eq!(config.backoff_multiplier, 2.0);
    }

    /// **CFG-2 / C-11 的运行期把关**：空 broker 不得建连（不得回落到任何默认地址）。
    #[test]
    fn empty_broker_is_rejected() {
        let cfg = NorthMqttConfig {
            enabled: true,
            ..Default::default()
        };
        match NorthMqttClient::new(&cfg) {
            Err(MqttBridgeError::ConnectionFailed(m)) => {
                assert!(m.contains("broker_addr"), "错误须点名 broker_addr：{m}");
            }
            other => panic!("空 broker 必须拒建连，实得 {:?}", other.map(|_| "Ok")),
        }
    }

    /// **fail-closed（TLS-2）**：`allow_plaintext=false` 且证书路径为空/不存在 ⇒
    /// `CertificateError`（**不得**回落明文，也不得 panic）。
    #[test]
    fn tls_without_readable_certs_is_rejected() {
        for (label, tls) in [
            (
                "三路径皆空",
                TlsConfig {
                    ca_cert: PathBuf::new(),
                    client_cert: PathBuf::new(),
                    client_key: PathBuf::new(),
                },
            ),
            (
                "路径指向不存在的文件",
                TlsConfig {
                    ca_cert: PathBuf::from("Z:/definitely/not/here/ca.crt"),
                    client_cert: PathBuf::from("Z:/definitely/not/here/client.crt"),
                    client_key: PathBuf::from("Z:/definitely/not/here/client.key"),
                },
            ),
        ] {
            let cfg = NorthMqttConfig {
                enabled: true,
                broker_addr: "127.0.0.1:8883".into(),
                client_id: "mupc-test".into(),
                allow_plaintext: false,
                tls,
                ..Default::default()
            };
            match NorthMqttClient::new(&cfg) {
                Err(MqttBridgeError::CertificateError(_)) => {}
                other => panic!(
                    "{label}：必须 CertificateError（fail-closed），实得 {:?}",
                    other.map(|_| "Ok")
                ),
            }
        }
    }

    /// **明文分支的唯一性**：`allow_plaintext=true` 时**不读证书**（路径故意指向不存在文件
    /// 也照样建连成功）⇒ 反证"明文分支不看证书"，且该分支由显式开关独占。
    #[test]
    fn plaintext_branch_ignores_certs_and_succeeds() {
        let cfg = NorthMqttConfig {
            enabled: true,
            broker_addr: "127.0.0.1:1883".into(),
            client_id: "mupc-test".into(),
            allow_plaintext: true,
            tls: TlsConfig {
                ca_cert: PathBuf::from("Z:/nope/ca.crt"),
                client_cert: PathBuf::from("Z:/nope/client.crt"),
                client_key: PathBuf::from("Z:/nope/client.key"),
            },
            ..Default::default()
        };
        let c = NorthMqttClient::new(&cfg).expect("明文模式不得读证书");
        assert!(c.is_plaintext());
        assert!(!c.is_connected(), "新建客户端不得自称已连接（握手未发生）");
        assert_eq!(
            c.cert_expiry(0).expect("明文模式无证书 ⇒ Ok(None)"),
            None,
            "明文模式无证书到期信息"
        );
    }

    #[test]
    fn broker_parsing_rules() {
        assert_eq!(
            parse_broker("mqtt.example.com:8883"),
            Some(("mqtt.example.com".to_string(), 8883))
        );
        assert_eq!(
            parse_broker("127.0.0.1:1883"),
            Some(("127.0.0.1".into(), 1883))
        );
        assert_eq!(parse_broker("[::1]:1883"), Some(("[::1]".into(), 1883)));
        // 非空但不可解析 ⇒ None（调用方拒）
        assert_eq!(parse_broker(""), None);
        assert_eq!(parse_broker("mqtt.example.com"), None, "缺端口");
        assert_eq!(parse_broker(":8883"), None, "缺 host");
        assert_eq!(parse_broker("host:0"), None, "端口 0 非法");
        assert_eq!(parse_broker("host:notaport"), None);
    }

    /// **TLS-3 证书到期解析**：真实自签证书 `notAfter` 可解析；剩余天数按注入的
    /// `now` 计算（保持用例与真实时间无关）。
    #[test]
    fn cert_expiry_is_parsed_from_real_pem() {
        let not_after = parse_cert_not_after(TEST_CERT_PEM.as_bytes()).expect("自签证书须可解析");
        assert!(
            not_after > 1_700_000_000,
            "notAfter 应是合理的时间戳: {not_after}"
        );

        let dir = std::env::temp_dir().join(format!("mupc-cert-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cert_path = dir.join("client.crt");
        std::fs::write(&cert_path, TEST_CERT_PEM).unwrap();

        let cfg = NorthMqttConfig {
            enabled: true,
            broker_addr: "127.0.0.1:8883".into(),
            client_id: "mupc-test".into(),
            allow_plaintext: false,
            tls: TlsConfig {
                ca_cert: cert_path.clone(),
                client_cert: cert_path.clone(),
                client_key: cert_path.clone(),
            },
            ..Default::default()
        };
        let c = NorthMqttClient::new(&cfg).expect("证书可读 ⇒ 建连对象可构造");
        let exp = c.cert_expiry(not_after - 86_400 * 90).unwrap().unwrap();
        assert_eq!(exp.remaining_days, 90, "剩余天数 = (notAfter − now)/86400");
        assert!(!exp.should_warn(), "剩余 90 天 ≥ 30 ⇒ 不告警");
        let exp2 = c.cert_expiry(not_after - 86_400 * 29).unwrap().unwrap();
        assert_eq!(exp2.remaining_days, 29);
        assert!(
            exp2.should_warn(),
            "剩余 29 天 < 30 ⇒ 告警（Q8 门限 30 天）"
        );
        let exp3 = c.cert_expiry(not_after + 86_400).unwrap().unwrap();
        assert_eq!(exp3.remaining_days, -1);
        assert!(exp3.should_warn(), "已过期 ⇒ 必须告警");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 解析失败 ⇒ `Err`（装配层 WARN 后继续，**不阻断连接**；不得 panic）。
    #[test]
    fn cert_parse_failure_is_error_not_panic() {
        assert!(parse_cert_not_after(b"not a pem at all").is_err());
    }
}
