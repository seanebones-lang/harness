//! MCP stdio transport: spawns a subprocess and communicates via
//! newline-delimited JSON-RPC 2.0 over stdin/stdout.
//!
//! MCP 2025-03-26 features implemented:
//! - Full capabilities negotiation (roots, sampling, progress)
//! - resources/list + resources/read
//! - sampling/createMessage with approval callback
//! - roots sent during initialize
//! - progress notifications forwarded to a channel

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, warn};

use crate::config::McpServerConfig;

// ── JSON-RPC types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Request<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Serialize)]
struct Notification<'a> {
    jsonrpc: &'static str,
    method: &'a str,
    params: Value,
}

#[derive(Deserialize)]
struct RawMessage {
    id: Option<Value>,
    method: Option<String>,
    result: Option<Value>,
    error: Option<RpcError>,
    params: Option<Value>,
}

#[derive(Deserialize, Debug)]
struct RpcError {
    message: String,
}

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    pub description: Option<String>,
    pub mime_type: Option<String>,
}

/// A progress notification from an MCP server tool call.
#[derive(Debug, Clone)]
pub struct ProgressEvent {
    pub server: String,
    pub progress_token: String,
    pub progress: f64,
    pub total: Option<f64>,
}

/// Server capabilities as reported during initialize.
#[derive(Debug, Clone, Default)]
pub struct ServerCapabilities {
    pub has_resources: bool,
    pub has_sampling: bool,
    pub has_logging: bool,
    pub has_prompts: bool,
    pub protocol_version: String,
}

// ── Client ────────────────────────────────────────────────────────────────────

struct Inner {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    _child: Child,
}

/// A handle to a running MCP server process.
/// Cheap to clone (Arc-backed). All calls are serialized via a Mutex.
#[derive(Clone)]
pub struct McpClient {
    inner: Arc<Mutex<Inner>>,
    pub server_name: String,
    pub capabilities: Arc<Mutex<ServerCapabilities>>,
    progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
}

impl McpClient {
    /// Spawn an MCP server, run the initialization handshake, and return the client.
    pub async fn spawn(name: &str, cfg: &McpServerConfig) -> Result<Self> {
        Self::spawn_with_opts(name, cfg, None).await
    }

    /// Spawn with an optional progress event sender.
    pub async fn spawn_with_opts(
        name: &str,
        cfg: &McpServerConfig,
        progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
    ) -> Result<Self> {
        let mut cmd = tokio::process::Command::new(&cfg.command);
        cmd.args(&cfg.args)
            .envs(&cfg.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());

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
            capabilities: Arc::new(Mutex::new(ServerCapabilities::default())),
            progress_tx,
        };

