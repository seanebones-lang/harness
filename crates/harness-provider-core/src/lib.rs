//! Core types and traits shared by Harness LLM providers.
#![deny(missing_docs)]

pub mod error;
/// Provider trait and streaming types.
pub mod provider;
/// Chat messages, tools, and request types.
pub mod types;

pub use error::ProviderError;
pub use provider::{ArcProvider, DeltaStream, Pricing, Provider};
pub use types::*;
