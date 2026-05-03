//! Build the default `ArcProvider` from loaded config + optional CLI model override.
//! Shared by `main` and `harness serve` hot-reload.

use anyhow::{Context, Result};
use harness_provider_core::ArcProvider;
use harness_provider_router::ProviderRouter;
use harness_provider_xai::{XaiConfig, XaiProvider};

use crate::config::Config;

/// Match `main.rs` provider construction (May 2026 smart router + xAI-only fast path).
pub fn build_arc_provider(cfg: &Config, cli_model: Option<&str>) -> Result<ArcProvider> {
    let has_anthropic = std::env::var("ANTHROPIC_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let has_xai = cfg.provider.api_key.is_some()
        || std::env::var("XAI_API_KEY")
            .map(|k| !k.is_empty())
            .unwrap_or(false);
    let has_openai = std::env::var("OPENAI_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);

    let model = cli_model
        .map(|s| s.to_string())
        .or_else(|| cfg.provider.model.clone())
        .unwrap_or_else(|| {
            if has_anthropic {
                "claude-sonnet-4-6".to_string()
            } else if has_xai {
                "grok-4.3".to_string()
            } else if has_openai {
                "gpt-5.5".to_string()
            } else {
                "qwen3-coder:30b".to_string()
            }
        });

    if !cfg.providers.is_empty() || has_anthropic || has_openai {
        let router = ProviderRouter::from_config(&cfg.providers, &cfg.router)
            .context("failed to build provider router")?;
        Ok(router.into_arc())
    } else if has_xai {
        let api_key = cfg
            .provider
            .api_key
            .clone()
            .or_else(|| std::env::var("XAI_API_KEY").ok())
            .unwrap();
        let xai_cfg = XaiConfig::new(&api_key)
            .with_model(&model)
            .with_max_tokens(cfg.provider.max_tokens.unwrap_or(8192))
            .with_temperature(cfg.provider.temperature.unwrap_or(0.7));
        Ok(std::sync::Arc::new(XaiProvider::new(xai_cfg)?))
    } else {
        let router = ProviderRouter::from_config(&cfg.providers, &cfg.router)
            .context("failed to build provider router")?;
        Ok(router.into_arc())
    }
}
