use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct RpcRequest {
    pub id: Value,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct RpcResponse {
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcResponse {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_success_response() {
        let resp = RpcResponse::ok(json!(1), json!({"ready": true}));
        assert_eq!(
            serde_json::to_value(resp).unwrap(),
            json!({"id":1, "result":{"ready":true}})
        );
    }

    #[test]
    fn serializes_error_response() {
        let resp = RpcResponse::err(json!(1), -32601, "method not found");
        assert_eq!(
            serde_json::to_value(resp).unwrap(),
            json!({"id":1, "error":{"code":-32601, "message":"method not found"}})
        );
    }
}
