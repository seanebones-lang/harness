//! `harness setup` — interactive provider and API key configuration.

use anyhow::{Context, Result};

use crate::config::{self, Config};

/// Returns `true` until an explicit primary provider and model are saved.
pub fn needs_setup(cfg: &Config) -> bool {
    let Some(primary) = cfg.router.default.as_deref() else {
        return true;
    };
    let route = std::iter::once(primary)
        .chain(
            cfg.router
                .fallback
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(String::as_str),
        )
        .collect::<Vec<_>>();
    let mut seen = std::collections::HashSet::new();
    route.iter().any(|name| {
        name.is_empty()
            || !seen.insert(*name)
            || cfg
                .providers
                .get(*name)
                .and_then(|entry| entry.model.as_deref())
                .unwrap_or("")
                .is_empty()
    })
}

/// Interactive first-run wizard. Updates config on disk when the user completes setup.
pub fn run_setup_interactive(cfg: &Config, force: bool) -> Result<()> {
    use std::io::Write;

    if !force && !needs_setup(cfg) {
        println!("A provider route is already configured.");
        println!(
            "Run `harness route show` to inspect it or `harness setup --force` to replace it."
        );
        return Ok(());
    }

    if !force {
        eprintln!("harness: No explicit provider route is configured.");
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
    println!("Available provider adapters (alphabetical; no provider is preferred):");
    for preset in harness_provider_router::PROVIDER_PRESETS {
        let status = if preset.local {
            "local"
        } else if preset.api_key_envs.iter().any(|name| {
            std::env::var(name)
                .map(|value| !value.is_empty())
                .unwrap_or(false)
        }) {
            "credentials detected"
        } else {
            "credentials not detected"
        };
        println!("  {:<12} {status}", preset.name);
    }
    println!();
    println!("Enter provider names in the exact order Harness should try them.");
    println!("The first is primary; the rest are fallbacks. One provider is valid.");
    print!("Route (comma-separated): ");
    std::io::stdout().flush().ok();

    let mut route_input = String::new();
    std::io::stdin().read_line(&mut route_input)?;
    let names: Vec<String> = route_input
        .split(',')
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty() {
        anyhow::bail!("At least one provider is required.");
    }

    let mut specs = Vec::with_capacity(names.len());
    for name in names {
        if harness_provider_router::provider_preset(&name).is_none()
            && !cfg.providers.contains_key(&name)
        {
            anyhow::bail!(
                "Unknown provider '{name}'. Add custom endpoints with `harness route custom`."
            );
        }
        print!("Model id for {name}: ");
        std::io::stdout().flush().ok();
        let mut model = String::new();
        std::io::stdin().read_line(&mut model)?;
        let model = model.trim();
        if model.is_empty() {
            anyhow::bail!("A model id is required for {name}.");
        }
        specs.push(format!("{name}:{model}"));
    }

    let config_path = config::active_config_toml_path();
    let mut new_cfg = cfg.clone();
    super::route::set_route(&mut new_cfg, &specs)?;

    config::write_config_toml(&config_path, &new_cfg)
        .with_context(|| format!("failed to write config to {}", config_path.display()))?;

    println!(
        "\n✓ Exact provider route saved to {}.",
        config_path.display()
    );
    println!("API keys remain in environment variables; Harness did not store a secret.");
    println!("Run `harness route show` to inspect or change the route.");
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

/// Agent-interactive commands that may prompt for API keys on first run.
pub fn command_needs_agent_runtime(cli: &crate::cli::Cli) -> bool {
    use crate::cli::Commands::*;

    if cli.prompt.is_some() {
        return true;
    }

    match &cli.command {
        None => true,
        Some(Run { .. }) => true,
        Some(Build { prepare_only, .. }) => !*prepare_only,
        Some(Serve { .. }) => true,
        Some(SelfDev { .. }) => true,
        Some(Daemon) => true,
        Some(Connect { .. }) => false,
        Some(Pr { comment, .. }) => comment.is_none(),
        Some(Voice { send, .. }) => *send,
        Some(Swarm { action }) => matches!(action, crate::cli::SwarmAction::Run { .. }),
        Some(RunBg { .. }) => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use crate::config::Config;
    use clap::Parser;

    #[test]
    fn needs_setup_true_when_key_exists_without_saved_route() {
        let mut cfg = Config::default();
        cfg.provider.api_key = Some("xai-test-key".into());
        assert!(needs_setup(&cfg));
    }

    #[test]
    fn needs_setup_false_when_route_and_model_are_saved() {
        let mut cfg = Config::default();
        cfg.router.default = Some("ollama".into());
        cfg.providers.entry("ollama".into()).or_default().model = Some("qwen".into());
        assert!(!needs_setup(&cfg));
    }

    #[test]
    fn needs_setup_true_when_route_contains_duplicate_provider() {
        let mut cfg = Config::default();
        cfg.router.default = Some("openai".into());
        cfg.router.fallback = Some(vec!["openai".into()]);
        cfg.providers.entry("openai".into()).or_default().model = Some("model".into());
        assert!(needs_setup(&cfg));
    }

    #[test]
    fn needs_setup_true_when_fallback_model_is_missing() {
        let mut cfg = Config::default();
        cfg.router.default = Some("openai".into());
        cfg.router.fallback = Some(vec!["ollama".into()]);
        cfg.providers.entry("openai".into()).or_default().model = Some("model".into());
        cfg.providers.entry("ollama".into()).or_default();
        assert!(needs_setup(&cfg));
    }

    #[test]
    fn command_needs_agent_runtime_matrix() {
        let tui = Cli::try_parse_from(["harness"]).expect("tui");
        assert!(command_needs_agent_runtime(&tui));

        let oneshot = Cli::try_parse_from(["harness", "hello world"]).expect("prompt");
        assert!(command_needs_agent_runtime(&oneshot));

        let run = Cli::try_parse_from(["harness", "run", "do it"]).expect("run");
        assert!(command_needs_agent_runtime(&run));

        let sessions = Cli::try_parse_from(["harness", "sessions"]).expect("sessions");
        assert!(!command_needs_agent_runtime(&sessions));

        let swarm_list = Cli::try_parse_from(["harness", "swarm", "list"]).expect("swarm list");
        assert!(!command_needs_agent_runtime(&swarm_list));

        let swarm_run =
            Cli::try_parse_from(["harness", "swarm", "run", "task"]).expect("swarm run");
        assert!(command_needs_agent_runtime(&swarm_run));

        let bench = Cli::try_parse_from(["harness", "bench"]).expect("bench");
        assert!(!command_needs_agent_runtime(&bench));
    }
}

#[cfg(test)]
mod tests_extended {
    use super::*;
    use crate::cli::Cli;
    use crate::config::Config;
    use clap::Parser;

    #[test]
    fn needs_setup_true_when_no_route_no_key() {
        let cfg = Config::default();
        assert!(needs_setup(&cfg));
    }

    #[test]
    fn needs_setup_true_when_primary_missing_model() {
        let mut cfg = Config::default();
        cfg.router.default = Some("openai".into());
        assert!(needs_setup(&cfg));
    }

    #[test]
    fn needs_setup_true_when_primary_empty_string() {
        let mut cfg = Config::default();
        cfg.router.default = Some("".into());
        assert!(needs_setup(&cfg));
    }

    #[test]
    fn needs_setup_true_when_fallback_empty_string() {
        let mut cfg = Config::default();
        cfg.router.default = Some("openai".into());
        cfg.providers.entry("openai".into()).or_default().model = Some("model".into());
        cfg.router.fallback = Some(vec!["".into()]);
        assert!(needs_setup(&cfg));
    }

    #[test]
    fn command_needs_agent_runtime_connect_false() {
        let cli =
            Cli::try_parse_from(["harness", "connect", "--url", "http://localhost:8787", "hi"])
                .expect("connect");
        assert!(!command_needs_agent_runtime(&cli));
    }

    #[test]
    fn command_needs_agent_runtime_pr_with_comment_false() {
        let cli =
            Cli::try_parse_from(["harness", "pr", "123", "--comment", "LGTM"]).expect("pr comment");
        assert!(!command_needs_agent_runtime(&cli));
    }

    #[test]
    fn command_needs_agent_runtime_pr_without_comment_true() {
        let cli = Cli::try_parse_from(["harness", "pr", "123"]).expect("pr");
        assert!(command_needs_agent_runtime(&cli));
    }

    #[test]
    fn command_needs_agent_runtime_voice_send_true() {
        let cli = Cli::try_parse_from(["harness", "voice", "--duration", "5", "--send"])
            .expect("voice send");
        assert!(command_needs_agent_runtime(&cli));
    }

    #[test]
    fn command_needs_agent_runtime_voice_realtime_false() {
        let cli = Cli::try_parse_from(["harness", "voice", "--realtime"]).expect("voice realtime");
        assert!(!command_needs_agent_runtime(&cli));
    }

    #[test]
    fn command_needs_agent_runtime_voice_record_only_false() {
        let cli =
            Cli::try_parse_from(["harness", "voice", "--duration", "5"]).expect("voice record");
        assert!(!command_needs_agent_runtime(&cli));
    }

    #[test]
    fn command_needs_agent_runtime_runbg_false() {
        let cli = Cli::try_parse_from(["harness", "run-bg", "do it"]).expect("run-bg");
        assert!(!command_needs_agent_runtime(&cli));
    }

    #[test]
    fn command_needs_agent_runtime_selfdev_true() {
        let cli = Cli::try_parse_from(["harness", "self-dev", "--model", "claude-sonnet-4-6"])
            .expect("self-dev");
        assert!(command_needs_agent_runtime(&cli));
    }

    #[test]
    fn command_needs_agent_runtime_daemon_true() {
        let cli = Cli::try_parse_from(["harness", "daemon"]).expect("daemon");
        assert!(command_needs_agent_runtime(&cli));
    }

    #[test]
    fn command_needs_agent_runtime_serve_true() {
        let cli =
            Cli::try_parse_from(["harness", "serve", "--addr", "127.0.0.1:8787"]).expect("serve");
        assert!(command_needs_agent_runtime(&cli));
    }
}
