//! Provider router for Harness.
//!
//! Selects from multiple configured providers based on the user's saved route,
//! with ordered fallback on provider errors.
//!
//! # Usage
//!
//! ```toml
//! [providers.anthropic]
//! model = "claude-sonnet-4-6"
//!
//! [providers.xai]
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
//! default = "anthropic"
//! fast_model = "xai:grok-4.1-fast"
//! heavy_model = "anthropic:claude-opus-4-7"
//! embed_model = "ollama:nomic-embed-text"
//! fallback = ["xai", "ollama"]
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
    /// Provider adapter. Usually omitted because the table name selects it.
    /// Set to `openai-compatible` for a custom OpenAI-format endpoint.
    pub kind: Option<String>,
    /// Deprecated adapter alias retained for existing configuration files.
    pub name: Option<String>,
    pub api_key: Option<String>,
    /// Environment variable containing the API key. Prefer this over storing a key.
    pub api_key_env: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
}

/// Metadata for built-in provider adapters and OpenAI-compatible presets.
/// The order is alphabetical for display only and is never used for routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPreset {
    pub name: &'static str,
    pub kind: &'static str,
    pub api_key_envs: &'static [&'static str],
    pub base_url: Option<&'static str>,
    pub local: bool,
}

pub const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "anthropic",
        kind: "anthropic",
        api_key_envs: &["ANTHROPIC_API_KEY"],
        base_url: None,
        local: false,
    },
    ProviderPreset {
        name: "bedrock",
        kind: "bedrock",
        api_key_envs: &["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"],
        base_url: None,
        local: false,
    },
    ProviderPreset {
        name: "cerebras",
        kind: "openai-compatible",
        api_key_envs: &["CEREBRAS_API_KEY"],
        base_url: Some("https://api.cerebras.ai/v1"),
        local: false,
    },
    ProviderPreset {
        name: "deepseek",
        kind: "openai-compatible",
        api_key_envs: &["DEEPSEEK_API_KEY"],
        base_url: Some("https://api.deepseek.com"),
        local: false,
    },
    ProviderPreset {
        name: "fireworks",
        kind: "openai-compatible",
        api_key_envs: &["FIREWORKS_API_KEY"],
        base_url: Some("https://api.fireworks.ai/inference/v1"),
        local: false,
    },
    ProviderPreset {
        name: "gemini",
        kind: "gemini",
        api_key_envs: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        base_url: None,
        local: false,
    },
    ProviderPreset {
        name: "groq",
        kind: "openai-compatible",
        api_key_envs: &["GROQ_API_KEY"],
        base_url: Some("https://api.groq.com/openai/v1"),
        local: false,
    },
    ProviderPreset {
        name: "huggingface",
        kind: "openai-compatible",
        api_key_envs: &["HF_TOKEN"],
        base_url: Some("https://router.huggingface.co/v1"),
        local: false,
    },
    ProviderPreset {
        name: "mistral",
        kind: "mistral",
        api_key_envs: &["MISTRAL_API_KEY"],
        base_url: Some("https://api.mistral.ai/v1"),
        local: false,
    },
    ProviderPreset {
        name: "mlx",
        kind: "mlx",
        api_key_envs: &[],
        base_url: None,
        local: true,
    },
    ProviderPreset {
        name: "nvidia",
        kind: "openai-compatible",
        api_key_envs: &["NVIDIA_API_KEY"],
        base_url: Some("https://integrate.api.nvidia.com/v1"),
        local: false,
    },
    ProviderPreset {
        name: "ollama",
        kind: "ollama",
        api_key_envs: &[],
        base_url: None,
        local: true,
    },
    ProviderPreset {
        name: "openai",
        kind: "openai",
        api_key_envs: &["OPENAI_API_KEY"],
        base_url: None,
        local: false,
    },
    ProviderPreset {
        name: "openrouter",
        kind: "openai-compatible",
        api_key_envs: &["OPENROUTER_API_KEY"],
        base_url: Some("https://openrouter.ai/api/v1"),
        local: false,
    },
    ProviderPreset {
        name: "perplexity",
        kind: "openai-compatible",
        api_key_envs: &["PERPLEXITY_API_KEY"],
        base_url: Some("https://api.perplexity.ai"),
        local: false,
    },
    ProviderPreset {
        name: "sambanova",
        kind: "openai-compatible",
        api_key_envs: &["SAMBANOVA_API_KEY"],
        base_url: Some("https://api.sambanova.ai/v1"),
        local: false,
    },
    ProviderPreset {
        name: "together",
        kind: "openai-compatible",
        api_key_envs: &["TOGETHER_API_KEY"],
        base_url: Some("https://api.together.ai/v1"),
        local: false,
    },
    ProviderPreset {
        name: "xai",
        kind: "xai",
        api_key_envs: &["XAI_API_KEY"],
        base_url: None,
        local: false,
    },
];

