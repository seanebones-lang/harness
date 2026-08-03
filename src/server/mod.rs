//! HTTP server mode: `harness serve`.
//!
//! Endpoints documented in module docs and PUBLIC_RELEASE / COOKBOOK.

mod auth;
mod collab_ws;
mod project_ops;
mod state;

pub use state::{ServeRuntimeState, ServerState};

use anyhow::Result;
use axum::extract::ConnectInfo;
use axum::http::StatusCode;
use axum::middleware;
use axum::{
    extract::{Path, State},
    response::{
        sse::{Event, KeepAlive, Sse},
        Html, Json,
    },
    routing::{get, post},
    Router,
};
use futures::stream::{self, StreamExt};
use harness_memory::Session;
use harness_provider_core::Message;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::info;

use crate::agent;
use crate::collab;
use crate::events::{channel as agent_event_channel, try_emit, AgentEvent};
use crate::projects;

use auth::{extract_bearer, require_auth};
use collab_ws::{agent_event_to_collab, collab_ws};
use project_ops::{
    collect_change_counts, collect_files, current_git_branch, default_test_command, git_output,
    git_ahead_behind, is_allowed_test_command, run_git_in_project, run_shell_in_project,
};


// ── Request / response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
struct ChatRequest {
    prompt: String,
    session_id: Option<String>,
    /// Optional bearer token (prefer `Authorization: Bearer` header).
    token: Option<String>,
}

#[derive(Serialize)]
struct SessionSummary {
    id: String,
    name: Option<String>,
    updated_at: String,
}

#[derive(Serialize)]
struct HealthKeyHints {
    /// `ANTHROPIC_API_KEY` non-empty on the **`harness serve` process** (browser cannot read your shell).
    anthropic_env: bool,
    xai_env: bool,
    openai_env: bool,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    /// Session default model label (`--model` or `~/.harness/config.toml`).
    model: String,
    /// Model identifier reported by the constructed provider backend.
    provider_model: String,
    #[serde(rename = "key_env")]
    keys: HealthKeyHints,
    /// Expanded path to the active Harness config file (loopback clients only).
    #[serde(skip_serializing_if = "Option::is_none")]
    config_path: Option<String>,
    /// Loopback-only bootstrap token for the bundled web UI.
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_token: Option<String>,
}

#[derive(Serialize)]
struct SetupStateResponse {
    primary: String,
    model: String,
    has_anthropic_key: bool,
    has_xai_key: bool,
    has_openai_key: bool,
}

#[derive(Serialize)]
struct PersistSetupResponse {
    ok: bool,
    message: String,
}

