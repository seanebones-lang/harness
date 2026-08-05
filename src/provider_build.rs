//! Build the default `ArcProvider` from loaded config + optional CLI model override.
//! Shared by `main` and `harness serve` hot-reload.

use anyhow::{Context, Result};
use harness_provider_core::ArcProvider;
use harness_provider_router::ProviderRouter;
use harness_provider_xai::{XaiConfig, XaiProvider};

use crate::ambient::AmbientProviders;
use crate::config::{self, Config};

fn has_anthropic_key() -> bool {
    std::env::var("ANTHROPIC_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}

fn has_xai_key(cfg: &Config) -> bool {
    cfg.provider.api_key.is_some()
        || std::env::var("XAI_API_KEY")
            .map(|k| !k.is_empty())
            .unwrap_or(false)
        || cfg
            .providers
            .get("xai")
            .and_then(|e| e.api_key.as_ref())
            .map(|k| !k.is_empty())
            .unwrap_or(false)
}

fn has_openai_key() -> bool {
    std::env::var("OPENAI_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}

fn default_model_for_keys(has_xai: bool, has_anthropic: bool, has_openai: bool) -> String {
    if has_xai {
        "grok-4.5".to_string()
    } else if has_anthropic {
        "claude-sonnet-4-6".to_string()
    } else if has_openai {
        "gpt-5.5".to_string()
    } else {
        "qwen3-coder:30b".to_string()
    }
}

pub fn resolved_model(cfg: &Config, cli_model: Option<&str>) -> String {
    cli_model
        .map(|s| s.to_string())
        .or_else(|| cfg.provider.model.clone())
        .unwrap_or_else(|| {
            default_model_for_keys(has_xai_key(cfg), has_anthropic_key(), has_openai_key())
        })
}

fn uses_router_path(cfg: &Config) -> bool {
    !cfg.providers.is_empty() || has_anthropic_key() || has_openai_key()
}

fn uses_xai_only_path(cfg: &Config) -> bool {
    has_xai_key(cfg) && !uses_router_path(cfg)
}

fn build_xai_provider(cfg: &Config, cli_model: Option<&str>) -> Result<ArcProvider> {
    let model = resolved_model(cfg, cli_model);
    let api_key = cfg
        .provider
        .api_key
        .clone()
        .or_else(|| cfg.providers.get("xai").and_then(|e| e.api_key.clone()))
        .or_else(|| std::env::var("XAI_API_KEY").ok())
        .filter(|k| !k.is_empty())
        .context("XAI_API_KEY or [provider].api_key is required for the xAI provider")?;
    let xai_cfg = XaiConfig::new(&api_key)
        .with_model(&model)
        .with_max_tokens(cfg.provider.max_tokens.unwrap_or(8192))
        .with_temperature(cfg.provider.temperature.unwrap_or(0.7));
    Ok(std::sync::Arc::new(XaiProvider::new(xai_cfg)?))
}

/// Build a multi-provider router from config (no CLI model override on router entries).
pub fn build_router(cfg: &Config) -> Result<ProviderRouter> {
    ProviderRouter::from_config(&cfg.providers, &cfg.router)
        .context("failed to build provider router")
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
    if uses_xai_only_path(cfg) {
        let p = build_xai_provider(cfg, cli_model)?;
        return Ok(AmbientProviders::same(p));
    }

    let router = build_router(cfg)?;
    let summary = clone_provider_or_default(&router, |r| r.fast_provider())?;
    let embed = clone_provider_or_default(&router, |r| r.embed_provider())?;
    Ok(AmbientProviders { summary, embed })
}

/// Match `main.rs` provider construction (May 2026 smart router + xAI-only fast path).
pub fn build_arc_provider(cfg: &Config, cli_model: Option<&str>) -> Result<ArcProvider> {
    let mut cfg = cfg.clone();
    if let Some(m) = cli_model {
        cfg.provider.model = Some(m.to_string());
        let default = cfg.router.default.clone().unwrap_or_else(|| "xai".into());
        if let Some(entry) = cfg.providers.get_mut(&default) {
            entry.model = Some(m.to_string());
        }
        config::sync_provider_model(&mut cfg);
    }
    if uses_router_path(&cfg) {
        Ok(build_router(&cfg)?.into_arc())
    } else if uses_xai_only_path(&cfg) {
        build_xai_provider(&cfg, cli_model)
    } else {
        Ok(build_router(&cfg)?.into_arc())
    }
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
        Some("xai") => {
            let api_key = cfg
                .providers
                .get("xai")
                .and_then(|e| e.api_key.clone())
                .or_else(|| cfg.provider.api_key.clone())
                .or_else(|| std::env::var("XAI_API_KEY").ok())
                .filter(|k| !k.is_empty())
                .context("xAI key required for worker model")?;
            let xcfg = XaiConfig::new(api_key)
                .with_model(model)
                .with_max_tokens(cfg.provider.max_tokens.unwrap_or(8192))
                .with_temperature(cfg.provider.temperature.unwrap_or(0.7));
            Ok((
                std::sync::Arc::new(XaiProvider::new(xcfg)?),
                model.to_string(),
            ))
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
