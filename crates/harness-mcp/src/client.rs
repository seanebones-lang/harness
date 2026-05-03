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
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
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

/// Boxed async writer used as the transport stdin. Trait-object so the same
/// `IoShared` can wrap a real child's stdin or a `tokio::io::duplex` half in tests.
type BoxedWrite = Box<dyn AsyncWrite + Send + Unpin>;

struct IoShared {
    stdin: Mutex<BoxedWrite>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>,
    /// Anything held purely for its `Drop` side-effect: the child process for real
    /// spawns, `()` for in-process tests. Never read.
    _keepalive: Box<dyn std::any::Any + Send + Sync>,
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

async fn mcp_reader_loop<R: AsyncBufRead + Send + Unpin>(
    mut stdout: R,
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
        let stdout = child.stdout.take().context("no stdout")?;

        Self::from_streams(
            stdout,
            stdin,
            child,
            name.to_string(),
            progress_tx,
            sampling_provider,
        )
        .await
    }

    /// Construct a client over arbitrary async streams. Used internally by `spawn_with_opts`
    /// and (under `#[cfg(test)]`) by in-process unit tests that swap a real subprocess for
    /// `tokio::io::duplex` halves.
    ///
    /// `keepalive` is held until the client is dropped; pass `child` for spawned servers,
    /// `()` for tests.
    pub(crate) async fn from_streams<R, W, K>(
        stdout: R,
        stdin: W,
        keepalive: K,
        name: String,
        progress_tx: Option<mpsc::UnboundedSender<ProgressEvent>>,
        sampling_provider: Option<ArcProvider>,
    ) -> Result<Self>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
        K: Send + Sync + 'static,
    {
        let stdout = BufReader::new(stdout);
        let io = Arc::new(IoShared {
            stdin: Mutex::new(Box::new(stdin)),
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            _keepalive: Box::new(keepalive),
        });
        let ctx = Arc::new(ReaderContext {
            server_name: name.clone(),
            progress_tx,
        });
        let io_reader = io.clone();
        let ctx_reader = ctx.clone();
        tokio::spawn(async move {
            mcp_reader_loop(stdout, io_reader, ctx_reader).await;
        });

        let client = McpClient {
            io,
            server_name: name,
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::DuplexStream;
    use tokio::sync::mpsc::unbounded_channel;

    /// Drive the standard `initialize` + `notifications/initialized` handshake, then return.
    /// Caller is responsible for any further dispatch on `(reader, writer)`.
    async fn do_handshake(
        reader: &mut BufReader<DuplexStream>,
        writer: &mut DuplexStream,
        capabilities: Value,
    ) {
        let mut line = String::new();
        // initialize
        let n = reader.read_line(&mut line).await.unwrap();
        assert!(n > 0, "client failed to send initialize");
        let init: Value = serde_json::from_str(line.trim()).expect("initialize is valid JSON");
        assert_eq!(init["method"], "initialize");
        let resp = json!({
            "jsonrpc": "2.0",
            "id": init["id"].clone(),
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": capabilities,
            }
        });
        let s = serde_json::to_string(&resp).unwrap();
        writer.write_all(s.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
        writer.flush().await.unwrap();

        // notifications/initialized (fire-and-forget; no response)
        line.clear();
        let n = reader.read_line(&mut line).await.unwrap();
        assert!(n > 0, "client failed to send notifications/initialized");
        let init_notif: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(init_notif["method"], "notifications/initialized");
    }

    fn make_pipes() -> (DuplexStream, DuplexStream, DuplexStream, DuplexStream) {
        let (client_w, server_r) = tokio::io::duplex(8192);
        let (server_w, client_r) = tokio::io::duplex(8192);
        (client_w, client_r, server_r, server_w)
    }

    #[tokio::test]
    async fn initialize_handshake_completes_and_parses_capabilities() {
        let (client_w, client_r, server_r, mut server_w) = make_pipes();
        let mut reader = BufReader::new(server_r);

        let server = tokio::spawn(async move {
            do_handshake(
                &mut reader,
                &mut server_w,
                json!({
                    "resources": {"listChanged": true},
                    "sampling": {},
                    "logging": {},
                }),
            )
            .await;
        });

        let client = McpClient::from_streams(client_r, client_w, (), "test".into(), None, None)
            .await
            .expect("client construct");
        server.await.expect("server task");

        let caps = client.capabilities.lock().await.clone();
        assert_eq!(caps.protocol_version, "2025-03-26");
        assert!(caps.has_resources, "resources cap should be advertised");
        assert!(caps.has_sampling, "sampling cap should be advertised");
        assert!(caps.has_logging, "logging cap should be advertised");
        assert!(!caps.has_prompts, "prompts cap not advertised in this test");
    }

    /// The most important regression test for the `3fa6d51` MCP refactor: a single
    /// MCP client must support multiple concurrent in-flight RPCs and demux replies
    /// by `id` regardless of arrival order. Pre-3fa6d51 the client held a mutex
    /// across `read_line().await`, which serialised every RPC. This test would have
    /// hung on the second concurrent call under the old code.
    #[tokio::test]
    async fn concurrent_rpcs_demux_correctly_when_replies_arrive_out_of_order() {
        let (client_w, client_r, server_r, mut server_w) = make_pipes();
        let mut reader = BufReader::new(server_r);

        let server = tokio::spawn(async move {
            do_handshake(&mut reader, &mut server_w, json!({})).await;

            // Collect 5 concurrent requests, then reply in REVERSE order so the client
            // must demux by id, not by arrival order.
            let mut pending = Vec::new();
            for _ in 0..5 {
                let mut line = String::new();
                let n = reader.read_line(&mut line).await.unwrap();
                assert!(n > 0);
                let msg: Value = serde_json::from_str(line.trim()).unwrap();
                let id = msg["id"].as_u64().unwrap();
                let arg = msg["params"]["arg"].as_str().unwrap_or("").to_string();
                pending.push((id, arg));
            }
            // Reverse the response order, with small delays between each.
            pending.reverse();
            for (id, arg) in pending {
                tokio::time::sleep(Duration::from_millis(5)).await;
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "echo": arg, "id": id }
                });
                let s = serde_json::to_string(&resp).unwrap();
                server_w.write_all(s.as_bytes()).await.unwrap();
                server_w.write_all(b"\n").await.unwrap();
                server_w.flush().await.unwrap();
            }
        });

