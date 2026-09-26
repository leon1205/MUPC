//! MQTT 配置结构（01 设计 §9.3.2 / §9.3.5；C-11 / C-12）
//!
//! # 本层的定位（C-12：**分层映射**）
//!
//! YAML(`core_config.mqtt_bridge`) → Rust(`mupc-core-bin::core_config::MqttBridgeConfig`)
//! → **本 crate 的 [`NorthMqttConfig`] / [`LocalMqttConfig`]**（装配期逐字段映射，
//! 见 `mupc-core-bin::uplink::north_client_config`）。本节结构**不重复** YAML 的
//! 周期/缓存字段（那些由生产者侧解释），只承载**客户端建连所需**的量。
//!
//! # `Default` 的**安全**口径（C-11 / CFG-2）
//!
//! `NorthMqttConfig::default()` 的 `broker_addr` 与 `client_id` 为**空串**、三条证书
//! 路径为**空路径**。理由：`::default()` 曾是缺陷源——它指向 `mqtt.example.com:8883`
//! 与 dummy 证书（原 `:56-63`），任何"漏改的默认路径"都会**真连假域名**。
//! 空串在本 crate 的建连路径上**必然失败**（`NorthMqttClient::new` 报
//! `ConnectionFailed`/`CertificateError`），而不是"连到一个看似合法的地址"。
//!
//! ⚠️ `test_config()`（把 `default()` 当测试替身）**已删除**：它等于把上述缺陷固化
//! 成"官方测试入口"。测试要造配置必须**显式**给出 broker/证书路径。
//!
//! `LocalMqttConfig::default()` 仍是回环待机形态（`127.0.0.1:1883`，`enabled=false`）
//! ——它**不指向外部域名**，且缺省不 spawn（§9.3.2 的 `local` 段默认如此）。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// MQTT 全局配置（本地 + 北向）。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MqttConfig {
    pub local: LocalMqttConfig,
    pub north: NorthMqttConfig,
}

/// 本地 mosquitto 配置。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocalMqttConfig {
    /// 是否启用本端桥接（审查 R2-B5：缺省 false——未启用不 spawn）
    #[serde(default)]
    pub enabled: bool,
    pub broker_addr: String,
    pub client_id: String,
    pub clean_session: bool,
    pub keepalive_secs: u64,
    pub reconnect: ReconnectConfig,
}

impl Default for LocalMqttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            broker_addr: "127.0.0.1:1883".to_string(),
            client_id: "mupc-local".to_string(),
            clean_session: true,
            keepalive_secs: 60,
            reconnect: ReconnectConfig::default(),
        }
    }
}

/// 北向 emqx 配置（§9.3.2 的 `mqtt_bridge.north` 逐字段映射目标）。
///
/// ⚠️ **`Debug` 手写**（§9.3.5：密码不得进日志）——`password` 一律打印 `***`。
#[derive(Clone, Deserialize, Serialize)]
pub struct NorthMqttConfig {
    /// 是否启用北向桥接（审查 R2-B5：缺省 false——未启用不真连任何外部地址）
    #[serde(default)]
    pub enabled: bool,
    /// `"host:port"`。**缺省空串**（C-11）：空串在建连路径上必然失败，不会静默连假域名。
    pub broker_addr: String,
    /// 客户端 ID（§9.3.2：YAML 层空 ⇒ 装配期取 dev_id，仍空则 validate 拒）。
    pub client_id: String,
    /// 用户名（可选；**不进日志**，§9.3.6）。
    #[serde(default)]
    pub username: Option<String>,
    /// 密码（可选；**不进日志/载荷**，§9.3.6）。本结构的 `Debug` **手写掩码**。
    #[serde(default)]
    pub password: Option<String>,
    /// 明文传输开关（§9.3.5 / Q9）：**仅**非生产构建 + 显式 true 允许；
    /// `false` ⇒ 走三件证书的 TLS（fail-closed）。缺省 false。
    #[serde(default)]
    pub allow_plaintext: bool,
    pub keepalive_secs: u64,
    pub tls: TlsConfig,
    pub reconnect: ReconnectConfig,
}

