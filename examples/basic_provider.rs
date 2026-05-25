//! Basic provider usage example.
//!
//! Shows how to stream a chat completion using `harness-provider-core` and
//! `harness-provider-ollama`. No API key is required — Ollama must be running
//! locally with a model pulled (e.g. `ollama pull llama3.2:3b`).
//!
//! # Cargo.toml snippet
//!
//! ```toml
//! [dependencies]
//! harness-provider-core = { path = "../crates/harness-provider-core" }
//! harness-provider-ollama = { path = "../crates/harness-provider-ollama" }
//! tokio = { version = "1", features = ["full"] }
//! anyhow = "1"
//! futures = "0.3"
//! ```
//!
//! # Running
//!
//! ```bash
//! # Start Ollama in another terminal: ollama serve
//! # Pull a model: ollama pull llama3.2:3b
//! cargo run --example basic_provider
//! ```

use anyhow::Result;
use futures::StreamExt;
use harness_provider_core::{ChatRequest, Delta, Message, Role};
use harness_provider_ollama::OllamaProvider;

#[tokio::main]
async fn main() -> Result<()> {
    // Build the provider. OllamaProvider connects to http://localhost:11434 by default.
    // Override with OllamaProvider::with_base_url("http://...") if needed.
    let provider = OllamaProvider::new("llama3.2:3b");

    // Build a chat request. The builder pattern lets you add messages, tools,
    // a system prompt, and optional flags (thinking budget, response schema, etc.).
    let request = ChatRequest::builder()
        .system("You are a concise Rust coding assistant.")
        .message(Message {
            role: Role::User,
            content: "In one sentence, what is the borrow checker?".into(),
            tool_call_id: None,
        })
        .build();

    println!("Sending request to Ollama (llama3.2:3b)...\n");

    // Stream the response. `stream_chat` returns an async stream of `Delta` values.
    let mut stream = provider.stream_chat(request).await;

    let mut full_response = String::new();

    while let Some(delta) = stream.next().await {
        match delta? {
            Delta::Text(chunk) => {
                // Print each text chunk as it arrives (streaming output).
                print!("{chunk}");
                full_response.push_str(&chunk);
            }
            Delta::ToolCall(tc) => {
                // This example has no tools registered, so this branch won't be reached.
                println!("\n[Tool call requested: {}]", tc.function.name);
            }
            Delta::Done { stop_reason } => {
                println!("\n\n[Done — stop reason: {stop_reason:?}]");
                break;
            }
            Delta::Usage { input, output } => {
                println!("[Token usage — input: {input}, output: {output}]");
            }
            Delta::CacheUsage { creation, read } => {
                // Anthropic-specific; Ollama will not emit this.
                println!("[Cache — created: {creation}, read: {read}]");
            }
        }
    }

    println!("\nFull response:\n{full_response}");
    Ok(())
}
