//! 加密策略管理
//!
//! 管理各通信通道的加密策略配置

use serde::{Deserialize, Serialize};

/// 通道加密策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPolicy {
    pub channel_name: String,
    pub encryption_required: bool,
    pub min_tls_version: String,
    pub allowed_ciphers: Vec<String>,
    pub require_mutual_auth: bool,
    pub require_sm_crypto: bool,
    pub key_rotation_days: u32,
}

impl Default for ChannelPolicy {
    fn default() -> Self {
        Self {
            channel_name: "default".into(),
            encryption_required: true,
            min_tls_version: "1.2".into(),
            allowed_ciphers: vec!["SM4-GCM".into(), "SM2".into()],
            require_mutual_auth: true,
            require_sm_crypto: true,
            key_rotation_days: 90,
        }
    }
}

/// 策略管理器
pub struct PolicyManager {
    policies: Vec<ChannelPolicy>,
}

impl Default for PolicyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyManager {
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
        }
    }

    pub fn load_defaults(&mut self) {
        self.policies = vec![
            ChannelPolicy {
                channel_name: "iec104".into(),
                ..Default::default()
            },
            ChannelPolicy {
                channel_name: "iec61850".into(),
                ..Default::default()
            },
            ChannelPolicy {
                channel_name: "mqtt".into(),
                ..Default::default()
            },
            ChannelPolicy {
                channel_name: "intercore".into(),
                ..Default::default()
            },
        ];
    }

    pub fn add_policy(&mut self, policy: ChannelPolicy) {
        self.policies
            .retain(|p| p.channel_name != policy.channel_name);
        self.policies.push(policy);
    }

    pub fn get_policy(&self, channel: &str) -> Option<&ChannelPolicy> {
        self.policies.iter().find(|p| p.channel_name == channel)
    }

    pub fn remove_policy(&mut self, channel: &str) -> bool {
        let len_before = self.policies.len();
        self.policies.retain(|p| p.channel_name != channel);
        self.policies.len() < len_before
    }

    pub fn list_channels(&self) -> Vec<&str> {
        self.policies
            .iter()
            .map(|p| p.channel_name.as_str())
            .collect()
    }

    pub fn validate(&self) -> Result<(), crate::errors::SecurityError> {
        let mut errors = Vec::new();
        for policy in &self.policies {
            if policy.channel_name.is_empty() {
                errors.push("策略缺少 channel_name".to_string());
            }
            if policy.encryption_required && policy.allowed_ciphers.is_empty() {
                errors.push(format!(
                    "通道 {} 要求加密但未配置任何算法",
                    policy.channel_name
                ));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(crate::errors::SecurityError::PolicyError(errors.join("; ")))
        }
    }
}
