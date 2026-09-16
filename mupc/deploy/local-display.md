# 本地显示终端渲染进程部署说明（12-MUPC Dev-B5 / v2.0 B3-2a 订正）

> 权威依据：`docs/superpowers/plans/modules/12-MUPC-本地显示终端-设计文档.md`（`[DESIGN_APPROVED]`）
> §5/§7/§9/§11/§13 + PRD `12-MUPC-本地显示终端-PRD.md`。本文件是部署清单摘要，不重复设计。
>
> ⚠️ **v2.0 订正（B3-2a）**：渲染链路已由 v1.0 的「纯 Rust 自绘（ab_glyph + 自研循环）」
> 切换为 **LVGL v9**（`lvgl-sys` 编 C 源码）⇒ ① 产物**不再是"零 C 依赖"**（需 C 交叉工具链，
> 见 §2）；② 构建加 `--features noto-font`（字库，见 §5）；③ v1.0 的 `font.rs`/`layout.rs`/
> `run.rs` 与 `--features bundled-font` 已删除。

## 1. 产物与落点

| 产物 | 落点 | 说明 |
|------|------|------|
| `mupc-local-display`（bin） | `/opt/mupc/bin/` | 渲染进程；Rust + LVGL(C) 静态链入，**单一可执行产物** |
| `mupc-display.service` | `/etc/systemd/system/` | 本目录 `systemd/` 下 |
| mupcd（发布侧） | `/opt/mupc/bin/mupcd` | `display.enabled=true` 时起 DisplayDataProvider + 回环 HTTP |

渲染进程**不装/不读** `mupc_core_config.yaml`（设计 §7.2）；通道端点一致性靠部署参数与
`display.bind_addr` 对齐（见 §3）。**一键自检**：`mupc-local-display --backend offscreen
--channel <URL> --smoke`（渲染 6 页 → 打印逐页像素与时序 → 结论行给出 `result=`）。

自检**四条判定口**（任一不成立即返回码 3；结论行点名是哪一条，便于 CI 判读）：

| 结论 | 判据 |
|------|------|
| `FAIL_EMPTY_PAGE` | 六页中存在内容区为空的页（白屏 / 持有型句柄被级联删除的典型形态） |
| `FAIL_NO_THROTTLE` | `renders == ticks`：帧驱动页每拍都重渲染（500 ms 节拍退化成永远整屏重绘） |
| `FAIL_NO_FLUSH` | `blits == 0`：flush 计数没接到真闭包上（此时 `dropped == 0` 是假象） |
| `FAIL_DROPPED_FRAME` | `dropped > 0`：有脏区没画上（目标被借走 ⇒ 屏上留永久陈旧像素而 LVGL 不知道） |

## 2. 交叉编译（aarch64）

```bash
cd mupc
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
# LVGL 字库（10 档位图字体）由 lvgl-sys 的 build.rs 编入；未生成时应改用不带 noto-font 的构建
cargo build -p local-display --release --features noto-font \
  --target aarch64-unknown-linux-gnu
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

## 5. 字库（设计 §1.1.2 / §13 前置项 8）

> ⚠️ **v2.0（B3-2a）已改写本节**：渲染链路切到 LVGL，中文上屏由 `lv_font_conv` 生成的
> **位图字库**承担（`crates/local-display/fonts/gen_fonts.sh`）。v1.0 的 `font.rs`
> （ab_glyph 光栅化）与 `bundled-font` feature **已删除** —— 构建脚本若仍传
> `--features bundled-font` 会**响亮报错**（不是静默编出一个没字库的产物）。

1. **位图字库（唯一路径）**：先跑 `crates/local-display/fonts/gen_fonts.sh` 生成
   `fonts/lv_font_noto_sc_*.c`（10 档，产物不入库），再以 **`--features noto-font`** 编译
   （未启用时 `Font::of(..)` 返回 `None` ⇒ 走 LVGL 内置 ASCII 回退，中文为占位框）。
2. `--font <PATH>` 在 v2.0 **不生效**（启动时打印告警）；保留该参数仅为 CLI 兼容。
3. 字库缺失时中文显示**空心占位盒**（不 panic，但屏面不可交付）——`--smoke` 自检会给出逐页像素数。

## 6. 无真屏先行验证（设计 §10.1，部署前建议先跑）

```bash
# 本机/目标板均可：off 屏 + 回环通道（mupcd 未起时屏显「正在连接数据通道…」并可观察通道态）
mupc-local-display --backend offscreen --channel http://127.0.0.1:9810/v1/display/latest
```

> ℹ️ **登记【自检路径的后置渲染泵】**（B3-2a 规格评审 建议 7）：`--smoke` 在自检收尾用
> 「`SMOKE_PUMP_ROUNDS` × `SMOKE_PUMP_STEP` 真等待 + 每轮一次 `lv_timer_handler`」把渲染
> 泵到位（见 `src/app.rs` 的 `SMOKE_PUMP_ROUNDS` 文档）。它**仅 `--smoke` 用，不属于生产
> 事件循环** —— 生产渲染**只由 `lv_timer_handler` 驱动**（设计 §5.2 不变量 2）。看到该小循环
> 出现在生产路径即为设计回归。

## 7. 待真机验证（设计 §13，本机无法覆盖）

1. `/dev/fb0` 是否映射 HDMI、像素格式/位深/字节序（现按 32bpp XRGB 假定）；
2. 1025–1028 未命名寄存器能否整段读（现为两段 FC04 读）；
3. HDMI 分辨率协商（非 1024x768 需改 `--width/--height`）；
4. 屏面显示与 19200 波特总线吞吐余量（心跳 + 活读 + 显示两段读 + 下发写）。

## 8. 运维要点

- 崩溃自恢复：`Restart=always` + `RestartSec=1`（≤3s）；`systemctl stop` 走 SIGTERM 优雅退出并打印统计行。
- 页眉时钟为 **UTC**（未引时间库，KISS）；如需本地时间属后续小改（`localtime_r`，仅 Linux）。
