//! IEC 104 协议解析

use byteorder::ReadBytesExt;
use mupc_common::{ErrorCode, MupcError};
use std::io::Cursor;

/// 帧类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameType {
    IFrame, // 编号的信息传输帧
    SFrame, // 确认帧
    UFrame, // 控制帧
}

/// U 帧类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UFrameType {
    StartDtAct, // 启动数据传输激活
    StartDtCon, // 启动数据传输确认
    StopDtAct,  // 停止数据传输激活
    StopDtCon,  // 停止数据传输确认
    TestFrAct,  // 测试帧激活
    TestFrCon,  // 测试帧确认
}

/// 类型标识 (Type ID)
///
/// ⚠️ **标准名 vs 本仓命名**（01 设计 §9.2.4 / §9.7 C-6）：**既有变体的名与值一律不改**
/// （改名或改值都会破坏既有报文与 `required_type_ids` 用例），但其中 4 个变体名与
/// IEC 60870-5-101/104 标准的 TypeID 名称**错位**，逐条登记如下（**新增变体一律用标准名**）：
///
/// | 值 | 本仓变体名 | **标准名** | 差异 |
/// |----|-----------|-----------|------|
/// | 30 | `MSpTa1` | **`M_SP_TB_1`**（单点 + CP56Time2a） | 本仓名写 `*TA*`（CP24Time2a 旧名），**本期按 30 组帧** |
/// | 31 | `MDpTa1` | **`M_DP_TB_1`**（双点 + CP56Time2a） | 本仓名写 `*TA*`；本期不用 |
/// | 34 | `MMeTa1` | **`M_ME_TD_1`**（归一化 + CP56Time2a） | 本仓名写 `*TA*`；本期不用 |
/// | 35 | `MMeTd1` | **`M_ME_TE_1`**（标度化 + CP56Time2a） | 本仓名写 `*TD*`；本期不用 |
///
/// ⇒ 本期上送统一取 **TI=36（[`TypeId::MMeTf1`]，短浮点 + CP56Time2a）** 与
/// **TI=30（[`TypeId::MSpTb1`]）**——后者是 [`TypeId::MSpTa1`] 的**别名**（见 `impl` 内关联常量）。
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum TypeId {
    // 监视方向 (Monitoring)
    MSpNa1 = 1,  // 单点遥信 (M_SP_NA_1)
    MDpNa1 = 3,  // 双点遥信 (M_DP_NA_1)
    MMeNa1 = 9,  // 测量值，归一化值 (M_ME_NA_1)
    MMeNc1 = 13, // 测量值，短浮点数 (M_ME_NC_1，**无时标**) —— 既有总表 6 点现行形态
    // —— 以下 4 个是既有变体：只订正注释，**不改名不改值** ——
    MSpTa1 = 30, // ⚠️ 名标 M_SP_TA_1；**标准 30 = M_SP_TB_1（带 CP56Time2a）**，本期按 30 组帧
    MDpTa1 = 31, // ⚠️ 名标 M_DP_TA_1；**标准 31 = M_DP_TB_1**；本期不用
    MMeTa1 = 34, // ⚠️ 名标 M_ME_TA_1；**标准 34 = M_ME_TD_1（归一化 + CP56Time2a）**；本期不用
    MMeTd1 = 35, // ⚠️ 名标 M_ME_TD_1；**标准 35 = M_ME_TE_1（标度化 + CP56Time2a）**；本期不用
    // —— 以下为新增（§9.2.4） ——
    MMeTf1 = 36,  // 测量值，短浮点 + CP56Time2a (M_ME_TF_1) —— **本期遥测统一形态**
    CIcNa1 = 100, // 站总召 (C_IC_NA_1)
    // 控制方向 (Control)
    CScNa1 = 45, // 单点遥控 (C_SC_NA_1)
    CDcNa1 = 46, // 双点遥控 (C_DC_NA_1)
    CSeNa1 = 48, // 调节命令 (C_SE_NA_1)
    CScTa1 = 58, // 单点遥控带时标 (C_SC_TA_1)
    CDcTa1 = 59, // 双点遥控带时标 (C_DC_TA_1)
    CSeTa1 = 61, // 调节命令带时标 (C_SE_TA_1)
}

impl TypeId {
    /// `M_SP_TB_1`（单点遥信 + CP56Time2a，标准值 30）的**别名**。
    ///
    /// Rust 不允许两个枚举变体同值 ⇒ 以**关联常量**表达与 [`TypeId::MSpTa1`] 同值同物
    /// （§9.2.4）。取值语义完全等价：`TypeId::MSpTb1 == TypeId::MSpTa1`（编译期断言见单测）。
    // 命名刻意与 TypeID 名（`M_SP_TB_1`）同形以保持与设计/文档逐字对应，故豁免命名风格检查。
    #[allow(non_upper_case_globals)]
    pub const MSpTb1: TypeId = TypeId::MSpTa1;

    /// 从 u8 值创建 TypeId
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            1 => Some(TypeId::MSpNa1),
            3 => Some(TypeId::MDpNa1),
            9 => Some(TypeId::MMeNa1),
            13 => Some(TypeId::MMeNc1),
            30 => Some(TypeId::MSpTa1),
            31 => Some(TypeId::MDpTa1),
            34 => Some(TypeId::MMeTa1),
            35 => Some(TypeId::MMeTd1),
            36 => Some(TypeId::MMeTf1),
            45 => Some(TypeId::CScNa1),
            46 => Some(TypeId::CDcNa1),
            48 => Some(TypeId::CSeNa1),
            58 => Some(TypeId::CScTa1),
            59 => Some(TypeId::CDcTa1),
            61 => Some(TypeId::CSeTa1),
            100 => Some(TypeId::CIcNa1),
            _ => None,
        }
    }
}

/// 传输原因常量（§9.2.4，`protocol.rs`）
pub const COT_CYCLIC: u8 = 1; // 周期（A/B 档）
pub const COT_SPONT: u8 = 3; // 突发/变位（C 档）
pub const COT_REQ: u8 = 5; // 请求（主站 → 装置）
pub const COT_ACT: u8 = 6; // 激活（装置 → 主站，应答遥控）
pub const COT_ACT_CON: u8 = 7; // 激活确认（总召 ACT_CON）
pub const COT_ACT_TERM: u8 = 10; // 激活终止（总召 ACT_TERM）
pub const COT_INTROGEN: u8 = 20; // 响应站召唤（总召数据）

/// 传输原因 (Cause of Transmission)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cot(pub u8);

