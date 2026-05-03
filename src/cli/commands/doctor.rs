//! `harness doctor` — environment and dependency smoke check.

use crate::config::Config;

pub async fn handle_doctor_command(cfg: &Config) {
    println!("harness doctor — system health check\n");

    let checks: &[(&str, &str, &str)] = &[
        (
            "ANTHROPIC_API_KEY",
            "Anthropic Claude 4.x",
            "claude-sonnet-4-6",
        ),
        ("XAI_API_KEY", "xAI Grok 4.x", "grok-4.3"),
        ("OPENAI_API_KEY", "OpenAI GPT-5.x", "gpt-5.5"),
    ];
    println!("  API Keys:");
    let mut any_key = false;
    for (env, name, model) in checks {
        let set = std::env::var(env).map(|k| !k.is_empty()).unwrap_or(false);
        if set {
            any_key = true;
        }
        println!(
            "  {} {} → {}",
            if set { "✓" } else { "✗" },
            name,
            if set {
                format!("key set, will use {model}")
            } else {
                format!("set {env} to enable")
            }
        );
    }
    if !any_key {
        println!("\n  ⚠ No API key found! Set ANTHROPIC_API_KEY to get started.");
        println!("  Export it in your shell: export ANTHROPIC_API_KEY=sk-ant-...");
    }

    let ollama_running = tokio::process::Command::new("ollama")
        .arg("list")
        .output()
        .await
        .map(|o| o.status.success())
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
