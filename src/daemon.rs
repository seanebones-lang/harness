//! Harness daemon: long-lived process listening on a Unix socket.
//!
//! The daemon holds all expensive resources — SQLite, provider clients, LSP servers,
//! ambient memory consolidation — and accepts JSON-RPC requests over a Unix socket.
//!
//! # Protocol
//!
//! Each message is a length-prefixed JSON frame (4-byte LE u32 length + JSON body).
//!
//! Request:
//!   { "id": 42, "method": "chat", "params": { "session_id": "...", "prompt": "...", "model": "..." } }
//!   { "id": 43, "method": "sessions", "params": {} }
//!   { "id": 44, "method": "status",   "params": {} }
//!   { "id": 45, "method": "shutdown", "params": {} }
//!
//! Response:
//!   { "id": 42, "result": { ... } }  — or  { "id": 42, "error": "message" }
//!
//! Chat responses are streamed as multiple frames:
//!   { "id": 42, "stream": "chunk", "text": "hello" }
//!   { "id": 42, "stream": "done" }
//!
//! # Auto-detect
//!
//! `harness` (without subcommand) checks for a running daemon socket before starting
//! in embedded mode. `harness daemon` starts the daemon explicitly.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// Default socket path.
pub fn socket_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".harness")
        .join("daemon.sock")
}

/// Check if a daemon is currently listening on the socket.
pub async fn is_running() -> bool {
    let path = socket_path();
    if !path.exists() {
        return false;
    }
    // Try a quick connect.
    tokio::net::UnixStream::connect(&path).await.is_ok()
}

// ── Wire protocol ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonRequest {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonResponse {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Streaming frame type: "chunk", "done", or absent for single-shot responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Write a length-prefixed JSON frame to a stream.
pub async fn write_frame(stream: &mut UnixStream, msg: &impl Serialize) -> Result<()> {
    let json = serde_json::to_vec(msg)?;
    let len = json.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&json).await?;
    Ok(())
}

/// Read a length-prefixed JSON frame from a stream.
pub async fn read_frame(stream: &mut UnixStream) -> Result<serde_json::Value> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > 64 * 1024 * 1024 {
        anyhow::bail!("frame too large: {len} bytes");
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}

// ── Client-side API ───────────────────────────────────────────────────────────

/// Connect to a running daemon and send a single request, returning the response.
pub async fn send_request(req: &DaemonRequest) -> Result<DaemonResponse> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).await?;
    write_frame(&mut stream, req).await?;
    let val = read_frame(&mut stream).await?;
    Ok(serde_json::from_value(val)?)
}

/// Stream chat events from the daemon, calling `on_chunk` for each text chunk.
#[allow(dead_code)]
pub async fn stream_chat(
    session_id: Option<&str>,
    prompt: &str,
    model: &str,
    mut on_chunk: impl FnMut(&str),
) -> Result<()> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).await?;

    let req = DaemonRequest {
        id: 1,
        method: "chat".into(),
        params: serde_json::json!({
            "session_id": session_id,
            "prompt": prompt,
            "model": model,
        }),
    };
    write_frame(&mut stream, &req).await?;

    loop {
        let val = read_frame(&mut stream).await?;
        let resp: DaemonResponse = serde_json::from_value(val)?;

        if let Some(text) = &resp.text {
            on_chunk(text);
        }

        match resp.stream.as_deref() {
            Some("done") | None => break,
            _ => {}
        }

        if let Some(err) = resp.error {
            anyhow::bail!("daemon error: {err}");
        }
    }

    Ok(())
}

// ── Daemon server ─────────────────────────────────────────────────────────────

type ArcSessionStore = std::sync::Arc<harness_memory::SessionStore>;
type ArcMemoryStore = Option<std::sync::Arc<harness_memory::MemoryStore>>;
type ArcTools = std::sync::Arc<harness_tools::ToolExecutor>;

