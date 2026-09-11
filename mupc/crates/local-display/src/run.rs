//! 渲染主循环编排（设计 §5.3）：`fetch → state.update → 按需重绘 → blit → 节拍 sleep`。
//!
//! 结构（可测性优先）：
//! - [`Renderer`]：**注入时钟**的状态+画布容器。`poll_once(now_ms)` 完成「拉帧→归一→按需重绘」
//!   一轮，时钟由调用方给（Unix 毫秒），因此无需真屏/真等待即可在单测与集成测试里确定性地
//!   走完「正常帧 / 帧过期 / 字段降级 / 通道断」四条路径（设计 §10.1 无真屏验证路径）。
//! - [`run_loop`]：真实节拍循环（阻塞轮询 + `sleep`，无忙等——PRD 4.1.3），可注入停止标志与
//!   最大 tick 数（测试/冒烟用）。
//!
//! 不裸 panic：通道失败只计入统计并驱动通道态（≤3s 切整屏断连态，PRD 6.3）；只有「后端打不开」
//! 这类启动期致命错误才向上返回明确错误，由 bin 打印后非零退出（交给 systemd Restart，设计 §9）。
//!
//! 时钟口径：帧 `ts_ms` 为 **Unix 毫秒**，故本文件统一用 Unix 毫秒（`now_epoch_ms`）做新鲜度
//! 比对；页眉时钟文本为 **UTC**（不引时间库，见 [`clock_text_hms`] 注释）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::canvas::Canvas;
use crate::channel::DisplayChannelClient;
use crate::config::CliConfig;
use crate::font::TextKit;
use crate::layout;
use crate::state::{DisplayState, Freshness, ScreenMode, UiSnapshot};

/// 强制重整周期（设计 §5.3「有变化 或 阈值态翻转 或 强制 2Hz」→ 本实现取 1Hz 重整，
/// 主要服务页眉时钟刷新；数值/态变化仍即时重绘）。
pub const FORCE_REDRAW_MS: u64 = 1000;

/// 主循环统计（诊断/`--backend offscreen` 调试打印/退出报告）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RunStats {
    /// 拉帧轮次。
    pub ticks: u64,
    /// 成功拉帧次数。
    pub ok: u64,
    /// 失败拉帧次数（连接拒绝/超时/非 200/JSON 错）。
    pub fail: u64,
    /// 实际重绘次数。
    pub redraws: u64,
    /// 最近一次整帧绘制耗时(ms)。
    pub last_draw_ms: u64,
    /// 整帧绘制耗时峰值(ms)（设计 §9 预算：单帧 ≤30ms）。
    pub max_draw_ms: u64,
}

/// 当前 Unix 毫秒（与帧 `ts_ms` 同口径；不受时区影响）。
pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 页眉时钟文本 `HH:MM:SS UTC`（**仅 ASCII**，落在 font.rs `REQ_ASCII` 码表内，无缺字风险）。
///
/// 说明（KISS，设计未覆盖）：不引 `chrono`/TZ 库，故为 **UTC** 而非本地时间；屏面用于就地读屏
/// 复述，UTC 已足够且不会因设备无 tzdata 而失真。若产品要求本地时间，可在真机部署期改用
/// `localtime_r`（需 libc，仅 Linux）——属后续小改，接口不变。
pub fn clock_text_hms(epoch_ms: u64) -> String {
    let sod = (epoch_ms / 1000) % 86_400;
    format!("{:02}:{:02}:{:02} UTC", sod / 3600, (sod % 3600) / 60, sod % 60)
}

/// `drm` 后端未实现时的**明确错误**（设计 §13 前置项 1：待真机校验 `/dev/fb0` 像素格式/位深后
/// 再决定是否需要 DRM dumb-buffer 主平面）。绝不静默回退 offscreen——渲染进程必须有真实输出。
pub fn drm_unimplemented_error() -> crate::Error {
    crate::Error::Backend(
        "backend drm 未实现：设计 §13 前置项 1 待真机校验 /dev/fb0 像素格式/位深/字节序后再定 \
         DRM dumb-buffer 后端；真机请用 --backend fbdev（或先 --backend offscreen 打通全链路）"
            .to_string(),
    )
}

/// 渲染器：通道状态 + 字库 + 画布（泛型持有，测试可直接对 `OffscreenCanvas` 断言像素）。
pub struct Renderer<C: Canvas> {
    tk: TextKit,
    canvas: C,
    state: DisplayState,
    stats: RunStats,
    /// 上次重绘的「语义键」：整屏模式 / 新鲜度 / 帧序号——任一变化即重绘。
    drawn_key: Option<(ScreenMode, Freshness, Option<u64>)>,
    /// 上次重绘时刻（Unix ms；驱动 [`FORCE_REDRAW_MS`] 整流）。
    last_draw_ms: Option<u64>,
}

