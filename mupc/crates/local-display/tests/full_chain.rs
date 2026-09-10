//! 全链集成测试（设计 §10.1「无真屏环境验证路径」）：**真实 HTTP 通道** → 状态归一 → 布局绘制
//! → 离屏画布像素断言。本机（Windows x86 / Linux CI，无 HDMI、无中文字库、无 framebuffer）全绿。
//!
//! 链路（除真屏驱动层外全为生产代码）：
//! ```text
//!   mock 127.0.0.1 HTTP 桩（模仿 B3 LoopbackHttpPublisher 契约）
//!     └─ GET /v1/display/latest → 200 + display-proto DisplayFrame JSON
//!          └─ DisplayChannelClient（真实 TCP 回环 + 手写 HTTP/1.1）
//!               └─ DisplayState（新鲜度/通道态/三态归一）
//!                    └─ layout::render → OffscreenCanvas（像素断言）
//! ```
//!
//! 覆盖场景（任务清单 1:1）：
//! | 测试 | 场景 |
//! |------|------|
//! | [`full_chain_live_frame_renders_soc_and_phase_values`] | 正常帧（值+语义色上屏） |
//! | [`full_chain_stale_frame_marks_expired_and_keeps_values`] | 帧超时 stale（冻结+打标，不冒充实时） |
//! | [`full_chain_missing_fields_render_dash_not_zero`] | 字段缺失 `--`（禁显 0.0） |
//! | [`full_chain_soc_lost_renders_dash_and_warning`] | SOC 双源皆失 → `--` + 源失效警示 |
//! | [`full_chain_channel_down_overlay_then_recover_to_live`] | 通道断开 → 整屏态 → 恢复回实时 |
//! | [`full_chain_run_loop_consumes_mock_frames`] | 主循环真实节拍消费桩帧（+ 路径契约 404） |
//!
//! 时钟：`Renderer::poll_once` 接受**注入的 Unix 毫秒**，故超时/断连无需真等 2s/3s，
//! 测试确定且快速（无 flaky sleep 依赖）。
//!
//! ⚠️ 本文件不含真机路径（`--backend fbdev` 的 `/dev/fb0` mmap、像素格式/bpp/字节序、
//! HDMI 分辨率协商）——属设计 §13 前置项 1/6，需 Linux 真机验证。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use mupc_local_display::canvas::{OffscreenCanvas, Rect};
use mupc_local_display::channel::DisplayChannelClient;
use mupc_local_display::config::CliConfig;
use mupc_local_display::font::TextKit;
use mupc_local_display::layout;
use mupc_local_display::run::{now_epoch_ms, Renderer, RunStats};
use mupc_local_display::state::ScreenMode;

/// 帧 JSON 里的占位符（桩按请求现算时间戳，模拟 mupcd 每 1s 组帧发布）。
const PH_TS: &str = "__TS__";
const PH_SEQ: &str = "__SEQ__";

/// 正常帧（值全有效、SOC 65%、充电中）——字面量 JSON，与设计 §3.3 线上格式逐字对齐
/// （**不**用 display-proto 序列化，避免「同源同错」掩盖契约偏差）。
fn live_frame_template() -> String {
    r#"{"version":1,"seq":__SEQ__,"ts_ms":__TS__,"soc":65.0,"soc_source":"pcs_reg1010",
        "soc_flag":"valid","run_state":2,"pcs_online":true,
        "p_phase":[{"v":12.3,"flag":"valid"},{"v":11.8,"flag":"valid"},{"v":12.0,"flag":"valid"}],
        "p_total":{"v":36.1,"flag":"valid"},
        "i_phase":[{"v":22.5,"flag":"valid"},{"v":22.1,"flag":"valid"},{"v":22.3,"flag":"valid"}],
        "inconsistency":false}"#
        .to_string()
}