/// Start the daemon. Binds to the Unix socket and serves requests.
/// This function runs until `shutdown_rx` fires or a "shutdown" method is received.
#[allow(clippy::too_many_arguments)]
pub async fn run_daemon(
    provider: harness_provider_core::ArcProvider,
    session_store: harness_memory::SessionStore,
    memory_store: Option<harness_memory::MemoryStore>,
    embed_model: Option<String>,
    tools: harness_tools::ToolExecutor,
    model: String,
    system_prompt: String,
    mut shutdown_rx: tokio::sync::watch::Receiver<()>,
) -> Result<()> {
    let sock_path = socket_path();
    // Remove stale socket file if it exists.
    let _ = std::fs::remove_file(&sock_path);
    std::fs::create_dir_all(sock_path.parent().unwrap_or(std::path::Path::new(".")))?;

    let listener = UnixListener::bind(&sock_path)?;
    tracing::info!(socket = %sock_path.display(), "daemon listening");

    let session_store: ArcSessionStore = std::sync::Arc::new(session_store);
    let memory_store: ArcMemoryStore = memory_store.map(std::sync::Arc::new);
    let tools: ArcTools = std::sync::Arc::new(tools);

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _)) => {
                        let p = provider.clone();
                        let ss = session_store.clone();
                        let ms = memory_store.clone();
                        let em = embed_model.clone();
                        let t = tools.clone();
                        let m = model.clone();
                        let sys = system_prompt.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, p, ss, ms, em, t, m, sys).await {
                                tracing::warn!("daemon connection error: {e}");
                            }
                        });
                    }
                    Err(e) => tracing::warn!("daemon accept error: {e}"),
                }
            }
            _ = shutdown_rx.changed() => {
                tracing::info!("daemon shutting down");
                break;
            }
        }
    }

    let _ = std::fs::remove_file(&sock_path);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn handle_connection(
    mut stream: UnixStream,
    provider: harness_provider_core::ArcProvider,
    session_store: ArcSessionStore,
    memory_store: ArcMemoryStore,
    embed_model: Option<String>,
    tools: ArcTools,
    model: String,
    system_prompt: String,
) -> Result<()> {
    let val = read_frame(&mut stream).await?;
    let req: DaemonRequest = serde_json::from_value(val)?;
    let id = req.id;

    match req.method.as_str() {
        "chat" => {
            let session_id = req.params["session_id"].as_str().map(|s| s.to_string());
            let prompt = req.params["prompt"].as_str().unwrap_or("").to_string();
            let req_model = req.params["model"].as_str().unwrap_or(&model).to_string();

            let mut session = if let Some(sid) = &session_id {
                session_store.find(sid)?
                    .unwrap_or_else(|| harness_memory::Session::new(&req_model))
            } else {
                harness_memory::Session::new(&req_model)
            };
            session.push(harness_provider_core::Message::user(&prompt));

            let (tx, mut rx) = crate::events::channel();
            let p2 = provider.clone();
            let t2 = (*tools).clone();
            let ms2 = memory_store.as_ref().map(|m| (**m).clone());
            let em2 = embed_model.clone();
            let sys2 = system_prompt.clone();

            let handle = tokio::spawn(async move {
                crate::agent::drive_agent(
                    &p2, &t2,
                    ms2.as_ref(), em2.as_deref(),
                    &mut session, &sys2, Some(&tx),
                ).await?;
                Ok::<harness_memory::Session, anyhow::Error>(session)
            });

            // Stream events back.
            while let Some(event) = rx.recv().await {
                use crate::events::AgentEvent;
                let frame = match &event {
                    AgentEvent::TextChunk(text) => DaemonResponse {
                        id,
                        result: None,
                        error: None,
                        stream: Some("chunk".into()),
                        text: Some(text.clone()),
                    },
                    AgentEvent::Done => DaemonResponse {
                        id,
                        result: None,
                        error: None,
                        stream: Some("done".into()),
                        text: None,
                    },
                    AgentEvent::Error(e) => DaemonResponse {
                        id,
                        result: None,
                        error: Some(e.clone()),
                        stream: Some("done".into()),
                        text: None,
                    },
                    _ => continue,
                };
                write_frame(&mut stream, &frame).await?;
                if matches!(event, AgentEvent::Done | AgentEvent::Error(_)) {
                    break;
                }
            }

            // Save session.
            if let Ok(final_session) = handle.await? {
                let _ = session_store.save(&final_session);
            }
        }

        "sessions" => {
            let sessions = session_store.list(20)?;
            let list: Vec<serde_json::Value> = sessions.into_iter().map(|(id, name, updated)| {
                serde_json::json!({ "id": id, "name": name, "updated": updated.to_string() })
            }).collect();
            let resp = DaemonResponse {
                id,
                result: Some(serde_json::json!({ "sessions": list })),
                error: None,
                stream: None,
                text: None,
            };
            write_frame(&mut stream, &resp).await?;
        }

        "status" => {
            let resp = DaemonResponse {
                id,
                result: Some(serde_json::json!({
                    "status": "running",
                    "model": model,
                    "pid": std::process::id(),
                })),
                error: None,
                stream: None,
                text: None,
            };
            write_frame(&mut stream, &resp).await?;
        }

        unknown => {
            let resp = DaemonResponse {
                id,
                result: None,
                error: Some(format!("unknown method: {unknown}")),
                stream: None,
                text: None,
            };
            write_frame(&mut stream, &resp).await?;
        }
    }

    Ok(())
}
