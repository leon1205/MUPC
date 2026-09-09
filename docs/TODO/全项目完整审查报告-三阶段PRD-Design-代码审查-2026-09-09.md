# 全项目完整审查报告：三阶段 PRD → Design → 代码审查

> 执行日期：2026-09-09
> 审查框架：`.claude/agents/PROMPTS/dispatch_project_review-prompts.md`
> 范围：全模块（01 通信网关 / 02 南向通信 / 03 数据处理 / 04 策略引擎 / 05 AI 引擎 / 06 安全 / 07 OTA / 08 Web / 09 本地运维 / 10 核间通信 + core-bin 装配/部署形态），策略+AI / 核间+南向 / 装配为阶段三代码深审重点
> 整体风险等级：**🔴 高（3 项 P0 阻塞 + 6 项 P1 严重）**

---

## 〇、执行摘要

本项目完成一轮端到端三阶段审查：**阶段一**逐条筛查 10 模块 PRD 功能点/运行场景/部署架构；**阶段二**将功能点与 Design 文档逐条细节级比对；**阶段三**由三组并行代码深审（strategy-engine+ai-engine / intercore+mupc-southd+rs485 / mupc-core-bin 装配+部署形态）对 Design 与代码逐项核对（完整性/健壮性/Rust Safety/部署形态），并对安全国密依赖作主控抽查。

**核心结论**：BECG 平台主线（modbus_rtu 生产路径）的**接线框架真实且闭环**，但**"喂给框架的数据真实性"存在系统性占位**——南向真机串口读会因 `O_NONBLOCK` 击穿超时语义而恒失败；AI 观测/奖励输入在生产为恒值占位；国密套件（SM2 签名/SM4-GCM/HKDF）依赖 ring 国际算法兜底。三者独立构成生产可信投运的 P0 阻塞。部署形态文档-代码一致（southd/mupc-io 内嵌装配是文档已收敛的结果）。

---

## 一、阶段一结论：PRD 需求功能点与运行场景筛查

10 模块 PRD 逐条筛查完成（功能点 F 系列 + 运行场景 S 系列 + 部署架构 D 系列）。此处给出各模块筛查要点统计与最高价值输出；完整逐条清单见各模块 PRD 文档。

| 模块 | PRD 规模概览 | 筛查要点 |
|---|---|---|
| 01 通信网关 | IEC104 + IEC61850 + MQTT 多协议 | 双网/多链路冗余、时标规范、协议转换 |
| 02 南向通信 | RS485/HPLC 多从站 + 协议处理器 | 8 路隔离 RS485、半双工 DE/RE、Modbus/TTU/逆变器/充电桩 |
| 03 数据处理 | 遥测采集 + 故障录波 + 存储 | 波形 .wave/.cfg、COMTRADE 转换、写入延迟 |
| 04 策略引擎 | 台区储能治理 + AI 集成兜底 | SOC 双源回落、TaiStorage 分相 P/Q、安全校验 |
| 05 AI 引擎 | LSTM 预测 + RL 决策 + RKNN | 动作空间、奖励函数、SafetyOverride、在线微调 |
| 06 安全 | 国密套件 + 证书 + 审计 | SM2/SM3/SM4、TLS、设备不存私钥、审计链 |
| 07 OTA | 固件/模型升级 | A/B 分区、公钥验签、版本回滚 |
| 08 Web 管理 | Axum REST+SSE | AI 可视化、RBAC 鉴权、决策记录 |
| 09 本地运维 | BLE/NearLink/WiFi | 通道加密、UUID 集合 |
| 10 核间通信 | 实时控制模块 TCP/modbus | PCS 协议、指令/回读、心跳看门狗、DI/DO 联锁 |

### 运行场景筛查特别提醒（易被功能点名称吞掉的对立场景）
- 「黑启动」隐含 电网无压→黑启动 与 电网有压→冷启动恢复 两对立场景；
- 「上电初始化/冷启动」逻辑在第一个控制周期前执行，最易被周期轮询掩盖（表现为"运行时检测有了、初始判定没有"）；
- 10 模块「上电恢复/停机语义」与 04 策略「AI 失效兜底」为本轮在代码层重点核对的场景（见阶段三）。

---

## 二、阶段二结论：PRD vs Design 比对

阶段二共核对 28 项需求细节级比对项：**21 项完全覆盖（75%）、7 项部分/偏差**（多已由文档自注授权偏离）。另含宏观组（01/02/03）、重点组（04/05/10）、安全运维组（06-09）三个方向的分组结论。

