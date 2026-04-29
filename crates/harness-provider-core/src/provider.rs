use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;
use std::sync::Arc;
use crate::{ChatRequest, Delta, ProviderError};

pub type DeltaStream = Pin<Box<dyn Stream<Item = Result<Delta, ProviderError>> + Send>>;

/// Per-million-token pricing for a provider/model pair.
#[derive(Debug, Clone, Copy)]
pub struct Pricing {
    pub input_per_m_usd: f64,
    pub output_per_m_usd: f64,
}

/// `Arc<dyn Provider>` — cheaply clonable, thread-safe provider handle.
pub type ArcProvider = Arc<dyn Provider + Send + Sync>;

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;

    /// Stream a chat completion. Yields `Delta` items until `Delta::Done`.
    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError>;

    /// Compute text embeddings. Returns a float vector.
    /// Default: returns `Err(ProviderError::Unsupported)`.
    async fn embed(&self, _model: &str, _text: &str) -> Result<Vec<f32>, ProviderError> {
        Err(ProviderError::Unsupported(
            format!("{} does not support embeddings", self.name())
        ))
    }

    /// Return per-million-token pricing for the current model, if known.
    fn pricing(&self) -> Option<Pricing> {
        None
    }
}
