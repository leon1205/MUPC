//! 离屏**进程级**自检（工作单元 B3-2a；设计 §9「HMI 一键自检」）。
//!
//! ## ⚠️ 文件命名偏差（B3-2b-1 登记；**主控裁定：保留现名**）
//!
//! 设计 **§12.3 的文件表**把本文件写作 **`tests/ui_offscreen.rs`**（标【重写】），而 B3-2a 实际
//! 落地的文件名是 **`tests/offscreen_smoke.rs`**。**内容与职责一致**（进程级集成自检：
//! 起真 bin → 六页走查 → 退出码 / PPM / 告警），只是**名字与设计表不同** ——
//! 主控裁定**保留现名**（改名会同时动设计文件表与所有引用点，收益不成比例；
//! `ui_offscreen` 这个名字还容易让人误以为它是 `ui/**` 的单元级用例，而它其实是**进程级**）。
//! **照此执行**：后续新增设计 / 评审若引用 §12.3，请按 `offscreen_smoke.rs` 检索。
//!
//! # 为什么是"起真进程"而不是"调库函数"
//!
//! 本文件断言的是**进程装配**本身：`main.rs` 的 CLI → LVGL init → display/indev 注册 →
//! 六页外壳装配 → **单一 `poll` 阻塞点的事件循环** → 优雅退出。这条链路的每一节都在 bin 里
//! （bin 不进 lib、集成测试够不着 `App` 的内部状态）⇒ 唯一能覆盖它的形态就是**把 bin 跑起来**
//! （`CARGO_BIN_EXE_mupc-local-display` 由 cargo 注入）。
//!
//! 它同时是本单元最关键的一条防退化网：**进程能起来 + 六页都真的画出了像素 + 返回码 0**。
//! 「屏空白但测试全绿」（持有型 LVGL 句柄被留在局部变量 ⇒ 函数返回即级联删除）在本项目
//! 复发过一次 —— 本文件用**真进程的 stdout 统计**把它钉住。
//!
//! # 桩服务端（读通道）
//!
//! 用一个本机 `TcpListener` 回 200 + 合法 v2 帧（`Content-Length` + `Content-Type`），
//! 让进程真的走一遍「GET → 解析 → `DisplayState` → 页面 render」。**同步 std 实现**
//! （crate 已无 tokio 依赖）。
//!
//! ## 桩判据史：`misses == 0` 为何**曾经偶发红**（本轮修复）
//!
//! **现象**：`cargo test -p local-display -j 2` 下本文件
//! `assert_eq!(misses.load(..), 0, "不应请求错误路径")` **约 1/8 概率**变红。
//!
//! **根因（结构性推导，非实测复现）**：旧桩对每条连接**只做一次 `sock.read`**，再拿整串与
//! `"GET /v1/display/latest "` 做前缀比 ⇒ 两类**根本没有请求任何路径**的连接被记进 `misses`：
//! ① **短读**：一条请求跨 TCP 段到达 ⇒ 只读到前半段 ⇒ 前缀比不中；
//! ② **空连接**：`read` 返 `Ok(0)` ⇒ 与 `""` 比 ⇒ 不中。
//! **空连接的真实来源**在客户端（`src/channel.rs`，两条**已登记的设计偏差**）：连接交一次性
//! 工作线程跑 `connect_timeout`（拿到 socket 即退出），以及关停路径**作废在途 GET**
//! （`Pending::cancel`，**不发任何应用层字节**）⇒ "连上但未写"的 socket 会被桩 accept 到。
//! **旁证**：客户端**自己**的测试桩（`src/channel.rs::read_request`）早已明写「**读到请求头结束**
//! （GET 无 body），避免与服务端 read 相互等待」—— 即本仓已知"单次 `read` 不是正确的 HTTP
//! 服务端"，只是本文件的桩没照做（判据与文案不符，与 T21c-3-r2 修掉的 W-2′ 同族）。
//!
//! **能力边界（如实声明）**：PM 侧 **26 次尝试未复现**（18 次单跑 `--test offscreen_smoke` 与
//! 8 次全量）⇒ 上述根因是**推导**，本文件**没有**原偶发的实测复现记录；故本修复**不声称**
//! 「偶发已 100% 归因」，它做的是两件可验证的事：
//!
//! 1. **判据与文案对齐** —— `misses` 语义收紧为「**收到完整请求行、但它不是契约路径**」，
//!    而"短读 / 空连接"由**独立计数 `aborted`** 单列（照旧打印，**不掩盖**）；
//! 2. **把不可复现的偶发变成可复现的回归** —— 桩的判定抽成纯函数
//!    [`classify_request`]，并用**确定性**用例（分段写入 / 空连接 / 真错路径 / 请求行外干扰）
//!    钉住两类判别（见文件末 `mod tests`）。实测：把读循环改回"单次 read"⇒ 分段用例**必红**。
//!
//! 时钟/环境：本用例全平台可跑（Windows 本机无 evdev/fb0 —— 用
//! `--backend offscreen` + `--channel` 指向桩，正是不变量"不许把 poll(2)/evdev 做成唯一路径"
//! 的可执行证据）。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// 帧 JSON（**字面量**，不用 display-proto 序列化 —— 避免"同源同错"掩盖契约偏差）。
/// **v3 契约（§15.2.1）**：`version` 必须 = `PROTO_VERSION`（=3）；本样例不带 `peripherals`
/// 段 ⇒ 走 `serde(default)` 显式降级（`available=false`、不补 0，§15.2.3）。
fn frame_json(seq: u64) -> String {
    format!(
        r#"{{"version":3,"seq":{seq},"ts_ms":{},"soc":65.0,"soc_source":"pcs_reg1010",
        "soc_flag":"valid","run_state":2,"pcs_online":true,
        "p_phase":[{{"v":12.3,"flag":"valid"}},{{"v":11.8,"flag":"valid"}},{{"v":12.0,"flag":"valid"}}],
        "p_total":{{"v":36.1,"flag":"valid"}},
        "i_phase":[{{"v":22.5,"flag":"valid"}},{{"v":22.1,"flag":"valid"}},{{"v":22.3,"flag":"valid"}}],
        "inconsistency":false}}"#,
        now_epoch_ms()
    )
}

