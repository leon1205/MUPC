//! 日志服务（F10）——开发单元 **H**（设计 `docs/superpowers/plans/modules/12-MUPC-本地显示终端-设计文档.md`）。
//!
//! 承接 §3.4 的两条**只读**端点（承接组件 = `LogService`）：
//!
//! | 端点 | 参数 | 返回 |
//! |------|------|------|
//! | `GET /v1/console/logs` | `levels`(多值) `targets`(多值) `range=1h\|24h\|custom` `from`/`to`(ms) `cursor`(可选) `limit`(≤200) | [`LogPage`] |
//! | `GET /v1/console/logs/targets` | — | `Vec<String>`（模块选项，≤[`LOG_TARGETS_MAX`]） |
//!
//! # ⚠️ 数据源（**如实**：ring 这一半**没有实现**，已拆为独立单元 H-2）
//!
//! 设计 §4.4 把 `LogService` 定为**两半**：
//!
//! | 半 | 设计要求 | 本单元（H）现状 |
//! |----|----------|------------------|
//! | ① **实时** | 自定义 `tracing_subscriber` Layer，把已格式化条目推入**有界 ring**（`VecDeque`，容量 = 契约 `LOG_RING_CAPACITY` = 2000，内存上界固定 ⇒ PRD §4.1.4「内存不得单调增长」） | ❌ **未实现**（无 Layer、无 ring、无 `live_ring` 消费者） |
//! | ② **历史** | 日志文件限额扫描（复用旧 `LogsHandler` 的文件定位/解析逻辑，去 `keyword`） | ✅ 已实现（本文件） |
//!
//! **缺口与归属（不许含糊）**：
//!
//! - 契约 `LogLimits.live_ring`（`display-proto/src/config.rs`）**零消费者**——全仓没有任何代码按它分配 ring；
//! - 设计 §12.4 / §11.1 把「ring 定容不增长」划给本单元，**本轮未做**，已由 PM 拆为**独立单元 H-2**；
//! - **为什么拆**：ring 的 `seq` 与**本文件的文件扫描 `seq`** 如何对齐（同一个计数器？ring 条目是否也要能从
//!   文件里被再次扫到？）设计里**没有写**，需要先出**设计补丁**再实现——先做会把"两套 `seq` 真源"焊死。
//! - **后果（诚实）**：屏上「实时日志」实际由 **500 ms 增量文件扫描**（`range=1h` + `cursor`）承担，
//!   延迟取决于落盘与扫描，**不是**设计承诺的"ring 推送 ≤ 2 s"；日志级别在 ring 上的过滤语义
//!   （`tracing` 层写盘前过滤 vs 本服务读盘后过滤）也**尚未**对齐。⇒ `entries` 全部**来自日志文件**，
//!   `seq` 由文件行推出（见下）。
//!
//! # 日志目录的**单一真源**（R2 整改）
//!
//! 写者（`main.rs` 的 `tracing_appender::rolling::daily(dir, "mupc.log")`）与读者（本服务）**必须是同一个值**。
//! 现状：`--log-dir` 是 `config.system.log_dir` 的**可选覆盖**，`main.rs` 在配置加载后把覆盖值**写回**
//! `config.system.log_dir` ⇒ 全进程只有这一个值（appender / 本服务 / 审计目录同源）。
//!
//! ⚠️ **S-2 登记：这次写回是「纯内存」的，不落盘。** 写回只改**内存里的** `CoreConfig` 副本；
//! `system.log_dir` **不在** G-2 可编辑字段表内（`hot_apply` 的字段表里没有它）⇒ 屏上改不了它、
//! 也没有任何回写路径 ⇒ **yaml 里永远是原值**。别误以为"屏上/接口改过了，yaml 里就是新目录"：
//! 下次冷启动仍按 yaml（+ `--log-dir` 覆盖）取值。
//! ⚠️ 历史缺陷：整改前写者取 `cli.log_dir`（默认 `/opt/mupc/logs`）、读者取 `config.system.log_dir`，
//! 两者可不同且**无人知晓** ⇒ 最坏不是 503 而是**静默失实**（`mkdir -p` 把读者那侧目录建出来 ⇒
//! 200 + `entries=[]` ⇒ 屏上「当前筛选条件下无日志」，而日志其实写在别处）。
//!
//! # `seq` 的定义（**契约未定，本单元必须自定；故此处逐字写死**）
//!
//! 契约把 `seq` 定为"**单调序号**，也是 `cursor` 增量拉取的依据"（`display-proto/src/log.rs` 行 76–78），
//! 但**没有**规定它与时间的换算关系。文件里的行**没有** `seq` 字段 ⇒ 服务端必须**推导**一个。
//! 本单元取：
//!
//! ```text
//! seq = ts_ms * 1000 + k        （k = 同一文件内、同一毫秒内该行的出现序号，0-based，上限 999）
//! ```
//!
//! **为什么不是"全局行号"**：全局行号在**旧文件被删除/轮转**时会整体平移 ⇒ 渲染端手里的
//! `cursor`（= 已见最大 `seq`）会突然指向别的条目（**静默错位**）。`ts_ms*1000+k` 只依赖
//! **该行自身的内容与它在同文件同毫秒内的次序** ⇒ 旧文件增删不动既有行的 `seq`（稳定）。
//!
//! **已知边界（如实登记）**：
//!
//! ① 同一毫秒内超过 1000 行时 `k` 截到 999 ⇒ **同一文件内**这些行的 `seq` 相同，同 `seq` 只保留一条
//!    （另一条被覆盖；`k` 的 999 上限是契约未定的自定口径，见上）；
//! ② 时间戳非单调的日志行会得到非单调的 `seq`（此时按 `seq` 排序与按时间排序不一致）——这正是
//!    契约"`seq` 才是新的权威判据"的既定口径；
//! ③ **跨文件同毫秒 ⇒ `seq` 相同（契约的"单调"在同 ms 下不成立）**。R6 整改：`kept` 用
//!    **复合键 `(seq, 文件序)`**，两条**都保留**（旧的 `kept.insert(seq, …)` 会让后扫到的旧文件
//!    条目**覆盖**新文件那条 ⇒ 不只丢一条，是**内容错**）。排序按 `(seq 降序, 同 seq 时新文件在前)`。
//!    渲染端只把 `seq` 当 `cursor` 高水位（`p3_logs.rs`：游标 = 已见最大 `seq`）⇒ 同 `seq` 并列
//!    不影响游标语义（两条都 `≥ cursor` 且不超过 `max(seq)`）；
//! ④ **扫描方向会按窗口位置自适应（见下），而 `k` 的组划分在"文件行非单调"时可能与另一方向不同**
//!    ⇒ 极端情况下同一条目在两次请求里可能拿到不同 `seq`（只影响同毫秒内几条的相对次序）。
//!    严格单调的文件（正常情况：`tracing_appender::non_blocking` 由**单一写者线程**按 FIFO 落盘）
//!    两个方向给出**逐字段一致**的结果，本文件有 `seq_is_stable_across_requests_and_old_file_removal` 兜底。
//! ⑤ **R6 残余（登记，本轮不改代码）**：渲染端游标 = 已见**最大** `seq`（`p3_logs.rs`）。若**同一
//!    毫秒**的条目跨文件且**多于 `limit` 条**（`> 200` 条同 ms 才会显形），一次增量请求无法把它们
//!    全部返回（`kept` 只留最新 `limit+1` 条），而游标已抬到这批里的最大 `seq` ⇒ 更旧文件里**同 ms**
//!    的那条会**永远取不回**（游标只增不减，它不再满足 `seq > cursor`）。属契约/渲染端固有口径的残
//!    余，需 >200 条同 ms 条目才显形；本模块**不做**特判（特判会让 `seq` 语义在这里裂成两套）。
//!
//! # 排序 / 分页（**按契约，不发明**）
//!
//! - **排序**：`entries` 恒按 `seq` **降序**（= 时间倒序）。
//! - **`cursor`**：含义 = "**我已有 `seq ≤ cursor` 的条目**" ⇒ 返回 `seq > cursor` 的条目
//!   （**取更新的**）。依据：设计 §4.4「增量拉取：返回 `seq > cursor` 的条目；HMI 每 500 ms 拉一次」，
//!   且渲染端 500 ms 增量路径**硬依赖**该语义（`p3_logs.rs::fire_increment` 传 `max(已见 seq)`、
//!   `app.rs` 把结果**整体替换**窗口 ⇒ 若回的是"更早的页"，屏上会退化成重复内容）。
//! - **`has_more`**：本次**范围内还有匹配条目未被返回**（即被 `limit` 截断）。
//! - **`next_cursor`**：`has_more` 时的"下一页（**更早**）游标" = 本页**最小** `seq`；否则 `None`。
//!   ⚠️ **契约缺口（本单元如实登记，未改契约）**：契约只给了一个 `cursor` 参数，而它已被**渲染端
//!   钉死为"取更新的"** ⇒ `next_cursor` **无法原样回喂给 `cursor`** 得到更早的页（回喂会再次返回
//!   同一页）。⇒ **"向前翻历史"在现契约下不可达**。渲染端也确实**不使用** `next_cursor`
//!   （`p3_logs.rs:1327` 明写"游标 = 已见最大 `seq`（不是 `LogPage.next_cursor`）"）⇒ 本字段当前
//!   只作"还有更早的"这一**事实**的载体。需 PM 裁定（见交付报告的「未决」）。
//!
//! # 限额（EDGE-15）：判据是**窗口内容**，不是**文件长度**
//!
//! 单次请求最多扫 `display.log.max_files` 个文件（默认 [`LOG_SCAN_MAX_FILES`]）、
//! 且**本次请求要交付的行数**（= 落在窗口内 **且** 通过级别/模块/`cursor` 三个筛选的行，
//! 见 [`ScanState::take`] 的口径论证）不超过 `display.log.max_lines`（默认
//! [`LOG_SCAN_MAX_LINES`]）；**任一超限 ⇒ 立即停止扫描**，并回
//! `LogPage { entries: 已收集的最新 limit 条, range_too_large: true }`
//! （**不执行全库检索**，但**带回已收集的内容**，见下「命中时回什么」）。
//! 依据：设计 §4.4 / §8.3 EDGE-15。渲染端的列表形态按 `range_too_large` 与 `entries` **两个**
//! 输入决定（`p3_logs.rs:728` 的 `list_view_of(rows, too_large)`：只在 `rows == 0` 时走
//! `Incomplete`，有条目时**照显行 + 超限提示是常驻构件**）⇒ 带条目返回**无需**渲染端改动。
//! 「超限」与「无日志」是**两个不同信号**，本文件绝不互替（空结果 ⇒ `range_too_large=false`）。
//!
//! ⚠️ **R3 整改（评审 PROBE4）**：整改前计的是**扫描行数**（`stats.lines_read`）⇒ 一个 60 000 行的文件
//! 配 1 ms 的窗口、窗口内仅 1 条命中，也会被判 `range_too_large=true`（屏上假话「请缩小时间范围」，
//! 而窗口再小也没用）。EDGE-15 的原文是「起止区间**跨度过大**」⇒ `range_too_large`
//! **只能**表达"**窗口内容**超过限额"，**不得**退化成"文件太长"。判据现为 [`ScanStats::window_lines`]。
//!
//! ## ⚠️ 硬代价闸（B-2 整改；**口径在整改三被订正**）——与 EDGE-15 判据**无关**的第二道闸
//!
//! 「窗口内容」判据有一个**它自己管不到**的洞：**解析不出时间戳的行既不计入 `window_lines`，
//! 也永不触发早退** ⇒ 一份 120 000 行的**纯文本**文件（日志格式漂移 / logrotate 压过的二进制 /
//! 别的工具写进 `mupc.log*`）会被**整份读完**，且**每个** 500 ms 请求都这样读一遍。
//! 故加一道与判据正交的**硬代价闸**，它只对**读不懂的内容**计数，两类：
//!
//! - [`SCAN_READ_BUDGET_LINES`]（= 契约上限 × 4）个**解析失败的行**（[`ScanStats::unparsable_lines`]）；
//! - [`SCAN_READ_BUDGET_BYTES`]（= 1.5 × 倒读读窗上限）个**读了却没产出任何行**的字节
//!   （[`ScanStats::skipped_bytes`]：倒读的翻窗重读 + "行长 > 读窗上限"的跳过片段）。
//!
//! ⚠️ **整改三订正（原口径是错的，评审两次抓到）**：整改二把闸门挂在 `lines_read`（**读入的总行数**）上，
//! 那是**上一轮把 R3 判 REQUEST_CHANGES 的同一句话**——「`range_too_large` 由'扫了多少行'决定」。
//! 实测：**500 000 行全部可解析**的文件 + 文件中部 **1 ms** 窗口 ⇒ 旧口径回
//! `lines_read=200001, window_lines=1, range_too_large=true, entries=[]`，屏上说「检索范围超限 ·
//! 请缩小时间范围」，而窗口已是 1 ms、**再缩也没用**。故：**可解析的行一律不计入硬闸**
//! （它们的代价由 EDGE-15 的窗口内容判据 + 择向 + 迟滞共同界定，见「扫描方向」），
//! 闸门只兜「**内容根本读不懂 / 读不动**」的无界代价。
//! ⚠️ **同轮订正（评审实测）**：翻窗重读与跳过分支过去**完全不计账**（`note_line_read` 只在
//! `backward_line` / `read_forward` 的行入口调用）⇒ 4 MiB 无换行文件实测读入 8 257 536 B
//! （**2.0×**）而 `lines_read` 只有 1 ⇒ 行数闸**永不触发**，且回的是
//! `entries=[] ∧ range_too_large=false`（= 把"没查完"说成"确实没有"）。字节闸就是为此加的。
//!
//! ⇒ 现在这两句自述**成立**，但**只对"读不懂的内容"成立**：解析失败的行 ≤ 200 001 条、
//! 翻窗重读/跳过片段的字节 ≤ 1.5 MiB + 一个读窗（**均与文件多大无关**，见两条常量的取值论证；
//! 注：不含块边界半行的重读字节与择向采样字节 —— ⚠️ **订正（整改五 D-3）**：本句原写"两者各自
//! 有界……都远小于本条闸的尺度"，**后半句不成立**："读窗 × 块数"与**文件大小同阶**（读窗有
//! [`MAX_REVERSE_WINDOW_BYTES`] 上限、块数随文件增长 ⇒ 上界 = `min(行长, 读窗) × 块数`）。
//! 对**短行的现实日志**它是小量（每块只多读半行 ⇒ ≈ 行长 × 块数 ≪ 数据量），但**不构成与文件
//! 大小无关的界**；后者（择向采样）≤ 128 KiB × 文件数，那一条才真的是小量）。
//! **不要**把它读成"任何输入下总代价有常数上界"：**完全可解析**的超大文件，本闸**一条都不拦**。
//!
//! ⚠️ **同轮订正（评审实测，原句是错的）**：本段原写"完全可解析的代价量级 =
//! `min(窗口左端之后的行数, 窗口右端之前的行数)`"（即假定择向后读到窗口另一侧就早退）。
//! **实测不成立**：早退靠 [`ScanState::note_stop_side_line`] 的**连续**计数，而**任何一条非早退侧
//! 的行都会把它清零**（见该函数体里那行 `stop_side_bytes = 0`）⇒ 只要文件里"窗口内 / 窗口外"交替出现，
//! 早退**一次都不会发生**，代价直接变成**整份文件**。实测（80 000 行 / 7.52 MB，窗口在正中）：
//! 原句给的界 `min = 40 000`，实际 `lines_read = 80 000`（**2×**）；200 000 行 / 18.8 MB 同形，
//! 实读 200 000 行、墙钟 **2.89 s**。
//!
//! ⇒ **完全可解析内容的读入量在代码上**没有**硬上界**，上界只由"该方向剩余多少行"决定
//! （最坏 = 日志目录里被扫到的全部行）。这与**契约字面**存在偏离，已登记为待裁项：
//! 契约 `display-proto/src/log.rs` 的 `LOG_SCAN_MAX_LINES` 与设计 §4.4 写的是
//! 「单次请求**总行数**上限 50 000」/「限额扫描（≤5 files，≤50 000 lines）」，
//! 而本实现自整改三起把它解释为「**窗口内**行数上限」（[`LOG_SCAN_MAX_LINES`] 喂 `window_lines`
//! 判据 —— ⚠️ **订正（整改五 D-4）**：它**不是只喂**这一处，同时还是硬代价闸的行数阈
//! [`SCAN_READ_BUDGET_LINES`] = `× 4` 的**输入**）—— 这是**为了修掉 R3 那个"1 ms 窗口也报超限"
//! 的假阳性**而做的取舍：
//! 按"扫了多少行"拒绝会把**已经查全**的窄窗口误报成"不完整"。两种解释不能同时满足，
//! **须由产品/契约裁定**（改契约措辞，或补一条真正映射到 `range_too_large` 的总行数上界）。
//! 现状的缓解因素：HMI 的热路径（500 ms 增量轮询）是**贴尾窗口**，走倒读、代价为常数
//! （用例 [`tests::a_window_at_the_tail_of_a_huge_file_must_pick_backward_and_stay_cheap`] 把守）；
//! 代价大的只有用户主动拉大时间范围的**低频**操作，且渲染端 5 s 超时不会把界面挂死。
//!
//! ## 超限时**回什么**：两条路径的返回形态**不同**（整改五 A 组裁定，逐字写死）
//!
//! | 触发 | `entries` | 其它字段 | 理由 |
//! |------|-----------|----------|------|
//! | **窗口内容 / 文件数超限**（EDGE-15、`ScanState::too_large`） | **已收集的最新 `limit` 条** | `range_too_large=true`，`has_more`/`next_cursor` 同正常路径的算法 | 见 [`ScanState::take`]：这是**用户真正要的内容**，回空页会让 HMI 把列表**清空**并常驻「未执行检索」，而 1 h 已是最小档 ⇒ "再缩也没用"；渲染端 `p3_logs.rs:728` 本就支持"有条目 + 超限提示"形态 |
//! | **硬代价闸耗尽**（`ScanState::budget_exhausted`） | **空** | `range_too_large=true`、`has_more=false`、`next_cursor=None` | 方案 (a)，上一轮已评审通过；成因是"内容读不懂/读不动"（见下），与用户要的内容无关 ⇒ 不回半截 |
//!
//! ⚠️ **如实登记的待裁**：两条路径的 `range_too_large` **契约语义完全相同**（"`entries` 不代表
//! 完整结果"），返回形态却一个带条目、一个空页 ⇒ **是否统一待 PM 裁定**（见
//! [`too_large_page`] 的 doc）。本轮**不动**硬闸形态。
//!
//! **命中硬闸时为什么不是 `Err` → 503（方案 (b)）**：回 `range_too_large=true`（**方案 (a)**），
//! **并**配一条 `tracing::warn!` 点名真实成因（不可解析 / 预算耗尽）。理由：
//!
//! - 契约对 `range_too_large` 的定义是"`entries` **不代表完整结果**"（`display-proto/src/log.rs` 行 103）
//!   ⇒ 硬闸命中时**确实**没查完，这个字段**说的就是实话**（不是谎报）；
//! - 走 `Err` 会让**整页**显示"不可用"，而现场很可能是"最新的文件好、更旧的那个被压过" ⇒
//!   (b) 把"部分源不可读"升级成"日志页不可用"，**代价更大**；
//! - 屏上文案「检索范围超限，请缩小时间范围」对用户**略有误导**（缩小窗口并不能解决格式漂移）——
//!   这一点**如实登记**：机器细节（不可解析 / 预算耗尽）只进 `tracing::warn!`，**不上屏**，
//!   与 R4 的口径一致。
//!
//! **绝不**回 `entries=[] ∧ range_too_large=false`（那会把"没查完"说成"确实没有"，本项目硬红线）。
//! 这条对**闸门耗尽的输入**是硬保证（用例 [`tests::a_multi_mibibyte_single_line_hits_the_byte_gate_and_never_claims_empty`]
//! 与 [`tests::a_newline_free_large_file_hits_the_byte_gate_and_never_claims_empty`] 把守）；
//! 对**没耗尽**的输入，空页的含义是"扫完了、确实没有**可解析的**条目"——**除**上文
//! 「残余的漏读（如实登记）」登记的形态（连续 ≥ 64 KiB 的窗口外内容之后仍有窗口内的行 ⇒ 那些行
//! **会被漏掉**）**之外**，这句话才成立 —— 这正是 R4 的口径
//! （读到了行却一行都解析不出来 ⇒ `tracing::warn!`，机器细节不上屏）。
//!
//! # 扫描方向（R3 整改的第二半：**代价有界**）
//!
//! 要的是**最新**几条，而它们大概率在**文件尾**；逐行从头读到尾 ⇒ 每个请求（HMI 每 500 ms 一次）
//! 都把整个文件读一遍 + 每行一次 JSON 解析。现按**窗口位置**择向（每文件先各采样 64 KiB 的首/尾
//! 块取首个/末个可解析时间戳）：
//!
//! - 窗口靠文件尾（HMI 的 `1h`/`24h`/`cursor` 增量：**主路径**）⇒ **从尾倒读**，遇到
//!   `ts < start` 的**连续**内容超过一个迟滞块即停止**本文件**；
//! - 窗口靠文件头（自定义的早期区间）⇒ **从头正读**，遇到 `ts > end` 的**连续**内容超过迟滞块即停止该文件。
//!
//! ⚠️ **订正（整改五 D-1）**：本段原写"两种方向的读入量都 ≤
//! `min(窗口右端之前的行数, 窗口左端之后的行数)`（+ 一个 64 KiB 块）⇒ 对 60 000 行的文件，
//! **任一窗口位置都 ≤ 约 3 万行**"——**该界被本文件上文「同轮订正（评审实测，原句是错的）」自己
//! 证伪**（实测 2×；"窗口内/窗口外"交替时早退一次都不发生 ⇒ 退化为**整份文件**）⇒ 此处不再复述
//! 那个界。**现行口径与上文一致**：完全可解析内容的读入量**没有**硬上界，只由"该方向还剩多少行"
//! 决定（最坏 = 日志目录里被扫到的全部行）；HMI 主路径（贴尾窗口）走倒读、代价为常数。
//! 择向结果按**每文件**计入 [`ScanStats::backward_files`] / [`ScanStats::forward_files`]
//! （R3-③：让"尾部窗口必须走倒读"这条**代价承诺**可断言，而不是靠"我记得写了"）。
//!
//! ⚠️ **整改五 D-2（本轮）**：**正读**此前用 `BufReader::lines()` ⇒ **单行无上限物化** ——
//! 同类缺陷的**最后一条漏网**（倒读有读窗上限 + 字节闸，`/logs/targets` 有 C 组整改，
//! 只有正读没有；且正读**先物化、后**截断，`note_line_read` 只 +1 ⇒ 两道闸一个都不拦）。
//! 现在**两个方向共用同一个有界按行读** [`BoundedLineReader`]：单行 ≤
//! [`MAX_REVERSE_WINDOW_BYTES`]、块读 ≤ [`REVERSE_CHUNK_BYTES`]（**共用一份代码，不再有第三份抄写**）、
//! **超限的行整行跳过并按字节计入字节闸**
//! （[`ScanState::note_unparsable_bytes`]，与倒读**同一种账**：不记行、不编号）。
//! ⇒「代价有界」这句对**读不懂 / 读不动**的输入（超长行、无换行块、二进制垃圾）**在两个方向
//! 上都成立**；对**完全可解析**的内容仍然**不成立**（见上「同轮订正（评审实测，原句是错的）」）。
//!
//! ⚠️ **复核实测订正（本轮）**：原写"单行 ≤ 上限 ⇒ 两个方向对同一份文件给出**同一套条目**"——
//! **说反了，已删**。同一上限在两个方向的可容行长**差 1 字节**：正读能收下**行长 ≤ 上限**的整行；
//! 倒读要在 ≤ 上限的窗口里同时容下该行**和它行尾的 `'\n'`** ⇒ **行长 ≥ 上限的行结构性装不下**。
//! 结局也不同：正读只**跳过那一条**、邻居照常交付；倒读把它计入字节闸，预算耗尽 ⇒ **整页拒绝
//! （邻居一起丢）**。实测（复核轮正/倒读对拍）：恰 1 MiB 的行 —— 正读 `rt=false` 正常交付，
//! 倒读 `rt=true` 空页。**两侧都是可见拒绝、绝不静默丢条**，但**条目集确实不同** ⇒ 已登记为待裁项。
//! 把守用例 = [`tests::forward_read_skips_an_overlong_line_and_keeps_its_neighbours`]、
//! [`tests::forward_read_charges_a_huge_skipped_line_to_the_byte_gate`]。
//!
//! ⚠️ **B-3 整改（早退只停本文件 + 迟滞）**：整改前 `ts < start` 直接返回 [`Step::EndOfScan`]，
//! 被上层升级为"**停止整个请求**"⇒ 一条时间戳偏小的行会让**本文件更早的全部行 + 所有更旧的文件**
//! 一起不被看 ⇒ 屏上显示 EDGE-08「当前筛选条件下没有日志」，而事实是"根本没去找"
//! （本项目硬红线：**"确实没有"与"没去找"不可区分**）。现在：
//!
//! 1. 命中早退条件只返回 [`Step::EndOfFile`]（**只停本文件**），更旧的文件**照常扫**；
//! 2. 早退加**迟滞**：连续 ≥ [`EARLY_STOP_HYSTERESIS_BYTES`]（一个 64 KiB 块量级）的窗口外内容
//!    才停，中间只要再出现一条**不满足早退条件**的行就清零 ⇒ 单条乱序行最多让本文件**多读**一个块；
//! 3. 正读的镜像条件（`ts > end`）按**同一**标准处理。
//!
//! **残余的漏读（如实登记，不许说成"没有"）**：迟滞只把"单条乱序行"挡掉；若文件里存在
//! **连续 ≥ 64 KiB 的窗口外内容**（成因不止"写者重排的那几毫秒"——**时钟回拨**是最现实的成因：
//! 无 RTC 的设备首次 NTP 同步会把时间戳整体拉回，可形成很长一段"比窗口还旧"的行），
//! 而这段内容**之后**（文件更早位置）仍有落在窗口内的行 ⇒ 那些行**会被漏掉**（本文件更早的全部行）。
//! 与逐行全读的旧实现相比，这是**用代价换有界**。
//!
//! ⚠️ **订正（整改三，评审实测）**：此处原写「更旧的文件**不再**受影响」——**实测不成立**。
//! 迟滞计数 [`ScanState::stop_side_bytes`] 是**逐文件**语义，而整改前它**跨文件复用**：
//! 文件 A 早退时留下的 ≥ 64 KiB 计数会让文件 B 的**第一条**早退侧行**立刻达阈** ⇒ B 被整体跳过
//! （复核实测：双文件样本回 `msgs=["in-newer-file"]`，B 里那条窗口内的行**丢失**，且
//! `range_too_large=false` ⇒ 屏上一条"看着正常但少一条"的列表，**用户无从察觉**）。
//! 现在 `scan_file` 入口显式重置**全部逐文件状态**（[`ScanState::begin_file`]），
//! 该句才成立；把守用例 = [`tests::a_previous_files_hysteresis_must_not_skip_the_next_file`]。
//!
//! ## ⚠️ 架构提示（整改三补注，**只在此处写，不改设计文档**）：这是 **ring 缺失期的权宜**
//!
//! **现实**：文件扫描层**无法**廉价地服务"尾部窗口"——而尾部窗口正是 HMI 每 500 ms 增量拉取的
//! **主路径**（`cursor` + `1h`）。正向扫必须一路读到**文件尾**才知道最新几条在哪；
//! 想便宜就得倒读（`choose_direction` / `read_backward` / 迟滞这套），而倒读又要处理块边界、
//! 超长行、非单调时间戳、进度守卫……本文件一半的复杂度、以及**连续三轮评审抓到的缺陷**，
//! 都长在这条分界上。
//!
//! **设计本意**：这个需求归 **ring** —— 设计 §4.4「实时推送 ≤ 2 s」那一半（有界 `VecDeque` 定容，
//! 最新条目**直接可得**，既不倒读也不碰文件）。而 ring 在本单元（H）**未实现**，已拆为独立单元 **H-2**。
//!
//! **结论（登记，不是建议扩算法）**：`choose_direction` / `read_backward` / 早退迟滞这套机制是
//! **ring 缺失期的权宜之计**。H-2（ring）落地后，应当**重新评估是否把整套倒读路径整体删除**
//! （保留一条正向扫的冷路径给"历史检索"即可）—— **不要**把它当成长期资产继续加固。
//!
//! ⚠️ 限额取自 `config.display.log`（[`LogLimits`]）——它是设计 §8.3「log 限额」的**配置真源**
//! （默认值 = 本模块的契约常量）。但 `page_limit_max` **不**作为 `limit` 的接受上限：契约的
//! `limit ≤ LOG_PAGE_LIMIT_MAX`(200) 是**跨侧硬约束**（渲染端的响应体上限 `MAX_BODY_BYTES`
//! 按 `LOG_PAGE_LIMIT_MAX × 1 KiB` 编译期推算，`console.rs:131`）⇒ 若 yaml 把它调大，屏上会
//! 出现"合法回包被判 `BodyTooLarge`"。故本模块**硬用契约常量**校验 `limit`，并在装配时对
//! `page_limit_max > LOG_PAGE_LIMIT_MAX` 发一条 `tracing::warn!`（**如实、不静默**）。
//!
//! # 单条消息 ≤ 1 KiB（跨侧约定，2026-09-15 补注）
//!
//! `display-proto` 的 `LogEntry.message` **无长度约束**，而渲染端的响应体上限按
//! 「`LOG_PAGE_LIMIT_MAX`(200) × 单条 ≤ 1 KiB」推算（`console.rs` 的
//! `ASSUMED_MAX_MESSAGE_BYTES = 1024`，带编译期 `assert!`）⇒ **服务端必须限制单条 ≤ 1 KiB**。
//! 超出即**截断并显式标注**：保留能放进 `1024 - 3` 字节的最长**字符边界前缀**，尾部追加
//! [`TRUNCATION_MARKER`]（ASCII `...`，**在生成字体码表内** —— `…`(U+2026) 不在，
//! 见 `p3_logs.rs` LG1 的同款处置）。截断**可见、不静默**（§2.6「降级可见、绝不造假」）。
//!
//! # 只读
//!
//! 两条端点都是 `GET`；路由由 `console_host::ConsoleHost::router` 按
//! `ConsoleEndpoint::method()` 结构性注册 ⇒ 写方法打到这两条路径得 **405**（不是 404、更不是执行）。
//! 本模块**不持有任何可变状态**（无缓存、无计数器落盘）⇒ 无副作用。
//!
//! **文件在"列目录"与"打开"之间消失**（轮转 / 被清理，R7 整改）：该文件**跳过**（`NotFound`），
//! 其余 IO 错误仍走 `Err` ⇒ 503。整改前一次轮转就可能让整条日志端点 503。
//!
//! # 可观测性（R4）
//!
//! 读到了行但**一行都解析不出来**（`lines_read > 0 ∧ parsed_lines == 0`）⇒ `tracing::warn!`。
//! 若不喊，日志格式整体漂移（JSON 层被换掉 / 字段改名）时屏上会显示 EDGE-08「无日志」，
//! 与"确实没有日志"**不可区分**且无从排障。**机器细节只进 `tracing`，不上屏**。
//!
//! # 待裁（登记，本轮**不做**）
//!
//! 1. `display.log.page_limit_max` **零消费者**（全仓只有定义 / 默认值 / 它自己的测试 + 本文件的
//!    一条 `warn!`）⇒ 删字段还是接语义（如按它限制 `limit`）待 PM 裁；
//! 2. `next_cursor` **无法回喂**（见上「契约缺口」）⇒ "向前翻历史"在现契约下结构性不可达；
//! 3. `/logs/targets` 返回裸 `Vec<String>` ⇒ **"被截断"不可表达**（> 50 项时屏上无从知晓），
//!    且截断取**字典序**前 50 ⇒ 非 ASCII 开头的 target 被**系统性**丢弃；
//! 4. `LogEntry.target` **无长度上限**（契约只约束了 `message` 的跨侧 1 KiB 约定）；
//! 5. 设计 §4.1 F10 自称日志真源 `/var/log/mupc/mupc.log` —— 与 `--log-dir` / `config.system.log_dir`
//!    的**默认值**（`/opt/mupc/logs`）**两者都不符**（设计文档缺陷，本单元只登记不改文档）；
//! 6. **正读 / 倒读的条目集在"超长行"处分叉**（复核实测，见上「复核实测订正」）：可容行长差
//!    1 字节，且行长 ≥ 上限时倒读是**整页拒绝**、正读只**跳过那一条**。两侧都可见、都不静默，
//!    但同一份文件两个方向给不同结果 —— 要不要对齐、怎么对齐（改谁的窗口/记账）待 PM 裁；
//! 7. **A 组裁定引入的代价形态**：计数移到"全部筛选之后" ⇒ **极宽窗口 + 选择性筛选**
//!    （如 `range` 覆盖整份文件 + `levels=[ERROR]` 而一条 ERROR 都没有）时 `window_lines` 恒为 0、
//!    超限闸**永不触发**，代价 = "该方向剩余的全部行"。**实测**（复核轮实测，
//!    debug 构建）：200 000 行 / 18.8 MB，`lines_read = 200 000`、`window_lines = 0`、
//!    `rt = false`、冷读墙钟 **15.2 s**（预热 3.2 s）。**与 `max_lines` 完全无关**。
//!    ⚠️ **HMI 热路径不受影响**（贴尾窗口走倒读、代价为常数），受影响的是用户主动拉大范围的
//!    **低频**操作，且渲染端 5 s 超时会先打掉这次查询。要不要补一道与 EDGE-15 正交的软闸待 PM 裁；
//! 8. **超限两条路径的返回形态不一致**：窗口内容超限 ⇒ **带条目**（A 组裁定）；硬代价闸
//!    （`budget_exhausted`）⇒ **仍回空页**。两者 `range_too_large` 的契约语义相同 ⇒ 是否统一待裁。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use mupc_display_proto::config::LogLimits;
use mupc_display_proto::log::{
    LogEntry, LogLevel, LogPage, LogRange, LOG_PAGE_LIMIT_MAX, LOG_SCAN_MAX_FILES,
    LOG_SCAN_MAX_LINES, LOG_TARGETS_MAX,
};

