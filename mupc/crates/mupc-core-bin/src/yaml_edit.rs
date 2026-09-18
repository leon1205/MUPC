//! **保留式 yaml 编辑**（设计 §4.3.2.1「评审必改 #2」）——开发单元 **G-2** 的写路径核心。
//!
//! # 为什么不是"把 `CoreConfig` 序列化回去"
//!
//! 现场 `mupc_core_config.yaml` 里有人写的注释、空行、键顺序、以及 `CoreConfig` **未建模**的
//! 段/键（运维手写的 `legacy_top:` 一类；单元 K 起，**现场 legacy `web_api:` 段也属这类**——
//! 该字段随 `web-api` crate 删除已从模型里退出，见 `core_config.rs` 的订正说明）。若整棵树
//! 序列化回写，这些字节**全部消失**（已建模段则"内容不丢、格式被重排"）——而本模块的
//! 兼容性主张（§7.3「现场既有 yaml 仍可正常加载、不强制运维立即改文件」）正是靠"文件不会
//! 被装置改写"支撑的。故正常路径**只替换目标标量所在的那一行**，其余字节**逐字不动**。
//!
//! # ⚠️ 已登记的能力边界：**不支持"段/键缺失时追加标量行"**（评审阻塞 3）
//!
//! 本算法只做**行级替换**（`locate_scalar` 找不到段/键即 `Err`），**不建段、不追加键**。
//! 后果（现场可观测）：部署 yaml **缺某段**时，对该段字段的**首次**写入必然落到
//! `SectionMissing` ⇒ 上层回退**整体回写**（`WriteMode::FullRewrite`）⇒ **该文件全部注释
//! 与未建模段丢失**。这正是评审阻塞 3 的实证：`deploy/config/*.yaml` 原先**都没有 `gateway:` 段**，
//! 而"设 IEC-104 监听地址"是**投运必做动作**（L2+ 强确认）⇒ 第一次设就会抹掉现场注释。
//! **处置**：① 给两份 deploy yaml 补 `gateway:` 段（开箱配置完整，本单元已做）；
//! ② **支持"追加/建段"登记为后续单元或 PM 裁定项**（实现要点：建段要决定插入位置、
//! 缩进风格与行尾风格，且必须与"段内已有同名键"的判重语义对齐——不在本单元授权范围内）。
//! ③ 回退**不静默**：`WriteMode::FullRewrite` 在回执 / `ConfigView` / 审计里可见（EDGE-23）。
//!
//! # 语义完备性的**硬前提**（设计 §4.3.2.1「可行性前提」）
//!
//! 本期 F9 的可写字段**全部是标量叶子**（字符串 IP / `u16` / `u64` / 枚举字面量）——**无列表、
//! 无嵌套对象、无多行标量**。行级替换因此完备无歧义。若将来给字段表加列表 / 嵌套字段，
//! **必须**先扩展本算法（或退回整体回写路径），否则会把"改一个值"变成"改坏结构"：
//! [`apply_edits`] 对非标量值（`|` / `>` 块标量、`key:` 后为空即嵌套对象）**响亮报错**，
//! 交给上层的回退路径（`config_service`）处理，**不猜**。
//!
//! # 与"不可定位"的分工
//!
//! 本模块**只回答"能不能行级替换"**（能 ⇒ 返回新文本；不能 ⇒ 返回**具体原因**）。
//! "不能定位 ⇒ 整体序列化回写 + `WriteMode::FullRewrite` + 显式声明"是**上层**的裁决
//! （设计 §4.3.2.1 回退路径：**禁止静默回退**，所以降级必须由上层可见地记录）。

use serde_json::Value;

/// 一次标量行替换请求。
#[derive(Debug, Clone)]
pub struct ScalarEdit {
    /// 顶层段名（如 `intercore`）。
    pub section: String,
    /// 段内键名（如 `port`）。
    pub leaf: String,
    /// 新值（JSON 形态；由字段表的 `kind` 校验后传入）。
    pub value: Value,
}