fn nonempty_env(var: &'static str) -> bool {
    std::env::var(var)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
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


const UI_HTML: &str = include_str!("../../static/index.html");


// ── Router ────────────────────────────────────────────────────────────────────

pub fn router(state: ServerState) -> Router {
    let shared = Arc::new(state);
    let protected = Router::new()
        .route("/api/sessions", get(list_sessions))
        .route("/api/projects", get(list_projects))
        .route("/api/projects/:id/action", post(project_action))
        .route("/api/projects/:id/files", get(project_files))
        .route("/api/sessions/:id", get(get_session))
        .route("/api/chat", post(chat))
        .route("/api/setup/persist", post(persist_setup))
        .layer(middleware::from_fn_with_state(shared.clone(), require_auth));

    Router::new()
        .route("/", get(ui))
        .route("/api/health", get(health))
        .route("/api/setup/state", get(setup_state))
        .route("/ws/session/:id", get(collab_ws))
        .merge(protected)
        .with_state(shared)
}


pub async fn serve(state: ServerState, addr: SocketAddr) -> Result<()> {
    if !addr.ip().is_loopback() {
        tracing::warn!(
            %addr,
            "harness serve bound to non-loopback address — bearer auth is required; \
             agent tools can execute shell commands (see docs/THREAT_MODEL.md)"
        );
    }
    let app = router(state);
    info!(%addr, "harness server listening");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn config_key_nonempty(cfg: &crate::config::Config, name: &str) -> bool {
    cfg.providers
        .get(name)
        .and_then(|e| e.api_key.as_ref())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

async fn setup_state(State(state): State<Arc<ServerState>>) -> Json<SetupStateResponse> {
    let g = state.inner.read().await;
    let cfg = &g.config;
    let primary = cfg
        .router
        .default
        .clone()
        .unwrap_or_else(|| "anthropic".to_string());
    Json(SetupStateResponse {
        primary,
        model: g.model.clone(),
        has_anthropic_key: config_key_nonempty(cfg, "anthropic")
            || nonempty_env("ANTHROPIC_API_KEY"),
        has_xai_key: config_key_nonempty(cfg, "xai")
            || !cfg
                .provider
                .api_key
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            || nonempty_env("XAI_API_KEY"),
        has_openai_key: config_key_nonempty(cfg, "openai") || nonempty_env("OPENAI_API_KEY"),
    })
}

async fn persist_setup(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<ServerState>>,
    Json(patch): Json<crate::config::WebSetupPersist>,
) -> Result<Json<PersistSetupResponse>, (StatusCode, Json<PersistSetupResponse>)> {
    if !addr.ip().is_loopback() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(PersistSetupResponse {
                ok: false,
                message:
                    "POST /api/setup/persist is restricted to localhost for security (reload from 127.0.0.1)."
                        .to_string(),
            }),
        ));
    }

    let path = crate::config::active_config_toml_path();
    let mut cfg = if path.exists() {
        crate::config::load(Some(path.as_path()))
            .map_err(|e| bad_persist(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    } else {
        crate::config::load(None)
            .map_err(|e| bad_persist(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };

    crate::config::apply_web_setup_patch(&mut cfg, &patch)
        .map_err(|e| bad_persist(StatusCode::BAD_REQUEST, e.to_string()))?;

    let candidate_provider =
        crate::provider_build::build_arc_provider(&cfg, None).map_err(|e| {
            bad_persist(
                StatusCode::BAD_REQUEST,
                format!("invalid provider configuration: {e}"),
            )
        })?;

    let Some(model_val) = cfg.provider.model.clone() else {
        return Err(bad_persist(
            StatusCode::BAD_REQUEST,
            "model missing after persist".into(),
        ));
    };
    let sys = cfg
        .agent
        .system_prompt
        .clone()
        .unwrap_or_else(|| agent::DEFAULT_SYSTEM.to_string());

    let mem = state.memory_store.as_ref().map(|m| (**m).clone());
    let candidate_tools = crate::cli::build_tools(
        candidate_provider.clone(),
        model_val.clone(),
        &cfg,
        state.browser_enabled,
        &state.browser_url,
        mem,
        state.embed_model.clone(),
        None,
        None, // server: no interactive MCP sampling UI
    )
    .await
    .map_err(|e| bad_persist(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    crate::config::write_config_toml(path.as_path(), &cfg).map_err(|e| {
        bad_persist(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed writing config: {e}"),
        )
    })?;

    {
        let mut g = state.inner.write().await;
        g.provider = candidate_provider;
        g.tools = candidate_tools;
        g.model = model_val;
        g.system_prompt = sys;
        g.config = cfg;
    }

    Ok(Json(PersistSetupResponse {
        ok: true,
        message: "Saved to config.toml and hot-reloaded the server runtime.".into(),
    }))
}

fn bad_persist(code: StatusCode, msg: String) -> (StatusCode, Json<PersistSetupResponse>) {
    (
        code,
        Json(PersistSetupResponse {
            ok: false,
            message: msg,
        }),
    )
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn ui() -> Html<&'static str> {
    Html(UI_HTML)
}

async fn health(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<ServerState>>,
) -> Json<HealthResponse> {
    let g = state.inner.read().await;
    let loopback = addr.ip().is_loopback();
    Json(HealthResponse {
        status: "ok",
        model: g.model.clone(),
        provider_model: g.provider.model().to_string(),
        keys: HealthKeyHints {
            anthropic_env: nonempty_env("ANTHROPIC_API_KEY"),
            xai_env: nonempty_env("XAI_API_KEY"),
            openai_env: nonempty_env("OPENAI_API_KEY"),
        },
        config_path: if loopback {
            Some(state.config_active_path.display().to_string())
        } else {
            None
        },
        auth_token: if loopback {
            Some(state.auth_token.clone())
        } else {
            None
        },
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
            let branch =
                current_git_branch(&project.path).unwrap_or_else(|| "(detached HEAD)".to_string());
            let upstream = git_output(
                &project.path,
                &[
                    "rev-parse",
                    "--abbrev-ref",
                    "--symbolic-full-name",
                    "@{upstream}",
                ],
            )
            .ok();
            let remote_url = git_output(&project.path, &["remote", "get-url", "origin"]).ok();
            let changes = collect_change_counts(&project.path)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
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
            let custom_cmd = req.command.is_some();
            let cmd = req
                .command
                .unwrap_or_else(|| default_test_command(&project.path));
            if custom_cmd && !is_allowed_test_command(&cmd) {
                return Ok(Json(ProjectActionResponse {
                    ok: false,
                    message: "custom test command not allowed; use a known prefix (cargo test, npm test, pytest, go test, make test)".into(),
                    output: None,
                }));
            }
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
    headers: axum::http::HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Sse<BoxSseStream> {
    if !crate::auth_token::verify(
        extract_bearer(&headers, req.token.as_deref()).as_deref(),
        &state.auth_token,
    ) {
        return error_sse("unauthorized — send Authorization: Bearer <token>");
    }

    let (tx, rx) = agent_event_channel();

    let rt = state.inner.read().await;
    let native_web = rt.config.native_tools.web_search_enabled();
    let native_code = rt.config.native_tools.code_execution_enabled();
    let native_x = rt.config.native_tools.x_search_enabled();

    // Resolve or create session
    let mut session = match req.session_id.as_deref() {
        Some(id) => match state.session_store.find(id) {
            Ok(Some(s)) => s,
            Ok(None) => return error_sse(format!("session not found: {id}")),
            Err(e) => return error_sse(e),
        },
        None => Session::new(&rt.model),
    };

    session.push(Message::user(&req.prompt));

    let provider = rt.provider.clone();
    let tools = rt.tools.clone();
    let sys = rt.system_prompt.clone();
    drop(rt);
    let mem = state.memory_store.as_ref().map(|m| (**m).clone());
    let em = state.embed_model.clone();
    let store = state.session_store.clone();
    let mem_store = state.memory_store.as_ref().map(|m| (**m).clone());
    let em2 = state.embed_model.clone();

    let session_id_str = session.id.clone();

    tokio::spawn(async move {
        if let Err(e) = agent::drive_agent_full(
            &provider,
            &tools,
            mem.as_ref(),
            em.as_deref(),
            &mut session,
            &sys,
            Some(&tx),
            None,
            native_web,
            native_code,
            native_x,
            None,
        )
        .await
        {
            try_emit(Some(&tx), AgentEvent::Error(format!("Agent error: {e}")));
        }

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
    let collab_broadcast = state.collab.clone();
    let collab_sid = session_id_str.clone();
    let sid_for_event = session_id_str.clone();
    let session_id_event = stream::once(async move {
        let data = format!(
            r#"{{"type":"session_id","id":{}}}"#,
            serde_json::to_string(&sid_for_event).unwrap_or_default()
        );
        Ok::<Event, std::convert::Infallible>(Event::default().data(data))
    });

    // Convert AgentEvent stream into SSE
    let event_stream = ReceiverStream::new(rx).map(move |event| {
        if let (Some(reg), Some(ce)) = (&collab_broadcast, agent_event_to_collab(&event)) {
            collab::broadcast_to_session(reg, &collab_sid, ce);
        }
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
            AgentEvent::ContextCompacted {
                messages_before,
                messages_after,
            } => {
                format!(
                    r#"{{"type":"context_compacted","messages_before":{messages_before},"messages_after":{messages_after}}}"#
                )
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


#[cfg(test)]
mod tests {
    use super::*;
    use harness_memory::SessionStore;
    use harness_provider_ollama::{OllamaConfig, OllamaProvider};
    use harness_tools::{ToolExecutor, ToolRegistry};
    use std::path::PathBuf;
    use tempfile::{tempdir, TempDir};

    async fn spawn_test_server() -> (
        String,
        tokio::task::JoinHandle<()>,
        Arc<SessionStore>,
        TempDir,
    ) {
        let provider =
            OllamaProvider::new(OllamaConfig::new("qwen3-coder:30b")).expect("build test provider");
        let session_dir = tempdir().expect("temp session dir");
        let session_db = session_dir.path().join("sessions.db");
        let session_store = Arc::new(SessionStore::open(&session_db).expect("open session db"));

        let inner = ServeRuntimeState {
            provider: Arc::new(provider),
            tools: ToolExecutor::new(ToolRegistry::new()),
            model: "test-model".to_string(),
            system_prompt: "You are a test assistant.".to_string(),
            config: crate::config::Config::default(),
        };

        let state = ServerState {
            inner: Arc::new(tokio::sync::RwLock::new(inner)),
            session_store: session_store.clone(),
            memory_store: None,
            embed_model: None,
            browser_enabled: false,
            browser_url: String::new(),
            config_active_path: Arc::new(PathBuf::from("/tmp/harness-test-config.toml")),
            auth_token: "test-token".to_string(),
            collab: None,
        };

        let app = router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test listener");
        let addr = listener.local_addr().expect("listener addr");
        let handle = tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await;
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
        assert!(
            ui.status().is_success(),
            "unexpected / status: {}",
            ui.status()
        );
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
        assert_eq!(payload["provider_model"], "qwen3-coder:30b");
        assert!(
            payload
                .get("config_path")
                .and_then(|v| v.as_str())
                .is_some(),
            "health should include config_path"
        );
        assert!(
            payload.get("key_env").is_some(),
            "health should include key_env map"
        );

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
        let auth = |req: reqwest::RequestBuilder| req.header("Authorization", "Bearer test-token");

        let sessions = auth(client.get(format!("{base_url}/api/sessions")))
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

        let loaded = auth(client.get(format!(
            "{base_url}/api/sessions/{}",
            urlencoding::encode(&session.id)
        )))
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