### 宏观组 01-03（网关/南向/数据处理）
| 编号 | 功能点 | 覆盖状态 | Design 位置 | 说明与建议 |
|---|---|---|---|---|
| M01 | IEC104 时标规范 | ⚠️ 矛盾 | 01 | 内部矛盾：epoch ms 与 CP56Time2a 7 字节互斥，需定一 |
| M02 | 南向站级调度 | ⚠️ 收窄 | 02 §10 | southd 站级调度为 PRD 未吸收的架构收敛：PRD 仍 cdylib 插件形态、无 BMS/关口表/8 路拓扑；Design 已收敛为 core-bin 内嵌装配，PRD 需双向追认 |
| M03 | 波形 .wave Header 布局 | ⚠️ 不一致 | 03 §3.4.1 vs §3.4.3 | offset 字段定义冲突，STO-02 基准无法同时满足 |
| M03 | COMTRADE 转换系数 | ⚠️ 不自洽 | 03 | ±250V 量程无法承载 420V 波峰会削顶（16 位有符号量程矛盾） |
| M03 | 波形写入延迟 | ⚠️ 冲突 | 03 | 写入 ≤10ms 指标 vs WriteBuffer 100ms 批量提交冲突 |
| M03 | 文档覆盖 | ⚠️ ~85% | 03 | 台账/告警/分区表 Design 自述多表主链路，实际仅 6 张表落地 |

### 重点组 04/05/10（策略/AI/核间）
| 编号 | 功能点 | 覆盖状态 | 说明 |
|---|---|---|---|
| V1 | 05 生产数据源 | ❌ **最高** | D9/q_realtime_margin/SafetyOverride 在 production(modbus_rtu) **无真实数据源**——10 已废 SafetyOverride/无 TCP 上送/遥测转 master_meter，05 仍保留 D9+R_safety_override+STATE-08 要求 DataUpload 帧来源（仅 TCP/sim 存在）。阶段三在代码层实锤（见 §四 P0-2） |
| V2 | 指令范围 | ⚠️ 三处不一致 | 10 PRD p_ref ±1000/k_droop 0~100 vs 05 动作空间 ±50/0~30 vs PCS clamp ±25 |
| V3 | interlock DiConf | ⚠️ 小缺口 | 10 DiConf 无 source 字段靠含词启发式归类（火/浸/急停）vs §12.3 action 配置化 schema |
| V4 | 05 PRD 自相矛盾 ×3 | ⚠️ | §5.4 LWW vs §5.7 远程优先；REWARD-11 vs §7.2 已删 reason；ACT-01~08 vs ACT-01~05 复用 |
| V5 | 05 Design validate | ⚠️ | §8.4 用旧 validate 与正文 validate_dual 冲突（文档自标"待统一"） |

### 安全运维组 06-09
| 编号 | 问题 | 等级 | 说明 |
|---|---|---|---|
| 06 | 国密落地断层 | **P0** | gmsm 实际 0.1.0，SM2 签名/SM4-GCM/HKDF/ECDH/x509 无真实现（仅 ring 模拟）——GM-01~04"真国密+生产 real_gmsm"无法兑现（阶段三代码层确认，见 §四 P0-3） |
| 06 | 私钥矛盾 | ⚠️ | "设备不存私钥" vs LEA-54 证书私钥存 /etc/mupc/certs |
| 07 | OTA 公钥路径双轨 | ⚠️ | /etc/mupc/ota_public_key.pem vs /security/ota_public_key.pem |
| 08 | RBAC/角色漂移 | ⚠️ | 角色集 5→4 漂移、SSE 设计新增未追认、RBAC 占位未实现（阶段三确认） |
| 09 | BLE UUID 超集未回写 | ⚠️ | FACE/FAC0 + 全通道 ECDH/AES-256-GCM 会话加密超集未回写 PRD |

**文档级映射度**：06 97% / 07 98% / 08 95% / 09 90%，差异为超集/收窄/延后，需双向追认。

---

## 三、阶段三（3a/3b/3c/3e）结论：Design vs 开发代码

三组并行深审。代码路径前缀 `mupc/crates/`。

### 3a）代码完整性审查

