[CODE_REVIEWED: REJECTED]

# MUPC Phase 1 代码评审报告

**评审日期**: 2026/05/27
**评审结果**: REJECTED
**严重问题数**: 2
**警告问题数**: 4
**优化建议数**: 3

---

## 问题详情

### 严重问题

#### 1. gateway/protocol.rs - IEC 104 TypeID 支持不完整

**位置**: `crates/gateway/src/iec104/protocol.rs`

**问题描述**: TypeId 枚举仅实现了 7 个类型，但技术设计文档要求支持更完整的类型列表。

**当前实现**:
```rust
pub enum TypeId {
    MSpTa1 = 30,      // 单点遥信带时标
    MDpTa1 = 31,      // 双点遥信带时标
    MMeTa1 = 34,      // 测量值带时标
    MMeTd1 = 35,      // 测量值带时标
    CScTa1 = 58,      // 单点遥控带时标
    CDcTa1 = 59,      // 双点遥控带时标
    CSeTa1 = 61,      // 调节命令带时标
}
```

**设计要求** (根据技术设计文档 3.3 节):
| TypeID | 值 | 名称 |
|--------|----|------|
| M_SP_NA_1 | 1 | 单点信息 |
| M_DP_NA_1 | 3 | 双点信息 |
| M_ME_NA_1 | 9 | 测量值，归一化值 |
| M_ME_NC_1 | 13 | 测量值，短浮点数 |
| M_SP_TB_1 | 30 | 单点信息带时标 |
| M_DP_TB_1 | 31 | 双点信息带时标 |
| M_ME_TD_1 | 34 | 测量值带时标 |
| C_SC_NA_1 | 45 | 单点命令 |
| C_DC_NA_1 | 46 | 双点命令 |
| C_SE_NA_1 | 48 | 调节命令 |

**修复建议**: 添加缺失的 TypeID，特别是基础遥测类型 (M_SP_NA_1=1, M_DP_NA_1=3, M_ME_NA_1=9, M_ME_NC_1=13) 和基础命令类型 (C_SC_NA_1=45, C_DC_NA_1=46, C_SE_NA_1=48)。

---

#### 2. web-api/src/routes/status.rs - 未定义的构建时环境变量

**位置**: `crates/web-api/src/routes/status.rs:53`

**问题描述**: 代码引用了 `env!("BUILT_TIME_RAW")` 环境变量，但该变量未在构建时定义，会导致编译失败。

**当前代码**:
```rust
build_time: env!("BUILT_TIME_RAW").to_string(),
```

**修复建议**: 使用 `chrono` 库在运行时获取构建时间，或在 `Cargo.toml` 中通过 build script 定义该变量。建议改为:
```rust
build_time: chrono::Utc::now().to_rfc3339(),
```

---

### 警告问题

#### 3. intercore/protocol.rs - 帧格式未达到定长 64 字节要求

**位置**: `crates/intercore/src/protocol.rs`

**问题描述**: 技术设计文档指定帧格式为"定长 64 字节"，但当前实现未添加填充字节。

**技术设计要求**:
```
┌──────────────────────────────────────────────────────────┐
│  0-1   │  2-3   │  4-7   │  8-15  │  16-23  │  24-63   │
├────────┼────────┼────────┼────────┼─────────┼──────────┤
│ Type   │ Length │ SeqNo  │ ai_    │ strategy│ Reserved │
│ (u16)  │ (u16)  │ (u32)  │ ready  │ _mode   │          │
│        │        │        │ (u8)   │ (u8)    │          │
└──────────────────────────────────────────────────────────┘
总长度：64 字节
```

**当前实现**: 帧头为 8 字节，加上数据后总长度 = 8 + N + 2 (CRC)，没有固定的 64 字节填充。

**修复建议**: 在 `IntercoreFrame::to_bytes()` 中添加填充逻辑，确保总帧长度为 64 字节。

---

#### 4. web-api/src/auth.rs - Session 过期检查可能存在时序问题

