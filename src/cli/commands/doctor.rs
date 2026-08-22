//! `harness doctor` — environment and dependency smoke check.

use crate::config::Config;
use crate::provider_build;
use std::time::Duration;

pub async fn handle_doctor_command(cfg: &Config) {
    println!("harness doctor — system health check\n");

    let resolved = provider_build::resolved_model(cfg, None);
    match provider_build::build_arc_provider(cfg, None) {
        Ok(p) => {
            println!(
                "  Resolved provider: {} · model {}",
                harness_provider_core::Provider::name(p.as_ref()),
                harness_provider_core::Provider::model(p.as_ref())
            );
        }
        Err(e) => {
            println!("  Resolved model (config): {resolved}");
            println!("  Provider build skipped: {e}");
        }
    }
    println!();

    // Doctor must honor config.toml keys the same way runtime/status do.
    // Env vars still win for messaging; config/[providers.*] count as configured.
    let config_top_key = cfg
        .provider
        .api_key
        .as_ref()
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let provider_entry_key = |name: &str| -> bool {
        cfg.providers
            .get(name)
            .and_then(|e| e.api_key.as_ref())
            .map(|k| !k.is_empty())
            .unwrap_or(false)
    };
    let env_key =
        |env: &str| -> bool { std::env::var(env).map(|k| !k.is_empty()).unwrap_or(false) };

    let checks: &[(&str, &str, &str, &str)] = &[
        (
            "ANTHROPIC_API_KEY",
            "anthropic",
            "Anthropic Claude 4.x",
            "claude-sonnet-4-6",
        ),
        ("XAI_API_KEY", "xai", "xAI Grok 4.x", "grok-4.5"),
        ("OPENAI_API_KEY", "openai", "OpenAI GPT-5.x", "gpt-5.5"),
        (
            "MISTRAL_API_KEY",
            "mistral",
            "Mistral (OpenAI-compatible)",
            "mistral-large-latest",
        ),
    ];
    println!("  API Keys:");
    let mut any_key = false;
    for (env, provider_name, name, model) in checks {
        let from_env = env_key(env);
        let from_provider = provider_entry_key(provider_name);
        // Top-level [provider].api_key is historically used for the active/default provider.
        let from_top = config_top_key
            && matches!(
                provider_name,
                &"xai" | &"anthropic" | &"openai" | &"mistral"
            )
            && cfg
                .provider
                .model
                .as_deref()
                .map(|m| {
                    let m = m.to_ascii_lowercase();
                    match *provider_name {
                        "xai" => m.contains("grok") || m.starts_with("xai"),
                        "anthropic" => {
                            m.contains("claude")
                                || m.contains("sonnet")
                                || m.contains("opus")
                                || m.contains("haiku")
                        }
                        "openai" => {
                            m.starts_with("gpt")
                                || m.contains("o1")
                                || m.contains("o3")
                                || m.contains("o4")
                        }
                        "mistral" => {
                            m.contains("mistral")
                                || m.contains("mixtral")
                                || m.contains("codestral")
                        }
                        _ => false,
                    }
                })
                .unwrap_or(false);
        let set = from_env || from_provider || from_top;
        if set {
            any_key = true;
        }
        let source = if from_env {
            format!("env {env}")
        } else if from_provider {
            format!("config [providers.{provider_name}]")
        } else if from_top {
            "config [provider].api_key".to_string()
        } else {
            String::new()
        };
        println!(
            "  {} {} → {}",
            if set { "✓" } else { "✗" },
            name,
            if set {
                format!("key set ({source}), will use {model}")
            } else {
                format!("set {env} or config key to enable")
            }
        );
    }
    // Any non-empty key in providers map counts even if not in the shortlist above.
    if !any_key {
        any_key = config_top_key
            || cfg
                .providers
                .values()
                .any(|e| e.api_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false));
    }
    if !any_key {
        println!("\n  ⚠ No API key found! Set ANTHROPIC_API_KEY / XAI_API_KEY / OPENAI_API_KEY,");
        println!(
            "  or put api_key under [provider] / [providers.<name>] in ~/.harness/config.toml."
        );
    }

    // `doctor` is a local diagnostic, so an optional Ollama installation must not
    // hold up the entire command when its background service is unavailable.
    let ollama_running = tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new("ollama").arg("list").output(),
    )
    .await
    .ok()
    .and_then(|result| result.ok())
    .map(|output| output.status.success())
    .unwrap_or(false);
    println!(
        "  {} Ollama local models: {}",
        if ollama_running { "✓" } else { "○" },
        if ollama_running {
            "running"
        } else {
            "not running (optional)"
        }
    );

    let mlx_ok = harness_provider_mlx::mlx_runtime_available();
    println!(
        "  {} MLX LM server (OpenAI-compatible HTTP): {}",
        if mlx_ok { "✓" } else { "○" },
        if mlx_ok {
            "mlx_lm.server on PATH or :8080 accepting connections"
        } else {
            "not detected (optional — Apple Silicon: mlx_lm.server, default http://127.0.0.1:8080/v1)"
        }
    );

    println!("\n  External tools:");
    let tools: &[(&str, &str)] = &[
        ("git", "version control"),
        ("gh", "GitHub CLI (PR/issues)"),
        ("rg", "ripgrep code search"),
        ("cargo", "Rust builds"),
        ("node", "Node.js (TypeScript LSP)"),
        ("sox", "audio recording (voice)"),
    ];
    for (tool, desc) in tools {
        let found = tokio::process::Command::new(tool)
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        println!("  {} {} — {}", if found { "✓" } else { "○" }, tool, desc);
    }

    println!("\n  Config:");
    let user_cfg = dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/config.toml");
    let local_cfg = std::path::PathBuf::from(".harness/config.toml");
    println!(
        "  {} ~/.harness/config.toml",
        if user_cfg.exists() {
            "✓"
        } else {
            "○ (optional)"
        }
    );
    println!(
        "  {} .harness/config.toml",
        if local_cfg.exists() {
            "✓"
        } else {
            "○ (optional — run harness init --project to create)"
        }
    );
    println!(
        "  Current model: {}",
        cfg.provider.model.as_deref().unwrap_or("(auto)")
    );

    let mem_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/sessions.db");
    let cost_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/cost.db");
    println!("\n  Data:");
    println!(
        "  {} sessions DB: {}",
        if mem_path.exists() { "✓" } else { "○" },
        mem_path.display()
    );
    println!(
        "  {} cost DB: {}",
        if cost_path.exists() { "✓" } else { "○" },
        cost_path.display()
    );

    println!("\n  Bridges ([bridges.*] in config):");
    let b = &cfg.bridges;
    let bridge_rows: &[(&str, bool, &str)] = &[
        (
            "obsidian",
            b.obsidian.enabled,
            if b.obsidian.vault.as_deref().unwrap_or("").is_empty() {
                "enabled — vault unset (URI without vault=)"
            } else {
                "enabled"
            },
        ),
        (
            "notes",
            b.notes.enabled,
            "Apple Notes via osascript (macOS)",
        ),
        (
            "calendar",
            b.calendar.enabled,
            "Calendar via osascript (macOS)",
        ),
        (
            "github_projects",
            b.github_projects.enabled,
            if b.github_projects.enabled
                && (b.github_projects.owner.is_none() || b.github_projects.project_number.is_none())
            {
                "enabled but owner/project_number incomplete"
            } else {
                "GitHub Project V2 list"
            },
        ),
    ];
    for (name, on, detail) in bridge_rows {
        if *on {
            println!("  ✓ {name} — {detail}");
        } else {
            println!("  ○ {name} — disabled (set [bridges.{name}] enabled = true)");
        }
    }
    println!("  CLI: harness bridge obsidian|notes|calendar|github-project …");

    println!("\n  Observability:");
    let obs = &cfg.observability;
    let traces_dir = dirs::home_dir().unwrap_or_default().join(".harness/traces");
    if obs.enabled {
        println!("  ✓ [observability] enabled");
    } else {
        println!("  ○ [observability] disabled");
    }
    println!(
        "  {} local JSONL traces dir: {}",
        if traces_dir.is_dir() { "✓" } else { "○" },
        traces_dir.display()
    );
    match obs.otlp_experimental_endpoint.as_deref() {
        Some(ep) if !ep.is_empty() => {
            println!("  ✓ otlp_experimental_endpoint = {ep}");
            println!("    (experimental JSON POST …/v1/traces — see docs/OTLP_SMOKE.md)");
        }
        _ => println!("  ○ otlp_experimental_endpoint unset (optional — docs/OTLP_SMOKE.md)"),
    }
    println!("  CLI: harness trace [id] · TUI: /trace last|list");

    println!("\n  Collab:");
    let c = &cfg.collab;
    if c.enabled {
        println!(
            "  ✓ enabled — max_users={} (WS /ws/session/:id — docs/COLLAB.md)",
            c.max_users.max(1)
        );
    } else {
        println!("  ○ disabled (set [collab] enabled = true — docs/COLLAB.md)");
    }

    let sock = dirs::home_dir()
        .unwrap_or_default()
        .join(".harness/daemon.sock");
    println!("\n  Daemon:");
    println!(
        "  {} daemon socket: {}",
        if sock.exists() {
            "✓ running"
        } else {
            "○ not running (optional — run harness daemon)"
        },
        sock.display()
    );

    println!("\nRun `harness init` to create a default config.");
    println!("Run `harness completions zsh > ~/.zfunc/_harness` to add shell completions.");
}