impl ScalarEdit {
    /// 由 `ConfigFieldMeta.yaml_path`（`"section.leaf"`）构造。
    ///
    /// `yaml_path` 与字段 `key` 同形（G-1 单测 `yaml_path_matches_key_for_every_field` 钉死），
    /// 故 `key` 即定位依据——**不另起一套路径串**（否则一个真源变两个）。
    pub fn from_key(key: &str, value: Value) -> Option<Self> {
        let (section, leaf) = key.split_once('.')?;
        Some(Self {
            section: section.to_string(),
            leaf: leaf.to_string(),
            value,
        })
    }

    /// 定位串（错误消息与审计用）。
    pub fn path(&self) -> String {
        format!("{}.{}", self.section, self.leaf)
    }
}

/// 行级替换失败（⇒ 上层走整体回写回退路径，并在回执 / 审计里声明注释与未建模键丢失）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// 顶层段不存在（该段整体缺省 ⇒ 段内键无行可改）。
    SectionMissing { section: String },
    /// 段内键不存在（该键缺省 ⇒ 无行可改）。
    KeyMissing { path: String },
    /// 键存在但**值不是标量**（`key:` 后为空 = 嵌套对象；或块标量 `|` / `>`）——行级替换
    /// 语义在此不完备，必须交回整体回写（设计 §4.3.2.1「值非标量」）。
    NotScalar { path: String, found: String },
    /// **同一段内同名键出现多次**（yaml 允许但语义是"后者胜"）——行级替换会改错行，
    /// 故**拒绝**而不是赌一把。
    DuplicateKey { path: String },
    /// 段头出现多次（同一顶层段重复定义）。
    DuplicateSection { section: String },
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SectionMissing { section } => write!(f, "yaml 中不存在顶层段 `{section}:`"),
            Self::KeyMissing { path } => write!(f, "yaml 中不存在键 `{path}`（无行可替换）"),
            Self::NotScalar { path, found } => {
                write!(f, "键 `{path}` 的值不是标量（`{found}`）⇒ 行级替换语义不适用")
            }
            Self::DuplicateKey { path } => write!(f, "键 `{path}` 在段内重复出现 ⇒ 定位有歧义"),
            Self::DuplicateSection { section } => {
                write!(f, "顶层段 `{section}:` 重复出现 ⇒ 定位有歧义")
            }
        }
    }
}

impl std::error::Error for EditError {}

/// **行级替换**（保留式编辑的唯一入口）。
///
/// 返回**新文本**；除每个目标键所在的**那一行**外，其余字节与 `src` 逐字相同（含行尾风格：
/// CRLF 仍 CRLF、LF 仍 LF；含行内注释、缩进、空行、键顺序、未建模段）。
///
/// 任一条 `edits` 不可定位 ⇒ 整次编辑**失败**（不产生"改了一半"的文本）：调用方要么整体
/// 放弃、要么走整体回写回退。**这是刻意的**——部分替换的文件 + 半生效的内存副本会构成
/// "装置处于两个配置之间"的状态（EDGE-10 明禁半生效）。
pub fn apply_edits(src: &str, edits: &[ScalarEdit]) -> Result<String, EditError> {
    let mut lines: Vec<String> = src.split_inclusive('\n').map(str::to_string).collect();
    for e in edits {
        let idx = locate_scalar(&lines, e)?;
        let old = body_of(&lines[idx]);
        let new_body = replace_scalar_value(old, &e.value);
        // 只替换行体，行尾（`\r\n` / `\n`）原样保留。
        let terminator = &lines[idx][old.len()..];
        lines[idx] = format!("{new_body}{terminator}");
    }
    Ok(lines.concat())
}

