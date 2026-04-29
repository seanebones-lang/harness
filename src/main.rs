mod agent;
mod ambient;
mod bridges;
mod collab;
mod diff_review;
mod observability;
mod swarm;
mod background;
mod checkpoint;
mod config;
mod cost;
mod daemon;
mod events;
mod highlight;
mod cost_db;
mod memory_project;
mod notifications;
mod server;
mod sync;
mod trust;
mod tui;

// mimalloc is linked but turso already sets the global allocator.
// We still benefit from mimalloc being in the dependency tree via turso.

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::generate;
use harness_browser::BrowserTool;
use harness_voice;
use harness_lsp::{
    DiagnosticsTool, FindDefinitionTool, FindReferencesTool, RenameSymbolTool,
    detect_and_spawn,
};
use harness_provider_core::ArcProvider;
use harness_provider_router::ProviderRouter;
use harness_provider_xai::{XaiConfig, XaiProvider};
use harness_tools::{ToolExecutor, ToolRegistry};
use harness_tools::tools::{
    ApplyPatchTool, ComputerUseTool, GhTool, GitTool, ListDirTool, PatchFileTool, ReadFileTool, RebuildSelfTool,
    ReloadSelfTool, SearchCodeTool, ShellConfig as ToolShellConfig, ShellTool, SpawnAgentTool,
    TestRunnerTool, WriteFileTool,
};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser)]
#[command(name = "harness", about = "Harness — multi-provider AI coding agent (Claude · GPT · Grok · Qwen)", long_about = "Harness is a Rust-native AI coding agent supporting Anthropic Claude 4.x, OpenAI GPT-5.x, xAI Grok 4.x, and Ollama Qwen3-Coder. Set ANTHROPIC_API_KEY, OPENAI_API_KEY, or XAI_API_KEY and run `harness` to start.", version)]
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

    /// Enable extended thinking with a token budget.
    /// Example: --think 10000. Use without value for adaptive thinking (Opus 4.7 only).
    #[arg(long, value_name = "BUDGET")]
    think: Option<u32>,
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
    /// List available providers and models, with an interactive picker to change defaults.
    Models {
        /// Set the default model (writes to .harness/config.toml). Format: "provider:model" or just "model".
        #[arg(long)]
        set: Option<String>,
    },
    /// Sync Harness state across machines via an encrypted git repository.
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    /// Show cost and usage statistics from the cost database.
    Cost {
        #[command(subcommand)]
        action: CostAction,
    },
    /// Open a PR review session pre-loaded with PR context (diff, comments, CI status).
    /// Requires gh CLI to be installed and authenticated.
    Pr {
        /// PR number.
        number: u64,
    },
    /// Store a project memory fact in .harness/memory/<topic>.md.
    /// These are automatically injected into the system prompt each session.
    Memorize {
        /// Topic name (used as filename, e.g. "architecture").
        topic: String,
        /// Fact to remember.
        fact: String,
    },
    /// Remove a project memory topic.
    Forget {
        /// Topic to remove.
        topic: String,
    },
    /// List all project memory topics.
    Memories,
    /// Record audio and transcribe via Whisper.
    /// Requires sox (brew install sox) for recording.
    Voice {
        /// Duration to record in seconds (default: 5).
        #[arg(long, short, default_value = "5")]
        duration: u64,
        /// Send transcript as a prompt to the agent instead of just printing it.
        #[arg(long)]
        send: bool,
        /// Use OpenAI Realtime API for duplex voice conversation (requires OPENAI_API_KEY).
        #[arg(long)]
        realtime: bool,
    },
    /// Manage parallel sub-agent swarm tasks.
    Swarm {
        #[command(subcommand)]
        action: SwarmAction,
    },
    /// Export observability traces.
    Trace {
        /// Trace ID to export (omit for last trace).
        id: Option<String>,
    },
    /// Run health checks: API keys, tools, config, daemon, MCP, LSP, and more.
    Doctor,
    /// Generate shell completions (bash, zsh, fish, powershell, elvish).
    Completions {
        /// Shell type.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
enum CheckpointAction {
    /// List all harness checkpoint stashes.
    List,
}

#[derive(Subcommand)]
enum SyncAction {
    /// Initialise sync with a remote git repository.
    Init {
        /// Git remote URL (e.g. git@github.com:user/harness-state.git).
        git_url: String,
    },
    /// Encrypt and push state to the remote.
    Push,
    /// Pull and decrypt state from the remote.
    Pull,
    /// Show sync status.
    Status,
    /// Show/set the sync passphrase.
    Auth,
}

#[derive(Subcommand)]
enum SwarmAction {
    /// List recent swarm tasks.
    List,
    /// Show status of a specific task.
    Status {
        /// Task ID.
        id: String,
    },
    /// Show result of a completed task.
    Result {
        /// Task ID.
        id: String,
    },
}

#[derive(Subcommand)]
enum CostAction {
    /// Show cost for today.
    Today,
    /// Show cost for the past 7 days.
    Week,
    /// Show cost for the past 30 days.
    Month,
    /// Show all-time cost.
    All,
    /// Show cost broken down by model.
    ByModel,
    /// Show cost broken down by project.
    ByProject,
    /// Tail recent usage rows live.
    Watch,
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

    // Detect available API keys (in priority order: Anthropic > xAI > OpenAI > legacy config)
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY").map(|k| !k.is_empty()).unwrap_or(false);
    let has_xai = cfg.provider.api_key.is_some()
        || std::env::var("XAI_API_KEY").map(|k| !k.is_empty()).unwrap_or(false);
    let has_openai = std::env::var("OPENAI_API_KEY").map(|k| !k.is_empty()).unwrap_or(false);
    let has_ollama = cfg.providers.contains_key("ollama");

    if !has_anthropic && !has_xai && !has_openai && !has_ollama && cfg.providers.is_empty() {
        eprintln!("harness: no API key found.\n\nSet one of:\n  ANTHROPIC_API_KEY (recommended — claude-sonnet-4-6)\n  XAI_API_KEY       (grok-4.20)\n  OPENAI_API_KEY    (gpt-5.5)\n\nOr run: harness doctor  for a guided setup.");
        std::process::exit(1);
    }

    let model = cli
        .model
        .or_else(|| cfg.provider.model.clone())
        .unwrap_or_else(|| {
            if has_anthropic { "claude-sonnet-4-6".to_string() }
            else if has_xai { "grok-4.20-0309-reasoning".to_string() }
            else if has_openai { "gpt-5.5".to_string() }
            else { "qwen3-coder:30b".to_string() }
        });

    let provider: ArcProvider = if !cfg.providers.is_empty() || has_anthropic || has_openai {
        // Smart router: builds from env vars + config
        let router = ProviderRouter::from_config(&cfg.providers, &cfg.router)
            .context("failed to build provider router")?;
        router.into_arc()
    } else if has_xai {
        let api_key = cfg.provider.api_key.clone()
            .or_else(|| std::env::var("XAI_API_KEY").ok())
            .unwrap();
        let xai_cfg = XaiConfig::new(&api_key)
            .with_model(&model)
            .with_max_tokens(cfg.provider.max_tokens.unwrap_or(8192))
            .with_temperature(cfg.provider.temperature.unwrap_or(0.7));
        std::sync::Arc::new(XaiProvider::new(xai_cfg)?)
    } else {
        // Fallback to router (handles Ollama etc.)
        let router = ProviderRouter::from_config(&cfg.providers, &cfg.router)
            .context("failed to build provider router")?;
        router.into_arc()
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

        Some(Commands::Pr { number }) => {
            use harness_tools::tools::gh::pr_context;
            eprintln!("Fetching PR #{number} context…");
            let context = pr_context(number).await.unwrap_or_else(|e| format!("Error fetching PR: {e}"));
            let system_pr = format!(
                "{}\n\n# Reviewing PR #{number}\nYou are helping review and babysit this pull request. \
                 Check the CI status, review the diff, and help address any review comments or failures.",
                cfg.agent.system_prompt.as_deref().unwrap_or(agent::DEFAULT_SYSTEM)
            );
            agent::run_once(
                &provider, &session_store,
                memory_store.as_ref(), embed_model.as_deref(),
                &tools, &model,
                Some(&system_pr),
                &context, None,
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
            let display_key = std::env::var("ANTHROPIC_API_KEY")
                .or_else(|_| std::env::var("XAI_API_KEY"))
                .or_else(|_| std::env::var("OPENAI_API_KEY"))
                .unwrap_or_default();
            run_status(&cfg, &model, &session_store, &display_key)?;
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

        Some(Commands::Voice { duration, send, realtime }) => {
            use harness_voice::{WhisperBackend, record_and_transcribe};
            use std::time::Duration;

            let openai_key = std::env::var("OPENAI_API_KEY").ok();

            if realtime {
                let key = openai_key.clone().context("OPENAI_API_KEY required for realtime voice")?;
                eprintln!("Starting realtime voice session (Ctrl+C to stop)…");
                eprintln!("Connect to the OpenAI Realtime API — speak naturally.");
                let mut session = harness_voice::RealtimeVoiceSession::connect(
                    &key,
                    "You are a helpful coding assistant. Be concise and technical.",
                ).await?;
                // Simple: capture 5s chunks, send to API, print transcripts
                let dur = Duration::from_secs(duration);
                loop {
                    let backend = WhisperBackend::OpenAI {
                        api_key: key.clone(),
                        base_url: "https://api.openai.com/v1".to_string(),
                    };
                    let wav = harness_voice::record_and_transcribe(dur, &backend).await;
                    match wav {
                        Ok(t) if !t.is_empty() => {
                            eprintln!("You: {t}");
                            if t.to_lowercase().contains("goodbye") || t.to_lowercase().contains("quit") {
                                break;
                            }
                        }
                        _ => {}
                    }
                    if let Ok(ev) = session.event_rx.try_recv() {
                        match ev {
                            harness_voice::RealtimeEvent::TurnComplete(text) => eprintln!("AI: {text}"),
                            harness_voice::RealtimeEvent::Error(e) => { eprintln!("Error: {e}"); break; }
                            _ => {}
                        }
                    }
                }
                return Ok(());
            }

            let backend = WhisperBackend::detect(openai_key.as_deref());
            if !harness_voice::voice_available() && matches!(backend, WhisperBackend::Local { .. }) {
                eprintln!("Warning: no local audio recorder found. Install sox: brew install sox");
            }

            eprintln!("Recording for {duration}s… (speak now)");
            let transcript = record_and_transcribe(Duration::from_secs(duration), &backend).await?;
            println!("{transcript}");

            if send && !transcript.is_empty() {
                agent::run_once(
                    &provider, &session_store,
                    memory_store.as_ref(), embed_model.as_deref(),
                    &tools, &model,
                    cfg.agent.system_prompt.as_deref(),
                    &transcript, cli.resume.as_deref(),
                ).await?;
            }
            return Ok(());
        }

        Some(Commands::Swarm { action }) => {
            match action {
                SwarmAction::List => swarm::print_status()?,
                SwarmAction::Status { id } => {
                    match swarm::get_task(&id)? {
                        Some(t) => println!("{} [{}] {}", t.id, t.status.as_str(), t.prompt),
                        None => println!("Task {id} not found."),
                    }
                }
                SwarmAction::Result { id } => {
                    match swarm::get_task(&id)? {
                        Some(t) => println!("{}", t.result.as_deref().unwrap_or("(no result)")),
                        None => println!("Task {id} not found."),
                    }
                }
            }
            return Ok(());
        }

        Some(Commands::Trace { id }) => {
            match id {
                Some(trace_id) => observability::export_trace(&trace_id)?,
                None => {
                    let spans = observability::load_last_trace()?;
                    if spans.is_empty() {
                        println!("No traces found. Enable [observability] in config.");
                    } else {
                        println!("Trace {} — {} spans:", spans[0].trace_id, spans.len());
                        for s in &spans {
                            println!("  {:<40} {}ms", s.name, s.duration_ms);
                        }
                    }
                }
            }
            return Ok(());
        }

        Some(Commands::Doctor) => {
            handle_doctor_command(&cfg).await;
            return Ok(());
        }

        Some(Commands::Completions { shell }) => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            generate(shell, &mut cmd, bin_name, &mut std::io::stdout());
            return Ok(());
        }

        Some(Commands::Models { set }) => {
            handle_models_command(set, &cfg).await?;
            return Ok(());
        }

        Some(Commands::Sync { action }) => {
            match action {
                SyncAction::Init { git_url } => sync::init(&git_url).await?,
                SyncAction::Push => sync::push().await?,
                SyncAction::Pull => sync::pull().await?,
                SyncAction::Status => sync::status().await?,
                SyncAction::Auth => {
                    println!("Sync passphrase is stored in the system keychain under 'harness-sync'.");
                    println!("To transfer to another machine, run: harness sync init <git-url>");
                    println!("Then on the new machine, run: harness sync pull");
                    println!("The passphrase will be regenerated and stored on the new machine.");
                }
            }
            return Ok(());
        }

        Some(Commands::Cost { action }) => {
            use cost_db::{CostDb, days_ago, format_usd};
            let db = CostDb::open().context("opening cost.db")?;
            match action {
                CostAction::Today => {
                    let usd = db.total_usd_since(days_ago(1))?;
                    println!("Today: {}", format_usd(usd));
                }
                CostAction::Week => {
                    let usd = db.total_usd_since(days_ago(7))?;
                    println!("Past 7 days: {}", format_usd(usd));
                }
                CostAction::Month => {
                    let usd = db.total_usd_since(days_ago(30))?;
                    println!("Past 30 days: {}", format_usd(usd));
                }
                CostAction::All => {
                    let usd = db.total_usd_since(0)?;
                    println!("All time: {}", format_usd(usd));
                }
                CostAction::ByModel => {
                    let rows = db.by_model_since(0)?;
                    if rows.is_empty() {
                        println!("No usage data yet.");
                    } else {
                        println!("{:<35} {}", "Model", "Cost");
                        println!("{}", "-".repeat(45));
                        for (model, usd) in rows {
                            println!("{:<35} {}", model, format_usd(usd));
                        }
                    }
                }
                CostAction::ByProject => {
                    let rows = db.by_project_since(0)?;
                    if rows.is_empty() {
                        println!("No usage data yet.");
                    } else {
                        println!("{:<35} {}", "Project", "Cost");
                        println!("{}", "-".repeat(45));
                        for (project, usd) in rows {
                            let display = if project.is_empty() { "(unnamed)".to_string() } else { project };
                            println!("{:<35} {}", display, format_usd(usd));
                        }
                    }
                }
                CostAction::Watch => {
                    println!("Watching cost.db (Ctrl+C to stop)…\n");
                    loop {
                        let rows = db.recent(5)?;
                        let today = db.total_usd_since(days_ago(1))?;
                        let month = db.total_usd_since(days_ago(30))?;
                        print!("\x1B[2J\x1B[H"); // clear screen
                        println!("  Today: {}  |  30 days: {}", format_usd(today), format_usd(month));
                        println!("\nRecent turns:");
                        for r in &rows {
                            println!(
                                "  {} │ {} │ ↑{} ↓{} │ {}",
                                r.model, &r.session_id[..8.min(r.session_id.len())],
                                r.in_tok, r.out_tok, format_usd(r.usd)
                            );
                        }
                        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    }
                }
            }
            return Ok(());
        }

        Some(Commands::Memorize { topic, fact }) => {
            match memory_project::remember(&topic, &fact) {
                Ok(path) => println!("Remembered under '{}': {}", topic, path.display()),
                Err(e) => eprintln!("Error saving memory: {e}"),
            }
            return Ok(());
        }

        Some(Commands::Forget { topic }) => {
            match memory_project::forget(&topic) {
                Ok(true) => println!("Forgot topic '{topic}'"),
                Ok(false) => println!("No memory for topic '{topic}'"),
                Err(e) => eprintln!("Error: {e}"),
            }
            return Ok(());
        }

        Some(Commands::Memories) => {
            let topics = memory_project::list_topics();
            if topics.is_empty() {
                println!("No project memories. Use: harness memorize <topic> <fact>");
            } else {
                println!("{} project memory topic(s):", topics.len());
                for t in &topics {
                    println!("  • {t}");
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
    registry.register(GhTool);
    registry.register(TestRunnerTool);
    registry.register(SpawnAgentTool::new(runner));

    if browser_enabled {
        registry.register(BrowserTool::new(browser_url));
        tracing::info!(url = %browser_url, "browser tool enabled");
    }

    // Computer use: gated, only enable if explicitly configured
    if cfg.computer_use.is_enabled() {
        let model_lower = model.to_lowercase();
        if model_lower.contains("claude-opus-4-7") || model_lower.contains("claude-opus-4") || model_lower.contains("claude-sonnet-4") {
            registry.register(ComputerUseTool);
            tracing::warn!("⚠️  COMPUTER USE ENABLED — agent can control mouse/keyboard");
        } else {
            tracing::warn!("computer_use enabled in config but model {} does not support it (requires Claude 4.7+)", model);
        }
    }

    // Lazy LSP: only spawn if a supported project type is detected in the cwd.
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let has_supported_project = cwd.join("Cargo.toml").exists()
        || cwd.join("tsconfig.json").exists()
        || cwd.join("package.json").exists()
        || cwd.join("pyproject.toml").exists()
        || cwd.join("setup.py").exists()
        || cwd.join("go.mod").exists();

    if has_supported_project {
        if let Some(lsp) = detect_and_spawn(&cwd).await {
            registry.register(FindDefinitionTool { client: lsp.clone() });
            registry.register(FindReferencesTool { client: lsp.clone() });
            registry.register(RenameSymbolTool { client: lsp.clone() });
            registry.register(DiagnosticsTool { client: lsp });
        }
    }

    // Load MCP tools.
    if let Some(mcp_path) = harness_mcp::find_config() {
        if let Err(e) = harness_mcp::load_mcp_tools(&mcp_path, &mut registry).await {
            tracing::warn!("MCP load failed: {e}");
        }
    }
    if let Some(mcp_path) = &cfg.mcp.config_path {
        if mcp_path.exists() {
            if let Err(e) = harness_mcp::load_mcp_tools(mcp_path, &mut registry).await {
                tracing::warn!("MCP config load failed: {e}");
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

/// Handle `harness models [--set provider:model]`.
async fn handle_models_command(set: Option<String>, cfg: &config::Config) -> Result<()> {
    use harness_provider_router::ProviderEntry;
    use std::collections::HashMap;

    // Model catalogue: provider → [(model, description)]
    let catalogue: &[(&str, &[(&str, &str)])] = &[
        ("anthropic", &[
            ("claude-opus-4-7",           "$5/$25 · 1M ctx · adaptive thinking"),
            ("claude-sonnet-4-6",         "$3/$15 · 1M ctx · default ★"),
            ("claude-haiku-4-5",          "$1/$5  · fast / cheap"),
        ]),
        ("openai", &[
            ("gpt-5.5",                   "$5/$30  · 1M ctx"),
            ("gpt-5.4",                   "$2.50/$15"),
            ("gpt-5.4-mini",              "$0.75/$4.50 · fast"),
            ("gpt-5.4-nano",              "$0.20/$1.25 · ultra-cheap"),
            ("o4-mini",                   "$1.10/$4.40 · reasoning"),
        ]),
        ("xai", &[
            ("grok-4.20-0309-reasoning",  "$2/$6   · 2M ctx · reasoning ★"),
            ("grok-4-1-fast-reasoning",   "$0.20/$0.50 · fast"),
        ]),
        ("ollama", &[
            ("qwen3-coder:30b",           "local · 256K ctx · agentic ★"),
            ("qwen2.5-coder:32b",         "local · 92.7% HumanEval"),
            ("nomic-embed-text",          "local · embed"),
        ]),
    ];

    if let Some(ref model_spec) = set {
        // Use toml_edit for clean, idempotent TOML manipulation
        let local_cfg = std::path::PathBuf::from(".harness").join("config.toml");
        let _ = std::fs::create_dir_all(".harness");
        let text = if local_cfg.exists() {
            std::fs::read_to_string(&local_cfg).unwrap_or_default()
        } else {
            String::new()
        };

        let (provider_part, model_part) = if model_spec.contains(':') {
            let mut parts = model_spec.splitn(2, ':');
            (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string())
        } else {
            (String::new(), model_spec.clone())
        };

        let mut doc: toml_edit::DocumentMut = text.parse().unwrap_or_default();

        // Set [provider].model
        if !doc.contains_key("provider") {
            doc["provider"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        doc["provider"]["model"] = toml_edit::value(model_part.as_str());

        if !provider_part.is_empty() {
            // Also set [router].default to the provider name
            if !doc.contains_key("router") {
                doc["router"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            doc["router"]["default"] = toml_edit::value(provider_part.as_str());
        }

        std::fs::write(&local_cfg, doc.to_string())?;
        println!("✓ Default model set to '{model_spec}' in {}", local_cfg.display());
        return Ok(());
    }

    // List all providers + models
    println!("Available models (April 2026):");
    println!();
    for (provider, models) in catalogue {
        let env_key = match *provider {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai"    => "OPENAI_API_KEY",
            "xai"       => "XAI_API_KEY",
            _           => "",
        };
        let available = if env_key.is_empty() {
            "local".to_string()
        } else if std::env::var(env_key).map(|k| !k.is_empty()).unwrap_or(false) {
            "✓ key set".to_string()
        } else {
            format!("✗ {} not set", env_key)
        };
        println!("  {provider} ({available})");
        for (model, desc) in *models {
            let current = cfg.provider.model.as_deref() == Some(model);
            let marker = if current { " ◀ current" } else { "" };
            println!("    {:42} {desc}{marker}", model);
        }
        println!();
    }

    let current = cfg.provider.model.as_deref().unwrap_or("claude-sonnet-4-6");
    println!("Current default: {current}");
    println!();
    println!("To switch: harness models --set <provider:model>");
    println!("Example:   harness models --set anthropic:claude-opus-4-7");

    let _ = cfg;
    let _ = HashMap::<String, ProviderEntry>::new();
    Ok(())
}

async fn handle_doctor_command(cfg: &config::Config) {
    println!("harness doctor — system health check\n");

    // API keys
    let checks: &[(&str, &str, &str)] = &[
        ("ANTHROPIC_API_KEY", "Anthropic Claude 4.x", "claude-sonnet-4-6"),
        ("XAI_API_KEY",       "xAI Grok 4.x",         "grok-4.20-0309-reasoning"),
        ("OPENAI_API_KEY",    "OpenAI GPT-5.x",        "gpt-5.5"),
    ];
    println!("  API Keys:");
    let mut any_key = false;
    for (env, name, model) in checks {
        let set = std::env::var(env).map(|k| !k.is_empty()).unwrap_or(false);
        if set { any_key = true; }
        println!("  {} {} → {}", if set { "✓" } else { "✗" }, name, if set { format!("key set, will use {model}") } else { format!("set {env} to enable") });
    }
    if !any_key {
        println!("\n  ⚠ No API key found! Set ANTHROPIC_API_KEY to get started.");
        println!("  Export it in your shell: export ANTHROPIC_API_KEY=sk-ant-...");
    }

    // Ollama
    let ollama_running = tokio::process::Command::new("ollama").arg("list").output().await.map(|o| o.status.success()).unwrap_or(false);
    println!("  {} Ollama local models: {}", if ollama_running { "✓" } else { "○" }, if ollama_running { "running" } else { "not running (optional)" });

    // Tools
    println!("\n  External tools:");
    let tools: &[(&str, &str)] = &[
        ("git",   "version control"),
        ("gh",    "GitHub CLI (PR/issues)"),
        ("rg",    "ripgrep code search"),
        ("cargo", "Rust builds"),
        ("node",  "Node.js (TypeScript LSP)"),
        ("sox",   "audio recording (voice)"),
    ];
    for (tool, desc) in tools {
        let found = tokio::process::Command::new(tool).arg("--version").output().await.map(|o| o.status.success()).unwrap_or(false);
        println!("  {} {} — {}", if found { "✓" } else { "○" }, tool, desc);
    }

    // Config files
    println!("\n  Config:");
    let user_cfg = dirs::home_dir().unwrap_or_default().join(".harness/config.toml");
    let local_cfg = std::path::PathBuf::from(".harness/config.toml");
    println!("  {} ~/.harness/config.toml", if user_cfg.exists() { "✓" } else { "○ (optional)" });
    println!("  {} .harness/config.toml", if local_cfg.exists() { "✓" } else { "○ (optional — run harness init --project to create)" });
    println!("  Current model: {}", cfg.provider.model.as_deref().unwrap_or("(auto)"));

    // Memory + cost DB
    let mem_path = dirs::home_dir().unwrap_or_default().join(".harness/sessions.db");
    let cost_path = dirs::home_dir().unwrap_or_default().join(".harness/cost.db");
    println!("\n  Data:");
    println!("  {} sessions DB: {}", if mem_path.exists() { "✓" } else { "○" }, mem_path.display());
    println!("  {} cost DB: {}", if cost_path.exists() { "✓" } else { "○" }, cost_path.display());

    // Daemon
    let sock = dirs::home_dir().unwrap_or_default().join(".harness/daemon.sock");
    println!("\n  Daemon:");
    println!("  {} daemon socket: {}", if sock.exists() { "✓ running" } else { "○ not running (optional — run harness daemon)" }, sock.display());

    println!("\nRun `harness init` to create a default config.");
    println!("Run `harness completions zsh > ~/.zfunc/_harness` to add shell completions.");
}

fn delete_session(store: &harness_memory::SessionStore, id: &str) -> Result<()> {
    if store.delete(id)? {
        println!("Deleted session: {id}");
    } else {
        println!("Session not found: {id}");
    }
    Ok(())
}
