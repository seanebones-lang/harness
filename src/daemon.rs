//! Harness daemon: long-lived process for VS Code / desktop integrations.
//!
//! On macOS/Linux the daemon listens on a Unix socket (`~/.harness/daemon.sock`).
//! On Windows it binds loopback TCP and writes the port to `~/.harness/daemon.port`.
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
use std::sync::OnceLock;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

/// TCP port file written when the daemon binds loopback TCP.
pub fn tcp_port_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".harness")
        .join("daemon.port")
}

/// `[daemon]` config block.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct DaemonConfig {
    /// `auto` (platform default), `unix`, or `tcp`.
    pub transport: Option<String>,
}

impl DaemonConfig {
    /// Resolved transport mode.
    pub fn effective_transport(&self) -> DaemonTransport {
        DaemonTransport::parse(self.transport.as_deref().unwrap_or("auto"))
    }
}

/// Daemon IPC transport selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonTransport {
    Auto,
    Unix,
    Tcp,
}

impl DaemonTransport {
    fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "unix" => Self::Unix,
            "tcp" => Self::Tcp,
            _ => Self::Auto,
        }
    }
}

static DAEMON_TRANSPORT: OnceLock<DaemonTransport> = OnceLock::new();

/// Apply `[daemon]` settings from config (first call wins).
pub fn configure(cfg: &DaemonConfig) {
    let _ = DAEMON_TRANSPORT.set(cfg.effective_transport());
}

#[cfg(unix)]
fn effective_transport() -> DaemonTransport {
    DAEMON_TRANSPORT
        .get()
        .copied()
        .unwrap_or(DaemonTransport::Auto)
}

/// Default Unix socket path (macOS/Linux).
pub fn socket_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".harness")
        .join("daemon.sock")
}

/// Connect to the running daemon transport for this platform.
pub async fn connect_daemon() -> Result<DaemonConn> {
    let _token = crate::auth_token::read_token_file("daemon.token")?;
    #[cfg(unix)]
    let mode = effective_transport();

    #[cfg(unix)]
    if mode != DaemonTransport::Tcp {
        if let Ok(stream) = UnixStream::connect(socket_path()).await {
            return Ok(DaemonConn::Unix(stream));
        }
        if mode == DaemonTransport::Unix {
            return Err(anyhow::anyhow!("daemon unix socket not available"));
        }
    }

    let port_text = std::fs::read_to_string(tcp_port_path())
        .map_err(|e| anyhow::anyhow!("daemon port file missing: {e}"))?;
    let port: u16 = port_text.trim().parse()?;
    let stream = TcpStream::connect(format!("127.0.0.1:{port}")).await?;
    Ok(DaemonConn::Tcp(stream))
}

/// Active daemon connection (Unix socket or loopback TCP).
pub enum DaemonConn {
    #[cfg(unix)]
    Unix(UnixStream),
    Tcp(TcpStream),
}

impl DaemonConn {
    async fn read_frame(&mut self) -> Result<serde_json::Value> {
        match self {
            #[cfg(unix)]
            DaemonConn::Unix(s) => read_frame(s).await,
            DaemonConn::Tcp(s) => read_frame(s).await,
        }
    }

    async fn write_frame(&mut self, msg: &impl Serialize) -> Result<()> {
        match self {
            #[cfg(unix)]
            DaemonConn::Unix(s) => write_frame(s, msg).await,
            DaemonConn::Tcp(s) => write_frame(s, msg).await,
        }
    }
}

/// Check if a daemon is currently listening.
pub async fn is_running() -> bool {
    connect_daemon().await.is_ok()
}

// ── Wire protocol ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonRequest {
    pub id: u64,
    pub method: String,
    /// Bearer token matching `~/.harness/daemon.token`.
    #[serde(default)]
    pub token: String,
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
pub async fn write_frame<S: AsyncWrite + Unpin>(
    stream: &mut S,
    msg: &impl Serialize,
) -> Result<()> {
    let json = serde_json::to_vec(msg)?;
    let len = json.len() as u32;
    stream.write_all(&len.to_le_bytes()).await?;
    stream.write_all(&json).await?;
    Ok(())
}

/// Read a length-prefixed JSON frame from a stream.
pub async fn read_frame<S: AsyncRead + Unpin>(stream: &mut S) -> Result<serde_json::Value> {
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
    let mut req = req.clone();
    if req.token.is_empty() {
        req.token = crate::auth_token::read_token_file("daemon.token")?;
    }
    let mut stream = connect_daemon().await?;
    stream.write_frame(&req).await?;
    let val = stream.read_frame().await?;
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
    let mut stream = connect_daemon().await?;

    let req = DaemonRequest {
        id: 1,
        method: "chat".into(),
        token: crate::auth_token::read_token_file("daemon.token").unwrap_or_default(),
        params: serde_json::json!({
            "session_id": session_id,
            "prompt": prompt,
            "model": model,
        }),
    };
    stream.write_frame(&req).await?;

    loop {
        let val = stream.read_frame().await?;
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
    std::fs::create_dir_all(
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".harness"),
    )?;

    let session_store: ArcSessionStore = std::sync::Arc::new(session_store);
    let memory_store: ArcMemoryStore = memory_store.map(std::sync::Arc::new);
    let tools: ArcTools = std::sync::Arc::new(tools);

    let auth_token = crate::auth_token::daemon_token()?;
    tracing::info!("daemon auth token loaded from ~/.harness/daemon.token");

    #[cfg(unix)]
    let transport = effective_transport();

    #[cfg(unix)]
    if transport != DaemonTransport::Tcp {
        let sock_path = socket_path();
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path)?;
        tracing::info!(socket = %sock_path.display(), "daemon listening (unix)");

        loop {
            tokio::select! {
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _)) => {
                            spawn_handler(
                                stream,
                                provider.clone(),
                                session_store.clone(),
                                memory_store.clone(),
                                embed_model.clone(),
                                tools.clone(),
                                model.clone(),
                                system_prompt.clone(),
                                auth_token.clone(),
                            );
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
        return Ok(());
    }

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    std::fs::write(tcp_port_path(), port.to_string())?;
    tracing::info!(port, "daemon listening (tcp loopback)");

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _)) => {
                        spawn_handler(
                            stream,
                            provider.clone(),
                            session_store.clone(),
                            memory_store.clone(),
                            embed_model.clone(),
                            tools.clone(),
                            model.clone(),
                            system_prompt.clone(),
                            auth_token.clone(),
                        );
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

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn spawn_handler<S>(
    stream: S,
    provider: harness_provider_core::ArcProvider,
    session_store: ArcSessionStore,
    memory_store: ArcMemoryStore,
    embed_model: Option<String>,
    tools: ArcTools,
    model: String,
    system_prompt: String,
    auth_token: String,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(e) = handle_connection(
            stream,
            provider,
            session_store,
            memory_store,
            embed_model,
            tools,
            model,
            system_prompt,
            auth_token,
        )
        .await
        {
            tracing::warn!("daemon connection error: {e}");
        }
    });
}