pub fn provider_preset(name: &str) -> Option<&'static ProviderPreset> {
    PROVIDER_PRESETS.iter().find(|preset| preset.name == name)
}

/// Fill adapter, key-environment, and base URL metadata without choosing a model.
pub fn configured_provider_entry(name: &str) -> ProviderEntry {
    let Some(preset) = provider_preset(name) else {
        return ProviderEntry::default();
    };
    ProviderEntry {
        kind: (preset.kind != preset.name).then(|| preset.kind.to_string()),
        api_key_env: preset
            .api_key_envs
            .first()
            .map(|value| (*value).to_string()),
        base_url: preset.base_url.map(str::to_string),
        ..ProviderEntry::default()
    }
}

pub fn provider_key_environment(name: &str, entry: &ProviderEntry) -> Option<String> {
    entry.api_key_env.clone().or_else(|| {
        provider_preset(name)
            .and_then(|preset| preset.api_key_envs.first())
            .map(|value| (*value).to_string())
    })
}

fn nonempty_environment(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn resolved_api_key(name: &str, entry: &ProviderEntry) -> String {
    entry
        .api_key
        .clone()
        .filter(|value| !value.is_empty())
        .or_else(|| entry.api_key_env.as_deref().and_then(nonempty_environment))
        .or_else(|| {
            provider_preset(name).and_then(|preset| {
                preset
                    .api_key_envs
                    .iter()
                    .find_map(|env_name| nonempty_environment(env_name))
            })
        })
        .unwrap_or_default()
}

/// Build an `ArcProvider` from a `ProviderEntry`.
///
/// `name` is the `[providers.<name>]` table key. `entry.kind` optionally selects
/// an adapter for a custom name.
/// Supported kinds: `anthropic`, `xai`, `openai`, `mistral`, `gemini`, `bedrock`,
/// `openai-compatible` / `compatible`, `ollama`, `mlx`. Built-in hosted presets
/// use the OpenAI-compatible adapter. Any other name with `base_url` is also
/// treated as OpenAI-compatible; unknown names without an adapter are rejected.
pub fn build_provider(name: &str, entry: &ProviderEntry) -> anyhow::Result<ArcProvider> {
    let preset = provider_preset(name);
    let kind = entry
        .kind
        .as_deref()
        .or(entry.name.as_deref())
        .or_else(|| preset.map(|value| value.kind))
        .unwrap_or(name);
    let api_key = resolved_api_key(name, entry);
    let base_url = entry
        .base_url
        .as_deref()
        .or_else(|| preset.and_then(|value| value.base_url));
    let model = entry
        .model
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("provider '{name}' requires an explicit model"))?;

    match kind {
        "anthropic" => {
            let cfg = harness_provider_anthropic::AnthropicConfig::new(&api_key).with_model(model);
            Ok(Arc::new(
                harness_provider_anthropic::AnthropicProvider::new(cfg)?,
            ))
        }
        "openai" => {
            let mut cfg = harness_provider_openai::OpenAIConfig::new(&api_key).with_model(model);
            if let Some(u) = base_url {
                cfg = cfg.with_base_url(u);
            }
            Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?))
        }
        "mistral" => {
            let mut cfg = harness_provider_openai::OpenAIConfig::mistral(api_key).with_model(model);
            if let Some(u) = base_url {
                cfg = cfg.with_base_url(u);
            }
            Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?))
        }
        "gemini" => {
            let mut cfg = harness_provider_openai::OpenAIConfig::gemini(api_key).with_model(model);
            if let Some(u) = base_url {
                cfg = cfg.with_base_url(u);
            }
            Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?))
        }
        "bedrock" => {
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
                            model: model.to_string(),
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
            harness_provider_bedrock::build_arc(Some(model.to_string()), region)
        }
        "openai-compatible" | "compatible" => {
            let base = base_url.filter(|s| !s.is_empty()).ok_or_else(|| {
                anyhow::anyhow!("provider '{name}' requires base_url (OpenAI-format endpoint)")
            })?;
            let cfg = harness_provider_openai::OpenAIConfig::new(&api_key)
                .with_provider_name(name)
                .with_base_url(base)
                .with_model(model);
            Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?))
        }
        "ollama" => {
            let mut cfg = harness_provider_ollama::OllamaConfig::new(model);
            if let Some(u) = base_url {
                cfg = cfg.with_base_url(u);
            }
            Ok(Arc::new(harness_provider_ollama::OllamaProvider::new(cfg)?))
        }
        "mlx" => harness_provider_mlx::build_arc(Some(model.to_string()), entry.base_url.clone()),
        "xai" => {
            let cfg = harness_provider_xai::XaiConfig::new(&api_key).with_model(model);
            Ok(Arc::new(harness_provider_xai::XaiProvider::new(cfg)?))
        }
        _ => {
            // Custom table name with base_url → OpenAI-compatible under that name.
            if let Some(base) = base_url.filter(|s| !s.is_empty()) {
                let cfg = harness_provider_openai::OpenAIConfig::new(&api_key)
                    .with_provider_name(name)
                    .with_base_url(base)
                    .with_model(model);
                return Ok(Arc::new(harness_provider_openai::OpenAIProvider::new(cfg)?));
            }
            anyhow::bail!(
                "unknown provider '{name}'; set kind = \"openai-compatible\" and base_url, or choose a built-in provider"
            )
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

#[derive(Debug, Clone, Default)]
struct ProviderEnvironment {
    anthropic_api_key: Option<String>,
    xai_api_key: Option<String>,
    openai_api_key: Option<String>,
    mistral_api_key: Option<String>,
    gemini_api_key: Option<String>,
    aws_access_key_id: Option<String>,
    aws_secret_access_key: Option<String>,
    bedrock_model_id: Option<String>,
    aws_region: Option<String>,
    mlx_runtime_available: bool,
    compatible_api_keys: HashMap<String, String>,
}

impl ProviderEnvironment {
    fn detect() -> Self {
        fn nonempty(name: &str) -> Option<String> {
            std::env::var(name).ok().filter(|value| !value.is_empty())
        }

        let compatible_api_keys = [
            ("cerebras", "CEREBRAS_API_KEY"),
            ("deepseek", "DEEPSEEK_API_KEY"),
            ("fireworks", "FIREWORKS_API_KEY"),
            ("groq", "GROQ_API_KEY"),
            ("huggingface", "HF_TOKEN"),
            ("nvidia", "NVIDIA_API_KEY"),
            ("openrouter", "OPENROUTER_API_KEY"),
            ("perplexity", "PERPLEXITY_API_KEY"),
            ("sambanova", "SAMBANOVA_API_KEY"),
            ("together", "TOGETHER_API_KEY"),
        ]
        .into_iter()
        .filter_map(|(provider, env_name)| {
            nonempty(env_name).map(|key| (provider.to_string(), key))
        })
        .collect();

        Self {
            anthropic_api_key: nonempty("ANTHROPIC_API_KEY"),
            xai_api_key: nonempty("XAI_API_KEY"),
            openai_api_key: nonempty("OPENAI_API_KEY"),
            mistral_api_key: nonempty("MISTRAL_API_KEY"),
            gemini_api_key: harness_provider_gemini::api_key_from_env(),
            aws_access_key_id: nonempty("AWS_ACCESS_KEY_ID"),
            aws_secret_access_key: nonempty("AWS_SECRET_ACCESS_KEY"),
            bedrock_model_id: nonempty("BEDROCK_MODEL_ID"),
            aws_region: nonempty("AWS_REGION").or_else(|| nonempty("AWS_DEFAULT_REGION")),
            mlx_runtime_available: harness_provider_mlx::mlx_runtime_available(),
            compatible_api_keys,
        }
    }

    fn provider_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for (name, available) in [
            ("anthropic", self.anthropic_api_key.is_some()),
            (
                "bedrock",
                self.aws_access_key_id.is_some() && self.aws_secret_access_key.is_some(),
            ),
            ("gemini", self.gemini_api_key.is_some()),
            ("mistral", self.mistral_api_key.is_some()),
            ("openai", self.openai_api_key.is_some()),
            ("xai", self.xai_api_key.is_some()),
            ("mlx", self.mlx_runtime_available),
        ] {
            if available {
                names.push(name.to_string());
            }
        }
        names.extend(self.compatible_api_keys.keys().cloned());
        names.sort();
        names
    }

    fn entry(&self, name: &str) -> Option<ProviderEntry> {
        let mut entry = configured_provider_entry(name);
        entry.api_key = match name {
            "anthropic" => self.anthropic_api_key.clone(),
            "xai" => self.xai_api_key.clone(),
            "openai" => self.openai_api_key.clone(),
            "mistral" => self.mistral_api_key.clone(),
            "gemini" => self.gemini_api_key.clone(),
            "bedrock" => None,
            "mlx" if self.mlx_runtime_available => None,
            other => self.compatible_api_keys.get(other).cloned(),
        };
        if name == "bedrock" {
            if self.aws_access_key_id.is_none() || self.aws_secret_access_key.is_none() {
                return None;
            }
            entry.model = self.bedrock_model_id.clone();
            entry.base_url = self.aws_region.clone();
        } else if (name == "mlx" && !self.mlx_runtime_available)
            || (!provider_preset(name).is_some_and(|preset| preset.local)
                && entry.api_key.is_none())
        {
            return None;
        }
        Some(entry)
    }
}

