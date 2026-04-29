//! Minimal JSON-RPC 2.0 message framing for LSP.
//!
//! LSP uses `Content-Length: N\r\n\r\n<json>` framing over stdio.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

impl Request {
    pub fn new(id: u64, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Value,
}

impl Notification {
    pub fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Response {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<ResponseError>,
}

#[derive(Debug, Deserialize)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
}

pub struct LspTransport {
    pub stdin: ChildStdin,
    pub stdout: BufReader<ChildStdout>,
}

impl LspTransport {
    pub fn new(stdin: ChildStdin, stdout: ChildStdout) -> Self {
        Self { stdin, stdout: BufReader::new(stdout) }
    }

    pub async fn send_notification(&mut self, notif: &Notification) -> Result<()> {
        let body = serde_json::to_string(notif)?;
        let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        self.stdin.write_all(msg.as_bytes()).await?;
        Ok(())
    }

    pub async fn send_request(&mut self, req: &Request) -> Result<()> {
        let body = serde_json::to_string(req)?;
        let msg = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        self.stdin.write_all(msg.as_bytes()).await?;
        Ok(())
    }

    /// Read one LSP message. Skips notifications (no id).
    pub async fn read_response(&mut self) -> Result<Response> {
        loop {
            let mut content_length: usize = 0;
            loop {
                let mut line = String::new();
                self.stdout.read_line(&mut line).await?;
                let line = line.trim();
                if line.is_empty() {
                    break;
                }
                if let Some(rest) = line.strip_prefix("Content-Length: ") {
                    content_length = rest.trim().parse()?;
                }
            }

            if content_length == 0 {
                anyhow::bail!("LSP: received message with zero content length");
            }

            let mut body = vec![0u8; content_length];
            self.stdout.read_exact(&mut body).await?;
            let msg: serde_json::Value = serde_json::from_slice(&body)?;

            // Skip notifications (they have no id or id is null)
            if msg.get("id").map(|v| !v.is_null()).unwrap_or(false) {
                let resp: Response = serde_json::from_value(msg)?;
                return Ok(resp);
            }
        }
    }

    /// Read responses until we find one matching the given request id.
    pub async fn read_response_for(&mut self, id: u64) -> Result<Response> {
        loop {
            let resp = self.read_response().await?;
            if resp.id == Some(id) {
                return Ok(resp);
            }
        }
    }
}