#### 策略 + AI 引擎
| Design 功能 | 代码位置 | 状态 | 说明 |
|---|---|---|---|
| dispatch 三路径全走兜底出口 | strategy-engine/ai_integration.rs:422/462/488→240 | ✅ | 三处均达 apply_soc_source(:266)→TaiStorage evaluate→send_tai_command |
| SOC 双源回落（S3b-1d 去 None-gate） | ai_integration.rs:177-231 + 纯函数 586-623 | ✅ | BMS/核间皆失只保冻结值+30s 节流 warn，不置 None；生产 modbus latest_soc 每拍活读（intercore/transport/modbus.rs:537）真源 |
| AiValidator 安全闸门接线 | core-bin/startup.rs:530（model=None） | ⚠️ 空壳 | model 恒 None → valid() 恒通过，ai_integration.rs:485-490 拒绝分支生产死代码 |
| validate_dual 关键路径 | ai-engine/model_manager.rs:421（Step7） | ✅ | RL 决策后真被调；仅 clamp 值域不拒收 |
| AI 决策→核间下发 | ai_integration.rs:498-524 send_dual_param | ✅（仅 AI 模式） | 生产默认旁路（local_priority=true 走 TaiStorage） |
| robustness 应急 | ai_integration.rs:426-451→532 | ⚠️ 绕过 | 不经 validate_dual/AiValidator 直发；检测源恒值基本不触发 |
| mode_selector 平滑过渡 | ai-engine/mode_selector.rs:488/632 | ❌ | 插值权重无人消费（core-bin 从不调 set_weights），doc"递归缓动"不成立 |
| online update / ab_test 进决策链 | model_manager.rs:516-555（恒 Ok(false)） | ❌ | 无生产触发者；仅数据采集(:497)生效 |

#### 核间 + 南向
| Design 功能 | 代码位置 | 状态 | 说明 |
|---|---|---|---|
| latest_soc 活读 | intercore/transport/modbus.rs:537-543 | ✅ | 每拍持 bus 锁 FC04 活读 REG_SOC 无缓存；越界/失败 None |
| SOC 通道接线 | scheduler.rs:350→startup.rs:339→ai_integration.rs:160 | ✅ | battery 站→on_battery_soc→set_battery_soc，0-100 守卫 |
| Modbus 心跳看门狗 | modbus.rs:378-448（startup.rs:426 spawn） | ✅ | 每拍 1013，连续 3 次失败 offline；1013=0 且已启动→仅告警不自动重启 |
| watchdog / heartbeat | watchdog.rs:98-105 | ❌ 空壳 | trigger_reset 仅日志无复位动作；HeartbeatManager 为 TCP 仿真遗留未装配——文档"心跳看门狗"职责实由 modbus 心跳+interlock 承担，双实现未收敛 |
| clamp/点表一致性 | modbus.rs:484/511；pcs.rs:8-24 | ⚠️ | REG500/1000/1001/1002/1006-1011 匹配 PCS V1.3；k_droop 恒功率忽略已注明语义偏离 |
| 双 transport 切换 | core-bin/startup.rs:408-445 | ✅ | modbus_rtu(prod)/tcp(sim) 二选一；tcp stop 降级 no-op(:137) |
| southd 站级调度装配 | startup.rs:622-687；SouthSink:281-344 | ⚠️ 框架就绪 | lib 被 core-bin 装配，grid 真接线；**bms/空调/储能表/消防全注释**（等 S3b-2 厂方点表），8 路拓扑未投产 |

#### core-bin 装配
| Design 组件 | 装配位置 | 状态 | 说明 |
|---|---|---|---|
| 消息总线 message_bus | startup.rs:368 | ❌ | 创建注册但全仓库无生产者/消费者（core DeviceRegistry/MessageBus 未接线，02 文档:1221 已自认） |
| security | startup.rs:377 | ⚠️ | 纯 log+注册无实例；仅 audit 经 web-api 用，TLS/SM2/SM4/cert 未运行 |
| storage(sqlx) | startup.rs:393-401 | ✅ | events/telemetry/interlock 有真实写方；**decisions 表无生产者** |
| intercore | startup.rs:408-445 | ✅ | modbus_rtu 生产 / tcp 仿真，N3 回读 |
| data_processing | startup.rs:477 | ⚠️ | 仅 FaultRecorderImpl，采集器/高频遥测空置（collector "延迟创建"即从未创建） |
| ai_engine | startup.rs:490-497 | ✅ | ModelManager+load_models，降级运行 |
| strategy/AiIntegrator | startup.rs:502-590 | ✅ | 1s 决策循环+validator+TaiStorage；联锁抑制 |
| IEC104 gateway | startup.rs:594-609 | ⚠️ | server 真启动但端口硬编码 2404；上送源仅 pv/load 模拟固定值 |
| southd | startup.rs:621-692 | ✅ | scheduler 条件装配，grid 单写 AiIntegrator |
| interlock(io) | startup.rs:546-566 | ✅ | io.enabled 装配，DB 读回+run_loop |
| web-api | startup.rs:779-832 | ✅ | 全路由+AppState 真源注入（AI 5 端点除外） |
| OTA | startup.rs:745-750 | ⚠️ | 仅创建注入 AppState，无后台 scheduler 常驻 |
| mqtt_bridge | startup.rs:887-911 | ⚠️ | 真 spawn 但 Default 配置（北向 mqtt.example.com/dummy 证书） |
| wireless | startup.rs:914-917 | ⚠️ | 纯注册 stub（Phase 2+ NoOp 预留，勿判死代码） |