impl std::fmt::Debug for NorthMqttConfig {
    /// **凭据不入日志**（§9.3.5 ①）：`password` 手写为 `***`（`Some`/`None` 可辨，
    /// 内容不可辨）；`username` 非密钥，照常可读，便于排查"认证被拒"类故障。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NorthMqttConfig")
            .field("enabled", &self.enabled)
            .field("broker_addr", &self.broker_addr)
            .field("client_id", &self.client_id)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("allow_plaintext", &self.allow_plaintext)
            .field("keepalive_secs", &self.keepalive_secs)
            .field("tls", &self.tls)
            .field("reconnect", &self.reconnect)
            .finish()
    }
}

impl Default for NorthMqttConfig {
    /// **缺省 = 空串 + 空路径 + disabled**（C-11：不再有 `mqtt.example.com` / dummy 证书）。
    fn default() -> Self {
        Self {
            enabled: false,
            broker_addr: String::new(),
            client_id: String::new(),
            username: None,
            password: None,
            allow_plaintext: false,
            keepalive_secs: 60,
            tls: TlsConfig {
                ca_cert: PathBuf::new(),
                client_cert: PathBuf::new(),
                client_key: PathBuf::new(),
            },
            reconnect: ReconnectConfig::default(),
        }
    }
}

impl NorthMqttConfig {
    /// 空 broker（未配置）⇒ 不得建连（CFG-2 的**运行期**二次把关：装配期 validate 之外
    /// 再有一道，防"未来新调用方绕过 validate 直接用 `::default()`"）。
    pub fn broker_is_configured(&self) -> bool {
        !self.broker_addr.trim().is_empty()
    }
}

/// TLS 配置（三件证书，§9.3.5）。
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    pub ca_cert: PathBuf,
    pub client_cert: PathBuf,
    pub client_key: PathBuf,
}

impl TlsConfig {
    /// 三路径皆非空（§9.3.2 的 validate 判据之一；此处供建连侧二次把关）。
    pub fn all_paths_set(&self) -> bool {
        let non_empty = |p: &PathBuf| !p.as_os_str().is_empty();
        non_empty(&self.ca_cert) && non_empty(&self.client_cert) && non_empty(&self.client_key)
    }
}

/// 重连配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReconnectConfig {
    /// 初始重连间隔（秒）
    pub initial_interval_secs: u64,
    /// 最大重连间隔（秒）
    pub max_interval_secs: u64,
    /// 退避乘数
    pub backoff_multiplier: f64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            initial_interval_secs: 1,
            max_interval_secs: 60,
            backoff_multiplier: 2.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **C-11 回归锚**：缺省不得再指向 `mqtt.example.com` / dummy 证书路径。
    #[test]
    fn north_default_is_empty_not_example_domain() {
        let c = NorthMqttConfig::default();
        assert!(!c.enabled, "缺省不启用（CFG-2：不产生连接尝试）");
        assert_eq!(
            c.broker_addr, "",
            "缺省 broker 必须为空串（原为 mqtt.example.com:8883）"
        );
        assert_eq!(c.client_id, "");
        assert!(!c.allow_plaintext, "缺省必须 fail-closed（走 TLS）");
        assert!(!c.tls.all_paths_set(), "缺省三证书路径必须为空");
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("example.com"), "缺省配置不得含假域名：{dbg}");
        assert!(!c.broker_is_configured());
    }

    /// **凭据不得进 `Debug`**（§9.3.5 / AC-U74-07 的一部分）：密码一律打 `***`。
    #[test]
    fn north_debug_masks_password() {
        let c = NorthMqttConfig {
            username: Some("mupc".into()),
            password: Some("s3cr3t-p@ss".into()),
            ..Default::default()
        };
        let dbg = format!("{c:?}");
        assert!(
            !dbg.contains("s3cr3t-p@ss"),
            "密码明文绝不得出现在 Debug 输出：{dbg}"
        );
        assert!(dbg.contains("***"), "密码位应打掩码：{dbg}");
        assert!(dbg.contains("mupc"), "用户名可读（非密钥）：{dbg}");
    }

    /// 空密码（`Some("")`）也不得泄漏为可辨内容——掩码与 `None` 同形。
    #[test]
    fn north_debug_masks_empty_password_too() {
        let c = NorthMqttConfig {
            password: Some(String::new()),
            ..Default::default()
        };
        assert!(format!("{c:?}").contains("***"));
    }
}
