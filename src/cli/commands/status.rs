//! `harness status` — config + MCP + recent sessions summary.

use crate::config::Config;
use anyhow::Result;
use harness_memory::SessionStore;

pub fn run_status(cfg: &Config, model: &str, store: &SessionStore) -> Result<()> {
    println!("harness status\n");

    let has_config_key = cfg.provider.api_key.is_some();
    let has_xai = std::env::var("XAI_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let has_openai = std::env::var("OPENAI_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let has_ollama = cfg.providers.contains_key("ollama");
    let has_provider_keys = cfg
        .providers
        .values()
        .any(|e| e.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false));

    let configured = has_config_key
        || has_xai
        || has_anthropic
        || has_openai
        || has_ollama
        || has_provider_keys
        || harness_provider_mlx::mlx_runtime_available();

    if configured {
        let key_source = if has_config_key {
            "config file"
        } else if has_xai {
            "XAI_API_KEY env var"
        } else if has_anthropic {
            "ANTHROPIC_API_KEY env var"
        } else if has_openai {
            "OPENAI_API_KEY env var"
        } else if has_ollama {
            "Ollama (local)"
        } else {
            "providers config / MLX"
        };
        println!("  API key : configured ({key_source})");
    } else {
        println!("  API key : not configured (run `harness setup`)");
    }
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