impl Cot {
    pub const PERIODIC: u8 = 1; // 周期/循环
    pub const BACKGROUND: u8 = 2; // 后台扫描
    pub const SPONTANEOUS: u8 = 3; // 突发
    pub const COMMAND: u8 = 6; // 命令
    pub const ACTIVATION: u8 = 7; // 激活
    pub const ACTIVATION_CON: u8 = 8; // 激活确认
    pub const DEACTIVATION: u8 = 9; // 停止激活
    pub const DEACTIVATION_CON: u8 = 10; // 停止激活确认
}

/// 数据质量
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Quality {
    Good,
    Overflow,
    Reserved,
    Invalid,
}

/// 数据值
#[derive(Debug, Clone)]
pub enum Value {
    SinglePoint(bool), // 单点 (开/关)
    DoublePoint(u8),   // 双点 (00=中间,01=开,10=关,11=无效)
    Normalized(f64),   // 归一化值 (-1.0 ~ 1.0)
    Scaled(i16),       // 标度化值
    Float(f64),        // 短浮点数
}

/// 信息对象地址 (IOA) 3字节
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ioa([u8; 3]);

impl Ioa {
    pub fn new(a1: u8, a2: u8, a3: u8) -> Self {
        Self([a1, a2, a3])
    }

    pub fn value(&self) -> u32 {
        (self.0[0] as u32) | ((self.0[1] as u32) << 8) | ((self.0[2] as u32) << 16)
    }
}

/// ASDU 头
#[derive(Debug, Clone)]
pub struct AsduHeader {
    pub type_id: TypeId,
    pub sq_num: u8,
    pub cot: Cot,
    pub orig_addr: u16,
}

/// IEC 104 帧
#[derive(Debug, Clone)]
pub struct Iec104Frame {
    pub frame_type: FrameType,
    pub start: u8,  // 0x68
    pub length: u8, // 后续长度
    pub control1: u8,
    pub control2: u8,
    pub control3: u8,
    pub control4: u8,
    pub asdu: Vec<u8>,
}

impl Iec104Frame {
    /// 解析 IEC 104 帧
    pub fn parse(data: &[u8]) -> Result<Self, MupcError> {
        if data.len() < 6 {
            return Err(MupcError::new(
                ErrorCode::FrameParseError,
                "Frame too short",
                "gateway",
            ));
        }

        let mut cursor = Cursor::new(data);

        // 起始字符
        let start = cursor.read_u8().map_err(|_| {
            MupcError::new(ErrorCode::FrameParseError, "Invalid start byte", "gateway")
        })?;
        if start != 0x68 {
            return Err(MupcError::new(
                ErrorCode::FrameParseError,
                format!("Invalid start byte: {:#x}", start),
                "gateway",
            ));
        }

        // 长度
        let length = cursor
            .read_u8()
            .map_err(|_| MupcError::new(ErrorCode::FrameParseError, "Invalid length", "gateway"))?;

        if length < 4 {
            return Err(MupcError::new(
                ErrorCode::FrameParseError,
                format!("Invalid length: {} (must be >= 4)", length),
                "gateway",
            ));
        }

        if data.len() < (length as usize + 2) {
            return Err(MupcError::new(
                ErrorCode::FrameParseError,
                "Frame length mismatch",
                "gateway",
            ));
        }

        // 控制字段
        let control1 = cursor.read_u8().map_err(|_| {
            MupcError::new(ErrorCode::FrameParseError, "Invalid control1", "gateway")
        })?;
        let control2 = cursor.read_u8().map_err(|_| {
            MupcError::new(ErrorCode::FrameParseError, "Invalid control2", "gateway")
        })?;
        let control3 = cursor.read_u8().map_err(|_| {
            MupcError::new(ErrorCode::FrameParseError, "Invalid control3", "gateway")
        })?;
        let control4 = cursor.read_u8().map_err(|_| {
            MupcError::new(ErrorCode::FrameParseError, "Invalid control4", "gateway")
        })?;

        // 确定帧类型
        let frame_type = Self::determine_frame_type(control1, control2, control3, control4);

        // ASDU
        let asdu_start = 6;
        let asdu_len = (length as usize) - 4;
        let asdu = data[asdu_start..asdu_start + asdu_len].to_vec();

        Ok(Self {
            frame_type,
            start,
            length,
            control1,
            control2,
            control3,
            control4,
            asdu,
        })
    }

    /// 确定帧类型
    fn determine_frame_type(c1: u8, _c2: u8, _c3: u8, _c4: u8) -> FrameType {
        // U 帧: 控制字段低 2 位 = 0b11（bit0=1, bit1=1）
        if c1 & 0x03 == 0x03 {
            return FrameType::UFrame;
        }

        // S 帧: 控制字段 bit0 = 1, bit1 = 0
        if c1 & 0x01 == 0x01 {
            return FrameType::SFrame;
        }

        // I 帧: bit0 = 0
        FrameType::IFrame
    }

    /// 创建 U 帧
    pub fn make_u_frame(u_type: UFrameType) -> Vec<u8> {
        let (c1, c2, c3, c4) = match u_type {
            UFrameType::StartDtAct => (0x07, 0x00, 0x00, 0x00),
            UFrameType::StartDtCon => (0x0B, 0x00, 0x00, 0x00),
            UFrameType::StopDtAct => (0x13, 0x00, 0x00, 0x00),
            UFrameType::StopDtCon => (0x23, 0x00, 0x00, 0x00),
            UFrameType::TestFrAct => (0x43, 0x00, 0x00, 0x00),
            UFrameType::TestFrCon => (0x83, 0x00, 0x00, 0x00),
        };

        vec![0x68, 0x04, c1, c2, c3, c4]
    }

    /// 创建 S 帧（确认 I 帧）
    ///
    /// **S 帧只携带"本端已接收序号"**（N(R)），控制域为 `68 04 01 00 <r<<1> <r>>7>`：
    /// 第 3 字节恒为 `0x01`（S 帧标识），第 4 字节为 `0x00`；原 `send_seq` 形参语义本就多余
    /// （S 帧不含发送序号），故按 §9.2.3 改为**单参签名**。
    pub fn make_s_frame(recv_seq: u16) -> Vec<u8> {
        let r = recv_seq & 0x7FFF;
        let s1 = 0x01;
        let s2 = 0x00;
        let s3 = ((r << 1) & 0xFF) as u8;
        let s4 = (r >> 7) as u8;

        vec![0x68, 0x04, s1, s2, s3, s4]
    }

