//! Provider error types returned from streaming and embedding calls.

use thiserror::Error;

/// Errors surfaced by provider implementations.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// JSON serialization or deserialization failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// The response stream ended before a terminal `Done` delta.
    #[error("Stream ended unexpectedly")]
    StreamEnded,
    /// Remote API returned a non-success HTTP status.
    #[error("API error {status}: {message}")]
    Api {
        /// HTTP status code.
        status: u16,
        /// Error message body.
        message: String,
    },
    /// The provider does not implement the requested operation (e.g. embed).
    #[error("Unsupported operation: {0}")]
    Unsupported(String),
    /// Other provider failure.
    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for ProviderError {
    fn from(e: anyhow::Error) -> Self {
        ProviderError::Other(e.to_string())
    }
}
