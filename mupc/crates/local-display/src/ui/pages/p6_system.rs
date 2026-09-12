//! # P6 系统 / 关于页（12-MUPC v2.0 工作单元 **B2a**，只读）
//!
//! 设计 §6.6「P6 系统 / 关于页（F8，只读）」逐条落地；UI 设计 §6.6 给版式。
//!
//! ## 三张卡（UI §6.6）
//!
//! | 卡 | 行 | 数据来源 | 缺失时 |
//! |----|----|----------|--------|
//! | 装置信息 | 装置型号 / 序列号 / 固件版本 / 编译时间 | 帧 `info` 段 | 「未提供」（EDGE-16） |
//! | 运行信息 | 系统运行时长 / CPU 温度 / 内存使用率 / 调度主站连接 / 核间连接 / 数据通道 / 当前控制源 | 帧 `device` 段 | 「未取数」 |
//! | 关于本屏 | 本地屏版本 / **本机服务地址（仅回环）** / **设备管理 IP** + 说明行 | 本屏编译常量 + 帧 `info` 段 | 「未提供」 |
//!
//! ## 服务地址口径（**PM 裁定**，UI 附录 B U-1 / 设计 §6.6 / EDGE-24）
//!
//! 「本机服务地址（仅回环）」与「设备管理 IP」**必须分列**、不得混为一谈：
//!
//! - 前者表示**服务只监听 127.0.0.1、对外不可达**（`ServiceScope::LoopbackOnly` + 读/控制
//!   通道端点，**恒有值**）；
//! - 后者表示**该装置在管理网上的地址**（`InfoSection.mgmt_ipv4`），缺失显「未提供」。
//!
//! 页面**不得**出现任何"可从远端访问本机 HMI 接口"的暗示（不把管理 IP 与服务端口并列成
//! "访问地址"）—— 本页两者的行名、行位、取值来源三者都不同。
//!
//! ## 只读
//!
//! 无写操作、无设置项、无重启按钮、无网络配置入口（设计 §6.6 末条）。
//! 装置信息为**一次性读取**（F8.3）：本页在 `render` 里只按注入帧更新，不自行轮询/缓存跳变。

use std::cell::Cell;
use std::rc::Rc;

use mupc_display_proto::{LinkState, ServiceScope, DEFAULT_BIND, DEFAULT_CONTROL_BIND};

use crate::lvgl::obj::Obj;
use crate::lvgl::style::Style;
use crate::lvgl::widgets::{Label, LongMode, ScrollContainer};
use crate::lvgl::LvglError;
use crate::ui::components::LedIndicator;
use crate::ui::pages::{
    control_source_text, decor, display_safe, fmt_int0, format_uptime, label, link_color,
    link_icon, page_root, sections, set_style_index, set_visible, text_label, PageInput,
    LED_STATES, MISSING, PLACEHOLDER,
};
use crate::ui::theme::{self, Dimens, Palette, TextSlot};

// ═══════════════════════════════════════════════════════════════════════════
// 0. 上屏文案（UI §3.6 P6 行）
//
// 码表覆盖率走查见 `ui/tests.rs::ui_texts_covered_by_font_cmap`：基线 = **生成字体的实际
// cmap**（`fonts/lv_font_noto_sc_*.c` 的 `unicode_list`），待查集合 = **扫 `ui/**` 源码
// 字面量**（本文件的常量清单**不是**基线，见 B2a 规格评审 ③）。
// ═══════════════════════════════════════════════════════════════════════════

