mod agent;
mod config;
mod tui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser)]
#[command(name = "harness", about = "Rust coding agent harness powered by Grok (xAI)", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Prompt to run in non-interactive mode.
    prompt: Option<String>,

    /// Resume a session by id or name prefix.
    #[arg(long, short)]
    resume: Option<String>,

    /// Config file path (default: ~/.harness/config.toml).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Model override (e.g. grok-3, grok-3-fast, grok-3-mini).
    #[arg(long, short)]
    model: Option<String>,

    /// Verbose logging.
    #[arg(long, short)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// List recent sessions.
    Sessions,
    /// Run a single prompt and exit (same as passing prompt as positional arg).
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

    let model = cli.model
        .or_else(|| cfg.provider.model.clone())
        .unwrap_or_else(|| "grok-3-fast".into());

    let xai_cfg = harness_provider_xai::XaiConfig::new(api_key)
        .with_model(&model)
        .with_max_tokens(cfg.provider.max_tokens.unwrap_or(8192))
        .with_temperature(cfg.provider.temperature.unwrap_or(0.7));

    let provider = harness_provider_xai::XaiProvider::new(xai_cfg)?;

    let store = harness_memory::SessionStore::open(
        cfg.session.db_path
            .clone()
            .unwrap_or_else(harness_memory::SessionStore::default_path),
    )?;

    let tools = build_tools();

    match cli.command {
        Some(Commands::Sessions) => {
            list_sessions(&store)?;
        }
        Some(Commands::Run { prompt }) => {
            agent::run_once(provider, store, tools, &model, cfg.agent.system_prompt.as_deref(), &prompt, cli.resume.as_deref()).await?;
        }
        None => {
            if let Some(prompt) = cli.prompt {
                agent::run_once(provider, store, tools, &model, cfg.agent.system_prompt.as_deref(), &prompt, cli.resume.as_deref()).await?;
            } else {
                tui::run(provider, store, tools, model, cfg).await?;
            }
        }
    }

    Ok(())
}

fn build_tools() -> harness_tools::ToolExecutor {
    use harness_tools::{ToolRegistry, ToolExecutor};
    use harness_tools::tools::*;

    let mut registry = ToolRegistry::new();
    registry.register(ReadFileTool);
    registry.register(WriteFileTool);
    registry.register(ListDirTool);
    registry.register(ShellTool);
    registry.register(SearchCodeTool);
    ToolExecutor::new(registry)
}

fn list_sessions(store: &harness_memory::SessionStore) -> Result<()> {
    let sessions = store.list(20)?;
    if sessions.is_empty() {
        println!("No sessions yet.");
        return Ok(());
    }
    println!("{:<10} {:<20} {}", "ID", "NAME", "UPDATED");
    for (id, name, updated) in sessions {
        println!("{:<10} {:<20} {}", &id[..8], name.unwrap_or_default(), updated);
    }
    Ok(())
}
