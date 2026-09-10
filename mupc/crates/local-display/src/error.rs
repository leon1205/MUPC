//! local-display 渲染端统一错误类型（通道拉帧 / 后端打开 / 字体加载）。
//!
//! 与 display-proto::error 分开：本 crate 的错误面是「渲染侧基础设施」（HTTP 拉帧、URL 解析、
//! 后端设备打开、字体加载），不是「帧 JSON 编解码」（后者归 display-proto）。

use thiserror::Error;

/// 渲染端统一结果类型。
pub type Result<T> = std::result::Result<T, Error>;

/// 渲染端错误。
#[derive(Debug, Error)]
pub enum Error {
    /// 通道 URL 非法（仅支持 `http://` 回环，无 TLS）。
    #[error("invalid channel url `{0}`: {1}")]
    BadUrl(String, String),

    /// TCP 连接失败（mupcd 未起 / 端口不存在 → 通道断）。
    #[error("channel connect to `{0}` failed: {1}")]
    Connect(String, std::io::Error),

    /// GET 请求/读取失败。
    #[error("channel io error on `{0}`: {1}")]
    Io(String, std::io::Error),

    /// 拉帧超时（设计 §5.3：单次 GET 2s 超时，失败记一次由上层判通道断）。
    #[error("channel get `{0}` timed out")]
    Timeout(String),

    /// 非 200 响应（503 = 服务端尚未就绪，视同无新帧重试）。
    #[error("channel http status {0} from `{1}`")]
    HttpStatus(u16, String),

    /// 响应体 JSON 解码失败（协议不匹配）。
    #[error("channel body decode error from `{0}`: {1}")]
    Json(String, serde_json::Error),

    /// 后端设备（framebuffer /dev/fb0）打开失败。
    #[error("backend open error: {0}")]
    Backend(String),

    /// 字体加载失败（缺文件 / 非法字体）——调用方应回退「无字形」，不 panic。
    #[error("font load error: {0}")]
    Font(String),
}
