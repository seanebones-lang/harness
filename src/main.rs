mod agent;
mod ambient;
mod config;
mod events;
mod highlight;
mod server;
mod tui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use harness_browser::BrowserTool;
use harness_provider_xai::{XaiConfig, XaiProvider};
use harness_tools::{ToolExecutor, ToolRegistry};
use harness_tools::tools::{
    ListDirTool, PatchFileTool, ReadFileTool, RebuildSelfTool, ReloadSelfTool,
    SearchCodeTool, ShellTool, SpawnAgentTool, WriteFileTool,
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
}

#[tokio::main]
async fn main() -> Result<()> {
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

    let provider = XaiProvider::new(xai_cfg)?;

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
    let ambient_shutdown = if let (Some(mem), Some(em)) = (&memory_store, &embed_model) {
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
            agent::run_once(
                &provider, &session_store,
                memory_store.as_ref(), embed_model.as_deref(),
                &tools, &model,
                cfg.agent.system_prompt.as_deref(),
                &prompt, cli.resume.as_deref(),
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
                agent::run_once(
                    &provider, &session_store,
                    memory_store.as_ref(), embed_model.as_deref(),
                    &tools, &model,
                    cfg.agent.system_prompt.as_deref(),
                    &prompt, cli.resume.as_deref(),
                ).await?;
            } else {
                let result = tui::run(
                    provider,
                    session_store,
                    memory_store,
                    embed_model,
                    tools,
                    model,
                    cfg,
                    cli.resume.as_deref(),
                    ambient_shutdown.clone(),
                )
                .await;
                if let Some(tx) = &ambient_shutdown {
                    let _ = tx.send(());
                }
                result?;
            }
        }
    }

    if let Some(tx) = &ambient_shutdown {
        let _ = tx.send(());
    }

    Ok(())
}

/// Build the full tool executor: base tools + SpawnAgentTool + MCP tools.
async fn build_tools(provider: XaiProvider, model: String, cfg: &config::Config, browser_enabled: bool, browser_url: &str) -> ToolExecutor {
    let base_registry = || {
        let mut r = ToolRegistry::new();
        r.register(ReadFileTool);
        r.register(WriteFileTool);
        r.register(PatchFileTool);
        r.register(ListDirTool);
        r.register(ShellTool);
        r.register(SearchCodeTool);
        r
    };

    // Sub-agent runner: runs a prompt through a fresh session with base tools only.
    let sub_provider = provider.clone();
    let sub_model = model.clone();
    let runner: harness_tools::tools::agent::SubAgentRunner = Arc::new(move |task: String| {
        let p = sub_provider.clone();
        let m = sub_model.clone();
        let tools = ToolExecutor::new(base_registry());
        Box::pin(async move {
            use harness_memory::Session;
            use harness_provider_core::Message;
            let mut session = Session::new(&m);
            session.push(Message::user(&task));
            agent::drive_agent(&p, &tools, None, None, &mut session, agent::DEFAULT_SYSTEM, None).await?;
            let reply = session.messages.iter().rev()
                .find(|m| matches!(m.role, harness_provider_core::Role::Assistant))
                .map(|m| m.content.as_str().to_string())
                .unwrap_or_else(|| "(no response)".into());
            Ok(reply)
        })
    });

    let mut registry = base_registry();
    registry.register(SpawnAgentTool::new(runner));

    if browser_enabled {
        registry.register(BrowserTool::new(browser_url));
        tracing::info!(url = %browser_url, "browser tool enabled");
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

    ToolExecutor::new(registry)
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
    provider: XaiProvider,
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
    registry.register(ShellTool);
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

fn delete_session(store: &harness_memory::SessionStore, id: &str) -> Result<()> {
    if store.delete(id)? {
        println!("Deleted session: {id}");
    } else {
        println!("Session not found: {id}");
    }
    Ok(())
}
