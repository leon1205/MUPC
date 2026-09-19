//! OTA 断点续传下载器
//!
//! Phase 3C.2 OTA 模块断点续传下载器
//! 支持 HTTP Range 请求、重试机制、SHA-256 校验

use std::path::{Path, PathBuf};
use std::sync::Arc;

use reqwest::Client;
use sha2::{Digest, Sha256};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::time::{sleep, Duration};

use super::error::OtaError;

/// 下载进度回调类型
pub type ProgressCallback = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// 下载结果
#[derive(Debug, Clone)]
pub struct DownloadResult {
    /// 下载文件路径
    pub path: PathBuf,
    /// 文件大小
    pub size: u64,
    /// SHA-256 哈希值
    pub hash: String,
}

/// 断点续传下载器
#[derive(Debug)]
pub struct Downloader {
    /// HTTP 客户端
    client: Client,
    /// 临时文件目录
    temp_dir: PathBuf,
    /// 最大重试次数
    max_retries: u32,
    /// 重试间隔（毫秒）
    retry_interval_ms: u64,
}

/// 从 URL 推导临时文件名（**保证只含单一文件名，不含任何目录成分**）。
///
/// - 查询参数 `file=<名>` **优先** —— 形如 `…/download?file=model.rknn&version=1` 的地址，
///   真正的文件名在查询串里，取路径末段只会得到 `download`；
/// - 否则取路径末段（已剥掉查询串）；
/// - 都取不到 / 都不安全 ⇒ `download.tmp`（如 `https://host/`）。
///
/// ⚠️ **净化是必需的**（2026-09-20 评审修复）：候选名含路径分隔符、NUL、`..`/`.` 或为空即弃用。
/// 否则服务器可控的 `?file=a/b.rknn` 会把名字拼成 `ota_<hash>_a/b.rknn` ⇒ 父目录不存在、
/// `create_new` 恒 ENOENT、该 URL **永远下载不了**（旧实现取路径末段，天然不含分隔符）。
fn extract_filename(url: &str) -> &str {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));

    let from_query = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("file=").filter(|n| is_safe_filename(n)));

    from_query
        .or_else(|| path.rsplit('/').next().filter(|n| is_safe_filename(n)))
        .unwrap_or("download.tmp")
}

