# MUPC 南向通信模块 产品需求文档（PRD）

> 文档版本：v1.13（2026-09-23）｜状态：**需求评审通过（终审 2026-09-21）**；v1.7 / v1.8 / v1.9 / v1.10 / v1.11 为评审通过后的 **§9 增量补登/订正**，v1.12 为 **§10 新增章节**，v1.13 为 **§10 的 4 项文档级订正 + 4 条建议改进**（设计评审附项落正文；均不构成重新评审）｜§9 站级南向设备语义点表集成（S3b-2）；**§10 块级采集周期覆盖（告警位单独快采，S3b-3）**
>
> **[REVIEWED: REJECTED: 2026-09-21]** 点表→`regs` 配置的落地机制缺失且自相矛盾、配置期校验规则与 §9.5.1 点表互斥、AC-2 与点表数值不符 → 退回需求分析师（详见审查意见）
>
> **[已修订待复审：v1.3，2026-09-21]** 逐条处理 A-1…A-4 / B-1…B-3 / C-1…C-4：① 给出**唯一**的「点表 → `regs` 块」落地规则（块 = 读窗口 + 传输口径；换算口径逐点声明，见 §9.4.2.1）并据此重算全部设备的请求数/单轮耗时/余量（§9.5.x.E、§9.8.1）；② 重裁定带负偏移量的 16 位量为 **`int16`**，并给出正确的配置期拒绝条件（§9.4.3）；③ 逐条回原文复算 AC-2 全部数值并补齐 AC-1 所需的 6 站完整 YAML（§9.4.1）；④ 删除「消防/PCS 软件版本号可读」的错误主张（§9.8.4）。**复审判定：REJECTED（仍未通过）——A/B/C 各段主张基本成立，但 §9.4.1 的「6 站完整 YAML」（AC-1 的验收输入）不满足既有 `south_stations.validate()`，AC-1 断言为假；另有 2 处点表数值自相矛盾，详见复审意见。**

> **[REVIEWED: REJECTED: 2026-09-21（复审）]** 逐条回原文复验 A-1…A-4 / B-1…B-3 / C-1…C-4：数值复算与带宽重算**成立**（battery 286ms / meter_batt 310ms 按本文公式可精确复现；位块与点名口径全文统一；G-6 延后口径一致），但发现 3 项须修：① **§9.4.1 的 meter_grid 段与既有配置/校验器冲突** —— 该段仅 1 个 `p` 块且站 id 写作 `meter_grid`，而生效配置（`deploy/config/mupc_core_config{,.production}.yaml`）为 `id: grid_meter` + 6 块（p/q/pf/u/i/p_total），`mupc-southd::config::validate()` 硬性要求 p/q/pf/u/i 四块存在（缺即 `Err`，启动期 `core_config.rs` 调用）⇒ AC-1 「该 YAML → `validate()` 通过」为**假**；② **ADL400 不平衡度 scale 差 10×** —— §9.4.1 `mb_phase` 的 `at:13/14` 只声明 `format: uint16`，继承块级 `scale: 0.01`，与 §9.5.3 表 / §9.7.5 的 `0.1`（原文「整型 单位0.1%」）矛盾；③ **BMS 116「量程 −1600.0 … +1676.7 A」不可自洽** —— `int16`+`0.1`+`offset −1600` 的满量程为 [−4876.8, +1676.7]，所写下限只在 raw 取无符号 0…32767 时成立，而 AC-2 以 raw=65535→−1600.1 A 作为「必须 int16」的证据，该值恰低于所声明下限。**退回需求分析师订正后再复审。**

> **[已按复审意见修订（v1.4），待三审：2026-09-21]** 逐条处理复审判定：① **阻塞 1（§9.4.1 配置与生效配置/校验器冲突）** —— 示例第 6 站改为**照录生效配置**：站 `id: grid_meter`（不再是 `meter_grid`，`meter_grid` 是 `role` 名）、含 **6 个相量块**（`p`/`q`/`pf`/`u`/`i` + 可选 `p_total`，`addr`/`format`/`scale`/`count` 取值逐字一致）；`AC-1` 由「该 YAML → `validate()` 通过」**降级为两级可证断言**（① 既有字段子集当前可跑通、② 完整 YAML 在实现 §9.4.3 后跑通），见 §9.9.1。② **阻塞 2（`mb_phase` scale）** —— `at:13/14` 显式 `scale: 0.1`（原文「整型 单位0.1%」），不再继承块级 `0.01`；仍为**块内逐点声明**，未退回拆块写法。③ **残留 3（C-4 未删净）** —— §9.3.1 hvac 行「单轮约 100ms」→ **≈48ms**；全文帧字节/耗时逐站复核（hvac **≈38 字节**，v1.3 的 50 为误；meter_batt 全文统一 ≈310ms，消除版本日志里的 311 与 310 并存）。④ **A-2 补充（116 量程自相矛盾）** —— 按 `int16` 满量程订正为 **[−4876.8, +1676.7] A**（raw ±32767/±32768 × 0.1 + offset −1600），并注明 −1600.0 只是 raw=0 处的值、不是下限。⑤ **5 条建议** —— hvac 帧字节 ≈38、查找键口径**限缩**（`grid_meter` 相量仍按**块名**查找）、`Q-17` → **`Q-16`**、新增点总数 400 量级 → **618 点**、严格拆块对照数（772ms/17 块/734ms）标注为**量级估算**。⑥ **Q-1** 补齐**阻塞级别/影响面/约束/解除条件**（【设计阶段阻塞】：架构师须先书面确认 §2.2 口径或给出双方案；若厂方确认相反口径则点表映射与 SOC 控制链**重新走需求评审**）；Q-4/Q-15 明确为**【投产阻塞·现场裁定，不阻塞设计】**（RC-3/RC-8）。

> **[已按四审意见修订（v1.5），待再审：2026-09-21]** 四审发现**一个贯穿全文的实质性错误**：v1.3 把一批「原文标 `UNIT` 且带负偏移量」的 16 位量改判为 `int16`，**该改判的理由是错的**（「负偏移证明有符号」不成立）。本轮回原文核实并订正：① **BMS 116/117/122/127/129/155/157/186、2991–2994 全部改回 `uint16`** —— 依据是文档 §3.1.1 类型表只定义 `UINT`/`INT`、**无 `UNIT`**，而 §5.3 输入寄存器表类型列 **110 处一律写作 `UNIT`**（判定为笔误），且 `INT` 在点表中**从未出现**；**强反证**：同文档 §5.2 用**正确拼写 `UINT` 且同样带负偏移**（`偏移：-50℃`、`偏移量：-3000A`）⇒ 负偏移是无符号零点平移的常规做法，**不能推出有符号**。② **116 量程订正为 [−1600.0, +4953.5] A**（raw 0/16000/65535 → −1600.0 / 0.0 / +4953.5 A）。③ **§9.4.3 删除两条误拒规则**（v1.2 的「`uint16`+负 offset」与 v1.3/v1.4 的「`uint16`+非零 offset」），替换为「符号性**可追溯 + 与点表一致**」的登记式校验。④ **G-2 的错误结论「带负偏移必须 `int16`」删除**并给出完整驳正（含 v1.3/v1.4 **循环论证**的说明：raw=65535 是判别点而非证据）。⑤ **AC-2 补真正的判别用例**（`uint16` +4953.5 A vs `int16` −1600.1 A，附现场判别逻辑）。⑥ **PCS 3 区 72 点逐点回原文核对，0 处不符**（PRD 与原文 `Int16`/`UInt16` 逐字一致）；增补"PCS 为**厂方逐点明写**"的来源说明。⑦ **空调有逐点类型标注**（30001/30003 原文即 `16位有符号` ⇒ `int16` **保持不改**）；**ADL400 为混合来源**、**消防无类型列** ⇒ 后者为**工程判断**，已逐条登记来源。⑧ 新增 **§9.10 Q-20**（推断依据 + 投运首日必做动作）。**顶部标记未改 PASS，保留全部历史 REJECTED 记录。**

> **[REVIEWED: REJECTED: 2026-09-21（四审）]** 独立回原文复核（BMS §5.3 逐点类型抽取 95 行、PCS 3 区 72 点逐行比对、空调/ADL400/消防原文类型列核验）：**本次修订的 8 条自我声明全部成立** —— ① 计数属实（整词口径）：`UNIT` 110（§5.3，其中 104 处纯 `UNIT` + 6 处 `UNIT32`）、`UINT` 191（§5.2 用**正确拼写且带负偏移** `-50℃`/`-3000A`，「无符号 + 负偏移」确系厂方主力做法）、`INT` 全文仅 **1** 次（§3.1.1 类型表），PM 的「`UINT` 出现 0 次」前提不成立；② BMS 目标点（116/117/122/127/129/155/157/186/2991–2994/4000–4005）原文**全标 `UNIT`**，无任何真写 `INT` 的点，116 满量程 **[−1600.0, +4953.5] A** 复算正确（0/16000/65535 → −1600.0/0.0/+4953.5）；③ **PCS 3 区 72 点与原文 `Int16`/`UInt16`/`UInt32` 逐点一致，0 处不符**（含 1018–1021、1041、1046–1049 等"看似不该为负"的点）；④ 空调**确有逐点类型列**（`16位有符号`/`16位无符号`/`BOOL类型`）；⑤ ADL400「4 字节功率类/PF 明写『有符号整形』、2 字节点无类型字样」属实；⑥ 消防**确无类型列/类型表**。**但仍有 1 项阻塞（验收契约内部自相矛盾），退回修订**：<br>**【阻塞 · AC-1 ③ 与 §9.4.3「可执行性说明」互相否定】** AC-1 ③ 把「`uint16/int16` 携带 `offset` 但点表未登记其符号性来源」列为**"均返回 `Err`"** 的可验证拒绝条件；而 §9.4.3 同一规则的第 ① 条自述「其『来源标注』在配置层**尚无对应字段** ⇒ **本轮先作为投运前核对项**（由 RC-1 保证），设计阶段须给出可解析的声明形式**再转**为可机械校验」。二者必居其一 —— 校验器看不到 §9.5 文档，无来源字段即**无法判定**"是否登记来源"，该条本轮**不可机械校验**，故 AC-1 ③ 该条为**不可满足的断言**（属「验收标准不可测」）。**修改建议**：从 AC-1 ③ 的触发清单中**删除该条**，并在 §9.4.3 处注明"① 的核验归属 RC-1 逐点比对 + §9.5 表核对（投运前项）"，将可机械校验的 ① 形态延后到设计给出点级来源字段后再纳入 AC。<br>**建议改进（不阻塞）**：① §9.4.2.4 ④ / Q-20 ① 的"110 处**一律写作 `UNIT`**"不精确（实为 104 处 `UNIT` + 6 处 `UNIT32`），建议按此拆分表述；② §9.5 前言「三种来源」表把 **ADL400 整体**归入"工程判断（文档无类型）"，与 §9.5.3/§9.7.5 的"**混合**"口径不完全一致（其 4 字节功率类/PF 有厂方标注），建议该格注明"4 字节点厂方明写、2 字节点工程判断"；③ §9.5.1 表 25/26 备注写"文档为 `UINT32`"、同节前言写"原文标 `UNIT32`"，两处对原文拼写的引述口径不一，建议统一为"原文 `UNIT32`（= `UINT32` 笔误）"；④ §9.8.1 称 286/90/310/300/48ms 是"可按公式**精确复算**"的值，其中 meter_batt 按本文公式复算为 ≈**308.8ms**（258 字节 ×1.04ms + 10×4ms），与 310ms 有 ~1ms 出入（`≈` 口径下不构成错误），建议改 309 或把"精确复算"改为"按公式估算"；⑤ Q-20 ② 的投运首日判别只点名 116/186，建议在 RC-1 中**显式点名 116/186 为必检点**（现"电流/功率必检"已隐含覆盖，显式化更可审）。<br>**未改动的已接受成果（已核对）**：§9.4.1 第 6 站 `grid_meter` 与 `deploy/config/mupc_core_config.yaml` **取值逐字一致**（id/role/port/baud_rate/slave/interval_ms + 6 个相量块）；`mb_phase` 的 `at:13/14` 仍为 `scale: 0.1`；耗时 battery 286 / meter_batt 310 / pcs 90 / fire 300 / hvac 48 全文统一；AC-1 两级结构、§9.4.2.1 块规则（A-1）、G-1/G-3/G-4/G-5/G-6 口径、Q-1 的【设计阶段阻塞】四栏均保持。

> **[已按四审意见修订（v1.6），待终审：2026-09-21]** 本轮为**收尾修订**，只处理 1 项阻塞 + 5 条建议，已获四轮评审接受的成果（§9.4.1 `grid_meter` 站、AC-1 两级结构、`mb_phase` 的 `scale: 0.1`、耗时 286/310/90/300/48ms、A-1 块/点规则、G-1/G-3/G-4/G-5/G-6、BMS 全面 `uint16`、Q-1 阻塞登记）**均未改动**。<br>**① 阻塞项（AC-1 ③ 与 §9.4.3「可执行性说明」互相否定）** —— 从 AC-1 ③ 的「触发即返回 `Err`」清单中**删除**「`uint16/int16` 携带 `offset` 但点表未登记其符号性来源」一条：配置层无该字段、校验器亦看不到 §9.5 文档 ⇒ 该断言**不可满足**（验收标准不可测）。§9.4.3 的"符号性声明一致性"① 条同步标注**不纳入 AC-1**，其核验**归属投运前 RC-1 逐点比对 + §9.5 表核对**；可机械校验的形态（设计给出**点级来源字段**后）再纳入 AC。**并逐条对表 AC-1 ③ × §9.4.3 规则清单**：① 条为**唯一**不可机械校验项；② 条补明其期望值来源（**校验器内的「点表登记值」常量表**，由设计按 §9.5 转写）⇒ 可判定；**补齐 AC-1 ③ 漏列的 4 条机械规则**（`role` 未知名、位块超上限、`addr` 非法、块区间重叠），使其与"逐条触发"的标题相符。两级可证断言结构（① 既有字段子集 / ② 完整 YAML）**未改**。<br>**② 建议 1（计数口径拆分）** —— 全文引用由"110 处一律写作 `UNIT`"改为准确口径：**整词 `UNIT` 104 处 + `UNIT32` 6 处（同族拼写，子串合计 110）**；并登记**同一文档 §5.2 保持寄存器表用正确拼写 `UINT` 164 处**（整词口径）—— 即厂方"无符号编码 + 负偏移做零点平移"的主力做法正写在该表里。**③ 建议 2（ADL400 来源）** —— §9.5 前言"三种来源"对照表把 ADL400 从"工程判断"行移出，**单列为第 4 行「混合（第 1 + 第 3 类）」**（4 字节功率类/PF 厂方明写、2 字节点工程判断），与 §9.5.3/§9.7.5 口径统一。<br>**④ 建议 3（引述口径）** —— 回原文核实：BMS §5.3 的 32 位类型列实际写作 **`UNIT32`**（6 处，与 104 处 `UNIT` 同族），故全文统一为「原文 `UNIT32`（= `UINT32` 笔误）」，删除 §9.5.1 表第 25 行"文档为 `UINT32`"的写法。<br>**⑤ 建议 4（耗时措辞）** —— §9.8.1 的"286/90/310/300/48ms 可精确复算"改为**分级表述**：battery/pcs/fire/hvac 为**按公式复算后取整**（286.1/89.9/299.7/47.6ms）；`meter_batt` 的 **≈310ms 为量级口径**（公式精确值 ≈308.8ms，与 310 不逐位相符）⇒ 数值与措辞自洽（§9.5.1 的两处"精确累加/精确值"同轮软化）。<br>**⑥ 建议 5（RC-1 点名）** —— RC-1 显式点名 **116（簇组电流）、186（簇实时充放电功率）** 为**必检点**（`UNIT`→`UINT` 推断的符号性判别关键点，判据见 Q-20 ②）。**顶部标记未改 PASS，保留全部历史 REJECTED 记录。**
>
> **[REVIEWED: PASS: 2026-09-21]**（**终审**）对 v1.6 的收尾修订作独立复验：**阻塞项已解决、未引入新问题，本 PRD（v1.6）需求评审通过**。<br>**① 阻塞项（AC-1 ③ 与 §9.4.3「可执行性说明」互相否定）确已消除** —— 「`uint16/int16` 携带 `offset` 但点表未登记其符号性来源 → 拒」已从 AC-1 ③ 的触发清单**删除**；§9.4.3 该规则①条同步标注**不纳入 AC-1**，并写明核验归属（投运前 **RC-1** 逐点比对 + §9.5 表核对），待设计给出**点级来源字段**后再以可机械校验形态纳入 AC。级联结构自洽，AC-1 ①/② 两级可证断言未受影响。<br>**② AC-1 ③ × §9.4.3 已穷举对表（逐条，非抽验）** —— §9.4.3 的 **17 条规则**（含 v1.6 补齐的 `role` 未知名 / 位块超上限 / `addr` 非法 / 块区间重叠 4 条）与 AC-1 ③ 的 **17 条触发项一一对应**，**再无第三条"AC 要求判定、而校验器无对应字段/能力"的断言**。作者新补的第 2 条问题处理**成立且可实施**：②条「`offset` 与点表登记值不一致」的期望值来自**校验器内由设计按 §9.5 转写的「点表登记值」常量表** —— 这是校验器**代码内的静态对照表**（而非"需新增配置字段"），与①条的不可判定性有本质区别；且已核对 §9.4.1 示例 YAML 的 `format`/`scale`/`offset` 与 §9.5 逐点一致，该规则**不会反过来否决 AC-1 ②**。4 条新补机械规则均与 §9.4.3 行号对应、且依 `role`/`func`/`count`/`addr`/块区间即可判定。<br>**③ 5 条建议均已落实**（计数与拼写经回原文逐词复核）：**整词 `UNIT` 104 处 + `UNIT32` 6 处（子串合计 110，全部位于 §5.3）**，四处（§9.4.2.4 ④ / §9.5 前言 / §9.5.1 / Q-20）**确已同步**；§5.2 正确拼写 `UINT` **整词 164 处**（全文 191 系含 §3.1.1 类型表与 §4 示例的总数；`INT` 全文仅 1 次 = 类型表自身）；ADL400 单列第 4 行「混合」；引述统一为「原文 `UNIT32`（= `UINT32` 笔误）」；§9.8.1 耗时分级表述；RC-1 显式点名 116/186 并附 0x0092、2991–2994。<br>**④ 已获四轮评审接受的成果未改动（复核通过）**：§9.4.1 `grid_meter` 站与 `mupc_core_config{,.production}.yaml` 取值**逐字一致**（id/role/port/baud_rate/slave/interval_ms + 6 相量块）、`mb_phase` `at:13/14` 的 `scale: 0.1`、A-1 块/点规则、BMS 全面 `uint16` 及 116 量程 **[−1600.0, +4953.5] A**、PCS 3 区 72 点类型**逐点回原文核对 0 处不符**、空调 30001/30003 `int16`（厂方明写 `16位有符号`）、ADL400 混合来源、消防工程判断登记、Q-1【设计阶段阻塞】四栏、Q-20 依据/代价/投运首日动作。<br>**⑤ 建议改进（不阻塞，供后续轮次顺带处理）**：a. §9.8.1"复算后取整"的四个精确值**常数口径不一** —— `battery` 286.1ms 系按本文自述常数 1.04ms/字节 得出（按 10/9600≈1.04167 精确常数为 **286.5ms**），而 `pcs`/`fire`/`hvac`（89.9 / 299.7 / 47.6ms）用的是精确常数；偏差 ≤0.4ms，不影响任何结论（余量 3.5×），建议统一口径或注明所用常数。b. §9.5.2 末段"表内 `#43/#44` 与 `#73/#74`"的编号是**原文序号**（原文 #44=1043、#74=1073），与本节"按值计数 1–72"的编号不同源，建议改写为"原文 #43/#44 与 #73/#74"。c. 顶部历史记录中的旧口径（四审记录的"`UINT` 191（§5.2 …）"实为全文数、v1.5 记录的"110 处一律写作 `UNIT`"）按"保留历史"未改，**判定可接受** —— 正文规范口径已全部订正且互相自洽，历史块本身即标注为既往轮次的记录。<br>**⑥ 分阶段门禁提示（承接既有登记，非本轮新缺陷）**：`Q-1` 仍为【设计阶段阻塞】（设计开工前须书面确认或给出双方案）；`Q-4`/`Q-15` 为【投产阻塞·现场裁定】；`D-1`（`parity` 字段）裁定为②时，§9.4.3 的"同口 `parity` 一致性"与 AC-1 ③ 对应触发项将失去对象（`baud` 一侧仍可验证），须随裁定同步回写。以上均已在 §9.10 显式登记，**不构成本轮不通过理由**。

> **[§9 增补 v1.7：设计阶段反馈的 3 项补登/订正（Δ-1/Δ-3/Δ-4），2026-09-21]** 设计阶段（架构师）与设计评审发现 3 项须由需求侧处置的事项，本 PRD 作**最小增量补登/订正**，**不改动已获终审通过的任何既有裁定**（`[REVIEWED: PASS: 2026-09-21]` 标记保持不动；本节不构成重新评审）：
>
> - **Δ-1【订正】`format` 缺省值表述与代码事实不符** —— §9.4.2.4 原称 `format` 缺省为 `int32_scaled`，**订正为 `float32`**（代码事实：`mupc-southd::config::default_reg_format()` 返回 `RegFormat::Float32`）；并**补登**"本轮新增块一律显式声明 `format`、不得依赖缺省"的护栏（缺省是 2 寄存器语义，16 位块漏写会静默解错值且少产点）。见 §9.4.2.4 块级/点级字段表 + §9.4.3 末的护栏说明。
> - **Δ-3【补登】块级新增配置字段 `read_slice`** —— §9.4.3 第 15 条"块取极大区间"与"现场设备单次读取上限（文档未明确、须现场实测）导致必须分片"存在真实冲突，原规则会**误拒合法分片**。故在 §9.4.2.4 正式补登块级字段 **`read_slice`（bool，缺省 `false`，仅豁免第 15 条极大性）**，并给出适用场景与"不得用于逃避合并"的四条约束；第 15 条同步补明豁免关系。**不以"按 `role` 特判"替代**（违反 §9.1.1 **G-5**）。
> - **Δ-4【补登】消防登记数点名 `fire_det_count`** —— §9.5.4 的"复合探测器登记数量"（寄存器 10）原无可引用点名，而 §9.5.4 的登记数交叉校验为**强制项**，配置期校验（点名唯一/空洞/位块等）与运行期消费均需按点名引用。故在 §9.5.4 与 §9.4.1 参考配置中正式定名 **`fire_det_count`**（与 `soc` 同为跨文档契约点）。

> **[§9 增补 v1.8：§9.4.3 第 15 条适用域裁定 + 字段必填性事实订正，2026-09-21]** 本轮为项目经理裁定后的**最小订正**（**不构成重新评审**；`[REVIEWED: PASS: 2026-09-21]` 标记保持不动），**只碰 §9.4.3 第 15 条与 §9.4.2.4 块级字段表的 `count` 行**（另同步 §9.4.2.1 第 5 条的定义源适用域，及两处订正的核对说明）：
>
> - **订正 1｜第 15 条（块落地极大性）适用域** —— 原文要求"块取极大区间"，而 §9.4.1 第 6 站 `grid_meter` 的 6 个相量块地址**严格相邻、零空洞**（`0x1000/0x1006/0x100C/0x1012/0x1018/0x101E`，`count 6/6/6/6/6/2`）⇒ 按原文**即被拒**，与 §9.4.1 ①「照录生效配置」及"既有 `meter_grid` 形态不动"**直接冲突**（线上配置迁移后启动 fail-fast）。该缺陷由需求侧与设计评审**各自独立复现**，定为**阻塞级**。裁定：① 本条**仅作用于「声明了 `points` 点级清单的块」**；② **未声明 `points` 的块不参与极大性合并判定**；③ 判据由"合并后空洞 ≤ 4"**改为**"合并后 `count ≤ 120` 寄存器"（120 = Modbus 单次读保守上限）；④ `read_slice: true` 的块**豁免本条**（设备单次读上限导致的合法分片）。订正后 §9.4.1 的 6 站参考配置**必然通过本条**（逐站依据见 §9.4.3 表后"第 15 条适用域自检"）。
> - **订正 2｜块级 `count` 的必填性表述与代码事实矛盾** —— §9.4.2.4 原写"**必填**，>0；S3a 既有块缺省 2"（自相矛盾），订正为 **非必填、缺省 2**（代码事实：`mupc-southd::config::RegBlockConf` 用 `#[serde(default = "default_reg_count")]`，`default_reg_count()` 返回 `2`）。同表**其余字段的"必填/缺省"表述已逐字段回 `config.rs` 核对**，**仅此一处为事实性错误**（核对结果见 §9.4.2.4 表下说明）。

> **[§9 增补 v1.9：Q-1 关闭 + Q-19 撤销 + 设备-端口映射表，2026-09-22]** 用户于 2026-09-22 就两项遗留问题给出确认结论，本 PRD 据此作**最小增量登记**，**不改动已获终审通过的任何既有裁定**（`[REVIEWED: PASS: 2026-09-21]` 标记保持不动；本节**不构成重新评审**，体例同 v1.7/v1.8）：
>
> - **登记 1｜Q-1 关闭（BMS 主从方向已确认，【设计阶段阻塞】解除）** —— 结论：**EMS 是通信主机（master），BMS 是通信从机（slave）**；现行实现（PRD §9 与设计 §11 均按协议 §2.2 口径、`mupc-southd` 由 MUPC 主动轮询 BMS 站）**正确，无需改动**。原三处并存的原文已逐处回原文核实并定性：§2.2（EMS 主机）为**正**、§3.1 TCP 段（主控做 TCP 服务器 + 后台监控主动连接）与 §2.2 **互相印证**（TCP 服务器/客户端与 Modbus 主机/从机是两个层次的概念，Modbus-TCP 惯例为客户端 = 主机）、§3.1 RTU 段（主控做主机）**确认为笔误**。完整依据链与工程判据见 **§9.10 Q-1 行 ⑤**（原条目与证据**保留未删**）；连带的门禁标注（§9.5.1 前言、§9.9.2 RC-4、§9.10 前言阻塞档位）已同步加注。
> - **登记 2｜Q-19 撤销（消防/空调并接 BMS 485-2 的风险不成立）** —— 依据 `hw/微信图片_20260908170935_64_1061_修正.png`（BECG-3568 接线拓扑图）：`RS485-1 → PCS`、`RS485-2 → BMS`、`RS485-3 → 空调`、`RS485-4 → 关口表`、`RS485-5 → 储能表`、`RS485-6 → 消防` ⇒ 消防与空调**均直连 BECG-3568（EMS）自身**，**未**挂在 BMS 的 485 总线上 ⇒ **不存在"双 master 共总线"**，`ttyS3`/`ttyS6` 照常启用。原疑问来源（CAN 协议图 2-1-1 系储能厂家以 SCU 总控为中心的自有系统视图、BMS 位 482 只说明 BMS 确有第二个 485 口）与拓扑图订正说明见 **§9.10 Q-19 行**（原条目**保留未删**）。
> - **登记 3｜新增 §9.3.3「设备 ↔ 端口映射（硬件接线契约）」** —— 把 §9.3.1 中分散书写的 `port` 收敛为**一张统一映射表**（设备 / BECG 接口 / Linux 设备节点 / `role` / 站 `id` / `slave` / `baud_rate`），取值**全部取自 §9.4.1 的 6 站参考配置**；并附两项依据（接线拓扑图 + BECG-3568 规格书的 8 路隔离 RS485 串口定义）与"各站独占一口"的硬件成立性说明。

> **[§9 增补 v1.10：§9.5.4 登记数容量算式补链首（+1），与代码 545a37c / 设计 §11.4.6 v1.8 对齐，2026-09-22]** 本轮为**最小增量订正**（**不构成重新评审**；`[REVIEWED: PASS: 2026-09-21]` 标记保持不动）：§9.5.4 的"登记数交叉校验"容量口径原只按 `fire_det*` 前缀块计（`Σ count / 6`），**漏计探测器 1**（其寄存器 11–16 在 `fire_sys` 块内，不在 `fire_det*` 区）—— 按 §9.4.1 参考配置（`fire_det.count = 114` ⇒ `114/6 = 19`，而读回登记数为 **20**）⇒ **判据恒真**。订正为：**链首可得**（存在读成功且覆盖寄存器 11 的块）时容量 = `1 + Σ(fire_det 前缀块 count) / 6`；**链首不可得**时降级为 `Σ / 6`（与地址序校验 Q-9 的降级口径**同源**）。参考配置复核：`1 + 114/6 = 20` == 读回 20 ⇒ 一致。全文其余位置（§9.4.3、§9.8、§9.9、§9.10）无同款算式，无需改动。

> **[§9 增补 v1.11：D-1 裁定（空调串口校验位，采用方案①），2026-09-22]** 用户于 2026-09-22 就 §9.10 的 **D-1** 给出裁定结论，本 PRD 据此作**最小增量登记**，**不改动已获终审通过的任何既有裁定**（`[REVIEWED: PASS: 2026-09-21]` 标记保持不动；本节**不构成重新评审**，体例同 v1.7–v1.10）：
>
> - **登记 1｜D-1 裁定：采用方案 ①（新增站级 `parity`），方案 ② 被否** —— ① **新增 per-station `parity` 配置字段**（`none` 缺省 / `even` / `odd`），**空调站配 `even`**；**同口各站的 `parity` 必须一致**（与 `baud_rate` 同级、同一道校验规则——同口共享物理校验位，配置不一致会被静默忽略）；② **方案 ② 被否**（不采用"要求现场把空调参数 40016 置 0"：依赖现场操作、不可复现、不可审计）；③ **空调站的投产限制随之解除** —— 原"在裁定前空调站不得投产"**不再适用**，空调站可与其余站点一同投产（真机验收项 §9.9.2 **RC-6** 仍须做）。完整五栏见 **§9.10 D-1 行**（原条目与"二选一"过程**保留未删**）。
> - **登记 2｜实现已就位（2026-09-22 回代码核实，需求侧登记，四条与描述一致）** —— a. `mupc/crates/mupc-southd/src/config.rs` 已定义 `StationParity{none,even,odd}`（`#[default] None`）与站级字段 `parity`，且 **规则 16 已含 `parity` 的同口一致性校验**（`validate()` 跨站一致性段，约 308–327 行，与 `baud_rate` 同一道防线）；b. 空调站在 §9.4.1 参考配置、`mupc/deploy/config/mupc_core_config{,.production}.yaml`、`mupc/crates/mupc-southd/tests/fixtures/south_stations_s3b2.yaml` 中**均为 `parity: even`**（其余各站为 `none`）；c. `parity` **已透传至串口**（T5：`mupc-southd/src/port_runtime.rs` 的 `bus_config()` 按 `StationParity → rs485_plugin::Parity` **三值逐一对映**，现场裁定 `even` 后**只改配置即可生效、无需改代码**）；d. 综上，**§9.4.1 的 `parity` 字段自本版起生效**（原"其生效以 D-1 裁定为前提"的条件**已满足**）。
> - **登记 3｜终审记录中的"悬空待办"处置：不适用** —— 终审记录（本文档顶部 `[REVIEWED: PASS: 2026-09-21]` 第 ⑥ 段，**历史块原文未动**）曾登记：「`D-1`（`parity` 字段）裁定为 **②** 时，§9.4.3 的"同口 `parity` 一致性"与 **AC-1 ③** 对应触发项将**失去对象**（`baud` 一侧仍可验证），须随裁定同步回写」。**因实际裁定为 ①（新增 `parity` 字段），该"回写"动作不适用、无需执行** —— §9.4.3 的「同口 `parity` 一致性」规则与 §9.9.1 **AC-1 ③** 对应触发项（"同口异 baud / 异 `parity` → 均返回 `Err`"）**均保留有效**，且有代码实现支撑（登记 2 a.）；原登记的前提（裁定为 ②）**未发生**，故不作任何删改。

> **[§10 新增 v1.12：块级采集周期覆盖（告警位单独快采），2026-09-23]** 用户于 2026-09-23 就 12 号模块的 **R-33** 作出裁定：**「告警位单独快采」** —— 同一站内，「告警位（FC02 位块）」与「标量测量值」使用**不同的采集周期**（位块更快，温湿度等标量维持站周期）。本 PRD 据此**新增 §10**（不修改 §9 已获终审通过的任何既有裁定；`[REVIEWED: PASS: 2026-09-21]` 标记保持不动；**本节不构成重新评审**，体例同 v1.7–v1.11）：
>
> - **登记 1｜新增 §10「块级采集周期覆盖」** —— 允许块级（`regs[]` 元素）以**可选字段 `interval_ms`** 覆盖站级周期；缺省 = 继承站周期 ⇒ **对既有配置与既有行为零变化**（§10.3）。含取值约束组（C1–C9，§10.3.2）、总线负载上界（§10.5）、边界与异常（§10.6）、验收标准 **AC-8-1…AC-8-8**（§10.7）。
> - **登记 2｜首例应用场景 = HVAC 位块快采** —— `hvac` 站 `interval_ms: 5000`、`hvac_di`（FC02 31 位）单独声明 `interval_ms: 1000`、`hvac_in`（FC04）维持站周期 5000 ⇒ 使 **12 号 PRD F25.3「离散告警位变化后 ≤2 s 上屏」** 的**采集侧前置**成立（现网 5 s 站周期下**物理不可达**）。**本轮唯一有实际收益的场景**（依据见 §10.4 末段：`meter_grid`/`battery`/`fire` 三站的判据块受 §10.3.2 规则 C6/C7 约束，不予提速；`meter_batt`/`pcs` 站周期已是 1000 ms、无收益）。
> - **登记 3｜回写 12 号设计 R-45** —— 12 号设计 §15.9 的 **R-45**（依赖 02 号提供"独立于站轮询周期的短周期采样通道"）状态由「**尚未落地**」更新为「**需求已立（02 PRD §10 / 02 设计 §12），实现待 S3b-3**」；**仅改 R-45 该行**，12 号设计其余内容与本 PRD 其余章节均未改动。
> - **登记 4｜边界声明（避免两边互相甩锅）** —— 本能力**只保证"告警位以更快周期被采集"**（采集侧），**不承诺端到端 ≤2 s**：12 号 F25.3 的端到端还含屏侧链路（组帧 + HMI 轮询 + 渲染，12 号设计 §15.6.1 记为 `+1.35 s`），其达标与否由 **12 号 R-33 的口径裁定**决定（见 §10.8 的逐段算式）。另：**F25.4「站离线变化 ≤2 s 上屏」的判定下界是 `stale_timeout_s` = 5 s（阈值语义，与轮询节奏无关）⇒ 本能力对 F25.4 的口径 B 无改善**，须由 12 号与产品就 `stale_timeout_s` 取值另行裁定（§10.8 登记 3）。
> - **登记 5｜新增待确认项 Q-21 / Q-22 / Q-23**（§10.9）：Q-21 位块周期的最终取值与 `south_stations.poll_ms` 的联动；Q-22 消防站 n>20 的"探测器块独立降频"是否要求块级拆分；Q-23 块级组失败期间其位点在 `latest_values` 的可得性判据（属 01 号 `mark_station_polled` 的粒度问题）。
> - **实现状态**：**尚未实现**（开发由后续 S3b-3 任务做）；§10.3.2 的约束组中，C1–C9 的**配置期落点**与运行期改造由 **02 设计 §12** 给出。

> **[§10 需求评审通过（v1.12，`[REVIEWED: PASS: 2026-09-23]`，需求评审员）]** 对 §10 作**首次评审**（§9 的 `[REVIEWED: PASS: 2026-09-21]` 与全部历史标记未动）：完整性（范围/能力/配置契约 C1–C9/边界 8 条/AC-8-1…AC-8-8/依赖与边界声明）与清晰度（约束**可机械判定**、数字**可复算**）**通过**，**AC 逐条可测性复核通过**：
>
> - **① AC-8-1/8-2 的判据真实可执行** —— 读计数器**实测存在**（`mupc/crates/mupc-southd/src/port_runtime.rs:257` `input_call_count` / `:280` `bit_call_count`）；tick 序列 `0,1000,…,9000`（10 tick）下 `10` 与 `2` 的**比率 5 = 5000/1000** 确能区分快慢组：`due_round` 的推进（`scheduler.rs:147-155`）在 t=5000 后把慢组 `next_due` 推到 10000 ⇒ t≤9000 内恰 2 次，且快组每 tick 到期 ⇒ 10 次；标量 3 点各 2 次同理。
> - **② C5（上界 = 站周期）确已封住"块比站慢"的降级旁路** —— 既有 `meter_grid`/`battery` 的 `interval_ms < DATA_FRESHNESS_MS` 校验**只看站级值**（`config.rs:249-260`）；若允许块周期 > 站周期，判据块即可超出 5 s 窗口而不触该校验 ⇒ 本条是必要的硬约束，不是冗余。**建议（不阻塞）**：把"若将来放开 C5（Q-22 选项 B），位点新鲜度判据（`data-processing/src/latest_values.rs:263/270-271`：位点新鲜度**只由站级活性**给出）会静默放宽 ⇒ 须与 01 号同步重裁"补入 C5 依据列。
> - **③ F25.4 的 `stale_timeout_s` 阈值语义边界写清、且已实地核对 12 号侧** —— 12 号 PRD F25.4 原文即"判定阈值沿用 `stale_timeout_s` = 5 s，本 PRD 不重定义"（`12-MUPC-本地显示终端-PRD.md:932`/`:790`）；12 号设计 **R-45 行已按其口径回写**并明载"**不改善**、口径 B 仍 `5 + 1.35 = 6.35 s`""**降级口径继续有效**、不假定其存在"（`plans/modules/12-MUPC-本地显示终端-设计文档.md:2871`）⇒ **12 号不会误读为达标**。§10.8 的其余算式（口径 A `1.0+0.75=1.75 s`、通知路径 `1.85 s`、兜底 `2.35 s`）与 12 号设计 §15.6.1 / R-33 / T-22 **逐位一致**，边界划分无甩锅空间。
> - **④ Q-21 / Q-22 / Q-23 均判为非阻塞** —— **Q-21**：能力实现与取值解耦（T8/T9 可开工），但 **AC-8-1/8-2/8-6 的数字已按 1000 ms 写死** ⇒ 若改取 ③ 500 ms 须连带改 AC 与段级 `poll_ms`，故**须在 T10（配置落地）前裁定**；**Q-22**：本轮取**选项 A（= 现状"整站 5000 ms"）**，与 §9.5.4 字面的差异已如实登记 ⇒ 非阻塞；**Q-23**：已有降级口径（`warn` + 组级退避）+ 升级路径（01 号 `mark_station_polled` 粒度），且**不违反** 12 号 F25.1"不得新造第二套新鲜度口径"的禁令 ⇒ 非阻塞，但**投产前**须由产品/01 号**书面接受**该"快组死而位点仍显示 Ok"的盲区（见文末建议）。
>
> **附 4 项非阻塞订正（须随本 PRD 下一轮顺带处理；均为文档级，不改任何契约/断言，故不构成本次不放行的理由）**：
>
> - **a. C7 依据列的引用不实** —— §10.3.2 C7 依据写「§10.9 **Q-21 ②** 登记了"允许 R 提速"的备选口径与代价」，而 §10.9 Q-21 全文只有**位块周期三档取值**（① 2000 / ② 1000 / ③ 500）＋ `grid_meter` 补算附项，**并无该登记**；§10.2.2 范围外表把同一事项归给 **Q-22** 亦不成立（Q-22 只谈 fire n>20）。⇒ **须补登该备选口径（或删引）**，否则读者按引用查不到依据。
> - **b. 行号引用偏差** —— §10.8.1 引「`12-MUPC-本地显示终端-PRD.md:783`」，而 **F25.3 的验收原文在第 788 行**（783 是 F25 的**用户故事**）。
> - **c. "改造前占用率"两处口径不一** —— §10.4 表写 **≈0.96 %**（`48 ms / 5000 ms`，取 §9.8.1 的**取整后** 48 ms），版本演进 v1.12 写"原 **0.95 %**"（`47.52 / 5000`，取**本表公式值**）⇒ 须统一为一个口径并注明所用常数（与终审 ⑤a 对 §9.8.1 常数口径的要求同款）。
> - **d. Q-22 选项 B 的可行性描述缺一环** —— 即便按 B 放开 C6/C7，仍须把 **fire 站级 `interval_ms` 设为 5000**（否则 `fire_det` 声明的 5000 违 **C5**），且此时**承载组 = 5000 组** ⇒ 站离线判定时延随之变为 5 s（与"整站 5000"同）—— 该连带影响须与"完整性守卫 + 合并视图"一并登记。
> - **建议改进（不阻塞，供下一轮顺带处理）**：a. **AC-8-5 未覆盖 C4**（`iv ≥ 1.5×T_组`）—— 现网 6 站的 `T_组 ≤ 310 ms` ⇒ `1.5×T_组 ≤ 465 ms < 500 ms`，**C4 被 C2 完全吸收**，该条只能靠构造大组反例才可测，建议在 AC-8-5 写明构造法；b. **AC-8-3 的可测性弱于 AC-8-1/8-2**（"逐条相同"未钉 tick 序列与计数），建议与 8-1 同款钉法；c. **AC-8-8 属"禁止越界断言"的元要求**（不可执行验证），建议移入 §10.8 的边界声明；d. **AC-8-7 ② 的退避断言偏弱** —— `backoff_extra(组周期, 1) == 组周期`，而 `due_round` 已把 `next_due` 推进一个组周期 ⇒ 该断言无法区分"退避生效"与"未生效"，建议改用 `fail` 两轮（oc=2 ⇒ 2×）构造。

---

> **[§10 增补 v1.13：§10 的 4 项文档级订正 + 4 条建议改进（设计评审附项落正文），2026-09-23]** 本轮**只处置设计评审对 §10 提出的"非阻塞文档级订正"**（评审块原文见上，体例同 v1.7–v1.11：**`[REVIEWED: PASS: 2026-09-21]`（§9）与 `[REVIEWED: PASS: 2026-09-23]`（§10）标记均保持不动，本节不构成重新评审**；**不改任何 C1–C9 约束、不改任何 AC 的可测判据**）：
>
> - **订正 a｜C7 依据列的引用不实（评审附项 a）** —— §10.3.2 **C7** 原引「§10.9 Q-21 ② 登记了"允许 R 提速"的备选口径与代价」，而 §10.9 Q-21 全文只有**位块周期三档取值**（① 2000 / ② 1000 / ③ 500）＋ `grid_meter` 补算附项，**并无该登记**；§10.2.2 范围外表把同一事项归给 **Q-22** 亦不成立（Q-22 只谈 `fire` n>20 的"**降频**"）。⇒ **改为三处真依据**：§10.2.2 范围外表的「判据块提速」行 + §10.3.2 的 `R(role)` 定义表 + §10.4 末的"可提速一览"表；并明写"**若要为判据块提速，须由需求侧另立条目**"。§10.2.2 该行的依据列同步订正（删 Q-22 引）。
> - **订正 b｜行号引用偏差（评审附项 b）** —— §10.8.1 原引 `12-MUPC-本地显示终端-PRD.md:783`，而 **F25.3 的验收原文在第 788 行**（783 是 F25 的**用户故事**，非验收条款）⇒ 已订正为 **`:788`** 并加注。
> - **订正 c｜"改造前占用率"口径不一（评审附项 c）** —— §10.4 表原 `≈0.96 %`（`48 ms / 5000 ms`，取 §9.8.1 的**取整值**）vs 版本演进 v1.12 的 `0.95 %`（`47.52 / 5000`，取**公式值**）⇒ **统一为"公式精确值"口径**：`U_前 = 47.52 / 5000 = 0.95040 % ⇒ 0.95 %`；配 `U_后 = 0.026848 ⇒ 2.68 %`；倍数 `2.8248 ≈ 2.8×`。§10.4 已加"占用率口径"对照表并明写"**不得**改用取整值 48 ms"。**口径与 02 设计 §12.6 逐位一致**。
> - **订正 d｜Q-22 选项 B 漏一环（评审附项 d）** —— 选项 B 原只写"完整性守卫 + 合并视图"**两件事**；补第三件：**仍须把 `fire` 站级 `interval_ms` 设为 5000**（否则 `fire_det` 声明的 5000 **违 C5**）。其连带后果一并登记：① `fire_sys` 的 1000 ms 改由**块级覆盖**表达；② **站级承载组由 `fire_sys`（1000）变为 `fire_det`（5000）** ⇒ **站离线判定节奏由 1 s 变为 5 s**，**须由 12 号确认**（落在其 F25.4 显示口径上）。
> - **建议 a｜AC-8-5 补 C4 的构造法** —— C4（`iv ≥ 1.5×T_组`）的违规域被 **C9** 真包含、且现网 `T_组 ≤ 310 ms` 使其被 **C2** 完全吸收 ⇒ **无法自然触发**。AC-8-5 已补**可构造算式**（3×FC04 `count: 120` 同组 ⇒ `T_组 = 801.36 ms`、`1.5×T_组 = 1202.04 ms`；断言文案含 `1202`），并**如实登记"C4 与 C9 必然同时触发、只能由文案区分"**。
> - **建议 b｜AC-8-3 钉 tick 序列与计数** —— 已补：tick 序 `0,1000,…,9000`（10 tick）+ 四项计数断言（读事务各 2 / `on_station_telemetry` 2 次 / 首轮 34 项与次轮 3 项 / 事件 0），与 AC-8-1 同款钉法。
> - **建议 c｜AC-8-8 移入边界声明** —— **AC-8-8 已从 §10.7 的 AC 表移出**（元要求"不得越界断言"，不可执行验证），落入 **§10.8.1 边界声明第 3 条**；**编号保留不回收**，且**不再计入 02 设计 §12.8 的用例映射**。§10.7 表下与 §10.10 已同步。
> - **建议 d｜AC-8-7 ② 的退避断言改"失败两轮"** —— 单轮时 `backoff_extra(组周期,1) == 组周期` 与 `due_round` 自身的 `next_due += 组周期` **数值相同**，无法区分退避是否生效 ⇒ 已改为 **连续 `fail` 两轮（`oc = 2` ⇒ 断言 `2 × 组周期`）**。
> - **未改**：C1–C9 的任何约束内容、AC-8-1/8-2/8-4/8-6 的判据、§10.4 的 `T`/`U` 数字（只统一**口径表述**）、§10.5/§10.6/§10.9 的条目、§1–§9 的任何既有裁定、任何代码/配置；**§9 的 `[REVIEWED: PASS: 2026-09-21]` 与 §10 的 `[REVIEWED: PASS: 2026-09-23]` 标记原文未动**。**02 设计 §12 已按其设计评审意见同步修订（v1.13，待复审）。**

---

## 1. 产品概述

### 1.1 背景与定位

MUPC 微电网特种调控装置通信管理模块是"异构双核心模块主控架构"中的**非实时处理核心**（大脑）。Phase 1 已完成北向通信网关（IEC 104）和核心架构设计，建立了与调度主站的通信能力。Phase 2+ 需要扩展南向通信能力，支持与台区设备（TTU、光伏逆变器、充电桩、柔性负荷、消防控制等）的直接通信。

**南向通信模块职责：**
- 建立统一的南向设备抽象层，支持多协议插件化接入
- 通过 RS485 总线与台区设备通信
- 通过 HPLC（高速电力线载波）与台区设备通信（预留）
- 通过动态插件系统支持协议扩展和运行时加载

### 1.2 目标平台

| 项目 | 要求 |
|------|------|
| 操作系统 | Linux (openEuler 22.03+) |
| 硬件 | RK3588 |
| 编程语言 | Rust |
| Rust 版本 | >= 1.75 |
| 异步运行时 | Tokio |

### 1.3 涉及 Crate

| Crate | 类型 | 说明 |
|-------|------|------|
| **device-trait** | NEW | 南向设备抽象定义（SouthDevice、ProtocolHandler、HplcDriver、Plugin） |
| **rs485-plugin** | ENHANCE | RS485 串口通信驱动，支持协议处理器注入 |
| **hplc-plugin** | NEW | HPLC 宽带电力线载波通信驱动（Mock 实现 + 芯片 SDK 预留） |
| **plugin-loader** | ENHANCE | 动态插件加载器（FFI 绑定、生命周期管理） |
| **mupc-southd** | NEW | 站级南向统一调度（配置驱动多端口多从站采集，S3）；§9 把厂方语义点表集成进该框架 |

### 1.4 支持设备清单

| 设备类型 | 通信方式 | 协议 | 处理器 |
|----------|----------|------|--------|
| **TTU**（配变终端） | RS485 | 电力行业规约 | TtuHandler |
| **光伏逆变器** | RS485 / 以太网 | 厂商私有协议 | InverterHandler |
| **充电桩** | RS485 / 以太网 | GB/T 27930 / OBC | ChargerHandler |
| **柔性负荷控制装置** | RS485 | Modbus RTU / 私有协议 | ModbusHandler / 自定义 |
| **消防控制系统** | RS485 | Modbus RTU / 火灾报警协议 | ModbusHandler / 自定义（专用 FireAlarmHandler 延后至 Phase 2+） |

> **站级南向采集（S3b-2）另纳入 5 类厂方设备**：储能 BMS（华塑储能主控模块）、两级式 PCS（**只读**）、储能电能表 ADL400、工商储火灾报警控制器、风冷空调机组。逐设备语义点表、配置契约与验收标准见 **§9**。BMS↔PCS 之间的 CAN2.0/J1939 私有链路**不在 MUPC 范围内**（§9.2.2 说明）。

---

## 2. SouthDevice 统一设备抽象

### 2.1 SouthDevice Trait

所有南向设备（RS485 / HPLC / 其他）统一实现 `SouthDevice` trait。此 trait 定义在 `device-trait` crate 中。

```rust
use device_trait::{DataFrame, DeviceError, DeviceStatus};

/// 南向设备统一接口
pub trait SouthDevice: Send + Sync {
    /// 获取设备ID
    fn device_id(&self) -> &str;

    /// 获取设备类型
    fn device_type(&self) -> &str;

    /// 获取设备状态
    fn status(&self) -> Result<DeviceStatus, DeviceError>;

    /// 连接设备
    fn connect(&self) -> Result<(), DeviceError>;

    /// 断开连接
    fn disconnect(&self) -> Result<(), DeviceError>;

    /// 读取数据
    fn read(&self) -> Result<DataFrame, DeviceError>;

    /// 批量读取（支持 >= 1Hz 遥测数据上送）
    fn read_batch(&self, count: usize) -> Result<Vec<DataFrame>, DeviceError>;

    /// 写入数据
    fn write(&self, data: &[u8]) -> Result<(), DeviceError>;

    /// 健康检查/心跳
    fn health_check(&self) -> Result<bool, DeviceError>;
}
```

### 2.2 数据结构

```rust
/// 设备状态
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceStatus {
    Online,
    Offline,
    Error(String),
}

/// 设备数据帧
#[derive(Debug, Clone)]
pub struct DataFrame {
    pub device_id: String,
    pub timestamp: u64,
    pub data: Vec<u8>,
    pub quality: DataQuality,
}

/// 数据质量
#[derive(Debug, Clone, PartialEq)]
pub enum DataQuality {
    Good,
    Invalid,
    Reserved,
}
```

### 2.3 设备错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("设备离线: {0}")]
    Offline(String),

    #[error("通信超时: {0}")]
    Timeout(String),

    #[error("数据校验失败: {0}")]
    ChecksumFailed(String),

    #[error("协议错误: {0}")]
    ProtocolError(String),

    #[error("设备忙: {0}")]
    Busy(String),

    #[error("其他错误: {0}")]
    Other(String),
}
```

### 2.4 设备类型枚举

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceType {
    Ttu,
    Inverter,
    Charger,
    FlexibleLoad,
    FireAlarm,
    Hplc,
    Unknown,
}
```

### 2.5 设备注册表

提供统一的设备注册、注销、查询机制。

```rust
/// 设备注册表接口
pub trait DeviceRegistry: Send + Sync {
    fn register(&self, device: Arc<dyn SouthDevice>) -> Result<(), RegistryError>;
    fn unregister(&self, device_id: &str) -> Result<(), RegistryError>;
    fn get(&self, device_id: &str) -> Option<Arc<dyn SouthDevice>>;
    fn query_by_type(&self, device_type: &str) -> Vec<Arc<dyn SouthDevice>>;
    fn list_all(&self) -> Vec<String>;
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("设备已存在: {0}")]
    AlreadyExists(String),

    #[error("设备不存在: {0}")]
    NotFound(String),

    #[error("注册失败: {0}")]
    RegisterFailed(String),
}
```

### 2.6 设备 ID 命名规范

```
格式: {设备类型}_{厂商}_{型号}_{序号}
示例: ttu_huawei_osu_001, inverter_sungrow_sg100_001
```

### 2.7 南向控制指令分发（SouthCommandSender）

**来源**：策略引擎模块通过 `SouthCommandSender` trait 向南向设备分发控制指令

策略引擎输出的两类南向控制指令通过 `SouthCommandSender` trait 发送到对应设备：

| 指令 | 目标设备 | 协议处理器 | 说明 |
|------|----------|------------|------|
| `pv_limit` | 光伏逆变器 | `InverterHandler` | 光伏限功率比例 [0.0, 1.0]，0=不限功率，0.5=限制到50% |
| `load_shedding` | 柔性负荷控制装置 | `ModbusHandler` | 可中断负荷切除量 (kW) |

**SouthCommandSender Trait：**

```rust
#[async_trait]
pub trait SouthCommandSender: Send + Sync {
    /// 发送光伏限功率命令
    async fn send_pv_limit(&self, cmd: PvLimitCommand) -> SouthSendResult;

    /// 发送负荷切除命令
    async fn send_load_shedding(&self, cmd: LoadSheddingCommand) -> SouthSendResult;
}

/// 光伏限功率命令
pub struct PvLimitCommand {
    pub device_id: String,      // 目标设备ID
    pub limit_ratio: f64,      // 限功率比例 [0.0, 1.0]
    pub priority: u8,          // 命令优先级
}

/// 负荷切除命令
pub struct LoadSheddingCommand {
    pub device_id: String,    // 目标设备ID
    pub power_kw: f64,         // 切除功率 (kW)
    pub priority: u8,         // 命令优先级
}
```

> **注意**：`pv_limit` 和 `load_shedding` **不通过核间通信发送**到实时控制模块，而是通过南向通信发送到对应的光伏逆变器和负荷控制装置。核间通信仅传输 `p_ref` 和 `k_droop` 双参数。

---

## 3. RS485 设备与协议处理器

### 3.1 架构

RS485 南向通信采用"统一设备抽象 + 协议处理器注入"模式。`Rs485Device` 结构体实现 `SouthDevice` trait，通过依赖注入 `ProtocolHandler` 支持多种设备协议。

```
rs485-plugin/
├── lib.rs                      # 插件入口
├── device.rs                   # Rs485Device 实现
├── config.rs                   # 配置定义
├── errors.rs                   # RS485 错误类型
├── protocol.rs                 # 帧解析 + CRC校验
└── handlers/
    ├── mod.rs                  # 协议处理器注册表
    ├── modbus_handler.rs       # Modbus RTU
    ├── ttu_handler.rs          # TTU 专用协议
    ├── inverter_handler.rs     # 光伏逆变器私有协议
    └── charger_handler.rs      # GB/T 27930 充电桩协议
```

### 3.2 Rs485Device 结构体

```rust
/// RS485 设备（支持协议注入）
pub struct Rs485Device {
    device_id: String,
    device_type: String,
    config: Config,
    port_fd: Mutex<Option<RawFd>>,
    status: Mutex<DeviceStatus>,
    handler: Arc<dyn ProtocolHandler>,  // 注入的协议处理器
}
```

### 3.3 ProtocolHandler Trait

```rust
/// RS485 协议处理器
///
/// 通过依赖注入支持多种协议
pub trait ProtocolHandler: Send + Sync {
    /// 编码请求数据
    fn encode_request(&self, device_id: &str, data: &[u8]) -> Vec<u8>;

    /// 解码响应数据
    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError>;

    /// 获取协议名称
    fn name(&self) -> &'static str;
}
```

### 3.4 协议处理器实现

| 处理器 | 实现协议 | 支持设备 | 优先级 |
|--------|----------|----------|--------|
| `ModbusHandler` | Modbus RTU | 通用 Modbus 设备 | 高 |
| `TtuHandler` | TTU 专用协议（电力行业规约） | 配变终端 | 高 |
| `InverterHandler` | 厂商私有协议 | 光伏逆变器 | 中 |
| `ChargerHandler` | GB/T 27930 | 充电桩 | 中 |
| `FireAlarmHandler` | 火灾报警协议（预留） | 消防控制系统 | Phase 2+ |

#### ModbusHandler 示例

```rust
pub struct ModbusHandler {
    device_addr: u8,
    crc_mode: CrcMode,
}

impl ProtocolHandler for ModbusHandler {
    fn encode_request(&self, _device_id: &str, data: &[u8]) -> Vec<u8> {
        // data: [func_code, addr_hi, addr_lo, ...]
        let mut frame = vec![self.device_addr];
        frame.extend_from_slice(data);
        let crc = Frame::calculate_crc(self.device_addr, data[0], &data[1..], self.crc_mode);
        frame.push((crc >> 8) as u8);
        frame.push(crc as u8);
        frame
    }

    fn decode_response(&self, frame: &[u8]) -> Result<DataFrame, DeviceError> {
        if frame.len() < 5 {
            return Err(DeviceError::protocol_error("响应太短"));
        }
        Ok(DataFrame::new(format!("modbus_{}", self.device_addr), frame.to_vec()))
    }

    fn name(&self) -> &'static str {
        "ModbusRTU"
    }
}
```

### 3.5 协议处理器注册表

```rust
pub struct ProtocolHandlerRegistry;

impl ProtocolHandlerRegistry {
    /// 根据名称获取协议处理器实例
    pub fn get(name: &str, config: &Config) -> Option<Arc<dyn ProtocolHandler>> {
        match name {
            "modbus" => Some(Arc::new(ModbusHandler::new(config.device_addr, config.crc_mode))),
            "ttu" => Some(Arc::new(TtuHandler::new(config.device_addr))),
            "inverter" => Some(Arc::new(InverterHandler::new(config.device_addr))),
            "charger" => Some(Arc::new(ChargerHandler::new(config.device_addr))),
            _ => None,
        }
    }
}
```

### 3.6 RS485 半双工控制（DE/RE GPIO）

RS485 为半双工通信，需要通过 GPIO 控制发送使能（DE）和接收使能（RE）。

```rust
pub enum Rs485Dir {
    Recv,  // 接收模式
    Send,  // 发送模式
}

/// 设置 RS485 方向
fn set_dir(&self, dir: Rs485Dir) -> Result<(), Rs485Error> {
    if let Some(gpio_num) = match dir {
        Rs485Dir::Send => self.config.de_gpio,
        Rs485Dir::Recv => self.config.re_gpio,
    } {
        gpio_set_value(gpio_num, dir == Rs485Dir::Send)?;
    }
    Ok(())
}
```

### 3.7 读-写-读事务（原子操作）

```rust
impl SouthDevice for Rs485Device {
    fn transaction(&self, request: &[u8], recv_timeout_ms: u64) -> Result<DataFrame, DeviceError> {
        let _guard = self.tx_lock.lock();  // 全局锁保证原子性

        // 1. 切换到发送模式
        self.set_dir(Rs485Dir::Send)?;

        // 2. 发送请求
        self.send_frame(request)?;

        // 3. 切换到接收模式
        self.set_dir(Rs485Dir::Recv)?;

        // 4. 接收响应
        self.recv_frame(recv_timeout_ms)
            .map_err(|e| DeviceError::Other(e.to_string()))
    }
}
```

### 3.8 RS485 通信参数

| 设备类型 | 波特率 | 数据位 | 停止位 | 校验 | 典型轮询周期 |
|----------|--------|--------|--------|------|-------------|
| TTU | 9600 | 8 | 1 | 偶校验 | 1s |
| 光伏逆变器 | 9600 / 19200 | 8 | 1 | 无 | 5s |
| 充电桩 | 19200 | 8 | 1 | 偶校验 | 10s |
| 柔性负荷 | 9600 | 8 | 1 | 无 | 1s |
| 消防控制 | 9600 | 8 | 1 | 无 | 1s |

> **说明**：以太网通信方式（TCP/IP）延后至 Phase 2+ 实现，当前 Phase 2 仅实现 RS485。

### 3.9 RS485 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum Rs485Error {
    #[error("串口打开失败: {0}")]
    OpenFailed(String),

    #[error("串口配置失败: {0}")]
    ConfigFailed(String),

    #[error("数据发送失败: {0}")]
    SendFailed(String),

    #[error("数据接收失败: {0}")]
    RecvFailed(String),

    #[error("串口读写超时")]
    Timeout,
}
```

---

## 4. HPLC 驱动

### 4.1 架构

HPLC（高速电力线载波）模块采用"通用驱动抽象 + 芯片 SDK 后续集成"策略。Phase 2 实现 Mock 驱动用于开发和验证数据通路；芯片 SDK 绑定预留接口，延后至 Phase 3 集成。

```
hplc-plugin/
├── lib.rs              # 插件入口
├── driver.rs           # HplcDriver trait
├── device.rs           # HplcDevice（实现 SouthDevice）
├── mock.rs             # MockHplcDriver（开发/测试用）
├── errors.rs           # HplcError
└── config.rs           # 配置定义
```

### 4.2 HplcDriver Trait

```rust
use crate::errors::HplcError;

/// HPLC 驱动接口
///
/// 抽象不同芯片厂商的 PLC SDK
pub trait HplcDriver: Send + Sync {
    /// 初始化驱动
    fn init(&self, config: HplcConfig) -> Result<(), HplcError>;

    /// 发送数据
    fn send(&self, data: &[u8]) -> Result<(), HplcError>;

    /// 接收数据
    fn recv(&self, timeout_ms: u64) -> Result<Vec<u8>, HplcError>;

    /// 检查连接状态
    fn is_connected(&self) -> bool;

    /// 获取驱动名称
    fn driver_name(&self) -> &'static str;
}
```

### 4.3 HplcConfig

```rust
#[derive(Debug, Clone)]
pub struct HplcConfig {
    /// 串口路径（跨平台：Linux=/dev/ttyUSB0, Windows=COM3）
    #[serde(alias = "serial_port", alias = "com_port")]
    pub port: String,
    /// 波特率
    pub baud_rate: u32,
    /// 芯片型号（FFI 预留）
    pub chip_type: Option<String>,
    /// 通道号
    pub channel: Option<u8>,
}
```

### 4.4 HplcDevice

```rust
/// HPLC 设备
pub struct HplcDevice {
    device_id: String,
    device_type: String,
    config: HplcConfig,
    driver: Arc<dyn HplcDriver>,
    status: Mutex<DeviceStatus>,
}
```

`HplcDevice` 实现 `SouthDevice` trait，通过 `HplcDriver` trait 与具体硬件解耦。

### 4.5 MockHplcDriver（开发/测试用）

```rust
/// Mock HPLC 驱动（用于开发测试）
///
/// 支持模拟数据注入，用于验证数据流通路
pub struct MockHplcDriver {
    connected: AtomicBool,
    mock_queue: Mutex<Vec<Vec<u8>>>,   // 模拟数据队列
    mock_delay_ms: u64,                // 模拟延迟
}

impl MockHplcDriver {
    pub fn new() -> Self { ... }

    /// 注入模拟数据
    pub fn inject_data(&self, data: Vec<u8>) {
        self.mock_queue.lock().unwrap().push(data);
    }
}

impl HplcDriver for MockHplcDriver {
    fn init(&self, _config: HplcConfig) -> Result<(), HplcError> {
        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn send(&self, data: &[u8]) -> Result<(), HplcError> {
        tracing::debug!("MockHplcDriver 发送 {} 字节", data.len());
        Ok(())
    }

    fn recv(&self, _timeout_ms: u64) -> Result<Vec<u8>, HplcError> {
        let mut queue = self.mock_queue.lock().unwrap();
        if let Some(data) = queue.pop() {
            return Ok(data);
        }
        Ok(Vec::new())  // 无模拟数据返回空
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn driver_name(&self) -> &'static str {
        "MockHplcDriver"
    }
}
```

### 4.6 芯片 SDK FFI 绑定（预留，Phase 3）

> **说明**：芯片 SDK FFI 绑定延后至 Phase 3 实现，Phase 2 使用 MockHplcDriver 进行开发验证。

```rust
/// 芯片 SDK FFI 绑定（预留接口）
pub struct SdkHplcDriver {
    handle: *mut std::os::raw::c_void,  // FFI 句柄
}

impl HplcDriver for SdkHplcDriver {
    fn init(&self, config: HplcConfig) -> Result<(), HplcError> {
        // 调用 libhplc.so 中的 hplc_init()
        let ret = unsafe { hplc_init(config.port.as_ptr(), config.baud_rate) };
        if ret != 0 {
            return Err(HplcError::InitFailed(format!("hplc_init failed: {}", ret)));
        }
        Ok(())
    }

    fn send(&self, data: &[u8]) -> Result<(), HplcError> {
        let ret = unsafe { hplc_send(self.handle, data.as_ptr(), data.len() as u32) };
        if ret != 0 {
            return Err(HplcError::SendFailed(format!("hplc_send failed: {}", ret)));
        }
        Ok(())
    }

    fn recv(&self, timeout_ms: u64) -> Result<Vec<u8>, HplcError> {
        let mut buf = vec![0u8; 1024];
        let len = unsafe {
            hplc_recv(self.handle, buf.as_mut_ptr(), buf.len() as u32, timeout_ms as i32)
        };
        if len < 0 {
            return Err(HplcError::RecvFailed(format!("hplc_recv failed: {}", len)));
        }
        Ok(buf[..len as usize].to_vec())
    }

    fn is_connected(&self) -> bool {
        !self.handle.is_null()
    }

    fn driver_name(&self) -> &'static str {
        "SdkHplcDriver"
    }
}
```

### 4.7 HPLC 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum HplcError {
    #[error("驱动初始化失败: {0}")]
    InitFailed(String),

    #[error("发送失败: {0}")]
    SendFailed(String),

    #[error("接收失败: {0}")]
    RecvFailed(String),

    #[error("连接断开: {0}")]
    Disconnected(String),

    #[error("SDK 错误: {0}")]
    SdkError(String),
}
```

### 4.8 HPLC 技术参数（预留）

| 参数 | 规格 |
|------|------|
| 调制方式 | OFDM（BPSK / QPSK / 16QAM / 64QAM 自适应） |
| 通信频段 | 0.7 MHz - 3 MHz |
| 物理层速率 | 2 Mbps - 10 Mbps（自适应） |
| 最大帧长 | 1500 字节 |
| 典型应用 | 台区全覆盖场景，替代 RS485 布线困难区域 |

---

## 5. 动态插件系统

### 5.1 架构

动态插件系统通过 FFI 绑定实现运行时加载/卸载 `.so` / `.dll` / `.dylib` 动态库。所有南向通信插件（rs485-plugin、hplc-plugin）需遵循此规范。

```
PluginLoader (plugin-loader crate)
    ↓
libloading (动态加载 .so/.dll)
    ↓
FFI 导出函数: create_plugin() + plugin_meta()
    ↓
Plugin trait 实例
```

### 5.2 Plugin Trait

```rust
/// 插件元信息
#[derive(Debug, Clone)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}

/// 插件接口
pub trait Plugin: Send + Sync {
    fn meta(&self) -> PluginMeta;
    fn init(&self, config: serde_json::Value) -> Result<(), PluginError>;
    fn start(&self) -> Result<(), PluginError>;
    fn stop(&self) -> Result<(), PluginError>;
    fn shutdown(self: Box<Self>) -> Result<(), PluginError>;
}
```

### 5.3 必需 FFI 导出符号

每个动态插件必须导出以下两个 `extern "C"` 函数：

| 符号 | 类型 | 说明 |
|------|------|------|
| `create_plugin` | `unsafe extern "C" fn() -> *mut dyn Plugin` | 插件工厂函数 |
| `plugin_meta` | `unsafe extern "C" fn() -> PluginMeta` | 获取插件元信息 |

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

### 5.4 插件生命周期

```
Load → Init → Start → Stop → Unload
```

| 阶段 | 操作 | 状态 |
|------|------|------|
| **Load** | `dlopen` / `libloading` 加载 `.so` / `.dll`，调用 `create_plugin()` 获取实例，调用 `plugin_meta()` 获取元信息，注册到 PluginRegistry | Loaded |
| **Init** | 调用 `plugin.init(config)`，传入 JSON 配置 | Initialized |
| **Start** | 调用 `plugin.start()`，启动插件业务逻辑 | Running |
| **Stop** | 调用 `plugin.stop()`，停止插件业务逻辑 | Stopped |
| **Unload** | 调用 `plugin.shutdown()`，从注册表移除，卸载动态库 | Unloaded |

### 5.5 PluginLoader Trait

```rust
pub trait PluginLoader: Send + Sync {
    fn load(&self, plugin_path: &str, config: serde_json::Value) -> Result<(), PluginError>;
    fn unload(&self, plugin_name: &str) -> Result<(), PluginError>;
    fn list(&self) -> Vec<PluginMeta>;
    fn get(&self, plugin_name: &str) -> Option<Arc<dyn Plugin>>;
    fn init(&self, plugin_name: &str, config: serde_json::Value) -> Result<(), PluginError>;
    fn start(&self, plugin_name: &str) -> Result<(), PluginError>;
    fn stop(&self, plugin_name: &str) -> Result<(), PluginError>;
}
```

### 5.6 插件错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("插件加载失败: {0}")]
    LoadFailed(String),

    #[error("插件初始化失败: {0}")]
    InitFailed(String),

    #[error("插件启动失败: {0}")]
    StartFailed(String),

    #[error("插件停止失败: {0}")]
    StopFailed(String),

    #[error("插件不存在: {0}")]
    NotFound(String),

    #[error("元信息错误: {0}")]
    MetaError(String),

    #[error("其他错误: {0}")]
    Other(String),
}
```

### 5.7 插件状态枚举

| 状态 | 说明 |
|------|------|
| `Loaded` | 已加载 |
| `Initialized` | 已初始化 |
| `Running` | 运行中 |
| `Stopped` | 已停止 |
| `Unloaded` | 已卸载 |

### 5.8 编译要求

```toml
[lib]
crate-type = ["cdylib"]  # 必须编译为动态库
```

| 平台 | 输出 |
|------|------|
| Linux | `target/release/libmy_plugin.so` |
| Windows | `target/release/my_plugin.dll` |
| macOS | `target/release/libmy_plugin.dylib` |

### 5.9 插件依赖关系

```
device-trait (无依赖)
    ↓
plugin-loader → device-trait
    ↓
rs485-plugin → device-trait (编译为 cdylib)
hplc-plugin → device-trait (编译为 cdylib)
```

---

## 6. 配置格式

### 6.1 RS485 设备配置

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
      "parity": "none",
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

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `device_id` | string | 是 | 设备唯一标识 |
| `device_type` | string | 是 | 设备类型（ttu / inverter / charger / flexible_load / fire_alarm） |
| `port` | string | 是 | 串口路径（Linux=/dev/ttyUSB0, Windows=COM3） |
| `baud_rate` | int | 是 | 波特率（9600 / 19200 / 115200 等） |
| `data_bits` | int | 否 | 数据位，默认 8 |
| `stop_bits` | int | 否 | 停止位，默认 1 |
| `parity` | string | 否 | 校验位（none / even / odd） |
| `timeout_ms` | int | 否 | 通信超时（毫秒），默认 1000 |
| `device_addr` | int | 否 | 设备 Modbus 地址 |
| `handler` | string | 是 | 协议处理器名称（modbus / ttu / inverter / charger） |
| `de_gpio` | int | 否 | DE (Driver Enable) GPIO 引脚编号 |
| `re_gpio` | int | 否 | RE (Receiver Enable) GPIO 引脚编号 |

### 6.2 HPLC 设备配置

```json
{
  "hplc_devices": [
    {
      "device_id": "hplc_001",
      "device_type": "hplc",
      "driver": "mock",
      "config": {
        "serial_port": "/dev/ttyUSB2",
        "baud_rate": 115200
      }
    }
  ]
}
```

### 6.3 插件通用配置

```json
{
  "device_path": "/dev/ttyUSB0",
  "timeout_ms": 5000,
  "baud_rate": 9600
}
```

插件配置通过 `serde_json::Value` 传入 `plugin.init(config)` 方法。

---

## 7. 非功能性需求

### 7.1 性能需求

| 指标 | 要求 |
|------|------|
| 南向设备轮询周期 | TTU/柔性负荷/消防：>= 1Hz；逆变器：>= 0.2Hz（5s）；充电桩：>= 0.1Hz（10s） |
| RS485 吞吐量 | 115200 波特率下无丢包 |
| 单设备读取延迟 | <= 500ms |
| 设备注册/注销操作 | <= 50ms |
| 插件加载时间 | 单个插件 <= 500ms |
| 并发设备数 | 支持 >= 100 个设备同时在线 |
| 消息总线吞吐量 | >= 10000 msg/s |

### 7.2 可靠性需求

| 指标 | 要求 |
|------|------|
| 通信丢包率 | < 0.1% |
| 误码率 | < 10^-6 |
| 超时重试机制 | 支持可配置重试次数和超时时间 |
| 故障隔离 | 单设备通信故障不影响其他设备 |

### 7.3 安全需求

| 需求 | 说明 |
|------|------|
| RS485 物理安全 | 物理层无加密，依赖物理访问控制 |
| 配置文件安全 | 设备地址和串口路径需要访问控制 |
| 插件验证 | 插件签名验证（可选） |
| 日志安全 | 日志中不得记录明文密码、密钥 |

### 7.4 可维护性需求

| 需求 | 说明 |
|------|------|
| 热插拔 | 支持运行时添加/移除设备配置（重载配置生效） |
| 动态加载 | 插件支持运行时加载/卸载 |
| 日志 | 基于 `tracing` 记录通信日志 |
| rustdoc | 公共 API 有 rustdoc 注释 |

---

## 8. 验收标准汇总

### 8.1 功能验收

| ID | 功能点 | 验收条件 |
|----|--------|----------|
| F01 | SouthDevice trait | 所有南向设备统一实现 SouthDevice trait |
| F02 | 设备注册表 | 支持注册、注销、按类型查询、列出所有设备 |
| F03 | RS485 Modbus 通信 | 能与通用 Modbus RTU 设备通信，编码/解码正确 |
| F04 | RS485 TTU 通信 | 能与 TTU 设备通信，遵循电力行业规约 |
| F05 | RS485 逆变器通信 | 能与光伏逆变器通信，支持厂商私有协议 |
| F06 | RS485 充电桩通信 | 能与充电桩通信，支持 GB/T 27930 |
| F07 | RS485 半双工控制 | DE/RE GPIO 方向切换正确，读-写-读事务原子性 |
| F08 | RS485 串口参数 | 支持配置波特率/数据位/停止位/校验位 |
| F09 | MockHplcDriver | 支持模拟数据注入，支持开发调试 |
| F10 | HplcDriver trait | 定义 init/send/recv/is_connected/driver_name |
| F11 | 插件加载 | `plugin-loader` 支持动态加载 `.so` / `.dll` |
| F12 | 插件卸载 | 支持动态卸载插件，释放资源 |
| F13 | 插件生命周期 | 严格遵循 Load -> Init -> Start -> Stop -> Unload |
| F14 | FFI 导出 | 插件必须导出 `create_plugin` 和 `plugin_meta` |
| F15 | 协议处理器注册 | 支持通过名称配置协议处理器，运行时注入 |

### 8.2 性能验收

| ID | 性能点 | 验收条件 |
|----|--------|----------|
| P01 | 插件加载时间 | 单个插件加载 < 500ms |
| P02 | RS485 吞吐量 | 115200 波特率下无丢包 |
| P03 | 设备注册/注销 | 操作耗时 < 50ms |
| P04 | 并发设备数 | 支持 >= 100 个设备同时在线 |
| P05 | 消息总线吞吐量 | >= 10000 msg/s |

### 8.3 质量验收

| ID | 质量点 | 验收条件 |
|----|--------|----------|
| Q01 | 编译通过 | `cargo build --release` 无警告 |
| Q02 | Clippy 检查 | `cargo clippy` 无 Error |
| Q03 | 单元测试 | `cargo test` 覆盖率 >= 80% |
| Q04 | 错误处理 | 所有错误实现 `std::error::Error` |
| Q05 | 无新增 unsafe | 除 FFI 导出外无新增不安全代码 |
| Q06 | rustdoc | 公共 API 有 rustdoc 注释 |
| Q07 | 格式化 | `cargo fmt` 通过 |

### 8.4 兼容性验收

| ID | 兼容性点 | 验收条件 |
|----|----------|----------|
| C01 | 操作系统 | openEuler 22.03+ |
| C02 | 硬件平台 | RK3588 |
| C03 | 编译器 | Rust >= 1.75 |

---

## 9. 站级南向设备语义点表集成（S3b-2）

### 9.1 背景与目标（Why）

**现状（S3a 交付物，2026-09-08）**：`mupc-southd` 已交付「配置驱动的多端口多从站采集」框架 —— `Role` 枚举（`meter_grid / meter_batt / battery / hvac / fire`）、`StationConf`/`RegBlockConf` 配置类型与段内校验、口级调度（每口一条 task、口内按 `interval_ms` 串行轮询）、站级超时隔离与指数退避降频、`DataPackage` 装配与 role 分发。**但只有 `Role::MeterGrid` 有真实语义映射**（`p/q/pf/u/i/p_total` 相量块 → `electrical.phase`，策略 phase 真源），`Role::Battery` 仅消费名为 `soc` 的块（v1.3 起该契约改为**点名为 `soc` 的点**，见 §9.4.3）；`Role::MeterBatt / Hvac / Fire` 在 mapper 中**返回空 `DataPackage` 占位**（仅作"站在线"信号），遥测值只能靠"每块取首值"的通用路径落库。

**本轮目标（S3b-2）**：5 份厂方协议文档已到位，把上述占位**替换为可据以编码的真实语义点表** —— 逐设备、逐语义点给出寄存器地址、功能码、数量、数据类型/格式、缩放系数、偏移、单位、有符号性、读写属性与含义，使其可直接落入 `south_stations.stations[].regs` 配置并驱动 decoder/mapper。

**与 S3a 的关系**：本轮**不改变** S3a 的调度架构、故障隔离语义与 role→DataPackage 分发骨架，只做两件事：① 补齐点表与配置（数据面）；② 补齐点表**表达与读取所必需**的能力缺口（见 §9.1.1）。设计文档 `02-MUPC-南向通信-设计文档.md` §10（`[DESIGN_APPROVED: 2026-09-08]`）的范围声明已写明「设备点表（BMS/空调/消防）待厂方提供后填配置」——即 `S3b-2` 为其既定后续，本 PRD 与 §10 不冲突。

**与已实现能力的关系（不得回归）**：① `meter_grid` 是策略 phase **唯一真源**（S3b-1c 收敛，配置期强校验），本轮新增站**一律不得接入** `AiIntegrator.latest_data`；② BMS 站 SOC 经 `on_battery_soc` 独立通道推 AiIntegrator（BMS 优先、超期回落核间，设计 §10.5、04 §2.11.1），本轮把该通道的**真实点表**（BMS 输入寄存器 118）落实；③ 仅 `meter_grid` 更新推进 5s 控制闸门时间戳，其余 role 不推进（防活性站掩盖陈旧 phase）——本轮新增站同样不推进。

#### 9.1.1 与现有实现的能力差距（本轮必须补齐，否则点表无法落地）

以下 6 项由代码事实核对得出，是本轮**范围实质包含**的能力补齐项（仅"填配置"不足以完成 S3b-2）：

| 编号 | 差距（代码事实） | 影响的点表 | 本轮要求 |
|------|------------------|------------|----------|
| **G-1** | `RegFormat` 仅有 `Float32` / `Int32Scaled`，**两者都固定占 2 寄存器**（`decode_regs` 对 `len<2` 直接返回 `0.0`） | 5 份协议中**绝大多数点表是 16 位单寄存器**（BMS `UINT/INT`、PCS `Int16/UInt16`、消防全部、空调全部、ADL400 的电压/电流/频率/PF） | `format` 扩展 **`uint16` / `int16`（各占 1 寄存器）** |
| **G-2** | 只有 `scale`，**无偏移量** | BMS 温度类（`raw−40℃`）、簇组电流（`raw×0.1 − 1600.0A`）、簇端子温度（`raw−40℃`） | **点级/块级新增 `offset`（缺省 0）**，换算 `值 = raw × scale + offset`。**偏移量不决定符号性**：`uint16` / `int16` 均允许携带任意 `offset`（工厂文档中「`UINT` + 负偏移」是无符号零点平移的标准做法，见 §9.4.2.4）；每个点的符号性**以厂方类型标注为准**（§9.5.1 / §9.4.2.4） |
| **G-3** | `StationBus` 只有 `read_holding`(FC03) / `read_input`(FC04)，**无 FC02 离散输入** | BMS 全部告警/状态位（离散输入表 200–599）、空调只读告警位（10001–10031） | 新增 **离散输入（FC02）读取能力**（`func: discrete`，`count` 语义 = **位数**；响应为按字节打包的位图，须按 §9.7.4 解包） |
| **G-4** | `mapper::telemetry_points` 对每个块**只取前 2 寄存器解 1 个标量**；`decode_regs` 只吃 2 寄存器；配置为 `count:1` 的块**不产出任何遥测点** | 单点块（BMS/PCS/消防/空调的绝大多数点）在现有实现下拿不到遥测值 | 支持 **单寄存器（16 位）块、多值块与逐点换算参数**：① 16 位点占 1 寄存器、32 位点占 2 寄存器；② 块级给缺省换算，**点级按完整点清单覆盖**（`points`，见 §9.4.2.1）；③ 未列出的寄存器**不产出遥测点**。点位命名规则见 §9.4.2.2（**唯一**，全文档只有这一处定义） |
| **G-5** | PCS 协议声明「**存在高 8 位和低 8 位互换**」，且 32 位电量寄存器明确「**低 16 位在低地址 / 高 16 位在高地址**」（与 `regs_to_u32_be` 的高字在前相反） | PCS 3 区全部 **72 个语义点（占 76 个寄存器）** | 配置须能**显式表达寄存器字节序与字序**（缺省 `大端/高字在前`；PCS 站置 `字节低-高互换`），32 位 PCS 电量置 `低字在前`（点级 `word_order: lo_hi`）。**禁止按 role 隐式特判**——换设备时会静默错读 |
| **G-6** | 寄存器内**字节拆分**无表达方式（`decode_regs` 只解整字） | （潜在）消防复合探测器数据 1（高字节=烟雾 dB/M 0.1、低字节=温度 offset −55）；BMS 位置编号 124/126/128/130 | **本轮不做（延后）**，理由与替代口径见 §9.4.2.3；两个候选点本身就是"不可靠拆"与"配置爆炸"，详见下文 |

> **G-4 的总线必要性论证（v1.3 重算）**：若回到"换算口径不同即拆块"的旧规则展开，BMS 需 **30–40 次 FC04 + 1 次 FC02** 事务/轮 —— 9600bps 下**量级 ≈772ms/轮**（**量级估算**，非精确值；复算式与误差口径见 §9.5.1 的对照算式与 §9.8.1），叠加 FC02 位块将顶破 1s 轮询预算与"单轮耗时 ×1.5 ≤ interval"的余量要求，并被迫把 `battery` 的 `interval_ms` 上调到 2000ms。因此"**读窗口合并 + 逐点换算**"不是优化项，而是**站点可上线的必要条件**（带宽核算见 §9.8.1）。
>
> **G-6 为何延后（本轮明确不做，不是遗漏）**：两个候选点各自的障碍都在"拆"之外 ——
> ① **BMS 位置编号 124/126/128/130**：原文 5.4「位置编号"位定义"」表把 Bit15–8 与 Bit7–0 **都**标为"PACK 编号（1 到 32）/（1 到 N）"，**低字节语义自相矛盾**（应为 PACK 内单体编号）。按矛盾文档拆点会产出**看似合理、实则语义不可信**的两个点（§9.10 Q-18）→ **整字采集原值**，拆分待厂方确认。
> ② **消防复合探测器数据 1**：探测器区是 **n 只 × 6 寄存器** 的重复结构（n ≤ 100）。逐点声明需 n×7 条配置；改为"每只一个块"则单轮事务数 = n（n=20 时单轮 ≈645ms，`interval_ms: 1000` 的余量仅 **1.55×**，几乎无余量，见 §9.8.1 的对照算式）→ **整字采集原值**，字节语义由展示/事件层按 §9.5.4 拆解。
>
> 两个替代口径都**保留原始寄存器值**（telemetry 落原值、可回溯），**不产生静默失真**；将来若确需模拟量进事件/策略，须以"重复结构的配置展开规则 + 带宽重算"另行立项（触发条件登记在 §9.10 D-2）。

### 9.2 范围

#### 9.2.1 范围内（主交付）

| 序号 | 设备 | Role | 主交付 |
|------|------|------|--------|
| 1 | 储能主控模块 BMS（华塑 RCU） | `battery` | 语义点表（FC04 遥测 + FC02 告警/状态位）+ SOC 控制链路 |
| 2 | 两级式 PCS | **`pcs`（新增 role）** | **只读采集** 3 区（FC04）全部 72 个语义点（占 76 个寄存器） |
| 3 | 储能电能表 ADL400 | `meter_batt` | 语义点表（FC03）：电气量 + 电能 |
| 4 | 工商储火灾报警控制器 | `fire` | 语义点表（FC03）：系统状态位 + 复合探测器块 |
| 5 | 风冷空调机组 | `hvac` | 只读语义点表（FC02 告警位 + FC04 温湿度） |

#### 9.2.2 范围外说明：BMS ↔ PCS 之间的 CAN2.0 链路（登记备查，本轮不接）

存在一份协议《储能系统总控+主控模块对 PCS 设备-CAN2.0 通信协议 A2 版》，**本轮不接、不解析、不写代码**。登记其存在与用途，避免日后被误判为"漏做"：

- **链路性质**：BMS 总控/主控模块（SCU/RCU）与 PCS 设备**直接对话**的私有链路，**两端都不是 MUPC**；MUPC 既不产生也不消费该链路上的任何报文。
- **物理层**（措辞按文档原文）：总控模块经**隔离 CAN 接口**（推荐使用 **CAN 2**，A 接口的 pin4 = 2H、pin12 = 2L）与 PCS 连接；主控模块经隔离 CAN 接口（推荐 **CAN 1**；RCU-01K8CC 的 C 接口 pin8 = 1H / pin16 = 1L，RCU-01K8CN 的 B 接口 pin12 = 1H / pin24 = 1L）。
- **通信机制（原文）**：「通信模式为 **CAN2.0**，通信速率 **250kbs**，高字节在前，低字节在后」「**PCS 做 CAN 通信主机，大储总控模块或主控模块做通信从机**」。（v1.2 曾写作"CAN2.0B / 250 kbps"，与原文措辞不符，v1.3 订正为原文用语。）
- **协议形态（原文）**：PDU **参照 J1939 协议，由七部分组成** —— 优先级(P)、保留位(R)、数据页(DP)、PDU 格式(PF)、特定 PDU(PS)、源地址(SA)、数据域；即「协议消息命令码 + 目标地址 + 源地址 + 数据段」。
- **报文与周期（原文：发送节点 / 接收节点 / 周期）**：`0x180150F1`（运行参数：单体极值电压/SOC/SOH/继电器状态）、`0x180250F1`（运行参数：总压/总流/充放允许电流）、`0x180650F1`（运行参数：电池状态/系统状态标识/一二级报警标识）、`0x180750F1`（运行参数）—— 均为**总控/主控模块 → 协调控制器**，**发送周期 200ms**，数据长度 8 字节；`0x1801F150`（PCS/HPS/PBD 状态参数）为**协调控制器 → 总控/主控模块**，**发送周期 500ms**。
- **对 MUPC 的推论**：PCS 侧掌握的 BMS 实时量（SOC/允许电流/报警）**不经该链路进入 MUPC**；MUPC 侧 BMS 数据唯一来源是本节 §9.5.1 的 485-1/LAN Modbus 点表。两侧同源但**不互为冗余**，不得假定 CAN 链路可作 MUPC 的 BMS 数据备份。

#### 9.2.3 写操作：独立 Task，不进本轮只读主线

**本轮主线 = 只读采集**（FC02 / FC03 / FC04）。写类操作**单独立项为一个 Task**，其内容与边界：

| 设备 | 写类内容（登记） | 本轮是否实现 |
|------|------------------|--------------|
| 消防 | 远程控制（保持寄存器 **1999**：0 消音 / 1 复位 / 2 启动 / 3 停止）；系统时间（**1996–1998** R/W）；本机地址 0 / 波特率倍数 1 / 系统音量 2 / 喷洒延时 3 | 否（写 Task） |
| 空调 | 保持寄存器 **40001–40059** 全部读写参数：制冷/加热停止温度与灵敏度、柜内外报警阈值、湿度设定、应急风机模式与时间、**485 地址/波特率/奇偶校验（40014/40015/40016）**、各使能位（40026–40032、40040/40041、40043）、报警继电器开关 40024、报警音 40025、清除故障 40047、**开关机 40048**、**强制模式 40059** | 否（写 Task） |
| PCS | **4 区（FC03/FC06）1000–1057**：有功模式 1000、有功/无功设定 1001/1002、分相 P/Q 1006–1011、无功模式 1016、功率因数 1017、变化率 1018、恢复出厂 1019、EMS 波特率 1020…等，及 3 个功能寄存器 500–503 | **本轮范围外（明确排除）** |
| BMS | 保持寄存器 504 簇上下电控制 / 524 故障复位 / 525 绝缘检测 / 526 重启复位 / 1000–1157 各类阈值参数 | 否；**是否纳入写 Task 待评审确认**（不在已确认的三项内） |

> **PCS 写操作本轮范围外的理由（必须继承，不得以"顺手实现"为名绕过）**：PCS 的 **4 区寄存器 500 = 模块启停**（0 停机 / 1 运行），该寄存器已被已验收的 **S2 DI/DO 安全联锁**用作**停机原语** —— 联锁触发即写 `500=0` 并**锁存禁启**（`intercore` 的 `ModbusRtuTransport` 负责，见 `plans/archive/2026-09-08-S2-DI-DO安全联锁.md`）。若 `southd` 的 PCS 站同时写 4 区，将出现**两个写方争用同一停机原语**：联锁锁存期间的下行中止语义、停机确认（`1013` 转 0）与人工释放流程都会被旁路。**分工未定前，southd 对 PCS 一律只读**；PCS 写操作的归属（并入 intercore 还是下放 southd）须单独评审后再立项。

### 9.3 设备与 Role 清单

#### 9.3.1 Role 与物理链路

| Role | 设备 | 物理链路 | 协议 / 帧格式 | 从站地址 | 轮询周期 | 周期理由 |
|------|------|----------|----------------|----------|----------|----------|
| `meter_grid` | 关口/台区总表 | RS485 `/dev/ttyS4`（BECG COM4） | Modbus-RTU，9600（现场校准） | 3（现场校准） | 1000ms（**须 <5000**） | 策略 phase 真源，5s 新鲜度窗口（既有实现，**本轮不动**） |
| **`pcs`（新增）** | 两级式 PCS（只读 3 区） | RS485 **`/dev/ttyS7`（建议值，待现场校准）** | Modbus-RTU，**19200 N-8-1**（无校验） | 1（拨码默认） | 1000ms | 只读健康/校核；3 区单次事务约 90ms，1s 预算余量充足；与 intercore 心跳（同设备另一口）解耦 |
| `battery` | 储能主控模块 BMS | RS485 `/dev/ttyS2`（BECG COM2） | Modbus-RTU（协议亦支持 TCP，本轮用 RTU），**波特率/校验位待现场核对**（见 §9.10 Q-2） | 1（= 簇号） | **1000ms（须 <5000，配置期强校验）** | SOC 是控制输入；BMS 站 `interval_ms ≥ 5000` 会让 SOC 每轮仅前 5s fresh、余下回落核间，造成**源周期翻转 / soc_protect 剪带震荡** |
| `hvac` | 风冷空调机组 | RS485 `/dev/ttyS3`（BECG COM3） | Modbus-RTU，9600，**出厂为偶校验**（见 §9.10 Q-14） | 1 | 5000ms | 温湿度为缓变量、纯展示无控制用途（设计 §10.3 已定 5000）；单轮 **≈48ms**（1×FC04 + 1×FC02，核算见 §9.8.1） |
| `meter_batt` | 储能电能表 ADL400 | RS485 `/dev/ttyS5`（BECG COM5） | Modbus-RTU（**选 Modbus 不选 DL/T645**，理由见 §9.5.3），9600 无校验（出厂默认） | 1（现场校准） | 1000ms（若加谐波须 ≥5000） | 能量流核算与 SOC 校核，与 `meter_grid` 同周期便于同窗口比对 |
| `fire` | 工商储火灾报警控制器 | RS485 `/dev/ttyS6`（BECG COM6） | Modbus-RTU，**9600 8N1**（出厂默认） | 1（本机地址出厂默认 1） | 1000ms（系统态）；探测器块按登记数量另计（§9.5.4） | 火灾报警实时性；n=20 时单轮 2 次事务约 300ms（§9.8.1），1s 可行 |

**端口分配事实（BECG-3568）**：板载 8 路隔离 RS485 = COM1–COM8 ↔ `/dev/ttyS0`、`ttyS2`–`ttyS8`（**无 `ttyS1`**）。已占用：`ttyS0` = intercore PCS 主链路（核间 §2.4）、`ttyS2` BMS、`ttyS3` 空调、`ttyS4` 关口表、`ttyS5` 储能表、`ttyS6` 消防。**空闲：`ttyS7`、`ttyS8`**。

> **拓扑为配置预留值 + 待现场校准**：上表串口/从站号来源于 BECG §12.1 接线契约与配置文件预留值，**全部为待现场校准项**，投运前须逐站核对（不得硬编码到代码）。**新增 PCS 站的串口为建议值 `/dev/ttyS7`**，待校准。

> **继承约束（强制）**：`south_stations` 中任一站的 `port` **不得**与 `intercore.modbus_rtu.serial_port`（PCS 主链路 `/dev/ttyS0`）相同或互为别名 —— RS485 总线仲裁未实现，**禁双 master 共总线**（该跨段校验已在 `core_config::validate_south_stations` 实现，本轮沿用）。

> **PCS 只读站的接线前提（待确认，§9.10 Q-15）**：PCS 协议载明「EMS 接 485 的 A2/B2；大屏远程监控接 485 的 A1/B1」。intercore 主链路已占 A2/B2（`ttyS0`），因此新增只读站必须走**另一个物理口**并接 PCS 的 **A1/B1**。**A1/B1 与 A2/B2 是否为相互隔离的独立接口，文档未明确**；若二者电气并联（同一总线），则本站在 `ttyS7` 上会与 intercore 形成双 master 共总线 → **该情形下 PCS 只读站不得启用**，须改由 intercore 复用其既有读路径。

#### 9.3.2 Role 新增的配置与调度影响

1. `Role` 枚举新增 `pcs`（YAML 值 `pcs`）。
2. 配置校验扩展：`pcs` 站**至多一个**（同 `battery`/`meter_grid` 的单站约束形态）；`pcs` 站 `regs` 不得为空（空 regs = 站永久 offline 的静默死配置）；`pcs` 站 `interval_ms` 无 `<5000` 硬约束（不参与控制决策），但**须 ≥ 500ms**（防误配的超短周期打满总线）。
3. 调度优先级（`role_priority`）：`pcs` 与 `meter_batt` 同级（**中**），**不得**与 `meter_grid`/`battery`（高）同档 —— 同口拥塞时优先保控制输入的采集节拍。
4. `pcs` 站**不得**接入 `on_battery_soc`，也**不得**触发 `on_grid_package`：其 3 区虽有 `BMS 系统 SOC`（1010），但那是 PCS 转述的 BMS 值，与 `battery` 站的 BMS 直采值构成**同源双写**，会破坏「BMS SOC 单源」约束（`AiIntegrator` 的 `bms_soc` 为单槽，多写方 = 最后写入者胜）。`pcs` 站全部点只走 `on_station_telemetry`。

#### 9.3.3 设备 ↔ 端口映射（硬件接线契约，v1.9 新增）

> 本节把 §9.3.1 中**分散书写**的 `port`（及 `slave`/`baud_rate`）收敛为**一张统一映射表**，并逐项给出**溯源依据**。
>
> **取值来源**：表内 `port` / `slave` / `baud_rate` **一律取自 §9.4.1 的 6 站参考配置**（本节只做汇总与溯源，**不引入任何新取值、不修改任何取值**）；若本节与 §9.4.1 出现不一致，**以 §9.4.1 为准**。取值为"现场校准/待核对"性质的，已沿用 §9.4.1/§9.3.1 的原标注，**不是**本节的结论。

| 设备 | BECG-3568 接口 | Linux 设备节点 | Role（`role`） | 站 `id` | 从站地址（`slave`） | 波特率（`baud_rate`） |
|------|----------------|----------------|----------------|---------|----------------------|------------------------|
| 两级式 PCS（只读 3 区） | **RS485-1**（`A0/B0` ↔ `ttyS0`；另有 **CAN 直连**，见 §9.2.2） | `ttyS0` **被 intercore 主链路占用**（本站**不得**复用，见下方约束）；新增只读站为 **`/dev/ttyS7`（建议值，待现场校准）** | `pcs` | `pcs` | `1`（拨码默认） | `19200`（N-8-1，无校验） |
| 储能主控模块 BMS | **RS485-2**（`A2/B2` ↔ `ttyS2`） | `/dev/ttyS2` | `battery` | `bms` | `1`（= 簇号） | `9600`（待现场核对，§9.10 Q-2） |
| 风冷空调机组 | **RS485-3**（`A3/B3` ↔ `ttyS3`） | `/dev/ttyS3` | `hvac` | `hvac` | `1` | `9600`（出厂为偶校验，§9.10 Q-14 / D-1） |
| 关口/台区总表 | **RS485-4**（`A4/B4` ↔ `ttyS4`） | `/dev/ttyS4` | `meter_grid` | **`grid_meter`**（注意：`meter_grid` 是 `role` 名，**不是**站 `id`） | `3`（现场校准） | `9600`（现场校准） |
| 储能电能表 ADL400 | **RS485-5**（`A5/B5` ↔ `ttyS5`） | `/dev/ttyS5` | `meter_batt` | `meter_batt` | `1`（现场校准，§9.10 Q-12） | `9600`（无校验，出厂默认） |
| 工商储火灾报警控制器 | **RS485-6**（`A6/B6` ↔ `ttyS6`） | `/dev/ttyS6` | `fire` | `fire` | `1`（本机地址出厂默认 1） | `9600`（8N1，出厂默认） |

**依据 1｜硬件接线拓扑**：`hw/微信图片_20260908170935_64_1061_修正.png`（**BECG-3568 接线拓扑图**；BECG-3568 即 EMS 的硬件载体）—— 该图逐口标注为 `RS485-1 → PCS`、`RS485-2 → BMS`、`RS485-3 → 空调`、`RS485-4 → 关口表`、`RS485-5 → 储能表`、`RS485-6 → 消防`，且 **PCS 与 BMS 之间另有 `CAN` 直连**（与 §9.2.2 的范围外登记一致）。**原件 `hw/微信图片_20260908170935_64_1061.png` 保留未动**，作为原始证据；其末两个串口框**笔误**标为 `RS485-4`（与关口表重复），已在**修正版**中改为 `RS485-5` / `RS485-6`，本表以**修正版**为准。

**依据 2｜规格书串口定义**：`hw/BECG-3568 BOX感知与控制主机规格书(20250818) .docx` 载明 **8 路隔离 RS485**（可选 6 路 RS485 或 2 路 RS232），接口定义逐对为 `A0/B0 → ttyS0`、`A2/B2 → ttyS2`、`A3/B3 → ttyS3`、`A4/B4 → ttyS4`、`A5/B5 → ttyS5`、`A6/B6 → ttyS6`、`A7/B7 → ttyS7`、`A8/B8 → ttyS8`（**无 `A1/B1`，故无 `ttyS1`**）⇒ 上表 6 站**各占一个物理口**，在硬件上成立、**互不共用总线**。
>
> **接口命名对照**（消除两套写法："接线拓扑图"用 `RS485-1…RS485-6`、"规格书"用 `A0/A2/…`、"PRD §9.3.1"用 `COM1…COM8`）：规格书**无 `A1` 段**，故按顺序一一对应 —— `RS485-1 = COM1 = A0/B0 = ttyS0`、`RS485-2 = COM2 = A2/B2 = ttyS2`、`RS485-3 = COM3 = A3/B3 = ttyS3`、`RS485-4 = COM4 = A4/B4 = ttyS4`、`RS485-5 = COM5 = A5/B5 = ttyS5`、`RS485-6 = COM6 = A6/B6 = ttyS6`（余 `RS485-7/8 ↔ ttyS7/ttyS8` 空闲）。本表"设备"与"接口"的对应关系即按此对照成立。

**由此得到的两条硬性推论（与 §9.3.1 既有约束一致）**：

1. **禁双 master 共总线（强制）**：`ttyS0` 已被 intercore 主链路占用 ⇒ 新增的 `pcs` 只读站**必须另占空闲口**（`ttyS7` / `ttyS8`，建议 `ttyS7`），且 `south_stations` 中任一站的 `port` **不得**与 `intercore.modbus_rtu.serial_port` 相同或互为别名（校验已在 `core_config::validate_south_stations` 实现，本轮沿用）。**空闲口：`ttyS7`、`ttyS8`。**
2. **PCS 接线前提（待确认，§9.10 Q-15）**：PCS 的 `RS485-1`（`A0/B0`）已由 intercore 用于主链路，故只读站须走 PCS 的**另一物理口**（A1/B1）—— A1/B1 与 A2/B2 是否电气隔离，文档未明确；若为同一总线，则该站**不得启用**。

### 9.4 配置契约（`south_stations` 段）

#### 9.4.1 段整体形制（6 站完整 YAML —— 同时是 AC-1 的验收输入）

段级字段保持 S3a 不变（`poll_ms` / `stale_timeout_s` / `stations[]` 的 `id`/`role`/`port`/`protocol`/`slave`/`baud_rate`/`interval_ms`/`regs`）；本轮新增**站级 `parity`**（`none` 缺省 / `even` / `odd`，同口一致性校验同 `baud_rate`）—— **§9.10 D-1 已裁定按 ①（2026-09-22，用户确认）⇒ 本字段生效**（实现已就位，见顶部 `[§9 增补 v1.11]` 登记 2）。

> 以下为 6 站的**完整配置**（点清单按 §9.4.2.1 的块规则展开；第 6 站为既有站，**本轮不动**）。行内注释给出「寄存器 ↔ 语义」对照，投运时须与 §9.5 点表逐点核对。
>
> **与生效配置的关系（AC-1 的事实基础）**：
> ① 第 6 站与生效配置 `mupc/deploy/config/mupc_core_config.yaml` 的 `grid_meter` 站**取值逐字一致** —— 站 `id: grid_meter`（**不是** `meter_grid`；`meter_grid` 是 `role` 名，二者不同）、`port`/`baud_rate`/`slave`/`interval_ms` 与 **6 个相量块**（`p`/`q`/`pf`/`u`/`i` + 可选 `p_total`，含各自 `addr`/`format`/`scale`/`count`）全部照录，**仅行内注释按本文体例改写**（注释不参与解析，不影响一致性）。原因：`mupc-southd::config::validate()` 对 `role: meter_grid` 的站**硬性要求 p/q/pf/u/i 四块齐备且 `count ≥ 6`、`addr > 0`、块区间不重叠、块名唯一、`int32_scaled` 块 `scale > 0`**（缺任一即 `Err`，启动期由 `core_config` 调用）；示例若只写 1 个 `p` 块、或把站名写成 `meter_grid`，即与生效配置/校验器冲突，AC-1 亦随之失真。
> ② 其余 5 站（`bms`/`pcs`/`meter_batt`/`fire`/`hvac`）为**新增站**：生效配置中对应行仍为**注释占位**（其中 `ac_unit`/`fire_host` 是注释文字，不构成契约），启用时以本节的站 `id` 为准。
> ③ **本轮新增的字段与取值（`parity`、`func: discrete`、`byte_swap`、`points`/`offset`/`word_order`）在实现 §9.4.3 之前不可解析**（如 `func: discrete` 会直接触发 serde 未知 variant 报错）⇒ AC-1 据此**分两级断言**（「既有字段子集」当前即可跑通；「完整 6 站 YAML」在实现后跑通），见 §9.9.1。

```yaml
south_stations:
  poll_ms: 1000            # 缺省轮询周期
  stale_timeout_s: 5       # 数据过期门限（对齐 AiIntegrator 5s）
  stations:
    # ================= 1) 储能主控模块 BMS（FC04 遥测 + FC02 告警位）=================
    - id: bms
      role: battery
      port: "/dev/ttyS2"
      protocol: modbus
      slave: 1
      baud_rate: 9600            # 现场核对（§9.10 Q-2）
      parity: none
      interval_ms: 1000          # 须 <5000（配置期强校验）
      regs:
        - name: bms_io           # 100–130；_k ↔ 寄存器 99+k
          func: input
          addr: 100
          count: 31
          format: uint16
          scale: 1.0
          points:
            - { at: 1 }                                   # 100 簇电池簇状态（枚举）
            - { at: 2,  count: 6, scale: 0.1 }             # 101–106 允许充放功率/电压/电流
            - { at: 8,  count: 8 }                        # 107–114 主控 DI1–DI8
            - { at: 16, scale: 0.1 }                      # 115 簇组电压 V
            - { at: 17, format: uint16, scale: 0.1, offset: -1600.0 }  # 116 簇组电流 A（原文 UNIT=UINT，见 §9.4.2.4）
            - { at: 18, format: uint16, scale: 1.0, offset: -40.0 }    # 117 簇组模块温度 ℃（原文 UNIT）
            - { at: 19, name: soc }                       # 118 簇组 SOC（控制输入，点名契约）
            - { at: 20 }                                  # 119 簇组 SOH %
            - { at: 21 }                                  # 120 簇组绝缘电阻 kΩ
            - { at: 22, scale: 0.001 }                    # 121 簇平均单体电压 V
            - { at: 23, format: uint16, scale: 1.0, offset: -40.0 }    # 122 簇平均单体温度 ℃（原文 UNIT）
            - { at: 24, scale: 0.001 }                    # 123 簇最高单体电压 V
            - { at: 25 }                                  # 124 最高单体电压对应点（整字，§9.10 Q-18）
            - { at: 26, scale: 0.001 }                    # 125 簇最低单体电压 V
            - { at: 27 }                                  # 126 最低单体电压对应点（整字）
            - { at: 28, format: uint16, scale: 1.0, offset: -40.0 }    # 127 簇最高单体温度 ℃（原文 UNIT）
            - { at: 29 }                                  # 128 最高单体温度对应点（整字）
            - { at: 30, format: uint16, scale: 1.0, offset: -40.0 }    # 129 簇最低单体温度 ℃（原文 UNIT）
            - { at: 31 }                                  # 130 最低单体温度对应点（整字）
        - name: bms_energy       # 139–157；_k ↔ 寄存器 138+k
          func: input
          addr: 139
          count: 19
          format: int32_scaled
          scale: 0.1
          points:
            - { at: 1 }                                   # 139–140 簇累计充电电量 kWh
            - { at: 3 }                                   # 141–142 簇累计放电电量
            - { at: 5 }                                   # 143–144 簇单次累计充电电量
            - { at: 7 }                                   # 145–146 簇单次累计放电电量
            - { at: 9 }                                   # 147–148 簇可充电量
            - { at: 11 }                                  # 149–150 簇可放电量
            - { at: 13, format: uint16, scale: 1.0 }      # 151 簇最高单体温升 ℃
            - { at: 15, format: uint16, scale: 0.001 }    # 153 簇最高单体最高电压变化 V
            - { at: 17, format: uint16, scale: 1.0, offset: -40.0 }   # 155 最高单体极柱温度 ℃（原文 UNIT）
            - { at: 19, format: uint16, scale: 1.0, offset: -40.0 }   # 157 最低单体极柱温度 ℃（原文 UNIT）
        - name: bms_meta         # 181–189；_k ↔ 寄存器 180+k
          func: input
          addr: 181
          count: 9
          format: uint16
          scale: 1.0
          points:
            - { at: 1 }                                   # 181 主控程序版本号
            - { at: 2 }                                   # 182 从控数量
            - { at: 3 }                                   # 183 簇组 SOE %
            - { at: 4 }                                   # 184 单体温度极差 ℃
            - { at: 5, scale: 0.001 }                     # 185 单体电压极差 V
            - { at: 6, format: uint16, scale: 0.1 }       # 186 簇实时充放电功率 kW（原文 UNIT；方向见 Q-3/Q-20）
            - { at: 7, scale: 0.1 }                       # 187 PACK 组压最高电压 V
            - { at: 9, scale: 0.1 }                       # 189 PACK 组压最低电压 V（188 为对应点，不采）
        - name: bms_term         # 2991–2994 端子温度 001–004；整窗口产出 4 点
          func: input
          addr: 2991
          count: 4
          format: uint16         # 原文 UNIT=UINT（§9.4.2.4）
          scale: 1.0
          offset: -40.0
        - name: bms_cap          # 4000–4005；_k ↔ 寄存器 3999+k
          func: input
          addr: 4000
          count: 6
          format: uint16
          scale: 1.0
          points:
            - { at: 1 }                                   # 4000 簇累计充电容量 Ah
            - { at: 3 }                                   # 4002 簇累计放电容量 Ah
            - { at: 5 }                                   # 4004 簇单次累计充电容量 Ah
            - { at: 6 }                                   # 4005 簇单次累计放电容量 Ah
        - name: bms_alarm        # FC02 位块 200–487；_k ↔ 位地址 199+k
          func: discrete
          addr: 200
          count: 288
    # ================= 2) 两级式 PCS（只读 3 区，FC04）=================
    - id: pcs
      role: pcs
      port: "/dev/ttyS7"           # 建议值，待现场校准（§9.10 Q-15）
      protocol: modbus
      slave: 1                     # 拨码默认
      baud_rate: 19200
      parity: none
      interval_ms: 1000            # 配置期下界 500ms
      regs:
        - name: pcs_3zone          # 1000–1075；_k ↔ 寄存器 999+k
          func: input
          addr: 1000
          count: 76
          byte_swap: true          # 「高 8 位和低 8 位互换」（§9.7.3）
          format: uint16
          scale: 1.0
          points:
            - { at: 1,  count: 6 }                                   # 1000–1005 告警1–5/BMS 工作状态
            - { at: 7,  format: int16, scale: 0.1, count: 2 }        # 1006–1007 可接受充/放电流 A
            - { at: 9,  scale: 0.1 }                                 # 1008 BMS 系统总电压 V
            - { at: 10, format: int16, scale: 0.1 }                  # 1009 BMS 系统总电流 A
            - { at: 11 }                                             # 1010 BMS 系统 SOC %（不得作控制源）
            - { at: 12, format: int16, scale: 0.1, count: 2 }        # 1011–1012 直流母线/中点电压 V
            - { at: 14, count: 5 }                                   # 1013–1017 运行/故障/降额/并离网/故障码
            - { at: 19, format: int16, scale: 0.1, count: 3 }        # 1018–1020 电网 A/B/C 相电压 V
            - { at: 22, format: int16, scale: 0.01 }                  # 1021 交流母线频率 Hz
            - { at: 23, format: int16, scale: 0.1, count: 3 }        # 1022–1024 输出电流 A/B/C A
            - { at: 26, format: int16, scale: 0.1, count: 4 }        # 1025–1028 视在功率 A/B/C/总 kVA
            - { at: 30, format: int16, scale: 0.1, count: 4 }        # 1029–1032 有功功率 A/B/C/总 kW
            - { at: 34, format: int16, scale: 0.1, count: 4 }        # 1033–1036 无功功率 A/B/C/总 kvar
            - { at: 38, format: int16, scale: 0.001, count: 4 }      # 1037–1040 A/B/C/总功率因数
            - { at: 42, format: int16, scale: 1.0 }                  # 1041 PCS 温度 ℃
            - { at: 43, format: int32_scaled, scale: 0.1, word_order: lo_hi }  # 1042–1043 交流累计充电电量
            - { at: 45, format: int32_scaled, scale: 0.1, word_order: lo_hi }  # 1044–1045 交流累计放电电量
            - { at: 47, format: int16, scale: 0.1, count: 3 }        # 1046–1048 STS 网侧电压 A/B/C V
            - { at: 50, format: int16, scale: 0.1 }                  # 1049 STS 电网电压幅值 V
            - { at: 51, format: int16, scale: 0.01 }                 # 1050 STS 电网电压频率 Hz
            - { at: 52, format: int16, scale: 0.1, count: 3 }        # 1051–1053 负载电流 A/B/C A
            - { at: 55, format: int16, scale: 0.1, count: 3 }        # 1054–1056 负载视在功率 kVA
            - { at: 58, format: int16, scale: 0.1, count: 3 }        # 1057–1059 负载有功功率 kW
            - { at: 61, format: int16, scale: 0.1, count: 3 }        # 1060–1062 负载无功功率 kvar
            - { at: 64, format: int16, scale: 0.001, count: 3 }      # 1063–1065 负载功率因数
            - { at: 67 }                                             # 1066 工作模式判断（枚举）
            - { at: 68, format: int16, scale: 0.1, count: 3 }        # 1067–1069 低压/高压总电流、低压外总压
            - { at: 71, format: int16, scale: 0.1 }                  # 1070 低压总功率 kW
            - { at: 72, format: int16, scale: 1.0 }                  # 1071 DCDC 温度 ℃
            - { at: 73, format: int32_scaled, scale: 0.1, word_order: lo_hi }  # 1072–1073 直流累计充电电量
            - { at: 75, format: int32_scaled, scale: 0.1, word_order: lo_hi }  # 1074–1075 直流累计放电电量
    # ================= 3) 储能电能表 ADL400（FC03）=================
    - id: meter_batt
      role: meter_batt
      port: "/dev/ttyS5"
      protocol: modbus
      slave: 1                     # 现场校准（§9.10 Q-12）
      baud_rate: 9600
      parity: none
      interval_ms: 1000
      regs:
        # 6 个总电能点各占 2 寄存器、彼此间隔 8 寄存器（空洞 8 > 4）⇒ 按 §9.4.2.1 第 3 条必须拆为 6 块
        - { name: mb_e_act_comb, func: holding, addr: 0x0000, count: 2, format: int32_scaled, scale: 0.01 }  # 组合有功总电能 kWh
        - { name: mb_e_act_fwd,  func: holding, addr: 0x000A, count: 2, format: int32_scaled, scale: 0.01 }  # 正向总有功电能
        - { name: mb_e_act_rev,  func: holding, addr: 0x0014, count: 2, format: int32_scaled, scale: 0.01 }  # 反向总有功电能
        - { name: mb_e_rea_comb, func: holding, addr: 0x001E, count: 2, format: int32_scaled, scale: 0.01 }  # 组合无功总电能 kvarh
        - { name: mb_e_rea_fwd,  func: holding, addr: 0x0028, count: 2, format: int32_scaled, scale: 0.01 }  # 正向总无功电能
        - { name: mb_e_rea_rev,  func: holding, addr: 0x0032, count: 2, format: int32_scaled, scale: 0.01 }  # 反向总无功电能
        - name: mb_ui            # 0x0061–0x0066；_k ↔ 0x0060+k
          func: holding
          addr: 0x0061
          count: 6
          format: uint16
          scale: 0.1
          points:
            - { at: 1, count: 3 }                        # 0x0061–0x0063 A/B/C 相电压 V
            - { at: 4, count: 3, scale: 0.01 }           # 0x0064–0x0066 A/B/C 相电流 A
        - name: mb_freq_line     # 0x0077–0x007A
          func: holding
          addr: 0x0077
          count: 4
          format: uint16
          scale: 0.1
          points:
            - { at: 1, scale: 0.01 }                     # 0x0077 频率 Hz
            - { at: 2, count: 3 }                        # 0x0078–0x007A A-B/C-B/A-C 线电压 V
        - name: mb_phase         # 0x0087–0x0094（14 寄存器；内含 0x008F–0x0091 保留空洞 3）
          func: holding
          addr: 0x0087
          count: 14
          format: int32_scaled
          scale: 0.01
          points:
            - { at: 1  }                                 # 0x0087–0x0088 A 相正向有功电能 kWh
            - { at: 3  }                                 # 0x0089–0x008A B 相正向有功电能
            - { at: 5  }                                 # 0x008B–0x008C C 相正向有功电能
            - { at: 7,  format: uint16, scale: 1.0 }     # 0x008D 电压变比 PT（只读对照）
            - { at: 8,  format: uint16, scale: 1.0 }     # 0x008E 电流变比 CT（只读对照）
            - { at: 12, format: uint16, scale: 0.01 }    # 0x0092 零序电流 A
            - { at: 13, format: uint16, scale: 0.1 }     # 0x0093 电压不平衡度 %（原文「整型 单位0.1%」→ 不得继承块级 0.01）
            - { at: 14, format: uint16, scale: 0.1 }     # 0x0094 电流不平衡度 %（同上）
        - name: mb_power         # 0x0164–0x017F（28 寄存器；_k ↔ 0x0163+k）
          func: holding
          addr: 0x0164
          count: 28
          format: int32_scaled
          scale: 0.001
          points:
            - { at: 1  }                                 # 0x0164–0x0165 A 相有功功率 kW
            - { at: 3  }                                 # 0x0166–0x0167 B 相有功功率
            - { at: 5  }                                 # 0x0168–0x0169 C 相有功功率
            - { at: 7  }                                 # 0x016A–0x016B 总有功功率
            - { at: 9  }                                 # 0x016C–0x016D A 相无功功率 kvar
            - { at: 11 }                                 # 0x016E–0x016F B 相无功功率
            - { at: 13 }                                 # 0x0170–0x0171 C 相无功功率
            - { at: 15 }                                 # 0x0172–0x0173 总无功功率
            - { at: 17 }                                 # 0x0174–0x0175 A 相视在功率 kVA
            - { at: 19 }                                 # 0x0176–0x0177 B 相视在功率
            - { at: 21 }                                 # 0x0178–0x0179 C 相视在功率
            - { at: 23 }                                 # 0x017A–0x017B 总视在功率
            - { at: 25, format: int16, scale: 0.001, count: 4 }   # 0x017C–0x017F A/B/C/总功率因数
    # ================= 4) 工商储火灾报警控制器（FC03）=================
    - id: fire
      role: fire
      port: "/dev/ttyS6"
      protocol: modbus
      slave: 1                     # 本机地址出厂默认 1
      baud_rate: 9600
      parity: none
      interval_ms: 1000            # 探测器块 n>20 时须 ≥5000（§9.5.4）
      regs:
        - name: fire_sys         # 4–16（含第 1 只探测器 11–16）；_k ↔ 寄存器 3+k
          func: holding
          addr: 4
          count: 13
          format: uint16
          scale: 1.0
          points:
            - { at: 1 }                                  # 4 系统状态（位图）
            - { at: 2 }                                  # 5 钢瓶气压 kPa（可选功能）
            - { at: 3 }                                  # 6 烟感状态（位图）
            - { at: 4 }                                  # 7 温感状态（位图）
            - { at: 5 }                                  # 8 可燃状态（位图）
            - { at: 6 }                                  # 9 火警状态（枚举）
            - { at: 7, name: fire_det_count }            # 10 复合探测器登记数量（v1.7 定名：跨文档契约点，供 §9.5.4 登记数交叉校验；不得改名）
            - { at: 8 }                                  # 11 探测器 1：地址
            - { at: 9 }                                  # 12 探测器 1：状态（位图）
            - { at: 10 }                                 # 13 探测器 1：数据 1（整字，§9.4.2.3）
            - { at: 11 }                                 # 14 探测器 1：数据 2 CO ppm
            - { at: 12 }                                 # 15 探测器 1：数据 3 VOC ppm
            - { at: 13 }                                 # 16 探测器 1：数据 4 H2 ppm
        - name: fire_det         # 探测器 2..n：17 起，6×(n−1) 寄存器（整窗口产出，整字口径）
          func: holding
          addr: 17
          count: 114             # n=20 时的取值；投运按登记数展开（每片 ≤120 寄存器 = ≤20 只）
          format: uint16         # 逐寄存器 1 点（数据 1 亦为整字，见 §9.4.2.3）
          scale: 1.0
    # ================= 5) 风冷空调机组（FC02 告警位 + FC04 温湿度）=================
    - id: hvac
      role: hvac
      port: "/dev/ttyS3"
      protocol: modbus
      slave: 1
      baud_rate: 9600
      parity: even               # 出厂为偶校验（§9.10 Q-14 / D-1 已裁定按①，2026-09-22 生效；同口须一致）
      interval_ms: 5000
      regs:
        - name: hvac_in          # FC04 30001–30004；_k ↔ 寄存器 k-1
          func: input
          addr: 0
          count: 4
          format: int16
          scale: 0.1
          points:
            - { at: 1 }                                  # 30001 柜内测量温度 ℃
            - { at: 3 }                                  # 30003 内盘管测量温度 ℃（§9.10 Q-10）
            - { at: 4, format: uint16 }                  # 30004 柜内测量湿度 %（无符号）
        - name: hvac_di          # FC02 位 0–30（PLC 10001–10031）
          func: discrete
          addr: 0
          count: 31
    # ================= 6) 关口/台区总表 grid_meter（既有站，本轮不动）=================
    - id: grid_meter              # 站 id（生效配置原值；注意 role 名才是 meter_grid）
      role: meter_grid
      port: "/dev/ttyS4"          # BECG COM4(ttyS4) ↔ 关口/台区总表；⚠️ 须与 intercore 串口不同
      baud_rate: 9600             # 同口多从站须一致（物理共享口波特率）
      protocol: modbus
      slave: 3                    # 现场校准
      interval_ms: 1000           # 须 < 5000（策略 phase 周期约束）
      regs:                       # 三相连续，Int32/Float32 均 2 寄存器/相；count=块寄存器数
        - { name: p,       addr: 0x1000, format: int32_scaled, scale: 0.01,  count: 6 }   # P_A/B/C
        - { name: q,       addr: 0x1006, format: int32_scaled, scale: 0.01,  count: 6 }   # Q_A/B/C
        - { name: pf,      addr: 0x100C, format: int32_scaled, scale: 0.001, count: 6 }   # PF_A/B/C
        - { name: u,       addr: 0x1012, format: float32,       scale: 1.0,   count: 6 }   # U_A/B/C（float32 不乘 scale）
        - { name: i,       addr: 0x1018, format: int32_scaled, scale: 0.01,  count: 6 }   # I_A/B/C
        - { name: p_total, addr: 0x101E, format: int32_scaled, scale: 0.01,  count: 2 }   # 可选：缺省降级 Σp
```

#### 9.4.2 `regs` 块写法

本轮把配置切成两层，这是解决 v1.2「落地机制矛盾」的核心裁定（**B-1 的答案：逐点语义一律放配置，代码只提供通用解码原语**）：

- **块 = 一次 Modbus 读事务**，只描述**传输口径**（`func`/`addr`/`count`/`byte_swap`）与**缺省换算**；
- **点 = 一个物理量的换算口径**（`format`/`scale`/`offset`/`word_order`/`name`），在块的 `points` 列表里**逐点声明**（可继承块级缺省）。

> 依据：一次总线事务只能读一段连续地址（物理事实，**不可逐点变化**）；而厂方点表里同一段连续地址内混排多种换算（`0.1V` 与 `0.01A`、`raw − 40℃` 与纯计数）是点表事实。把换算口径绑到块上，会强迫把一段物理上连续的读拆成十几个事务（BMS 100–130 段需 17 次），既不符设备语义又顶破带宽预算。

##### 9.4.2.1 块落地规则（唯一、可机械执行）

1. **传输口径相同的点才能同块**：`func` / `byte_swap` 不同 → **必须拆块**。
2. **地址必须连续**：块覆盖 `[addr, addr+count−1]` 的连续区间，中间不得跳读（Modbus 无跳读语义）→ 地址不连续 → **必须拆块**。
3. **空洞上限 4 寄存器（仅适用于声明了 `points` 的块）**：块内**未声明点**的寄存器**连续空洞 ≤ 4**；超过 → **必须在该空洞处拆块**。（未声明 `points` 的块"窗口内每值槽都是点"，不存在空洞。）
   - 4 的来源：**空读的字节数不得超过一次请求帧的字节数** —— 请求帧固定 8 字节 ⇒ 补读 `N` 寄存器有 `2N ≤ 8` ⇒ `N ≤ 4`。即"补读量比一次请求还贵就拆块"。
   - 窗口**首尾以声明点为准**（首/尾的未声明寄存器不得含入窗口；块内末尾的未声明寄存器不计入 `count`）。
   - `discrete` 位块不受此限（位块整窗口产出，见第 4 条）。
4. **`points` 的两种形态**：
   - **给出 `points`** = **完整点清单**：只有列出的点产出遥测；未列出的寄存器**不产出点**（用于排除"保留/对应点"寄存器，如 BMS 188/190）。
   - **不给 `points`** = 窗口内**每个值槽产出 1 点**（16 位格式：每寄存器 1 点；32 位格式：每 2 寄存器 1 点；`discrete`：每位 1 点）。
5. **极大性（保证解唯一）**：块 = 满足 1–4 的**极大**区间 —— 从最小地址的声明点起向后扩张，遇 1/2/3 任一即截断并开新块。故"点表 → 块"的映射在给定点表下**唯一**。（**v1.8 限定适用域，同第 3 条**：本条**仅适用于声明了 `points` 的块** —— 未声明 `points` 的整窗口块（既有 `meter_grid` 形态的按块名查找块、`discrete` 位块等）**不并入、不拆分**，其窗口边界由设备点表/既有契约给定；配置期对该域的强制执行条件见 §9.4.3 第 15 条，判据为"合并后 `count ≤ 120` 寄存器"。）
6. 站内（按 `func` 空间分别计算）块区间**不得重叠**；窗口内允许存在未声明的寄存器（空读），但须在配置注释中登记原因（保留区/对应点等）。

##### 9.4.2.2 点位命名规则（唯一）

`metric = 点的 name`（若该点显式声明 `name`）**否则** `metric = <块名>_<序号>`，其中 `序号` 为：

| 点的形态 | `序号` 取值 |
|----------|-------------|
| 16 位点（`uint16`/`int16`） | 所在寄存器的**块内偏移 + 1**（即 `序号 = 寄存器地址 − 块 addr + 1`） |
| 32 位点（`float32`/`int32_scaled`） | 其**低地址寄存器**的块内偏移 + 1 |
| `discrete` 位点 | 该位的块内偏移 + 1（即 `序号 = 位地址 − 块 addr + 1`） |

- **绝对位地址命名（如 `bms_alarm_424`）自 v1.3 起废止**；位地址与点名的对照由 §9.5 点表给出（`bms_alarm_225` = 位地址 424 = 簇一级告警）。
- 显式 `name` **仅用于跨文档契约点**（如 `soc`），站内唯一（重复 → 配置期拒）。
- **改名 = 破坏历史数据可比性**：点投运后不得改名；`addr`/`points` 顺序变更即等于改名，须按变更流程登记与评审（这就是第 9.4.2.1 条把 `序号` 锚定在"地址偏移"而非"列表顺序"的原因）。

##### 9.4.2.3 G-6（寄存器内字节拆分）本轮不做

配置契约**不含** `slice` 字段。理由与替代口径见 §9.1.1 的 G-6 说明：两个候选点（BMS 位置编号 124/126/128/130、消防探测器数据 1）分别因"原文位定义自相矛盾"与"探测器区重复结构导致配置/带宽爆炸"而不宜在本轮拆点；两者均**整字采集原值**（§9.5.1 表中标注"整字"、§9.5.4 数据 1 注明字节语义），字节拆解由展示/事件层完成，**原始值保留可回溯，无静默失真**。重启条件见 §9.10 D-2。

##### 9.4.2.4 字段表

**块级字段**

| 字段 | 取值 | 语义与约束 |
|------|------|-----------|
| `name` | 字符串 | 块名，**站内唯一**（重复 → 配置期拒）；用于**点位名前缀**与排障定位。**查找键口径（不得泛化）**：① **本轮新增的逐点消费一律按「点名」查找**（`soc` 契约见 §9.4.3）；② **例外 —— `grid_meter` 站（既有 `Role::MeterGrid` 路径）的相量仍按「块名」查找** `p`/`q`/`pf`/`u`/`i`/`p_total`（既有 mapper 实现，本轮不动），故这 6 个块名**仍是契约的一部分**，改名即改变行为（这也正是 §9.4.1 第 6 站必须照录生效配置的原因之一） |
| `func` | `holding`（缺省 FC03）/ `input`（FC04）/ **`discrete`（FC02，本轮新增）** | 决定功能码；`discrete` 时 `count` 语义为**位数**（非寄存器数）、`addr` 为**位地址** |
| `addr` | u16 | 起始寄存器地址（**0 基**）；`discrete` 为位地址。取厂方点表「起始地址」列，**禁止**填 PLC 地址（10001/30001/40001 形态） |
| `count` | u16（**非必填**，缺省 **2**；**v1.8 订正** —— 原文"必填，>0；S3a 既有块缺省 2"自相矛盾且与代码事实不符。代码事实：`mupc-southd::config::RegBlockConf` 的 `#[serde(default = "default_reg_count")]`，`default_reg_count()` 返回 **2** ⇒ **漏写不报错**，按 2 读取） | `holding`/`input`：**窗口寄存器数**；`discrete`：**位数**（不要求 8 的倍数，解包公式见 §9.7.4）。**显式取值须 > 0**（本轮新增块的取值要求，沿用原表述；serde 层不拦截 `count = 0`） |
| `byte_swap`（**新增**） | bool（缺省 false） | true = 逐寄存器**字节低-高互换**（PCS 专用，§9.7.3） |
| `format` / `scale` / `offset` | 同下列点级字段 | **块级缺省换算**，点级无声明时继承。**`format` 的块级缺省 = `float32`**（**v1.7 订正**；代码事实 = `mupc-southd::config::default_reg_format()` 返回 `RegFormat::Float32`，点级 `format` 无独立缺省）⇒ **本轮新增块一律显式声明 `format`**，见点级字段表 `format` 行与 §9.4.3 末的护栏。**`discrete` 块不适用**：位值恒为 0/1（等价 `scale = 1.0`、`offset = 0.0`），声明即为无意义配置 —— 故 §9.4.3 的 "`scale == 0` → 拒" 一条**不适用于 `discrete` 块**（v1.5 起已无"`uint16` + 非零 `offset` → 拒"这一条，见 §9.4.3） |
| `points` | 列表（可选） | 见 §9.4.2.1 第 4 条 |
| `read_slice`（**新增，v1.7 补登**） | bool（缺省 `false`） | **现场分片豁免标记**（**块级**）。`true` = "本块是**按设备单次读寄存器上限主动分片**得到的块"，**豁免 §9.4.3 第 15 条「块落地极大性」的拒绝**。**适用场景**：仅当"两块合并后的寄存器数**超过该设备单次读上限、必须分片读取**"时置 `true` —— 如 PCS 3 区（§9.5.2，建议 38+38）、消防探测器区（§9.5.4，每片 ≤120 寄存器）；而该上限**文档未明确、须现场实测**（§9.10 **Q-6/Q-7**），配置期无法自动判定 ⇒ 只能由配置者**显式声明**（**不得**由代码按 `role`/块名推断，见下）。**约束（不得用于逃避合并的边界）**：① 若两块合并后**仍可被设备一次读回**（即合并后寄存器数不超过设备单次读上限），则**必须合并**，**不得**标 `read_slice`；② 本字段**只**豁免第 15 条，**不豁免** §9.4.3 的其它各条（区间重叠、空洞上限、点位越界/重叠、点名唯一等一律照旧）；③ 豁免是**块级、逐块**判定 —— 同站某块标 `true` **不**使其它块获得豁免，**更不得**整站/按 `role` 批量套用；④ 每个置 `true` 的块**必须在配置注释中登记分流理由与依据**（现场实测上限值或厂方文档条目），该注释为 **RC-1/RC-5 的目视核对项**，未登记理由的 `read_slice` 视为**投运前待办项**。**为什么不用"按 `role` 特判"替代**：按 `role`（`pcs`/`fire`）决定是否豁免极大性属于**设备特判**，违反 §9.1.1 **G-5**"**禁止按 role 隐式特判** —— 换设备时会静默错读"；而"设备单次读上限"本就是**配置事实**（现场实测值），交给配置显式表达后，代码侧只保留一条与设备无关的通用判据 |

**点级字段**（`points` 列表项，仅当块声明了 `points`）

| 字段 | 取值 | 语义与约束 |
|------|------|-----------|
| `at` | u16（必填） | 块内**寄存器/位偏移 + 1**（1 起）；32 位点填其**低地址寄存器**的序号 |
| `count`（**新增**） | u16（缺省 1） | 自 `at` 起连续产出 `count` 个点（同换算、地址递增）；**32 位点必须 `count = 1`** |
| `name`（**新增**） | 字符串（可选） | 显式点名，站内唯一；用于跨文档契约点（如 `soc`） |
| `format` | `float32` / `int32_scaled` / **`uint16`（新增）** / **`int16`（新增）** | `float32`/`int32_scaled` 占 **2 寄存器/值**；`uint16`/`int16` 占 **1 寄存器/值**。⚠️ **缺省是 `float32`**（**v1.7 订正** —— v1.6 及以前误写为 `int32_scaled`；代码事实 = `mupc-southd::config::default_reg_format()` 返回 `RegFormat::Float32`）；**点级 `format` 无独立缺省，未声明时继承块级** ⇒ **本轮新增块一律显式声明 `format`，不得依赖缺省**：缺省是 **2 寄存器/值**语义，16 位块漏写会**静默按"2 寄存器 1 值"解错值且少产点**（护栏见 §9.4.3 末的"`format` 显式声明护栏"） |
| `scale` | f64（缺省 0.0） | 换算系数；**`int32_scaled`/`int16`/`uint16` 须显式 `scale ≠ 0`**（漏写 → 解 0 静默失真，沿用既有拦截） |
| `offset`（**新增**） | f64（缺省 0.0） | 换算偏移：`值 = raw × scale + offset` |
| `word_order`（**新增**） | `hi_lo`（缺省，高字在低地址）/ `lo_hi` | 32 位值的字序；PCS 32 位电量置 `lo_hi` |

> **块级字段"必填 / 缺省"核对（v1.8，逐字段回 `mupc-southd::config::RegBlockConf` 源码）**：`name` / `addr` **无 `#[serde(default)]` ⇒ 必填（漏写即解析报错）** —— 表内未声明缺省值，**不构成表述错误**；`func` 缺省 `holding`（`#[serde(default = "default_reg_func")]`）、`format` 缺省 `float32`（`#[serde(default = "default_reg_format")]`）、`scale` 缺省 `0.0`（`#[serde(default)]`）、`count` 缺省 **2**（`#[serde(default = "default_reg_count")]`）—— **均为非必填**，与表内表述一致。**本次核对仅发现 `count` 一处事实性错误（v1.8 已订正）**。`byte_swap` / `points` / `read_slice` / 块级 `offset` 属**本轮新增字段**（§9.4.1 ③ 已声明），代码中尚不存在，不参与此次核对。
>
> **换算与符号性（v1.5 重裁定，纠正 v1.3/v1.4 的错误）**
>
> **结论：`offset` 与符号性无关；符号性一律以厂方类型标注为准。**
>
> ① **偏移量的含义**：`offset` 只表示「**零点平移**」—— 换算 `值 = raw × scale + offset`。它与 raw 被如何解释为整数（`uint16` / `int16`）是两个独立的自由度，**任取组合都合法**。
> ② **「无符号 + 负偏移」是常见做法，不是有符号的证据**：同一份 BMS 文档中，**保持寄存器表（§5.2）用 `UINT` 正确拼写、且大量携带负偏移**（如「`UINT` … 精度：0.1 偏移：-50℃」、「`UINT` … 精度：0.1 偏移量：-3000A」）——厂方自己的主力做法就是「无符号编码 + 负偏移做零点平移」。由此可证：**负偏移不能推出有符号**；`uint16` 下 raw=0 对应最低物理量，即 116 的 raw=0 → −1600.0 A，这正是「无符号零点平移」的标准形态。
> ③ **v1.3/v1.4 的错误**：把「原文标 `UNIT` 且带负偏移」的 8 个 BMS 量改判为 `int16`（BMS 116/117/122/127/129/155/157/186、2991–2994），并把 raw=65535 → −1600.1 A 当作「必须 `int16`」的证据 —— 这是**循环论证**（先假设 `int16`，再拿它的解码结果当证据）。raw=65535 恰好是 `uint16`/`int16` 的**判别点**：`uint16` 得 **+4953.5 A**，`int16` 得 **−1600.1 A**。判别点本身不提供答案，**答案在厂方类型标注里**（116 原文标 `UNIT`，即 `UINT`，无符号）。
> ④ **BMS 侧的实际标注**：文档 §3.1.1「数据类型」表只定义 `UINT`（16bit 无符号 0~65535）与 `INT`（16bit 有符号），**没有 `UNIT` 这个类型**；而输入寄存器表（§5.3，含 116/117/122/127/129/155/157/186/2991–2994）的类型列**共 110 处写作 `UNIT` 族拼写 —— 整词 `UNIT` 104 处 + `UNIT32` 6 处**（`UNIT32` 是同一笔误在 32 位量上的写法），保持寄存器表（§5.2）则**一律用正确拼写 `UINT`（整词 164 处）** —— 判定 §5.3 的 `UNIT` 族为 `UINT`/`UINT32` 的拼写笔误（**推断，非厂方明文**，已登记为 §9.10 **Q-20**）。文档中 `INT` 在**点表里从未出现**（全文档仅 1 次，即 §3.1.1 类型表自身）⇒ **BMS 本轮全部 16 位量按 `uint16` 声明**。
> ⑤ **PCS / 空调的标注是厂方逐点明写**（`Int16`/`UInt16`、`16位有符号`/`16位无符号`），**不是**本 PRD 的推断；其符号性照抄原文（§9.5.2 / §9.5.5）。ADL400 与消防的取值来源不同（部分有、部分无类型标注），逐份登记在 §9.5.3 / §9.5.4 / §9.7.5。
> ⑥ **配置校验的相应调整**：v1.2 的「拒 `uint16` + 负 `offset`」与 v1.3/v1.4 的「拒 `uint16` + 非零 `offset`」**两条都删除** —— 前者会误拒主力点，后者同样会误拒（116/117/122/127/129/155/157/186/2991–2994 全是被误拒对象）。替换为「**符号性声明与厂方标注一致性**」的登记式校验，见 §9.4.3。
> ⑦ **残余风险与判别方法**：若现场实测发现某点在小值工况被解成大正值（例如充放电电流在放电小电流时逼近 65535），说明厂方实为有符号 ⇒ 该点须改声明 `int16` 并按变更流程登记（`format` 变更**不改变点名**——点名锚定地址而非 format，故不构成改名）。判据与登记见 §9.10 **Q-20**。

#### 9.4.3 配置期校验扩展（本轮必须新增的拒绝条件）

| 校验 | 拒绝条件 | 防的事故 |
|------|----------|----------|
| 新 role 合法性 | `role: pcs` 解析通过；未知名被拒 | 静默 fallback |
| 单站约束 | `pcs` 站 > 1 个 → 拒 | 同设备双站双读、点表冲突 |
| 必填点表 | `pcs` 站 `regs` 为空 → 拒 | 站永久 offline 的静默死配 |
| **`soc` 点契约**（v1.3 修订） | `battery` 站**没有任何点位名为 `soc`** → **拒**（v1.2 的"块名为 `soc`"随换算口径下沉到点而同步修订；对齐 `meter_grid` 缺相量块的既有拦截力度） | SOC 控制链路静默断供（消费方按**点名**查找，缺名即静默不推 SOC） |
| 格式与标度 | `int32_scaled`/`int16`/`uint16` 的点/块 `scale == 0.0` → 拒 | 解 0 静默喂控制链 |
| **符号性声明一致性（v1.5 新规则，替换 v1.2/v1.3 两条已废规则）** | ① `format` ∈ {`uint16`,`int16`} 的点/块，若其 **`offset ≠ 0` 而该点未在 §9.5 点表中标注符号性来源** → 拒（防"随手写死一个偏移、事后无人知道 raw 该怎么解释"）—— **⚠️ 本条本轮不可机械校验，不纳入 AC-1**（见下"可执行性说明"）；<br>② `format` ∈ {`uint16`,`int16`} 的点/块，其 **`offset` 与 §9.5 点表登记值不一致** → 拒（防配置与点表漂移）；<br>③ `scale == 0.0` 仍照上一条"格式与标度"拒绝（符号性规则**不再触碰 `scale`**）。<br>**可执行性说明（v1.6 修订）**：<br>• 第 ② 条**可机械校验** —— 其期望值来自**校验器内的「点表登记值」常量表**（`(role, addr) → {format, scale, offset, 来源}`，**由设计阶段按 §9.5 逐点转写**），比对配置与该常量表；⚠️ 该校验的可判定性**以设计落成该常量表为前提**（常量表不存在则 ② 与 ① 同样"无源可判"）。<br>• 第 ① 条**本轮不可机械校验**：配置层**尚无**"来源标注"字段，且校验器看不到 §9.5 文档，**无法判定"是否登记来源"** ⇒ **① 不纳入 AC-1**（v1.6 已把该条从 AC-1 ③ 的触发清单中**删除**），其核验**归属投运前 RC-1 逐点比对 + §9.5 表核对**（投运前项）；待设计给出**点级来源字段**（可解析的声明形式）后，再以"未声明来源而 `offset ≠ 0` → 拒"的形态**纳入 AC** | ① 真正的事故是"**raw 的解释方式无人可考**"，不是"`uint16` 不能配偏移"；② 本条把"负偏移 ⇒ 必须有符号"这一**错误推论**替换为"**符号性必须可追溯到厂方标注或登记为工程判断**"（§9.5.x 的"类型来源"栏 + §9.10 Q-20）；③ **`uint16` / `int16` 与任意 `offset` 的组合一律放行** —— BMS 116/117/122/127/129/155/157/186/2991–2994 与全部温度类点表可正常通过 |
| 点位越界 | 点的寄存器/位区间超出 `[addr, addr+count−1]` → 拒 | 读到块外寄存器、点位与读数不符 |
| 点位重叠 | 同一块内两点覆盖同一寄存器（或同一位）→ 拒 | 同地址双写、后写覆盖前写 |
| 32 位点对齐 | 32 位点区间跨越窗口末尾（`at + 1 > count`）→ 拒 | 半个 32 位值 |
| 点名唯一 | 站内点名重复（含自动点名与显式 `name` 相撞）→ 拒 | 遥测键冲突、指标相互覆盖 |
| 空洞上限 | 块内未声明寄存器的连续空洞 > **4** → 拒（§9.4.2.1 第 3 条） | 宽窗口空读挤占总线（须拆成多个块） |
| 位块上限 | `discrete` 块 `count` 超出设备/协议上限（BMS ≤ 2000 位）→ 拒 | 设备返回异常码、整块失败 |
| 地址有效性 | `addr == 0` 是否合法**按设备分别判定**：ADL400 电能块 `addr=0` 合法（点表首址即 0 且 §9.8 示例佐证）；空调 FC04/FC02 首址亦为 0；其余设备沿用 `addr > 0` | 误拒/误放 |
| 区间与重叠 | 同一站内块区间（按功能码空间分别计算）不得重叠 | 读到错寄存器 |
| **块落地极大性**（**v1.8 订正适用域与判据**） | **适用域（限定）**：本条**仅作用于「声明了 `points` 点级清单的块」**。**未声明 `points` 的块**（整窗口块 —— 含既有 `meter_grid` 形态的按块名查找块，如 §9.4.1 第 6 站 `grid_meter` 的 6 个相量块；亦含 `discrete` 位块）**不参与本条的合并判定**：无论其与邻块地址是否连续、也无论合并后 `count` 为多少，**一律不拒**（理由：这类块的窗口边界由设备点表/既有契约给定，"碎/合"属既有行为，本条不得改变既有站行为）。<br>**拒绝条件**：在**同一 `func` 空间**内，存在两个**均声明了 `points`** 的块，**同时**满足 ① `func` 与 `byte_swap` 相同；② **地址连续**（`addr₂ = addr₁ + count₁`，`discrete` 按位地址同理）；③ 合并窗口内未声明寄存器的**连续空洞 ≤ 4**（即 §9.4.2.1 第 3 条，保留，理由见下；`discrete` 位块不受 ③ 限制，同第 3 条）；④ **合并后 `count ≤ 120` 寄存器** ⇒ **拒**（提示合并）。<br>**③ 为何保留（v1.8 说明，防"订正反而引入新误拒"）**：若删去 ③ 而只看 ④，则"两块各自合法、但合起来会在 `count ≤ 120` 的窗口内连成 > 4 的连续空洞"这一**被 §9.4.2.1 第 3 条强制拆开**的合法配置，会被本条**反向误拒** —— 本条必须与第 3 条同向。故 v1.8 的实际变化是**新增 ④（`count ≤ 120`）作为"设备单次读"判据**，而非取消 ③。<br>**判据 ④ 的 120 来源**：Modbus **单次读的保守上限**（一次读事务可覆盖的寄存器数；`discrete` 位块按等价寄存器数折算）。**合并后仍可一次读回者必须合并**；合并后超过 120 者**本就必须分片**，不构成本条的拒绝对象。<br>**豁免（v1.7 补登）**：相邻块中**任一块**声明 **`read_slice: true`** → **不拒**（现场按设备单次读上限分片所致；本字段的定义、适用场景、"不得用于逃避合并"的边界与"为什么不用按 `role` 特判"见 §9.4.2.4 块级字段表）—— 即"合并后 `count ≤ 120`、但**设备实际单次读上限更小**"的合法分片，靠 `read_slice: true` 显式豁免。**`read_slice` 只在本条适用域内（块声明了 `points`）才有对象** —— 未声明 `points` 的块本就不参与判定，对其标 `read_slice` **不产生豁免效果**（不构成错误配置，但属冗余，配置评审时不计入分流理由）。<br>**自检结论（v1.8）**：**§9.4.1 的 6 站参考配置必然通过本条**（含既有 `grid_meter` 形态），依据见紧随本表后的"第 15 条适用域自检" | 人为碎块导致事务数膨胀、单轮超预算 |
| 同口一致性 | 同 `port` 各站的 `baud_rate` **与 `parity`** 必须一致 | 异波特/异校验被静默忽略 |
| 跨段互斥 | `port` 与 `intercore.modbus_rtu.serial_port` 同节点 → 拒（**既有实现，沿用**） | 双 master 共总线 |

> **本表不做的校验（防误拒，明确声明）**：① 不校验"点表数值是否落在物理量程内"（除 §9.6.3 的 SOC 域检查外，其余量纲各异，自动量程校验的误判风险高于收益）；② 不校验"配置点位与厂方点表的一致性"（无法机械判定，由投运前逐点比对 RC-1 保证）；③ 不再要求 `discrete` 块 8 位对齐（v1.2 的该条与 §9.5.5A 的 31 位点表**自相矛盾**：31 不是 8 的倍数）—— 解包按位地址直接索引，与是否对齐无关（§9.7.4）；④ **不校验"符号性对不对"**（校验器无法知道厂方真意）—— 只校验"符号性**可追溯**"（上表第 1 条）与"与点表**一致**"（第 2 条）；符号性**本身**的正确性靠 §9.5 的"类型来源"栏、Q-20 的现场判别与 RC-1 逐点比对保证。
>
> **`format` 显式声明护栏（v1.7 补登，非拒绝条件、不纳入 AC-1 ③）**：`format` 的**块级缺省是 `float32`**（代码事实，见 §9.4.2.4）—— 即"**2 寄存器/值**"。因此**本轮新增的每一个块（含未声明 `points` 的整窗口块）一律显式声明 `format`**，**不得依赖缺省**。理由：16 位块（`uint16`/`int16`）漏写 `format` 会被缺省当成 `float32` **静默按"2 寄存器 1 值"解码**（**既解错值又少产点**）；若该块同时还写了 `scale`/`offset`，失真进一步放大（例如漏写的 16 位温度块会被解成浮点）。**本条为"文档约束 + 加载期告警"（日志提示"块 X 未声明 `format`，按缺省 `float32` 处理"），不做配置期拒绝** —— 缺省值是既有代码契约（`default_reg_format()`），既有站与既有用例依赖之，硬拒绝会改变既有行为、并误伤"无换算的填充块"。故该条**不是 §9.4.3 的拒绝条件、不纳入 AC-1 ③**，其落实由 §9.4.1 参考配置（逐块显式声明）+ **RC-1 逐点比对**保证。
>
> **第 15 条适用域自检（v1.8，逐个相邻块对核过 §9.4.1 的 6 站参考配置）**：
> ① **第 6 站 `grid_meter`（问题源）** —— 6 个相量块 `p`(0x1000,c6) / `q`(0x1006,c6) / `pf`(0x100C,c6) / `u`(0x1012,c6) / `i`(0x1018,c6) / `p_total`(0x101E,c2) 地址**严格相邻、零空洞**（合并后 `count = 32 ≤ 120`，若只看 ④ 必被拒），但**六块均未声明 `points`** ⇒ 按适用域**整体出局、不拒**。这正是本条与 §9.4.1 ①「照录生效配置」「既有不动」的相容点。
> ② **唯一"地址连续的相邻块对"在 fire 站** —— `fire_sys`（`addr: 4`、`count: 13`、覆盖 4–16、**有 `points`**）紧接 `fire_det`（`addr: 17`、**无 `points`**）；后者**不参与判定** ⇒ 全配置**不存在"两块均声明 `points` 的邻块对"**，本条**无触发对象**。
> ③ **其余各站的相邻块地址均不连续**（不满足 ②，无论 `points` 与 `count` 如何）：BMS —— `bms_io` 100–130 │ 空洞 8 │ `bms_energy` 139–157 │ 空洞 23 │ `bms_meta` 181–189 │ 空洞 2801 │ `bms_term` 2991–2994（无 `points`）│ 空洞 1005 │ `bms_cap` 4000–4005；`meter_batt` —— `mb_ui` 0x0061–0x0066 │ 空洞 16 │ `mb_freq_line` 0x0077–0x007A │ 空洞 12 │ `mb_phase` 0x0087–0x0094 │ 空洞 207 │ `mb_power` 0x0164–0x017F（6 个电能单点块 `0x0000/0x000A/…/0x0032` 既**未声明 `points`**、彼此亦有 8 寄存器间隔，双重出局）；`hvac` —— `hvac_in`（`input`）与 `hvac_di`（`discrete`，无 `points`）**跨 `func` 空间**，不构成邻块对；`pcs` —— 仅 `pcs_3zone` 1 块，无邻块；`bms_alarm`（`discrete`、无 `points`）为 BMS 站唯一 `discrete` 块。
> ④ **结论**：§9.4.1 的 6 站参考配置在**不删任何块、不加任何 `points`、不标任何 `read_slice`** 的情况下，**必然通过第 15 条**；AC-1 ② 的"完整 6 站 YAML"因此**不再被本条否决**。（注：`read_slice` 在本参考配置中**一处都不需要** —— 其对象是"两块均声明 `points` 且合并后 `count ≤ 120`、但设备单次读上限更小"的分片，如 §9.5.2 的 PCS 3 区分片建议，本 6 站配置中无此形态。）

### 9.5 逐设备语义点表（核心交付物）

> 通用约定：`addr` 均为**厂方文档「起始地址」列的原值（0 基）**；`RW` 列 `R` = 本轮采集，`W/RW` = 写侧（§9.2.3）。**文档未明确**之处一律在 §9.10 登记，本文不填猜测值。
>
> **⚠️ `format`（符号性）的来源口径（v1.5 新增，逐设备必须区分，不得混同）**：本 PRD 的每个 16 位点的 `format` 有**三种来源**（其中 **ADL400 为混合来源**，同一设备内跨第 1、第 3 类，见表末单列行），各设备的下表与 §9.7.5 均显式标注：
>
> | 来源 | 含义 | 适用设备 | 误差风险 |
> |------|------|----------|----------|
> | **厂方逐点明写** | 文档类型列逐点给 `Int16`/`UInt16`（或 `16位有符号`/`16位无符号`），PRD **逐字照抄** | **PCS**（`Int16`/`UInt16`）、**空调**（`16位有符号`/`16位无符号`）、**ADL400 的 4 字节功率类与 PF**（原文备注「**有符号整形**」） | 最低 —— 与原文逐字一致，可机械核对 |
> | **厂方标注 + 推断订正** | 原文标 `UNIT` 而类型表只定义 `UINT` ⇒ 判定为笔误并按 `uint16` 落地（**推断**，已登记 Q-20） | **BMS**（§5.3 输入寄存器表：`UNIT` 族拼写 110 处 = 整词 `UNIT` 104 + `UNIT32` 6） | 中 —— 若笔误判断有误（实际为 `INT`）则大 raw 值处解错 |
> | **工程判断（文档无类型）** | 文档**不给任何类型**，仅给单位/精度/取值范围；按「取值范围非负 ⇒ `uint16`；需负值 ⇒ `int16`」判断并**显式登记** | **消防**（无类型列）、**ADL400 的 2 字节点**（电压/电流/频率/线电压/零序/PT/CT，无类型字样） | 中 —— 判断依据须逐条写明（见 §9.5.3 / §9.5.4） |
> | **混合（同一设备内跨第 1 类与第 3 类）** | 部分点有厂方明写的类型、部分点无任何类型字样 ⇒ 必须**逐点区分**，**不得整体归入任一类** | **ADL400**（口径见 §9.5.3 / §9.7.5） | 中 —— 每点来源已逐条登记 |
>
> 三种来源**不得互相美化**：工程判断**不得**写成"厂方标注"（v1.3/v1.4 的 BMS 改判即因未区分来源而出错）；**混合来源的设备**（ADL400）须**逐点**标注来源，**不得**笼统写成"厂方标注"或"工程判断"。

#### 9.5.1 储能主控模块 BMS（`Role::Battery`）

**协议事实**：华塑科技《储能系统主控模块对 EMS 系统通信协议》B0 版（2025-10-15）。§1.1 明确「主控模块与 EMS 系统间通过 **485-1 或 LAN** 通信协议」；§2.2 明确「通信模式为 **Modbus-TCP / Modbus-RTU**，**EMS 系统做通信主机，大储主控模块做通信从机**」。因此配置 `port: /dev/ttyS2` + `protocol: modbus`(RTU) + `slave: 1` **与文档 §2.2 一致且合法**，直接用现有串口轮询，**不需要为 southd 新增 TCP 传输**。
>
> ⚠️ **该口径是 §9.10 Q-1 的默认基线，而 Q-1 是【设计阶段阻塞】项**：厂方文档 §3.1 另有两处相反表述（「主控模块做主机」/「主控模块做 TCP 服务器」）。**设计开工前必须取得书面确认，或在设计中给出两套可切换方案**；若确认为相反口径，本节的点表映射与 §9.6.1 的 SOC 消费链须**重新走需求评审**（详见 §9.10 Q-1 的"影响面/约束/解除条件"）。
>
> ✅ **（v1.9，2026-09-22 更新）本项已结清、上述阻塞已解除**：用户于 2026-09-22 确认 **EMS 是通信主机、BMS 是通信从机**。两处相反表述已定性 —— §3.1 的 **TCP 段**（「主控模块做 TCP 服务器…后台监控主动连接主控模块」）与 §2.2 **互相印证**（TCP 服务器/客户端是传输层角色，Modbus 主机/从机是应用层角色，Modbus-TCP 惯例为**客户端 = 主机**），只有 **§3.1 的 RTU 段**（「主控模块做主机，EMS 做从机」）为**笔误**。故**本节口径（`port: /dev/ttyS2` + RTU + `slave: 1`）与 §9.6.1 的 SOC 消费链维持不变、无需重新评审**。完整依据链（含"Modbus 只有主机能主动读、无推送机制"的工程判据）见 §9.10 **Q-1 行 ⑤**。

- **数据序（§3.2）**：16 位数据**高字节在前**；32 位数据**高 16 位字在前**（低地址存高 16 位）——与 `RegFormat` 既有大端语义**一致，无需 `byte_swap`**。
- **功能码**：`03H` 读保持寄存器（单次 ≤ 120 寄存器）、`04H` 读输入寄存器（≤ 120）、`02H` 读离散输入（≤ 2000 位）。
- **从站地址 = 簇号**（召唤第 1 簇 = 地址 1）。本轮只接 1 簇（`slave: 1`）；多簇扩展 = 同口多从站各配一站（设计 §10.3 扩展点）。

**A. 输入寄存器（FC04）——遥测主表**（`func: input`）

> 本表逐点给换算口径；**块归属与点位名见 E 表**（`bms_io_k ↔ 寄存器 99+k`、`bms_energy_k ↔ 138+k`、`bms_meta_k ↔ 180+k`、`bms_term_k ↔ 2990+k`、`bms_cap_k ↔ 3999+k`），例外为 118（`name: soc`）。
>
> **本表 `format` 的来源（BMS 专项，v1.5）**：**全部 16 位点为 `uint16`**，依据是厂方文档 §3.1.1「数据类型」表只定义 `UINT`（16bit 无符号）与 `INT`（16bit 有符号）、**无 `UNIT`**，而 §5.3 输入寄存器表的类型列**共 110 处写作 `UNIT` 族拼写 —— 整词 `UNIT` 104 处 + `UNIT32` 6 处**（`UNIT32` 是同一笔误在 32 位量上的写法）⇒ 判定为 `UINT`/`UINT32` 的拼写笔误；且 `INT` 在**整个点表中从未出现**（全文档仅 1 次，即类型表自身）⇒ **BMS 侧不存在任何"厂方明写有符号"的 16 位点**。**这是推断而非厂方明文**，完整依据、反证（同一文档 §5.2 保持寄存器表用**正确拼写 `UINT`，整词 164 处**，且同样带负偏移）与现场判别方法见 §9.4.2.4 与 §9.10 **Q-20**。32 位电量点（139–150）原文标 `UNIT32`（同族笔误，即 `UINT32`），本 PRD 用 `int32_scaled` 表达并已在表内注明理由（量程上限远超实际）。

| # | addr | 名称（文档） | 格式 | scale | offset | count | 单位 | 属性 | 含义/备注 |
|---|------|--------------|------|-------|--------|-------|------|------|-----------|
| 1 | 100 | 簇电池簇状态 | uint16 | 1.0 | — | 1 | — | R | 枚举：0x00 初始 / 0x01 充电 / 0x02 放电 / 0x03 待机 / 0x04 保留 / 0x05 禁充 / 0x06 禁放 / 0x07 充放禁止 / 0x08 故障 / 0x09 故障恢复 / 0x0A 上电中 / 0x0B 下电中 / 0x0C 下电完成 / 0x0D 预留 |
| 2 | 101 | 簇允许充电最大功率 | uint16 | 0.1 | — | 1 | kW | R | 充电可用功率上限 |
| 3 | 102 | 簇允许放电最大功率 | uint16 | 0.1 | — | 1 | kW | R | 放电可用功率上限 |
| 4 | 103 | 簇允许充电最大电压 | uint16 | 0.1 | — | 1 | V | R | |
| 5 | 104 | 簇允许放电最大电压 | uint16 | 0.1 | — | 1 | V | R | |
| 6 | 105 | 簇允许充电最大电流 | uint16 | 0.1 | — | 1 | A | R | 充放电能力边界 |
| 7 | 106 | 簇允许放电最大电流 | uint16 | 0.1 | — | 1 | A | R | |
| 8 | 107–114 | 主控 DI1–DI8 | uint16 | 1.0 | — | 8 | — | R | 1=信号输入/0=未输入；DI1 主正继电器反馈、DI2 主负、DI3 预充、DI4 预留、DI5 断路器反馈、DI6–DI8 预留（配置为点级 `{ at: 8, count: 8 }` → 产出 `bms_io_8..15` 共 8 点） |
| 9 | 115 | 簇组电压 | uint16 | 0.1 | — | 1 | V | R | 直流侧总电压 |
| 10 | **116** | 簇组电流 | **uint16** | 0.1 | **−1600.0** | 1 | A | R | **类型来源：原文类型列 `UNIT`（= `UINT`，推断为拼写笔误，见 §9.4.2.4 ④与 Q-20）→ `uint16`**。0A 对应 raw=16000（无符号编码 + 负偏移做零点平移）。**`uint16` 满量程**：raw ∈ [0, 65535] ⇒ 物理量程 = **[−1600.0, +4953.5] A**（raw=0 → −1600.0 A；raw=65535 → +4953.5 A；raw=16000 → 0.0 A）。⚠️ 该量程远大于实际工况（±1600 A 上下），故 `uint16`/`int16` 在常用区间**数值等价**，仅在大 raw 值处分歧 —— 判别用例见 AC-2。方向「充电为正、放电为负」见 1130–1139 同族字段注明（**是否同样适用于 116，文档未明确**，§9.10 Q-3） |
| 11 | 117 | 簇组模块温度 | **uint16** | 1.0 | −40.0 | 1 | ℃ | R | **类型来源：原文 `UNIT`（= `UINT`）→ `uint16`**；负偏移 `−40.0` 为无符号零点平移（raw=40 → 0℃），非有符号证据（§9.4.2.4 ②③） |
| 12 | **118** | **簇组 SOC** | uint16 | 1.0 | — | 1 | % | R | **控制输入（点名必须显式声明为 `soc`，见 §9.4.3 的 `soc` 点契约）**；消费链路见 §9.6.1 |
| 13 | 119 | 簇组 SOH | uint16 | 1.0 | — | 1 | % | R | 健康度 |
| 14 | 120 | 簇组绝缘电阻 | uint16 | 1.0 | — | 1 | kΩ | R | |
| 15 | 121 | 簇平均单体电压 | uint16 | 0.001 | — | 1 | V | R | |
| 16 | 122 | 簇平均单体温度 | **uint16** | 1.0 | −40.0 | 1 | ℃ | R | 类型来源：原文 `UNIT`（= `UINT`）→ `uint16`；负偏移为无符号零点平移 |
| 17 | 123 | 簇最高单体电压 | uint16 | 0.001 | — | 1 | V | R | 极值对 |
| 18 | 124 | 簇最高单体电压对应点 | uint16 | 1.0 | — | 1 | — | R | **位定义**（原文 5.4）：bit15–8 = PACK 编号、bit7–0 = **"PACK 编号"（原文如此，与高字节重复 → 自相矛盾，§9.10 Q-18）**。本轮**整字采集**、不拆点（§9.4.2.3） |
| 19 | 125 | 簇最低单体电压 | uint16 | 0.001 | — | 1 | V | R | |
| 20 | 126 | 簇最低单体电压对应点 | uint16 | 1.0 | — | 1 | — | R | 同上位定义（整字，Q-18） |
| 21 | 127 | 簇最高单体温度 | **uint16** | 1.0 | −40.0 | 1 | ℃ | R | 类型来源：原文 `UNIT`（= `UINT`）→ `uint16` |
| 22 | 128 | 簇最高单体温度对应点 | uint16 | 1.0 | — | 1 | — | R | 同上位定义（整字，Q-18） |
| 23 | 129 | 簇最低单体温度 | **uint16** | 1.0 | −40.0 | 1 | ℃ | R | 类型来源：原文 `UNIT`（= `UINT`）→ `uint16` |
| 24 | 130 | 簇最低单体温度对应点 | uint16 | 1.0 | — | 1 | — | R | 同上位定义（整字，Q-18） |
| 25 | 139 | 簇累计充电电量 | int32_scaled | 0.1 | — | 2 | kWh | R | 原文 `UNIT32`（= `UINT32` 笔误，与 §5.3 的 `UNIT` 同族）；量程上限 2^31×0.1≈2.1 亿 kWh，远超实际，用有符号表达即可（如需严格无符号，随 `uint32` 一并扩展） |
| 26 | 141 | 簇累计放电电量 | int32_scaled | 0.1 | — | 2 | kWh | R | |
| 27 | 143 | 簇单次累计充电电量 | int32_scaled | 0.1 | — | 2 | kWh | R | |
| 28 | 145 | 簇单次累计放电电量 | int32_scaled | 0.1 | — | 2 | kWh | R | |
| 29 | 147 | 簇可充电量 | int32_scaled | 0.1 | — | 2 | kWh | R | 剩余可充容量 |
| 30 | 149 | 簇可放电量 | int32_scaled | 0.1 | — | 2 | kWh | R | 剩余可放容量 |
| 31 | 151 | 簇最高单体温升 | uint16 | 1.0 | — | 1 | ℃ | R | |
| 32 | 153 | 簇最高单体最高电压变化 | uint16 | 0.001 | — | 1 | V | R | 电压变化率相关 |
| 33 | 155 | 簇最高单体极柱温度 | **uint16** | 1.0 | −40.0 | 1 | ℃ | R | 类型来源：原文 `UNIT`（= `UINT`）→ `uint16` |
| 34 | 157 | 簇最低单体极柱温度 | **uint16** | 1.0 | −40.0 | 1 | ℃ | R | 类型来源：原文 `UNIT`（= `UINT`）→ `uint16` |
| 35 | 181 | 主控程序版本号 | uint16 | 1.0 | — | 1 | — | R | 用于投运核对固件版本 |
| 36 | 182 | 从控数量 | uint16 | 1.0 | — | 1 | 个 | R | |
| 37 | 183 | 簇组 SOE | uint16 | 1.0 | — | 1 | % | R | 能量状态；点名在**站内**唯一即可（跨站同名由 telemetry 的 device_id 维度区分，如 `bms_meta_3`） |
| 38 | 184 | 单体温度极差值 | uint16 | 1.0 | — | 1 | ℃ | R | 一致性判据 |
| 39 | 185 | 单体电压极差值 | uint16 | 0.001 | — | 1 | V | R | 一致性判据 |
| 40 | 186 | 簇实时充放电功率 | **uint16** | 0.1 | — | 1 | kW | R | **类型来源：原文类型列 `UNIT`（= `UINT`）→ `uint16`**（v1.3/v1.4 曾按"功率天然有符号"改判 `int16`，**该改判已撤销** —— 符号性以厂方标注为准，不以物理量的直觉为准）。⚠️ **本点是本轮残余风险最高的一项**：若厂方实际以补码下发放电负功率，则 `uint16` 会把负值解成数万 kW 级正数（`int16` 则解得合理负值）。判别方法见 §9.10 **Q-20**：投运首日须以「小功率放电」工况核对（放电时该点应为负值或小正值；若读到 ≈6 万 kW 量级即为解错）。正放负充方向：**文档未明确**（同族符号约定见 §9.10 Q-3） |
| 41 | 187 | PACK 组压最高电压 | uint16 | 0.1 | — | 1 | V | R | |
| 42 | 189 | PACK 组压最低电压 | uint16 | 0.1 | — | 1 | V | R | |
| 43 | 2991 | 簇端子温度 001–004（箱体 T1–T4） | **uint16** | 1.0 | −40.0 | 4 | ℃ | R | 类型来源：原文 `UNIT`（= `UINT`）→ `uint16`；块 `bms_term` 整窗口产出 4 点（`bms_term_1..4`） |
| 44 | 4000 | 簇累计充电容量 | uint16 | 1.0 | — | 1 | Ah | R | 文档标 4000~4001（跨 2 寄存器但为 1/AH 精度）；**32 位还是 16 位文档未明确**（§9.10 **Q-16**） |
| 45 | 4002 | 簇累计放电容量 | uint16 | 1.0 | — | 1 | Ah | R | 同上 |
| 46 | 4004 | 簇单次累计充电容量 | uint16 | 1.0 | — | 1 | Ah | R | |
| 47 | 4005 | 簇单次累计放电容量 | uint16 | 1.0 | — | 1 | Ah | R | |

**B. 离散输入表（FC02）——告警与状态位**（`func: discrete`，`addr` = 位地址）

> 点位名：块起点 = 位地址 200 ⇒ **`bms_alarm_<位地址−199>`**（如位地址 424 = 一级告警 ⇒ `bms_alarm_225`；§9.4.2.2 的位置命名规则，绝对位地址命名已废止）。

| 位地址 | 名称 | 含义 |
|--------|------|------|
| 200 | 簇主控通讯失联（保留） | 1=告警 |
| 201–203 | 簇端电压欠压 轻/中/重 | 三级告警 |
| 204–206 | 簇端电压过压 轻/中/重 | |
| 207–209 | 簇端充电电流 轻/中/重 | |
| 210–212 | 簇端放电电流 轻/中/重 | |
| 213–215 | 簇单体欠压 轻/中/重 | |
| 216–218 | 簇单体过压 轻/中/重 | |
| 219–221 | 簇单体欠温 轻/中/重 | |
| 222–224 | 簇单体过温 轻/中/重 | |
| 225–227 | 簇 SOC 低 轻/中/重 | 与 118 互为印证 |
| 228–230 | 簇 SOH 低 轻/中/重 | |
| 231–233 | 簇单体压差 轻/中/重 | |
| 234–236 | 簇单体温差 轻/中/重 | |
| 237–276 | 簇从控 1–40 通讯失联 | 逐从控 |
| 277–279 | 簇端子（箱体）温度过高 轻/中/重 | |
| 280–282 | 簇（pack）电压过高 轻/中/重 | |
| 283–285 | 簇（pack）电压过低 轻/中/重 | |
| 286 | 簇单体电压采集故障 | 保护类 |
| 287 | 簇单体温度采集故障 | 保护类 |
| 288–297 | 簇初始状态 / 充电 / 放电 / 就绪 / 维护（保留）/ 禁充 / 禁放 / 充放禁止 / 故障 / 测试模式（保留） | 运行态标识（288=初始态，289=充电，290=放电，291=就绪，293=禁充，294=禁放，295=充放禁止，296=故障） |
| 298 | 簇高压箱状态 | |
| 299 | 簇从控 DI 告警状态（风扇/气溶胶/MSD） | |
| 300–339 | 簇从控 1–40 DI 定制告警状态 | 逐从控 |
| 340–342 | 簇单体充电过温 轻/中/重 | |
| 343–345 | 簇单体充电欠温 轻/中/重 | |
| 346–348 | 簇单体放电过温 轻/中/重 | |
| 349–351 | 簇单体放电欠温 轻/中/重 | |
| 352–354 | 簇单体温升过大 轻/中/重 | |
| 355–357 | 簇单体极柱温度过温 轻/中/重 | |
| 358–360 | 簇单体极柱温度欠温 轻/中/重 | |
| 361–363 | 簇单体电压变化过大 轻/中/重 | |
| 364–423 | 定制保留 | 无消费方 |
| **424** | **簇一级告警** | **一级报警与保护标识（必须采集）** |
| **425** | **簇二级告警** | **二级** |
| **426** | **簇三级告警** | **三级** |
| 427–429 | 簇端子（箱体）温度过低 轻/中/重 | |
| 430–432 | MOS 过温 轻/中/重 | |
| 433–435 | MOS 欠温 轻/中/重 | |
| 436–438 | 簇 SOE 低 轻/中/重 | |
| 439–441 | 簇单体正极柱温度过温 轻/中/重 | |
| 442–444 | 簇单体正极柱温度欠温 轻/中/重 | |
| 445–447 | 簇单体负极柱温度过温 轻/中/重 | |
| 448–450 | 簇单体负极柱温度欠温 轻/中/重 | |
| 451–453 | 簇绝缘检测低 轻/中/重 | |
| 454–459 | 总正 / 总负 / 预充 / 风扇 / 休眠继电器粘连故障、断路器粘连故障 | 保护类 |
| 460 | AFE 故障 | |
| 461–462 | 单体温度短路 / 断路故障 | |
| 463 | MOS 温度故障 | |
| 464 | 均衡 MOS 故障 | |
| 465–469 | 从控通讯 / 供电 / 风扇 / 程序升级 / 参数设置故障 | |
| 470–472 | 从控供电过压故障 轻/中/重 | |
| 473 | 主控供电故障 | |
| 474 | 主控程序升级故障 | |
| 475–477 | 主控供电过压故障 轻/中/重 | |
| 478 | EEPROM 存储故障 | |
| 479 | 地址编码故障 | |
| 480 | CAN 电流采集故障 | |
| 481 | 485-1 通讯失联故障 | 与本站自身链路自检相关 |
| 482 | 485-2 通讯失联故障 | |
| 483 | PCS 失联故障 | PCS 侧链路状态（**仅事件，不参与联锁**） |
| 484–599 | 保留 | 无消费方 |

> **位块配置口径**：以 `{ name: bms_alarm, func: discrete, addr: 200, count: 288 }` 覆盖 200–487 的连续位窗口（288 位 = 36 字节），单次 FC02 事务（≈55ms，见 E 表）即可取得全部告警位。**一次事务优于按子语义拆多块**（拆分与位对齐无关：FC02 按位地址直接索引，不要求 8 对齐——v1.2 的"必须 8 位对齐"规则已在 §9.4.3 删除）。窗口内 364–423 为"定制保留"，其点位（`bms_alarm_165..224`）**产出但不参与任何判据**（§9.7.6）。

**C. 保持寄存器（FC03）——写侧**：504 簇上下电控制 / 524 簇系统故障复位（限流状态清除）/ 525 簇绝缘检测 / 526 簇系统重启复位 / 1000–1157 阈值与标定参数，全部 `R/W` → 归 §9.2.3 写 Task（本轮不采集：它们是**设定值**而非测量值，采集会造成"设定值被当成实测值"的语义污染）。

**D. 显式不采集的大批量点（取舍理由：总线带宽）**

| 点区 | 数量 | 不采理由 |
|------|------|----------|
| 簇单体电压 001–600（191–790） | 600 寄存器 | 单块 600 regs = 1200 字节，9600bps 下约 1.25s，独占整个轮询周期；本轮无单体级消费方（策略仅用 SOC/SOH 与极值）。后续若做单体级均衡分析，再按需分片接入 |
| 簇单体温度 001–600（891–1490） | 600 寄存器 | 同上；本轮以极值 + 极差（127/129/184）代表 |
| 极柱温度 1–300（1591–1890） | 300 寄存器 | 同上；本轮以极柱极值（155/157）代表 |
| PACK1–40 组压（2951–2990） | 40 寄存器 | 本轮以 PACK 极值（187/189）代表 |
| 定制保留 / 保留区 | — | 无消费方 |

**E. 块划分与带宽核算（9600bps，单轮；按 §9.4.2.1 规则唯一展开）**

| 块 | func | addr | count | 产出点 | 点位 ↔ 地址 | 事务耗时（估） |
|----|------|------|-------|--------|-------------|----------------|
| `bms_io`（100–130 关键遥测） | input | 100 | 31 | 31 | `_k` ↔ 99+k | ≈ 82ms |
| `bms_energy`（139–157 电量与温度极值） | input | 139 | 19 | 10 | `_k` ↔ 138+k | ≈ 57ms |
| `bms_meta`（181–189 版本/极差/功率） | input | 181 | 9 | 8 | `_k` ↔ 180+k | ≈ 36ms |
| `bms_term`（2991–2994 端子温度） | input | 2991 | 4 | 4 | `_k` ↔ 2990+k | ≈ 26ms |
| `bms_cap`（4000–4005 容量） | input | 4000 | 6 | 4 | `_k` ↔ 3999+k | ≈ 30ms |
| `bms_alarm`（200–487 告警位） | discrete | 200 | 288 位 | 288 | `_k` ↔ 位 199+k | ≈ 55ms |
| **合计** | | | 读 69 寄存器 + 288 位 | **345 点** | | **≈ 286ms/轮** → `interval_ms: 1000`，余量 **3.5×**（≥1.5× ✓） |

> **对照算式（为何必须合并读窗口；以下为量级估算，非精确值）**：若回到 v1.2 的"换算口径不同即拆块"规则展开，**仅 100–130 段（31 寄存器）就需 17 块**（该段含 **5 类**换算口径 —— 纯 `1.0`、`uint16`/0.1、`uint16`/0.1/offset −1600、`uint16`/1.0/offset −40、`uint16`/0.001；同类但被异类隔断的连续段不得合并，故 31 个寄存器按"连续同口径"合并后得 **17 块**。注：v1.4 曾把其中两类写作 `int16`，v1.5 按厂方 `UNIT` 标注改回 `uint16` —— **换算口径的分类结果不变**（仍是 5 类），故 17 块与 772ms 的对照数**不受影响**）；其余段各按同法展开，**全站 FC04 量级为 30–40 块**（粒度取"连续同口径合并"时约 30 块，取"一个语义点一块"时更多），叠加 1 次 FC02 后单轮 **量级 ≈772ms** —— `interval_ms: 1000` 的余量仅 1.3×（不满足 ≥1.5×），须把 `battery` 的周期上调到 2000ms 并牺牲 SOC 刷新率。**v1.3 采用"读窗口合并 + 逐点换算"后，块数回到 5×FC04 + 1×FC02，单轮 286ms（该数为 §9.8.1 公式复算后取整，精确值 286.1ms）、周期保持 1000ms。**
>
> **对照数的口径声明（防误当精确值）**：772ms / "17 块" 只在"换算口径不同即拆块"的旧规则下有定义，且随展开粒度（是否合并连续同口径寄存器）在 **730–780ms / 30–40 块** 区间浮动 —— 本节、§9.5.1 与 §9.8.1 三处引用的是**同一量级估算**，用途是说明"严格拆块与 1s 周期不可共存"，**不得**作为精确带宽值引用；只有 286ms（本版口径）是按 §9.8.1 公式复算后取整的值（精确值 286.1ms，其余站的精确值与量级口径见 §9.8.1 末条）。

#### 9.5.2 两级式 PCS（`Role::Pcs`，只读）

**协议事实**：《两级式 PCS 协议 V1.3》。RS485 + Modbus，**默认 19200 N-8-1**，模块地址拨码（**默认 1**），「监控设备为主机，分时查询所有从机（PCS 模块）」；**3 区只支持 04 功能码**，**4 区只支持 03/06 功能码**。**⚠️ 协议明文标注「存在高 8 位和低 8 位互换」**。

- **3 区与 4 区地址空间重叠**（3 区 1000–1075、4 区亦含 1000–1057），仅靠**功能码**区分 → 配置必须用 `func: input`（3 区）/ `func: holding`（4 区）显式区分，不得按地址判区。
- 本轮只读 **3 区**（`func: input`，`byte_swap: true`）。
- **32 位电量字序（文档明确）**：1042「低 16 位」/1043「高 16 位」、1044 低/1045 高、1072 低/1073 高、1074 低/1075 高 → **低字在低地址**，与 `regs_to_u32_be` 相反，须 `word_order: lo_hi`。

> **`format` 的类型来源（PCS 专项，v1.5 核实）**：PCS 文档《协议 V1.3》的 3 区点表**逐点明写类型**，写法为英文驼峰 **`Int16` / `UInt16`**（32 位电量写 `UInt32`），且**逐行不同**（例如 1000–1005 为 `UInt16`，1006/1007 为 `Int16`，1008 又是 `UInt16`，1018–1041 整段 `Int16`，1066 为 `UInt16`，1067–1071 为 `Int16`）。**本表的 `format` 与原文该行类型标注逐字一致**（`Int16` → `int16`、`UInt16` → `uint16`、`UInt32` → `int32_scaled`），**已逐点回原文核对，0 处不符** —— 与 BMS 的"推断订正"来源**不同**（PCS 无需推断，照抄即可），后续读者不得把两种来源混同。32 位电量原文标 `UInt32`（无符号），本 PRD 以 `int32_scaled` 表达，理由同 §9.5.1 的 32 位电量（量程上限远超实际，无需新增 `uint32` 变体）。

**3 区通讯点表（只读，`func: input`，全表 `byte_swap: true`）**

| # | addr | 名称 | 格式 | scale | count | 单位 | 含义 |
|---|------|------|------|-------|-------|------|------|
| 1 | 1000 | 模块故障告警 1 | uint16 | 1.0 | 1 | 位图 | bit0 BMS 故障(码9)/bit1 EMS 失联(10)/bit2 急停故障(11)/bit3 干节点2 故障(12)/bit4 干节点3 故障(13)/bit5 电网过压(14)/bit6 电网欠压(15)/bit7 频率异常(16)/bit8 开关电源故障(1)/bit9 风机故障(2)/bit10 模块过温(3)/bit11 FPGA 故障(4)/bit12 直流中点(5)/bit13 直流母线过压(6)/bit14 直流母线欠压(7)/bit15 BMS 失联(8) |
| 2 | 1001 | 模块故障告警 2 | uint16 | 1.0 | 1 | 位图 | bit0 AN 短路(25)/bit1 BN(26)/bit2 CN(27)/bit3 AB(28)/bit4 BC(29)/bit5 CA(30)/bit6 A 相过载超时(31)/bit7 B 相过载超时(32)/bit8 瞬时过流A(17)/bit9 瞬时过流B(18)/bit10 瞬时过流C(19)/bit11 延时过流A(20)/bit12 延时过流B(21)/bit13 延时过流C(22)/bit14 电网频率过高(23)/bit15 电网频率过低(24) |
| 3 | 1002 | 模块故障告警 3 | uint16 | 1.0 | 1 | 位图 | bit0 B 相零电压穿越超时停机(41)/bit1 C 相零电压穿越(42)/bit2 三相高电压120 穿越(43)/bit3 三相高电压125(44)/bit4 三相高电压130(45)/bit5 交流相序错误(46)/bit6 交流缺相(47)/bit7 电网孤岛故障(48)/bit8 C 相过载超时(33)/bit9 逆变器逐波限流超时(34)/bit10 三相低压穿越(35)/bit11 A 相低压穿越(36)/bit12 B 相低压穿越(37)/bit13 C 相低压穿越(38)/bit14 三相零电压穿越(39)/bit15 A 相零电压穿越(40) |
| 4 | 1003 | 模块故障告警 4 | uint16 | 1.0 | 1 | 位图 | bit0 直流反接保护(57)/bit1 低压瞬时过流1(58)/bit2 低压瞬时过流2(59)/bit3 低压瞬时过流3(60)/bit4 低压延时过流1(61)/bit5 低压延时过流2(62)/bit6 低压延时过流3(63)/bit7 低压短路1(64)/bit8 低压直流过压1(49)/bit9 低压直流过压2(50)/bit10 低压直流过压3(51)/bit11 低压直流欠压1(52)/bit12 低压直流欠压2(53)/bit13 低压直流欠压3(54)/bit14 外部直流欠压(55)/bit15 外部直流过压(56) |
| 5 | 1004 | 模块故障告警 5 | uint16 | 1.0 | 1 | 位图 | **bit8 低压短路2(65)/bit9 低压短路3(66)/bit10 预留故障1(67)/bit11 预留故障1(68)；bit0–7 未在对照表列出（文档未明确，§9.10 Q-5）** |
| 6 | 1005 | BMS 工作状态 | uint16 | 1.0 | 1 | 枚举 | 0 等待 / 1 正常 / 2 禁充 / 3 禁放 / 4 禁充放 / 5 故障 / 6 充电 / 7 放电 |
| 7 | 1006 | BMS 可接受的最大充电电流 | int16 | 0.1 | 1 | A | BMS 能力边界 |
| 8 | 1007 | BMS 可接受的最大放电电流 | int16 | 0.1 | 1 | A | |
| 9 | 1008 | BMS 系统总电压 | uint16 | 0.1 | 1 | V | 转述值（与 BMS 站 115 同源不同路） |
| 10 | **1009** | BMS 系统总电流 | int16 | 0.1 | 1 | A | 有符号；方向：文档未在点表注明（§9.10 Q-3） |
| 11 | **1010** | BMS 系统 SOC | uint16 | 1.0 | 1 | % | 转述值，**不得**作为 SOC 控制源（§9.6.2） |
| 12 | 1011 | 直流母线电压 | int16 | 0.1 | 1 | V | |
| 13 | 1012 | 中点电压 | int16 | 0.1 | 1 | V | |
| 14 | 1013 | 模块运行状态 | uint16 | 1.0 | 1 | 枚举 | 0 停机 / 1 待机 / 2 充电 / 3 放电（**与 intercore 心跳同含义**） |
| 15 | 1014 | 模块故障状态 | uint16 | 1.0 | 1 | 枚举 | 0 正常 / 1 故障 |
| 16 | 1015 | 模块降额状态 | uint16 | 1.0 | 1 | 枚举 | 0 正常 / 1 降额 |
| 17 | 1016 | 并离网状态 | uint16 | 1.0 | 1 | 枚举 | 0 并网 / 1 离网 / 2 并离网异常（离网模式检测到电网电压） |
| 18 | 1017 | 故障告警代码 | uint16 | 1.0 | 1 | 码 | 对照故障告警对照表（1–68） |
| 19–21 | 1018–1020 | 电网 A/B/C 相电压 | int16 | 0.1 | 3 | V | 多值块 3 点；**字节互换现场判据见 §9.7.3** |
| 22 | 1021 | 交流母线频率 | int16 | 0.01 | 1 | Hz | |
| 23–25 | 1022–1024 | 输出电流 A/B/C 相 | int16 | 0.1 | 3 | A | |
| 26–28 | 1025–1027 | 输出视在功率 A/B/C 相 | int16 | 0.1 | 3 | kVA | |
| 29 | 1028 | 设备总视在功率输出 | int16 | 0.1 | 1 | kVA | |
| 30–32 | 1029–1031 | 输出有功功率 A/B/C 相 | int16 | 0.1 | 3 | kW | 正放负充（见 §功能说明） |
| 33 | 1032 | 设备总有功功率输出 | int16 | 0.1 | 1 | kW | |
| 34–36 | 1033–1035 | 输出无功功率 A/B/C 相 | int16 | 0.1 | 3 | kvar | |
| 37 | 1036 | 设备总无功功率输出 | int16 | 0.1 | 1 | kvar | |
| 38–40 | 1037–1039 | A/B/C 相功率因数 | int16 | 0.001 | 3 | — | |
| 41 | 1040 | 总功率因数 | int16 | 0.001 | 1 | — | |
| 42 | 1041 | PCS 温度 | int16 | 1.0 | 1 | ℃ | |
| 43 | 1042 | 交流累计充电电量（**低 16 位**） | int32_scaled | 0.1 | 2 | kWh | `word_order: lo_hi` |
| 44 | 1044 | 交流累计放电电量（低 16 位） | int32_scaled | 0.1 | 2 | kWh | 同上 |
| 45–47 | 1046–1048 | STS 网侧电压 A/B/C | int16 | 0.1 | 3 | V | |
| 48 | 1049 | STS 电网电压幅值 | int16 | 0.1 | 1 | V | |
| 49 | 1050 | STS 电网电压频率 | int16 | 0.01 | 1 | Hz | |
| 50–52 | 1051–1053 | 负载电流 A/B/C 相 | int16 | 0.1 | 3 | A | |
| 53–55 | 1054–1056 | 负载视在功率 A/B/C 相 | int16 | 0.1 | 3 | kVA | |
| 56–58 | 1057–1059 | 负载有功功率 A/B/C 相 | int16 | 0.1 | 3 | kW | |
| 59–61 | 1060–1062 | 负载无功功率 A/B/C 相 | int16 | 0.1 | 3 | kvar | |
| 62–64 | 1063–1065 | 负载功率因数 A/B/C 相 | int16 | 0.001 | 3 | — | |
| 65 | 1066 | 工作模式判断 | uint16 | 1.0 | 1 | 枚举 | 0 低压恒流 / 1 高压恒流 / 2 高压恒压 / 3 恒功率 / 4 低压恒压 / 5 升压 MPPT / 6 降压 MPPT |
| 66 | 1067 | 低压总电流 | int16 | 0.1 | 1 | A | |
| 67 | 1068 | 高压总电流 | int16 | 0.1 | 1 | A | |
| 68 | 1069 | 低压外部总电压 | int16 | 0.1 | 1 | V | |
| 69 | 1070 | 低压总功率 | int16 | 0.1 | 1 | kW | |
| 70 | 1071 | DCDC 温度 | int16 | 1.0 | 1 | ℃ | |
| 71 | 1072 | 直流累计充电电量（低 16 位） | int32_scaled | 0.1 | 2 | kWh | `word_order: lo_hi` |
| 72 | 1074 | 直流累计放电电量（低 16 位） | int32_scaled | 0.1 | 2 | kWh | 同上 |

**3 区共 72 个语义点（占 76 个寄存器，1000–1075 连续；其中 1042–1045、1072–1075 为 4 个 32 位点，各占 2 寄存器 → 表内 #43/#44 与 #73/#74 是同一值的"低/高 16 位"两行，点表编号 1–72 已按"值"计数）**。块划分（§9.4.2.1 规则）：**1 个块 `pcs_3zone`**（`func: input`、`addr: 1000`、`count: 76`、`byte_swap: true`、72 点，点位 `pcs_3zone_<寄存器地址−999>`），19200bps 下 ≈90ms → `interval_ms: 1000` 余量 **11×**。若 `count=76` 超出 PCS 单次读取上限（**文档未明确**，§9.10 Q-6），须分片（建议 38+38），分片不改变点位命名（序号锚定绝对地址）。

> **3 区首点地址存在口径冲突（文档未明确，§9.10 Q-4）**：点表首点标注地址 **1000**（0x3E8），但文档「3 区查询数据帧格式」的示例帧用**起始地址 0x0000** 查询「3 区第一个数据」。二者必有一为"点号"、一为"Modbus 地址"。**本条须现场用 FC04 实测裁定**（读 1000 与读 0 各试一次，比对返回是否符合 1005 状态类量程），配置 `addr` 以实测定论。**在裁定前不得投产 PCS 站。**

#### 9.5.3 储能电能表 ADL400（`Role::MeterBatt`）

**协议事实**：安科瑞《ADL400 导轨式多功能电能表安装说明书 V1.9》。RS485 口支持 **Modbus-RTU 或 DL/T645**；表 7 出厂默认：**Modbus 9600 / 无校验**（645-07 为 2400/Even）；通讯地址 1–254。

**协议选择：Modbus-RTU（选它，不用 DL/T645）**。理由：① 仪表 `03H/10H` 命令与 southd 既有 Modbus 解码链（`protocol: modbus` + `meter_regs` 解码 + `Rs485Device`）**零改造对齐**，而 DL/T645 需另写帧编解码（0x68 起始、6 字节 BCD 地址域、数据域 +0x33 偏移、645 帧长度/校验规则）与新的站协议分支；② 其余 4 台设备均为 Modbus，统一协议减少一条独立链路与一套故障语义；③ 文档 §9 明确「仪表 RS485 通信接口支持 MODBUS-RTU」，其寄存器表（表 8/表 12）即为 Modbus 形态，点表证据完整；④ 645（§9.6/§9.7）仅作可选协议登记。

> **注**：仪表**只支持 `03H` 与 `10H`，不支持 `04H`** → 本站所有块必须 `func: holding`（缺省值），**不得**写 `func: input`。

> **`format` 的类型来源（ADL400 专项，v1.5 核实 —— 混合来源，逐点区分）**：文档表 8/表 12 的列是「**地址 / 名称 / 长度(字节) / 属性 / 备注**」，**没有独立的"数据类型"列**。类型信息只散见于「备注」列，且**覆盖不全**：
>
> | 点 | 原文备注 | PRD `format` | 来源 |
> |----|----------|--------------|------|
> | 电能类（0x0000/0x000A/0x0014/0x001E/0x0028/0x0032、0x0087–0x008C） | 「**整形**」（4 字节） | `int32_scaled` ×2 寄存器 | 厂方标注「整形」= 无符号；PRD 用有符号容器表达（量程上限远超实际） |
> | 功率类（0x0164–0x017F 的有功/无功/视在） | 「**有符号整形**」（4 字节, 3 位小数） | `int32_scaled` | **厂方明写有符号** |
> | 功率因数（0x017C–0x017F） | 「**有符号整形**」（2 字节） | `int16` | **厂方明写有符号** |
> | 电压/电流/频率/线电压（0x0061–0x0066、0x0077、0x0078–0x007A） | 「二次侧数据，单位V/A，保留 N 位小数」（2 字节）—— **无类型字样** | `uint16` | **工程判断**：电压/频率/电流模值为非负物理量；本 PRD 取 `uint16`。**此为判断，非厂方标注** |
> | 零序电流（0x0092） | 「二次侧数据，单位A，2 位小数」（2 字节）—— **无类型字样** | `uint16` | **工程判断**：按非负取 `uint16`。⚠️ 零序电流在部分口径下可为负，若现场读到大 raw 值须复核（同 Q-20 的判别方法） |
> | PT / CT 变比（0x008D/0x008E） | 无（2 字节 R/W） | `uint16` | **工程判断**：变比恒正 |
> | 不平衡度（0x0093/0x0094） | 0x0093「**整型** 单位0.1%」；0x0094 无备注（承前） | `uint16` | **厂方标「整型」**（非负），与工程判断一致 |
>
> 结论：ADL400 **有部分类型标注（4 字节功率类明写「有符号整形」），但 2 字节点普遍无类型字样** ⇒ 其 `uint16` 声明属**工程判断**，依据如上表逐条给出，**不得当作厂方标注引用**。

**Modbus 寄存器点表（`func: holding`，`addr` = 文档地址原值）**

| # | addr | 名称 | 格式 | scale | count | 单位 | 属性 | 备注 |
|---|------|------|------|-------|-------|------|------|------|
| 1 | 0x0000 | 当前组合有功总电能 | int32_scaled | 0.01 | 2 | kWh | R | 二次侧数据（有变比须乘 PT×CT） |
| 2 | 0x000A | 当前正向总有功电能 | int32_scaled | 0.01 | 2 | kWh | R | |
| 3 | 0x0014 | 当前反向总有功电能 | int32_scaled | 0.01 | 2 | kWh | R | |
| 4 | 0x001E | 当前组合无功总电能 | int32_scaled | 0.01 | 2 | kvarh | R | |
| 5 | 0x0028 | 当前正向总无功电能 | int32_scaled | 0.01 | 2 | kvarh | R | |
| 6 | 0x0032 | 当前反向总无功电能 | int32_scaled | 0.01 | 2 | kvarh | R | |
| 7 | 0x0061–0x0063 | A/B/C 相电压 | uint16 | 0.1 | 3 | V | R | 多值块 3 点；保留 1 位小数（有变比乘 PT） |
| 8 | 0x0064–0x0066 | A/B/C 相电流 | uint16 | 0.01 | 3 | A | R | 多值块 3 点；保留 2 位小数（有变比乘 CT） |
| 9 | 0x0077 | 频率 | uint16 | 0.01 | 1 | Hz | R | |
| 10 | 0x0078–0x007A | A-B / C-B / A-C 线电压 | uint16 | 0.1 | 3 | V | R | 三相三线/四线均可用 |
| 11 | 0x0087–0x008C | A/B/C 相正向有功电能 | int32_scaled | 0.01 | 6 | kWh | R | 分相电能，3 点（每相 2 寄存器；原 v1.2 误写区间上界 0x008B，**与 count 6 不符**，v1.3 订正为 0x008C —— 对应 C-1） |
| 12 | 0x0092 | 零序电流 | uint16 | 0.01 | 1 | A | R | |
| 13 | 0x0093 | 电压不平衡度 | uint16 | 0.1 | 1 | % | R | 整型 |
| 14 | 0x0094 | 电流不平衡度 | uint16 | 0.1 | 1 | % | R | |
| 15 | 0x008D | 电压变比 PT | uint16 | 1.0 | 1 | — | **R/W** | 本轮**只读采集对照**，不改写（写侧归写 Task） |
| 16 | 0x008E | 电流变比 CT | uint16 | 1.0 | 1 | — | **R/W** | 同上 |
| 17 | 0x0164–0x016B | A/B/C 相 + 总有功功率 | int32_scaled | 0.001 | 8 | kW | R | **有符号整形**；4 点（原 v1.2 误写上界 0x016A，与 count 8 不符 → 订正 0x016B，C-1） |
| 18 | 0x016C–0x0173 | A/B/C 相 + 总无功功率 | int32_scaled | 0.001 | 8 | kvar | R | 有符号；4 点（订正上界 0x0173，C-1） |
| 19 | 0x0174–0x017B | A/B/C 相 + 总视在功率 | int32_scaled | 0.001 | 8 | kVA | R | 文档标「有符号整形」；4 点（订正上界 0x017B，C-1） |
| 20 | 0x017C–0x017F | A/B/C 相 + 总功率因数 | int16 | 0.001 | 4 | — | R | **有符号整形**；多值块 4 点 |

> **数据序**：文档 §9.8 示例（`01 03 0000 0002` → `00 00 30 26`，高位 0000 / 低位 3026）证明**高字在前、高字节在前**，与 `RegFormat` 既有大端语义一致 → **无需 `byte_swap`/`word_order`**。

> **`addr: 0` 合法性**：本站点表首址即 0（组合有功总电能），且文档示例即以 0000H 起始 → **ADL400 站必须允许 `addr == 0`**（与 meter_grid 的 `addr > 0` 拦截不符，须按设备分别判定，§9.4.3）。

**块划分与带宽核算（9600bps，单轮；按 §9.4.2.1 规则唯一展开）**

| 块 | addr | count | 产出点 | 点位 ↔ 地址 | 事务耗时（估） |
|----|------|-------|--------|-------------|----------------|
| `mb_e_act_comb`（组合有功总电能） | 0x0000 | 2 | 1 | `mb_e_act_comb_1` | ≈ 22ms |
| `mb_e_act_fwd`（正向总有功） | 0x000A | 2 | 1 | — | ≈ 22ms |
| `mb_e_act_rev`（反向总有功） | 0x0014 | 2 | 1 | — | ≈ 22ms |
| `mb_e_rea_comb`（组合无功总电能） | 0x001E | 2 | 1 | — | ≈ 22ms |
| `mb_e_rea_fwd`（正向总无功） | 0x0028 | 2 | 1 | — | ≈ 22ms |
| `mb_e_rea_rev`（反向总无功） | 0x0032 | 2 | 1 | — | ≈ 22ms |
| `mb_ui`（相电压 + 相电流） | 0x0061 | 6 | 6 | `_k` ↔ 0x0060+k | ≈ 30ms |
| `mb_freq_line`（频率 + 线电压） | 0x0077 | 4 | 4 | `_k` ↔ 0x0076+k | ≈ 26ms |
| `mb_phase`（分相电能 + PT/CT + 零序/不平衡度；含 3 寄存器空洞） | 0x0087 | 14 | 8 | `_k` ↔ 0x0086+k | ≈ 47ms |
| `mb_power`（分相 + 总有功/无功/视在/功率因数） | 0x0164 | 28 | 16 | `_k` ↔ 0x0163+k | ≈ 76ms |
| **合计** | | 读 64 寄存器 | **40 点** | | **≈ 310ms/轮** → `interval_ms: 1000`，余量 **3.2×**（≥1.5× ✓） |

> **两个"不并块"的决定（按 §9.4.2.1 第 3 条）**：① 6 个总电能点彼此相隔 8 寄存器（0x0000/0x000A/…/0x0032），空洞 8 > **4** ⇒ **必须拆成 6 块**（若强行并成 0x0000–0x0033 的 52 寄存器窗口，须多读 46 个未声明寄存器 ≈ 96 字节 ⇒ **增量空读耗时 ≈ 100ms**（此为本方案的**增读代价**，非本站单轮总耗时——本站单轮总耗时见 §9.5.3 的 ≈310ms），只为省 5 次事务的固定开销 ≈ 66ms，**净亏时间**）；② 0x008F–0x0091 是 3 寄存器空洞（≤ 4）⇒ `mb_phase` **并入**（0x0087–0x0094）。

**谐波（2~31 次）——本轮显式不做，取舍理由如下**

| 项 | 事实 | 取舍理由 |
|----|------|----------|
| 总谐波畸变率 THDUa/b/c（0x05DD–0x05DF）、THDIa/b/c（0x05E0–0x05E2） | 各 1 寄存器（2 字节），整形，0.01% | 6 个点尚可承受，但**无任何已定义的消费方**（策略/告警/Web 需求中均无电能质量项） |
| 分相 2~31 次谐波含量（0x05E3/0x0601/0x061F 电压三相、0x063D/0x065B/0x0679 电流三相） | 每相 **2×30 = 60 寄存器**，六相合计 **360 寄存器** | 单轮读数 360 regs = 720 字节，9600bps 下**约 0.75s**，会顶掉整个 1s 轮询预算；且总量 360 个点无消费方 |
| 分相基波/谐波电压电流、谐波有功/无功（0x0697 起） | 另有数十点 | 同上 |

**结论**：本轮 **不采集任何谐波点**，`interval_ms: 1000` 只覆盖上表 10 个块（40 点）；若后续电能质量分析立项，须同时满足：① 给出明确消费方（事件/报表/策略）；② 本站 `interval_ms ≥ 5000`（谐波块单独降频轮询）；③ 单独评审其对 `meter_batt` 站带宽预算的影响。**本轮明确登记为"不做"，不是遗漏。**

> **`meter_batt` 与 `grid_meter` 的型号归属（须确认，§9.10 Q-13）**：本文档型号为 ADL400，文件标题含「关口表 储能表」两种称谓。本轮按既有配置口径把该点表用于 **`meter_batt`（储能表 `/dev/ttyS5`）**；`grid_meter`（关口/台区总表 `/dev/ttyS4`）的点表仍为占位示例，**本轮不动**。若现场关口表亦为 ADL400，可对齐本表校准 `grid_meter` 的 addr/format/scale —— 但 `grid_meter` 是**策略 phase 真源**，其点表变更会直接改变策略输入，**须单独立项评审**，不得随 S3b-2 一起改。

#### 9.5.4 工商储火灾报警控制器（`Role::Fire`）

**协议事实**：安徽正华同安《工商储火灾报警控制器 Modbus 协议 V1.3.1》。RS485，**默认 9600bps、8 位数据位、1 位停止位、无校验**（与 southd 固定 8N1 一致）；RTU 帧格式「本机地址 + 功能码 + 数据 + CRC16」；支持 **0x03 读 / 0x06 单写 / 0x10 连写**；**软件版本须 V1.72 及以上**（投运前须核对，否则寄存器语义可能与 V1.3.1 不一致）。

> **`format` 的类型来源（消防专项，v1.5 核实）**：消防文档《Modbus 协议 V1.3.1》§5「寄存器定义」的列是「**地址(十进制) / 名称 / 读/写(R/W) / 说明 / 默认值**」，**没有数据类型定义表，也没有逐点类型标注**。⇒ 本表**全部 `uint16` 为工程判断**，依据是厂商在「说明」列给出的**取值范围全部非负**：本机地址 1–254、波特率倍数 1–96、系统音量 0–15、喷洒延时 0–255 秒、钢瓶气压 kPa（负值无意义）、复合探测器地址 1–254、数据 2/3/4 明写「**0 – 65535 ppm**」（该区间恰为 16 位无符号满量程 ⇒ 强证据）。**此为工程判断，非厂方标注**。

**保持寄存器（FC03）点表**

| addr | 名称 | 属性 | 格式 | scale | offset | count | 单位 | 含义/取值 |
|------|------|------|------|-------|--------|-------|------|-----------|
| 0 | 本机地址 | R/W | uint16 | 1.0 | — | 1 | — | 1–254（0 为广播地址，仅支持查询地址）；默认 1 |
| 1 | 波特率倍数 N | R/W | uint16 | 1.0 | — | 1 | — | 1–96，波特率 = N×1200bps；默认 8（=9600，与配置一致） |
| 2 | 系统音量 | R/W | uint16 | 1.0 | — | 1 | — | 0–15；默认 15 |
| 3 | 喷洒延时 | R/W | uint16 | 1.0 | — | 1 | s | 0–255；默认 30 |
| **4** | **系统状态** | R | uint16 | 1.0 | — | 1 | 位图 | 见下表位定义 |
| **5** | **钢瓶气压** | R | uint16 | 1.0 | — | 1 | kPa | 默认 0；**「部分产品无此功能」→ 无此功能时恒 0，不得据此判"气压异常"** |
| **6** | **烟感状态** | R | uint16 | 1.0 | — | 1 | 位图 | bit2 点型感烟探测器触发（**预留功能，暂未启用**）/ bit1 复合探测器烟感触发 / bit0 烟感干接点输入接口触发；bit15–3 预留 |
| **7** | **温感状态** | R | uint16 | 1.0 | — | 1 | 位图 | bit2 点型感温（预留未启用）/ bit1 复合探测器温感触发 / bit0 温感干接点触发 |
| **8** | **可燃状态** | R | uint16 | 1.0 | — | 1 | 位图 | bit2 工业可燃探测器（预留未启用）/ bit1 复合探测器可燃触发 / bit0 可燃干接点触发 |
| **9** | **火警状态** | R | uint16 | 1.0 | — | 1 | 枚举 | 0 工作正常 / 1 一级报警 / 2 二级火警 / 3 预留 / 4 紧急启动 / 5 紧急停止 |
| **10** | **复合探测器登记数量** → 点名 **`fire_det_count`**（**v1.7 定名**） | R | uint16 | 1.0 | — | 1 | 个 | 最大 100；**跨文档契约点**（与 `soc` 同类，按 §9.4.2.2 显式 `name`、站内唯一）：**既是遥测点，又被配置期/运行期校验引用** —— §9.4.1 参考配置在 `fire_sys` 的 `at: 7` 上声明 `name: fire_det_count`；消费侧一律按**点名**取登记数（**不得**按"块名 `fire_det` + 硬编码寄存器 10"查找，那属设备特判，违反 G-5），用于本节的登记数交叉校验。**改名 = 断供该校验**，须按 §9.4.2.2 的改名流程登记与评审 |
| 11 | 复合探测器地址 | R | uint16 | 1.0 | — | 1 | — | 有效范围 1–254（第 1 只探测器） |
| 12 | 复合探测器状态 | R | uint16 | 1.0 | — | 1 | 位图 | 见下表位定义 |
| 13 | 复合探测器数据 1 | R | uint16 | 1.0 | — | 1 | — | **位域打包**（原文）：bit15–8 烟雾（dB/M，1 位小数）/ bit7–0 温度（℃，**偏移值 55**）。本轮**整字采集原值**，字节拆解在展示/事件层完成：烟雾 = `(raw >> 8) × 0.1` dB/M、温度 = `(raw & 0xFF) − 55` ℃（§9.4.2.3 的 G-6 延后口径；原文该字段语义**无歧义**，与 BMS 位置编号不同） |
| 14 | 复合探测器数据 2 | R | uint16 | 1.0 | — | 1 | ppm | CO 浓度 0–65535 |
| 15 | 复合探测器数据 3 | R | uint16 | 1.0 | — | 1 | ppm | VOC 浓度 0–65535 |
| 16 | 复合探测器数据 4 | R | uint16 | 1.0 | — | 1 | ppm | H2 浓度 0–65535 |
| 1994–1995 | 预留 | — | — | — | — | — | — | 不配 |
| 1996 | 系统时间【年 月】 | R/W | uint16 | — | — | 1 | — | bit15–8 年+2000（25=2025 年）/ bit7–0 月 |
| 1997 | 系统时间【日 时】 | R/W | uint16 | — | — | 1 | — | bit15–8 日 / bit7–0 时 |
| 1998 | 系统时间【分 秒】 | R/W | uint16 | — | — | 1 | — | bit15–8 分 / bit7–0 秒 |
| 1999 | 远程控制 | **W** | uint16 | 1.0 | — | 1 | — | 0 消音 / 1 复位 / 2 启动 / 3 停止 → **写 Task**（§9.2.3） |

**系统状态（addr 4）位定义**

| 位 | 含义 | 取值 |
|----|------|------|
| bit15 | 工作模式 | 1 自动 / 0 手动 |
| bit14 | 主电状态 | 1 故障 / 0 正常 |
| bit13 | 备电状态 | 1 故障 / 0 正常 |
| bit12 | 充电状态 | 1 充电 / 0 未充 |
| bit11 | 驱动电路状态 | 1 故障 / 0 正常 |
| bit10 | 压力传感器状态 | 1 故障 / 0 正常 |
| bit9 | 电磁阀接口状态 | 1 开启 / 0 关闭 |
| bit8 | 喷洒标记 | 1 已喷 / 0 未喷 |
| bit7–0 | 备电电量 | 0–100（%） |

**复合探测器状态（addr 12）位定义**

| 位 | 含义 | 取值 |
|----|------|------|
| bit15 | 通信状态 | 1 在线 / 0 离线 |
| bit14 | 故障总状态 | 1 故障 / 0 正常 |
| bit13 | 电磁阀接口状态 | 1 开启 / 0 关闭 |
| bit12 | 报警总状态 | 1 报警 / 0 正常 |
| bit11 | 反馈 2 输入状态 | 1 有效 / 0 正常 |
| bit10 | 反馈 1 输入状态 | 1 有效 / 0 正常 |
| bit9 | VOC 传感器状态 | 1 故障 / 0 正常 |
| bit8 | H2 传感器状态 | 1 故障 / 0 正常 |
| bit7 | CO 传感器状态 | 1 故障 / 0 正常 |
| bit6 | 烟雾传感器状态 | 1 故障 / 0 正常 |
| bit5 | 温度传感器状态 | 1 故障 / 0 正常 |
| bit4 | VOC 报警 | 1 报警 / 0 正常 |
| bit3 | H2 报警 | 1 报警 / 0 正常 |
| bit2 | CO 报警 | 1 报警 / 0 正常 |
| bit1 | 烟雾报警 | 1 报警 / 0 正常 |
| bit0 | 温度报警 | 1 报警 / 0 正常 |

**探测器块寻址规则**：**每只复合探测器占 6 个连续寄存器**，第 n 只的首地址 = **`(n-1)*6 + 11`**；探测器按**地址号从小到大顺序排列**，具体地址须读寄存器数据确认（即"第 n 只"是**按地址升序的第 n 只**，不一定等于其自身地址号）。

| 探测器内偏移 | 寄存器 | 语义 | 整字口径下的点位 |
|--------------|--------|------|------------------|
| +0 | 首地址 | 复合探测器地址（1–254） | 1 个点（u16/1.0） |
| +1 | 首地址+1 | 复合探测器状态（上表位定义） | 1 个点（u16/1.0，位含义见上表） |
| +2 | 首地址+2 | 数据 1（高字节烟雾 `0.1 dB/M`；低字节温度 `raw−55 ℃`） | 1 个点（**整字**，字节拆解在展示层） |
| +3 | 首地址+3 | 数据 2（CO，ppm） | 1 个点 |
| +4 | 首地址+4 | 数据 3（VOC，ppm） | 1 个点 |
| +5 | 首地址+5 | 数据 4（H2，ppm） | 1 个点 |

**块划分与轮询周期要求（9600bps；按 §9.4.2.1 规则唯一展开）**

| 块 | addr | count | 产出点 | 事务耗时（估） |
|----|------|-------|--------|----------------|
| `fire_sys`（系统态 4–16，含探测器 1 的 11–16） | 4 | 13 | 13 | ≈ 45ms |
| `fire_det`（探测器 2..n 区段 17 起，6×(n−1) 寄存器；**整窗口逐寄存器产出 1 点/寄存器**） | 17 | 6×(n−1) | 6×(n−1) | n=20（114 寄存器）→ ≈ 255ms；n=100 → 5 片 ≈ 1.28s |

- **点位命名**：`fire_sys_<寄存器−3>`（`fire_sys_1` = 系统状态）、`fire_det_<寄存器−16>`（`fire_det_1` = 探测器 2 的地址寄存器）。**唯一例外（v1.7）**：寄存器 10（复合探测器登记数量）在 §9.4.1 参考配置中**显式声明 `name: fire_det_count`** ⇒ 其点名是该显式名，**不再**取自动名 `fire_sys_7`（§9.4.2.2：显式 `name` 优先，仅用于跨文档契约点）。
- 配置的探测器块 `count` **按投运时实际登记数量固定**（登记数由**点名 `fire_det_count`**（寄存器 10，v1.7 定名）读出核对）。
- **必须分片**：Modbus FC03 单次读寄存器数上限 125（**本设备上限文档未明确**，§9.10 Q-7，须现场实测），故探测器区按**每片 ≤120 寄存器（= 20 只探测器）**分片；分片不改变点位命名（序号锚定绝对地址）。
- **周期要求**：系统态 1000ms；**探测器块按登记数量决定** —— n ≤ 20（单片）时并入 1000ms 周期（单轮总 ≈300ms，余量 3.3×）；**n > 20（需 2 片及以上）时探测器块独立降频（≥5000ms）**（n=100 时 5 片 ≈1.28s，对 5000ms 周期余量 3.9×）。
- **登记数交叉校验（强制）**：每轮读回后把**点名 `fire_det_count` 的点**（= 寄存器 10，**v1.7 定名**）与配置的探测器块**容量**比对，**不一致即产出告警事件**（探测器增减须人工复核配置，防"新增探测器未被采集"的静默盲区）。**该点名为跨文档契约（站内唯一、与 `soc` 同类）**，消费侧按点名查找 —— 改名即断供本校验，须按 §9.4.2.2 流程登记与评审。
  - **容量算式（v1.10 订正）**：探测器 1 的 6 个寄存器（地址 11–16）落在 **`fire_sys`** 块内，**不在** `fire_det*` 前缀块 ⇒ 只数 `fire_det*` 前缀块会**漏计探测器 1**（按 §9.4.1 参考配置 `fire_det.count = 114` ⇒ `114/6 = 19`，而读回登记数为 **20** ⇒ 判据恒真）。正确口径分两种情况：
    - **链首可得时**（存在读成功且覆盖**寄存器 11** 的块）：容量 = **`1 + Σ(fire_det 前缀块 count) / 6`**（+1 即探测器 1）；
    - **链首不可得时**（未覆盖寄存器 11 / 覆盖块读失败）：降级为 **`Σ(fire_det 前缀块 count) / 6`**（不加 1），与地址序校验（Q-9）的降级口径**同源**（共用"是否覆盖寄存器 11"这一判定）。
  - **参考配置复核**：§9.4.1 的 fire 站（`fire_det.count = 114`）+ 链首可得 ⇒ 容量 = `1 + 114/6 = 20` == 读回 **20** ⇒ **一致**。

#### 9.5.5 风冷空调机组（`Role::Hvac`）

**协议事实**：MODBUS RTU，8 位数据位、1 位停止位、校验位可选（无/奇/偶）、波特率 9.6/19.2/38.4 kbps，**出厂 9.6kbps + 偶校验**；支持 **FC02 读离散输入 / FC03 读保持 / FC06 单写 / FC04 读输入**；**数据地址从 0 开始编址（第一个数据地址为 0）**；数据类型为 1 位或 16 位。

> **⚠️ 与用户初述的偏差（以协议事实为准）**：初述要求"采集**保持寄存器**（柜内外温湿度等）"——协议事实是：**柜内外温湿度实测值在 FC04 输入寄存器（30001–30005），而 FC03 保持寄存器（40001–40059）全部是 R/W 设定/使能/开关机参数**（不是测量值）。因此本轮只读采集 = **FC02 位 + FC04 温湿度**，FC03 全部归写 Task（§9.2.3）。

> **⚠️ 配置 `addr` 用 0 基地址**：点表给的 PLC 地址（10001/30001/40001）**不得**写入 `addr`；须用「起始地址」列（0 基）。

**A. 离散输入（FC02，`func: discrete`）——只读告警位（31 位，PLC 10001–10031）**

> 点位名：`hvac_di_<位地址+1>`（块 `addr: 0`、`count: 31`），如 `hvac_di_1` = 内风机（§9.4.2.2）。

| 位地址 | PLC 地址 | 名称 | 取值 |
|--------|----------|------|------|
| 0 | 10001 | 内风机 | 0 停止 / 1 运行 |
| 1 | 10002 | 应急风机 | 0 停止 / 1 运行 |
| 2 | 10003 | 制冷状态 | 0 停止 / 1 运行 |
| 3 | 10004 | 加热状态 | 0 停止 / 1 运行 |
| 4 | 10005 | 制冷除湿状态 | 0 停止 / 1 运行 |
| 5 | 10006 | 加热除湿状态 | 0 停止 / 1 运行 |
| 6 | 10007 | 系统自检状态 | 0 停止 / 1 运行 |
| 7 | 10008 | 系统运行状态 | 0 停止 / 1 运行 |
| 8 | 10009 | 报警继电器输出 | 0 关闭 / 1 吸合 |
| 9 | 10010 | 柜内温感故障 | 0 正常 / 1 故障 |
| 10 | 10011 | 柜内高温告警 | 0 正常 / 1 告警 |
| 11 | 10012 | 柜内低温告警 | 0 正常 / 1 告警 |
| 12 | 10013 | 柜外温感故障 | 0 正常 / 1 故障 |
| 13 | 10014 | 柜外高温告警 | 0 正常 / 1 告警 |
| 14 | 10015 | 柜外低温告警 | 0 正常 / 1 告警 |
| 15 | 10016 | 柜内湿感故障 | 0 正常 / 1 故障 |
| 16 | 10017 | 柜内高湿告警 | 0 正常 / 1 告警 |
| 17 | 10018 | 柜内低湿告警 | 0 正常 / 1 告警 |
| 18 | 10019 | 柜外湿感故障 | 0 正常 / 1 故障 |
| 19 | 10020 | 柜外高湿告警 | 0 正常 / 1 告警 |
| 20 | 10021 | 柜外低湿告警 | 0 正常 / 1 告警 |
| 21 | 10022 | 压缩机高压告警 | 0 正常 / 1 告警 |
| 22 | 10023 | 压缩机低压告警 | 0 正常 / 1 告警 |
| 23 | 10024 | 制冷失效告警 | 0 正常 / 1 告警 |
| 24 | 10025 | 制热失效告警 | 0 正常 / 1 告警 |
| 25 | 10026 | 保留 | — |
| 26 | 10027 | 内盘管温感故障 | 0 正常 / 1 故障 |
| 27 | 10028 | 内盘管低温告警 | 0 正常 / 1 告警 |
| 28 | 10029 | 三相电报警 | 0 正常 / 1 告警 |
| 29 | 10030 | 接管/回风温差报警 | 0 正常 / 1 告警 |
| 30 | 10031 | 循环模式 | 0 非循环 / 1 循环 |

**B. 输入寄存器（FC04，`func: input`）——本轮要采集的温湿度**

> **`format` 的类型来源（空调专项，v1.5 核实）**：空调文档**有逐点类型标注** —— FC02 段标 `BOOL类型`，FC03/FC04 段标 **`16位有符号`** 或 **`16位无符号`**。⇒ 本表的 `format` **逐字照抄原文**：`16位有符号` → `int16`、`16位无符号` → `uint16`。**空调 30001/30003 = `int16` 是厂方明写（非工程判断）**：30001「柜内测量温度 … **16位有符号** 只读 ℃」，30003「内盘管测量温度 … **16位有符号** 只读 ℃」；30004「柜内测量湿度 … **16位无符号** 只读 %」——故 30001/30003 保持 `int16`，**无须也不得改回 `uint16`**。温度需负值（冬季/低温工况）与厂方标注互相印证。

| addr | PLC 地址 | 名称 | 格式 | scale | count | 单位 | 说明 |
|------|----------|------|------|-------|-------|------|------|
| 0 | 30001 | 柜内测量温度 | int16 | 0.1 | 1 | ℃ | 实际值 = 读数/10，**有符号** |
| 1 | 30002 | 柜外测量温度 | — | — | — | — | **文档明确标注「预留寄存器」→ 不配（无实测值）** |
| 2 | 30003 | 内盘管测量温度 | int16 | 0.1 | 1 | ℃ | **⚠️ 文档自相矛盾**：名称是"温度"、说明写「实际湿度=读数/10」（§9.10 Q-10）。按名称与单位列（℃）配为温度，投运须与设备显示比对确认 |
| 3 | 30004 | 柜内测量湿度 | uint16 | 0.1 | 1 | % | 实际值 = 读数/10，无符号 |
| 4 | 30005 | 柜外测量湿度 | — | — | — | — | **文档明确标注「预留寄存器」→ 不配** |

> **柜外温湿度无实测值的后果（必须写清）**：柜外温湿度只能通过 FC02 的 10013/10014/10015（柜外温感故障/高温/低温告警）与 10019/10020/10021（柜外湿感故障/高湿/低湿告警）**间接**反映，**不得**在 Web/报表中以"柜外温度 = ?"的数值形式展示。

**块划分与带宽核算（9600bps，单轮；按 §9.4.2.1 规则唯一展开）**

| 块 | func | addr | count | 产出点 | 点位 ↔ 地址 | 事务耗时（估） |
|----|------|------|-------|--------|-------------|----------------|
| `hvac_in`（30001/30003/30004；窗口止于最后一个声明点 30004，30002 的 1 寄存器空洞在限值内） | input | 0 | 4 | 3 | `_k` ↔ 寄存器 k−1 | ≈ 26ms |
| `hvac_di`（31 位告警） | discrete | 0 | 31 位 | 31 点 | `_k` ↔ 位 k−1 | ≈ 22ms |
| **合计** | | | 读 4 寄存器 + 31 位 | **34 点** | | **≈ 48ms/轮** → `interval_ms: 5000`，余量 **104×** |

> **v1.2 勘误（C-4）**：v1.2 的 §9.8.1 写本站为"3×FC04 + 1×FC02 / ≈15 字节 / ≈100ms"，与点表及块规则均不符（温湿度在同一次 FC04 窗口内，无需 3 次事务；15 字节亦低估了帧开销）。v1.3 按上表订正为 **1×FC04 + 1×FC02 / ≈48ms**。
>
> **v1.3 残留订正（本轮）**：v1.3 把帧字节写为 ≈50，按 §9.8.1 的估时公式复算应为 **≈38 字节** —— FC04（4 寄存器）= 请求 8 + 响应 (5+8) = **21**；FC02（31 位 = 4 字节）= 请求 8 + 响应 (5+4) = **17**；合计 **38**。§9.3.1 的 HVAC 行亦同轮订正（v1.3 仍留"单轮约 100ms"，与本表 ≈48ms 矛盾）。

**C. 保持寄存器（FC03，40001–40059）——全部写侧**：制冷/加热停止温度与灵敏度（40001–40004）、柜内外温/湿度报警阈值（40005–40009、40020–40023）、湿度设定 40007、应急风机模式与间隔/运行时间（40010–40012）、柜内模拟温度 40013（**写入型模拟值，不是实测**）、485 地址/波特率/奇偶校验（40014/40015/40016）、用户出厂设置 40017、压缩机制冷/加热间隔（40018/40019）、报警继电器开关 40024、报警音开关 40025、各传感器与检测使能位（40026–40032）、探头修正值（40033–40037）、温感故障时压缩机间隔/运行时间（40038/40039）、制冷/制热故障检测使能（40040/40041）、空闲内风机停止温度 40042、除湿使能 40043、湿度回差 40044、紧急停机功能使能 40045、工厂出厂设置 40046、清除制冷或制热故障 40047、**开关机 40048**、接管温差/时间（40049–40051）、接管温度 40056、**强制模式 40059** → 全部 `R/W`，**归写 Task**。

### 9.6 数据语义与消费方

#### 9.6.1 消费链路总表

| 站 | 数据 | 消费方 | 是否参与控制决策 |
|----|------|--------|------------------|
| `battery`（BMS） | **`soc` 块（118）** | `on_battery_soc` → AiIntegrator `bms_soc` → 策略 `TaiStorageStrategy`（BMS 源优先，超期回落核间 SOC，04 §2.11.1；设计 §10.5） | **是**（`soc_protect` 剪带依据） |
| `battery` | 其余全部点（SOH/总压总流/允许充放电流/单体极值/温度/电量/容量/状态） | `on_station_telemetry` → storage `telemetry`（device_id 维度）+ Web API 展示与校核 | **否**（沿用设计 §10.5 边界：本版仅采集 + SOC 输入，不做其它策略融合） |
| `battery` | FC02 告警位（424/425/426 及全部保护标识） | storage `events`/`faults` + SSE 推送 | **否**——**BMS 告警位不触发停机**（停机触发源仍仅急停/水浸/消防 DI3，S2 联锁语义不扩展） |
| `pcs` | 3 区全部 72 点（占 76 寄存器） | `on_station_telemetry` → telemetry + 健康/校核展示 | **否**（核间 §2.4 数据面边界 ADR-012：PCS 输出功率仅作健康/校验，不并作策略遥测） |
| `meter_batt` | 电气量 + 电能 | telemetry + 能量流核算/校核 + Web | **否**；**禁止**接入 `latest_data`（phase 真源唯一 = `meter_grid`，防双写方） |
| `fire` | 系统状态位 / 探测器 / 火警状态 | events + SSE；与 DI3 干接点按设计 §10.4 融合（**OR 取触发**、**停机仅以 DI3 触发**、恢复需双方复位、去重键「消防+源」） | **是（仅事件/联锁判据，非控制量）** |
| `hvac` | 告警位 + 柜内温湿度 | telemetry（温湿度）+ events（告警位）+ 热管理联动展示 | **否**（设计 §10.1 首版边界：空调只读遥测，温控/启停写指令属未来负载管理） |

**闸门语义（不得回归）**：只有 `meter_grid` 的更新推进 AiIntegrator 的 5s 控制闸门时间戳；`pcs`/`battery`/`meter_batt`/`hvac`/`fire` 的更新**不推进**闸门（防总表掉线而某站活性时，整体闸门被掩盖）。

#### 9.6.2 强制否定项（防双源/防越权）

| ID | 否定项 | 依据 |
|----|--------|------|
| N-1 | `Role::Pcs` **不得**调用 `on_battery_soc` | PCS 3 区 1010 是 BMS 的转述值，与 `battery` 站直采构成同源双写，破坏 `AiIntegrator.bms_soc` 单槽约束 |
| N-2 | `Role::Pcs`/`MeterBatt`/`Hvac`/`Fire` **不得**触发 `on_grid_package` | phase 真源唯一 = `meter_grid`（S3b-1c 收敛） |
| N-3 | `battery` 以外的任何新站点**不得**写入 `set_latest_data` | AiIntegrator 单写方约束 |
| N-4 | `pcs` 站**不得**执行任何写操作（FC05/06/0F/10） | 与 S2 联锁争用 PCS 4 区 500 停机原语（§9.2.3） |
| N-5 | 消防 1999（远程控制）**不得**在本轮被调用 | 属写 Task；控制动作须经策略/AiValidator 链路，不得由采集站直写 |
| N-6 | 采集侧**不得**改写设备参数寄存器（ADL400 的 PT/CT 008D/008E、空调 40014–40017、消防 0–3、BMS 1000+ 阈值） | 写侧变更会静默改变本侧所有换算与判据 |

#### 9.6.3 BMS SOC 的域检查（新增安全要求）

`soc` 是唯一进控制链的南向采集点。要求：**解出的 SOC 值先做域检查（0 ≤ SOC ≤ 100 且为有限值），越界视为该点无效** —— ① 不推 AiIntegrator（回落核间 SOC，避免以坏值剪带）；② 产出告警事件（含站 id、原始寄存器值）；③ telemetry 仍按原值落库（保留证据，不掩盖）。**不做**通用量程自校验（其它点量纲各异，误判风险高于收益），仅对本条控制输入点实施。

### 9.7 边界条件与异常流程

#### 9.7.1 通信超时 / 站离线（沿用 S3a 机制，本轮不改语义）

| 场景 | 行为 |
|------|------|
| 单块读超时 / CRC 错 / 异常码 | 该站本轮失败 → `offline_count + 1`，整站本轮无有效数据；同口其它站不受影响 |
| 站 offline 事件 | 首次即时告警；同一窗口内（按 `stale_timeout_s` = 5s 去抖）不重复刷屏 |
| 离线降频 | 按 `interval_ms << min(offline_count−1, 5)` 指数退避（封顶 32×），恢复后自动上线并产出 online 事件一次 |
| 口 open 失败（设备节点不存在/被占用） | 该口全部站 offline 并告警，**不阻断启动** |
| 确定性错误（非法地址/非法数据，异常码 01/02/03） | **不重试**（重试无意义且占带宽） |
| 数据新鲜度 | `stale_timeout_s = 5s`（对齐 AiIntegrator）；`battery` 站 `interval_ms ≥ 5000` 由配置期直接拒绝 |
| 响应超时门限 | 1000ms（rs485 既有缺省），异常即该块失败 |

#### 9.7.2 点表与现场设备不符时的处置（必须可运维）

1. **地址错**（设备返回异常码 02 或读到 0/常值）：站进入 offline，事件 `reason` 含 `slave/addr/count` → 运维据 `events` 定位到具体块。
2. **量纲/量程错**（如 ADL400 变比未计入、BMS 偏移量漏配）：**本轮不做自动量程校验**（除 SOC 外），判据是**投运前逐点比对**（§9.9 真机验收 RC-1）；比对不一致即修配置，**禁止**在代码中加设备特判。
3. **点表版本错**（BMS 协议 B0；PCS 协议 V1.3；消防协议 V1.3.1 且**要求设备软件 ≥ V1.72**）：BMS 以寄存器 181（主控程序版本号）在系统内核对；**消防与 PCS 协议没有版本寄存器**（§9.8.4），只能凭设备铭牌/本机显示屏/厂方调试工具核对。版本不符时须重新取点表，**不得**按现表凑读。
4. **登记数量变化**（消防探测器）：与寄存器 10 交叉校验不一致即告警（§9.5.4）。
5. **配置错误导致的静默失败**：一律由配置期校验拦截（§9.4.3），不在运行期兜底。

#### 9.7.3 【显式实现风险点】PCS 协议的「高 8 位和低 8 位互换」

**风险描述**：PCS 协议在串口配置说明中明确「**存在高 8 位和低 8 位互换**」，但**未说明作用范围**：是全部 16 位寄存器、还是仅部分；是"寄存器内字节序"，以及 32 位值（低字在低地址）在字节互换下如何组合，文档均**未明确**（§9.10 Q-6）。

**必须显式实现（不得隐式特判）**：`byte_swap: true` 由 PCS 站的每个块配置显式声明，实现按**逐寄存器 `u16::swap_bytes()`** 还原（与核间 §2.4 既有口径一致：`to_pcs_reg/from_pcs_reg` 均做 `swap_bytes`）。

**现场判据（可操作，用于确认互换方向）**：读 3 区 **1010（BMS SOC）**：
- 若读到 **65**（合理 SOC 量级 0–100）→ **无需互换**；
- 若读到 **16640**（= 65×256，即 0x4100）→ **需要互换**（0x0041 被交换为 0x4100）。
同理可用 1018 电网 A 相电压：合理值应为 ~2301（=230.1V）；若读到大得多的值即为字节序错。**投运前必须完成该判据，并把结论写回配置**。

**32 位电量的组合风险**：1042–1045/1072–1075 为「低 16 位在低地址」（`word_order: lo_hi`）**叠加**字节互换，文档未给 32 位示例 → 须现场以「交流累计充电电量」与设备显示/大屏读数比对后定序；**在该比对通过前，PCS 站的 32 位电量点视为不可信**（可先不配该 4 个块，待确认后补配）。

#### 9.7.4 位块（FC02）解包语义

- **Modbus FC02 以字节为单位返回**，且协议规定「较低地址的寄存器存储在一个字节的**较低位**上」（BMS 协议 §3.2.4 有完整示例：连续 16 位 1,1,0,1,1,1,0,0,… → 首字节 `00111011B` = 0x3B；随后 1,1,0,1,1,1,0,1 → `10111011B` = 0xBB）。
- **解包公式（唯一）**：块起点为 `addr`、位数为 `count`；位偏移 `k`（0 ≤ k < count）对应**位地址 `addr + k`**，其取值 = `响应字节[k/8]` 的第 `(k mod 8)` 位（**bit0 = LSB**）。响应字节数 = `ceil(count / 8)`。
- **不要求 8 位对齐**：`addr` 与 `count` 可任意取值（如空调 `addr: 0, count: 31`）。v1.2 的"起点向下取 8 的倍数、长度向上取 8 的倍数"规则已删除——它与 §9.5.5A 的 31 位点表自相矛盾，且解包按绝对位地址索引，与对齐无关（§9.4.3）。
- **点名 = `<块名>_<位偏移+1>`**（位置式，§9.4.2.2）：`bms_alarm_225` = 位地址 424 = 簇一级告警；`hvac_di_1` = 位地址 0 = 内风机。**绝对位地址命名（`bms_alarm_424`）自 v1.3 起废止**。语义映射以 §9.5.1B / §9.5.5A 的点表为准（表内按位地址列出）。
- **风险**：位错 1 位 ⇒ 全部告警语义整体偏移。验收须逐位对位核验（§9.9 AC-3）。

#### 9.7.5 有符号性、单位与分辨率（逐设备汇总）

| 设备 | **符号性来源** | 有符号/单位/分辨率要点 |
|------|----------------|------------------------|
| BMS | **推断订正**（原文 `UNIT` = `UINT` 笔误，§9.4.2.4、Q-20） | **全部 16 位点为 `uint16`**（含带负偏移的温度类与电流类）。温度类：`raw − 40℃`（簇组模块温度/平均/极值/极柱/端子，raw=40 → 0℃）；单体电压 `0.001V`；簇组电压 `0.1V`；**簇组电流 `0.1A + 偏移 −1600.0A`，`uint16` 满量程 [−1600.0, +4953.5] A**（raw=0 → −1600.0 A、raw=16000 → 0.0 A、raw=65535 → +4953.5 A；**与 v1.4 的 `int16` 量程 [−4876.8, +1676.7] A 不同，v1.5 已订正**）；186 簇实时充放电功率同为 `uint16`（**残余风险最高项**，Q-20）。SOC/SOH/SOE 为 `1%`；电量 `0.1kWh`；容量 `1Ah`。**充电为正、放电为负**见 1130–1139 注明（116/186 是否适用文档未明确，Q-3） |
| PCS | **厂方逐点明写**（`Int16`/`UInt16`，逐字照抄，0 处不符） | 逐点见 §9.5.2 表；`0.1V / 0.1A / 0.1kW / 0.1kvar / 0.1kVA / 0.001(PF) / 0.01Hz / 1℃`；**功率正放负充**（4 区设定语义注明"设置为正表示放电"）；32 位电量为 `0.1kWh`（原文 `UInt32`）且**低字在前**；**每寄存器字节互换** |
| ADL400 | **混合**：4 字节功率类/ PF **厂方明写「有符号整形」**；2 字节点（电压/电流/频率/线电压/零序/PT/CT）**无类型字样 ⇒ 工程判断**（表见 §9.5.3） | 电压 `0.1V`、电流 `0.01A`、频率 `0.01Hz`、功率 `0.001kW/kvar/kVA`（**有符号**）、PF `0.001`（**有符号**）、电能 `0.01kWh/kvarh`（**二次侧数据，有变比须乘 PT×CT**）、不平衡度 `0.1%` |
| 消防 | **工程判断**（**文档无类型列**；依据「取值范围全部非负」，其中 CO/VOC/H2 明写 0–65535） | 全部 16 位无符号；钢瓶气压 `1kPa`；CO/VOC/H2 `1ppm`；数据 1 为**位域打包**（高字节烟雾 `0.1 dB/M`、低字节温度 `raw − 55 ℃`）；火警状态为枚举（**二级火警 = 2，非"二级"位**） |
| 空调 | **厂方逐点明写**（`16位有符号` / `16位无符号` / `BOOL类型`） | 温度/湿度 `0.1` 分辨率（读数/10），温度**有符号**（30001/30003 原文即 `int16`）、湿度**无符号**（30004 原文即 `uint16`）；离散输入为 0/1 |

#### 9.7.6 无效值 / 未上电 / 预留字段的表达

| 情形 | 要求 |
|------|------|
| 消防钢瓶气压（addr 5） | 文档标「部分产品无此功能」，默认 0 → **恒 0 不得判为"气压异常/泄漏"**；该点语义标注为"可选"，无该功能时在展示层标注"未配置"而非"0 kPa" |
| 消防探测器 bit2（点型感烟/感温/可燃探测器） | 文档标「预留功能，暂未启用」→ 采集成点但**不产出告警事件** |
| 空调 FC04 30002/30005 | 文档标「预留寄存器」→ **不配块**（无实测值） |
| BMS 保留区（131–138、159–180、364–423、484–599、1591+ 等） | **不配块**（无消费方）；若因区间连续而被并入窗口（如 `bms_energy` 的 152/154/156/158 对应点选择不声明、`bms_alarm` 的 364–423 位），其点语义标注"保留"，**不得**纳入任何判据 |
| **整字采集的位域打包点**（BMS 124/126/128/130、消防探测器数据 1） | G-6 延后（§9.4.2.3）→ 落**原始整字值**；展示/事件层按 §9.5.1/§9.5.4 的字节语义拆解。**BMS 位置编号的低字节语义原文自相矛盾（Q-18）→ 拆分结果不得作为判据**；消防数据 1 拆解无歧义，可用于展示 |
| PCS 4 区标"无效"的参数（1042–1057 等） | 本轮不读（写侧）；如后续接入，须先与厂方确认"无效"含义 |
| 设备未上电 / 断链 | 站 offline，**保留上一有效值并标记 stale**（不写入 0 覆盖，避免"0 值"被误判为真实工况）；恢复后自动上线 |

#### 9.7.7 同口多从站共享带宽导致的周期约束

- 本轮 5+1 站**各占独立物理口**，无同口多站；但同口扩展（如多簇 BMS 共享 `ttyS2`）时：站按 `interval_ms` 计算 `next_due`，慢站超时不得拖累关键站 cadence（`meter_grid`/`battery` 优先，沿用设计 §10.2 口调度预算）。
- **配置期强制**：同口各站 `baud_rate` 与串口校验位必须一致（异值被拒，防静默忽略）。

### 9.8 非功能性需求

#### 9.8.1 轮询周期与总线负载（逐站核算，按 §9.4.2.1 的块规则**唯一**展开）

**估时公式（可复算，全表统一）**：9600bps 8N1 ⇒ 1 字节 = 10 bit ÷ 9600 = **1.04ms**；19200bps ⇒ **0.52ms**。一次 Modbus 事务的字节数 = 请求 8 字节 + 响应 `(5 + 2N)` 字节（N = 本次读回的**寄存器数**；FC02 时 N = `ceil(位数/8)` 字节），另加 **4ms** 从站周转。

| 站 | 单轮事务数 | 单轮读取量 / 帧字节 | 单轮耗时（估） | 配置 `interval_ms` | 余量（≥1.5× 要求） | 总线占用率 |
|----|-----------|---------------------|----------------|---------------------|---------------------|-----------|
| `battery` | 5×FC04 + 1×FC02 | 69 寄存器 + 288 位（36 字节） / ≈252 bytes | **≈ 286ms** | 1000 | **3.5×** ✓ | ≈ 29% |
| `pcs` | 1×FC04（76 寄存器，19200bps） | 76 寄存器 / ≈165 bytes | **≈ 90ms** | 1000 | **11×** ✓ | ≈ 9% |
| `meter_batt` | 10×FC03 | 64 寄存器 / ≈258 bytes | **≈ 310ms** | 1000 | **3.2×** ✓ | ≈ 31% |
| `fire` | 2×FC03（n=20：13 + 114 寄存器） | 127 寄存器 / ≈280 bytes | **≈ 300ms** | 1000（n>20 → 探测器块 ≥5000） | **3.3×** ✓ | ≈ 30% |
| `hvac` | 1×FC04 + 1×FC02 | 4 寄存器 + 31 位（4 字节） / ≈38 bytes | **≈ 48ms** | 5000 | **104×** ✓ | ≈ 1% |
| `meter_grid`（站 `grid_meter`，既有） | 6×FC03（既有） | 既有 | 既有 | 1000 | 既有 | 既有 |

- 各站独占串口 ⇒ 上述占用率**互不叠加**（物理并行）。
- **周期下限约束**：任一站的 `interval_ms` 不得小于该站单轮耗时的 1.5 倍（防上一轮未完下一轮即到期造成积压）；配置期对 `pcs` 站新增 `interval_ms ≥ 500ms` 的下界校验。
- **本轮无需上调任何站的 `interval_ms`**：v1.2 的带宽表与 §9.4.2 的"换算参数不同必须拆块"规则**不可共存**——按该规则严格展开（换算口径不同的连续段各成一块）时：BMS = **30–40×FC04（其中 100–130 段即 17 块）+ 1×FC02 ≈ 772ms/轮**（余量 1.3× ⇒ 须把 `battery` 上调到 **2000ms** 并牺牲 SOC 刷新率）；`meter_batt` = **34×FC03 ≈ 734ms/轮**（余量 1.4× ⇒ 同样须上调到 **2000ms**）。v1.3 以"**读窗口合并 + 逐点换算**"（§9.4.2）取代该规则后，全部站余量恢复到 **3.2× 以上**，`battery`/`meter_batt`/`fire`/`pcs` 周期保持 **1000ms**、`hvac` 保持 **5000ms**，**无需上调任何站**。
  > ⚠️ **本条对照数（772ms / 17 块 / 734ms）均为量级估算**（同一套数字在 §9.1.1、§9.5.1、§9.8.1 三处引用，因拆块粒度不同在 730–780ms 区间浮动），用途是说明"旧规则与 1s 周期不可共存"的量级结论，**不得**当作精确值；**上表的 286/90/300/48ms 才是本版口径下按估时公式复算后取整的值**（精确值分别 286.1 / 89.9 / 299.7 / 47.6ms，取整到 ms 与表中值一致）；⚠️ **`meter_batt` 的 ≈310ms 为量级口径，不与精确值逐位相符** —— 按同上公式（258 字节 ×1.04ms + 10×4ms 周转）= **≈308.8ms**，表中取 ≈310ms 系量级表述（`≈` 口径下不构成错误，但**不得**把 310ms 称作"精确复算值"）。
- **位块点的落库量级（须设计阶段确认口径）**：BMS `bms_alarm` 288 位 + 空调 31 位 = 319 个位点，按 `interval_ms` 全量落 telemetry 时，仅 BMS 位块即约 **288 行/秒**（≈ 2.5×10⁷ 行/天）。本 PRD 不改变"随 `on_station_telemetry` 落库"的既有口径，但要求设计阶段明确**位块点的落库策略**（全量 vs 仅变化沿 + 事件），并评估 SQLite 在 BECG-3568 上的写入与保留策略；该口径不影响本节的带宽结论（落库在 CPU/内存域，不在串口域）。

#### 9.8.2 丢包 / 重试

| 项 | 要求 |
|----|------|
| 通信丢包率 | < 0.1%（对齐 §7.2），以"连续 1h 采集无站 offline"为验收判据 |
| 超时重试 | 单轮内**不重试**；失败即记账 + 退避降频，靠下一轮自愈 |
| 确定性错误 | 异常码 01/02/03 **不重试**（非法地址/数据，重试无意义） |
| 恢复探测 | 退避到期后照常轮询，读成功即恢复上线 |

#### 9.8.3 CPU / 内存开销

| 项 | 要求 |
|----|------|
| 线程/任务 | 每口一条采集 task（阻塞 IO 走 `spawn_blocking`），本轮新增 1 口（PCS）→ 增量 1 task |
| CPU | 新增 1 站（PCS，约 1 次事务/s）对 BECG-3568 的 CPU 增量 < 1% |
| 内存 | 位块解包：BMS 288 位 ≈ 36 字节 + 点结构（每点 < 64 字节）；全部新增点 **618 点** —— battery 345 + pcs 72 + meter_batt 40 + fire 127（n=20）+ hvac 34（逐站点数取自 §9.4.1 的点清单；v1.3 的"400 点量级"为估计值，本轮按清单订正）⇒ 常驻增量 < 100KB |
| 无新增 unsafe | 沿用（§8.3 Q05） |

#### 9.8.4 日志与可观测性

| 项 | 要求 |
|----|------|
| 站级日志 | 采集失败 `warn`（含 `station/role/reason`，reason 须含 `slave/addr/count`）；成功 `debug` |
| 事件 | offline / online / SOC 越界 / 消防登记数不一致 / PCS 32 位电量比对未通过（若配置）均产出事件 |
| telemetry | 每点一个 metric，命名规则唯一（§9.4.2.2）：显式 `name` 优先，否则 `<块名>_<序号>`（序号锚定地址偏移）。随 `on_station_telemetry` 落库；位块点的落库策略（全量 vs 变化沿）见 §9.8.1 末条 |
| 可观测口径 | 站 id 与串口节点须同时出现在日志与事件中（排障时能定位到物理口） |
| 投运核对 | **BMS 主控程序版本号（输入寄存器 181）可读**，作为 BMS 版本核对依据。⚠️ **消防与 PCS 协议均无版本寄存器**（原文只写"软件版本号需 V1.72 及以上"的要求，未定义任何可读的版本/软件版本地址；PCS 1017 是**故障告警代码**，不是版本）→ 消防/PCS 的版本核对只能靠**设备铭牌/本机显示屏/厂方调试工具**，不得在系统内声称"读到版本号" |

### 9.9 验收标准

#### 9.9.1 本机可验证（mock/sim，不接真设备）

| ID | 验收条件 |
|----|----------|
| **AC-1** | **配置解析与校验（两级断言，均以 §9.4.1 的 6 站 YAML 为输入）**：<br>**① 既有字段子集（当前代码即可验证）**：剥离本轮新增的字段/取值（`parity`、`func: discrete`、`byte_swap`、`points`/`offset`/`word_order`）后，该 YAML 退化为既有形制 → **解析通过**，且对既有 `validate()` 的**全部拒绝条件逐条核对均不触发**（id 非空且唯一 / `meter_grid` 与 `battery` 各至多一站 / port 非空 / slave 1..=247 / `interval_ms > 0` / baud_rate 1..=4000000 / `meter_grid` 与 `battery` 的 `interval_ms < 5000` / `meter_grid` 相量块 p·q·pf·u·i 齐备且 `count ≥ 6`、`int32_scaled` 块 `scale > 0`、块名唯一、`addr > 0` 且区间不重叠 / 同口 baud 一致 / port 与 intercore 不同节点）。**其中 `grid_meter` 站与生效配置取值逐字一致，故必然通过**。<br>**② 完整 6 站 YAML（实现 §9.4.3 后验证）**：**解析通过**，且**除 §9.4.3 明列的新增拒绝条件外不触发任何既有拒绝条件**。⚠️ 本条**不主张"当前代码即可跑通完整 YAML"** —— 新增取值（`func: discrete`、`points` 等）在实现前会直接解析失败（未知 variant / 未知字段），这是**预期行为，不是缺陷**；AC-1 据此分两级，避免断言超出已核实事实。<br>**③ 逐条触发 §9.4.3 的拒绝条件**（v1.6 已与规则清单**逐条对表**；**保留的每一条均由校验器可判定** —— 配置层均有对应字段/规则）：`role` 未知名、`pcs` 站 >1、`pcs` 站空 regs、`battery` 站无 `soc` 点、`int16/uint16` 缺 scale、`offset` 与点表登记值不一致（期望值取自 §9.4.3 所述**校验器内的点表登记值常量表**）、点位越界、点位重叠、32 位点跨窗口、点名重复、空洞 >4、位块超上限（`discrete` 块 `count` > 2000 位）、`addr` 非法（非 ADL400/空调 的站 `addr == 0`）、块区间重叠、块落地非极大（**按 §9.4.3 第 15 条 v1.8 订正后的适用域与判据**：仅"均声明了 `points`"的相邻块参与，判据为"合并后空洞 ≤ 4 且 `count ≤ 120`"，`read_slice: true` 者豁免）、同口异 baud/异 `parity`、port 与 intercore 同节点 —— **均返回 `Err`**。<br>**⚠️ v1.6 删除项（四审阻塞）**：**「`uint16/int16` 携带 `offset` 但点表未登记其符号性来源 → 拒」已从本清单删除** —— 配置层**无**"来源标注"字段、校验器亦**看不到 §9.5 文档**，该断言**不可满足**（属"验收标准不可测"）。其核验**归属投运前 RC-1 逐点比对 + §9.5 表核对**；待设计给出**点级来源字段**后，再以"未声明来源而 `offset ≠ 0` → 拒"的形态**纳入 AC**（见 §9.4.3"可执行性说明"）。<br>**⚠️ v1.5 删除项**：v1.3/v1.4 的「`uint16` 携带非零 `offset` → 拒」**已废止**，不得再作为 AC-1 ③ 的触发条件 —— 它是错误规则，且会误拒 §9.4.1 的 `bms_io` 块（`at:17/18/23/28/30` 的 `uint16` + 负 offset 是合法配置） |
| **AC-2** | 解码正确性（注入 canned 寄存器，逐设备抽样；**全部数值已于 v1.3 回原文复算**）：<br>**BMS**：`soc`（118）raw=65 → **65.0 %**；簇组电压（115）raw=5123 → **512.3 V**；簇组模块温度（117，`uint16`/offset −40）raw=**65** → **25.0 ℃**（v1.2 写 raw=650 → 25.0℃ **错误**：650−40 = 610℃；按原文「1℃/bit 偏移 −40℃」须 raw=65）；`bms_term_1`（2991）raw=65 → **25.0 ℃**。<br>**BMS 116 簇组电流（`uint16`/`0.1`/offset −1600.0，v1.5 订正）**：raw=16000 → **0.0 A**（零点）；raw=15000 → **−100.0 A**（常用放电区间，`uint16`/`int16` **同解**）；raw=0 → **−1600.0 A**（量程下限）。<br>**BMS 116 的符号性判别用例（本 PRD 的判别逻辑，取代 v1.4 的循环论证）**：`raw = 65535` 是 `uint16`/`int16` 的**判别点** —— **`uint16` 解 +4953.5 A vs `int16` 解 −1600.1 A**。本 PRD 按厂方类型标注（116 原文标 `UNIT` = `UINT`，无符号）取 **`uint16` ⇒ 期望 +4953.5 A**。⚠️ **该用例的意义不是"证明必须 `uint16`"，而是"把两种解释的差异钉死在可观测边界上"**：因实际工况电流远小于该量程（±1600 A），raw 在 15000–48000 区间内两解释的差异最大也只有数 A 级 —— 但当 raw 落在 **32768–65535** 时两者**完全分道扬镳**。故现场判据为：**若测得某点在"小电流放电"时 raw 落在 32768 以上（表现为数万安培的巨大正值），则说明厂方实为有符号，该点须改声明 `int16`**（登记 Q-20，按变更流程改配置；`format` 变更不改点名）。<br>**186 簇实时充放电功率**：`uint16`/`0.1`、无 offset ⇒ 量程 [0.0, +6553.5] kW（**无负值**）。**这是同一判别逻辑的高风险点**：充电/放电方向由厂方同族字段注明"充电为正、放电为负"（Q-3），若 186 亦遵循该约定而下发补码负值，则 `uint16` 会把负功率解成 5 万 kW 级正数 ⇒ 投运首日必须以"小功率放电"工况核对（Q-20）。<br>**PCS**：1010（`byte_swap`）注入 0x4100 → swap → 0x0041 → **65.0 %**；32 位电量（低字在前）注入 `reg[1042]=0x6400`、`reg[1043]=0x0100` → 逐寄存器 swap 后低字=100、高字=1 → 值 = 100 + 1×65536 = 65636 → ×0.1 = **6563.6 kWh**（若误用 `hi_lo` 则得 655360.1，**必须不一致**）。<br>**ADL400**：A 相电流（0x0064）raw=946 → **9.46 A**（原文 §9.8.1 例 1 实测证据：`01 03 0064 0001` → `03 B2`=946 → 9.46 A）；组合有功总电能（0x0000）返回字 `[0000, 3026]` → (0×65536 + 12326)×0.01 = **123.26 kWh**（原文 §9.8.1 例 2 原值）；总无功功率（0x0172，`0.001`）注入 `[0x0000, 0x3B6C]` → 15212×0.001 = **15.212 kvar**。<br>**消防**：火警状态（9）= 2 → **二级火警**；复合探测器数据 1（13）= `0x4150` → **telemetry 落整字原值 16720**，展示层按 §9.5.4 的字节语义拆解得烟雾 = 0x41×0.1 = **6.5 dB/M**、温度 = 0x50 − 55 = **25 ℃**（**不涉及配置层 `slice` 字段——G-6 已延后，§9.4.2.3**）。<br>**空调**：30001 raw=258 → **25.8 ℃**；30004 raw=602 → **60.2 %**；FC02 位 0（10001）注入 `0x3B` 首字节 → 位 0/1/3/4/5 = 1、位 2/6/7 = 0（依据原文 BMS 同构的位打包语义，§9.7.4） |
| **AC-3** | 位块解包（按 §9.7.4 公式）：BMS `bms_alarm`（288 位 = 36 字节）**逐位与构造位图比对**（重点验证：字节内 **bit0 = 低位地址**、跨字节边界、非 8 倍数位数的尾部处理）；空调 `hvac_di` 31 位逐位比对（响应 4 字节，末字节高 1 位无关位不得污染前 31 位）；**位序号 ↔ 位地址映射**：`bms_alarm_225` = 位 424 = 簇一级告警、`bms_alarm_226` = 位 425 = 二级、`bms_alarm_227` = 位 426 = 三级；`hvac_di_1` = 位 0 = 内风机 |
| **AC-4** | 调度与隔离：新增 `pcs` 站后，站点超时/失败 → 该站 offline + 事件，同口其它站不受影响；退避降频生效；恢复后 online 事件一次；优先级中档（`pcs` 不占高优先级） |
| **AC-5** | 分发正确性：`battery` 站注入合法 SOC → `on_battery_soc` 被调用且值为百分数；注入 **SOC=65535（越界）** → **不调用** `on_battery_soc`、产出告警事件、telemetry 保留原值；`pcs` 站任何输入 → **不调用** `on_battery_soc` 与 `on_grid_package`；`pcs` 站不推进控制闸门时间戳 |
| **AC-6** | 点产出与命名：① 未声明 `points` 的块按"每值槽 1 点"产出（`bms_term` 4 寄存器 → `bms_term_1..4`；`bms_alarm` 288 位 → `bms_alarm_1..288`；`pcs_3zone` 不适用于此条，见下）；② 声明 `points` 的块**只产出列出的点**（`bms_meta` 声明 8 条 → **8 点**，188/190 **不产出**）；③ 点级 `count=N` 连续产出 N 点（`bms_io` 的 `at:8, count:8` → `bms_io_8..15`）；④ 点级 `name` 覆盖位置命名（`battery` 站产出 `soc` 点）；⑤ 32 位点占 2 寄存器且产出 1 点（`pcs_3zone` 76 寄存器 → **72 点**，`pcs_3zone_43` = 1042–1043）；⑥ 单寄存器点（`at` 单值）**必须产出 1 点**（回归"单点块不产点"的既有缺陷） |
| **AC-7** | 工程质量：`cargo build --release` 无警告、`cargo clippy` 无 Error、`cargo test` 全绿、`cargo fmt` 通过（§8.3） |

#### 9.9.2 需真机验证（接实际设备）

| ID | 验收条件 |
|----|----------|
| **RC-1** | **逐站逐点比对**：每站至少抽 10 个语义点，与本机显示值 / 厂方调试软件读数一致（容差 = 该点分辨率；电压/电流/功率/温度必检）。<br>**必须显式点名的必检点（不得以"电流/功率必检"泛化代替，避免真机核对时漏检）**：**BMS 116（簇组电流，`uint16` + scale 0.1 + offset −1600.0）与 BMS 186（簇实时充放电功率，`uint16` + scale 0.1，无 offset）** —— 这两点是本 PRD 「`UNIT` → `UINT`」推断订正的**符号性判别关键点**（若厂方实为有符号下发补码，二者会把负值解成数万量级的大正值），须在**充放电工况**下核对，判据与处置见 §9.10 **Q-20 ②**（判为有符号 ⇒ 该点 `format` 改声明 `int16`，**点名不变**，按变更流程登记）。<br>另：**ADL400 0x0092（零序电流，`uint16` 工程判断）** 与 **BMS 2991–2994（簇端子温度，`uint16` + offset −40.0）** 同属"判断来源"点，投运首日一并核对 |
| **RC-2** | **PCS 字节互换与字序**：按 §9.7.3 判据完成 1010/1018 的互换方向裁定并回写配置；32 位累计电量与设备显示比对通过（未通过则 PCS 的 4 个 32 位电量块**不得**配入） |
| **RC-3** | **PCS 3 区地址口径**：按 §9.5.2 以 FC04 实测裁定 `addr` 基准（0 基 vs 1000 基）并回写配置 |
| **RC-4** | **BMS 主从方向**：按 §2.2 口径（EMS 主机 / 主控从机）轮询实际可得数据（**这是 §9.10 Q-1 解除条件 c 的实证判据**：实测可得且连续 ≥1h 无离线 ⇒ Q-1 由【设计阶段阻塞】降级为投运前核对）；**若实测不可得**，立即转 §9.10 Q-1 与厂方书面确认，按裁定口径调整（若确认为相反口径 ⇒ 本 PRD 的点表映射与 SOC 控制链**重新走需求评审**）<br>✅ **（v1.9，2026-09-22 更新）Q-1 已关闭** —— EMS 为主机经用户确认（依据见 §9.10 Q-1 行 ⑤），本条**性质由"Q-1 解除判据"变为常规投运核实**：实测可得即通过（判据、时长、丢包率要求不变）；**若实测不可得**，仍按上文转厂方书面确认。 |
| **RC-5** | **消防全量探测器**：按实际登记数量读全量，记录单轮耗时；确认 `interval_ms` 满足 §9.5.4 的周期要求；登记数与寄存器 10 一致 |
| **RC-6** | **空调校验位**：现场核对空调**实际**校验位（厂方参数 40016 的实际值），并按 §9.10 **D-1 已裁定方案①** 实测串口通信正常；若实测与配置的 `even` 不符 ⇒ **改配置**（`parity` 字段），**不得改代码**。<br>（D-1 裁定后**空调站的投产限制已解除**，但本项仍须在投运验收中完成。原表述"偶校验/无校验两种情形至少验证一种成立"系 D-1 未裁定时的对冲写法，已随 v1.11 订正） |
| **RC-7** | **连续运行**：全部站点连续采集 ≥ 1h，无站 offline 反复抖动，丢包率 < 0.1%（§7.2） |
| **RC-8** | **总线不冲突**：确认 `pcs` 站串口（建议 `ttyS7`）与 intercore 的 `ttyS0` 为**物理独立**总线（PCS A1/B1 与 A2/B2 是否隔离，§9.10 Q-15）；若为同一总线 → **本站不得启用**，改由 intercore 复用读路径 |

### 9.10 待确认项登记（文档未明确 / 待与厂方确认 / 待评审裁定）

> **写作口径**：以下各项在厂方文档中**未给出明确值或存在内部矛盾**，本 PRD **不填猜测值**。未结项在实施前必须结清。
>
> **（v1.9，2026-09-22 状态更新）** **Q-1 已关闭**（用户确认 EMS 为主机/BMS 为从机 ⇒ 原【设计阶段阻塞】**解除**，见 Q-1 行 ⑤）、**Q-19 已撤销**（消防/空调并接 BMS 485-2 的风险**不成立**，见 Q-19 行"已关闭/撤销"栏）。二者原条目与证据**按"保留历史"未删**。**当前未结项 = 除 Q-1、Q-19 外的各项**（其余各项的结清要求不变）。
>
> **阻塞级别（三档；每档说清"卡在哪个阶段"，便于设计阶段一眼判定能否开工）**：
> - **【设计阶段阻塞】~~Q-1~~ —— ✅ 已关闭（2026-09-22），本档位当前为空、不阻塞设计**。原口径（供追溯）：口径未书面确认、且未给出双方案时，**不得进入设计**：它决定 SOC 采集链是否存在，是架构前提而非配置项（详见该行"影响面/约束/解除条件"三栏）。关闭结论与依据链见 Q-1 行 ⑤。
> - **【投产阻塞·现场裁定】Q-4、Q-15** —— **不阻塞需求评审与设计**（两者只影响 PCS 只读站自身的 `addr` 基准与物理总线前提，改动面限于 PCS 站配置），由现场 **RC-3 / RC-8** 实测裁定后**回写配置**；裁定为"不成立"时的唯一后果是 **PCS 站不启用**，不牵动其余 5 站、不改变任何点表与架构。
> - **【非阻塞·投运前核对】Q-2、Q-5…Q-14、Q-16…Q-18、Q-20、D-1…D-3** —— 按各项"处理口径/解除条件"在投运前结清，不阻塞设计推进。**Q-20 虽为非阻塞，但含"须在投运首日用充放电工况核对"的动作项**（见其"处理口径"栏），不得漏做。
>
> Q-18/Q-19 为 v1.3 新增登记（回应评审 C-2 与拓扑一致性）；**Q-1 的阻塞级别、影响面与解除条件于本版（v1.4）按复审意见补齐**；**Q-1 与 Q-19 于 v1.9（2026-09-22）关闭/撤销，关闭结论分别追加于 Q-1 行 ⑤ 与 Q-19 行"已关闭/撤销"栏**。

| ID | 类别 | 事项 | 文档证据 | 处理口径 |
|----|------|------|----------|----------|
| **Q-1** | **文档自相矛盾｜【设计阶段阻塞】**（**已于 2026-09-22 关闭 ⇒ 不再阻塞设计**；见本行 **⑤**） | **BMS 主从方向**：§2.2「EMS 做通信主机，主控模块做从机」；§3.1 补充段（TCP）「主控模块做 **TCP 服务器**…后台监控主动连接」；§3.1 补充段（RTU）「**主控模块做主机，EMS 做从机**，通过 485-1 通信」 | 三处并存（原文矛盾，**本 PRD 不擅自裁定**）<!-- 历史记录：以下 ①–④ 为本项关闭前的原登记，按"保留历史"未删；关闭结论见 ⑤ --> | **① 默认实现口径（本 PRD 的既定基线）**：取 **§2.2「EMS 做通信主机、主控模块做从机」** —— 与 southd「主机主动轮询」模型、与 `slave: 1`（从站地址 = 簇号）、与 §9.4.1 的 `battery` 站形态**三方吻合**；本 PRD 的**点表映射与 SOC 控制链全部建立在该口径上**。<br>**② 影响面（为何是设计阻塞，不是配置项）**：若厂方确认相反口径（**BMS 主控做主机、EMS 做从机**），则 southd **无站级从站模式** ⇒ `battery` 站的采集路径（谁发起请求）、`on_battery_soc` 的控制链时序（每轮时长 / 5s 新鲜度窗口 / 退避降频语义）、以及 §9.4.1 的 `slave` 字段语义**均须重新架构**。它决定的是"该采集链是否存在"，而非"参数取几"，故不能留到投运前解决。<br>**③ 对设计阶段的约束（强制）**：架构师**必须在设计开工前取得书面确认**（厂方/现场均须以书面形式答复 §2.2 口径是否成立），**或在设计中给出两套可切换方案**（含切换判据、切换代价、以及切换对 `interval_ms`/新鲜度窗口的影响）。未确认且未给双方案的，**设计不得进入实现**。<br>**④ 解除条件（任一满足即解除本项阻塞，并按下述分支处置）**：a. **厂方书面确认 §2.2** → 本 PRD 维持现状，阻塞直接解除；b. **厂方书面确认为相反口径** → **本 PRD 的 §9.5.1 点表映射、§9.6.1 SOC 消费链与 §9.4.1 的 `battery` 站配置须重新走需求评审**（不得由设计阶段自行改写 PRD）；c. **现场 RC-4 按默认口径实测可得数据（连续 ≥1h 无离线）** → 视为实证成立，阻塞降级为"投运前核对项"。<br>**⑤ 【已关闭（2026-09-22，用户确认）】** **结论：EMS 是通信主机（master），BMS 是通信从机（slave）**；现行实现（本 PRD §9 与设计 §11 均按协议 §2.2 口径、`mupc-southd` 由 MUPC **主动轮询** BMS 站）**正确，无需改动**；本项原【设计阶段阻塞】**解除**，①–④ 全部随之失效（不再需要"设计开工前书面确认"或"双方案"）。<br>**依据链（2026-09-22 逐处回原文核实，供后来者免于重新推导）**：<br>**a. 协议 §2.2「通信机制」** —— 明文「**EMS 系统做通信主机，大储主控模块做通信从机**」⇒ **直接支持** EMS 为主机。<br>**b. 同文档 §3.1「使用说明」（TCP 段）** —— 「当使用基于通用工业标准 Modbus-TCP 通信协议，**主控模块做 TCP 服务器**，默认端口 4002……**在该模式下，后台监控主动连接主控模块**。EMS-LAN 只支持一个客户端连接。」⇒ **`TCP 服务器/客户端` 与 `Modbus 主机/从机` 是两个层次的概念**（前者是传输层角色，后者是应用层角色）；**Modbus-TCP 惯例为「客户端 = Modbus 主机」**（发起请求的一方即主机），而「后台监控（EMS）主动连接」恰说明 **EMS 是发起请求方** ⇒ 本段与 a **互相印证**（同指 EMS 是主机），**并不构成矛盾**。原登记把 a 与 b 并列为"相反表述"**属误读**。<br>**c. 同文档 §3.1（RTU 段）** —— 「当使用 Modbus-RTU 通信协议，**主控模块做主机，EMS 做从机**，通过 485-1 通信」⇒ 与 a/b 两处**冲突**，**确认为笔误**（三处中唯一不成立的一处）。<br>**d. 工程判据（只有一种能成立）** —— MUPC 需采集 BMS 的 **SOC / 电压 / 电流 / 告警**，**全部是读操作**；而 **Modbus 只有主机能主动读、且协议无推送机制** ⇒ 若 BMS 是主机，MUPC 永远拿不到数据。故 c 不可行，**只有 a/b 的口径在工程上成立**。<br>**处置**：① Q-1 由「未经确认即不得进入设计」**降为无需处置**（不触发 ④b 的"重新走需求评审"分支）；② **§9.5.1 的点表映射、§9.6.1 的 SOC 消费链、§9.4.1 的 `battery` 站配置全部维持不变**；③ **RC-4 保留**（真机实测仍须做，但性质已由"阻塞解除判据"变为**常规投运核实**，见 §9.9.2 RC-4）。 |
| **Q-2** | 文档未明确 | BMS **485-1 的波特率与校验位** | §2.1 只给 LAN 物理层；§3.1 只说「通过 485-1 通信」 | 配置暂按 `9600 / 8N1`（缺省）填写；**投运前须现场核对** |
| **Q-3** | 文档未明确 | 电流/功率**符号方向**：BMS 116 簇组电流、186 簇实时充放电功率；PCS 1009 BMS 系统总电流 | BMS 在 1130–1139 注明「充电为正，放电为负」，但 116/186 处未注明；PCS 点表未注明（4 区设定处注明"正放负充"） | 按同族约定**推定**"充电为正、放电为负"，**在点表与配置注释中标注为推定**；投运须以充放电工况实测确认 |
| **Q-4** | **文档自相矛盾｜【投产阻塞·现场裁定，不阻塞设计】** | **PCS 3 区首点 Modbus 地址基准**：点表首点标 1000，而「3 区查询数据帧」示例用起始地址 **0x0000** 查询"3 区第一个数据" | 两处并存 | **投产阻塞项，但不是设计阻塞项**：由现场 **RC-3** 用 FC04 实测裁定（分别以 `addr = 1000` 与 `addr = 0` 各读一次，比对返回是否符合 1005 等状态类量程），**裁定结论只回写配置的 `addr`** —— 不改点表、不改点位命名、不牵动其它站；**裁定前 PCS 站不得投产**（其余 5 站照常）。 |
| **Q-5** | 文档未明确 | PCS **1004（模块故障告警 5）bit0–7** 含义 | 对照表只列 bit8–11（低压短路2/低压短路3/预留故障1/预留故障1） | 该 8 位**不产出任何语义**（仅保留原值于 telemetry）；不得据其判故障 |
| **Q-6** | 文档未明确 | **「高 8 位和低 8 位互换」的作用范围**：是否全设备寄存器、是否影响告警位定义（1000–1004 的 bit 划分）、32 位值的组合方式；以及 PCS **单次读取寄存器数上限** | 串口配置说明仅一句全局提示；点表未给 32 位示例；未给单次读取上限 | 按 §9.7.3 显式 `byte_swap` 实现 + 现场判据裁定；32 位电量在比对通过前视为不可信（可先不配）；单次读取超限时分片（先按 38+38 试） |
| **Q-7** | 文档未明确 | 消防**单次读取寄存器数上限** | 协议未给上限（Modbus 标准 FC03 上限 125） | 按 125 分片；现场实测确认 |
| **Q-8** | 文档不一致 | BMS 离散输入表汇总行「保留 **481** / 120」与明细冲突（明细 481 = 「485-1 通讯失联故障」，保留区明细从 **484** 起、尾部标 **600**） | 汇总表 vs 明细表 | 以明细为准（481 为告警位，484–599 保留）；**登记不一致**，须厂方确认汇总表是否为 600 之误 |
| **Q-9** | 文档未明确 | 消防「复合探测器按地址大小排序」与"第 n 个首地址 =(n−1)*6+11"的对应关系：n 是**登记序号**还是**地址号** | 第 3 条注意项要求"具体地址需要读取寄存器数据确认" | 按「**按地址升序的第 n 只**」实现，并以寄存器 11 读回的地址值交叉校验 |
| **Q-10** | 文档自相矛盾 | 空调 **30003 内盘管测量温度**：名称为"温度"、说明写「实际湿度=读数/10」 | 同一行内矛盾 | 按名称与单位列（℃，有符号，/10）配置；投运与设备显示比对确认（RC-1） |
| **Q-11** | 文档明确 | 空调 **30002 柜外测量温度 / 30005 柜外测量湿度** 标注「预留寄存器」 | 点表说明列 | **不配块**；柜外温湿度只能由 FC02 告警位间接反映（§9.5.5B）；不得在界面以数值展示 |
| **Q-12** | 文档未明确 | ADL400 的 **RS485 出厂地址**（表 7 只给范围 1–254，未给默认值）；**线制**（3P4L/3P3L）与 **DIR 电流方向** 的出厂值 | 表 7 未给默认值 | 配置按 `slave: 1` + 现场校准；投运前用表 7 菜单核对线制（三相三线**不分相功率与 PF**，不得按四线口径解读，文档 §8.2 注 2） |
| **Q-13** | 需评审 | **ADL400 点表是否也适用于 `grid_meter`（关口/台区总表）** | 文档标题含「关口表 储能表」 | 本轮**只用于 `meter_batt`**；`grid_meter` 点表变更 = 策略 phase 真源变更，须**单独立项评审** |
| **Q-14** | 文档与实现事实冲突 | 空调**出厂为偶校验**，而 southd 打开串口固定 **8N1（无校验）**（`rs485-plugin::Config::default()` 硬编码 data_bits=8/stop_bits=1/parity=None，且 `StationConf` **无校验位字段**） | 空调协议 §1；`rs485-plugin/src/config.rs`、`mupc-southd/src/port_runtime.rs` | **待评审裁定（D-1）** |
| **Q-15** | **文档未明确｜【投产阻塞·现场裁定，不阻塞设计】** | PCS 的 **A1/B1（大屏远程监控口）与 A2/B2（EMS 口）是否为相互隔离的独立接口** | §串口配置说明只给接线端子 | **投产阻塞项，但不是设计阻塞项**：由现场 **RC-8** 确认两接口是否电气隔离；若为同一总线 → **`pcs` 站不得启用**（改由 intercore 复用其既有读路径），其余 5 站与全部点表**不受影响**。 |
| **Q-16** | 文档未明确 | BMS 4000–4005「簇累计充/放电容量」标注为 `4000~4001`（跨 2 寄存器）但精度标 `1/AH` | 点表 4000–4005 行 | 按 **16 位单寄存器** 配置（`bms_cap` 只声明 4000/4002/4004/4005），投运比对量程；**若实际为 32 位**，则 4001/4003 是高字 → 须改 `format: int32_scaled`/`count: 2` 并**重排点位**（等于改名，须登记） |
| **Q-17** | 文档未明确 | 空调 FC02 位 25（10026）标「保留」；BMS 299 位「簇从控 DI 告警状态（风扇/气溶胶/MSD）」为聚合位，其内部拆分未展开 | 空调点表；BMS 离散输入表 | 保留位不产语义；BMS 299 仅采聚合位（1=有告警），拆分待厂方补充 |
| **Q-18**（v1.3 新增，回应 C-2） | 文档自相矛盾 | **BMS 位置编号"位定义"（5.4，适用于 124/126/128/130）**：原文把 Bit15–8 与 Bit7–0 **都**标为"PACK 编号（1 到 32）/ PACK 编号（1 到 N）"，**低字节语义自相矛盾**（按"簇最高单体电压对应点"的语义，低字节应为 **PACK 内单体编号**） | 协议 5.4「位置编号"位定义"说明」 | **本轮整字采集、不拆点**（§9.4.2.3）；`bms_io_25/27/29/31` 只落原始位域值，**不得据此判"哪个单体"**；待厂方确认后再引入字节拆分（需同时启用 G-6，见 D-2） |
| **Q-19**（v1.3 新增） | 拓扑归属未明确 → **已关闭/撤销（2026-09-22）** | **消防主机、空调是否同时挂在储能侧（BMS）的 485 总线上**：BMS 协议 §3.2 存在「485-2 通讯失联故障」告警位（482），且 CAN 协议 §2.1 图 2-1-1 把「消防主机 / 空调」画在总控模块侧并以 RS485/RS232 连接；而本 PRD 的拓扑是 MUPC 直连（`ttyS6` 消防、`ttyS3` 空调） | BMS 协议离散输入 482；CAN 协议图 2-1-1 | **不改变本轮拓扑**（BECG §12.1 接线契约为准），但**投运前须现场核对**：若空调/消防实际并接在 BMS 的 485-2 总线上，则 MUPC 的 `ttyS3`/`ttyS6` 现场接线会形成**双 master 共总线** → 按 §9.3.1 的"禁双 master 共总线"约束**该站不得启用**，须改由储能侧转发或重新布线。<!-- 以上为原登记，按"保留历史"未删；关闭结论见下 --><br>**【已关闭/撤销（2026-09-22）】—— 该风险不成立，本项撤销**，不再作为投运前核对项。<br>**依据（硬件接线拓扑）**：`hw/微信图片_20260908170935_64_1061_修正.png`（**BECG-3568 接线拓扑图**；BECG-3568 即 **EMS 的硬件载体**）逐口标注为 `RS485-1 → PCS`、`RS485-2 → BMS`、`RS485-3 → **空调**`、`RS485-4 → 关口表`、`RS485-5 → 储能表`、`RS485-6 → **消防**` ⇒ **消防与空调都直连 BECG-3568（EMS）自身**，**没有**挂在 BMS 的 485 总线上 ⇒ **不存在"双 master 共总线"**，`ttyS3`/`ttyS6` **照常启用**。该拓扑与规格书的串口定义（各站独占一口）互为佐证，统一登记于 **§9.3.3「设备 ↔ 端口映射（硬件接线契约）」**。<br>**原疑问的来源（如实登记，避免后人重蹈）**：<br>① 它源自 **`BMS 对 PCS` 的 CAN2.0 协议文档中的图 2-1-1「总控模块连接PCS设备」** —— 该图是**储能厂家（华塑）以 SCU 总控模块为中心**绘制的**自有系统视图**，把「消防主机 / 空调」画在总控模块一侧并以 RS485/RS232 连接；**该图不是本项目的接线拓扑**（本项目以 **EMS/BECG-3568** 为中心），此前据此产生的担忧属**误读**。<br>② 另一方面，BMS 协议离散输入位 **482「485-2 通讯失联故障」只说明 BMS 确有第二个 485 口**，**并不能推出消防/空调挂在其上**（该位与"谁挂在该口"无逻辑蕴含关系）。<br>**拓扑图订正说明**：原图 `hw/微信图片_20260908170935_64_1061.png` 的**后两个串口框误标为 `RS485-4`**（与关口表重复），已在 `hw/微信图片_20260908170935_64_1061_修正.png` 中改为 **`RS485-5` / `RS485-6`**；**原件保留未动，作为原始证据**（§9.3.3 亦记此项）。 |
| **Q-20**（**v1.5 新增，本轮核心订正项**） | **本 PRD 的推断（非厂方明文）｜【非阻塞·投运前核对，但含投运首日必做动作】** | **BMS 输入寄存器表（§5.3）类型列写作 `UNIT`，而该文档 §3.1.1「数据类型」表并不定义 `UNIT`** —— 该表只给 `BYTE`(8bit 无符号)、**`UINT`(16bit 无符号 0~65535)**、**`INT`(16bit 有符号 −32768~+32767)**、`FLOAT`、`UDINT`、`DINT`、`8BCD`、`16chString`、`32chString`、`BOOL`。**本 PRD 判定 `UNIT` 为 `UINT` 的拼写颠倒笔误，据此把 116/117/122/127/129/155/157/186、2991–2994 等全部 16 位量声明为 `uint16`**；32 位电量（139–150/4000–4005 区）原文标 `UNIT32`（同族笔误），PRD 以 `int32_scaled` 表达 | **① 计数事实**（**整词口径** —— `UNIT` 是 `UNIT32` 的子串，两者须分开计）：§5.3 输入寄存器表的类型列写作 `UNIT` 族拼写 **共 110 处 = 整词 `UNIT` 104 处 + `UNIT32` 6 处**（后者即 32 位电量行 139/141/143–144/145–146/147–148/149–150，与 `UNIT` 同族笔误）；**同一文档 §5.2 保持寄存器表用正确拼写 `UINT`，整词 164 处**；`INT` 全文仅出现 **1 次（即 §3.1.1 类型表自身），在点表中从未出现**。<br>**② 强反证（决定本判定成立）**：**同一文档的 §5.2 保持寄存器表用正确拼写 `UINT`，并且同样大量携带负偏移**（如「`UINT` … 精度：0.1 **偏移：-50℃**」「`UINT` … 精度：0.1 **偏移量：-3000A**，充电为正，放电为负」）⇒ **厂方自己的主力做法就是「无符号编码 + 负偏移做零点平移」**，可见**负偏移不能推出有符号**。v1.3/v1.4 的"负偏移 ⇒ 必须 `int16`"据此**撤销**。<br>**③ 已知代价**：实际工况电流远小于该量程（±1600 A），故 `uint16`/`int16` 在常用 raw 区间（≈15000–48000）**数值等价**；二者只在 raw ≥ 32768 处分歧（65535 → +4953.5 A vs −1600.1 A）。 | **① 本轮落地**：全部按 `uint16` 配置，`offset` 照录原文负值；§9.4.3 的"`uint16` + 非零 `offset` → 拒"规则**已删除**（见 §9.4.2.4 ⑥、§9.4.3）。<br>**② 投运首日必做动作（现场判据）**：以**充放电工况**核对 **116（簇组电流）与 186（簇实时充放电功率）** —— 若在"小电流放电/小功率放电"时读数表现为**数万量级的大正值**（电流 >3 万 A 或功率 >5 万 kW 量级），或原始 raw 落在 **32768–65535**，则**厂方实为有符号** ⇒ 该点 `format` **改声明 `int16`** 并按变更流程登记（`format` 变更**不改变点名**，因点名锚定地址而非 format）。<br>**③ 向厂方确认（书面）**：请厂方确认 §5.3 的 `UNIT` 是否为 `UINT` 笔误、以及 116/186 是否按补码下发负值。**厂方书面明确后本项转为"文档明确"并移除推断标记。**<br>**④ 影响面**：仅 BMS 站 `format` 声明，**不改架构、不改点名、不改块划分、不改带宽**（`uint16`/`int16` 均为 1 寄存器）。 |
| **D-1** | **已裁定（2026-09-22，用户确认按 ①）**（原类别：**待评审裁定**） | 空调串口校验位：① 新增 per-station `parity` 配置字段（`none` 缺省 / `even` / `odd`，同口一致性校验同 `baud_rate`），空调站配 `even`；② 不加字段，要求现场把空调参数 40016 置 0（无校验） | — | **原处理口径（保留未删，供追溯"曾有二选一"）**：**必须二选一后实施**；**在裁定前空调站不得投产**。推荐 ①（不依赖现场操作、可复现、可审计；且 `parity` 一旦成为口级参数，同口多站时必须一致，校验须一并加）。<!-- 以上为裁定前原登记，按"保留历史"未删；裁定结论见下 --><br>**【已裁定（2026-09-22，用户确认）】采用方案 ①，方案 ② 被否**：<br>**① 采用 ①** —— 新增站级 `parity` 配置字段（`none` 缺省 / `even` / `odd`），**空调站配 `even`**。<br>**② 同口 `parity` 须一致** —— 与 `baud_rate` 同级、同一条校验规则（**同口共享物理校验位，配置不一致会被静默忽略**；该一致性校验已在段内 `validate()` 实现，规则 16，约 308–327 行）。<br>**③ 方案 ② 被否** —— 不采用"要求现场把空调参数 40016 置 0"：依赖现场操作、不可复现、不可审计。<br>**④ 空调站投产限制解除** —— 原"**在裁定前空调站不得投产**"**不再适用**，空调站可与其余站点一同投产；真机验收项 **§9.9.2 RC-6**（按本裁定完成串口参数落地并实测通信正常）**仍须做**。<br>**⑤ 实现已就位（2026-09-22 回代码核实）** —— `StationParity{none,even,odd}` + 站级 `parity` 字段 + 规则 16 的 `parity` 同口一致性校验（`mupc-southd/src/config.rs`）；空调站在 §9.4.1 参考配置、`mupc/deploy/config/mupc_core_config{,.production}.yaml`、`tests/fixtures/south_stations_s3b2.yaml` 中**均为 `parity: even`**；`parity` **已透传至串口**（T5：`port_runtime::bus_config()` 三值逐一对映 `rs485_plugin::Parity`）。<br>**⑥ 连带项** —— 终审记录曾登记"若裁定为 ② 则 §9.4.3 的同口 `parity` 一致性规则与 AC-1 ③ 对应触发项**失去对象、须同步回写**"；**因裁定为 ①，该回写动作不适用**，上述规则与 AC-1 ③ 触发项**均保留有效**（详见顶部 `[§9 增补 v1.11]` 登记 3）。 |
| **D-2** | 已裁定（评审 2026-09-21）+ 本轮范围界定 | **§9.1.1 的 G-1…G-6 属对 `mupc-southd` / `data-processing` 的解码与读能力扩展（代码变更），已超出"填点表"的字面范围** | 评审意见 B 段：「G-1…G-6 属本 PRD 范围（是点表可表达/可读的前提，拆出则本轮不可上线）」 | **本轮范围 = G-1…G-5**（16 位格式 / `offset` / FC02 / 单寄存器与多值块 / 字节序字序开关），并在 G-4 上扩展**逐点换算参数**（§9.4.2，B-1 裁定的落地形态）。**G-6（字节拆分）本轮不做**，理由与替代口径见 §9.1.1 的 G-6 说明（两个候选点分别因"原文自相矛盾"与"重复结构导致配置/带宽爆炸"而不宜拆），替代口径为**整字采集原值 + 展示/事件层拆解**。<br>**重启条件（满足其一即须重新评审）**：① 厂方书面确认 124/126/128/130 的低字节语义（解 Q-18）；② 消防探测器烟雾/温度模拟量出现明确消费方（事件/策略/报表）—— 届时须同时给出"重复结构的配置展开规则"与带宽重算（§9.8.1）。 |
| **D-3** | 待评审裁定 | BMS **写侧**（504/524/525/526 + 阈值参数）是否纳入已确认的写 Task 范围（用户已确认的三项为消防远程控制、空调设定与使能位、PCS 4 区） | — | 请评审确认；本 PRD 已按"登记但不纳入"处理 |

### 9.11 与既有文档/设计的一致性声明

| 关联文档 | 关系 |
|----------|------|
| `plans/modules/02-MUPC-南向通信-设计文档.md` §10（`[DESIGN_APPROVED: 2026-09-08]`） | 本 PRD 是其 §10.9「设备点表待厂方提供后填配置」的落实；**新增 G-1…G-5 能力**（含 G-4 的"**读窗口 + 逐点换算参数**"配置结构、点位命名规则）与 `Role::Pcs` 需在设计文档 §10 追加/修订对应条款后方可实施（设计侧变更由架构师负责）。设计文档须至少明确：① `RegBlockConf` 的 `points` 结构与校验；② `StationConf` 的 `parity` 字段（D-1 裁定 ① 时）；③ 位块点的落库口径（§9.8.1 末条） |
| `plans/archive/2026-09-08-S3a-站级南向调度-框架与总表收敛.md` | 范围声明已写明「语义点表待厂方 → S3b」，本 PRD 为 S3b-2 |
| `specs/modules/10-MUPC-核间通信-PRD.md` §2.4 | PCS 的协议事实（19200 N-8-1、字节互换、FC06 逐写、3 区 1010/1013）与本文 §9.5.2 同源；**数据面边界 ADR-012 继承**（PCS 输出功率仅作健康/校验） |
| `plans/archive/2026-09-08-S2-DI-DO安全联锁.md` | PCS 4 区 **500=停机原语**已由 S2 占用 → 本 PRD §9.2.3 明确 PCS 写操作**本轮范围外** |
| `CLAUDE.md`（项目约束） | 全部点表来自厂方文档实证；无硬编码密钥；无新增 `unsafe`；错误类型实现 `std::error::Error`（§8.3） |

---

## 10. 块级采集周期覆盖（告警位单独快采，S3b-3）

> **本章来源**：用户 2026-09-23 就 **12 号模块 R-33** 作出的裁定 **「告警位单独快采」** —— 同一站内，「告警位（FC02 位块）」与「标量测量值」使用**不同的采集周期**。本章为**新增章节**，不改动 §1–§9 的任何既有裁定（§9 的 `[REVIEWED: PASS: 2026-09-21]` 保持不动）。

### 10.1 背景与目标（Why）

| 项 | 事实（回源） |
|----|--------------|
| **现状能力** | 采集周期是**站级单值**：`south_stations.stations[].interval_ms`（`mupc-southd/src/config.rs:76-77`）；调度器 `DueCalc` 的到期条目**每站一条**、按站计算 `next_due`（`mupc-southd/src/scheduler.rs:107-175`，到期判定 `:144-163`） |
| **⇒ 后果** | **同一站内的块（`regs[]` 元素）无法各自声明周期** —— 站内全部块在每个站轮里被一次性读齐（`poll_station` 的逐块读循环，`scheduler.rs:570-603`） |
| **需求冲突（12 号）** | 12 号 PRD **F25.3** 要求「离散告警位在**变化后 ≤ 2 s** 上屏」；而 HVAC 站现网周期 **5000 ms**（`mupc/deploy/config/mupc_core_config.production.yaml:399`；本 PRD §9.4.1 同值）⇒ 一个位的跳变**最早**也要 5 s 后才被采到 ⇒ **「站周期 5 s」与「告警位 ≤2 s」不可同时成立**（12 号设计 §15.6.1 约束 2 已如实登记该物理不可达，并登记依赖项 **R-45**） |
| **目标** | 给南向采集层补上**块级（分组级）采集周期覆盖**能力，使「告警位快采、标量维持站周期」在**同一站内**可配置、可审计、可验收；**既有配置与既有行为零变化** |

**不做什么（范围自限）**：本章**不**改变站离线的判定阈值与语义（`stale_timeout_s` 属既有机制，§9.7.1 零改动）、**不**新增事件名、**不**改变任何 telemetry 命名与落库口径、**不**触碰 `mapper` 的判据本体。

### 10.2 范围

#### 10.2.1 范围内（本章交付）

1. 块级可选字段 **`interval_ms`**（§10.3.1）与取值约束组 **C1–C9**（§10.3.2）；
2. 站内**分组语义**（分组键 = 块的**有效周期**，§10.3.3）与**站级承载组**规则（§10.3.3 规则 C6/C7/C8 的配套）；
3. **总线负载上界**的可测口径（§10.5）；**边界与异常**逐条（§10.6）；
4. 验收标准 **AC-8-1…AC-8-8**（§10.7）；与 12 号 F25 / 01 号的关系与边界声明（§10.8）。

#### 10.2.2 范围外（本章不做，逐条给出理由）

| 不做项 | 理由 |
|--------|------|
| **站离线判定提速**（改 `stale_timeout_s` 或新造第二套离线判据） | 12 号 F25.1 明令「沿用 F5 基线，**不新造第二套新鲜度口径**」；且 `stale_timeout_s` 是**阈值语义**，与轮询节奏无关（§10.8 登记 3） |
| **块级写操作 / 写周期** | 写 Task 独立立项（§9.2.3），本章只覆盖**只读采集** |
| **跨站的"组"**（同口多站共用一个采集组） | 无需求来源；同口多站已由既有 `next_due` + `role_priority` 承载（§9.7.7、设计 §10.2） |
| **新增事件名（如"块离线"）** | 会破 01/03/12 号既有事件消费契约（`events` 表与 SSE 消费方按既有 metric 集合解析）；块级失败的可观测性由 §10.6 的 `warn` 日志承担，其**点位陈旧可见性**缺口如实登记为 Q-23 |
| **`fire` 站与 `meter_grid` 站的判据块提速** | 会破坏站级语义判据的完整性（规则 C6/C7）或静默改变控制链节奏。**依据 = §10.3.2 的 `R(role)` 定义表 + §10.4 末段"可提速一览"表**（`meter_grid`/`fire` 的 `R` 覆盖其后全部块 ⇒ 无可提速块）；**不引 Q-22**（Q-22 只谈 `fire` 站 n>20 的"探测器块独立降频"，与"提速"是**反方向**的需求） |

### 10.3 能力定义与配置契约

#### 10.3.1 块级字段 `interval_ms`（新增，可选）

**字段表（需求级语义）**：

| 字段 | 层级 | 类型 | 必填 | 缺省 | 单位 | 语义 |
|------|------|------|------|------|------|------|
| `interval_ms` | **块级**（`regs[]` 元素） | 非负整数 | **否** | **缺省 = 继承站级 `interval_ms`** | ms | 该块的**目标采集周期**。声明即"本块按此周期被轮询"；缺省即"本块随站周期" |

**两条定性质（本章的核心承诺）**：

1. **缺省 ⇒ 零行为变化**：任何不声明块级 `interval_ms` 的配置，其调度行为（每轮读事务序列、上送点、事件序列、退避时延）与本章落地前**逐条相同**（§10.7 **AC-8-2** 是可机械验证的判据）。
2. **块级只表达"节奏"，不表达"顺序"**：块的**读序**仍按 `regs` 书写序（同组内），跨组的次序由调度器按**角色优先级 + 站序 + 组锚**确定（设计 §12.4）；本字段**不得**被用作"块优先级"的声明手段。

#### 10.3.2 取值约束组（C1–C9，**配置期全部可机械判定**）

> **符号**：`S` = 本站站级 `interval_ms`；`P` = `south_stations.poll_ms`（段级字段，现网 **1000 ms**，`mupc_core_config.production.yaml:143`）；`T_组` = 该组的单轮耗时估算值（按 **§9.8.1 的估时公式**用**本站 `baud_rate`** 复算：1 字节 = `10/baud_rate` 秒；事务字节数 = 请求 8 + 响应 `(5 + 2N)`（FC02 时 `N = ceil(位数/8)` 字节）；每事务另加 4 ms 周转）；`R(role)` = 该 role 的**站级语义判据所需块集合**（定义见下）。

| # | 约束 | 判据 | 违反处置 | 依据 |
|---|------|------|----------|------|
| **C1** | 正数 | `interval_ms > 0` | `Err` | 同站级 `interval_ms > 0`（`config.rs:227-232`） |
| **C2** | **绝对下界 500 ms** | `interval_ms ≥ 500` | `Err` | 与既有 **`PCS_MIN_INTERVAL_MS = 500`**（`config.rs:27`，PRD §9.3.2.2(2) + §9.8.1 末条）**同源同值**：项目内"最快允许轮询节奏"的唯一先例；且 500 ms 恰为"屏侧链路也 ≤2 s"的最快档（§10.8），故不另造数字 |
| **C3** | **tick 网格对齐** | `interval_ms % P == 0` | `Err` | 调度器以 `P` 为 tick 粒度（`scheduler.rs:492-507`，每轮 `sleep(P)`）；非整数倍会被网格**向上量化**（如 `P=1000` 时声明 1500 实际为 2000）⇒ 配置值名不副实，必须拒绝而非静默量化 |
| **C4** | **相对下界（按耗时）** | `interval_ms ≥ 1.5 × T_组` | `Err` | 沿用 §9.8.1 末条既有规则「任一站的 `interval_ms` 不得小于该站单轮耗时的 1.5 倍（防上一轮未完下一轮即到期造成积压）」的**块级对偶** |
| **C5** | **上界 = 站级周期（只提速、不降速）** | `interval_ms ≤ S` | `Err` | ① 站级 `interval_ms` 是本站**节奏真源**，"降速"需求应由站级表达；② 若允许块周期 > 站周期，则为 `meter_grid`/`battery` 的"`interval_ms < 5000` 新鲜度上界"（`config.rs:249-260`）开了**旁路**（把判据块降到 10 s 即绕过配置期拦截） |
| **C6** | **判据完整性：`R(role)` 内块的有效周期须全相同** | 对 `b ∈ R(role)`：`eff(b)` 全部相等 | `Err` | 站级语义判据按**一次 poll 的读集**求值（`mapper::poll_to_result`，`mapper.rs:407-464`；`round_signals` 的 `StationFlag` 判据，`scheduler.rs:345-380`）。若判据所需块被拆到不同组，则**每次求值都只看得到部分块** ⇒ ① `MeterGrid` 五相量块缺任一即 `Failed`（`mapper.rs:410-429`）⇒ **整站恒 offline**；② `Fire` 的登记数交叉校验在缺块时会**误报**（见 §10.9 Q-22 的复算） |
| **C7** | **判据组不得提速** | 对 `b ∈ R(role)`：`eff(b) ≥ S`（配合 C5 ⇒ `eff(b) == S`） | `Err` | `R(role)` 承载**控制链输入**（`meter_grid` 的 phase 真源、`battery` 的 SOC）与其**闸门/推送节奏**（`on_grid_package` / `on_battery_soc`）；让判据块提速会**静默改变控制 cadence**，属跨模块影响。**本轮明确不做**，其登记位置有三处（**不再引用任何"Q-21 ②"** —— Q-21 全文只有位块周期的三档取值与 `grid_meter` 补算附项，**无"允许 R 提速"的备选口径**）：① §10.2.2 **范围外**表的「判据块提速」行（理由：破坏判据完整性或静默改变控制链节奏）；② §10.3.2 的 **`R(role)` 定义表**（判据块的唯一权威口径）；③ §10.4 末的"**可提速一览**"表（`meter_grid`/`fire` ⇒ **无可提速块**）。**若要为判据块提速，须由需求侧另立条目**（涉及 04 策略引擎的控制 cadence，非本模块可单方决定） |
| **C8** | **站级承载组按"最大组周期"确定**（确定性，不设拒绝） | 站级承载组 = **组周期最大**的组（并列时取**锚块下标最小**者） | —（不拒绝） | `offline`/`online` 状态事件、`offline_count` 与站级退避须有**唯一承载者**（设计 §12.5）；取最大周期组 ⇒ **站离线判定时延与现状一致**（不受快组影响） |
| **C9** | **同口总线负载上界** | `U_口 = Σ_组 (T_组 / 组周期) ≤ 0.5` **且** 每组满足 C4 | `Err` | §10.5；`Err` 口径同理 `PCS_MIN_INTERVAL_MS` 的立意"防误配的超短周期打满总线" |

**`R(role)` 的定义（判据所需块集合，本章唯一权威口径）**：

| role | `R(role)` | 依据（回代码） |
|------|-----------|----------------|
| `meter_grid` | `{p, q, pf, u, i, p_total}` | `mapper::poll_to_result` 的 MeterGrid 分支按**块名**取 p/q/pf/u/i（缺任一 → `Failed`，`mapper.rs:409-432`）；`p_total` 供 `scalar_total` 降级求和 |
| `battery` | 承载 `soc` 点的那一块 | `mapper.rs:437-452`（`battery_soc` 按**点名**查找；该块读失败 → 整站 `Failed`） |
| `fire` | `{覆盖寄存器 11 的**寄存器块**}（`func ≠ discrete`）∪ `{块名前缀 `fire_det` 的块}` | `mapper::fire_detector_addr_order_violation`（`mapper.rs:565`，经 `fire_chain_head` `:326` 取链首）与 `mapper::fire_detector_mismatch`（`mapper.rs:359`，容量 = `Σ(fire_det* count)/6 + 链首`）**共用**这两类输入；"寄存器块"的限定与 `fire_chain_head` 的 `res.regs()?`（位块取不到寄存器 ⇒ 不能当链首）**同判** |
| `meter_batt` / `hvac` / `pcs` | **∅**（无站级语义判据） | `mapper.rs:460-462`：三者一律 `PollResult::Data(empty_package())`，无块级判据 |

> **`R(role)` 的另一重作用（本字段的"能不能快"一览）**：由 C5+C6+C7 ⇒ **`R(role)` 的块恒处在站周期组**。故：
> - **`meter_grid` 站**（§9.4.1 的 6 块全在 R 内）⇒ **无可提速块**；
> - **`fire` 站**（§9.4.1 的 `fire_sys` + `fire_det` = R）⇒ **无可提速块**（与 §9.5.4「n>20 时**探测器块**独立降频」的字面诉求存在张力，登记为 §10.9 **Q-22** 待裁定，本章不做隐式放开）；
> - **`battery` 站**（R = `soc` 块）⇒ `bms_alarm` 位块**可**提速（但其站周期已是 1000 ms，无收益）；
> - **`meter_batt` / `pcs` / `hvac` 站**（R = ∅）⇒ 块可提速；其中**只有 `hvac`（站周期 5000 ms）有实际收益** ⇒ **本章首例 = HVAC 位块快采**。

#### 10.3.3 站内分组语义（分组键 = 块的**有效周期**）

- **有效周期** `eff(块) = 块.interval_ms ?? 站.interval_ms`；
- **同站内 `eff` 相同的块构成一个「读组」**；一个读组在**一次轮询**里被依次读齐（组内事务串行，与既有"站轮内逐块读"同构）；
- **组的到期相互独立**：每个读组各自维护 `next_due`，按**组周期**推进（设计 §12.4）；
- **组间不共享"整站原子性"**：一个读组读失败**不**使其它读组的数据被丢弃（它们本就不同轮），但**站级失败语义按 C8 的承载组判定**（§10.6 第 4 条）。

**示例（首例配置，HVAC 站）**：

```yaml
- id: hvac
  role: hvac
  port: "/dev/ttyS3"
  slave: 1
  baud_rate: 9600
  parity: even
  interval_ms: 5000            # S = 5000（站周期 = 标量温湿度的节奏）
  regs:
    - name: hvac_in            # FC04 30001–30004（温湿度）——不声明 ⇒ eff = 5000（站周期组）
      func: input
      addr: 0
      count: 4
      format: int16
      scale: 0.1
      points:
        - { at: 1 }
        - { at: 3 }
        - { at: 4, format: uint16 }
    - name: hvac_di            # FC02 位 0–30（告警位）——**块级覆盖 1000**
      func: discrete
      addr: 0
      count: 31
      interval_ms: 1000        # ← 本章唯一新增字段；C1–C5 校验：1000>0 ✓ ≥500 ✓ %1000==0 ✓ ≥1.5×22ms(33ms) ✓ ≤5000 ✓
```

⇒ 分组结果：**组 A（`hvac_di`，1000 ms）** + **组 B（`hvac_in`，5000 ms）**；站级承载组 = 组 B（周期最大）⇒ `offline`/`online` 事件与站级退避的判定时延**与现状一致**。

### 10.4 应用场景（首例）：HVAC 位块快采

| 步骤 | 前 | 后 |
|------|----|----|
| HVAC 位块采集周期 | **5000 ms**（站周期） | **1000 ms**（块级覆盖） |
| HVAC 标量（温度/湿度）采集周期 | 5000 ms | **5000 ms（不变）** |
| 「柜内高温告警」位 0→1 的**采集时延上界** | ≤ 5000 ms（实测最坏 5 s） | **≤ 1000 ms** |
| 与 12 号 F25.3（告警位 ≤2 s）的采集侧前置 | **不成立**（物理不可达） | **成立**（采集侧 ≤1 s；端到端另见 §10.8） |
| 本口总线占用率 | **≈0.95 %**（`47.52 ms / 5000 ms = 0.95040 %`，取**本表公式精确值**；见下方"占用率口径"注） | **≈2.68 %**（精确 `0.026848`；复算见下） |

**单轮耗时与负载复算（9600 bps，8N1，`/dev/ttyS3` 独占一口）** —— 估时公式与 §9.8.1 逐字相同（1 字节 = 10/9600 s = **1.04 ms**；事务字节 = 请求 8 + 响应 `(5+2N)`；每事务 +4 ms 周转）：

| 组 | 块 | func | 参数 | 事务数 | 帧字节 | **T_组** | 组周期 | **占用率** | C4 下界 `1.5×T_组` | 余量 |
|----|----|------|------|--------|--------|----------|--------|------------|--------------------|------|
| **A（快组）** | `hvac_di` | FC02 | 31 位（= 4 字节） | 1 | `8 + (5+4) = 17` | `17×1.04 + 4 =` **21.68 ms**（≈22 ms） | **1000** | **2.17 %** | 32.5 ms | **46.1×** |
| **B（站周期组）** | `hvac_in` | FC04 | 4 寄存器 | 1 | `8 + (5+8) = 21` | `21×1.04 + 4 =` **25.84 ms**（≈26 ms） | 5000 | **0.52 %** | 38.8 ms | **193.5×** |
| **合计（本站 = 本口）** | | | | 2 | **38** | **47.5 ms**（两组同刻到期的上界） | | **`U_口 = 0.02168 + 0.005168 = 0.026848` ⇒ 2.68 %** | | **C9 上界 50% 的 1/18.6** |

> **取整口径（与 §9.8.1 同款要求）**：本表的 `T` 值为**按公式复算后取整**（21.68 / 25.84 ms），`U_口` 的**精确值 = 0.026848**（= 2.6848 %，取整为 **2.68 %**）；成分列各自取整为 2.17 % / 0.52 %（**直接相加得 2.69 %，与精确值差 0.01 pp，属取整差**）。**AC-8-6 的断言以精确算式为准**。
>
> **占用率口径（统一为"公式精确值"，评审订正）**：**改造前/后一律用本表公式的精确值**，**不得**混用 §9.8.1 的**取整后** 48 ms：
>
> | 口径 | 算式 | 结果 |
> |------|------|------|
> | **改造前**（采用） | `47.52 ms / 5000 ms`（= 整站一轮 `38 × 1.04 + 2 × 4`） | `0.0095040` ⇒ **0.95 %** |
> | 改造前（**不采用**） | `48 ms / 5000 ms`（§9.8.1 的**取整值**） | `0.0096` ⇒ 0.96 % |
> | **改造后**（采用） | `0.02168 + 0.005168` | `0.026848` ⇒ **2.68 %** |
> | **倍数** | `0.026848 / 0.0095040` | **2.8248 ≈ 2.8×** |
>
> ⇒ 全文（含版本演进、02 设计 §12.6）统一为 **0.95 %**（原 §10.4 表写的 `≈0.96 %` 系与"改造后"**不同源**的取整口径，已订正）。

- **最坏同刻叠加**：两组同时到期（t = 0/5000/10000…）时的单轮耗时 = `21.68 + 25.84 = 47.5 ms` ≪ 组周期 1000 ms ⇒ **不产生积压**；且该值与**改造前**的整站单轮 `≈48 ms`（§9.8.1）**实质相同**（同一批事务，只是不再每轮都做）。
- **敏感性（位块周期的三档取值，供 §10.9 Q-21 裁定）**：

| 位块周期 | `U_口` | 是否满足 C3（`% poll_ms==0`，`poll_ms=1000`） | 12 号 F25.3 采集侧时延上界 |
|----------|--------|------------------------------|------------------------------|
| **2000 ms** | `21.68/2000 + 0.52% = ` **1.60 %** | ✓ | ≤2.0 s |
| **1000 ms（推荐）** | **2.68 %** | ✓ | ≤1.0 s |
| **500 ms** | `21.68/500 + 0.52% = ` **4.85 %** | ✗（须把段级 `poll_ms` 1000 → 500，**全局 tick 加倍**） | ≤0.5 s |

> **推荐 1000 ms**：与 `battery`/`meter_batt`/`fire` 三站的现网周期（各 1000 ms）**同档**，使 HVAC 从"5 s 特例"回到与其它外设站一致的节奏；且**无需改动段级 `poll_ms`**（零额外配置面）。**500 ms 档**的收益是将端到端（含屏侧链路）也压到 ≤2 s（§10.8），代价是全局 tick 加倍；取舍登记为 **Q-21**。

**为何本轮只有 HVAC 有收益（可复算的"收益一览"）**：

| 站 | 站周期 | `R(role)` 是否为空 | 可否有快组 | 现状是否已达 ≤1 s 采集 | **是否有收益** |
|----|--------|--------------------|------------|------------------------|----------------|
| `hvac` | **5000 ms** | ∅ | ✓ | ✗（5 s） | **✓ 唯一有收益** |
| `battery` | 1000 ms | `{soc 块}` | ✓（`bms_alarm`） | ✓（已 1 s） | ✗（无收益） |
| `fire` | 1000 ms（n≤20） | `{fire_sys, fire_det}` | ✗（C6/C7） | ✓（已 1 s） | ✗ （n>20 的例外见 Q-22） |
| `meter_batt` | 1000 ms | ∅ | ✓ | ✓ | ✗（无收益） |
| `pcs` | 1000 ms | ∅ | ✓ | ✓ | ✗（无收益） |
| `meter_grid` | 1000 ms | `{p,q,pf,u,i,p_total}` | ✗（C6/C7） | ✓ | ✗（且 C7 明确禁止判据块提速） |

### 10.5 总线负载约束（同口多站共享带宽）

**要求**（同口多站或同站多组共享带宽 ⇒ 提频后须有明确上界）：

| 项 | 要求 | 可测口径 |
|----|------|----------|
| **单组不积压** | 任一组 `组周期 ≥ 1.5 × T_组` | 配置期按 §9.8.1 公式复算（C4）；运行期由"下一轮到期时上一轮已结束"保证 |
| **口占用上界** | `U_口 = Σ_组 (T_组 / 组周期) ≤ 0.5` | **配置期可计算**（C9，落点为配置期 `Err`）；留 ≥2× 余量以吸收抖动、重试与设备响应变慢 |
| **单轮最坏耗时** | 同口全部到期组串行执行，最坏 `Σ T_组` 须 ≤ 最小非零组周期的 1.5 倍 | 同上 |
| **退避隔离** | 失败组降频**不得**拖累同口其它组/站的 cadence | 沿用 §9.7.7 与设计 §10.2 的 M-11 口调度预算：优先级 `meter_grid`/`battery` > `meter_batt`/`pcs` > `hvac`/`fire`（`scheduler.rs:98-104`），退避按**组周期**指数增长、封顶 32×（`backoff_extra`，`scheduler.rs:90-93`） |

**现网复核（证明新增的 C9 不会拒掉现有配置）** —— 按 `T/周期` 逐站复算（`T_组` 取 §9.8.1 表值）：

| 口 | 站 | T（§9.8.1） | 站周期 | `U` 单站占用 | 本站独占一口 ⇒ `U_口` = 该值 | C9（≤0.5） |
|----|----|-------------|--------|--------------|------------------------------|------------|
| `ttyS2` | `battery` | ≈286 ms | 1000 | 0.286 | 0.286 | ✓ |
| `ttyS5` | `meter_batt` | ≈310 ms | 1000 | 0.310 | 0.310 | ✓（最大者） |
| `ttyS6` | `fire` | ≈300 ms | 1000 | 0.300 | 0.300 | ✓ |
| `ttyS7` | `pcs` | ≈90 ms | 1000 | 0.090 | 0.090 | ✓ |
| `ttyS3` | `hvac`（**首例生效后**） | 快组 ≈21.7 + 慢组 ≈25.8 | 1000 / 5000 | — | **0.0268** | ✓ |
| `ttyS4` | `grid_meter` | §9.8.1 标"既有"（未给值） | 1000 | — | **待补**（§10.9 Q-21 ③：本 PRD 未复算 `grid_meter`；按同公式补算后再纳断言） | 待补 |

⇒ 现网 6 站**全部通过** C9（最大 0.310 < 0.5），**C9 不构成对既有配置的新增拒绝**；它只在"误配的超短周期"（`组周期 ≈ T_组`）时才触发。

### 10.6 边界与异常（逐条）

| # | 情形 | 要求的行为 |
|---|------|------------|
| **1** | **块周期非法** | `interval_ms` 为 `0`（C1）、`< 500`（C2）、非 `poll_ms` 整数倍（C3）、`< 1.5×T_组`（C4）⇒ **配置期 `Err`**，错误文案须含**站 id + 块名 + 实际取值 + 期望区间**（沿用既有"错误消息即定位信息"风格）。**不得**静默夹取到最近合法值 |
| **2** | **块周期 > 站周期** | ⇒ **配置期 `Err`**（C5）。理由：① 站级是节奏真源；② 若放行，会为 `meter_grid`/`battery` 的 `interval_ms < 5000` 开旁路。**降速诉求的正当出口 = 下调站级 `interval_ms`**（既有校验随之生效） |
| **3** | **快慢块共口竞争** | 同一口内：到期组按 `(角色优先级, 站序, 组锚)` **串行**执行；要求满足 §10.5 的 C4/C9 两条。**不得**为快组开辟并发通道（同口单 poller 是既有硬约束：`Rs485PortBus` 的 per-port async Mutex，设计 §10.2 / `scheduler.rs:1-5` 模块头） |
| **4** | **站离线时（口未打开 / 读失败）** | ① **站级 `offline`/`online` 事件、`offline_count` 与站级退避的唯一真源 = 站级承载组**（C8：周期最大的组）——该组失败 ⇒ 按既有语义产 `offline` 事件（含 `reason`，文案含 `slave/addr/count`，§9.7.2 第 1 条）、`offline_count` +1、按组周期**指数退避**（封顶 32×）；② **块级（快采）组失败** ⇒ **不产站级 `offline` 事件、不自增 `offline_count`**，只①按**自身组周期**指数退避（组级计数）、②`warn` 日志（含 `station/role/块名/reason`）、③本轮该组无 telemetry/事件上送。**理由（不得违反）**：若块级失败也升级为站级 offline，则 12 号 F25.4 的屏侧行为会把该站**全部**点显示 `--` + 「站离线」，连同**正在正常上送的快采告警位**一起隐藏 —— 与本能力的诉求（告警位 ≤2 s 可见）**直接冲突**；且"快组成败"与"慢组成败"交替会使 `online`/`offline` 事件对**周期性刷屏**（`mark_success` 会清空 offline 去抖窗口 `last_offline_event`，`scheduler.rs:737`，⇒ 每对约 1 次/承载组周期） |
| **5** | **站恢复** | 站级承载组恢复 ⇒ 既有 `online` 事件一次 + `offline_count` 归零；且**该站全部组的变化沿基线一并重建**（恢复后现势值连续性不可假设，§11.4.7 既有口径的**组级推广**：不得只重建恢复那一组的基线） |
| **6** | **启动首轮** | 每个组**各自**在首次读到数据时建立全量快照（位点全量落库、事件只建基线不产）；**不得**要求"全站所有组都读过一轮"才允许交付（否则快采块的首帧会被最慢组拖到 S 之后） |
| **7** | **段级 `poll_ms` 与组周期不匹配** | 见 C3：配置期拒绝。**不**提供"运行时自动降级为 `poll_ms`"的容错（静默降级会让"配置写的 500 ms"实际跑成 1000 ms） |
| **8** | **站内全部块都声明了 `interval_ms`** | **不拒绝**（C8）：站级承载组 = 周期最大的组。但此时站级 `interval_ms` 已不再描述该站的实际节奏 ⇒ 属**配置异味**，由 `debug` 日志提示（**不产事件、不拒配置**） |

### 10.7 验收标准（增量，`AC-8-*`）

> 编号接续 §9.9 的 `AC-1…AC-7`；`AC-8-*` 为本章的**新增**验收项（**不修改** §9.9.1 的任何既有条目）。
>
> **⭐ 表内为 AC-8-1…AC-8-7（7 条可执行验收）；AC-8-8 已移出本表**（评审建议 c）：原 **AC-8-8「端到端边界声明（不得越界断言）」** 属**元要求**（"**不得**断言什么"），**不可执行验证** ⇒ 移入 **§10.8.1 的边界声明第 3 条**。**编号保留不回收**（避免既有引用失效），但**不再作为验收项**、**不计入用例映射**（02 设计 §12.8）。

| ID | 验收条件（**逐条可测**） |
|----|--------------------------|
| **AC-8-1** | **位块按独立周期被轮询**（可观测判据 ①）：以首例配置（`hvac.interval_ms = 5000`、`hvac_di.interval_ms = 1000`、`hvac_in` 不声明）与**确定性 tick 序列** `t = 0,1000,2000,…,9000`（共 10 tick）驱动：**`hvac_di` 的 FC02 读事务次数 = 10**；**`hvac_in` 的 FC04 读事务次数 = 2**（t=0 与 t=5000 各一次；t=10000 不在序列内）。比率 `10/2 = 5 = 5000/1000` 与配置一致（计数可经 `MockBus` 的读计数或真机串口抓包/`debug` 日志复现） |
| **AC-8-2** | **标量仍按站周期**（可观测判据 ②）：同一 tick 序列下，`hvac_in` 的 3 个标量点（`hvac_in_1`/`hvac_in_3`/`hvac_in_4`）**上送次数 = 2**（与站周期 5000 ms 一致），**且不受位块提速影响**（不得变成 10 次） |
| **AC-8-3** | **零回归（缺省即无变化）—— 钉 tick 序列与计数**：取 §10.3.3 的首例配置**去掉** `hvac_di.interval_ms`（⇒ 单组站、`eff` 全 = 5000），以与 AC-8-1 **同一确定性 tick 序列** `t = 0,1000,…,9000`（10 tick）驱动，逐项断言：① 二块的读事务次数**各 = 2**（单组每 5000 ms 到期一次）；② `on_station_telemetry` 调用次数 = **2**；③ 第 1 次调用项数 = **34**（3 标量 + 31 位，**首轮全量快照**）、第 2 次 = **3**（标量全量 + 无变化位）；④ 事件数 = **0**（首轮只建基线）。上述四项须与本章落地前**逐条相同**（以既有 `scheduler.rs` 的调度/退避/隔离/分发用例断言"一个字节都不动"为回归锚；既有用例总数 = **44**：`#[tokio::test]` 38 + `#[test]` 6） |
| **AC-8-4** | **位变化事件在块周期内产出**：`hvac_di` 的位 10（`hvac_di_11` 柜内高温告警）由 0→1 后，事件在**下一次位组到期轮**内产出（≤ 1000 ms + 单轮耗时）；且该事件经 `on_station_telemetry(.., is_event = true)` 上送（沿用既有事件通道，**无新增 metric**） |
| **AC-8-5** | **非法值逐条拒绝（含 C4 的可构造用例）**：`interval_ms: 0`（C1）、`300`（C2）、`1500`（C3，`poll_ms = 1000`）、`interval_ms > 站级`（C5）、`R(role)` 内块异周期（C6）、`R(role)` 内块周期 < 站周期（C7）、**C4 一例** ⇒ **均返回 `Err`**，且文案含站 id 与块名。<br>**C4 的构造法（现网 `T_组 ≤ 310 ms` ⇒ `1.5×T_组 ≤ 465 ms < C2 下界 500` ⇒ C4 被 C2 完全吸收、无法自然触发，须专门构造）**：造一个 `T_组 > 333.3 ms` 的组 —— 独立站（9600 bps、`poll_ms = 1000`、站 `interval_ms: 2000`）内同组放 **3 个 FC04 块**（各 `count: 120`）⇒ 单事务帧字节 `8 + (5 + 240) = 253` ⇒ `T_块 = 253 × 1.04 + 4 = 267.12 ms` ⇒ **`T_组 = 801.36 ms` ⇒ `1.5×T_组 = 1202.04 ms`**；取块 `interval_ms: 1000`：C1 ✓、**C2 ✓**、C3 ✓、C5 ✓、**C4 ✗（1000 < 1202.04）**⇒ 断言 `Err` 且**文案含 `1202`**（该值只有 C4 分支会写）。**如实登记**：C9 的违规域（`iv < 2×T_组`）**真包含** C4 的（`iv < 1.5×T_组`）⇒ 单组站上二者**必然同时触发**，只能由**文案**区分（C9 文案写 `U` 值与触发组，不写 `1.5×T_组`） |
| **AC-8-6** | **负载上界可复算**：首例配置按 §9.8.1 公式复算得 `U_口 = 2.68 % ≤ 0.5`（数值须与 §10.4 表逐位一致）；构造 `U_口 > 0.5` 的反例（组周期夹到 `≈T_组`）⇒ `Err` |
| **AC-8-7** | **失败隔离与事件语义不变**：① 站级承载组失败 ⇒ `offline` 事件 1 次（窗口内去抖）+ 按组周期退避 + 同口其它站不受影响；② **块级组失败 ⇒ 不产 `offline`/`online` 事件、`offline_count` 不自增**，且该块组按自身周期退避、下一轮照常探测（恢复即正常 cadence）—— **退避断言须用"连续失败两轮"构造**（`oc = 2` ⇒ `backoff_extra(组周期, 2) = 2 × 组周期`）：单轮时 `backoff_extra(组周期, 1)` 与 `due_round` 自身的 `next_due += 组周期` **数值相同**，无法区分"退避生效"与"退避未生效"；③ 站恢复 ⇒ `online` 事件 1 次 + **该站全部组**基线重建（恢复后首轮只建基线、不产事件） |

### 10.8 与 12 号 F25 / 01 号的关系（含 R-45 状态回写）

#### 10.8.1 本能力是 12 号 F25.3 的**前置**（不是 F25.3 本身）

12 号 PRD **F25.3** 的原文是「离散**告警位**在**变化后 ≤ 2 s** 上屏」（**验收原文在 `12-MUPC-本地显示终端-PRD.md:788`**；`:783` 是 F25 的**用户故事**，非验收条款），**"上屏"是端到端**，链路上有 4 段（12 号设计 §15.6.1 的口径 B：站周期 + ②入口 ≤0.5 s + ③段重建 ≤1 ms + ④组帧 ≤0.25 s + ⑤HMI 轮询+渲染 ≤0.6 s）。本章只**替换其中的"站周期"一段**：

| 站 | 改造前（12 号设计 §15.6.1 的"离散告警位"行） | 改造后（首例，位块 1000 ms） |
|----|--------------------------------------------|------------------------------|
| HVAC 位变化 | **物理不可达**（站周期 5000 ms ⇒ 采集段已 5 s） | 采集段 **≤1.0 s** ⇒ 口径 A（采集完成 → 帧发布）= `1.0 + 0.75 =` **1.75 s ≤ 2 s ✓** |
| 通知路径（12 号 §15.6.1 约束 1 的算式） | — | `1.0 + 0.25 + 0.5 + 0.1 =` **1.85 s ≤ 2 s ✓**（`latest_values` 的 `broadcast` 未丢帧时） |
| 兜底 tick 路径（广播丢帧时） | — | `1.0 + 0.5 + 0.85 =` **2.35 s > 2 s ✗**（**超差 0.35 s**，与 12 号设计对 `battery`/`fire`/`meter_batt`（同为 1.0 s 站周期）的算式**完全同构**——即 HVAC 由"5 s 特例"变为"与其余三站同档"） |
| 若位块取 **500 ms**（Q-21 选项 B） | — | 通知路径 **1.35 s ✓**、兜底路径 **1.85 s ✓**（**两条路径都 ≤2 s**） |

**⇒ 边界声明（写入双方文档，避免互相甩锅）**：

1. **02 号承诺**：告警位以**配置声明的块周期**被采集并上送（首例 ≤1 s）；`AC-8-1`/`AC-8-2`/`AC-8-4` 是它的可观测判据。
2. **02 号不承诺**：端到端 ≤2 s —— 其"兜底 tick 路径"一档由**屏侧组帧窗口与 HMI 轮询周期**决定（12 号设计 §15.6.1 约束 1 的 **R-33 口径裁定**：`periph_poll_ms = 500` 只保证通知路径达标，`= 100` 才保证丢帧也达标）。**若 12 号要"任何路径 ≤2 s"，其正解在屏侧配置（`periph_poll_ms`）或把位块取 500 ms（Q-21 选项 B），不在 02 号**。
3. **不得越界断言（原 AC-8-8，移入本处的元要求）**：**只**断言"告警位**采集**时延 ≤ 块周期"（首例 ≤ 1 s，判据 = AC-8-1 / AC-8-2 / AC-8-4）；**不得**断言"端到端上屏 ≤ 2 s"——后者含屏侧链路（12 号设计 §15.6.1 记 `+1.35 s`），其口径由 12 号 **R-33** 裁定。**该条不可执行验证，故不属验收项**（评审建议 c 移出 §10.7）。
4. **F25.4（站离线 ≤2 s 上屏）不在本能力的覆盖范围内**：其判定下界是 **`stale_timeout_s` = 5 s**（`mupc_core_config.production.yaml:144`；12 号 PRD F25.4 明写"**本 PRD 不重定义**该阈值"），这是**阈值语义**（"最后一次成功轮询后满 5 s 才给出离线结论"），**与轮询节奏无关** ⇒ 本能力对它**无改善**（口径 B 仍为 `5 s + 1.35 s = 6.35 s`）。**须由 12 号与产品就 `stale_timeout_s` 取值另行裁定**（02 号无权改该阈值语义）。⇒ 建议 12 号把 **R-45 拆为 R-45a（告警位快采：本能力覆盖）与 R-45b（站离线：属 `stale_timeout_s` 取值裁定）**，并把 R-45a 标为「需求已立」。<br>**⚠️ 本 PRD 只陈述事实与边界，不擅自改 12 号文档的条目编号**；实际回写见 §10.8.2。 |

#### 10.8.2 R-45 状态回写（**仅改 12 号设计的 R-45 那一行**）

12 号设计 `plans/modules/12-MUPC-本地显示终端-设计文档.md`（§15.9 报告项表）的 **R-45** 行，其"状态"由「**该能力尚未落地**」更新为：

> **需求已立（02 号）：能力与约束组见 02 PRD §10（v1.12，2026-09-23）、落点见 02 设计 §12；实现待 S3b-3**（**仍不假定其已存在**，本设计的降级口径继续有效）。

除该**一行**外，12 号设计**其余内容一律未改**（含其"不登记接口签名、不假定其存在"的口径与 §15.6.1 的各项算式）。

#### 10.8.3 与 01 号的关系

| 关联点 | 说明 |
|--------|------|
| 数据通路 | 快采组的位点仍经**既有通道**上送：`southd → core-bin SouthSink → data-processing::latest_values → display_host`（01 设计 §9.1 定义了 `latest_values`，写入方 = core-bin）⇒ **通路零改动、无新接口** |
| 位点可得性判据 | `latest_values` 对**位点**的可得性 = 「**站级轮询活性**（`mark_station_polled`） ∧ 点位质量」（01 设计 §9.1.2 的 `PointValue.ts_ms` 段）。位组成功并上送变化位时，站级活性被刷新（**不劣化**）；位组**失败**期间站级活性仍由站级承载组刷新 ⇒ **其位点的陈旧不会被自动判出** —— 该缺口如实登记为 **Q-23**（属 01 号 `mark_station_polled` 的**粒度**问题，本 PRD 不假定其扩展） |
| 上云（01 PRD §8） | 告警位的上送点表、IOA 表与分片主题均按**点名**生成（01 设计 §9.2 的段基址 + 段内序），与采集周期**解耦** ⇒ 提速只改变上送**频次**，不改变点表 |

### 10.9 待确认项登记（Q-21 / Q-22 / Q-23）

> 编号接续 §9.10 的 `Q-1…Q-20`；阻塞级别沿用三档口径。**三项均为【非阻塞·须裁定/备案】**，不阻塞本章生效与实现开工。

| ID | 类别 | 事项 | 处理口径 |
|----|------|------|----------|
| **Q-21** | **待裁定（产品/架构）**｜【非阻塞】 | **位块周期的最终取值**：① 2000 ms；② **1000 ms（本 PRD 推荐）**；③ 500 ms。三者还能否"端到端也 ≤2 s"取决于 12 号 R-33 的屏侧口径（§10.8.1 表） | **推荐 ② 1000 ms**：与 `battery`/`meter_batt`/`fire` 同档、**零改 `poll_ms`**、满足 C1–C5；且使 HVAC 从"5 s 特例"回到与其余三站**同档**（12 号设计对那三站的算式已是 1.85 s ✓ / 2.35 s ✗，属 **R-33 已登记的口径问题**，不是 HVAC 独有）。**若要"含兜底路径也 ≤2 s"** ⇒ 取 ③ 500 ms **且**把段级 `poll_ms` 1000 → 500（全局 tick 加倍，`U_口` 4.85%，仍 ≪ C9 的 50%）。<br>**③ 附项**：本 PRD **未复算 `grid_meter` 站的单轮耗时**（§9.8.1 该行标"既有"、未给值）⇒ 补算后再把该行纳入 §10.5 的 C9 复核表（**不影响本章结论**：其 `U` 只要 ≤0.5 即可） |
| **Q-22** | **待裁定（需求侧）**｜【非阻塞】 | **消防站 n>20 的"探测器块独立降频（≥5000 ms）"（§9.5.4）是否要求块级拆分**：字面要求"系统态 1000 ms、**探测器块** 5000 ms"⇒ **必须块级拆分**；但 `fire` 的两类判据（地址序 `mapper.rs:565`、登记数交叉校验 `mapper.rs:359`）**跨块**，按 C6/C7 本轮**不予放开** | **选项 A（本章口径，推荐本轮）**：**fire 站不支持块级覆盖** ⇒ n>20 时**整站** `interval_ms = 5000`（**现状实现**，系统态随之降到 5 s）⇒ 与 §9.5.4 的字面诉求有差异，**登记备案**；<br>**选项 B（下轮）**：放开"判据跨组"，须同时做**三件事**：① 判据求值前加**完整性守卫**（组内必须同时含链首块与全部 `fire_det*` 块，否则**跳过求值**）——**必须**，否则会**假报登记数不一致**：以 `reads = {fire_sys}` 为例，`fire_detector_mismatch` 得 `read_back = 20`、`capacity = 0 + 1 = 1`（`read_back` 取数见 `mapper.rs:363-372`；**容量算式**见 `mapper.rs:377-385` 的 `groups + u32::from(fire_chain_head(..).is_some())`）⇒ `20 ≠ 1` ⇒ **误报**；② 判据在**合并视图**上求值（各块取最近一次成功读），须额外定义"合并视图"的建立/失效与事件归属（设计复杂度中等）；③ **`fire` 站级 `interval_ms` 仍须设为 5000** —— 否则 `fire_det` 声明的 5000 **违 C5**（`interval_ms ≤ S`）。**③ 的连带后果（须与 ①② 一并登记）**：`fire_sys`（系统态）的 1000 ms 只能改由**块级覆盖**给出；且**站级承载组由 `fire_sys`（1000）变为 `fire_det`（5000）**⇒ **站离线判定节奏随之由 1 s 变为 5 s**（与选项 A 的"整站 5000"同），该变化落在 12 号 F25.4 的显示口径上，**须由 12 号确认**。**建议**：本轮取 A（与"改动小"一致），把 §9.5.4 的措辞在下一轮与本节一并消歧 |
| **Q-23** | **待裁定（跨文档·01 号）**｜【非阻塞】 | **块级（快采）组失败期间，其位点的陈旧可见性**：01 号 `latest_values` 的位点可得性判据是**站级**的（`mark_station_polled` + 组内点位质量，01 设计 §9.1.2/§9.1.3），而站级活性由**站级承载组**刷新 ⇒ 快采组失败时其位点**不会被判陈旧**（§10.6 第 4 条的**已知盲区**） | **① 本 PRD 的降级口径（本轮执行）**：块级组失败期间，其位点在 `latest_values` 中仍标 `Ok`；可观测性由 ② 保证。**不得**为此在 02 号侧新造第二套新鲜度判据（12 号 F25.1 同款禁令）。<br>**② 本轮的可观测替代**：`warn` 日志（含 `station/role/块名/reason`）+ 组级退避（掉线时长可由日志时间戳推得）。<br>**③ 建议的正解（须 01 号裁定，本章不假定其存在）**：把 `mark_station_polled` 的粒度由**站**扩为**（站, 组）**，或在 `PointValue` 上引入"组级活性"——两者均属 01 号 `latest_values` 的需求扩展，**请 01 号在下一轮增补时裁定**；02 号侧**无需改动**（本能力不改变任何既有判据的求值位置） |

### 10.10 与 §9 的一致性声明

| 关联 | 关系 |
|------|------|
| 本文档 §9（`[REVIEWED: PASS: 2026-09-21]`） | 本章**只在 §9 已定的块/点模型上新增一个块级字段**：① **不新增 role**（`R(role)` 复用既有 6 个 role）；② **不新增块级/点级字段以外的结构**（`points[]`、`func`、`addr`、`count`、`format`、`scale`、`offset`、`byte_swap`、`read_slice` 一律不变）；③ **不改变 §9.4.3 的 17 条与本 PRD 补落的规则 18/19**（本章的 C1–C9 由设计落为新增规则，其编号与落点见 02 设计 §12.7）；④ §9.4.1 的 6 站参考配置**逐字不变**（首例是**另一份生效配置的增量**，见 02 设计 §12.6 的配置迁移行） |
| §9.8.1（总线负载核算） | 本章的 `T_组` **复用其估时公式与常数**（1.04 ms/字节、`8 + 5 + 2N`、4 ms 周转），不另立口径；`U_口`（`Σ T/周期`）与其"总线占用率"列同源 |
| §9.7.1 / §9.7.2（站离线与运维可定位） | 站级 `offline`/`online` 语义与 `reason` 文案要求**零改动**（承载者由"该站唯一一轮"变为"站级承载组"，语义不变，§10.6 第 4 条） |
| §9.9.1 / §9.9.2（AC / RC） | 既有 `AC-1…AC-7`、`RC-1…RC-8` **全部保留有效**；本章新增 **`AC-8-1…AC-8-7`**（§10.7 的**可执行**验收项，7 条）；**AC-8-8 已按评审建议 c 移入 §10.8.1 的边界声明第 3 条**（元要求，不可执行验证，编号保留不回收）。**本章不新增 RC**（真机项沿用 RC-7 的连续运行与 RC-1 的逐点比对；HVAC 位块提速的效果由 AC-8-1/AC-8-4 的本机判据覆盖） |
| `plans/modules/12-MUPC-本地显示终端-设计文档.md` | 本章是 12 号 **R-45 / F25.3 的前置能力**；**仅回写其 R-45 那一行**（§10.8.2） |
| `plans/modules/01-MUPC-通信网关-设计文档.md` §9.1 | `latest_values` 的**类型与判据归属（01/03 号）不变**；Q-23 是"是否需扩其粒度"的**跨文档待裁项**，本章不假定其扩展 |

---

## 附录 A：依赖关系

### A.1 内部依赖

```
device-trait (无依赖)
    ↓
plugin-loader → device-trait
    ↓
rs485-plugin → device-trait
hplc-plugin → device-trait
```

### A.2 外部依赖

| Crate | 版本 | 用途 |
|-------|------|------|
| tokio | 1.x | 异步运行时 |
| serde | 1.x | 序列化 |
| serde_json | 1.x | JSON 解析 |
| thiserror | 1.x | 错误类型 |
| libloading | 0.8 | 动态库加载 |
| serial | 0.4 | 串口通信 |
| tracing | 1.x | 日志 |

## 附录 B：术语表

| 术语 | 说明 |
|------|------|
| MUPC | 微电网特种调控装置 |
| TTU | 台区智能融合终端 |
| HPLC | 高速电力线载波通信 |
| RS485 | 串行通信总线标准 |
| DE/RE | RS485 半双工使能引脚（Driver Enable / Receiver Enable） |
| GPIO | 通用输入输出引脚 |
| FFI | 外部函数接口（Foreign Function Interface） |
| cdylib | C 动态库格式（Rust crate-type） |
| Modbus RTU | 串行通信协议，RS485 物理层上的常用协议 |
| GB/T 27930 | 电动汽车非车载传导式充电机与电池管理系统通信协议 |
| BMS | 电池管理系统（§9 特指储能主控模块 RCU，华塑） |
| PCS | 储能变流器（§9 特指两级式 PCS，AC/DC + DC/DC） |
| SOC / SOH / SOE | 荷电状态 / 健康状态 / 能量状态（%） |
| FC02 / FC03 / FC04 | Modbus 功能码：读离散输入 / 读保持寄存器 / 读输入寄存器 |
| 点表 | 厂方给出的「寄存器地址 ↔ 物理量语义」映射表，落地为 `south_stations.regs` |
| 读窗口（块） | 一次 Modbus 事务覆盖的连续地址区间；只承载**传输口径**（`func`/`addr`/`count`/`byte_swap`）与缺省换算（§9.4.2.1） |
| 点位（point） | 一个物理量的**换算口径**（`format`/`scale`/`offset`/`word_order`/`name`）+ 其遥测键（metric）；块内逐点声明（§9.4.2.1/§9.4.2.2） |
| 多值块 | 一次 Modbus 事务读出的连续寄存器区间，逐值产出多个语义点（§9.1.1 G-4） |
| 位块 | `func: discrete` 的离散输入区间，逐位产出 0/1 点（§9.7.4） |
| BECG-3568 | 目标硬件平台（RK3568），板载 8 路隔离 RS485（COM1–8 ↔ ttyS0、ttyS2–ttyS8，无 ttyS1） |

---
## 附录：版本演进

> 正文已整合全部历史补丁，本表仅作演进追溯。

| 版本 | 主要变更 |
|------|----------|
| **v1.13（2026-09-23，§10 文档级订正，不构成重新评审）** | **只处置设计评审对 §10 的 4 项非阻塞订正 + 4 条建议改进**（标记 `[REVIEWED: PASS: 2026-09-21]`（§9）/ `[REVIEWED: PASS: 2026-09-23]`（§10）**原文未动**）：<br>**a. C7 依据列的引用不实** —— 原引「§10.9 Q-21 ② 登记了"允许 R 提速"的备选口径与代价」**不存在**（Q-21 全文只有三档取值 + `grid_meter` 补算附项）；§10.2.2 把它归给 Q-22 亦不成立（Q-22 谈**降频**）⇒ **改为三处真依据**（§10.2.2 范围外表的「判据块提速」行 + §10.3.2 的 `R(role)` 表 + §10.4 末"可提速一览"表）并明写"须另立需求"。<br>**b. 行号引用偏差** —— §10.8.1 引 `12-MUPC-本地显示终端-PRD.md:783`（F25 的**用户故事**）⇒ 订正为 **`:788`**（F25.3 **验收原文**）。<br>**c. "改造前占用率"口径不一** —— §10.4 表 `≈0.96 %`（取整值 48 ms）vs 版本演进 `0.95 %`（公式值 47.52 ms）⇒ **统一为公式精确值**：`U_前 = 47.52/5000 = 0.95040 % ⇒ 0.95 %`、`U_后 = 0.026848 ⇒ 2.68 %`、倍数 `2.8248 ≈ 2.8×`；§10.4 加"占用率口径"对照表并禁用取整值口径（与 02 设计 §12.6 逐位一致）。<br>**d. Q-22 选项 B 漏一环** —— 补**第三件事**：放开 C6/C7 后**仍须 `fire` 站级 `interval_ms = 5000`**（否则 `fire_det` 的 5000 违 **C5**）⇒ `fire_sys` 的 1000 ms 改由块级覆盖表达，且**承载组由 `fire_sys`(1000) 变 `fire_det`(5000)** ⇒ **站离线判定节奏 1 s → 5 s**，须由 12 号确认。<br>**建议 a–d** —— **a**：AC-8-5 补 **C4 可构造用例**（3×FC04 `count:120` ⇒ `T_组 = 801.36 ms`、`1.5×T_组 = 1202.04 ms`；并登记"C4 违规域被 C9 真包含 ⇒ 只能由文案区分"）；**b**：AC-8-3 钉 **tick 序 `0,1000,…,9000` + 四项计数**；**c**：**AC-8-8 移出 AC 表**，落入 §10.8.1 边界声明第 3 条（**编号保留不回收**，不再计入用例映射），§10.7 表下与 §10.10 同步；**d**：AC-8-7 ② 退避断言改 **`fail` 两轮（`oc=2` ⇒ `2×组周期`）**。<br>**未改**：C1–C9 约束内容、AC-8-1/8-2/8-4/8-6 判据、§10.4 的 `T`/`U` 数字（只统一口径表述）、§10.5/§10.6/§10.9 条目、§1–§9 任何既有裁定、任何代码/配置。**02 设计 §12 已按设计评审意见同步修订（v1.13，待复审）**。 |
| **v1.12（2026-09-23，新增 §10，不构成重新评审）** | **新增 §10「块级采集周期覆盖（告警位单独快采，S3b-3）」** —— 落实用户 2026-09-23 就 12 号 R-33 作出的裁定「**告警位单独快采**」。① **能力定义**：块级（`regs[]` 元素）新增可选字段 **`interval_ms`**（缺省 = 继承站周期 ⇒ 既有配置与行为**零变化**）；**分组语义** = 同站内 `eff = interval_ms ?? 站周期` 相同的块合成一个「读组」，各自独立到期（§10.3）。② **取值约束组 C1–C9**（正数 / 绝对下界 500 ms（与既有 `PCS_MIN_INTERVAL_MS` 同源）/ `poll_ms` 网格对齐 / `1.5×T_组` 相对下界 / **上界 = 站周期（只提速不降速）** / 判据完整性 / 判据组不得提速 / 站级承载组按最大组周期确定 / 口占用 `≤0.5`），并给出 **`R(role)`（站级判据所需块集合）** 的唯一定义（§10.3.2）。③ **首例 = HVAC 位块快采**（`hvac_di` 1000 ms + `hvac_in` 维持 5000 ms），单轮/负载**按 §9.8.1 公式重算**：`T_快组 = 21.68 ms`、`T_慢组 = 25.84 ms`、`U_口 = 2.68 %`（原 0.95 %），最坏同刻叠加 47.5 ms ≪ 1000 ms（§10.4）；并给出 2000/1000/500 ms 三档敏感性（§10.4）。④ **边界与异常 8 条**（含"块周期 > 站周期 ⇒ `Err`"与"**站级 `offline`/`online` 由站级承载组唯一承载，块级组失败不升级为站级 offline**"——否则屏侧会把正在正常上送的快采告警位一并隐藏，§10.6）。⑤ **验收标准 AC-8-1…AC-8-8**（含 AC-8-1「位块按独立周期被轮询」与 AC-8-2「标量仍按站周期」两条**可观测判据**，以确定性 tick 序列给精确计数）。⑥ **与 12 号 F25.3 / 01 号的边界声明**：本能力只保证"告警位以更快周期被采集"，**不承诺端到端 ≤2 s**（含屏侧 `+1.35 s`，其口径属 12 号 R-33）；**F25.4 的判定下界是 `stale_timeout_s` = 5 s 的阈值语义，与轮询节奏无关 ⇒ 本能力对它的口径 B 无改善**（建议 12 号把 R-45 拆为 R-45a / R-45b，§10.8.1）。⑦ **回写 12 号设计 R-45 那一行**：状态由「尚未落地」→「**需求已立（02 PRD §10 / 02 设计 §12），实现待 S3b-3**」（§10.8.2）。⑧ 新增待确认项 **Q-21**（位块周期取值与 `poll_ms` 联动）/ **Q-22**（消防 n>20 的"探测器块独立降频"是否要求块级拆分，含"缺块会**误报**登记数不一致"的复算证据）/ **Q-23**（块级组失败期间其位点在 `latest_values` 的陈旧可见性，属 01 号 `mark_station_polled` 粒度）。**未改**：§1–§9 的任何既有裁定、§9.4.1 的 6 站参考配置、任何代码/配置，**`[REVIEWED: PASS: 2026-09-21]` 标记未动**。**§10 评审**：需求评审员 **2026-09-23** 判 **`[REVIEWED: PASS: 2026-09-23]`**（附 4 项非阻塞文档订正，见文首标记） |
| **v1.11（2026-09-22，评审通过后的 §9 增量登记，不构成重新评审）** | **只做 §9.10 D-1（空调串口校验位）的裁定登记**，不动 §9 已获终审通过的任何既有裁定；`[REVIEWED: PASS: 2026-09-21]` 标记不动，顶部已追加 `[§9 增补 v1.11]`。用户 2026-09-22 裁定 **D-1 采用方案 ①**：**新增 per-station `parity`（`none` 缺省 / `even` / `odd`）**、**空调站配 `even`**、**同口各站 `parity` 须一致**（与 `baud_rate` 同级同规则）；**方案 ②（要求现场把空调参数 40016 置 0）被否**（依赖现场操作、不可复现、不可审计）；**空调站的投产限制随之解除**（原"在裁定前空调站不得投产"不再适用；真机项 RC-6 仍须做）。**D-1 行类别由「待评审裁定」改为「已裁定（2026-09-22，用户确认按 ①）」，原条目与"二选一"过程保留未删**。**§9.4.1** 的"其生效以 D-1 的裁定为前提"改为"D-1 已裁定按 ① ⇒ 本字段生效"，空调站 YAML 注释订正为已生效口径（**取值 `even` 一字未动**）。**处置终审记录中的"悬空待办"**：终审记录第 ⑥ 段曾登记"若 D-1 裁定为 ②，§9.4.3 的同口 `parity` 一致性与 AC-1 ③ 对应触发项将失去对象、须随裁定同步回写"—— **因裁定为 ①，该回写动作不适用、无需执行**，上述规则与 AC-1 ③ 触发项**均保留有效**；**历史块原文未动**。**实现已就位（回代码核实，四条与描述一致）**：① `mupc-southd/src/config.rs` 的 `StationParity{none,even,odd}`（`#[default] None`）+ 站级 `parity` + **规则 16 含 `parity` 同口一致性校验**（约 308–327 行）；② 空调站在 §9.4.1、`mupc/deploy/config/mupc_core_config{,.production}.yaml`、`tests/fixtures/south_stations_s3b2.yaml` 中**均为 `parity: even`**；③ `parity` **已透传至串口**（T5：`port_runtime::bus_config()` 三值逐一对映 `rs485_plugin::Parity`）。**未改任何代码/配置/设计文档。**<br>**补充（同日核验后）**：订正 **§9.9.2 RC-6** 的措辞为裁定后口径（原「偶校验/无校验两种情形至少验证一种成立」系 D-1 未定时的对冲表述，易被读成"两种都行"）—— 现明确：核对空调实际校验位（40016）、按①实测通信；不符时**改配置不改代码**。 |
| **v1.10（2026-09-22，评审通过后的 §9 最小订正，不构成重新评审）** | **只订正 §9.5.4 的登记数交叉校验容量算式**（漏计探测器 1），`[REVIEWED: PASS: 2026-09-21]` 标记不动，顶部已追加 `[§9 增补 v1.10]`。原容量口径为 `Σ(fire_det 前缀块 count) / 6`，因探测器 1 的寄存器（地址 11–16）落在 `fire_sys` 块内、不在 `fire_det*` 区 ⇒ **漏计 1 只**（按 §9.4.1 参考配置 `114/6 = 19`，读回登记数 **20** ⇒ 判据恒真）。订正为：**链首可得**时容量 = `1 + Σ / 6`；**链首不可得**时降级为 `Σ / 6`，与地址序校验（Q-9）的降级口径**同源**（共用"是否覆盖寄存器 11"判定）。全文其余位置（§9.4.3、§9.8、§9.9、§9.10）无同款算式，无需改动。 |
| **v1.9（2026-09-22，评审通过后的 §9 增量登记，不构成重新评审）** | 用户于 2026-09-22 就两项遗留问题给出确认结论，作**最小增量登记**：**不动 §9 已获终审通过的任何既有裁定**（`[REVIEWED: PASS: 2026-09-21]` 标记不动），顶部已追加 `[§9 增补 v1.9]`。① **Q-1 关闭（BMS 主从方向）** —— 【设计阶段阻塞】解除；结论 **EMS 为主机 / BMS 为从机**，现行实现（§9 与设计 §11 按 §2.2 口径、MUPC 主动轮询）**正确无需改动**。依据链逐处回原文核实：§2.2 明文支持；§3.1 **TCP 段**（主控做 TCP 服务器 + 后台监控主动连接）与 §2.2 **互相印证**（TCP 客户端/服务器 ≠ Modbus 主机/从机，Modbus-TCP 惯例为**客户端 = 主机**），原登记把二者并列为"相反表述"**属误读**；§3.1 **RTU 段**（主控做主机）**确认为笔误**；工程判据（MUPC 对 BMS **全为读操作**，而 Modbus **只有主机能主动读、无推送机制**）证明只有 §2.2 口径成立。Q-1 原条目与证据**保留未删**，关闭结论追加于该行 **⑤**；连带门禁标注同步更新（§9.5.1 前言、§9.9.2 **RC-4** 性质由"阻塞解除判据"改为**常规投运核实**、§9.10 前言的阻塞档位）。② **Q-19 撤销（消防/空调并接 BMS 485-2 的风险）** —— 依据 **BECG-3568 接线拓扑图**（`hw/微信图片_20260908170935_64_1061_修正.png`）：`RS485-1 → PCS`、`RS485-2 → BMS`、`RS485-3 → 空调`、`RS485-4 → 关口表`、`RS485-5 → 储能表`、`RS485-6 → 消防` ⇒ 消防与空调**均直连 BECG-3568（EMS）**，**不存在"双 master 共总线"**，`ttyS3`/`ttyS6` 照常启用。同时如实登记**原疑问的来源**（CAN2.0 协议 图 2-1-1「总控模块连接PCS设备」系储能厂家以 **SCU 总控**为中心的**自有系统视图**，非本项目接线拓扑 ⇒ 属**误读**；BMS 位 482 只说明 BMS 确有第二个 485 口，**不能推出**消防/空调挂其上）与**拓扑图订正**（原图后两框笔误标 `RS485-4`，已在修正版改为 `RS485-5`/`RS485-6`，**原件保留未动作为原始证据**）；Q-19 原条目**保留未删**。③ **新增 §9.3.3「设备 ↔ 端口映射（硬件接线契约）」** —— 把 §9.3.1 中分散书写的 `port`（及 `slave`/`baud_rate`）收敛为**一张统一映射表**（设备 / BECG 接口 / Linux 设备节点 / `role` / 站 `id` / `slave` / `baud_rate`），**取值全部取自 §9.4.1 的 6 站参考配置**（不引入新值、以 §9.4.1 为准）；附**依据 1**（接线拓扑图）、**依据 2**（`hw/BECG-3568 BOX感知与控制主机规格书(20250818) .docx` 载明 8 路隔离 RS485，`A0/B0→ttyS0`、`A2/B2→ttyS2` … `A8/B8→ttyS8`，**无 A1 故无 ttyS1**）与**接口命名对照**（`RS485-N = COM N = A(0/2…)/B… = ttyS0/ttyS2…`，消除三套写法），并给出两条硬性推论（**禁双 master 共总线**：`ttyS0` 被 intercore 占用 ⇒ PCS 只读站须另占空闲口 `ttyS7`/`ttyS8`；**PCS 接线前提** 承接 §9.10 Q-15）。**未改任何代码/配置/设计文档。** |
| **v1.8（2026-09-21，评审通过后的 §9 最小订正，不构成重新评审）** | **只订正 2 处（§9.4.3 第 15 条适用域 + §9.4.2.4 块级 `count` 必填性），不动 §9 已获五轮评审 + v1.7 补登的任何既有裁定**；`[REVIEWED: PASS: 2026-09-21]` 标记不动，顶部已追加 `[§9 增补 v1.8]`。① **订正 1（第 15 条"块落地极大性"，阻塞级缺陷）** —— 原文"块取极大区间"会拒掉 §9.4.1 第 6 站 `grid_meter` 的 6 个**严格相邻、零空洞**相量块（`0x1000/0x1006/0x100C/0x1012/0x1018/0x101E`，`count 6/6/6/6/6/2`），与 §9.4.1 ①「照录生效配置」「既有 `meter_grid` 形态不动」直接冲突（线上配置迁移后启动 fail-fast）；该缺陷由需求侧与设计评审**各自独立复现**。按项目经理裁定订正：**适用域限定为「声明了 `points` 点级清单的块」**，未声明 `points` 的块（既有 `meter_grid` 形态、`discrete` 位块等）**不参与合并判定**；判据由"合并后空洞 ≤ 4"**改为**"合并后 `count ≤ 120` 寄存器"（120 = Modbus 单次读保守上限）；`read_slice: true` 的块豁免本条且只在适用域内有对象。**③（空洞 ≤ 4）经论证保留** —— 它是 §9.4.2.1 第 3 条的强制拆块条件，删去会使本条反向误拒第 3 条强制产生的合法拆分（v1.8 的实质变化是**新增 ④ `count ≤ 120`**）。§9.4.2.1 第 5 条（极大性定义源）同步限定适用域。**并逐个相邻块对自检 §9.4.1 的 6 站**（结论：必然通过，含 `grid_meter`；唯一地址连续的邻块对是 fire 站 `fire_sys`(有 `points`) + `fire_det`(无 `points`)），自检写于 §9.4.3 表后。② **订正 2（字段必填性）** —— §9.4.2.4 块级 `count` 原写"必填，>0；S3a 既有块缺省 2"（自相矛盾），订正为**非必填、缺省 2**（代码事实：`RegBlockConf` 的 `#[serde(default = "default_reg_count")]` → `default_reg_count()` 返回 `2`）；同表其余字段"必填/缺省"**逐字段回 `config.rs` 核对**后仅此一处错误（`name`/`addr` 无 `default` ⇒ 必填但表内未声明缺省、不构成错误；`func`→`holding`、`format`→`float32`、`scale`→`0.0` 均与表一致），核对结果记为 §9.4.2.4 表下"块级字段必填/缺省核对"。 |
| **v1.7（2026-09-21，评审通过后的 §9 增量补登，不构成重新评审）** | **只做设计阶段反馈的 3 项补登/订正（Δ-1/Δ-3/Δ-4），不动 §9 已获五轮评审的任何既有裁定**；`[REVIEWED: PASS: 2026-09-21]` 标记不动，顶部已追加 `[§9 增补 v1.7]` 说明。① **Δ-1（订正）** —— §9.4.2.4 的 `format` **缺省值表述由 `int32_scaled` 订正为 `float32`**（代码事实：`mupc-southd::config::default_reg_format()` → `RegFormat::Float32`；原来会误导为"2 寄存器 1 值"的相反结论）；块级/点级字段表同步（点级 `format` 无独立缺省、未声明即继承块级），并**补登护栏**"本轮新增块一律显式声明 `format`、不得依赖缺省"（置 §9.4.3 末，**文档约束 + 加载期告警、非拒绝条件、不纳入 AC-1 ③** —— 缺省是既有代码契约，硬拒绝会改变既有行为）。② **Δ-3（补登）** —— §9.4.2.4 块级字段表**新增 `read_slice`（bool，缺省 `false`）**：分片豁免标记，**仅豁免 §9.4.3 第 15 条「块落地极大性」**，适用 PCS 3 区/消防探测器区等"设备单次读上限（Q-6/Q-7 文档未明确、须现场实测）导致必须分片"的块；给出四条"不得用于逃避合并"的约束（合并后仍可一次读回者必须合并；只豁免第 15 条；块级逐块、禁止整站/按 `role` 批量套用；须在注释登记理由，属 RC-1/RC-5 目视项），并写明**不以"按 `role` 特判"替代**（违反 §9.1.1 **G-5**）；第 15 条同步补明豁免关系。③ **Δ-4（补登）** —— 消防"复合探测器登记数量"（寄存器 10）正式定名 **`fire_det_count`**（跨文档契约点、站内唯一、与 `soc` 同类）：§9.4.1 参考配置 fire 站 `fire_sys` 的 `at: 7` 增列 `name: fire_det_count`；§9.5.4 点表该行标注点名与"既是遥测点、又被配置期/运行期校验引用"的角色，"登记数交叉校验（强制）"改为按**点名**查找（禁止按块名+硬编码地址，属设备特判），并登记"改名 = 断供该校验"。 |
| **v1.6（2026-09-21，按四审意见收尾修订，待终审）** | **只做 1 项阻塞 + 5 条建议，不动已获四轮评审接受的成果。** ① **阻塞项（AC-1 ③ 与 §9.4.3 互相否定）** —— 从 AC-1 ③ 触发清单**删除**「`uint16/int16` 携带 `offset` 但点表未登记符号性来源 → `Err`」（配置层无该字段、校验器看不到 §9.5 ⇒ **断言不可满足**）；§9.4.3 的该条（① ）标注**不纳入 AC-1**，核验**归属投运前 RC-1 + §9.5 表核对**，待设计给出**点级来源字段**后再纳入 AC；② 条补明期望值来源（**校验器内的点表登记值常量表**）⇒ 保持可机械校验。**AC-1 ③ 与 §9.4.3 规则清单逐条对表**后补齐 4 条漏列机械规则（`role` 未知名 / 位块超上限 / `addr` 非法 / 块区间重叠），并声明"保留的每一条均已由校验器可判定"；**两级可证断言结构未动**。② **计数口径** —— 全文"110 处一律写作 `UNIT`"改为**整词 `UNIT` 104 + `UNIT32` 6（子串合计 110）**，并登记 **§5.2 正确拼写 `UINT` 整词 164 处**（§9.4.2.4 ④ / §9.5 前言 / §9.5.1 A 前言 / Q-20 ① 四处同步）。③ **ADL400 来源** —— §9.5 前言"三种来源"表把 ADL400 移出"工程判断"行，**单列第 4 行「混合（第 1 + 第 3 类）」**（4 字节功率类/PF 厂方明写、2 字节点工程判断）。④ **引述口径** —— 回原文核实 32 位类型列实写 **`UNIT32`**（6 处），全文统一「原文 `UNIT32`（= `UINT32` 笔误）」，删 §9.5.1 表第 25 行"文档为 `UINT32`"。⑤ **耗时措辞** —— §9.8.1 的"286/90/310/300/48ms 可精确复算"改**分级表述**：286/90/300/48ms 为**复算后取整**（286.1/89.9/299.7/47.6ms），**meter_batt ≈310ms 为量级口径**（精确值 ≈308.8ms）；§9.5.1 两处"精确累加/精确值"同轮软化。⑥ **RC-1 显式点名 116 / 186 为必检点**（符号性判别关键点），并附 ADL400 0x0092、BMS 2991–2994 为同批核对点。 |
| **v1.5（2026-09-21，按四审意见修订，待再审）** | ① **核心订正（16 位量符号性被整体改判错）** —— 回原文核实 BMS 文档 §3.1.1「数据类型」表**只定义 `UINT`（16bit 无符号）与 `INT`（16bit 有符号），无 `UNIT` 这个类型**；而输入寄存器表（§5.3）类型列 **110 处一律写作 `UNIT`**、`INT` 在**点表中从未出现**（全文档仅 1 次，即类型表自身）⇒ 判定 `UNIT` 为 `UINT` 笔误。**判定依据（强反证）**：同一文档 §5.2 保持寄存器表用**正确拼写 `UINT` 且同样大量携带负偏移**（「`UINT` … 偏移：-50℃」「`UINT` … 偏移量：-3000A」）⇒ **负偏移不能推出有符号**，厂方主力做法就是「无符号编码 + 负偏移做零点平移」。据此把 v1.3/v1.4 改判为 `int16` 的 **BMS 116/117/122/127/129/155/157/186、2991–2994 全部改回 `uint16`**（§9.4.1 YAML、§9.5.1 A 表、§9.7.5、§9.4.2.4）。② **116 量程重算** —— `uint16` 满量程 = **raw 0 → −1600.0 A、raw 65535 → +4953.5 A，即 [−1600.0, +4953.5] A**（v1.4 的 `int16` 量程 [−4876.8, +1676.7] A 作废）。③ **§9.4.3 校验规则改正确** —— 删除 v1.2「拒 `uint16` + 负 offset」与 v1.3/v1.4「拒 `uint16` + 非零 offset」**两条误拒规则**（后者会把 9 个 BMS 主力点整体误拒），替换为「**符号性声明可追溯**（`offset ≠ 0` 须在 §9.5 点表登记来源）+ **与点表登记值一致**」的登记式校验；并明确「**不校验符号性对不对，只校验可追溯/一致**」。④ **删除 G-2 的错误结论**「带负偏移的量必须声明 `int16`」（§9.1.1、§9.4.2.4）；§9.4.2.4 新增「`offset` 与符号性无关」的完整论证（含 v1.3/v1.4 循环论证的驳正）。⑤ **AC-2 订正** —— 116 raw=65535 期望值改为 **+4953.5 A**，并**补真正的判别用例**（`uint16` +4953.5 A vs `int16` −1600.1 A，本 PRD 按厂方标注取前者；**把"若现场实测在小电流放电时被解成大正值则须改 `int16`"的判别逻辑写清，不再用结论当证据**）。⑥ **PCS 逐点核对（连带任务）** —— 回 PCS 原文**逐点**核对 3 区 72 点的 `Int16`/`UInt16` 标注，**结果：0 处不符**（PRD 与原文逐字一致）；在 §9.5.2 增补"PCS 类型为**厂方逐点明写**，与 BMS 的**推断处理不同**"的来源说明。⑦ **空调 / ADL400 / 消防 补做"类型来源"核对** —— **空调有逐点类型标注**（`16位有符号`/`16位无符号`/`BOOL类型`）⇒ 30001/30003 的 `int16` 系**厂方明写**（保持不改）；**ADL400 为混合来源**（4 字节功率类/PF 明写「有符号整形」；2 字节点无类型字样 ⇒ **工程判断**）；**消防无类型列**（⇒ 全部 `uint16` 为**工程判断**，依据取值范围非负，CO/VOC/H2 明写 0–65535）。**三份均在 §9.5.3/§9.5.4/§9.5.5 与 §9.7.5 显式登记来源，不得默不作声写成厂方标注**；§9.5 前言新增"三种来源"对照表。⑧ **新增 Q-20** —— 如实登记 `UNIT`→`UINT` 的**推断依据**（110 处一致、§5.2 反证）、已知代价、**投运首日必做动作**（以充放电工况核对 116/186，若解成大正值则改 `int16`）、向厂方书面确认项。⑨ 顶部标记保留历史 REJECTED 记录，**未改 PASS**。 |
| v1.0 | 初版，南向通信模块 PRD（SouthDevice 抽象、RS485 协议处理器、HPLC 驱动、动态插件系统） |
| v1.1 | 修复 P-06/P-07/P-08 三项低风险覆盖缺口：以太网连接方式、消防控制系统详细协议、HPLC 芯片 SDK 集成延后说明 |
| v1.2 | 新增 **§9 站级南向设备语义点表集成（S3b-2）**：5 份厂方协议（BMS/PCS/ADL400/消防/空调）逐设备语义点表；新增 `Role::Pcs`（只读）；BMS↔PCS 的 CAN2.0 链路范围外登记；写操作立为独立 Task 且 **PCS 写本轮范围外**（与 S2 联锁争用 4 区 500）；登记 G-1…G-6 能力差距、17 项文档未明确/矛盾项与 3 项待评审裁定；§1.3/§1.4 补交叉引用。**（2026-09-21 评审裁决 REJECTED）** |
| v1.4（2026-09-21，按复审意见修订，待三审） | ① **阻塞 1**：§9.4.1 第 6 站照录生效配置 —— 站 `id: grid_meter` + **6 个相量块**（改回生效值，站名全文统一）；AC-1 降级为**两级可证断言**（既有字段子集当前可跑通 / 完整 YAML 实现 §9.4.3 后跑通），不再主张"当前代码即可跑通"。② **阻塞 2**：`mb_phase` 的 `at:13/14` 显式 `scale: 0.1`（原文「整型 单位0.1%」），修正 10× 偏差；仍为块内逐点声明。③ **残留 3**：§9.3.1 hvac 单轮耗时 ≈100ms → **≈48ms**；hvac 帧字节 50 → **38**；meter_batt 单轮值全文统一为 **≈310ms**。④ **A-2 补充**：BMS 116 量程按 `int16` 满量程订正为 **[−4876.8, +1676.7] A**（−1600.0 非下限，仅 raw=0 处值），与 AC-2 的 raw=65535→−1600.1 A 自洽。⑤ **建议 1–5**：hvac 帧字节 ≈38；查找键口径限缩（`grid_meter` 相量按块名查找）；`Q-17`→**`Q-16`**；点总数 **618**（原"400 点量级"）；严格拆块对照数（772ms/17 块/734ms）标注**量级估算**并说明 17 块仅指 100–130 段、全站 30–40 块。⑥ **Q-1** 补齐【设计阶段阻塞】的阻塞级别/影响面/强制约束/解除条件（含"相反口径 → 重新走需求评审"触发条），Q-4/Q-15 定为【投产阻塞·现场裁定，不阻塞设计】。 |
| v1.3（2026-09-21，按评审意见修订，待复审） | ① **A-1**：给出**唯一**的「点表 → `regs` 块」落地规则 —— 块 = 读窗口 + 传输口径（`func`/`addr`/`count`/`byte_swap`）、换算口径**逐点声明**（`points`），并以"空洞 ≤ 4 寄存器（= 空读量不超过一次请求帧 8 字节）"给出机械的拆块判据；据此重算全部站：battery ≈286ms、pcs ≈90ms、meter_batt ≈310ms、fire ≈300ms（n=20）、hvac ≈48ms，**全部 ≥3.2× 余量，无需上调任何 `interval_ms`**（严格拆块时 BMS 需 772ms/2000ms，已作对照算式留存）。② **A-2**：带负偏移量的 16 位量（BMS 116/117/122/127/129/155/157/2991–2994、空调 30001/30003）重裁定为 **`int16`**，校验规则反转为"**拒 `uint16` + 非零 `offset`**"。③ **A-3**：AC-2 数值**逐条回原文复算**（模块温度 raw 650→**65**；电流/电量/电能/空调等全部重列），并补齐 AC-1 所需的 **§9.4.1 6 站完整 YAML**。④ **A-4**：删除"消防/PCS 版本号可读"的表述（两份协议**均无版本寄存器**）。⑤ **B-1…B-3**：逐点语义放**配置**、点名规则统一为位置式序列（绝对位地址命名废止）、G-6 **延后**（含两条重启条件）。⑥ **C-1…C-4**：订正 ADL400 4 处区间上界、登记 BMS 124 低字节矛盾（**Q-18**）、CAN 措辞与原文对齐（CAN2.0 / 250kbs）、订正 hvac 带宽行。⑦ 新增 **Q-19**（消防/空调是否并在 BMS 485-2）、位块落库量级提示、`parity` 字段（待 D-1 裁定） |

