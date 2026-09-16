//! **控制通道接线**的进程级集成用例（工作单元 **B3-2b-2**：控制通道接线 / T-3 闭环）。
//!
//! # 为什么必须"起真进程 + 本机 HTTP 桩"
//!
//! 本文件要证明的两件事，**判据都在进程的对外可观测面上**，进程内的库调用证明不了：
//!
//! 1. **T-3 门禁：未确认 = 零网络动作**。判据 = "桩服务端在整个自检里**只**收到 GET、
//!    **一包 POST 都没有**"。这条只能在**真进程**上观测 —— `ConsoleClient` 的生产实例化点
//!    在 `App::build` 里（`app.rs` 是 lib，但 `App` 的构造需要 LVGL 会话，进程内的
//!    `#[test]` 起不了第二条 LVGL 线程，见 `src/ui/tests.rs` 的模块头）。
//! 2. **P3 通道条态的驱动源是控制通道、不是帧通道**。判据 = 退出统计行的 `p3_channel=up|down`
//!    （`main.rs::report_exit`）。**两条通道的存亡可以相反**（EDGE-20），
//!    于是"控制通道活 + 帧通道死"这一组就是**判别性**的：若驱动源被换成帧通道，
//!    同一条用例会打印 `down` 而当场变红。
//!
//! 桩服务端 = 同步 `std::net::TcpListener`（crate 已无 tokio 依赖），与
//! `tests/offscreen_smoke.rs` 同法；**每个请求都记账**（方法 + 路径），用例据此断言。
//!
//! 时钟/环境：全平台可跑（Windows 本机无 evdev/fb0 ⇒ 一律 `--backend offscreen`）。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

/// bin 路径（cargo 注入；无需手工拼 target 目录）。
fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mupc-local-display"))
}

// ═══════════════════════════════════════════════════════════════════════════
// 桩：控制通道（§3.4 的 5 个 GET 端点；记账每一个请求的方法 + 路径）
// ═══════════════════════════════════════════════════════════════════════════

/// 一条被桩记录下来的请求（方法 + 路径，**不含**查询串 —— 用例只关心"是不是 POST"）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hit {
    method: String,
    path: String,
}

impl Hit {
    fn is_post(&self) -> bool {
        self.method == "POST"
    }
}

/// 控制通道桩：§3.4 的 5 个 GET 端点各回一份**合法最小 DTO**（`200` + `Content-Length`），
/// 其余路径 `404`；所有请求记入 `hits`。
///
/// **为什么每个 GET 都要回 200 且载荷合法**：`ConsoleClient` 把非 2xx 与解码失败
/// **一律计一次失败**（并累进 `fail_streak`）⇒ 若桩乱回，`p3_channel` 会变 `down`，
/// "控制通道健康"这一前提就没了（用例会变成在测别的东西）。
fn spawn_console_stub() -> (String, Arc<Mutex<Vec<Hit>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind console stub");
    let addr = listener.local_addr().expect("local_addr");
    let hits: Arc<Mutex<Vec<Hit>>> = Arc::new(Mutex::new(Vec::new()));
    let h = Arc::clone(&hits);
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            let Ok(mut sock) = sock else { break };
            let h = Arc::clone(&h);
            std::thread::spawn(move || {
                // 请求头可能分多次到达；读到 `\r\n\r\n` 即认为头齐（§3.4 的请求都很小）。
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let n = sock.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let head = String::from_utf8_lossy(&buf).into_owned();
                let mut parts = head.split_whitespace();
                let method = parts.next().unwrap_or_default().to_string();
                let target = parts.next().unwrap_or_default().to_string();
                let path = target.split('?').next().unwrap_or_default().to_string();
                h.lock().expect("hits lock").push(Hit {
                    method: method.clone(),
                    path: path.clone(),
                });
                let body = match path.as_str() {
                    "/v1/console/config" => {
                        r#"{"groups":[],"revision":1,"write_mode":"text_preserve"}"#.to_string()
                    }
                    "/v1/console/logs" => {
                        r#"{"entries":[],"next_cursor":null,"has_more":false,"range_too_large":false}"#
                            .to_string()
                    }
                    "/v1/console/logs/targets" => "[]".to_string(),
                    "/v1/console/audit" => {
                        r#"{"entries":[],"page":1,"page_size":20,"has_more":false,
                            "newest_ts_ms":null,"available":true}"#
                            .to_string()
                    }
                    "/v1/console/audit/ops" => "[]".to_string(),
                    _ => {
                        let _ = sock.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                        return;
                    }
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    (format!("http://{addr}"), hits)
}

/// 「接了就断」的哑端点：端口由本进程持有（不可能被抢），每个连接**收下即关**
/// ⇒ 对端拿到 EOF，永远得不到合法响应。与 `tests/offscreen_smoke.rs` 同法。
fn spawn_dead_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind dead endpoint");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            drop(sock); // 收下即断
        }
    });
    format!("http://{addr}")
}

