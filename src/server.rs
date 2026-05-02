//! HTTP server mode: `harness serve`.
//!
//! Endpoints:
//!   GET  /api/health          → {"status":"ok","model":"..."}
//!   GET  /api/sessions        → [{id, name, updated_at}]
//!   GET  /api/projects        → [{name, path, remote, default_branch, updated}]
//!   POST /api/projects/:id/action → project action result JSON
//!   GET  /api/projects/:id/files  → file paths for context picker
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
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, Json,
    },
    routing::{get, post},
    Router,
};
use futures::stream::{self, StreamExt};
use harness_memory::{MemoryStore, Session, SessionStore};
use harness_provider_core::{ArcProvider, Message};
use harness_tools::ToolExecutor;
use serde::{Deserialize, Serialize};
use std::path::Path as FsPath;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::info;

use crate::agent;
use crate::events::AgentEvent;
use crate::projects;

// ── Shared server state ───────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ServerState {
    pub provider: ArcProvider,
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

#[derive(Debug, Deserialize)]
struct ProjectActionRequest {
    action: String,
    branch: Option<String>,
    remote: Option<String>,
    command: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProjectActionResponse {
    ok: bool,
    message: String,
    output: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FilesQuery {
    q: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct ProjectSummary {
    name: String,
    path: String,
    remote: Option<String>,
    default_branch: Option<String>,
    updated: String,
}

const UI_HTML: &str = include_str!("../static/index.html");
const PROJECT_GIT_TIMEOUT: Duration = Duration::from_secs(60);
const PROJECT_TEST_TIMEOUT: Duration = Duration::from_secs(300);

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router(state: ServerState) -> Router {
    Router::new()
        .route("/", get(ui))
        .route("/api/health", get(health))
        .route("/api/sessions", get(list_sessions))
        .route("/api/projects", get(list_projects))
        .route("/api/projects/:id/action", post(project_action))
        .route("/api/projects/:id/files", get(project_files))
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
    Json(HealthResponse {
        status: "ok",
        model: state.model.clone(),
    })
}

async fn list_sessions(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Vec<SessionSummary>>, StatusCode> {
    let sessions = state
        .session_store
        .list(50)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        sessions
            .into_iter()
            .map(|(id, name, updated_at)| SessionSummary {
                id,
                name,
                updated_at,
            })
            .collect(),
    ))
}

async fn list_projects() -> Json<Vec<ProjectSummary>> {
    let store = projects::ProjectStore::load();
    let projects = store
        .list_sorted()
        .into_iter()
        .map(|p| ProjectSummary {
            name: p.name,
            path: p.path.display().to_string(),
            remote: p.remote,
            default_branch: p.default_branch,
            updated: p.updated,
        })
        .collect();
    Json(projects)
}

async fn project_action(
    Path(id): Path<String>,
    Json(req): Json<ProjectActionRequest>,
) -> Result<Json<ProjectActionResponse>, StatusCode> {
    let store = projects::ProjectStore::load();
    let project = store.find(&id).ok_or(StatusCode::NOT_FOUND)?;
    let remote = req.remote.as_deref().unwrap_or("origin");

    let result = match req.action.as_str() {
        "sync" => {
            run_git_in_project(&project.path, &["fetch", "--all", "--prune"])
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            run_git_in_project(&project.path, &["pull", "--ff-only"])
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            ProjectActionResponse {
                ok: true,
                message: "Project synced".to_string(),
                output: None,
            }
        }
        "push" => {
            let branch = req
                .branch
                .or_else(|| current_git_branch(&project.path))
                .or(project.default_branch.clone())
                .ok_or(StatusCode::BAD_REQUEST)?;
            run_git_in_project(&project.path, &["push", remote, &branch])
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            ProjectActionResponse {
                ok: true,
                message: format!("Pushed to {remote}/{branch}"),
                output: None,
            }
        }
        "status" => {
            let branch = current_git_branch(&project.path).unwrap_or_else(|| "(detached HEAD)".to_string());
            let upstream = git_output(&project.path, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"]).ok();
            let remote_url = git_output(&project.path, &["remote", "get-url", "origin"]).ok();
            let changes = collect_change_counts(&project.path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let (ahead, behind) = if upstream.is_some() {
                git_ahead_behind(&project.path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            } else {
                (0, 0)
            };
            ProjectActionResponse {
                ok: true,
                message: format!(
                    "branch={branch} upstream={} remote={} ahead={} behind={} staged={} unstaged={} untracked={}",
                    upstream.unwrap_or_else(|| "-".to_string()),
                    remote_url.unwrap_or_else(|| "-".to_string()),
                    ahead,
                    behind,
                    changes.staged,
                    changes.unstaged,
                    changes.untracked
                ),
                output: None,
            }
        }
        "test" => {
            let cmd = req
                .command
                .unwrap_or_else(|| default_test_command(&project.path));
            let output = run_shell_in_project(&project.path, &cmd)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            ProjectActionResponse {
                ok: true,
                message: "Test command finished".to_string(),
                output: Some(output),
            }
        }
        _ => {
            return Ok(Json(ProjectActionResponse {
                ok: false,
                message: format!("unknown action: {}", req.action),
                output: None,
            }));
        }
    };

    Ok(Json(result))
}

async fn project_files(
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<FilesQuery>,
) -> Result<Json<Vec<String>>, StatusCode> {
    let store = projects::ProjectStore::load();
    let project = store.find(&id).ok_or(StatusCode::NOT_FOUND)?;
    let q = query.q.unwrap_or_default().to_lowercase();
    let limit = query.limit.unwrap_or(120).clamp(10, 500);

    let mut out = Vec::new();
    collect_files(&project.path, &project.path, &q, limit, &mut out)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(out))
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
        let data = format!(
            r#"{{"type":"session_id","id":{}}}"#,
            serde_json::to_string(&session_id_str).unwrap_or_default()
        );
        Ok::<Event, std::convert::Infallible>(Event::default().data(data))
    });

    // Convert AgentEvent stream into SSE
    let event_stream = UnboundedReceiverStream::new(rx).map(|event| {
        let data = match event {
            AgentEvent::TextChunk(s) => {
                format!(
                    r#"{{"type":"text_chunk","content":{}}}"#,
                    serde_json::to_string(&s).unwrap_or_default()
                )
            }
            AgentEvent::ToolStart { name, .. } => {
                format!(
                    r#"{{"type":"tool_start","name":{}}}"#,
                    serde_json::to_string(&name).unwrap_or_default()
                )
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
                format!(
                    r#"{{"type":"sub_agent_spawned","task":{}}}"#,
                    serde_json::to_string(&task).unwrap_or_default()
                )
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
            AgentEvent::CacheUsage { creation, read } => {
                format!(r#"{{"type":"cache_usage","creation":{creation},"read":{read}}}"#)
            }
            AgentEvent::Done => r#"{"type":"done"}"#.to_string(),
            AgentEvent::Error(msg) => {
                format!(
                    r#"{{"type":"error","message":{}}}"#,
                    serde_json::to_string(&msg).unwrap_or_default()
                )
            }
        };
        Ok::<Event, std::convert::Infallible>(Event::default().data(data))
    });

    let combined = session_id_event.chain(event_stream);
    let boxed: BoxSseStream = Box::pin(combined);
    Sse::new(boxed).keep_alive(KeepAlive::default())
}

#[derive(Debug, Default)]
struct ChangeCounts {
    staged: usize,
    unstaged: usize,
    untracked: usize,
}

async fn run_git_in_project(path: &FsPath, args: &[&str]) -> anyhow::Result<String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.current_dir(path).args(args).kill_on_drop(true);
    let cmd_display = format!("git {}", args.join(" "));
    let output = timeout(PROJECT_GIT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("{cmd_display} timed out after {}s", PROJECT_GIT_TIMEOUT.as_secs()))??;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn run_shell_in_project(path: &FsPath, command: &str) -> anyhow::Result<String> {
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(path)
        .kill_on_drop(true);
    let output = timeout(PROJECT_TEST_TIMEOUT, cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("command timed out after {}s: {command}", PROJECT_TEST_TIMEOUT.as_secs()))??;
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    if !output.status.success() {
        anyhow::bail!("command failed: {command}\n{text}");
    }
    Ok(text)
}

fn current_git_branch(path: &FsPath) -> Option<String> {
    let output = Command::new("git")
        .current_dir(path)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

fn git_output(path: &FsPath, args: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("git")
        .current_dir(path)
        .args(args)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_ahead_behind(path: &FsPath) -> anyhow::Result<(u64, u64)> {
    let out = git_output(path, &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])?;
    let mut parts = out.split_whitespace();
    let ahead = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    let behind = parts.next().unwrap_or("0").parse::<u64>().unwrap_or(0);
    Ok((ahead, behind))
}

fn collect_change_counts(path: &FsPath) -> anyhow::Result<ChangeCounts> {
    let out = git_output(path, &["status", "--porcelain"])?;
    let mut counts = ChangeCounts::default();
    for line in out.lines() {
        if line.starts_with("?? ") {
            counts.untracked += 1;
            continue;
        }
        let bytes = line.as_bytes();
        if bytes.len() < 2 {
            continue;
        }
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        if x != ' ' && x != '?' {
            counts.staged += 1;
        }
        if y != ' ' && y != '?' {
            counts.unstaged += 1;
        }
    }
    Ok(counts)
}

fn default_test_command(path: &FsPath) -> String {
    if path.join("Cargo.toml").exists() {
        "cargo test".to_string()
    } else if path.join("package.json").exists() {
        "npm test".to_string()
    } else if path.join("pyproject.toml").exists() || path.join("pytest.ini").exists() {
        "pytest".to_string()
    } else if path.join("go.mod").exists() {
        "go test ./...".to_string()
    } else {
        "echo 'No known test command. Pass command in request.'".to_string()
    }
}

fn collect_files(
    root: &FsPath,
    dir: &FsPath,
    query: &str,
    limit: usize,
    out: &mut Vec<String>,
) -> anyhow::Result<()> {
    if out.len() >= limit {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == "node_modules" || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, query, limit, out)?;
            if out.len() >= limit {
                return Ok(());
            }
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            let rel_s = rel.display().to_string();
            if query.is_empty() || rel_s.to_lowercase().contains(query) {
                out.push(rel_s);
                if out.len() >= limit {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_provider_ollama::{OllamaConfig, OllamaProvider};
    use harness_tools::ToolRegistry;
    use tempfile::{tempdir, TempDir};

    async fn spawn_test_server() -> (String, tokio::task::JoinHandle<()>, Arc<SessionStore>, TempDir) {
        let provider = OllamaProvider::new(OllamaConfig::new("qwen3-coder:30b"))
            .expect("build test provider");
        let session_dir = tempdir().expect("temp session dir");
        let session_db = session_dir.path().join("sessions.db");
        let session_store = Arc::new(SessionStore::open(&session_db).expect("open session db"));

        let state = ServerState {
            provider: Arc::new(provider),
            session_store: session_store.clone(),
            memory_store: None,
            embed_model: None,
            tools: ToolExecutor::new(ToolRegistry::new()),
            model: "test-model".to_string(),
            system_prompt: "You are a test assistant.".to_string(),
        };

        let app = router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        (format!("http://{addr}"), handle, session_store, session_dir)
    }

    #[tokio::test]
    async fn http_smoke_health_and_ui() {
        let (base_url, handle, _store, _tmp_dir) = spawn_test_server().await;
        let client = reqwest::Client::new();

        let ui = client
            .get(format!("{base_url}/"))
            .send()
            .await
            .expect("GET / should succeed");
        assert!(ui.status().is_success(), "unexpected / status: {}", ui.status());
        let html = ui.text().await.expect("read html");
        assert!(
            html.contains("id=\"prompt\""),
            "ui should contain chat prompt textarea"
        );

        let health = client
            .get(format!("{base_url}/api/health"))
            .send()
            .await
            .expect("GET /api/health should succeed");
        assert_eq!(health.status(), StatusCode::OK);
        let payload: serde_json::Value = health.json().await.expect("health json");
        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["model"], "test-model");

        handle.abort();
        let _ = handle.await;
    }

    #[tokio::test]
    async fn http_smoke_sessions_endpoints() {
        let (base_url, handle, store, _tmp_dir) = spawn_test_server().await;
        let mut session = Session::new("test-model");
        session.push(Message::user("hello from smoke test"));
        store.save(&session).expect("save session");

        let client = reqwest::Client::new();
        let sessions = client
            .get(format!("{base_url}/api/sessions"))
            .send()
            .await
            .expect("GET /api/sessions should succeed");
        assert_eq!(sessions.status(), StatusCode::OK);
        let list: serde_json::Value = sessions.json().await.expect("sessions json");
        let list = list.as_array().expect("sessions array");
        assert!(
            list.iter().any(|entry| entry["id"] == session.id),
            "saved session should be listed"
        );

        let loaded = client
            .get(format!(
                "{base_url}/api/sessions/{}",
                urlencoding::encode(&session.id)
            ))
            .send()
            .await
            .expect("GET /api/sessions/:id should succeed");
        assert_eq!(loaded.status(), StatusCode::OK);
        let loaded_json: serde_json::Value = loaded.json().await.expect("session json");
        assert_eq!(loaded_json["id"], session.id);

        handle.abort();
        let _ = handle.await;
    }
}
