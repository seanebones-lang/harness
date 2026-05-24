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
        Self {
            stdin,
            stdout: BufReader::new(stdout),
        }
    }

    pub async fn send_notification(&mut self, notif: &Notification) -> Result<()> {
        let body = serde_json::to_string(notif)?;
        let msg = encode_lsp_frame(&body);
        self.stdin.write_all(msg.as_bytes()).await?;
        Ok(())
    }

    pub async fn send_request(&mut self, req: &Request) -> Result<()> {
        let body = serde_json::to_string(req)?;
        let msg = encode_lsp_frame(&body);
        self.stdin.write_all(msg.as_bytes()).await?;
        Ok(())
    }

    /// Read one LSP message. Skips notifications (no id).
    pub async fn read_response(&mut self) -> Result<Response> {
        loop {
            let msg = read_lsp_message(&mut self.stdout).await?;
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

/// Read one LSP JSON body from a framed stream (Content-Length headers).
pub async fn read_lsp_message<R>(reader: &mut R) -> Result<serde_json::Value>
where
    R: AsyncBufReadExt + AsyncReadExt + Unpin,
{
    loop {
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                anyhow::bail!("LSP: unexpected EOF while reading headers");
            }
            let line = line.trim();
            if line.is_empty() {
                if content_length.is_some() {
                    break;
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("Content-Length: ") {
                content_length = Some(rest.trim().parse()?);
            }
        }

        let content_length =
            content_length.ok_or_else(|| anyhow::anyhow!("LSP: missing Content-Length header"))?;
        if content_length == 0 {
            continue;
        }

        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body).await?;
        return Ok(serde_json::from_slice(&body)?);
    }
}

/// Encode a JSON body as an LSP `Content-Length` frame.
pub fn encode_lsp_frame(body: &str) -> String {
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

/// Decode one LSP frame from a buffer. Returns the JSON body and remaining input when a full frame is available.
///
/// Used by property tests; async `read_response` reads directly from the stream.
#[allow(dead_code)]
pub fn try_decode_lsp_frame(input: &str) -> Result<Option<(String, &str)>> {
    let mut rest = input;
    let mut content_length: Option<usize> = None;
    loop {
        let Some(nl) = rest.find('\n') else {
            return Ok(None);
        };
        let line = rest[..nl].trim_end_matches('\r');
        rest = &rest[nl + 1..];
        if line.is_empty() {
            if content_length.is_some() {
                break;
            }
            continue;
        }
        if let Some(len_str) = line.strip_prefix("Content-Length: ") {
            content_length = Some(len_str.trim().parse()?);
        }
    }
    let content_length =
        content_length.ok_or_else(|| anyhow::anyhow!("LSP: missing Content-Length header"))?;
    if content_length == 0 {
        return Ok(Some((String::new(), rest)));
    }
    if rest.len() < content_length {
        return Ok(None);
    }
    let body = rest[..content_length].to_string();
    rest = &rest[content_length..];
    Ok(Some((body, rest)))
}

#[cfg(test)]
mod framing_tests {
    use super::*;
    use proptest::prelude::*;
    use tokio::io::BufReader;

    fn ascii_body(max: usize) -> impl Strategy<Value = String> {
        prop::collection::vec(0x20u8..=0x7e, 0..max)
            .prop_map(|bytes| String::from_utf8(bytes).expect("ascii"))
    }

    proptest! {
        #[test]
        fn lsp_frame_round_trip(body in ascii_body(200)) {
            let framed = encode_lsp_frame(&body);
            let (decoded, rest) = try_decode_lsp_frame(&framed)
                .expect("decode")
                .expect("complete frame");
            prop_assert_eq!(decoded, body);
            prop_assert!(rest.is_empty());
        }

        #[test]
        fn lsp_frame_chunk_invariance(body in ascii_body(200)) {
            let framed = encode_lsp_frame(&body);
            if framed.is_empty() {
                return Ok(());
            }
            for split in 1..framed.len() {
                let (a, b) = framed.split_at(split);
                let first = try_decode_lsp_frame(a).expect("decode a");
                if first.is_none() {
                    let combined = format!("{a}{b}");
                    let (decoded, rest) = try_decode_lsp_frame(&combined)
                        .expect("decode combined")
                        .expect("complete frame");
                    prop_assert_eq!(decoded, body.clone());
                    prop_assert!(rest.is_empty());
                }
            }
        }
    }

    #[tokio::test]
    async fn read_lsp_message_accepts_chunked_async_stream() {
        let payload = serde_json::json!({"jsonrpc":"2.0","id":42,"result":{"ok":true}});
        let frame = encode_lsp_frame(&payload.to_string());
        let (mut writer, read_half) = tokio::io::duplex(4096);
        tokio::spawn(async move {
            for chunk in frame.as_bytes().chunks(5) {
                writer.write_all(chunk).await.unwrap();
                tokio::task::yield_now().await;
            }
        });
        let mut reader = BufReader::new(read_half);
        let msg = read_lsp_message(&mut reader).await.unwrap();
        assert_eq!(msg["id"], 42);
        assert_eq!(msg["result"]["ok"], true);
    }
}
