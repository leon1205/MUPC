# 本地显示终端渲染进程部署说明（12-MUPC Dev-B5）

> 权威依据：`docs/superpowers/plans/modules/12-MUPC-本地显示终端-设计文档.md`（`[DESIGN_APPROVED]`）
> §5/§7/§9/§11/§13 + PRD `12-MUPC-本地显示终端-PRD.md`。本文件是部署清单摘要，不重复设计。

## 1. 产物与落点

| 产物 | 落点 | 说明 |
|------|------|------|
| `mupc-local-display`（bin） | `/opt/mupc/bin/` | 渲染进程；纯 Rust、**零 C 依赖**（ab_glyph） |
| `mupc-display.service` | `/etc/systemd/system/` | 本目录 `systemd/` 下 |
| mupcd（发布侧） | `/opt/mupc/bin/mupcd` | `display.enabled=true` 时起 DisplayDataProvider + 回环 HTTP |

渲染进程**不装/不读** `mupc_core_config.yaml`（设计 §7.2）；通道端点一致性靠部署参数与
`display.bind_addr` 对齐（见 §3）。

## 2. 交叉编译（aarch64）

```bash
cd mupc
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
cargo build -p mupc-local-display --release --target aarch64-unknown-linux-gnu
# 产物：target/aarch64-unknown-linux-gnu/release/mupc-local-display
```

## 3. 部署步骤

```bash
# 1) 二进制
sudo cp mupc-local-display /opt/mupc/bin/ && sudo chmod 755 /opt/mupc/bin/mupc-local-display

# 2) mupcd 侧开启发布（core yaml）
#    display:
#      enabled: true
#      bind_addr: "127.0.0.1:9810"     # 必须与 unit 的 --channel 同端点
#      publish_ms: 1000
sudo systemctl restart mupcd

# 3) 渲染进程 unit
sudo cp mupc-display.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now mupc-display
systemctl status mupc-display
journalctl -u mupc-display -f
```

## 4. framebuffer / DRM 权限（真机校准，设计 §13 前置项 7）

- `--backend fbdev`（默认）需读写 `/dev/fb0`：通常属 `video` 组 →
  `sudo usermod -aG video mupc`（并保留 unit 的 `SupplementaryGroups`）。
- 若改用 DRM（本期**未实现**，`--backend drm` 会明确报错）需 `/dev/dri/*` 与 `render` 组。
- 快速自检：`sudo -u mupc /opt/mupc/bin/mupc-local-display --backend fbdev --width 1024 --height 768`
  （打不开设备时进程以非零码退出并打印原因，日志落 journal）。

## 5. 字库（设计 §5.4 / §13 前置项 8）

1. **捆绑子集（推荐，运行期零文件依赖）**：用字体子集化工具按 `crates/local-display/src/font.rs`
   顶部注释给出的命令（码表 = `REQ_TEXT` ∪ `REQ_ASCII`）生成
   `crates/local-display/fonts/NotoSansSC-subset.otf`，再以 `--features bundled-font` 编译。
2. **外部字库**：`--font /usr/share/fonts/.../xxx.otf` 覆盖（不入 core 配置）。
3. 两者皆无 → 中文显示**空心占位盒**、数字/拉丁走内置 ASCII 回退（不 panic，但屏面不可交付）。

## 6. 无真屏先行验证（设计 §10.1，部署前建议先跑）

```bash
# 本机/目标板均可：off 屏 + 回环通道（mupcd 未起时屏显「正在连接数据通道…」并可观察通道态）
mupc-local-display --backend offscreen --channel http://127.0.0.1:9810/v1/display/latest
```

## 7. 待真机验证（设计 §13，本机无法覆盖）

1. `/dev/fb0` 是否映射 HDMI、像素格式/位深/字节序（现按 32bpp XRGB 假定）；
2. 1025–1028 未命名寄存器能否整段读（现为两段 FC04 读）；
3. HDMI 分辨率协商（非 1024x768 需改 `--width/--height`）；
4. 屏面显示与 19200 波特总线吞吐余量（心跳 + 活读 + 显示两段读 + 下发写）。

## 8. 运维要点

- 崩溃自恢复：`Restart=always` + `RestartSec=1`（≤3s）；`systemctl stop` 走 SIGTERM 优雅退出并打印统计行。
- 页眉时钟为 **UTC**（未引时间库，KISS）；如需本地时间属后续小改（`localtime_r`，仅 Linux）。