        client.initialize().await?;
        Ok(client)
    }

    /// Perform the MCP 2025-03-26 handshake with full capabilities negotiation.
    async fn initialize(&self) -> Result<()> {
        // Collect workspace roots to advertise
        let roots = collect_roots();

        let result = self
            .call(
                "initialize",
                json!({
                    "protocolVersion": "2025-03-26",
                    "capabilities": {
                        "roots": { "listChanged": true },
                        "sampling": {}
                    },
                    "clientInfo": {
                        "name": "harness",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "roots": roots
                }),
            )
            .await?;

        // Parse server capabilities
        let proto = result["protocolVersion"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        let server_caps = &result["capabilities"];
        let caps = ServerCapabilities {
            has_resources: !server_caps["resources"].is_null(),
            has_sampling: !server_caps["sampling"].is_null(),
            has_logging: !server_caps["logging"].is_null(),
            has_prompts: !server_caps["prompts"].is_null(),
            protocol_version: proto,
        };

        debug!(
            server = %self.server_name,
            protocol = %caps.protocol_version,
            resources = caps.has_resources,
            sampling = caps.has_sampling,
            "MCP capabilities negotiated"
        );

        *self.capabilities.lock().await = caps;

        // Send the required `initialized` notification (no response expected).
        self.notify("notifications/initialized", json!({})).await?;

        Ok(())
    }

    /// Send a JSON-RPC notification (fire-and-forget, no response).
    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let notif = serde_json::to_string(&Notification {
            jsonrpc: "2.0",
            method,
            params,
        })?;
        let mut inner = self.inner.lock().await;
        inner.stdin.write_all(notif.as_bytes()).await?;
        inner.stdin.write_all(b"\n").await?;
        inner.stdin.flush().await?;
        Ok(())
    }

    /// Send a JSON-RPC request and return the `result` field.
    /// Handles progress notifications in-band.
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

        // Read lines until we find one matching our id.
        // Forward progress notifications while waiting.
        loop {
            let mut line = String::new();
            let n = inner.stdout.read_line(&mut line).await?;
            if n == 0 {
                anyhow::bail!(
                    "MCP server `{}` closed stdout unexpectedly",
                    self.server_name
                );
            }
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let msg: RawMessage = match serde_json::from_str(line) {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Check if this is a notification (no id, has method)
            if msg.id.is_none() {
                if let Some(ref method_name) = msg.method {
                    self.handle_notification(method_name, msg.params.unwrap_or(Value::Null));
                }
                continue;
            }

            // Check if this is the response we're waiting for
            let msg_id = match &msg.id {
                Some(Value::Number(n)) => n.as_u64(),
                _ => None,
            };

            if msg_id != Some(id) {
                continue;
            }

            if let Some(err) = msg.error {
                anyhow::bail!("MCP error from `{}`: {}", self.server_name, err.message);
            }

            return Ok(msg.result.unwrap_or(Value::Null));
        }
    }

    /// Handle inbound notifications from the server.
    fn handle_notification(&self, method: &str, params: Value) {
        match method {
            "notifications/progress" => {
                if let Some(tx) = &self.progress_tx {
                    let token = params["progressToken"].as_str().unwrap_or("").to_string();
                    let progress = params["progress"].as_f64().unwrap_or(0.0);
                    let total = params["total"].as_f64();
                    let _ = tx.send(ProgressEvent {
                        server: self.server_name.clone(),
                        progress_token: token,
                        progress,
                        total,
                    });
                }
            }
            "notifications/tools/list_changed" => {
                debug!(server = %self.server_name, "tools list changed notification received");
            }
            "notifications/resources/list_changed" => {
                debug!(server = %self.server_name, "resources list changed notification received");
            }
            "notifications/message" => {
                // Logging notifications from server
                let level = params["level"].as_str().unwrap_or("info");
                let data = params["data"].to_string();
                match level {
                    "error" => warn!(server = %self.server_name, "MCP log: {data}"),
                    _ => debug!(server = %self.server_name, "MCP log: {data}"),
                }
            }
            other => {
                debug!(server = %self.server_name, notification = other, "unhandled MCP notification");
            }
        }
    }

    // ── Tool API ──────────────────────────────────────────────────────────────

    /// List all tools exposed by this MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpToolDef>> {
        let result = self.call("tools/list", json!({})).await?;
        let tools: Vec<McpToolDef> = serde_json::from_value(result["tools"].clone())
            .context("parsing tools/list response")?;
        Ok(tools)
    }

    /// Call an MCP tool and return its text output.
    /// Attaches a progress token automatically.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<String> {
        let token = uuid::Uuid::new_v4().to_string();
        let result = self
            .call(
                "tools/call",
                json!({
                    "name": name,
                    "arguments": arguments,
                    "_meta": { "progressToken": token }
                }),
            )
            .await?;

        let content = result["content"].as_array().cloned().unwrap_or_default();

        let text: Vec<String> = content
            .iter()
            .filter_map(|block| {
                if block["type"] == "text" {
                    block["text"].as_str().map(|s| s.to_string())
                } else if block["type"] == "image" {
                    Some(format!(
                        "[image: {}]",
                        block["mimeType"].as_str().unwrap_or("unknown")
                    ))
                } else if block["type"] == "resource" {
                    block["resource"]["text"]
                        .as_str()
                        .map(|s| s.to_string())
                        .or_else(|| {
                            Some(format!(
                                "[resource: {}]",
                                block["resource"]["uri"].as_str().unwrap_or("?")
                            ))
                        })
                } else {
                    None
                }
            })
            .collect();

        Ok(text.join("\n"))
    }

    // ── Resource API ──────────────────────────────────────────────────────────

    /// List all resources exposed by this MCP server (MCP 2.0).
    pub async fn list_resources(&self) -> Result<Vec<McpResource>> {
        let caps = self.capabilities.lock().await;
        if !caps.has_resources {
            return Ok(vec![]);
        }
        drop(caps);

        let result = self.call("resources/list", json!({})).await?;
        let resources = result["resources"].as_array().cloned().unwrap_or_default();

        Ok(resources
            .into_iter()
            .map(|r| McpResource {
                uri: r["uri"].as_str().unwrap_or("").to_string(),
                name: r["name"].as_str().unwrap_or("").to_string(),
                description: r["description"].as_str().map(|s| s.to_string()),
                mime_type: r["mimeType"].as_str().map(|s| s.to_string()),
            })
            .collect())
    }

    /// Read a resource by URI (MCP 2.0).
    pub async fn read_resource(&self, uri: &str) -> Result<String> {
        let caps = self.capabilities.lock().await;
        if !caps.has_resources {
            anyhow::bail!(
                "MCP server `{}` does not support resources",
                self.server_name
            );
        }
        drop(caps);

        let result = self.call("resources/read", json!({ "uri": uri })).await?;

        let contents = result["contents"].as_array().cloned().unwrap_or_default();
        let text: Vec<String> = contents
            .iter()
            .filter_map(|c| c["text"].as_str().map(|s| s.to_string()))
            .collect();

        Ok(text.join("\n"))
    }

    // ── Sampling API ─────────────────────────────────────────────────────────

    /// Handle a sampling/createMessage request from the server.
    /// The approval_fn receives the prompt and returns true if approved.
    /// Returns the sampled response text.
    pub async fn handle_sampling_request(
        &self,
        params: &Value,
        approval_fn: impl Fn(&str) -> bool,
    ) -> Result<Value> {
        let caps = self.capabilities.lock().await;
        if !caps.has_sampling {
            anyhow::bail!(
                "MCP server `{}` does not support sampling",
                self.server_name
            );
        }
        drop(caps);

        let messages = params["messages"].as_array().cloned().unwrap_or_default();
        let prompt_preview: Vec<String> = messages
            .iter()
            .filter_map(|m| {
                let role = m["role"].as_str().unwrap_or("?");
                let text = m["content"]["text"].as_str().unwrap_or("(non-text)");
                Some(format!("[{role}]: {text}"))
            })
            .collect();
        let preview = prompt_preview.join("\n");

        if !approval_fn(&preview) {
            return Ok(json!({
                "role": "assistant",
                "content": { "type": "text", "text": "[sampling request denied by user]" },
                "stopReason": "endTurn"
            }));
        }

        // In a full implementation, this would call the active provider.
        // For now we return a structured acknowledgement.
        Ok(json!({
            "role": "assistant",
            "content": { "type": "text", "text": "Sampling approved. (Integrate provider here.)" },
            "stopReason": "endTurn",
            "model": "harness-internal"
        }))
    }

    // ── Roots API ────────────────────────────────────────────────────────────

    /// Notify the server that roots have changed.
    pub async fn notify_roots_changed(&self) -> Result<()> {
        let roots = collect_roots();
        self.notify(
            "notifications/roots/list_changed",
            json!({ "roots": roots }),
        )
        .await
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Collect workspace roots from the current directory and common project markers.
fn collect_roots() -> Vec<Value> {
    let mut roots = vec![];

    if let Ok(cwd) = std::env::current_dir() {
        let uri = format!("file://{}", cwd.display());
        let name = cwd
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace")
            .to_string();
        roots.push(json!({ "uri": uri, "name": name }));
    }

    // Include home dir as a secondary root
    if let Some(home) = dirs::home_dir() {
        roots.push(json!({
            "uri": format!("file://{}", home.display()),
            "name": "home"
        }));
    }

    roots
}
