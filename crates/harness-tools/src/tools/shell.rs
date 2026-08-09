use async_trait::async_trait;
use harness_provider_core::ToolDefinition;
use serde_json::{json, Value};
#[cfg(windows)]
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;

use crate::registry::Tool;
use crate::workspace_root::WorkspaceRoot;

/// On Windows, prefer Git Bash `sh`/`bash`, then PowerShell, then `cmd.exe /C`.
#[cfg(windows)]
fn windows_shell_interpreter() -> (PathBuf, ShellMode) {
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            for name in ["sh.exe", "bash.exe"] {
                let p = dir.join(name);
                if p.is_file() {
                    return (p, ShellMode::PosixSh);
                }
            }
        }
    }
    for name in ["pwsh.exe", "powershell.exe"] {
        if let Ok(output) = std::process::Command::new(name)
            .arg("-NoProfile")
            .arg("-Command")
            .arg("exit 0")
            .output()
        {
            if output.status.success() {
                return (PathBuf::from(name), ShellMode::PowerShell);
            }
        }
    }
    (PathBuf::from("cmd.exe"), ShellMode::Cmd)
}

#[cfg(windows)]
enum ShellMode {
    PosixSh,
    PowerShell,
    Cmd,
}

/// Configuration forwarded from the main config at build time.
#[derive(Debug, Clone)]
pub struct ShellConfig {
    /// Patterns blocked unconditionally (error returned if matched).
    pub denylist: Vec<String>,
    /// Patterns that flag for confirmation (signalled via Err with special prefix).
    pub confirm_required: Vec<String>,
    /// Path to append command log. `None` = no logging.
    pub log_path: Option<std::path::PathBuf>,
    /// When non-empty, only these absolute argv0 paths (first shell token) are allowed.
    pub cmd_allowlist: Option<Vec<String>>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            denylist: default_denylist(),
            confirm_required: default_confirm_required(),
            log_path: default_log_path(),
            cmd_allowlist: None,
        }
    }
}

/// Shell command execution tool.
pub struct ShellTool {
    config: ShellConfig,
    workspace: Arc<WorkspaceRoot>,
}

impl ShellTool {
    /// Create a shell tool with safety config and workspace sandbox.
    pub fn new(config: ShellConfig, workspace: Arc<WorkspaceRoot>) -> Self {
        Self { config, workspace }
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
             On Unix and Git Bash on Windows, full sh syntax is supported. \
             On native Windows without Git Bash, PowerShell or cmd.exe is used. \
             Use for build, test, git, and other CLI operations.",
            json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to execute. POSIX sh on Unix/Git Bash; PowerShell/cmd on native Windows."
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

        // Patterns that require explicit user approval (use --plan in TUI or run manually).
        for pattern in &self.config.confirm_required {
            if cmd_lower.contains(pattern.to_lowercase().as_str()) {
                return Err(anyhow::anyhow!(
                    "Command requires confirmation (matches '{}'): {}. \
                     Re-run with --plan to approve interactively, or execute manually.",
                    pattern,
                    command
                ));
            }
        }

        if let Some(allow) = self.config.cmd_allowlist.as_ref().filter(|a| !a.is_empty()) {
            if let Some(tok) = command.split_whitespace().next() {
                if tok.starts_with('/') && !allow.iter().any(|a| a.as_str() == tok) {
                    return Err(anyhow::anyhow!(
                        "absolute-path command `{}` is not in [shell].cmd_allowlist",
                        tok
                    ));
                }
            }
        }

        // Log the command before execution.
        self.log_command(command);

        let cwd = args["cwd"].as_str();
        let timeout_secs = args["timeout_secs"].as_u64().unwrap_or(120);

        let effective_cwd = if let Some(dir) = cwd {
            self.workspace.resolve(dir)?
        } else {
            self.workspace.root().to_path_buf()
        };

        let mut cmd = {
            #[cfg(windows)]
            {
                let (prog, mode) = windows_shell_interpreter();
                let mut c = Command::new(prog);
                match mode {
                    ShellMode::PosixSh => {
                        c.arg("-c").arg(command);
                    }
                    ShellMode::PowerShell => {
                        c.arg("-NoProfile").arg("-Command").arg(command);
                    }
                    ShellMode::Cmd => {
                        c.arg("/C").arg(command);
                    }
                }
                c
            }
            #[cfg(not(windows))]
            {
                let mut c = Command::new("sh");
                c.arg("-c").arg(command);
                c
            }
        };
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        cmd.current_dir(&effective_cwd);

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
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }
            let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
            let _ = writeln!(f, "[{ts}] {cmd}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Tool;
    use crate::workspace_root::{SandboxMode, WorkspaceRoot};
    use serde_json::json;
    use std::sync::Arc;

    struct ShellFixture {
        _dir: tempfile::TempDir,
        tool: ShellTool,
    }

    fn tool_with(cfg: ShellConfig) -> ShellFixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = WorkspaceRoot::new(dir.path().to_path_buf(), SandboxMode::Strict).unwrap();
        ShellFixture {
            tool: ShellTool::new(cfg, Arc::new(ws)),
            _dir: dir,
        }
    }

