# MUPC Phase 2A - 南向通信扩展 实施计划

**版本**: v1.0
**日期**: 2026-05-27
**状态**: DRAFT
**团队**: 团队A（2人）

---

## 1. 计划概述

### 1.1 目标

实现南向设备通信能力，建立统一的设备抽象层和插件化架构，支持 RS485 通信和动态插件加载。

### 1.2 架构

```
┌─────────────────────────────────────────────────────────────┐
│                      核心层 (Core)                          │
│  ┌────────────────┐  ┌────────────────┐  ┌─────────────┐ │
│  │  device-trait │  │  plugin-loader  │  │ MessageBus  │ │
│  └────────────────┘  └────────────────┘  └─────────────┘ │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                      插件层 (Plugins)                       │
│  ┌─────────────────┐                                       │
│  │  rs485-plugin   │  ← Phase 2A                          │
│  └─────────────────┘                                       │
│  ┌─────────────────┐                                       │
│  │  hplc-plugin    │  ← Phase 3 预留                      │
│  └─────────────────┘                                       │
└─────────────────────────────────────────────────────────────┘
```

### 1.3 技术栈

| 组件 | 技术选型 |
|------|----------|
| 编程语言 | Rust |
| 异步运行时 | Tokio |
| 插件系统 | `libloading` + trait object |
| 串口通信 | `serial` crate |
| 序列化 | `serde` + `serde_json` |
| 错误处理 | `thiserror` |

---

## 2. 文件结构

### 2.1 创建的 crate

| 路径 | 说明 |
|------|------|
| `crates/device-trait/` | 设备抽象定义（Device、DeviceRegistry、MessageBus trait） |
| `crates/plugin-loader/` | 动态插件加载器 |
| `crates/rs485-plugin/` | RS485 驱动插件 |

### 2.2 目录结构

```
mupc/
├── Cargo.toml                              # 修改：添加新 crate 依赖
└── crates/
    ├── device-trait/                       # 【NEW】
    │   ├── Cargo.toml
    │   ├── src/
    │   │   ├── lib.rs                      # 导出所有 trait
    │   │   ├── device.rs                   # Device trait
    │   │   ├── registry.rs                 # DeviceRegistry trait
    │   │   ├── message_bus.rs              # MessageBus trait
    │   │   ├── errors.rs                   # 统一错误类型
    │   │   └── types.rs                    # 公共类型（DataFrame、Topic、Message）
    │   └── tests/
    │       └── device_tests.rs
    ├── plugin-loader/                      # 【NEW】
    │   ├── Cargo.toml
    │   ├── src/
    │   │   ├── lib.rs
    │   │   ├── loader.rs                   # PluginLoader 实现
    │   │   ├── registry.rs                 # 插件注册表
    │   │   └── errors.rs
    │   └── tests/
    │       └── loader_tests.rs
    └── rs485-plugin/                       # 【NEW】
        ├── Cargo.toml
        ├── src/
        │   ├── lib.rs
        │   ├── device.rs                   # Rs485Device 实现
        │   ├── protocol.rs                  # 协议解析
        │   ├── config.rs                    # 配置解析
        │   └── errors.rs
        └── tests/
            └── rs485_tests.rs
```

---

## 3. 任务分解

### Task 1: device-trait（设备抽象层）

**目标**: 定义南向设备的核心 trait 接口

**文件列表**:
- `crates/device-trait/Cargo.toml`
- `crates/device-trait/src/lib.rs`
- `crates/device-trait/src/device.rs`
- `crates/device-trait/src/registry.rs`
- `crates/device-trait/src/message_bus.rs`
- `crates/device-trait/src/errors.rs`
- `crates/device-trait/src/types.rs`
- `crates/device-trait/tests/device_tests.rs`

**步骤**:

| 步骤 | 操作 | 说明 |
|------|------|------|
| 1 | 编写测试 | 定义 `Device`、`DeviceRegistry`、`MessageBus` trait 的单元测试 |
| 2 | 实现 trait | 按优先级实现：types.rs → errors.rs → device.rs → registry.rs → message_bus.rs → lib.rs |
| 3 | 验证 | `cargo build --package device-trait` + `cargo test --package device-trait` |
| 4 | 提交 | 创建 commit `feat(device-trait): 实现设备抽象层 trait 定义` |

---

### Task 2: rs485-plugin（RS485 驱动）

