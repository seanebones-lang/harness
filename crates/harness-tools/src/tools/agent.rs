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

/// Spawn a sub-agent with a subset of tools.
pub struct SpawnAgentTool {
    runner: SubAgentRunner,
}

impl SpawnAgentTool {
    /// Create the tool with a runtime-specific sub-agent runner.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn spawn_agent_invokes_runner_with_context() {
        let runner: SubAgentRunner =
            Arc::new(move |prompt| Box::pin(async move { Ok(format!("done:{prompt}")) }));
        let tool = SpawnAgentTool::new(runner);
        let out = tool
            .execute(json!({
                "task": "summarize",
                "context": "file foo.rs"
            }))
            .await
            .expect("spawn");
        assert!(out.contains("summarize"));
        assert!(out.contains("file foo.rs"));
    }

    #[test]
    fn definition_name_and_required_task() {
        let runner: SubAgentRunner =
            Arc::new(move |_prompt| Box::pin(async move { Ok(String::new()) }));
        let tool = SpawnAgentTool::new(runner);
        let def = tool.definition();
        assert_eq!(def.function.name, "spawn_agent");
        assert!(def.function.description.contains("sub-agent"));
        let required = def.function.parameters["required"]
            .as_array()
            .expect("required");
        assert!(required.iter().any(|v| v.as_str() == Some("task")));
        assert!(def.function.parameters["properties"]["task"].is_object());
        assert!(def.function.parameters["properties"]["context"].is_object());
    }

    #[tokio::test]
    async fn missing_task_errors() {
        let runner: SubAgentRunner =
            Arc::new(move |_prompt| Box::pin(async move { Ok("should not run".into()) }));
        let tool = SpawnAgentTool::new(runner);
        let err = tool.execute(json!({})).await.unwrap_err();
        assert!(err.to_string().contains("missing task"));
    }

    #[tokio::test]
    async fn non_string_task_errors() {
        let runner: SubAgentRunner =
            Arc::new(move |_prompt| Box::pin(async move { Ok("should not run".into()) }));
        let tool = SpawnAgentTool::new(runner);
        let err = tool
            .execute(json!({"task": 42, "context": "x"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("missing task"));
    }

    #[tokio::test]
    async fn empty_context_uses_task_only() {
        let runner: SubAgentRunner = Arc::new(move |prompt| {
            Box::pin(async move {
                assert_eq!(prompt, "solo-task");
                assert!(!prompt.contains("Additional context"));
                Ok(format!("ok:{prompt}"))
            })
        });
        let tool = SpawnAgentTool::new(runner);
        let out = tool
            .execute(json!({"task": "solo-task"}))
            .await
            .expect("spawn");
        assert_eq!(out, "ok:solo-task");

        // Explicit empty context string is treated the same as absent.
        let runner2: SubAgentRunner = Arc::new(move |prompt| {
            Box::pin(async move {
                assert_eq!(prompt, "solo-task");
                Ok("ok2".into())
            })
        });
        let tool2 = SpawnAgentTool::new(runner2);
        let out2 = tool2
            .execute(json!({"task": "solo-task", "context": ""}))
            .await
            .unwrap();
        assert_eq!(out2, "ok2");
    }

    #[tokio::test]
    async fn context_is_appended_with_header() {
        let runner: SubAgentRunner = Arc::new(move |prompt| {
            Box::pin(async move {
                assert!(prompt.starts_with("do work"));
                assert!(prompt.contains("Additional context:\nconstraints"));
                Ok(prompt)
            })
        });
        let tool = SpawnAgentTool::new(runner);
        let out = tool
            .execute(json!({
                "task": "do work",
                "context": "constraints"
            }))
            .await
            .unwrap();
        assert_eq!(out, "do work\n\nAdditional context:\nconstraints");
    }

    #[tokio::test]
    async fn runner_error_propagates() {
        let runner: SubAgentRunner =
            Arc::new(move |_prompt| Box::pin(async move { anyhow::bail!("sub-agent exploded") }));
        let tool = SpawnAgentTool::new(runner);
        let err = tool
            .execute(json!({"task": "x"}))
            .await
            .expect_err("runner failure");
        assert!(err.to_string().contains("sub-agent exploded"));
    }
}
