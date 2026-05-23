use crate::{ChatRequest, Delta, ProviderError};
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;

/// Stream of provider deltas or errors emitted during chat completion.
pub type DeltaStream = Pin<Box<dyn Stream<Item = Result<Delta, ProviderError>> + Send>>;

/// Per-million-token pricing for a provider/model pair.
#[derive(Debug, Clone, Copy)]
pub struct Pricing {
    /// USD per million input tokens.
    pub input_per_m_usd: f64,
    /// Price per million cache-hit (prompt-cached) input tokens. 0.0 = no caching.
    pub cached_input_per_m_usd: f64,
    /// USD per million output tokens.
    pub output_per_m_usd: f64,
}

/// Thread-safe, cheaply clonable provider handle.
pub type ArcProvider = Arc<dyn Provider + Send + Sync>;

/// LLM backend implemented by Anthropic, OpenAI, xAI, Ollama, and others.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Short provider id (e.g. `"anthropic"`, `"openai"`).
    fn name(&self) -> &str;
    /// Default chat model id for this provider instance.
    fn model(&self) -> &str;

    /// Stream a chat completion. Yields `Delta` items until `Delta::Done`.
    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError>;

    /// Compute text embeddings. Returns a float vector.
    /// Default: returns `Err(ProviderError::Unsupported)`.
    async fn embed(&self, _model: &str, _text: &str) -> Result<Vec<f32>, ProviderError> {
        Err(ProviderError::Unsupported(format!(
            "{} does not support embeddings",
            self.name()
        )))
    }

    /// Return per-million-token pricing for the current model, if known.
    fn pricing(&self) -> Option<Pricing> {
        None
    }
}