    /// 创建 I 帧
    ///
    /// 控制域为 **15 位序号**：`i1 = (N(S) << 1) & 0xFE`、`i2 = N(S) >> 7`、
    /// `i3 = (N(R) << 1) & 0xFE`、`i4 = N(R) >> 7`（§9.2.3 缺陷 #1 的修正）。
    /// 序号取 `& 0x7FFF`：超出 15 位按**回绕**处理（调用方 [`super::server::OutboundSeq`]
    /// 亦以 `& 0x7FFF` 自增）。
    ///
    /// ⚠️ 既有实现把 `i2`/`i4` 恒写 `0x00` 且 `i3` 带 `+1`，导致 **N(S) ≥ 128 即回绕**
    /// （234 点一轮必然触发 ⇒ 主站判序号错乱）。签名保持不变。
    pub fn make_i_frame(send_seq: u16, recv_seq: u16, asdu: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.push(0x68);

        let length = 4 + asdu.len();
        frame.push(length as u8);

        let s = send_seq & 0x7FFF;
        let r = recv_seq & 0x7FFF;

        let i1 = ((s << 1) & 0xFE) as u8;
        let i2 = (s >> 7) as u8;
        let i3 = ((r << 1) & 0xFE) as u8;
        let i4 = (r >> 7) as u8;

        frame.push(i1);
        frame.push(i2);
        frame.push(i3);
        frame.push(i4);
        frame.extend_from_slice(asdu);

        frame
    }

    /// 获取发送序号（从 I 帧）
    ///
    /// ⚠️ 高位字节（`control2`/`control4`）承载 15 位序号的**高 8 位**，**不得**再 `& 0x7F`
    /// ——否则序号 ≥ 16384 会被截断（`0x7FFF` 会读回 `0x3FFF`）。该掩码是 §9.2.3 缺陷 #1 的
    /// 读侧对偶缺陷，与写侧同批修正（15 位往返用例钉住）。
    pub fn send_sequence(&self) -> u16 {
        ((self.control1 >> 1) & 0x7F) as u16 | ((self.control2 as u16) << 7)
    }

    /// 获取接收序号（从 I 帧）
    pub fn recv_sequence(&self) -> u16 {
        ((self.control3 >> 1) & 0x7F) as u16 | ((self.control4 as u16) << 7)
    }

    /// 获取 U 帧类型
    pub fn u_frame_type(&self) -> Option<UFrameType> {
        if self.frame_type != FrameType::UFrame {
            return None;
        }

        match self.control1 {
            0x07 => Some(UFrameType::StartDtAct),
            0x0B => Some(UFrameType::StartDtCon),
            0x13 => Some(UFrameType::StopDtAct),
            0x23 => Some(UFrameType::StopDtCon),
            0x43 => Some(UFrameType::TestFrAct),
            0x83 => Some(UFrameType::TestFrCon),
            _ => None,
        }
    }

    /// 解析 ASDU 头
    pub fn parse_asdu_header(&self) -> Result<AsduHeader, MupcError> {
        if self.asdu.len() < 4 {
            return Err(MupcError::new(
                ErrorCode::FrameParseError,
                "ASDU too short",
                "gateway",
            ));
        }

        let type_id = TypeId::from_u8(self.asdu[0]).ok_or_else(|| {
            MupcError::new(
                ErrorCode::AsduTypeMismatch,
                format!("Unknown TypeID: {}", self.asdu[0]),
                "gateway",
            )
        })?;

        let sq_num = self.asdu[1] & 0x7F;
        let cot = Cot(self.asdu[2]);
        let orig_addr = ((self.asdu[3] as u16) << 8) | (self.asdu[4] as u16);

        Ok(AsduHeader {
            type_id,
            sq_num,
            cot: Cot(cot.0 & 0x3F), // 最高 2 位是 QOI
            orig_addr,
        })
    }
}

/// 编码遥测数据为 IEC104 监视方向 ASDU（M_ME_NC_1 短浮点测量值）
///
/// FIXME: 简化实现，IOA 与遥测点的映射根据点表确定
pub fn encode_telemetry_asdu(ioa: u32, value: f32, cot: u8) -> Vec<u8> {
    // TypeID=13 (M_ME_NC_1), 可变结构限定词=1(单对象), COT, 公共地址 2 字节
    let mut asdu = vec![TypeId::MMeNc1 as u8, 0x01, cot, 0x00, 0x00];
    // IOA（3 字节，小端）
    asdu.push((ioa & 0xFF) as u8);
    asdu.push(((ioa >> 8) & 0xFF) as u8);
    asdu.push(((ioa >> 16) & 0xFF) as u8);
    // 短浮点（IEEE 754 32 位，小端）
    asdu.extend_from_slice(&value.to_le_bytes());
    asdu
}

// ========== CP56Time2a（§9.2.4 时标口径） ==========

/// `CP56Time2a` 编码（7 字节，**UTC**）：
/// `毫秒(2, LE, 0–59999) | 分(1) | 时(1) | 日(1, 低 5 位) + 星期(1, 高 3 位) | 月(1) | 年(1, = year-2000)`
///
/// 口径与设计 §2.2「时标规范」一致：**UTC**、时标 = **采集时刻**（不是发送时刻）。
/// ⚠️ 首字段是**"秒 + 毫秒"合体**（`秒(0..59) × 1000 + 毫秒(0..999)`，上界 59999），
/// **不是**单圈秒以下的毫秒——按字面当"毫秒"会丢掉秒（§9.5 的字节级用例钉住）。
/// 年域只有 1 字节且**相对 2000**：可表达区间 2000–2099，越界时标按 `(year-2000) mod 100` 回绕。
pub fn encode_cp56time2a(ts_ms: u64) -> [u8; 7] {
    let secs = (ts_ms / 1000) as i64;
    let millis = (ts_ms % 1000) as u16;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = (sod / 3600) as u8;
    let minute = ((sod % 3600) / 60) as u8;
    // 首字段 = 秒(0..59) × 1000 + 毫秒(0..999)，上界 59999
    let ms_field = ((sod % 60) as u16) * 1000 + millis;

    // ISO 星期：周一 = 1 … 周日 = 7（1970-01-01 = 周四 = 4）。
    // ⚠️ 必须**先取模再 +1**：`days + 3` 本身可达 7..9（周一..周三），若"先 +1 再 `& 0x07`"
    // 就会把它们掩成 0/1/2 —— 违反 IEC 60870-5-4 的 DOW ∈ 1..7（周四恰好落 4 而漏检）。
    let weekday = ((days + 3).rem_euclid(7) + 1) as u8;

    [
        (ms_field & 0xFF) as u8,
        (ms_field >> 8) as u8,
        minute & 0x3F, // bit7 = 时标无效位（本实现恒为 0 = 有效）
        hour & 0x1F,
        ((day as u8) & 0x1F) | ((weekday & 0x07) << 5),
        (month as u8) & 0x0F,
        (year - 2000).rem_euclid(100) as u8,
    ]
}

