//! Shared HTTP server state.

use harness_memory::{MemoryStore, SessionStore};
use harness_provider_core::ArcProvider;
use harness_tools::ToolExecutor;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::collab::CollabRegistry;

/// Hot-reloaded fields (persist from dashboard writes `config.toml`, then swaps these).
pub struct ServeRuntimeState {
    pub provider: ArcProvider,
    pub tools: ToolExecutor,
    pub model: String,
    pub system_prompt: String,
    pub config: crate::config::Config,
}

#[derive(Clone)]
pub struct ServerState {
    pub inner: Arc<RwLock<ServeRuntimeState>>,
    pub session_store: Arc<SessionStore>,
    pub memory_store: Option<Arc<MemoryStore>>,
    pub embed_model: Option<String>,
    pub browser_enabled: bool,
    pub browser_url: String,
    pub config_active_path: Arc<PathBuf>,
    /// Bearer token required for mutating / sensitive API routes.
    pub auth_token: String,
    /// Live session broadcast registry when `[collab].enabled`.
    pub collab: Option<CollabRegistry>,
}
