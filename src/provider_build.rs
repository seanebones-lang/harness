//! Build the default `ArcProvider` from loaded config + optional CLI model override.
//! Shared by `main` and `harness serve` hot-reload.

use anyhow::{Context, Result};
use harness_provider_core::ArcProvider;
use harness_provider_router::ProviderRouter;
use harness_provider_xai::{XaiConfig, XaiProvider};

use crate::ambient::AmbientProviders;
use crate::config::Config;

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
}

fn has_openai_key() -> bool {
    std::env::var("OPENAI_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false)
}

fn default_model_for_keys(has_xai: bool, has_anthropic: bool, has_openai: bool) -> String {
    if has_xai {
        "grok-4.3".to_string()
    } else if has_anthropic {
        "claude-sonnet-4-6".to_string()
    } else if has_openai {
        "gpt-5.5".to_string()
    } else {
        "qwen3-coder:30b".to_string()
    }
}

fn resolved_model(cfg: &Config, cli_model: Option<&str>) -> String {
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
    if uses_router_path(cfg) {
        Ok(build_router(cfg)?.into_arc())
    } else if uses_xai_only_path(cfg) {
        build_xai_provider(cfg, cli_model)
    } else {
        Ok(build_router(cfg)?.into_arc())
    }
}