fn configured_entry_for_selected_provider(
    name: &str,
    environment: &ProviderEnvironment,
) -> Option<ProviderEntry> {
    environment.entry(name).or_else(|| {
        provider_preset(name)
            .filter(|preset| preset.local)
            .map(|_| configured_provider_entry(name))
    })
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

    /// Return a reference to a named provider.
    pub fn get(&self, name: &str) -> Option<&ArcProvider> {
        self.providers.get(name)
    }

    pub fn default_provider(&self) -> Option<&ArcProvider> {
        self.providers.get(&self.default_name)
    }

    /// Exact configured route: primary first, followed by fallbacks.
    pub fn route_names(&self) -> Vec<&str> {
        std::iter::once(self.default_name.as_str())
            .chain(self.fallback.iter().map(String::as_str))
            .collect()
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

    /// Build from named provider entries and an explicit routing policy.
    ///
    /// The first provider is `[router].default`; `[router].fallback` is tried in
    /// the exact saved order. When no route exists, exactly one configured or
    /// environment-detected provider is accepted as an unambiguous legacy case.
    /// Multiple candidates require the user to save a route.
    pub fn from_config(
        entries: &HashMap<String, ProviderEntry>,
        router_cfg: &RouterConfig,
    ) -> anyhow::Result<Self> {
        Self::from_config_with_environment(entries, router_cfg, &ProviderEnvironment::detect())
    }

    fn from_config_with_environment(
        entries: &HashMap<String, ProviderEntry>,
        router_cfg: &RouterConfig,
        environment: &ProviderEnvironment,
    ) -> anyhow::Result<Self> {
        let mut available = entries.clone();
        for name in environment.provider_names() {
            if let Some(entry) = environment.entry(&name) {
                available.entry(name).or_insert(entry);
            }
        }

        let default_name = if let Some(name) = router_cfg.default.as_deref() {
            name.trim().to_lowercase()
        } else {
            let mut candidates: Vec<String> = available.keys().cloned().collect();
            candidates.sort();
            candidates.dedup();
            match candidates.as_slice() {
                [only] => only.clone(),
                [] => anyhow::bail!(
                    "no provider route configured; run `harness setup` or `harness route set <provider:model> ...`"
                ),
                _ => anyhow::bail!(
                    "multiple providers are available but no route is configured ({}); run `harness setup` or `harness route set <provider:model> ...`",
                    candidates.join(", ")
                ),
            }
        };

        let fallback = router_cfg.fallback.clone().unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        seen.insert(default_name.clone());
        for name in &fallback {
            if !seen.insert(name.clone()) {
                anyhow::bail!("provider route contains duplicate entry '{name}'");
            }
        }

        let mut required = vec![default_name.clone()];
        required.extend(fallback.iter().cloned());
        for spec in [
            router_cfg.fast_model.as_deref(),
            router_cfg.heavy_model.as_deref(),
            router_cfg.embed_model.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            let (name, _) = parse_route_spec(spec);
            if !required.contains(&name) {
                required.push(name);
            }
        }

        let mut r = Self::new(&default_name).with_fallback(fallback);
        for name in required {
            let entry = available
                .get(&name)
                .cloned()
                .or_else(|| configured_entry_for_selected_provider(&name, environment))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "provider '{name}' is in the saved route but is not configured; add [providers.{name}] or rerun `harness setup`"
                    )
                })?;
            if entry.model.as_deref().unwrap_or("").is_empty() {
                anyhow::bail!(
                    "provider '{name}' is selected but has no model; run `harness setup` or `harness route model {name} <model>`"
                );
            }
            let provider = build_provider(&name, &entry).map_err(|error| {
                anyhow::anyhow!("failed to build selected provider '{name}': {error}")
            })?;
            r.providers.insert(name, provider);
        }

        if let Some(ref spec) = router_cfg.fast_model {
            let (name, model) = parse_route_spec(spec);
            r.fast_name = Some(name);
            r.fast_model_override = model;
        }
        if let Some(ref spec) = router_cfg.heavy_model {
            let (name, model) = parse_route_spec(spec);
            r.heavy_name = Some(name);
            r.heavy_model_override = model;
        }
        if let Some(ref spec) = router_cfg.embed_model {
            let (name, model) = parse_route_spec(spec);
            r.embed_name = Some(name);
            r.embed_model_override = model;
        }

        info!(
            default = %r.default_name,
            route = ?r.route_names(),
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
                ..ProviderEntry::default()
            },
        );
        entries.insert(
            "xai".into(),
            ProviderEntry {
                name: Some("xai".into()),
                api_key: Some("xai-test".into()),
                model: Some("grok-4.3".into()),
                base_url: None,
                ..ProviderEntry::default()
            },
        );

        let cfg = RouterConfig {
            default: Some("xai".into()),
            fallback: Some(vec!["anthropic".into()]),
            ..Default::default()
        };

        let router = ProviderRouter::from_config(&entries, &cfg).expect("router");
        assert_eq!(router.default_provider().unwrap().name(), "xai");
        assert_eq!(router.route_names(), ["xai", "anthropic"]);
        assert!(router.get("anthropic").is_some());
        assert!(router.get("xai").is_some());
    }

    #[test]
    fn from_config_requires_a_route_when_no_provider_is_available() {
        let entries = HashMap::new();
        let cfg = RouterConfig::default();
        let error = ProviderRouter::from_config_with_environment(
            &entries,
            &cfg,
            &ProviderEnvironment::default(),
        )
        .err()
        .expect("missing route must fail");
        assert!(error.to_string().contains("no provider route configured"));
    }

    #[test]
    fn from_config_rejects_ambiguous_provider_candidates() {
        let entries = HashMap::from([
            (
                "ollama".to_string(),
                ProviderEntry {
                    model: Some("qwen".into()),
                    ..ProviderEntry::default()
                },
            ),
            (
                "mlx".to_string(),
                ProviderEntry {
                    model: Some("mlx-model".into()),
                    ..ProviderEntry::default()
                },
            ),
        ]);
        let error = ProviderRouter::from_config_with_environment(
            &entries,
            &RouterConfig::default(),
            &ProviderEnvironment::default(),
        )
        .err()
        .expect("ambiguous route must fail");
        assert!(error
            .to_string()
            .contains("multiple providers are available"));
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
                ..ProviderEntry::default()
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
    fn build_provider_requires_explicit_model() {
        let error = build_provider(
            "mistral",
            &ProviderEntry {
                name: Some("mistral".into()),
                api_key: Some("mistral-test".into()),
                model: None,
                base_url: None,
                ..ProviderEntry::default()
            },
        )
        .err()
        .expect("missing model must fail");
        assert!(error.to_string().contains("explicit model"));
    }

    #[test]
    fn build_provider_uses_explicit_model() {
        let provider = build_provider(
            "gemini",
            &ProviderEntry {
                api_key: Some("gemini-test".into()),
                model: Some("user-selected-model".into()),
                ..ProviderEntry::default()
            },
        )
        .expect("gemini");
        assert_eq!(provider.model(), "user-selected-model");
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
                ..ProviderEntry::default()
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
                ..ProviderEntry::default()
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
                ..ProviderEntry::default()
            },
        )
        .expect("compatible");
        assert_eq!(p.name(), "my-proxy");
        assert_eq!(p.model(), "local-model");
    }

    #[test]
    fn provider_presets_are_alphabetical_and_unique() {
        let names: Vec<&str> = PROVIDER_PRESETS.iter().map(|preset| preset.name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names, sorted);
    }

    #[test]
    fn compatible_preset_seeds_transport_without_choosing_model() {
        let entry = configured_provider_entry("nvidia");
        assert_eq!(entry.kind.as_deref(), Some("openai-compatible"));
        assert_eq!(entry.api_key_env.as_deref(), Some("NVIDIA_API_KEY"));
        assert_eq!(
            entry.base_url.as_deref(),
            Some("https://integrate.api.nvidia.com/v1")
        );
        assert!(entry.model.is_none());
    }

    #[test]
    fn unknown_provider_without_adapter_is_rejected() {
        let error = build_provider(
            "mystery",
            &ProviderEntry {
                model: Some("m".into()),
                ..ProviderEntry::default()
            },
        )
        .err()
        .expect("unknown provider must fail");
        assert!(error.to_string().contains("unknown provider"));
    }
}