/// 字段缺失帧：三相功率/电流 `not_read`（设计 §6.4 点表未覆盖）→ 各卡应显 `--` + 「未取数」。
fn not_read_frame_template() -> String {
    r#"{"version":1,"seq":__SEQ__,"ts_ms":__TS__,"soc":50.0,"soc_source":"bms",
        "soc_flag":"valid","run_state":1,"pcs_online":false,
        "p_phase":[{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"}],
        "p_total":{"v":null,"flag":"not_read"},
        "i_phase":[{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"},{"v":null,"flag":"not_read"}],
        "inconsistency":false}"#
        .to_string()
}

/// SOC 双源皆失帧（设计 §6.2）：`soc=null` + `soc_source=lost` → `--` + 「SOC 源失效」。
fn soc_lost_frame_template() -> String {
    r#"{"version":1,"seq":__SEQ__,"ts_ms":__TS__,"soc":null,"soc_source":"lost",
        "soc_flag":"offline","run_state":null,"pcs_online":false,
        "p_phase":[{"v":null,"flag":"offline"},{"v":null,"flag":"offline"},{"v":null,"flag":"offline"}],
        "p_total":{"v":null,"flag":"offline"},
        "i_phase":[{"v":null,"flag":"offline"},{"v":null,"flag":"offline"},{"v":null,"flag":"offline"}],
        "inconsistency":false}"#
        .to_string()
}

fn fill(tpl: &str, seq: u64, ts_ms: u64) -> String {
    tpl.replace(PH_SEQ, &seq.to_string()).replace(PH_TS, &ts_ms.to_string())
}

/// mock 端：B3 契约的 `GET /v1/display/latest` → 200 + JSON + `Content-Length`；其余路径 → 404
/// （校验渲染端确实按设计 §3.1 的端点取帧，而非「任意路径都能拿帧」）。
struct MockFrameServer {
    url: String,
    hits: Arc<AtomicU64>,
    misses: Arc<AtomicU64>,
}

impl MockFrameServer {
    /// `body_fn` 每请求调用一次（桩可现算 `ts_ms` 模拟 1Hz 发布）。
    async fn spawn(body_fn: Arc<dyn Fn() -> String + Send + Sync>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/display/latest", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicU64::new(0));
        let misses = Arc::new(AtomicU64::new(0));
        let (h, m) = (Arc::clone(&hits), Arc::clone(&misses));
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(x) => x,
                    Err(_) => break,
                };
                let body = body_fn();
                let (h, m) = (Arc::clone(&h), Arc::clone(&m));
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    // 读请求头（GET 无 body）
                    let mut one = [0u8; 1];
                    let mut req = Vec::new();
                    loop {
                        if sock.read_exact(&mut one).await.is_err() {
                            return;
                        }
                        req.push(one[0]);
                        if req.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    let is_latest = req.starts_with(b"GET /v1/display/latest ");
                    let (status, payload) = if is_latest {
                        h.fetch_add(1, Ordering::Relaxed);
                        ("200 OK", body.into_bytes())
                    } else {
                        m.fetch_add(1, Ordering::Relaxed);
                        ("404 Not Found", Vec::new())
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&payload).await;
                });
            }
        });
        Self { url, hits, misses }
    }

    fn client(&self) -> DisplayChannelClient {
        DisplayChannelClient::try_new(&self.url).unwrap()
    }
}

/// 起一个「未就绪」桩：503（模拟 mupcd 端口在、帧未发布；渲染端视同无新帧重试）。
async fn spawn_503_stub() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/display/latest", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut one = [0u8; 1];
                let mut req = Vec::new();
                loop {
                    if sock.read_exact(&mut one).await.is_err() {
                        return;
                    }
                    req.push(one[0]);
                    if req.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                let _ = sock
                    .write_all(
                        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
            });
        }
    });
    url
}

fn renderer(stale_ms: u64) -> Renderer<OffscreenCanvas> {
    // 内置 ASCII 回退字库：本机无中文字体也能确定性出像素（中文为占位盒，仍非背景色）。
    Renderer::new(
        TextKit::fallback(),
        OffscreenCanvas::new(layout::SCREEN_W as u32, layout::SCREEN_H as u32),
        stale_ms,
    )
}

fn full_screen() -> Rect {
    Rect::new(0, 0, layout::SCREEN_W, layout::SCREEN_H)
}

// ---------------------------------------------------------------------------
// 1. 正常帧
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_chain_live_frame_renders_soc_and_phase_values() {
    let srv = MockFrameServer::spawn(Arc::new(|| fill(&live_frame_template(), 7, now_epoch_ms()))).await;
    let client = srv.client();
    let now = now_epoch_ms();
    let mut r = renderer(mupc_display_proto::DEFAULT_STALE_MS);

    assert!(r.poll_once(&client, now).await, "首帧应触发重绘");
    assert_eq!(srv.hits.load(Ordering::Relaxed), 1, "渲染端应命中 /v1/display/latest");
    assert_eq!(srv.misses.load(Ordering::Relaxed), 0);
    assert_eq!(r.state().screen_mode(now), ScreenMode::Live);
    assert_eq!(r.state().frame().map(|f| f.seq), Some(7));

    let cv = r.canvas();
    // SOC 大数字区：应有非背景字形像素，且用「15-85 青」区间色（65%）
    assert!(
        cv.has_non_background(&layout::soc_value_region(), layout::BG),
        "SOC 数值区应有字形像素"
    );
    assert!(
        cv.count_color(&layout::soc_value_region(), layout::SOC_CYAN) > 0,
        "65% 应落中段青色"
    );
    // 三相四卡（A/B/C/总）均有数值像素
    for i in 0..4 {
        let card = layout::phase_card(i);
        assert!(
            cv.has_non_background(&card, layout::BG),
            "第 {i} 张卡应有绘制内容"
        );
    }
    // 新鲜帧：全屏不应出现「数据过期」/断连的琥珀提示
    assert_eq!(cv.count_color(&full_screen(), layout::AMBER), 0);
}

