use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
use tokio::process::Command;

use crate::registry::Tool;

/// Configuration forwarded from the main config at build time.
#[derive(Debug, Clone, Default)]
pub struct ShellConfig {
    /// Patterns blocked unconditionally (error returned if matched).
    pub denylist: Vec<String>,
    /// Patterns that flag for confirmation (signalled via Err with special prefix).
    pub confirm_required: Vec<String>,
    /// Path to append command log. `None` = no logging.
    pub log_path: Option<std::path::PathBuf>,
}

pub struct ShellTool {
    config: ShellConfig,
}

impl ShellTool {
    pub fn new(config: ShellConfig) -> Self {
        Self { config }
    }
}

impl Default for ShellTool {
    fn default() -> Self {
        Self {
            config: ShellConfig {
                denylist: default_denylist(),
                confirm_required: default_confirm_required(),
                log_path: default_log_path(),
            },
        }
    }
}

fn default_denylist() -> Vec<String> {
    vec![
        "rm -rf /".into(),
        "rm -rf ~/".into(),
        "rm -rf ~".into(),
        "dd if=".into(),
        ":(){:|:&};:".into(),
        "mkfs".into(),
        "git push --force origin main".into(),
        "git push --force origin master".into(),
        "git push -f origin main".into(),
        "git push -f origin master".into(),
    ]
}

fn default_confirm_required() -> Vec<String> {
    vec![
        "git push".into(),
        "git reset --hard".into(),
        "npm publish".into(),
        "cargo publish".into(),
        "rm -rf".into(),
    ]
}

fn default_log_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".harness").join("shell.log"))
}

#[async_trait]
impl Tool for ShellTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "shell",
            "Run a shell command and return its stdout and stderr. \
             Supports pipes, redirections, &&, ||, and all shell syntax. \
             Use for build, test, git, and other CLI operations.",
            json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute. Full sh syntax supported."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory. Defaults to current directory."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Timeout in seconds. Default 120."
                    }
                },
                "required": ["command"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<String> {
        let command = args["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing command"))?
            .trim();

        // Check hard denylist — block catastrophic commands outright.
        let cmd_lower = command.to_lowercase();
        for blocked in &self.config.denylist {
            if cmd_lower.contains(blocked.to_lowercase().as_str()) {
                return Err(anyhow::anyhow!(
                    "Command blocked by safety denylist: contains '{}'. \
                     If you genuinely need this, run it in a real terminal.",
                    blocked
                ));
            }
        }

        // Log the command before execution.
        self.log_command(command);

        let cwd = args["cwd"].as_str();
        let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(120);

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

        let exit_code = output.status.code().unwrap_or(-1);
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
            result = format!("(exit code {})", exit_code);
        } else if exit_code != 0 {
            result.push_str(&format!("\n(exit code {})", exit_code));
        }

        // Cap output to avoid flooding the context window.
        if result.len() > 32_768 {
            let head = &result[..16_384];
            let tail = &result[result.len() - 4_096..];
            result = format!(
                "{}\n... [{} bytes omitted] ...\n{}",
                head,
                result.len() - 20_480,
                tail
            );
        }

        Ok(result)
    }
}

impl ShellTool {
    fn log_command(&self, cmd: &str) {
        let Some(ref path) = self.config.log_path else {
            return;
        };
        // Best-effort: ignore errors.
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
            let _ = writeln!(f, "[{ts}] {cmd}");
        }
    }
}
