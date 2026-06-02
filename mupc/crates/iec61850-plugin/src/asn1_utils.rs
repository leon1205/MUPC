//! ASN.1 BER/DER 编码/解码工具
//!
//! 实现 IEC 61850 MMS 协议的 ASN.1 数据编码

use crate::errors::{Iec61850Error, Result};
use crate::mms_types::{DataObject, MmsRequest, MmsResponse, MmsService};

/// MMS PDU 标签
mod pdu_tags {
    pub const CONFIRMED_REQUEST_PDU: u8 = 0x01;
    pub const CONFIRMED_RESPONSE_PDU: u8 = 0x02;
    pub const CONFIRMED_ERROR_PDU: u8 = 0x03;
    #[allow(dead_code)]
    pub const UNCONFIRMED_PDU: u8 = 0x04;
    pub const REJECTED_PDU: u8 = 0x05;
}

/// 编码 MMS 请求为 ASN.1 BER 格式
pub fn encode_mms_request(request: &MmsRequest) -> Result<Vec<u8>> {
    match request.service {
        MmsService::Read => encode_read_request(&request.object),
        MmsService::Write => encode_write_request(&request.object, &request.payload),
        MmsService::DefineVariableAccess => encode_define_request(&request.object),
        MmsService::GetDataAccessAttributes => encode_getda_request(&request.object),
    }
}

/// 解码 ASN.1 BER 响应
pub fn decode_mms_response(data: &[u8]) -> Result<MmsResponse> {
    if data.is_empty() {
        return Err(Iec61850Error::MmsInvalidResponse("空响应".into()));
    }

    // 检查 MMS PDU 类型
    match data[0] {
        pdu_tags::CONFIRMED_RESPONSE_PDU => Ok(MmsResponse {
            success: true,
            data: data.to_vec(),
            error: None,
        }),
        pdu_tags::CONFIRMED_ERROR_PDU => {
            Err(Iec61850Error::MmsProtocolError("Confirmed-ErrorPDU".into()))
        }
        pdu_tags::REJECTED_PDU => {
            Err(Iec61850Error::MmsProtocolError("RejectedPDU".into()))
        }
        _ => Err(Iec61850Error::MmsInvalidResponse(format!(
            "未知响应类型: {:02x}",
            data[0]
        ))),
    }
}

/// 编码 Read 请求
/// 构建 MMS Read Request APDU (IEC 61850-8-1 SSAP)
fn encode_read_request(object: &DataObject) -> Result<Vec<u8>> {
    // APDU 头: confirmed-RequestPDU
    // invokeId present (tag 0x81)
    // service: Read (tag 0x82)
    let mut buf = vec![
        pdu_tags::CONFIRMED_REQUEST_PDU,
        0x81,
        0x01,
        0x01, // invokeId = 1
        0x82,
    ];
    let read_content = encode_read_service(&object.to_string());
    buf.push(0x24); // sequence of
    encode_length(&mut buf, read_content.len());
    buf.extend_from_slice(&read_content);

    Ok(buf)
}

/// 编码 Read Service 参数
fn encode_read_service(object_name: &str) -> Vec<u8> {
    // specification-with-result (optional, skip for simple read)
    // variable-access-specification / variable-specification / object-name
    let mut buf = vec![
        0xA0, // list-of-variable-access-specification
        0xA1, // variable-specification
        0x07, // length
        0x80, // object-name tag
    ];
    let name_bytes = object_name.as_bytes();
    encode_length(&mut buf, name_bytes.len());
    buf.extend_from_slice(name_bytes);

    buf
}

/// 编码 Write 请求
fn encode_write_request(object: &DataObject, payload: &[u8]) -> Result<Vec<u8>> {
    // APDU 头: confirmed-RequestPDU
    // invokeId present (tag 0x81)
    // service: Write (tag 0x83)
    let mut buf = vec![
        pdu_tags::CONFIRMED_REQUEST_PDU,
        0x81,
        0x01,
        0x01, // invokeId = 1
        0x83,
    ];
    let write_content = encode_write_service(&object.to_string(), payload);
    buf.push(0x24); // sequence of
    encode_length(&mut buf, write_content.len());
    buf.extend_from_slice(&write_content);

    Ok(buf)
}

/// 编码 Write Service 参数
fn encode_write_service(object_name: &str, payload: &[u8]) -> Vec<u8> {
    // variable-access-specification / variable-specification / object-name
    let mut buf = vec![
        0xA0, // list-of-variable-access-specification
        0xA1, // variable-specification
        0x07, // length
        0x80, // object-name tag
    ];
    let name_bytes = object_name.as_bytes();
    encode_length(&mut buf, name_bytes.len());
    buf.extend_from_slice(name_bytes);

    // data (简单使用octet-string)
    buf.push(0x84); // data tag
    encode_length(&mut buf, payload.len());
    buf.extend_from_slice(payload);

    buf
}

/// 编码 DefineVariableAccess 请求
fn encode_define_request(_object: &DataObject) -> Result<Vec<u8>> {
    Err(Iec61850Error::Asn1EncodeFailed("DefineVariableAccess 未实现".into()))
}

/// 编码 GetDataAccessAttributes 请求
fn encode_getda_request(_object: &DataObject) -> Result<Vec<u8>> {
    Err(Iec61850Error::Asn1EncodeFailed("GetDataAccessAttributes 未实现".into()))
}

/// 编码 ASN.1 长度（TLV 格式）
fn encode_length(buf: &mut Vec<u8>, len: usize) {
    if len < 128 {
        buf.push(len as u8);
    } else if len < 256 {
        buf.push(0x81);
        buf.push(len as u8);
    } else {
        buf.push(0x82);
        buf.push((len >> 8) as u8);
        buf.push((len & 0xFF) as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_read_request() {
        let request = MmsRequest::read("LLN0", "ST$Pos");
        let encoded = encode_mms_request(&request);
        assert!(encoded.is_ok());
        let data = encoded.unwrap();
        assert!(!data.is_empty());
        assert_eq!(data[0], pdu_tags::CONFIRMED_REQUEST_PDU);
    }

    #[test]
    fn test_encode_write_request() {
        let request = MmsRequest::write("LLN0", "ST$Pos", vec![0x01, 0x02]);
        let encoded = encode_mms_request(&request);
        assert!(encoded.is_ok());
        let data = encoded.unwrap();
        assert!(!data.is_empty());
    }

    #[test]
    fn test_decode_success_response() {
        let data = vec![0x02, 0x00]; // confirmed-ResponsePDU
        let result = decode_mms_response(&data);
        assert!(result.is_ok());
        assert!(result.unwrap().success);
    }

    #[test]
    fn test_decode_error_response() {
        let data = vec![0x03, 0x00]; // confirmed-ErrorPDU
        let result = decode_mms_response(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_empty_response() {
        let result = decode_mms_response(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_length() {
        let mut buf = Vec::new();
        encode_length(&mut buf, 127);
        assert_eq!(buf, vec![127]);

        let mut buf = Vec::new();
        encode_length(&mut buf, 200);
        assert_eq!(buf, vec![0x81, 200]);
    }
}