    #[test]
    fn shell_config_defaults_include_denylist() {
        let cfg = ShellConfig::default();
        assert!(!cfg.denylist.is_empty());
        assert!(cfg.denylist.iter().any(|p| p.contains("rm -rf /")));
        assert!(cfg.confirm_required.iter().any(|p| p == "git push"));
    }

    #[tokio::test]
    async fn missing_command_errors() {
        let f = tool_with(ShellConfig {
            denylist: vec![],
            confirm_required: vec![],
            log_path: None,
            cmd_allowlist: None,
        });
        let err = f.tool.execute(json!({})).await.unwrap_err();
        assert!(err.to_string().contains("missing command"));
    }

    #[tokio::test]
    async fn denylist_blocks_catastrophic_commands() {
        let f = tool_with(ShellConfig {
            denylist: vec!["rm -rf /".into()],
            confirm_required: vec![],
            log_path: None,
            cmd_allowlist: None,
        });
        let err = f
            .tool
            .execute(json!({"command": "sudo rm -rf /"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("denylist"));
    }

    #[tokio::test]
    async fn confirm_required_blocks_without_running() {
        let f = tool_with(ShellConfig {
            denylist: vec![],
            confirm_required: vec!["git push".into()],
            log_path: None,
            cmd_allowlist: None,
        });
        let err = f
            .tool
            .execute(json!({"command": "git push origin main"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("requires confirmation"));
    }

    #[tokio::test]
    async fn cmd_allowlist_rejects_unknown_absolute_path() {
        let f = tool_with(ShellConfig {
            denylist: vec![],
            confirm_required: vec![],
            log_path: None,
            cmd_allowlist: Some(vec!["/usr/bin/safe".into()]),
        });
        let err = f
            .tool
            .execute(json!({"command": "/bin/evil -c 'x'"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("cmd_allowlist"));
    }

    #[tokio::test]
    async fn safe_echo_runs() {
        let f = tool_with(ShellConfig {
            denylist: vec![],
            confirm_required: vec![],
            log_path: None,
            cmd_allowlist: None,
        });
        let out = f
            .tool
            .execute(json!({"command": "echo hello-harness", "timeout_secs": 5}))
            .await
            .expect("echo");
        assert!(out.contains("hello-harness"), "got: {out}");
    }

    #[test]
    fn definition_name_is_shell() {
        let f = tool_with(ShellConfig {
            denylist: vec![],
            confirm_required: vec![],
            log_path: None,
            cmd_allowlist: None,
        });
        assert_eq!(f.tool.definition().function.name, "shell");
        assert!(f.tool.definition().function.description.contains("shell"));
    }

    #[tokio::test]
    async fn denylist_is_case_insensitive_and_substring() {
        let f = tool_with(ShellConfig {
            denylist: vec!["MKFS".into()],
            confirm_required: vec![],
            log_path: None,
            cmd_allowlist: None,
        });
        let err = f
            .tool
            .execute(json!({"command": "sudo mkfs.ext4 /dev/sda"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("denylist"));
    }

    #[tokio::test]
    async fn confirm_required_rm_rf_blocks_without_spawn() {
        let f = tool_with(ShellConfig {
            denylist: vec![],
            confirm_required: default_confirm_required(),
            log_path: None,
            cmd_allowlist: None,
        });
        let err = f
            .tool
            .execute(json!({"command": "rm -rf ./target"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("requires confirmation"));
    }

    #[tokio::test]
    async fn empty_cmd_allowlist_does_not_block() {
        // Some([]) is treated as unset (filter !a.is_empty())
        let f = tool_with(ShellConfig {
            denylist: vec![],
            confirm_required: vec![],
            log_path: None,
            cmd_allowlist: Some(vec![]),
        });
        let out = f
            .tool
            .execute(json!({"command": "echo allow-empty", "timeout_secs": 5}))
            .await
            .expect("run");
        assert!(out.contains("allow-empty"), "got: {out}");
    }

    #[tokio::test]
    async fn non_string_command_errors() {
        let f = tool_with(ShellConfig {
            denylist: vec![],
            confirm_required: vec![],
            log_path: None,
            cmd_allowlist: None,
        });
        let err = f.tool.execute(json!({"command": 123})).await.unwrap_err();
        assert!(err.to_string().contains("missing command"));
    }

    #[test]
    fn default_denylist_and_confirm_patterns() {
        let d = default_denylist();
        assert!(d.iter().any(|p| p.contains("rm -rf /")));
        assert!(d.iter().any(|p| p.contains("mkfs")));
        let c = default_confirm_required();
        assert!(c.iter().any(|p| p == "git push"));
        assert!(c.iter().any(|p| p == "rm -rf"));
    }
}
