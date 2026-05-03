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

pub struct SpawnSwarmTool {
    runner: SwarmEnqueueRunner,
}

impl SpawnSwarmTool {
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