        let client =
            McpClient::from_streams(client_r, client_w, (), "concurrent-test".into(), None, None)
                .await
                .expect("client construct");

        // Fire 5 concurrent calls; collect results.
        let mut handles = Vec::new();
        for i in 0..5u32 {
            let c = client.clone();
            handles.push(tokio::spawn(async move {
                let arg = format!("call-{i}");
                let result = c
                    .call("echo", json!({"arg": arg.clone()}))
                    .await
                    .expect("call should succeed");
                (arg, result)
            }));
        }

        let timeout = tokio::time::timeout(
            Duration::from_secs(5),
            futures::future::join_all(handles),
        )
        .await
        .expect("all 5 concurrent calls must complete inside 5s — would hang under serialised RPC");

        for h in timeout {
            let (sent_arg, result) = h.expect("task panicked");
            let echoed = result["echo"].as_str().unwrap();
            assert_eq!(echoed, sent_arg, "result must match the call's argument");
        }

        server.await.expect("server task");
    }

    #[tokio::test]
    async fn json_rpc_error_response_propagates_message() {
        let (client_w, client_r, server_r, mut server_w) = make_pipes();
        let mut reader = BufReader::new(server_r);

        let server = tokio::spawn(async move {
            do_handshake(&mut reader, &mut server_w, json!({})).await;

            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let msg: Value = serde_json::from_str(line.trim()).unwrap();
            let resp = json!({
                "jsonrpc": "2.0",
                "id": msg["id"].clone(),
                "error": { "code": -32601, "message": "method not found: bogus" }
            });
            let s = serde_json::to_string(&resp).unwrap();
            server_w.write_all(s.as_bytes()).await.unwrap();
            server_w.write_all(b"\n").await.unwrap();
            server_w.flush().await.unwrap();
        });

        let client = McpClient::from_streams(client_r, client_w, (), "err-test".into(), None, None)
            .await
            .unwrap();

        let err = client
            .call("bogus", json!({}))
            .await
            .expect_err("server returned JSON-RPC error; client must propagate");
        let msg = err.to_string();
        assert!(
            msg.contains("method not found: bogus"),
            "error must surface server message, got: {msg}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn server_close_fails_pending_calls_cleanly() {
        let (client_w, client_r, server_r, mut server_w) = make_pipes();
        let mut reader = BufReader::new(server_r);

        let server = tokio::spawn(async move {
            do_handshake(&mut reader, &mut server_w, json!({})).await;
            // Read one request, then drop the writer (server closes stdout) without responding.
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            drop(server_w);
            drop(reader);
        });

        let client =
            McpClient::from_streams(client_r, client_w, (), "close-test".into(), None, None)
                .await
                .unwrap();

        let res = tokio::time::timeout(
            Duration::from_secs(5),
            client.call("never_replied", json!({})),
        )
        .await
        .expect("client must surface error inside 5s when server closes");
        let err = res.expect_err("must be Err when server hangs up mid-RPC");
        let msg = err.to_string();
        assert!(
            msg.contains("closed") || msg.contains("transport"),
            "error must mention transport closure, got: {msg}"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn progress_notifications_forwarded_to_channel() {
        let (client_w, client_r, server_r, mut server_w) = make_pipes();
        let mut reader = BufReader::new(server_r);

        let (tx, mut rx) = unbounded_channel::<ProgressEvent>();

        let server = tokio::spawn(async move {
            do_handshake(&mut reader, &mut server_w, json!({})).await;

            // Wait for the tools/call request, then emit two progress notifications and a final result.
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let msg: Value = serde_json::from_str(line.trim()).unwrap();
            let id = msg["id"].clone();
            let token = msg["params"]["_meta"]["progressToken"]
                .as_str()
                .unwrap()
                .to_string();

            // Two progress notifications first.
            for p in [0.25, 0.75] {
                let notif = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/progress",
                    "params": { "progressToken": token, "progress": p, "total": 1.0 }
                });
                let s = serde_json::to_string(&notif).unwrap();
                server_w.write_all(s.as_bytes()).await.unwrap();
                server_w.write_all(b"\n").await.unwrap();
                server_w.flush().await.unwrap();
            }

            // Then the final tool result.
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "content": [{"type": "text", "text": "done"}] }
            });
            let s = serde_json::to_string(&resp).unwrap();
            server_w.write_all(s.as_bytes()).await.unwrap();
            server_w.write_all(b"\n").await.unwrap();
            server_w.flush().await.unwrap();
        });

        let client = McpClient::from_streams(
            client_r,
            client_w,
            (),
            "progress-test".into(),
            Some(tx),
            None,
        )
        .await
        .unwrap();

        let result = client
            .call_tool("any_tool", json!({}))
            .await
            .expect("call_tool should succeed");
        assert_eq!(result, "done");

        // Drain progress events with a short timeout.
        let mut got = Vec::new();
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await {
            got.push(ev);
        }
        assert_eq!(got.len(), 2, "expected 2 progress events, got {got:?}");
        assert_eq!(got[0].server, "progress-test");
        assert_eq!(got[0].progress, 0.25);
        assert_eq!(got[1].progress, 0.75);
        assert_eq!(got[0].total, Some(1.0));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn sampling_request_without_provider_returns_clear_error() {
        let (client_w, client_r, _server_r, mut server_w) = make_pipes();
        let mut reader = BufReader::new(_server_r);

        let server = tokio::spawn(async move {
            do_handshake(&mut reader, &mut server_w, json!({})).await;
        });

        let client = McpClient::from_streams(
            client_r,
            client_w,
            (),
            "sample-test".into(),
            None,
            None, // no sampling provider attached
        )
        .await
        .unwrap();
        server.await.unwrap();

        let params = json!({
            "messages": [
                {"role": "user", "content": {"type": "text", "text": "hello"}}
            ]
        });
        let err = client
            .handle_sampling_request(&params, |_| true)
            .await
            .expect_err("missing provider must return an explanatory error");
        let msg = err.to_string();
        assert!(
            msg.contains("attached LLM provider")
                || msg.contains("attach_sampling_provider")
                || msg.contains("spawn_with_opts"),
            "error must guide caller to attach a provider, got: {msg}"
        );
    }

    #[tokio::test]
    async fn sampling_request_denied_by_approval_callback_returns_text_response() {
        let (client_w, client_r, server_r, mut server_w) = make_pipes();
        let mut reader = BufReader::new(server_r);

        let server = tokio::spawn(async move {
            do_handshake(&mut reader, &mut server_w, json!({})).await;
        });

        let client =
            McpClient::from_streams(client_r, client_w, (), "deny-test".into(), None, None)
                .await
                .unwrap();
        server.await.unwrap();

        let params = json!({
            "messages": [
                {"role": "user", "content": {"type": "text", "text": "secret prompt"}}
            ]
        });
        // Approval rejects → fast-path returns synthesized denial WITHOUT touching provider.
        let resp = client
            .handle_sampling_request(&params, |_| false)
            .await
            .expect("denial path must not require provider");
        assert_eq!(resp["role"], "assistant");
        assert!(resp["content"]["text"].as_str().unwrap().contains("denied"));
        assert_eq!(resp["stopReason"], "endTurn");
    }

    #[tokio::test]
    async fn list_tools_round_trip() {
        let (client_w, client_r, server_r, mut server_w) = make_pipes();
        let mut reader = BufReader::new(server_r);

        let server = tokio::spawn(async move {
            do_handshake(&mut reader, &mut server_w, json!({})).await;

            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            let msg: Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(msg["method"], "tools/list");
            let resp = json!({
                "jsonrpc": "2.0",
                "id": msg["id"].clone(),
                "result": {
                    "tools": [
                        {
                            "name": "echo",
                            "description": "Echo input",
                            "inputSchema": { "type": "object" }
                        },
                        {
                            "name": "add",
                            "inputSchema": { "type": "object" }
                        }
                    ]
                }
            });
            let s = serde_json::to_string(&resp).unwrap();
            server_w.write_all(s.as_bytes()).await.unwrap();
            server_w.write_all(b"\n").await.unwrap();
            server_w.flush().await.unwrap();
        });

        let client =
            McpClient::from_streams(client_r, client_w, (), "tools-test".into(), None, None)
                .await
                .unwrap();

        let tools = client.list_tools().await.expect("list_tools");
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description.as_deref(), Some("Echo input"));
        assert_eq!(tools[1].name, "add");
        assert!(tools[1].description.is_none());

        server.await.unwrap();
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
