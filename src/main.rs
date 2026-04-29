mod agent;
mod ambient;
mod background;
mod checkpoint;
mod config;
mod cost;
mod daemon;
mod events;
mod highlight;
mod server;
mod trust;
mod tui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use harness_browser::BrowserTool;
use harness_lsp::{
    DiagnosticsTool, FindDefinitionTool, FindReferencesTool, RenameSymbolTool,
    detect_and_spawn,
};
use harness_provider_core::ArcProvider;
use harness_provider_router::ProviderRouter;
use harness_provider_xai::{XaiConfig, XaiProvider};
use harness_tools::{ToolExecutor, ToolRegistry};
use harness_tools::tools::{
    ApplyPatchTool, GitTool, ListDirTool, PatchFileTool, ReadFileTool, RebuildSelfTool,
    ReloadSelfTool, SearchCodeTool, ShellConfig as ToolShellConfig, ShellTool, SpawnAgentTool,
    TestRunnerTool, WriteFileTool,
};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser)]
#[command(name = "harness", about = "Rust coding agent harness powered by Grok (xAI)", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Prompt to run in non-interactive mode.
    prompt: Option<String>,

    /// Resume a session by id prefix or name.
    #[arg(long, short)]
    resume: Option<String>,

    /// Config file path (default: ~/.harness/config.toml or .harness/config.toml).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Model override (e.g. grok-3, grok-3-fast, grok-3-mini).
    #[arg(long, short)]
    model: Option<String>,

    /// Disable semantic memory recall for this run.
    #[arg(long)]
    no_memory: bool,

    /// Enable browser tool (requires Chrome with --remote-debugging-port=9222).
    #[arg(long)]
    browser: bool,

    /// Chrome DevTools remote URL (default: http://localhost:9222).
    #[arg(long, default_value = "http://localhost:9222")]
    browser_url: String,

    /// Verbose logging.
    #[arg(long, short)]
    verbose: bool,

    /// Plan mode: preview file writes, patches, and shell commands before they execute.
    /// In TUI, press Enter to approve or Esc to skip each change.
    #[arg(long)]
    plan: bool,

    /// Attach an image file to the initial prompt (PNG, JPEG, GIF, WEBP).
    #[arg(long)]
    image: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// List recent sessions.
    Sessions,
    /// Run a single prompt non-interactively.
    Run { prompt: String },
    /// Start the harness HTTP server.
    Serve {
        /// Address to listen on.
        #[arg(long, default_value = "127.0.0.1:8787")]
        addr: String,
    },
    /// Connect to a running harness server and chat via SSE.
    Connect {
        /// Server base URL.
        #[arg(default_value = "http://127.0.0.1:8787")]
        url: String,
        /// Prompt to send.
        prompt: String,
        /// Existing session id to continue.
        #[arg(long)]
        session: Option<String>,
    },
    /// Run harness in self-development mode: the agent can edit its own source
    /// and trigger rebuilds via the rebuild_self and reload_self tools.
    SelfDev {
        /// Directory containing harness source (defaults to current dir).
        #[arg(long)]
        src: Option<PathBuf>,
        /// Grok model to use (recommend grok-3 for self-dev).
        #[arg(long)]
        model: Option<String>,
    },
    /// Export a session as Markdown.
    Export {
        /// Session id prefix or name.
        id: String,
        /// Output file path (defaults to stdout).
        #[arg(long, short)]
        output: Option<PathBuf>,
    },
    /// Delete a session by id prefix or full id.
    Delete {
        /// Session id prefix or full id.
        id: String,
    },
    /// Start the harness daemon (long-lived process over ~/.harness/daemon.sock).
    /// The daemon holds provider clients, SQLite, LSP servers, and ambient memory.
    /// Other harness processes auto-connect to the daemon when it's running.
    Daemon,
    /// Check if the harness daemon is running and print its status.
    DaemonStatus,
    /// Run a prompt as a background agent (detached process).
    /// Output is streamed to ~/.harness/runs/<id>/output.log.
    RunBg {
        /// Prompt to run in the background.
        prompt: String,
    },
    /// List recent background runs.
    Runs,
    /// Add a tool auto-approval rule (skip confirmation for matching calls).
    /// Example: harness trust shell "cargo check"
    Trust {
        /// Tool name (e.g. shell, write_file, git, *).
        tool: String,
        /// Pattern to match in the first argument (use * for all).
        pattern: String,
    },
    /// Remove a previously added trust rule.
    Untrust {
        tool: String,
        pattern: String,
    },
    /// List all trust rules.
    TrustList,
    /// Set up harness for the first time (writes ~/.harness/config.toml).
    /// Pass --project to also write a project-level .harness/config.toml in CWD.
    Init {
        /// Also create a project-local .harness/config.toml in the current directory.
        #[arg(long)]
        project: bool,
        /// Overwrite existing config files without prompting.
        #[arg(long)]
        force: bool,
    },
    /// Show harness configuration and environment status.
    Status,
    /// Restore the most recent harness checkpoint stash (undo last agent turn).
    Undo,
    /// Manage harness checkpoint stashes.
    Checkpoint {
        #[command(subcommand)]
        action: CheckpointAction,
    },
}

