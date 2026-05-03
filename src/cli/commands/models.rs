//! `harness models [--set provider:model]`.

use anyhow::Result;

use crate::config::Config;

/// Handle `harness models [--set provider:model]`.
pub async fn handle_models_command(set: Option<String>, cfg: &Config) -> Result<()> {
    let catalogue: &[(&str, &[(&str, &str)])] = &[
        (
            "anthropic",
            &[
                ("claude-opus-4-7", "$5/$25 · 1M ctx · adaptive thinking"),
                ("claude-sonnet-4-6", "$3/$15 · 1M ctx · default ★"),
                ("claude-haiku-4-5", "$1/$5  · fast / cheap"),
            ],
        ),
        (
            "openai",
            &[
                ("gpt-5.5", "$5/$30  · 1M ctx"),
                ("gpt-5.4", "$2.50/$15"),
                ("gpt-5.4-mini", "$0.75/$4.50 · fast"),
                ("gpt-5.4-nano", "$0.20/$1.25 · ultra-cheap"),
                ("o4-mini", "$1.10/$4.40 · reasoning"),
            ],
        ),
        (
            "xai",
            &[
                ("grok-4.3", "$1.25/$2.50 · 1M ctx · flagship ★"),
                (
                    "grok-4.20-0309-reasoning",
                    "$2/$6   · pinned 2M ctx snapshot",
                ),
                ("grok-4-1-fast-reasoning", "$0.20/$0.50 · fast"),
            ],
        ),
        (
            "ollama",
            &[
                ("qwen3-coder:30b", "local · 256K ctx · agentic ★"),
                ("qwen2.5-coder:32b", "local · 92.7% HumanEval"),
                ("nomic-embed-text", "local · embed"),
            ],
        ),
    ];

    if let Some(ref model_spec) = set {
        let local_cfg = std::path::PathBuf::from(".harness").join("config.toml");
        let _ = std::fs::create_dir_all(".harness");
        let text = if local_cfg.exists() {
            std::fs::read_to_string(&local_cfg).unwrap_or_default()
        } else {
            String::new()
        };

        let (provider_part, model_part) = if model_spec.contains(':') {
            let mut parts = model_spec.splitn(2, ':');
            (
                parts.next().unwrap_or("").to_string(),
                parts.next().unwrap_or("").to_string(),
            )
        } else {
            (String::new(), model_spec.clone())
        };

        let mut doc: toml_edit::DocumentMut = text.parse().unwrap_or_default();

        if !doc.contains_key("provider") {
            doc["provider"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        doc["provider"]["model"] = toml_edit::value(model_part.as_str());

        if !provider_part.is_empty() {
            if !doc.contains_key("router") {
                doc["router"] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            doc["router"]["default"] = toml_edit::value(provider_part.as_str());
        }

        std::fs::write(&local_cfg, doc.to_string())?;
        println!(
            "✓ Default model set to '{model_spec}' in {}",
            local_cfg.display()
        );
        return Ok(());
    }

    println!("Available models (May 2026):");
    println!();
    for (provider, models) in catalogue {
        let env_key = match *provider {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "xai" => "XAI_API_KEY",
            _ => "",
        };
        let available = if env_key.is_empty() {
            "local".to_string()
        } else if std::env::var(env_key)
            .map(|k| !k.is_empty())
            .unwrap_or(false)
        {
            "✓ key set".to_string()
        } else {
            format!("✗ {} not set", env_key)
        };
        println!("  {provider} ({available})");
        for (model, desc) in *models {
            let current = cfg.provider.model.as_deref() == Some(model);
            let marker = if current { " ◀ current" } else { "" };
            println!("    {:42} {desc}{marker}", model);
        }
        println!();
    }

    let current = cfg.provider.model.as_deref().unwrap_or("claude-sonnet-4-6");
    println!("Current default: {current}");
    println!();
    println!("To switch: harness models --set <provider:model>");
    println!("Example:   harness models --set anthropic:claude-opus-4-7");

    Ok(())
}