/// 当前 Unix 毫秒（桩按现算 `ts_ms` 组帧，模拟 mupcd 的 1 Hz 发布）。
fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ═══════════════════════════════════════════════════════════════════════════
// 桩：按**完整请求行**判定（判据史见模块头）
// ═══════════════════════════════════════════════════════════════════════════

/// 契约路径（与传给 bin 的 `--channel` 末尾同源）。
const CONTRACT_PATH: &str = "/v1/display/latest";
/// 单次 `read` 的字节上限。
const READ_CHUNK: usize = 1024;
/// 请求头**总上限**：读到就收手（HTTP 请求头远小于此；**有界 = 防挂死 / 防无限增长**）。
const MAX_HEAD_BYTES: usize = 8 * 1024;

/// 一条连接收到的请求所属类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReqClass {
    /// 完整请求行 + 契约路径 ⇒ 回 200 + 帧。
    Contract,
    /// **完整请求行**、但不是契约路径 ⇒ 回 404、记 `misses`（**这条才是"请求了错误路径"**）。
    OtherPath,
    /// **没有完整请求行**（0 字节空连接 / 未见到 `\r\n` 就 EOF）⇒ **没有请求任何路径**：
    /// 记 `aborted`、**不记 `misses`**。
    Aborted,
}

/// **纯函数**：按**完整的第一行**（到第一个 `\r\n` 为止）判定请求类别。
///
/// 用"完整第一行"而不是"整串前缀比"，正是为了让两类**非请求**情形不再被误判成
/// "请求了错误路径"：
/// - **短读 / 分段**：字节没到齐 ⇒ 取不出完整第一行 ⇒ [`ReqClass::Aborted`]（不是 `misses`）；
/// - **请求行之后还跟着别的内容**（额外头 / 尾巴）：第一行已完整 ⇒ 判定不受后续字节影响。
fn classify_request(head: &[u8]) -> ReqClass {
    // 含 `head.is_empty()`：一次都没读到 ⇒ 空连接（对端连上就收手）。
    let Some(crlf) = head.windows(2).position(|w| w == b"\r\n") else {
        return ReqClass::Aborted;
    };
    let line = String::from_utf8_lossy(&head[..crlf]);
    let mut parts = line.split(' ');
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method == "GET" && target == CONTRACT_PATH {
        ReqClass::Contract
    } else {
        ReqClass::OtherPath
    }
}

