mod agent;
mod config;
mod events;
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

    /// Config file path (default: ~/.harness/config.toml).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Model override (e.g. grok-3, grok-3-fast, grok-3-mini).
    #[arg(long, short)]
    model: Option<String>,

    /// Disable memory recall for this run.
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let filter = if cli.verbose {
        EnvFilter::new("harness=debug,harness_provider_xai=debug")
    } else {
        EnvFilter::new("harness=info")
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
        cfg.session
            .db_path
            .clone()
            .unwrap_or_else(harness_memory::SessionStore::default_path),
    )?;

    // Memory store (same DB file, different table)
    let memory_db = cfg
        .memory
        .db_path
        .clone()
        .or_else(|| cfg.session.db_path.clone())
        .unwrap_or_else(harness_memory::SessionStore::default_path);

    let memory_store = if !cli.no_memory && cfg.memory.enabled.unwrap_or(true) {
        Some(harness_memory::MemoryStore::open(memory_db)?)
    } else {
        None
    };

    let embed_model = cfg
        .memory
        .embed_model
        .clone()
        .unwrap_or_else(|| "grok-3-embed-english".into());
    let embed_model = if memory_store.is_some() { Some(embed_model) } else { None };

    let tools = build_tools(provider.clone(), model.clone());

    match cli.command {
        Some(Commands::Sessions) => {
            list_sessions(&session_store)?;
        }
        Some(Commands::Run { prompt }) => {
            agent::run_once(
                &provider,
                &session_store,
                memory_store.as_ref(),
                embed_model.as_deref(),
                &tools,
                &model,
                cfg.agent.system_prompt.as_deref(),
                &prompt,
                cli.resume.as_deref(),
            )
            .await?;
        }
        None => {
            if let Some(prompt) = cli.prompt {
                agent::run_once(
                    &provider,
                    &session_store,
                    memory_store.as_ref(),
                    embed_model.as_deref(),
                    &tools,
                    &model,
                    cfg.agent.system_prompt.as_deref(),
                    &prompt,
                    cli.resume.as_deref(),
                )
                .await?;
            } else {
                tui::run(provider, session_store, memory_store, embed_model, tools, model, cfg)
                    .await?;
            }
        }
    }

    Ok(())
}

/// Build the default tool executor, including SpawnAgentTool which closes over a
/// fresh provider so sub-agents run independently.
fn build_tools(provider: XaiProvider, model: String) -> ToolExecutor {
    // Base tools available to both main agent and sub-agents
    let base_registry = || {
        let mut r = ToolRegistry::new();
        r.register(ReadFileTool);
        r.register(WriteFileTool);
        r.register(ListDirTool);
        r.register(ShellTool);
        r.register(SearchCodeTool);
        r
    };

    // Sub-agent runner: creates a fresh session and drives it with base tools only
    let sub_provider = provider.clone();
    let sub_model = model.clone();
    let runner: harness_tools::tools::agent::SubAgentRunner = Arc::new(move |task: String| {
        let p = sub_provider.clone();
        let m = sub_model.clone();
        let base_tools = ToolExecutor::new(base_registry());
        Box::pin(async move {
            use harness_memory::Session;
            use harness_provider_core::Message;

            let mut session = Session::new(&m);
            session.push(Message::user(&task));
            agent::drive_agent(&p, &base_tools, None, None, &mut session, agent::DEFAULT_SYSTEM, None).await?;

            // Return the last assistant message
            let reply = session
                .messages
                .iter()
                .rev()
                .find(|m| matches!(m.role, harness_provider_core::Role::Assistant))
                .map(|m| m.content.as_str().to_string())
                .unwrap_or_else(|| "(no response)".into());
            Ok(reply)
        })
    });

    let mut registry = base_registry();
    registry.register(SpawnAgentTool::new(runner));
    ToolExecutor::new(registry)
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
