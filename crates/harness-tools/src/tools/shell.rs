use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::registry::Tool;

pub struct ShellTool;

#[async_trait]
impl Tool for ShellTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "shell",
            "Run a shell command and return its stdout and stderr. Use for build, test, git, and other CLI operations.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute." },
                    "cwd": { "type": "string", "description": "Working directory. Defaults to current directory." },
                    "timeout_secs": { "type": "integer", "description": "Timeout in seconds. Default 60." }
                },
                "required": ["command"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let command = args["command"].as_str().ok_or_else(|| anyhow::anyhow!("missing command"))?;
        let cwd = args["cwd"].as_str();
        let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(60);

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let child = cmd.spawn()?;

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("command timed out after {timeout_secs}s"))??;

        let mut result = String::new();
        if !output.stdout.is_empty() {
            result.push_str(&String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n[stderr]\n");
            }
            result.push_str(&String::from_utf8_lossy(&output.stderr));
        }
        if result.is_empty() {
            result = format!("(exit code {})", output.status.code().unwrap_or(-1));
        }

        // Cap output to avoid flooding the context
        if result.len() > 16_384 {
            result.truncate(16_384);
            result.push_str("\n... (truncated)");
        }

        Ok(result)
    }
}
