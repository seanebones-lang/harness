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
use futures::StreamExt;
use harness_provider_core::{ArcProvider, ChatRequest, Delta, Message, MessageContent, Role};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot, Mutex};
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

struct IoShared {
    stdin: Mutex<ChildStdin>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
    _child: Child,
}

#[derive(Clone)]
struct ReaderContext {
    server_name: String,
    progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
}

/// A handle to a running MCP server process.
/// Cheap to clone (Arc-backed). Stdin writes are brief; stdout is read by a dedicated task.
#[derive(Clone)]
pub struct McpClient {
    io: Arc<IoShared>,
    pub server_name: String,
    pub capabilities: Arc<Mutex<ServerCapabilities>>,
    sampling_provider: Arc<Mutex<Option<ArcProvider>>>,
}

async fn mcp_reader_loop(
    mut stdout: BufReader<ChildStdout>,
    io: Arc<IoShared>,
    ctx: Arc<ReaderContext>,
) {
    loop {
        let mut line = String::new();
        let n = match stdout.read_line(&mut line).await {
            Ok(n) => n,
            Err(e) => {
                warn!(server = %ctx.server_name, "MCP stdout read error: {e}");
                break;
            }
        };
        if n == 0 {
            let mut pending = io.pending.lock().await;
            for (_, tx) in pending.drain() {
                let _ = tx.send(Err("MCP server closed stdout".into()));
            }
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let msg: RawMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if msg.id.is_none() {
            if let Some(ref method_name) = msg.method {
                dispatch_notification(&ctx, method_name, msg.params.unwrap_or(Value::Null));
            }
            continue;
        }

        let msg_id = match &msg.id {
            Some(Value::Number(n)) => n.as_u64(),
            _ => None,
        };
        let Some(rid) = msg_id else {
            continue;
        };

        let mut pending = io.pending.lock().await;
        if let Some(sender) = pending.remove(&rid) {
            let out = if let Some(err) = msg.error {
                Err(err.message)
            } else {
                Ok(msg.result.unwrap_or(Value::Null))
            };
            let _ = sender.send(out);
        }
    }
}

fn dispatch_notification(ctx: &ReaderContext, method: &str, params: Value) {
    match method {
        "notifications/progress" => {
            if let Some(tx) = &ctx.progress_tx {
                let token = params["progressToken"].as_str().unwrap_or("").to_string();
                let progress = params["progress"].as_f64().unwrap_or(0.0);
                let total = params["total"].as_f64();
                let _ = tx.send(ProgressEvent {
                    server: ctx.server_name.clone(),
                    progress_token: token,
                    progress,
                    total,
                });
            }
        }
        "notifications/tools/list_changed" => {
            debug!(
                server = %ctx.server_name,
                "tools list changed notification received"
            );
        }
        "notifications/resources/list_changed" => {
            debug!(
                server = %ctx.server_name,
                "resources list changed notification received"
            );
        }
        "notifications/message" => {
            let level = params["level"].as_str().unwrap_or("info");
            let data = params["data"].to_string();
            match level {
                "error" => warn!(server = %ctx.server_name, "MCP log: {data}"),
                _ => debug!(server = %ctx.server_name, "MCP log: {data}"),
            }
        }
        other => {
            debug!(
                server = %ctx.server_name,
                notification = other,
                "unhandled MCP notification"
            );
        }
    }
}

impl McpClient {
    /// Spawn an MCP server, run the initialization handshake, and return the client.
    pub async fn spawn(name: &str, cfg: &McpServerConfig) -> Result<Self> {
        Self::spawn_with_opts(name, cfg, None, None).await
    }

    /// Spawn with an optional progress event sender and optional LLM for MCP sampling.
    pub async fn spawn_with_opts(
        name: &str,
        cfg: &McpServerConfig,
        progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
        sampling_provider: Option<ArcProvider>,
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

        let io = Arc::new(IoShared {
            stdin: Mutex::new(stdin),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            _child: child,
        });
        let ctx = Arc::new(ReaderContext {
            server_name: name.to_string(),
            progress_tx,
        });
        let io_reader = io.clone();
        let ctx_reader = ctx.clone();
        tokio::spawn(async move {
            mcp_reader_loop(stdout, io_reader, ctx_reader).await;
        });

        let client = McpClient {
            io,
            server_name: name.to_string(),
            capabilities: Arc::new(Mutex::new(ServerCapabilities::default())),
            sampling_provider: Arc::new(Mutex::new(sampling_provider)),
        };

        client.initialize().await?;
        Ok(client)
    }

    /// Attach the active LLM provider for MCP sampling after spawn (optional).
    pub async fn attach_sampling_provider(&self, provider: ArcProvider) {
        *self.sampling_provider.lock().await = Some(provider);
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
        let mut stdin = self.io.stdin.lock().await;
        stdin.write_all(notif.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    /// Send a JSON-RPC request and return the `result` field.
    /// Progress notifications are dispatched by the background stdout reader.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.io.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.io.pending.lock().await.insert(id, tx);

        let req = serde_json::to_string(&Request {
            jsonrpc: "2.0",
            id,
            method,
            params,
        })?;

        debug!(server = %self.server_name, method, id, "→ MCP request");
        let write_ok = async {
            let mut stdin = self.io.stdin.lock().await;
            stdin.write_all(req.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if write_ok.is_err() {
            self.io.pending.lock().await.remove(&id);
        }
        write_ok?;

        match rx.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(message)) => anyhow::bail!("MCP error from `{}`: {}", self.server_name, message),
            Err(_) => anyhow::bail!(
                "MCP transport closed waiting for response from `{}`",
                self.server_name
            ),
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
        let messages = params["messages"].as_array().cloned().unwrap_or_default();
        let prompt_preview: Vec<String> = messages
            .iter()
            .map(|m| {
                let role = m["role"].as_str().unwrap_or("?");
                let text = m["content"]["text"].as_str().unwrap_or("(non-text)");
                format!("[{role}]: {text}")
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

        let provider = self.sampling_provider.lock().await.clone();
        let Some(provider) = provider else {
            anyhow::bail!(
                "MCP sampling/createMessage requires an attached LLM provider; pass `Some(provider)` \
                 to `McpClient::spawn_with_opts` or call `attach_sampling_provider` before handling sampling"
            );
        };

        let mut core_messages = Vec::with_capacity(messages.len());
        for m in &messages {
            core_messages.push(mcp_sampling_message_to_core(m)?);
        }

        let req = ChatRequest::new(provider.model().to_string()).with_messages(core_messages);
        let mut stream = provider.stream_chat(req).await?;
        let mut text = String::new();
        while let Some(item) = stream.next().await {
            match item {
                Ok(Delta::Text(t)) => text.push_str(&t),
                Ok(Delta::Done { .. }) => break,
                Ok(_) => {}
                Err(e) => anyhow::bail!("sampling LLM call failed: {e}"),
            }
        }

        Ok(json!({
            "role": "assistant",
            "content": { "type": "text", "text": text },
            "stopReason": "endTurn",
            "model": provider.model(),
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

fn mcp_sampling_message_to_core(m: &Value) -> Result<Message> {
    let role_str = m["role"].as_str().unwrap_or("user");
    let role = match role_str {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    };
    let tool_call_id = m["tool_call_id"].as_str().map(|s| s.to_string());
    let text = extract_mcp_text_content(m.get("content"));
    Ok(Message {
        role,
        content: MessageContent::Text(text),
        tool_call_id,
    })
}

fn extract_mcp_text_content(content_val: Option<&Value>) -> String {
    match content_val {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|p| {
                if p["type"].as_str() == Some("text") {
                    p["text"].as_str().unwrap_or("").to_string()
                } else {
                    format!("[{}]", p["type"].as_str().unwrap_or("part"))
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(obj) if obj.is_object() => {
            if let Some(t) = obj["text"].as_str() {
                t.to_string()
            } else if let Some(arr) = obj["content"].as_array() {
                extract_mcp_text_content(Some(&Value::Array(arr.clone())))
            } else {
                obj.to_string()
            }
        }
        Some(v) => v.to_string(),
    }
}

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