/// **有界**读到请求头结束（`\r\n\r\n`）或对端收手（EOF / 读错）为止。
///
/// 单次读上限 [`READ_CHUNK`]、总上限 [`MAX_HEAD_BYTES`] ⇒ 既不会与"只写了一半的对端"
/// 相互等待（客户端测试桩 `channel.rs::read_request` 的同款做法），也不会被无界对端撑爆。
fn read_request_head(sock: &mut TcpStream) -> Vec<u8> {
    let mut head = Vec::new();
    let mut chunk = [0u8; READ_CHUNK];
    while head.len() < MAX_HEAD_BYTES {
        match sock.read(&mut chunk) {
            Ok(0) => break, // EOF：对端收手（含"连上就关"的空连接）
            Ok(n) => {
                head.extend_from_slice(&chunk[..n]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") {
                    break; // 请求头结束（GET 无 body）
                }
            }
            Err(_) => break,
        }
    }
    head
}

/// 桩的三类计数（`Arc` 以便用例线程读；三量**必须分列** —— 合成一个计数就会重现旧误判）。
#[derive(Clone, Default)]
struct StubStats {
    /// 命中契约路径的请求数。
    hits: Arc<AtomicUsize>,
    /// **完整请求行**但非契约路径 —— 真"请求了错误路径"。
    misses: Arc<AtomicUsize>,
    /// 无完整请求行的连接（空连接 / 截断）。**单列，不计入 `misses`**。
    aborted: Arc<AtomicUsize>,
}

impl StubStats {
    fn new() -> Self {
        Self::default()
    }

    /// `(hits, misses, aborted)`（便于断言与打印）。
    fn counts(&self) -> (usize, usize, usize) {
        (
            self.hits.load(Ordering::SeqCst),
            self.misses.load(Ordering::SeqCst),
            self.aborted.load(Ordering::SeqCst),
        )
    }
}

/// 服务**一条**连接：有界读到请求头结束 → 判定 → 应答 / 记数。
///
/// 三类行为的差别就是本修复的核心：**只有 [`ReqClass::OtherPath`] 才记 `misses`**。
fn serve_one_connection(mut sock: TcpStream, stats: &StubStats) {
    let head = read_request_head(&mut sock);
    let resp = match classify_request(&head) {
        ReqClass::Contract => {
            let seq = stats.hits.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            let body = frame_json(seq);
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        }
        ReqClass::OtherPath => {
            stats.misses.fetch_add(1, Ordering::SeqCst);
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
        }
        ReqClass::Aborted => {
            stats.aborted.fetch_add(1, Ordering::SeqCst);
            // **打印而不吞**：便于将来排查偶发（尤其真机/CI 上"连上未写"的出现频率）。
            eprintln!(
                "[stub] 连接未给出完整请求行（读到 {} 字节）⇒ 记 aborted、不计 misses",
                head.len()
            );
            // 没有请求就没有响应可回（对端多半已 FIN；即便还开着，也不该回一个"应答"）。
            return;
        }
    };
    let _ = sock.write_all(resp.as_bytes());
}

/// 最小 HTTP 桩：对 `GET /v1/display/latest` 回 200 + 帧 JSON；**完整请求行的其它路径** 404。
///
/// 返回 `(url, 三类计数)`；桩线程随进程结束（`detach`）——用例不关心它的收尾。
fn spawn_frame_stub() -> (String, StubStats) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
    let addr = listener.local_addr().expect("local_addr");
    let stats = StubStats::new();
    let s = stats.clone();
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            let Ok(sock) = sock else { break };
            let s = s.clone();
            std::thread::spawn(move || serve_one_connection(sock, &s));
        }
    });
    (format!("http://{addr}{CONTRACT_PATH}"), stats)
}

/// 从 `[smoke] page=P1 active_px=1234` 里取第 `n` 页的像素数（缺行 ⇒ `None`）。
fn page_pixels(stdout: &str, n: usize) -> Option<usize> {
    let key = format!("[smoke] page=P{n} active_px=");
    stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix(&key))
        .and_then(|v| v.trim().parse::<usize>().ok())
}

/// 从 `[smoke]` 时序行里取 `key=value`（如 `renders=3` / `dropped=0`）；缺行或畸形 ⇒ `None`。
fn smoke_stat(stdout: &str, key: &str) -> Option<u64> {
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with("[smoke] ticks="))?;
    line.split_whitespace()
        .filter_map(|f| f.split_once('='))
        .find(|(k, _)| *k == key)
        .and_then(|(_, v)| v.parse::<u64>().ok())
}

/// bin 路径（cargo 注入；无需手工拼 target 目录）。
fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mupc-local-display"))
}