/// 日志文件名前缀（`tracing_appender::rolling::daily(dir, "mupc.log")` ⇒ `mupc.log.YYYY-MM-DD`）。
const LOG_FILE_PREFIX: &str = "mupc.log";

/// 单条消息的字节上限（= 渲染端 `console.rs::ASSUMED_MAX_MESSAGE_BYTES`）。
///
/// ⚠️ **两处是同一约定的两份抄写**（跨 crate：`local-display` 不是本 crate 的依赖，无法共享常量）
/// ⇒ 由本文件 `message_cap_matches_render_side_assumption` 用例把字面量钉死；渲染端另有
/// 编译期 `assert!`（`console.rs:131`）。任一侧改动必须**同时**改另一侧。
pub const MESSAGE_MAX_BYTES: usize = 1024;

/// 截断标注（**必须**在生成字体码表内：`.` = U+002E ✓；`…` = U+2026 ✗）。
/// 与渲染端 `p5_audit::TEXT_ELLIPSIS` / `p3_logs` 的消息列 `DOTS` 尾部标记**同款**。
pub const TRUNCATION_MARKER: &str = "...";

// ═══════════════════════════════════════════════════════════════════════════
// 1. 查询（解析 + 校验）
// ═══════════════════════════════════════════════════════════════════════════

/// 解析后的日志查询（设计 §4.4：`LogQuery` 去掉 `keyword`，加 `cursor` / 多值筛选）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogQuery {
    /// 级别多选（空 = 不按级别筛）。
    pub levels: Vec<LogLevel>,
    /// 模块多选（`tracing` target 原始键；空 = 不按模块筛）。
    pub targets: Vec<String>,
    /// 时间范围档位。
    pub range: LogRange,
    /// 自定义起始（Unix ms；仅 `custom` 档）。
    pub from_ms: Option<u64>,
    /// 自定义结束（Unix ms；仅 `custom` 档）。
    pub to_ms: Option<u64>,
    /// 增量游标（`None` = 取最新一页）。
    pub cursor: Option<u64>,
    /// 单次返回条数上限（1..=[`LOG_PAGE_LIMIT_MAX`]）。
    pub limit: usize,
}

impl Default for LogQuery {
    fn default() -> Self {
        Self {
            levels: Vec::new(),
            targets: Vec::new(),
            range: LogRange::H1,
            from_ms: None,
            to_ms: None,
            cursor: None,
            limit: LOG_PAGE_LIMIT_MAX,
        }
    }
}

impl LogQuery {
    /// 有效时间窗口（**含两端**）。
    ///
    /// `1h` / `24h` 由 `now_ms` 现算（**服务端算相对窗口**，UI 不读时钟 —— `p3_logs.rs` 行 658）；
    /// `custom` 的 `from`/`to` 由 [`parse_query`] 保证**齐备**（缺任一即拒）。
    pub fn window(&self, now_ms: u64) -> (u64, u64) {
        match self.range {
            LogRange::H1 => (now_ms.saturating_sub(3_600_000), now_ms),
            LogRange::H24 => (now_ms.saturating_sub(86_400_000), now_ms),
            LogRange::Custom => (
                self.from_ms.unwrap_or(0),
                self.to_ms.unwrap_or(u64::MAX),
            ),
        }
    }
}

/// 解析 `(键, 值)` 序列（**重复键 = 多值**，§3.4 补注；**不是**逗号拼接）。
///
/// # 校验口径（本单元拍板，逐条给理由）
///
/// | 情形 | 处置 | 理由 |
/// |------|------|------|
/// | `levels=error,warn` | **拒**（非 5 个线上名之一） | §3.4 补注：多值一律**重复键**；容错地把逗号当分隔符等于**自造第二种编码**，两侧口径必然漂移 |
/// | 未知键 | **拒** | 静默忽略一个筛选参数 = 在"用户以为已筛"的画面下给**未筛**的数据（本项目最忌的静默失实） |
/// | `range` 缺省 | 取 `1h` | 渲染端 `LogQuery::default()` 即 `H1`；缺省不是非法 |
/// | `range != custom` 且给了 `from`/`to` | **拒** | 窗口只能有一个真源。"档位优先、忽略 from/to" 会让**用户明确要求的窗口被静默放宽** ⇒ 给出比请求更多的数据 |
/// | `range=custom` 缺 `from` 或 `to` | **拒** | 半截窗口无意义（渲染端仅在两侧齐备时才发这对键，见 `control_route.rs:364`） |
/// | `from > to` | **拒** | 空窗口是"静默无结果"的伪装（应为显式非法） |
/// | `limit` 缺省 | 取 [`LOG_PAGE_LIMIT_MAX`] | 契约上限即默认上限（不放大） |
/// | `limit = 0` / `> 200` | **拒**（**不静默截断**） | 契约明写 `limit ≤ 200`；静默改小会让调用方以为拿到了它要的条数 |
/// | `targets=` 空值 | **拒** | 空 target 匹配不到任何条目 ⇒ 静默空页；渲染端不发空值键（空集 = 不发键） |
pub fn parse_query(pairs: &[(String, String)]) -> Result<LogQuery, String> {
    let mut q = LogQuery::default();
    let (mut from, mut to) = (None, None);
    let mut saw_range = false;
    let mut limit: Option<usize> = None;

    for (k, v) in pairs {
        match k.as_str() {
            "levels" => q.levels.push(parse_level(v)?),
            "targets" => {
                if v.is_empty() {
                    return Err("targets 的值不得为空（空集应不发该键）".to_string());
                }
                q.targets.push(v.clone());
            }
            "range" => match v.as_str() {
                "1h" => {
                    q.range = LogRange::H1;
                    saw_range = true;
                }
                "24h" => {
                    q.range = LogRange::H24;
                    saw_range = true;
                }
                "custom" => {
                    q.range = LogRange::Custom;
                    saw_range = true;
                }
                other => return Err(format!("range 非法: {other}（合法: 1h|24h|custom）")),
            },
            "from" => from = Some(parse_ms(v, "from")?),
            "to" => to = Some(parse_ms(v, "to")?),
            "cursor" => q.cursor = Some(parse_ms(v, "cursor")?),
            "limit" => {
                let n: usize = v
                    .parse()
                    .map_err(|_| format!("limit 不是非负整数: {v}"))?;
                limit = Some(n);
            }
            other => return Err(format!("未知查询参数: {other}")),
        }
    }
    let _ = saw_range; // `range` 缺省合法（默认 H1）——显式记录该事实，避免"忘了分支"

    // `cursor` 与 `from`/`to` 共用 `parse_ms`（同为非负整数毫秒）
    match q.range {
        LogRange::Custom => match (from, to) {
            (Some(f), Some(t)) => {
                if f > t {
                    return Err(format!("custom 窗口非法: from({f}) > to({t})"));
                }
                q.from_ms = Some(f);
                q.to_ms = Some(t);
            }
            _ => return Err("range=custom 必须同时给 from 与 to".to_string()),
        },
        _ => {
            if from.is_some() || to.is_some() {
                return Err(
                    "range 与 from/to 冲突：相对档位（1h/24h）下窗口由档位算出，不得再给 from/to"
                        .to_string(),
                );
            }
        }
    }

    let n = limit.unwrap_or(LOG_PAGE_LIMIT_MAX);
    if n == 0 || n > LOG_PAGE_LIMIT_MAX {
        return Err(format!(
            "limit 越界: {n}（合法区间 1..={LOG_PAGE_LIMIT_MAX}）"
        ));
    }
    q.limit = n;
    Ok(q)
}

fn parse_level(v: &str) -> Result<LogLevel, String> {
    match v.to_ascii_lowercase().as_str() {
        "error" => Ok(LogLevel::Error),
        "warn" => Ok(LogLevel::Warn),
        "info" => Ok(LogLevel::Info),
        "debug" => Ok(LogLevel::Debug),
        "trace" => Ok(LogLevel::Trace),
        other => Err(format!(
            "levels 非法: {other}（合法: error|warn|info|debug|trace；多值用重复键）"
        )),
    }
}

fn parse_ms(v: &str, field: &str) -> Result<u64, String> {
    v.parse::<u64>()
        .map_err(|_| format!("{field} 不是合法的非负毫秒数: {v}"))
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. 扫描统计 / 扫描状态（**仅测试可见**：限额"未继续扫"的实测证据）
// ═══════════════════════════════════════════════════════════════════════════

/// 一次扫描的实际工作量。
///
/// 存在的唯一理由：把"**超限即停止**""**代价有界**"从"我保证"变成"**可断言的事实**"。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// 实际**打开并读取**的日志文件数。
    pub files_scanned: usize,
    /// 实际**读入**的行数（含解析失败 / 被筛掉 / 落在窗口外的行）。
    ///
    /// ⚠️ **整改三起，它只是一条统计**：B-2 的硬闸**不再**以它为判据（那正是 R3 判过的
    /// "由扫了多少行决定 `range_too_large`"）⇒ 见 [`ScanStats::unparsable_lines`]。
    pub lines_read: usize,
    /// 实际**落在查询时间窗口内**的行数 —— **EDGE-15 的判据**（见模块头）。
    pub window_lines: usize,
    /// 其中**可解析**成条目的行数（R4：`lines_read > 0 ∧ parsed_lines == 0` ⇒ 日志格式漂移）。
    pub parsed_lines: usize,
    /// **解析失败**的行数 —— B-2 硬代价闸（[`SCAN_READ_BUDGET_LINES`]）的判据（整改三订正）。
    pub unparsable_lines: usize,
    /// 读了却**没产出任何行**的字节数 —— B-2 字节闸（[`SCAN_READ_BUDGET_BYTES`]）的判据
    /// （整改三补：倒读的翻窗重读 + "行长 > 读窗上限"的跳过片段，过去**完全不计账**）。
    pub skipped_bytes: u64,
    /// 实际走**倒读**（含尾部窗口的主路径）的文件数（R3-③：让择向**可断言**）。
    ///
    /// 存在的理由：择向只影响**代价**、不影响语义 ⇒ 把 `choose_direction` 写死 `Forward` 时，
    /// 语义类断言**全绿**（复核实测：35 条全绿）⇒ "尾部窗口便宜"这条主路径承诺**无人守**。
    pub backward_files: usize,
    /// 实际走**正读**的文件数（同上，另一个方向）。
    pub forward_files: usize,
}

/// 扫描方向（见模块头「扫描方向」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanDirection {
    /// 从头正读（窗口靠文件头）；`ts > end` ⇒ 本文件剩余部分不用看。
    Forward,
    /// 从尾倒读（窗口靠文件尾，HMI 主路径）；`ts < start` ⇒ 本文件与更旧的文件都不用看。
    Backward,
}

/// 单行的处理结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// 继续。
    Continue,
    /// 本文件剩余部分都在窗口外 / 已越过本文件的窗口边界 ⇒ **只停本文件**，换下一个（更旧）文件。
    ///
    /// ⚠️ **B-3**：整改前有一个 `EndOfScan`（"更旧的文件也不用看"）—— 它让**一条**时间戳偏小的行
    /// 把**整个请求**提前收工（更旧文件里窗口内的行再也拿不到 ⇒ 屏上假"无日志"）。该变体已**删除**：
    /// 单调性只对**文件内**近似成立，跨文件**不成立**（旧文件的日期早 ≠ 它的行时机都更早，
    /// 时钟回拨时更是相反）⇒ 本模块**不再**基于"文件更旧"做任何跨文件早退。
    EndOfFile,
    /// 窗口内容超过行数限额 ⇒ EDGE-15（**或** B-2 的硬代价闸耗尽 ⇒ 同为 `range_too_large`）。
    TooLarge,
}

/// 一次请求的扫描状态（**唯一**的可变状态；`LogService` 本身无状态）。
struct ScanState<'a> {
    q: &'a LogQuery,
    start: u64,
    end: u64,
    max_lines: usize,
    /// 保留"最新 `limit+1` 条"⇒ 本字段的**内存上界 = `limit+1` 条，与文件大小无关**。
    ///
    /// ⚠️ **整改五 B 组订正**：上面这句**只对本字段成立**，不构成整个 [`ScanState`] 的上界 ——
    /// 同毫秒组 `group`（见下）整改前**无长度判据**，它的条数上界才是决定性的；现已在
    /// [`ScanState::backward_line`] 的 `push` 前加与 `max_lines` 同源的守卫 ⇒ 现在
    /// `kept ≤ limit+1 条 ∧ group ≤ max_lines 条`，两句合起来才是本结构的内存口径。
    ///
    /// 键 = **`(seq, 文件序)`**（R6）：跨文件同毫秒的 `seq` 相同，唯此才能两条都留
    /// （单键会互相覆盖 ⇒ 丢条 + 内容错）。文件序**越大越新** ⇒ 同 `seq` 时新文件排在前。
    kept: BTreeMap<(u64, u32), LogEntry>,
    stats: ScanStats,
    too_large: bool,
    /// B-2：**硬代价闸**（行数闸 [`SCAN_READ_BUDGET_LINES`] **或**字节闸
    /// [`SCAN_READ_BUDGET_BYTES`]）是否已耗尽 ⇒ 回 `range_too_large`，
    /// 但成因**不是**窗口跨度（见模块头「硬代价闸」）。
    budget_exhausted: bool,
    /// B-3 早退迟滞：自上次"不满足早退条件"的行起，累计的窗口外字节数（**逐文件**，
    /// 见 [`ScanState::begin_file`]）。
    stop_side_bytes: u64,
    /// 倒读路径的"同毫秒组"（`k` 必须与正读方向一致，见下 `flush_group`）。
    ///
    /// ⚠️ **长度上界 = `max_lines` 条**（B 组整改；守卫在 [`ScanState::backward_line`] 的 `push`
    /// 之前）—— 整改前无判据 ⇒ 同毫秒巨量行会把内存吃满（每条 2 个 `String`，
    /// 且 `message` 尚未截断）。
    group_ts: Option<u64>,
    group: Vec<(LogLevel, String, String)>,
}

impl ScanState<'_> {
    fn new<'a>(q: &'a LogQuery, start: u64, end: u64, max_lines: usize) -> ScanState<'a> {
        ScanState {
            q,
            start,
            end,
            max_lines,
            kept: BTreeMap::new(),
            stats: ScanStats::default(),
            too_large: false,
            budget_exhausted: false,
            stop_side_bytes: 0,
            group_ts: None,
            group: Vec::new(),
        }
    }

    /// 开始扫描**下一个文件**：重置全部**逐文件**状态。
    ///
    /// # 为什么必须在这里集中重置（整改三，评审抓到的阻塞缺陷）
    ///
    /// [`ScanState`] 是**一次请求**的状态，而其中的字段分两类 —— 必须逐一点名，不能"顺手只清一个"：
    ///
    /// | 字段 | 语义 | 何时重置 |
    /// |------|------|----------|
    /// | `stop_side_bytes` | **逐文件**（迟滞是"本文件剩余部分都在窗口外"的判据） | **此处** |
    /// | `group_ts` / `group` | **逐文件**（倒读的同毫秒组，收尾由 `flush_group` 负责） | **此处**（防御性） |
    /// | `stats` / `kept` / `too_large` / `budget_exhausted` | **逐请求**（跨文件累计） | 不重置 |
    ///
    /// 整改前 `stop_side_bytes` **跨文件复用** ⇒ 文件 A 早退时留下的 ≥ 64 KiB 计数让文件 B 的
    /// **第一条**早退侧行立刻达阈 ⇒ **B 被整体跳过**（B 里更早位置的窗口内行永远读不到），
    /// 而回包是 `entries=[...] ∧ range_too_large=false` ⇒ 屏上一条"看着正常但少一条"的列表，
    /// **用户无从察觉**。把守用例 = [`tests::a_previous_files_hysteresis_must_not_skip_the_next_file`]。
    ///
    /// `group_ts` / `group` 在正常路径下已由 `flush_group` 清空（唯一例外是**终止整个请求**的
    /// `Step::TooLarge`，此时整页被丢弃）⇒ 此处的清空是**防御性**的，不是修某个已知缺陷。
    fn begin_file(&mut self) {
        self.stop_side_bytes = 0;
        self.group_ts = None;
        self.group.clear();
    }

    /// 计一行"读入"（**纯统计**，B-2 硬闸**不再**以它为判据 —— 见 [`ScanState::note_unparsable_line`]）。
    ///
    /// ⚠️ 整改三订正：整改二用它当硬闸，于是"扫了多少行"又一次决定了 `range_too_large`
    /// （= R3 被 REQUEST_CHANGES 的**同一句**话）。实测反例见模块头「硬代价闸」。
    fn note_line_read(&mut self) {
        self.stats.lines_read += 1;
    }

    /// 计一行**解析失败**（B-2 硬代价闸的**行数**判据）+ 硬闸是否耗尽。
    ///
    /// **只有解析失败的行计入**：可解析的行由 EDGE-15 的窗口内容判据裁决（R3 裁定），
    /// 它们既不解释成"没查完"、也不该把一份 500 000 行的正常日志判成超限。
    fn note_unparsable_line(&mut self) -> Step {
        self.stats.unparsable_lines += 1;
        if self.stats.unparsable_lines > SCAN_READ_BUDGET_LINES {
            self.budget_exhausted = true;
            return Step::TooLarge;
        }
        Step::Continue
    }

    /// 计 `n` 个**读了却没产出任何行**的字节（B-2 硬代价闸的**字节**判据）+ 硬闸是否耗尽。
    ///
    /// 用在哪：倒读的**翻窗重读**（块内凑不齐一整行 ⇒ 丢掉本轮读入、翻倍重读）与
    /// **"行长 > 读窗上限"的跳过片段**。这两处整改前**完全不计账** ⇒ 行数闸对它们永不触发
    /// （复核实测：4 MiB 无换行文件 `lines_read=1, read_bytes=8_257_536` = **2.0×**，且回
    /// `entries=[] ∧ range_too_large=false`）。返回"是否已耗尽"。
    fn note_unparsable_bytes(&mut self, n: u64) -> bool {
        self.stats.skipped_bytes = self.stats.skipped_bytes.saturating_add(n);
        if self.stats.skipped_bytes > SCAN_READ_BUDGET_BYTES {
            self.budget_exhausted = true;
            return true;
        }
        false
    }

    /// B-3 迟滞：本行落在**早退侧**（正读的 `ts > end` / 倒读的 `ts < start`）。
    /// 返回"连续窗口外内容是否已达迟滞阈值"（达阈 ⇒ 可以停本文件）。
    fn note_stop_side_line(&mut self, line: &str) -> bool {
        self.stop_side_bytes = self.stop_side_bytes.saturating_add(line.len() as u64 + 1);
        self.stop_side_bytes >= EARLY_STOP_HYSTERESIS_BYTES
    }

    /// B-3 迟滞：本行**不满足**早退条件（含窗口内、更早侧、级别/游标被筛掉的行）⇒ 清零。
    fn note_not_stop_side_line(&mut self) {
        self.stop_side_bytes = 0;
    }

    /// 一条**已确定 `k`** 的窗口内行 ⇒ 筛选 → 算 `seq` → **计数** → 入 `kept`。
    ///
    /// # `window_lines` 的计数位置（**整改五 A 组裁定：移到全部筛选之后**）
    ///
    /// 现口径：`window_lines` 只统计**本次请求真正要交付的内容规模** —— 级别 / 模块 / `cursor`
    /// 三个筛选**筛掉的行不计入**。
    ///
    /// **推翻的旧口径（如实登记，不静默改口）**：本函数此前写的是"`window_lines` 在级别/模块筛选
    /// **之前**计数 —— EDGE-15 判的是检索范围的**内容规模**，与用户勾了哪几个级别无关"。那句话
    /// **在 HMI 的主路径上是错的**，实测缺陷如下：
    ///
    /// - 1 h 窗口的繁忙日志（> 50 000 行 ≈ 14 行/秒）下，渲染端每 500 ms 的增量轮询
    ///   （`range=1h&cursor=X`）**每次都**回 `range_too_large=true` —— 因为 `cursor` 之前的那
    ///   几万行"我已经看过了"的行把 `window_lines` 顶爆，而本次请求真正要交付的**增量**只有几条；
    /// - 渲染端 `p3_logs.rs::apply_page` 见 `entries` 为空 ⇒ `shown=0` ⇒ **列表被清空**并常驻
    ///   「检索范围超限 · 未执行检索」；而 1 h 已是**最小档** ⇒ 按屏上提示"缩小时间范围"**再缩也没用**。
    ///
    /// ⇒ 旧句"与用户勾了哪几个级别无关"的**前提**（"窗口内容规模 = 用户要面对的数据量"）不成立：
    /// 被筛掉的行**不会被交付**，把它们计入会让「请缩小时间范围」这句提示**失真**（用户筛到 ERROR
    /// 仍然被拒，而真正要看到的只有几条）。故计数移到**全部筛选之后、`kept.insert` 之前**。
    /// 把守用例 = [`tests::a_busy_window_with_a_cursor_is_not_reported_as_too_large`]（增量轮询不超限）、
    /// [`tests::a_level_filter_that_shrinks_the_window_content_clears_the_over_cap_flag`]（筛选真的生效）。
    fn take(
        &mut self,
        ts_ms: u64,
        level: LogLevel,
        target: String,
        msg: String,
        k: u32,
        file_rank: u32,
    ) -> Step {
        if !self.q.levels.is_empty() && !self.q.levels.contains(&level) {
            return Step::Continue;
        }
        if !self.q.targets.is_empty() && !self.q.targets.iter().any(|t| t == &target) {
            return Step::Continue;
        }
        let seq = ts_ms.saturating_mul(1000).saturating_add(u64::from(k.min(999)));
        if let Some(c) = self.q.cursor {
            if seq <= c {
                return Step::Continue; // 增量：只要"更新的"（契约 §4.4）
            }
        }
        // **计数在此**（全部筛选之后）：见上面的口径论证。超限 ⇒ 立即停止（不再收集，
        // 由上层用**已收集的**内容组页 + `range_too_large=true`，见 [`finish_page`]）。
        self.stats.window_lines += 1;
        if self.stats.window_lines > self.max_lines {
            self.too_large = true;
            return Step::TooLarge;
        }
        self.kept.insert(
            (seq, file_rank),
            LogEntry {
                seq,
                ts_ms,
                level,
                target,
                message: truncate_message(&msg),
            },
        );
        // 只留最新 `limit+1` 条：多出来的那条**只用来证明 has_more**
        while self.kept.len() > self.q.limit + 1 {
            let oldest = *self.kept.keys().next().expect("非空");
            self.kept.remove(&oldest);
        }
        Step::Continue
    }

    /// 倒读路径的单行入口。
    ///
    /// **为什么要"组"**：倒读遇到同一毫秒的若干行是**逆序**的，而 `k` 按模块头定义是
    /// **文件正序**的下标 ⇒ 必须把整组收齐再**逆序**编号，否则同一条目在"倒读 vs 正读"
    /// 两次请求里会拿到不同的 `seq`（`cursor` 会漏条）。组 = 连续同 `ts` 的行。
    fn backward_line(&mut self, line: &str, file_rank: u32) -> Step {
        self.note_line_read(); // 统计（**不是**硬闸判据）
        let Some((ts_ms, level, target, msg)) = parse_json_log_line(line) else {
            // 不可解析的行**清零迟滞**：它既不是"比窗口新"也不是"比窗口旧"，
            // 不能靠它推断"后面都在窗口外"（B-2 的另一半正是"不可解析内容无界"）。
            self.note_not_stop_side_line();
            // B-2 硬代价闸：**解析失败的行**才计入（整改三订正 —— 可解析的行交给窗口内容判据）。
            return self.note_unparsable_line();
        };
        self.stats.parsed_lines += 1;
        if ts_ms > self.end {
            // 比窗口新 ⇒ 跳过；同时它是组边界（正常倒读时组尚未开始）。
            // 倒读的**早退侧是"更旧"**⇒ 这一侧不清零、也不停（它必然出现在窗口**之后**）。
            self.note_not_stop_side_line();
            return self.flush_group(file_rank);
        }
        if ts_ms < self.start {
            // 越过窗口左端 ⇒ **本文件**剩余行在窗口外。B-3：只停**本文件**（`EndOfFile`），
            // 且必须连续超出一个迟滞块才停（单条乱序行 / 时钟回拨的那一条不该让整个文件收工）。
            if !self.note_stop_side_line(line) {
                return self.flush_group(file_rank);
            }
            return match self.flush_group(file_rank) {
                Step::Continue => Step::EndOfFile,
                other => other,
            };
        }
        self.note_not_stop_side_line();
        if self.group_ts != Some(ts_ms) {
            match self.flush_group(file_rank) {
                Step::Continue => {}
                other => return other,
            }
            self.group_ts = Some(ts_ms);
        }
        // ⚠️ **整改五 B 组**：`group` 的长度守卫（**与 `max_lines` 同源**）。
        //
        // 倒读必须先**攒齐整组同毫秒行**才能编号 `k`（见 [`ScanState::flush_group`]）⇒ `kept` 的
        // "最新 `limit+1` 条"上界与 `window_lines` 的上界**都在这之后**才生效，而 `group` 本身
        // 整改前**没有任何长度判据**。写者卡顿后把同一毫秒的 N 行一次性刷出
        // （`non_blocking` 的批量落盘），或日志里出现**同一毫秒的巨量行**（外部工具批量追加）
        // ⇒ `group` 先长到 N 条，每条含 2 个 `String` 且 `message` **尚未截断**
        // （`truncate_message` 只在 `take` 里做）⇒ `kept` 处那句"内存上界 = `limit+1` 条，与文件
        // 大小无关"不再成立。
        //
        // 现在：组内条数到 `max_lines` 即判 `too_large`（**取数与窗口内容闸同源**）⇒ 可见拒绝，
        // 而不是先把内存吃满。**不静默丢条**：`range_too_large=true` 明说"`entries` 不代表完整
        // 结果"，且 `kept` 里**已收尾**的那些组照常交付（A 组裁定）。
        // ⚠️ **口径差（复核指出，如实登记）**：这里量的是**原始组长**，而 `take` 自 A 组起量的是
        // **通过全部筛选之后**的条数 ⇒ 同毫秒爆发若全被级别/模块筛掉，本处仍会判 `too_large`
        // 而 `window_lines` 为 0。两种量法**不是同一把尺**（触发需 ≥ `max_lines` 条同毫秒，
        // 罕见；且替代方案是无界内存，更坏）⇒ 保留现状，登记待裁。
        // 把守用例 = [`tests::a_single_millisecond_burst_cannot_grow_the_group_beyond_the_cap`]。
        if self.group.len() >= self.max_lines {
            self.too_large = true;
            return Step::TooLarge;
        }
        self.group.push((level, target, msg));
        Step::Continue
    }

    /// 收尾当前同毫秒组：按**文件正序**（= 遇到顺序的逆序）编号 `k` 后逐条 [`ScanState::take`]。
    fn flush_group(&mut self, file_rank: u32) -> Step {
        if self.group.is_empty() {
            self.group_ts = None;
            return Step::Continue;
        }
        let ts_ms = self.group_ts.expect("group 非空 ⇒ group_ts 必为 Some");
        let items: Vec<(LogLevel, String, String)> = self.group.drain(..).collect();
        self.group_ts = None;
        for (i, (level, target, msg)) in items.into_iter().rev().enumerate() {
            match self.take(ts_ms, level, target, msg, i as u32, file_rank) {
                Step::Continue => {}
                other => return other,
            }
        }
        Step::Continue
    }
}

