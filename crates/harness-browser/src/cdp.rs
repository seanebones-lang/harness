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