/// 自检导出目录：用 `target/` 下的**固定子目录**（避免引临时目录依赖；每次覆盖写）。
fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("smoke");
    std::fs::create_dir_all(&dir).expect("create smoke out dir");
    dir
}

// ═══════════════════════════════════════════════════════════════════════════
// 主用例：一键自检
// ═══════════════════════════════════════════════════════════════════════════

/// `--backend offscreen --channel <stub> --smoke --smoke-out <ppm>`：
/// **能起、能渲染六页、能按节拍拉帧、能停、返回码 0**，且导出的 PPM 尺寸正确。
///
/// **改什么会让本条变红**（逐条实测过，见交付报告「破坏性探针清单」）：
/// - 把 `App` 的 `Display`/`Indev`/`Shell` 任一持有型句柄改成局部变量 ⇒ 屏空白 ⇒
///   `active_px=0` ⇒ 第三段断言红；
/// - 事件循环里删掉 `Host::tick` 的通道推进（或把 `poll_due` 恒假）⇒ 帧数恒 0 ⇒ 第二段红；
/// - `smoke()` 里不切页（去掉 `shell.show`）⇒ 六页统计趋同/为空 ⇒ 第三段红。
#[test]
fn offscreen_smoke_renders_six_pages_and_exits_zero() {
    let (url, stats) = spawn_frame_stub();
    let ppm = out_dir().join("smoke.ppm");
    let _ = std::fs::remove_file(&ppm);

    let out = Command::new(bin())
        .args([
            "--backend",
            "offscreen",
            "--channel",
            &url,
            "--width",
            "1024",
            "--height",
            "768",
            "--smoke",
            "--smoke-out",
        ])
        .arg(&ppm)
        .output()
        .expect("spawn mupc-local-display");

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "自检应返回 0；实得 {:?}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        out.status.code()
    );

    // ① 时序数字（设计 §9「打印时序」）
    assert!(
        stdout.contains("[smoke] ticks="),
        "应打印时序行：\n{stdout}"
    );
    assert!(stdout.contains("[smoke] result=OK"), "自检结论行缺失：\n{stdout}");

    // ② 真的按节拍从读通道取到了帧（端点契约：只打 /v1/display/latest）
    let ticks = stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix("[smoke] ticks="))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    assert!(ticks >= 10, "事件循环应跑满 --smoke 的拍数，实得 {ticks}\n{stdout}");
    let (hits, misses, aborted) = stats.counts();
    assert!(hits >= 1, "读通道一次都没被命中：\n{stderr}");
    // **判据未放宽**（仍是 `== 0`）：`misses` 现在只统计「**收到完整请求行**但不是契约路径」
    // ⇒ 它恢复了自己文案的语义。短读 / 空连接由 `aborted` 单列（不进这条断言、但打印出来）。
    assert_eq!(
        misses, 0,
        "不应请求错误路径（misses = 完整请求行且非契约路径；本次 hits={hits} aborted={aborted}）：\n{stderr}"
    );
    // **不掩盖**：三类计数全打出来（`--nocapture` 可见；将来偶发时先看 aborted 是不是在涨）。
    eprintln!(
        "[stub] hits={hits} misses={misses} aborted={aborted}（aborted = 连上但未给出完整请求行，**不计** misses）"
    );
    assert!(
        !stdout.contains("frames_ok=0 "),
        "自检期间应至少成功取到一帧：\n{stdout}"
    );

    // ②′ **渲染节流真的生效**（B3-2a 质量评审 重要 I-2：判据 `needs_render` 本身有单测，
    //    但**调用点**此前零覆盖 —— 把接线退化成恒真时全部用例仍绿）。
    //    语义键（通道态/新鲜度/帧序号）不变就不该调页面 `render`（标脏 ⇒ 整屏重绘）。
    //    `--smoke` 只跑 20 拍、而 `--poll-ms` 节拍是 500 ms ⇒ 实测渲染次数应远小于拍数；
    //    恒真退化 ⇒ 两者相等 ⇒ 本断言红。
    let renders = smoke_stat(&stdout, "renders").unwrap_or_else(|| panic!("缺 renders= 字段：\n{stdout}"));
    assert!(
        renders < ticks,
        "帧驱动页渲染次数 {renders} 不少于拍数 {ticks} —— 节流没生效（每拍都标脏 = 永远整屏重绘）\n{stdout}"
    );

    // ②″ **没有整拍被静默跳过**（I-4：`dropped` 是"该画的像素没画上"的唯一观测哨；
    //     此前 `Blitter` 被 move 进 flush 闭包 ⇒ 该计数在生产与自检路径都读不到）。
    let dropped = smoke_stat(&stdout, "dropped").unwrap_or_else(|| panic!("缺 dropped= 字段：\n{stdout}"));
    assert_eq!(
        dropped, 0,
        "自检期间有 {dropped} 个脏区被丢弃（目标被借走 ⇒ 屏上留永久陈旧像素而 LVGL 不知道）\n{stdout}"
    );
    // ②‴ 对偶哨：**必须真的记到过搬运**。少了这条，`dropped == 0` 可能只是"计数没接线"
    //     （游离句柄恒 0）—— 两条合起来才既证明"读口接上了"、又证明"没丢帧"。
    let blits = smoke_stat(&stdout, "blits").unwrap_or_else(|| panic!("缺 blits= 字段：\n{stdout}"));
    assert!(
        blits > 0,
        "自检走查期间一次 flush 都没记到（blits=0）—— 计数句柄没接到真闭包上\n{stdout}"
    );

    // ③ **六页都非空壳**（内容区真的有像素；白屏/级联删除在此现形）
    let mut counts = Vec::new();
    for n in 1..=6 {
        let px = page_pixels(&stdout, n)
            .unwrap_or_else(|| panic!("缺第 {n} 页统计行：\n{stdout}"));
        counts.push(px);
    }
    for (i, px) in counts.iter().enumerate() {
        assert!(
            *px > 0,
            "第 {} 页内容区为空（active_px=0）—— 屏空白但测试全绿的典型形态；\
             实测六页 = {counts:?}\n{stdout}",
            i + 1
        );
    }
    // **反坍缩**：「六页都非空」若退化成"同一页被统计了六遍"，上面每条 `> 0` 都会通过 ——
    // 那是恒真断言。这里要求六个数里至少有 4 个互不相同（实测六页两两不同：P1 528800 /
    // P2 71664 / P3 89606 / P4 524189 / P5 136346 / P6 587308）。**不写死具体数值**：
    // 页面外观会随 UI 迭代变化，写死就会变成"改样式就红"的脆断言（那是伪门禁的另一种）。
    let distinct: std::collections::BTreeSet<_> = counts.iter().copied().collect();
    assert!(
        distinct.len() >= 4,
        "六页统计过于雷同（{counts:?}）—— 很可能根本没切页（每次画的是同一页）\n{stdout}"
    );

    // ③′ **退出统计行带上通道诊断三量**（B3-2a 质量评审 I-5 裁定：`addr` / 单次超时 /
    //     连续失败数接进 `report_exit` 让真机排障可读，而不是删掉这三个读口）。
    //     `channel_addr` 必须是**解析后**的目标地址（与发起连接同一个 `SocketAddr`）。
    let port = url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default();
    assert!(
        stderr.contains(&format!("channel_addr={port}")),
        "退出行应回显解析后的通道地址 `channel_addr={port}`：\n{stderr}"
    );
    assert!(
        stderr.contains("channel_timeout_ms=2000"),
        "退出行应回显单次 GET 超时（默认 2 s）：\n{stderr}"
    );
    assert!(
        stderr.contains("fail_streak="),
        "退出行应回显连续失败数：\n{stderr}"
    );

    // ④ PPM 导出：字节数与「头 + w*h*3」逐字节一致（尺寸对 = 屏面就是 1024×768）
    assert!(
        stdout.contains(&format!("[smoke] export={} bytes=", ppm.display())),
        "应打印导出行：\n{stdout}"
    );
    let meta = std::fs::metadata(&ppm).expect("导出的 PPM 应存在");
    assert_eq!(
        meta.len(),
        mupc_local_display::app::ppm_bytes(1024, 768),
        "PPM 字节数应为 头 + 1024*768*3"
    );
    let bytes = std::fs::read(&ppm).expect("read ppm");
    assert_eq!(&bytes[..2], b"P6", "PPM 魔数");
    let head = String::from_utf8_lossy(&bytes[..64.min(bytes.len())]);
    assert!(head.contains("1024 768"), "PPM 头应声明 1024x768：{head}");
}