#[derive(Subcommand)]
enum CheckpointAction {
    /// List all harness checkpoint stashes.
    List,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Auto-load .env from CWD or any parent directory (no-op if not found).
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    let filter = if cli.verbose {
        EnvFilter::new("harness=debug,harness_provider_xai=debug,harness_mcp=debug")
    } else {
        EnvFilter::new("harness=info,harness_mcp=warn")
    };
    fmt().with_env_filter(filter).with_target(false).init();

    let cfg = config::load(cli.config.as_deref())?;

    let api_key = cfg.provider.api_key.clone()
        .or_else(|| std::env::var("XAI_API_KEY").ok())
        .context("XAI_API_KEY not set and no api_key in config")?;

    let model = cli
        .model
        .or_else(|| cfg.provider.model.clone())
        .unwrap_or_else(|| "grok-3-fast".into());

    let xai_cfg = XaiConfig::new(&api_key)
        .with_model(&model)
        .with_max_tokens(cfg.provider.max_tokens.unwrap_or(8192))
        .with_temperature(cfg.provider.temperature.unwrap_or(0.7));

    let provider: ArcProvider = if !cfg.providers.is_empty() {
        // Multi-provider mode: build a router.
        let router = ProviderRouter::from_config(&cfg.providers, &cfg.router)
            .context("failed to build provider router")?;
        // If no explicit default in router config, try the legacy [provider] block as xai fallback.
        if !cfg.providers.contains_key("xai") {
            // Add the legacy xai provider to the router.
        }
        router.into_arc()
    } else {
        std::sync::Arc::new(XaiProvider::new(xai_cfg)?)
    };

    let session_store = harness_memory::SessionStore::open(
        cfg.session.db_path.clone()
            .unwrap_or_else(harness_memory::SessionStore::default_path),
    )?;

    let memory_db = cfg.memory.db_path.clone()
        .or_else(|| cfg.session.db_path.clone())
        .unwrap_or_else(harness_memory::SessionStore::default_path);

    let memory_store = if !cli.no_memory && cfg.memory.enabled.unwrap_or(true) {
        Some(harness_memory::MemoryStore::open(memory_db)?)
    } else {
        None
    };

    let embed_model = if memory_store.is_some() {
        Some(cfg.memory.embed_model.clone().unwrap_or_else(|| "grok-3-embed-english".into()))
    } else {
        None
    };

    // Start ambient memory consolidation if memory is enabled.
    // ambient_shutdown is (sender, join_handle); send () then await handle for a clean exit.
    let ambient_shutdown: Option<(tokio::sync::watch::Sender<()>, tokio::task::JoinHandle<()>)> =
        if let (Some(mem), Some(em)) = (&memory_store, &embed_model) {
            let mem_arc = std::sync::Arc::new(mem.clone());
            Some(ambient::spawn(provider.clone(), mem_arc, em.clone()))
        } else {
            None
        };

    // CLI --browser flag overrides config; config.browser.enabled is the opt-in default.
    let browser_enabled = cli.browser || cfg.browser.enabled.unwrap_or(false);
    let browser_url = cfg.browser.url.clone().unwrap_or(cli.browser_url);

    // Build tools (including MCP servers if config exists).
    let tools = build_tools(provider.clone(), model.clone(), &cfg, browser_enabled, &browser_url).await;

