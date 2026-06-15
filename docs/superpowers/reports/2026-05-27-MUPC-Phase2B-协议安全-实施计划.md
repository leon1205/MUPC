# MUPC Phase 2B - 协议与安全扩展 实施计划

**版本**: v1.0
**日期**: 2026-05-27
**状态**: DRAFT
**团队**: 团队B（1人）

---

## 1. 计划概述

### 1.1 目标

实现 IEC 61850-7-420 协议支持、MQTT 北向扩展（TLS、QoS 增强）、国密 SM2/SM4 安全机制。

### 1.2 架构

```
┌─────────────────────────────────────────────────────────────┐
│                      北向通信                                │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │    IEC 104   │  │ IEC 61850    │  │    MQTT      │     │
│  │   (Phase 1)  │  │   (Phase 2)  │  │   (Phase 2)  │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                    安全层 (Security)                         │
│  ┌────────────────────────────────────────────────────────┐│
│  │                    security crate                       ││
│  │  ├── SM2 签名/验签                                       ││
│  │  ├── SM4 加密/解密                                      ││
│  │  ├── TLS 连接器                                         ││
│  │  └── 证书管理                                           ││
│  └────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                    核心层 (Core)                             │
│  ┌────────────────────────────────────────────────────────┐│
│  │              device-trait crate (共享)                 ││
│  │  ├── Device trait                                      ││
│  │  ├── DeviceRegistry trait                              ││
│  │  └── MessageBus trait                                  ││
│  └────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

### 1.3 技术栈

| 组件 | 技术选型 |
|------|----------|
| 编程语言 | Rust |
| 异步运行时 | Tokio |
| IEC 61850 | libIEC61850（Rust 绑定或 FFI） |
| MQTT | `rumqttc` 0.20+ |
| 密码学 | GmSSL / `ring` |
| TLS | `rustls` |
| 序列化 | `serde` + `serde_json` |
| 错误处理 | `thiserror` |

---

## 2. 文件结构

### 2.1 创建的 crate

| 路径 | 说明 |
|------|------|
| `crates/security/` | 国密 SM2/SM4、TLS 证书管理 |
| `crates/iec61850-plugin/` | IEC 61850-7-420 协议插件 |
| `crates/mqtt-plugin/` | MQTT 北向扩展插件 |

### 2.2 目录结构

```
mupc/
├── Cargo.toml                              # 修改：添加新 crate 依赖
└── crates/
    ├── security/                           # 【NEW】
    │   ├── Cargo.toml
    │   ├── src/
    │   │   ├── lib.rs                      # 导出所有模块
    │   │   ├── sm2.rs                      # SM2 签名/验签
    │   │   ├── sm4.rs                      # SM4 加密/解密
    │   │   ├── cert.rs                     # 证书管理
    │   │   ├── tls.rs                      # TLS 连接器
    │   │   └── errors.rs
    │   └── tests/
    │       ├── sm2_tests.rs
    │       ├── sm4_tests.rs
    │       └── cert_tests.rs
    ├── iec61850-plugin/                   # 【NEW】
    │   ├── Cargo.toml
    │   ├── src/
    │   │   ├── lib.rs
    │   │   ├── device.rs                   # Iec61850Device 实现
    │   │   ├── mms_client.rs               # MMS 客户端封装
    │   │   ├── goose.rs                    # GOOSE 订阅处理
    │   │   ├── config.rs                   # 配置解析
    │   │   └── errors.rs
    │   └── tests/
    │       └── iec61850_tests.rs
    └── mqtt-plugin/                        # 【EXT】（Phase 1 已有时扩展）
        ├── Cargo.toml                     # 修改：添加 TLS 支持
        ├── src/
        │   ├── lib.rs                     # 修改：导出新增类型
        │   ├── client.rs                   # 修改：QoS 增强、TLS 支持
        │   ├── config.rs                   # 修改：添加 TLS 配置字段
        │   └── errors.rs
        └── tests/
            └── mqtt_tests.rs              # 修改：添加 TLS 测试