impl<C: Canvas> Renderer<C> {
    /// 装配（`stale_ms` 取自 CLI，默认 display-proto `DEFAULT_STALE_MS`）。
    pub fn new(tk: TextKit, canvas: C, stale_ms: u64) -> Self {
        let mut state = DisplayState::new();
        state.set_stale_ms(stale_ms);
        Self {
            tk,
            canvas,
            state,
            stats: RunStats::default(),
            drawn_key: None,
            last_draw_ms: None,
        }
    }

    pub fn canvas(&self) -> &C {
        &self.canvas
    }

    pub fn canvas_mut(&mut self) -> &mut C {
        &mut self.canvas
    }

    pub fn state(&self) -> &DisplayState {
        &self.state
    }

    pub fn stats(&self) -> RunStats {
        self.stats
    }

    /// 当前归一化视图（含页眉时钟文本；供测试/调试）。
    pub fn snapshot(&self, now_ms: u64) -> UiSnapshot {
        let mut snap = self.state.snapshot(now_ms);
        snap.clock_text = clock_text_hms(now_ms);
        snap
    }

    /// 一轮：拉最新帧 → 归一状态 → 按需重绘。返回本轮是否重绘。
    ///
    /// `now_ms` 为注入的 Unix 毫秒（决定新鲜度/通道态），失败**不**返回错误——通道断是正常
    /// 展示态之一（PRD 6.3），由 [`DisplayState`] 在连续/累计失败后派生。
    pub async fn poll_once(&mut self, client: &DisplayChannelClient, now_ms: u64) -> bool {
        let res = client.fetch_latest().await;
        match &res {
            Ok(_) => self.stats.ok += 1,
            Err(e) => {
                self.stats.fail += 1;
                // 首失败与每 10 次记一行，避免通道长期断连时刷屏（设计 §9 断连 CPU/日志友好）。
                if self.stats.fail == 1 || self.stats.fail % 10 == 0 {
                    eprintln!(
                        "[mupc-local-display] 拉帧失败(第 {} 次)：{e}（超过 3s 无成功将切「数据通道断开」态）",
                        self.stats.fail
                    );
                }
            }
        }
        self.state.update(res, now_ms);
        self.stats.ticks += 1;
        self.redraw_if_needed(now_ms)
    }

    /// 变化即重绘（模式/新鲜度/帧序号任一变化，或距上次 ≥[`FORCE_REDRAW_MS`]）。
    pub fn redraw_if_needed(&mut self, now_ms: u64) -> bool {
        let mode = self.state.screen_mode(now_ms);
        let fresh = self.state.freshness(now_ms);
        let seq = self.state.frame().map(|f| f.seq);
        let key = (mode, fresh, seq);
        let due = self
            .last_draw_ms
            .map_or(true, |t| now_ms.saturating_sub(t) >= FORCE_REDRAW_MS);
        if self.drawn_key == Some(key) && !due {
            return false;
        }
        self.draw(now_ms, key)
    }

    /// 无条件整帧重绘（首帧/`--smoke`/测试用）。
    pub fn force_redraw(&mut self, now_ms: u64) -> bool {
        let key = (
            self.state.screen_mode(now_ms),
            self.state.freshness(now_ms),
            self.state.frame().map(|f| f.seq),
        );
        self.draw(now_ms, key)
    }

    fn draw(&mut self, now_ms: u64, key: (ScreenMode, Freshness, Option<u64>)) -> bool {
        let t0 = Instant::now();
        let snap = self.snapshot(now_ms);
        // KISS：整屏重绘（脏区收窄为后续优化点）；离屏/帧缓冲共用同一套绘制命令。
        layout::render(&mut self.canvas, &self.tk, &snap);
        let ms = t0.elapsed().as_millis() as u64;
        self.stats.redraws += 1;
        self.stats.last_draw_ms = ms;
        self.stats.max_draw_ms = self.stats.max_draw_ms.max(ms);
        self.drawn_key = Some(key);
        self.last_draw_ms = Some(now_ms);
        true
    }
}