/// 定位目标标量行（返回行下标）。
fn locate_scalar(lines: &[String], e: &ScalarEdit) -> Result<usize, EditError> {
    // ① 段：缩进 0 且行体（去尾空白）恰为 `section:`。
    let mut section_start: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        let b = body_of(l);
        if indent_of(b) != 0 {
            continue;
        }
        let t = b.trim_end();
        if t == format!("{}:", e.section) {
            if section_start.is_some() {
                return Err(EditError::DuplicateSection {
                    section: e.section.clone(),
                });
            }
            section_start = Some(i);
        }
    }
    let start = section_start.ok_or_else(|| EditError::SectionMissing {
        section: e.section.clone(),
    })?;

    // ② 段块 = 段头之后，到下一个"缩进 0 的映射键行"之前（缩进 0 的注释 / 空行不结束块）。
    let end = (start + 1..lines.len())
        .find(|&j| {
            let b = body_of(&lines[j]);
            indent_of(b) == 0 && is_top_level_key(b)
        })
        .unwrap_or(lines.len());

    // ③ 段内键：缩进 > 0 且 `leaf:` 前是完整键名。
    let mut found: Option<usize> = None;
    for (j, line) in lines.iter().enumerate().take(end).skip(start + 1) {
        let b = body_of(line);
        let ind = indent_of(b);
        if ind == 0 {
            continue;
        }
        let Some(rest) = b.get(ind..) else { continue };
        if rest.trim_start().starts_with('#') {
            continue;
        }
        let key_part = rest.split(':').next().unwrap_or("").trim_end();
        if key_part != e.leaf {
            continue;
        }
        if found.is_some() {
            return Err(EditError::DuplicateKey { path: e.path() });
        }
        found = Some(j);
    }
    let idx = found.ok_or_else(|| EditError::KeyMissing { path: e.path() })?;

    // ④ 值必须是标量（`key:` 后为空 = 嵌套对象；`|` / `>` = 块标量 ⇒ 行级替换不完备）。
    let b = body_of(&lines[idx]);
    let rest = &b[b.find(':').map(|p| p + 1).unwrap_or(b.len())..];
    let after_ws = rest.trim_start();
    if after_ws.is_empty() || after_ws.starts_with('#') {
        return Err(EditError::NotScalar {
            path: e.path(),
            found: "空值（嵌套对象 / null）".to_string(),
        });
    }
    if after_ws.starts_with('|') || after_ws.starts_with('>') {
        return Err(EditError::NotScalar {
            path: e.path(),
            found: format!("块标量 `{}`", &after_ws[..1]),
        });
    }
    Ok(idx)
}

/// 行体（去行尾 `\n` 与 `\r`；两处 `unwrap_or` 都是"没有就原样"，不 panic、不吞字符）。
fn body_of(line: &str) -> &str {
    let t = line.strip_suffix('\n').unwrap_or(line);
    t.strip_suffix('\r').unwrap_or(t)
}

/// 缩进（前导空格数；含制表符的行按"非法缩进"处理 = 不可定位，不猜）。
fn indent_of(body: &str) -> usize {
    body.len() - body.trim_start_matches(' ').len()
}

/// 是否为"缩进 0 的映射键行"（段块结束判据；注释与空行不算）。
fn is_top_level_key(body: &str) -> bool {
    let t = body.trim_end();
    !t.is_empty() && !t.starts_with('#') && t.contains(':')
}

/// 替换 `body` 的**值记号**，返回新行体。
///
/// 保真的部分：键名、冒号、值前空格、值后空白、行内注释（`# ...`）、缩进——**逐字保留**。
/// 只换"值"本身：字面风格尽量继承旧的（原来带引号就仍带引号）。
fn replace_scalar_value(body: &str, value: &Value) -> String {
    let colon = match body.find(':') {
        Some(p) => p,
        // 定位阶段已保证 `key:` 存在 ⇒ 不可达；返回原样而不是 panic。
        None => return body.to_string(),
    };
    let after = &body[colon + 1..];
    let lead_ws_len = after.len() - after.trim_start_matches(' ').len();
    let value_start = colon + 1 + lead_ws_len;
    let tail = &body[value_start..];
    let value_len = scalar_token_len(tail);
    let old_token = &tail[..value_len];
    let suffix = &tail[value_len..];
    let style = QuoteStyle::of(old_token);
    format!(
        "{}{}{}",
        &body[..value_start],
        scalar_text(value, style),
        suffix
    )
}