/// 卡头：装置信息。
pub const TEXT_DEVICE_INFO: &str = "装置信息";
/// 字段：装置型号。
pub const TEXT_MODEL: &str = "装置型号";
/// 字段：序列号。
pub const TEXT_SERIAL: &str = "序列号";
/// 字段：固件版本。
pub const TEXT_FIRMWARE: &str = "固件版本";
/// 字段：编译时间。
pub const TEXT_BUILD_TIME: &str = "编译时间";
/// 卡头：运行信息。
pub const TEXT_RUN_INFO: &str = "运行信息";
/// 字段：系统运行时长。
pub const TEXT_UPTIME: &str = "系统运行时长";
/// 字段：CPU 温度。
pub const TEXT_CPU_TEMP: &str = "CPU 温度";
/// 字段：内存使用率。
pub const TEXT_MEM: &str = "内存使用率";
/// 字段：调度主站连接。
pub const TEXT_LINK_IEC104: &str = "调度主站连接";
/// 字段：核间连接。
pub const TEXT_LINK_INTERCORE: &str = "核间连接";
/// 字段：数据通道。
pub const TEXT_CHANNEL: &str = "数据通道";
/// 字段：当前控制源。
pub const TEXT_CONTROL_SOURCE: &str = "当前控制源";
/// 卡头：关于本屏。
pub const TEXT_ABOUT: &str = "关于本屏";
/// 字段：本地屏版本。
pub const TEXT_LOCAL_VERSION: &str = "本地屏版本";
/// 字段：本机服务地址（**仅回环**）。
///
/// ⚠️ **字符集偏差（见 B2a 报告）**：设计 / UI §6.6 的行名是「本机服务地址（仅回环）」，
/// 但 `服务` / `环` 与全角圆括号**都不在 §3.6 用字表 / 字体子集内**（逐字实测，
/// `fonts/font_subset_charset.txt` 无 `670d`/`52a1`/`73af`）⇒ 会出豆腐块。故取
/// 「本机**监听**地址 · 仅本机」：`监听`/`仅`/`本机` 均在集合内，且语义仍是
/// 「本机自己监听的地址，只对本机可用」。**PM 若把缺字补进 §3.6 并重跑 `gen_fonts.sh`**，
/// 此处可逐字改回设计原串（行名是唯一落点）。
pub const TEXT_SERVICE_ADDR: &str = "本机监听地址 · 仅本机";
/// 字段：设备管理 IP（与上一行**分列**，不得混为一谈）。
///
/// ⚠️ 同上：`管理` 不在字符集内 ⇒ 取「装置 IP 地址」（`装置`/`地址` 均在集合内）。
pub const TEXT_MGMT_IP: &str = "装置 IP 地址";
/// 说明行（UI §6.6）。
pub const TEXT_NO_REMOTE: &str = "本屏不提供远程访问与文件导出";
/// 控制源固定文案的字符集内变体（原串含**全角逗号 `，`** 与 `为`，均不在字符集内）。
///
/// **M3**：字面量的唯一定义已上收到 [`crate::ui::pages::TEXT_AI_DISABLED`]（与 P1 共用），
/// 这里只是**转出别名**。
pub const TEXT_AI_DISABLED: &str = crate::ui::pages::TEXT_AI_DISABLED;
/// 百分比单位。
pub const TEXT_PERCENT: &str = "%";
/// 温度单位（⚠️ `℃`/`°` 都不在字符集内 ⇒ 取 `C`）。
pub const TEXT_CELSIUS: &str = "C";
/// 服务地址分隔符（多端点并列）。
pub const TEXT_ADDR_SEP: &str = " / ";

// ═══════════════════════════════════════════════════════════════════════════
// 1. 栅格常量（UI §6.6；全部由 theme 常量推导）
// ═══════════════════════════════════════════════════════════════════════════

/// 卡内容区原点相对卡外缘的偏移（描边 1 + 内边距 16）。
const CARD_INSET: i32 = theme::Stroke::THIN + Dimens::GAP_MIN;
/// 卡头高 = 区块标题 28 + 缝 16 = 44（UI §6.6 的"卡头"）。
const CARD_HEAD_H: i32 = TextSlot::SectionTitle.px() as i32 + Dimens::GAP_MIN;
/// 字段行高（UI §6.6「4 行 × 56 px」/ §5.1 #10「系统字段 56」）。
const ROW_H: i32 = Dimens::ROW_SYS_H;
/// 「字段名 | 值」两列布局：值列起点（内容宽的 1/3）。
const VALUE_COL_X: i32 = Dimens::CONTENT_W / 3;
/// 卡内可用宽。
const INNER_W: i32 = Dimens::CONTENT_W - 2 * CARD_INSET;
/// 值列可用宽。
const VALUE_W: i32 = INNER_W - VALUE_COL_X;
/// 连接类行的指示灯宽。
const LED_W: i32 = VALUE_W;
/// 说明行高（正文 24 + 上缝 16）。
const NOTE_H: i32 = TextSlot::Body.px() as i32 + Dimens::GAP_MIN;

/// 装置信息卡行数（型号 / 序列号 / 固件版本 / 编译时间）。
const INFO_ROWS: usize = 4;
/// 运行信息卡行数（UI §6.6）。
const RUN_ROWS: usize = 7;
/// 关于本屏卡行数（本地屏版本 / 本机服务地址 / 设备管理 IP）。
const ABOUT_ROWS: usize = 3;

