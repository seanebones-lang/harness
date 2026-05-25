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
                (
                    "grok-4.1-fast",
                    "$0.20/$0.50 · fast · reasoning via API param",
                ),
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
        let (provider_part, model_part) = if model_spec.contains(':') {
            let mut parts = model_spec.splitn(2, ':');
            (
                parts.next().unwrap_or("").to_string(),
                parts.next().unwrap_or("").to_string(),
            )
        } else {
            (String::new(), model_spec.clone())
        };

        let paths = config_paths_to_update();
        for path in paths {
            apply_model_set(&path, &provider_part, &model_part)?;
            println!(
                "✓ Default model set to '{model_spec}' in {}",
                path.display()
            );
        }
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

fn config_paths_to_update() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    let local = std::path::PathBuf::from(".harness/config.toml");
    if local.parent().is_some() {
        let _ = std::fs::create_dir_all(".harness");
        paths.push(local);
    }
    if let Some(home) = dirs::home_dir() {
        let global = home.join(".harness/config.toml");
        let _ = std::fs::create_dir_all(global.parent().unwrap_or(std::path::Path::new(".")));
        if !paths.iter().any(|p| p == &global) {
            paths.push(global);
        }
    }
    paths
}

fn apply_model_set(
    path: &std::path::Path,
    provider_part: &str,
    model_part: &str,
) -> Result<()> {
    let text = if path.exists() {
        std::fs::read_to_string(path)?
    } else {
        String::new()
    };

    let mut doc: toml_edit::DocumentMut = text.parse().unwrap_or_default();

    if !doc.contains_key("provider") {
        doc["provider"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    doc["provider"]["model"] = toml_edit::value(model_part);

    let router_default = if !provider_part.is_empty() {
        if !doc.contains_key("router") {
            doc["router"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        doc["router"]["default"] = toml_edit::value(provider_part);
        provider_part.to_string()
    } else {
        doc.get("router")
            .and_then(|r| r.get("default"))
            .and_then(|v| v.as_str())
            .unwrap_or("xai")
            .to_string()
    };

    if !doc.contains_key("providers") {
        doc["providers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let providers = doc["providers"].as_table_mut().expect("providers table");
    if !providers.contains_key(&router_default) {
        providers[&router_default] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    providers[&router_default]["model"] = toml_edit::value(model_part);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, doc.to_string())?;
    Ok(())
}
