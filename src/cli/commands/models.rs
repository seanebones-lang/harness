//! `harness models [--set provider:model]`.

use anyhow::Result;

use crate::config::{self, Config};

/// Handle `harness models [--set provider:model]`.
pub async fn handle_models_command(set: Option<String>, cfg: &Config) -> Result<()> {
    let catalogue: &[(&str, &[(&str, &str)])] = &[
        (
            "anthropic",
            &[
                ("claude-opus-4-7", "$5/$25 · 1M ctx · adaptive thinking"),
                ("claude-sonnet-4-6", "$3/$15 · 1M ctx"),
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
            "nvidia",
            &[
                (
                    "deepseek-ai/deepseek-v4-flash-0731",
                    "NVIDIA · fast + reliable",
                ),
                (
                    "nvidia/nemotron-3-super-120b-a12b",
                    "NVIDIA · 120B MoE · deep reasoning",
                ),
                (
                    "nvidia/nemotron-3-ultra-550b-a55b",
                    "NVIDIA · 550B flagship (can 503 under load)",
                ),
            ],
        ),
        (
            "xai",
            &[
                ("grok-4.5", "$1.25/$2.50 · 1M ctx"),
                ("grok-4.3", "$1.25/$2.50 · 1M ctx"),
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
                ("qwen2.5-coder:1.5b", "local · small code model"),
                ("qwen2.5-coder:3b", "local · small-mid code"),
                ("llama3.2:1b", "local · tiny general"),
                ("qwen3-coder:30b", "local · 256K ctx · agentic"),
                ("qwen2.5-coder:32b", "local · 92.7% HumanEval"),
                ("nomic-embed-text", "local · embed"),
            ],
        ),
        (
            "gemini",
            &[
                ("gemini-2.0-flash", "Google · fast"),
                ("gemini-1.5-pro", "Google · 1M ctx"),
                ("gemini-1.5-flash", "Google · fast / cheap"),
            ],
        ),
        (
            "bedrock",
            &[
                (
                    "anthropic.claude-3-5-sonnet-20241022-v2:0",
                    "AWS Bedrock · Claude 3.5 Sonnet",
                ),
                ("amazon.nova-pro-v1:0", "AWS Bedrock · Nova Pro"),
            ],
        ),
        (
            "deepseek",
            &[
                ("deepseek-v4-flash", "DeepSeek API"),
                ("deepseek-v4-pro", "DeepSeek API"),
            ],
        ),
        ("cerebras", &[]),
        ("fireworks", &[]),
        ("groq", &[]),
        ("huggingface", &[]),
        ("mistral", &[("mistral-large-latest", "Mistral API")]),
        ("mlx", &[]),
        ("openrouter", &[]),
        ("perplexity", &[]),
        ("sambanova", &[]),
        ("together", &[]),
    ];

    if let Some(ref model_spec) = set {
        let (provider_part, model_part) = model_spec.split_once(':').ok_or_else(|| {
            anyhow::anyhow!(
                "model changes require provider:model; use `harness route model <provider> <model>`"
            )
        })?;
        let path = config::active_config_toml_path();
        apply_model_set(&path, provider_part, model_part)?;
        println!("✓ Model set to '{model_spec}' in {}", path.display());
        println!("Use `harness route show` to inspect the exact route.");
        return Ok(());
    }

    println!("Provider/model catalogue (examples only; no model is preferred):");
    println!();
    let current_provider = cfg.router.default.as_deref().unwrap_or("<route not set>");
    let current_model = cfg
        .router
        .default
        .as_deref()
        .and_then(|provider| cfg.providers.get(provider))
        .and_then(|entry| entry.model.as_deref())
        .unwrap_or("<model not set>");
    let mut sorted_catalogue = catalogue.to_vec();
    sorted_catalogue.sort_by_key(|(provider, _)| *provider);
    for (provider, models) in sorted_catalogue {
        let env_key = match provider {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "xai" => "XAI_API_KEY",
            "gemini" => "GEMINI_API_KEY",
            "bedrock" => "AWS_ACCESS_KEY_ID",
            other => harness_provider_router::provider_preset(other)
                .and_then(|preset| preset.api_key_envs.first())
                .copied()
                .unwrap_or(""),
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
        if models.is_empty() {
            println!("    <use any model id supported by this provider>");
        }
        for (model, desc) in models {
            let current = current_provider == provider && current_model == *model;
            let marker = if current { " ◀ current" } else { "" };
            println!("    {:42} {desc}{marker}", model);
        }
        println!();
    }

    println!("Current primary: {current_provider}:{current_model}");
    println!();
    println!("Set route:    harness route set <provider:model> [provider:model ...]");
    println!("Change model: harness route model <provider> <model>");

    Ok(())
}

fn apply_model_set(path: &std::path::Path, provider_part: &str, model_part: &str) -> Result<()> {
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

    if provider_part.is_empty() || model_part.is_empty() {
        anyhow::bail!("provider and model are required");
    }
    if !doc.contains_key("router") {
        doc["router"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    doc["router"]["default"] = toml_edit::value(provider_part);
    let router_default = provider_part.to_string();

    if !doc.contains_key("providers") {
        doc["providers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let providers = doc["providers"].as_table_mut().ok_or_else(|| {
        anyhow::anyhow!("config root `providers` is not a table — fix config TOML")
    })?;
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
