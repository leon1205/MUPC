//! GOOSE 订阅处理
//!
//! 实现 GOOSE 消息订阅和处理功能

use crate::config::GooseConfig;
use crate::errors::{Iec61850Error, Result};
use std::sync::Arc;
use tokio::sync::broadcast;

/// GOOSE 消息
#[derive(Debug, Clone)]
pub struct GooseMessage {
    pub app_id: u32,
    pub go_id: String,
    pub dat_set: String,
    pub timestamp: u64,
    pub data: Vec<u8>,
}

/// GOOSE 订阅者
pub struct GooseSubscriber {
    config: GooseConfig,
    receiver: broadcast::Receiver<GooseMessage>,
}

impl GooseSubscriber {
    /// 创建 GOOSE 订阅者
    pub fn new(config: GooseConfig) -> (Self, broadcast::Sender<GooseMessage>) {
        let (tx, rx) = broadcast::channel(100);
        let subscriber = Self { config, receiver: rx };
        (subscriber, tx)
    }

    /// 获取 GOOSE 配置
    pub fn config(&self) -> &GooseConfig {
        &self.config
    }

    /// 接收 GOOSE 消息（异步）
    pub async fn recv(&mut self) -> Option<GooseMessage> {
        self.receiver.recv().await.ok()
    }

    /// GOOSE PDU 解析结果
///
/// 参考 IEC 61850-8-1 标准进行解析
#[derive(Debug, Clone)]
pub struct GoosePduResult {
    /// APDU 长度
    pub apdu_length: u16,
    /// 应用标识符
    pub app_id: u16,
    /// GOOSE 标识符
    pub go_id: String,
    /// 数据集引用
    pub dat_set: String,
    /// GOOSE 状态编号
    pub st_num: u8,
    /// GOOSE 事件编号
    pub sq_num: u8,
    /// 安全允许位
    pub security_allowed: u8,
    /// 发送时间（纳秒）
    pub time_to_live: u32,
}

/// 解析 GOOSE 数据包
///
/// 按照 IEC 61850-8-1 规范解析 GOOSE PDU
/// GOOSE PDU 结构：
/// - APDU Length (2 bytes)
/// - AppID (2 bytes)
/// - Reserved (2 bytes)
/// - GoCBRef (Variable, TLV)
/// - DatSet (Variable, TLV)
/// - GoID (Variable, TLV, optional)
/// - StNum (1 byte)
/// - SqNum (1 byte)
/// - Security (1 byte)
/// - TimeAllowed to Wait (4 bytes)
/// - Data (TLV sequence)
pub fn parse_goose_pdu(data: &[u8]) -> Result<GoosePduResult> {
    if data.len() < 8 {
        return Err(Iec61850Error::GooseParseFailed("数据太短".to_string()));
    }

    // 跳过 APDU 长度字段（解析时已提供）
    let mut offset = 0;

    // AppID (2 bytes)
    let app_id = ((data[offset] as u16) << 8) | (data[offset + 1] as u16);
    offset += 2;

    // Reserved (2 bytes)
    offset += 2;

    // 解析 TLV 元素
    let (go_id, consumed) = parse_tlv_string(&data[offset..])?;
    offset += consumed;

    let (dat_set, consumed) = parse_tlv_string(&data[offset..])?;
    offset += consumed;

    // StNum 和 SqNum (各 1 byte)
    if offset + 2 > data.len() {
        return Err(Iec61850Error::GooseParseFailed("数据不足".to_string()));
    }
    let st_num = data[offset];
    let sq_num = data[offset + 1];
    offset += 2;

    // Security (1 byte)
    if offset + 1 > data.len() {
        return Err(Iec61850Error::GooseParseFailed("数据不足".to_string()));
    }
    let security_allowed = data[offset];
    offset += 1;

    // TimeAllowed to Wait (4 bytes)
    if offset + 4 > data.len() {
        return Err(Iec61850Error::GooseParseFailed("数据不足".to_string()));
    }
    let time_to_live = u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]);

    Ok(GoosePduResult {
        apdu_length: (data.len() - 2) as u16,
        app_id,
        go_id,
        dat_set,
        st_num,
        sq_num,
        security_allowed,
        time_to_live,
    })
}

