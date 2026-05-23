//! Minimal Chrome DevTools Protocol types and helpers.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct CdpRequest {
    pub id: u64,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Deserialize)]
pub struct CdpResponse {
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<CdpError>,
    // event fields
    pub method: Option<String>,
    pub params: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CdpError {
    pub code: i64,
    pub message: String,
}

/// A single Chrome browser target (tab) as returned by /json/list.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeTarget {
    pub id: String,
    pub title: String,
    pub r#type: String,
    pub web_socket_debugger_url: Option<String>,
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cdp_request_serializes_stable_shape() {
        let req = CdpRequest {
            id: 1,
            method: "Page.navigate".into(),
            params: json!({ "url": "https://example.com" }),
        };
        let s = serde_json::to_string(&req).unwrap();
        assert_eq!(
            s,
            r#"{"id":1,"method":"Page.navigate","params":{"url":"https://example.com"}}"#
        );
    }

    #[test]
    fn cdp_response_deserializes_success() {
        let raw = r#"{"id":1,"result":{"frameId":"abc"}}"#;
        let resp: CdpResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.id, Some(1));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn cdp_response_deserializes_error() {
        let raw = r#"{"id":2,"error":{"code":-32000,"message":"fail"}}"#;
        let resp: CdpResponse = serde_json::from_str(raw).unwrap();
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32000);
        assert_eq!(err.message, "fail");
    }

    #[test]
    fn cdp_response_deserializes_event() {
        let raw = r#"{"method":"Page.loadEventFired","params":{"timestamp":1.0}}"#;
        let resp: CdpResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.method.as_deref(), Some("Page.loadEventFired"));
        assert!(resp.params.is_some());
    }

    #[test]
    fn chrome_target_deserializes_camel_case() {
        let raw = r#"{"id":"1","title":"t","type":"page","webSocketDebuggerUrl":"ws://x","url":"about:blank"}"#;
        let target: ChromeTarget = serde_json::from_str(raw).unwrap();
        assert_eq!(target.web_socket_debugger_url.as_deref(), Some("ws://x"));
    }
}