    match cli.command {
        Some(Commands::Sessions) => {
            list_sessions(&session_store)?;
        }

        Some(Commands::Run { prompt }) => {
            let effective_prompt = build_prompt_with_image(&prompt, cli.image.as_deref())?;
            agent::run_once(
                &provider, &session_store,
                memory_store.as_ref(), embed_model.as_deref(),
                &tools, &model,
                cfg.agent.system_prompt.as_deref(),
                &effective_prompt, cli.resume.as_deref(),
            ).await?;
        }

        Some(Commands::Serve { addr }) => {
            let addr: std::net::SocketAddr = addr.parse().context("invalid address")?;
            let state = server::ServerState {
                provider,
                session_store: Arc::new(session_store),
                memory_store: memory_store.map(Arc::new),
                embed_model,
                tools,
                model,
                system_prompt: cfg.agent.system_prompt
                    .unwrap_or_else(|| agent::DEFAULT_SYSTEM.to_string()),
            };
            server::serve(state, addr).await?;
        }

        Some(Commands::Connect { url, prompt, session }) => {
            connect_to_server(&url, &prompt, session.as_deref()).await?;
        }

        Some(Commands::Export { id, output }) => {
            export_session(&session_store, &id, output.as_deref())?;
        }

        Some(Commands::Delete { id }) => {
            delete_session(&session_store, &id)?;
        }

        Some(Commands::Init { project, force }) => {
            run_init(project, force)?;
            return Ok(());
        }

        Some(Commands::Status) => {
            run_status(&cfg, &model, &session_store, &api_key)?;
            return Ok(());
        }

        Some(Commands::Trust { tool, pattern }) => {
            let mut store = trust::TrustStore::load();
            if store.add_rule(&tool, &pattern) {
                store.save()?;
                println!("Trust rule added: {tool} / {pattern}");
            } else {
                println!("Rule already exists: {tool} / {pattern}");
            }
            return Ok(());
        }

        Some(Commands::Untrust { tool, pattern }) => {
            let mut store = trust::TrustStore::load();
            if store.remove_rule(&tool, &pattern) {
                store.save()?;
                println!("Trust rule removed: {tool} / {pattern}");
            } else {
                println!("No matching rule: {tool} / {pattern}");
            }
            return Ok(());
        }

        Some(Commands::TrustList) => {
            let store = trust::TrustStore::load();
            let rules = store.list();
            if rules.is_empty() {
                println!("No trust rules. Use `harness trust <tool> <pattern>` to add one.");
            } else {
                println!("{:<20} {:<40} ADDED", "TOOL", "PATTERN");
                for rule in rules {
                    println!("{:<20} {:<40} {}", rule.tool, rule.pattern, rule.added);
                }
            }
            return Ok(());
        }

        Some(Commands::Daemon) => {
            println!("Starting harness daemon…");
            println!("Socket: {}", daemon::socket_path().display());
            println!("Press Ctrl+C to stop.");
            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(());
            let system = cfg.agent.system_prompt.clone()
                .unwrap_or_else(|| agent::DEFAULT_SYSTEM.to_string());
            tokio::select! {
                res = daemon::run_daemon(
                    provider, session_store, memory_store, embed_model,
                    tools, model, system, shutdown_rx
                ) => {
                    if let Err(e) = res { eprintln!("daemon: {e}"); }
                }
                _ = tokio::signal::ctrl_c() => {
                    println!("\nDaemon stopped.");
                    let _ = shutdown_tx.send(());
                }
            }
            return Ok(());
        }

        Some(Commands::DaemonStatus) => {
            if daemon::is_running().await {
                match daemon::send_request(&daemon::DaemonRequest {
                    id: 1,
                    method: "status".into(),
                    params: serde_json::json!({}),
                }).await {
                    Ok(resp) => {
                        if let Some(result) = resp.result {
                            println!("Daemon running: {}", serde_json::to_string_pretty(&result)?);
                        }
                    }
                    Err(e) => eprintln!("daemon status: {e}"),
                }
            } else {
                println!("Daemon is not running.");
                println!("Start with: harness daemon");
            }
            return Ok(());
        }

        Some(Commands::RunBg { prompt }) => {
            match background::spawn(&prompt) {
                Ok(id) => {
                    println!("Background run started: {id}");
                    println!("Output: ~/.harness/runs/{id}/output.log");
                    println!("Status: harness runs");
                }
                Err(e) => eprintln!("run-bg: {e}"),
            }
            return Ok(());
        }

        Some(Commands::Runs) => {
            let runs = background::list(20)?;
            if runs.is_empty() {
                println!("No background runs yet.");
            } else {
                println!("{:<10} {:<8} {:<25} PROMPT", "ID", "STATUS", "STARTED");
                for run in runs {
                    let prompt_preview = if run.prompt.len() > 40 {
                        format!("{}…", &run.prompt[..40])
                    } else {
                        run.prompt.clone()
                    };
                    println!("{:<10} {:<8} {:<25} {}", run.id, run.status, run.started_at, prompt_preview);
                }
            }
            return Ok(());
        }

        Some(Commands::Undo) => {
            match checkpoint::undo() {
                Ok(msg) => println!("{msg}"),
                Err(e) => eprintln!("undo: {e}"),
            }
            return Ok(());
        }

        Some(Commands::Checkpoint { action: CheckpointAction::List }) => {
            let entries = checkpoint::list()?;
            if entries.is_empty() {
                println!("No harness checkpoint stashes found.");
            } else {
                println!("{:<12} MESSAGE", "STASH");
                for (stash_ref, msg) in entries {
                    println!("{:<12} {}", stash_ref, msg);
                }
            }
            return Ok(());
        }

        Some(Commands::SelfDev { src, model: sd_model }) => {
            let src_dir = match src {
                Some(path) => path,
                None => std::env::current_dir().context("failed to get current directory")?,
            };
            let sd_model = sd_model.unwrap_or_else(|| model.clone());
            run_self_dev(provider, session_store, memory_store, embed_model, src_dir, sd_model, &cfg).await?;
        }

        None => {
            if let Some(prompt) = cli.prompt {
                let effective_prompt = build_prompt_with_image(&prompt, cli.image.as_deref())?;
                agent::run_once(
                    &provider, &session_store,
                    memory_store.as_ref(), embed_model.as_deref(),
                    &tools, &model,
                    cfg.agent.system_prompt.as_deref(),
                    &effective_prompt, cli.resume.as_deref(),
                ).await?;
            } else {
                let ambient_tx = ambient_shutdown.as_ref().map(|(tx, _)| tx.clone());
                // In plan mode, create a confirm gate channel and pass it to both the
                // tools executor and the TUI (which will handle the confirmation prompts).
                let (tools, confirm_rx) = if cli.plan {
                    let (gate, rx) = harness_tools::confirm::channel();
                    (tools.with_confirm_gate(gate), Some(rx))
                } else {
                    (tools, None)
                };
                let result = tui::run(
                    provider,
                    session_store,
                    memory_store,
                    embed_model,
                    tools,
                    model,
                    cfg,
                    cli.resume.as_deref(),
                    ambient_tx,
                    confirm_rx,
                )
                .await;
                graceful_ambient_shutdown(ambient_shutdown).await;
                result?;
                return Ok(());
            }
        }
    }

