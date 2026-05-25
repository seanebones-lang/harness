//! Structured output example.
//!
//! Shows how to use `ResponseSchema` with a `ChatRequest` to get strongly-typed
//! JSON output from Anthropic. The model is constrained to respond only with
//! JSON matching the provided schema.
//!
//! # Cargo.toml snippet
//!
//! ```toml
//! [dependencies]
//! harness-provider-core = { path = "../crates/harness-provider-core" }
//! harness-provider-anthropic = { path = "../crates/harness-provider-anthropic" }
//! futures = "0.3"
//! tokio = { version = "1", features = ["full"] }
//! anyhow = "1"
//! serde = { version = "1", features = ["derive"] }
//! serde_json = "1"
//! ```
//!
//! # Running
//!
//! ```bash
//! ANTHROPIC_API_KEY=sk-ant-... cargo run --example structured_output
//! ```

use anyhow::Result;
use futures::StreamExt;
use harness_provider_anthropic::{AnthropicConfig, AnthropicProvider};
use harness_provider_core::{ChatRequest, Delta, Message, MessageContent, ResponseSchema, Role};
use serde::Deserialize;
use serde_json::json;

// ── Output schema ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CodeReview {
    summary: String,
    issues: Vec<Issue>,
    quality_score: u8,
    approve: bool,
}

#[derive(Debug, Deserialize)]
struct Issue {
    severity: String,
    line: Option<u32>,
    description: String,
}

fn code_review_schema() -> ResponseSchema {
    ResponseSchema::new(
        "code_review",
        json!({
            "type": "object",
            "properties": {
                "summary":       { "type": "string" },
                "issues": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "severity":    { "type": "string", "enum": ["error", "warning", "info"] },
                            "line":        { "type": ["integer", "null"] },
                            "description": { "type": "string" }
                        },
                        "required": ["severity", "description"],
                        "additionalProperties": false
                    }
                },
                "quality_score": { "type": "integer", "minimum": 1, "maximum": 10 },
                "approve":       { "type": "boolean" }
            },
            "required": ["summary", "issues", "quality_score", "approve"],
            "additionalProperties": false
        }),
    )
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;

    let provider = AnthropicProvider::new(AnthropicConfig::new(api_key))?;

    let code_snippet = r#"fn divide(a: f64, b: f64) -> f64 { a / b }"#;

    let request = ChatRequest::new("claude-sonnet-4-6")
        .with_messages(vec![Message {
            role: Role::User,
            content: MessageContent::Text(format!(
                "Review this Rust function:\n```rust\n{code_snippet}\n```"
            )),
            tool_call_id: None,
        }])
        .with_response_schema(code_review_schema());

    println!("Sending structured-output request…\n");

    use harness_provider_core::Provider;
    let mut stream = provider.stream_chat(request).await?;
    let mut json_output = String::new();

    while let Some(delta) = stream.next().await {
        match delta? {
            Delta::Text(chunk) => json_output.push_str(&chunk),
            Delta::Done { .. } => break,
            _ => {}
        }
    }

    println!("Raw JSON:\n{json_output}\n");

    let review: CodeReview = serde_json::from_str(&json_output)
        .map_err(|e| anyhow::anyhow!("Failed to parse CodeReview: {e}\nRaw: {json_output}"))?;

    println!("Summary:       {}", review.summary);
    println!("Quality score: {}/10", review.quality_score);
    println!("Approve:       {}", review.approve);
    for issue in &review.issues {
        let line = issue.line.map(|l| format!(" line {l}")).unwrap_or_default();
        println!("  [{:7}]{line} {}", issue.severity, issue.description);
    }

    Ok(())
}