/// 倒读分块大小（字节）。
const REVERSE_CHUNK_BYTES: u64 = 64 * 1024;

/// 倒读读窗的**扩大上限**（B-1）：块内没有完整行（= 该行自身比块还长）时读窗翻倍再试，
/// 直到能凑齐一整行或被本上限挡住。64 KiB 只是**分块**大小，不是行长上限——HMI 的主路径是倒读，
/// 若把"行长 > 块长"当成"这行不存在"，一条超长日志（如带大 payload 的错误上下文）就会被静默丢。
/// 1 MiB 的取法：足够装下正常日志里任何一行，又不让单请求内存无界。
///
/// ⚠️ **整改五 D-2**：本常量**同时**是**正读**路径的单行上限（[`read_forward`] 用
/// [`BoundedLineReader::new`] 时把它当 `line_max`）。理由：两个方向对**同一份文件**必须给出
/// **同一套条目**——若正读的上限比它小，一条 1 MiB 的行在倒读里能组装上屏、在正读里却被跳过
/// ⇒ 同一份日志换个窗口位置就少一条（正是本项目"静默丢条"的红线）。
const MAX_REVERSE_WINDOW_BYTES: u64 = 1024 * 1024;

/// **硬代价闸 · 行数**（B-2）：单次请求最多读入多少**解析失败**的行
/// （[`ScanStats::unparsable_lines`]）—— **可解析的行一律不计入**。
///
/// 取 `4 ×` 契约上限 [`LOG_SCAN_MAX_LINES`]（= 200 000）而不是 `4 × `配置的 `max_lines`：
/// 后者会**误伤合法扫描** —— "窗口在大文件中部、窗口内只有 1 行"时，扫描必须**穿过**约半个文件
/// 才能走到窗口（R3 的判据网 `range_too_large_is_decided_by_window_content_not_by_lines_scanned`
/// 正是这种情形：限额 1 000、实读 3 万行，**应当**被正常服务）。本闸只兜"**内容根本读不懂**"的
/// 无界代价，**不参与 EDGE-15 判据**。
///
/// ⚠️ **整改三订正**：整改二的本闸判据是 `lines_read`（读入的**总**行数）⇒ （a）**可解析**的
/// 500 000 行正常文件 + 中部 1 ms 窗口会被判 `range_too_large`（**重犯了 R3 的原缺陷**）；
/// （b）行入口的计数**先于** `take`（窗口内容计数），故 `max_lines ≥` 本阈值（20 万）时本闸
/// **总是先触发** ⇒ 上面那句"不参与 EDGE-15 判据"**当时并不成立**（复核点名）。
/// 现在判据是 `unparsable_lines`，两句话才都落地：**可解析内容只由窗口内容判据裁决，本闸不碰它**。
const SCAN_READ_BUDGET_LINES: usize = LOG_SCAN_MAX_LINES * 4;

/// **硬代价闸 · 字节**（整改三新增）：单次请求最多"读了却没产出任何行"多少字节（**每请求**、非每文件）。
///
/// 存在的理由：行数闸拦不住"**读不动**"的输入 —— 倒读遇到块内凑不齐一整行时会**翻窗重读**
/// （B-1 的根因修复），那部分字节一行也没产出 ⇒ 不计入 `lines_read`。复核实测：4 MiB 无换行文件
/// 读入 8 257 536 B（**2.0×**）而 `lines_read = 1`，行数闸**永不触发**，回包还是
/// `entries=[] ∧ range_too_large=false`（硬红线）。本闸把这类代价封死。
///
/// # 取值 = `1.5 ×` [`MAX_REVERSE_WINDOW_BYTES`]（= 1.5 MiB）—— 数字是**算出来的**，不是拍的
///
/// 读窗有一个**结构性**上限（[`MAX_REVERSE_WINDOW_BYTES`]）：一行若**自身 ≥ 读窗上限**，
/// 就永远不可能被"装进一个窗口"（起点的判据要求窗口内真有一个**内部** `'\n'`）⇒ 它只能以
/// 行内片段的形式被读掉。于是：
///
/// | 输入 | 翻窗重读的字节代价 | 本闸的行为 |
/// |------|-------------------|-----------|
/// | 一条**贴尾**的长行，行长 < 读窗上限 | ≤ `64K+128K+256K+512K` = 960 KiB（然后是**成功**的那一轮） | 不耗尽 ⇒ **正常组装并上屏**（B-1 承诺） |
/// | 同一请求内**累计两条**这种长行（如同一次请求里的两个文件各一条） | ≥ 1.92 MiB | 耗尽 ⇒ `range_too_large=true` |
/// | 任意**行长 ≥ 读窗上限**的行，或无换行的文件 | ≥ `960 KiB + 1 MiB` = 1.94 MiB | **必然耗尽** ⇒ `range_too_large=true`（**可见拒绝**） |
///
/// 关键就是这条**夹逼**：`960 KiB < 1.5 MiB < 1.94 MiB` ⇒
/// **"装得下"的内容一律组装上屏，"装不下"的内容一律可见拒绝，二者之间没有静默丢弃的缝**。
/// （取值若 ≥ 1.94 MiB，则 1 MiB～2 MiB 的单行会被静默丢成 `entries=[] ∧ range_too_large=false`；
/// 这正是复核抓到的那类"看着正常但少一条"。）
/// ⚠️ **如实登记的代价**：本闸是**每请求**累计的 ⇒ 上述"两条接近 1 MiB 的贴尾长行"会一起被拒
/// （`range_too_large`），尽管它们**各自**都装得下。选"宁可可见拒绝、不可静默丢条"是刻意的取舍。
const SCAN_READ_BUDGET_BYTES: u64 = MAX_REVERSE_WINDOW_BYTES + MAX_REVERSE_WINDOW_BYTES / 2;

/// B-3 早退迟滞（字节）：**连续**这么多字节的"早退侧"内容才认定"本文件剩余部分都在窗口外"。
/// 取一个倒读块的大小（64 KiB）——量级依据见模块头「扫描方向」。
const EARLY_STOP_HYSTERESIS_BYTES: u64 = REVERSE_CHUNK_BYTES;

/// `/logs/targets` 的分块读大小（字节）——**同时是**该端点的单行行长上限。
///
/// 存在的理由（整改五 C 组）：本端点此前用 `BufReader::lines()` 整行物化 ⇒ 对"无换行 /
/// 超长行"是**无上限** `read_line`（1 GB 无换行的 `mupc.log.x` ⇒ 一次 ~1 GB 分配）。
/// 取 64 KiB：足够装下正常日志的任何一行（JSON 行通常几百字节），又不让单次分配无界。
const TARGETS_READ_CHUNK_BYTES: usize = 64 * 1024;

/// `/logs/targets` 的单行行长上限：超过即**跳过整行**（见 [`LogService::targets`]）。
const TARGETS_LINE_MAX_BYTES: usize = TARGETS_READ_CHUNK_BYTES;

// ═══════════════════════════════════════════════════════════════════════════
// 2'. 有界按行读（**正读**与 `/logs/targets` 采样**共用**）
// ═══════════════════════════════════════════════════════════════════════════

/// [`BoundedLineReader::next_line`] 的一次产出。
enum BoundedLine<'a> {
    /// 一整行 —— **不含**行尾 `'\n'`（若该行以 `\r\n` 结尾，`'\r'` 也一并去掉，
    /// 与 `tokio::io::BufReadExt::lines()` **逐字**同口径；见 [`BoundedLineReader::next_line`]）。
    /// 字节数 ≤ 构造时给的 `line_max`。
    Line(&'a [u8]),
    /// 一条**超过行长上限**的行：**整行已被丢弃**（前缀既没有留在 `line` 里、也没有交给调用方，
    /// 后续字节只是被跳过）⇒ 调用方按各自口径记账 / 告警。`bytes` = 该行**读入的字节数**
    /// （含被丢弃的前缀，不含行尾 `'\n'`）—— 与倒读路径"读了却没产出行的字节"同口径。
    Overlong { bytes: u64 },
}

/// **有界**按行读：单行物化 ≤ `line_max` 字节、单次 `read` ≤ `chunk_bytes` 字节
/// ⇒ 无论输入多大，本读器的瞬时分配都是常数。
///
/// # 为什么要有它（整改五 **D-2**，正读路径的漏网）
///
/// 同一类缺陷在整改五里已被修掉**两处**：`/logs/targets`（C 组：`BufReader::lines()` ⇒
/// 1 GB 无换行文件一次 ~1 GB 分配）与倒读路径（B-1 / 整改三：读窗 ± 字节闸）。
/// **只剩正读 `read_forward` 一条**仍是 `BufReader::new(f).lines()` —— 它对**单行**没有任何上限：
/// 一行多长就物化多长，物化**先于** `truncate_message` 截断，且 `lines_read` 只 +1 ⇒ **任何闸都
/// 不触发**。现实可达：择向 [`LogService::choose_direction`] 在"文件头是可解析时间戳、窗口靠
/// 文件头"时选 `Forward`，此后文件中间夹一条百 MB~GB 级的行（大 payload 打进 `message`、
/// 或别的工具把二进制块写进 `mupc.log*`）就是一次与行长同阶的分配。本模块的设计前提就是
/// "日志目录可能被别的工具污染"（见模块头）⇒ 这不是臆想输入。
///
/// # 语义（**逐字**对齐 `BufRead::lines()`，只多一条行长上限）
///
/// - 以 `'\n'` 切行；空段（`'\n'` 紧跟 `'\n'`）产出**空行**（照常交给调用方解析 ⇒ 解析失败，
///   与旧实现一致 —— 旧实现也**不**跳过空行）；
/// - 行尾 `'\n'` 去掉；**仅当**该行以 `'\n'` 结尾时再吃掉一个 `'\r'`（`\r\n` ⇒ 两个都去）；
///   **文件末行没有 `'\n'`** ⇒ 该行照常消费，其末尾的 `'\r'`（若有）**保留**
///   （`tokio::io::BufReadExt::lines()` 的实现就是这样：`ends_with('\n')` 才 pop `'\r'`）；
/// - 单行字节数 **> `line_max`** ⇒ 该行**整行丢弃**（**绝不物化**）⇒ 产出 [`BoundedLine::Overlong`]；
/// - 文件末尾没有 `'\n'` 的**最后一行**若本身超长 ⇒ 同样产出 [`BoundedLine::Overlong`]。
///
/// # 调用点（**两处共用，不许再抄第三份**）
///
/// | 调用方 | 块大小 | 行长上限 | 超长行的处置 |
/// |--------|--------|----------|--------------|
/// | [`LogService::targets`] | [`TARGETS_READ_CHUNK_BYTES`] | [`TARGETS_LINE_MAX_BYTES`] | 计数 + 事后一条 `warn!` |
/// | [`read_forward`] | [`REVERSE_CHUNK_BYTES`] | [`MAX_REVERSE_WINDOW_BYTES`] | 计入**字节闸** [`SCAN_READ_BUDGET_BYTES`] + 逐条 `warn!`（**与倒读同口径**） |
///
/// 两处的**单行上限取值不同**是刻意的：`targets` 只做 target 采样（一行几百字节足够），
/// 正读则是**页路径**——它要尽量与倒读同口径，免得两个方向对同一份文件给出不同的条目集。
/// ⚠️ **复核实测订正（本轮）**：原写"否则同一份文件会给出不同的条目集（同一个 1 MiB 的行，
/// 倒读能组装上屏、**正读却被跳过**）"—— **理由说反了**：恰 1 MiB 的行是**正读交付、
/// 倒读装不下**（倒读还要求行尾 `'\n'` 挤进窗口 ⇒ 可容行长比正读**少 1 字节**；行长 ≥ 上限时
/// 倒读是整页拒绝、正读只跳过那一条）。取同值的意义是**把差距压到最小**，**不是**两侧等价。
/// 真正的对齐口径与已知不对称见模块头「复核实测订正」。故正读沿用倒读的读窗上限
/// [`MAX_REVERSE_WINDOW_BYTES`] 作为行长口径。
struct BoundedLineReader<'a> {
    /// 底层文件（**从当前游标**读；调用方负责先 `seek`）。
    f: &'a mut tokio::fs::File,
    /// 块缓冲：一次 `read` 最多 `buf.len()` 字节。
    buf: Vec<u8>,
    /// `buf` 中已消费到的下标。
    filled: usize,
    /// `buf` 中本轮有效字节数。
    read: usize,
    /// 已确认读到文件尾（`read` 返回 0）。
    eof: bool,
    /// 当前行已累积的字节（**≤ `line_max`**）。
    line: Vec<u8>,
    /// 单行字节上限（达到即转"丢弃整行"）。
    line_max: usize,
    /// 正在丢弃一条超长行的剩余字节（直到下一个 `'\n'`）。
    skipping: bool,
    /// 当前这条被丢弃的超长行已读入的字节数（含丢弃前积在 `line` 里的那部分）。
    skipped_bytes: u64,
}

impl<'a> BoundedLineReader<'a> {
    fn new(f: &'a mut tokio::fs::File, chunk_bytes: usize, line_max: usize) -> Self {
        Self {
            f,
            buf: vec![0u8; chunk_bytes],
            filled: 0,
            read: 0,
            eof: false,
            line: Vec::new(),
            line_max,
            skipping: false,
            skipped_bytes: 0,
        }
    }

    /// 下一条有效产出；`None` = 文件读完。IO 错误原样上抛（调用方按各自口径包成 `Err`）。
    ///
    /// 返回的 `Line` 借用 `self` ⇒ 调用方在本次迭代内用完即弃（下一轮再调本方法）。
    async fn next_line(&mut self) -> std::io::Result<Option<BoundedLine<'_>>> {
        use tokio::io::AsyncReadExt;

