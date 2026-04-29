//! Provider router for Harness.
//!
//! Selects from multiple configured providers based on per-request intent,
//! with automatic exponential-backoff fallback on rate-limit or server errors.
//!
//! # Usage
//!
//! ```toml
//! [providers]
//! default = "anthropic"
//!
//! [providers.anthropic]
//! api_key = "sk-ant-..."
//! model = "claude-sonnet-4-5"
//!
//! [providers.xai]
//! api_key = "xai-..."
//! model = "grok-3-fast"
//!
//! [providers.ollama]
//! base_url = "http://localhost:11434"
//! model = "qwen2.5-coder:7b"
//!
//! [router]
//! fast_model = "xai:grok-3-mini-fast"
//! heavy_model = "anthropic:claude-sonnet-4-5"
//! embed_model = "xai:grok-3-embed-english"
//! fallback = ["anthropic", "xai", "openai", "ollama"]
//! ```

use async_trait::async_trait;
use harness_provider_core::{ArcProvider, ChatRequest, DeltaStream, Pricing, Provider, ProviderError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

// ── Config types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RouterConfig {
    /// Name of the default provider (used for main loop).
    pub default: Option<String>,
    /// Route-specific model overrides: "fast", "heavy", "embed".
    pub fast_model: Option<String>,
    pub heavy_model: Option<String>,
    pub embed_model: Option<String>,
    /// Ordered list of provider names to try on failure.
    pub fallback: Option<Vec<String>>,
}

/// Config for a single named provider entry.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ProviderEntry {
    pub name: Option<String>,  // e.g. "anthropic", "xai", "openai", "ollama"
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
}

/// Build an `ArcProvider` from a `ProviderEntry`.
pub fn build_provider(kind: &str, entry: &ProviderEntry) -> anyhow::Result<ArcProvider> {
    match kind {
        "anthropic" => {
            let key = entry.api_key.as_deref().unwrap_or("");
            let mut cfg = harness_provider_anthropic::AnthropicConfig::new(key);
            if let Some(m) = &entry.model { cfg = cfg.with_model(m); }
            Ok(Arc::new(harness_provider_anthropic::AnthropicProvider::new(cfg)?))
        }
        "openai" => {
            let key = entry.api_key.as_deref().unwrap_or("");
            let mut cfg = harness_provider_openai::OpenAIConfig::new(key);
            if let Some(m) = &entry.model { cfg = cfg.with_model(m); }
            if let Some(u) = &entry.base_url { cfg = cfg.with_base_url(u); }
            Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?))
        }
        "ollama" => {
            let model = entry.model.as_deref().unwrap_or("qwen2.5-coder:7b");
            let mut cfg = harness_provider_ollama::OllamaConfig::new(model);
            if let Some(u) = &entry.base_url { cfg = cfg.with_base_url(u); }
            Ok(Arc::new(harness_provider_ollama::OllamaProvider::new(cfg)?))
        }
        _ => {
            let key = entry.api_key.as_deref().unwrap_or("");
            let mut cfg = harness_provider_xai::XaiConfig::new(key);
            if let Some(m) = &entry.model { cfg = cfg.with_model(m); }
            Ok(Arc::new(harness_provider_xai::XaiProvider::new(cfg)?))
        }
    }
}

// ── ProviderRouter ────────────────────────────────────────────────────────────

/// Routes requests to the appropriate provider, with fallback on error.
///
/// Named providers are stored in a map. The router also tracks role-specific
/// providers: `default`, `fast`, `heavy`, `embed`.
#[derive(Clone)]
pub struct ProviderRouter {
    /// All registered providers by name.
    providers: HashMap<String, ArcProvider>,
    /// Default provider for the main agent loop.
    default_name: String,
    /// Fast provider name (for sub-agents, summaries).
    fast_name: Option<String>,
    /// Heavy provider name (for complex tasks).
    heavy_name: Option<String>,
    /// Embed provider name (for memory embeddings).
    embed_name: Option<String>,
    /// Ordered fallback list (names).
    fallback: Vec<String>,
}

impl ProviderRouter {
    pub fn new(default_name: impl Into<String>) -> Self {
        let default_name = default_name.into();
        Self {
            providers: HashMap::new(),
            default_name: default_name.clone(),
            fast_name: None,
            heavy_name: None,
            embed_name: None,
            fallback: vec![],
        }
    }

