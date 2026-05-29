# IEC 61850 libIEC61850 集成方案评估

## 1. 现状

当前使用自实现 mock ASN.1 BER 编码（`asn1_utils.rs`），功能有限：

- `MmsClient` 通过短连接 TCP 与 IED 通信
- `MmsRequest`/`MmsResponse` 使用简化的 ASN.1 编码
- 仅实现了 Read/Write 服务的 mock 编解码
- DefineVariableAccess 和 GetDataAccessAttributes 标记为"未实现"
- GOOSE 订阅实现了基础 PDU 解析
- 缺少完整的 MMS 协议栈（如 Initiate/Conclude/GetNameList 等服务）

**当前架构缺陷：**

- ASN.1 BER 编解码不完整，无法与真实 IED 设备互通
- 缺少 MMS 连接管理（Associate/Abort/Release）
- 无 MMS 数据模型映射（IEC 61850-7-3/7-4）
- GOOSE 报文解析仅实现了基础结构，缺少完整的数据集解码

## 2. 候选方案

### 方案 A: 集成 libIEC61850 C 库
- **库**: libIEC61850 (https://github.com/mz-automation/libiec61850)
- **许可**: GPL-3.0 / 商业许可
- **语言**: C
- **集成方式**: 通过 `cc` crate 编译，`bindgen` 生成 FFI 绑定
- **优点**: 成熟稳定、完整 MMS 协议栈、业界广泛使用
- **缺点**: GPL 许可需评估合规性、C 库需交叉编译、FFI 维护成本

### 方案 B: Rust 原生实现
- **库**: 自实现 ASN.1 BER + MMS 编解码
- **许可**: Apache-2.0 / MIT
- **优点**: 纯 Rust、无 FFI、许可友好
- **缺点**: 开发工作量大、协议兼容性需充分测试

### 方案 C: 混合方案
- 核心 MMS 用 libIEC61850 C 库
- 上层逻辑 Rust 封装
- 通过 `mupc-iec61850-sys` 子 crate 隔离 FFI

## 3. 推荐

**推荐方案 A**，条件：
1. 获取商业许可授权（避免 GPL 合规风险）
2. 通过 iec61850-sys 子 crate 封装 FFI
3. 仅集成 MMS 客户端子集（Read/Write/Report）
4. GOOSE 订阅可继续使用 Rust 实现

## 4. 实施路线
1. 评估商业许可费用
2. 创建 iec61850-sys crate（bindgen 生成 FFI）
3. 重构 mms_client.rs 使用真实协议栈
4. 兼容性测试（与主流 IED 设备联调）

## 5. 风险评估
- 许可合规: 高风险（需法务审查）
- 技术可行性: 低风险（libIEC61850 成熟稳定）
- 维护成本: 中风险（C 库版本升级需同步 FFI）