    graceful_ambient_shutdown(ambient_shutdown).await;
    Ok(())
}

/// Signal the ambient consolidation task to stop and wait up to 5 s for it.
async fn graceful_ambient_shutdown(
    ambient: Option<(tokio::sync::watch::Sender<()>, tokio::task::JoinHandle<()>)>,
) {
    if let Some((tx, handle)) = ambient {
        let _ = tx.send(());
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), handle).await;
    }
}

/// Build the full tool executor: base tools + SpawnAgentTool + MCP tools.
async fn build_tools(provider: ArcProvider, model: String, cfg: &config::Config, browser_enabled: bool, browser_url: &str) -> ToolExecutor {
    let shell_cfg = ToolShellConfig {
        denylist: cfg.shell.effective_denylist(),
        confirm_required: cfg.shell.effective_confirm_required(),
        log_path: cfg.shell.log_path.clone().or_else(|| {
            dirs::home_dir().map(|h| h.join(".harness").join("shell.log"))
        }),
    };

    // Sub-agent runner: runs a prompt through a fresh session with base tools only.
    let sub_provider: ArcProvider = provider.clone();
    let sub_model = model.clone();
    let sub_shell_cfg = shell_cfg.clone();
    let runner: harness_tools::tools::agent::SubAgentRunner = Arc::new(move |task: String| {
        let p: ArcProvider = sub_provider.clone();
        let m = sub_model.clone();
        let scfg = sub_shell_cfg.clone();
        let sub_tools = {
            let mut r = ToolRegistry::new();
            r.register(ReadFileTool);
            r.register(WriteFileTool);
            r.register(PatchFileTool);
            r.register(ListDirTool);
            r.register(ShellTool::new(scfg));
            r.register(SearchCodeTool);
            ToolExecutor::new(r)
        };
        Box::pin(async move {
            use harness_memory::Session;
            use harness_provider_core::Message;
            let mut session = Session::new(&m);
            session.push(Message::user(&task));
            agent::drive_agent(&p, &sub_tools, None, None, &mut session, agent::DEFAULT_SYSTEM, None).await?;
            let reply = session.messages.iter().rev()
                .find(|m| matches!(m.role, harness_provider_core::Role::Assistant))
                .map(|m| m.content.as_str().to_string())
                .unwrap_or_else(|| "(no response)".into());
            Ok(reply)
        })
    });

    let mut registry = ToolRegistry::new();
    registry.register(ReadFileTool);
    registry.register(WriteFileTool);
    registry.register(PatchFileTool);
    registry.register(ApplyPatchTool);
    registry.register(ListDirTool);
    registry.register(ShellTool::new(shell_cfg));
    registry.register(SearchCodeTool);
    registry.register(GitTool);
    registry.register(TestRunnerTool);
    registry.register(SpawnAgentTool::new(runner));

    if browser_enabled {
        registry.register(BrowserTool::new(browser_url));
        tracing::info!(url = %browser_url, "browser tool enabled");
    }

    // Auto-detect and spawn a language server for the current project.
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    if let Some(lsp) = detect_and_spawn(&cwd).await {
        registry.register(FindDefinitionTool { client: lsp.clone() });
        registry.register(FindReferencesTool { client: lsp.clone() });
        registry.register(RenameSymbolTool { client: lsp.clone() });
        registry.register(DiagnosticsTool { client: lsp });
    }

    // Load MCP tools if a config file exists.
    if let Some(mcp_path) = harness_mcp::find_config() {
        if let Err(e) = harness_mcp::load_mcp_tools(&mcp_path, &mut registry).await {
            tracing::warn!("MCP load failed: {e}");
        }
    }

    // Also check config-specified MCP path.
    if let Some(mcp_path) = &cfg.mcp.config_path {
        if mcp_path.exists() {
            if let Err(e) = harness_mcp::load_mcp_tools(mcp_path, &mut registry).await {
                tracing::warn!("MCP load failed: {e}");
            }
        }
    }

    let executor = ToolExecutor::new(registry);

    // Wire autotest if enabled in config.
    let executor = if cfg.autotest.enabled {
        executor.with_autotest(cfg.autotest.scope.clone())
    } else {
        executor
    };

    // Load trust rules.
    let trust_store = trust::TrustStore::load();
    let trusted_rules: Vec<(String, String)> = trust_store.list().iter()
        .map(|r| (r.tool.clone(), r.pattern.clone()))
        .collect();

    if trusted_rules.is_empty() {
        executor
    } else {
        executor.with_trusted(trusted_rules)
    }
}