### 3b）代码健壮性审查

| 功能模块 | 异常场景 | 处理状态 | 风险 | 说明与建议 |
|---|---|---|---|---|
| AI 数据源 | 电压/功率/频率/q_margin/SafetyOverride 生产无真源 | ❌ 恒值 | **高** | startup.rs:183-208 硬编码 q_margin=0.5/safety_override=false/peak=0，frame 值 FIXED(380V/50kW)；southd meter_grid 只写 AiIntegrator 不写 AI rt_source → reward/RL/robustness 输入生产恒值（voltage=1.0/soc=0.75）。D3/D5/D6 adapter 全 Err→dispatch_p_set 恒 None→ACT-DUAL-04 永不生效；reward 结果被丢弃（model_manager.rs:483） |
| AiValidator | 校验不通过降级 | ❌ | **高** | model=None→valid() 恒过，拒绝分支死代码；PRD/04 §6"校验不通过降级"形同虚设 |
| rs485-plugin | O_NONBLOCK 真机读 | ❌ | **高** | device.rs:150 open O_NONBLOCK + recv_frame:341-350 依赖 VMIN/VTIME 阻塞超时——非阻塞下 read 无数据立即 EAGAIN，VTIME 失效，真从站请求-响应恒超时（见 P0-1） |
| rs485-plugin | 半双工 DE/RE | ⚠️ | P2 | send_recv(:285-315) 读路径不经 set_dir；set_dir(:372-382) 双引脚单布尔（Send 置 DE 不清 RE）；Config 默认 de/re=None→依赖硬件 auto-direction |
| intercore modbus | 线路抖动 | ⚠️ | P2 | 单次失败即 mark_offline 清缓存→下条强制重写；bus 锁内 200ms 超时阻塞同链路上行；open_ctx 每事务重开串口——可接受但抖动放大 |
| config | 同口多从站 | ⚠️ | P2 | core_config.rs:556-599 校验 port 别名/波特率/PCS 互斥，**未见同口 slave 唯一性校验** |
| scheduler | 口 task panic | ⚠️ | P2 | 多处 std RwLock .unwrap() 中毒即 panic 静默终止口采集；startup.rs:671 重建 supervisor 留 TODO |
| TaiStorage 节流 | evaluate 60s 节流期 | ⚠️ | P2 | 节流内返回 last_cmd 克隆仍被 send_tai_command 每秒重写 6 个 4 区寄存器（idempotent 不越权但空耗 RS485 带宽） |
| SOC 双源皆失 | 双源超期 | ✅ | 低 | 30s 节流 warn 沿用冻结值，安全面可接受 |
| ActionValidator | 自死锁 | ✅ | — | 已修（read guard 限 if-let 块内，write 在块外）；reward_calculator 锁按语句释放+AtomicU32，无跨 await/函数尾残留 |

### 3c）Rust Safety 与内存安全审查

