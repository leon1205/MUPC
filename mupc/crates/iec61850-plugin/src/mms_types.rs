//! MMS 数据类型定义
//!
//! 定义 IEC 61850-7-2 MMS 协议数据类型

/// MMS 数据对象
#[derive(Debug, Clone, PartialEq)]
pub struct DataObject {
    /// 逻辑节点名（如 "LLN0", "MMXU1"）
    pub ln: String,
    /// 数据对象名（如 "ST$Pos", "MX$Measriz")
    pub do_name: String,
}

impl DataObject {
    /// 从字符串创建（如 "LLN0$ST$Pos"）
    pub fn from_str(s: &str) -> Self {
        let parts: Vec<&str> = s.splitn(2, '$').collect();
        if parts.len() == 2 {
            Self {
                ln: parts[0].to_string(),
                do_name: parts[1].to_string(),
            }
        } else {
            Self {
                ln: String::new(),
                do_name: s.to_string(),
            }
        }
    }

    /// 转换为字符串表示（如 "LLN0$ST$Pos"）
    pub fn to_string(&self) -> String {
        format!("{}${}", self.ln, self.do_name)
    }
}

/// MMS 请求
#[derive(Debug, Clone)]
pub struct MmsRequest {
    pub service: MmsService,
    pub object: DataObject,
    pub payload: Vec<u8>,
}

impl MmsRequest {
    /// 创建 Read 请求
    pub fn read(ln: &str, do_name: &str) -> Self {
        Self {
            service: MmsService::Read,
            object: DataObject {
                ln: ln.to_string(),
                do_name: do_name.to_string(),
            },
            payload: Vec::new(),
        }
    }

    /// 创建 Write 请求
    pub fn write(ln: &str, do_name: &str, value: Vec<u8>) -> Self {
        Self {
            service: MmsService::Write,
            object: DataObject {
                ln: ln.to_string(),
                do_name: do_name.to_string(),
            },
            payload: value,
        }
    }
}

/// MMS 服务类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MmsService {
    Read,
    Write,
    DefineVariableAccess,
    GetDataAccessAttributes,
}

impl MmsService {
    /// 转换为 MMS 协议操作类型
    pub fn to_op_type(&self) -> u32 {
        match self {
            MmsService::Read => 0,
            MmsService::Write => 1,
            MmsService::DefineVariableAccess => 2,
            MmsService::GetDataAccessAttributes => 3,
        }
    }
}

/// MMS 响应
#[derive(Debug, Clone)]
pub struct MmsResponse {
    pub success: bool,
    pub data: Vec<u8>,
    pub error: Option<String>,
}

impl MmsResponse {
    /// 创建成功响应
    pub fn success(data: Vec<u8>) -> Self {
        Self {
            success: true,
            data,
            error: None,
        }
    }

    /// 创建错误响应
    pub fn error(msg: String) -> Self {
        Self {
            success: false,
            data: Vec::new(),
            error: Some(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_object_from_str() {
        let dobj = DataObject::from_str("LLN0$ST$Pos");
        assert_eq!(dobj.ln, "LLN0");
        assert_eq!(dobj.do_name, "ST$Pos");
    }

    #[test]
    fn test_data_object_to_string() {
        let dobj = DataObject {
            ln: "LLN0".to_string(),
            do_name: "ST$Pos".to_string(),
        };
        assert_eq!(dobj.to_string(), "LLN0$ST$Pos");
    }

    #[test]
    fn test_mms_request_read() {
        let req = MmsRequest::read("MMXU1", "MX$Measriz");
        assert_eq!(req.service, MmsService::Read);
        assert_eq!(req.object.ln, "MMXU1");
        assert_eq!(req.object.do_name, "MX$Measriz");
    }

    #[test]
    fn test_mms_request_write() {
        let req = MmsRequest::write("LLN0", "ST$Pos", vec![0x01, 0x02]);
        assert_eq!(req.service, MmsService::Write);
        assert_eq!(req.payload, vec![0x01, 0x02]);
    }

    #[test]
    fn test_mms_response_success() {
        let resp = MmsResponse::success(vec![0x01, 0x02]);
        assert!(resp.success);
        assert_eq!(resp.data, vec![0x01, 0x02]);
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_mms_response_error() {
        let resp = MmsResponse::error("对象不存在".to_string());
        assert!(!resp.success);
        assert!(resp.data.is_empty());
        assert_eq!(resp.error, Some("对象不存在".to_string()));
    }
}