/// 主循环（阻塞轮询 + 定拍 sleep，无忙等）。返回退出时的统计。
///
/// - `stop`：外部停止标志（bin 侧由 Ctrl-C/SIGTERM 置位；测试注入）。
/// - `max_ticks`：`Some(n)` 跑满 n 轮即返回（冒烟/测试）；`None` = 常驻直到 `stop`。
pub async fn run_loop<C: Canvas>(
    cfg: &CliConfig,
    client: &DisplayChannelClient,
    renderer: &mut Renderer<C>,
    stop: &AtomicBool,
    max_ticks: Option<u64>,
) -> RunStats {
    let verbose = cfg.is_offscreen();
    let mut tick: u64 = 0;
    let mut last_log_ms: Option<u64> = None;
    if verbose {
        eprintln!(
            "[mupc-local-display] offscreen 调试模式：channel={} interval={}ms stale={}ms size={}x{}",
            cfg.channel, cfg.interval_ms, cfg.stale_ms, cfg.width, cfg.height
        );
    }
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let now = now_epoch_ms();
        let redrew = renderer.poll_once(client, now).await;
        tick += 1;

        if verbose && redrew {
            let s = renderer.stats();
            let due = last_log_ms.map_or(true, |t| now.saturating_sub(t) >= FORCE_REDRAW_MS);
            if due {
                last_log_ms = Some(now);
                let st = renderer.state();
                eprintln!(
                    "[local-display] tick={tick} mode={:?} seq={:?} ok={} fail={} redraws={} draw={}ms(max {}ms)",
                    st.screen_mode(now),
                    st.frame().map(|f| f.seq),
                    s.ok,
                    s.fail,
                    s.redraws,
                    s.last_draw_ms,
                    s.max_draw_ms
                );
            }
        }

        if let Some(m) = max_ticks {
            if tick >= m {
                break;
            }
        }
        // 定拍：睡眠到下一拍（间隔误差不累积——用 poll 周期做步长即可，KISS）。
        tokio::time::sleep(cfg.interval()).await;
    }
    renderer.stats()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{OffscreenCanvas, Rect};
    use crate::config::Backend;
    use crate::state::ChannelStatus;
    use mupc_display_proto::{DisplayFrame, Field, FieldFlag, RunState, SocSource};

    fn kit() -> TextKit {
        // 用内置 ASCII 回退：本机无中文字库也能确定性渲染（中文画占位盒）。
        TextKit::fallback()
    }

    fn full() -> Rect {
        Rect::new(0, 0, 1024, 768)
    }

    fn frame(seq: u64, ts_ms: u64) -> DisplayFrame {
        DisplayFrame {
            version: mupc_display_proto::PROTO_VERSION,
            seq,
            ts_ms,
            soc: Some(65.0),
            soc_source: SocSource::Bms,
            soc_flag: FieldFlag::Valid,
            run_state: Some(RunState::Charge),
            pcs_online: true,
            p_phase: [Field { v: Some(12.3), flag: FieldFlag::Valid }; 3],
            p_total: Field { v: Some(36.1), flag: FieldFlag::Valid },
            i_phase: [Field { v: Some(22.5), flag: FieldFlag::Valid }; 3],
            inconsistency: false,
            // v2 契约新增的四段：本测试桩只关心 v1 字段，四段一律取契约缺省
            // （`DeviceSection` 等均 `#[serde(default)]` + `Default`，语义 = 「未提供」）。
            device: Default::default(),
            alarms: Default::default(),
            info: Default::default(),
            interlock: Default::default(),
        }
    }

    /// 起一个最小 503 桩（服务端「尚未就绪」）——比连不通端口更确定/更快（各平台一致）。
    async fn spawn_503_stub() -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/display/latest", l.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = l.accept().await {
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

    #[test]
    fn clock_text_is_ascii_utc() {
        // 1970-01-01T00:00:00Z
        assert_eq!(clock_text_hms(0), "00:00:00 UTC");
        // 1970-01-01T12:34:56Z = 45296 s
        assert_eq!(clock_text_hms(45_296 * 1000), "12:34:56 UTC");
        // 跨天回绕（+24h 后仍同一时刻）
        assert_eq!(clock_text_hms(45_296 * 1000 + 86_400_000), "12:34:56 UTC");
        // 全 ASCII（font.rs REQ_ASCII 码表覆盖，无缺字风险）
        assert!(clock_text_hms(1_700_000_000_000).is_ascii());
    }

    /// 渲染器核心链：Init 占位 → 正常帧取值区着色 → 1Hz 整流重绘 → 语义键不变不重复重绘。
    #[test]
    fn renderer_draws_init_then_live_and_throttles() {
        let mut r = Renderer::new(kit(), OffscreenCanvas::new_default(), 2000);

        // 第 1 拍：失败（通道未就绪）→ Init 占位，必有重绘
        r.state.record_fail(1_000);
        assert!(r.redraw_if_needed(1_000));
        assert_eq!(r.state().channel_status(1_000), ChannelStatus::Init);
        assert_eq!(r.state().screen_mode(1_000), ScreenMode::Init);
        // Init 占位整屏无 SOC 数值着色
        assert!(!r.canvas().has_non_background(&layout::soc_value_region(), layout::BG));

        // 成功一帧 → Live，SOC 数值区出现字形像素
        r.state.record_success(frame(1, 2_000), 2_000);
        assert!(r.redraw_if_needed(2_000));
        assert_eq!(r.state().screen_mode(2_000), ScreenMode::Live);
        assert!(
            r.canvas().has_non_background(&layout::soc_value_region(), layout::BG),
            "SOC 数值区应有字形像素（非背景）"
        );
        assert_eq!(r.canvas().count_color(&full(), layout::AMBER), 0, "新鲜帧不应有过期琥珀标");

        // 语义键不变且未到整流周期 → 不重绘
        assert!(!r.redraw_if_needed(2_400));
        // 到 1Hz 整流周期 → 重绘（页眉时钟刷新）
        assert!(r.redraw_if_needed(3_100));
        assert_eq!(r.stats().redraws, 3);
    }

    #[test]
    fn stale_frame_keeps_value_and_marks_expired() {
        let mut r = Renderer::new(kit(), OffscreenCanvas::new_default(), 2000);
        r.state.record_success(frame(9, 1_000), 1_000);
        assert!(r.redraw_if_needed(5_000)); // now-ts = 4000 > 2000
        assert_eq!(r.state().freshness(5_000), Freshness::Stale);
        assert!(r.canvas().count_color(&full(), layout::AMBER) > 0, "应画「数据过期」琥珀标");
        assert!(
            r.canvas().has_non_background(&layout::soc_value_region(), layout::BG),
            "过期仅打标，数值仍保留展示（不冒充实时）"
        );
    }

    #[test]
    fn channel_down_overlays_screen() {
        let mut r = Renderer::new(kit(), OffscreenCanvas::new_default(), 2000);
        r.state.record_success(frame(1, 0), 0);
        r.state.record_fail(1_000);
        assert!(r.redraw_if_needed(4_000)); // 无成功 >3s → Down
        assert_eq!(r.state().channel_status(4_000), ChannelStatus::Down);
        assert_eq!(r.state().screen_mode(4_000), ScreenMode::ChannelDown);
        let mid = Rect::new(0, 260, 1024, 460);
        assert!(r.canvas().count_color(&mid, layout::AMBER) > 0, "整屏应提示「与主进程数据通道断开」");
        // 断连态为整屏覆盖：SOC 区不再有数值着色/降级灰（旧值不以实时样式呈现）
        let soc_region = layout::soc_value_region();
        assert_eq!(r.canvas().count_color(&soc_region, layout::SOC_CYAN), 0);
        assert_eq!(r.canvas().count_color(&soc_region, layout::DEGRADED), 0);
    }

    /// 主循环节拍：503 桩 → 每拍失败、Init 语义键不变只首拍重绘、跑满 max_ticks 即返回。
    #[tokio::test]
    async fn run_loop_paces_and_stops() {
        use crate::channel::DisplayChannelClient;
        let url = spawn_503_stub().await;
        let cfg = CliConfig {
            channel: url,
            interval_ms: 50,
            backend: Backend::Offscreen,
            ..Default::default()
        };
        let client = DisplayChannelClient::try_new(&cfg.channel).unwrap();
        let mut r = Renderer::new(kit(), OffscreenCanvas::new(320, 240), 2000);
        let stop = AtomicBool::new(false);
        let s = run_loop(&cfg, &client, &mut r, &stop, Some(3)).await;
        assert_eq!(s.ticks, 3);
        assert_eq!(s.fail, 3, "503 = 服务端未就绪 → 视同无新帧");
        assert_eq!(s.redraws, 1, "Init 态语义键不变 → 仅首拍重绘");
        assert_eq!(r.state().fail_streak(), 3);
    }

    /// 停止标志应尽快生效（SIGTERM 优雅退出路径）。
    #[tokio::test]
    async fn run_loop_respects_stop_flag() {
        use crate::channel::DisplayChannelClient;
        let url = spawn_503_stub().await;
        let cfg = CliConfig {
            channel: url,
            interval_ms: 50,
            ..Default::default()
        };
        let client = DisplayChannelClient::try_new(&cfg.channel).unwrap();
        let mut r = Renderer::new(kit(), OffscreenCanvas::new(320, 240), 2000);
        let stop = AtomicBool::new(true); // 起手即停
        let s = run_loop(&cfg, &client, &mut r, &stop, None).await;
        assert_eq!(s.ticks, 0);
    }

    #[test]
    fn drm_backend_error_is_explicit() {
        let msg = drm_unimplemented_error().to_string();
        assert!(msg.contains("drm"));
        assert!(msg.contains("fbdev"), "应给出可用替代，不静默：{msg}");
    }
}
