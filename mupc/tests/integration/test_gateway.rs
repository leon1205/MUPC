//! IEC 104 网关集成测试
//!
//! 测试 gateway 和 intercore 的协作

#[cfg(test)]
mod tests {
    use mupc_gateway::iec104::{
        Iec104Frame, FrameType, UFrameType, TypeId, Cot,
    };
    use mupc_intercore::protocol::{IntercoreFrame, FrameType as IntercoreFrameType};

    // ========== IEC 104 Frame Creation and Parsing ==========

    #[test]
    fn test_iec104_u_frame_roundtrip() {
        // 测试 U 帧的创建和解析
        let frame_data = Iec104Frame::make_u_frame(UFrameType::StartDtAct);
        let frame = Iec104Frame::parse(&frame_data).unwrap();

        assert_eq!(frame.frame_type, FrameType::UFrame);
        assert_eq!(frame.u_frame_type(), Some(UFrameType::StartDtAct));
    }

    #[test]
    fn test_iec104_s_frame_roundtrip() {
        // 测试 S 帧的创建和解析
        let frame_data = Iec104Frame::make_s_frame(5, 3);
        let frame = Iec104Frame::parse(&frame_data).unwrap();

        assert_eq!(frame.frame_type, FrameType::SFrame);
    }

    #[test]
    fn test_iec104_i_frame_with_asdu() {
        // 测试 I 帧带 ASDU
        let asdu = vec![0x0D, 0x00, 0x01, 0x00, 0x00]; // M_ME_NC_1
        let frame_data = Iec104Frame::make_i_frame(0, 0, &asdu);
        let frame = Iec104Frame::parse(&frame_data).unwrap();

        assert_eq!(frame.frame_type, FrameType::IFrame);

        let header = frame.parse_asdu_header().unwrap();
        assert_eq!(header.type_id, TypeId::MMeNc1);
        assert_eq!(header.cot.0, Cot::PERIODIC);
    }

    // ========== Intercore Frame Creation and Parsing ==========

