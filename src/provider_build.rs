//! Build the default `ArcProvider` from loaded config + optional CLI model override.
//! Shared by `main` and `harness serve` hot-reload.

use anyhow::{Context, Result};
use harness_provider_core::ArcProvider;
use harness_provider_router::ProviderRouter;

use crate::ambient::AmbientProviders;
use crate::config::Config;

pub fn resolved_model(cfg: &Config, cli_model: Option<&str>) -> String {
    cli_model
        .map(|s| s.to_string())
        .or_else(|| {
            cfg.router
                .default
                .as_deref()
                .and_then(|provider| cfg.providers.get(provider))
                .and_then(|entry| entry.model.clone())
        })
        .or_else(|| cfg.provider.model.clone())
        .unwrap_or_else(|| "<model not configured>".to_string())
}

/// Build the user-selected provider route. Legacy `[provider]` xAI settings are
/// migrated in memory only when they are the sole provider configuration.
pub fn build_router(cfg: &Config) -> Result<ProviderRouter> {
    let mut providers = cfg.providers.clone();
    let mut router = cfg.router.clone();
    if providers.is_empty() && cfg.provider.api_key.is_some() {
        let mut entry = harness_provider_router::configured_provider_entry("xai");
        entry.api_key = cfg.provider.api_key.clone();
        entry.model = cfg.provider.model.clone();
        providers.insert("xai".to_string(), entry);
        router.default.get_or_insert_with(|| "xai".to_string());
        router.fallback.get_or_insert_with(Vec::new);
    }
    ProviderRouter::from_config(&providers, &router).context("failed to build provider router")
}

fn clone_provider_or_default(
    router: &ProviderRouter,
    pick: fn(&ProviderRouter) -> Option<&ArcProvider>,
) -> Result<ArcProvider> {
    pick(router)
        .or_else(|| router.default_provider())
        .cloned()
        .context("no provider available for ambient consolidation")
}

/// Providers for ambient consolidation: router fast route for summaries, embed route for vectors.
pub fn build_ambient_providers(cfg: &Config, cli_model: Option<&str>) -> Result<AmbientProviders> {
    let cfg = with_cli_model(cfg, cli_model);
    let router = build_router(&cfg)?;
    let summary = clone_provider_or_default(&router, |r| r.fast_provider())?;
    let embed = clone_provider_or_default(&router, |r| r.embed_provider())?;
    Ok(AmbientProviders { summary, embed })
}

/// Build the provider selected by the saved route, with its exact fallback order.
pub fn build_arc_provider(cfg: &Config, cli_model: Option<&str>) -> Result<ArcProvider> {
    let cfg = with_cli_model(cfg, cli_model);
    Ok(build_router(&cfg)?.into_arc())
}

fn with_cli_model(cfg: &Config, cli_model: Option<&str>) -> Config {
    let mut cfg = cfg.clone();
    if let Some(model) = cli_model {
        cfg.provider.model = Some(model.to_string());
        if let Some(default) = cfg.router.default.clone() {
            if let Some(entry) = cfg.providers.get_mut(&default) {
                entry.model = Some(model.to_string());
            }
        }
    }
    cfg
}

/// Known `provider:` prefixes for `provider:model` specs (ollama models keep their own `:` tags).
const PROVIDER_PREFIXES: &[&str] = &[
    "ollama",
    "xai",
    "anthropic",
    "openai",
    "mistral",
    "gemini",
    "bedrock",
    "mlx",
    "openai-compatible",
    "cerebras",
    "deepseek",
    "fireworks",
    "groq",
    "huggingface",
    "nvidia",
    "openrouter",
    "perplexity",
    "sambanova",
    "together",
];

