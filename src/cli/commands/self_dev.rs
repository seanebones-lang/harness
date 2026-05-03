//! `harness self-dev` — rebuild/reload tools + TUI with a self-modification system prompt.

use anyhow::Result;
use harness_provider_core::ArcProvider;
use harness_tools::tools::{
    ListDirTool, PatchFileTool, ReadFileTool, RebuildSelfTool, ReloadSelfTool, SearchCodeTool,
    ShellConfig as ToolShellConfig, ShellTool, WriteFileTool,
};
use harness_tools::{SandboxMode, ToolExecutor, ToolRegistry, WorkspaceRoot};
use std::path::PathBuf;
use std::sync::Arc;

use crate::config;
use crate::tui;

const SELF_DEV_SYSTEM: &str = "\
You are harness, a Rust coding agent, running in self-development mode.
You have access to your own source code and can modify it.
You can also use browser automation (when enabled), MCP tools (when configured),
and spawn sub-agents for parallel tasks.

Source layout:
  src/main.rs          — CLI dispatch, agent entry points
  src/agent.rs         — core agent loop, memory injection
  src/tui/             — two-panel ratatui TUI
  src/highlight.rs     — syntect syntax highlighting
  src/server.rs        — axum HTTP/SSE server
  src/events.rs        — AgentEvent enum
  src/config.rs        — TOML config structs
  crates/harness-provider-core/  — Provider trait, Message/Delta types
  crates/harness-provider-xai/   — Grok streaming client, embeddings
  crates/harness-tools/          — Tool trait, built-in tools (file/shell/search/spawn/selfdev)
  crates/harness-memory/         — SQLite session & vector memory store
  crates/harness-mcp/            — MCP stdio protocol client

Workflow:
1. Read relevant source files before editing.
2. Make targeted edits with write_file or shell (use patch/sed for small changes).
3. Call rebuild_self to check your changes compile (use check_only=true for a fast check).
4. Fix any compiler errors, then rebuild.
5. Once the build succeeds, call reload_self to hot-swap to the new binary.

Be methodical: one change at a time, verify compilation before proceeding.
Prefer small, well-understood edits over large rewrites.";

/// Self-dev mode: tools with rebuild/reload, TUI with self-dev system prompt.
pub async fn run_self_dev(
    provider: ArcProvider,
    session_store: harness_memory::SessionStore,
    memory_store: Option<harness_memory::MemoryStore>,
    embed_model: Option<String>,
    src_dir: PathBuf,
    model: String,
    cfg: &config::Config,
) -> Result<()> {
    tracing::info!(src = %src_dir.display(), model = %model, "starting self-dev mode");

    let workspace = Arc::new(WorkspaceRoot::new(
        src_dir.clone(),
        SandboxMode::from_config(cfg.tools.sandbox.as_deref()),
    )?);

    let shell_cfg = ToolShellConfig {
        denylist: cfg.shell.effective_denylist(),
        confirm_required: cfg.shell.effective_confirm_required(),
        log_path: cfg
            .shell
            .log_path
            .clone()
            .or_else(|| dirs::home_dir().map(|h| h.join(".harness").join("shell.log"))),
        cmd_allowlist: cfg.shell.cmd_allowlist.clone(),
    };

    let mut registry = ToolRegistry::new();
    registry.register(ReadFileTool {
        workspace: workspace.clone(),
    });
    registry.register(WriteFileTool {
        workspace: workspace.clone(),
    });
    registry.register(PatchFileTool {
        workspace: workspace.clone(),
    });
    registry.register(ListDirTool {
        workspace: workspace.clone(),
    });
    registry.register(ShellTool::new(shell_cfg, workspace.clone()));
    registry.register(SearchCodeTool);
    registry.register(RebuildSelfTool::new(src_dir.clone()));
    registry.register(ReloadSelfTool::new(src_dir.clone()));

    let tools = ToolExecutor::new(registry);

    let mut sd_cfg = config::Config {
        provider: config::ProviderConfig {
            model: Some(model.clone()),
            ..Default::default()
        },
        agent: config::AgentConfig {
            system_prompt: Some(SELF_DEV_SYSTEM.to_string()),
        },
        ..Default::default()
    };
    sd_cfg.session = cfg.session.clone();
    sd_cfg.memory = cfg.memory.clone();
    sd_cfg.tools = cfg.tools.clone();

    tui::run(
        provider,
        session_store,
        memory_store,
        embed_model,
        tools,
        model,
        sd_cfg,
        None,
        None,
        None,
    )
    .await
}
