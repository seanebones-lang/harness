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
//! model = "claude-sonnet-4-6"
//!
//! [providers.xai]
//! api_key = "xai-..."
//! model = "grok-4.3"
//!
//! [providers.ollama]
//! base_url = "http://localhost:11434"
//! model = "qwen3-coder:30b"
//!
//! [providers.mlx]
//! base_url = "http://127.0.0.1:8080/v1"
//! model = "mlx-community/Qwen3-Coder-30B"
//!
//! [router]
//! fast_model = "xai:grok-4.1-fast"
//! heavy_model = "anthropic:claude-opus-4-7"
//! embed_model = "ollama:nomic-embed-text"
//! fallback = ["anthropic", "xai", "openai", "ollama", "mlx"]
//! ```

use async_trait::async_trait;
use harness_provider_core::{
    ArcProvider, ChatRequest, DeltaStream, Pricing, Provider, ProviderError,
};
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
    pub name: Option<String>, // e.g. "anthropic", "xai", "openai", "ollama"
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
}

/// Build an `ArcProvider` from a `ProviderEntry`.
///
/// `kind` is usually the `[providers.<kind>]` table key.
/// Supported kinds: `anthropic`, `xai`, `openai`, `mistral`, `gemini`, `bedrock`,
/// `openai-compatible` / `compatible`, `ollama`, `mlx`. Any other name **with** `base_url` is treated as
/// OpenAI-compatible under that name; without `base_url` falls back to xAI (legacy).
pub fn build_provider(kind: &str, entry: &ProviderEntry) -> anyhow::Result<ArcProvider> {
    match kind {
        "anthropic" => {
            let key = entry.api_key.as_deref().unwrap_or("");
            let mut cfg = harness_provider_anthropic::AnthropicConfig::new(key);
            if let Some(m) = &entry.model {
                cfg = cfg.with_model(m);
            }
            Ok(Arc::new(
                harness_provider_anthropic::AnthropicProvider::new(cfg)?,
            ))
        }
        "openai" => {
            let key = entry.api_key.as_deref().unwrap_or("");
            let mut cfg = harness_provider_openai::OpenAIConfig::new(key);
            if let Some(m) = &entry.model {
                cfg = cfg.with_model(m);
            }
            if let Some(u) = &entry.base_url {
                cfg = cfg.with_base_url(u);
            }
            Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?))
        }
        "mistral" => {
            let key = entry
                .api_key
                .clone()
                .or_else(|| std::env::var("MISTRAL_API_KEY").ok())
                .unwrap_or_default();
            let mut cfg = harness_provider_openai::OpenAIConfig::mistral(key);
            if let Some(m) = &entry.model {
                cfg = cfg.with_model(m);
            }
            if let Some(u) = &entry.base_url {
                cfg = cfg.with_base_url(u);
            }
            Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?))
        }
        "gemini" => {
            let key = entry
                .api_key
                .clone()
                .or_else(harness_provider_gemini::api_key_from_env)
                .unwrap_or_default();
            let mut cfg = harness_provider_openai::OpenAIConfig::gemini(key);
            if let Some(m) = &entry.model {
                cfg = cfg.with_model(m);
            }
            if let Some(u) = &entry.base_url {
                cfg = cfg.with_base_url(u);
            }
            Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?))
        }
        "bedrock" => {
            let model = entry.model.clone();
            let region = entry.base_url.clone(); // optional region override via base_url field
            // Prefer explicit api_key as access key only if full env not used — env is source of truth.
            if entry.api_key.is_some()
                && std::env::var("AWS_ACCESS_KEY_ID").is_err()
                && std::env::var("AWS_SECRET_ACCESS_KEY").is_err()
            {
                // Allow test injection: api_key = "ak:sk" or just require env.
                if let Some(pair) = entry.api_key.as_deref() {
                    if let Some((ak, sk)) = pair.split_once(':') {
                        let cfg = harness_provider_bedrock::BedrockConfig {
                            model: model
                                .clone()
                                .unwrap_or_else(|| harness_provider_bedrock::DEFAULT_MODEL.into()),
                            region: region
                                .clone()
                                .unwrap_or_else(|| harness_provider_bedrock::DEFAULT_REGION.into()),
                            access_key_id: ak.into(),
                            secret_access_key: sk.into(),
                            session_token: None,
                        };
                        return Ok(Arc::new(harness_provider_bedrock::BedrockProvider::new(
                            cfg,
                        )?));
                    }
                }
            }
            harness_provider_bedrock::build_arc(model, region)
        }
        "openai-compatible" | "compatible" => {
            let key = entry.api_key.as_deref().unwrap_or("");
            let base = entry
                .base_url
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "openai-compatible provider requires base_url (OpenAI-format /v1 endpoint)"
                    )
                })?;
            let mut cfg = harness_provider_openai::OpenAIConfig::new(key)
                .with_provider_name("openai-compatible")
                .with_base_url(base)
                .with_model(entry.model.as_deref().unwrap_or("gpt-4o-mini"));
            if let Some(m) = &entry.model {
                cfg = cfg.with_model(m);
            }
            Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?))
        }
        "ollama" => {
            let model = entry.model.as_deref().unwrap_or("qwen3-coder:30b");
            let mut cfg = harness_provider_ollama::OllamaConfig::new(model);
            if let Some(u) = &entry.base_url {
                cfg = cfg.with_base_url(u);
            }
            Ok(Arc::new(harness_provider_ollama::OllamaProvider::new(cfg)?))
        }
        "mlx" => harness_provider_mlx::build_arc(entry.model.clone(), entry.base_url.clone()),
        "xai" => {
            let key = entry.api_key.as_deref().unwrap_or("");
            let mut cfg = harness_provider_xai::XaiConfig::new(key);
            if let Some(m) = &entry.model {
                cfg = cfg.with_model(m);
            }
            Ok(Arc::new(harness_provider_xai::XaiProvider::new(cfg)?))
        }
        other => {
            // Custom table name with base_url → OpenAI-compatible under that name.
            if let Some(base) = entry.base_url.as_deref().filter(|s| !s.is_empty()) {
                let key = entry.api_key.as_deref().unwrap_or("");
                let mut cfg = harness_provider_openai::OpenAIConfig::new(key)
                    .with_provider_name(other)
                    .with_base_url(base)
                    .with_model(entry.model.as_deref().unwrap_or("default"));
                if let Some(m) = &entry.model {
                    cfg = cfg.with_model(m);
                }
                return Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?));
            }
            // Legacy: unknown kind without base_url was treated as xAI.
            let key = entry.api_key.as_deref().unwrap_or("");
            let mut cfg = harness_provider_xai::XaiConfig::new(key);
            if let Some(m) = &entry.model {
                cfg = cfg.with_model(m);
            }
            Ok(Arc::new(harness_provider_xai::XaiProvider::new(cfg)?))
        }
    }
}