/// Minimal SSE client for `harness connect`: streams events from server to stdout.
async fn connect_to_server(base_url: &str, prompt: &str, session_id: Option<&str>) -> Result<()> {
    let client = reqwest::Client::new();
    let mut body = serde_json::json!({ "prompt": prompt });
    if let Some(id) = session_id {
        body["session_id"] = serde_json::Value::String(id.to_string());
    }

    let resp = client
        .post(format!("{base_url}/api/chat"))
        .json(&body)
        .send()
        .await
        .context("connecting to harness server")?;

    if !resp.status().is_success() {
        let msg = resp.text().await.context("reading error body")?;
        anyhow::bail!("server error: {msg}");
    }

    use futures::StreamExt;
    let mut byte_stream = resp.bytes_stream();
    let mut buf = String::new();

    while let Some(chunk) = byte_stream.next().await {
        let bytes: bytes::Bytes = chunk.context("reading SSE stream")?;
        buf.push_str(&String::from_utf8_lossy(&bytes));

        // Process complete SSE lines
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim_end_matches('\r').to_string();
            buf = buf[pos + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                    match event["type"].as_str() {
                        Some("text_chunk") => {
                            print!("{}", event["content"].as_str().unwrap_or(""));
                            use std::io::Write;
                            std::io::stdout().flush().ok();
                        }
                        Some("tool_start") => eprintln!("\n[→ {}]", event["name"].as_str().unwrap_or("")),
                        Some("tool_result") => eprintln!("[← {}]", event["name"].as_str().unwrap_or("")),
                        Some("done") => { println!(); break; }
                        Some("error") => {
                            eprintln!("error: {}", event["message"].as_str().unwrap_or("unknown"));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

/// Self-dev mode: builds a tool executor with rebuild/reload tools, then launches
/// the TUI with a self-dev system prompt so the agent can modify its own source.
async fn run_self_dev(
    provider: ArcProvider,
    session_store: harness_memory::SessionStore,
    memory_store: Option<harness_memory::MemoryStore>,
    embed_model: Option<String>,
    src_dir: PathBuf,
    model: String,
    cfg: &config::Config,
) -> Result<()> {
    tracing::info!(src = %src_dir.display(), model = %model, "starting self-dev mode");

    let mut registry = ToolRegistry::new();
    registry.register(ReadFileTool);
    registry.register(WriteFileTool);
    registry.register(PatchFileTool);
    registry.register(ListDirTool);
    registry.register(ShellTool::default());
    registry.register(SearchCodeTool);
    registry.register(RebuildSelfTool::new(src_dir.clone()));
    registry.register(ReloadSelfTool::new(src_dir.clone()));

    let tools = ToolExecutor::new(registry);

    // Tailor the config for self-dev
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
    // Keep memory/session settings from user config
    sd_cfg.session = cfg.session.clone();
    sd_cfg.memory = cfg.memory.clone();

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

const SELF_DEV_SYSTEM: &str = "\
You are harness, a Rust coding agent, running in self-development mode.
You have access to your own source code and can modify it.
You can also use browser automation (when enabled), MCP tools (when configured),
and spawn sub-agents for parallel tasks.

Source layout:
  src/main.rs          — CLI, tool wiring, self-dev entry point
  src/agent.rs         — core agent loop, memory injection
  src/tui.rs           — two-panel ratatui TUI
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

fn export_session(
    store: &harness_memory::SessionStore,
    id: &str,
    output: Option<&std::path::Path>,
) -> Result<()> {
    use harness_provider_core::Role;
    use std::fmt::Write as FmtWrite;

    let session = store
        .find(id)?
        .ok_or_else(|| anyhow::anyhow!("session not found: '{}'. Use 'harness sessions' to list available sessions.", id))?;

    let mut md = String::new();

    // Front matter
    let title = session.name.as_deref().unwrap_or("Untitled session");
    writeln!(md, "# {title}")?;
    writeln!(md)?;
    writeln!(md, "**Session:** `{}`  ", session.id)?;
    writeln!(md, "**Model:** {}  ", session.model)?;
    writeln!(
        md,
        "**Created:** {}  ",
        session.created_at.format("%Y-%m-%d %H:%M UTC")
    )?;
    writeln!(md)?;
    writeln!(md, "---")?;
    writeln!(md)?;

    let mut turn = 0usize;
    for msg in &session.messages {
        match msg.role {
            Role::System => {
                writeln!(md, "> **System:** {}", msg.content.as_str())?;
                writeln!(md)?;
            }
            Role::User => {
                turn += 1;
                writeln!(md, "## Turn {turn} — User")?;
                writeln!(md)?;
                writeln!(md, "{}", msg.content.as_str())?;
                writeln!(md)?;
            }
            Role::Assistant => {
                let content = msg.content.as_str();
                if content.starts_with("__tool_calls__:") {
                    // Decode tool calls for readability
                    if let Some(json) = content.strip_prefix("__tool_calls__:") {
                        if let Ok(calls) =
                            serde_json::from_str::<serde_json::Value>(json)
                        {
                            if let Some(arr) = calls.as_array() {
                                for call in arr {
                                    let name = call["function"]["name"]
                                        .as_str()
                                        .unwrap_or("?");
                                    let args = call["function"]["arguments"]
                                        .as_str()
                                        .unwrap_or("{}");
                                    let pretty = serde_json::from_str::<serde_json::Value>(args)
                                        .map(|v| {
                                            serde_json::to_string_pretty(&v)
                                                .unwrap_or_else(|_| args.to_string())
                                        })
                                        .unwrap_or_else(|_| args.to_string());
                                    writeln!(md, "**→ `{name}`**")?;
                                    writeln!(md, "```json")?;
                                    writeln!(md, "{pretty}")?;
                                    writeln!(md, "```")?;
                                    writeln!(md)?;
                                }
                            }
                        }
                    }
                } else {
                    writeln!(md, "## Turn {turn} — Assistant")?;
                    writeln!(md)?;
                    writeln!(md, "{content}")?;
                    writeln!(md)?;
                }
            }
            Role::Tool => {
                let result = msg.content.as_str();
                // Truncate very long tool outputs
                let display = if result.len() > 2000 {
                    format!("{}\n\n_… ({} bytes truncated)_", &result[..2000], result.len() - 2000)
                } else {
                    result.to_string()
                };
                writeln!(md, "**← tool result**")?;
                writeln!(md, "```")?;
                writeln!(md, "{display}")?;
                writeln!(md, "```")?;
                writeln!(md)?;
            }
        }
    }

    match output {
        Some(path) => {
            std::fs::write(path, &md)?;
            eprintln!("Exported {} turns to {}", turn, path.display());
        }
        None => print!("{md}"),
    }

    Ok(())
}

fn list_sessions(store: &harness_memory::SessionStore) -> Result<()> {
    let sessions = store.list(20)?;
    if sessions.is_empty() {
        println!("No sessions yet.");
        return Ok(());
    }
    println!("{:<10} {:<24} UPDATED", "ID", "NAME");
    for (id, name, updated) in sessions {
        let short = id.chars().take(8).collect::<String>();
        println!("{:<10} {:<24} {}", short, name.unwrap_or_default(), updated);
    }
    Ok(())
}

fn run_init(project: bool, force: bool) -> Result<()> {
    use std::io::{self, Write};

    // ── Global config ──────────────────────────────────────────────────────────
    let global_dir = dirs::home_dir()
        .context("cannot determine home directory")?
        .join(".harness");
    std::fs::create_dir_all(&global_dir)?;
    let global_cfg = global_dir.join("config.toml");

    if global_cfg.exists() && !force {
        println!("Global config already exists at {}", global_cfg.display());
        println!("Run `harness init --force` to overwrite it.");
    } else {
        // Read API key from env, .env (already loaded by dotenvy), or prompt.
        let api_key = std::env::var("XAI_API_KEY").unwrap_or_default();
        let api_key = if api_key.is_empty() {
            print!("Enter your xAI API key (from https://console.x.ai): ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            input.trim().to_string()
        } else {
            println!("Found XAI_API_KEY in environment — using it.");
            api_key
        };

        let config_contents = format!(
            r#"[provider]
api_key = "{api_key}"
model = "grok-3-fast"
max_tokens = 8192
temperature = 0.7

[memory]
enabled = true
embed_model = "grok-3-embed-english"

[agent]
system_prompt = """
You are a powerful coding assistant running in a terminal.
You have access to tools to read and write files, run shell commands, search code, and patch files.
You can spawn sub-agents for parallel tasks using spawn_agent.
Browser automation is available when Chrome runs with --remote-debugging-port=9222.
MCP tools are loaded automatically from .harness/mcp.json if present.
Be concise and precise. Prefer making changes over explaining; show diffs when you edit files.
Always verify your changes work by running relevant tests or build commands.
"""
"#
        );
        std::fs::write(&global_cfg, config_contents)?;
        println!("Created global config at {}", global_cfg.display());
        println!("Edit it any time: {}", global_cfg.display());
    }

    // ── Project config ─────────────────────────────────────────────────────────
    if project {
        let project_dir = std::env::current_dir()?.join(".harness");
        std::fs::create_dir_all(&project_dir)?;
        let project_cfg = project_dir.join("config.toml");

        if project_cfg.exists() && !force {
            println!("Project config already exists at {}", project_cfg.display());
            println!("Run `harness init --project --force` to overwrite it.");
        } else {
            let cwd_name = std::env::current_dir()?
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "this project".to_string());
            let project_contents = format!(
                r#"# Project-level config for {cwd_name}
# Inherits from ~/.harness/config.toml — only override what you need.

[agent]
system_prompt = """
You are a coding assistant working in the {cwd_name} project.
You have access to tools to read/write files, run shell commands, patch files, and search code.
Prefer targeted edits with patch_file over rewriting whole files.
Always run tests after changes to confirm correctness.
"""
"#
            );
            std::fs::write(&project_cfg, project_contents)?;
            println!("Created project config at {}", project_cfg.display());

            // Write system prompt as editable markdown too
            let system_md = project_dir.join("SYSTEM.md");
            if !system_md.exists() || force {
                let md = format!(
                    "# Harness system prompt — {cwd_name}\n\nEdit this file to customize the agent's behavior for this project.\nThen copy the contents into `.harness/config.toml` under `[agent] system_prompt`.\n"
                );
                std::fs::write(&system_md, md)?;
                println!("Created system prompt template at {}", system_md.display());
            }
        }
    }

    println!();
    println!("All done. Start a session with:  harness");
    println!("Resume a session with:           harness --resume <id>");
    println!("Approve changes before writing:  harness --plan");
    Ok(())
}

fn run_status(
    cfg: &config::Config,
    model: &str,
    store: &harness_memory::SessionStore,
    api_key: &str,
) -> Result<()> {
    println!("harness status\n");

    // API key source
    let key_source = if cfg.provider.api_key.is_some() {
        "config file"
    } else if std::env::var("XAI_API_KEY").is_ok() {
        "XAI_API_KEY env var"
    } else {
        "unknown"
    };
    let key_preview = if api_key.len() > 8 {
        format!("{}…{}", &api_key[..6], &api_key[api_key.len() - 4..])
    } else {
        "(too short)".to_string()
    };
    println!("  API key : {} ({})", key_preview, key_source);
    println!("  Model   : {model}");

    // Config file in use
    let cfg_path = {
        let local = std::path::PathBuf::from(".harness/config.toml");
        let global = dirs::home_dir().unwrap_or_default().join(".harness/config.toml");
        if local.exists() {
            format!("{} (project)", local.display())
        } else if global.exists() {
            format!("{} (global)", global.display())
        } else {
            "defaults (no config file found)".to_string()
        }
    };
    println!("  Config  : {cfg_path}");

    // MCP config
    let mcp_path = harness_mcp::find_config();
    println!(
        "  MCP     : {}",
        mcp_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "not configured".to_string())
    );

    // Recent sessions
    println!();
    println!("Recent sessions:");
    match store.list(5) {
        Ok(sessions) if !sessions.is_empty() => {
            for (id, name, updated) in sessions {
                let short = id.chars().take(8).collect::<String>();
                println!("  {} · {} · {}", short, name.unwrap_or_else(|| "(unnamed)".to_string()), updated);
            }
        }
        Ok(_) => println!("  (none yet)"),
        Err(e) => println!("  (error reading sessions: {e})"),
    }

    println!();
    println!("Run `harness` to start a new session.");
    Ok(())
}

/// If an image path is provided, attach it to the prompt text as a note.
/// The actual image content is embedded in the message when the provider supports it.
fn build_prompt_with_image(prompt: &str, image: Option<&std::path::Path>) -> Result<String> {
    match image {
        None => Ok(prompt.to_string()),
        Some(path) => {
            // Embed image as a base64 data URI annotation that vision-capable providers understand.
            let _content = harness_provider_core::MessageContent::with_image(prompt, &path.to_string_lossy())?;
            // For now, return the prompt with a note about the image.
            // Providers that support vision (Anthropic, OpenAI, xAI) will be updated to use
            // the Parts variant directly when MessageContent wiring is complete.
            Ok(format!("{prompt}\n\n[image attached: {}]", path.display()))
        }
    }
}

fn delete_session(store: &harness_memory::SessionStore, id: &str) -> Result<()> {
    if store.delete(id)? {
        println!("Deleted session: {id}");
    } else {
        println!("Session not found: {id}");
    }
    Ok(())
}
