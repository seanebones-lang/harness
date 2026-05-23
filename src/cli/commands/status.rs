//! `harness status` — config + MCP + recent sessions summary.

use crate::config::Config;
use anyhow::Result;
use harness_memory::SessionStore;

pub fn run_status(cfg: &Config, model: &str, store: &SessionStore, _api_key: &str) -> Result<()> {
    println!("harness status\n");

    let key_source = if cfg.provider.api_key.is_some() {
        "config file"
    } else if std::env::var("XAI_API_KEY").is_ok() {
        "XAI_API_KEY env var"
    } else if std::env::var("ANTHROPIC_API_KEY").is_ok() {
        "ANTHROPIC_API_KEY env var"
    } else if std::env::var("OPENAI_API_KEY").is_ok() {
        "OPENAI_API_KEY env var"
    } else {
        "unknown"
    };
    println!("  API key : configured ({key_source})");
    println!("  Model   : {model}");

    let cfg_path = {
        let local = std::path::PathBuf::from(".harness/config.toml");
        let global = dirs::home_dir()
            .unwrap_or_default()
            .join(".harness/config.toml");
        if local.exists() {
            format!("{} (project)", local.display())
        } else if global.exists() {
            format!("{} (global)", global.display())
        } else {
            "defaults (no config file found)".to_string()
        }
    };
    println!("  Config  : {cfg_path}");

    let mcp_path = harness_mcp::find_config();
    println!(
        "  MCP     : {}",
        mcp_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "not configured".to_string())
    );

    println!();
    println!("Recent sessions:");
    match store.list(5) {
        Ok(sessions) if !sessions.is_empty() => {
            for (id, name, updated) in sessions {
                let short = id.chars().take(8).collect::<String>();
                println!(
                    "  {} · {} · {}",
                    short,
                    name.unwrap_or_else(|| "(unnamed)".to_string()),
                    updated
                );
            }
        }
        Ok(_) => println!("  (none yet)"),
        Err(e) => println!("  (error reading sessions: {e})"),
    }

    println!();
    println!("Run `harness` to start a new session.");
    Ok(())
}