/// 只接受「单一文件名」：非空、无路径分隔符/NUL、且不是 `.` / `..`。
fn is_safe_filename(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

impl Downloader {
    /// 创建新的下载器
    pub fn new(temp_dir: PathBuf) -> Result<Self, OtaError> {
        // ⚠️ 本构造函数**不做任何文件系统操作**（2026-09-20 评审回退）：此前的
        // `create_dir_all` 会让 `Downloader::new` 失败，而它在 `OtaManagerImpl::new`
        // 里被 `?` 上抛、最终令 mupcd **启动即退出**（startup.rs → process::exit(1)）——
        // 一个"临时目录建不出来"的局部问题不该变成整机起不来。目录改在真正下载时确保
        // （见 `download_with_progress`），失败也只影响这一次下载。
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|e| OtaError::NetworkError(e.to_string()))?;

        Ok(Self {
            client,
            temp_dir,
            max_retries: 3,
            retry_interval_ms: 1000,
        })
    }

    /// 设置最大重试次数
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// 设置重试间隔
    pub fn with_retry_interval_ms(mut self, interval_ms: u64) -> Self {
        self.retry_interval_ms = interval_ms;
        self
    }

    /// 下载文件（支持断点续传）
    ///
    /// # 参数
    /// * `url` - 下载 URL
    /// * `expected_hash` - 期望的 SHA-256 哈希值
    /// * `resume_from` - 从指定偏移量继续下载（字节）
    ///
    /// # 返回
    /// 下载结果包含文件路径、大小和哈希值
    pub async fn download(
        &self,
        url: &str,
        expected_hash: &str,
        resume_from: u64,
    ) -> Result<DownloadResult, OtaError> {
        self.download_with_progress(url, expected_hash, resume_from, None)
            .await
    }

    /// 下载文件（带进度回调，支持断点续传）
    ///
    /// # 参数
    /// * `url` - 下载 URL
    /// * `expected_hash` - 期望的 SHA-256 哈希值
    /// * `resume_from` - 从指定偏移量继续下载（字节）
    /// * `progress` - 进度回调函数 (已下载字节数, 总字节数)
    pub async fn download_with_progress(
        &self,
        url: &str,
        expected_hash: &str,
        resume_from: u64,
        progress: Option<ProgressCallback>,
    ) -> Result<DownloadResult, OtaError> {
        // 确保临时目录存在（失败只影响本次下载；**不**放在构造函数里，见 `new`）
        tokio::fs::create_dir_all(&self.temp_dir)
            .await
            .map_err(|e| {
                OtaError::DownloadFailed(format!(
                    "临时目录不可用 {}: {}",
                    self.temp_dir.display(),
                    e
                ))
            })?;

        // 生成临时文件路径
        let temp_path = self.generate_temp_path(url);

        // `resume_from == 0` = 调用方**不要续传**（`manager.rs` 传 0 并注明"不支持断点续传"）
        // ⇒ 必须清掉同名陈旧文件：否则 (1) start_offset 会等于旧文件长度 ⇒ `Range: bytes=<全长>-`
        // 被服务端判 416；(2) 即便偏移为 0，`do_download` 用 `create_new(true)` 打开 ⇒
        // 文件已存在时 EEXIST，同一 URL **再也下载不了**（2026-09-20 评审修复）。
        let start_offset = if resume_from > 0 {
            resume_from
        } else {
            if temp_path.exists() {
                tokio::fs::remove_file(&temp_path).await.map_err(|e| {
                    OtaError::DownloadFailed(format!(
                        "清理陈旧临时文件失败 {}: {}",
                        temp_path.display(),
                        e
                    ))
                })?;
            }
            0
        };

        // 执行下载（带重试机制）
        let (total_size, _) = self
            .download_with_retry(url, &temp_path, start_offset, progress)
            .await?;

        // 校验文件完整性
        let hash = compute_file_hash(&temp_path).await?;

        if hash != expected_hash {
            // 删除损坏的文件
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(OtaError::VerificationFailed(format!(
                "SHA-256 校验失败: 期望 {} 实际 {}",
                expected_hash, hash
            )));
        }

        Ok(DownloadResult {
            path: temp_path,
            size: total_size,
            hash,
        })
    }

    /// 带重试机制的执行下载
    async fn download_with_retry(
        &self,
        url: &str,
        temp_path: &PathBuf,
        start_offset: u64,
        progress: Option<ProgressCallback>,
    ) -> Result<(u64, String), OtaError> {
        let mut last_error = None;
        let mut current_offset = start_offset;

        for attempt in 0..self.max_retries {
            match self
                .do_download(url, temp_path, current_offset, progress.clone())
                .await
            {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    // 更新已下载大小用于断点续传
                    current_offset = self.get_downloaded_size(temp_path).await;
                    if current_offset > 0 {
                        tracing::warn!(
                            "下载尝试 {} 失败，已下载 {} 字节，将从此处继续",
                            attempt + 1,
                            current_offset
                        );
                    }

                    // 不是最后一次尝试则等待后重试
                    if attempt < self.max_retries - 1 {
                        sleep(Duration::from_millis(self.retry_interval_ms)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| OtaError::DownloadFailed("重试次数耗尽".to_string())))
    }

    /// 执行实际下载
    async fn do_download(
        &self,
        url: &str,
        temp_path: &PathBuf,
        start_offset: u64,
        progress: Option<ProgressCallback>,
    ) -> Result<(u64, String), OtaError> {
        // 构建 Range 请求
        let mut request = self.client.get(url);
        if start_offset > 0 {
            request = request.header("Range", format!("bytes={}-", start_offset));
        }

        let response = request
            .send()
            .await
            .map_err(|e| OtaError::NetworkError(format!("HTTP 请求失败: {}", e)))?;

        // 检查响应状态
        let status = response.status();
        if !status.is_success() && status.as_u16() != 206 {
            return Err(OtaError::DownloadFailed(format!(
                "HTTP 状态码: {}",
                status.as_u16()
            )));
        }

        // 获取 Content-Length
        let total_size = response.content_length().unwrap_or(0) + start_offset;

        // 获取响应体
        let bytes = response
            .bytes()
            .await
            .map_err(|e| OtaError::NetworkError(format!("读取响应体失败: {}", e)))?;

        let downloaded_size = bytes.len() as u64;

        // 如果是 Range 请求且从 0 开始，创建新文件；否则追加写入
        let file = if start_offset == 0 {
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(temp_path)
                .await
        } else {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(temp_path)
                .await
        }
        .map_err(|e| OtaError::DownloadFailed(format!("创建临时文件失败: {}", e)))?;

        let mut writer = file;
        writer
            .write_all(&bytes)
            .await
            .map_err(|e| OtaError::DownloadFailed(format!("写入文件失败: {}", e)))?;

        // 计算哈希值（从 start_offset 开始的所有数据）
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let hash = format!("{:x}", hasher.finalize());

        // 更新进度回调
        if let Some(callback) = progress {
            callback(start_offset + downloaded_size, total_size);
        }

        Ok((start_offset + downloaded_size, hash))
    }

    /// 生成临时文件路径
    fn generate_temp_path(&self, url: &str) -> PathBuf {
        let filename = extract_filename(url);

        // 生成带哈希前缀的唯一文件名（`ota_` 前缀 = 本仓临时产物的命名约定）
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let url_hash = &format!("{:x}", hasher.finalize())[..16];

        self.temp_dir.join(format!("ota_{}_{}", url_hash, filename))
    }

    /// 获取已下载的大小
    pub async fn get_downloaded_size(&self, temp_path: &PathBuf) -> u64 {
        tokio::fs::metadata(temp_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// 取消下载（清理临时文件）
    pub async fn cancel(&self, temp_path: &Path) -> Result<(), OtaError> {
        if temp_path.exists() {
            tokio::fs::remove_file(temp_path)
                .await
                .map_err(|e| OtaError::DownloadFailed(format!("删除临时文件失败: {}", e)))?;
        }
        Ok(())
    }
}

/// 计算文件的 SHA-256 哈希值
pub async fn compute_file_hash(path: &PathBuf) -> Result<String, OtaError> {
    let contents = tokio::fs::read(path)
        .await
        .map_err(|e| OtaError::DownloadFailed(format!("读取文件失败: {}", e)))?;

    let mut hasher = Sha256::new();
    hasher.update(&contents);
    Ok(format!("{:x}", hasher.finalize()))
}

// ============================================================================
// 单元测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ========== Downloader 创建测试 ==========

    #[test]
    fn test_downloader_new() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let downloader = Downloader::new(temp_dir.clone()).unwrap();

        assert_eq!(downloader.temp_dir, temp_dir);
        assert_eq!(downloader.max_retries, 3);
        assert_eq!(downloader.retry_interval_ms, 1000);
    }

    #[test]
    fn test_downloader_with_options() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let downloader = Downloader::new(temp_dir)
            .unwrap()
            .with_max_retries(5)
            .with_retry_interval_ms(2000);

        assert_eq!(downloader.max_retries, 5);
        assert_eq!(downloader.retry_interval_ms, 2000);
    }

    /// 回归保护（2026-09-20 评审）：构造函数**不得**触碰文件系统 —— 早先版本在 `new` 里
    /// `create_dir_all` 并在失败时返回 Err，会让 `OtaManagerImpl::new` 上抛、令 mupcd
    /// **启动即退出**；且用例 `Downloader::new("/nonexistent/...")` 的断言在 root 下会翻转
    /// （路径真能建出来）。此处改为**确定性判据**：以「普通文件」充当父目录 ⇒ `create_dir_all`
    /// 必然 ENOTDIR（与 uid 无关），失败只落在**下载**这一步。
    #[tokio::test]
    async fn download_into_unusable_temp_dir_fails_without_touching_ctor() {
        let dir = TempDir::new().unwrap();
        let blocker = dir.path().join("blocker_file");
        std::fs::write(&blocker, b"x").unwrap();
        let unusable = blocker.join("sub"); // 父路径是文件 ⇒ 建目录必失败

        let downloader = Downloader::new(unusable).expect("构造函数不应依赖文件系统");
        let err = downloader
            .download_with_progress("https://example.invalid/model.rknn", "deadbeef", 0, None)
            .await
            .expect_err("临时目录不可用时的下载必须失败");
        assert!(
            err.to_string().contains("临时目录不可用"),
            "错误应明确指向临时目录，实际: {err}"
        );
    }

    /// `?file=` 含路径分隔符/`..`/空 ⇒ 必须弃用并回退（净化），否则临时路径会指向不存在的
    /// 父目录（`ota_<hash>_a/b.rknn`）使该 URL 永远下载不了。
    #[test]
    fn test_generate_temp_path_rejects_unsafe_file_param() {
        let dir = TempDir::new().unwrap();
        let downloader = Downloader::new(dir.path().to_path_buf()).unwrap();

        let p =
            downloader.generate_temp_path("https://ota.example.com/dl/real.rknn?file=a/b.rknn&v=2");
        assert!(p.to_str().unwrap().ends_with("real.rknn"), "实际 {p:?}");

        let p2 = downloader.generate_temp_path("https://ota.example.com/dl/real2.rknn?file=..");
        assert!(p2.to_str().unwrap().ends_with("real2.rknn"), "实际 {p2:?}");

        let p3 = downloader.generate_temp_path("https://ota.example.com/?file=..");
        assert!(
            p3.to_str().unwrap().ends_with("download.tmp"),
            "实际 {p3:?}"
        );

        // 反向保护：安全的 file= 仍照常生效（净化不得误伤原有行为）
        let p4 =
            downloader.generate_temp_path("https://ota.example.com/download?file=model.rknn&v=1");
        assert!(p4.to_str().unwrap().ends_with("model.rknn"), "实际 {p4:?}");
    }

    /// 极简 HTTP 桩：对 `serve_count` 个连接各回一份 `body`（`Connection: close`）。
    /// 返回 `(url, 服务线程句柄)`；不引入任何新依赖。
    fn spawn_stub_http(body: Vec<u8>, serve_count: usize) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for _ in 0..serve_count {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf); // 只读请求头即可
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        (format!("http://{addr}/model.rknn"), handle)
    }

    /// 回归保护：**同一 URL 必须能重复下载**。早先实现会在成功后留下临时文件，而
    /// `do_download` 用 `create_new(true)` 打开 ⇒ 第二次必然 EEXIST（或按陈旧长度发 Range 被
    /// 判 416），该 URL 从此不可下载（2026-09-20 评审修复）。
    #[tokio::test]
    async fn same_url_is_downloadable_twice() {
        use sha2::Sha256;
        let body = b"model-bytes-for-regression".to_vec();
        let hash = format!("{:x}", Sha256::digest(&body));
        let (url, server) = spawn_stub_http(body.clone(), 2);

        let dir = TempDir::new().unwrap();
        let downloader = Downloader::new(dir.path().to_path_buf()).unwrap();

        let first = downloader
            .download_with_progress(&url, &hash, 0, None)
            .await
            .expect("第一次下载应成功");
        let second = downloader
            .download_with_progress(&url, &hash, 0, None)
            .await
            .expect("第二次下载同一 URL 也应成功（陈旧临时文件必须被清理）");

        assert_eq!(first.hash, hash);
        assert_eq!(second.hash, hash);
        assert_eq!(first.path, second.path);
        drop(server);
    }

    // ========== generate_temp_path 测试 ==========

    #[test]
    fn test_generate_temp_path() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let downloader = Downloader::new(temp_dir).unwrap();

        let url = "https://ota.example.com/models/lstm_v1.2.0.rknn";
        let path = downloader.generate_temp_path(url);

        assert!(path.to_str().unwrap().contains("lstm_v1.2.0.rknn"));
        assert!(path.to_str().unwrap().contains("ota_"));
    }

    #[test]
    fn test_generate_temp_path_with_query() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let downloader = Downloader::new(temp_dir).unwrap();

        let url = "https://ota.example.com/download?file=model.rknn&version=1";
        let path = downloader.generate_temp_path(url);

        // 查询参数应该被去除
        assert!(path.to_str().unwrap().contains("model.rknn"));
        assert!(!path.to_str().unwrap().contains("?"));
        assert!(!path.to_str().unwrap().contains("file="));
    }

    #[test]
    fn test_generate_temp_path_no_filename() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let downloader = Downloader::new(temp_dir).unwrap();

        let url = "https://ota.example.com/";
        let path = downloader.generate_temp_path(url);

        assert!(path.to_str().unwrap().ends_with("download.tmp"));
    }

    // ========== get_downloaded_size 测试 ==========

    #[tokio::test]
    async fn test_get_downloaded_size_nonexistent() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let downloader = Downloader::new(temp_dir).unwrap();

        let size = downloader
            .get_downloaded_size(&PathBuf::from("/nonexistent/file.tmp"))
            .await;
        assert_eq!(size, 0);
    }

    // ========== cancel 测试 ==========

    #[tokio::test]
    async fn test_cancel_nonexistent() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let downloader = Downloader::new(temp_dir).unwrap();

        // 取消不存在的文件不应该出错
        let result = downloader
            .cancel(&PathBuf::from("/nonexistent/file.tmp"))
            .await;
        assert!(result.is_ok());
    }

    // ========== compute_file_hash 测试 ==========

    #[tokio::test]
    async fn test_compute_file_hash() {
        let temp_dir = TempDir::new().unwrap().into_path();
        let file_path = temp_dir.join("test_file.txt");

        tokio::fs::write(&file_path, b"hello world").await.unwrap();

        let hash = compute_file_hash(&file_path).await.unwrap();

        // "hello world" 的 SHA-256 哈希值
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[tokio::test]
    async fn test_compute_file_hash_nonexistent() {
        let result = compute_file_hash(&PathBuf::from("/nonexistent/file.txt")).await;
        assert!(result.is_err());
    }

    // ========== DownloadResult 测试 ==========

    #[test]
    fn test_download_result_debug() {
        let result = DownloadResult {
            path: PathBuf::from("/tmp/test.rknn"),
            size: 1024,
            hash: "abc123".to_string(),
        };

        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("test.rknn"));
        assert!(debug_str.contains("1024"));
        assert!(debug_str.contains("abc123"));
    }

    // ========== ProgressCallback 测试 ==========

    #[test]
    fn test_progress_callback() {
        let received = Arc::new(std::sync::Mutex::new((0u64, 0u64)));
        let received_clone = received.clone();
        let callback: ProgressCallback = Arc::new(move |downloaded, total| {
            *received_clone.lock().unwrap() = (downloaded, total);
        });
        callback(100, 1000);
        assert_eq!(*received.lock().unwrap(), (100, 1000));
    }

    // ========== SHA-256 测试 ==========

    #[test]
    fn test_sha256_hash_consistency() {
        use sha2::{Digest, Sha256};

        let data = b"test data for hash";
        let mut hasher1 = Sha256::new();
        hasher1.update(data);
        let hash1 = format!("{:x}", hasher1.finalize());

        let mut hasher2 = Sha256::new();
        hasher2.update(data);
        let hash2 = format!("{:x}", hasher2.finalize());

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_sha256_empty_data() {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(b"");
        let hash = format!("{:x}", hasher.finalize());

        // 空数据的 SHA-256 哈希值
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