| 维度 | 问题位置 | 风险类型 | 等级 | 说明与建议 |
|---|---|---|---|---|
| 指令限幅 | action_space.rs:114-116 vs model_manager.rs:335 vs pcs.rs:24/36-39 vs modbus.rs:498-515 | 语义漂移 | **P1** | AI/应急 p_ref±50 走恒功率 1001 无 clamp 直写 to_pcs_reg；k_droop PCS 静默丢弃(1002 恒写0)；未经 transformer 容量配置。send_dual_param 前应加范围校验+clamp 收敛 |
| IEC104 外部指令 | core-bin/startup.rs:94-109 | 无界写入 | **P1** | 外部 master p_set 直送 DualParamCommand→1001 原值无 clamp/validator（越 i16 靠 as 饱和截断） |
| Panic/毒锁 | intercore/transport/modbus.rs:203/404/582、ai-engine/action_validator.rs、reward_calculator.rs 多处 | 锁中毒 panic | P2 | 短暂持锁+unwrap，锁内 panic 则毒化周期硬崩；建议 unwrap_or_else(into_inner)（ai_integration.rs 已示范） |
| FFI unsafe | rs485-plugin/device.rs:147-152/185-190/243-350 | 缺 SAFETY 注释 | P2 | fd<0/assume_init 前 tcgetattr/写循环长度守卫均正确；10+ 处 unsafe 无 SAFETY 注释，termios 恢复非 RAII(:353 仅正常路径 restore) |
| fd Drop | device.rs:717-721 | ✅ | — | configure_port 失败路径手动 close 无泄漏 |
| mupc-io | mupc-io/lib.rs:26-69 | fd 管理 | P2 | export 后 50ms 硬睡非轮询；无 unexport/Drop，gpio 号残留 |
| 并发同步 | modbus.rs:404/203 | 毒锁 | P2 | 范式不一致（ai_integration 用 unwrap_or_else 容忍中毒，此处 unwrap） |
| NPU/FFI | model_registry.rs:312-315、model_manager.rs:623-661 | 有兜底 | P2 | decide/parse Err→fallback 安全；预测未识别维度有 warn 回退 |

### 3e）部署形态与进程边界一致性

| 部署组件 | Design 声明 | 代码入口 | 一致性 | 说明 |
|---|---|---|---|---|
| mupcd | 主进程=北向+web+storage+AI | core-bin/src/main.rs:28 | ✅ | core-bin 是唯一生产装配者 |
| mupc-southd | 曾称"独立部署" | 纯 lib→core-bin 静态装配 | ✅（文档已改） | 02 Design/S3a 明写 core-bin 唯一装配者、southd 是依赖；Design 已收敛为内嵌，**非**"文档独立代码内嵌"不一致 |
| mupc-io | 曾称"独立 DI/DO" | 纯 lib→interlock.rs:31 用 SysfsIn/Out | ✅（文档已改） | S2 计划明写 leaf crate 由 core-bin startup 组装 |
| sim-bridge | 独立仿真 bin | sim-bridge/src/main.rs | ✅ | 唯一自持 main 的仿真进程 |
| 核间工具 bin | 开发/回放 | intercore/strategy-engine src/bin/{modbus_slave,pcs_slave}/{dump_row,tai_replay} | ✅ | 工具非部署组件 |

> 注：interlock tcp 仿真下 last_run_state 恒 None 会误置 stop_failed，代码注释已声明不可作生产联锁（仿真角色边界清晰）。

---

## 四、最终综合审查结论

### 需求覆盖情况
- PRD 功能点：10 模块全扫（规模概览见 §一）
- Design 覆盖率：75%（28 项中 21 完全）；宏观组文档级 ~85%
- 代码实现覆盖率：接线框架完整（storage/interlock/southd 调度/AiValidator 类有真源），**但生产数据真实性与安全闸门存在系统性占位**（见下）

### 关键问题汇总

**P0 阻塞（3 项）**
1. **南向真机串口读恒失败**：rs485-plugin device.rs:150 `O_NONBLOCK` 击穿 recv_frame:341-350 的 VMIN/VTIME 阻塞超时 → Modbus 真从站请求-响应恒超时。southd meter_grid/BMS 及 pv/load 南向读全受影响。BECG 南向投产前置（当前 8 路未投产故未暴露）。
2. **AI 生产观测/奖励输入全恒值占位**：startup.rs:183-208 + 698-711，voltage=1.0/SOC=0.75/q_margin=0.5/SafetyOverride=false 恒常，D3/D5/D6 adapter Err。AI 模式（local_priority=false）在假数据上决策直发 PCS。05 文档 D9/SafetyOverride"生产无真源"实锤。
3. **国密套件断层**：security/Cargo.toml:11-13 注释自认 SM2 签名/SM4-GCM/HKDF 用 ring（国际算法）兜底，gmsm 0.1.0 无真实现。06 PRD GM-01~04 生产真国密无法兑现（涉国密合规不可交付）。