        // 上一轮返回的 `Line` 已经交付 ⇒ 从空行重新累积。
        self.line.clear();
        loop {
            while self.filled < self.read {
                let b = self.buf[self.filled];
                self.filled += 1;
                if b == b'\n' {
                    if self.skipping {
                        self.skipping = false;
                        return Ok(Some(BoundedLine::Overlong { bytes: self.take_skipped() }));
                    }
                    // `lines()` 语义：`\r\n` 去掉两个；文件末行（无 `\n`）的 `\r` 保留
                    // （后者走下面 `eof` 分支，不经这里）。
                    if self.line.last() == Some(&b'\r') {
                        self.line.pop();
                    }
                    return Ok(Some(BoundedLine::Line(&self.line)));
                }
                if self.skipping {
                    self.skipped_bytes += 1;
                    continue;
                }
                if self.line.len() >= self.line_max {
                    // 单行已达上限 ⇒ **丢弃整行**（含已经积起来的那部分）：既不再往 `line` 里塞，
                    // 也不把半截交给调用方（半截 JSON 只会解析失败，徒增一次 `lines_read`）。
                    self.skipping = true;
                    self.skipped_bytes = self.line.len() as u64 + 1; // 前缀 + 当前这一字节
                    self.line.clear(); // 立刻真释放：别让上界变成 `line_max + chunk`
                    continue;
                }
                self.line.push(b);
            }
            if self.eof {
                if self.skipping {
                    self.skipping = false;
                    return Ok(Some(BoundedLine::Overlong { bytes: self.take_skipped() }));
                }
                if !self.line.is_empty() {
                    // 文件末尾**没有 `'\n'`** 的最后一行：照常消费（`lines()` 语义）
                    return Ok(Some(BoundedLine::Line(&self.line)));
                }
                return Ok(None);
            }
            self.read = self.f.read(&mut self.buf).await?;
            self.filled = 0;
            if self.read == 0 {
                self.eof = true;
            }
        }
    }

    fn take_skipped(&mut self) -> u64 {
        std::mem::take(&mut self.skipped_bytes)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 服务
// ═══════════════════════════════════════════════════════════════════════════

/// 日志服务（无状态：只持目录与限额 ⇒ 天然只读、天然并发安全）。
#[derive(Debug, Clone)]
pub struct LogService {
    dir: PathBuf,
    limits: LogLimits,
}

impl LogService {
    /// 构造（`log_dir` = **生效的** `config.system.log_dir`；`limits` = `config.display.log`）。
    ///
    /// ⚠️ `log_dir` 必须与 `main.rs` 交给 `tracing_appender::rolling::daily` 的**同一个值**
    /// （单一真源，见模块头「日志目录的单一真源」）。
    ///
    /// 装配期**不做** I/O：目录不存在**不**在此报错（运行中目录可能被清理）⇒ 「读不出来」在
    /// **请求期**以 `Err` 表达（handler 落 503，**不是**空页）——见 [`LogService::page`]。
    pub fn new(dir: impl Into<PathBuf>, limits: LogLimits) -> Self {
        if limits.page_limit_max > LOG_PAGE_LIMIT_MAX {
            // 合同上限是跨侧硬约束（渲染端 `MAX_BODY_BYTES` 按 `LOG_PAGE_LIMIT_MAX × 1 KiB`
            // 编译期推算）⇒ 配置调大它**不会**被本服务采纳，但必须**说出来**（不静默）。
            tracing::warn!(
                configured = limits.page_limit_max,
                enforced = LOG_PAGE_LIMIT_MAX,
                "display.log.page_limit_max 超过契约上限，本服务按契约常量执行（调大它会让屏上合法回包被判 BodyTooLarge）"
            );
        }
        if limits.max_files == 0 || limits.max_lines == 0 {
            tracing::warn!(
                max_files = limits.max_files,
                max_lines = limits.max_lines,
                "display.log 的限额为 0 ⇒ 任何检索都会判 range_too_large（请核对 mupc_core_config.yaml）"
            );
        }
        if limits.max_files > LOG_SCAN_MAX_FILES || limits.max_lines > LOG_SCAN_MAX_LINES {
            // 设计 §4.4 的 5 文件 / 50 000 行是**代价评估的上限** ⇒ 调大它不是非法，但单次请求
            // 的代价会超出设计评估，必须**说出来**（不静默）。
            tracing::warn!(
                max_files = limits.max_files,
                max_lines = limits.max_lines,
                design_max_files = LOG_SCAN_MAX_FILES,
                design_max_lines = LOG_SCAN_MAX_LINES,
                "display.log 的扫描限额高于设计默认值 ⇒ 单次检索代价高于设计评估（EDGE-15 的上限按此默认值定）"
            );
        }
        Self {
            dir: dir.into(),
            limits,
        }
    }

    /// `GET /v1/console/logs`。
    ///
    /// `Err` 只在**日志源不可读**时出现（目录不存在 / 无权限 / 打开文件失败）——由 handler 落
    /// **非 2xx**（§3.4 补注：GET 失败走 HTTP 状态码，**不得** `200` + 空列表冒充"无日志"）。
    pub async fn page(&self, q: &LogQuery, now_ms: u64) -> Result<LogPage, String> {
        self.page_with_stats(q, now_ms).await.map(|(p, _)| p)
    }

    /// 同 [`LogService::page`]，另回扫描统计（**超限/有界的实测证据**用；handler 不消费）。
    ///
    /// **超限时的返回形态有两条、且不同**（整改五 A 组裁定，逐字见模块头「超限时回什么」）：
    /// **窗口内容 / 文件数超限** ⇒ 已收集的最新 `limit` 条 + `range_too_large=true`；
    /// **硬代价闸耗尽** ⇒ 空页 + `range_too_large=true`（并配 `tracing::warn!` 点名真实成因）。
    pub async fn page_with_stats(
        &self,
        q: &LogQuery,
        now_ms: u64,
    ) -> Result<(LogPage, ScanStats), String> {
        let (start, end) = q.window(now_ms);
        let files = self.list_log_files().await?;
        let mut st = ScanState::new(q, start, end, self.limits.max_lines);
        let mut in_range_files = 0usize;

        // 文件名升序 ⇒ 反转即"新文件在前"（`mupc.log.YYYY-MM-DD` 的字典序 = 日期序）。
        for (idx, path) in files.iter().rev().enumerate() {
            // 整个文件都比窗口还旧 ⇒ 更旧的文件不可能有命中，**停**（省 I/O）。
            // 留一天余量：文件名日期由 `tracing_appender` 按**本地时**取，而 JSON 行时间戳是
            // **UTC** ⇒ 时区差最大 1 天，无余量可能漏扫当天的文件（静默漏）。
            if let Some((_day_start, day_end)) = file_day_window(path) {
                if day_end.saturating_add(86_400_000) < start {
                    break;
                }
            }
            in_range_files += 1;
            if in_range_files > self.limits.max_files {
                // **未打开第 max_files+1 个文件**就在这里停止（`files_scanned` 不含它）。
                st.too_large = true;
                break;
            }
            // 文件序：**越大越新**（`files` 升序 ⇒ 反转后 idx=0 最新 ⇒ rank 最大）。
            let file_rank = (files.len() - 1 - idx) as u32;
            // B-3：**没有**"更旧的文件也全在窗口外"这种跨文件早退（`EndOfScan` 已删除）——
            // 每个文件都独立判断（`Continue` = 继续下一个文件；`EndOfFile` = 本文件到此为止）。
            if let Step::TooLarge = self.scan_file(path, &mut st, file_rank).await? {
                st.too_large = true;
                break;
            }
        }

        let stats = st.stats;
        warn_if_log_format_looks_blind(&stats);
        if st.budget_exhausted {
            // 方案 (a)：仍回 `range_too_large=true`（契约语义 = "entries 不代表完整结果"，这一点诚实），
            // 但**真实成因**必须喊出来——它与 EDGE-15 的"窗口跨度过大"**不是**一回事，
            // 屏上文案「请缩小时间范围」对用户**略有误导**（已在模块头如实登记）。
            tracing::warn!(
                lines_read = stats.lines_read,
                unparsable_lines = stats.unparsable_lines,
                budget_lines = SCAN_READ_BUDGET_LINES,
                skipped_bytes = stats.skipped_bytes,
                budget_bytes = SCAN_READ_BUDGET_BYTES,
                window_lines = stats.window_lines,
                parsed_lines = stats.parsed_lines,
                files_scanned = stats.files_scanned,
                "硬代价闸耗尽（行数闸 = 解析失败的行；字节闸 = 读了却没产出行的字节。典型成因：日志格式\
                 漂移 / 被压缩过的二进制 / 别的工具写进 mupc.log* / 无换行或超长的行）⇒ 回 \
                 range_too_large 拒绝，但真实成因**不是**窗口跨度过大：屏上文案对用户略有误导"
            );
        }
        // **两条路径的返回形态不同**（整改五 A 组裁定，见模块头「命中时回什么」）：
        // - 硬闸（`budget_exhausted`）⇒ **空页** + 标志（方案 (a)，已评审通过，本轮不改）；
        // - 窗口内容 / 文件数超限（`too_large`）⇒ **已收集的最新 `limit` 条** + 标志
        //   （渲染端 `p3_logs.rs:728` 的 `list_view_of(rows, too_large)` 在有条目时照显行，
        //    超限提示是常驻构件 ⇒ 渲染端无需改动）。
        let page = if st.budget_exhausted {
            too_large_page()
        } else if st.too_large {
            finish_page(st.kept, q.limit, true)
        } else {
            finish_page(st.kept, q.limit, false)
        };
        Ok((page, stats))
    }

    /// 扫描单个文件（**按窗口位置择向**，见模块头「扫描方向」）；返回是否需要继续。
    ///
    /// **B-3**：本函数**只**返回"这个文件读完了 / 这个文件不用再读"（[`Step::EndOfFile`]），
    /// **不再**有"更旧的文件也不用看"这层含义 —— 是否继续由调用方按"文件列表还没走完"决定。
    async fn scan_file(
        &self,
        path: &Path,
        st: &mut ScanState<'_>,
        file_rank: u32,
    ) -> Result<Step, String> {
        // 阻塞-1 整改（评审）：`ScanState` 是**逐请求**的状态，而其中一部分字段是**逐文件**语义
        // ⇒ 必须在**每个文件开头**显式重置。曾漏掉 `stop_side_bytes`：文件 A 早退留下的
        // ≥ 64 KiB 迟滞计数让文件 B 的**第一条**早退侧行立刻达阈 ⇒ B 被整体跳过（静默少一条）。
        st.begin_file();
        let mut f = match tokio::fs::File::open(path).await {
            Ok(f) => f,
            // R7：列目录与打开之间被轮转/删除 ⇒ **跳过该文件**（其余 IO 错误仍报错 ⇒ 503）。
            // 整改前一次轮转就可能让整个日志页 503（用户什么都没做错）。
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(
                    file = %path.display(),
                    "日志文件在列目录与打开之间消失（轮转竞态），已跳过该文件"
                );
                return Ok(Step::Continue);
            }
            Err(e) => return Err(format!("打开日志文件 {} 失败: {e}", path.display())),
        };
        st.stats.files_scanned += 1;
        let len = f
            .metadata()
            .await
            .map_err(|e| format!("读取日志文件 {} 元信息失败: {e}", path.display()))?
            .len();
        if len == 0 {
            return Ok(Step::Continue);
        }
        // R3-③：把**实际择向**记进统计（否则"尾部窗口走倒读"这条主路径承诺无人在守：
        // 把 `choose_direction` 写死 `Forward` 时，全部语义类用例仍然全绿）。
        match self.choose_direction(&mut f, len, st.start, st.end, path).await? {
            ScanDirection::Backward => {
                st.stats.backward_files += 1;
                read_backward(&mut f, len, st, file_rank, path).await
            }
            ScanDirection::Forward => {
                st.stats.forward_files += 1;
                read_forward(f, st, file_rank, path).await
            }
        }
    }

    /// 择向：文件首/尾各采样一个 64 KiB 块，取首个/末个**可解析**时间戳，比较
    /// 「窗口右端距文件头」与「窗口左端距文件尾」两个跨度，取**小**的一侧。
    ///
    /// 采样不出时间戳（文件极短 / 全是碎片行）⇒ 默认**倒读**（HMI 的主路径是"最近的窗口"）。
    /// 择向只影响**代价**，不影响语义（除模块头「会漏行的条件」所述的边界）。
    async fn choose_direction(
        &self,
        f: &mut tokio::fs::File,
        len: u64,
        start: u64,
        end: u64,
        path: &Path,
    ) -> Result<ScanDirection, String> {
        let head = read_at(f, 0, REVERSE_CHUNK_BYTES.min(len))
            .await
            .map_err(|e| format!("读日志文件 {} 失败: {e}", path.display()))?;
        let tail_off = len.saturating_sub(REVERSE_CHUNK_BYTES);
        let tail = read_at(f, tail_off, len - tail_off)
            .await
            .map_err(|e| format!("读日志文件 {} 失败: {e}", path.display()))?;
        Ok(match (first_parsable_ts(&head), last_parsable_ts(&tail)) {
            (Some(first), Some(last)) => {
                let forward_span = end.saturating_sub(first);
                let backward_span = last.saturating_sub(start);
                if forward_span <= backward_span {
                    ScanDirection::Forward
                } else {
                    ScanDirection::Backward
                }
            }
            _ => ScanDirection::Backward,
        })
    }

    /// `GET /v1/console/logs/targets`：模块选项（设计 §4.4「选项列表来自 ring + 最近日志文件采样
    /// （去重排序，≤50 项）」；本单元无 ring ⇒ **只来自文件采样**）。
    ///
    /// 采样口径：**新文件在前**，最多 `display.log.max_files` 个文件 / `display.log.max_lines` 行
    /// （**与 `/logs` 同一条扫描预算** —— R5 整改：整改前这里硬用契约常量，运维把配置调小后
    /// 两个端点的扫描预算不一致）；去重后**字典序升序**，截到 [`LOG_TARGETS_MAX`] 项。
    /// ⚠️ 截断**无法在契约里表达**（`Vec<String>` 没有"还有更多"位）⇒ 只以 `tracing::warn!` 明示。
    ///
    /// # ⚠️ 整改五 C 组：本函数此前是**无上限的整行物化**
    ///
    /// 整改前这里用 `tokio::io::BufReader::new(f).lines()` + `next_line()` —— 即一次
    /// **无上限的 `read_line`**。而**同一个目录、同一个现实输入**（"别的工具写进 `mupc.log*` 的
    /// 二进制 / 无换行文件"）在页路径上被两道闸挡着（读窗上限 [`MAX_REVERSE_WINDOW_BYTES`] +
    /// 字节闸 [`SCAN_READ_BUDGET_BYTES`]），打在本端点上却是一次**无上限分配**：
    /// 一个 1 GB **无换行**的 `mupc.log.x` ⇒ 一次 ~1 GB 分配，且 `lines_read == 1`
    /// ⇒ `max_lines` 闸**永不触发**。
    ///
    /// ⚠️ **订正（实施期实测，避免把成因说大）**：渲染端对**本端点**是**启动期读清单里的一次**
    /// （`local-display/src/app.rs:687` 的 `pending_reads`），**不是** 500 ms 轮询（那条是
    /// `/logs` 的 `cursor` 增量）。但"可重复"仍然成立且**更现实**的来源是：本端点是**公开 HTTP
    /// 端点**（运维脚本 / 客户端重连 / 进程重启后重拉），只要那个大文件还在目录里，**每次调用都
    /// 重演一次**这份代价。
    ///
    /// 现在改为**按 [`TARGETS_READ_CHUNK_BYTES`] 分块读 + 单行 ≤ [`TARGETS_LINE_MAX_BYTES`]**：
    /// 超过行长上限的行**跳过**并 `tracing::warn!`（**如实说明"该行里的 target 采样不到"**，
    /// 不是静默丢弃）。本函数**不做**整文件物化（"顺手改成整文件读"是被明令禁止的改法）。
    /// 把守用例 = [`tests::targets_survive_a_newline_free_file_without_materializing_it`]。
    ///
    /// ⚠️ **整改五 D-2**：上面的分块切行逻辑已抽成**公共 helper** [`BoundedLineReader`]
    /// （正读 [`read_forward`] 用同一份）—— 本函数此前是那段逻辑的**第二份抄写**，而第三处
    /// （正读）没抄到 ⇒ 漏了整改。**不要再抄回去**。
    pub async fn targets(&self) -> Result<Vec<String>, String> {
        let files = self.list_log_files().await?;
        let mut set: BTreeSet<String> = BTreeSet::new();
        let mut lines_read = 0usize;
        let mut overlong_lines = 0usize;

        'files: for path in files.iter().rev().take(self.limits.max_files) {
            let mut f = match tokio::fs::File::open(path).await {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue, // 同 R7
                Err(e) => return Err(format!("打开日志文件 {} 失败: {e}", path.display())),
            };
            let mut r = BoundedLineReader::new(&mut f, TARGETS_READ_CHUNK_BYTES, TARGETS_LINE_MAX_BYTES);
            loop {
                let item = match r.next_line().await {
                    Ok(item) => item,
                    Err(e) => return Err(format!("读日志文件 {} 失败: {e}", path.display())),
                };
                let Some(item) = item else { break };
                match item {
                    BoundedLine::Overlong { .. } => {
                        // 行长超上限 ⇒ **整行被丢弃**（绝不物化）。计数 + 事后告警，不静默。
                        overlong_lines += 1;
                        continue;
                    }
                    BoundedLine::Line(line) => {
                        take_target_line(&mut set, line, &mut lines_read, self.limits.max_lines);
                        if lines_read >= self.limits.max_lines {
                            break 'files;
                        }
                    }
                }
            }
            if lines_read >= self.limits.max_lines {
                break;
            }
        }
        if overlong_lines > 0 {
            tracing::warn!(
                overlong_lines,
                line_max_bytes = TARGETS_LINE_MAX_BYTES,
                "模块选项采样遇到超过行长上限的行：已**跳过整行**（该行里的 target 采样不到；\
                 成因同页路径的'超长行 / 无换行文件'，见 log_service 模块头）"
            );
        }

        let mut out: Vec<String> = set.into_iter().collect();
        if out.len() > LOG_TARGETS_MAX {
            tracing::warn!(
                total = out.len(),
                kept = LOG_TARGETS_MAX,
                "模块选项超过契约上限，已截断（契约的 Vec<String> 无'还有更多'位，屏上表现为选项变少）"
            );
            out.truncate(LOG_TARGETS_MAX);
        }
        Ok(out)
    }

    /// 目录下的日志**常规文件**（`mupc.log*`），**按文件名升序**。
    ///
    /// 目录读不出来 ⇒ `Err`（handler 落 503）——**不得**回空列表：空列表会被屏上读成
    /// EDGE-08「当前筛选条件下无日志」，而事实是"日志源不可用"（两态互替是本项目硬红线）。
    ///
    /// **R1 整改（迁移回退修复）**：只收**常规文件**（`is_file()`）。整改前只按文件名前缀收，
    /// 一个**同名目录**（如 `mupc.log.2025-09-10/`）就会让 `File::open` 失败 ⇒ `page()` 返回 `Err`
    /// ⇒ 整条日志端点 **503**。迁出前的 `web-api::routes::logs::LogsHandler::list_log_files`
    /// **有** `path.is_file()`，设计 §4.4 的「迁移落点」明写要复用其文件定位逻辑 ⇒ 这是迁移回退。
    async fn list_log_files(&self) -> Result<Vec<PathBuf>, String> {
        let mut rd = tokio::fs::read_dir(&self.dir)
            .await
            .map_err(|e| format!("读日志目录 {} 失败: {e}", self.dir.display()))?;
        let mut out = Vec::new();
        while let Some(entry) = rd
            .next_entry()
            .await
            .map_err(|e| format!("遍历日志目录 {} 失败: {e}", self.dir.display()))?
        {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with(LOG_FILE_PREFIX) {
                continue;
            }
            if !entry
                .file_type()
                .await
                .map_err(|e| format!("读日志目录项类型失败 ({}): {e}", self.dir.display()))?
                .is_file()
            {
                continue; // 目录 / 符号链接（d_type 不跟随）/ 其它 ⇒ 不是日志文件
            }
            out.push(entry.path());
        }
        out.sort();
        Ok(out)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3'. 两种方向的行读取（**共享** `ScanState` 的处理逻辑）
// ═══════════════════════════════════════════════════════════════════════════

/// 从头正读；`ts > end` **连续**超过一个迟滞块 ⇒ 本文件剩余部分不用看（返回 [`Step::EndOfFile`]）。
///
/// ⚠️ 必须**先 seek(0)**：调用方（[`LogService::choose_direction`]）刚把游标留在文件尾
/// （采样用过同一个句柄），而 [`BoundedLineReader`] 是从**当前游标**开始读的。
///
/// # ⚠️ 整改五 **D-2**：本函数此前是**无上限的整行物化**（同类缺陷的第三处、最后一条漏网）
///
/// 整改前这里是 `tokio::io::BufReader::new(f).lines()` + `next_line()` —— 一次**无上限**的
/// `read_line`：行多长就物化多长，**先物化、后**交给 `truncate_message` 截断（1 KiB 的截断
/// 拦不住这件事），且 `note_line_read` 只 +1 ⇒ 行数闸 / 字节闸**一个都不触发**。
///
/// **现实可达**（不是臆想输入）：[`LogService::choose_direction`] 在"文件头是可解析时间戳、
/// 窗口靠文件头"时选 `Forward`；此后文件里只要有一条极长的行（大 payload 被打进 `message`，
/// 或**别的工具把二进制块写进 `mupc.log*`** —— 本模块的设计前提就是"日志目录可能被污染"，
/// 见模块头），就是一次与行长同阶的分配（几百 MB ~ GB 级）。**同一个目录、同一个输入**在
/// 倒读路径上被读窗上限 [`MAX_REVERSE_WINDOW_BYTES`] + 字节闸 [`SCAN_READ_BUDGET_BYTES`] 挡着，
/// 在 `/logs/targets` 上被 C 组整改挡着 —— 只剩正读这一条没闸。
///
/// 现在改用**共用的有界按行读** [`BoundedLineReader`]（与 `targets` 同一份代码）：
///
/// - 单行 ≤ [`MAX_REVERSE_WINDOW_BYTES`]（**与倒读取同一个数**，但**不是"两侧等价"**：
///   倒读还要求行尾 `'\n'` 也挤进读窗 ⇒ 其可容行长比正读**少 1 字节**，且行长 ≥ 上限时
///   倒读是**整页拒绝**、正读只**跳过那一条**；见模块头「复核实测订正」）；
///   块读 ≤ [`REVERSE_CHUNK_BYTES`]（与倒读块同尺寸）；
/// - **超过上限的行：整行跳过**（不解析、不入库、`k` 不推进），并按**倒读的同一种记账**
///   把它读入的字节计入**字节闸**（[`ScanState::note_unparsable_bytes`] = "读了却没产出任何行
///   的字节"）+ `tracing::warn!`；预算耗尽即 `Step::TooLarge` ⇒ **可见拒绝**，绝不静默丢条。
///   ⚠️ **不**计入 `unparsable_lines`、**不**计入 `lines_read`：被跳过的行**不是一行**（它没有
///   时间戳、不进任何同毫秒组、不参与 `k` 编号），这与倒读遇到"行长 ≥ 读窗上限"时**只记字节、
///   不记行**的处理**同口径**。**不许**再新造第三套计数口径。
///
/// 把守用例 = [`tests::forward_read_skips_an_overlong_line_and_keeps_its_neighbours`]。
async fn read_forward(
    mut f: tokio::fs::File,
    st: &mut ScanState<'_>,
    file_rank: u32,
    path: &Path,
) -> Result<Step, String> {
    use tokio::io::AsyncSeekExt;

    f.seek(std::io::SeekFrom::Start(0))
        .await
        .map_err(|e| format!("定位日志文件 {} 失败: {e}", path.display()))?;
    let mut r = BoundedLineReader::new(
        &mut f,
        REVERSE_CHUNK_BYTES as usize,
        MAX_REVERSE_WINDOW_BYTES as usize,
    );
    // 同一毫秒内的行序（见模块头 `seq` 定义）；**逐文件独立**（跨文件重置）。
    let mut k: u32 = 0;
    let mut cur_ts: Option<u64> = None;
    loop {
        let item = r
            .next_line()
            .await
            .map_err(|e| format!("读日志文件 {} 失败: {e}", path.display()))?;
        let Some(item) = item else { break };
        let line: &[u8] = match item {
            BoundedLine::Line(line) => line,
            BoundedLine::Overlong { bytes } => {
                // 超长行 ⇒ **整行跳过**：不解析、不入库，`k` / `cur_ts` **一个都不动** ——
                // 它没有时间戳，**不进任何同毫秒组、不参与 `k` 编号**（与倒读遇到装不进读窗的行
                // 同口径：那边也只记字节、不编号）。告警**逐条**发（与倒读那条 `warn!` 同频）。
                tracing::warn!(
                    file = %path.display(),
                    line_bytes = bytes,
                    cap = MAX_REVERSE_WINDOW_BYTES,
                    "正读遇到超过单行上限的超长行：**整行跳过**（不解析、不入库；成因同倒读路径的\
                     '超长行/无换行文件'，见 log_service 模块头）"
                );
                st.note_not_stop_side_line(); // 它**不是**"后面都超出窗口右端"的证据（同不可解析行）
                if st.note_unparsable_bytes(bytes) {
                    // 字节闸耗尽 ⇒ 可见拒绝（上层回 range_too_large=true + 点名真实成因）
                    return Ok(Step::TooLarge);
                }
                continue;
            }
        };
        st.note_line_read(); // 统计（**不是**硬闸判据）
        let line = String::from_utf8_lossy(line);
        let Some((ts_ms, level, target, msg)) = parse_json_log_line(&line) else {
            st.note_not_stop_side_line(); // 不可解析 ≠ "后面都超出窗口右端"
            if let Step::TooLarge = st.note_unparsable_line() {
                return Ok(Step::TooLarge); // B-2：硬代价闸（**解析失败的行**才计入）
            }
            continue;
        };
        st.stats.parsed_lines += 1;
        // `k` 对**每一条可解析行**（含窗口外）都要推进，才与"倒读分组"给出同一套 `seq`。
        k = if cur_ts == Some(ts_ms) { k + 1 } else { 0 };
        cur_ts = Some(ts_ms);
        if ts_ms > st.end {
            // B-3 镜像：正读的早退侧是"比窗口新"。**连续**超过一个迟滞块才停本文件
            // （整改前：一条时钟跳变的行立刻 `EndOfFile` ⇒ 它之后窗口内的行全丢）。
            if st.note_stop_side_line(&line) {
                return Ok(Step::EndOfFile);
            }
            continue;
        }
        st.note_not_stop_side_line();
        if ts_ms < st.start {
            continue;
        }
        match st.take(ts_ms, level, target, msg, k, file_rank) {
            Step::Continue => {}
            other => return Ok(other),
        }
    }
    Ok(Step::Continue)
}

/// 从尾**分块倒读**；`ts < start` **连续**超过一个迟滞块 ⇒ 本文件剩余部分不用看（返回 [`Step::EndOfFile`]）。
///
/// # 只在**行首**切块（**否则每跨一个块边界就丢一行**）
///
/// 每轮读 `[off, pos)`，然后**只在块内第一个 `'\n'` 之后**开始处理：块首那半行的**前半段在更低的
/// 地址里**，本轮看不到 ⇒ 本轮不处理它；而下一轮的区间右端就是那个 `'\n'` 的位置 ⇒ 下一轮会把
/// 这一行**连头带尾**整个包进来。于是：
///
/// - 每个被处理的区间都**以行首开始**（首轮例外：其右端是文件尾，末段可能是没有 `'\n'` 结尾的
///   **文件最后一行** —— 它本身就是完整的一行）；
/// - 区间之间**恰好相接、不重叠**（本轮处理的区间 = `[off+i+1, pos)`，下一轮的右端 = `off+i+1`）
///   ⇒ 每行**恰好**被处理一次（既不丢也不重）。
///
/// ⚠️ 反例（**本实现的第一版就是这样，被网抓出来**）：若"把块首半行当行处理、把块尾半行留到
/// 下一块续接"，块首那半行会被当作一条（解析失败的）行**丢掉**，而块尾那半行被一路推迟
/// ⇒ **每跨一个 64 KiB 边界就丢一行**（实测：60 000 行的文件丢 91 行、读入 60 091 行，
/// 且全是解析失败，屏上完全看不出来）。网 =
/// [`backward_chunked_read_sees_every_line_exactly_once`]。
///
/// 按**字节**切分（不对半行做 UTF-8 转换）⇒ 不会把一个多字节字符劈成半个；
/// 空段（行尾 `'\n'` 之后）跳过，与正读的 `BufRead::lines()` 同口径。
///
/// # B-1 整改：两个"绝不空转"的保证
///
/// 1. **块内没有完整行时读窗翻倍**（而不是原地不动）：`position('\n')` 取到的第一个换行**恰在块尾**
///    那一字节时（= 本块整段都是**同一行**的行内内容），整改前 `start == buf.len()` ⇒ 本块一行也不
///    处理、`pos` 又不推进 ⇒ **死循环**（实测：单行 ≥ 64 KiB 的当日文件，倒读迭代 61 次仍不收敛，
///    无护栏时进程被 `timeout` 杀掉 ⇒ **日志页当天永久不可用**，且每个 500 ms 请求再挂一个空转任务）。
///    现在这种形态与"整块无换行"**同一条分支**：先把读窗翻倍（`MAX_REVERSE_WINDOW_BYTES` 封顶），
///    直到能在一轮里凑齐一整行 —— 于是**行长 < 读窗上限的行都能被完整解析**（`message` 再按
///    1 KiB 截断上屏），而不是被当成"不存在"。翻到上限仍凑不齐（**行长 ≥ 1 MiB 的行结构性装不进
///    任何一个读窗**）⇒ 记一条 `tracing::warn!`，并把这段字节计入**字节闸**（整改三）：
///    预算耗尽即 `range_too_large=true` ⇒ **可见拒绝**，绝不静默丢条（取值论证见
///    [`SCAN_READ_BUDGET_BYTES`]）。
/// 2. **进度守卫**：`pos = if next < pos { next } else { off };` —— 两个分支都严格小于 `pos`
///    （`off = pos - chunk` 且 `chunk > 0`）。
///    ⚠️ 整改三订正：`else` 分支经代数证明 + 实测（把该分支换成 `panic!`，该模块全部用例 0 红）
///    **不可达**，因此它**不是**"兜底能救"的机制，**别**把它读成一条安全网 ——
///    真正的收敛保证是下面这段代数与 `chunk` 的倍增上限；`else` 仅作将来改动分支结构时的
///    防御性留置（**若**它哪天真被执行，效果也只是把 `pos` 退到 `off`，仍是推进而非空转）。
///
/// 收敛性论证（对所有块边界形态）：每轮 `off = pos - chunk`（`chunk > 0` ⇒ `off < pos`，
/// 当 `pos > 0`）。翻窗分支只在 `chunk < MAX_REVERSE_WINDOW_BYTES` 时 `continue` ⇒ `chunk` 严格
/// 倍增 ⇒ 有限步到达上限后必然走 `pos = off`（严格减小）；正常分支的 `pos` 取 `off + start`，
/// 而 `start < buf.len() ≤ chunk` ⇒ 严格减小（`off == 0` 那一路直接 `break`）。
/// 收敛由**上面的代数**保证；进度守卫的 `else` 分支不可达（见上），不参与这条论证。
///
/// ⚠️ 字节代价（整改三）：翻窗重读与"行长 > 读窗上限"的跳过都**没有产出任何行**，故按字节
/// 计入 [`SCAN_READ_BUDGET_BYTES`]（[`ScanState::note_unparsable_bytes`]）；耗尽即 `TooLarge`
/// ⇒ `range_too_large=true`，**绝不**静默回空页。
async fn read_backward(
    f: &mut tokio::fs::File,
    len: u64,
    st: &mut ScanState<'_>,
    file_rank: u32,
    path: &Path,
) -> Result<Step, String> {
    let mut pos = len;
    let mut chunk = REVERSE_CHUNK_BYTES; // 读窗（遇到超长行会翻倍；每轮结束后复位）
    let mut step = Step::Continue;
    while pos > 0 {
        let off = pos.saturating_sub(chunk);
        let buf = read_at(f, off, pos - off)
            .await
            .map_err(|e| format!("读日志文件 {} 失败: {e}", path.display()))?;
        let start = if off == 0 {
            0 // 已到文件头：本块**从行首开始**（文件的第一行）
        } else {
            match buf.iter().position(|b| *b == b'\n') {
                // 正常：块内第一个 '\n' **之后**（其前是上一行的 tail，留给下一轮）
                Some(i) if i + 1 < buf.len() => i + 1,
                // 本块**没有完整行**：无换行，或唯一的换行恰是块尾那一字节 ⇒ 整块都落在
                // 同一行的行内（该行自身比块还长）⇒ 本轮无行可处理。
                _ => {
                    // ⚠️ 本条分支读进来的字节**一行也没产出** ⇒ 必须**按字节记账**（整改三）。
                    // 整改前这里完全不计账：硬闸只被 `backward_line` / `read_forward` 的行入口推动
                    // ⇒ 一份没有换行的 4 MiB 文件（`lines_read = 1`）读入 8 MB 也**永不触发**任何
                    // 闸门，且回包是 `entries=[] ∧ range_too_large=false`（硬红线）。
                    let read = pos - off;
                    if chunk < MAX_REVERSE_WINDOW_BYTES {
                        // B-1 根因修复：读窗翻倍再试（**不是**原地 `pos = off` 后立刻
                        // "这行不存在"——那会静默丢掉一条超长日志）。本轮读入的字节被整段丢弃
                        // （下一轮重读更大的一块）⇒ 翻窗重读的代价一并计账。
                        if st.note_unparsable_bytes(read) {
                            return Ok(Step::TooLarge);
                        }
                        chunk = (chunk * 2).min(MAX_REVERSE_WINDOW_BYTES);
                        continue;
                    }
                    // 行长 > 上限：只作有界推进（该行的行内片段本轮不处理，但**不会挂死**）
                    tracing::warn!(
                        file = %path.display(),
                        at = pos,
                        cap = MAX_REVERSE_WINDOW_BYTES,
                        "倒读遇到超过读窗上限的超长行：跳过其行内片段（该行无法完整解析）以保证收敛"
                    );
                    if st.note_unparsable_bytes(read) {
                        // 字节预算耗尽 ⇒ **不得**回 `entries=[] ∧ range_too_large=false`
                        // （把"没查完"说成"确实没有"）⇒ 交上层回 `range_too_large=true` + 告警。
                        return Ok(Step::TooLarge);
                    }
                    pos = off;
                    chunk = REVERSE_CHUNK_BYTES;
                    continue;
                }
            }
        };
        // 块内**从新到旧**（区间以行首开始 ⇒ 每段都是一整行；末段为空或"文件最后一行"）
        for line in buf[start..].split(|b| *b == b'\n').rev() {
            if line.is_empty() {
                continue;
            }
            step = st.backward_line(&String::from_utf8_lossy(line), file_rank);
            if step != Step::Continue {
                break;
            }
        }
        if step != Step::Continue || off == 0 {
            break;
        }
        let next = off + start as u64; // = 块内第一个 '\n' 之后 ⇒ 行首（下一轮的右端）
        // B-1 进度守卫：`pos` 必须**严格减小**。
        //
        // ⚠️ **`else` 分支代数上不可达（整改三订正，评审要求"不许把死代码说成兜底能救"）**：
        // 执行到这里的前提是上面 `if step != Step::Continue || off == 0 { break; }` **没** break
        // ⇒ `off > 0`（且 `step == Continue`）⇒ 本轮走的是"正常分支"（`_` 分支只 `continue` 或
        // `break` 到不了这里）。正常分支下 `next = off + start` 且 `start < buf.len()`：
        //   · `off > 0` 时 `buf.len() = pos - off = chunk` ⇒ `next < off + chunk = pos`；
        //   · `off == 0` 已 break（`next = start < buf.len() = pos` 其实也成立，只是走不到）。
        // 实测：复核实测把本 `else` 换成 `panic!` 后跑全部 225 条 **0 红**；整改三的实施者又跑了一遍
        // `log_service::` 全部 **38 条**（含本轮新增的 4 条）同样 **0 红** ⇒ 不可达得证。
        // 真正的收敛保证来自**代数**（上式）与下面 `chunk` 的倍增上限；本守卫是**纯防御**保留，
        // 不是"出错时还能救回来"的机制 —— 将来若有人改动上面的分支结构，它才会重新有意义。
        pos = if next < pos { next } else { off };
        chunk = REVERSE_CHUNK_BYTES;
    }
    if step == Step::Continue {
        step = st.flush_group(file_rank);
    }
    Ok(step)
}

/// 消费 [`LogService::targets`] 流式采样里的**一整行**（行尾 `\r` 先修剪）；返回"是否已达
/// `max_lines`"（达阈 ⇒ 采样可以收工）。
///
/// 输入是**有界**的字节切片（≤ [`TARGETS_LINE_MAX_BYTES`]）—— 这是 C 组整改的核心：
/// 单行物化不再随文件增长。
///
/// ⚠️ **整改五 D-2**：`\r` 的修剪现在**主要由 [`BoundedLineReader`] 做**（`\r\n` 结尾的行 ——
/// 与 `lines()` 同口径）⇒ 这里的 `strip_suffix('\r')` 只剩一个**仍然有效**的场合：
/// **文件末行没有 `'\n'`** 且以 `\r` 收尾（读器按 `lines()` 的口径**不**吃它，而本端点的采样
/// 口径**照旧**吃它）。这不是"多此一举"：删掉它会让**末行**的采样口径与整改前不同。
fn take_target_line(
    set: &mut BTreeSet<String>,
    line: &[u8],
    lines_read: &mut usize,
    max_lines: usize,
) -> bool {
    *lines_read += 1;
    let s = String::from_utf8_lossy(line);
    let s: &str = s.strip_suffix('\r').unwrap_or(&s);
    if let Some((_, _, target, _)) = parse_json_log_line(s) {
        if !target.is_empty() {
            set.insert(target);
        }
    }
    *lines_read >= max_lines
}

/// 从 `off` 起读最多 `n` 字节（读满或遇 EOF 为止）。
async fn read_at(f: &mut tokio::fs::File, off: u64, n: u64) -> std::io::Result<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut buf = vec![0u8; n as usize];
    f.seek(std::io::SeekFrom::Start(off)).await?;
    let mut done = 0usize;
    while done < buf.len() {
        let r = f.read(&mut buf[done..]).await?;
        if r == 0 {
            break;
        }
        done += r;
    }
    buf.truncate(done);
    Ok(buf)
}

