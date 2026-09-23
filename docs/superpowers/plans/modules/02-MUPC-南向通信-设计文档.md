# MUPC 南向通信模块 设计文档

> ✅ **`[DESIGN_APPROVED: 2026-09-21, 设计评审员]`** —— §11「站级南向设备语义点表集成（S3b-2）」经三轮设计评审通过（§10「S3a」沿用既有批准，见 S3a 计划合同）。

> ✅ **`[CODE_REVIEWED: PASS: 2026-09-22]`** —— S3b-2 的 **T1–T7 全部实现并通过项目级最终代码评审门禁**（评审报告见 `../../reports/S3b-2-代码评审报告-2026-09-22.md`）。

> **[§12 按设计评审意见修订（v1.13，2026-09-23）—— 待复审，未门禁]** 设计评审对 §12（v1.12）判 **REJECTED（1 阻塞 + 若干非阻塞）**，本轮逐条修订，**本次未获任何门禁标记**（**不自行添加 `[DESIGN_APPROVED]`**；既有 `[DESIGN_APPROVED: 2026-09-21]` / `[CODE_REVIEWED: PASS: 2026-09-22]` **原文未动**，且其覆盖范围仍**只到 §11**）：
>
> - **①【阻塞·B-1】`judges_evaluable` 作用域错**（旧写法门控**整段**遥测/事件 ⇒ 非承载组恒 `false` ⇒ 静默丢弃其全部位/标量遥测与事件）⇒ **已改为只门控「判据 / 站级量」路径**（§12.4.3 的"三句话" + §12.4.5 的作用域声明 + §12.5 的产出归属两条）；并**修正"合法配置下恒真"的错误自述**（正确的陈述 = "在**承载组**作用域内、配置合法时恒 `true`；**非承载组不适用**"）。
> - **②【补测】§12.8 新增两个用例**：**B-1 判别锚** `non_carrier_group_still_emits_its_bits`（battery 站 + `bms_alarm` 快组 1000 ms / `soc` 块站 2000 ms ⇒ 构造**非承载组**，断言其位遥测与事件**照常产出**；**按旧写法实现必红**）；**守卫负向锚** `station_flag_guard_still_scopes_to_carrier`（fire 站绕过校验构造"判据跨组" ⇒ 断言 `fire_detector_*` 事件恒 0；**删掉守卫即红** —— 钉住"修法是收窄作用域、不是取消守卫"）。
> - **③【建议 a–d】** AC-8-5 补 **C4 的可构造用例**（`T_组 = 801.36 ms` ⇒ `1.5×T_组 = 1202.04 ms`，§12.8）；AC-8-3 钉 **tick 序 `0,1000,…,9000` 与逐项计数**；AC-8-8 **移出用例映射**（元要求，落 §12.9 边界声明）；AC-8-7 ② 退避断言改为 **fail 两轮（`oc=2` ⇒ `2×组周期`）**。
> - **④【口径偏订正】** `mapper.rs:363-372` 实为 **`read_back` 取数段**（**容量算式在 `:377-385`**）；"既有 20 例"订正为 **`scheduler.rs` 现为 44 例（`#[tokio::test]` 38 + `#[test]` 6）**（§12.8 / §12.11）。
> - **⑤【同轮连带】** 评审对 **02 PRD §10** 的 4 项文档级订正 + 4 条建议已**同轮落入 PRD 正文**（PRD v1.13，见 §12.10.2 末的对照表）⇒ 本章与 PRD 的数字/引用**逐位一致**（占用率统一为公式精确值 `0.95 %`；Q-22 选项 B 补第三环）。
> - **改动范围**：只动本文件的 **§12**（§1–§11 零改动）与 `02-MUPC-南向通信-PRD.md` 的 **§10 正文**（评审附项，见 PRD 的 v1.13 行）。**01 号设计的接口登记不属本批改动** —— 它是 `01-MUPC-通信网关-设计文档.md` **自身版本行 `v1.4-r3`** 的内容（来源 = T1 `latest_values` 代码评审的两项设计侧偏离登记，见**该文件**的版本行与 §9.1.3 / §9.1.8 / §9.4）。**不改任何代码/配置**；**12 号设计未动**。

> **[§12 按设计评审意见（第二轮）修订（v1.13-r1，2026-09-23）—— 待复审，未门禁]** 第二轮设计评审对 §12（v1.13）判 **REJECTED（3 阻塞 + 3 警告 + 4 优化）**，但**独立验证了原阻塞 B-1 的修法正确**（`judges_evaluable` 的作用域已改对、占位不用动）。本轮逐条处置，**本次未获任何门禁标记**（**不自行添加 `[DESIGN_APPROVED]`**；既有 `[DESIGN_APPROVED: 2026-09-21]` / `[CODE_REVIEWED: PASS: 2026-09-22]` **原文未动**，其覆盖范围仍**只到 §11**）：
>
> - **①【阻塞 S-1】空 `regs` 站 = 可达 panic + 静默移出调度**：`carrier_group` 旧写法在空 `regs` 上走 `.expect` ⇒ **panic**；且 `from_group` 对该站产 **0** 条目 ⇒ 该站**永不轮询**。该输入**可达**：`scheduler.rs::battery_station_without_soc_block_does_not_push`（`:1515-1535`）**内联构造** `StationConf { regs: vec![], .. }` 并直接 `tick_once(0)`，**不经 `validate`**；而配置期对空 `regs` 的拒绝**只覆盖 `Role::Pcs`**（`config.rs:296-303`），其余 role 中 `battery` 另由**规则 4**（`soc` 点契约，`config.rs:486-488`）拒 ⇒ 该输入的可达性**只**由"该单测**不经 `validate`** 直接内联构造"支撑（**范围订正 v1.13-r2，见 §12.10.2 Δ-12**）。⇒ **已改为**：`read_groups_of` 对空 `regs` **产出一个退化组**（**空块集**、周期 = 站周期、锚 = 哨兵 `EMPTY_GROUP_ANCHOR`）⇒ `from_group` 恒产 **1** 条目、该组即承载组 ⇒ 与既有"**站照常被轮询、读集为空、`poll_to_result(role, &[])` 照常求值、成功/失败记账照旧**"**逐字等价**（不是"不调度"）；`carrier_group` 改返回 **`Option<ReadGroup>`**（构造期**无任何 `expect`/`unwrap` 落在该可达输入上**）；**`.expect` 的理由文案已订正**（原称"空 regs 由配置期规则 3（pcs）与运行期空读路径分别覆盖"——**与事实不符**，配置期只对 `pcs` 拒空）；**V-5 补空集情形**；**新增 Δ-12**（"是否应在配置期对非 `pcs` role 也拒空 `regs`"登记为**待裁定**：空 `regs` 是"静默死配"，但新增配置期拒绝 = **新增约束 = 需求变更** ⇒ **本轮不单方面加**，现状由"逐字保持既有行为"兜住）。**同源加固**：`run_port_round` 里两处 `calc.group_backoff_input(key).unwrap()` 一并去掉 —— `bump_group_fail` 改为**原子返回** `Option<(组周期, 加一后的计数)>`、调用点一律 `if let`（§12.4.2 / §12.4.3）⇒ 该段**无 `unwrap`/`expect`**（同类缺陷的纵深防御：键必存在，但不以 panic 表达）。
> - **②【阻塞 S-2】AC-8-5 的"C4 可构造用例"按字面构造时并不触发 C4**：旧例"同组 3 个 FC04 块 + 取**其中一个**块声明 `interval_ms: 1000`" —— 按 §12.4.1 的分组语义，**只要有块声明了 `interval_ms`，该块就按 `eff` 自成一组**（另两块继承站周期）⇒ 该块所在组 `T_组 = 267.12 ms`、`1.5×T_组 = 400.7 ms ≤ 1000` ⇒ **C4 通过**；实际触发的是 **C9**（`U = 267.12/1000 + 2×267.12/2000 = 0.534 > 0.5`），而 C9 文案不写 `1202` ⇒ "文案含 `1202`"落空、**C4 在 AC-8-5 里无人覆盖**。**修法**：改为**三块均声明** `interval_ms: 1000`（仍为同一读组 ⇒ `T_组 = 801.36 ms` ⇒ `1000 < 1202.04` ⇒ **规则 20（C4）先于规则 23（C9）返回**，文案含 `1202`）；并把**规则 20 的 `T_组` 定义写死**为"**该块所在读组**的整组耗时"（同组内各块 `eff` 相同 ⇒ 组由 `eff` 唯一确定 ⇒ **定义无循环**），消除复审点出的"两种读法必居其一"。**同轮落 PRD §10.7 的 AC-8-5 那一格**（只改"C4 的构造法"的**例子**并补"三块 `eff` 相同 ⇒ 仍为同一读组"一句，**判据未动**；PRD v1.13-r1）。
> - **③【阻塞 S-3】"站级重建全部组基线"的触发条件取错 + 同 tick 组序**：旧伪码用 `if station_was_offline`（`= offline_count > 0`）触发全组基线重建，而 `mark_success`（**清 `offline_count`**，`scheduler.rs:730-737`）**只在承载组分支**调用 ⇒ **承载组持续失败**时非承载组**每一轮**成功都会重建全组 ⇒（i）该组 `primed` 恒 `false` ⇒ **0→1 变化沿事件永不产出**（与 §12.5 表"只受本组基线状态与组级失败重建影响"**直接矛盾**）；（ii）`full_snapshot` 每轮为真 ⇒ 该组**每轮全量落位**（BMS 288 位/轮 ≈ 2.5×10⁷ 行/天，正是 PRD §9.8.1 末条明文告警的量级）。既有实现不会出现（单组 ⇒ 成功即清 `offline_count`）。**修法**：① 触发条件限定为 **`poll.is_carrier && station_was_offline`**（真正的"本轮恢复"）；② **同 tick 组序**改为"**站内承载组优先**"—— `due_round` 排序键由 `(role_priority, station_index, anchor_blk)` 改为 **`(role_priority, station_index, !is_carrier, anchor_blk)`** ⇒ 站恢复当轮**承载组先**重建全组基线、**再**轮到其它组产出（否则快组会先用**离线前**的基线产出一屏，且**下一轮**还要多一次 `full_snapshot`；论证见 §12.4.2）；③ §12.8 补 **S-3 判别锚** `non_carrier_group_events_survive_carrier_failure`（承载组**持续失败**期间，非承载组仍逐轮产变化沿事件、且**不每轮全量落位**；**按旧写法必红**）；④ §12.4.4 连带项 a / §12.5 重建条件表 / §12.10 / §12.11 / §12.12 的"谁能重建基线"表述**全章统一**为：「**站级重建（全组）只由承载组触发**；组级重建（仅本组）由本组 `was_failing` 触发」。
> - **④【警告 W-1】PRD §10.5「单轮最坏耗时」在 §12 无落点，且 C9 不能蕴含它**：新增 **规则 24**（同 `port` 全部组 **`Σ T_组 > 1.5 × 最小非零组周期`** ⇒ 配置期 `Err`，文案含口名 / 站 id / 实测 `Σ T_组` 与阈值）—— **逐字落 PRD §10.5 第 3 行**；并登记"**`U ≤ 0.5` 但 `Σ T` 超标**"的**反例**（同口两组 `(1000, 19.6 ms)` + `(5000, 8×267.12 = 2137 ms)` ⇒ 各组 C4 通过、`U = 0.0196 + 0.4274 = 0.447 ≤ 0.5` 通过，而 `Σ T = 2156.6 ms > 1500 ms`）作为规则 24 的**正当性证据**；**现网复核**：`hvac` `Σ T = 21.68 + 25.84 = 47.52 ms ≤ 1.5 × 1000 = 1500 ms ✓`（其余 4 站为单组站、由 C4 蕴含，`grid_meter` 缺 `T` 同 Δ-11）。**PRD §10.5 一字未动**（需求侧约束已在那儿，本轮只是把它落到设计）。
> - **⑤【警告 W-2】新增引用漂移**：§12.8 用例①（= v1.13 行 Ⅱ 项所指的同一个用例）把"`battery` 回退周期 2000 ms"的依据标为 `02 PRD:1754` ⇒ 订正为 **`:1768`**（"须把 `battery` 的周期上调到 2000ms"，另 `:2208` 同义）；`:1754` 实为"不采点区"表的 `定制保留 / 保留区` 行。**已逐处核**：全文 `:1754` 引用**只此一处**（用例①），其余引用（`:1515-1535` / `:1768` / `:2208` / `config.rs:296-303` / `scheduler.rs:730-737` / `production.yaml:398`）均指向所述内容。 ⚠️ **订正（v1.13-r2）**：本项写下的 ":1768/:2208" 是按**改动前**行号计，而**同一批**又在 PRD 顶部新增 4 行 ⇒ 交付态真值为 **":1772/:2212"**，正文用例①已按真值订正。
> - **⑥【警告 W-3】用例① 的断言时点自相矛盾**：按 tick 序 `0,1000,…,5000`，t=0 建基线（不产事件）、**t=1000 就该产边沿**；旧文写"t=2000 的那一轮必须产出 `is_event == true`"⇒ 按字面**取不到**。**修法**：断言时点改为 **t=1000 轮**（判别力不变），并**逐条复核**了其余数字/时点（`bit_call_count(1,200)==6`、`input_call_count(1,100)==3`、承载组 t=0/2000/4000）—— 改动后**仍自洽**。
> - **⑦【优化 1】文首"改动范围"误导**：旧文写"…与 `01-MUPC-通信网关-设计文档.md` §9.1.3 / §9.4（两处小补登记）"，易读成"本批含 01 号"⇒ 已改写为"**01 号不属本批**，其登记见该文件**自身版本行 `v1.4-r3`**"。
> - **⑧【优化 2】§12.3 标题"一一对应"不实**：C3/C4/C8/C9 无对应 V，V-5/V-6 对应的是 §10.3.1 定性 1 与 §10.6 第 3 条 ⇒ 已**改标题**并补一句**对应关系声明**（去掉"一一对应"）。
> - **⑨【优化 3】§12.6 两处口径错**：① `hvac` 实为 **`parity: even`（8E1）**（`production.yaml:398`），非 8N1 ⇒ 已按 **PRD §9.8.1 的 10 bit/字节口径**显式声明并给 8E1 的敏感性复算（`T_快组 ≈ 23.5 ms` / `T_慢组 ≈ 28.1 ms` / `U_后 ≈ 2.91 %`），**余量极大 ⇒ 结论不变**（且不动 PRD §10.4 的数字）；② 响应帧算式 `(5 + 2N)` 与同表 `FC02 = 8 + (5+4) = 17` **不自洽** ⇒ 已改正为 **`5 + D`**（`D` = **数据字节数**：FC02 = `ceil(位数/8)`；FC03/04 = `2 × 寄存器数`），改正后与表内 `17` / `21` 及 `T_快组 = 21.68 ms` / `T_慢组 = 25.84 ms` **仍一致** ⇒ **无需改 PRD §10.4 的任何数字**；PRD §10.4 的**同一句文字**偏差登记为 **Δ-13**（文档级，不影响任何数字/判据）。
> - **⑩【优化 4】伪码可编译性/冗余字段**：`GroupKey { ..poll.into() }`（需一个**未声明**的 `From<GroupPoll>`）已改**显式构造**（与同段下方同款）；`PortRunner.carrier_anchor` **无使用点**（`poll_group` 查的是 `runner.group_of[&key]`）⇒ **已删除**；§12.5 表的"站级退避 = `backoff_extra(站周期, offline_count)`"与 §12.4.3 注的"承载组自身**组**周期"⇒ **统一为后者**（非退化配置下二者恒等，退化配置下按实际 cadence 才自洽）。
>
> **改动范围（本轮）**：只动本文件的 **§12**（§1–§11 零改动）与 `02-MUPC-南向通信-PRD.md` 的 **§10.7 AC-8-5 那一格（仅"C4 的构造法"的例子）+ 版本行**（PRD v1.13-r1，**判据未动、不构成重新评审**）。**01 号的接口登记属其自身版本行 `v1.4-r3`，不属本批**；**不改任何代码/配置**；**12 号设计未动**。**本次未加任何新门禁标记**（待复审）。

> **[§12 T8 实现发现的合同勘误回写（v1.13-r3，2026-09-24）—— 门禁标记不变]** 依据 = **T8 实现 + 两轮评审**（`28fd0c1` / `609f65c` / `4b0f6c2`）暴露的三处合同文本与实现不符；**只订正合同的行文与理由，不改任何契约、约束、数字与伪码**：
> - **① C8 提示"加载期发射"⇒ 不可达（§12.7 末段）**：`CoreConfig::validate` 属 main **Phase 1**（`main.rs:105`），tracing 到 **Phase 2**（`main.rs:164`）才初始化 ⇒ 配置期日志**无订阅者、被直接丢弃**。**订正为**："判定 = 纯函数 `block_interval_hints`；**发射点在 startup 装配期**（`startup.rs`，晚于 tracing 初始化）"，理由引 `core_config.rs:512-514` 的既有成文约定（判定放配置期、发射放装配期）。
> - **② 规则 24 的"单组站由 C4 自动成立"⇒ 理由错（§12.7）**：C4 只判**显式声明** `interval_ms` 的块，未声明时 C4 **未求值**（现网 4 站正属此列）⇒ 旧理由不成立。**订正为**：口内仅一组时本条**恒被规则 23（C9）先判**（`U > 0.5` 的违规域真包含 `T_组 > 1.5 × 组周期`，因 `0.5 < 1.5`）⇒ 结论（"只在多组口上可能有独立作用、现网零新增拒绝"）不变。
> - **③ 字节耗时"2 位小数取整"⇒ 须追认为口径（§12.6）**：AC-8-5「文案含 `1202`」**依赖** 该取整（不取整得 `1203.94`）⇒ 明写为口径的一部分，并注明非 9600 波特率下 ≤0.5% 的估计偏差。
> **未改**：C1–C9 的任何约束、AC-8-1…AC-8-7 的任何判据、§12.4 的全部伪码与不变量、§12.5/§12.6 的任何数字、§1–§11、任何代码（本行只改合同文本）。**`[DESIGN_APPROVED: 2026-09-23]` 标记原文未动，覆盖范围仍只到 §12**。

> **[§12 评审后文字订正（v1.13-r2，2026-09-23）—— 门禁标记不变]** 本版**只**处置 §12 评审（`[DESIGN_APPROVED: 2026-09-23]`）自己登记的 **3 条遗留项**（评审明写"**须在下一修订版处理**"）与 **4 条优化**：**不引入任何语义 / 约束 / 伪码 / 数字改动**（新增的 **Δ-14** 属**差异登记**、非设计变更）；**紧随本块之后的 `[DESIGN_APPROVED: 2026-09-23, 设计评审员]` 标记原文与其覆盖范围（仅 §12）均不变**（为避免插入本块后产生新的行号漂移，此处以标记原文指代、不写行号）。逐条：
>
> - **①【遗留 1·行号真值】** §12.8 用例① 的 PRD 依据由 `:1768` / `:2208` 订正为**交付态真值 `:1772` / `:2212`**；v1.13-r1 行写下的 ":1768/:2208" 系按**改动前**行号计，属**历史记录不追改**（仅在该行末追加订正说明）。
> - **②【遗留 2·补登记】** 排序键多出的 `!is_carrier` 与 **PRD §10.6 第 3 条**字面不符 ⇒ **新增 Δ-14**（§12.10.2）；§12.4.2 的 `!is_carrier` 论证段末与 §12.3 的 **V-6** 处各补**一处指向 Δ-14** 的交叉引用。**建议需求的修法**：把该条元组订正为 `(角色优先级, 站序, !承载组, 组锚)`（**本轮不改 PRD**）；该细化**不改变**"同口串行、不并发"硬约束。
> - **③【遗留 3·收窄】** Δ-12 的"非 `pcs` role 空 `regs` 能通过校验"**收窄**为"**除 `pcs` 与 `battery` 外**"（`hvac` / `meter_batt` / `fire` / `meter_grid`）—— `Role::Battery` 被**规则 4**（`soc` 点契约，`config.rs:486-488`）拒；**结论（该输入可达）保留**，可达性由"既有单测**不经 `validate`** 直接内联构造"独立支撑。**同批订正**文首 S-1 项与 §12.4.2 的 `.expect` 理由注，**共 3 处**。
> - **④【优化·扩 Δ-13 覆盖面】** Δ-13 由 `(5+2N)` **扩为两处**：同段 **PRD §10.4** 的 **`8N1`**（`02 PRD:2449`；实配 `parity: even` = 8E1）一并入登记；**两处均为文档级、不影响任何数字/判据**（8E1 敏感性复算见 §12.6、结论不变）。
> - **⑤【优化·内部引用项号】** §12.4.4 末的"§12.10.1 **第 4 项**"订正为**第 8 项**（"站级重建的触发者 = 承载组"）。
> - **⑥【优化·版本号】** §12.2.2 末"已删除的字段"注：`carrier_anchor` 系 **v1.12**（非 v1.13）曾声明 ⇒ 改正版本号，并写准删除依据（§12.4.3 伪码**无使用点**；`poll_group` 查 `runner.group_of[&key]`）。
> - **⑦【优化·用例 slave 注】** §12.8 用例① 补注"**本用例自建站（slave = 1）**，与既有 `battery_*` fixture 的 `slave = 2` 无关"（**未改用例任何数字**）。
> - **⑧【记账】** = 本块 + 文首遗留项行的**关闭标记** + 附录版本表新增 **v1.13-r2** 行。**未改任何代码 / 配置 / PRD / 其它文档**。
>

> ✅ **`[DESIGN_APPROVED: 2026-09-23, 设计评审员]`** —— **§12 设计评审通过（v1.13-r1 复审）**。**覆盖范围：仅本文件的 §12（§12.1–§12.12）**；**§1–§11 的 `[DESIGN_APPROVED: 2026-09-21, 设计评审员]` 原文未动、其覆盖范围（§10 / §11）与结论均不变**（`[CODE_REVIEWED: PASS: 2026-09-22]` 同理未动）。
> 复审依据：对返工提交 **`f562b0a`** 逐 hunk 复核（仅命中本文件 §12 + 文首/附录版本表，与 PRD 的 §10.7 AC-8-5 格 + 版本行），并**回代码取证 + 算术复算**——第二轮 3 个阻塞**均已验证为真改**（S-1：空 `regs` 退化组 + `carrier_group` 返 `Option`，`poll_to_result(role, &[])` 有既有覆盖；S-2：三块均声明 `interval_ms` ⇒ `T_组 = 801.36 ms` / `1.5×T_组 = 1202.04 ms`，规则 20 先于规则 23 返回；S-3：门控 `is_carrier && station_was_offline` + 排序键加 `!is_carrier` ⇒ 承载组站内恒最先，两条新增判别锚按旧写法**必红**）。**§1–§11 与既有门禁标记零改动；本批未新增任何其它门禁标记。**
> （**已于 v1.13-r2 关闭**）**遗留项（非阻塞，须在下一修订版处理；不影响 §12 准予进入实现）**：① §12.8 用例①引 PRD 行号 `:1768` / `:2208` 因本批 PRD 顶部新增 4 行而**漂移为 `:1772` / `:2212`**；② 排序键新增的 `!is_carrier` 与 **PRD §10.6 第 3 条**的 `(角色优先级, 站序, 组锚)` **字面不符且未登记 Δ**；③ **Δ-12** 的"非 `pcs` role 的空 `regs` 站能通过校验"对 `battery` 不成立（被规则 4 拒，`config.rs:486-488`）⇒ 其"新增约束=需求变更"论证须限定为 `hvac`/`meter_batt`/`fire`/`meter_grid`。

> **[§12 新增（v1.12，2026-09-23）：块级采集周期覆盖（告警位单独快采，S3b-3）]** 新增 **§12**，落实 02 PRD **§10（v1.12）** 与用户 2026-09-23 就 12 号 R-33 作出的裁定「**告警位单独快采**」。**改动范围**：① 本文件**仅追加 §12**（§1–§11 与既有门禁标记 `[DESIGN_APPROVED: 2026-09-21, 设计评审员]` / `[CODE_REVIEWED: PASS: 2026-09-22]` **原文未动**）；② **未改**任何 PRD、12 号设计（除其 R-45 **那一行**的状态回写，见 02 PRD §10.8.2）、任何代码/配置。**实现状态：尚未实现**（开发由后续 S3b-3 任务做，Task 拆分见 §12.11）。**§12 是新增章节，不覆盖既有门禁**（体例同 §11 由 `[DESIGN_APPROVED]` 单独覆盖）。

> **文档定位：** 本文档记录实现级设计决策。需求级内容（功能描述、验收标准、性能指标）请参考 [02-MUPC-南向通信-PRD](../specs/modules/02-MUPC-南向通信-PRD.md)。

## 目录

1. [模块架构](#1-模块架构)
2. [RS485 设备设计](#2-rs485-设备设计)
3. [协议处理器设计](#3-协议处理器设计)
4. [HPLC 驱动设计](#4-hplc-驱动设计)
5. [动态插件系统设计](#5-动态插件系统设计)
6. [接口定义](#6-接口定义)
7. [文件结构](#7-文件结构)
8. [配置格式](#8-配置格式)
9. [技术决策记录](#9-技术决策记录)
10. [站级多从站统一调度框架](#10-站级多从站统一调度框架)
11. [站级南向设备语义点表集成（S3b-2）](#11-站级南向设备语义点表集成s3b-2)
12. [块级采集周期覆盖（告警位单独快采，S3b-3）](#12-块级采集周期覆盖告警位单独快采s3b-3)

---

## 1. 模块架构

### 1.1 架构概览

南向通信模块采用**分层+插件化**架构，自底向上分为四层：

```
┌──────────────────────────────────────────────────────────────────┐
│                         上层使用者                                 │
│            strategy-engine / data-processing / gateway            │
└──────────────────────────────┬───────────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────┐
│                   统一设备抽象层 (device-trait)                     │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  SouthDevice trait  │  DeviceRegistry trait  │  MessageBus │  │
│  │  ProtocolHandler    │  HplcDriver trait     │  Plugin      │  │
│  │  DataFrame / DeviceError / DeviceStatus     │  PluginLoader│  │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────┬───────────────────────────────────┘
                               │
          ┌────────────────────┼────────────────────┐
          ▼                    ▼                    ▼
┌───────────────────┐ ┌──────────────────┐ ┌───────────────────┐
│   rs485-plugin    │ │   hplc-plugin    │ │   其他插件         │
│  ┌─────────────┐  │ │ ┌──────────────┐ │ │  (未来扩展)       │
│  │ Rs485Device │  │ │ │ HplcDevice   │ │ │                   │
│  │ Modbus      │  │ │ │ MockDriver   │ │ │                   │
│  │ TTU         │  │ │ │ SdkDriver(预留)│ │                   │
│  │ Inverter    │  │ │ └──────────────┘ │ │                   │
│  │ Charger     │  │ └──────────────────┘ │                   │
│  └─────────────┘  │                      │                   │
└───────────────────┘                      └───────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────┐
│                     物理层 (Physical Layer)                       │
│     RS485 总线 (DE/RE GPIO)   /   HPLC 电力线载波 (FFI)          │
└──────────────────────────────────────────────────────────────────┘
```

### 1.2 核心概念

| 概念 | 说明 |
|------|------|
| **SouthDevice** | 所有南向设备的统一接口 abstraction |
| **ProtocolHandler** | RS485 协议处理器，通过依赖注入支持多种协议 |
| **HplcDriver** | HPLC 芯片驱动抽象，支持 Mock 和 SDK 接入 |
| **Plugin** | 动态插件接口，所有南向插件必须实现 |
| **PluginLoader** | 动态插件加载器，管理插件生命周期 |
| **DeviceRegistry** | 设备注册表，管理设备注册/注销/查询 |
| **MessageBus** | 消息总线，设备数据发布/订阅 |

### 1.3 数据流

```
策略引擎 → SouthDevice::write() → Rs485Device/HplcDevice → 物理层
物理层 → SouthDevice::read() → DataFrame → MessageBus → 策略引擎/数据处理
```

### 1.4 设备类型支持

| 设备类型 | 通信方式 | 协议 | 处理器 | 状态 |
|----------|----------|------|--------|------|
| **TTU**（配变终端） | RS485 | 电力行业规约 | `TtuHandler` | 已实现 |
| **光伏逆变器** | RS485 | 厂商私有协议 | `InverterHandler` | 已实现 |
| **充电桩** | RS485 | GB/T 27930 | `ChargerHandler` | 已实现 |
| **柔性负荷控制装置** | RS485 | Modbus RTU | `ModbusHandler` | 已实现 |
| **消防控制系统** | RS485 | Modbus RTU | `ModbusHandler` | 已实现 |
| **HPLC 设备** | 电力线载波 | 芯片 SDK | `MockHplcDriver`(开发) / `SdkHplcDriver`(预留) | 已实现(Mock) |

### 1.5 依赖关系

```
device-trait (无外部依赖)
    ↓
plugin-loader → device-trait
    ↓
rs485-plugin → device-trait (编译为 cdylib 供动态加载)
hplc-plugin  → device-trait (编译为 cdylib 供动态加载)
```

---

## 2. RS485 设备设计

### 2.1 总体设计

RS485 南向通信采用**统一设备抽象 + 协议处理器注入**模式。`Rs485Device` 结构体实现 `SouthDevice` trait，通过依赖注入 `ProtocolHandler` 支持多种设备协议。

```
rs485-plugin/
├── lib.rs                      # 插件入口
├── device.rs                   # Rs485Device 实现（串口操作、DE/RE、事务）
├── config.rs                   # 配置定义 + 验证
├── errors.rs                   # RS485 错误类型
├── protocol.rs                 # 帧解析 + CRC 校验 + 数据单元解析
└── handlers/
    ├── mod.rs                  # 协议处理器注册表 + 本地 CRC
    ├── modbus_handler.rs       # Modbus RTU
    ├── ttu_handler.rs          # TTU 专用协议
    ├── inverter_handler.rs     # 光伏逆变器私有协议
    └── charger_handler.rs      # GB/T 27930 充电桩协议
```

### 2.2 Rs485Device 结构体

```rust
/// RS485 设备驱动
pub struct Rs485Device {
    /// 设备唯一标识
    device_id: String,
    /// 设备类型
    device_type: String,
    /// 配置
    config: Config,
    /// 串口文件描述符
    port_fd: Mutex<Option<RawFd>>,
    /// 设备状态
    status: Mutex<DeviceStatus>,
    /// 是否已打开
    opened: AtomicBool,
    /// 发送锁（保证事务原子性）
    tx_lock: StdMutex<()>,
}
```

**关键方法：**

| 方法 | 说明 |
|------|------|
| `new(device_id, device_type, config)` | 创建 RS485 设备实例 |
| `open()` | 打开串口并配置参数（Unix: libc termios） |
| `close()` | 关闭串口 |
| `send_frame(frame)` | 发送原始数据帧 |
| `recv_frame(timeout_ms)` | 接收原始数据帧 |
| `transaction(request, timeout)` | 原子读-写-读事务 |
| `send_recv(frame, timeout)` | 发送并接收 |
| `read_holding_registers(addr, count)` | Modbus 功能码 0x03 |
| `write_single_register(addr, value)` | Modbus 功能码 0x06 |

### 2.3 串口配置

使用 Unix `libc` termios 直接操作串口，避免第三方库依赖：

```rust
fn configure_port(&self, fd: RawFd) -> Result<(), Rs485Error> {
    // 1. 获取当前终端属性 (tcgetattr)
    // 2. 设置波特率 (cfsetispeed / cfsetospeed)
    // 3. 设置数据位 (CS5/CS6/CS7/CS8)
    // 4. 设置校验位 (PARENB / PARODD)
    // 5. 设置停止位 (CSTOPB)
    // 6. 启用 CLOCAL | CREAD
    // 7. 设置超时 (VTIME / VMIN)
    // 8. 应用设置 (tcsetattr TCSANOW)
    // 9. 刷新缓冲区 (tcflush)
}
```

#### 2.3.1 串口打开语义（`O_NONBLOCK` 的清除 —— 设计约束）

> **约束**：`open()` 按惯例带 `O_NONBLOCK`，其**唯一目的**是防止 `open` 本身因载波（DCD）等待而阻塞；**`configure_port()` 成功后必须立即清除该标志**（`fcntl(F_GETFL)` → `fcntl(F_SETFL, fl & !O_NONBLOCK)`）。
>
> **为什么是硬约束**：若保持非阻塞，则 `recv_frame()` 所依赖的 **`VMIN=0` / `VTIME` 阻塞弱超时读语义失效** —— `read` 在无数据时**立即返回 `EAGAIN`**，而不是等至多 `VTIME×0.1s` ⇒ **任何 Modbus 真从站的请求-响应恒超时**。其现场现象是"全站 `offline` 但配置看起来完全正常"，极难定位（本约束源自真机缺陷 P0-1，2026-09-09 修复）。清除后读超时回到由 termios 的 `VMIN=0`/`VTIME`（按调用方 `timeout_ms` 设置）控制；**发送路径（`write`）不受影响**。
>
> **实现落点**：`mupc/crates/rs485-plugin/src/device.rs::open()`（清除 + 根因注释）。**验证限制**：该代码在 `cfg(unix)` 分支，Windows 本机不编译 ⇒ 相关变更须在 Linux / 目标机 `cargo check -p rs485-plugin` 或真机（pty 回环）验证。

### 2.4 DE/RE GPIO 控制

RS485 为半双工通信，需要通过 GPIO 控制发送使能（DE）和接收使能（RE）。

```rust
pub enum Rs485Dir {
    Recv,  // 接收模式
    Send,  // 发送模式
}

impl Rs485Device {
    fn set_dir(&self, dir: Rs485Dir) -> Result<(), Rs485Error> {
        let gpio_num = match dir {
            Rs485Dir::Send => self.config.de_gpio,
            Rs485Dir::Recv => self.config.re_gpio,
        };
        if let Some(gpio) = gpio_num {
            gpio_set_value(gpio, dir == Rs485Dir::Send)?;
        }
        Ok(())
    }
}
```

`gpio_set_value` 实现（跨平台）：

| 平台 | 实现方式 |
|------|----------|
| Linux | sysfs GPIO: `/sys/class/gpio/gpio{num}/value` |
| Windows | 模拟实现（实际需要 platform-specific 驱动） |
| 其他 | debug 日志模拟 |

**验证规则：** DE 和 RE 引脚不能相同。

### 2.5 事务原子操作

```rust
pub fn transaction(&self, request: &[u8], recv_timeout_ms: u64) -> Result<DataFrame, Rs485Error> {
    let _guard = self.tx_lock.lock();  // 全局锁保证原子性

    // 1. 切换到发送模式
    self.set_dir(Rs485Dir::Send)?;

    // 2. 发送请求
    self.send_frame(request)?;

    // 3. 切换到接收模式
    self.set_dir(Rs485Dir::Recv)?;

    // 4. 接收响应
    let data = self.recv_frame(recv_timeout_ms)?;
    Ok(DataFrame::new(self.device_id.clone(), data))
}
```

**设计要点：**
- 使用 `StdMutex<()>` 作为全局锁，跨异步任务保证设备独占访问
- 发送前切换到发送模式，发送后立刻切换回接收模式
- 超时由 termios `VTIME` 控制，避免阻塞

### 2.6 RS485 通信参数

| 设备类型 | 波特率 | 数据位 | 停止位 | 校验 | 典型轮询周期 |
|----------|--------|--------|--------|------|-------------|
| TTU | 9600 | 8 | 1 | 偶校验 | 1s |
| 光伏逆变器 | 9600 / 19200 | 8 | 1 | 无 | 5s |
| 充电桩 | 19200 | 8 | 1 | 偶校验 | 10s |
| 柔性负荷 | 9600 | 8 | 1 | 无 | 1s |
| 消防控制 | 9600 | 8 | 1 | 无 | 1s |

### 2.7 RS485 错误类型

```rust
#[derive(Debug, Error)]
pub enum Rs485Error {
    OpenFailed(String),    // 串口打开失败
    ConfigFailed(String),  // 串口配置失败
    SendFailed(String),    // 数据发送失败
    RecvFailed(String),    // 数据接收失败
    Timeout,               // 串口读写超时
    CrcFailed(String),     // CRC 校验失败
    NotConnected(String),  // 设备未连接
    GpioError(String),     // GPIO 控制错误
    IoError(#[from] std::io::Error),  // IO 错误
}
```

---

### 2.8 南向控制指令分发（SouthCommandSender）

**来源**：策略引擎模块通过 `SouthCommandSender` trait 向南向设备分发控制指令

**设计目标：**

策略引擎输出的两类南向控制指令通过 `SouthCommandSender` trait 发送到对应设备，与核间通信的 `p_ref`/`k_droop` 双参数指令分离：

```
┌──────────────────────────────────────────────────────────────┐
│                    策略引擎 (strategy-engine)                  │
├──────────────────────────────────────────────────────────────┤
│  p_ref + k_droop  →  IntercoreClient  →  实时控制模块        │  ← 核间通信
│  pv_limit         →  SouthCommandSender  →  光伏逆变器      │  ← 南向通信
│  load_shedding    →  SouthCommandSender  →  负荷控制装置    │  ← 南向通信
└──────────────────────────────────────────────────────────────┘
```

**Trait 定义（定义于 `strategy-engine/src/south_command_sender.rs`）：**

```rust
#[async_trait]
pub trait SouthCommandSender: Send + Sync {
    async fn send_pv_limit(&self, cmd: PvLimitCommand) -> SouthSendResult;
    async fn send_load_shedding(&self, cmd: LoadSheddingCommand) -> SouthSendResult;
}

pub struct PvLimitCommand {
    pub device_id: String,
    pub limit_ratio: f64,      // [0.0, 1.0]
    pub priority: u8,
}

pub struct LoadSheddingCommand {
    pub device_id: String,
    pub power_kw: f64,
    pub priority: u8,
}
```

**实现类：**

| 实现 | 文件 | 说明 |
|------|------|------|
| `MockSouthCommandSender` | `south_command_sender.rs` | 开发/测试用模拟实现 |
| `Rs485SouthSender` | `south_command_sender.rs` | 真实 RS485 通信 |
| `HplcSouthCommandSender` | 预留（未实现） | 真实 HPLC 通信 |

**与核间通信的分工：**

| 指令 | 发送路径 | 目标 |
|------|----------|------|
| `p_ref` (有功基准点) | 核间通信 → 实时控制模块 | 下垂闭环控制 |
| `k_droop` (下垂系数) | 核间通信 → 实时控制模块 | 下垂闭环控制 |
| `pv_limit` (限功率) | 南向通信 → 光伏逆变器 | 防逆流/功率限制 |
| `load_shedding` (切负荷) | 南向通信 → 负荷控制装置 | 需量控制 |

---

## 3. 协议处理器设计

### 3.1 设计模式

采用**策略模式**：`ProtocolHandler` trait 定义编码/解码接口，由具体的处理器实现不同协议。

```
Rs485Device (上下文)
    │
    ├── handler: Arc<dyn ProtocolHandler>  (注入的策略)
    │
    └── transaction() 时调用:
        handler.encode_request(device_id, data) → 编码请求
        handler.decode_response(frame)         → 解码响应
```

### 3.2 ProtocolHandler Trait

```rust
pub trait ProtocolHandler: Send + Sync {
    /// 编码请求数据
    fn encode_request(&self, device_id: &str, data: &[u8]) -> Vec<u8>;

    /// 解码响应数据
    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError>;

    /// 获取协议名称
    fn name(&self) -> &'static str;
}
```

### 3.3 协议处理器注册表

```rust
pub struct ProtocolHandlerRegistry;

impl ProtocolHandlerRegistry {
    pub fn get(name: &str, config: &Config) -> Option<Arc<dyn ProtocolHandler>> {
        match name {
            "modbus"   => Some(Arc::new(ModbusHandler::new(config.device_addr, config.crc_mode))),
            "ttu"      => Some(Arc::new(TtuHandler::new(config.device_addr))),
            "inverter" => Some(Arc::new(InverterHandler::new(config.device_addr))),
            "charger"  => Some(Arc::new(ChargerHandler::new(config.device_addr))),
            _ => None,
        }
    }
}
```

### 3.4 各处理器详情

#### 3.4.1 ModbusHandler

| 属性 | 值 |
|------|-----|
| 协议 | Modbus RTU |
| 帧格式 | `[设备地址][功能码][数据][CRC16高][CRC16低]` |
| 最小帧长 | 5 字节 |
| CRC 验证 | 严格校验，地址+数据不匹配则拒绝 |
| 支持功能码 | 0x01(读线圈)、0x03(读保持寄存器)、0x04(读输入寄存器)、0x05(写线圈)、0x06(写寄存器)、0x10(写多寄存器) |

```rust
impl ProtocolHandler for ModbusHandler {
    fn encode_request(&self, _device_id: &str, data: &[u8]) -> Vec<u8> {
        let mut frame = vec![self.device_addr];
        frame.extend_from_slice(data);
        let crc = crc16_modbus(&frame);
        frame.push((crc >> 8) as u8);
        frame.push(crc as u8);
        frame
    }

    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError> {
        // 验证最小长度、设备地址、CRC
        if frame.len() < 5 { return Err(...); }
        if frame[0] != self.device_addr { return Err(...); }
        // CRC 校验
        // ...
        Ok(DataFrame::new(format!("modbus_{}", self.device_addr), frame.to_vec()))
    }
}
```

#### 3.4.2 TtuHandler

| 属性 | 值 |
|------|-----|
| 协议 | 电力行业规约（类 101 规约简化版） |
| 帧格式 | `[0x68][版本][数据长度][数据载荷][校验和][0x16]` |
| 校验方式 | 累加和校验 |
| 数据长度 | 单字节，最大 255 |

```rust
impl ProtocolHandler for TtuHandler {
    fn encode_request(&self, _device_id: &str, data: &[u8]) -> Vec<u8> {
        let mut frame = vec![0x68];  // 起始符
        frame.push(self.protocol_version);  // 版本
        frame.push(data.len() as u8);  // 数据长度
        frame.extend_from_slice(data);  // 数据载荷
        let checksum: u8 = frame[1..].iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
        frame.push(checksum);  // 校验和
        frame.push(0x16);  // 结束符
        frame
    }
}
```

#### 3.4.3 InverterHandler

| 属性 | 值 |
|------|-----|
| 协议 | 光伏逆变器厂商私有协议 |
| 帧格式 | `[0x01][数据长度高][数据长度低][数据载荷]` |
| 数据长度 | 2 字节大端序 |

#### 3.4.4 ChargerHandler

| 属性 | 值 |
|------|-----|
| 协议 | GB/T 27930（电动汽车充电通信） |
| 帧格式 | `[0xFF][0xFE][协议版本][数据载荷][校验和]` |
| 校验方式 | 累加和（从协议版本开始） |

#### 3.4.5 消防控制（预留扩展）

消防控制系统使用 Modbus RTU 协议，通过 `ModbusHandler` 实现。如需求定制协议，可新增 `FireAlarmHandler`。

### 3.5 协议解析层

`protocol.rs` 提供底层帧解析和 CRC 计算：

```rust
pub struct Frame {
    pub addr: u8,        // 设备地址
    pub func_code: u8,   // 功能码
    pub data: Vec<u8>,   // 数据载荷
    pub crc: u16,        // CRC 校验码
}
```

**支持的 CRC 模式：**
- `Crc16Modbus` — Modbus CRC16 (x^16 + x^15 + x^2 + 1)
- `Crc16Xmodem` — XMODEM CRC16 (x^16 + x^12 + x^5 + 1)
- `None` — 无校验

**DataUnitParser** 提供数据单元解析工具：

| 方法 | 说明 |
|------|------|
| `parse_i16(data)` | 解析 16 位有符号整数 |
| `parse_u16(data)` | 解析 16 位无符号整数 |
| `parse_f32(data)` | 解析 32 位浮点数 |
| `pack_u16(value)` | 打包 16 位无符号整数 |
| `pack_i16(value)` | 打包 16 位有符号整数 |
| `pack_f32(value)` | 打包 32 位浮点数 |

---

## 4. HPLC 驱动设计

### 4.1 总体设计

HPLC（高速电力线载波）模块采用**通用驱动抽象 + 芯片 SDK 后续集成**策略。

- **Phase 2**：实现 Mock 驱动用于开发和验证数据通路
- **Phase 3**：芯片 SDK 绑定（预留接口）

```
hplc-plugin/
├── lib.rs              # 插件入口 + FFI 导出
├── driver.rs           # HplcDriver trait
├── device.rs           # HplcDevice（实现 SouthDevice）
├── mock.rs             # MockHplcDriver（开发/测试用）
├── errors.rs           # HplcError
└── config.rs           # 配置定义
```

### 4.2 HplcDriver Trait

```rust
pub trait HplcDriver: Send + Sync {
    /// 转换为 Any，用于 downcasting（获取实际类型引用）
    fn as_any(&self) -> &dyn Any;

    /// 初始化驱动
    fn init(&self, config: HplcConfig) -> Result<(), HplcError>;

    /// 发送数据
    fn send(&self, data: &[u8]) -> Result<(), HplcError>;

    /// 接收数据（阻塞，超时返回空）
    fn recv(&self, timeout_ms: u64) -> Result<Vec<u8>, HplcError>;

    /// 检查连接状态
    fn is_connected(&self) -> bool;

    /// 获取驱动名称
    fn driver_name(&self) -> &'static str;
}
```

### 4.3 HplcConfig

```rust
pub struct HplcConfig {
    pub port: String,              // 串口路径（Linux=/dev/ttyUSB0, Windows=COM3）
    pub baud_rate: u32,            // 波特率
    pub chip_type: Option<String>, // 芯片型号（FFI 预留）
    pub channel: Option<u8>,       // 通道号
}
```

**JSON 别名支持：** `serial_port`、`com_port` 均可作为 `port` 的别名。

### 4.4 HplcDevice

```rust
pub struct HplcDevice {
    device_id: String,
    device_type: String,
    config: HplcConfig,
    driver: Arc<dyn HplcDriver>,
    status: Mutex<DeviceStatus>,
}
```

`HplcDevice` 实现 `SouthDevice` trait：
- `connect()` 调用 `driver.init()`
- `read()` 调用 `driver.recv()`
- `write()` 调用 `driver.send()`
- `health_check()` 调用 `driver.is_connected()`

### 4.5 MockHplcDriver

```rust
pub struct MockHplcDriver {
    connected: AtomicBool,
    mock_queue: Mutex<Vec<Vec<u8>>>,  // 模拟数据队列
    mock_delay_ms: AtomicU64,         // 模拟延迟
}
```

**能力：**
- `inject_data(data)` — 注入模拟数据到接收队列
- `set_mock_delay_ms(ms)` — 设置模拟延迟
- 支持多次连续注入和接收（FIFO 队列）

**用途：** 开发和测试阶段使用，不依赖实际硬件。

### 4.6 SdkHplcDriver（预留，Phase 3）

```rust
pub struct SdkHplcDriver {
    handle: *mut c_void,  // FFI 句柄
}

impl HplcDriver for SdkHplcDriver {
    fn init(&self, config: HplcConfig) -> Result<(), HplcError> {
        // 调用 libhplc.so 中的 hplc_init()
        unsafe { hplc_init(config.port.as_ptr(), config.baud_rate) }
    }
    fn send(&self, data: &[u8]) -> Result<(), HplcError> {
        unsafe { hplc_send(self.handle, data.as_ptr(), data.len() as u32) }
    }
    fn recv(&self, timeout_ms: u64) -> Result<Vec<u8>, HplcError> {
        unsafe { hplc_recv(self.handle, buf.as_mut_ptr(), buf.len() as u32, timeout_ms as i32) }
    }
}
```

### 4.7 HPLC 技术参数（预留）

| 参数 | 规格 |
|------|------|
| 调制方式 | OFDM（BPSK/QPSK/16QAM/64QAM 自适应） |
| 通信频段 | 0.7 MHz - 3 MHz |
| 物理层速率 | 2 Mbps - 10 Mbps（自适应） |
| 最大帧长 | 1500 字节 |
| 典型应用 | 台区全覆盖，替代 RS485 布线困难区域 |

### 4.8 HPLC 错误类型

```rust
#[derive(Debug, Error)]
pub enum HplcError {
    InitFailed(String),    // 驱动初始化失败
    SendFailed(String),    // 发送失败
    RecvFailed(String),    // 接收失败
    Disconnected(String),  // 连接断开
    SdkError(String),      // SDK 错误
}
```

---

## 5. 动态插件系统设计

### 5.1 架构

动态插件系统通过 FFI 绑定实现运行时加载/卸载 `.so` / `.dll` / `.dylib` 动态库。

```
PluginLoader (plugin-loader crate)
    ↓
libloading (动态加载 .so/.dll)
    ↓
FFI 导出函数: create_plugin() + plugin_meta()
    ↓
Plugin trait 实例 (dyn Plugin trait object)
```

### 5.2 Plugin Trait

```rust
pub trait Plugin: Send + Sync {
    fn meta(&self) -> PluginMeta;
    fn init(&self, config: serde_json::Value) -> Result<(), PluginError>;
    fn start(&self) -> Result<(), PluginError>;
    fn stop(&self) -> Result<(), PluginError>;
    fn shutdown(self: Box<Self>) -> Result<(), PluginError>;
}
```

### 5.3 PluginMeta

```rust
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}
```

### 5.4 插件生命周期

```
Load ──→ Init ──→ Start ──→ Stop ──→ Unload
```

| 阶段 | 操作 | 状态 |
|------|------|------|
| **Load** | 使用 `libloading` 加载 `.so/.dll`，调用 `create_plugin()` 获取实例，调用 `plugin_meta()` 获取元信息 | `Loaded` |
| **Init** | 调用 `plugin.init(config)` 传入 JSON 配置 | `Initialized` |
| **Start** | 调用 `plugin.start()` 启动业务逻辑 | `Running` |
| **Stop** | 调用 `plugin.stop()` 停止业务逻辑 | `Stopped` |
| **Unload** | 调用 `plugin.shutdown()`，从注册表移除，卸载动态库 | `Unloaded` |

### 5.5 必需 FFI 导出符号

每个动态插件必须导出以下两个 `extern "C"` 函数：

```rust
#[no_mangle]
pub unsafe extern "C" fn create_plugin() -> *mut dyn Plugin {
    Box::into_raw(Box::new(MyPlugin::new())) as *mut dyn Plugin
}

#[no_mangle]
pub unsafe extern "C" fn plugin_meta() -> PluginMeta {
    MyPlugin::new().meta()
}
```

### 5.6 PluginLoader Trait

```rust
pub trait PluginLoader: Send + Sync {
    fn load(&self, plugin_path: &str, config: Value) -> Result<(), PluginError>;
    fn unload(&self, plugin_name: &str) -> Result<(), PluginError>;
    fn list(&self) -> Vec<PluginMeta>;
    fn get(&self, plugin_name: &str) -> Option<Arc<dyn Plugin>>;
    fn is_loaded(&self, plugin_name: &str) -> bool;
    fn plugin_count(&self) -> usize;
    fn unload_all(&self) -> Result<(), PluginError>;
}
```

### 5.7 PluginLoaderImpl

核心实现（使用 `libloading`）：

```rust
pub struct PluginLoaderImpl {
    plugins: RwLock<HashMap<String, PluginHandle>>,
    search_paths: RwLock<Vec<String>>,
}
```

**加载流程：**
1. 检查插件是否已加载（防止重复加载）
2. 使用 `libloading::Library::new()` 加载动态库
3. 通过 `library.get(b"create_plugin")` 获取工厂函数
4. 通过 `library.get(b"plugin_meta")` 获取元信息
5. 调用 `create_fn()` 创建插件实例
6. 存储到 `HashMap<String, PluginHandle>`

**卸载流程：**
1. 从 `HashMap` 中移除 `PluginHandle`
2. `PluginHandle` 被 `drop`，自动释放 `Library`（卸载 `.so`）

### 5.8 插件注册表 (PluginRegistry)

管理插件的元信息和生命周期状态，与 PluginLoader 配合使用：

```rust
pub struct PluginRegistry {
    entries: RwLock<HashMap<String, PluginEntry>>,
}
```

**能力：**
- `register` / `unregister` — 注册/注销插件
- `get` / `names` — 查询插件
- `query_by_state(state)` — 按状态查询
- `update_state` — 更新插件状态

### 5.9 编译要求

```toml
[lib]
crate-type = ["cdylib"]  # 必须编译为动态库
```

| 平台 | 输出 |
|------|------|
| Linux | `target/release/libmy_plugin.so` |
| Windows | `target/release/my_plugin.dll` |
| macOS | `target/release/libmy_plugin.dylib` |

### 5.10 插件错误类型

```rust
#[derive(Debug, Error)]
pub enum PluginError {
    LoadFailed(String),    // 插件加载失败
    InitFailed(String),    // 插件初始化失败
    StartFailed(String),   // 插件启动失败
    StopFailed(String),    // 插件停止失败
    NotFound(String),      // 插件不存在
    MetaError(String),     // 元信息错误
    Other(String),         // 其他错误
}
```

---

## 6. 接口定义

### 6.1 SouthDevice Trait

```rust
pub trait SouthDevice: Send + Sync {
    fn device_id(&self) -> &str;
    fn device_type(&self) -> &str;
    fn status(&self) -> Result<DeviceStatus, DeviceError>;
    fn connect(&self) -> Result<(), DeviceError>;
    fn disconnect(&self) -> Result<(), DeviceError>;
    fn read(&self) -> Result<DataFrame, DeviceError>;
    fn read_batch(&self, count: usize) -> Result<Vec<DataFrame>, DeviceError>;
    fn write(&self, data: &[u8]) -> Result<(), DeviceError>;
    fn health_check(&self) -> Result<bool, DeviceError>;
}
```

### 6.2 Device Trait（早期抽象，SouthDevice 的前身）

```rust
pub trait Device: Send + Sync {
    fn read(&self) -> Result<DataFrame, DeviceError>;
    fn write(&self, data: &[u8]) -> Result<(), DeviceError>;
    fn status(&self) -> Result<DeviceStatus, DeviceError>;
    fn device_id(&self) -> &str;
    fn device_type(&self) -> &str;
}
```

> **说明：** `Device` 是 Phase 1 的早期抽象，`SouthDevice` 是增强版本（增加了 `connect/disconnect/read_batch/health_check`）。`Rs485Device` 同时实现了 `Device` 和 `SouthDevice` trait。

### 6.3 DeviceRegistry Trait

```rust
pub trait DeviceRegistry: Send + Sync {
    fn register(&self, device: Arc<dyn Device>) -> Result<(), RegistryError>;
    fn unregister(&self, device_id: &str) -> Result<(), RegistryError>;
    fn get(&self, device_id: &str) -> Option<Arc<dyn Device>>;
    fn query_by_type(&self, device_type: &str) -> Vec<Arc<dyn Device>>;
    fn list_all(&self) -> Vec<String>;
    fn count(&self) -> usize;
    fn clear(&self) -> Result<(), RegistryError>;
}
```

**设备查询条件：**

```rust
pub struct DeviceQuery {
    pub device_type: Option<DeviceType>,
    pub status_online: Option<bool>,
    pub tags: Option<Vec<String>>,
}
```

### 6.4 MessageBus Trait

```rust
pub trait MessageBus: Send + Sync {
    fn publish(&self, topic: &Topic, msg: Message) -> Result<(), BusError>;
    fn subscribe(&self, topic: &Topic, handler: Arc<dyn MessageHandler>) -> Result<(), BusError>;
    fn unsubscribe(&self, topic: &Topic, handler_id: &str) -> Result<(), BusError>;
}

pub trait MessageHandler: Send + Sync {
    fn handle(&self, message: &Message) -> Result<(), BusError>;
}
```

### 6.5 核心数据类型

**设备状态：**

```rust
pub enum DeviceStatus {
    Online,
    Offline,
    Error(String),   // 设备故障
}
```

**数据帧：**

```rust
pub struct DataFrame {
    pub device_id: String,  // 设备唯一标识
    pub timestamp: u64,     // 时间戳（毫秒）
    pub data: Vec<u8>,      // 数据载荷
    pub quality: DataQuality, // 数据质量
}
```

**数据质量：**

```rust
pub enum DataQuality {
    Good,      // 数据有效
    Invalid,   // 数据无效
    Reserved,  // 保留
}
```

**设备类型枚举：**

```rust
pub enum DeviceType {
    Ttu,           // 配变终端
    Inverter,      // 光伏逆变器
    Charger,       // 充电桩
    FlexibleLoad,  // 柔性负荷
    FireAlarm,     // 消防控制
    Unknown,       // 未知类型
}
```

**设备 ID 命名规范：**
```
格式: {设备类型}_{厂商}_{型号}_{序号}
示例: ttu_huawei_osu_001, inverter_sungrow_sg100_001
```

**消息系统：**

```rust
pub struct Topic(String);

pub struct Message {
    pub topic: Topic,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}
```

**测量值：**

```rust
pub struct Measurement {
    pub name: String,       // 测量点名称
    pub value: f64,         // 测量值
    pub unit: Option<String>,  // 单位
}
```

### 6.6 错误类型定义

**DeviceError（统一设备错误）：**

```rust
pub enum DeviceError {
    Offline(String),        // 设备离线
    Timeout(String),        // 通信超时
    ChecksumFailed(String), // 数据校验失败
    ProtocolError(String),  // 协议错误
    Busy(String),           // 设备忙
    IoError(std::io::Error),// IO 错误
    Other(String),          // 其他错误
}
```

**RegistryError（注册表错误）：**

```rust
pub enum RegistryError {
    AlreadyExists(String),   // 设备已存在
    NotFound(String),        // 设备不存在
    RegisterFailed(String),  // 注册失败
    UnregisterFailed(String),// 注销失败
    Other(String),           // 其他错误
}
```

**BusError（总线错误）：**

```rust
pub enum BusError {
    TopicNotFound(String),   // 主题不存在
    PublishFailed(String),   // 发布失败
    SubscribeFailed(String), // 订阅失败
    UnsubscribeFailed(String),// 取消订阅失败
    Other(String),           // 其他错误
}
```

---

## 7. 文件结构

### 7.1 完整文件树

```
mupc/crates/
│
├── device-trait/                          # 设备抽象层
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                         # 模块导出 + re-export
│       ├── device.rs                      # Device trait (早期抽象)
│       ├── south_device.rs                # SouthDevice trait + ProtocolHandler + HplcDriver + 处理器实现
│       ├── registry.rs                    # DeviceRegistry trait + DeviceQuery
│       ├── message_bus.rs                 # MessageBus trait + MessageHandler
│       ├── plugin.rs                      # Plugin trait + PluginState + NoOpPlugin
│       ├── plugin_loader.rs               # PluginLoader trait
│       ├── types.rs                       # DataFrame, DeviceStatus, DeviceType, Topic, Message, CrcMode, Rs485Config, Parity, 等
│       └── errors.rs                      # DeviceError, PluginError, BusError, RegistryError
│
├── rs485-plugin/                          # RS485 驱动插件
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                         # 插件入口 + re-export
│       ├── device.rs                      # Rs485Device (串口操作, DE/RE GPIO, 事务)
│       ├── config.rs                      # Config (串口参数, de_gpio, re_gpio, 验证)
│       ├── errors.rs                      # Rs485Error
│       ├── protocol.rs                    # Frame 解析, CRC 计算, DataUnitParser
│       └── handlers/
│           ├── mod.rs                     # ProtocolHandlerRegistry + 本地 CRC
│           ├── modbus_handler.rs           # Modbus RTU
│           ├── ttu_handler.rs              # TTU 配变终端
│           ├── inverter_handler.rs         # 光伏逆变器
│           └── charger_handler.rs          # GB/T 27930 充电桩
│
├── hplc-plugin/                           # HPLC 驱动插件
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                         # 插件入口 + FFI 导出
│       ├── config.rs                      # HplcConfig
│       ├── driver.rs                      # HplcDriver trait
│       ├── device.rs                      # HplcDevice (SouthDevice 实现)
│       ├── mock.rs                        # MockHplcDriver
│       └── errors.rs                      # HplcError
│
└── plugin-loader/                         # 动态插件加载器
    ├── Cargo.toml
    └── src/
        ├── lib.rs                          # 模块导出 + re-export
        ├── loader.rs                       # PluginLoaderImpl (libloading 实现)
        ├── registry.rs                     # PluginRegistry + PluginEntry + PluginState
        └── errors.rs                       # LoaderError
```

### 7.2 device-trait Cargo.toml

```toml
[package]
name = "device-trait"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
thiserror = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
```

### 7.3 rs485-plugin Cargo.toml

```toml
[package]
name = "rs485-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["lib", "cdylib"]

[dependencies]
device-trait = { path = "../device-trait" }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
parking_lot = { workspace = true }

[target.'cfg(unix)'.dependencies]
libc = "0.2"

[dev-dependencies]
tempfile = "3"
```

### 7.4 hplc-plugin Cargo.toml

```toml
[package]
name = "mupc-hplc-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["lib", "cdylib"]

[dependencies]
device-trait = { path = "../device-trait" }
thiserror = { workspace = true }
parking_lot = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tokio = { workspace = true }

[features]
ffi = []
```

### 7.5 plugin-loader Cargo.toml

```toml
[package]
name = "plugin-loader"
version = "0.1.0"
edition = "2021"

[dependencies]
device-trait = { path = "../device-trait" }
libloading = "0.8"
parking_lot = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
```

---

## 8. 配置格式

### 8.1 RS485 设备配置

```json
{
  "rs485_devices": [
    {
      "device_id": "ttu_001",
      "device_type": "ttu",
      "port": "/dev/ttyUSB0",
      "baud_rate": 9600,
      "data_bits": 8,
      "stop_bits": 1,
      "parity": "even",
      "timeout_ms": 1000,
      "device_addr": 0x01,
      "handler": "ttu",
      "de_gpio": 17,
      "re_gpio": 27
    },
    {
      "device_id": "inverter_001",
      "device_type": "inverter",
      "port": "/dev/ttyUSB1",
      "baud_rate": 19200,
      "handler": "inverter",
      "de_gpio": 18,
      "re_gpio": 22
    }
  ]
}
```

**字段说明：**

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | 是 | — | 设备唯一标识 |
| `device_type` | string | 是 | — | 设备类型 |
| `port` | string | 是 | — | 串口路径 |
| `baud_rate` | int | 是 | — | 波特率 |
| `data_bits` | int | 否 | 8 | 数据位 |
| `stop_bits` | int | 否 | 1 | 停止位 |
| `parity` | string | 否 | "none" | 校验位 |
| `timeout_ms` | int | 否 | 1000 | 通信超时 |
| `device_addr` | int | 否 | 0x01 | 设备地址 |
| `handler` | string | 是 | — | 协议处理器名称 |
| `de_gpio` | int | 否 | null | DE 引脚编号 |
| `re_gpio` | int | 否 | null | RE 引脚编号 |

### 8.2 HPLC 设备配置

```json
{
  "hplc_devices": [
    {
      "device_id": "hplc_001",
      "device_type": "hplc",
      "driver": "mock",
      "config": {
        "serial_port": "/dev/ttyUSB2",
        "baud_rate": 115200,
        "chip_type": null,
        "channel": null
      }
    }
  ]
}
```

### 8.3 插件通用配置

```json
{
  "device_path": "/dev/ttyUSB0",
  "timeout_ms": 5000,
  "baud_rate": 9600
}
```

插件配置通过 `serde_json::Value` 传入 `plugin.init(config)` 方法。

---

## 9. 技术决策记录

### 9.1 架构决策

| 决策 | 选择 | 替代方案 | 理由 |
|------|------|----------|------|
| 设备抽象层次 | `SouthDevice` + `Device` 并存 | 统一为单个 trait | 向后兼容 Phase 1 的 `Device` 接口，同时提供增强的 `SouthDevice` |
| 协议扩展方式 | 策略模式 (ProtocolHandler 注入) | 继承/泛型参数 | 运行时可选，配置驱动，无需重新编译 |
| 串口操作 | 直接使用 `libc` termios | `serial` crate | 减少外部依赖，更细粒度控制 |
| 插件隔离 | 同一进程加载（trait object） | 子进程隔离 | 子进程 IPC 开销大，Rust 类型系统可保证安全 |
| 插件 FFI | `unsafe extern "C" fn` | C ABI struct | 简化绑定，`libloading` 原生支持 |

### 9.2 风险与对策

| 风险 | 等级 | 对策 |
|------|------|------|
| RS485 电气特性导致通信不稳定 | 低 | 增加重试机制和超时控制 |
| 插件隔离不足导致崩溃影响主进程 | 中 | 使用 Rust 的 Safe Trait 约束，`catch_unwind` |
| `libloading` 在 Windows 平台兼容性 | 低 | 测试阶段覆盖 Windows 环境 |
| 多线程竞争串口访问 | 低 | `StdMutex<()>` 保证事务原子性 |

**重试机制与故障隔离（实现级）：**

- **事务超时**：单次 Modbus 请求-响应超时，默认 1000ms（配置项 `timeout_ms`，见 §8.1 配置表）
- **重试**：超时后按可配置次数重试（对齐 PRD §7.2「可配置重试次数和超时时间」）
- **故障隔离**：单设备通信故障不影响其他设备（对齐 PRD §7.2）；故障设备跳过本轮轮询，独立标记离线并告警，恢复后自动重新上线
- **重试边界**：仅对超时/无响应/CRC 错重试；地址非法、数据非法等确定性错误直接返回错误、不重试

### 9.3 验收标准

> 验收标准（功能验收、质量验收）详见 [02-MUPC-南向通信-PRD](../specs/modules/02-MUPC-南向通信-PRD.md) 第 7 章。

## 10. 站级多从站统一调度框架

> **目标平台**：BECG-3568（RK3568，后续 RK3588 接口一致），板载 **8 路隔离 RS485（每路独立 Modbus master）**。接线分配见 **核间 10 §12.1** 与 deploy/deploy.md §九（现场接线与配置核对）。

### 10.1 背景与目标（Why）

现南向采集是 **core-bin 逐个硬编码 task**（台区总表一个、南向 pv/load 一个），`DeviceRegistry`/`MessageBus` trait 存在但未接线；AiIntegrator 仅 `latest_data` 单输入。新增站级从站（BMS/空调/储能表/消防状态）若沿用硬编码会失控。目标：把「南向站级采集」收敛为**配置驱动的统一多端口多从站调度器**（新 crate `mupc-southd`），复用既有 rs485-plugin 串口/handler 与 meter_regs 解码；每口独立 master 并行、口内多从站串行轮询；站级故障隔离。

**范围**：BMS(RS485-2/ttyS2)、空调(RS485-3/ttyS3)、关口表/台区总表(RS485-4/ttyS4)、储能表(RS485-5/ttyS5)、消防状态(RS485-6/ttyS6)。PCS(RS485-1/ttyS0) 走 intercore `modbus_rtu`（核间 10），**不在**本调度器内。**首版边界**：严格「采集 + meter_grid phase 输入 / battery SOC 输入 + 事件上送」，**不承载控制写侧**（空调只读遥测，温控/启停写指令与 pv/load 命令下发属未来负载管理，另设计）。**与 §9.2 重试条款关系**：§9.2 基于「单总线多设备」假设；本框架 BECG 每口独立 master，站超时隔离同口其它站，确定性错误不重试，语义见 §10.7。

### 10.2 架构

```
mupc-southd（新 crate）
├─ SouthScheduler         # 口级调度：每 port 一条采集 task
│   ├─ PortRuntime        # 串口 fd + tx 锁 + 口状态（BECG 每口独立 master）
│   │   └─ StationLoop    # 口内从站串行轮询（同口多从站），站超时→该站 offline
│   └─ Station            # 配置驱动：port + protocol + slave + interval + regs + role
├─ role 映射（Station→DataPackage）：meter_grid/meter_batt/battery/hvac/fire
└─ 上送：telemetry 落库 + role 分发（grid_meter → AiIntegrator.latest_data）
依赖复用：rs485-plugin 串口/ModbusRTU handler、data-processing meter_regs 解码、storage telemetry/events
```

- **线程承载**：rs485-plugin 串口为同步阻塞 fd，每口采集 task 内以 `spawn_blocking`/独立线程承载阻塞读写（避免多口阻塞占满 async worker）。
- 每站独立 `port` → 不同口并行（BECG 隔离 485 各自 master）；同 `port` 多站 → 口内串行轮询（现有 Rs485Device 事务 tx 锁机制扩展，单请求 slave 参数化——当前 Rs485Device 固定 device_addr，框架内新增「请求级 slave 覆盖」）。**口调度预算**：站按 `interval_ms` 计算 next_due；同口慢从站超时阻塞轮询时，优先保 `role=meter_grid/battery` 的站（防超 5s stale），慢站按 offline 计并降频，不拖累关键站 cadence。
- **故障隔离**：站级请求超时/CRC 错 → 标记该站 offline 并告警，跳过本轮；不牵连同口其它站；恢复后自动上线。

### 10.3 stations 配置（core_config `south_stations:` 段）

```yaml
south_stations:
  poll_ms: 1000            # 缺省轮询周期
  stale_timeout_s: 5       # 上送数据过期阈值（对齐 AiIntegrator 5s）
  stations:
    - { id: grid_meter,    role: meter_grid, port: ttyS4, protocol: modbus, slave: 1, interval_ms: 1000, regs: <分相 p/q/pf/u/i 点表> }
    - { id: meter_storage, role: meter_batt,  port: ttyS5, protocol: modbus, slave: 1, interval_ms: 1000, regs: <储能表点表> }
    - { id: bms,           role: battery,     port: ttyS2, protocol: modbus, slave: 1, interval_ms: 1000, regs: <BMS 点表> }
    - { id: ac_unit,       role: hvac,        port: ttyS3, protocol: modbus, slave: 1, interval_ms: 5000, regs: <空调点表> }
    - { id: fire_host,     role: fire,        port: ttyS6, protocol: modbus, slave: 1, interval_ms: 1000, regs: <消防状态点表> }
```

- 点表 `regs`：配置化寄存器映射，复用 meter_regs `RegFormat`（float32/int32_scaled）解码。
- **master_meter 收敛**：现有 `master_meter` 配置段与硬编码总表 task **收敛为 `role: meter_grid` 站**（行为回归等价，策略 phase 输入链路不变）；`master_meter.enabled/serial_port` 语义保留为 meter_grid 入口（兼容别名，实施后统一单写方 = southd mapper **唯一** `set_latest_data`，移除硬编码总表 task，避免双写 AiIntegrator）。**迁移期排他**：`master_meter` 段与 `south_stations.meter_grid` 站**二选一启用**（validate 互斥，禁双 master 同总线）；收敛目标形态 = 删除 `master_meter` 段、总表统一走 `south_stations`（分批：先单写守卫断言 → 后移除 alias）。
- **跨段校验（收敛后迁移）**：`south_stations.port` 与 `intercore.modbus_rtu.serial_port`（PCS ttyS0）**不得重复**（双 master 共总线禁止）；`meter_grid` 站 `interval_ms < 5000`（对齐策略数据新鲜度）；原 `master_meter` 段 `/dev/ttyUSB0` 特判随收敛移除（BECG 无 USB 概念），校验迁至 `south_stations` 段。
- **新鲜度共享常量**：策略 5s 数据新鲜度与各站 `interval_ms` 边界统一引用共享常量 `data_freshness_ms`（避免三处硬编码漂移）。
- **BMS 多包/多块（扩展点）**：真实 BMS 若多从站包或多寄存器块，以「同口多从站各包一站」或扩展 `Station.regs` 为多块聚合处理；SOC 聚合规则待厂方点表确认后落地。
- **协议**：第一版按通用 Modbus RTU + 配置点表；BMS/空调/消防主机若厂家私有帧 → 追加 `ProtocolHandler` 实现（registry 已有扩展点），私有点表待厂方提供后落地。

### 10.4 role → DataPackage 语义映射

| role | 产出字段 | 去向 |
|---|---|---|
| meter_grid | `electrical.phase`（分相 p/q/pf/u/i，总表） | `AiIntegrator.latest_data`（策略 phase 源，语义不变）+ telemetry + IEC104 |
| meter_batt | `electrical`（储能表总能/总无功） | telemetry + 展示/校核 |
| battery | `battery.soc/soh/temperature` + 告警字 | telemetry + 事件 + SOC 融合（见 10.5） |
| hvac | `device_status` 环境量/状态字 | telemetry + 事件 |
| fire | `device_status` 消防状态字 | 事件（与 DI3 消防报警融合规则见下） |

> **fire(RS485) 与 DI3 消防报警融合**：同一消防信号可能双源接入。规则：DI 干接点为**高完整性主判据**（触发即报，RS485 作确认/校核）；触发取 **OR**（任一源触发即触发）——OR 仅限 fire 状态/告警事件；**停机仍仅以 DI3 触发**（见核间 10 §12.3）；恢复需**双方复位**；`events` 以「消防+源」去重键防双报。详细联锁语义见核间 10 §12.3。

### 10.5 与策略的接口（SOC 源融合）

BMS 站在线时其 SOC **优先**于 intercore `latest_soc`（核间回读）注入策略 `TaiStorageStrategy`；BMS 掉线回落核间 SOC（可配优先级）。生效点：04 策略引擎 §2.11 AiIntegrator 数据注入。本版仅采集 + SOC 输入，不做其它策略融合。

**推包与新鲜度闸门**：`set_latest_data` **仅由 `meter_grid`（phase 真源）更新触发**推进 AiIntegrator 控制闸门时间戳（`last_data_ts`）；其余 role（battery/hvac/fire/meter_batt）更新**不推进**闸门（只落库/事件 + 各自逐源时间戳）——避免总表掉线而 BMS 活性时，整体 5s 闸门被活性站掩盖、策略以陈旧 phase 驱动。phase/SOC 逐源过期标记同构（04 §2.11.1）。

### 10.6 上行、事件与存储

每站采集结果 DataPackage → storage `telemetry`（device_id 维度，已有）批量落库；role=fire/hvac 状态变化与告警字 → storage `events`/`faults` 落库 + SSE（复用 startup SSE 推送）；`DeviceRegistry`/`MessageBus` trait 本次接线（承载 device 注册与事件路由）。

### 10.7 错误与故障隔离

站超时/CRC/地址错 → 该站 offline 计数 + `events` 告警 + role 数据置 stale；同口其它站照常；恢复探测后自动上线。确定性错误（非法地址/数据）不重试。口 open 失败 → 该口全部站 offline，启动告警不阻断（PCS 主链路不在本模块）。

### 10.8 文件结构与测试

> **依赖形态**：`mupc-southd` 以**静态库**依赖 rs485-plugin 的复用 API（串口、`ProtocolHandler`/`ModbusRTU` handler、`meter_regs` 解码），避免与 cdylib 插件加载产生双实例；`Rs485Device` 增加请求级 slave 参数化（现 `encode_request` 用固定 `device_addr`，签名按口内多从站需要扩展）。
> **方向控制确认（板端待核）**：现有 rs485-plugin DE/RE GPIO 源自外接 USB-485 适配器假设；BECG 板载隔离 485 若为自动方向收发器则无需 DE/RE（配置缺省关闭），若仍需方向控制须按板端实际 GPIO 提供；实现前以板端核对为准。

- **`mupc-southd`**（新）：`config`（stations 反序列化+validate）、`station.rs`（模型/role）、`scheduler.rs`（口级 task + 口内轮询）、`port_runtime.rs`、`mapper.rs`（role→DataPackage）。
- **core-bin**：移除硬编码总表/pv-load 采集 task 的 `set_latest_data` 段，改装配 southd；`south_stations` 配置段。
- **device-trait/rs485-plugin**：`Rs485Device` 请求级 slave 覆盖（同口多从站需要）、`DeviceRegistry` 接线。
- **strategy-engine**：SOC 源优先级小改（04 §2.11）。
- 测试：调度器多站并发/同口串行、站超时隔离、role→DataPackage 映射、配置解析/校验、SOC 优先级切换、总表回归（phase 链路与既有策略测试零破坏）。

### 10.9 验证状态

设计按 writing-plans 分批实施；master_meter 收敛以总表回归测试（策略 phase 输入等价）为闸门。设备点表（BMS/空调/消防）待厂方提供后填配置。

---

## 11. 站级南向设备语义点表集成（S3b-2）

### 11.1 背景与现状差距（Why）

**现状（代码事实核对，2026-09-21）**：`mupc-southd` 的 role 分发骨架已就位，但数据面只能表达"每块 2 寄存器 1 值"：

| 现状代码事实 | 位置 | 后果 |
|--------------|------|------|
| `Role` 无 `Pcs`；`Role::{MeterBatt,Hvac,Fire}` 在 mapper 返回 `empty_package()` | `config.rs:20` / `mapper.rs:199` | 三站只作"站在线"信号 |
| `RegFormat` 仅 `Float32`/`Int32Scaled`，**两者都固定占 2 寄存器**；`decode_regs` 对 `len < 2` 返回 0.0 | `meter_regs.rs:16,39` | 16 位点表（绝大多数）无法表达 |
| 只有 `scale`，无 `offset` / `byte_swap` / `word_order` | `config.rs:65` | 温度类（`raw−40`）与 PCS 字节序不可表达 |
| `StationBus` 只有 `read_holding`(FC03) / `read_input`(FC04) | `port_runtime.rs:35` | FC02 位块（BMS 288 位 + 空调 31 位）读不到 |
| `mapper::telemetry_points` 每块只取**前 2 寄存器解 1 标量**、metric = 块名 | `mapper.rs:207` | 单点块（`count:1`）**不产任何点** |
| 无逐点换算参数 | `mapper.rs` 全篇 | 同一读窗口内混排换算（0.1V 与 0.01A、`raw−40` 与纯计数）无法表达 |

**本轮目标**：把上表 6 项能力补齐（PRD G-1…G-5），并让 5 份厂方点表（§9.5 共 **618 点**：`battery` 345 + `pcs` 72 + `meter_batt` 40 + `fire` 127 + `hvac` 34）**逐点可配置、可解码、可校验、可消费**。G-6（寄存器内字节拆分）PRD 已裁定**本轮不做**，替代口径见 §11.10。

**G-1…G-6 的设计落点（一眼定位）**：

| 差距 | 设计落点 |
|------|----------|
| G-1（16 位格式） | §11.4.2 `RegFormat::{Uint16,Int16}` + `RegDecode::width()` |
| G-2（`offset`） | §11.4.1 块/点级 `offset`；§11.4.2 换算 `值 = raw×scale + offset` |
| G-3（FC02） | §11.4.1 `RegFunc::Discrete`；§11.4.5 `StationBus::read_discrete` + `unpack_bits` |
| G-4（单寄存器块 / 多值块 / 逐点换算） | §11.4.1 `points[]`；§11.4.3 `points::expand()`（校验与运行期**同一函数**） |
| G-5（字节序/字序） | §11.4.1 `byte_swap` / `word_order`；§11.4.2 解码顺序（先逐寄存器 swap，再按字序拼） |
| G-6（字节拆分） | **不做**；§11.10 整字采集 + 展示层拆解 |
| **消防事件（`fire → events`）**（PRD §9.6.1 明文要求，**不属 G-1…G-6**） | **§11.2.6 选型（F3）→ §11.4.7.1 统一事件模型（`SignalSpec` + `EdgeTracker`）→ §11.7.2 信号清单 → §11.10 与 G-6 的三层边界表** |

### 11.2 方案探索与选型决策（先探索，后设计）

#### 11.2.1 配置模型：块=事务 / 点=换算口径

| 路线 | 形态 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| **A1（选定）块内嵌 `points[]`** | `RegBlockConf` 原地扩字段：传输口径（`func/addr/count/byte_swap`）+ 缺省换算（`format/scale/offset`）+ 可选 `points[]` | 与 PRD §9.4.2.4 字段表**逐字对应**；向后兼容仅靠 `#[serde(default)]`；单一解析路径、单一校验器 | `RegBlockConf` 字段变多（**6 → 10**：既有 `name`/`addr`/`func`/`format`/`scale`/`count`，新增 `offset`/`byte_swap`/`points`/`read_slice`；原写"9 → 13"多算，已订正） | **选 A1** |
| A2 双形态枚举 | `RegsConf = Legacy(Block) \| Pointed(Block+points)`（untagged） | "老写法/新写法"概念分离清晰 | YAML 两套写法长期并存 → 文档歧义 + 校验分支翻倍；`untagged` 报错信息极差 | 否 |
| A3 双数组并行 | 保留 `regs` 旧语义 + 新增 `blocks` 段，旧站走 `regs`、新站走 `blocks` | 旧站零风险 | 同一语义两处配置（漂移）；`meter_grid` 站要同时兼容两段；违背 PRD"唯一落地规则" | 否 |

**不破坏既有 `meter_grid` 写法的机制（本设计的关键约束）**：新增字段**全部**带 `#[serde(default)]`，且**缺省值 = 既有行为**：`offset=0.0`（等价无偏移）、`byte_swap=false`、`points=[]`（空）、`read_slice=false`。因此既有 `- { name: p, addr: 0x1000, format: int32_scaled, scale: 0.01, count: 6 }` 一行的解析结果与改动前**逐字段相同**；`meter_grid` 的运行路径（mapper 按**块名** `p/q/pf/u/i/p_total` 查找 + `decode_phase_block`）**完全不经过** `points[]` 与新的逐点展开（§11.4.6），回归锚 = `tests/grid_convergence.rs` 的**全部断言**（`config.rs` 的既有用例中，**只有 meter_grid-only 系列与解析类**同属回归锚 —— 非 grid 站的用例含 9 处因 metric 更名而必须订正的期望值，见 §11.5.3.3/§11.5.3.4）。

> **一处刻意的行为变更（须登记）**：`telemetry_points` 对**未声明 `points` 的块**由"取前 2 寄存器 1 点、metric=块名"改为"**每个值槽 1 点、metric=`<块名>_<序号>`**"（PRD §9.4.2.1 第 4 条）。该函数**只被非 grid 站调用**（grid 走 `on_grid_package`），而当前生效配置中只有 `grid_meter` 一个站 → **线上零影响**；但任何"非 grid 站靠块名做 metric"的旧配置（`deploy` 里的注释占位）会改名，须随本轮一并对齐 §9.4.1（§11.7.4 迁移清单）。
>
> **该变更的测试级影响（上轮评审 P0-4 的要点，清单见 §11.5.3）**：**线上零影响 ≠ 测试零影响** —— 单元测试里大量以"非 grid 站 + 块名即 metric"构造期望值，metric 更名后这些**断言期望值必须订正**（属"断言订正"，不是回归）。因此 §11.2.1 的"回归锚"口径随之收窄为：**`tests/grid_convergence.rs` 的断言零改动**（grid 路径不经 `telemetry_points`），`config.rs`/`core_config.rs`/`scheduler.rs`/`mapper.rs` 的既有用例按 §11.5.3 的**分类清单**处理（fixture 订正 / 断言订正 / 不得改）。

#### 11.2.2 `RegFormat` 的扩展方式

| 路线 | 形态 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| **B1（选定）扩枚举 + 引入解码规格** | `RegFormat` 增 `Uint16/Int16` 并给 `reg_width()`；新增 `RegDecode{format,scale,offset,word_order,byte_swap}` 承载解码；`decode_regs` 保留为薄包装 | 宽度/字节序/字序**集中在一处**（"代码只提供通用解码原语"，PRD B-1 裁定）；既有调用点**零改动**；4 种格式用 `match` 足够 | `decode_regs` 与 `RegDecode::decode` 两个入口（须靠测试钉住等价） | **选 B1** |
| B2 trait 化 codec | `trait PointCodec { fn width(); fn decode() }` + 各格式实现 | 未来加格式不改 match | 4 个格式无扩展压力（YAGNI）；动态分派/trait object 徒增复杂度 | 否 |
| B3 双枚举并存 | 块级仍 `RegFormat`，点级新 `PointFormat` | 不动既有类型 | 双枚举的转换/校验/文档三处重复，`format` 一个概念两个名字 | 否 |

宽度口径**单点定义**：`RegFormat::reg_width() -> usize`（`Uint16`/`Int16` → 1，`Float32`/`Int32Scaled` → 2）。PRD 的"32 位点对齐/占 2 寄存器"等规则全部引用该函数，不再散落魔数。

#### 11.2.3 FC02 离散输入的接入点

| 路线 | 形态 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| **C1（选定）`read_discrete → Vec<bool>`** | bus 层完成"响应字节 → 位向量"解包（`unpack_bits` 纯函数在 rs485-plugin） | 语义最清晰（长度 = 位数）；解包公式只有一份；MockBus canned 注入即"位向量"，AC-3 逐位比对**直接可测** | `StationBus` 增 1 个方法（MockBus/未来实现都要补） | **选 C1** |
| C2 bus 返回原始字节 | `read_discrete → Vec<u8>`，解包在 southd | 帧层最小改动 | 帧格式知识泄漏到 southd（MockBus 也要造字节）；解包公式与 §9.7.4 的对应关系跨 crate 断裂 | 否 |
| C3 复用 `Vec<u16>` 签名 | 把位图塞进 u16 数组，复用现有 `Result<Vec<u16>>` | diff 最小 | 位数非 16 倍数时尾部含无关位（AC-3 明确要求"末字节高 1 位不得污染前 31 位"）→ 需另一套文档化的掩码约定，最易错 | 否 |

#### 11.2.4 位块点的落库策略（PRD §9.8.1 末条要求设计定口径）

| 路线 | 形态 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| D1 全量落库（现状口径） | 每轮 319 个位点全落 telemetry | 实现最简、时序最完整 | BMS 位块 **288 行/s ≈ 2490 万行/天**；`storage` 逐行 `INSERT ... VALUES`（事务批内）→ BECG-3568 上写入压力与 `retention` 清理量级均不可接受；而稳态下 99.9% 的行是重复的 0 | 否 |
| **D2（选定）变化沿落库 + 告警上升沿事件** | 位点**仅在与上一轮不同时**落 telemetry；`alarm` 类的 0→1 跳变另产**事件**；模拟量点照旧全量落库 | 稳态写量≈0；跳变有记录、可回溯；与 PRD §9.6.1"告警位→events/SSE"一致；**内存现势值**仍每轮可得（供即时展示） | 需每站一份位图记忆（288 位 ≈ 36 字节/块，可忽略）；"某位长时间未变"时最后一条 telemetry 时间戳陈旧（须按 §11.7.3 展示口径标 stale） | **选 D2** |
| D3 位块不入 telemetry，只产事件 | 位点只走 events | 写量最小 | 丢失"位现势值"，无法回答"此刻该位是 0 还是 1"（只能靠事件重建）；AC-6 ① 的点产出与落库被割裂 | 否 |

> **落库口径与 AC-6 的分工**：`mapper::telemetry_points` 仍**返回全部点**（含 288 位，AC-6 ① 在 mapper 层断言"点产出"）；**变化沿过滤发生在 scheduler**（§11.4.7），即"点产出"与"落库节流"是两层，互不混淆。
>
> **保留策略**：沿用 `storage` 既有 `telemetry_retention_days` / `event_retention_days`（`RetentionManager`），本轮**不新增**清理逻辑；模拟量点新增约 299 行/轮（618 − 319 位点 = 299），5 站合计 ≈ 300 行/s，与既有 `meter_grid` 量级同阶，retention 天数须在投运前按盘容量复核（§11.12 待决项）。

#### 11.2.5 Q-1（BMS 主从方向）：口径裁定与可切换设计

**这是 PRD 标为【设计阶段阻塞】的项，本节给出显式处理（详见 §11.9）**。三条路线的比较：

| 路线 | 形态 | 是否可行 | 结论 |
|------|------|----------|------|
| E1 按 PRD §2.2 单一口径实现（MUPC 主动轮询） | 无切换代码 | 可行 | 采纳为**基线**，但不单独使用（见 E3） |
| E2 实现"主站轮询 + 被动接收"双模式可配置切换 | southd 增从站模式 | **不可行（伪选项）** —— Modbus 从机语义下，"BMS 做主机"意味着 BMS 来**读 MUPC 的寄存器**，链路上**不存在** BMS 数据的下行通道；MUPC 拿不到数据不是"southd 缺一个模式"，而是**该采集链不存在** | 否（诚实说明，见 §11.9.2） |
| **E3（选定）基线轮询 + 消费侧与发起方解耦 + 第二方案影响面评估** | 按 E1 实现；同时把 SOC 消费契约锚定在"**点名 `soc`**"而非"battery 站轮询"（§11.9.3），并给出相反口径下的替代链路（BMS LAN/Modbus-TCP 客户端）与切换判据/代价/新鲜度影响 | 可行且满足 PRD ③"给两套可切换方案"的实质 | **选 E3** |

选 E3 的关键权衡：E2 看起来"更灵活"，但它把"链路是否存在"错当成"配置项"——真正能切换的不是初动方向，而是**接入路径**（RTU 主站轮询 / TCP 客户端）。E3 把**唯一会随口径变化的接缝**（SOC 供给源）从调度器里剥离出来，使两种口径下的消费侧（AiIntegrator/策略/telemetry）**零改动**。

#### 11.2.6 消防事件（`fire → events`）的落点

**问题**：PRD §9.6.1 要求 `fire` 的系统状态位/探测器/火警状态进 events，但消防**没有任何 `discrete` 块**（全部状态量在保持寄存器整字内）⇒ §11.4.7 原事件模型（只认离散位）**一条都产不出来**。

| 路线 | 形态 | 优点 | 缺点 | 结论 |
|------|------|------|------|------|
| F1 把消防状态位改配为 `func: discrete` 块 | 用 FC02 读 addr 4/6/7/8/9/12 | 直接复用既有 `BitClass::Alarm` 通路 | **物理上不可行**：这些量在**保持寄存器**（FC03）里，FC02 读的是另一套离散输入空间（消防协议亦未提供对应位区） | 否 |
| F2 在展示层/Web 侧检测 | 由前端或 core-bin 旁路读 telemetry 后判跃迁 | southd 零改动 | **落点错**：事件必须进 `storage.events` + `AlertFeed` + SSE，且要与 §10.4 联锁融合 —— 该通道只由 southd 持有；展示层检测会**错过实时性**，也让 `fire` 行的"events"名不副实 | 否 |
| F2′ 不加语义，把整字当 16 个位逐位建信号 | 用离散位的 `Bit` 机制套整字 | 复用现有 `Bit` 通路、零新概念 | **语义噪声**：20+ 寄存器的每个位都成信号（含保留位、备电电量的 8 位、预留的 bit2）⇒ 事件量爆炸且**违背 PRD §9.7.6"预留位不产事件"**；还会把"0 = 离线"的极性位误判成"1 = 异常" | 否 |
| **F3（选定）信号层：`SignalSpec` + 统一 `EdgeTracker`** | `point_table` 登记**逐信号**的 `SignalPick`（`WordBit{mask}` / `WordEnum{active}`），**只登记产事件的信号**；scheduler 用**同一个** `EdgeTracker` 同时处理离散位与字级信号 | ① 语义显式且可逐条对表 PRD §9.5.4；② 与既有位点**共用**跃迁记忆与事件产出路径（"统一"而非"并存"）；③ 天然支持"预留位不产事件"（不入 mask）、"极性反转位不猜"（不入表）、"未登记 = 只落 telemetry"；④ 消防取**双向**事件，正好补上 §10.4"恢复需双方复位"缺的输入 | 需新增一个内部枚举与一张信号表（规模：系统状态 ~6 + 火警 4 + 3 组触发 + 探测器 2×n） | **选 F3**，落点见 §11.4.7.1 |

> **与 G-6 的关系**：F3 **不**触碰字节拆解（只用 `mask`/`active`），故与 §11.10 的"整字采集 + 展示层拆解"并存不冲突，边界见 §11.10 的三层表。

### 11.3 模块划分与文件结构

**依赖方向不变**（§10.8/§10 依赖声明）：`mupc-southd` 仍**不依赖** strategy-engine / storage；回传由 core-bin 的 `SouthSink` 回调注入；`mupc-southd` 仍以静态库形态依赖 `rs485-plugin` 与 `mupc-data-processing`。

| 文件 | 动作 | 职责 | 扩展点来源 |
|------|------|------|------------|
| `crates/data-processing/src/meter_regs.rs` | 修改 | `RegFormat` 增 2 变体 + `reg_width()`；新增 `WordOrder`/`RegDecode`；`decode_regs` 保留为薄包装 | §10 已定"复用 meter_regs 解码" |
| `crates/rs485-plugin/src/device.rs` | 修改 | `read_discrete_inputs_from` + `parse_bits_response` + `unpack_bits`（纯函数） | §10.8 "Rs485Device 按口内多从站需要扩展" |
| `crates/rs485-plugin/src/lib.rs` | 修改 | 导出 `unpack_bits` / `read_discrete_inputs_from` 相关符号（+ `pub use device_trait::Parity`，供 southd 具名使用） | 同上 |
| `crates/mupc-southd/src/config.rs` | 修改 | `Role::Pcs`、`RegFunc::Discrete`、`StationParity`、`RegBlockConf` 新字段、`PointConf`；`validate()` 扩展（§11.5） | §10.3 stations 配置段 |
| `crates/mupc-southd/src/points.rs` | **新增** | 块 → 点展开（`expand`/`footprint`）、命名规则、位解包适配、**字级信号的活跃判据（`SignalSpec` → `SignalState`）** | §10 未涉及（新能力）；信号表见 §11.4.7.1 |
| `crates/mupc-southd/src/point_table.rs` | **新增** | 点表登记注册表（§11.4.4）：`(role, addr) → {format, scale, offset, sym_src, kind, label, signals}`（探测器区为 **6 条组内模板**） | PRD §9.4.3 要求的"校验器内点表登记值常量表"；`signals` 承载消防事件源 |
| `crates/mupc-southd/src/mapper.rs` | 修改 | `telemetry_points` 重构（逐点）、`Battery` 按**点名** `soc` 查找、`Role::Pcs` 臂、SOC 域检查、消防登记数比对 | §10.5 role 映射 |
| `crates/mupc-southd/src/port_runtime.rs` | 修改 | `StationBus::read_discrete`；`Rs485PortBus` 实现（`bus_lock` + `spawn_blocking` 同构）；`MockBus` 位注入；**`parity` 透传（实现 = `bus_config` 纯函数，由 `open` 调用；已为裁定项 ① 的落地）** | §10.2/§10.7 口级总线 |
| `crates/mupc-southd/src/scheduler.rs` | 修改 | `RegFunc::Discrete` 分发、位点变化沿过滤、`role_priority(Pcs)=1`、SOC 越界/消防登记数事件 | §10.2 口调度预算 |
| `crates/mupc-southd/src/station.rs` | 修改（可选） | 若把位变化沿记忆放 `Station`（本设计放 `PortRunner`，故**不改**） | — |
| `crates/mupc-core-bin/src/startup.rs` | 修改（小） | `SouthSink`：`is_event=true` 且非 offline/online 时把 **value 写进事件 message**（SOC 越界原始值落证）；事件 message 可选查 `point_table::label` 补中文名 | §10.6 事件落库 |
| `crates/mupc-core-bin/src/core_config.rs` | 修改（测试数据） | 既有 fixture 订正（§11.5.3） | §10.3 跨段校验 |
| `mupc/deploy/config/mupc_core_config.yaml`(+`.production`) | 修改 | `south_stations` 段换为 §9.4.1 的 6 站（PCS 站默认**保持注释**，待 Q-15 裁定） | §10.3 |

**不新增 crate、不新增独立设计文档**（项目 CLAUDE.md 文档原则）。

> **设备-端口映射的硬件依据（不新开条目）**：本章 6 站的"站 ↔ 串口"对应关系（`battery → ttyS2` / `hvac → ttyS3` / `meter_grid → ttyS4` / `meter_batt → ttyS5` / `fire → ttyS6` / `pcs` 只读站 `→ ttyS7` 建议值）**与 §10.1 的 RS485 分配同源**，取自两份硬件资料：① **BECG-3568 接线拓扑图** —— `hw/微信图片_20260908170935_64_1061.png`（订正件 `…_修正.png`：`RS485-1 → PCS`（另有 CAN 直连）/ `RS485-2 → BMS` / `RS485-3 → 空调` / `RS485-4 → 关口表` / `RS485-5 → 储能表` / `RS485-6 → 消防`）；② **BECG-3568 BOX 感知与控制主机规格书**（`hw/BECG-3568 BOX感知与控制主机规格书(20250818) .docx`：8 路隔离 RS485 = COM1–8 ↔ `ttyS0`、`ttyS2`–`ttyS8`，**无 `ttyS1`**）。**该拓扑图同时是"消防/空调直连 EMS、不挂 BMS 485 总线"的硬件依据**（RC-11 撤销，见 §11.11.3）。**端口/从站号仍为待现场校准项**（PRD §9.4.1 末注），**不得硬编码到代码**。

### 11.4 数据结构与接口定义

#### 11.4.1 配置结构（`mupc-southd::config`）

```rust
/// 站级校验位（YAML: none 缺省 / even / odd）——PRD §9.4.1 站级 `parity`
/// （**D-1 已裁定按 ①：新增站级 `parity` 字段、空调站配 `even`、同口一致性校验同 `baud_rate`**，
/// 2026-09-22；**字段自该日起生效**，见 §11.4.5 的 `parity` 透传段与 §11.11.3 RC-6）
/// **取证点**：`StationParity{None(缺省), Even, Odd}` 与站级 `parity` 字段已定义（`config.rs:29-39` / `:75`）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StationParity { #[default] None, Even, Odd }

/// 站类型角色（新增 Pcs；YAML 值 `pcs`）
pub enum Role { MeterGrid, MeterBatt, Battery, Hvac, Fire, Pcs }

/// 读功能码（新增 Discrete = FC02）
pub enum RegFunc { Holding, Input, Discrete }

// ── 32 位值的字序（YAML: hi_lo 缺省 / lo_hi）：**单一定义** ──
// 类型**只在 `mupc-data-processing::meter_regs` 定义一次**（§11.4.2 —— 它与 `RegDecode` 同居，
// 解码原语与它的字序参数不可分离；该 crate 已依赖 serde，`RegFormat` 即同一先例）。
// 本 crate **复用**同一类型、不再重定义（此前在 §11.4.1/§11.4.2 各定义一次 = §11.2.2 B3 的双枚举缺陷）：
pub use mupc_data_processing::meter_regs::WordOrder;

pub struct StationConf {
    // 既有字段全部保留（id/role/port/protocol/slave/baud_rate/interval_ms/regs）
    #[serde(default)] pub parity: StationParity,          // 新增
}

pub struct RegBlockConf {
    pub name: String,
    pub addr: u16,
    #[serde(default = "default_reg_func")]    pub func: RegFunc,
    #[serde(default = "default_reg_format")]  pub format: RegFormat,
    #[serde(default)]                         pub scale: f64,
    #[serde(default = "default_reg_count")]   pub count: u16,
    // ── S3b-2 新增：全部 #[serde(default)]，缺省 = 既有行为 ──
    #[serde(default, skip_serializing_if = "is_zero_f64")] pub offset: f64,        // G-2
    #[serde(default, skip_serializing_if = "is_false")]    pub byte_swap: bool,    // G-5
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub points: Vec<PointConf>, // G-4
    #[serde(default, skip_serializing_if = "is_false")]    pub read_slice: bool,   // PRD §9.4.2.4 块级字段（PRD v1.7 补登），语义见 §11.5.2
}

/// 点级换算口径（PRD §9.4.2.4 点级字段表）
pub struct PointConf {
    pub at: u16,                                   // 块内寄存器/位偏移 + 1（32 位点填低地址寄存器序号）
    #[serde(default = "default_point_count")] pub count: u16,     // 缺省 1
    #[serde(default, skip_serializing_if = "Option::is_none")] pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub format: Option<RegFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub offset: Option<f64>,
    #[serde(default, skip_serializing_if = "is_default_word_order")] pub word_order: WordOrder,
}
```

**为什么点级用 `Option<T>` 而不是"缺省值语义"**：`scale`/`offset` 的"未声明"与"显式 0"是**两种不同事实**——前者须继承块级，后者是配置错误（`scale == 0` 拒，PRD §9.4.3）。用 `Option` 让"继承"与"显式 0"在类型上可区分，校验器才能既拒 `scale: 0` 又不误伤"继承块级 `scale: 0.01`"的点。

**`read_slice`（PRD 已正式补登，本设计只做落地）**：语义 = "本块是按**设备单次读上限**主动分片的结果，**仅**豁免 §11.5 的块落地极大性检查（第 15 条）"；缺省 `false`。字段定义、适用场景与四条"不得用于逃避合并"的约束以 **PRD §9.4.2.4 块级字段表**为准（PRD v1.7 补登），落地口径见 §11.5.2。

**兼容性论证（"既有 `meter_grid` 写法不得破坏"）**：

| 既有写法 | 解析结果 | 运行行为 |
|----------|----------|----------|
| `{ name: p, addr: 0x1000, format: int32_scaled, scale: 0.01, count: 6 }` | 与改动前逐字段相同（新字段取缺省） | 同（mapper 按块名找 `p` → `decode_phase_block` → `decode_regs`） |
| 站级无 `parity` | `StationParity::None` | `Rs485PortBus::open` 传 `Parity::None`（与 `Config::default()` 一致） |
| 无 `points` 的块 | `points = []` | **非 grid 站的 metric 命名变化**（§11.2.1 已登记；grid 不受影响） |

#### 11.4.2 解码原语（`mupc-data-processing::meter_regs`）

```rust
pub enum RegFormat { Float32, Int32Scaled, Uint16, Int16 }   // 新增 2 变体

impl RegFormat {
    /// 单值占用的寄存器数（16 位 = 1，32 位 = 2）——"宽度"的唯一定义
    pub fn reg_width(self) -> usize { match self { Uint16 | Int16 => 1, Float32 | Int32Scaled => 2 } }
}

/// 32 位值的字序 —— **全项目唯一一处定义**：`hi_lo` = 高字在低地址（缺省，
/// 与既有 `regs_to_u32_be` 一致）/ `lo_hi`（PCS 32 位电量）。serde 形态与同文件的
/// `RegFormat` 完全同构（`rename_all = "snake_case"` + 全链 `Serialize` 供配置回写）；
/// `mupc-southd::config` 以 `pub use` 复用（§11.4.1），**不得**在别处再定义一份。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WordOrder { #[default] HiLo, LoHi }

/// 解码规格：格式 + 换算 + 字节序/字序（"通用解码原语"的载体）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegDecode {
    pub format: RegFormat,
    pub scale: f64,        // Float32 忽略（沿用既有语义：float32 不乘 scale）
    pub offset: f64,       // Float32 忽略（offset 是"整数零点平移"的语义，浮点原值无零点平移概念）
    pub word_order: WordOrder,   // 仅 32 位格式生效
    pub byte_swap: bool,         // 仅按寄存器生效（逐寄存器 u16::swap_bytes）
}
impl RegDecode {
    pub fn width(&self) -> usize { self.format.reg_width() }
    /// 解码 1 个值；寄存器数不足 → 0.0；非有限值 → 0.0（沿用既有守卫）
    pub fn decode(&self, regs: &[u16]) -> f64;
}
/// 既有入口保留（薄包装，语义不变：hi_lo + 无 swap + offset 0）
pub fn decode_regs(r: &[u16], format: RegFormat, scale: f64) -> f64;
```

**解码算法（唯一、有序）**：

```
1) 长度守卫：regs.len() < width() → 0.0
2) 字节序还原：对参与本次解码的每个寄存器做 swap_bytes()（byte_swap=true 时）
3) 位模式组装：
   - width == 1：v = regs[0]
     - Uint16 → raw_u = v as u64
     - Int16  → raw_i = v as i16 as i64
   - width == 2：依 word_order
     - HiLo（缺省）：u32 = (regs[0] << 16) | regs[1]        ← 与既有 regs_to_u32_be 逐位等价
     - LoHi         ：u32 = (regs[1] << 16) | regs[0]
4) 物理解算：
   - Float32      → f32::from_bits(u32) as f64（忽略 scale/offset）
   - Int32Scaled  → (u32 as i32) as f64 * scale + offset
   - Uint16/Int16 → raw as f64 * scale + offset
5) 非有限守卫 → 0.0
```

> **算法顺序的判据（PCS 场景）**：AC-2 的期望值**唯一地**钉住了顺序 —— `reg[1042]=0x6400, reg[1043]=0x0100`，`byte_swap=true`、`word_order=lo_hi` ⇒ 先逐寄存器 swap 得 `[0x0064, 0x0001]`，再按 `lo_hi` 拼得 `0x00010064 = 65636`，`×0.1 = 6563.6 kWh`（若**先拼后 swap** 会得不同值；若误用 `hi_lo` 得 655360.1，**必须不一致**）。故"先 swap 再拼字"不是风格选择，而是被验收用例固定的语义。

#### 11.4.3 点展开与命名（`mupc-southd::points`，新模块）

```rust
/// 一个可产出的遥测点（16 位/32 位标量 或 1 个位）
pub struct PointSpec {
    pub metric: String,      // 点位名（PRD §9.4.2.2 唯一命名规则）
    pub kind: PointKind,     // Scalar { offset: u16, decode: RegDecode } | Bit { offset: u16 }
}
pub enum PointKind { Scalar { offset: u16, decode: RegDecode }, Bit { offset: u16 } }

/// 块 → 完整点清单。**校验期与运行期调用同一函数**（防"校验通过但运行期展开不同"）
pub fn expand(block: &RegBlockConf) -> Result<Vec<PointSpec>, String>;

/// 某块点清单的寄存器占位（校验空洞/极大性用）：返回 [(起始偏移, 长度)]
pub fn footprint(block: &RegBlockConf) -> Result<Vec<(u16, u16)>, String>;

/// **字级信号**：把某块的整字读数按 `point_table::SignalSpec` 求值为"活跃/非活跃"（§11.4.7.1）。
/// 返回 (信号键全名 `<点名>@<key>`, 是否活跃)；`scheduler` 用它喂 `EdgeTracker`。
/// 注意：**只读整字、不改写遥测值**；无信号的块 → 空 Vec。
pub fn signals_of_block(role: Role, block: &RegBlockConf, regs: &[u16]) -> Vec<(String, bool)>;
```

**展开规则（与 PRD §9.4.2.1 第 4 条一一对应）**：

| 情形 | 产出 |
|------|------|
| `func: discrete` + 无 `points` | `count` 位各 1 点，`metric = <块名>_<k+1>`（k = 位偏移，0 起） |
| `func: discrete` + 有 `points` | 仅列出的位（`at` 与点级 `count` 展开），同上命名 |
| 标量块 + 无 `points` | 窗口内**每个值槽** 1 点：按 `format.reg_width()` 步进；`metric = <块名>_<序号>`，序号 = **低地址寄存器的块内偏移 + 1** |
| 标量块 + 有 `points` | 仅列出的点；点级 `format/scale/offset/word_order` 覆盖块级缺省（`Option::None` = 继承）；点级 `count: N` 在 16 位格式下产出连续 N 点（序号 `at, at+1, …`），**32 位格式必须 `count = 1`** |
| 点级 `name` | 覆盖位置命名；**仅允许 `count == 1`**（多值点无"命名序列"定义，PRD 未定义 → 取保守口径**配置期拒**，见 §11.5.2 设计补充） |

**护栏（落规则 19）——无 `points` 的块 `count` 必须是宽度的整数倍**：无 `points` 的标量块按 `format.reg_width()` 步进产点，若 `count % width != 0`（例如 32 位格式 `count: 3`），步进的最后一格**装不下一个完整值** ⇒ 该尾槽**静默不产点**（`3 / 2 = 1` 点，而非 2 点），配置者却看不出少了一个点。这条**不产生运行时错误、只产生"少产点"的静默失真**，正是 PRD §9.4.3 要防的形态，故在**配置期直接拒**（规则 19，§11.5.1）：`func != Discrete && points.is_empty() && count % format.reg_width() != 0 → Err`。`discrete` 块（`count` = 位数，无"宽度"概念）与**声明了 `points` 的块**（`count` 由规则 11 的"首尾锚定"约束，见 §11.5.1 #11）均不适用。

**位解包（`unpack_bits`，rs485-plugin 提供纯函数）**：

```rust
/// Modbus FC02 响应字节 → 位向量（PRD §9.7.4 公式，唯一）
/// bit k = bytes[k/8] 的第 (k % 8) 位（bit0 = LSB）；返回长度 = count
pub fn unpack_bits(bytes: &[u8], count: u16) -> Vec<bool>;
```
- 响应字节数 = `ceil(count / 8)`；`count` 非 8 倍数时**末字节高位为无关位，不得影响前 `count` 位**（AC-3 空调 31 位的断言点）。
- 示例（PRD §9.7.4 引用 BMS 协议原文）：连续 16 位 `1,1,0,1,1,1,0,0,…` → 首字节 `00111011B = 0x3B`；随后 `1,1,0,1,1,1,0,1` → `10111011B = 0xBB`。

#### 11.4.4 点表登记注册表（`mupc-southd::point_table`，新模块）

**为什么需要它**：PRD §9.4.3 明确要求"符号性声明一致性"的第 ② 条的可判定性**以设计落成"点表登记值常量表"为前提**（**查表键 = `(role, space, addr)`**（见下"地址空间维度"段），行内容 → `{format, scale, offset, 来源}`，由设计阶段按 §9.5 逐点转写）。

```rust
/// 符号性来源（PRD §9.5 前言的三分类）
pub enum SymSrc { Vendor,       // 厂方逐点明写（PCS、空调、ADL400 的 4 字节功率类/PF）
                  VendorTypo,   // 厂方标注 + 推断订正（BMS 的 UNIT→UINT）
                  Engineer }    // 工程判断（消防、ADL400 2 字节点）

/// 位点分类（决定是否产出告警事件，见 §11.7.2）
pub enum BitClass { Alarm, State, Reserved }

pub struct PointReg {
    pub role: Role,
    pub addr: u16,               // 绝对寄存器地址；discrete 块为**位地址**
    pub kind: RegPointKind,      // Scalar(RegFormat) | Bit(BitClass)
    pub scale: f64,
    pub offset: f64,
    pub sym_src: Option<SymSrc>, // Bit 行 / 无偏移点可为 None
    pub label: &'static str,     // 中文名（事件/展示/RC-1 核对清单用）
    pub signals: &'static [SignalSpec],  // 该格子内的**字级信号**（消防专用，见 §11.4.7）
}

/// 字级信号：一个整字（或一位）上的**可跃迁量**，**入表即产事件**。仅消防站使用（其状态位/枚举
/// 全在保持寄存器整字内，无 `discrete` 块）—— 详见 §11.4.7.1 的事件模型与 §11.7.2 的信号清单。
///
/// **口径（避免"登记了却不产事件"的歧义）**：`signals` **只登记产事件的信号**；
/// 未登记的位/枚举**一律只落 telemetry**（等价于离散位块的 `State`/`Reserved` 语义）。
/// 故**不设 `class` 字段**（本轮无"登记为 State"的形态；将来若需要，再以新变体扩展）。
pub struct SignalSpec {
    pub key: &'static str,       // 信号键（事件名后缀），表内唯一
    pub pick: SignalPick,        // 活跃判据
}
pub enum SignalPick {
    /// 位图：整字 & `mask` ≠ 0 视为活跃。`mask` 应为**单一或一组同类位**，
    /// 不得跨位图混装（每 bit 一个独立 key，便于事件定位）。
    /// **极性反转位不建信号**（如消防探测器 bit15「1 = 在线 / 0 = 离线」）—— 本设计不为其造判据。
    WordBit { mask: u16 },
    /// 枚举：整字值 ∈ `active` 视为活跃（活跃值之间跃迁亦产事件：如 1 一级报警 → 2 二级火警）。
    WordEnum { active: &'static [(u16, &'static str)] },
}

/// **地址空间维度**：位空间与寄存器空间**各自从 0 编址、互不相干**
///（PRD §9.4.2.1 第 6 条 / §9.4.3「区间与重叠」按功能码空间分别计算）。故查表键是
/// `(role, space, addr)` 而**不是** `(role, addr)`（理由见下"地址空间维度"段）。
pub enum AddrSpace {
    Reg,   // 寄存器空间（FC03 保持 / FC04 输入）
    Bit,   // 位空间（FC02 离散输入）
}

pub const POINT_REGS: &[PointReg] = &[ /* 展开后 618 行（n=20），由 §9.5 逐点转写 */ ];

/// 按 `(role, space, addr)` 查登记行（**规范形态**；查不到 → `None` ⇒ 调用方**放行**，见下"查不到行"口径）。
pub fn lookup_in(role: Role, space: AddrSpace, addr: u16) -> Option<&'static PointReg>;

/// **寄存器空间**查登记行 = `lookup_in(role, Reg, addr)` —— **兼容包装（语义不变）**：
/// 规则 6 的强制校验只作用于**标量点**，其地址必属寄存器空间，故该签名足够。
pub fn lookup(role: Role, addr: u16) -> Option<&'static PointReg>;

/// **位空间**查登记行 = `lookup_in(role, Bit, addr)`（`discrete` 块的 `addr` 是**位地址**）。
pub fn lookup_bit(role: Role, addr: u16) -> Option<&'static PointReg>;

pub fn label(role: Role, metric: &str) -> Option<&'static str>;

/// 某 `(role, addr)` 上的信号（无信号 → 空切片）。**不设 `space` 参数**：字级信号仅消防使用，
/// 而消防站**没有** `discrete` 块（状态量全在保持寄存器整字内）⇒ 只查寄存器空间。
pub fn signals_of(role: Role, addr: u16) -> &'static [SignalSpec];
```

**地址空间维度：为什么查表键必须带 `space`（实现期发现的设计缺陷，实现修正正确且向后兼容）**

| 项 | 口径 |
|----|------|
| **两个取值** | `AddrSpace::Reg` = **寄存器空间**（FC03 保持 / FC04 输入）；`AddrSpace::Bit` = **位空间**（FC02 离散输入）。两者**各自从 0 编址、互不相干** —— `(role, Reg, 0)` 与 `(role, Bit, 0)` 是**两行不同的登记**，不是同一行的两种解释 |
| **为什么必须带（实证）** | **`hvac`（风冷空调机组）站在同一批地址上真实共存两个空间的行**：FC04 温湿度寄存器 **0 / 2 / 3**（柜内测量温度 / 内盘管测量温度 / 柜内测量湿度）与 FC02 位块 `hvac_di` 的位 **0 / 2 / 3**（位 0 = 内风机、位 2 = 制冷状态、位 3 = 加热状态）。在单一 `(role, addr)` 键下，**后登记的行遮蔽先登记的行**（或反之），消歧只能依赖**数组顺序** —— **顺序一改、查表语义就变**，属"靠顺序维持正确性"的脆弱形态（本设计一贯拒绝；PRD §9.4.3 要防的正是这类**静默失真**）。带上 `space` 后两行各有唯一键，**结果与登记顺序无关** |
| **`label()` 同样受害** | `label(Role::Hvac, "hvac_in_1")`（= 寄存器 0 → "柜内测量温度 ℃"）与 `label(Role::Hvac, "hvac_di_1")`（= 位 0 → "内风机"）**必须给出两个不同的中文名**。无 `space` 时二者会解析到**同一行** ⇒ 事件 `message` 与 RC-1 的机读核对清单**双双出错**（这正是"能过校验但语义已错"的形态） |
| **与规则 14 的对应（口径自洽性）** | §11.5.1 规则 14（区间与重叠）按**功能码空间**（holding / input / discrete 三套）分别判半开区间重叠（PCS 的 3 区/4 区同址不同 `func` 由此天然放行）。**查表键的 `space` 维度就是规则 14 在点表侧的同一件事**：规则 14 宣告"两个空间的重叠互不相干"，则点表必须**在键里就能区分**这两个空间，否则等于"先把两个空间混成一个、再由规则 14 事后拆分" —— 同一文档内两处口径相左。故 `space` 不是实现细节，而是规则 14 的**前置条件** |
| **`holding` / `input` 为何合成 `Reg`** | 规则 14 的**判定**单位是功能码空间（三套），但**换算**口径（`RegFormat` / `scale` / `offset`）对 FC03 与 FC04 **完全相同**，故登记表只需**寄存器 vs 位**二分；`func`（FC03/FC04）由**配置块**声明、不参与查表。**边界**：若将来某 role 同时以 holding 与 input 在**重叠地址**上登记点，则该二分不足（需 `space` 细分为三）——§9.4.1 六站**均不属**此形态（`bms`/`pcs` 全 `input`、`meter_batt`/`fire`/`grid_meter` 全 `holding`、`hvac` = `input` + `discrete`），故**本轮该边界不可达**；登记为**残余边界（就地登记于本节，不新开风险条目）**：出现时**扩展 `AddrSpace` 枚举**（而非改语义），并同步规则 14 的判定粒度 |
| **规则 6 是否受影响** | **不受**：规则 6 的强制校验只作用于**标量点**（`format ∈ {uint16,int16} && offset ≠ 0`），其 `addr` 必属寄存器空间 ⇒ 其 `lookup(role, addr)` 即 **`Reg` 空间包装**，下文"覆盖范围与强制口径"的两条 `Err` 判据与 §11.5.1 #6 **一字不改**（`lookup` 的调用点亦不变，向后兼容） |

> **探测器区的登记形态（防"618 行"与可变长度区矛盾）**：`fire_det` 区的行数**随现场登记数变化**（n=20 时 114 行；n=100 时 594 行），无法逐地址静态登记。故该区在 `POINT_REGS` 中登记为 **6 条"组内语义模板"**（`+0 地址` / `+1 状态` / `+2 数据 1` / `+3 CO` / `+4 VOC` / `+5 H2`），运行期按 `(addr − 17) % 6` 取模板（**仅在 `space = Reg` 时生效** —— 探测器区是保持寄存器区，位空间无模板，见上"地址空间维度"段）；**信号挂在前两条模板上**（`+1 状态` 的 bit12/bit14）。因此 §11.4.4 末的断言口径改为：**"按 n=20 的参考配置展开后 == 618 行"**（而不是"静态数组 618 行"）。

**覆盖范围与强制口径（本设计的取舍，须评审确认）**：

| 用途 | 口径 |
|------|------|
| 覆盖 | **全 618 点**（按 n=20 参考配置展开；探测器区为模板 + 运行期展开）逐点转写（PRD §9.4.3 明文要求）——它同时是 RC-1 的**机读核对清单**与事件/展示的中文名来源 |
| **强制**（`Err`） | ① `format ∈ {uint16,int16}` 且 `offset ≠ 0`，而 `lookup(role, addr)` **命中一行**、但该行 `sym_src` 为空 → 拒<br>② `lookup` 命中且 `offset` 与登记值不等（**含"漏配 → 缺省 0 ≠ −40"**）→ 拒（PRD ② 明文，比较字段 = `offset`） |
| **不强制**（测试期） | `format` / `scale` 与登记值不一致**不拒** —— 这是刻意的：Q-4（PCS 地址基准）、Q-16（4000/4005 的 16/32 位）、Q-20（116/186 改 `int16`）三项**现场裁定会合法改变** `addr`/`format`/`scale`，若强校验会把现场裁定后的正确配置**拒在启动期**。这些字段的一致性改由 `tests/point_table_vs_reference_config.rs`（§11.4.4 末）与 RC-1 逐点比对保证 |
| **查不到行** | **不拒**（裁定：不采用"① 无行 → 拒"，只保留"命中而行内 `sym_src` 为空 → 拒"） |

> **为什么「查不到行 → 不拒」（P0-2 的裁定理由，必须写清）**：**现场 RC-3 会合法改 `addr` 基准** —— PCS 3 区首点到底是 0 基还是 1000 基**文档自相矛盾**（PRD §9.5.2 / Q-4），现场实测后**回写 `addr` 就是本次裁定的产物**；此时 `POINT_REGS` 按 1000 基登记的键**自然全部失配**。若"无行即拒"，则**合法的现场校准配置会把站拒在启动期**（而且是 PCS 站，恰好是 Q-4 唯一要校准的站）——这是"把不确定性误判为错误"。故 ① 「无行或 `sym_src` 空 → 拒」**删去"无行"半句**，只保留"**命中而行内 `sym_src` 为空** → 拒"（该形态是**表自身的转录错误**：一条 `offset ≠ 0` 的现行偏偏没登记来源），全表据此自洽（§11.5.1 #6 同步）。**代价与补偿**：查不到行时，`offset` 的**可追溯性由 ② 与 RC-1 逐点比对兜底**（登记为 §11.12.3 R-3）。

**漂移防护（表 vs 配置的双向核对，实现期必做）**：`tests/fixtures/south_stations_s3b2.yaml`（= PRD §9.4.1 的 6 站参考配置）与 `POINT_REGS` 逐行交叉核对：凡表内 `offset ≠ 0` 的行，配置展开后必须命中同名同值；凡配置中 `offset ≠ 0` 的点，表内必须命中。测试同时断言**展开后行数 == 618**（= PRD §9.8.3 的点数；探测器区模板按 n=20 展开，见上）。

#### 11.4.5 总线与帧层扩展

```rust
#[async_trait]
pub trait StationBus: Send + Sync {
    async fn read_holding (&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError>;
    async fn read_input   (&self, slave: u8, addr: u16, count: u16) -> Result<Vec<u16>, BusError>;
    /// FC02 读离散输入：`count` = **位数**（非寄存器数）；返回长度 = count 的位向量
    async fn read_discrete(&self, slave: u8, addr: u16, count: u16) -> Result<Vec<bool>, BusError>;
}
```
- `Rs485PortBus::read_discrete`：与既有两方法**同构**（先取 per-port `bus_lock` 强制口内串行 → `spawn_blocking` 内 `dev.read_discrete_inputs_from(slave, addr, count)`）。
- `rs485-plugin`：`read_discrete_inputs_from(slave, addr, count)` 复用 `build_read_frame(slave, 0x02, addr, count, crc)`；响应解析**不能**复用 `parse_regs_response`（它按 `byte_count / 2` 拆寄存器，**会丢掉非偶数字节的末字节**）→ 新增 `parse_bits_response(response, count) -> Result<Vec<bool>, Rs485Error>`，内部调 `unpack_bits`。
- `MockBus`：新增 `put_bits(slave, addr, Vec<bool>)` / `fail_bits_once(slave, addr)` / `bit_calls`（与 holding/input 三套独立键，FC03/04/02 是不同寄存器空间）。
- **`Rs485PortBus::open` 的 `parity` 透传（落点见 §11.3 表）**：现有实现只透传 `baud_rate`/`device_addr`，其余走 `rs485_plugin::config::Config::default()` ⇒ **校验位恒 `Parity::None`**，站级 `parity` 会被**静默忽略**（配置写了 `even` 却按 `none` 通信 ⇒ 空调站全站通信失败，且现象是"offline"而非"配置错"）。本轮须补一行映射：`parity: match conf.parity { StationParity::None => device_trait::Parity::None, Even => Parity::Even, Odd => Parity::Odd }`（`device_trait::Parity` 由 `rs485-plugin` 的 `pub use` 具名引入，避免在本 crate 再依赖 `device-trait`）；`timeout_ms`/`crc_mode`/8N1/DE-RE 仍取 `Config::default()`（**本轮不改**）。该透传是 **D-1 空调校验位裁定（RC-6）的代码侧前置** —— **裁定已于 2026-09-22 作出（按 ①：空调站配 `even`）**，故本条由"前置"转为"**已落地**"，且**只改配置即可生效、无需改代码**（T5 起成立）：实现 = `port_runtime.rs::bus_config`（`:161-171`，`StationParity::{None,Even,Odd}` → `rs485_plugin::Parity` **三值逐一对映**）；测试 = `port_runtime.rs::bus_config_passes_station_parity_through`（`:607`，**纯函数接缝、不触真串口**），另有 `bus_config_keeps_existing_passthrough_fields`（`:622`）钉住既有字段不受影响。**现场侧**（RC-6）只剩"真机实测 + 核对厂方参数 40016 的实际取值"，见 §11.11.3。

#### 11.4.6 mapper 扩展（`mupc-southd::mapper`）

```rust
/// 遥测点样本（比 (String, f64) 多一个类别标志，供 scheduler 做变化沿过滤）
pub struct TelemetrySample { pub metric: String, pub value: f64, pub kind: SampleKind }
pub enum SampleKind { Scalar, Bit }   // bool → 枚举（后续若加"字级信号"不破签名）

/// 全量点位（含 288 位）——逐点展开，读失败的块整块跳过
pub fn telemetry_points(role: Role, reads: &BlockReads) -> Vec<TelemetrySample>;

/// battery 的 SOC 域检查（PRD §9.6.3）：0 ≤ v ≤ 100 且有限 ⇒ Some(v)，否则 None
pub fn soc_in_domain(v: f64) -> bool;

/// battery 分支的 SOC 取值结果（把"底块读失败"等四种情形显式化，见下）
pub enum SocOutcome { Value(f64), NoSuchPoint, BlockFailed(String) }
pub fn battery_soc(reads: &BlockReads) -> SocOutcome;

/// 消防探测器登记数交叉校验（PRD §9.5.4"强制"）：返回 (读回的登记数, 配置容量) 不一致时的读回值
pub fn fire_detector_mismatch(role: Role, reads: &BlockReads) -> Option<f64>;

/// 消防探测器**地址升序**交叉校验（PRD §9.10 Q-9）：读回**全部探测器**的 `+0` 寄存器（地址号），
/// 非严格升序或重复 → Some((首个违规探测器的 1 基序号, 该探测器的地址值))。**Q-9 的设计落点。**
/// **链首 = 寄存器 11（探测器 1 的地址号）**，其后接 `fire_det*` 区的各探测器组
/// ⇒ 序号与 PRD §9.5.4"第 n 只 = 按地址升序的第 n 只"的 **n 逐一对齐**（口径见 §11.4.7.2）。
pub fn fire_detector_addr_order_violation(role: Role, reads: &BlockReads) -> Option<(usize, u16)>;

/// 钢瓶气压「是否配置」（PRD §9.7.6）：`ever_nonzero` = 本站生命周期内该点是否出现过非 0 值。
/// 从未非 0 ⇒ false（展示层标"未配置"而非 "0 kPa"），一旦出现过 ⇒ true（此后恒 0 按真实 0 展示）。
pub fn cylinder_pressure_configured(reads: &BlockReads, ever_nonzero: bool) -> bool;
```

- **按点名查找（本轮新语义）**：`Battery` 分支由"找 `b.name == "soc"` 的块"改为"**展开各块点清单，找 `metric == "soc"` 的点**"（PRD §9.4.3 的 `soc` 点契约）。解码用该点的 `RegDecode`。
- **Battery 分支的四种情形（上轮评审指出"底块读失败"语义未写）**：`SocOutcome` 把语义钉死，**逐条实现、逐条测**（② 另含"**读回长度装不下 `soc` 点**"这一子情形 —— 见下表 **② 补**行）：

  | 情形 | 判据 | `pkg.battery.soc` | `PollResult` | 说明 |
  |------|------|-------------------|--------------|------|
  | ① 底块读成功且含 `soc` 点 | 该块 `Ok(_)`，展开后 `metric == "soc"` | `Some(值)`（**域外则 `None`**，见下条） | `Data` | 正常路径 |
  | ② **底块读失败**（承载 `soc` 的那一块） | 该块 `Err(e)` | `None` | **`Failed(e)`** | **整轮无数据**：沿用 §10.7"任一块失败 = 整站本轮失败、无部分交付"的既有语义；退避/offline 记账同既有。**不得**退化为"只丢 SOC、其余照常"（那会造成"站在线但 SOC 静默缺失"） |
  | **② 补（判据补充 —— 不计作第五种情形，`SocOutcome` 三变体不变）** | **读回长度不足以容纳 `soc` 点**：承载 `soc` 的块 `Ok(v)`，但 `v.len() <` 该点占用的寄存器数（`RegDecode::width()`；如 32 位 `soc` 点只读回 1 寄存器、分片/截断返回） | `None` | **`Failed(...)`（= `SocOutcome::BlockFailed`，归属 ②）** | **必须归 ②**：该形态与"块 `Err`"同为"数据不可用 ⇒ 整站本轮失败"。**若静默返回 `None` + `Data`，即构成 §11.4.6 ② 明确禁止的"站在线但 SOC 静默缺失"**（更糟：`decode_regs` 对长度不足**解出 `0.0`**（§11.1 现状表），而 `0.0` 在域内 ⇒ 会被当成"真实 SOC = 0"推给控制链）。T6 实现即按 ② 同策处理，本行是**设计补齐**、后人不必推断 |
  | ③ 全站无任何块含 `soc` 点 | 展开后无 `metric == "soc"` | `None` | `Data`（空占位） | 防御性分支：**该形态在配置期已被规则 4 拒**（§11.5.1），此处的 `Data` 只为"调度器单测可绕过 validate"而保留，**不是**允许的配置形态 |
  | ④ 非承载 `soc` 的其它块读失败 | 任一其它块 `Err(e)` | `None` | **`Failed(e)`** | 与 ② 同：整站失败（§10.7），避免"半个站" |

  > ②③④ 的差别是刻意的：**②④ 属"通信/链路"故障 ⇒ 整站失败（可退避自愈）**；**③ 属"配置"错误 ⇒ 配置期拒**。二者混为一谈会让"配置错"在运行期表现为"站离线"（PRD §9.7.2 第 5 条明确禁止：配置错误一律由配置期校验拦截，不在运行期兜底）。**② 补**（读回长度装不下 `soc` 点）与 ② 同类（**链路/数据不可用 ⇒ 整站失败、可退避自愈**），**不得**按 ③ 的"配置缺该点"处理（它是**本轮没读全**，不是配置里没有）。
- **`Role::Pcs`**：与 `MeterBatt/Hvac/Fire` 同臂 → `empty_package()`（"站活着"信号）；其全部点只走 `telemetry_points`/`on_station_telemetry`，**不触发** `on_battery_soc`（N-1）与 `on_grid_package`（N-2）——这两条由 scheduler 的 role 判断**结构性保证**（只有 `Role::Battery` 才调 `on_battery_soc`；只有 `Role::MeterGrid` 才调 `on_grid_package`）。
- **SOC 域检查落点**：`Battery` 分支解出 `soc` 后，**域外则把 `pkg.battery.soc` 置 `None`**（控制链拿不到坏值），而 `telemetry_points` 仍按**原值**产出 `soc`（`§9.6.3` ③"telemetry 保留证据"）。越界的**告警事件**由 scheduler 发（§11.4.7）。
- **消防登记数**：`fire_detector_mismatch` 读"点名 `fire_det_count` 的点"（PRD §9.4.1 参考配置已在 `fire_sys` 的 `at: 7`（寄存器 10）上声明 `name: fire_det_count`），容量 = **升序链实际覆盖的探测器只数** = **`1 + Σ(以 `fire_det` 为前缀的块 `count`) / 6`**（**链首存在时 +1**；单块即退化为 `1 + 此块 count/6`）。两者不等 → 返回读回值。
  - **为什么必须 +1（订正原 `Σ/6` 口径 —— 原口径与"探测器 1 纳入升序链首"的裁定自相矛盾）**：已裁定**探测器 1（寄存器 11）纳入升序链首**，而**探测器 1 并不在 `fire_det` 块里 —— 它在 `fire_sys`（寄存器 11–16）**。因此 `Σ(fire_det 前缀块 count)/6` 只数得到探测器 **2..n**，比设备的登记数**恒少 1**：按 §9.4.1 参考配置 `fire_det.count = 114` ⇒ 旧式得 **19**（19 只），而**寄存器 10 读回 20** ⇒ **判据恒真**。
  - **恒真的后果（在状态翻转制下）**：**不是"事件风暴"** —— 翻转制下稳态本应 0 条；而是 **判据恒真 ⇒ 永久假告警**：首次观测产 **1** 条 + **每次站恢复再产 1 条**（§11.4.7.2 C 的"恢复后首轮仍成立 ⇒ 按首次观测即产再产 1 条"）⇒ **PRD §9.5.4 的"强制交叉校验"永久失效（RC-5 失效）**：真正的探测器增减被一个恒定成立的本底告警淹没，运维再也分不出"现场真的改了探测器"。
  - **与地址序校验同源（这是 `+1` 的判据，不是"凑数"）**：容量式与 `fire_detector_addr_order_violation` **覆盖同一个探测器集合**（链首 + `fire_det*` 区各组的 `+0`）。若两处集合不同，会出现"地址序链含 n 只、容量却按 n−1 只算"的口径分裂，且这类分裂在**探测器 1 被漏采**时恰好互相掩盖。
  - **链首不可得时的降级口径（与地址序校验的降级口径一致 —— 同一条条件、同一个集合）**：已定"配置未覆盖寄存器 11（非 §9.4.1 参考形态）⇒ **链首不可得，退化为只校 `fire_det*` 区**、不臆断违规"。**同一降级下容量式取 `Σ(fire_det 前缀块 count) / 6`（不加 1）**：即**容量式只计"链实际覆盖的探测器集合"**，与地址序校验**共用同一个"是否覆盖寄存器 11"的判定**、随之一同降级。**该形态下的比对照常执行（不得跳过）**：它是"配置所采 vs 设备登记"的事实比对，与地址序的"不臆断违规"是两回事 —— 配置只覆盖 `fire_det*` 区而设备登记数为 20 ⇒ **应当**报不一致（**这正是 RC-5 该抓的形态**：探测器 1 被漏采）。
  - **按 §9.4.1 参考配置复核**：链首存在（`fire_sys` 的 `addr: 4` + `count: 13` 覆盖寄存器 11）⇒ 容量 = **`1 + 114/6` = 20**；寄存器 10 读回 **20** ⇒ **一致，不产告警**（旧式 19 与之不等 ⇒ 判据恒真，即上条后果）。
  - **注（就地登记，非阻塞）**：**PRD §9.5.4 的同款算式原文亦是 `Σ(fire_det 前缀块 count)/6`**（未含链首）—— 链首裁定后该原文**同样少算 1**，与本设计的订正口径出现差异。本设计**按订正口径（`1 + Σ/6`）实现**（否则触发上条"永久假告警"），并**建议需求侧随 §9.5.4 同步订正**（属链首裁定的连带订正；**不改 PRD**，仅登记）。
- **消防地址升序（Q-9）**：`fire_detector_addr_order_violation` 逐只读 `+0`（地址号），**必须严格升序**（PRD §9.5.4"探测器按地址号从小到大顺序排列"）；违规 → 产事件（§11.4.7 ⑤，**事件产出频次口径见 §11.4.7.2**），并把该轮探测器区的点**标记为不可信**（telemetry 仍落原值，判据层拒用）——含义：**"第 n 只"只能等于"地址升序的第 n 只"**，顺序异常时点位与物理探测器**不再一一对应**。
  - **链首含探测器 1（须评审追认见 §11.12.1 项 8）**：升序链 = **〔寄存器 11〕→〔`fire_det*` 区各组的 `+0`〕**。依据 **PRD §9.10 Q-9 原文**"以**寄存器 11** 读回的地址值交叉校验" + §9.5.4"第 n 只 = 按地址升序的第 n 只"（该表述对 **n = 1 同样成立**，故探测器 1 必须在链里，否则"第 1 只"是否为最小地址**无判据**）。**取数方式与配置解耦**：在 `reads` 中取**覆盖寄存器 11 的那一块**的块内偏移 `11 − addr`（§9.4.1 参考配置即 `fire_sys`（`addr: 4`）的偏移 7），因此**不依赖块名、不依赖点位名**；若配置未覆盖寄存器 11（非 §9.4.1 参考形态）⇒ **链首不可得，退化为"只校 `fire_det*` 区"（= T5 现状），不臆断违规**（本设计**不新增**配置期规则要求覆盖 11）。该值**只参与升序比较**，不另判其合法域（PRD 未给判据，不猜）。

#### 11.4.7 scheduler 扩展（`mupc-southd::scheduler`）

| 改动 | 内容 |
|------|------|
| 块读分发 | `match blk.func { Holding => read_holding, Input => read_input, Discrete => read_discrete }`（`read_discrete` 的 `BlockReads` 需要承载 `Vec<bool>`：`BlockReads` 元素类型扩为 `BlockData::Regs(Vec<u16>) \| BlockData::Bits(Vec<bool>)`，**或**引入平行的 `BitReads` 通道——本设计选**前者**：`BlockReads = Vec<(RegBlockConf, Result<BlockData, String>)>`，避免两条 mapper 入口） |
| 变化沿过滤 | `PortRunner` 增 `edge_tracker: Mutex<HashMap<usize /*站下标*/, EdgeTracker>>`（**`BitEdgeTracker` 已推广为 `EdgeTracker`**，同时承载离散位与字级信号，见下"统一事件模型"）；每轮取"与上轮不同的位/信号 + 首次/复位后全量"；**站从 offline 恢复时先 `reset()`**（恢复后现势值连续性不可假设，须重发一次全量快照） |
| 事件产出 | ① `Battery` 且 SOC 越界 → `on_station_telemetry(id, role, vec![("soc_out_of_range", 原始值, true)])`（**不调** `on_battery_soc`；**属"状态型事件"，产出频次按 §11.4.7.2 的状态翻转口径 —— T6 落地）** ② 位点 `BitClass::Alarm` 的 **0→1** 跳变 → `(点位名, 1.0, true)` ③ 消防登记数不一致 → `("fire_detector_count_mismatch", 读回值, true)`（**同为状态型事件，按 §11.4.7.2 —— T6 落地**）④ **消防字级信号**（系统状态位 / 火警状态枚举 / 探测器总状态位）进入与退出活跃 → `("<点名>@<信号键>", 1.0/0.0, true)` ⑤ **消防探测器地址升序违规**（Q-9）→ **按状态翻转产出**：进入 = `("fire_detector_addr_order_invalid", 首个违规探测器的 1 基序号, true)`、恢复 = `("fire_detector_addr_order_invalid@recovered", 0.0, true)`；**状态未变的轮次一条都不产**、**首次观测即产**（**口径见 §11.4.7.2 —— T5 需按此补改**；原"命中即产"= 8.6 万条/日） ⑥ 消防钢瓶气压"未配置"**不产事件**（仅展示层标注，PRD §9.7.6）⑦ **PCS 32 位电量比对未通过**（PRD §9.8.4）：**本设计不产该事件** —— 该要求由 RC-2「未通过则不配 4 个 32 位电量点」替代（§11.7.4），**处置与理由见 §11.12.2 Δ-9（须需求侧追认）**；故事件表**不含**此项，实现时**不得**自行造判据（PRD §9.7.2 第 2 条禁本轮做量程自校验） |
| role 优先级 | `role_priority`：`MeterGrid\|Battery => 0`、**`Pcs\|MeterBatt => 1`**、`Hvac\|Fire => 2`（PRD §9.3.2.3） |
| 闸门不变 | 只有 `MeterGrid` 的 `on_grid_package` 推进 AiIntegrator 闸门；`Pcs`/`Battery`/`MeterBatt`/`Hvac`/`Fire` 一律不推进（§10.5 语义**零改动**） |

#### 11.4.7.1 统一事件模型

**问题（上轮评审 P0-3）**：PRD §9.6.1 要求 `fire` 的「**系统状态位 / 探测器 / 火警状态**」进 `events` + SSE，但 §11.4.7 的事件模型只认**离散位点**（`func: discrete` 块的 `BitClass::Alarm`）—— 而**消防全部状态量都在保持寄存器整字里**（`fire_sys`：addr 4 系统状态位图、addr 6/7/8 烟/温/可燃状态位图、addr 9 火警状态枚举；探测器 addr 12 状态位图），**本站没有任何 `discrete` 块** ⇒ 消防一个事件都产不出来，§11.7.1 的 `fire` 行「events」与 §11.8「维持 §10.4 融合」**缺一半输入**。

**解法：把"事件源"从"离散位"推广为"信号（Signal）"** —— 四类"字/位信号"（下表前四行）共用**同一套跃迁记忆与同一条事件产出路径**，唯一的差别是**活跃判据函数**（另加第 5 类**站级派生布尔量** `StationFlag`，见下表末行 —— 它同样走这条路径，但产出频次口径不同，见 §11.4.7.2）：

| 信号形态 | 载体 | 取值 | 活跃判据 | 事件产出 |
|----------|------|------|----------|----------|
| `Bit`（**既有**：BMS 288 位、空调 31 位） | `func: discrete` 块 | 位向量第 k 位 | 位 == 1 | 仅 **0→1 上升沿**（`BitClass::Alarm` 行；`State`/`Reserved` 只落 telemetry） |
| `WordBit`（**新增**：消防系统状态 / 烟温可燃 / 探测器总状态） | 保持寄存器**整字** | 整字值 | `字 & mask ≠ 0`（mask 取自 `point_table::signals`） | **进入/退出活跃均**产事件（见下"为何双向"）。**入表即产事件**，未入表的位一律只落 telemetry |
| `WordEnum`（**新增**：消防火警状态 addr 9） | 保持寄存器**整字** | 整字值 | 值 ∈ `active` 集合 | 同上；**活跃值之间跃迁亦产事件**（1 一级报警 → 2 二级火警） |
| `WordScalar`（阈值型模拟量） | — | — | — | **本轮不引入**（PRD 明令不做通用量程自校验；消防无模拟量消费方 —— 这同时是 G-6 延后的保护边界，见 §11.10） |
| **`StationFlag`（站级派生布尔量 —— 地址序违规 / SOC 域检查 / 登记数交叉校验）** | **无寄存器**（`mapper` 的交叉校验/域检查结果） | 布尔（判据成立与否） | `mapper::fire_detector_addr_order_violation(..).is_some()` / `!soc_in_domain(..)` / `fire_detector_mismatch(..).is_some()` | **按状态翻转产出**（进入 1 条 + `@recovered` 1 条、**状态未变不产**、**首次观测即产**）—— 与前三行的"跃迁即产"**不同**，口径与理由见 **§11.4.7.2** |

**统一机制（同一 `EdgeTracker`）**：每个信号（含每个离散位）在 `EdgeTracker` 里占一格"上轮活跃态"，每轮算出 `(上轮, 本轮)` 二元组：
- `false → true`：**进入活跃** ⇒ 产事件（`value = 1.0`）；
- `true → false`：**退出活跃** ⇒ 产事件（`value = 0.0`）；
- 其余（含首次采样的"未知 → X"）：**不产事件**（首轮只建立基线；站恢复后 `reset()` 同理，避免"恢复即刷一屏事件"）。
> **唯一例外**：第 5 类信号 `StationFlag`（站级派生布尔量）**首次观测即产**（它至多 1 条/站，"上线时已异常"必须可见）—— 见 **§11.4.7.2**。其余四类的"首轮只建基线"规则**不变**。

```rust
/// 某站的变化沿记忆（scheduler 内，按站下标索引）
pub struct EdgeTracker { last: HashMap<String /*信号全名或点位名*/, bool>, primed: bool }
impl EdgeTracker {
    /// 首轮/站恢复时调用：清空记忆并把本轮的活跃态记为基线（不产事件）
    pub fn prime(&mut self, now: &[(String, bool)]);
    /// 产事件：返回 (metric, value, is_event=true) —— 含 进入(1.0)/退出(0.0) 两类跳变
    pub fn edges(&mut self, now: &[(String, bool)]) -> Vec<(String, f64, bool)>;
}
```

> **双向/单向的落地形态（删除无定义的 `EmitBidi`/`emit_falling_edges`）**：此前曾在上面列出一个 `pub fn emit_falling_edges(class: EmitBidi) -> bool;`，但 **`EmitBidi` 在本设计中从未定义**（悬空符号）。本版**删去该行**：双向性是**信号形态自身的属性**，不需要额外开关 —— `WordBit`/`WordEnum`（消防字级信号）进入与退出**都产事件**；`Bit`（离散位块）**仅 0→1 上升沿**（`BitClass::Alarm` 行）。该差别落在 `scheduler` 调 `EdgeTracker::edges()` 后的**一次过滤**上（离散位只取 `false→true` 的项），**不是**一个公开 API，也不引入新类型。若将来需要开关，再按"新变体扩展"（与 `SignalSpec` 同一取向）。

事件名（**事件命名空间，非遥测命名空间**）：`<遥测点名>@<信号键>`。例：`fire_sys_1@main_power_fault`（系统状态 bit14 主电故障）、`fire_sys_6@level2`（火警状态 = 2 二级火警）、`fire_sys_8@alarm`（探测器 1 报警总状态）、`fire_det_2@fault`（探测器 2 故障总状态）。事件最终进 `storage.events` 的键仍是既有的 `south_station.<站id>.<metric>`，`metric` 即上述带 `@` 的名字（`SouthSink` 侧查 `point_table::label` 补中文名，查不到用原名）。

**为什么消防取"双向"，而 BMS/空调位块仍只取上升沿（不对称是刻意的，须登记）**：
1. 消防是**联锁判据源**（PRD §9.6.1：`fire` 行"是（仅事件/联锁判据，非控制量）"）；设计 §10.4 的融合规则含「**恢复需双方复位**」—— 联锁要能收敛，就必须知道**消防侧已解除**；只有上升沿会让"消防侧复位"这一事实**永远不可观测**。
2. BMS 288 位 / 空调 31 位的告警位**没有停机或联锁消费方**（PRD §9.6.1：BMS 告警位不触发停机），下降沿无消费方；对 288 位产双向事件只会制造事件噪声（去重键与保留策略压力）。
3. 消防信号总数受登记数约束、跃迁稀疏；BMS 位块相反（288 位/轮）。
> 该不对称登记为设计决策（§11.12.1 项 6），待评审追认。若评审要求统一，退路是"全部双向 + 事件节流"，代价见 §11.12.1。

**消防信号清单（哪些产事件、哪些只落 telemetry）**：

| 寄存器 | 点名 | 登记的信号键（**入表 = 产事件**） | 形态 | 依据 |
|--------|------|-----------------------------------|------|------|
| 4 系统状态 | `fire_sys_1` | `main_power_fault`（bit14）/ `backup_power_fault`（bit13）/ `drive_circuit_fault`（bit11）/ `pressure_sensor_fault`（bit10） | `WordBit{mask: 1<<n}` | PRD §9.5.4 位定义表；均为「1 = 故障」 |
| 4 | `fire_sys_1` | `spray_fired`（bit8，1 = 已喷）/ `valve_open`（bit9，1 = 开启） | `WordBit{mask: 1<<n}` | 灭火动作必须留痕（PRD 明确要求列举"喷洒标记"）；语义由 `label` 文案区分为"动作"而非"故障" |
| 6 / 7 / 8 | `fire_sys_3/4/5` | `smoke_trigger` / `temp_trigger` / `combustible_trigger` | `WordBit{mask: 0b11}` | bit1 复合探测器触发、bit0 干接点触发；**bit2（点型，预留未启用）不纳入 mask** —— PRD §9.7.6 明文"采集成点但不产出告警事件" |
| 9 火警状态 | `fire_sys_6` | `level1`（值 1）/ `level2`（值 2）/ `emg_start`（值 4）/ `emg_stop`（值 5） | `WordEnum{active: …}` | 值 3 预留（不纳入 `active`）；值 0 工作正常 = 非活跃（`active` 只列 1/2/4/5，故"任何活跃值 → 0"即退出活跃） |
| 12 探测器状态 | `fire_sys_9`（探测器 1）/ `fire_det_<序号>`（探测器 2..n） | `alarm`(bit12 报警总状态) / `fault`(bit14 故障总状态) | `WordBit{mask: 1<<n}` | PRD §9.6.1 明确要求"**探测器**"进 events；**探测器级只取这两条总状态**，bit0–4 的传感器细分**不产独立事件**（取舍：n≤100 ⇒ 逐传感器产事件会让事件源数 ×5，且告警类别可从该探测器原始整字回溯；登记为设计取舍 §11.12.1 项 6） |

**不登记信号（只落 telemetry，与上表同等重要 —— 防止"顺手多产"）**：

| 寄存器 / 位 | 为什么不登记 |
|-------------|--------------|
| 系统状态 bit15 工作模式 / bit12 充电状态 / bit7–0 备电电量 | 普通状态量与连续量；备电量是"值"不是"跃迁"（§11.7.2 第 7 条） |
| 探测器状态 bit15 通信状态（**0 = 离线，与其余位极性相反**）/ bit13 电磁阀 / bit10–11 反馈输入 | **极性反转位与反馈位无明确告警语义 ⇒ 本设计不猜、不造判据**（`SignalPick::WordBit` 的注释已把该口径写成约束） |
| 探测器状态 bit0–4（烟雾/温度/CO/H2/VOC 报警与传感器故障）| 逐传感器事件会让事件源数 ×5（n≤100），且 bit12 报警总状态已覆盖"该探测器报警"；细分可从整字原值回溯 |
| 5 钢瓶气压（整字） | 恒 0 不得判"气压异常"（PRD §9.7.6）；展示层按"未配置"呈现（§11.7.3），**永不产事件** |
| 11 探测器地址 / 13 数据 1 / 14–16 CO·VOC·H2 | 数据 1 的字节拆解属 G-6 展示层（§11.10），**事件层不触碰**；CO/VOC/H2 无阈值口径（文档未给），不得自行造判据 |

**事件检测**发生在**哪一层**：**`mupc-southd` 侧的 scheduler/tracker 层**（南向的"事件层"），**不**在下游展示层 —— 因为事件只能经 `on_station_telemetry(is_event=true)` 进 `storage.events` + `AlertFeed`，而**该通道只由 southd 持有**；若把检测放到 Web/展示层，火警将无法及时进 events/SSE，也接不上 §10.4 的联锁融合。

**与 §11.10 的 G-6 口径是否冲突：不冲突，边界如下（三层职责互不重叠）**：
1. **telemetry 值**：消防状态量一律**整字原值**落库（G-6 延后，`fire_sys_1/6/9` 等点的 `value` 就是整字 0–65535）——**不变**；
2. **事件层**：只做 **`字 & mask` / `字 ∈ 枚举集`** 的**位/枚举判定**（不涉跨字节组合的物理量、**不修改 telemetry 值**）—— 这是 PRD §9.7.6 明文授权的范围（"展示/**事件层**按字节语义拆解"）；
3. **展示层**：仍负责 §11.10 的**字节拆解**（唯一涉及跨字节的是"探测器数据 1"：高字节烟雾/低字节温度）—— 该点 **不产事件**（无模拟量消费方，PRD D-2 重启条件 ②），故与事件层**零重叠**。

> 一句话：**G-6 延后的是"把整字拆成两个遥测点"；消防事件用的是位/枚举语义 —— 二者不是同一件事，也不共用同一份拆解代码**（事件层只有 `mask/active` 常量，没有"字节切片"逻辑）。

#### 11.4.7.2 状态型事件的口径裁定

> **本节是两条事件口径的**设计裁定**（架构师职权），不是实现细节**：T5 已实现并通过代码评审，评审把 2 条事件口径问题**升级为设计裁定**，并明确"**不阻塞 T5，但禁止在裁定前把 `fire` 站写入生效配置**"。故本节给出可实施、可测、可验收的口径；**若需改代码，明确写出"需 T5 补改"**。

##### A. 背景（问题是什么）

| 项 | 事实 |
|----|------|
| **问题 1：事件风暴** | §11.4.7 的 ⑤「消防探测器地址升序违规」在 T5 实现上按"**命中即产**"落地 —— 判据仍成立的**每个轮询周期都产一条**。评审实测：5 轮 = **5 条**；`fire` 站 `interval_ms = 1000` ⇒ **约 8.6 万条/日**，对 `storage.events`（写入 + retention 清理）与 SSE（`AlertFeed` 推送）有实际压力 |
| **问题 2：`WordEnum` 退出的误读风险** | 火警枚举 `0→1→2` 时，`1→2` 那一轮**同轮**产出 `fire_sys_6@level1 = 0.0`（退出）与 `fire_sys_6@level2 = 1.0`（进入）。**联锁消费方逐条独立处理**时，看到 `level1 = 0.0` 会误判"一级报警已解除"，而 §10.4 的语义是"**恢复需双方复位**"（报警并未解除，只是**升级**） |
| **口径缺口（本裁定的对象）** | 设计目前只给 **offline** 定义了去抖口径（"首次失败立即记一次 + `stale_timeout_s` 窗内不重复"，§10.7）；**⑤（及同类 ① ③）没有任何产出频次口径**，PRD §9.10 Q-9 也未定义 ⇒ 实现忠于设计（**故不判 T5 错**），缺口由本节补齐 |

##### B. 判定的类：什么算"状态型事件"

| §11.4.7 事件 | 判据来源 | 判据取值 | 类别 |
|--------------|----------|----------|------|
| ① `soc_out_of_range` | `mapper::soc_in_domain`（PRD §9.6.3） | 布尔（域内 / 域外） | **状态型（本节口径）** |
| ② 位点 `BitClass::Alarm` | 离散位向量第 k 位 | 布尔 | 非（**已是跃迁量**，走 §11.4.7.1 的 `EdgeTracker`） |
| ③ `fire_detector_count_mismatch` | `mapper::fire_detector_mismatch` | 布尔（一致 / 不一致） | **状态型（本节口径）** |
| ④ 消防字级信号 | `SignalPick::is_active` | 布尔 | 非（**已是跃迁量**） |
| ⑤ `fire_detector_addr_order_invalid` | `mapper::fire_detector_addr_order_violation` | 布尔（合法 / 违规） | **状态型（本节口径，T5 已实现待补改）** |
| ⑥ 消防钢瓶气压"未配置" | — | — | **不产事件**（PRD §9.7.6，不适用） |

**"状态型"的判据 = 该事件的"成立与否"是一个判据函数的布尔返回**，**它本身不是跃迁量**（跃迁由"与上一轮的比较"产生，不是判据函数的输出）。**这一类的共同风险 = 只要判据不清零，就会每轮重产**。

##### C. 裁定 1：状态型事件一律按"状态翻转"产出（不逐轮重复）

| 项 | 口径（**可实施、可测**） |
|----|--------------------------|
| **产出口径** | **只在该布尔态发生翻转的那一轮产事件**：`false → true`（进入）产 **1** 条（事件名 = **原名**）；`true → false`（恢复）产 **1** 条（事件名 = **`<原名>@recovered`**）。**状态未变的轮次一条都不产**（稳态事件数 = **0**，只留翻转点） |
| **判据/去重的载体** | **复用 §11.4.7.1 的同一个 `EdgeTracker`**（不新建第二套记忆）：把该布尔量作为**第 5 类信号 `StationFlag`** 喂进同一个 `RoundSignals.all`，与位/字级信号**共用同一条事件产出路径**（信号形态表见 §11.4.7.1 末行） |
| **首次发现是否产** | **产**（进入事件）—— 这是"首轮/恢复后首轮只建基线、不产事件"**唯一的一条例外**。**理由**：① 它至多产 **1 条/站**（不构成风暴，无需靠"不产"来防刷屏）；② 若不产，则"**上线时地址序就已经错了**"将**永不产出事件**（状态无翻转点）⇒ §9.5.4"第 n 只 = 按地址升序的第 n 只"的前提**永久不可观测**，点位与物理探测器的错配被静默；这与 PRD §9.7.2 第 5 条（配置错误不得由运行期静默兜底）与 §9.7.6 的取舍取向（**该产的必须产**，不该产的一条都不产）同源 |
| **恢复后是否产"已恢复"事件** | **产**（`<原名>@recovered`，`value = 0.0`）。**理由**：它是"现场已改正探测器地址/接线"**唯一**的可观测点；没有它，事件日志**无法区分"仍然违规"与"已修复"**（与 §10.4"恢复需双方复位"要求复位可观测同一取向） |
| **站 offline → 恢复后** | 与其它信号**同等**处理（共用 `EdgeTracker`，恢复后首轮 `reset()` 重建基线）：恢复后首轮**仍违规** ⇒ 按"首次观测即产"**再产 1 条**。**不为它单开"跨离线保持"的第二种记忆** —— 代价 = 每次恢复最多多 1 条（恢复本身已是事件、由 offline 口径去抖），**收益 = 全系统只有一套记忆语义**（KISS） |
| **`value` 约定（进入/退出正交）** | **进入**：`value` = **诊断量**（⑤ = 首个违规探测器的 **1 基序号**；③ = 读回登记数；① = 越界原始值）—— ⑤ 与 T5 已实现的取值**一致**（不丢 RC-10 的定位信息）；**退出**：`value` 恒为 **`0.0`**（哨兵，不承载诊断量）。**为什么把"状态"放在事件名而不是 `value` 上**：诊断量**本身可以取 0**（③ 读回 0 只；① 若 `offset < 0` 则 raw 0 解出越界值）⇒ 单靠 `value` **无法同时承载"诊断量"与"进入/退出"两个维度**；`@recovered` 后缀让两维**正交**、消费方零歧义、存储键可直接区分（`south_station.<站id>.<metric>@recovered`）。它仍在 §11.4.7.1 的**事件命名空间**内（其 `metric` **不对应任何遥测点** ⇒ 与 `<遥测点名>@<信号键>` 的区分判据 = **前缀是否为已登记遥测点名**） |
| **为何不选"套 offline 的 `stale_timeout_s` 去抖窗"** | offline 用时间窗是**刻意**的：通信故障是"**每轮仍在发生的过程**"，周期性重提醒有意义（链路仍断着）。而**配置/接线类事实不随时间变化** —— 时间窗只把 **8.6 万条/日**降成 **1.7 万条/日**（5s 窗），**治标不治本**且把"频次"绑到一个语义无关的通信参数上；状态翻转口径把稳态事件数降到 **0** |
| **不做的事** | ① **不新增**"事件节流/采样/漏桶"机制（不引入 `stale_timeout_s` 之外的第二套窗）；② **不改** ② ④（它们已是跃迁量，本就是翻转产事件）；③ **不改**消防字级信号的双向决策（见下 D 节） |

**落点与影响（谁要补改）**

| 项 | 影响 | 说明 |
|----|------|------|
| **⑤ 地址序（T5）** | **需 T5 补改** | `mupc-southd/src/scheduler.rs`：把 `mapper::fire_detector_addr_order_violation(role, &reads).is_some()` 作为 `StationFlag` 汇入 `RoundSignals.all`（**去掉**现在"每轮直接 `events.push(...)`"的写法）；`edges()` **不**直接给这个 metric 的进入事件赋 `1.0` —— 在**同一处的后处理**里把进入事件的 `value` 替换为本轮判定出的**组序号**、把退出事件的 metric 改名为 `...@recovered`（`value = 0.0`）。**测试补改**：`scheduler.rs::fire_detector_addr_order_violation_emits_event` 现断言"首轮即违规 ⇒ 产 1 条 `value = 2.0`"，须改为"**首轮即违规 ⇒ 产 1 条**（首次观测即产）"+"**连续违规轮不重复**"+"**恢复升序 ⇒ 产 1 条 `@recovered`/`0.0`**"；并按 §11.4.6 的链首订正用例（见 E 节） |
| **① SOC 越界 / ③ 登记数不一致（T6，尚未实现）** | **无补改成本，但必须按本口径实现** | 这两条落在 T6（§11.13 T6："SOC 域检查、消防登记数"）。**若 T6 沿用"命中即产"，同样的风暴会原样重现**（SOC 越界持续 1h = 3600 条；登记数不一致持续 1 天 = 8.6 万条）—— 这正是本次裁定把口径**按类**给出（而非只补 ⑤）的原因 |
| **是否需需求侧追认"周期重提醒"** | 见 §11.12.1 项 8 | 若运维要求"持续异常须**周期性**重提醒"，那是一个**周期口径**，须由**需求侧**给出（本设计**不自造**窗）；机制上退化为"状态翻转 + 固定周期重发" |

##### D. 裁定 2：`WordEnum` 跃迁的"退出"事件语义 —— 维持现状 + 补消费方判据

**问题复述（评审原话的落地）**：`WordEnum` 取**双向**（§11.4.7.1 的刻意设计），故 `0→1→2` 时 `1→2` 会**同轮**产出 `fire_sys_6@level1 = 0.0` 与 `fire_sys_6@level2 = 1.0`。评审明确"这是统一规则的必然结果、与设计字面一致，**非缺陷**"；但需裁定是否存在**理解/联锁风险**。

**裁定：维持现状（不删退出事件、不改"活跃值间跃迁亦产"），把语义与消费方判据写死（不改代码）。**

| 项 | 口径 |
|----|------|
| **退出事件的语义** | "**该点（整字）已离开该枚举值**"——**不表示**该级别报警已解除，**不表示**消防侧已复位 |
| **消费方判据（★强制，须在联锁/展示消费侧生效）** | 判定"消防侧已复位"**必须按点名成组、同轮整体判读**，**禁止逐条独立解读**：<br>① 同一轮内、**同一 `<点名>`**（如 `fire_sys_6`）下出现**「退出 + 进入」** ⇒ 这是**级别跃迁**（报警**仍在**，级别已变，如 1→2、2→5）；<br>② 同一轮内、同一 `<点名>` 下**只有退出、无进入** ⇒ 该整字已回到**非活跃**（`WordEnum` 即值 0「工作正常」）⇒ **这才是"消防侧复位"的可观测证据**（§10.4"恢复需双方复位"的消防侧输入）；<br>③ `WordBit`（系统状态位图 / 探测器总状态）的各 bit **相互独立**，某 bit 退出即该位归 0，**可直接采用**（不适用①②的成组规则） |
| **是否需改 T5 代码** | **不需要**。T5 的实现与既有用例（`fire_wordenum_emits_transition_events_between_active_values`：`1→2` 恰产 1 退 1 进）**与此一致**；本裁定补的是**此前缺的消费方判据**，不是行为 |
| **现存消费方（实测）** | **无** —— core-bin 的联锁（`mupc-core-bin/src/interlock.rs`）目前**只由 DI/GPIO 通道驱动**（"停机仅以 DI3 触发"），**尚未消费 southd 的 fire 事件**；核间 10 §I-2 规划的"事件流 / `fire_state` 二选一"属**待实现**。故本裁定**无需补改任何现存消费代码**；它的作用是给**将来实现该消费方的人**一条**强制判据**（否则必踩"`level1 = 0.0` ⇒ 一级报警已解除"的坑） |
| **为何不选 (b)"只产更重级别的进入"** | 会**破坏按事件流重建状态**：`0→1→2` 下 `level1` 被静默置为非活跃（**不产**退出），消费方重建状态时会认为 `level1` **仍活跃**；且该规则要求 `EdgeTracker` **跨信号联动**（低级是否产退出取决于高级是否同轮进入），破坏 §11.4.7.1"一个信号占一格、彼此独立"的结构 |
| **为何不选 (c)"载荷带整字"** | 会让 `value` **同时**承载"该信号的活跃态"与"整字诊断量"两个维度，与 `Bit`/`WordBit` 的 `value = 0.0/1.0` 口径**分裂**；而整字原值**已在 telemetry 里**（§11.7.2 第 6 条"telemetry 仍落整字原值"）⇒ 需要整字的消费方**读 telemetry 现势值**即可，不必在事件里重复 |

##### E. 与 §10.4 联锁语义、§11.7 消费链路的边界（防误用，必须写清）

| 关系 | 口径 |
|------|------|
| **⑤ / ③ 不得并入 §10.4 的"触发取 OR"**（**强制**） | `fire_detector_addr_order_invalid`（与 `fire_detector_count_mismatch`）是**数据可信性告警**，**不是火警信号** ⇒ 若并入 §10.4 的 OR 判据，"**探测器地址配错 / 登记数不符**"会被升级为**消防停机触发**（PRD §9.6.1 的 fire 行是"仅事件/联锁判据"，其触发语义仍只能来自消防**报警/状态**量）。它们的消费方**只有运维侧**：事件流 / SSE / 展示层的"点位不可信"标记（§11.7.3） |
| **⑤ 的"不可信标记"不随事件去重而改变** | 事件**去重**只作用于 `events` 产出；§11.7.2 第 9 条的"**该轮**探测器区点位标记不可信"是**逐轮现势判定**，**每一轮都照旧生效**（两者互不影响：一个是事件频次，一个是判据层拒用） |
| **§11.7.1 的 fire 行** | "telemetry（整字原值）+ events（信号进入/退出活跃 + 登记数/地址序异常）"**不变**；本次新增的只是①②③的**频次口径**与 `@recovered` 命名 |
| **§11.7.2 第 7 条枚举清单** | 四条枚举信号与"值 0 = 非活跃"**一字不改** |
| **§11.8 的 §10.4 联接** | **已在 §11.8 同处补入**"成组判读约束"一句（**§10.4 本体不改**、§10 全章不改）。**若 §10/S2 侧的既有实现日后按"单条退出 = 复位"解读，须单独立项修正**（不属本章范围） |
| **§11.4.7.1 的"双向"理由（① 联锁要能收敛）** | **不变**：退出事件仍是"消防侧复位"的**必要**观测点 —— 本裁定只声明它**不充分**（同轮若有同点名进入 ⇒ 是跃迁而非复位） |


### 11.5 配置期校验（PRD §9.4.3 十七条逐条落点 + 规则 18/19）

#### 11.5.1 落点表

| # | PRD 规则 | 落点函数 | 判定依据 | 说明 |
|---|----------|----------|----------|------|
| 1 | 新 role 合法性 | serde（`Role`/`RegFunc`/`RegFormat`/`WordOrder`/`StationParity` 枚举） | 未知 YAML 取值 → 反序列化 `Err`（启动期 config load 即失败） | 无需新代码，**补单测**（未知 role 字符串 → Err） |
| 2 | 单站约束 | `SouthStationsConfig::validate` | `Role::Pcs` 计数 > 1 → Err | 与既有 `meter_grid`/`battery` 单站约束同形态 |
| 3 | 必填点表 | 同上 | `role == Pcs && regs.is_empty()` → Err | 空 regs = 站永久 offline 的静默死配 |
| 4 | `soc` 点契约 | `validate_station_regs` | `role == Battery` 且 `points::expand()` 展开后**无 `metric == "soc"`** → Err | 消费方按点名查找，缺名即静默不推 SOC |
| 5 | 格式与标度 | `validate_station_regs` | `int32_scaled/int16/uint16` 的**块级或点级** `scale == 0`（点级 `None` = 继承块级，只判一次）→ Err；`discrete` 块**不适用** | 防"raw×0 整块解 0" |
| 6 | 符号性一致性 | `validate_symbolicity` | ① `format∈{u16,i16} && offset≠0` 且 `lookup(role,addr)` **命中一行**、而该行 `sym_src` 为空 → Err（**已删去"无行即拒"半句**，理由见 §11.4.4）② `lookup` 命中且 `offset` 与登记值不等（含漏配 → 0）→ Err | 期望值来自 §11.4.4 的 `POINT_REGS`；`format`/`scale` 不强制（§11.4.4 口径）；**查不到行 → 放行** |
| 7 | 点位越界 | `points::expand` | 点覆盖 `[at−1, at−1+width)` 超出 `[0, count)` → Err | 返回 Err 含块名/点名/`at` |
| 8 | 点位重叠 | `points::expand` | 同块内两点覆盖同一寄存器/位 → Err | 32 位点占 2 寄存器，与相邻 16 位点重叠也算 |
| 9 | 32 位点对齐 | `points::expand` | `width == 2 && at − 1 + 2 > count` → Err | "半个 32 位值" |
| 10 | 点名唯一 | `validate_station_regs` | 展开后站内 `metric` 去重（含自动点名与显式 `name` 相撞）→ Err | 遥测键冲突 |
| 11 | 空洞上限 | `validate_station_regs` + `footprint` | 声明了 `points` 的标量块**窗口首尾以声明点锚定**：首个声明寄存器必须落在块内偏移 **0**（不得含前导未声明寄存器），最后一个声明寄存器**末端必须恰好等于 `count`**（末尾未声明寄存器不计入 `count`）；块内未声明寄存器的**连续空洞 > 4** → Err。`discrete` 块**不受此限** | 空洞判据来自 PRD §9.4.2.1 第 3 条（空读 ≤ 一次请求帧 8 字节） |
| 12 | 位块上限 | `validate_station_regs` | `func == Discrete && count > 2000` → Err | BMS ≤ 2000 位；`count == 0` 亦拒 |
| 13 | 地址有效性 | `validate_station_regs` | `addr == 0` **仅允许** `role ∈ {MeterBatt, Hvac}`；其余 role（含既有 `MeterGrid`、新增 `Battery`/`Pcs`/`Fire`）要求 `addr > 0` | 既有 `MeterGrid` 的 `addr > 0` 检查**被吸收进**本规则（行为不变） |
| 14 | 区间与重叠 | `validate_station_regs` | 同站**按功能码空间**（holding / input / discrete 三套地址空间）分别判半开区间重叠 → Err | 覆盖既有"仅 meter_grid 做重叠检查"的形态（PCS 3 区/4 区同址不同 func 由此天然放行） |
| **15** | **块落地极大性（重写口径）** | `validate_maximality` | **适用域（先行过滤）**：**仅当被考察的两个块都声明了 `points`（非空点清单）、且均非 `discrete` 块时才参与判定**；任一块未声明 `points` → **跳过、不判**（既有 `meter_grid` 六相量块、`bms_alarm`/`hvac_di` 两个位块均属此列）。<br>**判据（同时满足才拒）**：两块 `func` 相同、`byte_swap` 相同、地址**严格相邻**（`b.addr == a.addr + b.count` 即 `b.addr == a.addr + a.count`）、**合并后 `count = a.count + b.count ≤ MAX_SINGLE_READ_REGS (120)`**、**合并窗口内连续空洞 ≤ 4**、且**两块均未标 `read_slice: true`** → **Err**（提示合并）。<br>**豁免**：相邻块中**任一块**标 `read_slice: true` → 不拒（PRD §9.4.2.4，PRD v1.7 补登；四条"不得用于逃避合并"的边界以 PRD 为准） | **重写要点**：① **限定适用域**（只作用于声明了 `points` 的标量块）—— 否则会拒掉既有 `grid_meter`（六块地址严格相邻）⇒ 现场启动 fail-fast；② **判据与 PRD v1.8 逐条对齐**：`count ≤ 120`（④）+ 空洞 ≤ 4（③）**都实现**，其中 **③ 在适用域内由规则 11 的首尾锚定恒成立**（证明见 §11.5.2(1) 表后"③ 的等价性"），故实现 ③ 只是与 PRD 逐字对齐、不引入额外拒绝。**逐站证明见 §11.5.2(2)** |
| **18** | **`pcs` 站周期下界**（设计补落点；PRD §9.3.2.2(2) + §9.8.1 末条） | `SouthStationsConfig::validate`（站级基础校验，**与既有 `meter_grid`/`battery` 的 `< 5000` 拦截同址**） | `role == Pcs && interval_ms < 500` → Err | 防"误配的超短周期打满总线"；`pcs` **无** `< 5000` 上界约束（不参与控制决策，PRD §9.3.2.2(2)）。**该条不在 PRD §9.4.3 表内，由 PRD 另两条明文要求**（§9.3.2.2(2)、§9.8.1 末条）——AC-1 ③ 一并断言（§11.11.2） |
| **19** | **无 `points` 块的宽度护栏**（设计补落点；设计补充） | `validate_station_regs` | `func != Discrete && points.is_empty() && count % format.reg_width() != 0` → Err | 防"32 位格式的尾槽静默不产点"（如 `count: 3` 只产 1 点、少 1 点而无人知）。`discrete` 块与声明了 `points` 的块不适用（理由见 §11.4.3 护栏段）。登记为设计补充（§11.12.1 项 3b） |
| 16 | 同口一致性 | `validate()` 内联段 | 同 `port` 各站 `baud_rate` **与 `parity`** 必须一致 → Err | 既有 baud 循环扩展 parity（缺省 `none` 参与比较：空调站配 `even` 而同口另站未写 → Err，防静默忽略）。D-1 已裁定按 ①（新增站级 `parity`、空调 `even`）⇒ 本条的 `parity` 一侧为**活判据**（可执行用例见 §11.11.2 AC-1 ③，实现落点 `config.rs:308-327`） |
| 17 | 跨段互斥 | `core_config::validate_south_stations` | `port` 与 `intercore.modbus_rtu.serial_port` 同节点 → Err | **既有实现，沿用，零改动** |

**顺序（钉住，含一条保住既有断言的实现约束）**：`validate()` 内先做站级基础校验（id/port/slave/interval/baud，**含规则 18 的 `pcs` 下界**）→ 再 `validate_station_regs`（含展开，**含规则 10/13/14/19**）→ 最后跨站（单站约束计数、同口一致性、极大性）。**逐站短路返回首个 Err**（沿用既有风格：错误消息即定位信息，含站 id / 块名 / 点名 / 期望值）。

> **⚠️ 实现约束（必须遵守，理由见 §11.5.3.4.1）**：既有的 **`meter_grid` 站内完整性校验整组**（缺相量块 `p/q/pf/u/i` / `int32_scaled` 块 `scale > 0` / 相量块 `count ≥ 6` / `p_total count ≥ 2` / **块名唯一** / `addr > 0` / 半开区间不重叠）**保持在 `validate_station_regs` 之前、原地不动**（即仍在逐站循环的"站级基础校验"阶段），**不得后移、不得被通用规则 10/13/14/19 取代**。原因：这组校验对同一份坏配置会给出**与通用规则不同但更具体**的文案（`块名重复` / `addr 不能为 0` / `寄存器区间重叠` / `count 须 ≥ 6` / `须显式 scale>0`），而 §11.5.3.4 **C2**（S3b-1c 校验语义的回归锚）逐条断言了这些文案；顺序一换，C2 即失配（其中"两块同名 `p`"这一例**新规则 10 同样成立**，最易被误判为"文案可改"）。

> **规则 18/19 的编号说明**：PRD §9.4.3 的表是 **17 条**；18/19 是设计侧补的两条落点（18 由 PRD §9.3.2.2(2)+§9.8.1 明文要求但**未进 §9.4.3 表**；19 为设计补充的护栏）。二者均上报需求侧（§11.12.2），**AC-1 ③ 一并断言**但标注"非 §9.4.3 表内条件"。

#### 11.5.2 第 15 条的完整口径与「§9.4.1 六站必然通过」的证明

**（1）第 15 条的完整口径（按 PRD v1.8 逐条对齐重写）**

| 要素 | 旧口径 | **本版** | 为什么必须改 |
|------|-----------|------------------|--------------|
| **适用域** | 无（对所有块生效） | **仅当被考察的两个块都声明了 `points`（非空点清单）、且均非 `discrete` 块时才参与判定**；不满足者（未声明 `points` / 位块）**一律不参与**极大性合并判定 | 旧口径会把 `grid_meter` 的 6 个相量块（地址严格相邻）判成"可合并"→ `Err` → **现场启动 fail-fast**，与 PRD §9.4.1 第 6 站"逐字不动"、§11.2.1 的兼容性承诺、以及 T3/T7（旧口径）的"既有 20 例零破坏"互相矛盾（T3/T7 与回归闸门的措辞已一并订正，见 §11.13） |
| **判据** | 合并后**空洞 ≤ 4 寄存器**（PRD v1.6 的表述，只有这一条） | **PRD v1.8 的四条全实现**：①`func`/`byte_swap` 相同 ②地址连续 ③合并后空洞 ≤ 4 ④**合并后 `count ≤ MAX_SINGLE_READ_REGS (120)`**。其中 **③ 在适用域内恒成立**（证明见下），**④ 是真正起作用的新判据** | PRD v1.6 少了 ④ ⇒ 仅凭"空洞 0"就会把 `fire_sys`(13)+`fire_det`(114) 判成"可合并"→ `Err`，而合并体 127 寄存器**本就超过设备单次读上限**（PRD §9.5.4 要求 ≤120 分片）⇒ 结论方向反了。补 ④（"合并后能否一次读回"）后，`fire` 这类"本就该分片"的形态**不会被要求合并** |
| **豁免** | 块级 `read_slice`（设计自创、PRD 未列） | 相邻块中**任一块**标 **`read_slice: true`** → 不拒（**PRD §9.4.2.4 已正式补登**，v1.7；四条边界以 PRD 为准）；**且 `read_slice` 只在适用域内有对象**（未声明 `points` 的块本就不参与，标它是冗余、不构成错误配置 —— PRD v1.8 明文） | 现场按实测上限分片是**合法配置**，不能误拒；但豁免必须**显式、可审计**（注释登记理由，属 RC-1/RC-5 目视项），**不得**按 `role` 隐式特判（违反 G-5） |

`MAX_SINGLE_READ_REGS = 120`：取 **PRD 自己的分片口径**（BMS ≤120 寄存器、消防每片 ≤120 寄存器）——比 Modbus 标准上限 125 留 5 寄存器余量，是"保守的设备单次读上限"。也正因如此，**`pcs_3zone`（76 寄存器）与 `fire_det`（114 寄存器）自身单块 ≤120**，不需要 `read_slice`；只有当它们在现场被拆成多片时，才由配置者给分片块标 `read_slice: true`。

> **为什么必须"两块都声明 `points`"才判（强化理由，不只是"legacy 豁免"）**：未声明 `points` 的块，其点清单由**窗口宽度隐式决定**（每值槽 1 点），合并会同时改变**点的数量与默认点位名** —— 例如把 `fire_det` 并入 `fire_sys` 后，原 `fire_det_1` 会变成 `fire_sys_17`，而 PRD §9.4.2.2 明确"**改名 = 破坏历史数据可比性**、须按变更流程登记与评审"。而 `grid_meter` 的块名 `p/q/pf/u/i/p_total` **本身就是 mapper 的查找键**（PRD §9.4.2.4 明列为契约），合并它们等于**销毁契约**。因此对"含未声明 `points` 块"的相邻对做合并提示，是把**改名/契约破坏风险**伪装成"碎块风险"，**收益为负**。

> **③（合并后空洞 ≤ 4）在适用域内恒成立 —— 等价性证明（与 PRD v1.8 逐条对齐的关键）**：PRD v1.8 的拒绝条件是 ① `func`/`byte_swap` 相同、② 地址连续、③ 合并窗口内连续空洞 ≤ 4、④ 合并后 `count ≤ 120`。但在**适用域**（两块**都声明了 `points` 的标量块**）内，③ **必然成立**，理由：
> - 由 §11.5.1 **规则 11 的首尾锚定**，任一 `points` 块"**首个声明寄存器落在块内偏移 0**、**最后一个声明寄存器的末端恰好等于 `count`**" ⇒ A 块的**最后一个寄存器必被声明**、B 块的**第一个寄存器（= A 的末寄存器 +1）必被声明** ⇒ **接缝处空洞 = 0**；
> - 合并窗口内的连续空洞**只能来自 A、B 各块的内部空洞**（接缝已无空洞），而各块内部空洞**已被规则 11 单独限制为 ≤ 4**（否则该块自身早被拒）⇒ 合并后的每个连续空洞 ≤ 4 ⇒ **③ 成立**。
>
> **结论**：实现 ③（与 PRD 逐字对齐）**不引入额外拒绝**；同时 ④ 是本条真正起作用的新判据（"合并后能否被设备一次读回"）。**故本设计的判据与 PRD v1.8 行为等价**，且"六站必然通过"的结论不受 ③ 影响。实现顺序建议：先判 ④（`count > 120` → **不拒**，直接返回），再判 ③，最后判 ① ② 与 `read_slice` —— 这样 `fire` 的 `fire_sys+fire_det`（若未来 `fire_det` 也声明 `points`）会被 ④ 提前放行，**不会**因错误顺序被 ③ 之外的条件误拒。（注：`fire_det` 当前**未声明 `points`**，故 `fire` 站在适用域外，与实现顺序无关。）
>
> **澄清（不改变判据语义）：①②③④ 是「合取」——四者全成立 ⇒ 拒；任一不成立 ⇒ 不拒（放行）。因此 ③ *不是独立放行条件*，也不存在"仅 ③ 成立即放行"的形态**：在适用域内 ③ 恒成立，故一个"两块均声明 `points`、地址连续、合并后 `count ≤ 120`、无 `read_slice`"的配置**四判据全成立 ⇒ 必判 `Err`（提示合并）**。任何把"判据 ③ 单列"读成"③ 成立 ⇒ `Ok`"的用例期望值都与本条相左（§11.11.2 的该处已按此订正）。
>
> **一处需求文本的两种读法 —— 本设计取更保守者，并说明其本轮不可达（须评审备案）**：PRD v1.8 的本条正文以"**未声明 `points` 的块**（…亦含 `discrete` 位块）不参与"为例，但同条 ② / ③ 又出现"`discrete` 按位地址同理""`discrete` 位块不受 ③ 限制" —— 字面上可读成"**声明了 `points` 的 `discrete` 位块仍参与本条**"。**本设计取保守读法：`discrete` 块一律不参与**（与"本体不改变既有行为"的立意一致，也避免为位块引入"合并后位数折算成寄存器数"的口径）。**该分歧在本轮不可达**：§9.4.1 的两个位块（`bms_alarm` 288 位、`hvac_di` 31 位）**都未声明 `points`** ⇒ 两种读法对本轮配置**判定结果完全相同**。登记为需求侧备案项（§11.12.2 Δ-8）。

**（2）§9.4.1 参考配置（6 站）必然通过第 15 条 —— 逐站证明**

先枚举"同 `func` 且地址严格相邻（`b.addr == a.addr + a.count`）"的块对，再按适用域过滤。下表每一格的 `addr`/`count` **逐字取自 PRD §9.4.1 的 YAML**（可逐行复核，无需推断）。

> **取值真源与「不得取证于单测 fixture」的口径（二轮评审判定 §11.5.2(2) 的 `grid_meter` 行事实错误）**：
> - **生效配置的真值（唯一权威）**：`mupc/deploy/config/mupc_core_config.yaml:143-148` 与 `mupc/deploy/config/mupc_core_config.production.yaml:148-153` —— 两份文件该站的 6 行 `regs` **取值逐字相同**（仅注释措辞不同），且与 **PRD §9.4.1 第 6 站（PRD:1391-1396）逐字一致**：
>   `p`@`0x1000`(6) → `q`@`0x1006`(6) → `pf`@`0x100C`(6) → `u`@`0x1012`(6) → `i`@`0x1018`(6) → `p_total`@`0x101E`(2)。
> - **单测 fixture 的形态（仅测试自用，**不得**作为核对待迁移配置的依据）**：`mupc/crates/mupc-southd/src/config.rs:324-329` 的 `VALID_5_STATION_YAML` 里是另一种写法：`p`@`0x1000`(6) → **`p_total`@`0x1006`(2) → `q`@`0x1008`(6) → `pf`@`0x100E`(6) → `u`@`0x1014`(6) → `i`@`0x101A`(6)`，且 `format` **全部** `float32`（生效配置是 `int32_scaled`/`float32` 混排）、站 `id` 为 `meter_grid`（生效配置是 `grid_meter`）。
> - **两者确实不同，这是允许的**（单测自有构造数据、不调 `validate` 的解析类用例更不要求与现场配置同形），但**必须分别标注**：§11.5.3.4 C3 已把该 fixture 定性为"legacy 写法解析逐字段不变"的实证锚、**不得**被"顺手改成新写法"；而**本节及 §11.7.4 的迁移结论一律以"生效配置的真值"为准**。原 `grid_meter` 行恰恰把 fixture 形态当成了真值（并误称"逐字取自 PRD §9.4.1"）—— 该错误据二轮评审订正，**结论不变**（见该行"不拒 ✓"）。
> - **其余 5 站的逐站复核结论**：`bms` / `pcs` / `meter_batt` / `fire` / `hvac` 五行的 `addr`/`count`/`func` **逐格与 PRD §9.4.1 的 5 个新增站一致**（这五站在生效配置中仍是注释占位，真值即 PRD §9.4.1，不存在"fixture 与真值两张皮"的问题）⇒ **仅 `grid_meter` 一行有此类取证错误，已订正**。

| 站 | 同 `func` 且严格相邻的块对（穷举） | 是否参与判定 | 结论 |
|----|-----------------------------------|--------------|------|
| **`grid_meter`（既有站，PRD §9.4.1 第 6 站 = 生效配置真值）** | `p`(0x1000,6) → `q`(0x1006,6) → `pf`(0x100C,6) → `u`(0x1012,6) → `i`(0x1018,6) → `p_total`(0x101E,2)：**5 个相邻对全部成立**（0x1006=0x1000+6、0x100C=0x1006+6、0x1012=0x100C+6、0x1018=0x1012+6、0x101E=0x1018+6，`func` 均 `holding`、`byte_swap` 均缺省 false）。**注**：`p_total` 是**末块**（不是第二块）—— 单测 fixture（`config.rs:324-329`）把 `p_total` 放第二位且 `format` 全为 `float32`，**与生效配置不同**，勿据其核对待迁移配置 | **否 —— 6 块均未声明 `points`** | **不拒 ✓**（这正是旧口径误拒、必须重写的那一处） |
| `bms` | `bms_io`(100,31) 止于 130；`bms_energy`(139,19) 止于 157；`bms_meta`(181,9) 止于 189；`bms_term`(2991,4)；`bms_cap`(4000,6)；`bms_alarm` 为 `discrete`。逐对检查：139−131=**8**、181−158=**23**、2991−190 巨大、4000−2995 巨大 ⇒ **无相邻对** | 无 | **不拒 ✓** |
| `pcs` | 单块 `pcs_3zone`(1000,76) | 无相邻对 | **不拒 ✓** |
| `meter_batt` | `mb_e_*` 六块起点 0x0000/0x000A/0x0014/0x001E/0x0028/0x0032，`count` 均 2 ⇒ 0x0000+2=0x0002 ≠ 0x000A（**相隔 8 寄存器**，PRD 注释亦写明）；`mb_ui`(0x0061,6) 止于 0x0067 ≠ 0x0077；`mb_freq_line`(0x0077,4) 止于 0x007B ≠ 0x0087；`mb_phase`(0x0087,14) 止于 0x0095 ≠ 0x0164 ⇒ **无相邻对** | 无 | **不拒 ✓** |
| `fire` | `fire_sys`(4,13) 与 `fire_det`(17,114)：4+13 = **17**，同 `holding`、同 `byte_swap` ⇒ **1 个相邻对成立** | **否 —— `fire_det` 未声明 `points`**（`fire_sys` 有）⇒ 不参与 | **不拒 ✓**。**双重保险**：即便参与判定，合并后 `count = 13 + 114 = 127 > 120` ⇒ 判据也不触发（与现场"探测器区必须分片"的事实一致） |
| `hvac` | `hvac_in`(0,4,`input`) 与 `hvac_di`(0,31,`discrete`)：`func` **不同** ⇒ 非相邻；块内无其它块 | 无 | **不拒 ✓** |

**结论**：**§9.4.1 的 6 站参考配置（含既有 `grid_meter` 形态）必然通过第 15 条**。依据两条、各自充分：
1. **`bms`/`pcs`/`meter_batt`/`hvac` 四站**：穷举后**不存在任何"同 `func` 且严格相邻"的块对** ⇒ 第 15 条**无从触发**；
2. **`grid_meter` 与 `fire` 两站**：相邻对确实存在（前者 5 对、后者 1 对），但**都因"至少一方未声明 `points`"而不参与判定** ⇒ **不拒**；其中 `fire` 还有第二重保险（合并体 127 > 120，即便参与也不触发）。

故 **AC-1 ② 的断言（"除 §9.4.3 明列条件外不触发任何既有/新增拒绝"）对第 15 条成立** —— 现场启动不会因本条 fail-fast；且 §11.11.2 的 AC-1 ③ 已把第 15 条的 **8 种形态**（`Ok` **7** + `Err` **1**）**全部做成可执行用例**（`Ok`：六站 YAML / 仅 grid_meter / 一方无 `points` / 双方无 `points` / 合并后 >120 / 带 `read_slice` / `discrete` 块相邻；`Err`：双方有 `points` + 合并 ≤120 + 无 `read_slice` —— **该形态即"判据 ③ 单列"的形态**，因 ③ 在适用域内恒成立，故其断言是 `Err`），"必然通过"这句话本身也有断言钉住。

**（3）`read_slice` 真正的适用场景（避免被当成"逃避合并的开关"）**：只有**两个都声明了 `points` 的块**、合并后 `count ≤ 120`（说明"设备本来能一次读回"）、却因现场实测上限更低而必须分片时，才标 `read_slice: true` —— 典型是 **PCS 3 区按实测拆成 38+38**（两块都是 `points` 块，合并 76 ≤ 120 ⇒ 不标就会拒）。PRD §9.4.2.4 的四条约束（合并后仍可一次读回者必须合并 / 只豁免本条 / 块级逐块判定 / 须注释登记理由）在此**逐条适用**。

**（4）另一处设计补充的保守口径（PRD 未定义）**：点级 `name` 与 `count > 1` 同时出现 → **配置期拒**。理由：PRD §9.4.2.2 只定义了"点名 ↔ 序号"的单点形态，未定义"命名序列"；禁用比发明安全。该条**不在 PRD §9.4.3 清单内**，登记为设计补充（§11.12.1 项 3）。

#### 11.5.3 既有测试的连锁订正清单

上轮清单只到"文件 + fixture"级，且漏报了**断言级失配**与两处用例（`core_config::test_south_stations_grid_only_passes`、`test_south_stations_same_port_same_spelling_passes`），与 §11.13「仅 §11.5.3 列出的 fixture 订正」口径不符。本版按**三类**重建清单（已逐处 grep 核实到 `file:行`）：

- **① fixture 订正**：构造数据要改，**断言不改**；
- **② 断言订正**：期望值要改（**因 metric 更名而来，不是回归**）；
- **③ 断言不得改**：回归锚 —— 改了就是**掩盖回归**，须在评审中说明理由。

##### 11.5.3.1 订正规模（逐个数核实）

| 受影响的构造点 | 实测处数 | 位置（`file:行`） |
|----------------|----------|-------------------|
| `RegBlockConf` 字面量 | **10** | `mupc-southd/src/mapper.rs:226`、`mupc-southd/src/scheduler.rs:430`、`746`、`754`、`794`、`802`、`810`、`853`、`861`、`tests/grid_convergence.rs:28` |
| `StationConf` 字面量 | **8** | `mupc-southd/src/port_runtime.rs:289`、`mupc-southd/src/scheduler.rs:452`、`474`、`487`、`737`、`785`、`844`、`918` |
| 需改构造数据的 YAML fixture / 内联 YAML | 见 11.5.3.2 | （下同） |
| 需改期望值的断言 | 见 11.5.3.3 | （下同） |

> 这 18 处**全部**要补新字段（`RegBlockConf` 补 `offset/byte_swap/points/read_slice`；`StationConf` 补 `parity`），属**纯机械补字段**（Rust 结构体字面量无 `..Default::default()`，故逐处补），**不改变任何测值**。

##### 11.5.3.2 【① fixture 订正】（构造数据要改，断言不改）

| # | 文件 | 用例 / fixture（`file:行`） | 订正内容 | 订正后断言 |
|---|------|---------------------------|----------|-----------|
| A1 | `mupc-southd/src/config.rs` | 静态 fixture `VALID_5_STATION_YAML`（`:336` `battery_1`、`:348` `fire_1` —— 行号已订正，原写 `:345`；该 fixture 的 meter_grid 段见 `:324-329`，其取值与生效配置**不同**，理由见 §11.5.2(2) 的"取值真源"注） | ① `battery_1` 的块 `{ name: soc, addr: 100, format: int32_scaled, scale: 0.1, count: 2 }` → 改为**点名式**：`{ name: bms_io, addr: 118, count: 1, format: uint16, scale: 1.0, points: [{ at: 1, name: soc }] }`（否则规则 4 拒）；② `fire_1` 的 `addr: 0` → `addr: 4`（否则规则 13 拒） | 使用该 fixture 的 3 个用例：`parses_valid_5_station_yaml`（只解析，**不受影响**）、`meter_grid_regs_contain_pq_pfu_i_p_total`（只查 grid 站，**不受影响**）、`validate_passes_for_valid_config`（`is_ok()`，**依赖本订正**） |
| A2 | 同上 | `validate_battery_interval_boundary`（`:502`，内联 YAML `{ id: bat, role: battery, … interval_ms: {iv} }`） | 补含 `soc` 点的 `regs` | `assert_eq!(validate().is_ok(), ok)`（2 组 iv）**不变** |
| A3 | 同上 | `validate_accepts_max_slave`（`:471`，`slave: 247`） | 同上 | `is_ok()` **不变** |
| A4 | 同上 | `validate_accepts_same_port_same_baud`（`:791`） | battery 侧补 `regs` | `is_ok()` **不变** |
| A5 | 同上 | `validate_accepts_diff_ports_same_baud`（`:817`） | 同上 | `is_ok()` **不变** |
| A6 | `mupc-core-bin/src/core_config.rs` | `test_south_stations_valid_5_station_passes`（`:1744`） | `battery_1` 补含 `soc` 点的 `regs` | `is_ok()` **不变** |
| **A7** | 同上 | **`test_south_stations_grid_only_passes`（`:1856`）** —— **上轮漏报** | 同上 | `is_ok()` **不变** |
| **A8** | 同上 | **`test_south_stations_same_port_same_spelling_passes`（`:1973`）** —— **上轮漏报** | 同上 | `is_ok()` **不变** |
| A9 | 同上 | `test_south_stations_shared_serial_with_pcs_short_rejected`（`:1786`） | `battery_1` 补 `regs` —— **必须补**：否则规则 4 的 `soc` 错误会**先于**跨段校验返回，`err.contains("重复") && err.contains("仲裁")` 直接失配（这正属"断言级"影响，旧清单只说"补 regs"未说清**为什么**） | 断言**不变** |
| A10 | 同上 | `test_south_stations_shared_serial_with_pcs_fullpath_rejected`（`:1816`） | 同上 | 断言**不变** |
| A11 | 同上 | `test_south_stations_same_port_alias_spelling_rejected`（`:1941`） | `battery_1` 补 `regs` | `err.contains("别名")或("同节点")` + `contains("hvac_1") && contains("battery_1")` **不变** |
| A12 | `mupc-southd/src/mapper.rs` | `poll_to_result_battery_soc_block_maps_soc`（`:369`）的 `let ok = vec![sblock("soc", 65.5), sblock("temp", 25.0)];` | 承载 `soc` 的条目改为**点名式块**（`RegBlockConf{ name: "bms_io", addr: 100, func: Holding, format: Float32, scale: 1.0, count: 2, points: [PointConf{at:1, name: Some("soc")}] , …}`）—— 否则新语义（按**点名**查找）找不到 `soc` 点 | `pkg.battery.soc == Some(65.5)`、`temperature == None`、`inverter_status == Running`、`Err → Failed`、`空 reads → Data + soc None` **全部不变** |
| A13 | `mupc-southd/src/scheduler.rs` | `battery_conf()`（`:473`，块名 `soc`） | 同 A12 改为点名式（保持 float32 + 65.5 量值，使 `soc_of == Some(65.5)` 成立） | 见 11.5.3.3 **第 9 条**（交叉引用已订正：原写"第 8 条"，该条讲的是 `telemetry_points` 的值槽数，与本例无关） |
| A14 | **同一 crate（`mupc-southd`）**的 18 处字面量（11.5.3.1；原写"三个 crate"有误，这 18 处全部落在 `mupc-southd`：`src/mapper.rs`、`src/scheduler.rs`、`src/port_runtime.rs`、`tests/grid_convergence.rs`） | 机械补新字段 | 无 | 无 |
| **A15** | `mupc-southd/src/config.rs` | **`validate_rejects_slave_out_of_range`（`:520`）** —— **由 §11.5.3.5 移入本类** | 补合法 `regs`（含 `soc` 点） | `is_err()` **不变**（见下方"语义漂移"注） |
| **A16** | 同上 | **`validate_rejects_zero_interval`（`:534`）** —— **同上移入** | 同上 | `is_err()` **不变** |
| **A17** | 同上 | **`validate_rejects_duplicate_id`（原 `:406`）** —— **由 §11.5.3.5 移入本类（实现期实证为假绿）** | 首站 `battery` 补含 `soc` 点的 `regs`（否则先被规则 4 拒，`Err` 文案即规则 4） | `is_err()` **不变** |
| **A18** | 同上 | **`validate_rejects_multiple_meter_grid`（原 `:419`）** —— **同上移入** | 两个 `meter_grid` 站均补**完整相量块**（否则先被既有 `meter_grid` 整组校验"缺相量块"拒） | `is_err()` **不变** |
| **A19** | 同上 | **`validate_rejects_multiple_battery`（原 `:434`）** —— **同上移入** | 两站均补含 `soc` 点的 `regs`（否则先被规则 4 拒） | `is_err()` **不变** |
| **A20** | 同上 | **`validate_rejects_same_port_mixed_baud`（原 `:779`）** —— **同上移入** | battery 侧补含 `soc` 点的 `regs`（否则先被规则 4 拒，测不到规则 16） | `is_err()` **不变** |
| **A21** | 同上 | **`validate_rejects_same_port_mixed_baud_3_station`（原 `:803`）** —— **同上移入** | 同上 | `is_err()` **不变** |

##### 11.5.3.3 【② 断言订正】（metric 更名导致期望值要改 —— **不是**回归锚）

根因：`telemetry_points` 对**未声明 `points` 的块**由"取首值、metric = 块名"改为"**每值槽 1 点、metric = `<块名>_<序号>`**"（§11.2.1 已登记的行为变更）。**受影响范围已逐处核实**：

| # | 用例（`file:行`） | 原断言 | 订正为 | 依据 |
|---|-------------------|--------|--------|------|
| 1–5 | `scheduler.rs`：`two_stations_same_port_schedules_by_due`（`:632`）、`grid_offline_isolated_hvac_continues`（`:662`）、`station_recovers_after_failure`（`:685`）、`station_recovers_after_extended_backoff`（`:719`）、`non_battery_role_never_triggers_soc_channel`（`:951`）—— **共 5 处 `assert_eq!(telemetry_of("hvac"), …)`** | `vec![("temp".to_string(), 23.5)]` | **`vec![("temp_1".to_string(), 23.5)]`** | 块 `temp`：`addr 100`、`count 2`、`format float32` ⇒ 宽度 2 ⇒ 1 个值槽 ⇒ 序号 = 0+1 = 1 |
| 6 | `scheduler.rs::station_mixed_holding_and_input_blocks`（`:768-770`） | `m == "temp"`、`m == "alarm_in"` | `m == "temp_1"`、`m == "alarm_in_1"` | 同上（两块各 `count 2` + `float32`） |
| 7 | `scheduler.rs::pure_input_blocks_station_collects_via_fc04`（`:877-879`） | `m == "alarm_in" && v == 0.5`、`m == "status_in" && v == 1.5` | `m == "alarm_in_1"`、`m == "status_in_1"`（**值 0.5/1.5 不变**） | 同上 |
| 8 | `mapper.rs::telemetry_points_first_value_per_block`（`:392`） | `assert_eq!(pts, vec![("p",1.0),("u",220.0)])` | **`vec![("p_1",1.0),("p_3",2.0),("p_5",3.0),("u_1",220.0),("u_3",221.0),("u_5",222.0)]`**（**测试名同步改为 `telemetry_points_every_value_slot`**） | 单块 6 寄存器 / `float32` ⇒ 3 个值槽；**点产出数量本身也变了**（1 点 → 3 点），这是 AC-6 ①"每值槽 1 点"的钉子。注：`blk()` 的 `scale = 0.0` 不影响 `float32`（`scale` 仅对整数格式生效，规则 5 亦不适用于 `Float32`），故值仍为 1.0/2.0/3.0 |
| 9 | `scheduler.rs::battery_station_soc_pushed_via_dedicated_channel`（`:896`） | `m == "soc"` | **不变**（`battery_conf` 按 A13 改为点名式后，点位名就是显式 `soc`） | PRD §9.4.2.2"显式 `name` 优先" |

> **为什么这些不属"断言不得改"**：它们断言的是**遥测键名**，而键名的变化是 PRD §9.4.2.1 第 4 条**规定的**（"未声明 `points` ⇒ 每值槽 1 点 + 位置式命名"）；#8 甚至断言的是**点数**，也由同一条规定改变。把它们当回归锚会**永久锁死 PRD 的规则**。反之（见 11.5.3.4）`grid_convergence.rs` 的断言与 grid 站的 `decode_phase_block` 语义**不经过**这条路径 —— 那才是回归锚。

##### 11.5.3.4 【③ 断言不得改】（回归锚，逐项说明"为什么它是锚"）

| # | 用例 / 断言 | 为什么不得改 |
|---|-------------|--------------|
| C1 | **`mupc-southd/tests/grid_convergence.rs` 全部 4 个用例的全部断言**（`meter_grid_phase_matches_legacy_semantics_canned` / `meter_grid_p_total_raw_when_present` / `meter_grid_missing_phase_block_returns_failed` / `meter_grid_negative_p_direction_signs_current`） | **评审员特别强调**：这是 S3a 收敛闸门（§10.9）。grid 路径走 `poll_to_result(MeterGrid)` → `decode_phase_block`（**按块名**查找），**完全不经过** `points[]`/`telemetry_points` ⇒ 本轮所有更名对它**零影响**。**只允许改 `block()` helper 的构造**（补 4 个新字段），**任何断言值的改动都视为掩盖回归** |
| C2 | `mupc-southd/src/config.rs` 的 **meter_grid-only 系列**：`meter_grid_regs_contain_pq_pfu_i_p_total`、`validate_meter_grid_interval_boundary`、`validate_rejects_meter_grid_missing_phase_block`/`_empty_regs`/`_zero_addr`/`_overlapping_regs`/`_phase_block_count_lt_6`/`_int32_scaled_zero_scale`/`_duplicate_block_name`、`validate_accepts_meter_grid_complete_regs`/`_without_p_total`、`meter_grid_full_regs_yaml`/`meter_grid_only_yaml` | 它们是 S3b-1c 的既有校验语义（相量块齐备、`count ≥ 6`、`addr > 0`、区间不重叠、块名唯一、`int32_scaled` 的 `scale > 0`）的**唯一钉子**；且这 6 个相量块**未声明 `points`** ⇒ 新的规则 15/19 与逐点展开**都不触碰它们**（这正是 §11.5.2 的适用域限定要保住的）。**⚠️ 与规则 10 的冲突及钉法见下方 §11.5.3.4.1** |
| C3 | `mupc-southd/src/config.rs` 的**解析类**：`default_reg_format_used_when_omitted`（`blk.format == Float32`、`scale == 0.0`、`count == 2`）、`reg_block_func_parses_holding_input`（`func` 缺省 = `Holding`）、`default_field_fallbacks_apply`、`default_baud_and_func_apply_when_omitted`、`parses_valid_5_station_yaml` | 它们钉住"**legacy 写法（块名 `soc`、无 `points`、缺省 `format`）解析逐字段不变**" = §11.2.1 兼容性论证的实证。**其 fixture 仍保留 `name: soc` 的旧写法**（parse-only，不调 `validate`），**不得**被"顺手改成新写法" |
| C4 | `mupc-core-bin/src/core_config.rs` 的 6 个 southern-stations 用例的**断言**（含 `err.contains("重复") && err.contains("仲裁")`、`err.contains("别名")`、`err.contains("interval_ms") && err.contains("south_stations")`、两处 `is_ok()`） | 跨段校验（port 与 intercore 互斥、同口别名、interval 传播）是 §10.3 的既有契约；**只改 fixture（A6–A11），断言一个字节都不动** |
| C5 | `scheduler.rs` 的 `soc_of` / `grid_count` / `event_count` / `bus.call_count` / `input_call_count` / `state[..].offline_count` 类断言（`:632` 的 `call_count`、`:640`、`:668`、`:693`、`:778`、`:839`、`:886`、`:901`、`:1015`、`:1039`、`:1066` 等） | 与 metric 命名无关：断言的是**调度节拍、退避、隔离、SOC 通道 gating**（§10.2/§10.5 语义）。若它们变红，就是**回归** |
| C6 | `scheduler.rs::battery_station_without_soc_block_does_not_push`（`:914`，`regs: vec![]`）**的全部断言**（无 offline/online 事件、`soc_of` 为 `None`） | 该配置形态在新规则下**已被配置期拒**（规则 4），但本用例**不调 `validate`**（直接构造 `StationConf` 交给调度器）⇒ 仍绿，且它是**调度器层的纵深防御锚**："无 `soc` 点 ⇒ 绝不误推 `on_battery_soc`"（PRD §9.4.3 规则 4 的运行期对偶）。故：**断言保留不动**，只加注释说明"该形态在配置期已被规则 4 拒，本测保的是调度器侧不依赖配置校验的负向 gating"；**同时**在 `config.rs` **新增** `validate_rejects_battery_without_soc_point` 覆盖配置期侧（两层各有其测） |
| C7 | `mupc-southd/src/mapper.rs` 的 `meter_grid_*`（`:286`/`:314`/`:331`/`:338`/`:347`）与 `poll_to_result_other_role_returns_minimal_data`（`:404`）断言 | 前者是 mapper 层的总表语义锚，后者钉住"非 grid/battery role ⇒ 空 `DataPackage`"；本轮只给 `Pcs` 加同臂，**语义零改动** |

> **`SouthScheduler::new` 不得新增 `validate()` 调用**（决议）：否则 C6 会因"构造了非法配置"而变红，且会把"配置期一次校验"变成"每次装配都校验"。配置校验的**唯一入口**仍是 `core_config` 的装配期（§10.3）。

##### 11.5.3.4.1 【C2 与规则 10 的冲突：实现判定顺序约束】

**冲突事实**：C2 里的 `validate_rejects_meter_grid_duplicate_block_name`（`config.rs:681-694`）构造的是**两个同名 `p` 块**（`p`@0x1000(6) 与 `p`@0x1006(6)，均无 `points`、`format: float32`）：
- **既有规则**（`meter_grid` 块名唯一，`config.rs:213-223`）命中 ⇒ 报 `south_stations: meter_grid 站 {id} regs 块名重复: {name}（mapper 按 name 取首块，后者静默失效）`，断言 `err.contains("块名重复") && err.contains("p")`；
- **但新规则 10（点名唯一）同样成立**：两个块展开后各产 `p_1`/`p_3`/`p_5`（`float32` 宽度 2、`count 6` ⇒ 3 个值槽）⇒ 站内 `metric` 重复 ⇒ 规则 10 也可判 `Err`（文案是"点名重复"一类）。

**若实现顺序变化，报错文案随之改变，而 C2 又把它列为"断言不得改"** ⇒ 开发无从猜测。**本设计按下述方式钉死（不必猜）**：

1. **判定顺序固定为**：站级基础校验（含既有 `meter_grid` 整组校验，**块名唯一在其中**）→ `validate_station_regs`（含展开 + 规则 10/13/14/19）→ 跨站（单站计数 / 同口一致性 / 极大性）。即 §11.5.1「顺序」给出的**实现约束**：既有 `meter_grid` 整组**原地不动、先于展开校验**。
2. **该用例命中的规则与文案**：命中**既有 `meter_grid` 块名唯一规则**，文案**逐字不变**（上引原文）⇒ **C2 断言不动**（"断言不得改"类别维持）。
3. **规则 10 不得因此删除，且有它独有的覆盖面**：块名唯一是 **`meter_grid` 专有**（因 mapper 按 `name` 用 `.find` 取首块）；其余 role 的同名块，以及"**块名不同、展开后 metric 却相同**"（如两块各有一个 `name: soc` 的显式点名、或显式 `name` 与另一块的自动位置点名相撞）**只有规则 10 能拒** ⇒ 须**新增用例**覆盖（已并入 §11.11.2 AC-1 ③ 的规则 10 一条）。
4. **为什么不订正 C2（对上一轮清单的处置说明）**：上一轮把 C2 列为"断言不得改"是**正确的，本轮不作订正** —— ① `块名重复` 是**根因**（mapper 按 `name` 取首块 ⇒ 后者静默死配），`点名重复` 只是同一配置的**派生症状**，报根因对运维可操作性更强；② C2 是 S3b-1c 校验语义的**唯一钉子**，改文案等于削弱回归锚；③ 本约束**零实现成本**（既有检查原地不动即可），不存在"为了保测试而扭曲设计"的代价。

##### 11.5.3.5 【④ 期望 `Err` 的用例：fixture 与断言均无需改动】

> **分类归属订正（评审建议 5）**：本类原本被标为"③"（与 §11.5.3.4 的"断言不得改"重号），且其中**两个用例实为 ①（fixture 订正）**。现：本类改号为 **④**；`validate_rejects_slave_out_of_range` 与 `validate_rejects_zero_interval` **移入 §11.5.3.2 的 A15/A16**（见该表与下方"语义漂移"注）。
>
> **再订正**：实现期实测发现本类原列的 10 例中另有 **5 例的 fixture 亦须订正**（与 A15/A16 同因：**前置规则截胡 ⇒ 假绿**，`is_err()` 虽绿却测不到目标规则）⇒ `validate_rejects_duplicate_id`、`validate_rejects_multiple_meter_grid`、`validate_rejects_multiple_battery`、`validate_rejects_same_port_mixed_baud`、`validate_rejects_same_port_mixed_baud_3_station` **移入 §11.5.3.2 的 A17–A21**（**断言仍不变**）。**本类余下 5 例"整例无需改动"的结论不变。**

本类**余下 5 例**（`validate_rejects_empty_id`（`:449`）、`validate_rejects_empty_port`（`:460`）、`validate_rejects_meter_grid_slow_poll`（`:545`）、`validate_rejects_zero_baud_rate`（`:830`）、`validate_rejects_zero_baud_same_port_all_zero`（`:842`））—— 断言均为 `is_err()`，fixture 亦无需改，**整例无需改动**。理由：它们的目标规则（`id`/`port` 非空、`interval` 下界、`baud` 下界）**都落在"站级基础校验"阶段**（§11.5.1「顺序」），**先于**规则 3/4（`regs` 相关）执行 ⇒ 不存在被截胡的假绿。（原列表中的 `validate_rejects_slave_out_of_range`、`validate_rejects_zero_interval` 已移出本类见 A15/A16；上述 5 例见 A17–A21。）

> **须登记的一处语义漂移（不是缺陷，但要写下来；已将其从"口头要求"落成 ① 的 A15/A16）**：`validate_rejects_slave_out_of_range`（`:520`，battery、无 `regs`）与 `validate_rejects_zero_interval`（`:534`，battery、无 `regs`）在实现新规则后**会先被规则 4（`soc` 点契约）拒**，而**不再由原本要测的那条规则拒**（`is_err()` 仍成立 ⇒ 用例仍绿，但已测不到 slave 下界 / interval 下界）。这不是缺陷，但属"测试通过却没测到"的隐患 ⇒ 实现期按 **A15/A16 补上合法 `regs`** 使二者真正测到目标规则；另**新增**两个用例（`validate_rejects_battery_without_soc_point` 已含在 C6；`pcs` 下界 `validate_rejects_pcs_interval_below_500` 对应规则 18）。**"补 regs"属 ① fixture 订正，断言仍不得改**。

### 11.6 字节序 / 字序 / 特殊编码（含 AC-2 算式复算）

| 场景 | 配置 | 解码链 | AC-2 期望值（逐位复算） |
|------|------|--------|--------------------------|
| BMS `soc`（118） | 块 `uint16`/`scale 1.0` + 点 `name: soc` | 1 寄存器 → u16 → ×1.0 | raw `0x0041`(65) → **65.0 %** |
| BMS 簇组电压（115） | `uint16`/`0.1` | 1 寄存器 → u16 → ×0.1 | raw `0x1403`(5123) → **512.3 V** |
| BMS 簇组模块温度（117） | `uint16`/`1.0`/`offset −40.0` | 1 寄存器 → u16 → ×1.0 + (−40) | raw 65 → **25.0 ℃** |
| BMS 116 簇组电流 | `uint16`/`0.1`/`offset −1600.0` | 1 寄存器 → u16 → ×0.1 − 1600 | raw 16000 → **0.0 A**；raw 15000 → **−100.0 A**；raw 0 → **−1600.0 A**；raw 65535 → **+4953.5 A**（判别点，与 `int16` 的 −1600.1 **必须不同**） |
| PCS `soc` 转述（1010） | 块 `byte_swap: true` | swap(0x4100) = 0x0041 → u16 → ×1.0 | 注入 `0x4100` → **65.0 %** |
| PCS 交流累计充电电量（1042–1043） | `int32_scaled`/`0.1`/`byte_swap`/`word_order: lo_hi` | swap 后 `[0x0064, 0x0001]` → lo_hi 拼 → 65636 → ×0.1 | 注入 `[0x6400, 0x0100]` → **6563.6 kWh**（误用 `hi_lo` 得 655360.1 → 必须不等） |
| ADL400 A 相电流（0x0064） | `uint16`/`0.01` | 1 寄存器 → u16 → ×0.01 | raw 946 → **9.46 A** |
| ADL400 组合有功总电能（0x0000） | `int32_scaled`/`0.01`（无 swap/字序） | hi_lo → (0×65536+12326) → ×0.01 | `[0x0000, 0x3026]` → **123.26 kWh** |
| 消防火警状态（9） | `uint16`/`1.0` | 1 寄存器 → u16 | 2 → **二级火警**（枚举语义，非位） |
| 消防探测器数据 1（13） | `uint16`/`1.0` | 1 寄存器 → u16（**整字**，G-6 延后） | `0x4150` → telemetry 落 **16720**；展示层拆解得烟雾 6.5 dB/M、温度 25 ℃（§11.10） |
| 空调 30001 / 30004 | `int16`/`0.1`；`uint16`/`0.1` | 1 寄存器 → i16/u16 → ×0.1 | 258 → **25.8 ℃**；602 → **60.2 %** |
| 位点（`hvac_di_1`） | `discrete`,`addr 0`,`count 31` | `unpack_bits(bytes, 31)[0]` | 首字节 `0x3B` → 位 0/1/3/4/5 = 1、位 2/6/7 = 0 |

> **换算的符号性口径（PRD v1.5 重裁定，设计不得回退）**：`offset` 只表示**零点平移**，与"raw 如何解释为整数"**无关**；`uint16`/`int16` 与任意 `offset` 的组合**一律放行**（BMS 的 116/117/122/127/129/155/157/186/2991–2994 全部是"无符号编码 + 负偏移"）。校验器**只校验"可追溯"与"与点表一致"**，不校验"符号性对不对"（后者靠 §9.5 的"类型来源"栏 + Q-20 现场判别 + RC-1）。

### 11.7 消费链路

#### 11.7.1 分流总表（对齐 PRD §9.6.1 / §10.5，**零语义变更**）

| 站 | 数据 | 通道 | 是否推进 5s 控制闸门 |
|----|------|------|----------------------|
| `battery` | 点名 `soc`（118） | `on_battery_soc` → `AiIntegrator::set_battery_soc`（BMS 优先源，超期回落核间，04 §2.11.1） | **否** |
| `battery` | 其余 344 点（含 288 位） | `on_station_telemetry`(is_event=false) → storage `telemetry`（device_id = 站 id） | 否 |
| `battery` | 告警族位的 0→1 跳变 | `on_station_telemetry`(is_event=true) → storage `events` + `AlertFeed` | 否 |
| `pcs` | 3 区 72 点 | `on_station_telemetry` → telemetry + 健康/校核展示 | 否 |
| `meter_batt` | 40 点 | telemetry + 能量流核算 | 否 |
| `fire` | 127 点 + **字级信号**（§11.7.2）+ 登记数不一致 + 探测器地址升序违规（**后两条属"状态型事件"，按状态翻转产出，见 §11.4.7.2**） | telemetry（整字原值）+ **events**（信号进入/退出活跃 + 登记数/地址序异常）；火警/联锁融合维持 §10.4（**停机仅以 DI3 触发**；**地址序/登记数告警不得并入 OR 触发**，§11.4.7.2 E） | 否（**仅事件/联锁判据，非控制量**，PRD §9.6.1） |
| `hvac` | 34 点 | telemetry + events（告警位） | 否 |
| `grid_meter`（既有） | 6 点 | `on_grid_package` → AiIntegrator + IEC104 | **是（唯一）** |

#### 11.7.2 位点落库与事件（D2 口径的落地 + 消防信号落点）

1. **每轮**在内存中形成完整点位（`mapper::telemetry_points` 返回全部 288+31 位），供即时展示/判据使用；
2. **落库**：位点仅在与上轮不同时经 `on_station_telemetry(is_event=false)` 落 `telemetry`（稳态≈0 行/s）；
3. **事件**：`BitClass::Alarm` 的 **0→1** 跳变 → events（metric = 点位名，如 `bms_alarm_225`）；`Reserved` 位（如 364–423、484–599）**只产 telemetry、不产事件**（PRD §9.7.6）；`State` 位同理只落 telemetry；
4. **事件可读性**：`SouthSink` 组装 `SystemEvent.message` 时可选查 `mupc_southd::point_table::label(role, metric)` 补中文名（如 **`bms_alarm_225` → "簇一级告警"** —— 按 §9.7.4/§9.5.1B，点名序号 225 = 位偏移 224 = **位地址 424 = 簇一级告警**；**勿**与位地址 225（"簇 SOC 低·轻"）混淆 —— 位地址 225 对应的**点名**是 `bms_alarm_26`，此处示例曾把两者串了、已订正），查不到则用原名（**不引入新的事件类型枚举**，沿用 `south_station.<站id>.<metric>`）；
5. **SOC 越界事件**（PRD §9.6.3）：metric `soc_out_of_range`，`value` = **原始寄存器解码值**（`SouthSink` 侧把 `is_event` 且非 offline/online 的点的 `value` 写进 message，作为"原始值落证"）。

**消防字级信号（P0-3 的落地；机制见 §11.4.7.1）**：

6. **信号来源与检测层**：消防全部状态量在**保持寄存器整字**内（`fire_sys_1/3/4/5/6/9` 等）⇒ 由 `scheduler` 每轮在**整字原值**上做 **`字 & mask`（`WordBit`）/ `字 ∈ active`（`WordEnum`）** 判定，与离散位共用同一个 `EdgeTracker` 与同一条事件产出路径。**telemetry 仍落整字原值**（值不变、可回溯），事件**不改写**任何 telemetry 值；
7. **产事件的信号清单**（逐条对表 PRD §9.5.4 的位定义表）：
   - **系统状态（addr 4）**：`main_power_fault`(bit14) / `backup_power_fault`(bit13) / `drive_circuit_fault`(bit11) / `pressure_sensor_fault`(bit10) / **`spray_fired`(bit8)** / `valve_open`(bit9) —— 前 4 条为故障类；**`spray_fired`/`valve_open` 为"灭火动作已发生"的留痕**（PRD 明确要求列举"喷洒标记"），文案由 `label` 区分（"喷洒标记置位"而非"故障"）；`work_mode_manual`(bit15)、`charging`(bit12)、备电电量 bit7–0 只落 telemetry（普通状态量，无事件消费方）；
   - **烟/温/可燃状态（addr 6/7/8）**：各产 `smoke_trigger` / `temp_trigger` / `combustible_trigger`，`mask = 0b11`（bit1 复合探测器触发 + bit0 干接点触发）—— **bit2 点型探测器"预留未启用"不纳入 mask**（PRD §9.7.6 明文"不产出告警事件"）；
   - **火警状态（addr 9，枚举）**：`level1`(值 1) / `level2`(值 2) / `emg_start`(值 4) / `emg_stop`(值 5)；值 3 预留不入 `active`；**值 0 工作正常 = 非活跃** ⇒ 任何活跃值回到 0 即"退出活跃"事件（联锁恢复的可观测点，§10.4"恢复需双方复位"）；
   - **探测器状态（addr 12）**：每只探测器产 `alarm`(bit12 报警总状态) 与 `fault`(bit14 故障总状态)；bit0–4 传感器细分**不产独立事件**（取舍见 §11.12.1 项 6）；
   - **不产事件的消防点**：钢瓶气压（addr 5，PRD §9.7.6）、探测器地址/数据 1/CO/VOC/H2（无阈值口径）、复合探测器 `comm_offline`(bit15) 与反馈位（极性相反，不猜 —— 仅展示）；
8. **事件名**：`<遥测点名>@<信号键>`（例 `fire_sys_1@spray_fired`、`fire_sys_6@level2`、`fire_sys_9@alarm`）—— **属事件命名空间，不新增遥测点、不占用 telemetry 命名**（§11.12.1 项 5）；
9. **探测器地址升序违规（Q-9）**：`fire_detector_addr_order_violation` 命中 → 按**状态翻转**产事件（**口径见 §11.4.7.2**）：进入 = `("fire_detector_addr_order_invalid", 首个违规探测器的 1 基序号, true)`、恢复 = `("fire_detector_addr_order_invalid@recovered", 0.0, true)`、**状态未变的轮次不产**（**不是"命中即产"**）、**首次观测即产**；且**每一轮**（不论是否产事件）**该轮探测器区点位标记为不可信**（telemetry 照落原值）——**事件去重与逐轮"不可信"标记是两件事，互不影响**。升序链的**链首 = 寄存器 11**（探测器 1），见 §11.4.6。

10. **（登记项，**不属**上面的消防信号清单）`offline` 事件的块级 `reason` —— 消费方可见变更**：`SouthSink` **覆写** `on_station_offline`，把 scheduler 组装的 `reason`（含 `slave/addr/count`，§11.4.7/§10.7 的站失败信息）写进事件 `message`，使 **PRD §9.7.2 第 1 条**（"站进入 offline，事件 `reason` 含 `slave/addr/count` → 运维据 `events` 定位到具体块"）**真正可用**（默认实现只发 `("offline", 1.0, true)`，`reason` 被丢弃）。**与默认实现逐字一致的部分（刻意保持，防消费方被动受影响）**：事件类型仍 `south_station.<站id>.offline`、等级仍 `warning`、**事件去抖仍归 scheduler**（本层不重复去抖 ⇒ **不会多产** offline 事件）。**唯一差异是文案**：既有文案（`站 <站id> role=<role> 离线（采集失败）`）**追加 `：{reason}` 后缀**。**`online` 的文案逐字不变**（`站 <站id> role=<role> 恢复上线`，info 级）。
   > **属消费方可见变更**：若有消费方 / 文档 / 告警规则 / 前端按 `offline` 文案**逐字**匹配，须同步核对（`online` 无需处理）。**T6 已落地，本项只作登记，提示 T7 时留意**（§11.13 T7 行）。

#### 11.7.3 展示与存储口径

- 位点"长时间未变" ⇒ 最后一条 telemetry 时间戳陈旧，展示层按"最后变更时刻 + 现势内存值"呈现，**不得**把陈旧时间戳当作"该位当前为 0/1 的证据"（与 §10/§9.7.6"保留上一有效值并标记 stale"同口径）。
- 新增模拟量点 ≈ 299 行/轮（5 站合计 ≈ 300 行/s），与既有 `meter_grid` 同阶；retention 天数复用既有配置，投运前按盘容量复核（§11.12.3 待决项）。
- **消防钢瓶气压"未配置"口径（PRD §9.7.6 的设计落点）**：`fire_sys_2`（addr 5）在**本站生命周期内从未出现过非 0 值** ⇒ 展示层标 **"未配置"**，**不得**显示 "0 kPa"、**不得**据此判"气压异常/泄漏"（`cylinder_pressure_configured`，§11.4.6）。该"从未非 0"事实由 scheduler 每站一个 `bool` 记忆维护（初值 false，任何一轮读到非 0 即置 true 且此后不再回退 —— 钢瓶气压不会在业务上"变回未配置"）。**注意**：该点**不产任何事件**（PRD §9.7.6 明令），故它不是"阈值判据"，只是**展示口径**。
- **消防探测器点位的"地址序有效前提"（Q-9 的展示侧落点）**：`fire_detector_addr_order_violation` 命中的那一轮，展示层对探测器区（`fire_det_*`）**不得**按"第 n 只探测器"呈现点位与物理编号的对应关系（升序前提被破坏 ⇒ 位置式点名与实物不再一一对应），应同时展示地址序异常告警；原始值仍可查（telemetry 照落）。

#### 11.7.4 配置迁移清单（生效配置从"1 站"到"6 站"）

| 项 | 动作 |
|----|------|
| `grid_meter` 站 | **逐字不动**（PRD §9.4.1 第 6 站与生效配置取值一致） |
| `bms`/`pcs`/`meter_batt`/`fire`/`hvac` | 按 PRD §9.4.1 的 6 站 YAML 替换注释占位；`pcs` 站**默认保持注释**，待 Q-15（A1/B1 是否隔离）现场裁定后启用（RC-8） |
| 旧 `battery` 站写法（`name: soc` 的**块**） | 必须改成"点名 `soc` 的点"（§11.4.6）；`deploy` 中的注释行按 §9.4.1 重写 |
| `fire` 站的两处"非默认"写法 | ① `fire_sys` 的 `at: 7` 声明 `name: fire_det_count`（PRD §9.4.1 参考配置已给，直接照录）；② 探测器区若现场需分片，分片块加 `read_slice: true` **并在注释登记理由**（PRD §9.4.2.4 第 ④ 条，RC-5 目视项） |
| **`fire` 站的准入门槛** | **写入生效配置前，必须先结清 §11.4.6 的「消防登记数」容量式订正（`1 + Σ(fire_det 前缀块 count)/6`）** —— 与 T5 的「**裁定前禁止把 `fire` 站写入生效配置**」（§11.4.7.2 引言）**同构**：容量式若仍按旧式 `Σ/6` 实现，`fire` 站一上线即 **`fire_detector_count_mismatch` 恒真 ⇒ 永久假告警**（首次观测 1 条 + 每次站恢复 1 条），且 **RC-5 的登记数交叉校验随之永久失效**（真增减被本底告警淹没）。结清 = 代码按 `1 + Σ/6`（含链首不可得时的 `Σ/6` 降级）实现并经用例钉住，再由本行放行 |
| `pcs` 站若按 RC-2 不配 4 个 32 位电量点 | 从 `pcs_3zone` 的 `points` 中删去 `at: 43/45/73/75` 四条（**删点不删块**：块窗口与其余点的序号锚定绝对地址，故删点**不构成改名**；但须在变更流程中登记）—— 与 §9.7.3"比对通过前 32 位电量视为不可信"一致。**⚠️ 本行同时是 PRD §9.8.4「比对未通过 → 事件」的替代口径（用"不配点"替代运行期事件），差异登记见 §11.12.2 Δ-9** |

### 11.8 写操作独立 Task 的设计边界与 S2 联锁分工

**本轮结构性保证"只读"**：`StationBus` trait **没有任何写方法**（§11.4.5 只加 `read_discrete`），scheduler/mapper 均无写路径 ⇒ PRD N-4（`pcs` 不写）、N-5（消防 1999 不被调用）、N-6（不改写设备参数寄存器）**由类型系统保证**，不依赖约定。PCS 4 区（1000–1057 与 500–503）与 BMS 1000+ 阈值、消防 1999、空调 40001–40059 **一律不配块**（不进 `regs`），避免"设定值被当实测值"（§9.5.1C）。

**写 Task 立项时须先结清的 4 件事（本设计只给边界，不给实现）**：

1. **PCS 写归属**：4 区 **500 = 模块启停**已被 S2 DI/DO 安全联锁用作**停机原语**（`intercore::ModbusRtuTransport` 持有，触发即 `500=0` 并**锁存禁启**）。若 `southd` 也写 4 区，将出现**两个写方争用同一停机原语**。故"并入 intercore 还是下放 southd"须**单独评审**后再立项；归属未定前 `southd` 对 PCS **一律只读**。
2. **写路径的仲裁**：写必须与读共用同一个 `Rs485PortBus`（同一 `bus_lock` + 同一 `tx_lock` 语义），否则口内请求会交错。
3. **控制动作的准入**：写指令必须经策略/AiValidator 链路（PRD N-5），不得由采集站直写；写操作须有"谁/何时/写了什么/回读确认"审计。
4. **与 S2 的分工（不得重叠）**：`southd` 负责**采集面**（`fire` 的状态位、`hvac` 的只读告警/温湿度、`pcs` 的只读 3 区）；S2 负责**安全联锁面**（DI 触发 → 停机原语 → 锁存）。两面的接口 = 既有事件通道（storage `events` + `AlertFeed`），**不新增直连调用**。消防的融合规则维持 §10.4（OR 取触发、停机仅 DI3、恢复需双方复位、去重键"消防+源"）。**补一处联接**：§10.4 的"**恢复需双方复位**"需要知道**消防侧是否已解除** —— 这正是 §11.4.7.1 给消防信号取**双向**（进入/退出活跃）的原因；`fire` 侧的"退出"事件（`fire_sys_6@level2` 等，`value = 0.0`）是融合判据**唯一**可观测的消防侧复位信号，故该不对称决策**不是风格选择，而是 §10.4 融合的必要输入**。
> **成组判读约束（消费侧澄清，`§10.4` 本体不改）**：上句的"退出事件 = 消防侧复位信号"**须按点名成组判读**，**不得**逐条独立解读 —— **同一轮内、同一 `<点名>` 下「退出 + 进入」= 级别跃迁（报警仍在，如 `level1 → level2`）**；**「只有退出、无进入」= 回到非活跃（值 0 工作正常）**，仅后者可作为"消防侧已复位"。理由与完整判据见 **§11.4.7.2 D**（`WordEnum` 跃迁时 `level1 = 0.0` 不表示"一级报警已解除"）。另：**地址序/登记数告警（数据可信性）不得并入 §10.4 的 OR 触发**（§11.4.7.2 E）。

### 11.9 Q-1（BMS 主从方向）的处理

> **状态（2026-09-22）：Q-1 已关闭** —— 用户确认**EMS 为主机、BMS 主控为从机**（PRD 侧登记见其 §9.10 Q-1 行 ⑤），与本节 §11.9.1 采用的基线口径**一致** ⇒ 原【设计阶段阻塞】**解除**。**本节保留 §11.9.2 的"相反口径影响面"与 §11.9.3 的可切换设计（E3）**，供厂方口径若反转时参照；**§11.11.3 的 RC-4 性质随之由"Q-1 的解除判据"变为常规投运核实**（与 PRD §9.9.2 RC-4 的口径对齐）。

#### 11.9.1 采用的口径（书面结论）

**本设计采用 PRD §2.2 口径：MUPC（EMS）为 Modbus 主机，BMS 主控模块为从机，由 `southd` 主动轮询**（`port: /dev/ttyS2`、`protocol: modbus`(RTU)、`slave: 1` = 簇号）。依据：① 与 `southd`"每口一 master、主动轮询"的调度模型一致（无需新增从站模式）；② 与 `slave` 字段、`interval_ms` 排程、退避降频语义自洽；③ 该口径是 PRD §9.5.1 点表映射与 §9.6.1 SOC 消费链的既定基线。

**本设计不擅自修改 PRD**：若厂方书面确认为相反口径，按 PRD Q-1 ④-b **须由需求侧重新走需求评审**（点表映射与 SOC 消费链），设计不代庖。

#### 11.9.2 相反口径的影响面（为何"双模式"不可行，以及真正会变的是什么）

若厂方确认"**BMS 主控做主机、EMS 做从机**"：

- **RTU 链路的数据面消失**：Modbus 从机（MUPC）只会被 BMS **读取**自己的寄存器，链路上**不存在** BMS → MUPC 的数据下行。因此① `battery` 站的采集路径**不存在**（不是"参数取几"）；② `on_battery_soc` 的控制链时序、5s 新鲜度窗口、退避降频语义**失去意义**；③ 由此可得结论：**"主站/从站双模式可切换"是伪选项**——真正可切换的不是初动方向，而是**接入路径**。
- **唯一可行的替代路径**：BMS 协议 §1.1 载明同时支持 **LAN**，§3.1 载明 TCP 形态（"主控模块做 TCP 服务器，后台监控主动连接"）。即相反口径下的替代方案 = **MUPC 作为 Modbus-TCP 客户端**连到 BMS 的 TCP 服务器（数据由 MUPC 主动读，与"MUPC 轮询"的消费语义相同，只是传输换成 TCP）。
- **影响面清单（切换代价）**：
  | 维度 | 影响 |
  |------|------|
  | 传输层 | 新增 `protocol: modbus_tcp` 站点类型（`StationConf.protocol` 字段已在，但 `StationBus` 需新增 TCP 客户端实现：连接管理 + 重连 + MBAP 头 + 与串口的超时/退避语义对齐）→ **≈1 个新模块 + 一套连接生命周期测试** |
  | 采集面 | 点表、块划分、点位名、解码**完全复用**（本设计的 `points`/`RegDecode`/`point_table` 与传输无关）→ **零改动** |
  | SOC 链 | 若采用 TCP 客户端，消费侧**零改动**（见 §11.9.3）；若不采用（无 LAN 施工条件），则 BMS 站的 SOC **不可得**，SOC 只能回落核间（PCS 侧转述值 1010 **不得**作为控制源——N-1；若要启用，须重新评审） |
  | 新鲜度 | TCP 轮询语义与 RTU 相同（仍是轮询），5s 窗口/`interval_ms < 5000` 约束不变；若退化为"BMS 主动上报"，则须重新定义新鲜度与丢报判据（**属需求变更，非设计可裁**） |
  | `slave` 字段 | RTU 从机地址语义失效（TCP 用 Unit ID，缺省 1），配置字段保留、语义转为 Unit ID |
- **切换判据与时点**：判据 = **RC-4**（按 §2.2 口径实测，连续 ≥1h 无离线）；时点 = **PCS 站启用前**（Q-15 同理）。判据不成立 → 立即转厂方书面确认，按 PRD Q-1 ④ 分支处置。

#### 11.9.3 让口径切换不牵动消费侧的设计（E3 的落点）

把"SOC 供给"从"某站必须轮询"解耦为"**站内有点名为 `soc` 的点**"：

- 消费契约（PRD §9.4.3）锚定在**点名**而非块名/站类型；
- scheduler 只在 `role == Battery` 时推 `on_battery_soc`（当前唯一供给方）；
- 若将来供给方换成 TCP 客户端站，只需该站仍声明 `soc` 点名，`scheduler`/`SouthSink`/`AiIntegrator`/策略**均不需改**。

> **结论**：设计**不阻塞于** Q-1 的书面答复即可开工实现（按 §11.9.1 基线），但 **RC-4 与 PCS 站启用前必须结清**；本设计已按要求给出可切换方案（E3）与切换代价（§11.9.2）。

### 11.10 G-6（寄存器内字节拆分）延后口径与展示层拆解边界

**本轮不做**（PRD §9.4.2.3，配置契约**不含 `slice` 字段**），替代口径 = **整字采集原值 + 展示/事件层拆解**：

| 点 | 采集 | 展示层拆解（**仅展示，不作判据**） |
|----|------|-----------------------------------|
| BMS 位置编号 124/126/128/130（`bms_io_25/27/29/31`） | 整字 u16 原值 | **不拆**——原文 Bit15–8 与 Bit7–0 都标"PACK 编号"，低字节语义自相矛盾（Q-18）⇒ 拆解结果**不得**作为判据、也不建议展示"哪个单体" |
| 消防探测器数据 1（`fire_sys_10` / `fire_det_*`） | 整字 u16 原值 | 可拆：烟雾 = `(raw >> 8) × 0.1` dB/M；温度 = `(raw & 0xFF) − 55` ℃（原文语义无歧义） |

**跨模块一致性靠什么保证（必须有落点）**：拆解公式**不在南向代码里**，而是由 ① PRD §9.5.1/§9.5.4 明文（唯一真源）+ ② `point_table` 的 `label` 与 §11.10 表格（展示侧可直接引用本节的公式与示例值 `0x4150 → 6.5 dB/M / 25 ℃`）+ ③ 展示层单测复算 AC-2 的同一个例子共同保证。**南向侧只保证"原值不丢、不改、可回溯"**。

**与"消防事件层"的边界（回应 P0-3 的"是否与 G-6 冲突"）**：不冲突，因为两件事不同、代码也不同 ——

| 层 | 处理对象 | 手法 | 产物 | 本轮是否做 |
|----|----------|------|------|-----------|
| **遥测层**（southd `telemetry_points`） | 整字 | **不拆** | telemetry 落整字原值（`fire_sys_1/6`、`fire_det_*` 的 `value` = 0–65535） | 做（口径不变） |
| **事件层**（southd `scheduler` + `EdgeTracker`，§11.4.7.1） | 整字内的**位/枚举** | `字 & mask` / `字 ∈ active` —— **只有常量掩码与活跃集，没有"字节切片"逻辑** | 事件（`点名@信号键`），**不产出新遥测点、不改写原值** | 做（本轮新增） |
| **展示层** | 整字内的**字节**（唯一：探测器数据 1） | `raw >> 8` / `raw & 0xFF` − 55 | 界面/报表的"烟雾 dB/M、温度 ℃" | 做（不在南向仓库） |

> 即：**G-6 延后的是"把整字拆成两个遥测点"**（避免配置/带宽爆炸与语义不可信），而**事件层用的是位/枚举语义** —— 位定义表（PRD §9.5.4）是厂方**无歧义**给出的，与 G-6 的两个"不可可靠拆"候选点无关。**唯一涉及字节拆解的点（探测器数据 1）在事件层被显式排除**（它不产事件），故三层零重叠。

### 11.11 测试策略（AC / RC 在设计层的落点）

#### 11.11.1 可测接缝（三层，逐层可独立断言）

| 层 | 接缝 | 能断言什么 |
|----|------|------------|
| L1 纯函数 | `RegDecode::decode` / `unpack_bits` / `points::expand` / `validate_*` / `soc_in_domain` | AC-2 的**全部数值**、AC-3 位序、AC-1 ③ 逐条拒绝、AC-6 点产出/命名 |
| L2 总线注入 | `MockBus::{put, put_input, put_bits}` + `fail_*_once` + `calls` | AC-2 的**端到端**解码（canned 寄存器 → 点位值）、AC-4 隔离/退避、AC-5 分发 |
| L3 装配 | `SouthScheduler::new(cfg, buses, FakeSink)` + `tick_once`（既有测试驱动） | 站级分发（`on_battery_soc`/`on_grid_package` 是否被调）、位点/信号变化沿（含**消防字级信号进入与退出活跃**、预留位零事件、恢复后不刷事件）、SOC 越界事件、消防登记数与探测器地址序事件 |

#### 11.11.2 AC 逐条落点

| AC | 用例落点 | 要点 |
|----|----------|------|
| **AC-1** | `mupc-southd/tests/s3b2_config.rs` + `tests/fixtures/south_stations_s3b2.yaml`（= PRD §9.4.1 六站 YAML） | ① **既有字段子集**（剥离 `parity`/`discrete`/`byte_swap`/`points`/`offset`/`word_order` 后）解析通过 + 逐条核对既有拒绝条件不触发（其中 `grid_meter` 逐字一致 ⇒ 必过）② 完整 YAML 解析通过且除 §9.4.3 明列条件外无拒绝 ③ **§9.4.3 的 17 条 + 设计补落点的 2 条（规则 18/19）逐条**各一个最小坏配置 → `Err`。**必含的边界用例**（上轮评审打回的直接对应项）：<br>• 规则 15 —— **正向**：`tests/fixtures/south_stations_s3b2.yaml`（六站）与**仅含 `grid_meter` 六相量块**的配置均 `Ok`（**"六站必然通过第 15 条"的可执行断言**，见 §11.5.2(2)）；**反向**：两个**都声明 `points`** 的相邻块、合并后 `count ≤ 120` 且无 `read_slice` → `Err`（**①②③④ 四条合取全成立**；③ 在适用域内恒成立，见 §11.5.2(1)）；同一对块任一方标 `read_slice: true` → `Ok`；两块合并后 `count > 120` → `Ok`（fire 形态：127）；**相邻的一方未声明 `points`** → `Ok`（grid_meter / fire 形态）；**两块都未声明 `points` 且严格相邻**（grid_meter 形态）→ `Ok`；**`discrete` 块相邻** → `Ok`（保守读法，Δ-8）<br>• 规则 15 的**判据 ③（合并后空洞 ≤ 4）**不另立"放行"用例 —— **v1.5 勘误**：原写"单列一个用例、断言 `Ok`"与 §11.5.1 #15 / §11.5.2 的**合取判据**相左（**③ 不是独立放行条件**）。正确形态：构造两个**各自合法（含块内 ≤4 空洞）、地址连续（`b.addr == a.addr + a.count`）且均声明 `points`** 的块，合并后 `count ≤ 120`、无 `read_slice` ⇒ **①②③④ 全成立 ⇒ `Err`**（与上一条反向用例**同判**，可作为其**变体**：块内各带 ≤4 空洞）；该用例的价值在于**拒因文案"应合并"**：`Err` 由 ①②④ 给出、③ 恒成立 ⇒ 证明"③ 的加入不引入额外拒绝"（§11.5.2(1) 等价性证明的钉子）<br>• 规则 6 —— ① 命中行而 `sym_src` 空 → `Err`；② `offset` 与登记值不等 → `Err`；**③ 负向：`addr` 改基准（RC-3 形态）使 `lookup` 无行、且 `offset ≠ 0` → `Ok`（"无行不拒"的可执行断言）**<br>• 规则 18 —— `pcs` 站 `interval_ms: 499` → `Err`、`500` → `Ok`；规则 19 —— 32 位块 `count: 3` 且无 `points` → `Err`、`count: 4` → `Ok`、`discrete` 块 `count: 31` → `Ok`<br>• **规则 16（同口一致性）的 `parity` 一侧（D-1 裁定按 ①，该判据与本 AC 触发项**保留有效**）**：同口异 `parity`（`even` vs 缺省 `none`）→ `Err` 含 `parity`；同口同 `parity` → `Ok`。**可执行用例 = `mupc-southd/tests/s3b2_config.rs::ac1_rule16_same_port_parity_consistency`（`:698`，T3 已落地）**，实现落点 = `config.rs` 跨站一致性段（`:308-327`，与 `baud_rate` 同一道防线）。**注**："`parity` 判据有对象"以**本裁定（①）**为前提 —— 若取 ②（不加字段）则该项与本 AC 触发项将**失去对象**（`baud` 一侧仍可验）；**该前提未发生，故此处不删不改**（D-1 裁定为 ①，该回写不适用；规则 16 见 §11.5.1，判据保留有效）<br>• **规则 10（点名唯一）—— 补一个"只有它能拒"的形态**：两个**块名不同**、展开后 `metric` 相同的配置（如两块各声明 `points: [{ at: 1, name: soc }]`，或一块的显式 `name` 与另一块的自动位置点名相撞）→ `Err` 含"点名重复"。**注意与 `meter_grid` 块名唯一的分工**：同名块（两块都叫 `p`）由**既有块名唯一规则**先拒（文案"块名重复"，见 §11.5.3.4.1），本用例**不得**用它退化替换 |
| **AC-2** | L1 `meter_regs`/`points` 表驱动 + L2 `tests/s3b2_decode_e2e.rs` | 逐设备抽样，期望值取 §11.6 表（全部已回原文复算）；**必含** BMS 116 raw=65535 → +4953.5（与 `int16` 的 −1600.1 不等）、PCS 32 位 `lo_hi` 6563.6（与误用 `hi_lo` 不等） |
| **AC-3** | L1 `rs485-plugin`（`unpack_bits` 单测）+ L2 点位名映射 | 288 位逐位比对（含跨字节边界、非 8 倍数尾部）、31 位（末字节高位不污染）、`bms_alarm_225` = 位 424 = 簇一级告警、`hvac_di_1` = 位 0 |
| **AC-4** | L3 scheduler | `pcs` 站超时 → 该站 offline + 事件、同口其它站不受影响；退避 `interval << min(n−1,5)` 封顶；恢复 online 一次；`role_priority(Pcs) = 1`（不与 grid/battery 同档） |
| **AC-5** | L3 scheduler（`FakeSink` 记录调用） | battery 注入合法 SOC → `on_battery_soc` 被调且为百分数；注入越界（65535）→ **不调** + 事件 + telemetry 保留原值；`pcs` 站任何输入 → 不调 `on_battery_soc`/`on_grid_package`；`meter_grid` 之外不推进闸门（**已订正 —— 等价落点，非 core-bin 直接断言**：① scheduler 侧 `pcs_station_never_triggers_grid_or_soc_channels`（`scheduler.rs:2542`）/ `non_battery_role_never_triggers_soc_channel`（`scheduler.rs:1543`）断言 `on_grid_package` **不被非 grid 站调用**；② core-bin 侧 `on_grid_package`（`startup.rs:378`）→ `AiIntegrator::set_latest_data` 是 `last_data_ts` 的**唯一写入路径**（`ai_integration.rs:212`；`on_station_telemetry`/`on_battery_soc` 均不触碰）⇒ 闸门**结构性不可被非 grid 站推进**。裁定理由与"不补 core-bin 断言"的论证见本节等价落点论证）。<br>**事件侧扩展（承接 PRD §9.6.1 的 `fire` 行"events + SSE"，PRD 的 AC 表未单列，属需求承接、一并上报）**：<br>• **消防字级信号**（§11.4.7.1）—— 注入 `fire_sys_1 = 0x4400`（bit14 主电故障 + bit10 压力传感器故障）→ 恰产 **2** 个事件（`fire_sys_1@main_power_fault`、`fire_sys_1@pressure_sensor_fault`），**不产**其它位的事件；下一轮注入 `0x0000` → 恰产 **2** 个"退出活跃"事件（`value = 0.0`）；**首轮只建基线、不产事件**；站从 offline 恢复后首轮同样**不产**事件（`reset()` 生效）<br>• **枚举跃迁**：`fire_sys_6`（火警状态）= 0 → 1 → 2 逐轮注入 → `0→1` 产 `level1`（进入）；**`1→2` **同轮**产 `level1` 退出（`value = 0.0`）**+** `level2` 进入（`value = 1.0`）**（活跃值间跃迁）；`2→0` 产 `level2` 退出。**并钉住消费方成组判据（§11.4.7.2 D）**：`1→2` 那一轮的 `level1 = 0.0` **不得**被判为"一级报警已解除"（该轮存在同点名进入事件 ⇒ 属**级别跃迁**）<br>• **预留位不产事件**：`fire_sys_3 = 0x0004`（bit2 点型，预留未启用）→ **零事件**（PRD §9.7.6），但 telemetry 仍落 4<br>• **探测器总状态**：`fire_sys_9`（探测器 1 状态）bit12 → `fire_sys_9@alarm`；`fire_det_2`（探测器 2 状态，整字）= 0x4000（bit14）→ `fire_det_2@fault`<br>• **不产事件**：`fire_sys_2`（钢瓶气压）任何取值（含恒 0）→ 零事件；探测器 bit0–4 传感器位 → 零事件<br>• **地址序（Q-9，按 §11.4.7.2 的状态翻转口径 + §11.4.6 的链首订正）**：注入**探测器 1（寄存器 11）地址 3、探测器 2..n 的 `+0` 为 4,5,…（严格升序）** → **不产**；注入**非升序**（如探测器 2 的地址 ≤ 探测器 1 的地址）→ **首轮即产 1 条** `fire_detector_addr_order_invalid`（**首次观测即产**，`value` = 首个违规探测器的 1 基序号）；**其后各轮沿用同一非升序值 → 一条都不产**（**去重的可执行断言**，即"5 轮 = 1 条"而非"5 轮 = 5 条"）；**改回严格升序 → 产 1 条 `fire_detector_addr_order_invalid@recovered`（`value = 0.0`）**；**再次违规 → 再产 1 条进入事件**。**链首覆盖用例（防"探测器 1 不在链里"回归）**：探测器 2..n 各自升序、但**探测器 1 的地址最大**（如 寄存器 11 = 9，探测器 2..n = 2,3,4）→ **必须产**（`value = 2`，即"第 2 只的地址 ≤ 第 1 只"）；**且每一轮（不论是否产事件）探测器区点位标记不可信照旧生效**<br>• **位块仍只上升沿**：BMS `bms_alarm_*` 注入 1 → 0 → **不产**"退出"事件（与消防双向形成对照，钉住 §11.4.7.1 的不对称决策）|
| **AC-6** | L1 `points::expand` 表驱动 | ① 无 `points` 块按"每值槽 1 点"（`bms_term` → 4 点、`bms_alarm` → 288 点）② 有 `points` 块只产列出点（`bms_meta` 8 点，188/190 不产）③ 点级 `count: N` 连续产出（`bms_io` `at:8,count:8` → `bms_io_8..15`）④ 点级 `name` 覆盖（`soc`、**`fire_det_count`**）⑤ 32 位点占 2 寄存器产 1 点（`pcs_3zone` 76 → **72 点**，`pcs_3zone_43` = 1042–1043）⑥ 单寄存器点必产 1 点 ⑦ **无 `points` 的 32 位块产 `count/2` 点**（`count: 6` → 3 点，即 §11.5.3.3 第 8 条的 mapper 用例）|
| **AC-7** | 工程门禁 | `cargo build --release` 无警告 / `cargo clippy` 无 Error / `cargo test` 全绿 / 格式：**本 Task 新增或修改的行 fmt-clean**（口径见 §11.11.2.1 —— **行级**判据，**不含**项目既有基线；**禁止**执行 `cargo fmt --all`） |

##### 11.11.2.1 AC-7 的 fmt 口径（把不可执行的仓库级命题收敛为行级可判定命题）

**背景（为什么原文的"`cargo fmt`"不可执行）**：AC-7 原文把"`cargo fmt`"与 build/clippy/test 并列，隐含前提是"仓库整体格式正确"。**该前提在本仓库不成立**：S3b-2 实施期实测 `cargo fmt --all --check` = **1204 处**不符合，分布在 **106 个文件**，集中在 `local-display/src/ui/tests.rs`（221）、`mupc-core-bin/src/log_service.rs`（89）、`mupc-core-bin/src/console_host.rs`（66）、`mupc-southd/src/scheduler.rs`（22）等。这是**项目既有基线**（自 S3a 及更早累积），**不是 S3b-2 引入的回归** —— 用"全仓 fmt 变绿"当验收条件，等于把 1204 处历史债算进本特性。

**更关键的约束（为什么不能"跑一次全仓 fmt 了事"）**：本项目**明令禁止**执行 `cargo fmt --all`。理由有二：① 一次性全仓格式化产生 1204 处**与本特性无关**的改动，会把真实 diff 淹没到不可评审（本项目已有"无差别暂存导致回归"的事故先例，见根 `CLAUDE.md` 的提交规范）；② 工作区存在大量与本特性无关的既有格式差异，全仓格式化的结果**不可归因**到本特性。**故 AC-7 的 fmt 部分必须给出可执行、可验证的替代口径，而不是"豁免格式检查"。**

**订正后的判据（AC-7 的 fmt 部分以此为准）**：

| # | 判据 | 验证方式（可执行、可复现） |
|---|------|--------------------------|
| **① 不得新增**（**主判据**） | **本 Task 新增/修改的行** fmt-clean：**"实际不符合行"集合 ∩ 本次变更的新增行集合 = ∅** | ① 对**本 Task 触及的 `.rs` 文件**（**工作区**版本）跑 `rustfmt --edition 2021 --check <file>`；② 从输出取**带 `-` 的行**（= `rustfmt` 要替换掉的**当前文件内容**，即**真正不符合**的行）：以每个 `Diff in <file>:N:` 的 `N` 为该 hunk **首行**在**工作区文件**中的行号，**逐行推进** —— **context 行与 `-` 行各占一个行号，`+` 行不占**（`+` 行只属于格式化后的结果）⇒ 得集合 `R`；③ `R ∩ git diff <Task 起点>..HEAD -- <file>` 的**新增（`+`）行号集合** ⇒ **为空即通过；非空即给出具体 `file:行`**（可复核、可定位）。**⚠️ 两条不得省**：(a) **不得**拿 hunk **锚点行号** `N` 当"不符合行" —— 锚点行常是 format-clean 的 **context 行**（实测 `tests/s3b2_config.rs`：锚点 `:374`，**实际不符合行 = `:377`**），且**锚点数 ≠ 不符合行数**（该文件 **27 个 hunk / 34 条不符合行**，实测）；(b) `R` 与新增行号**必须是同一坐标系** —— `rustfmt --check` 读的是**工作区（新）文件**，其行号即 `git diff` 的**新文件**行号（**不得**与旧文件行号混用，否则有增删行时整体错位） |
| **② 纯新增文件零报出（收敛：判据 ① 的推论，非独立判据）** | 本 Task **新建**的 `.rs` 文件全部行均为新增行 ⇒ 判据 ① 退化为"该文件 **0 处**报出"。**适用对象限定（W1 裁定）**：仅适用于 **fmt 行级口径订立后（T4 起）各 Task 新建**的文件，时间基准点 = 新建该文件的 Task 的 diff 起点；**T1–T3 期间新建/修改的文件不适用本条追溯**，其不符行一律按下方「S3b-2 格式债基线清单」登记、按判据 ③ 处理 | `rustfmt --edition 2021 --color never --check <新文件>` → 无输出 |
| **③ 既存基线一律不动** | 本 Task **未触及**的文件、以及文件内**未被本 Task 修改**的既有行，其既存不符合**一律不处置**（**不得顺手格式化**） | 不适用（**禁止** `cargo fmt --all`；发现即视为超范围改动） |

> **为什么这是"可验证"而不是"豁免"**：判据 ① 把"格式正确"从**不可执行的仓库级命题**收敛为**行级可判定命题** —— 两条命令（`rustfmt --check` 输出的**实际不符合行（带 `-` 的行）**行号；`git diff` 的新增行号）取交集，**空/非空是确定的二值结果**，且非空时直接给出 `file:行`（可复核、可定位）。它既能被 T7 的闸门自动断言，也**不会**因基线 1204 处而恒假（原文口径的问题）、**不会**退化为"随便看看"。**代价（刻意接受）**：本判据不承诺仓库整体趋近 `rustfmt` 风格 —— 历史债的清理是**独立的格式债任务**（须单独立项、独立 PR，不与本特性混提），不在 S3b-2 的 AC 内。

**T4 的实测结论（截至 `5d46541`，供 T7 收口用 —— 行号口径按判据 ① = 输出中**带 `-` 的行**）**：
1. **判据 ② 满足**：`mupc-southd/src/point_table.rs`（T4 填实的点表主体）、`tests/point_table_vs_reference_config.rs`（**T4 新建**）、`tests/s3b2_decode_e2e.rs`（**T6-5 新建** —— **归属订正**：此前把它并列在"T4 实测"条目下，易被读成 T4 新建，评审以 `git log --diff-filter=A` 核实其**实由 T6-5（`fb925e7`）新建**；**只订正归属，"0 处报出"的结论不变**）均 **0 处**报出（复跑：三个文件带 `-` 的行数均为 **0**；其中 `s3b2_decode_e2e.rs` 的复跑发生在 T6-5 落地之后）—— 判据 ② 覆盖的三个**新增文件零报出**；
2. **判据 ① 违反 1 处（T4 引入）**：`tests/s3b2_config.rs`（T3 新建文件，T4 在其中补规则 6 ② 配置级用例）的 `assert_eq!` 中，链式调用 `mupc_southd::point_table::lookup(Role::Battery, 116).unwrap().offset` 长度超 `rustfmt` 的 `chain_width`（60）⇒ 该 hunk 的锚点为 `:374`，**实际不符合行（带 `-` 者）= `:377`**（实测复核，见 §11.11.2.1 判据 ① 的注意事项 (a)）。**由 T7 收口**（见 §11.13 T7）；
3. **判据 ③ 的实例（须区分，防误判为 T4 回归）**：同一文件 `tests/s3b2_config.rs` 整体另有 **26 个 hunk / 33 条不符合行**（按判据 ① 的 `-` 行计；**该文件合计 27 个 hunk / 34 条**），**全部由 T3 引入**（`git show 8db56df` 的既有行），按判据 ③ **不在本 Task 处置范围**。**这正是判据 ① 与 ② 并存的原因**：对"跨 Task 演进的文件"必须用**行级**判据（①），只有**纯新增文件**才适用文件级判据（②）（**② 的收敛说明**：② 为 ① 在纯新增文件上的**推论**且限定适用于 T4 起新建的文件，不是可追溯至 T1–T3 的独立文件级判据 —— 理由见本节判据 ② 行）。同理，T1–T3 若曾修改既有文件（如 `config.rs` / `points.rs` / `scheduler.rs`），其既存不符合属基线，不计入本特性。

**S3b-2 格式债基线清单（W1 裁定的登记落点，66 条不符行的权威源）**：

T1–T3（早于 fmt 行级口径订立）期间新建/修改文件的全部 `rustfmt` 不符行，**统一按判据 ③ 既存基线处理**，不计入任何 Task 的判据 ①。逐行 blame 归因（评审报告 2026-09-22 W1/O1 + `rustfmt --edition 2021 --color never --check` 复跑，2026-09-22）：**T3 引入 66 条；T4–T7 自身零条**（T4 曾引入 1 条 `s3b2_config.rs:377`，已由 T7 修净 —— 复跑该文件 33 条/26 hunk，与 v1.7 记录的 34 条/27 hunk 之差即此条）。

| 文件（`mupc-southd/`） | T3 归因不符行 | 文件当前不符行合计（2026-09-22 复跑） | 说明 |
|---|---|---|---|
| `src/points.rs` | **16**（8 hunk） | 16 | T3（`f83a838`）新建，全部 T3 引入 —— W1 的钉证文件 |
| `tests/s3b2_config.rs` | **33**（26 hunk） | 33 | T3 新建；T4 引入的第 34 条（`:377` chain_width）已由 T7 修净 |
| `src/config.rs` | **7** | 21 | 其余 14 条为 S3b-2 之前既有基线（同属 1204 处仓库债） |
| `src/scheduler.rs` | **6** | 25 | 其余 19 条为既有基线（与 22 处记录同源） |
| `src/mapper.rs` | **3** | 3 | 全部 T3 引入 |
| `src/port_runtime.rs` | **1** | 12 | 其余 11 条为既有基线 |
| **合计** | **66** | — | 与评审报告 O1「16 + 其余 50（33/7/6/3/1）」一致 |

**格式债待办（另立，指向 `docs/technical-debt.md`）**：上表 66 条应作为**独立的格式债任务**登记至 `docs/technical-debt.md`（单独立项、独立 PR 清理，**不与本特性混提**；登记动作归项目经理执行，本清单为权威源）。对照项：判据 ② 适用域内的三个新增文件（`src/point_table.rs` / `tests/point_table_vs_reference_config.rs` / `tests/s3b2_decode_e2e.rs`）复跑仍 **0 条**，条目 1 结论不变、不在基线清单内。

#### 11.11.3 RC 的代码/配置准备（设计侧已就位，现场只做裁定与回写）

| RC | 现场动作 | 回写项 | 设计侧准备 |
|----|----------|--------|------------|
| RC-1 逐点比对 | 每站 ≥10 点与设备显示/厂方工具比对（**必须点名** BMS 116/186，另加 ADL400 0x0092、BMS 2991–2994） | 不一致 → 改配置（**禁止**代码加设备特判） | `POINT_REGS` + `label` 即核对清单；`tests/point_table_vs_reference_config.rs` 保证表↔配置不漂移 |
| RC-2 PCS 字节/字序 | 1010/1018 判据裁定；32 位电量与显示比对 | `byte_swap` / `word_order` 结论；未通过则不配 4 个 32 位电量点 | `byte_swap`/`word_order` 已是显式字段（无隐式特判） |
| RC-3 PCS 地址基准 | FC04 读 `addr=1000` 与 `addr=0` 各一次 | PCS 站 `addr` | 表按 (role, addr) 查，基准变更后退化为"无证据"（不误拒） |
| **RC-4 BMS 主从方向**（**Q-1 已关闭，2026-09-22** —— 性质由"Q-1 的解除判据"变为**常规投运核实**） | 按 §2.2 口径（EMS 主机 / 主控从机）实测 ≥1h | 实测可得即通过；**不可得** → 立即转厂方书面确认（若确认为相反口径 ⇒ 需求侧重走需求评审） | §11.9（基线 + 切换方案 + 代价）；PRD §9.9.2 RC-4 已改同口径 |
| RC-5 消防全量探测器 | 按实际登记数读全量、记单轮耗时 | `fire_det` 块 `count`（分片时加 `read_slice: true`） | `fire_detector_mismatch` 自动告警登记数不一致 |
| **RC-6 空调校验位**（**D-1 已裁定按 ①，2026-09-22** —— 空调站配 `even`、投产限制解除；**本条仍成立**，性质由"裁定前置"变为"**按 ① 落地后的真机实测**"） | **真机实测空调站通信正常**（9600/8E1）；并**顺带核对厂方参数 40016 的实际值确为偶校验**（**现场核对项**；实测不成立 ⇒ 按"改配置 `parity`"处置，**不得**改代码加设备特判） | 站级 `parity`（**已定 `even`、已写入配置**；仅当实测不符时才回写） | `StationParity` 字段与规则 16 同口一致性（`config.rs:29-39` / `:308-327`）+ **`parity` 透传**（`port_runtime.rs::bus_config`，`:161-171`，T5 已落地 —— 否则现场改配置不生效）；配置侧 `hvac` 站已写 `parity: even`（`deploy/config/mupc_core_config{,.production}.yaml`、`tests/fixtures/south_stations_s3b2.yaml`）。**注**："代码已支持透传"（已实现）与"现场空调确实工作在偶校验"（未核实）是两件事，后者正是本条的现场动作 |
| RC-7 连续运行 | 全站 ≥1h、丢包 < 0.1% | — | 站级 offline/online 事件 + 退避封顶 |
| RC-8 总线不冲突 | 确认 `ttyS7` 与 intercore 的 `ttyS0` 物理独立 | 若同总线 → **PCS 站不启用**（保持注释） | 跨段互斥校验（沿用） |
| **RC-9 消防钢瓶气压功能有无**（PRD §9.7.6） | 现场确认该机型**是否有钢瓶气压功能**（读 `fire_sys_2` 是否恒 0）；有 → 用厂方工具核对量值 | 无 → **展示层标"未配置"**（不回写配置；该点**不产事件**） | `cylinder_pressure_configured`（§11.4.6）+ §11.7.3 展示口径已就位 |
| **RC-10 Q-9 探测器地址序**（PRD §9.10 Q-9） | 读回**全部探测器（含探测器 1 = 寄存器 11）**的 `+0` 地址寄存器，确认**严格升序且唯一**；核对"第 n 只"= **地址升序第 n 只** | 若顺序/编号与预期不符 → 调整探测器实际地址（或按实际序核对点位名） | `fire_detector_addr_order_violation` 自动产事件（**链首含寄存器 11**，§11.4.6；**产出频次按状态翻转**，§11.4.7.2/§11.7.2 第 9 条），现场只需确认 |
| **RC-11 Q-19 消防/空调总线归属**（PRD §9.10 Q-19）—— **已撤销（2026-09-22；原条目内容保留如下，供追溯"曾有此核对项、后如何结清"）** | **原核对项（保留原文）**：现场核对消防主机、空调是否**并接在储能侧 BMS 的 485-2 总线**上（旁证：BMS 位 482「485-2 通讯失联」告警）。<br>**结清结论（2026-09-22，风险不成立 ⇒ 撤销）**：**消防、空调各自直连 EMS（BECG-3568），未挂接 BMS 的 485 总线** ⇒ 不存在双 master 共总线情形，**`fire`（`ttyS6`）/`hvac`（`ttyS3`）两站按原计划启用**。**依据** = `hw/微信图片_20260908170935_64_1061.png`（BECG-3568 接线拓扑图 = EMS 的硬件载体）逐口为 `RS485-1 → PCS`（另有 CAN 直连）/ `RS485-2 → BMS` / `RS485-3 → 空调` / `RS485-4 → 关口表` / `RS485-5 → 储能表` / `RS485-6 → 消防`。<br>**原疑问的来源（如实登记，防后人重犯）**：**BMS 对 PCS 的 CAN 协议图 2-1-1「总控模块连接PCS设备」**（提取件 `hw/通信协议/_extracted/proto_BMS 对 PCS__…CAN2.0通信协议A2版.txt`；其对象清单含「消防主机」「空调」）画的是**储能厂家（华塑）以 SCU 总控为中心的自有系统视图**，**并非本项目的接线拓扑** ⇒ 据此认为消防/空调在 MUPC 侧挂 BMS 总线属**误读**；BMS 对 EMS 协议离散输入位 **482「485-2通讯失联故障」**（`proto_BMS对 EMS__…LAN通信协议B0版.txt`）**只说明 BMS 有第二个 485 口，不能推出消防/空调挂在其上**。<br>**另登记（证据瑕疵）**：拓扑图**原图**末两框误标 `RS485-4`（与关口表重复），已在 `hw/微信图片_20260908170935_64_1061_修正.png` 订正为 `RS485-5` / `RS485-6`（**原件保留未动，作为原始证据**） | **（原回写项，前提已不成立、不再适用）**若并接 → 该站不得启用（§9.3.1"禁双 master 共总线"），改由储能侧转发或重新布线 ⇒ **无需回写**；两站端口/校验位等**其余**现场项照原样（`hvac` 的 `parity` 已由 **D-1 裁定为 `even`（①，2026-09-22）**、**待做的只是 RC-6 的真机实测**，`fire` 仍待 RC-5 的登记数分片裁定） | **（原设计侧准备，保留）**与 RC-8 同构（跨段互斥校验只覆盖 `intercore` 串口，**不覆盖此情形**）—— **该论断本身仍成立**（校验的覆盖边界未变，属能力边界陈述，见 RC-8 行与 §11.12.2 Δ-6）；只是**此情形已由现场核对排除**，**不再作为 `fire`/`hvac` 的启用门槛** |
| **RC-12 消防/PCS 设备软件版本核对**（PRD **§9.7.2 第 3 条** + **§9.8.4**） | 核对① 消防主机软件版本 **≥ V1.72**（消防协议 V1.3.1 的前置要求）、② PCS 协议为 **V1.3**（点表对应该版本）；**两者均无版本寄存器**（只有 BMS 有：输入寄存器 **181** 主控程序版本号，已由 `bms_meta` 的 `at: 1` 点采集）⇒ 只能凭**设备铭牌 / 本机显示屏 / 厂方调试工具**核对 | 版本不符 → **重新取点表**，**不得**按现表凑读（§9.7.2 第 3 条）；核对结论记入投运记录（与 RC-1 同批） | **刻意无代码落点**：系统内**不得**声称"读到消防/PCS 版本号"（§9.8.4 明文；PCS 1017 是**故障告警代码**不是版本号）；BMS 侧的可读依据由 `bms_meta` 的 `at: 1`（181）点提供（§11.4.4 登记） |

> **RC-9…RC-12 的来源与编号说明（原 4 条；现为 3 条）**：PRD §9.9.2 的 RC 表只到 **RC-8**，但这四项分别由 **PRD §9.7.6**（钢瓶气压）、**§9.10 Q-9**（探测器地址序）、**§9.10 Q-19**（消防/空调总线归属）、**§9.7.2 第 3 条 + §9.8.4**（消防/PCS 版本核对）明文要求（前两项还需**展示层口径**与**交叉校验**落地，RC-12 明确**不得**在系统内落代码）。本设计**续号补充**并**上报需求侧回写 PRD §9.9.2**（§11.12.2 Δ-6），不擅自改动 PRD 的 RC 编号体系。
>
> **订正**：上句中 **RC-11 已于 2026-09-22 现场核对后撤销**（结论见本表 RC-11 行）⇒ **有效续号的为 RC-9 / RC-10 / RC-12 三项**（**编号不回收、不重排**，保留 RC-11 的空位与原始条目以便追溯），**上报需求侧回写 PRD §9.9.2 的清单随之收窄为三项**（Δ-6 同步）。**RC-11 撤销 ≠ RC-8 撤销**：RC-8（`pcs` 只读站与 intercore 串口是否同总线）**仍未结清**，门槛**一字不动**。

### 11.12 风险、待决项与 PRD 差异上报

#### 11.12.1 本设计新增/变更的字段与契约（须评审追认）

| 项 | 层级 | 来源 | 用途 | 若不追认的退路 |
|----|------|------|------|----------------|
| 1 `read_slice` | 块级（缺省 false） | **PRD §9.4.2.4 已正式补登（PRD v1.7，Δ-3）** —— 本设计改为"落地 PRD 定义"，不再是设计新增 | 豁免第 15 条（设备单次读上限导致的分片，Q-6/Q-7 现场才知） | 不适用（PRD 已定）；若需求侧撤回该字段，退路 = 第 15 条改为"仅警告不拒"（弱化 PRD） |
| 2 点名 `fire_det_count` | 点级（消防 `fire_sys` 的 `at: 7`） | **PRD §9.4.1/§9.5.4 已正式定名（PRD v1.7，Δ-4）** —— 不再是设计补充 | 消防登记数交叉校验（PRD §9.5.4"强制"） | 不适用（PRD 已定）；撤回则退化为"硬编码消防地址 10 / 块名 `fire_det`"（**违背** PRD 反设备特判取向）或不做该校验 |
| 3 `name` 与 `count > 1` 并存 → 拒 | 校验规则 | **PRD 未定义** | 保守口径（禁用而非发明） | 允许并取"首点命名、其余位置命名"（语义模糊） |
| **3b** 无 `points` 块的 `count % width != 0` → 拒（规则 19） | 校验规则 | **PRD 未定义**（设计补充，护栏） | 防"尾槽静默不产点"（§11.4.3） | 改为加载期告警（不拒），或允许静默（**不建议**：与本 PRD 反复整治的"静默失真"取向相反） |
| 4 `MAX_SINGLE_READ_REGS = 120` | 常量 | PRD §9.5.4 的分片口径 | 第 15 条判据 + 分片口径统一 | 取标准 125（与 PRD 分片口径不一致） |
| 5 **`TelemetrySample.kind` / `BlockData::{Regs,Bits}` / `SocOutcome`** | 内部类型 | 设计选择 | 位点变化沿过滤 + "点产出/落库节流"分层 + Battery 四种情形显式化（§11.4.6） | 两条 mapper 入口（前者）；Battery 语义继续隐式（后者，**不建议**，上轮评审正因语义未写而打回） |
| **6 消防字级信号（`SignalSpec` / `@信号键` 事件名 / 双向事件 / "入表即产事件"）** | 内部类型 + 事件命名契约 | **PRD 只要求"消防 → events"（§9.6.1）与"告警位 → events"，未定义信号的表达形态、事件名语法、单向/双向与"哪些位入表"** | 补齐消防事件落点（P0-3） | ① 若评审不接受 `@` 语法 → 改用 `<点名>.<信号键>`（换分隔符，语义不变）；② 若评审要求**位块也双向** → 统一双向 + 事件节流（代价：BMS 288 位在"全 1 复位"时刷新 288 条恢复事件，须加去抖窗口与事件保留策略复核）；③ 若不接受"探测器只取 bit12/bit14" → 展开到 bit0–4（代价：信号源数 ×5）；④ 若不接受"`signals` 只登记产事件者（无 `class` 字段）" → 恢复 `SignalClass{Alarm,State}` 并在表中登记 State 行（代价：出现"登记了却不产事件"的歧义，且需为极性反转位补判据语义） |
| **7 规则 18（`pcs` ≥ 500ms）** | 校验规则 | **PRD §9.3.2.2(2) + §9.8.1 末条明文要求，但未进 §9.4.3 表** | 设计侧补落点 | 不适用（PRD 已要求）；建议需求侧把该条**补进 §9.4.3 表**（现表 17 条 → 18 条），使 AC-1 ③ 有明确出处 |
| **8 状态型事件的产出频次与载荷约定 + `WordEnum` 退出语义** | **事件契约（运行时语义）** | **PRD 未定义**：§9.10 Q-9 只定义"按地址升序 + 以寄存器 11 交叉校验"，**未定义事件产出频次**；§9.6.1 只要求 `fire` → events + SSE，**未定义"退出事件"的语义与消费方判据**；§9.5.4/§9.9.1 亦未给"登记数不一致"的产出频次 | 消除 T5 实测的"**命中即产**"事件风暴（5 轮 = 5 条 ⇒ 约 **8.6 万条/日**，压力在 `storage.events` + SSE）；并把"进入/退出"与"诊断量"两个维度分离（**§11.4.7.2**） | ① 若评审/运维要求"持续异常须**周期性**重提醒" → 需**需求侧给出周期口径**（本设计**不自造**窗），机制退化为"状态翻转 + 固定周期重发"；② 若不接受 `@recovered` 后缀 → 退回"同名 + `value = 0.0` 哨兵"（**代价**：③ 读回 0 只、① 配负 offset 时 raw 0 越界 ⇒ **进入与退出同值不可分**）；③ 若不接受"首次观测即产" → "上线时已违规"的站**永不产事件**（**不可接受**，不建议）；④ 若要求"位块也双向" → 见项 6 的退路（统一双向 + 事件节流） |
| **8b 建议需求侧补登（非阻塞）** | 需求文档 | 同上：**"状态型异常按状态翻转产事件（不逐轮重复）"这一取向 PRD 无出处** | 防后续实现反复（同一形态在 ①③ 上重犯，见 §11.4.7.2 B/C） | **建议**在 PRD §9.6.1 或 §9.9.1 补一句频次口径；**不追认亦按本设计执行**，不影响验收 —— 故**不登记为 §11.12.2 的 Δ**（不是"设计与 PRD 相左"，而是"PRD 未定义、设计补齐"） |
| **9 钢瓶气压「是否配置」的取数接缝 + 每站"曾非 0"记忆（评审裁定"可接受"）** | 接缝（`mupc-southd` → 展示侧）+ 运行期状态 | **PRD §9.7.6 有明文要求**（钢瓶气压恒 0 不得判"气压异常/泄漏"、未配置须标"未配置"），但**未定义取数形态**；设计 §11.4.6/§11.7.3 只给了 **mapper 纯函数** `cylinder_pressure_configured(reads, ever_nonzero)`，**未写"谁持有 `ever_nonzero`、展示侧如何按站取"** | ① `PortRunner` 维护**每站一个"本站生命周期内是否曾出现非 0"**的记忆（初值 false，任一读到非 0 即置 true、**只置位不回退** —— 气压不会在业务上"变回未配置"）；② 取数接缝 **`SouthScheduler::cylinder_pressure_configured(station_index: usize) -> bool`**，使**展示侧**能按站取"该点是否已配置"（落地 §11.7.3 的展示口径）。**展示侧的实际消费（Web/展示层读取该接缝）不属本章范围，属后续任务** —— 本章只保证记忆与接缝就位；该点**永不产事件**（PRD §9.7.6 明令） | ① 若不追认该接缝 → 展示侧拿不到 `ever_nonzero`，只能按**当轮值**呈现 ⇒ **把"未配置"显示成 `0 kPa`**（PRD §9.7.6 明确禁止的形态）；② 若把记忆上移给调用方每轮传入，等于把同一记忆挪到外部、未减少耦合，且"只置位不回退"的语义会散落多处 |

> **核对：本表无 `parity` / D-1 相关条目，无需同步。** `parity` 是 **PRD §9.4.1 已定义**的站级字段（`none` 缺省 / `even` / `odd`，同口一致性校验同 `baud_rate`），**非"设计新增"** ⇒ 本表**不需**新增条目、也**不需**状态变更（对照项：本表的 1 / 2 / 3b 等均为设计侧新增或 PRD 未定义项）。D-1 的裁定（按 ①）与 PRD 表述**同向、无差异**，处置见 §11.14 一致性声明。

#### 11.12.2 PRD 差异上报（**需求侧待订正，设计与实现按下列口径执行**）

| 编号 | 差异 | 事实 | 本设计执行口径 |
|------|------|------|----------------|
| **Δ-1**（**已闭合**） | PRD §9.4.2.4 点级 `format` 行注"⚠️ 缺省是 `int32_scaled`" | **代码事实是 `Float32`**（`config.rs::default_reg_format`，且既有单测 `default_reg_format_used_when_omitted` 钉住） | **PRD v1.7 已订正为 `float32` 并补登"本轮新增块一律显式声明 `format`"护栏** ⇒ 差异闭合。本设计**维持 `Float32` 缺省**（改缺省会改既有 S3a 行为、破坏既有单测与 §9.4.1 第 6 站语义），并把该单测列入"**断言不得改**"（§11.5.3.4 C3） |
| **Δ-2**（**部分闭合**） | PRD §9.4.3 把符号性规则 ① 标为"本轮不可机械校验、不纳入 AC-1" | 有了 §11.4.4 的 `POINT_REGS`（含 `sym_src`），① 变为**可机械判定** —— 但**只对"命中的行"成立**：v1.3 按 P0-2 裁定，**查不到行 → 不拒**（现场 RC-3 合法改 `addr` 基准），故 ① 的形态为"**命中而 `sym_src` 空 → 拒**" | **实现 ①（收窄形态）**（不改变 AC-1 清单，故不违反 §9.4.1 的 YAML ⇒ AC-1 ② 仍成立）。是否把 ① 纳入 AC-1 ③ **属需求侧决定**，本设计不擅自改 AC；**请需求侧注意**："无行不拒"是 ① 的**能力边界**（可追溯性由 ② + RC-1 兜底），PRD §9.4.3 的表述宜同步说明 |
| **Δ-3**（**已闭合**） | PRD 第 15 条"块落地极大性 → 拒"与 §9.5.2/§9.5.4 的"现场分片"张力；以及第 15 条**作用域**未限定（会误拒 `grid_meter`） | PCS 单次读上限（Q-6）、消防上限（Q-7）**均"文档未明确"** ⇒ 分片与否现场才定；且既有 `grid_meter` 六块严格相邻 | **PRD v1.7 已补登 `read_slice`（Δ-3 落地）；v1.8 按项目经理裁定补明第 15 条的适用域（仅作用于声明了 `points` 的块）与判据（合并后 `count ≤ 120`）** ⇒ 差异闭合。本设计 §11.5.1 #15 / §11.5.2 与之一致，并给出"六站必然通过"的证明 |
| **Δ-4**（**已闭合**） | PRD §9.5.4 登记数交叉校验为"强制"，但参考配置未给该点的可识别形态 | `fire_sys` 的点均无 `name` | **PRD v1.7 已正式定名 `fire_det_count` 并在 §9.4.1 的 `at: 7` 声明该名** ⇒ 差异闭合。本设计 §11.4.6/§11.4.7 按**点名**查找（禁按块名 + 硬编码地址） |
| **Δ-5**（**新**） | **规则 18（`pcs` 站 `interval_ms ≥ 500ms`）在 PRD 有明文要求（§9.3.2.2(2)、§9.8.1 末条），但未进 §9.4.3 的 17 条表** | AC-1 ③ 的措辞是"逐条触发 §9.4.3 的拒绝条件" ⇒ 该条**没有 AC 出处** | 本设计**补落点并纳入 AC-1 ③**（§11.5.1 #18、§11.11.2），标注"非 §9.4.3 表内条件"。**建议需求侧把该条补进 §9.4.3 表**（17 → 18 条） |
| **Δ-6**（**新；已扩列；已收窄**） | PRD §9.6.1 要求 `fire` 的数据进 **events + SSE**，但 **PRD §9.9.1 的 AC 表没有对应验收项**；同理 **§9.7.6 的钢瓶气压展示口径、§9.10 的 Q-9/Q-19、§9.7.2 第 3 条 + §9.8.4 的消防/PCS 版本核对，均未进 §9.9.2 的 RC 表** | 需求侧确有要求（上述各条均为明文），但**验收侧无落点** | 本设计在 §11.11.2（AC-5 事件侧扩展）与 §11.11.3（**续号 RC-9/RC-10/RC-11/RC-12**）补齐落点并上报；**建议需求侧回写 PRD 的 AC/RC 表**（本设计不擅自改 PRD 编号体系）。<br>**已收窄（2026-09-22）**：其中 **§9.10 Q-19 / RC-11 已由现场核对结清并撤销**（消防/空调直连 EMS，非并接 BMS 485-2 —— 见 **§11.11.3 的 RC-11 行**）⇒ 本项**待需求侧回写 §9.9.2 的 RC 清单收窄为 RC-9 / RC-10 / RC-12 三项**（**编号不回收**，RC-11 空位与撤销依据保留在 §11.11.3）；**Q-19 自身的关闭由需求侧在 PRD §9.10 另行同步**（本设计不改 PRD） |
| **Δ-7**（**新；已闭合，仅登记过程**） | PRD §9.4.3 第 15 条判据若只按 **PRD v1.6 原文**（"合并后空洞 ≤ 4"）实现，会把 `fire_sys`+`fire_det`（空洞 0、合并 127 > 120）判为"应合并"⇒ 与 §9.5.4"必须分片"直接冲突 | 单一判据下"空洞小"并不等于"合并不超设备上限"，结论方向反了 | **PRD v1.8 已补 ④"合并后 `count ≤ 120`"并保留 ③，同时限定适用域** ⇒ 差异闭合。本设计**四条判据全实现**（§11.5.1 #15），并证明 **③ 在适用域内由规则 11 恒成立**（§11.5.2(1)）⇒ 与 PRD **行为等价**；实现顺序建议先判 ④（提前放行"本就该分片"的形态） |
| **Δ-8**（**新；备案项，非阻塞**） | PRD v1.8 第 15 条的"`discrete` 位块是否参与合并判定"**字面有两种读法**（正文把位块列为"不参与"的例子，但 ②/③ 又出现"`discrete` 按位地址同理""位块不受 ③ 限制"） | 本设计取**保守读法**（`discrete` 一律不参与）；两种读法在本轮**判定结果完全相同**（§9.4.1 的两个位块均未声明 `points`） | 按保守读法实现；**请需求侧在 §9.4.3 第 15 条补一句消歧**（例如"声明了 `points` 的 `discrete` 位块是否参与：不参与/参与并如何折算位数为寄存器数"）。本设计不擅自改 PRD |
| **Δ-9**（**新；须需求侧追认**） | PRD **§9.8.4「事件」栏要求「PCS 32 位电量比对未通过（若配置）」产事件** | 该"比对"是 **RC-2 的现场人工比对**（把 1042–1045/1072–1075 的累计电量与设备显示/大屏读数比对，§9.7.3），**系统内没有参考量可判**；而 PRD §9.7.2 第 2 条又明令"本轮**不做自动量程校验**（除 SOC 外）" ⇒ 运行期**不存在可落地的判据**（既不能自动比对、又不允许量程自校验） | **本设计以 RC-2「未通过则不配 4 个 32 位电量点」替代**（落地口径见 §11.7.4 迁移清单末行"删点不删块"；现场动作见 §11.11.3 RC-2）——即**用投运前的显式裁定替代运行期事件**：比对不通过 ⇒ 这些点根本不进配置 ⇒ 系统内不存在"不可信数据"，也就无需"比对未通过"事件（这正是 PRD §9.7.3"该比对通过前 32 位电量视为不可信"的形态）。**本轮实现口径 = 不产该事件**（§11.4.7 事件产出表 ⑦）。**登记为与 PRD 的差异，需需求侧追认或订正 PRD §9.8.4**：二者择一 —— a. **接受本口径**，把该事件从句中删除（或改写为"RC-2 未通过 ⇒ 不配点"，与 §9.7.3 表述对齐）；b. **坚持要事件**，则须由需求侧给出**系统内可判的判据**（例如"该 4 点已配置且值域/单调性校核失败"），本设计再据以补落点 |

> **说明：本表不新增 Δ。** 本章的两条事件口径裁定**均不与 PRD 字面相左** —— ① **探测器 1 链首**（§11.4.6）是 PRD §9.10 Q-9 处理口径"以**寄存器 11** 读回的地址值交叉校验"的**字面落地**（原设计/实现少算了链首，属**欠实现**而非"多出的设计"）；② **状态型事件的产出频次与 `@recovered` 载荷**（§11.4.7.2）属 **PRD 未定义、由设计补齐**（PRD 未给"事件产出频次"，§9.6.1 只要求 `fire` → events + SSE），已登记为**设计侧新增契约**（§11.12.1 项 8，**须评审追认**），并**建议**需求侧在 §9.6.1/§9.9.1 补一句频次口径（**非阻塞**，不追认亦按本设计执行）。
>
> **另需注意（不属 Δ，属跨章实现约束）**：§11.4.7.2 D 的"**成组判读**"判据约束的是 **§10.4 融合的消费侧**（未来实现者），**§10.4 融合规则本体与 PRD 均不改**；实测现存联锁（core-bin `interlock.rs`）**只由 DI/GPIO 驱动、尚未消费 southd 的 fire 事件**，故**无需补改现存代码**。
>
> **说明：本表不新增 Δ（2026-09-22）**。**D-1（空调串口校验位）已裁定按 ①**（新增站级 `parity`、空调站 `even`、同口一致性校验同 `baud_rate`）—— ① 本就是 **PRD §9.4.1 已定义**的站级字段 ⇒ 既非"设计新增"（不入 §11.12.1），也与 PRD **同向无差异**（不入本表）。评审/终审记录中「若裁定为 ② 则「同口 `parity` 一致性」与 **AC-1 ③** 对应触发项**失去对象**（`baud` 一侧仍可验）、须随裁定回写」的登记，**因前提（②）未发生而不适用、无需回写**；该规则与 AC 项**保留有效**（实现 `config.rs:308-327` + 用例 `tests/s3b2_config.rs:698`）。需求侧登记见 PRD `[§9 增补 v1.11]` 与 §9.10 D-1 行。

#### 11.12.3 实现风险（开发期须注意）

| 风险 | 说明 | 缓解 |
|------|------|------|
| R-1 既有测试/构造点连锁订正 | ① **20 处** YAML fixture / 内联 YAML 需改构造数据（含 A1–A13 + **A15/A16**（移入）+ **A17–A21**（移入，均为"假绿订正"）；其中多数是"battery 补含 `soc` 点的 `regs`"、1 处 `fire` `addr` 改非 0、2 处 mapper/scheduler fixture 改点名式、1 处两站补完整相量块）；② **`RegBlockConf` 字面量 10 处 + `StationConf` 字面量 8 处**（逐个核实，非此前的"≈13 处"）需机械补新字段；③ `BlockReads` 改为承载 `BlockData::{Regs,Bits}` 后，`grid_convergence.rs` 与 mapper/scheduler 单测的 canned 构造点需同步；④ `telemetry_points` 签名/返回类型变更（`(String,f64)` → `TelemetrySample`），其单测**断言与测试名**需订正（§11.5.3.3 第 8 条）；⑤ **9 处 metric 更名导致的断言订正**（§11.5.3.3） | **完整分类清单见 §11.5.3**（fixture 订正 / 断言订正 / 断言不得改，逐项到 `file:行`）。执行顺序：先做类型改造 + 机械补字面量 + 改 fixture（**在一个提交内完成**），再按 11.5.3.3 订正断言期望值；`§11.5.3.4`（C1–C7）列出的断言**一个都不许改** |
| R-2 表↔配置漂移 | `POINT_REGS`（展开后 618 行，探测器区为模板）与配置双份数据 | `tests/point_table_vs_reference_config.rs` 双向核对（§11.4.4）+ 行数断言 |
| R-3 需求未定项的现场回写 | Q-4/Q-16/Q-20/RC-2/RC-3 会合法改 `addr`/`format`/`word_order`；**RC-3 改 `addr` 基准后 `POINT_REGS` 的键会整体失配** | 这些字段**不做**强校验（只作用于 `offset`），且 **`lookup` 查不到行 → 放行**（P0-2 裁定）⇒ 现场校准**不会**被启动期拒；漂移由 `tests/point_table_vs_reference_config.rs`（参考配置侧）与 RC-1 逐点比对兜底 |
| R-4 位错 1 位 ⇒ 全表语义偏移 | FC02 位序（bit0 = LSB） | `unpack_bits` 单测（PRD 原文示例 0x3B/0xBB）+ AC-3 逐位 + RC-1 抽点 |
| R-5 618 点落库量 | 模拟量 ≈300 行/s；位点按 D2 稳态≈0 | retention 天数投运前复核；位点稳态零写 |
| **R-6 消防信号误报/漏报** | 信号 mask/活跃集写错 → 火警事件误报（虚假停机判据）或漏报；`bit15 = 0 表示离线` 的**极性反转**位最易写错 | ① 信号清单逐条对表 PRD §9.5.4（§11.4.7.1 表）；② AC-5 事件侧用例**逐信号钉住**（含"预留 bit2 零事件""bit15 不入事件"的负向断言）；③ `EdgeTracker` 首轮/恢复后只建基线不产事件（防"上线即刷事件"）；④ 探测器地址序异常时点位标记不可信（防空地址乱序下的误判） |

### 11.13 实施顺序（建议 Task 拆分，供项目经理排期）

| # | Task | 内容 | 闸门 |
|---|------|------|------|
| T1 | 解码原语 | `RegFormat` 扩展 + `RegDecode` + **`WordOrder`（唯一定义）**；`decode_regs` 薄包装 | `meter_regs` 单测全绿（含既有 6 例不变）+ 新增 AC-2 L1 向量 |
| T2 | 帧层 FC02 | `unpack_bits` + `parse_bits_response` + `read_discrete_inputs_from` | `unpack_bits` 单测（PRD 示例 + 31 位/288 位） |
| T3 | 配置与校验 | §11.4.1 结构 + §11.5 **十七条 + 规则 18/19** + `read_slice`；**含 ① fixture 订正（§11.5.3.2）与 ② 断言订正（§11.5.3.3）**；`validate_maximality` 按 §11.5.2(1) 的四条判据实现（**建议判定顺序：先 ④ `count > 120` → 放行；再 ③ 空洞；再 ① ② 与 `read_slice`**，使"本就该分片"的形态提前放行） | **AC-1 ①②③ 全绿**（含 §11.11.2 AC-1 ③ 的规则 15/6/18/19 边界用例）；**既有用例经 §11.5.3 分类订正后全绿**——注意 T3 的"DoD"不是"既有用例一字不改"（旧措辞不准确，见下注） |
| T4 | 点展开与点表 | `points.rs` + `point_table.rs`（探测器区**模板 + 运行期展开**，n=20 展开 618 行）+ 消防 `SignalSpec` 表 | AC-6 全绿；表↔配置交叉核对绿；**消防信号清单与 PRD §9.5.4 位定义逐条对表** |
| T5 | 总线与调度 | `StationBus::read_discrete`/MockBus；scheduler 分发 + **统一 `EdgeTracker`（位点 + 字级信号）** + `role_priority(Pcs)`<br>**（补改，T5 已实现并通过代码评审，下列 2 项为评审升级的设计裁定所致）**：① **⑤ 地址序事件改按"状态翻转"产出**（进入 / `@recovered` 退出、连续同态不产、**首次观测即产** —— **§11.4.7.2 C**；去掉现"每轮直接 `events.push`"）；② **升序链首含寄存器 11**（探测器 1 —— **§11.4.6**） | AC-3/AC-4 绿；**消防信号正向（进入/退出/枚举跃迁/地址序）与负向（预留位零事件、气压零事件、位块无退出事件）用例见 AC-5 事件侧**；**追加**：`fire_detector_addr_order_violation_emits_event` 按 AC-5 的地址序条目重写（"首轮即产 1 条 / 连续轮 0 条 / 恢复产 `@recovered`"），并新增"**探测器 1 地址最大仍须产**"的链首用例（既有用例期望值因链首 +1） |
| T6 | mapper 与消费 | `telemetry_points` 重构（逐点/每值槽）、**`soc` 点名 + `SocOutcome` 四情形**、`Pcs` 臂、SOC 域检查、消防登记数 + 地址序 + 钢瓶气压口径；core-bin `SouthSink` 事件 message<br>**（补一句约束）**：① SOC 越界与 ③ 登记数不一致**必须按 §11.4.7.2 的"状态型事件"口径实现**（状态翻转 + 首次观测即产 + `@recovered` 退出），**不得**沿用"命中即产"（否则同一风暴在 ①③ 上原样重现） | AC-2 L2 / AC-5 绿（含事件侧扩展）；**追加**：①③ 的"首轮即产 1 条 / 连续轮 0 条 / 恢复产 `@recovered`"用例 |
| T7 | 配置迁移与文档 | `deploy/config/*.yaml` 换 §9.4.1 的 6 站（PCS 保持注释）；确认本设计的落地状态（**不新增构建参数，`build.md` 不改**）；**收口 T4 遗留的格式不符合 1 处**：`tests/s3b2_config.rs` 的 `assert_eq!` 链式调用 `point_table::lookup(Role::Battery, 116).unwrap().offset` 超 `rustfmt` 的 `chain_width`（报 `:374`，实际不符合行 `= :377`）⇒ 按 `rustfmt` 建议拆链（**断言语义不变**） | AC-7 + `grid_convergence.rs` 回归锚（**断言零改动**）。**AC-7 的 fmt 部分按 §11.11.2.1 的行级口径执行**（**不是**"跑一次 `cargo fmt`"）：<br>① **主判据（不得新增）**：`rustfmt --edition 2021 --check` 对**本 Task 触及的 `.rs` 文件**报出的**"实际不符合行"集合**（= 输出中**带 `-` 的行**；**不是** hunk 锚点行，抽取规则见 §11.11.2.1 判据 ① 的注意事项 (a)(b)）∩ 本 Task diff 的新增行号集合 = **∅**（非空即给出 `file:行`）；<br>② **纯新增文件零报出**：`mupc-southd/src/point_table.rs`、`tests/point_table_vs_reference_config.rs`、`tests/s3b2_decode_e2e.rs`（**归属订正：前者 T4 新建、末者 T6-5 新建**）均**已实测 0 处**，T7 复跑仍须 0 处；<br>③ **禁止** `cargo fmt --all`（既有基线 **1204 处 / 106 文件**，与本特性无关；全仓格式化会淹没真实 diff）；**T3 遗留在 `tests/s3b2_config.rs` 的 26 个 hunk / 33 条既存不符合行不动**（§11.11.2.1 判据 ③）；<br>④ **消费方可见变更核对（登记项，提示本 Task 留意）**：`SouthSink` 的 **`offline` 事件文案新增 `：{reason}` 后缀**（**`online` 文案逐字不变**；事件类型 `south_station.<站id>.offline`、等级 `warning`、去抖归 scheduler **一字不变**）—— PRD §9.7.2 第 1 条的落地（§11.7.2 第 10 条），**若有消费方/文档/告警规则/前端按旧文案逐字匹配须同步核对**；<br>⑤ **准入门槛**：**`fire` 站写入生效配置前，必须先结清 §11.4.6 的登记数容量式订正**（`1 + Σ/6`；旧式 `Σ/6` 恒真 ⇒ 永久假告警 + RC-5 失效）—— 见 §11.7.4 的 `fire` 站门槛行，与本行同批放行 |

> **T3 闸门措辞订正（P0-4 的直接后果）**：旧版写的是"既有用例全绿"＋回归闸门写"三者零破坏（**仅** §11.5.3 列出的 fixture 数据订正）"——该口径**不成立**：metric 更名（`temp` → `temp_1`）会让 `scheduler.rs` / `mapper.rs` 的**断言期望值**必须改（§11.5.3.3，共 9 处），这不是"fixture 订正"。本版统一为：**"经 §11.5.3 的三类分类处理（fixture 订正 / 断言订正 / 断言不得改）后全绿"**，并在 §11.5.3.4 明确"不得改"的回归锚清单。

> **T7 的 AC-7 / fmt 口径订正（实现期实测的直接后果）**：T7 原列的 `cargo fmt` 闸门**不可执行** —— 实测仓库既有基线 `cargo fmt --all --check` = **1204 处 / 106 文件**（非 S3b-2 回归），且本项目**明令禁止** `cargo fmt --all`（1204 处无关改动会淹没真实 diff）⇒ **不能**用"跑一次全仓 fmt 使其变绿"来满足 AC-7。现按 **§11.11.2.1** 改为**行级可验证口径**：判据 ①（本 Task **新增/修改的行** fmt-clean：`rustfmt --check` 报出行号 ∩ 本次 diff 新增行号 = ∅）为主判据、判据 ②（本 Task **新建**的 `.rs` 文件零报出）对纯新增文件适用、判据 ③（既存基线一律不动）封住超范围格式化。**T4 实测**：`point_table.rs`、`tests/point_table_vs_reference_config.rs`、`tests/s3b2_decode_e2e.rs` = **0 处**（判据 ② 满足；其中 `tests/s3b2_decode_e2e.rs` 为 **T6-5** 新建，**归属订正** —— 其复跑在 T6-5 落地之后，见 §11.11.2.1 条目 1）；T4 **仅**在 `tests/s3b2_config.rs` 引入 **1 处**（判据 ① 违反）⇒ **T7 收口**（见上表 T7 行）；该文件另有的 **26 处**为 T3 既存基线（判据 ③，不动）。
> **措辞订正（本注内的"报出行号"即判据 ① 的抽取对象）**：`rustfmt --check` 的"不符合行"**须取输出中带 `-` 的行**，**不是** hunk 锚点行 `Diff in <file>:N:` 的 `N` —— 实测该文件锚点 `:374` 对应**实际不符合行 `:377`**，且**锚点数（27 个 hunk）≠ 不符合行数（34 条）** ⇒ 判据 ① 的抽取规则与逐行推进方式见 **§11.11.2.1 判据 ① 的注意事项 (a)(b)**（本注的"报出行号"按此理解为"带 `-` 的行"）。
> **收敛注（W1 裁定，本注与上表 T7 行闸门 ② 的阅读说明）**：判据 ② 已收敛为判据 ① 的推论并限定适用于 **T4 起新建**的文件 —— T7 行闸门 ② 所列三个文件（`point_table.rs` / `tests/point_table_vs_reference_config.rs` / `s3b2_decode_e2e.rs`）**不受影响**（复跑仍 0 条，"T7 复跑仍须 0 处"照旧）；**T1–T3 新建的 `points.rs`（16 条）/ `s3b2_config.rs`（33 条）等 66 条不符行不适用判据 ②**，已显式登记为 §11.11.2.1「S3b-2 格式债基线清单」（判据 ③ 处理 + 独立格式债待办指向 `docs/technical-debt.md`）。T7 闸门 ③ 中"T3 遗留在 `tests/s3b2_config.rs` 的 26 个 hunk / 33 条既存不符合行不动"与本清单一致。

**回归闸门**：
1. **`mupc-southd/tests/grid_convergence.rs`** —— meter_grid 语义逐字段等价，**只改 `block()` 构造的字段补齐，断言一个字节都不动**（§11.5.3.4 C1）；
2. **`config.rs` 的 meter_grid-only 系列 + 解析类系列**（§11.5.3.4 C2/C3）—— 断言不动；
3. **`core_config.rs` 的 6 个 southern-stations 用例** —— 断言不动（只改 fixture，§11.5.3.4 C4）；
4. **`scheduler.rs` 的调度/退避/隔离/SOC gating 断言**（§11.5.3.4 C5）—— 不动。

> 即：**"零破坏"的对象是"断言语义"，不是"逐字不动"** —— 因 PRD §9.4.2.1 第 4 条（每值槽 1 点 + 位置式命名）而必须改的期望值属**规则变更的传导**（§11.5.3.3 已逐处列明），其余任何断言变红都按**回归**处理。

### 11.14 一致性声明

| 关联 | 关系 |
|------|------|
| PRD §9（**v1.8**，`[REVIEWED: PASS: 2026-09-21]`；第 15 条已按项目经理裁定订正适用域与判据） | 本章是 §9 的实现级落地；§9.4.3 的 **17 条 + 设计补落点的 2 条（规则 18/19）**逐条落点见 §11.5（**第 15 条四条判据的完整口径、"③ 恒成立"的等价性证明与"六站必然通过"的逐站证明**见 §11.5.2）；G-1…G-6 落点见 §11.1（含**消防事件**这一非 G-项落点）；Q-1 处理见 §11.9；**G-6 替代口径与"消防事件层"三层边界**见 §11.10；**消防事件统一模型（信号/`EdgeTracker`）**见 §11.4.7.1；既有测试连锁订正的**三类分类清单（fixture / 断言 / 不得改）**见 §11.5.3；PRD 差异上报（**Δ-1…Δ-9**）见 §11.12.2（Δ-1/Δ-3/Δ-4/Δ-7 已闭合；**Δ-8 为待需求侧消歧的备案项**；**Δ-9 为新增项、须需求侧追认**：§9.8.4 的"PCS 32 位电量比对未通过 → 事件"由 RC-2「未通过则不配点」替代，本轮不产该事件）；**事件口径裁定（⑤ 地址序去重 / `WordEnum` 退出语义 / 探测器 1 链首）**见 **§11.4.7.2 / §11.4.6**，**不新增 Δ**（两项均**不偏离 PRD 字面**：Q-9 原文即要求"以**寄存器 11** 读回的地址值交叉校验"；PRD 未定义"事件产出频次"，属设计补齐） |
| **输入契约** | `specs/modules/02-MUPC-南向通信-PRD.md` §9（`[REVIEWED: PASS: 2026-09-21]`，**PRD v1.7 补登 + PRD v1.8 订正**）。§9 已定的块/点模型、位置式点名、类型来源三分类、G-1…G-6 范围、Q-1/Q-20 登记与 AC/RC 划分是**需求约束**，本章只落地、不推翻。 |
| **与 PRD v1.8 / PRD v1.7 的对齐声明**（本设计已按同一表述对齐，不另起炉灶） | ① **第 15 条**：PRD v1.8 已落定 ——「**仅作用于声明了 `points` 的块**」「未声明 `points` 的块（含既有 `meter_grid` 形态、`discrete` 位块）**不参与**合并判定」「拒绝条件 = ① `func`/`byte_swap` 同 ② 地址连续 **③ 合并后空洞 ≤ 4** **④ 合并后 `count ≤ 120`**」「`read_slice: true` 豁免，且只在本条适用域内有对象」。本设计的 §11.5.1 #15 / §11.5.2 **四条全实现**（不是只取 ④），并额外给出：**③ 在适用域内由规则 11 的首尾锚定恒成立**（⇒ 实现 ③ 不引入额外拒绝，与 PRD 行为等价）、**§9.4.1 六站必然通过本条**的逐站证明、以及**建议的实现判定顺序**（先 ④ 再 ③）。**本设计未推翻：`grid_meter` 形态、`discrete` 位块、既有站行为一律不参与判定。**<br>② **Δ-1**（`format` 缺省 = `float32`）与 **Δ-4**（`fire_det_count` 定名）已由 PRD v1.7 正式补登，本设计**不再是差异**（§11.12.2 已标注"已闭合"）；<br>③ **Δ-3**（块级 `read_slice`）已由 PRD v1.7 正式补登为块级字段（§9.4.2.4），本设计的 §11.4.1/§11.5.2 改为**引用 PRD 定义**而非"设计新增"。 |
| **本章改动范围（硬约束）** | 本章的改动范围限于 §11 自身；§10 与 PRD 均未因本章变更而改动。 |
| 本文档 §10（S3a） | 调度架构/故障隔离/role 分发骨架**不变**；本章只在 §10 预留接缝上扩展（§11.3 扩展点列），并对 §10.6"role 映射"与 §10.8"文件结构"作**增量补充**（不回改原条款） ；**与 §10 的关系**：§10（S3a，已批准并实施）的**调度架构、故障隔离语义、role→DataPackage 分发骨架全部不变**。本章只做两件事：① 补齐点表**表达与读取所必需**的数据面能力（PRD §9.1.1 的 G-1…G-5）；② 接入**一个新 role**（`Role::Pcs`）并落实 5 份厂方点表。所有扩展都落在 §10 已预留的接缝上（见 §11.3 的"扩展点"列）。 |
| PRD §9.11 ① | 已明确要求设计文档至少覆盖：`RegBlockConf.points` 结构与校验（§11.4.1/§11.5）、`StationConf.parity`（§11.4.1，**D-1 已裁定按 ①、2026-09-22 生效**）、位块点落库口径（§11.7.2）——**三项均已落实** |
| 核间 10 §2.4 / `archive/2026-09-08-S2-DI-DO安全联锁.md` | PCS 只读 + 4 区 500 停机原语归属见 §11.8；数据面边界 ADR-012 维持 |
| 04 策略引擎 §2.11.1 | SOC 双源（BMS 优先/超期回落核间）链路不变（§11.7.1、§11.9.3） |
| 项目 CLAUDE.md | 无硬编码密钥；无新增 `unsafe`；错误类型实现 `std::error::Error`；不新增独立设计文档 |

---

## 12. 块级采集周期覆盖（告警位单独快采，S3b-3）

> **本章来源**：02 PRD **§10（v1.12 新增；v1.13 为评审附项的文档级订正，2026-09-23）**；用户 2026-09-23 就 12 号 R-33 的裁定「**告警位单独快采**」。**本章为新增章节**（接续 §11 的编号），不改动 §1–§11 的任何条款与既有门禁标记。**本章已按设计评审意见修订（v1.13，待复审），见文首登记块；本次未获门禁标记。**

### 12.1 背景与落点总览（Why）

| 项 | 事实（回代码） | 本章落点 |
|----|----------------|----------|
| 采集周期是**站级单值** | `StationConf.interval_ms`（`mupc-southd/src/config.rs:76-77`）；`DueCalc` 的到期条目**每站一条**、`next_due` 按站推进（`scheduler.rs:106-114` / `:126-163`） | §12.2.1 新增**块级** `interval_ms`；§12.2.2 引入 `ReadGroup` 与 `GroupKey`；§12.4 改造 `DueCalc` |
| 站轮内**一次性读齐全部块** | `poll_station` 的逐块读循环（`scheduler.rs:568-606`） | §12.4.3 拆为 `poll_group`（只读该组块集） |
| 站级语义判据按**一次读集**求值 | `mapper::poll_to_result`（`mapper.rs:407-464`）；`round_signals` 的 `StationFlag` 判据（`scheduler.rs:345-380`） | §12.3 的 **C6/C7**（PRD §10.3.2）+ §12.4.5 的运行期守卫 `judges_evaluable`（**作用域 = 判据/站级量路径**，见 §12.4.5 的作用域声明） |
| 变化沿记忆**按站** | `PortRunner.trackers: HashMap<usize /*站下标*/, EdgeTracker>`（`scheduler.rs:284-285`）；`EdgeTracker::prime` **先 `clear()` 再插入**（`scheduler.rs:237-243`） | §12.4.4：键由 `usize` 改为 `GroupKey`（**必须**，否则组间会互相清空基线） |
| 站级 `offline`/`online` 与退避 | `handle_failure`（`scheduler.rs:701-726`）/ `mark_success`（`:730-746`）/ `backoff_extra`（`:88-93`） | §12.5：**站级承载组唯一承载**（PRD §10.3.2 C8 + §10.6 第 4 条） |

### 12.2 数据结构

#### 12.2.1 配置结构（`mupc-southd::config`）—— 唯一新增字段

```rust
pub struct RegBlockConf {
    // …既有 10 个字段（name/addr/func/format/scale/count/offset/byte_swap/points/read_slice）全部不动…
    /// **块级采集周期覆盖**（PRD §10.3.1，S3b-3 新增）：
    /// `Some(v)` = 本块按 v ms 轮询；`None`（**缺省**）= 继承站级 `interval_ms`。
    /// **缺省 ⇒ 既有配置与既有行为零变化**（§12.8 的 AC-8-3 回归锚钉住这一点）。
    /// 取值由 PRD §10.3.2 的 C1–C5 约束；`skip_serializing_if` 使**既有 YAML 往返逐字不变**。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_ms: Option<u64>,
}

/// 块的有效采集周期 —— **唯一求值点**（校验期与调度期共用同一函数，防"两处各算一次"漂移）
impl RegBlockConf {
    pub fn effective_interval_ms(&self, station_interval_ms: u64) -> u64 {
        self.interval_ms.unwrap_or(station_interval_ms)
    }
}
```

**为什么用 `Option<u64>` 而不是"缺省值语义"**：`None`（继承）与 `Some(站周期)`（显式声明为站周期）在**校验语义上等价、在配置意图上不同**——与点级 `scale`/`offset` 用 `Option<T>` 区分"未声明"与"显式 0"是同一取向（§11.4.1）。`None` 保证**零行为变化**可机械证明。

#### 12.2.2 调度内部结构（`mupc-southd::scheduler`）

```rust
/// 站内一个「读组」：**有效周期相同**的块合为一组；一组一次轮询读齐组内全部块。
#[derive(Debug, Clone)]
struct ReadGroup {
    /// 组内块在 `StationConf::regs` 中的下标（**升序 = regs 书写序**）
    blk_indices: Vec<usize>,
    /// 组周期 = 组内块的有效周期（构造期由 `read_groups_of` 保证同组同值）
    interval_ms: u64,
    /// 组锚 = `blk_indices[0]`（块 → 组是 1:1，故锚**唯一**，可直接作稳定键）；
    /// **空块集（`regs` 为空的退化组）取哨兵 `EMPTY_GROUP_ANCHOR = usize::MAX`**
    /// —— 无真实块下标可与之相等，故锚仍**单射**（§12.4.1）。
    anchor_blk: usize,
}

/// 组键：`(站下标, 组锚块下标)` —— 稳定、与 cfg 序绑定、可作 `HashMap` 键（`Hash + Eq`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupKey { pub station_index: usize, pub anchor_blk: usize }

/// 本轮应采的**一个读组**（替代既有的 `StationPoll`，`scheduler.rs:78-82`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupPoll {
    pub station_index: usize,
    pub anchor_blk: usize,
    /// 该组是否为**站级承载组**（PRD §10.3.2 **C8**：站内周期**最大**的组；并列取锚最小者）
    pub is_carrier: bool,
    /// 该组**上一轮是否失败**（决定本轮成功后是否重建变化沿基线；见 §12.5）
    pub was_failing: bool,
}

/// 到期条目：`station_index`/`role`/`interval_ms`/`next_due` 语义同既有，
/// 新增 `key`（组键）、`is_carrier`（C8）与 `group_fail_count`（**组级**连续失败计数）。
struct DueEntry {
    key: GroupKey,
    role: Role,
    interval_ms: u64,      // = 组周期
    next_due: u64,         // uptime ms，单调驱动（同既有）
    is_carrier: bool,
    group_fail_count: u32, // 仅块级组自增；站级承载组的失败由站级 `offline_count` 记账
}
```

`PortRunner`（`scheduler.rs:280-290`）的两处改动：

```rust
struct PortRunner {
    bus: Option<Arc<dyn StationBus>>,
    calc: std::sync::Mutex<DueCalc>,
    /// **变化沿记忆：键由 `usize`（站下标）改为 `GroupKey`**（§12.4.4 给出"必须改"的理由）
    trackers: std::sync::Mutex<HashMap<GroupKey, EdgeTracker>>,
    /// 站下标 → 该站的**全部组键**（**承载组**判定"站恢复"时须"重建全部组基线"，§12.4.4 连带项 a）
    /// —— 注意：**只有承载组**会用到它（S-3 修订：非承载组不得触发站级全组重建）
    groups_of_station: HashMap<usize, Vec<GroupKey>>,
    /// **组键 → 组内块在 `StationConf::regs` 中的下标**（构造期由 `read_groups_of` 一次算好，
    /// `poll_group` 直接查表 ⇒ 免每轮重新分组；也是"块 → 组"的唯一权威映射）。
    /// **空 `regs` 站的退化组映射到空块集 `vec![]`** ⇒ `poll_group` 的读循环 **0 次**、
    /// `poll_to_result(role, &[])` 照常求值（与既有空 `regs` 站逐字等价，见 §12.4.1）。
    group_of: HashMap<GroupKey, Vec<usize>>,
    cylinder_seen_nonzero: std::sync::Mutex<HashSet<usize>>, // 语义不变（站级）
}
```

> **已删除的字段（优化 4 之二）**：**v1.12 曾声明** `carrier_anchor: HashMap<usize, usize>`（"站下标 → 承载组锚块"；见附录 **v1.12** 行"`PortRunner` 增 `carrier_anchor` / `groups_of_station`"），但该字段在 §12.4.3 的伪码中**无任何使用点** —— `poll_group` 实际查的是 **`runner.group_of[&key]`**、承载性由 `GroupPoll.is_carrier`（构造期由 `DueCalc::from_group` 算好）携带 ⇒ v1.13-r1 **删除**（需要"站 → 承载组锚"时用 `carrier_group(c).map(|g| g.anchor_blk)` 现算，或由 `DueEntry.is_carrier` 携带）。

### 12.3 分组不变量（V-1…V-6）

> **与 PRD 的对应关系（不再是"一一对应"，优化 2 订正）**：v1.13 的标题自称"与 §10.3.2 的 C1–C9 **一一对应**"—— **不实**。实际对应是：**V-1** ↔ §10.3.3（分组语义）；**V-2** ↔ C6；**V-3** ↔ C5/C7；**V-4** ↔ C1/C2；**V-5** ↔ §10.3.1 定性 1；**V-6** ↔ §10.6 第 3 条。**C3 / C4 / C8 / C9 没有"分组不变量"形式**：C3（`poll_ms` 网格）与 C4（`1.5×T_组` 下界）与 C9（口占用上界 `U ≤ 0.5`）是**数值约束**（落 §12.7 规则 21 / 20 / 23），C8 是**承载组选择规则**（落 `carrier_group()` 与 §12.5）；PRD §10.5 第 3 行（`Σ T_组` 上界）落**规则 24**（同样无 V 形式）。⇒ 「不变量」与「C 编号」是**两种不同形态的约束**，本节与 C1–C9 **不是一一对应**。

| 不变量 | 内容 | 对应 PRD | 保证方式 |
|--------|------|----------|----------|
| **V-1** | 分组键 = **有效周期**：`ReadGroup` 按 `eff(块)` 分桶，同桶同组 | §10.3.3 | `read_groups_of`（§12.4.1，**唯一分组实现**） |
| **V-2** | `R(role)` 内块**同组**（⇒ 判据恒看到齐备读集） | C6 | 配置期 `Err`（§12.7 规则 22）+ 运行期守卫（§12.4.5） |
| **V-3** | `R(role)` 的组周期 **= 站级 `interval_ms`**（C5+C7 合取） | C5/C7 | 配置期 `Err`（规则 21/22） |
| **V-4** | 组周期恒 `> 0`（⇒ `next_due` 推进与 `backoff_extra` 的 `interval_ms > 0` 前提成立，与 `scheduler.rs:536-541` 的既有论证同源） | C1/C2 | 配置期 `Err`（规则 20）；退化组取站周期，而站级 `interval_ms > 0` 由既有站级校验保证（`config.rs:227-229`） |
| **V-5** | **单组站恒恰有 `1` 个组** ⇒ 调度行为与改造前**逐字等价**。两种情形：**(a)** 有块且全部块未声明 `interval_ms` ⇒ 唯一组 = `(站周期, 全部块)`；**(b) `regs` 为空** ⇒ 唯一**退化组** = `(站周期, **空块集**)`（锚 = 哨兵 `EMPTY_GROUP_ANCHOR`） | §10.3.1 定性 1 | `read_groups_of`：情形 (a) 在"全部块 `None`"时天然只产出一个桶；情形 (b) 由**显式的"空则补一个空桶"分支**保证（§12.4.1）—— **不得**返回 0 个组 |
| **V-6** | 口内**串行**不变：组是新的调度粒度，但同口仍单 poller 串行（`Rs485PortBus` 的 per-port async Mutex，§10.2 / `scheduler.rs:1-5`） | §10.6 第 3 条 | 不改 `spawn` 的"每口一条 task"结构（`scheduler.rs:492-507`）。**注**：`due_round` 排序键在 §10.6 第 3 条的 `(角色优先级, 站序, 组锚)` 之外多出 `!is_carrier`（**只定同 tick 先后、不引入并发**），该字面差异已登记为 **Δ-14** |

**V-5 的机械证明（零行为变化的依据，两情形并列）**：

- **情形 (a)**（有块、全部块 `interval_ms == None`）：`eff` 全等于 `station.interval_ms` ⇒ `read_groups_of` 产出**唯一桶**（周期 = 站周期、`blk_indices` = 全部块、`anchor_blk = 0`）⇒ `DueCalc.entries` 与改造前**逐条目同值**（`interval_ms`/`next_due` 初值 0 均同）；`due_round` 的推进/钳制（`scheduler.rs:146-155`）与排序键（`(role_priority, station_index, !is_carrier, anchor_blk)` ≡ 既有的 `(role_priority, 条目序)` —— 单组站的 `!is_carrier = false` 恒同、`anchor_blk = 0` 恒同、条目序 = cfg 站序）**逐项相同**；`poll_group` 读的块集 = 全部块（同 `poll_station`）⇒ 读事务序列、上送点、事件序列**全部相同**。
- **情形 (b)**（`regs` 为空）：退化组**仍产 1 条目**（`interval_ms` = 站周期、`next_due` 初值 0，与改造前的该站条目**同值**）⇒ 到期节奏、`delay_station`→`delay_group` 的后移量**同值**；`poll_group` 的读循环 **0 次**（`group_of` = `vec![]`）⇒ `io_error = None`（口已打开）⇒ 该组为承载组 ⇒ `poll_to_result(role, &[])` 与 `mark_success` **照常调用**；`round_signals_group`/`round_station_flags` 在空读集上产空信号 ⇒ **无遥测、无事件**。⇒ 与既有 `poll_station` 空 `regs` 站的行为**逐字等价**（既有单测 `battery_station_without_soc_block_does_not_push` 的断言"`offline`/`online` 各 0 条 + `soc` 不误推"继续成立）。

### 12.4 调度器改造（可直接编码的伪码与签名）

#### 12.4.1 分组划分（**纯函数**，校验期与调度期共用）

```rust
/// 空块集（`regs` 为空）退化组的**哨兵锚**。取 `usize::MAX`：任何真实块下标（`< regs.len()`）
/// 都不可能与之相等 ⇒ 「锚 → 组」仍是**单射**，`(station_index, anchor_blk)` 仍可作稳定 `HashMap` 键。
pub const EMPTY_GROUP_ANCHOR: usize = usize::MAX;

/// 站 → 读组划分。**唯一分组实现**（配置期校验与调度期构造都调它，防两处漂移）。
/// 返回按**组周期升序**（`BTreeMap` 序）。
/// **不变量：对任何 `StationConf`（含 `regs` 为空）至少返回一个组。**
/// - 有块且无块声明 `interval_ms` ⇒ 唯一桶（= 全部块、周期 = 站周期）—— V-5(a)；
/// - **`regs` 为空 ⇒ 一个退化组**（空块集、周期 = 站周期、锚 = `EMPTY_GROUP_ANCHOR`）—— V-5(b)。
///   **不得**返回 0 个组：那会使该站**永不进入 `DueCalc`**（= 被静默移出调度），
///   与既有"空 `regs` 站照常被轮询、读集为空、`poll_to_result(role, &[])` 照常求值、
///   成功/失败记账照旧"的行为**不符**（既有单测 `scheduler.rs::battery_station_without_soc_block_does_not_push`
///   内联构造 `regs: vec![]` 直接 `tick_once`，**不经 `validate`** ⇒ 该输入**可达**）。
pub fn read_groups_of(c: &StationConf) -> Vec<ReadGroup> {
    let mut by_period: std::collections::BTreeMap<u64, Vec<usize>> = Default::default();
    for (i, b) in c.regs.iter().enumerate() {
        by_period
            .entry(b.effective_interval_ms(c.interval_ms))
            .or_default()
            .push(i);
    }
    if by_period.is_empty() {
        by_period.insert(c.interval_ms, Vec::new());   // 退化组：空块集、周期 = 站周期
    }
    by_period
        .into_iter()
        .map(|(interval_ms, blk_indices)| ReadGroup {
            // 键：块 → 组 1:1 ⇒ 锚唯一；空块集取哨兵（**不索引 `blk_indices[0]`**）
            anchor_blk: blk_indices.first().copied().unwrap_or(EMPTY_GROUP_ANCHOR),
            blk_indices,
            interval_ms,
        })
        .collect()
}

/// 站级承载组（PRD §10.3.2 **C8**）：**周期最大**的组；并列取 `anchor_blk` 最小者。
/// 语义：`offline`/`online` 状态事件、`offline_count` 与站级退避都由它承载
/// ⇒ **站离线判定时延与现状一致**（不被快组影响）。
///
/// **返回 `Option<ReadGroup>`（S-1 修订：绝不在该可达输入上 panic）**：
/// `read_groups_of` 的"至少一组"不变量保证实际恒 `Some`（含空 `regs` 站的退化组），
/// 但**即便如此也不得用 `expect`/`unwrap`** —— 空 `regs` 是**可达输入**
/// （配置期只对 `Role::Pcs` 拒空，`config.rs:296-303`；调度器单测直接内联构造），
/// 一旦将来 `read_groups_of` 的不变量被改坏，`expect` 会把"配置错误"变成"进程 panic"。
pub fn carrier_group(c: &StationConf) -> Option<ReadGroup> {
    read_groups_of(c)
        .into_iter()
        .max_by_key(|g| (g.interval_ms, std::cmp::Reverse(g.anchor_blk)))
}
```

> **`.expect` 的理由文案订正（S-1 之三）**：v1.13 曾写"`regs` 非空时必有组；空 regs 由配置期规则 3（pcs）与运行期空读路径分别覆盖"—— 该理由**两条都不成立**：① 配置期对空 `regs` 的拒绝**只**覆盖 `Role::Pcs`（`config.rs:296-303`），**除 `pcs` 与 `battery` 外**的 role 空 `regs` 站**能通过校验**（`battery` 的空 `regs` 另被**规则 4** `soc` 点契约拒，`config.rs:486-488`；**该范围收窄于 v1.13-r2，见 §12.10.2 Δ-12**）；② "运行期空读路径"覆盖的是"口未打开/读失败"，与"空 `regs` 站"无关。⇒ 正确做法就是上面两条：**`read_groups_of` 不变量保证至少一组 + `carrier_group` 返回 `Option`**（无需任何解释性理由来自证不 panic）。
>
> **`anchor_blk` 作键的合法性**：每个块**恰属一个组**，组锚 = 组内最小块下标 ⇒ 组锚 → 组是**单射**；空块集退化为哨兵 `usize::MAX`（无真实下标可与之相等）⇒ 单射性保持，故 `(station_index, anchor_blk)` 唯一标识一个组（也唯一标识该组内的块集 = 配置的纯函数）。

#### 12.4.2 `DueCalc` 改造

```rust
impl DueCalc {
    /// 构造：由本口站组构造。**每站产 `read_groups_of(c).len()` 个条目，且恒 ≥ 1**
    /// （单组站与**空 `regs` 站**各 1 个，同既有；见 §12.4.1 的不变量）。
    fn from_group(group: &[(usize, &StationConf)]) -> Self {
        let mut entries = Vec::new();
        for (idx, c) in group {
            // `carrier_group` 返回 `Option`（S-1）：不变量保证 `Some`；`unwrap_or` 仅作**无 panic 兜底**
            // —— 真取到 `None` 时 `read_groups_of` 亦为空 ⇒ 下面循环不执行，该值不被使用。
            let carrier = carrier_group(c).map(|g| g.anchor_blk).unwrap_or(EMPTY_GROUP_ANCHOR);
            for g in read_groups_of(c) {
                entries.push(DueEntry {
                    key: GroupKey { station_index: *idx, anchor_blk: g.anchor_blk },
                    role: c.role,
                    interval_ms: g.interval_ms,
                    next_due: 0,                 // 首轮全部立即到期（同既有决议）
                    is_carrier: g.anchor_blk == carrier,
                    group_fail_count: 0,
                });
            }
        }
        Self { entries }
    }

    /// 到期判定与推进：**推进/钳制逻辑与既有逐字相同**（`scheduler.rs:146-155`），
    /// 排序键由 `(role_priority, 条目序)` 改为
    /// `(role_priority, station_index, **!is_carrier**, anchor_blk)`（S-3 修订，理由见下注）。
    pub fn due_round(&mut self, now_ms: u64) -> Vec<GroupPoll> {
        let mut due: Vec<usize> = Vec::new();
        for (i, e) in self.entries.iter_mut().enumerate() {
            if now_ms >= e.next_due {
                if now_ms >= e.next_due + e.interval_ms {
                    e.next_due = now_ms + e.interval_ms;   // 落后一轮以上 → 防追跳补采
                } else {
                    e.next_due += e.interval_ms;
                }
                due.push(i);
            }
        }
        // 原序（`i`）→ `(station_index, !is_carrier, anchor_blk)`：
        //  · 单组站：每站恰 1 条目、且恒为该站承载组 ⇒ `!is_carrier = false`、`anchor_blk` 恒同
        //    ⇒ 键退化为 `(role_priority, station_index)`，与既有的 `(role_priority, 条目序)`
        //    **逐项等价**（条目序 = cfg 站序）⇒ V-5 的零行为变化不受影响；
        //  · 多组站：**站内承载组恒排最前**（S-3 修订）—— 使"站恢复当轮的全组基线重建"发生在本站
        //    其它组的产出**之前**（论证见下方注）。
        due.sort_by_key(|&i| {
            let e = &self.entries[i];
            (role_priority(e.role), e.key.station_index, !e.is_carrier, e.key.anchor_blk)
        });
        due.into_iter()
            .map(|i| {
                let e = &self.entries[i];
                GroupPoll {
                    station_index: e.key.station_index,
                    anchor_blk: e.key.anchor_blk,
                    is_carrier: e.is_carrier,
                    was_failing: e.group_fail_count > 0,
                }
            })
            .collect()
    }

    /// 退避（替代既有 `delay_station`，`scheduler.rs:165-174`）：把该组 `next_due` 后移到
    /// `now_ms + extra_ms`（若现 `next_due` 已更晚则不动）。签名与语义逐字沿用。
    pub fn delay_group(&mut self, key: GroupKey, now_ms: u64, extra_ms: u64);

    /// 组级失败计数 +1，**并返回 `(组周期, 加一后的失败计数)`**（= 退避入参；见下注：使
    /// "计数 +1"与"取入参"**原子**，调用方无需二次查表）；键不存在 ⇒ `None`（**不 panic**）。
    pub fn bump_group_fail(&mut self, key: GroupKey) -> Option<(u64, u32)>;
    pub fn clear_group_fail(&mut self, key: GroupKey);

    /// 读该组的 `(组周期, 组级失败计数)` —— 退避公式的入参（替代既有从 `state` 取 `(interval, oc)`）；
    /// 键不存在 ⇒ `None`（**不 panic**）。
    pub fn group_backoff_input(&self, key: GroupKey) -> Option<(u64, u32)>;
}
```

> **为什么 `bump_group_fail` 顺带返回入参（v1.13-r1）**：v1.13 的 `run_port_round` 写作 `calc.bump_group_fail(key);` 紧接 `calc.group_backoff_input(key).unwrap()` —— 与 S-1 同类的"查表失败即 panic"风险（虽然该键必然存在）。改为**返回 `Option` 并在调用点 `if let`**（§12.4.3）⇒ 该段**无 `unwrap`/`expect`**；两个方法都返回 `Option` 且都不 panic。

> **为什么排序键要加 `!is_carrier`（S-3 修订的"同 tick 组序"论证）**：
>
> **问题的来源**：站级全组基线重建的触发者是**承载组**（`poll.is_carrier && station_was_offline`，§12.4.3 的 ① 段）。而承载组是**周期最大**的组，其组锚**不必然最大** —— 例：`hvac` 若把 `hvac_di`（1000 ms）写在配置前面，则快组锚 = 0、承载组锚 = 1 ⇒ 旧的升序键 `(.., anchor_blk)` 会把**承载组排在快组之后**。于是"站恢复的那一 tick"里，快组**先**（在承载组重建全组基线之前）完成产出，承载组**后**才 `reset()` 全组。
>
> **为什么这不可接受（三条，按重要性）**：
> 1. **语义原子性**：站恢复是**站级**事实（"该站全部组基线一并重建"，§12.4.4 连带项 a）。若次序不钉住，则同 tick 内会出现"某组已按其（可能陈旧的）基线产出、随后才被重建"的**半状态**；该组本轮的产出是否成立取决于**配置书写次序**（锚的偶然大小），这不是设计可接受的"两种次序皆正确"。
> 2. **多一次全量落位**：承载组后跑 ⇒ 已经把本轮产出交付出去的组，会在**下一轮**再做一次 `full_snapshot`（该组的位点全量落位，BMS 组 = 288 位）—— 与连带项 a 的"防恢复即刷一屏"取向相悖（量级小，但方向相反）。承载组先跑 ⇒ 全组在**同一轮**完成"重建基线（全量快照、不产事件）"，无多余轮次。
> 3. **钉住"谁能观察到站恢复"**：承载组先跑 ⇒ 它在本轮 `mark_success` 清 `offline_count` 发生在其它组读它**之前**，其它组看到的 `station_was_offline` 恒为 `false` —— 这是**正确**值（站在本轮已恢复），且**不丢失任何重建**：其它组**不需要**该标志（组级重建由 `poll.was_failing` 独立决定，`was_failing` 在 `due_round` 时**已快照**，先于本轮任何组执行；站级重建已由承载组替它们完成）。
>
> **为什么"承载组先跑 + `mark_success` 先清 `offline_count`"不会让同 tick 后来的组误判**：后来组的 `station_was_offline` 只用于 ① 段（且 ① 段已被 `poll.is_carrier` 门控 ⇒ 对它们恒不生效）；它们的组级重建入参 `poll.was_failing` 取自 `due_round` 的快照，与本轮 `offline_count` 的读写时序**完全无关**。⇒ 清零只影响"谁触发站级重建"，而站级重建**已经**由承载组执行完毕。
>
> **对既有行为的影响 = 零**：单组站（含空 `regs` 站）每站恰 1 条目、且它就是承载组 ⇒ `!is_carrier = false` 为常量、`anchor_blk` 亦为常量 ⇒ 排序与既有 `(role_priority, 条目序)` 逐项一致（V-5；用例 `single_group_ordering_unchanged` 钉住）。（**注**：`!is_carrier` 这一细化与 **PRD §10.6 第 3 条**字面元组的差异，已登记为 **Δ-14**。）

#### 12.4.3 `run_port_round` / `poll_group`

```rust
/// 跑一口的一个 tick（now_ms 驱动 due → 逐到期组 poll → 失败组退避）。
/// **结构同既有**（`scheduler.rs:509-544`）：`due` 空则直接返回（**零空转**，
/// 故把 `poll_ms` 降到 500 只增加"到期检查"次数，不增加任何 IO）。
async fn run_port_round(&self, port_i: usize, now_ms: u64) {
    let runner = &self.runners[port_i];
    let due = runner.calc.lock().unwrap().due_round(now_ms);
    if due.is_empty() { return; }

    let mut failed: Vec<GroupPoll> = Vec::new();
    for poll in due {
        let ok = self.poll_group(runner, poll).await;
        let mut calc = runner.calc.lock().unwrap();
        if ok {
            // 显式构造（优化 4 之一：v1.13 曾写 `GroupKey { ..poll.into() }`，那需要一个
            // **未声明**的 `From<GroupPoll> for GroupKey`；与下方 `let key = ..` 同款即可）
            calc.clear_group_fail(GroupKey { station_index: poll.station_index, anchor_blk: poll.anchor_blk });
        } else {
            failed.push(poll);
        }
    }
    if failed.is_empty() { return; }

    // 站级承载组：退避入参 = (**承载组自身组周期**, 站级 offline_count)（非退化配置下 ≡ 站周期，
    //             与既有 scheduler.rs:526-542 同源；口径统一说明见下注）
    // 块级组：     退避入参 = (组周期, **组级** fail_count)（`bump_group_fail` **原子**返回两者）
    // 先一次锁 state 快取承载组的 offline_count，**勿持 state 锁跨 calc 锁**（既有约定）
    let oc_of = |si: usize| -> u32 { self.state.read().unwrap()[si].offline_count };
    let mut calc = runner.calc.lock().unwrap();
    for p in failed {
        let key = GroupKey { station_index: p.station_index, anchor_blk: p.anchor_blk };
        // `key` 必存在（来自本 tick 的 `due_round`，条目由 `from_group` 建成）⇒ 两方法恒 `Some`；
        // 但**一律用 `if let`、不 `unwrap`**（v1.13-r1：与 S-1 同款"查表失败不得 panic"取向）。
        if p.is_carrier {
            if let Some((iv, _)) = calc.group_backoff_input(key) {
                calc.delay_group(key, now_ms, backoff_extra(iv, oc_of(p.station_index)));
            }
            // 承载组失败：站级 offline 记账已在 poll_group 内完成（handle_failure）
        } else if let Some((iv, fc)) = calc.bump_group_fail(key) {
            calc.delay_group(key, now_ms, backoff_extra(iv, fc));
        }
    }
}
```

> **退避入参取"组周期"而非"站周期"（一句说明）**：承载组的退避用**它自己的组周期** `iv`。在**非退化配置**下二者**恒等** —— 因为 C5（块周期 ≤ 站周期）使站周期组就是"周期最大的组"，而 C7 保证 `R(role)` 的块处在站周期组 ⇒ 承载组 = 站周期组（`iv == s.conf.interval_ms`，与既有 `scheduler.rs:526-542` 的 `s.interval_ms` 逐字等价）。它只在**退化配置**（站内全部块都声明了 `interval_ms`，PRD §10.6 第 8 条）下不同 —— 此时按"该组的实际 cadence"退避才是自洽的。

```rust
/// 单组一轮采集：逐**组内块**读 → 语义判定 → 分发 + 调度态更新。
/// **返回该组本轮是否成功**（调用方据此退避）。
async fn poll_group(&self, runner: &PortRunner, poll: GroupPoll) -> bool {
    let si = poll.station_index;
    let key = GroupKey { station_index: si, anchor_blk: poll.anchor_blk };
    let (station_id, role, slave, blk_indices) = {
        let st = self.state.read().unwrap();
        let s = &st[si];
        (s.conf.id.clone(), s.conf.role, s.conf.slave,
         runner.group_of[&key].clone())     // 构造期算好的「组 → 组内块下标」表（免每轮重分组）
    };
    // 「读前」的站离线态（**必须在 `mark_success` 清 `offline_count` 之前取** —— 既有约定，scheduler.rs:620-622）
    let station_was_offline = { self.state.read().unwrap()[si].offline_count > 0 };

    // ── 读循环：与既有 `poll_station` 的逐块读（scheduler.rs:568-606）**逐字相同**，
    //    仅把"全部块"换成"组内块"。首块 Err 即 break（钳制同口 cadence 受损上界，既有决议保留）──
    let mut reads: BlockReads = Vec::with_capacity(blk_indices.len());
    let mut io_error: Option<String> = None;
    if let Some(b) = &runner.bus {
        for &bi in &blk_indices {
            let blk = &self.cfg.stations[si].regs[bi];
            let res = match blk.func {
                RegFunc::Holding  => b.read_holding(slave, blk.addr, blk.count).await.map(mapper::BlockData::Regs),
                RegFunc::Input    => b.read_input(slave, blk.addr, blk.count).await.map(mapper::BlockData::Regs),
                RegFunc::Discrete => b.read_discrete(slave, blk.addr, blk.count).await.map(mapper::BlockData::Bits),
            };
            match res {
                Ok(d) => reads.push((blk.clone(), Ok(d))),
                Err(e) => { io_error = Some(format!("{} @ {:#06x} x{}", e, blk.addr, blk.count)); break; }
            }
        }
    } else {
        io_error = Some("口未打开（open 失败）".into());
    }

    // ── 失败路径（PRD §10.6 第 4 条）──
    if let Some(reason) = io_error {
        if poll.is_carrier {
            self.handle_failure(si, &reason).await;   // 站级 offline 记账 + 事件（既有，零改动）
        } else {
            tracing::warn!(station = %station_id, ?role, group = poll.anchor_blk, reason,
                "southd 块组采集失败（组级退避；不升级为站级 offline）");
        }
        return false;
    }

    // ── 语义判定（**只在承载组上**）——C6/C7 保证 `R(role) ⊆ 承载组`
    //    ⇒ 其读集与"改造前的整站一轮"在 R 相关块上**等价**（其余 role 返回空包，mapper.rs:460-462）──
    if poll.is_carrier {
        match mapper::poll_to_result(role, &reads) {
            PollResult::Failed(msg) => { self.handle_failure(si, &msg).await; return false; }
            PollResult::Data(pkg) => {
                self.mark_success(si).await;
                if role == Role::MeterGrid {
                    self.sink.on_grid_package(pkg).await;
                } else if role == Role::Battery {
                    if let Some(soc) = pkg.battery.soc { self.sink.on_battery_soc(&station_id, soc).await; }
                }
            }
        }
    }

    // ── 遥测 / 事件（承载组与块级组**同一路径**；grid 站不走此路，同既有）──
    //
    // ★★ **守卫的作用域（B-1 修订，2026-09-23）** ★★
    //   `judges_evaluable` **只**门控「**判据 / 站级量**」这一条路径（`StationFlag`：
    //   SOC 域 / 消防地址升序 / 消防登记数交叉校验 —— 这些量**跨块**求值，缺块即**假报**，
    //   见 §12.4.5）；**位 / 标量遥测与事件产出不受它门控**。
    //
    //   反例（旧写法为何是错的）：把守卫套在**整段**遥测/事件上 ⇒ 对 **非承载组**
    //   （battery 的 `bms_alarm` 快组**天然不含** `soc` 块）恒 `false` ⇒ 该组的**全部位/标量
    //   遥测与事件被静默丢弃**（无日志、无事件），既与 §12.5「组内块只喂本组 tracker」
    //   自相矛盾，也使 PRD §10.3.2 明文允许的 "`bms_alarm` 可提速" 失效。
    //   可达性：battery `S = 2000`（§12.6 末 + 02 PRD §9.5.1 对照算式段已登记的档位）
    //   或段级 `poll_ms → 500`（PRD §10.9 Q-21 ③ 活选项）。首例 hvac（`R = ∅`）不暴露该错
    //   —— 故**必须**由 §12.8 的 `non_carrier_group_still_emits_its_bits` 专门钉住。
    if role != Role::MeterGrid {
        // ① 本组块**自身**的信号（离散位 + 字级信号）：只读本组读集、**不跨块求判据**
        //    ⇒ 对**任何**组都安全，**恒产出**（= "块级组照发遥测" 的落点）。
        //    实现：把既有 `round_signals`（`scheduler.rs:311-383`）拆为两半（**机械拆分，不改判据**）：
        //      前者 `round_signals_group` = 既有 `:313-344` 的逐块循环体（位 + 字级信号）；
        //      后者 `round_station_flags` = 既有 `:345-375` 的 `StationFlag` 三项；
        //      既有 `:377-381` 的 `rs.all.extend(station_flags)` 移入下面的 ② 分支（口径不变）。
        let mut signals = round_signals_group(role, &reads);
        // ② 判据 / 站级量：**仅当**「本组 = 站级承载组」**且**「组内齐备 `R(role)`」时求值；
        //    否则**跳过求值**（不是"丢弃本组数据"，而是"本轮不产出站级量"）。
        //    非承载组不含 `R(role)` 的块是**正常形态**（battery 快组 / hvac 位组皆如此）。
        if poll.is_carrier && judges_evaluable(role, &reads) {
            signals.station_flags = round_station_flags(role, &reads);
            // 站级量与前四类**同栏**喂 tracker（既有口径，scheduler.rs:377-381 逐字沿用）
            signals.all.extend(
                signals.station_flags.iter().map(|f| (f.metric.to_string(), f.active)),
            );
        }
        let (events, changed_bits) = {
            let mut trackers = runner.trackers.lock().unwrap();
            // ① **站级恢复** ⇒ **该站全部组**基线重建（§12.4.4 连带项 a）。
            //    ★★ 触发者**只能是承载组**（S-3 修订）★★：`station_was_offline` 单独**不足以**定
            //    触发 —— `offline_count` 只在**承载组**成功时被 `mark_success` 清零
            //    （`scheduler.rs:730-737`），因此当**承载组持续失败**时该标志对非承载组**恒为真**，
            //    若据此触发"全组重建"，则非承载组**每一轮**成功都会把自己的基线重置 ⇒
            //    (i) 该组 `primed` 恒 `false` ⇒ **0→1 变化沿事件永不产出**（与 §12.5 表
            //        "只受本组基线状态与组级失败重建影响"直接矛盾）；
            //    (ii) `full_snapshot` 每轮为真 ⇒ **每轮全量落位**（BMS 288 位/轮 ≈ 2.5×10⁷ 行/天，
            //         正是 PRD §9.8.1 末条明文告警的量级）。
            //    ⇒ 门控 = `is_carrier` **且** 读前 `offline_count > 0`（= 真正的"本轮恢复"）。
            //    ⇒ 承载组在站内**恒排最前**（§12.4.2 的排序键）⇒ 本段的重建**先于**该站其它组的
            //       本轮产出发生，不存在"先产出、后被重建"的半状态。
            if poll.is_carrier && station_was_offline {
                for k in &runner.groups_of_station[&si] {
                    trackers.entry(*k).or_default().reset();
                }
            }
            // ② 组级：本组上一轮失败过 ⇒ 本轮只重建基线（不产事件，§12.5 的重建条件表）
            let tracker = trackers.entry(key).or_default();
            if poll.was_failing { tracker.reset(); }
            // ★ `station_flags` 在非承载组上**恒为空**（上面 ② 分支已跳过求值）⇒ 本行
            //   对非承载组是幂等空操作；**不得**因它为空而短路整段（B-1 修订）。
            tracker.mark_station_flags(&signals.station_flags);
            let full_snapshot = !tracker.primed;              // 判定须在 `edges()` 之前取（既有约定）
            let raw_edges = tracker.edges(&signals.all);
            // …以下与 scheduler.rs:640-657 **逐字相同**（changed 集合 / 站级量后处理 / bits 过滤）…
            (events, bits)
        };
        // …以下与 scheduler.rs:660-685 **逐字相同**（标量全量 + 位点变化沿 → `on_station_telemetry`）…
        //     ★ 该段**不受**守卫门控 ⇒ **非承载组的位/标量遥测照常上送**
        //       （B-1 修订的落点：守卫只少产"站级量"，**不"静默丢弃整段"**）。
    }
    true
}
```

> **为什么"块级组不调 `poll_to_result`"**：`MeterGrid` 分支缺任一相量块即 `Failed`（`mapper.rs:410-429`）、`Battery` 分支要求 `soc` 块在组内（`:437-452`）。对快组调用它会**恒 `Failed` ⇒ 假 offline**。C6/C7 保证 **R ⊆ 承载组**，故"只在承载组求值"与"改造前整站一轮求值"在 R 相关块上**读集相同**。

> **判据路径与非判据路径的边界（B-1 修订的"三句话"）**：
> 1. **`poll_to_result`（分发 + 站级成功/失败记账）** —— 只由 `is_carrier` 门控（既有权衡，不变）；
> 2. **`judges_evaluable`（判据完整性守卫）** —— 只由 `is_carrier && judges_evaluable(..)` 门控 **`round_station_flags`（`StationFlag` 三项）** 的求值；**不得**用来门控位/标量遥测与事件；
> 3. **位/标量遥测 + 变化沿事件** —— **任意组**都可产出，只受"本组 tracker 的基线状态"（§12.4.4）影响；该基线的**重建只有两个来源**：「**本组** `was_failing` 恢复（组级，仅本组）」与「**该站承载组**判定站恢复（站级，该站全部组）」—— **非承载组自身的成功不触发站级全组重建**（S-3 修订；§12.5 的重建条件表）。

#### 12.4.4 变化沿记忆的键（**必须改为组键**，否则组间互相清空）

**问题（机制级，回源）**：`EdgeTracker::prime` 的动作是 **`self.last.clear()` 再插入本轮观测**（`scheduler.rs:237-243`）。若 tracker 仍按**站**存：

1. t=0 快组（`hvac_di`）→ 喂入 31 个位信号 → `prime`：`last` = {31 位}（**清掉了标量的记忆**）；
2. t=5000 慢组（`hvac_in`）→ 喂入 3 个标量信号 → `prime`：`last` = {3 标量}（**清掉了 31 位的记忆**）；
3. t=1000 快组再读 → 位信号的 `last` 查不到 → 走 `edges()` 的 `_ => {}` 分支（未知不是跃迁 ⇒ **不产事件**）⇒ **位块在 t=5000 之后的第一次真实跳变会被静默吞掉**，且此后每 5 s 复发一次。

⇒ **tracker 键必须由 `usize`（站）改为 `GroupKey`**（`scheduler.rs:284-285`），使每组持有**独立**的"上轮活跃态"记忆。这与既有"`PortRunner` 按站下标各持一个"的注释（`:284`）是同一意图的**粒度细化**。

**连带的两条**：

| # | 规范 | 理由 |
|---|------|------|
| a | **站恢复 ⇒ 该站全部组一并 `reset()`；且"站恢复"只由承载组判定**（S-3 修订） | 既有 `poll_station` 在 `recovered` 时只 `reset()` 当前（唯一）tracker（`scheduler.rs:634-636`）。有了多组，若只重置"先成功的那一组"，其余组会拿**离线前**的基线比对 ⇒ 恢复即刷一屏事件（`online` 之后紧跟一堆位事件）。实现：`PortRunner.groups_of_station` + §12.4.3 伪码 ① 段的遍历，**门控 = `poll.is_carrier && station_was_offline`**。<br>**为什么门控必须有 `is_carrier`**：`offline_count` 只在承载组成功时清零 ⇒ 承载组持续失败时该标志对非承载组恒真；若任由非承载组触发全组重建，其基线每轮被重置 ⇒ 该组**永不产变化沿事件**且**每轮全量落位**（§12.4.3 ① 段的两条后果）。<br>**为什么还要"承载组站内排最前"**：见 §12.4.2 的排序键论证（重建必须**先于**本站其它组的本轮产出发生，否则同 tick 出现"先产出、后被重建"的半状态，且该组下一轮多做一次全量快照） |
| b | **首轮/组恢复后只建基线、不产事件** | 沿用既有口径（`primed = false` ⇒ `edges()` 只 `prime()` 返回空，`scheduler.rs:255-265`）；**唯一例外**仍是已登记的 `StationFlag`（首次观测即产）——该例外**不需要**跨组推广（`StationFlag` 的判据块恒在承载组，§12.4.5） |

> **全章一致的"谁能重建基线"口径（S-3 之四）**：① **站级（该站全部组）重建 —— 唯一触发者 = 承载组**（条件：读前 `offline_count > 0` 且本轮成功）；② **组级（仅本组）重建 —— 触发者 = 本组**（条件：`poll.was_failing`，即本组上一轮失败过）。**不存在**"任一组成功即全组重建"或"站恢复即由任意组触发全组重建"的写法（§12.5 重建条件表 / §12.4.3 ① 段 / §12.10.1 第 8 项 / §12.11 T9 / §12.12 均已按本条统一）。

#### 12.4.5 判据完整性守卫（运行期对偶，纵深防御）

**作用域声明（B-1 修订，2026-09-23）**：本守卫**只作用于「判据 / 站级量」路径**
（`round_station_flags` 求出的 `StationFlag` 三项：SOC 域 / 消防地址升序 / 消防登记数一致性），
**调用点唯一**且在 §12.4.3 伪码中写作 `if poll.is_carrier && judges_evaluable(role, &reads) { … }`。

> ⚠️ **本守卫的返回值不得用于门控"位 / 标量遥测与事件"**。旧版本设计把它套在**整段**遥测/事件上，
> 并自述"合法配置下恒真"——**该自述为假**：守卫的**入参是"一个读组"的读集**，而
> **非承载组天然不含 `R(role)` 的块**（battery 的 `bms_alarm` 快组不含 `soc` 块、
> hvac 位组不含标量块）。在这些**合法**组上 `judges_evaluable` 恒 `false` ⇒ 旧写法会
> **静默丢弃该组全部位/标量遥测与事件**（既无日志、也无用例覆盖），并使
> PRD §10.3.2 明文允许的 "`bms_alarm` 可提速" 失效。正确陈述是：
> **「在"承载组"这一作用域内、且配置合法时，守卫恒 `true`（V-2 由配置期保证）；
> 在非承载组上它**不适用**，不是"返回 false"」。**

```rust
/// **组内是否齐备"站级判据所需的块"**（= PRD §10.3.2 的 `R(role)`）。
///
/// **作用域（B-1 修订）**：**只**管「判据 / 站级量」路径（`round_station_flags`）；
/// **不**管位/标量遥测与事件（后者对任意读组都安全，见 §12.4.3 的"三句话"）。
/// 调用点唯一：`if poll.is_carrier && judges_evaluable(role, &reads) { round_station_flags(...) }`。
///
/// **在承载组作用域内、配置合法时恒 `true`**（V-2 由配置期 C6 保证）；
/// 本守卫是**纵深防御**，为两种"配置期保证失效"的场合兜底：
/// ① 配置校验被绕过（直接构造 `StationConf` 交给调度器的既有用法，见
///    `scheduler.rs::battery_station_without_soc_block_does_not_push` 的注释）；
/// ② 将来若放开 C6/C7（PRD §10.9 **Q-22** 选项 B），**必须**先让本守卫生效 —— 否则
///    `reads = {fire_sys}` 时 `fire_detector_mismatch` 会**假报**：`read_back = 20`、
///    `capacity = 0 + 1 = 1`（`fire_det*` 不在组内；`read_back` 取数见 `mapper.rs:363-372`，
///    **容量算式**见 `mapper.rs:377-385` 的 `groups + u32::from(fire_chain_head(..).is_some())`）⇒ `20 ≠ 1`。
///
/// **守卫失败时的行为 = "跳过求值"，不是"丢弃本组数据"**：本轮不产出任何站级量事件
/// （宁可静默，不得用部分读集臆断）；本组的位/标量遥测**照常产出**。
fn judges_evaluable(role: Role, reads: &BlockReads) -> bool {
    match role {
        // p/q/pf/u/i 齐备（`poll_to_result` 的 MeterGrid 分支，mapper.rs:409-432）
        Role::MeterGrid => ["p", "q", "pf", "u", "i"].iter()
            .all(|n| reads.iter().any(|(b, r)| b.name == *n && r.is_ok())),
        // `soc` 点所在块在组内（`battery_soc` 按点名查找，mapper.rs:250-...）
        Role::Battery => !matches!(mapper::battery_soc(reads), mapper::SocOutcome::NoSuchPoint),
        // 链首（覆盖寄存器 11 的**寄存器块**）+ 至少一个 `fire_det*` 块（两判据共用，mapper.rs:326/359/565）
        Role::Fire => fire_head_present(reads) && reads.iter().any(|(b, _)| b.name.starts_with("fire_det")),
        // MeterBatt / Hvac / Pcs：`poll_to_result` 返回空包（mapper.rs:460-462），无判据 ⇒ 恒真
        Role::MeterBatt | Role::Hvac | Role::Pcs => true,
    }
}
```

> **实现注意**：`fire_head_present` 的判据与 `mapper::fire_chain_head`（`mapper.rs:326-336`）**同源**（"存在读成功的**寄存器块**覆盖寄存器 11"，即 `addr ≤ 11 < addr + count` 且 `res.regs()` 可取）；**不得**改写成"块名 == fire_sys"（那属设备特判，违反 G-5）。`Role::Battery` 的守卫复用 `mapper::battery_soc` 的返回枚举，不另造判据。

### 12.5 事件、失败与活性语义（落地 PRD §10.6 第 4/5 条）

| 事件/状态 | 承载者 | 语义 |
|-----------|--------|------|
| 站级 `offline` / `online` | **站级承载组**（C8） | 与既有**逐字相同**：组内任一块读 `Err` 或 `poll_to_result::Failed` ⇒ `handle_failure`（`offline_count` +1、按 `stale_timeout_s` 窗口去抖后产一次事件、`reason` 含 `slave/addr/count`）；成功 ⇒ `mark_success`（`online` 一次 + 归零） |
| 站级退避 | **站级承载组** | `backoff_extra(**承载组自身组周期**, 站级 `offline_count`)` —— **口径与 §12.4.3 的注统一**（v1.13 此处曾写"站周期"，与 §12.4.3 注的"组周期"并存 ⇒ 优化 4 之三已统一为**组周期**）：**非退化配置**下 `组周期 ≡ 站周期`（C5+C7 ⇒ 承载组 = 站周期组）⇒ 与既有 `scheduler.rs:536-541` **逐字等价**；只在**退化配置**（站内全部块都声明了 `interval_ms`，PRD §10.6 第 8 条）下二者不同，此时按该组**实际 cadence** 退避才自洽 |
| 块级（快采）组失败 | **无事件** | ① `warn` 日志（含 `station/role/块名/reason`）；② **组级**计数 + 按**组周期**指数退避（`backoff_extra(组周期, group_fail_count)`，封顶 32× 同既有 `MAX_BACKOFF_SHIFT`）；③ `offline_count` **不自增**、`offline`/`online` **不产** |
| 变化沿输入 | **组级 tracker** | 组内块只喂**本组** tracker（§12.4.4）；**各组的基线互不可见**（故非承载组不会清空承载组的记忆，反之亦然） |
| **位 / 标量遥测 · 变化沿事件产出** | **任一读组**（承载组与块级组**同一路径**） | **非承载组照常产出** —— **不受** `judges_evaluable` 门控（B-1 修订；§12.4.3 的"三句话"第 3 条）。产出内容 = 本组块自身的位（变化沿 / 首轮全量）+ 标量（每轮全量），同既有 D2 口径。**基线的重建只受两个来源影响**：「本组 `was_failing`」与「**该站承载组**判定站恢复（全组，S-3 修订）」—— **非承载组自身的成功/失败不触发站级全组重建**（否则其基线每轮被重置 ⇒ 0→1 变化沿永不产出 + 每轮全量落位） |
| **站级派生量 `StationFlag`**（SOC 域 / 消防地址序 / 消防登记数） | **站级承载组**（且组内齐备 `R(role)`） | **唯一**求值点：`if poll.is_carrier && judges_evaluable(..)`。非承载组**跳过求值**（不臆断、不产事件）——该量的判据**跨块**，用部分读集求值会**假报**（§12.4.5） |

**"块级组失败不得升级为站级 offline"的两条硬理由（不得违反）**：

1. **否则会隐藏正在正常上送的快采告警位**：站被判 offline ⇒ 12 号 F25.4 规定该站**全部**点显示 `--` + 「站离线」（12 PRD F25.4 第 3 条），把**刚刚成功采到并上送的告警位**一并遮蔽 —— 与本能力的诉求（告警位 ≤2 s 可见）**直接冲突**。
2. **否则会形成周期性事件对刷屏**：`mark_success` 会把 `last_offline_event` 清 `None`（`scheduler.rs:737`），恰好**重置 offline 去抖窗口** ⇒ "快组成 / 慢组败"交替时，每轮都会"`online` 1 条 + `offline` 1 条"（以 5 s 承载周期计 ≈ **3.5 万条/日**，且全都指向同一物理事实）。

**组级基线的重建条件（两条，取"或"）—— 唯一的"谁能重建"口径（S-3 修订），全章一致**：

| 触发（**谁能重建**） | 范围 | 依据 |
|------|------|------|
| 组级：**本组** `poll.was_failing == true`（= 本组上一轮失败过）且本轮成功 | **仅该组** | 与服务体"恢复后现势值连续性不可假设"同一取向（`scheduler.rs:620-621`）；避免用离线前基线产出陈旧跃迁 |
| 站级：**该站承载组**（`poll.is_carrier`）且**读前** `offline_count > 0`、且本轮成功 | **该站全部组** | §12.4.4 的连带项 a（防"恢复即刷一屏事件"）。**触发者被限定为承载组** —— `offline_count` 只在承载组成功时清零（`mark_success` 只在承载分支调用），若允许非承载组按该标志触发，则承载组持续失败期间非承载组会**每轮**把全组基线重置（⇒ 变化沿永不产出 + 每轮全量落位） |

> **两个来源的集合关系（消除"谁重建"的歧义）**：站级重建（承载组、范围 = 全站组）**真包含**承载组自身的组级需求；非承载组若在站离线期间**也在失败**，则其自身的 `was_failing` 规则已保证"恢复后只建基线"（**不依赖**站级重建）；若它在站离线期间**一直成功**，则其基线**本就是新鲜的**，无需重建。⇒ 两条规则合起来覆盖全部情形，且**任何一条都不会让一个健康的非承载组每轮重置**。

**已知盲区（如实登记，不粉饰）**：块级组失败期间，其位点在 01 号 `latest_values` 中**仍标 `Ok`**（位点可得性 = 站级活性 ∧ 点位质量，而站级活性由承载组刷新）⇒ 由 `warn` 日志暴露，属跨文档待裁项（PRD **§10.9 Q-23**）。**本章不在 02 号侧新造第二套新鲜度判据**（12 PRD F25.1 同款禁令）。

### 12.6 带宽与单轮耗时（按 §9.8.1 的既有口径重算）

**估时公式（沿用 PRD §9.8.1 的常数与结构，不另立口径）**：1 字节 = `10 / baud_rate` 秒（9600 bps ⇒ **1.04 ms**）；事务字节数 = 请求 **8** + 响应 **`5 + D`**，其中 **`D` = 响应中的"数据字节数"**（FC02 读离散输入：`D = ceil(位数/8)`；FC03/FC04 读寄存器：`D = 2 × 寄存器数`）；每事务另加 **4 ms** 从站周转。

> **字节耗时的取整口径（v1.13-r3 追认）**：上式 1 字节耗时按 **2 位小数**取整（9600 ⇒ `10/9600 × 1000 = 1.0416…` ⇒ **1.04 ms**），本章表内**全部**数字（`21.68` / `25.84` / `267.12` / `801.36` / `1202.04`）均按该值复算 ⇒ **该取整是口径的一部分**，不是实现细节：若改用未取整的 `1.0416667`，AC-8-5 的 C4 一例会算成 `1.5 × T_组 = 1203.94`，与 PRD AC-8-5「文案含 `1202`」的机械判据**不符**。对非 9600 波特率（如 `pcs` 站的 19200）该取整引入 ≤0.5% 的估计偏差（估时量，不影响任何判据）。实现证据（T8）：`config.rs` 的 `byte_time_ms` 及其"为什么必须取 2 位小数"注 + 用例 `tx_time_uses_station_baud_rate`（9600 ⇒ `21.68`、19200 ⇒ `14.92`）。
>
> **算式订正（优化 3 之二）**：v1.13 写"响应 `(5 + 2N)`（FC02 时 `N = ceil(位数/8)` **字节**）"—— 该句与**同一张表**的 `FC02 = 8 + (5+4) = 17` **不自洽**（按该句应为 `5 + 2×4 = 13` ⇒ `8 + 13 = 21`）。FC02 的响应帧结构 = `slave(1) + func(1) + byte_count(1) + D + CRC(2)` = **`5 + D`**（`D = ceil(位数/8)`）；FC04 = `5 + 2×寄存器数`（= 原式 `2N` 的 `N` 取"寄存器数"）。改正后与表内 `17` / `21`、以及 `T_快组 = 17 × 1.04 + 4 = 21.68 ms` / `T_慢组 = 21 × 1.04 + 4 = 25.84 ms` **逐位一致** ⇒ **PRD §10.4 的任何数字都无需改动**（PRD §9.8.1 的常数 `10 bit/字节` 与 `4 ms` 周转亦未动）；PRD §10.4 中**同一句的 `(5+2N)` 文字**偏差（文档级、不影响其表内数字）已登记为 **Δ-13**。

**首例（`hvac` 站 = `/dev/ttyS3`，9600 **8E1**（`parity: even`，生效配置 `mupc_core_config.production.yaml:398`），**独占一口**）**：

> **比特/字节口径声明（优化 3 之一，防新写错事实）**：本表沿用 **PRD §9.8.1 的 `10 bit/字节` 口径**（1 起始 + 8 数据 + 1 停止），而生效配置的 hvac 站为 **8E1 = 11 bit/字节** ⇒ 按 8E1 精确复算为：`T_快组 = 17 × 11/9600 + 4 = 23.5 ms`、`T_慢组 = 21 × 11/9600 + 4 = 28.1 ms`、整站 `Σ T = 51.5 ms`、`U_后 = 23.5/1000 + 28.1/5000 = 0.0291 ⇒ 2.91 %`、`U_前 = 51.5/5000 = 1.03 %`（倍数仍 **2.8×**）。**结论不变**：C4 `1.5 × 23.5 = 35.2 ms ≤ 1000` ✓ / `1.5 × 28.1 = 42.1 ms ≤ 5000` ✓；C9 `2.91 % ≤ 50 %`（余量 **17.2×**）；规则 24 `Σ T = 51.5 ms ≤ 1.5 × 1000 = 1500 ms` ✓。**本表保留 `10 bit/字节` 口径**的唯一理由是**与 PRD §9.8.1 / §10.4 逐位同源**（§9.8.1 公式即该口径；若改口径则 PRD 全表数字需连动，超出本章授权）。

| 组 | 块（PRD §9.4.1 取值） | func | `D`（数据字节） | 帧字节 | **T_组** | 组周期 | 占用率 |
|----|------------------------|------|---|--------|----------|--------|--------|
| 快组 | `hvac_di`（`addr: 0`, `count: 31` 位, **`interval_ms: 1000`**） | FC02 | `ceil(31/8) = 4` | `8 + (5+4) = 17` | `17 × 1.04 + 4 =` **21.68 ms**（≈**22 ms**） | 1000 | **2.17 %** |
| 站周期组（= **承载组**，C8） | `hvac_in`（`addr: 0`, `count: 4`, `points` 3 点, 未声明） | FC04 | `2 × 4 = 8` | `8 + (5+8) = 21` | `21 × 1.04 + 4 =` **25.84 ms**（≈**26 ms**） | 5000 | **0.52 %** |
| **合计** | | | | **38** | **47.5 ms**（两组同刻到期的上界） | — | **`U_口 = 0.02168 + 0.005168 = 0.026848` ⇒ 2.68 %** |

- **改造前**：整站一轮 = `38 × 1.04 + 2 × 4 = 47.52 ms`（**同一批事务**，只是每轮都做）⇒ 占用率 **`U_前 = 47.52 / 5000 = 0.95040 % ⇒ 0.95 %`**（与 §9.8.1 的 "≈1%" 一致）。
  > **占用率口径统一（评审订正，2026-09-23）**：**统一取"按本表公式复算的精确值"**（与"改造后"的 `0.026848` 完全同口径）：
  > `U_前 = 47.52 / 5000 = 0.0095040` ⇒ **0.95 %**；`U_后 = 0.026848` ⇒ **2.68 %**；倍数 `0.026848 / 0.009504 = 2.8248 ≈ 2.8×`。
  > **不得**改用 §9.8.1 的**取整值** `48 ms`（那会得 `48/5000 = 0.96 %`，与"改造后"的精确口径**不同源**，且使倍数算成 2.79×）。⇒ PRD §10.4 表中原写的 `≈0.96 %` 已在同轮订正为 **`≈0.95 %`**（02 PRD §10.4，v1.13）。
- **改造后**：占用率 **2.68 %**（精确 0.026848；**2.8×**），单轮耗时**不变**（47.5 ms），最坏同刻叠加仍 `≪ 1000 ms` ⇒ **无积压**；C4 下界：快组 `1.5 × 21.68 = 32.5 ms ≤ 1000` ✓，慢组 `1.5 × 25.84 = 38.8 ms ≤ 5000` ✓；C9 上界 `0.026848 ≤ 0.5` ✓（**余量 18.6×**）；**规则 24**（PRD §10.5 第 3 行，W-1 新增）：本口两组 `Σ T_组 = 21.68 + 25.84 = 47.52 ms ≤ 1.5 × 最小非零组周期 = 1.5 × 1000 = 1500 ms` ✓（**余量 31.6×**）。
- **三档敏感性**（对应 PRD §10.9 **Q-21**）：2000 ms ⇒ `U = 1.60 %`；**1000 ms（推荐）** ⇒ `2.68 %`；500 ms ⇒ `4.85 %` **但须把段级 `poll_ms` 1000 → 500**（C3 网格对齐；`poll_ms` 是段级字段，`mupc_core_config.production.yaml:143`）。
- **口预算复核（证明 C9 不拒既有配置）**：`battery` 286/1000 = 0.286、`meter_batt` 310/1000 = 0.310、`fire` 300/1000 = 0.300、`pcs` 90/1000 = 0.090、`hvac`（改造后）0.0268 ⇒ **全部 ≤ 0.5**（最大 = `meter_batt` 0.310）。`grid_meter` 的 `T` 本 PRD/设计**均未复算**（§9.8.1 该行标"既有"）⇒ 见 PRD §10.9 **Q-21 ③**（补算后纳入断言）。

**配置迁移（首例，唯一需改的生效配置行）**：

```yaml
# mupc/deploy/config/mupc_core_config.production.yaml（hvac 站，399 行附近）
      regs:
        - name: hvac_in
          …
        - name: hvac_di          # FC02 位 0–30（PLC 10001–10031）
          func: discrete
          addr: 0
          count: 31
          interval_ms: 1000      # ← 本能力唯一新增行（PRD §10.3.2 C1–C5 全通过）
```

> **其余 5 站零改动**（原因见 PRD §10.4 末表：`meter_grid`/`fire` 的判据块受 C6/C7 约束不支持提速；`battery`/`meter_batt`/`pcs` 站周期已是 1000 ms、无收益）。

### 12.7 配置期校验（落点表）

**新增落点函数（单一入口，避免散落）**：`validate_block_intervals(cfg: &SouthStationsConfig) -> Result<(), String>`，在 `SouthStationsConfig::validate`（`config.rs:208-333`）的既有**判定顺序**中插入为 **②′**：

```
① 站级基础校验（既有，原地不动，含规则 18/19 的空间）
   + 既有 meter_grid 整组校验（**必须在 ② 之前**，理由见 §11.5.3.4.1 — 不得调整）
② validate_station_regs（既有）
②′ **validate_block_intervals（本章新增；须在②之后、③之前）**   ← 新增
③ 跨站（既有：单站约束计数 / 同口一致性 / 极大性）
```

> **为什么 ②′ 必须排在②之后**：②（含点展开）会先给出**更具体**的文案（点位越界/重叠/点名重复等）；若先跑 ②′ 的"判据完整性"检查，可能对同一份坏配置先报出"R(role) 块异周期"这类**次生**结论，破既有回归锚的文案断言（同 §11.5 的"顺序钉住"理由）。

| # | PRD 约束 | 落点 | 判据（可机械判定） | 文案要求 |
|---|----------|------|--------------------|----------|
| **规则 20** | C1 + C2 + C4 | 同上 | 逐块：`iv == 0` → `Err`；`iv < BLOCK_MIN_INTERVAL_MS(500)` → `Err`；`iv < 1.5 × T_组` → `Err`。`T_组` 按 §12.6 公式用本站 `baud_rate` 复算。**仅当块显式声明了 `interval_ms` 时才判**（`None` ⇒ 继承站周期，站级已有规则覆盖） | 含**站 id + 块名 + 实际取值 + 期望下界**（C4 分支的文案须写 `1.5 × T_组` 的**计算值**，见下注） |
| **规则 21** | C3 + C5 | 同上 | `iv % cfg.poll_ms != 0` → `Err`；`iv > 站 interval_ms` → `Err` | 同上（C3 须写出 `poll_ms` 当前值） |
| **规则 22** | C6 + C7（V-2/V-3） | 同上 | 按 `R(role)`（§12.4.5 同表）取该站所需块的下标集；若其 `eff` **不唯一** → `Err`（C6）；若其 `eff < 站 interval_ms` → `Err`（C7） | 含站 id、`role`、**冲突的两个块名与其取值** |
| **规则 23** | C9（口预算） | 同上（按 `port` 聚合） | `U_口 = Σ_组 (T_组 / 组周期) > 0.5` → `Err`（`T_组` 按 §12.6 公式；`组` 由 `read_groups_of` 给出） | 含 **port + 计算出的 U 值 + 触发的组（站 id / 块名 / T / 周期）**（**不写** `1.5 × T_组` —— 该值只由规则 20 的 C4 分支写，故"文案含 `1202`"机械证明触发者是 C4） |
| **规则 24** | **PRD §10.5 第 3 行**（"单轮最坏耗时"：同口全部到期组串行执行，最坏 `Σ T_组 ≤ 最小非零组周期的 1.5 倍"） | 同上（按 `port` 聚合） | `Σ_{组 ∈ 本口} T_组 > 1.5 × min{ 组周期 \| 本口全部组 }` → `Err`（`T_组` 按 §12.6 公式；`组` 由 `read_groups_of` 给出；各站用**本站** `baud_rate`，同口 `baud_rate` 已由既有规则 16 强制一致）。"**非零**"字样逐字沿用 PRD（V-4/站级校验保证组周期恒 `> 0`，此处作防御性表述） | 含 **port（口名）+ 站 id + 实测 `Σ T_组` + 阈值 `1.5 × 最小非零组周期`（并给出该最小周期的取值与来源组）** |

> **`T_组` 的定义写死（S-2 之三；消除复审点出的"两种读法"）**：规则 20 与规则 23/24 中的 `T_组` 一律指 **"**该块所在读组**的整组耗时"**（= 该组全部块的事务耗时之和，逐事务按 §12.6 公式累加），**不是**"单块耗时"。定义**无循环**：同一读组内所有块的 `eff` 相同（V-1）⇒ **组由 `eff` 唯一确定** ⇒ "该块所在读组"在比较之前就已由 `read_groups_of` 确定，故 `T_组` 与"正在校验的那一块"一一对应、可直接算出（含**单块组**时 `T_组 = T_块` 的退化情形，与 §12.6 的 C4 一致）。
>
> **多约束同时违反时报哪一条（判据顺序，S-2 之一的前提）**：`validate_block_intervals` **按规则号升序逐条求值，首个失败即 `Err` 返回**（同一规则内部亦按"`iv == 0` → `< 500` → `< 1.5×T_组`"顺序）。⇒ AC-8-5 的"文案含 `1202`"能**机械证明触发者是规则 20 的 C4 分支**（若先判规则 23，则报的是 `U` 值、文案里没有 `1202`）。

**常量**：

```rust
// config.rs —— 与既有 PCS_MIN_INTERVAL_MS 同源同值（PRD §10.3.2 C2）：
/// "最快允许轮询节奏"的**唯一常量**（站级 pcs 下界与块级下界共用，防双定义漂移）
pub const MIN_POLL_INTERVAL_MS: u64 = 500;
/// 既有名保留为别名（**不得删除**：既有单测与文档引用它）
pub const PCS_MIN_INTERVAL_MS: u64 = MIN_POLL_INTERVAL_MS;
```

**C8 不设配置期拒绝**（PRD §10.3.2 C8 的"确定性规则"）：站级承载组由 `carrier_group()` 按"周期最大、并列取锚最小"唯一确定；"站内全部块都声明了 `interval_ms`"⇒ 承载组 = 周期最大的组，须给**配置异味提示**（不产事件、不拒配置）。
> **提示的判定与发射必须分家（v1.13-r3 订正；原写"加载期 `debug` 日志提示"在实现上不可达）**：
> - **判定** = 纯函数 `block_interval_hints(&SouthStationsConfig) -> Vec<String>`（`mupc-southd::config`，每站一条，**不拒绝**）；
> - **发射点** = startup 装配期（`mupc-core-bin/src/startup.rs` 的南向站装配分支；`startup::initialize_all` 在 `main.rs:187` 调用，**晚于** `tracing_subscriber::try_init()` 的 `main.rs:164`）；
> - **为什么不能在配置期发射**：`CoreConfig::validate` 属 main **Phase 1**（`main.rs:105`），早于 tracing 初始化 ⇒ 配置期日志**无订阅者、被直接丢弃**（与 `core_config.rs:512-514` 的既有成文约定同源："判定放配置期、发射放 startup 装配期"）；
> - **实现证据（T8）**：`config.rs` 的 `block_interval_hints` + `startup.rs:1394` 的发射循环 + 用例 `all_blocks_declared_is_accepted_with_debug_hint` 的**双向断言**（全声明 ⇒ 恰 1 条含站 id；部分声明 ⇒ 空列表，且先断言该反例本身合法）。

**规则 24 的正当性证据（W-1：C9 **不能**蕴含它）**：

| 项 | 构造 | 复算 |
|----|------|------|
| 组 A | 组周期 **1000**、`T_组 = 19.6 ms` | C4：`1.5 × 19.6 = 29.4 ≤ 1000` ✓ |
| 组 B | 组周期 **5000**、`T_组 = 8 × 267.12 = 2137 ms`（如 §12.8 的 C4 构造：8 个 FC04 `count: 120` 块同组） | C4：`1.5 × 2137 = 3205.5 ≤ 5000` ✓ |
| **C9**（规则 23） | `U_口 = 19.6/1000 + 2137/5000 = 0.0196 + 0.4274` | **0.447 ≤ 0.5 ⇒ 通过** |
| **PRD §10.5 第 3 行**（规则 24） | `Σ T_组 = 19.6 + 2137 = 2156.6 ms` vs `1.5 × min{1000, 5000} = 1500 ms` | **2156.6 > 1500 ⇒ `Err`** |
| 实际后果（若不设本条） | 慢组**独占同口 ≈ 2.16 s**（一轮内串行做完 8 个重事务） ⇒ 快组的 `1000 ms` cadence 被打坏，且 `U ≤ 0.5` 对此**完全不敏感** | ⇒ **规则 24 不是 C9 的推论**，必须独立落地 |

**规则 24 的现网复核（证明它不构成对既有 6 站的新增拒绝）**：

| 口 | 本口的组（`Σ T_组`） | 实测 `Σ T_组` | 最小非零组周期 | 阈值 `1.5 ×` | 结论 |
|----|----------------------|---------------|----------------|--------------|------|
| `ttyS3` | `hvac`：快组 21.68 + 站周期组 25.84 | **47.52 ms** | 1000 | 1500 | ✓（余量 31.6×） |
| `ttyS2` / `ttyS5` / `ttyS6` / `ttyS7` | `battery` / `meter_batt` / `fire` / `pcs` 各**单组** | 286 / 310 / 300 / 90 ms（§9.8.1 表值） | 1000 | 1500 | ✓（各站远低于阈值） |
| `ttyS4` | `grid_meter`（**单组**） | 未复算（`§9.8.1` 该行标"既有"）⇒ **本章不擅填** | 1000 | 1500 | **待补**（口径同 **Δ-11** / PRD §10.9 Q-21 ③；补算后一并纳入断言） |

> **与 C9 的关系（防误读为"对现网的新增拒绝"；v1.13-r3 订正理由）**：在**口内仅一组**时，`U_口 = T_组 / 组周期`，而 C9 的触发条件是 `U > 0.5` ⟺ `T_组 > 0.5 × 组周期`，本条是 `T_组 > 1.5 × 组周期`；因 `0.5 < 1.5`，**本条的违规域真包含于 C9 的** ⇒ 该情形下本条**永不成为首个错误**。
> ⚠️ **不得用 C4 推"单组站自动成立"**（v1.13 旧写法如此，v1.13-r3 订正）：C4 只判**显式声明**了 `interval_ms` 的块，未声明时 **C4 根本未求值**。⇒ 本条只在**同一口内多于一个组**（同站多组，或同口多站）时**可能**产生新增拒绝；现网 6 站中未声明块级周期的 4 站（`battery`/`meter_batt`/`fire`/`pcs`，C4 不求值）其单轮 `T_组` 亦远小于 `1.5 × 站周期`（上表已复算）⇒ **现网零新增拒绝**。

### 12.8 测试策略

| 层 | 用例（`scheduler.rs` 的 `mod tests`，复用既有 `MockBus` / `FakeSink` / `tick_once`） | 钉住的判据 |
|----|------------------------------------------------------------------------------------------|------------|
| 单测·调度 | `block_interval_overrides_station_period`：首例配置 + 确定性 tick `0,1000,…,9000` ⇒ `bus.bit_call_count(1, 0) == 10` **且** `bus.input_call_count(1, 0) == 2` | **AC-8-1**（可观测判据 ①；`MockBus` 已有 `bit_call_count`/`input_call_count`，`port_runtime.rs:257/280`） |
| 单测·调度 | `scalar_still_follows_station_period`：同 tick 序下 `sink.telemetry_of("hvac")` 的 3 个标量点各 **2 次** | **AC-8-2**（判据 ②） |
| 单测·调度 | `no_block_interval_is_bit_identical_to_legacy`（**AC-8-3 已钉 tick 序与计数**）：取首例配置**去掉** `hvac_di.interval_ms` ⇒ 单组站（`eff` 全 = 5000）；**确定性 tick 序列 `t = 0,1000,…,9000`（10 tick）** 下逐项断言：① `bus.bit_call_count(1, 0) == 2`、`bus.input_call_count(1, 0) == 2`（单组每 5000 ms 到期一次）；② `sink.telemetry_of("hvac")` 的**调用次数 = 2**（每轮 1 次；位块无变化 ⇒ 第 2 轮无位点）；③ 第 1 次调用项数 = **34**（3 标量 + 31 位，**首轮全量快照**）、第 2 次 = **3**（标量全量 + 无变化位）；④ `event_count("hvac", *) == 0`（首轮只建基线）<br>—— 该四项与**改造前**的既有断言**逐条相同**（"一个字节都不动"） | **AC-8-3**（V-5）+ 既有 **44 例**（`scheduler.rs`：`#[tokio::test]` **38** + `#[test]` **6**；**断言一字不改**） |
| 单测·调度 | `bit_change_event_within_block_period`：位 10 由 0→1（`put_bits`），下一 tick 即产事件（`is_event = true`，无新增 metric） | **AC-8-4** |
| 单测·调度 | `edge_memory_is_per_group`：**交错 tick**（快组 t=0/1000/2000…、慢组 t=5000）下位块的 0→1 **必须**产事件（若 tracker 键未改 ⇒ 静默吞掉 ⇒ 本用例**必红**） | §12.4.4 的机制钉子 |
| 单测·调度 | **`non_carrier_group_still_emits_its_bits`（B-1 回归锚，2026-09-23 新增）**：**本用例自建站（slave = 1）**，与既有 `battery_*` fixture 的 `slave = 2`（`scheduler.rs:838/874`）**无关**（勿照抄本用例的 slave 值去改既有 fixture）；以 **`battery` 站**构造"**非承载组**"——站 `interval_ms: 2000`（该档位在 02 PRD §9.5.1「对照算式」段已登记为 `battery` 的回退周期，**`:1772`**（同义见 `:2212`）；且站级校验 `interval_ms < 5000` 通过 ⇒ **合法可构造**）、`bms_alarm`（FC02，`addr 200`，`count 288`）声明 `interval_ms: 1000`、含 `soc` 点的 `bms_io`（FC04，`addr 100`，`count 31`）**不声明**（`eff = 2000` = **承载组**，C8）；tick 序列 `t = 0,1000,2000,3000,4000,5000`（6 tick）⇒ 断言：<br>① **分组与计数**：`bus.bit_call_count(1, 200) == 6`（非承载组每 tick 到期）、`bus.input_call_count(1, 100) == 3`（承载组 t=0/2000/4000）—— 该计数同时证明"C6/C7 未被破坏"（`soc` 块恒在承载组）；<br>② **非承载组的位遥测与事件照发（判别性断言）**：t=1000 前 `put_bits(1, 200, …)` 把**位地址 201**（`point_table::lookup_bit(Role::Battery, 201) == BitClass::Alarm` ⇒ metric **`bms_alarm_2`**，`point_table.rs:388`）由 0 置 1 ⇒ **t=1000 的那一轮必须产出 `bms_alarm_2` 的 telemetry 项且 `is_event == true`**（**时点订正 W-3**：t=0 轮只建基线、**不产事件**，故边沿必在**第一次看到新值的那一轮** = **t=1000** 产出；旧文写 t=2000 —— 按该 tick 序 t=2000 时该值**已无变化** ⇒ 取不到 `is_event == true`）；<br>③ 非承载组的**标量**点同样照发（该组若有标量块，按 D2 口径每轮全量） | **B-1 的作用域钉子**：②是该用例的**判别性断言** —— 若把守卫误扩到**整段遥测/事件**（旧写法），非承载组的读集 `reads = {bms_alarm}` 在 `judges_evaluable(Role::Battery, …)` 下**恒 `false`**（`battery_soc` 见不到 `soc` 点 ⇒ `SocOutcome::NoSuchPoint`，`mapper.rs:250-252`）⇒ 位跳变**永不产出**（该组的**全部**位/标量遥测与事件被静默丢弃）⇒ **本用例必红** |
| 单测·调度 | **`station_flag_guard_still_scopes_to_carrier`（守卫有效性负向用例，2026-09-23 新增）**：**直接构造**（绕过 `validate()`，同既有 `battery_station_without_soc_block_does_not_push` 的用法）一个 **`fire` 站**：`fire_det` 声明 `interval_ms: 1000`、`fire_sys` 不声明（站 `interval_ms: 5000`）—— 该形态**违 C7**（配置期会拒，故只能直接构造）⇒ 断言：① **非承载组照常产出**：6 tick 内 `sink.telemetry_of("fire")` 的**非空调用次数 ≥ 6**（`fire_det` 组每 tick 一轮的标量全量），证明"读集不含 `R(fire)` 的组"仍在正常交付（**不得**因守卫恒 `false` 而整段静默）；② **`fire_detector_count_mismatch` 与 `fire_detector_addr_order_invalid` 事件恒 0 条**（6 tick 内），且此时 `fire_sys` 的 `fire_det_count` 读数（20）与容量构造（`0 + 1 = 1`）**本不相等** | **守卫"未被削弱"的钉子**：若不设守卫/把守卫删掉，承载组的 `reads = {fire_sys}` 会让 `fire_detector_mismatch` **假报**（`read_back = 20`、`capacity = 0 + 1 = 1`，`mapper.rs:363-372` / 容量算式 `:377-385`）⇒ **必产一条假告警 ⇒ 本用例红**。该用例同时钉住"B-1 的修法是**收窄作用域**，不是**取消守卫**" |
| 单测·调度 | `block_group_failure_no_station_offline` + `block_group_backs_off_by_group_period`：① `fail_bits_once` 后 `event_count(id,"offline") == 0`、`offline_count == 0`；② **退避断言改为"失败两轮"**（`oc = 2`）：首例 `hvac` 配置下，对**位组**连续 `fail` **两轮**（每轮失败后该组 `group_fail_count` +1，`backoff_extra(1000, 1) = 1000`、`backoff_extra(1000, 2) = 2000`）⇒ 断言 **第 3 轮到期间隔 = `2 × 组周期 = 2000 ms`**（判据：`next_due` 相对失败时刻的后移量）。**为什么必须两轮**：单轮失败时 `backoff_extra(组周期,1) == 组周期` 与 `due_round` 自身的 `next_due += interval` **数值相同** ⇒ 无法区分"退避生效"与"退避未生效"（评审建议 d） | **AC-8-7 ②** |
| 单测·调度 | `carrier_group_failure_emits_offline_once`：慢组读失败 ⇒ `offline` 1 条 + `offline_count == 1` + 按站周期退避 | **AC-8-7 ①** |
| 单测·调度 | `station_recovery_resets_all_group_baselines`：站离线 → 离线期间位由 0→1 → 恢复 ⇒ **只产 `online` 1 条**、无位事件（全组基线重建；**触发者 = 承载组**，非承载组不得触发） | **AC-8-7 ③** + §12.4.4 连带项 a |
| 单测·调度 | **`non_carrier_group_events_survive_carrier_failure`（S-3 判别锚，2026-09-23 新增）**：与 B-1 锚**同款构造**（`battery` 站：站 `interval_ms: 2000`、`bms_alarm`（FC02 `addr 200` `count 288`）声明 `1000`、含 `soc` 点的 `bms_io`（FC04 `addr 100` `count 31`）不声明 ⇒ 快组 = **非承载组**、`bms_io` 组 = 承载组）；tick 序 `t = 0,1000,…,6000`（7 tick）；**承载组到期 tick = t=0/2000/4000/6000**，在 **t=2000/4000/6000 三轮前各调一次** `bus.fail_input_once(1, 100)`（⇒ 承载组在窗口内**持续失败、不恢复** ⇒ `offline_count` 自 t=2000 起恒 `> 0`；**这一点必须写死** —— 若让 t=6000 的承载组成功，则站恢复当轮会（按 §12.4.2 的"承载组优先"）先重建全组基线，t=6000 的边沿会被正确地抑制掉，断言 ② 就不该期望 2 条）；位 201 的摆布：t=0 起为 0（基线），**t=3000 前置 1**、t=5000 前置 0、**t=6000 前再置 1** ⇒ 两次 0→1 上升沿（`Alarm` 位只在上升沿产事件）⇒ 断言：<br>① `bus.bit_call_count(1, 200) == 7`（非承载组每 tick 照常到期、**不被**承载组失败影响）；<br>② **变化沿照常产出**：`sink.event_count("battery", "bms_alarm_2") == 2`（两次 0→1 各 1 条，分别在 **t=3000 轮**与 **t=6000 轮** —— 用 `events_since` 按轮切片钉时点）；<br>③ **不每轮全量落位**：t=3000 轮的**该组位遥测项数 = 1**（仅变化的位 201），而非 **288**（全量快照）；<br>④ 站级语义不被非承载组污染：`offline_count > 0` 期间 `event_count("battery","offline")` 仍为去抖后的 **1** 条，且非承载组成功**不**清 `offline_count`（`sink.event_count("battery","online") == 0`） | **S-3 的判别锚**：按旧写法（① 段触发条件只用 `station_was_offline`、不含 `is_carrier`）—— 承载组持续失败时 `offline_count > 0` 对非承载组**恒真** ⇒ 非承载组**每轮**成功都把全组基线 `reset()` ⇒ ② 取到 **0 条**事件（该组的位跳变被静默吞掉）且 ③ 取到 **288** 项/轮（每轮全量落位，≈2.5×10⁷ 行/天）⇒ **本用例必红** |
| 单测·调度 | `single_group_ordering_unchanged`：`DueCalc::due_round` 对单组站的返回序与既有 `(role_priority, 站序)` **逐项一致** | V-5 |
| 单测·配置 | `block_interval_constraints_rejected`（逐条）：`0`（C1）/ `300`（C2）/ `1500`（`poll_ms = 1000`，C3）/ `6000 > 站周期`（C5）/ `R` 内异周期（以 `fire` 的两块构造，C6）/ `R` 内提速（以 `battery` 的 `soc` 块构造，C7）/ **C4 一例** ⇒ **均 `Err`**（各类文案含站 id + 块名 + 实际取值 + 期望下界）<br>**C4 的可构造用例（评审建议 a；现网 `T_组 ≤ 310 ms` ⇒ `1.5×T_组 ≤ 465 ms < C2 下界 500` ⇒ **C4 被 C2 完全吸收**，须专门构造）**：造一个 `T_组 > 333.3 ms` 的组即可。**构造法（v1.13-r1 订正：S-2）**：与首例同口无关的独立站（`baud_rate: 9600`、`poll_ms: 1000`、站 `interval_ms: 2000`），同组放 **3 个 FC04 块**（各 `count: 120`、不声明 `points`、`addr` 互不重叠）⇒ 单事务帧字节 `8 + (5 + 240) = 253` ⇒ `T_块 = 253 × 1.04 + 4 = 267.12 ms` ⇒ **`T_组 = 3 × 267.12 = 801.36 ms`**（按 §12.6 公式逐事务累加）⇒ **`1.5 × T_组 = 1202.04 ms`**。**三块均声明** `interval_ms: 1000`（**必须三块都声明** —— 只要有**一块**声明、另两块继承站周期 2000，该块就按 `eff` **自成一组**，组内只剩 1 块 ⇒ `T_组 = 267.12 ms`、`1.5×T_组 = 400.7 ms ≤ 1000` ⇒ **C4 通过**，实际触发的是 C9：`U = 267.12/1000 + 2×267.12/2000 = 0.534 > 0.5`，而 C9 文案**不写** `1202` ⇒ C4 未被覆盖）：C1 ✓、**C2 ✓（1000 ≥ 500）**、C3 ✓（`% 1000 == 0`）、C5 ✓（`≤ 2000`）、**C4 ✗（1000 < 1202.04）** ⇒ 断言 `Err` 且**文案含 `1202`（`1.5×T_组` 的计算值）** —— 该值**只有规则 20 的 C4 分支会写**（判据顺序见 §12.7 的注：规则号升序、首个失败即返回），故机械证明 C4 被求值。**该形态另需登记**：三块 `eff` 相同 ⇒ 仍为**同一读组**（V-1）；规则 24 对本例**通过**（`Σ T = 801.36 ≤ 1.5 × 1000 = 1500`）。<br>**诚实登记（C4 不可单独触发）**：C9 的违规域是 `iv < 2×T_组`（`U > 0.5`），**真包含** C4 的违规域 `iv < 1.5×T_组` ⇒ 单组站上 C4 触发时 **C9 必然同时触发**；二者由**文案**区分（C9 文案写 `U` 值与触发的组，不写 `1.5×T_组`） | **AC-8-5**（规则 20–22；C4 一例见左） |
| 单测·配置 | `accepts_first_case_hvac_fast_bit_block`：首例 YAML ⇒ `validate().is_ok()` | 首例可通过 |
| 单测·配置 | `bus_budget_accepts_field_config_and_rejects_overload`：① §10.5 的现网 6 站（含改造后的 hvac）⇒ `Ok`；② 构造 `U > 0.5`（组周期夹到 ≈`T_组`）⇒ `Err`；③ **规则 24 的正反两例（W-1 新增）**：**通过例** = §12.7 的现网复核（`hvac` `Σ T = 47.52 ≤ 1500`）⇒ `Ok`；**拒绝例** = §12.7 的正当性证据构造（同口两组 `(周期 1000, T 19.6 ms)` + `(周期 5000, T 8×267.12 = 2137 ms)` ⇒ `U = 0.447 ≤ 0.5` **但** `Σ T = 2156.6 > 1500`）⇒ **必须 `Err`**（**该例在只有规则 23 时必然 `Ok`** ⇒ 是本条存在的判别锚），文案须含**口名 + 站 id + `Σ T_组` + 阈值** | **AC-8-6**（规则 23 + 规则 24） |
| 单测·配置 | `block_interval_serde_roundtrip_is_unchanged_when_absent`：既有 YAML（无该字段）反序列化 ⇒ `None`；`serde_yaml::to_string` 往返**不出现 `interval_ms` 键**（`skip_serializing_if`） | §12.2.1 的兼容性承诺 |
| 集成 | `tests/s3b2_config.rs` 与 `tests/grid_convergence.rs` 的**既有断言全部保留**（同 §11.11.2 的回归闸门 1–4） | 零回归 |

> **AC-8-8 不设用例（评审建议 c）**：PRD §10.7 原 **AC-8-8**（"端到端边界声明，不得越界断言"）是**元要求**（"不得断言什么"），**不可执行验证** ⇒ 建议需求侧将其移入 §10.8 的**边界声明**（PRD v1.13 已办）。本章对应落点为 **§12.9 的接口边界表**（第 1/2 行）与 **§12.5 的"已知盲区"**，**不上用例表**。
>
> **用例总数**：调度 **13** 个测试函数（12 行；含 §12.8 新增的 `non_carrier_group_still_emits_its_bits`（B-1 锚）、`station_flag_guard_still_scopes_to_carrier`（守卫负向锚）与 **`non_carrier_group_events_survive_carrier_failure`（S-3 锚，v1.13-r1）**）+ 配置 **4** 个（`bus_budget_...` 一例中新增**规则 24 的正反两例**）+ 集成回归 **2 套**。

**复跑门禁**：`cargo test -p mupc-southd`（**既有 44 例**：`scheduler.rs` 的 `#[tokio::test]` **38** + `#[test]` **6**，断言一字不改）+ `cargo clippy --workspace` 0 warning + `cargo fmt --all`（判据同 §11.11.2.1 的行级口径）。

### 12.9 与 12 号（本地显示终端）的接口边界

| 项 | 02 号（本章）**保证** | 02 号**不保证**（属 12 号 / 产品） |
|----|------------------------|-----------------------------------|
| 采集时延 | 告警位以其**块周期**被采集并上送（首例 ≤1 s）；判据 = **AC-8-1 / AC-8-4** | — |
| 端到端 `≤2 s` 上屏 | — | **不保证**。含屏侧链路：口径 A（采集完成→帧发布）= `1.0 + 0.75 = 1.75 s ✓`；通知路径 = `1.0+0.25+0.5+0.1 = 1.85 s ✓`；**兜底 tick 路径 = `1.0+0.5+0.85 = 2.35 s ✗`**（超差 0.35 s）。其正解在**屏侧配置**（`periph_poll_ms`，12 号设计 §15.6.1 约束 1 的 **R-33** 口径）或把位块取 500 ms（PRD §10.9 Q-21 选项 ③） |
| 站离线（F25.4） | — | **不改善**：判定下界 = `stale_timeout_s` = 5 s（**阈值语义**，与轮询节奏无关）⇒ 口径 B 仍 `5 + 1.35 = 6.35 s`。须由 12 号/产品就 `stale_timeout_s` 取值另行裁定（PRD §10.8.1 登记 3；建议 12 号把 R-45 拆为 **R-45a**（本能力覆盖）/ **R-45b**（阈值裁定）） |
| 数据通路 / 点表 | 通路与点表**零改动**（`southd → SouthSink → latest_values → display_host` 逐段不变） | 位点可得性判据的**粒度**问题（PRD §10.9 **Q-23**，属 01 号 `latest_values`） |

**接口级声明**：本章**不新增任何对外接口签名**（不新增 `StationSink` 方法、不新增事件 metric、不改 `DataPackage`/`DisplayFrame`）；**唯一对消费方可见的变化是既有通道上的"上送频次"**。⇒ 12 号设计**无需**为本章改接口，只需按 PRD §10.8.2 回写 R-45 行。

### 12.10 风险、待决项与差异上报

#### 12.10.1 本设计新增/变更的字段与契约（须评审追认）

| 项 | 层级 | 来源 | 用途 | 若不追认的退路 |
|----|------|------|------|----------------|
| 1 **`RegBlockConf.interval_ms`** | 块级（缺省 `None` = 继承） | **PRD §10.3.1 已正式定义**（v1.12）⇒ 本设计只做落地，**非设计新增** | 块级周期覆盖 | 不适用（PRD 已定） |
| 2 **`ReadGroup` / `GroupKey` / `GroupPoll` / `DueEntry.group_fail_count`** | 内部类型 | 设计选择 | 分组调度与组级退避（**不对外**：`pub` 仅为测试可观测） | 退化为"站内只允许一个快组"（用 `Option<usize>` 单快组索引）——**不建议**：§10.3.3 的分组语义会被削弱，且 Q-22 选项 B 将无法演进 |
| 3 **`EdgeTracker` 键由站改为组** | 内部状态 | 设计选择（§12.4.4 的机制论证） | 防组间互相清空基线 | **不可退**：不改则 `edge_memory_is_per_group` 必红（真实丢事件） |
| 4 **组级退避 + "块级失败不升级为站级 offline"** | 运行期语义 | 设计选择（§12.5 两条硬理由） | ① 不遮蔽正在上送的快采告警位；② 消除周期性事件对刷屏 | **不可退**（退则会破 12 号 F25.4 的显示口径或产生 ≈3.5 万条/日事件） |
| 5 **`judges_evaluable` 运行期守卫** | 内部函数 | 设计补充（PRD 的 C6 已在**配置期**保证；本守卫是其**运行期对偶**） | 纵深防御（配置校验被绕过 / 未来放开 C6 时的假事件防线） | 可退化为 `debug_assert!`（代价：Q-22 选项 B 失去前置守卫） |
| 6 **规则 20–24 与常量 `MIN_POLL_INTERVAL_MS`** | 校验规则 + 常量 | PRD §10.3.2 / §10.5 明文要求，**未进 §9.4.3 的 17 条表**（同既有规则 18/19 的形态） | 配置期拦截误配 | 不适用（PRD 已要求）；**建议需求侧把 20–24 补进 §10.3.2 的 C 表**（本版已写入，见 PRD §10.3.2）。**规则 24 是 v1.13-r1 新增**（落 PRD §10.5 **第 3 行**的"单轮最坏耗时"，原设计无落点；PRD 侧无对应 C 编号） |
| 7 **退化组哨兵 `EMPTY_GROUP_ANCHOR`** | 内部常量（`usize::MAX`） | 设计选择（S-1 修订：空 `regs` 站须有 1 个组、且不得 panic） | 使 `read_groups_of` 的"至少一组"不变量对空 `regs` 成立，同时保持"锚 → 组"单射 | 不适用（内部常量，无对外契约）。**注**：本项是**行为等价**的实现细节 —— 空 `regs` 站的调度行为仍与既有逐字相同（§12.3 V-5(b)） |
| 8 **站级重建的触发者 = 承载组**（S-3 修订） | 运行期语义 | 设计选择（§12.4.3 ① 段的门控 + §12.4.2 的"承载组站内优先"排序键） | 使站恢复当轮"全组基线重建"**先于**本站其它组产出，且非承载组不会每轮被重置（否则变化沿永不产出 + 每轮全量落位） | **不可退**（退则 §12.8 的 `non_carrier_group_events_survive_carrier_failure` 必红，且与 PRD §9.8.1 末条告警的写量同量级） |

#### 12.10.2 PRD 差异上报

| 编号 | 差异 | 事实 | 本设计执行口径 |
|------|------|------|----------------|
| **Δ-10** | **PRD §9.5.4「n>20 时**探测器块**独立降频（≥5000 ms）」的字面诉求 = **块级拆分**（系统态 1000 ms + 探测器块 5000 ms）** | 该字面诉求**必须**块级周期才能表达；但 `fire` 的两类判据跨块（`mapper.rs:359` / `:565`），受本章 **C6/C7** 约束 ⇒ **`fire` 站本轮无可提速块** ⇒ 该诉求在 n>20 时仍只能"整站 5000 ms" | **本轮按"整站 5000 ms"执行**（= 现状，与 §9.5.4 的字面有差异，已如实登记为 PRD **§10.9 Q-22**）。**选项 A（推荐本轮）** 保持 C6/C7；**选项 B（下轮）** 放开 C6/C7 并同时落地"**完整性守卫 + 合并视图**"。<br>**★ 选项 B 的第三环（评审补登，2026-09-23；已同步 PRD §10.9 Q-22 选项 B）**：放开 C6/C7 **还不够** —— 仍须把 **`fire` 站级 `interval_ms` 设为 5000**，否则 `fire_det` 声明的 5000 **违 C5**（`eff ≤ S`）。其直接后果有二：① `fire_sys`（系统态）的 1000 ms 诉求只能由**块级覆盖**表达（这正是 §9.5.4 的字面形态）；② **站级承载组由 `fire_sys`（1000）变为 `fire_det`（5000）** ⇒ **站离线判定节奏随之由 1 s 变为 5 s**（与"整站 5000"同），且该变化落在 **12 号 F25.4 的显示口径**上（站离线判定时延），**必须与"完整性守卫 + 合并视图"一并登记并由 12 号确认**。<br>**建议需求侧在 §9.5.4 或 §10 消歧**（本节不擅自改 PRD §9.5.4） |
| **Δ-11** | PRD **§9.8.1 的 `grid_meter` 行未给单轮耗时**（标"既有"） | 本章的 **C9（口预算）** 要对**每一口**复算 `U`，`grid_meter`（`ttyS4`）缺 `T` | **本章不擅自估填**（避免造一个无出处的数字）；已在 PRD §10.9 **Q-21 ③** 登记"补算后再纳入断言"。**不影响本章任何结论**（C9 的判据是 `U ≤ 0.5`，其余 5 站已复算通过，且 `grid_meter` 独占一口） |

| **Δ-12**（**v1.13-r1 新增**） | **"空 `regs` 站"是否应在配置期被拒（非 `pcs` role）** —— 待裁定 | 事实：① 空 `regs` = "站永不产出任何点"的**静默死配**，仅由运行期"读集为空 + 成功记账"兜住（**无任何日志**）；② 既有配置期**只**对 `Role::Pcs` 拒空（`config.rs:296-303`，文案已写明"空 regs = 站永久 offline 的静默死配"）；③ **除 `pcs` 与 `battery` 外**的 role（`hvac` / `meter_batt` / `fire` / `meter_grid`）的空 `regs` 站**能通过 `validate`**，且**行为上是"合法"的**（既有单测 `battery_station_without_soc_block_does_not_push` 就以空 `regs` 的 battery 站为**正例**，断言"不产 `offline`/`online`、不误推 soc"）。**收窄订正（v1.13-r2）**：`Role::Battery` 的空 `regs` **不能**通过校验 —— 被**规则 4**（`soc` 点契约）拒（`config.rs:486-488`：`role == Battery && 无任何点名 soc` ⇒ `Err`），故"空 `regs` 站通过校验"**只对其余四个 role 成立**；`battery` 空 `regs` 站的**可达性**由**第二条证据**独立支撑（既有单测**不经 `validate`**、直接内联构造） | **本轮不单方面加配置期拒绝**，执行口径 = **逐字保持既有行为**（§12.3 V-5(b) 的机械证明 + `carrier_group` 不 panic）。**理由**：对**除 `pcs` 与 `battery` 外**的 role 新增拒绝 = **新增约束**（会让既有"合法可构造"的用法变成配置错误）= **需求变更**，须由需求侧裁定（建议与 §10.3.2 的 C 表一并审）⇒ **登记为待裁定项**（本章**不新增**任何 role 的空 `regs` 配置期拒绝：`battery` 的空 `regs` 现状由既有**规则 4** 的 `soc` 点契约拒，其余 role 仍照现状通过） |
| **Δ-13**（**v1.13-r1 新增**；**v1.13-r2 扩面**） | PRD **§10.4 的两处文档级文字**：① 响应帧算式 `(5+2N)` 与**该表自己的 `FC02 = 8 + (5+4) = 17`** 不自洽；② **波特率/校验位字样 `8N1`**（同段 `02 PRD:2449`）与实配不符（`hvac` 实为 `parity: even` = **8E1**，`production.yaml:398`） | 行政性（文档级）：① FC02 响应 = `slave(1)+func(1)+byte_count(1)+D+CRC(2)` = **`5 + D`**（`D = ceil(位数/8)` 字节）；PRD §10.4 的 `N` 注写作"FC02 时 `N = ceil(位数/8)` **字节**" ⇒ 按该注算得 `5+2×4 = 13`、帧 `21`，与其表内 `17` 矛盾（`17` 才是对的）；② 同段的 `8N1` 是**文字层**的默认值残留（该段算式另明写"估时公式与 §9.8.1 逐字相同"，故 `8N1` 只影响该处字样、不参与算式） | **本章 §12.6 已按 `5 + D` 写死**（与本表 `17`/`21`、`T_快组 = 21.68 ms` / `T_慢组 = 25.84 ms` 逐位一致）⇒ **PRD 表内**的**任何数字都无需改动**；只需需求侧把该句的 `N` 定义订正为"`D` = 数据字节数（FC02 = `ceil(位数/8)`；FC03/04 = `2 × 寄存器数`）"，并把 `8N1` 字样订正为 `8E1`（或改为按站取 `parity`）。**两处均为文档级、不影响任何数字/判据** —— `8E1` 的**敏感性复算已在本章 §12.6 给出且结论不变**（按 §9.8.1 的 **10 bit/字节**口径：`T_快组 ≈ 23.5 ms` / `T_慢组 ≈ 28.1 ms` / `U_后 ≈ 2.91 %`） |
| **Δ-14**（**v1.13-r2 新增**） | **`DueCalc::due_round` 的排序键含 `!is_carrier`，与 PRD §10.6 第 3 条的元组字面不符** | 事实：本章的排序键为 **`(role_priority, station_index, !is_carrier, anchor_blk)`**（§12.4.2），比 **PRD §10.6 第 3 条**（`02 PRD:2522`）字面写的 `(角色优先级, 站序, 组锚)` **多出 `!is_carrier`** 一项。**依据**：这是 S-3 修订的**有意细化** —— 站恢复当轮**必须让承载组先跑**以完成"全组基线重建"，否则快组会先用**离线前**的基线产出（同 tick "先产出、后被重建"的半状态）且**下一轮**多一次 `full_snapshot`（论证见 §12.4.2 的三条"为什么不可接受"） | **本轮按 `(role_priority, station_index, !is_carrier, anchor_blk)` 执行**（= 已有写法，不改）。**建议（需求侧动作，本轮不改 PRD）**：把 **PRD §10.6 第 3 条**的元组订正为 **`(角色优先级, 站序, !承载组, 组锚)`**，使两文档字面一致。**该细化不改变**"同口**串行**、不并发"这条硬约束 —— 串行仍由 `Rs485PortBus` 的 per-port async Mutex / 单 poller 结构保证（V-6 / `scheduler.rs:1-5`）；`!is_carrier` 只决定**同 tick 内的先后**，不引入并发通道 |

> **说明**：本章的 Δ 表 = **需下一步动作的真实缺口**（消歧 / 补算 / 待裁定 / 文档级文字），**不是**"设计与 PRD 相左"。**Δ-10 / Δ-11 为 v1.13 既有**；**Δ-12 / Δ-13 为 v1.13-r1 新增**；**Δ-14 为 v1.13-r2 新增**（**Δ-12 的"空 `regs` 通过校验"范围、Δ-13 的覆盖面对，已于 v1.13-r2 订正/扩面**）。PRD §10 的 **C1–C9 与本章的规则 20–23 逐条对齐**，**PRD §10.5 第 3 行**由 **v1.13-r1 新增的规则 24** 落点（该行在 PRD 侧没有 C 编号，故不计入"C ↔ 规则"的对齐表）。

> **同轮落地的 PRD §10 文档级订正（评审附 4 项 + 建议改进 4 条，2026-09-23）**：设计评审对 **02 PRD §10** 提出的 4 项非阻塞订正与 4 条建议改进，**已同轮落入 PRD §10 正文**（PRD v1.13 的 `[§10 增补 v1.13]` 块逐条登记）；本章据此同步的口径见下表 —— 这些**不是** Δ（无需设计侧另行"上报差异"），只是**两文档必须逐位一致**的数字/引用：
>
> | # | PRD §10 订正 | 本章的连带落点 |
> |---|--------------|----------------|
> | a | **C7 依据列的引用不实**（原引「§10.9 Q-21 ② 登记了"允许 R 提速"的备选口径与代价」，而 Q-21 全文只有三档取值）⇒ 改为**真依据**：§10.2.2 范围外表的「判据块提速」行 + §10.3.2 的 `R(role)` 表 + §10.4 末的"可提速一览"表（**本轮明确不做，须另立需求**） | §12.3 **V-3** 与 §12.7 **规则 22** 的 C7 口径不变（只改 PRD 的引用出处）；§12.4.5 的作用域声明与之对齐 |
> | b | **行号引用偏差**：§10.8.1 引 `12-MUPC-本地显示终端-PRD.md:783`（F25 的**用户故事**）⇒ 订正为 **`:788`**（F25.3 的**验收原文**） | §12.9 的 F25.3/F25.4 边界表述不变（无行号引用） |
> | c | **"改造前占用率"口径不一**（§10.4 表 `≈0.96 %` vs 版本演进 `0.95 %`）⇒ 统一为**本表公式精确值** `47.52/5000 = 0.95 %` | §12.6 已写明统一算式与"不得改用取整值 48 ms"的禁令（见上） |
> | d | **Q-22 选项 B 漏一环**：放开 C6/C7 后**仍须 `fire` 站级 = 5000**（否则违 C5），且**承载组变 5000**（站离线判定节奏 1 s → 5 s） | 见上表 **Δ-10** 的"★ 第三环"补登 |
> | 建议 a | **AC-8-5 补 C4 的构造法** | §12.8 的 `block_interval_constraints_rejected` 用例**新增 C4 一列并给出可构造算式**（低波特率/多块放大 `T_组`） |
> | 建议 b | **AC-8-3 钉 tick 序列与计数** | §12.8 的 `no_block_interval_is_bit_identical_to_legacy` 用例补**确定性 tick 序与逐项计数** |
> | 建议 c | **AC-8-8 移入边界声明**（元要求，不可执行验证） | §12.8 的**用例映射不列 AC-8-8**；其性质 = §12.9 的**边界声明**（本节表第 1/2 行）+ **§12.5 的"已知盲区"**（不设用例） |
> | 建议 d | **AC-8-7 ② 退避断言改 `fail` 两轮（oc=2）** | §12.8 的 `block_group_backs_off_by_group_period` 用例改为**两轮失败、断言 `2 × 组周期`** |

> **v1.13-r1 的 PRD 落点（S-2 之二，**仅** AC-8-5 的"C4 构造法"示例）**：02 PRD **§10.7 的 AC-8-5 那一格**中"**C4 的构造法**"的**例子**已改为"**三块均声明** `interval_ms: 1000`"，并补一句"**该组内三块 `eff` 相同 ⇒ 仍为同一读组**"（PRD **v1.13-r1**）。**AC-8-5 的判据本身（非法值逐条 `Err`、文案含 `1202`）未动**；PRD 的 C1–C9 约束内容、任何 AC 判据、§9 与 §1–§9 的内容**一字未动**。（旧例"取该组某块声明 1000"按 §12.4.1 的分组语义**不触发 C4** —— 见文首 v1.13-r1 块的 ② 项。）

#### 12.10.3 实现风险

| 风险 | 说明 | 缓解 |
|------|------|------|
| R-7 **组间基线互相清空**（最易犯） | 见 §12.4.4 的机制论证；若照搬 `PortRunner.trackers: HashMap<usize, _>`（`scheduler.rs:284-285`）而不换键，症状是"位块事件随机丢失"（5 s 周期复发一次），**测试若只跑单组站则完全测不出** | 专测 `edge_memory_is_per_group`（交错 tick）；并在 `poll_group` 处加注释指向 §12.4.4 |
| R-8 **对快组误调 `poll_to_result`** | `MeterGrid` 缺相量即 `Failed`（`mapper.rs:410-429`）⇒ **快组恒判失败**（若将来有 grid 快组） | 只在 `is_carrier` 分支调用（§12.4.3 伪码已钉）；加负向单测（构造 grid 站 + 假快组 ⇒ 不得产 offline） |
| R-9 **恢复时只重置当前组** | 见 §12.4.4 连带项 a（症状：`online` 后紧跟一屏位事件）。**触发者被限定为承载组**（S-3 修订）⇒ 非承载组在"承载组持续失败"期间**不会**每轮重置全组（否则症状反过来：0→1 事件永不产出 + 每轮全量落位） | 专测 `station_recovery_resets_all_group_baselines`（正向：恢复当轮只建基线）+ **`non_carrier_group_events_survive_carrier_failure`**（负向：不得每轮重建） |
| R-12（**v1.13-r1 新增**）**空 `regs` 站的可达 panic / 静默移出调度** | 若 `carrier_group` 用 `.expect`、或 `read_groups_of` 对空 `regs` 返回 **0** 个组，则既有单测 `battery_station_without_soc_block_does_not_push`（内联构造 `regs: vec![]`、**不经 `validate`**）会 **panic** 或该站**永不轮询**（§12.4.1 / §12.3 V-5(b)） | `read_groups_of` 的"**至少返回一个组**"不变量（空 ⇒ 退化组）+ `carrier_group` 返回 `Option`（**无 `expect`/`unwrap`**）；由既有 44 例（含该用例）作回归锚 |
| R-13（**v1.13-r1 新增**）**规则 24 的 `T_组` 口径** | 与规则 20 的 `T_组` 必须是**同一口径**（"该块所在读组的整组耗时"）；若规则 24 误用"单块耗时"，则多块组会被低估而漏拒（§12.7 的 `T_组` 定义注） | 两处共用同一个纯函数 `group_tx_time(baud_rate, &ReadGroup)`（与 R-10 同源）；单测在 `bus_budget_...` 的规则 24 拒绝例中钉住 `Σ T = 2156.6 ms` |
| R-10 **C4 的 `T_组` 用错 baud_rate** | 同口各站 `baud_rate` 由规则 16 强制一致（`config.rs:308-327`），但**跨口不同**（PCS 19200）⇒ 复算须用**本站**的 `baud_rate` | 纯函数 `group_tx_time(baud_rate, func, count)` 单测（9600 / 19200 各一例）；调用点只传本站值 |
| R-11 **C3 网格对齐被误当成"可放宽"** | 若实现成"运行时把 `iv` 向上取整到 `poll_ms`"，则配置写的 1500 实际跑 2000（**静默失真**，与本 PRD 反复整治的取向相反） | 规则 21 在**配置期** `Err`；单测 `block_interval_constraints_rejected` 钉住 `1500` 一例 |

### 12.11 实施顺序（建议 Task 拆分）

| # | Task | 内容 | 闸门 |
|---|------|------|------|
| **T8** | 配置与校验 | §12.2.1 的 `interval_ms`（`Option<u64>` + `skip_serializing_if`）+ `effective_interval_ms` + `read_groups_of`（**含空 `regs` 的退化组**）/`carrier_group`（**返回 `Option`**）+ **规则 20–24**（`validate_block_intervals`，插入为 ②′）+ 常量 `MIN_POLL_INTERVAL_MS` / `EMPTY_GROUP_ANCHOR` | 配置期单测全绿（§12.8 的 4 条 `*_config` 用例，含**规则 24 的正反两例**）+ **既有 `config.rs` 用例断言零改动** |
| **T9** | 调度器改造 | §12.2.2 的结构（**无 `carrier_anchor`**）+ `DueCalc`（`due_round`（**排序键含 `!is_carrier`**）/`delay_group`/`bump_*`/`clear_*`）+ `run_port_round` + `poll_group` + **`round_signals` 拆为 `round_signals_group` / `round_station_flags`** + `judges_evaluable`（**作用域 = 判据/站级量路径**，§12.4.5）+ **tracker 键改 `GroupKey`** + **站恢复"全组重建"（触发者 = 承载组，`poll.is_carrier && station_was_offline`）** | 调度器单测全绿（§12.8 的**前 12 行 / 13 个测试函数**，含 **B-1 回归锚 `non_carrier_group_still_emits_its_bits`**、**守卫负向锚 `station_flag_guard_still_scopes_to_carrier`** 与 **S-3 锚 `non_carrier_group_events_survive_carrier_failure`**）+ **既有 `scheduler.rs` 44 例断言零改动**（AC-8-3，含空 `regs` 站那一例） |
| **T10** | 配置与文档落地 | 生效配置的 hvac 站加 `interval_ms: 1000`（§12.6 迁移行）+ `tests/fixtures/south_stations_s3b2.yaml` 同步 + 12 号设计 R-45 行回写（**若 T10 由需求侧执行则见 PRD §10.8.2**） | **AC-8-1…AC-8-7 全绿**（**AC-8-8 已移入边界声明 ⇒ 不适用例**，v1.13）+ `cargo test -p mupc-southd` + `cargo clippy --workspace` + `cargo fmt --all` |

**依赖**：T8 → T9 → T10（T9 依赖 T8 的 `read_groups_of`）；**三者均不依赖 12 号侧的任何改动**（§12.9）。

### 12.12 一致性声明

| 关联 | 关系 |
|------|------|
| PRD §10（**v1.12 新增 / v1.13 评审附项订正 / v1.13-r1 仅 AC-8-5 例子订正**，2026-09-23） | 本章是 §10 的实现级落地：C1–C9 → **规则 20–23 + C8 的确定性规则**（§12.7）；**PRD §10.5 第 3 行（单轮最坏耗时 `Σ T_组`）→ 规则 24**（v1.13-r1 新增，PRD 侧无 C 编号）；首例与负载数字 → §12.6（与 PRD §10.4 **逐位一致**，占用率统一为公式精确值 `0.95 %` / `2.68 %`）；边界与异常 8 条 → §12.5 与 §12.4（含"块级失败不升级站级 offline"的两条硬理由）；**AC-8-1…AC-8-7 → §12.8 的用例映射**（**AC-8-8 已按评审建议 c 移入 §10.8 的边界声明**，不适用例；本章对应落点 = §12.9 的接口边界表与 §12.5 的"已知盲区"） |
| 本文档 §10（S3a）/ §11（S3b-2） | **调度架构、故障隔离语义、role→DataPackage 分发骨架全部不变**；本章只把**调度粒度**由"站"细化为"组"，且**单组站（无块声明）与空 `regs` 站均与既有逐字等价**（§12.3 V-5(a)/(b) 的机械证明）。§11 的统一事件模型（`EdgeTracker`/`StationFlag`）**零改动**，只改其**记忆键** |
| 本文档 §11.4.7 / §11.7.2 | 事件产出路径与位点落库节流口径（D2）**零改动**；"恢复后只建基线"由"当前站"推广为"该站全部组"，且**触发者被钉为『该站承载组』**（§12.4.4 连带项 a + §12.5 重建条件表，S-3 修订），**不新增事件名** |
| `plans/modules/12-MUPC-本地显示终端-设计文档.md` | 本章是 12 号 **R-45 / F25.3 的前置能力**；接口边界见 §12.9（**不新增任何接口签名**）。12 号文档**只回写 R-45 那一行**（PRD §10.8.2） |
| `plans/modules/01-MUPC-通信网关-设计文档.md` §9.1 | `latest_values` 的归属与判据**不变**；Q-23（位点可得性粒度）是跨文档待裁项，本章不假定其扩展（§12.5 的"已知盲区"如实登记） |
| 项目 CLAUDE.md | 无硬编码密钥；无新增 `unsafe`；错误类型实现 `std::error::Error`；不新增独立设计文档 |

---

## 附录 A：术语表

| 术语 | 说明 |
|------|------|
| MUPC | 微电网特种调控装置 |
| TTU | 台区智能融合终端（配电变压器终端单元） |
| HPLC | 高速电力线载波通信 (High-speed Power Line Carrier) |
| RS485 | 串行通信总线标准（半双工，差分信号） |
| DE/RE | RS485 半双工使能引脚（Driver Enable / Receiver Enable） |
| GPIO | 通用输入输出引脚 |
| FFI | 外部函数接口 (Foreign Function Interface) |
| cdylib | C 动态库格式（Rust crate-type） |
| Modbus RTU | 串行通信协议，RS485 物理层上的常用工业协议 |
| GB/T 27930 | 电动汽车非车载传导式充电机与 BMS 通信协议 |
| OFDM | 正交频分复用 (Orthogonal Frequency Division Multiplexing) |
| termios | Unix 终端 I/O 控制系统（串口配置标准接口） |
| CRC | 循环冗余校验 (Cyclic Redundancy Check) |
| RKNN | Rockchip Neural Network（RK3588 NPU 推理框架） |
| FC02 / FC03 / FC04 | Modbus 功能码：读离散输入 / 读保持寄存器 / 读输入寄存器（§11.4.5） |
| 读窗口（块） | 一次 Modbus 事务覆盖的连续地址区间，只承载**传输口径**（`func`/`addr`/`count`/`byte_swap`）与缺省换算（§11.2.1） |
| 点位（point） | 一个物理量的**换算口径**（`format`/`scale`/`offset`/`word_order`/`name`）+ 其遥测键（metric）；块内 `points[]` 逐点声明（§11.4.1/§11.4.3） |
| 位块 | `func: discrete` 的离散输入区间，逐位产出 0/1 点（§11.4.3/§11.4.5） |
| 点表登记注册表 | `mupc-southd::point_table::POINT_REGS`：`(role, addr) → {format, scale, offset, 符号性来源, 分类, 中文名, signals}`，按 n=20 参考配置**展开后 618 行**（探测器区为 6 条组内模板 + 运行期展开）由 PRD §9.5 逐点转写；用作配置期一致性校验的期望值来源、RC-1 核对清单与消防事件源（§11.4.4/§11.4.7.1） |
| 变化沿落库 | 位点仅在与上一轮取值不同时写 telemetry（稳态零写），`Alarm` 位另产上升沿事件（§11.2.4 D2 / §11.7.2） |
| **信号（Signal）** | 一个**可跃迁量**：离散位（`Bit`）或保持寄存器整字内的**位/枚举**（`WordBit`/`WordEnum`）（**另含站级派生布尔量 `StationFlag`** —— 地址序违规/SOC 域/登记数一致性）。四类"字/位信号"+ `StationFlag` 共用一套跃迁记忆与事件产出路径，差别仅在"活跃判据函数"与**产出频次口径**（§11.4.7.1 / §11.4.7.2） |
| **`EdgeTracker`** | scheduler 内按站维护的"上轮活跃态"记忆表，同时承载离散位与字级信号；站恢复后 `reset()`（首轮只建基线、不产事件）（§11.4.7） |
| **`read_slice`** | 块级字段（PRD §9.4.2.4，PRD v1.7 补登）：该块是"按设备单次读上限主动分片"的结果，**仅**豁免第 15 条极大性检查（§11.5.2） |

---

## 附录：版本演进

> 正文已整合全部历史补丁，本表仅作演进追溯。

| 版本 | 主要变更 |
|------|----------|
| **v1.13-r3（2026-09-24，**T8 实现发现的合同勘误回写**；**未新增门禁标记**，`[DESIGN_APPROVED: 2026-09-23]` 的覆盖范围仍只到 §12）** | **只订正合同行文与理由**（依据 = T8 的 `28fd0c1`/`609f65c`/`4b0f6c2` + 两轮评审）：**①** §12.7 末段 C8 的"**加载期** `debug` 日志"⇒ 该路径**不可达**（`CoreConfig::validate` 属 main **Phase 1** `main.rs:105`，tracing 到 **Phase 2** `main.rs:164` 才初始化 ⇒ 无订阅者、日志被丢弃）⇒ 订正为"**判定 = 纯函数 `block_interval_hints`；发射点在 startup 装配期**"（理由引 `core_config.rs:512-514` 的既有成文约定），并写明实现证据与**双向断言**用例；**②** §12.7 规则 24 的"单组站由 **C4** 自动成立"⇒ **理由错**（C4 只判**显式声明** `interval_ms` 的块；现网 4 站的块全部未声明 ⇒ C4 **未求值**）⇒ 订正为"口内仅一组时本条**恒被规则 23（C9）先判**（`U > 0.5` 的违规域真包含 `T_组 > 1.5 × 组周期`，因 `0.5 < 1.5`）"（**结论不变**：只在多组口上可能有独立作用、现网零新增拒绝）；**③** §12.6 追认"字节耗时按 **2 位小数**取整"**为口径的一部分**（AC-8-5「文案含 `1202`」**依赖**它；不取整得 `1203.94`），并注明非 9600 波特率下 ≤0.5% 的估计偏差。**未改**：C1–C9 的任何约束、AC-8-1…AC-8-7 的任何判据、§12.4 的全部伪码与不变量、§12.5/§12.6 的任何数字、§1–§11、任何代码/配置/PRD/12 号设计；**既有门禁标记原文与覆盖范围不变**。 |
| **v1.13-r2（2026-09-23，§12 评审后的**文字/记账订正**；**未新增门禁标记**，`[DESIGN_APPROVED: 2026-09-23]` 的覆盖范围仍只到 §12）** | **只动本文件 §12 的文字/记账**，处置 §12 设计评审自己登记的 **3 条遗留项**（评审明写"须在下一修订版处理"）+ **4 条优化**：**①** §12.8 用例① 的 PRD 依据行号由 `:1768` / `:2208` 订正为**交付态真值 `:1772` / `:2212`**（v1.13-r1 行按改动前行号计，属历史记录不追改，仅加订正说明）；**②** 排序键多出的 `!is_carrier` 与 PRD §10.6 第 3 条字面不符 ⇒ **新增 Δ-14** + §12.4.2 论证段末 / §12.3 V-6 两处交叉引用（建议需求侧把元组订正为 `(角色优先级, 站序, !承载组, 组锚)`；本轮不改 PRD）；**③** Δ-12 的"非 `pcs` role 空 `regs` 能通过校验"**收窄**为"**除 `pcs` 与 `battery` 外**"（`battery` 被**规则 4** `soc` 点契约拒，`config.rs:486-488`；可达性结论**保留**、由"既有单测不经 `validate` 内联构造"支撑），同批订正文首 S-1 项与 §12.4.2 的 `.expect` 理由注（**共 3 处**）；**④** Δ-13 **扩面**至**同段 PRD §10.4** 的 `8N1`（`02 PRD:2449`；实配 8E1）—— 两处均**文档级**、不影响任何数字/判据；**⑤** §12.4.4 末的"§12.10.1 第 4 项"订正为**第 8 项**；**⑥** §12.2.2 末"已删除的字段"注的 `carrier_anchor` 声明版本号 v1.13 ⇒ **v1.12**，并写准删除依据；**⑦** §12.8 用例① 补"**自建站 slave = 1**、与既有 `battery_*` fixture `slave = 2` 无关"注（未改用例数字）；**⑧** 记账（本行 + 遗留项关闭标记）。**无任何语义 / 约束 / 伪码 / 数字改动**；**未改** §1–§11、PRD、任何代码/配置、12 号设计；**既有门禁标记原文与覆盖范围不变**。 |
| **v1.13-r1（2026-09-23，按设计评审意见（第二轮）修订，待复审；本次未获门禁标记）** | **只改 §12（§1–§11 零改动）**，逐条处置第二轮设计评审的 **3 阻塞 + 3 警告 + 4 优化**（并确认其独立验证的"B-1 修法正确"不动）：<br>**① 阻塞 S-1（空 `regs` 站的可达 panic + 静默移出调度）**：`carrier_group` 的 `.expect` 在空 `regs` 上必 panic（该输入**可达**：既有单测内联构造 `regs: vec![]` 直接 `tick_once`；配置期只对 `Role::Pcs` 拒空，`config.rs:296-303`），且 `from_group` 产 0 条目 ⇒ 该站永不轮询 ⇒ 破"既有 44 例零改动"。**修法**：`read_groups_of` 对空 `regs` **产出一个退化组**（空块集、周期 = 站周期、锚 = 哨兵 `EMPTY_GROUP_ANCHOR`）⇒ 恒 1 条目、该组即承载组 ⇒ 与既有"站照常轮询、读集为空、`poll_to_result(role,&[])` 照常求值、成败记账照旧"**逐字等价**；`carrier_group` 改返回 **`Option<ReadGroup>`**（无可达 `expect`/`unwrap`）；**订正 `.expect` 的理由文案**；**V-5 补空集情形**；新增 **Δ-12**（"非 `pcs` role 的空 `regs` 是否应配置期拒绝" = **待裁定**，**本轮不加**）；**同源加固**：`bump_group_fail` 改**原子返回** `Option<(组周期, 计数)>`、调用点一律 `if let` ⇒ 退避段**无 `unwrap`/`expect`**。<br>**② 阻塞 S-2（AC-8-5 的 C4 用例按字面构造不触发 C4）**：旧例"只取该组**一块**声明 `interval_ms: 1000`" —— 声明周期的块按 `eff` **自成一组** ⇒ `T_组 = 267.12 ms`、`1.5×T_组 = 400.7 ≤ 1000` ⇒ **C4 通过**，实际触发 **C9**（`U = 534 > 500`）而 C9 文案不写 `1202` ⇒ C4 无人覆盖。**修法**：改为**三块均声明** `1000`（仍同组 ⇒ `T_组 = 801.36 ms`、`1.5×T_组 = 1202.04` ⇒ `1000 < 1202.04` ⇒ **规则 20 先于规则 23 返回**）；**规则 20 的 `T_组` 定义写死**为"该块**所在读组**的整组耗时"（同组同 `eff` ⇒ 组由 `eff` 唯一确定 ⇒ **无循环**）；**同轮落 PRD §10.7 的 AC-8-5 那一格（仅例子，判据未动，PRD v1.13-r1）**。<br>**③ 阻塞 S-3（站级重建的触发条件取错 + 同 tick 组序）**：旧条件 `if station_was_offline` 而 `offline_count` 只在承载组成功时清零 ⇒ 承载组持续失败时非承载组**每轮**重置全组 ⇒ 变化沿永不产出 + 每轮全量落位（288 位/轮 ≈ 2.5×10⁷ 行/天）。**修法**：门控 **`poll.is_carrier && station_was_offline`**；`due_round` 排序键加 **`!is_carrier`**（**站内承载组优先**，使全组基线重建**先于**本站其它组产出；并在 §12.4.2 给出"清零不会让后来组误判"的论证）；新增用例 **`non_carrier_group_events_survive_carrier_failure`**（**按旧写法必红**）；§12.4.4 / §12.5 / §12.10 / §12.11 / §12.12 的"谁能重建基线"表述**全章统一**。<br>**④ 警告 W-1（PRD §10.5 第 3 行无落点）**：新增 **规则 24**（同口 `Σ T_组 > 1.5 × 最小非零组周期` ⇒ `Err`），并登记"`U ≤ 0.5` 但 `Σ T` 超标"的**反例**（`Σ T = 2156.6 > 1500`）为正当性证据 + 现网复核（`hvac` `47.52 ≤ 1500` ✓）；**PRD §10.5 一字未动**。<br>**⑤ 警告 W-2（引用漂移）**：`battery` 回退周期 2000 ms 的依据 `:1754` ⇒ **`:1768`**（`:2208` 同义）。<br>**⑥ 警告 W-3（断言时点自相矛盾）**：用例① 的边沿断言由 t=2000 改为 **t=1000**（t=0 只建基线），其余计数逐条复核仍自洽。<br>**⑦ 优化 1–4**：文首"改动范围"去歧义（01 号属其自身 `v1.4-r3`）；§12.3 去掉"与 C1–C9 一一对应"的不实标题并补对应关系；§12.6 订正 **hvac = 8E1**（按 §9.8.1 的 10 bit/字节口径声明 + 8E1 敏感性复算，结论不变）与**响应帧算式**（`5 + D`，与表内 `17`/`21`、`21.68`/`25.84 ms` 自洽 ⇒ **PRD §10.4 数字无需改**；其文字偏差登记 **Δ-13**）；`GroupKey { ..poll.into() }` 改显式构造；删 `PortRunner.carrier_anchor`（无使用点）；§12.5 退避口径统一为"承载组自身组周期"。<br>**未改**：§1–§11、任何代码/配置、12 号设计；**既有门禁标记**（`[DESIGN_APPROVED: 2026-09-21]` / `[CODE_REVIEWED: PASS: 2026-09-22]` 原文未动，覆盖范围仍只到 §11）；**本次未加任何新门禁标记**（待复审）。**PRD 侧只动**：§10.7 的 AC-8-5 示例（"C4 的构造法"例子）+ 版本行（v1.13-r1，**判据未动、不构成重新评审**） |
| **v1.13（2026-09-23，按设计评审意见修订，待复审；本次未获门禁标记）** | **只改 §12（§1–§11 零改动）**，逐条处置设计评审的 1 阻塞 + 非阻塞项：<br>**① 阻塞 B-1 —— `judges_evaluable` 作用域错**：旧写法把守卫套在**整段**遥测/事件上，而它按"**组内**齐备 `R(role)`"判定 ⇒ **`R(role) ≠ ∅` 的站上非承载组恒 `false`**（battery 快组不含 `soc` 块）⇒ **静默丢弃全部位/标量遥测与事件**（无日志、无用例），既与 §12.5"组内块只喂本组 tracker"自相矛盾，也使 PRD §10.3.2 允许的"`bms_alarm` 可提速"失效。**修法**：守卫**只管「判据 / 站级量」路径** —— 把既有 `round_signals`（`scheduler.rs:311-383`）机械拆为 `round_signals_group`（逐块位/字信号，**恒产**）与 `round_station_flags`（`StationFlag` 三项，**仅 `if poll.is_carrier && judges_evaluable(..)` 时求值**）；位/标量遥测与变化沿事件**对任意读组照常产出**（§12.4.3 的"三句话" + §12.4.5 的作用域声明 + §12.5 产出归属表两行）。**并订正"合法配置下恒真"的错误自述**（正确 = "**承载组作用域内**、配置合法时恒真；非承载组**不适用**"）。<br>**② 补两个用例**：**B-1 判别锚** `non_carrier_group_still_emits_its_bits`（battery 站：站 2000 / `bms_alarm` 1000 / `soc` 块不声明；tick 6 轮 ⇒ 断言 `bit_call_count(1,200)==6`、`input_call_count(1,100)==3`、位地址 201（`bms_alarm_2`，`BitClass::Alarm`）的 telemetry + `is_event` **必产**；**按旧写法实现必红**）；**守卫负向锚** `station_flag_guard_still_scopes_to_carrier`（fire 站绕过校验构造"判据跨组" ⇒ 断言 `fire_detector_count_mismatch` / `fire_detector_addr_order_invalid` 恒 0 条；**删掉守卫即红**）。<br>**③ 建议 a–d**：AC-8-5 补 **C4 可构造用例**（3×FC04 `count:120` ⇒ `T_组 = 801.36 ms` ⇒ `1.5×T_组 = 1202.04 ms`；并如实登记"C4 违规域被 C9 真包含 ⇒ 只能由**文案**区分"）；AC-8-3 钉 **tick 序 `0,1000,…,9000` + 逐项计数（2/2/34/3/0 事件）**；AC-8-8 **移出用例映射**；AC-8-7 ② 退避断言改 **fail 两轮 ⇒ `2×组周期`**。<br>**④ 口径偏订正**：`mapper.rs:363-372` 订正为 **`read_back` 取数段**（容量算式在 **`:377-385`**）；"既有 20 例"订正为 **`scheduler.rs` 44 例（38 `#[tokio::test]` + 6 `#[test]`）**。<br>**⑤ Q-22 选项 B 补第三环**（Δ-10）：放开 C6/C7 后**仍须 `fire` 站级 = 5000**（否则违 C5）⇒ **承载组由 `fire_sys`(1000) 变 `fire_det`(5000)** ⇒ 站离线判定节奏 1 s → 5 s，须与"完整性守卫 + 合并视图"一并登记。<br>**⑥ 占用率口径统一**（§12.6）：`U_前 = 47.52/5000 = 0.95 %`、`U_后 = 0.026848 = 2.68 %`、倍数 2.8×，**不得**改用 §9.8.1 的取整值 48 ms（那会得 0.96 % 且与"改造后"不同源）。<br>**⑦ 同轮连带**：02 PRD §10 的 4 项文档级订正 + 4 条建议已**同轮落入 PRD 正文**（PRD v1.13）；本章与 PRD 逐位一致。<br>**未改**：§1–§11、任何代码/配置、**既有门禁标记**（`[DESIGN_APPROVED: 2026-09-21]` / `[CODE_REVIEWED: PASS: 2026-09-22]` 原文未动；其覆盖范围仍只到 §11）。**本次未加任何新门禁标记**（待复审） |
| **v1.12（2026-09-23）** | **新增 §12「块级采集周期覆盖（告警位单独快采，S3b-3）」**（对应 02 PRD §10 v1.12，用户 2026-09-23 就 12 号 R-33 的裁定）：① **配置**——`RegBlockConf` 新增 `interval_ms: Option<u64>`（`#[serde(default, skip_serializing_if)]`，缺省 = 继承站周期 ⇒ **零行为变化**）+ `effective_interval_ms`；② **内部结构**——`ReadGroup` / `GroupKey` / `GroupPoll` / `DueEntry{key,is_carrier,group_fail_count}`，`PortRunner` 增 `carrier_anchor` / `groups_of_station`，**`trackers` 键由站改为 `GroupKey`**；③ **调度器**——`read_groups_of`（唯一分组实现）+ `carrier_group`（C8）+ `DueCalc::due_round/delay_group/bump_group_fail/clear_group_fail` + `run_port_round` + `poll_group`（含**只在承载组调 `poll_to_result`** 与 `judges_evaluable` 运行期守卫）；④ **失败语义**——「站级 `offline`/`online` 由**站级承载组**唯一承载，**块级组失败不升级为站级 offline**」+ 组级退避 + "站恢复重建**全部组**基线"（两条硬理由：不遮蔽正在上送的告警位、消除周期性事件对刷屏）；⑤ **带宽重算**——按 §9.8.1 公式复算首例：`T_快组 = 21.68 ms` / `T_慢组 = 25.84 ms` / `U_口 = 2.68 %`（精确 0.026848；改造前 0.95 %），三档敏感性 1.60/2.68/4.85 %，并复核现网 6 站的 C9（最大 `meter_batt` 0.310 ≤ 0.5）；⑥ **校验落点**——新增 `validate_block_intervals`（**规则 20–23**，插入为判定顺序的 ②′）+ 常量 `MIN_POLL_INTERVAL_MS`（与 `PCS_MIN_INTERVAL_MS` 同源，旧名保留为别名）；⑦ **测试策略**（13 条单测 + 集成回归）+ **Task T8/T9/T10**；⑧ 差异上报 **Δ-10**（§9.5.4 的"探测器块独立降频"需块级拆分而 `fire` 站受 C6/C7 约束 ⇒ 本轮按整站 5000 ms，登记 PRD Q-22）/ **Δ-11**（§9.8.1 的 `grid_meter` 行缺 `T` ⇒ 不擅填，登记 PRD Q-21 ③）；风险 R-7…R-11。**未改**：§1–§11、PRD、任何代码/配置、**既有门禁标记**（`[DESIGN_APPROVED: 2026-09-21]` / `[CODE_REVIEWED: PASS: 2026-09-22]` 原文未动）<br>**⚠️ 订正（v1.13）**：本行的"**未改 PRD**"在 v1.13 轮次被推翻 —— 设计评审对 02 PRD §10 提出的 **4 项文档级订正 + 4 条建议**要求落到 **PRD 正文**，已随 v1.13 同轮执行（见 v1.13 行第 ⑦ 条与 §12.10.2 末的对照表）。**§1–§11 与门禁标记仍未改动**（该部分承诺继续成立）。 |
| **v1.11** | D-1 裁定闭合（空调串口校验位按 ①：新增站级 `parity`、空调站配 `even`、同口一致性校验同 `baud_rate`）同步至 §11.4.1 / §11.4.5 / §11.5.1 / §11.11.2 / §11.11.3 / §11.12 / §11.14。 |
| **v1.10** | 处置 `[CODE_REVIEWED: PASS]` 的两项文档口径警告 W1/W2：fmt 判据 ② 收敛为判据 ① 的推论并显式登记「S3b-2 格式债基线清单」（66 条）；AC-5 括号注订正为等价落点表述。 |
| **v1.9** | 现场核对结论同步：RC-11（消防/空调总线归属）撤销并保留原条目与结清依据；§11.3 补"设备-端口映射的硬件依据"。 |
| **v1.8** | 消防登记数容量式订正为 `1 + Σ/6`（含链首不可得时的 `Σ/6` 降级）；`SocOutcome` 补"读回长度不足"判据；`tests/s3b2_decode_e2e.rs` 归属订正；登记 `offline` 文案 `：{reason}` 后缀与 `cylinder_pressure_configured` 取数接缝。 |
| **v1.7** | 事件口径裁定：站级派生量按状态翻转产事件（`@recovered`、状态未变不产、首次观测即产）；`WordEnum` 退出语义与消费方成组判读约束；探测器 1（寄存器 11）纳入升序链首；fmt 判据 ① 的抽取规则订正。 |
| **v1.6** | 点表查表键补 `AddrSpace` 维度（`(role, space, addr)`）；AC-7 / T7 的 `cargo fmt` 门禁订正为行级可验证口径（新增 §11.11.2.1）。 |
| **v1.5** | 订正 §11.11.2 规则 15 判据 ③ 用例期望值为 `Err`（四判据为合取）；§11.5.3.5 的 5 例假绿移入 A17–A21。 |
| **v1.4** | 按二轮设计评审修订：`grid_meter` 行按生效配置真值重写并加"取值真源"注；钉死 C2 与规则 10 的判定顺序约束；登记 Δ-9；并改净 `bms_alarm_225` 示例、`RegBlockConf` 字段数 6→10 等建议项。 |
| **v1.3** | 按首轮设计评审修订：第 15 条重写落点并给出「六站必然通过」的逐站证明；取"查不到行不拒"；新增统一事件模型与消防信号清单；§11.5.3 重建为三类清单；补规则 18/19 与 RC-9/RC-10/RC-11。 |
| **v1.2** | 新增 §11「站级南向设备语义点表集成（S3b-2）」：方案探索、能力补齐（G-1…G-6）、`Role::Pcs`、17 条配置期规则、`POINT_REGS` 点表登记注册表、AC/RC 落点。 |
| v1.1 | BECG-3568 站级多从站统一调度（S3，§10）：`mupc-southd` 调度器、`south_stations` 配置、role 映射、master_meter 收敛 meter_grid、口内多从站轮询与站级故障隔离、DeviceRegistry 接线、SOC 源融合接口（04 §2.11 注）。 |
| v1.0 | 初版：定义南向通信模块实现级设计（RS485/HPLC/插件系统）。 |