#[allow(clippy::too_many_arguments)]
async fn handle_connection<S>(
    mut stream: S,
    provider: harness_provider_core::ArcProvider,
    session_store: ArcSessionStore,
    memory_store: ArcMemoryStore,
    embed_model: Option<String>,
    tools: ArcTools,
    model: String,
    system_prompt: String,
    auth_token: String,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let val = read_frame(&mut stream).await?;
    let req: DaemonRequest = serde_json::from_value(val)?;
    let id = req.id;

    if !crate::auth_token::verify(Some(&req.token), &auth_token) {
        let resp = DaemonResponse {
            id,
            result: None,
            error: Some("unauthorized — set token from ~/.harness/daemon.token".into()),
            stream: None,
            text: None,
        };
        write_frame(&mut stream, &resp).await?;
        return Ok(());
    }

    match req.method.as_str() {
        "chat" => {
            let session_id = req.params["session_id"].as_str().map(|s| s.to_string());
            let prompt = req.params["prompt"].as_str().unwrap_or("").to_string();
            let req_model = req.params["model"].as_str().unwrap_or(&model).to_string();

            let mut session = if let Some(sid) = &session_id {
                session_store
                    .find(sid)?
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
                    &p2,
                    &t2,
                    ms2.as_ref(),
                    em2.as_deref(),
                    &mut session,
                    &sys2,
                    Some(&tx),
                )
                .await?;
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

            // Save session with auto-generated title when missing.
            if let Ok(mut final_session) = handle.await? {
                if let Some(title) =
                    crate::agent::suggest_session_name(&provider, &final_session).await
                {
                    let _ = session_store.set_name_if_missing(&final_session.id, &title);
                    final_session.name = Some(title);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn frame_round_trip_over_duplex() {
        let (mut client, mut server) = duplex(8192);
        let req = DaemonRequest {
            id: 1,
            method: "status".into(),
            token: String::new(),
            params: serde_json::json!({}),
        };
        write_frame(&mut client, &req).await.unwrap();
        let val = read_frame(&mut server).await.unwrap();
        let decoded: DaemonRequest = serde_json::from_value(val).unwrap();
        assert_eq!(decoded.method, "status");
    }

    #[test]
    fn daemon_transport_parse_aliases() {
        assert_eq!(DaemonTransport::parse("unix"), DaemonTransport::Unix);
        assert_eq!(DaemonTransport::parse("UNIX"), DaemonTransport::Unix);
        assert_eq!(DaemonTransport::parse("  tcp  "), DaemonTransport::Tcp);
        assert_eq!(DaemonTransport::parse("Tcp"), DaemonTransport::Tcp);
        assert_eq!(DaemonTransport::parse("auto"), DaemonTransport::Auto);
        assert_eq!(DaemonTransport::parse("weird"), DaemonTransport::Auto);
        assert_eq!(DaemonTransport::parse(""), DaemonTransport::Auto);
    }

    #[test]
    fn daemon_config_effective_transport() {
        let auto = DaemonConfig { transport: None };
        assert_eq!(auto.effective_transport(), DaemonTransport::Auto);
        let unix = DaemonConfig {
            transport: Some("unix".into()),
        };
        assert_eq!(unix.effective_transport(), DaemonTransport::Unix);
        let tcp = DaemonConfig {
            transport: Some("TCP".into()),
        };
        assert_eq!(tcp.effective_transport(), DaemonTransport::Tcp);
    }

    #[test]
    fn socket_and_port_paths_end_with_expected_names() {
        let sock = socket_path();
        assert!(sock.ends_with("daemon.sock"));
        assert!(sock.to_string_lossy().contains(".harness"));
        let port = tcp_port_path();
        assert!(port.ends_with("daemon.port"));
        assert!(port.to_string_lossy().contains(".harness"));
    }

    #[tokio::test]
    async fn write_read_frame_json_object() {
        let (mut a, mut b) = duplex(4096);
        let payload = serde_json::json!({"hello": "world", "n": 7});
        write_frame(&mut a, &payload).await.unwrap();
        let got = read_frame(&mut b).await.unwrap();
        assert_eq!(got["hello"], "world");
        assert_eq!(got["n"], 7);
    }
}