/// 采样块里**首个**可解析行的时间戳（块首可能是被劈开的半行 ⇒ 解析不出来，自然跳过）。
fn first_parsable_ts(buf: &[u8]) -> Option<u64> {
    let s = String::from_utf8_lossy(buf);
    s.lines().find_map(|l| parse_json_log_line(l).map(|t| t.0))
}

/// 采样块里**末个**可解析行的时间戳（块尾同理可能是半行）。
fn last_parsable_ts(buf: &[u8]) -> Option<u64> {
    let s = String::from_utf8_lossy(buf);
    s.lines().rev().find_map(|l| parse_json_log_line(l).map(|t| t.0))
}

/// R4：读到了行但**一行都解析不出来** ⇒ 响亮告警（**机器细节只进 `tracing`，不上屏**）。
fn warn_if_log_format_looks_blind(stats: &ScanStats) {
    if stats.lines_read > 0 && stats.parsed_lines == 0 {
        tracing::warn!(
            lines_read = stats.lines_read,
            "读到了日志行但**无一行**可解析成契约条目（JSON 层 / 字段名可能已整体漂移）⇒ 屏上会显示为「无日志」，与'确实没有日志'不可区分；请核对 main.rs 的 tracing JSON 层与 parse_json_log_line"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 纯函数（解析 / 截断 / 组页）
// ═══════════════════════════════════════════════════════════════════════════

/// **硬代价闸**超限页：`entries` **空**、`has_more=false`、`range_too_large=true`。
///
/// **只给 [`ScanState::budget_exhausted`] 用**（行数闸 [`SCAN_READ_BUDGET_LINES`] / 字节闸
/// [`SCAN_READ_BUDGET_BYTES`] 耗尽）—— 这是已评审通过的**方案 (a)**（见模块头「硬代价闸」）。
///
/// **为什么不回"已扫到的半截"**：契约字段的语义是"`entries` **不代表完整结果**"，
/// 而截断位置由扫描顺序（文件/行预算）决定，**与筛选条件无关** ⇒ 半截结果会让用户以为
/// "这些就是最新的日志"（渲染端第三条形态 `Incomplete` 也正是按空页设计的）。
///
/// ⚠️ **如实登记的待裁（整改五 A 组点名的不一致）**：**窗口超限**（EDGE-15 / 文件数超限）
/// 自整改五起改为**带条目返回**（{@link finish_page} 的 `range_too_large=true` 形态），
/// **硬闸仍是空页**。两者的 `range_too_large` **契约语义完全相同**（都是"`entries` 不代表完整
/// 结果"），返回形态却不同 ⇒ 是否把硬闸也统一成"带条目返回"**待 PM 裁定**；本轮**不改**硬闸
/// 形态（"方案 (a)"是上一轮已评审通过的结论，改它需要 PM 开口）。
fn too_large_page() -> LogPage {
    LogPage {
        entries: Vec::new(),
        next_cursor: None,
        has_more: false,
        range_too_large: true,
    }
}

/// 用 `kept`（最新 `limit+1` 条）组页：取最新 `limit` 条。
///
/// 键 `(seq, 文件序)` 升序遍历 + `.rev()` ⇒ **`seq` 降序**，且同 `seq`（跨文件同毫秒）时
/// **新文件在前**（`文件序` 越大越新，R6）。
///
/// # `range_too_large` 参数（整改五 A 组）
///
/// 由调用方给出：正常路径 `false`；**窗口内容超限 / 文件数超限**路径 `true`
/// （此时返回的是**已收集的**最新 `limit` 条 + 标志 —— 不再回空页，理由见 [`ScanState::take`]
/// 的口径论证）。`has_more` / `next_cursor` 的算法两条路径**一致**（`has_more` = `kept` 里还有
/// 第 `limit+1` 条 ⇒ "还有更早的未返回"），因为超限路径下 `kept` 装的就是**已扫到的**内容，
/// 语义与正常路径同构。
fn finish_page(
    kept: BTreeMap<(u64, u32), LogEntry>,
    limit: usize,
    range_too_large: bool,
) -> LogPage {
    let has_more = kept.len() > limit;
    let entries: Vec<LogEntry> = kept.into_values().rev().take(limit).collect();
    // `next_cursor` = 本页**最小** `seq`（"下一页（更早）"的续点）；无更多 ⇒ `None`。
    let next_cursor = if has_more {
        entries.last().map(|e| e.seq)
    } else {
        None
    };
    LogPage {
        entries,
        next_cursor,
        has_more,
        range_too_large,
    }
}

/// 单条消息截断（**≤ [`MESSAGE_MAX_BYTES`] 字节**；超出 ⇒ 尾部追加 [`TRUNCATION_MARKER`]）。
///
/// 按**字符边界**截（不劈开 UTF-8 字符）；截断后总字节数 ≤ 1024（含标记）。未超限则**原样返回**
/// （一个字节都不动）。
pub fn truncate_message(msg: &str) -> String {
    if msg.len() <= MESSAGE_MAX_BYTES {
        return msg.to_string();
    }
    let budget = MESSAGE_MAX_BYTES - TRUNCATION_MARKER.len();
    let mut cut = budget;
    while cut > 0 && !msg.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = String::with_capacity(MESSAGE_MAX_BYTES);
    out.push_str(&msg[..cut]);
    out.push_str(TRUNCATION_MARKER);
    out
}

/// 解析一行 `tracing_subscriber::fmt().json()` 输出。
///
/// 形状（`main.rs` 的 JSON 层 + `with_target(true)`）：
/// `{"timestamp":"2026-09-17T07:12:34.567890Z","level":"INFO","target":"mupcd","fields":{"message":"…"}}`
///
/// # 跳过（`None`）的分支与理由
///
/// | 情形 | 处置 | 理由 |
/// |------|------|------|
/// | 非 JSON / 无 `timestamp` / 时间戳不可解析 | 跳过 | 无时刻 ⇒ 无法放进窗口、无法算 `seq`（**不臆造时间**） |
/// | `level` 不在契约 5 档内 | 跳过 | 契约只有 5 档；**编一个级别**（如默认 INFO）是谎报级别，跳过错得明白 |
/// | `fields.message` 缺失 | **保留**（`message = ""`） | 事件确实存在（如 `tracing::info!(k = v)` 无消息体）；丢行会让"日志少了一条"无从解释 |
fn parse_json_log_line(line: &str) -> Option<(u64, LogLevel, String, String)> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let ts = v.get("timestamp")?.as_str()?;
    let ts_ms = chrono::DateTime::parse_from_rfc3339(ts).ok()?.timestamp_millis();
    if ts_ms < 0 {
        return None;
    }
    let level = match v.get("level")?.as_str()?.to_ascii_lowercase().as_str() {
        "error" => LogLevel::Error,
        "warn" => LogLevel::Warn,
        "info" => LogLevel::Info,
        "debug" => LogLevel::Debug,
        "trace" => LogLevel::Trace,
        _ => return None,
    };
    let target = v
        .get("target")
        .and_then(|t| t.as_str())
        .unwrap_or_default()
        .to_string();
    let message = v
        .get("fields")
        .and_then(|f| f.get("message"))
        .and_then(|m| m.as_str())
        .unwrap_or_default()
        .to_string();
    Some((ts_ms as u64, level, target, message))
}

/// `mupc.log.YYYY-MM-DD` ⇒ 该日 `[00:00:00, 23:59:59.999]`（Unix ms，**UTC**）。
///
/// 文件名不含可解析日期（如裸 `mupc.log`）⇒ `None`（调用方按"无法判定 ⇒ 不跳过"处理，
/// 宁可多扫一个文件也**不静默漏**）。
fn file_day_window(path: &Path) -> Option<(u64, u64)> {
    let name = path.file_name()?.to_str()?;
    let date = name.strip_prefix(LOG_FILE_PREFIX)?.trim_start_matches('.');
    let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let start = d.and_hms_opt(0, 0, 0)?;
    let start_ms = start.and_utc().timestamp_millis() as u64;
    Some((start_ms, start_ms + 86_400_000 - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    // ── 脚手架 ──────────────────────────────────────────────────────────────

    /// 一行 `tracing` JSON（与 `main.rs` 的 JSON 层同形状）。
    fn json_line(ts_iso: &str, level: &str, target: &str, msg: &str) -> String {
        serde_json::json!({
            "timestamp": ts_iso,
            "level": level,
            "target": target,
            "fields": { "message": msg },
        })
        .to_string()
    }

    /// 2025-09-09 12:00:00.000Z 起的第 `secs` 秒。
    const BASE_MS: u64 = 1_757_412_000_000;

    /// 某日 00:00:00Z（Unix ms）——文件日剪枝用例的窗口端点。
    fn day_ms(y: i32, m: u32, d: u32) -> u64 {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis() as u64
    }

    fn iso_of(ms: u64) -> String {
        chrono::DateTime::from_timestamp_millis(ms as i64)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    }

    fn svc(dir: &TempDir) -> LogService {
        LogService::new(dir.path(), LogLimits::default())
    }

    fn q_default() -> LogQuery {
        LogQuery {
            range: LogRange::Custom,
            from_ms: Some(BASE_MS - 1),
            to_ms: Some(BASE_MS + 600_000),
            ..Default::default()
        }
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(k, s)| (k.to_string(), s.to_string())).collect()
    }

    /// **挂死护栏**（B-1）：本项目多次遇到"循环类失败表现为**挂死**而不是报错"（B-1 就是活例：
    /// 倒读空转 ⇒ 测试会**卡住**而不是变红）。凡涉及大文件 / 大行数 / 倒读的用例一律套本护栏，
    /// 让挂死以 `panic!` 的形式**变红**。
    async fn with_watchdog<F, T>(what: &str, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        match tokio::time::timeout(std::time::Duration::from_secs(30), fut).await {
            Ok(v) => v,
            Err(_) => panic!("{what}：超过 30 s 仍未返回（疑似倒读不收敛 / 死循环）"),
        }
    }

    // ── ① 查询解析 / 校验 ───────────────────────────────────────────────────

    /// **多值 = 重复键**（§3.4 补注）；逗号拼接必须**被拒**（否则两侧各有一套编码）。
    #[test]
    fn multi_value_params_are_repeated_keys_not_comma_joined() {
        let q = parse_query(&pairs(&[
            ("range", "1h"),
            ("levels", "error"),
            ("levels", "warn"),
            ("targets", "a"),
            ("targets", "b"),
            ("limit", "20"),
        ]))
        .expect("重复键必须解析成功");
        assert_eq!(q.levels, vec![LogLevel::Error, LogLevel::Warn]);
        assert_eq!(q.targets, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(q.limit, 20);

        // 逗号拼接 ⇒ **拒**（不得被当成两个级别）
        let err = parse_query(&pairs(&[("levels", "error,warn")])).unwrap_err();
        assert!(err.contains("levels 非法"), "{err}");
        let err = parse_query(&pairs(&[("range", "1h,24h")])).unwrap_err();
        assert!(err.contains("range 非法"), "{err}");
    }

    /// 校验表逐条（每条都是**拒**，不是"容忍"）。
    #[test]
    fn query_validation_rejects_every_illegal_shape() {
        for (case, kv) in [
            ("未知键", vec![("nope", "1")]),
            ("levels 非法", vec![("levels", "fatal")]),
            ("targets 空值", vec![("targets", "")]),
            ("limit=0", vec![("limit", "0")]),
            ("limit>200", vec![("limit", "201")]),
            ("limit 非数", vec![("limit", "x")]),
            ("from 非数", vec![("range", "custom"), ("from", "x"), ("to", "1")]),
            ("custom 缺 to", vec![("range", "custom"), ("from", "1")]),
            ("custom 缺 from", vec![("range", "custom"), ("to", "2")]),
            ("custom from>to", vec![("range", "custom"), ("from", "9"), ("to", "1")]),
            ("相对档给 from", vec![("range", "1h"), ("from", "1")]),
            ("相对档给 to", vec![("range", "24h"), ("to", "1")]),
        ] {
            let e = parse_query(&pairs(&kv)).err();
            assert!(e.is_some(), "`{case}` 必须被拒，实际通过: {kv:?}");
        }
        // 正例：缺 range ⇒ 默认 1h（不是非法）；缺 limit ⇒ 取契约上限
        let q = parse_query(&pairs(&[])).expect("空参数合法");
        assert_eq!(q.range, LogRange::H1);
        assert_eq!(q.limit, LOG_PAGE_LIMIT_MAX);
    }

    /// `window()` 三档；`custom` 用给定 `from`/`to`（**不读时钟**）。
    #[test]
    fn window_is_relative_for_presets_and_explicit_for_custom() {
        let now = BASE_MS;
        let h1 = LogQuery { range: LogRange::H1, ..Default::default() };
        assert_eq!(h1.window(now), (now - 3_600_000, now));
        let h24 = LogQuery { range: LogRange::H24, ..Default::default() };
        assert_eq!(h24.window(now), (now - 86_400_000, now));
        assert_eq!(q_default().window(now), (BASE_MS - 1, BASE_MS + 600_000));
    }

    // ── ② 截断（≤1 KiB + 可见标注）────────────────────────────────────────

    /// 超 1 KiB ⇒ 截断且**标注可见**；恰好 1 KiB ⇒ **原样**（一个字节不动）；多字节字符不劈开。
    #[test]
    fn message_over_1kib_is_truncated_with_visible_marker() {
        assert_eq!(MESSAGE_MAX_BYTES, 1024, "跨侧约定（渲染端 ASSUMED_MAX_MESSAGE_BYTES）");

        let exact = "a".repeat(MESSAGE_MAX_BYTES);
        assert_eq!(truncate_message(&exact), exact, "恰好 1 KiB 不得截断");

        let long = "a".repeat(MESSAGE_MAX_BYTES + 1);
        let t = truncate_message(&long);
        assert!(t.len() <= MESSAGE_MAX_BYTES, "字节数必须 ≤1 KiB：{}", t.len());
        assert!(t.ends_with(TRUNCATION_MARKER), "截断必须可见（尾部标注）");
        assert_eq!(t.len(), MESSAGE_MAX_BYTES);

        // 多字节：按字符边界截（不得产生非法 UTF-8 / 半个汉字）
        let cjk = "汉".repeat(1000); // 3000 B
        let t = truncate_message(&cjk);
        assert!(t.len() <= MESSAGE_MAX_BYTES);
        assert!(t.ends_with(TRUNCATION_MARKER));
        assert!(t.starts_with('汉') && t.trim_end_matches('.').chars().all(|c| c == '汉'));
    }

    // ── ③ 限额扫描（EDGE-15）──────────────────────────────────────────────

    /// 窗口查询（`custom`，起止由调用方给）。
    fn win(from: u64, to: u64) -> LogQuery {
        LogQuery {
            range: LogRange::Custom,
            from_ms: Some(from),
            to_ms: Some(to),
            ..Default::default()
        }
    }

    /// 一份"密集"日志：`n` 行、每行相隔 1 ms、从 [`BASE_MS`] 起（总跨度 `n` ms）。
    fn dense_file(t: &TempDir, n: u64) -> PathBuf {
        let mut body = String::with_capacity(n as usize * 128);
        for i in 0..n {
            body.push_str(&json_line(&iso_of(BASE_MS + i), "INFO", "mupc_big", "x"));
            body.push('\n');
        }
        t.write("mupc.log.2025-09-09", &body)
    }

    /// **文件数超限** ⇒ `range_too_large=true` 且**未打开第 6 个文件**；已扫到的 5 条**照样返回**。
    ///
    /// 证据 = `files_scanned`（在 `File::open` 成功之后立刻 `+= 1`）**恰为 5**，而不是 6。
    ///
    /// ⚠️ **本用例因「整改五 A 组」裁定而改口径**（原断言 `entries.is_empty()` =
    /// "超限不得回半截结果"）：超限现在回 **已收集的最新 `limit` 条** + 标志 ⇒ 断言**更强**
    /// （既断言 flag，又断言 5 条的具体条数与内容），理由见 [`ScanState::take`] 与模块头「超限时回什么」。
    #[tokio::test]
    async fn scan_file_limit_flags_range_too_large_and_never_opens_the_next_file() {
        let t = TempDir::new("logs-files");
        // 6 个**都在窗口内**的候选文件（日期 09-04..09-09；窗口取这 6 天 ⇒ 日剪枝不会提前 break）
        for d in 4..=9 {
            let name = format!("mupc.log.2025-09-{d:02}");
            t.write(&name, &format!("{}\n", json_line(&iso_of(BASE_MS), "INFO", "m", "x")));
        }

        let q = LogQuery {
            range: LogRange::Custom,
            from_ms: Some(day_ms(2025, 9, 4)),
            to_ms: Some(day_ms(2025, 9, 9) + 86_400_000 - 1),
            ..Default::default()
        };
        let (page, stats) = svc(&t).page_with_stats(&q, BASE_MS).await.expect("不得 Err");
        assert!(page.range_too_large, "6 个文件 > 5 ⇒ 必须判超限");
        // A 组裁定后：超限**带回已收集条目**（5 个文件各 1 条，第 6 个根本没打开）
        assert_eq!(page.entries.len(), 5, "已扫到的 5 条必须返回：{:?}", page.entries);
        assert!(page.entries.iter().all(|e| e.message == "x"), "{:?}", page.entries);
        assert!(!page.has_more, "5 条 < limit ⇒ 无'更多'");
        assert_eq!(page.next_cursor, None);
        assert_eq!(stats.files_scanned, 5, "只准打开 5 个文件：{stats:?}");
        assert_eq!(stats.lines_read, 5);
    }

    /// **窗口内容超限** ⇒ `range_too_large=true`、**读到上限+1 行即停**、**且带回已收集的最新
    /// `limit` 条**。
    ///
    /// 判据是「**落在窗口内且通过筛选**的行数」（[`ScanStats::window_lines`]），不是"读入的总行数"。
    ///
    /// ⚠️ **本用例因「整改五 A 组」裁定而改口径**（原断言 `entries.is_empty()`）：超限页现在带条目
    /// ⇒ 断言**更强** —— 除了 flag 与扫描统计，还钉死**交付内容**（条数 = `limit`、首条 = 已扫部分
    /// 里 `seq` 最大的那条、`seq` 严格降序、全在窗口内）。理由见 [`ScanState::take`]。
    ///
    /// ⚠️ 交付内容只能是**"已收集的"**：本夹具的择向是**正读**（窗口覆盖整份文件 ⇒ 窗口左端贴文件头
    /// ⇒ [`LogService::choose_direction`] 选 `Forward`）⇒ 扫到上限时"已收集"= 文件**前** 50 001 行里
    /// 最新的 200 条（`seq` 最大者 = 第 49 999 行），**不是**整份文件最新的那几条。这是裁定原文
    /// "返回已收集的最新 `limit` 条"的字面含义，不是缺陷。
    #[tokio::test]
    async fn window_content_over_the_cap_flags_range_too_large_and_stops_at_the_cap() {
        const N: u64 = 60_000;
        let t = TempDir::new("logs-wide-window");
        dense_file(&t, N);
        // 窗口**覆盖整份文件**：窗口内 60 000 行 > 50 000 ⇒ 必须判超限
        let q = win(BASE_MS, BASE_MS + N - 1);
        let (page, stats) = svc(&t).page_with_stats(&q, BASE_MS + 600_000).await.expect("不得 Err");
        assert!(page.range_too_large, "窗口内容超限必须判超限：{stats:?}");
        assert_eq!(stats.window_lines, LOG_SCAN_MAX_LINES + 1, "{stats:?}");
        assert_eq!(
            stats.lines_read,
            LOG_SCAN_MAX_LINES + 1,
            "必须在上限+1 行处立即停止（文件实际 {N} 行）：{stats:?}"
        );

        // ── A 组裁定：超限回**已收集的最新 `limit` 条**（不再是空页）──────────────
        assert_eq!(stats.forward_files, 1, "本夹具的择向应为正读（贴着文件头）：{stats:?}");
        assert_eq!(
            page.entries.len(),
            LOG_PAGE_LIMIT_MAX,
            "必须恰好是 limit 条已收集内容：{:?}",
            page.entries.len()
        );
        assert_eq!(
            page.entries[0].ts_ms,
            BASE_MS + LOG_SCAN_MAX_LINES as u64 - 1,
            "首条 = 已扫部分里 seq 最大的那条（正读 ⇒ 上限 -1 行）：{:?}",
            page.entries[0]
        );
        for w in page.entries.windows(2) {
            assert!(w[0].seq > w[1].seq, "交付内容必须 seq 严格降序");
        }
        assert!(
            page.entries.iter().all(|e| e.ts_ms >= BASE_MS && e.ts_ms < BASE_MS + N),
            "交付内容必须全在窗口内"
        );
        // `kept` 留了 `limit+1` 条 ⇒ 本页之外**还有**更早的 ⇒ `has_more`（与正常路径同算法）
        assert!(page.has_more, "{stats:?}");
        assert_eq!(page.next_cursor, page.entries.last().map(|e| e.seq));
    }

    /// **A 组裁定 · 新语义①（HMI 增量轮询的主路径）**：繁忙窗口 + `cursor` ⇒ **不得**判超限，
    /// 且必须返回**增量条目**。
    ///
    /// 机理（整改前**必然复现**的缺陷）：`window_lines` 计在 `cursor` 筛选**之前** ⇒ 已看过的
    /// 那几万行把计数顶爆 ⇒ 每个 500 ms 请求都回 `range_too_large=true` + **空页** ⇒ 渲染端
    /// `p3_logs.rs::apply_page` 见 `entries` 为空 ⇒ `shown=0` ⇒ **列表被清空**并常驻
    /// 「检索范围超限 · 未执行检索」，而 `1h` 已是**最小档** ⇒ 按提示"再缩"**也没用**。
    /// 本用例在整改前是**红的**（`range_too_large=true, entries=[]`），是本组裁定的把守网。
    #[tokio::test]
    async fn a_busy_window_with_a_cursor_is_not_reported_as_too_large() {
        const N: u64 = 60_000;
        let t = TempDir::new("logs-busy-increment");
        dense_file(&t, N);
        // 渲染端 `fire_increment` 的形态：cursor = 已见最大 `seq`（取文件倒数第 3 行的 seq）
        let cursor = (BASE_MS + N - 3) * 1000;
        let q = LogQuery { cursor: Some(cursor), ..win(BASE_MS, BASE_MS + N - 1) };
        let (page, stats) = svc(&t).page_with_stats(&q, BASE_MS + 600_000).await.expect("不得 Err");
        assert!(
            !page.range_too_large,
            "增量轮询不得被判超限（否则 HMI 每 500 ms 清空一次列表）：{stats:?}"
        );
        assert_eq!(
            stats.window_lines, 2,
            "只有 2 条通过 cursor 筛选 ⇒ 计数**只算这 2 条**（计数位置 = 全部筛选之后）：{stats:?}"
        );
        assert_eq!(page.entries.len(), 2, "增量条目必须返回：{:?}", page.entries);
        assert!(page.entries.iter().all(|e| e.seq > cursor), "{:?}", page.entries);
        assert_eq!(page.entries[0].ts_ms, BASE_MS + N - 1, "最新一条在前");
        assert_eq!(page.entries[1].ts_ms, BASE_MS + N - 2);
    }

    /// **A 组裁定 · 新语义②**：无 `cursor` + 窗口内容超限 ⇒ `range_too_large=true` **且**
    /// `entries` **非空**、**恰好是最新的 `limit` 条**（首条 = 全局 `seq` 最大者）。
    ///
    /// 与上一个超限用例的区别：本夹具的窗口**贴文件尾** ⇒ 择向走**倒读** ⇒ "已收集"正是
    /// **全局最新**的那批（把裁定原文"返回已收集的最新 `limit` 条"的两个方向都钉住）。
    #[tokio::test]
    async fn over_cap_window_without_cursor_returns_the_newest_limit_entries_with_the_flag() {
        const N: u64 = 60_000;
        let t = TempDir::new("logs-over-cap-newest");
        dense_file(&t, N);
        let q = LogQuery { limit: 10, ..win(BASE_MS + 5_000, BASE_MS + N - 1) };
        let (page, stats) = svc(&t).page_with_stats(&q, BASE_MS + 600_000).await.expect("不得 Err");
        assert!(page.range_too_large, "55 000 行 > 50 000 ⇒ 必须判超限：{stats:?}");
        assert_eq!(stats.backward_files, 1, "窗口贴文件尾 ⇒ 必须倒读：{stats:?}");
        assert_eq!(page.entries.len(), 10, "恰好 `limit` 条（不是 0、也不是半截）");
        assert_eq!(page.entries[0].ts_ms, BASE_MS + N - 1, "首条 = 全局最新那条");
        assert_eq!(
            page.entries[0].seq,
            (BASE_MS + N - 1) * 1000,
            "首条的 seq 必须是全体最大的那个"
        );
        for (i, e) in page.entries.iter().enumerate() {
            assert_eq!(
                e.ts_ms,
                BASE_MS + N - 1 - i as u64,
                "必须是**连续**最新的 `limit` 条（第 {i} 条）"
            );
        }
        assert!(stats.window_lines > LOG_SCAN_MAX_LINES, "{stats:?}");
        assert!(page.has_more, "`kept` 留了 `limit+1` 条 ⇒ 还有更早的：{stats:?}");
        assert_eq!(page.next_cursor, page.entries.last().map(|e| e.seq));
    }

    /// **A 组裁定 · 新语义③（钉住计数位置）**：同一窗口、同一份文件，加一个把内容压到上限
    /// **之下**的 `levels` 筛选 ⇒ **不再超限**。
    ///
    /// 这条只有在"计数在全部筛选之后"时才成立：计数若仍在筛选之前（旧口径），无论怎么筛都会
    /// 在 50 001 行处超限 ⇒ 本用例**红**。
    #[tokio::test]
    async fn a_level_filter_that_shrinks_the_window_content_clears_the_over_cap_flag() {
        const N: u64 = 60_000;
        let t = TempDir::new("logs-cap-vs-filter");
        dense_file(&t, N); // 全是 INFO
        let q_all = win(BASE_MS, BASE_MS + N - 1);
        let (page_all, stats_all) = svc(&t).page_with_stats(&q_all, BASE_MS + 600_000).await.unwrap();
        assert!(page_all.range_too_large, "对照：不筛 ⇒ 超限 {stats_all:?}");

        // 同一个窗口、同一份文件，只加 `levels=[ERROR]`（本文件一行 ERROR 都没有）
        let q_err = LogQuery {
            levels: vec![LogLevel::Error],
            ..win(BASE_MS, BASE_MS + N - 1)
        };
        let (page_err, stats_err) = svc(&t).page_with_stats(&q_err, BASE_MS + 600_000).await.unwrap();
        assert!(
            !page_err.range_too_large,
            "筛选后的交付规模 = 0 ⇒ 不得判超限：{stats_err:?}"
        );
        assert_eq!(
            stats_err.window_lines, 0,
            "`window_lines` 必须在**全部筛选之后**计数：{stats_err:?}"
        );
        assert!(page_err.entries.is_empty() && !page_err.has_more, "没有 ERROR 行 ≠ 超限");
    }

    /// **B 组**：同一毫秒的巨量行（写者卡顿后一次性刷出 / 外部工具批量追加）**不得**让倒读的
    /// 同毫秒组无界增长。
    ///
    /// 机理（整改前**必然发生**）：`group` 没有任何长度判据，而 `kept` 的 `limit+1` 上界与
    /// `window_lines` 上界都只在 `flush_group` → `take` 里生效 ⇒ 一个同毫秒的 N 行爆发会先让
    /// `group` 长到 N 条（每条 2 个 `String`、`message` **未截断**）⇒ `kept` 处那句
    /// "内存上界 = `limit+1` 条，与文件大小无关"**不成立**。现在到 `max_lines` 即判 `too_large`
    /// （**可见拒绝**，不静默丢条）。
    ///
    /// 夹具结构（**每一处都是必要的**）：首行一条**极旧**的行 ⇒ 把择向扳到**倒读**；中段是同
    /// 毫秒爆发（比 `max_lines` 大 50 倍）；尾段 500 行**各自独立毫秒** ⇒ 倒读先收尾它们、
    /// **照常交付**（A 组裁定：超限页带已收集条目），再撞上爆发。
    #[tokio::test]
    async fn a_single_millisecond_burst_cannot_grow_the_group_beyond_the_cap() {
        const CAP: usize = 1_000;
        const BURST: usize = 50_000; // 50 × cap
        let t = TempDir::new("logs-burst");
        let mut body = String::with_capacity(BURST * 120 + 64 * 1024);
        // ① 极旧的一行（窗口外）⇒ 令 `choose_direction` 的 `forward_span` 远大于 `backward_span`
        body.push_str(&json_line(&iso_of(BASE_MS - 1_000_000), "INFO", "m", "ancient"));
        body.push('\n');
        // ② 同毫秒爆发（窗口内、比 `max_lines` 大 50 倍）
        for i in 0..BURST {
            body.push_str(&json_line(&iso_of(BASE_MS), "INFO", "m", &format!("burst{i}")));
            body.push('\n');
        }
        // ③ 尾巴：500 行各自独立毫秒（倒读先收尾它们 ⇒ `kept` 非空）
        for i in 0..500u64 {
            body.push_str(&json_line(&iso_of(BASE_MS + 1_000 + i), "INFO", "m", &format!("tail{i}")));
            body.push('\n');
        }
        t.write("mupc.log.2025-09-09", &body);

        let limits = mupc_display_proto::config::LogLimits { max_lines: CAP, ..Default::default() };
        let s = LogService::new(t.path(), limits);
        let q = win(BASE_MS, BASE_MS + 600_000);
        let (page, stats) = with_watchdog("同一毫秒爆发", s.page_with_stats(&q, BASE_MS + 7_200_000))
            .await
            .expect("不得 Err");

        assert_eq!(stats.backward_files, 1, "夹具必须把择向扳到倒读：{stats:?}");
        assert!(page.range_too_large, "组超上限必须**可见拒绝**（不得静默丢条）：{stats:?}");
        assert!(
            stats.lines_read <= CAP + 512,
            "必须**早停**（不得把 50 000 行爆发读完）：{stats:?}"
        );
        // A 组裁定：超限页带回**已收集**的条目 ⇒ 尾巴那批照常交付
        assert_eq!(page.entries.len(), LOG_PAGE_LIMIT_MAX, "limit 默认 200：{stats:?}");
        assert_eq!(page.entries[0].ts_ms, BASE_MS + 1_499, "首条 = 全局最新那条");
        assert!(
            page.entries.iter().all(|e| e.message.starts_with("tail")),
            "交付内容不得混入爆发组的行：{:?}",
            page.entries.iter().map(|e| &e.message).take(3).collect::<Vec<_>>()
        );
    }

    /// **R3 主判据（评审 PROBE4 的反例）**：60 000 行的文件 + **1 ms** 窗口 + 窗口内**1 条**命中
    /// ⇒ **不得**判 `range_too_large`，那一条必须被返回，且**不得**读满 5 万行。
    ///
    /// 整改前的行为（实测复现）：`range_too_large=true, entries=0, lines_read=50_001`
    /// —— 屏上说"请缩小时间范围"，而窗口已经窄到 1 ms，**再缩也没用**（EDGE-15 的原文是
    /// 「起止区间**跨度过大**」）。`range_too_large` **只能**表达"窗口内容超过限额"。
    #[tokio::test]
    async fn narrow_window_in_a_huge_file_is_served_and_costs_about_half() {
        const N: u64 = 60_000;
        let t = TempDir::new("logs-narrow-window");
        dense_file(&t, N);
        let s = svc(&t);

        // ① 窗口在文件**中部**（文件尾/头两侧距离相当）⇒ 择向把代价压到约一半
        let q = win(BASE_MS + 30_000, BASE_MS + 30_000);
        let (page, stats) = s.page_with_stats(&q, BASE_MS + 600_000).await.expect("不得 Err");
        assert!(
            !page.range_too_large,
            "窄窗口不得被判超限（EDGE-15 判的是窗口内容，不是文件长度）：{stats:?}"
        );
        assert_eq!(page.entries.len(), 1, "{stats:?}");
        assert_eq!(page.entries[0].ts_ms, BASE_MS + 30_000);
        assert_eq!(stats.window_lines, 1, "{stats:?}");
        assert!(
            stats.lines_read < LOG_SCAN_MAX_LINES,
            "不得读满 5 万行：{stats:?}"
        );
        assert!(stats.lines_read <= 31_000, "择向应把读入量压到约一半：{stats:?}");

        // ② 窗口在文件**头部** ⇒ 择向改走"正读"，代价 ≈ 窗口右端之前的行数
        let q_head = win(BASE_MS + 1_000, BASE_MS + 1_000);
        let (page_head, stats_head) =
            s.page_with_stats(&q_head, BASE_MS + 600_000).await.expect("不得 Err");
        assert!(!page_head.range_too_large, "{stats_head:?}");
        assert_eq!(page_head.entries.len(), 1);
        // ⚠️ **语义变更（B-3，评审要求）**：整改前是 `<= 1_100`（读到窗口右端就停）。B-3 给
        // 正读早退加了**迟滞**（连续 ≥ 一个 64 KiB 块才算"后面都超出窗口右端"）⇒ 代价 =
        // 窗口右端之前的行数（1 000）+ 迟滞块（≈690，实测 1 650）。语义断言（窗口内那一条
        // 必须被返回、不得判 `range_too_large`）原样保留，此处只改**代价上界**。
        assert!(
            stats_head.lines_read <= 2_000,
            "窗口靠文件头时应正读，读入量 ≈ 1 000 行 + 一个迟滞块：{stats_head:?}"
        );
        assert_eq!(stats_head.forward_files, 1, "该窗口在文件头 ⇒ 必须正读：{stats_head:?}");
    }

    /// **R3-③（覆盖缺口）**：择向逻辑**零覆盖** ⇒ "尾部窗口要便宜"这条**主路径承诺**无人守。
    ///
    /// 复核实测：把 `choose_direction` **写死返回 `Forward`**，`log_service::tests::` 35 条**全绿**
    /// ⇒ 连"倒读每行恰一次"那张网也一并失效（它只管完整性，不管**走的是哪个方向**）。
    /// 而"尾部窗口便宜"正是 HMI 的真实主路径：60 000 行文件 + 贴尾 100 行窗口（复核正对照实测：
    /// `lines_read: 101`）。
    ///
    /// 本条把择向**上锁**（[`ScanStats::backward_files`]）并直接断言**主路径代价**：
    /// 写死 `Forward` ⇒ 全读 60 000 行 ⇒ 两条断言都红（实测）。
    ///
    /// # ⚠️ 怎么正确地验证"这条代价上界有没有牙"（整改三补注，**别再用错方法**）
    ///
    /// - ✅ **拆掉早退本身**（让 [`ScanState::note_stop_side_line`] 恒返回 `false`）⇒ 倒读一路
    ///   读到文件头 ⇒ `lines_read` 立刻从 101 变成 60 000 ⇒ 本条 `<= 1_000` **变红**。
    ///   **这才是**"上界有没有牙"的检验。
    /// - ❌ **拆掉迟滞**（让早退立即生效）**不可能是**有效检验：那只会**减少**读入量
    ///   （更早收工），数学上 `lines_read <= 1_000` **必然仍绿** ⇒ 会得出"界太松/无用例"的
    ///   **错误结论**。同理，"窗口内那几条必须返回"这类**语义**断言也必须另配反例，不能拿它当代价网的牙。
    ///
    /// 简记：**语义网拆语义来源，代价网拆早退来源**；拆错了方向，网看起来永远是绿的。
    #[tokio::test]
    async fn a_window_at_the_tail_of_a_huge_file_must_pick_backward_and_stay_cheap() {
        const N: u64 = 60_000;
        let t = TempDir::new("logs-tail-window");
        dense_file(&t, N);
        let q = win(BASE_MS + N - 100, BASE_MS + N + 600_000); // 窗口内 = 最后 100 行
        let (page, stats) = with_watchdog(
            "R3-③ 贴尾窗口",
            svc(&t).page_with_stats(&q, BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert_eq!(
            stats.backward_files, 1,
            "尾部窗口是 HMI 主路径 ⇒ **必须**择向倒读（把 choose_direction 写死 Forward 时这条变红）：{stats:?}"
        );
        assert_eq!(stats.forward_files, 0, "{stats:?}");
        assert!(
            stats.lines_read <= 1_000,
            "贴尾窗口的代价必须是**一个块**量级（正向全读 = 60 000 行）：{stats:?}"
        );
        assert_eq!(page.entries.len(), 100, "{stats:?}");
        assert_eq!(page.entries[0].ts_ms, BASE_MS + N - 1, "首条 = 文件最后一行");
        assert!(!page.range_too_large);
    }

    /// **分块倒读的完整性网**：整文件走一遍 ⇒ **每行恰处理一次**（不丢、不重）。
    ///
    /// 倒读按 64 KiB 分块（60 000 行 ≈ 90 个块边界）。本用例把窗口开到比文件还宽
    /// （`end` 远超文件尾 ⇒ 择向必然是**倒读**）并放开行数限额 ⇒ 必须
    /// `lines_read == window_lines == parsed_lines == N`：**少一行 = 边界处丢行**
    /// （实施中真实发生过一次：第一版丢 91 行），**多一行 = 边界处把同一行处理两次**。
    #[tokio::test]
    async fn backward_chunked_read_sees_every_line_exactly_once() {
        const N: u64 = 60_000;
        let t = TempDir::new("logs-chunks");
        dense_file(&t, N);
        let limits = LogLimits {
            max_lines: 200_000, // 放开限额，逼它把整份文件读完
            ..LogLimits::default()
        };
        let s = LogService::new(t.path(), limits);

        let q = win(BASE_MS, BASE_MS + 1_000_000); // 远宽于文件 ⇒ 择向 = 倒读
        let (page, stats) = s.page_with_stats(&q, BASE_MS + 600_000).await.expect("不得 Err");
        assert_eq!(stats.lines_read, N as usize, "每行恰读一次：{stats:?}");
        assert_eq!(stats.window_lines, N as usize, "窗口覆盖整份文件 ⇒ 一行不少：{stats:?}");
        assert_eq!(
            stats.parsed_lines, N as usize,
            "块首的半行不得被当垃圾丢掉（它必须在下一轮连同前半段一起被读到）：{stats:?}"
        );
        assert!(!page.range_too_large && page.entries.len() == LOG_PAGE_LIMIT_MAX, "{stats:?}");
        // 屏上第一条 = 文件最后一行（时间跨度 60 s 的末端）
        assert_eq!(page.entries[0].ts_ms, BASE_MS + N - 1);
    }

    /// **B-1（阻塞，最高危）**：一行**自身 ≥ 64 KiB** 时，倒读**绝不允许**空转。
    ///
    /// 机理（复核实测）：倒读取"块内第一个 `'\n'` 之后"作为本块起点；当**块内首个 `'\n'` 恰是块的
    /// 最后一个字节**时，起点 == 块长 ⇒ 本块一行也不处理、`pos` 又不推进、`off != 0` 故不 break
    /// ⇒ **永不收敛**（探针实测迭代 61 次仍在转，无护栏时进程被 `timeout` 杀掉）。触发条件 =
    /// 存在一行自身 ≥ 64 KiB（含换行符）。HMI 的 `1h`/增量主路径在"当日文件"下**正是倒读**
    /// ⇒ 只要最新文件里有这么一行，**当天日志页永久不可用**（每个 500 ms 请求再挂一个空转任务）。
    ///
    /// 本条同时钉两件事：① 倒读**收敛**（有 `with_watchdog`，挂死 ⇒ 变红而不是卡住）；
    /// ② 那条超长行**被完整读出、解析、按 1 KiB 截断上屏**（"跳过它"不算修好）。
    ///
    /// 改坏验证：删掉 `read_backward` 的翻窗分支/进度守卫 ⇒ 本条在 30 s 护栏处**变红**（实测）。
    #[tokio::test]
    async fn a_line_longer_than_the_read_chunk_is_assembled_on_the_backward_path() {
        let t = TempDir::new("logs-huge-line");
        let huge_msg = "H".repeat(80 * 1024); // 单行 ≈ 82 KiB > 一个 64 KiB 块
        let mut body = String::new();
        for i in 0..3u64 {
            body.push_str(&json_line(&iso_of(BASE_MS + i * 1000), "INFO", "m", &format!("head-{i}")));
            body.push('\n');
        }
        body.push_str(&json_line(&iso_of(BASE_MS + 100_000), "ERROR", "mupc_big", &huge_msg));
        body.push('\n');
        for i in 0..3u64 {
            let ts = BASE_MS + 200_000 + i * 1000;
            body.push_str(&json_line(&iso_of(ts), "INFO", "m", &format!("tail-{i}")));
            body.push('\n');
        }
        t.write("mupc.log.2025-09-09", &body);

        // 窗口贴尾且覆盖那条超长行 ⇒ 择向必为**倒读**（HMI 主路径的形态）
        let q = win(BASE_MS + 99_000, BASE_MS + 203_000);
        let (page, stats) = with_watchdog(
            "B-1 超长行倒读",
            svc(&t).page_with_stats(&q, BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert_eq!(stats.backward_files, 1, "本用例必须跑在**倒读**路径上：{stats:?}");
        assert!(!page.range_too_large, "{stats:?}");
        assert_eq!(stats.lines_read, 7, "每行恰读一次（含那条超长行）：{stats:?}");
        assert_eq!(
            stats.parsed_lines, 7,
            "超长行必须被**完整**读出并解析（整改前它会让倒读空转，屏上永远拿不到日志）：{stats:?}"
        );
        let huge = page
            .entries
            .iter()
            .find(|e| e.level == LogLevel::Error)
            .expect("那条超长行必须在结果里");
        assert_eq!(huge.ts_ms, BASE_MS + 100_000);
        assert_eq!(huge.message.len(), MESSAGE_MAX_BYTES, "按 1 KiB 截断上屏");
        assert!(huge.message.ends_with(TRUNCATION_MARKER), "截断必须可见");
        assert_eq!(page.entries.len(), 4, "窗口内 = 超长行 + 尾部 3 条：{stats:?}");
    }

    /// **R3 的判据网（把"窗口内容"与"扫描行数"两个判据分开）**：`range_too_large`
    /// **只**由**窗口内容**决定，与"读入（扫过）了多少行"**无关**。
    ///
    /// 做法：把限额压到 1 000，再看 60 000 行文件里 1 ms 的窗口（窗口内恰 1 行）。
    /// 为找到这一行，扫描必须**读入**约 3 万行（它们全在窗口外）⇒ 两个判据在此**必然分歧**：
    /// 「窗口内容 = 1 ≤ 1000」不超限（**正确**），「扫描行数 = 30001 > 1000」超限（**整改前的错误**）。
    /// 评审 PROBE4 的机理正是后者 ⇒ 屏上「请缩小时间范围」是假话（窗口缩到 1 ms 也没用）。
    ///
    /// ⚠️ 本条是**补测**：最初的 R3 用例在"限额=50 000 + 已加早退"下，两个判据**不会分歧**
    /// （读入量已被压到 3 万行以下）⇒ 把判据改回"扫描行数"**不会变红**（实测）。故必须把限额
    /// 单独压小，才能把两个判据分开。
    ///
    /// ⚠️ **覆盖缺口（整改三，评审抓到）**：本条的 `lines_read`（≈3 万）**小于硬代价闸阈值**
    /// （[`SCAN_READ_BUDGET_LINES`] = 20 万）⇒ 它**测不出**"硬闸挂错判据（挂到读入总行数上）"
    /// 这一形态 —— 整改二正是这么挂的，而本条**照样全绿**。补网 =
    /// [`tests::a_fully_parsable_huge_file_is_never_flagged_by_the_hard_gate`]
    /// （500 000 行全可解析、`lines_read` ≈25 万 > 20 万，改坏即红）。
    #[tokio::test]
    async fn range_too_large_is_decided_by_window_content_not_by_lines_scanned() {
        const N: u64 = 60_000;
        const TIGHT: usize = 1_000;
        let t = TempDir::new("logs-judge");
        dense_file(&t, N);
        let limits = LogLimits {
            max_lines: TIGHT,
            ..LogLimits::default()
        };
        let s = LogService::new(t.path(), limits);

        let q = win(BASE_MS + 30_000, BASE_MS + 30_000); // 窗口内恰 1 行
        let (page, stats) = s.page_with_stats(&q, BASE_MS + 600_000).await.expect("不得 Err");
        assert!(
            stats.lines_read > TIGHT,
            "本用例的前置：扫描行数必须**远超**限额（否则两个判据不分歧，用例失去区分度）：{stats:?}"
        );
        assert_eq!(stats.window_lines, 1, "{stats:?}");
        assert!(
            !page.range_too_large,
            "窗口内容没超限 ⇒ 不得判 range_too_large（判据是窗口内容，不是扫描行数）：{stats:?}"
        );
        assert_eq!(page.entries.len(), 1, "{stats:?}");
        assert_eq!(page.entries[0].ts_ms, BASE_MS + 30_000);
    }

    /// **B-2（阻塞）**：**不可解析**的大文件 ⇒ 代价必须有界（**硬代价闸**），且结果**不得**是
    /// `entries=[] ∧ range_too_large=false`。
    ///
    /// 机理（复核实测）：判据从"扫描行数"换成"窗口内行数"后，**解析不出来的行既不计入 `window_lines`、
    /// 也永不触发早退** ⇒ 120 000 行纯文本（`max_lines = 50_000`）被**整份读完**（复数实测
    /// `lines_read: 120000, window_lines: 0, parsed_lines: 0`），且**每个** 500 ms 请求都这样读一遍。
    /// 触发源都是现实形态：日志格式漂移（正是 R4 要告警的那个场景）、被 logrotate 压过的二进制、
    /// 别的工具写进 `mupc.log*`。
    ///
    /// 本条夹具取 **300 000 行**（> 硬闸 [`SCAN_READ_BUDGET_LINES`] = 200 000）⇒ 断言 `lines_read`
    /// **恰在闸门处**停住（实测数字见交付报告），而不是"读完整个文件"。
    ///
    /// ⚠️ **夹具是"行行都解析失败"** ⇒ `unparsable_lines == lines_read`，故整改三把闸门判据从
    /// `lines_read` 换成 `unparsable_lines` 后**本条的数字原样成立**（20 万+1）；本条**不能**用来
    /// 区分这两个判据 —— 区分它的是 [`tests::a_fully_parsable_huge_file_is_never_flagged_by_the_hard_gate`]。
    /// 本条的牙在别处：把闸（`note_unparsable_line`）**摘掉** ⇒ 读满 300 000 行
    /// ⇒ 上面的 `lines_read == 200 001` 断言**变红**（实测：`left: 300000, right: 200001`）。
    #[tokio::test]
    async fn unparsable_huge_file_hits_the_hard_cost_gate_and_never_claims_empty() {
        const N: u64 = 300_000;
        let t = TempDir::new("logs-hard-budget");
        let mut body = String::with_capacity(N as usize * 16);
        for i in 0..N {
            body.push_str(&format!("plain text line {i}\n")); // 一行都解析不出（R4 场景的真实规模版）
        }
        t.write("mupc.log.2025-09-09", &body);

        let (page, stats) = with_watchdog(
            "B-2 不可解析大文件",
            svc(&t).page_with_stats(&q_default(), BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert_eq!(stats.parsed_lines, 0, "夹具必须一行都解析不出：{stats:?}");
        assert_eq!(stats.window_lines, 0, "{stats:?}");
        assert_eq!(
            stats.lines_read,
            SCAN_READ_BUDGET_LINES + 1,
            "必须在硬代价闸处立即停止（夹具 {N} 行，闸 = {SCAN_READ_BUDGET_LINES}）：{stats:?}"
        );
        assert!(
            stats.lines_read < N as usize,
            "不得读完整个文件（整改前正是读满 {N} 行）：{stats:?}"
        );
        // 语义（硬红线）：不得是 `entries=[] ∧ range_too_large=false` —— 那等于把"没查完"
        // 说成"确实没有"。本模块选**方案 (a)**：回 `range_too_large=true`（契约语义 = "entries
        // 不代表完整结果"，这一点诚实）+ `tracing::warn!` 点名真实成因（见模块头「硬代价闸」）。
        assert!(page.range_too_large, "预算耗尽必须显式置位：{stats:?}");
        assert!(page.entries.is_empty());
        assert!(!page.has_more);
        assert_eq!(page.next_cursor, None);

        // 正对照：同样不可解析、但**行数在闸门以内**的小文件 ⇒ 仍按 R4 的口径走"空态 + 告警"
        // （不是超限）⇒ 本条断言不是恒真（证明超限真的由**预算耗尽**引起，而不是"纯文本即超限"）。
        let t2 = TempDir::new("logs-hard-budget-small");
        t2.write("mupc.log.2025-09-09", "plain text line 1\nplain text line 2\n");
        let (small, st2) = svc(&t2)
            .page_with_stats(&q_default(), BASE_MS + 600_000)
            .await
            .expect("不得 Err");
        assert!(!small.range_too_large && small.entries.is_empty(), "{st2:?}");
    }

    /// **重要-2（整改三，评审实测）· ① ≥ 1 MiB 的单个行**：代价必须有上界，且结果**不得**是
    /// `entries=[] ∧ range_too_large=false`。
    ///
    /// 机理（复核实测）：翻窗重读与"超长行跳过"这两条路径**一行也不产出** ⇒ 它们读入的字节
    /// **完全不计入任何闸门**。整改前一份**一行 1.5 MiB** 的文件实测 `lines_read=3`、
    /// 读入 3 174 599 B（≈2.0×），**那条窗口内的条目从结果里消失**且 `range_too_large=false`
    /// ⇒ 屏上无任何提示（只有 `tracing::warn!`）。
    ///
    /// 本条把字节账**钉死**：`skipped_bytes` 有界（≤ 闸 + 一个读窗），且预算耗尽 ⇒
    /// **可见拒绝**（`range_too_large=true`）。取 **2 MiB** 单行：它 > 读窗上限 1 MiB ⇒
    /// 结构性装不进任何一个窗口 ⇒ 必然走字节闸（取值论证见 [`SCAN_READ_BUDGET_BYTES`]）。
    #[tokio::test]
    async fn a_multi_mibibyte_single_line_hits_the_byte_gate_and_never_claims_empty() {
        let t = TempDir::new("logs-byte-gate-line");
        let huge = "H".repeat(2 * 1024 * 1024); // 单行 ≈ 2 MiB（> MAX_REVERSE_WINDOW_BYTES）
        t.write(
            "mupc.log.2025-09-09",
            &format!("{}\n", json_line(&iso_of(BASE_MS), "ERROR", "mupc_big", &huge)),
        );

        let (page, stats) = with_watchdog(
            "重要-2 超长单行",
            svc(&t).page_with_stats(&q_default(), BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert!(
            stats.skipped_bytes > 0,
            "翻窗重读必须**按字节记账**（整改前这里是 0，代价无人管）：{stats:?}"
        );
        assert!(
            stats.skipped_bytes <= SCAN_READ_BUDGET_BYTES + MAX_REVERSE_WINDOW_BYTES,
            "字节代价必须有上界（闸 = {SCAN_READ_BUDGET_BYTES}）：{stats:?}"
        );
        assert!(stats.lines_read <= 8, "行数也必须是有界的（不是整份读）：{stats:?}");
        // 硬红线：不得把"没查完"说成"确实没有"
        assert!(
            !page.entries.is_empty() || page.range_too_large,
            "绝不回 `entries=[] ∧ range_too_large=false`（整改前正是这个）：{stats:?}"
        );
        assert!(page.range_too_large, "字节闸耗尽 ⇒ 必须显式置位：{stats:?}");
        assert!(page.entries.is_empty() && !page.has_more);
        assert_eq!(page.next_cursor, None);
    }

    /// **重要-2 · ② 全文无换行的大文件**：同上，且这正是 `entries=[] ∧ range_too_large=false`
    /// 的**最纯粹**反例（复核实测整改前：4 MiB 无换行文件 ⇒ `lines_read=1, rt=false, entries=0,
    /// read_bytes=8_257_536`，**2.0×**，闸永不触发）。
    ///
    /// **改坏验证**（实测）：让 [`ScanState::note_unparsable_bytes`] 变成**空操作**（恒 `false`，
    /// 即整改前的"完全不计账"）⇒ 本条在"绝不回 `entries=[] ∧ range_too_large=false`"处**变红**，
    /// 实测回包 = `lines_read: 1, skipped_bytes: 0, entries=[] ∧ range_too_large=false`
    /// —— 与复核抓到的 4 MiB 样本**同一形态**。
    #[tokio::test]
    async fn a_newline_free_large_file_hits_the_byte_gate_and_never_claims_empty() {
        let t = TempDir::new("logs-byte-gate-nonewline");
        // 8 MiB **一个换行都没有**（logrotate 压过的二进制 / 别的工具写进来的真实形状）
        let body = "N".repeat(8 * 1024 * 1024);
        t.write("mupc.log.2025-09-09", &body);

        let (page, stats) = with_watchdog(
            "重要-2 无换行大文件",
            svc(&t).page_with_stats(&q_default(), BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert!(
            stats.skipped_bytes <= SCAN_READ_BUDGET_BYTES + MAX_REVERSE_WINDOW_BYTES,
            "字节代价必须有上界（文件 8 MiB、闸 = {SCAN_READ_BUDGET_BYTES}）：{stats:?}"
        );
        assert!(
            stats.lines_read <= 8,
            "无换行内容不得被逐行计入（整改前这里靠行数闸永远拦不住）：{stats:?}"
        );
        assert!(
            !page.entries.is_empty() || page.range_too_large,
            "绝不回 `entries=[] ∧ range_too_large=false`：{stats:?}"
        );
        assert!(page.range_too_large, "字节闸耗尽 ⇒ 必须显式置位：{stats:?}");
        assert!(page.entries.is_empty() && !page.has_more);
    }

    /// **重要-3（整改三，评审实测）**：硬闸**不得**在**完全可解析**的内容上误判 ——
    /// `range_too_large` 由**窗口内容**裁决，不由"扫了多少行"裁决。
    ///
    /// 机理（复核实测）：整改二把硬闸挂在 `lines_read` 上 ⇒ 一份 **500 000 行全部可解析**的
    /// 文件 + 文件中部 **1 ms** 窗口实测回 `lines_read=200001, window_lines=0/1,
    /// range_too_large=true, entries=[]` ⇒ 屏上说「检索范围超限 · 请缩小时间范围」，
    /// 而窗口已经窄到 1 ms、**再缩也没用** —— 这正是上一轮把 R3 判 REQUEST_CHANGES 的**同一句话**。
    ///
    /// 现在硬闸只对**解析失败的行**（与跳过的字节）计数 ⇒ 本样本必须被**正常服务**：
    /// `!range_too_large` 且窗口内那 1 条**必须返回**。
    ///
    /// **改坏验证**（实测）：把硬闸换回整改二的形态 —— `note_line_read` 改回返回 `Step` 并在
    /// **每一行**上判 `stats.lines_read > SCAN_READ_BUDGET_LINES` ⇒ 本条在
    /// `window_lines == 1` 处即变红（实测 `left: 0, right: 1`，`lines_read=200001,
    /// window_lines=0, parsed_lines=200000` —— 与复核的实测数字**逐字段一致**）。
    #[tokio::test]
    async fn a_fully_parsable_huge_file_is_never_flagged_by_the_hard_gate() {
        const N: u64 = 500_000;
        let t = TempDir::new("logs-hard-gate-parsable");
        dense_file(&t, N);

        // 窗口 = 文件**中部**的 1 ms（窗口内恰 1 行）；倒读要穿过约 N/2 行才能走到它
        let q = win(BASE_MS + 250_000, BASE_MS + 250_000);
        let (page, stats) = with_watchdog(
            "重要-3 可解析大文件 + 窄窗口",
            svc(&t).page_with_stats(&q, BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert!(
            stats.lines_read > SCAN_READ_BUDGET_LINES,
            "本用例的前置：读入量必须**超过**行数闸阈值（否则测不出'闸挂错了判据'）：{stats:?}"
        );
        assert_eq!(stats.unparsable_lines, 0, "夹具全部可解析 ⇒ 硬闸的运行判据为 0：{stats:?}");
        assert_eq!(stats.skipped_bytes, 0, "{stats:?}");
        assert_eq!(stats.window_lines, 1, "窗口内恰 1 行：{stats:?}");
        assert!(
            !page.range_too_large,
            "完全可解析的内容**只**由窗口内容判据裁决（窗口内 1 行 ≪ 限额 50 000）⇒ 不得判超限：{stats:?}"
        );
        assert_eq!(page.entries.len(), 1, "窗口内那一条必须返回：{stats:?}");
        assert_eq!(page.entries[0].ts_ms, BASE_MS + 250_000);
        assert!(stats.lines_read < N as usize, "择向应把读入量压到约一半：{stats:?}");
    }

    /// 回归网：**"文件长"本身永远不构成 `range_too_large`**。
    ///
    /// 窗口落在文件**之前**（窗口内 0 行）⇒ 空态（EDGE-08），**不是**超限（EDGE-15）。
    /// 整改前：任何超过 50 000 行的文件都会被判超限，无论窗口多窄、有没有命中。
    #[tokio::test]
    async fn a_long_file_alone_never_means_range_too_large() {
        const N: u64 = 60_000;
        let t = TempDir::new("logs-long-only");
        dense_file(&t, N);
        let q = win(BASE_MS - 10_000, BASE_MS - 5_000); // 窗口里什么都没有
        let (page, stats) = svc(&t).page_with_stats(&q, BASE_MS + 600_000).await.expect("不得 Err");
        assert!(!page.range_too_large, "文件长 ≠ 超限：{stats:?}");
        assert!(page.entries.is_empty());
        assert_eq!(stats.window_lines, 0, "{stats:?}");
        // ⚠️ **语义变更（B-3，评审要求）**：整改前这里是 `lines_read <= 2`（看到首行 `ts > end`
        // 立刻 `EndOfFile`）。B-3 给正读的早退**加了迟滞**（连续 ≥ 一个 64 KiB 块才停，
        // 否则一条时钟跳变的行会把"之后仍在窗口内的行"整段丢掉）⇒ 本用例的代价从 1 行变成
        // **一个迟滞块**（实测 649 行，仍 ≪ 全文件 60 000 行）。**不是**迁就实现让红变绿：
        // `!range_too_large ∧ entries 空 ∧ window_lines == 0` 三条语义断言原样保留。
        assert!(
            stats.lines_read <= 1_000,
            "窗口在文件之外 ⇒ 读一个迟滞块即可判定（≤64 KiB ≈ 690 行）：{stats:?}"
        );
        assert!(stats.forward_files == 1, "该窗口在文件头侧 ⇒ 必须正读：{stats:?}");
    }

    // ── ③' R1 / R6 / R7（迁移回退、跨文件同 ms、删改竞态）──────────────────

    /// **R1**：一个**与日志文件同名**的目录不得让整条日志端点 503，也**不得**占扫描预算。
    ///
    /// 整改前：只按文件名前缀收集 ⇒ `File::open(目录)` 失败 ⇒ `page()` 返回 `Err` ⇒ **503**，
    /// 整个日志页不可用。迁出前的 `web-api::routes::logs::LogsHandler` **有** `path.is_file()`。
    #[tokio::test]
    async fn a_directory_named_like_a_log_file_does_not_break_the_endpoint() {
        let t = TempDir::new("logs-dir-clash");
        t.write(
            "mupc.log.2025-09-09",
            &format!("{}\n", json_line(&iso_of(BASE_MS), "INFO", "m", "keep")),
        );
        std::fs::create_dir(t.join("mupc.log.2025-09-10")).unwrap();
        let s = svc(&t);

        let names: Vec<String> = s
            .list_log_files()
            .await
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["mupc.log.2025-09-09"], "只收常规文件");

        let (page, stats) = s
            .page_with_stats(&q_default(), BASE_MS + 60_000)
            .await
            .expect("同名目录不得让端点 503");
        assert_eq!(page.entries.len(), 1, "{stats:?}");
        assert!(!page.range_too_large);
        assert_eq!(stats.files_scanned, 1, "同名目录不是候选文件，也不占文件预算：{stats:?}");
    }

    /// **R6**：两个文件里各有一条**同一毫秒**的日志 ⇒ **两条都要在**，且**新文件那条在前**。
    ///
    /// 整改前：`kept.insert(seq, …)` 用单键 ⇒ 后扫到的**旧文件**那条**覆盖**新文件那条
    /// ⇒ 不只丢一条，是**内容错**（留下的是过期的）。`seq` 因此在这两条上**相同**
    /// （契约的"单调"在同毫秒跨文件时不成立，已在模块头登记）。
    #[tokio::test]
    async fn same_millisecond_across_two_files_keeps_both_and_newest_first() {
        let t = TempDir::new("logs-xfile-ms");
        t.write(
            "mupc.log.2025-09-08",
            &format!("{}\n", json_line(&iso_of(BASE_MS), "INFO", "m", "older-file")),
        );
        t.write(
            "mupc.log.2025-09-09",
            &format!("{}\n", json_line(&iso_of(BASE_MS), "INFO", "m", "newer-file")),
        );
        let page = svc(&t).page(&q_default(), BASE_MS + 60_000).await.unwrap();
        let msgs: Vec<&str> = page.entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            msgs,
            vec!["newer-file", "older-file"],
            "同 ms 跨文件：两条都要保留，且新文件在前"
        );
        assert_eq!(
            page.entries[0].seq, page.entries[1].seq,
            "同 ms ⇒ seq 相同（契约的'单调'在同 ms 下不成立，已登记）"
        );
    }

    /// **R7**：文件在"列目录"与"打开"之间消失（轮转 / 被清理）⇒ **跳过该文件**，不是整请求 503；
    /// 其余 IO 错误仍必须 `Err`（不得把"读不了"混进"没有日志"）。
    #[tokio::test]
    async fn a_file_vanishing_between_listing_and_open_is_skipped_not_503() {
        let t = TempDir::new("logs-race");
        let s = svc(&t);
        let q = q_default();
        let (start, end) = q.window(BASE_MS);

        // ① `NotFound` ⇒ 跳过（`Ok`），且不计入 `files_scanned`
        let missing = t.join("mupc.log.2025-09-09");
        let mut st = ScanState::new(&q, start, end, LOG_SCAN_MAX_LINES);
        assert!(
            s.scan_file(&missing, &mut st, 0).await.is_ok(),
            "轮转竞态必须跳过而不是 Err（否则一次轮转 = 整个日志页 503）"
        );
        assert_eq!(st.stats.files_scanned, 0, "没打开成的文件不计入：{:?}", st.stats);

        // ② 其它 IO 错误仍 `Err`（拿一个**目录**当文件路径：Windows=PermissionDenied，
        //    Linux=打开后 read 得 EISDIR ⇒ 两种都不是 NotFound）
        let dir = t.join("mupc.log.2025-09-10");
        std::fs::create_dir(&dir).unwrap();
        let mut st2 = ScanState::new(&q, start, end, LOG_SCAN_MAX_LINES);
        assert!(
            s.scan_file(&dir, &mut st2, 0).await.is_err(),
            "非 NotFound 的 IO 错误必须 Err ⇒ 503"
        );
    }

    // ── ④ 空态 ≠ 超限态（两个信号不得互替）───────────────────────────────

    #[tokio::test]
    async fn no_matching_entries_is_empty_page_not_range_too_large() {
        let t = TempDir::new("logs-empty");
        t.write(
            "mupc.log.2025-09-09",
            &format!("{}\n", json_line(&iso_of(BASE_MS), "ERROR", "m", "boom")),
        );
        // 级别筛到 INFO ⇒ 0 条命中，但**没有**超限
        let q = LogQuery { levels: vec![LogLevel::Info], ..q_default() };
        let page = svc(&t).page(&q, BASE_MS).await.unwrap();
        assert!(page.entries.is_empty());
        assert!(!page.range_too_large, "无日志 ≠ 超限（硬口径）");
        assert!(!page.has_more);
        assert_eq!(page.next_cursor, None);

        // 目录里一个日志文件都没有（目录存在）= 确实没有 ⇒ 同样是空态而非超限
        let t2 = TempDir::new("logs-nodir-file");
        let page2 = svc(&t2).page(&q_default(), BASE_MS).await.unwrap();
        assert!(page2.entries.is_empty() && !page2.range_too_large);

        // 目录**不存在** ⇒ Err（handler 落 503），不得回空页冒充"无日志"
        let missing = t.path().join("nope");
        let svc3 = LogService::new(&missing, LogLimits::default());
        assert!(svc3.page(&q_default(), BASE_MS).await.is_err(), "源不可用必须 Err");
    }

    // ── ⑤ 排序 / 分页 / 增量游标 ──────────────────────────────────────────

    /// 造 30 条（每 1 s 一条，INFO/ERROR 交替）。
    fn thirty(t: &TempDir) -> String {
        let mut body = String::new();
        for i in 0..30u64 {
            let lvl = if i % 2 == 0 { "ERROR" } else { "INFO" };
            body.push_str(&json_line(&iso_of(BASE_MS + i * 1000), lvl, "mupc_gateway", "m"));
            body.push('\n');
        }
        t.write("mupc.log.2025-09-09", &body);
        body
    }

    /// `has_more` / `next_cursor` 的**两侧**：截断侧（还有更早）与"已到最早"侧。
    #[tokio::test]
    async fn has_more_and_next_cursor_cover_both_sides() {
        let t = TempDir::new("logs-page");
        thirty(&t);
        let s = svc(&t);

        // 侧 A：范围内 30 条、limit=10 ⇒ 最新 10 条、has_more=true、next_cursor=本页最小 seq
        let q = LogQuery { limit: 10, ..q_default() };
        let page = s.page(&q, BASE_MS + 60_000).await.unwrap();
        assert_eq!(page.entries.len(), 10);
        assert!(page.has_more);
        // 倒序：seq 严格递减，且第 0 条是**最新**（i=29）
        for w in page.entries.windows(2) {
            assert!(w[0].seq > w[1].seq, "必须按 seq 降序");
        }
        assert_eq!(page.entries[0].ts_ms, BASE_MS + 29_000);
        assert_eq!(page.entries[9].ts_ms, BASE_MS + 20_000);
        assert_eq!(page.next_cursor, Some(page.entries[9].seq), "下一頁游标 = 本页最小 seq");
        assert!(!page.range_too_large);

        // 侧 B：范围内只有 5 条、limit=10 ⇒ has_more=false、next_cursor=None（已到最早）
        let q5 = LogQuery {
            range: LogRange::Custom,
            limit: 10,
            from_ms: Some(BASE_MS + 25_000),
            to_ms: Some(BASE_MS + 600_000),
            ..Default::default()
        };
        let p5 = s.page(&q5, BASE_MS + 60_000).await.unwrap();
        assert_eq!(p5.entries.len(), 5);
        assert!(!p5.has_more, "已到最早 ⇒ 无更多");
        assert_eq!(p5.next_cursor, None);
    }

    /// `cursor` = **取更新的**（`seq > cursor`），且"已到最新" ⇒ 空页 + `has_more=false`
    /// （**不是**超限、**不是**重复拉取）。
    #[tokio::test]
    async fn cursor_means_strictly_newer_and_latest_is_empty_not_duplicate() {
        let t = TempDir::new("logs-cursor");
        thirty(&t);
        let s = svc(&t);

        let first = s.page(&q_default(), BASE_MS + 60_000).await.unwrap();
        assert_eq!(first.entries.len(), 30);
        let max_seq = first.entries[0].seq;

        // 已到最新：cursor = 最大 seq ⇒ 空页、无更多、**未超限**
        let page = s
            .page(&LogQuery { cursor: Some(max_seq), ..q_default() }, BASE_MS + 60_000)
            .await
            .unwrap();
        assert!(page.entries.is_empty(), "seq > cursor 无命中 ⇒ 空页");
        assert!(!page.has_more && !page.range_too_large && page.next_cursor.is_none());

        // 追加 3 条新日志 ⇒ 只有这 3 条被返回（**不重复**既有 30 条）
        let mut body = std::fs::read_to_string(t.join("mupc.log.2025-09-09")).unwrap();
        for i in 30..33u64 {
            body.push_str(&json_line(&iso_of(BASE_MS + i * 1000), "INFO", "mupc_gateway", "new"));
            body.push('\n');
        }
        std::fs::write(t.join("mupc.log.2025-09-09"), body).unwrap();

        let inc = s
            .page(&LogQuery { cursor: Some(max_seq), ..q_default() }, BASE_MS + 60_000)
            .await
            .unwrap();
        assert_eq!(inc.entries.len(), 3, "只回更新的 3 条");
        assert!(inc.entries.iter().all(|e| e.seq > max_seq));
        assert_eq!(inc.entries[0].ts_ms, BASE_MS + 32_000);
        assert!(!inc.has_more);
    }

    /// `seq` **稳定**：同一条目在两次请求里 `seq` 相同；**窗口内**旧文件的**增 / 删**都不得
    /// 平移既有 `seq`。
    ///
    /// ⚠️ **整改五 E-1：本用例此前是恒真的**（评审点名）。旧版往目录里放的是
    /// `mupc.log.2025-09-01`，它（a）被 `file_day_window` 的"留一天余量"规则在 `page_with_stats`
    /// 里**直接 `break` 掉、根本没被打开**，（b）条目时间 `BASE_MS - 999_000` **也在窗口外**
    /// ⇒ 那一次请求与"没放文件"**逐字节等价** ⇒ `a == c` 与 `seq` 算法**无关**。
    /// 实测证据（E-1 破坏性探针）：把 `seq` 的算法换成"组页时按交付集合从最旧一条起重新编号"，
    /// **旧版用例依旧全绿**；本版**变红**（`seq: 1757412000000029` → `1757411700000033`）。
    #[tokio::test]
    async fn seq_is_stable_across_requests_and_old_file_removal() {
        let t = TempDir::new("logs-seq");
        thirty(&t);
        let s = svc(&t);
        // 窗口放宽到 ±10 min ⇒ 下面那个"更旧的文件"里的条目**也落在窗口内**（否则它根本不会被
        // 扫到，断言又成恒真）
        let q = LogQuery {
            range: LogRange::Custom,
            from_ms: Some(BASE_MS - 600_000),
            to_ms: Some(BASE_MS + 600_000),
            ..Default::default()
        };
        let a = s.page(&q, BASE_MS + 60_000).await.unwrap();
        assert_eq!(a.entries.len(), 30, "此时只有新版文件");

        let b = s.page(&q, BASE_MS + 60_000).await.unwrap();
        assert_eq!(a.entries, b.entries, "同一份文件 ⇒ 逐字段一致（含 seq）");

        // ① 加一个**更旧、但条目在窗口内**的文件 ⇒ 既有条目的 seq **逐条**不得变化
        const OLD_NAME: &str = "mupc.log.2025-09-08";
        let mut old_body = String::new();
        for i in 0..4u64 {
            old_body
                .push_str(&json_line(&iso_of(BASE_MS - 300_000 + i * 1000), "WARN", "mupc_old", "old"));
            old_body.push('\n');
        }
        t.write(OLD_NAME, &old_body);
        let c = s.page(&q, BASE_MS + 60_000).await.unwrap();
        // **先证明旧文件真的被扫到了**（否则下面那条断言还是恒真的）
        assert_eq!(c.entries.len(), 34, "窗口内的旧文件 ⇒ 它的 4 条必须出现：{}", c.entries.len());
        assert_eq!(
            c.entries.iter().filter(|e| e.target == "mupc_old").count(),
            4,
            "旧文件的 4 条必须在结果里"
        );
        // 既有 30 条**逐条**比对 `seq`（不是只比条数、也不是只比集合相等）
        let seq_of: std::collections::HashMap<(u64, String), u64> =
            c.entries.iter().map(|e| ((e.ts_ms, e.message.clone()), e.seq)).collect();
        for e in &a.entries {
            assert_eq!(
                seq_of.get(&(e.ts_ms, e.message.clone())),
                Some(&e.seq),
                "加了一个**窗口内**的旧文件后，既有条目的 seq 不得平移（ts={}, msg={}）",
                e.ts_ms,
                e.message
            );
        }
        // 新旧条目不得撞 `seq`（`seq` 是 cursor 的高水位 ⇒ 撞车会漏条）
        let all: BTreeSet<u64> = c.entries.iter().map(|e| e.seq).collect();
        assert_eq!(all.len(), c.entries.len(), "seq 必须互不相同（不得因跨文件而撞车）");

        // ② 再把旧文件**删掉** ⇒ 必须回到与 `a` **逐字段一致**（增 / 删都不动既有 seq）
        std::fs::remove_file(t.path().join(OLD_NAME)).expect("删掉旧文件");
        let d = s.page(&q, BASE_MS + 60_000).await.unwrap();
        assert_eq!(d.entries, a.entries, "删掉旧文件后必须回到原样（逐字段，含 seq）");
    }

    /// 同毫秒多条的 `k` 递增（`seq` 不撞车 ⇒ `cursor` 增量不漏条）。
    #[tokio::test]
    async fn same_millisecond_entries_get_distinct_seq() {
        let t = TempDir::new("logs-same-ms");
        let mut body = String::new();
        for i in 0..5 {
            body.push_str(&json_line(&iso_of(BASE_MS), "INFO", "m", &format!("m{i}")));
            body.push('\n');
        }
        t.write("mupc.log.2025-09-09", &body);
        let page = svc(&t).page(&q_default(), BASE_MS + 60_000).await.unwrap();
        assert_eq!(page.entries.len(), 5, "同毫秒 5 条必须都在（不得按 seq 去重掉）");
        let seqs: BTreeSet<u64> = page.entries.iter().map(|e| e.seq).collect();
        assert_eq!(seqs.len(), 5, "seq 必须互不相同");
        assert_eq!(page.entries[0].message, "m4", "同一毫秒内后写的 seq 更大");
    }

    /// 筛选：`levels` / `targets` 各自独立生效，且**空集 = 不筛**。
    #[tokio::test]
    async fn level_and_target_filters_apply_independently() {
        let t = TempDir::new("logs-filter");
        let body = format!(
            "{}\n{}\n",
            json_line(&iso_of(BASE_MS), "ERROR", "mupc_gateway", "g-err"),
            json_line(&iso_of(BASE_MS + 1000), "INFO", "mupc_intercore", "i-info"),
        );
        t.write("mupc.log.2025-09-09", &body);
        let s = svc(&t);

        let only_err = s
            .page(&LogQuery { levels: vec![LogLevel::Error], ..q_default() }, BASE_MS + 60_000)
            .await
            .unwrap();
        assert_eq!(only_err.entries.len(), 1);
        assert_eq!(only_err.entries[0].message, "g-err");

        let only_tgt = s
            .page(&LogQuery { targets: vec!["mupc_intercore".into()], ..q_default() }, BASE_MS + 60_000)
            .await
            .unwrap();
        assert_eq!(only_tgt.entries.len(), 1);
        assert_eq!(only_tgt.entries[0].message, "i-info");

        let all = s.page(&q_default(), BASE_MS + 60_000).await.unwrap();
        assert_eq!(all.entries.len(), 2, "空集 = 不筛");
    }

    /// 窗口：窗口外的行不进结果（`custom` 用给定的 from/to）。
    #[tokio::test]
    async fn time_window_excludes_out_of_range_lines() {
        let t = TempDir::new("logs-window");
        t.write(
            "mupc.log.2025-09-09",
            &format!(
                "{}\n{}\n",
                json_line(&iso_of(BASE_MS), "INFO", "m", "in"),
                json_line(&iso_of(BASE_MS + 120_000), "INFO", "m", "out")
            ),
        );
        let q = LogQuery {
            range: LogRange::Custom,
            from_ms: Some(BASE_MS),
            to_ms: Some(BASE_MS + 1_000),
            ..Default::default()
        };
        let page = svc(&t).page(&q, BASE_MS + 600_000).await.unwrap();
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].message, "in");
    }

    /// **阻塞-1（整改三，评审抓到的真缺陷）**：迟滞计数是**逐文件**状态，上一个文件留下的计数
    /// **不得**让下一个文件被整体跳过。
    ///
    /// 机理：`stop_side_bytes` 只在 `note_not_stop_side_line` 里清零，而 `scan_file` 入口
    /// **不重置**它（[`ScanState::begin_file`] 之前）⇒ 文件 A 早退时 **留下** ≥ 64 KiB 的计数，
    /// 文件 B 的**第一条**早退侧行立刻达阈 ⇒ B 立即 `EndOfFile` ⇒ B 里**更早位置**的窗口内行
    /// 永远读不到；回包却是 `range_too_large=false` ⇒ 屏上是一条"**看着正常但少一条**"的列表，
    /// **用户无从察觉**（比空页更坏：空页至少还看得出不对）。
    ///
    /// 夹具：A（新文件）= 800 行"远早于窗口"的行 + 末尾一条窗口内的行（倒读先拿到窗口内那条，
    /// 再撞上 800 行 ≥ 64 KiB 的早退侧内容 ⇒ `EndOfFile` **并留下计数**）；
    /// B（更旧文件）= 一条窗口内的行 + 一条远早于窗口的行（后者在倒读里**先**被遇到）。
    ///
    /// **改坏验证**（实测）：把 `begin_file` 里的 `self.stop_side_bytes = 0;`（即那行重置）摘掉
    /// ⇒ 本条在 `msgs` 断言处**变红**：`left: ["in-newer-file"]`（实测，与复核的双文件样本逐字一致）。
    #[tokio::test]
    async fn a_previous_files_hysteresis_must_not_skip_the_next_file() {
        let t = TempDir::new("logs-hysteresis-leak");
        // A：800 行远早于窗口的行（≥ 64 KiB，迟滞的量级）+ 末尾一条窗口内的行
        let mut a = String::new();
        for i in 0..800u64 {
            a.push_str(&json_line(
                &iso_of(BASE_MS - 9_000_000 - i),
                "INFO",
                "old",
                "filler-before-window",
            ));
            a.push('\n');
        }
        a.push_str(&json_line(&iso_of(BASE_MS + 1_000), "INFO", "m", "in-newer-file"));
        a.push('\n');
        t.write("mupc.log.2025-09-09", &a);

        // B：**头**是窗口内的行，**尾**是远早于窗口的行（倒读先遇到尾部那条）
        t.write(
            "mupc.log.2025-09-08",
            &format!(
                "{}\n{}\n",
                json_line(&iso_of(BASE_MS + 2_000), "INFO", "m", "in-older-file"),
                json_line(&iso_of(BASE_MS - 9_000_000), "INFO", "m", "stray-before-window"),
            ),
        );

        let q = win(BASE_MS, BASE_MS + 600_000);
        let (page, stats) = with_watchdog(
            "阻塞-1 迟滞跨文件泄漏",
            svc(&t).page_with_stats(&q, BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert_eq!(stats.backward_files, 2, "两个文件都该走倒读（窗口贴尾）：{stats:?}");
        let msgs: Vec<&str> = page.entries.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            msgs,
            vec!["in-older-file", "in-newer-file"],
            "上一个文件留下的迟滞计数**不得**让下一个文件被整体跳过（整改前这里只有 in-newer-file）：{msgs:?}"
        );
        assert_eq!(
            stats.window_lines, 2,
            "两个文件里**各自**那条窗口内的行都必须被读到（整改前 window_lines=1）：{stats:?}"
        );
        assert!(!page.range_too_large, "{stats:?}");
        assert!(
            !page.entries.is_empty(),
            "更不得退化成 `entries=[] ∧ range_too_large=false`：{msgs:?}"
        );
    }

    /// **B-3（阻塞）**：一条时间戳**偏小**的行**不得**让整个请求提前收工。
    ///
    /// 机理（复核实测，同一文件仅方向不同）：倒读命中 `ts < start` 时整改前返回 `EndOfScan`
    /// ⇒ 被上层升级为**停止整个请求**（连更旧的文件都不看）。对照实测：正序文件 =
    /// [窗口内一行, 远早于窗口的一行] ⇒ **倒读把窗口内那行彻底丢了**，返回
    /// `entries=[] ∧ range_too_large=false` ⇒ 屏上显示 EDGE-08「当前筛选条件下没有日志」
    /// —— **"确实没有"与"没去找"不可区分**（本项目硬红线）。
    ///
    /// 触发源不止多线程重排：**时钟回拨**（无 RTC 的设备首次 NTP 同步）是最现实的成因。
    ///
    /// 本条同时覆盖两件事：① 命中早退只停**本文件**（更旧的文件照常扫）；② 早退有**迟滞**
    /// （单条乱序行连"停本文件"都不该触发）。改回 `EndOfScan` ⇒ 第一条与两条 `contains` 全红（实测）。
    #[tokio::test]
    async fn a_stray_older_timestamp_does_not_truncate_this_file_nor_the_older_files() {
        let t = TempDir::new("logs-nonmonotonic");
        // 新文件：**头**是窗口内的一行、**尾**是一条时间戳远早于窗口的行（时钟回拨的真实形状）
        t.write(
            "mupc.log.2025-09-09",
            &format!(
                "{}\n{}\n",
                json_line(&iso_of(BASE_MS + 1_000), "INFO", "m", "in-newer-file"),
                json_line(&iso_of(BASE_MS - 9_000_000), "INFO", "m", "clock-rolled-back"),
            ),
        );
        // 更旧的文件：里面**也有**一条窗口内的行 —— 整改前（EndOfScan）它连看都不会被看到
        t.write(
            "mupc.log.2025-09-08",
            &format!(
                "{}\n",
                json_line(&iso_of(BASE_MS + 2_000), "INFO", "m", "in-older-file"),
            ),
        );
        let q = win(BASE_MS, BASE_MS + 600_000);

        let (page, stats) = with_watchdog(
            "B-3 非单调样本",
            svc(&t).page_with_stats(&q, BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert_eq!(stats.backward_files, 2, "两个文件都该走倒读（窗口贴尾）：{stats:?}");
        let msgs: Vec<&str> = page.entries.iter().map(|e| e.message.as_str()).collect();
        assert!(
            msgs.contains(&"in-newer-file"),
            "本文件**更早位置**的那条窗口内行不得被早退吃掉（迟滞要挡住单条乱序行）：{msgs:?}"
        );
        assert!(
            msgs.contains(&"in-older-file"),
            "**更旧的文件**仍必须被扫描（整改前 EndOfScan 会让它连看都不看）：{msgs:?}"
        );
        assert!(!page.range_too_large, "{stats:?}");
        assert!(
            !page.entries.is_empty(),
            "更不得退化成 `entries=[] ∧ range_too_large=false`（= 屏上假『无日志』）：{msgs:?}"
        );
        // 倒序：更晚的那条（+2 s，来自旧文件）在前
        assert_eq!(msgs[0], "in-older-file");
    }

    /// **B-3 的正读镜像**：`ts > end`（时钟**跳变向前**，与时钟回拨同一类成因）不得把
    /// "之后仍在窗口内的行"整段吃掉。整改前 `read_forward` 见到第一条 `ts > end` 就 `EndOfFile`。
    #[tokio::test]
    async fn a_clock_jump_line_does_not_cut_the_forward_scan_short() {
        let t = TempDir::new("logs-forward-nonmonotonic");
        t.write(
            "mupc.log.2025-09-09",
            &format!(
                "{}\n{}\n{}\n",
                json_line(&iso_of(BASE_MS + 1_000), "INFO", "m", "first-in-window"),
                json_line(&iso_of(BASE_MS + 10_000_000), "INFO", "m", "clock-jumped-forward"),
                json_line(&iso_of(BASE_MS + 1_500), "INFO", "m", "second-in-window"),
            ),
        );
        let q = win(BASE_MS, BASE_MS + 2_000); // 贴文件头 ⇒ 择向必须是正读

        let (page, stats) = with_watchdog(
            "B-3 正读镜像",
            svc(&t).page_with_stats(&q, BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert_eq!(stats.forward_files, 1, "该窗口贴文件头 ⇒ 必须正读：{stats:?}");
        let msgs: Vec<&str> = page.entries.iter().map(|e| e.message.as_str()).collect();
        assert!(
            msgs.contains(&"first-in-window") && msgs.contains(&"second-in-window"),
            "时钟跳变的那一行不得把**它之后**仍在窗口内的行吃掉：{msgs:?}"
        );
        assert!(!page.range_too_large, "{stats:?}");
    }

    /// **整改五 D-2**：**正读**路径对"超长行"必须**有上限** —— 与倒读
    /// （[`a_multi_mibibyte_single_line_hits_the_byte_gate_and_never_claims_empty`]）和
    /// `/logs/targets`（[`targets_survive_a_newline_free_file_without_materializing_it`]）
    /// **三条路径对称**。本条是同类缺陷的**第三处、也是最后一条漏网**。
    ///
    /// # 夹具（把择向**扳到正读**）
    ///
    /// 文件头是可解析行 + 窗口贴文件头 + 尾部一条**远晚于窗口**的行（把 `last_parsable_ts` 推远
    /// ⇒ `forward_span < backward_span`）⇒ [`LogService::choose_direction`] 必选 `Forward`。
    /// 断言 `stats.forward_files == 1` **钉死**这一点：否则本用例会在倒读上跑成**恒真**
    /// （倒读那边本来就有闸）。
    ///
    /// 文件中部夹一条 **1 MiB + 8 KiB** 的**合法 JSON 行**（target = `huge_target`）。
    ///
    /// # 判别力（整改前**必红**）
    ///
    /// 整改前 `read_forward` 用 `BufReader::lines()` ⇒ 那条行被**整行物化**并**解析成功**
    /// ⇒ `huge_target` 进 `entries`（并且已经付出与行长同阶的内存：`lines_read` 只 +1，
    /// 行数闸 / 字节闸**一个都不拦**）。新实现**整行跳过** ⇒ `huge_target` **不得**出现。
    ///
    /// ⚠️ 夹具规模是**算过**的，**不是随手写**（两个夹逼都钉在断言里）：
    /// 行长 ≈ 1 MiB + 8 KiB **同时**满足
    /// ① `> MAX_REVERSE_WINDOW_BYTES` ⇒ **必被跳过**；
    /// ② `≤ SCAN_READ_BUDGET_BYTES`（1.5 MiB）⇒ 字节闸**不**耗尽 ⇒ 邻居行照常交付。
    /// 少了 ② 条，"条目为空"会让"不含 `huge_target`"这句**假通过**
    /// （所以下面有"邻居行必须在"的正对照）。
    #[tokio::test]
    async fn forward_read_skips_an_overlong_line_and_keeps_its_neighbours() {
        let huge = "z".repeat(MAX_REVERSE_WINDOW_BYTES as usize + 8 * 1024);
        let t = TempDir::new("logs-forward-overlong");
        t.write(
            "mupc.log.2025-09-09",
            &format!(
                "{}\n{}\n{}\n{}\n",
                json_line(&iso_of(BASE_MS), "INFO", "first_target", "m1"),
                json_line(&iso_of(BASE_MS + 1_000), "INFO", "huge_target", &huge),
                json_line(&iso_of(BASE_MS + 2_000), "INFO", "third_target", "m3"),
                json_line(&iso_of(BASE_MS + 7_200_000), "INFO", "tail_marker", "late"),
            ),
        );
        let q = win(BASE_MS - 1, BASE_MS + 3_000);

        let (page, stats) = with_watchdog(
            "D-2 正读超长行",
            svc(&t).page_with_stats(&q, BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert_eq!(stats.forward_files, 1, "夹具必须把择向扳到**正读**：{stats:?}");
        let targets: Vec<&str> = page.entries.iter().map(|e| e.target.as_str()).collect();
        assert!(
            !targets.contains(&"huge_target"),
            "超过单行上限的行必须**整行跳过**（不解析、不入库）—— 旧实现用 \
             `BufReader::lines()` 会把它解析出来，本条即红：{targets:?}"
        );
        // 正对照：**邻居行必须照常交付** —— 否则上一条会因"整页为空"而**假通过**
        assert!(targets.contains(&"first_target"), "超长行之前的那一行丢了：{targets:?}");
        assert!(targets.contains(&"third_target"), "超长行**之后**的那一行丢了：{targets:?}");
        assert!(
            !page.range_too_large,
            "行长（≈1 MiB + 8 KiB）≤ 字节闸（{SCAN_READ_BUDGET_BYTES}）⇒ 不得判超限：{stats:?}"
        );
    }

    /// **D-2 的另一半（记账）**：正读跳过的超长行**必须走字节闸** —— 一条真的超预算的行
    /// ⇒ **可见拒绝**，绝不回 `entries=[] ∧ range_too_large=false`（把"没查完"说成"确实没有"，
    /// 本项目硬红线）。
    ///
    /// 与 [`forward_read_skips_an_overlong_line_and_keeps_its_neighbours`] 成对：那条钉"跳过 + 邻居
    /// 照常交付"，本条钉"跳过的字节**进账**"。取 **2 MiB** 单行（> 1 MiB 上限 ⇒ 必被跳过；
    /// **≫ 1.5 MiB 字节闸** ⇒ 必耗尽），窗口仍贴文件头 ⇒ 择向正读（同样断言 `forward_files == 1`）。
    #[tokio::test]
    async fn forward_read_charges_a_huge_skipped_line_to_the_byte_gate() {
        let huge = "H".repeat(2 * 1024 * 1024);
        let t = TempDir::new("logs-forward-overlong-gate");
        t.write(
            "mupc.log.2025-09-09",
            &format!(
                "{}\n{}\n{}\n",
                json_line(&iso_of(BASE_MS), "INFO", "first_target", "m1"),
                json_line(&iso_of(BASE_MS + 1_000), "INFO", "huge_target", &huge),
                // 尾部这条是**择向的必需品**：末样本块（尾 64 KiB）必须能采到一个可解析时间戳，
                // 否则 `choose_direction` 落 `_ => Backward`（本用例就跑不到正读路径上）
                json_line(&iso_of(BASE_MS + 7_200_000), "INFO", "tail_marker", "late"),
            ),
        );
        let q = win(BASE_MS - 1, BASE_MS + 3_000);

        let (page, stats) = with_watchdog(
            "D-2 正读超长行入账",
            svc(&t).page_with_stats(&q, BASE_MS + 600_000),
        )
        .await
        .expect("不得 Err");

        assert_eq!(stats.forward_files, 1, "夹具必须把择向扳到**正读**：{stats:?}");
        assert!(
            stats.skipped_bytes >= 2 * 1024 * 1024,
            "跳过的那条 2 MiB 行必须**按字节计入**字节闸（整改前这里恒为 0 —— 代价无人管）：{stats:?}"
        );
        assert!(page.range_too_large, "字节闸耗尽 ⇒ 必须显式置位：{stats:?}");
        // 形态写 `rt || !empty`（而不是 `!(empty && !rt)`）：与上面同一句语义，但**不触发**
        // `clippy::nonminimal_bool` —— 本仓已有的那两处旧写法就在报这条警告，本轮不新增警告。
        assert!(
            page.range_too_large || !page.entries.is_empty(),
            "绝不回 `entries=[] ∧ range_too_large=false`：{stats:?}"
        );
    }

    /// 非 JSON 行 / 未知级别 / 无时间戳行被**跳过**（不 panic、不臆造），且**仍计入行数**；
    /// 这三行同时是**硬代价闸的行数判据**（[`ScanStats::unparsable_lines`]）。
    ///
    /// ⚠️ **整改五 E-2**：本用例此前只断言了 `lines_read` / `parsed_lines`，把硬闸**自己的**
    /// 判据（`unparsable_lines`）漏掉了 —— 而那正是复核实测"行数闸对翻窗重读永不触发"时看的量。
    #[tokio::test]
    async fn malformed_lines_are_skipped_but_still_counted() {
        let t = TempDir::new("logs-malformed");
        t.write(
            "mupc.log.2025-09-09",
            &format!(
                "not json at all\n{}\n{}\n{}\n",
                json_line(&iso_of(BASE_MS), "FATAL", "m", "unknown level"),
                r#"{"level":"INFO","target":"m","fields":{"message":"no ts"}}"#,
                json_line(&iso_of(BASE_MS), "INFO", "m", "good"),
            ),
        );
        let (page, stats) = svc(&t).page_with_stats(&q_default(), BASE_MS + 600_000).await.unwrap();
        assert_eq!(page.entries.len(), 1, "只有一行可表达");
        assert_eq!(page.entries[0].message, "good");
        assert_eq!(stats.lines_read, 4, "读入的行都计数");
        assert_eq!(stats.parsed_lines, 1, "只有 1 行能解析成契约条目");
        assert_eq!(
            stats.unparsable_lines, 3,
            "三行（非 JSON / 未知级别 / 无时间戳）都必须计入硬代价闸的行数判据：{stats:?}"
        );
        assert!(
            stats.unparsable_lines <= SCAN_READ_BUDGET_LINES,
            "远未到硬闸阈值 ⇒ 不得判超限：{stats:?}"
        );
        assert!(!page.range_too_large, "{stats:?}");
    }

    /// 极简 `Subscriber`：**只数 WARN 事件**（R4 的观测点）。
    ///
    /// 不引第三方捕获设施、不改 `Cargo.toml`：`tracing` 的 `Subscriber` 面很小，
    /// 这里只需要 `event()` 里的一个计数。
    #[derive(Clone, Default)]
    struct WarnCounter(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl WarnCounter {
        fn warns(&self) -> usize {
            self.0.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl tracing::Subscriber for WarnCounter {
        fn enabled(&self, _m: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _s: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _s: &tracing::span::Id, _v: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _s: &tracing::span::Id, _f: &tracing::span::Id) {}
        fn event(&self, e: &tracing::Event<'_>) {
            if *e.metadata().level() == tracing::Level::WARN {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }
        fn enter(&self, _s: &tracing::span::Id) {}
        fn exit(&self, _s: &tracing::span::Id) {}
    }

    /// **R4 可观测性**：整体解析失败（日志格式漂移）时必须**响亮告警**。
    ///
    /// 若不喊，屏上显示 EDGE-08「当前筛选条件下无日志」，而事实是"一行都读不懂"
    /// —— 两态**不可区分**且无从排障。注意：告警**只进 `tracing`，不上屏**（机器细节不上屏）。
    #[tokio::test]
    async fn a_fully_unparsable_log_file_is_warned_loudly() {
        let t = TempDir::new("logs-blind");
        // 模拟"JSON 层被换成纯文本"：全是行，但一行都不是契约形状
        t.write("mupc.log.2025-09-09", "plain text line 1\nplain text line 2\n");

        let counter = WarnCounter::default();
        let _guard = tracing::subscriber::set_default(counter.clone());
        let (page, stats) = svc(&t).page_with_stats(&q_default(), BASE_MS).await.unwrap();

        // 屏上确实是"无日志"（契约没有"格式漂移"位，这一点改不了）……
        assert!(page.entries.is_empty() && !page.range_too_large);
        // ……但**运维侧**必须能看见：读到了行、一行都没解析出来、且真的喊了
        assert!(stats.lines_read > 0, "{stats:?}");
        assert_eq!(stats.parsed_lines, 0, "告警条件 lines_read>0 ∧ parsed_lines==0 必须成立：{stats:?}");
        assert!(
            counter.warns() >= 1,
            "日志格式整体漂移必须 WARN（否则屏上「无日志」无从排障）"
        );

        // 正对照：**正常**日志文件不得触发该告警（否则告警恒亮 = 没人看）
        let t2 = TempDir::new("logs-blind-control");
        t2.write(
            "mupc.log.2025-09-09",
            &format!("{}\n", json_line(&iso_of(BASE_MS), "INFO", "m", "ok")),
        );
        let counter2 = WarnCounter::default();
        let _g2 = tracing::subscriber::set_default(counter2.clone());
        let _ = svc(&t2).page_with_stats(&q_default(), BASE_MS).await.unwrap();
        assert_eq!(counter2.warns(), 0, "能解析的日志不得触发'格式漂移'告警");
    }

    // ── ⑥ `/logs/targets` ─────────────────────────────────────────────────

    /// 去重 + 字典序 + 上限 50（超出截断，**不 panic**）。
    #[tokio::test]
    async fn targets_are_deduped_sorted_and_capped_at_contract_max() {
        let t = TempDir::new("logs-targets");
        let mut body = String::new();
        // 60 个不同 target + 大量重复
        for i in 0..60 {
            body.push_str(&json_line(&iso_of(BASE_MS + i as u64), "INFO", &format!("t{i:02}"), "m"));
            body.push('\n');
            body.push_str(&json_line(&iso_of(BASE_MS + i as u64), "INFO", &format!("t{i:02}"), "m"));
            body.push('\n');
        }
        t.write("mupc.log.2025-09-09", &body);
        let out = svc(&t).targets().await.unwrap();
        assert_eq!(out.len(), LOG_TARGETS_MAX, "必须截到契约上限");
        assert_eq!(out[0], "t00");
        // ⚠️ 整改三：改前这里是 `out[59.min(out.len() - 1)]` 与 `format!(… out.len() - 1)` ——
        // **自引用 `out.len()`** ⇒ 截断一旦坏掉（60 项全返回）它仍然自洽通过（弱断言）。
        // 现按**该用例的真实语义**钉死下标：字典序排完 t00..t59 截到第 50 个 ⇒ 末项 = `t49`。
        assert_eq!(
            out[LOG_TARGETS_MAX - 1], "t49",
            "截断后末项必须是字典序第 {LOG_TARGETS_MAX} 项（t49），不是 t59"
        );
        assert_eq!(out[49], "t49", "同上（字面下标版，防止 LOG_TARGETS_MAX 被改动时失去判别力）");
        let mut sorted = out.clone();
        sorted.sort();
        assert_eq!(out, sorted, "必须字典序升序");
        // 无重复
        let uniq: BTreeSet<_> = out.iter().collect();
        assert_eq!(uniq.len(), out.len());
    }

    /// **R5**：`/logs/targets` 的扫描预算必须走**配置**（`self.limits`），与 `/logs` 一致。
    ///
    /// 整改前这里硬用契约常量 [`LOG_SCAN_MAX_FILES`] / [`LOG_SCAN_MAX_LINES`] ⇒ 运维把
    /// `display.log.max_files/max_lines` 调小后，两个端点的扫描预算**不一致**。
    #[tokio::test]
    async fn targets_use_the_configured_scan_budget_like_logs_does() {
        let t = TempDir::new("logs-targets-budget");
        t.write(
            "mupc.log.2025-09-08",
            &format!("{}\n", json_line(&iso_of(BASE_MS), "INFO", "old_only", "m")),
        );
        t.write(
            "mupc.log.2025-09-09",
            &format!("{}\n", json_line(&iso_of(BASE_MS), "INFO", "new_only", "m")),
        );

        let limits = LogLimits {
            max_files: 1, // 只准采样最新的 1 个文件
            ..LogLimits::default()
        };
        let s = LogService::new(t.path(), limits);
        assert_eq!(
            s.targets().await.unwrap(),
            vec!["new_only".to_string()],
            "必须按配置的 max_files 采样（整改前硬用契约常量 5 ⇒ 会多出 old_only）"
        );

        // 正对照：契约默认（= [`LOG_SCAN_MAX_FILES`]）下两个文件都进采样 ⇒ 上一条不是恒真
        assert_eq!(
            svc(&t).targets().await.unwrap(),
            vec!["new_only".to_string(), "old_only".to_string()]
        );
    }

    /// **C 组**：`/logs/targets` 对"**无换行的大文件**"与"**超长行**"必须**有界** ——
    /// 不 `Err`、不挂死、不整行物化，也**不得**把超长行的内容当成 target 返回。
    ///
    /// 机理（整改前**必然发生**）：本端点用 `BufReader::lines()` 整行物化 ⇒ 一个无换行的
    /// `mupc.log.x` 会被**一次读进内存**（`lines_read == 1` ⇒ `max_lines` 闸永不触发）；
    /// 而本端点是**公开 HTTP 端点**（渲染端只在**启动期**拉一次清单，`local-display/src/app.rs`；
    /// 500 ms 增量轮询的是 `/logs`）⇒ 可重复触发的现实来源是运维脚本 / 重连 / 进程重启后重拉。
    /// ⚠️ **复核实测订正**：此处原写"渲染端**周期性**拉本端点"，与实测不符，已改。
    /// 现在超长行**整行跳过** + `tracing::warn!`。
    ///
    /// **判别力（本用例在整改前是红的）**：夹具里那条超长行是一条**合法的 JSON 日志行**
    /// （只是 > 行长上限）⇒ 旧实现会把它解析出来、把 `huge_target` 进选项表（**且已付出整行
    /// 物化的内存**）；新实现**跳过整行** ⇒ 选项表里只有正常文件的 target。
    #[tokio::test]
    async fn targets_survive_a_newline_free_file_without_materializing_it() {
        let t = TempDir::new("logs-targets-newline-free");
        // ① 完全无换行的大文件（几 MiB 即可，别造 GB 级；旧实现会把它整份读成一行）
        t.write("mupc.log.2025-09-08", &"z".repeat(5 * 1024 * 1024));
        // ② 一条**合法但超长**的 JSON 行（> 行长上限），同样无结尾换行
        t.write(
            "mupc.log.2025-09-09",
            &json_line(
                &iso_of(BASE_MS),
                "INFO",
                "huge_target",
                &"z".repeat(TARGETS_LINE_MAX_BYTES * 2),
            ),
        );
        // ③ 最新的文件：正常一行 ⇒ 采样结果里必须**只有**它的 target
        t.write(
            "mupc.log.2025-09-10",
            &format!("{}\n", json_line(&iso_of(BASE_MS), "INFO", "mupc_gateway", "m")),
        );

        let out = with_watchdog("targets 无换行/超长行", svc(&t).targets()).await.expect("不得 Err");
        assert_eq!(
            out,
            vec!["mupc_gateway".to_string()],
            "超长行必须在**行长上限**处被跳过：既不得把它的内容当 target 返回，\
             也不得吞掉别的文件的采样（旧实现会回 [`mupc_gateway`, `huge_target`]）"
        );

        // 正对照：正常行必须被采样到（上一条不是"恒不采样"的恒真断言）
        let t2 = TempDir::new("logs-targets-control");
        t2.write(
            "mupc.log.2025-09-09",
            &format!("{}\n", json_line(&iso_of(BASE_MS), "INFO", "another", "m")),
        );
        assert_eq!(
            svc(&t2).targets().await.unwrap(),
            vec!["another".to_string()],
            "正对照：正常文件必须被采样到"
        );
    }

    /// 目录不存在 ⇒ `Err`（handler 落 503），**不是** 空选项（空选项会被屏上读成"没有模块"）。
    #[tokio::test]
    async fn targets_on_missing_dir_is_err_not_empty_options() {
        let t = TempDir::new("logs-targets-missing");
        let missing = t.path().join("nope");
        let s = LogService::new(&missing, LogLimits::default());
        assert!(s.targets().await.is_err());
    }

    // ── ⑦ 文件名日期（扫描窗口剪枝）──────────────────────────────────────

    #[test]
    fn file_day_window_parses_only_dated_names() {
        let p = PathBuf::from("/x/mupc.log.2025-09-09");
        let (s, e) = file_day_window(&p).expect("dated name");
        assert_eq!(s, 1_757_376_000_000, "2025-09-09T00:00:00Z");
        assert_eq!(e, s + 86_400_000 - 1);
        assert!(file_day_window(&PathBuf::from("/x/mupc.log")).is_none());
        assert!(file_day_window(&PathBuf::from("/x/other.log.2025-09-09")).is_none());
    }

    /// `list_log_files` 只认 `mupc.log*`（不得把 audit/ 之类扫进来），且升序。
    #[tokio::test]
    async fn list_log_files_is_prefix_filtered_and_sorted() {
        let t = TempDir::new("logs-list");
        t.write("mupc.log.2025-09-09", "");
        t.write("mupc.log.2025-09-10", "");
        t.write("other.log.2025-09-09", "");
        std::fs::create_dir(t.join("archive")).unwrap();
        let s = svc(&t);
        let files = s.list_log_files().await.unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["mupc.log.2025-09-09", "mupc.log.2025-09-10"]);
    }

    // ── ⑧ 上屏用字的码表网（R9，与 `console_host` 的回执网同款）────────────

    /// 生成字体（`lv_font_cmap`）的 cmap 派生清单（**入库真源**，见 `fonts/gen_fonts.sh`）。
    ///
    /// 用 `include_str!`（**编译期**、按源文件相对路径、与 CWD 无关）⇒ 天然**无第二真源**；
    /// 文件不在（如交叉编译的纯净树）则**跳过**本网，而不是拿一个"假清单"过关。
    fn font_cmap() -> Option<std::collections::BTreeSet<char>> {
        let src = include_str!("../../local-display/fonts/lv_font_cmap.txt");
        let set: std::collections::BTreeSet<char> = src
            .lines()
            .filter_map(|l| l.strip_prefix("U+"))
            .filter_map(|h| u32::from_str_radix(h.trim(), 16).ok())
            .filter_map(char::from_u32)
            .collect();
        if set.is_empty() {
            return None;
        }
        Some(set)
    }

    /// **R9**：本单元**唯一**会中屏的字符串常量 —— [`TRUNCATION_MARKER`]（经 `LogEntry.message`
    /// 直达 P3 日志页消息列）—— 必须逐字符落在生成字体的 cmap 内，否则真机上是**豆腐块**。
    ///
    /// # 哪些 H 的字符串**不进**这张网（故意）
    ///
    /// - `"打开日志文件 … 失败"` / `"读日志目录 … 失败"` 等：**只进 400/503 响应体与 `tracing`**。
    ///   渲染端 `ConsoleClient` **不解析** GET 的错误体（`Error::HttpStatus`，§3.4 补注）⇒ **不上屏**；
    ///   把它们拉进网只会逼着人把排障文案写残（R9 明确禁止）。
    /// - `LogEntry.message` / `LogEntry.target` 的**正文**来自日志行本身（自由文本，不受码表约束
    ///   —— 这是渲染端既有的「自由文本不受码表约束」口径，见 PD24）。
    #[test]
    fn truncation_marker_is_inside_the_generated_font_cmap() {
        let Some(cmap) = font_cmap() else {
            panic!("cmap 清单解析为空 ⇒ 本网已失效（检查 fonts/lv_font_cmap.txt 是否被改坏）");
        };
        // 反例探针：这两个字符**确实不在** cmap 内 ⇒ 证明本网认得出缺字（不是恒真断言）
        for poison in ['…', '汉'] {
            assert!(
                !cmap.contains(&poison),
                "探针字符 `{poison}` 被判成「cmap 内」⇒ 本网认不出缺字，是恒真断言"
            );
        }
        let missing: Vec<char> = TRUNCATION_MARKER
            .chars()
            .filter(|c| !cmap.contains(c))
            .collect();
        assert!(
            missing.is_empty(),
            "截断标记 {TRUNCATION_MARKER:?} 含 cmap 外字符 {missing:?} ⇒ 真机豆腐块"
        );
        // 走一遍真正的上屏通路：截断后的消息（正文 + 标记）里，**标记那部分**必须在 cmap 内
        let long = "A".repeat(MESSAGE_MAX_BYTES * 2); // `A` 在 cmap 内（U+0041）
        let cut = truncate_message(&long);
        assert!(cut.ends_with(TRUNCATION_MARKER));
        let missing: Vec<char> = cut
            .chars()
            .filter(|c| !cmap.contains(c))
            .collect();
        assert!(missing.is_empty(), "截断后的上屏串含 cmap 外字符 {missing:?}");
    }

    /// 契约常量与配置默认值必须同源（跨 crate 的"两份抄写"由本用例钉死）。
    #[test]
    fn configured_limits_default_to_the_contract_constants() {
        let d = LogLimits::default();
        assert_eq!(d.max_files, LOG_SCAN_MAX_FILES);
        assert_eq!(d.max_lines, LOG_SCAN_MAX_LINES);
        assert_eq!(d.live_ring, mupc_display_proto::log::LOG_RING_CAPACITY);
        assert_eq!(d.page_limit_max, LOG_PAGE_LIMIT_MAX);
    }
}
