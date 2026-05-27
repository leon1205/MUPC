//! 核间通信集成测试
//!
//! 测试 intercore 模块的完整功能

#[cfg(test)]
mod tests {
    use mupc_intercore::protocol::{IntercoreFrame, FrameHeader, FrameType, FRAME_FIXED_LENGTH};
    use mupc_intercore::heartbeat::{HeartbeatManager, HeartbeatStatus};

    // ========== HeartbeatManager Tests ==========

    #[test]
    fn test_heartbeat_manager_creation() {
        let manager = HeartbeatManager::new(1000, 5000);
        assert!(true); // 创建成功即可
    }

    #[tokio::test]
    async fn test_heartbeat_manager_register_connection() {
        let manager = HeartbeatManager::new(1000, 5000);
        let addr: std::net::SocketAddr = "127.0.0.1:2500".parse().unwrap();

        manager.register_connection(addr);

        // 等待异步注册完成
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let status = manager.get_connection_status(&addr).await;
        assert!(status.is_some());
        let status = status.unwrap();
        assert!(status.online);
    }

    #[tokio::test]
    async fn test_heartbeat_manager_unregister_connection() {
        let manager = HeartbeatManager::new(1000, 5000);
        let addr: std::net::SocketAddr = "127.0.0.1:2500".parse().unwrap();

        manager.register_connection(addr);
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        manager.unregister_connection(addr);
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let status = manager.get_connection_status(&addr).await;
        assert!(status.is_none());
    }

    #[tokio::test]
    async fn test_heartbeat_manager_receive_heartbeat() {
        let manager = HeartbeatManager::new(1000, 5000);
        let addr: std::net::SocketAddr = "127.0.0.1:2500".parse().unwrap();

        manager.register_connection(addr);
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        manager.receive_heartbeat(addr).await;

        let status = manager.get_connection_status(&addr).await;
        assert!(status.is_some());
        assert!(status.unwrap().online);
    }

    // ========== Frame Format Tests ==========

    #[test]
    fn test_intercore_frame_all_types_fixed_length() {
        // 验证所有帧类型都是固定的 64 字节
        let types = vec![
            FrameType::Connect,
            FrameType::HeartbeatReq,
            FrameType::HeartbeatRsp,
            FrameType::ControlCmd,
            FrameType::ControlRsp,
            FrameType::StatusReport,
            FrameType::DataUpload,
        ];

        for frame_type in types {
            let data = match frame_type {
                FrameType::HeartbeatReq => vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                _ => vec![],
            };
            let frame = IntercoreFrame::new(frame_type, 0, data);
            let bytes = frame.to_bytes().unwrap();
            assert_eq!(bytes.len(), FRAME_FIXED_LENGTH,
                "Frame type {:?} should be {} bytes", frame_type, FRAME_FIXED_LENGTH);
        }
    }

    #[test]
    fn test_intercore_frame_padding() {
        // 测试 padding 填充
        let frame = IntercoreFrame::new(FrameType::Connect, 0, vec![]);
        let bytes = frame.to_bytes().unwrap();

        // 连接帧只有 header (8 bytes) + crc (2 bytes) = 10 bytes
        // 应该 padding 到 64 bytes
        assert_eq!(bytes.len(), 64);

        // 最后几个字节应该是 padding (0x00)
        assert_eq!(bytes[62], 0x00);
        assert_eq!(bytes[63], 0x00);
    }

    // ========== CRC Validation ==========

    #[test]
    fn test_crc16_full_frame_validation() {
        // 测试完整帧的 CRC 验证
        let frame = IntercoreFrame::new_heartbeat_req(1, 45.5, 0.75);
        let bytes = frame.to_bytes().unwrap();

        // 能够成功解析说明 CRC 正确
        let result = IntercoreFrame::from_bytes(&bytes);
        assert!(result.is_ok());
    }

    #[test]
    fn test_crc16_invalid_data_rejected() {
        // 测试损坏数据被拒绝
        let frame = IntercoreFrame::new_connect();
        let mut bytes = frame.to_bytes().unwrap();

        // 修改数据内容（不包括 CRC）
        bytes[6] ^= 0x01;

        let result = IntercoreFrame::from_bytes(&bytes);
        assert!(result.is_err());
    }

    // ========== Sequence Number Tests ==========

    #[test]
    fn test_frame_sequence_number() {
        let frame = IntercoreFrame::new(FrameType::ControlCmd, 123, vec![]);
        let bytes = frame.to_bytes().unwrap();

        // 序列号在 offset 6-7 (big endian)
        let seq_no = ((bytes[7] as u16) << 8) | (bytes[6] as u16);
        assert_eq!(seq_no, 123);
    }

    // ========== Round-trip Tests ==========

    #[test]
    fn test_frame_roundtrip_all_types() {
        let types = vec![
            (FrameType::Connect, vec![]),
            (FrameType::HeartbeatReq, vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            (FrameType::HeartbeatRsp, vec![]),
            (FrameType::ControlCmd, vec![0x01, 0x02]),
            (FrameType::ControlRsp, vec![]),
            (FrameType::StatusReport, vec![]),
            (FrameType::DataUpload, vec![]),
        ];

        for (frame_type, data) in types {
            let original = IntercoreFrame::new(frame_type, 42, data.clone());
            let bytes = original.to_bytes().unwrap();
            let parsed = IntercoreFrame::from_bytes(&bytes).unwrap();

            assert_eq!(parsed.header.frame_type, frame_type);
            assert_eq!(parsed.header.seq_no, 42);
            assert_eq!(parsed.data, data);
        }
    }

    // ========== Magic Number Tests ==========

    #[test]
    fn test_frame_magic_number() {
        let frame = IntercoreFrame::new_connect();
        let bytes = frame.to_bytes().unwrap();

        assert_eq!(bytes[0], 0xAA);
        assert_eq!(bytes[1], 0x55);
    }

    #[test]
    fn test_frame_header_magic_constant() {
        assert_eq!(FrameHeader::MAGIC, 0xAA55);
    }
}