**P1 严重（6 项）**
1. AiValidator 安全闸门生产空壳（model=None→valid() 恒过）；IEC104 外部 p_set 绕过校验直写。
2. 应急/鲁棒路径绕过所有安全校验，且检测源为恒值。
3. 指令限幅三处不一致 + PCS k_droop 静默丢弃（"下发成功"与物理支撑背离）。
4. IEC104 北向近空转+假数据（端口硬编码 2404、上送源模拟固定值、southd meter_grid 不进 IEC104）。
5. decisions 决策记录无生产者（决策不可审计/回放）。
6. 联锁释放/急停无鉴权（RequireRole 恒 Admin + login role 硬编码 operator）。

**P2 批次（已识 10 项）**：DE/RE 半双工未进读路径+双引脚单布尔；online update/ab_test/mode_selector 插值权重"存在未接线"（try_online_update 恒 Ok(false)、reward 丢弃 model_manager.rs:483）；TaiStorage 节流期空耗带宽；AI 5 端点占位包装成可用；MQTT 假域名被真 spawn+stub 标 Running（运维误判）；std RwLock unwrap 毒锁+unsafe 缺 SAFETY 注释+termios 非 RAII；watchdog trigger_reset 空壳/HeartbeatManager 双实现；core message_bus/data-processing collector 未接线；8 路站级拓扑生产未投产（等 S3b-2 点表，属框架就绪勿判死代码）；scheduler 口 task panic 无观测。

**部署形态**：✅ 一致（southd/mupc-io 内嵌为文档已收敛；sim-bridge 唯一独立 bin；无孤儿 bin）。

### 风险等级评定

**🔴 高风险**。评定依据：3 项 P0 各自独立阻止当前形态的生产可信投运——真机南向一接即失败（P0-1）、AI 模式在假数据上决策（P0-2）、国密合规不可交付（P0-3）；叠加 6 项 P1（其中 AiValidator 空壳直接使"AI 指令安全闸门"失效）。

### 优先修复 Action Items

| 优先级 | 修复项 | 负责模块 | 建议 |
|---|---|---|---|
| P0-1 | 南向真机串口读 | rs485-plugin | 去 O_NONBLOCK（改回阻塞+VTIME）或 poll()+按需组帧；附真机回环测试 |
| P0-2 | AI 生产数据源 | core-bin/startup + ai-engine | rt_source 融合态改接 AiIntegrator 已裁决的真实 phase/电气量（含 modbus 可读电压），替换恒值占位 |
| P0-3 | 国密定位 | security | 按部署合规需求裁决：升级 gmsm/引入真 SM2 签名+SM4-GCM 实现，或文档降级标注"生产 ring 兜底" |
| P1-1 | AiValidator 真接线 | core-bin + strategy-engine | set 真模型或改接 ai-engine ActionValidator 复核后下发；IEC104 p_set 复用校验+clamp |
| P1-2 | 指令 clamp 收敛 | strategy-engine/ai-engine/intercore | send_dual_param 前统一范围校验；k_droop 无 PCS 接口则显式禁/告警 |
| P1-3 | IEC104 数据源 | gateway + southd | 上送源改接 southd meter_grid；端口读 config |
| P1-4 | decisions 落库 | storage + strategy-engine | dispatch_ai_decision 生产路径 insert 决策记录 |
| P1-5 | 联锁/Web 鉴权 | web-api + interlock | 落地真实会话鉴权后再放开联锁写端点 |
| P2 | 批次收敛 | 各模块 | DE/RE 方向进读路径；dead 机收敛（online update/weights/reward）；毒锁 unwrap_or_else；SAFETY 注释；stub 标 NotStarted；mock 兜底禁冒真 |

### 审查总结

**正向确认**：SOC 双源回落（S3b-1d）接线正确（锁不跨 await）；action_validator 自死锁已修复无同类残留；storage events/telemetry/interlock 有真实写方；部署形态文档-代码一致；southd 调度框架完备；PCS 化点表（REG500/1000/1001/1002/1006-1011/1010/1013）与 V1.3 协议匹配。

**项目质量评价**：核心架构与 BECG 落地接线（核间 PCS 化、站级南向调度、联锁）设计扎实、闭环意识强；Rust 并发/FFI 使用总体克制（SAFETY 注释与毒锁为低危批次项）。**最大系统性风险不是"没接线"，而是"接线的数据是占位的"**——这是文档-代码-数据三层的占位叠加（05 文档保留已废输入 → startup 恒值填充 → reward/RL 消费），单看任何一层都"正常"。建议按 P0-1→P0-2→P1-1 顺序穿透修复，使生产路径真正吃到南向真实量，再谈国密合规定位（P0-3）。
