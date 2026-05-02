//! SpawnAgentTool: runs a sub-agent with a given task and returns the result.
//! Sub-agents have access to base tools (read/write/shell/search) but cannot
//! spawn further sub-agents to prevent runaway recursion.

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::registry::Tool;

/// Closure type: given a task string, run a sub-agent and return its output.
pub type SubAgentRunner = Arc<
    dyn Fn(
            String,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>>
        + Send
        + Sync,
>;

pub struct SpawnAgentTool {
    runner: SubAgentRunner,
}

impl SpawnAgentTool {
    pub fn new(runner: SubAgentRunner) -> Self {
        Self { runner }
    }
}

#[async_trait]
impl Tool for SpawnAgentTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "spawn_agent",
            "Spawn a sub-agent to complete a self-contained task in parallel. \
             The sub-agent has full tool access (file I/O, shell, code search). \
             Returns the sub-agent's final response.",
            json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "Clear, self-contained task for the sub-agent. Include all context it needs."
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional: extra context or constraints for the sub-agent."
                    }
                },
                "required": ["task"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let task = args["task"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing task"))?
            .to_string();
        let context = args["context"].as_str().unwrap_or("").to_string();
        let full_prompt = if context.is_empty() {
            task.clone()
        } else {
            format!("{task}\n\nAdditional context:\n{context}")
        };
        (self.runner)(full_prompt).await
    }
}
