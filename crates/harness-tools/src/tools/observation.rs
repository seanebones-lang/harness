//! read_obs / grep_obs tools for paging and searching packed observations.
use crate::observation::{ObservationHandle, ObservationStore};
use crate::registry::Tool;
use harness_provider_core::ToolDefinition;
use serde_json::Value;
use std::sync::Arc;

/// Tool for reading observation ranges.
pub struct ReadObsTool {
    /// Observation store to read from.
    pub store: Arc<ObservationStore>,
}

#[async_trait::async_trait]
impl Tool for ReadObsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "read_obs",
            "Page an observation by handle and line range",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "handle": { "type": "string", "description": "obs:// handle from a prior tool result" },
                    "start_line": { "type": "integer", "minimum": 0, "default": 0 },
                    "end_line": { "type": "integer", "minimum": 1 }
                },
                "required": ["handle"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let handle_str = args.get("handle").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("missing handle"))?;
        let handle = ObservationHandle::from_uri(handle_str).ok_or_else(|| anyhow::anyhow!("invalid handle"))?;
        let start = args.get("start_line").and_then(Value::as_u64).unwrap_or(0) as usize;
        let end = args.get("end_line").and_then(Value::as_u64).map(|v| v as usize);
        self.store.read_range(&handle, start, end).await
    }
}

/// Tool for regex search within observations.
pub struct GrepObsTool {
    /// Observation store to search.
    pub store: Arc<ObservationStore>,
}

#[async_trait::async_trait]
impl Tool for GrepObsTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "grep_obs",
            "Regex search within an observation handle",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "handle": { "type": "string" },
                    "pattern": { "type": "string" }
                },
                "required": ["handle", "pattern"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let handle_str = args.get("handle").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("missing handle"))?;
        let handle = ObservationHandle::from_uri(handle_str).ok_or_else(|| anyhow::anyhow!("invalid handle"))?;
        let pattern = args.get("pattern").and_then(Value::as_str).ok_or_else(|| anyhow::anyhow!("missing pattern"))?;
        let matches = self.store.grep(&handle, pattern).await?;
        if matches.is_empty() {
            Ok("(no matches)".into())
        } else {
            Ok(matches.into_iter().map(|(n, l)| format!("{}:{}", n, l)).collect::<Vec<_>>().join("\n"))
        }
    }
}
