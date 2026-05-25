//! Structured output example.
//!
//! Shows how to use `ResponseSchema` with `drive_agent_with_schema` to get
//! strongly-typed JSON output from a provider. The agent is forced to respond
//! with a JSON object that matches the provided schema — no free-form text.
//!
//! Anthropic achieves this by injecting a synthetic tool (`respond_<schema-name>`)
//! and forcing the model to call it. OpenAI and xAI use `response_format` with
//! `json_schema` strict mode.
//!
//! # Cargo.toml snippet
//!
//! ```toml
//! [dependencies]
//! harness-provider-core = { path = "../crates/harness-provider-core" }
//! harness-provider-anthropic = { path = "../crates/harness-provider-anthropic" }
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
use harness_provider_core::{ChatRequest, Message, ResponseSchema, Role};
use harness_provider_anthropic::AnthropicProvider;
use serde::Deserialize;
use serde_json::json;

// ── Target output schema ──────────────────────────────────────────────────────

/// The Rust type we expect the model to produce. `serde::Deserialize` lets us
/// parse the model's JSON response directly into this struct.
#[derive(Debug, Deserialize)]
struct CodeReview {
    /// One-line summary of what the code does.
    summary: String,
    /// List of specific issues found (empty if the code is clean).
    issues: Vec<Issue>,
    /// Overall quality score from 1 (poor) to 10 (excellent).
    quality_score: u8,
    /// Whether the code is ready to merge as-is.
    approve: bool,
}

#[derive(Debug, Deserialize)]
struct Issue {
    severity: String,   // "error" | "warning" | "info"
    line: Option<u32>,
    description: String,
}

// ── JSON Schema definition ────────────────────────────────────────────────────

/// Build a `ResponseSchema` that constrains the model's output to `CodeReview`.
/// The schema is JSON Schema Draft 7 compatible.
fn code_review_schema() -> ResponseSchema {
    ResponseSchema {
        name: "code_review".to_string(),
        strict: true,
        schema: json!({
            "type": "object",
            "properties": {
                "summary": {
                    "type": "string",
                    "description": "One-line summary of what the code does."
                },
                "issues": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "severity": {
                                "type": "string",
                                "enum": ["error", "warning", "info"]
                            },
                            "line": {
                                "type": ["integer", "null"],
                                "description": "Source line number, if applicable."
                            },
                            "description": { "type": "string" }
                        },
                        "required": ["severity", "description"],
                        "additionalProperties": false
                    }
                },
                "quality_score": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10
                },
                "approve": {
                    "type": "boolean",
                    "description": "True if the code is ready to merge."
                }
            },
            "required": ["summary", "issues", "quality_score", "approve"],
            "additionalProperties": false
        }),
    }
}

// ── Agent entry point ─────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY not set"))?;

    let provider = AnthropicProvider::new(api_key, "claude-sonnet-4-6");

    let code_snippet = r#"
fn divide(a: f64, b: f64) -> f64 {
    a / b
}
"#;

    // Build a ChatRequest with the ResponseSchema attached. When the provider
    // receives this request it will constrain the model's output to the schema.
    let schema = code_review_schema();
    let request = ChatRequest::builder()
        .system("You are a code reviewer. Analyse the code and respond using the provided schema.")
        .message(Message {
            role: Role::User,
            content: format!("Review this Rust function:\n```rust\n{code_snippet}\n```"),
            tool_call_id: None,
        })
        .response_schema(schema)
        .build();

    println!("Sending structured-output request...\n");

    // drive_agent_with_schema drives a single-turn agent that returns the
    // structured JSON string produced by the model.
    //
    // In production you would use:
    //   harness::agent::drive_agent_with_schema(provider, tools, session, system, schema).await?
    //
    // For this standalone example we call stream_chat directly and collect text:
    use futures::StreamExt;
    use harness_provider_core::Delta;

    let mut stream = provider.stream_chat(request).await;
    let mut json_output = String::new();

    while let Some(delta) = stream.next().await {
        match delta? {
            Delta::Text(chunk) => json_output.push_str(&chunk),
            Delta::Done { .. } => break,
            _ => {}
        }
    }

    println!("Raw JSON from model:\n{json_output}\n");

    // Parse the JSON into our typed struct.
    let review: CodeReview = serde_json::from_str(&json_output)
        .map_err(|e| anyhow::anyhow!("Failed to parse model output as CodeReview: {e}\nRaw: {json_output}"))?;

    println!("Parsed CodeReview:");
    println!("  Summary:       {}", review.summary);
    println!("  Quality score: {}/10", review.quality_score);
    println!("  Approve:       {}", review.approve);

    if review.issues.is_empty() {
        println!("  Issues:        none");
    } else {
        println!("  Issues:");
        for issue in &review.issues {
            let line = issue.line.map(|l| format!("line {l}")).unwrap_or_default();
            println!("    [{:7}] {} {}", issue.severity, line, issue.description);
        }
    }

    Ok(())
}
