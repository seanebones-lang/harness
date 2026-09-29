//! SpawnAgentTool: runs a sub-agent with a given task and returns the result.
//! Sub-agents have access to base tools (read/write/shell/search) but cannot
//! spawn further sub-agents to prevent runaway recursion.

use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::registry::Tool;

/// Prompt plus the child agent id Deadbolt bound, when lineage is attached.
pub struct SpawnRequest {
    /// Full prompt passed to the sub-agent.
    pub prompt: String,
    /// Child lease id. `None` when Deadbolt is not attached.
    pub child_agent_id: Option<String>,
}

/// Closure type: given a spawn request, run a sub-agent and return its output.
pub type SubAgentRunner = Arc<
    dyn Fn(
            SpawnRequest,
        ) -> Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send>>
        + Send
        + Sync,
>;

/// Registers a child lease under a parent. `Ok(true)` means the child is live.
pub type ChildRegister = Arc<dyn Fn(&str, &str) -> anyhow::Result<bool> + Send + Sync>;

/// Parent/child binding applied before the sub-agent runner starts.
pub struct AgentLineage {
    /// Parent lease id. Not a vendor key.
    pub parent_agent_id: String,
    /// Out-of-band registrar. Must not be a model tool.
    pub register: ChildRegister,
}

/// Spawn a sub-agent with a subset of tools.
pub struct SpawnAgentTool {
    runner: SubAgentRunner,
    lineage: Option<AgentLineage>,
}

impl SpawnAgentTool {
    /// Create the tool with a runtime-specific sub-agent runner.
    pub fn new(runner: SubAgentRunner) -> Self {
        Self {
            runner,
            lineage: None,
        }
    }

    /// Register each spawned child under `parent` before the runner starts.
    pub fn with_lineage(mut self, lineage: AgentLineage) -> Self {
        self.lineage = Some(lineage);
        self
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
        let child_agent_id = if let Some(lineage) = &self.lineage {
            let child = mint_child_id();
            let live = (lineage.register)(&lineage.parent_agent_id, &child)?;
            if !live {
                anyhow::bail!("deadbolt:killed");
            }
            Some(child)
        } else {
            None
        };
        (self.runner)(SpawnRequest {
            prompt: full_prompt,
            child_agent_id,
        })
        .await
    }
}

fn mint_child_id() -> String {
    static N: AtomicU64 = AtomicU64::new(1);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("h-{}-{tick:x}-{n:x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    fn runner_ok() -> SubAgentRunner {
        Arc::new(|req| {
            let prompt = req.prompt;
            Box::pin(async move { Ok(format!("done:{prompt}")) })
        })
    }

    #[tokio::test]
    async fn spawn_agent_invokes_runner_with_context() {
        let tool = SpawnAgentTool::new(runner_ok());
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
        let tool = SpawnAgentTool::new(runner_ok());
        let def = tool.definition();
        assert_eq!(def.function.name, "spawn_agent");
        assert!(!harness_deadbolt::is_shutdown_tool(&def.function.name));
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
        let tool = SpawnAgentTool::new(runner_ok());
        let err = tool.execute(json!({})).await.unwrap_err();
        assert!(err.to_string().contains("missing task"));
    }

    #[tokio::test]
    async fn non_string_task_errors() {
        let tool = SpawnAgentTool::new(runner_ok());
        let err = tool
            .execute(json!({"task": 42, "context": "x"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("missing task"));
    }

    #[tokio::test]
    async fn empty_context_uses_task_only() {
        let runner: SubAgentRunner = Arc::new(|req| {
            let prompt = req.prompt;
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

        let runner2: SubAgentRunner = Arc::new(|req| {
            let prompt = req.prompt;
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
        let runner: SubAgentRunner = Arc::new(|req| {
            let prompt = req.prompt;
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
            Arc::new(|_req| Box::pin(async move { anyhow::bail!("sub-agent exploded") }));
        let tool = SpawnAgentTool::new(runner);
        let err = tool
            .execute(json!({"task": "x"}))
            .await
            .expect_err("runner failure");
        assert!(err.to_string().contains("sub-agent exploded"));
    }

    #[tokio::test]
    async fn revoked_parent_skips_runner() {
        let hits = Arc::new(AtomicUsize::new(0));
        let hits2 = hits.clone();
        let runner: SubAgentRunner = Arc::new(move |_req| {
            hits2.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok("ran".into()) })
        });
        let seen = Arc::new(Mutex::new(None));
        let seen2 = seen.clone();
        let tool = SpawnAgentTool::new(runner).with_lineage(AgentLineage {
            parent_agent_id: "parent".into(),
            register: Arc::new(move |parent, child| {
                *seen2.lock().expect("lock") = Some((parent.to_string(), child.to_string()));
                Ok(false)
            }),
        });
        let err = tool.execute(json!({"task": "x"})).await.unwrap_err();
        assert!(err.to_string().contains("deadbolt:killed"));
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(seen.lock().expect("lock").is_some());
    }

    #[tokio::test]
    async fn live_lineage_passes_child_id() {
        let runner: SubAgentRunner = Arc::new(|req| {
            let id = req.child_agent_id.unwrap_or_default();
            Box::pin(async move { Ok(id) })
        });
        let tool = SpawnAgentTool::new(runner).with_lineage(AgentLineage {
            parent_agent_id: "parent".into(),
            register: Arc::new(|_p, _c| Ok(true)),
        });
        let out = tool.execute(json!({"task": "x"})).await.unwrap();
        assert!(out.starts_with("h-"));
    }
}
