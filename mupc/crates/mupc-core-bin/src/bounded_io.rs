//! **有界按行读**（crate 内共用）—— 单元 I 从 `log_service.rs` **上收**而来。
//!
//! ⚠️ 搬迁措辞（代码质量评审点名，原写"代码逐字节不变"**不精确**）：
//! **搬迁本身未改代码体——已逐字节核对**；非代码体的改动只有两处：
//! ① 可见性从"`log_service.rs` 的私有项"改成 `pub(crate)`、模块路径从
//! `crate::log_service::…` 改成 `crate::bounded_io::…`；
//! ② **文档表随第三调用方更新**（新增 `ConsoleAuditService::page` 一行 —— 见下面的表）。
//!
//! 另有一处**本轮**的改动（如实登记）：按整改要求对本文件跑了一次 `rustfmt`，它把两处
//! `BoundedLine::Overlong { bytes: … }` 单行字面量重排成多行（`next_line` 的两处 `return`）——
//! **纯格式、语义逐字节等价**（`rustfmt` 的 `struct_lit_width` 阈值所致，与 `log_service.rs`
//! 里那份的写法因此不再逐字相同，但**行为**由本模块的两条 `\r` 用例 + H 的 45 条共同守着）。
//!
//! # 为什么上收成独立模块（高风险的搬迁，理由必须写死）
//!
//! 「无上限的整行物化」这个缺陷在单元 H 的 `log_service.rs` 里**同形出现过三处**
//! （`/logs/targets` 的 `BufReader::lines()`、正读 `read_forward`、倒读读窗），前两处最终都
//! 收敛到本实现，其文档原话是「两处共用，**不许再抄第三份**」。
//!
//! 单元 I（`ConsoleAuditService` 读审计 JSONL）是**第三个调用方**。两条路：
//! ① 就地再抄一份有界读 ⇒ 仓库里出现**第四份**同类代码（"两份抄写"正是这个坑的成因，
//!    下一次修一行漏一处）；② 上收为共用模块。
//! **选 ②**：搬迁是纯机械的（改动只有可见性、模块路径与文档表三处，见文件顶部的措辞订正），
//! 且 `log_service` 的既有用例就是现成的回归网
//! （搬迁前后 `cargo test -p mupc-core-bin log_service` 逐条同数同结果）。
//!
//! # 本模块**不含**任何策略
//!
//! 它只保证「单行物化 ≤ `line_max`、单次 `read` ≤ `chunk_bytes`」；**超长行怎么处置由调用方
//! 自定**（这正是三个调用方口径不同的地方，故不能塞进本模块）：
//!
//! | 调用方 | 块大小 | 行长上限 | 超长行的处置 |
//! |--------|--------|----------|--------------|
//! | `LogService::targets` | `TARGETS_READ_CHUNK_BYTES` | `TARGETS_LINE_MAX_BYTES` | 计数 + 事后一条 `warn!` |
//! | `read_forward` | `REVERSE_CHUNK_BYTES` | `MAX_REVERSE_WINDOW_BYTES` | 计入字节闸 `SCAN_READ_BUDGET_BYTES` + 逐条 `warn!` |
//! | `ConsoleAuditService::page` | `AUDIT_READ_CHUNK_BYTES` | `AUDIT_LINE_MAX_BYTES` | **整页不可用**（`available=false`）：审计是合规凭据，静默丢一条 = 谎报"无记录" |

