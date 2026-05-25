//! Custom tool implementation example.
//!
//! Shows how to implement the `Tool` trait from `harness-tools` for a custom
//! tool, define its JSON Schema for arguments, and register it alongside the
//! built-in tools via `build_tools()`.
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
use harness_tools::{Tool, ToolDefinition};
use serde_json::{Value, json};

// ── Custom tool implementation ────────────────────────────────────────────────

/// A simple tool that returns the current UTC timestamp formatted as RFC 3339.
/// The LLM can call this tool whenever it needs to know the current time.
pub struct TimestampTool;

#[async_trait]
impl Tool for TimestampTool {
    /// Return the tool's JSON Schema definition. This is sent to the LLM provider
    /// so the model knows the tool's name, description, and argument structure.
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_timestamp".to_string(),
            description: "Returns the current UTC date and time in RFC 3339 format. \
                          Use this when you need to know what time it is right now."
                .to_string(),
            // JSON Schema for the input arguments. This tool takes an optional
            // `format` field ("rfc3339" | "unix_ms") defaulting to "rfc3339".
            parameters: json!({
                "type": "object",
                "properties": {
                    "format": {
                        "type": "string",
                        "enum": ["rfc3339", "unix_ms"],
                        "description": "Output format. 'rfc3339' for human-readable ISO 8601, 'unix_ms' for milliseconds since epoch."
                    }
                },
                "required": []
            }),
        }
    }

    /// Execute the tool with the JSON arguments provided by the LLM.
    /// The return value is a UTF-8 string sent back to the model as the tool result.
    async fn execute(&self, args: Value) -> Result<String> {
        let format = args
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("rfc3339");

        let now = std::time::SystemTime::now();

        match format {
            "unix_ms" => {
                let millis = now
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0);
                Ok(millis.to_string())
            }
            _ => {
                // RFC 3339 using a simple manual formatting (avoids chrono dependency).
                let secs = now
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                // Rough ISO 8601 from seconds — in production use the `time` or `chrono` crate.
                Ok(format!("Unix seconds: {secs} (add chrono/time crate for full RFC 3339 formatting)"))
            }
        }
    }
}

// ── Registering the tool ──────────────────────────────────────────────────────

/// Build the full tool set: built-in tools plus the custom TimestampTool.
///
/// In a real harness integration, you would call `harness::cli::wiring::build_tools(config)`
/// and then `.push(Arc::new(TimestampTool))` to extend the default set. This example
/// shows the pattern without importing the full binary crate.
fn build_tools_with_custom() -> Vec<Box<dyn Tool>> {
    // In production: let mut tools = harness_tools::build_default_tools(&config);
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();

    // Add the custom tool.
    tools.push(Box::new(TimestampTool));

    tools
}

#[tokio::main]
async fn main() -> Result<()> {
    let tools = build_tools_with_custom();

    println!("Registered tools:");
    for tool in &tools {
        let def = tool.definition();
        println!("  {} — {}", def.name, def.description);
    }

    // Exercise the tool directly (as the agent executor would).
    let timestamp_tool = TimestampTool;

    let result_default = timestamp_tool.execute(json!({})).await?;
    println!("\nget_timestamp (default): {result_default}");

    let result_unix = timestamp_tool.execute(json!({"format": "unix_ms"})).await?;
    println!("get_timestamp (unix_ms): {result_unix}");

    Ok(())
}
