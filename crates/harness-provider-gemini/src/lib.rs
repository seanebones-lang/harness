//! Google Gemini provider via the OpenAI-compatible Generative Language endpoint.
//!
//! Thin wrapper around [`harness_provider_openai::OpenAIProvider`]. Prefer building
//! through `harness_provider_router::build_provider("gemini", …)` or
//! [`OpenAIConfig::gemini`](harness_provider_openai::OpenAIConfig::gemini).

use harness_provider_core::ArcProvider;
use harness_provider_openai::{OpenAIConfig, OpenAIProvider};

/// Default chat model.
pub const DEFAULT_MODEL: &str = "gemini-2.0-flash";
/// OpenAI-compatible Gemini base URL.
pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";

/// Build an [`ArcProvider`] for Gemini.
pub fn build_arc(api_key: impl Into<String>, model: Option<String>) -> anyhow::Result<ArcProvider> {
    let mut cfg = OpenAIConfig::gemini(api_key);
    if let Some(m) = model {
        cfg = cfg.with_model(m);
    }
    Ok(std::sync::Arc::new(OpenAIProvider::new(cfg)?))
}

/// Resolve API key from env (`GEMINI_API_KEY` then `GOOGLE_API_KEY`).
pub fn api_key_from_env() -> Option<String> {
    std::env::var("GEMINI_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .or_else(|| {
            std::env::var("GOOGLE_API_KEY")
                .ok()
                .filter(|k| !k.is_empty())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_config_defaults() {
        let cfg = OpenAIConfig::gemini("test-key");
        assert_eq!(cfg.provider_name, "gemini");
        assert_eq!(cfg.model, DEFAULT_MODEL);
        assert!(cfg.base_url.contains("generativelanguage.googleapis.com"));
        assert_eq!(cfg.api_key, "test-key");
    }

    #[test]
    fn build_arc_ok() {
        let p = build_arc("k", Some("gemini-1.5-pro".into())).expect("build");
        assert_eq!(p.name(), "gemini");
        assert_eq!(p.model(), "gemini-1.5-pro");
    }
}