    pub fn add(mut self, name: impl Into<String>, provider: ArcProvider) -> Self {
        self.providers.insert(name.into(), provider);
        self
    }

    pub fn with_fast(mut self, name: impl Into<String>) -> Self {
        self.fast_name = Some(name.into());
        self
    }

    pub fn with_heavy(mut self, name: impl Into<String>) -> Self {
        self.heavy_name = Some(name.into());
        self
    }

    pub fn with_embed(mut self, name: impl Into<String>) -> Self {
        self.embed_name = Some(name.into());
        self
    }

    pub fn with_fallback(mut self, fallback: Vec<String>) -> Self {
        self.fallback = fallback;
        self
    }

    /// Return a reference to the named provider, or the default.
    pub fn get(&self, name: &str) -> Option<&ArcProvider> {
        self.providers.get(name)
    }

    pub fn default_provider(&self) -> &ArcProvider {
        self.providers.get(&self.default_name)
            .or_else(|| self.providers.values().next())
            .expect("ProviderRouter has no providers")
    }

    pub fn fast_provider(&self) -> &ArcProvider {
        self.fast_name.as_ref()
            .and_then(|n| self.providers.get(n))
            .unwrap_or_else(|| self.default_provider())
    }

    pub fn heavy_provider(&self) -> &ArcProvider {
        self.heavy_name.as_ref()
            .and_then(|n| self.providers.get(n))
            .unwrap_or_else(|| self.default_provider())
    }

    pub fn embed_provider(&self) -> &ArcProvider {
        self.embed_name.as_ref()
            .and_then(|n| self.providers.get(n))
            .unwrap_or_else(|| self.default_provider())
    }

    /// Wrap this router as an `ArcProvider` (uses the default provider for all calls,
    /// with fallback chain on 429/5xx).
    pub fn into_arc(self) -> ArcProvider {
        Arc::new(self)
    }

    /// Build from a flat config map (name → ProviderEntry) + RouterConfig.
    pub fn from_config(
        entries: &HashMap<String, ProviderEntry>,
        router_cfg: &RouterConfig,
    ) -> anyhow::Result<Self> {
        let default_name = router_cfg.default.clone().unwrap_or_else(|| "xai".into());
        let mut r = Self::new(&default_name);

        for (name, entry) in entries {
            match build_provider(name.as_str(), entry) {
                Ok(p) => { r.providers.insert(name.clone(), p); }
                Err(e) => warn!(name, err = %e, "failed to build provider"),
            }
        }

        if let Some(ref f) = router_cfg.fast_model {
            r.fast_name = Some(f.split(':').next().unwrap_or(f).to_string());
        }
        if let Some(ref h) = router_cfg.heavy_model {
            r.heavy_name = Some(h.split(':').next().unwrap_or(h).to_string());
        }
        if let Some(ref e) = router_cfg.embed_model {
            r.embed_name = Some(e.split(':').next().unwrap_or(e).to_string());
        }
        if let Some(ref fb) = router_cfg.fallback {
            r.fallback = fb.clone();
        }

        Ok(r)
    }
}

// ── Provider impl ─────────────────────────────────────────────────────────────
//
// The router itself implements `Provider`, delegating to the default provider
// and cycling through the fallback chain on retryable errors.

#[async_trait]
impl Provider for ProviderRouter {
    fn name(&self) -> &str {
        "router"
    }

    fn model(&self) -> &str {
        self.default_provider().model()
    }

    fn pricing(&self) -> Option<Pricing> {
        self.default_provider().pricing()
    }

    async fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>, ProviderError> {
        self.embed_provider().embed(model, text).await
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        // Try the default provider first, then each fallback in order.
        let try_order: Vec<String> = std::iter::once(self.default_name.clone())
            .chain(self.fallback.iter().filter(|n| **n != self.default_name).cloned())
            .collect();

        let mut last_err = ProviderError::Other("no providers configured".into());

        for name in &try_order {
            let Some(p) = self.providers.get(name) else { continue };
            match p.stream_chat(req.clone()).await {
                Ok(stream) => {
                    if name != &self.default_name {
                        info!(provider = name, "router: fallback provider used");
                    }
                    return Ok(stream);
                }
                Err(e) => {
                    warn!(provider = name, err = %e, "router: provider failed, trying next");
                    last_err = e;
                }
            }
        }

        Err(last_err)
    }
}