// ---------------------------------------------------------------------------
// 2. 帧超时（stale）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_chain_stale_frame_marks_expired_and_keeps_values() {
    // 桩持续发布「5 秒前」的帧（ts_ms 固定在过去）→ now - ts > stale_ms(2000)
    let srv = MockFrameServer::spawn(Arc::new(|| fill(&live_frame_template(), 8, now_epoch_ms() - 5_000))).await;
    let client = srv.client();
    let now = now_epoch_ms();
    let mut r = renderer(mupc_display_proto::DEFAULT_STALE_MS);

    assert!(r.poll_once(&client, now).await);
    assert_eq!(
        r.state().freshness(now),
        mupc_local_display::state::Freshness::Stale,
        "now-ts > 2000ms 应判过期"
    );
    assert_eq!(r.state().screen_mode(now), ScreenMode::Live, "过期不是整屏断连态");

    let cv = r.canvas();
    assert!(
        cv.count_color(&full_screen(), layout::AMBER) > 0,
        "应打「数据过期」琥珀标（页眉/SOC 区）"
    );
    assert!(
        cv.count_color(&layout::soc_value_region(), layout::SOC_CYAN) > 0,
        "过期仅打标：保留最近有效值展示（不冒充实时，但也不清空）"
    );
}

// ---------------------------------------------------------------------------
// 3. 字段缺失 → `--`
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_chain_missing_fields_render_dash_not_zero() {
    let srv = MockFrameServer::spawn(Arc::new(|| fill(&not_read_frame_template(), 9, now_epoch_ms()))).await;
    let client = srv.client();
    let now = now_epoch_ms();
    let mut r = renderer(mupc_display_proto::DEFAULT_STALE_MS);
    assert!(r.poll_once(&client, now).await);

    let cv = r.canvas();
    // 每张卡内应有降级灰 `--` 字形（PRD 不造假值：绝不显示 0.0）
    for i in 0..4 {
        let card = layout::phase_card(i);
        assert!(
            cv.count_color(&card, layout::DEGRADED) > 0,
            "第 {i} 张卡应画降级灰 `--`（未取数）"
        );
    }
    // SOC 50% 仍有效 → 中段青色正常展示（点级独立降级，F5.5）
    assert!(cv.count_color(&layout::soc_value_region(), layout::SOC_CYAN) > 0);
    // 未取数不改整体新鲜度
    assert_eq!(
        r.state().freshness(now),
        mupc_local_display::state::Freshness::Fresh
    );
}

// ---------------------------------------------------------------------------
// 4. SOC 双源皆失
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_chain_soc_lost_renders_dash_and_warning() {
    let srv = MockFrameServer::spawn(Arc::new(|| fill(&soc_lost_frame_template(), 10, now_epoch_ms()))).await;
    let client = srv.client();
    let now = now_epoch_ms();
    let mut r = renderer(mupc_display_proto::DEFAULT_STALE_MS);
    assert!(r.poll_once(&client, now).await);

    let cv = r.canvas();
    assert!(
        cv.count_color(&layout::soc_value_region(), layout::DEGRADED) > 0,
        "双源皆失应画 `--` 降级灰（不补 0、不沿用旧值）"
    );
    // 顶部 3px 警示描边（SOC_RED）
    let accent = Rect::new(layout::SOC_X0, layout::SOC_Y0, layout::SOC_X1, layout::SOC_Y0 + 3);
    assert!(cv.count_color(&accent, layout::SOC_RED) > 0, "源失效应有红色警示描边");
    // SOC 区不应出现任何区间色（青/橙）
    assert_eq!(cv.count_color(&layout::soc_value_region(), layout::SOC_CYAN), 0);
    assert_eq!(cv.count_color(&layout::soc_value_region(), layout::SOC_ORANGE), 0);
}