```

---

## 3. 任务分解

### Task 1: iec61850-plugin（IEC 61850-7-420 插件）

**目标**: 实现 IEC 61850-7-420 协议客户端，支持 MMS 读写、GOOSE 订阅

**文件列表**:
- `crates/iec61850-plugin/Cargo.toml`
- `crates/iec61850-plugin/src/lib.rs`
- `crates/iec61850-plugin/src/device.rs`
- `crates/iec61850-plugin/src/mms_client.rs`
- `crates/iec61850-plugin/src/goose.rs`
- `crates/iec61850-plugin/src/config.rs`
- `crates/iec61850-plugin/src/errors.rs`
- `crates/iec61850-plugin/tests/iec61850_tests.rs`

**步骤**:

| 步骤 | 操作 | 说明 |
|------|------|------|
| 1 | 编写测试 | 定义 `Iec61850Device` trait 测试，模拟 MMS 读写、GOOSE 订阅 |
| 2 | 实现配置解析 | 实现 `Iec61850Config` 配置结构 |
| 3 | 实现 MMS 客户端 | 实现 MMS 协议客户端封装（连接、读写请求） |
| 4 | 实现 GOOSE 处理 | 实现 GOOSE 消息订阅和处理 |
| 5 | 实现设备接口 | 实现 `Iec61850Device` trait |
| 6 | 验证 | `cargo build --package iec61850-plugin` + `cargo test --package iec61850-plugin` |
| 7 | 提交 | 创建 commit `feat(iec61850-plugin): 实现 IEC 61850-7-420 插件` |

---

### Task 2: mqtt-plugin（MQTT 北向扩展）

**目标**: 扩展 Phase 1 MQTT 插件，支持 TLS、QoS 1/2 级别

**文件列表**:
- `crates/mqtt-plugin/Cargo.toml`（修改）
- `crates/mqtt-plugin/src/lib.rs`（修改）
- `crates/mqtt-plugin/src/client.rs`（修改）
- `crates/mqtt-plugin/src/config.rs`（修改）
- `crates/mqtt-plugin/tests/mqtt_tests.rs`（修改）

**步骤**:

| 步骤 | 操作 | 说明 |
|------|------|------|
| 1 | 编写测试 | 扩展 MQTT 测试用例，覆盖 TLS 连接、QoS 1/2 |
| 2 | 更新配置 | 添加 `use_tls`、`ca_cert`、`client_cert`、`client_key` 字段 |
| 3 | 实现 TLS 支持 | 在 `MqttClient` 中集成 TLS 连接 |
| 4 | 实现 QoS 增强 | 支持 QoS 0/1/2 级别消息发布 |
| 5 | 验证 | `cargo build --package mqtt-plugin` + `cargo test --package mqtt-plugin` |
| 6 | 提交 | 创建 commit `feat(mqtt-plugin): 扩展 TLS 和 QoS 支持` |

---

### Task 3: security（国密 SM2/SM4）

**目标**: 实现国密算法支持，包括 SM2 签名/验签、SM4 加密/解密、TLS 证书管理

**文件列表**:
- `crates/security/Cargo.toml`
- `crates/security/src/lib.rs`
- `crates/security/src/sm2.rs`
- `crates/security/src/sm4.rs`
- `crates/security/src/cert.rs`
- `crates/security/src/tls.rs`
- `crates/security/src/errors.rs`
- `crates/security/tests/sm2_tests.rs`
- `crates/security/tests/sm4_tests.rs`
- `crates/security/tests/cert_tests.rs`

**步骤**:

| 步骤 | 操作 | 说明 |
|------|------|------|
| 1 | 编写测试 | 定义 SM2、SM4、证书管理的单元测试 |
| 2 | 实现 SM2 | 实现 `sm2_sign`、`sm2_verify` 函数 |
| 3 | 实现 SM4 | 实现 `sm4_encrypt`、`sm4_decrypt` 函数 |
| 4 | 实现证书管理 | 实现证书加载、验证、轮换功能 |
| 5 | 实现 TLS 连接器 | 实现 `TlsConnector` 构建器 |
| 6 | 验证 | `cargo build --package security` + `cargo test --package security` |
| 7 | 提交 | 创建 commit `feat(security): 实现国密 SM2/SM4 和 TLS 支持` |

---

### Task 4: 集成与测试

**目标**: 集成 security、iec61850-plugin、mqtt-plugin，验证安全通信功能

**文件列表**:
- 修改 `mupc/Cargo.toml` - 添加新 crate 依赖
- 新增 `crates/security/tests/integration_tests.rs`
- 新增 `crates/iec61850-plugin/tests/tls_tests.rs`

**步骤**:

| 步骤 | 操作 | 说明 |
|------|------|------|
| 1 | 更新 Cargo.toml | 在 workspace 中注册 security、iec61850-plugin、mqtt-plugin |
| 2 | 集成测试 | 编写集成测试验证 TLS 连接、SM2 签名、SM4 加密流程 |
| 3 | 编译验证 | `cargo build --workspace` 无错误 |
| 4 | Clippy 检查 | `cargo clippy --workspace` 无 Error |
| 5 | 单元测试 | `cargo test --workspace` 通过率 ≥ 80% |
| 6 | 提交 | 创建 commit `feat(integration): 集成协议与安全模块` |

---

## 4. 里程碑

| 里程碑 | 内容 | 交付物 |
|--------|------|--------|
| M2.3 | IEC 61850 插件 | iec61850-plugin crate |
| M2.4 | MQTT 南向插件 | mqtt-plugin crate（扩展） |
| M2.5 | 安全组件 | security crate |

---

## 5. 依赖关系

```
device-trait (Phase 2A 提供，共享)
    ↓
iec61850-plugin → device-trait, security
mqtt-plugin → device-trait, security
    ↓
security (无依赖)
    ↓
集成测试 → security, iec61850-plugin, mqtt-plugin
```

---

## 6. 风险与对策

| 风险 | 等级 | 对策 |
|------|------|------|
| IEC 61850 协议栈复杂度高 | 高 | 使用成熟开源库（如 libIEC61850）或 Rust 实现子集 |
| 国密库 Rust 支持不完善 | 中 | 预备纯软件实现方案（参考 GmSSL Rust 绑定） |
| TLS 性能开销 | 中 | 优化连接复用，减少握手次数 |