**目标**: 实现 RS485 串口通信驱动，支持 TTU、光伏逆变器、充电桩等设备

**文件列表**:
- `crates/rs485-plugin/Cargo.toml`
- `crates/rs485-plugin/src/lib.rs`
- `crates/rs485-plugin/src/device.rs`
- `crates/rs485-plugin/src/protocol.rs`
- `crates/rs485-plugin/src/config.rs`
- `crates/rs485-plugin/src/errors.rs`
- `crates/rs485-plugin/tests/rs485_tests.rs`

**步骤**:

| 步骤 | 操作 | 说明 |
|------|------|------|
| 1 | 编写测试 | 定义 `Rs485Device` trait 实现测试，模拟串口读写 |
| 2 | 实现配置解析 | 实现 `Rs485Config`、`Parity` 配置结构 |
| 3 | 实现设备驱动 | 实现 `Rs485Device` trait，支持 `send_frame`/`recv_frame` |
| 4 | 实现协议解析 | 实现数据帧解析、校验（CRC） |
| 5 | 验证 | `cargo build --package rs485-plugin` + `cargo test --package rs485-plugin` |
| 6 | 提交 | 创建 commit `feat(rs485-plugin): 实现 RS485 驱动插件` |

---

### Task 3: plugin-loader（动态插件加载）

**目标**: 实现插件的动态加载、卸载、生命周期管理

**文件列表**:
- `crates/plugin-loader/Cargo.toml`
- `crates/plugin-loader/src/lib.rs`
- `crates/plugin-loader/src/loader.rs`
- `crates/plugin-loader/src/registry.rs`
- `crates/plugin-loader/src/errors.rs`
- `crates/plugin-loader/tests/loader_tests.rs`

**步骤**:

| 步骤 | 操作 | 说明 |
|------|------|------|
| 1 | 编写测试 | 定义 `PluginLoader` trait 的单元测试 |
| 2 | 实现加载器 | 实现 `PluginLoader` trait，支持 `load`/`unload`/`list`/`get` |
| 3 | 实现生命周期 | 实现插件 `init`/`start`/`stop`/`shutdown` 生命周期管理 |
| 4 | 验证 | `cargo build --package plugin-loader` + `cargo test --package plugin-loader` |
| 5 | 提交 | 创建 commit `feat(plugin-loader): 实现动态插件加载器` |

---

### Task 4: 集成与测试

**目标**: 集成 device-trait、plugin-loader、rs485-plugin，验证整体功能

**文件列表**:
- 修改 `mupc/Cargo.toml` - 添加新 crate 依赖
- 修改 `mupc/crates/device-trait/src/lib.rs` - 导出 Plugin trait
- 新增 `crates/plugin-loader/tests/integration_tests.rs`

**步骤**:

| 步骤 | 操作 | 说明 |
|------|------|------|
| 1 | 更新 Cargo.toml | 在 workspace 中注册 device-trait、plugin-loader、rs485-plugin |
| 2 | 集成测试 | 编写集成测试验证插件加载和设备通信流程 |
| 3 | 编译验证 | `cargo build --workspace` 无错误 |
| 4 | Clippy 检查 | `cargo clippy --workspace` 无 Error |
| 5 | 单元测试 | `cargo test --workspace` 通过率 ≥ 80% |
| 6 | 提交 | 创建 commit `feat(integration): 集成南向通信模块` |

---

## 4. 里程碑

| 里程碑 | 内容 | 交付物 |
|--------|------|--------|
| M2.1 | 核心 trait 定义 | device-trait crate |
| M2.2 | RS485 插件 | rs485-plugin crate |
| M2.3 | 插件加载器 | plugin-loader crate |
| M2.4 | 集成测试 | 完整南向通信模块 |

---

## 5. 依赖关系

```
device-trait (Task 1)
    ↓
plugin-loader (Task 3) → device-trait
rs485-plugin (Task 2) → device-trait
    ↓
集成测试 (Task 4) → device-trait, plugin-loader, rs485-plugin
```

---

## 6. 风险与对策

| 风险 | 等级 | 对策 |
|------|------|------|
| RS485 电气特性导致通信不稳定 | 低 | 增加重试机制和超时控制 |
| 插件隔离不足 | 中 | 使用 Rust 的 Safe Trait 约束 |
| libloading 在 Windows 平台兼容性 | 低 | 测试阶段覆盖 Windows 环境 |