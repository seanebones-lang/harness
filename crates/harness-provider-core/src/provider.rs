use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;
use crate::{ChatRequest, Delta, ProviderError};

pub type DeltaStream = Pin<Box<dyn Stream<Item = Result<Delta, ProviderError>> + Send>>;

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;

    /// Stream a chat completion. Yields `Delta` items until `Delta::Done`.
    async fn stream_chat(&self, req: ChatRequest) -> Result<DeltaStream, ProviderError>;
}
