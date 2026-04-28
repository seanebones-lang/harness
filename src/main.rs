mod agent;
mod config;
mod events;
mod server;
mod tui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use harness_provider_xai::{XaiConfig, XaiProvider};
use harness_tools::{ToolExecutor, ToolRegistry};
use harness_tools::tools::{
    ListDirTool, ReadFileTool, SearchCodeTool, ShellTool, SpawnAgentTool, WriteFileTool,
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

    // Build tools (including MCP servers if config exists).
    let tools = build_tools(provider.clone(), model.clone(), &cfg).await;

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
                tui::run(provider, session_store, memory_store, embed_model, tools, model, cfg).await?;
            }
        }
    }

    Ok(())
}

/// Build the full tool executor: base tools + SpawnAgentTool + MCP tools.
async fn build_tools(provider: XaiProvider, model: String, cfg: &config::Config) -> ToolExecutor {
    let base_registry = || {
        let mut r = ToolRegistry::new();
        r.register(ReadFileTool);
        r.register(WriteFileTool);
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

fn list_sessions(store: &harness_memory::SessionStore) -> Result<()> {
    let sessions = store.list(20)?;
    if sessions.is_empty() {
        println!("No sessions yet.");
        return Ok(());
    }
    println!("{:<10} {:<24} {}", "ID", "NAME", "UPDATED");
    for (id, name, updated) in sessions {
        println!("{:<10} {:<24} {}", &id[..8], name.unwrap_or_default(), updated);
    }
    Ok(())
}
