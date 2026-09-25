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
#    ⚠️ 配置目录须对 mupc 用户可写（本地屏「配置保存」的原子落盘目标）：
#       mupcd.service 已把 /opt/mupc/config 列入 ReadWritePaths；
#       文件属主仍需可写 —— sudo chown mupc:mupc /opt/mupc/config/*.yaml
sudo systemctl restart mupcd

# 3) 触摸屏稳定设备名（udev）——**不可省**，否则 /dev/mupc-touch 不存在、触摸不可用
#    规则默认注释，须按真机 idVendor/idProduct 填写后再启用（步骤见文件内注释）
sudo cp udev/99-mupc-touch.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
ls -l /dev/mupc-touch               # 应指向 /dev/input/eventN

# 4) 渲染进程 unit
sudo cp systemd/mupc-display.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now mupc-display
systemctl status mupc-display
journalctl -u mupc-display -f
```

## 4. framebuffer / DRM / 触摸 权限（真机校准，设计 §13 前置项 7）

- `--backend fbdev`（默认）需读写 `/dev/fb0`：通常属 `video` 组 →
  `sudo usermod -aG video mupc`（并保留 unit 的 `SupplementaryGroups`）。
- 若改用 DRM（本期**未实现**，`--backend drm` 会明确报错）需 `/dev/dri/*` 与 `render` 组。
- **触摸需 `/dev/input/event*`（属 `input` 组）**——`/dev/input/event*` 为 `root:input 0660`，
  进程以 `User=mupc` 运行，**缺 `input` 组即 EACCES**：进程仍会启动，但退化为
  「只读展示 + 页眉『触摸不可用』角标」，v2.0 的全部交互功能（配置/日志/联锁/审计）不可达。
  unit 已含 `SupplementaryGroups=dialout video render input`；若改用 `usermod` 方式亦需含 `input`。
  自检：`id mupc` 应列出 `input`。
- 快速自检（同时验三样：fb / 触摸 / 通道）：
  `sudo -u mupc /opt/mupc/bin/mupc-local-display --backend fbdev --touch-device /dev/mupc-touch
  --width 1024 --height 768`
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

### 5.1 上屏中文的真源

上屏中文**只**来自 `display-proto`（crate 名 `display-proto` / lib `mupc_display_proto`）的
`peripherals_labels`（即屏侧**可静态扫描**的源码）：

| 来源 | 常量 | 内容 |
|------|------|------|
| 短标签表 | `PERIPH_LABELS` | 每个白名单点的**精炼短标签 + 单位**；与白名单 `PERIPH_WHITELIST` **同序同长**（各 **447** 行） |
| UI 固定文案 | `ui_text` | 「站离线 / 未取数 / 数据异常 / 未配置 / 名称未获取 / 明细不可用 / 未启用 / 站点未启用 / 外设数据不可用 / 无活跃告警位 …」 |
| 分组标题 | `GROUP_TITLES` | P4 / P6 各段标题（含全角括号与其中的数字，按**字面量**） |

⚠️ **禁止**把 `point_table` 的登记 `label` 直接上屏——设计阶段实测**缺口 189 码位**（`℃`、全角
括号、`簇` / `阀` / `驱` / `馈` 等；设计 §15.7.1 F-3），上屏即豆腐块。

### 5.2 改文案后的流水线（**同批提交** = 门禁 H-1）

码表的**上游真源 = UI 设计文档 §3.6「全屏用字表」**（`extract_charset.py` 的唯一真源）；
`fonts/font_subset_charset.txt` 是该脚本的产物——**单行、无换行、无注释**（换行会被 shell 吃成
空格、污染字符集）。改上屏文案后：

```bash
cd crates/local-display/fonts
python3 extract_charset.py    # 从 UI §3.6 提取 → 覆盖写 font_subset_charset.txt（护栏见 §5.3）
./gen_fonts.sh                # 全档位 10 档：24 26 28 32 48 56 64 96 112 148（.c 产物不入库）
git status                    # 同批提交：font_subset_charset.txt + lv_font_cmap.txt + lv_font_metrics.txt
```

> ⚠️ 用 `python3`（本仓两处工具脚本的既有写法）：Windows 开发机上 `python3` 可能是 **Store 的
> 空壳别名**（执行无输出也无报错 ⇒ 脚本**静默不产出**），须加指向真实解释器的 shim —— 详见
> `fonts/gen_fonts.sh` 头部注释；`gen_fonts.sh` 另需 `npx`（首次联网拉 `lv_font_conv@1.5.2`）。

- **`lv_font_cmap.txt`（+ `lv_font_metrics.txt`）必须与字库重生成同批提交**：干净 clone / CI 上
  **没有 `.c` 产物**，`ui/tests.rs` 改以它们做权威基线——"生成物实际 cmap ≠ 入库清单"即**漂移检测变红**。
- 手工**只**往 `font_subset_charset.txt` 添字**不生效**：该文件由脚本**覆盖写**。要加字须先改
  **UI §3.6 用字表**再重跑（设计 §15.7.2 H-1 说的"并集写入码表"即这一步的净效果）。

### 5.3 R-2 护栏（重跑不得丢字）

`extract_charset.py` 以 §3.6 的**第 3 列**为唯一真源并**覆盖写**码表，而该列单元格里混有
**叙述性文字** ⇒ 每次重跑结果都与入库码表**不同**。护栏在**写入前**比对：

- **丢字 = 硬失败**：重跑结果 ⊉ 入库码表 ⇒ 打印缺失字符集、**非 0 退出**、**不覆写**入库文件。
  删字属**显式决定**（先手工改码表再重跑），不得靠重跑悄悄发生。
- **净增 = 只打印**（不失败）。
- `gen_fonts.sh` 自身**不**重做该比对（比对落在**产生漂移的那一步**）。

**实测（2026-09-25，基准 = HEAD `403459d` 的入库码表 464 字符）**：重跑得 **485** 字符 ⇒
**净增 22**（多为 §3.6 单元格内的叙述性文字，与该脚本头部记的 ~22 一致）、**缺失 1 = `空` U+7A7A**
⇒ **照现状直接重跑会被护栏非 0 拦下（且不覆写）**。

**缺 `空` 的原因（已知，非回归）**：`空` 是「空调」用字，只出现在 UI §3.6 的**行标签 / 说明段**
里，**不在第 3 列**；它是按 H-1「并集写入码表」**手工并入**码表的唯一例外（`display-proto`
的字符核验注：码表 `463 → 464`、cmap `461 → 462`）。⇒ **重跑前先处理**：① **推荐**把 `空` 补进
UI §3.6 的**第 3 列**（护栏自此自洽）；② 切勿顺"删字"路径删掉它 —— 「空调」是屏上的段名 / 站名
（`ui_text::ROLE_HVAC`），缺字形即豆腐块，H-2 覆盖率用例会红。

### 5.4 字库门禁 H-1…H-5

> **未过 H-1…H-5 不得通过 P4 / P6 真机验收**（设计 §15.7.2）。

| ID | 要求 | 在哪验 |
|----|------|--------|
| **H-1** | 扩字库：新增文案字符并集入码表 → 重跑 `gen_fonts.sh`（10 档）→ **同批**提交 `lv_font_cmap.txt` | 构建机（漂移检测） |
| **H-2** | 静态覆盖率用例（T-23）：短标签表 ∪ `ui_text` ∪ 分组标题 ⊆ 字库 cmap | 本机单测（可离屏） |
| **H-3** | 短标签表覆盖性：`PERIPH_WHITELIST.len() == PERIPH_LABELS.len()`（各 447 行） | `display-proto` 单测（不需要字库） |
| **H-4** | **真机逐字核对豆腐块**（P4 / P6 逐页） | 真机 |
| **H-5** | **槽宽门禁**：带符号数值（如 BMS 簇组电流 `-1600.0`）与长标签不越槽 | 真机 |

### 5.5 已知字库缺口（写这两处文案前必看）

`NotoSansSC-Regular.otf` **没有** `✕ U+2715` / `❚ U+275A` 的字形 ⇒ 这两个**仍在码表里、但不在
生成字体的 cmap 内**（实测：码表 **464** 字符、10 档 cmap 交集 **462** 码位，缺口即这两个），
**生产未使用**。要在屏上写这两处文案，**必须先改字库或换字符**（先例：`₂` 因 OTF 无字形，
已在 T21b2 按裁定改为 ASCII `H2`）。

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

## 9. 两端必须同版本发布

> 唯一真源：帧契约版本 `PROTO_VERSION`（`display-proto` 的 `frame.rs`）；**当前 = 3**。
> **各段不带版本** ——两端协商只看它一个数。版本判定**不放宽**：没有"兼容模式"、
> 没有"容忍任意版本"，也**不**静默按旧语义展示（设计 §15.2.1）。

**为什么必须同版本**

- 帧 `version != PROTO_VERSION` ⇒ 屏侧**在帧层拒帧** ⇒ 该帧**一个字段都不采纳**：不会出现
  "半帧 / 混版帧"，也不会拿旧语义解新帧。
- 该态**粘性**（`ChannelStatus::Incompatible{got,expected}`）：**只有收到一次成功帧才清除**，
  避免在「版本不匹配」与「通道断开」之间闪（设计 §15.6.2 ③ 不变量）。
- 屏侧日志会明确打出两端版本（唯一能"一眼看出谁旧"的信号）：
  ```
  journalctl -u mupc-display | grep 'version mismatch'
  # [mupc-local-display] 拉帧失败(第 1 次)：channel protocol version mismatch from
  #   `http://127.0.0.1:9810/v1/display/latest`: got 2, expected 3；超过 3 s 无成功将切
  #   「与主进程数据通道断开」态
  ```

**操作要求（升级 / 回滚）**

- `/opt/mupc/bin/mupcd` 与 `/opt/mupc/bin/mupc-local-display` **必须同批替换，不得只换其一**；
  回滚同理（两个一起回）。**发布批次 / 固件包是唯一有效的"同版本"凭据**。
- ⚠️ **不要**用 `--version` 判同版本：两端虽都支持 `mupcd --version` / `mupc-local-display --version`，
  但打印的是**进程自身的 crate 版本**（workspace 统一版本，当前两 crate 均为 `0.1.0`），
  它**既不是**帧契约 `PROTO_VERSION`、对"是否同批次"也**没有区分度**。

**现场表现与排障（⚠️ 易误判）**

- 版本不匹配被判为「读通道不可用」⇒ **屏上出现的不是版本专属文案**，而是既有的 EDGE-03 整屏
  降级（设计口径 ≤3 s；判据 = 3 s 无成功帧）：整屏压暗 + 中央「**与主进程数据通道断开**」+「N 秒」，
  页眉通道胶囊取红。设计所要求的版本专属整屏画面（灰底 + 「屏与主进程版本不匹配：屏 vN /
  主进程 vM，请刷同版本固件」）**整屏落点仍缺位**：文案常量（`ui_text::VERSION_MISMATCH` /
  `FLASH_SAME_VERSION`）与 `ScreenMode::VersionMismatch` 均已就位，但**无生产消费者**
  （设计 §15.9 **R-46** 已登记）。
- ⇒ **排障口径**：屏上出现「与主进程数据通道断开」而 **`mupcd` 进程仍在跑**
  （`systemctl status mupcd` 正常）时，**先核两端发布批次 / 看上面那条 `version mismatch` 日志**；
  **不要**先去查网络、触摸、`/dev/fb0`、`display.bind_addr` ——那些都不是本症因。