    #[test]
    fn test_intercore_connect_frame() {
        let frame = IntercoreFrame::new_connect();
        let bytes = frame.to_bytes().unwrap();

        assert_eq!(bytes.len(), 64);
        assert_eq!(bytes[0], 0xAA);
        assert_eq!(bytes[1], 0x55);

        let parsed = IntercoreFrame::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.header.frame_type, IntercoreFrameType::Connect);
    }

    #[test]
    fn test_intercore_heartbeat_frame() {
        let frame = IntercoreFrame::new_heartbeat_req(1, 45.5, 0.75);
        let bytes = frame.to_bytes().unwrap();

        assert_eq!(bytes.len(), 64);

        let parsed = IntercoreFrame::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.header.frame_type, IntercoreFrameType::HeartbeatReq);
    }

    // ========== Cross-Module协作测试 ==========

    #[test]
    fn test_gateway_and_intercore_independent_operation() {
        // 测试 gateway 和 intercore 可以独立工作

        // IEC 104 U 帧
        let iec_frame = Iec104Frame::make_u_frame(UFrameType::TestFrAct);
        assert_eq!(iec_frame.len(), 6);

        // Intercore 帧
        let intercore_frame = IntercoreFrame::new_connect();
        let intercore_bytes = intercore_frame.to_bytes().unwrap();
        assert_eq!(intercore_bytes.len(), 64);

        // 两者格式完全不同，互不影响
    }

    #[test]
    fn test_multiple_iec104_frames_sequence() {
        // 测试连续接收多个 IEC 104 帧

        // STARTDT_ACT
        let frame1 = Iec104Frame::make_u_frame(UFrameType::StartDtAct);
        let parsed1 = Iec104Frame::parse(&frame1).unwrap();
        assert_eq!(parsed1.u_frame_type(), Some(UFrameType::StartDtAct));

        // TESTFR_ACT
        let frame2 = Iec104Frame::make_u_frame(UFrameType::TestFrAct);
        let parsed2 = Iec104Frame::parse(&frame2).unwrap();
        assert_eq!(parsed2.u_frame_type(), Some(UFrameType::TestFrAct));

        // STOPDT_ACT
        let frame3 = Iec104Frame::make_u_frame(UFrameType::StopDtAct);
        let parsed3 = Iec104Frame::parse(&frame3).unwrap();
        assert_eq!(parsed3.u_frame_type(), Some(UFrameType::StopDtAct));
    }

    #[test]
    fn test_multiple_intercore_frames_sequence() {
        // 测试连续发送多个 Intercore 帧

        let frame1 = IntercoreFrame::new_connect();
        let bytes1 = frame1.to_bytes().unwrap();

        let frame2 = IntercoreFrame::new_heartbeat_req(0, 50.0, 0.8);
        let bytes2 = frame2.to_bytes().unwrap();

        let frame3 = IntercoreFrame::new_heartbeat_rsp();
        let bytes3 = frame3.to_bytes().unwrap();

        // 所有帧都是 64 字节
        assert_eq!(bytes1.len(), 64);
        assert_eq!(bytes2.len(), 64);
        assert_eq!(bytes3.len(), 64);

        // 可以正确解析
        assert!(IntercoreFrame::from_bytes(&bytes1).is_ok());
        assert!(IntercoreFrame::from_bytes(&bytes2).is_ok());
        assert!(IntercoreFrame::from_bytes(&bytes3).is_ok());
    }

    // ========== IEC 104 Connection Lifecycle Tests ==========

    #[test]
    fn test_iec104_connection_establishment_sequence() {
        // 测试 IEC 104 连接建立的完整握手序列

        // 1. 调度主站发送 STARTDT_ACT (启动数据传输激活)
        let startdt_act = Iec104Frame::make_u_frame(UFrameType::StartDtAct);
        let parsed1 = Iec104Frame::parse(&startdt_act).unwrap();
        assert_eq!(parsed1.u_frame_type(), Some(UFrameType::StartDtAct));

        // 2. 装置回复 STARTDT_CON (启动数据传输确认)
        let startdt_con = Iec104Frame::make_u_frame(UFrameType::StartDtCon);
        let parsed2 = Iec104Frame::parse(&startdt_con).unwrap();
        assert_eq!(parsed2.u_frame_type(), Some(UFrameType::StartDtCon));

        // 3. 双方进入数据传输状态，可以交换 I 帧
        let i_frame = Iec104Frame::make_i_frame(0, 0, &[0x0D, 0x00, 0x01, 0x00, 0x00]);
        let parsed3 = Iec104Frame::parse(&i_frame).unwrap();
        assert_eq!(parsed3.frame_type, FrameType::IFrame);
    }

    #[test]
    fn test_iec104_connection_disconnect_sequence() {
        // 测试 IEC 104 连接断开序列

        // 1. 调度主站发送 STOPDT_ACT (停止数据传输激活)
        let stopdt_act = Iec104Frame::make_u_frame(UFrameType::StopDtAct);
        let parsed1 = Iec104Frame::parse(&stopdt_act).unwrap();
        assert_eq!(parsed1.u_frame_type(), Some(UFrameType::StopDtAct));

        // 2. 装置回复 STOPDT_CON (停止数据传输确认)
        let stopdt_con = Iec104Frame::make_u_frame(UFrameType::StopDtCon);
        let parsed2 = Iec104Frame::parse(&stopdt_con).unwrap();
        assert_eq!(parsed2.u_frame_type(), Some(UFrameType::StopDtCon));

        // 3. 连接应该进入停止状态，不再处理 I 帧
        // 注意：STOPDT_CON 后连接可能仍保持 TCP 连接但不再传输数据
    }

    #[test]
    fn test_iec104_testfr_keepalive_sequence() {
        // 测试 TESTFR 心跳保活序列

        // 1. 发送 TESTFR_ACT
        let testfr_act = Iec104Frame::make_u_frame(UFrameType::TestFrAct);
        let parsed1 = Iec104Frame::parse(&testfr_act).unwrap();
        assert_eq!(parsed1.u_frame_type(), Some(UFrameType::TestFrAct));

        // 2. 收到 TESTFR_CON 表示对方存活
        let testfr_con = Iec104Frame::make_u_frame(UFrameType::TestFrCon);
        let parsed2 = Iec104Frame::parse(&testfr_con).unwrap();
        assert_eq!(parsed2.u_frame_type(), Some(UFrameType::TestFrCon));
    }

    // ========== Intercore Command Dispatch Tests ==========

    #[test]
    fn test_intercore_control_command_dispatch() {
        // 测试 intercore 控制指令下发

        // 构造控制命令帧
        let cmd_data = vec![
            0x01, // 命令类型: 开
            0x02, 0x00, // 设备ID: 2
            0x00,       // 保留
        ];
        let frame = IntercoreFrame::new(IntercoreFrameType::ControlCmd, 1, cmd_data.clone());
        let bytes = frame.to_bytes().unwrap();

        // 验证帧格式
        assert_eq!(bytes.len(), 64);
        assert_eq!(bytes[0], 0xAA);
        assert_eq!(bytes[1], 0x55);

        // 解析验证
        let parsed = IntercoreFrame::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.header.frame_type, IntercoreFrameType::ControlCmd);
        assert_eq!(parsed.header.seq_no, 1);
        assert_eq!(parsed.data, cmd_data);
    }

    #[test]
    fn test_intercore_control_response_handling() {
        // 测试 intercore 控制指令响应处理

        // 构造控制响应帧
        let rsp_data = vec![
            0x00, // 响应码: 成功
            0x01, 0x00, 0x00, // 执行时间戳
        ];
        let frame = IntercoreFrame::new(IntercoreFrameType::ControlRsp, 1, rsp_data.clone());
        let bytes = frame.to_bytes().unwrap();

        // 验证响应帧
        let parsed = IntercoreFrame::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.header.frame_type, IntercoreFrameType::ControlRsp);
        assert_eq!(parsed.header.seq_no, 1);
        assert_eq!(parsed.data[0], 0x00); // 成功响应
    }

    #[test]
    fn test_intercore_status_report_parsing() {
        // 测试 intercore 状态上报帧解析

        // 构造状态上报帧
        let status_data = vec![
            0x01, // 状态: 在线
            0x64, // CPU温度: 100
            0x4B, 0x00, 0x00, // 内存使用: 75%
        ];
        let frame = IntercoreFrame::new(IntercoreFrameType::StatusReport, 0, status_data.clone());
        let bytes = frame.to_bytes().unwrap();

        // 验证状态上报帧
        let parsed = IntercoreFrame::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.header.frame_type, IntercoreFrameType::StatusReport);
        assert_eq!(parsed.data[0], 0x01); // 在线状态
    }

    // ========== Error Frame Handling Tests ==========

    #[test]
    fn test_invalid_iec104_frame_rejected() {
        // 无效的 IEC 104 帧应该被拒绝
        let invalid_data = [0x69, 0x04, 0x07, 0x00, 0x00, 0x00]; // 错误起始字符
        let result = Iec104Frame::parse(&invalid_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_intercore_frame_rejected() {
        // 无效的 Intercore 帧应该被拒绝
        let mut invalid_data = vec![0u8; 64];
        invalid_data[0] = 0xFF; // 错误的 magic
        invalid_data[1] = 0xFF;

        let result = IntercoreFrame::from_bytes(&invalid_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_intercore_frame_rejected() {
        // 篡改的 Intercore 帧应该被拒绝
        let frame = IntercoreFrame::new_connect();
        let mut bytes = frame.to_bytes().unwrap();

        // 篡改数据
        bytes[4] ^= 0xFF;

        let result = IntercoreFrame::from_bytes(&bytes);
        assert!(result.is_err());
    }
}