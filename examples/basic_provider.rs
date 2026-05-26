//! Basic provider usage example.
//!
//! Streams a chat completion using `harness-provider-ollama`. No API key
//! required — Ollama must be running locally with a model pulled.
//!
//! # Cargo.toml snippet
//!
//! ```toml
//! [dependencies]
//! harness-provider-core = { path = "../crates/harness-provider-core" }
//! harness-provider-ollama = { path = "../crates/harness-provider-ollama" }
//! futures = "0.3"
//! tokio = { version = "1", features = ["full"] }
//! anyhow = "1"
//! ```
//!
//! # Running
//!
//! ```bash
//! # Start Ollama: ollama serve
//! # Pull a model: ollama pull llama3.2:3b
//! cargo run --example basic_provider
//! ```

use anyhow::Result;
use futures::StreamExt;
use harness_provider_core::{ChatRequest, Delta, Message, MessageContent, Provider, Role};
use harness_provider_ollama::{OllamaConfig, OllamaProvider};

#[tokio::main]
async fn main() -> Result<()> {
    let provider = OllamaProvider::new(OllamaConfig::new("llama3.2:3b"))?;

    let request = ChatRequest::new("llama3.2:3b")
        .with_system("You are a concise Rust coding assistant.")
        .with_messages(vec![Message {
            role: Role::User,
            content: MessageContent::Text(
                "In one sentence, what is the borrow checker?".to_string(),
            ),
            tool_call_id: None,
        }]);

    println!("Streaming from Ollama (llama3.2:3b)...\n");

    let mut stream = provider.stream_chat(request).await?;
    let mut full_response = String::new();

    while let Some(delta) = stream.next().await {
        match delta? {
            Delta::Text(chunk) => {
                print!("{chunk}");
                full_response.push_str(&chunk);
            }
            Delta::ToolCall(tc) => {
                println!("\n[Tool call: {}]", tc.function.name);
            }
            Delta::Done { stop_reason } => {
                println!("\n\n[Done — {stop_reason:?}]");
                break;
            }
            Delta::Usage {
                input_tokens,
                output_tokens,
            } => {
                println!("[Usage — in: {input_tokens}, out: {output_tokens}]");
            }
            Delta::CacheUsage {
                cache_creation_tokens,
                cache_read_tokens,
            } => {
                println!("[Cache — created: {cache_creation_tokens}, read: {cache_read_tokens}]");
            }
        }
    }

    println!("\nFull response:\n{full_response}");
    Ok(())
}
