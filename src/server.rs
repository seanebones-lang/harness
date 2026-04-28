//! HTTP server mode: `harness serve`.
//!
//! Endpoints:
//!   GET  /api/health          → {"status":"ok","model":"..."}
//!   GET  /api/sessions        → [{id, name, updated_at}]
//!   POST /api/chat            → SSE stream of AgentEvents (JSON)
//!   GET  /api/sessions/:id    → full session JSON
//!
//! Body for POST /api/chat:
//!   { "prompt": "...", "session_id": "..." (optional) }
//!
//! SSE event format:
//!   data: {"type":"text_chunk","content":"..."}
//!   data: {"type":"tool_start","name":"..."}
//!   data: {"type":"tool_result","name":"...","result":"..."}
//!   data: {"type":"memory_recall","count":3}
//!   data: {"type":"done"}
//!   data: {"type":"error","message":"..."}

use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{sse::{Event, KeepAlive, Sse}, Html, Json},
    routing::{get, post},
    Router,
};
use futures::stream::{self, StreamExt};
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::Message;
use harness_provider_xai::XaiProvider;
use harness_tools::ToolExecutor;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::info;

use crate::agent;
use crate::events::AgentEvent;

// ── Shared server state ───────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ServerState {
    pub provider: XaiProvider,
    pub session_store: Arc<SessionStore>,
    pub memory_store: Option<Arc<MemoryStore>>,
    pub embed_model: Option<String>,
    pub tools: ToolExecutor,
    pub model: String,
    pub system_prompt: String,
}

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct ChatRequest {
    prompt: String,
    session_id: Option<String>,
}

#[derive(Serialize)]
struct SessionSummary {
    id: String,
    name: Option<String>,
    updated_at: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    model: String,
}

const UI_HTML: &str = include_str!("../static/index.html");

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router(state: ServerState) -> Router {
    Router::new()
        .route("/", get(ui))
        .route("/api/health", get(health))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/:id", get(get_session))
        .route("/api/chat", post(chat))
        .with_state(Arc::new(state))
}

pub async fn serve(state: ServerState, addr: std::net::SocketAddr) -> Result<()> {
    let app = router(state);
    info!(%addr, "harness server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn ui() -> Html<&'static str> {
    Html(UI_HTML)
}

async fn health(State(state): State<Arc<ServerState>>) -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok", model: state.model.clone() })
}

async fn list_sessions(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Vec<SessionSummary>>, StatusCode> {
    let sessions = state.session_store.list(50).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        sessions
            .into_iter()
            .map(|(id, name, updated_at)| SessionSummary { id, name, updated_at })
            .collect(),
    ))
}

async fn get_session(
    State(state): State<Arc<ServerState>>,
    Path(id): Path<String>,
) -> Result<Json<Session>, StatusCode> {
    match state.session_store.find(&id) {
        Ok(Some(s)) => Ok(Json(s)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

type SseResult = Result<Event, std::convert::Infallible>;
type BoxSseStream = std::pin::Pin<Box<dyn futures::Stream<Item = SseResult> + Send>>;

fn error_sse(msg: impl std::fmt::Display) -> Sse<BoxSseStream> {
    let data = format!(r#"{{"type":"error","message":"{}"}}"#, msg);
    let s: BoxSseStream = Box::pin(stream::once(async move {
        Ok::<Event, std::convert::Infallible>(Event::default().data(data))
    }));
    Sse::new(s).keep_alive(KeepAlive::default())
}

async fn chat(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<ChatRequest>,
) -> Sse<BoxSseStream> {
    let (tx, rx) = mpsc::unbounded_channel::<AgentEvent>();

    // Resolve or create session
    let mut session = match req.session_id.as_deref() {
        Some(id) => match state.session_store.find(id) {
            Ok(Some(s)) => s,
            Ok(None) => return error_sse(format!("session not found: {id}")),
            Err(e) => return error_sse(e),
        },
        None => Session::new(&state.model),
    };

    session.push(Message::user(&req.prompt));

    let provider = state.provider.clone();
    let tools = state.tools.clone();
    let mem = state.memory_store.as_ref().map(|m| (**m).clone());
    let em = state.embed_model.clone();
    let sys = state.system_prompt.clone();
    let store = state.session_store.clone();
    let mem_store = state.memory_store.as_ref().map(|m| (**m).clone());
    let em2 = state.embed_model.clone();

    let session_id_str = session.id.clone();

    tokio::spawn(async move {
        let _ = agent::drive_agent(
            &provider,
            &tools,
            mem.as_ref(),
            em.as_deref(),
            &mut session,
            &sys,
            Some(&tx),
        )
        .await;

        if let Some(title) = agent::suggest_session_name(&provider, &session).await {
            let _ = store.set_name_if_missing(&session.id, &title);
            session.name = Some(title);
        }
        let _ = store.save(&session);

        if let (Some(m), Some(em)) = (mem_store, em2) {
            agent::store_turn_memory(&provider, &m, &em, &session).await;
        }
    });

    // Prepend a session_id event so the client can track the session.
    let session_id_event = stream::once(async move {
        let data = format!(r#"{{"type":"session_id","id":{}}}"#,
            serde_json::to_string(&session_id_str).unwrap_or_default());
        Ok::<Event, std::convert::Infallible>(Event::default().data(data))
    });

    // Convert AgentEvent stream into SSE
    let event_stream = UnboundedReceiverStream::new(rx).map(|event| {
        let data = match event {
            AgentEvent::TextChunk(s) => {
                format!(r#"{{"type":"text_chunk","content":{}}}"#, serde_json::to_string(&s).unwrap_or_default())
            }
            AgentEvent::ToolStart { name, .. } => {
                format!(r#"{{"type":"tool_start","name":{}}}"#, serde_json::to_string(&name).unwrap_or_default())
            }
            AgentEvent::ToolResult { name, result, .. } => {
                format!(
                    r#"{{"type":"tool_result","name":{},"result":{}}}"#,
                    serde_json::to_string(&name).unwrap_or_default(),
                    serde_json::to_string(&result[..result.len().min(500)]).unwrap_or_default()
                )
            }
            AgentEvent::MemoryRecall { count } => {
                format!(r#"{{"type":"memory_recall","count":{count}}}"#)
            }
            AgentEvent::SubAgentSpawned { task } => {
                format!(r#"{{"type":"sub_agent_spawned","task":{}}}"#, serde_json::to_string(&task).unwrap_or_default())
            }
            AgentEvent::SubAgentDone { task, summary } => {
                format!(
                    r#"{{"type":"sub_agent_done","task":{},"summary":{}}}"#,
                    serde_json::to_string(&task).unwrap_or_default(),
                    serde_json::to_string(&summary).unwrap_or_default()
                )
            }
            AgentEvent::TokenUsage { input, output } => {
                format!(r#"{{"type":"token_usage","input":{input},"output":{output}}}"#)
            }
            AgentEvent::Done => r#"{"type":"done"}"#.to_string(),
            AgentEvent::Error(msg) => {
                format!(r#"{{"type":"error","message":{}}}"#, serde_json::to_string(&msg).unwrap_or_default())
            }
        };
        Ok::<Event, std::convert::Infallible>(Event::default().data(data))
    });

    let combined = session_id_event.chain(event_stream);
    let boxed: BoxSseStream = Box::pin(combined);
    Sse::new(boxed).keep_alive(KeepAlive::default())
}