/// 跑一次 `--smoke` 自检（20 拍）并收集输出。
fn run_smoke(control: &str, frame: &str) -> Output {
    Command::new(bin())
        .args([
            "--backend",
            "offscreen",
            "--channel",
            frame,
            "--control-channel",
            control,
            "--smoke",
        ])
        .output()
        .expect("spawn mupc-local-display")
}

/// 从退出统计行里取 `key=value`（控制通道的六个量都在那一行）。
fn exit_stat(stderr: &str, key: &str) -> Option<String> {
    stderr
        .lines()
        .find(|l| l.contains("[mupc-local-display] 退出："))
        .and_then(|line| {
            line.split_whitespace()
                .filter_map(|f| f.split_once('='))
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        })
}

// ═══════════════════════════════════════════════════════════════════════════
// ① T-3 门禁（**本单元最关键的一条**）
// ═══════════════════════════════════════════════════════════════════════════

/// **未确认 ⇒ 零写请求**：整个自检（20 拍 + 六页走查，**不触发任何确认**）里，
/// 控制通道桩**只**收到 GET，**一包 POST 都没有**；且 GET **确实发生过**（否则本条会
/// 因为"根本没接线"而**恒真通过** —— 那正是本项目最忌讳的伪门禁）。
///
/// # 改什么会让本条变红（**实测**，见交付报告「破坏性探针清单」）
///
/// - 在启动期（或任何"非确认"路径）加一句 `console.begin_write("apply", ..)` ⇒
///   桩收到 `POST /v1/console/config/apply` ⇒ 第二段断言立刻红；
/// - 把 `App::bind_intents` 里的意图回调改成"注册即发"（不经过队列）⇒ 同样红。
///
/// # 第一段断言**证明不了**什么（B3-2b-2 整改 **重要 2**：实测订正，勿高估）
///
/// ⚠️ **启动期那 5 条 GET 走 `pending_reads` + `tick_console` 路径，`不经意图队列`**
/// （生产段见 `app.rs::App::build` 的读清单与 `app.rs::App::tick_console` 的 ② 段）。
/// 故第一段「GET 必须真的发生过」只证明**tick 路径活着**；**它证明不了意图队列被消费**。
/// **实测**：把 `App::on_lv_events` 里的 `handle_control_intents` 调用摘掉，本条**仍然全绿**
/// —— 于是此前文档里写的「摘掉 `handle_control_intents` ⇒ 第一段会红」是**不实**描述，
/// 已删除。
///
/// 意图队列的消费由**两处**承担，均与本条无关：
/// ① `App::begin_write_intent` / `begin_query_intent` 的**唯一调用点**在
///    `App::handle_control_intents`（单点结构）；② 下一节
///    [`on_lv_events_consumes_the_intent_queue`] 的**源码扫描断言**（摘掉调用 ⇒ 它红）。
///
/// # 本条的**能力边界**（如实登记）
///
/// 它证明的是"**在本进程跑过的这段路径上**没有 POST"。它**不能**证明"用户确认后一定
/// 会 POST"—— 那条的正向判据需要触摸事件（离屏后端无 evdev），属真机验证项（设计 §14）。
/// ⇒ 门禁的**负向**一侧由本条钉死，**正向**一侧由 `control_route` 的纯逻辑用例
/// （`write_intents_are_exactly_the_three_post_endpoints`）与代码结构
/// （`begin_write` 的唯一调用点在 [`ControlIntent`] 的写分支）共同承担。
#[test]
fn no_confirmation_means_zero_write_requests() {
    let (ctl, hits) = spawn_console_stub();
    let frame = spawn_dead_endpoint();
    let out = run_smoke(&ctl, &frame);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let hits = hits.lock().expect("hits lock").clone();

    assert!(
        out.status.success(),
        "自检应返回 0；实得 {:?}\n--- stderr ---\n{stderr}",
        out.status.code()
    );
    // ① 接线必须**真的活着**（否则"零 POST"是假象：根本没发过任何请求）。
    let gets: Vec<&Hit> = hits.iter().filter(|h| !h.is_post()).collect();
    assert!(
        !gets.is_empty(),
        "控制通道桩一条请求都没收到 —— 接线没生效（或启动期读清单没发出去）；\
         此时「零 POST」是恒真假象\n--- stderr ---\n{stderr}"
    );
    // ② **T-3 门禁本体**：在任何"确认"发生之前，一包 POST 都不许有。
    let posts: Vec<&Hit> = hits.iter().filter(|h| h.is_post()).collect();
    assert!(
        posts.is_empty(),
        "未确认即发出写请求（违反 T-3：确认完成前不发任何请求）：{posts:?}\n\
         全部请求 = {hits:?}\n--- stderr ---\n{stderr}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ①′ 意图队列**消费点**的源码哨（补上一条抓不到的退化）
// ═══════════════════════════════════════════════════════════════════════════

/// **意图队列的消费点确实存在**：`App::on_lv_events` 的函数体里调用了
/// `handle_control_intents`。
///
/// # 为什么需要它（上一条被高估的教训，B3-2b-2 整改 **重要 2**）
///
/// [`no_confirmation_means_zero_write_requests`] 的第一段断言「桩收到过 GET」
/// **证明不了意图队列被消费** —— **实测**：把 `App::on_lv_events` 里的
/// `handle_control_intents` 调用摘掉，上一条**仍然全绿**（启动期 5 条 GET 走
/// `pending_reads` + `tick_console`，**不经意图队列**）。本条把"调用点存在"变成可判据的
/// 源码断言。
///
/// # 能力边界（如实登记，不得高估）
///
/// 源码扫描证明的是"**这一行在源码里**"（删掉 / 注释掉 ⇒ 红），**不是**"它在运行期被执行过"。
/// 运行期的存在性由**结构**保证：`App` 实现 [`mupc_local_display::timing::Host`]，而
/// `on_lv_events` 是 `timing::run` 循环体每拍必调的五个点之一；且该函数体**只有**这一句。
/// 与 `app.rs::tests::transport_failure_branch_dispatches_the_receipt_it_built` 同法。
///
/// **改什么会让本条变红**（**已实测**，见交付报告「探针 2」）：注释掉
/// `app.rs::App::on_lv_events` 里的 `self.handle_control_intents(now_epoch_ms());` ⇒ 本条红。
#[test]
fn on_lv_events_consumes_the_intent_queue() {
    /// 待扫源码（**生产段**；集成测试够得着源码文本，同 `app.rs` 内的先例）。
    const SRC: &str = include_str!("../src/app.rs");
    /// 入口签名；函数体自其右花括号起算。
    const FN_HEAD: &str = "fn on_lv_events(&mut self) {";
    /// 「就近」窗口（字符）：函数体很短，取足够覆盖它、又不至于越过下一个函数。
    const BODY_WINDOW: usize = 200;
    // 只扫**生产段**：测试段自身含同样的字面量（本文件与 `app.rs` 都算），会自证失真。
    let prod = SRC
        .split("#[cfg(test)]\nmod tests {")
        .next()
        .expect("app.rs 应能切出生产段");
    assert_ne!(
        prod.len(),
        SRC.len(),
        "未切出生产段（切分标记失效）：扫描器失真，本用例必须响亮失败"
    );
    let at = prod
        .find(FN_HEAD)
        .expect("`App::on_lv_events` 必须存在 —— 它是意图队列的**唯一**消费点");
    let tail = &prod[at + FN_HEAD.len()..];
    let near = &tail[..tail.len().min(BODY_WINDOW)];
    assert!(
        near.contains("self.handle_control_intents("),
        "`App::on_lv_events` 函数体内没有调用 `handle_control_intents` ⇒ **意图队列永不被消费**\
         （页面回调压进队列的写/读请求全部滞留；T-3 门禁的落点名存实亡）。\
         注意：进程级用例**抓不到**这个退化 —— 启动期 GET 走 `pending_reads`，不经意图队列。"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ② P3 通道条态的**驱动源**（控制通道 ≠ 帧通道）
// ═══════════════════════════════════════════════════════════════════════════

/// **控制通道健康 + 帧通道死** ⇒ P3 显「已连接」（`p3_channel=up`）。
///
/// # 本条的**能力边界**（**实测**订正，勿高估）
///
/// 它**不能**单独判别驱动源：把 `app.rs::tick_console` 的
/// `p3_connected(self.console.fail_streak())` 换成 `self.state.fail_streak()`（帧通道）后，
/// **本条仍绿** —— 实测原因是"接了就断"的帧通道对端在 20 拍自检里只失败 **1** 次
/// （`fail=1 < P3_DOWN_FAIL_STREAK=2`），判据同样是 `up`。**判别**由下一条
/// （[`p3_channel_goes_down_when_the_control_channel_is_dead`]）承担。
///
/// 本条锁的是**另一侧**的退化：若 `set_channel` 被接成常量 `false`、或"任一处失败就报断开"
/// （过度降级：控制通道明明健康却常年显「已断开」），本条立刻红。
///
/// **改什么会让本条变红**：把 `set_channel(connected)` 写成 `set_channel(false)` ⇒ 红；
/// 把 `p3_connected` 的阈值改成"失败 1 次即断"且帧通道先失败 ⇒ 红。
#[test]
fn p3_channel_state_follows_the_control_channel_not_the_frame_channel() {
    let (ctl, _hits) = spawn_console_stub();
    let frame = spawn_dead_endpoint(); // 帧通道**恒失败**
    let out = run_smoke(&ctl, &frame);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        exit_stat(&stderr, "p3_channel").as_deref(),
        Some("up"),
        "控制通道健康时 P3 必须显「已连接」（不得因**帧**通道断而误报断开）\n\
         --- stderr ---\n{stderr}"
    );
    // 对偶哨：帧通道确实在失败（否则本条连"帧断不误报"这一半也没测到）。
    let frames_fail = exit_stat(&stderr, "fail").unwrap_or_default();
    assert!(
        frames_fail.parse::<u64>().unwrap_or(0) > 0,
        "本组用例要求帧通道**确实在失败**（fail>0）；实得 fail={frames_fail}\n\
         --- stderr ---\n{stderr}"
    );
}

/// **控制通道死 + 帧通道健康** ⇒ P3 显「已断开」（`p3_channel=down`）。
///
/// 这一组才是**判别性**的（两条通道存亡相反，结论必须跟着**控制通道**翻）：
/// **实测**——把驱动源换成帧通道后，本用例打印 `p3_channel=up` 并**当场变红**
/// （帧健康 ⇒ `up`，而断言要 `down`）。这正是本单元要修的那个现存缺陷
/// （`P3LogsPage::set_channel` 此前生产零调用者，且语义上极易被误接成帧通道的 `ChannelStatus`）。
///
/// **改什么会让本条变红**：把驱动源换成帧通道 ⇒ 红（**已实测**）；
/// 把阈值从 2 改成 1 **不会**让本条红（死通道下两者都判 down）—— 阈值那一侧由
/// `control_route::tests::p3_channel_uses_control_fail_streak_with_threshold_two` 锁。
#[test]
fn p3_channel_goes_down_when_the_control_channel_is_dead() {
    let ctl = spawn_dead_endpoint(); // 控制通道**恒失败**
    let (frame_url, frame_hits, _misses) = spawn_frame_stub_url();
    let out = run_smoke(&ctl, &frame_url);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(
        exit_stat(&stderr, "p3_channel").as_deref(),
        Some("down"),
        "控制通道连续失败 ⇒ P3 必须显「已断开」（若为 up，说明驱动源接到了帧通道上）\n\
         --- stderr ---\n{stderr}"
    );
    let ctl_streak = exit_stat(&stderr, "console_fail_streak").unwrap_or_default();
    assert!(
        ctl_streak.parse::<u32>().unwrap_or(0) >= 2,
        "判据是**连续 2 次**控制通道失败（设计 §6.3）；实得 console_fail_streak={ctl_streak}\n\
         --- stderr ---\n{stderr}"
    );
    assert!(
        frame_hits.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "本组用例要求帧通道**确实在成功**（否则不构成对偶）\n--- stderr ---\n{stderr}"
    );
}

/// 帧通道桩（只要一个「活得下去」的 GET 对端；载荷用最小 v2 帧字面量）。
fn spawn_frame_stub_url() -> (String, Arc<std::sync::atomic::AtomicUsize>, Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::AtomicUsize;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind frame stub");
    let addr = listener.local_addr().expect("addr");
    let hits = Arc::new(AtomicUsize::new(0));
    let misses = Arc::new(AtomicUsize::new(0));
    let (h, m) = (Arc::clone(&hits), Arc::clone(&misses));
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            let Ok(mut sock) = sock else { break };
            let (h, m) = (Arc::clone(&h), Arc::clone(&m));
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                let n = sock.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                let resp = if req.starts_with("GET /v1/display/latest ") {
                    h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let body = frame_json();
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                } else {
                    m.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_string()
                };
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    (format!("http://{addr}/v1/display/latest"), hits, misses)
}

/// 最小合法 v2 帧（**字面量 JSON**，与 `tests/offscreen_smoke.rs` 同法：避免"同源同错"）。
fn frame_json() -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    format!(
        r#"{{"version":2,"seq":1,"ts_ms":{ts},"soc":65.0,"soc_source":"pcs_reg1010",
        "soc_flag":"valid","run_state":2,"pcs_online":true,
        "p_phase":[{{"v":12.3,"flag":"valid"}},{{"v":11.8,"flag":"valid"}},{{"v":12.0,"flag":"valid"}}],
        "p_total":{{"v":36.1,"flag":"valid"}},
        "i_phase":[{{"v":22.5,"flag":"valid"}},{{"v":22.1,"flag":"valid"}},{{"v":22.3,"flag":"valid"}}],
        "inconsistency":false}}"#
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// ③ 启动期回显 + 诊断读口
// ═══════════════════════════════════════════════════════════════════════════

/// `--control-channel` 的**实际取值**必须出现在启动行上，且**不得**再声称"不影响行为"
/// （B3-2b-2 之后那句已失真：参数此刻真的影响行为）。
///
/// **改什么会让本条变红**：把 `main.rs` 的 `eprintln!` 删掉 ⇒ 第 1 段红；
/// 把文案改回 `control_channel_pending_warning` ⇒ 第 2 段红。
#[test]
fn startup_echoes_the_live_control_channel_and_never_claims_it_is_inert() {
    let (ctl, _hits) = spawn_console_stub();
    let frame = spawn_dead_endpoint();
    let out = run_smoke(&ctl, &frame);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains(&format!("--control-channel {ctl}")),
        "启动行须回显**实际取值**（真机排障第一问：到底连的谁）：\n{stderr}"
    );
    assert!(
        !stderr.contains("不影响行为"),
        "B3-2b-2 已接线 ⇒ 不得再打印「不影响行为」的旧告警（失真文案必须整条删除）：\n{stderr}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// ④ `app` 层的两条"喂入值"接线（B3-2c：源码哨）
// ═══════════════════════════════════════════════════════════════════════════

/// **`Shell::set_modal_open` 喂的是页面的生产可见查询口**，不是常量。
///
/// # 为什么必须是源码哨
///
/// 喂入值在 `App::tick` 里（`App` 的构造需要 LVGL 会话 ⇒ 进程内 `#[test]` 起不了第二条
/// LVGL 线程，见 `src/ui/tests.rs` 模块头）；而 `--smoke` 路径里**任何弹层都不会打开**
/// （T-3 门禁要求"未确认 = 零写动作"）⇒ 进程级用例同样观测不到这条路径。
///
/// 本条守的是**这一行的形态**：喂入值必须由 `P2ConfigPage::dialog_open()` /
/// `P4InterlockPage::dialog_open()` 取或得到。外壳侧的**语义**（弹层打开 ⇒ 暂停计时 /
/// 不强制切页）由 `ui/tests.rs::shell_chain` ⑤″ 段以**真弹层 + 对象级断言**证。
///
/// # 能力边界（如实登记，不得高估）
///
/// 源码扫描证明的是"**这一行在源码里**"，**不是**"它在运行期被执行过"（`tick` 每拍必调，
/// 由 `timing::run` 的结构保证）。它抓的是那类**屏上看起来正常的退化**：
/// 把它改回常量 `false`（本单元开工前的状态）⇒ 弹层打开期间空闲回归照常倒计时并把用户
/// 从 P2/P4 顶回 P1（正在确认写操作的用户被踢出页面）—— 屏上不会报任何错。
///
/// **改什么会让本条变红**（**已实测**，见交付报告探针）：把这一行换回
/// `self.shell.set_modal_open(false)` 或 `self.control.confirm_open()` ⇒ 红。
#[test]
fn app_feeds_modal_open_from_the_pages_production_query() {
    const SRC: &str = include_str!("../src/app.rs");
    // 只扫**生产段**（测试段自身含同样的字面量 ⇒ 会自证失真，本项目踩过"扫描器失真"）。
    let prod = SRC
        .split("#[cfg(test)]\nmod tests {")
        .next()
        .expect("app.rs 应能切出生产段");
    assert_ne!(
        prod.len(),
        SRC.len(),
        "未切出生产段（切分标记失效）：扫描器失真，本用例必须响亮失败"
    );
    // 两段合起来 = 那一整条表达式（不锁换行版式：rustfmt 可能把它折成两行）。
    assert!(
        prod.contains("set_modal_open(self.shell.p2().dialog_open()"),
        "`App::tick` 必须把 P2 的**生产可见**弹层查询口喂进 `Shell::set_modal_open`\
         （喂入点或 P2 那一侧没了 ⇒ TT-13 再次落空）"
    );
    assert!(
        prod.contains("|| self.shell.p4().dialog_open()"),
        "P4 同理（两页**取或** = 「屏上此刻有任一确认弹层」；少一侧 ⇒ P4 弹层期间照常倒计时）"
    );
    // **负向**：恒 `false` / 恒读 `ControlState::confirm_open()` 两种旧写法都不得复活。
    assert!(
        !prod.contains("set_modal_open(false)"),
        "喂入值不得是常量 `false`（B3-2c 开工前的退化形态：TT-13 名存实亡）"
    );
    assert!(
        !prod.contains("confirm_open()"),
        "喂入值不得读 `ControlState::confirm_open()` —— 它**没有生产者**（恒 `None`），\
         弹层的生命周期归页面（见 `state.rs` 的 S-4 订正段）"
    );
}