/// 值记号的字节长度（到"行内注释"或"行尾空白"之前）。
fn scalar_token_len(tail: &str) -> usize {
    let bytes = tail.as_bytes();
    let mut end = tail.len();
    for i in 0..bytes.len() {
        // YAML 注释：`#` 且前面是空白（或处于值起始处）。
        if bytes[i] == b'#' && (i == 0 || bytes[i - 1] == b' ' || bytes[i - 1] == b'\t') {
            end = i;
            break;
        }
    }
    let token = &tail[..end];
    token.trim_end_matches([' ', '\t']).len()
}

/// 旧值的引号风格（尽量继承，避免"只是改了个数"却让整行风格突变）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStyle {
    /// 无引号（裸标量）。
    Plain,
    /// 单引号。
    Single,
    /// 双引号。
    Double,
}

impl QuoteStyle {
    /// 由旧值记号推断风格。
    pub fn of(token: &str) -> Self {
        if token.len() >= 2 && token.starts_with('"') && token.ends_with('"') {
            Self::Double
        } else if token.len() >= 2 && token.starts_with('\'') && token.ends_with('\'') {
            Self::Single
        } else {
            Self::Plain
        }
    }
}

/// JSON 值 → yaml 标量文本（**不产生多行**：本模块的硬前提是无多行标量）。
pub fn scalar_text(value: &Value, style: QuoteStyle) -> String {
    match value {
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::String(s) => quote(s, style),
        // 定位前的字段校验（`ConfigKind::validate_value`）只放行字符串与非负整数 ⇒ 不可达；
        // 万一可达，也输出**单行合法 yaml**（JSON 是 YAML 子集）而不是 panic / 多行。
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".to_string()),
    }
}

/// 字符串标量加引号（能裸写就裸写；旧值带引号则**保持同种引号**）。
fn quote(s: &str, style: QuoteStyle) -> String {
    match style {
        // 旧值裸写：值也裸写安全就裸写，否则加双引号（不制造歧义）
        QuoteStyle::Plain => {
            if plain_safe(s) {
                s.to_string()
            } else {
                double_quoted(s)
            }
        }
        // 旧值单引号：单引号内只有 `'` 需要转义（用 `''`）；含 `'` 时退回双引号
        QuoteStyle::Single => {
            if s.contains('\'') {
                double_quoted(s)
            } else {
                format!("'{s}'")
            }
        }
        // 旧值双引号：保持双引号
        QuoteStyle::Double => double_quoted(s),
    }
}

