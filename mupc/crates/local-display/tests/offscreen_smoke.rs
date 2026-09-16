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
//! 时钟/环境：本用例全平台可跑（Windows 本机无 evdev/fb0 —— 用
//! `--backend offscreen` + `--channel` 指向桩，正是不变量"不许把 poll(2)/evdev 做成唯一路径"
//! 的可执行证据）。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// 帧 JSON（**字面量**，不用 display-proto 序列化 —— 避免"同源同错"掩盖契约偏差）。
fn frame_json(seq: u64) -> String {
    format!(
        r#"{{"version":2,"seq":{seq},"ts_ms":{},"soc":65.0,"soc_source":"pcs_reg1010",
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

/// 最小 HTTP 桩：对 `GET /v1/display/latest` 回 200 + 帧 JSON；其余路径 404。
///
/// 返回 `(url, 命中数, 漏检数)`；桩线程随进程结束（`detach`）——用例不关心它的收尾。
fn spawn_frame_stub() -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub");
    let addr = listener.local_addr().expect("local_addr");
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
                    h.fetch_add(1, Ordering::SeqCst);
                    let body = frame_json(h.load(Ordering::SeqCst) as u64);
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                } else {
                    m.fetch_add(1, Ordering::SeqCst);
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_string()
                };
                let _ = sock.write_all(resp.as_bytes());
            });
        }
    });
    (
        format!("http://{addr}/v1/display/latest"),
        hits,
        misses,
    )
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
    let (url, hits, misses) = spawn_frame_stub();
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
    assert!(hits.load(Ordering::SeqCst) >= 1, "读通道一次都没被命中：\n{stderr}");
    assert_eq!(misses.load(Ordering::SeqCst), 0, "不应请求错误路径：\n{stderr}");
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

/// **两个「已解析但当前不生效」的参数必须在启动期可见**（B3-2a 规格评审 建议 4/5）：
/// `--font`（v2.0 已废弃，字库改由构建期绑定）与 `--control-channel`（消费者属 B3-2b）。
///
/// 三重锁定：
/// ① 两条告警都真的打到 stderr（不是只在 help 里写写 —— 生产路径必须真调用文案函数）；
/// ② **退出码 0**：`--font` 是告警而非硬错误（本进程由 systemd `Restart=always` 托管，
///    硬错误 = 起不来 + 无限重启）；
/// ③ 告警里带**实际取值**（现场能据此判断"我这条 unit 是不是要改"）。
///
/// **改什么会让本条变红**：删掉 `main.rs::run_process` 里任一条 `eprintln!`（或把
/// `--font` 改成 `ExitCode::from(EXIT_USAGE)` 硬错误）⇒ ① / ② 立刻红。
#[test]
fn deprecated_params_warn_loudly_and_do_not_abort() {
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
        "控制通道告警缺失或未回显取值：\n{err}"
    );
    assert!(err.contains("不影响行为"), "控制通道告警须明说当前不影响行为：\n{err}");
    assert!(err.contains("B3-2b"), "控制通道告警须给出接线下游：\n{err}");
}