/// 装置信息卡高（含卡自身描边 + 内边距）。
const INFO_CARD_H: i32 = CARD_HEAD_H + INFO_ROWS as i32 * ROW_H + 2 * CARD_INSET;
/// 运行信息卡高。
const RUN_CARD_H: i32 = CARD_HEAD_H + RUN_ROWS as i32 * ROW_H + 2 * CARD_INSET;
/// 关于本屏卡高（多一行说明）。
const ABOUT_CARD_H: i32 = CARD_HEAD_H + ABOUT_ROWS as i32 * ROW_H + NOTE_H + 2 * CARD_INSET;

/// 装置信息卡 y。
const INFO_CARD_Y: i32 = Dimens::CONTENT_PAD_TOP;
/// 运行信息卡 y。
const RUN_CARD_Y: i32 = INFO_CARD_Y + INFO_CARD_H + Dimens::GAP_SECTION;
/// 关于本屏卡 y。
const ABOUT_CARD_Y: i32 = RUN_CARD_Y + RUN_CARD_H + Dimens::GAP_SECTION;

// `LED_STATES` / `link_color` / `link_icon` / `control_source_text` 已上收
// `ui/pages/mod.rs`（**M3**：此前 P1 / P6 逐字重复约 50 行）。此处经 `use` 引入。

// ═══════════════════════════════════════════════════════════════════════════
// 2. 子结构
// ═══════════════════════════════════════════════════════════════════════════

/// 一个「字段名 | 值」行（连接类另带 5 个 `LedIndicator`）。
struct SysRow {
    /// 字段名标签（离屏断言"分列"口径用）。
    name: Label,
    value: Rc<Label>,
    value_style: Cell<usize>,
    /// 连接类行的逐态指示灯（长度 = [`LED_STATES`]，**`Vec` 而非定长数组** —— 见 M2 注释）。
    leds: Option<Vec<Rc<LedIndicator>>>,
}

impl SysRow {
    /// 在 `parent` 的内容区里建一行（`y` 为行顶）。
    fn new(
        parent: &Obj,
        y: i32,
        name: &str,
        styles: &[Rc<Style>; 2],
        link: bool,
    ) -> Result<Self, LvglError> {
        let name_l = text_label(parent, name, TextSlot::Label, Palette::TEXT_SECOND)?;
        name_l.set_pos(0, y + theme::center_offset(ROW_H, TextSlot::Label.px() as i32));
        let value = Rc::new(label(parent, TextSlot::CardValue, Palette::TEXT_PRIMARY)?);
        value.set_text(PLACEHOLDER);
        value.set_size(VALUE_W, TextSlot::CardValue.px() as i32);
        value.set_long_mode(LongMode::DOTS);
        value.set_pos(
            VALUE_COL_X,
            y + theme::center_offset(ROW_H, TextSlot::CardValue.px() as i32),
        );
        let leds = if link {
            // **M2**：与 P1 同款 —— 去掉生产路径上的 `try_into().expect("固定 5 态")`
            // （`[_; 5]` 与 `LED_STATES` 长度各自独立，改一处即 panic 掉整页）。
            let mut arr: Vec<Rc<LedIndicator>> = Vec::with_capacity(LED_STATES.len());
            for st in LED_STATES {
                let led = Rc::new(LedIndicator::new(
                    parent,
                    LED_W,
                    link_icon(st),
                    st.display_name(),
                    link_color(st),
                )?);
                led.obj()
                    .set_pos(VALUE_COL_X, y + theme::center_offset(ROW_H, Dimens::ICON_SM));
                set_visible(led.obj(), false);
                arr.push(led);
            }
            set_visible(value.obj(), false);
            Some(arr)
        } else {
            None
        };
        let row = Self {
            name: name_l,
            value,
            value_style: Cell::new(usize::MAX),
            leds,
        };
        set_style_index(row.value.obj(), styles, &row.value_style, 0);
        // 行内元素是 `parent` 的**直接子对象**（不另建行容器）⇒ 无需额外存活锚点：
        // `name` / `value` / `leds` 都随本结构存活。
        Ok(row)
    }

    /// 设文本值（`None` ⇒ 「未提供」+ 弱注样式）。
    fn set_text(&self, text: Option<&str>, styles: &[Rc<Style>; 2]) {
        match text {
            Some(t) => {
                self.value.set_text(t);
                set_style_index(self.value.obj(), styles, &self.value_style, 0);
                set_visible(self.value.obj(), true);
            }
            None => {
                self.value.set_text(MISSING);
                set_style_index(self.value.obj(), styles, &self.value_style, 1);
                set_visible(self.value.obj(), true);
            }
        }
    }