/// [`BoundedLineReader::next_line`] 的一次产出。
pub(crate) enum BoundedLine<'a> {
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
/// 不触发**。现实可达：择向 `LogService::choose_direction` 在"文件头是可解析时间戳、窗口靠
/// 文件头"时选 `Forward`，此后文件中间夹一条百 MB~GB 级的行（大 payload 打进 `message`、
/// 或别的工具把二进制块写进 `mupc.log*`）就是一次与行长同阶的分配。本模块的设计前提就是
/// "日志目录可能被别的工具污染"（见 `log_service.rs` 模块头）⇒ 这不是臆想输入。
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
/// # 调用点（**三个调用方共用本实现，不许再抄一份**）
///
/// 三个调用方的**口径差异表只有模块头那**一份（见本文件顶部「本模块不含任何策略」）——
/// 这里**不再抄第二份**（本轮去重，评审点名：两张表各写一遍，改一张漏一张就是下次漂移的种子）。
///
/// 三处的**单行上限取值不同**是刻意的：`targets` 只做 target 采样（一行几百字节足够），
/// 正读则是**页路径**——它要尽量与倒读同口径，免得两个方向对同一份文件给出不同的条目集。
/// ⚠️ **复核实测订正**：原写"否则同一份文件会给出不同的条目集（同一个 1 MiB 的行，
/// 倒读能组装上屏、**正读却被跳过**）"—— **理由说反了**：恰 1 MiB 的行是**正读交付、
/// 倒读装不下**（倒读还要求行尾 `'\n'` 挤进窗口 ⇒ 可容行长比正读**少 1 字节**；行长 ≥ 上限时
/// 倒读是整页拒绝、正读只跳过那一条）。取同值的意义是**把差距压到最小**，**不是**两侧等价。
/// 真正的对齐口径与已知不对称见 `log_service.rs` 模块头「复核实测订正」。故正读沿用倒读的读窗上限
/// `MAX_REVERSE_WINDOW_BYTES` 作为行长口径。
///
/// 搬迁说明（单元 I）：本结构体的**代码体未改一行（已逐字节核对）**；改动只有
/// ① 从 `log_service.rs` 的**私有项**变成 `pub(crate)`（模块路径随之改为 `crate::bounded_io`）与
/// ② **文档表随第三调用方更新**（新增 `ConsoleAuditService::page` 一行）。
/// （另有一处**本轮** `rustfmt` 造成的纯格式重排，见模块头「搬迁措辞」。）
/// 回归网：`cargo test -p mupc-core-bin log_service` 搬迁前后 **45/45 逐条同结果**。
pub(crate) struct BoundedLineReader<'a> {
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
    pub(crate) fn new(f: &'a mut tokio::fs::File, chunk_bytes: usize, line_max: usize) -> Self {
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
    pub(crate) async fn next_line(&mut self) -> std::io::Result<Option<BoundedLine<'_>>> {
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
                        return Ok(Some(BoundedLine::Overlong {
                            bytes: self.take_skipped(),
                        }));
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
                    return Ok(Some(BoundedLine::Overlong {
                        bytes: self.take_skipped(),
                    }));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    /// 读一份**字节**语料（不是 JSON），把每行的**原始字节**逐条收出来。
    ///
    /// 为什么要在本模块**直接**测字节（而不是走 `ConsoleAuditService::page`）：`\r` 的两条分支
    /// 在审计侧**观测不到**——`serde_json` 把 `\r` 当空白字符，行尾留一个 `\r` 照样解析成功
    /// ⇒ 拿"页面可用 / 条目数"当断言，**去掉 `\r` 修剪分支也照样绿**（用例 ⑭ 的注释承认过
    /// 这个洞）。本模块的文档对这两条分支写了"**逐字**对齐 `lines()`"的强声明 ⇒ 只有在**字节层**
    /// 才钉得住。
    async fn read_lines(tag: &str, corpus: &[u8]) -> Vec<Result<Vec<u8>, u64>> {
        let t = TempDir::new(tag);
        let p = t.write("corpus", ""); // 建文件（下面直接以字节覆盖）
        std::fs::write(&p, corpus).unwrap();
        let mut f = tokio::fs::File::open(&p).await.unwrap();
        let mut r = BoundedLineReader::new(&mut f, 64, 1024);
        let mut out = Vec::new();
        while let Some(line) = r.next_line().await.unwrap() {
            out.push(match line {
                BoundedLine::Line(b) => Ok(b.to_vec()),
                BoundedLine::Overlong { bytes } => Err(bytes),
            });
        }
        out
    }

    /// **`\r` 分支之一**（评审点名的测试缺口）：`\r\n` 结尾 ⇒ 两个都去掉
    /// （`lines()` 的 `ends_with('\n')` 才 pop `'\r'` 那一半）。
    ///
    /// **破坏性验证**：把 `next_line` 里 `if self.line.last() == Some(&b'\r') { self.line.pop(); }`
    /// 整段摘掉 ⇒ 本条变红（拿到的是 `b"abc\r"`）。
    #[tokio::test]
    async fn crlf_line_endings_drop_the_carriage_return() {
        let got = read_lines("bio-crlf", b"abc\r\ndef\r\n").await;
        assert_eq!(
            got,
            vec![Ok(b"abc".to_vec()), Ok(b"def".to_vec())],
            "`\\r\\n` 结尾的行必须同时去掉 `\\r` 与 `\\n`（同 `lines()`）"
        );
        // 单独的 `\r`（**不在**行尾）必须原样保留 —— 修剪只针对紧邻 `\n` 的那一个。
        let got2 = read_lines("bio-cr-mid", b"a\rb\n").await;
        assert_eq!(
            got2,
            vec![Ok(b"a\rb".to_vec())],
            "行**中间**的 `\\r` 不得被吃掉（只修剪紧邻 `\\n` 的那个）"
        );
    }

    /// **`\r` 分支之二**：文件**末行无 `'\n'`** 且以 `'\r'` 结尾 ⇒ `'\r'` **保留**。
    ///
    /// 这条分支在 `eof` 里（不经行尾 `\n` 那条路），是最容易被"顺手也 pop 一下"改坏的一处。
    /// **破坏性验证**：在 `eof` 分支的 `if !self.line.is_empty()` 之前加一句同样的 pop ⇒ 本条变红。
    #[tokio::test]
    async fn a_trailing_carriage_return_survives_on_the_last_unterminated_line() {
        let got = read_lines("bio-cr-eof", b"abc\r").await;
        assert_eq!(
            got,
            vec![Ok(b"abc\r".to_vec())],
            "末行没有 `\\n` ⇒ 其末尾的 `\\r` **保留**（`lines()` 的实现就是这样：`ends_with('\\n')` 才 pop）"
        );
        // 对照：同样内容**有** `\n` 结尾 ⇒ `\r` 被去掉（两句合起来才说明修剪的判据是"有没有 `\n`"，
        // 而不是"文件到没到头"）。
        let got2 = read_lines("bio-cr-eof-nl", b"abc\r\n").await;
        assert_eq!(got2, vec![Ok(b"abc".to_vec())]);
    }
}
