//! 通道客户端：`DisplayChannelClient`——对 mupcd 回环发布端点做最小 HTTP/1.1 GET。
//!
//! 对齐 `[DESIGN_APPROVED]` 设计 §3.1/§5.3：
//! - 形态：TCP 回环 `127.0.0.1:<port>`，`GET /v1/display/latest` → 200 + JSON 帧。
//! - 语义：每次取到即最新有效帧；单次 GET 超时 2s；失败返回 `Err` 由上层（state/run）判通道断。
//! - 实现：**裸 tokio TcpStream + 手写 HTTP/1.1**（无 reqwest/ureq 重依赖）。请求头带
//!   `Connection: close`；响应解析先读头到 `\r\n\r\n`，有 `Content-Length` 按长度精确读，
//!   否则读到 EOF 作为 body——对我们的 LoopbackHttpPublisher / 测试桩（均回 Content-Length）
//!   语义正确且可 mock。
//! - 无 TLS（仅回环，设计 D1：本地可信、不暴露外网）。

use std::time::Duration;

use crate::error::{Error, Result};
use mupc_display_proto::DisplayFrame;

/// 单次 GET 超时（设计 §5.3：失败记一次，由上层按「连续失败 / 无成功 >3s」判通道断）。
pub const GET_TIMEOUT: Duration = Duration::from_secs(2);

/// 响应头读取上限（64 KiB）。
pub const MAX_HEAD_BYTES: usize = 64 * 1024;

/// 响应体读取上限（W2：与头对齐 64 KiB）。标称帧 JSON 约数百字节，余量充分；
/// 超限即 `Err`（计入失败），杜绝畸形/异常对端用巨额 `Content-Length` 触发内存失控
/// （PRD 4.4.3：渲染端不得因通道对端行为异常而崩溃或内存失控）。
pub const MAX_BODY_BYTES: usize = 64 * 1024;

/// 已解析的回环通道端点（`http://host:port/path`；host 为字面量，用于直连）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelEndpoint {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl ChannelEndpoint {
    /// 解析通道 URL。仅支持 `http://`（无 TLS）；host 不做 DNS（回环直连字面量）；
    /// 端口缺省 80；路径缺省 `/`。
    pub fn parse(url: &str) -> Result<Self> {
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| Error::BadUrl(url.into(), "仅支持 http://（回环无 TLS）".into()))?;
        if let Some((host_port, path)) = rest.split_once('/') {
            let mut endpoint = Self::parse_host_port(url, host_port)?;
            endpoint.path = format!("/{path}");
            Ok(endpoint)
        } else {
            let mut endpoint = Self::parse_host_port(url, rest)?;
            endpoint.path = "/".to_string();
            Ok(endpoint)
        }
    }

    fn parse_host_port(url: &str, host_port: &str) -> Result<Self> {
        let (host, port) = match host_port.rsplit_once(':') {
            Some((h, p)) => {
                let port: u16 = p
                    .parse()
                    .map_err(|_| Error::BadUrl(url.into(), format!("非法端口 `{p}`")))?;
                (h.to_string(), port)
            }
            None => (host_port.to_string(), 80),
        };
        if host.is_empty() {
            return Err(Error::BadUrl(url.into(), "host 为空".into()));
        }
        Ok(Self {
            host,
            port,
            path: String::new(),
        })
    }

    /// 组装请求行 Host 头（回环字面量）。
    fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// 回环发布端点默认 URL（display-proto 共享常量）。
pub const DEFAULT_CHANNEL_URL: &str = mupc_display_proto::DEFAULT_CHANNEL_URL;

/// 渲染侧通道客户端。无连接态持有——每轮 `fetch_latest` 新建连接（设计 §3.1：服务端不依赖保活）。
#[derive(Debug, Clone)]
pub struct DisplayChannelClient {
    endpoint: ChannelEndpoint,
    /// 单次 GET 超时（默认 `GET_TIMEOUT`；可注入便于测试）。
    timeout: Duration,
}

impl Default for DisplayChannelClient {
    fn default() -> Self {
        Self::new(DEFAULT_CHANNEL_URL)
    }
}

impl DisplayChannelClient {
    pub fn new(url: &str) -> Self {
        Self {
            endpoint: ChannelEndpoint::parse(url)
                .unwrap_or_else(|_| ChannelEndpoint::parse(DEFAULT_CHANNEL_URL).expect("default url")),
            timeout: GET_TIMEOUT,
        }
    }