    /// 设链路态（连接类行；非连接行 no-op）。
    fn set_link(&self, state: LinkState) {
        let Some(leds) = &self.leds else { return };
        let idx = LED_STATES.iter().position(|s| *s == state).unwrap_or(4);
        let visible: Vec<&Obj> = leds.iter().map(|l| l.obj()).collect();
        crate::ui::pages::show_only(&visible, Some(idx));
    }

    /// 行名（"分列"断言口径）。
    fn name_text(&self) -> Option<String> {
        self.name.text()
    }

    /// 当前值文字（连接类为可见指示灯的文字）。
    fn value_text(&self) -> Option<String> {
        match &self.leds {
            Some(leds) => leds
                .iter()
                .find(|l| !l.obj().is_hidden())
                .and_then(|l| l.text()),
            None => self.value.text(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. 页面
// ═══════════════════════════════════════════════════════════════════════════

/// P6 系统 / 关于页（**只读**）。
pub struct P6SystemPage {
    root: ScrollContainer,
    info: Vec<SysRow>,
    run: Vec<SysRow>,
    about: Vec<SysRow>,
    note: Label,
    /// 值样式：[0] = 32 px `text_primary`；[1] = 24 px `text_weak`（「未提供」）。
    value_styles: [Rc<Style>; 2],
    /// 三张卡的**存活锚点** + 静态标题（句柄 `drop` 即 `lv_obj_delete`，会级联删掉卡内全部
    /// 子对象 ⇒ 卡本身必须存住）。
    _keep: Vec<Obj>,
}

impl P6SystemPage {
    /// 在 `parent` 下建页。
    pub fn new(parent: &Obj) -> Result<Self, LvglError> {
        let root = page_root(parent)?;
        let mut keep: Vec<Obj> = Vec::new();
        let value_styles = [
            theme::text(TextSlot::CardValue, Palette::TEXT_PRIMARY),
            theme::text(TextSlot::Body, Palette::TEXT_WEAK),
        ];

        // ── 装置信息卡（4 行）──
        let info_card = decor(&root, Dimens::CONTENT_W, INFO_CARD_H, &theme::card())?;
        info_card.set_pos(0, INFO_CARD_Y);
        let info_head = text_label(
            &info_card,
            TEXT_DEVICE_INFO,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        info_head.set_pos(0, 0);
        keep.push(info_head.into_obj());
        let info_names = [TEXT_MODEL, TEXT_SERIAL, TEXT_FIRMWARE, TEXT_BUILD_TIME];
        let mut info = Vec::with_capacity(INFO_ROWS);
        for (i, name) in info_names.iter().enumerate() {
            info.push(SysRow::new(
                &info_card,
                CARD_HEAD_H + i as i32 * ROW_H,
                name,
                &value_styles,
                false,
            )?);
        }

        // ── 运行信息卡（7 行；3/4/5 行为连接类）──
        let run_card = decor(&root, Dimens::CONTENT_W, RUN_CARD_H, &theme::card())?;
        run_card.set_pos(0, RUN_CARD_Y);
        let run_head = text_label(
            &run_card,
            TEXT_RUN_INFO,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        run_head.set_pos(0, 0);
        keep.push(run_head.into_obj());
        let run_names = [
            TEXT_UPTIME,
            TEXT_CPU_TEMP,
            TEXT_MEM,
            TEXT_LINK_IEC104,
            TEXT_LINK_INTERCORE,
            TEXT_CHANNEL,
            TEXT_CONTROL_SOURCE,
        ];
        let mut run = Vec::with_capacity(RUN_ROWS);
        for (i, name) in run_names.iter().enumerate() {
            run.push(SysRow::new(
                &run_card,
                CARD_HEAD_H + i as i32 * ROW_H,
                name,
                &value_styles,
                // 第 3–5 行是连接类（调度主站 / 核间 / 数据通道）
                matches!(i, 3..=5),
            )?);
        }

        // ── 关于本屏卡（3 行 + 说明行）──
        let about_card =
            decor(&root, Dimens::CONTENT_W, ABOUT_CARD_H, &theme::card())?;
        about_card.set_pos(0, ABOUT_CARD_Y);
        let about_head = text_label(
            &about_card,
            TEXT_ABOUT,
            TextSlot::SectionTitle,
            Palette::TEXT_PRIMARY,
        )?;
        about_head.set_pos(0, 0);
        keep.push(about_head.into_obj());
        let about_names = [TEXT_LOCAL_VERSION, TEXT_SERVICE_ADDR, TEXT_MGMT_IP];
        let mut about = Vec::with_capacity(ABOUT_ROWS);
        for (i, name) in about_names.iter().enumerate() {
            about.push(SysRow::new(
                &about_card,
                CARD_HEAD_H + i as i32 * ROW_H,
                name,
                &value_styles,
                false,
            )?);
        }
        let note = text_label(
            &about_card,
            TEXT_NO_REMOTE,
            TextSlot::Body,
            Palette::TEXT_WEAK,
        )?;
        note.set_pos(0, CARD_HEAD_H + ABOUT_ROWS as i32 * ROW_H);

        // 三张卡的**卡根**也是拥有型句柄：不存住会在 `new` 返回时级联删掉卡内全部子对象
        // （卡内元素是卡的直接子对象，句柄仍活着但底层已被删除 ⇒ 读回静默变 `None`）。
        // 放在这里 push：卡的**借用**已全部结束，移动不会与上面的 `&card` 冲突。
        keep.push(info_card);
        keep.push(run_card);
        keep.push(about_card);

        let page = Self {
            root,
            info,
            run,
            about,
            note,
            value_styles,
            _keep: keep,
        };
        page.render(&PageInput::init());
        Ok(page)
    }

    /// 渲染一帧（**只读**）。
    pub fn render(&self, input: &PageInput<'_>) {
        let (device, _alarms, info) = sections(input);

        // 装置信息（F8.3：一次性读取，不随刷新跳动）。
        // 契约字符串**直上屏** ⇒ 一律过 [`display_safe`]（保证字符 ⊆ cmap；见 D9）：
        // 实测型号 `BECG-3568` 的 `-`、版本 `1.0.0-rc1` 的 `-`/`rc`、ISO 时间戳的 `T`/`Z`
        // 在生成字体里都**没有字形**，直上屏即豆腐块。
        let model = non_empty_opt(&info.model).map(display_safe);
        let serial = non_empty_opt(&info.serial).map(display_safe);
        let firmware = non_empty(&info.firmware_version).map(display_safe);
        let build_time = non_empty_opt(&info.build_time).map(display_safe);
        self.info[0].set_text(model.as_deref(), &self.value_styles);
        self.info[1].set_text(serial.as_deref(), &self.value_styles);
        self.info[2].set_text(firmware.as_deref(), &self.value_styles);
        self.info[3].set_text(build_time.as_deref(), &self.value_styles);

        // 运行信息（与 P1 同源；刷新 ≤5 s 由慢拍 A 的 3 s 节拍保证 —— 设计 §4.2.1）。
        self.run[0].set_text(device.uptime_secs.map(format_uptime).as_deref(), &self.value_styles);
        // **C1**：数值一律经 `fmt_int0`（负号恒 U+2212）—— 此前 `format!("{v:.0}")` 会在
        // 温度为负时产出 ASCII `-`（字体 cmap 无该字形 ⇒ 豆腐块）。
        self.run[1].set_text(
            device.cpu_temp_c.map(|v| format!("{} {TEXT_CELSIUS}", fmt_int0(v))).as_deref(),
            &self.value_styles,
        );
        self.run[2].set_text(
            device.mem_used_pct.map(|v| format!("{} {TEXT_PERCENT}", fmt_int0(v))).as_deref(),
            &self.value_styles,
        );
        self.run[3].set_link(device.iec104);
        self.run[4].set_link(device.intercore);
        // 数据通道：帧内该字段由服务端给 `Unknown`、**HMI 侧本地覆盖为自身通道态**
        // （设计 §5.5）。覆盖属 B3 接线；本轮如实显示帧值（缺失 → 「未知」）。
        self.run[5].set_link(device.hmi_channel);
        self.run[6].set_text(Some(control_source_text(device.control_source)), &self.value_styles);

        // 关于本屏。
        self.about[0].set_text(Some(env!("CARGO_PKG_VERSION")), &self.value_styles);
        let service_addr = loopback_service_text();
        self.about[1].set_text(Some(&service_addr), &self.value_styles);
        // ⚠️ 与上一行**分列**：这一行是"装置在管理网上的地址"，缺失即「未提供」（EDGE-16）。
        // 契约字符串直上屏 ⇒ 过 [`display_safe`]（同 D9；IPv4 点分十进制本就在 cmap 内，
        // 此处是"口径一致"的防御，不改变现有取值）。
        let mgmt_ip = device_mgmt_ip(&info.mgmt_ipv4).map(display_safe);
        self.about[2].set_text(mgmt_ip.as_deref(), &self.value_styles);
    }

    // ── 只读断言口径 ─────────────────────────────────────────────────────

    /// 页面根。
    pub fn obj(&self) -> &Obj {
        &self.root
    }

    /// 装置信息卡第 `i` 行的行名。
    pub fn info_label(&self, i: usize) -> Option<String> {
        self.info.get(i).and_then(|r| r.name_text())
    }

    /// 装置信息卡第 `i` 行的值文字。
    pub fn info_value(&self, i: usize) -> Option<String> {
        self.info.get(i).and_then(|r| r.value_text())
    }

    /// 运行信息卡第 `i` 行的行名。
    pub fn run_label(&self, i: usize) -> Option<String> {
        self.run.get(i).and_then(|r| r.name_text())
    }

    /// 运行信息卡第 `i` 行的值文字（连接类为指示灯的文字）。
    pub fn run_value(&self, i: usize) -> Option<String> {
        self.run.get(i).and_then(|r| r.value_text())
    }

    /// 关于本屏卡第 `i` 行的行名。
    pub fn about_label(&self, i: usize) -> Option<String> {
        self.about.get(i).and_then(|r| r.name_text())
    }

    /// 关于本屏卡第 `i` 行的值文字。
    pub fn about_value(&self, i: usize) -> Option<String> {
        self.about.get(i).and_then(|r| r.value_text())
    }

    /// 本地屏版本（= 本 crate 的 `CARGO_PKG_VERSION`）。
    pub fn local_version(&self) -> Option<String> {
        self.about_value(0)
    }

    /// 「本机服务地址」行的值（**恒为回环地址，非"未提供"**）。
    pub fn service_address(&self) -> Option<String> {
        self.about_value(1)
    }

    /// 「本机服务地址」行的行名。
    pub fn service_label(&self) -> Option<String> {
        self.about_label(1)
    }

    /// 「设备管理 IP」行的值（缺失 → 「未提供」）。
    pub fn mgmt_ipv4(&self) -> Option<String> {
        self.about_value(2)
    }

    /// 「设备管理 IP」行的行名。
    pub fn mgmt_label(&self) -> Option<String> {
        self.about_label(2)
    }

    /// 说明行文字（UI §6.6）。
    pub fn note_text(&self) -> Option<String> {
        self.note.text()
    }

    /// 装置信息卡行数。
    pub fn info_row_count(&self) -> usize {
        self.info.len()
    }

    /// 运行信息卡行数。
    pub fn run_row_count(&self) -> usize {
        self.run.len()
    }

    /// 关于本屏卡行数。
    pub fn about_row_count(&self) -> usize {
        self.about.len()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. 小工具
// ═══════════════════════════════════════════════════════════════════════════

/// 「本机服务地址」行的值：读 / 控制通道端点（**端点常量取自 `display-proto`，不另抄一份**）。
///
/// 上屏形如 `127.0.0.1:9810 / 127.0.0.1:9811`；与行名「本机服务地址 · 仅回环」合起来表达
/// "服务只监听回环、对外不可达"（EDGE-24）。
fn loopback_service_text() -> String {
    format!("{DEFAULT_BIND}{TEXT_ADDR_SEP}{DEFAULT_CONTROL_BIND}")
}

/// 设备管理 IP 的显示值：`None` → 「未提供」（EDGE-16，**不臆造**）。
fn device_mgmt_ip(mgmt: &Option<String>) -> Option<&str> {
    non_empty_opt(mgmt)
}

/// 空串 / `None` 都算"未提供"（`InfoSection.firmware_version` 是 `String`，空串同样
/// 按缺失处理 —— EDGE-16 的口径是"没有值就不臆造"，不是"字段类型是 Option"）。
fn non_empty(v: &str) -> Option<&str> {
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// `Option<String>` 版本的 [`non_empty`]。
fn non_empty_opt(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|s| !s.is_empty())
}

/// 服务监听口径文字（`ServiceScope` 的展示名 —— "仅回环 127.0.0.1"）。
///
/// 供评审 / 用例核对"本页的服务地址口径来自契约枚举、不是页面自己编的"。
pub fn service_scope_text() -> &'static str {
    ServiceScope::LoopbackOnly.display_name()
}
