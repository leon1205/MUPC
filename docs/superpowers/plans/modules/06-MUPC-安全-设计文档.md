# MUPC 安全模块 — 完整技术设计文档

| 版本 | 日期 | 作者 | 状态 |
|------|------|------|------|
| v1.0 | 2026-05-29 | 架构师 | 合并版 |

**合并来源文档：**

| # | 源文档 | 状态标记 |
|---|--------|----------|
| 1 | `2026-05-28-SM2-SM4-国密真正实现-实施计划.md` | — |
| 2 | `2026-05-27-MUPC-Phase2B-协议安全-实施计划.md` | DRAFT |
| 3 | `2026-05-29-MUPC-安全启动-设计文档.md` | [DESIGN_APPROVED] |
| 4 | `2026-05-29-MUPC-电力安全合规增强-设计文档.md` | 待评审 |
| 5 | `06-MUPC-安全-PRD.md` | [REVIEWED: PASS] |

---

## 目录

1. [模块架构](#1-模块架构)
2. [SM2/SM4 国密算法设计](#2-sm2sm4-国密算法设计)
3. [SM2 TLS 集成设计](#3-sm2-tls-集成设计)
4. [纵向加密认证设计](#4-纵向加密认证设计)
5. [安全启动设计](#5-安全启动设计)
6. [证书生命周期管理设计](#6-证书生命周期管理设计)
7. [控制指令全链路加密审计设计](#7-控制指令全链路加密审计设计)
8. [安全告警与合规仪表盘设计](#8-安全告警与合规仪表盘设计)
9. [接口定义](#9-接口定义)
10. [文件结构](#10-文件结构)
11. [技术决策记录](#11-技术决策记录)

---

## 1. 模块架构

### 1.1 设计目标

MUPC 安全模块覆盖三大安全防线，作为整个系统的密码学基础设施和安全策略中枢：

| 防线 | 领域 | 覆盖范围 |
|------|------|----------|
| 第一防线 | SM2/SM4/SM3 国密算法实现 | 密码学基础设施 |
| 第二防线 | 安全启动（Secure Boot） | BootROM → U-Boot → Kernel → RootFS 信任链 |
| 第三防线 | 电力安全合规增强 | 纵向加密认证、证书管理、全链路加密审计 |

### 1.2 架构总览

```
调度主站
    │
    │ IPSec VPN（纵向加密认证装置）
    │ SM2 证书双向认证 + SM4 加密（IPSec ESP）
    ▼
┌─────────────────────────────────────────────────────────┐
│ gateway（IEC 104 / IEC 61850 / MQTT）                    │
│   - APDU SM4 加密/解密（security::sm4）                  │
│   - APDU SM2 签名/验签（security::sm2）                  │
│   - IPSec 隧道绑定（security::lea）                      │
└────────────────────┬────────────────────────────────────┘
                     │ 消息总线（加密指令流转）
                     ▼
┌─────────────────────────────────────────────────────────┐
│ strategy-engine                                         │
│   - 指令 SM4 解密（security::sm4）                       │
│   - 重放防护（序列号检查）                                │
│   - 指令 SM2 签名（security::sm2）                       │
└────────────────────┬────────────────────────────────────┘
                     │ 消息总线（加密指令流转）
                     ▼
┌─────────────────────────────────────────────────────────┐
│ intercore                                               │
│   - TCP 会话 SM4-GCM 加密                               │
│   - 共享密钥派生（security::sm2 密钥交换）               │
└────────────────────┬────────────────────────────────────┘
                     │ RJ45 TCP（SM4 加密密文）
                     ▼
实时控制模块
```

### 1.3 security crate 模块结构

```
mupc/crates/security/                  ← security crate（所有安全功能）
├── sm2.rs                             # SM2 签名/验签/密钥交换
├── sm3.rs                             # SM3 消息摘要
├── sm4.rs                             # SM4 加密/解密（GCM + CBC）
├── cert.rs                            # SM2 证书解析与基本加载
├── tls.rs                             # SM2 TLS 配置入口
├── tls_sm2.rs                         # SM2 TLS CryptoProvider 实现
├── lea.rs                             # 纵向加密认证管理
├── lea_vici.rs                        # strongSwan VICI 协议客户端
├── cert_mgr.rs                        # 证书生命周期管理
├── audit.rs                           # 加密审计日志
├── policy.rs                          # 加密策略管理
├── alarm.rs                           # 安全事件告警
├── compliance.rs                      # 合规自检引擎
├── errors.rs                          # 统一错误类型
├── secure_boot/                       # 安全启动模块 [DESIGN_APPROVED]
│   ├── mod.rs                         # 模块入口 + SecureBootService
│   ├── status.rs                      # 安全启动状态管理
│   ├── monitor.rs                     # 运行时完整性监控
│   ├── audit.rs                       # 安全审计日志
│   ├── health.rs                      # 健康检查
│   └── rollback.rs                    # 防回滚接口
└── lib.rs                             # 模块导出
```

### 1.4 模块依赖与集成矩阵

| 消费者 crate | 集成的 security 组件 | 交互方式 |
|-------------|---------------------|----------|
| gateway | sm2（签名/验签）、sm4（APDU 加密）、lea（隧道绑定） | 消息总线 + API 调用 |
| strategy-engine | sm2（签名）、sm4（解密）、audit、alarm | 消息总线 + API 调用 |
| intercore | sm2（密钥交换）、sm4（TCP 会话加密） | API 调用 |
| mqtt-plugin | tls_sm2（CryptoProvider）、sm4（载荷加密） | API 调用 |
| web-api | secure_boot::status、alarm、compliance、cert_mgr | REST API |
| ota-update | sm2（固件验签）、secure_boot::rollback | API 调用 |

---

## 2. SM2/SM4 国密算法设计

### 2.1 技术选型 [DESIGN_APPROVED]

| 特性 | 说明 |
|------|------|
| 核心库 | gmsm 0.14（纯 Rust，无外部依赖） |
| 标准符合 | GM/T 0002-2012（SM4）、GM/T 0003-2012（SM2）、GM/T 0004-2012（SM3） |
| 平台支持 | Linux/RK3588、Windows、macOS |
| 许可证 | Apache-2.0 / MIT |
| Feature 开关 | `default = ["real_gmsm"]`；`fake_gmsm` = 使用 ring 模拟（仅 CI/开发） |

### 2.2 Cargo 依赖配置

```toml
# security/Cargo.toml
[package]
name = "mupc-security"
version = "0.1.0"
edition = "2021"

[dependencies]
# 国密实现（gmsm）
gmsm = { version = "0.14", features = ["sm2", "sm3", "sm4", "x509"], optional = true }
# 兼容旧代码（ring 模拟实现）
ring = { version = "0.16", optional = true }
thiserror = "1"
serde = { workspace = true }
serde_json = { workspace = true }
base64 = "0.21"
hex = "0.4"
getrandom = "0.2"
# 证书生命周期管理
notify = "6"
chrono = { workspace = true }
sha2 = "0.10"
serde_yaml = "0.9"
uuid = { workspace = true }

[features]
default = ["real_gmsm"]
real_gmsm = ["dep:gmsm"]
fake_gmsm = ["dep:ring"]

[dev-dependencies]
tokio-test = "0.4"
zeroize = "1"
```

### 2.3 SM2 签名与验签

#### 2.3.1 核心接口

```rust
/// SM2 签名
/// - data: 待签名数据
/// - private_key_pem: PEM 格式私钥（字符串或文件路径）
pub fn sm2_sign(data: &[u8], private_key_pem: &str) -> Result<Vec<u8>>;

/// SM2 验签
pub fn sm2_verify(data: &[u8], signature: &[u8], public_key_pem: &str) -> Result<bool>;

/// 生成 SM2 密钥对（必须使用 RK3588 TRNG）
pub fn sm2_key_generate() -> Result<Sm2KeyPair>;

/// 派生共享密钥（ECDH 风格）
pub fn sm2_derive_shared_key(key_pair: &Sm2KeyPair, peer_public_key: &[u8]) -> Result<Vec<u8>>;

/// 签名 R/S 分量转换
pub fn signature_to_rs(signature: &[u8]) -> Result<(Vec<u8>, Vec<u8>)>;
pub fn rs_to_signature(r: &[u8], s: &[u8]) -> Result<Vec<u8>>;
```

#### 2.3.2 密钥对结构

```rust
/// SM2 密钥对（用于签名）
pub struct Sm2KeyPair {
    key: gmsm::Sm2KeyPair,
}

/// SM2 签名结构
pub struct Sm2Signature {
    pub r: Vec<u8>,
    pub s: Vec<u8>,
}
```

#### 2.3.3 密钥 PEM 加载

```rust
/// 从 PEM 文件加载 SM2 私钥
pub fn load_sm2_private_key(path: &str) -> Result<Vec<u8>>;

/// 从 PEM 文件加载 SM2 公钥
pub fn load_sm2_public_key(path: &str) -> Result<Vec<u8>>;
```

#### 2.3.4 密钥生成要求 [DESIGN_APPROVED]

- 密钥生成必须使用 RK3588 TRNG（硬件真随机数发生器）
- 私钥加密存储：AES-256-GCM，密码强度不少于 16 位
- 输出格式：私钥（加密 PEM）、公钥（DER + PEM）、公钥 SHA-256 哈希（OTP 用）

### 2.4 SM4 对称加密

#### 2.4.1 核心接口

```rust
/// SM4 密钥结构（16 字节，128 位）
pub struct Sm4Key { ... }

impl Sm4Key {
    pub fn from_bytes(key: &[u8]) -> Result<Self>;   // 16 字节
    pub fn from_hex(hex: &str) -> Result<Self>;
    pub fn as_bytes(&self) -> &[u8; 16];
}
```

#### 2.4.2 GCM 模式

```rust
/// SM4 GCM 模式加密（带认证标签）
/// IV 推荐 12 字节，安全警告：严禁重用 IV！
pub fn sm4_gcm_encrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>>;

/// SM4 GCM 模式解密
pub fn sm4_gcm_decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>>;
```

#### 2.4.3 CBC 模式

```rust
/// SM4 CBC 模式加密（含 PKCS7 填充）
pub fn sm4_cbc_encrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>>;

/// SM4 CBC 模式解密
pub fn sm4_cbc_decrypt(data: &[u8], key: &[u8], iv: &[u8]) -> Result<Vec<u8>>;
```

#### 2.4.4 IV 生成与安全

```rust
/// 生成 16 字节随机 IV（使用系统 CSPRNG）
pub fn generate_iv() -> Vec<u8>;
```

**IV 安全策略：** [DESIGN_APPROVED]

| 机制 | 说明 |
|------|------|
| 随机 IV | 每次加密使用 12 字节随机 IV（getrandom） |
| 单调计数器 | 适用于流式加密场景 |
| IV 管理 | 严禁重用 — 同一 IV 加密多条消息将导致密文可被攻击者解密 |

### 2.5 SM3 消息摘要

#### 2.5.1 核心接口

```rust
/// SM3 消息摘要
/// 标准：GM/T 0004-2012《SM3 密码杂凑算法》
/// 输出：32 字节（256 位）哈希值
pub fn sm3_hash(data: &[u8]) -> Result<Vec<u8>>;

/// SM3 密钥派生（HKDF-SM3）
pub fn sm3_derive_key(
    input_key: &[u8],
    salt: &[u8],
    info: &[u8],
    output_len: usize,
) -> Result<Vec<u8>>;
```

### 2.6 SM2 证书支持

```rust
/// SM2 证书
pub struct Sm2Cert { ... }

/// 证书存储
pub struct CertStore { ... }

impl CertStore {
    /// 从 PEM 文件加载证书
    pub fn from_pem_file(path: &str) -> Result<Self>;
    /// 添加证书
    pub fn add_cert(&mut self, cert: Sm2Cert);
    /// 验证证书链
    pub fn verify_chain(&self, root: &Sm2Cert) -> Result<bool>;
}

/// 加载 SM2 证书
pub fn load_sm2_certificate(path: &str) -> Result<Sm2Cert>;
```

### 2.7 Feature Flag 与兼容性 [DESIGN_APPROVED]

| Feature | 说明 | 适用场景 |
|---------|------|----------|
| `real_gmsm`（默认） | 使用 gmsm 实现真正的国密算法 | 生产环境、正式测试 |
| `fake_gmsm` | 使用 ring 库的 P-256/AES-256 模拟 | CI 测试、开发环境 |
| 无 feature | 仅返回 Err | 编译期验证 |

**生产环境必须启用 `real_gmsm`。** 接口签名在两种 feature 下保持一致，确保业务代码无需修改。

### 2.8 敏感数据清除

```rust
// 使用 zeroize 安全清除敏感内存
use zeroize::Zeroizing;
let key = Zeroizing::new(sensitive_data);  // 作用域结束后自动清零
```

### 2.9 验收标准 [REVIEWED: PASS]

| ID | 验收内容 | 验证方法 |
|----|----------|----------|
| GM-01 | SM2 签名符合 GM/T 0003-2012 | 国家密码管理局测试向量 |
| GM-02 | SM2 验签符合 GM/T 0003-2012 | 国家密码管理局测试向量 |
| GM-03 | SM4 加密符合 GM/T 0002-2012 | KAT 测试向量 |
| GM-04 | SM4 解密符合 GM/T 0002-2012 | KAT 测试向量 |
| GM-05 | SM3 摘要符合 GM/T 0004-2012 | 标准测试向量 |
| GM-06 | 接口向后兼容 | 现有代码无需修改 |
| GM-07 | 错误类型正确 | 所有 GmError 实现 `std::error::Error` |

---

## 3. SM2 TLS 集成设计

### 3.1 核心挑战 [DESIGN_APPROVED]

rustls 0.22/0.23 **不原生支持** SM2 签名算法和 SM4 密码套件。需要实现：

1. 自定义 `rustls::crypto::CryptoProvider` — 接管签名验证 + 密钥交换 + 记录加密
2. 自定义 `rustls::client::danger::ServerCertVerifier` — 验证 SM2 证书
3. SM4-GCM AEAD 实现 — 用于 TLS 记录层加密（RFC 8998 要求）

### 3.2 整体架构

```
┌─────────────────────────────────────────────────────────┐
│                      rustls                              │
│  ┌───────────────────────────────────────────────────┐  │
│  │            CryptoProvider（自定义）                  │  │
│  │  ┌────────────────┐  ┌────────────────────────┐   │  │
│  │  │ Sm2SigningKey  │  │ Sm4GcmCipherSuite     │   │  │
│  │  │ - SM2 签名/验签  │  │ - SM4-GCM 加密/解密   │   │  │
│  │  │ - RFC 8998     │  │ - 12B nonce / 16B tag │   │  │
│  │  │   sig_scheme   │  └────────────────────────┘   │  │
│  │  └────────────────┘                                │  │
│  │  ┌────────────────┐  ┌────────────────────────┐   │  │
│  │  │ Sm2KeyExchange │  │ Sm2ServerCertVerifier  │   │  │
│  │  │ - ECDHE with   │  │ - SM2 证书链验证        │   │  │
│  │  │   sm2p256v1    │  │ - 有效期/CRL 检查       │   │  │
│  │  └────────────────┘  └────────────────────────┘   │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

### 3.3 自定义 CryptoProvider

> **Phase 2+ 状态：** `SmCryptoProvider` 当前为简单 struct（含 server_cert/client_cert 字段），**未实现** `rustls::CryptoProvider` trait。下文 API 为 Phase 2+ 目标设计。当前 TLS 加密由 ring 默认 provider 提供。

```rust
/// SM2 + SM4 国密 CryptoProvider
pub struct SmCryptoProvider;

impl CryptoProvider for SmCryptoProvider {
    fn cipher_suites(&self) -> Vec<CipherSuite> {
        vec![
            // TLS 1.3: SM4-GCM（RFC 8998）
            //   TODO: 注册自定义 TLS_AUTO_SM4_GCM_SM3 套件
            // TLS 1.2: TLS_ECDHE_SM2_WITH_SM4_GCM_SM3（0x00, 0xC6）
        ]
    }
    fn kx_groups(&self) -> &[SupportedKxGroup] {
        &[&Sm2KxGroup]  // sm2p256v1 曲线 ECDHE
    }
    fn signature_verification_algorithms(&self) -> &[SignatureScheme] {
        &[SignatureScheme::SM2_SIG_SM3]  // RFC 8998
    }
}
```

### 3.4 SM4-GCM TLS 记录加密（RFC 8998）

```rust
/// SM4-GCM TLS 记录保护器
pub struct Sm4GcmCipher {
    key: [u8; 16],        // SM4 128-bit 密钥
    fixed_iv: [u8; 12],   // 4B fixed + 8B implicit nonce
    sequence_number: u64,
}

impl Sm4GcmCipher {
    /// 加密 TLS 记录
    pub fn encrypt(&mut self, plaintext: &[u8], content_type: u8) -> Result<Vec<u8>> {
        let nonce = self.build_nonce();               // fixed_iv XOR seq
        let ciphertext = sm4_gcm_encrypt(plaintext, &self.key, &nonce)?;
        self.sequence_number += 1;
        Ok(ciphertext)                                 // ciphertext + 16B tag
    }

    pub fn decrypt(&mut self, ciphertext: &[u8], content_type: u8) -> Result<Vec<u8>> {
        let nonce = self.build_nonce();
        let plaintext = sm4_gcm_decrypt(ciphertext, &self.key, &nonce)?;
        self.sequence_number += 1;
        Ok(plaintext)
    }
}
```

### 3.5 SM2 证书验证器

```rust
/// SM2 服务器证书验证器
pub struct Sm2ServerCertVerifier;

impl rustls::client::danger::ServerCertVerifier for Sm2ServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer,
        intermediate_certs: &[rustls::pki_types::CertificateDer],
        server_name: &rustls::pki_types::ServerName,
        ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // 1. 解析 X.509 v3 证书（DER）
        // 2. 验证签名算法 OID = SM2-with-SM3（1.2.156.10197.1.501）
        // 3. 验证公钥曲线 = sm2p256v1（1.2.156.10197.1.301）
        // 4. 验证有效期
        // 5. 验证证书链
        // 6. CRL 检查
        Ok(ServerCertVerified::assertion())
    }
}
```

### 3.6 SM2 客户端签名密钥

```rust
/// SM2 签名密钥（用于客户端证书认证）
pub struct Sm2SigningKey {
    private_key: gmsm::Sm2KeyPair,
}

impl rustls::sign::SigningKey for Sm2SigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme])
        -> Option<Box<dyn rustls::sign::Signer>>
    {
        if offered.contains(&SignatureScheme::SM2_SIG_SM3) {
            Some(Box::new(Sm2Signer { key: self.private_key.clone() }))
        } else {
            None  // 不支持非 SM2 方案
        }
    }
}

struct Sm2Signer { key: gmsm::Sm2KeyPair }

impl rustls::sign::Signer for Sm2Signer {
    fn sign(&self, message: &[u8]) -> Result<rustls::Signature, rustls::Error> {
        let sig_bytes = sm2_sign(message, ...)?;
        Ok(rustls::Signature::new(SignatureScheme::SM2_SIG_SM3, sig_bytes))
    }
    fn scheme(&self) -> SignatureScheme { SignatureScheme::SM2_SIG_SM3 }
}
```

### 3.7 TLS 配置入口

```rust
/// 构建国密 TLS 客户端配置
pub fn build_sm2_tls_client_config(
    ca_cert_path: &str,
    client_cert_path: Option<&str>,
    client_key_path: Option<&str>,
) -> Result<rustls::ClientConfig>;
```

### 3.8 rustls SM2 方案支持策略

rustls 0.23 的 `SignatureScheme` 枚举当前未包含 `SM2_SIG_SM3`。采用分阶段实施： [DESIGN_APPROVED]

| 阶段 | 方案 | 说明 |
|------|------|------|
| Phase 1 | 混合架构：Tongsuo/GmSSL 动态库调用 | 集成最快，开发成本最低 |
| Phase 2 | 自定义 CryptoProvider + CertificateVerifier | 在验证器内部处理 SM2 证书验证，绕过枚举限制 |
| Phase 3 | 提交 RFC 8998 PR 到 rustls 上游 | 若不被采纳，维护轻量级 fork |

### 3.9 MQTT over TLS + 国密集成

#### 3.9.1 MQTT 配置扩展

```rust
/// MQTT 国密配置（新增字段）
pub struct MqttConfig {
    pub broker_addr: String,
    pub client_id: String,
    pub use_tls: bool,
    pub cipher_suite: Option<CipherSuitePreference>,  // "SM2-SM4-SM3" | "RSA-AES"
    pub payload_encryption: bool,                      // SM4 应用层加密
    pub payload_key_derive: Option<PayloadKeyDeriveConfig>,
    pub ca_cert: Option<String>,
    pub client_cert: Option<String>,
    pub client_key: Option<String>,
    pub qos: MqttQos,
    pub keepalive_secs: u16,
    pub clean_session: bool,
}
```

#### 3.9.2 SM4 应用层加密

```rust
/// SM4 应用层加密器（MQTT Payload）
pub struct Sm4PayloadCipher {
    session_key: Sm4Key,   // SM2 密钥交换派生
    key_id: String,
}

impl Sm4PayloadCipher {
    /// 通过 SM2 密钥交换初始化
    pub fn from_key_exchange(local_key_path: &str, peer_public_key_path: &str) -> Result<Self>;

    /// 加密 MQTT Payload
    /// 返回格式: [key_id(8)][iv(12)][ciphertext][tag(16)]
    pub fn encrypt_payload(&self, payload: &[u8]) -> Result<Vec<u8>>;

    /// 解密 MQTT Payload
    pub fn decrypt_payload(&self, data: &[u8]) -> Result<Vec<u8>>;
}
```

#### 3.9.3 MQTT 5.0 用户属性传递

```
MQTT CONNECT User Properties:
  "gm-payload-encryption": "SM4-GCM"
  "gm-key-id": "a1b2c3d4e5f6g7h8"
  "gm-key-exchange": "SM2-ECDH"
  "gm-key-ver": "1"

MQTT PUBLISH User Properties:
  "gm-payload-encryption": "SM4-GCM"
  "gm-key-id": "a1b2c3d4e5f6g7h8"
  "gm-gcm-iv": "base64(iv)"
```

### 3.10 验收标准 [REVIEWED: PASS]

| ID | 验收内容 | 验证方法 |
|----|----------|----------|
| LEA-20 | SM2 证书使用 SM2-with-SM3 签名算法（OID 1.2.156.10197.1.501） | openssl 解析验证 |
| LEA-21 | 公钥曲线为 sm2p256v1（OID 1.2.156.10197.1.301） | openssl 解析验证 |
| LEA-22 | TLS 1.3 使用 sig_sm2_sign_sm3（RFC 8998） | 抓包验证 |
| LEA-23 | 错误信息明确区分证书格式/算法/过期/链不完整四种场景 | 功能测试 |
| LEA-24 | 同时管理 ≥ 3 张 SM2 证书（TLS 服务器、客户端、IPSec） | 配置验证 |
| LEA-25 | TLS 1.2 支持 `TLS_ECDHE_SM2_WITH_SM4_GCM_SM3`（0x00,0xC6） | 握手测试 |
| LEA-26 | TLS 1.3 支持 SM2 签名 + SM4-GCM 记录加密 | 抓包验证 |
| LEA-27 | MUPC 客户端和服务端均可使用 SM2 证书完成双向认证 | 双向测试 |
| LEA-28 | 完整握手 P99 ≤ 100ms（RK3588） | 1000 次测量 |
| LEA-29 | 与 GmSSL 3.x、Tongsuo 8.x、OpenSSL 3.x + SM2 互通 | 连通性测试 |

---

## 4. 纵向加密认证设计

### 4.1 架构策略：strongSwan VICI 协议集成 [DESIGN_APPROVED]

由于 Rust 生态尚无成熟的 IPSec IKEv2 原生实现，采用通过 Unix Socket 调用 strongSwan VICI 协议的方式集成。

```
┌─────────────────────────────┐
│  security::lea              │
│  ┌───────────────────────┐  │
│  │ LeaManager            │  │
│  │  - init_conn()        │  │
│  │  - create_tunnel()    │  │
│  │  - close_tunnel()     │  │
│  │  - get_tunnel_status()│  │
│  └─────────┬─────────────┘  │
│            │ Unix Socket     │
│            │ /var/run/       │
│            │ charon.vici     │
└────────────┼─────────────────┘
             ▼
┌─────────────────────────────┐
│  strongSwan charon daemon   │
│  ┌───────────────────────┐  │
│  │ IKEv2 + SM2 + SM4/SM3 │  │
│  │ IPSec SA Manager      │  │
│  └───────────────────────┘  │
└─────────────────────────────┘
```

### 4.2 LeaManager 核心接口

```rust
/// 加密隧道配置
pub struct LeaConfig {
    pub name: String,
    pub peer_addr: String,          // 对端加密装置地址
    pub local_id: String,           // 本地身份
    pub remote_id: String,          // 对端身份
    pub local_cert_path: String,    // SM2 本地证书路径
    pub local_key_path: String,     // SM2 私钥路径
    pub ca_cert_path: String,       // CA 证书路径
    pub ike_version: u8,            // 1=IKEv1, 2=IKEv2
    pub ike_cipher: String,         // 默认 SM4-GCM
    pub ike_integrity: String,      // 默认 HMAC-SM3
    pub esp_cipher: String,         // 默认 SM4-GCM
    pub esp_integrity: String,      // 默认 HMAC-SM3
    pub timeout_secs: u32,          // 隧道超时（默认 30s）
    pub reconnect_initial: u32,     // 重连初始间隔（秒）
    pub reconnect_max: u32,         // 重连最大间隔（秒）
}

/// 隧道状态
pub enum TunnelState {
    Disconnected,
    Connecting,
    Connected { uptime_secs: u64, bytes_in: u64, bytes_out: u64,
                cipher: String, integrity: String },
    Error(String),
}

/// 加密隧道管理器
pub struct LeaManager {
    configs: Vec<LeaConfig>,
    tunnels: HashMap<String, TunnelState>,
    vici_socket_path: PathBuf,
}
```

### 4.3 VICI 协议通信

VICI 协议使用 segment-based 编码，通过 Unix Socket（`/var/run/charon.vici`）传输。

#### 4.3.1 VICI 报文编码

```
[长度: 4字节大端][名称: 长度字节][类型: 1字节][值...]

类型:
  0x00 = 节段开始（section start）
  0x01 = 节段结束（section end）
  0x02 = 键值对（字符串值）
  0x03 = 列表项（list item）
  0x04 = 键值对（原始字节值）
```

#### 4.3.2 关键操作映射

| 操作 | VICI 命令 | 参数 |
|------|-----------|------|
| 加载证书 | `load-cert` | `{type: "x509", cert: "<PEM>"}` |
| 加载私钥 | `load-key` | `{type: "private", key: "<PEM>"}` |
| 创建 IKE 连接 | `load-conn` | `<IKE 配置 JSON>` |
| 初始化隧道 | `initiate` | `{child: "<conn_name>"}` |
| 查询状态 | `list-sas` | `{ike: "<conn_name>"}` |
| 关闭隧道 | `unload-conn` | `{name: "<conn_name>"}` |

#### 4.3.3 ViciClient 实现

```rust
/// strongSwan VICI 协议客户端
struct ViciClient {
    socket_path: PathBuf,
    stream: UnixStream,
}

impl ViciClient {
    async fn connect(socket_path: &Path) -> Result<Self>;
    async fn send_command(&mut self, cmd: &str, params: &Value) -> Result<Value>;
    async fn load_cert(&mut self, cert_type: &str, pem_data: &str) -> Result<()>;
    async fn load_key(&mut self, key_type: &str, pem_data: &str) -> Result<()>;
    async fn load_conn(&mut self, name: &str, config: &LeaConfig) -> Result<()>;
    async fn initiate(&mut self, conn_name: &str) -> Result<()>;
    async fn list_sas(&mut self) -> Result<Vec<SaInfo>>;
    async fn unload_conn(&mut self, name: &str) -> Result<()>;
}
```

### 4.4 strongSwan 配置模板

```
conn mupc-tunnel-{name}
    left={local_addr}
    leftid={local_id}
    leftcert={local_cert_path}
    leftfirewall=yes
    right={peer_addr}
    rightid={remote_id}
    rightca={ca_cert_path}
    keyexchange=ikev2
    ike=sm4-gcm-sm3-modp2048
    esp=sm4-gcm-sm3
    auth=pubkey
    ikelifetime=24h
    lifetime=8h
    mobike=yes
    dpddelay=10s
    dpdtimeout=30s
    dpdaction=restart
```

### 4.5 隧道管理关键流程

**创建隧道：**
1. 检查 strongSwan charon 守护进程是否运行
2. 通过 VICI 加载 SM2 证书和私钥
3. 通过 VICI 加载连接配置
4. 发起 IKEv2 握手（SM2 证书双向认证）
5. 协商 SM4-GCM + HMAC-SM3 算法
6. 建立 IPSec SA，开始数据加密传输

**自动重连（指数退避）：**
```
退避序列：1s, 2s, 4s, 8s, 16s, 32s, 60s（上限）
重连成功或手动重置后恢复初始值
```

**多隧道支持：**
```rust
pub fn add_tunnel(&mut self, config: LeaConfig) -> Result<String>;
pub fn remove_tunnel(&self, name: &str) -> Result<()>;
pub fn list_tunnels(&self) -> Vec<(String, TunnelState)>;
```

### 4.6 加密强度降级策略 [REVIEWED: PASS]

**核心原则：加密强度只可升级不可降级。**

| 场景 | 降级行为 | 安全约束 |
|------|----------|----------|
| SM2 TLS 握手失败 | 不允许降级至 RSA TLS | 必须双向证书认证；加密强度不低于 AES-128-GCM |
| 双向认证失败 | 连接被拒绝 | 无降级路径 |
| 协议版本低于 TLS 1.2 | 连接被拒绝 | 必须 TLS 1.2+ |
| 证书链验证失败 | 连接被拒绝 | 无降级路径 |

### 4.7 验收标准 [REVIEWED: PASS]

| ID | 验收内容 | 优先级 |
|----|----------|--------|
| LEA-01 | IEC 104 APDU 支持 SM2 签名 + SM4 加密 | P0 |
| LEA-02 | 无效签名指令被拒绝执行 | P0 |
| LEA-03 | 遥测/遥信上行附 SM2 签名 | P0 |
| LEA-04 | 加密启用后 APDU 载荷为 SM4 密文 | P0 |
| LEA-05 | 可按通道粒度配置加密/签名算法 | P0 |
| LEA-10 | 与标准加密装置建立 IKEv2 隧道 | P0 |
| LEA-11 | IPSec 使用 SM2 证书双向认证 | P0 |
| LEA-12 | SA 协商使用 SM4-GCM + HMAC-SM3 | P0 |
| LEA-13 | 支持 IKEv2 NAT 穿透、DDoS 防护 | P0 |
| LEA-14 | 隧道超时可配置（默认 30s，范围 10~120s） | P0 |
| LEA-15 | 断开后指数退避重连（1s ~ 60s） | P0 |
| LEA-16 | 同时维护 ≥ 2 条独立隧道 | P0 |
| LEA-17 | 隧道状态查询返回详细状态 | P1 |
| LEA-18 | 隧道状态变化产生 syslog 告警 | P1 |
| LEA-19 | 断开超 5 分钟触发运行告警 | P1 |

---

## 5. 安全启动设计 [DESIGN_APPROVED]

> **当前状态：骨架代码已就位，硬件验证逻辑为 Phase 2+ 待实现。** `SecureBootManager::verify_boot_chain()` 当前为 stub（直接返回 Verified），未执行实际 SPL/U-Boot/Kernel/RootFS 信任链验签。下文详细的四层信任链、OTP 存储布局、恢复模式等均为 Phase 2+ 目标架构。

### 5.1 技术选型

针对 RK3588 / ARM64 平台的三种安全启动方案对比：

| 方案 | 描述 | RK3588 兼容性 | 成熟度 | 维护成本 | 评分 |
|------|------|:---:|:---:|:---:|:---:|
| **A: U-Boot Verified Boot（FIT 签名）** | U-Boot 原生 verified boot，FIT 镜像格式，嵌入公钥验签 | **原生支持** | 高 | 低 | **推荐** |
| B: GRUB + shim | x86 主流方案，依赖 UEFI Secure Boot | 不适用 | — | — | 淘汰 |
| C: 自定义 EFI Stub + 签名工具 | 内核自验签或自定义 bootloader | 可行但冗余 | 低 | 极高 | 淘汰 |

**决策结论：采用方案 A，U-Boot Verified Boot（FIT 签名）。** [DESIGN_APPROVED]

### 5.2 四层信任链架构

```
+---------+     +------------+     +--------+     +------------------+     +-----------+
| BootROM | --> | U-Boot SPL | --> | U-Boot | --> | Linux Kernel     | --> | RootFS    |
|（芯片内） |     |（DDR init） |     |（主引导） |     | + FIT（DTB/init） |     |（dm-verity）|
+---------+     +------------+     +--------+     +------------------+     +-----------+
    |                 |                |                     |                     |
    | 验签SPL         | 加载+验签UBoot | 验签FIT镜像         | 挂载rootfs          | 块级实时验签
    |（OTP公钥哈希）   |（嵌在SPL的公钥） |（嵌在U-Boot的公钥）  |（dm-verity根哈希    |
    |                 |                |                     |  由FIT签名保护）     |
    v                 v                v                     v                     v
  安全状态           安全状态          安全状态              安全状态               安全状态
  [SEC_ROM_OK]      [SEC_SPL_OK]     [SEC_UBOOT_OK]        [SEC_KERNEL_OK]       [SEC_ROOTFS_OK]
```

### 5.3 信任链传递

**Step 1 - BootROM：**
- 读取 OTP fuses 中的根公钥 SHA-256 哈希
- 验签 U-Boot SPL（RSA-4096 PKCS#1 v1.5）
- 通过 → 释放 SPL 执行权；失败 → `SECURE_BOOT_FAIL`（`ERR_BOOTROM_SIG_FAIL`）

**Step 2 - U-Boot SPL：**
- 初始化 DDR、时钟等硬件
- 从 eMMC boot 分区加载完整 U-Boot
- 验签 U-Boot 镜像（使用 SPL 内置公钥）
- 通过 → 释放 U-Boot 执行权；失败 → `ERR_SPL_SIG_FAIL`

**Step 3 - U-Boot：**
- 加载 FIT 镜像（kernel + DTB + initramfs）
- 验签 FIT 镜像内各组件哈希（RSA-4096/SM2）
- 检查防回滚计数器（固件版本 >= OTP 记录值）
- 检查吊销清单（签名公钥未被吊销）
- 通过 → 启动内核并传递 dm-verity 根哈希；失败 → `ERR_UBOOT_FIT_SIG_FAIL`

**Step 4 - Linux Kernel：**
- 使用内核 cmdline 中的 dm-verity 根哈希挂载 rootfs
- dm-verity 在 I/O 路径实时校验数据块
- 检测到篡改 → 按策略处理（panic/restart）

**Step 5 - 用户空间初始化：**
- systemd 读取安全启动状态设备节点
- mupc-security::secure_boot::status 初始化
- 记录安全启动日志，北向通道上报

### 5.4 各阶段验签详情

| 阶段 | 验签对象 | 签名算法 | 验签密钥位置 | 预期耗时 |
|------|----------|----------|-------------|---------|
| BootROM | U-Boot SPL 镜像 | RSA-4096 PKCS#1 v1.5 | OTP fuses（sha256 哈希） | ≤ 500ms |
| U-Boot SPL | U-Boot 主镜像 | RSA-4096/SM2 | SPL 内置公钥 | ≤ 500ms |
| U-Boot | FIT（kernel+DTB） | RSA-4096/SM2 | U-Boot 内置公钥 | ≤ 2s |
| Kernel | rootfs（dm-verity） | SHA-256/SM3（哈希树） | 内核 cmdline 传根哈希 | 启动时 |
| 运行时 | 关键文件周期性校验 | SM3/SHA-256 | mupc-security 存储 | 每 60s |

**关键说明：** [DESIGN_APPROVED]
- BootROM 不支持 SM2（Rockchip BootROM 固件算法固定），SPL 签名必须使用 RSA-4096
- U-Boot SPL 和 U-Boot 主镜像可同时支持 RSA-4096 和 SM2
- dm-verity 根哈希以明文传递给内核 cmdline，其本身由 FIT 镜像签名保护

### 5.5 密钥管理方案

#### 5.5.1 四层密钥体系

```
┌─────────────────────────────────────────────────────────────────────┐
│ Layer 0: 根密钥（Root Key）                                          │
│  算法: RSA-4096 或 SM2                                              │
│  存储: 离线 HSM / 硬件加密离线存储                                    │
│  用途: 签发出厂 U-Boot SPL 镜像                                      │
│  OTP 存储: sha256(root_pubkey_der) → 32 字节                        │
│  生命周期: 整个设备生命周期（不可变更）                                  │
└─────────────────────────────────────────────────────────────────────┘
                       │ 签发
                       ▼
┌─────────────────────────────────────────────────────────────────────┐
│ Layer 1: SPL 签名密钥（SPL Signing Key）                             │
│  算法: RSA-4096                                                     │
│  存储: 离线 HSM                                                     │
│  用途: 签名 U-Boot SPL 镜像                                          │
└─────────────────────────────────────────────────────────────────────┘
                       │ 签发
                       ▼
┌─────────────────────────────────────────────────────────────────────┐
│ Layer 2: U-Boot 签名密钥（U-Boot Signing Key）                       │
│  算法: RSA-4096 或 SM2                                              │
│  存储: 离线 HSM                                                     │
│  用途: 签名 FIT 镜像（kernel + DTB）                                  │
│  支持双密钥嵌入（无缝轮换）                                            │
└─────────────────────────────────────────────────────────────────────┘
                       │ 签发
                       ▼
┌─────────────────────────────────────────────────────────────────────┐
│ Layer 3: dm-verity 根哈希（Root Hash）                               │
│  说明: 由 U-Boot 签名密钥间接保护（内嵌于已签名的 FIT 镜像中）          │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│ Layer R: 恢复分区签名密钥（Recovery Key）                             │
│  算法: RSA-4096（独立于主密钥链）                                      │
│  存储: 离线 HSM，与主密钥物理隔离                                     │
│  用途: 签名恢复分区镜像                                               │
└─────────────────────────────────────────────────────────────────────┘
```

#### 5.5.2 OTP 存储布局

RK3588 OTP（eFuse）存储规划（64 words = 2048 bits）：

| 偏移（word） | 名称 | 大小（bits） | 写入时机 | 描述 |
|:---:|------|:---:|------|------|
| 0-7 | root_key_hash | 256 | 出厂前 | sha256(根公钥 DER 编码) |
| 8 | secure_boot_enable | 1 | 出厂前 | 使能安全启动 |
| 9-15 | anti_rollback_counter | 224 | 每次固件升级 | 单调递增计数器（支持 224 次更新） |
| 16-23 | reserved_1 | 256 | — | 预留 |
| 32-39 | device_identity | 256 | 出厂前 | 设备唯一标识（可选） |

#### 5.5.3 密钥轮换 [DESIGN_APPROVED]

**模式 B：代码内多密钥支持**

U-Boot 公钥嵌入配置（双密钥）：
```c
// U-Boot 设备树公钥节点配置（.dts）
&{/signature} {
    key-uboot-2025 {
        required = "conf";
        algo = "sha256,rsa4096";
        key-name-hint = "uboot-2025";
    };
    key-uboot-2026 {
        required = "conf";
        algo = "sha256,rsa4096";
        key-name-hint = "uboot-2026";
    };
};
```

**轮换流程：** 生成新密钥对 → 重新签名固件 → 发布含新公钥固件 → 过渡期双密钥有效（新旧公钥同时嵌入 U-Boot/FIT）→ 过渡期后吊销旧公钥。

#### 5.5.4 吊销清单

吊销清单格式（嵌入 FIT 镜像的独立配置节点）：
```
FIT 镜像:
/ {
    images { kernel{...}; fdt{...}; };
    configurations { config-1 { signature{...}; }; };
    revocation {                       // 吊销清单节点
        revoked-keys = [
            "sha256-<fingerprint1>",
            "sha256-<fingerprint2>",
        ];
        revocation-sig { ... };        // 吊销清单自身签名
    };
};
```

### 5.6 分区布局设计

| 分区 | 起始偏移 | 大小 | 文件系统 | 完整性保护 | 说明 |
|------|----------|------|----------|-----------|------|
| SPL | 0x004000 | 4MB | raw | BootROM RSA 签名 | U-Boot SPL |
| U-Boot | 4MB | 64MB | raw | SPL RSA/SM2 签名 | U-Boot 主镜像（含内置公钥 + 吊销清单） |
| U-Boot ENV | 68MB | 1MB | raw | — | 环境变量（冗余两副本） |
| Misc | 69MB | 1MB | raw | SHA-256 | misc 分区 |
| Recovery | 70MB | 128MB | raw | RSA-4096 签名 | 恢复镜像，使用独立密钥签名 |
| Vendor | 198MB | 64MB | ext4 | dm-verity | 厂商配置 |
| DTB | 262MB | 16MB | raw | FIT 签名 | 设备树备份 |
| Boot（FIT） | 278MB | 256MB | raw | FIT RSA/SM2 签名 | FIT: kernel + DTB + initramfs |
| Rootfs_A | 534MB | 2GB | ext4 | dm-verity | 根文件系统 A（当前激活） |
| Rootfs_B | 2582MB | 2GB | ext4 | dm-verity | 根文件系统 B（OTA 备用） |
| User_Data | 4630MB | 剩余 | ext4 | 运行时监控 | 用户数据/日志 |
| Boot_Log | 末尾-32MB | 32MB | ext4 | 只读挂载 | 安全启动日志 |

### 5.7 Failed 处理与恢复机制

#### 5.7.1 安全启动错误码 [DESIGN_APPROVED]

| 错误码 | 阶段 | 含义 | 恢复方式 |
|--------|:---:|------|----------|
| ERR_BOOTROM_SIG_FAIL | BootROM | SPL 签名验证失败 | 仅恢复模式可修复 |
| ERR_SPL_SIG_FAIL | SPL | U-Boot 签名验证失败 | 恢复模式可修复 |
| ERR_UBOOT_FIT_SIG_FAIL | U-Boot | FIT 签名失败 | 恢复模式可修复 |
| ERR_ROLLBACK_DETECTED | U-Boot | 防回滚计数器检测到降级 | 刷写新版固件 |
| ERR_REVOCATION_LIST_INVALID | U-Boot | 吊销清单签名无效 | 恢复模式可修复 |
| ERR_KEY_REVOKED | U-Boot | 签名公钥在吊销清单中 | 刷写有效密钥签名固件 |
| ERR_ROOTFS_HASH_MISMATCH | Kernel | dm-verity 根哈希不匹配 | 恢复模式可修复 |
| ERR_DM_VERITY_CORRUPTION | Runtime | dm-verity 检测到数据块篡改 | 自动重启 |

#### 5.7.2 恢复模式流程

1. 物理按下恢复按钮（≥3 秒）或系统连续 3 次启动失败自动进入
2. U-Boot 从 Recovery 分区加载恢复镜像（使用独立恢复密钥签名）
3. 恢复镜像执行：初始化网络 → 连接 OTA 服务器 → 下载带签名修复固件 → 验证签名 → 写入主分区 → 重启
4. 安全限制：恢复模式不提供 shell 访问，不支持加载任意未签名代码

#### 5.7.3 防回滚计数器 [DESIGN_APPROVED]

```
存储: OTP fuses（words 9-15, 共 224 bits）
编码: 7 × 32-bit word, 每位代表一次递增
最大值: 224（约 9 年，以每月 2 次 OTA 计算）

读取: 统计所有 set bits 总数
递增: 将下一个未使用的 bit 置 1
写入保护: 仅在 U-Boot 阶段可写，用户空间不可写入

阈值告警:
  ≥ 180（80%）:  WARNING 告警
  = 224（100%）:  CRITICAL 告警（需返厂）
```

### 5.8 运行时完整性监控 [DESIGN_APPROVED]

```rust
/// 完整性监控器
pub struct IntegrityMonitor {
    checklist: IntegrityChecklist,  // 文件路径 → 期望哈希
    check_interval: Duration,       // 默认 60 秒
}

impl IntegrityMonitor {
    /// 启动周期性完整性检查
    pub async fn start_monitoring(&self) {
        // 每 60 秒遍历检查清单，使用 SM3/SHA-256 计算哈希对比
        // 检测到篡改 → 上报告警日志
    }
}
```

### 5.9 验收标准 [REVIEWED: PASS]

| ID | 验收内容 | 验证方法 |
|----|----------|----------|
| SB-01 | BootROM 验签：篡改 SPL 后设备不启动 | 硬件测试 |
| SB-02 | U-Boot FIT 验签：篡改内核/DTB 后拒绝加载 | 硬件测试 |
| SB-03 | dm-verity：篡改 rootfs 后检测并重启 | 集成测试 |
| SB-04 | 全链验签时序 ≤ 15 秒 | 实测计时 |
| SB-05 | 密钥生成：正确生成 RSA-4096/SM2 密钥对 | 功能测试 |
| SB-06 | OTP 烧录不可修改或擦除 | 硬件测试 |
| SB-07 | 密钥轮换：新旧密钥过渡期内均可靠 | 集成测试 |
| SB-08 | 防回滚保护：计数器低于 OTP 记录值 → 拒绝 | 集成测试 |
| SB-09 | 恢复模式：物理触发 → 刷写修复固件 | 硬件测试 |
| SB-10 | 启动状态查询：GET /api/security/boot-status | API 测试 |
| SB-11 | 告警触发：验签失败 → CRITICAL 告警 + 北向上报 | 集成测试 |
| SB-12 | 日志审计：所有事件完整记录 | 功能测试 |
| SB-13 | 运行时完整性校验：关键文件被篡改 60s 内检测 | 集成测试 |
| SB-14 | 签名工具链：正确签名各级固件 | CI 流水线测试 |
| SB-15 | 吊销清单：被吊销密钥签名的固件被拒绝 | 集成测试 |
| SB-16 | dm-verity 性能影响 ≤ 5% | 基准测试对比 |

---

## 6. 证书生命周期管理设计

### 6.1 CertManager 核心架构

```
/etc/mupc/certs/
├── ca/                 # CA 证书链
│   ├── root-ca.pem
│   └── intermediate-ca.pem
├── device/             # 设备证书
│   ├── tls-server.pem + tls-server-key.pem (600)
│   ├── tls-client.pem + tls-client-key.pem (600)
│   └── ipsec.pem + ipsec-key.pem (600)
├── crl/                # CRL 文件
│   └── ca.crl
└── certs.json          # 证书元数据索引
```

### 6.2 核心接口

> **Phase 2+ 状态：** 当前 `CertManager` 为简化实现（ca_cert/client_cert/client_key + CrlManager），仅支持 `load_ca_cert`/`load_client_cert` 基本操作。下文 `import_cert`/`import_crl`/`revoke_cert`/`list_certs`/`get_cert_chain` 等完整 API 为 Phase 2+ 目标设计。

```rust
/// 证书角色
pub enum CertRole { RootCa, IntermediateCa, TlsServer, TlsClient, Ipsec }

/// 证书元数据
pub struct CertMeta {
    pub id: String,
    pub role: CertRole,
    pub subject: String,
    pub issuer: String,
    pub serial: String,
    pub not_before: i64,
    pub not_after: i64,
    pub fingerprint_sha256: String,
    pub imported_at: i64,
    pub is_revoked: bool,
    pub revoked_at: Option<i64>,
}

/// 证书管理器
pub struct CertManager {
    cert_dir: PathBuf,
    certs: HashMap<String, LoadedCert>,
    watcher: Option<notify::RecommendedWatcher>,
}

impl CertManager {
    pub fn new(cert_dir: &Path) -> Result<Self>;
    /// 导入证书（PEM 格式），自动验证格式/SM2 算法/有效期/证书链
    pub fn import_cert(&mut self, role: CertRole, cert_pem: &str,
                       key_pem: Option<&str>) -> Result<CertMeta>;
    pub fn import_crl(&mut self, crl_pem: &str) -> Result<()>;
    pub fn revoke_cert(&mut self, cert_id: &str) -> Result<()>;
    pub fn get_cert_chain(&self, role: CertRole) -> Result<CertChain>;
    pub fn check_cert_valid(&self, cert_id: &str) -> Result<bool>;
    pub fn list_certs(&self) -> Vec<CertMeta>;
    pub fn reload(&mut self) -> Result<()>;
}
```

### 6.3 证书导入验证流程 [DESIGN_APPROVED]

```
Step 1: PEM 解码 → DER 解码 → X.509 v3 证书
Step 2: 签名算法 OID = SM2-with-SM3（1.2.156.10197.1.501）
Step 3: 公钥曲线 = sm2p256v1（1.2.156.10197.1.301）
Step 4: 有效期检查（notBefore ≤ now ≤ notAfter）
Step 5: 证书链验证（按 Issuer 匹配，递归至自签名 CA）
Step 6: CRL 检查（如 CRL 存在）
Step 7: 私钥匹配验证（如提供私钥）
Step 8: 保存到 /etc/mupc/certs/
```

### 6.4 证书热更新设计 [DESIGN_APPROVED]

```rust
/// 证书热更新处理器
pub struct HotReloadHandler {
    current: Arc<RwLock<CertStore>>,
    pending: Arc<RwLock<Option<CertStore>>>,
}

impl HotReloadHandler {
    /// 启动文件监控（notify crate）
    pub fn start_watching(&self, cert_dir: &Path) -> Result<()>;
    /// 尝试切换 pending 证书（原子切换）
    pub fn try_switch(&mut self) -> Result<bool>;
}
```

**热更新策略：** 旧连接保持旧证书（直至断开），新建连接使用新证书。`GracefulSwitch` 模式为推荐行为。

**自动重载定时器：** 每 10 分钟检测一次文件变更。

### 6.5 CRL 管理

```rust
pub struct CrlManager {
    crls: HashMap<String, CertificateRevocationList>,
    check_enabled: bool,
    update_interval: Duration,
}

impl CrlManager {
    pub fn parse_crl(&mut self, crl_der: &[u8]) -> Result<()>;
    pub fn check_revoked(&self, cert_serial: &[u8]) -> Result<bool>;
}
```

### 6.6 验收标准 [REVIEWED: PASS]

| ID | 验收内容 | 优先级 |
|----|----------|--------|
| LEA-52 | 支持 Web 上传 + SCP 两种证书导入方式 | P1 |
| LEA-53 | 导入时自动验证格式/算法/有效期/证书链 | P1 |
| LEA-54 | 私钥文件权限 600，/etc/mupc/certs/ | P1 |
| LEA-55 | 支持三级证书链存储 | P1 |
| LEA-56 | 热更新：已有连接不受影响 | P1 |
| LEA-57 | 到期前 30 天 WARNING，7 天 CRITICAL 告警 | P1 |
| LEA-58 | 过期后已有连接维持，新连接拒绝 | P1 |
| LEA-59 | 每 10 分钟自动重载证书 | P1 |
| LEA-60 | CRL 导入及验证 | P1 |
| LEA-61 | CRL 检查可开关，可配置更新频率 | P1 |
| LEA-62 | 吊销审计日志 | P1 |
| LEA-63 | 吊销不影响已有连接 | P1 |

---

## 7. 控制指令全链路加密审计设计

### 7.1 加密流转架构

```
调度主站下发指令（IEC 104 APDU）
    │  IPSec VPN 隧道（SM4-GCM 加密整个 TCP 流）
    ▼
gateway（IEC 104）
  1. IPSec 隧道解密（内核 IPsec 已处理）
  2. 解析 APDU，提取控制指令明文
  3. SM2 验签（验调度主站签名）
  4. SM4-GCM 加密指令 payload
  5. 生成加密上下文 + 签名
  6. 发往消息总线（EncryptedControlCommand）
    │
    ▼
strategy-engine
  1. 重放防护（序列号检查）
  2. SM2 验签（验 gateway 签名）
  3. SM4-GCM 解密
  4. 策略校验（削峰填谷、需量控制等）
  5. 用 intercore 密钥重新加密 + 签名
  6. 发往消息总线
    │
    ▼
intercore
  1. SM2 验签（验 strategy-engine 签名）
  2. SM4-GCM 解密
  3. 组装 IntercoreFrame
  4. TCP 会话级 SM4-GCM 加密
  5. 发送到实时控制模块
    │  RJ45 TCP（SM4-GCM 加密帧）
    ▼
实时控制模块 — 解密 TCP 帧 → 执行
```

### 7.2 加密上下文

```rust
/// 加密上下文：跨模块传递加解密信息
pub struct EncryptionContext {
    pub cmd_id: String,             // 指令唯一 ID（UUID v4）
    pub timestamp_ms: u64,          // 时间戳（epoch ms）
    pub source: String,             // 源模块标识
    pub target: String,             // 目标模块标识
    pub cipher: String,             // "SM4-GCM"
    pub signature_algo: String,     // "SM2-WITH-SM3"
    pub key_id: String,
    pub signing_key_id: String,
    pub seq_no: u64,                // 指令序列号（递增 64 位）
    pub replay_window_ms: u64,      // 防重放时间窗口（默认 300000ms/5分钟）
}
```

### 7.3 全链路传递协议

```rust
/// 加密控制指令（消息总线传递格式）
pub struct EncryptedControlCommand {
    pub context: EncryptionContext,         // 明文上下文
    pub encrypted_payload: Vec<u8>,         // SM4-GCM 密文
    pub iv: Vec<u8>,                       // SM4-GCM IV
    pub signature: Vec<u8>,                 // SM2 签名（context + encrypted_payload）
    pub signer_fingerprint: String,         // 发送模块公钥指纹
}
```

### 7.4 TCP 会话级加密（intercore）

```rust
/// TCP 会话级 SM4-GCM 加密器
pub struct Sm4GcmSessionEncryptor {
    session_key: [u8; 16],     // SM2 密钥交换派生
    send_counter: u64,          // GCM nonce 发送端序列号
    recv_counter: u64,          // GCM nonce 接收端序列号
}

impl Sm4GcmSessionEncryptor {
    /// 使用 SM2 密钥交换派生会话密钥
    pub fn from_key_exchange(local_key: &Sm2KeyPair, peer_public_key: &[u8]) -> Result<Self>;
    /// 加密帧
    pub fn encrypt_frame(&mut self, frame: &[u8]) -> Result<Vec<u8>>;
    /// 解密帧
    pub fn decrypt_frame(&mut self, encrypted: &[u8]) -> Result<Vec<u8>>;
}
```

### 7.5 重放防护

```rust
/// 重放防护器
pub struct ReplayProtector {
    window: ReplayWindow,       // 滑动窗口（5 分钟）
    last_cleanup: Instant,
}

impl ReplayProtector {
    pub fn check(&mut self, seq_no: u64, timestamp_ms: u64) -> Result<()> {
        // 1. 时间窗口检查（5 分钟）
        // 2. 序列号重复检查（BTreeSet）
        Ok(())
    }
}

struct ReplayWindow {
    seqs: BTreeSet<u64>,
    max_size: usize,
}
```

### 7.6 加密审计日志

```rust
/// 加密审计日志记录器
pub struct AuditLogger {
    log_dir: PathBuf,
    current_writer: Option<BufWriter<File>>,
    file_seq: u64,
    last_hash: Vec<u8>,          // 上次 SM3 哈希（日志链）
    chain: Vec<LogChainEntry>,   // 日志完整性链
}

/// 审计日志条目
pub struct AuditLogEntry {
    pub log_id: String,
    pub timestamp_ms: u64,
    pub cmd_id: String,
    pub operation: AuditOperation,  // Encrypt/Decrypt/Sign/Verify/...
    pub source_module: String,
    pub target_module: String,
    pub cipher_algo: String,
    pub cipher_key_id: String,
    pub sign_algo: String,
    pub sign_key_id: String,
    pub seq_no: u64,
    pub result: AuditResult,        // Success/Failure
    pub error_msg: Option<String>,
}

/// 日志完整性链（SM3 哈希链）
pub struct LogChainEntry {
    pub file_path: String,
    pub hash: Vec<u8>,          // 当前文件 SM3 哈希
    pub prev_hash: Vec<u8>,     // 上一块哈希
    pub timestamp_ms: u64,
}
```

**日志滚动策略：**
```
/var/log/mupc/audit/
  - audit_000001.jsonl  (上限 100MB)
  - audit_000002.jsonl
  - ...

每 5 分钟计算一次 SM3 哈希，追加到鉴证文件
磁盘低于 200MB 时触发日志归档：gzip + 迁移到 /var/log/mupc/audit/archive/
保留最后 100MB 日志不删除（防丢日志兜底）
在线保留 30 天，历史日志归档
```

### 7.7 验收标准 [REVIEWED: PASS]

| ID | 验收内容 | 优先级 |
|----|----------|--------|
| LEA-36 | 控制指令 gateway→strategy→intercore 全链路 SM4 加密 | P0 |
| LEA-37 | RJ45 TCP 报文 SM4-GCM 加密 | P0 |
| LEA-38 | 唯一序列号防重放，5 分钟窗口 | P0 |
| LEA-39 | 解密/验签/序列号失败指令被拒绝并告警 | P0 |
| LEA-40 | 全链路延迟增量 ≤ 10ms | P0 |
| LEA-41 | 审计日志包含完整加密链路信息 | P0 |
| LEA-42 | SM3 哈希链防篡改（每 5 分钟校验） | P1 |
| LEA-43 | 多维度审计查询和导出 | P1 |
| LEA-44 | 日志 ≥ 1GB，30 天在线 + 归档 | P1 |

---

## 8. 安全告警与合规仪表盘设计

### 8.1 安全事件告警

```rust
/// 告警严重级别
pub enum AlertSeverity { Critical, Error, Warning, Info }

/// 告警事件
pub struct AlertEvent {
    pub alert_id: u64,
    pub timestamp_ms: u64,
    pub severity: AlertSeverity,
    pub category: AlertCategory,
    pub description: String,
    pub impact: String,
    pub suggestion: String,
    pub source: String,
}

/// 告警分类
pub enum AlertCategory {
    CertificateExpiring,         // 证书即将过期
    CertificateExpired,         // 证书已过期
    CertificateVerifyFailed,    // 证书签名验证失败
    DecryptFailed,             // SM4 解密失败
    TlsHandshakeFailed,        // TLS 握手失败
    TunnelDisconnected,        // 加密装置隧道断开
    TunnelRecovered,           // 加密装置隧道恢复
    AuthFailedExcessive,       // 连续身份认证失败
    AuditIntegrityBroken,      // 审计日志完整性破坏
    ReplayAttackDetected,      // 重放攻击检测
}
```

### 8.2 告警推送通道

```rust
#[async_trait]
pub trait AlertSink: Send + Sync {
    async fn send(&self, alert: &AlertEvent) -> Result<()>;
    fn name(&self) -> &str;
}

// 内置推送通道
pub struct SyslogSink;          // syslog 输出
pub struct SnmpTrapSink;        // SNMP Trap v2c/v3（可选）
pub struct LedIndicatorSink;    // LED 运行指示灯（GPIO 控制）
pub struct BusAlertSink;        // 消息总线告警（供 Web API 读取）
pub struct LogSink;             // tracing 日志输出
```

### 8.3 告警规则

| 等级 | 触发条件 | 上报通道 |
|:---:|----------|----------|
| CRITICAL | 任何一级验签失败、dm-verity 篡改、OTP 异常 | tracing ERROR + IEC 104 遥信 + MQTT 告警 |
| CRITICAL | 证书已过期 | tracing ERROR + MQTT |
| WARNING | 防回滚计数器 ≥ 80%、证书 30 天到期 | tracing WARN + MQTT |
| WARNING | 运行时完整性校验发现篡改、隧道断开 | tracing WARN + MQTT |
| INFO | 安全启动状态变更、恢复模式触发 | tracing INFO |

### 8.4 合规自检引擎

```rust
/// 合规自检引擎
pub struct ComplianceChecker {
    security: Arc<SecurityService>,
    cert_mgr: Arc<CertManager>,
    policy_mgr: Arc<PolicyManager>,
}

impl ComplianceChecker {
    /// 执行全量合规自检
    pub async fn check_all(&self) -> ComplianceReport {
        // 1. 国密算法可用性
        // 2. 加密策略完整性
        // 3. 证书有效性
        // 4. 加密隧道状态
        // 5. TLS 配置
        // 6. 审计日志完整性
        // 7. 发改委 14 号令第十四条检查
        // 8. GB/T 36572-2018 第 6.3 节检查
        // 9. 等保 2.0 三级安全通信网络检查
    }
}
```

### 8.5 合规仪表盘数据接口

```rust
pub struct DashboardData {
    pub overall_status: ComplianceStatus,       // Pass/Warning/Failed
    pub categories: HashMap<String, ComplianceStatus>,
    pub cert_stats: CertStats,
    pub tunnel_stats: TunnelStats,
    pub alert_trend: Vec<AlertTrendPoint>,      // 近 24 小时
    pub last_check_time: u64,
    pub next_check_time: u64,
}

pub const DASHBOARD_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
```

### 8.6 合规仪表盘 Web API

```
GET    /api/v1/security/compliance              # 获取合规自检报告
POST   /api/v1/security/compliance/check        # 触发合规自检
GET    /api/v1/security/compliance/dashboard    # 获取仪表盘数据

GET    /api/v1/security/certs                   # 列出所有证书
POST   /api/v1/security/certs/import            # 导入证书
DELETE /api/v1/security/certs/:id               # 吊销证书
POST   /api/v1/security/certs/crl               # 导入 CRL

GET    /api/v1/security/tunnels                 # 列出加密隧道
POST   /api/v1/security/tunnels                 # 创建隧道
DELETE /api/v1/security/tunnels/:name           # 删除隧道

GET    /api/v1/security/policy                  # 获取加密策略
PUT    /api/v1/security/policy                  # 更新加密策略

GET    /api/v1/security/alerts                  # 获取告警列表
GET    /api/v1/security/audit-log               # 查询审计日志
```

### 8.7 验收标准 [REVIEWED: PASS]

| ID | 验收内容 | 优先级 |
|----|----------|--------|
| LEA-45 | 9 类安全事件必须告警 | P1 |
| LEA-46 | 告警含 7 个标准字段 | P1 |
| LEA-47 | 支持 syslog/SNMP/LED 三种推送 | P1 |
| LEA-48 | CRITICAL 告警 ≤ 10 秒 | P1 |
| LEA-49 | 仪表盘展示加密/证书/隧道/告警状态 | P1 |
| LEA-50 | 红/黄/绿三色标识合规状态 | P1 |
| LEA-51 | 每 60 秒自动刷新 | P1 |

---

## 9. 接口定义

### 9.1 加密原语接口（security crate 公开 API）

| 接口 | 所在模块 | 说明 |
|------|----------|------|
| `sm2_sign(data, key_pem) -> Result<Vec<u8>>` | sm2 | SM2 签名 |
| `sm2_verify(data, sig, key_pem) -> Result<bool>` | sm2 | SM2 验签 |
| `sm2_key_generate() -> Result<Sm2KeyPair>` | sm2 | SM2 密钥生成 |
| `sm2_derive_shared_key(kp, peer_pub) -> Result<Vec<u8>>` | sm2 | ECDH 共享密钥派生 |
| `sm3_hash(data) -> Result<Vec<u8>>` | sm3 | SM3 哈希计算 |
| `sm3_derive_key(ikm, salt, info, len) -> Result<Vec<u8>>` | sm3 | HKDF-SM3 密钥派生 |
| `sm4_gcm_encrypt(data, key, iv) -> Result<Vec<u8>>` | sm4 | SM4-GCM 加密 |
| `sm4_gcm_decrypt(data, key, iv) -> Result<Vec<u8>>` | sm4 | SM4-GCM 解密 |
| `sm4_cbc_encrypt(data, key, iv) -> Result<Vec<u8>>` | sm4 | SM4-CBC 加密 |
| `sm4_cbc_decrypt(data, key, iv) -> Result<Vec<u8>>` | sm4 | SM4-CBC 解密 |
| `load_sm2_certificate(path) -> Result<Sm2Cert>` | cert | 加载 SM2 证书 |
| `build_sm2_tls_client_config(...) -> Result<ClientConfig>` | tls | 构建国密 TLS 配置 |

### 9.2 安全启动服务接口（SecureBootService）

| 方法 | 说明 |
|------|------|
| `initialize() -> Result<SecureBootStatus>` | 初始化：读取内核安全启动状态 |
| `get_status() -> Result<SecureBootStatus>` | 获取当前安全启动状态 |
| `get_anti_rollback_counter() -> Result<u32>` | 获取防回滚计数器值 |
| `log_event(event: BootEvent) -> Result<()>` | 记录安全启动事件日志 |
| `query_logs(filter: LogFilter) -> Result<Vec<BootEvent>>` | 查询安全启动日志 |
| `run_integrity_check() -> Result<IntegrityReport>` | 执行运行时完整性检查 |
| `run_health_check() -> Result<HealthReport>` | 执行健康检查 |

### 9.3 纵向加密认证接口（LeaManager）

| 方法 | 说明 |
|------|------|
| `add_tunnel(config: LeaConfig) -> Result<String>` | 创建加密隧道 |
| `remove_tunnel(name: &str) -> Result<()>` | 删除加密隧道 |
| `list_tunnels() -> Vec<(String, TunnelState)>` | 列出所有隧道状态 |
| `get_tunnel_status(name: &str) -> Result<TunnelState>` | 获取单条隧道状态 |

### 9.4 证书管理接口（CertManager）

| 方法 | 说明 |
|------|------|
| `import_cert(role, cert_pem, key_pem) -> Result<CertMeta>` | 导入证书 |
| `import_crl(crl_pem) -> Result<()>` | 导入 CRL |
| `revoke_cert(cert_id) -> Result<()>` | 吊销证书 |
| `get_cert_chain(role) -> Result<CertChain>` | 获取证书链 |
| `check_cert_valid(cert_id) -> Result<bool>` | 检查证书有效性 |
| `list_certs() -> Vec<CertMeta>` | 列出所有证书 |
| `reload() -> Result<()>` | 手动触发重新加载 |

### 9.5 合规自检接口（ComplianceChecker）

| 方法 | 说明 |
|------|------|
| `check_all() -> ComplianceReport` | 执行全量合规自检 |
| `get_dashboard() -> DashboardData` | 获取仪表盘数据 |

### 9.6 Web API 接口

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/security/boot-status` | 安全启动状态 |
| GET | `/api/security/boot-log` | 安全启动日志 |
| GET | `/api/security/health` | 健康检查 |
| POST | `/api/security/integrity-check` | 触发完整性检查 |
| GET | `/api/v1/security/compliance` | 合规自检报告 |
| POST | `/api/v1/security/compliance/check` | 触发合规自检 |
| GET | `/api/v1/security/compliance/dashboard` | 合规仪表盘 |
| GET | `/api/v1/security/certs` | 证书列表 |
| POST | `/api/v1/security/certs/import` | 导入证书 |
| DELETE | `/api/v1/security/certs/:id` | 吊销证书 |
| POST | `/api/v1/security/certs/crl` | 导入 CRL |
| GET | `/api/v1/security/tunnels` | 加密隧道列表 |
| POST | `/api/v1/security/tunnels` | 创建隧道 |
| DELETE | `/api/v1/security/tunnels/:name` | 删除隧道 |
| GET | `/api/v1/security/policy` | 加密策略 |
| PUT | `/api/v1/security/policy` | 更新加密策略 |
| GET | `/api/v1/security/alerts` | 告警列表 |
| GET | `/api/v1/security/audit-log` | 审计日志查询 |

### 9.7 签名工具链 CLI

| 工具 | 功能 | 示例 |
|------|------|------|
| `mupc-keygen` | 生成安全启动密钥集 | `mupc-keygen --algo rsa4096 --output ./keys/` |
| `mupc-sign` | 签名各级镜像 | `mupc-sign --key ./key.pem --type fit --input Image --output Image.sig` |
| `mupc-otp` | 生成 OTP 烧录文件 | `mupc-otp prepare --key-hash root_key_hash.bin --output otp.bin` |
| `mupc-verity` | 生成 dm-verity 哈希树 | `mupc-verity setup --rootfs rootfs.img --hash rootfs.hash` |
| `mupc-rollback-ctl` | 管理防回滚计数器 | `mupc-rollback-ctl read / write --value 5` |

---

## 10. 文件结构

### 10.1 security crate 完整文件结构

```
mupc/crates/security/
├── Cargo.toml                              # [修改] 新增依赖
├── src/
│   ├── lib.rs                              # [修改] 模块导出
│   ├── errors.rs                           # [修改] 新增错误变体
│   │
│   │  ── 国密算法层 ──
│   ├── sm2.rs                              # [重构] SM2 签名/验签/密钥交换
│   ├── sm3.rs                              # [新建] SM3 消息摘要
│   ├── sm4.rs                              # [重构] SM4 GCM + CBC
│   ├── cert.rs                             # [修改] SM2 证书解析与加载
│   │
│   │  ── TLS 层 ──
│   ├── tls.rs                              # [修改] SM2 TLS 配置入口
│   ├── tls_sm2.rs                          # [新增] SM2 TLS CryptoProvider
│   │   ├── struct SmCryptoProvider
│   │   ├── struct Sm2ServerCertVerifier
│   │   ├── struct Sm2ClientCertVerifier
│   │   ├── struct Sm2SigningKey
│   │   ├── struct Sm2KxGroup
│   │   └── struct Sm4GcmCipher
│   │
│   │  ── 纵向加密认证 ──
│   ├── lea.rs                              # [新增] 纵向加密认证模块
│   │   ├── struct LeaConfig
│   │   ├── struct LeaManager
│   │   ├── struct LeaMonitor
│   │   └── enum TunnelState
│   ├── lea_vici.rs                         # [新增] strongSwan VICI 协议客户端
│   │   ├── struct ViciClient
│   │   ├── struct ViciEncoder
│   │   ├── struct ViciDecoder
│   │   └── fn parse_vici_response()
│   │
│   │  ── 证书生命周期管理 ──
│   ├── cert_mgr.rs                         # [新增] 证书生命周期管理
│   │   ├── struct CertManager
│   │   ├── struct CertMeta
│   │   ├── enum CertRole
│   │   ├── struct HotReloadHandler
│   │   └── struct CrlManager
│   │
│   │  ── 审计与告警 ──
│   ├── audit.rs                            # [新增] 加密审计日志
│   │   ├── struct AuditLogger
│   │   ├── struct AuditLogEntry
│   │   ├── enum AuditOperation
│   │   ├── struct LogChainEntry
│   │   ├── struct ReplayProtector
│   │   └── struct ReplayWindow
│   ├── alarm.rs                            # [新增] 安全事件告警
│   │   ├── struct AlertManager
│   │   ├── struct AlertEvent
│   │   ├── enum AlertSeverity
│   │   ├── enum AlertCategory
│   │   ├── trait AlertSink
│   │   ├── struct SyslogSink
│   │   ├── struct SnmpTrapSink
│   │   └── struct LedIndicatorSink
│   │
│   │  ── 合规与策略 ──
│   ├── policy.rs                           # [新增] 加密策略管理
│   │   ├── struct PolicyManager
│   │   ├── struct ChannelPolicy
│   │   ├── enum EncryptionMode
│   │   └── enum SignatureMode
│   ├── compliance.rs                       # [新增] 合规自检
│   │   ├── struct ComplianceChecker
│   │   ├── struct ComplianceReport
│   │   ├── struct ComplianceCheckResult
│   │   ├── enum ComplianceStatus
│   │   └── struct DashboardData
│   │
│   │  ── 安全启动模块 ──
│   └── secure_boot/                        # [新增] 安全启动模块
│       ├── mod.rs                          # 模块入口 + SecureBootService
│       ├── status.rs                       # 安全启动状态
│       ├── monitor.rs                      # 运行时完整性监控
│       ├── audit.rs                        # 安全审计日志
│       ├── health.rs                       # 健康检查
│       └── rollback.rs                     # 防回滚接口
│
└── tests/
    ├── sm2_tests.rs                        # [更新] SM2 测试
    ├── sm3_tests.rs                        # [新建] SM3 测试
    ├── sm4_tests.rs                        # [更新] SM4 测试
    ├── cert_tests.rs                       # [更新] 证书测试
    ├── secure_boot_test.rs                 # [新建] 安全启动测试
    └── integration_tests.rs                # [新建] 集成测试
```

### 10.2 签名工具链文件结构

```
tools/mupc-signing-tool/                   # 独立于 workspace 的工具链项目
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── bin/
│   │   ├── keygen.rs                      # 密钥生成
│   │   ├── sign.rs                        # 签名工具
│   │   ├── otp.rs                         # OTP 烧录
│   │   ├── verity.rs                      # dm-verity 哈希树生成
│   │   └── rollback_ctl.rs                # 防回滚计数器管理
│   ├── signer/
│   │   ├── mod.rs
│   │   ├── spl.rs
│   │   ├── fit.rs
│   │   ├── uboot.rs
│   │   └── recovery.rs
│   ├── verity/
│   │   ├── mod.rs
│   │   └── hash_tree.rs
│   ├── otp/
│   │   ├── mod.rs
│   │   └── fuses.rs
│   └── key/
│       ├── mod.rs
│       ├── generate.rs
│       ├── store.rs
│       └── manifest.rs
└── tests/
    ├── integration_test.rs
    └── test_keys/
```

### 10.3 其他 crate 修改文件

```
mupc/crates/gateway/
├── src/lib.rs                          # 集成 SecurityService
├── src/iec104/server.rs                # APDU 加密/签名
└── src/iec104/protocol.rs              # APDU 加密扩展

mupc/crates/strategy-engine/
├── Cargo.toml                          # 依赖 security
├── src/lib.rs                          # 集成 SecurityService
└── src/strategies.rs                    # 指令解密/重放防护

mupc/crates/intercore/
├── Cargo.toml                          # 依赖 security
├── src/lib.rs                          # 集成会话加密
├── src/protocol.rs                     # 加密帧扩展
└── src/secure_session.rs               # [新增] TCP 会话加密层

mupc/crates/mqtt-plugin/
├── Cargo.toml                          # 依赖 security
├── src/config.rs                       # 国密配置扩展
├── src/client.rs                       # SM4 应用层加解密
└── src/sm4_cipher.rs                   # [新增] SM4 MQTT 载荷加密器

mupc/crates/web-api/
├── src/routes/security.rs              # [新增] 安全配置管理接口
├── src/routes/compliance.rs            # [新增] 合规仪表盘接口
└── src/routes/certificate.rs           # [新增] 证书管理接口
```

### 10.4 新增配置文件

```
/etc/mupc/
├── security-policy.yaml
│   channels:
│     - name: "iec104-master"
│       encryption: sm4-gcm
│       signature: sm2-with-sm3
│       enabled: true
│     - name: "mqtt-platform"
│       encryption: sm4-gcm
│       payload_encryption: true
│       signature: sm2-with-sm3
│       enabled: true
│   compliance:
│     auto_check: true
│     check_interval_secs: 3600
│
├── certs/
│   ├── certs.json                      # 证书元数据索引
│   ├── ca/
│   │   └── root-ca.pem
│   ├── device/
│   │   ├── tls-server.pem + key
│   │   ├── tls-client.pem + key
│   │   └── ipsec.pem + key
│   └── crl/
│       └── ca.crl
│
└── /var/log/mupc/audit/               # 审计日志
    ├── audit_000001.jsonl
    ├── audit_000002.jsonl
    ├── archive/
    └── chain.dat                       # 日志哈希链文件
```

---

## 11. 技术决策记录

### ADR-001：国密算法库选型 — gmsm

| 项目 | 内容 |
|------|------|
| 决策 | 使用 gmsm 0.14 作为国密算法实现库 |
| 备选 | ring（模拟）、GmSSL C FFI、自研 |
| 理由 | 纯 Rust、无外部 C 依赖、支持 SM2/SM3/SM4/x509、Apache-2.0/MIT 许可 |
| 状态 | 已采纳 |

### ADR-002：安全启动方案 — U-Boot Verified Boot

| 项目 | 内容 |
|------|------|
| 决策 | 采用 U-Boot Verified Boot（FIT 签名）方案 |
| 备选 | GRUB+shim、自定义 bootloader |
| 理由 | RK3588 BSP 原生支持、成熟度高、维护成本低 |
| 状态 | [DESIGN_APPROVED] |

### ADR-003：IPSec VPN — strongSwan VICI 协议集成

| 项目 | 内容 |
|------|------|
| 决策 | 通过 Unix Socket 调用 strongSwan VICI 协议管理 IPSec |
| 备选 | 自研 Rust IPSec 库、libipsec FFI |
| 理由 | Rust 生态无成熟 IPSec IKEv2 实现；strongSwan 是业界标准 IPSec 实现 |
| 状态 | [DESIGN_APPROVED] |

### ADR-004：rustls SM2 支持策略 — 分阶段实施

| 项目 | 内容 |
|------|------|
| 决策 | Phase 1 使用 GmSSL/Tongsuo 动态库；Phase 2 自定义 CryptoProvider；Phase 3 提交上游 PR |
| 备选 | 直接 Fork rustls |
| 理由 | rustls 0.23 的 SignatureScheme 枚举不包含 SM2_SIG_SM3，上游合并需要时间 |
| 状态 | 待定 |

### ADR-005：合规功能扩展策略 — 在 security crate 内扩展

| 项目 | 内容 |
|------|------|
| 决策 | 不新建 crate，所有安全合规功能在现有 security crate 内扩展 |
| 备选 | 新建 mupc-compliance crate |
| 理由 | 新模块直接调用 SM2/SM4/SM3 内部接口、共享内部状态（证书存储/审计日志/策略）、减少编译单元 |
| 状态 | 已采纳 |

### ADR-006：加密降级策略 — 严格禁止降级到非国密

| 项目 | 内容 |
|------|------|
| 决策 | SM2/SM4 协商失败时禁止降级到 RSA/AES |
| 备选 | 允许降级到 RSA/AES（过渡期兼容） |
| 理由 | 法规强制要求电力监控系统使用国密；降级路径破坏合规性 |
| 状态 | [REVIEWED: PASS] |

### ADR-007：密钥存储策略 — 生产环境离线 HSM

| 项目 | 内容 |
|------|------|
| 决策 | 生产环境私钥存储于离线 HSM；开发测试私钥加密文件存储 |
| 备选 | 纯软件存储、PKCS#11 HSM |
| 理由 | 离线 HSM 提供最高安全级别；RK3588 不支持内置 HSM |
| 状态 | [DESIGN_APPROVED] |

### ADR-008：BootROM 签名算法 — RSA-4096

| 项目 | 内容 |
|------|------|
| 决策 | BootROM 阶段签名算法固定为 RSA-4096 PKCS#1 v1.5 |
| 备选 | SM2 |
| 理由 | RK3588 BootROM 固件算法固化于芯片 ROM，不支持 SM2 |
| 状态 | [DESIGN_APPROVED] |

### ADR-009：防回滚计数器 — OTP 位图方式

| 项目 | 内容 |
|------|------|
| 决策 | 使用 OTP 7 个 word（224 bits）的位图编码实现单调递增计数器 |
| 备选 | 专用计数器寄存器、eMMC 存储 |
| 理由 | OTP 不可回写保证安全性；224 位宽覆盖约 9 年 OTA 更新 |
| 状态 | [DESIGN_APPROVED] |

### ADR-010：证书热更新 — GracefulSwitch 模式

| 项目 | 内容 |
|------|------|
| 决策 | 旧连接维持旧证书直至断开，新建连接使用新证书 |
| 备选 | ImmediateSwitch（立即切换） |
| 理由 | 零业务中断；符合安全管理员预期行为 |
| 状态 | 已采纳 |

---

## 附录 A：性能指标

| 指标 | 要求 | 测量方法 |
|------|------|----------|
| SM2 签名 P99 | ≤ 2ms（RK3588 单核） | 连续 1000 次 |
| SM2 验签 P99 | ≤ 3ms（RK3588 单核） | 连续 1000 次 |
| SM4-GCM 加密吞吐 | ≥ 200 MB/s | 1MB × 100 次 |
| SM4-GCM 解密吞吐 | ≥ 200 MB/s | 1MB × 100 次 |
| SM3 哈希性能 | ≥ 500 MB/s | 1MB × 100 次 |
| TLS 完整握手 P99 | ≤ 100ms | 连续 100 次 |
| IKEv2 握手 P99 | ≤ 500ms | 连续 50 次 |
| 全链路加密额外延迟 | ≤ 10ms/指令 | 连续 1000 次 |
| 安全启动总时长 | ≤ 15 秒 | 上电到系统就绪 |
| dm-verity 性能影响 | ≤ 5% | 基准测试对比 |
| 运行时完整性检查 CPU | ≤ 2% | RK3588 A76 |
| 证书导入时间 | ≤ 1 秒/张 | 含格式验证和链验证 |

## 附录 B：加密算法基础数据

| 算法 | 密钥长度 | 安全强度 | 等效 RSA 长度 |
|------|----------|----------|---------------|
| RSA | 4096 bit | 128 位 | — |
| SM2 | 256 bit（曲线） | 128 位 | 3072 |
| SHA-256 | 256 bit | 128 位 | — |
| SM3 | 256 bit | 128 位 | — |
| SM4 | 128 bit | 128 位 | — |

## 附录 C：法规与标准引用

| 标准/法规 | 条款 | 对应功能 |
|-----------|------|----------|
| 国家发改委 2014 年第 14 号令 | 第十四条：纵向加密认证 | 第 4 章 |
| GB/T 36572-2018 | 第 6.3 节：通信安全 | 第 4 章 |
| 等保 2.0 三级 | 安全通信网络 | 第 4 章、第 8 章 |
| GM/T 0022-2014《IPSec VPN 技术规范》 | 国密 IPSec 规范 | 第 4 章 |
| RFC 8998《SM Cipher Suites for TLS 1.3》 | Section 2 | 第 3 章 |
| GM/T 0002-2012《SM4 分组密码算法》 | 完整标准 | 第 2 章 |
| GM/T 0003-2012《SM2 椭圆曲线公钥密码算法》 | 完整标准 | 第 2 章 |
| GM/T 0004-2012《SM3 密码杂凑算法》 | 完整标准 | 第 2 章 |

## 附录 D：错误码速查

| 错误码 | 阶段 | 含义 |
|--------|:---:|------|
| `GmError::KeyLoadFailed` | 通用 | 密钥加载/解析失败 |
| `GmError::SignFailed` | SM2 | 签名操作失败 |
| `GmError::VerifyFailed` | SM2 | 验签操作失败 |
| `GmError::EncryptFailed` | SM4 | 加密操作失败 |
| `GmError::DecryptFailed` | SM4 | 解密操作失败 |
| `GmError::InvalidKeyLength` | SM4 | 无效密钥长度（非 16 字节） |
| `GmError::KeyGenerationFailed` | SM2 | 密钥生成失败 |
| `GmError::KeyDeriveFailed` | SM2 | 密钥派生失败 |
| `GmError::CertVerifyFailed` | 证书 | 证书验证失败 |
| `GmError::TlsConfigError` | TLS | TLS 配置错误 |
| `GmError::ReplayDetected` | 审计 | 重放攻击检测 |
| `ERR_BOOTROM_SIG_FAIL` | BootROM | SPL 签名验证失败 |
| `ERR_SPL_SIG_FAIL` | SPL | U-Boot 签名验证失败 |
| `ERR_UBOOT_FIT_SIG_FAIL` | U-Boot | FIT 签名失败 |
| `ERR_ROLLBACK_DETECTED` | U-Boot | 防回滚计数器检测到降级 |
| `ERR_DM_VERITY_CORRUPTION` | Runtime | dm-verity 数据块篡改 |

## 附录 E：未解决问题与风险

| # | 问题 | 影响 | 状态 |
|---|------|------|------|
| 1 | RK3588 OTP 可用存储空间精确大小 | 阻塞：影响密钥体系设计 | 待确认 |
| 2 | openEuler BSP 内核是否默认使能 dm-verity | 阻塞：影响 dm-verity 方案 | 待确认 |
| 3 | U-Boot 2023.07+ 是否原生支持 SM2 FIT 验签 | 阻塞：若不支持需评估国密路线 | 待确认 |
| 4 | rustls 自定义 CryptoProvider 稳定性 | 中等：SM2 支持可能需要维护 fork | 持续监控 |
| 5 | strongSwan 进程管理（MUPC 是否负责启动 charon） | 中等：影响部署方案 | 待决策 |
| 6 | SNMP Trap SNMP 库成熟度（`snmp` crate 年久失修） | 低：可降级为 syslog + Webhook | 待决策 |
| 7 | HSM 对接（PKCS#11 `cryptoki` crate） | 低：后续阶段可选 | 待规划 |
| 8 | 证书自动续期（ACME 协议） | 低：当前依赖手动导入 | 待规划 |
| 9 | SM2 签名缺失 (gmsm 0.1.0 无签名API) | fake_gmsm 回退至 ring ECDSA P-256，非真正 SM2 | gmsm 0.1.0 不支持 sm2_sign；需等待 gmsm 0.14 发布 | P0 |
| 10 | SM4 GCM 回退至 ring AES-128-GCM | fake_gmsm 回退至 ring AES-256-GCM（key 16→32 扩展），非真正 SM4-GCM | gmsm 0.1.0 不支持 SM4 GCM 模式 | P0 |
| 11 | SM3 HKDF 未实现 | 任何 feature 下均返回 Err(GmError::InvalidParam) | gmsm 0.1.0 无 HkdfSm3 API | P1 |
| 12 | SM2 ECDH 未实现 | 任何 feature 下均返回 Err(GmError::InvalidParam) | gmsm 0.1.0 无 derive_shared_secret API | P1 |
| 13 | gmsm 版本 0.1.0 远低于计划的 0.14 | 缺少签名/GCM/HKDF/ECDH API，当前仅 SM3 hash + SM4 CBC + SM2 keygen 可用 | 上游 gmsm crate 尚未发布 0.14 版本 | P0 |

---

**文档状态：** 合并版 v1.0 — 从四份源文档中提取并整合为统一的设计文档。

**保留的设计审批标记：**
- `[DESIGN_APPROVED]` — 安全启动设计文档（第 5 章）
- `[REVIEWED: PASS]` — 安全 PRD 的验收标准章节

---

## 12. Phase 2B 实现笔记（安全部分）

> **来源**: `docs/superpowers/reports/2026-05-27-MUPC-Phase2B-协议安全-实施计划.md`（已归档）
> **状态**: DRAFT
> **团队**: 团队B（1人）

### 12.1 安全组件实施任务

| Task | 内容 | 提交信息 |
|------|------|----------|
| Task 3 | security crate：SM2 签名/验签 → SM4 加密/解密 → 证书管理 → TLS 连接器 | `feat(security): 实现国密 SM2/SM4 和 TLS 支持` |

### 12.2 安全模块技术选型

| 组件 | 选型 | 说明 |
|------|------|------|
| SM2 签名/验签 | GmSSL / `ring` | 国密椭圆曲线签名算法 |
| SM4 加密/解密 | GmSSL / `ring` | 国密分组密码算法 |
| TLS | `rustls` | 纯 Rust TLS 实现，支持自定义 CryptoProvider |
| 证书管理 | `rustls` + X.509 | 证书加载、验证、轮换 |

### 12.3 安全里程碑

| 里程碑 | 内容 | 交付物 |
|--------|------|--------|
| M2.5 | 安全组件 | security crate（sm2.rs, sm4.rs, cert.rs, tls.rs） |

### 12.4 实施风险

| 风险 | 等级 | 对策 |
|------|------|------|
| 国密库 Rust 支持不完善 | 中 | 预备纯软件实现方案（参考 GmSSL Rust 绑定） |
| TLS 性能开销 | 中 | 优化连接复用，减少握手次数 |
| `rustls` 自定义 CryptoProvider 稳定性 | 中 | 持续监控，可能需要维护 fork |

---

## 13. SM2/SM4 国密实现笔记

> **来源**: `docs/superpowers/plans/2026-05-28-SM2-SM4-国密真正实现-实施计划.md`（原始实施计划，已归档至 reports/）
> **状态**: 部分实现 -- SM3 hash / SM4 CBC / SM2 密钥生成已完成；SM2 签名 / SM4 GCM / SM3 HKDF / SM2 ECDH 待 gmsm 升级至 0.14

### 13.1 Feature Flag 架构 (real_gmsm vs fake_gmsm)

所有国密函数均通过 Rust 条件编译 (`#[cfg(feature = "real_gmsm")]` / `#[cfg(not(feature = "real_gmsm"))]`) 实现双路径分发：

```toml
# security/Cargo.toml
[features]
default = ["real_gmsm"]
real_gmsm = ["dep:gmsm"]
fake_gmsm = ["dep:ring"]
```

| Feature | 依赖 | 算法实现 | 适用场景 |
|---------|------|----------|----------|
| `real_gmsm`（默认） | gmsm 0.1.0（规划升级 0.14） | 真正国密 SM2/SM3/SM4 | 生产环境 |
| `fake_gmsm` | ring 0.16 | ring ECDSA P-256 / AES-256-GCM 模拟 | CI 测试 / 开发环境 |
| 无 feature | 无 | 所有函数返回 Err | 编译期验证 |

**关键约束**: 生产环境必须启用 `real_gmsm`。接口签名在两种 feature 下保持一致，业务代码无需修改。

### 13.2 gmsm 0.1.0 实际可用 API

gmsm 当前可用版本为 0.1.0（非计划的 0.14），API 覆盖有限。以下为实测可用的底层 API：

| 功能 | gmsm 0.1.0 API | 状态 | 说明 |
|------|---------------|------|------|
| SM3 哈希 | `sm3_byte` | 可用 | 输入 `&[u8]`，输出 32 字节哈希 |
| SM4 CBC 加密 | `sm4_cbc_encrypt_byte` | 可用 | 16 字节 key + 16 字节 IV，PKCS7 填充 |
| SM4 CBC 解密 | `sm4_cbc_decrypt_byte` | 可用 | 同上 |
| SM2 密钥生成 | `sm2_generate_key_hex` | 可用 | 返回十六进制字符串格式密钥对 |
| SM2 签名 | 无 | 不可用 | gmsm 0.1.0 未暴露签名 API |
| SM2 验签 | 无 | 不可用 | gmsm 0.1.0 未暴露验签 API |
| SM4 GCM 加密 | 无 | 不可用 | gmsm 0.1.0 无 AEAD/GCM 模式 |
| SM4 GCM 解密 | 无 | 不可用 | 同上 |
| SM3 HKDF | 无 | 不可用 | gmsm 0.1.0 无 HkdfSm3 |
| SM2 ECDH | 无 | 不可用 | gmsm 0.1.0 无 derive_shared_secret |
| X.509 证书 | gmsm::x509 | 不可用 | gmsm 0.1.0 x509 feature 不可用 |

### 13.3 Ring 回退策略（fake_gmsm 路径）

对于 gmsm 0.1.0 未提供的 API，`fake_gmsm` feature 路径使用 ring 库提供功能降级回退：

| 目标功能 | fake_gmsm 回退 | 回退算法 | 安全等级差异 |
|----------|---------------|----------|-------------|
| SM2 签名 | ring ECDSA P-256 SHA-256 | `ECDSA_P256_SHA256_FIXED_SIGNING` | 曲线不同（P-256 vs sm2p256v1），非真正 SM2 |
| SM2 验签 | ring ECDSA P-256 SHA-256 | `ECDSA_P256_SHA256_FIXED_VERIFICATION` + `UnparsedPublicKey` | 同上 |
| SM4 GCM 加密 | ring AES-256-GCM | `LessSafeKey::seal_in_place_separate_tag` | 算法不同（AES vs SM4），密钥 16→32 扩展 |
| SM4 GCM 解密 | ring AES-256-GCM | `LessSafeKey::open_in_place` | 同上 |
| SM3 哈希 | 返回 Err | `GmError::InvalidParam("SM3 需要 gmsm 库")` | 不可用 |
| SM3 HKDF | 返回 Err | `GmError::InvalidParam("HKDF-SM3 需要 gmsm 库")` | 不可用 |
| SM2 密钥生成 | 返回 Err | `GmError::InvalidParam("密钥生成需要 gmsm 库")` | 不可用 |
| SM2 ECDH | 返回 Err | `GmError::InvalidParam("共享密钥派生需要 gmsm 库")` | 不可用 |

#### SM4 GCM Ring 回退关键技术细节

由于 SM4 使用 128-bit（16 字节）密钥而 ring 的 `AES_256_GCM` 需要 256-bit（32 字节）密钥，回退实现通过**密钥自复制扩展**适配：

```rust
// 16 字节 SM4 key → 32 字节 AES-256 key
let mut expanded_key = [0u8; 32];
expanded_key[..16].copy_from_slice(key);
expanded_key[16..].copy_from_slice(key);  // 复制自身以适配 AES-256

let unbound_key = UnboundKey::new(&AES_256_GCM, &expanded_key)?;
let less_safe_key = LessSafeKey::new(unbound_key);

// 加密：密文尾部附带 16 字节认证标签
let tag = less_safe_key.seal_in_place_separate_tag(
    Nonce::assume_unique_is_key(iv[..12].try_into().unwrap()),
    Aad::empty(),
    &mut in_out,
)?;
```

此回退方案**不是真正的 SM4-GCM**，仅用于开发和 CI 环境功能验证。生产环境必须启用 `real_gmsm` 并等待 gmsm 0.14 的 SM4 GCM 支持。

#### SM2 签名 Ring 回退关键技术细节

```rust
// 使用 ring ECDSA P-256 模拟 SM2（非真正 SM2）
use ring::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};

let ecdsa_key_pair = EcdsaKeyPair::from_pkcs8(
    &ECDSA_P256_SHA256_FIXED_SIGNING,
    &private_key_bytes,
)?;
let signature = ecdsa_key_pair.sign(data)?;
```

### 13.4 gmsm 版本差距与升级路线

| 项目 | 当前 | 计划 | 差距 |
|------|------|------|------|
| gmsm 版本 | 0.1.0 | 0.14 | 缺少 sign/verify/GCM/HKDF/ECDH/x509 |
| 可用功能 | SM3 hash + SM4 CBC + SM2 keygen | 全部 SM2/SM3/SM4 + x509 | 5 项核心 API 缺失 |
| 阻塞项 | 上游未发布新版本 | -- | 等待 gmsm crate 发布 0.14 |

**临时缓解措施**：
- SM2 签名/验签：CI 环境使用 ring ECDSA P-256 模拟，生产环境等待 gmsm 0.14
- SM4 GCM：CI 环境使用 ring AES-256-GCM 模拟（含密钥扩展），生产环境等待 gmsm 0.14
- SM3 HKDF / SM2 ECDH：暂无回退方案，标记为 Unsupported
- SM2 证书（x509）：gmsm 0.1.0 x509 feature 不可用，生产环境需另行方案

### 13.5 SM2 证书管理实现要点

证书模块（`cert.rs`）的设计目标为通过 `gmsm::x509` 提供 SM2 X.509 v3 证书解析和验证：

```rust
#[cfg(feature = "real_gmsm")]
use gmsm::x509;

pub struct Sm2Cert { cert: x509::Certificate }

impl CertStore {
    pub fn from_pem_file(path: &str) -> Result<Self> {
        // 加载 PEM 编码证书 → gmsm::x509::Certificate::from_pem
    }
    pub fn verify_chain(&self, root: &Sm2Cert) -> Result<bool> {
        // 证书链验证（当前为占位实现，返回 true）
    }
}
```

**当前状态**: x509 feature 在 gmsm 0.1.0 中不可用，`CertStore::from_pem_file` 和 `verify_chain` 在 `real_gmsm` 下为占位实现。完整的证书解析、证书链验证、CRL 检查需等待 gmsm 0.14 的 x509 feature。

### 13.6 VICI Client 集成要点

IPSec 隧道管理通过 Unix Socket 与 strongSwan charon 守护进程的 VICI 协议通信：

```
security::lea_vici::ViciClient
    ↓ Unix Socket (/var/run/charon.vici)
strongSwan charon daemon
    ↓ IKEv2 + SM2 + SM4/SM3
IPSec SA（加密隧道）
```

**VICI 报文编码**（segment-based，4 字节大端长度前缀）：

| 类型字节 | 含义 |
|---------|------|
| 0x00 | 节段开始 (section start) |
| 0x01 | 节段结束 (section end) |
| 0x02 | 键值对（字符串值） |
| 0x03 | 列表项 (list item) |
| 0x04 | 键值对（原始字节值） |

**关键操作映射**：

| 操作 | VICI 命令 | 用途 |
|------|-----------|------|
| 加载证书 | `load-cert` | 将 SM2 PEM 证书加载到 charon |
| 加载私钥 | `load-key` | 加载 SM2 私钥 |
| 创建连接 | `load-conn` | 加载 IKEv2 连接配置 |
| 发起隧道 | `initiate` | 启动 IKEv2 握手 |
| 查询状态 | `list-sas` | 获取 SA 状态 |
| 关闭隧道 | `unload-conn` | 卸载连接配置 |

### 13.7 安全启动信任链实现要点

安全启动采用 **U-Boot Verified Boot (FIT 签名)** 方案，形成四级信任链：

```
BootROM (OTP 根公钥哈希)
  → U-Boot SPL (RSA-4096 验签)
    → U-Boot (RSA-4096/SM2 验签)
      → FIT 镜像 {kernel + DTB} (RSA-4096/SM2 验签)
        → RootFS (dm-verity 块级验签)
```

**关键约束**：

| 阶段 | 签名算法 | 密钥位置 | 说明 |
|------|----------|----------|------|
| BootROM | RSA-4096 PKCS#1 v1.5 | OTP fuses (sha256 哈希) | Rockchip BootROM 固件算法固化，不支持 SM2 |
| U-Boot SPL | RSA-4096 | SPL 内置公钥 | 支持双密钥嵌入（新旧密钥过渡期） |
| U-Boot/FIT | RSA-4096 或 SM2 | U-Boot 内置公钥 | 若 gmsm 支持，可切换至 SM2 验签 |
| RootFS | SHA-256/SM3 哈希树 | 内核 cmdline (由 FIT 签名保护) | dm-verity 实时块级校验 |

**OTP 存储布局**（RK3588 eFuse, 64 words = 2048 bits）：

| 偏移(word) | 字段 | 大小(bits) | 写入时机 |
|:---:|------|:---:|------|
| 0-7 | root_key_hash | 256 | 出厂前 |
| 8 | secure_boot_enable | 1 | 出厂前 |
| 9-15 | anti_rollback_counter | 224 | 每次固件升级 |
| 16-23 | reserved | 256 | — |
| 32-39 | device_identity | 256 | 出厂前 |

**防回滚计数器**：使用位图编码（224 bits = 7 words），每位代表一次递增，最大值 224（约 9 年，每月 2 次 OTA）。≥80% WARNING 告警，100% CRITICAL 告警（需返厂）。

---

**文档状态：** v1.1 -- 追加 SM2/SM4 国密实现笔记（第 13 章），补充技术债条目（附录 E #9-#13）