    /// 解析失败时保留错误可见的构造（供 CLI 层给用户明确报错）。成功返回 Ok(Client)。
    pub fn try_new(url: &str) -> Result<Self> {
        Ok(Self {
            endpoint: ChannelEndpoint::parse(url)?,
            timeout: GET_TIMEOUT,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_timeout(url: &str, timeout: Duration) -> Result<Self> {
        Ok(Self {
            endpoint: ChannelEndpoint::parse(url)?,
            timeout,
        })
    }

    pub fn endpoint(&self) -> &ChannelEndpoint {
        &self.endpoint
    }

    /// 拉取最新帧。非 200（含 503=尚未就绪）返回 `Err`，上层视同本轮无新帧。
    pub async fn fetch_latest(&self) -> Result<DisplayFrame> {
        let authority = self.endpoint.authority();
        let addr = format!("{}:{}", self.endpoint.host, self.endpoint.port);
        let path = self.endpoint.path.clone();

        let stream = tokio::time::timeout(self.timeout, tokio::net::TcpStream::connect(&addr))
            .await
            .map_err(|_| Error::Timeout(addr.clone()))?
            .map_err(|e| Error::Connect(addr.clone(), e))?;

        let request =
            format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
        let op_addr = addr.clone();
        let op = async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let io = |e: std::io::Error| Error::Io(op_addr.clone(), e);
            let mut stream = stream;
            stream.write_all(request.as_bytes()).await.map_err(io)?;
            // 读响应头直到 \r\n\r\n
            let mut head_buf = Vec::new();
            let mut byte = [0u8; 1];
            loop {
                stream.read_exact(&mut byte).await.map_err(io)?;
                head_buf.push(byte[0]);
                if head_buf.ends_with(b"\r\n\r\n") {
                    break;
                }
                if head_buf.len() > MAX_HEAD_BYTES {
                    return Err(io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "response header too large",
                    )));
                }
            }
            let head = String::from_utf8_lossy(&head_buf).into_owned();
            let (status_line, headers) = head.split_once("\r\n").unwrap_or((&head, ""));
            let status: u16 = status_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if status != 200 {
                return Err(Error::HttpStatus(status, op_addr));
            }
            // Content-Length → 精确读；否则读到 EOF 作 body（对自控桩成立）。
            let content_length = headers
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    (k.trim().eq_ignore_ascii_case("content-length"))
                        .then(|| v.trim().parse::<usize>().ok())
                        .flatten()
                });
            let mut body = Vec::new();
            match content_length {
                Some(n) => {
                    // W2：先校验上限再预分配——畸形对端宣称巨额长度时直接拒绝（不 resize）。
                    if n > MAX_BODY_BYTES {
                        return Err(Error::BodyTooLarge(op_addr.clone(), n));
                    }
                    body.resize(n, 0);
                    stream.read_exact(&mut body).await.map_err(io)?;
                }
                None => {
                    // 无 Content-Length → 读到 EOF，但同样按上限封顶（防无限流撑爆内存）。
                    let mut limited = (&mut stream).take(MAX_BODY_BYTES as u64 + 1);
                    limited.read_to_end(&mut body).await.map_err(io)?;
                    if body.len() > MAX_BODY_BYTES {
                        return Err(Error::BodyTooLarge(op_addr.clone(), body.len()));
                    }
                }
            }
            let frame: DisplayFrame = serde_json::from_slice(&body)
                .map_err(|e| Error::Json(op_addr.clone(), e))?;
            // W3：版本一致性校验（设计 §3.3/PRD 4.4.1）——不符即明确错误（计入失败），
            // 绝不静默按旧语义展示。
            if frame.version != mupc_display_proto::PROTO_VERSION {
                return Err(Error::ProtoVersion(
                    op_addr.clone(),
                    frame.version,
                    mupc_display_proto::PROTO_VERSION,
                ));
            }
            Ok(frame)
        };
        tokio::time::timeout(self.timeout, op)
            .await
            .map_err(|_| Error::Timeout(addr.clone()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_parse_full_url() {
        let e = ChannelEndpoint::parse("http://127.0.0.1:9810/v1/display/latest").unwrap();
        assert_eq!(e.host, "127.0.0.1");
        assert_eq!(e.port, 9810);
        assert_eq!(e.path, "/v1/display/latest");
    }

    #[test]
    fn endpoint_parse_defaults() {
        let e = ChannelEndpoint::parse("http://localhost/v1").unwrap();
        assert_eq!(e.host, "localhost");
        assert_eq!(e.port, 80);
        assert_eq!(e.path, "/v1");
    }

    #[test]
    fn endpoint_parse_rejects_non_http() {
        assert!(ChannelEndpoint::parse("https://127.0.0.1:1/x").is_err());
        assert!(ChannelEndpoint::parse("unix:///tmp/x").is_err());
    }

    #[test]
    fn client_default_points_at_loopback_publisher() {
        let c = DisplayChannelClient::default();
        assert_eq!(c.endpoint().host, "127.0.0.1");
        assert_eq!(c.endpoint().port, 9810);
        assert_eq!(c.endpoint().path, "/v1/display/latest");
    }

    // ---- mock HTTP 服务器：起本地端口 → 拉帧解析 ----
    /// 起一个最小 HTTP 桩：对任意 GET 返回 200 + JSON 帧（Content-Length 定长）。
    async fn spawn_stub(body: String) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{}/v1/display/latest", addr);
        let (body_bytes, status) = (body.into_bytes(), 200);
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = match listener.accept().await {
                    Ok(x) => x,
                    Err(_) => break,
                };
                let body = body_bytes.clone();
                let status = status;
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    // 读到请求头结束（GET 无 body），避免与服务端 read 相互等待。
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
                    let resp = format!(
                        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                });
            }
        });
        url
    }

    fn sample_frame_json() -> String {
        // v2 契约：`version` 必须等于 `PROTO_VERSION`（=2），否则帧被拒（设计 §3.5 条 1）。
        r#"{"version":2,"seq":7,"ts_ms":1700000000000,"soc":65.0,"soc_source":"pcs_reg1010",
            "soc_flag":"valid","run_state":2,"pcs_online":true,
            "p_phase":[{"v":12.3,"flag":"valid"},{"v":11.8,"flag":"valid"},{"v":12.0,"flag":"valid"}],
            "p_total":{"v":36.1,"flag":"valid"},
            "i_phase":[{"v":22.5,"flag":"valid"},{"v":22.1,"flag":"valid"},{"v":22.3,"flag":"valid"}],
            "inconsistency":false}"#
            .to_string()
    }

    #[tokio::test]
    async fn fetch_latest_parses_frame_from_stub() {
        let url = spawn_stub(sample_frame_json()).await;
        let c = DisplayChannelClient::with_timeout(&url, Duration::from_secs(3)).unwrap();
        let f = c.fetch_latest().await.unwrap();
        assert_eq!(f.seq, 7);
        assert_eq!(f.soc, Some(65.0));
        assert_eq!(f.run_state, Some(mupc_display_proto::RunState::Charge));
        assert_eq!(f.p_phase[0].v, Some(12.3));
    }

    #[tokio::test]
    async fn fetch_latest_503_is_error() {
        // 503 = 服务端未就绪 → Err（上层视同无新帧重试）
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/display/latest", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut req = Vec::new();
            let mut one = [0u8; 1];
            loop {
                if sock.read_exact(&mut one).await.is_err() {
                    return;
                }
                req.push(one[0]);
                if req.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let resp = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = sock.write_all(resp.as_bytes()).await;
        });
        let c = DisplayChannelClient::with_timeout(&url, Duration::from_secs(3)).unwrap();
        let err = c.fetch_latest().await.unwrap_err();
        assert!(matches!(err, Error::HttpStatus(503, _)));
    }

    #[tokio::test]
    async fn fetch_latest_connection_refused_is_connect_error() {
        // 取一个「刚被释放」的本地端口（bind→drop）→ 该端口确定无监听，连接被立即拒绝。
        // 不用固定端口 1：Windows 上对 1 号端口的连接会被静默丢弃（表现为超时而非拒绝），
        // 断言不稳定；bind-then-drop 在各平台都确定得到 ECONNREFUSED。
        let port = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        let url = format!("http://127.0.0.1:{port}/v1/display/latest");
        // 注意：本机（Windows）对无监听端口的 REFUSED 回包约需 2s（SYN 重试），若用默认 2s
        // 超时会把 Connect 掩盖成 Timeout。此处给 5s 让「被拒绝」这一路真正走到 Connect 分支。
        // 语义上两者都是 Err → 上层同样判通道断，故不影响运行时正确性。
        let c = DisplayChannelClient::with_timeout(&url, Duration::from_secs(5)).unwrap();
        let err = c.fetch_latest().await.unwrap_err();
        assert!(matches!(err, Error::Connect(_, _)), "got {err:?}");
    }

    #[tokio::test]
    async fn fetch_latest_no_response_times_out() {
        // 服务端 accept 后不回应 → 单次 GET 超时 → Err（上层记一次失败，判通道断靠无成功 >3s）。
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/display/latest", listener.local_addr().unwrap());
        tokio::spawn(async move {
            // 持有连接不回包，直到进程结束。
            let mut held = Vec::new();
            if let Ok((sock, _)) = listener.accept().await {
                held.push(sock);
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
            drop(held);
        });
        let c = DisplayChannelClient::with_timeout(&url, Duration::from_millis(300)).unwrap();
        let err = c.fetch_latest().await.unwrap_err();
        assert!(matches!(err, Error::Timeout(_)), "got {err:?}");
    }

    // ---- W2/W3：响应体上限 + 帧版本一致性 ----

    /// 起一个「自定义原始响应」桩：对任意 GET 回 `resp` 原文（可造畸形 Content-Length/版本）。
    async fn spawn_raw_stub(resp: String) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/display/latest", listener.local_addr().unwrap());
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
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
                let _ = sock.write_all(resp.as_bytes()).await;
            }
        });
        url
    }

    /// W2：`Content-Length` 超上限 → 明确 Err（BodyTooLarge），不做巨额预分配。
    #[tokio::test]
    async fn fetch_latest_rejects_oversized_content_length() {
        let huge = MAX_BODY_BYTES + 1;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {huge}\r\nConnection: close\r\n\r\n"
        );
        let url = spawn_raw_stub(resp).await;
        let c = DisplayChannelClient::with_timeout(&url, Duration::from_secs(3)).unwrap();
        let err = c.fetch_latest().await.unwrap_err();
        assert!(
            matches!(err, Error::BodyTooLarge(_, n) if n == huge),
            "超限 Content-Length 应 Err(BodyTooLarge)，实际: {err:?}"
        );
    }

    /// W2：无 `Content-Length` 时读到 EOF 亦按上限封顶（防无限流）。
    #[tokio::test]
    async fn fetch_latest_rejects_oversized_chunked_body() {
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
            "x".repeat(MAX_BODY_BYTES + 10)
        );
        let url = spawn_raw_stub(resp).await;
        let c = DisplayChannelClient::with_timeout(&url, Duration::from_secs(5)).unwrap();
        let err = c.fetch_latest().await.unwrap_err();
        assert!(
            matches!(err, Error::BodyTooLarge(_, _)),
            "无长度头的超长体应封顶拒绝，实际: {err:?}"
        );
    }

    /// W3：帧 `version` 与 `PROTO_VERSION` 不符 → Err(ProtoVersion)（不静默按旧语义展示）。
    #[tokio::test]
    async fn fetch_latest_rejects_proto_version_mismatch() {
        let body = sample_frame_json().replace("\"version\":2", "\"version\":99");
        let url = spawn_stub(body).await;
        let c = DisplayChannelClient::with_timeout(&url, Duration::from_secs(3)).unwrap();
        let err = c.fetch_latest().await.unwrap_err();
        assert!(
            matches!(err, Error::ProtoVersion(_, 99, _)),
            "版本不符应 Err(ProtoVersion)，实际: {err:?}"
        );
    }

    // ---- 通道错误 → 上层通道态（通道断）语义闭环 ----
    #[tokio::test]
    async fn all_transport_failures_surface_as_err_for_channel_down() {
        // Connect/Timeout/Io/HttpStatus/Json 任一 → Err，上层 `DisplayState::record_fail`
        // 累计失败并在无成功 >3s 时切 ChannelDown（UI §7.5）。此处校验「拒绝连接」这一路。
        let port = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        let c = DisplayChannelClient::with_timeout(
            &format!("http://127.0.0.1:{port}/x"),
            Duration::from_secs(5),
        )
        .unwrap();
        let mut st = crate::state::DisplayState::new();
        st.update(c.fetch_latest().await, 0);
        assert_eq!(st.fail_streak(), 1);
        assert_eq!(
            st.screen_mode(crate::state::CHANNEL_DOWN_MS),
            crate::state::ScreenMode::ChannelDown
        );
    }
}