// ═══════════════════════════════════════════════════════════════════════════
// 自检必须**能失败**（否则它只是"打印"）
// ═══════════════════════════════════════════════════════════════════════════

/// `--smoke-out` 单给（无 `--smoke`）⇒ **参数错误**（退出码 2），不静默忽略该路径。
///
/// 这条守的是"解析了但无人消费的静默 no-op"（本项目最忌讳的一类缺陷）。
#[test]
fn smoke_out_without_smoke_is_a_usage_error() {
    let out = Command::new(bin())
        .args(["--backend", "offscreen", "--smoke-out", "x.ppm"])
        .output()
        .expect("spawn");
    assert_eq!(
        out.status.code(),
        Some(2),
        "应为用法错误退出码 2；stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 非法参数（轮询节拍超红线）⇒ 退出码 2，且**不**起事件循环（不静默取默认值）。
#[test]
fn poll_ms_above_red_line_is_rejected() {
    let out = Command::new(bin())
        .args(["--backend", "offscreen", "--poll-ms", "600"])
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--poll-ms") || err.contains("轮询"), "错误须点名参数：{err}");
}

/// `--help` / `--version` 早退（退出码 0，不装配 LVGL）。
#[test]
fn help_and_version_exit_zero_without_starting_the_loop() {
    for flag in ["--help", "--version"] {
        let out = Command::new(bin())
            .arg(flag)
            .output()
            .expect("spawn");
        assert_eq!(out.status.code(), Some(0), "{flag} 应以 0 退出");
        assert!(!out.stdout.is_empty(), "{flag} 应有输出");
    }
}

/// 「接了就断」的哑端点：**端口由本进程持有**（不可能被抢），但每个连接收到即关闭
/// ⇒ 读通道永远拿不到合法响应。
///
/// 旧版取"刚被释放的端口"赌它没被别的进程抢（B3-2a 质量评审 建议 I-3：理论上有竞态窗口）；
/// 本版把端口握在自己手里，且**不必**让子进程等满 2 s 超时（连接立刻被 FIN ⇒ 失败立刻可见）。
fn spawn_dead_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind dead endpoint");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            drop(sock); // 收下即断：对端读到 EOF，拿不到任何响应
        }
    });
    format!("http://{addr}/v1/display/latest")
}

