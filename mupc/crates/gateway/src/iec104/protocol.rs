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
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum TypeId {
    // 监视方向 (Monitoring)
    MSpNa1 = 1,  // 单点遥信 (M_SP_NA_1)
    MDpNa1 = 3,  // 双点遥信 (M_DP_NA_1)
    MMeNa1 = 9,  // 测量值，归一化值 (M_ME_NA_1)
    MMeNc1 = 13, // 测量值，短浮点数 (M_ME_NC_1)
    MSpTa1 = 30, // 单点遥信带时标 (M_SP_TA_1)
    MDpTa1 = 31, // 双点遥信带时标 (M_DP_TA_1)
    MMeTa1 = 34, // 测量值带时标，归一化值 (M_ME_TA_1)
    MMeTd1 = 35, // 测量值带时标，归一化值 (M_ME_TD_1)
    // 控制方向 (Control)
    CScNa1 = 45, // 单点遥控 (C_SC_NA_1)
    CDcNa1 = 46, // 双点遥控 (C_DC_NA_1)
    CSeNa1 = 48, // 调节命令 (C_SE_NA_1)
    CScTa1 = 58, // 单点遥控带时标 (C_SC_TA_1)
    CDcTa1 = 59, // 双点遥控带时标 (C_DC_TA_1)
    CSeTa1 = 61, // 调节命令带时标 (C_SE_TA_1)
}

impl TypeId {
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
            45 => Some(TypeId::CScNa1),
            46 => Some(TypeId::CDcNa1),
            48 => Some(TypeId::CSeNa1),
            58 => Some(TypeId::CScTa1),
            59 => Some(TypeId::CDcTa1),
            61 => Some(TypeId::CSeTa1),
            _ => None,
        }
    }
}

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
    fn determine_frame_type(c1: u8, c2: u8, c3: u8, c4: u8) -> FrameType {
        // U 帧: 3 字节全为 0x07 或 0x13
        if c1 == 0x07 && c2 == 0x00 && c3 == 0x07 && c4 == 0x00 {
            return FrameType::UFrame;
        }
        if c1 == 0x13 && c2 == 0x00 && c3 == 0x13 && c4 == 0x00 {
            return FrameType::UFrame;
        }

        // S 帧: 第 1 字节为 0x01
        if c1 == 0x01 {
            return FrameType::SFrame;
        }

        // I 帧: 其他情况
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
    pub fn make_s_frame(send_seq: u16, recv_seq: u16) -> Vec<u8> {
        let s1 = ((send_seq * 2) & 0xFE) as u8;
        let s2 = 0x00;
        let s3 = (((recv_seq * 2) + 1) & 0xFE) as u8;
        let s4 = 0x00;

        vec![0x68, 0x04, s1, s2, s3, s4]
    }

    /// 创建 I 帧
    pub fn make_i_frame(send_seq: u16, recv_seq: u16, asdu: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.push(0x68);

        let length = 4 + asdu.len();
        frame.push(length as u8);

        let i1 = ((send_seq * 2) & 0xFE) as u8;
        let i2 = 0x00;
        let i3 = (((recv_seq * 2) + 1) & 0xFE) as u8;
        let i4 = 0x00;

        frame.push(i1);
        frame.push(i2);
        frame.push(i3);
        frame.push(i4);
        frame.extend_from_slice(asdu);

        frame
    }

    /// 获取发送序号（从 I 帧）
    pub fn send_sequence(&self) -> u16 {
        ((self.control1 >> 1) & 0x7F) as u16 | (((self.control2 & 0x7F) as u16) << 7)
    }

    /// 获取接收序号（从 I 帧）
    pub fn recv_sequence(&self) -> u16 {
        ((self.control3 >> 1) & 0x7F) as u16 | (((self.control4 & 0x7F) as u16) << 7)
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
        assert_eq!(TypeId::from_u8(2), None);
        assert_eq!(TypeId::from_u8(100), None);
        assert_eq!(TypeId::from_u8(255), None);
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
        // 测试 make_s_frame 生成正确格式
        let frame_data = Iec104Frame::make_s_frame(0, 0);
        assert_eq!(frame_data.len(), 6);
        assert_eq!(frame_data[0], 0x68); // start byte
        assert_eq!(frame_data[1], 0x04); // length
        assert_eq!(frame_data[2], 0x01); // S frame identifier
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
}
