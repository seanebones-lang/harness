use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Stream ended unexpectedly")]
    StreamEnded,
    #[error("API error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for ProviderError {
    fn from(e: anyhow::Error) -> Self {
        ProviderError::Other(e.to_string())
    }
}