// ---------------------------------------------------------------------------
// 5. 通道断开 → 整屏态 → 恢复
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_chain_channel_down_overlay_then_recover_to_live() {
    let t0 = 1_000_000u64; // 注入时钟：无需真等 3s
    // ① 端口在但帧未就绪（503）→ Init（渲染可先于 mupcd 启动，设计 §4.3.4）
    let stub503 = spawn_503_stub().await;
    let down_client = DisplayChannelClient::try_new(&stub503).unwrap();
    let mut r = renderer(2_000);
    assert!(r.poll_once(&down_client, t0).await, "Init 占位需首绘");
    assert_eq!(r.state().screen_mode(t0), ScreenMode::Init);

    // ② 无成功 >3s → 整屏「与主进程数据通道断开」（设计 §6.3 / PRD 6.3）
    assert!(r.poll_once(&down_client, t0 + 3_500).await);
    assert_eq!(r.state().screen_mode(t0 + 3_500), ScreenMode::ChannelDown);
    let cv = r.canvas();
    assert!(
        cv.count_color(&Rect::new(0, 260, layout::SCREEN_W, 460), layout::AMBER) > 0,
        "整屏应提示「与主进程数据通道断开」"
    );
    assert_eq!(
        cv.count_color(&layout::soc_value_region(), layout::SOC_CYAN),
        0,
        "断连态为整屏覆盖，不以旧值冒充实时"
    );

    // ③ mupcd 恢复 → 下一次成功 GET（≤500ms 语义）即回实时
    let srv = MockFrameServer::spawn(Arc::new(|| fill(&live_frame_template(), 11, now_epoch_ms()))).await;
    let live_client = srv.client();
    let now = now_epoch_ms();
    assert!(r.poll_once(&live_client, now).await);
    assert_eq!(r.state().screen_mode(now), ScreenMode::Live);
    let cv = r.canvas();
    assert_eq!(
        cv.count_color(&Rect::new(0, 260, layout::SCREEN_W, 460), layout::AMBER),
        0,
        "恢复后不应残留断连覆盖"
    );
    assert!(cv.count_color(&layout::soc_value_region(), layout::SOC_CYAN) > 0, "回实时后正常取值");
}

// ---------------------------------------------------------------------------
// 6. 主循环（真实节拍）+ 端点契约
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_chain_run_loop_consumes_mock_frames() {
    let live = Arc::new(live_frame_template());
    let srv = MockFrameServer::spawn(Arc::new(move || fill(&live, 12, now_epoch_ms()))).await;
    let client = srv.client();

    let cfg = CliConfig {
        channel: srv.url.clone(),
        interval_ms: 100,
        backend: mupc_local_display::config::Backend::Offscreen,
        ..Default::default()
    };
    let mut r = renderer(cfg.stale_ms);
    let stop = std::sync::atomic::AtomicBool::new(false);

    let stats: RunStats = mupc_local_display::run::run_loop(&cfg, &client, &mut r, &stop, Some(3)).await;
    assert_eq!(stats.ticks, 3);
    assert_eq!(stats.ok, 3, "3 拍都应取到桩帧");
    assert_eq!(stats.fail, 0);
    assert!(stats.redraws >= 1);
    assert_eq!(srv.hits.load(Ordering::Relaxed), 3, "渲染端应命中正确端点 3 次");
    assert_eq!(srv.misses.load(Ordering::Relaxed), 0, "不应请求错误路径");
    assert!(
        stats.max_draw_ms <= 30,
        "单帧绘制应 ≤30ms（设计 §9 预算），实得 {}ms",
        stats.max_draw_ms
    );
    assert!(r.canvas().has_non_background(&layout::soc_value_region(), layout::BG));
    // 页眉时钟由渲染侧注入（OCR 可读的 ASCII 文本，非空即已绘制）
    assert!(!r.snapshot(now_epoch_ms()).clock_text.is_empty());
}

/// 端点契约：错误路径 → 404 → 渲染端判失败（不静默当作有效帧）。
#[tokio::test]
async fn wrong_path_is_rejected_and_counted_as_failure() {
    let srv = MockFrameServer::spawn(Arc::new(|| fill(&live_frame_template(), 1, now_epoch_ms()))).await;
    let wrong_url = srv.url.replace("/v1/display/latest", "/wrong");
    let client = DisplayChannelClient::try_new(&wrong_url).unwrap();
    let mut r = renderer(2_000);
    let now = now_epoch_ms();
    r.poll_once(&client, now).await;
    assert_eq!(r.state().fail_streak(), 1, "404 应计入失败");
    assert!(r.state().frame().is_none(), "不得把 404 当作有效帧");
    assert_eq!(srv.misses.load(Ordering::Relaxed), 1);
    assert_eq!(srv.hits.load(Ordering::Relaxed), 0);
}
