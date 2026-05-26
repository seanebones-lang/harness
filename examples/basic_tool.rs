//! Custom tool implementation example.
//!
//! Shows how to implement the `Tool` trait from `harness-tools`, define its
//! JSON Schema via `ToolDefinition::new`, and call the tool directly.
//!
//! # Cargo.toml snippet
//!
//! ```toml
//! [dependencies]
//! harness-tools = { path = "../crates/harness-tools" }
//! harness-provider-core = { path = "../crates/harness-provider-core" }
//! tokio = { version = "1", features = ["full"] }
//! anyhow = "1"
//! serde_json = "1"
//! async-trait = "0.1"
//! ```

use anyhow::Result;
use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use harness_tools::registry::Tool;
use serde_json::{json, Value};

// ── Custom tool ───────────────────────────────────────────────────────────────

/// Returns the current UTC time. The LLM calls this when it needs the time.
pub struct TimestampTool;

#[async_trait]
impl Tool for TimestampTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "get_timestamp",
            "Returns the current UTC time in RFC 3339 or as Unix milliseconds.",
            json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "enum": ["rfc3339", "unix_ms"],
                        "description": "'rfc3339' for ISO 8601, 'unix_ms' for epoch milliseconds."
                    }
                },
                "required": []
            }),
        )
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let format = args
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("rfc3339");
        let now = std::time::SystemTime::now();
        match format {
            "unix_ms" => {
                let ms = now
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                Ok(ms.to_string())
            }
            _ => {
                let secs = now
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                Ok(format!(
                    "Unix epoch seconds: {secs} (use chrono for full RFC 3339)"
                ))
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let tool = TimestampTool;

    let def = tool.definition();
    println!("Tool: {} — {}", def.function.name, def.function.description);

    let result = tool.execute(json!({})).await?;
    println!("get_timestamp (default): {result}");

    let result_ms = tool.execute(json!({"format": "unix_ms"})).await?;
    println!("get_timestamp (unix_ms): {result_ms}");

    Ok(())
}