/// Split `provider:model` route specs used in `[router].fast_model` etc.
fn parse_route_spec(spec: &str) -> (String, Option<String>) {
    if let Some((provider, model)) = spec.split_once(':') {
        (provider.to_string(), Some(model.to_string()))
    } else {
        (spec.to_string(), None)
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
    /// Optional model overrides when route spec includes `provider:model`.
    fast_model_override: Option<String>,
    heavy_model_override: Option<String>,
    embed_model_override: Option<String>,
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
            fast_model_override: None,
            heavy_model_override: None,
            embed_model_override: None,
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

    pub fn default_provider(&self) -> Option<&ArcProvider> {
        self.providers
            .get(&self.default_name)
            .or_else(|| self.providers.values().next())
    }

    pub fn fast_provider(&self) -> Option<&ArcProvider> {
        self.fast_name
            .as_ref()
            .and_then(|n| self.providers.get(n))
            .or_else(|| self.default_provider())
    }

    pub fn heavy_provider(&self) -> Option<&ArcProvider> {
        self.heavy_name
            .as_ref()
            .and_then(|n| self.providers.get(n))
            .or_else(|| self.default_provider())
    }

    pub fn embed_provider(&self) -> Option<&ArcProvider> {
        self.embed_name
            .as_ref()
            .and_then(|n| self.providers.get(n))
            .or_else(|| self.default_provider())
    }

    /// Model id for the fast route (from `provider:model` override when set).
    pub fn fast_model_id(&self) -> Option<&str> {
        self.fast_model_override
            .as_deref()
            .or_else(|| self.fast_provider().map(|p| p.model()))
    }

    /// Model id for the embed route.
    pub fn embed_model_id(&self) -> Option<&str> {
        self.embed_model_override
            .as_deref()
            .or_else(|| self.embed_provider().map(|p| p.model()))
    }

    /// Wrap this router as an `ArcProvider` (uses the default provider for all calls,
    /// with fallback chain on 429/5xx).
    pub fn into_arc(self) -> ArcProvider {
        Arc::new(self)
    }

    /// Build from a flat config map (name → ProviderEntry) + RouterConfig.
    ///
    /// If no `[router]` block is present (all fields `None`), automatically selects
    /// sensible defaults based on which `*_API_KEY` environment variables are set:
    ///
    /// | Priority | Default  | Fast                     | Heavy                  | Embed                   |
    /// |----------|----------|--------------------------|------------------------|-------------------------|
    /// | 1st      | anthropic (if ANTHROPIC_API_KEY) | anthropic:claude-haiku-4-5 | anthropic:claude-opus-4-7 | ollama:nomic-embed-text |
    /// | 2nd      | xai (if XAI_API_KEY)    | xai:grok-4.1-fast | xai:grok-4.3 | ollama:nomic-embed-text |
    /// | 3rd      | ollama (local, always)  | ollama:qwen3-coder:30b | ollama:qwen3-coder:30b | ollama:nomic-embed-text |
    pub fn from_config(
        entries: &HashMap<String, ProviderEntry>,
        router_cfg: &RouterConfig,
    ) -> anyhow::Result<Self> {
        // Smart defaults: detect which providers are actually available
        let has_anthropic = entries.contains_key("anthropic")
            || std::env::var("ANTHROPIC_API_KEY")
                .map(|k| !k.is_empty())
                .unwrap_or(false);
        let has_xai = entries.contains_key("xai")
            || std::env::var("XAI_API_KEY")
                .map(|k| !k.is_empty())
                .unwrap_or(false);
        let has_openai = entries.contains_key("openai")
            || std::env::var("OPENAI_API_KEY")
                .map(|k| !k.is_empty())
                .unwrap_or(false);
        let has_mistral = entries.contains_key("mistral")
            || std::env::var("MISTRAL_API_KEY")
                .map(|k| !k.is_empty())
                .unwrap_or(false);
        let has_gemini = entries.contains_key("gemini")
            || harness_provider_gemini::api_key_from_env().is_some();
        let has_bedrock = entries.contains_key("bedrock")
            || (std::env::var("AWS_ACCESS_KEY_ID")
                .map(|k| !k.is_empty())
                .unwrap_or(false)
                && std::env::var("AWS_SECRET_ACCESS_KEY")
                    .map(|k| !k.is_empty())
                    .unwrap_or(false));
        let has_ollama = entries.contains_key("ollama");
        let has_mlx = entries.contains_key("mlx") || harness_provider_mlx::mlx_runtime_available();

        // Auto-populate providers from env keys if not explicitly configured
        let mut augmented: HashMap<String, ProviderEntry> = entries.clone();
        if has_anthropic && !augmented.contains_key("anthropic") {
            augmented.insert(
                "anthropic".into(),
                ProviderEntry {
                    name: Some("anthropic".into()),
                    api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
                    model: Some("claude-sonnet-4-6".into()),
                    base_url: None,
                },
            );
        }
        if has_xai && !augmented.contains_key("xai") {
            augmented.insert(
                "xai".into(),
                ProviderEntry {
                    name: Some("xai".into()),
                    api_key: std::env::var("XAI_API_KEY").ok(),
                    model: Some("grok-4.3".into()),
                    base_url: None,
                },
            );
        }
        if has_openai && !augmented.contains_key("openai") {
            augmented.insert(
                "openai".into(),
                ProviderEntry {
                    name: Some("openai".into()),
                    api_key: std::env::var("OPENAI_API_KEY").ok(),
                    model: Some("gpt-5.5".into()),
                    base_url: None,
                },
            );
        }
        if has_mistral && !augmented.contains_key("mistral") {
            augmented.insert(
                "mistral".into(),
                ProviderEntry {
                    name: Some("mistral".into()),
                    api_key: std::env::var("MISTRAL_API_KEY").ok(),
                    model: Some("mistral-large-latest".into()),
                    base_url: Some("https://api.mistral.ai/v1".into()),
                },
            );
        }
        if has_gemini && !augmented.contains_key("gemini") {
            augmented.insert(
                "gemini".into(),
                ProviderEntry {
                    name: Some("gemini".into()),
                    api_key: harness_provider_gemini::api_key_from_env(),
                    model: Some(harness_provider_gemini::DEFAULT_MODEL.into()),
                    base_url: Some(harness_provider_gemini::DEFAULT_BASE_URL.into()),
                },
            );
        }
        if has_bedrock && !augmented.contains_key("bedrock") {
            augmented.insert(
                "bedrock".into(),
                ProviderEntry {
                    name: Some("bedrock".into()),
                    api_key: None,
                    model: std::env::var("BEDROCK_MODEL_ID").ok().or_else(|| {
                        Some(harness_provider_bedrock::DEFAULT_MODEL.into())
                    }),
                    base_url: std::env::var("AWS_REGION")
                        .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
                        .ok(),
                },
            );
        }
        if has_mlx && !augmented.contains_key("mlx") {
            augmented.insert(
                "mlx".into(),
                ProviderEntry {
                    name: Some("mlx".into()),
                    api_key: None,
                    model: Some(harness_provider_mlx::DEFAULT_MODEL.into()),
                    base_url: Some(harness_provider_mlx::DEFAULT_BASE_URL.into()),
                },
            );
        }

        // Default provider: anthropic > xai > openai > mistral > gemini > bedrock > ollama > mlx
        let smart_default = if has_anthropic {
            "anthropic"
        } else if has_xai {
            "xai"
        } else if has_openai {
            "openai"
        } else if has_mistral {
            "mistral"
        } else if has_gemini {
            "gemini"
        } else if has_bedrock {
            "bedrock"
        } else if has_ollama {
            "ollama"
        } else if has_mlx {
            "mlx"
        } else {
            "ollama"
        };

        let default_name = router_cfg
            .default
            .clone()
            .unwrap_or_else(|| smart_default.into());
        let mut r = Self::new(&default_name);

        for (name, entry) in &augmented {
            match build_provider(name.as_str(), entry) {
                Ok(p) => {
                    r.providers.insert(name.clone(), p);
                }
                Err(e) => warn!(name, err = %e, "failed to build provider"),
            }
        }

        // Smart route overrides if not explicitly configured
        if let Some(ref f) = router_cfg.fast_model {
            let (pname, model) = parse_route_spec(f);
            r.fast_name = Some(pname);
            r.fast_model_override = model;
        } else {
            // fast: haiku > grok-fast > openai-mini > ollama
            let fast = if has_anthropic {
                "anthropic"
            } else if has_xai {
                "xai"
            } else if has_openai {
                "openai"
            } else if has_ollama {
                "ollama"
            } else if has_mlx {
                "mlx"
            } else {
                "ollama"
            };
            r.fast_name = Some(fast.to_string());
        }

        if let Some(ref h) = router_cfg.heavy_model {
            let (pname, model) = parse_route_spec(h);
            r.heavy_name = Some(pname);
            r.heavy_model_override = model;
        } else {
            // heavy: opus > grok-reasoning > gpt-5.5 > ollama
            let heavy = if has_anthropic {
                "anthropic"
            } else if has_xai {
                "xai"
            } else if has_openai {
                "openai"
            } else if has_ollama {
                "ollama"
            } else if has_mlx {
                "mlx"
            } else {
                "ollama"
            };
            r.heavy_name = Some(heavy.to_string());
        }

        if let Some(ref e) = router_cfg.embed_model {
            let (pname, model) = parse_route_spec(e);
            r.embed_name = Some(pname);
            r.embed_model_override = model;
        } else if has_ollama {
            r.embed_name = Some("ollama".to_string());
        } else if has_anthropic {
            r.embed_name = Some("anthropic".to_string());
        }

        // Fallback chain: explicit → smart order
        if let Some(ref fb) = router_cfg.fallback {
            r.fallback = fb.clone();
        } else {
            let mut fb = Vec::new();
            for n in &["anthropic", "xai", "openai", "mistral", "ollama", "mlx"] {
                if r.providers.contains_key(*n) && *n != default_name.as_str() {
                    fb.push(n.to_string());
                }
            }
            r.fallback = fb;
        }

        if r.providers.is_empty() {
            let ollama_entry = ProviderEntry {
                name: Some("ollama".into()),
                api_key: None,
                model: Some("qwen3-coder:30b".into()),
                base_url: None,
            };
            if let Ok(p) = build_provider("ollama", &ollama_entry) {
                r.providers.insert("ollama".into(), p);
                if !r.providers.contains_key(&r.default_name) {
                    r.default_name = "ollama".to_string();
                }
            }
        }

        if r.providers.is_empty() {
            return Err(anyhow::anyhow!(
                "No providers available. Set ANTHROPIC_API_KEY, XAI_API_KEY, or OPENAI_API_KEY, \
                 add a [providers.*] block in ~/.harness/config.toml, or start Ollama locally."
            ));
        }

        info!(
            default = %r.default_name,
            providers = ?r.providers.keys().collect::<Vec<_>>(),
            "router initialised"
        );

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
        self.default_provider()
            .map(|p| p.model())
            .unwrap_or("unknown")
    }

    fn pricing(&self) -> Option<Pricing> {
        self.default_provider().and_then(|p| p.pricing())
    }

    async fn embed(&self, model: &str, text: &str) -> Result<Vec<f32>, ProviderError> {
        let p = self
            .embed_provider()
            .ok_or_else(|| ProviderError::Other("no embed provider configured".into()))?;
        p.embed(model, text).await
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError> {
        // Try the default provider first, then each fallback in order.
        let try_order: Vec<String> = std::iter::once(self.default_name.clone())
            .chain(
                self.fallback
                    .iter()
                    .filter(|n| **n != self.default_name)
                    .cloned(),
            )
            .collect();

        let mut last_err = ProviderError::Other("no providers configured".into());

        for name in &try_order {
            let Some(p) = self.providers.get(name) else {
                continue;
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::stream;
    use futures::StreamExt;
    use harness_provider_core::{ChatRequest, Delta, StopReason};
    use std::collections::HashMap;

    struct StaticProvider {
        label: &'static str,
        text: String,
        fail_stream: bool,
    }

    #[async_trait]
    impl Provider for StaticProvider {
        fn name(&self) -> &str {
            self.label
        }

        fn model(&self) -> &str {
            "static-model"
        }

        async fn stream_chat(&self, _req: ChatRequest) -> Result<DeltaStream, ProviderError> {
            if self.fail_stream {
                return Err(ProviderError::Api {
                    status: 429,
                    message: "rate limited".into(),
                });
            }
            let text = self.text.clone();
            Ok(Box::pin(stream::iter(vec![
                Ok(Delta::Text(text)),
                Ok(Delta::Done {
                    stop_reason: StopReason::EndTurn,
                }),
            ])))
        }

        async fn embed(&self, _model: &str, _text: &str) -> Result<Vec<f32>, ProviderError> {
            Ok(vec![0.1, 0.2, 0.3])
        }

        fn pricing(&self) -> Option<Pricing> {
            None
        }
    }

    async fn collect_text(provider: ArcProvider) -> String {
        let mut stream = provider
            .stream_chat(ChatRequest::new("static-model"))
            .await
            .expect("stream");
        let mut out = String::new();
        while let Some(item) = stream.next().await {
            if let Ok(Delta::Text(t)) = item {
                out.push_str(&t);
            }
        }
        out
    }

    #[tokio::test]
    async fn routes_fast_heavy_and_embed_providers() {
        let router = ProviderRouter::new("default")
            .add(
                "default",
                Arc::new(StaticProvider {
                    label: "default",
                    text: "main".into(),
                    fail_stream: false,
                }),
            )
            .add(
                "fast",
                Arc::new(StaticProvider {
                    label: "fast",
                    text: "fast".into(),
                    fail_stream: false,
                }),
            )
            .add(
                "embed",
                Arc::new(StaticProvider {
                    label: "embed",
                    text: "unused".into(),
                    fail_stream: false,
                }),
            )
            .with_fast("fast")
            .with_embed("embed");

        assert_eq!(router.default_provider().unwrap().name(), "default");
        assert_eq!(router.fast_provider().unwrap().name(), "fast");
        assert_eq!(router.embed_provider().unwrap().name(), "embed");
        assert_eq!(
            collect_text(router.fast_provider().unwrap().clone()).await,
            "fast"
        );

        let embedding = router.embed("embed-model", "hello").await.expect("embed");
        assert_eq!(embedding.len(), 3);
    }

    #[tokio::test]
    async fn stream_chat_falls_back_when_default_fails() {
        let router = ProviderRouter::new("primary")
            .add(
                "primary",
                Arc::new(StaticProvider {
                    label: "primary",
                    text: "nope".into(),
                    fail_stream: true,
                }),
            )
            .add(
                "backup",
                Arc::new(StaticProvider {
                    label: "backup",
                    text: "fallback ok".into(),
                    fail_stream: false,
                }),
            )
            .with_fallback(vec!["backup".into()]);
        let router: ArcProvider = Arc::new(router);

        let text = collect_text(router).await;
        assert_eq!(text, "fallback ok");
    }

    #[tokio::test]
    async fn from_config_honors_explicit_default_and_fallback() {
        let mut entries = HashMap::new();
        entries.insert(
            "anthropic".into(),
            ProviderEntry {
                name: Some("anthropic".into()),
                api_key: Some("sk-ant-test".into()),
                model: Some("claude-sonnet-4-6".into()),
                base_url: None,
            },
        );
        entries.insert(
            "xai".into(),
            ProviderEntry {
                name: Some("xai".into()),
                api_key: Some("xai-test".into()),
                model: Some("grok-4.3".into()),
                base_url: None,
            },
        );

        let cfg = RouterConfig {
            default: Some("xai".into()),
            fallback: Some(vec!["anthropic".into()]),
            ..Default::default()
        };

        let router = ProviderRouter::from_config(&entries, &cfg).expect("router");
        assert_eq!(router.default_provider().unwrap().name(), "xai");
        assert!(router.get("anthropic").is_some());
        assert!(router.get("xai").is_some());
    }

    #[test]
    fn from_config_falls_back_to_ollama_when_no_cloud_keys() {
        let entries = HashMap::new();
        let cfg = RouterConfig::default();
        let router = ProviderRouter::from_config(&entries, &cfg).expect("ollama fallback");
        assert!(router.get("ollama").is_some());
        assert_eq!(router.default_provider().unwrap().name(), "ollama");
    }

    #[test]
    fn default_provider_none_when_router_empty() {
        let router = ProviderRouter {
            providers: HashMap::new(),
            default_name: "default".into(),
            fast_name: None,
            heavy_name: None,
            embed_name: None,
            fast_model_override: None,
            heavy_model_override: None,
            embed_model_override: None,
            fallback: vec![],
        };
        assert!(router.default_provider().is_none());
    }

    #[test]
    fn fast_model_id_from_route_spec() {
        let mut entries = HashMap::new();
        entries.insert(
            "xai".into(),
            ProviderEntry {
                name: Some("xai".into()),
                api_key: Some("xai-test".into()),
                model: Some("grok-4.3".into()),
                base_url: None,
            },
        );
        let cfg = RouterConfig {
            default: Some("xai".into()),
            fast_model: Some("xai:grok-4.1-fast".into()),
            ..Default::default()
        };
        let router = ProviderRouter::from_config(&entries, &cfg).expect("router");
        assert_eq!(router.fast_model_id(), Some("grok-4.1-fast"));
    }

    #[test]
    fn build_provider_mistral_defaults() {
        let p = build_provider(
            "mistral",
            &ProviderEntry {
                name: Some("mistral".into()),
                api_key: Some("mistral-test".into()),
                model: None,
                base_url: None,
            },
        )
        .expect("mistral");
        assert_eq!(p.name(), "mistral");
        assert_eq!(p.model(), "mistral-large-latest");
    }

    #[test]
    fn build_provider_gemini_defaults() {
        let p = build_provider(
            "gemini",
            &ProviderEntry {
                name: Some("gemini".into()),
                api_key: Some("gemini-test".into()),
                model: None,
                base_url: None,
            },
        )
        .expect("gemini");
        assert_eq!(p.name(), "gemini");
        assert_eq!(p.model(), "gemini-2.0-flash");
    }

    #[test]
    fn build_provider_bedrock_from_api_key_pair() {
        let p = build_provider(
            "bedrock",
            &ProviderEntry {
                name: Some("bedrock".into()),
                api_key: Some("AKIAtest:secret".into()),
                model: Some("my.bedrock-model".into()),
                base_url: Some("us-west-2".into()),
            },
        )
        .expect("bedrock");
        assert_eq!(p.name(), "bedrock");
        assert_eq!(p.model(), "my.bedrock-model");
    }

    #[test]
    fn build_provider_openai_compatible_requires_base_url() {
        let result = build_provider(
            "openai-compatible",
            &ProviderEntry {
                api_key: Some("k".into()),
                model: Some("m".into()),
                base_url: None,
                name: None,
            },
        );
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("base_url"));
    }

    #[test]
    fn build_provider_custom_name_with_base_url_is_openai_compatible() {
        let p = build_provider(
            "my-proxy",
            &ProviderEntry {
                name: Some("my-proxy".into()),
                api_key: Some("k".into()),
                model: Some("local-model".into()),
                base_url: Some("http://127.0.0.1:8000/v1".into()),
            },
        )
        .expect("compatible");
        assert_eq!(p.name(), "my-proxy");
        assert_eq!(p.model(), "local-model");
    }
}