/// `CP56Time2a` 解码为 Unix 毫秒（UTC）；字节不足或字段越界 ⇒ `None`。
///
/// 年域按设计**字面**还原：`year = 2000 + b[6]`（⇒ 可表达区间 2000–2099，与编码互逆）。
/// 1970 年的时标（如 §9.2.5 回归钉的 `ts=1000`）编码后落在 2070 年——**这是 1 字节年域
/// 的固有歧义**，本模块按设计字面不擅自加世纪消歧（回归钉改用**字节级**断言）。
pub fn decode_cp56time2a(b: &[u8]) -> Option<u64> {
    if b.len() < 7 {
        return None;
    }
    let ms_field = (b[0] as u64) | ((b[1] as u64) << 8);
    let minute = (b[2] & 0x3F) as i64;
    let hour = (b[3] & 0x1F) as i64;
    let day = (b[4] & 0x1F) as i64;
    let month = (b[5] & 0x0F) as i64;
    let year = 2000 + (b[6] as i64);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || minute > 59
        || hour > 23
        || ms_field > 59_999
    {
        return None;
    }
    let days = days_from_civil(year, month as u32, day as u32);
    let secs = days * 86_400 + hour * 3600 + minute * 60 + (ms_field / 1000) as i64;
    if secs < 0 {
        return None;
    }
    Some(secs as u64 * 1000 + (ms_field % 1000))
}

/// 自 Unix 纪元起的**天数** → `(年, 月, 日)`（Howard Hinnant 的 `civil_from_days`，纯整数运算）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]，3 月 = 0
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `(年, 月, 日)` → 自 Unix 纪元起的天数（`days_from_civil`）。
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 }.div_euclid(400);
    let yoe = (y - era * 400) as u64; // [0, 399]
    let mp = if m > 2 { m - 3 } else { m + 9 } as u64; // 3 月 = 0
    let doy = (153 * mp + 2) / 5 + (d as u64) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe as i64 - 719_468
}

/// 监视方向 ASDU 的公共头（`type_id + VSQ + COT + CA(2)`，共 5 字节）。
///
/// `CA`（公共地址）取 `0x0000`——与既有 [`encode_telemetry_asdu`] 同口径（现场追认的单站形态）。
fn asdu_header(type_id: TypeId, cot: u8) -> Vec<u8> {
    vec![type_id as u8, 0x01, cot, 0x00, 0x00]
}

/// 3 字节 IOA（小端）
fn push_ioa(asdu: &mut Vec<u8>, ioa: u32) {
    asdu.push((ioa & 0xFF) as u8);
    asdu.push(((ioa >> 8) & 0xFF) as u8);
    asdu.push(((ioa >> 16) & 0xFF) as u8);
}

/// `M_ME_TF_1`(TI=36)：`IOA(3) + 短浮点(4, LE) + CP56Time2a(7)` ⇒ ASDU 共 **19 字节**（帧长 25）。
///
/// 时标 = `ts_ms`（**采集时刻**，UTC），不得用响应时刻重打（§9.2.5 的回归断言钉住）。
pub fn encode_me_tf1(ioa: u32, value: f32, ts_ms: u64, cot: u8) -> Vec<u8> {
    let mut asdu = asdu_header(TypeId::MMeTf1, cot);
    push_ioa(&mut asdu, ioa);
    asdu.extend_from_slice(&value.to_le_bytes());
    asdu.extend_from_slice(&encode_cp56time2a(ts_ms));
    asdu
}

/// `M_SP_TB_1`(TI=30)：`IOA(3) + SIQ(1) + CP56Time2a(7)` ⇒ ASDU 共 **16 字节**（帧长 22）。
///
/// `SIQ` 的 bit4 为质量位（`0` = 有效），本实现恒发有效（无有效的点**不上送**，§9.2.6）。
pub fn encode_sp_tb1(ioa: u32, value: bool, ts_ms: u64, cot: u8) -> Vec<u8> {
    let mut asdu = asdu_header(TypeId::MSpTb1, cot);
    push_ioa(&mut asdu, ioa);
    asdu.push(u8::from(value)); // SIQ：bit0 = SPI
    asdu.extend_from_slice(&encode_cp56time2a(ts_ms));
    asdu
}