/// Split `ollama:qwen2.5-coder:1.5b` → (`ollama`, `qwen2.5-coder:1.5b`).
/// Bare `qwen2.5-coder:1.5b` stays unsplit (Ollama tag colon).
pub fn split_provider_model(spec: &str) -> (Option<&str>, &str) {
    let spec = spec.trim();
    if let Some((head, rest)) = spec.split_once(':') {
        let head_l = head.to_ascii_lowercase();
        if PROVIDER_PREFIXES.iter().any(|p| *p == head_l) && !rest.is_empty() {
            return (Some(head), rest);
        }
    }
    (None, spec)
}

fn looks_like_local_ollama_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    if m.contains("grok") || m.contains("claude") || m.contains("gpt-") || m.starts_with("o1") {
        return false;
    }
    m.contains("qwen")
        || m.contains("coder")
        || m.contains("llama")
        || m.contains("mistral")
        || m.contains("phi")
        || m.contains("deepseek")
        || m.contains("nomic")
        || m.contains("gemma")
}

/// Build a provider aimed at a specific model (swarm / spawn_agent slaves).
///
/// Accepts `ollama:qwen2.5-coder:1.5b`, bare Ollama tags, or cloud model ids.
/// Falls back to the orchestrator path when the kind cannot be inferred.
pub fn build_worker_provider(cfg: &Config, model_spec: &str) -> Result<(ArcProvider, String)> {
    let (kind, model) = split_provider_model(model_spec);
    let kind = kind.map(|k| k.to_ascii_lowercase()).or_else(|| {
        if looks_like_local_ollama_model(model) {
            Some("ollama".into())
        } else if model.to_ascii_lowercase().contains("grok") {
            Some("xai".into())
        } else if model.to_ascii_lowercase().contains("claude") {
            Some("anthropic".into())
        } else if model.to_ascii_lowercase().contains("gpt") {
            Some("openai".into())
        } else {
            None
        }
    });

    match kind.as_deref() {
        Some("ollama") => {
            let entry = cfg.providers.get("ollama");
            let mut ocfg = harness_provider_ollama::OllamaConfig::new(model);
            if let Some(u) = entry.and_then(|e| e.base_url.as_deref()) {
                ocfg = ocfg.with_base_url(u);
            }
            let p = std::sync::Arc::new(harness_provider_ollama::OllamaProvider::new(ocfg)?);
            Ok((p, model.to_string()))
        }
        Some(other) => {
            if let Some(entry) = cfg.providers.get(other) {
                let mut entry = entry.clone();
                entry.model = Some(model.to_string());
                let mut providers = cfg.providers.clone();
                providers.insert(other.to_string(), entry);
                let mut router_cfg = cfg.router.clone();
                router_cfg.default = Some(other.to_string());
                let router = ProviderRouter::from_config(&providers, &router_cfg)?;
                let p = router
                    .default_provider()
                    .cloned()
                    .context("worker provider missing from router")?;
                Ok((p, model.to_string()))
            } else {
                Ok((build_arc_provider(cfg, Some(model))?, model.to_string()))
            }
        }
        None => Ok((build_arc_provider(cfg, Some(model))?, model.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_provider_model_ollama_keeps_tag_colon() {
        assert_eq!(
            split_provider_model("ollama:qwen2.5-coder:1.5b"),
            (Some("ollama"), "qwen2.5-coder:1.5b")
        );
        assert_eq!(
            split_provider_model("qwen2.5-coder:1.5b"),
            (None, "qwen2.5-coder:1.5b")
        );
        assert_eq!(split_provider_model("grok-4.5"), (None, "grok-4.5"));
        assert_eq!(
            split_provider_model("xai:grok-4.5"),
            (Some("xai"), "grok-4.5")
        );
    }

    #[test]
    fn looks_like_local_ollama_model_detects_coders() {
        assert!(looks_like_local_ollama_model("qwen2.5-coder:1.5b"));
        assert!(looks_like_local_ollama_model("llama3.2:1b"));
        assert!(!looks_like_local_ollama_model("grok-4.5"));
        assert!(!looks_like_local_ollama_model("claude-sonnet-4-6"));
    }
}