/// 是否可作**裸标量**（无引号、无歧义）：首字符为字母/数字/`.`，其余为 `[A-Za-z0-9._-]`。
///
/// 本期字段值域（IPv4 字符串、`info`/`debug` 一类枚举字面量、整数）全部满足 ⇒ 正常路径
/// **不会**改变原有的引号风格。含 `:` / `#` / 空格 / 中文等的一律加引号（防造出坏 yaml）。
fn plain_safe(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '.' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// 双引号标量（`serde_json` 的转义规则是 YAML 双引号的子集 ⇒ 直接复用，不另写一份转义）。
fn double_quoted(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 现场样例：**含注释 + 含 legacy `web_api:` 段 + 含未建模键 + 非字母序**（设计 §4.3.2.1
    /// 往返单测 1 的输入形态）。CRLF / LF 两版共用同一份内容，缩进为 2 空格。
    ///
    /// ⚠️ 单元 K 订正（原为「评审重要 5」的相反结论）：`web_api:` **曾经**是已建模段
    /// （`CoreConfig` 有 `pub web_api: WebApiConfig`），随 `web-api` crate 删除已退为**未建模段**。
    /// 对本算法**没有影响**：保留式编辑一律逐字保留非目标行，建模与否都不进判据。差异只在
    /// **整体回写**回退路径下——那时未建模段**整段消失**（`WriteMode::FullRewrite` 可见，
    /// EDGE-23 已登记）。
    fn field_sample(eol: &str) -> String {
        let lf: &str = r#"# MUPC 主配置（现场手工维护，注释务必保留）
version: "0.1.0"
# ── 系统 ──
system:
  log_level: info        # 现场调过：默认 info
  log_dir: /var/log/mupc
intercore:
  host: 10.0.0.7
  port: 9100   # PCS 端口
  heartbeat_interval_sec: 5
  reconnect_interval_sec: 3
  transport: tcp
south_stations:            # 不在本模块字段表 FIELDS 内（保留式编辑下逐字保留）
  poll_ms: 1000
  stations: []
web_api:                   # 现场 legacy 段（单元 K 后 CoreConfig 已无此字段）：保留式编辑下逐字保留
  listen_addr: 0.0.0.0:8080
gateway:
  listen_addr: 0.0.0.0
  listen_port: 2404
  future_key: keep-me      # 未建模键（不是任何 ConfigFieldMeta）
plugins:
  auto_load: []            # 空列表也要原样
"#;
        if eol == "\n" {
            lf.to_string()
        } else {
            lf.replace('\n', "\r\n")
        }
    }

    fn edit(key: &str, v: Value) -> ScalarEdit {
        ScalarEdit::from_key(key, v).expect("key 形如 section.leaf")
    }

    /// **主用例（设计 §4.3.2.1 往返单测 1）**：只改 `intercore.port` 一个键。
    ///
    /// 断言方式 = **与手写字面量逐字节相等**（最强判据：注释 / 顺序 / 未建模段 / 行尾全在内），
    /// 而不是"只比几个字段"——后者漏掉任何一处字节变化都不会红。
    ///
    /// **改什么会让本条变红**：任何让非目标行发生变化的实现（例如"解析→序列化"回写）都会
    /// 立刻红（注释、`web_api:` 段、`future_key` 全部消失）。
    #[test]
    fn preserve_edit_changes_exactly_one_scalar_line() {
        let src = field_sample("\n");
        let out = apply_edits(&src, &[edit("intercore.port", json!(2405))]).unwrap();
        let want = src.replace("  port: 9100   # PCS 端口", "  port: 2405   # PCS 端口");
        assert_eq!(out, want, "除目标标量外**逐字节**必须不变（行内注释与空格原样保留）");
        // 负对照：确认期望串确实与源不同（否则本用例是恒真）
        assert_ne!(want, src);
        // 其余关键字节**逐条**复核（不靠"整体相等"一条兜着）
        // 注：期望串随样例注释的**事实订正**同步更新（单元 K：`web_api:` 已从"已建模段"退为
        // legacy 段）。断言强度不变——仍是**逐字节**包含该行（含对齐空格与注释全文）。
        assert!(out.contains(
            "web_api:                   # 现场 legacy 段（单元 K 后 CoreConfig 已无此字段）：保留式编辑下逐字保留"
        ));
        assert!(out.contains("  future_key: keep-me"));
        assert!(out.contains("  log_level: info        # 现场调过：默认 info"));
        assert!(out.contains("# MUPC 主配置（现场手工维护，注释务必保留）"));
    }

    /// CRLF 文件的行尾**不得**被规范化（现场是 Windows 编辑过的 yaml 时，保存一次不能把整份
    /// 文件改成 LF——那是"改一个端口号却让 git 显示全文件重写"）。
    #[test]
    fn crlf_line_endings_are_preserved() {
        let src = field_sample("\r\n");
        let out = apply_edits(&src, &[edit("intercore.port", json!(2405))]).unwrap();
        assert_eq!(out, src.replace("port: 9100", "port: 2405"));
        assert_eq!(out.matches("\r\n").count(), src.matches("\r\n").count());
        assert_eq!(out.matches('\n').count(), src.matches('\n').count());
    }

    /// **多键批量**（设计 §4.3.2.1 往返单测 2）：N 个键 ⇒ 恰有 N 行被替换。
    #[test]
    fn batch_edits_replace_exactly_n_lines() {
        let src = field_sample("\n");
        let out = apply_edits(
            &src,
            &[
                edit("system.log_level", json!("debug")),
                edit("intercore.host", json!("192.168.3.21")),
                edit("gateway.listen_port", json!(2405)),
                edit("intercore.heartbeat_interval_sec", json!(9)),
            ],
        )
        .unwrap();
        // 逐行比对：只有 4 行不同，且都是目标行
        let (a, b): (Vec<&str>, Vec<&str>) = (src.lines().collect(), out.lines().collect());
        assert_eq!(a.len(), b.len());
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        assert_eq!(diff.len(), 4, "恰有 4 行被替换，实得 {diff:?}");
        // 逐行核对（按文件出现顺序，不是按 edits 顺序）
        assert_eq!(b[diff[0]], "  log_level: debug        # 现场调过：默认 info");
        assert_eq!(b[diff[1]], "  host: 192.168.3.21");
        assert_eq!(b[diff[2]], "  heartbeat_interval_sec: 9");
        assert_eq!(b[diff[3]], "  listen_port: 2405");
    }

    /// 引号风格**继承**：原来带引号的值改完仍带引号（不制造"改一个值顺带改了风格"的噪声）。
    #[test]
    fn quote_style_is_inherited_from_the_old_token() {
        let src = "version: \"0.1.0\"\nsystem:\n  log_level: \"info\"\n";
        let out = apply_edits(src, &[edit("system.log_level", json!("warn"))]).unwrap();
        assert_eq!(out, "version: \"0.1.0\"\nsystem:\n  log_level: \"warn\"\n");
    }

    /// **不可定位**：段缺失 / 键缺失 ⇒ 响亮报错（上层据此走整体回写回退）。
    #[test]
    fn missing_section_or_key_is_loud_not_silent() {
        let src = field_sample("\n");
        // 段缺失
        assert_eq!(
            apply_edits(src.as_str(), &[edit("telemetry.report_interval_sec", json!(5))]),
            Err(EditError::SectionMissing {
                section: "telemetry".into()
            })
        );
        // 段在、键缺失
        assert_eq!(
            apply_edits(src.as_str(), &[edit("intercore.reconnect_interval_s", json!(5))]),
            Err(EditError::KeyMissing {
                path: "intercore.reconnect_interval_s".into()
            })
        );
        // 键名必须是**完整**匹配：`port` 不得命中 `ports`
        let src2 = "intercore:\n  ports: 9100\n";
        assert!(matches!(
            apply_edits(src2, &[edit("intercore.port", json!(1))]),
            Err(EditError::KeyMissing { .. })
        ));
    }

    /// **非标量**：`key:` 后为空（嵌套对象）或块标量 ⇒ 拒绝（回退路径的触发条件之一）。
    #[test]
    fn non_scalar_values_are_refused() {
        let nested = "gateway:\n  listen_addr:\n    host: 0.0.0.0\n";
        assert!(matches!(
            apply_edits(nested, &[edit("gateway.listen_addr", json!("0.0.0.0"))]),
            Err(EditError::NotScalar { .. })
        ));
        let block = "system:\n  log_level: |\n    info\n";
        assert!(matches!(
            apply_edits(block, &[edit("system.log_level", json!("debug"))]),
            Err(EditError::NotScalar { .. })
        ));
    }

    /// 段内同名键重复 ⇒ 定位有歧义 ⇒ 拒绝（**不赌**"后者胜"）。
    #[test]
    fn duplicate_key_is_refused_instead_of_guessed() {
        let src = "intercore:\n  port: 1\n  port: 2\n";
        assert_eq!(
            apply_edits(src, &[edit("intercore.port", json!(3))]),
            Err(EditError::DuplicateKey {
                path: "intercore.port".into()
            })
        );
    }

    /// 段块边界：下一个顶层键结束本段 ⇒ **不得**越段改到别的段里的同名键。
    #[test]
    fn section_block_does_not_leak_into_the_next_section() {
        let src = "intercore:\n  host: 10.0.0.7\ngateway:\n  host: 0.0.0.0\n";
        let out = apply_edits(src, &[edit("intercore.host", json!("10.0.0.8"))]).unwrap();
        assert_eq!(out, "intercore:\n  host: 10.0.0.8\ngateway:\n  host: 0.0.0.0\n");
    }

    /// 顶层注释**不**结束段块（现场常见"段内小标题注释"写法）。
    #[test]
    fn top_level_comment_does_not_end_a_section_block() {
        let src = "intercore:\n  host: 10.0.0.7\n# 下面是端口\n  port: 9100\n";
        let out = apply_edits(src, &[edit("intercore.port", json!(9101))]).unwrap();
        assert_eq!(out, "intercore:\n  host: 10.0.0.7\n# 下面是端口\n  port: 9101\n");
    }

    /// 标量文本生成：裸写安全值不加引号；含特殊字符的值加引号（防造坏 yaml）。
    #[test]
    fn scalar_text_never_produces_a_multi_line_or_ambiguous_scalar() {
        assert_eq!(scalar_text(&json!(2404), QuoteStyle::Plain), "2404");
        assert_eq!(scalar_text(&json!("192.168.3.10"), QuoteStyle::Plain), "192.168.3.10");
        assert_eq!(scalar_text(&json!("info"), QuoteStyle::Plain), "info");
        assert_eq!(scalar_text(&json!("0.0.0.0"), QuoteStyle::Plain), "0.0.0.0");
        // 含 `#` / 空格 / `:` ⇒ 必须加引号（否则解析出注释或坏结构）
        for bad in ["a #b", "a b", "a: b", ""] {
            let t = scalar_text(&json!(bad), QuoteStyle::Plain);
            assert!(t.starts_with('"') && t.ends_with('"'), "`{bad}` 应被引号包裹，实得 {t}");
            assert!(!t.contains('\n'));
        }
        // 单引号继承且值内无 `'` ⇒ 用单引号
        assert_eq!(scalar_text(&json!("a b"), QuoteStyle::Single), "'a b'");
        assert_eq!(scalar_text(&json!("it's"), QuoteStyle::Single), "\"it's\"");
    }

    /// 行内注释与"值后空白"严格分离：`port: 9100   # x` 改值后空格与注释一字不差。
    #[test]
    fn inline_comment_and_trailing_spaces_survive() {
        let src = "intercore:\n  port: 9100   # PCS 端口\n";
        let out = apply_edits(src, &[edit("intercore.port", json!(12))]).unwrap();
        assert_eq!(out, "intercore:\n  port: 12   # PCS 端口\n");
    }

    /// `#` **不是**注释（前面无空白，如值里带 `#`）：此时整段到行尾都是值 ⇒ 也被替换掉，
    /// 不会把值切成两半（`scalar_token_len` 的判据是 YAML 注释规则）。
    #[test]
    fn hash_without_leading_space_is_not_a_comment() {
        let src = "intercore:\n  host: a#b\n";
        let out = apply_edits(src, &[edit("intercore.host", json!("10.0.0.1"))]).unwrap();
        assert_eq!(out, "intercore:\n  host: 10.0.0.1\n");
    }

    /// `from_key` 只认 `section.leaf` 两段（多段 / 无点 ⇒ `None`，不猜）。
    #[test]
    fn from_key_requires_exactly_two_segments() {
        assert!(ScalarEdit::from_key("system.log_level", json!("info")).is_some());
        assert!(ScalarEdit::from_key("nope", json!(1)).is_none());
    }
}
