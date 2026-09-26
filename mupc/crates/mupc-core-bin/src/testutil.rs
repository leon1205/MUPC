//! **仅测试可见**的一次性临时目录（RAII 清理）。
//!
//! 为什么自己写而不是引 `tempfile`：本单元**不新增依赖**（任务书 §四.8），而"造一个临时目录、
//! 用完删掉"用 `std::env::temp_dir()` + 计数器已足够。目录名带**进程号 + 单调计数器 + 纳秒**
//! 三重区分：单进程内多用例并行、以及同一台机上多份 `cargo test` 并发都不互撞。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 临时目录（`Drop` 时尽力递归删除；删不掉不 panic——它落在系统临时目录里）。
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// 建一个空目录。
    pub fn new(tag: &str) -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("mupc-g2-{tag}-{}-{n}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).expect("建临时目录");
        Self { path }
    }

    /// 目录路径。
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 目录路径下的文件。
    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// 写入一个文件（建父目录）。
    pub fn write(&self, name: &str, text: &str) -> PathBuf {
        let p = self.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("建父目录");
        }
        std::fs::write(&p, text).expect("写文件");
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