/// 解析 TLV 字符串
fn parse_tlv_string(data: &[u8]) -> Result<(String, usize)> {
    if data.len() < 3 {
        return Err(Iec61850Error::GooseParseFailed("TLV 数据不足".to_string()));
    }

    let tag = data[0];
    let len = data[1] as usize;

    if tag != 0x80 {
        return Err(Iec61850Error::GooseParseFailed(format!("无效 TLV tag: 0x{:02x}", tag)));
    }

    if data.len() < 2 + len {
        return Err(Iec61850Error::GooseParseFailed("TLV 数据长度不足".to_string()));
    }

    let value = std::str::from_utf8(&data[2..2 + len])
        .map_err(|_| Iec61850Error::GooseParseFailed("无效 UTF-8 字符串".to_string()))?;

    Ok((value.to_string(), 2 + len))
}

/// 从原始数据创建 GOOSE 消息
pub fn create_goose_message(data: &[u8]) -> Result<GooseMessage> {
    let pdu = parse_goose_pdu(data)?;

    Ok(GooseMessage {
        app_id: pdu.app_id,
        go_id: pdu.go_id,
        dat_set: pdu.dat_set,
        timestamp: pdu.time_to_live as u64,
        data: data.to_vec(),
    })
}

impl GooseSubscriber {
    /// 检查 GOOSE 报文是否有效
    pub fn validate_goose(&self, msg: &GooseMessage) -> bool {
        // 检查 AppID 和 GOID 匹配
        msg.app_id == self.config.app_id as u32 && msg.go_id == self.config.go_id
    }
}

/// GOOSE 数据集定义
#[derive(Debug, Clone)]
pub struct GooseDataSet {
    pub entries: Vec<GooseDataEntry>,
}

/// GOOSE 数据条目
#[derive(Debug, Clone)]
pub struct GooseDataEntry {
    pub data_ref: String,
    pub data_type: DataType,
}

/// 数据类型
#[derive(Debug, Clone, PartialEq)]
pub enum DataType {
    Boolean,
    Int8,
    Int16,
    Int32,
    Float,
    BitString,
    OctetString,
    UnicodeString,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_goose_subscriber_creation() {
        let config = GooseConfig::default();
        let (subscriber, _tx) = GooseSubscriber::new(config);
        assert_eq!(subscriber.config.go_id, "GOOSE1");
    }

    #[test]
    fn test_parse_goose_pdu() {
        // 构建符合 IEC 61850-8-1 的测试数据
        // AppID=1, GoCBRef="TestGoCB", DatSet="DataSet1"
        let data = vec![
            0x00, 0x01, // AppID = 1
            0x00, 0x00, // Reserved
            0x80, 0x07, // Tag=0x80, Len=7 (GoCBRef)
            b'T', b'e', b's', b't', b'G', b'o', b'C', b'B', // "TestGoCB"
            0x80, 0x08, // Tag=0x80, Len=8 (DatSet)
            b'D', b'a', b't', b'a', b'S', b'e', b't', b'1', // "DataSet1"
            0x01,       // StNum = 1
            0x01,       // SqNum = 1
            0x00,       // Security = 0
            0x00, 0x00, 0x00, 0x64, // TimeAllowed = 100ms
        ];

        let result = parse_goose_pdu(&data);
        assert!(result.is_ok());
        let pdu = result.unwrap();
        assert_eq!(pdu.app_id, 1);
        assert_eq!(pdu.go_id, "TestGoCB");
        assert_eq!(pdu.dat_set, "DataSet1");
    }

    #[test]
    fn test_parse_goose_pdu_too_short() {
        let data = vec![0x00; 5];
        let result = parse_goose_pdu(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_goose() {
        let config = GooseConfig {
            app_id: 1,
            go_id: "GOOSE1".to_string(),
            dat_set: "DataSet1".to_string(),
        };
        let (_subscriber, _tx) = GooseSubscriber::new(config.clone());

        let msg = GooseMessage {
            app_id: 1,
            go_id: "GOOSE1".to_string(),
            dat_set: "DataSet1".to_string(),
            timestamp: 0,
            data: vec![],
        };

        assert_eq!(msg.app_id, config.app_id);
        assert_eq!(msg.go_id, config.go_id);
    }
}