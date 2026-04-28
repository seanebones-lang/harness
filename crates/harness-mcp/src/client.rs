//! MCP stdio transport: spawns a subprocess and communicates via
//! newline-delimited JSON-RPC 2.0 over stdin/stdout.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;
use tracing::debug;

use crate::config::McpServerConfig;

// ── JSON-RPC types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Request<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Deserialize)]
struct Response {
    id: Option<u64>,
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Deserialize, Debug)]
struct RpcError {
    message: String,
}

// ── Tool definitions returned by tools/list ───────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

// ── Client ────────────────────────────────────────────────────────────────────

struct Inner {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    /// Keep child alive; dropped when Inner is dropped.
    _child: Child,
}

/// A handle to a running MCP server process.
/// Cheap to clone (Arc-backed). All calls are serialized via a Mutex.
#[derive(Clone)]
pub struct McpClient {
    inner: Arc<Mutex<Inner>>,
    pub server_name: String,
}

impl McpClient {
    /// Spawn an MCP server, run the initialization handshake, and return the client.
    pub async fn spawn(name: &str, cfg: &McpServerConfig) -> Result<Self> {
        let mut cmd = tokio::process::Command::new(&cfg.command);
        cmd.args(&cfg.args)
            .envs(&cfg.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null()); // silence server logs

        let mut child = cmd
            .spawn()
            .with_context(|| format!("spawning MCP server `{}`", cfg.command))?;

        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().context("no stdout")?);

        let client = McpClient {
            inner: Arc::new(Mutex::new(Inner {
                stdin,
                stdout,
                next_id: 1,
                _child: child,
            })),
            server_name: name.to_string(),
        };

        client.initialize().await?;
        Ok(client)
    }

    /// Perform the MCP handshake.
    async fn initialize(&self) -> Result<()> {
        self.call(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "harness", "version": "0.1.0" }
            }),
        )
        .await?;

        // Send the required `initialized` notification (no response expected).
        let notif = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        }))?;
        let mut inner = self.inner.lock().await;
        inner.stdin.write_all(notif.as_bytes()).await?;
        inner.stdin.write_all(b"\n").await?;
        inner.stdin.flush().await?;

        debug!(server = %self.server_name, "MCP handshake complete");
        Ok(())
    }

    /// Send a JSON-RPC request and return the `result` field.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let mut inner = self.inner.lock().await;

        let id = inner.next_id;
        inner.next_id += 1;

        let req = serde_json::to_string(&Request {
            jsonrpc: "2.0",
            id,
            method,
            params,
        })?;

        debug!(server = %self.server_name, method, id, "→ MCP request");
        inner.stdin.write_all(req.as_bytes()).await?;
        inner.stdin.write_all(b"\n").await?;
        inner.stdin.flush().await?;

        // Read lines until we find one with our id (ignore notifications).
        loop {
            let mut line = String::new();
            let n = inner.stdout.read_line(&mut line).await?;
            if n == 0 {
                anyhow::bail!("MCP server closed stdout unexpectedly");
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let resp: Response = match serde_json::from_str(line) {
                Ok(r) => r,
                Err(_) => continue, // might be a notification — skip it
            };

            if resp.id != Some(id) {
                continue; // notification or different request
            }

            if let Some(err) = resp.error {
                anyhow::bail!("MCP error from `{}`: {}", self.server_name, err.message);
            }

            return Ok(resp.result.unwrap_or(Value::Null));
        }
    }

    /// List all tools exposed by this MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>> {
        let result = self.call("tools/list", json!({})).await?;
        let tools: Vec<McpToolDef> = serde_json::from_value(
            result["tools"].clone(),
        )
        .context("parsing tools/list response")?;
        Ok(tools)
    }

    /// Call an MCP tool and return its text output.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<String> {
        let result = self
            .call("tools/call", json!({ "name": name, "arguments": arguments }))
            .await?;

        // MCP returns content as an array of typed blocks.
        let content = result["content"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let text: Vec<String> = content
            .iter()
            .filter_map(|block| {
                if block["type"] == "text" {
                    block["text"].as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect();

        Ok(text.join("\n"))
    }
}
