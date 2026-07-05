# 第三轮代码审查 — O24 REFUTED 记录

**日期**: 2026-07-06

**审查结论**: 第三轮共 25 个发现，24 个已修复，1 个确认误判。

---

## O24: startup.rs `tokio::fs::File::create` 冗余 — **REFUTED**

| 项目 | 内容 |
|------|------|
| **审查员声称** | SQLite 初始化时会自动创建不存在的数据库文件，`File::create` 是冗余操作 |
| **引入提交** | `f859e58` (2026-07-05) — "启动时自动创建 SQLite 数据库文件" |
| **引入原因** | 目标板 (NanoPC-T6-LTS, RK3588, Ubuntu 22.04) 上 `mupcd` 启动失败 `[0x0005] unable to open database file` |
| **根因** | `strace` 确认 sqlx 的 `openat` 调用缺少 `O_CREAT` 标志，仅尝试 `O_RDWR` 和 `O_RDONLY`，文件不存在时返回 `ENOENT` |
| **strace 证据** | `openat(AT_FDCWD, "/opt/mupc/data/mupc.db", O_RDWR\|O_NOFOLLOW\|O_CLOEXEC) = -1 ENOENT` |
| **代码意图** | 在 `init_pool` 前预创建空文件，sqlx 将其作为有效 SQLite 库初始化 |
| **状态** | **保留。sqlx 的自动创建行为非跨平台保证，此防御性代码必要。** |

---

**记录人**: Claude
**日期**: 2026-07-06
