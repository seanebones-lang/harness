//! `harness setup` — interactive provider and API key configuration.

use anyhow::{Context, Result};

use crate::config::{self, Config};

/// Returns `true` when no provider keys or explicit `[providers]` entries are configured.
pub fn needs_setup(cfg: &Config) -> bool {
    let has_xai = cfg.provider.api_key.is_some()
        || std::env::var("XAI_API_KEY")
            .map(|k| !k.is_empty())
            .unwrap_or(false);
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let has_openai = std::env::var("OPENAI_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let has_ollama = cfg.providers.contains_key("ollama");

    !has_xai
        && !has_anthropic
        && !has_openai
        && !has_ollama
        && cfg.providers.is_empty()
        && !harness_provider_mlx::mlx_runtime_available()
}

/// Interactive first-run wizard. Updates config on disk when the user completes setup.
pub fn run_setup_interactive(cfg: &Config, force: bool) -> Result<()> {
    use std::io::Write;

    if !force && !needs_setup(cfg) {
        println!("Provider keys are already configured.");
        println!("Run `harness setup --force` to reconfigure, or `harness status` to inspect.");
        return Ok(());
    }

    if !force {
        eprintln!("harness: No API keys configured.");
        eprintln!();
        eprintln!("Would you like to set one up now? (y/n)");

        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err()
            || !input.trim().to_lowercase().starts_with('y')
        {
            eprintln!("Setup skipped. Run `harness setup` when ready.");
            return Ok(());
        }
    }

    println!();
    println!("Recommended: xAI (Grok 4.3) — fast and cost-efficient.");
    println!();
    println!("Which provider?");
    println!("  [1] xAI (Grok)         ← Recommended");
    println!("  [2] Anthropic (Claude)");
    println!("  [3] OpenAI");
    println!("  [4] Ollama (local)");
    println!();
    print!("Enter choice [1-4]: ");
    std::io::stdout().flush().ok();

    let mut choice = String::new();
    std::io::stdin().read_line(&mut choice).ok();

    let provider_choice = choice.trim();
    let (provider_name, env_var) = match provider_choice {
        "1" => ("xai", "XAI_API_KEY"),
        "2" => ("anthropic", "ANTHROPIC_API_KEY"),
        "3" => ("openai", "OPENAI_API_KEY"),
        "4" => {
            println!("\n→ Make sure Ollama is running:");
            println!("   ollama run qwen3-coder:30b");
            let config_path = config::active_config_toml_path();
            let mut new_cfg = cfg.clone();
            new_cfg.providers.entry("ollama".into()).or_default().model =
                Some("qwen3-coder:30b".into());
            config::write_config_toml(&config_path, &new_cfg)
                .context("failed to write Ollama provider config")?;
            println!("\n✓ Ollama provider saved to {}", config_path.display());
            return Ok(());
        }
        _ => {
            anyhow::bail!("Invalid choice.");
        }
    };

    let default_model = match provider_name {
        "xai" => "grok-4.3",
        "anthropic" => "claude-sonnet-4-6",
        "openai" => "gpt-5.5",
        _ => "grok-4.3",
    };
    println!();
    println!("Recommended model for {provider_name}: {default_model}");
    print!("Enter model (or press Enter for default): ");
    std::io::stdout().flush().ok();

    let mut model_input = String::new();
    std::io::stdin().read_line(&mut model_input).ok();
    let chosen_model = if model_input.trim().is_empty() {
        default_model.to_string()
    } else {
        model_input.trim().to_string()
    };

    println!();
    print!("Enter your {env_var} key: ");
    std::io::stdout().flush().ok();

    let mut key = String::new();
    std::io::stdin().read_line(&mut key).ok();
    let key = key.trim().to_string();

    if key.is_empty() {
        anyhow::bail!("No key provided.");
    }

    let config_path = config::active_config_toml_path();
    let mut new_cfg = cfg.clone();

    let entry = new_cfg
        .providers
        .entry(provider_name.to_string())
        .or_default();
    entry.api_key = Some(key.clone());
    entry.model = Some(chosen_model.clone());

    new_cfg.provider.api_key = Some(key);
    new_cfg.provider.model = Some(chosen_model);

    config::write_config_toml(&config_path, &new_cfg)
        .with_context(|| format!("failed to write config to {}", config_path.display()))?;

    println!("\n✓ Key saved to {}.", config_path.display());
    println!("Run `harness` to start a session.");
    Ok(())
}

/// Prompt on first launch when keys are missing (non-blocking if user declines).
pub fn maybe_run_first_time_wizard(cfg: &Config) -> Result<()> {
    if needs_setup(cfg) {
        run_setup_interactive(cfg, false)?;
    }
    Ok(())
}

/// Non-agent commands that must not block on the interactive setup wizard.
pub fn command_skips_first_run_wizard(cmd: Option<&crate::cli::Commands>) -> bool {
    use crate::cli::Commands::*;
    matches!(
        cmd,
        Some(
            Setup { .. }
                | Update
                | Project { .. }
                | Doctor
                | Completions { .. }
                | Init { .. }
                | Sessions
                | Status
                | Export { .. }
                | Delete { .. }
        )
    )
}