/// `C_IC_NA_1`(TI=100) 的 `ACT_CON` / `ACT_TERM` 空应答：`IOA = 0` + `QOI = 0` ⇒ ASDU 共 **9 字节**。
pub fn encode_ic_term(cot: u8) -> Vec<u8> {
    let mut asdu = asdu_header(TypeId::CIcNa1, cot);
    push_ioa(&mut asdu, 0);
    asdu.push(0x00); // QOI = 0（空应答不携带 QOI）
    asdu
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== TypeId Tests ==========

    #[test]
    fn test_type_id_all_required_values() {
        // 验证所有必需的 TypeID 都存在
        let required_type_ids = vec![
            (1, TypeId::MSpNa1, "单点遥信"),
            (3, TypeId::MDpNa1, "双点遥信"),
            (9, TypeId::MMeNa1, "测量值-归一化"),
            (13, TypeId::MMeNc1, "测量值-短浮点"),
            (30, TypeId::MSpTa1, "单点遥信带时标"),
            (31, TypeId::MDpTa1, "双点遥信带时标"),
            (34, TypeId::MMeTa1, "测量值带时标-归一化"),
            (35, TypeId::MMeTd1, "测量值带时标"),
            // 新增三行（§9.2.3 受影响断言表）：30 的标准名别名 / 36 本期遥测形态 / 100 站总召
            (30, TypeId::MSpTb1, "单点遥信带时标（标准名别名，同值同物）"),
            (
                36,
                TypeId::MMeTf1,
                "测量值-短浮点带时标（本期遥测统一形态）",
            ),
            (100, TypeId::CIcNa1, "站总召"),
            (45, TypeId::CScNa1, "单点遥控"),
            (46, TypeId::CDcNa1, "双点遥控"),
            (48, TypeId::CSeNa1, "调节命令"),
            (58, TypeId::CScTa1, "单点遥控带时标"),
            (59, TypeId::CDcTa1, "双点遥控带时标"),
            (61, TypeId::CSeTa1, "调节命令带时标"),
        ];

        for (val, expected_type, _name) in required_type_ids {
            let type_id = TypeId::from_u8(val);
            assert_eq!(
                type_id,
                Some(expected_type),
                "TypeID {} should be {:?}",
                val,
                expected_type
            );
        }
    }

    #[test]
    fn test_type_id_from_u8_invalid() {
        assert_eq!(TypeId::from_u8(0), None);
        // 2 = M_SP_TA_1（**未实现**）：本条**保持**断言 None（§9.2.3 受影响断言表）
        assert_eq!(TypeId::from_u8(2), None);
        // ⚠️ 100 由 None **改为** Some(CIcNa1) —— 总召 TypeID 落地（§9.2.3 表：本条必然变红）
        assert_eq!(TypeId::from_u8(100), Some(TypeId::CIcNa1));
        assert_eq!(TypeId::from_u8(255), None);
    }

    /// `MSpTb1` 是 `MSpTa1` 的**值别名**（关联常量，§9.2.4）：两者恒等，且都等于 30。
    #[test]
    fn test_type_id_msptb1_is_alias_of_mspta1() {
        assert_eq!(TypeId::MSpTb1, TypeId::MSpTa1);
        assert_eq!(TypeId::MSpTb1 as u8, 30);
        assert_eq!(TypeId::from_u8(30), Some(TypeId::MSpTb1));
        // 既有变体值不得被改动（改名/改值会破坏既有报文）
        assert_eq!(TypeId::MSpNa1 as u8, 1);
        assert_eq!(TypeId::MMeNc1 as u8, 13);
        assert_eq!(TypeId::MMeTa1 as u8, 34);
        assert_eq!(TypeId::MMeTd1 as u8, 35);
        assert_eq!(TypeId::MMeTf1 as u8, 36);
        assert_eq!(TypeId::CIcNa1 as u8, 100);
    }

    #[test]
    fn test_type_id_partial_eq() {
        assert_eq!(TypeId::MSpNa1, TypeId::MSpNa1);
        assert_ne!(TypeId::MSpNa1, TypeId::MDpNa1);
    }

    // ========== FrameType Tests ==========

    #[test]
    fn test_frame_type_determination() {
        // U 帧: TESTFR_ACT - 68 04 43 00 00 00
        let data = [0x68, 0x04, 0x43, 0x00, 0x00, 0x00];
        let frame = Iec104Frame::parse(&data).unwrap();
        assert_eq!(frame.frame_type, FrameType::UFrame);

        // U 帧: TESTFR_CON - 68 04 83 00 00 00
        let data = [0x68, 0x04, 0x83, 0x00, 0x00, 0x00];
        let frame = Iec104Frame::parse(&data).unwrap();
        assert_eq!(frame.frame_type, FrameType::UFrame);

        // S 帧 - 68 04 01 00 01 00
        let data = [0x68, 0x04, 0x01, 0x00, 0x01, 0x00];
        let frame = Iec104Frame::parse(&data).unwrap();
        assert_eq!(frame.frame_type, FrameType::SFrame);

        // I 帧 - 68 04 00 00 00 00 (及其他)
        let data = [0x68, 0x04, 0x00, 0x00, 0x00, 0x00];
        let frame = Iec104Frame::parse(&data).unwrap();
        assert_eq!(frame.frame_type, FrameType::IFrame);
    }

    // ========== U Frame Tests ==========

    #[test]
    fn test_u_frame_parse() {
        // STARTDT_act: 68 04 07 00 00 00
        let data = [0x68, 0x04, 0x07, 0x00, 0x00, 0x00];
        let frame = Iec104Frame::parse(&data).unwrap();
        assert_eq!(frame.frame_type, FrameType::UFrame);
        assert_eq!(frame.u_frame_type(), Some(UFrameType::StartDtAct));
    }

    #[test]
    fn test_u_frame_types() {
        let test_cases = vec![
            ([0x68, 0x04, 0x07, 0x00, 0x00, 0x00], UFrameType::StartDtAct),
            ([0x68, 0x04, 0x0B, 0x00, 0x00, 0x00], UFrameType::StartDtCon),
            ([0x68, 0x04, 0x13, 0x00, 0x00, 0x00], UFrameType::StopDtAct),
            ([0x68, 0x04, 0x23, 0x00, 0x00, 0x00], UFrameType::StopDtCon),
            ([0x68, 0x04, 0x43, 0x00, 0x00, 0x00], UFrameType::TestFrAct),
            ([0x68, 0x04, 0x83, 0x00, 0x00, 0x00], UFrameType::TestFrCon),
        ];

        for (data, expected_type) in test_cases {
            let frame = Iec104Frame::parse(&data).unwrap();
            assert_eq!(
                frame.u_frame_type(),
                Some(expected_type),
                "U frame type mismatch for {:?}",
                data
            );
        }
    }

    #[test]
    fn test_u_frame_make() {
        // 测试 make_u_frame 生成正确格式
        let frame_data = Iec104Frame::make_u_frame(UFrameType::TestFrAct);
        assert_eq!(frame_data, vec![0x68, 0x04, 0x43, 0x00, 0x00, 0x00]);

        let frame_data = Iec104Frame::make_u_frame(UFrameType::StartDtCon);
        assert_eq!(frame_data, vec![0x68, 0x04, 0x0B, 0x00, 0x00, 0x00]);
    }

    // ========== S Frame Tests ==========

    #[test]
    fn test_s_frame_parse() {
        // S frame: 68 04 01 00 01 00
        let data = [0x68, 0x04, 0x01, 0x00, 0x01, 0x00];
        let frame = Iec104Frame::parse(&data).unwrap();
        assert_eq!(frame.frame_type, FrameType::SFrame);
    }

    #[test]
    fn test_s_frame_make() {
        // 测试 make_s_frame 生成正确格式（**单参签名**：只带接收序号，§9.2.3）
        let frame_data = Iec104Frame::make_s_frame(0);
        assert_eq!(frame_data.len(), 6);
        assert_eq!(frame_data[0], 0x68); // start byte
        assert_eq!(frame_data[1], 0x04); // length
        assert_eq!(frame_data[2], 0x01); // S frame identifier
    }

    /// **新增**（§9.2.3 受影响断言表）：S 帧第 3/4 字节 = `recv_seq << 1`（LE）；
    /// 第 3 字节恒 `0x01`、第 4 字节恒 `0x00`；**不再**把发送序号写进控制域。
    #[test]
    fn test_s_frame_make_bytes_carry_only_recv_seq() {
        // recv_seq = 5 ⇒ 68 04 01 00 0A 00
        assert_eq!(
            Iec104Frame::make_s_frame(5),
            vec![0x68, 0x04, 0x01, 0x00, 0x0A, 0x00]
        );
        // recv_seq = 0 ⇒ 68 04 01 00 00 00
        assert_eq!(
            Iec104Frame::make_s_frame(0),
            vec![0x68, 0x04, 0x01, 0x00, 0x00, 0x00]
        );
        // recv_seq = 1 ⇒ 68 04 01 00 02 00（原实现会写成 … 01 00 03 00，两处不合规）
        assert_eq!(
            Iec104Frame::make_s_frame(1),
            vec![0x68, 0x04, 0x01, 0x00, 0x02, 0x00]
        );
        // 15 位高位字节：recv_seq = 128 ⇒ 低字节 (128<<1)&0xFF = 0x00，高字节 1
        assert_eq!(
            Iec104Frame::make_s_frame(128),
            vec![0x68, 0x04, 0x01, 0x00, 0x00, 0x01]
        );
        // 15 位满量程：recv_seq = 32767 ⇒ 低字节 0xFE，高字节 0xFF
        assert_eq!(
            Iec104Frame::make_s_frame(32767),
            vec![0x68, 0x04, 0x01, 0x00, 0xFE, 0xFF]
        );

        // 解析回读：仍被识别为 S 帧（第 3 字节 0x01 是 S 帧标识）
        let parsed = Iec104Frame::parse(&Iec104Frame::make_s_frame(5)).unwrap();
        assert_eq!(parsed.frame_type, FrameType::SFrame);
        assert_eq!(parsed.control1, 0x01);
        assert_eq!(parsed.control2, 0x00);
    }

    // ========== I Frame Tests ==========

    #[test]
    fn test_i_frame_parse() {
        // I 帧格式: 68 <length> <send_seq_low> 00 <recv_seq_low> 00 <asdu...>
        let data = [0x68, 0x06, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00];
        let frame = Iec104Frame::parse(&data).unwrap();
        assert_eq!(frame.frame_type, FrameType::IFrame);
        assert_eq!(frame.start, 0x68);
        assert_eq!(frame.length, 0x06);
    }

    #[test]
    fn test_i_frame_sequence() {
        // I 帧带序号
        let send_seq = 5u16;
        let recv_seq = 3u16;
        let asdu = vec![0x01, 0x00]; // type_id and sq_num

        let frame_data = Iec104Frame::make_i_frame(send_seq, recv_seq, &asdu);
        let frame = Iec104Frame::parse(&frame_data).unwrap();

        assert_eq!(frame.send_sequence(), send_seq);
        assert_eq!(frame.recv_sequence(), recv_seq);
    }

    #[test]
    fn test_i_frame_make() {
        let send_seq = 10u16;
        let recv_seq = 5u16;
        let asdu = vec![0x0D, 0x00, 0x01, 0x00, 0x00]; // M_ME_NC_1 example

        let frame_data = Iec104Frame::make_i_frame(send_seq, recv_seq, &asdu);

        assert_eq!(frame_data[0], 0x68); // start byte
        assert_eq!(frame_data.len(), 6 + asdu.len()); // header + asdu
    }

    /// **新增**（§9.2.3 硬性验收 4）：把 `make_i_frame` 的**高位字节编码钉死 15 位全范围**
    /// —— 对 4 个代表值逐个断言 4 个控制字节的**期望值**（不是只测回绕一个点）。
    #[test]
    fn test_i_frame_make_15bit_control_bytes_full_range() {
        // (send_seq, recv_seq, 期望 i1..i4)
        let cases: &[(u16, u16, [u8; 4])] = &[
            // N(S) = 0x7FFF（15 位满量程）：i1 = (0x7FFF<<1)&0xFE = 0xFE，i2 = 0x7FFF>>7 = 0xFF
            (0x7FFF, 0x7FFF, [0xFE, 0xFF, 0xFE, 0xFF]),
            // N(S) = 0x7F80：低位字节被 &0xFE 吃掉 ⇒ 0x00，高位字节 0xFF
            (0x7F80, 0x7F80, [0x00, 0xFF, 0x00, 0xFF]),
            // N(S) = 0x0100：i1 = (0x100<<1)&0xFE = 0x00，i2 = 0x100>>7 = 0x02
            (0x0100, 0x0100, [0x00, 0x02, 0x00, 0x02]),
            // N(S) = 0x00FF：i1 = (0xFF<<1)&0xFE = 0xFE，i2 = 0xFF>>7 = 0x01
            (0x00FF, 0x00FF, [0xFE, 0x01, 0xFE, 0x01]),
            // 发/收序号可不同（低 7 位 + 高 8 位分列两个字节）
            (0x00FF, 0x0100, [0xFE, 0x01, 0x00, 0x02]),
            // 0x00FE ⇒ i1 = (0xFE<<1)&0xFE = 0xFC（bit0 恒 0：I 帧标识），i2 = 0x01
            (0x00FE, 0x0000, [0xFC, 0x01, 0x00, 0x00]),
            // 0x0080 ⇒ 旧实现的**首个断裂值**：i1 = 0x00、i2 = 0x01（旧实现恒写 0x00 ⇒ 读回 0）
            (0x0080, 0x0000, [0x00, 0x01, 0x00, 0x00]),
        ];

        for (s, r, expect) in cases {
            let f = Iec104Frame::make_i_frame(*s, *r, &[0x0D, 0x00, 0x01, 0x00, 0x00]);
            assert_eq!(
                [f[2], f[3], f[4], f[5]],
                *expect,
                "send_seq={:#06x} recv_seq={:#06x} 的控制字节不符",
                s,
                r
            );
            // 控制域 bit0 必须为 0（I 帧标识）
            assert_eq!(f[2] & 0x01, 0x00);
        }
    }

    /// **新增**（§9.2.3 受影响断言表）：`seq ≥ 128` 与 `32767` 回绕的**往返**用例（回归钉）。
    #[test]
    fn test_i_frame_sequence_roundtrip_ge_128_and_wrap() {
        // ≥ 64 之后（旧实现从 128 起回绕到 0）——这些值在旧编码下全部读回错误
        for seq in [
            63u16, 64, 127, 128, 129, 255, 256, 1024, 16383, 16384, 32766, 32767,
        ] {
            let f = Iec104Frame::make_i_frame(seq, seq, &[0x0D, 0x00, 0x01, 0x00, 0x00]);
            let parsed = Iec104Frame::parse(&f).unwrap();
            assert_eq!(parsed.send_sequence(), seq, "send_seq={} 往返失败", seq);
            assert_eq!(parsed.recv_sequence(), seq, "recv_seq={} 往返失败", seq);
        }

        // 回绕：15 位取模 —— 0x8000（32767 之后一格）与 0 编码同物
        assert_eq!(
            Iec104Frame::make_i_frame(0x8000, 0, &[0x01]),
            Iec104Frame::make_i_frame(0, 0, &[0x01])
        );
        assert_eq!(
            Iec104Frame::make_i_frame(0xFFFF, 0, &[0x01]),
            Iec104Frame::make_i_frame(0x7FFF, 0, &[0x01])
        );
        // 回绕边界（旧实现的真实断裂点 = 128，**不是**设计原文写的 64）：
        // 127 在旧编码下亦正确，128 起回绕 ⇒ 显式钉住两侧
        let f127 = Iec104Frame::make_i_frame(127, 0, &[0x01]);
        let f128 = Iec104Frame::make_i_frame(128, 0, &[0x01]);
        assert_eq!([f127[2], f127[3]], [0xFE, 0x00]);
        assert_eq!([f128[2], f128[3]], [0x00, 0x01]);
    }

    // ========== ASDU Header Tests ==========

    #[test]
    fn test_parse_asdu_header() {
        // ASDU: TypeID(1) + SQNUM(1) + COT(1) + ORIG_ADDR(2) + ...
        let asdu = vec![0x0D, 0x00, 0x01, 0x00, 0x00];
        let frame = Iec104Frame::make_i_frame(0, 0, &asdu);
        let parsed = Iec104Frame::parse(&frame).unwrap();
        let header = parsed.parse_asdu_header().unwrap();

        assert_eq!(header.type_id, TypeId::MMeNc1);
        assert_eq!(header.sq_num, 0);
        assert_eq!(header.cot.0, Cot::PERIODIC);
    }

    #[test]
    fn test_parse_asdu_header_invalid() {
        // 空的 ASDU 应该失败
        let frame = Iec104Frame::make_i_frame(0, 0, &[]);
        let parsed = Iec104Frame::parse(&frame).unwrap();
        let result = parsed.parse_asdu_header();
        assert!(result.is_err());
    }

    // ========== Frame Parsing Error Tests ==========

    #[test]
    fn test_parse_invalid_start_byte() {
        // 不是 0x68 起始字符
        let data = [0x69, 0x04, 0x07, 0x00, 0x00, 0x00];
        let result = Iec104Frame::parse(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_frame_too_short() {
        // 帧太短
        let data = [0x68, 0x04];
        let result = Iec104Frame::parse(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_length_too_small() {
        // length < 4 的恶意帧不应 panic（此前 (length as usize) - 4 下溢），应返回错误
        let data = [0x68, 0x02, 0x07, 0x00, 0x00, 0x00]; // length=2 < 4
        let result = Iec104Frame::parse(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_length_mismatch() {
        // 声明的长度与实际数据不匹配
        let data = [0x68, 0x10, 0x07, 0x00, 0x00, 0x00]; // length=16 but data only 6
        let result = Iec104Frame::parse(&data);
        assert!(result.is_err());
    }

    // ========== Ioa Tests ==========

    #[test]
    fn test_ioa_new_and_value() {
        let ioa = Ioa::new(0x12, 0x34, 0x56);
        assert_eq!(ioa.value(), 0x563412);
    }

    #[test]
    fn test_ioa_value_calculation() {
        let ioa = Ioa::new(0x00, 0x00, 0x01);
        assert_eq!(ioa.value(), 0x010000);

        let ioa = Ioa::new(0x01, 0x00, 0x00);
        assert_eq!(ioa.value(), 0x01);
    }

    // ========== CP56Time2a 与编码器 Tests（§9.2.4 / §9.5） ==========

    /// CP56Time2a：**UTC 编解码往返**（年域相对 2000，故可往返区间 = 2000-01-01 … 2099-12-31）。
    #[test]
    fn test_cp56time2a_roundtrip_utc() {
        for ts in [
            946_684_800_000u64, // 2000-01-01T00:00:00Z（年域下界）
            1_726_185_600_000,  // 2024-09-13T00:00:00Z
            1_790_253_296_789,  // 2026-09-24T12:34:56.789Z（含毫秒）
            1_790_000_000_000,  // 2026 年
            4_100_000_000_000,  // 2099 年（年域上界内）
        ] {
            let back = decode_cp56time2a(&encode_cp56time2a(ts));
            assert_eq!(back, Some(ts), "ts={} 往返失败（得 {:?}）", ts, back);
        }
    }

    /// §9.2.5 的回归钉：注入 `ts = 1000`（1970-01-01T00:00:01Z）的点时，帧内 CP56Time2a
    /// 必须**逐字节等于** `encode_cp56time2a(1000)`。
    ///
    /// ⚠️ 年域按设计字面 `year - 2000`（只 1 字节）⇒ 1970 年编码为 `(1970-2000) mod 100 = 70`，
    /// 解回时落在 2070 年：**该年域的世纪归属不由本模块裁定**（设计只有"= year-2000"一句），
    /// 故本用例对 1970 时标按**字节**钉，对 2000+ 时标按**往返**钉（见上一用例）。
    #[test]
    fn test_cp56time2a_ts_1000_regression_anchor() {
        let b = encode_cp56time2a(1_000);
        assert_eq!([b[0], b[1]], [0xE8, 0x03]); // 首字段 = 秒(1)×1000 + 毫秒(0) = 1000
        assert_eq!(b[2], 0); // 分
        assert_eq!(b[3], 0); // 时
        assert_eq!(b[4] & 0x1F, 1); // 日 = 1
        assert_eq!(b[4] >> 5, 4); // 星期四
        assert_eq!(b[5], 1); // 月 = 1
        assert_eq!(b[6], 70); // 年 = (1970 - 2000) mod 100
    }

    /// CP56Time2a 字节级：秒与毫秒**同处首字段**（`秒×1000 + 毫秒`），年月日/时分逐字节可核。
    #[test]
    fn test_cp56time2a_bytes_utc() {
        // 2026-09-24T12:34:56.789Z = 1790253296789 ms
        let ts = 1_790_253_296_789u64;
        let b = encode_cp56time2a(ts);
        // 首字段 = 秒(56)×1000 + 毫秒(789) = 56789 = 0xDDD5 ⇒ LE 0xD5 0xDD
        assert_eq!([b[0], b[1]], [0xD5, 0xDD]);
        assert_eq!(b[2], 34); // 分
        assert_eq!(b[3], 12); // 时
        assert_eq!(b[4] & 0x1F, 24); // 日
        assert_eq!(b[4] >> 5, 4); // 星期四（2026-09-24 为周四）
        assert_eq!(b[5], 9); // 月
        assert_eq!(b[6], 26); // 年（= year - 2000）

        // 时标无效位（bit7 of 分）恒为 0
        assert_eq!(b[2] & 0x80, 0x00);
        assert_eq!(decode_cp56time2a(&b), Some(ts));

        // 字节不足 / 字段越界 ⇒ None（不 panic）
        assert_eq!(decode_cp56time2a(&b[..6]), None);
        assert_eq!(decode_cp56time2a(&[0, 0, 0, 0, 0, 0, 0][..7]), None); // 月 = 0 越界
    }

    /// CP56Time2a 的 **ISO 星期域（`DOW ∈ 1..=7`，IEC 60870-5-4）**：周一 / 周三 / 周四**逐字节**钉。
    ///
    /// 旧写法 `(days.rem_euclid(7) + 3) + 1` 之后再 `& 0x07` ⇒ 周一 = 0 / 周二 = 1 / 周三 = 2
    /// （**非法 DOW**）。既有两个 CP56Time2a 用例只取**周四**（1970-01-01 与 2026-09-24 皆为周四）
    /// ⇒ 恰好落在 4 而全绿漏检；本用例补周一/周三两个断裂点。
    #[test]
    fn test_cp56time2a_iso_weekday_bytes() {
        // 1970-01-01 为周四 ⇒ 4（epoch 基准锚）
        assert_eq!(encode_cp56time2a(0)[4] >> 5, 4, "1970-01-01 必须为周四");
        // 2026-09-24 为周四 ⇒ 4（既有用例同款，防回归）
        assert_eq!(encode_cp56time2a(1_790_253_296_789)[4] >> 5, 4);
        // 2026-09-28 为周一 ⇒ 1（旧写法得 0）
        let mon = encode_cp56time2a(1_790_553_600_000);
        assert_eq!([mon[4] & 0x1F, mon[4] >> 5], [28, 1], "2026-09-28 = 周一");
        // 2026-09-30 为周三 ⇒ 3（旧写法得 2）
        let wed = encode_cp56time2a(1_790_726_400_000);
        assert_eq!([wed[4] & 0x1F, wed[4] >> 5], [30, 3], "2026-09-30 = 周三");
    }

    /// CP56Time2a 星期域的**范围不变量**：连续 7 天各编码一次，结果恰为 `{1,2,3,4,5,6,7}`
    /// （不越界、不重复）——钉住"绝不发出 0"这个不变量，防将来再引入同类掩码错。
    #[test]
    fn test_cp56time2a_iso_weekday_covers_1_to_7_over_seven_days() {
        const DAY_MS: u64 = 86_400_000;
        let start = 1_790_553_600_000u64; // 2026-09-28（周一）
        let mut got: Vec<u8> = (0..7)
            .map(|i| encode_cp56time2a(start + i * DAY_MS)[4] >> 5)
            .collect();
        got.sort_unstable();
        assert_eq!(got, vec![1, 2, 3, 4, 5, 6, 7], "7 天必须恰覆盖 DOW 1..=7");
    }

    /// `M_ME_TF_1`(TI=36)：ASDU 19 B、帧长 25 B（§9.2.2 帧长表）。
    #[test]
    fn test_encode_me_tf1_bytes_and_frame_length_25() {
        let asdu = encode_me_tf1(301, 12.5, 1_790_253_296_789, COT_INTROGEN);
        assert_eq!(asdu.len(), 19);
        assert_eq!(asdu[0], 36); // TypeID
        assert_eq!(asdu[1], 0x01); // VSQ = 1（单对象）
        assert_eq!(asdu[2], COT_INTROGEN); // COT = 20
        assert_eq!([asdu[3], asdu[4]], [0x00, 0x00]); // CA
        assert_eq!([asdu[5], asdu[6], asdu[7]], [0x2D, 0x01, 0x00]); // IOA=301 LE
        assert_eq!(asdu[8..12], 12.5f32.to_le_bytes()); // 短浮点 LE
        assert_eq!(decode_cp56time2a(&asdu[12..19]), Some(1_790_253_296_789)); // 时标 = 采集时刻

        let frame = Iec104Frame::make_i_frame(0, 0, &asdu);
        assert_eq!(frame.len(), 25);
        assert_eq!(frame[1], 23); // length = 4 + ASDU 19
    }

    /// `M_SP_TB_1`(TI=30)：ASDU 16 B、帧长 22 B（§9.2.2 帧长表）。
    #[test]
    fn test_encode_sp_tb1_bytes_and_frame_length_22() {
        let asdu = encode_sp_tb1(315, true, 1_000, COT_INTROGEN);
        assert_eq!(asdu.len(), 16);
        assert_eq!(asdu[0], 30); // TypeID（= MSpTa1 = MSpTb1）
        assert_eq!(asdu[2], COT_INTROGEN);
        assert_eq!([asdu[5], asdu[6], asdu[7]], [0x3B, 0x01, 0x00]); // IOA=315 LE
        assert_eq!(asdu[8], 0x01); // SIQ：SPI = 1
        assert_eq!(asdu[9..16], encode_cp56time2a(1_000)); // 时标逐字节 = 采集时刻

        let asdu0 = encode_sp_tb1(315, false, 1_000, COT_INTROGEN);
        assert_eq!(asdu0[8], 0x00);

        let frame = Iec104Frame::make_i_frame(0, 0, &asdu);
        assert_eq!(frame.len(), 22);
        assert_eq!(frame[1], 20); // length = 4 + ASDU 16
    }

    /// `C_IC_NA_1`(TI=100) 的 ACT_CON / ACT_TERM 空应答：IOA=0 + QOI=0，ASDU 9 B。
    #[test]
    fn test_encode_ic_term_bytes() {
        let act_con = encode_ic_term(COT_ACT_CON);
        assert_eq!(act_con.len(), 9);
        assert_eq!(act_con[0], 100);
        assert_eq!(act_con[2], COT_ACT_CON);
        assert_eq!(&act_con[5..9], &[0x00, 0x00, 0x00, 0x00]);

        let act_term = encode_ic_term(COT_ACT_TERM);
        assert_eq!(act_term[2], COT_ACT_TERM);

        // 编出的 ASDU 能被本仓的 ASDU 头解析器认出（TypeID 100 已落地）
        let frame = Iec104Frame::make_i_frame(0, 0, &act_con);
        let header = Iec104Frame::parse(&frame)
            .unwrap()
            .parse_asdu_header()
            .unwrap();
        assert_eq!(header.type_id, TypeId::CIcNa1);
        assert_eq!(header.cot.0, COT_ACT_CON);
    }
}
