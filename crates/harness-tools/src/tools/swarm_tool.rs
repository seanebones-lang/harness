//! `spawn_swarm` — queue parallel background agent tasks (semaphore-limited).

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::registry::Tool;

/// Enqueue one or more swarm tasks; returns a short summary (task ids).
pub type SwarmEnqueueRunner = Arc<
    dyn Fn(String, usize) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send>>
        + Send
        + Sync,
>;

/// Parallel swarm task enqueue tool.
pub struct SpawnSwarmTool {
    runner: SwarmEnqueueRunner,
}

impl SpawnSwarmTool {
    /// Create the tool with a runtime-specific enqueue callback.
    pub fn new(runner: SwarmEnqueueRunner) -> Self {
        Self { runner }
    }
}

#[async_trait]
impl Tool for SpawnSwarmTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "spawn_swarm",
            "Queue parallel agent tasks tracked in the swarm registry. \
             Tasks run in the background under a concurrency limit and complete asynchronously. \
             Returns task id(s); use `harness swarm status <id>` and `harness swarm result <id>`. \
             Prefer `spawn_agent` when you need the reply inline in the same turn.",
            json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "Task description for each swarm worker."
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of parallel tasks (default 1, max 32)."
                    }
                },
                "required": ["prompt"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let prompt = args["prompt"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing prompt"))?
            .to_string();
        let count = args["count"].as_u64().unwrap_or(1).clamp(1, 32) as usize;
        (self.runner)(prompt, count).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Tool;
    use serde_json::json;
    use std::sync::Arc;

    fn tool_capturing() -> SpawnSwarmTool {
        SpawnSwarmTool::new(Arc::new(|prompt, count| {
            Box::pin(async move { Ok(format!("{count}:{prompt}")) })
        }))
    }

    #[tokio::test]
    async fn missing_prompt_errors() {
        let t = tool_capturing();
        let err = t.execute(json!({})).await.unwrap_err();
        assert!(err.to_string().contains("missing prompt"));
    }

    #[tokio::test]
    async fn non_string_prompt_errors() {
        let t = tool_capturing();
        let err = t.execute(json!({"prompt": 42})).await.unwrap_err();
        assert!(err.to_string().contains("missing prompt"));
    }

    #[tokio::test]
    async fn default_count_is_one() {
        let t = tool_capturing();
        let out = t.execute(json!({"prompt": "do work"})).await.expect("ok");
        assert_eq!(out, "1:do work");
    }

    #[tokio::test]
    async fn count_clamps_to_1_32() {
        let t = tool_capturing();
        let low = t
            .execute(json!({"prompt": "p", "count": 0}))
            .await
            .expect("ok");
        assert_eq!(low, "1:p");
        let high = t
            .execute(json!({"prompt": "p", "count": 99}))
            .await
            .expect("ok");
        assert_eq!(high, "32:p");
        let mid = t
            .execute(json!({"prompt": "p", "count": 5}))
            .await
            .expect("ok");
        assert_eq!(mid, "5:p");
    }

    #[test]
    fn definition_names_spawn_swarm() {
        let t = tool_capturing();
        let d = t.definition();
        assert_eq!(d.function.name, "spawn_swarm");
        assert!(d.function.description.contains("swarm"));
    }
}