/// **不动手**也能证明"自检有失败路径"：`--smoke` 在无帧来源（通道对端接了就断）时
/// 仍然**渲染六页**（页面降级为「通道未连接 / 数据未取数」——那**也是**非空渲染），
/// 故返回码仍为 0；本用例守住的是"通道不通 ≠ 屏空白"（EDGE-13 的只读展示语义）。
#[test]
fn smoke_without_channel_still_renders_pages() {
    let url = spawn_dead_endpoint();
    let out = Command::new(bin())
        .args(["--backend", "offscreen", "--channel", &url, "--smoke"])
        .output()
        .expect("spawn");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "通道不通时仍应完成自检并返回 0（降级展示）：\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for n in 1..=6 {
        let px = page_pixels(&stdout, n).unwrap_or(0);
        assert!(px > 0, "通道不通时第 {n} 页也应非空壳（降级态本身要上屏）：\n{stdout}");
    }
}

/// **启动期参数可见性**：`--font`（v2.0 已废弃，字库改由构建期绑定）与
/// `--control-channel`（**B3-2b-2 已接线**）。
///
/// # ⚠️ 本用例的沿革（**改名 + 断言订正**；主控裁定 2）
///
/// 本用例**原名** `deprecated_params_warn_loudly_and_do_not_abort`，锁的是「`--control-channel`
/// **待接线**」的告警：它硬断言启动行明说该参数**当前不生效**、`ConsoleClient` 将在 B3-2b
/// 接线。B3-2b-2 把 `ConsoleClient` 真接进 `app.rs` 之后，**同一行的语义已变** —— 它现在是
/// 「**生效回显**」（参数此刻真的影响行为），旧断言若留着，就是要求日志里必须写一句**已经
/// 失真**的话（§2.6）。故：**改名**（见下）+ 断言**随之更新**为「回显实际取值 + 如实标注已接线」，
/// 且**不再**要求（或引用）任何已失真的字样。
///
/// 名称取 `startup_warns_deprecated_font_and_echoes_live_control_channel`：两个参数的性质
/// 已经**分叉**（`--font` 仍是"已废弃的告警"，`--control-channel` 已是"生效回显"），原名把它们
/// 统称 `deprecated_params` 已不准确。
///
/// **沿革的沿革（原"禁止旧告警复活"那条禁令去哪了）**：它**不在本用例**了 —— 同一禁令由
/// `tests/control_channel.rs::startup_echoes_the_live_control_channel_and_never_claims_it_is_inert`
/// 承担（本单元新增的进程级用例，判据同款）；旧告警函数本身已连同
/// `config.rs::control_channel_pending_warning` 整体删除（见该文件的沿革段）。
///
/// 三重锁定（③ 的第 5 段按上文订正后为 3 段）：
/// ① 两条启动行都真的打到 stderr（不是只在 help 里写写 —— 生产路径必须真调用文案函数）；
/// ② **退出码 0**：`--font` 是告警而非硬错误（本进程由 systemd `Restart=always` 托管，
///    硬错误 = 起不来 + 无限重启）；
/// ③ 启动行里带**实际取值**（现场能据此判断"我这条 unit 是不是要改"）+ `--control-channel`
///    如实标注**已接线**。
///
/// **改什么会让本条变红**：删掉 `main.rs::run_process` 里任一条 `eprintln!`（或把
/// `--font` 改成 `ExitCode::from(EXIT_USAGE)` 硬错误）⇒ ① / ② 立刻红；
/// 把控制通道启动行改回"待接线"口径（`config.rs::control_channel_notice` 不再回显已接线）
/// ⇒ ③ 的第 3 段红。
#[test]
fn startup_warns_deprecated_font_and_echoes_live_control_channel() {
    let out = Command::new(bin())
        .args([
            "--backend",
            "offscreen",
            "--smoke",
            "--font",
            "/nope/x.otf",
            "--control-channel",
            "http://127.0.0.1:9811",
        ])
        .output()
        .expect("spawn");
    let err = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "两个参数都只是告警、进程须照常跑完（退出码 0）；实得 {:?}\n{err}",
        out.status.code()
    );
    assert!(err.contains("--font /nope/x.otf"), "字体告警缺失或未回显取值：\n{err}");
    assert!(err.contains("不生效"), "字体告警须说明不生效：\n{err}");
    assert!(
        err.contains("--control-channel http://127.0.0.1:9811"),
        "控制通道启动行缺失或未回显取值：\n{err}"
    );
    assert!(
        err.contains("B3-2b-2 已接线"),
        "控制通道启动行须如实标注已接线（B3-2b-2 之后「待接线」已失真）：\n{err}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 桩判据的**确定性**用例
//
// 为什么需要：原 `misses == 0` 偶发红**不可复现**（26 次尝试未复现，见模块头）⇒ 只能把
// "短读 / 空连接 / 真错路径"三条判别抽成纯函数 + 起真 socket 的确定性用例，才能把
// **不可复现的偶发**变成**可复现的回归**。本模块即"改前必红"的钉。
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Shutdown, SocketAddr};
    use std::time::Duration;

    /// 单连接桩：accept **一条**、**内联**服务（不起额外线程）后退出 ⇒ `join()` 返回时计数已终局
    /// （**无 sleep、无轮询** ⇒ 确定性；不同于 [`spawn_frame_stub`] 的 `detach` 形态）。
    fn one_shot_stub() -> (SocketAddr, StubStats, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind one-shot stub");
        let addr = listener.local_addr().expect("addr");
        let stats = StubStats::new();
        let s = stats.clone();
        let handle = std::thread::spawn(move || {
            if let Ok((sock, _)) = listener.accept() {
                serve_one_connection(sock, &s);
            }
        });
        (addr, stats, handle)
    }

    /// 客户端：连上 → 按序**分段**写（可选段间间隔）→ 关写端 → 读完响应。
    ///
    /// 关写端是关键：它让服务端的读循环**确定性地**看到 EOF（而不是靠超时猜）。写/读的
    /// 错误一律**故意忽略** —— 本模块断言的是**桩的计数**，不是客户端能不能写完
    /// （旧单读实现在第一段后就回 404 并关连接，第二段写入可能失败，这正是"改前必红"的形态）。
    fn drive(addr: SocketAddr, chunks: &[&[u8]], gap: Option<Duration>) -> String {
        let mut sock = TcpStream::connect(addr).expect("connect one-shot stub");
        for (i, c) in chunks.iter().enumerate() {
            if i > 0 {
                if let Some(g) = gap {
                    std::thread::sleep(g);
                }
            }
            let _ = sock.write_all(c);
        }
        let _ = sock.shutdown(Shutdown::Write);
        let mut resp = Vec::new();
        let _ = sock.read_to_end(&mut resp);
        String::from_utf8_lossy(&resp).into_owned()
    }

    /// **① 分段（短读）仍判命中** —— 本修复"有牙"的正面证据。
    ///
    /// 一条完整请求分两次写（段间 250 ms ⇒ **第一段必然被单独读到**，不靠内核是否合并 TCP 段）。
    /// 旧实现（单次 `read` + 整串前缀比）在这里**必红**（只读 `GET /v1/displ` ⇒ 记 `misses`）。
    #[test]
    fn split_reads_of_one_request_still_count_as_a_hit() {
        let (addr, stats, handle) = one_shot_stub();
        let resp = drive(
            addr,
            &[b"GET /v1/displ", b"ay/latest HTTP/1.1\r\n\r\n"],
            Some(Duration::from_millis(250)),
        );
        handle.join().expect("one-shot stub thread");
        assert_eq!(
            stats.counts(),
            (1, 0, 0),
            "分段到达的同一条请求应判**命中**（旧单读实现会记 misses）"
        );
        assert!(resp.starts_with("HTTP/1.1 200"), "应回 200：{resp:?}");
    }

    /// **② 空连接不记 `misses`**（它没请求任何路径）⇒ 记 `aborted`。
    ///
    /// 空连接的真实来源是客户端已登记的两条设计偏差（`connect_timeout` 工作线程 / 作废在途 GET）。
    #[test]
    fn empty_connection_is_aborted_and_never_a_miss() {
        let (addr, stats, handle) = one_shot_stub();
        let resp = drive(addr, &[], None);
        handle.join().expect("one-shot stub thread");
        assert_eq!(
            stats.counts(),
            (0, 0, 1),
            "连上但一个字节没写 ⇒ aborted=1、misses=0（**不是**「请求了错误路径」）"
        );
        assert!(resp.is_empty(), "无请求 ⇒ 无响应：{resp:?}");
    }

    /// **③ 真错路径必须记 `misses`** —— 证明主用例的 `misses == 0` **仍有牙**。
    #[test]
    fn a_complete_request_line_for_another_path_is_a_miss() {
        let (addr, stats, handle) = one_shot_stub();
        let resp = drive(addr, &[b"GET /wrong HTTP/1.1\r\n\r\n"], None);
        handle.join().expect("one-shot stub thread");
        assert_eq!(
            stats.counts(),
            (0, 1, 0),
            "完整请求行且非契约路径 ⇒ 必须记 misses（否则断言被架空）"
        );
        assert!(resp.starts_with("HTTP/1.1 404"), "应回 404：{resp:?}");
    }

    /// **④ 请求行之外的干扰**（额外头 / 头部之后的尾巴）不影响命中判定。
    #[test]
    fn trailing_bytes_after_a_valid_request_line_do_not_break_the_hit() {
        let (addr, stats, handle) = one_shot_stub();
        let resp = drive(
            addr,
            &[b"GET /v1/display/latest HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Junk: aaa\r\n\r\nJUNK-AFTER-HEAD"],
            None,
        );
        handle.join().expect("one-shot stub thread");
        assert_eq!(stats.counts(), (1, 0, 0), "第一行完整即判命中：{resp:?}");
        assert!(resp.starts_with("HTTP/1.1 200"), "应回 200：{resp:?}");
    }

    /// **纯函数**层面的两条边界（不起 socket ⇒ 覆盖"读到一半 EOF"这种 socket 用例不好构造的形态）。
    #[test]
    fn classify_request_needs_a_complete_request_line() {
        // 没读到任何字节 / 没有完整第一行 ⇒ Aborted（**不是**"错误路径"）
        assert_eq!(classify_request(b""), ReqClass::Aborted);
        assert_eq!(classify_request(b"GET /v1/displ"), ReqClass::Aborted);
        assert_eq!(classify_request(b"GET /wrong"), ReqClass::Aborted);
        // 第一行完整就够了（后面的头/体不参与判定）
        assert_eq!(
            classify_request(b"GET /v1/display/latest HTTP/1.1\r\n"),
            ReqClass::Contract
        );
        assert_eq!(
            classify_request(b"GET /v1/display/latest HTTP/1.1\r\nHost: x\r\n\r\n"),
            ReqClass::Contract
        );
        // 完整请求行 + 别的路径 / 别的方法 ⇒ OtherPath（有牙）
        assert_eq!(
            classify_request(b"GET /v1/display/latestx HTTP/1.1\r\n\r\n"),
            ReqClass::OtherPath
        );
        assert_eq!(
            classify_request(b"POST /v1/display/latest HTTP/1.1\r\n\r\n"),
            ReqClass::OtherPath
        );
    }
}