**位置**: `crates/web-api/src/auth.rs:48-50`

**问题描述**: `is_expired()` 方法使用 `Utc::now()` 比较时间，但在高并发场景下可能出现微小的时序问题。

**当前代码**:
```rust
pub fn is_expired(&self) -> bool {
    Utc::now() > self.expires_at
}
```

**修复建议**: 考虑使用 `Duration` 比较或添加一个小的时间窗口容差。

---

#### 5. web-api/src/routes/status.rs - 硬编码 IP 地址

**位置**: `crates/web-api/src/routes/config.rs:76`

**问题描述**: 配置中存在硬编码的默认 IP 地址。

**当前代码**:
```rust
remote_addr: "192.168.1.100".to_string(),
```

**修复建议**: 将默认值改为更通用的配置或从环境变量读取。

---

#### 6. gateway/src/iec104/server.rs - 连接处理任务未正确清理

**位置**: `crates/gateway/src/iec104/server.rs:169-170`

**问题描述**: 连接处理完成后，虽然更新了连接状态为 `Disconnected`，但未从 `connections` 列表中移除。

**当前代码**:
```rust
// 清理连接
Ok(())
```

**修复建议**: 在连接关闭时显式调用清理逻辑，从 `connections` 列表中移除已关闭的连接。

---

### 优化建议

#### 7. 测试覆盖率不足

**位置**: `tests/integration/*.rs`

**问题描述**: 集成测试仅为空的 TODO 实现。

**当前代码**:
```rust
#[test]
fn test_iec104_frame_parse() {
    // TODO: 实现 IEC 104 集成测试
    assert!(true);
}
```

**优化建议**: 实现实际的集成测试用例，覆盖正常流程和异常流程。

---

#### 8. 代码重复 - MupcError 创建模式

**位置**: 多个模块

**问题描述**: 各模块重复使用 `MupcError::new()` 创建错误，缺少统一的错误创建宏。

**优化建议**: 扩展 `define_error!` 宏的使用，为每个模块生成专用错误创建函数。

---

#### 9. 日志配置路径不兼容 Windows

**位置**: `crates/common/src/logging.rs:34`

**问题描述**: 默认日志路径 `/var/log/mupc` 使用 Unix 风格路径，在 Windows 平台不兼容。

**当前代码**:
```rust
directory: "/var/log/mupc".to_string(),
```

**优化建议**: 根据 `std::env::consts::OS` 选择不同的默认路径，或使用当前工作目录。

---

## 审查通过项

### 统一错误类型 (common/error.rs) - PASS
- MupcError 包含错误码 (ErrorCode)、错误描述、错误来源模块
- 正确实现 `std::error::Error` trait
- `source()` 方法正确返回错误链

### 核间通信帧格式 (intercore/protocol.rs) - PASS
- 帧格式：0xAA 0x55 (magic) + 长度 + 类型 + 序号 + 数据 + CRC16
- CRC16 校验实现正确
- 帧类型定义完整 (Connect, HeartbeatReq, HeartbeatRsp, ControlCmd, ControlRsp, StatusReport, DataUpload)

### 心跳机制 (intercore/heartbeat.rs) - PASS
- 心跳周期 1000ms (1秒) - 符合设计
- 看门狗超时 10000ms (10秒) - 符合设计

### Session 认证 (web-api/auth.rs) - PASS
- Session 管理器实现完整
- 登录/登出/验证功能正确
- 支持 remember me 功能

### 连接管理 - PASS
- 最大连接数限制正确实现
- 连接状态机正确转换

---

## 结论

代码整体结构清晰，核心模块设计合理，但存在以下阻塞问题：

1. **TypeID 支持不完整** - IEC 104 协议实现不完整，影响与调度主站的兼容性
2. **环境变量未定义** - 会导致编译失败

建议修复上述问题后重新提交评审。

---

**评审人**: Claude Code Reviewer
**审查时间**: 2026/05/27